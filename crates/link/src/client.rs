//! Node side: keep a link to the master up, reconnecting with backoff.

use std::future::Future;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use harness_protocol::{MasterToNode, NodeToMaster};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::wire::{Hello, connect, node_handshake};
use crate::{LinkError, NodeKey};

pub struct Remote {
    pub url: String,
    pub fingerprint: String,
    pub node_id: String,
}

/// Run until `stop`. `attach` connects the node runtime and returns its
/// channels (called once per connection; it replays `hello` with heads, so
/// the master resumes whatever it missed while the link was down).
/// A rejection (revoked key) ends the loop with an error.
pub async fn run<F, Fut>(
    remote: Remote,
    key: NodeKey,
    stop: CancellationToken,
    attach: F,
) -> Result<(), LinkError>
where
    F: Fn() -> Fut,
    Fut: Future<
        Output = (
            mpsc::UnboundedSender<MasterToNode>,
            mpsc::UnboundedReceiver<NodeToMaster>,
        ),
    >,
{
    let mut backoff = Duration::from_millis(250);
    while !stop.is_cancelled() {
        match session(&remote, &key, &stop, &attach).await {
            Ok(()) => backoff = Duration::from_millis(250),
            Err(LinkError::Rejected(r)) => return Err(LinkError::Rejected(r)),
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

async fn session<F, Fut>(
    remote: &Remote,
    key: &NodeKey,
    stop: &CancellationToken,
    attach: &F,
) -> Result<(), LinkError>
where
    F: Fn() -> Fut,
    Fut: Future<
        Output = (
            mpsc::UnboundedSender<MasterToNode>,
            mpsc::UnboundedReceiver<NodeToMaster>,
        ),
    >,
{
    let mut ws = connect(&remote.url, &remote.fingerprint).await?;
    let id = node_handshake(&mut ws, key, &remote.fingerprint, |signature| Hello::Auth {
        node_id: remote.node_id.clone(),
        signature,
    })
    .await?;
    tracing::info!(node = %id, url = %remote.url, "connected to master");
    let (to_node, mut from_node) = attach().await;
    let (mut sink, mut stream) = ws.split();
    let mut ping = crate::ping_ticker();
    let mut heard = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _ = stop.cancelled() => {
                let _ = sink.send(Message::Close(None)).await;
                return Ok(());
            }
            _ = ping.tick() => {
                if heard.elapsed() > crate::IDLE_LIMIT {
                    return Err(LinkError::Protocol("master went silent".into()));
                }
                sink.send(Message::Ping(Vec::new().into())).await?;
            }
            out = from_node.recv() => match out {
                Some(m) => {
                    let t = serde_json::to_string(&m).map_err(|e| LinkError::Protocol(e.to_string()))?;
                    sink.send(Message::text(t)).await?;
                }
                None => return Ok(()),
            },
            inc = stream.next() => match inc.inspect(|_| heard = tokio::time::Instant::now()) {
                Some(Ok(Message::Text(t))) => {
                    let m: MasterToNode = serde_json::from_str(t.as_str())
                        .map_err(|e| LinkError::Protocol(e.to_string()))?;
                    if to_node.send(m).is_err() {
                        return Ok(());
                    }
                }
                Some(Ok(Message::Close(_))) | None => {
                    return Err(LinkError::Protocol("master closed the link".into()));
                }
                Some(Err(e)) => return Err(e.into()),
                Some(Ok(_)) => {}
            },
        }
    }
}

/// One-time join: present the token, receive the node id.
pub async fn join(
    url: &str,
    fingerprint: &str,
    token: &str,
    hostname: &str,
    key: &NodeKey,
) -> Result<String, LinkError> {
    let mut ws = connect(url, fingerprint).await?;
    let id = node_handshake(&mut ws, key, fingerprint, |signature| Hello::Join {
        token: token.to_string(),
        public_key: key.public_hex(),
        hostname: hostname.to_string(),
        signature,
    })
    .await?;
    let _ = ws.close(None).await;
    Ok(id)
}
