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

/// Review R43-2: a memory.db at version 1 with a table no migration makes
/// is refused unchanged.
#[test]
fn a_memory_db_whose_schema_is_not_its_versions_is_refused_and_left_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.db");
    drop(SqliteMemory::open(&path).unwrap());
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE added (x INTEGER);")
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let err = match SqliteMemory::open(&path) {
        Ok(_) => panic!("a memory.db that is not schema version 1 was opened"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("memory.db") && err.contains("does not match schema version 1"),
        "{err}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// Review R43-2 (and T41's residual): a memory.db copied in place of a
/// node.db carries a valid version, but not the node's schema. Neither
/// file kind opens as the other.
#[test]
fn a_memory_db_and_a_node_db_do_not_open_as_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let node = dir.path().join("node.db");
    let memory = dir.path().join("memory.db");
    drop(SqliteMemory::open(&node).unwrap());
    drop(harness_store::Store::open(&memory, harness_store::Durability::Full).unwrap());
    for path in [&node, &memory] {
        let before = std::fs::read(path).unwrap();
        let err = if path == &node {
            harness_store::Store::open(path, harness_store::Durability::Full)
                .err()
                .map(|e| e.to_string())
        } else {
            SqliteMemory::open(path).err().map(|e| e.to_string())
        };
        let err = err.unwrap_or_else(|| panic!("{} opened as the other kind", path.display()));
        assert!(err.contains("does not match schema version 1"), "{err}");
        assert_eq!(&std::fs::read(path).unwrap(), &before);
    }
}

/// See `every_version_matches_its_recorded_schema` in the store's tests:
/// `crates/memory/schema/<k>.sql` per version.
#[test]
fn every_version_matches_its_recorded_schema() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema");
    harness_store::migrate::check_recorded(&dir, MIGRATIONS);
    assert_eq!(
        harness_memory::DB_SCHEMA.recorded,
        harness_store::migrate::built(MIGRATIONS),
        "DB_SCHEMA must include_str! schema/{}.sql",
        MIGRATIONS.len()
    );
}
