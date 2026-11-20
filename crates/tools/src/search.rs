use std::path::{Path, PathBuf};
use std::time::SystemTime;

use harness_core::policy::ToolRisk;
use serde_json::{Value, json};

use crate::{Tool, ToolCtx, ToolOutcome, str_arg};

const MAX_GREP_RESULTS: usize = 200;
const MAX_GLOB_RESULTS: usize = 500;
/// With bounded reads.
const BOUNDED_GREP_RESULTS: usize = 60;
const BOUNDED_GLOB_RESULTS: usize = 150;

/// Files one search records a version for (and heads with a tag), at most.
/// A recorded version is the file's text kept in memory for the whole node,
/// whose snapshots share [`crate::seen::SNAPSHOT_MAX_TOTAL`] (8 MiB): the
/// agent usually acts on a handful of the files a search lists, and a wide
/// search (a common word over a whole repository) must not fill the budget
/// the files it really read live in. Sixteen files is more than a person
/// edits after one search; the rest are listed without a tag, which tells
/// the model it must `read` them.
pub(crate) const MAX_TAGGED_FILES: usize = 16;
/// Bytes of file text one search records, at most: an eighth of the node's
/// snapshot budget (a typical source file is 5 to 50 KiB, so this holds the
/// sixteen above; sixteen files of the largest kept size, 512 KiB, would
/// not, and the later ones are listed without a tag). Versions nobody read
/// are also the first the store evicts ([`crate::seen`]).
pub(crate) const MAX_TAGGED_BYTES: usize = 1 << 20;

pub struct Grep;
pub struct Glob;

fn root(ctx: &ToolCtx, input: &Value) -> Result<PathBuf, String> {
    match input.get("path").and_then(|v| v.as_str()) {
        Some(p) => ctx.resolve(p),
        None => ctx.workspace_dir(),
    }
}

/// Links are not followed (the `ignore` default), and a protected folder
/// (the data directory) is never entered: a search rooted above it, which
/// outside mode allows, skips it.
///
/// `own` is a sub-agent's node-owned folder (inside the protected sessions
/// folder by design): it alone is searched although protected.
fn walker(
    root: &PathBuf,
    protected: &std::sync::Arc<Vec<PathBuf>>,
    own: Option<PathBuf>,
) -> ignore::Walk {
    let protected = protected.clone();
    ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .follow_links(false)
        .filter_entry(move |e| {
            e.file_name() != ".git"
                && (own.as_ref().is_some_and(|o| e.path().starts_with(o))
                    || !in_protected(e.path(), e.file_type(), &protected))
        })
        .build()
}

/// [`walker`] for the composer's completion (`crate::mention::complete`).
pub(crate) fn walk_folder(
    root: &PathBuf,
    protected: &std::sync::Arc<Vec<PathBuf>>,
    own: Option<PathBuf>,
) -> ignore::Walk {
    walker(root, protected, own)
}

/// A folder `path` is (or is in) a protected folder, judged by its real
/// path too.
pub(crate) fn in_protected_dir(path: &Path, protected: &[PathBuf]) -> bool {
    !protected.is_empty()
        && (protected.iter().any(|d| path.starts_with(d))
            || path
                .canonicalize()
                .is_ok_and(|real| protected.iter().any(|d| real.starts_with(d))))
}

fn in_protected(path: &Path, kind: Option<std::fs::FileType>, protected: &[PathBuf]) -> bool {
    if protected.is_empty() {
        return false;
    }
    if protected.iter().any(|d| path.starts_with(d)) {
        return true;
    }
    // The root may not be canonical; judge folders by their real path.
    kind.is_some_and(|k| k.is_dir())
        && path
            .canonicalize()
            .is_ok_and(|real| protected.iter().any(|d| real.starts_with(d)))
}

