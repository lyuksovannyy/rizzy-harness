# OpenClaw, Round 2 source investigation: sessions cluster

## 1. Repository

- `openclaw/openclaw`, pinned SHA `8bac830dcb8ff99055ba71558dd369a989cc7881`, commit date 2026-11-10T20:27:14Z, clone `.claude/research-repos/openclaw` (about 1 GB, 2,932 files in `src/gateway` alone).
- Read: the gateway WebSocket ingress and handshake (`src/gateway/server/`, `src/gateway/server/ws-connection/`), the protocol schemas (`packages/gateway-protocol/src/schema/frames.ts`, `worker-admission.ts`, `worker-inference.ts`), the cloud-worker subsystem (`src/gateway/worker-environments/`: admission, credential, credential broker, store transitions, transcript commit, inference model approval, GitHub binding, node enrollment, SSH), the worker side (`src/worker/`: connection, endpoint, launch descriptor, GitHub binding), node and device pairing (`src/infra/device-pairing-*`, `src/gateway/node-pairing-*`), the SQLite schemas (`src/state/openclaw-agent-schema.sql`, `src/state/openclaw-state-schema.sql`), the transcript write fence (`src/config/sessions/`), the writer claim (`src/agents/embedded-agent-runner/run/`), the delivery queue recovery, cron restart recovery, HTTP hooks, MCP client transports, tool search config, worktree snapshots, and the Control UI credential storage (`ui/src/app/settings.ts`, `ui/src/lib/nodes/index.ts`).
- Not read: the agent loop itself, compaction, approvals in depth (safety and agents clusters), most of the 2,000-line Rust node host in `crates/`.

## 2. Architecture as it bears on this cluster

- **Language and process model.** TypeScript on Node, one Gateway process; SQLite via Kysely with many "worker" (Node worker thread) stores. A Rust node host also exists: `crates/openclaw-node-host` ("Reusable Rust OpenClaw node runtime and headless host", MIT, ed25519-dalek, tokio). https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/crates/openclaw-node-host/Cargo.toml#L1-L13 (crate metadata and dependencies).
- **One port, three kinds of socket.** The HTTP upgrade handler classifies the path: the public worker route `/__openclaw__/worker` gets connection kind `worker` and its own rate limiter; everything else is a normal gateway socket. https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server-http-upgrades.ts#L257-L294 (worker route upgrade, rate check, socket marked `worker`). https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-protocol-primitives.ts#L8-L8 (the path constant).
- **Worker frames never reach the generic handler.** https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L99-L100 (dedicated worker ingress handler).
- **Where the decisions live.** Worker admission: `src/gateway/worker-environments/admission.ts`. Credential minting: `credential-broker.ts`. Owner epoch: `store-transitions.ts` and `store-mutations.ts`. Transcript fence: `src/config/sessions/session-accessor.sqlite-scope.ts`. Operator auth: `src/gateway/server/ws-connection/connect-auth.ts`. Method scopes: `src/gateway/method-scopes.ts`.
- **Two databases.** Per-agent `openclaw-agent` SQLite holds sessions and transcripts; the shared `openclaw-state` SQLite holds pairing, devices, worker environments, credentials, cron, the delivery queue. https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L1-L4 (storage doctrine comment).

## 3. Findings

### Session storage

#### Sessions are SQLite rows: a logical node, generations ("windows"), and a numbered event table
- What the code does: `session_nodes` holds one row per session key with the hot entry as `entry_json` plus promoted index columns (owner, creator, parent, fork source, archive time). `session_windows` holds transcript generations of that key, each with a `reason` (`initial`, `reset`, `rollover`, `fork`, `rewind`, `switch`, `recovery`, `compaction`) and a `previous_session_id` link. `transcript_events` is keyed `(session_id, seq)` and stores the event as JSON or as a zstd blob (each capped at 4 MiB) with a separate navigation summary. Message idempotency is a unique index on `(session_id, message_idempotency_key)`.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L16-L51 (session_nodes); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L140-L171 (session_windows with reason enum); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L454-L505 (transcript_events, zstd, 4 MiB checks); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L603-L617 (event identities and idempotency index).
- Edge cases and failure handling: reset and deleted generations move to `session_transcript_archives` with a SHA-256 and a publish-attempt counter; the derived file "may be recreated from this row after a crash". https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L507-L525 (archive table).
- Round 1: `corrected`. Round 1 gave the per-agent SQLite path only. The code shows the transcript is an event table with generations, not a flat transcript, and has a cold archive tier.
- For rizzy-harness: our event log already has `seq`; copy the idempotency-key unique index on message events and the generation link (`previous_session_id`, `reason`) for resets and forks.
- Confidence: `read the definition`.

#### Accepted input is stored outside the transcript until a turn takes it
- What the code does: `session_pending_inputs` stores each accepted message with `idempotency_key`, `run_id`, `request_hash` and a state (`queued`, `interrupted`, `cancelled`), unique per `(session_id, idempotency_key)`. Completion is a separate table and "neither schedules replay".
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L897-L932 (pending inputs and completions).
- Edge cases and failure handling: an `interrupted` state exists, so a restart does not silently re-run accepted input; the restart resume path re-injects one resume prompt instead (see below).
- Round 1: `confirmed` ("accepted input is stored before the Gateway acks").
- For rizzy-harness: a durable pending-input row with a request hash is the piece our queue lacks; it makes a client retry idempotent.
- Confidence: `read the definition`.

#### Retention defaults: 30 days, 5,000 entries, and a 10 GiB disk budget
- What the code does: maintenance defaults to `enforce` mode, prunes sessions after 30 days, caps entries at 5,000, prunes model-run sessions after 24 hours, archives dashboard sessions after 7 days, and cleans oldest-first once main file plus WAL plus artifacts cross 10 GiB (high water 80%).
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/store-maintenance.ts#L26-L34 (default constants); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/store-maintenance.ts#L132-L152 (resolution from config).
- Edge cases and failure handling: an unset `resetArchiveRetention` keeps reset archives until disk pressure (comment at L146).
- Round 1: `confirmed` (`pruneAfter` 30d, `maxEntries` 5000); the disk budget is `new (not documented)`.
- For rizzy-harness: a disk-budget trigger is a better backstop than age alone for our ever-growing event log (AUDIT D5).
- Confidence: `read the definition`. (checked in review)

