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
//!   transaction). A second opener waits for the file only as long as its
//!   busy timeout (5 s in `Store` and `SqliteMemory`) and is then refused
//!   as `Busy`, unchanged, wherever the lock hits it (the version read,
//!   `BEGIN`, the commit); one that gets the lock re-reads the version and
//!   finds it migrated. What keeps a second process off the data directory
//!   is its lock (A.20), not this.
//! - Foreign keys are off while migrations run, so a table rebuilt the way
//!   SQLite documents it keeps the rows that reference it, and
//!   `foreign_key_check` must find nothing before the first step (else the
//!   reference dangled before, `DanglingBefore`) and before the version is
//!   bumped (else the migration left it, `Dangling`). The connection's own
//!   setting is put back afterwards.
//! - A version-0 file that already has tables (from before schema
//!   versions) is adopted as version 1 only when its schema is exactly
//!   version 1's, and is refused as `Mismatch` otherwise; migration 1 is
//!   not run over tables it did not make.
//! - An up-to-date database is only read, never written, on open.

use std::path::Path;

use rusqlite::{Connection, ErrorCode, OpenFlags, TransactionBehavior, ffi};

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
        "{db}: migrating the schema from version {from} to {to} failed in migration {step}; nothing was changed \
         in this file (a database migrated before it in this start stays migrated, and older builds refuse it): {source}"
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
    /// The reference was dangling before any migration ran (a row removed
    /// by hand with foreign keys off, or a bug): not the migration's doing.
    #[error(
        "{db}: rows of {table} already point at missing rows of {parent}, before any migration from \
         schema version {from} to {to} ran (a row removed by hand with foreign keys off, or a bug). \
         It was not changed: `PRAGMA foreign_key_check` in the sqlite3 shell lists those rows; \
         repair or remove them and start again"
    )]
    DanglingBefore {
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
         another kind of file, or one changed by hand). It was not changed. Before the first \
         release a schema change is not migrated: remove this data directory (or move it aside) \
         and start again"
    )]
    Mismatch { db: String, version: i64 },
    /// The file was locked past the busy timeout: for the version read
    /// (a writer committing to a rollback-journal file holds it
    /// exclusively), for `BEGIN IMMEDIATE`, or for the commit.
    #[error(
        "{db} is held by another process for writing (waited the busy timeout), so its schema version \
         could not be checked or migrated. It was not changed: stop the other process and start again"
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

/// SQLite's error as a refusal of `db`: a lock not free within the busy
/// timeout is `Busy` wherever it hits (review R43-5 and its re-check), the
/// rest is `Sqlite`.
fn sqlite_error(db: &str, source: rusqlite::Error) -> MigrateError {
    match source.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => {
            MigrateError::Busy { db: db.to_string() }
        }
        _ => MigrateError::Sqlite {
            db: db.to_string(),
            source,
        },
    }
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
fn same_schema(
    conn: &Connection,
    db: &str,
    version: i64,
    expected: &str,
) -> Result<(), MigrateError> {
    let now = schema_text(conn).map_err(|e| sqlite_error(db, e))?;
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

/// Start-up preflight (review R43-3): `path` is absent, or at a version
/// this build can open with that version's schema. Read through a
/// read-only connection, so the database file is not changed, not even by
/// a checkpoint of a crashed newer build's WAL on close (beside a WAL
/// database SQLite may leave an empty `-wal` and a `-shm`: re-check D2).
/// Run for every database of the data directory before any is opened, so
/// a newer file found last does not leave the ones before it migrated.
///
/// A file a read-only connection cannot judge, because SQLite must first
/// roll back a hot journal or recover a WAL (an interrupted creation or
/// commit: re-check D1), is passed on: the real open recovers it, as it
/// always did, and then applies the same version and schema checks. For a
/// newer build's file that recovery is the one the newer build would make
/// itself, and the file is then refused at its committed version.
pub fn check_file(path: &Path, schema: &Schema) -> Result<(), MigrateError> {
    if !path.exists() {
        return Ok(());
    }
    let db = path.display().to_string();
    match judge(path, &db, schema) {
        Err(MigrateError::Sqlite { source, .. }) if needs_recovery(&source) => {
            tracing::info!(
                db,
                "a read-only check cannot read this file before SQLite recovers it; \
                 the open that follows recovers and checks it"
            );
            Ok(())
        }
        other => other,
    }
}

fn judge(path: &Path, db: &str, schema: &Schema) -> Result<(), MigrateError> {
    let sqlite = |e| sqlite_error(db, e);
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(sqlite)?;
    let known = schema.steps.len() as i64;
    if refuse(db, user_version(&conn).map_err(sqlite)?, known)? == known {
        same_schema(&conn, db, known, schema.recorded)?;
    }
    Ok(())
}

/// A read-only connection's refusal to read a file that needs a writer
/// first: a hot rollback journal to roll back, or a WAL to recover.
fn needs_recovery(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _) if matches!(
            f.extended_code,
            ffi::SQLITE_READONLY_ROLLBACK | ffi::SQLITE_READONLY_RECOVERY | ffi::SQLITE_READONLY_CANTINIT
        )
    )
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
    let sqlite = |e| sqlite_error(db, e);
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
    let sqlite = |e| sqlite_error(db, e);
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite)?;
    // Again under the write lock: another process may have migrated it.
    let from = refuse(db, user_version(&tx).map_err(sqlite)?, known)?;
    if from == known {
        // Migrated by another process while this one waited for the lock.
        if let Some(expected) = expected {
            same_schema(&tx, db, known, expected)?;
        }
        return Ok(Migrated { from, to: known });
    }
    // A version-0 file with tables (owner decision): version 1 only if it
    // is exactly version 1's schema, never migration 1 run over tables it
    // did not make (whose IF NOT EXISTS would keep an older shape, or
    // trip on it and blame the migration).
    let mut start = from;
    if from == 0 {
        let now = schema_text(&tx).map_err(sqlite)?;
        if !now.is_empty() {
            if now != schema_of(&steps[..1]).map_err(sqlite)? {
                return Err(MigrateError::Mismatch {
                    db: db.to_string(),
                    version: 1,
                });
            }
            start = 1;
        }
    }
    // Re-check D4: a reference that dangles already is not this
    // migration's doing, and must not be reported as if it were.
    if let Some((table, parent)) = dangling(&tx).map_err(sqlite)? {
        return Err(MigrateError::DanglingBefore {
            db: db.to_string(),
            from,
            to: known,
            table,
            parent,
        });
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
    for (i, step) in steps.iter().enumerate().skip(start as usize) {
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
fn schema_of(steps: &[Migration]) -> rusqlite::Result<String> {
    let conn = Connection::open_in_memory()?;
    for step in steps {
        for sql in *step {
            conn.execute_batch(sql)?;
        }
    }
    schema_text(&conn)
}

/// `schema_of`, for the recorded-schema tests.
#[doc(hidden)]
pub fn built(steps: &[Migration]) -> String {
    schema_of(steps).unwrap()
}

/// Test support (store and memory tests): version `k`'s schema, built from
/// migrations `1..=k`, equals `dir/<k>.sql` for every `k`, and no file
/// exists for a version the code does not have. With
/// `HARNESS_WRITE_SCHEMA=1` a version without a file gets one; a recorded
/// file is never rewritten (before the first tag, delete `1.sql` to record
/// migration 1 again). What this guards is the schema a released
/// migration makes, not its data statements: a backfill `UPDATE` or
/// `INSERT` changed after release leaves the schema as it was and passes
/// (review R43-6); only review catches that.
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
