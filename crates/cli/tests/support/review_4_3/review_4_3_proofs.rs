//! Fix plan 4.3 review: proofs against the real runner
//! (`harness_store::migrate`) and `Store::open`. Not part of the suite:
//! `run_rust_proofs.sh` copies this to `crates/store/tests/` for one run.
//!
//! Tests named `finding_*` pass while the finding holds (they assert the
//! wrong behaviour). Tests named `holds_*` check a claim of the builder.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use harness_store::migrate::{self, MigrateError, Migration};
use harness_store::{Durability, MIGRATIONS, Store};
use rusqlite::Connection;

fn version_of(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

/// As `Store::open`: busy_timeout, migrate, then WAL.
fn open(path: &Path, steps: &[Migration]) -> Result<Connection, MigrateError> {
    let mut conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "busy_timeout", 5000).unwrap();
    migrate::migrate(&mut conn, &path.display().to_string(), steps)?;
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    // As `Store::init`.
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    Ok(conn)
}

// ---------------------------------------------------------------------
// F1: foreign keys are ON while a migration runs (bundled SQLite is built
// with SQLITE_DEFAULT_FOREIGN_KEYS=1) and the runner gives a migration no
// way to turn them off (the pragma is a no-op inside a transaction). The
// documented way to change a table in SQLite (new table, copy, drop old,
// rename) then deletes every row that references it ON DELETE CASCADE.
// ---------------------------------------------------------------------

const FK_V1: Migration = &["
    CREATE TABLE users (user_id TEXT PRIMARY KEY, username TEXT NOT NULL);
    CREATE TABLE blobs (
        blob_id  TEXT PRIMARY KEY,
        owner_id TEXT REFERENCES users(user_id) ON DELETE CASCADE);
"];
/// "Make username UNIQUE": only possible by rebuilding the table.
const FK_V2: Migration = &["
    CREATE TABLE users_new (user_id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE);
    INSERT INTO users_new SELECT user_id, username FROM users;
    DROP TABLE users;
    ALTER TABLE users_new RENAME TO users;
"];

#[test]
fn finding_f1_a_table_rebuild_migration_deletes_the_rows_that_reference_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("master.db");
    {
        let c = open(&path, &[FK_V1]).unwrap();
        c.execute("INSERT INTO users VALUES ('u1', 'ann')", []).unwrap();
        c.execute("INSERT INTO blobs VALUES ('b1', 'u1')", []).unwrap();
        c.execute("INSERT INTO blobs VALUES ('b2', 'u1')", []).unwrap();
    }
    // A fresh connection, as the next start: nothing has set foreign_keys.
    let raw = Connection::open(&path).unwrap();
    let fk: i64 = raw.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    println!("F1: foreign_keys on a fresh connection, before migrate = {fk}");
    drop(raw);

    let c = open(&path, &[FK_V1, FK_V2]).expect("the migration reports success");
    let users: i64 = c.query_row("SELECT count(*) FROM users", [], |r| r.get(0)).unwrap();
    let blobs: i64 = c.query_row("SELECT count(*) FROM blobs", [], |r| r.get(0)).unwrap();
    println!("F1: after migration 2: version {}, users {users}, blobs {blobs} (was 2)", version_of(&path));
    assert_eq!(users, 1);
    assert_eq!(blobs, 0, "FINDING GONE: the child rows survived");
}

// ---------------------------------------------------------------------
// F2: atomic per file, not per data directory. `serve` opens master.db,
// node.db, memory.db, chat.db one after the other. One migrates, the next
// fails: the new build cannot start, and the old build refuses the file
// that did migrate.
// ---------------------------------------------------------------------

const D_V1: Migration = &["CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT NOT NULL);"];
/// Fine on a database without duplicates, fails on one with.
const D_V2: Migration = &["CREATE UNIQUE INDEX t_a ON t(a);"];

#[test]
fn finding_f2_one_file_migrates_the_next_fails_and_neither_build_starts() {
    let dir = tempfile::tempdir().unwrap();
    let master = dir.path().join("master.db");
    let chat = dir.path().join("chat.db");
    drop(open(&master, &[D_V1]).unwrap());
    {
        let c = open(&chat, &[D_V1]).unwrap();
        c.execute("INSERT INTO t (a) VALUES ('dup')", []).unwrap();
        c.execute("INSERT INTO t (a) VALUES ('dup')", []).unwrap();
    }
    // The new build starts: master.db first, then chat.db.
    let start = |steps: &[Migration]| -> Result<(), MigrateError> {
        drop(open(&master, steps)?);
        drop(open(&chat, steps)?);
        Ok(())
    };
    let new = start(&[D_V1, D_V2]).unwrap_err();
    println!("F2 new build: {new}");
    assert!(matches!(new, MigrateError::Failed { .. }));
    assert_eq!((version_of(&master), version_of(&chat)), (2, 1));
    // Going back to the old build:
    let old = start(&[D_V1]).unwrap_err();
    println!("F2 old build: {old}");
    assert!(matches!(old, MigrateError::TooNew { .. }));
    // And the new build again: the same failure.
    assert!(matches!(start(&[D_V1, D_V2]), Err(MigrateError::Failed { .. })));
}

// ---------------------------------------------------------------------
// F3: until the tag "migration 1 is edited in place". A database already
// stamped 1 then gets nothing of the edit, with no message; before this
// change the IF NOT EXISTS statements ran on every open and an added
// table or index appeared.
// ---------------------------------------------------------------------

const E_V1: Migration = &["CREATE TABLE IF NOT EXISTS t (id INTEGER PRIMARY KEY, a TEXT);"];
const E_V1_EDITED: Migration = &[
    "CREATE TABLE IF NOT EXISTS t (id INTEGER PRIMARY KEY, a TEXT);",
    "CREATE TABLE IF NOT EXISTS added (id INTEGER PRIMARY KEY);
     CREATE INDEX IF NOT EXISTS t_a ON t(a);",
];

#[test]
fn finding_f3_an_in_place_edit_of_migration_1_never_reaches_a_stamped_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.db");
    drop(open(&path, &[E_V1]).unwrap());
    let c = open(&path, &[E_V1_EDITED]).expect("opens without a word");
    let n: i64 = c
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name IN ('added', 't_a')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let err = c.execute("INSERT INTO added DEFAULT VALUES", []).unwrap_err();
    println!("F3: objects of the edit present: {n} of 2; first use: {err}");
    assert_eq!(n, 0, "FINDING GONE");
    // What main does today (every statement on every open):
    for sql in E_V1_EDITED {
        c.execute_batch(sql).unwrap();
    }
    c.execute("INSERT INTO added DEFAULT VALUES", []).unwrap();
}

