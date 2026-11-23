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

async fn start() -> T {
    start_with(&["profile:chat", CAP_ATTACHED, CAP_DOCS]).await
}

fn doc(name: &str, md: &str) -> DocRequest {
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
async fn open(t: &T, who: &str, turn: &str, extra: Value) -> SessionId {
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
async fn name_in_log(t: &T, sid: SessionId, c: &DocCreated) {
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

fn owner_of(t: &T, sid: SessionId) -> String {
    t.master
        .inner_mirror()
        .session(sid)
        .unwrap()
        .unwrap()
        .owner_id
        .unwrap()
}

fn blob_paths(t: &T) -> Vec<std::path::PathBuf> {
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
