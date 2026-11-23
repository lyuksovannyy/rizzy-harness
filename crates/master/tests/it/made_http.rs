//! Files an agent makes in a plain chat (PLAN.md T97, README "Documents the
//! agent makes"): `doc_create` and `sheet_create` become a `docs.request`
//! the master answers from the model's own text, keeping the file as the
//! owner's blob for that chat; it is downloadable by the owner only once the
//! chat's own log names it, and the agent can read it again with
//! `attached_file`. The nodes here are fakes the test drives, so a request
//! can be forged at will.

use super::attached_http::{T, chat, landed, req, send, start_with, users};
use crate::common::{Login as _, SameOrigin as _};

use std::time::{Duration, Instant};

use harness_attached::Package;
use harness_core::SessionId;
use harness_core::attached::{AttachedOp, AttachedRequest};
use harness_core::docs::{DocCreated, DocRequest};
use harness_protocol::{CAP_ATTACHED, CAP_DOCS};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

pub(crate) async fn start() -> T {
    start_with(&["profile:chat", CAP_ATTACHED, CAP_DOCS]).await
}

pub(crate) fn doc(name: &str, md: &str) -> DocRequest {
    DocRequest::Doc {
        name: name.into(),
        markdown: md.into(),
        page: None,
        landscape: false,
        title: None,
    }
}

fn sheet(name: &str, spec: Value) -> DocRequest {
    DocRequest::Sheet {
        name: name.into(),
        spec,
    }
}

/// A plain chat of `who` with a turn open the way an honest node opens it.
pub(crate) async fn open(t: &T, who: &str, turn: &str, extra: Value) -> SessionId {
    let sid = chat(t, who, json!({})).await;
    next_turn(t, who, sid, turn, extra).await;
    sid
}

async fn next_turn(t: &T, who: &str, sid: SessionId, turn: &str, extra: Value) {
    let (st, v) = send(t, who, sid, &[], extra).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    t.chat.take_prompt(sid, turn).await;
    landed(t, sid).await;
}

/// The chat's own log names the file, as the node's loop writes it.
pub(crate) async fn name_in_log(t: &T, sid: SessionId, c: &DocCreated) {
    t.chat
        .emit(
            sid,
            "tool_call_finished",
            json!({"call_id": "c1", "ok": true, "output": "made",
                   "made": {"name": c.name, "size": c.bytes, "sha256": c.sha256, "file_id": c.file_id}}),
        )
        .await;
    landed(t, sid).await;
}

async fn download(t: &T, who: &str, sid: SessionId, file_id: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!("{}/api/sessions/{sid}/made/{file_id}", t.base))
        .login(who)
        .same_origin()
        .send()
        .await
        .unwrap()
}

pub(crate) fn owner_of(t: &T, sid: SessionId) -> String {
    t.master
        .inner_mirror()
        .session(sid)
        .unwrap()
        .unwrap()
        .owner_id
        .unwrap()
}

pub(crate) fn blob_paths(t: &T) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for shard in std::fs::read_dir(t.dir.path().join("blobs"))
        .into_iter()
        .flatten()
        .flatten()
    {
        if shard.file_name() == "tmp" {
            continue;
        }
        for f in std::fs::read_dir(shard.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            out.push(f.path());
        }
    }
    out
}

fn package(bytes: &[u8], part: &str) -> String {
    let p = Package::open(bytes, Instant::now() + Duration::from_secs(10)).unwrap();
    String::from_utf8(p.read_small(part, 8 << 20).unwrap()).unwrap()
}

// ------------------------------------------------------------------

