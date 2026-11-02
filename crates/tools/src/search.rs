use std::path::PathBuf;
use std::time::SystemTime;

use harness_core::policy::ToolRisk;
use serde_json::{Value, json};

use crate::{Tool, ToolCtx, ToolOutcome, str_arg};

const MAX_GREP_RESULTS: usize = 200;
const MAX_GLOB_RESULTS: usize = 500;

pub struct Grep;
pub struct Glob;

fn root(ctx: &ToolCtx, input: &Value) -> Result<PathBuf, String> {
    match input.get("path").and_then(|v| v.as_str()) {
        Some(p) => ctx.resolve(p),
        None => Ok(ctx.workspace.clone()),
    }
}

fn walker(root: &PathBuf) -> ignore::Walk {
    ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
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
        let ws = ctx.workspace.clone();
        let cancel = ctx.cancel.clone();
        let res = tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            'files: for entry in walker(&root).flatten() {
                if cancel.is_cancelled() {
                    break;
                }
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
                let Ok(bytes) = std::fs::read(path) else {
                    continue;
                };
                if bytes.iter().take(8192).any(|b| *b == 0) {
                    continue;
                }
                let text = String::from_utf8_lossy(&bytes);
                for (i, line) in text.lines().enumerate() {
                    if re.is_match(line) {
                        let line: String = line.chars().take(300).collect();
                        out.push(format!("{}:{}:{}", rel.display(), i + 1, line));
                        if out.len() >= MAX_GREP_RESULTS {
                            out.push(format!("[stopped at {MAX_GREP_RESULTS} matches]"));
                            break 'files;
                        }
                    }
                }
            }
            out
        })
        .await;
        match res {
            Ok(lines) if lines.is_empty() => ToolOutcome::ok("No matches."),
            Ok(lines) => ToolOutcome::ok(lines.join("\n")),
            Err(e) => ToolOutcome::err(format!("grep failed: {e}")),
        }
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
        let ws = ctx.workspace.clone();
        let res = tokio::task::spawn_blocking(move || {
            let mut found: Vec<(SystemTime, String)> = Vec::new();
            for entry in walker(&root).flatten() {
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
            found
        })
        .await;
        match res {
            Ok(f) if f.is_empty() => ToolOutcome::ok("No files found."),
            Ok(f) => {
                let n = f.len();
                let mut lines: Vec<String> = f
                    .into_iter()
                    .take(MAX_GLOB_RESULTS)
                    .map(|(_, p)| p)
                    .collect();
                if n > MAX_GLOB_RESULTS {
                    lines.push(format!("[{n} files, showing {MAX_GLOB_RESULTS}]"));
                }
                ToolOutcome::ok(lines.join("\n"))
            }
            Err(e) => ToolOutcome::err(format!("glob failed: {e}")),
        }
    }
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
