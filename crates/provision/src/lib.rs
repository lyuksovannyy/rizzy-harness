//! Setting up worker nodes over SSH (see PLAN.md "Node setup"). Two modes
//! share the same inspection and scripts: **automatic** runs them, with a
//! dead-man revert for anything that could lock the user out; **manual**
//! shows them to be run by hand. Passwords and user keys are used once to
//! install harness's own key and are never stored.

pub mod ssh;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ssh: {0}")]
    Ssh(String),
    #[error("login failed (wrong user, password or key)")]
    AuthFailed,
    #[error("the server's host key changed: expected {expected}, got {got}")]
    HostKeyChanged { expected: String, got: String },
    #[error("{0}")]
    Step(String),
}

pub type Result<T> = std::result::Result<T, Error>;