#### Fork copies a completed prefix and refuses forks that are too large
- What the code does: fork resolves a token ceiling for the destination model, copies the completed prefix of the parent transcript (an active tail is excluded) into the destination store, and returns `too-large` when the parent exceeds the ceiling.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/session-create-fork.ts#L26-L70 (fork preparation and size refusal).
- Edge cases and failure handling: a failed copy returns "failed to fork parent session transcript" rather than a partial session.
- Round 1: `new (not documented)`; Round 1 had no fork entry.
- For rizzy-harness: copying only the completed prefix avoids forking mid-turn state; worth copying if we add fork.
- Confidence: `read the definition`.

#### Restart resume injects one fixed prompt, not a replay
- What the code does: on startup the Gateway marks eligible sessions with `resumeAfterRestart` and sends a fixed text telling the agent to check the state of files, processes, sub-agents and external actions before repeating anything. A legacy bridge accepts sessions interrupted within six hours before process start.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/session-restart-resume.ts#L78-L85 (resume text and 6 h window); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/session-restart-resume.ts#L525-L552 (marker selection, orphaned-run detection).
- Edge cases and failure handling: the marker stays on the entry until the new turn accepts, so a second restart during preparation keeps it (comment at L551-L552).
- Round 1: `confirmed`.
- For rizzy-harness: copy the wording idea: tell the model that tool effects before the crash are unknown.
- Confidence: `read the definition`.

### Checkpoints and undo

#### Worktree snapshots are git refs; idle worktrees are collected after 7 days only when no owner is active
- What the code does: `snapshotWorktree` builds a commit with a temporary index under `refs/openclaw/snapshots/<worktreeId>` (or an `exact-` prefix for exact state). Nested git repositories are refused. Idle GC uses `IDLE_GC_MS` of 7 days; the owner check returns `active` (no removal) when the session was touched within 7 days, when any session mutation or controller work is active, when facts changed since the census, or when placement facts are missing ("Missing session metadata cannot erase a durable remote worker's ownership").
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/worktrees/snapshot-inventory.ts#L406-L434 (nested-repo refusal, ref name); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/worktrees/service.ts#L123-L123 (7-day constant); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/worktrees/owner-protection.ts#L64-L99 (owner protection decisions).
- Edge cases and failure handling: every uncertain branch resolves to `active`, so the failure mode is "keep", not "delete".
- Round 1: `confirmed` (snapshot ref before removal); the dirty-idle guard was a Round 2 question and is answered above.
- For rizzy-harness: the fail-to-keep rule is the right default for checkpoint GC on nodes.
- Confidence: `read the definition` (the removal path that calls snapshot before delete was not traced).

#### Cloud-worker repository checkpoints are validated refs under `refs/openclaw/worker-results/`
- What the code does: a checkpoint ref must match `^refs/openclaw/worker-results/[A-Za-z0-9-]+$`; publication refs are derived by SHA-256 of the checkpoint ref.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/session-repository-checkpoint-refs.ts#L9-L23 (ref derivation and validation).
- Edge cases and failure handling: no valid checkpoint throws rather than falling back to HEAD.
- Round 1: `new (not documented)`.
- For rizzy-harness: validate ref names with a strict regex before passing them to git.
- Confidence: `read the definition`.

### The client and server split

#### Gateway protocol: JSON frames over WebSocket, challenge first, protocol version 4
- What the code does: on a gateway socket the server first sends a `connect.challenge` event with a random UUID nonce. The client's first request is `connect` with `minProtocol`/`maxProtocol`, client identity, `role`, `scopes`, an optional `device` block (id, public key, signature, `signedAt`, `nonce`) and an `auth` block (`token`, `bootstrapToken`, `deviceToken`, `password`, plus two runtime tokens). Afterwards frames are `req` (id, method, params), `res` (ok, payload or error) and `event` (optional `seq` and `stateVersion`). Current protocol is 4; nodes may be one version behind (3).
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/connection.ts#L324-L335 (challenge nonce sent); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/frames.ts#L34-L95 (connect params); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/frames.ts#L182-L215 (req, res, event frames); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/version.ts#L2-L6 (protocol 4, node minimum 3).
- Edge cases and failure handling: all schemas are `closedObject` (no unknown fields).
- Round 1: `confirmed`.
- For rizzy-harness: closed schemas on every frame match our approach; note OpenClaw puts all clients and nodes on one socket type, we have two links.
- Confidence: `read the definition`.

#### Device proof: Ed25519 over the nonce and the bearer token, two-minute skew
- What the code does: if a device block is present, the Gateway derives the device id from the public key and rejects a mismatch, rejects `signedAt` more than 2 minutes off, requires the nonce to equal this connection's challenge nonce, and verifies the Ed25519 signature over a payload containing device id, client id and mode, role, scopes, `signedAt`, the presented token and the nonce (v3 adds platform and device family; v2 is still accepted).
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/connect-auth.ts#L65-L65 (skew constant); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/connect-auth.ts#L369-L401 (id, skew, nonce, signature checks); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/handshake-auth-helpers.ts#L205-L245 (signed payload fields, v3 then v2).
- Edge cases and failure handling: each failure closes with 1008 and emits an auth security event. Signing the token binds the bearer to the key, so a stolen device token alone is not enough when a device block is required.
- Round 1: `confirmed`.
- For rizzy-harness: our link already pins keys; the "sign the nonce plus the presented token plus requested scopes" payload is a good template for the master's browser-device flow if we add one.
- Confidence: `traced end to end`. (checked in review)

#### Roles are a closed pair and unclassified methods require admin
- What the code does: the role claim parses to `operator` or `node` only; node methods are callable only by `node`, everything else only by `operator`. Operator scope checks pass for `operator.admin`; otherwise the method's required scope is resolved, and a method with no classification requires `ADMIN_SCOPE`.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/role-policy.ts#L6-L24 (role parse and role-method split); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/method-scopes.ts#L269-L317 (scope authorization, admin default at L310).
- Edge cases and failure handling: malformed `plugins.sessionAction` params pass with any operator scope so the handler can return a validation error (L284-L291), a small hole by design.
- Round 1: `confirmed` that scopes exist; "a scope ceiling can only filter" is consistent: scopes gate methods, not data.
- For rizzy-harness: copy fail-closed-to-admin for unclassified routes; our routes should have the same default.
- Confidence: `read the definition`.