#[async_trait::async_trait]
impl Tool for Grep {
    fn name(&self) -> &'static str {
        "grep"
    }
    fn description(&self) -> &'static str {
        "Search file contents with a regular expression (Rust regex syntax). Respects .gitignore. Prints `[path#TAG]` then `line:text` rows, or `path:line:text` if untagged; `edit` takes the tag and `lines` for those rows with no `read`."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string", "description": "Directory or file to search (default: workspace)"},
                "glob": {"type": "string", "description": "Only files matching this glob, e.g. *.rs"},
                "case_insensitive": {"type": "boolean"}
            },
            "required": ["pattern"]
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    fn refusal(&self, ctx: &ToolCtx, input: &Value) -> Option<String> {
        root(ctx, input).err()
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let pattern = match str_arg(&input, "pattern") {
            Ok(p) => p.to_string(),
            Err(e) => return ToolOutcome::err(e),
        };
        let root = match root(ctx, &input) {
            Ok(r) => r,
            Err(e) => return ToolOutcome::err(e),
        };
        let ci = input
            .get("case_insensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let re = match regex::RegexBuilder::new(&pattern)
            .case_insensitive(ci)
            .build()
        {
            Ok(r) => r,
            Err(e) => return ToolOutcome::err(format!("bad regex: {e}")),
        };
        let filter = match input.get("glob").and_then(|v| v.as_str()) {
            Some(g) => match globset::Glob::new(g) {
                Ok(g) => Some(g.compile_matcher()),
                Err(e) => return ToolOutcome::err(format!("bad glob: {e}")),
            },
            None => None,
        };
        let ws = ctx
            .workspace_dir()
            .unwrap_or_else(|_| ctx.workspace.clone());
        let protected = ctx.protected.clone();
        let own = ctx.owned.as_ref().map(|o| o.top.clone());
        let cancel = ctx.cancel.clone();
        let bounded = ctx.savings.bounded_reads;
        let max_results = if bounded {
            BOUNDED_GREP_RESULTS
        } else {
            MAX_GREP_RESULTS
        };
        let res = tokio::task::spawn_blocking(move || {
            let cap_chars = if bounded { 200 } else { 300 };
            let mut hits: Vec<FileHit> = Vec::new();
            let mut count = 0usize;
            let mut stopped = false;
            let mut skipped = 0usize;
            let (mut tagged, mut tagged_bytes) = (0usize, 0usize);
            'files: for entry in walker(&root, &protected, own).flatten() {
                if cancel.is_cancelled() {
                    return Err(());
                }
                // Links are not followed and FIFOs, devices and sockets are
                // never opened; `load` checks the type again on the handle.
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    continue;
                }
                let path = entry.path();
                let rel = path.strip_prefix(&ws).unwrap_or(path);
                if let Some(f) = &filter
                    && !f.is_match(rel)
                    && !path.file_name().is_some_and(|n| f.is_match(n))
                {
                    continue;
                }
                let bytes = match load(path) {
                    Load::Bytes(b) => b,
                    Load::TooBig => {
                        skipped += 1;
                        continue;
                    }
                    Load::Unreadable => continue,
                };
                if bytes.iter().take(crate::fs::BINARY_SNIFF).any(|b| *b == 0) {
                    continue;
                }
                let mut hit = FileHit {
                    path: path.to_path_buf(),
                    rel: rel.display().to_string(),
                    rows: Vec::new(),
                    bytes: None,
                    version: None,
                    head: None,
                };
                let utf8 = {
                    let text = String::from_utf8_lossy(&bytes);
                    // The rows `read` numbers: split at each `\n`, a final
                    // row without one counts, a `\r` before the `\n` (or at
                    // the very end) is not part of the row's text.
                    for (i, row) in text.split_inclusive('\n').enumerate() {
                        let row = row.strip_suffix('\n').unwrap_or(row);
                        let row = row.strip_suffix('\r').unwrap_or(row);
                        if !re.is_match(row) {
                            continue;
                        }
                        let n = i + 1;
                        let mut shown: String = row.chars().take(cap_chars).collect();
                        // A row cut is not shown whole: no edit may rest on it.
                        let whole = !crate::fs::is_cut(row, cap_chars);
                        if !whole {
                            shown.push('…');
                        }
                        hit.rows.push((n, shown, whole));
                        count += 1;
                        if count >= max_results {
                            stopped = true;
                            break;
                        }
                    }
                    matches!(text, std::borrow::Cow::Borrowed(_))
                };
                if !hit.rows.is_empty() {
                    // A version is recorded only where `read` would keep one
                    // (UTF-8, at most the snapshot size) and the search's own
                    // bounds allow it.
                    if hit.rows.iter().any(|r| r.2)
                        && utf8
                        && bytes.len() <= crate::seen::SNAPSHOT_MAX_FILE
                        && tagged < MAX_TAGGED_FILES
                        && tagged_bytes + bytes.len() <= MAX_TAGGED_BYTES
                    {
                        tagged += 1;
                        tagged_bytes += bytes.len();
                        hit.bytes = Some(bytes);
                    }
                    hits.push(hit);
                }
                if stopped {
                    break 'files;
                }
            }
            Ok((hits, skipped, stopped))
        })
        .await;
        let (mut hits, skipped, stopped) = match res {
            Ok(Ok(r)) => r,
            Ok(Err(())) => return ToolOutcome::err("[cancelled]"),
            Err(e) => return ToolOutcome::err(format!("grep failed: {e}")),
        };
        // The versions the model may name: recorded first, headed after, so
        // a version the store dropped meanwhile is not offered. No row counts
        // as shown yet: the rows the output holds do, once it is final.
        let mut recorded: Vec<(usize, PathBuf, crate::seen::Sha)> = Vec::new();
        for (i, h) in hits.iter_mut().enumerate() {
            let Some(bytes) = h.bytes.take() else {
                continue;
            };
            let Some(key) = tag_key(ctx, &h.path) else {
                continue;
            };
            ctx.seen.add_version(&key, &bytes);
            recorded.push((i, key, crate::seen::sha256(&bytes)));
        }
        for (i, key, sha) in recorded {
            if let Some(v) = ctx.seen.version_of(&key, &sha) {
                hits[i].head = Some(crate::fs::header(ctx, &hits[i].path, &v.tag));
                hits[i].version = Some((key, sha));
            }
        }
        let mut printed: Vec<crate::edit::RowRanges> = vec![Vec::new(); hits.len()];
        let mut text = if hits.is_empty() {
            "No matches.".to_string()
        } else {
            let mut budget = crate::shown::Budget::new();
            let (text, left) = render(&hits, bounded, &mut budget, &mut printed);
            let mut text = text;
            if left > 0 {
                text.push_str(&format!(
                    "\n[{left} more matched row{} not shown: the result is limited to about {} \
                     characters; narrow the pattern or the path]",
                    if left == 1 { "" } else { "s" },
                    crate::shown::RESULT_BUDGET
                ));
            }
            text
        };
        if stopped {
            text.push_str(&format!(
                "\n[stopped at {max_results} matches; narrow the pattern or the path]"
            ));
        }
        if skipped > 0 {
            text.push_str(&format!(
                "\n[skipped {skipped} file{} over {} bytes; search those with bash]",
                if skipped == 1 { "" } else { "s" },
                crate::MAX_GREP_FILE_BYTES
            ));
        }
        // The rows printed in full count as shown, of the version their
        // heading names (the file does not count as read).
        if let Some(proof) = crate::shown::seal(&text) {
            for (h, rows) in hits.iter().zip(&printed) {
                if let Some((key, sha)) = &h.version {
                    ctx.seen.add_shown(key, sha, rows, &proof);
                }
            }
        }
        ToolOutcome::ok(text)
    }
}

