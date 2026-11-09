# Center ↔ node link: security review (2026-11-09)

Scope: `crates/link` (transport, handshake, identity), the master's
acceptor (`crates/master/src/nodes.rs`), and how the master treats what a
node sends (`add_link`, `on_node_message`, `Store::append`).

## What holds

- **Transport.** WebSocket over TLS 1.2/1.3 (rustls, aws-lc-rs, safe
  defaults) on a port that speaks only this protocol. Nodes pin the
  master's self-signed certificate by SHA-256 fingerprint; no CA is trusted.
- **Node identity.** One Ed25519 key per node, pinned at join. Each
  connection signs a fresh 256-bit nonce together with the master's
  certificate fingerprint, so a signature cannot be replayed against
  another master or relayed through a man in the middle.
- **Join.** Tokens are 128-bit, single-use, valid for 15 minutes and stored
  only as hashes. The key's signature is checked before the token is spent,
  spending happens in one transaction, and a key can join only once.
- **A node speaks only as itself.** The first frame must be `node.hello`
  for the authenticated id. Events are appended only to sessions that node
  owns (`WrongOwner` otherwise), only the owner may delete a session, a
  sub-agent session adopts an owner only from the same node, and artifact
  chunks are routed by `(node, request id)`.
- **Revocation** rejects the key and drops a live link.
- **Limits.** 8 MiB per frame, 10 s for each handshake step.
- **The node decides.** `node.toml` is authoritative on the node; the
  master's requests are clamped to it.

## Fixed in this change

| # | Severity | Issue | Fix |
|---|---|---|---|
| 1 | Medium | A remote node could announce `profile:chat` and register as a chat runtime. Whenever the master's own runtime was not connected, it would receive chats meant for it, including those of people without the agentic grant and incognito chats. | Remote links that claim the chat profile are refused (`add_link`). |
| 2 | Medium | Unauthenticated audit growth: an `auth` hello with any node id (up to 8 MiB) wrote an audit row, without limit. | Ids must look like the ones the master assigns (at most 64 of `[a-z0-9-]`); only failures for registered nodes are audited. |
| 3 | Low–Medium | Unlimited concurrent pre-auth handshakes, each holding a task and up to an 8 MiB buffer for 20 s. | At most 32 handshakes in progress; more are dropped at once. |
| 4 | Low | Acks were matched by request id alone, so a node that learned another node's request id could settle that command. | Each pending command remembers its node; only that node's ack counts. |
| 5 | Low | No keepalive: a half-open link stayed "connected" and commands to it timed out until TCP gave up. | Both ends ping every 20 s and close after 60 s of silence. |
| 6 | Low | `node join --token` put the token in the process list. | `HARNESS_JOIN_TOKEN` is read too, and the UI's join command uses it. |
| 7 | Low | The hostname and version a remote node reports went to the UI unbounded. | Control characters are removed and each is cut to 64 characters. |

Related, from the `.env` work: the master's agent shells used to inherit
its whole environment minus the provider-key variables. Once a `.env` is
loaded, that would include every key and URL it holds. Now every `HARNESS_*` variable and every name a `.env` sets is
removed from tool processes, and the secret ones are also redacted.

## Left open

- **Backpressure from nodes.** The master reads node frames into an
  unbounded channel, so a registered (authenticated) node could make it
  buffer without limit. The fix is a bounded `NodeLink::from_node`; it
  touches about 30 call sites and is left for its own change.
- **Channel binding** uses the certificate fingerprint, not a TLS exporter
  (RFC 9266). Equivalent here, since losing the master's key is total
  anyway.
- **No certificate rotation.** If the master's key leaks, every node must
  join again.
