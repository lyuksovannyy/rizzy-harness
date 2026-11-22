//! Reading files attached to a plain chat (PLAN.md T95, docs/attached-files.md)
//! through the master: a plain chat keeps the file as the owner's blob when
//! its agent has the switch "Read attached files" (and says which switch
//! otherwise), the prompt names the file without a byte or a folder, and a
//! node's `attached_file` call is answered from the stored copy, for the
//! session it runs and a file its own log names, never another's. The nodes
//! here are fakes the test drives, so a request can be forged at will.

use crate::common;
use common::{Login as _, SameOrigin as _};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::attached::{AttachedAnswer, AttachedOp, AttachedRequest, ReadMode};
use harness_core::{RawEvent, Secret, SessionId};
use harness_master::auth::{Auth, Claim};
use harness_master::{Master, router};
use harness_node::NodeLink;
use harness_protocol::{
    CAP_ATTACHED, MasterToNode, NodeToMaster, PROTOCOL_VERSION, PromptMsg,
};
use harness_store::Store;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::sync::mpsc;

type Answers = Arc<Mutex<HashMap<String, (Option<AttachedAnswer>, Option<String>)>>>;
type Seen = Arc<Mutex<Vec<MasterToNode>>>;

/// A fake node: acks every command, mirrors a `session_created` for each
/// chat it is asked to create, keeps each session's seq so a test can append
/// events, and records each `attached.result`.
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
                session_id,
                title,
                cwd,
                incognito,
                ..
            } = &msg
            {
                let payload = json!({"title": title, "cwd": cwd, "model": "m", "backend": "native",
                    "node_id": node, "profile": "chat", "approval_mode": "ask",
                    "incognito": incognito});
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
            if let MasterToNode::AttachedResult {
                req_id,
                answer,
                error,
            } = &msg
            {
                answers
                    .lock()
                    .unwrap()
                    .insert(req_id.clone(), (answer.clone(), error.clone()));
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

    /// The newest prompt `sid` got.
    fn last_prompt(&self, sid: SessionId) -> PromptMsg {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|m| match m {
                MasterToNode::Prompt(p) if p.session_id == sid => Some(p.clone()),
                _ => None,
            })
            .expect("no prompt for the session")
    }

    /// Act as an honest node does with the newest prompt of `sid`: its turn
    /// starts with the agent as the master specified it (what it withheld
    /// included) and its message is recorded with the files it names.
    async fn take_prompt(&self, sid: SessionId, turn: &str) {
        let p = self.last_prompt(sid);
        let agent = p.agent.as_ref().map(|a| a.agent_ref());
        self.emit(sid, "turn_started", json!({"turn_id": turn, "agent": agent}))
            .await;
        let attached: Vec<Value> = p
            .attached
            .iter()
            .map(|a| {
                json!({"file_id": a.file_id, "name": a.name, "bytes": a.size,
                       "sha256": a.sha256, "mime": a.mime})
            })
            .collect();
        self.emit(
            sid,
            "user_message",
            json!({"turn_id": turn, "text": p.text, "attached": attached}),
        )
        .await;
    }

    /// Ask on behalf of `sid` and wait for the answer.
    async fn ask(&self, sid: SessionId, request: AttachedRequest) -> Result<AttachedAnswer, String> {
        let req_id = harness_core::new_id();
        self.tx
            .send(NodeToMaster::Attached {
                req_id: req_id.clone(),
                session_id: sid,
                request,
            })
            .await
            .unwrap();
        for _ in 0..500 {
            if let Some((a, e)) = self.answers.lock().unwrap().get(&req_id) {
                return match e {
                    Some(e) => Err(e.clone()),
                    None => Ok(a.clone().expect("an answer")),
                };
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no attached.result for {req_id}");
    }

    /// Whether any answer came for a request that was sent.
    async fn nothing_came(&self, sid: SessionId, request: AttachedRequest) -> bool {
        let before = self.answers.lock().unwrap().len();
        self.tx
            .send(NodeToMaster::Attached {
                req_id: harness_core::new_id(),
                session_id: sid,
                request,
            })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        self.answers.lock().unwrap().len() == before
    }
}

struct T {
    base: String,
    master: Master,
    /// The chat runtime (announces the capability unless a test says not).
    chat: Fake,
    dir: tempfile::TempDir,
}

async fn start() -> T {
    start_with(&["profile:chat", CAP_ATTACHED]).await
}

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
    let mut cfg = harness_master::config::Config::default();
    cfg.media.enabled = true;
    let (rt, _) =
        harness_master::providers::ProviderRuntime::start(cfg, dir.path(), Default::default())
            .unwrap();
    master.set_provider_runtime(Arc::new(rt));
    harness_master::storage::open(&master, dir.path()).unwrap();
    let (link, chat) = fake_node("master", caps);
    master.add_link(link, true).await.unwrap();
    let m2 = master.clone();
    tokio::spawn(async move { axum::serve(listener, router(m2)).await.unwrap() });
    T {
        base: format!("http://{addr}"),
        master,
        chat,
        dir,
    }
}

async fn req(t: &T, method: Method, path: &str, body: Option<Value>, c: &str) -> (StatusCode, Value) {
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
    let pw = req(t, Method::POST, "/api/users", Some(json!({"username": "bob"})), &alex)
        .await
        .1["password"]
        .as_str()
        .unwrap()
        .to_string();
    let first = common::signed_in(
        post_anon(t, "/api/login", json!({"username": "bob", "password": pw}), None).await,
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

async fn chat(t: &T, c: &str, extra: Value) -> SessionId {
    let mut body = json!({"profile": "chat"});
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    let (st, v) = req(t, Method::POST, "/api/sessions", Some(body), c).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().parse().unwrap()
}

/// Wait until the mirror has `sid` at the node's latest seq.
async fn landed(t: &T, sid: SessionId) {
    let want = *t.chat.seqs.lock().unwrap().get(&sid).unwrap();
    for _ in 0..300 {
        if t.master.inner_mirror().head_seq(sid).unwrap_or(0) >= want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("events of {sid} did not land");
}

async fn upload(t: &T, who: &str, name: &str, query: &str, bytes: Vec<u8>) -> (StatusCode, Value) {
    let r = reqwest::Client::new()
        .post(format!(
            "{}/api/attachments?name={}{query}",
            t.base,
            name.replace(' ', "%20")
        ))
        .login(who)
        .same_origin()
        .body(bytes)
        .send()
        .await
        .unwrap();
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn send(t: &T, who: &str, sid: SessionId, ids: &[&str], extra: Value) -> (StatusCode, Value) {
    let mut body = json!({"text": "what is in it?", "attachments": ids});
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    req(t, Method::POST, &format!("/api/sessions/{sid}/prompt"), Some(body), who).await
}

/// A zip with stored entries, made by hand: a text file, a binary one.
fn zip() -> Vec<u8> {
    let ents: [(&str, &[u8]); 3] = [
        ("hello.txt", b"one\ntwo\nthree\nsecret=sk-live-9f8e7d6c5b4a39281706\n"),
        ("bin/blob.dat", &[0, 1, 2, 3, 250, 251, 252]),
        ("../../escape.txt", b"x"),
    ];
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in ents {
        let at = out.len() as u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 8]);
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&at.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let off = out.len() as u32;
    let n = 3u16;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn blob_files(t: &T) -> usize {
    let mut n = 0;
    let root = t.dir.path().join("blobs");
    for shard in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        if shard.file_name() == "tmp" {
            continue;
        }
        n += std::fs::read_dir(shard.path()).map_or(0, |d| d.count());
    }
    n
}

fn ask(op: AttachedOp, id: &str) -> AttachedRequest {
    AttachedRequest {
        op,
        file_id: id.into(),
        entry: None,
        offset: None,
        limit: None,
        mode: None,
    }
}

/// Attach `zip()` to a new chat of `who`, send it and have the fake node
/// record the message and open the turn: the chat, the file's id.
async fn with_file(t: &T, who: &str, turn: &str) -> (SessionId, String) {
    let sid = chat(t, who, json!({})).await;
    let (st, v) = upload(t, who, "Q3 report.zip", &format!("&session={sid}"), zip()).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let (st, v) = send(t, who, sid, &[&id], json!({})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    t.chat.take_prompt(sid, turn).await;
    landed(t, sid).await;
    (sid, id)
}

// ------------------------------------------------------------------

/// The whole path: a plain chat keeps the zip (the response says kept, not
/// saved), the prompt names it by reference with no byte pushed and no
/// folder, the node records it, and the master serves it from then on.
#[tokio::test]
async fn a_plain_chat_keeps_a_zip_and_the_prompt_names_it_without_a_byte() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = chat(&t, &alex, json!({})).await;
    let bytes = zip();
    let (st, v) = upload(&t, &alex, "Q3 report.zip", &format!("&session={sid}"), bytes.clone()).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["kept"], json!(true), "{v}");
    assert!(v.get("saved").is_none(), "{v}");
    assert_eq!(v["bytes"], json!(bytes.len()));
    assert_eq!(blob_files(&t), 1, "the bytes are a stored file");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let (st, v) = send(&t, &alex, sid, &[&id], json!({})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let p = t.chat.last_prompt(sid);
    assert!(p.uploads.is_empty(), "nothing is pushed to a folder");
    assert_eq!(p.attached.len(), 1);
    let a = &p.attached[0];
    assert_eq!(
        (a.file_id.as_str(), a.name.as_str(), a.size),
        (id.as_str(), "Q3 report.zip", bytes.len() as u64)
    );
    assert!(
        !t.chat
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|m| matches!(m, MasterToNode::UploadChunk { .. })),
        "no byte travels to the node"
    );
    assert!(p.agent.as_ref().unwrap().allows_tool("attached_file"));
    // Not served before the chat's log names it; served after.
    t.chat.take_prompt(sid, "t1").await;
    landed(&t, sid).await;
    let got = t.chat.ask(sid, ask(AttachedOp::Info, &id)).await.unwrap();
    match got {
        AttachedAnswer::Info { name, bytes: n, label, archive, .. } => {
            assert_eq!((name.as_str(), n, label.as_str()), ("Q3 report.zip", bytes.len() as u64, "ZIP archive"));
            assert!(archive);
        }
        other => panic!("{other:?}"),
    }
    // The upload row was used: the same id is not offered again.
    let (st, v) = send(&t, &alex, sid, &[&id], json!({})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
}

/// The tool, end to end over the link: the listing, a text entry as
/// numbered lines (the entry named as the listing shows it), a binary
/// one as a hex dump, and an entry whose path tries to leave.
#[tokio::test]
async fn list_and_read_entries_of_the_zip() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let (sid, id) = with_file(&t, &alex, "t1").await;
    let AttachedAnswer::List { entries, complete, more, .. } =
        t.chat.ask(sid, ask(AttachedOp::List, &id)).await.unwrap()
    else {
        panic!("not a list")
    };
    let paths: Vec<_> = entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["hello.txt", "bin/blob.dat", "../../escape.txt"]);
    assert!(complete && !more);
    let mut r = ask(AttachedOp::Read, &id);
    r.entry = Some("hello.txt".into());
    r.limit = Some(2);
    let AttachedAnswer::Read { mode, text, next, .. } = t.chat.ask(sid, r.clone()).await.unwrap() else {
        panic!()
    };
    assert_eq!((mode.as_str(), text.as_str(), next), ("text", "1\tone\n2\ttwo", Some(3)));
    r.entry = Some("bin/blob.dat".into());
    r.limit = None;
    let AttachedAnswer::Read { mode, text, .. } = t.chat.ask(sid, r.clone()).await.unwrap() else {
        panic!()
    };
    assert_eq!(mode, "hex");
    assert!(text.starts_with("00000000  00 01 02 03 fa fb fc"), "{text}");
    r.mode = Some(ReadMode::Strings);
    r.entry = Some("hello.txt".into());
    r.limit = None;
    let AttachedAnswer::Read { mode, text, .. } = t.chat.ask(sid, r).await.unwrap() else { panic!() };
    assert_eq!(mode, "strings");
    assert!(text.contains("three") && !text.contains("one"), "{text}");
    // A path that tries to leave names an entry, nothing more.
    let mut r = ask(AttachedOp::Read, &id);
    r.entry = Some("../../escape.txt".into());
    let AttachedAnswer::Read { text, .. } = t.chat.ask(sid, r).await.unwrap() else { panic!() };
    assert_eq!(text, "1\tx");
    // Not an entry of the archive.
    let mut r = ask(AttachedOp::Read, &id);
    r.entry = Some("/etc/passwd".into());
    let e = t.chat.ask(sid, r).await.unwrap_err();
    assert!(e.contains("no entry"), "{e}");
}

/// Which file a call may reach: only an id its own chat's log names, never
/// another person's, another chat's of the same person, an invented one, a
/// sub-agent's chat's; a fork reaches it by its own row.
#[tokio::test]
async fn a_call_reaches_only_the_files_its_own_log_names() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let (a_chat, a_id) = with_file(&t, &alex, "t1").await;
    let (b_chat, b_id) = with_file(&t, &bob, "t1").await;
    // Bob's chat asking for Alex's file id, and the reverse.
    for (sid, id) in [(b_chat, &a_id), (a_chat, &b_id)] {
        let e = t.chat.ask(sid, ask(AttachedOp::Info, id)).await.unwrap_err();
        assert!(e.contains("no such file in this chat"), "{e}");
    }
    // Another chat of the same person, with a file of its own and its turn
    // open: it reaches that one and not the first chat's.
    let (other, o_id) = with_file(&t, &alex, "t1").await;
    assert_ne!(o_id, a_id);
    let e = t.chat.ask(other, ask(AttachedOp::Info, &a_id)).await.unwrap_err();
    assert!(e.contains("no such file in this chat"), "{e}");
    assert!(t.chat.ask(other, ask(AttachedOp::Info, &o_id)).await.is_ok());
    // And one with no file at all is not offered the tool.
    let bare = chat(&t, &alex, json!({})).await;
    send(&t, &alex, bare, &[], json!({})).await;
    t.chat.take_prompt(bare, "t1").await;
    landed(&t, bare).await;
    let e = t.chat.ask(bare, ask(AttachedOp::Info, &a_id)).await.unwrap_err();
    assert!(e.contains("not available"), "{e}");
    // Ids that cannot be ids, and ones nobody sent.
    for bad in ["", "../x", "a b", &"x".repeat(200), "invented-id"] {
        let e = t.chat.ask(a_chat, ask(AttachedOp::Info, bad)).await.unwrap_err();
        assert!(e.contains("no such file in this chat"), "{bad}: {e}");
    }
    // A session that is not this node's.
    let (link, stranger) = fake_node("stranger", &["profile:agentic", "exec:sandbox", CAP_ATTACHED]);
    t.master.add_link(link, false).await.unwrap();
    let e = stranger.ask(a_chat, ask(AttachedOp::Info, &a_id)).await.unwrap_err();
    assert!(e.contains("no such session"), "{e}");
    // A sub-agent's chat: its own log has no file, and its parent's is not its.
    let child = SessionId::new();
    t.chat.seqs.lock().unwrap().insert(child, 0);
    t.chat
        .emit(
            child,
            "session_created",
            json!({"title": "c", "cwd": "/w", "model": "m", "backend": "native", "node_id": "master",
                   "profile": "chat", "approval_mode": "ask", "parent_session": a_chat.to_string()}),
        )
        .await;
    landed(&t, child).await;
    let e = t.chat.ask(child, ask(AttachedOp::Info, &a_id)).await.unwrap_err();
    assert!(e.contains("sub-agent"), "{e}");
    // The real one still works.
    assert!(t.chat.ask(a_chat, ask(AttachedOp::Info, &a_id)).await.is_ok());
}

/// A fork is a chat of its own that keeps the messages it copies, and with
/// them the files: it reads the file by its own row for the same blob, and
/// deleting the parent does not take it away.
#[tokio::test]
async fn a_fork_reads_the_same_blob_and_the_last_chat_frees_it() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let (p, id) = with_file(&t, &alex, "t1").await;
    t.chat.emit(p, "assistant_message", json!({"turn_id": "t1", "message_id": "m1", "text": "ok"})).await;
    t.chat
        .emit(p, "turn_ended", json!({"turn_id": "t1", "reason": "completed"}))
        .await;
    landed(&t, p).await;
    let events = t.master.inner_mirror().events_after(p, 0, 100).unwrap();
    let through = events.len() as u64;
    let f = SessionId::new();
    t.chat.seqs.lock().unwrap().insert(f, 0);
    t.chat
        .emit(
            f,
            "session_created",
            json!({"title": "c (fork)", "cwd": "/w", "model": "m", "backend": "native",
                   "node_id": "master", "profile": "chat", "approval_mode": "ask",
                   "forked": {"parent": p.to_string(), "through_seq": through}}),
        )
        .await;
    for e in events.iter().skip(1) {
        t.chat.emit(f, &e.kind, e.payload.clone()).await;
    }
    landed(&t, f).await;
    t.master.inner_mirror().claim_session(f, Some(&owner_of(&t, p)), None).unwrap();
    // The fork's next turn.
    t.chat.emit(f, "turn_started", json!({"turn_id": "t2"})).await;
    landed(&t, f).await;
    assert!(t.chat.ask(f, ask(AttachedOp::Info, &id)).await.is_err(), "no agent on the open turn");
    t.chat
        .emit(f, "turn_ended", json!({"turn_id": "t2", "reason": "completed"}))
        .await;
    send(&t, &alex, f, &[], json!({})).await;
    t.chat.take_prompt(f, "t3").await;
    landed(&t, f).await;
    let got = t.chat.ask(f, ask(AttachedOp::Info, &id)).await.unwrap();
    assert!(matches!(got, AttachedAnswer::Info { .. }), "{got:?}");
    // Delete the parent: the fork still reads, the blob stays.
    let (st, _) = req(&t, Method::DELETE, &format!("/api/sessions/{p}"), None, &alex).await;
    assert_eq!(st, StatusCode::OK);
    assert!(t.chat.ask(f, ask(AttachedOp::Info, &id)).await.is_ok());
    assert!(t.master.inner_mirror().blobs_unreferenced(i64::MAX).unwrap().is_empty());
    assert_eq!(blob_files(&t), 1);
    // The last chat goes: the blob is a leftover the sweep takes.
    let (st, _) = req(&t, Method::DELETE, &format!("/api/sessions/{f}"), None, &alex).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(t.master.inner_mirror().blobs_unreferenced(i64::MAX).unwrap().len(), 1);
}

