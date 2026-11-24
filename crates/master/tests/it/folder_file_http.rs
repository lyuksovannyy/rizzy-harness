//! Reading a text file of a coding chat's folder (PLAN.md T99, ADOPTION_PLAN
//! X.63): `GET /api/sessions/{id}/folder-file?path=`. The master reads
//! nothing: it asks the owning node (a fake here; the real node's rules are in
//! `crates/node/tests/folder_file.rs` and `crates/tools/src/folder_file.rs`),
//! owner only, one at a time per account.

use super::folder_preview_http::{T, TOKEN, account, account_with_id, chat, req, start};

use std::time::Duration;

use harness_core::SessionId;
use harness_protocol::{FolderFileText, NodeToMaster};
use reqwest::{Method, StatusCode};
use serde_json::json;

use crate::common::{Login as _, SameOrigin as _};

fn url(sid: SessionId, query: &str) -> String {
    format!("/api/sessions/{sid}/folder-file{query}")
}

async fn get(t: &T, sid: SessionId, query: &str, who: &str) -> (StatusCode, serde_json::Value) {
    req(t, Method::GET, &url(sid, query), None, who).await
}

#[tokio::test]
async fn the_owner_reads_a_folder_file_through_the_node() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    let (st, v) = get(&t, sid, "?path=src/a.rs", &t.alex).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["name"], "a.rs", "the name is the page's own path's");
    assert_eq!(v["text"], "fn main() {}\n");
    assert_eq!(v["size"], 13);
    // The flags that are false are not there.
    assert!(
        v.get("truncated").is_none() && v.get("lossy").is_none(),
        "{v}"
    );
    assert_eq!(t.local.files_asked(), ["src/a.rs"]);
    // The API token reads any chat, as it can everywhere else.
    let (st, _) = get(&t, sid, "?path=a.rs", TOKEN).await;
    assert_eq!(st, StatusCode::OK);
    // A path with characters the query must encode arrives as written.
    let (st, _) = get(&t, sid, "?path=a%20b%26c/a.rs", &t.alex).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(t.local.files_asked()[2], "a b&c/a.rs");
}

/// Another member, a server admin: the 404 of a chat that does not exist, and
/// the node is never asked. No login: 401.
#[tokio::test]
async fn nobody_else_reads_not_even_an_admin_and_the_node_is_never_asked() {
    let t = start().await;
    let bob = account(&t, "bob", "member").await;
    let carol = account(&t, "carol", "admin").await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    for (who, name) in [(&bob, "a member"), (&carol, "a server admin")] {
        let (st, v) = get(&t, sid, "?path=a.rs", who).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}");
    }
    let (st, _) = get(&t, sid, "?path=a.rs", "nope").await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert!(t.local.files_asked().is_empty());
}

