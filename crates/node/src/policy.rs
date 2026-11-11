//! Node-local policy (`node.toml`). Authoritative: nothing the master sends
//! can loosen it (PLAN.md T5).

use std::path::{Path, PathBuf};

use harness_core::policy::ApprovalMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NodePolicy {
    /// The loosest approval mode any session on this node may use.
    pub max_approval_mode: ApprovalMode,
    /// Session working directories must be inside one of these.
    pub workspace_roots: Vec<PathBuf>,
    /// Tools that are always refused.
    pub denied_tools: Vec<String>,
    /// Allow file tools to touch paths outside the session workspace.
    pub allow_outside_workspace: bool,
    /// File checkpoints kept per working folder for undo/rewind (0 = off).
    pub checkpoints_keep: usize,
    /// Let agents query a graphify knowledge graph (runs the third-party
    /// `graphify` program here; install it with `rizzy-harness graphify install`).
    pub graphify: bool,
    /// Publishing files as artifacts (`[artifacts]`).
    pub artifacts: ArtifactsPolicy,
    /// Variables passed to tool child processes on top of the core list
    /// (`PATH`, `HOME`, `LANG`, ...): names or globs such as `"CARGO_*"`.
    /// Provider keys, `.env` names and `HARNESS_*` are never passed.
    pub env_pass: Vec<String>,
    /// Resource limits (`[limits]`).
    pub limits: LimitsPolicy,
    /// The shell sandbox (`[sandbox]`, FIX_PLAN 2.7).
    pub sandbox: SandboxPolicy,
    /// Folders no session may include and no file tool may touch, in every
    /// mode: the runtime's own data directory (keys, the API token,
    /// databases). Set by the runtime with [`NodePolicy::protect`], never
    /// read from node.toml. Canonical paths.
    #[serde(skip)]
    pub protected_dirs: Vec<PathBuf>,
    /// Set by the runtime when `[sandbox] mode = "required"` and the sandbox
    /// does not work here: every agentic session is refused with this text
    /// (at create, before every turn and restore). Never read from node.toml.
    #[serde(skip)]
    pub sandbox_refusal: Option<String>,
    /// Where the runtime keeps per-session temp folders (removed with the
    /// session). Never read from node.toml.
    #[serde(skip)]
    pub sandbox_tmp: Option<PathBuf>,
}

/// `[sandbox]` in node.toml: what the agent's shell (and graphify) may
/// touch. Nothing the master sends can change it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SandboxPolicy {
    /// `"required"` (default: refuse agentic sessions where Landlock does
    /// not work), `"best-effort"` or `"off"`.
    pub mode: harness_tools::sandbox::Mode,
    /// `"on"` (default) or `"off"`: deny TCP connect and bind.
    pub network: harness_tools::sandbox::Network,
    /// More read-only folders (toolchains: `~/.cargo`, `~/.rustup`, ...).
    /// `~/` is the node user's home.
    pub read: Vec<PathBuf>,
    /// More read-write folders (caches: `~/.cache`, `~/.npm`, ...).
    pub write: Vec<PathBuf>,
}

impl SandboxPolicy {
    /// The settings with `~/` expanded; relative entries are an error.
    pub fn settings(&self) -> Result<harness_tools::sandbox::Settings, String> {
        let home = dirs::home_dir();
        let expand = |key: &str, list: &[PathBuf]| -> Result<Vec<PathBuf>, String> {
            list.iter()
                .map(|p| {
                    let p = match (p.strip_prefix("~"), &home) {
                        (Ok(rest), Some(h)) => h.join(rest),
                        (Ok(_), None) => {
                            return Err(format!(
                                "[sandbox] {key}: {} uses ~ but there is no home directory",
                                p.display()
                            ));
                        }
                        (Err(_), _) => p.clone(),
                    };
                    if p.is_absolute() {
                        Ok(p)
                    } else {
                        Err(format!(
                            "[sandbox] {key}: {} must be absolute (or start with ~/)",
                            p.display()
                        ))
                    }
                })
                .collect()
        };
        Ok(harness_tools::sandbox::Settings {
            mode: self.mode,
            network: self.network,
            read: expand("read", &self.read)?,
            write: expand("write", &self.write)?,
        })
    }
}

