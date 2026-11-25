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

/// (status, seen, promoted) of a routine run: its chat is hidden by
/// `hide_seen_runs` only when the run is over, seen and not continued.
const RUNS: [(&str, i64, i64); 8] = [
    ("completed", 1, 0),
    ("completed", 0, 0),
    ("running", 1, 0),
    ("running", 0, 0),
    ("waiting", 1, 0),
    ("completed", 1, 1),
    ("failed", 1, 0),
    ("pending", 0, 0),
];

/// Several owners and workspaces, ownerless chats, incognito ones,
/// sub-agent chats nested at several depths (one owned by somebody else
/// than its parent's owner), chats mapped to a workspace their owner is
/// not in, project links, every (status, seen, promoted) case of `RUNS`
/// for both a and b, seen marks, and many equal `updated_ms`.
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
    let mut runnable: HashMap<&str, Vec<SessionId>> = HashMap::new();
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
        // Two in a row: the second is often a child of the first.
        let parent = matches!(k % 11, 4 | 5)
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
        // Every chat may become a parent: sub-agents nest.
        parents.entry(key).or_default().push(sid);
        if let Some(o @ ("a" | "b")) = owner
            && parent.is_none()
            && k % 9 != 2
        {
            runnable.entry(o).or_default().push(sid);
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
        if k % 3 == 0 && k % 2 == 0 {
            // a has looked at some of the finished ones (and at b's).
            s.mark_seen("a", sid, 3).unwrap();
        }
    }
    // Each case of RUNS on its own top-level chat of a and of b.
    for u in ["a", "b"] {
        for (i, (status, seen, promoted)) in RUNS.iter().enumerate() {
            let sid = runnable[u][i * 3];
            exec(
                &s,
                "INSERT INTO routine_runs (run_id, routine_id, trigger_kind, started_ms, status,
                     session_id, seen, promoted, untrusted)
                 VALUES (?1, ?2, 'manual', 0, ?3, ?4, ?5, ?6, ?7)",
                params![
                    format!("run-{u}-{i}"),
                    format!("r-{u}"),
                    status,
                    sid.to_string(),
                    seen,
                    promoted,
                    i % 2 == 0
                ],
            );
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

/// Every chat in `rows` below `parents`, at any depth, newest first: what
/// today's sidebar nests (it sees only `rows`, so a chain stops at a chat
/// not among them).
fn old_descendants(rows: &[SessionRow], parents: &[SessionId]) -> Vec<SessionRow> {
    let mut found: HashSet<String> = parents.iter().map(ToString::to_string).collect();
    let mut taken: HashSet<SessionId> = HashSet::new();
    let mut out = Vec::new();
    loop {
        let before = out.len();
        for r in rows {
            if r.parent_id.as_ref().is_some_and(|p| found.contains(p)) && taken.insert(r.session_id)
            {
                found.insert(r.session_id.to_string());
                out.push(r.clone());
            }
        }
        if out.len() == before {
            break;
        }
    }
    out.sort_by_key(|r| std::cmp::Reverse(key(r)));
    out
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
#[expect(
    clippy::disallowed_methods,
    reason = "counts statements in a test; nothing is logged"
)]
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
    // index), at every level of the recursion, not by walking the owner's
    // chats; the owner is a filter.
    for (owner, bound) in [(Owner::User("a"), Some("a")), (Owner::Everyone, None)] {
        let p = plan(
            &f.s,
            &children_sql(owner),
            params![bound, "[]", 10i64, 11i64],
        );
        let by_parent = p
            .iter()
            .filter(|l| l.starts_with("SEARCH s USING INDEX sessions_parent (parent_id=?)"))
            .count();
        assert_eq!(by_parent, 2, "the seed and the step: {owner:?}: {p:#?}");
        assert!(!scans(&p, "s"), "{owner:?}: {p:#?}");
        assert!(
            !p.iter().any(|l| l.contains("sessions_owner")),
            "{owner:?}: {p:#?}"
        );
        for sql in [unread_sql(owner), links_sql(owner), marks_sql(owner)] {
            let p = if sql.contains("?3") {
                plan(&f.s, &sql, params![bound, "a", "[]"])
            } else {
                plan(&f.s, &sql, params![bound, "[]"])
            };
            for alias in ["s", "sp", "r"] {
                assert!(!scans(&p, alias), "{sql}: {p:#?}");
            }
            // Driven by the ids, not by the owner's chats (the owner
            // index walks all of them: 16 ms at 10,000, measured).
            assert!(
                !p.iter().any(|l| l.contains("sessions_owner")),
                "{sql}: {p:#?}"
            );
        }
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
    let a = Owner::User("a");
    let (n, r) = statements(s, || s.unread_of(a, "a", &ids500));
    assert_eq!(n, 1);
    assert!(!r.unwrap().is_empty());
    let (n, r) = statements(s, || s.project_links_of(a, &ids500));
    assert_eq!(n, 1);
    assert!(!r.unwrap().is_empty());
    let (n, r) = statements(s, || s.routine_marks_of(a, &ids500));
    assert_eq!(n, 1);
    assert!(!r.unwrap().is_empty());
    let (n, r) = statements(s, || s.children_of(a, &ids500));
    assert_eq!(n, 1);
    assert!(!r.unwrap().rows.is_empty());
    let (n, r) = statements(s, || s.children_of(Owner::Everyone, &ids500));
    assert_eq!(n, 1);
    assert!(!r.unwrap().rows.is_empty());
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
            s.unread_of(a, "a", &[]).unwrap(),
            s.project_links_of(a, &[]).unwrap(),
            s.routine_marks_of(a, &[]).unwrap(),
            s.children_of(a, &[]).unwrap().rows,
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
                s.unread_of(owner, viewer, &all).unwrap(),
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
        let mut links = s.project_links_of(owner, &all).unwrap();
        old_links.sort_by_key(|l| l.session_id.to_string());
        links.sort_by_key(|l| l.session_id.to_string());
        assert_eq!(links, old_links, "{owner:?}");
        assert_eq!(
            s.routine_marks_of(owner, &all).unwrap(),
            s.routine_marks(&all).unwrap(),
            "{owner:?}"
        );
        // Today's sidebar nests every listed descendant of a listed
        // parent; the parents of a page are top-level chats.
        let tops: Vec<SessionId> = rows
            .iter()
            .filter(|r| r.parent_id.is_none())
            .map(|r| r.session_id)
            .collect();
        let want = old_descendants(&rows, &tops);
        let got = s.children_of(owner, &tops).unwrap();
        assert!(!got.truncated);
        assert_eq!(got.rows, want, "{owner:?}");
        if owner != Owner::User("c") {
            assert!(
                want.iter().any(|r| r
                    .parent_id
                    .as_deref()
                    .is_some_and(|p| !tops.iter().any(|t| t.to_string() == p))),
                "{owner:?}: the fixture nests deeper than one level"
            );
        }
    }
    // The fixture exercises every branch.
    assert!(checked > 1_000, "{checked}");
    let marks = s.routine_marks(&ids(&s.sessions().unwrap())).unwrap();
    assert!(marks.values().any(|m| !listed(m)) && marks.values().any(listed));
    assert!(
        !s.unread_of(Owner::Everyone, "a", &ids(&s.sessions().unwrap()))
            .unwrap()
            .is_empty()
    );
    for u in ["a", "b"] {
        let theirs = ids(&s.sessions_of(Some(u)).unwrap());
        let marks = s.routine_marks_of(Owner::User(u), &theirs).unwrap();
        for (status, seen, promoted) in RUNS {
            assert!(
                marks.values().any(|m| m.status == status
                    && m.seen == (seen != 0)
                    && m.promoted == (promoted != 0)),
                "{u}: ({status}, {seen}, {promoted}) is in the fixture"
            );
        }
    }
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
        for r in s.children_of(Owner::User(u), &all).unwrap().rows {
            assert_eq!(r.owner_id.as_deref(), Some(u));
        }
    }
    assert!(rows_seen > 1_000, "{rows_seen}");
    // b's child of a's chat is b's, not a's.
    let a_children = s.children_of(Owner::User("a"), &all).unwrap().rows;
    let b_children = s.children_of(Owner::User("b"), &all).unwrap().rows;
    assert!(b_children.iter().any(|r| r.updated_ms == 5_000));
    assert!(a_children.iter().all(|r| r.updated_ms != 5_000));
}

