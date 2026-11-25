//! FIX_PLAN 4.5's benchmark: 50,000 chats across 5 users; one user's first
//! page takes under 20 ms and does not block a concurrent append.
//!
//! Too slow for every CI run (about 20 s to build the database in release,
//! far longer in a debug build); run by hand, in release, with the
//! database somewhere disposable:
//!
//! ```text
//! BENCH_DB=/tmp/…/bench.db cargo test --release -p harness-store --lib \
//!     session_pages::bench -- --ignored --nocapture
//! ```
//!
//! An existing `BENCH_DB` is reused as it is. Numbers from the stage-0 run
//! are in `docs/store-readers.md` §2.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use harness_core::{RawEvent, SessionId};
use rusqlite::params;
use serde_json::json;

use super::{Owner, PageQuery};
use crate::{AppendOutcome, Durability, NewUser, Role, Store};

const USERS: usize = 5;
const PER_USER: usize = 10_000;
const TURNS: usize = 3;
const PAGE: usize = 50;
/// The plan's bar for the first page.
const FIRST_PAGE_P99: Duration = Duration::from_millis(20);
/// "Does not block a concurrent append": an append waiting behind the
/// page and its annotations stays this short at p99 (stage 0: 0.3 ms;
/// behind today's whole list: 70 ms).
const APPEND_P99: Duration = Duration::from_millis(5);
/// The page's sub-agent chats: a handful of indexed lookups at most.
const CHILDREN_P99: Duration = Duration::from_millis(1);

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

const WORDS: &[&str] = &[
    "the",
    "build",
    "fails",
    "when",
    "cargo",
    "runs",
    "tests",
    "against",
    "sqlite",
    "store",
    "session",
    "list",
    "sidebar",
    "agent",
    "tool",
    "call",
    "approval",
    "node",
    "master",
    "link",
    "rust",
    "async",
    "tokio",
    "mutex",
    "reader",
    "writer",
    "index",
    "query",
    "page",
    "cursor",
    "workspace",
    "project",
    "routine",
    "memory",
    "search",
    "error",
    "fix",
    "refactor",
    "please",
    "explain",
    "function",
    "module",
    "crate",
    "compile",
    "warning",
    "clippy",
    "format",
    "deploy",
];

fn text(r: &mut Rng, words: usize) -> String {
    let mut s = String::with_capacity(words * 7);
    for i in 0..words {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(WORDS[r.below(WORDS.len() as u64) as usize]);
    }
    s
}

fn ev(sid: SessionId, seq: u64, ts: i64, kind: &str, payload: serde_json::Value) -> RawEvent {
    RawEvent {
        session_id: sid,
        seq,
        ts_ms: ts,
        kind: kind.into(),
        payload,
    }
}

/// `session_created` and three turns of six events each: 19 events.
fn session_events(r: &mut Rng, sid: SessionId, start: i64) -> Vec<RawEvent> {
    let mut v = vec![ev(
        sid,
        1,
        start,
        "session_created",
        json!({"title": text(r, 5), "cwd": "/home/u/code", "model": "demo", "profile": "agentic"}),
    )];
    let mut ts = start;
    for t in 0..TURNS {
        let n = v.len() as u64;
        ts += 1000 + r.below(60_000) as i64;
        let tid = format!("t{t}");
        let n_user = 20 + r.below(60) as usize;
        let user = text(r, n_user);
        let n_out = 80 + r.below(250) as usize;
        let out = text(r, n_out);
        let n_said = 80 + r.below(200) as usize;
        let said = text(r, n_said);
        v.push(ev(sid, n + 1, ts, "turn_started", json!({"turn_id": tid})));
        v.push(ev(
            sid,
            n + 2,
            ts,
            "user_message",
            json!({"message_id": format!("u{t}"), "text": user}),
        ));
        v.push(ev(
            sid,
            n + 3,
            ts + 5,
            "tool_call_started",
            json!({"call_id": format!("c{t}"), "tool": "shell", "input": {"command": "ls"}}),
        ));
        v.push(ev(
            sid,
            n + 4,
            ts + 900,
            "tool_call_finished",
            json!({"call_id": format!("c{t}"), "output": out, "ok": true}),
        ));
        v.push(ev(
            sid,
            n + 5,
            ts + 2000,
            "assistant_message",
            json!({"message_id": format!("a{t}"), "text": said}),
        ));
        v.push(ev(
            sid,
            n + 6,
            ts + 2001,
            "turn_completed",
            json!({"turn_id": tid, "status": "completed"}),
        ));
    }
    v
}