/// `[artifacts]` in node.toml: the node's own switch and limits for files
/// published from it (docs/artifacts.md). Nothing the master sends can
/// loosen them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ArtifactsPolicy {
    /// Off: no `publish` tool, no "Save as artifact", no capability.
    pub enabled: bool,
    /// One published file or folder (uncompressed), in MB.
    pub max_mb: u64,
    /// Every staged file together; the oldest go first beyond it.
    pub stage_max_mb: u64,
    /// A stage the master never fetched is deleted after this long.
    pub stage_ttl_hours: u64,
    /// More names never published (globs: `*`, `?`, `**`), on top of the
    /// built-in secret-like names.
    pub deny_globs: Vec<String>,
}

impl Default for ArtifactsPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_mb: 200,
            stage_max_mb: 2048,
            stage_ttl_hours: 24,
            deny_globs: Vec::new(),
        }
    }
}

/// `[limits]` in node.toml.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LimitsPolicy {
    /// Bytes one tool call may stream live (stored as events and sent to
    /// the master); the rest is read and dropped after one marker.
    pub tool_stream_bytes: usize,
    /// Model calls one turn may make. Each sub-agent's turn has the same cap
    /// of its own; the caps do not add up into one budget.
    pub max_steps: usize,
    /// The longest a `bash` call may run, in seconds, whatever the model
    /// asks for.
    pub bash_timeout_max_s: u64,
    /// Sub-agents one turn may start (`delegate` calls run). The master's
    /// agent spec may ask for fewer, never more (Gate 3 review P-2).
    pub max_subagents: u32,
    /// How deep delegation may nest: 1 lets a turn's agent delegate and
    /// its sub-agents not; 0 turns delegation off on this node.
    pub max_delegation_depth: u32,
}

/// The most `[limits] max_subagents` and `max_delegation_depth` may say.
pub const MAX_SUBAGENTS: u32 = 32;
pub const MAX_DELEGATION_DEPTH: u32 = 4;

impl Default for LimitsPolicy {
    fn default() -> Self {
        Self {
            tool_stream_bytes: harness_tools::stream::DEFAULT_STREAM_BYTES,
            max_steps: harness_core::policy::DEFAULT_MAX_STEPS,
            bash_timeout_max_s: harness_tools::DEFAULT_TIMEOUT_MAX_S,
            // Today's values: the default caps, and one level (the master
            // resolves a sub-agent's own delegates to none).
            max_subagents: harness_core::agent::DelegateCaps::default().max_subagents,
            max_delegation_depth: 1,
        }
    }
}

impl LimitsPolicy {
    /// The master's agent spec within this node's delegation bounds: its
    /// per-turn sub-agent cap no higher than `max_subagents`, at every
    /// level, and no delegates below `max_delegation_depth`.
    pub fn bound_delegation(
        &self,
        mut agent: harness_core::agent::AgentSpec,
    ) -> harness_core::agent::AgentSpec {
        self.bound_at(&mut agent, 0);
        agent
    }

    fn bound_at(&self, agent: &mut harness_core::agent::AgentSpec, depth: u32) {
        if depth >= self.max_delegation_depth || self.max_subagents == 0 {
            agent.delegates.clear();
        }
        let mut caps = agent.caps.unwrap_or_default();
        caps.max_subagents = caps.max_subagents.min(self.max_subagents);
        agent.caps = Some(caps);
        for d in &mut agent.delegates {
            self.bound_at(d, depth + 1);
        }
    }
}