#### Gateway auth defaults to a token that startup generates when none is configured
- What the code does: the mode is `override ?? config ?? (password ? "password" : "token")`. Startup generates a 24-byte random hex token when no token source exists. Tailscale identity is allowed by default only when Tailscale Serve is on and the mode is not `password` or `trusted-proxy`.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/auth-resolve.ts#L55-L84 (mode resolution, Tailscale default); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/startup-auth.ts#L199-L199 (generated token).
- Edge cases and failure handling: mode `none` exists; a local backend client may skip device pairing when mode is `none`. https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/handshake-auth-helpers.ts#L165-L187 (local backend self-pairing skip).
- Round 1: `confirmed`.
- For rizzy-harness: nothing to copy; our loopback-default plus accounts is stricter.
- Confidence: `read the definition`. (checked in review)

#### Control UI: shared token per tab, but the device private key and device tokens live in localStorage
- What the code does: the shared Gateway token is kept in `sessionStorage` under a key scoped to the Gateway origin, and a `#token=` fragment is accepted (a `?token=` query warns). The browser's Ed25519 device identity is a raw 32-byte seed from `crypto.getRandomValues`, stored base64url in `localStorage`, and issued device tokens per role are also written to `localStorage`.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/app/settings.ts#L287-L333 (token load and persist in sessionStorage); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/app/startup-settings.ts#L198-L221 (fragment and query token handling); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/lib/nodes/index.ts#L239-L251 (raw seed generated); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/lib/nodes/index.ts#L112-L126 (device auth store written to localStorage).
- Edge cases and failure handling: any script running in the Control UI origin can read the private key and device tokens; a non-extractable WebCrypto key is not used.
- Round 1: `corrected`. Round 1 said "token kept in session storage per tab". True for the shared token, but the long-lived device key and device tokens persist in localStorage.
- For rizzy-harness: if we add browser device keys, use a non-extractable WebCrypto key in IndexedDB; do not copy the localStorage seed.
- Confidence: `read the definition`. (checked in review)

### Remote workers, nodes and executors

#### Node and device pairing today: one device record, a pending node surface, and four operator methods
- What the code does: the standalone node store with its per-node token is retired; a doctor migration folds old `nodes/{paired,pending}.json` rows into device records. A node's approved command surface now lives on the paired device row (`node_surface_json`, `pending_node_surface_json`). Operators use `node.pair.list`, `node.pair.approve`, `node.pair.reject`, `node.pair.remove` (scope `operator.pairing`); `node.pair.request` and `node.pair.verify` are not in the method table.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/node-pairing-migration.ts#L1-L8 (retired store, per-node token, migration); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-state-schema.sql#L576-L599 (device_pairing_paired with tokens_json and node surfaces); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/methods/core-descriptors.ts#L271-L284 (pairing, device token and setup-code methods); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/device-pairing-node.ts#L211-L262 (node approval as a device-pairing mutation).
- Edge cases and failure handling: an approval is refused while a reconnect owns the pending revision (L256-L258), so a reconnect cannot race an approval onto a changed surface.
- Round 1: `confirmed` (store retired, `node.pair.request`/`verify` removed).
- For rizzy-harness: one identity record per machine with a separately approved capability surface is close to our enrolment; we should keep the surface on the node, not on the master.
- Confidence: `read the definition`.

#### Device tokens are stored in plaintext and compared in constant time
- What the code does: a device token is 32 random bytes base64url, one per role, kept with its scopes in `tokens_json`; verification compares the presented token with the stored token by `safeEqualSecret`, then checks revocation, issuer generation, and that the token's scopes are inside the device's approved baseline. Bootstrap (setup) tokens are also stored in plaintext with a 10-minute TTL.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/pairing-token.ts#L4-L18 (generate and verify); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/device-pairing-token-utils.ts#L14-L37 (token record contains the bearer string); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/device-pairing-row-mapping.ts#L119-L119 (tokens serialized to the row); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/device-pairing-token-policy.ts#L54-L108 (verification order); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-state-schema.sql#L604-L616 (bootstrap tokens table, `token` column); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/device-bootstrap.worker-types.ts#L5-L5 (10-minute TTL).
- Edge cases and failure handling: a read of `openclaw-state.sqlite` yields usable bearer tokens for every paired device. Only browser pairing requests store `approved_token_sha256` (schema L653).
- Round 1: `new (not documented)`. The contrast with the hashed worker credential is not in the docs.
- For rizzy-harness: store only hashes of any bearer we issue; OpenClaw hashes worker credentials but not device tokens.
- Confidence: `traced end to end`. (checked in review)

#### SSH-verified node auto-approval reads the node's identity back over SSH and compares keys
- What the code does: only a fresh, scopeless, non-browser `role: node` request with a directly attributable client IP qualifies. The probe target must be in private or CGNAT ranges, not loopback (or in configured CIDRs). The Gateway runs `ssh` with `BatchMode=yes` and `StrictHostKeyChecking=yes` and the remote command `sh -lc 'openclaw node identity --json'`, then requires both the device id and the normalized public key to match. Failures cool down 60 s, mismatches 5 min; at most 4 probes run at once. It is on by default (`sshVerify: false` disables it).
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-pairing-auto-approve.ts#L50-L65 (eligibility floor); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-pairing-ssh-verify.ts#L61-L86 (default-on policy, user is the Gateway's OS user); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-pairing-ssh-verify.ts#L105-L130 (probe scope); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-pairing-ssh-verify.ts#L190-L240 (probe, compare, cooldowns); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-pairing-ssh-verify.runtime.ts#L20-L45 (remote command and pinned SSH options).
- Edge cases and failure handling: approval rests on the Gateway's own SSH key reaching the node host with a known host key; anyone who can make that host print a matching JSON line (for example a shell profile, since `sh -l` is used) can satisfy the probe. Role or scope upgrades never auto-approve.
- Round 1: `confirmed` (SSH-verified, default on, compares device id and key).
- For rizzy-harness: we do not need this; our one-time enrolment code is simpler. If we ever auto-approve, bind to a key proof, not a network position.
- Confidence: `traced end to end`. (checked in review)