/// A workspace's own admin is no owner of a member's chat in it either.
#[tokio::test]
async fn a_workspace_admin_is_not_the_owner() {
    let t = start().await;
    let _bob = account(&t, "bob", "member").await;
    let dana = account(&t, "dana", "member").await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/workspaces",
        Some(json!({"name": "team"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let team = v["workspace_id"].as_str().unwrap().to_string();
    for (who, role) in [("bob", "member"), ("dana", "admin")] {
        let (st, v) = req(
            &t,
            Method::POST,
            &format!("/api/workspaces/{team}/members"),
            Some(json!({"username": who, "role": role})),
            &t.alex,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    let sid = chat(&t, &t.alex, "local", json!({"workspace_id": team})).await;
    let (st, v) = get(&t, sid, "?path=a.rs", &dana).await;
    assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
    assert_eq!(v["error"], "unknown session");
    assert!(t.local.files_asked().is_empty());
}

#[tokio::test]
async fn a_plain_chat_a_temporary_chat_and_a_bad_path_are_404_and_ask_nothing() {
    let t = start().await;
    let plain = chat(&t, &t.alex, "plain", json!({"profile": "chat"})).await;
    let (st, _) = get(&t, plain, "?path=a.rs", &t.alex).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let inc = chat(&t, &t.alex, "inc", json!({})).await;
    let (st, _) = get(&t, inc, "?path=a.rs", &t.alex).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    for q in ["", "?path=", &format!("?path={}", "a".repeat(1025))] {
        let (st, _) = get(&t, sid, q, &t.alex).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{q:.30}");
    }
    // The longest path is passed on.
    let long = format!("?path={}", "a".repeat(1024));
    let (st, _) = get(&t, sid, &long, &t.alex).await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "the fake node knows no such file"
    );
    assert_eq!(t.local.files_asked().len(), 1);
    assert!(t.plain.files_asked().is_empty() && t.inc.files_asked().is_empty());
    // An unknown chat.
    let (st, _) = get(&t, SessionId::new(), "?path=a.rs", &t.alex).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

/// Each refusal of the node is a plain sentence with its own status, never a
/// 500; what the page asks about a path the node will not show is the same
/// 404 as any missing file.
#[tokio::test]
async fn each_refusal_has_its_sentence() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    for (path, code, has) in [
        ("../etc/passwd", 404, "unknown file"),
        ("gone.txt", 404, "unknown file"),
        ("bin.dat", 422, "not text"),
        ("denied.txt", 403, "does not let files be read"),
        ("busy.txt", 503, "try again"),
        ("newer.txt", 502, "not understood"),
        // A node that sends more text than may be, is not forwarded.
        ("huge.txt", 502, "not understood"),
    ] {
        let (st, v) = get(&t, sid, &format!("?path={path}"), &t.alex).await;
        assert_eq!(st.as_u16(), code, "{path}: {v}");
        assert!(v["error"].as_str().unwrap().contains(has), "{path}: {v}");
    }
}

#[tokio::test]
async fn an_old_node_and_an_away_node_get_a_sentence_not_a_hang() {
    let t = start().await;
    let old = chat(&t, &t.alex, "old", json!({})).await;
    let (st, v) = get(&t, old, "?path=a.rs", &t.alex).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"]
            .as_str()
            .unwrap()
            .contains("cannot show files; update"),
        "{v}"
    );
    assert!(t.old.files_asked().is_empty());
    let away = chat(&t, &t.alex, "local", json!({})).await;
    t.local.go_away();
    for _ in 0..100 {
        if !t.master.is_connected("local") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (st, v) = get(&t, away, "?path=a.rs", &t.alex).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("is away"), "{v}");
}

/// An owner whose level no longer allows agentic chats is refused, and the
/// node is never asked.
#[tokio::test]
async fn an_owner_who_lost_the_level_is_refused_and_the_node_is_never_asked() {
    let t = start().await;
    let (bob, bob_id) = account_with_id(&t, "bob", "member").await;
    let sid = chat(&t, &bob, "remote", json!({"exec_mode": "host"})).await;
    let (st, v) = get(&t, sid, "?path=a.rs", &bob).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = req(
        &t,
        Method::PUT,
        &format!("/api/users/{bob_id}/exec-ceiling"),
        Some(json!({"level": "sandbox"})),
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = get(&t, sid, "?path=a.rs", &bob).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(t.remote.files_asked().len(), 1, "only the first was asked");
}

async fn slow(t: &T, sid: SessionId) -> tokio::task::JoinHandle<(StatusCode, String)> {
    let (base, alex) = (t.base.clone(), t.alex.clone());
    let h = tokio::spawn(async move {
        let r = reqwest::Client::new()
            .get(format!(
                "{base}/api/sessions/{sid}/folder-file?path=slow.txt"
            ))
            .login(&alex)
            .same_origin()
            .send()
            .await
            .unwrap();
        (r.status(), r.text().await.unwrap())
    });
    for _ in 0..100 {
        if !t.local.file_req_ids().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    h
}

/// One account's reads run one at a time (the places are shared with the
/// document previews): a second is told to wait; another account is not held
/// up. An answer from a node that was not asked is ignored.
#[tokio::test]
async fn one_account_reads_one_file_at_a_time_and_a_forged_answer_is_ignored() {
    let t = start().await;
    let bob = account(&t, "bob", "member").await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    let bsid = chat(&t, &bob, "remote", json!({})).await;
    let first = slow(&t, sid).await;
    let (st, v) = get(&t, sid, "?path=a.rs", &t.alex).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    // A document preview of the same account waits for the same place.
    let (st, _) = req(
        &t,
        Method::GET,
        &format!("/api/sessions/{sid}/folder-preview?path=ok.docx"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    let (st, v) = get(&t, bsid, "?path=a.rs", &bob).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // Another node answers for the waiting request: ignored.
    t.old.say(NodeToMaster::FolderFileResult {
        req_id: t.local.file_req_ids()[0].clone(),
        file: Some(FolderFileText {
            text: "FORGED".into(),
            size: 6,
            truncated: false,
            lossy: false,
        }),
        refusal: None,
    });
    // And the node's own answer to another kind of request is not taken.
    t.old.say(NodeToMaster::DocPreviewResult {
        req_id: t.local.file_req_ids()[0].clone(),
        preview: Some(json!({"kind": "doc"})),
        refusal: None,
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!first.is_finished(), "a forged answer ended the request");
    t.local.release.send(true).unwrap();
    let (st, body) = first.await.unwrap();
    assert_eq!(st, StatusCode::OK, "{body}");
    assert!(body.contains("late") && !body.contains("FORGED"), "{body}");
    let (st, _) = get(&t, sid, "?path=a.rs", &t.alex).await;
    assert_eq!(st, StatusCode::OK);
}

/// A person who closes the page while the node is still reading gives the
/// account's place back at once.
#[tokio::test]
async fn a_request_the_page_gave_up_frees_the_accounts_place() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    let first = slow(&t, sid).await;
    let (st, _) = get(&t, sid, "?path=a.rs", &t.alex).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "the place is taken");
    first.abort();
    let _ = first.await;
    let mut got = StatusCode::SERVICE_UNAVAILABLE;
    for _ in 0..60 {
        got = get(&t, sid, "?path=a.rs", &t.alex).await.0;
        if got == StatusCode::OK {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(got, StatusCode::OK);
}
