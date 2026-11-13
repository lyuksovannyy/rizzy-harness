//! Fix plan 4.3, delta re-check of the review fixes (R43-1 to R43-7):
//! proofs against the real runner (`harness_store::migrate`) and
//! `Store::open`. Not part of the suite: `run_rust_proofs.sh` copies this to
//! `crates/store/tests/` for one run.
//!
//! `finding_*` pass while the finding holds (they assert the wrong
//! behaviour). `holds_*` check a claim. `measure_*` only print.

use std::path::{Path, PathBuf};

use harness_store::migrate::{self, MigrateError, Migration};
use harness_store::{DB_SCHEMA, Durability, MIGRATIONS, Store};
use rusqlite::Connection;

fn version_of(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn entries(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            format!(
                "{} ({} bytes)",
                e.file_name().to_string_lossy(),
                e.metadata().unwrap().len()
            )
        })
        .collect();
    v.sort();
    v
}

// ---------------------------------------------------------------------
// D1: the preflight opens read-only. A read-only connection cannot roll a
// hot rollback journal back, so a `master.db` whose FIRST creation was
// interrupted between the journal's sync and its deletion (a kill -9, an
// OOM kill, a power cut while the commit syncs; a fresh file is in
// rollback-journal mode until `journal_mode = WAL` runs after the
// migration) is refused by the preflight on every later start, although
// `Store::open` on the very same files rolls the journal back and creates
// the database. The builder's T39 residual names only a NEWER build's hot
// journal ("should fail closed"); this is this build's own file.
//
// The crash image is made the only way one process can: migration 1 inside
// an open transaction with a small page cache, so the pager syncs the
// journal and writes pages to the file before the commit, and both files
// are copied while the transaction is still open. That is the on-disk
// state of a commit that died after writing its pages.
// ---------------------------------------------------------------------

/// `dir/master.db` + `dir/master.db-journal` as an interrupted first
/// creation leaves them.
fn crashed_first_creation(dir: &Path) -> PathBuf {
    let live = dir.join("live");
    std::fs::create_dir(&live).unwrap();
    let live_db = live.join("master.db");
    let c = Connection::open(&live_db).unwrap();
    c.pragma_update(None, "cache_size", 10).unwrap();
    c.execute_batch("BEGIN IMMEDIATE;").unwrap();
    for step in MIGRATIONS {
        for sql in *step {
            c.execute_batch(sql).unwrap();
        }
    }
    let journal = live.join("master.db-journal");
    assert!(journal.exists(), "no journal: {:?}", entries(&live));
    let size = std::fs::metadata(&live_db).unwrap().len();
    assert!(size > 0, "nothing spilled to the file: {:?}", entries(&live));
    let crash = dir.join("crash");
    std::fs::create_dir(&crash).unwrap();
    std::fs::copy(&live_db, crash.join("master.db")).unwrap();
    std::fs::copy(&journal, crash.join("master.db-journal")).unwrap();
    c.execute_batch("ROLLBACK;").unwrap();
    crash.join("master.db")
}

fn copy_image(from: &Path, to: &Path) -> PathBuf {
    std::fs::create_dir(to).unwrap();
    for name in ["master.db", "master.db-journal"] {
        std::fs::copy(from.parent().unwrap().join(name), to.join(name)).unwrap();
    }
    to.join("master.db")
}

#[test]
fn finding_d1_the_preflight_refuses_for_good_a_hot_journal_that_store_open_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let image = crashed_first_creation(dir.path());
    println!("D1: crash image: {:?}", entries(image.parent().unwrap()));

    // What `serve` does first now: the preflight. Three starts in a row.
    let a = copy_image(&image, &dir.path().join("a"));
    for start in 1..=3 {
        let err = migrate::check_file(&a, &DB_SCHEMA).unwrap_err();
        println!("D1: start {start}: preflight: {err}");
        assert!(matches!(err, MigrateError::Sqlite { .. }), "{err:?}");
    }
    println!(
        "D1: after three refused starts: {:?}",
        entries(a.parent().unwrap())
    );

    // What the same build does with the same files without the preflight
    // (and what origin/main does): it rolls the journal back and creates
    // the database.
    let b = copy_image(&image, &dir.path().join("b"));
    drop(Store::open(&b, Durability::Normal).expect("Store::open recovers the hot journal"));
    println!(
        "D1: Store::open on the same image: Ok, version {}, {:?}",
        version_of(&b),
        entries(b.parent().unwrap())
    );
    assert_eq!(version_of(&b), MIGRATIONS.len() as i64);
}

