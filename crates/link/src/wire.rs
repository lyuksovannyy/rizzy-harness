//! Handshake on a fresh connection, then plain JSON protocol frames.
//!
//! ```text
//! master -> node   {"type":"challenge","nonce":"<32 bytes hex>"}
//! node   -> master {"type":"join","token","public_key","hostname","signature"}
//!                | {"type":"auth","node_id","signature","protocol_version"}
//! master -> node   {"type":"accepted","node_id"} | {"type":"rejected","reason"}
//! ```
//! `signature` signs `identity::auth_message(nonce, master_fingerprint)`.
//! A `rejected` ends the node's reconnect loop (`client::run`): every
//! refusal the master sends needs a person (a revoked or unknown key, a
//! spent token, another protocol version).

use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use crate::{LinkError, NodeKey, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Challenge { nonce: String },
    Accepted { node_id: String },
    Rejected { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Hello {
    Join {
        token: String,
        public_key: String,
        hostname: String,
        signature: String,
        /// As in `auth`: a join of another protocol is refused with the
        /// reason before its token is spent (review N-9).
        protocol_version: u32,
    },
    Auth {
        node_id: String,
        signature: String,
        /// The node's `harness_protocol::PROTOCOL_VERSION`, so the master
        /// can refuse another one with the reason before `accepted`
        /// (A.19). Required: a hello without it is malformed (no
        /// compatibility code before the first tag, review N-4).
        protocol_version: u32,
    },
}

/// The most characters of a master's `rejected` reason a node keeps
/// (review A19-6): the master's own refusals are one line of about 150.
pub const MAX_REASON_CHARS: usize = 300;

/// What a node may print of text its master sent: one line, no control
/// characters (no escape sequences, no forged log lines), at most `max`
/// characters, cut with an ellipsis.
pub fn printable(text: &str, max: usize) -> String {
    let mut out: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out.trim().to_string()
}

pub type ServerStream = WebSocketStream<tokio_rustls::server::TlsStream<TcpStream>>;
pub type ClientStream = WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>;

pub fn ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(crate::MAX_MESSAGE))
        .max_frame_size(Some(crate::MAX_MESSAGE))
}

pub fn new_nonce() -> String {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("OS randomness");
    hex::encode(b)
}

pub async fn send_json<S, T: Serialize>(ws: &mut WebSocketStream<S>, v: &T) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let text = serde_json::to_string(v).map_err(|e| LinkError::Protocol(e.to_string()))?;
    ws.send(Message::text(text)).await?;
    Ok(())
}

/// Next text frame as `T` (pings are answered by tungstenite).
pub async fn recv_json<S, T: for<'de> Deserialize<'de>>(
    ws: &mut WebSocketStream<S>,
) -> Result<Option<T>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    while let Some(m) = ws.next().await {
        match m? {
            Message::Text(t) => {
                return serde_json::from_str(t.as_str())
                    .map(Some)
                    .map_err(|e| bad_frame(&e));
            }
            Message::Close(_) => return Ok(None),
            _ => {}
        }
    }
    Ok(None)
}

/// A frame that does not parse, as an error that says only where and what
/// kind: serde's own message can quote the input, which may hold a join
/// token or anything else the peer sent.
pub(crate) fn bad_frame(e: &serde_json::Error) -> LinkError {
    LinkError::Protocol(format!(
        "bad frame ({:?} error at line {} column {})",
        e.classify(),
        e.line(),
        e.column()
    ))
}

/// Master side: TLS + WebSocket accept.
pub async fn accept(tcp: TcpStream, tls: Arc<rustls::ServerConfig>) -> Result<ServerStream> {
    let acceptor = tokio_rustls::TlsAcceptor::from(tls);
    let stream = acceptor.accept(tcp).await?;
    Ok(tokio_tungstenite::accept_async_with_config(stream, Some(ws_config())).await?)
}

/// Node side: TLS (pinned) + WebSocket connect to `wss://host:port`, with
/// the default timeouts.
pub async fn connect(url: &str, master_fingerprint: &str) -> Result<ClientStream> {
    connect_with(url, master_fingerprint, crate::Timing::default()).await
}

/// `connect` within `timing.connect` for TCP and `timing.handshake` for
/// TLS plus the WebSocket upgrade, each as a whole.
pub async fn connect_with(
    url: &str,
    master_fingerprint: &str,
    timing: crate::Timing,
) -> Result<ClientStream> {
    let rest = url
        .strip_prefix("wss://")
        .ok_or_else(|| LinkError::Protocol("master URL must start with wss://".into()))?;
    let hostport = rest.split('/').next().unwrap_or(rest);
    let host = hostport
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(hostport)
        .trim_matches(['[', ']']);
    let connector =
        tokio_rustls::TlsConnector::from(crate::pinned_client_config(master_fingerprint)?);
    let name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|e| LinkError::Tls(e.to_string()))?;
    let tcp = tokio::time::timeout(timing.connect, TcpStream::connect(hostport))
        .await
        .map_err(|_| crate::timed_out("connecting to the master"))??;
    tcp.set_nodelay(true)?;
    tokio::time::timeout(timing.handshake, async {
        let tls = connector.connect(name, tcp).await?;
        let (ws, _) =
            tokio_tungstenite::client_async_with_config(url, tls, Some(ws_config())).await?;
        Ok(ws)
    })
    .await
    .map_err(|_| crate::timed_out("TLS handshake with the master"))?
}

/// Node side of the handshake. Returns the node id the master assigned or
/// confirmed.
pub async fn node_handshake(
    ws: &mut ClientStream,
    key: &NodeKey,
    master_fingerprint: &str,
    hello: impl FnOnce(String) -> Hello,
) -> Result<String> {
    let nonce = match recv_json::<_, ServerMsg>(ws).await? {
        Some(ServerMsg::Challenge { nonce }) => nonce,
        other => {
            return Err(LinkError::Protocol(format!(
                "expected a challenge, got {}",
                kind(other.as_ref())
            )));
        }
    };
    let sig = key.sign_hex(&crate::identity::auth_message(&nonce, master_fingerprint));
    send_json(ws, &hello(sig)).await?;
    match recv_json::<_, ServerMsg>(ws).await? {
        Some(ServerMsg::Accepted { node_id }) => Ok(node_id),
        Some(ServerMsg::Rejected { reason }) => {
            Err(LinkError::Rejected(printable(&reason, MAX_REASON_CHARS)))
        }
        other => Err(LinkError::Protocol(format!(
            "unexpected {}",
            kind(other.as_ref())
        ))),
    }
}

/// A handshake message by its kind only: what a master sent in it is not
/// printed.
fn kind(m: Option<&ServerMsg>) -> &'static str {
    match m {
        None => "the end of the connection",
        Some(ServerMsg::Challenge { .. }) => "a challenge",
        Some(ServerMsg::Accepted { .. }) => "an acceptance",
        Some(ServerMsg::Rejected { .. }) => "a refusal",
    }
}
