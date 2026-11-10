//! The data phase of a link, the same on both ends: frames from the socket
//! into a bounded channel, frames from a bounded channel onto the socket.
//!
//! Reading and writing run concurrently, so a peer that is slow to read
//! never stops us from reading it (and the other way round): two ends both
//! blocked on writing cannot deadlock. A frame is read only once the
//! channel has room for it, so a full channel stops reading the socket and
//! TCP pushes back on the peer; nothing is buffered beyond the channel.

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::{LinkError, Timing};

/// How long a closing frame may take before the socket is just dropped.
const CLOSE_GRACE: Duration = Duration::from_secs(1);

/// Run until `stop`, either channel closes (`Ok`), or the peer closes,
/// goes silent or breaks the protocol (`Err`). `check` sees every incoming
/// message before it is passed on; an error closes the link.
pub(crate) async fn pump<S, In, Out>(
    ws: WebSocketStream<S>,
    timing: Timing,
    stop: &CancellationToken,
    outgoing: &mut mpsc::Receiver<Out>,
    incoming: &mpsc::Sender<In>,
    mut check: impl FnMut(&In) -> Result<(), String>,
) -> Result<(), LinkError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    In: DeserializeOwned,
    Out: Serialize,
{
    let (mut sink, mut stream) = ws.split();
    let read = async {
        loop {
            // Room first, then read: a stalled consumer stops the socket.
            let Ok(permit) = incoming.reserve().await else {
                return Ok(());
            };
            // Only time spent waiting on the peer counts as silence.
            let text = loop {
                match tokio::time::timeout(timing.idle_limit, stream.next()).await {
                    Err(_) => return Err(LinkError::Protocol("peer went silent".into())),
                    Ok(None) | Ok(Some(Ok(Message::Close(_)))) => {
                        return Err(LinkError::Protocol("peer closed the link".into()));
                    }
                    Ok(Some(Err(e))) => return Err(e.into()),
                    Ok(Some(Ok(Message::Text(t)))) => break t,
                    Ok(Some(Ok(_))) => {}
                }
            };
            let msg: In =
                serde_json::from_str(text.as_str()).map_err(|e| crate::wire::bad_frame(&e))?;
            check(&msg).map_err(LinkError::Protocol)?;
            permit.send(msg);
        }
    };
    let write = async {
        let start = tokio::time::Instant::now() + timing.ping_every;
        let mut ping = tokio::time::interval_at(start, timing.ping_every);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let frame = tokio::select! {
                _ = ping.tick() => Message::Ping(Vec::new().into()),
                m = outgoing.recv() => match m {
                    Some(m) => Message::text(
                        serde_json::to_string(&m).map_err(|e| LinkError::Protocol(e.to_string()))?,
                    ),
                    None => return Ok(()),
                },
            };
            // A peer that takes nothing for this long is gone (or wedged).
            tokio::time::timeout(timing.idle_limit, sink.send(frame))
                .await
                .map_err(|_| LinkError::Protocol("peer stopped reading".into()))??;
        }
    };
    let res = tokio::select! {
        _ = stop.cancelled() => Ok(()),
        r = read => r,
        w = write => w,
    };
    let _ = tokio::time::timeout(CLOSE_GRACE, sink.send(Message::Close(None))).await;
    res
}
