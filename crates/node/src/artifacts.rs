//! Staging files an agent or the user publishes, for the master to pull
//! (docs/artifacts.md, PLAN T18).
//!
//! A path is confined to the chat's folder (or, where node.toml allows
//! work outside it, to a workspace root), never reaches the node's data
//! dir, and is opened without following a final symlink, as a regular file,
//! with the descriptor's real path checked again. The bytes are then copied
//! (a folder packed into `.tar.gz`) into `<stage_dir>/<stage_id>/data`, so
//! what the master pulls cannot change under it. The master names a stage
//! only by its id, which this module looks up in its own index: an id is
//! never joined into a path, and release removes only an indexed folder.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use harness_core::artifacts::{
    ArtifactMeta, ArtifactSink, Entry, GitInfo, MAX_LISTED, MAX_PER_TURN, Package, PublishRequest,
    Published, StagedBy, valid_stage_id,
};
use harness_core::event::{ArtifactStaged, EventKind};
use harness_core::{SessionId, now_ms};
use harness_protocol::{MAX_CHUNK, NodeToMaster};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::Inner;
use crate::policy::{ArtifactsPolicy, NodePolicy};

/// Where a worker node stages published files, and the data dir that is
/// never published from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageConfig {
    /// The runtime's data dir (for the embedded node, the master's): keys,
    /// secrets and databases there are refused whatever the path says.
    pub data_dir: PathBuf,
    pub stage_dir: PathBuf,
}

impl StageConfig {
    /// Stages under `<data_dir>/artifacts-stage`. Built through here, so
    /// fields added later need no change where runtimes are configured.
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            stage_dir: data_dir.join("artifacts-stage"),
            data_dir,
        }
    }
}

/// Fetches read at once on this node (two transfers with four requests in
/// flight each); more get a `busy` chunk and are asked again.
const FETCHES: usize = 8;
const MAX_PACK_ENTRIES: u64 = 10_000;
const MAX_PACK_PATH: usize = 255;
const MAX_DEPTH: usize = 32;
/// Manifests are read for a name and a version, never whole.
const MANIFEST_READ: u64 = 64 * 1024;
const RECORD_READ: u64 = 64 * 1024;
const DATA: &str = "data";
const RECORD: &str = "meta.json";
const MB: u64 = 1024 * 1024;

/// Names the agent may never publish and packs always skip (any path
/// component). The user may save such a file explicitly.
const SECRET_NAMES: &[&str] = &[
    ".env*",
    "*.pem",
    "*.key",
    "*.p12",
    "*.pfx",
    "id_rsa*",
    "id_ed25519*",
    ".netrc",
    ".npmrc",
    ".pypirc",
    "credentials*",
    ".ssh",
    ".gnupg",
    ".aws",
];

/// Where a fetch's answer goes (the master link).
pub(crate) type Reply = Arc<dyn Fn(NodeToMaster) + Send + Sync>;

/// What a stage folder records next to its data, so the index can be
/// rebuilt at start.
#[derive(Debug, Serialize, Deserialize)]
struct Record {
    session_id: SessionId,
    size: u64,
    sha256: String,
    created_ms: i64,
    staged: ArtifactStaged,
}

/// One staged file in the index.
pub(crate) struct Stage {
    dir: PathBuf,
    size: u64,
    created_ms: i64,
    /// Fetches of one stage answer one at a time, in arrival order.
    order: tokio::sync::Mutex<()>,
}

/// A path that passed the checks: its real location and the root it is
/// confined to.
#[derive(Debug, Clone)]
pub(crate) struct Resolved {
    pub path: PathBuf,
    root: PathBuf,
    is_dir: bool,
}

/// What to stage, and for whom.
pub(crate) struct Request {
    pub session: SessionId,
    pub by: StagedBy,
    pub name: Option<String>,
    pub notes: Option<String>,
    pub kind_hint: Option<String>,
    pub call_id: Option<String>,
}

/// The node's stage: its folder, limits and index.
pub(crate) struct Stages {
    dir: PathBuf,
    /// Canonical: everything under it is refused.
    data_dir: PathBuf,
    policy: ArtifactsPolicy,
    /// Canonical workspace roots, for nodes that allow work outside the
    /// chat's folder.
    roots: Vec<PathBuf>,
    allow_outside: bool,
    index: Mutex<HashMap<(SessionId, String), Arc<Stage>>>,
    fetches: Arc<Semaphore>,
    /// The node's checkpoint store: whether a published file's repository
    /// has uncommitted changes is asked of its shadow there, never of
    /// `git status` in the user's repository (R.1, S-1).
    checkpoints: Option<PathBuf>,
    /// The sandbox that git runs in (R1b-2).
    git_sandbox: Option<Arc<harness_tools::sandbox::Sandbox>>,
}

fn private_dir(p: &Path) -> io::Result<()> {
    std::fs::create_dir_all(p)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn private_file(p: &Path) -> io::Result<File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(p)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// `*` matches within one path segment, `**` across segments, `?` one
/// character (not `/`). Byte-wise, which is exact for the ASCII patterns
/// node.toml holds.
fn glob(pat: &[u8], s: &[u8]) -> bool {
    match pat.first() {
        None => s.is_empty(),
        Some(b'*') if pat.get(1) == Some(&b'*') => {
            let rest = pat[2..].strip_prefix(b"/").unwrap_or(&pat[2..]);
            (0..=s.len()).any(|i| glob(rest, &s[i..]))
        }
        Some(b'*') => (0..=s.len())
            .take_while(|&i| i == 0 || s[i - 1] != b'/')
            .any(|i| glob(&pat[1..], &s[i..])),
        Some(b'?') => s.first().is_some_and(|&c| c != b'/') && glob(&pat[1..], &s[1..]),
        Some(&c) => s.first() == Some(&c) && glob(&pat[1..], &s[1..]),
    }
}

/// Does any component of `rel` look like a secret?
fn is_secret(rel: &Path) -> bool {
    rel.components().any(|c| {
        let name = c.as_os_str().to_string_lossy();
        SECRET_NAMES
            .iter()
            .any(|p| glob(p.as_bytes(), name.as_bytes()))
    })
}

/// Why a path cannot be opened for publishing.
#[derive(Debug)]
enum OpenError {
    /// A symlink, FIFO, socket or device.
    Special,
    /// Its real location is outside the root, or in the data dir.
    Outside,
    Io(io::Error),
}

impl OpenError {
    fn message(&self, shown: &str) -> String {
        match self {
            OpenError::Special => format!("{shown} is not a regular file"),
            OpenError::Outside => {
                format!("{shown} is outside the folders this chat may publish from")
            }
            OpenError::Io(e) => format!("{shown}: {e}"),
        }
    }
}

/// Open `path` for reading without following a final symlink or blocking
/// on a FIFO, check it is a regular file, and check where the descriptor
/// really points (a folder swapped for a symlink after the path was
/// checked shows up here). Bytes are then read from the descriptor only.
fn open_regular(path: &Path, root: &Path, data_dir: &Path) -> Result<File, OpenError> {
    use nix::fcntl::OFlag;
    use std::os::unix::fs::OpenOptionsExt;
    let flags = OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits())
        .open(path)
        .map_err(|e| match e.raw_os_error() {
            // O_NOFOLLOW on a symlink.
            Some(code) if code == nix::libc::ELOOP => OpenError::Special,
            _ => OpenError::Io(e),
        })?;
    let md = f.metadata().map_err(OpenError::Io)?;
    if !md.file_type().is_file() {
        return Err(OpenError::Special);
    }
    let real = real_path(&f, path).map_err(OpenError::Io)?;
    if !real.starts_with(root) || real.starts_with(data_dir) {
        return Err(OpenError::Outside);
    }
    Ok(f)
}

