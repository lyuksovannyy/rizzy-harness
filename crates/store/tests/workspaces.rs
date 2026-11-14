//! Workspaces (master only): Personal workspaces, team membership, the
//! last-owner rule, and which workspace a chat is in.

use std::path::Path;

use harness_core::{RawEvent, SessionId};
use harness_store::{NewUser, Role, Store, StoreError, UserPatch};
use serde_json::{Value, json};

fn user(id: &str, name: &str) -> NewUser {
    NewUser {
        user_id: id.into(),
        username: name.into(),
        password_hash: "$argon2id$x".into(),
        role: Role::Member,
        exec_ceiling: harness_core::policy::ExecLevel::None,
        must_change_password: false,
    }
}

/// A store with the accounts alex (a1), bob (b1) and carol (c1).
fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    for (id, name) in [("a1", "alex"), ("b1", "bob"), ("c1", "carol")] {
        s.create_user(&user(id, name)).unwrap();
    }
    s
}

fn ev(sid: SessionId, seq: u64, kind: &str, payload: Value) -> RawEvent {
    RawEvent {
        session_id: sid,
        seq,
        ts_ms: seq as i64,
        kind: kind.into(),
        payload,
    }
}

fn created(title: &str, node: &str, parent: Option<SessionId>) -> Value {
    let mut p = json!({"title": title, "cwd": "", "model": "m", "backend": "native", "node_id": node, "profile": "agentic", "approval_mode": "ask"});
    if let Some(parent) = parent {
        p["parent_session"] = json!(parent.to_string());
    }
    p
}

/// A chat on `node`, not yet stamped with an owner or workspace.
fn chat_on(s: &Store, node: &str, title: &str, parent: Option<SessionId>) -> SessionId {
    let sid = SessionId::new();
    s.append(
        node,
        &ev(sid, 1, "session_created", created(title, node, parent)),
    )
    .unwrap();
    sid
}

/// A chat created on the master and stamped the way the master does after
/// the create ack.
fn chat(s: &Store, owner: Option<&str>, workspace: Option<&str>) -> SessionId {
    let sid = chat_on(s, "master", "t", None);
    s.claim_session(sid, owner, workspace).unwrap();
    sid
}

/// The effective workspace of `sid`, checking that the single-row query and
/// the list agree.
fn ws_of(s: &Store, sid: SessionId) -> Option<String> {
    let one = s.session(sid).unwrap().unwrap().workspace_id;
    let listed = s
        .sessions()
        .unwrap()
        .into_iter()
        .find(|r| r.session_id == sid)
        .unwrap()
        .workspace_id;
    assert_eq!(one, listed, "session() and sessions() agree");
    one
}

