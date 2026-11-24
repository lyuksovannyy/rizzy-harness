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
use harness_protocol::{
    CAP_FORGE_PR, CAP_FORGE_PUSH, CAP_PROJECTS, MasterToNode, NodeToMaster, PromptMsg,
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use super::forge_clone_http::{
    T, TOKEN_A, assert_no_token_anywhere, clone_body, connect, fake, member, ready, req, start,
};

pub(crate) struct Chat {
    t: T,
    alice: String,
    alice_id: String,
    sid: SessionId,
    /// The fake node that announces `forge-push` and runs the chat.
    node: super::forge_clone_http::Node,
}

/// Alice's connection, a project cloned with it on a node that pushes, and
/// an agentic chat of hers in that project.
pub(crate) async fn chat() -> Chat {
    let t = start().await;
    let (l, node) = fake(
        "pusher",
        &[
            CAP_PROJECTS,
            "forge-clone:600",
            CAP_FORGE_PUSH,
            CAP_FORGE_PR,
        ],
    );
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

// ---------------------------------------------- second review: N1

impl Chat {
    async fn started(&self, call: &str, tool: &str) {
        let input = if tool == "bash" {
            json!({"command": "ls"})
        } else {
            json!({})
        };
        self.emit(
            "tool_call_started",
            json!({"turn_id": "t1", "call_id": call, "tool": tool, "input": input}),
        )
        .await;
    }

    async fn requested(&self, approval: &str, call: &str, tool: &str) {
        self.emit(
            "approval_requested",
            json!({"approval_id": approval, "call_id": call, "tool": tool,
                   "input": {}, "reason": "r"}),
        )
        .await;
    }

    /// The owner allows `approval` and the node asks: whether a token came.
    async fn allowed_gets_token(&self, approval: &str) -> bool {
        assert_eq!(
            self.click(&self.alice, approval, "allow").await,
            StatusCode::OK
        );
        self.ask(approval).await.1.is_some()
    }
}

/// A chat with a turn running and a target named.
async fn running() -> Chat {
    let c = chat().await;
    assert!(c.turn(Chat::owner()).await.is_some());
    c.card("t1", "push", "warm").await;
    c
}

/// N1: one approval id on two calls, in both orders: the page's first card
/// may be the `bash ls` one; no token whatever was clicked.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_reused_approval_id_gets_nothing_in_either_order() {
    let c = running().await;
    c.started("cb", "bash").await;
    c.requested("r1", "cb", "bash").await;
    c.started("cp", "push").await;
    c.requested("r1", "cp", "push").await;
    assert!(!c.allowed_gets_token("r1").await, "bash first, then push");
    let c = running().await;
    c.started("cp", "push").await;
    c.requested("r2", "cp", "push").await;
    c.started("cb", "bash").await;
    c.requested("r2", "cb", "bash").await;
    assert!(!c.allowed_gets_token("r2").await, "push first, then bash");
}

/// N1: a call announced twice (retitled), in both orders.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_retitled_call_gets_nothing_in_either_order() {
    let c = running().await;
    c.started("c1", "bash").await;
    c.requested("r1", "c1", "push").await;
    c.started("c1", "push").await;
    assert!(!c.allowed_gets_token("r1").await, "bash, then push");
    let c = running().await;
    c.started("c2", "push").await;
    c.requested("r2", "c2", "push").await;
    c.started("c2", "bash").await;
    assert!(!c.allowed_gets_token("r2").await, "push, then bash");
}

/// N1: three requests with one id on one push call.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_requests_with_one_id_get_nothing() {
    let c = running().await;
    c.started("cp", "push").await;
    for _ in 0..3 {
        c.requested("r1", "cp", "push").await;
    }
    assert!(!c.allowed_gets_token("r1").await);
}

/// N1: the duplicate arrives after the click, before the request: refused
/// when the credential is asked for.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_duplicate_after_the_click_is_refused_at_the_request() {
    let c = running().await;
    c.started("cp", "push").await;
    c.requested("r1", "cp", "push").await;
    assert_eq!(c.click(&c.alice, "r1", "allow").await, StatusCode::OK);
    assert!(
        c.t.master.push_click_pending(c.sid, "r1"),
        "the click was taken"
    );
    c.started("cb", "bash").await;
    c.requested("r1", "cb", "bash").await;
    let (_, token, error) = c.ask("r1").await;
    assert!(token.is_none(), "{error:?}");
}

// ---------------------------------------------- re-check: N4

