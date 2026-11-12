//! Remote nodes: the TLS listener, the join/auth handshake, revocation.

use std::sync::Arc;
use std::time::Duration;

use harness_link::wire::{Hello, ServerMsg, accept, new_nonce, recv_json, send_json};
use harness_link::{MasterCert, identity};
use harness_node::NodeLink;
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::{Master, lock};

/// How long a join token stays valid.
pub const JOIN_TOKEN_TTL: Duration = Duration::from_secs(15 * 60);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Connections that have not finished the handshake yet. Anyone who can
/// reach the port can open one; past this many, new ones are dropped at
/// once instead of each holding a task and a buffer for the timeout.
const MAX_PENDING_HANDSHAKES: usize = 32;
/// Audit rows a run of failed handshakes may write per node id and per
/// peer address in [`AUTH_FAILURE_WINDOW_MS`] (Gate 3 review L-7).
pub const AUTH_FAILURE_AUDITS: usize = 5;
const AUTH_FAILURE_WINDOW_MS: i64 = 15 * 60 * 1000;

/// Node ids the master assigns: `[a-z0-9-]`, at most 64 characters. Others
/// are refused before any lookup or audit row.
fn plausible_node_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The limiter of audit rows for failed handshakes ([`AUTH_FAILURE_AUDITS`]
/// per key; keys are `node:<id>` and `peer:<address>`).
pub(crate) fn auth_audit_limit() -> crate::auth::limits::FailureMap {
    crate::auth::limits::FailureMap::new(4096, AUTH_FAILURE_AUDITS, AUTH_FAILURE_WINDOW_MS)
}

/// Why a node speaking `node` is refused, for the person who runs it: both
/// versions and which side to upgrade. `None` when it speaks the master's.
/// No range is admitted before the first tag (`docs/node-update.md` §8.3).
pub(crate) fn protocol_refusal(node: u32) -> Option<String> {
    use harness_protocol::PROTOCOL_VERSION as MASTER;
    let older = match node.cmp(&MASTER) {
        std::cmp::Ordering::Equal => return None,
        std::cmp::Ordering::Greater => "the master is older: upgrade the master first",
        std::cmp::Ordering::Less => "this node is older: upgrade it to the master's build",
    };
    Some(format!(
        "this node speaks protocol {node} and the master (rizzy-harness {}) protocol {MASTER}: {older}",
        env!("CARGO_PKG_VERSION")
    ))
}

fn token_hash(t: &str) -> String {
    hex::encode(Sha256::digest(t.trim().as_bytes()))
}

impl Master {
    /// A single-use join token (only its hash is stored).
    pub async fn create_join_token(
        &self,
        by: Option<String>,
    ) -> Result<String, harness_store::StoreError> {
        let mut b = [0u8; 16];
        getrandom::fill(&mut b).expect("OS randomness");
        let token = hex::encode(b);
        let (h, exp) = (
            token_hash(&token),
            harness_core::now_ms() + JOIN_TOKEN_TTL.as_millis() as i64,
        );
        let detail = serde_json::json!({ "by": by, "expires_ms": exp }).to_string();
        self.inner
            .mirror
            .call(move |s| {
                s.join_token_create(&h, exp, by.as_deref())?;
                // Who may add a node, and when (Gate 3 review N-5); never
                // the token or its hash.
                s.audit("node.join_token", &detail)
            })
            .await?;
        Ok(token)
    }

    /// Revoke a node: its key stops working and a live link is dropped.
    pub async fn revoke_node(&self, node_id: &str) -> Result<bool, harness_store::StoreError> {
        let id = node_id.to_string();
        let done = self
            .inner
            .mirror
            .call(move |s| s.node_revoke(&id, harness_core::now_ms()))
            .await?;
        if done {
            // Under `nodes`, as `add_link` reads it: a link registered
            // before this is taken down here, one after it is refused there
            // (Gate 3 review L-4).
            let mut nodes = lock(&self.inner.nodes);
            lock(&self.inner.revoked).insert(node_id.to_string());
            if let Some(n) = nodes.get_mut(node_id) {
                n.tx = None;
                n.info.connected = false;
            }
        }
        if let Some(stop) = lock(&self.inner.remote_stops).remove(node_id) {
            stop.cancel();
        }
        let _ = self.inner.live.send(crate::Live::NodesChanged);
        let id = node_id.to_string();
        let _ = self
            .inner
            .mirror
            .call(move |s| {
                s.audit(
                    "node.revoke",
                    &serde_json::json!({ "node_id": id }).to_string(),
                )
            })
            .await;
        Ok(done)
    }

    /// Accept remote nodes on `listener` (TLS with `cert`) until the process ends.
    pub fn serve_nodes(&self, listener: TcpListener, cert: Arc<MasterCert>) -> anyhow::Result<()> {
        let tls = cert.server_config()?;
        let this = self.clone();
        let pending = Arc::new(tokio::sync::Semaphore::new(MAX_PENDING_HANDSHAKES));
        tokio::spawn(async move {
            loop {
                let (tcp, peer) = match listener.accept().await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(error = %e, "node listener accept failed");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let Ok(permit) = pending.clone().try_acquire_owned() else {
                    tracing::warn!(%peer, "too many node handshakes in progress; dropped");
                    continue;
                };
                let (this, tls, cert) = (this.clone(), tls.clone(), cert.clone());
                tokio::spawn(async move {
                    if let Err(e) = this.node_connection(tcp, peer, tls, &cert, permit).await {
                        tracing::info!(%peer, error = %e, "node connection ended");
                    }
                });
            }
        });
        Ok(())
    }

    async fn node_connection(
        &self,
        tcp: tokio::net::TcpStream,
        peer: std::net::SocketAddr,
        tls: Arc<rustls::ServerConfig>,
        cert: &MasterCert,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> anyhow::Result<()> {
        let mut ws = tokio::time::timeout(HANDSHAKE_TIMEOUT, accept(tcp, tls)).await??;
        let nonce = new_nonce();
        send_json(
            &mut ws,
            &ServerMsg::Challenge {
                nonce: nonce.clone(),
            },
        )
        .await?;
        let hello: Option<Hello> =
            tokio::time::timeout(HANDSHAKE_TIMEOUT, recv_json(&mut ws)).await??;
        // The handshake is over either way: free the slot.
        drop(permit);
        let signed = identity::auth_message(&nonce, &cert.fingerprint());
        let reject = |reason: &str| ServerMsg::Rejected {
            reason: reason.into(),
        };
        match hello {
            Some(Hello::Join {
                token,
                public_key,
                hostname,
                signature,
                protocol_version,
            }) => {
                // Possession of the key first, then the token is spent.
                if !identity::verify(&public_key, &signed, &signature) {
                    send_json(&mut ws, &reject("bad signature")).await?;
                    return Ok(());
                }
                // A join of another protocol is refused with the reason
                // before the token is spent, and only to whoever holds a
                // valid token (review N-9): a stranger learns nothing.
                if let Some(why) = protocol_refusal(protocol_version) {
                    let h = token_hash(&token);
                    let valid = self
                        .inner
                        .mirror
                        .call(move |s| s.join_token_valid(&h, harness_core::now_ms()))
                        .await?;
                    let why = if valid {
                        tracing::warn!("refused a node's join: {why}");
                        why
                    } else {
                        "invalid or expired join token".to_string()
                    };
                    send_json(&mut ws, &reject(&why)).await?;
                    return Ok(());
                }
                let hostname: String = hostname
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(64)
                    .collect();
                let (h, pk, hn) = (token_hash(&token), public_key.clone(), hostname.clone());
                let id = self
                    .inner
                    .mirror
                    .call(move |s| s.node_join(&h, harness_core::now_ms(), &pk, &hn))
                    .await?;
                match id {
                    Some(id) => {
                        let detail =
                            serde_json::json!({ "node_id": id, "hostname": hostname }).to_string();
                        let _ = self
                            .inner
                            .mirror
                            .call(move |s| s.audit("node.join", &detail))
                            .await;
                        send_json(&mut ws, &ServerMsg::Accepted { node_id: id }).await?;
                        let _ = self.inner.live.send(crate::Live::NodesChanged);
                    }
                    None => send_json(&mut ws, &reject("invalid or expired join token")).await?,
                }
                Ok(())
            }
            Some(Hello::Auth {
                node_id,
                signature,
                protocol_version,
            }) => {
                if !plausible_node_id(&node_id) {
                    send_json(&mut ws, &reject("unknown or revoked node")).await?;
                    return Ok(());
                }
                let id = node_id.clone();
                let key = self.inner.mirror.call(move |s| s.node_key(&id)).await?;
                let known = key.is_some();
                let ok = key.is_some_and(|k| identity::verify(&k, &signed, &signature));
                if !ok {
                    // Audit rows only for registered, unrevoked nodes (a
                    // wrong key): ids anyone can make up would let a
                    // stranger grow the audit log without limit. And even
                    // those at most AUTH_FAILURE_AUDITS per node id and per
                    // peer address in the window: node ids are guessable
                    // (L-7). The refusal is the same words either way.
                    if known && self.audit_auth_failure(&node_id, peer) {
                        let detail = serde_json::json!({ "node_id": node_id }).to_string();
                        let _ = self
                            .inner
                            .mirror
                            .call(move |s| s.audit("node.auth_failed", &detail))
                            .await;
                    } else if known {
                        tracing::debug!(node = %node_id, %peer, "failed node auth; audit rows for it are at their limit");
                    } else {
                        tracing::info!(node = %node_id, "auth from an unknown node id");
                    }
                    send_json(&mut ws, &reject("unknown or revoked node")).await?;
                    return Ok(());
                }
                // Only now, with the key proved: the versions are told to
                // the master's own nodes, never to a stranger (A.19). A
                // node refused here stops reconnecting and shows the reason.
                if let Some(why) = protocol_refusal(protocol_version) {
                    tracing::warn!(node = %node_id, "refused a node: {why}");
                    send_json(&mut ws, &reject(&why)).await?;
                    return Ok(());
                }
                send_json(
                    &mut ws,
                    &ServerMsg::Accepted {
                        node_id: node_id.clone(),
                    },
                )
                .await?;
                let stop = CancellationToken::new();
                if let Some(old) =
                    lock(&self.inner.remote_stops).insert(node_id.clone(), stop.clone())
                {
                    old.cancel();
                }
                // A revoke that ran between the key check and the line above
                // found no token to cancel: look again now that it is there
                // (Gate 3 review L-4). `add_link` refuses it as well.
                let id = node_id.clone();
                if self
                    .inner
                    .mirror
                    .call(move |s| s.node_is_revoked(&id))
                    .await?
                {
                    stop.cancel();
                    let mut stops = lock(&self.inner.remote_stops);
                    if stops.get(&node_id).is_some_and(|s| s.is_cancelled()) {
                        stops.remove(&node_id);
                    }
                    return Ok(());
                }
                let (to_node, from_node) = harness_link::server::bridge(ws, node_id.clone(), stop);
                self.add_link(NodeLink { to_node, from_node }, false)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))?;
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// Whether a failed handshake for `node_id` from `peer` may write its
    /// audit row: both its node id and its address are under their limit,
    /// and then both count it.
    fn audit_auth_failure(&self, node_id: &str, peer: std::net::SocketAddr) -> bool {
        let now = harness_core::now_ms();
        let (by_id, by_peer) = (format!("node:{node_id}"), format!("peer:{}", peer.ip()));
        let limit = &self.inner.node_auth_audits;
        match (
            limit.try_reserve(&by_id, now),
            limit.try_reserve(&by_peer, now),
        ) {
            (Ok(a), Ok(b)) => {
                a.failed();
                b.failed();
                true
            }
            // A reservation not settled is given back when dropped.
            _ => false,
        }
    }

    /// Registered remote nodes with their connection state.
    pub async fn registered_nodes(
        &self,
    ) -> Result<Vec<serde_json::Value>, harness_store::StoreError> {
        let rows = self.inner.mirror.call(|s| s.registered_nodes()).await?;
        let live = self.nodes();
        Ok(rows
            .into_iter()
            .map(|r| {
                let connected = live.iter().any(|n| n.node_id == r.node_id && n.connected);
                let pk = &r.public_key;
                serde_json::json!({
                    "node_id": r.node_id,
                    "hostname": r.hostname,
                    "key_fingerprint": format!("ed25519:{}", &pk[..pk.len().min(16)]),
                    "created_ms": r.created_ms,
                    "revoked": r.revoked_ms.is_some(),
                    "connected": connected && r.revoked_ms.is_none(),
                })
            })
            .collect())
    }
}
