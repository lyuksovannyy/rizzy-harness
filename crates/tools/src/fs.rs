use harness_core::policy::ToolRisk;
use serde_json::{Value, json};

use crate::{Tool, ToolCtx, ToolOutcome, str_arg};

const DEFAULT_READ_LINES: usize = 2000;
const MAX_LINE_CHARS: usize = 2000;
/// With bounded reads: the default window, and the per-line cap.
const BOUNDED_READ_LINES: usize = 400;
const BOUNDED_LINE_CHARS: usize = 500;

pub struct Read;
pub struct Write;
pub struct Edit;

#[async_trait::async_trait]
impl Tool for Read {
    fn name(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> &'static str {
        "Read a text file. Output lines are prefixed with their 1-based line number. Use offset/limit for large files."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "offset": {"type": "integer", "description": "1-based first line (default 1)"},
                "limit": {"type": "integer", "description": "Max lines (default 2000)"}
            },
            "required": ["path"]
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let path = match str_arg(&input, "path").and_then(|p| ctx.resolve(p)) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };
        let offset = input
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1) as usize;
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(if ctx.savings.bounded_reads {
                BOUNDED_READ_LINES
            } else {
                DEFAULT_READ_LINES
            });
        let (max_chars, compact) = if ctx.savings.bounded_reads {
            (BOUNDED_LINE_CHARS, true)
        } else {
            (MAX_LINE_CHARS, false)
        };
        let cancel = ctx.cancel.clone();
        let p = path.clone();
        let window = Window {
            first: offset,
            limit,
            max_chars,
            compact,
        };
        let res = tokio::task::spawn_blocking(move || {
            read_window(&p, &window, crate::MAX_READ_BYTES, &cancel)
        })
        .await;
        let Lines {
            mut out,
            total,
            capped,
        } = match res {
            Ok(Ok(l)) => l,
            Ok(Err(e)) => return ToolOutcome::err(e),
            Err(e) => return ToolOutcome::err(format!("read failed: {e}")),
        };
        if capped {
            out.push_str(&format!(
                "[file truncated at {} bytes: reading stopped there, in line {total}; the rest \
                 was not read. Use bash (tail, sed -n) for later parts]\n",
                crate::MAX_READ_BYTES
            ));
        } else if total == 0 {
            out.push_str("(empty file)\n");
        } else if offset - 1 + limit < total {
            out.push_str(&format!(
                "[showing lines {}-{} of {total}; use offset to read more{}]\n",
                offset,
                offset - 1 + limit,
                if ctx.savings.outline_tool {
                    ", or `outline` for the file's structure"
                } else {
                    ""
                }
            ));
        }
        ToolOutcome::ok(out)
    }
}

#[async_trait::async_trait]
impl Tool for Write {
    fn name(&self) -> &'static str {
        "write"
    }
    fn description(&self) -> &'static str {
        "Create or overwrite a file with the given content. Parent directories are created."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"]
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::Mutating
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let path = match str_arg(&input, "path").and_then(|p| ctx.resolve_for_write(p)) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };
        let content = match str_arg(&input, "content") {
            Ok(c) => c,
            Err(e) => return ToolOutcome::err(e),
        };
        if let Some(parent) = path.parent()
            && let Err(e) = tokio::fs::create_dir_all(parent).await
        {
            return ToolOutcome::err(format!("{}: {e}", parent.display()));
        }
        match write_nofollow(&path, content.as_bytes()).await {
            Ok(()) => ToolOutcome::ok(format!(
                "Wrote {} ({} lines)",
                path.display(),
                content.lines().count()
            )),
            Err(e) => ToolOutcome::err(e),
        }
    }
}

#[async_trait::async_trait]
impl Tool for Edit {
    fn name(&self) -> &'static str {
        "edit"
    }
    fn description(&self) -> &'static str {
        "Replace an exact string in a file. `old_string` must match exactly once unless `replace_all` is true. Read the file first."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old_string": {"type": "string"},
                "new_string": {"type": "string"},
                "replace_all": {"type": "boolean"}
            },
            "required": ["path", "old_string", "new_string"]
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::Mutating
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let path = match str_arg(&input, "path").and_then(|p| ctx.resolve_for_write(p)) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };
        let (old, new) = match (str_arg(&input, "old_string"), str_arg(&input, "new_string")) {
            (Ok(o), Ok(n)) => (o, n),
            (Err(e), _) | (_, Err(e)) => return ToolOutcome::err(e),
        };
        if old.is_empty() {
            return ToolOutcome::err("old_string must not be empty; use `write` to create files");
        }
        if old == new {
            return ToolOutcome::err("old_string and new_string are identical");
        }
        let replace_all = input
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let text = match read_nofollow(&path).await {
            Ok(t) => t,
            Err(e) => return ToolOutcome::err(e),
        };
        let count = text.matches(old).count();
        if count == 0 {
            return ToolOutcome::err("old_string not found in file");
        }
        if count > 1 && !replace_all {
            return ToolOutcome::err(format!(
                "old_string matches {count} times; add context to make it unique or set replace_all"
            ));
        }
        let first_line = text[..text.find(old).unwrap_or(0)].lines().count().max(1);
        let updated = if replace_all {
            text.replace(old, new)
        } else {
            text.replacen(old, new, 1)
        };
        match write_nofollow(&path, updated.as_bytes()).await {
            Ok(()) => ToolOutcome::ok(format!(
                "Edited {}: {count} replacement(s), first at line {first_line}",
                path.display()
            )),
            Err(e) => ToolOutcome::err(e),
        }
    }
}

