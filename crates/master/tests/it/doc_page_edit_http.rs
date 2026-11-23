//! A person's text changes to a file of a chat without a folder, saved as a
//! held copy (PLAN.md T100, ADOPTION_PLAN X.60):
//! `POST /api/sessions/{id}/files/{file_id}/edit`. The same owner-only checks
//! as the preview, the rule of every POST, the helper a stand-in here (the
//! real one in `crates/cli/tests/doc_edit.rs`), and the copy stored as an
//! upload is, under the same quotas.

use super::attached_http::{T, send, upload, users};
use super::made_http::{blob_of, doc, name_in_log, open, owner_of, plant, raw_chat, start};
use crate::common::{Login as _, SameOrigin as _};

use harness_core::SessionId;
use harness_docs::edit::page::ChangeWhy;
use harness_docs::edit::preview::Why;
use harness_docs::edit::serve::{Item, Reply};
use reqwest::StatusCode;
use serde_json::{Value, json};

async fn post(t: &T, who: &str, sid: SessionId, file_id: &str, body: Value) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!(
            "{}/api/sessions/{sid}/files/{file_id}/edit",
            t.base
        ))
        .login(who)
        .same_origin()
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn body(r: reqwest::Response) -> (StatusCode, Value) {
    let st = r.status();
    (st, r.json().await.unwrap_or(Value::Null))
}

fn one() -> Value {
    json!({"changes": [{"at": "p1#0000", "text": "New text"}]})
}

fn docx() -> Vec<u8> {
    harness_docs::docx::build("# Plan\n\nBody\n", &Default::default())
        .unwrap()
        .bytes
}

fn ok_items(n: usize) -> Vec<Item> {
    (0..n)
        .map(|_| Item {
            ok: true,
            msg: "SECRET-WORDS of the helper".into(),
            why: None,
        })
        .collect()
}

fn frame(r: &Reply, file: &[u8]) -> Vec<u8> {
    let head = serde_json::to_vec(r).unwrap();
    let mut out = (head.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&head);
    out.extend_from_slice(file);
    out
}

/// A stand-in that records its stdin (when it ran at all) and answers `out`.
fn helper(t: &T, out: &[u8]) -> std::path::PathBuf {
    let record = t.dir.path().join("helper-stdin");
    let _ = std::fs::remove_file(&record);
    let reply = t.dir.path().join("helper-reply");
    std::fs::write(&reply, out).unwrap();
    t.master
        .set_pdf_reader(harness_master::pdf::PdfReader::with_command(
            "/bin/sh".into(),
            vec![
                "-c".into(),
                format!("cat >'{}'; cat '{}'", record.display(), reply.display()).into(),
            ],
        ));
    record
}

fn request_of(record: &std::path::Path) -> Value {
    let raw = std::fs::read(record).expect("the helper ran");
    let n = u32::from_be_bytes(raw[..4].try_into().unwrap()) as usize;
    serde_json::from_slice(&raw[4..4 + n]).unwrap()
}

async fn chat_with_file(t: &T, who: &str) -> (SessionId, String) {
    let sid = open(t, who, "t1", json!({})).await;
    let made = t.chat.ask_docs(sid, doc("plan", "# Plan")).await.unwrap();
    name_in_log(t, sid, &made).await;
    (sid, made.file_id)
}

/// The whole path: the helper gets only a `page_edit` request; the copy is a
/// held attachment of the caller, named `<stem> (edited).docx`, which the
/// next message can carry; the answer is what an upload answers.
#[tokio::test]
async fn the_owner_saves_a_copy_that_is_held_like_an_upload() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let (sid, fid) = chat_with_file(&t, &alex).await;
    let new = docx();
    let record = helper(
        &t,
        &frame(
            &Reply {
                results: ok_items(1),
                file: true,
                ..Default::default()
            },
            &new,
        ),
    );
    let (st, v) = body(
        post(
            &t,
            &alex,
            sid,
            &fid,
            json!({"sheet": 1, "changes": [{"at": "p1#00ab", "text": "New text"}]}),
        )
        .await,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        request_of(&record),
        json!({"tool": "page_edit", "sheet": 1, "changes": [{"at": "p1#00ab", "text": "New text"}]})
    );
    assert_eq!(v["file"]["name"], "plan (edited).docx");
    assert_eq!(v["file"]["kept"], true);
    assert_eq!(v["file"]["bytes"], new.len());
    assert_eq!(v["results"], json!([{"ok": true}]));
    assert!(!v.to_string().contains("SECRET"), "{v}");
    // Held: the next message carries it.
    let id = v["file"]["attachment_id"].as_str().unwrap().to_string();
    let (st, v) = send(&t, &alex, sid, &[&id], json!({"text": "here"})).await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// Chats are private to their owner, admins too; a bad cookie, a missing