#### Node command policy is enforced on the Gateway and, for `system.run`, again on the node
- What the code does: before `node.invoke` the Gateway requires the command to be in its allowlist, not private, and declared by the node. On the node host, `system.run` re-evaluates the node's own runtime config and exec-approvals file (security mode, ask mode, allowlist, safe bins) and refuses with `SYSTEM_RUN_DENIED` or `SYSTEM_RUN_DISABLED`; a delayed approval is rejected if the node's policy snapshot changed.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-command-policy.ts#L460-L480 (Gateway allowlist and declaration check); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L334-L357 (node-side policy from its own config, snapshot check); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L13-L26 (node policy layering from local config).
- Edge cases and failure handling: other node commands were not traced for node-side checks.
- Round 1: `corrected`. Round 1 and the synthesis say the Gateway allowlist is the gate; for `system.run` the node also decides from its own file. This answers Round 2 question 4: both.
- For rizzy-harness: OpenClaw is the one peer with execution-side refusal; its "policy snapshot changed, approve again" check is worth copying for delayed approvals.
- Confidence: `read the definition`. (checked in review)

#### Cloud workers: the box is enrolled as a node, then a worker process dials the Gateway over WebSocket
- What the code does: a provisioned environment receives a setup code (bootstrap token, profile `cloud-worker`) whose redemption binds the device to the environment inside the pairing transaction. Node enrollment resolves the Gateway's public URL and refuses loopback, link-local or unspecified hosts because "Cloud workers call back over the public network". The worker process's endpoint is built from the node's Gateway URL plus `/__openclaw__/worker`; it is `ws:` or `wss:` (optionally with a pinned TLS fingerprint or Cloudflare Access headers) or a Unix socket. The worker reconnects with exponential backoff (250 ms to 30 s).
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/device-pairing-cloud-worker.ts#L29-L89 (setup completion bound to the provisioning environment); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/node-enrollment.ts#L85-L106 (public URL required); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/node-worker-supervisor-commands.ts#L173-L204 (worker endpoint from the node's Gateway URL); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-endpoint.ts#L45-L86 (endpoint schema: no credentials in URL, TLS pin only with wss); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection.ts#L58-L63 (reconnect backoff).
- Edge cases and failure handling: the Gateway also opens outbound SSH to the box for bootstrap and workspace sync, with pinned options (`-F none`, `StrictHostKeyChecking=yes`, own known-hosts file, `IdentityAgent=none`, no agent or X11 forwarding). https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291 (SSH options). So the worker's protocol link is outbound from the worker, but the control plane also dials in over SSH.
- Round 1: `corrected`. Round 1 inferred outbound-only; the agent link is worker-to-Gateway WebSocket, but provisioning and workspace transfer are Gateway-to-worker SSH.
- For rizzy-harness: our node dials the master and nothing dials the node; keep it that way. Copy the endpoint validation (no userinfo, no query, TLS pin requires `wss`).
- Confidence: `traced end to end` for the endpoint; `read the definition` for enrollment. (checked in review)

#### Per-dispatch worker credential: 32 random bytes, SHA-256 with a domain tag and the turn claim, 10-minute TTL
- What the code does: a credential is `generateSecureToken(32)`, registered for log redaction, and stored only as `sha256Base64Url("openclaw-worker-credential-v1\0" + credential)`, or, for a session turn, `sha256(domain-v1-turn \0 serialized turn claim \0 credential)`, so the hash binds the secret to the exact durable claim. TTL is 10 minutes. There is one credential row per environment (primary key `environment_id`), so minting a new one replaces the old; minting first cancels the environment's in-flight inference.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential.ts#L9-L60 (TTL, domains, hash, generation); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential-broker.ts#L105-L126 (mint: cancel inference, renew with expected owner epoch); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-state-schema.sql#L2598-L2608 (credentials table, one row per environment).
- Edge cases and failure handling: `renewCredential` throws if the owner epoch changed, the environment is not `ready`/`idle`/`attached`, destroy was requested, or there is no bootstrap receipt. https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/store.kernel.ts#L382-L421 (renew preconditions). A credential cannot be marked delivered after it expired (L440-L442).
- Round 1: `confirmed` (new credential per dispatch, only hash stored), with the binding to the turn claim `new (not documented)`.
- For rizzy-harness: this is the template for per-session link credentials: hash with a domain tag and the binding, one row per target so rotation is an overwrite.
- Confidence: `traced end to end`. (checked in review)

#### Worker admission checks credential, environment, build, session, owner epoch, RPC set and features, in that order
- What the code does: the worker's first frame must be the worker `connect` schema with `role: worker` and an `admission` block (environment id, credential, owner epoch, RPC set version, build handshake, session id and run id, both null or both set). The Gateway hashes the presented credential and compares in constant time with the stored hash, then requires: environment state `ready`/`idle`/`attached` and no destroy; bundle hash equal in credential, bootstrap receipt and the Gateway's expected build; same OpenClaw version; same session; same owner epoch in admission, credential and environment; RPC set version 1; identical protocol feature lists. For a session turn the durable turn claim must match session, run, environment and epoch.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L137-L170 (admission and connect schema); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L79-L186 (admission checks); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/worker-turn-rpc.ts#L632-L690 (preflight, build preparation, second admission, turn validation).
- Edge cases and failure handling: admission is serialized per client IP and only failures are charged to the rate limiter; an `environment-unavailable` outage is not charged. On the public route every rejection reason is reported to the worker as the opaque `invalid-handshake`. https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L218-L262 (serialized, charged, opaque rejection). The worker receives a minimal `worker-hello-ok`, never the general Gateway snapshot. https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L177-L194 (hello shape).
- Round 1: `confirmed` and refined.
- For rizzy-harness: copy the opaque public rejection and "charge only failures"; binding admission to the exact build hash is also cheap for us.
- Confidence: `traced end to end`. (checked in review)

