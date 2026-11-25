//! The copy of the databases before a start migrates them, and the
//! rollback from it (FIX_PLAN 4.4, second half; docs/backup-restore.md
//! "A copy before each migration").
//!
//! Since 4.3 a start migrates each database on its own: a migration that
//! fails on the second file leaves the first one at a version the older
//! build refuses. So before any database is opened at a start that will
//! migrate one, all of them are copied (they belong together) into
//! `<data>/pre-migration/`, the way an offline backup copies them: a byte
//! copy of each database with its `-wal`, `-shm` and `-journal`, then
//! `VACUUM INTO` from that private copy.
//!
//! A set is the rollback point of ONE upgrade ([`before_start`] has the
//! rule): it is kept while the databases are at its `before` or its
//! `target` versions (a failed start, retried), made only from one
//! consistent state, never replaced from a state an upgrade produced, and
//! never touched by a rollback. `restore --pre-migration` is staged under
//! `RESTORING` and can be run again after any interruption.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, bail};
use harness_store::backup as db;
use serde::{Deserialize, Serialize};

use crate::backup::{
    Entry, RESTORING, ROLLBACK_MARK, check_db, copy_declared, copy_stream, create_private,
    databases, db_files_regular, free_bytes, mb, mkdir_private, open_regular, sync_dir, test_pause,
};

/// The set's folder in the data directory, its temporary name while it is
/// written, and the old set's name for the moment of the swap.
pub const SET: &str = "pre-migration";
pub const SET_PARTIAL: &str = "pre-migration.partial";
pub const SET_OLD: &str = "pre-migration.old";
/// The suffix a rollback gives what it puts aside.
pub const ASIDE: &str = ".failed-upgrade-";
/// A rollback's copy of a database, beside its destination, until placed.
const STAGED: &str = ".rollback-partial";
const MANIFEST: &str = "MANIFEST.json";
const KIND: &str = "pre-migration";
const FORMAT: u32 = 1;
/// The store databases share one schema and are always migrated together.
const STORE_FAMILY: [&str; 3] = ["master.db", "chat.db", "node.db"];

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetManifest {
    pub format: u32,
    pub kind: String,
    /// The build that made the copy (the newer one, about to migrate).
    pub harness: String,
    pub created_ms: i64,
    /// Each database's schema version as it was, before the start (0: a
    /// file from before schema versions, holding tables).
    pub before: BTreeMap<String, i64>,
    /// The version this build was about to migrate each one to.
    pub target: BTreeMap<String, i64>,
    pub files: Vec<Entry>,
}

/// A start that made or kept a set: what it says if the start then fails
/// after a database moved from its `before` version.
pub struct Made {
    pub advice: String,
    pub before: BTreeMap<String, i64>,
}

/// One database present in the data directory.
#[derive(Debug, Clone, Copy)]
struct Live {
    version: i64,
    tables: bool,
    want: i64,
}

impl Live {
    /// It will be migrated by this build: below the build's version, or at
    /// 0 with tables (4.3 adopts such a file as version 1, then migrates).
    fn behind(&self) -> bool {
        (self.version > 0 && self.version < self.want) || (self.version == 0 && self.tables)
    }

    /// Its version for comparing with its family (0 with tables is 1).
    fn effective(&self) -> i64 {
        if self.version == 0 && self.tables {
            1
        } else {
            self.version
        }
    }
}

