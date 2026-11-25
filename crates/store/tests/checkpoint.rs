//! `Store::checkpoint` beside another program that reads the database (a
//! backup, the sqlite3 shell): it must not hold the store's only connection
//! while it waits for that reader, and a truncate it could not make must
//! not pass as made.

use std::time::{Duration, Instant};

use harness_store::{Durability, Store};
use rusqlite::{Connection, OpenFlags};

fn wal_len(db: &std::path::Path) -> u64 {
    let mut p = db.as_os_str().to_owned();
    p.push("-wal");
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// An outside read transaction open on the file: the TRUNCATE checkpoint
/// cannot finish. It answers at once (not after the 5 s busy timeout, during
/// which every other call on this store would wait), says the truncate was
/// not made, and remembers it; once the reader is gone the retry truncates.
#[test]
fn a_checkpoint_beside_an_outside_reader_neither_waits_nor_hides_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("master.db");
    let s = Store::open(&db, Durability::Normal).unwrap();
    s.kv_set("a", "1").unwrap();
    let reader = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    reader.execute_batch("BEGIN").unwrap();
    let n: i64 = reader
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
        .unwrap();
    assert!(n > 0);
    // Written after the reader's snapshot: the WAL cannot be reset under it.
    s.kv_set("b", "2").unwrap();
    assert!(wal_len(&db) > 0);

    let t = Instant::now();
    let truncated = s.checkpoint().unwrap();
    let took = t.elapsed();
    // Far below the 5 s busy timeout the old code waited out.
    assert!(took < Duration::from_secs(2), "checkpoint waited {took:?}");
    assert!(!truncated, "the truncate could not be made and must say so");
    assert!(wal_len(&db) > 0);
    assert!(s.truncate_pending());
    let mark = dir.path().join("master.db-truncate-pending");
    assert!(mark.exists(), "doctor has nothing to report");

    drop(reader);
    assert!(s.retry_truncate().unwrap(), "no reader left: it truncates");
    assert_eq!(wal_len(&db), 0);
    assert!(!s.truncate_pending());
    assert!(!mark.exists());
    // Nothing pending: the retry does nothing and says it is done.
    assert!(s.retry_truncate().unwrap());
}

/// Without a reader nothing changes: truncated at once, nothing pending.
#[test]
fn a_checkpoint_alone_truncates() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("master.db");
    let s = Store::open(&db, Durability::Normal).unwrap();
    s.kv_set("a", "1").unwrap();
    assert!(s.checkpoint().unwrap());
    assert_eq!(wal_len(&db), 0);
    assert!(!s.truncate_pending());
}

/// A mark left by an earlier process (a truncate pending when it stopped):
/// the store opened again takes it as pending, so the periodic retry makes
/// the truncate and removes the mark, without waiting for a delete.
#[test]
fn a_mark_left_before_a_restart_is_retried() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("chat.db");
    drop(Store::open(&db, Durability::Full).unwrap());
    let mark = dir.path().join("chat.db-truncate-pending");
    std::fs::write(&mark, b"").unwrap();
    let s = Store::open(&db, Durability::Full).unwrap();
    assert!(s.truncate_pending(), "the mark was not taken as pending");
    assert!(s.retry_truncate().unwrap());
    assert!(!mark.exists());
}
