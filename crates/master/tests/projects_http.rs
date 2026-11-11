//! Projects end to end (docs/projects.md): CRUD and who sees what, chats in
//! projects and the per-turn project context, project memory proposals, the
//! board and its templates.

mod common;
use common::SameOrigin as _;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_agent_native::{
    ChatMsg, ChatRequest, Completion, NativeBackend, NativeConfig, Provider, ProviderError,
    mock::Mock,
};
use harness_core::Secret;
use harness_core::event::Profile;
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_memory::SqliteMemory;
use harness_node::{NodeConfig, NodePolicy, NodeRuntime};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

struct Rec {
    reqs: Mutex<Vec<ChatRequest>>,
}

#[async_trait::async_trait]
impl Provider for Rec {
    fn name(&self) -> &'static str {
        "mock"
    }
    async fn complete(
        &self,
        req: &ChatRequest,
        d: mpsc::UnboundedSender<String>,
    ) -> Result<Completion, ProviderError> {
        self.reqs.lock().unwrap().push(req.clone());
        Mock {
            delay: Duration::from_millis(0),
        }
        .complete(req, d)
        .await
    }
}

struct T {
    base: String,
    rec: Arc<Rec>,
    mem: Arc<SqliteMemory>,
    _dir: tempfile::TempDir,
}

async fn start() -> T {
    let dir = tempfile::tempdir().unwrap();
    let rec = Arc::new(Rec {
        reqs: Mutex::new(vec![]),
    });
    let chat = NodeRuntime::start(NodeConfig {
        node_id: "master".into(),
        hostname: "h".into(),
        policy: NodePolicy::default(),
        store: Store::open_in_memory().unwrap(),
        backend: Arc::new(NativeBackend::new(
            rec.clone(),
            vec![Arc::new(harness_memory::Remember)],
            NativeConfig::default(),
        )),
        default_model: "mock".into(),
        secrets: Default::default(),
        profile: Profile::Chat,
        incognito_ttl: None,
        checkpoints_dir: None,
        artifacts: None,
    })
    .await
    .unwrap();
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
    let mem = Arc::new(SqliteMemory::open_in_memory().unwrap());
    master.set_memory(mem.clone());
    master.add_link(chat.attach().await, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        rec,
        mem,
        _dir: dir,
    }
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
    let r = rb.header("cookie", c).same_origin().send().await.unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

fn cookie(r: &reqwest::Response) -> String {
    r.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

/// The admin "alex" and a member "bob".
async fn users(t: &T) -> (String, String) {
    let r = reqwest::Client::new()
        .post(format!("{}/api/setup", t.base))
        .json(&json!({"claim_code": "CLAIM", "username": "alex", "password": "admin password"}))
        .send()
        .await
        .unwrap();
    let alex = cookie(&r);
    req(
        t,
        Method::POST,
        "/api/users",
        Some(json!({"username": "bob", "password": "temporary pw"})),
        &alex,
    )
    .await;
    let r = reqwest::Client::new()
        .post(format!("{}/api/login", t.base))
        .json(&json!({"username": "bob", "password": "temporary pw"}))
        .send()
        .await
        .unwrap();
    let r = reqwest::Client::new()
        .post(format!("{}/api/me/password", t.base))
        .header("cookie", cookie(&r))
        .same_origin()
        .json(&json!({"current": "temporary pw", "new": "bobs own password"}))
        .send()
        .await
        .unwrap();
    (alex, cookie(&r))
}

async fn new_project(t: &T, c: &str, body: Value) -> Value {
    let (st, v) = req(t, Method::POST, "/api/projects", Some(body), c).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v
}

async fn new_chat(t: &T, c: &str, project: Option<&str>) -> String {
    let (st, v) = req(
        t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "project_id": project})),
        c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().to_string()
}