/// Where an open descriptor points: `/proc/self/fd` on Linux; elsewhere the
/// path again, accepted only when it still names the same file.
#[cfg(target_os = "linux")]
fn real_path(f: &File, _path: &Path) -> io::Result<PathBuf> {
    use std::os::fd::AsRawFd;
    std::fs::read_link(format!("/proc/self/fd/{}", f.as_raw_fd()))
}

#[cfg(not(target_os = "linux"))]
fn real_path(f: &File, path: &Path) -> io::Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let (a, b) = (f.metadata()?, std::fs::symlink_metadata(path)?);
    if a.dev() != b.dev() || a.ino() != b.ino() {
        return Err(io::Error::other("the file changed while it was opened"));
    }
    path.canonicalize()
}

/// Exactly `left` bytes from `inner`, or an error: a file that shrinks
/// while it is packed must fail the pack, not corrupt it.
struct Exact<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for Exact<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 {
            return Ok(0);
        }
        let want = buf
            .len()
            .min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let n = self.inner.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "a file got shorter while it was packed",
            ));
        }
        self.left -= n as u64;
        Ok(n)
    }
}

/// Writes through, hashing and counting, and refuses past `max` bytes.
struct Hashing<W> {
    inner: W,
    hasher: Sha256,
    size: u64,
    max: u64,
}

impl<W: Write> Write for Hashing<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.size + buf.len() as u64 > self.max {
            return Err(io::Error::other(too_large(self.max)));
        }
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.size += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn too_large(max: u64) -> String {
    format!("it is larger than this node's {} MB limit", max / MB)
}

fn cancelled() -> io::Error {
    io::Error::other("cancelled")
}

/// Copy one regular file into `dest`, hashing it in the same pass and
/// stopping once it passes `max` bytes.
fn copy_file(
    mut f: File,
    dest: &Path,
    max: u64,
    cancel: &CancellationToken,
) -> io::Result<(u64, String)> {
    let mut out = Hashing {
        inner: private_file(dest)?,
        hasher: Sha256::new(),
        size: 0,
        max,
    };
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        if cancel.is_cancelled() {
            return Err(cancelled());
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
    }
    out.inner.sync_all()?;
    Ok((out.size, hex::encode(out.hasher.finalize())))
}

/// A folder pack's outcome.
struct Packed {
    size: u64,
    sha256: String,
    listed: Vec<Entry>,
    total: u64,
    skipped: BTreeMap<String, u64>,
}

/// What a pack does with one walked entry.
enum Walked {
    Dir,
    File(File, u64),
    Skip(&'static str),
}

/// Pack `folder` (regular files and folders only) into `dest` as tar over
/// gzip. Symlinks, special files, secret-like names, `.git`, the data dir
/// and names node.toml denies are skipped and counted; more than 10,000
/// entries or `max` bytes uncompressed fails the pack.
struct Packer<'a> {
    stages: &'a Stages,
    folder: &'a Path,
    root: &'a Path,
    cancel: &'a CancellationToken,
}

