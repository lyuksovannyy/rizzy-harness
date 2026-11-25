//! `rizzy-harness backup <dir>`, `backup --verify <dir>` and
//! `restore <dir>` (FIX_PLAN 4.4, docs/backup-restore.md).
//!
//! A backup is a plain directory that mirrors the data directory, finished
//! by `MANIFEST.json` (written last: a directory without it is an
//! interrupted backup and restore refuses it). It runs as a second process
//! beside a running server (no lock of the server's can be taken from
//! here), or, when the data directory's lock is free, holding it:
//!
//! 1. `master.db` is copied first (`VACUUM INTO`, one read transaction).
//! 2. Every stored file that copy names is hard-linked into a staging
//!    folder in the data directory. A blob file never changes once stored
//!    and is deleted only after its row, so a link taken while it exists is
//!    the file for ever; one that is gone had its row deleted after the
//!    copy, and the copy is taken again (at most [`ATTEMPTS`] times).
//! 3. `memory.db`, `chat.db`, `node.db`: after `master.db`, so a node's log
//!    is never behind the master's mirror of it (a mirror ahead of a node
//!    log freezes that chat on the restored server; a mirror behind is
//!    filled from the log).
//! 4. The plain files and `keys/`, after the databases that use them.
//! 5. The staged files, copied for real (never linked: the backup shares
//!    no inode with the live store), then decrypted once and checked
//!    against the SHA-256 their rows name.
//!
//! Everything is created 0600 (folders 0700) exclusively and without
//! following links, under temporary names renamed at the end; nothing a
//! file holds is ever printed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use anyhow::{Context, bail};
use harness_store::backup as db;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::lock;

/// The manifest's name; written last.
pub const MANIFEST: &str = "MANIFEST.json";
/// In a data directory while a restore fills it: `serve` and `node run`
/// refuse to start on it.
pub const RESTORING: &str = "RESTORING";
/// The staging folder of the hard links, in the data directory, and the
/// lock that keeps one backup at a time on a data directory.
pub const STAGING: &str = "backup-staging";
const STAGING_LOCK: &str = "backup.lock";
/// Copies of `master.db` taken before the backup gives up on files that
/// keep being deleted.
pub const ATTEMPTS: u32 = 5;
const FORMAT: u32 = 1;
/// The copy buffer: every file, of any size, streams through this much.
pub const BUF: usize = 1 << 20;
/// A manifest larger than this is not one of ours.
const MANIFEST_MAX: u64 = 64 << 20;

/// The databases, in the order they are copied, with their schema.
fn databases() -> [(
    &'static str,
    &'static harness_store::migrate::Schema<'static>,
); 4] {
    [
        ("master.db", &harness_store::DB_SCHEMA),
        ("memory.db", &harness_memory::DB_SCHEMA),
        ("chat.db", &harness_store::DB_SCHEMA),
        ("node.db", &harness_store::DB_SCHEMA),
    ]
}

/// The plain files of the data directory a backup holds.
const PLAIN: &[&str] = &[
    "config.toml",
    "node.toml",
    "secrets.toml",
    "api-token",
    "remote.toml",
    "AGENTS.md",
    "project-mcp.json",
];

/// What a backup does not hold, said by the command at the end.
pub const NOT_INCLUDED: &str = "\
Not in this backup: the project folders and repositories the chats work in; checkpoints \
(after a restore, undo and rewind of the turns made before it are refused); a remote node's \
own data (back it up on that machine, with its --data-dir); .env and HARNESS_* environment \
variables.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: u32,
    pub harness: String,
    pub created_ms: i64,
    /// Taken beside a running server (true) or holding the lock.
    pub online: bool,
    /// Each database's schema version.
    pub databases: BTreeMap<String, i64>,
    pub files: Vec<Entry>,
}

fn interrupted() -> anyhow::Error {
    anyhow::anyhow!("interrupted")
}

fn check_cancel(cancel: &AtomicBool) -> anyhow::Result<()> {
    if cancel.load(Ordering::SeqCst) {
        return Err(interrupted());
    }
    Ok(())
}

