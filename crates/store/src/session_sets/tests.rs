//! Each set read against the whole-table path it replaces (the master's
//! code before slice 4, rebuilt here from `sessions()`, `sessions_of`,
//! `project_links(None)` and `routine_marks`) on the paged list's mixed
//! fixture; the owner term; and one statement per call whatever the size.

use std::collections::{HashMap, HashSet};

use harness_core::SessionId;
use rusqlite::params;

use crate::session_pages::Owner;
use crate::session_pages::tests::{Fix, exec, fixture, plain, plan, scans, statements, user};
use crate::{ProjectLink, SessionRow, Store};

/// The owners a request can act as: each account, one with no chats, the
/// API token.
const AS: [Owner<'static>; 5] = [
    Owner::User("a"),
    Owner::User("b"),
    Owner::User("c"),
    Owner::User("nobody"),
    Owner::Everyone,
];

/// `Principal::can_see`.
fn can_see(owner: Owner<'_>, chat_owner: Option<&str>) -> bool {
    match owner {
        Owner::Everyone => true,
        Owner::User(me) => chat_owner == Some(me),
    }
}

fn bind(owner: Owner<'_>) -> Option<&str> {
    match owner {
        Owner::User(u) => Some(u),
        Owner::Everyone => None,
    }
}

fn all_ids(s: &Store) -> Vec<SessionId> {
    let mut ids: Vec<SessionId> = s.sessions().unwrap().iter().map(|r| r.session_id).collect();
    // One id that is no chat.
    ids.push(SessionId::new());
    ids
}

fn links(s: &Store) -> HashMap<SessionId, ProjectLink> {
    s.project_links(None)
        .unwrap()
        .into_iter()
        .map(|l| (l.session_id, l))
        .collect()
}

/// The master's `projects::effective`.
fn effective<'a>(links: &'a HashMap<SessionId, ProjectLink>, r: &SessionRow) -> Option<&'a str> {
    let l = links.get(&r.session_id)?;
    (r.workspace_id.as_deref() == Some(l.workspace_id.as_str())).then_some(l.project_id.as_str())
}

fn workspaces(f: &Fix) -> Vec<Option<String>> {
    vec![
        None,
        Some(f.personal["a"].clone()),
        Some(f.personal["b"].clone()),
        Some(f.team.clone()),
        Some(f.other.clone()),
    ]
}

// ---- board_json and both chat_owners: `chat_titles_of` ----

/// Before: `sessions()` into a map, then per card `can_see` → title,
/// else "private", absent → nothing. The memory pages' `chat_owners` asked
/// the same map whether a source chat is the caller's.
#[test]
fn titles_are_what_the_whole_table_map_gave() {
    let f = fixture();
    let s = &f.s;
    let rows: HashMap<SessionId, SessionRow> = s
        .sessions()
        .unwrap()
        .into_iter()
        .map(|r| (r.session_id, r))
        .collect();
    let ids = all_ids(s);
    for owner in AS {
        let old: HashMap<SessionId, Option<String>> = ids
            .iter()
            .filter_map(|id| {
                let r = rows.get(id)?;
                Some((
                    *id,
                    can_see(owner, r.owner_id.as_deref()).then(|| r.title.clone()),
                ))
            })
            .collect();
        assert_eq!(s.chat_titles_of(owner, &ids).unwrap(), old, "{owner:?}");
    }
}

#[test]
fn a_title_is_only_ever_the_owners() {
    let f = fixture();
    let s = &f.s;
    let owners: HashMap<SessionId, Option<String>> = s
        .sessions()
        .unwrap()
        .into_iter()
        .map(|r| (r.session_id, r.owner_id))
        .collect();
    let ids = all_ids(s);
    for me in ["a", "b", "c", "nobody"] {
        let got = s.chat_titles_of(Owner::User(me), &ids).unwrap();
        for (id, title) in &got {
            if title.is_some() {
                assert_eq!(owners[id].as_deref(), Some(me));
            }
        }
        // Every chat that exists is answered (as "private" at least), and
        // nothing that is no chat.
        assert_eq!(got.len(), owners.len());
    }
    assert!(s.chat_titles_of(Owner::User("a"), &[]).unwrap().is_empty());
}

// ---- own_chats: `project_chat_counts` ----

