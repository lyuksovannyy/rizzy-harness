//! Stored skills reach sub-agents through their parent's spec (X.34 step
//! B, review of PR 148 H1): the master puts the list once, on the turn's
//! own spec; each child, started now, continued or in the background, gets
//! that list minus its own `deny_skills`.

#[path = "support/rig.rs"]
mod support;

use harness_agent_native::provider::ChatMsg;
use harness_core::skills::{StoredScope, StoredSkillEntry};
use serde_json::json;
use support::Rig;

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

fn child_of(out: &str) -> harness_core::SessionId {
    out.strip_prefix("session: ")
        .and_then(|r| r.lines().next())
        .and_then(|id| id.parse().ok())
        .unwrap_or_else(|| panic!("no child in {out}"))
}

/// Every user text the agent `id` was sent, joined.
fn seen_by(r: &Rig, id: &str) -> String {
    let me = format!("You are @{id}.");
    r.requests
        .lock()
        .unwrap()
        .iter()
        .filter(|q| matches!(q.messages.first(), Some(ChatMsg::System(s)) if s.contains(&me)))
        .flat_map(|q| {
            q.messages
                .iter()
                .filter_map(|m| m.user_text().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn stored() -> Vec<StoredSkillEntry> {
    ["deploy", "notes"]
        .iter()
        .map(|n| StoredSkillEntry {
            name: n.to_string(),
            description: format!("Stored {n}"),
            scope: StoredScope::Workspace,
        })
        .collect()
}

async fn rig(child_deny: &[&str]) -> Rig {
    let mut r = Rig::new().await;
    r.git(&["init", "-q"]);
    r.write("a.txt", "alpha\n");
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "init"]);
    let mut scout = Rig::spec("scout", &["read", "skill"], vec![]);
    scout.deny_skills = child_deny.iter().map(|s| s.to_string()).collect();
    r.delegates.push(scout);
    r.lead_stored = stored();
    r.lead_caps = Some(harness_core::agent::DelegateCaps {
        max_subagents: 4,
        max_tokens: 200_000,
        background: true,
    });
    r
}

/// A child allowed a skill its parent is denied gets it, and the parent
/// does not list it; continued, the child keeps the list.
#[tokio::test]
async fn a_child_gets_the_parents_list_minus_its_own_denials() {
    let mut r = rig(&[]).await;
    r.lead_deny_skills = vec!["deploy".into()];
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "scout", "task": "read: a.txt"}),
        ]))
        .await;
    let kid = child_of(&outs[0]);
    let scout = seen_by(&r, "scout");
    assert!(scout.contains("deploy: Stored deploy"), "{scout}");
    assert!(scout.contains("notes: Stored notes"), "{scout}");
    // A follow-up to that child: still the list.
    let before = r.requests.lock().unwrap().len();
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "scout", "task": "read: a.txt", "session": kid.to_string()}),
        ]))
        .await;
    assert!(outs[0].starts_with(&format!("session: {kid}")), "{outs:?}");
    assert!(r.requests.lock().unwrap().len() > before);
}

/// The reverse: a child denied a skill its parent may use does not get it.
#[tokio::test]
async fn a_child_denied_a_skill_does_not_get_it() {
    let mut r = rig(&["notes"]).await;
    r.lead_turn(&delegate(&[
        json!({"agent": "scout", "task": "read: a.txt"}),
    ]))
    .await;
    let scout = seen_by(&r, "scout");
    assert!(scout.contains("deploy: Stored deploy"), "{scout}");
    assert!(!scout.contains("notes: Stored notes"), "{scout}");
}

/// A background child gets the list as a foreground one does.
#[tokio::test]
async fn a_background_child_gets_the_list() {
    let mut r = rig(&["deploy"]).await;
    let (_, outs) = r
        .lead_turn(&delegate(&[
            json!({"agent": "scout", "task": "read: a.txt", "background": true}),
        ]))
        .await;
    assert!(!outs[0].contains("no background"), "{outs:?}");
    while !r.seen.iter().any(|e| e.kind == "background_ended") {
        r.until(|k| matches!(k, harness_core::event::EventKind::BackgroundEnded(_)))
            .await;
    }
    let scout = seen_by(&r, "scout");
    assert!(scout.contains("notes: Stored notes"), "{scout}");
    assert!(!scout.contains("deploy: Stored deploy"), "{scout}");
}
