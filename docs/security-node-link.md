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
- **Who proves what.** The master is authenticated only by TLS against the
  pinned certificate; the node by signing the master's nonce. This is not a
  mutual challenge-response (the master signs nothing itself).
- **Limits.** 8 MiB per frame and message (`harness_protocol::MAX_FRAME`),
  refused from the frame header before the payload is read; a violation
  closes the link and logs the sizes only. On the master, 10 s for each
  handshake step and at most 32 handshakes in progress. On the node, 10 s
  for the TCP connect, 10 s for TLS plus the upgrade and 10 s for the
  challenge exchange, each a deadline for the whole step.
- **Back-pressure (FIX_PLAN 3.7).** See below.
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

## Back-pressure (2026-11-10, FIX_PLAN 3.7, audit L1/L15)

`NodeLink` is bounded in both directions, and each end of a remote link
reads and writes its socket concurrently (`crates/link/src/pump.rs`), so
two ends that both have a lot to say cannot deadlock on each other.

| Queue | Bound | When full |
|---|---|---|
| Master, from one node (parsed, not yet stored) | 8 messages | the master stops reading that node's socket; TCP pushes back on the node. Worst case per node: (8 + 2) × 8 MiB with maximal frames, a few KiB in practice. Other nodes and the UI are not involved. |
| Master, to one node (not yet written) | 64 commands | a new command fails at once (`Timeout`); a `resume` that does not fit is asked again on the next gap or hello |
| Node, commands from the master | 64 | the node stops reading the master's socket |
| Node, to the master | 256 messages | session actors (appending an event) and the command loop (acks, snapshots) wait. Nothing is dropped. If the link dies while they wait, the wait ends, the event is already in the node's log, and the next hello plus `resume` replays it. Artifact chunks alone are sent without waiting and dropped when full; the master's transfer times out and asks again. |

Idle detection counts only time spent waiting on the peer: an end that is
not reading because its own consumer is behind does not call the peer dead.
A peer that sends nothing for 60 s while we read, or takes no byte of a frame
for 60 s while we write, is dropped. `session.snapshot` batches stop at 500
events or 2 MiB (a quarter of the frame limit), whichever comes first; one
event over 2 MiB goes alone.

`allow_remote_sessions`: T5 claimed the node decides "whether
remote-created sessions are allowed at all". No such setting existed, and
on a worker node every session is created by the master, so the setting
would mean "run no sessions", which not running `node run` already does.
The sentence was removed instead of adding the setting.

Tests: `crates/link/tests/backpressure.rs` (a flooding node, a flooding
master, a busy link that is not dead, a silent node, an oversized frame,
three stalled handshakes) and `resume_batches_are_limited_by_bytes` in
`crates/node/tests/runtime.rs`. They measure queue length and how far the
flooding peer got, not process memory.

## One writer per session (2026-11-11, ADOPTION_PLAN A.3)

Two findings from the round-2 review (`final-review-ours.md`, missed 3 and 4):

- **A repeated `seq` was acknowledged without looking at it.** Two processes
  with one node key, or a node restored from an old disk, could send another
  event under a stored `seq`; the master called it a duplicate and then took
  the second writer's next in-order frame, mixing two logs silently. Now
  `Store::append` compares the incoming event with the stored one (kind,
  timestamp, payload as JSON values). A difference marks the session diverged
  (`sessions.diverged_seq`) and every later append for it is refused. The
  rule, what the node is told and what is refused: `PLAN.md` §3.
- **A revoked node's queued frames were still applied** (up to the 8 the
  master's queue holds): revoke stopped the pump, but `apply` checked only
  ownership. `Store::append` now refuses a node whose `nodes.revoked_ms` is
  set, in the transaction that would store the frame.

Tests: `crates/store/tests/event_log.rs` (identical resend, divergent resend,
sticky refusal, key order and float round trips, revoked writer) and
`crates/master/tests/link_hardening.rs` (two links with one key, eight frames
queued at revoke, a reconnect that replays). Not done here: a generation per
connection and an epoch per session (ADOPTION_PLAN C.1).

## What a registered node sends (2026-11-11, Gate 3 review, R.3)

Findings L-1 to L-6, L-8 and L-9 of `docs/security-review/2026-11-gate3.md`
(on the review branch): what a node that is registered, but hostile, can do
to the master and to other users through its frames.

| # | Severity | Issue | Fix |
|---|---|---|---|
| L-2 | Medium | A second `session_created` was applied to the row at any `seq`: naming another user's chat on the node as parent moved the chat to that user; an unknown parent left it with no owner; `incognito` and `profile` were rewritten. | The row is set at `seq` 1 only; a later `session_created` is refused and freezes the session; no event changes an owner once set. |
| L-1 | Medium | `parent_session` was stored whatever it named; `project_of` then followed it with no check, so the node got another chat's project instructions, board and facts. | A parent is kept only when it is a session of the same node with the same owner (checked at append and at the create's stamp), and every follower of `parent_id` checks again. |
| L-3 | Medium | Titles, `cwd`, model and agent id were stored uncapped and every listing loaded every account's rows. Nothing capped one event below the frame. | Events are capped at 4 MiB (`MAX_EVENT_BYTES`) where they are appended; the row's strings are capped; listings filter by owner in SQL. |

A refused event (too large, malformed) is not stored. On the mirror the
session is frozen like a diverged one (logged once at `error` with the rule
and no content, `session.cancel`, commands refused); a node's own log
applies the same rules first, so an honest node fails its own append
instead of sending it. PLAN.md §3 has the rules.

Tests: `crates/store/tests/event_log.rs` (the inverted proof tests:
`the_row_is_set_at_seq_1_and_its_owner_never_changes`,
`a_parent_is_kept_only_under_the_same_node_and_owner`,
`a_nodes_title_is_capped_in_the_row`; plus the size cap, identifiers and
the per-owner listing), `crates/master/tests/link_hardening.rs` and
`hostile_node.rs` (a fake node sends the hostile frames to a real master).

## Left open

- **Channel binding** uses the certificate fingerprint, not a TLS exporter
  (RFC 9266). Equivalent here, since losing the master's key is total
  anyway.
- **No certificate rotation.** If the master's key leaks, every node must
  join again.