// ---------------------------------------------------------------------
// F4: a version-0 file with tables of another shape is stamped version 1.
// ---------------------------------------------------------------------

#[test]
fn finding_f4_a_stale_version_0_database_is_stamped_version_1() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("master.db");
    {
        let c = Connection::open(&path).unwrap();
        // `nodes` as an older dev build might have had it.
        c.execute_batch("CREATE TABLE nodes (node_id TEXT PRIMARY KEY); INSERT INTO nodes VALUES ('n1');")
            .unwrap();
    }
    match Store::open(&path, Durability::Normal) {
        Ok(_) => {}
        Err(e) => panic!("FINDING GONE (refused): {e}"),
    }
    let c = Connection::open(&path).unwrap();
    let recorded = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/1.sql"),
    )
    .unwrap();
    let now = migrate::schema_text(&c).unwrap();
    println!(
        "F4: version {} ; schema equals schema/1.sql: {}",
        version_of(&path),
        now == recorded
    );
    assert_eq!(version_of(&path), MIGRATIONS.len() as i64);
    assert_ne!(now, recorded);
}

// ---------------------------------------------------------------------
// Read-only: migrate.rs says "a read-only disk still opens an up-to-date
// one". Printed, not asserted either way.
// ---------------------------------------------------------------------

#[test]
fn claim_read_only_directory_and_up_to_date_database() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("data");
    std::fs::create_dir(&sub).unwrap();
    let path = sub.join("node.db");
    drop(Store::open(&path, Durability::Full).unwrap());
    let leftovers: Vec<_> = std::fs::read_dir(&sub)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    println!("RO: files after a clean close: {leftovers:?}");

    // (a) the file read-only, the directory writable
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    let a = Store::open(&path, Durability::Full).map(|_| ()).map_err(|e| e.to_string());
    println!("RO (a) file 0444, dir writable: {a:?}");

    // (b) the directory read-only too (a read-only mount)
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o555)).unwrap();
    let b = Store::open(&path, Durability::Full).map(|_| ()).map_err(|e| e.to_string());
    println!("RO (b) file 0444, dir 0555:     {b:?}");

    // (c) the same through the runner alone
    let mut conn = Connection::open(&path).unwrap();
    let c = migrate::migrate(&mut conn, "node.db", MIGRATIONS).map_err(|e| e.to_string());
    println!("RO (c) migrate() alone:          {c:?}");

    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
}