/// Before: `sessions_of(owner)`, top-level and not incognito, counted by
/// effective project over `project_links(None)` (every account's links).
#[test]
fn project_counts_are_what_the_whole_list_counted() {
    let f = fixture();
    let s = &f.s;
    let links = links(s);
    let project_ws: HashMap<String, String> = links
        .values()
        .map(|l| (l.project_id.clone(), l.workspace_id.clone()))
        .collect();
    for owner in AS {
        let mut old: HashMap<String, usize> = HashMap::new();
        for r in s.sessions_of(bind(owner)).unwrap() {
            if !can_see(owner, r.owner_id.as_deref()) || r.incognito || r.parent_id.is_some() {
                continue;
            }
            if let Some(p) = effective(&links, &r) {
                *old.entry(p.to_string()).or_default() += 1;
            }
        }
        for ws in workspaces(&f) {
            let want: HashMap<String, usize> = old
                .iter()
                .filter(|(p, _)| ws.as_ref().is_none_or(|w| &project_ws[*p] == w))
                .map(|(p, n)| (p.clone(), *n))
                .collect();
            let got = s.project_chat_counts(owner, ws.as_deref()).unwrap();
            assert_eq!(got, want, "{owner:?} in {ws:?}");
        }
    }
}

/// A shared project counts each member's own chats only.
#[test]
fn a_shared_project_counts_only_the_callers_chats() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "a");
    user(&s, "b");
    let team = s.create_workspace("Team", "a").unwrap().workspace_id;
    s.add_member(&team, "b", "member").unwrap();
    exec(
        &s,
        "INSERT INTO projects (project_id, workspace_id, name, icon, accent, created_ms, updated_ms)
         VALUES ('pt', ?1, 'p', 'i', 'a', 0, 0)",
        params![team],
    );
    for (owner, n) in [("a", 2), ("b", 5)] {
        for k in 0..n {
            let id = plain(&s, owner, k).to_string();
            exec(
                &s,
                "INSERT INTO session_workspaces (session_id, workspace_id) VALUES (?1, ?2)",
                params![id, team],
            );
            exec(
                &s,
                "INSERT INTO session_projects (session_id, project_id, added_ms) VALUES (?1, 'pt', 0)",
                params![id],
            );
        }
    }
    let count = |o| {
        s.project_chat_counts(o, Some(&team))
            .unwrap()
            .get("pt")
            .copied()
    };
    assert_eq!(count(Owner::User("a")), Some(2));
    assert_eq!(count(Owner::User("b")), Some(5));
    assert_eq!(count(Owner::User("c")), None);
    assert_eq!(count(Owner::Everyone), Some(7));
}

// ---- search, no words: `recent_chats` ----

/// Before: `sessions_of(owner)`, the caller's top-level chats in the
/// workspace, less every chat with a routine mark, skip, take. The old
/// order among equal `updated_ms` was SQLite's choice; the new one breaks
/// ties by id, so the old list is sorted that way to compare.
#[test]
fn recent_chats_are_what_the_whole_list_paged() {
    let f = fixture();
    let s = &f.s;
    for owner in AS {
        for ws in workspaces(&f) {
            let mut old: Vec<SessionRow> = s
                .sessions_of(bind(owner))
                .unwrap()
                .into_iter()
                .filter(|r| {
                    bind(owner).is_none_or(|o| r.owner_id.as_deref() == Some(o))
                        && ws
                            .as_deref()
                            .is_none_or(|w| r.workspace_id.as_deref() == Some(w))
                        && !r.incognito
                        && r.parent_id.is_none()
                })
                .collect();
            let ids: Vec<SessionId> = old.iter().map(|r| r.session_id).collect();
            let runs = s.routine_marks(&ids).unwrap();
            old.retain(|r| !runs.contains_key(&r.session_id));
            old.sort_by_key(|r| std::cmp::Reverse((r.updated_ms, r.session_id.to_string())));
            for (offset, limit) in [(0, 20), (0, 1000), (7, 13), (50, 50), (10_000, 5)] {
                let want: Vec<SessionRow> = old.iter().skip(offset).take(limit).cloned().collect();
                let got = s.recent_chats(owner, ws.as_deref(), offset, limit).unwrap();
                assert_eq!(got, want, "{owner:?} {ws:?} {offset} {limit}");
            }
        }
    }
}

// ---- search with words: `sessions_by_ids` ----

