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

/// Create or truncate `path` and write `bytes`, refusing a symlink at the
/// last component (see [`crate::open_nofollow`]).
async fn write_nofollow(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let mut f = crate::open_nofollow(
        path,
        tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true),
    )
    .await?;
    f.write_all(bytes)
        .await
        .map_err(|e| format!("{}: {e}", path.display()))?;
    f.flush()
        .await
        .map_err(|e| format!("{}: {e}", path.display()))
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
}
