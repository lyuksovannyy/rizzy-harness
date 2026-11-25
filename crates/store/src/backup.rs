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

/// `VACUUM INTO` `dst` from `src` through a read-only connection: one read
/// transaction, so the copy is the database as one commit left it, with
/// the WAL folded in (a crashed server's too: a read-only connection reads
/// the WAL through its `-shm`) and free pages (deleted rows) left behind.
/// Nothing of the source is written, whether a server runs or not. `dst`
/// must not exist or be an empty file: the caller makes it first, 0600 and
/// exclusively, so the copy is never readable by others.
pub fn snapshot(src: &Path, dst: &Path) -> rusqlite::Result<()> {
    vacuum_into(&open_ro(src)?, dst)
}

/// [`snapshot`] of a private copy this process made of a stopped server's
/// files (database, `-wal`, `-shm`): opened read-write, so SQLite may
/// recover a WAL whose `-shm` is missing, in the copy, never the source.
pub fn snapshot_private_copy(src: &Path, dst: &Path) -> rusqlite::Result<()> {
    let c = Connection::open_with_flags(
        src,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    c.busy_timeout(BUSY)?;
    vacuum_into(&c, dst)
}

fn vacuum_into(c: &Connection, dst: &Path) -> rusqlite::Result<()> {
    let to = dst
        .to_str()
        .ok_or_else(|| rusqlite::Error::InvalidPath(dst.to_path_buf()))?;
    c.execute("VACUUM INTO ?1", [to])?;
    Ok(())
}

/// Whether an error is SQLite refusing a read-only reader a file that needs
/// a writer first (a WAL to recover without its index, a hot journal).
pub fn needs_writer(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _) if matches!(
            f.extended_code,
            rusqlite::ffi::SQLITE_READONLY_ROLLBACK
                | rusqlite::ffi::SQLITE_READONLY_RECOVERY
                | rusqlite::ffi::SQLITE_READONLY_CANTINIT
                | rusqlite::ffi::SQLITE_READONLY_CANTLOCK
        )
    )
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

use crate::mirror_keys::{CREATE_INTENT_PREFIX, auto_model_key, intent_expired};

/// What [`settle_copies`] changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Settled {
    /// Chats caught between their create and its owner stamp, stamped from
    /// the create's intent, as the master's late claim does.
    pub stamped: usize,
    /// Ownerless chats whose intent had expired at the snapshot's moment:
    /// left ownerless and the intent dropped, as the live late claim does.
    pub expired: usize,
    /// Chats begun after the `master.db` copy, or caught ownerless with no
    /// usable intent: removed from every copy.
    pub left_out: usize,
    /// Chats deleted after the `master.db` copy: removed from it.
    pub deleted: usize,
}

fn store_ids(s: &crate::Store) -> crate::Result<Vec<crate::SessionRow>> {
    s.sessions()
}

