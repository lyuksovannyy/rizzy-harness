//! Files the person attached that the model cannot read as text (PLAN.md
//! T94, docs/plan/X.53.md): staged on the node, then saved into the chat's
//! folder under `attachments/`.
//!
//! The master pushes a file with `session.upload` chunks, one acked chunk
//! at a time, into a staging folder under the node's data directory (a
//! protected folder: no session's agent can write there, so what the node
//! checked is what it saves). The prompt that names the file takes it:
//! the node checks its size and sha256 again, and writes it into the
//! folder with nothing resolved a second time after the folder was
//! checked and opened:
//!
//! - the chat's folder is opened without following a link at its end;
//! - `attachments` is made with `mkdirat` or opened with
//!   `O_DIRECTORY | O_NOFOLLOW`; a link or a file in its place is refused
//!   with a sentence, never followed, never replaced;
//! - the bytes go to a temporary name made with `O_CREAT | O_EXCL |
//!   O_NOFOLLOW`, are hashed as they are copied, synced, and only then get
//!   their name with `renameat2(RENAME_NOREPLACE)` (`linkat` where the
//!   filesystem lacks it): anything already there, a link included, is
//!   never replaced; the name gets ` (2)`, ` (3)` instead.
//!
//! The name is the person's file name reduced to one safe component
//! ([`safe_name`]); the master's word for it is never a path.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use harness_core::SessionId;
use harness_core::event::UploadedFile;
use harness_protocol::{MAX_CHUNK, UploadRef};
use nix::errno::Errno;
use nix::fcntl::{AtFlags, OFlag};
use nix::sys::stat::{Mode, SFlag};
use sha2::{Digest, Sha256};

/// The folder files are saved in, inside the chat's folder.
pub(crate) const DIR: &str = "attachments";

/// Staged uploads this node holds at once, all chats together: with the
/// file limit this bounds the staging folder (16 x 50 MiB by default).
const MAX_STAGED: usize = 16;
/// A staged upload nobody took within this long is deleted.
const STAGE_TTL: Duration = Duration::from_secs(3600);
/// Names the saved file may take before the node gives up: `x (2)` to
/// `x (999)`.
const MAX_SUFFIX: u32 = 999;
/// The longest saved name, before a ` (n)` suffix.
const MAX_NAME_BYTES: usize = 120;

/// The refusal for a chat without tools.
pub(crate) const NO_TOOLS: &str = "this chat's agent cannot open files; start a coding chat, or \
     attach text, a PDF or an image";

// ---------------------------------------------------------------- names

/// Characters that read as a path separator or that Windows refuses in a
/// name: each becomes `_`. The look-alikes of `/` and `\` are in it so the
/// name is the same one component on every system it is synced to.
fn is_unsafe(c: char) -> bool {
    matches!(
        c,
        '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*'
            | '\u{2044}' // fraction slash
            | '\u{2215}' // division slash
            | '\u{2216}' // set minus
            | '\u{29F5}' // reverse solidus operator
            | '\u{29F8}' // big solidus
            | '\u{29F9}' // big reverse solidus
            | '\u{FE68}' // small reverse solidus
            | '\u{FF0F}' // fullwidth solidus
            | '\u{FF3C}' // fullwidth reverse solidus
    )
}

/// The person's file name as one safe file name: its last path segment
/// (`/` or `\`), hidden characters and controls out
/// ([`harness_core::text::clean_line`]), look-alikes of a separator `_`,
/// no leading dot, dash or space (no hidden file, no option for a
/// command), no trailing dot or space, at most 120 bytes with the
/// extension kept, and `file` for what is left empty.
pub(crate) fn safe_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or("");
    let line = harness_core::text::clean_line(last, last.chars().count());
    let swapped: String = line
        .chars()
        .map(|c| if is_unsafe(c) { '_' } else { c })
        .collect();
    let trimmed = swapped
        .trim_start_matches(['.', '-', ' '])
        .trim_end_matches(['.', ' ']);
    let capped = cap_bytes(trimmed);
    if capped.is_empty() {
        "file".into()
    } else {
        capped
    }
}

