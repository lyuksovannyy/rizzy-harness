//! The blob store shared by Media and Artifacts: encrypted files under
//! `<data>/blobs`, quotas, upload slots and sweeps (docs/media.md).
//!
//! A file is written to `tmp/` while it is hashed and sealed, synced,
//! renamed into its shard, and only then recorded, in one store
//! transaction with the row that references it and the quota check. A
//! crash leaves at most a file without a row, which the sweeps delete.
//! Renames and row inserts hold `commit`, and so do the sweeps, so a sweep
//! never deletes a file whose row is about to be written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use axum::body::{Body, Bytes};
use axum::http::StatusCode;
use futures::StreamExt;
use harness_store::blobs::{Limits, OverQuota};
use serde::{Deserialize, Serialize};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};

use crate::blobs::{Writer, Written};
use crate::{Master, lock};

const MB: u64 = 1024 * 1024;
/// Uploads at once, per account and on the whole server; more are refused
/// (429), never queued.
const UPLOADS_PER_USER: usize = 2;
const UPLOADS_TOTAL: usize = 4;
/// An upload that sends nothing for this long, or runs this long in all,
/// is cut off (408).
const UPLOAD_IDLE: Duration = Duration::from_secs(30);
const UPLOAD_TOTAL: Duration = Duration::from_secs(30 * 60);
/// How often a running upload checks the free-disk floor.
const DISK_CHECK_EVERY: u64 = 16 * MB;
/// Files without a row, and temp files, younger than this are left to the
/// write that may still be committing them.
const SWEEP_AGE: Duration = Duration::from_secs(3600);

/// A refusal for the HTTP layer: status and a message for the user.
pub type Refusal = (StatusCode, String);

/// `[storage]` in config.toml; left out of the file while it is the
/// default. Sizes in MB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// One upload, one published file or folder pack.
    pub max_file_mb: u64,
    /// An account's Media and Artifacts together (0: unlimited).
    pub account_quota_mb: u64,
    /// Every stored file together.
    pub server_quota_mb: u64,
    /// Free space kept on the data disk.
    pub min_free_mb: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            max_file_mb: 200,
            account_quota_mb: 4096,
            server_quota_mb: 20480,
            min_free_mb: 1024,
        }
    }
}

impl StorageConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Why these settings cannot be saved, if they cannot.
    pub fn check(&self) -> Result<(), String> {
        // Far above any disk, and far below where MB → bytes overflows.
        const MAX: u64 = 1 << 30;
        if self.max_file_mb == 0 || self.server_quota_mb == 0 {
            return Err("the file size limit and the server quota must be at least 1 MB".into());
        }
        if [
            self.max_file_mb,
            self.account_quota_mb,
            self.server_quota_mb,
            self.min_free_mb,
        ]
        .iter()
        .any(|v| *v > MAX)
        {
            return Err(format!("sizes are at most {MAX} MB"));
        }
        Ok(())
    }

    pub fn max_file(&self) -> u64 {
        self.max_file_mb * MB
    }

    /// The account quota in bytes; 0 is unlimited.
    pub fn account_quota(&self) -> u64 {
        self.account_quota_mb * MB
    }

    /// The limits a new blob of `owner` (`None`: the API token, which has
    /// no account quota) must fit.
    pub fn limits(&self, owner: Option<&str>) -> Limits {
        Limits {
            account: if owner.is_some() {
                self.account_quota()
            } else {
                0
            },
            server: self.server_quota_mb * MB,
        }
    }
}

/// The open blob store, in `MasterInner.storage`.
pub struct Storage {
    /// `<data>/blobs`; files in `<root>/<xx>/`, temp files in `<root>/tmp`.
    root: PathBuf,
    /// Held by renames into shards with their row inserts, and by sweeps.
    commit: tokio::sync::Mutex<()>,
    uploads: Slots,
    /// Upload idle and total limits (tests shorten them).
    timeouts: Mutex<(Duration, Duration)>,
}

/// Upload slots per account and on the whole server.
struct Slots {
    per_user: Arc<Mutex<HashMap<String, usize>>>,
    total: Arc<Semaphore>,
}

/// One upload slot, given back when dropped.
pub struct Slot {
    user: String,
    per_user: Arc<Mutex<HashMap<String, usize>>>,
    _total: OwnedSemaphorePermit,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut m = lock(&self.per_user);
        if let Some(n) = m.get_mut(&self.user) {
            *n -= 1;
            if *n == 0 {
                m.remove(&self.user);
            }
        }
    }
}