impl Default for NodePolicy {
    fn default() -> Self {
        Self {
            max_approval_mode: ApprovalMode::Ask,
            workspace_roots: dirs::home_dir().into_iter().collect(),
            denied_tools: Vec::new(),
            allow_outside_workspace: false,
            checkpoints_keep: 10,
            graphify: false,
            artifacts: ArtifactsPolicy::default(),
            env_pass: Vec::new(),
            limits: LimitsPolicy::default(),
            sandbox: SandboxPolicy::default(),
            protected_dirs: Vec::new(),
            sandbox_refusal: None,
            sandbox_tmp: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("cannot read {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("invalid {0}: {1}")]
    Parse(PathBuf, toml::de::Error),
    #[error("invalid {0}: {1}")]
    Invalid(PathBuf, String),
}

impl NodePolicy {
    /// Load `node.toml`; a missing file means defaults.
    pub fn load(path: &Path) -> Result<Self, PolicyError> {
        let p: Self = match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).map_err(|e| PolicyError::Parse(path.into(), e))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(PolicyError::Io(path.into(), e)),
        };
        p.validate()
            .map_err(|e| PolicyError::Invalid(path.into(), e))?;
        Ok(p)
    }

    /// Values serde cannot check: `env_pass` globs, `[limits]` ranges.
    pub fn validate(&self) -> Result<(), String> {
        harness_tools::env::compile_pass(&self.env_pass)?;
        let n = self.limits.tool_stream_bytes;
        let (min, max) = (
            harness_tools::stream::MIN_STREAM_BYTES,
            harness_tools::stream::MAX_STREAM_BYTES,
        );
        if !(min..=max).contains(&n) {
            return Err(format!(
                "[limits] tool_stream_bytes = {n}: must be between {min} and {max}"
            ));
        }
        let n = self.limits.max_steps;
        let (min, max) = (
            harness_core::policy::MIN_MAX_STEPS,
            harness_core::policy::MAX_MAX_STEPS,
        );
        if !(min..=max).contains(&n) {
            return Err(format!(
                "[limits] max_steps = {n}: must be between {min} and {max}"
            ));
        }
        let n = self.limits.bash_timeout_max_s;
        let (min, max) = (
            harness_tools::MIN_TIMEOUT_MAX_S,
            harness_tools::MAX_TIMEOUT_MAX_S,
        );
        if !(min..=max).contains(&n) {
            return Err(format!(
                "[limits] bash_timeout_max_s = {n}: must be between {min} and {max}"
            ));
        }
        let n = self.limits.max_subagents;
        if n > MAX_SUBAGENTS {
            return Err(format!(
                "[limits] max_subagents = {n}: must be between 0 and {MAX_SUBAGENTS}"
            ));
        }
        let n = self.limits.max_delegation_depth;
        if n > MAX_DELEGATION_DEPTH {
            return Err(format!(
                "[limits] max_delegation_depth = {n}: must be between 0 and {MAX_DELEGATION_DEPTH}"
            ));
        }
        self.sandbox.settings()?;
        Ok(())
    }

    /// The environment tool child processes get here: the core list, plus
    /// `env_pass`, minus `deny` (names that hold secrets).
    pub fn tool_env(
        &self,
        deny: impl IntoIterator<Item = String>,
    ) -> Result<harness_tools::ToolEnv, String> {
        harness_tools::ToolEnv::new(&self.env_pass, deny)
    }

    /// Validate and canonicalize a requested session cwd.
    pub fn check_cwd(&self, cwd: &str) -> Result<PathBuf, String> {
        if let Some(why) = &self.sandbox_refusal {
            return Err(why.clone());
        }
        let p = Path::new(cwd)
            .canonicalize()
            .map_err(|e| format!("cwd {cwd}: {e}"))?;
        if !p.is_dir() {
            return Err(format!("cwd {cwd} is not a directory"));
        }
        self.check_protected(&p)?;
        self.check_roots(&p)?;
        Ok(p)
    }

    /// The workspace root (canonical) that holds `dir` (already canonical),
    /// or a refusal naming `workspace_roots`.
    pub fn root_of(&self, dir: &Path) -> Result<PathBuf, String> {
        self.workspace_roots
            .iter()
            .filter_map(|root| root.canonicalize().ok())
            .filter(|r| dir.starts_with(r))
            // The innermost root: the walk for a repository stops there.
            .max_by_key(|r| r.components().count())
            .ok_or_else(|| {
                format!(
                    "cwd {} is outside the folders this node allows (workspace_roots in \
                     node.toml)",
                    dir.display()
                )
            })
    }

    fn check_roots(&self, dir: &Path) -> Result<(), String> {
        self.root_of(dir).map(|_| ())
    }

    /// What is checked again before every turn, restore, delegation and
    /// publish of an agentic session (`cwd` is its folder as the pin
    /// checked it): the sandbox still works here (see
    /// [`Self::sandbox_refusal`]), the folder does not include a protected
    /// one, and it is still inside `workspace_roots` (node.toml may have
    /// been narrowed since the session was made).
    pub fn check_session(&self, cwd: &Path) -> Result<(), String> {
        if let Some(why) = &self.sandbox_refusal {
            return Err(why.clone());
        }
        self.check_protected(cwd)?;
        let real = cwd
            .canonicalize()
            .map_err(|e| format!("cwd {}: {e}", cwd.display()))?;
        self.check_roots(&real)
    }

    /// Add `dir` to [`Self::protected_dirs`] (canonicalized; a folder that
    /// does not exist yet is kept as given, made absolute).
    pub fn protect(&mut self, dir: &Path) {
        let d = dir
            .canonicalize()
            .unwrap_or_else(|_| std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf()));
        if !self.protected_dirs.contains(&d) {
            self.protected_dirs.push(d);
        }
    }