#### Owner-epoch fencing: a global, never-reused generation, and revalidation on every worker request
- What the code does: an environment's `owner_epoch` is set to `max(all environment epochs, all transcript commit-head epochs) + 1` on a transition that attaches a session or ends an owner. The credential-revoking transitions are to `attached`, `attached`→`idle`, `draining`, `destroyed`, `failed`, `orphaned`, or clearing the bootstrap receipt, and all of them delete the credential; the epoch bump applies to those only when the source state is `ready`, `idle` or `attached`, or when the transition accepts an attached credential (`ownerEndingTransition` and `acceptsAttachedCredential` in `store-transitions.ts` L274-L292). A failure out of an earlier state such as `bootstrapping` revokes the credential without bumping the epoch (corrected in review). Minting a credential with `renewCredential` does not bump the epoch; it only requires the epoch to be unchanged. Every worker request re-reads the credential and environment: a different credential hash closes with `credential-replaced`, a changed epoch with `owner-epoch-mismatch`, expiry with `credential-expired` (except for an exact durable turn). The worker treats `credential-replaced` and `owner-epoch-mismatch` as terminal ("fenced") and stops reconnecting.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/store-mutations.ts#L30-L50 (global epoch, never reused because commit identity is session, epoch, seq); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/store-transitions.ts#L303-L350 (which transitions bump the epoch and revoke the credential); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L218 (per-request revalidation); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/worker-turn-rpc.ts#L691-L716 (service validation, durable-turn exception); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-contract.ts#L22-L26 (fenced reasons stop the worker).
- Edge cases and failure handling: a socket whose client was invalidated is closed on its next frame (`credential-replaced`), and a timer at credential expiry re-runs validation and closes unattached workers. https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L326-L351 (expiry timer and invalidated client).
- Round 1: `confirmed` (credential rotation plus owner-epoch fencing).
- For rizzy-harness: this is the mechanism to copy for "fence stale nodes": a monotonic generation stored with the session's binding, checked on every write, never reused.
- Confidence: `traced end to end` (corrected in review: the epoch bump is limited to transitions from ready, idle or attached, and a credential mint does not bump it)

#### Worker transcript commits are sequenced per (session, epoch) with a durable ledger and cached replies
- What the code does: a worker sends `worker.transcript.commit` with `runEpoch`, `seq`, `baseLeafId` and messages. The Gateway hashes the request, opens or replays a ledger row in `worker_transcript_commits` keyed `(session_id, run_epoch, seq)` (`pending` or `terminal` with the cached result), and applies the batch inside one agent-database transaction: it re-checks session id and lifecycle revision, compares the transcript version (generation, raw seq, updatedAt) with the prepared plan (`stale-base-leaf` on mismatch), and appends messages with idempotency keys derived from session, epoch, seq and index. The authority check (`assertCurrent`) runs before reservation, inside the write and before the ACK cursor update.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/worker-turn-rpc.ts#L279-L320 (commit under per-environment lock, assertCurrent, ACK cursor); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/transcript-commit.runtime.ts#L266-L307 (request hash, writer fence, derived idempotency keys); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/transcript-commit.runtime.ts#L324-L366 (ledger begin, replay, reject, discard on refusal); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/transcript-commit.kernel.ts#L41-L59 (in-transaction session and version checks); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-state-schema.sql#L2610-L2640 (commit heads and commit ledger).
- Edge cases and failure handling: a lost ACK replays the cached terminal result; a settled persistence failure returns `retryable: false` so it "must not masquerade as a lost ACK and replay forever". https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L503-L519 (replayable versus settled commit errors).
- Round 1: `new (not documented)`.
- For rizzy-harness: our node is the only writer, so we do not need the ledger, but the "cache the terminal reply under (epoch, seq)" pattern fixes duplicate appends on link retries.
- Confidence: `traced end to end`.

#### The writer claim: a field on the session row, set by compare-and-swap, checked inside every fenced transcript write
- What the code does: a run claims a session by writing `activeWriterRunId = runId` with the expected session id and lifecycle revision; if another run held the claim, that run is superseded only after the new claim is durable. A transcript write captures `expectedWriterRunId` from the run's async context and, inside the SQLite write transaction, re-reads the session row and refuses with `session-rebound` unless `entry.activeWriterRunId === expectedWriterRunId` (and session id and lifecycle revision match). A refusal under a writer fence throws `SessionTranscriptWriterClaimReboundError`. Release is a CAS `writer-revoke` that expects the same run id.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/session-bootstrap.ts#L543-L590 (claim by CAS, supersede after commit); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/transcript-write-context.ts#L549-L561 (fence attached to the write scope); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-transcript-write-snapshot.ts#L104-L130 (read the row inside the write, refuse); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-scope.ts#L79-L97 (the comparison); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-transcript-write-snapshot.ts#L177-L179 (refusal throws); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/session-writer-revocation.ts#L28-L45 (CAS revoke).
- Edge cases and failure handling: a write whose scope has no expected writer skips the writer comparison (`scope.expectedWriterRunId === undefined` passes), so protection depends on every writer carrying its fence. The session id is still compared, and the lifecycle-revision and owner comparisons still apply when the scope sets them. The fence is not passed by hand: `withOwnedSessionTranscriptWriterFence` attaches it from the run's async context (`transcript-write-context.ts` L549-L561), so a write made outside that context is the unfenced case (refined in review). The claim is last-writer-wins takeover, not a lock: a newer run evicts the incumbent.
- Round 1: `corrected`. Round 1 said the claim is checked "in the same transaction as every transcript write" (Distinctive) and the review narrowed it to "the commit transaction verifies it". The code: verified in the write transaction, but only for fenced writes, and acquiring the claim supersedes the holder rather than waiting.
- For rizzy-harness: our session actor gives one writer by construction; the useful idea is the storage-level check as defence in depth, and to make the unfenced path impossible rather than optional.
- Confidence: `traced end to end` (corrected in review: the unfenced write still checks session id, and the fence is attached implicitly from the async context)