impl Packer<'_> {
    fn pack(&self, dest: &Path) -> io::Result<Packed> {
        let max = self.stages.max_bytes();
        let out = Hashing {
            inner: private_file(dest)?,
            hasher: Sha256::new(),
            size: 0,
            max,
        };
        let gz = flate2::write::GzEncoder::new(out, flate2::Compression::default());
        let mut tar = tar::Builder::new(gz);
        let top = self
            .folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "folder".into());
        let (mut listed, mut total, mut bytes) = (Vec::new(), 0u64, 0u64);
        let mut skipped: BTreeMap<String, u64> = BTreeMap::new();
        let mut skip = |why: &str| *skipped.entry(why.into()).or_default() += 1;
        let walk = walkdir::WalkDir::new(self.folder)
            .follow_links(false)
            .max_depth(MAX_DEPTH)
            .sort_by_file_name();
        let mut it = walk.into_iter();
        while let Some(next) = it.next() {
            if self.cancel.is_cancelled() {
                return Err(cancelled());
            }
            let entry = match next {
                Ok(e) => e,
                Err(_) => {
                    skip("unreadable");
                    continue;
                }
            };
            let rel = entry
                .path()
                .strip_prefix(self.folder)
                .unwrap_or(Path::new(""));
            let name = if rel.as_os_str().is_empty() {
                top.clone()
            } else {
                format!("{top}/{}", rel.to_string_lossy())
            };
            let walked = self.classify(&entry, rel, name.len());
            if let Walked::Skip(why) = walked {
                if entry.file_type().is_dir() && entry.depth() > 0 {
                    it.skip_current_dir();
                }
                skip(why);
                continue;
            }
            total += 1;
            if total > MAX_PACK_ENTRIES {
                return Err(io::Error::other(format!(
                    "the folder has more than {MAX_PACK_ENTRIES} entries"
                )));
            }
            let md = entry.metadata().map_err(io::Error::other)?;
            let mut h = tar::Header::new_gnu();
            {
                use std::os::unix::fs::MetadataExt;
                h.set_mode(md.mode() & 0o7777);
                h.set_mtime(md.mtime().max(0) as u64);
            }
            h.set_uid(0);
            h.set_gid(0);
            match walked {
                Walked::Dir => {
                    h.set_entry_type(tar::EntryType::Directory);
                    h.set_size(0);
                    tar.append_data(&mut h, format!("{name}/"), io::empty())?;
                }
                Walked::File(f, size) => {
                    bytes += size;
                    if bytes > max {
                        return Err(io::Error::other(too_large(max)));
                    }
                    if listed.len() < MAX_LISTED {
                        listed.push(Entry {
                            path: name.clone(),
                            size,
                        });
                    }
                    h.set_entry_type(tar::EntryType::Regular);
                    h.set_size(size);
                    tar.append_data(
                        &mut h,
                        &name,
                        Exact {
                            inner: f,
                            left: size,
                        },
                    )?;
                }
                Walked::Skip(_) => unreachable!("skipped above"),
            }
        }
        let gz = tar.into_inner()?;
        let out = gz.finish()?;
        out.inner.sync_all()?;
        Ok(Packed {
            size: out.size,
            sha256: hex::encode(out.hasher.finalize()),
            listed,
            total,
            skipped,
        })
    }

    fn classify(&self, entry: &walkdir::DirEntry, rel: &Path, name_len: usize) -> Walked {
        let ft = entry.file_type();
        let path = entry.path();
        if path.starts_with(&self.stages.data_dir) {
            return Walked::Skip("data_dir");
        }
        if ft.is_dir() && entry.depth() > 0 && entry.file_name() == ".git" {
            return Walked::Skip("git");
        }
        if is_secret(rel) {
            return Walked::Skip("secret");
        }
        if self.stages.denied(rel) {
            return Walked::Skip("denied");
        }
        if name_len > MAX_PACK_PATH {
            return Walked::Skip("long_path");
        }
        if ft.is_symlink() {
            return Walked::Skip("symlink");
        }
        if ft.is_dir() {
            return Walked::Dir;
        }
        if !ft.is_file() {
            return Walked::Skip("special");
        }
        match open_regular(path, self.root, &self.stages.data_dir) {
            Ok(f) => match f.metadata() {
                Ok(md) => Walked::File(f, md.len()),
                Err(_) => Walked::Skip("unreadable"),
            },
            Err(OpenError::Special) => Walked::Skip("special"),
            Err(OpenError::Outside) => Walked::Skip("outside"),
            Err(OpenError::Io(_)) => Walked::Skip("unreadable"),
        }
    }
}

impl Stages {
    /// Use the checkpoint store under `dir` for a published file's
    /// repository state (see [`Stages::checkpoints`]).
    pub(crate) fn with_checkpoints(
        mut self,
        dir: Option<PathBuf>,
        sandbox: Option<Arc<harness_tools::sandbox::Sandbox>>,
    ) -> Self {
        self.checkpoints = dir;
        self.git_sandbox = sandbox;
        self
    }

    /// Open the stage under `cfg.stage_dir` and rebuild its index from the
    /// records on disk. Stages of sessions this node does not host, and
    /// anything that does not read back, are deleted.
    pub(crate) fn open(
        cfg: &StageConfig,
        policy: &NodePolicy,
        hosted: &HashSet<SessionId>,
    ) -> io::Result<Self> {
        private_dir(&cfg.stage_dir)?;
        let data_dir = cfg.data_dir.canonicalize()?;
        let stages = Self {
            dir: cfg.stage_dir.canonicalize()?,
            data_dir,
            policy: policy.artifacts.clone(),
            roots: policy
                .workspace_roots
                .iter()
                .filter_map(|r| r.canonicalize().ok())
                .collect(),
            allow_outside: policy.allow_outside_workspace,
            index: Mutex::new(HashMap::new()),
            fetches: Arc::new(Semaphore::new(FETCHES)),
            checkpoints: None,
            git_sandbox: None,
        };
        for entry in std::fs::read_dir(&stages.dir)?.flatten() {
            let dir = entry.path();
            let id = entry.file_name().to_string_lossy().into_owned();
            match read_record(&dir)
                .filter(|r| valid_stage_id(&id) && hosted.contains(&r.session_id))
            {
                Some(r) => {
                    lock(&stages.index).insert(
                        (r.session_id, id),
                        Arc::new(Stage {
                            dir,
                            size: r.size,
                            created_ms: r.created_ms,
                            order: Default::default(),
                        }),
                    );
                }
                None => {
                    let _ = std::fs::remove_dir_all(&dir);
                }
            }
        }
        stages.expire();
        Ok(stages)
    }

    fn max_bytes(&self) -> u64 {
        self.policy.max_mb.saturating_mul(MB)
    }

    /// Does node.toml's `deny_globs` name `rel` (the whole relative path,
    /// or its last component)?
    fn denied(&self, rel: &Path) -> bool {
        let whole = rel.to_string_lossy();
        let last = rel
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        self.policy
            .deny_globs
            .iter()
            .any(|g| glob(g.as_bytes(), whole.as_bytes()) || glob(g.as_bytes(), last.as_bytes()))
    }

