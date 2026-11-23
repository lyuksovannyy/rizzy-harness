//! The preview of a Word or Excel file of a chat without a folder, in the
//! page's side panel (PLAN.md T99): `GET /api/sessions/{id}/files/{file_id}/preview`.
//! Owner only like the download; the file is parsed only by the confined
//! `doc-edit` helper (a stand-in here; the real one in
//! `crates/cli/tests/doc_edit.rs`), under the account's one slot.

use super::attached_http::{T, chat, landed, req, send, upload, users};
use super::made_http::{
    blob_of, doc, helper_says, name_in_log, open, owner_of, plant, raw_chat, start,
};
use crate::common::{Login as _, SameOrigin as _};

use std::time::{Duration, Instant};

use harness_core::SessionId;
use harness_docs::edit::preview::{Block, Preview, Run, Why};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

async fn get(t: &T, who: &str, sid: SessionId, file_id: &str, query: &str) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!(
            "{}/api/sessions/{sid}/files/{file_id}/preview{query}",
            t.base
        ))
        .login(who)
        .same_origin()
        .send()
        .await
        .unwrap()
}

async fn body(r: reqwest::Response) -> (StatusCode, Value) {
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

fn one_paragraph(text: &str) -> Preview {
    Preview::Doc {
        blocks: vec![Block::P {
            r: vec![Run {
                t: text.into(),
                b: true,
                i: false,
                s: false,
            }],
            at: String::new(),
        }],
        more: false,
        notes: vec![],
    }
}

/// A reply of the stand-in helper with a preview.
fn preview_reply(p: Option<Preview>) -> Vec<u8> {
    why_reply(p, None)
}

fn why_reply(p: Option<Preview>, why: Option<Why>) -> Vec<u8> {
    let r = harness_docs::edit::serve::Reply {
        preview: p,
        why,
        ..Default::default()
    };
    let head = serde_json::to_vec(&r).unwrap();
    let mut out = (head.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&head);
    out
}

/// A stand-in that writes its stdin to `record` (when the helper ran at all)
/// and answers with `frame`.
fn helper_records(t: &T, frame: &[u8]) -> std::path::PathBuf {
    let record = t.dir.path().join("helper-stdin");
    let _ = std::fs::remove_file(&record);
    let path = t.dir.path().join("helper-reply");
    std::fs::write(&path, frame).unwrap();
    t.master
        .set_pdf_reader(harness_master::pdf::PdfReader::with_command(
            "/bin/sh".into(),
            vec![
                "-c".into(),
                format!("cat >'{}'; cat '{}'", record.display(), path.display()).into(),
            ],
        ));
    record
}

fn request_of(record: &std::path::Path) -> Value {
    let raw = std::fs::read(record).expect("the helper ran");
    let n = u32::from_be_bytes(raw[..4].try_into().unwrap()) as usize;
    serde_json::from_slice(&raw[4..4 + n]).unwrap()
}

/// The whole path for a made file and for an attached one: the owner gets
/// the helper's preview, the request is only `preview` (and the sheet), the
/// answer is never cached, an attached file is served only once the chat's
/// own log names it.
#[tokio::test]
async fn the_owner_previews_a_made_file_and_an_attached_one() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let made = t.chat.ask_docs(sid, doc("plan", "# Plan")).await.unwrap();
    name_in_log(&t, sid, &made).await;
    // An attached file (kept as the owner's blob; no reader is set up yet,
    // so its text is not read and the file is kept as it is).
    let (st, v) = upload(
        &t,
        &alex,
        "notes.docx",
        &format!("&session={sid}"),
        harness_docs::docx::build("# Notes", &Default::default())
            .unwrap()
            .bytes,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let (st, _) = send(&t, &alex, sid, &[&id], json!({"text": "x"})).await;
    assert_eq!(st, StatusCode::OK);
    let record = helper_records(&t, &preview_reply(Some(one_paragraph("hello"))));
    let r = get(&t, &alex, sid, &made.file_id, "").await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()["cache-control"], "no-store");
    assert!(
        r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["name"], "plan.docx");
    assert_eq!(v["preview"]["kind"], "doc");
    assert_eq!(v["preview"]["blocks"][0]["r"][0]["t"], "hello");
    assert_eq!(
        request_of(&record),
        json!({"tool": "preview", "sheet": null})
    );
    // The place of a sheet goes to the helper as asked; nothing else does.
    let r = get(&t, &alex, sid, &made.file_id, "?sheet=2").await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(request_of(&record), json!({"tool": "preview", "sheet": 2}));
    for bad in ["Data", "-1", "1.5", "99999999999", ""] {
        let r = get(&t, &alex, sid, &made.file_id, &format!("?sheet={bad}")).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{bad}");
    }
    // An attached file: held before the log names it, previewed after.
    assert_eq!(
        get(&t, &alex, sid, &id, "").await.status(),
        StatusCode::NOT_FOUND,
        "held"
    );
    t.chat.take_prompt(sid, "t2").await;
    landed(&t, sid).await;
    let (st, v) = body(get(&t, &alex, sid, &id, "").await).await;
    assert_eq!(
        (st, v["name"].as_str()),
        (StatusCode::OK, Some("notes.docx"))
    );
}

