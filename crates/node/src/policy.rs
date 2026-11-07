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
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("cannot read {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("invalid {0}: {1}")]
    Parse(PathBuf, toml::de::Error),
}

impl NodePolicy {
    /// Load `node.toml`; a missing file means defaults.
    pub fn load(path: &Path) -> Result<Self, PolicyError> {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).map_err(|e| PolicyError::Parse(path.into(), e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(PolicyError::Io(path.into(), e)),
        }
    }

    /// Validate and canonicalize a requested session cwd.
    pub fn check_cwd(&self, cwd: &str) -> Result<PathBuf, String> {
        let p = Path::new(cwd)
            .canonicalize()
            .map_err(|e| format!("cwd {cwd}: {e}"))?;
        if !p.is_dir() {
            return Err(format!("cwd {cwd} is not a directory"));
        }
        let ok = self.workspace_roots.iter().any(|root| {
            root.canonicalize()
                .map(|r| p.starts_with(r))
                .unwrap_or(false)
        });
        if ok {
            Ok(p)
        } else {
            Err(format!(
                "cwd {} is outside this node's workspace_roots",
                p.display()
            ))
        }
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
        assert!(NodePolicy::load(&dir.path().join("missing.toml")).is_ok());
    }
}