    /// Check a path from the `publish` tool (`by` agent) or the user's
    /// "Save as artifact". Fast: no file is read.
    pub(crate) fn check(&self, cwd: &Path, raw: &str, by: StagedBy) -> Result<Resolved, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("the path is empty".into());
        }
        let joined = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            cwd.join(raw)
        };
        let path = joined
            .canonicalize()
            .map_err(|_| format!("{raw}: no such file or folder"))?;
        let cwd = cwd
            .canonicalize()
            .map_err(|e| format!("the chat's folder: {e}"))?;
        let root = if path.starts_with(&cwd) {
            cwd
        } else if self.allow_outside {
            // A hard floor the file tools lack: only inside a root.
            self.roots
                .iter()
                .find(|r| path.starts_with(r))
                .cloned()
                .ok_or_else(|| format!("{raw} is outside this node's workspace roots"))?
        } else {
            return Err(format!("{raw} is outside the chat's folder"));
        };
        if path.starts_with(&self.data_dir) {
            return Err("the node's data folder is never published".into());
        }
        let rel = path.strip_prefix(&root).unwrap_or(Path::new(""));
        if by == StagedBy::Agent && is_secret(rel) {
            return Err(format!("{raw} looks like a secret; it is never published"));
        }
        if self.denied(rel) {
            return Err(format!("{raw} is denied by this node's policy"));
        }
        let md = std::fs::symlink_metadata(&path).map_err(|e| format!("{raw}: {e}"))?;
        if !md.is_dir() && !md.is_file() {
            return Err(format!("{raw} is not a regular file or folder"));
        }
        Ok(Resolved {
            is_dir: md.is_dir(),
            path,
            root,
        })
    }

    /// Copy (or pack) a checked path into a new stage, describe it and
    /// index it. Returns the `artifact_staged` payload for the session log.
    pub(crate) async fn stage(
        self: &Arc<Self>,
        req: Request,
        src: Resolved,
        cancel: CancellationToken,
    ) -> Result<ArtifactStaged, String> {
        let stage_id = harness_core::new_id();
        let dir = self.dir.join(&stage_id);
        let this = self.clone();
        let (s, d, c) = (src.clone(), dir.clone(), cancel);
        let copied = tokio::task::spawn_blocking(move || this.fill(&s, &d, &c))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string()));
        let (size, sha256, mut meta) = match copied {
            Ok(v) => v,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(e);
            }
        };
        self.describe(&src, &mut meta).await;
        let fname = src
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let filename = if src.is_dir {
            format!("{fname}.tar.gz")
        } else {
            fname.clone()
        };
        let staged = ArtifactStaged {
            stage_id: stage_id.clone(),
            call_id: req.call_id,
            by: req.by,
            name: req.name.unwrap_or_else(|| fname.chars().take(80).collect()),
            filename,
            packed: src.is_dir.then(|| "tar.gz".into()),
            size,
            sha256: sha256.clone(),
            kind_hint: req.kind_hint,
            source_path: src.path.display().to_string(),
            notes: req.notes,
            meta: meta.cleaned().fit(),
        };
        let record = Record {
            session_id: req.session,
            size,
            sha256,
            created_ms: now_ms(),
            staged: staged.clone(),
        };
        if let Err(e) = write_record(&dir, &record) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!("could not stage the file: {e}"));
        }
        lock(&self.index).insert(
            (req.session, stage_id),
            Arc::new(Stage {
                dir,
                size,
                created_ms: record.created_ms,
                order: Default::default(),
            }),
        );
        Ok(staged)
    }

    /// The blocking part of `stage`: the copy or the pack.
    fn fill(
        &self,
        src: &Resolved,
        dir: &Path,
        cancel: &CancellationToken,
    ) -> io::Result<(u64, String, ArtifactMeta)> {
        private_dir(dir)?;
        let data = dir.join(DATA);
        if !src.is_dir {
            let shown = src.path.display().to_string();
            let f = open_regular(&src.path, &src.root, &self.data_dir)
                .map_err(|e| io::Error::other(e.message(&shown)))?;
            let (size, sha) = copy_file(f, &data, self.max_bytes(), cancel)?;
            return Ok((size, sha, ArtifactMeta::default()));
        }
        let p = Packer {
            stages: self,
            folder: &src.path,
            root: &src.root,
            cancel,
        }
        .pack(&data)?;
        let meta = ArtifactMeta {
            entries: Some(p.listed),
            entries_total: Some(p.total),
            skipped: (!p.skipped.is_empty()).then_some(p.skipped),
            ..Default::default()
        };
        Ok((p.size, p.sha256, meta))
    }

    /// Display metadata: toolchain and package from the nearest manifest,
    /// the repository state, the target. No build command is ever run.
    async fn describe(&self, src: &Resolved, meta: &mut ArtifactMeta) {
        let dir = if src.is_dir {
            src.path.clone()
        } else {
            src.path.parent().unwrap_or(&src.root).to_path_buf()
        };
        let (d, root, data_dir) = (dir.clone(), src.root.clone(), self.data_dir.clone());
        if let Ok((toolchain, package)) =
            tokio::task::spawn_blocking(move || manifest(&d, &root, &data_dir)).await
        {
            meta.toolchain = toolchain;
            meta.package = package;
        }
        // The repository is looked for up to the workspace root (else the
        // chat's folder), never above.
        let ceiling = self
            .roots
            .iter()
            .filter(|r| dir.starts_with(r))
            .max_by_key(|r| r.components().count())
            .cloned()
            .unwrap_or_else(|| src.root.clone());
        let bounds = crate::git::Bounds {
            ceiling: Some(ceiling),
            protected: vec![self.data_dir.clone()],
            sandbox: self.git_sandbox.clone(),
        };
        meta.git = crate::git::describe(&dir, self.checkpoints.as_deref(), &bounds)
            .await
            .map(|(commit, branch, dirty)| GitInfo {
                commit,
                branch,
                dirty,
            });
        meta.target = Some(target(&src.path));
    }

    fn lookup(&self, session: SessionId, stage_id: &str) -> Option<Arc<Stage>> {
        if !valid_stage_id(stage_id) {
            return None;
        }
        lock(&self.index)
            .get(&(session, stage_id.to_string()))
            .cloned()
    }

    /// Answer one `artifact.fetch` from a task of its own, so the node's
    /// command loop never waits on a disk read. Beyond `FETCHES` at once,
    /// the master gets `busy` and asks again.
    pub(crate) fn fetch(
        self: &Arc<Self>,
        out: Reply,
        req_id: String,
        session: SessionId,
        stage_id: String,
        offset: u64,
        len: u32,
    ) {
        let reply = |data: String, eof: bool, error: Option<String>| NodeToMaster::ArtifactChunk {
            req_id: req_id.clone(),
            stage_id: stage_id.clone(),
            offset,
            data,
            eof,
            error,
        };
        let Ok(permit) = self.fetches.clone().try_acquire_owned() else {
            out(reply(String::new(), false, Some("busy".into())));
            return;
        };
        let Some(stage) = self.lookup(session, &stage_id) else {
            out(reply(String::new(), false, Some("unknown stage".into())));
            return;
        };
        tokio::spawn(async move {
            let _permit = permit;
            let _order = stage.order.lock().await;
            let (path, size) = (stage.dir.join(DATA), stage.size);
            let len = len.min(MAX_CHUNK);
            let read = tokio::task::spawn_blocking(move || read_at(&path, offset, len, size)).await;
            let (data, eof, error) = match read {
                Ok(Ok(bytes)) => {
                    let eof = offset + bytes.len() as u64 >= size;
                    (
                        base64::engine::general_purpose::STANDARD.encode(bytes),
                        eof,
                        None,
                    )
                }
                Ok(Err(e)) => (String::new(), false, Some(e.to_string())),
                Err(e) => (String::new(), false, Some(e.to_string())),
            };
            out(NodeToMaster::ArtifactChunk {
                req_id,
                stage_id,
                offset,
                data,
                eof,
                error,
            });
        });
    }

    /// Delete a stage the master is done with: only the folder its index
    /// entry records.
    pub(crate) fn release(&self, session: SessionId, stage_id: &str) -> Result<(), String> {
        if !valid_stage_id(stage_id) {
            return Err("unknown stage".into());
        }
        let stage = lock(&self.index)
            .remove(&(session, stage_id.to_string()))
            .ok_or("unknown stage")?;
        std::fs::remove_dir_all(&stage.dir).map_err(|e| e.to_string())
    }

    /// Delete stages older than the TTL, then the oldest beyond
    /// `stage_max_mb`.
    pub(crate) fn expire(&self) {
        let ttl = self.policy.stage_ttl_hours.saturating_mul(3_600_000) as i64;
        let cutoff = now_ms().saturating_sub(ttl);
        let mut index = lock(&self.index);
        let mut gone: Vec<(SessionId, String)> = index
            .iter()
            .filter(|(_, s)| s.created_ms < cutoff)
            .map(|(k, _)| k.clone())
            .collect();
        let mut live: Vec<_> = index
            .iter()
            .filter(|(k, _)| !gone.contains(k))
            .map(|(k, s)| (s.created_ms, s.size, k.clone()))
            .collect();
        live.sort();
        let mut total: u64 = live.iter().map(|(_, size, _)| size).sum();
        let max = self.policy.stage_max_mb.saturating_mul(MB);
        for (_, size, key) in live {
            if total <= max {
                break;
            }
            total -= size;
            gone.push(key);
        }
        for key in gone {
            if let Some(s) = index.remove(&key) {
                let _ = std::fs::remove_dir_all(&s.dir);
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        lock(&self.index).len()
    }
}

fn read_record(dir: &Path) -> Option<Record> {
    let mut text = String::new();
    File::open(dir.join(RECORD))
        .ok()?
        .take(RECORD_READ)
        .read_to_string(&mut text)
        .ok()?;
    let r: Record = serde_json::from_str(&text).ok()?;
    let len = std::fs::symlink_metadata(dir.join(DATA)).ok()?.len();
    (len == r.size).then_some(r)
}

fn write_record(dir: &Path, r: &Record) -> io::Result<()> {
    let mut f = private_file(&dir.join(RECORD))?;
    f.write_all(&serde_json::to_vec(r)?)?;
    f.sync_all()
}

/// Up to `len` bytes of a staged file from `offset`.
fn read_at(path: &Path, offset: u64, len: u32, size: u64) -> io::Result<Vec<u8>> {
    if offset > size {
        return Err(io::Error::other("offset past the end of the file"));
    }
    let want = (size - offset).min(u64::from(len)) as usize;
    let mut f = File::open(path)?;
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; want];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

/// Read at most `MANIFEST_READ` bytes of a manifest inside `root`.
fn read_small(path: &Path, root: &Path, data_dir: &Path) -> Option<String> {
    let f = open_regular(path, root, data_dir).ok()?;
    let mut s = String::new();
    f.take(MANIFEST_READ).read_to_string(&mut s).ok()?;
    Some(s)
}

/// Toolchain and package from the nearest manifest between `dir` and
/// `root`.
fn manifest(dir: &Path, root: &Path, data_dir: &Path) -> (Option<String>, Option<Package>) {
    let mut at = Some(dir);
    while let Some(d) = at.filter(|d| d.starts_with(root)) {
        if let Some(found) = manifest_in(d, root, data_dir) {
            return found;
        }
        at = d.parent();
    }
    (None, None)
}

fn toml_str(t: &toml::Table, table: &str, key: &str) -> Option<String> {
    t.get(table)?.get(key)?.as_str().map(str::to_string)
}

fn package(name: Option<String>, version: Option<String>) -> Option<Package> {
    Some(Package {
        name: name?,
        version: version.unwrap_or_default(),
    })
}

fn manifest_in(
    d: &Path,
    root: &Path,
    data_dir: &Path,
) -> Option<(Option<String>, Option<Package>)> {
    let read = |name: &str| read_small(&d.join(name), root, data_dir);
    if let Some(text) = read("Cargo.toml") {
        let t: toml::Table = toml::from_str(&text).unwrap_or_default();
        let channel = read("rust-toolchain.toml")
            .and_then(|s| toml::from_str::<toml::Table>(&s).ok())
            .and_then(|t| toml_str(&t, "toolchain", "channel"));
        let toolchain = match channel {
            Some(c) => format!("rust {c}"),
            None => "rust".into(),
        };
        let pkg = package(
            toml_str(&t, "package", "name"),
            toml_str(&t, "package", "version"),
        );
        return Some((Some(toolchain), pkg));
    }
    if let Some(text) = read("package.json") {
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
        return Some((Some("node".into()), package(s("name"), s("version"))));
    }
    if let Some(text) = read("pyproject.toml") {
        let t: toml::Table = toml::from_str(&text).unwrap_or_default();
        let poetry = t.get("tool").and_then(|x| x.get("poetry"));
        let pick = |k: &str| {
            toml_str(&t, "project", k).or_else(|| poetry?.get(k)?.as_str().map(str::to_string))
        };
        return Some((
            Some("python".into()),
            package(pick("name"), pick("version")),
        ));
    }
    if let Some(text) = read("go.mod") {
        let field = |k: &str| {
            text.lines()
                .find_map(|l| l.trim().strip_prefix(k).map(|v| v.trim().to_string()))
        };
        let toolchain = match field("go ") {
            Some(v) => format!("go {v}"),
            None => "go".into(),
        };
        return Some((Some(toolchain), package(field("module "), None)));
    }
    let present = |name: &str| d.join(name).is_file();
    let toolchain = if present("pom.xml") {
        "java (maven)"
    } else if present("build.gradle.kts") {
        "kotlin (gradle)"
    } else if present("build.gradle") {
        "java (gradle)"
    } else if std::fs::read_dir(d)
        .ok()?
        .flatten()
        .any(|e| e.path().extension().is_some_and(|x| x == "csproj"))
    {
        ".net"
    } else {
        return None;
    };
    Some((Some(toolchain.into()), None))
}

/// The target triple from a `target/<triple>/(release|debug)/` path, else
/// this host's architecture and OS.
fn target(path: &Path) -> String {
    let parts: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts
        .windows(3)
        .find(|w| w[0] == "target" && matches!(w[2].as_str(), "release" | "debug"))
        .map(|w| w[1].clone())
        .unwrap_or_else(|| format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS))
}

