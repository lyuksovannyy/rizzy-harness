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
        let bytes = match tokio::fs::read(&path).await {
            Ok(b) => b,
            Err(e) => return ToolOutcome::err(format!("{}: {e}", path.display())),
        };
        if bytes.iter().take(8192).any(|b| *b == 0) {
            return ToolOutcome::err(format!("{} looks like a binary file", path.display()));
        }
        let text = String::from_utf8_lossy(&bytes);
        let total = text.lines().count();
        let mut out = String::new();
        for (i, line) in text.lines().enumerate().skip(offset - 1).take(limit) {
            let line: String = if line.chars().count() > max_chars {
                line.chars().take(max_chars).collect::<String>() + "…"
            } else {
                line.to_string()
            };
            if compact {
                // No padding: a few tokens per line add up over long files.
                out.push_str(&format!("{}\t{}\n", i + 1, line));
            } else {
                out.push_str(&format!("{:>6}\t{}\n", i + 1, line));
            }
        }
        if total == 0 {
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
        let path = match str_arg(&input, "path").and_then(|p| ctx.resolve(p)) {
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
        match tokio::fs::write(&path, content).await {
            Ok(()) => ToolOutcome::ok(format!(
                "Wrote {} ({} lines)",
                path.display(),
                content.lines().count()
            )),
            Err(e) => ToolOutcome::err(format!("{}: {e}", path.display())),
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
        let path = match str_arg(&input, "path").and_then(|p| ctx.resolve(p)) {
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
        let text = match tokio::fs::read_to_string(&path).await {
            Ok(t) => t,
            Err(e) => return ToolOutcome::err(format!("{}: {e}", path.display())),
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
        match tokio::fs::write(&path, updated).await {
            Ok(()) => ToolOutcome::ok(format!(
                "Edited {}: {count} replacement(s), first at line {first_line}",
                path.display()
            )),
            Err(e) => ToolOutcome::err(format!("{}: {e}", path.display())),
        }
    }
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
}
