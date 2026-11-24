//! The credential for an agent's `push` (ADOPTION_PLAN X.27 step 4,
//! PLAN.md T101), against the clone tests' fake forge and fake node: a
//! project cloned with Alice's connection, an agentic chat in it, turns the
//! master sends, the node's events and its `forge.push_credential`. The
//! token is handed out only for an Allow of a push card that the master
//! itself relayed from the chat's owner, once; never to another node, for
//! another tool's card, after the owner lost the right, past the hourly cap,
//! and the master names a target only in the owner's own signed-in turn.

use std::time::Duration;

use harness_core::SessionId;
use harness_core::forge::PushTarget;
use harness_master::PromptOpts;
use harness_protocol::{CAP_FORGE_PUSH, CAP_PROJECTS, MasterToNode, NodeToMaster, PromptMsg};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use super::forge_clone_http::{
    T, TOKEN_A, assert_no_token_anywhere, clone_body, connect, fake, member, ready, req, start,
};

struct Chat {
    t: T,
    alice: String,
    alice_id: String,
    sid: SessionId,
    /// The fake node that announces `forge-push` and runs the chat.
    node: super::forge_clone_http::Node,
}

/// Alice's connection, a project cloned with it on a node that pushes, and
/// an agentic chat of hers in that project.
async fn chat() -> Chat {
    let t = start().await;
    let (l, node) = fake("pusher", &[CAP_PROJECTS, "forge-clone:600", CAP_FORGE_PUSH]);
    t.master.add_link(l, false).await.unwrap();
    let (alice, alice_id) = member(&t, "alice").await;
    let conn = connect(&t, &alice, TOKEN_A).await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/projects",
        Some(clone_body(&conn, "ada/secret", "pusher")),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    let pid = v["project_id"].as_str().unwrap().to_string();
    ready(&t, &alice, &pid).await;
    let (st, v) = req(
        &t,
        Method::POST,
        "/api/sessions",
        Some(json!({"profile": "agentic", "project_id": pid, "exec_mode": "sandbox"})),
        &alice,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let sid: SessionId = v["session_id"].as_str().unwrap().parse().unwrap();
    for _ in 0..200 {
        if t.master.inner_mirror().session(sid).unwrap().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Chat {
        t,
        alice,
        alice_id,
        sid,
        node,
    }
}

impl Chat {
    async fn emit(&self, kind: &str, payload: Value) {
        let seq = {
            let mut s = self.node.seqs.lock().unwrap();
            let n = s.entry(self.sid).or_insert(0);
            *n += 1;
            *n
        };
        self.node
            .tx
            .send(NodeToMaster::Event {
                event: harness_core::RawEvent {
                    session_id: self.sid,
                    seq,
                    ts_ms: 10 + seq as i64,
                    kind: kind.into(),
                    payload,
                },
            })
            .await
            .unwrap();
        for _ in 0..200 {
            if self.t.master.inner_mirror().head_seq(self.sid).unwrap_or(0) >= seq {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("event {kind} did not land");
    }

    /// The master sends a turn with `opts`; the push target it named.
    async fn turn(&self, opts: PromptOpts) -> Option<PushTarget> {
        self.t
            .master
            .prompt(
                self.sid,
                "hello".into(),
                PromptOpts {
                    agent_id: Some("coder".into()),
                    ..opts
                },
            )
            .await
            .unwrap();
        self.node
            .seen
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|m| match m {
                MasterToNode::Prompt(PromptMsg { context, .. }) => {
                    Some(context.as_ref().and_then(|c| c.push.clone()))
                }
                _ => None,
            })
            .expect("a prompt")
    }

    fn owner() -> PromptOpts {
        PromptOpts {
            by_owner: true,
            ..Default::default()
        }
    }

    /// The node records turn `turn` running as the coder, and a card of
    /// `tool` with `approval_id`.
    async fn card(&self, turn: &str, tool: &str, approval_id: &str) {
        if self.open_turn() != Some(turn.to_string()) {
            self.emit(
                "turn_started",
                json!({"turn_id": turn,
                    "agent": {"agent_id": "coder", "name": "Coder", "prompt_hash": "h"}}),
            )
            .await;
            self.emit("user_message", json!({"turn_id": turn, "text": "hello"}))
                .await;
        }
        let call = format!("call-{approval_id}");
        self.emit(
            "tool_call_started",
            json!({"turn_id": turn, "call_id": call, "tool": tool, "input": {}}),
        )
        .await;
        self.emit(
            "approval_requested",
            json!({"approval_id": approval_id, "call_id": call, "tool": tool,
                   "input": {}, "reason": "push"}),
        )
        .await;
    }

    fn open_turn(&self) -> Option<String> {
        self.t
            .master
            .inner_mirror()
            .open_turn(self.sid)
            .unwrap()
            .map(|o| o.turn_id)
    }

    async fn click(&self, who: &str, approval_id: &str, decision: &str) -> StatusCode {
        let (st, _) = req(
            &self.t,
            Method::POST,
            &format!("/api/sessions/{}/approvals/{approval_id}", self.sid),
            Some(json!({"decision": decision})),
            who,
        )
        .await;
        st
    }

    /// The node asks for approval `approval_id`'s credential (as `node`,
    /// default the chat's own); the master's answer.
    async fn ask(&self, approval_id: &str) -> (Option<String>, Option<String>, Option<String>) {
        self.ask_as(&self.node, approval_id).await
    }

    async fn ask_as(
        &self,
        node: &super::forge_clone_http::Node,
        approval_id: &str,
    ) -> (Option<String>, Option<String>, Option<String>) {
        let req_id = harness_core::new_id();
        node.tx
            .send(NodeToMaster::PushCredential {
                req_id: req_id.clone(),
                session_id: self.sid,
                approval_id: approval_id.into(),
            })
            .await
            .unwrap();
        for _ in 0..500 {
            let found = node.seen.lock().unwrap().iter().find_map(|m| match m {
                MasterToNode::PushCredentialResult {
                    req_id: r,
                    username,
                    token,
                    error,
                } if *r == req_id => Some((
                    username.clone(),
                    token.as_ref().map(|t| t.expose().clone()),
                    error.clone(),
                )),
                _ => None,
            });
            if let Some(f) = found {
                return f;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no answer to the credential request");
    }
}

/// The owner's own turn names the target (the project's source, her newest
/// connection's login); her Allow of the push card, relayed by the master,
/// gets the token once; a second request with the same card, a card never
/// clicked, a click on another tool's card and another node get nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_token_goes_only_for_an_allow_the_master_relayed_once() {
    let c = chat().await;
    let target = c.turn(Chat::owner()).await.expect("a target");
    assert_eq!(target.host, "127.0.0.1");
    assert_eq!(target.full_name, "ada/secret");
    assert_eq!(target.login, "ada");
    // Before any click: nothing.
    c.card("t1", "push", "a1").await;
    let (_, token, error) = c.ask("a1").await;
    assert!(token.is_none(), "{error:?}");
    assert!(error.unwrap().contains("no allow"));
    // The owner's Allow: the token, once.
    assert_eq!(c.click(&c.alice, "a1", "allow").await, StatusCode::OK);
    let (user, token, error) = c.ask("a1").await;
    assert_eq!(user.as_deref(), Some("ada"), "{error:?}");
    assert_eq!(token.as_deref(), Some(TOKEN_A));
    let (_, token, _) = c.ask("a1").await;
    assert!(token.is_none(), "used once");
    // A denial records nothing.
    c.card("t1", "push", "a2").await;
    assert_eq!(c.click(&c.alice, "a2", "deny").await, StatusCode::OK);
    assert!(c.ask("a2").await.1.is_none());
    // An Allow of another tool's card is no push Allow.
    c.card("t1", "bash", "a3").await;
    assert_eq!(c.click(&c.alice, "a3", "allow").await, StatusCode::OK);
    assert!(c.ask("a3").await.1.is_none());
    // Another node asking for this chat: refused, and the click stays unused.
    c.card("t1", "push", "a4").await;
    assert_eq!(c.click(&c.alice, "a4", "allow").await, StatusCode::OK);
    let (l, other) = fake("other", &[CAP_FORGE_PUSH]);
    c.t.master.add_link(l, false).await.unwrap();
    let (_, token, error) = c.ask_as(&other, "a4").await;
    assert!(token.is_none() && error.unwrap().contains("no such session"));
    assert_eq!(c.ask("a4").await.1.as_deref(), Some(TOKEN_A));
    // The audit line names the push, never the token.
    let audit: Vec<String> =
        c.t.master
            .inner_mirror()
            .audit_lines("forge.push_credential")
            .unwrap()
            .into_iter()
            .map(|(_, d)| d)
            .collect();
    assert_eq!(audit.len(), 2, "{audit:?}");
    assert!(audit[0].contains("ada/secret") && !audit[0].contains(TOKEN_A));
    assert_no_token_anywhere(&c.t);
}

/// An Allow from anyone but the chat's owner signed in (the API token) is
/// not a push Allow; after it the chat is marked and names no target.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_api_token_allow_is_no_push_allow() {
    let c = chat().await;
    assert!(c.turn(Chat::owner()).await.is_some());
    c.card("t1", "push", "a1").await;
    let _ = c.click("api", "a1", "allow").await;
    let (_, token, _) = c.ask("a1").await;
    assert!(token.is_none());
    assert!(c.turn(Chat::owner()).await.is_none(), "the chat is marked");
}

/// Turns that name no target: the API token's, a routine run's, a callback
/// (the master sends no context push there), a webhook's (untrusted), and
/// parallel answers.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_the_owners_own_signed_in_turn_names_a_target() {
    let c = chat().await;
    assert!(
        c.turn(PromptOpts {
            untrusted: true,
            by_owner: true,
            ..Default::default()
        })
        .await
        .is_none()
    );
    let c = chat().await;
    assert!(c.turn(Chat::owner()).await.is_some());
    // The API token's message (not by the owner): none, and the chat is
    // marked for good.
    assert!(c.turn(PromptOpts::default()).await.is_none());
    assert!(c.turn(Chat::owner()).await.is_none());
}

/// The owner lost the right after the turn started (disabled, or no longer
/// allowed agentic work): her Allow gets nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_owner_who_lost_the_right_gets_nothing() {
    for loss in ["disabled", "level"] {
        let c = chat().await;
        assert!(c.turn(Chat::owner()).await.is_some());
        c.card("t1", "push", "a1").await;
        assert_eq!(c.click(&c.alice, "a1", "allow").await, StatusCode::OK);
        let (method, path, body) = match loss {
            "disabled" => (
                Method::PATCH,
                format!("/api/users/{}", c.alice_id),
                json!({"disabled": true}),
            ),
            _ => (
                Method::PUT,
                format!("/api/users/{}/exec-ceiling", c.alice_id),
                json!({"level": "none"}),
            ),
        };
        let (st, v) = req(&c.t, method, &path, Some(body), &c.t.alex).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        let (_, token, error) = c.ask("a1").await;
        assert!(token.is_none(), "{loss}: {error:?}");
    }
}

/// At most `PER_HOUR` credentials per chat in any hour.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hourly_cap_holds() {
    let c = chat().await;
    assert!(c.turn(Chat::owner()).await.is_some());
    for i in 0..=harness_master::forge_push::PER_HOUR {
        let a = format!("a{i}");
        c.card("t1", "push", &a).await;
        assert_eq!(c.click(&c.alice, &a, "allow").await, StatusCode::OK);
        let (_, token, error) = c.ask(&a).await;
        if i < harness_master::forge_push::PER_HOUR {
            assert!(token.is_some(), "{i}: {error:?}");
        } else {
            assert!(token.is_none() && error.unwrap().contains("last hour"));
        }
    }
}

