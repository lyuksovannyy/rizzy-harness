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
pub(crate) fn databases() -> [(
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
variables; pre-migration/ (the copy of older databases kept for rolling an upgrade back).";

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
    /// Items that were symbolic links on the source and are plain files
    /// here (their names only: where they pointed is not recorded).
    #[serde(default)]
    pub links: Vec<String>,
    /// What the source had that this backup does not hold, in words.
    #[serde(default)]
    pub skipped: Vec<String>,
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
pub(crate) fn create_private(p: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(p)
}

/// Make folder `p` 0700 (it may exist already as a real folder of ours).
pub(crate) fn mkdir_private(p: &Path) -> io::Result<()> {
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

pub(crate) fn sync_dir(p: &Path) -> io::Result<()> {
    File::open(p)?.sync_all()
}

/// What a path in the data directory or a backup is, without opening it.
#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Missing,
    Regular,
    Link,
    /// A FIFO, a device, a socket or a folder.
    Other,
}

fn kind(p: &Path) -> io::Result<Kind> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.file_type().is_symlink() => Ok(Kind::Link),
        Ok(m) if m.is_file() => Ok(Kind::Regular),
        Ok(_) => Ok(Kind::Other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Kind::Missing),
        Err(e) => Err(e),
    }
}

/// Open a file to read, and only a regular file: never through a link,
/// never a FIFO or device (judged before the open, so opening one blocks
/// nothing and touches no device; the open itself is non-blocking and
/// makes no terminal ours, and the file opened must be the one judged).
pub(crate) fn open_regular(p: &Path) -> io::Result<(File, fs::Metadata)> {
    let not_regular = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file (a link, a FIFO, a device or a socket)",
        )
    };
    let before = fs::symlink_metadata(p)?;
    if !before.is_file() {
        return Err(not_regular());
    }
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(
            nix::libc::O_NOFOLLOW
                | nix::libc::O_CLOEXEC
                | nix::libc::O_NONBLOCK
                | nix::libc::O_NOCTTY,
        )
        .open(p)?;
    let m = f.metadata()?;
    if !m.is_file() || m.dev() != before.dev() || m.ino() != before.ino() {
        return Err(not_regular());
    }
    Ok((f, m))
}

/// The most a file of the fixed list (configuration, keys, agents) may be.
pub const ITEM_CAP: u64 = 16 << 20;

/// Open one item of the fixed list, following a symbolic link one level:
/// `None` when it is absent; else the file, its length and whether it was
/// a link. Its target must be a regular file within [`ITEM_CAP`], judged
/// before it is opened (non-blocking) and checked again once open; anything
/// else fails the backup naming the item, never its target's path.
fn open_item(p: &Path, name: &str) -> anyhow::Result<Option<(File, u64, bool)>> {
    let lm = match fs::symlink_metadata(p) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {name}")),
    };
    let link = lm.file_type().is_symlink();
    let m = if link {
        match fs::metadata(p) {
            Ok(m) => m,
            Err(_) => bail!(
                "{name} is a symbolic link whose target is missing or cannot be read: the \
                 backup would lack it, so nothing was backed up; fix the link or remove it"
            ),
        }
    } else {
        lm
    };
    let through = if link {
        " (through its symbolic link)"
    } else {
        ""
    };
    if !m.is_file() {
        bail!(
            "{name}{through} is not a regular file (a FIFO, a device, a socket or a folder): \
             nothing was backed up; remove it, or put the real file back"
        );
    }
    if m.len() > ITEM_CAP {
        bail!(
            "{name}{through} is {} bytes, more than the {} MiB a configuration file may be: \
             nothing was backed up",
            m.len(),
            ITEM_CAP >> 20
        );
    }
    let mut flags = nix::libc::O_CLOEXEC | nix::libc::O_NONBLOCK | nix::libc::O_NOCTTY;
    if !link {
        flags |= nix::libc::O_NOFOLLOW;
    }
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(p)
        .with_context(|| format!("reading {name}"))?;
    let o = f.metadata()?;
    if !o.is_file() || o.dev() != m.dev() || o.ino() != m.ino() {
        bail!("{name}{through} changed while it was read: nothing was backed up; try again");
    }
    Ok(Some((f, m.len(), link)))
}

