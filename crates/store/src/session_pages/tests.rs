//! The page and its annotations against today's whole-list functions on a
//! mixed fixture, the query plans, statement counts, and the owner term.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use harness_core::SessionId;
use rusqlite::params;
use rusqlite::trace::{TraceEvent, TraceEventCodes};

use super::*;
use crate::{NewUser, Role};

fn user(s: &Store, id: &str) -> String {
    s.create_user(&NewUser {
        user_id: id.into(),
        username: id.into(),
        password_hash: "x".into(),
        role: Role::Member,
        exec_ceiling: harness_core::policy::ExecLevel::None,
        must_change_password: false,
    })
    .unwrap();
    s.personal_workspace(id).unwrap()
}

struct Chat<'a> {
    owner: Option<&'a str>,
    updated_ms: i64,
    incognito: bool,
    parent: Option<SessionId>,
    finished_seq: i64,
}

fn chat(s: &Store, c: Chat<'_>) -> SessionId {
    let sid = SessionId::new();
    s.lock()
        .execute(
            "INSERT INTO sessions (session_id, node_id, title, created_ms, updated_ms, head_seq,
                 owner_id, incognito, parent_id, finished_seq)
             VALUES (?1, 'node-1', ?1, 0, ?2, 3, ?3, ?4, ?5, ?6)",
            params![
                sid.to_string(),
                c.updated_ms,
                c.owner,
                c.incognito,
                c.parent.map(|p| p.to_string()),
                c.finished_seq
            ],
        )
        .unwrap();
    sid
}

fn plain(s: &Store, owner: &str, updated_ms: i64) -> SessionId {
    chat(
        s,
        Chat {
            owner: Some(owner),
            updated_ms,
            incognito: false,
            parent: None,
            finished_seq: 0,
        },
    )
}

fn exec(s: &Store, sql: &str, p: impl rusqlite::Params) {
    s.lock().execute(sql, p).unwrap();
}

/// Workspaces and owners the mixed fixture is built from.
struct Fix {
    s: Store,
    /// Personal workspaces of a, b and c.
    personal: HashMap<&'static str, String>,
    /// a owns it, b is a member; c is not.
    team: String,
    /// c's workspace; a is not a member.
    other: String,
}

const OWNERS: [&str; 3] = ["a", "b", "c"];