impl Storage {
    fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// Where blob `blob_id`'s file is (`None` for a malformed id).
    pub fn path(&self, blob_id: &str) -> Option<PathBuf> {
        crate::blobs::path(&self.root, blob_id)
    }

    /// An upload slot for `user` (`None`: the API token), if one is free.
    pub fn upload_slot(&self, user: Option<&str>) -> Option<Slot> {
        let user = user.unwrap_or("").to_string();
        let mut m = lock(&self.uploads.per_user);
        if m.get(&user).is_some_and(|n| *n >= UPLOADS_PER_USER) {
            return None;
        }
        // Counted only once the server-wide slot is taken, so a refusal
        // leaves no entry behind.
        let permit = self.uploads.total.clone().try_acquire_owned().ok()?;
        *m.entry(user.clone()).or_default() += 1;
        Some(Slot {
            user,
            per_user: self.uploads.per_user.clone(),
            _total: permit,
        })
    }

    fn timeouts(&self) -> (Duration, Duration) {
        *lock(&self.timeouts)
    }
}

/// The blob store, once opened.
pub fn get(m: &Master) -> Option<&Storage> {
    m.inner.storage.get()
}

/// The current `[storage]` settings (defaults without a managed config).
pub fn config(m: &Master) -> StorageConfig {
    m.provider_runtime()
        .map(|rt| rt.config().storage)
        .unwrap_or_default()
}

/// Shorten the upload limits (tests: a 30 s idle wait is too long).
pub fn set_upload_timeouts(m: &Master, idle: Duration, total: Duration) {
    if let Some(st) = get(m) {
        *lock(&st.timeouts) = (idle, total);
    }
}