// ---------------------------------------------------------------------
// Holds: a migration that fails for a full disk after pages were already
// written to the file (cache spill) leaves it byte-identical, in both
// journal modes.
// ---------------------------------------------------------------------

const BIG: Migration = &[
    "CREATE TABLE big (x BLOB);",
    "WITH RECURSIVE c(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM c WHERE i < 20000)
     INSERT INTO big SELECT randomblob(400) FROM c;",
];

#[test]
fn holds_a_full_database_mid_migration_leaves_the_file_byte_identical() {
    for wal in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let mut c = Connection::open(&path).unwrap();
            migrate::migrate(&mut c, "test.db", &[D_V1]).unwrap();
            if wal {
                c.pragma_update(None, "journal_mode", "WAL").unwrap();
            }
            c.execute("INSERT INTO t (a) VALUES ('kept')", []).unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let err = {
            let mut c = Connection::open(&path).unwrap();
            c.pragma_update(None, "cache_size", 5).unwrap();
            let pages: i64 = c.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap();
            let _: i64 = c
                .query_row(&format!("PRAGMA max_page_count = {}", pages + 300), [], |r| r.get(0))
                .unwrap();
            migrate::migrate(&mut c, "test.db", &[D_V1, BIG]).unwrap_err()
        };
        println!("FULL wal={wal}: {err}");
        assert!(matches!(err, MigrateError::Failed { step: 2, .. }), "{err:?}");
        assert!(err.to_string().contains("full"), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "wal={wal}");
        assert_eq!(version_of(&path), 1);
    }
}

// ---------------------------------------------------------------------
// Holds: kill -9 in the middle of a migration (after pages reached the
// disk) leaves version N, no half-made table, and a file that passes
// integrity_check; the next start migrates.
// ---------------------------------------------------------------------

const SLOW: Migration = &[
    "CREATE TABLE big (x BLOB);",
    "WITH RECURSIVE c(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM c WHERE i < 400000)
     INSERT INTO big SELECT randomblob(200) FROM c;",
    // Reached only after the insert: keeps the transaction open.
    "WITH RECURSIVE c(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM c WHERE i < 300000000)
     SELECT count(*) FROM c;",
];
const SLOW_OK: Migration = &["CREATE TABLE big (x BLOB);"];

/// The child of the next test: runs only when told to.
#[test]
fn child_migrates_slowly() {
    let Some(path) = std::env::var_os("REVIEW_43_CHILD") else {
        return;
    };
    let path = PathBuf::from(path);
    let mut c = Connection::open(&path).unwrap();
    c.pragma_update(None, "cache_size", 5).unwrap();
    let r = migrate::migrate(&mut c, "test.db", &[D_V1, SLOW]);
    // Not reached before the kill; if it is, say so.
    std::fs::write(path.with_extension("done"), format!("{r:?}")).unwrap();
}

#[test]
fn holds_kill_9_mid_migration_leaves_version_n_and_a_sound_file() {
    for wal in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let mut c = Connection::open(&path).unwrap();
            migrate::migrate(&mut c, "test.db", &[D_V1]).unwrap();
            if wal {
                c.pragma_update(None, "journal_mode", "WAL").unwrap();
            }
            c.execute("INSERT INTO t (a) VALUES ('kept')", []).unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "child_migrates_slowly", "--nocapture"])
            .env("REVIEW_43_CHILD", &path)
            .spawn()
            .unwrap();
        // Wait until the migration has written at least 4 MiB somewhere.
        let side = if wal { "test.db-wal" } else { "test.db" };
        let side = dir.path().join(side);
        let t0 = Instant::now();
        let grown = loop {
            let len = std::fs::metadata(&side).map(|m| m.len()).unwrap_or(0);
            let grown = len as i64 - if wal { 0 } else { before.len() as i64 };
            if grown > 4 << 20 {
                break grown;
            }
            assert!(t0.elapsed() < Duration::from_secs(60), "the child wrote nothing");
            assert!(child.try_wait().unwrap().is_none(), "the child ended early");
            std::thread::sleep(Duration::from_millis(20));
        };
        child.kill().unwrap(); // SIGKILL
        child.wait().unwrap();
        assert!(!path.with_extension("done").exists(), "the child finished before the kill");
        let hot = dir.path().join("test.db-journal").exists();
        println!("KILL wal={wal}: killed with {grown} bytes written; hot journal: {hot}");

        // The old build's start.
        {
            let c = open(&path, &[D_V1]).unwrap();
            let ok: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
            assert_eq!(ok, "ok");
            let big: i64 = c
                .query_row("SELECT count(*) FROM sqlite_master WHERE name = 'big'", [], |r| r.get(0))
                .unwrap();
            assert_eq!(big, 0);
            let kept: String = c.query_row("SELECT a FROM t", [], |r| r.get(0)).unwrap();
            assert_eq!(kept, "kept");
        }
        assert_eq!(version_of(&path), 1, "wal={wal}");
        if !wal {
            // Rolled back from the journal: the same bytes, the journal mode
            // aside (`open` switched it to WAL: bytes 18 and 19 of the header)
            // and the change counters.
            let after = std::fs::read(&path).unwrap();
            println!("KILL wal=false: file length before {} after {}", before.len(), after.len());
            assert_eq!(after.len(), before.len());
        }
        // The new build's next start.
        drop(open(&path, &[D_V1, SLOW_OK]).unwrap());
        assert_eq!(version_of(&path), 2);
    }
}

