//! The copy of the databases before a start migrates them, and the
//! rollback from it (FIX_PLAN 4.4, second half; docs/backup-restore.md
//! "A copy before each migration").
//!
//! Since 4.3 a start migrates each database on its own: a migration that
//! fails on the second file leaves the first one at a version the older
//! build refuses. So before any database is opened at a start that will
//! migrate one, all of them are copied (they belong together) into
//! `<data>/pre-migration/`, the way an offline backup copies them: a byte
//! copy of each database with its `-wal` and `-shm`, then `VACUUM INTO`
//! from that private copy. The set is written under `pre-migration.partial`
//! and renamed into place only once complete; one set is kept.
//! `restore --pre-migration` puts the set back, keeping the upgraded files
//! aside, and the operator starts the older build.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, bail};
use harness_store::backup as db;
use serde::{Deserialize, Serialize};

use crate::backup::{
    Entry, check_db, copy_declared, copy_stream, create_private, databases, db_files_regular,
    free_bytes, mb, mkdir_private, open_regular, sync_dir,
};

/// The set's folder in the data directory, its temporary name while it is
/// written, and the old set's name for the moment of the swap.
pub const SET: &str = "pre-migration";
pub const SET_PARTIAL: &str = "pre-migration.partial";
pub const SET_OLD: &str = "pre-migration.old";
/// The suffix a rollback gives the upgraded databases it puts aside.
pub const ASIDE: &str = ".failed-upgrade-";
const MANIFEST: &str = "MANIFEST.json";
const KIND: &str = "pre-migration";
const FORMAT: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetManifest {
    pub format: u32,
    pub kind: String,
    /// The build that made the copy (the newer one, about to migrate).
    pub harness: String,
    pub created_ms: i64,
    /// Each database's schema version as it was, before the start.
    pub before: BTreeMap<String, i64>,
    /// The version this build was about to migrate each one to.
    pub target: BTreeMap<String, i64>,
    pub files: Vec<Entry>,
}

/// What a start that made a set says if its migration then fails.
pub struct Made {
    pub advice: String,
}

/// Each database present in `data` with its schema version, read-only.
fn versions(data: &Path) -> anyhow::Result<BTreeMap<&'static str, (i64, i64)>> {
    let mut out = BTreeMap::new();
    for (name, schema) in databases() {
        db_files_regular(data, name)?;
        let p = data.join(name);
        if !p.is_file() {
            continue;
        }
        let v = match db::user_version(&p) {
            Ok(v) => v,
            // A hot journal or a WAL without its index (an interrupted
            // write): a read-only reader may not read it, so the version
            // is read from a private copy, recovered there.
            Err(e) if db::needs_writer(&e) => probe_version(data, name)?,
            Err(e) => return Err(e).with_context(|| format!("reading {name}'s schema version")),
        };
        out.insert(name, (v, schema.steps.len() as i64));
    }
    remove_any(&data.join(SET_PARTIAL))?;
    Ok(out)
}

/// The version of `name` as SQLite's recovery would leave it, read from a
/// byte copy in `pre-migration.partial/probe/` (the source is not written).
fn probe_version(data: &Path, name: &str) -> anyhow::Result<i64> {
    let probe = data.join(SET_PARTIAL).join("probe");
    remove_any(&data.join(SET_PARTIAL))?;
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(&probe)?;
    let cancel = AtomicBool::new(false);
    let mut buf = vec![0u8; crate::backup::BUF];
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let f = format!("{name}{suffix}");
        let (mut src, _) = match open_regular(&data.join(&f)) {
            Ok(x) => x,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("reading {f}")),
        };
        let mut out = create_private(&probe.join(&f))?;
        copy_stream(&mut src, &mut out, &mut buf, &cancel)?;
    }
    db::user_version_private_copy(&probe.join(name))
        .with_context(|| format!("reading {name}'s schema version from a copy"))
}