fn private_dir(p: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(p)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Open the blob store under `data_dir`. Called before the first node link,
/// so a replayed publish always finds it. Leftovers of an interrupted run
/// go: every temp file, and every file without a row.
pub fn open(m: &Master, data_dir: &Path) -> std::io::Result<()> {
    let root = data_dir.join("blobs");
    private_dir(&root)?;
    let st = Storage {
        commit: tokio::sync::Mutex::new(()),
        uploads: Slots {
            per_user: Arc::default(),
            total: Arc::new(Semaphore::new(UPLOADS_TOTAL)),
        },
        timeouts: Mutex::new((UPLOAD_IDLE, UPLOAD_TOTAL)),
        root,
    };
    let _ = std::fs::remove_dir_all(st.tmp());
    private_dir(&st.tmp())?;
    let known = m
        .inner
        .mirror
        .known_blob_ids()
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let removed = remove_orphans(&st.root, &known, None);
    if removed > 0 {
        tracing::info!(files = removed, "removed stored files without a row");
    }
    fail_missing(m, &st).map_err(|e| std::io::Error::other(e.to_string()))?;
    let _ = m.inner.storage.set(st);
    Ok(())
}

/// Ready artifact versions whose file is gone (a restored database without
/// its `blobs/`) become failed instead of failing every download.
fn fail_missing(m: &Master, st: &Storage) -> harness_store::Result<()> {
    let missing: Vec<String> = m
        .inner
        .mirror
        .artifact_ready_blobs()?
        .into_iter()
        .filter(|id| st.path(id).is_some_and(|p| !p.exists()))
        .collect();
    if !missing.is_empty() {
        let n = m.inner.mirror.artifact_fail_missing(&missing)?;
        tracing::warn!(versions = n, "artifact files are missing; marked failed");
    }
    Ok(())
}

/// Delete shard files whose blob has no row (only those older than `age`,
/// when given). Returns how many went.
fn remove_orphans(
    root: &Path,
    known: &std::collections::HashSet<String>,
    age: Option<Duration>,
) -> usize {
    let Ok(shards) = std::fs::read_dir(root) else {
        return 0;
    };
    let mut n = 0;
    for shard in shards.flatten() {
        let name = shard.file_name();
        let name = name.to_string_lossy();
        if name.len() != 2 || !shard.path().is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(shard.path()) else {
            continue;
        };
        for f in files.flatten() {
            let fname = f.file_name();
            let Some(id) = fname
                .to_string_lossy()
                .strip_suffix(".rzb")
                .map(str::to_string)
            else {
                continue;
            };
            if known.contains(&id) || age.is_some_and(|a| younger(&f.path(), a)) {
                continue;
            }
            if std::fs::remove_file(f.path()).is_ok() {
                n += 1;
            }
        }
    }
    n
}

/// Modified less than `age` ago (unknown times count as young).
fn younger(p: &Path, age: Duration) -> bool {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_none_or(|d| d < age)
}

/// Free and total bytes on the data disk.
pub fn disk(m: &Master) -> Option<(u64, u64)> {
    let st = get(m)?;
    let s = nix::sys::statvfs::statvfs(st.root.as_path()).ok()?;
    let frag = s.fragment_size() as u64;
    Some((
        (s.blocks_available() as u64).saturating_mul(frag),
        (s.blocks() as u64).saturating_mul(frag),
    ))
}

/// Refuse when writing `more` bytes would leave less than the free-disk
/// floor.
fn disk_floor(m: &Master, cfg: &StorageConfig, more: u64) -> Result<(), Refusal> {
    match disk(m) {
        Some((free, _)) if free < (cfg.min_free_mb * MB).saturating_add(more) => Err((
            StatusCode::INSUFFICIENT_STORAGE,
            "The server is almost out of disk space; ask an admin.".into(),
        )),
        _ => Ok(()),
    }
}

/// The free-disk floor for a write of `more` bytes, as a user message.
pub(crate) fn disk_ok(m: &Master, more: u64) -> Result<(), String> {
    disk_floor(m, &config(m), more).map_err(|(_, msg)| msg)
}

/// Whether `size` more bytes of `owner`'s fit: the file limit, the
/// account's and the server's quota, the free-disk floor. Checked before a
/// published file moves; the insert transaction checks the quotas again.
pub async fn check_room(m: &Master, owner: Option<&str>, size: u64) -> Result<(), String> {
    let cfg = config(m);
    if size > cfg.max_file() {
        return Err(format!(
            "it is larger than the {} MB limit for one file",
            cfg.max_file_mb
        ));
    }
    let o = owner.map(str::to_string);
    let (used, total) = m
        .inner
        .mirror
        .call(move |s| {
            Ok::<_, harness_store::StoreError>((s.blob_usage(o.as_deref())?.used, s.blob_total()?))
        })
        .await
        .map_err(|e| e.to_string())?;
    let limits = cfg.limits(owner);
    for (used, limit, server) in [(used, limits.account, false), (total, limits.server, true)] {
        if limit > 0 && used.saturating_add(size) > limit {
            return Err(full_message(&OverQuota {
                used,
                limit,
                server,
            }));
        }
    }
    disk_ok(m, size)
}

/// How a full quota reads to the user.
pub fn full_message(q: &OverQuota) -> String {
    let gb = |b: u64| b as f64 / (1024.0 * MB as f64);
    if q.server {
        format!(
            "The server's storage is full ({:.1} of {:.0} GB); ask an admin.",
            gb(q.used),
            gb(q.limit)
        )
    } else {
        format!(
            "Your storage is full ({:.1} of {:.0} GB). Delete files in Media or Artifacts to make room.",
            gb(q.used),
            gb(q.limit)
        )
    }
}

/// What `owner` may still store: the smaller of the account's and the
/// server's room. The authoritative check is in the insert transaction;
/// this one refuses early (before a body is read).
async fn room(m: &Master, cfg: &StorageConfig, owner: Option<&str>) -> Result<u64, Refusal> {
    let o = owner.map(str::to_string);
    let (used, total) = m
        .inner
        .mirror
        .call(move |s| {
            Ok::<_, harness_store::StoreError>((s.blob_usage(o.as_deref())?.used, s.blob_total()?))
        })
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let limits = cfg.limits(owner);
    let mut room = u64::MAX;
    for (used, limit, server) in [(used, limits.account, false), (total, limits.server, true)] {
        if limit == 0 {
            continue;
        }
        let left = limit.saturating_sub(used);
        if left == 0 {
            return Err((
                StatusCode::INSUFFICIENT_STORAGE,
                full_message(&OverQuota {
                    used,
                    limit,
                    server,
                }),
            ));
        }
        room = room.min(left);
    }
    Ok(room)
}

/// Bytes handed to the writer thread; a closed channel without `End` is an
/// abandoned file.
enum Chunk {
    Data(Bytes),
    End,
}

/// A new file in `tmp/`, hashed and sealed on a blocking thread as bytes
/// arrive (an upload, or a file pulled from a node). Dropped or abandoned
/// before `finish`, its file is deleted.
pub(crate) struct TempWriter {
    tx: mpsc::Sender<Chunk>,
    handle: tokio::task::JoinHandle<std::io::Result<Option<Written>>>,
}

impl TempWriter {
    pub(crate) fn start(st: &Storage) -> std::io::Result<Self> {
        let blob_id = harness_core::new_id();
        let mut w = Writer::create(&st.tmp(), &blob_id, crate::sniff::HEAD)?;
        let (tx, mut rx) = mpsc::channel::<Chunk>(8);
        let handle = tokio::task::spawn_blocking(move || -> std::io::Result<Option<Written>> {
            loop {
                match rx.blocking_recv() {
                    Some(Chunk::Data(b)) => w.write(&b)?,
                    Some(Chunk::End) => return w.finish().map(Some),
                    // Abandoned: dropping the writer deletes its file.
                    None => return Ok(None),
                }
            }
        });
        Ok(Self { tx, handle })
    }

    /// False when the writer stopped (a disk error).
    pub(crate) async fn write(&self, b: Bytes) -> bool {
        self.tx.send(Chunk::Data(b)).await.is_ok()
    }

    /// Seal the last segment and sync: the file, ready to commit.
    pub(crate) async fn finish(self) -> std::io::Result<Written> {
        let _ = self.tx.send(Chunk::End).await;
        drop(self.tx);
        match self.handle.await.map_err(std::io::Error::other)?? {
            Some(w) => Ok(w),
            None => Err(std::io::Error::other("the file was not finished")),
        }
    }

    /// Stop and delete the file, waiting until it is gone.
    pub(crate) async fn abandon(self) {
        drop(self.tx);
        let _ = self.handle.await;
    }
}

/// Receive an upload body into a new temp file, with every limit checked
/// as it streams: `max_file` (413), the owner's and the server's room and
/// the free-disk floor (507, from `Content-Length` before any byte is
/// read), 30 s without a byte or 30 min in all (408). The caller holds an
/// upload slot.
pub async fn receive(
    m: &Master,
    owner: Option<&str>,
    content_length: Option<u64>,
    body: Body,
) -> Result<Written, Refusal> {
    let st = get(m).ok_or((StatusCode::NOT_FOUND, "file storage is off".into()))?;
    let cfg = config(m);
    let max = cfg.max_file();
    let too_large = || {
        (
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("Files are at most {} MB.", cfg.max_file_mb),
        )
    };
    let room = room(m, &cfg, owner).await?;
    let full = || {
        (
            StatusCode::INSUFFICIENT_STORAGE,
            "This file does not fit in your remaining storage.".to_string(),
        )
    };
    if let Some(len) = content_length {
        if len > max {
            return Err(too_large());
        }
        if len > room {
            return Err(full());
        }
    }
    disk_floor(m, &cfg, content_length.unwrap_or(0))?;

    let writer = TempWriter::start(st).map_err(|e| {
        tracing::warn!(error = %e, "cannot start a stored file");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not write the file".to_string(),
        )
    })?;

    let (idle, total) = st.timeouts();
    let deadline = Instant::now() + total;
    let mut stream = body.into_data_stream();
    let mut size = 0u64;
    let mut checked = 0u64;
    let outcome: Result<(), Refusal> = loop {
        let wait = idle.min(deadline.saturating_duration_since(Instant::now()));
        let next = match tokio::time::timeout(wait, stream.next()).await {
            Ok(n) => n,
            Err(_) => {
                break Err((
                    StatusCode::REQUEST_TIMEOUT,
                    "The upload stalled; try again.".into(),
                ));
            }
        };
        let bytes = match next {
            None => break Ok(()),
            Some(Ok(b)) => b,
            Some(Err(_)) => {
                break Err((
                    StatusCode::BAD_REQUEST,
                    "The upload was interrupted.".into(),
                ));
            }
        };
        size += bytes.len() as u64;
        if size > max {
            break Err(too_large());
        }
        if size > room {
            break Err(full());
        }
        if size - checked >= DISK_CHECK_EVERY {
            checked = size;
            if let Err(e) = disk_floor(m, &cfg, 0) {
                break Err(e);
            }
        }
        if !writer.write(bytes).await {
            break Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not write the file".into(),
            ));
        }
    };
    if let Err(e) = outcome {
        writer.abandon().await;
        return Err(e);
    }
    writer.finish().await.map_err(|e| {
        tracing::warn!(error = %e, "writing an upload failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not write the file".into(),
        )
    })
}

