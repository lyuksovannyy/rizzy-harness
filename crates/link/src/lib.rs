//! The master <-> remote node link (PLAN.md §3–4, T7/T8).
//!
//! * **Transport:** WebSocket over TLS on a dedicated port that speaks only
//!   this protocol. The master's TLS certificate is self-signed; nodes pin
//!   its SHA-256 fingerprint at join time (SSH-style), so no CA is trusted.
//! * **Node identity:** an Ed25519 key per node, pinned by the master at
//!   join. Every connection proves possession by signing a fresh nonce
//!   bound to the master's certificate fingerprint (a signature cannot be
//!   replayed against another master or relayed through a MITM).
//! * **Join:** a single-use, short-lived token an admin creates in the UI.
//!
//! Signing and TLS use aws-lc-rs (already used by rustls here).

pub mod client;
pub mod identity;
pub mod server;
pub mod tls;
pub mod wire;

pub use identity::NodeKey;
pub use tls::{MasterCert, fingerprint, pinned_client_config};
pub use wire::{Hello, ServerMsg};

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("tls: {0}")]
    Tls(String),
    #[error("websocket: {0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("key: {0}")]
    Key(String),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("rejected: {0}")]
    Rejected(String),
}

pub type Result<T> = std::result::Result<T, LinkError>;

/// Largest WebSocket message accepted on the link.
pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;

/// Write a file only the owner can read.
pub fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    o.mode(0o600);
    let mut f = o.open(&tmp)?;
    f.write_all(data)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}