/// Several owners and workspaces, ownerless chats, incognito ones,
/// sub-agent children (one owned by somebody else than its parent's
/// owner), chats mapped to a workspace their owner is not in, project
/// links, routine runs in every state that matters, seen marks, and many
/// equal `updated_ms`.
fn fixture() -> Fix {
    let s = Store::open_in_memory().unwrap();
    let mut personal = HashMap::new();
    for u in OWNERS {
        personal.insert(u, user(&s, u));
    }
    let team = s.create_workspace("Team", "a").unwrap().workspace_id;
    s.add_member(&team, "b", "member").unwrap();
    let other = s.create_workspace("Other", "c").unwrap().workspace_id;
    for (p, ws) in [
        ("pa", &personal["a"]),
        ("pb", &personal["b"]),
        ("pt", &team),
        ("po", &other),
    ] {
        exec(
            &s,
            "INSERT INTO projects (project_id, workspace_id, name, icon, accent, created_ms, updated_ms)
             VALUES (?1, ?2, 'p', 'i', 'a', 0, 0)",
            params![p, ws],
        );
    }
    for u in ["a", "b"] {
        exec(
            &s,
            "INSERT INTO routines (routine_id, owner_id, workspace_id, name, spec, created_ms, updated_ms)
             VALUES (?1, ?2, ?3, ?1, '{}', 0, 0)",
            params![format!("r-{u}"), u, personal[u]],
        );
    }
    // (status, seen, promoted): hidden only when over, seen and not continued.
    let runs = [
        ("completed", 1, 0),
        ("completed", 0, 0),
        ("running", 1, 0),
        ("waiting", 1, 0),
        ("completed", 1, 1),
        ("failed", 1, 0),
        ("pending", 0, 0),
    ];
    let mut parents: HashMap<&str, Vec<SessionId>> = HashMap::new();
    for k in 0..240usize {
        let owner = match k % 7 {
            0..=2 => Some("a"),
            3 | 4 => Some("b"),
            5 => Some("c"),
            _ => None,
        };
        let key = owner.unwrap_or("-");
        // Many ties: eight chats share each time.
        let updated_ms = 1_000 + (k / 8) as i64;
        let parent = (k % 11 == 4)
            .then(|| parents.get(key).and_then(|v| v.last().copied()))
            .flatten();
        let sid = chat(
            &s,
            Chat {
                owner,
                updated_ms,
                incognito: k % 9 == 2,
                parent,
                finished_seq: if k % 3 == 0 { 3 } else { 0 },
            },
        );
        if parent.is_none() {
            parents.entry(key).or_default().push(sid);
        }
        let id = sid.to_string();
        if k % 5 == 1 {
            exec(
                &s,
                "INSERT INTO session_workspaces (session_id, workspace_id) VALUES (?1, ?2)",
                params![id, team],
            );
        } else if k % 13 == 0 {
            // a and b are not members of c's workspace: their chats fall
            // back to Personal.
            exec(
                &s,
                "INSERT INTO session_workspaces (session_id, workspace_id) VALUES (?1, ?2)",
                params![id, other],
            );
        }
        if k % 4 == 0 {
            let p = ["pa", "pb", "pt", "po"][k / 4 % 4];
            exec(
                &s,
                "INSERT INTO session_projects (session_id, project_id, added_ms) VALUES (?1, ?2, 0)",
                params![id, p],
            );
        }
        if k % 6 == 0 && matches!(owner, Some("a" | "b")) {
            let (status, seen, promoted) = runs[k / 6 % runs.len()];
            exec(
                &s,
                "INSERT INTO routine_runs (run_id, routine_id, trigger_kind, started_ms, status,
                     session_id, seen, promoted, untrusted)
                 VALUES (?1, ?2, 'manual', 0, ?3, ?4, ?5, ?6, ?7)",
                params![
                    format!("run-{k}"),
                    format!("r-{}", owner.unwrap()),
                    status,
                    id,
                    seen,
                    promoted,
                    k % 12 == 0
                ],
            );
        }
        if k % 3 == 0 && k % 2 == 0 {
            // a has looked at some of the finished ones (and at b's).
            s.mark_seen("a", sid, 3).unwrap();
        }
    }
    // A child of a's chat owned by b.
    chat(
        &s,
        Chat {
            owner: Some("b"),
            updated_ms: 5_000,
            incognito: false,
            parent: Some(parents["a"][0]),
            finished_seq: 0,
        },
    );
    Fix {
        s,
        personal,
        team,
        other,
    }
}

fn key(r: &SessionRow) -> (i64, String) {
    (r.updated_ms, r.session_id.to_string())
}

/// The master's `routines::listed`: a run chat stays listed while going,
/// while unseen, and once continued.
fn listed(m: &RoutineMark) -> bool {
    !m.seen || m.promoted || matches!(m.status.as_str(), "pending" | "running" | "waiting")
}

/// Today's path: the whole list, filtered and sorted in Rust.
fn old_list(s: &Store, q: &PageQuery<'_>) -> Vec<SessionRow> {
    let mut rows = s.sessions_of(q.owner.bind()).unwrap();
    rows.retain(|r| q.workspace.is_none() || r.workspace_id.as_deref() == q.workspace);
    if q.top_level {
        rows.retain(|r| r.parent_id.is_none() && !r.incognito);
    }
    if q.hide_seen_runs {
        let ids: Vec<SessionId> = rows.iter().map(|r| r.session_id).collect();
        let marks = s.routine_marks(&ids).unwrap();
        rows.retain(|r| marks.get(&r.session_id).is_none_or(listed));
    }
    rows.sort_by_key(|r| std::cmp::Reverse(key(r)));
    rows
}

/// Every page from the top, checking each page's size and cursor.
fn walk(s: &Store, q: &PageQuery<'_>) -> Vec<SessionRow> {
    let limit = q.limit.clamp(1, MAX_PAGE);
    let mut q = *q;
    q.after = None;
    let mut out = Vec::new();
    loop {
        let (page, next) = s.sessions_page(&q).unwrap();
        out.extend(page.iter().cloned());
        match next {
            Some(c) => {
                assert_eq!(page.len(), limit, "a page with a next is full");
                assert_eq!(Some(c), page.last().map(Cursor::of));
                q.after = Some(c);
            }
            None => {
                assert!(page.len() <= limit);
                return out;
            }
        }
    }
}