/// A callback turn of the chat: the open turn's cause makes the master
/// refuse, whatever was clicked.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_callback_turn_gets_nothing() {
    let c = chat().await;
    assert!(c.turn(Chat::owner()).await.is_some());
    c.emit(
        "turn_started",
        json!({"turn_id": "cb", "cause": {"callback": ["bg-1"]},
            "agent": {"agent_id": "coder", "name": "Coder", "prompt_hash": "h"}}),
    )
    .await;
    c.card("cb", "push", "a1").await;
    assert_eq!(c.click(&c.alice, "a1", "allow").await, StatusCode::OK);
    let (_, token, error) = c.ask("a1").await;
    assert!(token.is_none() && error.unwrap().contains("not available in this turn"));
}

// ------------------------------------------------- the review's findings

/// (F2) The page draws a card from its call's `tool_call_started`; a node
/// that labels a `bash ls` call's approval as `push` gets nothing for the
/// owner's Allow of what she saw as `bash`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_card_whose_call_is_not_a_push_gets_nothing() {
    let c = chat().await;
    assert!(c.turn(Chat::owner()).await.is_some());
    c.card("t1", "push", "warm").await;
    c.emit(
        "tool_call_started",
        json!({"turn_id": "t1", "call_id": "cx", "tool": "bash", "input": {"command": "ls"}}),
    )
    .await;
    c.emit(
        "approval_requested",
        json!({"approval_id": "m1", "call_id": "cx", "tool": "push",
               "input": {}, "reason": "`bash` can change files or run programs"}),
    )
    .await;
    assert_eq!(c.click(&c.alice, "m1", "allow").await, StatusCode::OK);
    let (_, token, error) = c.ask("m1").await;
    assert!(
        token.is_none(),
        "a bash card's Allow was a push Allow: {error:?}"
    );
}