/// `name` cut to [`MAX_NAME_BYTES`] on a character boundary, keeping a
/// short extension.
fn cap_bytes(name: &str) -> String {
    if name.len() <= MAX_NAME_BYTES {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && name.len() - i <= 17 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let room = MAX_NAME_BYTES - ext.len();
    let mut cut = room.min(stem.len());
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{ext}", stem[..cut].trim_end_matches(['.', ' ']))
}

/// `name` for the `n`th try: `x.zip`, `x (2).zip`, `x (3).zip`.
fn numbered(name: &str, n: u32) -> String {
    if n <= 1 {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({n}){}", &name[..i], &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

/// The type the browser said, kept only when it is a plain media type
/// (informational: nothing decides on it).
pub(crate) fn clean_mime(mime: &str) -> String {
    let ok = !mime.is_empty()
        && mime.len() <= 80
        && mime
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$&^_.+-/".contains(&b));
    if ok {
        mime.to_ascii_lowercase()
    } else {
        "application/octet-stream".into()
    }
}

// --------------------------------------------------------------- saving

fn dir_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC
}

fn umask() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Umask:"))
                .and_then(|v| u32::from_str_radix(v.trim(), 8).ok())
        })
        .unwrap_or(0o022)
}

/// What a saved file is, for [`rollback`].
#[derive(Debug, Clone)]
pub(crate) struct Saved {
    pub file: UploadedFile,
    dev: u64,
    ino: u64,
}

/// Copy `from` to `to`, hashing what passes: the byte count and the hex
/// sha256. Stops with an error at more than `max` bytes. A seam for the
/// full-disk test.
fn copy_hashing(
    from: &mut impl Read,
    to: &mut impl Write,
    max: u64,
) -> std::io::Result<(u64, String)> {
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut n = 0u64;
    loop {
        let got = from.read(&mut buf)?;
        if got == 0 {
            break;
        }
        n += got as u64;
        if n > max {
            return Err(std::io::Error::other("the file is larger than it said"));
        }
        hash.update(&buf[..got]);
        to.write_all(&buf[..got])?;
    }
    Ok((n, hex::encode(hash.finalize())))
}

/// Open `attachments` in `top`, making it when it is missing. A link or a
/// file in its place is refused with the cause.
fn open_dir(top: &OwnedFd) -> Result<OwnedFd, String> {
    let mine = |e: Errno| {
        format!(
            "cannot open {DIR}/ in the chat's folder: {}",
            std::io::Error::from(e)
        )
    };
    for _ in 0..2 {
        match nix::fcntl::openat(top, DIR, dir_flags(), Mode::empty()) {
            Ok(fd) => return Ok(fd),
            Err(Errno::ENOENT) => {
                let mode = Mode::from_bits_truncate(0o777 & !umask());
                match nix::sys::stat::mkdirat(top, DIR, mode) {
                    Ok(()) | Err(Errno::EEXIST) => {}
                    Err(e) => {
                        return Err(format!(
                            "cannot make {DIR}/ in the chat's folder: {}",
                            std::io::Error::from(e)
                        ));
                    }
                }
            }
            Err(Errno::ENOTDIR | Errno::ELOOP) => {
                let what = match nix::sys::stat::fstatat(top, DIR, AtFlags::AT_SYMLINK_NOFOLLOW) {
                    Ok(st)
                        if SFlag::from_bits_truncate(st.st_mode) & SFlag::S_IFMT
                            == SFlag::S_IFLNK =>
                    {
                        "a link"
                    }
                    _ => "a file",
                };
                return Err(format!(
                    "{DIR} in the chat's folder is {what}, not a folder; it was not followed or \
                     replaced and nothing was saved: move it away to attach files"
                ));
            }
            Err(e) => return Err(mine(e)),
        }
    }
    Err(format!("cannot open {DIR}/ in the chat's folder"))
}

/// Save the staged file `staged` (hex sha256 `sha256`, `size` bytes, the
/// person's file `name`) as `attachments/<safe name>` in `folder`, which
/// the caller has just checked against the session's pin. The saved name
/// is returned in the record; a name already taken (by anything) is never
/// replaced.
pub(crate) fn save_file(
    folder: &Path,
    staged: &Path,
    name: &str,
    mime: &str,
    size: u64,
    sha256: &str,
) -> Result<Saved, String> {
    save_with(folder, staged, name, mime, size, sha256, |t, f| {
        copy_hashing(f, t, size)
    })
}

