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
//!   transaction). A second opener waits for the write lock only as long
//!   as its busy timeout (5 s in `Store` and `SqliteMemory`) and is then
//!   refused as `Busy`, unchanged; one that gets the lock re-reads the
//!   version and finds it migrated. What keeps a second process off the
//!   data directory is its lock (A.20), not this.
//! - Foreign keys are off while migrations run, so a table rebuilt the way
//!   SQLite documents it keeps the rows that reference it, and
//!   `foreign_key_check` must find nothing before the version is bumped.
//!   The connection's own setting is put back afterwards.
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
    #[error(
        "{db}: migrating the schema from version {from} to {to} left rows of {table} pointing at missing rows of {parent}; nothing was changed"
    )]
    Dangling {
        db: String,
        from: i64,
        to: i64,
        table: String,
        parent: String,
    },
    /// The version is one this build knows, the schema is not that
    /// version's: a file from before the first release, another kind of
    /// database (a memory.db in place of a node.db), or one edited by hand.
    #[error(
        "{db} does not match schema version {version} (a database from before the first release, \
         another kind of file, or one changed by hand). It was not changed: before the first \
         release a schema change needs a fresh data directory"
    )]
    Mismatch { db: String, version: i64 },
    /// The write lock was not free within the busy timeout.
    #[error(
        "{db} needs a schema migration, but another process holds it for writing (waited the busy timeout). \
         It was not changed: stop the other process and start again"
    )]
    Busy { db: String },
    #[error("{db}: reading or writing the schema version: {source}")]
    Sqlite {
        db: String,
        #[source]
        source: rusqlite::Error,
    },
}

/// What `migrate` did: `from == to` when nothing was to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migrated {
    pub from: i64,
    pub to: i64,
}

/// A database kind: its migrations and the schema the last one leaves
/// (`schema/<n>.sql`, recorded and checked by `check_recorded`).
pub struct Schema<'a> {
    pub steps: &'a [Migration],
    pub recorded: &'a str,
}

/// `found` as a version this build can open or migrate, or the refusal.
fn refuse(db: &str, found: i64, known: i64) -> Result<i64, MigrateError> {
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
}

/// The schema of `conn` is `expected`, or the refusal for `version`.
fn same_schema(conn: &Connection, db: &str, version: i64, expected: &str) -> Result<(), MigrateError> {
    let now = schema_text(conn).map_err(|source| MigrateError::Sqlite {
        db: db.to_string(),
        source,
    })?;
    if now == expected {
        Ok(())
    } else {
        Err(MigrateError::Mismatch {
            db: db.to_string(),
            version,
        })
    }
}

fn user_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
}

/// Bring `conn` to `schema`'s last version, or refuse. `db` names the file
/// in the error (a path or a file name; never anything secret). The
/// version is trusted only with the schema (review R43-2): an up-to-date
/// file must have exactly the recorded schema, and a migrated one must end
/// with it before its version is bumped, or it is refused unchanged.
pub fn migrate_checked(
    conn: &mut Connection,
    db: &str,
    schema: &Schema,
) -> Result<Migrated, MigrateError> {
    apply(conn, db, schema.steps, Some(schema.recorded))
}

/// `migrate_checked` without the schema comparison: for building the
/// recorded schema and for the runner's own tests.
pub fn migrate(
    conn: &mut Connection,
    db: &str,
    steps: &[Migration],
) -> Result<Migrated, MigrateError> {
    apply(conn, db, steps, None)
}

fn apply(
    conn: &mut Connection,
    db: &str,
    steps: &[Migration],
    expected: Option<&str>,
) -> Result<Migrated, MigrateError> {
    let known = steps.len() as i64;
    let sqlite = |source: rusqlite::Error| MigrateError::Sqlite {
        db: db.to_string(),
        source,
    };
    // Read first, outside any write: an up-to-date or newer file is never
    // written to (a read-only disk still opens an up-to-date one).
    if refuse(db, user_version(conn).map_err(sqlite)?, known)? == known {
        if let Some(expected) = expected {
            same_schema(conn, db, known, expected)?;
        }
        return Ok(Migrated {
            from: known,
            to: known,
        });
    }
    // Foreign keys off while migrations run (review R43-1): a table change
    // the way SQLite documents it (new table, copy, drop, rename) would
    // otherwise cascade the DROP into every child row. The pragma is a
    // no-op inside a transaction, so it is set before BEGIN, and put back
    // whatever happens; `foreign_key_check` stands in for it before commit.
    let fk: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .map_err(sqlite)?;
    conn.pragma_update(None, "foreign_keys", "OFF")
        .map_err(sqlite)?;
    let result = run(conn, db, steps, expected);
    let restored = conn.pragma_update(None, "foreign_keys", fk);
    let done = result?;
    restored.map_err(sqlite)?;
    Ok(done)
}

