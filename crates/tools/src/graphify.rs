//! Optional graphify integration (https://github.com/Graphify-Labs/graphify,
//! Apache-2.0): a knowledge graph of the repository the model can query
//! instead of reading many files.
//!
//! Off unless three things hold: the node allows it (`graphify = true` in
//! node.toml: it runs a third-party program on this machine), the program
//! is installed (`rizzy-harness graphify install`), and the admin's token-saving
//! settings turn it on. The graph is written under harness's data dir
//! (`GRAPHIFY_OUT`), never into the user's repository, and harness gives it no
//! API keys, so it only does its AST (no-LLM) extraction.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use harness_core::policy::ToolRisk;
use serde_json::{Value, json};
use tokio::process::Command;

use crate::{Tool, ToolCtx, ToolOutcome, str_arg};

/// The version installed by `rizzy-harness graphify install` (pinned: a newer
/// release is a new third-party program and should be a deliberate update).
pub const VERSION: &str = "0.9.102";
const PACKAGE: &str = "graphifyy";
/// Rebuild the graph when it is older than this.
const MAX_AGE: Duration = Duration::from_secs(3600);
const BUILD_TIMEOUT: Duration = Duration::from_secs(600);
const QUERY_TIMEOUT: Duration = Duration::from_secs(120);

/// Shared between the tool and whoever may switch it (the embedded node's
/// settings page, or node.toml at start).
pub struct GraphifyState {
    allowed: bool,
    out_root: PathBuf,
}

impl GraphifyState {
    pub fn new(allowed: bool, out_root: PathBuf) -> Arc<Self> {
        Arc::new(Self { allowed, out_root })
    }

    pub fn allowed(&self) -> bool {
        self.allowed
    }

    /// Where the graph for `workspace` lives (outside the repository).
    fn out_dir(&self, workspace: &Path) -> PathBuf {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&workspace, &mut h);
        self.out_root
            .join(format!("{:016x}", std::hash::Hasher::finish(&h)))
    }
}

/// The installed `graphify` program: on PATH, or where `uv tool` / `pipx`
/// put it.
pub fn find_binary() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("graphify"))
            .find(|p| p.is_file())
    });
    on_path.or_else(|| {
        let p = dirs_home()?.join(".local/bin/graphify");
        p.is_file().then_some(p)
    })
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// The installed version (`graphify --version`), if any.
pub async fn installed_version() -> Option<String> {
    let bin = find_binary()?;
    let out = Command::new(bin).arg("--version").output().await.ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Some(if s.is_empty() { "installed".into() } else { s })
}

/// Install the pinned version for the current user with `uv tool` or
/// `pipx` (whichever exists). Returns what was run and its output.
pub async fn install() -> Result<String, String> {
    let spec = format!("{PACKAGE}=={VERSION}");
    let candidates: [(&str, Vec<&str>); 2] = [
        ("uv", vec!["tool", "install", "--force", &spec]),
        ("pipx", vec!["install", "--force", &spec]),
    ];
    for (prog, args) in candidates {
        let ran = Command::new(prog).args(&args).output().await;
        match ran {
            Err(_) => continue, // not installed
            Ok(o) => {
                let text = format!(
                    "$ {prog} {}\n{}{}",
                    args.join(" "),
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                return if o.status.success() {
                    Ok(text)
                } else {
                    Err(text)
                };
            }
        }
    }
    Err(format!(
        "neither `uv` nor `pipx` is installed. Install one (e.g. `sudo apt install pipx`), \
         or run: python3 -m pip install --user {spec}"
    ))
}

pub struct GraphQuery {
    state: Arc<GraphifyState>,
}

impl GraphQuery {
    pub fn new(state: Arc<GraphifyState>) -> Self {
        Self { state }
    }
}

async fn run(
    ctx: &ToolCtx,
    bin: &Path,
    args: &[&str],
    out: &Path,
    timeout: Duration,
) -> Result<String, String> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .current_dir(&ctx.workspace)
        .env("GRAPHIFY_OUT", out)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    for k in &ctx.env_remove {
        cmd.env_remove(k);
    }
    let fut = cmd.output();
    let o = tokio::select! {
        r = tokio::time::timeout(timeout, fut) => match r {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return Err(format!("graphify: {e}")),
            Err(_) => return Err(format!("graphify timed out after {}s", timeout.as_secs())),
        },
        _ = ctx.cancel.cancelled() => return Err("[cancelled]".into()),
    };
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    if o.status.success() {
        Ok(text)
    } else {
        Err(format!(
            "graphify failed: {}{}",
            text,
            String::from_utf8_lossy(&o.stderr)
        ))
    }
}