/// (F3) One click, one token, ever: a second Allow of a used card, and an
/// Allow of a card already answered, hand out nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_used_or_answered_card_hands_out_nothing_again() {
    let c = chat().await;
    assert!(c.turn(Chat::owner()).await.is_some());
    c.card("t1", "push", "a1").await;
    assert_eq!(c.click(&c.alice, "a1", "allow").await, StatusCode::OK);
    assert_eq!(c.ask("a1").await.1.as_deref(), Some(TOKEN_A));
    assert_eq!(c.click(&c.alice, "a1", "allow").await, StatusCode::OK);
    let (_, token, error) = c.ask("a1").await;
    assert!(token.is_none(), "a used card re-armed: {error:?}");
    // A card the node already recorded as answered.
    c.card("t1", "push", "a2").await;
    c.emit(
        "approval_resolved",
        json!({"approval_id": "a2", "decision": "deny", "by": "user"}),
    )
    .await;
    assert_eq!(c.click(&c.alice, "a2", "allow").await, StatusCode::OK);
    assert!(c.ask("a2").await.1.is_none());
}

/// A chat someone else acted in after the turn started (here the API
/// token's answer to another card): the owner's Allow gets nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_chat_marked_after_the_turn_started_gets_nothing() {
    let c = chat().await;
    assert!(c.turn(Chat::owner()).await.is_some());
    c.card("t1", "push", "a0").await;
    let _ = c.click("api", "a0", "deny").await;
    c.card("t1", "push", "a1").await;
    assert_eq!(c.click(&c.alice, "a1", "allow").await, StatusCode::OK);
    let (_, token, error) = c.ask("a1").await;
    assert!(token.is_none());
    assert!(error.unwrap().contains("someone other than"));
}