// ---------------------------------------------------------------------
// D2 (measure): what the read-only preflight leaves beside a WAL database
// that was closed cleanly (no -wal, no -shm before). The CLI test builds
// its newer file in rollback mode, so "only `lock` beside it" was never
// checked for the file kind the harness itself writes.
// ---------------------------------------------------------------------

#[test]
fn measure_d2_what_the_preflight_leaves_beside_a_clean_wal_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.db");
    drop(Store::open(&path, Durability::Full).unwrap());
    let before = std::fs::read(&path).unwrap();
    println!("D2: after Store::open and close: {:?}", entries(dir.path()));
    migrate::check_file(&path, &DB_SCHEMA).unwrap();
    println!("D2: after the preflight (current): {:?}", entries(dir.path()));
    for ext in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(dir.path().join(format!("node.db{ext}")));
    }
    Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 1)
        .unwrap();
    let newer = std::fs::read(&path).unwrap();
    assert_ne!(before, newer);
    println!("D2: newer WAL file, closed: {:?}", entries(dir.path()));
    let err = migrate::check_file(&path, &DB_SCHEMA).unwrap_err();
    println!("D2: preflight: {err}");
    println!("D2: after the refusal: {:?}", entries(dir.path()));
    assert_eq!(std::fs::read(&path).unwrap(), newer, "the file itself");
}

// ---------------------------------------------------------------------
// R43-7 (2): would `a_newer_rollback_journal_store_is_refused_before_any_
// pragma_writes` fail on main's order (journal_mode = WAL first)? The
// builder did not run it against that order. Emulated here on the same
// file: WAL first, then close, as a refusal after the pragma would.
// ---------------------------------------------------------------------

#[test]
fn holds_wal_first_changes_a_newer_rollback_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.db");
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (1);")
            .unwrap();
        c.pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 1)
            .unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    {
        let c = Connection::open(&path).unwrap();
        c.pragma_update(None, "journal_mode", "WAL").unwrap();
    }
    let after = std::fs::read(&path).unwrap();
    println!(
        "R43-7(2): main's order: bytes equal {}, header bytes 18..20 {:?} -> {:?}",
        before == after,
        &before[18..20],
        &after[18..20]
    );
    assert_ne!(before, after, "the builder's test would catch main's order");
    // And the real order leaves it alone.
    std::fs::write(&path, &before).unwrap();
    assert!(Store::open(&path, Durability::Full).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

// ---------------------------------------------------------------------
// R43-3: would `the_preflight_leaves_a_crashed_newer_builds_wal_alone`
// fail if `check_file` opened read-write? Emulated: the same crash image,
// a read-write connection reads the version and closes.
// ---------------------------------------------------------------------

#[test]
fn holds_a_read_write_open_checkpoints_a_crashed_wal() {
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("live.db");
    let crashed = dir.path().join("crashed");
    std::fs::create_dir(&crashed).unwrap();
    let c = Connection::open(&live).unwrap();
    c.pragma_update(None, "journal_mode", "WAL").unwrap();
    c.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    c.execute_batch("CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (1);")
        .unwrap();
    c.pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 1)
        .unwrap();
    for ext in ["", "-wal", "-shm"] {
        std::fs::copy(
            dir.path().join(format!("live.db{ext}")),
            crashed.join(format!("node.db{ext}")),
        )
        .unwrap();
    }
    drop(c);
    let path = crashed.join("node.db");
    let before = std::fs::read(&path).unwrap();
    {
        let rw = Connection::open(&path).unwrap();
        let v: i64 = rw.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, MIGRATIONS.len() as i64 + 1);
    }
    println!(
        "R43-3: read-write open and close: file equal {}, left {:?}",
        std::fs::read(&path).unwrap() == before,
        entries(&crashed)
    );
    assert_ne!(std::fs::read(&path).unwrap(), before);
}