    /// Refuse a session folder that is a protected folder, inside one, or
    /// contains one (an ancestor: `$HOME` holds `~/.local/share/...`). The
    /// folder is canonicalized here, so a link or `..` is judged by where it
    /// leads; one that cannot be resolved is refused.
    pub fn check_protected(&self, cwd: &Path) -> Result<(), String> {
        if self.protected_dirs.is_empty() {
            return Ok(());
        }
        let p = cwd
            .canonicalize()
            .map_err(|e| format!("cwd {}: {e}", cwd.display()))?;
        for d in &self.protected_dirs {
            // Compare with where it is now, too: it may have been replaced
            // by a link since start.
            let now = d.canonicalize().unwrap_or_else(|_| d.clone());
            for d in [d, &now] {
                if p.starts_with(d) {
                    return Err(format!(
                        "cwd {} is inside the harness's own data directory {}; choose a \
                         project folder outside it",
                        p.display(),
                        d.display()
                    ));
                }
                if d.starts_with(&p) {
                    return Err(format!(
                        "cwd {} cannot be used: choose a project folder; this one contains the \
                         harness's own data directory {}",
                        p.display(),
                        d.display()
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn effective_mode(&self, requested: Option<ApprovalMode>) -> ApprovalMode {
        ApprovalMode::clamp(
            requested.unwrap_or(ApprovalMode::Ask),
            self.max_approval_mode,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gate 3 review P-2: node.toml bounds what the master's spec asks for
    /// delegation, at every level; it never raises it.
    #[test]
    fn delegation_is_bounded_by_the_node_at_every_level() {
        use harness_core::agent::{AgentSpec, DelegateCaps};
        let spec = |id: &str, delegates: Vec<AgentSpec>, caps: Option<DelegateCaps>| AgentSpec {
            agent_id: id.into(),
            name: id.into(),
            system_prompt: String::new(),
            tools: harness_core::agent::ToolSelection::List(vec![]),
            model: None,
            fallbacks: vec![],
            unavailable: vec![],
            no_effort: vec![],
            context_windows: Default::default(),
            effort: None,
            budget: Default::default(),
            ui: Default::default(),
            prompt_hash: "h".into(),
            deny_tools: vec![],
            delegates,
            caps,
        };
        let caps = |n| {
            Some(DelegateCaps {
                max_subagents: n,
                max_tokens: 1,
            })
        };
        // A spec nested three deep, asking for 10 per turn.
        let deep = spec(
            "a",
            vec![spec("b", vec![spec("c", vec![], caps(10))], caps(10))],
            caps(10),
        );
        let l = LimitsPolicy::default();
        assert_eq!((l.max_subagents, l.max_delegation_depth), (4, 1));
        let got = l.bound_delegation(deep.clone());
        assert_eq!(got.caps.unwrap().max_subagents, 4);
        assert_eq!(got.delegates.len(), 1);
        assert!(got.delegates[0].delegates.is_empty(), "one level only");
        // Fewer than the node allows stays as asked.
        assert_eq!(
            l.bound_delegation(spec("a", vec![], caps(2)))
                .caps
                .unwrap()
                .max_subagents,
            2
        );
        let two = LimitsPolicy {
            max_delegation_depth: 2,
            ..LimitsPolicy::default()
        };
        let got = two.bound_delegation(deep.clone());
        assert_eq!(got.delegates[0].delegates.len(), 1);
        assert!(got.delegates[0].delegates[0].delegates.is_empty());
        assert_eq!(got.delegates[0].caps.unwrap().max_subagents, 4);
        for off in [
            LimitsPolicy {
                max_delegation_depth: 0,
                ..LimitsPolicy::default()
            },
            LimitsPolicy {
                max_subagents: 0,
                ..LimitsPolicy::default()
            },
        ] {
            assert!(off.bound_delegation(deep.clone()).delegates.is_empty());
        }
        let mut p = NodePolicy::default();
        p.limits.max_subagents = MAX_SUBAGENTS + 1;
        assert!(p.validate().unwrap_err().contains("max_subagents"));
        p.limits.max_subagents = 0;
        p.limits.max_delegation_depth = MAX_DELEGATION_DEPTH + 1;
        assert!(p.validate().unwrap_err().contains("max_delegation_depth"));
    }

    #[test]
    fn master_cannot_loosen() {
        let p = NodePolicy::default();
        assert_eq!(
            p.effective_mode(Some(ApprovalMode::Auto)),
            ApprovalMode::Ask
        );
        assert_eq!(p.effective_mode(None), ApprovalMode::Ask);
        let loose = NodePolicy {
            max_approval_mode: ApprovalMode::Auto,
            ..Default::default()
        };
        // Even a permissive node defaults to asking.
        assert_eq!(loose.effective_mode(None), ApprovalMode::Ask);
        assert_eq!(
            loose.effective_mode(Some(ApprovalMode::Auto)),
            ApprovalMode::Auto
        );
    }

    #[test]
    fn cwd_confinement() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("proj")).unwrap();
        let p = NodePolicy {
            workspace_roots: vec![root.path().into()],
            ..Default::default()
        };
        assert!(
            p.check_cwd(root.path().join("proj").to_str().unwrap())
                .is_ok()
        );
        assert!(p.check_cwd(other.path().to_str().unwrap()).is_err());
        assert!(p.check_cwd("/definitely/missing").is_err());
    }

    /// The data directory holds the API token, the session keys and every
    /// user's chats: no session may include it (AUDIT S1).
    #[test]
    fn cwd_never_includes_the_data_dir() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path().canonicalize().unwrap();
        let local = home.join(".local");
        let data = local.join("share/rizzy-harness");
        std::fs::create_dir_all(data.join("keys")).unwrap();
        std::fs::create_dir_all(home.join("projects/x")).unwrap();
        let mut p = NodePolicy {
            workspace_roots: vec![home.clone()],
            ..Default::default()
        };
        p.protect(&data);
        let s = |p: &Path| p.to_str().unwrap().to_string();
        for bad in [&home, &local, &data, &data.join("keys")] {
            let e = p.check_cwd(&s(bad)).unwrap_err();
            assert!(e.contains("choose a project folder"), "{e}");
            assert!(e.contains(&s(&data)), "{e}");
        }
        assert!(p.check_cwd(&s(&home.join("projects/x"))).is_ok());
        assert!(p.check_cwd(&s(&home.join("projects"))).is_ok());
        // A link that leads into it is judged by where it leads.
        std::os::unix::fs::symlink(&data, home.join("projects/data")).unwrap();
        assert!(p.check_cwd(&s(&home.join("projects/data"))).is_err());
        std::os::unix::fs::symlink(&home, home.join("projects/up")).unwrap();
        assert!(p.check_cwd(&s(&home.join("projects/up"))).is_err());
        // A `..` path is no way around it either.
        assert!(p.check_cwd(&s(&home.join("projects/x/../.."))).is_err());
        // Nothing in node.toml can set or clear the list.
        assert!(toml::from_str::<NodePolicy>("protected_dirs = []\n").is_err());
    }

    #[test]
    fn sandbox_settings_parse_and_validate() {
        use harness_tools::sandbox::{Mode, Network};
        let d = NodePolicy::default();
        assert_eq!(d.sandbox.mode, Mode::Required, "fail closed by default");
        assert_eq!(d.sandbox.network, Network::On, "Q6: on by default");
        let p: NodePolicy = toml::from_str(
            "[sandbox]\nmode = \"best-effort\"\nnetwork = \"off\"\n\
             read = [\"~/.cargo\", \"/opt/x\"]\nwrite = [\"~/.cache\"]\n",
        )
        .unwrap();
        assert_eq!(p.sandbox.mode, Mode::BestEffort);
        assert_eq!(p.sandbox.network, Network::Off);
        let s = p.sandbox.settings().unwrap();
        let home = dirs::home_dir().unwrap();
        assert_eq!(s.read, [home.join(".cargo"), PathBuf::from("/opt/x")]);
        assert_eq!(s.write, [home.join(".cache")]);
        for bad in [
            "[sandbox]\nmode = \"maybe\"\n",
            "[sandbox]\nnetwork = \"udp\"\n",
            "[sandbox]\nreads = []\n",
            "sandbox = \"off\"\n",
        ] {
            assert!(toml::from_str::<NodePolicy>(bad).is_err(), "{bad}");
        }
        let p: NodePolicy = toml::from_str("[sandbox]\nread = [\"relative/dir\"]\n").unwrap();
        assert!(p.validate().unwrap_err().contains("absolute"));
        // The runtime-only fields cannot come from node.toml.
        assert!(toml::from_str::<NodePolicy>("sandbox_refusal = \"x\"\n").is_err());
        assert!(toml::from_str::<NodePolicy>("sandbox_tmp = \"/x\"\n").is_err());
    }

    /// Without a working sandbox (mode `required`), no agentic session is
    /// created, continued or restored.
    #[test]
    fn a_sandbox_refusal_refuses_every_session() {
        let root = tempfile::tempdir().unwrap();
        let mut p = NodePolicy {
            workspace_roots: vec![root.path().into()],
            ..Default::default()
        };
        assert!(p.check_cwd(root.path().to_str().unwrap()).is_ok());
        assert!(p.check_session(root.path()).is_ok());
        p.sandbox_refusal = Some("set [sandbox] mode in node.toml".into());
        let e = p.check_cwd(root.path().to_str().unwrap()).unwrap_err();
        assert!(e.contains("[sandbox] mode"), "{e}");
        assert!(p.check_session(root.path()).is_err());
    }

    #[test]
    fn parses_toml_and_rejects_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("node.toml");
        std::fs::write(
            &f,
            "max_approval_mode = \"auto\"\ndenied_tools = [\"bash\"]\n",
        )
        .unwrap();
        let p = NodePolicy::load(&f).unwrap();
        assert_eq!(p.max_approval_mode, ApprovalMode::Auto);
        assert_eq!(p.denied_tools, vec!["bash".to_string()]);
        std::fs::write(&f, "max_aproval_mode = \"auto\"\n").unwrap();
        assert!(NodePolicy::load(&f).is_err());
        std::fs::write(&f, "[artifacts]\nmax_mb = 5\ndeny_globs = [\"*.sqlite\"]\n").unwrap();
        let p = NodePolicy::load(&f).unwrap();
        assert!(p.artifacts.enabled, "unset keys keep their defaults");
        assert_eq!(p.artifacts.max_mb, 5);
        assert_eq!(p.artifacts.stage_ttl_hours, 24);
        std::fs::write(&f, "[artifacts]\nmax_megabytes = 5\n").unwrap();
        assert!(NodePolicy::load(&f).is_err());
        assert!(NodePolicy::load(&dir.path().join("missing.toml")).is_ok());
    }

    #[test]
    fn env_pass_and_limits_are_validated_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("node.toml");
        let d = NodePolicy::load(&f).unwrap();
        assert!(d.env_pass.is_empty(), "nothing extra passes by default");
        assert_eq!(d.limits.tool_stream_bytes, 1 << 20);
        std::fs::write(
            &f,
            "env_pass = [\"CARGO_*\", \"NVM_DIR\"]\n[limits]\ntool_stream_bytes = 65536\n",
        )
        .unwrap();
        let p = NodePolicy::load(&f).unwrap();
        assert_eq!(p.env_pass, ["CARGO_*", "NVM_DIR"]);
        assert_eq!(p.limits.tool_stream_bytes, 65536);
        for bad in [
            "env_pass = [\"CARGO_[\"]\n",
            "env_pass = [\"A=B\"]\n",
            "env_pass = [\"\"]\n",
            "[limits]\ntool_stream_bytes = 10\n",
            "[limits]\nmax_steps = 0\n",
            "[limits]\nmax_steps = 1001\n",
            "[limits]\nbash_timeout_max_s = 0\n",
            "[limits]\nbash_timeout_max_s = 3601\n",
            "[limits]\ntool_bytes = 65536\n",
        ] {
            std::fs::write(&f, bad).unwrap();
            let e = NodePolicy::load(&f).unwrap_err().to_string();
            assert!(e.contains("node.toml"), "{bad}: {e}");
        }
        std::fs::write(&f, "env_pass = [\"CARGO_[\"]\n").unwrap();
        let e = NodePolicy::load(&f).unwrap_err().to_string();
        assert!(e.contains("env_pass") && e.contains("CARGO_["), "{e}");
    }

    #[test]
    fn loop_limits_default_to_todays_values_and_stop_the_node_out_of_range() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("node.toml");
        let d = NodePolicy::load(&f).unwrap();
        assert_eq!(d.limits.max_steps, 50);
        assert_eq!(d.limits.bash_timeout_max_s, 3600);
        std::fs::write(&f, "[limits]\nmax_steps = 2\nbash_timeout_max_s = 1\n").unwrap();
        let p = NodePolicy::load(&f).unwrap();
        assert_eq!((p.limits.max_steps, p.limits.bash_timeout_max_s), (2, 1));
        std::fs::write(
            &f,
            "[limits]\nmax_steps = 1000\nbash_timeout_max_s = 3600\n",
        )
        .unwrap();
        assert!(NodePolicy::load(&f).is_ok(), "the range ends are valid");
        for (bad, key, range) in [
            ("max_steps = 0", "max_steps", "1 and 1000"),
            ("max_steps = 1001", "max_steps", "1 and 1000"),
            ("bash_timeout_max_s = 0", "bash_timeout_max_s", "1 and 3600"),
            (
                "bash_timeout_max_s = 3601",
                "bash_timeout_max_s",
                "1 and 3600",
            ),
        ] {
            std::fs::write(&f, format!("[limits]\n{bad}\n")).unwrap();
            let e = NodePolicy::load(&f).unwrap_err().to_string();
            assert!(e.contains(key) && e.contains(range), "{bad}: {e}");
        }
    }
}