/// The whole path: the request makes a real docx kept as the owner's
/// encrypted blob and counted in their quota; it cannot be downloaded
/// before the chat's own log names it, is an attachment of unknown type
/// after, has the bytes the answer's hash says, and the agent reads it
/// again with `attached_file` in its next turn.
#[tokio::test]
async fn a_file_is_made_kept_named_by_the_log_and_downloaded_by_its_owner() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let c = t
        .chat
        .ask_docs(sid, doc("plan", "# The plan\n\n**Bold** text."))
        .await
        .unwrap();
    assert_eq!(c.name, "plan.docx");
    assert_eq!(c.mime, harness_core::docs::DOCX_MIME);
    assert!(c.notes[0].contains("paragraphs"), "{:?}", c.notes);
    assert_eq!(c.file_id.len(), 32);
    // Kept as an encrypted blob, counted in the owner's quota.
    let files = blob_paths(&t);
    assert_eq!(files.len(), 1);
    let raw = std::fs::read(&files[0]).unwrap();
    assert!(raw.len() as u64 >= c.bytes);
    assert!(
        !raw.windows(8).any(|w| w == b"word/doc"),
        "the blob is not stored in the clear"
    );
    let owner = owner_of(&t, sid);
    let used = t
        .master
        .inner_mirror()
        .blob_usage(Some(&owner))
        .unwrap()
        .used;
    assert_eq!(used, c.bytes);
    // Not the chat's until its log names it.
    assert_eq!(
        download(&t, &alex, sid, &c.file_id).await.status(),
        StatusCode::NOT_FOUND
    );
    name_in_log(&t, sid, &c).await;
    let r = download(&t, &alex, sid, &c.file_id).await;
    assert_eq!(r.status(), StatusCode::OK);
    let h = r.headers().clone();
    assert_eq!(h["content-type"], "application/octet-stream");
    let d = h["content-disposition"].to_str().unwrap();
    assert!(d.starts_with("attachment; filename=\"plan.docx\""), "{d}");
    assert_eq!(h["x-content-sha256"].to_str().unwrap(), c.sha256);
    assert_eq!(h["x-content-type-options"], "nosniff");
    let bytes = r.bytes().await.unwrap().to_vec();
    assert_eq!(bytes.len() as u64, c.bytes);
    use sha2::{Digest, Sha256};
    assert_eq!(hex::encode(Sha256::digest(&bytes)), c.sha256);
    let body = package(&bytes, "word/document.xml");
    assert!(
        body.contains("The plan") && body.contains("<w:b/>"),
        "{body}"
    );
    assert!(package(&bytes, "[Content_Types].xml").contains("wordprocessingml.document.main+xml"));

    // The next turn: the file is the chat's own, so the agent has the read
    // tool (the chat has a file its log names) and reads it again.
    t.chat
        .emit(
            sid,
            "turn_ended",
            json!({"turn_id": "t1", "reason": "completed"}),
        )
        .await;
    next_turn(&t, &alex, sid, "t2", json!({})).await;
    assert!(
        t.chat
            .last_prompt(sid)
            .agent
            .unwrap()
            .allows_tool("attached_file")
    );
    let got = t
        .chat
        .ask(
            sid,
            AttachedRequest {
                op: AttachedOp::Info,
                file_id: c.file_id.clone(),
                entry: None,
                offset: None,
                limit: None,
                mode: None,
            },
        )
        .await
        .unwrap();
    match got {
        harness_core::attached::AttachedAnswer::Info {
            name,
            archive,
            label,
            ..
        } => {
            assert_eq!(name, "plan.docx");
            assert!(archive, "a docx is a zip");
            assert_eq!(label, "ZIP archive");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_sheet_is_made_with_a_name_the_tool_chooses() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let c = t
        .chat
        .ask_docs(
            sid,
            sheet(
                "../../etc/Budget.XLSX",
                json!({"sheets": [{"name": "Q1", "columns": [{"header": "Item"}, {"header": "Cost", "format": "currency"}],
                                   "rows": [["Hosting", 12.5], ["Total", {"f": "SUM(B2:B2)"}]]}]}),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        (c.name.as_str(), c.mime.as_str()),
        ("Budget.xlsx", harness_core::docs::XLSX_MIME)
    );
    assert!(
        c.notes
            .iter()
            .any(|n| n.contains("calculated when the file is opened")),
        "{:?}",
        c.notes
    );
    name_in_log(&t, sid, &c).await;
    let bytes = download(&t, &alex, sid, &c.file_id)
        .await
        .bytes()
        .await
        .unwrap();
    let wb = package(&bytes, "xl/workbook.xml");
    assert!(
        wb.contains("name=\"Q1\"") && wb.contains("fullCalcOnLoad=\"1\""),
        "{wb}"
    );
    assert!(package(&bytes, "xl/worksheets/sheet1.xml").contains("<f>SUM(B2:B2)</f>"));
}

/// The refusals come back as sentences and leave nothing behind.
#[tokio::test]
async fn a_refused_request_leaves_no_file_and_says_why() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    for (req, why) in [
        (doc("a", ""), "empty"),
        (doc("a", "<div>x</div>"), "no content"),
        (
            doc(
                "a",
                &"x".repeat(harness_docs::limits::MAX_MARKDOWN_BYTES + 1),
            ),
            "larger",
        ),
        (
            sheet(
                "a",
                json!({"sheets": [{"rows": [[{"f": "WEBSERVICE(\"http://x\")"}]]}]}),
            ),
            "WEBSERVICE",
        ),
        (sheet("a", json!({"sheets": []})), "no sheet"),
        (
            DocRequest::Doc {
                name: "a".into(),
                markdown: "x".into(),
                page: Some("a3".into()),
                landscape: false,
                title: None,
            },
            "a4 or letter",
        ),
    ] {
        let e = t.chat.ask_docs(sid, req).await.unwrap_err();
        assert!(e.contains(why), "{why}: {e}");
    }
    assert!(blob_paths(&t).is_empty(), "nothing is stored");
}

/// Chats are private to their owner, admins too; a file id is only ever a
/// file of its own chat.
#[tokio::test]
async fn another_persons_chat_and_another_chats_file_id_cannot_download() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let a1 = open(&t, &alex, "t1", json!({})).await;
    let c = t.chat.ask_docs(a1, doc("mine", "# Private")).await.unwrap();
    name_in_log(&t, a1, &c).await;
    assert_eq!(
        download(&t, &alex, a1, &c.file_id).await.status(),
        StatusCode::OK
    );
    // Bob, a member: the 404 a chat that does not exist gets.
    assert_eq!(
        download(&t, &bob, a1, &c.file_id).await.status(),
        StatusCode::NOT_FOUND
    );
    // Alex, an admin, in bob's chat with bob's file.
    let b1 = open(&t, &bob, "t1", json!({})).await;
    let cb = t.chat.ask_docs(b1, doc("his", "# Bob's")).await.unwrap();
    name_in_log(&t, b1, &cb).await;
    assert_eq!(
        download(&t, &bob, b1, &cb.file_id).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        download(&t, &alex, b1, &cb.file_id).await.status(),
        StatusCode::NOT_FOUND
    );
    // Another chat of the same person: alex's file id is not a file of it.
    let a2 = open(&t, &alex, "t1", json!({})).await;
    assert_eq!(
        download(&t, &alex, a2, &c.file_id).await.status(),
        StatusCode::NOT_FOUND
    );
    // Invented ids and shapes that are not ids.
    for bad in [
        "invented",
        "0123456789abcdef0123456789abcdef",
        "..%2F..",
        "a%20b",
    ] {
        assert_eq!(
            download(&t, &alex, a1, bad).await.status(),
            StatusCode::NOT_FOUND,
            "{bad}"
        );
    }
    // Not logged in: refused.
    let r = reqwest::Client::new()
        .get(format!("{}/api/sessions/{a1}/made/{}", t.base, c.file_id))
        .same_origin()
        .send()
        .await
        .unwrap();
    assert!(
        matches!(
            r.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
        ),
        "{}",
        r.status()
    );
}

