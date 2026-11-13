//! Schema versions (fix plan 4.3): `PRAGMA user_version` and numbered
//! migrations for `master.db`, `chat.db` and `node.db` (`Store`). memory.db
//! has the same tests in `crates/memory/tests/schema.rs`.

use std::path::Path;

use harness_store::migrate::{self, MigrateError, Migration};
use harness_store::{Durability, MIGRATIONS, Store};
use rusqlite::Connection;

const V1: Migration = &["CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT NOT NULL);"];
const V2: Migration = &[
    "ALTER TABLE t ADD COLUMN b INTEGER NOT NULL DEFAULT 7;",
    "CREATE INDEX t_b ON t(b);",
];
/// Its first statement works, its second does not: nothing of it may stay.
const BROKEN: Migration = &[
    "CREATE TABLE half (x INTEGER); INSERT INTO half VALUES (1);",
    "INSERT INTO no_such_table VALUES (1);",
];

/// Open like the harness does: WAL, as `Store::open` and `SqliteMemory::open`.
fn open(path: &Path, steps: &[Migration]) -> Result<Connection, MigrateError> {
    let mut conn = Connection::open(path).unwrap();
    migrate::migrate(&mut conn, "test.db", steps)?;
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    Ok(conn)
}

fn version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn version_of(path: &Path) -> i64 {
    version(&Connection::open(path).unwrap())
}

fn set_version(path: &Path, v: i64) {
    let c = Connection::open(path).unwrap();
    c.pragma_update(None, "user_version", v).unwrap();
}

#[test]
fn a_database_one_version_behind_is_migrated_and_keeps_its_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    {
        let c = open(&path, &[V1]).unwrap();
        assert_eq!(version(&c), 1);
        c.execute("INSERT INTO t (a) VALUES ('kept')", []).unwrap();
    }
    let c = open(&path, &[V1, V2]).unwrap();
    assert_eq!(version(&c), 2);
    let (a, b): (String, i64) = c
        .query_row("SELECT a, b FROM t", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!((a.as_str(), b), ("kept", 7));
    // Opening again at the same version runs nothing (an ALTER twice fails).
    drop(c);
    assert_eq!(version(&open(&path, &[V1, V2]).unwrap()), 2);
}