#[async_trait::async_trait]
impl Tool for Glob {
    fn name(&self) -> &'static str {
        "glob"
    }
    fn description(&self) -> &'static str {
        "Find files by glob pattern (e.g. **/*.rs), newest first. Respects .gitignore."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string", "description": "Directory to search (default: workspace)"}
            },
            "required": ["pattern"]
        })
    }
    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }
    fn parallel_safe(&self) -> bool {
        true
    }
    fn refusal(&self, ctx: &ToolCtx, input: &Value) -> Option<String> {
        root(ctx, input).err()
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let pattern = match str_arg(&input, "pattern") {
            Ok(p) => p.to_string(),
            Err(e) => return ToolOutcome::err(e),
        };
        let root = match root(ctx, &input) {
            Ok(r) => r,
            Err(e) => return ToolOutcome::err(e),
        };
        let matcher = match globset::GlobBuilder::new(&pattern)
            .literal_separator(true)
            .build()
        {
            Ok(g) => g.compile_matcher(),
            Err(e) => return ToolOutcome::err(format!("bad glob: {e}")),
        };
        let ws = ctx
            .workspace_dir()
            .unwrap_or_else(|_| ctx.workspace.clone());
        let protected = ctx.protected.clone();
        let own = ctx.owned.as_ref().map(|o| o.top.clone());
        let max_files = if ctx.savings.bounded_reads {
            BOUNDED_GLOB_RESULTS
        } else {
            MAX_GLOB_RESULTS
        };
        let cancel = ctx.cancel.clone();
        let res = tokio::task::spawn_blocking(move || {
            let mut found: Vec<(SystemTime, String)> = Vec::new();
            for entry in walker(&root, &protected, own).flatten() {
                if cancel.is_cancelled() {
                    return None;
                }
                if !entry.file_type().is_some_and(|t| t.is_file()) {
                    continue;
                }
                let path = entry.path();
                let rel_root = path.strip_prefix(&root).unwrap_or(path);
                if matcher.is_match(rel_root) {
                    let mtime = entry
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .unwrap_or(SystemTime::UNIX_EPOCH);
                    let rel = path.strip_prefix(&ws).unwrap_or(path);
                    found.push((mtime, rel.display().to_string()));
                }
            }
            found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            Some(found)
        })
        .await;
        match res {
            Ok(None) => ToolOutcome::err("[cancelled]"),
            Ok(Some(f)) if f.is_empty() => ToolOutcome::ok("No files found."),
            Ok(Some(f)) => {
                let n = f.len();
                let mut lines: Vec<String> =
                    f.into_iter().take(max_files).map(|(_, p)| p).collect();
                if n > max_files {
                    lines.push(format!("[{n} files, showing {max_files}]"));
                }
                ToolOutcome::ok(lines.join("\n"))
            }
            Err(e) => ToolOutcome::err(format!("glob failed: {e}")),
        }
    }
}