/// At a start, holding the data directory's lock and before any database
/// is opened: if any database will be migrated, copy all of them into
/// `pre-migration/`. A copy that cannot be written stops the start, unless
/// `without_copy` (the operator's `--migrate-without-copy`).
pub fn before_start(data: &Path, without_copy: bool) -> anyhow::Result<Option<Made>> {
    let v = versions(data)?;
    // Version 0 is a file being created, not migrated.
    let behind: Vec<&str> = v
        .iter()
        .filter(|(_, (have, want))| *have > 0 && have < want)
        .map(|(n, _)| *n)
        .collect();
    if behind.is_empty() {
        return Ok(None);
    }
    let list = behind.join(", ");
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
    // Each database is read twice beside the data: its byte copy, then the
    // snapshot from it.
    let size: u64 = v
        .keys()
        .flat_map(|n| ["", "-wal", "-shm", "-journal"].map(|s| data.join(format!("{n}{s}"))))
        .filter_map(|p| fs::symlink_metadata(p).ok())
        .map(|m| m.len())
        .sum();
    let need = size.saturating_mul(2);
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
    let before: Vec<String> = v
        .iter()
        .map(|(n, (have, _))| format!("{n} {have}"))
        .collect();
    Ok(Some(Made {
        advice: format!(
            "The databases as they were before this start (schema versions: {}) are in {}. To go \
             back to the older build: `{} --data-dir {} restore --pre-migration`, then start the \
             older build.",
            before.join(", "),
            data.join(SET).display(),
            harness_core::BRAND,
            data.display()
        ),
    }))
}

fn remove_any(p: &Path) -> io::Result<()> {
    match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(p),
        Ok(_) => fs::remove_file(p),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn write_set(data: &Path, v: &BTreeMap<&'static str, (i64, i64)>) -> anyhow::Result<()> {
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
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let f = format!("{name}{suffix}");
            let (mut src, _) = match open_regular(&data.join(&f)) {
                Ok(x) => x,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e).with_context(|| format!("reading {f}")),
            };
            let mut out = create_private(&work.join(&f))?;
            copy_stream(&mut src, &mut out, &mut buf, &cancel)?;
        }
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
            .map(|(n, (have, _))| ((*n).to_string(), *have))
            .collect(),
        target: v
            .iter()
            .map(|(n, (_, want))| ((*n).to_string(), *want))
            .collect(),
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
fn read_set(data: &Path) -> anyhow::Result<SetManifest> {
    let set = data.join(SET);
    match fs::symlink_metadata(&set) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => bail!("{} is not a folder: nothing was rolled back", set.display()),
        Err(_) => bail!(
            "{} holds no copy from before a migration (no {SET}/ folder): nothing to roll back to",
            data.display()
        ),
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

/// `restore --pre-migration`: under the data directory's lock (the caller
/// holds it: a running server's directory is refused), put the set back.
/// The upgraded databases are kept aside as `<file>.failed-upgrade-<time>`.
pub fn rollback(data: &Path) -> anyhow::Result<()> {
    let m = read_set(data)?;
    let set = data.join(SET);
    let stamp = harness_core::now_ms();
    let cancel = AtomicBool::new(false);
    let mut aside = Vec::new();
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
    }
    let mut buf = vec![0u8; crate::backup::BUF];
    for e in &m.files {
        let (mut f, _) = open_regular(&set.join(&e.path))?;
        let to = data.join(&e.path);
        let mut out = create_private(&to).with_context(|| format!("writing {}", e.path))?;
        let (n, h) = copy_declared(&mut f, &mut out, e.size, &mut buf, &cancel)
            .with_context(|| e.path.clone())?;
        if n != e.size || h != e.sha256 {
            bail!(
                "{} changed while it was put back: run the rollback again",
                e.path
            );
        }
        out.sync_all()?;
    }
    sync_dir(data)?;
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
    let versions: Vec<String> = m.before.iter().map(|(n, v)| format!("{n} {v}")).collect();
    println!(
        "put back {} databases from {} (schema versions {}), as they were before the start that \
         migrated them",
        m.files.len(),
        set.display(),
        versions.join(", ")
    );
    println!(
        "the upgraded files are kept aside, 0600: {} (delete them once the older build runs \
         well)",
        aside.join(", ")
    );
    println!(
        "Now start the older build: this build ({}) would migrate them again. Stored files were \
         not touched: one stored after the upgrade has no row now and is deleted at the next \
         start; one deleted after the upgrade is named by a row but gone (its download fails, an \
         Artifact version shows as failed).",
        env!("CARGO_PKG_VERSION")
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
         by {}; schema versions {}), {} in {}; it is replaced at the next migrating start; \
         delete the folder to free the space once the upgrade runs well",
        m.harness,
        versions.join(", "),
        mb(bytes),
        set.display()
    ))
}