fn build(s: &Store, users: &[String]) {
    let t0 = Instant::now();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let now = harness_core::now_ms();
    for u in users {
        s.create_user(&NewUser {
            user_id: u.clone(),
            username: u.clone(),
            password_hash: "x".into(),
            role: Role::Member,
            exec_ceiling: harness_core::policy::ExecLevel::None,
            must_change_password: false,
        })
        .unwrap();
        s.personal_workspace(u).unwrap();
    }
    let team = s.create_workspace("team", &users[0]).unwrap().workspace_id;
    for u in &users[1..] {
        s.add_member(&team, u, "member").unwrap();
    }
    {
        let conn = s.lock();
        for u in users {
            for k in 0..5 {
                conn.execute(
                    "INSERT INTO projects (project_id, workspace_id, name, icon, accent, created_ms, updated_ms)
                     SELECT ?1, workspace_id, 'p', 'i', 'a', 0, 0 FROM workspaces WHERE personal_owner = ?2",
                    params![format!("{u}-p{k}"), u],
                )
                .unwrap();
            }
        }
        conn.execute(
            "INSERT INTO routines (routine_id, owner_id, workspace_id, name, spec, created_ms, updated_ms)
             SELECT 'r0', ?1, workspace_id, 'r', '{}', 0, 0 FROM workspaces WHERE personal_owner = ?1",
            params![users[0]],
        )
        .unwrap();
    }
    let mut parent: Option<String> = None;
    for batch in 0..(USERS * PER_USER / 500) {
        let mut conn = s.lock();
        let tx = conn.transaction().unwrap();
        for i in 0..500 {
            let k = batch * 500 + i;
            let u = &users[k % USERS];
            let sid = SessionId::new();
            // Spread over a year, newest at "now".
            let start = now - rng.below(365 * 86_400_000) as i64;
            for e in session_events(&mut rng, sid, start) {
                let o = Store::append_in(&tx, "node-1", &e).unwrap();
                assert!(matches!(o, AppendOutcome::Applied), "{o:?}");
            }
            let id = sid.to_string();
            let fin = if rng.below(10) == 0 { 19 } else { 0 };
            // One chat in 25 of user 0's is a sub-agent of the one before.
            let child = u == &users[0] && rng.below(25) == 0;
            tx.execute(
                "UPDATE sessions SET owner_id = ?2, finished_seq = ?3, parent_id = ?4 WHERE session_id = ?1",
                params![id, u, fin, if child { parent.clone() } else { None }],
            )
            .unwrap();
            if u == &users[0] && !child {
                parent = Some(id.clone());
            }
            if rng.below(20) == 0 {
                tx.execute(
                    "INSERT INTO session_workspaces (session_id, workspace_id) VALUES (?1, ?2)",
                    params![id, team],
                )
                .unwrap();
            }
            if rng.below(5) == 0 {
                tx.execute(
                    "INSERT INTO session_projects (session_id, project_id, added_ms) VALUES (?1, ?2, 0)",
                    params![id, format!("{u}-p{}", rng.below(5))],
                )
                .unwrap();
            }
            if u == &users[0] && rng.below(50) == 0 {
                tx.execute(
                    "INSERT INTO routine_runs (run_id, routine_id, trigger_kind, started_ms, status, session_id, seen)
                     VALUES (?1, 'r0', 'manual', 0, 'completed', ?2, ?3)",
                    params![format!("run-{id}"), id, rng.below(2) as i64],
                )
                .unwrap();
            }
        }
        tx.commit().unwrap();
    }
    println!("BUILD: {} chats in {:.1?}", USERS * PER_USER, t0.elapsed());
}

fn pct(mut v: Vec<Duration>) -> (Duration, Duration, Duration) {
    v.sort();
    let p = |q: f64| v[((v.len() as f64 - 1.0) * q) as usize];
    (p(0.5), p(0.99), v[v.len() - 1])
}

