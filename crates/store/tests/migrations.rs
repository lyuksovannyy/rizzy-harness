//! Schema versions (fix plan 4.3): `PRAGMA user_version` and numbered
//! migrations for `master.db`, `chat.db` and `node.db` (`Store`). memory.db
//! has the same tests in `crates/memory/tests/memory.rs`.

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

/// Every version's schema as recorded in `crates/store/schema/<k>.sql`. A
/// migration that is already released must never change: a database made
/// with it would no longer match what the next one expects. Change the
/// schema with a new migration and record its file
/// (`HARNESS_WRITE_SCHEMA=1 cargo test -p harness-store --test migrations`).
/// Before the first tag, migration 1 is still edited in place (PLAN.md §0)
/// and its file rewritten the same way.
#[test]
fn every_version_matches_its_recorded_schema_and_migrating_matches_fresh() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema");
    migrate::check_recorded(&dir, MIGRATIONS);
}