/// The top-level names of a data directory a backup knows: kept, or left
/// out by design (README "Back up and restore").
fn known_top(n: &str) -> bool {
    const LEFT_OUT: &[&str] = &[
        "blobs",
        "lock",
        STAGING_LOCK,
        STAGING,
        RESTORING,
        "claim-code",
        "checkpoints",
        "graphify",
        "sandbox-tmp",
        "uploads-stage",
        "artifacts-stage",
        "projects-staging",
        "keys",
        "agents",
        "update-pending.json",
        crate::premigration::SET,
        crate::premigration::SET_PARTIAL,
        crate::premigration::SET_OLD,
        "update-failed",
        "rizzy-harness.prev",
    ];
    if LEFT_OUT.contains(&n) || PLAIN.contains(&n) || n.contains(crate::premigration::ASIDE) {
        return true;
    }
    databases().iter().any(|(d, _)| {
        n == *d
            || ["-wal", "-shm", "-journal", harness_store::TRUNCATE_PENDING]
                .iter()
                .any(|s| n == format!("{d}{s}"))
    })
}

/// A database and the files SQLite opens beside it: each absent or a
/// regular file (SQLite's own open would block on a FIFO).
pub(crate) fn db_files_regular(dir: &Path, name: &str) -> anyhow::Result<()> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let f = format!("{name}{suffix}");
        match kind(&dir.join(&f))? {
            Kind::Missing | Kind::Regular => {}
            Kind::Link => bail!(
                "{f} in {} is a symbolic link: a database behind a link has its -wal and -shm \
                 beside the target, not beside the link, so a copy could miss what they hold; \
                 nothing was read; point --data-dir at the real folder or put the file itself \
                 there",
                dir.display()
            ),
            Kind::Other => bail!(
                "{f} in {} is not a regular file (a link, a FIFO, a device or a socket): \
                 nothing was read from it; remove it, or put the real file back",
                dir.display()
            ),
        }
    }
    Ok(())
}