fn owner_of(t: &T, sid: SessionId) -> String {
    t.master
        .inner_mirror()
        .session(sid)
        .unwrap()
        .unwrap()
        .owner_id
        .unwrap()
}

/// The switch: a chat whose agent lacks "Read attached files" refuses the
/// file at attach time and at send, naming the switch, and the upload is
/// given back at send; the tool is withheld from a turn of such an agent and
/// from a chat without any file.
#[tokio::test]
async fn the_switch_decides_at_attach_at_send_and_in_the_turn() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = chat(&t, &alex, json!({})).await;
    // At attach: Researcher has no such switch.
    let (st, v) = upload(&t, &alex, "a.zip", &format!("&session={sid}&agent=researcher"), zip()).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    let why = v["error"].as_str().unwrap();
    assert!(why.contains("Read attached files") && why.contains("Researcher"), "{why}");
    assert_eq!(blob_files(&t), 0, "nothing is stored");
    // At the home composer too (no chat yet).
    let (st, v) = upload(&t, &alex, "a.zip", "&agent=researcher", zip()).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    let (st, _) = upload(&t, &alex, "a.zip", "&agent=general", zip()).await;
    assert_eq!(st, StatusCode::OK);
    // At send: attached with the default agent, sent to another.
    let (st, v) = upload(&t, &alex, "b.zip", &format!("&session={sid}"), zip()).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let (st, v) = send(&t, &alex, sid, &[&id], json!({"agent_id": "researcher"})).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"].as_str().unwrap().contains("Read attached files"), "{v}");
    assert!(
        !t.chat.seen.lock().unwrap().iter().any(|m| matches!(m, MasterToNode::Prompt(_))),
        "nothing reached the node"
    );
    // Given back: the same file goes with an agent that has the switch.
    let (st, v) = send(&t, &alex, sid, &[&id], json!({"agent_id": "general"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    t.chat.take_prompt(sid, "t1").await;
    landed(&t, sid).await;
    assert!(t.chat.ask(sid, ask(AttachedOp::Info, &id)).await.is_ok());
    t.chat.emit(sid, "turn_ended", json!({"turn_id": "t1", "reason": "completed"})).await;
    // A later turn of an agent without the switch: the tool is withheld and
    // the service refuses it from the log.
    let (st, v) = send(&t, &alex, sid, &[], json!({"agent_id": "researcher"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let p = t.chat.last_prompt(sid);
    assert!(!p.agent.as_ref().unwrap().allows_tool("attached_file"));
    t.chat.take_prompt(sid, "t2").await;
    landed(&t, sid).await;
    let e = t.chat.ask(sid, ask(AttachedOp::Info, &id)).await.unwrap_err();
    assert!(e.contains("not available"), "{e}");
    t.chat.emit(sid, "turn_ended", json!({"turn_id": "t2", "reason": "completed"})).await;
    // A chat that never had a file is not offered the tool.
    let bare = chat(&t, &alex, json!({})).await;
    send(&t, &alex, bare, &[], json!({})).await;
    assert!(!t.chat.last_prompt(bare).agent.unwrap().allows_tool("attached_file"));
}

/// A node that does not announce the capability: the chat refuses the file
/// at attach time (the old reason), and a request from it is not answered.
#[tokio::test]
async fn an_old_chat_runtime_is_refused_cleanly() {
    let t = start_with(&["profile:chat"]).await;
    let [alex, _] = users(&t).await;
    let sid = chat(&t, &alex, json!({})).await;
    let (st, v) = upload(&t, &alex, "a.zip", &format!("&session={sid}"), zip()).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert!(v["error"].as_str().unwrap().contains("cannot open files"), "{v}");
    assert_eq!(blob_files(&t), 0);
    assert!(t.chat.nothing_came(sid, ask(AttachedOp::Info, "x")).await);
}

/// A temporary chat keeps no file, and a file that is never sent is purged
/// with the unsent ones, its blob swept with it.
#[tokio::test]
async fn a_temporary_chat_keeps_none_and_an_unsent_file_is_purged() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let temp = chat(&t, &alex, json!({"incognito": true})).await;
    let (st, v) = upload(&t, &alex, "a.zip", &format!("&session={temp}"), zip()).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert!(v["error"].as_str().unwrap().contains("temporary"), "{v}");
    // Unsent: purged by age, then its blob is a leftover.
    let sid = chat(&t, &alex, json!({})).await;
    let (st, _) = upload(&t, &alex, "a.zip", &format!("&session={sid}"), zip()).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(blob_files(&t), 1);
    let s = t.master.inner_mirror();
    assert!(s.blobs_unreferenced(i64::MAX).unwrap().is_empty());
    assert_eq!(s.chat_uploads_purge(i64::MAX).unwrap(), 1);
    assert_eq!(s.blobs_unreferenced(i64::MAX).unwrap().len(), 1);
}

/// The bounds that were there still hold for a plain chat: at most 10 files
/// and images in a message, at most 20 unsent files an account holds, and
/// the account's quota counts the blob.
#[tokio::test]
async fn the_limits_hold_in_a_plain_chat() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = chat(&t, &alex, json!({})).await;
    let q = format!("&session={sid}");
    let mut ids = Vec::new();
    for i in 0..20 {
        // Different bytes, so no two share a blob.
        let mut z = zip();
        z.extend_from_slice(format!("pad{i}").as_bytes());
        let (st, v) = upload(&t, &alex, &format!("f{i}.zip"), &q, z).await;
        assert_eq!(st, StatusCode::OK, "{i}: {v}");
        ids.push(v["attachment_id"].as_str().unwrap().to_string());
    }
    let (st, v) = upload(&t, &alex, "one-more.zip", &q, zip()).await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "{v}");
    let refs: Vec<&str> = ids.iter().take(11).map(String::as_str).collect();
    let (st, v) = send(&t, &alex, sid, &refs, json!({})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].as_str().unwrap().contains("at most 10"), "{v}");
    // Over 10 MiB: refused with the number.
    let (st, v) = upload(&t, &alex, "big.zip", &q, vec![b'P'; 10 * 1024 * 1024 + 1]).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("10 MB"), "{v}");
    // Ten go.
    let refs: Vec<&str> = ids.iter().take(10).map(String::as_str).collect();
    let (st, v) = send(&t, &alex, sid, &refs, json!({})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(t.chat.last_prompt(sid).attached.len(), 10);
}

/// A file gone from the disk (the chat deleted while a read ran, a
/// restored data directory) is a sentence, not a crash.
#[tokio::test]
async fn a_file_removed_while_it_is_read_is_a_sentence() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let (sid, id) = with_file(&t, &alex, "t1").await;
    for shard in std::fs::read_dir(t.dir.path().join("blobs")).unwrap().flatten() {
        if shard.file_name() == "tmp" {
            continue;
        }
        for f in std::fs::read_dir(shard.path()).unwrap().flatten() {
            std::fs::remove_file(f.path()).unwrap();
        }
    }
    let e = t.chat.ask(sid, ask(AttachedOp::Info, &id)).await.unwrap_err();
    assert!(e.contains("the file is gone"), "{e}");
}