fn ids(rows: &[SessionRow]) -> Vec<SessionId> {
    rows.iter().map(|r| r.session_id).collect()
}

/// Statements `f` runs on `s`'s connection.
fn statements<R>(s: &Store, f: impl FnOnce() -> R) -> (usize, R) {
    let n = Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    s.lock()
        .trace_v2(
            TraceEventCodes::SQLITE_TRACE_STMT,
            Some(move |e: TraceEvent<'_>| {
                // Trigger bodies are reported as "-- ..." lines.
                if let TraceEvent::Stmt(_, sql) = e
                    && !sql.starts_with("--")
                {
                    seen.fetch_add(1, Ordering::SeqCst);
                }
            }),
        )
        .unwrap();
    let r = f();
    s.lock()
        .trace_v2(TraceEventCodes::empty(), None::<fn(TraceEvent<'_>)>)
        .unwrap();
    (n.load(Ordering::SeqCst), r)
}

fn plan(s: &Store, sql: &str, p: impl rusqlite::Params) -> Vec<String> {
    let c = s.lock();
    let mut st = c.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    st.query_map(p, |r| r.get::<_, String>(3))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

fn scans(plan: &[String], alias: &str) -> bool {
    plan.iter()
        .any(|l| l == &format!("SCAN {alias}") || l.starts_with(&format!("SCAN {alias} ")))
}

#[test]
fn cursor_round_trips_and_is_parsed_strictly() {
    for ms in [0, 1_732_500_000_000, -1, i64::MIN, i64::MAX] {
        let c = Cursor {
            updated_ms: ms,
            session_id: SessionId::new(),
        };
        let text = c.encode();
        assert_eq!(text.len(), 34, "{text}");
        assert!(text.len() <= CURSOR_MAX_LEN);
        assert_eq!(Cursor::parse(&text), Some(c));
    }
    let good = Cursor {
        updated_ms: 7,
        session_id: SessionId::new(),
    };
    let text = good.encode();
    let bytes = |b: &[u8]| URL_SAFE_NO_PAD.encode(b);
    let mut v2 = URL_SAFE_NO_PAD.decode(&text).unwrap();
    v2[0] = 2;
    let refused = [
        String::new(),
        "x".into(),
        format!("{text}="),
        format!("{text}A"),
        text[..33].to_string(),
        "A".repeat(CURSOR_MAX_LEN + 1),
        bytes(&v2),
        bytes(&[1u8; 24]),
        bytes(&[1u8; 26]),
        text.replace(['-', '_'], "+"),
        format!(" {text}"),
        "1:1732500000000:0194f5c0-0000-7000-8000-000000000000".into(),
    ];
    for bad in &refused {
        if bad == &text {
            continue;
        }
        assert_eq!(Cursor::parse(bad), None, "{bad:?}");
    }
    // The last of 34 characters carries 4 unused bits: only zeros are canonical.
    let last = text.chars().last().unwrap();
    let pos = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
        .find(last)
        .unwrap();
    let twin = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
        .chars()
        .nth(pos ^ 1)
        .unwrap();
    assert_eq!(
        Cursor::parse(&format!("{}{twin}", &text[..33])),
        None,
        "a second spelling of the same cursor"
    );
}

#[test]
fn the_owner_page_seeks_the_owner_index_and_never_scans_sessions() {
    let f = fixture();
    for after in [false, true] {
        let sql = page_sql(Owner::User("a"), after);
        for ws in [None, Some(f.team.as_str())] {
            for flags in [(false, false), (true, true)] {
                let p = plan(
                    &f.s,
                    &sql,
                    params![
                        "a",
                        after.then_some(1_010i64),
                        after.then_some("0194f5c0-0000-7000-8000-000000000000"),
                        ws,
                        flags.0,
                        flags.1,
                        51i64
                    ],
                );
                assert!(
                    p.iter()
                        .any(|l| l.starts_with("SEARCH s USING INDEX sessions_owner (owner_id=?")),
                    "{p:#?}"
                );
                if after {
                    assert!(
                        p.iter().any(|l| l.contains("updated_ms<")),
                        "the cursor seeks: {p:#?}"
                    );
                }
                assert!(!scans(&p, "s"), "{p:#?}");
            }
        }
    }
    // The children are found from the parent ids (migration 7's partial
    // index), not by walking the owner's chats; the owner is a filter.
    for (owner, bound) in [(Owner::User("a"), Some("a")), (Owner::Everyone, None)] {
        let p = plan(&f.s, &children_sql(owner), params![bound, "[]"]);
        assert!(
            p.iter()
                .any(|l| l.starts_with("SEARCH s USING INDEX sessions_parent (parent_id=?)")),
            "{owner:?}: {p:#?}"
        );
        assert!(!scans(&p, "s"), "{owner:?}: {p:#?}");
        assert!(
            !p.iter().any(|l| l.contains("sessions_owner")),
            "{owner:?}: {p:#?}"
        );
    }
    for (sql, alias, list) in [(UNREAD_OF, "s", 2), (LINKS_OF, "sp", 1), (MARKS_OF, "r", 1)] {
        let p = if list == 2 {
            plan(&f.s, sql, params!["a", "[]"])
        } else {
            plan(&f.s, sql, params!["[]"])
        };
        assert!(!scans(&p, alias), "{sql}: {p:#?}");
    }
}

#[test]
fn one_hundred_twenty_chats_page_as_fifty_fifty_twenty() {
    // Equal times in runs of seven, and then all of them equal.
    for tie in [7i64, 1_000] {
        let s = Store::open_in_memory().unwrap();
        user(&s, "a");
        user(&s, "b");
        for k in 0..120 {
            plain(&s, "a", 100 + k / tie);
        }
        // Another owner's chats at the same times interleave in the index.
        for k in 0..30 {
            plain(&s, "b", 100 + k / tie);
        }
        let q = PageQuery {
            owner: Owner::User("a"),
            workspace: None,
            top_level: true,
            hide_seen_runs: true,
            after: None,
            limit: 50,
        };
        let (p1, c1) = s.sessions_page(&q).unwrap();
        let (p2, c2) = s.sessions_page(&PageQuery { after: c1, ..q }).unwrap();
        let (p3, c3) = s.sessions_page(&PageQuery { after: c2, ..q }).unwrap();
        assert_eq!((p1.len(), p2.len(), p3.len()), (50, 50, 20));
        assert!(c1.is_some() && c2.is_some());
        assert_eq!(c3, None);
        let all: Vec<SessionId> = [p1, p2, p3].concat().iter().map(|r| r.session_id).collect();
        let unique: HashSet<SessionId> = all.iter().copied().collect();
        assert_eq!(unique.len(), 120, "none twice");
        let mut want: Vec<(i64, String)> =
            s.sessions_of(Some("a")).unwrap().iter().map(key).collect();
        want.sort_by(|x, y| y.cmp(x));
        let got: Vec<(i64, String)> = all
            .iter()
            .map(|id| key(&s.session(*id).unwrap().unwrap()))
            .collect();
        assert_eq!(got, want, "newest first, ties by id, none lost");
    }
}

/// A chat whose time changes between two page reads: moved down past the
/// cursor it is listed again; moved up past it, it is passed over in this
/// walk (the next read from the top lists it). Every other chat is listed
/// exactly once either way.
#[test]
fn a_chat_that_moves_between_reads_never_hides_another() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "a");
    let made: Vec<SessionId> = (0..30).map(|k| plain(&s, "a", 100 + k / 3)).collect();
    let q = PageQuery {
        owner: Owner::User("a"),
        workspace: None,
        top_level: false,
        hide_seen_runs: false,
        after: None,
        limit: 10,
    };
    let (p1, c1) = s.sessions_page(&q).unwrap();
    // One already listed goes back in time (a node clock behind), one not
    // yet listed jumps to the top (a new turn).
    let down = p1[3].session_id;
    let (rest, _) = s.sessions_page(&PageQuery { after: c1, ..q }).unwrap();
    let up = rest[4].session_id;
    exec(
        &s,
        "UPDATE sessions SET updated_ms = 50 WHERE session_id = ?1",
        params![down.to_string()],
    );
    exec(
        &s,
        "UPDATE sessions SET updated_ms = 999 WHERE session_id = ?1",
        params![up.to_string()],
    );
    let mut seen: Vec<SessionId> = ids(&p1);
    let mut after = c1;
    while let Some(c) = after {
        let (p, next) = s
            .sessions_page(&PageQuery {
                after: Some(c),
                ..q
            })
            .unwrap();
        seen.extend(ids(&p));
        after = next;
    }
    let count = |id: SessionId| seen.iter().filter(|x| **x == id).count();
    assert_eq!(count(down), 2, "moved down past the cursor: listed again");
    assert_eq!(count(up), 0, "moved up past the cursor: passed over");
    for id in made.iter().filter(|id| **id != down && **id != up) {
        assert_eq!(count(*id), 1, "{id} unchanged, listed once");
    }
    let (top, _) = s.sessions_page(&q).unwrap();
    assert_eq!(top[0].session_id, up, "the next read from the top lists it");
}