/// Where one session's turn publishes: the node's stage. Counts the turn's
/// publishes on the node.
pub(crate) struct Sink {
    pub node: Arc<Inner>,
    pub stages: Arc<Stages>,
    pub session: SessionId,
    pub cwd: PathBuf,
    /// The session's folder as pinned (R.1, T4): checked before every
    /// publish, so a replaced folder publishes nothing.
    pub pin: Option<harness_core::folder::FolderPin>,
    pub count: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl ArtifactSink for Sink {
    async fn publish(&self, req: PublishRequest) -> Result<Published, String> {
        if self.count.fetch_add(1, Ordering::SeqCst) >= MAX_PER_TURN {
            return Err(format!(
                "at most {MAX_PER_TURN} files can be published in one turn"
            ));
        }
        let cwd = match &self.pin {
            Some(pin) => pin.check()?,
            None => {
                return Err(format!(
                    "{}: {}",
                    harness_core::folder::REPLACED,
                    self.cwd.display()
                ));
            }
        };
        self.node.cfg.policy.check_session(&cwd)?;
        let src = self.stages.check(&cwd, &req.path, StagedBy::Agent)?;
        let staged = self
            .stages
            .stage(
                Request {
                    session: self.session,
                    by: StagedBy::Agent,
                    name: req.name,
                    notes: req.notes,
                    kind_hint: req.kind_hint,
                    call_id: Some(req.call_id),
                },
                src,
                req.cancel,
            )
            .await?;
        let published = Published {
            name: staged.name.clone(),
            size: staged.size,
            sha256: staged.sha256.clone(),
            packed: staged.packed.is_some(),
        };
        let stage_id = staged.stage_id.clone();
        let recorded = match self.node.session(self.session) {
            // The turn's own log: a failure ends the turn.
            Some(s) => s
                .emit(EventKind::ArtifactStaged(Box::new(staged)))
                .await
                .map_err(|e| e.to_string()),
            None => Err("the chat is gone".into()),
        };
        if let Err(e) = recorded {
            // No event names the copy: drop it.
            let _ = self.stages.release(self.session, &stage_id);
            return Err(e);
        }
        Ok(published)
    }
}

/// How often stages are checked for expiry.
pub(crate) const EXPIRE_EVERY: Duration = Duration::from_secs(3600);

#[cfg(test)]
mod tests {
    use super::*;