/// Chats are private to their owner, admins too; a file id is only ever a
/// file of its own chat; nothing reaches the helper for any of these.
#[tokio::test]
async fn nobody_else_previews_and_a_file_id_is_only_its_own_chats() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let a1 = open(&t, &alex, "t1", json!({})).await;
    let c = t.chat.ask_docs(a1, doc("mine", "# Private")).await.unwrap();
    name_in_log(&t, a1, &c).await;
    let b1 = open(&t, &bob, "t1", json!({})).await;
    let cb = t.chat.ask_docs(b1, doc("his", "# Bob's")).await.unwrap();
    name_in_log(&t, b1, &cb).await;
    let record = helper_records(&t, &preview_reply(Some(one_paragraph("secret"))));
    // The body an unknown chat gets.
    let (st, unknown) = body(get(&t, &alex, SessionId::new(), &c.file_id, "").await).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    // Bob, a member, in alex's chat; alex, an admin, in bob's.
    for (who, sid, fid) in [(&bob, a1, &c.file_id), (&alex, b1, &cb.file_id)] {
        let (st, v) = body(get(&t, who, sid, fid, "").await).await;
        assert_eq!((st, &v), (StatusCode::NOT_FOUND, &unknown));
    }
    // Another chat of the same person: the file id is not a file of it.
    let a2 = open(&t, &alex, "t1", json!({})).await;
    assert_eq!(
        get(&t, &alex, a2, &c.file_id, "").await.status(),
        StatusCode::NOT_FOUND
    );
    // Ids that are not ids.
    for bad in [
        "invented",
        "0123456789abcdef0123456789abcdef",
        "..%2F..",
        "a%20b",
    ] {
        assert_eq!(
            get(&t, &alex, a1, bad, "").await.status(),
            StatusCode::NOT_FOUND,
            "{bad}"
        );
    }
    // Not logged in.
    let r = reqwest::Client::new()
        .get(format!(
            "{}/api/sessions/{a1}/files/{}/preview",
            t.base, c.file_id
        ))
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
    assert!(!record.exists(), "the helper never ran");
    // The honest request still works.
    assert_eq!(
        get(&t, &alex, a1, &c.file_id, "").await.status(),
        StatusCode::OK
    );
}