#[test]
fn the_limit_is_clamped_inside_the_store() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "a");
    for k in 0..230 {
        plain(&s, "a", k);
    }
    let q = |limit| PageQuery {
        owner: Owner::User("a"),
        workspace: None,
        top_level: false,
        hide_seen_runs: false,
        after: None,
        limit,
    };
    for (limit, want) in [(0, 1), (1, 1), (200, 200), (201, 200), (usize::MAX, 200)] {
        let (p, next) = s.sessions_page(&q(limit)).unwrap();
        assert_eq!(p.len(), want, "limit {limit}");
        assert!(next.is_some());
    }
}

#[test]
fn each_annotation_is_one_statement_and_an_empty_list_none() {
    let f = fixture();
    let s = &f.s;
    let mut ids500: Vec<SessionId> = ids(&s.sessions().unwrap());
    ids500.truncate(240);
    ids500.extend((ids500.len()..500).map(|_| SessionId::new()));
    assert_eq!(ids500.len(), 500);
    let (n, r) = statements(s, || s.unread_of("a", &ids500));
    assert_eq!(n, 1);
    assert!(!r.unwrap().is_empty());
    let (n, r) = statements(s, || s.project_links_of(&ids500));
    assert_eq!(n, 1);
    assert!(!r.unwrap().is_empty());
    let (n, r) = statements(s, || s.routine_marks_of(&ids500));
    assert_eq!(n, 1);
    assert!(!r.unwrap().is_empty());
    let (n, r) = statements(s, || s.children_of(Owner::User("a"), &ids500));
    assert_eq!(n, 1);
    assert!(!r.unwrap().is_empty());
    let (n, r) = statements(s, || s.children_of(Owner::Everyone, &ids500));
    assert_eq!(n, 1);
    assert!(!r.unwrap().is_empty());
    let (n, r) = statements(s, || {
        s.sessions_page(&PageQuery {
            owner: Owner::User("a"),
            workspace: Some(&f.team),
            top_level: true,
            hide_seen_runs: true,
            after: None,
            limit: 200,
        })
    });
    assert_eq!(n, 1);
    assert!(!r.unwrap().0.is_empty());
    // The old per-id loop, for contrast: one statement per id.
    let (n, _) = statements(s, || s.routine_marks(&ids500));
    assert_eq!(n, 500);

    let (n, r) = statements(s, || {
        (
            s.unread_of("a", &[]).unwrap(),
            s.project_links_of(&[]).unwrap(),
            s.routine_marks_of(&[]).unwrap(),
            s.children_of(Owner::User("a"), &[]).unwrap(),
        )
    });
    assert_eq!(n, 0, "an empty list makes no query");
    assert!(r.0.is_empty() && r.1.is_empty() && r.2.is_empty() && r.3.is_empty());
}