/// Replace `path` with `bytes` atomically (see [`replace_with`]).
async fn write_nofollow(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let path = path.to_path_buf();
    let bytes = bytes.to_vec();
    // Blocking I/O; a cancelled turn that drops this future does not stop
    // the replacement halfway: the task runs to its end either way.
    tokio::task::spawn_blocking(move || replace_with(&path, &bytes, std::io::Write::write_all))
        .await
        .map_err(|e| format!("write failed: {e}"))?
}

/// Prefix of the temporary file [`replace_with`] writes next to its target,
/// so a leftover after a crash or `kill -9` can be recognised and deleted.
const TEMP_PREFIX: &str = ".rizzy-harness-write.";

/// Replace `path` with `bytes` so that the file is always either the old
/// one or the new one, never truncated or half-written (ADOPTION_PLAN A.8,
/// PLAN.md T30). `fill` writes the bytes (a seam for tests).
///
/// The parent directory is opened once and every later step is relative to
/// that handle. The target must be absent or a regular file this process
/// may write; a symlink is refused (`O_NOFOLLOW`). The bytes go to a new
/// file `.rizzy-harness-write.<random>.tmp` in the same directory
/// (`O_CREAT|O_EXCL`, mode 0600), which is flushed, given the target's
/// permission bits (a new file: 0666 less the umask, as `create` gives),
/// `fsync`ed and renamed over the target; then the directory is `fsync`ed.
/// Just before the rename the target is checked again: if it is no longer
/// the file that was opened (or, for a new file, has appeared), nothing is
/// replaced. The temporary file is removed on every failure.
///
/// The rename splits a hard link and drops extended attributes and ACLs;
/// the owner becomes this process's user. A directory this process cannot
/// write to cannot hold the temporary file, so its files cannot be changed:
/// that is an error, never a fallback to writing in place.
fn replace_with(
    path: &std::path::Path,
    bytes: &[u8],
    fill: impl FnOnce(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
) -> Result<(), String> {
    use nix::errno::Errno;
    use nix::fcntl::{AtFlags, OFlag};
    use nix::sys::stat::{Mode, SFlag};
    let shown = path.display();
    let ioerr = |e: std::io::Error| format!("{shown}: {e}");
    let nixerr = |e: Errno| format!("{shown}: {}", std::io::Error::from(e));
    let name = path
        .file_name()
        .map(std::path::Path::new)
        .ok_or_else(|| format!("{shown}: not a file name"))?;
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => std::path::Path::new("."),
    };
    // A FIFO or device is refused before anything opens it.
    if let Ok(md) = std::fs::symlink_metadata(path)
        && !md.file_type().is_symlink()
    {
        crate::check_regular(path, md.file_type())?;
    }
    let dir = nix::fcntl::open(
        parent,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| format!("{}: {}", parent.display(), std::io::Error::from(e)))?;

    // The target as it is now: absent, or a regular file we may write.
    // Opening it for writing (never truncating) keeps the rule that a
    // read-only file is not changed.
    let nofollow = OFlag::O_NOFOLLOW | OFlag::from_bits_truncate(crate::open_flags());
    let before = match nix::fcntl::openat(
        &dir,
        name,
        OFlag::O_WRONLY | OFlag::O_CLOEXEC | nofollow,
        Mode::empty(),
    ) {
        Ok(fd) => {
            let f = std::fs::File::from(fd);
            crate::check_regular(path, f.metadata().map_err(ioerr)?.file_type())?;
            Some(nix::sys::stat::fstat(&f).map_err(nixerr)?)
        }
        Err(Errno::ENOENT) => None,
        Err(Errno::ELOOP) => {
            return Err(format!("{shown}: is a symlink; refusing to follow it"));
        }
        Err(e) => return Err(nixerr(e)),
    };
    let mode = match &before {
        Some(st) => Mode::from_bits_truncate(st.st_mode & 0o777),
        None => Mode::from_bits_truncate(0o666 & !umask()),
    };

    let mut id = [0u8; 16];
    getrandom::fill(&mut id)
        .map_err(|e| format!("{shown}: no randomness for a temporary name: {e}"))?;
    let hex: String = id.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = std::path::PathBuf::from(format!("{TEMP_PREFIX}{hex}.tmp"));
    let fd = nix::fcntl::openat(
        &dir,
        &tmp,
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_CLOEXEC | nofollow,
        Mode::from_bits_truncate(0o600),
    )
    .map_err(|e| match e {
        Errno::EACCES | Errno::EPERM | Errno::EROFS => format!(
            "cannot change {shown}: its directory {} is not writable ({}). Files are replaced \
             through a temporary file in the same directory, so a crash never leaves them \
             half-written; ask the user to make this change or to make the directory writable",
            parent.display(),
            std::io::Error::from(e)
        ),
        e => format!(
            "{shown}: cannot create a temporary file in {}: {}",
            parent.display(),
            std::io::Error::from(e)
        ),
    })?;
    let mut guard = TempGuard {
        dir: &dir,
        name: &tmp,
        armed: true,
    };
    let mut f = std::fs::File::from(fd);
    fill(&mut f, bytes).map_err(ioerr)?;
    std::io::Write::flush(&mut f).map_err(ioerr)?;
    nix::sys::stat::fchmod(&f, mode).map_err(nixerr)?;
    f.sync_all().map_err(ioerr)?;
    drop(f);

    // Still the file that was opened? A link or another file swapped in
    // meanwhile is left alone.
    let now = match nix::sys::stat::fstatat(&dir, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(st) => Some(st),
        Err(Errno::ENOENT) => None,
        Err(e) => return Err(nixerr(e)),
    };
    let same = match (&before, &now) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.st_dev == b.st_dev
                && a.st_ino == b.st_ino
                && SFlag::from_bits_truncate(b.st_mode) & SFlag::S_IFMT == SFlag::S_IFREG
        }
        _ => false,
    };
    if !same {
        return Err(format!(
            "{shown}: changed while it was being written (replaced, removed, created or turned \
             into a symlink); nothing was written. Read it again and retry"
        ));
    }
    nix::fcntl::renameat(&dir, &tmp, &dir, name).map_err(nixerr)?;
    guard.armed = false;
    nix::unistd::fsync(&dir).map_err(|e| {
        format!(
            "{shown} was replaced, but its directory could not be synced ({}); the change may \
             not survive a crash",
            std::io::Error::from(e)
        )
    })
}