/// An annotation answers only for the asking owner's chats, whatever ids
/// it is handed (a request, a lookup by id, a search hit).
#[test]
fn annotations_never_answer_for_another_owners_chat() {
    let f = fixture();
    let s = &f.s;
    let theirs = ids(&s.sessions_of(Some("b")).unwrap());
    let b = Owner::User("b");
    assert!(!s.unread_of(b, "b", &theirs).unwrap().is_empty());
    assert!(!s.project_links_of(b, &theirs).unwrap().is_empty());
    assert!(!s.routine_marks_of(b, &theirs).unwrap().is_empty());
    assert!(!s.children_of(b, &theirs).unwrap().rows.is_empty());
    let a = Owner::User("a");
    for viewer in OWNERS {
        assert!(
            s.unread_of(a, viewer, &theirs).unwrap().is_empty(),
            "{viewer}"
        );
    }
    assert!(s.project_links_of(a, &theirs).unwrap().is_empty());
    assert!(s.routine_marks_of(a, &theirs).unwrap().is_empty());
    assert!(s.children_of(a, &theirs).unwrap().rows.is_empty());
}

fn child(s: &Store, owner: &str, parent: SessionId, updated_ms: i64) -> SessionId {
    chat(
        s,
        Chat {
            owner: Some(owner),
            updated_ms,
            incognito: false,
            parent: Some(parent),
            finished_seq: 0,
        },
    )
}

