//! Execution levels (O13, N.10a part 2; docs/execution-modes.md, PLAN.md
//! T39): who may run which execution mode.
//!
//! * An account has a **ceiling**, set by server admins (`users.exec_ceiling`,
//!   `none` for a new account). A server admin's is `host` while they are
//!   one; a disabled account's is `none`.
//! * A member has a **level in each workspace**, set by that workspace's
//!   admins (`workspace_members.exec_level`, `none` for a new member). An
//!   owner's is `host`, so their effective level is their ceiling. A
//!   Personal workspace has no level of its own: there the ceiling applies.
//! * The **effective level** is the lower of the two, and a session's mode
//!   must be at most it, at create and on every later action that starts
//!   work (`workspaces::work_refusal`, checked in `Master::send_command`).
//!   The node's own ceiling still decides what it offers.
//!
//! The levels protect against members, not against a stolen master, which
//! can still ask a node for anything up to the node's ceiling (O13).

use harness_core::policy::{ExecLevel, ExecMode};
use harness_store::{Role, UserRow};

use crate::Master;
use crate::auth::Principal;
use crate::workspaces::Access;

/// An account's ceiling as it counts: `host` for a server admin, `none` for
/// a disabled account, else as stored.
pub fn ceiling_of(u: &UserRow) -> ExecLevel {
    if u.disabled {
        ExecLevel::None
    } else if u.role == Role::Admin {
        ExecLevel::Host
    } else {
        u.exec_ceiling
    }
}

/// A member's level in a workspace as it counts (`Access::exec_level`):
/// `host` in a Personal workspace (it has none of its own, so the ceiling
/// decides) and for an owner; otherwise as stored.
pub fn workspace_level(personal: bool, owner: bool, stored: ExecLevel) -> ExecLevel {
    if personal || owner {
        ExecLevel::Host
    } else {
        stored
    }
}

/// `who`'s effective level in `ws`: the lower of their ceiling and their
/// level there. `ws` is `None` only for a chat without a workspace, which
/// only the API token makes.
pub fn effective(who: &Principal, ws: Option<&Access>) -> ExecLevel {
    who.exec_ceiling
        .min(ws.map_or(ExecLevel::Host, |a| a.exec_level))
}

/// What a level that is too low for `mode` means, after "is".
fn too_low(level: ExecLevel, mode: ExecMode) -> String {
    match level {
        ExecLevel::None => "none, so no agent chats".into(),
        ExecLevel::Container => "container, and no node offers containers yet (they are not \
             built), so no agent chats"
            .into(),
        ExecLevel::Sandbox => format!("sandbox, which does not include the {} mode", mode.id()),
        ExecLevel::Host => "host".into(),
    }
}

/// Why `who` may not have a session in `mode` in `ws`, or `None` when they
/// may. The words say which level stops them and who can change it.
pub fn refusal(who: &Principal, ws: Option<&Access>, mode: ExecMode) -> Option<String> {
    if effective(who, ws).allows(mode) {
        return None;
    }
    if !who.exec_ceiling.allows(mode) {
        return Some(format!(
            "your account's level is {}; a server admin sets it",
            too_low(who.exec_ceiling, mode)
        ));
    }
    let a = ws?;
    let who_sets = match mode {
        ExecMode::Host => "only its owner can grant host",
        ExecMode::Sandbox => "ask one of its admins",
    };
    Some(format!(
        "your level in {} is {}; {who_sets}",
        a.workspace.name,
        too_low(a.exec_level, mode)
    ))
}

/// Why `who` can run no agent chat at all in `ws` (not even `sandbox`, the
/// mode routines always use), or `None`.
pub fn no_work(who: &Principal, ws: Option<&Access>) -> Option<String> {
    refusal(who, ws, ExecMode::Sandbox)
}

/// The mode a new agentic chat of `who` in `ws` asks the node for: the one
/// requested, else the most confined their level allows (never `host` by
/// default). `Err` with the reason when their level does not allow it.
pub fn create_mode(
    who: &Principal,
    ws: Option<&Access>,
    requested: Option<ExecMode>,
) -> Result<ExecMode, String> {
    let mode = requested
        .or_else(|| effective(who, ws).default_mode())
        .unwrap_or(ExecMode::Sandbox);
    refusal(who, ws, mode).map_or(Ok(mode), Err)
}

/// Who changed a level, for its audit line: the user id, or "api token".
pub fn by(who: &Principal) -> String {
    who.user_id.clone().unwrap_or_else(|| "api token".into())
}

/// One audit line for a level that changed without a level request (a
/// role change that makes or unmakes an owner or a server admin, a
/// removal): who, whose, where, from, to and why. No chat content. A
/// failure is logged.
pub async fn audit(m: &Master, kind: &'static str, detail: serde_json::Value) {
    let detail = detail.to_string();
    if let Err(e) = m.inner.mirror.call(move |s| s.audit(kind, &detail)).await {
        tracing::warn!(error = %e, kind, "could not write a level audit line");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_store::WorkspaceRow;

    fn who(ceiling: ExecLevel) -> Principal {
        Principal {
            user_id: Some("u".into()),
            username: "u".into(),
            role: Role::Member,
            exec_ceiling: ceiling,
            must_change_password: false,
        }
    }

    fn team(level: ExecLevel) -> Access {
        Access {
            workspace: WorkspaceRow {
                workspace_id: "w".into(),
                name: "Acme".into(),
                personal_owner: None,
                created_ms: 0,
                settings: "{}".into(),
            },
            personal: false,
            role: Some(crate::workspaces::Role::Member),
            role_id: "member".into(),
            use_memory: false,
            exec_level: level,
        }
    }

    #[test]
    fn the_effective_level_is_the_lower_and_the_refusal_names_it() {
        use ExecLevel::*;
        // The ceiling limits: a server admin sets it.
        let why = refusal(&who(Sandbox), Some(&team(Host)), ExecMode::Host).unwrap();
        assert!(why.starts_with("your account's level is sandbox"), "{why}");
        assert!(why.contains("server admin"), "{why}");
        // The workspace level limits: its admins, or its owner for host.
        let why = refusal(&who(Host), Some(&team(Sandbox)), ExecMode::Host).unwrap();
        assert!(
            why.contains("in Acme is sandbox") && why.contains("owner"),
            "{why}"
        );
        let why = refusal(&who(Host), Some(&team(None)), ExecMode::Sandbox).unwrap();
        assert!(why.contains("in Acme is none, so no agent chats"), "{why}");
        let why = no_work(&who(Host), Some(&team(Container))).unwrap();
        assert!(why.contains("no node offers containers yet"), "{why}");
        assert_eq!(
            refusal(&who(Host), Some(&team(Host)), ExecMode::Host),
            Option::None
        );
        // Never host by default; the default needs the level too.
        assert_eq!(
            create_mode(&who(Host), Some(&team(Host)), Option::None),
            Ok(ExecMode::Sandbox)
        );
        assert!(create_mode(&who(Container), Option::None, Option::None).is_err());
        assert_eq!(effective(&who(Sandbox), Some(&team(Host))), Sandbox);
        assert_eq!(effective(&who(Host), Some(&team(Container))), Container);
        assert!(
            workspace_level(true, false, None) == Host
                && workspace_level(false, true, None) == Host
        );
    }
}