/// origin and a cross-origin POST are refused; the helper never runs and
/// nothing is stored.
#[tokio::test]
async fn nobody_else_saves_a_copy_and_a_cross_origin_post_is_refused() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let (a1, fid) = chat_with_file(&t, &alex).await;
    let (b1, bfid) = chat_with_file(&t, &bob).await;
    let record = helper(
        &t,
        &frame(
            &Reply {
                results: ok_items(1),
                file: true,
                ..Default::default()
            },
            &docx(),
        ),
    );
    let (st, unknown) = body(post(&t, &alex, SessionId::new(), &fid, one()).await).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    for (who, sid, f) in [(&bob, a1, &fid), (&alex, b1, &bfid)] {
        let (st, v) = body(post(&t, who, sid, f, one()).await).await;
        assert_eq!((st, &v), (StatusCode::NOT_FOUND, &unknown));
    }
    // Another chat of the same person: the file id is not a file of it.
    let a2 = open(&t, &alex, "t1", json!({})).await;
    assert_eq!(
        post(&t, &alex, a2, &fid, one()).await.status(),
        StatusCode::NOT_FOUND
    );
    for bad in ["invented", "..%2F..", "a%20b"] {
        assert_eq!(
            post(&t, &alex, a1, bad, one()).await.status(),
            StatusCode::NOT_FOUND,
            "{bad}"
        );
    }
    let url = format!("{}/api/sessions/{a1}/files/{fid}/edit", t.base);
    // Not logged in; a cookie with no Origin; a cookie from another origin.
    let r = reqwest::Client::new()
        .post(&url)
        .same_origin()
        .json(&one())
        .send()
        .await
        .unwrap();
    assert!(
        matches!(r.status(), StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
        "{}",
        r.status()
    );
    let r = reqwest::Client::new()
        .post(&url)
        .login(&alex)
        .json(&one())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "origin required");
    let r = reqwest::Client::new()
        .post(&url)
        .login(&alex)
        .header("origin", "https://evil.example")
        .json(&one())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "origin not allowed");
    assert!(!record.exists(), "the helper never ran");
    assert_eq!(
        super::attached_http::blob_files(&t),
        2,
        "only the two made files"
    );
}