fn child_ids(s: &Store, owner: &str, parents: &[SessionId]) -> Vec<SessionId> {
    ids(&s.children_of(Owner::User(owner), parents).unwrap().rows)
}

#[test]
fn children_are_every_descendant_and_stop_at_another_owner() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "a");
    user(&s, "b");
    // Depth 3 below the parent, newest first.
    let p = plain(&s, "a", 1);
    let c = child(&s, "a", p, 2);
    let g = child(&s, "a", c, 3);
    let gg = child(&s, "a", g, 4);
    assert_eq!(child_ids(&s, "a", &[p]), vec![gg, g, c]);
    // b's chat in the middle of a chain of a's: it stops there, and b
    // sees only their own part below a's chat.
    let p2 = plain(&s, "a", 10);
    let x = child(&s, "b", p2, 11);
    let y = child(&s, "a", x, 12);
    assert_eq!(child_ids(&s, "a", &[p2]), Vec::<SessionId>::new());
    assert_eq!(child_ids(&s, "b", &[p2]), vec![x]);
    assert_eq!(child_ids(&s, "a", &[x]), vec![y]);
    // A chain longer than MAX_DEPTH is cut at it.
    let root = plain(&s, "a", 100);
    let mut at = root;
    for k in 0..(MAX_DEPTH + 4) {
        at = child(&s, "a", at, 101 + k as i64);
    }
    assert_eq!(child_ids(&s, "a", &[root]).len(), MAX_DEPTH);
}

/// A corrupt loop of `parent_id`s ends (at MAX_DEPTH) and lists each chat
/// once.
#[test]
fn a_parent_loop_ends() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "a");
    let one = plain(&s, "a", 1);
    let two = child(&s, "a", one, 2);
    exec(
        &s,
        "UPDATE sessions SET parent_id = ?2 WHERE session_id = ?1",
        params![one.to_string(), two.to_string()],
    );
    let me = plain(&s, "a", 3);
    exec(
        &s,
        "UPDATE sessions SET parent_id = ?1 WHERE session_id = ?1",
        params![me.to_string()],
    );
    assert_eq!(child_ids(&s, "a", &[one]), vec![two, one]);
    assert_eq!(child_ids(&s, "a", &[me]), vec![me]);
}

fn plain_running(s: &Store, owner: &str, parent: SessionId, updated_ms: i64) -> SessionId {
    let id = child(s, owner, parent, updated_ms);
    exec(
        s,
        "UPDATE sessions SET running = 1 WHERE session_id = ?1",
        params![id.to_string()],
    );
    id
}

fn descendants(s: &Store, parent: SessionId) -> Descendants {
    s.children_of(Owner::User("a"), &[parent]).unwrap()
}