// ---------------------------------------------------------------------
// Two openers of one fresh file at once (what the data directory lock
// prevents between processes; here: threads, each with its own
// connection, which is the same thing to SQLite).
// ---------------------------------------------------------------------

#[test]
fn claim_many_openers_of_one_fresh_file() {
    let mut failures = Vec::new();
    let rounds = 60;
    for round in 0..rounds {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.db");
        let gate = Arc::new(Barrier::new(6));
        let threads: Vec<_> = (0..6)
            .map(|_| {
                let (path, gate) = (path.clone(), gate.clone());
                std::thread::spawn(move || {
                    gate.wait();
                    Store::open(&path, Durability::Full).map(|_| ()).map_err(|e| e.to_string())
                })
            })
            .collect();
        for t in threads {
            if let Err(e) = t.join().unwrap() {
                failures.push((round, e));
            }
        }
        assert_eq!(version_of(&path), MIGRATIONS.len() as i64);
        let mode: String = Connection::open(&path)
            .unwrap()
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        if mode != "wal" {
            failures.push((round, format!("journal_mode ended as {mode}")));
        }
    }
    println!("RACE: {} failures in {rounds} rounds of 6 openers", failures.len());
    let mut kinds = std::collections::BTreeMap::<String, usize>::new();
    for (_, e) in &failures {
        *kinds.entry(e.clone()).or_default() += 1;
    }
    for (e, n) in kinds {
        println!("RACE: {n} x {e}");
    }
}

// ---------------------------------------------------------------------
// The race taken apart: which statement answers "database is locked", in
// which journal mode, and whether main's order (WAL first, no version)
// does the same.
// ---------------------------------------------------------------------

fn race(
    label: &str,
    rounds: usize,
    prepare: impl Fn(&Path),
    opener: impl Fn(&Path) -> Result<(), String> + Send + Sync + Clone + 'static,
) -> usize {
    let mut kinds = std::collections::BTreeMap::<String, usize>::new();
    let t0 = Instant::now();
    for _ in 0..rounds {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.db");
        prepare(&path);
        let gate = Arc::new(Barrier::new(6));
        let threads: Vec<_> = (0..6)
            .map(|_| {
                let (path, gate, opener) = (path.clone(), gate.clone(), opener.clone());
                std::thread::spawn(move || {
                    gate.wait();
                    opener(&path)
                })
            })
            .collect();
        for t in threads {
            if let Err(e) = t.join().unwrap() {
                *kinds.entry(e).or_default() += 1;
            }
        }
    }
    let total: usize = kinds.values().sum();
    println!(
        "RACE2 [{label}]: {total} failed opens of {} in {:.1}s",
        rounds * 6,
        t0.elapsed().as_secs_f64()
    );
    for (e, n) in &kinds {
        println!("RACE2 [{label}]:   {n} x {e}");
    }
    total
}

/// The runner's own statements, one label each (`migrate.rs` lines 90-113).
fn staged(path: &Path, steps: &[Migration], wal_after: bool) -> Result<(), String> {
    let mut conn = Connection::open(path).map_err(|e| format!("open: {e}"))?;
    conn.pragma_update(None, "busy_timeout", 5000)
        .map_err(|e| format!("busy_timeout: {e}"))?;
    let known = steps.len() as i64;
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| format!("first read of user_version: {e}"))?;
    if v != known {
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| format!("BEGIN IMMEDIATE: {e}"))?;
        let from: i64 = tx
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| format!("second read of user_version: {e}"))?;
        for step in steps.iter().skip(from as usize) {
            for sql in *step {
                tx.execute_batch(sql).map_err(|e| format!("step: {e}"))?;
            }
        }
        tx.pragma_update(None, "user_version", known)
            .map_err(|e| format!("write of user_version: {e}"))?;
        tx.commit().map_err(|e| format!("COMMIT: {e}"))?;
    }
    if wal_after {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| format!("journal_mode=WAL: {e}"))?;
    }
    Ok(())
}

