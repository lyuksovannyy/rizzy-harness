//! `outline`: a source file's definitions (signature lines with line
//! numbers) without bodies, so the model can find what to read.

use harness_core::policy::ToolRisk;
use serde_json::{Value, json};

use crate::{Tool, ToolCtx, ToolOutcome, str_arg};

pub struct Outline;

#[async_trait::async_trait]
impl Tool for Outline {
    fn name(&self) -> &'static str {
        "outline"
    }

    fn description(&self) -> &'static str {
        "List a source file's definitions (functions, types, methods) with line numbers, \
         without their bodies. Much shorter than reading the file; then `read` the lines \
         you need. Rust, Python, JavaScript, TypeScript, Go."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn enabled(&self, s: &harness_core::savings::Savings) -> bool {
        s.outline_tool
    }

    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let path = match str_arg(&input, "path").and_then(|p| ctx.resolve(p)) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::err(e),
        };
        if !harness_codemap::supported(&path) {
            return ToolOutcome::err(format!(
                "{}: outline supports .rs .py .js .ts .tsx .go; use `read`",
                path.display()
            ));
        }
        let text = match tokio::fs::read_to_string(&path).await {
            Ok(t) => t,
            Err(e) => return ToolOutcome::err(format!("{}: {e}", path.display())),
        };
        let lines = text.lines().count();
        match tokio::task::spawn_blocking(move || harness_codemap::outline(&path, &text)).await {
            Ok(Some(o)) => ToolOutcome::ok(format!("{o}[{lines} lines in the file]")),
            _ => ToolOutcome::err("could not parse the file; use `read`"),
        }
    }
}