/// A name of one path segment we write: letters, digits, `.`, `_`, `-`,
/// not starting with a dot.
fn valid_name(n: &str) -> bool {
    (1..=128).contains(&n.len())
        && !n.starts_with('.')
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn valid_blob(shard: &str, file: &str) -> bool {
    let Some(id) = file.strip_suffix(".rzb") else {
        return false;
    };
    id.len() == 32
        && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && shard == &id[30..]
}

/// The only paths a manifest may name: the known top-level files,
/// `keys/<name>`, `agents/<name>` and `blobs/<xx>/<id>.rzb`. Nothing
/// absolute, no `..`, `.` or empty segment, nothing else.
pub fn valid_path(p: &str) -> bool {
    let parts: Vec<&str> = p.split('/').collect();
    match parts.as_slice() {
        [one] => databases().iter().any(|(n, _)| n == one) || PLAIN.contains(one),
        ["keys" | "agents", name] => valid_name(name),
        ["blobs", shard, file] => valid_blob(shard, file),
        _ => false,
    }
}

/// Create `p` 0600, exclusively, not through a link.
fn create_private(p: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(p)
}

/// Make folder `p` 0700 (it may exist already as a real folder of ours).
fn mkdir_private(p: &Path) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let m = fs::symlink_metadata(p)?;
            if !m.is_dir() {
                return Err(io::Error::other(format!(
                    "{} exists and is not a folder",
                    p.display()
                )));
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn sync_dir(p: &Path) -> io::Result<()> {
    File::open(p)?.sync_all()
}

/// Open a file to read: never through a link, and only a regular file.
fn open_regular(p: &Path) -> io::Result<(File, fs::Metadata)> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(p)?;
    let m = f.metadata()?;
    if !m.is_file() {
        return Err(io::Error::other(format!(
            "{} is not a regular file",
            p.display()
        )));
    }
    Ok((f, m))
}

/// Stream `from` into `to` through `buf` (nothing else is held, whatever
/// the size), hashing; the bytes and the SHA-256 in hex.
pub fn copy_stream(
    from: &mut impl Read,
    to: &mut impl Write,
    buf: &mut [u8],
    cancel: &AtomicBool,
) -> anyhow::Result<(u64, String)> {
    let mut h = Sha256::new();
    let mut n = 0u64;
    loop {
        check_cancel(cancel)?;
        let k = match from.read(buf) {
            Ok(0) => break,
            Ok(k) => k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        h.update(&buf[..k]);
        to.write_all(&buf[..k])?;
        n += k as u64;
    }
    Ok((n, hex::encode(h.finalize())))
}

fn mb(n: u64) -> String {
    format!("{:.1} MB", n as f64 / 1_000_000.0)
}

/// Each stored file `master` names, decrypted from `path_of(id)` and
/// checked against the SHA-256 and size of its row; how many.
fn verify_blobs(
    master: &Path,
    path_of: impl Fn(&str) -> PathBuf,
    cancel: &AtomicBool,
) -> anyhow::Result<u64> {
    let mut n = 0u64;
    db::each_blob(master, |b| {
        if cancel.load(Ordering::SeqCst) {
            return Err("interrupted".into());
        }
        let p = path_of(&b.blob_id);
        let bad = |why: &str| format!("stored file {}: {why}", b.blob_id);
        let mut r = harness_master::blobs::Reader::open(&p, &b.blob_id, &b.key, b.size)
            .map_err(|e| bad(&e.to_string()))?;
        let mut h = Sha256::new();
        if b.size > 0 {
            r.range(0, b.size - 1, |s| {
                h.update(s);
                Ok(())
            })
            .map_err(|e| bad(&e.to_string()))?;
        }
        if hex::encode(h.finalize()) != b.sha256 {
            return Err(bad("its content does not match the SHA-256 its row names"));
        }
        n += 1;
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!(e))?;
    Ok(n)
}

/// Check one database copy: SQLite's integrity and foreign keys, and a
/// schema this build knows (a newer one is refused).
fn check_db(
    name: &str,
    p: &Path,
    schema: &harness_store::migrate::Schema<'_>,
) -> anyhow::Result<i64> {
    db::check(p).map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
    harness_store::migrate::check_file(p, schema).map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
    db::user_version(p).with_context(|| format!("{name}: reading its schema version"))
}

// ---------------------------------------------------------------- backup

/// The destination, checked and made ready: absent (made here, 0700) or an
/// empty folder of this user's (made 0700); never a link; never inside the
/// data directory or holding it.
fn prepare_dest(data: &Path, dest: &Path) -> anyhow::Result<(PathBuf, bool)> {
    let name = dest
        .file_name()
        .with_context(|| format!("{} names no folder", dest.display()))?;
    let parent = match dest.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let parent = parent
        .canonicalize()
        .with_context(|| format!("the folder {} must exist", parent.display()))?;
    let real = parent.join(name);
    if real.starts_with(data) || data.starts_with(&real) {
        bail!(
            "{} is inside the data directory {} or holds it: a backup goes somewhere else",
            dest.display(),
            data.display()
        );
    }
    match fs::symlink_metadata(&real) {
        Ok(m) if m.file_type().is_symlink() => bail!(
            "{} is a symbolic link: give the folder it points to, or a new one",
            dest.display()
        ),
        Ok(m) if !m.is_dir() => bail!("{} exists and is not a folder", dest.display()),
        Ok(m) => {
            if m.uid() != nix::unistd::geteuid().as_raw() {
                bail!("{} belongs to another user", dest.display());
            }
            if fs::read_dir(&real)?.next().is_some() {
                bail!(
                    "{} is not empty: a backup goes into a new or empty folder",
                    dest.display()
                );
            }
            fs::set_permissions(&real, fs::Permissions::from_mode(0o700))?;
            Ok((real, false))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&real)
                .with_context(|| format!("creating {}", dest.display()))?;
            Ok((real, true))
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", dest.display())),
    }
}

/// The hard links of one backup, removed when dropped (every way out but
/// SIGKILL; a leftover is removed by the next backup, and the server never
/// reads it: it is outside `blobs/`).
struct Staging {
    dir: PathBuf,
    _lock: File,
}

impl Staging {
    fn open(data: &Path) -> anyhow::Result<Self> {
        let lp = data.join(STAGING_LOCK);
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(&lp)
            .with_context(|| format!("opening {}", lp.display()))?;
        match f.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => bail!(
                "another backup of {} is running: one at a time",
                data.display()
            ),
            // No file locks here: nothing to coordinate with, carry on.
            Err(fs::TryLockError::Error(_)) => {}
        }
        let dir = data.join(STAGING);
        // A leftover of a backup that was killed.
        match fs::symlink_metadata(&dir) {
            Ok(m) if m.is_dir() => fs::remove_dir_all(&dir)?,
            Ok(_) => fs::remove_file(&dir)?,
            Err(_) => {}
        }
        fs::DirBuilder::new().mode(0o700).create(&dir)?;
        Ok(Self { dir, _lock: f })
    }

    /// Empty the links of an attempt that found files missing.
    fn reset(&self) -> io::Result<()> {
        let b = self.dir.join("blobs");
        if fs::symlink_metadata(&b).is_ok() {
            fs::remove_dir_all(&b)?;
        }
        Ok(())
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// What the backup wrote, removed again unless it finished.
struct Written {
    dest: PathBuf,
    made_dest: bool,
    top: BTreeSet<PathBuf>,
    done: bool,
}

impl Written {
    fn note(&mut self, p: &Path) {
        if let Ok(rel) = p.strip_prefix(&self.dest)
            && let Some(first) = rel.components().next()
        {
            self.top.insert(self.dest.join(first));
        }
    }
}

impl Drop for Written {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if self.made_dest {
            let _ = fs::remove_dir_all(&self.dest);
            return;
        }
        for p in &self.top {
            match fs::symlink_metadata(p) {
                Ok(m) if m.is_dir() => {
                    let _ = fs::remove_dir_all(p);
                }
                Ok(_) => {
                    let _ = fs::remove_file(p);
                }
                Err(_) => {}
            }
        }
    }
}

/// A file being written under its temporary name.
struct Pending {
    tmp: PathBuf,
    path: String,
    /// Size and SHA-256 when the copy already hashed it.
    hashed: Option<(u64, String)>,
}

fn tmp_name(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!(".{name}.partial"))
}

/// The pause before the first links (debug builds only): the tests delete
/// a file or add events between the copy of `master.db` and the links.
#[cfg(debug_assertions)]
fn test_pause() {
    let Ok(p) = std::env::var("HARNESS_BACKUP_PAUSE_FILE") else {
        return;
    };
    let go = PathBuf::from(&p);
    let _ = fs::write(format!("{p}.snapshotted"), b"");
    for _ in 0..3000 {
        if go.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(not(debug_assertions))]
fn test_pause() {}

/// The result, for the lines the command prints.
pub struct Done {
    pub online: bool,
    pub databases: usize,
    pub blobs: u64,
    pub others: usize,
    pub bytes: u64,
    pub attempts: u32,
    /// Chats begun during the backup, left out of it.
    pub left_out: usize,
    pub manifest_sha256: String,
    pub skipped: Vec<String>,
    pub secs: f64,
}

/// Back up `data` into `dest`.
pub fn backup(data: &Path, dest: &Path, cancel: &AtomicBool) -> anyhow::Result<Done> {
    let started = Instant::now();
    if !databases().iter().any(|(n, _)| data.join(n).exists()) {
        bail!(
            "{} holds no rizzy-harness database: is --data-dir right?",
            data.display()
        );
    }
    let (dest, made_dest) = prepare_dest(data, dest)?;
    let mut w = Written {
        dest: dest.clone(),
        made_dest,
        top: BTreeSet::new(),
        done: false,
    };
    // Free: no server runs and none can start until this ends. Held:
    // beside a running server (the steps are the same).
    let held = lock::try_take(data, "backup")?;
    let online = held.is_none();
    let staging = Staging::open(data)?;
    let mut buf = vec![0u8; BUF];
    let mut pending: Vec<Pending> = Vec::new();
    let mut versions = BTreeMap::new();

    // 1 and 2: master.db and the links to the files it names.
    let master_tmp = tmp_name(&dest, "master.db");
    let mut attempts = 0;
    let mut blob_ids: Vec<String> = Vec::new();
    if data.join("master.db").exists() {
        loop {
            attempts += 1;
            check_cancel(cancel)?;
            staging.reset()?;
            if fs::symlink_metadata(&master_tmp).is_ok() {
                fs::remove_file(&master_tmp)?;
            }
            w.note(&master_tmp);
            drop(create_private(&master_tmp)?);
            db::snapshot(&data.join("master.db"), &master_tmp, !online)
                .context("copying master.db")?;
            if attempts == 1 {
                test_pause();
            }
            blob_ids = db::blob_ids(&master_tmp).map_err(|e| anyhow::anyhow!("master.db: {e}"))?;
            let root = data.join("blobs");
            let mut missing = 0u64;
            for id in &blob_ids {
                check_cancel(cancel)?;
                let src = harness_master::blobs::path(&root, id)
                    .with_context(|| "master.db names a stored file with a malformed id")?;
                let dst = harness_master::blobs::path(&staging.dir.join("blobs"), id)
                    .with_context(|| "malformed id")?;
                mkdir_private(&staging.dir.join("blobs"))?;
                mkdir_private(dst.parent().unwrap_or(&staging.dir))?;
                match fs::hard_link(&src, &dst) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => missing += 1,
                    // No links here (another mount, a file system without
                    // them): a copy, which widens the window the retry
                    // covers.
                    Err(_) => match open_regular(&src) {
                        Ok((mut f, _)) => {
                            let mut out = create_private(&dst)?;
                            copy_stream(&mut f, &mut out, &mut buf, cancel)?;
                        }
                        Err(e) if e.kind() == io::ErrorKind::NotFound => missing += 1,
                        Err(e) => return Err(e).context("reading a stored file"),
                    },
                }
            }
            if missing == 0 {
                break;
            }
            if attempts >= ATTEMPTS {
                bail!(
                    "{missing} stored files were deleted between the copy of master.db and \
                     their links, {ATTEMPTS} times in a row: try again, or stop the server and \
                     back up then; nothing was kept"
                );
            }
        }
        pending.push(Pending {
            tmp: master_tmp.clone(),
            path: "master.db".into(),
            hashed: None,
        });
    }

    // 3: the other databases, after master.db.
    for (name, _) in databases().iter().skip(1) {
        check_cancel(cancel)?;
        let src = data.join(name);
        if !src.exists() {
            continue;
        }
        let tmp = tmp_name(&dest, name);
        w.note(&tmp);
        drop(create_private(&tmp)?);
        db::snapshot(&src, &tmp, !online).with_context(|| format!("copying {name}"))?;
        pending.push(Pending {
            tmp,
            path: (*name).into(),
            hashed: None,
        });
    }
    for (name, schema) in databases() {
        if let Some(p) = pending.iter().find(|p| p.path == name) {
            versions.insert(name.to_string(), check_db(name, &p.tmp, schema)?);
        }
    }
    // Chats begun after master.db's copy would come back without their
    // owner: they are left out (docs/backup-restore.md).
    let mut left_out = 0;
    let mut skipped = Vec::new();
    if versions.contains_key("master.db") {
        let current = harness_store::DB_SCHEMA.steps.len() as i64;
        for name in ["chat.db", "node.db"] {
            let Some(p) = pending.iter().find(|p| p.path == name) else {
                continue;
            };
            if versions.get(name) == Some(&current) {
                left_out += db::drop_unknown_chats(&p.tmp, &master_tmp)
                    .with_context(|| format!("{name}: leaving out chats begun meanwhile"))?;
                check_db(name, &p.tmp, &harness_store::DB_SCHEMA)?;
            } else {
                skipped.push(format!(
                    "nothing of {name}: it is at another schema version than this build; chats \
                     begun during the backup are in it without their owner (back up with the \
                     server's own build to avoid this)"
                ));
            }
        }
    }

    // 4: plain files, keys/, agents/.
    let copy_in = |src: &Path,
                   dir: &Path,
                   file: &str,
                   path: String,
                   w: &mut Written,
                   pending: &mut Vec<Pending>,
                   buf: &mut [u8]|
     -> anyhow::Result<Option<String>> {
        let (mut f, _) = match open_regular(src) {
            Ok(v) => v,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Ok(Some(format!("{path} (a link or not a regular file)"))),
        };
        mkdir_private(dir)?;
        let tmp = tmp_name(dir, file);
        w.note(&tmp);
        let mut out = create_private(&tmp)?;
        let hashed = copy_stream(&mut f, &mut out, buf, cancel)?;
        out.sync_all()?;
        pending.push(Pending {
            tmp,
            path,
            hashed: Some(hashed),
        });
        Ok(None)
    };
    for name in PLAIN {
        if let Some(s) = copy_in(
            &data.join(name),
            &dest,
            name,
            (*name).into(),
            &mut w,
            &mut pending,
            &mut buf,
        )? {
            skipped.push(s);
        }
    }
    for sub in ["keys", "agents"] {
        let Ok(m) = fs::symlink_metadata(data.join(sub)) else {
            continue;
        };
        if !m.is_dir() {
            skipped.push(format!("{sub}/ (not a folder)"));
            continue;
        }
        let mut names: Vec<String> = fs::read_dir(data.join(sub))?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for n in names {
            if !valid_name(&n) {
                skipped.push(format!("{sub}/{n:?} (a name a backup does not keep)"));
                continue;
            }
            if let Some(s) = copy_in(
                &data.join(sub).join(&n),
                &dest.join(sub),
                &n,
                format!("{sub}/{n}"),
                &mut w,
                &mut pending,
                &mut buf,
            )? {
                skipped.push(s);
            }
        }
    }

    // 5: the stored files, copied for real from the links.
    for id in &blob_ids {
        check_cancel(cancel)?;
        let staged =
            harness_master::blobs::path(&staging.dir.join("blobs"), id).context("malformed id")?;
        let to = harness_master::blobs::path(&dest.join("blobs"), id).context("malformed id")?;
        let shard = to.parent().context("no shard")?.to_path_buf();
        mkdir_private(&dest.join("blobs"))?;
        mkdir_private(&shard)?;
        let file = format!("{id}.rzb");
        let tmp = tmp_name(&shard, &file);
        w.note(&tmp);
        // Read only: the link shares the live file's inode.
        let (mut f, _) = open_regular(&staged)?;
        let mut out = create_private(&tmp)?;
        let hashed = copy_stream(&mut f, &mut out, &mut buf, cancel)?;
        out.sync_all()?;
        pending.push(Pending {
            tmp,
            path: format!("blobs/{}/{file}", &id[30..]),
            hashed: Some(hashed),
        });
    }
    drop(staging);
    if !blob_ids.is_empty() {
        let by_path: BTreeMap<&str, &Pending> =
            pending.iter().map(|p| (p.path.as_str(), p)).collect();
        verify_blobs(
            &master_tmp,
            |id| {
                by_path
                    .get(format!("blobs/{}/{id}.rzb", &id[30..]).as_str())
                    .map(|p| p.tmp.clone())
                    .unwrap_or_default()
            },
            cancel,
        )?;
    }

    // Hash the databases (the other files were hashed as they were
    // copied), then rename every file to its name and sync the folders.
    let mut files = Vec::new();
    let mut dirs = BTreeSet::new();
    let mut bytes = 0u64;
    for p in &pending {
        check_cancel(cancel)?;
        let (size, sha) = match &p.hashed {
            Some(h) => h.clone(),
            None => {
                let (mut f, _) = open_regular(&p.tmp)?;
                f.sync_all()?;
                copy_stream(&mut f, &mut io::sink(), &mut buf, cancel)?
            }
        };
        bytes += size;
        files.push(Entry {
            path: p.path.clone(),
            size,
            sha256: sha,
        });
    }
    for p in &pending {
        let to = dest.join(&p.path);
        fs::rename(&p.tmp, &to)?;
        if let Some(d) = to.parent() {
            dirs.insert(d.to_path_buf());
        }
    }
    for d in dirs.iter().rev() {
        sync_dir(d)?;
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let manifest = Manifest {
        format: FORMAT,
        harness: env!("CARGO_PKG_VERSION").to_string(),
        created_ms: harness_core::now_ms(),
        online,
        databases: versions,
        files,
    };
    let text = serde_json::to_vec_pretty(&manifest)?;
    let tmp = tmp_name(&dest, MANIFEST);
    w.note(&tmp);
    let mut out = create_private(&tmp)?;
    out.write_all(&text)?;
    out.sync_all()?;
    drop(out);
    check_cancel(cancel)?;
    fs::rename(&tmp, dest.join(MANIFEST))?;
    sync_dir(&dest)?;
    w.done = true;
    let n_db = manifest.databases.len();
    Ok(Done {
        online,
        databases: n_db,
        blobs: blob_ids.len() as u64,
        others: pending.len() - n_db - blob_ids.len(),
        bytes,
        attempts,
        left_out,
        manifest_sha256: hex::encode(Sha256::digest(&text)),
        skipped,
        secs: started.elapsed().as_secs_f64(),
    })
}

/// The lines `backup` prints: what, where, the manifest's hash; never what
/// a file holds.
pub fn print_backup(given: &Path, d: &Done) {
    println!(
        "backed up to {} ({})",
        given.display(),
        if d.online {
            "beside the running server"
        } else {
            "no server was running; the data directory was locked meanwhile"
        }
    );
    println!(
        "{} databases, {} stored files, {} other files: {} in {:.1} s{}",
        d.databases,
        d.blobs,
        d.others,
        mb(d.bytes),
        d.secs,
        if d.attempts > 1 {
            format!(
                " ({} copies of master.db: files were deleted meanwhile)",
                d.attempts
            )
        } else {
            String::new()
        }
    );
    if d.left_out > 0 {
        println!(
            "{} chats begun while the backup ran are not in it (they began after master.db \
             was copied)",
            d.left_out
        );
    }
    for s in &d.skipped {
        println!("skipped {s}");
    }
    println!(
        "{MANIFEST} SHA-256 {} (note it: `backup --verify` checks the backup against it)",
        d.manifest_sha256
    );
    println!(
        "This folder holds every account, chat, stored file and secret of this server, the API \
         token among them, unencrypted: keep it as you keep the data directory (README, \"Back \
         up and restore\")."
    );
    println!("{NOT_INCLUDED}");
}

// ------------------------------------------------------- manifest, verify

/// Every folder from `root` down to `rel`'s parent is a real folder (no
/// link anywhere on the way).
fn no_links_on_the_way(root: &Path, rel: &str) -> anyhow::Result<()> {
    let mut at = root.to_path_buf();
    let parts: Vec<&str> = rel.split('/').collect();
    for part in &parts[..parts.len() - 1] {
        at.push(part);
        let m = fs::symlink_metadata(&at)?;
        if !m.is_dir() {
            bail!("{rel}: {} in the backup is not a folder", part);
        }
    }
    Ok(())
}

/// Read and check `MANIFEST.json` of `backup`: the format, schema versions
/// this build knows, every path in the fixed shape, no path twice, every
/// database listed.
pub fn read_manifest(backup: &Path) -> anyhow::Result<(Manifest, String)> {
    let m =
        fs::symlink_metadata(backup).with_context(|| format!("reading {}", backup.display()))?;
    if !m.is_dir() {
        bail!("{} is not a folder", backup.display());
    }
    let p = backup.join(MANIFEST);
    let (mut f, md) = match open_regular(&p) {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::NotFound => bail!(
            "{} has no {MANIFEST}: it is not a finished backup (a backup that was interrupted \
             has none); nothing was restored",
            backup.display()
        ),
        Err(e) => return Err(e).with_context(|| format!("reading {}", p.display())),
    };
    if md.len() > MANIFEST_MAX {
        bail!("{} is too large to be a manifest", p.display());
    }
    let mut text = Vec::new();
    f.read_to_end(&mut text)?;
    let sha = hex::encode(Sha256::digest(&text));
    let man: Manifest = serde_json::from_slice(&text)
        .map_err(|_| anyhow::anyhow!("{} is not a manifest this build reads", p.display()))?;
    if man.format > FORMAT {
        bail!(
            "the backup's format is {}; this build reads format {FORMAT}: restore it with the \
             build that made it or a newer one",
            man.format
        );
    }
    let mut seen = BTreeSet::new();
    for e in &man.files {
        if !valid_path(&e.path) {
            bail!(
                "the manifest names a path a backup never holds ({:?}): refused, nothing was \
                 restored",
                e.path.chars().take(80).collect::<String>()
            );
        }
        if !seen.insert(e.path.as_str()) {
            bail!("the manifest names {} twice", e.path);
        }
        if e.sha256.len() != 64 || !e.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("the manifest's hash of {} is malformed", e.path);
        }
    }
    let known: BTreeMap<&str, i64> = databases()
        .iter()
        .map(|(n, s)| (*n, s.steps.len() as i64))
        .collect();
    for (name, v) in &man.databases {
        let Some(k) = known.get(name.as_str()) else {
            bail!("the manifest names an unknown database");
        };
        if v > k {
            bail!(
                "{name} in the backup has schema version {v}; this build knows up to {k}: \
                 restore it with the build that made it or a newer one"
            );
        }
        if !seen.contains(name.as_str()) {
            bail!("the manifest gives {name} a version but does not list the file");
        }
    }
    for (name, _) in databases() {
        if seen.contains(name) && !man.databases.contains_key(name) {
            bail!("the manifest lists {name} without its version");
        }
    }
    Ok((man, sha))
}

/// Files under `backup` the manifest does not list (not followed into
/// links): how many.
fn unlisted(backup: &Path, man: &Manifest) -> u64 {
    let listed: BTreeSet<PathBuf> = man.files.iter().map(|e| backup.join(&e.path)).collect();
    let mut n = 0;
    let mut stack = vec![backup.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            match fs::symlink_metadata(&p) {
                Ok(m) if m.is_dir() => stack.push(p),
                Ok(_) => {
                    if p != backup.join(MANIFEST) && !listed.contains(&p) {
                        n += 1;
                    }
                }
                Err(_) => {}
            }
        }
    }
    n
}

/// Open the backup's file of `e`, checked: no link on the way or at it, a
/// regular file of the manifest's size.
fn open_entry(backup: &Path, e: &Entry) -> anyhow::Result<File> {
    no_links_on_the_way(backup, &e.path).map_err(|err| {
        anyhow::anyhow!("{}: {err}; the backup is damaged or was changed", e.path)
    })?;
    let (f, m) = open_regular(&backup.join(&e.path)).map_err(|err| {
        anyhow::anyhow!(
            "{}: {} (missing, a link or not a regular file): the backup is damaged or was \
             changed",
            e.path,
            err.kind()
        )
    })?;
    if m.len() != e.size {
        bail!(
            "{}: the backup's file is {} bytes, its manifest says {}: the backup is damaged",
            e.path,
            m.len(),
            e.size
        );
    }
    Ok(f)
}

fn mismatch(path: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{path}: its content does not match the manifest (SHA-256 differs): the backup is \
         damaged or was changed"
    )
}

