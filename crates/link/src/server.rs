//! Master side: after the handshake, pump protocol frames between the
//! WebSocket and a pair of bounded channels (the same shape as an
//! in-process link).

use harness_protocol::{MasterToNode, NodeToMaster};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::wire::ServerStream;
use crate::{FROM_NODE_QUEUE, TO_NODE_QUEUE, Timing};

/// Bridge an authenticated connection. The first node message must be a
/// `node.hello` for `node_id`; anything else closes the link (a node can
/// only speak as itself). `stop` closes it from the master side (revoke).
///
/// At most `FROM_NODE_QUEUE` messages from the node wait for the master;
/// past that the node's socket is not read until the master catches up.
pub fn bridge(
    ws: ServerStream,
    node_id: String,
    stop: CancellationToken,
) -> (mpsc::Sender<MasterToNode>, mpsc::Receiver<NodeToMaster>) {
    bridge_with(ws, node_id, stop, Timing::default())
}

/// `bridge` with other timers (tests).
pub fn bridge_with(
    ws: ServerStream,
    node_id: String,
    stop: CancellationToken,
    timing: Timing,
) -> (mpsc::Sender<MasterToNode>, mpsc::Receiver<NodeToMaster>) {
    let (to_node_tx, mut to_node_rx) = mpsc::channel::<MasterToNode>(TO_NODE_QUEUE);
    let (from_node_tx, from_node_rx) = mpsc::channel::<NodeToMaster>(FROM_NODE_QUEUE);
    tokio::spawn(async move {
        let mut first = true;
        let check = |msg: &NodeToMaster| {
            let hello = matches!(msg, NodeToMaster::Hello { .. });
            if std::mem::take(&mut first) {
                match msg {
                    NodeToMaster::Hello { node_id: id, .. } if *id == node_id => Ok(()),
                    _ => Err("hello missing or for another node".to_string()),
                }
            } else if hello {
                Err("second hello".to_string())
            } else {
                Ok(())
            }
        };
        let res = crate::pump::pump(ws, timing, &stop, &mut to_node_rx, &from_node_tx, check).await;
        if let Err(e) = res {
            tracing::info!(node = %node_id, error = %e, "node link closed");
        }
    });
    (to_node_tx, from_node_rx)
}
