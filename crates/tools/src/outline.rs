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
        // Regular files of bounded size only: a FIFO named `x.rs` must not
        // block the call, nor a huge file take the memory (AUDIT S10).
        let p = path.clone();
        let text = match tokio::task::spawn_blocking(move || {
            crate::read_small(&p, crate::MAX_GREP_FILE_BYTES)
        })
        .await
        {
            Ok(Ok(b)) => String::from_utf8_lossy(&b).into_owned(),
            Ok(Err(e)) => return ToolOutcome::err(e),
            Err(e) => return ToolOutcome::err(format!("outline failed: {e}")),
        };
        if ctx.cancel.is_cancelled() {
            return ToolOutcome::err("[cancelled]");
        }
        let lines = text.lines().count();
        match tokio::task::spawn_blocking(move || harness_codemap::outline(&path, &text)).await {
            Ok(Some(o)) => ToolOutcome::ok(format!("{o}[{lines} lines in the file]")),
            _ => ToolOutcome::err("could not parse the file; use `read`"),
        }
    }
}
