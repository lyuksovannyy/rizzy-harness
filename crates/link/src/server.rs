//! Master side: after the handshake, pump protocol frames between the
//! WebSocket and a pair of channels (the same shape as an in-process link).

use futures::{SinkExt, StreamExt};
use harness_protocol::{MasterToNode, NodeToMaster};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::wire::ServerStream;

/// Bridge an authenticated connection. The first node message must be a
/// `node.hello` for `node_id`; anything else closes the link (a node can
/// only speak as itself). `stop` closes it from the master side (revoke).
pub fn bridge(
    ws: ServerStream,
    node_id: String,
    stop: CancellationToken,
) -> (
    mpsc::UnboundedSender<MasterToNode>,
    mpsc::UnboundedReceiver<NodeToMaster>,
) {
    let (to_node_tx, mut to_node_rx) = mpsc::unbounded_channel::<MasterToNode>();
    let (from_node_tx, from_node_rx) = mpsc::unbounded_channel::<NodeToMaster>();
    let (mut sink, mut stream) = ws.split();
    tokio::spawn(async move {
        let mut first = true;
        loop {
            tokio::select! {
                _ = stop.cancelled() => break,
                out = to_node_rx.recv() => match out {
                    Some(m) => {
                        let Ok(t) = serde_json::to_string(&m) else { continue };
                        if sink.send(Message::text(t)).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
                inc = stream.next() => match inc {
                    Some(Ok(Message::Text(t))) => {
                        let msg: NodeToMaster = match serde_json::from_str(t.as_str()) {
                            Ok(m) => m,
                            Err(e) => {
                                tracing::warn!(node = %node_id, error = %e, "bad frame; closing");
                                break;
                            }
                        };
                        if first {
                            first = false;
                            match &msg {
                                NodeToMaster::Hello { node_id: id, .. } if *id == node_id => {}
                                _ => {
                                    tracing::warn!(node = %node_id, "hello missing or for another node; closing");
                                    break;
                                }
                            }
                        } else if matches!(msg, NodeToMaster::Hello { .. }) {
                            break;
                        }
                        if from_node_tx.send(msg).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                },
            }
        }
        let _ = sink.send(Message::Close(None)).await;
    });
    (to_node_tx, from_node_rx)
}
