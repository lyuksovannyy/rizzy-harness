//! Schema versions (fix plan 4.3): `PRAGMA user_version` and numbered
//! migrations, for every database the harness keeps: `master.db`, `chat.db`
//! and `node.db` (`Store`, one schema) and `memory.db` (`SqliteMemory`).
//!
//! Migration `k` (1-based) takes a database from version `k - 1` to `k`.
//! Migration 1 is the whole schema; a fresh file is version 0 and gets all of
//! them. Before the first tag migration 1 is still edited in place and dev
//! data directories are wiped (PLAN.md §0); from the tag on a released
//! migration never changes, which `check_recorded` enforces against the
//! schema recorded per version in the crate's `schema/` folder.
//!
//! Rules this keeps:
//! - A database from a newer build (a version this build does not know) is
//!   refused before anything is written, so the newer build can still open
//!   it. An older build cannot know what a newer column or table means (a
//!   newer restriction would be ignored), so it must not guess.
//! - All pending migrations and the version bump are one `IMMEDIATE`
//!   transaction: a failure, a full disk or a crash leaves the database as
//!   it was (the version is in the header page, written in the same
//!   transaction), and a second process waits for the write lock and then
//!   sees the new version.
//! - An up-to-date database is only read, never written, on open.

use std::path::Path;

use rusqlite::{Connection, TransactionBehavior};

/// One schema step: SQL batches run in order, inside the migration's
/// transaction.
pub type Migration = &'static [&'static str];

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error(
        "{db} was written by a newer rizzy-harness (schema version {found}; this build knows up to {known}). \
         It was not changed: run that build or a newer one"
    )]
    TooNew { db: String, found: i64, known: i64 },
    #[error("{db} has schema version {found}, which no rizzy-harness writes. It was not changed")]
    Unknown { db: String, found: i64 },
    #[error(
        "{db}: migrating the schema from version {from} to {to} failed in migration {step}; nothing was changed: {source}"
    )]
    Failed {
        db: String,
        from: i64,
        to: i64,
        step: i64,
        #[source]
        source: rusqlite::Error,
    },
    #[error("{db}: reading or writing the schema version: {source}")]
    Sqlite {
        db: String,
        #[source]
        source: rusqlite::Error,
    },
}

fn user_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
}

/// Bring `conn` to version `steps.len()`, or refuse. `db` names the file in
/// the error (a path or a file name; never anything secret).
pub fn migrate(conn: &mut Connection, db: &str, steps: &[Migration]) -> Result<(), MigrateError> {
    let known = steps.len() as i64;
    let sqlite = |source: rusqlite::Error| MigrateError::Sqlite {
        db: db.to_string(),
        source,
    };
    let refuse = |found: i64| {
        if found < 0 {
            Err(MigrateError::Unknown {
                db: db.to_string(),
                found,
            })
        } else if found > known {
            Err(MigrateError::TooNew {
                db: db.to_string(),
                found,
                known,
            })
        } else {
            Ok(found)
        }
    };
    // Read first, outside any write: an up-to-date or newer file is never
    // written to (a read-only disk still opens an up-to-date one).
    if refuse(user_version(conn).map_err(sqlite)?)? == known {
        return Ok(());
    }
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite)?;
    // Again under the write lock: another process may have migrated it.
    let from = refuse(user_version(&tx).map_err(sqlite)?)?;
    for (i, step) in steps.iter().enumerate().skip(from as usize) {
        for sql in *step {
            tx.execute_batch(sql)
                .map_err(|source| MigrateError::Failed {
                    db: db.to_string(),
                    from,
                    to: known,
                    step: i as i64 + 1,
                    source,
                })?;
        }
    }
    // Dropping `tx` on any error above rolls everything back.
    tx.pragma_update(None, "user_version", known)
        .map_err(sqlite)?;
    tx.commit().map_err(sqlite)
}

/// The schema of `conn` as text: every object in `sqlite_master` (FTS5's
/// shadow tables and automatic indexes included), sorted by name.
pub fn schema_text(conn: &Connection) -> rusqlite::Result<String> {
    let mut stmt =
        conn.prepare("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY name, type")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut out = String::new();
    for row in rows {
        let (kind, name, table, sql) = row?;
        out.push_str(&format!("-- {kind} {name} on {table}\n"));
        match sql {
            Some(sql) => out.push_str(&format!("{sql};\n\n")),
            None => out.push_str("-- (created by SQLite)\n\n"),
        }
    }
    Ok(out)
}

fn built(steps: &[Migration]) -> String {
    let mut conn = Connection::open_in_memory().unwrap();
    migrate(&mut conn, ":memory:", steps).unwrap();
    schema_text(&conn).unwrap()
}

/// Test support (store and memory tests): version `k`'s schema, built from
/// migrations `1..=k`, equals `dir/<k>.sql` for every `k`, no file exists
/// for a version the code does not have, and a database migrated from any
/// earlier version ends with the same schema as a fresh one. With
/// `HARNESS_WRITE_SCHEMA=1` the files are written instead of compared.
#[doc(hidden)]
pub fn check_recorded(dir: &Path, steps: &[Migration]) {
    let write = std::env::var_os("HARNESS_WRITE_SCHEMA").is_some_and(|v| v == "1");
    for k in 1..=steps.len() {
        let file = dir.join(format!("{k}.sql"));
        let now = built(&steps[..k]);
        if write {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(&file, &now).unwrap();
            continue;
        }
        let recorded = std::fs::read_to_string(&file).unwrap_or_else(|e| {
            panic!(
                "{}: {e} (record it with HARNESS_WRITE_SCHEMA=1)",
                file.display()
            )
        });
        assert!(
            recorded == now,
            "version {k} no longer builds the schema recorded in {}. A released migration \
             must not change: add a new one instead. Built now:\n{now}",
            file.display()
        );
    }
    let next = dir.join(format!("{}.sql", steps.len() + 1));
    assert!(!next.exists(), "{} has no migration", next.display());
    let fresh = built(steps);
    for k in 1..steps.len() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, ":memory:", &steps[..k]).unwrap();
        migrate(&mut conn, ":memory:", steps).unwrap();
        assert_eq!(
            schema_text(&conn).unwrap(),
            fresh,
            "migrating from version {k} does not give the fresh schema"
        );
    }
}