/// A row that points at another owner's blob, a sub-agent's chat, a chat
/// with a folder and a temporary chat: 404, and no helper.
#[tokio::test]
async fn a_planted_row_a_folder_chat_an_incognito_chat_and_a_sub_agent_are_not_previewed() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let top = open(&t, &alex, "t1", json!({})).await;
    let c = t.chat.ask_docs(top, doc("mine", "# Mine")).await.unwrap();
    name_in_log(&t, top, &c).await;
    let (blob, sha) = blob_of(&t, top, &c.file_id);
    let owner = owner_of(&t, top);
    let record = helper_records(&t, &preview_reply(Some(one_paragraph("x"))));
    // Alex's chat with a row of bob's blob.
    let b1 = open(&t, &bob, "t1", json!({})).await;
    let cb = t.chat.ask_docs(b1, doc("his", "# Bob's")).await.unwrap();
    name_in_log(&t, b1, &cb).await;
    let (bblob, bsha) = blob_of(&t, b1, &cb.file_id);
    // With the blob's real size, so that only the owner check refuses it.
    plant_named(
        &t,
        top,
        &owner_of(&t, b1),
        (&bblob, &bsha, cb.bytes),
        "p-bob",
        "planted.docx",
    )
    .await;
    assert_eq!(
        get(&t, &alex, top, "p-bob", "").await.status(),
        StatusCode::NOT_FOUND
    );
    let sub = raw_chat(&t, &alex, "chat", Some(top)).await;
    t.master
        .inner_mirror()
        .claim_session(sub, Some(&owner), None)
        .unwrap();
    plant(&t, sub, Some(owner.clone()), &blob, &sha, "p-sub").await;
    let dir = raw_chat(&t, &alex, "agentic", None).await;
    t.master
        .inner_mirror()
        .claim_session(dir, Some(&owner), None)
        .unwrap();
    plant(&t, dir, Some(owner.clone()), &blob, &sha, "p-dir").await;
    let temp = chat(&t, &alex, json!({"incognito": true})).await;
    plant(&t, temp, Some(owner), &blob, &sha, "p-temp").await;
    for (sid, id) in [(sub, "p-sub"), (dir, "p-dir"), (temp, "p-temp")] {
        assert_eq!(
            get(&t, &alex, sid, id, "").await.status(),
            StatusCode::NOT_FOUND,
            "{id}"
        );
    }
    assert!(!record.exists(), "the helper never ran");
    // The honest file of the honest chat is previewed.
    assert_eq!(
        get(&t, &alex, top, &c.file_id, "").await.status(),
        StatusCode::OK
    );
}

/// A name that is not a Word or Excel name never reaches the helper (415); a
/// renamed text file does and the helper says it is no document (415); a
/// failing helper is a plain sentence (422, 502, 503) that does not carry
/// what the helper wrote.
#[tokio::test]
async fn what_is_not_a_document_is_415_and_a_failing_helper_does_not_leak() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let made = t.chat.ask_docs(sid, doc("plan", "# Plan")).await.unwrap();
    name_in_log(&t, sid, &made).await;
    let (blob, sha) = blob_of(&t, sid, &made.file_id);
    let owner = owner_of(&t, sid);
    // Rows of this chat under other names, as the log would name them.
    for (id, name) in [
        ("n-zip", "archive.zip"),
        ("n-pdf", "paper.pdf"),
        ("n-docx", "notes.docx"),
        ("n-xlsx", "book.xlsx"),
        ("n-none", "plain"),
    ] {
        plant_named(&t, sid, &owner, (&blob, &sha, made.bytes), id, name).await;
    }
    // The reader is set once; what the stand-in does is a script it runs.
    let script = t.dir.path().join("helper.sh");
    let says = |body: &str| std::fs::write(&script, body).unwrap();
    let record = t.dir.path().join("helper-stdin");
    let reply_none = t.dir.path().join("reply-none");
    std::fs::write(&reply_none, why_reply(None, Some(Why::NotOffice))).unwrap();
    says(&format!(
        "cat >'{}'; cat '{}'",
        record.display(),
        reply_none.display()
    ));
    t.master
        .set_pdf_reader(harness_master::pdf::PdfReader::with_command(
            "/bin/sh".into(),
            vec![script.clone().into_os_string()],
        ));
    for id in ["n-zip", "n-pdf", "n-none"] {
        let (st, v) = body(get(&t, &alex, sid, id, "").await).await;
        assert_eq!(st, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{id}: {v}");
        assert!(
            v["error"].as_str().unwrap().contains("Word or Excel"),
            "{v}"
        );
    }
    assert!(!record.exists(), "a zip and a PDF never reach the helper");
    // The helper finds a text file named .docx is no document.
    let (st, v) = body(get(&t, &alex, sid, "n-docx", "").await).await;
    assert_eq!(st, StatusCode::UNSUPPORTED_MEDIA_TYPE, "{v}");
    assert!(record.exists(), "this one reached the helper");
    // A helper that refuses the file with its own words: not repeated.
    says("cat >/dev/null; printf 'SECRET-NAME.docx word/evil.xml'; exit 3");
    let r = get(&t, &alex, sid, "n-xlsx", "").await;
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let text = r.text().await.unwrap();
    assert!(!text.contains("SECRET") && !text.contains("evil"), "{text}");
    // A helper that crashes, one that answers nonsense, one that cannot
    // confine itself.
    says("cat >/dev/null; exit 9");
    assert_eq!(
        get(&t, &alex, sid, "n-xlsx", "").await.status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    says("cat >/dev/null; printf 'garbage'");
    let r = get(&t, &alex, sid, "n-xlsx", "").await;
    assert_eq!(r.status(), StatusCode::BAD_GATEWAY);
    assert!(!r.text().await.unwrap().contains("garbage"));
    says("exit 125");
    let r = get(&t, &alex, sid, "n-xlsx", "").await;
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// A row of chat `sid` for `blob` under `name`, named by the chat's own log.
async fn plant_named(
    t: &T,
    sid: SessionId,
    owner: &str,
    (blob, sha, size): (&str, &str, u64),
    fid: &str,
    name: &str,
) {
    let taken = harness_store::chat_uploads::TakenUpload {
        upload_id: fid.into(),
        owner_id: Some(owner.into()),
        blob_id: blob.into(),
        name: name.into(),
        mime: "application/octet-stream".into(),
        size,
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
                "file_id": fid, "name": name, "bytes": size, "sha256": sha, "mime": "application/octet-stream"}]}),
        )
        .await;
    landed(t, sid).await;
}