fn disable(s: &Store, user_id: &str) {
    s.update_user(
        user_id,
        &UserPatch {
            disabled: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
}

fn member_ids(s: &Store, ws: &str) -> Vec<String> {
    s.members(ws)
        .unwrap()
        .into_iter()
        .map(|m| m.user_id)
        .collect()
}

/// Change the database behind the store's back, the way an older build or
/// a crash could have left it.
fn raw(path: &Path, sql: &str) {
    let c = rusqlite::Connection::open(path).unwrap();
    c.pragma_update(None, "foreign_keys", "ON").unwrap();
    c.execute_batch(sql).unwrap();
}

#[test]
fn every_account_gets_a_personal_workspace() {
    let s = Store::open_in_memory().unwrap();
    s.create_user(&user("a1", "alex")).unwrap();
    let ms = s.memberships("a1").unwrap();
    assert_eq!(ms.len(), 1);
    let m = &ms[0];
    assert!(m.workspace.personal());
    assert_eq!(m.workspace.personal_owner.as_deref(), Some("a1"));
    assert_eq!(m.workspace.name, "Personal");
    assert_eq!(m.workspace.settings, "{}");
    assert_eq!(
        (m.role_id.as_str(), m.use_memory, m.members),
        ("owner", true, 1)
    );
    assert_eq!(
        s.personal_workspace("a1").unwrap(),
        m.workspace.workspace_id
    );
    assert_eq!(
        s.workspace(&m.workspace.workspace_id).unwrap().as_ref(),
        Some(&m.workspace)
    );

    // A refused account leaves nothing behind.
    let dup = s.create_user(&user("a2", "alex"));
    assert!(matches!(dup, Err(StoreError::Conflict(_))), "{dup:?}");
    assert_eq!(s.workspaces().unwrap().len(), 1);
}

#[test]
fn missing_personal_workspaces_are_created_and_healed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("master.db");
    let s = Store::open(&path, harness_store::Durability::Normal).unwrap();
    s.create_user(&user("a1", "alex")).unwrap();
    s.create_user(&user("b1", "bob")).unwrap();
    s.create_user(&user("c1", "carol")).unwrap();
    let bob_ws = s.personal_workspace("b1").unwrap();
    // alex predates workspaces; bob's owner membership got lost.
    raw(
        &path,
        &format!(
            "DELETE FROM workspaces WHERE personal_owner = 'a1';
             DELETE FROM workspace_members WHERE workspace_id = '{bob_ws}';"
        ),
    );
    assert!(s.memberships("a1").unwrap().is_empty());
    assert!(s.memberships("b1").unwrap().is_empty());

    assert_eq!(s.ensure_personal_workspaces().unwrap(), 2);
    let alex = s.memberships("a1").unwrap();
    assert_eq!(alex.len(), 1);
    assert!(alex[0].workspace.personal() && alex[0].role_id == "owner");
    let bob = s.memberships("b1").unwrap();
    assert_eq!(bob[0].workspace.workspace_id, bob_ws, "kept, not replaced");
    assert_eq!(bob[0].role_id, "owner");
    assert_eq!(s.ensure_personal_workspaces().unwrap(), 0, "idempotent");
    assert_eq!(s.workspaces().unwrap().len(), 3);

    // The one-user version heals the same way.
    raw(
        &path,
        "DELETE FROM workspaces WHERE personal_owner = 'c1';
         DELETE FROM workspace_members WHERE user_id = 'b1';",
    );
    let carol_ws = s.personal_workspace("c1").unwrap();
    assert_eq!(s.personal_workspace("c1").unwrap(), carol_ws);
    assert_eq!(s.memberships("c1").unwrap()[0].role_id, "owner");
    assert_eq!(s.personal_workspace("b1").unwrap(), bob_ws);
    assert_eq!(member_ids(&s, &bob_ws), ["b1"]);
}

#[test]
fn a_chat_is_in_its_mapped_workspace_while_its_owner_is_a_member() {
    let s = store();
    let alex = s.personal_workspace("a1").unwrap();
    let bob = s.personal_workspace("b1").unwrap();
    let team = s.create_workspace("Team", "a1").unwrap().workspace_id;

    let token_chat = chat(&s, None, None);
    assert_eq!(ws_of(&s, token_chat), None, "no owner, no workspace");
    let personal = chat(&s, Some("a1"), None);
    assert_eq!(ws_of(&s, personal).as_deref(), Some(alex.as_str()));
    let mapped = chat(&s, Some("a1"), Some(&team));
    assert_eq!(ws_of(&s, mapped).as_deref(), Some(team.as_str()));

    // An owner-less chat may be mapped; once it gets an owner who is not a
    // member, the mapping no longer counts.
    let adopted = chat(&s, None, Some(&team));
    assert_eq!(ws_of(&s, adopted).as_deref(), Some(team.as_str()));
    s.claim_session(adopted, Some("b1"), None).unwrap();
    assert_eq!(ws_of(&s, adopted).as_deref(), Some(bob.as_str()));
    s.add_member(&team, "b1", "member").unwrap();
    assert_eq!(ws_of(&s, adopted).as_deref(), Some(team.as_str()));
}

#[test]
fn claim_maps_only_members_into_team_workspaces() {
    let s = store();
    let alex = s.personal_workspace("a1").unwrap();
    let bob = s.personal_workspace("b1").unwrap();
    let team = s.create_workspace("Team", "a1").unwrap().workspace_id;

    let sid = chat_on(&s, "master", "t", None);
    assert!(!s.claim_session(sid, Some("b1"), Some(&team)).unwrap());
    let row = s.session(sid).unwrap().unwrap();
    assert_eq!(row.owner_id.as_deref(), Some("b1"), "owner set anyway");
    assert_eq!(row.workspace_id.as_deref(), Some(bob.as_str()));

    let sid = chat_on(&s, "master", "t", None);
    assert!(!s.claim_session(sid, Some("a1"), Some(&alex)).unwrap());
    assert_eq!(ws_of(&s, sid).as_deref(), Some(alex.as_str()));
    assert!(!s.claim_session(sid, Some("a1"), Some("nope")).unwrap());
    assert_eq!(ws_of(&s, sid).as_deref(), Some(alex.as_str()));

    // Without a new owner, the chat's current one must be a member.
    assert!(!s.claim_session(sid, Some("b1"), None).unwrap());
    assert!(!s.claim_session(sid, None, Some(&team)).unwrap());
    assert!(s.claim_session(sid, Some("a1"), Some(&team)).unwrap());
    assert_eq!(ws_of(&s, sid).as_deref(), Some(team.as_str()));
}

#[test]
fn removing_a_member_returns_only_their_chats_to_personal() {
    let s = store();
    let bob = s.personal_workspace("b1").unwrap();
    let team = s.create_workspace("Team", "a1").unwrap().workspace_id;
    assert!(s.add_member(&team, "b1", "member").unwrap());
    let alex_chat = chat(&s, Some("a1"), Some(&team));
    let bob_chat = chat(&s, Some("b1"), Some(&team));

    assert!(s.remove_member(&team, "b1", None).unwrap());
    assert!(
        !s.remove_member(&team, "b1", None).unwrap(),
        "no longer a member"
    );
    assert_eq!(ws_of(&s, alex_chat).as_deref(), Some(team.as_str()));
    assert_eq!(ws_of(&s, bob_chat).as_deref(), Some(bob.as_str()));
    // The mapping is gone, not just ignored: rejoining does not bring it back.
    s.add_member(&team, "b1", "member").unwrap();
    assert_eq!(ws_of(&s, bob_chat).as_deref(), Some(bob.as_str()));
}

#[test]
fn deleting_a_workspace_keeps_its_chats() {
    let s = store();
    let alex = s.personal_workspace("a1").unwrap();
    let team = s.create_workspace("Team", "a1").unwrap().workspace_id;
    s.add_member(&team, "b1", "member").unwrap();
    let sid = chat(&s, Some("a1"), Some(&team));

    assert!(s.delete_workspace(&team).unwrap());
    assert!(!s.delete_workspace(&team).unwrap());
    assert!(s.workspace(&team).unwrap().is_none());
    assert!(s.members(&team).unwrap().is_empty());
    assert_eq!(ws_of(&s, sid).as_deref(), Some(alex.as_str()));
    assert_eq!(s.memberships("b1").unwrap().len(), 1, "Personal only");

    let personal = s.delete_workspace(&alex);
    assert!(
        matches!(personal, Err(StoreError::Refused(_))),
        "{personal:?}"
    );
}

#[test]
fn deleting_a_chat_drops_its_mapping() {
    let s = store();
    let alex = s.personal_workspace("a1").unwrap();
    let team = s.create_workspace("Team", "a1").unwrap().workspace_id;
    let sid = chat(&s, Some("a1"), Some(&team));
    assert!(s.delete_session(sid, false).unwrap());
    // The same id again (no tombstone) starts without a workspace row.
    s.append(
        "master",
        &ev(sid, 1, "session_created", created("t", "master", None)),
    )
    .unwrap();
    s.claim_session(sid, Some("a1"), None).unwrap();
    assert_eq!(ws_of(&s, sid).as_deref(), Some(alex.as_str()));
}

#[test]
fn sub_agents_inherit_the_mapping_from_the_same_node_only() {
    let s = store();
    let team = s.create_workspace("Team", "a1").unwrap().workspace_id;
    let parent = chat_on(&s, "n1", "parent", None);
    assert!(s.claim_session(parent, Some("a1"), Some(&team)).unwrap());

    let child = chat_on(&s, "n1", "child", Some(parent));
    let row = s.session(child).unwrap().unwrap();
    assert_eq!(row.owner_id.as_deref(), Some("a1"));
    assert_eq!(row.workspace_id.as_deref(), Some(team.as_str()));

    // Another node cannot plant a chat in the parent's workspace.
    let planted = chat_on(&s, "n2", "planted", Some(parent));
    let row = s.session(planted).unwrap().unwrap();
    assert_eq!(row.owner_id, None);
    assert_eq!(row.workspace_id, None);
}

#[test]
fn a_team_workspace_keeps_an_enabled_owner() {
    let s = store();
    let team = s.create_workspace("Team", "a1").unwrap().workspace_id;
    s.add_member(&team, "b1", "member").unwrap();

    let refused = |r: harness_store::Result<bool>| match r {
        Err(StoreError::Refused(msg)) => assert!(msg.contains("Team"), "{msg}"),
        other => panic!("expected a refusal, got {other:?}"),
    };
    refused(s.remove_member(&team, "a1", None));
    refused(s.set_member_role(&team, "a1", None, "admin"));
    match s.delete_user("a1") {
        Err(StoreError::Refused(msg)) => {
            assert_eq!(
                msg,
                "alex is the only owner of Team; make another member an owner first"
            )
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(s.user_by_id("a1").unwrap().is_some(), "nothing deleted");
    assert_eq!(member_ids(&s, &team), ["a1", "b1"]);

    // A disabled owner does not count as the other owner...
    assert!(s.set_member_role(&team, "b1", None, "owner").unwrap());
    disable(&s, "b1");
    refused(s.remove_member(&team, "a1", None));
    refused(s.set_member_role(&team, "a1", None, "member"));
    assert!(matches!(s.delete_user("a1"), Err(StoreError::Refused(_))));
    // ...and taking one away is always allowed.
    assert!(s.set_member_role(&team, "b1", None, "admin").unwrap());
    assert!(s.set_member_role(&team, "b1", None, "owner").unwrap());
    assert!(s.remove_member(&team, "b1", None).unwrap());

    // With an enabled co-owner, either may go.
    s.add_member(&team, "c1", "owner").unwrap();
    assert!(s.set_member_role(&team, "a1", None, "member").unwrap());
    refused(s.remove_member(&team, "c1", None));
    assert!(s.remove_member(&team, "a1", None).unwrap());
    assert_eq!(s.owners(&team).unwrap(), ["carol"]);

    // Members of a workspace whose only owner is disabled can still leave.
    s.add_member(&team, "a1", "member").unwrap();
    disable(&s, "c1");
    assert!(s.remove_member(&team, "a1", None).unwrap());

    // Unknown members and workspaces are reported, not refused.
    assert!(!s.set_member_role(&team, "a1", None, "member").unwrap());
    assert!(!s.remove_member("nope", "a1", None).unwrap());
}

/// A change made against the role the caller's guards saw is refused once
/// that role changed in between (an admin cannot demote or remove someone
/// an owner just made an owner), and changes nothing.
#[test]
fn role_changes_expect_the_role_that_was_checked() {
    let s = store();
    let team = s.create_workspace("Team", "a1").unwrap().workspace_id;
    s.add_member(&team, "b1", "member").unwrap();
    s.add_member(&team, "c1", "admin").unwrap();
    let raced = |r: harness_store::Result<bool>| match r {
        Err(StoreError::Refused(msg)) => {
            assert_eq!(msg, "this member's role just changed; reload and try again")
        }
        other => panic!("expected a refusal, got {other:?}"),
    };
    let role = |id: &str| s.member(&team, id).unwrap().unwrap().role_id;

    // Carol (admin) saw Bob as a member; Alex made him an owner meanwhile.
    assert!(
        s.set_member_role(&team, "b1", Some("member"), "owner")
            .unwrap()
    );
    raced(s.set_member_role(&team, "b1", Some("member"), "guest"));
    raced(s.remove_member(&team, "b1", Some("member")));
    assert_eq!(role("b1"), "owner", "nothing changed");
    assert_eq!(member_ids(&s, &team), ["a1", "b1", "c1"]);

    // The role that is really there passes; unknown members are reported.
    assert!(
        s.set_member_role(&team, "c1", Some("admin"), "guest")
            .unwrap()
    );
    assert!(s.remove_member(&team, "c1", Some("guest")).unwrap());
    assert!(
        !s.set_member_role(&team, "c1", Some("guest"), "member")
            .unwrap()
    );
    assert!(!s.remove_member(&team, "c1", Some("guest")).unwrap());
}

#[test]
fn deleting_a_user_cleans_up_their_workspaces() {
    let s = store();
    let alex = s.personal_workspace("a1").unwrap();
    let solo = s.create_workspace("Solo", "a1").unwrap().workspace_id;
    let shared = s.create_workspace("Shared", "b1").unwrap().workspace_id;
    s.add_member(&shared, "a1", "admin").unwrap();
    let alex_chat = chat(&s, Some("a1"), Some(&shared));
    let bob_chat = chat(&s, Some("b1"), Some(&shared));

    s.delete_user("a1").unwrap();
    assert!(
        s.workspace(&solo).unwrap().is_none(),
        "sole member: deleted"
    );
    assert!(s.workspace(&alex).unwrap().is_none(), "Personal cascades");
    assert!(s.memberships("a1").unwrap().is_empty());
    assert_eq!(member_ids(&s, &shared), ["b1"]);
    let mut left: Vec<_> = s
        .workspaces()
        .unwrap()
        .into_iter()
        .map(|w| (w.name, w.personal_owner))
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            ("Personal".to_string(), Some("b1".to_string())),
            ("Personal".to_string(), Some("c1".to_string())),
            ("Shared".to_string(), None),
        ]
    );
    // The chat itself stays (only the API token can still see it).
    assert_eq!(ws_of(&s, alex_chat), None);
    assert_eq!(ws_of(&s, bob_chat).as_deref(), Some(shared.as_str()));
}

#[test]
fn attention_and_search_know_the_workspace() {
    let s = store();
    let alex = s.personal_workspace("a1").unwrap();
    let team = s.create_workspace("Team", "a1").unwrap().workspace_id;
    s.add_member(&team, "b1", "member").unwrap();
    let stamp = |title: &str, owner: &str, ws: Option<&str>| {
        let sid = SessionId::new();
        s.append(
            "master",
            &ev(sid, 1, "session_created", created(title, "master", None)),
        )
        .unwrap();
        s.append("master", &ev(sid, 2, "approval_requested", json!({})))
            .unwrap();
        s.claim_session(sid, Some(owner), ws).unwrap();
        sid
    };
    let in_team = stamp("team plan", "a1", Some(&team));
    let in_personal = stamp("personal plan", "a1", None);
    let bobs = stamp("bob plan", "b1", Some(&team));

    let mut items: Vec<_> = s
        .attention(Some("a1"), "a1")
        .unwrap()
        .into_iter()
        .map(|a| (a.session_id, a.workspace_id))
        .collect();
    items.sort();
    let mut want = vec![
        (in_team, Some(team.clone())),
        (in_personal, Some(alex.clone())),
    ];
    want.sort();
    assert_eq!(items, want, "every workspace, each item says which");

    let found = |owner: Option<&str>, ws: Option<&str>| {
        let mut ids: Vec<_> = s
            .search("\"plan\"", owner, ws, 10)
            .unwrap()
            .into_iter()
            .map(|h| h.session_id)
            .collect();
        ids.sort();
        ids
    };
    let sorted = |mut v: Vec<SessionId>| {
        v.sort();
        v
    };
    assert_eq!(found(Some("a1"), Some(&team)), [in_team]);
    assert_eq!(found(Some("a1"), Some(&alex)), [in_personal]);
    assert_eq!(found(Some("a1"), None), sorted(vec![in_team, in_personal]));
    // The owner filter still applies inside a shared workspace.
    assert_eq!(found(Some("b1"), Some(&team)), [bobs]);
    assert_eq!(found(None, Some(&team)), sorted(vec![in_team, bobs]));
}

#[test]
fn memberships_list_personal_first_then_by_name() {
    let s = store();
    for name in ["beta", "Alpha", "gamma"] {
        s.create_workspace(name, "a1").unwrap();
    }
    let beta = s.memberships("a1").unwrap()[2]
        .workspace
        .workspace_id
        .clone();
    s.add_member(&beta, "b1", "guest").unwrap();

    let ms = s.memberships("a1").unwrap();
    let names: Vec<_> = ms.iter().map(|m| m.workspace.name.as_str()).collect();
    assert_eq!(names, ["Personal", "Alpha", "beta", "gamma"]);
    assert_eq!(ms[2].members, 2);
    assert!(ms.iter().all(|m| m.role_id == "owner"));
    let bob: Vec<_> = s
        .memberships("b1")
        .unwrap()
        .into_iter()
        .map(|m| (m.workspace.name, m.role_id))
        .collect();
    assert_eq!(
        bob,
        [
            ("Personal".to_string(), "owner".to_string()),
            ("beta".to_string(), "guest".to_string())
        ]
    );
    let all: Vec<_> = s
        .workspaces()
        .unwrap()
        .into_iter()
        .map(|w| w.name)
        .collect();
    assert_eq!(
        all,
        ["Alpha", "beta", "gamma", "Personal", "Personal", "Personal"],
        "team workspaces first"
    );
}

#[test]
fn members_and_workspace_fields_can_change() {
    let s = store();
    let alex = s.personal_workspace("a1").unwrap();
    let team = s.create_workspace("Team", "a1").unwrap().workspace_id;
    assert!(s.add_member(&team, "b1", "member").unwrap());
    let again = s.add_member(&team, "b1", "admin");
    assert!(matches!(again, Err(StoreError::Conflict(_))), "{again:?}");
    assert!(!s.add_member(&team, "nobody", "member").unwrap());
    assert!(!s.add_member("nope", "b1", "member").unwrap());
    let personal = s.add_member(&alex, "b1", "member");
    match personal {
        Err(StoreError::Refused(msg)) => assert_eq!(msg, "a Personal workspace has only you"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(matches!(
        s.remove_member(&alex, "a1", None),
        Err(StoreError::Refused(_))
    ));

    let bob = s.member(&team, "b1").unwrap().unwrap();
    assert_eq!(
        (
            bob.username.as_str(),
            bob.role_id.as_str(),
            bob.disabled,
            bob.use_memory
        ),
        ("bob", "member", false, false)
    );
    assert!(s.set_use_memory(&team, "b1", true).unwrap());
    assert!(s.member(&team, "b1").unwrap().unwrap().use_memory);
    assert!(s.set_use_memory(&team, "b1", false).unwrap());
    assert!(!s.member(&team, "b1").unwrap().unwrap().use_memory);
    assert!(!s.memberships("b1").unwrap()[1].use_memory);
    assert!(!s.set_use_memory(&team, "c1", false).unwrap());
    assert!(s.member(&team, "c1").unwrap().is_none());
    assert_eq!(s.member_count(&team).unwrap(), 2);
    assert_eq!(member_ids(&s, &team), ["a1", "b1"]);

    assert!(s.rename_workspace(&team, "Crew").unwrap());
    assert!(
        s.set_workspace_settings(&team, r#"{"default_agent":"coder"}"#)
            .unwrap()
    );
    let w = s.workspace(&team).unwrap().unwrap();
    assert_eq!(w.name, "Crew");
    assert_eq!(w.settings, r#"{"default_agent":"coder"}"#);
    assert!(!w.personal());
    assert!(!s.rename_workspace("nope", "x").unwrap());
}

/// Sharing your facts with a team workspace is your choice, off until you
/// make it: for the creator and for everyone added later. Personal stays
/// on, and rows saved before this default keep their value.
#[test]
fn facts_are_shared_with_a_team_only_when_the_member_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("master.db");
    let s = Store::open(&path, harness_store::Durability::Normal).unwrap();
    for (id, name) in [("a1", "alex"), ("b1", "bob"), ("c1", "carol")] {
        s.create_user(&user(id, name)).unwrap();
    }
    let personal = s.personal_workspace("a1").unwrap();
    assert!(s.member(&personal, "a1").unwrap().unwrap().use_memory);
    let team = s.create_workspace("Team", "a1").unwrap().workspace_id;
    assert!(s.add_member(&team, "b1", "member").unwrap());
    assert!(
        !s.member(&team, "a1").unwrap().unwrap().use_memory,
        "creator"
    );
    assert!(
        !s.member(&team, "b1").unwrap().unwrap().use_memory,
        "member"
    );
    // A healed Personal membership is on too.
    raw(
        &path,
        &format!("DELETE FROM workspace_members WHERE workspace_id = '{personal}';"),
    );
    s.ensure_personal_workspaces().unwrap();
    assert!(s.member(&personal, "a1").unwrap().unwrap().use_memory);
    // A row an earlier build saved as on stays on, also after reopening.
    assert!(s.add_member(&team, "c1", "member").unwrap());
    raw(
        &path,
        &format!(
            "UPDATE workspace_members SET use_memory = 1 WHERE workspace_id = '{team}' AND user_id = 'c1';"
        ),
    );
    drop(s);
    let s = Store::open(&path, harness_store::Durability::Normal).unwrap();
    assert!(s.member(&team, "c1").unwrap().unwrap().use_memory);
    assert!(!s.member(&team, "b1").unwrap().unwrap().use_memory);
}

/// N.10a part 2: a new member starts at `none`, an account at `none`; a
/// level changes only while it is still what the caller's guards read (two
/// admins at once: the second is refused, not silently overwritten), and
/// every change writes its audit line in the same transaction.
#[test]
fn levels_start_at_none_and_change_only_from_what_was_read() {
    use harness_core::policy::ExecLevel;
    let s = store();
    let ws = s.create_workspace("Acme", "a1").unwrap().workspace_id;
    assert!(s.add_member(&ws, "b1", "member").unwrap());
    assert_eq!(s.member(&ws, "b1").unwrap().unwrap().exec_level, ExecLevel::None);
    assert_eq!(s.user_by_id("b1").unwrap().unwrap().exec_ceiling, ExecLevel::None);
    let lines = |s: &Store, k: &str| s.audit_lines(k).unwrap().len();

    // The first admin's change lands; the second, decided on the same
    // reading, is refused and writes no line.
    assert!(
        s.set_member_level(&ws, "b1", "member", ExecLevel::None, ExecLevel::Sandbox, "{\"n\":1}")
            .unwrap()
    );
    let second =
        s.set_member_level(&ws, "b1", "member", ExecLevel::None, ExecLevel::Container, "{\"n\":2}");
    assert!(matches!(second, Err(StoreError::Refused(m)) if m == harness_store::LEVEL_CHANGED));
    assert_eq!(s.member(&ws, "b1").unwrap().unwrap().exec_level, ExecLevel::Sandbox);
    assert_eq!(lines(&s, "level.workspace"), 1);
    // A role changed meanwhile is refused too.
    assert!(s.set_member_role(&ws, "b1", None, "admin").unwrap());
    assert!(matches!(
        s.set_member_level(&ws, "b1", "member", ExecLevel::Sandbox, ExecLevel::None, "{}"),
        Err(StoreError::Refused(_))
    ));
    // Not a member, and a Personal workspace.
    assert!(!s.set_member_level(&ws, "c1", "member", ExecLevel::None, ExecLevel::Host, "{}").unwrap());
    let personal = s.personal_workspace("b1").unwrap();
    assert!(s.set_member_level(&personal, "b1", "owner", ExecLevel::None, ExecLevel::Host, "{}").is_err());
    assert_eq!(lines(&s, "level.workspace"), 1);

    // The account ceiling, the same way.
    assert!(s.set_exec_ceiling("b1", ExecLevel::None, ExecLevel::Host, "{}").unwrap());
    assert!(matches!(
        s.set_exec_ceiling("b1", ExecLevel::None, ExecLevel::Sandbox, "{}"),
        Err(StoreError::Refused(_))
    ));
    assert!(!s.set_exec_ceiling("nobody", ExecLevel::None, ExecLevel::Host, "{}").unwrap());
    assert_eq!(s.user_by_id("b1").unwrap().unwrap().exec_ceiling, ExecLevel::Host);
    assert_eq!(lines(&s, "level.account"), 1);
}