fn remove_any(p: &Path) -> io::Result<()> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Each database present in `data` (an empty file at version 0 counts as
/// absent), read-only; a file a read-only reader may not read (a hot
/// journal) is read from a private copy.
fn live(data: &Path) -> anyhow::Result<BTreeMap<&'static str, Live>> {
    let mut out = BTreeMap::new();
    let mut probed = false;
    for (name, schema) in databases() {
        db_files_regular(data, name)?;
        let p = data.join(name);
        if !p.is_file() {
            continue;
        }
        let (version, tables) = match db::version_and_tables(&p, false) {
            Ok(v) => v,
            Err(e) if db::needs_writer(&e) => {
                probed = true;
                probe(data, name)?
            }
            Err(e) => return Err(e).with_context(|| format!("reading {name}'s schema version")),
        };
        if version == 0 && tables == 0 {
            continue;
        }
        out.insert(
            name,
            Live {
                version,
                tables: tables > 0,
                want: schema.steps.len() as i64,
            },
        );
    }
    if probed {
        remove_any(&data.join(SET_PARTIAL))?;
    }
    Ok(out)
}

/// `name`'s version and tables as SQLite's recovery would leave them, read
/// from a byte copy in `pre-migration.partial/probe/` (never the source).
fn probe(data: &Path, name: &str) -> anyhow::Result<(i64, i64)> {
    let probe = data.join(SET_PARTIAL).join("probe");
    remove_any(&data.join(SET_PARTIAL))?;
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(&probe)?;
    byte_copy(data, name, &probe)?;
    db::version_and_tables(&probe.join(name), true)
        .with_context(|| format!("reading {name}'s schema version from a copy"))
}

/// A byte copy of `name` with its `-wal`, `-shm` and `-journal` into `to`.
fn byte_copy(data: &Path, name: &str, to: &Path) -> anyhow::Result<()> {
    let cancel = AtomicBool::new(false);
    let mut buf = vec![0u8; crate::backup::BUF];
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let f = format!("{name}{suffix}");
        let (mut src, _) = match open_regular(&data.join(&f)) {
            Ok(x) => x,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("reading {f}")),
        };
        let mut out = create_private(&to.join(&f))?;
        copy_stream(&mut src, &mut out, &mut buf, &cancel)?;
    }
    Ok(())
}

/// The stored files under `blobs/`, by id.
fn blob_files(data: &Path) -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    let Ok(shards) = fs::read_dir(data.join("blobs")) else {
        return out;
    };
    for shard in shards.flatten() {
        if shard.file_name().len() != 2 {
            continue;
        }
        let Ok(files) = fs::read_dir(shard.path()) else {
            continue;
        };
        for f in files.flatten() {
            let n = f.file_name().to_string_lossy().into_owned();
            if let Some(id) = n.strip_suffix(".rzb") {
                out.insert(id.to_string(), f.path());
            }
        }
    }
    out
}

fn has_aside(data: &Path) -> bool {
    fs::read_dir(data).is_ok_and(|rd| {
        rd.flatten()
            .any(|e| e.file_name().to_string_lossy().contains(ASIDE))
    })
}