/// A reply bigger than the page may take is refused, not forwarded.
#[tokio::test]
async fn a_reply_past_the_limit_is_not_forwarded() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let made = t.chat.ask_docs(sid, doc("plan", "# Plan")).await.unwrap();
    name_in_log(&t, sid, &made).await;
    helper_says(
        &t,
        &preview_reply(Some(one_paragraph(&"x".repeat(600 * 1024)))),
    );
    let r = get(&t, &alex, sid, &made.file_id, "").await;
    assert_eq!(r.status(), StatusCode::BAD_GATEWAY);
}

/// Many previews at once from one account: one helper at a time; the
/// others wait their turn and, past the queue bound, are told to try
/// again. Another account is not held up by it.
#[tokio::test]
async fn one_account_runs_one_preview_at_a_time() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let made = t.chat.ask_docs(sid, doc("plan", "# Plan")).await.unwrap();
    name_in_log(&t, sid, &made).await;
    let b1 = open(&t, &bob, "t1", json!({})).await;
    let cb = t.chat.ask_docs(b1, doc("his", "# Bob's")).await.unwrap();
    name_in_log(&t, b1, &cb).await;
    // Each helper run takes a second; a waiter gives up after 300 ms.
    let path = t.dir.path().join("helper-reply");
    std::fs::write(&path, preview_reply(Some(one_paragraph("slow")))).unwrap();
    t.master.set_pdf_reader(
        harness_master::pdf::PdfReader::with_command(
            "/bin/sh".into(),
            vec![
                "-c".into(),
                format!("cat >/dev/null; sleep 1; cat '{}'", path.display()).into(),
            ],
        )
        .with_limits(Duration::from_secs(10), Duration::from_millis(300)),
    );
    let started = Instant::now();
    let first = tokio::spawn({
        let (base, alex, fid) = (t.base.clone(), alex.clone(), made.file_id.clone());
        async move {
            reqwest::Client::new()
                .get(format!("{base}/api/sessions/{sid}/files/{fid}/preview"))
                .login(&alex)
                .same_origin()
                .send()
                .await
                .unwrap()
                .status()
        }
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    // The same account, a second one: waits 300 ms, then 503.
    let (st, v) = body(get(&t, &alex, sid, &made.file_id, "").await).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("busy"), "{v}");
    // Another account has its own place (the server has two).
    let (st, _) = body(get(&t, &bob, b1, &cb.file_id, "").await).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(first.await.unwrap(), StatusCode::OK);
    assert!(started.elapsed() >= Duration::from_secs(1));
    let _ = req(&t, Method::GET, "/api/sessions", None, &alex).await;
}