/// `backup --verify <dir>`: every file against the manifest, every
/// database's integrity and schema, every stored file decrypted against
/// its row. Nothing is written.
pub fn verify(backup: &Path, cancel: &AtomicBool) -> anyhow::Result<()> {
    let started = Instant::now();
    let (man, sha) = read_manifest(backup)?;
    let mut buf = vec![0u8; BUF];
    let mut bytes = 0;
    for e in &man.files {
        let mut f = open_entry(backup, e)?;
        let (n, h) = copy_stream(&mut f, &mut io::sink(), &mut buf, cancel)?;
        if n != e.size || h != e.sha256 {
            return Err(mismatch(&e.path));
        }
        bytes += n;
    }
    for (name, schema) in databases() {
        if man.databases.contains_key(name) {
            let v = check_db(name, &backup.join(name), schema)?;
            if Some(&v) != man.databases.get(name) {
                bail!("{name}: its schema version is not the manifest's: the backup was changed");
            }
        }
    }
    let mut blobs = 0;
    if man.databases.contains_key("master.db") {
        let listed: BTreeSet<&str> = man.files.iter().map(|e| e.path.as_str()).collect();
        for id in db::blob_ids(&backup.join("master.db")).map_err(|e| anyhow::anyhow!(e))? {
            if !listed.contains(format!("blobs/{}/{id}.rzb", &id[30..]).as_str()) {
                bail!("stored file {id} is named by master.db but missing from the backup");
            }
        }
        blobs = verify_blobs(
            &backup.join("master.db"),
            |id| {
                backup
                    .join("blobs")
                    .join(&id[30..])
                    .join(format!("{id}.rzb"))
            },
            cancel,
        )?;
    }
    let extra = unlisted(backup, &man);
    println!(
        "{} is a complete backup: {} files ({}) match its manifest, {} databases pass their \
         checks, {blobs} stored files open and match their rows ({:.1} s)",
        backup.display(),
        man.files.len(),
        mb(bytes),
        man.databases.len(),
        started.elapsed().as_secs_f64()
    );
    if extra > 0 {
        println!("{extra} files in it are not in its manifest: a restore ignores them");
    }
    println!("{MANIFEST} SHA-256 {sha}");
    Ok(())
}