/// A forked chat names no target.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_forked_chat_names_no_target() {
    let c = chat().await;
    let db = rusqlite::Connection::open(c.t.dir.path().join("master.db")).unwrap();
    db.execute(
        "INSERT INTO session_forks (session_id, parent_id, through_seq, created_ms)
         VALUES (?1, ?2, 1, 1)",
        rusqlite::params![c.sid.to_string(), SessionId::new().to_string()],
    )
    .unwrap();
    assert!(c.turn(Chat::owner()).await.is_none());
}

/// A sub-agent's session (and so a temporary chat's, which is refused the
/// same way) is refused before anything else, by its own rule.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_sub_agents_session_is_refused_by_its_own_rule() {
    let c = chat().await;
    assert!(c.turn(Chat::owner()).await.is_some());
    let kid = SessionId::new();
    c.node
        .tx
        .send(NodeToMaster::Event {
            event: harness_core::RawEvent {
                session_id: kid,
                seq: 1,
                ts_ms: 1,
                kind: "session_created".into(),
                payload: json!({"title": "@coder", "cwd": "/", "model": "m", "backend": "native",
                    "node_id": "pusher", "profile": "agentic", "approval_mode": "auto",
                    "exec_mode": "sandbox", "parent_session": c.sid}),
            },
        })
        .await
        .unwrap();
    for _ in 0..200 {
        if c.t.master.inner_mirror().session(kid).unwrap().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let kc = Chat {
        t: c.t,
        alice: c.alice,
        alice_id: c.alice_id,
        sid: kid,
        node: c.node,
    };
    let (_, token, error) = kc.ask("a1").await;
    assert!(token.is_none());
    assert!(
        error
            .unwrap()
            .contains("only for a project chat's own turn")
    );
}