/// At a start (`role`), holding the data directory's lock and before any
/// database is opened. The rule:
///
/// - Databases missing while the directory shows it is no first run (a
///   set naming them, files put aside by a rollback, or, for `serve`,
///   stored files without `master.db`): refused, naming the rollback.
/// - Nothing behind: nothing to do.
/// - A complete set whose `target` is this build's versions, with every
///   database at that set's `before` or `target` (one made for this very
///   upgrade; a database the set lacks at this build's version, made by
///   a failed start): kept, and the migration resumes.
/// - Otherwise a new set is made only from one consistent state: the
///   store databases (`master.db`, `chat.db`, `node.db`) at one version
///   (0 holding tables counting as 1, as 4.3 adopts it); `memory.db` has
///   its own schema. Mixed versions with no matching set: refused (no
///   honest rollback point), unless `--migrate-without-copy`.
/// - A copy that cannot be written stops the start before anything is
///   migrated, unless `--migrate-without-copy`.
pub fn before_start(data: &Path, role: &str, without_copy: bool) -> anyhow::Result<Option<Made>> {
    let v = live(data)?;
    // The manifest only, here: the files are hashed below when needed.
    let set = read_set(data, false).ok();
    let rollback = format!(
        "`{} --data-dir {} restore --pre-migration`",
        harness_core::BRAND,
        data.display()
    );
    if let Some(s) = &set {
        let missing: Vec<&str> = s
            .before
            .keys()
            .map(String::as_str)
            .filter(|n| !v.contains_key(n))
            .collect();
        if !missing.is_empty() {
            bail!(
                "{} lacks {} although {}/ holds a copy of them from before an upgrade: this is \
                 no first run but a rollback that did not finish, or a lost file. Nothing was \
                 started. Run {rollback} to put the copy back (it says what it will do first)",
                data.display(),
                missing.join(", "),
                SET
            );
        }
    }
    if role == "serve" && !v.contains_key("master.db") {
        let mut found = Vec::new();
        if !blob_files(data).is_empty() {
            found.push("stored files in blobs/");
        }
        if has_aside(data) {
            found.push("files a rollback put aside (*.failed-upgrade-*)");
        }
        if fs::symlink_metadata(data.join(SET)).is_ok() {
            found.push("pre-migration/");
        }
        if !found.is_empty() {
            bail!(
                "{} has no master.db but holds {}: this is no first run, so no new server is \
                 made over it. If an upgrade's rollback did not finish, run {rollback}; \
                 otherwise put the right master.db back, or move those away to start afresh",
                data.display(),
                found.join(", ")
            );
        }
    }
    let behind: Vec<&str> = v
        .iter()
        .filter(|(_, l)| l.behind())
        .map(|(n, _)| *n)
        .collect();
    if behind.is_empty() {
        return Ok(None);
    }
    let list = behind.join(", ");
    let shown: Vec<String> = v
        .iter()
        .map(|(n, l)| format!("{n} {}", l.version))
        .collect();
    let shown = shown.join(", ");
    // A set for this very upgrade, matching what the databases show (and
    // intact: every file checked).
    if let Some(s) = set.as_ref().filter(|_| read_set(data, true).is_ok()) {
        let for_this = s
            .target
            .iter()
            .all(|(n, t)| v.get(n.as_str()).is_none_or(|l| *t == l.want));
        let matches = v.iter().all(|(n, l)| match s.before.get(*n) {
            Some(b) => l.version == *b || Some(&l.version) == s.target.get(*n),
            None => l.version == l.want,
        });
        if for_this && matches {
            tracing::info!(
                set = %data.join(SET).display(),
                databases = %shown,
                "keeping the copy from before this upgrade (a start retried after a failed \
                 one): the migration resumes"
            );
            return Ok(Some(Made {
                advice: advice(data, &s.before),
                before: s.before.clone(),
            }));
        }
    }
    let family: BTreeSet<i64> = STORE_FAMILY
        .iter()
        .filter_map(|n| v.get(n))
        .map(Live::effective)
        .collect();
    let consistent = family.len() <= 1;
    if without_copy {
        tracing::warn!(
            databases = %list,
            "MIGRATING WITHOUT A COPY (--migrate-without-copy): if a migration fails, there is no \
             set to roll back to and the older build may refuse what was migrated"
        );
        eprintln!(
            "warning: migrating {list} WITHOUT a copy (--migrate-without-copy): if it fails there \
             is no way back to the older build"
        );
        return Ok(None);
    }
    let stop = |why: String| {
        anyhow::anyhow!(
            "{why}. The start stopped before migrating anything ({list} would be migrated). \
             Make room or fix the disk and start again; or start once with \
             --migrate-without-copy to migrate without a way back to the older build (not \
             recommended)"
        )
    };
    if !consistent {
        return Err(stop(format!(
            "the databases are at mixed versions ({shown}) and no copy from before this upgrade \
             matches them, so no honest rollback point can be made: a copy now would hold a \
             state no older build left (restore a backup, or roll back with {rollback} if \
             pre-migration/ belongs to another upgrade)"
        )));
    }
    // Each database is read three times beside the data: its byte copy,
    // the snapshot from it, and SQLite's temporary space for `VACUUM INTO`
    // (counted at the database's size, its most).
    let size: u64 = v
        .keys()
        .flat_map(|n| ["", "-wal", "-shm", "-journal"].map(|s| data.join(format!("{n}{s}"))))
        .filter_map(|p| fs::symlink_metadata(p).ok())
        .map(|m| m.len())
        .sum();
    let need = size.saturating_mul(3);
    let want = need.saturating_add((need / 10).max(64 << 20));
    let free = free_bytes(data).map_err(|e| {
        stop(format!(
            "the free space of the data directory's disk cannot be read ({e})"
        ))
    })?;
    if free < want {
        return Err(stop(format!(
            "the copy of the databases before this start's migration needs {} with a margin, and \
             the data directory's disk has {} free",
            mb(want),
            mb(free)
        )));
    }
    write_set(data, &v).map_err(|e| {
        stop(format!(
            "the copy of the databases before this start's migration could not be written into \
             {} ({e:#})",
            data.join(SET).display()
        ))
    })?;
    tracing::info!(
        set = %data.join(SET).display(),
        databases = %list,
        "copied every database before migrating; `restore --pre-migration` rolls back to it"
    );
    let before: BTreeMap<String, i64> = v.iter().map(|(n, l)| (n.to_string(), l.version)).collect();
    Ok(Some(Made {
        advice: advice(data, &before),
        before,
    }))
}

