//! A person's text changes to a Word or Excel file of a coding chat's folder
//! (PLAN.md T100, ADOPTION_PLAN X.60): `POST /api/sessions/{id}/folder-edit`.
//! The master writes nothing: it asks the owning node (a fake here; the real
//! node's rules are in `crates/node/tests/folder_page_edit.rs` and
//! `crates/tools/src/folder_edit.rs`), owner only, one at a time per account,
//! and takes nothing a node says on trust.

use super::folder_preview_http::{T, TOKEN, account, account_with_id, chat, req, start};
use crate::common::{Login as _, SameOrigin as _};

use std::time::Duration;

use harness_core::SessionId;
use harness_protocol::{ChangeResult, NodeToMaster};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn url(sid: SessionId) -> String {
    format!("/api/sessions/{sid}/folder-edit")
}

fn body(path: &str) -> Value {
    json!({"path": path, "changes": [{"at": "p1#00ab", "text": "New"}, {"at": "p2#0001", "text": "Other"}]})
}

async fn post(t: &T, who: &str, sid: SessionId, b: Value) -> (StatusCode, Value) {
    req(t, Method::POST, &url(sid), Some(b), who).await
}

async fn waits_for_node(t: &T) {
    for _ in 0..100 {
        if !t.local.edit_req_ids().is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the node was never asked");
}

#[tokio::test]
async fn the_owner_saves_a_copy_through_the_node() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    let (st, v) = post(
        &t,
        &t.alex,
        sid,
        json!({"path": "docs/ok.docx", "sheet": 2, "changes": [{"at": "B2", "text": "7"}]}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["saved"], "docs/ok (edited).docx");
    assert_eq!(v["name"], "ok (edited).docx");
    assert_eq!(v["results"], json!([{"ok": true}]));
    assert_eq!(
        t.local.edits(),
        [(
            "docs/ok.docx".to_string(),
            Some(2),
            json!([{"at": "B2", "text": "7"}])
        )]
    );
    // The API token acts as it does everywhere else.
    let (st, _) = post(&t, TOKEN, sid, body("ok.docx")).await;
    assert_eq!(st, StatusCode::OK);
    // A caller who is not the chat's owner marks the chat for good
    // (`acting_session`): custom connectors are not used in it again.
    assert!(t.master.inner_mirror().chat_foreign(sid).unwrap());
}

#[tokio::test]
async fn nobody_else_saves_and_a_cross_origin_post_is_refused_and_the_node_is_never_asked() {
    let t = start().await;
    let bob = account(&t, "bob", "member").await;
    let carol = account(&t, "carol", "admin").await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    for (who, name) in [(&bob, "a member"), (&carol, "a server admin")] {
        let (st, v) = post(&t, who, sid, body("ok.docx")).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{name}: {v}");
        assert_eq!(v["error"], "unknown session", "{name}");
    }
    let (st, _) = post(&t, "nope", sid, body("ok.docx")).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let full = format!("{}{}", t.base, url(sid));
    let r = reqwest::Client::new()
        .post(&full)
        .login(&t.alex)
        .json(&body("ok.docx"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "origin required");
    let r = reqwest::Client::new()
        .post(&full)
        .login(&t.alex)
        .header("origin", "https://evil.example")
        .json(&body("ok.docx"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN, "origin not allowed");
    assert!(t.local.edits().is_empty());
}

#[tokio::test]
async fn a_plain_chat_a_temporary_one_and_a_bad_path_are_404_and_ask_nothing() {
    let t = start().await;
    let plain = chat(&t, &t.alex, "plain", json!({"profile": "chat"})).await;
    let (st, _) = post(&t, &t.alex, plain, body("ok.docx")).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let temp = chat(&t, &t.alex, "inc", json!({})).await;
    let (st, _) = post(&t, &t.alex, temp, body("ok.docx")).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    for p in ["", &"a".repeat(1025)] {
        let (st, _) = post(&t, &t.alex, sid, body(p)).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{:.20}", p);
    }
    let (st, _) = post(
        &t,
        &t.alex,
        sid,
        json!({"changes": [{"at": "A1", "text": "x"}]}),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND, "no path");
    assert!(t.local.edits().is_empty() && t.plain.edits().is_empty() && t.inc.edits().is_empty());
}

#[tokio::test]
async fn the_bounds_are_checked_before_the_node_is_asked() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    let ch = |n: usize| {
        (0..n)
            .map(|i| json!({"at": format!("B{}", i + 1), "text": "1"}))
            .collect::<Vec<_>>()
    };
    for (b, want) in [
        (
            json!({"path": "ok.docx", "changes": []}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"path": "ok.docx", "changes": ch(201)}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"path": "ok.docx", "changes": [{"at": "p".repeat(65), "text": "x"}]}),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            json!({"path": "ok.docx", "changes": [{"at": "A1", "text": "x".repeat(80_001)}]}),
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            json!({"path": "ok.docx", "changes": [{"at": "A1", "text": "x", "save_as": "/etc/x"}]}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"path": "ok.docx", "to": "/etc/x", "changes": ch(1)}),
            StatusCode::BAD_REQUEST,
        ),
        (json!({"path": "ok.docx"}), StatusCode::BAD_REQUEST),
    ] {
        let (st, v) = post(&t, &t.alex, sid, b).await;
        assert_eq!(st, want, "{v}");
    }
    let r = reqwest::Client::new()
        .post(format!("{}{}", t.base, url(sid)))
        .login(&t.alex)
        .same_origin()
        .header("content-type", "application/json")
        .body(vec![b' '; (1 << 20) + 1])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let sized = |n: usize| json!({"path": "ok.docx", "changes": (0..200).map(|i| json!({"at": format!("B{}", i + 1), "text": "x".repeat(n)})).collect::<Vec<_>>()});
    let (st, _) = post(&t, &t.alex, sid, sized(5_200)).await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(t.local.edits().is_empty(), "never asked");
    let (st, v) = post(&t, &t.alex, sid, sized(5_000)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(t.local.edits().len(), 1);
}