/// A node cannot name a file the master did not make for this chat, nor one
/// with another hash: the row is the master's, the log only claims it.
#[tokio::test]
async fn a_log_line_cannot_claim_a_file_the_master_did_not_make_for_this_chat() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let a1 = open(&t, &alex, "t1", json!({})).await;
    let b1 = open(&t, &bob, "t1", json!({})).await;
    let cb = t.chat.ask_docs(b1, doc("his", "# Bob's")).await.unwrap();
    // Alex's chat claims Bob's file id with Bob's hash.
    name_in_log(&t, a1, &cb).await;
    assert_eq!(
        download(&t, &alex, a1, &cb.file_id).await.status(),
        StatusCode::NOT_FOUND
    );
    // Bob's own chat claims it with the wrong hash: still not served.
    let mut wrong = cb.clone();
    wrong.sha256 = "00".repeat(32);
    name_in_log(&t, b1, &wrong).await;
    assert_eq!(
        download(&t, &bob, b1, &cb.file_id).await.status(),
        StatusCode::NOT_FOUND
    );
    // With the right one it is.
    name_in_log(&t, b1, &cb).await;
    assert_eq!(
        download(&t, &bob, b1, &cb.file_id).await.status(),
        StatusCode::OK
    );
}

/// A claimed file is served only from its own chat's row and the blob must
/// be the chat owner's: a row planted for another account's blob is refused.
#[tokio::test]
async fn the_switch_decides_in_the_turn_and_at_the_service() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    // An agent without "Create and edit documents": the tools are withheld
    // and the service refuses from the log.
    let sid = open(&t, &alex, "t1", json!({"agent_id": "researcher"})).await;
    let p = t.chat.last_prompt(sid);
    let a = p.agent.unwrap();
    assert!(!a.allows_tool("doc_create") && !a.allows_tool("sheet_create"));
    let e = t.chat.ask_docs(sid, doc("a", "# x")).await.unwrap_err();
    assert!(
        e.contains("not available") && e.contains("Create and edit documents"),
        "{e}"
    );
    // The Assistant has both.
    let sid = open(&t, &alex, "t1", json!({})).await;
    let a = t.chat.last_prompt(sid).agent.unwrap();
    assert!(a.allows_tool("doc_create") && a.allows_tool("sheet_create"));
    assert!(t.chat.ask_docs(sid, doc("a", "# x")).await.is_ok());
    // A turn whose log says the master withheld the tool is refused whatever
    // the agent says now.
    t.chat
        .emit(
            sid,
            "turn_ended",
            json!({"turn_id": "t1", "reason": "completed"}),
        )
        .await;
    t.chat
        .emit(
            sid,
            "turn_started",
            json!({"turn_id": "t2", "agent": {"agent_id": "general", "name": "Assistant", "prompt_hash": "h", "withheld": ["doc_create"]}}),
        )
        .await;
    landed(&t, sid).await;
    let e = t.chat.ask_docs(sid, doc("a", "# x")).await.unwrap_err();
    assert!(e.contains("not available in this turn"), "{e}");
    // The other tool of the family is withheld on its own.
    assert!(
        t.chat
            .ask_docs(sid, sheet("a", json!({"sheets": [{}]})))
            .await
            .is_ok()
    );
}

