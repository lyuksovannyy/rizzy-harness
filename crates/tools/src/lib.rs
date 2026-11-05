//! Built-in tools: bash, read, write, edit, grep, glob.
//!
//! Every tool is cancellable through `ToolCtx::cancel`. Path-taking tools are
//! confined to the workspace unless the node policy says otherwise.

mod bash;
pub mod filter;
mod fs;
pub mod graphify;
mod outline;
pub mod outputs;
mod search;

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use harness_core::event::OutputStream;
use harness_core::policy::ToolRisk;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub use bash::Bash;
pub use fs::{Edit, Read, Write};
pub use outline::Outline;
pub use outputs::{OutputStore, ToolOutput};
pub use search::{Glob, Grep};

/// Cap on what the model sees from one tool call.
pub const MAX_OUTPUT_CHARS: usize = 30_000;

pub struct ToolCtx {
    pub workspace: PathBuf,
    pub cancel: CancellationToken,
    /// Live output chunks for the UI (bash only today).
    pub output: Option<mpsc::UnboundedSender<(OutputStream, String)>>,
    pub allow_outside_workspace: bool,
    pub timeout: Duration,
    /// Environment variables removed from child processes (secret holders).
    pub env_remove: Vec<String>,
    /// Text this conversation got from the user and from tools, never text
    /// the model wrote. Network tools only follow URLs found here, so injected
    /// instructions cannot make the model smuggle data out in a URL it builds.
    pub observed: Arc<Vec<String>>,
    /// Token-saving methods in effect for this call.
    pub savings: harness_core::savings::Savings,
    /// Where full outputs of filtered commands are kept (`tool_output`).
    pub outputs: Option<Arc<OutputStore>>,
}

impl ToolCtx {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            workspace: workspace.into(),
            cancel: CancellationToken::new(),
            output: None,
            allow_outside_workspace: false,
            timeout: Duration::from_secs(600),
            env_remove: Vec::new(),
            observed: Arc::default(),
            savings: Default::default(),
            outputs: None,
        }
    }

    /// Resolve a user/model-supplied path against the workspace and enforce
    /// confinement. Works for paths that do not exist yet.
    pub fn resolve(&self, p: &str) -> Result<PathBuf, String> {
        let raw = Path::new(p);
        let joined = if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            self.workspace.join(raw)
        };
        let normalized = normalize(&joined);
        if self.allow_outside_workspace {
            return Ok(normalized);
        }
        let ws = self
            .workspace
            .canonicalize()
            .map_err(|e| format!("workspace: {e}"))?;
        // Canonicalize the deepest existing ancestor to defeat symlink escapes.
        let mut existing = normalized.as_path();
        let mut rest = Vec::new();
        while !existing.exists() {
            match (existing.parent(), existing.file_name()) {
                (Some(parent), Some(name)) => {
                    rest.push(name.to_os_string());
                    existing = parent;
                }
                _ => break,
            }
        }
        let mut real = existing
            .canonicalize()
            .map_err(|e| format!("{}: {e}", existing.display()))?;
        for part in rest.iter().rev() {
            real.push(part);
        }
        if real.starts_with(&ws) {
            Ok(real)
        } else {
            Err(format!(
                "path {} is outside the session workspace {}",
                p,
                ws.display()
            ))
        }
    }
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutcome {
    pub ok: bool,
    pub output: String,
    pub exit_code: Option<i32>,
    pub truncated: bool,
}

impl ToolOutcome {
    pub fn ok(output: impl Into<String>) -> Self {
        let (output, truncated) = cap(output.into());
        Self {
            ok: true,
            output,
            exit_code: None,
            truncated,
        }
    }

    pub fn err(output: impl Into<String>) -> Self {
        let (output, truncated) = cap(output.into());
        Self {
            ok: false,
            output,
            exit_code: None,
            truncated,
        }
    }
}

/// Keep head and tail if over the cap.
pub fn cap(s: String) -> (String, bool) {
    if s.chars().count() <= MAX_OUTPUT_CHARS {
        return (s, false);
    }
    let half = MAX_OUTPUT_CHARS / 2;
    let head: String = s.chars().take(half).collect();
    let tail: String = {
        let v: Vec<char> = s.chars().rev().take(half).collect();
        v.into_iter().rev().collect()
    };
    (
        format!("{head}\n\n[... output truncated ...]\n\n{tail}"),
        true,
    )
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// JSON Schema of the input object.
    fn schema(&self) -> Value;
    fn risk(&self) -> ToolRisk;
    /// Offered to the model only when this returns true (tools that exist
    /// for a token-saving method are off with it).
    fn enabled(&self, _savings: &harness_core::savings::Savings) -> bool {
        true
    }
    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome;
}

pub type ToolSet = Vec<Arc<dyn Tool>>;

pub fn builtin() -> ToolSet {
    vec![
        Arc::new(Bash),
        Arc::new(Read),
        Arc::new(Write),
        Arc::new(Edit),
        Arc::new(Grep),
        Arc::new(Glob),
        Arc::new(ToolOutput),
        Arc::new(Outline),
    ]
}

pub(crate) fn str_arg<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing string argument `{key}`"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_confines_to_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ToolCtx::new(dir.path());
        assert!(ctx.resolve("a/b.txt").is_ok());
        assert!(ctx.resolve("../escape").is_err());
        assert!(ctx.resolve("/etc/passwd").is_err());
        let inside = dir.path().join("x");
        assert!(ctx.resolve(inside.to_str().unwrap()).is_ok());
    }

    #[test]
    fn resolve_blocks_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        let ctx = ToolCtx::new(dir.path());
        assert!(ctx.resolve("link/file").is_err());
    }

    #[test]
    fn cap_keeps_head_and_tail() {
        let s = "a".repeat(MAX_OUTPUT_CHARS) + &"b".repeat(100);
        let (out, t) = cap(s);
        assert!(t);
        assert!(out.starts_with('a') && out.ends_with('b'));
    }
}