/// N4: a card with an id outside the node's id rule beside a real push
/// card: the page sends that id encoded, the master answers 404 and
/// records nothing for the push card; ids outside the rule are 404.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ids_outside_the_rule_are_404_and_release_nothing() {
    let c = running().await;
    c.started("cp", "push").await;
    c.requested("u1", "cp", "push").await;
    c.started("cb", "bash").await;
    c.requested("u1#x", "cb", "bash").await;
    for path_id in [
        "u1%23x",
        "u1%3Fx",
        "x%2F..%2Fu1",
        "u1%252Fx",
        "a%20b",
        &"a".repeat(500),
    ] {
        assert_eq!(
            c.click(&c.alice, path_id, "allow").await,
            StatusCode::NOT_FOUND,
            "{path_id}"
        );
    }
    assert!(
        !c.t.master.push_click_pending(c.sid, "u1"),
        "a bash card's click armed the push card"
    );
    let (_, token, _) = c.ask("u1").await;
    assert!(token.is_none());
}

/// N4: a push card whose call id is outside the rule releases nothing,
/// whoever clicks it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_push_card_with_a_call_id_outside_the_rule_gets_nothing() {
    let c = running().await;
    c.started("c#1", "push").await;
    c.requested("v1", "c#1", "push").await;
    assert!(!c.allowed_gets_token("v1").await);
}

// ------------------------------------------------ pull requests (T102)

/// What the master answered a `forge.pull_request`.
#[derive(Debug)]
struct PrAnswer {
    number: Option<u64>,
    url: Option<String>,
    existed: bool,
    error: Option<String>,
}

impl Chat {
    /// A `pull_request` card: the call with `input`, the approval naming it
    /// (its own `input` field `shown`, which the page does not draw).
    async fn pr_card(&self, approval: &str, call: &str, input: Value, shown: Value) {
        if self.open_turn() != Some("t1".to_string()) {
            self.card("t1", "push", "warm").await;
        }
        self.emit(
            "tool_call_started",
            json!({"turn_id": "t1", "call_id": call, "tool": "pull_request", "input": input}),
        )
        .await;
        self.emit(
            "approval_requested",
            json!({"approval_id": approval, "call_id": call, "tool": "pull_request",
                   "input": shown, "reason": "open a pull request"}),
        )
        .await;
    }

    async fn open_pr(&self, approval: &str) -> PrAnswer {
        let req_id = harness_core::new_id();
        self.node
            .tx
            .send(NodeToMaster::PullRequest {
                req_id: req_id.clone(),
                session_id: self.sid,
                approval_id: approval.into(),
            })
            .await
            .unwrap();
        for _ in 0..1500 {
            let found = self.node.seen.lock().unwrap().iter().find_map(|m| match m {
                MasterToNode::PullRequestResult {
                    req_id: r,
                    number,
                    url,
                    existed,
                    error,
                } if *r == req_id => Some(PrAnswer {
                    number: *number,
                    url: url.clone(),
                    existed: *existed,
                    error: error.clone(),
                }),
                _ => None,
            });
            if let Some(f) = found {
                return f;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no answer to the pull request");
    }

    /// Card, Allow by the owner, the node's request.
    async fn pr(&self, approval: &str, input: Value) -> PrAnswer {
        let call = format!("call{approval}");
        self.pr_card(approval, &call, input.clone(), input).await;
        assert_eq!(
            self.click(&self.alice, approval, "allow").await,
            StatusCode::OK
        );
        self.open_pr(approval).await
    }
}

fn honest() -> Value {
    json!({"head": "feat", "base": "main", "title": "Add x", "body": "What changed.\n\n- a", "draft": true})
}

/// The pull request is made from the input of the card's call as the
/// master's mirror holds it, not from the approval's own `input` (which a
/// lying node can set apart from what the page draws), with the owner's
/// token, and answered with its number and address.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pull_request_is_made_from_the_calls_input_in_the_mirror() {
    let c = running().await;
    let evil = json!({"head": "feat", "base": "main", "title": "EVIL", "body": "EVIL"});
    c.pr_card("p1", "cp1", honest(), evil).await;
    assert_eq!(c.click(&c.alice, "p1", "allow").await, StatusCode::OK);
    let a = c.open_pr("p1").await;
    assert_eq!(a.number, Some(7), "{a:?}");
    assert!(a.url.unwrap().ends_with("/ada/secret/pull/7"));
    let sent = c.t.forge.pulls.lock().unwrap().clone();
    assert_eq!(
        sent,
        vec![json!({"title": "Add x", "head": "feat", "base": "main",
                    "body": "What changed.\n\n- a", "draft": true})]
    );
    // Once: the same card again opens nothing more.
    assert!(c.open_pr("p1").await.error.is_some());
    assert_eq!(c.t.forge.pulls.lock().unwrap().len(), 1);
    let audit =
        c.t.master
            .inner_mirror()
            .audit_lines("forge.pull_request")
            .unwrap();
    assert_eq!(audit.len(), 1);
    assert!(!audit[0].1.contains(TOKEN_A));
    assert_no_token_anywhere(&c.t);
}

/// No base: the forge's default branch, asked after the Allow.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_base_is_the_forges_default_branch() {
    let c = running().await;
    let a = c.pr("p1", json!({"head": "feat", "title": "t"})).await;
    assert_eq!(a.number, Some(7), "{a:?}");
    assert_eq!(c.t.forge.pulls.lock().unwrap()[0]["base"], "main");
}