/// No turn, a callback turn, a temporary chat, a sub-agent's chat, a chat
/// with a folder and an old chat runtime: each refused.
#[tokio::test]
async fn where_the_service_does_not_make_files() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    // Outside a turn.
    let idle = chat(&t, &alex, json!({})).await;
    t.chat
        .emit(idle, "turn_started", json!({"turn_id": "x", "agent": null}))
        .await;
    t.chat
        .emit(
            idle,
            "turn_ended",
            json!({"turn_id": "x", "reason": "completed"}),
        )
        .await;
    landed(&t, idle).await;
    let e = t.chat.ask_docs(idle, doc("a", "# x")).await.unwrap_err();
    assert!(e.contains("outside a turn"), "{e}");
    // A callback turn.
    let sid = open(&t, &alex, "t1", json!({})).await;
    let agent = serde_json::to_value(t.chat.last_prompt(sid).agent.unwrap().agent_ref()).unwrap();
    t.chat
        .emit(
            sid,
            "turn_ended",
            json!({"turn_id": "t1", "reason": "completed"}),
        )
        .await;
    t.chat
        .emit(
            sid,
            "turn_started",
            json!({"turn_id": "cb", "agent": agent, "cause": {"callback": ["bg1"]}}),
        )
        .await;
    landed(&t, sid).await;
    let e = t.chat.ask_docs(sid, doc("a", "# x")).await.unwrap_err();
    assert!(e.contains("not available in this turn"), "{e}");
    // A temporary chat: the tool is withheld and the service refuses.
    let temp = chat(&t, &alex, json!({"incognito": true})).await;
    let (st, v) = send(&t, &alex, temp, &[], json!({})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let a = t.chat.last_prompt(temp).agent.unwrap();
    assert!(!a.allows_tool("doc_create"), "withheld in a temporary chat");
    t.chat.take_prompt(temp, "t1").await;
    landed(&t, temp).await;
    let e = t.chat.ask_docs(temp, doc("a", "# x")).await.unwrap_err();
    assert!(e.contains("temporary"), "{e}");
    assert!(blob_paths(&t).is_empty());
    // A node that does not announce the capability: the tool is withheld
    // from the chat, a request from it is dropped, not answered.
    let old = start_with(&["profile:chat", CAP_ATTACHED]).await;
    let [alex2, _] = users(&old).await;
    let sid = chat(&old, &alex2, json!({})).await;
    let (st, v) = send(&old, &alex2, sid, &[], json!({})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let a = old.chat.last_prompt(sid).agent.unwrap();
    assert!(!a.allows_tool("doc_create") && !a.allows_tool("sheet_create"));
    assert!(old.chat.no_docs_answer(sid, doc("a", "# x")).await);
}

#[tokio::test]
async fn the_sixty_first_call_of_a_turn_is_refused() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    for i in 0..60 {
        t.chat
            .ask_docs(sid, doc(&format!("d{i}"), "# same"))
            .await
            .unwrap_or_else(|e| panic!("call {i}: {e}"));
    }
    let e = t
        .chat
        .ask_docs(sid, doc("one more", "# same"))
        .await
        .unwrap_err();
    assert!(e.contains("60 times") && e.contains("made files"), "{e}");
    // A new turn counts afresh.
    t.chat
        .emit(
            sid,
            "turn_ended",
            json!({"turn_id": "t1", "reason": "completed"}),
        )
        .await;
    next_turn(&t, &alex, sid, "t2", json!({})).await;
    assert!(t.chat.ask_docs(sid, doc("fresh", "# same")).await.is_ok());
}

#[tokio::test]
async fn a_chat_keeps_at_most_a_hundred_made_files() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let mut made = 0;
    let mut last = Ok(());
    for turn in 0..3 {
        for i in 0..40 {
            match t
                .chat
                .ask_docs(sid, doc(&format!("d{turn}-{i}"), &format!("# {turn} {i}")))
                .await
            {
                Ok(_) => made += 1,
                Err(e) => last = Err(e),
            }
        }
        t.chat
            .emit(
                sid,
                "turn_ended",
                json!({"turn_id": format!("t{}", turn + 1), "reason": "completed"}),
            )
            .await;
        if turn < 2 {
            next_turn(&t, &alex, sid, &format!("t{}", turn + 2), json!({})).await;
        }
    }
    assert_eq!(made, harness_store::session_files::MAX_MADE_PER_CHAT);
    let e = last.unwrap_err();
    assert!(e.contains("100 made files"), "{e}");
}