/// Stream at most `size` bytes of `from` into `to` (one more is read, to
/// refuse a file that grew past what its manifest says), hashing.
pub fn copy_declared(
    from: &mut impl Read,
    to: &mut impl Write,
    size: u64,
    buf: &mut [u8],
    cancel: &AtomicBool,
) -> anyhow::Result<(u64, String)> {
    let (n, h) = copy_stream(&mut from.take(size.saturating_add(1)), to, buf, cancel)?;
    if n > size {
        bail!("it grew past the {size} bytes its manifest says while it was read");
    }
    Ok((n, h))
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

pub(crate) fn mb(n: u64) -> String {
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
pub(crate) fn check_db(
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
    if real.starts_with(data) || data.starts_with(&real) || aliased(data, &real)? {
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

/// Free bytes for this user on `p`'s file system (the field types differ
/// between platforms, hence the casts).
#[allow(clippy::unnecessary_cast)]
pub(crate) fn free_bytes(p: &Path) -> nix::Result<u64> {
    // Tests only (debug builds): a disk this full.
    #[cfg(debug_assertions)]
    if let Some(n) = std::env::var("HARNESS_TEST_FREE_BYTES")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        return Ok(n);
    }
    let v = nix::sys::statvfs::statvfs(p)?;
    Ok((v.blocks_available() as u64).saturating_mul(v.fragment_size() as u64))
}

/// Refuse an offline backup whose copies would not fit beside the data:
/// `need` bytes of database files, a margin of 64 MiB or a tenth more.
fn room_for(need: u64, free: u64) -> anyhow::Result<()> {
    let want = need.saturating_add((need / 10).max(64 << 20));
    if free < want {
        bail!(
            "an offline backup first copies the databases ({}) beside the data, and the data \
             directory's disk has {} free, less than the {} it needs with a margin: free some \
             space there, or back up while the server runs (which needs no such copy)",
            mb(need),
            mb(free),
            mb(want)
        );
    }
    Ok(())
}

/// Whether `<db>-shm` exists and this process may write it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shm {
    Missing,
    Writable,
    ReadOnly,
}

fn shm_state(db: &Path) -> Shm {
    let mut p = db.as_os_str().to_owned();
    p.push("-shm");
    let p = PathBuf::from(p);
    if fs::symlink_metadata(&p).is_err() {
        return Shm::Missing;
    }
    match nix::unistd::access(&p, nix::unistd::AccessFlags::W_OK) {
        Ok(()) => Shm::Writable,
        Err(_) => Shm::ReadOnly,
    }
}

/// The refusal when SQLite will not let a read-only reader read `name`
/// beside a lock holder, in what is true of each case.
fn needs_writer_sentence(name: &str, holder: &str, shm: Shm, had_shm: bool) -> String {
    let cause = match shm {
        Shm::ReadOnly => format!(
            "{name}-shm, the database's shared-memory file, cannot be written by this user, so \
             SQLite cannot read the database's log through it"
        ),
        _ => format!(
            "{name} needs SQLite's recovery, which only a writer may make, and the data \
             directory is {holder}"
        ),
    };
    let made = if !had_shm && shm != Shm::Missing {
        format!(
            "; nothing else was written, but the reader made {name}-shm (an empty index SQLite rebuilds; it may be deleted while nothing holds the directory)"
        )
    } else {
        "; nothing of the database or its log was written".into()
    };
    format!(
        "{cause}{made}. Stop whatever holds the directory and back up then: a backup that \
         holds the lock reads a private copy and needs no writer"
    )
}

/// The identity of a folder.
fn id_of(p: &Path) -> io::Result<(u64, u64)> {
    let m = fs::metadata(p)?;
    Ok((m.dev(), m.ino()))
}

/// Whether `dest` lies inside `data`, or holds it, by the folders' own
/// identities (device and inode) rather than their names: a bind mount of
/// the data directory under another name is caught too.
fn aliased(data: &Path, dest: &Path) -> io::Result<bool> {
    let d = id_of(data)?;
    // dest, or the part of it that exists, and every ancestor.
    let mut at = Some(dest);
    while let Some(p) = at {
        if let Ok(id) = id_of(p)
            && id == d
        {
            return Ok(true);
        }
        at = p.parent();
    }
    // data's ancestors (data itself included) against dest, if it exists.
    if let Ok(t) = id_of(dest) {
        let mut at = Some(data);
        while let Some(p) = at {
            if id_of(p)? == t {
                return Ok(true);
            }
            at = p.parent();
        }
    }
    Ok(false)
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
            Err(fs::TryLockError::Error(e)) => bail!(
                "{} cannot be locked ({e}): without the lock two backups could share the \
                 staging folder, so none is taken here; back up from a file system with file \
                 locks",
                lp.display()
            ),
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

/// A pause for the tests (debug builds only), named by `var`: the backup's
/// before the first links (a file is deleted, events are added between the
/// copy of `master.db` and the links); the restore's after its first file
/// (it is killed there).
#[cfg(debug_assertions)]
fn test_pause(var: &str) {
    let Ok(p) = std::env::var(var) else {
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
fn test_pause(_: &str) {}

/// The result, for the lines the command prints.
pub struct Done {
    pub online: bool,
    pub databases: usize,
    pub blobs: u64,
    pub others: usize,
    pub bytes: u64,
    pub attempts: u32,
    /// What the settling of the copies did.
    pub settled: db::Settled,
    pub manifest_sha256: String,
    pub skipped: Vec<String>,
    /// Fixed items that were symbolic links (names only).
    pub links: Vec<String>,
    pub secs: f64,
}

/// Back up `data` into `dest`.
pub fn backup(data: &Path, dest: &Path, cancel: &AtomicBool) -> anyhow::Result<Done> {
    let started = Instant::now();
    // Before anything is opened: SQLite's own open would block on a FIFO.
    for (name, _) in databases() {
        db_files_regular(data, name)?;
    }
    // The stored files are never read through a link.
    if let Ok(m) = fs::symlink_metadata(data.join("blobs"))
        && !m.is_dir()
    {
        bail!(
            "blobs/ in {} is a symbolic link or not a folder: the stored files are never read \
             through a link; nothing was backed up",
            data.display()
        );
    }
    if !databases()
        .iter()
        .any(|(n, _)| kind(&data.join(n)).is_ok_and(|k| k == Kind::Regular))
    {
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

    // Where each database is read from. Beside a running server: its own
    // files, read-only (a reader writes only its read marks into the
    // server's `-shm` index, as every reader does). With the lock held: a
    // byte copy of the stopped server's files (database, `-wal`, `-shm`)
    // in the staging folder, so not a byte of the source changes, not even
    // the index a crashed server left, and a WAL without its index is
    // recovered in the copy. Every database's schema is checked there
    // before any is copied: one of a newer build is refused untouched.
    let present: Vec<&str> = databases()
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| data.join(n).exists())
        .collect();
    let mut source = BTreeMap::new();
    if online {
        for n in &present {
            source.insert(*n, data.join(n));
        }
    } else {
        // The copies go beside the data, on its own disk.
        let need: u64 = present
            .iter()
            .flat_map(|n| ["", "-wal", "-shm", "-journal"].map(|s| data.join(format!("{n}{s}"))))
            .filter_map(|p| fs::symlink_metadata(p).ok())
            .map(|m| m.len())
            .sum();
        let free =
            free_bytes(data).context("reading the free space of the data directory's disk")?;
        room_for(need, free)?;
        let into = staging.dir.join("db");
        mkdir_private(&into)?;
        for n in &present {
            for suffix in ["", "-wal", "-shm", "-journal"] {
                let f = format!("{n}{suffix}");
                let (mut src, _) = match open_regular(&data.join(&f)) {
                    Ok(v) => v,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e).with_context(|| format!("reading {f}")),
                };
                let mut out = create_private(&into.join(&f))?;
                copy_stream(&mut src, &mut out, &mut buf, cancel)?;
            }
            source.insert(*n, into.join(n));
        }
    }
    for (name, schema) in databases() {
        if let Some(src) = source.get(name) {
            harness_store::migrate::check_file(src, schema)
                .map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
        }
    }
    // Who holds the directory (for a refusal's words) and which `-shm`
    // files existed before any read.
    let holder = match lock::status(data) {
        Ok(lock::Status::Held(m)) => m,
        _ => "another process".into(),
    };
    let shm_before: BTreeSet<&str> = present
        .iter()
        .copied()
        .filter(|n| data.join(format!("{n}-shm")).exists())
        .collect();
    let had_shm = |n: &str| shm_before.contains(n);
    let snap = |name: &str, tmp: &Path| -> anyhow::Result<()> {
        let src = &source[name];
        let r = if online {
            db::snapshot(src, tmp)
        } else {
            db::snapshot_private_copy(src, tmp)
        };
        r.map_err(|e| {
            if db::needs_writer(&e) {
                anyhow::anyhow!(
                    "{}",
                    needs_writer_sentence(
                        name,
                        &holder,
                        shm_state(&data.join(name)),
                        had_shm(name)
                    )
                )
            } else {
                anyhow::anyhow!("copying {name}: {e}")
            }
        })
    };

    // 1 and 2: master.db and the links to the files it names.
    let master_tmp = tmp_name(&dest, "master.db");
    let mut attempts = 0;
    let mut snapshot_ms = harness_core::now_ms();
    let mut blob_ids: Vec<String> = Vec::new();
    if source.contains_key("master.db") {
        loop {
            attempts += 1;
            check_cancel(cancel)?;
            staging.reset()?;
            if fs::symlink_metadata(&master_tmp).is_ok() {
                fs::remove_file(&master_tmp)?;
            }
            w.note(&master_tmp);
            drop(create_private(&master_tmp)?);
            // The moment the copies are as of (the settling's clock).
            snapshot_ms = harness_core::now_ms();
            snap("master.db", &master_tmp)?;
            if attempts == 1 {
                test_pause("HARNESS_BACKUP_PAUSE_FILE");
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
        if !source.contains_key(name) {
            continue;
        }
        let tmp = tmp_name(&dest, name);
        w.note(&tmp);
        drop(create_private(&tmp)?);
        snap(name, &tmp)?;
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
    // The copies made to agree as of master.db's copy: chats caught
    // before their owner's stamp get it, chats begun later are left out,
    // chats deleted meanwhile go (docs/backup-restore.md).
    let mut settled = db::Settled::default();
    let mut skipped = Vec::new();
    if versions.contains_key("master.db") {
        let current = harness_store::DB_SCHEMA.steps.len() as i64;
        let logs: Vec<(&Path, &str)> = [("chat.db", "master"), ("node.db", "local")]
            .iter()
            .filter_map(|(name, node)| {
                pending
                    .iter()
                    .find(|p| p.path == *name)
                    .map(|p| (p.tmp.as_path(), *node))
            })
            .collect();
        let all_current = ["master.db", "chat.db", "node.db"]
            .iter()
            .all(|n| versions.get(*n).is_none_or(|v| *v == current));
        if all_current {
            settled = db::settle_copies(&master_tmp, &logs, snapshot_ms)
                .context("making the copies of master.db and the node logs agree")?;
            for (name, schema) in databases() {
                if name != "memory.db"
                    && let Some(p) = pending.iter().find(|p| p.path == name)
                {
                    check_db(name, &p.tmp, schema)?;
                }
            }
        } else {
            skipped.push(
                "the settling of chats begun, stamped or deleted during the backup: the \
                 databases are at another schema version than this build (back up with the \
                 server's own build)"
                    .into(),
            );
        }
    }

    // 4: plain files, keys/, agents/. A symbolic link among these fixed
    // items is followed (container secrets, a dotfiles repository): the
    // content goes in as a plain file and the manifest notes the name only.
    let mut links: Vec<String> = Vec::new();
    let copy_in = |src: &Path,
                   dir: &Path,
                   file: &str,
                   path: String,
                   w: &mut Written,
                   pending: &mut Vec<Pending>,
                   buf: &mut [u8]|
     -> anyhow::Result<Option<bool>> {
        let Some((mut f, len, was_link)) = open_item(src, &path)? else {
            return Ok(None);
        };
        mkdir_private(dir)?;
        let tmp = tmp_name(dir, file);
        w.note(&tmp);
        let mut out = create_private(&tmp)?;
        let hashed = copy_declared(&mut f, &mut out, len, buf, cancel)
            .with_context(|| format!("reading {path}"))?;
        out.sync_all()?;
        pending.push(Pending {
            tmp,
            path,
            hashed: Some(hashed),
        });
        Ok(Some(was_link))
    };
    for name in PLAIN {
        if copy_in(
            &data.join(name),
            &dest,
            name,
            (*name).into(),
            &mut w,
            &mut pending,
            &mut buf,
        )? == Some(true)
        {
            links.push((*name).into());
        }
    }
    for sub in ["keys", "agents"] {
        let at = data.join(sub);
        let lm = match fs::symlink_metadata(&at) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("reading {sub}/")),
        };
        if lm.file_type().is_symlink() {
            match fs::metadata(&at) {
                Ok(m) if m.is_dir() => links.push(sub.into()),
                Ok(_) => bail!(
                    "{sub} is a symbolic link to something that is not a folder: nothing was \
                     backed up; fix the link"
                ),
                Err(_) => bail!(
                    "{sub} is a symbolic link whose target is missing: the backup would lack \
                     it, so nothing was backed up; fix the link or remove it"
                ),
            }
        } else if !lm.is_dir() {
            bail!("{sub} in the data directory is not a folder: nothing was backed up");
        }
        let mut names: Vec<String> = fs::read_dir(&at)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for n in names {
            let shown = format!("{sub}/{}", n.escape_debug());
            if fs::symlink_metadata(at.join(&n)).is_ok_and(|m| m.is_dir()) {
                skipped.push(format!(
                    "{shown}/ (a folder: a backup keeps only the files directly in {sub}/)"
                ));
                continue;
            }
            if !valid_name(&n) {
                skipped.push(format!("{shown} (a name a backup does not keep)"));
                continue;
            }
            if copy_in(
                &at.join(&n),
                &dest.join(sub),
                &n,
                format!("{sub}/{n}"),
                &mut w,
                &mut pending,
                &mut buf,
            )? == Some(true)
            {
                links.push(format!("{sub}/{n}"));
            }
        }
    }
    // Anything else at the top that a backup does not know.
    let mut tops: Vec<String> = fs::read_dir(data)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !known_top(n))
        .collect();
    tops.sort();
    for n in tops {
        skipped.push(format!(
            "{} (not something a backup keeps)",
            n.escape_debug()
        ));
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
        created_ms: snapshot_ms,
        online,
        databases: versions,
        files,
        links: links.clone(),
        skipped: skipped.clone(),
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
        settled,
        manifest_sha256: hex::encode(Sha256::digest(&text)),
        skipped,
        links,
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
    if d.settled.left_out > 0 {
        println!(
            "{} chats begun while the backup ran are not in it (they began after master.db \
             was copied)",
            d.settled.left_out
        );
    }
    if d.settled.deleted > 0 {
        println!(
            "{} chats deleted while the backup ran are not in it",
            d.settled.deleted
        );
    }
    if d.settled.stamped > 0 {
        println!(
            "{} chats caught between their creation and their owner's stamp were given their \
             owner",
            d.settled.stamped
        );
    }
    for l in &d.links {
        println!("followed the symbolic link {l}: its content is in the backup as a plain file");
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
    for l in &man.links {
        if !(valid_path(l) || l == "keys" || l == "agents")
            || databases().iter().any(|(d, _)| d == l)
        {
            bail!(
                "the manifest names a link a backup never records: refused, nothing was restored"
            );
        }
    }
    if man.skipped.len() > 10_000 || man.skipped.iter().any(|s| s.len() > 1000) {
        bail!("the manifest's list of skipped items is not one a backup writes: refused");
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
        let (n, h) = copy_declared(&mut f, &mut io::sink(), e.size, &mut buf, cancel)
            .with_context(|| e.path.clone())?;
        if n != e.size || h != e.sha256 {
            return Err(mismatch(&e.path));
        }
        bytes += n;
    }
    // SQLite opens these beside a database (and would block on a FIFO); a
    // backup never holds them.
    for (name, _) in databases() {
        for suffix in ["-wal", "-shm", "-journal"] {
            let f = format!("{name}{suffix}");
            if kind(&backup.join(&f))? != Kind::Missing {
                bail!(
                    "the backup holds {f}, which a backup never has: it was changed; nothing \
                     was opened"
                );
            }
        }
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
    let what = format!(
        "{} files ({}) match its manifest, {} databases pass their checks, {blobs} stored files \
         open and match their rows ({:.1} s)",
        man.files.len(),
        mb(bytes),
        man.databases.len(),
        started.elapsed().as_secs_f64()
    );
    if man.skipped.is_empty() {
        println!("{} is a complete backup: {what}", backup.display());
    } else {
        println!(
            "{} is a finished backup of what it holds, not of everything the source had: {what}",
            backup.display()
        );
        for sk in &man.skipped {
            println!("skipped on the source: {}", sk.escape_debug());
        }
    }
    if !man.links.is_empty() {
        println!(
            "{} items were symbolic links on the source (now plain files): {}",
            man.links.len(),
            man.links.join(", ")
        );
    }
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
    /// Items that were symbolic links on the source (names only).
    pub links: Vec<String>,
    /// What the source had that the backup does not hold.
    pub skipped: Vec<String>,
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
        let (n, h) = copy_declared(&mut f, &mut out, e.size, &mut buf, cancel)
            .with_context(|| e.path.clone())?;
        if n != e.size || h != e.sha256 {
            return Err(mismatch(&e.path));
        }
        out.sync_all()?;
        bytes += n;
        dirs.insert(parent);
        if bytes == n {
            test_pause("HARNESS_RESTORE_PAUSE_FILE");
        }
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
        links: man.links.clone(),
        skipped: man.skipped.clone(),
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
    if !r.links.is_empty() {
        println!(
            "{} items were symbolic links on the source and are plain files now: {}",
            r.links.len(),
            r.links.join(", ")
        );
    }
    for sk in &r.skipped {
        println!(
            "not in this backup (skipped on the source): {}",
            sk.escape_debug()
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

/// Refuse a data directory that holds an interrupted restore (every
/// command that reads or writes one; `doctor` reports it instead).
pub fn refuse_restoring(dir: &Path) -> anyhow::Result<()> {
    if fs::symlink_metadata(dir.join(RESTORING)).is_ok() {
        bail!("{}", restoring_advice(dir));
    }
    Ok(())
}

/// What to do about an interrupted restore, in words.
pub fn restoring_advice(dir: &Path) -> String {
    format!(
        "{} holds an interrupted restore (a {RESTORING} file is in it), and no command uses it \
         as it is: delete everything in it (lock and backup.lock too), or the directory \
         itself, then run `{} --data-dir {} restore <backup>` again",
        dir.display(),
        harness_core::BRAND,
        dir.display()
    )
}

/// Run `f` off the async runtime; SIGINT or SIGTERM asks it to stop (it
/// removes what it wrote and ends with "interrupted").
pub async fn run_stoppable<T: Send + 'static>(
    f: impl FnOnce(&AtomicBool) -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    // First, so a signal at any moment after the work starts is caught.
    let mut signals = crate::stop::Signals::install().context("catching stop signals")?;
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    let mut task = tokio::task::spawn_blocking(move || f(&c));
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
            "pre-migration/master.db",
            "pre-migration",
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

    /// A file that grows past its declared size while it streams in is
    /// refused (the copy reads at most one byte past it). This bound is
    /// tested here only: an integration test of a file growing during a
    /// restore would race the writer, and restore and `--verify` both
    /// read through this one function.
    #[test]
    fn a_file_longer_than_declared_is_refused() {
        let mut buf = vec![0u8; 16];
        // Without end, as a file that keeps growing: refused after 101 bytes.
        let mut endless = io::repeat(7);
        let mut seen = Vec::new();
        let e = copy_declared(
            &mut endless,
            &mut seen,
            100,
            &mut buf,
            &AtomicBool::new(false),
        );
        assert!(e.unwrap_err().to_string().contains("grew"));
        assert_eq!(seen.len(), 101);
        // Exactly as declared: taken.
        let (n, _) = copy_declared(
            &mut io::repeat(7).take(100),
            &mut io::sink(),
            100,
            &mut buf,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(n, 100);
    }

    /// The free-space refusal gives both numbers; enough room passes.
    #[test]
    fn an_offline_backup_needs_room_for_its_copies() {
        let e = room_for(500 << 20, 100 << 20).unwrap_err().to_string();
        assert!(e.contains("524.3 MB") && e.contains("104.9 MB free"), "{e}");
        room_for(500 << 20, 1 << 30).unwrap();
    }

    /// The refusal says what is true: a read-only `-shm`, or a recovery
    /// beside the named holder; and a `-shm` the reader made is owned up to.
    #[test]
    fn the_needs_writer_refusal_says_what_is_true() {
        let a = needs_writer_sentence("master.db", "held by restore (pid 7)", Shm::ReadOnly, true);
        assert!(
            a.contains("master.db-shm, the database's shared-memory file, cannot be written"),
            "{a}"
        );
        assert!(
            a.contains("nothing of the database or its log was written"),
            "{a}"
        );
        let b = needs_writer_sentence("chat.db", "held by node join (pid 9)", Shm::Writable, false);
        assert!(
            b.contains("held by node join (pid 9)") && b.contains("made chat.db-shm"),
            "{b}"
        );
        assert!(!b.contains("nothing was written"), "{b}");
    }
}
