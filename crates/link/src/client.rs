//! Node side: keep a link to the master up, reconnecting with backoff.

use std::future::Future;
use std::time::Duration;

use harness_protocol::{MasterToNode, NodeToMaster};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::wire::{ClientStream, Hello, connect_with, node_handshake};
use crate::{LinkError, NodeKey, Timing, timed_out};

pub struct Remote {
    pub url: String,
    pub fingerprint: String,
    pub node_id: String,
}

/// Run until `stop`. `attach` connects the node runtime and returns its
/// channels (called once per connection; it replays `hello` with heads, so
/// the master resumes whatever it missed while the link was down).
/// A rejection ends the loop with an error: each one the master sends needs
/// a person to act (a revoked or unknown key: join again; another protocol
/// version: upgrade the older side), so trying again changes nothing.
/// Everything else (a master that is down or restarting, a network error,
/// a TLS failure, a timeout, a link dropped after `accepted`) is retried
/// with backoff up to 30 s.
///
/// The channels are bounded: when the runtime's command queue is full the
/// master's socket is not read, so a master cannot make the node buffer
/// without limit.
pub async fn run<F, Fut>(
    remote: Remote,
    key: NodeKey,
    stop: CancellationToken,
    attach: F,
) -> Result<(), LinkError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = (mpsc::Sender<MasterToNode>, mpsc::Receiver<NodeToMaster>)>,
{
    run_with(remote, key, stop, Timing::default(), attach).await
}

/// `run` with other timers (tests).
pub async fn run_with<F, Fut>(
    remote: Remote,
    key: NodeKey,
    stop: CancellationToken,
    timing: Timing,
    attach: F,
) -> Result<(), LinkError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = (mpsc::Sender<MasterToNode>, mpsc::Receiver<NodeToMaster>)>,
{
    let mut backoff = Duration::from_millis(250);
    while !stop.is_cancelled() {
        match session(&remote, &key, &stop, timing, &attach).await {
            Ok(()) => backoff = Duration::from_millis(250),
            Err(e @ (LinkError::Rejected(_) | LinkError::Untrusted(_))) => return Err(e),
            Err(e) => tracing::warn!(error = %e, "link to master failed"),
        }
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = tokio::time::sleep(backoff) => {}
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
    Ok(())
}

/// Connect and authenticate as `remote.node_id`, within `timing`'s connect
/// and handshake limits (a master that accepts and then says nothing, or
/// says it slowly, is given up on).
pub async fn connect_node(
    remote: &Remote,
    key: &NodeKey,
    timing: Timing,
) -> Result<(ClientStream, String), LinkError> {
    let (ws, id) = open(&remote.url, &remote.fingerprint, key, timing, |signature| {
        Hello::Auth {
            node_id: remote.node_id.clone(),
            signature,
            protocol_version: harness_protocol::PROTOCOL_VERSION,
        }
    })
    .await?;
    // `auth` names this node: the master confirms that id, no other.
    if id != remote.node_id {
        return Err(LinkError::Untrusted(
            "it accepted this node under another id".into(),
        ));
    }
    Ok((ws, id))
}

async fn open(
    url: &str,
    fingerprint: &str,
    key: &NodeKey,
    timing: Timing,
    hello: impl FnOnce(String) -> Hello,
) -> Result<(ClientStream, String), LinkError> {
    let mut ws = connect_with(url, fingerprint, timing).await?;
    let id = tokio::time::timeout(
        timing.handshake,
        node_handshake(&mut ws, key, fingerprint, hello),
    )
    .await
    .map_err(|_| timed_out("handshake with the master"))??;
    Ok((ws, id))
}

async fn session<F, Fut>(
    remote: &Remote,
    key: &NodeKey,
    stop: &CancellationToken,
    timing: Timing,
    attach: &F,
) -> Result<(), LinkError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = (mpsc::Sender<MasterToNode>, mpsc::Receiver<NodeToMaster>)>,
{
    let (ws, id) = connect_node(remote, key, timing).await?;
    tracing::info!(node = %id, url = %remote.url, "connected to master");
    let (to_node, mut from_node) = attach().await;
    crate::pump::pump(ws, timing, stop, &mut from_node, &to_node, |_| Ok(())).await
}

/// One-time join: present the token, receive the node id.
pub async fn join(
    url: &str,
    fingerprint: &str,
    token: &str,
    hostname: &str,
    key: &NodeKey,
) -> Result<String, LinkError> {
    let (mut ws, id) = open(url, fingerprint, key, Timing::default(), |signature| {
        Hello::Join {
            token: token.to_string(),
            public_key: key.public_hex(),
            hostname: hostname.to_string(),
            signature,
            protocol_version: harness_protocol::PROTOCOL_VERSION,
        }
    })
    .await?;
    let _ = tokio::time::timeout(Duration::from_secs(1), ws.close(None)).await;
    Ok(id)
}
