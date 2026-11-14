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

use std::time::Duration;

pub mod client;
pub mod identity;
mod pump;
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
    /// The master's `rejected` reason, made printable
    /// (`wire::printable`).
    #[error("the master refused this node: {0}")]
    Rejected(String),
    /// The node refuses its master: it answered outside the protocol's
    /// shape (an id that is not one). Like a refusal, it needs a person.
    #[error("this node refused its master: {0}")]
    Untrusted(String),
}

pub type Result<T> = std::result::Result<T, LinkError>;

/// Largest WebSocket frame and message accepted on the link.
pub const MAX_MESSAGE: usize = harness_protocol::MAX_FRAME;

/// Both ends ping this often once the link is up...
pub const PING_EVERY: Duration = Duration::from_secs(20);
/// ...and drop it when the peer sends nothing for this long while we are
/// reading, or does not take a frame we are writing for this long (a
/// half-open TCP connection would otherwise look connected until the OS
/// gives up). Time spent not reading because our own consumer is behind
/// does not count: a busy link is not a dead one.
pub const IDLE_LIMIT: Duration = Duration::from_secs(60);
/// Node side: the most bytes the kernel holds unsent for the master's
/// socket (`TCP_NOTSENT_LOWAT`, A.21). Without it the send buffer
/// autotunes up to `tcp_wmem`'s maximum (4 MiB on the machine B.7 ran on),
/// and every byte in it is ahead of a Stop's ack: up to 16 s at 256 KiB/s
/// (measured over loopback with the node's lane for acks: 8.9 s without
/// this cap, 1.0 s with it, `node/tests/control_over_link.rs`). Bytes
/// in flight are not limited (the congestion window decides), so a fast
/// link keeps its rate; the node's own queue holds the rest.
pub const UNSENT_LIMIT: u32 = 128 * 1024;
/// Node side: the TCP connect to the master...
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// ...then TLS plus the WebSocket upgrade, and separately the challenge
/// exchange, each as a whole (a peer that sends a byte a second does not
/// extend it).
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Master side: messages from one node parsed and waiting to be stored.
/// When full, the master stops reading that node's socket, so TCP pushes
/// back on the node. With frames of at most `MAX_MESSAGE`, one node can
/// hold at most `FROM_NODE_QUEUE + 2` messages in the master (the queue,
/// the one being read, the one being stored).
pub const FROM_NODE_QUEUE: usize = 8;
/// Master side: commands to one node waiting to be written. When full,
/// a new command fails at once as if the node had not answered.
pub const TO_NODE_QUEUE: usize = 64;

/// The link's timers; `Default` is the production values (tests shorten
/// them).
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub ping_every: Duration,
    pub idle_limit: Duration,
    pub connect: Duration,
    pub handshake: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            ping_every: PING_EVERY,
            idle_limit: IDLE_LIMIT,
            connect: CONNECT_TIMEOUT,
            handshake: HANDSHAKE_TIMEOUT,
        }
    }
}

pub(crate) fn timed_out(what: &str) -> LinkError {
    LinkError::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("{what} timed out"),
    ))
}

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