    fn stages(root: &Path, data: &Path, edit: impl FnOnce(&mut NodePolicy)) -> Arc<Stages> {
        let mut p = NodePolicy {
            workspace_roots: vec![root.into()],
            ..Default::default()
        };
        edit(&mut p);
        Arc::new(Stages::open(&StageConfig::new(data.into()), &p, &HashSet::new()).unwrap())
    }

    struct Dirs {
        _t: tempfile::TempDir,
        ws: PathBuf,
        data: PathBuf,
        outside: PathBuf,
    }

    fn dirs() -> Dirs {
        let t = tempfile::tempdir().unwrap();
        let (ws, data, outside) = (
            t.path().join("ws"),
            t.path().join("data"),
            t.path().join("out"),
        );
        for d in [&ws, &data, &outside] {
            std::fs::create_dir_all(d).unwrap();
        }
        Dirs {
            ws: ws.canonicalize().unwrap(),
            data: data.canonicalize().unwrap(),
            outside: outside.canonicalize().unwrap(),
            _t: t,
        }
    }

    #[test]
    fn globs() {
        assert!(glob(b"*.pem", b"server.pem"));
        assert!(!glob(b"*.pem", b"a/server.pem"));
        assert!(glob(b"**/*.pem", b"a/b/server.pem"));
        assert!(glob(b"**/*.pem", b"server.pem"));
        assert!(glob(b".env*", b".env.local"));
        assert!(glob(b"id_rsa?", b"id_rsa2"));
        assert!(!glob(b"id_rsa?", b"id_rsa"));
        assert!(is_secret(Path::new("a/.ssh/config")));
        assert!(is_secret(Path::new("credentials.json")));
        assert!(!is_secret(Path::new("src/main.rs")));
    }