/// Send `text` and wait for the turn to end.
async fn say(t: &T, c: &str, sid: &str, text: &str) {
    let (_, before) = req(t, Method::GET, &format!("/api/sessions/{sid}"), None, c).await;
    let head = before["head_seq"].as_u64().unwrap();
    let (st, v) = req(
        t,
        Method::POST,
        &format!("/api/sessions/{sid}/prompt"),
        Some(json!({"text": text})),
        c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    for _ in 0..300 {
        let (_, row) = req(t, Method::GET, &format!("/api/sessions/{sid}"), None, c).await;
        if row["running"] == false && row["head_seq"].as_u64().unwrap_or(0) > head + 2 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("turn did not end");
}

/// The per-turn context of the last request (the newest user message); the
/// system prompt must never carry project text (prompt cache).
fn last_context(t: &T) -> String {
    let reqs = t.rec.reqs.lock().unwrap();
    let r = reqs.last().unwrap();
    let ChatMsg::System(sys) = &r.messages[0] else {
        panic!()
    };
    assert!(!sys.contains("Project \""), "{sys}");
    r.messages
        .iter()
        .rev()
        .find_map(|m| match m {
            ChatMsg::User(u) => Some(u.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn crud_and_privacy() {
    let t = start().await;
    let (alex, bob) = users(&t).await;
    let p = new_project(
        &t,
        &alex,
        json!({"name": "  Garden\u{200B} ", "icon": "house", "accent": "green", "description": "Our allotment"}),
    )
    .await;
    let id = p["project_id"].as_str().unwrap().to_string();
    assert_eq!(p["name"], "Garden", "trimmed, hidden characters dropped");
    assert_eq!(p["can_manage"], true);
    assert_eq!(p["settings"]["workspace_memory"], true);

    let (_, list) = req(&t, Method::GET, "/api/projects", None, &alex).await;
    assert_eq!(list["projects"].as_array().unwrap().len(), 1);
    assert_eq!(list["can_create"], true);
    assert!(list["icons"].as_array().unwrap().contains(&json!("house")));

    for (body, why) in [
        (json!({"icon": "skull"}), "unknown icon"),
        (json!({"accent": "red"}), "unknown accent"),
        (json!({"name": " "}), "empty name"),
        (
            json!({"settings": {"workspace_memory": true, "extra": 1}}),
            "unknown setting",
        ),
        (json!({"colour": "green"}), "unknown field"),
    ] {
        let (st, _) = req(
            &t,
            Method::PATCH,
            &format!("/api/projects/{id}"),
            Some(body),
            &alex,
        )
        .await;
        assert!(st.is_client_error(), "{why}: {st}");
    }
    let (st, v) = req(
        &t,
        Method::PATCH,
        &format!("/api/projects/{id}"),
        Some(json!({
            "instructions": "Plants are listed by Latin name.\nUse metric units.",
            "settings": {"workspace_memory": false, "node_id": null, "cwd": " /srv/garden "}
        })),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["settings"]["cwd"], "/srv/garden");
    assert_eq!(v["icon"], "house", "unchanged");

    // Bob is in another (Personal) workspace: he sees nothing of it.
    let (_, list) = req(&t, Method::GET, "/api/projects", None, &bob).await;
    assert!(list["projects"].as_array().unwrap().is_empty());
    for (method, path) in [
        (Method::GET, format!("/api/projects/{id}")),
        (Method::DELETE, format!("/api/projects/{id}")),
        (Method::GET, format!("/api/projects/{id}/board")),
        (Method::GET, format!("/api/projects/{id}/memory")),
    ] {
        let (st, _) = req(&t, method, &path, None, &bob).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{path}");
    }
    let (st, _) = req(
        &t,
        Method::PATCH,
        &format!("/api/projects/{id}"),
        Some(json!({"name": "mine now"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // The API token must name a workspace, and one that exists.
    let (st, _) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(json!({"name": "x"})),
        "",
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let token = |body: Value| {
        reqwest::Client::new()
            .post(format!("{}/api/projects", t.base))
            .bearer_auth("tok")
            .json(&body)
            .send()
    };
    let r = token(json!({"name": "x"})).await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    let r = token(json!({"name": "x", "workspace_id": "nope"}))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    let ws = p["workspace_id"].as_str().unwrap();
    let r = token(json!({"name": "By token", "workspace_id": ws}))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let (_, list) = req(&t, Method::GET, "/api/projects", None, &alex).await;
    assert_eq!(list["projects"].as_array().unwrap().len(), 2);

    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/projects/{id}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(&t, Method::GET, &format!("/api/projects/{id}"), None, &alex).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn chats_join_and_get_the_project_context() {
    let t = start().await;
    let (alex, bob) = users(&t).await;
    let p = new_project(
        &t,
        &alex,
        json!({"name": "Garden", "instructions": "Use metric units."}),
    )
    .await;
    let id = p["project_id"].as_str().unwrap().to_string();

    let inside = new_chat(&t, &alex, Some(&id)).await;
    let (_, row) = req(
        &t,
        Method::GET,
        &format!("/api/sessions/{inside}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(row["project_id"], id.as_str());
    let (_, d) = req(&t, Method::GET, &format!("/api/projects/{id}"), None, &alex).await;
    assert_eq!(d["chats"], 1);
    say(&t, &alex, &inside, "how deep do I plant garlic?").await;
    let ctx = last_context(&t);
    assert!(ctx.contains("Project \"Garden\""), "{ctx}");
    assert!(
        ctx.contains("Project instructions:\nUse metric units."),
        "{ctx}"
    );
    assert!(ctx.contains("scope \"project\""), "{ctx}");

    // A chat outside the project gets none of it, until it joins.
    let outside = new_chat(&t, &alex, None).await;
    say(&t, &alex, &outside, "how deep do I plant garlic?").await;
    assert!(!last_context(&t).contains("Project \""));
    let (st, v) = req(
        &t,
        Method::PUT,
        &format!("/api/sessions/{outside}/project"),
        Some(json!({"project_id": id})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    say(&t, &alex, &outside, "and onions?").await;
    assert!(last_context(&t).contains("Use metric units."));
    // ...and leaves again.
    req(
        &t,
        Method::PUT,
        &format!("/api/sessions/{outside}/project"),
        Some(json!({"project_id": null})),
        &alex,
    )
    .await;
    let (_, row) = req(
        &t,
        Method::GET,
        &format!("/api/sessions/{outside}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(row["project_id"], Value::Null);

    // Temporary chats cannot join.
    let (st, _) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "incognito": true, "project_id": id})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    // Bob can neither start a chat in it nor move his chat into it, nor
    // move Alex's chat anywhere.
    let (st, _) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "project_id": id})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let bobs = new_chat(&t, &bob, None).await;
    let (st, _) = req(
        &t,
        Method::PUT,
        &format!("/api/sessions/{bobs}/project"),
        Some(json!({"project_id": id})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _) = req(
        &t,
        Method::PUT,
        &format!("/api/sessions/{inside}/project"),
        Some(json!({"project_id": null})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Deleting the project keeps the chat, outside any project.
    req(
        &t,
        Method::DELETE,
        &format!("/api/projects/{id}"),
        None,
        &alex,
    )
    .await;
    let (st, row) = req(
        &t,
        Method::GET,
        &format!("/api/sessions/{inside}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(row["project_id"], Value::Null);
}

async fn project_facts(t: &T, c: &str, id: &str) -> Vec<Value> {
    let (st, v) = req(
        t,
        Method::GET,
        &format!("/api/projects/{id}/memory"),
        None,
        c,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["memories"].as_array().unwrap().clone()
}

#[tokio::test]
async fn project_memory_is_proposed_confirmed_and_recalled() {
    let t = start().await;
    let (alex, _) = users(&t).await;
    let p = new_project(&t, &alex, json!({"name": "Garden"})).await;
    let id = p["project_id"].as_str().unwrap().to_string();
    let sid = new_chat(&t, &alex, Some(&id)).await;
    say(
        &t,
        &alex,
        &sid,
        "remember project: the beds are watered at dawn",
    )
    .await;
    let mut facts = vec![];
    for _ in 0..100 {
        facts = project_facts(&t, &alex, &id).await;
        if !facts.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0]["status"], "proposed");
    assert_eq!(facts[0]["source_session"], sid.as_str());
    let (_, personal) = req(&t, Method::GET, "/api/memory", None, &alex).await;
    assert!(
        personal["memories"].as_array().unwrap().is_empty(),
        "a project fact is not a personal one"
    );
    let fid = facts[0]["id"].as_str().unwrap().to_string();

    // Not recalled until confirmed.
    say(&t, &alex, &sid, "when are the beds watered?").await;
    assert!(!last_context(&t).contains("watered at dawn"));
    let (st, v) = req(
        &t,
        Method::PATCH,
        &format!("/api/projects/{id}/memory/{fid}"),
        Some(json!({"status": "active"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    say(&t, &alex, &sid, "when are the beds watered?").await;
    let ctx = last_context(&t);
    assert!(
        ctx.contains("What this project has established") && ctx.contains("watered at dawn"),
        "{ctx}"
    );
    // Only chats in the project recall it.
    let outside = new_chat(&t, &alex, None).await;
    say(&t, &alex, &outside, "when are the beds watered?").await;
    assert!(!last_context(&t).contains("watered at dawn"));
    // Outside a project, a project proposal becomes a personal one.
    say(
        &t,
        &alex,
        &outside,
        "remember project: tomatoes need stakes",
    )
    .await;
    let mut personal = vec![];
    for _ in 0..100 {
        let (_, v) = req(&t, Method::GET, "/api/memory", None, &alex).await;
        personal = v["memories"].as_array().unwrap().clone();
        if !personal.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(personal.len(), 1);
    assert_eq!(personal[0]["text"], "tomatoes need stakes");

    // Added by hand: active at once. Personal memory routes cannot reach it.
    let (st, f) = req(
        &t,
        Method::POST,
        &format!("/api/projects/{id}/memory"),
        Some(json!({"text": "Compost bin is by the shed"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{f}");
    assert_eq!(f["status"], "active");
    let added = f["id"].as_str().unwrap();
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/memory/{added}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // Deleting the project forgets its facts.
    use harness_memory::MemoryStore;
    let owner = format!("project:{id}");
    assert_eq!(t.mem.list(Some(&owner)).await.unwrap().len(), 2);
    req(
        &t,
        Method::DELETE,
        &format!("/api/projects/{id}"),
        None,
        &alex,
    )
    .await;
    assert!(t.mem.list(Some(&owner)).await.unwrap().is_empty());
}

fn column<'a>(board: &'a Value, name: &str) -> &'a Value {
    board["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap()
}

#[tokio::test]
async fn board_and_templates() {
    let t = start().await;
    let (alex, _) = users(&t).await;
    let p = new_project(&t, &alex, json!({"name": "App", "template_id": "software"})).await;
    let id = p["project_id"].as_str().unwrap().to_string();
    let (_, board) = req(
        &t,
        Method::GET,
        &format!("/api/projects/{id}/board"),
        None,
        &alex,
    )
    .await;
    let names: Vec<_> = board["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Backlog", "To do", "In progress", "Review", "Done"]);
    assert_eq!(column(&board, "Done")["done"], true);

    let todo = column(&board, "To do")["column_id"]
        .as_str()
        .unwrap()
        .to_string();
    let doing = column(&board, "In progress")["column_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, board) = req(
        &t,
        Method::POST,
        &format!("/api/projects/{id}/board/cards"),
        Some(json!({"column_id": todo, "title": "Login screen", "notes": "email + password"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{board}");
    let card = column(&board, "To do")["cards"][0]["card_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, board) = req(
        &t,
        Method::PATCH,
        &format!("/api/projects/{id}/board/cards/{card}"),
        Some(json!({"column_id": doing, "position": 0})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{board}");
    assert_eq!(
        column(&board, "In progress")["cards"][0]["title"],
        "Login screen"
    );
    assert!(
        column(&board, "To do")["cards"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // The model sees the open cards.
    let sid = new_chat(&t, &alex, Some(&id)).await;
    say(&t, &alex, &sid, "what's next?").await;
    assert!(
        last_context(&t).contains("Board — In progress: Login screen."),
        "{}",
        last_context(&t)
    );
    // A card links one of your chats in the project, and only those.
    let (st, board) = req(
        &t,
        Method::PATCH,
        &format!("/api/projects/{id}/board/cards/{card}"),
        Some(json!({"session_id": sid})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{board}");
    assert_eq!(
        column(&board, "In progress")["cards"][0]["chat"]["session_id"],
        sid.as_str()
    );
    let outside = new_chat(&t, &alex, None).await;
    let (st, _) = req(
        &t,
        Method::PATCH,
        &format!("/api/projects/{id}/board/cards/{card}"),
        Some(json!({"session_id": outside})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, board) = req(
        &t,
        Method::PATCH,
        &format!("/api/projects/{id}/board/cards/{card}"),
        Some(json!({"session_id": null})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        column(&board, "In progress")["cards"][0]["chat"],
        Value::Null
    );

    // Nothing is lost silently: a column with cards stays, and a template
    // does not replace a board that has cards.
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/projects/{id}/board/columns/{doing}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (st, _) = req(
        &t,
        Method::POST,
        &format!("/api/projects/{id}/board/template"),
        Some(json!({"template_id": "basic"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);

    // Save the columns as a template; a new project starts from it.
    let (st, tpl) = req(
        &t,
        Method::POST,
        "/api/board-templates",
        Some(json!({"name": "Our flow", "project_id": id})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{tpl}");
    let tid = tpl["template_id"].as_str().unwrap().to_string();
    let (_, all) = req(&t, Method::GET, "/api/board-templates", None, &alex).await;
    let all = all["templates"].as_array().unwrap();
    assert_eq!(all[0]["template_id"], "basic", "built-ins first");
    assert!(
        all.iter()
            .any(|x| x["template_id"] == tid.as_str() && x["builtin"] == false)
    );
    let q = new_project(&t, &alex, json!({"name": "Second", "template_id": tid})).await;
    let qid = q["project_id"].as_str().unwrap();
    let (_, qb) = req(
        &t,
        Method::GET,
        &format!("/api/projects/{qid}/board"),
        None,
        &alex,
    )
    .await;
    assert_eq!(qb["columns"].as_array().unwrap().len(), 5);
    assert!(
        qb["columns"][0]["cards"].as_array().unwrap().is_empty(),
        "columns only"
    );
    // An empty board takes another template.
    let (st, qb) = req(
        &t,
        Method::POST,
        &format!("/api/projects/{qid}/board/template"),
        Some(json!({"template_id": "writing"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{qb}");
    assert_eq!(qb["columns"][0]["name"], "Ideas");

    let (st, _) = req(
        &t,
        Method::DELETE,
        "/api/board-templates/basic",
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/board-templates/{tid}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
}

async fn wait_facts(t: &T, c: &str, path: &str, n: usize) -> Vec<Value> {
    let mut facts = vec![];
    for _ in 0..200 {
        let (_, v) = req(t, Method::GET, path, None, c).await;
        facts = v["memories"].as_array().cloned().unwrap_or_default();
        if facts.len() >= n {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    facts
}

/// Projects in a team workspace: members use them, admins curate them, the
/// personal-memory switch leaves project memory alone, and the workspace
/// takes its projects when it goes.
#[tokio::test]
async fn team_workspace_projects() {
    let t = start().await;
    let (alex, bob) = users(&t).await;
    let (st, w) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{w}");
    let acme = w["workspace_id"].as_str().unwrap().to_string();
    let (st, v) = req(
        &t,
        Method::POST,
        &format!("/api/workspaces/{acme}/members"),
        Some(json!({"username": "bob", "role": "member"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let p = new_project(
        &t,
        &alex,
        json!({"name": "Launch", "workspace_id": acme, "instructions": "Ship on Fridays."}),
    )
    .await;
    let id = p["project_id"].as_str().unwrap().to_string();
    assert_eq!(p["workspace_id"], acme.as_str());
    // Alex's active workspace is still Personal: its list has none of it.
    let (_, list) = req(&t, Method::GET, "/api/projects", None, &alex).await;
    assert!(list["projects"].as_array().unwrap().is_empty());

    // Bob, a member, sees it and its board, but changes neither.
    let q = format!("/api/projects?workspace={acme}");
    let (_, list) = req(&t, Method::GET, &q, None, &bob).await;
    assert_eq!(list["projects"].as_array().unwrap().len(), 1);
    assert_eq!(list["can_create"], false);
    let (_, d) = req(&t, Method::GET, &format!("/api/projects/{id}"), None, &bob).await;
    assert_eq!(d["can_manage"], false);
    let (st, board) = req(
        &t,
        Method::GET,
        &format!("/api/projects/{id}/board"),
        None,
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let todo = board["columns"][0]["column_id"].as_str().unwrap();
    for (method, path, body) in [
        (
            Method::PATCH,
            format!("/api/projects/{id}"),
            json!({"name": "Mine"}),
        ),
        (
            Method::POST,
            format!("/api/projects/{id}/board/cards"),
            json!({"column_id": todo, "title": "x"}),
        ),
        (
            Method::POST,
            format!("/api/projects/{id}/memory"),
            json!({"text": "x"}),
        ),
        (
            Method::POST,
            "/api/projects".into(),
            json!({"name": "x", "workspace_id": acme}),
        ),
    ] {
        let (st, _) = req(&t, method, &path, Some(body), &bob).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{path}");
    }

    // A chat starts in the project only in its workspace.
    let (st, _) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "project_id": id})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "Bob's active one is Personal");
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "chat", "project_id": id, "workspace_id": acme})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let (_, row) = req(&t, Method::GET, &format!("/api/sessions/{sid}"), None, &bob).await;
    assert_eq!(
        (row["workspace_id"].as_str(), row["project_id"].as_str()),
        (Some(acme.as_str()), Some(id.as_str()))
    );

    // Bob keeps his personal memory out of Acme: the project's context
    // stays, and only project facts may be proposed.
    let (st, _) = req(
        &t,
        Method::PUT,
        &format!("/api/workspaces/{acme}/memory"),
        Some(json!({"use_memory": false})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    say(&t, &bob, &sid, "remember: I take my tea black").await;
    let ctx = last_context(&t);
    assert!(ctx.contains("Ship on Fridays."), "{ctx}");
    assert!(ctx.contains("use no other scope"), "{ctx}");
    say(
        &t,
        &bob,
        &sid,
        "remember project: deliveries come on Mondays",
    )
    .await;
    let memory = format!("/api/projects/{id}/memory");
    let facts = wait_facts(&t, &bob, &memory, 1).await;
    assert_eq!(facts.len(), 1, "{facts:?}");
    assert_eq!(facts[0]["status"], "proposed");
    let (_, mine) = req(&t, Method::GET, "/api/memory", None, &bob).await;
    assert!(
        mine["memories"].as_array().unwrap().is_empty(),
        "nothing personal filed: {mine}"
    );
    // The proposal is Bob's alone; he keeps it, then it is Alex's to curate.
    assert!(project_facts(&t, &alex, &id).await.is_empty());
    let fid = facts[0]["id"].as_str().unwrap().to_string();
    let fact = format!("/api/projects/{id}/memory/{fid}");
    // Bob may word his proposal, but activating it needs a manager, unless
    // the project lets members (3.5).
    let (st, v) = req(
        &t,
        Method::PATCH,
        &fact,
        Some(json!({"status": "active"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, v) = req(
        &t,
        Method::PATCH,
        &fact,
        Some(json!({"text": "deliveries come on Mondays"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(project_facts(&t, &alex, &id).await.is_empty());
    let (st, v) = req(
        &t,
        Method::PATCH,
        &format!("/api/projects/{id}"),
        Some(json!({"settings": {"members_activate_facts": true}})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = req(
        &t,
        Method::PATCH,
        &fact,
        Some(json!({"status": "active"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(project_facts(&t, &alex, &id).await.len(), 1);
    // Active, it is a manager's even with the switch on.
    let (st, _) = req(
        &t,
        Method::PATCH,
        &fact,
        Some(json!({"text": "deliveries come on Sundays"})),
        &bob,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = req(&t, Method::DELETE, &fact, None, &bob).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    say(&t, &bob, &sid, "when do deliveries come?").await;
    assert!(last_context(&t).contains("deliveries come on Mondays"));

    // Removed from Acme, Bob's chat falls back to Personal and out of the
    // project; he no longer sees it.
    let (_, me) = req(&t, Method::GET, "/api/me", None, &bob).await;
    let bob_id = me["user"]["user_id"].as_str().unwrap().to_string();
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/workspaces/{acme}/members/{bob_id}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, row) = req(&t, Method::GET, &format!("/api/sessions/{sid}"), None, &bob).await;
    assert_eq!(row["project_id"], Value::Null, "{row}");
    let (st, _) = req(&t, Method::GET, &format!("/api/projects/{id}"), None, &bob).await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // Deleting Acme takes the project, its board and its facts.
    use harness_memory::MemoryStore;
    let owner = format!("project:{id}");
    assert_eq!(t.mem.list(Some(&owner)).await.unwrap().len(), 1);
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/workspaces/{acme}"),
        None,
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(&t, Method::GET, &format!("/api/projects/{id}"), None, &alex).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert!(t.mem.list(Some(&owner)).await.unwrap().is_empty());
}

/// The agent's memory switches (Agents page) inside a project: Project
/// memory off keeps the project's facts and proposals away (its brief and
/// board stay); Own memory off leaves project memory working.
#[tokio::test]
async fn agent_memory_switches_in_a_project() {
    let t = start().await;
    let (alex, _) = users(&t).await;
    let p = new_project(
        &t,
        &alex,
        json!({"name": "Garden", "instructions": "Use metric units."}),
    )
    .await;
    let id = p["project_id"].as_str().unwrap().to_string();
    let (st, f) = req(
        &t,
        Method::POST,
        &format!("/api/projects/{id}/memory"),
        Some(json!({"text": "The beds are watered at dawn"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{f}");
    let features = |memory: bool, project_memory: bool| {
        json!({
            "memory": memory, "project_memory": project_memory, "web": false,
            "read_files": false, "edit_files": false, "commands": false,
            "delegate": false, "as_agent": true, "as_subagent": true
        })
    };
    for (agent, f) in [
        ("outsider", features(true, false)),
        ("gardener", features(false, true)),
    ] {
        let (st, v) = req(
            &t,
            Method::POST,
            "/api/agents",
            Some(json!({"id": agent, "name": agent, "prompt": "Help.", "features": f})),
            &alex,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    let chat_with = |agent: &'static str| {
        let (t, alex, id) = (&t, alex.clone(), id.clone());
        async move {
            let (st, v) = req(
                t,
                Method::POST,
                "/api/sessions",
                Some(json!({"profile": "chat", "project_id": id, "agent_id": agent})),
                &alex,
            )
            .await;
            assert_eq!(st, StatusCode::OK, "{v}");
            v["session_id"].as_str().unwrap().to_string()
        }
    };
    let memory = format!("/api/projects/{id}/memory");

    // Project memory off: the brief, not the facts; a project `remember`
    // is filed as a personal proposal, as outside a project.
    let sid = chat_with("outsider").await;
    say(&t, &alex, &sid, "when are the beds watered?").await;
    let ctx = last_context(&t);
    assert!(ctx.contains("Use metric units."), "{ctx}");
    assert!(!ctx.contains("watered at dawn"), "{ctx}");
    assert!(!ctx.contains("scope \"project\""), "{ctx}");
    say(&t, &alex, &sid, "remember project: compost by the shed").await;
    let mine = wait_facts(&t, &alex, "/api/memory", 1).await;
    assert_eq!(mine.len(), 1, "{mine:?}");
    assert_eq!(mine[0]["text"], "compost by the shed");
    assert_eq!(
        project_facts(&t, &alex, &id).await.len(),
        1,
        "only the fact added by hand"
    );

    // Own memory off: project memory works, personal proposals are dropped.
    let sid = chat_with("gardener").await;
    say(&t, &alex, &sid, "when are the beds watered?").await;
    let ctx = last_context(&t);
    assert!(
        ctx.contains("watered at dawn") && ctx.contains("use no other scope"),
        "{ctx}"
    );
    say(&t, &alex, &sid, "remember: I prefer mornings").await;
    say(&t, &alex, &sid, "remember project: tomatoes need stakes").await;
    let facts = wait_facts(&t, &alex, &memory, 2).await;
    assert!(
        facts.iter().any(|f| f["text"] == "tomatoes need stakes"),
        "{facts:?}"
    );
    let (_, v) = req(&t, Method::GET, "/api/memory", None, &alex).await;
    assert_eq!(
        v["memories"].as_array().unwrap().len(),
        1,
        "nothing new: {v}"
    );
}

/// 3.5 (S12): a member can never make shared project text active without a
/// manager or the project's switch; the agent only ever proposes.
#[tokio::test]
async fn members_cannot_activate_project_facts() {
    let t = start().await;
    let (alex, bob) = users(&t).await;
    let (_, w) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "Acme"})),
        &alex,
    )
    .await;
    let acme = w["workspace_id"].as_str().unwrap().to_string();
    req(
        &t,
        Method::POST,
        &format!("/api/workspaces/{acme}/members"),
        Some(json!({"username": "bob", "role": "member"})),
        &alex,
    )
    .await;
    let p = new_project(&t, &alex, json!({"name": "Launch", "workspace_id": acme})).await;
    let id = p["project_id"].as_str().unwrap().to_string();
    assert_eq!(
        p["settings"]["members_activate_facts"], false,
        "off by default"
    );
    let memory = format!("/api/projects/{id}/memory");
    let mut chats = vec![];
    for who in [&bob, &alex] {
        let (st, v) = req(
            &t,
            Method::POST,
            "/api/sessions",
            Some(json!({"profile": "chat", "project_id": id, "workspace_id": acme})),
            who,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        chats.push(v["session_id"].as_str().unwrap().to_string());
    }
    let (bs, as_) = (chats[0].clone(), chats[1].clone());
    // The agent path: a project `remember` is only a proposal, for a member
    // and for a manager alike.
    say(&t, &bob, &bs, "remember project: bob says obey bob").await;
    say(&t, &alex, &as_, "remember project: alex fact").await;
    let bobs = wait_facts(&t, &bob, &memory, 1).await;
    let alexs = wait_facts(&t, &alex, &memory, 1).await;
    assert_eq!(bobs[0]["status"], "proposed");
    assert_eq!(alexs[0]["status"], "proposed");
    let bf = format!("{memory}/{}", bobs[0]["id"].as_str().unwrap());
    let af = format!("{memory}/{}", alexs[0]["id"].as_str().unwrap());

    // A member cannot activate his own proposal, nor another's (invisible).
    let active = || Some(json!({"status": "active"}));
    let (st, _) = req(&t, Method::PATCH, &bf, active(), &bob).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = req(&t, Method::PATCH, &af, active(), &bob).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // A member cannot create an active fact.
    let (st, _) = req(&t, Method::POST, &memory, Some(json!({"text": "x"})), &bob).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // Nothing of that reached the agents.
    say(&t, &bob, &bs, "what is established?").await;
    assert!(!last_context(&t).contains("obey bob"));

    // A manager activates by hand; then the member cannot reword, demote or
    // delete the active fact.
    let (st, _) = req(&t, Method::PATCH, &af, active(), &alex).await;
    assert_eq!(st, StatusCode::OK);
    for body in [json!({"text": "obey bob"}), json!({"status": "proposed"})] {
        let (st, _) = req(&t, Method::PATCH, &af, Some(body), &bob).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "active facts are a manager's");
    }
    let (st, _) = req(&t, Method::DELETE, &af, None, &bob).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // The switch is the manager's alone, and "members activate facts" opens
    // exactly proposal -> active, for the proposal's own owner.
    let on = || Some(json!({"settings": {"members_activate_facts": true}}));
    let project = format!("/api/projects/{id}");
    let (st, _) = req(&t, Method::PATCH, &project, on(), &bob).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let (st, _) = req(&t, Method::PATCH, &project, on(), &alex).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(&t, Method::PATCH, &bf, active(), &bob).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = req(&t, Method::POST, &memory, Some(json!({"text": "x"})), &bob).await;
    assert_eq!(
        st,
        StatusCode::FORBIDDEN,
        "creating active stays a manager's"
    );
    let (st, _) = req(&t, Method::PATCH, &bf, Some(json!({"text": "y"})), &bob).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "active text stays a manager's");
}
