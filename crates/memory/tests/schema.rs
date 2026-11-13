//! Schema versions of `memory.db` (fix plan 4.3); the runner itself is
//! tested in `crates/store/tests/migrations.rs`.

use std::path::Path;

use harness_memory::{MIGRATIONS, SqliteMemory};
use rusqlite::Connection;

fn version_of(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn a_fresh_memory_db_carries_the_latest_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.db");
    drop(SqliteMemory::open(&path).unwrap());
    assert_eq!(version_of(&path), MIGRATIONS.len() as i64);
}

#[test]
fn a_memory_db_from_a_newer_build_is_refused_and_left_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.db");
    drop(SqliteMemory::open(&path).unwrap());
    let newer = MIGRATIONS.len() as i64 + 1;
    Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", newer)
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let err = match SqliteMemory::open(&path) {
        Ok(_) => panic!("memory.db from a newer build was opened"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("memory.db") && err.contains("newer"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(version_of(&path), newer);
}

/// See `every_version_matches_its_recorded_schema_and_migrating_matches_fresh`
/// in the store's tests: `crates/memory/schema/<k>.sql` per version.
#[test]
fn every_version_matches_its_recorded_schema() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema");
    harness_store::migrate::check_recorded(&dir, MIGRATIONS);
}