/// Move a written file into its shard and run `record` (the store
/// transaction that inserts its row) under the commit lock. `record` says
/// whether the file is still needed (false: a dedupe hit or a refusal);
/// unneeded or failed, the file is deleted.
pub async fn commit<T, F>(m: &Master, w: Written, record: F) -> Result<T, Refusal>
where
    F: FnOnce(&harness_store::Store) -> harness_store::Result<(T, bool)> + Send + 'static,
    T: Send + 'static,
{
    let st = get(m).ok_or((StatusCode::NOT_FOUND, "file storage is off".into()))?;
    let dest = st
        .path(&w.blob_id)
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "bad blob id".into()))?;
    let _g = st.commit.lock().await;
    let (src, to) = (w.path.clone(), dest.clone());
    let moved = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        let dir = to
            .parent()
            .ok_or_else(|| std::io::Error::other("no shard"))?;
        private_dir(dir)?;
        std::fs::rename(&src, &to)?;
        std::fs::File::open(dir)?.sync_all()
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Err(e) = moved {
        let _ = std::fs::remove_file(&w.path);
        let _ = std::fs::remove_file(&dest);
        tracing::warn!(error = %e, "could not store a file");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not store the file".into(),
        ));
    }
    let res = m.inner.mirror.call(record).await;
    match res {
        Ok((v, true)) => Ok(v),
        Ok((v, false)) => {
            let _ = std::fs::remove_file(&dest);
            Ok(v)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&dest);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

/// Delete stored files whose rows are gone (the WAL already truncated by
/// the caller, so the keys left the live files first).
pub async fn unlink(m: &Master, blob_ids: Vec<String>) {
    let Some(st) = get(m) else { return };
    let paths: Vec<PathBuf> = blob_ids.iter().filter_map(|id| st.path(id)).collect();
    let _ = tokio::task::spawn_blocking(move || {
        for p in paths {
            let _ = std::fs::remove_file(p);
        }
    })
    .await;
}

/// After an account was deleted: its blob rows went with it (cascade), so
/// checkpoint the WAL, then unlink every file without a row.
pub async fn on_user_deleted(m: &Master) {
    let Some(st) = get(m) else { return };
    let _g = st.commit.lock().await;
    let known = m
        .inner
        .mirror
        .call(|s| {
            s.checkpoint()?;
            s.known_blob_ids()
        })
        .await;
    match known {
        Ok(known) => {
            let root = st.root.clone();
            let _ = tokio::task::spawn_blocking(move || remove_orphans(&root, &known, None)).await;
        }
        Err(e) => tracing::warn!(error = %e, "could not sweep a deleted account's files"),
    }
}

/// The hourly part of the blob store: Artifacts' retention, unreferenced
/// rows, then temp files and files without a row, each older than an hour.
pub async fn hourly(m: &Master) {
    let Some(st) = get(m) else { return };
    let _g = st.commit.lock().await;
    let released = crate::artifacts::hourly(m).await;
    let before = harness_core::now_ms() - SWEEP_AGE.as_millis() as i64;
    let res = m
        .inner
        .mirror
        .call(move |s| {
            let stale = s.blobs_unreferenced(before)?;
            let mut gone = s.blob_delete(&stale)?;
            gone.extend(released);
            if !gone.is_empty() {
                s.checkpoint()?;
            }
            Ok::<_, harness_store::StoreError>((gone, s.known_blob_ids()?))
        })
        .await;
    let (gone, known) = match res {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "storage sweep failed");
            return;
        }
    };
    let (root, tmp) = (st.root.clone(), st.tmp());
    let paths: Vec<PathBuf> = gone.iter().filter_map(|id| st.path(id)).collect();
    let _ = tokio::task::spawn_blocking(move || {
        for p in paths {
            let _ = std::fs::remove_file(p);
        }
        if let Ok(files) = std::fs::read_dir(&tmp) {
            for f in files.flatten() {
                if !younger(&f.path(), SWEEP_AGE) {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
        remove_orphans(&root, &known, Some(SWEEP_AGE));
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_checks() {
        assert!(StorageConfig::default().check().is_ok());
        let zero = StorageConfig {
            max_file_mb: 0,
            ..Default::default()
        };
        assert!(zero.check().is_err());
        let huge = StorageConfig {
            server_quota_mb: u64::MAX,
            ..Default::default()
        };
        assert!(huge.check().is_err());
        let unlimited = StorageConfig {
            account_quota_mb: 0,
            ..Default::default()
        };
        assert!(unlimited.check().is_ok());
        assert_eq!(unlimited.limits(Some("u")).account, 0);
        let d = StorageConfig::default();
        assert_eq!(d.max_file(), 209_715_200);
        assert_eq!(
            d.limits(None).account,
            0,
            "the API token has no account quota"
        );
        assert_eq!(d.limits(Some("u")).account, 4096 * MB);
    }

    #[test]
    fn full_messages() {
        let q = OverQuota {
            used: 4 * 1024 * MB,
            limit: 4 * 1024 * MB,
            server: false,
        };
        assert!(full_message(&q).starts_with("Your storage is full (4.0 of 4 GB)"));
        let s = OverQuota { server: true, ..q };
        assert!(full_message(&s).starts_with("The server's storage is full"));
    }
}