// ---------------------------------------------------------------------
// R43-3: `check_file` refuses a file at the current version whose schema
// is not that version's (migrate.rs:184-186). No builder test calls
// `check_file` with such a file: behaviour shown here, coverage is not.
// ---------------------------------------------------------------------

#[test]
fn holds_the_preflight_refuses_a_current_version_with_another_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.db");
    drop(Store::open(&path, Durability::Full).unwrap());
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE added (x INTEGER);")
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let err = migrate::check_file(&path, &DB_SCHEMA).unwrap_err();
    println!("preflight mismatch: {err}");
    assert!(matches!(err, MigrateError::Mismatch { .. }), "{err:?}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

// ---------------------------------------------------------------------
// R43-5: `Busy` is given only for a failed BEGIN IMMEDIATE. On a file in
// rollback-journal mode (every fresh file until its first open ends) a
// writer that is committing holds the file exclusively, and then it is the
// version READ before BEGIN that times out.
// ---------------------------------------------------------------------

const V1: Migration = &["CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT NOT NULL);"];
const V2: Migration = &["ALTER TABLE t ADD COLUMN b INTEGER NOT NULL DEFAULT 7;"];

#[test]
fn measure_r43_5_a_committing_writer_on_a_rollback_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    {
        let mut c = Connection::open(&path).unwrap();
        migrate::migrate(&mut c, "test.db", &[V1]).unwrap();
    }
    let holder = Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let mut c = Connection::open(&path).unwrap();
    c.pragma_update(None, "busy_timeout", 50).unwrap();
    let err = migrate::migrate(&mut c, "test.db", &[V1, V2]).unwrap_err();
    println!("R43-5: exclusive holder, rollback mode: {err:?}\n        -> {err}");
    holder.execute_batch("ROLLBACK;").unwrap();
}

// ---------------------------------------------------------------------
// R43-1, re-checked on the real schema: `users` rebuilt the way SQLite
// documents it, with real child rows (`blobs`, ON DELETE CASCADE).
// ---------------------------------------------------------------------

#[test]
fn holds_r43_1_a_rebuild_of_the_real_users_table_keeps_its_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("master.db");
    drop(Store::open(&path, Durability::Normal).unwrap());
    let mut c = Connection::open(&path).unwrap();
    c.pragma_update(None, "foreign_keys", "ON").unwrap();
    c.execute_batch(
        "INSERT INTO users (user_id, username, password_hash, role, created_ms)
             VALUES ('u1', 'ann', 'x', 'admin', 1);
         INSERT INTO blobs (blob_id, owner_id, sha256, size, kind, format, mime, display, key, created_ms)
             VALUES ('b1', 'u1', 's1', 1, 'k', 'f', 'm', 'd', x'00', 1),
                    ('b2', 'u1', 's2', 1, 'k', 'f', 'm', 'd', x'00', 1);",
    )
    .unwrap();
    let users_sql: String = c
        .query_row("SELECT sql FROM sqlite_master WHERE name = 'users'", [], |r| r.get(0))
        .unwrap();
    let rebuild = format!(
        "{};
         INSERT INTO users_new SELECT * FROM users;
         DROP TABLE users;
         ALTER TABLE users_new RENAME TO users;",
        users_sql.replacen("CREATE TABLE users", "CREATE TABLE users_new", 1)
    );
    let rebuild: &'static str = Box::leak(rebuild.into_boxed_str());
    let step: &'static [&'static str] = Box::leak(vec![rebuild].into_boxed_slice());
    let steps: Vec<Migration> = vec![MIGRATIONS[0], step];
    let done = migrate::migrate(&mut c, "master.db", &steps).unwrap();
    let count = |t: &str| -> i64 {
        c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    };
    let fk: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
    println!(
        "R43-1: {done:?}; users {}, blobs {} (was 2); foreign_keys after = {fk}",
        count("users"),
        count("blobs")
    );
    assert_eq!((count("users"), count("blobs"), fk), (1, 2, 1));
    // The cascade still works afterwards (the children still point at `users`).
    c.execute("DELETE FROM users", []).unwrap();
    assert_eq!(count("blobs"), 0);
}