/// One request per chat at a time: while one is being answered, another
/// is refused with its own sentence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_request_of_the_chat_meanwhile_is_refused() {
    let c = chat().await;
    assert!(c.turn(Chat::owner()).await.is_some());
    c.card("t1", "push", "a1").await;
    assert_eq!(c.click(&c.alice, "a1", "allow").await, StatusCode::OK);
    let held =
        c.t.master
            .hold_push_request(c.sid)
            .expect("the chat's place");
    let (_, token, error) = c.ask("a1").await;
    assert!(token.is_none());
    assert!(error.unwrap().contains("being answered"));
    drop(held);
    assert_eq!(
        c.ask("a1").await.1.as_deref(),
        Some(TOKEN_A),
        "the click was kept"
    );
}

/// (F7) A token the master could not send to the node is taken back: the
/// click stays usable, nothing counts against the hour, no audit line.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_token_that_could_not_be_sent_is_given_back() {
    let c = chat().await;
    assert!(c.turn(Chat::owner()).await.is_some());
    c.card("t1", "push", "a1").await;
    assert_eq!(c.click(&c.alice, "a1", "allow").await, StatusCode::OK);
    c.node.deaf.notify_one();
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.node
        .tx
        .send(NodeToMaster::PushCredential {
            req_id: "lost".into(),
            session_id: c.sid,
            approval_id: "a1".into(),
        })
        .await
        .unwrap();
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        c.t.master.push_click_pending(c.sid, "a1"),
        "the click was spent"
    );
    let audit =
        c.t.master
            .inner_mirror()
            .audit_lines("forge.push_credential")
            .unwrap();
    assert!(audit.is_empty(), "{audit:?}");
}

/// (F2c) The page shows the destination the master knows for the chat's
/// open push card: its owner only.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_page_gets_the_masters_destination_for_the_owner_only() {
    let c = chat().await;
    let path = format!("/api/sessions/{}/push-target", c.sid);
    let (st, _) = req(&c.t, Method::GET, &path, None, &c.alice).await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "no target before a turn names one"
    );
    assert!(c.turn(Chat::owner()).await.is_some());
    let (st, v) = req(&c.t, Method::GET, &path, None, &c.alice).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        v,
        json!({"host": "127.0.0.1", "full_name": "ada/secret", "login": "ada"})
    );
    let (st, _) = req(&c.t, Method::GET, &path, None, &c.t.alex).await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "an admin sees no other person's target"
    );
}