// --------------------------------------------------------------- restore

/// What restore found and did.
pub struct Restored {
    pub files: usize,
    pub bytes: u64,
    pub ignored: u64,
    pub changes: db::Restored,
    pub had_master: bool,
    pub secs: f64,
}

/// Restore `backup` into the data directory `data`, which the caller holds
/// locked and which must be empty (nothing but the lock file: even a
/// `lost+found` makes it not empty).
pub fn restore(
    data: &Path,
    backup: &Path,
    pause_routines: bool,
    cancel: &AtomicBool,
) -> anyhow::Result<Restored> {
    let started = Instant::now();
    let others = fs::read_dir(data)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() != lock::NAME)
        .count();
    if others > 0 {
        bail!(
            "{} is not empty ({others} entries): a restore goes into a new or empty data \
             directory (anything in it, even lost+found, counts); give another --data-dir",
            data.display()
        );
    }
    let backup = backup
        .canonicalize()
        .with_context(|| format!("reading {}", backup.display()))?;
    if backup.starts_with(data) || data.starts_with(&backup) {
        bail!("the backup and the data directory must not hold each other");
    }
    let (man, _) = read_manifest(&backup)?;
    let marker = data.join(RESTORING);
    drop(create_private(&marker).with_context(|| format!("creating {}", marker.display()))?);
    sync_dir(data)?;
    let mut guard = Undo {
        data: data.to_path_buf(),
        done: false,
    };
    let mut buf = vec![0u8; BUF];
    let mut bytes = 0;
    let mut dirs = BTreeSet::new();
    for e in &man.files {
        check_cancel(cancel)?;
        let mut f = open_entry(&backup, e)?;
        let to = data.join(&e.path);
        let parent = to.parent().context("no parent")?.to_path_buf();
        let mut d = data.to_path_buf();
        for part in parent
            .strip_prefix(data)
            .unwrap_or(Path::new(""))
            .components()
        {
            d.push(part);
            mkdir_private(&d)?;
        }
        let mut out = create_private(&to).with_context(|| format!("creating {}", e.path))?;
        let (n, h) = copy_stream(&mut f, &mut out, &mut buf, cancel)?;
        if n != e.size || h != e.sha256 {
            return Err(mismatch(&e.path));
        }
        out.sync_all()?;
        bytes += n;
        dirs.insert(parent);
    }
    for (name, schema) in databases() {
        if let Some(v) = man.databases.get(name) {
            let found = check_db(name, &data.join(name), schema)?;
            if found != *v {
                bail!("{name}: its schema version is not the manifest's: the backup was changed");
            }
        }
    }
    let had_master = man.databases.contains_key("master.db");
    let changes = if had_master {
        db::on_restore(&data.join("master.db"), pause_routines)
            .context("dropping the logins of the restored master.db")?
    } else {
        db::Restored::default()
    };
    for d in dirs.iter().rev() {
        sync_dir(d)?;
    }
    check_cancel(cancel)?;
    fs::remove_file(&marker)?;
    sync_dir(data)?;
    guard.done = true;
    Ok(Restored {
        files: man.files.len(),
        bytes,
        ignored: unlisted(&backup, &man),
        changes,
        had_master,
        secs: started.elapsed().as_secs_f64(),
    })
}