/// Make the copies of `master.db` and of this machine's node logs agree,
/// as of the `master.db` copy (docs/backup-restore.md):
///
/// 1. A chat whose row has no owner yet but whose create intent is still in
///    `kv` (the master drops the intent and stamps the owner in two steps)
///    is stamped from the intent, when the intent names this chat's node;
///    an ownerless chat with an intent that cannot be read is left out.
/// 2. A chat in a node log's copy that the `master.db` copy does not know
///    began after it (the master stamps the owner only once the node has
///    the chat): left out, so it never comes back without an owner. A
///    sub-agent's chat under a kept chat stays (it inherits its owner).
/// 3. A chat of one of these nodes in the `master.db` copy that its node
///    log's copy no longer has was deleted meanwhile: removed from it.
///
/// `nodes`: each node log's copy with the node id it logs for (`chat.db`
/// for `master`, `node.db` for `local`). Every copy must be at this build's
/// schema version (the caller checks): they are opened as stores, which
/// would migrate an older one. Each is put back to one plain file.
pub fn settle_copies(
    master_copy: &Path,
    nodes: &[(&Path, &str)],
    snapshot_ms: i64,
) -> crate::Result<Settled> {
    let mut out = Settled::default();
    {
        let m = crate::Store::open(master_copy, crate::Durability::Full)?;
        // 1: the intents.
        for (key, v) in m.kv_prefix(CREATE_INTENT_PREFIX)? {
            let Some(sid) = key
                .strip_prefix(CREATE_INTENT_PREFIX)
                .and_then(|id| id.parse::<harness_core::SessionId>().ok())
            else {
                continue;
            };
            let Some(row) = m.session(sid)? else {
                continue;
            };
            if row.owner_id.is_some() {
                m.kv_delete(&key)?;
                continue;
            }
            if intent_expired(&v, snapshot_ms) {
                m.kv_delete(&key)?;
                out.expired += 1;
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(&v).unwrap_or_default();
            let owner = v["owner"].as_str();
            let workspace = v["workspace"].as_str();
            if v["node"].as_str() != Some(row.node_id.as_str())
                || (owner.is_none() && workspace.is_none())
            {
                m.delete_session(sid, false)?;
                m.kv_delete(&key)?;
                out.left_out += 1;
                continue;
            }
            m.claim_session(sid, owner, workspace)?;
            if v["auto"].as_bool() == Some(true) {
                m.kv_set(&auto_model_key(sid), "1")?;
            }
            if let Some(p) = v["project"].as_str() {
                m.link_new_session(sid, p)?;
            }
            m.kv_delete(&key)?;
            out.stamped += 1;
        }
        let known: std::collections::HashSet<String> = store_ids(&m)?
            .iter()
            .map(|r| r.session_id.to_string())
            .collect();
        // 2 and 3, per node log.
        for (copy, node_id) in nodes {
            let n = crate::Store::open(copy, crate::Durability::Full)?;
            let rows = store_ids(&n)?;
            let mut keep: std::collections::HashSet<String> = rows
                .iter()
                .map(|r| r.session_id.to_string())
                .filter(|id| known.contains(id))
                .collect();
            loop {
                let more: Vec<String> = rows
                    .iter()
                    .filter(|r| {
                        let id = r.session_id.to_string();
                        !keep.contains(&id)
                            && r.parent_id.as_ref().is_some_and(|p| keep.contains(p))
                    })
                    .map(|r| r.session_id.to_string())
                    .collect();
                if more.is_empty() {
                    break;
                }
                keep.extend(more);
            }
            for r in &rows {
                if !keep.contains(&r.session_id.to_string()) {
                    n.delete_session(r.session_id, false)?;
                    out.left_out += 1;
                }
            }
            let in_log: std::collections::HashSet<String> =
                rows.iter().map(|r| r.session_id.to_string()).collect();
            for r in store_ids(&m)? {
                if r.node_id == *node_id && !in_log.contains(&r.session_id.to_string()) {
                    // As the master's own delete of a chat (the same
                    // path): its node will not send it again.
                    m.forget_session(r.session_id)?;
                    out.deleted += 1;
                }
            }
        }
    }
    for p in std::iter::once(master_copy).chain(nodes.iter().map(|(p, _)| *p)) {
        plain_file(p)?;
    }
    Ok(out)
}

/// Fold a copy's WAL back and make it one plain file again, as
/// `VACUUM INTO` left it.
fn plain_file(p: &Path) -> rusqlite::Result<()> {
    let c = Connection::open_with_flags(
        p,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    c.query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(()))?;
    Ok(())
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
        snapshot(&src, &dst).unwrap();
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
        assert!(snapshot(&src, &dst).is_err());
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
        snapshot(&src, &dst).unwrap();
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

    fn new_user(id: &str) -> crate::NewUser {
        crate::NewUser {
            user_id: id.into(),
            username: id.into(),
            password_hash: "$argon2id$x".into(),
            role: crate::Role::Member,
            exec_ceiling: harness_core::policy::ExecLevel::None,
            must_change_password: false,
        }
    }

    fn created(s: &crate::Store, node: &str, sid: harness_core::SessionId) {
        s.append(
            node,
            &harness_core::RawEvent {
                session_id: sid,
                seq: 1,
                ts_ms: 1,
                kind: "session_created".into(),
                payload: serde_json::json!({"title": "t", "cwd": "", "model": "m",
                    "backend": "native", "node_id": node, "profile": "chat",
                    "approval_mode": "ask"}),
            },
        )
        .unwrap();
    }

    /// The create's two steps (drop the intent, stamp the owner) are not
    /// one transaction, so a copy can hold the chat with no owner and its
    /// intent still there: the copy step stamps it as the claim would, and
    /// the chat keeps its owner and stays in the node log's copy.
    #[test]
    fn a_chat_caught_before_its_stamp_gets_its_owner_from_the_intent() {
        let d = tempfile::tempdir().unwrap();
        let (m, n) = (d.path().join("master.db"), d.path().join("chat.db"));
        let master = crate::Store::open(&m, crate::Durability::Full).unwrap();
        let node = crate::Store::open(&n, crate::Durability::Full).unwrap();
        master.create_user(&new_user("u1")).unwrap();
        let sid = harness_core::SessionId::new();
        created(&master, "master", sid);
        created(&node, "master", sid);
        drop(node);
        master
            .kv_set(
                &format!("create-intent:{sid}"),
                &serde_json::json!({"node": "master", "owner": "u1", "workspace": null,
                    "at": harness_core::now_ms(), "project": null, "auto": true})
                .to_string(),
            )
            .unwrap();
        drop(master);
        let done = settle_copies(&m, &[(&n, "master")], harness_core::now_ms()).unwrap();
        assert_eq!(
            done,
            Settled {
                stamped: 1,
                expired: 0,
                left_out: 0,
                deleted: 0
            }
        );
        let master = crate::Store::open(&m, crate::Durability::Full).unwrap();
        let row = master.session(sid).unwrap().unwrap();
        assert_eq!(
            row.owner_id.as_deref(),
            Some("u1"),
            "the chat came back ownerless"
        );
        assert!(
            master
                .kv_get(&format!("create-intent:{sid}"))
                .unwrap()
                .is_none()
        );
    }

    fn files(dir: &Path) -> Vec<(String, u64, u64)> {
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                let b = std::fs::read(&p).unwrap();
                let mut h = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&b, &mut h);
                (
                    p.file_name().unwrap().to_string_lossy().into_owned(),
                    b.len() as u64,
                    std::hash::Hasher::finish(&h),
                )
            })
            .collect();
        v.sort();
        v
    }

    /// A database as a crash leaves it (`-wal` and `-shm` beside it, rows
    /// only in the WAL), read by the bundled SQLite through a read-only
    /// connection: the copy has the WAL's rows, and the database and WAL
    /// do not change; the `-shm` does (every reader, a running server's
    /// too, writes its read marks into that index), which is why a
    /// stopped server's files are copied first and snapshotted from the
    /// copy. Without its `-shm` a read-only reader even makes one; the
    /// private copy, opened read-write, recovers and has the rows.
    #[test]
    fn a_crashed_wal_is_read_without_writing_or_refused() {
        let d = tempfile::tempdir().unwrap();
        let live = d.path().join("live");
        std::fs::create_dir(&live).unwrap();
        let s = crate::Store::open(&live.join("m.db"), crate::Durability::Full).unwrap();
        s.kv_set("in-the-wal", "yes").unwrap();
        // The process "dies": its files are taken as they are, open.
        let crashed = d.path().join("crashed");
        std::fs::create_dir(&crashed).unwrap();
        for n in ["m.db", "m.db-wal", "m.db-shm"] {
            std::fs::copy(live.join(n), crashed.join(n)).unwrap();
        }
        assert!(std::fs::metadata(crashed.join("m.db-wal")).unwrap().len() > 0);
        let before = files(&crashed);
        let out = d.path().join("out.db");
        empty_0600(&out);
        snapshot(&crashed.join("m.db"), &out).unwrap();
        let no_shm = |v: Vec<(String, u64, u64)>| -> Vec<_> {
            v.into_iter().filter(|f| !f.0.ends_with("-shm")).collect()
        };
        assert_eq!(
            no_shm(files(&crashed)),
            no_shm(before),
            "the read-only snapshot wrote"
        );
        let got: Option<String> = Connection::open(&out)
            .unwrap()
            .query_row("SELECT v FROM kv WHERE k = 'in-the-wal'", [], |r| r.get(0))
            .optional()
            .unwrap();
        assert_eq!(got.as_deref(), Some("yes"), "the WAL's row is missing");

        // Without its `-shm`, a read-only reader makes one beside the
        // source (so it writes): never done on a source. The private copy,
        // opened read-write, recovers and has the rows.
        let bare = d.path().join("bare");
        std::fs::create_dir(&bare).unwrap();
        for n in ["m.db", "m.db-wal"] {
            std::fs::copy(crashed.join(n), bare.join(n)).unwrap();
        }
        let throwaway = d.path().join("throwaway");
        std::fs::create_dir(&throwaway).unwrap();
        for n in ["m.db", "m.db-wal"] {
            std::fs::copy(bare.join(n), throwaway.join(n)).unwrap();
        }
        let o = d.path().join("o.db");
        empty_0600(&o);
        let _ = snapshot(&throwaway.join("m.db"), &o);
        assert!(
            throwaway.join("m.db-shm").exists(),
            "SQLite no longer makes a -shm for a read-only reader: revisit the private copy"
        );
        let before = files(&bare);
        let private = d.path().join("private");
        std::fs::create_dir(&private).unwrap();
        for n in ["m.db", "m.db-wal"] {
            std::fs::copy(bare.join(n), private.join(n)).unwrap();
        }
        let out3 = d.path().join("out3.db");
        empty_0600(&out3);
        snapshot_private_copy(&private.join("m.db"), &out3).unwrap();
        assert_eq!(files(&bare), before);
        let got: Option<String> = Connection::open(&out3)
            .unwrap()
            .query_row("SELECT v FROM kv WHERE k = 'in-the-wal'", [], |r| r.get(0))
            .optional()
            .unwrap();
        assert_eq!(got.as_deref(), Some("yes"), "the private copy lost the WAL");
        drop(s);
    }

    /// The other two settlements: a chat only the node log's copy has is
    /// left out; a chat only the `master.db` copy has, of that node, is
    /// deleted from it; a chat of another node stays.
    #[test]
    fn settling_leaves_out_late_chats_and_deleted_ones() {
        let d = tempfile::tempdir().unwrap();
        let (m, n) = (d.path().join("master.db"), d.path().join("chat.db"));
        let master = crate::Store::open(&m, crate::Durability::Full).unwrap();
        let node = crate::Store::open(&n, crate::Durability::Full).unwrap();
        let (both, late, gone, remote) = (
            harness_core::SessionId::new(),
            harness_core::SessionId::new(),
            harness_core::SessionId::new(),
            harness_core::SessionId::new(),
        );
        created(&master, "master", both);
        created(&node, "master", both);
        created(&node, "master", late);
        created(&master, "master", gone);
        created(&master, "elsewhere", remote);
        drop((master, node));
        let done = settle_copies(&m, &[(&n, "master")], harness_core::now_ms()).unwrap();
        assert_eq!(
            done,
            Settled {
                stamped: 0,
                expired: 0,
                left_out: 1,
                deleted: 1
            }
        );
        let master = crate::Store::open(&m, crate::Durability::Full).unwrap();
        let node = crate::Store::open(&n, crate::Durability::Full).unwrap();
        assert!(master.session(both).unwrap().is_some());
        assert!(master.session(gone).unwrap().is_none());
        assert!(master.session(remote).unwrap().is_some());
        assert!(node.session(late).unwrap().is_none());
        assert!(node.session(both).unwrap().is_some());
    }

    /// The live server's late claim drops an intent older than its time and
    /// leaves the chat ownerless (only the API token reads it): the copy
    /// does the same, measured at the snapshot's moment. Just inside the
    /// limit the chat is stamped; just outside it is not, and its intent
    /// goes.
    #[test]
    fn an_expired_intent_stamps_no_owner_in_the_copy() {
        let ttl = crate::mirror_keys::CREATE_INTENT_TTL_MS;
        let snap_ms = 10 * ttl;
        for (age, owned) in [(ttl - 1000, true), (ttl + 1000, false)] {
            let d = tempfile::tempdir().unwrap();
            let (m, n) = (d.path().join("master.db"), d.path().join("chat.db"));
            let master = crate::Store::open(&m, crate::Durability::Full).unwrap();
            let node = crate::Store::open(&n, crate::Durability::Full).unwrap();
            master.create_user(&new_user("u1")).unwrap();
            let sid = harness_core::SessionId::new();
            created(&master, "master", sid);
            created(&node, "master", sid);
            master
                .kv_set(
                    &format!("create-intent:{sid}"),
                    &serde_json::json!({"node": "master", "owner": "u1", "workspace": null,
                        "at": snap_ms - age, "project": null, "auto": false})
                    .to_string(),
                )
                .unwrap();
            drop((master, node));
            settle_copies(&m, &[(&n, "master")], snap_ms).unwrap();
            let master = crate::Store::open(&m, crate::Durability::Full).unwrap();
            let row = master.session(sid).unwrap().expect("the chat stays");
            assert_eq!(row.owner_id.is_some(), owned, "age {age}");
            assert!(
                master
                    .kv_get(&format!("create-intent:{sid}"))
                    .unwrap()
                    .is_none()
            );
            let node = crate::Store::open(&n, crate::Durability::Full).unwrap();
            assert!(node.session(sid).unwrap().is_some(), "age {age}");
        }
    }

    /// A chat deleted during the backup leaves the copy as the live delete
    /// leaves the mirror: its auto-model mark and callback note go too.
    #[test]
    fn a_deleted_chat_leaves_its_marks_too() {
        let d = tempfile::tempdir().unwrap();
        let (m, n) = (d.path().join("master.db"), d.path().join("chat.db"));
        let master = crate::Store::open(&m, crate::Durability::Full).unwrap();
        let node = crate::Store::open(&n, crate::Durability::Full).unwrap();
        let gone = harness_core::SessionId::new();
        created(&master, "master", gone);
        master
            .kv_set(&format!("session-model-auto:{gone}"), "1")
            .unwrap();
        master
            .kv_set(&format!("callback-attention:{gone}"), "{}")
            .unwrap();
        drop((master, node));
        settle_copies(&m, &[(&n, "master")], harness_core::now_ms()).unwrap();
        let master = crate::Store::open(&m, crate::Durability::Full).unwrap();
        assert!(master.session(gone).unwrap().is_none());
        assert!(
            master
                .kv_get(&format!("session-model-auto:{gone}"))
                .unwrap()
                .is_none()
        );
        assert!(
            master
                .kv_get(&format!("callback-attention:{gone}"))
                .unwrap()
                .is_none()
        );
    }
}
