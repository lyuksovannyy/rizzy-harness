//! Which agents of a workspace may use which skill or MCP server
//! (ADOPTION_PLAN X.34 step A, PLAN.md T80), on the master: what nodes
//! announce, who may choose, and that every spec the master sends for a
//! chat carries exactly the deny lists of that chat's workspace for that
//! agent (a sub-agent its own; an alias, a deleted and remade agent, a node
//! that does not enforce them, another workspace). The nodes are fakes the
//! test drives, so an announcement can be forged at will.

mod common;
use common::{Login as _, SameOrigin as _};

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::agent::AgentSpec;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, PromptOpts, router};
use harness_node::NodeLink;
use harness_protocol::{
    AnnouncedSkill, CAP_PLUGIN_ACCESS, MasterToNode, NodePlugins, NodeToMaster, PROTOCOL_VERSION,
};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

type Seen = Arc<Mutex<Vec<MasterToNode>>>;

/// A fake node: acks every command, mirrors a `session_created` for each
/// chat it is asked to create, and records every command.
struct Fake {
    tx: mpsc::Sender<NodeToMaster>,
    seen: Seen,
}

fn fake_node(node_id: &str, caps: &[&str]) -> (NodeLink, Fake) {
    let (to_node, mut commands) = mpsc::channel::<MasterToNode>(1024);
    let (to_master, from_node) = mpsc::channel::<NodeToMaster>(1024);
    to_master
        .try_send(NodeToMaster::Hello {
            protocol_version: PROTOCOL_VERSION,
            node_id: node_id.into(),
            hostname: "h".into(),
            version: "0".into(),
            capabilities: caps.iter().map(|c| c.to_string()).collect(),
            sessions: vec![],
        })
        .unwrap();
    let fake = Fake {
        tx: to_master.clone(),
        seen: Arc::default(),
    };
    let seen = fake.seen.clone();
    let node = node_id.to_string();
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            if let MasterToNode::Create {
                session_id, title, ..
            } = &msg
            {
                let payload = json!({"title": title, "cwd": "/", "model": "m",
                    "backend": "native", "node_id": node, "profile": "chat",
                    "approval_mode": "ask"});
                let event = RawEvent {
                    session_id: *session_id,
                    seq: 1,
                    ts_ms: 1,
                    kind: "session_created".into(),
                    payload,
                };
                to_master.try_send(NodeToMaster::Event { event }).unwrap();
            }
            let req_id = msg.req_id().map(str::to_string);
            seen.lock().unwrap().push(msg);
            if let Some(req_id) = req_id {
                let ack = NodeToMaster::Ack {
                    req_id,
                    ok: true,
                    error: None,
                };
                to_master.try_send(ack).unwrap();
            }
        }
    });
    (NodeLink { to_node, from_node }, fake)
}

impl Fake {
    /// The agent of the newest prompt.
    fn last_agent(&self) -> AgentSpec {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|m| match m {
                MasterToNode::Prompt { agent, .. } => agent.clone(),
                _ => None,
            })
            .expect("a prompt")
    }

    async fn announce(&self, skills: &[(&str, &str)], servers: &[&str]) {
        self.tx
            .send(NodeToMaster::Plugins {
                plugins: NodePlugins {
                    skills: skills
                        .iter()
                        .map(|(n, d)| AnnouncedSkill {
                            name: n.to_string(),
                            description: d.to_string(),
                        })
                        .collect(),
                    mcp_servers: servers.iter().map(|s| s.to_string()).collect(),
                    left_out: 0,
                },
            })
            .await
            .unwrap();
    }
}

struct T {
    base: String,
    master: Master,
    node: Fake,
    /// The master's database, which a test can break under it.
    db: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

/// A master whose (embedded, chat-profile) node announces `caps`.
async fn start_with(caps: &[&str]) -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "X").unwrap();
    let db = dir.path().join("master.db");
    let master = Master::new(
        Store::open(&db, harness_store::Durability::Full).unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        }),
    );
    let mut all = vec!["profile:chat"];
    all.extend_from_slice(caps);
    let (link, node) = fake_node("master", &all);
    master.add_link(link, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        master,
        node,
        db,
        _dir: dir,
    }
}

async fn start() -> T {
    start_with(&[CAP_PLUGIN_ACCESS]).await
}