/// Past the cap the newest are kept, whatever order the walk finds them
/// in, and `truncated` says exactly whether a chat was left out.
#[test]
fn the_cap_keeps_the_newest_and_says_exactly_when_it_cut() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "a");
    // Children inserted oldest first: the walk meets the oldest first.
    let p = plain(&s, "a", 0);
    let made: Vec<SessionId> = (0..MAX_DESCENDANTS + 5)
        .map(|k| child(&s, "a", p, 1 + k as i64))
        .collect();
    let d = descendants(&s, p);
    assert!(d.truncated);
    assert_eq!(d.rows.len(), MAX_DESCENDANTS);
    let mut newest: Vec<SessionId> = made[5..].to_vec();
    newest.reverse();
    assert_eq!(ids(&d.rows), newest, "the 2,000 newest, newest first");

    // A running grandchild, the newest of all, behind 2,002 old children
    // of the first child's siblings: the walk finds it last.
    let q = plain(&s, "a", 0);
    let first = child(&s, "a", q, 1);
    for k in 0..(MAX_DESCENDANTS + 2) {
        child(&s, "a", q, 2 + k as i64);
    }
    let running = plain_running(&s, "a", first, 1_000_000);
    let d = descendants(&s, q);
    assert!(d.truncated);
    assert_eq!(d.rows[0].session_id, running);
    assert!(d.rows[0].running);

    // Exactly the cap: nothing cut; one more: cut.
    for (n, cut) in [(MAX_DESCENDANTS, false), (MAX_DESCENDANTS + 1, true)] {
        let r = plain(&s, "a", 0);
        for k in 0..n {
            child(&s, "a", r, 1 + k as i64);
        }
        let d = descendants(&s, r);
        assert_eq!(d.truncated, cut, "{n}");
        assert_eq!(d.rows.len(), n.min(MAX_DESCENDANTS), "{n}");
    }
}

/// A corrupt `parent_id` loop counts each chat once: it neither cuts the
/// answer early nor makes `truncated` say a chat was left out.
#[test]
fn a_parent_loop_does_not_make_truncated_lie() {
    for (others, cut) in [(MAX_DESCENDANTS - 2, false), (MAX_DESCENDANTS - 1, true)] {
        let s = Store::open_in_memory().unwrap();
        user(&s, "a");
        let one = plain(&s, "a", 1);
        let two = child(&s, "a", one, 2);
        exec(
            &s,
            "UPDATE sessions SET parent_id = ?2 WHERE session_id = ?1",
            params![one.to_string(), two.to_string()],
        );
        for k in 0..others {
            child(&s, "a", one, 10 + k as i64);
        }
        // Reachable from `one`: `two`, `one` itself and the others.
        let d = descendants(&s, one);
        assert_eq!(d.truncated, cut, "{others}");
        assert_eq!(d.rows.len(), (others + 2).min(MAX_DESCENDANTS), "{others}");
        let distinct: HashSet<SessionId> = ids(&d.rows).into_iter().collect();
        assert_eq!(distinct.len(), d.rows.len(), "each chat once");
    }
}

/// Decided: a child is shown nested under its parent, so it follows the
/// parent's workspace whatever its own mapping says (today's list would
/// drop it from the parent's workspace).
#[test]
fn a_child_follows_its_parent_whatever_its_own_workspace() {
    let f = fixture();
    let s = &f.s;
    let p = plain(s, "a", 9_000);
    exec(
        s,
        "INSERT INTO session_workspaces (session_id, workspace_id) VALUES (?1, ?2)",
        params![p.to_string(), f.team],
    );
    let c = child(s, "a", p, 9_001);
    let (page, _) = s
        .sessions_page(&PageQuery {
            owner: Owner::User("a"),
            workspace: Some(&f.team),
            top_level: true,
            hide_seen_runs: false,
            after: None,
            limit: 1,
        })
        .unwrap();
    assert_eq!(ids(&page), vec![p]);
    let kids = s.children_of(Owner::User("a"), &[p]).unwrap().rows;
    assert_eq!(ids(&kids), vec![c]);
    assert_eq!(
        kids[0].workspace_id.as_deref(),
        Some(f.personal["a"].as_str())
    );
    let old = old_list(
        s,
        &PageQuery {
            owner: Owner::User("a"),
            workspace: Some(&f.team),
            top_level: false,
            hide_seen_runs: false,
            after: None,
            limit: 1,
        },
    );
    assert!(!ids(&old).contains(&c), "today's list leaves it out");
}