fn advice(data: &Path, before: &BTreeMap<String, i64>) -> String {
    let versions: Vec<String> = before.iter().map(|(n, v)| format!("{n} {v}")).collect();
    format!(
        "The databases as they were before this upgrade (schema versions: {}) are in {}, and some \
         are no longer as they were. To go back to the older build: `{} --data-dir {} restore \
         --pre-migration` (it says what it will do; add --yes), then start the older build.",
        versions.join(", "),
        data.join(SET).display(),
        harness_core::BRAND,
        data.display()
    )
}

/// After a start failed: whether a database is no longer at its `before`
/// version (then the advice is due).
pub fn moved(data: &Path, before: &BTreeMap<String, i64>) -> bool {
    before.iter().any(|(n, b)| {
        let p = data.join(n);
        !p.is_file()
            || db::version_and_tables(&p, false)
                .map(|(v, _)| v != *b)
                .unwrap_or(true)
    })
}

fn write_set(data: &Path, v: &BTreeMap<&'static str, Live>) -> anyhow::Result<()> {
    let cancel = AtomicBool::new(false);
    let partial = data.join(SET_PARTIAL);
    remove_any(&partial)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&partial)
        .context("making the folder")?;
    let work = partial.join("work");
    mkdir_private(&work)?;
    let mut buf = vec![0u8; crate::backup::BUF];
    let mut files = Vec::new();
    for name in v.keys() {
        byte_copy(data, name, &work)?;
        let to = partial.join(name);
        drop(create_private(&to)?);
        db::snapshot_private_copy(&work.join(name), &to)
            .with_context(|| format!("copying {name}"))?;
        let (mut f, _) = open_regular(&to)?;
        f.sync_all()?;
        let (size, sha256) = copy_stream(&mut f, &mut io::sink(), &mut buf, &cancel)?;
        files.push(Entry {
            path: (*name).to_string(),
            size,
            sha256,
        });
    }
    fs::remove_dir_all(&work)?;
    let m = SetManifest {
        format: FORMAT,
        kind: KIND.into(),
        harness: env!("CARGO_PKG_VERSION").into(),
        created_ms: harness_core::now_ms(),
        before: v
            .iter()
            .map(|(n, l)| ((*n).to_string(), l.version))
            .collect(),
        target: v.iter().map(|(n, l)| ((*n).to_string(), l.want)).collect(),
        files,
    };
    let mut out = create_private(&partial.join(MANIFEST))?;
    out.write_all(&serde_json::to_vec_pretty(&m)?)?;
    out.sync_all()?;
    drop(out);
    sync_dir(&partial)?;
    // The swap: the complete new set replaces the old one only now.
    let set = data.join(SET);
    let old = data.join(SET_OLD);
    remove_any(&old)?;
    if fs::symlink_metadata(&set).is_ok() {
        fs::rename(&set, &old)?;
    }
    fs::rename(&partial, &set)?;
    sync_dir(data)?;
    remove_any(&old)?;
    Ok(())
}