async fn req(
    t: &T,
    method: Method,
    path: &str,
    body: Option<Value>,
    c: &str,
) -> (StatusCode, Value) {
    let mut rb = reqwest::Client::new().request(method, format!("{}{path}", t.base));
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.login(c).same_origin().send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn post_anon(t: &T, path: &str, body: Value, c: Option<&str>) -> reqwest::Response {
    let mut rb = reqwest::Client::new()
        .post(format!("{}{path}", t.base))
        .json(&body);
    if let Some(c) = c {
        rb = rb.login(c).same_origin();
    }
    rb.send().await.unwrap()
}

/// The admin "alex" and a member "bob".
async fn users(t: &T) -> [String; 2] {
    let alex = common::signed_in(
        post_anon(
            t,
            "/api/setup",
            json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}),
            None,
        )
        .await,
    )
    .await;
    let pw = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob"})),
        &alex,
    )
    .await
    .1["password"]
        .as_str()
        .unwrap()
        .to_string();
    let first = common::signed_in(
        post_anon(
            t,
            "/api/login",
            json!({"username": "bob", "password": pw}),
            None,
        )
        .await,
    )
    .await;
    let bob = common::signed_in(
        post_anon(
            t,
            "/api/me/password",
            json!({"current": pw, "new": "bobs own password"}),
            Some(&first),
        )
        .await,
    )
    .await;
    [alex, bob]
}

