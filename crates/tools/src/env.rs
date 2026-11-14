//! The environment of every child process a tool starts (PLAN.md T3).
//!
//! An allow-list, not a deny-list: the child starts from an empty
//! environment and gets only [`CORE_VARS`] (and `LC_*`) plus the names the
//! node's `env_pass` matches. Whatever else the server was started with
//! (cloud credentials, tokens, an SSH agent socket) never reaches the agent.
//! A second gate removes the deny-list (provider key variables, names a
//! `.env` file set, every `HARNESS_*` setting) even when `env_pass` matches
//! them, so an operator cannot pass a key through with a broad glob.

use std::ffi::OsString;
use std::sync::Arc;

use globset::{Glob, GlobSet, GlobSetBuilder};

/// Passed to every child process: what ordinary commands need to run.
pub const CORE_VARS: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LANGUAGE",
    "TERM",
    "COLORTERM",
    "TZ",
    "TMPDIR",
];

/// Prefixes of passed names (locale categories).
pub const CORE_PREFIXES: &[&str] = &["LC_"];

/// Prefixes that are never passed, whatever `env_pass` says: the server's
/// own settings and keys (`HARNESS_PROVIDER_API_KEY`, `HARNESS_JOIN_TOKEN`).
pub const DENY_PREFIXES: &[&str] = &["HARNESS_"];

/// Which variables tool child processes get. Cheap to clone.
#[derive(Clone)]
pub struct ToolEnv {
    inner: Arc<Inner>,
}

struct Inner {
    pass: Vec<String>,
    set: GlobSet,
    deny: Vec<String>,
}

impl Default for ToolEnv {
    /// The core list only.
    fn default() -> Self {
        Self {
            inner: Arc::new(Inner {
                pass: Vec::new(),
                set: GlobSet::empty(),
                deny: Vec::new(),
            }),
        }
    }
}

impl std::fmt::Debug for ToolEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolEnv")
            .field("pass", &self.inner.pass)
            .field("deny", &self.inner.deny)
            .finish()
    }
}

/// Check and compile `env_pass` (names or globs: `*`, `?`, `[...]`).
pub fn compile_pass(pass: &[String]) -> Result<GlobSet, String> {
    let mut b = GlobSetBuilder::new();
    for p in pass {
        if p.is_empty() {
            return Err("env_pass: an entry is empty".into());
        }
        if let Some(c) = p
            .chars()
            .find(|c| *c == '=' || *c == '/' || c.is_whitespace() || c.is_control())
        {
            return Err(format!(
                "env_pass: {p:?} contains {c:?}; entries are variable names or globs such as \
                 \"CARGO_*\""
            ));
        }
        let g = Glob::new(p).map_err(|e| format!("env_pass: {p:?} is not a valid glob: {e}"))?;
        b.add(g);
    }
    b.build().map_err(|e| format!("env_pass: {e}"))
}

impl ToolEnv {
    /// `pass` is node.toml's `env_pass`; `deny` are names never passed.
    pub fn new(pass: &[String], deny: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let set = compile_pass(pass)?;
        let mut deny: Vec<String> = deny.into_iter().collect();
        deny.sort();
        deny.dedup();
        Ok(Self {
            inner: Arc::new(Inner {
                pass: pass.to_vec(),
                set,
                deny,
            }),
        })
    }

    /// The same deny-list with another pass list: an MCP server's own
    /// `env_vars` (node.toml) in place of the node's `env_pass`.
    pub fn with_pass(&self, pass: &[String]) -> Result<Self, String> {
        Self::new(pass, self.inner.deny.iter().cloned())
    }

    /// node.toml's `env_pass`, as given.
    pub fn pass(&self) -> &[String] {
        &self.inner.pass
    }

    /// Whether a variable of this name is given to child processes.
    pub fn allows(&self, name: &str) -> bool {
        if DENY_PREFIXES.iter().any(|p| name.starts_with(p))
            || self.inner.deny.iter().any(|d| d == name)
        {
            return false;
        }
        CORE_VARS.contains(&name)
            || CORE_PREFIXES.iter().any(|p| name.starts_with(p))
            || self.inner.set.is_match(name)
    }

    /// The allowed subset of `vars`. Names that are not UTF-8 are dropped.
    pub fn filter(
        &self,
        vars: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Vec<(OsString, OsString)> {
        vars.into_iter()
            .filter(|(k, _)| k.to_str().is_some_and(|k| self.allows(k)))
            .collect()
    }

    /// The allowed subset of this process's environment.
    pub fn vars(&self) -> Vec<(OsString, OsString)> {
        self.filter(std::env::vars_os())
    }

    /// Give `cmd` exactly the allowed environment. Variables the caller sets
    /// on `cmd` after this are kept.
    pub fn apply(&self, cmd: &mut tokio::process::Command) {
        cmd.env_clear().envs(self.vars());
    }

    /// [`Self::apply`] for a `std::process::Command`.
    pub fn apply_std(&self, cmd: &mut std::process::Command) {
        cmd.env_clear().envs(self.vars());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(v: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        v.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
    }

    fn names(v: Vec<(OsString, OsString)>) -> Vec<String> {
        v.into_iter()
            .map(|(k, _)| k.into_string().unwrap())
            .collect()
    }

    #[test]
    fn core_list_only_by_default() {
        let env = ToolEnv::default();
        let got = names(env.filter(os(&[
            ("PATH", "/bin"),
            ("HOME", "/h"),
            ("LC_ALL", "C"),
            ("AWS_SECRET_ACCESS_KEY", "x"),
            ("GITHUB_TOKEN", "x"),
            ("SSH_AUTH_SOCK", "/tmp/s"),
        ])));
        assert_eq!(got, ["PATH", "HOME", "LC_ALL"]);
    }

    #[test]
    fn pass_names_and_globs() {
        let env = ToolEnv::new(&["SSH_AUTH_SOCK".into(), "CARGO_*".into()], []).unwrap();
        assert!(env.allows("SSH_AUTH_SOCK"));
        assert!(env.allows("CARGO_HOME"));
        assert!(!env.allows("CARGO"), "the glob needs the underscore");
        assert!(!env.allows("GITHUB_TOKEN"));
    }

    #[test]
    fn deny_wins_over_pass() {
        let env = ToolEnv::new(&["*".into()], ["OPENAI_API_KEY".to_string()]).unwrap();
        assert!(env.allows("ANYTHING"));
        assert!(!env.allows("OPENAI_API_KEY"));
        assert!(!env.allows("HARNESS_PROVIDER_API_KEY"));
        assert!(!env.allows("HARNESS_JOIN_TOKEN"));
        // Even a core name, if it is on the deny-list.
        let env = ToolEnv::new(&[], ["TERM".to_string()]).unwrap();
        assert!(!env.allows("TERM"));
    }

    #[test]
    fn invalid_entries_are_rejected() {
        for bad in ["", "CARGO_[", "A=B", "a b", "x/y"] {
            assert!(compile_pass(&[bad.into()]).is_err(), "{bad:?}");
        }
        assert!(compile_pass(&["NVM_DIR".into(), "*_PROXY".into(), "LC_?".into()]).is_ok());
    }
}