fn save_with(
    folder: &Path,
    staged: &Path,
    name: &str,
    mime: &str,
    size: u64,
    sha256: &str,
    copy: impl FnOnce(&mut std::fs::File, &mut std::fs::File) -> std::io::Result<(u64, String)>,
) -> Result<Saved, String> {
    let shown = harness_core::event::shown_name(name);
    let top = nix::fcntl::open(folder, dir_flags(), Mode::empty()).map_err(|e| {
        format!(
            "cannot open the chat's folder to save {shown}: {}",
            std::io::Error::from(e)
        )
    })?;
    let dir = open_dir(&top)?;
    let mut id = [0u8; 16];
    getrandom::fill(&mut id).map_err(|e| format!("no randomness for a temporary name: {e}"))?;
    let tmp = format!(".rizzy-upload.{}.tmp", hex::encode(id));
    let mode = Mode::from_bits_truncate(0o600);
    let fd = nix::fcntl::openat(
        &dir,
        tmp.as_str(),
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        mode,
    )
    .map_err(|e| format!("cannot save {shown}: {}", std::io::Error::from(e)))?;
    let cleanup = |dir: &OwnedFd| {
        let _ = nix::unistd::unlinkat(dir, tmp.as_str(), nix::unistd::UnlinkatFlags::NoRemoveDir);
    };
    let mut out = std::fs::File::from(fd);
    let mut src = match std::fs::File::open(staged) {
        Ok(f) => f,
        Err(e) => {
            cleanup(&dir);
            return Err(format!("{shown}: the staged file cannot be read: {e}"));
        }
    };
    match copy(&mut out, &mut src) {
        Ok((n, hash)) if n == size && hash == sha256 => {}
        Ok(_) => {
            cleanup(&dir);
            return Err(format!(
                "{shown}: the file does not match its size and sha256; nothing was saved"
            ));
        }
        Err(e) => {
            cleanup(&dir);
            return Err(format!(
                "cannot save {shown} ({e}); nothing was saved. Is the disk full?"
            ));
        }
    }
    let finish = (|| -> std::io::Result<()> {
        out.flush()?;
        // Ordinary permissions, as `Write` gives a new file; never wider.
        let _ = nix::sys::stat::fchmod(&out, Mode::from_bits_truncate(0o666 & !umask()));
        out.sync_all()
    })();
    if let Err(e) = finish {
        cleanup(&dir);
        return Err(format!(
            "cannot save {shown} ({e}); nothing was saved. Is the disk full?"
        ));
    }
    let st = nix::sys::stat::fstat(&out).map_err(|e| {
        cleanup(&dir);
        format!("cannot save {shown}: {}", std::io::Error::from(e))
    })?;
    drop(out);
    let base = safe_name(name);
    let mut placed = None;
    for n in 1..=MAX_SUFFIX {
        let cand = numbered(&base, n);
        match place(&dir, &tmp, &cand) {
            Ok(()) => {
                placed = Some(cand);
                break;
            }
            // Taken, by anything: a file, a folder, a link. Never replaced.
            Err(Errno::EEXIST) => continue,
            Err(e) => {
                cleanup(&dir);
                return Err(format!("cannot save {shown}: {}", std::io::Error::from(e)));
            }
        }
    }
    let Some(final_name) = placed else {
        cleanup(&dir);
        return Err(format!(
            "{DIR}/ already holds {MAX_SUFFIX} files named like {shown}; nothing was saved"
        ));
    };
    // Where the filesystem has no RENAME_NOREPLACE the temporary name was
    // linked, not moved.
    let _ = nix::unistd::unlinkat(&dir, tmp.as_str(), nix::unistd::UnlinkatFlags::NoRemoveDir);
    let _ = nix::unistd::fsync(&dir);
    Ok(Saved {
        file: UploadedFile {
            name: base,
            path: format!("{DIR}/{final_name}"),
            bytes: size,
            sha256: sha256.to_string(),
            mime: clean_mime(mime),
        },
        dev: st.st_dev,
        ino: st.st_ino,
    })
}