#[tokio::test]
async fn the_account_quota_counts_a_made_file() {
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
    let sid = open(&t, &alex, "t1", json!({})).await;
    // Rows that compress badly: 64 characters from a 64-letter alphabet are
    // six bits each, a sheet of about 600 KB.
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut x: u64 = 88172645463325252;
    let mut rows = Vec::new();
    for _ in 0..4000 {
        let mut cells = Vec::new();
        for _ in 0..3 {
            let mut cell = String::new();
            for _ in 0..64 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                cell.push(ALPHABET[(x % 64) as usize] as char);
            }
            cells.push(json!(cell));
        }
        rows.push(Value::Array(cells));
    }
    let big = |salt: &str| {
        let mut r = rows.clone();
        r.push(json!([salt]));
        sheet("big", json!({"sheets": [{"rows": r}]}))
    };
    let first = t.chat.ask_docs(sid, big("a")).await;
    let c = first.unwrap_or_else(|e| panic!("{e}"));
    assert!(c.bytes > 400_000 && c.bytes < 1 << 20, "{}", c.bytes);
    let e = t.chat.ask_docs(sid, big("b")).await.unwrap_err();
    assert!(e.contains("storage is full"), "{e}");
    assert_eq!(blob_paths(&t).len(), 1, "the refused file left nothing");
}

/// Deleting the chat frees the blob once no chat refers to it.
#[tokio::test]
async fn the_blob_goes_with_the_last_chat_that_refers_to_it() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let c = t.chat.ask_docs(sid, doc("gone", "# soon")).await.unwrap();
    name_in_log(&t, sid, &c).await;
    assert_eq!(blob_paths(&t).len(), 1);
    let owner = owner_of(&t, sid);
    assert!(
        t.master
            .inner_mirror()
            .session_file(&sid.to_string(), &c.file_id)
            .unwrap()
            .is_some()
    );
    let (st, _) = req(
        &t,
        Method::DELETE,
        &format!("/api/sessions/{sid}"),
        None,
        &alex,
    )
    .await;
    assert!(st == StatusCode::OK || st == StatusCode::NO_CONTENT, "{st}");
    assert!(
        t.master
            .inner_mirror()
            .session_file(&sid.to_string(), &c.file_id)
            .unwrap()
            .is_none()
    );
    let _ = owner;
}

// -- guards that no other test proved ------------------------------------------

/// A chat of `kind` ("agentic", or "chat" with a parent) the fake node wrote
/// and `who` owns: what the route and the service must refuse for.
pub(crate) async fn raw_chat(
    t: &T,
    who: &str,
    profile: &str,
    parent: Option<SessionId>,
) -> SessionId {
    let sid = SessionId::new();
    let mut payload = json!({"title": "t", "cwd": "/w", "model": "m", "backend": "native",
        "node_id": "master", "profile": profile, "approval_mode": "ask", "incognito": false});
    if let Some(p) = parent {
        payload["parent_session"] = json!(p.to_string());
    }
    t.chat.emit(sid, "session_created", payload).await;
    landed(t, sid).await;
    let owner = t
        .master
        .inner_mirror()
        .session(parent.unwrap_or(sid))
        .unwrap()
        .unwrap()
        .owner_id;
    assert!(owner.is_some() || parent.is_none());
    let _ = who;
    sid
}

/// A row for `blob` in chat `sid`, the way a send holds it, named by the
/// chat's own log: what a node (or a bug) could plant.
pub(crate) async fn plant(
    t: &T,
    sid: SessionId,
    owner: Option<String>,
    blob: &str,
    sha: &str,
    fid: &str,
) {
    let taken = harness_store::chat_uploads::TakenUpload {
        upload_id: fid.into(),
        owner_id: owner,
        blob_id: blob.into(),
        name: "planted.docx".into(),
        mime: "application/zip".into(),
        size: 10,
        sha256: sha.into(),
    };
    t.master
        .inner_mirror()
        .session_files_hold(&sid.to_string(), &[taken])
        .unwrap();
    t.chat
        .emit(
            sid,
            "user_message",
            json!({"turn_id": "p", "text": "planted", "attached": [{
                "file_id": fid, "name": "planted.docx", "bytes": 10, "sha256": sha, "mime": "application/zip"}]}),
        )
        .await;
    landed(t, sid).await;
}

pub(crate) fn blob_of(t: &T, sid: SessionId, fid: &str) -> (String, String) {
    let f = t
        .master
        .inner_mirror()
        .session_file(&sid.to_string(), fid)
        .unwrap()
        .unwrap();
    (f.blob_id, f.sha256)
}