fn workspaces(f: &Fix) -> Vec<Option<&str>> {
    let mut v: Vec<Option<&str>> = vec![None, Some(&f.team), Some(&f.other), Some("no-such")];
    v.extend(f.personal.values().map(|w| Some(w.as_str())));
    v
}

#[test]
fn the_page_and_its_annotations_equal_todays_path() {
    let f = fixture();
    let s = &f.s;
    let mut checked = 0;
    for owner in [
        Owner::User("a"),
        Owner::User("b"),
        Owner::User("c"),
        Owner::Everyone,
    ] {
        for ws in workspaces(&f) {
            for top_level in [false, true] {
                for hide_seen_runs in [false, true] {
                    for limit in [7, 50] {
                        let q = PageQuery {
                            owner,
                            workspace: ws,
                            top_level,
                            hide_seen_runs,
                            after: None,
                            limit,
                        };
                        let got = walk(s, &q);
                        let want = old_list(s, &q);
                        assert_eq!(got, want, "{q:?}");
                        checked += got.len();
                    }
                }
            }
        }
        let rows = s.sessions_of(owner.bind()).unwrap();
        let all = ids(&rows);
        let set: HashSet<SessionId> = all.iter().copied().collect();
        for viewer in OWNERS {
            let old: HashSet<SessionId> = s
                .unread(owner.bind(), viewer)
                .unwrap()
                .into_iter()
                .filter(|id| set.contains(id))
                .collect();
            assert_eq!(
                s.unread_of(viewer, &all).unwrap(),
                old,
                "{owner:?} {viewer}"
            );
        }
        let mut old_links: Vec<ProjectLink> = s
            .project_links(None)
            .unwrap()
            .into_iter()
            .filter(|l| set.contains(&l.session_id))
            .collect();
        let mut links = s.project_links_of(&all).unwrap();
        old_links.sort_by_key(|l| l.session_id.to_string());
        links.sort_by_key(|l| l.session_id.to_string());
        assert_eq!(links, old_links, "{owner:?}");
        assert_eq!(
            s.routine_marks_of(&all).unwrap(),
            s.routine_marks(&all).unwrap(),
            "{owner:?}"
        );
        let mut old_children: Vec<SessionRow> = rows
            .iter()
            .filter(|r| {
                r.parent_id
                    .as_deref()
                    .is_some_and(|p| all.iter().any(|id| id.to_string() == p))
            })
            .cloned()
            .collect();
        old_children.sort_by_key(|r| std::cmp::Reverse(key(r)));
        assert_eq!(
            s.children_of(owner, &all).unwrap(),
            old_children,
            "{owner:?}"
        );
    }
    // The fixture exercises every branch.
    assert!(checked > 1_000, "{checked}");
    let marks = s.routine_marks(&ids(&s.sessions().unwrap())).unwrap();
    assert!(marks.values().any(|m| !listed(m)) && marks.values().any(listed));
    assert!(
        !s.unread_of("a", &ids(&s.sessions().unwrap()))
            .unwrap()
            .is_empty()
    );
}