/// The migration transaction itself; foreign keys are off.
fn run(
    conn: &mut Connection,
    db: &str,
    steps: &[Migration],
    expected: Option<&str>,
) -> Result<Migrated, MigrateError> {
    let known = steps.len() as i64;
    let sqlite = |source: rusqlite::Error| MigrateError::Sqlite {
        db: db.to_string(),
        source,
    };
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|source| match source.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
                MigrateError::Busy { db: db.to_string() }
            }
            _ => sqlite(source),
        })?;
    // Again under the write lock: another process may have migrated it.
    let from = refuse(db, user_version(&tx).map_err(sqlite)?, known)?;
    if from == known {
        // Migrated by another process while this one waited for the lock.
        if let Some(expected) = expected {
            same_schema(&tx, db, known, expected)?;
        }
        return Ok(Migrated { from, to: known });
    }
    // A fresh file (version 0) is a creation, not worth a line; a real
    // migration is: from its commit on, an older build refuses the file,
    // and a long one should not look like a hang.
    let logged = from != 0 && from != known;
    let started = std::time::Instant::now();
    if logged {
        tracing::info!(
            db,
            from,
            to = known,
            "migrating the schema; once done, older builds refuse this file"
        );
    }
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
    if let Some(expected) = expected {
        same_schema(&tx, db, known, expected)?;
    }
    if let Some((table, parent)) = dangling(&tx).map_err(sqlite)? {
        return Err(MigrateError::Dangling {
            db: db.to_string(),
            from,
            to: known,
            table,
            parent,
        });
    }
    // Dropping `tx` on any error above rolls everything back.
    tx.pragma_update(None, "user_version", known)
        .map_err(sqlite)?;
    tx.commit().map_err(sqlite)?;
    if logged {
        tracing::info!(
            db,
            from,
            to = known,
            ms = started.elapsed().as_millis() as u64,
            "schema migrated"
        );
    }
    Ok(Migrated { from, to: known })
}

/// The first row `PRAGMA foreign_key_check` reports: (child, parent table).
fn dangling(conn: &Connection) -> rusqlite::Result<Option<(String, String)>> {
    let mut stmt = conn.prepare("PRAGMA foreign_key_check")?;
    let mut rows = stmt.query([])?;
    match rows.next()? {
        Some(r) => Ok(Some((r.get(0)?, r.get(2)?))),
        None => Ok(None),
    }
}

/// The tables FTS5 makes for each of its virtual tables (`<name>_<suffix>`).
const FTS5_SHADOWS: [&str; 5] = ["config", "content", "data", "docsize", "idx"];

/// The schema of `conn` as text: every object the migrations made, from
/// `sqlite_master`, sorted by name. What SQLite makes beside them is left
/// out (review R43-6): its own `sqlite_*` objects (automatic indexes,
/// `sqlite_sequence`, `sqlite_stat*`) and FTS5's shadow tables. Their text
/// follows from the statements recorded here, and a SQLite update may
/// change it without any migration changing.
pub fn schema_text(conn: &Connection) -> rusqlite::Result<String> {
    let mut stmt = conn.prepare(
        "SELECT type, name, tbl_name, sql FROM sqlite_master
         WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
         ORDER BY name, type",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let fts5: Vec<&str> = rows
        .iter()
        .filter(|(kind, _, _, sql)| {
            kind == "table" && sql.starts_with("CREATE VIRTUAL TABLE") && sql.contains("fts5")
        })
        .map(|(_, name, _, _)| name.as_str())
        .collect();
    let shadow = |table: &str| {
        fts5.iter().any(|v| {
            table
                .strip_prefix(v)
                .and_then(|rest| rest.strip_prefix('_'))
                .is_some_and(|suffix| FTS5_SHADOWS.contains(&suffix))
        })
    };
    let mut out = String::new();
    for (kind, name, table, sql) in &rows {
        if shadow(table) {
            continue;
        }
        out.push_str(&format!("-- {kind} {name} on {table}\n{sql};\n\n"));
    }
    Ok(out)
}

/// The schema migrations `steps` build on an empty database.
#[doc(hidden)]
pub fn built(steps: &[Migration]) -> String {
    let mut conn = Connection::open_in_memory().unwrap();
    migrate(&mut conn, ":memory:", steps).unwrap();
    schema_text(&conn).unwrap()
}

/// Test support (store and memory tests): version `k`'s schema, built from
/// migrations `1..=k`, equals `dir/<k>.sql` for every `k`, and no file
/// exists for a version the code does not have. With
/// `HARNESS_WRITE_SCHEMA=1` a version without a file gets one; a recorded
/// file is never rewritten (before the first tag, delete `1.sql` to record
/// migration 1 again).
#[doc(hidden)]
pub fn check_recorded(dir: &Path, steps: &[Migration]) {
    let write = std::env::var_os("HARNESS_WRITE_SCHEMA").is_some_and(|v| v == "1");
    check_recorded_with(dir, steps, write);
}

/// `check_recorded` with write mode given, for its own tests.
#[doc(hidden)]
pub fn check_recorded_with(dir: &Path, steps: &[Migration], write: bool) {
    for k in 1..=steps.len() {
        let file = dir.join(format!("{k}.sql"));
        let now = built(&steps[..k]);
        if write && !file.exists() {
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
}