#### Inference is brokered by model reference; the worker never names an endpoint or a key
- What the code does: `worker.inference.start` carries `modelRef` (provider, model), context (system prompt, messages, tools) and options. The Gateway resolves the ref against the agent's model visibility policy and catalog and refuses with `model-not-approved` if it is not allowed or the provider changes after alias resolution, then runs inference with Gateway-held auth and streams events back. Pending and terminal inference turns are persisted for replay. A separate worker-local inference mode keeps endpoint and credentials on the node; the Gateway still approves the model ref.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-inference.ts#L80-L131 (model ref, context, options, start params); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-model.ts#L34-L89 (approval against catalog and policy); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-model.ts#L91-L124 (worker-local inference: credentials only on the node); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-state-schema.sql#L2642-L2660 (inference turn ledger).
- Edge cases and failure handling: minting a new credential cancels in-flight inference for the environment (credential-broker L109-L112).
- Round 1: `confirmed`, with worker-local inference `new (not documented)`.
- For rizzy-harness: option (c) of the credentials decision is real and shipped here, and OpenClaw also kept (a) as a mode. That suggests supporting both rather than choosing once.
- Confidence: `read the definition`. (checked in review)

#### GitHub: the worker does receive a GitHub token
- What the code does: the worker launch descriptor has an optional `github` block with a `token` (up to 2,048 chars), login, branch and remote URL; heartbeats can return a refreshed `{generation, token, expiresAtMs}`. The worker writes the token into a per-turn `gh` profile directory, resets inherited credential helpers to `gh auth git-credential`, and registers the token for log redaction.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/launch-descriptor.ts#L183-L211 (GitHub launch schema with token); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L210-L221 (heartbeat GitHub refresh); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/github-binding.runtime.ts#L159-L198 (token written to a per-turn profile, helpers reset).
- Edge cases and failure handling: on Windows the profile is dropped if not owner-only (L199-L211); on POSIX that check was not seen in the lines read.
- Round 1: `corrected`. Round 1 says "GitHub publication via a Gateway broker", implying no GitHub credential on the worker. The Gateway does broker publication flows, but a turn-scoped GitHub token reaches the worker and its agent's shell.
- For rizzy-harness: if a node ever pushes on a user's behalf, hand it a short-lived, repository-scoped token and expect the agent to be able to read it.
- Confidence: `read the definition`. (checked in review)

#### The worker method allowlist is closed and feature-gated per build
- What the code does: the worker socket accepts only `worker.heartbeat`, `worker.transcript.commit`, `worker.live-event`, `worker.computer`, the exec-approval method, the gateway-tool invoke and cancel, the two inference methods and turn admission. Each call also requires the matching protocol feature in the admitted build and passes schema validation; anything else closes with `method-not-allowed`. Frames are capped (16 queued, 32 MiB queued bytes), method names at 64 chars, and long tool operations at a fixed concurrency.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L54-L66 (RPC set and method list); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L69-L75 (queue caps and method set); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection-dispatch.ts#L112-L131 (revalidate ownership, feature gate); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection-dispatch.ts#L263-L266 (default reject).
- Edge cases and failure handling: the gateway-tool methods let the worker ask the Gateway to run Gateway-side tools for its turn, so the surface is closed but not small.
- Round 1: `confirmed`.
- For rizzy-harness: our node-to-master message set should be a closed list with a default reject, the same way.
- Confidence: `traced end to end`. (checked in review)

#### Disconnect: the worker reconnects with the same claim-bound secret; tool effects are reported unknown
- What the code does: an exact durable turn may stay connected and reconnect with the same claim-bound credential past the 10-minute TTL until the terminal ACK releases its placement claim. Requests whose owner keeps settlement reuse the same call id after reconnect. When the node is lost mid-turn the error says tool effects may be unknown and the turn cannot continue automatically.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L326-L339 (TTL fences unattached workers only); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection.ts#L255-L269 (replay with the same call identity); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-node-loss.ts#L3-L15 (node-lost error, `canContinue = false`).
- Edge cases and failure handling: a `gateway-unavailable` reply during startup is retryable with a retry-after; fenced reasons are terminal. https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L157-L187 (startup unavailable, protocol mismatch).
- Round 1: `new (not documented)`.
- For rizzy-harness: copy the "effects unknown, verify before retrying" message and never auto-continue a turn after a lost node.
- Confidence: `read the definition`. (checked in review)

### Headless and scheduled operation

#### Delivery queue recovery refuses blind replay of a send that may have happened
- What the code does: outbound messages are queued in `delivery_queue_entries` with `retry_count`, `recovery_state` and `platform_send_started_at`. On recovery, an entry whose send may have started is replayed only if the channel adapter's reconciliation proves it was not sent, or it is a plain final text explicitly marked as safe to retry after an ambiguous transport error; otherwise it fails with "refusing blind replay without adapter reconciliation". Default max retries is 5; a fixed list of error patterns (chat not found, bot blocked) is permanent.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-state-schema.sql#L1557-L1579 (queue table); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/outbound/delivery-queue-recovery.ts#L597-L625 (replay only on proof or safe text); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/outbound/delivery-queue-recovery-policy.ts#L4-L34 (retry default, safe final text rule); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/outbound/delivery-queue-recovery-policy.ts#L55-L77 (permanent errors).
- Edge cases and failure handling: an unresolved but retryable reconciliation counts a failure and retries later (L617-L623).
- Round 1: `new (not documented)`.
- For rizzy-harness: for any outbound effect (notifications, webhooks) record "send started" before the call and do not replay without proof.
- Confidence: `read the definition`.

#### Cron restart recovery: restore finished runs, retry an interrupted agent turn only if delivery had not started
- What the code does: on startup, a run whose task finalized is restored without rerun. Otherwise the run is marked interrupted (error, delivery `unknown`), and a replacement time is set only for an `agentTurn` job with an `at`, `every` or `cron` schedule, a scheduled receipt, `deliveryNotStarted === true`, an unchanged config revision and no pending schedule change; the delay is the minimum refire gap for `at` or the error backoff. A started one-shot with no replacement is disabled.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/cron/store/run-recovery.kernel.ts#L191-L239 (restore or mark interrupted, retry condition); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/cron/service/jobs-scheduling.ts#L31-L49 (retry time, agentTurn only); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/cron/service/startup-run-repair.ts#L30-L75 (interrupted state, one-shot disabled).
- Edge cases and failure handling: no retry counter was found. What stops repetition is the gate on `deliveryNotStarted` and the config revision; a run that crashes again before delivery would qualify again.
- Round 1: `corrected`. Round 1 says "interrupted time-scheduled agent-turn jobs get one retry"; the code has no "one" counter, only conditions. This answers the Round 2 question "how is one retry recorded so it cannot repeat": it is not recorded as a count.
- For rizzy-harness: if we add routine restart retry, store an attempt count on the receipt.
- Confidence: `read the definition`. (checked in review)

