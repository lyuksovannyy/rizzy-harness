//! The owner's switch of a chat's execution mode over HTTP (N.10b, README
//! "Execution modes", PLAN.md T39), against real nodes: only the chat's
//! owner (every other account, admins, workspace admins and the API token
//! on a person's chat get the 404 of a chat that does not exist); the
//! checks of a create for the target mode (level, node rule, the node
//! announced it); an older node is told to update; a chat without tools,
//! a sub-agent's, a routine's and one nobody owns are refused; every
//! request for the host shell is audited; the mirror follows the node's
//! event, fails closed on one it cannot read, and a mode above the owner's
//! level stops the chat's work as it does at any time.

use std::time::Duration;

use harness_core::event::EventKind;
use harness_core::policy::ExecMode;
use harness_core::{RawEvent, SessionId};
use harness_protocol::{MasterToNode, NodeToMaster, PROTOCOL_VERSION};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use super::exec_mode_http::{
    BOTH, HOST_ONLY, Server, TOKEN, account, account_with, call, create, personal, start,
};

/// A node that offers both modes but is older than the switch: its hello
/// has no `exec-switch`. It opens chats (recording the mode asked for) and
/// counts the `session.set_exec_mode` it is sent (none must come).
fn older_node(
    id: &str,
) -> (
    harness_node::NodeLink,
    std::sync::Arc<std::sync::Mutex<usize>>,
) {
    let (to_node, mut commands) = tokio::sync::mpsc::channel::<MasterToNode>(64);
    let (tx, from_node) = tokio::sync::mpsc::channel::<NodeToMaster>(64);
    tx.try_send(NodeToMaster::Hello {
        protocol_version: PROTOCOL_VERSION,
        node_id: id.into(),
        hostname: "h".into(),
        version: "0".into(),
        capabilities: vec![
            "native".into(),
            "profile:agentic".into(),
            "exec:sandbox".into(),
            "exec:host".into(),
        ],
        sessions: vec![],
    })
    .unwrap();
    let sent: std::sync::Arc<std::sync::Mutex<usize>> = Default::default();
    let (seen, node) = (sent.clone(), id.to_string());
    tokio::spawn(async move {
        while let Some(msg) = commands.recv().await {
            match &msg {
                MasterToNode::SetExecMode { .. } => *seen.lock().unwrap() += 1,
                MasterToNode::Create {
                    session_id,
                    exec_mode,
                    ..
                } => {
                    let (kind, payload) =
                        EventKind::SessionCreated(harness_core::event::SessionCreated {
                            forked: None,
                            title: "t".into(),
                            cwd: "/w".into(),
                            model: "m".into(),
                            backend: "native".into(),
                            node_id: node.clone(),
                            approval_mode: harness_core::policy::ApprovalMode::Ask,
                            profile: harness_core::event::Profile::Agentic,
                            agent_id: None,
                            incognito: false,
                            parent_session: None,
                            no_callbacks: false,
                            exec_mode: *exec_mode,
                        })
                        .into_parts();
                    let _ = tx.try_send(NodeToMaster::Event {
                        event: RawEvent {
                            session_id: *session_id,
                            seq: 1,
                            ts_ms: 0,
                            kind,
                            payload,
                        },
                    });
                }
                _ => {}
            }
            if let Some(r) = msg.req_id() {
                let _ = tx.try_send(NodeToMaster::Ack {
                    req_id: r.into(),
                    ok: true,
                    error: None,
                });
            }
        }
    });
    (harness_node::NodeLink { to_node, from_node }, sent)
}

async fn switch(s: &Server, who: &str, sid: &str, mode: &str) -> (StatusCode, Value) {
    call(
        s,
        Method::POST,
        &format!("/api/sessions/{sid}/exec-mode"),
        Some(json!({ "mode": mode })),
        who,
    )
    .await
}

/// The chat's events, from the master's copy.
async fn events(s: &Server, who: &str, sid: &str) -> Vec<Value> {
    let (_, v) = call(
        s,
        Method::GET,
        &format!("/api/sessions/{sid}/events"),
        None,
        who,
    )
    .await;
    v["events"].as_array().cloned().unwrap_or_default()
}