/// Give `tmp` the name `to` in `dir` unless something holds it:
/// `renameat2(RENAME_NOREPLACE)`, or `linkat` (which also never replaces)
/// where the filesystem lacks it.
fn place(dir: &OwnedFd, tmp: &str, to: &str) -> Result<(), Errno> {
    match nix::fcntl::renameat2(dir, tmp, dir, to, nix::fcntl::RenameFlags::RENAME_NOREPLACE) {
        Err(Errno::EINVAL | Errno::ENOSYS | Errno::EOPNOTSUPP) => {
            nix::unistd::linkat(dir, tmp, dir, to, AtFlags::empty())
        }
        r => r,
    }
}

/// Take back a file [`save_file`] wrote, when the message it came with
/// was not recorded: only the very file saved (same device and inode,
/// still a regular file) is removed, found again without following a
/// link. Best effort; a person's file is never deleted by name alone.
pub(crate) fn rollback(folder: &Path, saved: &Saved) {
    let Ok(top) = nix::fcntl::open(folder, dir_flags(), Mode::empty()) else {
        return;
    };
    let Ok(dir) = nix::fcntl::openat(&top, DIR, dir_flags(), Mode::empty()) else {
        return;
    };
    let name: &OsStr = match saved.file.path.strip_prefix("attachments/") {
        Some(n) => OsStr::new(n),
        None => return,
    };
    if let Ok(st) = nix::sys::stat::fstatat(&dir, name, AtFlags::AT_SYMLINK_NOFOLLOW)
        && SFlag::from_bits_truncate(st.st_mode) & SFlag::S_IFMT == SFlag::S_IFREG
        && st.st_dev == saved.dev
        && st.st_ino == saved.ino
    {
        let _ = nix::unistd::unlinkat(&dir, name, nix::unistd::UnlinkatFlags::NoRemoveDir);
    }
}

// -------------------------------------------------------------- staging

/// What the node has of one pushed file.
struct Entry {
    path: PathBuf,
    size: u64,
    sha256: String,
    received: u64,
    hash: Sha256,
    complete: bool,
    touched: Instant,
}

/// The staging folder and what it holds, per chat and upload id.
pub(crate) struct Staging {
    dir: Option<PathBuf>,
    table: Mutex<HashMap<(SessionId, String), Entry>>,
}