#[async_trait::async_trait]
impl Tool for GraphQuery {
    fn name(&self) -> &'static str {
        "graph_query"
    }

    fn description(&self) -> &'static str {
        "Ask the repository's knowledge graph (graphify) a question, e.g. \"what calls \
         parse_config\" or \"how does auth reach the database\". Returns the relevant part of \
         the graph (symbols, files, relations) instead of whole files. The graph is built on \
         first use (can take a minute)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"question": {"type": "string"}},
            "required": ["question"]
        })
    }

    fn risk(&self) -> ToolRisk {
        ToolRisk::ReadOnly
    }

    fn enabled(&self, s: &harness_core::savings::Savings) -> bool {
        s.graphify && self.state.allowed() && find_binary().is_some()
    }

    async fn run(&self, ctx: &ToolCtx, input: Value) -> ToolOutcome {
        let q = match str_arg(&input, "question") {
            Ok(q) => q.trim(),
            Err(e) => return ToolOutcome::err(e),
        };
        if !self.state.allowed() {
            return ToolOutcome::err("graphify is not allowed on this node (node.toml)");
        }
        let Some(bin) = find_binary() else {
            return ToolOutcome::err("graphify is not installed on this node");
        };
        let out = self.state.out_dir(&ctx.workspace);
        let graph = out.join("graph.json");
        let fresh = std::fs::metadata(&graph)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < MAX_AGE);
        if !fresh {
            if let Err(e) = std::fs::create_dir_all(&out) {
                return ToolOutcome::err(format!("{}: {e}", out.display()));
            }
            if let Err(e) = run(ctx, &bin, &["update", "."], &out, BUILD_TIMEOUT).await {
                return ToolOutcome::err(e);
            }
        }
        let graph_s = graph.to_string_lossy().into_owned();
        match run(
            ctx,
            &bin,
            &["query", q, "--budget", "2000", "--graph", &graph_s],
            &out,
            QUERY_TIMEOUT,
        )
        .await
        {
            Ok(t) if t.trim().is_empty() => ToolOutcome::ok("(no match in the graph)"),
            Ok(t) => ToolOutcome::ok(t),
            Err(e) => ToolOutcome::err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_unless_allowed_and_enabled() {
        let s = GraphifyState::new(false, std::env::temp_dir());
        let t = GraphQuery::new(s.clone());
        let on = harness_core::savings::Savings {
            graphify: true,
            ..Default::default()
        };
        assert!(!t.enabled(&on), "node does not allow it");
        assert!(!t.enabled(&Default::default()));
        // Allowed in node.toml: still needs the binary; without it, off.
        let t = GraphQuery::new(GraphifyState::new(true, std::env::temp_dir()));
        assert_eq!(t.enabled(&on), find_binary().is_some());
    }

    #[test]
    fn graph_lives_outside_the_repo() {
        let s = GraphifyState::new(true, PathBuf::from("/data/graphify"));
        let a = s.out_dir(Path::new("/home/u/proj"));
        let b = s.out_dir(Path::new("/home/u/other"));
        assert!(a.starts_with("/data/graphify") && a != b);
    }
}

/// Runs the real program when installed (`rizzy-harness graphify install`); a
/// no-op otherwise. HARNESS_GRAPHIFY_TEST=1 makes it mandatory.
#[cfg(test)]
mod real {
    use super::*;

    #[tokio::test]
    async fn builds_outside_the_repo_and_answers() {
        if find_binary().is_none() {
            assert!(
                std::env::var("HARNESS_GRAPHIFY_TEST").is_err(),
                "graphify not installed"
            );
            return;
        }
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join("src")).unwrap();
        std::fs::write(
            repo.path().join("src/config.py"),
            "def parse_config(path):\n    return load(path)\n\ndef load(path):\n    return open(path).read()\n",
        )
        .unwrap();
        std::fs::write(
            repo.path().join("src/main.py"),
            "from config import parse_config\n\ndef main():\n    parse_config('a.toml')\n",
        )
        .unwrap();
        let data = tempfile::tempdir().unwrap();
        let t = GraphQuery::new(GraphifyState::new(true, data.path().join("graphify")));
        let mut ctx = ToolCtx::new(repo.path());
        ctx.savings.graphify = true;
        assert!(t.enabled(&ctx.savings));
        let out = t
            .run(&ctx, json!({"question": "what calls parse_config"}))
            .await;
        assert!(out.ok, "{}", out.output);
        assert!(out.output.contains("parse_config"), "{}", out.output);
        // Nothing written into the repository.
        let names: Vec<_> = std::fs::read_dir(repo.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, vec!["src".to_string()], "repo touched: {names:?}");
        assert!(
            std::fs::read_dir(data.path().join("graphify"))
                .unwrap()
                .count()
                == 1
        );
    }
}
