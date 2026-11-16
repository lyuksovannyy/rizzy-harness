//! A sub-agent and the person's Personalisation (PLAN.md T74): a child
//! inherits its parent turn's standing sections, but the `personal` one
//! only when its own switch is on; a continued child (T68) the same; and
//! the continued child's next request holds its last one byte for byte
//! (T59).

#[path = "support/rig.rs"]
mod support;

use harness_agent_native::provider::ChatMsg;
use harness_core::SessionId;
use harness_core::backend::{MasterContext, StandingKey, StandingPart};
use serde_json::json;
use support::Rig;

const MARK: &str = "PERSONAL-MARK-7f3";

fn delegate(calls: &[serde_json::Value]) -> String {
    format!(
        "script: {}",
        json!(
            calls
                .iter()
                .map(|c| json!({"tool": "delegate", "input": c}))
                .collect::<Vec<_>>()
        )
    )
}

fn child_of(out: &str) -> SessionId {
    out.strip_prefix("session: ")
        .and_then(|r| r.lines().next())
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("no child in {out}"))
}

/// The requests sent for agent `id` (its system prompt names it).
fn requests_of(r: &Rig, id: &str) -> Vec<Vec<ChatMsg>> {
    let me = format!("You are @{id}.");
    r.requests
        .lock()
        .unwrap()
        .iter()
        .filter(|q| matches!(q.messages.first(), Some(ChatMsg::System(s)) if s.contains(&me)))
        .map(|q| q.messages.clone())
        .collect()
}

fn holds_mark(reqs: &[Vec<ChatMsg>]) -> bool {
    reqs.iter()
        .flatten()
        .filter_map(|m| m.user_text())
        .any(|t| t.contains(MARK))
}

#[tokio::test]
async fn a_child_gets_personalisation_only_when_its_own_switch_is_on() {
    let mut r = Rig::new().await;
    r.git(&["init", "-q"]);
    r.write("a.txt", "alpha\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    let mut quiet = Rig::spec("quiet", &["read", "grep"], vec![]);
    quiet.no_personalisation = true;
    r.delegates.push(quiet);
    r.lead_context = Some(MasterContext {
        turn: Some("Context: the user's name is Ann.".into()),
        standing: vec![StandingPart {
            part: StandingKey::Personal,
            text: format!("How the person wants replies to sound: {MARK}"),
        }],
        unattended: false,
    });
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "look", "task": "read: a.txt"}),
            json!({"agent": "quiet", "task": "read: a.txt"}),
        ]))
        .await;
    assert_eq!(outs.len(), 2, "{outs:#?}");
    // The parent and the child whose switch is on have it; the other not.
    assert!(holds_mark(&requests_of(&r, "lead")), "the lead's own turn");
    assert!(holds_mark(&requests_of(&r, "look")), "look's switch is on");
    assert!(
        !holds_mark(&requests_of(&r, "quiet")),
        "quiet's switch is off"
    );
    // The settings line still reaches both.
    for id in ["look", "quiet"] {
        assert!(
            requests_of(&r, id)
                .iter()
                .flatten()
                .filter_map(|m| m.user_text())
                .any(|t| t.contains("the user's name is Ann")),
            "{id}"
        );
    }

    // Continued: quiet still does not get it, and its next request extends
    // its last one byte for byte.
    let quiet_child = child_of(&outs[1]);
    let before = requests_of(&r, "quiet");
    let last = before.last().unwrap().clone();
    let (_, outs) = r
        .lead_turn(&delegate(&[json!({
            "agent": "quiet",
            "session": quiet_child.to_string(),
            "task": "And again?"
        })]))
        .await;
    assert_eq!(child_of(&outs[0]), quiet_child, "{outs:#?}");
    let after = requests_of(&r, "quiet");
    assert!(after.len() > before.len());
    assert!(!holds_mark(&after), "a continued child keeps its switch");
    let next = &after[before.len()];
    assert_eq!(
        next[..last.len()],
        last[..],
        "the follow-up extends the last request"
    );
}