/// Read and check the set: its manifest, every file present with its size
/// and SHA-256, and only database names.
fn read_set(data: &Path, hashes: bool) -> anyhow::Result<SetManifest> {
    let set = data.join(SET);
    match fs::symlink_metadata(&set) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => bail!("{} is not a folder: nothing was rolled back", set.display()),
        Err(_) => {
            let left: Vec<&str> = [SET_OLD, SET_PARTIAL]
                .into_iter()
                .filter(|n| fs::symlink_metadata(data.join(n)).is_ok())
                .collect();
            if left.is_empty() {
                bail!(
                    "{} holds no copy from before a migration (no {SET}/ folder): nothing to roll \
                     back to",
                    data.display()
                );
            }
            bail!(
                "{} holds no complete copy from before a migration, only what an interrupted \
                 copy left ({}): nothing to roll back to; those may be deleted",
                data.display(),
                left.join(", ")
            );
        }
    }
    let (mut f, md) = open_regular(&set.join(MANIFEST)).map_err(|_| {
        anyhow::anyhow!(
            "the copy in {} has no readable {MANIFEST}: it is incomplete; nothing was rolled back",
            set.display()
        )
    })?;
    if md.len() > 1 << 20 {
        bail!("{MANIFEST} in {} is not one of ours", set.display());
    }
    let mut text = Vec::new();
    io::Read::read_to_end(&mut f, &mut text)?;
    let m: SetManifest = serde_json::from_slice(&text).map_err(|_| {
        anyhow::anyhow!(
            "{MANIFEST} in {} is not a manifest this build reads",
            set.display()
        )
    })?;
    if m.kind != KIND || m.format > FORMAT {
        bail!(
            "{MANIFEST} in {} is not a pre-migration copy this build reads",
            set.display()
        );
    }
    let names: Vec<&str> = databases().iter().map(|(n, _)| *n).collect();
    let mut buf = vec![0u8; crate::backup::BUF];
    let cancel = AtomicBool::new(false);
    for e in &m.files {
        if !names.contains(&e.path.as_str()) {
            bail!("the copy's manifest names a file a copy never holds: nothing was rolled back");
        }
        let p = set.join(&e.path);
        if !hashes {
            if !p.is_file() {
                bail!("{} is missing from the copy", e.path);
            }
            continue;
        }
        let (mut f, md) = open_regular(&p).map_err(|_| {
            anyhow::anyhow!(
                "{} is missing from the copy in {} (or not a regular file): the set is incomplete; \
                 nothing was rolled back",
                e.path,
                set.display()
            )
        })?;
        let (n, h) = copy_declared(&mut f, &mut io::sink(), e.size, &mut buf, &cancel)
            .with_context(|| e.path.clone())?;
        if md.len() != e.size || n != e.size || h != e.sha256 {
            bail!(
                "{} in the copy does not match its manifest (size or SHA-256): it was damaged or \
                 changed; nothing was rolled back",
                e.path
            );
        }
    }
    for name in m.before.keys() {
        if !m.files.iter().any(|e| &e.path == name) {
            bail!("{name} is missing from the copy's manifest: the set is incomplete");
        }
    }
    Ok(m)
}

fn pause(step: &str) {
    #[cfg(debug_assertions)]
    if std::env::var("HARNESS_ROLLBACK_PAUSE").is_ok_and(|s| s == step) {
        test_pause("HARNESS_ROLLBACK_PAUSE_FILE");
    }
    let _ = step;
}

