# 05. The control side and the execution side: links, workers and client protocols

Round 2 synthesis, research date 2026-11-10. Source: the Round 2 per-repository files in
`research/round2/` (each pinned to the commit in `00-repos.tsv`). This file adds no finding
that is not in those files; where two input files disagreed, section 1 says how it was settled.

## 1. Scope

**The group.** Every place where the code splits a *controlling* process (the one that owns
the conversation, the user interface or the authority) from an *executing* process (the one
that runs the agent loop, the tools, or both): gateways and cloud workers, remote executors
and exec servers, SSH daemons and helpers, hubs and their clients, remote workspaces, relays
and tunnels. For each: who dials, over what transport, the handshake, pairing and identity,
what the controlling side sends and what the executing side may refuse, how stale workers are
fenced, per-dispatch credentials, reconnect and resume, back-pressure, and the protocols
between user interfaces and engines (JSON-RPC app servers, HTTP plus WebSocket, envelope
protocols, the Agent Client Protocol), with the authentication of every listener.

Terms used throughout:

- **Fencing**: refusing work from a process that used to hold a role (owner of a session,
  attached worker) after another process took it over. A *fencing token* is the counter or
  id that the receiver checks.
- **Epoch / generation**: a counter that increases each time ownership changes; work stamped
  with an older value is refused.
- **Writer claim**: a field naming the one process allowed to append, set by compare-and-swap
  (CAS: "write the new value only if the old value is still X").
- **Durable frame**: a message that carries a sequence number, is persisted, and is replayed
  after a reconnect. **Transient frame**: a message (streaming text, progress, snapshot) that
  is never persisted and never replayed.
- **ACP**: the Agent Client Protocol, JSON-RPC over stdio between an editor ("client") and
  an agent ("agent").

**Files read.** For every repository: the architecture section, every finding about remote
execution, client/server split, transports, authentication of listeners, pairing, resume and
reconnect, event streaming, ACP, SSH back ends, relays and credentials sent to workers, plus
section 4 (Round 1 claims) and section 8 (review) of each file. Concretely:
`codex.sessions.md` 3.1, 3.3, 3.4, 3.5, 3.6; `codex.safety.md` socket isolation, remote
exec-server, credential broker, 3.8; `opencode.sessions.md` client/server, pairing, durable
log, crash resume, remote workers, PTY daemon, SDK; `opencode.safety.md` workspace drivers,
server password; `openclaw.sessions.md` client/server, remote workers, headless;
`openclaw.safety.md` "Running commands somewhere else", approval forwarding, secrets;
`openclaw.agents.md` ingress and announce; `hermes-agent.sessions.md` client/server, gateway
reliability, Kanban, other client protocols; `hermes-agent.safety.md` SSH and cloud
sandboxes; `oh-my-pi.sessions.md` ownership lease, client/server, remote workers, auth
broker/gateway; `oh-my-pi.safety.md` ACP, SSH, broker, gateway; `pi.sessions.md` RPC, pi-env,
p2; `pi.safety.md` pi-env, durable resume; `deepseek-harness.sessions.md` durability, lease,
resume, client/server, remote workers, headless; `deepseek-harness.safety.md` 20, 23-25;
`deepseek-harness.agents.md` external harnesses; `goose.sessions.md` resume, client/server,
remote workers, roaming; `goose.agents.md` and `goose.safety.md` ACP providers;
`cline.agents-sessions.md` hub, protocol, SSH, core/host split; `cline.safety.md` defaults,
hub approvals, SSH, 3.11; `openhands.agents-sessions.md` sessions client/server, remote
workers, client tools, ACP; `openhands.safety.md` server-level shell, Docker bind, session
key, WebSocket auth, accounts; `zed.md` 3.4 and the external-agent permission finding;
`warp.md` 2, 3.3-3.5, 3.6, 3.8; `opencodex.md` 1, 4, 6, 13, 15, 16, 17, 29-37.

Our own state was taken from `research/round1/own-rizzy-harness.md`,
`docs/nodes-credentials-environments.md` and the brief (pull requests 4, 6, 7).

**What was not available.**

- OpenHands: the agent loop, Agent Server (`session_protocol.py`), persistence and the ACP
  spawn live in `OpenHands/software-agent-sdk`, not cloned. Everything below about OpenHands
  is the browser client (Canvas) and its launchers.
- Warp: the agent loop and model call run on Warp's server (wire schema in
  `warpdotdev/warp-proto-apis`), and the self-hosted worker is `warpdotdev/oz-agent-worker`;
  neither was cloned. What is here is the client: tool executor, policy gate, ACP client,
  orchestration plumbing.
- Codex: the registry, relay and remote-control enrolment service are hosted services; their
  server side is not in the repository. The Codex Desktop "Handoff" is closed source.
- OpenCode: the `--remote` tunnel's encryption lives in the external `@opentunnel/client`.
- Hermes: the relay connector's buffering and persistence are not in the repository.
- Oh My Pi: the real collab relay is not in the repository (only an offline stand-in).
- DeepSeek harness: the LiveShare relay server is not in the repository.
- Zed: the ACP schema is the crates.io dependency `agent-client-protocol`, not vendored.

**Contradictions between input files, and how they were settled.**

1. *OpenClaw: does the Gateway ever dial the worker box?* The first draft of
   `openclaw.safety.md` said "the Gateway never dials it"; `openclaw.sessions.md` said the
   Gateway SSHes into the box. The reviewer of both files settled it: the protocol link is
   worker-to-Gateway WebSocket, and the Gateway also opens SSH to the box for bootstrap,
   tunnel control and workspace transfer. The safety file was corrected. This file uses the
   corrected version.
2. *Pi: does the pi-env daemon apply any policy?* `pi.sessions.md` did not read the Rust
   daemon source and left daemon policy open; `pi.safety.md` read it and its reviewer
   confirmed by search that there is none (no seccomp, chroot, rlimit or allow-list). The
   reviewer also noted that both Pi files wrongly said `durable` depends on `pi-env`; the
   reverse holds. This file uses the safety file's version.
3. *Codex: is every non-loopback WebSocket listener refused without auth?* Both Codex files
   first said yes; review corrected both: true for the app-server listener only, while the
   exec-server `--listen ws://IP:PORT` listener has no such refusal.
4. *Cline: can an Origin-only hub client read the token anyway?* The first draft of
   `cline.agents-sessions.md` said the caller would need to read the 0600 token file; review
   corrected it (loopback TCP is open to any local process, so the file permission does not
   protect the Origin path). Review states that the two Cline files otherwise agree.
5. *goose: do OS child processes survive a roaming revocation?* `goose.sessions.md` could
   not settle it; its reviewer answered from the safety cluster: a cancelled run drops the
   tool future and `ShellProcess::drop` kills the process group, so shell children are
   killed, but a `setsid` child or an MCP server child is not covered.

## 2. Implementations by project

Trust labels on each account: **[checked in review]** (a reviewer opened the cited code and
agreed), **[corrected in review]** (a reviewer changed it; the corrected text is used),
**[unchecked]** (the author's reading only), **[inferred]** (the author's inference, not
traced), **[absence confirmed]** (a negative claim a reviewer searched for).

### Codex

Codex has four control/execution splits. The **app-server** (JSON-RPC) owns the agent loop
for every loaded thread; the TUI and `codex exec` talk to it in process, IDEs and SDKs talk to
a separate `codex app-server` over stdio, a Unix socket or a WebSocket. An **exec-server**
can host the tools on another machine while the loop stays in the harness. **Remote
control** makes the local app-server dial a vendor relay so phones can reach it. A
**background daemon** keeps one app-server alive behind a private Unix socket.

**App-server protocol and transports.**

1. One Rust macro defines every method, its wire name, params, response and a per-request
   serialisation scope (`thread_id(...)`, `global("key")`, shared read); TypeScript and JSON
   Schema are generated from the same types and a checked-in `schema/` tree is compared by
   tests. Experimental methods are rejected unless the connection declared `experimentalApi`
   at `initialize`; `initialize` may be sent once.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/common.rs#L522-L530
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/common.rs#L129-L139
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/request_serialization.rs#L25-L60
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/message_processor.rs#L1040-L1052
   **[unchecked]**
2. Transports: stdio JSONL (default), `unix://` (WebSocket framing over a private Unix
   socket), `ws://IP:PORT` (axum), and remote control (outbound). WebSocket auth modes are
   `capability-token` (a token file or its SHA-256, constant-time compare) and
   `signed-bearer-token` (HS256 JWT, secret of at least 32 bytes, optional issuer and
   audience, 30 s skew). Auth is checked once, at the HTTP upgrade, before `initialize`. Any
   request with an `Origin` header gets 403 (health checks included). Credentials are loaded
   once at startup, and the source says expiry and revocation are not enforced after the
   socket is up: a JWT that expires mid-connection stays connected.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L115-L130
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L99-L113
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L286-L330
   **[checked in review]**
3. Non-loopback listener refusal: `start_websocket_acceptor` returns `InvalidInput` for a
   non-loopback bind with no auth; the former `--allow-unauthenticated-non-loopback-ws` flag
   was removed. This applies to the app-server only. The exec-server's
   `--listen ws://IP:PORT` listener never calls the predicate, its module comment says
   "unauthenticated listeners remain supported for compatibility", and it serves process and
   file RPCs. Loopback `ws://` with no auth is allowed for both, so any local process can
   drive the agent.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L160-L175
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L279-L284
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/lib.rs#L866-L876
   **[corrected in review]**
4. Back-pressure: each connection feeds a bounded `mpsc` of 128. A full queue answers a
   *request* with `{code:-32001, "Server overloaded; retry later."}` (dropped with a warning
   if the outbound queue is also full); non-request messages (responses to server requests,
   notifications) wait on an awaited `send` instead. The WebSocket outbound queue is 32,768
   entries; the in-process client uses an unbounded event queue so unread notifications
   cannot block responses. No `retry_after` is sent.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/mod.rs#L231-L262
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L55-L56
   **[unchecked]** in the sessions file; the safety file's version of the same finding
   (capacity 128, `try_send`) is **[checked in review]**.
5. A connection gate closes when its auth is no longer current, rejecting queued handlers
   while started ones finish.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/connection_rpc_gate.rs#L11-L50
   **[unchecked]**
6. "Human approval" capability: a connection may declare `requiredHumanApproval` at
   `initialize`; a `HumanCommand` approval is routed only to connections that declared it, and
   `exec` declares false, so a headless client structurally cannot answer it.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/outgoing_message.rs#L414-L430
   **[unchecked]**
7. Remote TUI: `--remote ws[s]://host:port` or `unix://`; a bearer token only from a named
   environment variable, and only over `wss://` or loopback `ws://`; Unix endpoints carry no
   token.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/cli/src/main.rs#L2649-L2676
   **[checked in review]**
8. Resume and rejoin: `thread/resume` takes a thread id, a rollout path, or inline history;
   if the id names a running thread the app-server rejoins it.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L340-L374
   **[unchecked]**
9. One writer per thread is an exclusive advisory file lock per thread, coordinated by a
   `.coordination.lock`; a second writer gets `WouldBlock`. It fences only processes that
   share one filesystem.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/writer_lock.rs#L45-L95
   **[checked in review]**

**Background daemon and the unauthenticated control socket.** `codex app-server daemon`
manages one detached process with a pid file. The control socket's real file lives in a
hashed path under a directory that must be 0700 and owned by the user; the socket is 0600 and
the advertised name is a symlink. A client accepts the socket only if the symlink target is
the expected protected path and the directory is 0700 and owned by its euid. The acceptor
does no authentication of its own (no peer-credential check); only the directory mode
protects it. A sandboxed command runs as the same uid, so bubblewrap hides the directory with
`--perms 000 --tmpfs ... --remount-ro` and the helper refuses legacy Landlock for any
filesystem-restricted policy ("requires bubblewrap to isolate app-server sockets"). Why
Landlock cannot do it is the author's inference (Landlock does not block `connect()` to an
existing Unix socket), not stated in code. On restart the daemon writes `loaded-threads.json`
and the next generation consumes it to restore interrupted turns.
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L71-L145
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L322-L352
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L148-L246
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L386-L393
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/bwrap.rs#L796-L830
**[corrected in review]** (socket isolation); daemon lifecycle and recovery snapshot
**[unchecked]**.

**Exec-server: the loop stays, the tools move.**

1. `codex exec-server` is a JSON-RPC tool host: `process/start|read|write|signal|terminate`,
   `fs/*`, `environment/info|status`, `http/request`, `capabilityRoots/discoverV1`. The
   harness holds an `Environment` per id and routes shell, `apply_patch`, file reads, skill
   discovery and MCP stdio launches through it. The executor does not call the model.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-protocol/src/protocol.rs#L25-L65
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/executor_process_transport.rs#L1-L20
   **[unchecked]**
2. **The controlling side sends the policy.** `process/start` carries a "portable sandbox
   intent"; the executor turns it into a wrapper for its platform. A request with no
   `sandbox` runs unsandboxed. No executor-side ceiling on what the harness asks for was
   found (executor-local requirements exist only for MCP). Credentials in the env are masked
   before spawn, failure mode `Warn`.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-protocol/src/protocol.rs#L374-L414
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L142-L172
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L215-L230
   **[checked in review]**; the missing ceiling is **[absence confirmed]**.
3. Who dials: `codex exec-server --remote <registry URL> --environment-id ID` registers
   with a hosted registry and keeps an outbound WebSocket to a relay (default transport
   `noise`). Registry auth: ChatGPT token from stdin, agent-identity token, AWS SigV4, a
   one-shot registration bearer from stdin, or rotating executor credentials read from stdin,
   with the process exiting when the pipe closes. Alternatively `--listen ws://IP:PORT`
   serves directly with optional auth (no refusal of an unauthenticated non-loopback bind),
   or the harness launches a `program` such as `ssh host codex exec-server --listen stdio`.
   The scoped executor skips telemetry exporters because they "may contain account-wide
   credentials".
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-cli/src/command.rs#L56-L160
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/remote.rs#L72-L88
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/environment_toml.rs#L120-L145
   **[corrected in review]**
4. Handshake: a Noise `hybridIK` handshake (X25519 + ML-KEM-768, AES-GCM, SHA-256) to the
   executor's static key, **pinned from the registry response**. The first message
   authenticates the harness key; the executor asks the registry whether that key is
   authorised (10 s timeout, 32 pending validations) before completing. Up to 128 streams
   multiplex over one relay WebSocket (protobuf frames with stream id, `ack`, `ack_bits`,
   `Data{seq, segment}`, `Resume{next_seq}`, `Reset`, `Heartbeat`, `Handshake`). After 8
   failed handshakes the executor cools down 10 s. Ciphertext is reordered within 64
   sequence numbers and 1 MiB; frames are capped at 256 KiB before parsing. The relay sees
   only ciphertext.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/noise_channel.rs#L1-L37
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/relay.rs#L640-L690
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/proto/codex.exec_server.relay.v1.proto#L5-L43
   **[unchecked]** (the safety file's shorter version, pinned key plus registry
   authorisation, is **[checked in review]**).
5. Reconnect and fencing: `initialize` can carry `resumeSessionId`. The executor keeps a
   `SessionEntry` (with its process table) per session and attaches at most one connection:
   a second attach gets `SESSION_ALREADY_ATTACHED`; after the 80 s `RECOVERY_RETENTION` it gets
   "unknown session id" and the processes are shut down. A disconnected session is
   `Detached{expires_at}` and its processes live until then. The client retries for 50 s and
   reads missed process output with `process/read` from a sequence cursor. **This fences a
   second client, not a stale one**: a stale harness still holding the connection blocks the
   new attach until the executor notices the disconnect. There is no epoch or fencing token
   on commands anywhere in exec-server.
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/server/session_registry.rs#L15-L130
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-protocol/src/protocol.rs#L90-L106
   https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/client_recovery.rs#L57-L62
   **[checked in review]**; no epoch **[absence confirmed]**.

**Remote control (app-server dials a vendor relay).** `codex remote-control` enrols the local
app-server with a ChatGPT-hosted service, receives a `remote_control_token` with
`expires_at`, and keeps an outbound WebSocket (protocol "3"). Messages are enveloped with
`client_id`, `stream_id`, `seq_id`, split into segments of about 100 KiB (max 150 KiB,
reassembled max 100 MiB, 1,024 segments); the socket resumes with an
`x-codex-subscribe-cursor` header. Idle clients drop after 10 minutes. The relay URL must be
a `chatgpt.com` / `chatgpt-staging.com` host or localhost, so a poisoned config cannot
redirect the token. Pairing-code expiry is enforced server-side, not in this repository.
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/protocol.rs#L21-L56
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/protocol.rs#L209-L226
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/segment.rs#L19-L23
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/websocket.rs#L70-L73
**[unchecked]**

**Headless.** `codex exec` is the same app-server in process with `approval_policy = Never`;
server-initiated approval, user-input and dynamic-tool requests are rejected and MCP
elicitations cancelled.
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec/src/lib.rs#L2228-L2300
**[checked in review]**

**Session moves.** No code in the open tree moves a chat with its Git state ("Handoff" is
closed-source Desktop). The nearest piece: `bind_thread` writes `codex-thread.json`
(`{version, ownerThreadId}`) into the worktree's Git metadata with a no-clobber rename and
fails if another thread owns the checkout.
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/worktree/src/metadata.rs#L1-L90
**[absence confirmed]** (for Handoff)

**Per-dispatch credentials.** None on the exec-server link beyond the registry
authorisation. The local network proxy has an off-by-default credential broker that puts
shaped dummy values in the child env and substitutes the real value only for the bound host
(it forces MITM on).
https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/network-proxy/src/config.rs#L185-L228
**[checked in review]** (defaults only)

### OpenCode