/// A zip that unpacks to far more than it says: a read is bounded by what
/// it asks for, not by what the archive holds.
#[tokio::test]
async fn a_zip_bomb_is_read_in_a_window() {
    use std::io::Write;
    let t = start().await;
    let [alex, _] = users(&t).await;
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(&vec![0u8; 10 << 20]).unwrap();
    let packed = e.finish().unwrap();
    let name = b"zeros.bin";
    let mut z = Vec::new();
    z.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    z.extend_from_slice(&[20, 0, 0, 0, 8, 0, 0, 0, 0, 0]);
    z.extend_from_slice(&0u32.to_le_bytes());
    z.extend_from_slice(&(packed.len() as u32).to_le_bytes());
    z.extend_from_slice(&(10u32 << 20).to_le_bytes());
    z.extend_from_slice(&(name.len() as u16).to_le_bytes());
    z.extend_from_slice(&0u16.to_le_bytes());
    z.extend_from_slice(name);
    z.extend_from_slice(&packed);
    let off = z.len() as u32;
    let mut c = Vec::new();
    c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    c.extend_from_slice(&[20, 0, 20, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    c.extend_from_slice(&(packed.len() as u32).to_le_bytes());
    c.extend_from_slice(&(10u32 << 20).to_le_bytes());
    c.extend_from_slice(&(name.len() as u16).to_le_bytes());
    c.extend_from_slice(&[0; 8]);
    c.extend_from_slice(&0u32.to_le_bytes());
    c.extend_from_slice(&0u32.to_le_bytes());
    c.extend_from_slice(name);
    z.extend_from_slice(&c);
    z.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    z.extend_from_slice(&[0; 4]);
    z.extend_from_slice(&1u16.to_le_bytes());
    z.extend_from_slice(&1u16.to_le_bytes());
    z.extend_from_slice(&(c.len() as u32).to_le_bytes());
    z.extend_from_slice(&off.to_le_bytes());
    z.extend_from_slice(&0u16.to_le_bytes());
    let sid = chat(&t, &alex, json!({})).await;
    let (st, v) = upload(&t, &alex, "bomb.zip", &format!("&session={sid}"), z).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    send(&t, &alex, sid, &[&id], json!({})).await;
    t.chat.take_prompt(sid, "t1").await;
    landed(&t, sid).await;
    let mut r = ask(AttachedOp::Read, &id);
    r.entry = Some("zeros.bin".into());
    let started = std::time::Instant::now();
    let AttachedAnswer::Read { mode, text, shown, .. } = t.chat.ask(sid, r).await.unwrap() else { panic!() };
    assert_eq!((mode.as_str(), shown), ("hex", 512));
    assert!(text.len() < 8192);
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// The account's quota counts a kept file like any blob of theirs: the
/// second 600 KB file does not fit a 1 MB quota and is refused with the
/// numbers, nothing stored for it.
#[tokio::test]
async fn the_account_quota_counts_a_kept_file() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let (st, v) = req(
        &t,
        Method::PUT,
        "/api/admin/storage",
        Some(json!({"max_file_mb": 10, "account_quota_mb": 1, "server_quota_mb": 100, "min_free_mb": 0})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = chat(&t, &alex, json!({})).await;
    let q = format!("&session={sid}");
    let mut big = zip();
    big.extend(std::iter::repeat_n(7u8, 600_000));
    let (st, v) = upload(&t, &alex, "a.zip", &q, big.clone()).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    big[100] ^= 0xff;
    let (st, v) = upload(&t, &alex, "b.zip", &q, big).await;
    assert_eq!(st, StatusCode::INSUFFICIENT_STORAGE, "{v}");
    assert_eq!(blob_files(&t), 1);
}