/// Insert a chat of `owner` whose stored id is not in canonical form.
fn odd(s: &Store, owner: Option<&str>, updated_ms: i64) -> String {
    let id = SessionId::new().to_string().to_uppercase();
    exec(
        s,
        "INSERT INTO sessions (session_id, node_id, title, created_ms, updated_ms, head_seq, owner_id)
         VALUES (?1, 'node-1', 'secret title', 0, ?2, 0, ?3)",
        params![id, updated_ms, owner],
    );
    id
}

/// Every page from the top, at most `max_pages`.
fn walk_bounded(s: &Store, q: PageQuery<'_>, max_pages: usize) -> Vec<SessionId> {
    let mut q = q;
    let mut out = Vec::new();
    for _ in 0..max_pages {
        let (page, next) = s.sessions_page(&q).unwrap();
        assert!(page.len() <= q.limit.clamp(1, MAX_PAGE));
        out.extend(ids(&page));
        match next {
            Some(c) => {
                assert_eq!(Some(c), page.last().map(Cursor::of), "from a good row");
                q.after = Some(c);
            }
            None => return out,
        }
    }
    panic!("paging did not end in {max_pages} pages");
}

fn by_limit(owner: Owner<'_>, limit: usize) -> PageQuery<'_> {
    PageQuery {
        owner,
        workspace: None,
        top_level: false,
        hide_seen_runs: false,
        after: None,
        limit,
    }
}

/// Decided: a chat whose stored id is not in canonical form is skipped
/// (logged with its id), never returned and never a cursor; the list goes
/// on and paging always ends.
#[test]
fn a_stored_id_in_another_spelling_is_skipped_and_the_list_goes_on() {
    // The reviewer's probe: three good chats and a newer odd one.
    let s = Store::open_in_memory().unwrap();
    user(&s, "a");
    let good: Vec<SessionId> = (1..=3).map(|k| plain(&s, "a", k)).collect();
    odd(&s, Some("a"), 10);
    let mut want = good.clone();
    want.reverse();
    assert_eq!(walk_bounded(&s, by_limit(Owner::User("a"), 50), 3), want);
    assert_eq!(walk_bounded(&s, by_limit(Owner::User("a"), 1), 10), want);

    // Only odd ones, tied: an empty list and no cursor.
    let s = Store::open_in_memory().unwrap();
    user(&s, "a");
    for _ in 0..6 {
        odd(&s, Some("a"), 7);
    }
    let (page, next) = s.sessions_page(&by_limit(Owner::User("a"), 2)).unwrap();
    assert!(page.is_empty());
    assert_eq!(next, None);

    // Mixed: a whole page of odd ones, and odd ones tied with good ones.
    let s = Store::open_in_memory().unwrap();
    user(&s, "a");
    user(&s, "b");
    let mut good = Vec::new();
    for t in 0..12i64 {
        for k in 0..3 {
            if (t + k) % 3 == 0 || t % 4 == 1 {
                odd(&s, Some(if k == 1 { "b" } else { "a" }), t);
            } else {
                good.push((t, plain(&s, if k == 1 { "b" } else { "a" }, t)));
            }
        }
    }
    for t in 20..26 {
        odd(&s, Some("a"), t);
    }
    odd(&s, None, 5);
    let expect = |owner: Option<&str>| -> Vec<SessionId> {
        let mut v: Vec<(i64, String, SessionId)> = good
            .iter()
            .map(|(t, id)| (*t, id.to_string(), *id))
            .filter(|(_, text, _)| {
                owner.is_none_or(|o| {
                    s.session(text.parse().unwrap())
                        .unwrap()
                        .unwrap()
                        .owner_id
                        .as_deref()
                        == Some(o)
                })
            })
            .collect();
        v.sort_by(|x, y| (y.0, &y.1).cmp(&(x.0, &x.1)));
        v.into_iter().map(|x| x.2).collect()
    };
    for (owner, bound) in [
        (Owner::User("a"), Some("a")),
        (Owner::User("b"), Some("b")),
        (Owner::Everyone, None),
    ] {
        let got = walk_bounded(&s, by_limit(owner, 2), 100);
        let once: HashSet<SessionId> = got.iter().copied().collect();
        assert_eq!(once.len(), got.len(), "{owner:?}: each good row once");
        assert_eq!(got, expect(bound), "{owner:?}");
    }
}

/// The canonical-id predicate accepts exactly `SessionId::to_string()`.
#[test]
fn the_canonical_id_predicate_accepts_only_the_canonical_spelling() {
    let c = rusqlite::Connection::open_in_memory().unwrap();
    let sql = format!("SELECT {}", canonical_id("?1"));
    let ok = |v: rusqlite::types::Value| -> bool {
        c.query_row(&sql, [v], |r| r.get::<_, Option<bool>>(0))
            .unwrap()
            .unwrap_or(false)
    };
    use rusqlite::types::Value::{Blob, Integer, Null, Text};
    for _ in 0..1_000 {
        assert!(ok(Text(SessionId::new().to_string())));
    }
    let id = SessionId::new();
    let text = id.to_string();
    for bad in [
        Text(text.to_uppercase()),
        Text(format!("{{{text}}}")),
        Text(text.replace('-', "")),
        Text(format!("urn:uuid:{text}")),
        Text(text[..35].to_string()),
        Text(format!("{text}0")),
        Text(format!("{text}\n")),
        Text(format!(" {text}")),
        Text(text.replacen('-', "g", 1)),
        Text(String::new()),
        Null,
        Integer(1),
        Blob(text.clone().into_bytes()),
        Blob(id.0.as_bytes().to_vec()),
    ] {
        assert!(!ok(bad.clone()), "{bad:?}");
    }
}

/// Odd-id rows are filtered in SQL: 5,000 of them between two good stretches
/// cost nothing extra, the page is one statement and both pages are right.
#[test]
fn many_odd_ids_cost_one_statement() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "a");
    let oldest = plain(&s, "a", 1);
    for k in 0..5_000 {
        odd(&s, Some("a"), 10 + k);
    }
    let newest: Vec<SessionId> = (0..50).map(|k| plain(&s, "a", 10_000 + k)).collect();
    let q = by_limit(Owner::User("a"), 50);
    let (n, r) = statements(&s, || s.sessions_page(&q));
    let (p1, next) = r.unwrap();
    assert_eq!(n, 1);
    let mut want = newest.clone();
    want.reverse();
    assert_eq!(ids(&p1), want);
    let (n, r) = statements(&s, || s.sessions_page(&PageQuery { after: next, ..q }));
    let (p2, next) = r.unwrap();
    assert_eq!(n, 1);
    assert_eq!(ids(&p2), vec![oldest]);
    assert_eq!(next, None);
}