    #[test]
    fn confinement() {
        let d = dirs();
        let st = stages(&d.ws, &d.data, |_| {});
        std::fs::write(d.ws.join("app"), b"bin").unwrap();
        std::fs::write(d.outside.join("secret"), b"s").unwrap();
        std::fs::write(d.ws.join(".env"), b"K=V").unwrap();
        std::fs::create_dir(d.ws.join("sub")).unwrap();
        let agent = StagedBy::Agent;
        // Relative and absolute inside, `..` that stays inside.
        assert!(st.check(&d.ws, "app", agent).is_ok());
        assert!(
            st.check(&d.ws, d.ws.join("app").to_str().unwrap(), agent)
                .is_ok()
        );
        assert!(st.check(&d.ws, "sub/../app", agent).is_ok());
        // `..` out, absolute out, a symlink out.
        assert!(st.check(&d.ws, "../out/secret", agent).is_err());
        assert!(
            st.check(&d.ws, d.outside.join("secret").to_str().unwrap(), agent)
                .is_err()
        );
        std::os::unix::fs::symlink(d.outside.join("secret"), d.ws.join("link")).unwrap();
        assert!(st.check(&d.ws, "link", agent).is_err());
        std::os::unix::fs::symlink("/dev/null", d.ws.join("null")).unwrap();
        assert!(st.check(&d.ws, "null", agent).is_err(), "a device outside");
        // Secret names: refused for the agent, allowed for the user.
        assert!(
            st.check(&d.ws, ".env", agent)
                .unwrap_err()
                .contains("secret")
        );
        assert!(st.check(&d.ws, ".env", StagedBy::User).is_ok());
        // A FIFO is not a regular file.
        nix::unistd::mkfifo(&d.ws.join("fifo"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        assert!(st.check(&d.ws, "fifo", agent).is_err());
        assert!(matches!(
            open_regular(&d.ws.join("fifo"), &d.ws, &d.data),
            Err(OpenError::Special)
        ));
        assert!(st.check(&d.ws, "", agent).is_err());
        assert!(st.check(&d.ws, "missing", agent).is_err());
    }

    #[test]
    fn the_data_dir_is_never_published() {
        let d = dirs();
        // The data dir inside the chat's folder (the embedded node in ~).
        let data = d.ws.join("data");
        std::fs::create_dir(&data).unwrap();
        std::fs::write(data.join("master.db"), b"db").unwrap();
        let st = stages(&d.ws, &data, |_| {});
        for by in [StagedBy::Agent, StagedBy::User] {
            let e = st.check(&d.ws, "data/master.db", by).unwrap_err();
            assert!(e.contains("data folder"), "{e}");
            assert!(st.check(&d.ws, "data", by).is_err());
        }
    }

    #[test]
    fn deny_globs_apply_to_both_paths() {
        let d = dirs();
        let st = stages(&d.ws, &d.data, |p| {
            p.artifacts.deny_globs = vec!["*.sqlite".into()]
        });
        std::fs::write(d.ws.join("x.sqlite"), b"db").unwrap();
        assert!(st.check(&d.ws, "x.sqlite", StagedBy::Agent).is_err());
        assert!(st.check(&d.ws, "x.sqlite", StagedBy::User).is_err());
    }

    #[test]
    fn workspace_roots_are_a_floor_outside_the_folder() {
        let d = dirs();
        std::fs::write(d.outside.join("f"), b"x").unwrap();
        let st = stages(&d.ws, &d.data, |p| {
            p.allow_outside_workspace = true;
            p.workspace_roots = vec![d.ws.clone(), d.outside.clone()];
        });
        assert!(
            st.check(
                &d.ws,
                d.outside.join("f").to_str().unwrap(),
                StagedBy::Agent
            )
            .is_ok()
        );
        let tight = stages(&d.ws, &d.data, |p| p.allow_outside_workspace = true);
        assert!(
            tight
                .check(
                    &d.ws,
                    d.outside.join("f").to_str().unwrap(),
                    StagedBy::Agent
                )
                .is_err()
        );
    }

    /// A folder swapped for a symlink after the path was checked: the open
    /// descriptor points outside, so nothing is read.
    #[test]
    fn swap_after_resolve_is_caught_on_the_descriptor() {
        let d = dirs();
        std::fs::create_dir(d.ws.join("dir")).unwrap();
        std::fs::write(d.ws.join("dir/f"), b"inside").unwrap();
        std::fs::write(d.outside.join("f"), b"outside").unwrap();
        let st = stages(&d.ws, &d.data, |_| {});
        let r = st.check(&d.ws, "dir/f", StagedBy::Agent).unwrap();
        std::fs::remove_dir_all(d.ws.join("dir")).unwrap();
        std::os::unix::fs::symlink(&d.outside, d.ws.join("dir")).unwrap();
        assert!(matches!(
            open_regular(&r.path, &r.root, &d.data),
            Err(OpenError::Outside)
        ));
    }

    #[tokio::test]
    async fn stages_files_and_packs_folders() {
        let d = dirs();
        let st = stages(&d.ws, &d.data, |_| {});
        let sid = SessionId::new();
        let req = |by| Request {
            session: sid,
            by,
            name: None,
            notes: None,
            kind_hint: None,
            call_id: None,
        };
        std::fs::write(d.ws.join("app"), b"hello").unwrap();
        let r = st.check(&d.ws, "app", StagedBy::Agent).unwrap();
        let s = st
            .stage(req(StagedBy::Agent), r, Default::default())
            .await
            .unwrap();
        assert_eq!(
            (s.size, s.name.as_str(), s.packed.as_deref()),
            (5, "app", None)
        );
        assert_eq!(s.sha256, hex::encode(Sha256::digest(b"hello")));
        let rec = read_record(&st.dir.join(&s.stage_id)).unwrap();
        assert_eq!(rec.session_id, sid);

        let f = d.ws.join("proj");
        std::fs::create_dir_all(f.join(".git")).unwrap();
        std::fs::create_dir_all(f.join("src")).unwrap();
        std::fs::write(f.join("src/main.rs"), b"fn main() {}").unwrap();
        std::fs::write(f.join(".git/config"), b"x").unwrap();
        std::fs::write(f.join("server.pem"), b"k").unwrap();
        std::fs::write(
            f.join("Cargo.toml"),
            "[package]\nname = \"proj\"\nversion = \"0.3.1\"\n",
        )
        .unwrap();
        std::os::unix::fs::symlink("/etc/passwd", f.join("pw")).unwrap();
        nix::unistd::mkfifo(&f.join("pipe"), nix::sys::stat::Mode::S_IRWXU).unwrap();
        let r = st.check(&d.ws, "proj", StagedBy::User).unwrap();
        let s = st
            .stage(req(StagedBy::User), r, Default::default())
            .await
            .unwrap();
        assert_eq!(s.filename, "proj.tar.gz");
        assert_eq!(s.packed.as_deref(), Some("tar.gz"));
        let skipped = s.meta.skipped.clone().unwrap();
        for k in ["git", "secret", "symlink", "special"] {
            assert_eq!(skipped.get(k), Some(&1), "{k}: {skipped:?}");
        }
        assert_eq!(s.meta.toolchain.as_deref(), Some("rust"));
        assert_eq!(s.meta.package.as_ref().unwrap().version, "0.3.1");
        let paths: Vec<_> = s
            .meta
            .entries
            .unwrap()
            .into_iter()
            .map(|e| e.path)
            .collect();
        assert_eq!(paths, ["proj/Cargo.toml", "proj/src/main.rs"]);
        // The archive reads back: folders, the two files, uid 0.
        let data = std::fs::read(st.dir.join(&s.stage_id).join(DATA)).unwrap();
        assert_eq!(hex::encode(Sha256::digest(&data)), s.sha256);
        let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(&data[..]));
        let names: Vec<(String, u64)> = ar
            .entries()
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    e.path().unwrap().display().to_string(),
                    e.header().uid().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            names.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["proj/", "proj/Cargo.toml", "proj/src/", "proj/src/main.rs"]
        );
        assert!(names.iter().all(|(_, uid)| *uid == 0));
    }