/// A team workspace of `owner`'s with bob in it as a member.
async fn team(t: &T, owner: &str, name: &str) -> String {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/workspaces",
        Some(json!({ "name": name })),
        owner,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let ws = v["workspace_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        t,
        Method::POST,
        &format!("/api/workspaces/{ws}/members"),
        Some(json!({ "username": "bob", "role": "member" })),
        owner,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    ws
}

async fn set(t: &T, c: &str, ws: &str, kind: &str, name: &str, body: Value) -> (StatusCode, Value) {
    req(
        t,
        Method::PUT,
        &format!("/api/workspaces/{ws}/plugins/{kind}/{name}"),
        Some(body),
        c,
    )
    .await
}

async fn assign(t: &T, c: &str, ws: &str, kind: &str, name: &str, agents: &[&str]) {
    let (st, v) = set(
        t,
        c,
        ws,
        kind,
        name,
        json!({"access": "assigned", "agents": agents}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

async fn chat(t: &T, c: &str, ws: &str) -> SessionId {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "workspace_id": ws})),
        c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid: SessionId = v["session_id"].as_str().unwrap().parse().unwrap();
    for _ in 0..200 {
        if t.master.inner_mirror().session(sid).unwrap().is_some() {
            return sid;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{sid} did not land");
}

/// Prompt `sid` as agent `agent` and return the spec the node got.
async fn turn_as(t: &T, sid: SessionId, agent: &str) -> AgentSpec {
    t.master
        .prompt(
            sid,
            "hello".into(),
            PromptOpts {
                agent_id: Some(agent.into()),
                by_owner: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    t.node.last_agent()
}

fn sub<'a>(spec: &'a AgentSpec, id: &str) -> &'a AgentSpec {
    spec.delegates
        .iter()
        .find(|d| d.agent_id == id)
        .unwrap_or_else(|| panic!("no delegate {id}"))
}

async fn personal_id(t: &T, who: &str) -> String {
    let v = req(t, Method::GET, "/api/workspaces", None, who).await.1;
    v["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["personal"] == true)
        .unwrap()["workspace_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// An Assigned item reaches its agents only: the turn's agent and each
/// sub-agent carry, by their own ids, the names they may not use; an alias
/// is the same agent; the other workspace's chats, on the same node, carry
/// none of it; Shared again lifts it; the lists change the prompt hash.
#[tokio::test]
async fn an_assigned_item_is_denied_to_every_other_agent_and_workspace() {
    let t = start().await;
    let [alex, _bob] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    let other = team(&t, &alex, "Other").await;
    let before = {
        let sid = chat(&t, &alex, &ws).await;
        turn_as(&t, sid, "coder").await
    };
    assert!(before.deny_skills.is_empty() && before.deny_mcp_servers.is_empty());
    assign(&t, &alex, &ws, "skill", "deploy", &["scout"]).await;
    assign(&t, &alex, &ws, "mcp", "github", &["coder"]).await;
    let sid = chat(&t, &alex, &ws).await;
    let coder = turn_as(&t, sid, "coder").await;
    assert_eq!(coder.agent_id, "coder");
    assert_eq!(coder.deny_skills, ["deploy"]);
    assert!(coder.deny_mcp_servers.is_empty());
    assert_ne!(
        coder.prompt_hash, before.prompt_hash,
        "the lists enter the hash"
    );
    // Each sub-agent by its own id: scout may load deploy, reviewer not;
    // neither may use github (a sub-agent uses its chat's servers only by
    // its own switch, X.34 step C; the list only narrows).
    let scout = sub(&coder, "scout");
    assert!(scout.deny_skills.is_empty(), "{:?}", scout.deny_skills);
    assert_eq!(scout.deny_mcp_servers, ["github"]);
    let reviewer = sub(&coder, "reviewer");
    assert_eq!(reviewer.deny_skills, ["deploy"]);
    assert_eq!(reviewer.deny_mcp_servers, ["github"]);
    // Nothing else was touched: no tool withheld, none added.
    assert_eq!(coder.tools, before.tools);
    assert_eq!(coder.deny_tools, before.deny_tools);
    // An alias is its agent.
    let dev = turn_as(&t, sid, "dev").await;
    assert_eq!(dev.agent_id, "coder");
    assert_eq!(dev.deny_skills, ["deploy"]);
    // Another top-level agent, named directly: both denied.
    let g = turn_as(&t, sid, "teamlead").await;
    assert_eq!(g.deny_skills, ["deploy"]);
    assert_eq!(g.deny_mcp_servers, ["github"]);
    assert!(
        sub(&g, "scout").deny_skills.is_empty(),
        "a sub-agent by its own id"
    );
    // Another workspace on the same node, and the owner's Personal one.
    for w in [other.clone(), personal_id(&t, &alex).await] {
        let sid = chat(&t, &alex, &w).await;
        let c = turn_as(&t, sid, "coder").await;
        assert!(
            c.deny_skills.is_empty() && c.deny_mcp_servers.is_empty(),
            "{w}"
        );
        assert!(sub(&c, "reviewer").deny_skills.is_empty(), "{w}");
        assert_eq!(c.prompt_hash, before.prompt_hash, "{w}");
    }
    // Assigned to nobody: denied to every agent.
    assign(&t, &alex, &ws, "skill", "deploy", &[]).await;
    let c = turn_as(&t, sid, "coder").await;
    assert_eq!(sub(&c, "scout").deny_skills, ["deploy"]);
    // Shared again: nothing denied.
    let (st, v) = set(
        &t,
        &alex,
        &ws,
        "skill",
        "deploy",
        json!({"access": "shared"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let c = turn_as(&t, sid, "coder").await;
    assert!(c.deny_skills.is_empty());
    assert!(sub(&c, "reviewer").deny_skills.is_empty());
}

/// Only an editor (the workspace's `settings` permission) chooses; agents
/// are this workspace's, by id; names follow the node's rules; a member
/// reads the list.
#[tokio::test]
async fn only_editors_choose_and_only_this_workspaces_agents_by_id() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    let other = team(&t, &alex, "Other").await;
    // A member may read, not change.
    let (st, v) = set(
        &t,
        &bob,
        &ws,
        "skill",
        "deploy",
        json!({"access": "assigned", "agents": ["coder"]}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, v) = req(
        &t,
        Method::GET,
        &format!("/api/workspaces/{ws}/plugins"),
        None,
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["can_edit"], false);
    // Another workspace's agent, an unknown id, an alias.
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/agents?workspace={other}"),
        Some(
            json!({"id": "ops", "name": "Ops", "prompt": "Do ops.", "features": {
            "memory": false, "web": false, "read_files": true, "edit_files": false,
            "commands": false, "delegate": false, "as_agent": true, "as_subagent": true}}),
        ),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for agents in [
        json!(["ops"]),
        json!(["nobody"]),
        json!(["dev"]),
        json!(["Coder"]),
    ] {
        let (st, v) = set(
            &t,
            &alex,
            &ws,
            "skill",
            "deploy",
            json!({"access": "assigned", "agents": agents}),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{agents}: {v}");
    }
    // Names: a look-alike letter, a hidden character, `__`, a bad kind.
    for (kind, name) in [
        ("skill", "d%D0%B5ploy"),
        ("skill", "x%E2%80%8B"),
        ("skill", "a%20b"),
        ("mcp", "a__b"),
        ("mcp", "GitHub"),
        ("hook", "x"),
    ] {
        let (st, v) = set(&t, &alex, &ws, kind, name, json!({"access": "shared"})).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{kind}/{name}: {v}");
    }
    let (st, _) = set(&t, &alex, &ws, "skill", "x", json!({"access": "all"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = set(
        &t,
        &alex,
        &ws,
        "skill",
        "x",
        json!({"access": "shared", "agents": ["coder"]}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = set(
        &t,
        &alex,
        &ws,
        "skill",
        "x",
        json!({"access": "assigned", "agents": ["coder"], "start": "srv"}),
    )
    .await;
    assert!(st.is_client_error(), "an unknown field is refused");
    // Not a member at all.
    let mine = personal_id(&t, &alex).await;
    let (st, _) = set(
        &t,
        &bob,
        &mine,
        "skill",
        "x",
        json!({"access": "assigned", "agents": []}),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // The cap on agents per item.
    let many: Vec<String> = (0..101).map(|i| format!("a{i}")).collect();
    let (st, _) = set(
        &t,
        &alex,
        &ws,
        "skill",
        "x",
        json!({"access": "assigned", "agents": many}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // The editor's own choice lands, listed for the member too.
    assign(&t, &alex, &ws, "skill", "deploy", &["coder", "coder"]).await;
    let v = req(
        &t,
        Method::GET,
        &format!("/api/workspaces/{ws}/plugins"),
        None,
        &bob,
    )
    .await
    .1;
    let item = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "deploy")
        .unwrap()
        .clone();
    assert_eq!(item["access"], "assigned");
    assert_eq!(item["agents"], json!(["coder"]), "each once");
    assert_eq!(item["nodes"], json!([]), "no node announces it");
    let v = req(
        &t,
        Method::GET,
        &format!("/api/workspaces/{other}/plugins"),
        None,
        &alex,
    )
    .await
    .1;
    assert!(
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["name"] != "deploy"),
        "{v}"
    );
}

/// A deleted agent leaves every list: a later agent made with its id gets
/// none of what the first was given.
#[tokio::test]
async fn a_deleted_and_remade_agent_starts_with_nothing_assigned() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    let make = |id: &str| {
        json!({"id": id, "name": "Ops", "prompt": "Do ops.", "features": {
            "memory": false, "web": false, "read_files": true, "edit_files": false,
            "commands": false, "delegate": false, "as_agent": true, "as_subagent": true}})
    };
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/agents?workspace={ws}"),
        Some(make("ops")),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assign(&t, &alex, &ws, "skill", "deploy", &["ops"]).await;
    let sid = chat(&t, &alex, &ws).await;
    assert!(turn_as(&t, sid, "ops").await.deny_skills.is_empty());
    let (st, v) = req(
        &t,
        Method::DELETE,
        &format!("/api/agents/ops?workspace={ws}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/agents?workspace={ws}"),
        Some(make("ops")),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(turn_as(&t, sid, "ops").await.deny_skills, ["deploy"]);
    // A server agent likewise, in every workspace.
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/agents?scope=server",
        Some(make("ops2")),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assign(&t, &alex, &ws, "skill", "deploy", &["ops2"]).await;
    let (st, _) = req(
        &t,
        Method::DELETE,
        "/api/agents/ops2?scope=server",
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(
        &t,
        Method::POST,
        "/api/agents?scope=server",
        Some(make("ops2")),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(turn_as(&t, sid, "ops2").await.deny_skills, ["deploy"]);
}

/// A node that does not announce that it enforces the lists (an older
/// node) would ignore them: the master withholds the whole `skill` tool or
/// every MCP tool from each agent with any denial instead, sub-agents
/// included, through `deny_tools`, which every node reads.
#[tokio::test]
async fn a_node_that_does_not_enforce_gets_the_whole_tool_withheld() {
    let t = start_with(&[]).await;
    let [alex, _] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    assign(&t, &alex, &ws, "skill", "deploy", &["scout"]).await;
    assign(&t, &alex, &ws, "mcp", "github", &["coder"]).await;
    let sid = chat(&t, &alex, &ws).await;
    let coder = turn_as(&t, sid, "coder").await;
    assert!(
        coder.deny_tools.iter().any(|d| d == "skill"),
        "{:?}",
        coder.deny_tools
    );
    assert!(
        !coder.deny_tools.iter().any(|d| d == "mcp"),
        "{:?}",
        coder.deny_tools
    );
    assert!(!coder.allows_tool("skill"));
    let scout = sub(&coder, "scout");
    assert!(!scout.deny_tools.iter().any(|d| d == "skill"));
    assert!(scout.deny_tools.iter().any(|d| d == "mcp"));
    // The listing says which nodes do not enforce.
    let v = req(
        &t,
        Method::GET,
        &format!("/api/workspaces/{ws}/plugins"),
        None,
        &alex,
    )
    .await
    .1;
    assert_eq!(v["unenforced"][0]["node_id"], "master", "{v}");
    // Nothing denied: nothing withheld.
    let w = personal_id(&t, &alex).await;
    let sid = chat(&t, &alex, &w).await;
    let c = turn_as(&t, sid, "coder").await;
    assert!(c.allows_tool("skill") && c.allows_mcp());
}

/// What a node announces is listed bounded and clean, one announcement per
/// link (a node cannot change its list faster than it reconnects), with
/// the node that announces each item; the listing carries no path,
/// command or environment, and a name never decides what a node runs.
#[tokio::test]
async fn announcements_are_bounded_one_per_link_and_names_only() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    t.node
        .announce(
            &[
                ("deploy", "Ship \u{202e}it\nnow"),
                ("d\u{0435}ploy", "look-alike"),
                ("x\u{200b}", "hidden"),
            ],
            &["github", "Git_Hub", "a__b"],
        )
        .await;
    // A second one on the same link is ignored.
    t.node.announce(&[("evil", "x")], &["evil"]).await;
    let mut v = Value::Null;
    for _ in 0..200 {
        v = req(
            &t,
            Method::GET,
            &format!("/api/workspaces/{ws}/plugins"),
            None,
            &bob,
        )
        .await
        .1;
        if !v["items"].as_array().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let v2 = req(
        &t,
        Method::GET,
        &format!("/api/workspaces/{ws}/plugins"),
        None,
        &bob,
    )
    .await
    .1;
    assert_eq!(v["items"], v2["items"], "the second announcement was taken");
    let items: Vec<(String, String)> = v2["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["kind"].as_str().unwrap().to_string(),
                i["name"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        items,
        [
            ("mcp".to_string(), "github".to_string()),
            ("skill".to_string(), "deploy".to_string())
        ]
    );
    let deploy = &v2["items"][1];
    assert_eq!(deploy["nodes"][0]["node_id"], "master");
    assert_eq!(deploy["nodes"][0]["description"], "Ship it now");
    assert_eq!(deploy["access"], "shared");
    let text = v2.to_string();
    for no in ["command", "args", "env", "path", "\\u202e"] {
        assert!(!text.contains(no), "{no} in {text}");
    }
    assert_eq!(v2["left_out"][0]["count"], 4, "{v2}");
    // Agents for the picker, with their two switches.
    let coder = v2["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "coder")
        .unwrap();
    assert_eq!(coder["skills"], true);
    assert_eq!(coder["connectors"], true);
}

/// Assignments are checked against a node only as deny lists: the spec of
/// a turn names nothing to load or start beyond what the agent's own
/// definition has, so an item no node has stays absent everywhere.
#[tokio::test]
async fn the_lists_never_name_anything_to_run() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    assign(&t, &alex, &ws, "mcp", "notonanynode", &["general"]).await;
    let sid = chat(&t, &alex, &ws).await;
    let general = turn_as(&t, sid, "general").await;
    // The assigned agent: no list at all, and no MCP tool it did not have.
    assert!(general.deny_mcp_servers.is_empty());
    assert!(
        !general.allows_mcp(),
        "general has no MCP tools: an assignment adds none"
    );
    let json = serde_json::to_string(&general).unwrap();
    assert!(!json.contains("notonanynode"), "{json}");
}

/// Break the master's database under it with `sql` (another connection).
fn break_db(t: &T, sql: &str) {
    let c = rusqlite::Connection::open(&t.db).unwrap();
    c.pragma_update(None, "foreign_keys", false).unwrap();
    c.execute_batch(sql).unwrap();
}

/// Review of PR 147 (M2, M11): Plugins settings that cannot be read fail
/// closed: the turn's agent and its sub-agents lose the whole `skill` tool
/// and every MCP tool, whether the settings' own read fails or the
/// workspace's.
#[tokio::test]
async fn settings_that_cannot_be_read_withhold_skills_and_connectors() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    assign(&t, &alex, &ws, "skill", "deploy", &["coder"]).await;
    let sid = chat(&t, &alex, &ws).await;
    let ok = turn_as(&t, sid, "coder").await;
    assert!(ok.allows_tool("skill") && ok.allows_mcp());
    // The settings' own read fails.
    break_db(&t, "DROP TABLE plugin_access;");
    let c = turn_as(&t, sid, "coder").await;
    for a in [&c, sub(&c, "reviewer")] {
        assert!(
            !a.allows_tool("skill"),
            "{}: {:?}",
            a.agent_id,
            a.deny_tools
        );
        assert!(!a.allows_mcp(), "{}: {:?}", a.agent_id, a.deny_tools);
    }
    // The workspace's read fails: the server's settings, still closed.
    break_db(&t, "ALTER TABLE workspaces RENAME TO workspaces_gone;");
    let spec = t
        .master
        .resolver_for(Some(&ws))
        .await
        .agent_spec_with(Some("coder"), None, None)
        .unwrap();
    assert!(
        !spec.allows_tool("skill") && !spec.allows_mcp(),
        "{:?}",
        spec.deny_tools
    );
    assert!(!sub(&spec, "scout").allows_tool("skill"));
}