fn show(label: &str, v: Vec<Duration>) -> Duration {
    let n = v.len();
    let (p50, p99, max) = pct(v);
    println!("{label}: n={n} p50={p50:.2?} p99={p99:.2?} max={max:.2?}");
    p99
}

fn timed<R>(runs: usize, mut f: impl FnMut() -> R) -> (Vec<Duration>, R) {
    let mut v = Vec::with_capacity(runs);
    let mut last = None;
    for _ in 0..runs {
        let t = Instant::now();
        last = Some(f());
        v.push(t.elapsed());
    }
    (v, last.unwrap())
}

/// Appends `n` events to `sid` 0.5 ms apart, timing each.
fn appends(s: &Store, sid: SessionId, head: &mut u64, n: usize) -> Vec<Duration> {
    (0..n)
        .map(|_| {
            *head += 1;
            let e = ev(
                sid,
                *head,
                harness_core::now_ms(),
                "tool_call_finished",
                json!({"call_id": "x", "output": "ok", "ok": true}),
            );
            let t = Instant::now();
            let o = s.append("node-1", &e).unwrap();
            let took = t.elapsed();
            assert!(matches!(o, AppendOutcome::Applied), "{o:?}");
            std::thread::sleep(Duration::from_micros(500));
            took
        })
        .collect()
}

/// Appends while `reader` runs in a loop with 1 ms between runs (request
/// traffic; a hot loop starves the appender on the unfair std mutex, see
/// the note's hot-loop remark).
fn contended(
    s: &Store,
    sid: SessionId,
    head: &mut u64,
    reader: impl Fn() + Send + 'static,
) -> (Vec<Duration>, usize) {
    let stop = Arc::new(AtomicBool::new(false));
    let go = Arc::new(Barrier::new(2));
    let (st, g) = (stop.clone(), go.clone());
    let h = std::thread::spawn(move || {
        g.wait();
        let mut n = 0;
        while !st.load(Ordering::Relaxed) {
            reader();
            n += 1;
            std::thread::sleep(Duration::from_millis(1));
        }
        n
    });
    go.wait();
    let v = appends(s, sid, head, 300);
    stop.store(true, Ordering::Relaxed);
    (v, h.join().unwrap())
}

/// The page and the annotations the list needs for it; with `children`
/// also the page's sub-agent chats.
fn page_and_annotations(s: &Store, q: &PageQuery<'_>, viewer: &str, children: bool) -> usize {
    let (rows, _) = s.sessions_page(q).unwrap();
    let ids: Vec<SessionId> = rows.iter().map(|r| r.session_id).collect();
    let n = s.unread_of(viewer, &ids).unwrap().len()
        + s.project_links_of(&ids).unwrap().len()
        + s.routine_marks_of(&ids).unwrap().len();
    if children {
        n + s.children_of(q.owner, &ids).unwrap().len()
    } else {
        n
    }
}