/// A NULL or BLOB id (a TEXT PRIMARY KEY on a rowid table takes both)
/// does not fail the list.
#[test]
fn a_null_or_blob_id_does_not_fail_the_list() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "a");
    let good = plain(&s, "a", 1);
    exec(
        &s,
        "INSERT INTO sessions (session_id, node_id, created_ms, updated_ms, head_seq, owner_id)
         VALUES (NULL, 'n', 0, 5, 0, 'a'), (x'00ff', 'n', 0, 6, 0, 'a'), (?1, 'n', 0, 7, 0, 'a')",
        params![SessionId::new().0.as_bytes().to_vec()],
    );
    for owner in [Owner::User("a"), Owner::Everyone] {
        assert_eq!(
            walk_bounded(&s, by_limit(owner, 1), 5),
            vec![good],
            "{owner:?}"
        );
    }
}

/// A child with an odd id is not returned and its subtree is not walked;
/// the good child is, with no error.
#[test]
fn an_odd_child_is_left_out_with_its_subtree() {
    let s = Store::open_in_memory().unwrap();
    user(&s, "a");
    let p = plain(&s, "a", 1);
    let good = child(&s, "a", p, 2);
    let upper = SessionId::new().to_string().to_uppercase();
    for (id, t) in [("garbage".to_string(), 3), (upper.clone(), 4)] {
        exec(
            &s,
            "INSERT INTO sessions (session_id, node_id, created_ms, updated_ms, head_seq, owner_id, parent_id)
             VALUES (?1, 'n', 0, ?2, 0, 'a', ?3)",
            params![id, t, p.to_string()],
        );
    }
    // Below the odd children: never reached.
    for (parent, t) in [("garbage".to_string(), 5), (upper, 6)] {
        exec(
            &s,
            "INSERT INTO sessions (session_id, node_id, created_ms, updated_ms, head_seq, owner_id, parent_id)
             VALUES (?1, 'n', 0, ?2, 0, 'a', ?3)",
            params![SessionId::new().to_string(), t, parent],
        );
    }
    // One level down, in the recursive step: an odd grandchild under the
    // good child, and a good chat below it, never reached either.
    let odd_grand = SessionId::new().to_string().to_uppercase();
    exec(
        &s,
        "INSERT INTO sessions (session_id, node_id, created_ms, updated_ms, head_seq, owner_id, parent_id)
         VALUES (?1, 'n', 0, 7, 0, 'a', ?2)",
        params![odd_grand, good.to_string()],
    );
    exec(
        &s,
        "INSERT INTO sessions (session_id, node_id, created_ms, updated_ms, head_seq, owner_id, parent_id)
         VALUES (?1, 'n', 0, 8, 0, 'a', ?2)",
        params![SessionId::new().to_string(), odd_grand],
    );
    let d = s.children_of(Owner::User("a"), &[p]).unwrap();
    assert_eq!(ids(&d.rows), vec![good]);
    assert!(!d.truncated);
}