enum Load {
    Bytes(Vec<u8>),
    TooBig,
    Unreadable,
}

/// One file for `grep`: a regular file (checked on the open handle too) of
/// at most [`crate::MAX_GREP_FILE_BYTES`].
fn load(path: &Path) -> Load {
    match crate::read_small(path, crate::MAX_GREP_FILE_BYTES) {
        Ok(b) => Load::Bytes(b),
        Err(_) if std::fs::metadata(path).is_ok_and(|m| m.len() > crate::MAX_GREP_FILE_BYTES) => {
            Load::TooBig
        }
        Err(_) => Load::Unreadable,
    }
}

/// One file with matches.
struct FileHit {
    path: PathBuf,
    /// As the search has always printed it.
    rel: String,
    /// The matched rows: number, text as printed (a cut row ends in `…`) and
    /// whether it is printed in full.
    rows: Vec<(usize, String, bool)>,
    /// The file's bytes, while a version may still be recorded.
    bytes: Option<Vec<u8>>,
    /// The version recorded for the file, if one is kept.
    version: Option<(PathBuf, crate::seen::Sha)>,
    /// The `[path#TAG]` header, when a version was recorded and is kept.
    head: Option<String>,
}

/// The key a version of `path` is kept under, if `read` would read that
/// very file for the model and `edit` may change it: the same path checks
/// (`resolve`, then `resolve_for_write`'s refusal of `.git`), and the file
/// the walk found is the file they name (no link in between).
fn tag_key(ctx: &ToolCtx, path: &Path) -> Option<PathBuf> {
    let s = path.to_str()?;
    let real = ctx.resolve(s).ok()?;
    ctx.resolve_for_write(s).ok()?;
    let key = ctx.seen_key(&real);
    (key == ctx.seen_key(path)).then_some(key)
}