/// A restore that did not finish empties the directory again (the lock
/// file stays); if that fails, `RESTORING` stays and keeps the server off.
struct Undo {
    data: PathBuf,
    done: bool,
}

impl Drop for Undo {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let Ok(rd) = fs::read_dir(&self.data) else {
            return;
        };
        let mut ok = true;
        for e in rd.flatten() {
            let n = e.file_name();
            if n == lock::NAME || n == RESTORING {
                continue;
            }
            let p = e.path();
            let r = match fs::symlink_metadata(&p) {
                Ok(m) if m.is_dir() => fs::remove_dir_all(&p),
                Ok(_) => fs::remove_file(&p),
                Err(e) => Err(e),
            };
            ok &= r.is_ok();
        }
        if ok {
            let _ = fs::remove_file(self.data.join(RESTORING));
        }
    }
}

pub fn print_restore(data: &Path, r: &Restored) {
    println!(
        "restored {} files ({}) into {} in {:.1} s",
        r.files,
        mb(r.bytes),
        data.display(),
        r.secs
    );
    if r.ignored > 0 {
        println!(
            "{} files in the backup are not in its manifest and were not restored",
            r.ignored
        );
    }
    if r.had_master {
        println!(
            "{} logins were dropped: everyone signs in again (the API token is the backup's)",
            r.changes.logins
        );
        if r.changes.routines > 0 {
            println!(
                "{} routines were paused (--pause-routines)",
                r.changes.routines
            );
        }
        println!(
            "This copy is the same server: the same accounts, API token, node keys and join \
             tokens. Do not run it beside the original: remote nodes accept either, and every \
             routine runs on both (one with catch_up = once runs late at the first start){}.",
            if r.changes.routines > 0 {
                ""
            } else {
                "; --pause-routines restores with every routine paused"
            }
        );
    }
    println!("{NOT_INCLUDED}");
}

