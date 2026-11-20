//! The skills store on the master (ADOPTION_PLAN X.34 step B, PLAN.md
//! T81): who may write which scope, the check on save, the names a turn
//! carries (never a body), and the body a node asks for, answered from the
//! session alone. The nodes are fakes the test drives, so a request can be
//! forged at will.

use crate::common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::agent::AgentSpec;
use harness_core::skills::StoredScope;
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, PromptOpts, router};
use harness_node::NodeLink;
use harness_protocol::{
    CAP_PLUGIN_ACCESS, CAP_STORED_SKILLS, MasterToNode, NodeToMaster, PROTOCOL_VERSION, PromptMsg,
};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

type Answers = Arc<Mutex<HashMap<String, Result<String, String>>>>;
type Seen = Arc<Mutex<Vec<MasterToNode>>>;

/// A fake node: acks every command, mirrors a `session_created` for each
/// chat it is asked to create, keeps each session's seq, records every
/// command and each `skill.result`.
struct Fake {
    tx: mpsc::Sender<NodeToMaster>,
    seqs: Arc<Mutex<HashMap<SessionId, u64>>>,
    answers: Answers,
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
        seqs: Arc::default(),
        answers: Arc::default(),
        seen: Arc::default(),
    };
    let (seqs, answers, seen) = (fake.seqs.clone(), fake.answers.clone(), fake.seen.clone());
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
                seqs.lock().unwrap().insert(*session_id, 1);
                let event = RawEvent {
                    session_id: *session_id,
                    seq: 1,
                    ts_ms: 1,
                    kind: "session_created".into(),
                    payload,
                };
                to_master.try_send(NodeToMaster::Event { event }).unwrap();
            }
            if let MasterToNode::SkillResult {
                req_id,
                body,
                error,
            } = &msg
            {
                let r = match (body, error) {
                    (_, Some(e)) => Err(e.clone()),
                    (Some(b), None) => Ok(b.clone()),
                    (None, None) => Err("empty".into()),
                };
                answers.lock().unwrap().insert(req_id.clone(), r);
                continue;
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
    async fn emit(&self, sid: SessionId, kind: &str, payload: Value) {
        let seq = {
            let mut s = self.seqs.lock().unwrap();
            let n = s.entry(sid).or_insert(0);
            *n += 1;
            *n
        };
        let event = RawEvent {
            session_id: sid,
            seq,
            ts_ms: 10 + seq as i64,
            kind: kind.into(),
            payload,
        };
        self.tx.send(NodeToMaster::Event { event }).await.unwrap();
    }

    /// Ask for skill `name` on behalf of `sid` with request id `req_id`
    /// and wait for the answer.
    async fn ask_as(&self, sid: SessionId, name: &str, req_id: &str) -> Result<String, String> {
        self.tx
            .send(NodeToMaster::Skill {
                req_id: req_id.into(),
                session_id: sid,
                name: name.into(),
            })
            .await
            .unwrap();
        for _ in 0..300 {
            if let Some(r) = self.answers.lock().unwrap().remove(req_id) {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no skill.result for {req_id}");
    }

    async fn ask(&self, sid: SessionId, name: &str) -> Result<String, String> {
        self.ask_as(sid, name, &harness_core::new_id()).await
    }

    fn last_agent(&self) -> AgentSpec {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|m| match m {
                MasterToNode::Prompt(PromptMsg { agent, .. }) => agent.clone(),
                _ => None,
            })
            .expect("a prompt")
    }
}

struct T {
    base: String,
    master: Master,
    chat: Fake,
    other: Fake,
    _dir: tempfile::TempDir,
}

/// A master whose chat-profile node announces `caps`, and a second node
/// ("other") that takes stored skills too.
async fn start_with(caps: &[&str]) -> T {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let claim_path = dir.path().join("claim-code");
    std::fs::write(&claim_path, "X").unwrap();
    let master = Master::new(
        Store::open_in_memory().unwrap(),
        Auth::new(Secret::new("tok".into()), vec![addr.to_string()], false).with_claim(Claim {
            code: Secret::new("CLAIM".into()),
            path: claim_path,
        }),
    );
    let mut all = vec!["profile:chat"];
    all.extend_from_slice(caps);
    let (link, chat) = fake_node("master", &all);
    master.add_link(link, true).await.unwrap();
    let (link, other) = fake_node(
        "other",
        &[CAP_PLUGIN_ACCESS, CAP_STORED_SKILLS, "exec:sandbox"],
    );
    master.add_link(link, false).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        master,
        chat,
        other,
        _dir: dir,
    }
}