fn private_dir(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(p)?;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_sha(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl Staging {
    /// `dir`: where to stage, or `None` where this node saves no uploads.
    /// Whatever an earlier run left there is deleted: a half-pushed file
    /// is pushed again from the start.
    pub(crate) fn new(dir: Option<PathBuf>) -> Self {
        if let Some(d) = &dir {
            let _ = std::fs::remove_dir_all(d);
            if let Err(e) = private_dir(d) {
                tracing::warn!(dir = %d.display(), error = %e, "the upload staging folder could not be made; uploads stay off");
                return Self {
                    dir: None,
                    table: Mutex::default(),
                };
            }
        }
        Self {
            dir,
            table: Mutex::default(),
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.dir.is_some()
    }

    fn table(&self) -> std::sync::MutexGuard<'_, HashMap<(SessionId, String), Entry>> {
        self.table.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Delete the staged files nobody took within [`STAGE_TTL`].
    fn sweep(&self) {
        let mut t = self.table();
        t.retain(|_, e| {
            let keep = e.touched.elapsed() < STAGE_TTL;
            if !keep {
                let _ = std::fs::remove_file(&e.path);
            }
            keep
        });
    }

    /// One chunk of a pushed file. Blocking (file I/O): call it from a
    /// blocking task. `max` is this node's file limit in bytes. Checks
    /// everything the master sent: the ids, the declared size against the
    /// limit, the offset against what was received, the chunk against
    /// [`MAX_CHUNK`] and the declared size, and at the last chunk the
    /// sha256. A bad chunk deletes what was staged of that file.
    // The fields of one `session.upload` and the limit, as the command names them.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn chunk(
        &self,
        sid: SessionId,
        upload_id: &str,
        offset: u64,
        size: u64,
        sha256: &str,
        data: &[u8],
        max: u64,
    ) -> Result<(), String> {
        let Some(root) = &self.dir else {
            return Err("this node saves no uploaded files".into());
        };
        if !valid_id(upload_id) || !valid_sha(sha256) {
            return Err("a malformed upload".into());
        }
        if size == 0 || size > max {
            return Err(format!(
                "this node saves files up to {} MiB",
                max.div_ceil(1024 * 1024)
            ));
        }
        if data.is_empty() || data.len() as u64 > u64::from(MAX_CHUNK) {
            return Err("a chunk of an upload is between 1 byte and 256 KiB".into());
        }
        let key = (sid, upload_id.to_string());
        let mut entry = if offset == 0 {
            // A start, or a start again: what was staged of it goes.
            if let Some(old) = self.table().remove(&key) {
                let _ = std::fs::remove_file(&old.path);
            }
            self.sweep();
            if self.table().len() >= MAX_STAGED {
                return Err(
                    "this node already holds as many uploads as it takes; send them first".into(),
                );
            }
            let dir = root.join(sid.to_string());
            private_dir(&dir).map_err(|e| format!("cannot stage the upload: {e}"))?;
            let path = dir.join(format!("{upload_id}.part"));
            let _ = std::fs::remove_file(&path);
            Entry {
                path,
                size,
                sha256: sha256.to_string(),
                received: 0,
                hash: Sha256::new(),
                complete: false,
                touched: Instant::now(),
            }
        } else {
            let e = self
                .table()
                .remove(&key)
                .ok_or("this upload did not start here: send it again")?;
            if e.size != size || e.sha256 != sha256 || e.received != offset || e.complete {
                let _ = std::fs::remove_file(&e.path);
                return Err("an upload chunk out of order: send the file again".into());
            }
            e
        };
        if entry.received + data.len() as u64 > entry.size {
            let _ = std::fs::remove_file(&entry.path);
            return Err("an upload is longer than it said".into());
        }
        let wrote = (|| -> std::io::Result<()> {
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .mode(0o600)
                .custom_flags((OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC).bits())
                .open(&entry.path)?;
            f.write_all(data)?;
            f.flush()
        })();
        if let Err(e) = wrote {
            let _ = std::fs::remove_file(&entry.path);
            return Err(format!("cannot stage the upload ({e}): is the disk full?"));
        }
        entry.hash.update(data);
        entry.received += data.len() as u64;
        entry.touched = Instant::now();
        if entry.received == entry.size {
            let got = hex::encode(entry.hash.clone().finalize());
            if got != entry.sha256 {
                let _ = std::fs::remove_file(&entry.path);
                return Err(
                    "the file does not match its sha256: it was damaged on the way; send it again"
                        .into(),
                );
            }
            entry.complete = true;
        }
        self.table().insert(key, entry);
        Ok(())
    }

    /// The staged file a prompt names, when it arrived whole with the
    /// size and sha256 the prompt gives.
    pub(crate) fn ready(&self, sid: SessionId, r: &UploadRef) -> Result<PathBuf, String> {
        let t = self.table();
        match t.get(&(sid, r.upload_id.clone())) {
            Some(e) if e.complete && e.size == r.size && e.sha256 == r.sha256 => Ok(e.path.clone()),
            _ => Err(format!(
                "{}: the file did not reach this node whole; attach it again",
                harness_core::event::shown_name(&r.name)
            )),
        }
    }

    /// Delete one staged file (saved, or refused).
    pub(crate) fn discard(&self, sid: SessionId, upload_id: &str) {
        if let Some(e) = self.table().remove(&(sid, upload_id.to_string())) {
            let _ = std::fs::remove_file(&e.path);
        }
    }

    /// Delete everything staged for a chat (it was deleted).
    pub(crate) fn drop_session(&self, sid: SessionId) {
        let mut t = self.table();
        t.retain(|(s, _), e| {
            let keep = *s != sid;
            if !keep {
                let _ = std::fs::remove_file(&e.path);
            }
            keep
        });
        if let Some(root) = &self.dir {
            let _ = std::fs::remove_dir_all(root.join(sid.to_string()));
        }
    }

    /// How many files are staged now (tests).
    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        self.table().len()
    }
}

#[cfg(test)]
#[path = "uploads_tests.rs"]
mod tests;