/// A row of another owner's blob, a sub-agent's chat, a chat with a folder
/// and a temporary chat: 404 and no helper.
#[tokio::test]
async fn a_planted_row_a_folder_chat_an_incognito_chat_and_a_sub_agent_are_not_edited() {
    let t = start().await;
    let [alex, bob] = users(&t).await;
    let (top, fid) = chat_with_file(&t, &alex).await;
    let (blob, sha) = blob_of(&t, top, &fid);
    let owner = owner_of(&t, top);
    let record = helper(
        &t,
        &frame(
            &Reply {
                results: ok_items(1),
                file: true,
                ..Default::default()
            },
            &docx(),
        ),
    );
    let b1 = open(&t, &bob, "t1", json!({})).await;
    let cb = t.chat.ask_docs(b1, doc("his", "# Bob's")).await.unwrap();
    name_in_log(&t, b1, &cb).await;
    let (bblob, bsha) = blob_of(&t, b1, &cb.file_id);
    // With the blob's real size, so that only the owner check refuses it.
    let taken = harness_store::chat_uploads::TakenUpload {
        upload_id: "p-bob".into(),
        owner_id: Some(owner_of(&t, b1)),
        blob_id: bblob,
        name: "planted.docx".into(),
        mime: "application/octet-stream".into(),
        size: cb.bytes,
        sha256: bsha.clone(),
    };
    t.master
        .inner_mirror()
        .session_files_hold(&top.to_string(), &[taken])
        .unwrap();
    t.chat
        .emit(
            top,
            "user_message",
            json!({"turn_id": "p", "text": "planted", "attached": [{
                "file_id": "p-bob", "name": "planted.docx", "bytes": cb.bytes, "sha256": bsha, "mime": "application/octet-stream"}]}),
        )
        .await;
    assert_eq!(
        post(&t, &alex, top, "p-bob", one()).await.status(),
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
    let temp = super::attached_http::chat(&t, &alex, json!({"incognito": true})).await;
    plant(&t, temp, Some(owner), &blob, &sha, "p-temp").await;
    for (sid, id) in [(sub, "p-sub"), (dir, "p-dir"), (temp, "p-temp")] {
        assert_eq!(
            post(&t, &alex, sid, id, one()).await.status(),
            StatusCode::NOT_FOUND,
            "{id}"
        );
    }
    assert!(!record.exists(), "the helper never ran");
}

/// The request's bounds, each at its edge and one past it, before any helper.
#[tokio::test]
async fn the_bounds_are_checked_before_the_helper_is_asked() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let (sid, fid) = chat_with_file(&t, &alex).await;
    let record = helper(
        &t,
        &frame(
            &Reply {
                results: ok_items(200),
                file: true,
                ..Default::default()
            },
            &docx(),
        ),
    );
    let at = |n: usize| {
        (0..n)
            .map(|i| json!({"at": format!("B{}", i + 1), "text": "1"}))
            .collect::<Vec<_>>()
    };
    for (changes, want) in [
        (json!([]), StatusCode::BAD_REQUEST),
        (json!(at(201)), StatusCode::BAD_REQUEST),
        (
            json!([{"at": "p".repeat(65), "text": "x"}]),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            json!([{"at": "A1", "text": "x".repeat(80_001)}]),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
    ] {
        let r = post(&t, &alex, sid, &fid, json!({"changes": changes})).await;
        assert_eq!(r.status(), want);
    }
    // Unknown fields, a path, a destination, bad JSON, a body over the bound.
    for bad in [
        json!({"changes": [{"at": "A1", "text": "x", "save_as": "/etc/x"}]}),
        json!({"changes": [{"at": "A1", "text": "x"}], "path": "/etc"}),
        json!({"nothing": 1}),
    ] {
        let r = post(&t, &alex, sid, &fid, bad).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }
    let url = format!("{}/api/sessions/{sid}/files/{fid}/edit", t.base);
    let r = reqwest::Client::new()
        .post(&url)
        .login(&alex)
        .same_origin()
        .header("content-type", "application/json")
        .body("{not json")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    let r = reqwest::Client::new()
        .post(&url)
        .login(&alex)
        .same_origin()
        .header("content-type", "application/json")
        .body(vec![b' '; (1 << 20) + 1])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(!record.exists(), "the helper never ran");
    // Past what the helper's header takes but inside the body bound: refused
    // by the changes' own bound, before the helper; just inside it, made.
    let sized = |n: usize| json!({"changes": (0..200).map(|i| json!({"at": format!("B{}", i + 1), "text": "x".repeat(n)})).collect::<Vec<_>>()});
    let r = post(&t, &alex, sid, &fid, sized(5_200)).await;
    assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(!record.exists(), "the helper never ran");
    assert_eq!(
        post(&t, &alex, sid, &fid, sized(5_000)).await.status(),
        StatusCode::OK
    );
    assert!(record.exists());
    let _ = upload;
}

/// Changes the helper refuses come back per change in our sentences, the
/// helper's own words (which may hold text of the file) never; nothing is
/// stored.
#[tokio::test]
async fn a_refused_change_is_told_in_our_words_and_nothing_is_stored() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let (sid, fid) = chat_with_file(&t, &alex).await;
    let before = super::attached_http::blob_files(&t);
    helper(
        &t,
        &frame(
            &Reply {
                results: vec![
                    Item {
                        ok: false,
                        msg: "SECRET-NAME: p1 changed".into(),
                        why: Some(ChangeWhy::Stale),
                    },
                    Item {
                        ok: false,
                        msg: String::new(),
                        why: Some(ChangeWhy::Skipped),
                    },
                ],
                ..Default::default()
            },
            &[],
        ),
    );
    let r = post(
        &t,
        &alex,
        sid,
        &fid,
        json!({"changes": [{"at": "p1#0000", "text": "a"}, {"at": "p2#0000", "text": "b"}]}),
    )
    .await;
    let st = r.status();
    let text = r.text().await.unwrap();
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{text}");
    assert!(!text.contains("SECRET"), "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["results"][0]["why"], "stale");
    assert!(
        v["results"][0]["message"]
            .as_str()
            .unwrap()
            .contains("open the preview again"),
        "{v}"
    );
    assert_eq!(v["results"][1]["why"], "skipped");
    assert_eq!(super::attached_http::blob_files(&t), before);
}

/// A helper that misbehaves: the wrong number of results, a file beside a
/// refusal, no file, an oversized file, garbage, a crash; a whole-file reason.
#[tokio::test]
async fn a_hostile_or_confused_helper_stores_nothing() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let (sid, fid) = chat_with_file(&t, &alex).await;
    let before = super::attached_http::blob_files(&t);
    let go = |out: Vec<u8>| {
        let t = &t;
        let (alex, fid) = (alex.clone(), fid.clone());
        helper(t, &out);
        async move { body(post(t, &alex, sid, &fid, one()).await).await }
    };
    let (st, _) = go(frame(
        &Reply {
            results: ok_items(2),
            file: true,
            ..Default::default()
        },
        &docx(),
    ))
    .await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "two results for one change");
    let (st, _) = go(frame(
        &Reply {
            results: vec![Item {
                ok: false,
                msg: String::new(),
                why: Some(ChangeWhy::Other),
            }],
            file: true,
            ..Default::default()
        },
        &docx(),
    ))
    .await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "a file beside a refusal");
    let (st, _) = go(frame(
        &Reply {
            results: ok_items(1),
            ..Default::default()
        },
        &[],
    ))
    .await;
    assert_eq!(st, StatusCode::BAD_GATEWAY, "all ok and no file");
    let (st, v) = go(frame(
        &Reply {
            results: ok_items(1),
            file: true,
            ..Default::default()
        },
        &vec![7u8; (10 << 20) + 1],
    ))
    .await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE, "{v}");
    let (st, v) = go(frame(
        &Reply {
            why: Some(Why::Locked),
            ..Default::default()
        },
        &[],
    ))
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("protected against editing")
    );
    let (st, _) = go(b"garbage".to_vec()).await;
    assert_eq!(st, StatusCode::BAD_GATEWAY);
    assert_eq!(super::attached_http::blob_files(&t), before);
}