#[test]
fn a_database_from_a_newer_build_is_refused_and_left_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    {
        let c = open(&path, &[V1, V2]).unwrap();
        c.execute("INSERT INTO t (a) VALUES ('x')", []).unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    let err = open(&path, &[V1]).unwrap_err();
    assert!(
        matches!(
            err,
            MigrateError::TooNew {
                found: 2,
                known: 1,
                ..
            }
        ),
        "{err:?}"
    );
    let msg = err.to_string();
    for part in ["test.db", "newer", "version 2", "up to 1", "not changed"] {
        assert!(msg.contains(part), "{part:?} missing from: {msg}");
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(version_of(&path), 2);
}

#[test]
fn a_negative_version_is_refused_and_left_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    drop(open(&path, &[V1]).unwrap());
    set_version(&path, -1);
    let before = std::fs::read(&path).unwrap();
    let err = open(&path, &[V1]).unwrap_err();
    assert!(
        matches!(err, MigrateError::Unknown { found: -1, .. }),
        "{err:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn a_failing_migration_leaves_the_file_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    {
        let c = open(&path, &[V1]).unwrap();
        c.execute("INSERT INTO t (a) VALUES ('kept')", []).unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    // V2 succeeds, BROKEN fails halfway: neither may be applied.
    let err = open(&path, &[V1, V2, BROKEN]).unwrap_err();
    match &err {
        MigrateError::Failed { from, to, step, .. } => assert_eq!((*from, *to, *step), (1, 3, 3)),
        other => panic!("{other:?}"),
    }
    assert!(err.to_string().contains("nothing was changed"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let c = open(&path, &[V1]).unwrap();
    assert_eq!(version(&c), 1);
    let half: i64 = c
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name IN ('half', 't_b')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(half, 0);
}

#[test]
fn a_fresh_store_carries_the_latest_version() {
    let dir = tempfile::tempdir().unwrap();
    for (name, d) in [
        ("node.db", Durability::Full),
        ("master.db", Durability::Normal),
    ] {
        let path = dir.path().join(name);
        drop(Store::open(&path, d).unwrap());
        assert_eq!(version_of(&path), MIGRATIONS.len() as i64, "{name}");
    }
}

/// `master.db` (Normal), `chat.db` and `node.db` (Full) all open through
/// `Store::open`.
#[test]
fn a_store_from_a_newer_build_is_refused_and_left_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    for (name, d) in [
        ("master.db", Durability::Normal),
        ("chat.db", Durability::Full),
        ("node.db", Durability::Full),
    ] {
        let path = dir.path().join(name);
        drop(Store::open(&path, d).unwrap());
        let newer = MIGRATIONS.len() as i64 + 1;
        set_version(&path, newer);
        let before = std::fs::read(&path).unwrap();
        let err = match Store::open(&path, d) {
            Ok(_) => panic!("{name} from a newer build was opened"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains(name) && err.contains("newer"), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "{name}");
        assert_eq!(version_of(&path), newer, "{name}");
    }
}

/// The parent and a child that cascades, as `users` and `blobs` do.
const FK_V1: Migration = &["CREATE TABLE users (id TEXT PRIMARY KEY, name TEXT);
     CREATE TABLE blobs (id TEXT PRIMARY KEY,
         owner TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE);"];
/// `users.name` becomes NOT NULL the way SQLite documents a table change:
/// a new table, a copy, the old one dropped, the new one renamed.
const FK_REBUILD: Migration = &[
    "CREATE TABLE users_new (id TEXT PRIMARY KEY, name TEXT NOT NULL);
     INSERT INTO users_new SELECT id, name FROM users;
     DROP TABLE users;
     ALTER TABLE users_new RENAME TO users;",
];
/// Leaves every `blobs` row pointing at a user that is gone.
const FK_DANGLING: Migration = &["DELETE FROM users;"];

/// A connection with foreign keys on, as `Store` keeps them, and one user
/// with two blobs.
fn fk_db(path: &Path) -> Connection {
    let mut c = Connection::open(path).unwrap();
    c.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate::migrate(&mut c, "test.db", &[FK_V1]).unwrap();
    c.execute_batch(
        "INSERT INTO users VALUES ('u1', 'ann');
         INSERT INTO blobs VALUES ('b1', 'u1'), ('b2', 'u1');",
    )
    .unwrap();
    c
}

fn count(c: &Connection, table: &str) -> i64 {
    c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn foreign_keys(c: &Connection) -> i64 {
    c.query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap()
}

/// Review R43-1: with foreign keys on during the migration, the DROP of
/// the old table cascades and deletes every child row, and the migration
/// still reports success.
#[test]
fn a_table_rebuild_migration_keeps_the_rows_that_reference_the_table() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    drop(fk_db(&path));
    let mut c = Connection::open(&path).unwrap();
    c.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate::migrate(&mut c, "test.db", &[FK_V1, FK_REBUILD]).unwrap();
    assert_eq!(version(&c), 2);
    assert_eq!((count(&c, "users"), count(&c, "blobs")), (1, 2));
    assert_eq!(foreign_keys(&c), 1, "foreign keys are on again");
}

/// Review R43-1: with foreign keys off during the migration, a dangling
/// reference must fail the migration (`foreign_key_check`), not commit.
#[test]
fn a_migration_that_leaves_a_dangling_reference_fails_and_leaves_the_file_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    drop(fk_db(&path));
    let before = std::fs::read(&path).unwrap();
    let mut c = Connection::open(&path).unwrap();
    c.pragma_update(None, "foreign_keys", "ON").unwrap();
    let err = migrate::migrate(&mut c, "test.db", &[FK_V1, FK_DANGLING]).unwrap_err();
    let msg = err.to_string();
    for part in ["test.db", "blobs", "users", "nothing was changed"] {
        assert!(msg.contains(part), "{part:?} missing from: {msg}");
    }
    assert_eq!(foreign_keys(&c), 1, "foreign keys are on again");
    drop(c);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// Review R43-2: the version is only trusted together with the schema. A
/// file at version 1 that lacks an index migration 1 makes (migration 1
/// edited in place before the tag), or has a table no migration makes (a
/// pre-release file, another program's file), is refused unchanged.
#[test]
fn a_store_whose_schema_is_not_its_versions_is_refused_and_left_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    for (case, edit) in [
        ("missing", "DROP INDEX session_workspaces_ws;"),
        ("extra", "CREATE TABLE added (x INTEGER);"),
    ] {
        let path = dir.path().join(format!("{case}-master.db"));
        drop(Store::open(&path, Durability::Normal).unwrap());
        Connection::open(&path)
            .unwrap()
            .execute_batch(edit)
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let err = match Store::open(&path, Durability::Normal) {
            Ok(_) => panic!("{case}: a file that is not schema version 1 was opened"),
            Err(e) => e.to_string(),
        };
        for part in [
            "master.db",
            "does not match schema version 1",
            "not changed",
        ] {
            assert!(err.contains(part), "{case}: {part:?} missing from: {err}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), before, "{case}");
    }
}

/// Review R43-2: a file from before schema versions (version 0) whose
/// tables have an older shape is not stamped version 1.
#[test]
fn an_unversioned_file_with_old_tables_is_refused_and_left_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE nodes (node_id TEXT PRIMARY KEY);")
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let err = match Store::open(&path, Durability::Full) {
        Ok(_) => panic!("an old-shaped file was opened"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("does not match schema version 1"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(version_of(&path), 0);
}

/// Review R43-7: the newer file in rollback-journal mode, made without
/// `Store::open`: the refusal must come before `journal_mode = WAL`, which
/// would rewrite its header.
#[test]
fn a_newer_rollback_journal_store_is_refused_before_any_pragma_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.db");
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE t (x INTEGER); INSERT INTO t VALUES (1);")
            .unwrap();
        c.pragma_update(None, "user_version", MIGRATIONS.len() as i64 + 1)
            .unwrap();
        let mode: String = c
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
    }
    let before = std::fs::read(&path).unwrap();
    assert!(Store::open(&path, Durability::Full).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// Review R43-6: the recorded schema holds what the migrations make, not
/// what SQLite makes beside it (FTS5's shadow tables, automatic indexes),
/// whose text a SQLite update may change with no migration changed.
#[test]
fn the_recorded_schema_leaves_out_what_sqlite_makes() {
    let mut c = Connection::open_in_memory().unwrap();
    migrate::migrate(&mut c, ":memory:", MIGRATIONS).unwrap();
    let text = migrate::schema_text(&c).unwrap();
    for made_by_sqlite in ["artifact_fts_config", "chat_fts_data", "sqlite_autoindex"] {
        assert!(
            !text.contains(made_by_sqlite),
            "{made_by_sqlite} in:\n{text}"
        );
    }
    assert!(text.contains("CREATE VIRTUAL TABLE artifact_fts"), "{text}");
}

/// Review R43-4: the runner says what it did, so a migration can be logged.
#[test]
fn the_runner_reports_the_versions_it_migrated_between() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    drop(open(&path, &[V1]).unwrap());
    let mut c = Connection::open(&path).unwrap();
    let done = migrate::migrate(&mut c, "test.db", &[V1, V2]).unwrap();
    assert_eq!((done.from, done.to), (1, 2));
    let again = migrate::migrate(&mut c, "test.db", &[V1, V2]).unwrap();
    assert_eq!((again.from, again.to), (2, 2));
}

/// Review R43-5: a second opener waits only the busy timeout for the write
/// lock, then is refused with what holds it (not "the schema version").
#[test]
fn a_database_held_by_another_writer_is_refused_as_busy_and_left_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    drop(open(&path, &[V1]).unwrap());
    let before = std::fs::read(&path).unwrap();
    let holder = Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let mut c = Connection::open(&path).unwrap();
    c.pragma_update(None, "busy_timeout", 50).unwrap();
    let err = migrate::migrate(&mut c, "test.db", &[V1, V2]).unwrap_err();
    assert!(matches!(err, MigrateError::Busy { .. }), "{err:?}");
    let msg = err.to_string();
    for part in ["test.db", "another process", "not changed"] {
        assert!(msg.contains(part), "{part:?} missing from: {msg}");
    }
    holder.execute_batch("ROLLBACK;").unwrap();
    drop((holder, c));
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// Review R43-3: the start-up preflight reads a file without changing it:
/// an absent file is fine, a newer one, a negative version and a current
/// version with another schema are refused by name (re-check: only the
/// newer one was tested).
#[test]
fn the_preflight_refuses_a_newer_file_and_passes_an_absent_or_current_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.db");
    migrate::check_file(&path, &harness_store::DB_SCHEMA).unwrap();
    assert!(!path.exists(), "the preflight created the file");
    drop(Store::open(&path, Durability::Full).unwrap());
    migrate::check_file(&path, &harness_store::DB_SCHEMA).unwrap();
    set_version(&path, MIGRATIONS.len() as i64 + 1);
    let err = migrate::check_file(&path, &harness_store::DB_SCHEMA).unwrap_err();
    assert!(matches!(err, MigrateError::TooNew { .. }), "{err:?}");
    assert!(err.to_string().contains("chat.db"), "{err}");
    set_version(&path, -3);
    let err = migrate::check_file(&path, &harness_store::DB_SCHEMA).unwrap_err();
    assert!(
        matches!(err, MigrateError::Unknown { found: -3, .. }),
        "{err:?}"
    );

    let other = dir.path().join("node.db");
    drop(Store::open(&other, Durability::Full).unwrap());
    Connection::open(&other)
        .unwrap()
        .execute_batch("CREATE TABLE added (x INTEGER);")
        .unwrap();
    let before = std::fs::read(&other).unwrap();
    let err = migrate::check_file(&other, &harness_store::DB_SCHEMA).unwrap_err();
    assert!(
        matches!(err, MigrateError::Mismatch { version: 1, .. }),
        "{err:?}"
    );
    assert!(err.to_string().contains("node.db"), "{err}");
    assert_eq!(std::fs::read(&other).unwrap(), before);
}

/// `dir/crash/<name>` and its hot `-journal`: a transaction that wrote
/// every statement of `MIGRATIONS` (a first creation) after `committed`
/// had been committed, cut off after its pages reached the file and
/// before its commit. A small page cache makes the pager sync the journal
/// and spill pages into the file mid-transaction; both files are copied
/// while it is open, which is what a kill at that moment leaves.
fn interrupted_creation(dir: &Path, name: &str, committed: &str) -> std::path::PathBuf {
    let live = dir.join("live");
    std::fs::create_dir_all(&live).unwrap();
    let live_db = live.join(name);
    let c = Connection::open(&live_db).unwrap();
    c.execute_batch(committed).unwrap();
    c.pragma_update(None, "cache_size", 10).unwrap();
    c.execute_batch("BEGIN IMMEDIATE;").unwrap();
    for step in MIGRATIONS {
        for sql in *step {
            c.execute_batch(sql).unwrap();
        }
    }
    let journal = live.join(format!("{name}-journal"));
    assert!(journal.exists(), "no journal");
    assert!(std::fs::metadata(&live_db).unwrap().len() > 0, "no spill");
    let crash = dir.join("crash");
    std::fs::create_dir_all(&crash).unwrap();
    std::fs::copy(&live_db, crash.join(name)).unwrap();
    std::fs::copy(&journal, crash.join(format!("{name}-journal"))).unwrap();
    c.execute_batch("ROLLBACK;").unwrap();
    crash.join(name)
}

/// Re-check D1: a first creation of this build's own file, interrupted
/// (kill -9, OOM, power cut) with a hot rollback journal. A read-only
/// connection cannot roll it back (SQLITE_READONLY_ROLLBACK), so the
/// preflight refused it on every start, although the real open recovers
/// it. The preflight must pass it on, and the real open then rolls the
/// journal back and creates the database.
#[test]
fn the_preflight_passes_on_an_interrupted_creation_and_the_real_open_recovers_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = interrupted_creation(dir.path(), "master.db", "");
    for start in 1..=2 {
        if let Err(e) = migrate::check_file(&path, &harness_store::DB_SCHEMA) {
            panic!("start {start}: the preflight refused a recoverable file: {e}");
        }
    }
    drop(Store::open(&path, Durability::Normal).unwrap());
    assert_eq!(version_of(&path), MIGRATIONS.len() as i64);
    assert!(!dir.path().join("crash/master.db-journal").exists());
    migrate::check_file(&path, &harness_store::DB_SCHEMA).unwrap();
}

/// Re-check D1, the other side: passing a hot journal on is not passing
/// the file. A newer build's file with a hot journal is rolled back by the
/// real open (what the newer build would do itself) and then refused as
/// newer, at its committed version.
#[test]
fn a_newer_file_with_a_hot_journal_is_still_refused_by_the_real_open() {
    let dir = tempfile::tempdir().unwrap();
    let newer = MIGRATIONS.len() as i64 + 1;
    let path = interrupted_creation(
        dir.path(),
        "chat.db",
        &format!("CREATE TABLE t (x INTEGER); PRAGMA user_version = {newer};"),
    );
    migrate::check_file(&path, &harness_store::DB_SCHEMA).unwrap();
    let err = match Store::open(&path, Durability::Full) {
        Ok(_) => panic!("a newer file with a hot journal was opened"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("chat.db") && err.contains("newer"), "{err}");
    assert_eq!(version_of(&path), newer);
}

/// Re-check D4: a reference that dangled before the migration (a row
/// deleted with the sqlite3 shell, whose foreign keys are off) must not be
/// blamed on the migration: the refusal says it was there before, names
/// the tables, and nothing is changed.
#[test]
fn a_reference_that_dangled_before_the_migration_is_named_as_such() {
    const OTHER: Migration = &["CREATE TABLE other (x INTEGER);"];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    {
        let c = fk_db(&path);
        c.pragma_update(None, "foreign_keys", "OFF").unwrap();
        c.execute("DELETE FROM users", []).unwrap();
    }
    let before = std::fs::read(&path).unwrap();
    let mut c = Connection::open(&path).unwrap();
    let err = migrate::migrate(&mut c, "test.db", &[FK_V1, OTHER]).unwrap_err();
    let msg = err.to_string();
    for part in [
        "test.db",
        "blobs",
        "users",
        "before",
        "version 1",
        "not changed",
    ] {
        assert!(msg.contains(part), "{part:?} missing from: {msg}");
    }
    assert!(!msg.contains("left rows"), "blames the migration: {msg}");
    drop(c);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// Re-check of R43-5: on a file in rollback-journal mode (every file
/// until its first `Store::open` ends) a committing writer holds the file
/// exclusively, so the version read itself times out, before `BEGIN`. That
/// is the same condition and gets the same refusal, in the runner and in
/// the preflight.
#[test]
fn a_rollback_journal_file_held_by_a_committing_writer_is_refused_as_busy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    {
        // Not `open`: this file stays in rollback-journal mode.
        let mut c = Connection::open(&path).unwrap();
        migrate::migrate(&mut c, "test.db", &[V1]).unwrap();
    }
    let holder = Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let mut c = Connection::open(&path).unwrap();
    c.pragma_update(None, "busy_timeout", 50).unwrap();
    let err = migrate::migrate(&mut c, "test.db", &[V1, V2]).unwrap_err();
    assert!(matches!(err, MigrateError::Busy { .. }), "{err:?}");
    let err = migrate::check_file(&path, &harness_store::DB_SCHEMA).unwrap_err();
    assert!(matches!(err, MigrateError::Busy { .. }), "{err:?}");
    assert!(err.to_string().contains("another process"), "{err}");
    holder.execute_batch("ROLLBACK;").unwrap();
}

/// The branch under the write lock: the version read before `BEGIN` is
/// behind, another process migrates and commits while this one waits for
/// the lock, and the re-read finds the file migrated, so nothing is run
/// again (V2's ALTER would fail a second time). A busy handler says when
/// this one is waiting, so the order is not left to a sleep.
#[test]
fn a_file_migrated_by_another_process_while_this_one_waited_is_not_migrated_again() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    drop(open(&path, &[V1]).unwrap());
    let other = Connection::open(&path).unwrap();
    other.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let waiting = Arc::new(AtomicBool::new(false));
    let mut c = Connection::open(&path).unwrap();
    let flag = waiting.clone();
    c.busy_handler(Some(move |_| {
        flag.store(true, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(5));
        true
    }))
    .unwrap();
    let this = std::thread::spawn(move || migrate::migrate(&mut c, "test.db", &[V1, V2]));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !waiting.load(Ordering::SeqCst) {
        assert!(std::time::Instant::now() < deadline, "never waited");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    for sql in V2 {
        other.execute_batch(sql).unwrap();
    }
    other.pragma_update(None, "user_version", 2).unwrap();
    other.execute_batch("COMMIT;").unwrap();
    let done = this.join().unwrap().unwrap();
    assert_eq!((done.from, done.to), (2, 2));
    assert_eq!(version_of(&path), 2);
}

/// Owner decision (b): a version-0 file that already has tables is
/// adopted as version 1 only when its schema is exactly version 1's.
#[test]
fn an_unversioned_file_with_exactly_version_1s_schema_is_adopted_as_version_1() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("master.db");
    {
        let c = Connection::open(&path).unwrap();
        for sql in MIGRATIONS[0] {
            c.execute_batch(sql).unwrap();
        }
        c.execute_batch(
            "INSERT INTO users (user_id, username, password_hash, role, created_ms)
                 VALUES ('u1', 'ann', 'x', 'admin', 1);",
        )
        .unwrap();
    }
    assert_eq!(version_of(&path), 0);
    drop(Store::open(&path, Durability::Normal).unwrap());
    assert_eq!(version_of(&path), 1);
    let users: i64 = Connection::open(&path)
        .unwrap()
        .query_row("SELECT count(*) FROM users", [], |r| r.get(0))
        .unwrap();
    assert_eq!(users, 1);
}

/// Owner decision (b), the other side: a version-0 file whose tables are
/// not version 1's is refused as a mismatch with the advice to remove the
/// data directory, also when migration 1 itself would trip on those
/// tables (here an index on a column the old `sessions` lacks), which used
/// to be reported as a failed migration.
#[test]
fn an_unversioned_file_whose_tables_break_migration_1_is_refused_as_a_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE sessions (session_id TEXT PRIMARY KEY);")
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let err = match Store::open(&path, Durability::Full) {
        Ok(_) => panic!("an old-shaped file was opened"),
        Err(e) => e.to_string(),
    };
    for part in [
        "chat.db",
        "does not match schema version 1",
        "not changed",
        "remove this data directory",
    ] {
        assert!(err.contains(part), "{part:?} missing from: {err}");
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(version_of(&path), 0);
}

/// Review R43-4 (re-check: no test would miss the log lines): a real
/// migration is logged before and after, naming the file and both
/// versions; a creation (0 to the latest) and a failed one's "after" are
/// not.
#[test]
fn a_real_migration_is_logged_before_and_after_and_a_creation_is_not() {
    use std::sync::{Arc, Mutex};
    struct Capture(Arc<Mutex<Vec<String>>>);
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
            struct Fields(String);
            impl tracing::field::Visit for Fields {
                fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                    self.0.push_str(&format!("{}={:?} ", f.name(), v));
                }
            }
            let mut line = Fields(format!("{} ", event.metadata().level()));
            event.record(&mut line);
            self.0.lock().unwrap().push(line.0);
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }
    let events = Arc::new(Mutex::new(Vec::new()));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("node.db");
    tracing::subscriber::with_default(Capture(events.clone()), || {
        let mut c = Connection::open(&path).unwrap();
        migrate::migrate(&mut c, "node.db", &[V1]).unwrap();
        assert_eq!(events.lock().unwrap().len(), 0, "a creation was logged");
        migrate::migrate(&mut c, "node.db", &[V1, V2]).unwrap();
        let lines = events.lock().unwrap().clone();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].contains("migrating the schema"), "{lines:?}");
        assert!(lines[1].contains("schema migrated"), "{lines:?}");
        for l in &lines {
            assert!(
                l.starts_with("INFO")
                    && l.contains("node.db")
                    && l.contains("from=1")
                    && l.contains("to=2"),
                "{l}"
            );
        }
        migrate::migrate(&mut c, "node.db", &[V1, V2, BROKEN]).unwrap_err();
        let lines = events.lock().unwrap().clone();
        assert_eq!(lines.len(), 3, "a failed migration logs its start only: {lines:?}");
    });
}

