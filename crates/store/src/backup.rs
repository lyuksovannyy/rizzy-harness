//! The SQLite half of `rizzy-harness backup` and `restore`
//! (docs/backup-restore.md): a consistent copy of one database file, read
//! while its server may be writing it, and the few changes a restore makes
//! to its copy. The files, the order and the manifest are the CLI's.

use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension};

/// How long a snapshot waits for a writer's lock (the server's own timeout).
const BUSY: std::time::Duration = std::time::Duration::from_millis(5000);

fn open_ro(path: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    c.busy_timeout(BUSY)?;
    Ok(c)
}

/// `VACUUM INTO` `dst` from `src`: one read transaction, so the copy is the
/// database as one commit left it, with the WAL folded in and free pages
/// (deleted rows) left behind. `dst` must not exist or be an empty file:
/// the caller makes it first, 0600 and exclusively, so the copy is never
/// readable by others, not even for a moment.
///
/// `writable`: open the source read-write (no create), for a backup that
/// holds the data directory's lock: a WAL a crashed server left needs a
/// writer to be recovered, and nothing else has the file open. Otherwise
/// read-only (a running server owns the file; WAL readers never block it).
pub fn snapshot(src: &Path, dst: &Path, writable: bool) -> rusqlite::Result<()> {
    let c = if writable {
        let c = Connection::open_with_flags(
            src,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        c.busy_timeout(BUSY)?;
        c
    } else {
        open_ro(src)?
    };
    let to = dst
        .to_str()
        .ok_or_else(|| rusqlite::Error::InvalidPath(dst.to_path_buf()))?;
    c.execute("VACUUM INTO ?1", [to])?;
    Ok(())
}

/// The schema version of a database file (read-only).
pub fn user_version(path: &Path) -> rusqlite::Result<i64> {
    open_ro(path)?.query_row("PRAGMA user_version", [], |r| r.get(0))
}

/// What a check of one copy found wrong, in words that name no content.
pub fn check(path: &Path) -> Result<(), String> {
    let c = open_ro(path).map_err(|e| e.to_string())?;
    let ok: String = c
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if ok != "ok" {
        return Err("it fails SQLite's integrity check".into());
    }
    let dangling: Option<String> = c
        .query_row("PRAGMA foreign_key_check", [], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(table) = dangling {
        return Err(format!("rows of {table} point at missing rows"));
    }
    Ok(())
}

/// One stored file a `master.db` snapshot names.
pub struct SnapBlob {
    pub blob_id: String,
    pub sha256: String,
    pub size: u64,
    /// The file's key (it seals the file; it lives only in `master.db`).
    pub key: Vec<u8>,
}

/// Every row of `blobs` in a `master.db` copy, each handed to `f` (keys
/// are not collected: one at a time).
pub fn each_blob(
    path: &Path,
    mut f: impl FnMut(SnapBlob) -> Result<(), String>,
) -> Result<(), String> {
    let c = open_ro(path).map_err(|e| e.to_string())?;
    let mut st = c
        .prepare("SELECT blob_id, sha256, size, key FROM blobs ORDER BY blob_id")
        .map_err(|e| e.to_string())?;
    let mut rows = st.query([]).map_err(|e| e.to_string())?;
    while let Some(r) = rows.next().map_err(|e| e.to_string())? {
        let size: i64 = r.get(2).map_err(|e| e.to_string())?;
        f(SnapBlob {
            blob_id: r.get(0).map_err(|e| e.to_string())?,
            sha256: r.get(1).map_err(|e| e.to_string())?,
            size: u64::try_from(size).map_err(|_| "a stored file has a negative size")?,
            key: r.get(3).map_err(|e| e.to_string())?,
        })?;
    }
    Ok(())
}

/// The ids of every stored file a `master.db` copy names.
pub fn blob_ids(path: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    each_blob(path, |b| {
        out.push(b.blob_id);
        Ok(())
    })?;
    Ok(out)
}

fn has_table(c: &Connection, name: &str) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
}

/// Remove from a node log's copy (`chat.db`, `node.db`) every chat that
/// the `master.db` copy does not know: one created after `master.db` was
/// copied. The master writes a chat's owner, workspace and project into
/// `master.db` only once the node has made it, so such a chat would come
/// back without an owner, out of its owner's reach. A sub-agent's chat
/// whose parent is kept stays (it inherits its owner from the parent).
/// Returns how many chats went.
///
/// The copy must be at this build's schema version (the caller checks):
/// it is opened as a store, which would migrate an older one.
pub fn drop_unknown_chats(node_copy: &Path, master_copy: &Path) -> crate::Result<usize> {
    let mut known = std::collections::HashSet::new();
    {
        let c = open_ro(master_copy)?;
        let mut st = c.prepare("SELECT session_id FROM sessions")?;
        let ids = st.query_map([], |r| r.get::<_, String>(0))?;
        for id in ids {
            known.insert(id?);
        }
    }
    let n = {
        let s = crate::Store::open(node_copy, crate::Durability::Full)?;
        let rows = s.sessions()?;
        let mut keep: std::collections::HashSet<String> = rows
            .iter()
            .map(|r| r.session_id.to_string())
            .filter(|id| known.contains(id))
            .collect();
        // Sub-agents' chats under a kept chat, at any depth.
        loop {
            let more: Vec<String> = rows
                .iter()
                .filter(|r| {
                    let id = r.session_id.to_string();
                    !keep.contains(&id) && r.parent_id.as_ref().is_some_and(|p| keep.contains(p))
                })
                .map(|r| r.session_id.to_string())
                .collect();
            if more.is_empty() {
                break;
            }
            keep.extend(more);
        }
        let mut n = 0;
        for r in &rows {
            if !keep.contains(&r.session_id.to_string()) {
                s.delete_session(r.session_id, false)?;
                n += 1;
            }
        }
        n
    };
    // Back to one plain file, as `VACUUM INTO` left it.
    let c = Connection::open_with_flags(
        node_copy,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    c.query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(()))?;
    Ok(n)
}

/// What a restore changes in its copy of `master.db`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Restored {
    /// Logins dropped: everyone signs in again.
    pub logins: usize,
    /// Routines switched off (`--pause-routines`).
    pub routines: usize,
}