/// Before: `session(id)` per hit; the hits were the caller's already.
#[test]
fn rows_by_id_are_the_single_lookups_of_the_owners_chats() {
    let f = fixture();
    let s = &f.s;
    let ids = all_ids(s);
    for owner in AS {
        let old: HashMap<SessionId, SessionRow> = ids
            .iter()
            .filter_map(|id| s.session(*id).unwrap())
            .filter(|r| can_see(owner, r.owner_id.as_deref()))
            .map(|r| (r.session_id, r))
            .collect();
        assert_eq!(s.sessions_by_ids(owner, &ids).unwrap(), old, "{owner:?}");
    }
}

// ---- members, workspaces, accounts, the sweep: `sessions_owned_by` ----

/// Before: `sessions_of(u)` for each user, kept when owned by `u` (and,
/// for a workspace, in it).
#[test]
fn owned_chats_are_what_the_per_member_loop_found() {
    let f = fixture();
    let s = &f.s;
    let sets: [&[&str]; 5] = [&["a"], &["b"], &["a", "b"], &["a", "b", "c", "ghost"], &[]];
    for users in sets {
        let users: Vec<String> = users.iter().map(|u| u.to_string()).collect();
        for ws in workspaces(&f) {
            let mut old = HashSet::new();
            for u in &users {
                old.extend(
                    s.sessions_of(Some(u))
                        .unwrap()
                        .into_iter()
                        .filter(|r| r.owner_id.as_deref() == Some(u.as_str()))
                        .filter(|r| ws.is_none() || r.workspace_id == ws)
                        .map(|r| r.session_id),
                );
            }
            let got: Vec<SessionId> = s
                .sessions_owned_by(&users, ws.as_deref())
                .unwrap()
                .iter()
                .map(|r| r.session_id)
                .collect();
            assert_eq!(got.len(), old.len(), "no chat twice: {users:?} {ws:?}");
            assert_eq!(
                got.into_iter().collect::<HashSet<_>>(),
                old,
                "{users:?} {ws:?}"
            );
        }
    }
}

// ---- the members roster: `kv_get_many`, `profile_image_shas` ----

#[test]
fn a_rosters_settings_and_pictures_are_the_single_reads() {
    let s = Store::open_in_memory().unwrap();
    let users: Vec<String> = (0..20).map(|i| format!("u{i}")).collect();
    for (i, u) in users.iter().enumerate() {
        user(&s, u);
        if i.is_multiple_of(3) {
            s.kv_set(
                &format!("settings:{u}"),
                &format!("{{\"display_name\":\"{u}\"}}"),
            )
            .unwrap();
        }
        if i.is_multiple_of(4) {
            exec(
                &s,
                "INSERT INTO blobs (blob_id, owner_id, sha256, size, kind, format, mime, display, key, created_ms)
                 VALUES (?1, ?2, ?3, 1, 'image', 'png', 'image/png', 'img', x'00', 0)",
                params![format!("b{i}"), u, format!("{i:064x}")],
            );
            exec(
                &s,
                "INSERT INTO profile_images (user_id, slot, blob_id, updated_ms) VALUES (?1, 'avatar', ?2, 0)",
                params![u, format!("b{i}")],
            );
        }
    }
    let keys: Vec<String> = users.iter().map(|u| format!("settings:{u}")).collect();
    let old: HashMap<String, String> = keys
        .iter()
        .filter_map(|k| Some((k.clone(), s.kv_get(k).unwrap()?)))
        .collect();
    let (n, got) = statements(&s, || s.kv_get_many(&keys).unwrap());
    assert_eq!(got, old);
    assert_eq!(n, 1);
    let old: HashMap<String, String> = users
        .iter()
        .filter_map(|u| Some((u.clone(), s.profile_image(u, "avatar").unwrap()?.sha256)))
        .collect();
    let (n, got) = statements(&s, || s.profile_image_shas(&users, "avatar").unwrap());
    assert_eq!(got, old);
    assert_eq!(n, 1);
    assert!(s.profile_image_shas(&users, "banner").unwrap().is_empty());
}

// ---- one statement each ----