#### Heartbeat is a default cron job every 30 minutes
- What the code does: the default proactive job is `every` 30 minutes, delivers `announce` to the `owner`, and its prompt tells the agent to use `NO_REPLY` when there is nothing to say. It is created once per owner; a deletion stays a deletion.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/cron/default-proactive-job.ts#L23-L50 (prompt, cadence, delivery); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/cron/default-proactive-job.ts#L71-L72 (created once).
- Round 1: `confirmed`.
- For rizzy-harness: nothing to copy now (feature freeze).
- Confidence: `read the definition`.

#### Webhooks: header token only, rate-limited failures, and an in-memory idempotency cache
- What the code does: `?token=` is rejected with 400; only POST; the token from `Authorization: Bearer` or `X-OpenClaw-Token` is compared with `safeEqualSecret`; failures are rate-limited per client (429 with Retry-After) and loopback is not exempt. An `Idempotency-Key` maps to a replay cache keyed by hashes of token, path scope and key, held in a process `Map`.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/hooks-request-handler.ts#L100-L112 (in-memory cache, limiter with `exemptLoopback: false`); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/hooks-request-handler.ts#L142-L205 (replay key and cache); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/hooks-request-handler.ts#L220-L256 (query rejection, method, auth, throttling).
- Edge cases and failure handling: the idempotency cache does not survive a restart, so a retried webhook after a Gateway restart runs again. A config change mid-request returns 409.
- Round 1: `confirmed` (dedicated token, query rejected, 429); the in-memory dedupe is `new (not documented)` and explains the docs' "not exactly-once".
- For rizzy-harness: persist idempotency keys if we accept webhooks.
- Confidence: `traced end to end`.

### MCP as a client

#### MCP client: stdio, SSE and streamable HTTP, with OAuth per operator or per requester; tool search on by default
- What the code does: the transport factory turns server config into an SDK stdio, SSE or streamable-HTTP transport; HTTP servers can use an auth profile bearer or OAuth with an identity that is either the operator's or the requester's, and OAuth redirects use a stricter fail-closed fetch. Tool search is enabled when `tools.toolSearch` is unset (`enabled: setting ?? true`), mode `tools` by default (or `directory`), 8 results by default, 20 maximum.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/mcp-transport.ts#L82-L200 (transport selection and auth); https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-search-config.ts#L5-L25 (tool search defaults).
- Edge cases and failure handling: not traced (timeouts, tool-list size limits).
- Round 1: `confirmed` at the level Round 1 described; per-requester OAuth identity is `new (not documented)`.
- For rizzy-harness: per-requester OAuth is the shape we need for multi-user MCP on a shared master.
- Confidence: `read the definition`.

## 4. Round 1 claims checked

| Claim | Verdict | Citation |
|---|---|---|
| Worker dials the Gateway's public worker route | confirmed (WebSocket or Unix socket) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/node-worker-supervisor-commands.ts#L173-L204 |
| Worker connection is outbound-only (section 5) | corrected: Gateway also SSHes into the box | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291 |
| Closed method allowlist on the worker route | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L54-L66 |
| Each dispatch mints a credential, Gateway stores only the hash | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential.ts#L41-L60 |
| Owner-epoch fencing rejects stale workers | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L218 |
| Provider keys stay on the Gateway, inference by model ref | confirmed, plus a worker-local mode | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-model.ts#L91-L124 |
| GitHub publication via a Gateway broker (no GitHub secret on worker implied) | corrected: worker receives a GitHub token | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/launch-descriptor.ts#L183-L211 |
| Writer claim checked in the same transaction as every transcript write | corrected: checked in the write transaction only for fenced writes; claim supersedes the holder | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-scope.ts#L79-L97 |
| Per-node token store retired, `node.pair.request`/`verify` removed | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/node-pairing-migration.ts#L1-L8 |
| Every client and node signs the `connect.challenge` nonce | confirmed (when a device block is sent; shared-auth operators may skip device identity) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/role-policy.ts#L13-L16 |
| SSH-verified auto-approval, default on, compares device id and key | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-pairing-ssh-verify.ts#L190-L240 |
| 10-minute bootstrap token | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/device-bootstrap.worker-types.ts#L5-L5 |
| Gateway allowlist is the gate for node commands | corrected: node host also enforces its own policy for `system.run` | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L334-L357 |
| Interrupted time-scheduled agent-turn jobs get one retry | corrected: conditional retry, no counter | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/cron/store/run-recovery.kernel.ts#L211-L226 |
| Heartbeat replaced by a 30-minute proactive job to the owner | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/cron/default-proactive-job.ts#L30-L50 |
| Webhook token via header, query rejected, 429 on failures | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/hooks-request-handler.ts#L220-L256 |
| `pruneAfter` 30d, `maxEntries` 5000 | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/store-maintenance.ts#L26-L34 |
| Control UI token in session storage per tab | corrected: shared token yes; device key and device tokens in localStorage | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/lib/nodes/index.ts#L112-L126 |
| Accepted input stored before the Gateway acks | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L897-L914 |
| Restart resume injects one resume input | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/session-restart-resume.ts#L78-L85 |
| Managed worktrees snapshot to `refs/openclaw/snapshots/<id>` | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/worktrees/snapshot-inventory.ts#L431-L434 |

## 5. In the code but not in the documentation