#[test]
#[ignore = "50,000 chats; run by hand in release with BENCH_DB set"]
fn fifty_thousand_chats() {
    let path = std::path::PathBuf::from(std::env::var("BENCH_DB").expect("BENCH_DB"));
    let fresh = !path.exists();
    let s = Store::open(&path, Durability::Normal).unwrap();
    let users: Vec<String> = (0..USERS).map(|i| format!("user-{i}")).collect();
    if fresh {
        build(&s, &users);
    }
    println!(
        "DB: {:.1} MiB, {} chats",
        std::fs::metadata(&path).unwrap().len() as f64 / 1_048_576.0,
        s.sessions().unwrap().len()
    );
    let u0 = users[0].clone();
    let personal = s.personal_workspace(&u0).unwrap();
    let team: String = s
        .lock()
        .query_row(
            "SELECT workspace_id FROM workspaces WHERE name = 'team'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let q = PageQuery {
        owner: Owner::User(&u0),
        workspace: Some(&personal),
        top_level: true,
        hide_seen_runs: true,
        after: None,
        limit: PAGE,
    };

    // Today's handler, store side, for contrast.
    let (t, _) = timed(10, || {
        let r = s.sessions_of(Some(&u0)).unwrap();
        let _ = s.unread(Some(&u0), &u0).unwrap();
        let _ = s.project_links(None).unwrap();
        let ids: Vec<SessionId> = r.iter().map(|r| r.session_id).collect();
        s.routine_marks(&ids).unwrap().len()
    });
    show("today's list, store side (4 calls)", t);

    let (t, p) = timed(100, || s.sessions_page(&q).unwrap());
    assert_eq!(p.0.len(), PAGE);
    let first = show("first page, owner + Personal", t);
    let (t, _) = timed(100, || {
        s.sessions_page(&PageQuery {
            workspace: None,
            ..q
        })
        .unwrap()
    });
    let first_any = show("first page, owner, every workspace", t);
    let (t, _) = timed(100, || {
        s.sessions_page(&PageQuery {
            workspace: Some(&team),
            ..q
        })
        .unwrap()
    });
    show("first page, owner + team (5 % of the owner's chats)", t);
    let (t, _) = timed(20, || {
        s.sessions_page(&PageQuery {
            workspace: Some("no-such"),
            ..q
        })
        .unwrap()
    });
    show("first page, owner + a workspace with none (walks all)", t);
    let (t, _) = timed(20, || {
        s.sessions_page(&PageQuery {
            owner: Owner::Everyone,
            workspace: None,
            ..q
        })
        .unwrap()
    });
    show("first page, every owner (API token, no index)", t);
    let mut after = None;
    for _ in 0..100 {
        after = s
            .sessions_page(&PageQuery {
                after,
                workspace: None,
                ..q
            })
            .unwrap()
            .1;
    }
    assert!(after.is_some());
    let (t, _) = timed(100, || {
        s.sessions_page(&PageQuery {
            after,
            workspace: None,
            ..q
        })
        .unwrap()
    });
    show("page 101 by cursor", t);
    let ids: Vec<SessionId> = p.0.iter().map(|r| r.session_id).collect();
    let (t, _) = timed(100, || {
        s.unread_of(&u0, &ids).unwrap().len()
            + s.project_links_of(&ids).unwrap().len()
            + s.routine_marks_of(&ids).unwrap().len()
    });
    show("unread, links, marks of the page (3 statements)", t);
    let (t, _) = timed(100, || s.children_of(q.owner, &ids).unwrap().len());
    let children = show("children of the page", t);
    let (t, _) = timed(100, || page_and_annotations(&s, &q, &u0, true));
    show("page + every annotation", t);

    let rows = s.sessions_of(Some(&u0)).unwrap();
    let mid = &rows[rows.len() / 2];
    let (sid, mut head) = (mid.session_id, mid.head_seq);
    show(
        "append, nothing else running",
        appends(&s, sid, &mut head, 300),
    );
    let mut append = Vec::new();
    for with_children in [false, true] {
        let (s1, u, w) = (s.clone(), u0.clone(), personal.clone());
        let (t, n) = contended(&s, sid, &mut head, move || {
            let q = PageQuery {
                owner: Owner::User(&u),
                workspace: Some(&w),
                top_level: true,
                hide_seen_runs: true,
                after: None,
                limit: PAGE,
            };
            page_and_annotations(&s1, &q, &u, with_children);
        });
        let what = if with_children {
            "page + annotations + children"
        } else {
            "page + unread, links, marks"
        };
        append.push(show(&format!("append while {what} loop ({n} runs)"), t));
    }
    let (s1, u) = (s.clone(), u0.clone());
    let (t, n) = contended(&s, sid, &mut head, move || {
        let r = s1.sessions_of(Some(&u)).unwrap();
        let _ = s1.unread(Some(&u), &u).unwrap();
        let _ = s1.project_links(None).unwrap();
        let ids: Vec<SessionId> = r.iter().map(|r| r.session_id).collect();
        let _ = s1.routine_marks(&ids).unwrap();
    });
    show(
        &format!("append while today's list loops ({n} runs), for contrast"),
        t,
    );

    assert!(first < FIRST_PAGE_P99, "first page p99 {first:?}");
    assert!(first_any < FIRST_PAGE_P99, "first page p99 {first_any:?}");
    assert!(append[0] < APPEND_P99, "append p99 {:?}", append[0]);
    // Without migration 7's `sessions_parent` the children walk all of
    // the owner's chats under the lock (5.5 ms, append p99 5.9 ms).
    assert!(
        children < CHILDREN_P99 && append[1] < APPEND_P99,
        "children p99 {children:?}, append p99 behind them {:?}",
        append[1]
    );
}