#[test]
fn each_set_read_is_one_statement_whatever_its_size() {
    let f = fixture();
    let s = &f.s;
    let ids = all_ids(s);
    let a = Owner::User("a");
    let users: Vec<String> = ["a", "b", "c"].iter().map(|u| u.to_string()).collect();
    let (n, _) = statements(s, || s.chat_titles_of(a, &ids).unwrap());
    assert_eq!(n, 1, "titles");
    let (n, _) = statements(s, || s.project_chat_counts(a, None).unwrap());
    assert_eq!(n, 1, "counts");
    let (n, _) = statements(s, || s.recent_chats(a, None, 0, 1_000).unwrap());
    assert_eq!(n, 1, "recent");
    let (n, _) = statements(s, || s.sessions_by_ids(a, &ids).unwrap());
    assert_eq!(n, 1, "by id");
    let (n, _) = statements(s, || s.sessions_owned_by(&users, None).unwrap());
    assert_eq!(n, 1, "owned");
    let (n, _) = statements(s, || s.latest_message_texts(&ids).unwrap());
    assert_eq!(n, 1, "latest texts");
}

// ---- query plans ----

/// An account's reads go through an index, never a walk of every chat:
/// by owner (`sessions_owner`) or by id (the primary key).
#[test]
fn an_accounts_set_reads_never_walk_every_chat() {
    let f = fixture();
    let s = &f.s;
    let one = super::super::session_pages::id_list(&[SessionId::new()]).unwrap();
    let a = Owner::User("a");
    let owners = r#"["a","b"]"#;
    let ws: Option<&str> = None;
    for (what, sql, plan) in [
        (
            "titles",
            super::titles_sql(a),
            plan(s, &super::titles_sql(a), params!["a", one]),
        ),
        (
            "counts",
            super::counts_sql(a),
            plan(s, &super::counts_sql(a), params!["a", ws]),
        ),
        (
            "recent",
            super::recent_sql(a),
            plan(s, &super::recent_sql(a), params!["a", ws, 10, 0]),
        ),
        (
            "by id",
            super::by_ids_sql(a),
            plan(s, &super::by_ids_sql(a), params!["a", one]),
        ),
        (
            "owned",
            super::owned_sql(),
            plan(s, &super::owned_sql(), params![owners, ws]),
        ),
    ] {
        assert!(
            !scans(&plan, "s"),
            "{what} walks every chat: {plan:?}\n{sql}"
        );
    }
}

// ---- the project page's "Add a chat": `loose_sessions_page` ----

/// Before: the loaded sidebar rows (`sessions_of`, the workspace's
/// top-level chats, seen routine runs hidden) with no effective project.
/// Now: the same, paged in SQL, so chats past the loaded pages are offered.
#[test]
fn chats_in_no_project_are_what_the_whole_list_filtered() {
    use crate::session_pages::{MAX_PAGE, PageQuery};
    let f = fixture();
    let s = &f.s;
    let links = links(s);
    for owner in AS {
        for ws in workspaces(&f) {
            let mut old: Vec<SessionRow> = s
                .sessions_of(bind(owner))
                .unwrap()
                .into_iter()
                .filter(|r| {
                    ws.as_deref()
                        .is_none_or(|w| r.workspace_id.as_deref() == Some(w))
                        && r.parent_id.is_none()
                        && !r.incognito
                        && effective(&links, r).is_none()
                })
                .collect();
            let ids: Vec<SessionId> = old.iter().map(|r| r.session_id).collect();
            let marks = s.routine_marks(&ids).unwrap();
            old.retain(|r| {
                marks.get(&r.session_id).is_none_or(|m| {
                    !m.seen
                        || m.promoted
                        || matches!(m.status.as_str(), "pending" | "running" | "waiting")
                })
            });
            old.sort_by_key(|r| std::cmp::Reverse((r.updated_ms, r.session_id.to_string())));
            for limit in [7, MAX_PAGE] {
                let mut q = PageQuery {
                    owner,
                    workspace: ws.as_deref(),
                    top_level: true,
                    hide_seen_runs: true,
                    project: None,
                    after: None,
                    limit,
                };
                let mut got = Vec::new();
                loop {
                    let (page, next) = s.loose_sessions_page(&q).unwrap();
                    got.extend(page);
                    match next {
                        Some(c) => q.after = Some(c),
                        None => break,
                    }
                }
                assert_eq!(got, old, "{owner:?} {ws:?} {limit}");
            }
        }
    }
    // It names no project.
    let q = PageQuery {
        owner: Owner::User("a"),
        workspace: None,
        top_level: true,
        hide_seen_runs: true,
        project: Some("pa"),
        after: None,
        limit: 10,
    };
    assert!(s.loose_sessions_page(&q).is_err());
    let (n, _) = statements(s, || {
        s.loose_sessions_page(&PageQuery { project: None, ..q })
    });
    assert_eq!(n, 1);
}