- The worker credential hash binds the secret to the serialized durable turn claim with a domain tag (credential.ts L41-L47).
- The owner epoch is a global counter that also accounts for transcript commit heads, so a generation is never reused across environments (store-mutations.ts L30-L50).
- A durable transcript-commit ledger and inference-turn ledger give at-least-once replay with cached terminal replies, keyed by (session, epoch, seq).
- Worker admission is bound to the exact build hash, version and protocol feature list; on the public route all rejection reasons are reported as `invalid-handshake`.
- Device tokens and bootstrap tokens are stored in plaintext in `openclaw-state.sqlite`; only worker credentials and browser pairing approvals are hashed.
- The Control UI keeps the browser's Ed25519 private seed and device tokens in localStorage.
- The node host evaluates `system.run` against its own config and approvals file, with a policy-snapshot check on delayed approvals.
- A worker-local inference mode keeps the model endpoint and key on the node while the Gateway still approves the model ref.
- The worker gets a turn-scoped GitHub token, refreshed through heartbeats.
- The webhook idempotency cache is in memory only.
- Delivery queue recovery requires adapter reconciliation before replaying a possibly-sent message.
- A Rust node host crate (`crates/openclaw-node-host`, MIT) exists alongside the TypeScript one.
- Session retention includes a 10 GiB disk budget with an 80% high-water mark.

## 6. Could not find or could not settle

- How `steer` is injected into an active run (Round 2 question): `src/sessions/session-controller.steer.ts` admits the input "behind older reservations" and calls a backend `messageInjectionV2.steerNow`; the backend side was not traced. Reading the embedded runner's message-injection backend would settle it.
- Whether the GC removal path always snapshots before deleting a worktree: the owner-protection guard was read, the removal function (`createWorktreeGcRemoval`) was not.
- Session visibility (`none/view/suggest/write`) enforcement on the WebSocket, `openclaw mcp serve` authentication, and the operator scope ceiling for named roles: not reached in this pass; `src/gateway/session-sharing-*.ts` and `src/mcp/channel-server.ts` are the places to read.
- MCP limits (tool-list size, call timeouts) and how directory mode presents MCP tools: `src/agents/tool-search-directory.ts` not read.
- Whether node commands other than `system.run` are re-checked on the node host: only `system.run` was traced.
- The POSIX permission check on the worker's GitHub profile directory: only the Windows branch was seen.

## 7. Citation check

`openclaw.sessions.md: 137 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit`

## 8. Review

Checked 17 of the 31 findings against the pinned clone. Verdicts: 15 supported, 2 partly (corrected in the text above), 0 not supported, 0 wrong.

| Finding | Verdict | Reason |
|---|---|---|
| Retention defaults | supported | 30 days, 5,000 entries, 10 GiB, 0.8 high water, `enforce` mode |
| Device proof | supported | 2-minute skew, id derived from key, nonce equals challenge, signature versions |
| Gateway auth defaults | supported | `override ?? config ?? (password ? "password" : "token")`; 24-byte hex token generated |
| Control UI storage | supported | device identity seed and device tokens written to `localStorage` |
| Device tokens in plaintext | supported | token record holds the bearer string; compared with `safeEqualSecret` |
| SSH-verified auto-approval | supported | `BatchMode`, `StrictHostKeyChecking=yes`, `sh -lc 'openclaw node identity --json'`, enabled unless `false`; the "shell profile can satisfy the probe" remark is an inference, not traced |
| Node command policy | supported | Gateway `isNodeCommandAllowed` and node-side `system.run` re-evaluation |
| Cloud workers: enrolment, endpoint, SSH | supported | endpoint schema (no userinfo, no query, TLS pin needs `wss`); `workerSshOptions` is shared by bootstrap, tunnel control and workspace transfer, so the Gateway does open SSH to the worker box; this also corrects the safety file's "never dials" |
| Per-dispatch credential | supported | 32 bytes, domain-tagged SHA-256 with the turn claim, 10-minute TTL (`WORKER_CREDENTIAL_TTL_MS`), one row per environment, plaintext never in durable state, mint cancels in-flight inference |
| Worker admission | supported | check order and constant-time compare confirmed |
| Owner-epoch fencing | partly | global `max + 1` and per-request revalidation confirmed; the bump applies only from `ready`/`idle`/`attached` (or when accepting an attached credential), and a credential mint does not bump it; the durable-turn expiry exception was already in this file |
| Writer claim | partly | CAS on session id and lifecycle revision (not on the previous writer), takeover by supersede after the claim is durable, comparison inside the write transaction confirmed; the unfenced case still checks session id and the fence is attached from the async context |
| Inference by model reference | supported | `modelRef` approval, `model-not-approved`, worker-local mode in `inference-model.ts` |
| GitHub token on the worker | supported | `GitHubLaunchSchema.token` up to 2,048 chars, heartbeat refresh up to 4,096, per-turn profile directory; POSIX permission check not seen (Windows only), as stated |
| Closed method allowlist | supported | method set, queue caps 16 and 32 MiB, feature gate, default reject |
| Disconnect and reconnect | supported | durable-turn expiry exception, `WorkerNodeLostError` with `canContinue = false` |
| Cron restart recovery | supported | retry gated on `deliveryNotStarted`, config revision and no schedule change; no counter |

Answers to the specific questions in the review request:

- Owner epoch: computed as `max(all environment epochs, all transcript commit-head epochs) + 1` (`nextGlobalOwnerEpoch`), so it is global and never reused; bumped on transitions out of `ready`/`idle`/`attached` that revoke the credential, and when attaching; not bumped by a credential mint or by transitions out of earlier states.
- Per-dispatch credential: 32 random bytes, 10-minute TTL, only the domain-separated SHA-256 stored (bound to the turn claim for a session turn), plaintext kept in memory until delivery is acknowledged. Confirmed.
- Writer claim: set by CAS on session id and lifecycle revision, re-read inside the write transaction, refused as `session-rebound` on mismatch; writes with no expected writer skip the writer comparison. Confirmed with the refinements above.
- The Gateway also connects to the worker box over SSH: confirmed (bootstrap, tunnel control, workspace transfer).
- The worker receives a GitHub token: confirmed.

Not checked: session storage schema, pending inputs, fork, restart resume, worktree snapshots and GC, repository checkpoint refs, protocol frames, roles and scopes, node and device pairing methods, transcript commits, delivery queue recovery, heartbeat job, webhooks, MCP client. Overall: the checked findings, which cover the high-risk worker and credential mechanisms, were accurate, and the two corrections were refinements of scope. The unchecked remainder is mostly schema and constant reading and can be trusted at the same level. The citation check was re-run after editing.
