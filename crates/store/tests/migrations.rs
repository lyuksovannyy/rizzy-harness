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
const FK_REBUILD: Migration = &["CREATE TABLE users_new (id TEXT PRIMARY KEY, name TEXT NOT NULL);
     INSERT INTO users_new SELECT id, name FROM users;
     DROP TABLE users;
     ALTER TABLE users_new RENAME TO users;"];
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
        Connection::open(&path).unwrap().execute_batch(edit).unwrap();
        let before = std::fs::read(&path).unwrap();
        let err = match Store::open(&path, Durability::Normal) {
            Ok(_) => panic!("{case}: a file that is not schema version 1 was opened"),
            Err(e) => e.to_string(),
        };
        for part in ["master.db", "does not match schema version 1", "not changed"] {
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
    assert!(!dir.path().join("chat.db-wal").exists());
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
        assert!(!text.contains(made_by_sqlite), "{made_by_sqlite} in:\n{text}");
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

/// Review R43-3: the start-up preflight reads a file without writing it:
/// an absent file is fine, a newer one is refused by name.
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
    assert!(!dir.path().join("2.sql").exists(), "nothing is written after a mismatch");
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
}