// ---------------------------------------------------------------------
// R43-1, the other side of the check: a reference that dangled BEFORE the
// migration (a row removed with the sqlite3 shell, whose foreign keys are
// off by default) fails every later migration, and the message blames the
// migration.
// ---------------------------------------------------------------------

const FK_V1: Migration = &["CREATE TABLE users (id TEXT PRIMARY KEY);
     CREATE TABLE blobs (id TEXT PRIMARY KEY,
         owner TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE);
     CREATE TABLE other (x INTEGER);"];
const FK_V2: Migration = &["ALTER TABLE other ADD COLUMN y INTEGER;"];

#[test]
fn measure_r43_1_a_reference_that_dangled_before_blocks_every_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    let mut c = Connection::open(&path).unwrap();
    migrate::migrate(&mut c, "test.db", &[FK_V1]).unwrap();
    c.execute_batch("INSERT INTO users VALUES ('u1'); INSERT INTO blobs VALUES ('b1', 'u1');")
        .unwrap();
    c.pragma_update(None, "foreign_keys", "OFF").unwrap();
    c.execute("DELETE FROM users", []).unwrap();
    c.pragma_update(None, "foreign_keys", "ON").unwrap();
    let err = migrate::migrate(&mut c, "test.db", &[FK_V1, FK_V2]).unwrap_err();
    println!("R43-1: pre-existing dangling row: {err}");
    assert!(matches!(err, MigrateError::Dangling { .. }), "{err:?}");
}

// ---------------------------------------------------------------------
// R43-4: the two log lines, which the builder checked by reading only.
// A hand-made subscriber records every event of this thread.
// ---------------------------------------------------------------------

static EVENTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

struct Capture;

impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct V(String);
        impl tracing::field::Visit for V {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                self.0.push_str(&format!("{}={:?} ", f.name(), v));
            }
        }
        let mut v = V(format!(
            "{} {}: ",
            event.metadata().level(),
            event.metadata().target()
        ));
        event.record(&mut v);
        EVENTS.lock().unwrap().push(v.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn holds_r43_4_a_real_migration_is_logged_before_and_after_and_a_creation_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.db");
    tracing::subscriber::with_default(Capture, || {
        let mut c = Connection::open(&path).unwrap();
        migrate::migrate(&mut c, "node.db", &[V1]).unwrap();
        let after_creation = EVENTS.lock().unwrap().len();
        println!("R43-4: events for a creation (0 -> 1): {after_creation}");
        migrate::migrate(&mut c, "node.db", &[V1, V2]).unwrap();
        let events = EVENTS.lock().unwrap().clone();
        for e in &events {
            println!("R43-4: {e}");
        }
        assert_eq!(after_creation, 0);
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e.starts_with("INFO")
            && e.contains("node.db")
            && e.contains("from=1")
            && e.contains("to=2")));
        // A failing migration: the "before" line only.
        const BROKEN: Migration = &["INSERT INTO no_such_table VALUES (1);"];
        migrate::migrate(&mut c, "node.db", &[V1, V2, BROKEN]).unwrap_err();
        let events = EVENTS.lock().unwrap().clone();
        println!("R43-4: after a failed migration: {} events; last: {}", events.len(), events.last().unwrap());
        assert_eq!(events.len(), 3);
    });
}