async fn start() -> T {
    start_with(&[CAP_PLUGIN_ACCESS, CAP_STORED_SKILLS]).await
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

/// A user made by `admin`, signed in with their own password.
async fn user(t: &T, admin: &str, name: &str) -> String {
    let pw = req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({ "username": name })),
        admin,
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
            json!({"username": name, "password": pw}),
            None,
        )
        .await,
    )
    .await;
    common::signed_in(
        post_anon(
            t,
            "/api/me/password",
            json!({"current": pw, "new": format!("{name}s own password")}),
            Some(&first),
        )
        .await,
    )
    .await
}

/// The server admin "alex", a member "bob" and "carol", who will be one
/// workspace's admin but not the server's.
async fn users(t: &T) -> [String; 3] {
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
    let bob = user(t, &alex, "bob").await;
    let carol = user(t, &alex, "carol").await;
    [alex, bob, carol]
}

/// A team workspace of `owner`'s with bob a member and carol an admin.
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
    for (who, role) in [("bob", "member"), ("carol", "admin")] {
        let (st, v) = req(
            t,
            Method::POST,
            &format!("/api/workspaces/{ws}/members"),
            Some(json!({ "username": who, "role": role })),
            owner,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    ws
}

async fn make(t: &T, c: &str, ws: &str, body: Value) -> (StatusCode, Value) {
    req(
        t,
        Method::POST,
        &format!("/api/workspaces/{ws}/skills"),
        Some(body),
        c,
    )
    .await
}

async fn write(t: &T, c: &str, ws: &str, scope: &str, name: &str, description: &str, body: &str) {
    let (st, v) = make(
        t,
        c,
        ws,
        json!({"scope": scope, "name": name, "description": description, "body": body}),
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

async fn landed(t: &T, sid: SessionId) {
    let want = *t.chat.seqs.lock().unwrap().get(&sid).unwrap();
    for _ in 0..200 {
        if t.master.inner_mirror().head_seq(sid).unwrap_or(0) >= want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("events of {sid} did not land");
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
    t.chat.last_agent()
}

/// Mark turn `turn` of `sid` running on the node as agent `id`, with
/// `withheld` withheld.
async fn running(t: &T, sid: SessionId, turn: &str, id: &str, withheld: &[&str]) {
    t.chat
        .emit(
            sid,
            "turn_started",
            json!({"turn_id": turn,
                   "agent": {"agent_id": id, "name": id, "prompt_hash": "h", "withheld": withheld}}),
        )
        .await;
    landed(t, sid).await;
}

async fn ended(t: &T, sid: SessionId, turn: &str) {
    t.chat
        .emit(
            sid,
            "turn_ended",
            json!({"turn_id": turn, "reason": "completed"}),
        )
        .await;
    landed(t, sid).await;
}

fn names(s: &AgentSpec) -> Vec<(String, StoredScope)> {
    s.stored_skills
        .iter()
        .map(|e| (e.name.clone(), e.scope))
        .collect()
}

fn sub<'a>(spec: &'a AgentSpec, id: &str) -> &'a AgentSpec {
    spec.delegates
        .iter()
        .find(|d| d.agent_id == id)
        .unwrap_or_else(|| panic!("no delegate {id}"))
}

/// Who writes where: a workspace's skills by whoever holds its `settings`
/// permission (its admin carol, not the member bob), the server's by the
/// server's admins only; another workspace's not at all (404). Every
/// member reads, as text.
#[tokio::test]
async fn only_the_scopes_editors_write_and_members_read() {
    let t = start().await;
    let [alex, bob, carol] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    let other = {
        let (st, v) = req(
            &t,
            Method::POST,
            "/api/workspaces",
            Some(json!({"name": "Alex only"})),
            &alex,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        v["workspace_id"].as_str().unwrap().to_string()
    };
    let skill = |scope: &str| json!({"scope": scope, "name": "notes", "description": "Notes", "body": "<b>Do</b> it."});
    // The member writes nothing.
    let (st, _) = make(&t, &bob, &ws, skill("workspace")).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // The workspace's admin writes its scope, never the server's, and not
    // a workspace she is not in.
    let (st, v) = make(&t, &carol, &ws, skill("server")).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, _) = make(&t, &carol, &other, skill("workspace")).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, v) = make(&t, &carol, &ws, skill("workspace")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // The server's admin writes the server's scope.
    write(&t, &alex, &ws, "server", "style", "House style", "Short.").await;
    // Bob reads both, as text, and changes neither.
    let path = |scope: &str, n: &str| format!("/api/workspaces/{ws}/skills/{scope}/{n}");
    let (st, v) = req(&t, Method::GET, &path("workspace", "notes"), None, &bob).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["body"], "<b>Do</b> it.");
    assert_eq!(v["can_edit"], false);
    let (st, v) = req(&t, Method::GET, &path("server", "style"), None, &bob).await;
    assert_eq!((st, v["can_edit"].clone()), (StatusCode::OK, json!(false)));
    for (m, scope, n) in [
        (Method::PUT, "workspace", "notes"),
        (Method::DELETE, "workspace", "notes"),
        (Method::PUT, "server", "style"),
    ] {
        let body = (m == Method::PUT).then(|| json!({"description": "x", "body": "y"}));
        let (st, _) = req(&t, m.clone(), &path(scope, n), body, &bob).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{m} {scope}");
    }
    let (st, _) = req(
        &t,
        Method::PUT,
        &path("server", "style"),
        Some(json!({"description": "x", "body": "y"})),
        &carol,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // Carol edits and deletes her workspace's.
    let (st, _) = req(
        &t,
        Method::PUT,
        &path("workspace", "notes"),
        Some(json!({"description": "Notes, kept", "body": "Keep them."})),
        &carol,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(
        &t,
        Method::DELETE,
        &path("workspace", "notes"),
        None,
        &carol,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(&t, Method::GET, &path("workspace", "notes"), None, &bob).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // The Plugins listing shows the server's as Built-in, read-only to bob.
    let v = req(
        &t,
        Method::GET,
        &format!("/api/workspaces/{ws}/plugins"),
        None,
        &bob,
    )
    .await
    .1;
    let style = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "style")
        .unwrap();
    assert_eq!(style["stored"]["origin"], "builtin");
    assert_eq!(style["stored"]["can_edit"], false);
    assert_eq!(v["can_edit_server"], false);
}

/// The check on save refuses, with a reason and nothing stored: a name of
/// the wrong shape, hidden or control characters, an over-long field, a
/// 10 MB upload, front matter that does not end; an upload's unknown keys
/// are listed back as ignored and `allowed-tools` is said to have no
/// effect.
#[tokio::test]
async fn the_check_refuses_with_a_reason_and_stores_nothing_cut() {
    let t = start().await;
    let [alex, _bob, _carol] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    let field = |name: &str, description: &str, body: &str| json!({"scope": "workspace", "name": name, "description": description, "body": body});
    for (b, why) in [
        (field("My Skill", "d", "b"), "name"),
        (field("-x", "d", "b"), "name"),
        (field("a--b", "d", "b"), "name"),
        (field(&"a".repeat(65), "d", "b"), "name"),
        (field("x\u{200B}y", "d", "b"), "name"),
        (field("ok", "line\u{202E}evil", "b"), "hidden"),
        (field("ok", "one\ntwo", "b"), "line break"),
        (field("ok", &"d".repeat(1025), "b"), "1024"),
        (field("ok", "", "b"), "description"),
        (field("ok", "d", "fine\nbad\u{E0041}tag"), "line 2"),
        (field("ok", "d", "bell\u{7}"), "control"),
        (field("ok", "d", &"b".repeat(24 * 1024 + 1)), "KiB"),
        (field("ok", "d", " \n "), "body"),
        (
            json!({"scope": "workspace", "skill_md": "---\nname: ok\ndescription: d\nno end"}),
            "does not end",
        ),
        (
            json!({"scope": "workspace", "skill_md": "---\ndescription: d\n---\nBody"}),
            "no `name`",
        ),
    ] {
        let (st, v) = make(&t, &alex, &ws, b.clone()).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{b}: {v}");
        let e = v["error"].as_str().unwrap();
        assert!(e.contains(why), "{why}: {e}");
    }
    // A 10 MB upload is refused before it is read.
    let big = format!(
        "---\nname: big\ndescription: d\n---\n{}",
        "x".repeat(10 << 20)
    );
    let (st, _) = make(
        &t,
        &alex,
        &ws,
        json!({"scope": "workspace", "skill_md": big}),
    )
    .await;
    assert!(st.is_client_error(), "{st}");
    let v = req(
        &t,
        Method::GET,
        &format!("/api/workspaces/{ws}/plugins"),
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
            .all(|i| i["stored"].is_null()),
        "nothing stored: {v}"
    );
    // An upload: its other keys are ignored and said so.
    let md = "---\nname: release-notes\ndescription: >\n  Write release notes\n  from the log.\n\
              allowed-tools: Bash(git:*)\nlicense: MIT\n---\n# Release notes\n\nRead the log.\n";
    let (st, v) = make(
        &t,
        &alex,
        &ws,
        json!({"scope": "workspace", "skill_md": md}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["name"], "release-notes");
    assert_eq!(v["ignored"], json!(["allowed-tools", "license"]));
    let notes = v["notes"].to_string();
    assert!(notes.contains("allowed-tools has no effect"), "{notes}");
    let v = req(
        &t,
        Method::GET,
        &format!("/api/workspaces/{ws}/skills/workspace/release-notes"),
        None,
        &alex,
    )
    .await
    .1;
    assert_eq!(v["description"], "Write release notes from the log.");
    assert_eq!(v["body"], "# Release notes\n\nRead the log.\n");
    // Front matter that is not YAML is read as the node reads it, and said.
    let md = "---\nname: lines\ndescription: Use when: the log is long\n---\nBody\n";
    let (st, v) = make(
        &t,
        &alex,
        &ws,
        json!({"scope": "workspace", "skill_md": md}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["notes"].to_string().contains("not valid YAML"), "{v}");
    // An edit's file must keep the skill's name.
    let (st, v) = req(
        &t,
        Method::PUT,
        &format!("/api/workspaces/{ws}/skills/workspace/lines"),
        Some(json!({"skill_md": "---\nname: other\ndescription: d\n---\nB"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
}

/// A server skill wins its name: a workspace skill cannot be made under
/// it, and one made before is left out of the workspace's turns and shown
/// as hidden to its editors only.
#[tokio::test]
async fn a_server_skill_wins_its_name() {
    let t = start().await;
    let [alex, bob, _carol] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    write(&t, &alex, &ws, "workspace", "notes", "Team notes", "Team.").await;
    write(&t, &alex, &ws, "server", "notes", "Server notes", "Server.").await;
    let (st, v) = make(
        &t,
        &alex,
        &ws,
        json!({"scope": "workspace", "name": "notes", "description": "d", "body": "b"}),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    let plugins = |c: &str| {
        let c = c.to_string();
        let t = &t;
        let ws = ws.clone();
        async move {
            req(
                t,
                Method::GET,
                &format!("/api/workspaces/{ws}/plugins"),
                None,
                &c,
            )
            .await
            .1
        }
    };
    let v = plugins(&alex).await;
    assert_eq!(v["hidden_skills"][0]["name"], "notes");
    assert!(
        plugins(&bob).await["hidden_skills"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let sid = chat(&t, &alex, &ws).await;
    let spec = turn_as(&t, sid, "coder").await;
    assert_eq!(names(&spec), [("notes".to_string(), StoredScope::Server)]);
    running(&t, sid, "t1", "coder", &[]).await;
    assert_eq!(t.chat.ask(sid, "notes").await.unwrap(), "Server.");
}

/// Every spec of a turn carries the names and descriptions its agent may
/// load in that workspace (a sub-agent its own, after the Plugins rule),
/// never a body; another workspace's chats get its own and the server's
/// only; a chat agent without file access gets `skill` only when there is
/// one to load; a node without the capability gets no list.
#[tokio::test]
async fn turns_carry_names_by_the_plugins_rule_and_never_a_body() {
    let t = start().await;
    let [alex, _bob, _carol] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    let other = team(&t, &alex, "Other").await;
    // Nothing stored yet: the Assistant has no `skill`; the developer has.
    let sid = chat(&t, &alex, &ws).await;
    let g = turn_as(&t, sid, "general").await;
    assert!(g.stored_skills.is_empty() && !g.allows_tool("skill"));
    let c = turn_as(&t, sid, "coder").await;
    assert!(c.stored_skills.is_empty() && c.allows_tool("skill"));
    write(
        &t,
        &alex,
        &ws,
        "workspace",
        "deploy",
        "Deploy it",
        "SECRET-BODY-1",
    )
    .await;
    write(
        &t,
        &alex,
        &ws,
        "server",
        "style",
        "House style",
        "SECRET-BODY-2",
    )
    .await;
    let g = turn_as(&t, sid, "general").await;
    assert!(g.allows_tool("skill"));
    assert_eq!(
        names(&g),
        [
            ("deploy".to_string(), StoredScope::Workspace),
            ("style".to_string(), StoredScope::Server)
        ]
    );
    let wire = serde_json::to_string(&t.chat.seen.lock().unwrap().clone()).unwrap();
    assert!(!wire.contains("SECRET-BODY"), "a body went out with a turn");
    // Assigned to the scout only: the developer and the reviewer lose it,
    // the scout (a sub-agent, by its own id) keeps it.
    let (st, v) = req(
        &t,
        Method::PUT,
        &format!("/api/workspaces/{ws}/plugins/skill/deploy"),
        Some(json!({"access": "assigned", "agents": ["scout"]})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // The list travels once, on the top spec, with what any spec of the
    // message may use; each spec's own deny list says what it may not.
    let c = turn_as(&t, sid, "coder").await;
    assert_eq!(names(&c).len(), 2, "the scout may use deploy");
    assert!(c.deny_skills.contains(&"deploy".to_string()));
    assert!(sub(&c, "scout").deny_skills.is_empty());
    assert!(
        sub(&c, "reviewer")
            .deny_skills
            .contains(&"deploy".to_string())
    );
    assert!(c.delegates.iter().all(|d| d.stored_skills.is_empty()));
    // The Assistant (no sub-agents) gets only what it may use.
    let g = turn_as(&t, sid, "general").await;
    assert_eq!(names(&g), [("style".to_string(), StoredScope::Server)]);
    // The other workspace: the server's only.
    let o = chat(&t, &alex, &other).await;
    let c = turn_as(&t, o, "coder").await;
    assert_eq!(names(&c), [("style".to_string(), StoredScope::Server)]);
    // A node that does not take stored skills: no list, and the Assistant
    // loses the tool it had only for them.
    let t2 = start_with(&[CAP_PLUGIN_ACCESS]).await;
    let [alex2, _, _] = users(&t2).await;
    let ws2 = team(&t2, &alex2, "Team").await;
    write(&t2, &alex2, &ws2, "workspace", "deploy", "Deploy it", "B").await;
    let s2 = chat(&t2, &alex2, &ws2).await;
    let g = turn_as(&t2, s2, "general").await;
    assert!(g.stored_skills.is_empty() && !g.allows_tool("skill"));
    let c = turn_as(&t2, s2, "coder").await;
    assert!(c.stored_skills.is_empty() && c.allows_tool("skill"));
    running(&t2, s2, "t1", "coder", &[]).await;
    let e = t2.chat.ask(s2, "deploy").await.unwrap_err();
    assert!(e.contains("does not take"), "{e}");
}

/// A body goes only to the node that runs the session, for its open
/// turn's agent, by the Plugins rule checked again here: not to another
/// node, not after the turn ended, not where the turn had `skill`
/// withheld, not for a name the workspace does not see, not past the
/// per-turn cap; an edit or a delete holds from the next request on.
#[tokio::test]
async fn a_body_reaches_only_the_open_turns_agent() {
    let t = start().await;
    let [alex, _bob, _carol] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    let other = team(&t, &alex, "Other").await;
    write(
        &t,
        &alex,
        &ws,
        "workspace",
        "deploy",
        "Deploy it",
        "Ship carefully.",
    )
    .await;
    write(
        &t,
        &alex,
        &other,
        "workspace",
        "theirs",
        "Theirs",
        "Not yours.",
    )
    .await;
    let sid = chat(&t, &alex, &ws).await;
    // No turn yet.
    let e = t.chat.ask(sid, "deploy").await.unwrap_err();
    assert!(e.contains("during a turn"), "{e}");
    running(&t, sid, "t1", "coder", &[]).await;
    assert_eq!(t.chat.ask(sid, "deploy").await.unwrap(), "Ship carefully.");
    // Another node asking for this session learns nothing.
    let e = t.other.ask(sid, "deploy").await.unwrap_err();
    assert_eq!(e, "no such session on this node");
    // Another workspace's skill, a name of the wrong shape: none here.
    for n in ["theirs", "../deploy", "Deploy"] {
        let e = t.chat.ask(sid, n).await.unwrap_err();
        assert!(e.contains("no stored skill"), "{n}: {e}");
    }
    // Assigned away from the turn's agent: refused on the master too.
    let (st, _) = req(
        &t,
        Method::PUT,
        &format!("/api/workspaces/{ws}/plugins/skill/deploy"),
        Some(json!({"access": "assigned", "agents": ["scout"]})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let e = t.chat.ask(sid, "deploy").await.unwrap_err();
    assert!(e.contains("no stored skill"), "{e}");
    // A sub-agent's session of that chat, as the scout: its own id counts.
    let kid = SessionId::new();
    t.chat
        .emit(
            kid,
            "session_created",
            json!({"title": "@scout", "cwd": "/", "model": "m", "backend": "native",
                   "node_id": "master", "profile": "chat", "approval_mode": "ask",
                   "parent_session": sid}),
        )
        .await;
    landed(&t, kid).await;
    running(&t, kid, "k1", "scout", &[]).await;
    assert_eq!(t.chat.ask(kid, "deploy").await.unwrap(), "Ship carefully.");
    // Shared again; a turn that had `skill` withheld gets nothing.
    let (st, _) = req(
        &t,
        Method::PUT,
        &format!("/api/workspaces/{ws}/plugins/skill/deploy"),
        Some(json!({"access": "shared"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    ended(&t, sid, "t1").await;
    let e = t.chat.ask(sid, "deploy").await.unwrap_err();
    assert!(e.contains("during a turn"), "after the turn: {e}");
    running(&t, sid, "t2", "coder", &["skill"]).await;
    let e = t.chat.ask(sid, "deploy").await.unwrap_err();
    assert!(e.contains("not available in this turn"), "{e}");
    ended(&t, sid, "t2").await;
    // The per-turn cap, a repeated request id counted like any other.
    running(&t, sid, "t3", "coder", &[]).await;
    for i in 0..harness_master::stored_skills::PER_TURN {
        let id = if i < 2 { "same" } else { "" };
        let r = if id.is_empty() {
            t.chat.ask(sid, "deploy").await
        } else {
            t.chat.ask_as(sid, "deploy", id).await
        };
        assert!(r.is_ok(), "{i}: {r:?}");
    }
    let e = t.chat.ask(sid, "deploy").await.unwrap_err();
    assert!(e.contains("already loaded"), "{e}");
    ended(&t, sid, "t3").await;
    // Edited mid-chat: the next load has the new text; deleted: none.
    running(&t, sid, "t4", "coder", &[]).await;
    let (st, _) = req(
        &t,
        Method::PUT,
        &format!("/api/workspaces/{ws}/skills/workspace/deploy"),
        Some(json!({"description": "Deploy it", "body": "Ship on Fridays never."})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        t.chat.ask(sid, "deploy").await.unwrap(),
        "Ship on Fridays never."
    );
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/workspaces/{ws}/skills/workspace/deploy"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let e = t.chat.ask(sid, "deploy").await.unwrap_err();
    assert!(e.contains("no stored skill"), "{e}");
}

fn command_bytes(t: &T, pick: impl Fn(&MasterToNode) -> bool) -> (usize, MasterToNode) {
    let seen = t.chat.seen.lock().unwrap();
    let m = seen
        .iter()
        .rev()
        .find(|m| pick(m))
        .expect("a command")
        .clone();
    (serde_json::to_vec(&m).unwrap().len(), m)
}

/// The most a turn's command may grow by its stored-skill list at the
/// caps: 150 entries of a 64-character name and a 200-character
/// description of 4-byte characters, about 130 KiB, sent once.
const LIST_BOUND: usize = 256 * 1024;

/// Review of PR 148 (H1): at the store's caps (75 + 75 skills whose
/// descriptions are 1,024 four-byte characters) the list travels once, on
/// the top spec, its descriptions cut to 200 characters on the wire: a
/// Senior Teamlead's prompt and a workspace lead's agent switch with 18
/// sub-agents stay far under the link's frame, and the next message goes.
#[tokio::test]
async fn the_list_travels_once_and_fits_the_link() {
    let t = start().await;
    let [alex, _bob, _carol] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    let sid = chat(&t, &alex, &ws).await;
    turn_as(&t, sid, "teamlead").await;
    let (before, _) = command_bytes(&t, |m| matches!(m, MasterToNode::Prompt(_)));
    let desc: String = "\u{1F600}".repeat(1024);
    for i in 0..75 {
        write(&t, &alex, &ws, "workspace", &format!("w{i}"), &desc, "b").await;
        write(&t, &alex, &ws, "server", &format!("s{i}"), &desc, "b").await;
    }
    let spec = turn_as(&t, sid, "teamlead").await;
    let (lead, _) = command_bytes(&t, |m| matches!(m, MasterToNode::Prompt(_)));
    assert!(spec.delegates.len() >= 4, "{}", spec.delegates.len());
    assert_eq!(spec.stored_skills.len(), 150);
    assert!(spec.delegates.iter().all(|d| d.stored_skills.is_empty()));
    assert!(
        spec.stored_skills
            .iter()
            .all(|e| e.description.chars().count() == 200),
        "cut to 200 on the wire"
    );
    assert!(
        lead - before < LIST_BOUND,
        "teamlead prompt grew {before} -> {lead}"
    );
    // The full description stays in the store and on the screen.
    let v = req(
        &t,
        Method::GET,
        &format!("/api/workspaces/{ws}/skills/workspace/w0"),
        None,
        &alex,
    )
    .await
    .1;
    assert_eq!(v["description"].as_str().unwrap().chars().count(), 1024);
    // A workspace lead with 18 sub-agents.
    let features = json!({
        "memory": false, "web": false, "read_files": true, "edit_files": true,
        "commands": false, "delegate": false, "as_agent": true, "as_subagent": true
    });
    for i in 0..12 {
        let (st, v) = req(
            &t,
            Method::POST,
            &format!("/api/agents?workspace={ws}"),
            Some(
                json!({"id": format!("helper{i}"), "name": format!("Helper {i}"),
                        "prompt": "Help.", "features": features}),
            ),
            &alex,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    let mut f = features.clone();
    f["delegate"] = json!(true);
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/agents?workspace={ws}"),
        Some(json!({"id": "lead", "name": "Lead", "prompt": "Lead.", "features": f})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/sessions/{sid}/agent"),
        Some(json!({"agent_id": "lead"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (set_len, set) = command_bytes(&t, |m| matches!(m, MasterToNode::SetAgent { .. }));
    let MasterToNode::SetAgent { agent, .. } = set else {
        unreachable!()
    };
    assert!(agent.delegates.len() >= 18, "{}", agent.delegates.len());
    assert!(
        set_len < LIST_BOUND + 64 * 1024,
        "set_agent is {set_len} bytes"
    );
    t.master
        .prompt(
            sid,
            "hello".into(),
            PromptOpts {
                by_owner: true,
                ..Default::default()
            },
        )
        .await
        .expect("the next message goes");
}

/// Review of PR 148 (M1, the coordinator's decision): unlike `recall` and
/// the board, a stored skill loads in a callback turn and in a temporary
/// chat: it is an admin's instructions for agents, not the person's data.
#[tokio::test]
async fn callback_and_temporary_turns_load_stored_skills() {
    let t = start().await;
    let [alex, _bob, _carol] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    write(
        &t,
        &alex,
        &ws,
        "workspace",
        "deploy",
        "Deploy it",
        "Ship carefully.",
    )
    .await;
    let sid = chat(&t, &alex, &ws).await;
    t.chat
        .emit(
            sid,
            "turn_started",
            json!({"turn_id": "cb1", "cause": {"callback": ["bg-1"]},
                   "agent": {"agent_id": "coder", "name": "coder", "prompt_hash": "h", "withheld": []}}),
        )
        .await;
    landed(&t, sid).await;
    assert_eq!(t.chat.ask(sid, "deploy").await.unwrap(), "Ship carefully.");
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "workspace_id": ws, "incognito": true})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let tmp: SessionId = v["session_id"].as_str().unwrap().parse().unwrap();
    for _ in 0..200 {
        if t.master.inner_mirror().session(tmp).unwrap().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    running(&t, tmp, "t1", "coder", &[]).await;
    assert_eq!(t.chat.ask(tmp, "deploy").await.unwrap(), "Ship carefully.");
}

/// Review of PR 148 (M2): a turn and its sub-agents share one cap,
/// counted on the chat's turn.
#[tokio::test]
async fn a_turn_and_its_sub_agents_share_the_cap() {
    let t = start().await;
    let [alex, _bob, _carol] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    write(&t, &alex, &ws, "workspace", "deploy", "Deploy it", "Ship.").await;
    let sid = chat(&t, &alex, &ws).await;
    running(&t, sid, "t1", "coder", &[]).await;
    let per = harness_master::stored_skills::PER_TURN;
    for _ in 0..per / 2 {
        t.chat.ask(sid, "deploy").await.unwrap();
    }
    let mut kids = Vec::new();
    for k in 0..3 {
        let kid = SessionId::new();
        t.chat
            .emit(
                kid,
                "session_created",
                json!({"title": "@scout", "cwd": "/", "model": "m", "backend": "native",
                       "node_id": "master", "profile": "chat", "approval_mode": "ask",
                       "parent_session": sid}),
            )
            .await;
        landed(&t, kid).await;
        running(&t, kid, &format!("k{k}"), "scout", &[]).await;
        kids.push(kid);
    }
    let mut ok = per / 2;
    for kid in &kids {
        for _ in 0..per {
            ok += t.chat.ask(*kid, "deploy").await.is_ok() as u32;
        }
    }
    assert_eq!(ok, per, "one cap for the turn and its sub-agents");
    let e = t.chat.ask(sid, "deploy").await.unwrap_err();
    assert!(e.contains("already loaded"), "{e}");
    // The chat's next turn has a cap of its own.
    ended(&t, sid, "t1").await;
    running(&t, sid, "t2", "coder", &[]).await;
    assert!(t.chat.ask(sid, "deploy").await.is_ok());
}

/// Review of PR 148 (L7): a chat outside any workspace (the API token's)
/// carries no list and is answered no body.
#[tokio::test]
async fn a_chat_outside_any_workspace_gets_no_stored_skill() {
    let t = start().await;
    let [alex, _bob, _carol] = users(&t).await;
    let ws = team(&t, &alex, "Team").await;
    write(&t, &alex, &ws, "server", "style", "House style", "Short.").await;
    let r = reqwest::Client::new()
        .post(format!("{}/api/sessions", t.base))
        .bearer_auth("tok")
        .json(&json!({"profile": "chat"}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{}", r.status());
    let v: Value = r.json().await.unwrap();
    let sid: SessionId = v["session_id"].as_str().unwrap().parse().unwrap();
    for _ in 0..200 {
        if t.master.inner_mirror().session(sid).unwrap().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let row = t.master.inner_mirror().session(sid).unwrap().unwrap();
    assert!(row.workspace_id.is_none(), "{row:?}");
    let spec = turn_as(&t, sid, "coder").await;
    assert!(spec.stored_skills.is_empty());
    running(&t, sid, "t1", "coder", &[]).await;
    let e = t.chat.ask(sid, "style").await.unwrap_err();
    assert!(e.contains("only in a workspace's chats"), "{e}");
}