/// The click binding of push holds for pull requests: a reused id, a
/// retitled call, an id outside the rule, a push card's click, no click.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pull_request_needs_its_own_unique_card_and_click() {
    let c = running().await;
    // Reused approval id: a bash card and a pull request card share it.
    c.started("cb", "bash").await;
    c.requested("r1", "cb", "bash").await;
    c.pr_card("r1", "cp", honest(), honest()).await;
    assert_eq!(c.click(&c.alice, "r1", "allow").await, StatusCode::OK);
    assert!(c.open_pr("r1").await.error.is_some());
    // A retitled call.
    c.started("cr", "bash").await;
    c.pr_card("r2", "cr", honest(), honest()).await;
    assert_eq!(c.click(&c.alice, "r2", "allow").await, StatusCode::OK);
    assert!(c.open_pr("r2").await.error.is_some());
    // A push card's Allow is no pull request's.
    c.started("cpu", "push").await;
    c.requested("r3", "cpu", "push").await;
    assert_eq!(c.click(&c.alice, "r3", "allow").await, StatusCode::OK);
    assert!(c.open_pr("r3").await.error.is_some());
    // No click at all.
    c.pr_card("r4", "c4", honest(), honest()).await;
    assert!(c.open_pr("r4").await.error.is_some());
    // An id outside the rule.
    c.pr_card("r#5", "c5", honest(), honest()).await;
    assert_eq!(
        c.click(&c.alice, "r%235", "allow").await,
        StatusCode::NOT_FOUND
    );
    assert!(c.open_pr("r#5").await.error.is_some());
    assert!(c.t.forge.pulls.lock().unwrap().is_empty());
}

/// Input the rule refuses never reaches the forge.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_input_never_reaches_the_forge() {
    let c = running().await;
    let long_body = "b".repeat(100 * 1024);
    let long_title = "t".repeat(300);
    for (i, bad) in [
        json!({"head": "main", "base": "main", "title": "t"}),
        json!({"head": "a..b", "title": "t"}),
        json!({"head": "a~1", "title": "t"}),
        json!({"head": "a b", "title": "t"}),
        json!({"head": "-a", "title": "t"}),
        json!({"head": "a", "title": long_title}),
        json!({"head": "a", "title": "two\nlines"}),
        json!({"head": "a", "title": "t", "body": long_body}),
        json!({"head": "a", "title": "t", "body": "bell\u{7}"}),
        json!({"head": "a", "title": "t", "repo": "other/x"}),
    ]
    .into_iter()
    .enumerate()
    {
        let a = c.pr(&format!("b{i}"), bad).await;
        assert!(a.error.is_some(), "{i}: {a:?}");
    }
    assert!(c.t.forge.pulls.lock().unwrap().is_empty());
}

/// The forge's refusals: already open (said so), a hostile error text
/// (escapes, bidi, a token echo: cleaned and cut), a 5 MB answer, a slow
/// answer and a redirect: errors, and nothing secret anywhere.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_forges_answers_are_bounded_and_clean() {
    let c = running().await;
    let mut input = honest();
    input["title"] = json!("exists");
    let a = c.pr("e1", input.clone()).await;
    assert!(a.existed && a.error.is_none(), "{a:?}");
    input["title"] = json!("hostile");
    let a = c.pr("e2", input.clone()).await;
    let e = a.error.unwrap();
    assert!(
        !e.contains(TOKEN_A) && !e.contains('\u{1b}') && !e.contains('\u{202e}'),
        "{e}"
    );
    assert!(
        e.contains("No commits between") && e.chars().count() <= 400,
        "{e}"
    );
    for (t, want) in [
        ("big", "too large"),
        ("redirect", "HTTP 307"),
        ("slow", "in time"),
    ] {
        input["title"] = json!(t);
        let a = c.pr(&format!("e-{t}"), input.clone()).await;
        let e = a.error.unwrap_or_default();
        assert!(e.contains(want), "{t}: {e}");
    }
    assert_no_token_anywhere(&c.t);
}

/// Five per chat in any hour; the sixth is refused before the forge.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_sixth_in_an_hour_is_refused() {
    let c = running().await;
    for i in 0..=harness_master::forge_pr::PER_HOUR {
        let a = c.pr(&format!("h{i}"), honest()).await;
        if i < harness_master::forge_pr::PER_HOUR {
            assert!(a.error.is_none(), "{i}: {a:?}");
        } else {
            assert!(a.error.unwrap().contains("last hour"));
        }
    }
    assert_eq!(
        c.t.forge.pulls.lock().unwrap().len(),
        harness_master::forge_pr::PER_HOUR
    );
}

/// A turn the API token sent names no target, so nothing is offered.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_api_token_turn_offers_no_pull_request() {
    let c = chat().await;
    assert!(c.turn(PromptOpts::default()).await.is_none());
}