/// The blob must be the chat owner's, whatever a row says: a row of Alex's
/// chat that points at Bob's blob (the row's account is Bob) is not served.
#[tokio::test]
async fn a_row_that_points_at_another_owners_blob_is_not_served() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let b1 = open(&t, &bob, "t1", json!({})).await;
    let cb = t.chat.ask_docs(b1, doc("his", "# Bob's")).await.unwrap();
    name_in_log(&t, b1, &cb).await;
    let (blob, sha) = blob_of(&t, b1, &cb.file_id);
    let bob_id = owner_of(&t, b1);
    let a1 = open(&t, &alex, "t1", json!({})).await;
    plant(&t, a1, Some(bob_id), &blob, &sha, "planted1").await;
    assert!(
        t.master
            .inner_mirror()
            .session_file(&a1.to_string(), "planted1")
            .unwrap()
            .is_some(),
        "the row is there and logged"
    );
    assert_eq!(
        download(&t, &alex, a1, "planted1").await.status(),
        StatusCode::NOT_FOUND
    );
    // Bob's own copy still downloads.
    assert_eq!(
        download(&t, &bob, b1, &cb.file_id).await.status(),
        StatusCode::OK
    );
}

/// The service and the route refuse chats that are not plain top-level
/// chats of the person: a sub-agent's chat, a chat with a folder, a
/// temporary chat.
#[tokio::test]
async fn the_service_and_the_route_refuse_what_is_not_a_plain_chat() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let top = open(&t, &alex, "t1", json!({})).await;
    let c = t.chat.ask_docs(top, doc("mine", "# Mine")).await.unwrap();
    name_in_log(&t, top, &c).await;
    let (blob, sha) = blob_of(&t, top, &c.file_id);
    let owner = owner_of(&t, top);

    // A sub-agent's chat of the plain chat.
    let sub = raw_chat(&t, &alex, "chat", Some(top)).await;
    t.master
        .inner_mirror()
        .claim_session(sub, Some(&owner), None)
        .unwrap();
    let e = t.chat.ask_docs(sub, doc("a", "# x")).await.unwrap_err();
    assert!(e.contains("sub-agent"), "{e}");
    plant(&t, sub, Some(owner.clone()), &blob, &sha, "p-sub").await;
    assert_eq!(
        download(&t, &alex, sub, "p-sub").await.status(),
        StatusCode::NOT_FOUND
    );

    // A chat with a folder.
    let dir = raw_chat(&t, &alex, "agentic", None).await;
    t.master
        .inner_mirror()
        .claim_session(dir, Some(&owner), None)
        .unwrap();
    let e = t.chat.ask_docs(dir, doc("a", "# x")).await.unwrap_err();
    assert!(e.contains("with a folder writes the file there"), "{e}");
    plant(&t, dir, Some(owner.clone()), &blob, &sha, "p-dir").await;
    assert_eq!(
        download(&t, &alex, dir, "p-dir").await.status(),
        StatusCode::NOT_FOUND
    );

    // A temporary chat.
    let temp = chat(&t, &alex, json!({"incognito": true})).await;
    plant(&t, temp, Some(owner), &blob, &sha, "p-temp").await;
    assert_eq!(
        download(&t, &alex, temp, "p-temp").await.status(),
        StatusCode::NOT_FOUND
    );
    // The honest chat still serves its own.
    assert_eq!(
        download(&t, &alex, top, &c.file_id).await.status(),
        StatusCode::OK
    );
}

// ---- editing a file of the chat (X.57, T98) -------------------------------

/// A stand-in for the `doc-edit` helper that reads its stdin and answers
/// with the framed reply in a file (the real helper is tested in
/// `crates/cli/tests/doc_edit.rs`; here the service is).
pub(crate) fn helper_says(t: &T, frame: &[u8]) {
    let path = t.dir.path().join("helper-reply");
    std::fs::write(&path, frame).unwrap();
    t.master
        .set_pdf_reader(harness_master::pdf::PdfReader::with_command(
            "/bin/sh".into(),
            vec![
                "-c".into(),
                format!("cat >/dev/null; cat '{}'", path.display()).into(),
            ],
        ));
}

pub(crate) fn reply(
    text: &str,
    results: &[(bool, &str)],
    notes: &[&str],
    file: Option<&[u8]>,
) -> Vec<u8> {
    let r = harness_docs::edit::serve::Reply {
        text: text.into(),
        results: results
            .iter()
            .map(|(ok, m)| harness_docs::edit::serve::Item {
                ok: *ok,
                msg: (*m).into(),
            })
            .collect(),
        notes: notes.iter().map(|s| s.to_string()).collect(),
        file: file.is_some(),
        preview: None,
        why: None,
    };
    let head = serde_json::to_vec(&r).unwrap();
    let mut out = (head.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&head);
    out.extend_from_slice(file.unwrap_or(&[]));
    out
}

fn edit_req(file: &str, save_as: Option<&str>) -> DocRequest {
    DocRequest::DocEdit {
        file: file.into(),
        edits: vec![json!({"op": "delete", "at": "p1#0000"})],
        save_as: save_as.map(str::to_string),
    }
}