/// A last page that is exactly full has no next cursor.
#[test]
fn an_exact_multiple_has_no_next_page() {
    for n in [50, 100] {
        let s = Store::open_in_memory().unwrap();
        user(&s, "a");
        for k in 0..n {
            plain(&s, "a", k);
        }
        let q = PageQuery {
            owner: Owner::User("a"),
            workspace: None,
            top_level: false,
            hide_seen_runs: false,
            after: None,
            limit: 50,
        };
        let (p1, next) = s.sessions_page(&q).unwrap();
        assert_eq!(p1.len(), 50);
        if n == 50 {
            assert_eq!(next, None, "{n}");
            continue;
        }
        let (p2, next) = s.sessions_page(&PageQuery { after: next, ..q }).unwrap();
        assert_eq!(p2.len(), 50);
        assert_eq!(next, None, "{n}");
    }
}

/// `hide_seen_runs` hides a run's chat only when the run is over, seen
/// and not continued; with it off every one is listed.
#[test]
fn hide_seen_runs_hides_only_runs_over_seen_and_not_continued() {
    let s = Store::open_in_memory().unwrap();
    let ws = user(&s, "a");
    exec(
        &s,
        "INSERT INTO routines (routine_id, owner_id, workspace_id, name, spec, created_ms, updated_ms)
         VALUES ('r', 'a', ?1, 'r', '{}', 0, 0)",
        params![ws],
    );
    let plain_chat = plain(&s, "a", 0);
    let mut hidden = Vec::new();
    let mut shown = vec![plain_chat];
    for (i, (status, seen, promoted)) in RUNS.iter().enumerate() {
        let sid = plain(&s, "a", 1 + i as i64);
        exec(
            &s,
            "INSERT INTO routine_runs (run_id, routine_id, trigger_kind, started_ms, status, session_id, seen, promoted)
             VALUES (?1, 'r', 'manual', 0, ?2, ?3, ?4, ?5)",
            params![format!("run-{i}"), status, sid.to_string(), seen, promoted],
        );
        let over = !matches!(*status, "pending" | "running" | "waiting");
        if over && *seen != 0 && *promoted == 0 {
            hidden.push(sid);
        } else {
            shown.push(sid);
        }
    }
    assert_eq!(hidden.len(), 2, "completed and failed, seen");
    for hide in [false, true] {
        let (page, _) = s
            .sessions_page(&PageQuery {
                owner: Owner::User("a"),
                workspace: None,
                top_level: true,
                hide_seen_runs: hide,
                after: None,
                limit: 50,
            })
            .unwrap();
        let got: HashSet<SessionId> = ids(&page).into_iter().collect();
        for id in &shown {
            assert!(got.contains(id), "hide {hide}: shown");
        }
        for id in &hidden {
            assert_eq!(got.contains(id), !hide, "hide {hide}");
        }
    }
}