/// Removes [`replace_with`]'s temporary file unless it was renamed.
struct TempGuard<'a> {
    dir: &'a std::os::fd::OwnedFd,
    name: &'a std::path::Path,
    armed: bool,
}

impl Drop for TempGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ =
                nix::unistd::unlinkat(self.dir, self.name, nix::unistd::UnlinkatFlags::NoRemoveDir);
        }
    }
}

/// This process's umask, read without changing it (`umask(2)` can only be
/// read by setting it, which races with every other thread creating a
/// file). Linux has it in `/proc/self/status`; elsewhere, or if that cannot
/// be read, the common default 022.
fn umask() -> nix::sys::stat::mode_t {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Umask:"))
                .and_then(|v| nix::sys::stat::mode_t::from_str_radix(v.trim(), 8).ok())
        })
        .unwrap_or(0o022)
}

/// Read `path` as UTF-8 without following a symlink at the last component.
/// Regular files of at most [`crate::MAX_GREP_FILE_BYTES`] only: `edit`
/// holds the file and its edited copy in memory.
async fn read_nofollow(path: &std::path::Path) -> Result<String, String> {
    use tokio::io::AsyncReadExt;
    let cap = crate::MAX_GREP_FILE_BYTES;
    let f = crate::open_nofollow(path, tokio::fs::OpenOptions::new().read(true)).await?;
    let len = f
        .metadata()
        .await
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if len > cap {
        return Err(crate::too_big(path, len, cap));
    }
    let mut bytes = Vec::with_capacity(len as usize);
    f.take(cap + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > cap {
        return Err(crate::too_big(path, bytes.len() as u64, cap));
    }
    String::from_utf8(bytes).map_err(|_| format!("{}: not valid UTF-8", path.display()))
}

/// Which lines `read` shows, and how.
struct Window {
    /// 1-based.
    first: usize,
    limit: usize,
    max_chars: usize,
    compact: bool,
}

struct Lines {
    out: String,
    /// Lines seen (all of them unless `capped`).
    total: usize,
    /// Reading stopped at the byte cap with more of the file left.
    capped: bool,
}

/// Stream a regular file line by line through a fixed buffer, keeping only
/// the lines in `w` (each cut at `max_chars`). Memory is bounded by the
/// window, never by the file; at most `cap` bytes are read, so a huge or
/// growing file ends in bounded time. Checks `cancel` between chunks.
fn read_window(
    path: &std::path::Path,
    w: &Window,
    cap: u64,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<Lines, String> {
    use std::io::{BufRead, Read as _};
    let cancelled = || Err("[cancelled]".to_string());
    if cancel.is_cancelled() {
        return cancelled();
    }
    let f = crate::open_regular(path)?;
    let io = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut r = std::io::BufReader::with_capacity(64 * 1024, f.take(cap + 1));
    let head = r.fill_buf().map_err(io)?;
    if head.iter().take(8192).any(|b| *b == 0) {
        return Err(format!("{} looks like a binary file", path.display()));
    }
    // A line's bytes kept for display: enough for `max_chars` characters.
    let keep = w.max_chars * 4 + 4;
    let mut out = String::new();
    let mut line: Vec<u8> = Vec::new();
    let mut long = false;
    let mut total = 0usize;
    let mut read = 0u64;
    let mut pending = false;
    let last = w.first.saturating_sub(1).saturating_add(w.limit);
    let emit = |n: usize, line: &mut Vec<u8>, long: bool, out: &mut String| {
        if n >= w.first && n <= last {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let text = String::from_utf8_lossy(line);
            let mut shown: String = text.chars().take(w.max_chars).collect();
            if long || text.chars().count() > w.max_chars {
                shown.push('…');
            }
            if w.compact {
                // No padding: a few tokens per line add up over long files.
                out.push_str(&format!("{n}\t{shown}\n"));
            } else {
                out.push_str(&format!("{n:>6}\t{shown}\n"));
            }
        }
        line.clear();
    };
    loop {
        if cancel.is_cancelled() {
            return cancelled();
        }
        let buf = r.fill_buf().map_err(io)?;
        if buf.is_empty() || read >= cap {
            break;
        }
        // Never go past the cap, even inside a buffered chunk.
        let room = (cap - read).min(buf.len() as u64) as usize;
        let buf = &buf[..room];
        let (part, nl) = match buf.iter().position(|b| *b == b'\n') {
            Some(i) => (&buf[..i], true),
            None => (buf, false),
        };
        let wanted = total + 1 >= w.first && total < last;
        if wanted {
            let take = keep.saturating_sub(line.len()).min(part.len());
            line.extend_from_slice(&part[..take]);
            long |= take < part.len();
        }
        pending = true;
        let used = part.len() + usize::from(nl);
        r.consume(used);
        read += used as u64;
        if nl {
            total += 1;
            emit(total, &mut line, long, &mut out);
            long = false;
            pending = false;
        }
    }
    // More bytes than the cap: the file was cut (or grew while read).
    let capped = read >= cap && !r.fill_buf().map_err(io)?.is_empty();
    if pending {
        total += 1;
        emit(total, &mut line, long, &mut out);
    }
    Ok(Lines { out, total, capped })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn write_read_edit() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path());
        let w = Write
            .run(
                &ctx,
                json!({"path": "src/a.txt", "content": "one\ntwo\ntwo\n"}),
            )
            .await;
        assert!(w.ok, "{}", w.output);
        let r = Read.run(&ctx, json!({"path": "src/a.txt"})).await;
        assert!(r.output.contains("     2\ttwo"), "{}", r.output);
        let amb = Edit
            .run(
                &ctx,
                json!({"path": "src/a.txt", "old_string": "two", "new_string": "2"}),
            )
            .await;
        assert!(!amb.ok && amb.output.contains("2 times"));
        let e = Edit
            .run(
                &ctx,
                json!({"path": "src/a.txt", "old_string": "one", "new_string": "1"}),
            )
            .await;
        assert!(e.ok, "{}", e.output);
        let all = Edit
            .run(
                &ctx,
                json!({"path": "src/a.txt", "old_string": "two", "new_string": "2", "replace_all": true}),
            )
            .await;
        assert!(all.ok);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("src/a.txt")).unwrap(),
            "1\n2\n2\n"
        );
    }

    #[tokio::test]
    async fn read_offset_limit() {
        let dir = tempfile::tempdir().unwrap();
        let body: String = (1..=10).map(|i| format!("l{i}\n")).collect();
        std::fs::write(dir.path().join("f"), body).unwrap();
        let ctx = ToolCtx::new(dir.path());
        let r = Read
            .run(&ctx, json!({"path": "f", "offset": 3, "limit": 2}))
            .await;
        assert!(r.output.contains("     3\tl3"));
        assert!(r.output.contains("     4\tl4"));
        assert!(!r.output.contains("l5"));
        assert!(r.output.contains("of 10"));
    }

    #[tokio::test]
    async fn refuses_outside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path());
        let r = Read.run(&ctx, json!({"path": "/etc/hostname"})).await;
        assert!(!r.ok && r.output.contains("outside"));
    }

    /// AUDIT S10: no file tool may block on, or load, something that is not
    /// a bounded regular file.
    #[cfg(unix)]
    mod wedge {
        use super::*;
        use std::os::unix::fs::symlink;
        use std::time::Duration;

        fn fifo(path: &std::path::Path) {
            nix::unistd::mkfifo(path, nix::sys::stat::Mode::from_bits_truncate(0o600)).unwrap();
        }

        async fn quick(f: impl std::future::Future<Output = ToolOutcome>) -> ToolOutcome {
            tokio::time::timeout(Duration::from_secs(3), f)
                .await
                .expect("the tool blocked")
        }

        #[tokio::test]
        async fn read_refuses_what_is_not_a_regular_file_at_once() {
            let ws = tempfile::tempdir().unwrap();
            fifo(&ws.path().join("p"));
            std::fs::create_dir(ws.path().join("d")).unwrap();
            let _sock = std::os::unix::net::UnixListener::bind(ws.path().join("s")).unwrap();
            for (link, target) in [("lp", "p"), ("ld", "d"), ("ls", "s")] {
                symlink(ws.path().join(target), ws.path().join(link)).unwrap();
            }
            let ctx = ToolCtx::new(ws.path());
            for p in ["p", "lp", "d", "ld", "s", "ls"] {
                let r = quick(Read.run(&ctx, json!({ "path": p }))).await;
                assert!(!r.ok, "{p}: {}", r.output);
                assert!(r.output.contains("not a regular file"), "{p}: {}", r.output);
            }
            // Devices, reachable in outside mode, directly and by a link.
            symlink("/dev/zero", ws.path().join("lz")).unwrap();
            let out = ToolCtx {
                allow_outside_workspace: true,
                ..ToolCtx::new(ws.path())
            };
            for p in ["/dev/zero", "/dev/random", "/dev/urandom", "lz", "p"] {
                let r = quick(Read.run(&out, json!({ "path": p }))).await;
                assert!(
                    !r.ok && r.output.contains("not a regular file"),
                    "{p}: {}",
                    r.output
                );
            }
        }

        #[tokio::test]
        async fn write_edit_and_outline_do_not_block_on_a_fifo() {
            let ws = tempfile::tempdir().unwrap();
            fifo(&ws.path().join("p.rs"));
            let ctx = ToolCtx {
                savings: harness_core::savings::Savings {
                    outline_tool: true,
                    ..Default::default()
                },
                ..ToolCtx::new(ws.path())
            };
            let w = quick(Write.run(&ctx, json!({"path": "p.rs", "content": "x"}))).await;
            assert!(!w.ok, "{}", w.output);
            let e = quick(Edit.run(
                &ctx,
                json!({"path": "p.rs", "old_string": "a", "new_string": "b"}),
            ))
            .await;
            assert!(
                !e.ok && e.output.contains("not a regular file"),
                "{}",
                e.output
            );
            let o = quick(crate::Outline.run(&ctx, json!({"path": "p.rs"}))).await;
            assert!(
                !o.ok && o.output.contains("not a regular file"),
                "{}",
                o.output
            );
        }

        /// Resident memory of this process, in bytes.
        fn rss() -> u64 {
            let s = std::fs::read_to_string("/proc/self/statm").unwrap();
            let pages: u64 = s.split_whitespace().nth(1).unwrap().parse().unwrap();
            pages * 4096
        }

        #[tokio::test]
        async fn read_of_a_2gb_sparse_file_stays_under_the_cap() {
            let ws = tempfile::tempdir().unwrap();
            let path = ws.path().join("big.log");
            let head: String = (1..=2000).map(|i| format!("line {i}\n")).collect();
            std::fs::write(&path, &head).unwrap();
            std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(2 << 30)
                .unwrap();
            let ctx = ToolCtx::new(ws.path());
            let before = rss();
            // Past every line there is: the whole cap is scanned.
            let r = tokio::time::timeout(
                Duration::from_secs(20),
                Read.run(&ctx, json!({"path": "big.log", "offset": 5000})),
            )
            .await
            .expect("read took too long");
            let grown = rss().saturating_sub(before);
            assert!(
                r.output
                    .contains(&format!("truncated at {} bytes", crate::MAX_READ_BYTES)),
                "{}",
                r.output
            );
            assert!(grown < 256 << 20, "resident memory grew by {grown} bytes");
            // The start of the file still reads normally.
            let r = Read.run(&ctx, json!({"path": "big.log", "limit": 3})).await;
            assert!(r.ok && r.output.contains("     3\tline 3"), "{}", r.output);
        }

        #[tokio::test]
        async fn a_cancelled_read_returns() {
            let ws = tempfile::tempdir().unwrap();
            std::fs::write(ws.path().join("f"), "a\n").unwrap();
            let ctx = ToolCtx::new(ws.path());
            ctx.cancel.cancel();
            let r = quick(Read.run(&ctx, json!({"path": "f"}))).await;
            assert!(!r.ok && r.output.contains("cancelled"), "{}", r.output);
        }
    }

    #[cfg(unix)]
    mod links {
        use super::*;
        use std::os::unix::fs::symlink;

        fn write(ctx: &ToolCtx, path: &str) -> ToolOutcome {
            futures_block(Write.run(ctx, json!({"path": path, "content": "x\n"})))
        }

        fn edit(ctx: &ToolCtx, path: &str) -> ToolOutcome {
            futures_block(Edit.run(
                ctx,
                json!({"path": path, "old_string": "a", "new_string": "b"}),
            ))
        }

        fn futures_block<F: std::future::Future>(f: F) -> F::Output {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(f)
        }

        #[test]
        fn dangling_link_as_last_component_is_refused() {
            let ws = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("x.service");
            symlink(&target, ws.path().join("notes.md")).unwrap();
            let ctx = ToolCtx::new(ws.path());
            let w = write(&ctx, "notes.md");
            assert!(!w.ok, "{}", w.output);
            assert!(w.output.contains("symlink"), "{}", w.output);
            assert!(!target.exists(), "the write followed the link");
        }

        #[test]
        fn dangling_link_as_a_middle_component_is_refused() {
            let ws = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("newdir");
            symlink(&target, ws.path().join("d")).unwrap();
            let ctx = ToolCtx::new(ws.path());
            let w = write(&ctx, "d/f.txt");
            assert!(!w.ok, "{}", w.output);
            assert!(!target.exists(), "the write created a directory outside");
        }

        #[test]
        fn link_to_an_existing_outside_file_is_refused() {
            let ws = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("keep");
            std::fs::write(&target, "a\n").unwrap();
            symlink(&target, ws.path().join("l")).unwrap();
            let ctx = ToolCtx::new(ws.path());
            assert!(!write(&ctx, "l").ok);
            assert!(!edit(&ctx, "l").ok);
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "a\n");
        }

        #[test]
        fn link_to_an_inside_file_writes_the_target() {
            let ws = tempfile::tempdir().unwrap();
            std::fs::write(ws.path().join("real.txt"), "a\n").unwrap();
            symlink(ws.path().join("real.txt"), ws.path().join("l")).unwrap();
            let ctx = ToolCtx::new(ws.path());
            let e = edit(&ctx, "l");
            assert!(e.ok, "{}", e.output);
            assert_eq!(
                std::fs::read_to_string(ws.path().join("real.txt")).unwrap(),
                "b\n"
            );
            let w = write(&ctx, "l");
            assert!(w.ok, "{}", w.output);
            assert_eq!(
                std::fs::read_to_string(ws.path().join("real.txt")).unwrap(),
                "x\n"
            );
            assert!(
                std::fs::symlink_metadata(ws.path().join("l"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }

        #[test]
        fn git_directory_is_read_only() {
            let ws = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(ws.path().join(".git/hooks")).unwrap();
            std::fs::write(ws.path().join(".git/config"), "a\n").unwrap();
            let ctx = ToolCtx::new(ws.path());
            for p in [".git/config", ".git/hooks/pre-commit", "sub/../.git/config"] {
                let w = write(&ctx, p);
                assert!(!w.ok && w.output.contains(".git"), "{p}: {}", w.output);
            }
            let e = edit(&ctx, ".git/config");
            assert!(!e.ok && e.output.contains(".git"), "{}", e.output);
            assert!(!ws.path().join(".git/hooks/pre-commit").exists());
            assert_eq!(
                std::fs::read_to_string(ws.path().join(".git/config")).unwrap(),
                "a\n"
            );
            let r = futures_block(Read.run(&ctx, json!({"path": ".git/config"})));
            assert!(r.ok, "{}", r.output);
            // A link into .git is no way around it.
            symlink(ws.path().join(".git"), ws.path().join("g")).unwrap();
            let w = write(&ctx, "g/hooks/pre-commit");
            assert!(!w.ok && w.output.contains(".git"), "{}", w.output);
        }

        #[test]
        fn git_rule_holds_outside_the_workspace_too() {
            let ws = tempfile::tempdir().unwrap();
            let ctx = ToolCtx {
                allow_outside_workspace: true,
                ..ToolCtx::new(ws.path())
            };
            assert!(!write(&ctx, ".git/config").ok);
            let w = write(&ctx, "plain.txt");
            assert!(w.ok, "{}", w.output);
        }

        #[test]
        fn outside_mode_still_does_not_follow_a_final_link() {
            let ws = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("t");
            symlink(&target, ws.path().join("l")).unwrap();
            let ctx = ToolCtx {
                allow_outside_workspace: true,
                ..ToolCtx::new(ws.path())
            };
            assert!(!write(&ctx, "l").ok);
            assert!(!target.exists());
        }

        #[test]
        fn plain_nested_files_and_traversal() {
            let ws = tempfile::tempdir().unwrap();
            let ctx = ToolCtx::new(ws.path());
            let w = write(&ctx, "a/b/c.txt");
            assert!(w.ok && w.output.starts_with("Wrote "), "{}", w.output);
            assert_eq!(
                std::fs::read_to_string(ws.path().join("a/b/c.txt")).unwrap(),
                "x\n"
            );
            let again = write(&ctx, "a/b/c.txt");
            assert!(again.ok, "{}", again.output);
            let t = write(&ctx, "../escape.txt");
            assert!(!t.ok && t.output.contains("outside"), "{}", t.output);
        }
    }

    /// ADOPTION_PLAN A.8: `edit` and `write` replace a file atomically, so a
    /// failure halfway never leaves it truncated or half-written.
    #[cfg(unix)]
    mod atomic {
        use super::*;
        use std::os::unix::fs::{PermissionsExt, symlink};
        use std::path::Path;

        fn mode(p: &Path) -> u32 {
            std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o7777
        }

        fn chmod(p: &Path, m: u32) {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
        }

        /// The names in `dir`, sorted.
        fn names(dir: &Path) -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        }

        /// Write half of the bytes, then fail like a full disk.
        fn half_then_fail(f: &mut std::fs::File, b: &[u8]) -> std::io::Result<()> {
            std::io::Write::write_all(f, &b[..b.len() / 2])?;
            Err(std::io::Error::other("simulated full disk"))
        }

        #[test]
        fn a_write_failing_halfway_leaves_the_original_and_no_temp_file() {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("keep.txt");
            let original = "the user's work\n".repeat(100);
            std::fs::write(&p, &original).unwrap();
            let r = replace_with(&p, "new content\n".repeat(50).as_bytes(), half_then_fail);
            assert!(r.is_err(), "{r:?}");
            assert!(r.unwrap_err().contains("simulated full disk"));
            assert_eq!(std::fs::read_to_string(&p).unwrap(), original);
            assert_eq!(names(dir.path()), ["keep.txt"]);
        }

        #[test]
        fn a_new_file_failing_halfway_leaves_nothing() {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("new.txt");
            let r = replace_with(&p, b"0123456789", half_then_fail);
            assert!(r.is_err(), "{r:?}");
            assert!(names(dir.path()).is_empty(), "{:?}", names(dir.path()));
        }

        #[tokio::test]
        async fn mode_0640_is_kept_by_edit_and_write() {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("f.txt");
            std::fs::write(&p, "a\n").unwrap();
            chmod(&p, 0o640);
            let ctx = ToolCtx::new(dir.path());
            let e = Edit
                .run(
                    &ctx,
                    json!({"path": "f.txt", "old_string": "a", "new_string": "b"}),
                )
                .await;
            assert!(e.ok, "{}", e.output);
            assert_eq!(mode(&p), 0o640);
            let w = Write
                .run(&ctx, json!({"path": "f.txt", "content": "c\n"}))
                .await;
            assert!(w.ok, "{}", w.output);
            assert_eq!(mode(&p), 0o640);
            assert_eq!(std::fs::read_to_string(&p).unwrap(), "c\n");
        }

        #[tokio::test]
        async fn a_symlink_target_is_refused_by_edit_and_write() {
            let ws = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("t");
            std::fs::write(&target, "a\n").unwrap();
            let link = ws.path().join("l");
            symlink(&target, &link).unwrap();
            // Outside mode does not resolve links, so the final link reaches
            // the write itself.
            let ctx = ToolCtx {
                allow_outside_workspace: true,
                ..ToolCtx::new(ws.path())
            };
            let w = Write
                .run(&ctx, json!({"path": "l", "content": "x\n"}))
                .await;
            assert!(!w.ok && w.output.contains("symlink"), "{}", w.output);
            let e = Edit
                .run(
                    &ctx,
                    json!({"path": "l", "old_string": "a", "new_string": "b"}),
                )
                .await;
            assert!(!e.ok && e.output.contains("symlink"), "{}", e.output);
            // The write step itself, as `edit` would reach it.
            let r = replace_with(&link, b"x\n", std::io::Write::write_all);
            assert!(r.as_ref().is_err_and(|e| e.contains("symlink")), "{r:?}");
            assert!(
                std::fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(std::fs::read_link(&link).unwrap(), target);
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "a\n");
            assert_eq!(names(ws.path()), ["l"]);
        }

        /// A link swapped in while the new content is written (the race
        /// with the check) is neither followed nor replaced.
        #[test]
        fn a_link_swapped_in_during_the_write_is_refused() {
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("t");
            std::fs::write(&target, "outside\n").unwrap();
            let p = dir.path().join("f.txt");
            std::fs::write(&p, "a\n").unwrap();
            let (p2, t2) = (p.clone(), target.clone());
            let r = replace_with(&p, b"x\n", move |f, b| {
                std::fs::remove_file(&p2)?;
                symlink(&t2, &p2)?;
                std::io::Write::write_all(f, b)
            });
            assert!(r.is_err(), "{r:?}");
            assert!(
                std::fs::symlink_metadata(&p)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "outside\n");
            assert_eq!(names(dir.path()), ["f.txt"]);
        }

        /// A rename needs only the directory writable; a read-only file
        /// must still not be replaced, as it could not be written in place.
        #[tokio::test]
        async fn a_read_only_file_is_still_not_replaced() {
            let dir = tempfile::tempdir().unwrap();
            if std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(dir.path()).unwrap()) == 0 {
                eprintln!("skipped: running as root, file permissions are not enforced");
                return;
            }
            let p = dir.path().join("ro.txt");
            std::fs::write(&p, "a\n").unwrap();
            chmod(&p, 0o444);
            let ctx = ToolCtx::new(dir.path());
            let w = Write
                .run(&ctx, json!({"path": "ro.txt", "content": "x\n"}))
                .await;
            assert!(!w.ok, "{}", w.output);
            assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\n");
            assert_eq!(mode(&p), 0o444);
            assert_eq!(names(dir.path()), ["ro.txt"]);
        }

        #[tokio::test]
        async fn a_new_file_gets_the_mode_a_plain_create_gives() {
            let dir = tempfile::tempdir().unwrap();
            // What `create(true)` gives here: 0o666 less the umask.
            std::fs::File::create(dir.path().join("probe")).unwrap();
            let expected = mode(&dir.path().join("probe"));
            let ctx = ToolCtx::new(dir.path());
            let w = Write
                .run(&ctx, json!({"path": "new.txt", "content": "x\n"}))
                .await;
            assert!(w.ok, "{}", w.output);
            assert_eq!(mode(&dir.path().join("new.txt")), expected);
        }

        #[tokio::test]
        async fn a_successful_edit_leaves_no_temp_file() {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("f.txt"), "one\ntwo\n").unwrap();
            let ctx = ToolCtx::new(dir.path());
            let e = Edit
                .run(
                    &ctx,
                    json!({"path": "f.txt", "old_string": "two", "new_string": "2"}),
                )
                .await;
            assert!(e.ok, "{}", e.output);
            assert_eq!(
                std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
                "one\n2\n"
            );
            assert_eq!(names(dir.path()), ["f.txt"]);
        }

        #[tokio::test]
        async fn a_file_in_a_read_only_directory_is_not_edited() {
            let dir = tempfile::tempdir().unwrap();
            // A new directory is owned by the effective user.
            if std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(dir.path()).unwrap()) == 0 {
                eprintln!("skipped: running as root, directory permissions are not enforced");
                return;
            }
            let sub = dir.path().join("ro");
            std::fs::create_dir(&sub).unwrap();
            let p = sub.join("f.txt");
            std::fs::write(&p, "a\n").unwrap();
            chmod(&sub, 0o555);
            let ctx = ToolCtx::new(dir.path());
            let e = Edit
                .run(
                    &ctx,
                    json!({"path": "ro/f.txt", "old_string": "a", "new_string": "b"}),
                )
                .await;
            let w = Write
                .run(&ctx, json!({"path": "ro/f.txt", "content": "c\n"}))
                .await;
            chmod(&sub, 0o755);
            for r in [&e, &w] {
                assert!(!r.ok, "{}", r.output);
                assert!(r.output.contains("not writable"), "{}", r.output);
            }
            assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\n");
            assert_eq!(names(&sub), ["f.txt"]);
        }
    }
}