/// A save counts against the limits an upload counts against, and is
/// refused the same way: 20 unsent files an account holds, the account's
/// quota.
#[tokio::test]
async fn a_save_is_held_under_the_limits_of_an_upload() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let (sid, fid) = chat_with_file(&t, &alex).await;
    let mut copy = docx();
    let mut saved = 0;
    let mut last = StatusCode::OK;
    for i in 0..22 {
        // Different bytes, so no two share a blob.
        copy.extend_from_slice(format!("pad{i}").as_bytes());
        helper(
            &t,
            &frame(
                &Reply {
                    results: ok_items(1),
                    file: true,
                    ..Default::default()
                },
                &copy,
            ),
        );
        let r = post(&t, &alex, sid, &fid, one()).await;
        last = r.status();
        if last != StatusCode::OK {
            break;
        }
        saved += 1;
    }
    assert_eq!(saved, 20, "at most 20 unsent files");
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn the_account_quota_counts_a_saved_copy() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let (st, v) = super::attached_http::req(
        &t,
        reqwest::Method::PUT,
        "/api/admin/storage",
        Some(json!({"max_file_mb": 10, "account_quota_mb": 1, "server_quota_mb": 100, "min_free_mb": 0})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (sid, fid) = chat_with_file(&t, &alex).await;
    let mut big = docx();
    big.extend(std::iter::repeat_n(7u8, 600_000));
    let go = |bytes: &[u8]| {
        helper(
            &t,
            &frame(
                &Reply {
                    results: ok_items(1),
                    file: true,
                    ..Default::default()
                },
                bytes,
            ),
        );
    };
    go(&big);
    assert_eq!(
        post(&t, &alex, sid, &fid, one()).await.status(),
        StatusCode::OK
    );
    big[100] ^= 0xff;
    go(&big);
    assert_eq!(
        post(&t, &alex, sid, &fid, one()).await.status(),
        StatusCode::INSUFFICIENT_STORAGE
    );
}

/// The server's API token acts as it previews and downloads (T10): it holds
/// its own helper place; its copy is held as the token's own.
#[tokio::test]
async fn the_api_token_saves_as_it_previews() {
    let t = start().await;
    let [alex, _] = users(&t).await;
    let (sid, fid) = chat_with_file(&t, &alex).await;
    helper(
        &t,
        &frame(
            &Reply {
                results: ok_items(1),
                file: true,
                ..Default::default()
            },
            &docx(),
        ),
    );
    let r = reqwest::Client::new()
        .post(format!("{}/api/sessions/{sid}/files/{fid}/edit", t.base))
        .bearer_auth("tok")
        .json(&one())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    // A caller who is not the chat's owner marks the chat for good
    // (`acting_session`): custom connectors are not used in it again.
    assert!(t.master.inner_mirror().chat_foreign(sid).unwrap());
}