/// A view returns the helper's text; an edit's new file is kept as a made
/// file of this chat (named `name (edited).docx`, or `save_as`), the
/// original is not touched; a refused edit is an error with each edit's
/// reason and keeps nothing.
#[tokio::test]
async fn an_edit_of_the_chats_own_file_is_kept_as_a_new_made_file() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let made = t
        .chat
        .ask_docs(sid, doc("plan", "# Plan\n\ntext"))
        .await
        .unwrap();
    name_in_log(&t, sid, &made).await;
    let blobs = blob_paths(&t).len();
    // A view.
    helper_says(&t, &reply("p1#a3f Plan", &[], &[], None));
    let v = t
        .chat
        .ask_docs(
            sid,
            DocRequest::View {
                file: made.file_id.clone(),
                sheet: None,
                from: 0,
                limit: 0,
            },
        )
        .await
        .unwrap();
    assert_eq!(v.text, "p1#a3f Plan");
    assert!(
        v.file_id.is_empty() && blob_paths(&t).len() == blobs,
        "a view keeps nothing"
    );
    // An edit: the helper's file is the new made file.
    let new = harness_docs::docx::build("# Plan\n\nedited", &Default::default())
        .unwrap()
        .bytes;
    helper_says(
        &t,
        &reply(
            "",
            &[(true, "edit 1 (delete): p2: deleted")],
            &["a note"],
            Some(&new),
        ),
    );
    let c = t
        .chat
        .ask_docs(sid, edit_req(&made.file_id, None))
        .await
        .unwrap();
    assert_eq!(c.name, "plan (edited).docx");
    assert_ne!(c.file_id, made.file_id);
    assert_eq!(c.mime, harness_core::docs::DOCX_MIME);
    assert_eq!(c.bytes, new.len() as u64);
    assert_eq!(c.notes, ["edit 1 (delete): p2: deleted", "a note"]);
    name_in_log(&t, sid, &c).await;
    let r = download(&t, &alex, sid, &c.file_id).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.bytes().await.unwrap().to_vec(), new);
    let r = download(&t, &alex, sid, &made.file_id).await;
    assert_eq!(r.status(), StatusCode::OK, "the original is still there");
    // save_as; a name is only a name.
    let c = t
        .chat
        .ask_docs(sid, edit_req(&made.file_id, Some("../../etc/Final.DOCX")))
        .await
        .unwrap();
    assert_eq!(c.name, "Final.docx");
    // Refused: no file, every reason.
    helper_says(
        &t,
        &reply(
            "",
            &[
                (true, "edit 1: ok (not applied)"),
                (false, "edit 2: no paragraph p9"),
            ],
            &[],
            None,
        ),
    );
    let n = blob_paths(&t).len();
    let e = t
        .chat
        .ask_docs(sid, edit_req(&made.file_id, None))
        .await
        .unwrap_err();
    assert!(
        e.starts_with("nothing was changed.") && e.contains("edit 2: no paragraph p9"),
        "{e}"
    );
    assert_eq!(blob_paths(&t).len(), n);
    // A sheet gets xlsx; macros keep their extension.
    let x = t
        .chat
        .ask_docs(sid, sheet("book", json!({"sheets": [{"rows": [[1]]}]})))
        .await
        .unwrap();
    name_in_log(&t, sid, &x).await;
    helper_says(
        &t,
        &reply("", &[(true, "ok")], &[], Some(b"PK\x03\x04 bytes")),
    );
    let c = t
        .chat
        .ask_docs(
            sid,
            DocRequest::SheetEdit {
                file: x.file_id.clone(),
                sheet: "Sheet1".into(),
                edits: vec![json!({"op": "clear", "range": "A1"})],
                save_as: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (c.name.as_str(), c.mime.as_str()),
        ("book (edited).xlsx", harness_core::docs::XLSX_MIME)
    );
}

/// The id is the chat's own: another chat's file, a made-up id, an id with
/// odd characters and a file past the cap are refused before any helper.
#[tokio::test]
async fn an_edit_names_only_this_chats_own_files() {
    let t = start().await;
    let [alex, bea] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let other = open(&t, &bea, "t1", json!({})).await;
    let theirs = t.chat.ask_docs(other, doc("secret", "# s")).await.unwrap();
    name_in_log(&t, other, &theirs).await;
    // A helper that would say yes to anything: the service must not ask it.
    let new = harness_docs::docx::build("x", &Default::default())
        .unwrap()
        .bytes;
    helper_says(&t, &reply("leaked", &[(true, "ok")], &[], Some(&new)));
    let n = blob_paths(&t).len();
    for id in [
        theirs.file_id.as_str(),
        "nope",
        "../../etc/passwd",
        "",
        "a b",
    ] {
        let e = t.chat.ask_docs(sid, edit_req(id, None)).await.unwrap_err();
        assert!(e.contains("no such file"), "{id}: {e}");
        let e = t
            .chat
            .ask_docs(
                sid,
                DocRequest::View {
                    file: id.into(),
                    sheet: None,
                    from: 0,
                    limit: 0,
                },
            )
            .await
            .unwrap_err();
        assert!(e.contains("no such file"), "{id}: {e}");
    }
    assert_eq!(blob_paths(&t).len(), n, "nothing was made");
    // The turn's switch decides: an agent without it has no edit either.
    let sid = open(&t, &alex, "t1", json!({"agent_id": "researcher"})).await;
    let e = t.chat.ask_docs(sid, edit_req("x", None)).await.unwrap_err();
    assert!(e.contains("Create and edit documents"), "{e}");
}