/// The results as text. A file with a tag is headed `[path#TAG]` as `read`
/// heads it, once, with its rows below (`N:text`, or `  N: text` in the
/// bounded style): in the plain style that costs one header and no path per
/// row, fewer characters than `path:N:text` for every file with two matches
/// or more, and about the same for one. A file without one is printed as it
/// always was.
///
/// Printing stops at the first row that does not fit `budget` (a header
/// goes with its first row): `printed` then holds, per file, the rows
/// printed in full, as inclusive ranges, and the count of matched rows left
/// out is returned. Only the final newline is dropped: the last row ends as
/// it is, as the rows before it do.
fn render(
    hits: &[FileHit],
    bounded: bool,
    budget: &mut crate::shown::Budget,
    printed: &mut [crate::edit::RowRanges],
) -> (String, usize) {
    let mut out = String::new();
    let mut left = 0;
    let mut full = false;
    for (h, ranges) in hits.iter().zip(printed.iter_mut()) {
        let mut lead = match (&h.head, bounded) {
            (Some(head), _) => format!("{head}\n"),
            (None, true) => format!("{}:\n", h.rel),
            (None, false) => String::new(),
        };
        for (n, t, whole) in &h.rows {
            if full {
                left += 1;
                continue;
            }
            let row = match (&h.head, bounded) {
                (_, true) => format!("  {n}: {t}\n"),
                (Some(_), false) => format!("{n}:{t}\n"),
                (None, false) => format!("{}:{n}:{t}\n", h.rel),
            };
            let piece = format!("{lead}{row}");
            if !budget.take(&piece) {
                full = true;
                left += 1;
                continue;
            }
            lead.clear();
            out.push_str(&piece);
            if *whole {
                match ranges.last_mut() {
                    Some(r) if r.1 + 1 == *n => r.1 = *n,
                    _ => ranges.push((*n, *n)),
                }
            }
        }
    }
    if out.ends_with('\n') {
        out.pop();
    }
    (out, left)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/inner")).unwrap();
        std::fs::write(dir.path().join("src/a.rs"), "fn main() {}\n// TODO x\n").unwrap();
        std::fs::write(dir.path().join("src/inner/b.rs"), "todo later\n").unwrap();
        std::fs::write(dir.path().join("notes.md"), "TODO md\n").unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
        std::fs::create_dir_all(dir.path().join("ignored")).unwrap();
        std::fs::write(dir.path().join("ignored/c.rs"), "TODO hidden\n").unwrap();
        // `ignore` only honours .gitignore inside a git repo.
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        dir
    }

    #[tokio::test]
    async fn grep_finds_and_filters() {
        let dir = fixture();
        let ctx = ToolCtx::new(dir.path());
        let r = Grep.run(&ctx, json!({"pattern": "TODO"})).await;
        // A file with matches is headed with its tag, as `read` heads it.
        assert!(
            r.output.contains("[src/a.rs#") && r.output.contains("\n2:// TODO x"),
            "{}",
            r.output
        );
        assert!(r.output.contains("[notes.md#"), "{}", r.output);
        assert!(r.output.contains("1:TODO md"), "{}", r.output);
        assert!(!r.output.contains("ignored"), "{}", r.output);
        let r = Grep
            .run(
                &ctx,
                json!({"pattern": "todo", "case_insensitive": true, "glob": "*.rs"}),
            )
            .await;
        assert!(r.output.contains("[src/inner/b.rs#"), "{}", r.output);
        assert!(!r.output.contains("notes.md"));
    }

    /// AUDIT S10: FIFOs, devices, link loops and huge files cannot wedge a
    /// search, and a cancelled one stops.
    #[cfg(unix)]
    #[tokio::test]
    async fn grep_survives_a_hostile_tree() {
        use std::os::unix::fs::symlink;
        let dir = fixture();
        let p = dir.path();
        nix::unistd::mkfifo(
            &p.join("src/pipe.rs"),
            nix::sys::stat::Mode::from_bits_truncate(0o600),
        )
        .unwrap();
        symlink(p.join("src/pipe.rs"), p.join("src/to_pipe.rs")).unwrap();
        symlink("/dev/zero", p.join("src/zero.rs")).unwrap();
        symlink(p.join("src"), p.join("src/inner/loop")).unwrap();
        let big = p.join("src/huge.rs");
        std::fs::write(&big, "TODO in a huge file\n").unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&big)
            .unwrap()
            .set_len(crate::MAX_GREP_FILE_BYTES + 1)
            .unwrap();
        let ctx = ToolCtx::new(p);
        let quick = std::time::Duration::from_secs(5);
        for root in [None, Some("src/pipe.rs"), Some("src/to_pipe.rs")] {
            let mut input = json!({"pattern": "TODO"});
            if let Some(r) = root {
                input["path"] = json!(r);
            }
            let r = tokio::time::timeout(quick, Grep.run(&ctx, input))
                .await
                .expect("grep blocked");
            assert!(!r.output.contains("huge.rs:1"), "{}", r.output);
            if root.is_none() {
                assert!(
                    r.output.contains("[src/a.rs#") && r.output.contains("\n2:// TODO x"),
                    "{}",
                    r.output
                );
                assert!(
                    r.output.contains("skipped 1 file over"),
                    "a skipped file must be said: {}",
                    r.output
                );
            }
        }
        // Outside mode, rooted at a device.
        let out = ToolCtx {
            allow_outside_workspace: true,
            ..ToolCtx::new(p)
        };
        let r = tokio::time::timeout(
            quick,
            Grep.run(&out, json!({"pattern": "x", "path": "/dev/zero"})),
        )
        .await
        .expect("grep blocked on /dev/zero");
        assert!(!r.output.contains("dev/zero:"), "{}", r.output);
    }

    #[tokio::test]
    async fn cancelled_searches_say_so() {
        let dir = fixture();
        let ctx = ToolCtx::new(dir.path());
        ctx.cancel.cancel();
        let g = Grep.run(&ctx, json!({"pattern": "TODO"})).await;
        assert!(!g.ok && g.output.contains("cancelled"), "{}", g.output);
        let l = Glob.run(&ctx, json!({"pattern": "**/*"})).await;
        assert!(!l.ok && l.output.contains("cancelled"), "{}", l.output);
    }

    #[tokio::test]
    async fn glob_matches() {
        let dir = fixture();
        let ctx = ToolCtx::new(dir.path());
        let r = Glob.run(&ctx, json!({"pattern": "**/*.rs"})).await;
        assert!(r.output.contains("src/a.rs"));
        assert!(r.output.contains("src/inner/b.rs"));
        assert!(!r.output.contains("ignored"));
        let r = Glob.run(&ctx, json!({"pattern": "*.md"})).await;
        assert_eq!(r.output, "notes.md");
    }
}