#[tokio::test]
async fn each_refusal_has_a_sentence_and_per_change_reasons_are_ours() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    let (st, v) = post(&t, &t.alex, sid, body("stale.docx")).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["results"][0]["why"], "stale");
    assert!(
        v["results"][0]["message"]
            .as_str()
            .unwrap()
            .contains("open the preview again")
    );
    assert_eq!(v["results"][1]["why"], "skipped");
    assert!(v.get("saved").is_none());
    for (name, st, has) in [
        (
            "locked.docx",
            StatusCode::UNPROCESSABLE_ENTITY,
            "protected against editing",
        ),
        (
            "nowrite.docx",
            StatusCode::UNPROCESSABLE_ENTITY,
            "does not let a copy be saved",
        ),
        (
            "toolarge.docx",
            StatusCode::UNPROCESSABLE_ENTITY,
            "open it in Word or Excel",
        ),
        ("full.docx", StatusCode::SERVICE_UNAVAILABLE, "try again"),
        ("missing.docx", StatusCode::NOT_FOUND, "unknown file"),
        // A word this master does not know: a 502, not a broken link.
        ("newer.docx", StatusCode::BAD_GATEWAY, "not understood"),
    ] {
        let (got, v) = post(&t, &t.alex, sid, body(name)).await;
        assert_eq!(got, st, "{name}: {v}");
        assert!(v["error"].as_str().unwrap().contains(has), "{name}: {v}");
    }
}

/// What a node that misbehaves says is not forwarded and not believed: a path
/// that is absolute, climbs, holds a control character or is too long; an
/// answer with more or fewer results than changes, a copy beside a refused
/// change, no copy beside all ok.
#[tokio::test]
async fn a_node_that_misbehaves_is_a_502_and_nothing_it_says_is_forwarded() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    for name in [
        "dotdot.docx",
        "abs.docx",
        "ctrl.docx",
        "bidi.docx",
        "zwsp.docx",
        "bom.docx",
        "private.docx",
        "softhyphen.docx",
        "long.docx",
        "empty.docx",
        "more.docx",
        "fewer.docx",
        "mixed.docx",
        "nofile.docx",
    ] {
        let (st, v) = post(&t, &t.alex, sid, body(name)).await;
        assert_eq!(st, StatusCode::BAD_GATEWAY, "{name}: {v}");
        let text = v.to_string();
        assert!(
            !text.contains("etc") && !text.contains("..") && !text.contains("aaaa"),
            "{name}: {text}"
        );
    }
}

#[tokio::test]
async fn an_old_node_and_an_away_node_get_a_sentence_not_a_hang() {
    let t = start().await;
    let old = chat(&t, &t.alex, "old", json!({})).await;
    let (st, v) = post(&t, &t.alex, old, body("ok.docx")).await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v["error"].as_str().unwrap().contains("update the node"),
        "{v}"
    );
    assert!(t.old.edits().is_empty());
    let away = chat(&t, &t.alex, "local", json!({})).await;
    t.local.go_away();
    for _ in 0..100 {
        if !t.master.is_connected("local") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (st, v) = post(&t, &t.alex, away, body("ok.docx")).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{v}");
    assert!(v["error"].as_str().unwrap().contains("is away"), "{v}");
}

#[tokio::test]
async fn an_owner_who_lost_the_level_is_refused_and_the_node_is_never_asked() {
    let t = start().await;
    let (bob, bob_id) = account_with_id(&t, "bob", "member").await;
    let sid = chat(&t, &bob, "remote", json!({"exec_mode": "host"})).await;
    let (st, v) = post(&t, &bob, sid, body("ok.docx")).await;
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
    let (st, _) = post(&t, &bob, sid, body("ok.docx")).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert_eq!(t.remote.edits().len(), 1, "only the first was asked");
}