/// Drop every login (`ui_sessions`) of a restored `master.db`, and switch
/// its routines off when `pause_routines`. One transaction; the copy is
/// this process's own (the restore holds the target's lock).
pub fn on_restore(path: &Path, pause_routines: bool) -> rusqlite::Result<Restored> {
    let mut c = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    c.busy_timeout(BUSY)?;
    // Deleted logins are overwritten, as the server deletes them.
    c.pragma_update(None, "secure_delete", "ON")?;
    let tx = c.transaction()?;
    let mut out = Restored::default();
    if has_table(&tx, "ui_sessions")? {
        out.logins = tx.execute("DELETE FROM ui_sessions", [])?;
    }
    if pause_routines && has_table(&tx, "routines")? {
        out.routines = tx.execute("UPDATE routines SET enabled = 0 WHERE enabled != 0", [])?;
    }
    tx.commit()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    fn empty_0600(p: &Path) {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(p)
            .unwrap();
    }

    /// The copy goes into the empty 0600 file made for it and stays 0600;
    /// SQLite leaves nothing else beside it; the version is kept.
    #[test]
    fn a_snapshot_fills_the_private_file_made_for_it() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.db");
        let s = crate::Store::open(&src, crate::Durability::Full).unwrap();
        s.kv_set("k", "v").unwrap();
        let out = d.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let dst = out.join("a.db");
        empty_0600(&dst);
        snapshot(&src, &dst, false).unwrap();
        let mode = std::fs::metadata(&dst).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let names: Vec<_> = std::fs::read_dir(&out)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["a.db"]);
        assert_eq!(
            user_version(&dst).unwrap(),
            crate::DB_SCHEMA.steps.len() as i64
        );
        check(&dst).unwrap();
        // A file that is not empty is refused, not overwritten.
        assert!(snapshot(&src, &dst, false).is_err());
    }

    /// OpenClaw's note that VACUUM may renumber implicit rowids: our
    /// tables that order by rowid (audit) keep it through the bundled
    /// SQLite's VACUUM INTO, deletions in between included.
    #[test]
    fn a_snapshot_keeps_implicit_rowids() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.db");
        let s = crate::Store::open(&src, crate::Durability::Full).unwrap();
        for i in 0..300 {
            s.audit(&format!("k{i}"), "d").unwrap();
        }
        let c = Connection::open(&src).unwrap();
        c.execute("DELETE FROM audit WHERE rowid % 3 = 0", [])
            .unwrap();
        let rows = |c: &Connection| -> Vec<(i64, String)> {
            c.prepare("SELECT rowid, kind FROM audit ORDER BY rowid")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        let before = rows(&c);
        let dst = d.path().join("b.db");
        empty_0600(&dst);
        snapshot(&src, &dst, false).unwrap();
        assert_eq!(rows(&Connection::open(&dst).unwrap()), before);
        assert_eq!(before.len(), 200);
    }

    /// Logins go, routines stay unless asked.
    #[test]
    fn a_restore_drops_logins_and_pauses_routines_only_when_asked() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("master.db");
        drop(crate::Store::open(&p, crate::Durability::Full).unwrap());
        let seed = |c: &Connection| {
            // Rows alone, without their account and workspace.
            c.execute_batch(
                "PRAGMA foreign_keys = OFF;\
                 INSERT INTO ui_sessions VALUES ('h1', 'b', 'u', 0, 0, 0);\
                 INSERT INTO ui_sessions VALUES ('h2', 'b', 'u', 0, 0, 0);\
                 INSERT INTO routines (routine_id, workspace_id, owner_id, name, spec, \
                 created_ms, updated_ms) VALUES ('r', 'w', 'u', 'n', '{}', 0, 0);",
            )
            .unwrap();
        };
        let enabled = |c: &Connection| -> i64 {
            c.query_row("SELECT enabled FROM routines", [], |r| r.get(0))
                .unwrap()
        };
        let c = Connection::open(&p).unwrap();
        seed(&c);
        let n = on_restore(&p, false).unwrap();
        assert_eq!(
            n,
            Restored {
                logins: 2,
                routines: 0
            }
        );
        assert_eq!(enabled(&c), 1);
        c.execute_batch("DELETE FROM routines").unwrap();
        seed(&c);
        let n = on_restore(&p, true).unwrap();
        assert_eq!(
            n,
            Restored {
                logins: 2,
                routines: 1
            }
        );
        assert_eq!(enabled(&c), 0);
    }
}