/// Run `f` off the async runtime; SIGINT or SIGTERM asks it to stop (it
/// removes what it wrote and ends with "interrupted").
pub async fn run_stoppable<T: Send + 'static>(
    f: impl FnOnce(&AtomicBool) -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    let mut task = tokio::task::spawn_blocking(move || f(&c));
    let mut signals = crate::stop::Signals::install().context("catching stop signals")?;
    tokio::select! {
        r = &mut task => r.context("the command's task ended")?,
        name = signals.first() => {
            eprintln!("{name}: stopping; removing what was written");
            cancel.store(true, Ordering::SeqCst);
            task.await.context("the command's task ended")?
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_fixed_shapes_are_paths() {
        for ok in [
            "master.db",
            "node.toml",
            "api-token",
            "keys/secrets.key",
            "keys/ui-cert.pem",
            "agents/reviewer.md",
            "blobs/aa/0123456789abcdef0123456789abcdaa.rzb",
        ] {
            assert!(valid_path(ok), "{ok}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "keys/../master.db",
            "keys/.hidden",
            "keys/a/b",
            "agents/",
            "./master.db",
            "master.db/",
            "lock",
            "MANIFEST.json",
            "RESTORING",
            "claim-code",
            "checkpoints/x.git",
            "blobs/ab/0123456789abcdef0123456789abcdaa.rzb",
            "blobs/aa/0123456789ABCDEF0123456789abcdaa.rzb",
            "blobs/aa/0123456789abcdef0123456789abcdaa.rzb/x",
            "blobs/tmp/x",
            "keys/a\nb",
        ] {
            assert!(!valid_path(bad), "{bad:?}");
        }
    }

    /// A 2 GB sparse file streams through the fixed buffer: the peak
    /// resident memory grows by far less than the file.
    #[test]
    fn a_large_file_streams_through_the_buffer() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("big");
        let f = File::create(&p).unwrap();
        f.set_len(2 << 30).unwrap();
        drop(f);
        let hwm = || -> u64 {
            let s = fs::read_to_string("/proc/self/status").unwrap();
            s.lines()
                .find_map(|l| l.strip_prefix("VmHWM:"))
                .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
                .unwrap()
        };
        let before = hwm();
        let mut buf = vec![0u8; BUF];
        let (mut src, _) = open_regular(&p).unwrap();
        let (n, _) =
            copy_stream(&mut src, &mut io::sink(), &mut buf, &AtomicBool::new(false)).unwrap();
        assert_eq!(n, 2 << 30);
        let grew_kb = hwm().saturating_sub(before);
        assert!(grew_kb < 64 * 1024, "peak RSS grew {grew_kb} kB");
    }

    #[test]
    fn a_stop_ends_a_copy() {
        let mut buf = vec![0u8; 16];
        let e = copy_stream(
            &mut io::repeat(1),
            &mut io::sink(),
            &mut buf,
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert_eq!(e.to_string(), "interrupted");
    }
}