There is no node or worker protocol. One HTTP server process per machine owns all sessions,
the loop and every tool; the TUI, web app, desktop, `opencode run` and the SDK are clients of
its HTTP API.

1. **Discovery and lifetime.** The managed service writes `service.json` (url, pid, version,
   **password**, instance id) at mode 0600 and checks every 5 s that the file still names it,
   shutting down if replaced. A client probes `GET /api/info` with Basic auth and checks pid
   and version. A private child server (`--standalone`) receives a random password in its
   environment, deletes it from its own environment so tools cannot inherit it, and exits
   when stdin reaches EOF (the parent's pipe is the lease).
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/services/service-registration.ts#L39-L74
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/server-process.ts#L233-L245
   **[corrected in review]** (default port 49374 applies only to some release channels);
   stdio lease **[checked in review]**.
2. **Listener authentication.** The server refuses to start without a password and binds
   `127.0.0.1` by default. Credentials are Basic auth with fixed user `opencode`, a session
   cookie, or `?auth_token=` (the secret in a URL). The password compare is a plain `===`;
   only the session-token HMAC uses `timingSafeEqual`. Authorisation yields a boolean only:
   one secret, no identity, so any holder sees every session. The in-memory SDK router built
   without a password has no check at all. Browsers are fenced by `Sec-Fetch-Site` and a CORS
   allow-list.
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/process.ts#L55-L58
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/middleware/authorization.ts#L13-L40
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/auth.ts#L33-L37
   **[checked in review]**; no per-user identity **[absence confirmed]**.
3. **Pairing.** `POST /api/pair` (authenticated) mints 16 random bytes, held in memory for 5
   minutes (capacity 1,000). `GET /auth/connect/:code` is unauthenticated and atomically
   removes the code, then issues a token `"<expiry>.<HMAC>"` keyed from the server password:
   a 30-day `httpOnly`, `sameSite=lax` cookie (no `Secure`) for browsers, or `{token}` for
   API clients. There is no per-token record and no individual revocation; rotating the
   password revokes every session. A paired phone can mint further pairing codes.
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/pairing.ts#L7-L34
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/auth.ts#L39-L55
   **[checked in review]**
4. **Remote machines over SSH (desktop).** The desktop runs `ssh` to the host, finds or
   installs `opencode` (downloaded from the npm registry on the remote, or uploaded over
   stdin; verified only by `--version`), starts the service, `cat`s `service*.json` back to
   learn the loopback URL **and the password**, and opens `ssh -L`. SSH options are filtered
   to a short allow-list; newline, CR and NUL in the target are refused. One server per
   remote machine, reached by tunnel; nothing dials the controller.
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/gui-plugins/src/ssh/bootstrap.ts#L76-L100
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/gui-plugins/src/ssh/remote-cli.ts#L120-L122
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/gui-plugins/src/ssh/command.ts#L48-L134
   **[checked in review]**
5. **`--remote` tunnel.** A random 16-hex route on an OpenTunnel relay subdomain, treated "as
   sensitive as the password"; the password is still required. Encryption is in the external
   package.
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/services/remote-tunnel.ts#L7-L47
   **[unchecked]**
6. **Event log and resume contract.** Durable events get a per-aggregate contiguous `seq`
   under a per-aggregate lock inside an `immediate` SQLite transaction together with the
   projections. `replay(event)` treats an identical event at the same seq as a no-op, a
   different payload as "Replay diverged", a gap as "Sequence mismatch", and refuses an
   aggregate with a different `owner_id`; it has no non-test caller. The log read endpoint
   `GET /api/experimental/session/:id/log?after=<seq>&follow=` captures the latest seq as a
   watermark, **subscribes before reading**, pages to the watermark, emits one `log.synced`
   marker, then follows. But the durable `event` table is written only when `persist` is on,
   and the shipped CLI server leaves it off.
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L372-L412
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L329-L378
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L765-L806
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L214-L214
   **[checked in review]**
7. **What clients actually do on reconnect.** `GET /api/event` is a volatile SSE stream
   (15 s heartbeat; per-subscriber dropping queue of 4,096 after which that subscriber's
   stream fails). The first-party clients re-fetch state over REST on `server.connected` and
   apply live events, with a race they must hide.
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/event-feed.ts#L66-L68
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/client/src/solid/data.ts#L703-L722
   **[checked in review]**
8. **Crash resume without a fence.** `Execution.Started` and a claim commit in one
   transaction; boot finds claimed sessions, increments `resume_attempts`, injects "The server
   restarted while you were working..." and drains again, or aborts after 10 attempts. The
   sweep assumes every orphaned claim's owner is dead; nothing in the data is a lease or
   token, so two servers on one database would break it.
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/execution.ts#L73-L82
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/execution/restart.ts#L76-L99
   **[checked in review]**
9. **The execution seam.** Tools and MCP stdio servers run through an `Environment`
   (`spawner` plus `files`), bound either to the local machine or to a `WorkspaceDriver`
   (`create`, `connect`, `suspendForIdle`, `destroy`, idempotent per id). The default
   registry is empty; only SDK embedders supply drivers. Permission evaluation stays in the
   server process; the driver only executes. With a workspace bound, shells start with no
   inherited host environment.
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/workspace/driver.ts#L26-L62
   https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L331-L346
   **[checked in review]**
10. **PTY and WebSocket tickets.** Upgrades to PTYs and client-MCP connections use
    single-use, scope-bound tickets with a 60 s TTL, minted by an authenticated POST that
    needs a custom header (so a cross-origin page cannot mint one).
    https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/pty/ticket.ts#L7-L36
    **[checked in review]**
11. **The agent can reach its own control plane.** Outside stdio mode the server password
    stays in the environment every shell inherits, and the service password is in a 0600
    file the agent can read; with the default allows, a prompt-injected agent could call the
    local API and answer its own pending approvals (the self-approval path is inferred).
    https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/server-process.ts#L85-L97
    **[checked in review]** (env handling); self-approval **[inferred]**.

### OpenClaw

OpenClaw has the most complete control/execution split in the survey. One Gateway process
(TypeScript) serves operators, nodes and cloud workers on one port; the HTTP upgrade handler
gives the path `/__openclaw__/worker` its own connection kind and rate limiter, and worker
frames never reach the generic method handler. A Rust node host crate also exists.
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server-http-upgrades.ts#L257-L294
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L99-L100

**Gateway protocol (operators and nodes).**

1. On a gateway socket the server first sends `connect.challenge` with a random nonce. The
   client's first request is `connect` with `minProtocol`/`maxProtocol`, `role`, `scopes`, an
   optional `device` block and an `auth` block. Frames are `req`, `res`, and `event` (with an
   optional `seq` and `stateVersion`). Protocol 4; nodes may be at 3. All schemas are closed
   objects.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/connection.ts#L324-L335
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/frames.ts#L34-L95
   **[unchecked]**
2. Device proof: the device id is derived from the public key; `signedAt` must be within 2
   minutes; the nonce must equal this connection's challenge; Ed25519 signs device id,
   client id and mode, role, scopes, `signedAt`, the presented token and the nonce. Signing
   the token binds the bearer to the key.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/connect-auth.ts#L369-L401
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/handshake-auth-helpers.ts#L205-L245
   **[checked in review]**
3. Roles are a closed pair (`operator`, `node`); node methods are callable only by `node`;
   a method with no scope classification requires admin.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/role-policy.ts#L6-L24
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/method-scopes.ts#L269-L317
   **[unchecked]**
4. Gateway auth defaults to a token that startup generates (24 random bytes hex) when none
   is configured; mode `none` exists.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/auth-resolve.ts#L55-L84
   **[checked in review]**
5. Device tokens (32 random bytes, one per role) are stored **in plaintext** and compared in
   constant time; bootstrap tokens likewise, with a 10-minute TTL. A read of the state
   database yields every paired device's bearer.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/pairing-token.ts#L4-L18
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/device-pairing-token-policy.ts#L54-L108
   **[checked in review]**
6. The browser Control UI keeps the shared token in `sessionStorage` but the Ed25519 device
   seed and device tokens in `localStorage`, readable by any script in that origin.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/lib/nodes/index.ts#L112-L126
   **[checked in review]**

**Nodes: pairing and execution-side refusal.**

1. Node pairing is a device-pairing record with a separately approved command surface
   (`node_surface_json`, `pending_node_surface_json`); operators approve, reject or remove.
   An approval is refused while a reconnect owns the pending revision.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/device-pairing-node.ts#L211-L262
   **[unchecked]**
2. SSH-verified auto-approval (on by default): a fresh, scopeless `role: node` request from
   a private or CGNAT address makes the Gateway run
   `ssh -o BatchMode=yes -o StrictHostKeyChecking=yes ... sh -lc 'openclaw node identity --json'`
   and compare device id and key. Anyone who can make that host print matching JSON (a login
   shell profile, for example) satisfies it (inference).
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-pairing-ssh-verify.ts#L190-L240
   **[checked in review]**; the profile attack **[inferred]**.
3. `node.invoke` on the Gateway requires the command to be allow-listed, not private, and
   declared by the node.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-command-policy.ts#L460-L480
   **[checked in review]**
4. **The node re-decides `system.run`, but trusts a forwarded approval over its own
   allow-list.** The node recomputes `security` and `ask` from its own config and approvals
   file, refuses any approval that lacks a `systemRunPlan` exactly matching argv, command
   text, cwd, agentId and sessionKey, refuses PATH and other blocked env overrides, and
   refuses a delayed approval if its own policy snapshot changed. But in
   `evaluateSystemRunPolicy`, `approved === true` sets `approvedByAsk`, which satisfies both
   "ask" and "allow-list miss". Only a local `security=deny` holds against a
   Gateway-asserted approval.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L121-L141
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L233-L264
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L349-L357
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117
   **[checked in review]**
5. On the Gateway side, a client's `approved` flags are always stripped and re-added only
   from a server-side record by `runId` matching node id, requester device, argv, cwd,
   agentId, sessionKey and env hash; `allow-once` is consumed atomically.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-invoke-system-run-approval.ts#L197-L252
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-invoke-system-run-approval.ts#L434-L452
   **[checked in review]**
6. A node in `auto` mode runs the reviewer on the node, with the node's own config and model
   credentials and no transcript; a reviewer "ask" becomes a denial because the node has no
   human route.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L491-L543
   **[checked in review]**

**Cloud workers: enrolment, admission, fencing, credentials.**

1. *Enrolment and who dials.* A provisioned box redeems a setup code (profile
   `cloud-worker`) that binds the device to the environment inside the pairing transaction.
   Enrolment refuses a loopback, link-local or unspecified Gateway URL. The worker process
   dials `<Gateway URL>/__openclaw__/worker` over `ws:`/`wss:` (optionally with a pinned TLS
   fingerprint or Cloudflare Access headers) or a Unix socket; the endpoint schema forbids
   userinfo and query and allows a TLS pin only with `wss`. Reconnect backoff 250 ms to 30 s.
   **The Gateway also dials the box over SSH** for bootstrap, tunnel control and workspace
   transfer, with `-F none`, `StrictHostKeyChecking=yes`, its own known-hosts file,
   `IdentityAgent=none`, no agent or X11 forwarding.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/device-pairing-cloud-worker.ts#L29-L89
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/node-enrollment.ts#L85-L106
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-endpoint.ts#L45-L86
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291
   **[checked in review]**; "outbound only" **[corrected in review]** (see section 1).
2. *Per-dispatch credential.* 32 random bytes, registered for log redaction, stored only as
   `sha256("openclaw-worker-credential-v1\0" + credential)` or, for a session turn,
   `sha256(domain-v1-turn \0 serialized turn claim \0 credential)`, which binds the secret to
   the exact durable claim. TTL 10 minutes. One credential row per environment (primary key
   `environment_id`), so minting replaces the old one; minting first cancels the
   environment's in-flight inference. `renewCredential` throws if the owner epoch changed,
   the environment is not `ready`/`idle`/`attached`, destroy was requested, or there is no
   bootstrap receipt.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential.ts#L9-L60
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential-broker.ts#L105-L126
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/store.kernel.ts#L382-L421
   **[checked in review]**
3. *Admission (first frame).* The worker's first frame is a `connect` with `role: worker`
   and an `admission` block (environment id, credential, owner epoch, RPC set version, build
   handshake, session id and run id). Checked in order: constant-time compare of the
   credential hash; environment state and no destroy; bundle hash equal across credential,
   bootstrap receipt and expected build; same version; same session; same owner epoch in
   admission, credential and environment; RPC set version 1; identical feature lists; for a
   session turn, the durable turn claim must match session, run, environment and epoch.
   Admission is serialised per client IP, only failures are charged to the rate limiter, and
   every public rejection is the opaque `invalid-handshake`. The worker gets a minimal
   `worker-hello-ok` and checks it against what it asked for.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L137-L170
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L79-L186
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L218-L262
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-admission.ts#L56-L66
   **[checked in review]**
4. *Owner-epoch fencing.* An environment's `owner_epoch` is set to
   `max(all environment epochs, all transcript commit-head epochs) + 1`, so it is global and
   never reused. It is bumped on transitions out of `ready`, `idle` or `attached` that revoke
   the credential (to `attached`, `attached`→`idle`, `draining`, `destroyed`, `failed`,
   `orphaned`, clearing the bootstrap receipt) and when a transition accepts an attached
   credential; a failure out of an earlier state (such as `bootstrapping`) revokes without
   bumping, and a credential mint does not bump. **Every worker request re-reads the
   credential and environment**: a different hash closes with `credential-replaced`, a changed
   epoch with `owner-epoch-mismatch`, expiry with `credential-expired` (except for an exact
   durable turn). The worker treats `credential-replaced` and `owner-epoch-mismatch` as
   terminal ("fenced") and stops reconnecting. A timer at credential expiry re-runs
   validation and closes unattached workers.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/store-mutations.ts#L30-L50
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/store-transitions.ts#L303-L350
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L218
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/worker-turn-rpc.ts#L691-L716
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-contract.ts#L22-L26
   **[corrected in review]**
5. *Transcript commits.* `worker.transcript.commit` carries `runEpoch`, `seq`, `baseLeafId`
   and messages. The Gateway hashes the request, opens or replays a ledger row keyed
   `(session_id, run_epoch, seq)` (`pending`, or `terminal` with the cached result), and
   applies the batch in one transaction that re-checks session id and lifecycle revision and
   compares the transcript version with the prepared plan (`stale-base-leaf`). Message
   idempotency keys derive from session, epoch, seq and index. The authority check runs before
   reservation, inside the write, and before the ACK cursor update. A lost ACK replays the
   cached result; a settled persistence failure returns `retryable: false` so it cannot
   "masquerade as a lost ACK and replay forever".
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/worker-turn-rpc.ts#L279-L320
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/transcript-commit.runtime.ts#L324-L366
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L503-L519
   **[unchecked]**
6. *Writer claim.* A run claims a session by CAS of `activeWriterRunId` against session id
   and lifecycle revision; an incumbent is superseded once the new claim is durable
   (last-writer-wins takeover, not a lock). A fenced transcript write captures
   `expectedWriterRunId` from the run's async context and, inside the SQLite write
   transaction, re-reads the row and refuses with `session-rebound` on mismatch. A write with
   no expected writer skips the writer comparison (session id is still checked).
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/session-bootstrap.ts#L543-L590
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-transcript-write-snapshot.ts#L104-L130
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-scope.ts#L79-L97
   **[corrected in review]**
7. *Closed method list.* The worker socket accepts only `worker.heartbeat`,
   `worker.transcript.commit`, `worker.live-event`, `worker.computer`, the exec-approval
   method, gateway-tool invoke and cancel, the two inference methods and turn admission. Each
   also needs its protocol feature in the admitted build and passes schema validation;
   anything else closes with `method-not-allowed` (1008). Queues are capped at 16 frames and
   32 MiB, method names at 64 chars. The heartbeat echoes `ownerEpoch` and revalidates.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L54-L66
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection.ts#L69-L75
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection-dispatch.ts#L112-L131
   **[checked in review]**
8. *Model references, not keys.* `worker.inference.start` carries a `modelRef`; the Gateway
   resolves it against the agent's visibility policy and catalogue and refuses
   `model-not-approved`, any `@profile` suffix, a session mismatch, or `runEpoch !=
   ownerEpoch`, then streams with a Gateway-held key. A worker-local inference mode keeps the
   endpoint and key on the node, and the Gateway still approves the model ref.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-runtime.ts#L122-L153
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-model.ts#L91-L124
   **[checked in review]**
9. *But a GitHub token does reach the worker.* The launch descriptor carries a `github.token`
   (up to 2,048 chars); heartbeats can refresh it; the worker writes it into a per-turn `gh`
   profile, so the agent's shell can read it.
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/launch-descriptor.ts#L183-L211
   https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/github-binding.runtime.ts#L159-L198
   **[checked in review]**
10. *Disconnect and resume.* An exact durable turn may stay connected and reconnect with the
    same claim-bound credential past the TTL until the terminal ACK. Requests whose owner
    keeps settlement reuse the same call id after reconnect. When the node is lost mid-turn,
    `WorkerNodeLostError` says tool effects may be unknown and `canContinue = false`.
    https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection.ts#L255-L269
    https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-node-loss.ts#L3-L15
    **[checked in review]**

**Ingress and restart.** Accepted input is stored before acknowledgement: channel messages
in `channel_ingress_events` (unique on queue and event id; claimed with token, owner and
lease; transport ACK only after the claim is owned), agent turns as a staged pending-input row
(unique per `(session_id, idempotency_key)`, with `request_hash`; failure is "agent turn was
not durably admitted"). Restart resume injects one fixed prompt telling the agent to check
files, processes and external actions before repeating anything. Delivery recovery refuses
blind replay of a send that may have started unless the adapter proves it did not happen.
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/channels/message/ingress-queue.kernel.ts#L145-L258
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/agent-turn/agent-run-user-turn.ts#L402-L426
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L897-L932
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/session-restart-resume.ts#L78-L85
https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/outbound/delivery-queue-recovery.ts#L597-L625
Ingress **[checked in review]**; pending-input schema, restart prompt and delivery recovery
**[unchecked]**.

### Hermes Agent

The **gateway** process owns execution (agent loop, cron ticker, Kanban dispatcher); the
CLI, TUI, desktop, ACP adapter and dashboard are viewers. All share one SQLite `state.db` per
profile; correctness rests on transactions plus a `runtime_epoch` fence.

1. **Local control socket and tickets.** The gateway listens on `$HERMES_HOME/gateway.sock`
   (named pipe on Windows), one JSON request per connection. On POSIX the peer uid is read
   from the socket and only a same-user peer gets a ticket. `TicketStore.mint` stores
   SHA-256 of a 32-byte ticket with a 30 s expiry, a purpose (`interactive`, `native-http`,
   `worker-adoption`, ...), a capability set, a subject and a scope, capped at 1,024
   outstanding; `redeem` deletes it and checks profile and purpose. Clients dial the WebSocket
   with subprotocols `hermes-gateway-v1` and `hermes-gateway-ticket.<ticket>`, so the ticket
   is never in a URL. Requests over 64 KiB, a stale `instance_id`, or a bare subject label
   fail closed.
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/control_socket.py#L381-L409
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/control_socket.py#L411-L425
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/runtime_bootstrap.py#L25-L75
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/web_server_boundary.py#L156-L173
   **[checked in review]**
2. **Dashboard listener.** Any non-loopback host requires an auth provider (RFC 1918 counts
   as public); the legacy `allow_public` is ignored and `--insecure` does not bypass it. Host
   header checked against the bound interface (DNS-rebinding fence). All principals act as
   one owner ("Hermes is single-user").
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/web_server_boundary.py#L111-L119
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/main_dashboard.py#L640-L662
   **[checked in review]**
3. **OpenAI-compatible API server.** Binds `127.0.0.1:8642`, refuses to start without a
   strong `API_SERVER_KEY`, constant-time compare, 429 beyond 10 concurrent runs. One shared
   key, no scopes.
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/platforms/api_server.py#L3907-L3931
   **[checked in review]**
4. **Durable per-session FIFO with an epoch fence.** Each inbound message is first committed
   as a `session_admissions` row keyed `(principal, target_session, request_id)` (a retry is
   an exact replay; a changed payload under the same key raises `admission_conflict`), with a
   payload digest, an intent (`queue`, `steer`, `redirect`) and a status `queued -> started
   -> terminal | unknown`; at most 32 queued per session. `claim_session_input` takes the
   head in one write transaction, refuses if any admission or worker for the session is
   `unknown`, bumps the session's `runtime_generation`, and stamps the row with
   `(owner_epoch, generation)`. `settle_session_input` accepts only when status, epoch and
   generation all match (`stale_generation` otherwise). At start, `begin_runtime_epoch`
   increments a singleton epoch under the profile ownership lock, and
   `recover_session_inputs` marks every `started` admission from an older epoch and every
   non-terminal worker execution `unknown`, never requeued. Only an explicit resolution
   settles an `unknown` row: to `interrupted`, or to `completed`/`failed` if the same epoch
   had captured the result before its settlement write failed.
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L126-L177
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L206-L242
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L245-L274
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L453-L464
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L663-L694
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/session_authority.py#L1450-L1467
   **[corrected in review]**
5. Unattended `unknown` turns (cron principal, `automation:*`, Kanban, webhook markers) are
   settled at boot as interrupted so a queue does not stall; a person's `unknown` still
   pauses the session.
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/session_unattended_unknown.py#L41-L66
   **[checked in review]**
6. **Relay: the gateway dials out.** `WebSocketRelayTransport` dials `wss://.../relay`
   (ping 30 s, pong deadline 60 s) and sends one `hello` per fronted identity. The upgrade
   bearer is `base64url(gateway_id:exp:HMAC-SHA256(gateway_id:exp, secret))`, TTL 300 s, with
   rotating secrets on the connector. Newline-delimited JSON frames. Inbound events replayed
   from the connector's buffer carry a `bufferId`; the gateway sends `inbound_ack` **only
   after the message was admitted into `session_admissions`** (or permanently refused), so a
   crash before admission leaves it unacknowledged and the connector redelivers. Admission
   runs in a per-chat lane. A 4401 close after a successful handshake is revocation (terminal);
   an expired token is retried once. Media sends settle as "possibly delivered".
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/auth.py#L36-L40
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/ws_transport.py#L442-L494
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/ws_transport.py#L971-L1004
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/ws_transport.py#L66
   **[unchecked]**
7. **Delivery ledger.** The reply is persisted as an obligation and immediately marked
   `attempting` before the send; boot reclaims rows whose owner `(pid, start time)` is dead by
   CAS, abandons after 3 attempts or 24 h, and resends the rest with a visible "may be a
   duplicate" prefix.
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/delivery_ledger.py#L368-L446
   **[checked in review]**
8. **Kanban "workers" are thin clients.** The dispatcher claims a task (claim TTL 15 min,
   with a `claim_lock`) and starts a detached subprocess whose only job is to submit
   `kanban.run` to the profile's gateway and poll a receipt every 0.2 s; the agent and its
   tools run inside the gateway owner. A claim reclaimed elsewhere makes the worker's later
   transitions fail.
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_db_dispatch.py#L2697-L2700
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_worker_client.py#L121-L135
   **[unchecked]**
9. **ACP is a viewer.** `hermes acp` constructs a `GatewayACPAgent` that never runs an agent
   or MCP itself: `session/new` calls the owner's `runtime.describe`, requires the
   `acp-editor-policy-v1` capability, and creates the session with edit approval `ask`;
   `session/fork` is the gateway's branch mutation. Permission options map to `allow_once`,
   `allow_session` (sent as ACP `allow_always`, meaning kept in the id), `allow_always`,
   `deny`, `deny_always`; an unknown id maps to deny; a timeout (default 300 s) returns
   `"timeout"`, reported separately from a denial.
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/entry.py#L205-L215
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/gateway_server.py#L271-L291
   https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/permissions.py#L67-L74
   **[unchecked]**
10. **Execution back ends where the loop stays home.** SSH uses
    `ControlMaster=auto`, `BatchMode=yes`, `StrictHostKeyChecking=accept-new` (TOFU), syncs
    skills and credential files to the remote `~/.hermes` before each command, and on
    cleanup tars the remote `~/.hermes` back and writes changed or new files onto the host,
    last write wins; a compromised remote could add files under the host's skills tree
    (inferred from the path mapping). Modal and Daytona run commands through their SDK `exec`
    with no env forwarding and no egress control.
    https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/ssh.py#L122-L134
    https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/file_sync.py#L484-L496
    https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/modal.py#L190-L204
    SSH **[checked in review]** (mechanism), skills write-back **[inferred]**; cloud
    sandboxes **[unchecked]**.

### Oh My Pi

One process per interactive session owns the TUI, the loop, the tools and the session
writer; RPC, ACP, print and collab all embed that process. No node or worker split; the
satellite servers (auth broker, auth gateway, stats, browser relay, LSP mux) are optional.

1. **Ownership lease.** Ownership is an OS file lock per session id that the kernel releases
   when the process dies; a non-owner never writes the owner's file and rewrites to a sibling
   file with a new id instead.
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L1157-L1182
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-storage.ts#L536-L541
   **[unchecked]**
2. **RPC mode.** NDJSON over stdio; first frame `ready` (protocol 1, supports 1 and 2, max
   frame sizes). `negotiate_protocol` switches to v2, which chunks oversize frames into
   `rpc_chunk` (256 KiB payloads, up to 64 MiB reassembled); a v1 frame over 1 MiB is shrunk
   by passes with explicit elision markers, not rejected. No authentication: the parent that
   owns the pipes is the trust boundary. Stdin EOF disposes the session; a stuck stdout
   writer terminates the process. `get_entries {since}` pulls the log incrementally (an
   unknown `since` id is an `unknown_since` error, not a silent full send); `set_host_tools`
   lets the client register tools the client executes (`host_tool_call` / `host_tool_result`).
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-frame.ts#L5-L16
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L2023-L2027
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L2341-L2360
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/host-tools.ts#L1-L60
   RPC framing **[checked in review]**; incremental read and host tools **[unchecked]**.
3. **ACP.** `omp acp` serves ACP over stdio; `process.stdout` is replaced by a sink to
   stderr first so extensions cannot corrupt the channel. Under an ACP client that supports
   `requestPermission`, `bash` is forced to `always-ask` unless the user explicitly chose
   auto-approve.
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/acp/acp-mode.ts#L43-L72
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-tools.ts#L1174-L1217
   Stdout isolation **[unchecked]**; ACP bash default **[checked in review]**.
4. **SSH is file access only.** `ssh://host/path` reads, lists and writes text files (UTF-8,
   at most 1 MiB) over an OpenSSH ControlMaster with `BatchMode=yes` and
   `StrictHostKeyChecking=accept-new`; no tool runs `bash` remotely. Hosts may come from a
   project `ssh.json`, whose `${VAR}` values expand from the process environment (a leak path,
   inferred).
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/ssh/connection-manager.ts#L193-L209
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/internal-urls/ssh-protocol.ts#L1-L35
   **[checked in review]**; no remote exec **[absence confirmed]**; env leak **[inferred]**.
5. **Collab: the host runs everything; the relay forwards sealed frames.** The host opens
   `wss://my.omp.sh/r/<roomId>`, generates a 32-byte room key and a 16-byte write token, and
   puts them in the link fragment. All session payloads (`welcome`, `snapshot-chunk`,
   `entry`, `event`, `state`, `ui-request`, `transcript`) are AES-256-GCM sealed; the relay
   sees peer ids, sizes, timing and join/leave events. A guest is writable only if its token
   matches (timing-safe); prompts from read-only peers are refused. A guest that cannot keep
   up is shed and may auto-rejoin at most 3 times in 5 minutes. A guest prompt runs under the
   host's approval mode (entry point traced, approval wiring not).
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/protocol.ts#L1-L100
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/host.ts#L882-L920
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/wire/src/index.ts#L450-L463
   **[checked in review]** (token and write gate only); relay behaviour read from a
   stand-in.
6. **Auth broker and auth gateway (credential distribution).** The broker binds
   `127.0.0.1:8765` by default (`--bind` accepts anything), one static bearer from a 0600
   file checked by `Set.has` (not constant time; an empty set disables auth), plain HTTP,
   `/v1/healthz` open; clients receive API keys and OAuth access tokens (only refresh tokens
   are replaced by a sentinel). The gateway, by contrast, holds the keys and forwards
   provider-format requests so clients never see a credential; it supports scoped clients
   stored as SHA-256 digests (model and account allow-lists) and `--no-auth`, and answers with
   `Access-Control-Allow-Origin: *` (a web page could spend the user's accounts with
   `--no-auth`, inferred).
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L109-L116
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth/pool.ts#L715-L735
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/clients.ts#L1-L60
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/http.ts#L306-L323
   **[checked in review]**; CORS attack **[inferred]**.
7. The browser relay rejects any `Origin` on its CDP side but takes an optional `?token=`
   compared with `!==`.
   https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/browser/relay/server.ts#L130-L160
   **[checked in review]**

### Pi

The coding agent has no remote-worker mode. Two first-party pieces are relevant: the RPC
mode (an embedding protocol) and `pi-env` (a remote tool daemon used by the experimental
`apps/p2`).

1. **RPC mode.** `pi --mode rpc` takes over stdout, reads strict-LF JSON commands
   (`prompt`, `steer`, `follow_up`, `abort`, `fork`, `clone`, `get_entries` with `since`,
   `get_tree`, ...) and writes responses and events. No token, socket or multiplexing. Writes
   await stdout back-pressure before the next command. There is no `navigate_tree` command.
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L401-L738
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L768-L824
   **[unchecked]** (no-auth is true by construction of stdio; the no-`navigate_tree` claim
   was confirmed by the reviewer's search).
2. **pi-env: the controller dials over SSH; the daemon is a framed stdio tool server.**
   Every ssh call uses `-T -a -x`, `BatchMode=yes`, `ClearAllForwardings=yes`,
   `ForwardAgent=no`, `ForwardX11=no`, `ControlMaster=no`, `ControlPath=none`,
   `RemoteCommand=none`, `PermitLocalCommand=no`, `SendEnv=-*`, `StrictHostKeyChecking=yes`,
   an app-owned `UserKnownHostsFile`, `GlobalKnownHostsFile=none`, a fixed `HostKeyAlias`,
   `IdentitiesOnly=yes`, then `--`; host, alias and user starting with `-` or containing
   whitespace are refused. Trust on first use is explicit: `scanHostKey` probes into a
   throwaway file and returns fingerprints for out-of-band comparison; a changed key is never
   accepted automatically.
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/ssh.ts#L59-L123
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/ssh.ts#L185-L210
   **[checked in review]**
3. Deployment by content hash: the daemon file is named by the first 32 hex of its SHA-256,
   uploaded over ssh stdin to a `mktemp` file in a 0700 directory, re-hashed, then renamed;
   verified again before each later start. Hash check and `exec` are separate steps.
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/ssh.ts#L403-L464
   **[unchecked]**
4. Wire protocol: `u32 length | u8 type | u32 id | u32 jsonLength | json | payload` (REQUEST,
   RESULT, ERROR, EVENT, CANCEL, PING), max 16 MiB, version checked by `hello`. The daemon
   prints `PI-ENV <token>` first and the client discards everything before it; **the token is
   a sync marker, not a credential**. Both sides ping every 5 s; after 30 s of silence, on
   stdin EOF or on a signal, the daemon kills every process group it started. A request in
   flight when the connection drops fails with `lost: true` ("their outcome is then unknown"
   for mutations); handles are session-bound and fail after reconnect. Cancel names one
   request and is registered before running.
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/connection.ts#L421-L436
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/daemon/src/main.rs#L532-L547
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/connection.ts#L225-L228
   **[checked in review]**
5. **What the daemon refuses: nothing.** `dispatch` handles file operations, `exec`, `watch`,
   `pty` and `process` with paths passed straight to the OS: no root, no confinement, no
   allow-list, no seccomp, chroot or rlimit. `exec` spawns with the daemon's environment plus
   overrides, in a new session. Bounds are frame size, 4,096 handles, 16 file workers and the
   silence timeout.
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/daemon/src/main.rs#L137-L282
   **[checked in review]**; no policy **[absence confirmed]**.
6. Credentials stay local because only tools move: the remote command starts with the remote
   daemon's environment and an explicit `shellEnv`, never the client's `process.env`.
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/remote-env.ts#L871-L874
   **[checked in review]**
7. `apps/p2` (prototype) states that tools and workspace files live on an SSH environment
   while "the worker, Durable history, provider credentials and model calls stay on this
   device", with no fallback to local execution. Its web server binds `127.0.0.1`, judges
   every request by its Host header, refuses forwarded headers on loopback and cross-site
   Fetch Metadata, and signs in with a one-use 10-minute token printed in the terminal.
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/apps/p2/docs/remote-envs.md#L1-L5
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/apps/p2/src/hosts/principals.ts#L17-L38
   **[checked in review]**
8. Durable runs (`pi-durable`): on open, tasks found `running` are demoted to `pending` with
   `restarts + 1` in one transaction. The tool task commits `{phase: "execute", arguments,
   replay: tool.replay ?? "unsafe"}` before running the tool; on resume in phase `execute` the
   tool reruns only if both the stored and the current policy say `safe`, otherwise the call
   settles as "was interrupted and may have partially run". Only tool-search and codemode
   declare `safe`.
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/durable/src/harness/scheduler.ts#L450-L492
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/durable/src/harness/tool.ts#L195-L198
   https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/durable/src/harness/tool.ts#L201-L227
   **[checked in review]**

### DeepSeek harness

In the `web` profile the agent loop, tools, HTTP server, WebSocket gateway and scheduler are
rows of one Cordis Loader in one Node process; the Electron app spawns that host as a child.
The only remote split is an SSH back end where the loop stays local.
https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/web-app/cordis.patch.yml#L116-L136
**[checked in review]**

1. **Transport.** Unary calls are `POST /api/<namespace>/<method>` with `{rpcId, method,
   payload}`; streams (`follow`, `control`, `observeState`) share one WebSocket per peer at
   `/api/remote.mux` with `open`/`item`/`end`/`cancel` frames, ping/pong tolerating two missed
   pings, and a per-stream uplink byte budget. A duplicate stream id or malformed frame closes
   with 1008; binary frames with 1003. Method descriptors are generated at build time.
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/connection/src/rpc-host.ts#L304-L349
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/gateway/src/stream-server.ts#L193-L198
   **[unchecked]**
2. **Listener authentication.** A 32-byte launch token per process is printed in the start
   URL as `?token=` (so it sits in argv of the browser opener and in scrollback); `GET /` with
   it mints a cookie `dsh-auth-<sha256(authority)>` signed with an HMAC secret persisted in
   the credential store, `HttpOnly; SameSite=Strict`, `Secure` only when the listener itself
   serves TLS, 30 days. Cookies survive restarts; there is no revocation short of rotating
   the secret, and anyone who can read the credential file can forge one. Every principal is
   "owner".
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/connection/src/browser-auth.ts#L182-L199
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/connection/src/browser-auth.ts#L244-L305
   **[checked in review]**
3. **Host/Origin fence and bind refusal.** Every `/api` request and upgrade must carry a
   loopback, bind-address or `trustedHosts` Host; `Sec-Fetch-Site: cross-site` is refused; an
   `Origin`, if present, must equal the Host authority (`null` refused). Fence failure is 403,
   auth failure 401. Wildcard binds (`0.0.0.0`, `::`) are refused at config load with an
   error naming remote code execution; default `127.0.0.1:3080`.
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/connection/src/api-request-trust.ts#L121-L151
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/host/webserver/src/index.ts#L358-L366
   **[checked in review]**
4. **Event streaming and reconnect.** `follow` first yields one snapshot (header, a
   message-aligned window with a `cursor` seq, projection baseline, assistant-stream
   baseline), then durable events from a listener buffer **armed before the snapshot is
   read**, then live assistant frames. Duplicates below `next` are dropped; a gap throws.
   The request has **no resume cursor**: on reconnect the client reopens `follow` and repairs
   any break with `session.page(throughSeq)`. Assistant frames carry a dense `revision`. The
   listener buffer is an unbounded `Deque`, so a stalled client grows host memory. The
   docstring says `follow` takes the caller's last seq, but the request type has no such
   field.
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/session-controller/src/web/controller.ts#L32-L77
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/gateway/src/client/journal-stream.ts#L305-L330
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/session-controller/src/history/listener.ts#L176-L248
   **[unchecked]**
5. **Idempotent prompt admission.** A `prompt` carries a client `requestId`; the host finds
   accepted ids by scanning log entries whose source has `rpcId` and returns `accepted: true`
   for a repeat. No side table.
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/session-controller/src/commands.ts#L389-L413
   **[unchecked]**
6. **Approvals over the wire.** `approval/asked` and `approval/decided` are logged; the
   request is forwarded to owner clients as a `waterfall` frame and answered by an HTTP
   `$events/result`. Pending requests live in a gateway `Map`, are re-delivered to owner
   clients that connect later, settle on the first answer, and never time out; a restart drops
   them and resume closes the turn.
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/gateway/src/index.ts#L638-L700
   **[checked in review]**
7. **Durability rule.** Events are buffered up to 200 ms per session, but the checkpoint
   policy awaits a flush before the model stream is built, before every top-level tool body
   and at each pre-step; a failed flush rejects. Writer exclusion is a non-blocking `flock` on
   `session.lock` **with no expiry**; resume takes the lock, reads the whole log and appends
   closers: every unanswered tool call gets a synthetic error result ("outcome is unknown ...
   do not retry blindly", or "not started"), then `step/end` and `turn/end` with reason
   `interrupted`.
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-checkpoint-policy/src/index.ts#L63-L83
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-persistence-jsonl/src/lease.ts#L70-L116
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/repair.ts#L53-L98
   **[checked in review]**
8. **SSH back end: loop local, pinned helper remote.** `ssh -T -M -S <ctl> -o
   ControlPersist=no -o BatchMode=yes -o StrictHostKeyChecking=yes -o ForwardAgent=no -o
   ClearAllForwardings=yes ...` runs a helper that speaks a versioned (protocol 3) framed JSON
   RPC over stdio. Its SHA-256 is **self-reported** by the helper over its own file, so it
   detects version drift, not a hostile host. Streams go through Unix-socket forwards
   authenticated with TLS 1.2 PSK (`PSK-AES256-GCM-SHA384`, per-stream key sent over the admin
   channel). The helper holds a lease (heartbeat every third of a 30 s lease) and cleans up its
   processes on expiry. **SSH loss is final**: no reconnect, all pending requests reject, "remote
   outcomes and cleanup are unknown"; an ambiguous request is never replayed. The SSH sandbox
   provider forwards `confine` to the helper, which runs its own local provider chain, and the
   remote's reported `enforcement` is trusted as sent.
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/index.ts#L279-L328
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/helper.ts#L103-L119
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/stream-security.ts#L1-L47
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/index.ts#L269-L277
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/sandbox-ssh/src/index.ts#L9-L31
   **[checked in review]**
9. **LiveShare (experimental, off in the default profile).** An owner-side worker holds a
   WebSocket to a separately deployed relay (heartbeat 25 s, drop after two missed pongs) and
   forwards viewer requests to the owner host, where each reader becomes a `share-viewer`
   principal that must pass role checks.
   https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/experimental/liveshare/src/owner-gateway/worker.ts#L1-L60
   **[unchecked]**
10. **External harnesses as sub-agents (ACP and app-server as client).** Optional providers
    run Codex as `node <wrapper> app-server --stdio`, Claude Code through its SDK, or any ACP
    agent over stdio. The ACP client initialises with **empty client capabilities** (no fs,
    no terminal), passes `mcpServers: []`, and answers permission requests by a fixed policy
    (`reject` by default). Every approval request from Codex is answered `cancel`/`decline`,
    user-input requests get empty answers, elicitations are declined. The environment is the
    parent's with ambient credentials and `DSH_*` stripped. But the permission mode is fixed
    per provider and **independent of the parent's**: a read-only parent can delegate to a
    child configured to bypass approvals and sandbox.
    https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-acp/src/run.ts#L415-L451
    https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent-codex/src/wire.ts#L574-L625
    **[checked in review]**

### goose

Everything a goose UI does goes through ACP: the desktop opens a WebSocket to `/acp` on a
`goose serve` it spawns; `goose acp` is stdio. One `StateMachineServices` (sessions,
scheduler, providers, extensions) is shared by every connection. There is no worker model:
a client dials a server that owns the code.
https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/server_factory.rs#L90-L112
No dial-out worker **[checked in review]** (negative search over the files read).

1. **`goose serve`.** One axum router: ACP at `/acp` behind an optional token middleware,
   `/health` and `/status` unauthenticated, two MCP-app proxy routes. Default
   `127.0.0.1:3284`, optional TLS. No check that a non-loopback bind uses TLS: plain HTTP on
   `0.0.0.0` is accepted.
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/mod.rs#L225-L248
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L1875-L1912
   **[checked in review]**
2. **Authentication.** The secret is read only from `GOOSE_SERVER__SECRET_KEY`; without it
   `goose serve` refuses to start unless `--dangerously-unauthenticated` (which nothing
   restricts to loopback). Every `/acp` request carries `X-Secret-Key` **or `?token=`**,
   compared with `subtle::ConstantTimeEq`; the desktop's WebSocket uses the query form. ACP
   `authenticate` always succeeds. No per-method authorisation: any holder can call
   `config/extensions/add` (stores a stdio command that is later spawned), `tools/call`,
   `config/upsert` and the working-dir update. The MCP-app proxy compares its secret with
   `!=` but demands a loopback peer.
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/auth.rs#L9-L36
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L1821-L1834
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/server/dispatch.rs#L31-L41
   **[checked in review]**
3. Origin allow-list on upgrades: with a secret and no `--allowed-origin`, loopback origins
   plus `null` and `file://`; wildcards are refused at startup; requests without `Origin`
   pass.
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/mod.rs#L27-L99
   **[unchecked]**
4. **TLS and pinning.** `--tls` with no paths generates a self-signed certificate (SANs
   `127.0.0.1`, `localhost`) cached under the config dir and prints
   `GOOSED_CERT_FINGERPRINT=<SHA-256>` to stdout. The server never verifies clients; pinning
   happens only in the Electron main process, per hostname, with **TOFU when the stored
   fingerprint is blank**; with no record it fails closed.
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/tls.rs#L145-L178
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/main.ts#L305-L379
   **[checked in review]**; no Rust verifier **[absence confirmed]**.
5. The desktop generates a 32-byte secret, passes it to the child in the environment (not
   argv), and builds `wss://127.0.0.1:<port>/acp?token=<secret>`.
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/gooseServe.ts#L243-L270
   **[unchecked]**
6. **goose as ACP agent.** Implements `initialize`, `authenticate`, `session/new|load|prompt|
   cancel|set_config_option|set_mode|list|delete|close|fork`, plus 91 `_goose/unstable/*`
   methods. Permission requests offer `allow_always`, `allow_once`, `reject_once`,
   `reject_always`; an unknown option id falls back to `Cancel`. goose does not use ACP
   `fs/*` or `terminal/*` client methods.
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/server.rs#L1520-L1585
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/common.rs#L55-L65
   Method set **[checked in review]**; permission mapping **[unchecked]**.
7. **Resume.** `session/load` replays the stored conversation as `session/update` chunks
   (optionally only the tail), asks the provider to resume its own session id, and restarts a
   turn if the active turn has an unanswered tool-confirmation request or a stored but
   unapplied response. A decision is persisted as a message first; a conflicting second
   decision, or one for a request that already has a tool response, is rejected. If the
   extension lease is gone, the pending call gets a synthetic "Tool approval expired" error
   instead of executing.
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/server/load_session.rs#L406-L452
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/tool_confirmation.rs#L148-L186
   **[checked in review]**
8. **Roaming (experimental, feature `roaming`).** A persisted Ed25519 key is the iroh
   endpoint id; a "connection card" (`goose+roam://` plus base64 of version, endpoint id,
   relay URLs) carries no secret; its fingerprint is 16 bytes of SHA-256 in eight groups. Access
   exists only if the host's trust book lists the peer key, authenticated by the QUIC-TLS
   handshake; a 10 s, 64 KiB-capped handshake then hands the stream to the ACP server. **Every
   inbound connection re-reads the trust book and refuses on read error**; revoked keys are
   refused even if allowed; writes are atomic under a sidecar `flock`; a watcher polls the
   file every 2 s and closes live connections of removed keys; `accept` re-checks trust after
   registering the connection. An accepted peer gets the full ACP surface over the host's
   shared services, so it can list and load the host user's own sessions and add stdio
   extensions; the forced cwd for new/load/fork is bypassed by the working-dir update. The CLI
   uses four hard-coded goose relays unless overridden.
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/trust.rs#L19-L125
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/node.rs#L586-L621
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/node.rs#L324-L355
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/server/manage_sessions.rs#L4-L30
   Trust and peer surface **[checked in review]**; identity and card **[unchecked]**.
9. **goose as ACP client (providers).** `claude-acp`, `codex-acp`, `pi-acp`, `amp-acp`,
   `copilot-acp` spawn the vendor adapter over stdio with `kill_on_drop` and the **whole
   parent environment** (only `CLAUDECODE` removed for Claude), empty client capabilities,
   and goose's stdio and HTTP extensions forwarded as MCP servers (their env values and
   headers included). goose's modes map onto the adapter's (Claude `auto` to
   `bypassPermissions`, Codex `auto` to `agent-full-access`; Pi gets no mapping, Amp `chat`
   gets `default`). In `auto` goose answers the adapter's permission prompts `AllowOnce`
   itself, in `chat` `RejectOnce`; only the approval modes reach the user.
   `map_permission_response` never widens.
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/provider.rs#L1500-L1531
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/provider.rs#L2388-L2394
   https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/common.rs#L70-L107
   **[checked in review]**

### Cline

The SDK has three runtime hosts: `LocalRuntimeHost` (loop in the calling process),
`HubRuntimeHost` (a hub daemon over WebSocket) and `RemoteRuntimeHost` (a hub at a
user-supplied endpoint with a token).
https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/host/host.ts#L137-L260

1. **The hub runs the loop in-process.** One WebSocket plus HTTP server per user, default
   `127.0.0.1:25463`, path `/hub`, singleton by OS lock. The types declare `SpokeRecord` and
   `assignedSpokeId`, but no code creates spoke processes. One daemon crash takes every
   session with it.
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-server-transport.ts#L334-L343
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/shared/src/hub.ts#L296-L310
   **[absence confirmed]** (spokes)
2. **Hub authentication: a token, and a loopback-Origin shortcut.** Each start mints
   `randomBytes(32).hex` into a 0600 discovery file. An upgrade is accepted if
   `Sec-WebSocket-Protocol` carries `cline-hub-auth.<token>` (constant-time), **or** if the hub
   is bound to loopback and the `Origin` hostname is `localhost`, `127.0.0.1` or `::1`. Any
   local process can send that header, and loopback TCP is open to every local user, so the
   file permission does not protect this path. Origin-only connections are marked
   `tokenAuthenticated: false` and refused only `providers.*`, `mcp.*` and cross-workspace
   branches; session creation, run, send-input and approval do not read the flag. HTTP
   `/status`, `/drain`, `/shutdown` need a Bearer; `/health`, `/version` are open.
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L676-L722
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-server-transport.ts#L874-L884
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/browser-websocket.ts#L69-L113
   **[corrected in review]**
3. **Protocol and client-contributed executors.** JSON frames (`command`, `reply`,
   `stream.subscribe`, events), about 40 commands. A client registers capabilities including
   `tool_executor.<readFile|search|bash|webFetch|editor|applyPatch|...>`; when the loop needs
   such a tool the hub sends `capability.requested` to that client and only the target may
   answer; if it is gone the call waits 7 s and fails the run. `approval.requested` (with the
   tool input) is broadcast, held with no timeout, re-issued on resubscribe, and
   `approval.respond` checks only that the id exists, not who answers. `session.messages`
   returns any session's transcript with no owner check.
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/shared/src/hub.ts#L192-L224
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/capability-handlers.ts#L174-L180
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/approval-handlers.ts#L123-L170
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/session-handlers.ts#L1055-L1077
   **[checked in review]**
4. **The controlling side sets the policy.** `session.create` takes `toolPolicies` (a
   global `autoApprove` becomes the session's setting), `systemPrompt`, `cwd`,
   `workspaceRoot`, `maxIterations` from the client; no workspace-authority check on
   `payload.cwd` was found. Combined with the Origin shortcut, a local page could start an
   auto-approved session in any directory (code reading, not exercised).
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/session-handlers.ts#L497-L560
   **[checked in review]** (as an inference: **[inferred]**).
5. **Resume by sequence.** Every published event is appended to a SQLite table with an
   autoincrement `sequence` before fan-out; `stream.subscribe {sinceSequence}` replays pages of
   200 (up to 1,000 pages) then tails live. Retention 7 days, 200,000 rows or 64 MiB. The log
   is a replay aid, not the source of truth. Client backoff 250 ms to 5 s with 50% jitter;
   ping every 30 s.
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-event-log.ts#L78-L90
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/browser-websocket.ts#L373-L450
   **[checked in review]**
6. **SSH remote: upload the CLI, start a loopback hub, tunnel to it.** `ssh` with
   `BatchMode=yes`, `StrictHostKeyChecking=yes`, `ConnectTimeout=30`, an optional pinned
   `UserKnownHostsFile`; the platform binary is uploaded by `umask 077; cat > <tmp>` into a
   hash-named file under `~/.cline/remote` (no network installer); `cline hub ensure --host
   127.0.0.1 --port 0` runs remotely and **prints the hub's auth token on stdout**; a random
   local port is reserved and `ssh -N -L` opened (no `ExitOnForwardFailure`); the remote hub
   must report a loopback URL. The desktop sidecar attaches the provider's API key or access
   token (refresh token removed) to each `session.create`, which travels through the tunnel.
   The loop and tools run on the remote host. A failed connect persists a cleanup record.
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L325-L400
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L1436-L1490
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/examples/desktop-app/sidecar/chat-session.ts#L921-L980
   **[checked in review]**
7. **Core/host split (JetBrains and CLI-hosted build).** The core serves ProtoBus gRPC on
   `127.0.0.1:26040` with **insecure credentials and no authentication**; the host bridge
   back to the IDE uses a per-spawn secret from `CLINE_CORE_CONNECTION_TOKEN`, which the core
   captures into memory and deletes from `process.env` before launching any child.
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/hosts/external/host-bridge-auth.ts#L4-L47
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/standalone/protobus-service.ts#L11-L36
   **[unchecked]**
8. **Other listeners and approvals.** The `cline hub dashboard` web app binds
   `127.0.0.1:8787`, requires `ROOM_SECRET` off loopback, takes it as `?roomSecret=` and
   compares with `===`. The CLI's desktop-approval IPC is a pair of JSON files in a directory
   anyone with write access can approve through, denied after 5 minutes. ACP mode auto-approve
   is off unless explicit; the CLI default is on.
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cline-hub/src/server/browser-auth.ts#L109-L122
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/tools/tool-approval.ts#L29-L101
   https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/main.ts#L802-L811
   **[checked in review]**

### OpenHands

What is in this repository is Canvas, the browser client, plus its launchers. The Agent
Server (loop, tools, persistence, `session_protocol.py`, ACP spawn) is in
`OpenHands/software-agent-sdk`, not cloned. The browser dials each Agent Server directly (or
through a path-prefix ingress with no authentication of its own). There is no pairing, no
per-dispatch credential, no fencing and no node-side refusal in this repository.
https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/ingress.mjs#L1-L20

1. **The session socket is an envelope protocol.** One socket per conversation at
   `/sockets/session/<conversationId>`; "the URL is the protocol version, there is no
   handshake". Frames are a closed set: `sync` (the range about to be replayed), `durable`
   (one persisted event plus its `seq`), `transient` (published, never persisted, no `seq`),
   `item_started` / `delta` / `item_aborted` (streaming progress keyed by the id the durable
   event will later carry), and `error`. Unknown frames are dropped. Progress frames are never
   replayed, so on every reopen the client discards open streaming slots and waits for the
   durable event. A `full_state` snapshot is sent as a `transient` frame after `sync`.
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/types/agent-server/session-frames.ts#L1-L79
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/types/agent-server/session-frames.ts#L81-L107
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/contexts/conversation-websocket-context.tsx#L1305-L1312
   **[checked in review]** (client side)
2. **Resume is "everything after seq N".** The first connect asks `after_seq=<n>`, where `n`
   comes from a REST event count read *before* the history page; each socket then keeps its
   own cursor: the highest seq with nothing missing below it, stepping over a hole after more
   than 1,024 later seqs (the server skips unreadable events). Replayed events are deduplicated
   by id and their side effects skipped. Backoff 1 s to 30 s with 30% jitter; a 10 s watchdog
   on CONNECTING.
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/session-seq-cursor.ts#L1-L63
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/hooks/query/conversation-history-query-options.ts#L109-L158
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/contexts/conversation-websocket-context.tsx#L793-L823
   **[checked in review]**
3. **WebSocket auth is the first message.** After `onopen` the client sends
   `{"type":"auth","session_api_key": ...}`, so the key never appears in a URL (only
   `after_seq` is in the URL). Whether the server closes a socket that never authenticates is
   server code, not here. HTTP uses `X-Session-API-Key` locally and `Authorization: Bearer` for
   Cloud.
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/websocket-auth.ts#L1-L18
   **[checked in review]**
4. **Send path.** Over the socket with `run: true` only once the socket has delivered a frame
   (the server subscribes a new socket only after the running step releases the lock);
   otherwise over HTTP. On Cloud with a paused sandbox, held in browser memory.
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/contexts/conversation-websocket-context.tsx#L1550-L1566
   **[checked in review]**
5. **One shared key, in the page and in the agent.** The launcher generates a 256-bit key
   (0600 file), injects it into `index.html` and `localStorage` on a loopback bind (refused
   off loopback unless `--allow-lan-session-key`), and every key holder owns everything. The
   launcher also puts the same key in the agent-server's environment as
   `OPENHANDS_AUTOMATION_API_KEY` "to terminal commands spawned by the agent-server" and tells
   the agent to send it; with it a prompt-injected agent can answer its own confirmations and
   run server-level bash. Docker mode binds `0.0.0.0` by default.
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/static-server.mjs#L388-L406
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L528-L543
   **[checked in review]**
6. **The controlling side sends executable configuration.** The start request is the whole
   configuration (`agent_profile_id`, `workspace`, `confirmation_policy`, `client_tools`,
   `secrets`, `hook_config`, `plugins`, `tool_module_qualnames`, `container` limits);
   `tool_module_qualnames` names Python modules the server imports; container limits are
   forwarded with only a 0.01-CPU client minimum. Any key holder can also run commands at
   server level through `BashClient.executeCommand`, outside any conversation.
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1274-L1306
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L808-L835
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/bash-service/bash-service.api.ts#L24-L44
   Start payload **[unchecked]**; container limits and server-level bash **[checked in
   review]**.
7. **Client tools run in the browser.** `client_tools` registered at start
   (`launch_child_conversation`, `request_secret`, ...) are acknowledged by the server
   immediately; the browser does the work and posts the result as a prefixed user message.
   Replays are guarded only by a `localStorage` claim ledger, so a second browser can repeat
   a launch; with no tab open, nothing runs the call.
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/claim-client-tool-call.ts#L1-L34
   **[checked in review]**
8. **DigitalOcean Managed Agents (opt-in, `MARS_WEB=1`).** The Node side dials the
   provider's port-forward WebSocket with `Authorization: Bearer` and exposes it as a
   loopback listener registered as an ordinary local backend. Tokens never cross into the
   renderer.
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/tunnel-client.mjs#L148-L154
   **[corrected in review]** (opt-in noted)
9. **Agent Server as ACP client.** Per the docs, the server spawns Claude Code, Codex, Gemini
   CLI, Pi or OpenCode adapters over stdio and resolves their credentials by secret name at
   spawn (`LookupSecret` carries the backend's own auth header inside the start request).
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docs/ACP_AGENTS.md#L8-L31
   https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1112-L1144
   **[unchecked]**; the LookupSecret header **[checked in review]** (safety file).

### Zed

The Zed Agent runs inside the editor process; external ACP agents are child processes over
stdio JSON-RPC (wrapped by the remote client for remote projects, not studied). Zed is an
**ACP client**; it implements no server, link or worker.

1. **Install and launch.** The registry index comes from
   `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json` with no signature
   check; archive entries may omit `sha256` (non-GitHub URLs then download unverified), and npx
   packages take the registry version as a ceiling, not a pin. The command is built by
   `ShellBuilder`; env is **added to the inherited environment** (no scrubbing); stdout is read
   line by line and every line logged; `OwnedChild` kills on drop. v2 negotiation only with the
   `acp-beta` flag, else v1.
   https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/project/src/agent_server_store.rs#L1131-L1340
   https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/transport.rs#L31-L110
   **[unchecked]**
2. **What it serves.** Handlers for `session/request_permission`, `fs/read_text_file`,
   `fs/write_text_file`, `terminal/create|kill|release|output|wait_for_exit`; capabilities
   advertise fs and `terminal: true`. Reads and writes go through project buffers and are
   restricted to the project's worktrees. **`terminal/create` runs in a normal project terminal,
   without a sandbox and without a permission check.**
   https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/v1.rs#L442-L480
   https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/v1.rs#L10549-L10590
   **[unchecked]**
3. **Permissions not applied to external agents.** The settings comment says deny and
   confirm "still take precedence" for external agents; no reader of `tool_permissions` exists
   in `agent_ui`, `acp_thread` or `agent_servers`; `handle_request_permission` forwards the
   agent's options to the user without a settings lookup.
   https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/settings_content/src/agent.rs#L403-L413
   https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/v1.rs#L10172-L10225
   **[absence confirmed]** (in the three crates searched)
4. The ACP schema is the crates.io dependency `agent-client-protocol` 5.1.0 (schema 1.16.0),
   not vendored.
   https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/Cargo.toml#L538
   **[checked in review]**

### Warp

The split is inverted and partly outside the repository: **the agent loop and model call run
on Warp's server**; the client is a tool executor, UI and policy gate. Every model turn is one
`POST {server}/ai/multi-agent` (protobuf, bearer token) answered as an SSE stream; the client
runs returned tool calls locally and sends results in the next request. Tool availability is
decided client-side from session type (local, remote SSH host, none).
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_multi_agent_client/src/lib.rs#L48-L89
https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api/impl.rs#L225-L303
**[checked in review]** (client side)

1. **BYO keys travel to the controller.** Provider keys from the OS keychain are copied into
   `Request.settings.api_keys` on every request, so model calls go through Warp's server.
   https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api/impl.rs#L59-L63
   **[unchecked]**
2. **ACP client for cloud and headless runs.** Protocol version 1, client capabilities
   `fs.readTextFile`/`writeTextFile`, `terminal: false`. The adapter runs as a command in the
   driver's PTY; a bridge subcommand connects back to a Unix socket (or loopback TCP) with a
   one-time 32-byte token and hands it to the agent as stdio, so protocol bytes never cross
   the PTY. The `fs/read_text_file` handler reads any path with no policy check.
   https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/acp/bridge.rs#L43-L59
   https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/acp/mod.rs#L1553-L1556
   **[checked in review]**
3. **ACP permission policy.** Only `Execute` and file-write requests are evaluated; a `Deny`
   only for an explicit denylist match or a protected path; every other denial reason is
   logged and allowed ("allowing in an unattended run"); `AllowAlways` is picked before
   `AllowOnce`; with no bound conversation, allowed.
   https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/acp/policy.rs#L80-L117
   **[checked in review]**
4. **Unattended CLI children run with their own permission systems off.** Hidden panes run
   `claude --dangerously-skip-permissions`, `codex --dangerously-bypass-approvals-and-sandbox`;
   the driver pre-writes trust for the working directory into their configs. Warp's own
   policy is not applied to these.
   https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/pane_group/pane/local_harness_launch.rs#L120-L137
   **[checked in review]**
5. **Inbox and event stream with a server-side cursor.** Agent messages are REST posts;
   lifecycle events arrive on `GET /api/v1/agent/events/stream?run_ids[]=...&since=<seq>`
   with a monotonic `sequence`; the client persists its cursor on the server
   (`PATCH agent/runs/{id}/event-sequence`) so a restored driver does not replay. Backoff 1, 2,
   5, 10 s; stream recycled every 14 minutes; give up after 3 consecutive 401/403.
   https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/server/server_api/ai.rs#L3715-L3730
   https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_events/driver.rs#L17-L36
   **[unchecked]**
6. **Local-to-cloud hand-off.** Refused while a long-running command or a child is in
   progress; the workspace is derived from paths the agent wrote in the last 500 tool calls;
   per repo `git diff --binary HEAD` plus untracked non-ignored files; 100 files, 25 MiB per
   file.
   https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/handoff/pipeline.rs#L440-L461
   **[checked in review]**
7. **Self-hosted workers.** Only a listing call and a design note; the worker and the
   server-to-worker WebSocket are in another repository.
   https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/server/server_api/ai.rs#L1374-L1392
   **[inferred]** (design note only)
8. Locally typed CLI agents are observed through unauthenticated OSC 777 notifications on the
   PTY; any process that writes to the PTY can emit them.
   https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_core/src/cli_agent_protocol.rs#L1-L38
   **[unchecked]**

### OpenCodex

A model gateway in one Bun process with up to four listeners distinguished by an `ingress`
value that selects route allow-list and auth policy. It has two worker features: **Remote
Workspace** (agent on the hub, tools on an executor that dials out) and **Remote Link**
(a Child machine relays model calls to a Home machine through an SSH tunnel).
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index/serve-options.ts#L330-L364

**Listeners.**

1. Data plane: no auth on loopback (no token read at all); a mandatory shared secret on any
   other bind (`assertServerAuthConfig` throws at startup). Keys are literal text in
   `config.json`, compared with `timingSafeEqual`; a rotation overlap of 10 minutes.
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/auth-cors.ts#L337-L339
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/auth-cors.ts#L383-L392
   **[checked in review]**
2. An optional unauthenticated loopback listener serves a short inference allow-list, with a
   startup warning that "any local process can use it"; the hub-management ingress admits
   `/api/*`, the GUI and, when enabled, the Remote Workspace pair and agent routes, and refuses
   `/v1/*`.
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index.ts#L404-L433
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index.ts#L790-L802
   **[unchecked]**
3. Management: an admin token that may not equal any data-plane secret (startup fails),
   pid-and-port-bound single-use HMAC capability headers, and GUI sessions with a 32-byte
   CSRF token bound to server and browser origins (5 minutes on loopback, 12 hours remote).
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/management-auth.ts#L222-L252
   **[unchecked]**

**Back-pressure.** The generic SSE bridge is a pull stream whose only buffer is the default
high-water mark of one, so a slow client stops the upstream read; client disconnect cancels
the upstream fetch. Other paths buffer more (combo preflight up to 4 MiB, Cursor transport
32 MiB). Inbound bodies take reservations from a global FIFO byte budget (32 waiters, 120 s,
then 503 with `Retry-After: 1`); 256 concurrent turns.
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/bridge/sse.ts#L1600-L1606
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/inbound-body-admission.ts#L143-L193
**[corrected in review]** (bridge) / **[checked in review]** (budget)

**Remote Workspace.**

1. *Where the loop runs.* On the hub, in a `mkdtemp` 0700 directory, as `codex app-server
   --listen stdio://` whose only required tools are an auto-approved local MCP bridge
   (`ocx_remote_workspace`), with the user's MCP servers and hooks disabled and every turn
   sent with `approvalPolicy: "never"`. "Use no other tool, stop rather than fall back
   locally" is a prompt instruction plus the absence of a local tool path, not a hard
   guarantee.
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-codex-runtime.ts#L392-L414
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-tools.ts#L212-L225
   **[checked in review]**
2. *Pairing.* A dashboard consent session asks for a grant: 12 characters from a 32-letter
   alphabet (about 60 bits), 10 minutes, at most 16 outstanding, stored as SHA-256. The
   executor reads the code from stdin and posts name, platform, Ed25519 public key, requested
   capabilities and root ids and labels (no paths). The hub refuses requests carrying
   `Origin`, rate-limits by kernel-observed peer address (10 failures in 10 minutes, then 429
   with `Retry-After`), compares in constant time, **deletes the grant before validating the
   metadata**, and returns a bearer `ocxrw_<43 base64url>` plus the hub's public key; it stores
   only `SHA-256(token)`. The hub's own Ed25519 private key is in a 0600 file in the clear.
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L377-L425
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index/serve-options.ts#L484-L513
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L131-L138
   **[checked in review]**
3. *Who dials and fencing.* The executor opens `wss://hub/remote-workspace/agent` with the
   bearer, sends `presence` with capabilities and a heartbeat nonce, and must see
   `presence_ack` within 10 s. The hub refuses `Origin`, finds the device by constant-time
   compare over stored hashes, and **closes any previous connection for that device**
   ("executor reconnected"). A presence that claims more than the device's stored
   capabilities throws; the stored capabilities are what the executor declared at pairing
   (the dashboard grant carries none). `revokeDevice` removes durable state first, then closes.
   The bearer has no expiry and is not bound to a connection; fencing is latest-connection-wins,
   not a generation in each message; there is no message replay across reconnects.
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index/serve-options.ts#L567-L603
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L427-L463
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L386-L450
   **[corrected in review]**
4. *Per-session handshake.* The hub sends a hello signed with its Ed25519 key over a
   transcript (version, session id, device id, command profile, capabilities, ephemeral P-256
   key, nonce); the executor verifies against the **pinned** hub key, **intersects the
   requested capabilities with what it allows**, and answers signed over a transcript
   including the hash of the client's. HKDF-SHA256 keys per direction; AES-256-GCM frames with
   a counter that must equal the expected value (replays and reordering fail); 64 KiB per
   frame. Key confirmation is implicit. The cipher terminates inside the hub, so it protects
   against a TLS-terminating proxy, not against the hub.
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/crypto.ts#L214-L286
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-agent-connection.ts#L286-L330
   **[unchecked]** (spot-checked: transcript, counter, 64 KiB); security reading
   **[inferred]**.
5. *RPC v2.* `prepare` (full request, timeout 1 to 120,000 ms), then a separate `grant` sent
   only if not timed out, `cancel`, `response`; an old endpoint that executes request frames
   immediately cannot participate. 8 active requests, 65 s default timeout, cancel on timeout.
   The executor refuses a request whose session or root differs from the handshake, a tool
   needing an ungranted capability, or a duplicate id or grant. Tools: `list_directory`,
   `read_file`, `write_file` (with `expectedSha256` compare-and-swap, or null for create),
   `exec` (argv vector, max 64, relative cwd, at most 60 s); results capped at 256 KiB. A
   failed encrypted write closes the transport and fails every pending request.
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-rpc.ts#L162-L207
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-rpc.ts#L298-L346
   **[unchecked]**
6. *What the executor refuses, independent of the hub.* Paths relative, no NUL, contained
   inside the root after resolution; every component `lstat`ed and any symlink refused; opened
   files checked by `fstat` against `lstat` for identity and type; more than one hard link
   refused; the root's device, inode and birth time rechecked before every operation; writes
   temp-file plus rename with CAS; one serialised promise chain; `exec` refused outright
   without a configured OS sandbox. The bubblewrap command clears the environment, unshares
   network, binds one writable root, and has no seccomp, `--unshare-user` or `--cap-drop`. A
   probe gates `exec`, not writes; the hub picks `accessMode` per session (default
   read-only).
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-executor.ts#L122-L166
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-command-runner.ts#L375-L436
   https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-sessions.ts#L406-L418
   Refusals **[corrected in review]**; bubblewrap and probe **[checked in review]**.

**Remote Link (`ocx-child`).** The Child runs a Rust HTTP relay. On every upstream
connection it sends a challenge to the Home's relay-auth path with a nonce and an HMAC keyed
by the link token's fingerprint, and requires a 204 with a matching listener proof (so a
local impostor on the tunnel port fails). Caller credential headers (`authorization`,
`x-api-key`, `cookie`, ...) are stripped and the link token injected; the Home holds the
provider keys. Limits: 256 MiB body, 32 workers, 300 s timeout.
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/native/ocx-child/src/relay.rs#L225-L246
https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/native/ocx-child/src/http_wire.rs#L3-L16
**[checked in review]** (Home side not traced)

## 3. Comparison

### 3.1 Topology, identity and refusal

"Loop" is the agent loop (model calls and tool-call decisions); "tools" is where tool calls
execute. "Refuses" is what the executing side can refuse on its own authority, whatever the
controlling side sends.

| System | Loop runs | Tools run | Who dials | Transport | Pairing and identity | Executing side refuses |
|---|---|---|---|---|---|---|
| Codex app-server | app-server | app-server, or an exec-server | client to server | stdio JSONL, `unix://`, `ws://` JSON-RPC | capability token or HS256 JWT at upgrade; none on loopback | n/a (single process) |
| Codex exec-server | harness | executor | executor to relay (`--remote`); or harness to `--listen` / `ssh ... --listen stdio` | relay WebSocket + Noise IK, 128 streams | executor key pinned from a registry; registry authorises harness key | nothing: the sandbox comes from the request; none means unsandboxed |
| Codex remote control | local app-server | local | app-server to vendor relay | WebSocket v3, segmented envelopes | enrolment token with `expires_at`; relay host allow-listed | n/a |
| OpenCode service | server | server (or SDK `WorkspaceDriver`) | client to server | HTTP + SSE | one shared password; pairing code to password-derived HMAC token | driver only executes; decision stays on server |
| OpenCode SSH (desktop) | remote server | remote server | desktop over SSH, then `ssh -L` | HTTP through tunnel | password read from remote `service.json` over SSH | n/a |
| OpenClaw cloud worker | worker (inference brokered by Gateway) | worker | worker to Gateway WS; Gateway also SSHes in | WebSocket JSON frames, closed method set | setup code binds box to environment; per-dispatch credential | worker checks the hello matches its request; model choice approved by Gateway |
| OpenClaw node | Gateway | node (`system.run` and others) | node to Gateway WS | WebSocket, protocol 4 | Ed25519 device proof over nonce + token; operator approval or SSH auto-approval | `system.run`: own security/ask, plan binding, PATH, policy drift; but a forwarded approval satisfies its allow-list |
| Hermes viewers (CLI, TUI, ACP) | gateway | gateway | viewer to gateway | Unix socket for a ticket, then WebSocket | peer uid; 30 s single-use ticket in a subprotocol | ACP viewer never runs tools |
| Hermes relay | gateway | gateway | gateway to relay | WSS, NDJSON | HMAC upgrade token (300 s), rotating secrets | n/a |
| Hermes SSH / Modal / Daytona | host | remote | host over SSH or SDK | `ssh bash -c` / SDK exec | `accept-new` (TOFU) | nothing |
| Oh My Pi RPC / ACP | embedded process | same | parent spawns child | NDJSON / ACP over stdio | pipe ownership | n/a |
| Oh My Pi collab | host | host | host and guests to relay | WSS, AES-256-GCM sealed frames | room key and write token in link fragment | guest prompts run under host's mode; read-only peers refused |
| Pi pi-env | controller | daemon | controller over SSH | length-prefixed binary frames on stdio | SSH strict host keys, private known-hosts, `scanHostKey` | nothing (no root, no allow-list) |
| DeepSeek SSH | local | remote helper | local over SSH | framed JSON RPC (protocol 3); TLS-PSK per stream | SSH config; self-reported helper hash | helper runs its own sandbox chain; reported enforcement trusted |
| DeepSeek web | host process | host process | browser to host | HTTP POST + one WebSocket mux | launch token to signed cookie | n/a |
| goose serve | server | server | client to server | HTTP / WebSocket ACP | one shared secret (header or `?token=`) | n/a |
| goose roaming | host | host | peer to host (iroh QUIC) | QUIC streams carrying ACP | key in a trust book; fingerprint compared out of band | host forces cwd on new/load/fork (bypassable by update) |
| Cline hub | hub | hub, or a client's contributed executors | client to hub | one WebSocket, JSON frames | per-start token, or loopback `Origin` | n/a; client sets tool policies |
| Cline SSH remote | remote hub | remote hub | desktop over SSH, then `ssh -L` | hub WebSocket through tunnel | token printed on remote stdout | n/a |
| OpenHands Canvas to Agent Server | Agent Server (not in repo) | Agent Server, or browser for client tools | browser to server | WebSocket envelope + REST | one shared key, first message | not in repo |
| Zed (ACP client) | external agent | agent, or Zed fs/terminal | Zed spawns agent | ACP stdio | none | fs limited to worktrees; terminal unconfined |
| Warp client | Warp server (not in repo) | client | client to Warp server | HTTPS POST + SSE protobuf | bearer | client policy gate |
| OpenCodex Remote Workspace | hub | executor | executor to hub | WSS + per-session signed ECDH, AES-GCM | single-use code to bearer (hash stored); pinned hub key | paths, symlinks, hard links, root identity, CAS writes, `exec` without sandbox; intersects capabilities |
| OpenCodex Remote Link | child's Codex | child | child relay to Home through SSH tunnel | HTTP | link token; mutual HMAC challenge | strips caller credentials |

### 3.2 Fencing, credentials, resume, frames, listener auth

| System | Fencing of stale workers | Credential per dispatch | Resume mechanism | Durable vs transient frames | Listener authentication |
|---|---|---|---|---|---|
| Codex app-server | file lock per thread (one host) | none | `thread/resume` by id/path/history; rejoin running thread | none distinguished on the wire | non-loopback refused without auth; loopback none; Origin 403; no post-upgrade expiry |
| Codex exec-server | one attachment per session id, 80 s retention; no epoch | rotating executor credentials over stdin (registry) | `resumeSessionId`; `process/read` cursor; relay `Resume{next_seq}` | relay `Data{seq}` with ack bits | `--listen` auth optional, non-loopback allowed |
| Codex remote control | server side, not in repo | enrolment token | `x-codex-subscribe-cursor` | envelope `seq_id` | outbound only |
| OpenCode | none; restart sweep assumes owner dead | none | clients refetch REST; log API (`after`, watermark, `log.synced`) exists but `persist` off | durable event table opt-in; SSE volatile | password required; `===`; `?auth_token=` accepted; SDK router unauthenticated |
| OpenClaw cloud worker | global `owner_epoch` revalidated per request; writer claim CAS checked inside the write | 32 bytes, domain-tagged hash bound to turn claim, 10 min | same claim-bound credential; same call id; commit ledger `(session, epoch, seq)` caches replies | `transcript.commit` (sequenced, ledgered) vs `live-event` | opaque `invalid-handshake`, failures rate-limited |
| OpenClaw node | (pairing revision; per-run approval binding) | none | not traced | `event` frames carry optional `seq` | device proof; token default; mode `none` exists |
| Hermes gateway | `runtime_epoch` + per-session `runtime_generation`, checked at settle | none | lost turns marked `unknown`, never replayed | admissions durable before ack | control socket peer uid; non-loopback dashboard needs auth; API key required |
| Hermes relay | 4401 after handshake is terminal | HMAC token, 300 s | connector redelivers unacked `bufferId` | `inbound_ack` after durable admission | n/a (dials out) |
| Oh My Pi | OS file lock per session; sibling file on conflict | none | RPC `get_entries {since}`; collab snapshot then entries | v2 chunking; collab sealed | RPC none (pipe); broker one bearer `Set.has`; gateway digests, `--no-auth` |
| Pi pi-env | pipe lifetime; 30 s silence kills process groups | none (SSH) | none: lost requests are `lost: true`, handles die | n/a | SSH only; token is a sync marker |
| DeepSeek | `flock` with no expiry (one host); SSH helper lease 30 s | TLS-PSK key per stream | `follow` snapshot per reopen + client gap repair; no cursor | durable events vs assistant frames with `revision` | Host/Origin fence + cookie; wildcard bind refused; launch token in URL |
| goose | none | none | `session/load` replays whole conversation (or tail) | n/a | secret required unless `--dangerously-unauthenticated`; `?token=` accepted |
| goose roaming | trust book re-read per connect; 2 s revocation poll closes connections | none | as above | n/a | QUIC-TLS key identity |
| Cline hub | none (single daemon) | provider key sent per session (SSH) | `stream.subscribe {sinceSequence}` from a 7-day log | log is a replay aid; messages on disk canonical | token or loopback `Origin`; ProtoBus gRPC unauthenticated |
| OpenHands | not in repo | per-conversation `session_api_key` on Cloud | `after_seq` with gap-tolerant cursor; `sync` frame names the range | `durable` / `transient` / `delta` frames, explicitly | shared key, first message; ingress has none |
| Zed | n/a | n/a | n/a | n/a | no listener |
| Warp | server side | BYO keys sent in every request; HPKE-sealed managed secrets | inbox `since=<seq>` with cursor stored server-side | event `sequence` | client is never a listener (ACP bridge one-time token) |
| OpenCodex Remote Workspace | latest connection wins per device; bearer has no expiry | none per dispatch; device bearer | none: sessions die with the connection | counter-bound encrypted frames | pairing route unauthenticated (Origin refused, rate-limited); agent route bearer |

### 3.3 Listeners that can run without authentication

| Project | Listener | Condition | Citation |
|---|---|---|---|
| Codex | app-server `ws://` | loopback bind, no auth configured (default) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L91-L96 |
| Codex | exec-server `--listen ws://IP:PORT` | any bind, auth optional | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-cli/src/command.rs#L56-L160 |
| Codex | daemon control socket | always; protected by directory mode only | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L148-L246 |
| OpenCode | in-memory SDK router | built without a password | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/fetch.ts#L34-L36 |
| OpenCode | `/auth/connect/:code` | always (the code is the credential) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/middleware/authorization.ts#L87-L94 |
| OpenClaw | Gateway | auth mode `none` | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/handshake-auth-helpers.ts#L165-L187 |
| Hermes | control socket | same-uid peer only, no secret | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/control_socket.py#L411-L425 |
| Oh My Pi | auth gateway | `--no-auth` (with `Access-Control-Allow-Origin: *`) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-gateway-cli.ts#L452-L468 |
| Oh My Pi | browser relay | no token configured | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/browser/relay/server.ts#L130-L160 |
| Oh My Pi, Pi | RPC and ACP modes | always (stdio; the parent is the boundary) | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L401-L738 |
| goose | `goose serve` | `--dangerously-unauthenticated`, any bind | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L1821-L1834 |
| Cline | hub WebSocket | loopback bind plus a loopback `Origin` header | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L676-L722 |
| Cline | core ProtoBus gRPC `127.0.0.1:26040` | always | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/standalone/protobus-service.ts#L11-L36 |
| Cline | hub dashboard `127.0.0.1:8787` | loopback default, no `ROOM_SECRET` | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cline-hub/src/options.ts#L72-L84 |
| OpenHands | ingress | always (auth left to upstreams) | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/ingress.mjs#L185-L226 |
| OpenCodex | data plane | loopback bind (no token read) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/auth-cors.ts#L337-L339 |
| OpenCodex | unauthenticated loopback companion | when configured (logged warning) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index.ts#L790-L802 |
| OpenCodex | `/remote-workspace/pair` | always (the code is the credential; Origin refused, rate-limited) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index/serve-options.ts#L484-L513 |

Health and version probes are open almost everywhere (Codex `/readyz` `/healthz`, goose
`/health` `/status`, Cline `/health` `/version`, Oh My Pi broker `/v1/healthz`).

## 4. Patterns and trade-offs

### 4.1 Where the loop sits relative to the tools

- **Loop on the controlling side, tools remote**: Codex exec-server, OpenCode workspace
  drivers, Pi pi-env, DeepSeek SSH, Hermes SSH/Modal/Daytona, OpenCodex Remote Workspace,
  Cline client-contributed executors, and Warp (loop on the vendor's server, tools on the
  client). The executor is a "dumb" tool host, so the *policy* travels with each request
  (Codex `sandbox` intent, OpenCodex capability set chosen at session creation). The cost is
  that the executor either trusts the request (Codex, pi-env, Warp's client trusts the
  server) or has to re-implement path and identity checks itself (OpenCodex's executor does,
  thoroughly). Every one of these needs a round trip per tool call, and a disconnect loses
  every in-flight call (pi-env `lost: true`, DeepSeek "loss is final", OpenCodex sessions die
  with the connection).
- **Loop and tools on the executing side**: OpenClaw cloud workers (with inference brokered
  back to the Gateway), Cline SSH remote (a whole hub on the host), OpenCode SSH (a whole
  server on the host), goose roaming (the host owns everything), OpenHands Agent Server, and
  rizzy-harness. The controlling side then needs fencing and a commit protocol for what the
  executor writes back; only OpenClaw built one.
- **Single process**: Oh My Pi, Pi, DeepSeek's web profile, Hermes' gateway with viewers.
  No link to fence; one-host locks suffice.

### 4.2 Who dials

- **Executor dials out** (no listener on the executor): Codex exec-server `--remote`,
  Codex remote control, OpenClaw workers and nodes, Hermes relay, OpenCodex executors,
  Oh My Pi collab, DeepSeek LiveShare. All of them need reconnect semantics; only OpenClaw
  fences a reconnect with a generation and a per-request re-check. OpenCodex and Codex fence
  by "one connection at a time" (latest wins, or first wins with a retention window).
- **Controller dials over SSH**: OpenCode, Cline, Pi, DeepSeek, Hermes, Codex `program`,
  and OpenClaw provisioning. The executor's lifetime is tied to the SSH pipe (pi-env kills its
  process groups on EOF or 30 s silence; DeepSeek's helper holds a 30 s lease), which fences
  stale executors for free but makes the session die with the network. The flag sets differ a
  lot: pi-env's (`ClearAllForwardings`, `SendEnv=-*`, `RemoteCommand=none`, private
  known-hosts, explicit `scanHostKey`) and OpenClaw's (`-F none`, `IdentityAgent=none`) are
  strict; Hermes and Oh My Pi use `accept-new`. Several SSH designs then move a secret back
  over the channel: OpenCode reads the remote password from `service.json`, Cline reads the
  hub token from remote stdout.

### 4.3 Fencing mechanisms

| Approach | Projects | What it covers | What it misses |
|---|---|---|---|
| None, assume the old owner is dead | OpenCode restart sweep, goose | single process restarts | two processes on one database; any network split |
| Pipe or lease lifetime | pi-env, DeepSeek SSH helper, OpenCode stdio child, Codex `exit-on-stdin-close` | a dead controller cannot leave a live executor | the session cannot survive a disconnect |
| OS lock with no expiry | DeepSeek `flock`, Codex thread lock, Oh My Pi lease | two processes on one filesystem | anything across hosts; network filesystems |
| One attachment plus retention | Codex exec-server (80 s) | a second client | a stale client that still holds the socket, up to the detection time |
| Latest connection wins | OpenCodex Remote Workspace; our master (`nodes.rs:208-212` per the design note) | a reconnect replaces a dead socket | frames already read from the old socket; a stolen key flapping |
| Monotonic epoch, re-checked per request, plus writer CAS inside the write | OpenClaw | stale worker after re-assignment, rotation, destroy; duplicate commits (ledger) | unfenced writes that carry no expected writer |
| Epoch plus per-session generation, checked at settle | Hermes | a restart's lost turns; a stale settle | it is one host; it fences settlement, not a remote writer |

The only design that fences a remote executor's writes is OpenClaw's, and it relies on three
pieces together: a global, never-reused epoch; a per-dispatch credential whose hash is bound
to the epoch and turn claim; and an authority check inside the same database transaction as
the write.

### 4.4 Per-dispatch credentials and where model keys live

- **Keys stay on the controller; the executor holds a proxy credential**: OpenClaw
  (`worker.inference.start` by model reference), OpenCodex Remote Link (link token; the
  relay strips caller credentials), Oh My Pi auth gateway (scoped client digests). Cost: model
  traffic goes through the controller.
- **Keys go to the executor per session**: Cline SSH (API key or access token in every
  `session.create`), OpenHands (`LookupSecret` resolved at spawn), and our design note's
  `session.grant`. Cost: the executor's operator can read them; removal cannot un-send them.
- **Keys go to the controller from the client**: Warp (BYO keys in every request to Warp's
  server).
- **Credential distributors**: Oh My Pi's auth broker hands clients API keys and access
  tokens.
- **Leaks that defeat the model**: OpenClaw's worker receives a GitHub token that its agent
  can read; OpenHands puts the stack's session key in the agent's environment; OpenCode leaves
  its server password in the shell environment outside stdio mode. In each case the agent can
  reach the control plane.

### 4.5 Resume and replay

- **Cursor over a durable log**: Cline `sinceSequence` (global sequence, 7-day replay aid),
  OpenHands `after_seq` (per conversation, gap-tolerant because the server skips unreadable
  events), OpenCode's log API (exclusive cursor, subscribe first, watermark, `log.synced`
  marker; unused by its own clients), Warp `since=<seq>` with the cursor stored server-side,
  Oh My Pi `get_entries {since}`, Codex remote-control subscribe cursor and exec-server
  `process/read` cursor.
- **Snapshot per reconnect**: DeepSeek (`follow` yields a fresh snapshot; client repairs
  gaps by paging), OpenCode's clients (refetch over REST). Cost: a full read per reconnect and
  a race the client must hide.
- **Replay the conversation**: goose `session/load`.
- **Acknowledge after durable commit**: Hermes (`inbound_ack` after admission), OpenClaw (ACK
  cursor after the authority check, cached terminal replies).
- **Idempotency keys**: DeepSeek (`requestId` stored in the logged message), OpenClaw
  (pending inputs unique on idempotency key with a request hash; commit idempotency keys from
  session, epoch, seq, index), Hermes (admission unique key with payload digest).
- **What is never replayed**: a tool call whose outcome is unknown (DeepSeek closers, Pi
  durable `replay: "unsafe"`, OpenClaw `canContinue = false`, Hermes `unknown`, pi-env `lost`),
  a send that may have started (OpenClaw delivery queue, Hermes ledger with a visible duplicate
  marker), and progress frames (OpenHands).

### 4.6 Durable versus transient frames

OpenHands is the only protocol that names the distinction on the wire: `durable` frames carry
a `seq` and are replayed; `transient`, `item_started`, `delta` and `item_aborted` are never
persisted or replayed, and deltas are keyed by the id the durable event will carry. DeepSeek
gets the same effect with durable events plus assistant frames that carry a dense `revision`.
OpenClaw separates `worker.transcript.commit` (sequenced, ledgered) from `worker.live-event`.
OpenCode's live bus is volatile by contract and its durable log is optional. Cline's log is a
replay aid behind canonical message files.

### 4.7 Back-pressure

Codex rejects *requests* with `-32001` when its 128-entry inbound queue is full but never drops
responses or notifications (they wait). OpenCodex makes the SSE bridge a pull stream so a slow
client stops the upstream read, and sheds body admission with 503 plus `Retry-After`. OpenCode
gives each subscriber a 4,096-entry queue and fails that subscriber on overflow. OpenClaw caps
the worker socket at 16 queued frames and 32 MiB. Oh My Pi sheds a lagging collab guest. Pi's
RPC awaits stdout back-pressure. DeepSeek's per-follower `Deque` is unbounded: a stalled client
grows host memory.

### 4.8 What the executing side can refuse

From weakest to strongest: nothing (Codex exec-server, pi-env, Zed's `terminal/create`, Warp's
`fs/read_text_file`, Cline's hub against a client's `toolPolicies`, goose against a roaming
peer's config methods); trusts a report (DeepSeek's SSH sandbox `enforcement`); re-decides but
yields to a forwarded approval (OpenClaw node: only local `security=deny` holds); refuses by its
own rules and intersects what is requested (OpenCodex executor: path, symlink, hard-link and
root-identity checks, CAS writes, no `exec` without a sandbox, capability intersection at the
handshake).

### 4.9 Listener authentication patterns

- **Refuse a wider bind without auth**: Codex app-server, OpenCode (refuses without a password
  at all), Hermes dashboard (`--insecure` ignored), OpenCodex data plane, goose (refuses without
  a secret unless a "dangerously" flag). DeepSeek goes further and refuses wildcard binds.
- **Where the credential travels**: HTTP upgrade header (Codex), first WebSocket message
  (OpenHands), WebSocket subprotocol (Hermes tickets, Cline hub token), cookie after a one-time
  code (OpenCode, DeepSeek, Pi p2), and the query string (goose `?token=`, OpenCode
  `?auth_token=`, DeepSeek launch URL, Cline dashboard, Oh My Pi browser relay).
- **Browser defences**: Codex rejects any `Origin`; DeepSeek checks Host, then
  `Sec-Fetch-Site`, then Origin; OpenCodex's pairing and agent routes refuse `Origin`;
  Cline *accepts* a loopback `Origin` as a credential, which is the anti-pattern.

### 4.10 ACP as agent and as client

As **agent**: goose (its whole UI runs over ACP), Oh My Pi (`omp acp`, with `bash` forced to
ask under a permission-capable client), Hermes (a thin viewer that never runs tools), Cline's
ACP mode (auto-approve off unless explicit). As **client**: Zed (serves fs and terminal; applies
path checks to fs, none to terminal, and does not apply its own tool permissions), Warp (fs
only, auto-allows everything except denylist and protected writes, prefers `AllowAlways`),
goose providers (inherits the whole environment; answers prompts itself in `auto`), DeepSeek's
`subagent-acp` (empty client capabilities, no MCP servers, rejects permission requests by
default), and the OpenHands Agent Server (not in the repository). The pattern across clients:
ACP's permission request is advisory, the agent describes its own action, and most clients
answer mechanically. Only DeepSeek's client starts from "grant nothing".

## 5. Documentation versus code

| Project | What Round 1 (documentation) said | What the code shows | Verdict | Permalink |
|---|---|---|---|---|
| Codex | Non-loopback WebSocket listeners are unauthenticated by default "during rollout" | app-server refuses to start without auth; exec-server `--listen` has no such refusal | refuted (app-server), corrected (exec-server) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L160-L175 |
| Codex | WebSocket auth before `initialize`; `Origin` gets 403 | at the HTTP upgrade | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L99-L130 |
| Codex | Bounded queues answer `-32001` | requests only; other messages block | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/mod.rs#L231-L262 |
| Codex | Daemon Unix socket is host-local | 0700 dir, 0600 socket, verified symlink | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L322-L352 |
| Codex | `environment_id` runs an MCP server on a remote executor | MCP stdio through the executor | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/executor_process_transport.rs#L1-L20 |
| Codex | Handoff moves chat and code; worktree from HEAD plus uncommitted changes | HEAD only; Handoff is closed Desktop | not in code | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/worktree/src/lib.rs#L62-L140 |
| OpenCode | Pairing link single-use, 5-minute expiry; 30-day cookie | as stated; token is password-derived, no per-token revocation | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/pairing.ts#L7-L34 |
| OpenCode | Web UI password-protected by default | refuses to start without a password; one shared secret | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/process.ts#L55-L58 |
| OpenCode | `--remote` is end-to-end encrypted via OpenTunnel | encryption in the external package | not in code | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/services/remote-tunnel.ts#L26-L47 |
| OpenCode | Durable per-session log at `/log`, exclusive sequence, `follow=true` | API exists; shipped service writes nothing to the event table | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L214-L214 |
| OpenCode | `/api/event` volatile; slow consumer overflows | queue 4,096, then the subscriber fails | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/event-feed.ts#L66-L68 |
| OpenCode | One shared service, no node fleet | no node or worker dial-in code | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/workspace/driver.ts#L88-L88 |
| OpenCode | SSH forward is a manual option | the desktop automates SSH bootstrap and tunnelling | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/gui-plugins/src/ssh/bootstrap.ts#L76-L100 |
| OpenClaw | Worker dials the Gateway's public worker route | WebSocket or Unix socket | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/node-worker-supervisor-commands.ts#L173-L204 |
| OpenClaw | Worker connection is outbound-only | the Gateway also SSHes into the box | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291 |
| OpenClaw | Closed method allow-list on the worker route | closed, feature-gated, default reject | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L54-L66 |
| OpenClaw | Each dispatch mints a credential, only the hash stored | domain-tagged hash bound to turn claim | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential.ts#L41-L60 |
| OpenClaw | Owner-epoch fencing rejects stale workers | per-request revalidation; bump limited to some transitions | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L218 |
| OpenClaw | Provider keys stay on the Gateway; inference by model ref | confirmed, plus a worker-local mode | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-model.ts#L91-L124 |
| OpenClaw | GitHub publication via a Gateway broker | the worker receives a GitHub token | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/launch-descriptor.ts#L183-L211 |
| OpenClaw | Writer claim checked in the same transaction as every transcript write | only for fenced writes; claim supersedes the holder | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-scope.ts#L79-L97 |
| OpenClaw | Every client and node signs the challenge nonce | when a device block is sent | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/role-policy.ts#L13-L16 |
| OpenClaw | SSH-verified auto-approval, default on | compares device id and key | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-pairing-ssh-verify.ts#L190-L240 |
| OpenClaw | Gateway allow-list is the gate for node commands | the node also re-evaluates `system.run`, but trusts a Gateway approval over its allow-list | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117 |
| OpenClaw | Control UI token in session storage per tab | device key and device tokens in `localStorage` | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/lib/nodes/index.ts#L112-L126 |
| OpenClaw | Accepted input stored before the Gateway acks | staged rows and an ingress table | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L897-L914 |
| Hermes | CLI attaches with single-use 30 s tickets via a control socket | ticket in a WebSocket subprotocol | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/runtime_bootstrap.py#L25-L75 |
| Hermes | Non-loopback dashboard fails closed without auth | and `--insecure` is ignored | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/web_server_boundary.py#L111-L119 |
| Hermes | Relay outbound-only, per-gateway secret and delivery key | gateway side confirmed; connector buffering not in repo | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/ws_transport.py#L442-L494 |
| Hermes | Crashed turn `unknown`, never replayed | and unattended ones auto-settle | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L453-L464 |
| Hermes | Kanban worker per OS process | a thin client; the agent runs in the gateway | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_db_dispatch.py#L2697-L2700 |
| Hermes | `hermes acp` serves ACP over stdio with persistent sessions | true, but it is a viewer of the gateway | corrected (process model) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/entry.py#L205-L215 |
| Oh My Pi | RPC NDJSON, v1 1 MiB, v2 64 MiB chunks | as stated; v1 frames shrunk, not rejected | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-frame.ts#L5-L9 |
| Oh My Pi | Whether `bash` runs over SSH is unknown | no remote exec back end; `ssh://` file I/O only | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/ssh/connection-manager.ts#L700-L709 |
| Oh My Pi | Auth broker loopback-only by default | a default bind; any `--bind` accepted | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-broker-cli.ts#L145-L157 |
| Oh My Pi | Auth gateway: own bearer, clients get no credentials | and scoped clients by SHA-256 digest | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/clients.ts#L1-L30 |
| Oh My Pi | Collab: relay, AES-256-GCM, key in fragment, no accounts | as stated | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/crypto.ts#L1-L10 |
| Pi | RPC command list including tree navigation | no `navigate_tree` command | corrected | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/modes/rpc/rpc-types.ts#L22-L74 |
| Pi | pi-env: Rust daemon, framed stdio, SHA-256-named, strict host keys, no forwarding | as stated | confirmed | [ssh.ts#L77-L123](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/ssh.ts#L77-L123) |
| Pi | pi-env keeps worker, storage and credentials local | only tools move | confirmed | [remote-env.ts#L871-L874](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/remote-env.ts#L871-L874) |
| Pi | `pi-web-ui` is probably gone | not a package, but `apps/p2` is a web UI | corrected | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/apps/p2/README.md#L1-L24 |
| DeepSeek | SSH remote: same policy, separate back end | helper hash is self-attestation | confirmed, with correction | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/helper.ts#L103-L119 |
| DeepSeek | Browser surface is "fetch/SSE" | HTTP POST plus a WebSocket mux | corrected | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/gateway/src/stream-protocol.ts#L6-L7 |
| DeepSeek | Loop and web server share one process | bundle composition settles it | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/web-app/cordis.patch.yml#L116-L136 |
| goose | `goose serve` is HTTP at `/acp` plus WebSocket, port 3284 | as stated | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L860-L905 |
| goose | Secret as `X-Secret-Key`; `token` query for browser WebSockets | as stated; constant-time on `/acp` | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/auth.rs#L15-L36 |
| goose | Startup prints a fingerprint for pinning, otherwise TOFU | TOFU is desktop-only | confirmed | [main.ts L360-L379](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/main.ts#L360-L379) |
| goose | `--dangerously-unauthenticated` is for local use only | nothing restricts it to loopback | corrected | [cli.rs L1821-L1834](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L1821-L1834) |
| goose | Roaming: revoke closes connections within seconds | 2 s poll | confirmed | [node.rs L324-L355](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/node.rs#L324-L355) |
| goose | Roaming: spawned OS processes survive revocation | shell children are killed by `ShellProcess::drop`; `setsid` and MCP children not covered (review answer) | not settled in full | see section 7 |
| Cline | "Spoke" workers run the loop, owned by the daemon | the hub runs the loop in-process; no spoke code | refuted | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-server-transport.ts#L334-L343 |
| Cline | The hub has no documented authentication | random token plus a loopback-Origin fallback | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L676-L722 |
| Cline | Roles creator, participant, observer per participant | a type; no authorisation reads it | confirmed as a type only | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/shared/src/hub.ts#L225-L230 |
| Cline | SSH: helper under `~/.cline/remote`, loopback hub, `ssh -L` to a random port | helper is the CLI binary | confirmed | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L325-L400 |
| Cline | Tunnel closes after about 45 s of network loss; provider key sent per session | 15 s x 3 keepalives; the separate startup timeout constant is 40 s; key sent, refresh token removed | confirmed, with correction | [remote-environments.ts L177-L182](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L177-L182) |
| OpenHands | Whether `X-Session-API-Key` is checked on WebSocket upgrades | Canvas sends the key as the first message, not on the upgrade; server enforcement not in repo | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/websocket-auth.ts#L1-L18 |
| OpenHands | Request path is browser to Canvas to selected backend | the browser calls the Agent Server directly | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/no-direct-agent-server-calls.test.ts#L5-L10 |
| OpenHands | Browser embeds the key only with explicit Docker opt-in | on a loopback bind the key is always embedded | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/bind-host.mjs#L50-L77 |
| OpenHands | Docker runtime mode binds all interfaces | as stated | confirmed | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L528-L543 |
| OpenHands | DigitalOcean sessions appear as backends, PAT auth | as stated, opt-in | confirmed | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-tunnel-bridge.mjs#L1-L18 |
| Zed | Docs do not say how tool permissions apply to external agents | the settings comment claims precedence; no enforcement found | corrected | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/settings_content/src/agent.rs#L403-L413 |
| Zed | ACP permission requests are advisory; clients apply their own checks | Zed applies path checks to fs only, none to terminal | confirmed as spec | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/v1.rs#L10549-L10590 |
| Warp | ACP support appears only in changelog wording | Warp implements an ACP client for cloud and headless runs | corrected | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/acp/bridge.rs#L1-L22 |
| Warp | Where the loop runs (unstated) | on Warp's server; the client executes tools | new | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api/impl.rs#L16-L26 |
| Warp | Hand-off blocked while a command runs or children are active | as stated | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/handoff/pipeline.rs#L440-L461 |
| Warp | Self-hosted workers | not in this repository | not in code | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/specs/REMOTE-1373/TECH.md#L87-L112 |
| OpenCodex | Remote Workspace: Ed25519-signed ECDH, AES-256-GCM | as stated | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/crypto.ts#L289-L312 |
| OpenCodex | One-time pairing code; hub stores a hash of the bearer; 429 after ten failures | as stated | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L377-L412 |
| OpenCodex | Read-only default; write needs a command-sandbox probe | the probe gates `exec`; writes need none | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L372-L384 |
| OpenCodex | A disconnected executor never falls back to the hub filesystem | instruction plus absence of a local tool path | confirmed (not a hard guarantee) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-tools.ts#L212-L225 |
| OpenCodex | Remote Workspace may be the `ocx-child` relay | `ocx-child` is Remote Link | refuted | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/native/ocx-child/src/lib.rs#L1-L20 |
| OpenCodex | WebSocket relay 4 MiB per frame | 4 MiB is the SSE frame cap; live-sideband WebSocket cap 50 MiB | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/sse-frame-buffer.ts#L4-L4 |
| OpenCodex | Unauthenticated loopback companion listener exposes hub credentials to local users | as stated | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index.ts#L790-L802 |

## 6. For rizzy-harness

Our position, for reference. On `main`: the node dials the master over TLS with a pinned
self-signed master certificate; the node's Ed25519 key signs a nonce bound to the certificate
fingerprint; 8 MiB frames; 10 s handshake steps; 32 concurrent pre-auth handshakes
(`research/round1/own-rizzy-harness.md`, "Link"). The owning node is the only writer; the
master mirrors its events and rejects events for sessions another node owns; a link reconnect
replays with `Resume{after}`. Streaming deltas are stored as events forever (AUDIT D5). On
`main` the master reads node frames into an unbounded channel (AUDIT L1); pull request 4
bounds it (8 inbound parsed messages, 64 outbound commands). The design note
(`docs/nodes-credentials-environments.md`) adds pair codes (§4.4), per-turn `session.grant`
(§4.3), and fork by `session.seed` batches written as `history_imported` (§4.9), and records
that "a second connection replaces the first (`nodes.rs:208-212`)" (§6).

The surveyed code says one thing clearly: **only OpenClaw fences a remote executor's writes**,
and every system that lets the controlling side send policy ends up letting it widen the
executing side. Our invariants put us on the right side of both; what we lack is the fence
itself and a resume protocol that keeps deltas out of the log.

Feature freeze note: items 1 to 4 below harden the node link and the approval path (security
code under `CLAUDE.md`); items 8 and later are features and wait for Gate 3. Check each against
`FIX_PLAN.md` before scheduling; this file does not know the plan's task numbers.

### 6.1 (Rank 1) Fence the node link and every session write

**What we have is "latest connection wins"**, the same as OpenCodex Remote Workspace
(`workspace-hub.ts` `attachConnection`, finding 31) and weaker than OpenClaw. It replaces a
dead socket, but it does not refuse frames that were already read from the old socket and
are still queued (pull request 4's 8-message inbound channel is exactly such a queue), and it
says nothing about a session that was archived, deleted or forked while its node was offline.
Codex's "one attachment plus retention" leaves the same gap (review: "no epoch or fencing term
found anywhere in exec-server").

**Design. Two counters, both in `master.db`, both checked inside the transaction that writes.**

1. `nodes.link_gen` (u64). Incremented in the transaction that accepts a node connection,
   after the signature check. Sent to the node in `Accepted { node_id, link_gen }`. The master's
   in-memory registry holds the current value per node.
   - *Checked where:* in the master's reader pump, on **every** inbound frame, before
     dispatch: the frame's connection must still be the registry's current `link_gen` for that
     node; otherwise drop the frame and close that socket with a `superseded` code. This is
     OpenClaw's per-request revalidation (`admission.ts` L188-L218, `worker-connection.ts`
     L326-L351) reduced to one integer compare.
   - *Bumped when:* each accepted connection; node revocation; node re-pairing.
2. `sessions.owner_epoch` (u64), drawn from one global counter that never goes backwards and
   never reuses a value (OpenClaw: `max(...) + 1`, `store-mutations.ts` L30-L50; a single
   `meta.epoch_counter` row does the same with one `UPDATE ... RETURNING`).
   - *Set when:* the session is created on its node.
   - *Bumped when:* the session is archived, deleted, its node is revoked or removed from the
     workspace, or the source of a "continue elsewhere" fork is closed (6.2). **Not** bumped by a
     reconnect of the same node: the node is still the owner and must be able to resume.
   - *Carried:* in `session.create`/`session.open` to the node, and in every node-to-master
     event frame as `(session_id, owner_epoch, seq, hash)`.
   - *Checked where (master):* in the same SQLite transaction as the mirror insert, as one
     predicate: `owner_node = conn.node_id AND owner_epoch = frame.epoch AND state = 'active'
     AND seq = last_seq + 1`. For `seq <= last_seq`, compare the stored event hash: equal means
     a duplicate from a resend, acknowledge it; different means divergence (a node restored from
     an old disk image, a cloned VM with the same key): refuse with `diverged` and mark the
     session for attention. This is OpenCode's `replay` rule ("Replay diverged", "Sequence
     mismatch", owner refused; `bus.ts` L329-L378) applied to a real link.
   - *Checked where (node):* the session actor stores the epoch it was given and refuses any
     master command for that session that carries a different epoch. On a `fenced` reply to an
     append, the actor stops, kills the running tool's process group, marks the session
     read-only locally, and does **not** retry (OpenClaw's worker treats `owner-epoch-mismatch`
     as terminal: `worker-connection-contract.ts` L22-L26).
3. Grants (`session.grant`, design note §4.3) are bound to `(session_id, owner_epoch,
   turn req_id)`; the node drops a grant on `grant.drop`, at expiry, on a `fenced` reply, or
   when the epoch it holds changes. OpenClaw binds its credential hash to the durable turn
   claim for the same reason (`credential.ts` L9-L60).

**How this breaks, and what the counters do about it.** A stolen node key: both copies get
`link_gen` bumps, each kills the other's socket, frames from the loser are dropped at dispatch;
the design note's "revoke after 3 flaps in 10 minutes" (§6) then bumps every session's epoch. A
node restored from a backup: its replayed events diverge by hash and are refused. A session
archived while its node was offline: the node's resume replays events stamped with the old
epoch and is told `fenced`. A master restored from an older backup: it may hold a smaller
`last_seq` than the node; the node's resend fills the gap (hashes match), which is the desired
outcome.

**Tests that fail on `main` and pass after.** (a) Queue two event frames on connection A,
connect B for the same node before A's frames are dispatched: A's frames are refused and not
mirrored. (b) Archive a session while its node is disconnected; reconnect; the node's resume is
refused with `fenced` and the node marks the session read-only. (c) Resend an already-mirrored
event with the same hash: acknowledged, not duplicated. (d) Resend `seq` N with a different
payload: refused with `diverged`. (e) A grant is not usable after the session's epoch changes.

### 6.2 (Rank 1, same change) Session fork without live migration

The design note forbids live migration and forks by re-seeding history (§4.9). The fence above
makes that safe; the protocol:

1. The master, in one transaction: chooses `through_seq` as the highest **contiguous mirrored**
   seq of the source session S (never an unmirrored tail); records `session_forks(S', S,
   through_seq, hash_at_through_seq)`; creates S' with a fresh `owner_epoch` and its target node.
   For "continue elsewhere" (source closed), the same transaction sets `S.state = archived` and
   bumps `S.owner_epoch`, so any later append from S's node is refused.
2. If S's node is online and S has a running turn, refuse "continue elsewhere" until the turn
   ends, or close it: pending approvals in S become synthetic closers (goose answers a pending
   approval whose lease is gone with "Tool approval expired", `ops_toolcalling.rs` L1010-L1031;
   DeepSeek's closers say the outcome is unknown, `repair.ts` L53-L98). Events S's node wrote
   after `through_seq` and never delivered are refused by the fence; the UI must say how many
   (the master knows the node's reported head from its last resume).
3. Seeding: `session.seed { session_id: S', batch_no, events, last }`. The target node writes
   each batch as `history_imported` events with its own seq, keyed by `batch_no` so a resend is a
   no-op, and acknowledges only after the batch is durable (Hermes acknowledges a relay frame only
   after durable admission, `ws_transport.py` L971-L1004). S' refuses prompts until the last batch
   is acknowledged. The first event of S' records `forked_from { parent: S, through_seq,
   parent_hash }`.
4. No files cross. The design note already says files come back from the forge. Codex offers
   nothing to copy here except the "owner id inside `.git`" marker (`worktree/src/metadata.rs`
   L1-L90), which is worth copying so two sessions never share one worktree.

### 6.3 (Rank 2) Nothing the master sends may widen the node

Every design surveyed that lets the controlling side send policy lets it widen the executing
side. Do **not** copy these properties:

| Project | Property | Why it widens the executing side |
|---|---|---|
| Codex exec-server | sandbox intent sent per request; no `sandbox` means unsandboxed; no executor ceiling | the caller's policy is the executor's policy |
| OpenClaw node | `approved === true` from the Gateway satisfies the node's ask and allow-list miss; only `security=deny` holds | a master-side approval overrides the node's own list |
| OpenClaw worker | a GitHub token in the launch descriptor, written where the agent's shell reads it | the controller hands the agent a credential |
| OpenClaw provisioning | the Gateway SSHes into the worker box | the controller dials the executor, so the executor must listen |
| Cline hub | `session.create` takes `toolPolicies`, `autoApprove`, `cwd`, `systemPrompt` from the client; approvals answerable by anyone | the client sets the executing side's policy |
| Cline hub | loopback `Origin` accepted as a credential | any local page or process becomes a controller |
| goose roaming and serve | a peer gets the full ACP surface, including `config/extensions/add` (a stored stdio command) and an unforced working-dir update | the controller adds code to the host and moves its root |
| OpenHands | start request carries `tool_module_qualnames` (imported by the server) and container limits (forwarded uncapped); a server-level bash endpoint | the controller chooses code and resources |
| OpenHands, OpenCode | the stack's own key reachable from the agent's shell | the executed agent can act as the controller and approve itself |
| DeepSeek external sub-agents | the child's permission mode is fixed per provider, independent of the parent | delegation escapes the parent's limits |
| Warp | unattended CLI children launched with their own permission systems off; ACP policy allows every non-denylisted request and prefers `AllowAlways` | the controller turns off the executor's guards |
| Zed | its `tool_permissions` are not applied to external agents; `terminal/create` runs unconfined | the client's own policy does not bind the tools it serves |
| Hermes SSH | sync-back writes remote files into the host's skills tree | the executor writes into the controller's instructions |
| pi-env | the daemon trusts every request on stdin | no executor-side policy at all |

The node must treat every `MasterToNode` field as a request that `node.toml` can only narrow:
`approval.respond` (an allow from the master must still pass `node.toml`'s `denied_tools` and
`max_approval_mode`), `session.create` (cwd inside `workspace_roots`, agent spec within what the
node allows), grants (refused when `model_keys` is `local` or `none`, as the design note says),
container and resource requests (caps from `[containers]`, as OpenHands should have done), and
anything that names code to load (refuse outright). Pull request 6's server-side enforcement of
where an agent may run is master-side narrowing; good, but the node must check again.

Copy the opposite cases: OpenCodex's executor intersects requested capabilities with what it
allows at the per-session handshake and refuses a presence that claims more than was enrolled
(`crypto.ts` L214-L286, `workspace-hub.ts` L427-L463); OpenClaw's node refuses a delayed approval
when its own policy snapshot changed and refuses PATH overrides (`invoke-system-run.ts` L349-L357,
L265-L290). Note that OpenCodex's reviewer suggested the hub should decide the capability set at
enrolment; for us that is backwards. Our node declares its ceiling in `node.toml`, the master
narrows.

**Tests.** A master `approval.respond` that allows a tool `node.toml` denies is refused and
logged by the node. A `session.create` whose cwd is outside `workspace_roots` is refused. A
grant sent to a node with `model_keys = "none"` gets an ack error. A container request above the
node's cap is clamped or refused, never applied.

### 6.4 (Rank 3) Resume: durable events by cursor, deltas transient

**Durable stream, node to master.**

- Every event frame carries `(session_id, owner_epoch, seq, hash)`. The node's sender is a
  **cursor over the node's own log**, not an in-memory queue: it keeps `next_to_send` and
  `acked` per session and reads from the log. With at most W unacknowledged frames per link
  (a count and a byte cap), a slow master makes the sender wait, while the agent loop keeps
  appending locally because the node's log is the truth. This removes any need for an unbounded
  buffer on either side (AUDIT L1) and makes pull request 4's bounded channel safe to fill.
- The master acknowledges `(session_id, seq)` only after its mirror transaction commits,
  including the fence predicate (OpenClaw updates its ACK cursor only after `assertCurrent`,
  `worker-turn-rpc.ts` L279-L320; Hermes acks after durable admission).
- On reconnect the master sends `resume { session_id, after_seq }` with its mirrored head
  (we have `Resume{after}`); the node rewinds the cursor; duplicates are absorbed by the hash
  rule in 6.1.

**Transient stream.** `delta { session_id, owner_epoch, item_id, rev, text }` and tool
progress: no `seq`, not stored, coalesced latest-wins per `item_id`, dropped first under
back-pressure, never replayed. The durable `assistant_message` event carries the same
`item_id` and supersedes the deltas. This is OpenHands' envelope (`session-frames.ts` L1-L79:
`durable` with `seq`, `transient` and `delta` without) and DeepSeek's dense `revision` on
assistant frames. It also fixes AUDIT D5 (every delta stored forever): stop writing deltas
into the event log. Our own inventory says deltas are stored today
(`crates/core/src/event.rs:59-80`); the brief says the design separates them. Settle that
contradiction in code before building resume on top of it.

**Master to browser.** `subscribe { session_id, after_seq }`; the master registers the listener
first, reads the mirror up to a captured watermark, sends `sync { through_seq }`, then live
events (OpenCode's log contract, `bus.ts` L829-L878; DeepSeek's "listener armed before the
snapshot"). Each socket gets a bounded queue; on overflow close it with a "lagging" code and let
the client reconnect with its cursor (OpenCode fails the subscriber at 4,096; DeepSeek's
unbounded `Deque` is the bug to avoid). Deltas reach the browser only live; on reconnect the
client discards open streaming slots and waits for the durable event (OpenHands
`conversation-websocket-context.tsx` L1305-L1312). Because our seq is contiguous per session
with one writer, the cursor is "highest contiguous seq"; we never need OpenHands' 1,024-hole
heuristic.

**Master to node commands.** Every `session.prompt` carries a client-generated `req_id`
stored in the node's `user_message` event; the node acknowledges a repeat as already accepted
(DeepSeek `commands.ts` L389-L413). Every `approval.respond` names the `approval_id` and the
session epoch; the node accepts it only if an `approval_asked` event exists without a decision,
and rejects a second, conflicting decision (goose `tool_confirmation.rs` L148-L186). Pending
approvals are derived from the log, so a reconnecting UI rebuilds them (DeepSeek keeps them in a
process `Map` and loses them on restart).

**Node restart mid-turn.** Append closers with fixed wording (DeepSeek: "outcome is unknown ...
do not retry blindly", `repair.ts` L31-L41); never re-run a non-idempotent tool on resume (Pi
durable commits `replay: "unsafe"` before the effect, `tool.ts` L195-L227); never auto-continue
a turn whose node was lost (OpenClaw `canContinue = false`, `worker-node-loss.ts` L3-L15);
settle unattended routine turns as interrupted at boot so a routine queue does not stall
(Hermes `session_unattended_unknown.py` L41-L66), while a person's turn waits for the person.

**Tests.** Kill the link mid-turn, reconnect: the mirror has every durable event exactly once
and no deltas. Stall the master's mirror writes: the node keeps appending, memory stays bounded,
and the sender catches up after. A browser that stops reading is disconnected, reconnects with
its cursor, and sees no gap and no duplicates. A prompt resent with the same `req_id` after a
reconnect produces one user message.

### 6.5 (Rank 4) Credentials on the node and the link key

- The node's link key and any grant must be unreachable from the agent's process tree.
  OpenHands puts its stack key in the agent's environment on purpose
  (`dev-with-automation.mjs` L975-L985); OpenCode leaves its password in shells outside stdio mode
  (`server-process.ts` L85-L97); in both, a prompt-injected agent can approve itself. Cline
  captures its spawn secret into memory and deletes it from the environment before spawning
  anything (`host-bridge-auth.ts` L4-L47); pull request 4's allow-listed empty child environment
  is the stronger form. Test: the key file and `$CREDENTIALS_DIRECTORY` are not readable from a
  sandboxed shell, and no grant value appears in a child's environment.
- Any node-local socket (a future control socket, an MCP bridge) must authenticate its peer or
  be hidden from the sandbox. Codex hides its unauthenticated daemon directory with bubblewrap and
  refuses legacy Landlock for that reason (`linux_run_main.rs` L386-L393); that Landlock cannot
  stop `connect()` to an existing Unix socket is the author's inference, and it matches pull
  request 4's recorded residual (pathname Unix sockets before Linux 7.1). Test: a sandboxed
  command cannot connect to any socket the node opens.
- Grants: OpenClaw keeps keys on the controller and brokers inference by model reference,
  refusing an unapproved model or a profile suffix (`inference-runtime.ts` L122-L153), and it
  also ships a worker-local mode. The design note's `ModelAccess::Proxy` slot is the same idea;
  keep it reserved, and bind each `Direct` grant to session, epoch and turn as in 6.1.
- Store only hashes of any bearer the master issues. OpenClaw hashes worker credentials but keeps
  device tokens in plaintext (`pairing-token.ts` L4-L18); OpenCodex hashes device bearers
  (`workspace-hub.ts` L377-L425). Our node identity is a key pair, so this applies to pair codes
  (the design note already compares hashes) and to any future per-client API tokens.

### 6.6 (Rank 5) Pairing

The design note's pair code (§4.4) is in line with the best surveyed (OpenCodex: single use,
10 minutes, hashed, rate-limited per source; OpenCode: atomic consume). Copy three details:
**consume the code before validating the rest of the request** (OpenCodex deletes the grant
first, so a rejected request still burns it; `workspace-hub.ts` L377-L425); refuse any request
carrying an `Origin` header on the pairing path (OpenCodex `serve-options.ts` L484-L513); and
answer every failure with one opaque reason while charging only failures to the limiter
(OpenClaw `worker-connection.ts` L218-L262). Never let a pairing flow print a secret on a remote
stdout and read it back (Cline's SSH remote does, `remote-environments.ts` L1436-L1490), and never
put a secret in a URL or a flag value (Codex accepts a token only from a named environment
variable and only over `wss://` or loopback, `cli/src/main.rs` L2649-L2676).

### 6.7 (Rank 6) Back-pressure without deadlock

Pull request 4 bounds the link (8 inbound, 64 outbound). Two rules from Codex to check against it:
reject *new requests* with a retryable error when a queue is full, but never drop *responses or
events* (Codex `transport/mod.rs` L231-L262); and make sure a full queue of tool output cannot
block an approval reply or a cancel (our per-session lanes answer approvals inline; add a test
that a node streaming at the cap still processes a cancel within the grace period). With the
cursor sender of 6.4, the node-to-master direction never needs to drop anything.

### 6.8 (Rank 7) Listener authentication on the master

Our master already re-checks login on long-lived sockets (60 s recheck, close on logout); Codex
does not re-check expiry after the upgrade, so we are ahead there. Copy DeepSeek's fence order on
every `/api` request and upgrade: Host (loopback, bind address or `--allowed-host`), then
`Sec-Fetch-Site: cross-site` refused, then `Origin` must equal the Host authority, then the
credential (`api-request-trust.ts` L121-L151). Consider DeepSeek's refusal of wildcard binds
(it names one interface instead), which is stricter than our "explicit, logged" rule. The node
opens no listener; keep it that way (OpenClaw's provisioning SSH and Cline's remote hub both
require the executor to listen).

### 6.9 (Rank 8) Version gate at admission

OpenClaw refuses a worker whose bundle hash or version differs from the expected build
(`admission.ts` L79-L186); OpenHands refuses an Agent Server older than `minimumAgentServer`
(`agent-server-compatibility.ts` L253-L318). Our `PROTOCOL_VERSION = 1` should become a minimum
plus a feature list in `node.hello`, so the master refuses an incompatible node before it
receives sessions and the UI can show which optional features a node supports.

### 6.10 (After Gate 3) Protocol and client surfaces

- Generate the web UI's TypeScript types from the Rust protocol types and fail CI on drift
  (Codex `export.rs` L125-L150; OpenCode generates clients from one API definition).
- Put the ordering key of each master API method in the method table, not the handler (Codex
  `common.rs` L129-L139).
- If we ever offer ACP, copy Hermes' shape: a thin adapter that attaches to the node-owned
  session and never runs tools (`acp_adapter/entry.py` L205-L215). If we ever host another
  vendor's agent as an ACP client, start from DeepSeek's: empty client capabilities, no MCP
  servers, decline every approval by default, scrubbed environment, and derive the child's mode
  from the node policy rather than fixing it per provider.
- A "human approval" class that headless clients structurally cannot answer (Codex
  `outgoing_message.rs` L414-L430) is a cheap primitive for routines.

## 7. Not settled

- **OpenClaw transcript-commit ledger and protocol frames** were not checked in review; the
  fencing and credential findings that 6.1 relies on were.
- **Codex exec-server Noise channel internals** (stream numbers, reordering window) and the
  **remote-control** transport were read from definitions only; the relay, registry and pairing
  expiry are hosted services not in the repository.
- **Codex: what happens when no connection qualifies for a human-only approval** (the remainder
  of `outgoing_message.rs` after L430) was not traced.
- **Hermes relay** (ack after admission, 4401 semantics) and **Kanban** were not checked; the
  connector's buffering is outside the repository.
- **OpenCodex Remote Workspace RPC v2** (prepare/grant) and the handshake's security reading
  were not checked (crypto only spot-checked); the Home side of Remote Link was not traced;
  the macOS and Windows helper (`native/remote-workspace-helper`) was not read.
- **DeepSeek `follow` and reconnect, prompt idempotency and LiveShare** were not checked;
  LiveShare's relay is not in the repository.
- **goose**: whether `setsid` children and MCP server children survive a roaming revocation
  (shell children are killed). The iroh relay's visibility into ACP content was not verified.
- **OpenHands**: whether the Agent Server closes a socket that never sends the auth frame,
  how it orders and skips events, its fork and persistence: all in `software-agent-sdk`, not
  cloned.
- **Warp**: the server-side loop, inbox durability and ordering, and the self-hosted worker
  (`warpdotdev/oz-agent-worker`) were not available.
- **Zed**: registry download verification when `sha256` is absent (`download_server_binary` not
  read); a reader of `tool_permissions` in a crate outside the three searched is not ruled out;
  the ACP schema is in `agentclientprotocol/agent-client-protocol`, not cloned.
- **Cline**: the Origin-only exploit (create an auto-approved session in any directory) is code
  reading, not exercised; where a provider key is dropped from the remote hub's memory after a
  session was not found.
- **Oh My Pi**: the real collab relay and the approval wiring of guest prompts.
- **Pi**: how `apps/p2` authenticates its workers to its server (`owner-link.ts`) was not read.
- **Our own code**: whether `nodes.rs:208-212` drains or drops frames already queued from a
  replaced connection, and whether deltas are still written to the log on the current branches,
  must be checked in our tree before 6.1 and 6.4 are written; this synthesis cites them only
  from the design note and the Round 1 inventory.