    #[tokio::test]
    async fn size_caps_and_cancel() {
        let d = dirs();
        let st = stages(&d.ws, &d.data, |p| p.artifacts.max_mb = 1);
        std::fs::write(d.ws.join("big"), vec![7u8; (MB + 1) as usize]).unwrap();
        let req = || Request {
            session: SessionId::new(),
            by: StagedBy::Agent,
            name: None,
            notes: None,
            kind_hint: None,
            call_id: None,
        };
        let r = st.check(&d.ws, "big", StagedBy::Agent).unwrap();
        let e = st.stage(req(), r, Default::default()).await.unwrap_err();
        assert!(e.contains("1 MB"), "{e}");
        std::fs::create_dir(d.ws.join("dir")).unwrap();
        std::fs::write(d.ws.join("dir/big"), vec![7u8; (MB + 1) as usize]).unwrap();
        let r = st.check(&d.ws, "dir", StagedBy::Agent).unwrap();
        assert!(st.stage(req(), r, Default::default()).await.is_err());
        std::fs::write(d.ws.join("small"), b"x").unwrap();
        let r = st.check(&d.ws, "small", StagedBy::Agent).unwrap();
        let c = CancellationToken::new();
        c.cancel();
        assert!(st.stage(req(), r, c).await.is_err());
        // Nothing failed was kept.
        assert_eq!(st.len(), 0);
        assert_eq!(std::fs::read_dir(&st.dir).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn the_index_is_rebuilt_released_and_expired() {
        let d = dirs();
        let st = stages(&d.ws, &d.data, |_| {});
        let (mine, gone) = (SessionId::new(), SessionId::new());
        std::fs::write(d.ws.join("a"), b"a").unwrap();
        let mut ids = vec![];
        for sid in [mine, gone] {
            let r = st.check(&d.ws, "a", StagedBy::Agent).unwrap();
            let req = Request {
                session: sid,
                by: StagedBy::Agent,
                name: None,
                notes: None,
                kind_hint: None,
                call_id: None,
            };
            ids.push(st.stage(req, r, Default::default()).await.unwrap().stage_id);
        }
        // A planted folder with no record, and one named like a path.
        std::fs::create_dir(st.dir.join("junk")).unwrap();
        let policy = NodePolicy {
            workspace_roots: vec![d.ws.clone()],
            ..Default::default()
        };
        let cfg = StageConfig::new(d.data.clone());
        let hosted: HashSet<SessionId> = [mine].into();
        let st = Arc::new(Stages::open(&cfg, &policy, &hosted).unwrap());
        assert_eq!(st.len(), 1, "only the hosted session's stage stays");
        assert!(!st.dir.join(&ids[1]).exists() && !st.dir.join("junk").exists());
        // Ids are keys: `../` and unknown ids name nothing, and another
        // session's id does not reach this one's stage.
        for bad in ["../data", "..", &ids[1], &harness_core::new_id()] {
            assert!(st.release(mine, bad).is_err(), "{bad}");
        }
        assert!(st.release(gone, &ids[0]).is_err());
        assert!(st.lookup(mine, &ids[0]).is_some());
        st.release(mine, &ids[0]).unwrap();
        assert!(!st.dir.join(&ids[0]).exists());
        assert!(d.ws.join("a").exists(), "the source is untouched");

        // Expiry: a zero TTL removes everything at the next check.
        let r = st.check(&d.ws, "a", StagedBy::Agent).unwrap();
        let req = Request {
            session: mine,
            by: StagedBy::Agent,
            name: None,
            notes: None,
            kind_hint: None,
            call_id: None,
        };
        st.stage(req, r, Default::default()).await.unwrap();
        let mut zero = policy.clone();
        zero.artifacts.stage_ttl_hours = 0;
        std::thread::sleep(Duration::from_millis(5));
        let st = Stages::open(&cfg, &zero, &hosted).unwrap();
        assert_eq!(st.len(), 0);
    }

    /// Beyond eight reads at once the master hears `busy`; ids that are
    /// not in the index (or another session's) get an error chunk.
    #[tokio::test]
    async fn fetches_are_bounded_and_looked_up() {
        let d = dirs();
        let st = stages(&d.ws, &d.data, |_| {});
        let sid = SessionId::new();
        std::fs::write(d.ws.join("f"), vec![1u8; 300_000]).unwrap();
        let r = st.check(&d.ws, "f", StagedBy::User).unwrap();
        let req = Request {
            session: sid,
            by: StagedBy::User,
            name: None,
            notes: None,
            kind_hint: None,
            call_id: None,
        };
        let id = st.stage(req, r, Default::default()).await.unwrap().stage_id;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let out: Reply = Arc::new(move |m| {
            let _ = tx.send(m);
        });
        async fn chunk(
            rx: &mut tokio::sync::mpsc::UnboundedReceiver<NodeToMaster>,
        ) -> (String, bool, Option<String>, u64) {
            match rx.recv().await.unwrap() {
                NodeToMaster::ArtifactChunk {
                    data,
                    eof,
                    error,
                    offset,
                    ..
                } => (data, eof, error, offset),
                other => panic!("{other:?}"),
            }
        }
        st.fetch(out.clone(), "r1".into(), sid, id.clone(), 0, u32::MAX);
        let (data, eof, error, _) = chunk(&mut rx).await;
        assert_eq!(error, None);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap();
        assert_eq!(bytes.len(), MAX_CHUNK as usize, "clamped to one chunk");
        assert!(!eof);
        st.fetch(
            out.clone(),
            "r2".into(),
            sid,
            id.clone(),
            u64::from(MAX_CHUNK),
            MAX_CHUNK,
        );
        let (data, eof, _, offset) = chunk(&mut rx).await;
        assert_eq!(offset, u64::from(MAX_CHUNK));
        assert!(eof);
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap()
                .len(),
            300_000 - MAX_CHUNK as usize
        );
        for bad in ["../f", &harness_core::new_id()] {
            st.fetch(out.clone(), "r3".into(), sid, bad.to_string(), 0, 10);
            assert_eq!(chunk(&mut rx).await.2.as_deref(), Some("unknown stage"));
        }
        st.fetch(
            out.clone(),
            "r4".into(),
            SessionId::new(),
            id.clone(),
            0,
            10,
        );
        assert_eq!(chunk(&mut rx).await.2.as_deref(), Some("unknown stage"));
        let held: Vec<_> = (0..FETCHES)
            .map(|_| st.fetches.clone().try_acquire_owned().unwrap())
            .collect();
        st.fetch(out.clone(), "r5".into(), sid, id.clone(), 0, 10);
        assert_eq!(chunk(&mut rx).await.2.as_deref(), Some("busy"));
        drop(held);
        st.fetch(out, "r6".into(), sid, id, 0, 10);
        assert_eq!(chunk(&mut rx).await.2, None);
    }

    #[test]
    fn targets() {
        assert_eq!(
            target(Path::new("/w/target/aarch64-apple-darwin/release/app")),
            "aarch64-apple-darwin"
        );
        let host = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);
        assert_eq!(target(Path::new("/w/target/release/app")), host);
        assert_eq!(target(Path::new("/w/dist/app")), host);
    }
}