/// Review R43-3 (untested claim): a newer build that died with its WAL not
/// checkpointed. Refusing it must not checkpoint that WAL into the file, as
/// closing the last read-write connection would.
#[test]
fn the_preflight_leaves_a_crashed_newer_builds_wal_alone() {
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
    // Copied while the connection is open: the files a crash leaves.
    for ext in ["", "-wal", "-shm"] {
        std::fs::copy(
            dir.path().join(format!("live.db{ext}")),
            crashed.join(format!("node.db{ext}")),
        )
        .unwrap();
    }
    drop(c);
    let path = crashed.join("node.db");
    let wal = crashed.join("node.db-wal");
    let (db_before, wal_before) = (std::fs::read(&path).unwrap(), std::fs::read(&wal).unwrap());
    assert!(!wal_before.is_empty());
    let err = migrate::check_file(&path, &harness_store::DB_SCHEMA).unwrap_err();
    assert!(matches!(err, MigrateError::TooNew { .. }), "{err:?}");
    assert_eq!(std::fs::read(&path).unwrap(), db_before);
    assert_eq!(std::fs::read(&wal).unwrap(), wal_before);
}

/// Review R43-6: write mode records a version that has no file yet and
/// never rewrites a recorded one (before the tag, 1.sql is deleted first).
#[test]
fn write_mode_creates_missing_files_and_never_rewrites_a_recorded_one() {
    let dir = tempfile::tempdir().unwrap();
    migrate::check_recorded_with(dir.path(), &[V1], true);
    let one = std::fs::read_to_string(dir.path().join("1.sql")).unwrap();
    assert!(one.contains("CREATE TABLE t"), "{one}");
    std::fs::write(dir.path().join("1.sql"), "-- a released version 1\n").unwrap();
    let rewrote = std::panic::catch_unwind(|| {
        migrate::check_recorded_with(dir.path(), &[V1, V2], true);
    });
    assert!(rewrote.is_err(), "a changed version 1 passed in write mode");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("1.sql")).unwrap(),
        "-- a released version 1\n"
    );
    assert!(
        !dir.path().join("2.sql").exists(),
        "nothing is written after a mismatch"
    );
}

/// Every version's schema as recorded in `crates/store/schema/<k>.sql`. A
/// migration that is already released must never change: a database made
/// with it would no longer match what the next one expects. Change the
/// schema with a new migration and record its file
/// (`HARNESS_WRITE_SCHEMA=1 cargo test -p harness-store --test migrations`,
/// which only creates files that do not exist). Before the first tag,
/// migration 1 is still edited in place (PLAN.md §0): delete `1.sql` and
/// record it again, and wipe dev data directories.
#[test]
fn every_version_matches_its_recorded_schema() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema");
    migrate::check_recorded(&dir, MIGRATIONS);
    // What `Store::open` compares against is the last recorded version.
    assert_eq!(
        harness_store::DB_SCHEMA.recorded,
        migrate::built(MIGRATIONS),
        "DB_SCHEMA must include_str! schema/{}.sql",
        MIGRATIONS.len()
    );
}