/// The mode changes the chat's log holds, as (mode, from).
async fn changes(s: &Server, who: &str, sid: &str) -> Vec<(String, String)> {
    events(s, who, sid)
        .await
        .iter()
        .filter(|e| e["kind"] == "exec_mode_changed")
        .map(|e| {
            (
                e["payload"]["mode"].as_str().unwrap().to_string(),
                e["payload"]["from"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// An account with the `host` ceiling, logged in: (cookie, user id).
async fn host_account(s: &Server, name: &str) -> (String, String) {
    let (cookie, id) = account_with(s, name, true).await;
    let (st, v) = call(
        s,
        Method::PUT,
        &format!("/api/users/{id}/exec-ceiling"),
        Some(json!({"level": "host"})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    (cookie, id)
}

async fn chat(s: &Server, who: &str, node: &str, mode: Option<&str>) -> String {
    let ws = personal(s, who).await;
    let (st, v) = create(s, who, &ws, node, mode).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["session_id"].as_str().unwrap().to_string()
}

/// Until the chat's copy holds `n` mode changes.
async fn until_changes(s: &Server, who: &str, sid: &str, n: usize) -> Vec<(String, String)> {
    for _ in 0..300 {
        let c = changes(s, who, sid).await;
        if c.len() >= n {
            return c;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no {n} mode changes for {sid}");
}

/// The owner at the `host` level switches both ways; the chat's log says so
/// (the node recorded it, the master's copy has it before the answer).
#[tokio::test]
async fn the_owner_switches_an_idle_chat_both_ways() {
    let s = start().await;
    let (alex, _) = host_account(&s, "alex2").await;
    let sid = chat(&s, &alex, BOTH, None).await;
    let (st, v) = switch(&s, &alex, &sid, "host").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        changes(&s, &alex, &sid).await,
        [("host".to_string(), "sandbox".to_string())]
    );
    let (st, v) = switch(&s, &alex, &sid, "sandbox").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(changes(&s, &alex, &sid).await.len(), 2);
    // Asking for the mode it is in records nothing.
    let (st, _) = switch(&s, &alex, &sid, "sandbox").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(changes(&s, &alex, &sid).await.len(), 2);
    // Unknown words are not a mode.
    let (st, _) = switch(&s, &alex, &sid, "quantum").await;
    assert!(st.is_client_error(), "{st}");
}

/// Not the owner: another member, a server admin, the API token on a
/// person's chat: the 404 of a chat that does not exist, nothing sent to
/// the node, nothing appended.
#[tokio::test]
async fn nobody_but_the_owner_can_switch_a_chat() {
    let s = start().await;
    let (bob, _) = host_account(&s, "bob").await;
    let carol = account(&s, "carol").await;
    let sid = chat(&s, &bob, BOTH, None).await;
    for (who, what) in [
        (carol.as_str(), "another member"),
        (s.admin.as_str(), "a server admin"),
        (TOKEN, "the API token"),
    ] {
        let (st, v) = switch(&s, who, &sid, "host").await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{what}: {v}");
    }
    assert!(changes(&s, &bob, &sid).await.is_empty());
    assert!(
        s.mirror
            .audit_lines("session.exec_mode.host")
            .unwrap()
            .is_empty()
            || s.mirror
                .audit_lines("session.exec_mode.host")
                .unwrap()
                .iter()
                .all(|(_, d)| !d.contains(&sid)),
        "a request for a chat that is not theirs reached the audit as a switch"
    );
    // The owner can.
    let (st, v) = switch(&s, &bob, &sid, "host").await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

/// A chat nobody owns (made with the API token) has no level to check: the
/// token is refused, not given the switch.
#[tokio::test]
async fn a_chat_nobody_owns_is_refused() {
    let s = start().await;
    let (st, v) = call(
        &s,
        Method::POST,
        "/api/sessions",
        Some(json!({"cwd": s.ws.display().to_string(), "title": "t", "node_id": BOTH})),
        TOKEN,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid = v["session_id"].as_str().unwrap().to_string();
    let (st, v) = switch(&s, TOKEN, &sid, "host").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v.to_string().contains("nobody owns"), "{v}");
    assert!(changes(&s, TOKEN, &sid).await.is_empty());
}

/// The level of the create applies: a member at `sandbox` cannot move a
/// chat to the host shell, and the node never hears of it; one whose level
/// was lowered below a host chat's mode can still LEAVE it (the work rule
/// that stops its work is unchanged), but not come back.
#[tokio::test]
async fn the_level_decides_the_target_as_at_create() {
    let s = start().await;
    let (bob, _) = account_with(&s, "bob", true).await;
    let sid = chat(&s, &bob, BOTH, None).await;
    let (st, v) = switch(&s, &bob, &sid, "host").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(
        v.to_string().contains("your account's level is sandbox"),
        "{v}"
    );
    assert!(changes(&s, &bob, &sid).await.is_empty());

    // A host chat of an owner who is then lowered.
    let (carol, carol_id) = host_account(&s, "carol").await;
    let host = chat(&s, &carol, BOTH, Some("host")).await;
    let (st, v) = call(
        &s,
        Method::PUT,
        &format!("/api/users/{carol_id}/exec-ceiling"),
        Some(json!({"level": "sandbox"})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // Its work is refused by the work rule, as before...
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/sessions/{host}/prompt"),
        Some(json!({"text": "hi"})),
        &carol,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    // ...and leaving for the sandbox is allowed; coming back is not.
    let (st, v) = switch(&s, &carol, &host, "sandbox").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, _) = switch(&s, &carol, &host, "host").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

/// The node must offer the target (the master's hello check), and an older
/// node is told to update; neither is sent the command.
#[tokio::test]
async fn the_node_must_offer_the_mode_and_the_switch() {
    let s = start().await;
    let (alex, _) = host_account(&s, "alex3").await;
    // `local` offers only the sandbox: no host there.
    let on_local = chat(&s, &alex, "local", None).await;
    let (st, v) = switch(&s, &alex, &on_local, "host").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v.to_string().contains("does not offer the host mode"),
        "{v}"
    );
    // A node that offers only the host shell: a host chat cannot go to the
    // sandbox there.
    let on_host_only = chat(&s, &alex, HOST_ONLY, Some("host")).await;
    let (st, v) = switch(&s, &alex, &on_host_only, "sandbox").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(
        v.to_string().contains("does not offer the sandbox mode"),
        "{v}"
    );
    // A node from before the switch.
    let (link, sent) = older_node("node-older1");
    s.master.add_link(link, false).await.unwrap();
    let old = chat(&s, &alex, "node-older1", None).await;
    let (st, v) = switch(&s, &alex, &old, "host").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("update the node"), "{v}");
    assert_eq!(
        *sent.lock().unwrap(),
        0,
        "an older node was sent the command"
    );
}

/// A chat without tools has no mode; a sub-agent's chat follows its parent's.
#[tokio::test]
async fn a_chat_and_a_sub_agents_chat_are_refused() {
    let s = start().await;
    let (alex, _) = host_account(&s, "alex4").await;
    let (st, v) = call(
        &s,
        Method::POST,
        "/api/sessions",
        Some(json!({"title": "t", "profile": "chat"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let plain = v["session_id"].as_str().unwrap().to_string();
    let (st, v) = switch(&s, &alex, &plain, "host").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("no execution mode"), "{v}");
    // A sub-agent's chat: the mirror has it as a session of the parent's
    // node whose `session_created` names the parent.
    let parent = chat(&s, &alex, BOTH, None).await;
    let child = SessionId::new();
    let (kind, mut payload) = EventKind::SessionCreated(harness_core::event::SessionCreated {
        forked: None,
        title: "@scout".into(),
        cwd: "/w".into(),
        model: "m".into(),
        backend: "native".into(),
        node_id: BOTH.into(),
        approval_mode: harness_core::policy::ApprovalMode::Ask,
        profile: harness_core::event::Profile::Agentic,
        agent_id: Some("scout".into()),
        incognito: false,
        parent_session: Some(parent.parse().unwrap()),
        no_callbacks: true,
        exec_mode: Some(ExecMode::Sandbox),
    })
    .into_parts();
    payload["parent_session"] = json!(parent);
    s.mirror
        .append(
            BOTH,
            &RawEvent {
                session_id: child,
                seq: 1,
                ts_ms: 0,
                kind,
                payload,
            },
        )
        .unwrap();
    let (st, v) = switch(&s, &alex, &child.to_string(), "host").await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v.to_string().contains("sub-agent"), "{v}");
}

/// Every request for the host shell is audited, whatever refuses it, as a
/// create is; a granted switch to the sandbox is too, and a refused one is
/// not.
#[tokio::test]
async fn every_request_for_the_host_shell_is_audited() {
    let s = start().await;
    let host_lines = |s: &Server| s.mirror.audit_lines("session.exec_mode.host").unwrap();
    let last =
        |s: &Server| -> Value { serde_json::from_str(&host_lines(s).last().unwrap().1).unwrap() };
    let (bob, bob_id) = account_with(&s, "bob", true).await;
    let sid = chat(&s, &bob, BOTH, None).await;
    let (st, _) = switch(&s, &bob, &sid, "host").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    let l = last(&s);
    assert_eq!(l["user"], bob_id.as_str(), "{l}");
    assert_eq!(l["session"], sid.as_str(), "{l}");
    assert_eq!(l["node"], BOTH, "{l}");
    assert_eq!(l["from"], "sandbox", "{l}");
    assert_eq!(l["to"], "host", "{l}");
    assert!(
        l["outcome"]
            .as_str()
            .unwrap()
            .starts_with("refused: your account's level is sandbox"),
        "{l}"
    );
    let (carol, carol_id) = host_account(&s, "carol").await;
    let sid = chat(&s, &carol, BOTH, None).await;
    let (st, _) = switch(&s, &carol, &sid, "host").await;
    assert_eq!(st, StatusCode::OK);
    let l = last(&s);
    assert_eq!(l["user"], carol_id.as_str(), "{l}");
    assert_eq!(l["outcome"], "granted", "{l}");
    assert_eq!(host_lines(&s).len(), 2);
    let (st, _) = switch(&s, &carol, &sid, "sandbox").await;
    assert_eq!(st, StatusCode::OK);
    let back = s.mirror.audit_lines("session.exec_mode.sandbox").unwrap();
    assert_eq!(back.len(), 1, "{back:?}");
    assert_eq!(
        host_lines(&s).len(),
        2,
        "a switch to the sandbox is no host line"
    );
    let _ = until_changes(&s, &carol, &sid, 2).await;
}

/// The mirror takes the mode from the event, whichever node wrote it: a
/// change to a mode the owner's level does not allow stops the chat's work
/// as any such chat is stopped (the work rule), and a mode this build cannot
/// read fails closed.
#[tokio::test]
async fn the_mirror_follows_the_event_and_fails_closed() {
    let s = start().await;
    let (bob, _) = account_with(&s, "bob", true).await;
    let sid = chat(&s, &bob, BOTH, None).await;
    let say = |seq: u64, mode: &str| {
        let s2 = &s;
        let sid2: SessionId = sid.parse().unwrap();
        s2.mirror
            .append(
                BOTH,
                &RawEvent {
                    session_id: sid2,
                    seq,
                    ts_ms: 0,
                    kind: "exec_mode_changed".into(),
                    payload: json!({"mode": mode, "from": "sandbox"}),
                },
            )
            .unwrap();
    };
    let head = events(&s, &bob, &sid).await.len() as u64;
    let work = |text: &'static str| {
        let (s, bob, sid) = (&s, bob.clone(), sid.clone());
        async move {
            call(
                s,
                Method::POST,
                &format!("/api/sessions/{sid}/prompt"),
                Some(json!({"text": text})),
                &bob,
            )
            .await
        }
    };
    // A node that wrote `host` for a chat of an owner at `sandbox`: its
    // work is refused.
    say(head + 1, "host");
    let (st, v) = work("hi").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v.to_string().contains("sandbox"), "{v}");
    // A mode nobody knows: no work at all.
    say(head + 2, "quantum");
    let (st, v) = work("hi again").await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    assert!(v.to_string().contains("execution mode is not known"), "{v}");
}

/// B1: ONE rule everywhere, a session's mode is what its own log says: a
/// sub-agent's row keeps the mode it was created in when its chat is
/// switched (the node takes no turn of it once the two differ), so the
/// work rule and the lowering sweep read the same mode the node runs. The
/// parent's own row moves, and a lowered owner is refused there.
#[tokio::test]
async fn a_sub_agents_row_keeps_its_own_mode_when_its_chat_is_switched() {
    let s = start().await;
    let (alex, alex_id) = host_account(&s, "alexp").await;
    let parent = chat(&s, &alex, BOTH, None).await;
    let child = SessionId::new();
    let (kind, mut payload) = EventKind::SessionCreated(harness_core::event::SessionCreated {
        forked: None,
        title: "@helper".into(),
        cwd: "/w".into(),
        model: "m".into(),
        backend: "native".into(),
        node_id: BOTH.into(),
        approval_mode: harness_core::policy::ApprovalMode::Ask,
        profile: harness_core::event::Profile::Agentic,
        agent_id: None,
        incognito: false,
        parent_session: Some(parent.parse().unwrap()),
        no_callbacks: true,
        exec_mode: Some(ExecMode::Sandbox),
    })
    .into_parts();
    payload["parent_session"] = json!(parent);
    s.mirror
        .append(
            BOTH,
            &RawEvent {
                session_id: child,
                seq: 1,
                ts_ms: 0,
                kind,
                payload,
            },
        )
        .unwrap();
    let (st, v) = switch(&s, &alex, &parent, "host").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = call(
        &s,
        Method::PUT,
        &format!("/api/users/{alex_id}/exec-ceiling"),
        Some(json!({"level": "sandbox"})),
        &s.admin,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let prow = s.mirror.session(parent.parse().unwrap()).unwrap().unwrap();
    let crow = s.mirror.session(child).unwrap().unwrap();
    assert_eq!(prow.exec_mode.as_deref(), Some("host"));
    assert_eq!(
        crow.exec_mode.as_deref(),
        Some("sandbox"),
        "the child's row moved"
    );
    // The lowered owner: the host chat is refused by the work rule; the
    // child, in the sandbox its log says, is within their level (the node
    // refuses it for its mode: `exec_switch.rs`, B1).
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/sessions/{parent}/prompt"),
        Some(json!({"text": "hi"})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/sessions/{child}/prompt"),
        Some(json!({"text": "hi"})),
        &alex,
    )
    .await;
    assert!(
        !v.to_string().contains("does not include the host mode"),
        "the child was judged as a host chat: {st} {v}"
    );
}

/// D2 through the master: a fork of an earlier turn, made after the chat was
/// switched, is a chat in the mode its parent has now; its log carries its
/// own `exec_mode_changed`, and the mirror's row agrees.
#[tokio::test]
async fn a_fork_after_a_switch_is_in_the_parents_mode_now() {
    let s = start().await;
    let (alex, _) = host_account(&s, "alexf").await;
    let sid = chat(&s, &alex, BOTH, None).await;
    let turn = |n: usize| {
        let (s, alex, sid) = (&s, alex.clone(), sid.clone());
        async move {
            let (st, v) = call(
                s,
                Method::POST,
                &format!("/api/sessions/{sid}/prompt"),
                Some(json!({"text": "hello"})),
                &alex,
            )
            .await;
            assert_eq!(st, StatusCode::OK, "{v}");
            for _ in 0..500 {
                let ended: Vec<Value> = events(s, &alex, &sid)
                    .await
                    .into_iter()
                    .filter(|e| e["kind"] == "turn_ended")
                    .collect();
                if ended.len() >= n {
                    return ended[n - 1]["payload"]["turn_id"]
                        .as_str()
                        .unwrap()
                        .to_string();
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            panic!("turn {n} did not end");
        }
    };
    let first = turn(1).await;
    let (st, v) = switch(&s, &alex, &sid, "host").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (st, v) = call(
        &s,
        Method::POST,
        &format!("/api/sessions/{sid}/fork"),
        Some(json!({"turn_id": first})),
        &alex,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let fork = v["session_id"].as_str().unwrap().to_string();
    assert_eq!(
        until_changes(&s, &alex, &fork, 1).await,
        [("host".to_string(), "sandbox".to_string())]
    );
    let row = s.mirror.session(fork.parse().unwrap()).unwrap().unwrap();
    assert_eq!(row.exec_mode.as_deref(), Some("host"));
}
