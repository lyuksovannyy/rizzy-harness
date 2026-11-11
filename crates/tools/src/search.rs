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
fn walker(root: &PathBuf, protected: &std::sync::Arc<Vec<PathBuf>>) -> ignore::Walk {
    let protected = protected.clone();
    ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .follow_links(false)
        .filter_entry(move |e| {
            e.file_name() != ".git" && !in_protected(e.path(), e.file_type(), &protected)
        })
        .build()
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
        "Search file contents with a regular expression (Rust regex syntax). Respects .gitignore. Returns path:line:text."
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
        let cancel = ctx.cancel.clone();
        let bounded = ctx.savings.bounded_reads;
        let max_results = if bounded {
            BOUNDED_GREP_RESULTS
        } else {
            MAX_GREP_RESULTS
        };
        let res = tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            let mut skipped = 0usize;
            'files: for entry in walker(&root, &protected).flatten() {
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
                if bytes.iter().take(8192).any(|b| *b == 0) {
                    continue;
                }
                let text = String::from_utf8_lossy(&bytes);
                for (i, line) in text.lines().enumerate() {
                    if re.is_match(line) {
                        let line: String =
                            line.chars().take(if bounded { 200 } else { 300 }).collect();
                        out.push(format!("{}:{}:{}", rel.display(), i + 1, line));
                        if out.len() >= max_results {
                            out.push(format!(
                                "[stopped at {max_results} matches; narrow the pattern or the path]"
                            ));
                            break 'files;
                        }
                    }
                }
            }
            Ok((out, skipped))
        })
        .await;
        let (lines, skipped) = match res {
            Ok(Ok(r)) => r,
            Ok(Err(())) => return ToolOutcome::err("[cancelled]"),
            Err(e) => return ToolOutcome::err(format!("grep failed: {e}")),
        };
        let mut text = if lines.is_empty() {
            "No matches.".to_string()
        } else if bounded {
            group_by_file(&lines)
        } else {
            lines.join("\n")
        };
        if skipped > 0 {
            text.push_str(&format!(
                "\n[skipped {skipped} file{} over {} bytes; search those with bash]",
                if skipped == 1 { "" } else { "s" },
                crate::MAX_GREP_FILE_BYTES
            ));
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
        let max_files = if ctx.savings.bounded_reads {
            BOUNDED_GLOB_RESULTS
        } else {
            MAX_GLOB_RESULTS
        };
        let cancel = ctx.cancel.clone();
        let res = tokio::task::spawn_blocking(move || {
            let mut found: Vec<(SystemTime, String)> = Vec::new();
            for entry in walker(&root, &protected).flatten() {
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

/// `path:line:text` lines regrouped so each path is written once.
fn group_by_file(lines: &[String]) -> String {
    let mut out = String::new();
    let mut current: Option<&str> = None;
    for l in lines {
        let mut parts = l.splitn(3, ':');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(p), Some(n), Some(t)) if n.chars().all(|c| c.is_ascii_digit()) => {
                if current != Some(p) {
                    out.push_str(p);
                    out.push_str(":\n");
                    current = Some(p);
                }
                out.push_str(&format!("  {n}: {t}\n"));
            }
            _ => {
                out.push_str(l);
                out.push('\n');
            }
        }
    }
    out.trim_end().to_string()
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
        assert!(r.output.contains("src/a.rs:2:// TODO x"), "{}", r.output);
        assert!(r.output.contains("notes.md:1"));
        assert!(!r.output.contains("ignored"), "{}", r.output);
        let r = Grep
            .run(
                &ctx,
                json!({"pattern": "todo", "case_insensitive": true, "glob": "*.rs"}),
            )
            .await;
        assert!(r.output.contains("src/inner/b.rs:1"));
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
                assert!(r.output.contains("src/a.rs:2"), "{}", r.output);
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