#[test]
fn race_taken_apart() {
    let fresh = |_: &Path| {};
    // 1. The runner's statements on a fresh file, then WAL (as Store::open).
    race("fresh file, staged, WAL after", 12, fresh, |p| staged(p, MIGRATIONS, true));
    // 2. The same without the switch to WAL.
    race("fresh file, staged, no WAL", 12, fresh, |p| staged(p, MIGRATIONS, false));
    // 3. An existing WAL database at version 1, six openers at version 2.
    race(
        "WAL database one version behind",
        12,
        |p| drop(open(p, &[D_V1]).unwrap()),
        |p| open(p, &[D_V1, SLOW_OK]).map(|_| ()).map_err(|e| e.to_string()),
    );
    // 4. An up-to-date Store, six openers.
    race(
        "up-to-date Store",
        12,
        |p| drop(Store::open(p, Durability::Full).unwrap()),
        |p| Store::open(p, Durability::Full).map(|_| ()).map_err(|e| e.to_string()),
    );
    // 5. main's order: WAL, synchronous, busy_timeout, then every
    //    IF NOT EXISTS batch in autocommit.
    race("fresh file, main's Store::open", 12, fresh, |p| {
        let conn = Connection::open(p).map_err(|e| format!("open: {e}"))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| format!("journal_mode=WAL: {e}"))?;
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(|e| format!("synchronous: {e}"))?;
        conn.pragma_update(None, "busy_timeout", 5000)
            .map_err(|e| format!("busy_timeout: {e}"))?;
        for sql in MIGRATIONS[0] {
            conn.execute_batch(sql).map_err(|e| format!("schema: {e}"))?;
        }
        Ok(())
    });
}

/// One round of six openers on a fresh file, each stage timed.
#[test]
fn race_timed() {
    for wal_first in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("node.db");
        if wal_first {
            // An existing WAL database one version behind.
            drop(open(&path, &[D_V1]).unwrap());
        }
        let steps: &'static [Migration] = if wal_first { &[D_V1, SLOW_OK] } else { MIGRATIONS };
        let gate = Arc::new(Barrier::new(6));
        let t0 = Instant::now();
        let threads: Vec<_> = (0..6)
            .map(|n| {
                let (path, gate) = (path.clone(), gate.clone());
                std::thread::spawn(move || {
                    gate.wait();
                    let ms = |t: Instant| t.elapsed().as_millis();
                    let mut log = format!("T{n}:");
                    let mut conn = Connection::open(&path).unwrap();
                    conn.pragma_update(None, "busy_timeout", 5000).unwrap();
                    let t = Instant::now();
                    let v: rusqlite::Result<i64> = conn.query_row("PRAGMA user_version", [], |r| r.get(0));
                    log += &format!(" read {v:?} {}ms;", ms(t));
                    let t = Instant::now();
                    match conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate) {
                        Err(e) => log += &format!(" BEGIN IMMEDIATE failed after {}ms: {e}", ms(t)),
                        Ok(tx) => {
                            log += &format!(" BEGIN {}ms;", ms(t));
                            let from: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
                            let t = Instant::now();
                            for step in steps.iter().skip(from as usize) {
                                for sql in *step {
                                    tx.execute_batch(sql).unwrap();
                                }
                            }
                            tx.pragma_update(None, "user_version", steps.len() as i64).unwrap();
                            log += &format!(" from {from}, steps {}ms;", ms(t));
                            let t = Instant::now();
                            let r = tx.commit();
                            log += &format!(" COMMIT {r:?} {}ms;", ms(t));
                        }
                    }
                    let t = Instant::now();
                    let r = conn.pragma_update(None, "journal_mode", "WAL");
                    log += &format!(" WAL {r:?} {}ms", ms(t));
                    log
                })
            })
            .collect();
        for t in threads {
            println!("TIMED wal_first={wal_first} {}", t.join().unwrap());
        }
        println!("TIMED wal_first={wal_first} round took {}ms", t0.elapsed().as_millis());
    }
}