/// A reply past what a preview can be is a failure of the helper, not
/// something read: the master reads only a preview-sized answer.
#[tokio::test]
async fn the_helper_may_answer_only_a_preview_sized_reply() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let made = t.chat.ask_docs(sid, doc("plan", "# Plan")).await.unwrap();
    name_in_log(&t, sid, &made).await;
    helper_says(
        &t,
        &preview_reply(Some(one_paragraph(&"x".repeat(1024 * 1024)))),
    );
    let r = get(&t, &alex, sid, &made.file_id, "").await;
    assert_eq!(r.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// Every reason has its own sentence: not a document (415), protected with a
/// password, too large (download it), damaged, no such sheet, empty.
#[tokio::test]
async fn each_reason_has_its_own_sentence() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let made = t.chat.ask_docs(sid, doc("plan", "# Plan")).await.unwrap();
    name_in_log(&t, sid, &made).await;
    let (blob, sha) = blob_of(&t, sid, &made.file_id);
    plant_named(
        &t,
        sid,
        &owner_of(&t, sid),
        (&blob, &sha, 0),
        "empty",
        "empty.docx",
    )
    .await;
    let script = t.dir.path().join("helper.sh");
    let reply = t.dir.path().join("reply");
    let says = |why: Option<Why>| {
        std::fs::write(&reply, why_reply(None, why)).unwrap();
        std::fs::write(
            &script,
            format!("cat >/dev/null; cat '{}'", reply.display()),
        )
        .unwrap();
    };
    says(None);
    t.master
        .set_pdf_reader(harness_master::pdf::PdfReader::with_command(
            "/bin/sh".into(),
            vec![script.clone().into_os_string()],
        ));
    let mut seen = vec![];
    for (why, code, word) in [
        (
            Why::NotOffice,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Word or Excel",
        ),
        (Why::Protected, StatusCode::UNPROCESSABLE_ENTITY, "password"),
        (
            Why::TooLarge,
            StatusCode::UNPROCESSABLE_ENTITY,
            "too large to preview; download it to open it",
        ),
        (Why::Damaged, StatusCode::UNPROCESSABLE_ENTITY, "damaged"),
        (Why::NoSheet, StatusCode::BAD_REQUEST, "no such sheet"),
    ] {
        says(Some(why));
        let (st, v) = body(get(&t, &alex, sid, &made.file_id, "").await).await;
        let e = v["error"].as_str().unwrap_or_default().to_string();
        assert_eq!(st, code, "{why:?}: {e}");
        assert!(e.contains(word), "{why:?}: {e}");
        assert!(!seen.contains(&e), "one sentence each: {e}");
        seen.push(e);
    }
    // An empty file is not "too large".
    let (st, v) = body(get(&t, &alex, sid, "empty", "").await).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
    let e = v["error"].as_str().unwrap();
    assert!(e.contains("empty") && !e.contains("large"), "{e}");
}

/// The server's API token reads any chat, as it can download from it (T10);
/// it holds its own helper place, not the owner's.
#[tokio::test]
async fn the_api_token_previews_as_it_downloads() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let sid = open(&t, &alex, "t1", json!({})).await;
    let made = t.chat.ask_docs(sid, doc("plan", "# Plan")).await.unwrap();
    name_in_log(&t, sid, &made).await;
    helper_says(&t, &preview_reply(Some(one_paragraph("hi"))));
    let r = reqwest::Client::new()
        .get(format!(
            "{}/api/sessions/{sid}/files/{}/preview",
            t.base, made.file_id
        ))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
}