/// A helper that cannot confine itself, one that crashes, one that answers
/// nonsense: sentences, nothing kept.
#[tokio::test]
async fn a_failing_helper_keeps_nothing_and_says_why() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let made = t.chat.ask_docs(sid, doc("plan", "# Plan")).await.unwrap();
    name_in_log(&t, sid, &made).await;
    let n = blob_paths(&t).len();
    // The reader is set once; what the stand-in does is a script it runs.
    let script = t.dir.path().join("helper.sh");
    let says = |body: &str| std::fs::write(&script, body).unwrap();
    says("exit 125");
    t.master
        .set_pdf_reader(harness_master::pdf::PdfReader::with_command(
            "/bin/sh".into(),
            vec![script.clone().into_os_string()],
        ));
    let e = t
        .chat
        .ask_docs(sid, edit_req(&made.file_id, None))
        .await
        .unwrap_err();
    assert!(e.contains("Landlock"), "{e}");
    says("cat >/dev/null; printf 'a part is damaged'; exit 3");
    let e = t
        .chat
        .ask_docs(sid, edit_req(&made.file_id, None))
        .await
        .unwrap_err();
    assert_eq!(e, "a part is damaged");
    says("cat >/dev/null; printf 'garbage'");
    let e = t
        .chat
        .ask_docs(sid, edit_req(&made.file_id, None))
        .await
        .unwrap_err();
    assert!(e.contains("not understood"), "{e}");
    assert_eq!(blob_paths(&t).len(), n);
}

/// Every refusal of an edit keeps its reason: a call with many edits has one
/// for each, and all of them reach the node through the real `handle`.
#[tokio::test]
async fn a_refused_call_keeps_the_reason_of_every_edit() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let made = t.chat.ask_docs(sid, doc("plan", "# Plan")).await.unwrap();
    name_in_log(&t, sid, &made).await;
    let long = |k: u8| format!("edit {k}: {}", format!("reason{k} ").repeat(40));
    helper_says(
        &t,
        &reply(
            "",
            &[(false, &long(1)), (false, &long(2)), (false, &long(3))],
            &["a warning"],
            None,
        ),
    );
    let e = t
        .chat
        .ask_docs(sid, edit_req(&made.file_id, None))
        .await
        .unwrap_err();
    for k in 1..=3 {
        assert!(e.contains(&format!("reason{k}")), "edit {k} is in: {e}");
    }
    assert!(
        e.lines().count() >= 4 && e.lines().all(|l| l.chars().count() <= 400),
        "{e}"
    );
}

/// A helper that ran counts against the turn whatever it said (a slow
/// hostile file cannot be retried without limit); a refusal before it starts
/// gives its count back.
#[tokio::test]
async fn a_run_of_the_helper_counts_and_a_refusal_before_it_does_not() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let made = t.chat.ask_docs(sid, doc("plan", "# Plan")).await.unwrap();
    name_in_log(&t, sid, &made).await;
    let script = t.dir.path().join("helper.sh");
    std::fs::write(&script, "cat >/dev/null; printf 'bad part'; exit 3").unwrap();
    t.master
        .set_pdf_reader(harness_master::pdf::PdfReader::with_command(
            "/bin/sh".into(),
            vec![script.into_os_string()],
        ));
    // One call was the creation above; sixty in all are allowed in a turn.
    // A hundred refusals before the helper starts count for nothing.
    for _ in 0..100 {
        let e = t
            .chat
            .ask_docs(sid, edit_req("nope", None))
            .await
            .unwrap_err();
        assert!(e.contains("no such file"), "{e}");
    }
    let mut ran = 0;
    let err = loop {
        match t.chat.ask_docs(sid, edit_req(&made.file_id, None)).await {
            Err(e) if e == "bad part" => ran += 1,
            Err(e) => break e,
            Ok(_) => panic!("the helper failed"),
        }
        assert!(ran <= 60, "no limit");
    };
    assert_eq!(ran, 59, "the creation and 59 failed runs are the turn's 60");
    assert!(err.contains("60 times"), "{err}");
}