/// `restore --pre-migration [--yes]`: under the data directory's lock (the
/// caller holds it: a running server's directory is refused). It says what
/// it will do; with `--yes` it does it, staged so that any interruption
/// leaves the directory refused by every command but this one, which then
/// finishes from the set (never changed):
///
/// 1. the set verified, free space checked;
/// 2. `RESTORING` written (a rollback's);
/// 3. each copy written beside its database as `<db>.rollback-partial`,
///    synced;
/// 4. each live database (with `-wal`, `-shm`, `-journal`) put aside as
///    `<file>.failed-upgrade-<ms>` (0600), its copy renamed into place;
/// 5. stored files the set's `master.db` has no row for moved into
///    `blobs.failed-upgrade-<ms>/`;
/// 6. the versions checked, `RESTORING` removed.
pub fn rollback(data: &Path, yes: bool) -> anyhow::Result<()> {
    let m = read_set(data, true)?;
    let set = data.join(SET);
    let resuming = crate::backup::rollback_marked(data);
    let set_ids: Option<BTreeSet<String>> = if m.files.iter().any(|e| e.path == "master.db") {
        Some(
            db::blob_ids(&set.join("master.db"))
                .map_err(|e| anyhow::anyhow!("reading the copy's master.db: {e}"))?
                .into_iter()
                .collect(),
        )
    } else {
        None
    };
    let orphans: Vec<PathBuf> = match &set_ids {
        Some(ids) => blob_files(data)
            .into_iter()
            .filter(|(id, _)| !ids.contains(id))
            .map(|(_, p)| p)
            .collect(),
        None => Vec::new(),
    };
    // What it will do, first.
    let age_h = (harness_core::now_ms() - m.created_ms).max(0) / 3_600_000;
    println!(
        "{}the copy in {} was made by build {} at unix time {} ({age_h} h ago) before it \
         migrated:",
        if resuming {
            "finishing an interrupted rollback; "
        } else {
            ""
        },
        set.display(),
        m.harness,
        m.created_ms / 1000
    );
    for (name, b) in &m.before {
        let now = match db::version_and_tables(&data.join(name), false) {
            Ok((v, _)) if data.join(name).is_file() => format!("now {v}"),
            _ => "now missing".into(),
        };
        println!("  {name}: version {b} in the copy, {now}");
    }
    println!(
        "it will put the copy back and keep every current database beside it as \
         <file>{ASIDE}<time> (0600); {} stored files have no row in the copy (stored after the \
         upgrade): they are moved into blobs{ASIDE}<time>/, not deleted",
        orphans.len()
    );
    if !yes {
        bail!("nothing was changed: run it again with --yes to roll back");
    }
    let size: u64 = m.files.iter().map(|e| e.size).sum();
    let want = size.saturating_add((size / 10).max(64 << 20));
    let free = free_bytes(data).context("reading the free space of the data directory's disk")?;
    if free < want {
        bail!(
            "the copies go beside the databases first and need {} with a margin; the data \
             directory's disk has {} free: nothing was changed",
            mb(want),
            mb(free)
        );
    }
    // 2: from here on every command but this one refuses the directory.
    let marker = data.join(RESTORING);
    remove_any(&marker)?;
    let mut f = create_private(&marker)?;
    f.write_all(ROLLBACK_MARK.as_bytes())?;
    f.sync_all()?;
    drop(f);
    sync_dir(data)?;
    pause("marked");
    // 3: the copies, beside their destinations.
    let cancel = AtomicBool::new(false);
    let mut buf = vec![0u8; crate::backup::BUF];
    for e in &m.files {
        let staged = data.join(format!("{}{STAGED}", e.path));
        remove_any(&staged)?;
        let (mut f, _) = open_regular(&set.join(&e.path))?;
        let mut out = create_private(&staged)?;
        let (n, h) = copy_declared(&mut f, &mut out, e.size, &mut buf, &cancel)
            .with_context(|| e.path.clone())?;
        if n != e.size || h != e.sha256 {
            bail!(
                "{} changed while it was copied: run the rollback again",
                e.path
            );
        }
        out.sync_all()?;
    }
    sync_dir(data)?;
    pause("copied");
    // 4: aside and into place.
    let stamp = harness_core::now_ms();
    let mut aside = Vec::new();
    let mut placed = 0;
    for (name, _) in databases() {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let f = format!("{name}{suffix}");
            let p = data.join(&f);
            if fs::symlink_metadata(&p).is_err() {
                continue;
            }
            let to = data.join(format!("{f}{ASIDE}{stamp}"));
            fs::rename(&p, &to).with_context(|| format!("putting {f} aside"))?;
            fs::set_permissions(&to, fs::Permissions::from_mode(0o600))?;
            aside.push(
                to.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        if m.files.iter().any(|e| e.path == name) {
            fs::rename(data.join(format!("{name}{STAGED}")), data.join(name))
                .with_context(|| format!("putting {name} in place"))?;
            placed += 1;
            if placed == 1 {
                pause("half-placed");
            }
        }
    }
    sync_dir(data)?;
    pause("placed");
    // 5: the stored files of the upgrade, kept aside.
    let mut moved = 0;
    if !orphans.is_empty() {
        let into = data.join(format!("blobs{ASIDE}{stamp}"));
        mkdir_private(&into)?;
        for p in &orphans {
            let shard = p
                .parent()
                .and_then(|s| s.file_name())
                .context("a stored file outside a shard")?;
            let to_dir = into.join(shard);
            mkdir_private(&to_dir)?;
            if let Some(file) = p.file_name() {
                fs::rename(p, to_dir.join(file))?;
                moved += 1;
            }
        }
        sync_dir(&into)?;
    }
    pause("blobs-moved");
    // 6: the versions, then the marker last.
    for (name, schema) in databases() {
        if let Some(want) = m.before.get(name) {
            let got = check_db(name, &data.join(name), schema)?;
            if got != *want {
                bail!(
                    "{name} came back at schema version {got}, not {want}: run the rollback again"
                );
            }
        }
    }
    fs::remove_file(&marker)?;
    sync_dir(data)?;
    let versions: Vec<String> = m.before.iter().map(|(n, v)| format!("{n} {v}")).collect();
    println!(
        "put back {} databases from {} (schema versions {})",
        m.files.len(),
        set.display(),
        versions.join(", ")
    );
    println!(
        "kept aside, 0600: {}; {moved} stored files of the upgrade moved into \
         blobs{ASIDE}{stamp}/ (to delete once the older build runs well, or to move back with \
         the aside databases to return to the upgraded state)",
        aside.join(", ")
    );
    println!(
        "Now start the older build: this build ({}) would migrate them again. A stored file \
         deleted after the upgrade is gone though a row names it (its download fails, an \
         Artifact version shows as failed). The copy in {} stays.",
        env!("CARGO_PKG_VERSION"),
        set.display()
    );
    Ok(())
}

/// For `doctor`: a line about the set, if there is one.
pub fn doctor_line(data: &Path) -> Option<String> {
    let set = data.join(SET);
    let text = fs::read(set.join(MANIFEST)).ok()?;
    let m: SetManifest = serde_json::from_slice(&text).ok()?;
    let bytes: u64 = m.files.iter().map(|e| e.size).sum();
    let when = m.created_ms / 1000;
    let versions: Vec<String> = m.before.iter().map(|(n, v)| format!("{n} {v}")).collect();
    Some(format!(
        "pre-migration: a copy of the databases from before a migration (made at unix time {when} \
         by {}; schema versions {}), {} in {}; it is replaced at the next upgrade's start; \
         delete the folder to free the space once the upgrade runs well",
        m.harness,
        versions.join(", "),
        mb(bytes),
        set.display()
    ))
}
