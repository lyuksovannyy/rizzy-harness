//! Office documents through the HTTP API (ADOPTION_PLAN X.55, PLAN.md T96):
//! the helper's text goes in the message as a PDF's does and the file is
//! kept as well where this chat keeps files; where it cannot, the text goes
//! alone and nothing is refused; a document that cannot be read is a plain
//! file with one sentence. The helper here is a stand-in script (the real
//! one is `crates/cli/tests/doc_text.rs`).

use std::time::Duration;

use harness_attached::testkit::{docx, p, zip};
use harness_master::attachments::file_id_of;
use harness_master::pdf::PdfReader;
use reqwest::StatusCode;
use serde_json::json;

use crate::attached_http::{T, chat, send, start, upload, users};

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// A helper that says `reply` (its stdout) and exits with `code`.
fn helper(t: &T, reply: &str, code: i32) {
    t.master.set_pdf_reader(PdfReader::with_command(
        "/bin/sh".into(),
        vec![
            "-c".into(),
            format!("cat >/dev/null; printf '%s' '{reply}'; exit {code}").into(),
        ],
    ));
}

fn words() -> String {
    format!("{DOCX}\nHello from the document\n")
}

/// A document in a chat that keeps files: its text is the message's and the
/// file is the chat's, read by `attached_file`; one id, one chip.
#[tokio::test]
async fn a_document_is_text_and_a_kept_file() {
    let t = start().await;
    helper(&t, &words(), 0);
    let [alex, _] = users(&t).await;
    let sid = chat(&t, &alex, json!({})).await;
    // The name is not what makes it a document.
    let (st, v) = upload(
        &t,
        &alex,
        "plan.bin",
        &format!("&session={sid}"),
        docx(&p("x")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["mime"], DOCX);
    assert_eq!(v["chars"], 24);
    assert_eq!(v["file_kept"], true, "{v}");
    assert!(v.get("kept").is_none(), "the chip is the text's: {v}");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let (st, v) = send(&t, &alex, sid, &[&id], json!({})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pm = t.chat.last_prompt(sid);
    assert_eq!(pm.attachments.len(), 1);
    assert_eq!(pm.attachments[0].text, "Hello from the document\n");
    assert_eq!(pm.attached.len(), 1, "the file is kept as well");
    assert_eq!(pm.attached[0].name, "plan.bin");
    // Both rows are consumed by the send.
    let m = t.master.inner_mirror();
    assert!(m.chat_uploads_among(&[file_id_of(&id)]).unwrap().is_empty());
}

/// Without the agent's switch the text still goes and the file is simply not
/// kept: no refusal, at attach time or at send.
#[tokio::test]
async fn without_the_switch_the_text_goes_and_the_file_does_not() {
    let t = start().await;
    helper(&t, &words(), 0);
    let [alex, _] = users(&t).await;
    let sid = chat(&t, &alex, json!({})).await;
    // At attach: Researcher has no such switch.
    let (st, v) = upload(
        &t,
        &alex,
        "a.docx",
        &format!("&session={sid}&agent=researcher"),
        docx(&p("x")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v.get("file_kept").is_none(), "{v}");
    assert_eq!(v["mime"], DOCX);
    // At send: kept with the default agent, sent to another.
    let (st, v) = upload(
        &t,
        &alex,
        "b.docx",
        &format!("&session={sid}"),
        docx(&p("y")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["file_kept"], true);
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let (st, v) = send(&t, &alex, sid, &[&id], json!({"agent_id": "researcher"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let pm = t.chat.last_prompt(sid);
    assert_eq!(pm.attachments.len(), 1);
    assert!(pm.attached.is_empty(), "the file was dropped, not refused");
}

/// A coding chat to come keeps the file for its folder next to the text.
#[tokio::test]
async fn a_coding_chat_keeps_the_file_for_its_folder() {
    let t = start().await;
    helper(&t, &words(), 0);
    let [alex, _] = users(&t).await;
    let (st, v) = upload(&t, &alex, "a.docx", "&agentic=1", docx(&p("x"))).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["file_kept"], true, "{v}");
    let id = v["attachment_id"].as_str().unwrap().to_string();
    let m = t.master.inner_mirror();
    assert_eq!(
        m.chat_uploads_among(&[file_id_of(&id)]).unwrap(),
        vec![file_id_of(&id)]
    );
}

/// The reverse of "by name": a zip the helper says is no document is a plain
/// file whatever it is called, and a document is text whatever it is called.
#[tokio::test]
async fn a_zip_named_docx_is_not_one_and_the_name_never_decides() {
    let t = start().await;
    helper(&t, "", 5);
    let [alex, _] = users(&t).await;
    let sid = chat(&t, &alex, json!({})).await;
    let (st, v) = upload(
        &t,
        &alex,
        "report.docx",
        &format!("&session={sid}"),
        zip(&[("a.txt", b"x")]),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["kept"], true, "{v}");
    assert!(v.get("note").is_none(), "no new sentence: {v}");
    assert!(v.get("chars").is_none());
}

/// A document that cannot be read is kept as a plain file with one sentence
/// saying so; never a 500, never a lost upload. Where files are not kept it
/// is refused with the same sentence.
#[tokio::test]
async fn an_unreadable_document_is_a_file_with_a_sentence() {
    let t = start().await;
    helper(&t, "a part of the document is missing", 3);
    let [alex, _] = users(&t).await;
    let sid = chat(&t, &alex, json!({})).await;
    let (st, v) = upload(
        &t,
        &alex,
        "a.docx",
        &format!("&session={sid}"),
        docx(&p("x")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["kept"], true, "{v}");
    let note = v["note"].as_str().unwrap();
    assert!(note.contains("could not be read"), "{note}");
    // A chat that cannot keep it says the same.
    let (st, v) = upload(
        &t,
        &alex,
        "a.docx",
        &format!("&session={sid}&agent=researcher"),
        docx(&p("x")),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
}

/// The helper that never finishes is killed at its timeout; the document is
/// then a plain file with the sentence, and the slot comes back.
#[tokio::test]
async fn a_helper_killed_on_timeout_leaves_a_file_with_a_sentence() {
    let t = start().await;
    t.master.set_pdf_reader(
        PdfReader::with_command(
            "/bin/sh".into(),
            vec![
                "-c".into(),
                format!(
                    "while read -r _ _ _ p _ </proc/$$/stat && [ \"$p\" = {} ]; do :; done",
                    std::process::id()
                )
                .into(),
            ],
        )
        .with_limits(Duration::from_millis(700), Duration::from_secs(1)),
    );
    let [alex, _] = users(&t).await;
    let sid = chat(&t, &alex, json!({})).await;
    let (st, v) = upload(
        &t,
        &alex,
        "a.docx",
        &format!("&session={sid}"),
        docx(&p("x")),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v["note"].as_str().unwrap().contains("too long"), "{v}");
    let r = t.master.pdf_reader().unwrap();
    assert_eq!(r.free_slots(), harness_master::pdf::CONCURRENT);
}

/// Two helpers busy: a third document waits its bound and gets 503, as a PDF.
#[tokio::test]
async fn two_busy_helpers_make_a_third_document_wait_and_get_503() {
    let t = start().await;
    t.master.set_pdf_reader(
        PdfReader::with_command(
            "/bin/sh".into(),
            vec![
                "-c".into(),
                format!(
                    "while read -r _ _ _ p _ </proc/$$/stat && [ \"$p\" = {} ]; do :; done",
                    std::process::id()
                )
                .into(),
            ],
        )
        .with_limits(Duration::from_secs(4), Duration::from_millis(300)),
    );
    let [alex, _] = users(&t).await;
    let base = t.base.clone();
    let cookie = alex.clone();
    let mut busy = Vec::new();
    for _ in 0..harness_master::pdf::CONCURRENT {
        let (base, cookie) = (base.clone(), cookie.clone());
        busy.push(tokio::spawn(async move {
            use crate::common::{Login as _, SameOrigin as _};
            reqwest::Client::new()
                .post(format!("{base}/api/attachments?name=a.docx&agentic=1"))
                .login(&cookie)
                .same_origin()
                .body(docx(&p("x")))
                .send()
                .await
                .unwrap()
                .status()
        }));
    }
    let r = t.master.pdf_reader().unwrap();
    for _ in 0..300 {
        if r.free_slots() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(r.free_slots(), 0);
    let (st, v) = upload(&t, &alex, "c.docx", "&agentic=1", docx(&p("z"))).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    for b in busy {
        b.await.unwrap();
    }
    assert_eq!(r.free_slots(), harness_master::pdf::CONCURRENT);
}