/// For every combination of parameters, and cursors taken from anybody's
/// rows, a page for one owner holds only that owner's chats; so do the
/// children of anybody's chats.
#[test]
fn an_owner_never_receives_another_owners_chat() {
    let f = fixture();
    let s = &f.s;
    let everyone = s.sessions().unwrap();
    let cursors: Vec<Option<Cursor>> = std::iter::once(None)
        .chain(everyone.iter().step_by(17).map(|r| Some(Cursor::of(r))))
        .chain([Some(Cursor {
            updated_ms: i64::MAX,
            session_id: SessionId::new(),
        })])
        .collect();
    let all = ids(&everyone);
    let mut rows_seen = 0;
    for u in OWNERS {
        for ws in workspaces(&f) {
            for top_level in [false, true] {
                for hide_seen_runs in [false, true] {
                    for limit in [1, 3, 50, usize::MAX] {
                        for after in &cursors {
                            let q = PageQuery {
                                owner: Owner::User(u),
                                workspace: ws,
                                top_level,
                                hide_seen_runs,
                                after: *after,
                                limit,
                            };
                            let (page, _) = s.sessions_page(&q).unwrap();
                            rows_seen += page.len();
                            for r in &page {
                                assert_eq!(r.owner_id.as_deref(), Some(u), "{q:?}");
                            }
                        }
                    }
                }
            }
        }
        for r in s.children_of(Owner::User(u), &all).unwrap() {
            assert_eq!(r.owner_id.as_deref(), Some(u));
        }
    }
    assert!(rows_seen > 1_000, "{rows_seen}");
    // b's child of a's chat is b's, not a's.
    let a_children = s.children_of(Owner::User("a"), &all).unwrap();
    let b_children = s.children_of(Owner::User("b"), &all).unwrap();
    assert!(b_children.iter().any(|r| r.updated_ms == 5_000));
    assert!(a_children.iter().all(|r| r.updated_ms != 5_000));
}