/// One place per account, shared with the preview; an answer of another node
/// or of the other kind of request is ignored; a page that gives up frees the
/// place and a late answer finds nobody.
#[tokio::test]
async fn one_at_a_time_and_a_wrong_answer_is_ignored() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    let (base, alex) = (t.base.clone(), t.alex.clone());
    let slow = tokio::spawn(async move {
        let r = reqwest::Client::new()
            .post(format!("{base}/api/sessions/{sid}/folder-edit"))
            .login(&alex)
            .same_origin()
            .json(&body("slow.docx"))
            .send()
            .await
            .unwrap();
        (r.status(), r.text().await.unwrap())
    });
    waits_for_node(&t).await;
    let rid = t.local.edit_req_ids()[0].clone();
    // A second save, and a preview, of the same account wait their turn.
    let (st, _) = post(&t, &t.alex, sid, body("ok.docx")).await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    let (st, _) = req(
        &t,
        Method::GET,
        &format!("/api/sessions/{sid}/folder-preview?path=ok.docx"),
        None,
        &t.alex,
    )
    .await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    // Forged answers: from another node, and the preview's kind from the
    // asked node.
    t.old.say(NodeToMaster::DocPageEditResult {
        req_id: rid.clone(),
        saved: Some("FORGED (edited).docx".into()),
        results: vec![
            ChangeResult {
                ok: true,
                why: None
            };
            2
        ],
        refusal: None,
    });
    t.local.say(NodeToMaster::DocPreviewResult {
        req_id: rid,
        preview: Some(json!({"kind": "doc", "blocks": [], "more": false, "notes": []})),
        refusal: None,
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!slow.is_finished(), "a forged answer ended the request");
    t.local.release.send(true).unwrap();
    let (st, text) = slow.await.unwrap();
    assert_eq!(st, StatusCode::OK, "{text}");
    assert!(
        text.contains("slow (edited).docx") && !text.contains("FORGED"),
        "{text}"
    );
    // The place is free again.
    let (st, _) = post(&t, &t.alex, sid, body("ok.docx")).await;
    assert_eq!(st, StatusCode::OK);
}

#[tokio::test]
async fn a_request_the_page_gave_up_frees_the_accounts_place() {
    let t = start().await;
    let sid = chat(&t, &t.alex, "local", json!({})).await;
    let (base, alex) = (t.base.clone(), t.alex.clone());
    let slow = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{base}/api/sessions/{sid}/folder-edit"))
            .login(&alex)
            .same_origin()
            .json(&body("slow.docx"))
            .send()
            .await
    });
    waits_for_node(&t).await;
    slow.abort();
    let _ = slow.await;
    let mut st = StatusCode::SERVICE_UNAVAILABLE;
    for _ in 0..100 {
        (st, _) = post(&t, &t.alex, sid, body("ok.docx")).await;
        if st == StatusCode::OK {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(st, StatusCode::OK);
    // The late answer to the abandoned request finds nobody and harms nobody.
    t.local.release.send(true).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (st, _) = post(&t, &t.alex, sid, body("ok.docx")).await;
    assert_eq!(st, StatusCode::OK);
}

/// A sub-agent's chat of the owner shares its parent's folder and is asked as
/// any other coding chat; another person's is not theirs to ask.
#[tokio::test]
async fn a_sub_agent_chat_of_the_owner_is_asked_as_any_other() {
    use harness_core::RawEvent;
    use harness_core::event::{EventKind, Profile, SessionCreated};
    use harness_core::policy::ApprovalMode;
    let t = start().await;
    let bob = account(&t, "bob", "member").await;
    let top = chat(&t, &t.alex, "local", json!({})).await;
    let sub = SessionId::new();
    let (kind, payload) = EventKind::SessionCreated(SessionCreated {
        forked: None,
        exec_mode: Some(harness_core::policy::ExecMode::Sandbox),
        no_callbacks: false,
        title: "sub".into(),
        cwd: "/w".into(),
        model: "demo::mock".into(),
        backend: "native".into(),
        node_id: "local".into(),
        approval_mode: ApprovalMode::Ask,
        profile: Profile::Agentic,
        agent_id: None,
        incognito: false,
        parent_session: Some(top),
        sub_folder: None,
    })
    .into_parts();
    t.local.say(NodeToMaster::Event {
        event: RawEvent {
            session_id: sub,
            seq: 1,
            ts_ms: harness_core::now_ms(),
            kind,
            payload,
        },
    });
    for _ in 0..100 {
        if t.master.inner_mirror().session(sub).unwrap().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let row = t
        .master
        .inner_mirror()
        .session(sub)
        .unwrap()
        .expect("the sub-agent's chat");
    assert_eq!(
        row.parent_id.as_deref().map(|p| p.to_string()),
        Some(top.to_string())
    );
    let (st, v) = post(&t, &t.alex, sub, body("ok.docx")).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(t.local.edits().len(), 1, "the node was asked");
    let (st, _) = post(&t, &bob, sub, body("ok.docx")).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(t.local.edits().len(), 1);
}
