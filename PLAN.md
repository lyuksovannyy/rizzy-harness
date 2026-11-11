# harness — plan

Working name `harness`. The name lives in exactly two kinds of places:
`harness_core::APP_NAME` (used for the binary banner, data dir, cookie name,
env-var prefix) and the Cargo package names (`harness-*`). A rename is a
search-and-replace over `Cargo.toml` files plus that one constant.

Status notes are appended at the bottom (`## Status`).

---

## 0. Product direction (revised 2026-11-02)

Phase 1 was framed as a coding-agent harness. The product is broader: a **self-hosted,
ChatGPT-style personal assistant** that is also the browser's new-tab page, with plain chat on
the central server and agentic work delegated to worker nodes. Sections 1–4 (architecture,
event log, node protocol, threat model) still hold. The build order in §8 was
rewritten after the layout mockup (foundations, UI shell and bar first, onboarding after).

### Principles

* **The harness process stays light; the server around it may not.** The central server is the
  user's hub and can host local models and helper services next to the harness. Soft budget for
  the harness itself, measured every phase: idle < 50 MB RSS, and no core feature *requires* a
  local ML model. Breaking it needs a written reason here. The sidebar areas (2026-11-08)
  keep it: stored files stream in 64 KiB pieces, nothing is decoded in the process (thumbnails
  are made by the browser), and the routine scheduler is one sleeping task.
* **Everything heavy is a service = a URL.** Memory server, embedding model, reranker, later
  speech. Each is *managed* (the master installs, starts and health-checks it), *LAN* (another
  own machine) or *cloud API*. Locality only changes the URL; the code path is identical. Local
  models run in existing runtimes with OpenAI-compatible APIs (e.g. Ollama, llama.cpp server).
* **Use maintained dependencies instead of hand-rolling.** Exact pins, committed lockfiles,
  `cargo-deny` in CI (licences, advisories, duplicates), APIs checked on docs.rs before use.
  Phase 1's hand-rolled SSE parser, markdown renderer and provider client get re-evaluated
  against maintained libraries (the renderer must sanitize).
* **Multiple accounts, admin-gated; workspaces group them.** The first account is an
  admin created with a one-time claim code. Only admins create accounts (temporary password,
  changed at first login). Everyone sees only their own conversations, in every workspace.
  Each account has a Personal workspace and may join team workspaces with a fixed role
  (owner, admin, member, guest); settings come in four tiers: account, workspace (what applies
  inside it), workspace admin (members, defaults) and server. Agentic sessions (a shell on a
  worker node) need a per-user grant, and the workspace role must allow them too. Localhost by
  default; remote only with an explicit flag. Shared chats and shared memory are not built.
* **No compatibility code before the first release.** Nothing has shipped, so the schema is
  created complete, with no migrations, aliases or renames; a dev data dir is simply wiped
  when the schema changes. Migrations start with the first tagged release.

### Omnibox (the new-tab bar)

* One input with a visible **mode switch** (Ask / Search / Open chat / …). **Tab** cycles modes,
  **Enter** runs the selected one. Focused on load; first paint must be near-instant.
* A tiny **in-browser classifier** (linear model over character n-grams + cheap features: word
  count, URL-like, question words, `?`, title match) preselects the most probable mode. Rules
  give the cold start. Each Tab override is a label, stored per user; weights retrain on the
  master and sync down. Top-1 accuracy is logged from day one, and the rules stay if the model
  is not clearly better. Behind a `ModeClassifier` interface.
* Search runs a user-chosen search-engine URL template (picked in onboarding).
* **Similar earlier chats** are shown under the bar as suggestions, never a redirect. v1 uses
  SQLite FTS5 (BM25) over titles and user messages; vectors optional later. Time-sensitive asks
  ("weather today") are excluded.
* Hosted decision models were evaluated and **not** used for the bar: Jev (TypeSafe AI, closed
  cloud API; would send every keystroke to a third party) and Laya (Apache-2.0, ModernBERT-large
  421M; too heavy per keystroke, near-random until fine-tuned).

### Conversations

* One conversation model with two **profiles**; messages carry an `author` (user or agent id)
  so group chats can be added later without a schema break.
* **Chat** runs on the master: documents, web search, research. Low token use by default: short
  system prompt, few tools with schemas loaded on demand, cheaper default model, rolling history
  summary, a token meter in the UI.
* **Agentic** runs on a node (the embedded node counts), picked in the bar's node picker.
* **No session modes.** DSH-style Standard/PTC/Minimal selectors were considered and dropped;
  everything a mode would configure lives on the **agent** (below).
* **Group chats** (several AIs answering freely): deferred (turn-taking, AI↔AI loops, cost).
  **Parallel answers** (same prompt to N models, pick or merge) replace them: the event log
  stores answer variants now (`variant_group`, `variant_index`, `variant_selected`); the
  side-by-side UI comes later.
* **Incognito** (chat only in v1): events live in a TTL'd ephemeral table, purged on close or
  after N hours, excluded from search, similar-chat suggestions and memory. The UI states that
  the provider still receives the text.

### Agents

* The single user-facing primitive for "who works and how": name, avatar, system prompt,
  allowed tools, attached skills, default model + effort, token budget (lean / standard).
  Stored on the master as markdown with YAML frontmatter.
* Built-ins (editable, clonable): **General** (default), **Coder**, **Writer**, **Scout**
  (read-only research), **Ultracode** (orchestrator: has the `delegate` tool to spawn other
  agents, with user-visible caps on subagent count and tokens/cost per turn). Users create
  agents and choose the default.
* The same agents serve chat and agentic sessions; tools the runtime lacks are unavailable.
  Node policy still wins: an agent's tool list is a request, not a permission.
* Switching agent mid-session is allowed (`agent_changed` event; history carries over; the
  prompt cache resets). One agent answers per turn; an `@mention` routes that turn to the named
  agent (mention popup lists agents; the selector shows the override). Several agents in one
  turn only through an orchestrator.
* Event fields: `author` (agent id) on assistant/tool events; agent snapshot (name + prompt
  hash) in `turn_started` so old turns stay explainable after an agent is edited.

### Home screen and bar (from the mockup)

* Sidebar v1: New chat, chat search, Recents, user menu. Projects and Library later; Images,
  Scheduled, Plugins and Explore cut.
* Sidebar since 2026-11-08: New chat, Agents, **Routines** (was "Scheduled"), **Media** (was
  "Images") and **Artifacts** (was "Library"), each shown per viewer and workspace; Plugins
  stay cut, as a disabled **Connections** entry ("Later"); Projects and Recent sections; the
  account menu in the footer, which also holds the workspaces; search in a panel (Ctrl K)
  instead of the inline box.
* Home: localized, time-of-day greeting with the user's name (strings in i18n catalogs from day one).
* Bar: `+` attach · input · agent selector · model + effort (effort only when the model
  supports it, from a model capability registry) · mic (later) · mode switch
  **Search / Ask / Agentic** with an **Incognito** toggle · node picker when Agentic.
  Agentic is disabled with a "set up a worker node" link when no node is online.
* Under the bar: earlier chats that may already answer the question.

### Memory

* Not built from scratch: an **existing, separate memory server** behind a `MemoryStore`
  trait, local preferred, LAN or cloud allowed. OpenViking (volcengine, AGPLv3, Python HTTP
  service) is the first candidate. AGPL is acceptable only while we run it unmodified as a
  separate process over HTTP; no code is copied. It is compared with two other maintained memory
  servers (licence, API stability, ops weight, can skills be disabled) before choosing.
* Quality presets: **Fast** (small embedding model, no reranker), **Balanced** (medium +
  reranker), **Best** (large + reranker). Concrete models are picked by benchmark on reference
  hardware; onboarding shows RAM and latency per preset.
* No memory server configured → FTS5 recall only, so a bare install still works.
* Memory is visible and editable on a Memory page, each fact linked to its source chat.

### Onboarding

1. Claim the install (first-run code) and create the owner account.
2. **Model provider** (the only hard requirement): Anthropic API (API key only; never
   subscription OAuth) or any OpenAI-compatible endpoint. One test call; show the model list.
   Keys typed in the UI go to a 0600 secrets file (later the OS keyring), are never sent back to
   the browser, logged, or put in the event log. Env-var references stay for headless installs.
3. Name, timezone (auto-filled), optional country/city, search engine.
4. Memory preset (with the measured numbers). Layout and accent live in Settings, not here.

Country/city exist so the AI knows where to look for local information (transit, news, shops).
Location learned later from chats is *proposed* as memory and saved only after the user confirms.

### Agent backends

v1: API providers only. Driving on-device harnesses (Claude Code, Codex, OpenCode) as
subprocesses (e.g. over ACP) is deferred; the `AgentBackend` trait already allows it.

A judge model may later power an agentic **auto-approval** mode. Rules fixed now: it runs on
the node after `node.toml` and can only turn "ask" into "allow" within node policy; it fails
closed (low confidence, timeout or error → ask); tool arguments are scrubbed before any cloud
judge sees them; every verdict is an event (`approval_resolved{by: judge, score}`, an additive
`ResolvedBy` variant); it is tested against prompt injection before release.

### References checked

* OpenBot (CopilotKit, MIT): named "coworkers" with role descriptions. opencode-workspace:
  explore / researcher / coder / scribe / reviewer specialists. Ideas only, no code.
* "Buzz" (agent roles): not found; unverified.
* **Amazon "Infinity"** (researched 2026-11-03; `github.com/hydro-project/infinity`,
  Apache-2.0, Rust, AWS + UC Berkeley Hydro project, announced 2026-10-23 as an early
  research release, no tagged releases). Agent runtime for extreme concurrency plus
  "Infinity Code". Underneath: agents run as short *slices* (load history → call model →
  dispatch tool → stop; never block); every input (user text, tool result, child report,
  timer) is a message on the thread's queue; a ConversationStore + a StateStore of
  processed message ids give at-least-once delivery with dedup; one ordered worker per
  thread (SQS FIFO + Lambda on AWS); compaction at ~75% of context; tools speak RAP
  (ack now, POST the result to a callback later; MCP via a bridge); sleep and spawn-child
  are built-in tools; Infinity Code isolates each child in its own git worktree/jj
  workspace and reaches other machines over SSH.
  **Decisions for us:**
  - Adopt the *ideas*, not the runtime: we already have per-session single-writer
    ordering and an event log with replay; the gap Infinity highlights is that our turns
    hold a task while waiting (approvals, long tools). For Phase 8 (orchestration) and
    Phase 6 (research), model long waits as *messages back into the session* (the
    "front door" pattern) so a waiting turn costs no running task and survives restarts.
  - Sub-agents (Ultracode `delegate`) report back as synthetic tool results and each
    gets its own worktree (matches our Phase 3-later worktree plan).
  - RAP-style async tools (callback when done) are the shape for deep research and other
    long jobs; evaluate `rap-protocol` crates when Phase 6 starts (Apache-2.0, so code
    reuse is allowed), but do not depend on a pre-release runtime before then.
  - Nothing in Infinity covers memory or the mode classifier; Phases 4 and 7 are unaffected.
  - Unverified: the AWS post itself (blocked here) and the 75k-agents figure (idle agents,
    simulated model).

---

## 1. Architecture

```
                 browser (React SPA, embedded in binary)
                        │  HTTPS/WS (cookie auth, same-origin)
                        ▼
 ┌──────────────────────────── master ────────────────────────────┐
 │ axum: /api/* (REST commands), /ws (event fan-out), static SPA  │
 │ mirror store (master.db): sessions + replicated events         │
 │ node registry, router (session_id -> node_id)                  │
 │        │ in-process channels           │ WSS (Phase 2)          │
 │        ▼                               │                        │
 │  embedded node runtime ◄── same code ──┼──► remote node runtime │
 └────────────────────────────────────────┘    (dials out)         │
                                                                    ▼
 node runtime: session actors · agent backend (native loop) · tools
               node store (node.db): authoritative event log
               node policy (node.toml): authoritative permissions
```

* **The agent loop runs on the node that owns the session.** The master never
  executes tools. It mirrors the node's log and forwards commands.
* **Single writer.** A session's events are appended only by its owning node's
  session actor. `seq` is assigned by that actor, contiguous from 1.
* **Event sourcing.** Everything the UI shows is a fold over the event log.
  There is no other session state that matters for display.
* **One code path for local and remote nodes.** A `NodeLink` is a pair of
  channels carrying `protocol::NodeToMaster` / `protocol::MasterToNode`.
  The embedded node gets the channels directly; a remote node (Phase 2) gets
  them bridged over a WebSocket. The master-side handling is identical.

### Workspace

| crate | role | phase |
|---|---|---|
| `harness-core` | `APP_NAME`, ids, event schema (typed + raw), session fold, approval policy types, `Secret` | 1 |
| `harness-store` | SQLite event log (node + mirror modes), cursors, session index | 1 |
| `harness-protocol` | versioned master⇄node messages | 1 |
| `harness-tools` | bash (process-group kill), read, edit/write, grep, glob | 1 |
| `harness-agent-native` | native agent loop, `Provider` trait, OpenAI-compatible streaming provider | 1 |
| `harness-node` | node runtime, session actors, backend trait, policy enforcement | 1 |
| `harness-master` | axum server, auth, mirror, router, WS fan-out, embedded SPA | 1 |
| `harness` (crates/cli) | single binary: `serve`, `doctor`, `node join`, `pair` | 1 (join/pair in 2) |
| `harness-skills`, `harness-mcp` | added in Phase 4, not scaffolded empty | 4 |

Change from the brief: `skills/` and `mcp/` are **not** scaffolded as empty
crates in Phase 0. Empty crates rot and add CI time; they are created in
Phase 4 when they have content.

### Data dir

`$HARNESS_DATA_DIR` or `~/.local/share/rizzy-harness/` (`dirs` crate):
`master.db`, `node.db`, `ui-token` (0600), `config.toml`, `node.toml`,
later `keys/`.

The embedded node keeps its **own** `node.db` even though it shares a process
with the master. That duplicates data on a single machine but means
replication and resume-by-cursor are exercised from Phase 1, not first in
Phase 2.

---

## 2. Event schema (decision: expensive to reverse, fixed here)

Wire/storage form (`RawEvent`) — what the store and protocol carry:

```json
{ "session_id": "0193…(uuid v7)", "seq": 17, "ts_ms": 1767225600000,
  "kind": "tool_call_started", "payload": { … } }
```

* `(session_id, seq)` is the primary key and the idempotency key.
  `seq` starts at 1 and is contiguous; a gap is a bug or a missing replay.
* The master mirror stores `kind` and `payload` **opaquely**; kinds it does not
  know are mirrored as they are and rendered generically.
* Typed form (`harness_core::EventKind`) is a serde enum with
  `tag = "kind", content = "payload"`. Unknown kinds deserialize to an error at
  the typed layer only; the raw layer never fails on them.
* From the first release on, payload changes are additive only (new optional
  fields) and a breaking change is a new kind. Before it, the schema changes freely.

Kinds (v1):

| kind | payload |
|---|---|
| `session_created` | `title, cwd, model, backend, node_id` — always seq 1 (**added**: needed so the log alone describes the session) |
| `turn_started` | `turn_id` |
| `user_message` | `turn_id, text` |
| `assistant_delta` | `turn_id, message_id, text` — coalesced, see below |
| `assistant_message` | `turn_id, message_id, text, tool_calls[{call_id, tool, input}]` — authoritative full text |
| `tool_call_started` | `turn_id, call_id, tool, input` |
| `tool_output_delta` | `call_id, stream (stdout/stderr), text` |
| `tool_call_finished` | `call_id, ok, output, exit_code?, truncated` — `output` is what the model sees |
| `approval_requested` | `approval_id, call_id, tool, input, reason, turn_allow` (`turn_allow` only when true: "for this turn" may be offered) |
| `approval_resolved` | `approval_id, decision (allow/deny/allow_session), by (user/policy/cancelled/interrupted)` |
| `checkpoint` | `turn_id, phase (before/after), git_ref` — Phase 3 |
| `turn_ended` | `turn_id, reason (completed/cancelled/error/interrupted), usage?` |
| `error` | `turn_id?, message` |

**Delta coalescing (decision).** One event per token is wasteful for SQLite and
replay. The native loop buffers deltas and flushes an `assistant_delta` /
`tool_output_delta` every 75 ms or 2 KiB, whichever first. `assistant_message`
carries the full text, so the UI and the provider history never depend on
deltas being complete — deltas are a display optimisation.

**Crash recovery (decision).** On node start, every session whose last turn has
`turn_started` but no `turn_ended` gets, in order: `approval_resolved{by:
interrupted, decision: deny}` for each open approval, `error`, and
`turn_ended{reason: interrupted}`. Turns are not auto-resumed: re-running
half-finished side effects is worse than asking the user to continue.

**Durability.** The node store runs WAL with `synchronous=FULL`: an event is
fsynced before it is published to anyone, so a seq the master has seen can
never be reused after a node crash or power loss. The mirror uses
`synchronous=NORMAL` — it can always re-fetch from the node.

**A failed append ends the turn (decision).** The log is written first: an
event is appended before the effect it describes. When an append fails (full
disk, I/O error, a broken database), `TurnHost::emit` returns the error and
the turn ends: no further tool body runs and no further model request is made.
That covers every event, deltas included, the start of a tool call, the
approval request and its answer (an approval the log does not hold allows
nothing), and a tool result (the model would otherwise get a history the log
does not hold). The first failure cancels the turn's token, so a tool still
running or a delegation running beside the one that failed stops through the
usual cancel path and grace period; the rest of the turn's appends fail
without being tried. The closing `error` and `turn_ended` are each tried once;
if `turn_ended` cannot be written, the next prompt first closes that turn
(`error`, `turn_ended{reason: error}`) and is refused, with the store's
reason, while it cannot. `turn_started` and the user's message are written
before the prompt is acknowledged, so a broken log refuses the prompt itself.
The failure is logged once per turn, naming the store's error, never a payload.

### Fold

`harness_core::SessionView::apply(&RawEvent)` is the single reference reducer
(messages, tool cards, pending approvals, turn status). The SPA has a TypeScript
port of the same reducer. "Replay to exact state" is tested as:
`fold(all events) == fold(events seen before crash ++ events after reconnect)`
with the seq sequence strictly contiguous.

---

## 3. Node protocol (decision: expensive to reverse, fixed here)

JSON messages, `{"type": "...", ...}`, `PROTOCOL_VERSION = 1`. Version is
negotiated in `node.hello` / `master.welcome`; a mismatch in major version
closes the link with a reason.

Node → master
* `node.hello {protocol_version, node_id, hostname, version, capabilities, sessions: [{session_id, head_seq}]}`
* `session.event {event: RawEvent}`
* `session.snapshot {session_id, head_seq, events: [RawEvent]}` — a replay batch answering `resume`
* `ack {req_id, ok, error?}`

Master → node
* `master.welcome {protocol_version}`
* `session.create {req_id, session_id, title, cwd, model?}` — master mints the uuid v7
* `session.prompt {req_id, session_id, text}`
* `session.cancel {req_id, session_id}`
* `approval.respond {req_id, session_id, approval_id, decision}`
* `resume {session_id, last_seq}`

**Replication rule (master side):** for an incoming event with
`seq == mirror_head + 1` → store and fan out; `seq <= mirror_head` → drop
(duplicate); `seq > mirror_head + 1` → drop and send `resume{last_seq:
mirror_head}` (at most one outstanding per session). On hello, the master
sends `resume` for every session where `node.head_seq > mirror_head`, and
creates mirror rows for sessions it has never seen. This makes the link
self-healing with no ordering assumptions beyond per-session FIFO.

Pairing (as built in Phase 8): the node has an Ed25519 key; the master has a
self-signed TLS certificate. An admin creates a single-use join token; the
node runs `rizzy-harness node join <master-url> --fingerprint <sha256>` with
it, and the master pins the node's public key. The two ends prove themselves
differently, so this is **not** a mutual challenge-response:
* **Master → node:** the TLS handshake, against the certificate fingerprint
  the node pinned at join (no CA is trusted). The master signs nothing at
  the application layer.
* **Node → master:** after TLS, the master sends a fresh 256-bit nonce; the
  node signs it together with the master's certificate fingerprint (channel
  binding), and the master checks the signature against the pinned key.

**Flow control.** Both directions are bounded queues with back-pressure:
the master holds at most `FROM_NODE_QUEUE` (8) parsed messages per node and
stops reading that node's socket when they are not stored yet; a node holds
at most 64 commands and stops reading the master's socket the same way. A
node's own outgoing queue (256) makes event publishers wait; it never drops
an event, and if the link dies while they wait, the next hello and `resume`
replay from the node's log. Frames are at most 8 MiB (`MAX_FRAME`, refused
from the header); `session.snapshot` batches are at most 500 events and 2
MiB. Idle detection counts only time spent waiting on the peer, so a busy
link is not declared dead. The node gives up on a TCP connect after 10 s and
on TLS plus upgrade, and on the challenge exchange, after 10 s each.

---

## 4. Threat model

The master can make nodes run shell commands. Treat it as an RCE service with a
login page.

**Assets:** shell on every node; source code; provider API keys; the UI login.

**Adversaries and mitigations**

| # | Threat | Mitigation |
|---|---|---|
| T1 | Network attacker reaches the master UI | Bind `127.0.0.1` by default. Non-loopback bind requires TLS (Phase 2) or the explicit `--insecure-bind` flag (tailnet/WireGuard mode), which logs a warning on every start. |
| T2 | Malicious website in the user's browser (CSRF, DNS rebinding, cross-site WS) | Accounts with Argon2id passwords; the first (admin) account needs a one-time claim code from a 0600 file. Login sets one `HttpOnly; SameSite=Strict` cookie holding a random 256-bit session id; only its SHA-256 is stored (`ui_sessions`), so a leaked database does not leak logins. `authenticate` is one indexed lookup joined to `users` (missing, expired or disabled → 401). A session ends 30 days after its last use (the expiry is moved at most once a minute); logout deletes its row, and a password change or reset, disabling the account and deleting it delete all of the user's rows, so a copied cookie stops working at the next request; the same paths close every UI WebSocket opened with those rows at once (close code 4401; logout closes only that login's sockets). Each socket is registered under its user with its session's SHA-256 (never the cookie) before its first login check, and revocation flips that socket's own flag after the rows are deleted, so a socket still upgrading during a logout is caught by one or the other; it is not a broadcast, so a lagging socket cannot skip it. A socket also re-checks its login every 60 s and before every subscribe, read only (an open tab does not slide the expiry; only HTTP requests count as use), which covers expiry and disabled accounts; a store error closes it. Client WS messages and frames are refused above 64 KiB by the WebSocket layer before buffering; a user (the API token counts as one) has at most 8 sockets, the 9th upgrade gets 429 (refused rather than closing the oldest, which would make two tabs evict each other forever). API-token sockets have no login to end and live until they close; unknown usernames are verified against a dummy hash, under the same hash permit and throttle slot, so timing and status do not reveal them (login limits: T12). A separate 256-bit API token (0600 file) serves scripts as `Bearer`. Every request checks `Host` against an allowlist (anti-rebinding). Any `Origin` sent must be exactly `scheme://host[:port]` of an allowed entry with that entry's scheme: the server's own (`https` with `--tls`), or the one an entry names (`--allowed-host https://chat.example` for a TLS proxy in front; `X-Forwarded-Proto` is never read), so `null`, another port or the other scheme are refused. A cookie-authenticated (no `Bearer`) state-changing request (not GET/HEAD/OPTIONS) or WebSocket upgrade without `Origin` is refused (403 `origin required`); `Bearer` clients, cookie-less logins and webhooks need none (audit L6; `master/tests/hardening_http.rs`). Token compared in constant time. |
| T3 | Secrets leak via logs, URLs, event log | Provider keys are referenced by env-var name in config, held in `Secret<String>` (redacted `Debug`), never put in events. Tool output is scrubbed of known secret values before it is stored. No token in any URL (no `?token=`). `tracing` never logs headers. Tool child processes (bash, graphify) get an allow-listed environment: `env_clear()`, then `PATH HOME USER LOGNAME SHELL LANG LANGUAGE LC_* TERM COLORTERM TZ TMPDIR` and the names node.toml's `env_pass` matches; provider key variables, `.env` names and `HARNESS_*` are removed even when `env_pass` matches them. The embedded node and `node run` build it through the same code. Open: `git` children (`node/src/git.rs`) still inherit the node's environment. Redaction guards against accidents only: it replaces exact known values, so an agent (or a hostile prompt) that encodes, reverses, splits or otherwise transforms a secret gets past it; the control is keeping secrets out of the shell's reach. Within that limit it is exact on streams: streamed output (tool output and model text) goes through a `StreamScrubber` that holds back the last (longest secret − 1, at most 4096) bytes until the next chunk or the end of the stream, so a secret that straddles a flush is still replaced; matching is leftmost-longest, and event payloads are scrubbed field by field (never as serialized JSON, where a secret containing a quote or backslash is escaped and would be missed). Residual: a secret cut off by the output cap leaves its prefix before the cap marker, and a secret longer than 4096 bytes may leak if split. |
| T4 | Prompt injection from repo content makes the model run harmful commands | Default approval mode `ask`: every mutating tool (bash, edit, write) needs a human click; read-only tools are confined to the session workspace. **bash and graphify run in a Landlock sandbox** (`rizzy-harness sandbox-exec`, `[sandbox]` in node.toml, FIX_PLAN 2.7): read-write only the session folder and a per-session `TMPDIR` (a sub-agent worktree also gets its repository's `.git`), read-only `/usr /bin /sbin /lib* /etc /opt /proc` and node.toml's `[sandbox] read`, read-write `[sandbox] write`, five device files (not `/dev/tty`: TIOCSTI); `no_new_privs` (setuid programs such as `sudo` fail); TCP connect/bind denied with `network = "off"` (default on, Q6); abstract Unix socket and signal scoping (ABI 6) and pathname Unix socket connects (ABI 9) best-effort. `mode = "required"` (default) refuses agentic sessions (and `node run` refuses to start) unless a start-up self-test through the wrapper is denied a canary in the data directory and the kernel enforces Landlock ABI 3 in full (plus ABI 4 TCP rights with `network = "off"`); a `[sandbox]` entry or system folder that equals, contains or is inside the data directory stops the node. Tests: `crates/cli/tests/sandbox.rs`, `tests/data_dir.rs`. File tools walk every path with `lstat`: a symlink that dangles or leaves the workspace is refused, `write`/`edit` do not follow a link at the last component, and nothing is written under `.git` (its config and hooks run commands). Stored shell output (`tool_output`) has random ids and belongs to the session that produced it. Residual: an intermediate directory swapped for a link between check and open is followed (by the file tools; the sandbox still bounds bash); sandbox residuals in R2. "Allow for this turn" (`allow_session`) allows every later call of that tool in the turn, `bash` commands included, so the node honours it only after the tool's first approval in the turn (the request carries `turn_allow` when it may be offered; an earlier one is applied as a single allow, whatever the client sent); the UI labels it as exactly that. |
| T5 | Compromised master widens node permissions | **Node-local policy (`node.toml`) is authoritative** and is not settable over the protocol or over HTTP (no endpoint writes it or installs software; graphify is allowed only by `graphify = true` in it, read at start): allowed workspace roots, max approval mode (`ask` default; master can request stricter, never looser), tool denylist. The node logs every command it received in its own store. |
| T6 | Compromised master approves things on nodes | Accepted in v1: the master is the approval channel, so a compromised master can approve anything inside node policy. Bounded by T5. Future: approvals signed by a user device key verified on the node (R1). |
| T7 | Rogue node / stolen node key | Node keys pinned after operator approval; revocation deletes the pin and drops the link. A node only writes its own sessions; the master rejects events for sessions owned by another node. |
| T8 | MITM on the node link | WSS with the master's certificate pinned by fingerprint on the node (no CA, so a mis-issued certificate is not enough), and the node's Ed25519 key pinned on the master: the node signs a fresh master nonce bound to that fingerprint, so its signature cannot be replayed to or relayed through another endpoint. The master authenticates only through TLS (it signs nothing itself). Both directions are bounded queues with back-pressure, and the node times out connect and handshake (§3), so neither end can make the other buffer without limit or hang it. |
| T10 | Another account reads or drives someone else's work | Every session carries `owner_id`; REST and WS show a session only to its owner, admins included (server admins manage accounts, workspace admins and owners manage members and defaults; neither gets chats). A workspace only groups chats: its mapping is master-side (never asserted by a node, T7) and is ignored once the owner leaves, so a chat cannot be planted in or stranded outside its owner's workspaces. Only the API token, the machine owner's key on disk, sees all. **The data directory stays out of every session** (it holds the API token, the session keys, `secrets.toml` and `master.db` with everyone's chats): every node runtime protects its own data directory (for the embedded node, the master's), a session cwd that is inside it, equal to it or an ancestor of it (`$HOME` with the default `~/.local/share/rizzy-harness`) is refused at create and again before every turn and restore (sessions loaded from the log included), and `read`/`write`/`edit`/`outline`/`grep`/`glob` refuse any path whose real location (links and `..` resolved, a dangling link judged by its target) is inside it, `allow_outside_workspace` included; searches rooted above it skip it. The API token is in the redaction set. bash and graphify cannot open the data directory: the sandbox grants nothing in it but the session's own temp folder (`<data>/sandbox-tmp/<session>`) and graphify's own output folder (T4). Residual: on kernels before Landlock ABI 9 a command can reach pathname Unix sockets and through them start programs outside the sandbox (R2), which is why the embedded node stays admins-only by default (T11). |
| T11 | A low-trust account gets a shell | Agentic sessions run bash on a node, so they need a per-user `can_agentic` grant (default off; admins always on) — the security boundary. A workspace role must also allow agentic work there (an extra AND, never a grant). Both are checked at create and again before every prompt, allowing approval and restore, so revoking either stops further work; running agentic turns of affected users are cancelled. Deny and cancel always stay possible. Chat sessions have no file or shell tools. **The embedded node is for server admins** (`[nodes] local_node_users = "admins"`, the default): it runs as the server's own OS user beside its data directory; bash there is sandboxed (T4), but before Landlock ABI 9 the session bus or a Docker socket gets a command out of the sandbox (R2), so a shell there is still treated as admin-equivalent. Other accounts with the grant get 403 ("use a remote node, or ask the server admin") at create and at every prompt, allowing approval and restore on a `local` chat, are not offered the node in `/api/nodes`, and new chats without a chosen node go to a remote one; agentic routines on `local` are refused at save and at every fire; demoting an admin cancels their running `local` turns. The API token counts as admin. `"granted"` lets every account with the grant use it, for installs where those accounts are trusted like admins. |
| T12 | Password guessing / account takeover | Login throttle (5 failures per username per 15 min → 429 with `Retry-After`; while locked even the right password is refused). Order of checks at `/api/login` (`Auth::login`): body over 16 KiB → 413 before parsing (also `/api/setup`); a username that fails the account format (1–32 of `a-z 0-9 _ . -`, checked on length before any copy) or a password over 1024 bytes → the same 401 body as a wrong password, with no map entry and no hash; a locked name → 429 without waiting; then one of **4 Argon2 permits** (wait at most 2 s, else 429 `Retry-After: 1`); then a failure slot is **reserved under one lock** before hashing (so 50 parallel guesses record 5 failures and the rest get 429), kept on failure, cleared on success, given back on a store error or a cancelled request. Well-formed unknown names take the same path (permit, slot, dummy hash) as known ones. Every Argon2 run in the process (login, setup, account creation, password change and reset) shares the 4 permits, so hashing uses at most 4 × 19 MiB; a permit moves into the blocking task, so a disconnect frees it when the hash ends and a panic frees it while unwinding. The failure map holds at most 65,536 names (about 10 MiB); when full, expired names go first, then the name whose latest failure is oldest. Residuals, accepted: (1) an attacker can lock out a known username for 15 minutes; (2) anyone who can reach the port can keep all 4 permits busy, so other logins, setups and password changes get 429 while the flood lasts (no per-IP limit yet; it ends when the flood does); (3) eviction: flushing a locked name needs 65,536 newer failed names, each a full hash through the 4 permits: at ~50 ms per hash (estimate for a current x86 core, not measured) that is ~80/s, about 14 min per flush, ~7 min on hardware twice as fast, during which (2) holds for everyone. That buys at most 5 extra guesses per flush, so a single name sees at most ~2–3× the 20 guesses/hour the throttle allows. Refusing new names when full was rejected: the same cost would lock every not-yet-listed username out for 15 minutes after the flood stops. `/api/setup` gets the format check first, the body limit and the permits, but no failure throttle: the claim code has ~100 bits, a wrong code costs no hash, and a global throttle would let anyone block the owner's claim. No self-signup; admin-created accounts start with a temporary password that must be changed; disabling, deleting or resetting a password signs the user out everywhere, open WebSockets included (T2). The last active admin cannot be removed or demoted, and admins cannot change their own access. Workspace managers add members by username, so the answer tells them whether a username exists: failed adds are throttled per caller (10 per 15 min → 429), accepted as a residual leak. |
| T13 | Credentials sniffed on the network | Default bind is 127.0.0.1. `--insecure-bind` serves plain HTTP with a loud warning (VPN/tailnet only); real TLS arrives with the node link (Phase 8). |
| T9 | Runaway processes | Bash runs in its own process group; cancel/timeout sends SIGTERM then SIGKILL to the group. Cancel propagates master → node as `session.cancel`. Output is capped: the model's copy at 30 000 characters; the live stream (`ToolOutputDelta` events, stored and replicated) at `[limits] tool_stream_bytes` per call (default 1 MiB, 4 KiB–1 GiB), after which one marker is sent and the rest is read and dropped so the child never blocks. The stream's channels are bounded (8 pipe chunks, 32 sink chunks); a full channel back-pressures the pipe reader, and cancel and timeout are awaited alongside every send. **No tool can wedge a node** (AUDIT S10): file tools open regular files only, checked before opening (a device is never opened) and again with `fstat` on the handle, opened `O_NONBLOCK`/`O_NOCTTY` so a FIFO swapped in meanwhile never blocks; `read` streams through a fixed buffer, keeps only its line window and stops at 32 MiB with a `file truncated at N bytes` note; `grep` skips non-regular files (links are not followed) and files over 8 MiB, saying how many; `edit`/`outline` refuse files over 8 MiB. Every tool gets the turn's live cancel token (`read`, `grep`, `glob` check it per chunk or entry); after a cancel a tool has 2 s (`TOOL_CANCEL_GRACE`) to return before the loop abandons it, so a tool stuck in a syscall cannot hold the turn (its blocking thread may linger). The node's command loop answers `Cancel` and `ApprovalRespond` inline and hands every other command to its session's lane (a bounded queue of 64 and one task per session, ended after 30 s idle): one session's commands run in order, sessions never wait for each other, a full lane nacks at once; an inline cancel also leaves a marker in the lane so a prompt queued before it is stopped too. Events are still appended only through the session's `append` (one writer, contiguous `seq`). Residual: the type check before `open` and the open itself race, so a device swapped in at that instant is opened (and refused on `fstat`); a node restart loses queued commands, which were never acked. |
| T14 | Model output (possibly steered by injected web or repo content) phishes, exfiltrates or poses as the harness through what the chat renders | No model HTML, scripts, iframes or remote images; links show their real host and odd ones need a confirm with the full URL; `ui:` blocks are validated in Rust and drawn from the stored spec in a "from <agent>" frame; secret-asking questions and fields are refused; answers carry ids, never model text; the node checks every answer against its log. Details: Status, "2026-11-07 — Rich messages". |
| T16 | Routines run without a human present, partly on external webhook data: injected text steers an agent or uses its web tools as a relay, a revoked user's routine keeps running, a webhook is abused or its sender locked out | The owner's account, membership, `routines` permission and, for agentic routines, the grant and `agentic` permission are re-checked at every fire; a refusal pauses the routine. Webhook data only fills the prompt, single pass, cleaned and capped, and marks the run untrusted: new chat, memory off, `web_fetch`/`web_search` and `remember` withheld, the routine's own agent, fixed title, an untrusted note; the flag sticks to the chat for every later turn (`routine_untrusted_chats`). Agentic routines run in `ask`/`ask_all` only (no `auto`); parked approvals, delegates' included, are cancelled after 24 h. Webhook secrets are 256-bit, header-only, stored as SHA-256, compared in constant time; throttles count failures only before authentication, and audit lines carry no content. Caps per user, per workspace and per day. Details: [docs/routines.md](docs/routines.md). |
| T17 | Files stored and served from the app origin (uploads, published builds) run script, phish, crash the viewer's tab, or outlive their deletion | Kinds by magic bytes; inline only for sniffed raster image, audio and video (SVG and HEIC only in `<img>` context, with `Vary` and a variant ETag); everything else attachment + octet-stream; a per-response sandbox CSP that the guard keeps; nosniff, CORP same-origin; owner-only access (or shared-artifact members); corrected download extension; sniffed thumbnails; dimensions from headers and no browser decoding above 40 MP; no server-side decoding or unpacking; no import by URL. Per-blob keys plus `secure_delete` and a WAL truncate remove deleted files and their keys from the live files (not from SSD or copy-on-write remnants of master.db): crypto-shredding, not encryption at rest, since the keys sit in master.db beside the files (audit L8; format and test vectors in docs/media.md); account deletion cleans files, FTS rows and the WAL at once. Details: [docs/media.md](docs/media.md). |
| T18 | A compromised node or steered agent uses publishing to fill the master's disk, pull secrets, plant files in another user's artifacts, or slip files past checks; a compromised master uses stage ids to read or delete node files | Publishing is a mutating tool (approval in `ask`) and node.toml `[artifacts] enabled` can turn it off; node-side confinement (session cwd or workspace roots, never the data dir, `O_NOFOLLOW` + fd re-check, regular files only, secret-like names refused or skipped); a staged copy; stage ids resolved through the node's own index, never as paths; pull-only, windowed transfers started by a validated event from the owning node and sent only to nodes that announce the `artifacts` capability; owner, workspace and project from the publishing session's own row, never `parent_id`; the path a version was published from (`meta.source_path`) is shown to the artifact's owner only, never to the members of a shared artifact (audit L5); exact size and sha256; quotas, a free-disk floor, per-node pending and hourly caps, and per-owner lineage caps. Details: [docs/artifacts.md](docs/artifacts.md). |
| T19 | A working copy arrives with a hostile git configuration (an unpacked archive, a shared folder): `.git/config` names `core.fsmonitor`, hooks, filter, diff or signing commands, and the harness runs git there before anything is approved (every turn starts with a checkpoint) | **Turn checkpoints, their diffs and undo never use the user's repository:** they run in a harness-owned shadow repository (`<data>/checkpoints/<hash>.git`, `--git-dir`/`--work-tree`), which reads the user's objects through `objects/info/alternates` (data only; new objects go to the shadow) and copies `.git/info/exclude`; the work tree's `.gitattributes` are switched off (`GIT_ATTR_SOURCE` = empty tree, `core.attributesFile=/dev/null`) and name no defined filter in any case. **Every git process** (`node/src/git.rs`): cleared environment (only `PATH`, `HOME`, `LANG`, `TMPDIR`; no inherited `GIT_*`), `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null` (one exception: a read-only `git config --global --includes --get core.excludesFile` that finds the user's global ignore file, whose patterns are copied into the shadow), no prompts or askpass, and `-c` overrides for `core.fsmonitor`, `core.hooksPath=/dev/null`, pager, editors, `core.askPass`, `credential.helper`, `commit.gpgSign`, `protocol.allow=never` (no ssh/proxy/`ext::`/lazy fetch), `submodule.recurse`, auto gc/maintenance; diffs pass `--no-ext-diff --no-textconv`; a 120 s timeout per call, and the process group is killed on timeout or cancel. **Sub-agent worktrees** must run in the user's repository (`worktree add`, its base/result snapshots, `git apply`): same hardened base, hooks never run, and before anything runs there the repository's config (`git config --list --includes`, which executes nothing) is checked; any `filter.<x>.clean/smudge/process` other than git-lfs's standard commands, or git-lfs's own command keys (`lfs.customtransfer.*`, `lfs.extension.*`, `lfs.standalonetransferagent`), refuses the worktree with the key names and the sub-agent works in the shared folder. Tests in `git.rs` and `tests/runtime.rs`. **Residual:** git-lfs's standard filters still run in a worktree checkout (and git-lfs reads the repository's own config, so a network fetch from its `lfs.url` can happen); the artifact `describe` call (`git rev-parse`/`status` in the published folder) runs in the user's repository with every defined filter blanked on the command line; the user's object store is trusted as data, so an attacker who controls `.git/objects` can make a checkpoint record (and undo write back) a forged blob for content git believes it already has, which only rewrites files inside that same working copy; config keys only consulted by commands the harness never runs (`alias.*`, `merge.<x>.driver`, `uploadpack`/`receivepack`, `core.alternateRefsCommand`, `sendemail.*`) are not overridden. |
| T20 | A project member plants instructions in every other member's agent through shared project memory (cross-user prompt injection, audit S12) | Project facts reach every member's turns, so only active ones do and only a manager activates them: `PATCH {status: "active"}` needs `settings` unless a manager turned on the project's `members_activate_facts` (default off), and then a member confirms only their own proposal. Adding, rewording or deleting an active fact is always a manager's. A proposal is visible only to its source chat's owner. The agent path (`remember` scope `project`) only files proposals, for a manager's chat too: activation is a human request. Residual: with the switch on, a member (guests included) can publish text to the project. [docs/projects.md](docs/projects.md). |
| T21 | Slow or numerous clients hold the HTTP server's connections and handlers (slowloris, a silent socket, a TLS handshake that never completes, a handler that never returns) | **The serve loop is ours** (`master/src/serve.rs`, replacing `axum::serve` and the old accept-loop in `cli/src/net.rs`): hyper's HTTP/1 server with a header-read timeout of 10 s over the whole request head (half a request line, silence and a header a byte at a time all end at the same deadline; it is also the idle time of a keep-alive connection), and a 10 s timeout on the TLS handshake. **Connection cap** 512: the permit is taken before `accept`, so at the cap the server stops accepting and clients wait in the kernel backlog; the permit lives in the connection task and is released however the connection ends. An upgraded connection (WebSocket) releases its permit at the upgrade: sockets neither count against the HTTP cap nor hold shutdown; their number is bounded per user in `ws.rs`. **Request timeout** (`master/src/limits.rs`, a route layer keyed by the route template): 30 s on every route by default, 120 s on the routes that call a provider, none on the WebSocket, uploads, blob downloads and artifact saves; a timed-out request answers 504 `{"error":"request timed out"}` and its handler future is dropped. A new route is limited by default. **Shutdown**: Ctrl-C stops accepting, lets in-flight requests finish for up to 30 s, then closes. HTTP/2 is not enabled. Tests in `master/tests/server_limits.rs`. **Residual:** the request timeout bounds the handler's future, not the work it spawned (`spawn_blocking`, detached tasks); there is no write timeout, so a client that reads a response slowly holds its connection (and permit) until it finishes or disconnects; nothing limits connections per source address. |
| T22 | A prompt-injected model uses the harness's network clients as a relay or a way inward: `web_fetch` to an internal address or to a URL it built to carry data out, a redirect or DNS answer that points inside, a backend server that redirects a request carrying an API key | **`web_fetch`:** the public-address check runs on every address the client connects to (our DNS resolver drops non-public answers, so a rebinding name cannot reach an inner address; IP-literal URLs and every redirect hop are checked by `check_url`; environment proxies are ignored). `is_public` refuses loopback, private, link-local, CGNAT, multicast, documentation, reserved IPv4 and, for IPv6, whole ranges that embed or tunnel to IPv4 rather than decoding them: `::/96` (unspecified, loopback, IPv4-compatible), SIIT, NAT64 (`64:ff9b::/32`), 6to4 (`2002::/16`), `2001::/23` (Teredo and other protocol assignments), discard-only, unique-local, link/site-local, multicast, documentation (`2001:db8::/32`, `3fff::/20`), SRv6; IPv4-mapped addresses are judged as their IPv4 address. **Provenance:** a URL is fetched only if it appears, spelled the same (host case, default port, fragment and IDN folded by the `url` crate; path, query, port, trailing dot and userinfo not), in the user's own words this turn or earlier, or in the output of a *successful* `web_search`, `web_fetch` or `read` call. Never in: any tool's error text, `bash` output (and so `tool_output`), grep/glob/outline, memory recall and per-turn page context (they ride on the user message but are stripped from the provenance set), a sub-agent's task text (written by its parent model), or the model's own text and call arguments. A sub-agent inherits its parent's set as it stands when the task is delegated (the user's words and the parent's successful search/fetch/read results; passed by the node in-process as `TurnExtra::observed`, shared by `Arc`, never from the protocol, and grandchildren inherit it the same way) and adds its own successful results; a URL the parent could not fetch is refused in the child too, so an injected parent cannot launder one through the task. Residual: a page the model legitimately read can contain URLs, so the trusted set includes them; a file the model wrote and then `read` can plant one (writing needs approval in `ask`); `www.` and the bare domain count as the same host; a pasted URL can still carry data the user typed. **mem0 backend:** every id and owner id is one percent-encoded path segment or query value (`/`, `?`, `#`, `%` cannot change the path; `.` and `..` are never sent), and the client uses the same-origin redirect policy of the embeddings client, so a mem0 server cannot send `X-API-Key` to another host. Tests: `crates/memory/tests/mem0.rs`. |
| T23 | Any account (a chat-only guest included) uploads a hostile file the server parses: a PDF that loops, inflates without bound, overflows the stack, panics or exploits the parser, inside the process that holds every key; or floods uploads and paid dictation | **The server never parses a PDF** (FIX_PLAN 3.4, AUDIT S7; `crates/master/src/pdf.rs`). A file is a PDF only by `%PDF-` at offset 0, never by name (a `.pdf` text file is text). The server starts its own binary as `rizzy-harness pdf-text` (dispatched before any runtime: one thread, no `.env`) with an empty environment, cwd `/`, stderr discarded, in its own process group. Before reading a byte the helper confines itself with Landlock (ABI 3 file rights with **no** rule, so no file can be opened at all; ABI 4 TCP connect/bind denied; `no_new_privs`), then caps its address space (`RLIMIT_AS`) at 512 MiB above its start-up mappings, so a bomb's allocation fails and the helper aborts. The server: 2 helpers at once, a request waits at most 10 s for a slot (then 503), 20 s per file, stdout capped at 400 KB (then killed), the group SIGKILLed and the child reaped on timeout, excess output or a dropped request; slots are RAII permits. **Where Landlock or ABI 4 is missing (Linux before 6.7, non-Linux) PDFs are refused** (422 "cannot read PDFs safely"): an exploited parser that could read `<data>` could return `secrets.toml` as "extracted text" to the uploader, so timeout and memory limit alone are not enough. Nothing about a file (name, bytes, text, parser message) is logged. Unsent uploads: 20 and 8 MiB of text per account (counted and inserted under one lock; per owner, so one account never fills another's), swept after the incognito TTL; taking them for a message is one transaction, so the sweep never splits a send. Dictation: one at a time per account (429, not queued) and `[voice] daily_requests` / `daily_mb` (200 / 100 MiB per UTC day) counted in memory before forwarding; a restart resets the day's counts (only an admin restarts). Tests: `pdf.rs`, `attachments.rs`, `voice.rs`, `master/tests/pdf_http.rs`, `store/tests/attachments.rs`, `cli/tests/pdf_text.rs` (real helper: text, malformed file, a 768 MiB flate bomb aborts on its limit). **Residual:** `ttf-parser` is unmaintained (`deny.toml` names this row); the helper can still spend 20 s of one CPU and 512 MiB per slot; UDP is not covered by Landlock (the helper has nothing to send but the uploader's own file); a real parser-looping PDF is covered by the supervision test with a stand-in, not by a crafted file. |
| T24 | An admin session (a careless admin, a stolen login, a second admin) uses the provider and model tests to read the server's own network: cloud metadata, a router, a database's HTTP port (SSRF, audit L3) | `POST /api/admin/providers/test`, `/providers/models/test` (chat and embeddings) and every model listing (`refresh`, the daily refresh) go through `outbound::Outbound::provider_call` (`master/src/outbound.rs`): the URL must pass `url_allowed` (https, or plain http only to `localhost`/`127.0.0.1`/`[::1]` or a listed host; no userinfo), then the connection uses `web_fetch`'s public-only resolver, which checks the address actually connected to on every connection (DNS rebinding included); a private IP literal or `localhost` is refused before any connection. Redirects stay on the same origin (`same_origin_redirects`), so a listed host cannot bounce the call inward. Local model servers are allowed by name in `HARNESS_PLAIN_HTTP_HOSTS` (read at start), `localhost` included; the refusal names the variable and the value to add. Tests: `master/tests/hardening_http.rs`, `outbound.rs`. **Residual:** chat turns and memory embeddings use the saved provider URL without this check (it was checked by `url_allowed` when saved, but https to a private address passes that); the transcription client follows cross-origin redirects; a listed host may itself be a proxy to anywhere. |
| T25 | A response leaks internals or outlives its session: a 500 with SQL text or a path, an API answer or stored file left in the browser's disk cache after logout or delete, a downgrade to plain HTTP (audit L4, L6) | The outermost layer (`master/src/response.rs`) gives every response a random 64-bit request id (`x-request-id`, generated per request, never taken from the client); any 500 body is logged with that id and replaced by `{"error":"internal error","request_id":…}`, for every handler by construction, not per call site. Every `/api/*` response, errors, stored files and 304s included, gets `Cache-Control: no-store` (the hashed UI files keep their caching). With `--tls`, `Strict-Transport-Security: max-age=15552000` (180 days, no `includeSubDomains`: the harness does not own its host's siblings), not for `localhost` names (HSTS is per name, not per port, and would switch every other local dev server to https) or IP addresses. Tests: `master/tests/hardening_http.rs`. **Residual:** 5xx other than 500 (502 from a provider, 503 node offline) keep their messages; a panic in a handler closes the connection without a response; behind a TLS-terminating proxy the proxy must send HSTS. |
| T26 | Another local user reads a secret file the server uses (a restored backup, a copied `.env`, a key file with a loose umask) (audit L7) | At start `serve` and `node run` refuse any secret file whose mode has group or other bits (`mode & 0o077`), before reading it, with the path, the mode and `chmod 600 <file>`: `secrets.toml`, `api-token`, `claim-code` (both in `load_or_create_secret`, so every reader), private keys in `keys/` (`*-key.der`, `node.key`) and the `.env` `serve` loads. A symlink is judged by its target. Files the harness writes are created 0600 by `open(mode)`, never chmod-ed afterwards; `secrets.toml`'s temp file is removed and created anew, so a stale one cannot keep a looser mode. Unix only (other systems have no mode bits; nothing is checked there). Tests: `cli/tests/data_dir.rs` (`open_secret_files_stop_the_server`), `auth.rs`, `secrets.rs`. **Residual:** ownership is not checked (a 0600 file owned by another user is still read); `master.db` (chats, blob keys) is protected by the 0700 data directory only; `link::write_private` reuses a stale temp file's mode. |
| T28 | The node's store cannot append (full disk, I/O error, a corrupted or locked database, perhaps brought about on purpose by a command that fills the disk), and the agent keeps acting with nothing in the log: a tool runs, an approval is acted on, the model is called, none of it auditable or visible to the user (final review A6, A7) | **A failed append ends the turn** (§2, "A failed append ends the turn"): `TurnHost::emit` is fallible; the native loop (`agent-native`, `Logged`) stops at the first failure, cancels the turn's token (running tool and parallel delegations stop within the grace period) and fails every later append untried; the node's session denies an approval whose request or answer was not written, skips the after-turn checkpoint, tries the closing `error` and `turn_ended` once each, frees the turn slot and, if the end was not written, closes that turn before the next prompt. A prompt whose `turn_started` or message cannot be written is refused with the store's reason, so the sender sees it. One `tracing::error!` per turn, with the store's error only. Tests: `agent-native` (`a_tool_does_not_run_when_its_start_cannot_be_logged`, `no_model_request_after_a_result_cannot_be_logged`, `a_failed_append_cancels_the_parallel_delegations`), `node/tests/runtime.rs` (`a_tool_does_not_run_when_its_event_cannot_be_written`, a real SQLite store broken by a trigger). **Residual:** a tool that was already running when a later append failed (one of its output deltas) has had its effect, and its result is not recorded; a turn whose end could not be written shows as running in the UI until the next prompt or a node restart closes it; the mirror and the UI learn of a mid-turn failure only when the store works again (no other channel, no new protocol message). |

---

## 5. Decisions taken (beyond section 2 of the brief)

* **SQLite via `rusqlite` (bundled)**, not sqlx: synchronous, simple, no
  compile-time DB, single static binary. Calls go through `spawn_blocking`.
* **Providers via `genai` 0.7.2, pinned exactly** (replaced the Phase 1
  hand-rolled OpenAI SSE client). Covers Anthropic Messages API and any
  OpenAI-compatible endpoint, streamed tool calls, reasoning effort, custom
  endpoints. Chosen over `rig-core` (an agent framework that would compete
  with our loop) and `async-openai` (OpenAI only). Verified from its source:
  `AuthData` redacts in `Debug`; its OpenTelemetry spans (which can carry
  prompts) sit behind the non-default `otel` feature. Used only in
  `agent-native/src/llm.rs`, behind our `Provider` trait.
* **Secrets file** `<data>/secrets.toml` (0600, atomic writes, refuses to read
  if group/other-readable) for keys entered in the UI; config references keys
  by name (`api_key_secret`) or env var (`api_key_env`).
* **Two runtimes in one process.** The embedded worker node (`local`, agentic,
  `node.db`) and the master's chat runtime (`master`, chat profile, `chat.db`,
  no file or shell tools) both run the same `NodeRuntime`/session code; each
  announces its profile in `node.hello` (`profile:chat` / `profile:agentic`)
  and the master routes new sessions by profile.
* **Checkpoints always in a harness-owned shadow repository** (2026-11-10, fix plan 2.6,
  T19), also for git repositories: the user's config, hooks, filters and index are never
  consulted for a checkpoint, diff or undo. Rejected: hardening in-repo calls only (a
  filter name can only be neutralised once known, and `commit-tree` honours
  `commit.gpgSign`; every new git option is a new hole). Costs: the first checkpoint of a
  repository hashes every file once (the shadow keeps its own index afterwards; objects
  already in the user's repository are not stored again, via alternates); snapshots are
  the bytes on disk (no eol/filter conversion), which makes undo exact. Ignore rules are
  the same as before: `.gitignore`, `.git/info/exclude`, and the user's global ignore file
  (`core.excludesFile` read by one dedicated `git config --global --get`, the only call
  that sees the global config, else `$XDG_CONFIG_HOME/git/ignore` or
  `~/.config/git/ignore`), copied into the shadow's `info/exclude` at every checkpoint
  (regular files of at most 1 MiB only). Checkpoint
  commits have no parent. A sub-agent's temporary worktree gets no turn checkpoints (it is
  deleted when the sub-agent ends). Sub-agent base/result refs stay under `refs/harness/`
  in the user's repository (`worktree add` needs the commit there) and are pruned to the
  latest `checkpoints_keep`.
* **Agents travel with commands.** The master owns agent definitions and puts
  the resolved `AgentSpec` on `session.create` / `session.prompt` /
  `session.set_agent`; nodes never store definitions. Effort is stripped by the
  master unless the model is registered with `effort = true`.
* **YAML via `serde-saphyr`** (pure Rust, released 2026-10) for agent and,
  later, skill frontmatter; `serde_yaml` is deprecated and its forks idle.
* **`cargo-deny`** in CI: permissive licences only; MPL-2.0 allowed solely for
  `option-ext` (via `dirs`, unmodified); internal crates are `publish = false`.
* **Model registry** in `config.toml`: `[[providers]]`, each with its
  `models` and what the provider reported about them (context, max output,
  effort, vision, tools; unreported = unsupported). Model ids are
  `<provider id>::<raw id>` (`::` because raw ids contain `/`); one router
  provider sends each request by prefix. Effort is only sent to models with
  `effort = true`.
* **Process-group kill via `nix::sys::signal::killpg`** (safe API, no `unsafe`
  in our code); `tokio::process::Command::process_group(0)`.
* **Approval modes:** `ask_all` (every tool), `ask` (default: mutating tools),
  `auto`. Decisions: `allow`, `deny`, `allow_session` (this tool, every later
  call in the same turn, not the session; the UI says "for this turn"). The
  node honours it only after the tool's first approval in the turn: on the
  first request (`approval_requested.turn_allow` absent/false) it is recorded
  and applied as a single `allow`, whatever the client sent.
* **UI ⇄ master:** REST for commands, one WebSocket for streams. Client sends
  `subscribe{session_id, after_seq}`; server subscribes to live broadcast
  *before* reading the DB backlog, then drops live events `<= last sent seq`, so
  there is no window where an event is missed or doubled.
  A socket ends with its login (T2): logout, password change or reset,
  disable and delete close it with code 4401, and it re-checks its login
  every 60 s and before each subscribe. Client messages are at most 64 KiB;
  at most 8 sockets per user.
* **Mock provider** (the `demo` provider, `kind = "mock"`) ships in the binary: deterministic
  scripted replies with tool calls. Used by tests and for trying the UI without
  an API key.
* **Frontend:** React 19 + Vite + TypeScript 5.9 (not TS 7 — boring choice),
  no UI framework, hand-written CSS. `web/dist` is embedded via `rust-embed`;
  a `build.rs` creates an empty `web/dist` if missing so `cargo test` works
  without Node; the server then serves a "UI not built" page.
* **pnpm 12 for `web/`** (2026-11-08; npm before). `packageManager` pins
  pnpm@12.17.0 and pnpm records it, with integrity, in `pnpm-lock.yaml`. pnpm 12
  ignores both npm's `overrides` and the `pnpm` field in package.json, so
  overrides live in `web/pnpm-workspace.yaml` (the hastscript pin THIRD_PARTY_NOTICES
  lists). CI uses `pnpm/action-setup` reading `packageManager`; the image installs
  that version with `npm install --global` (no corepack, which Node 25+ no
  longer ships). The isolated layout keeps transitive packages out of
  `web/node_modules`, so `scripts/size.mjs` finds KaTeX through
  `remark-math` → `micromark-extension-math` instead of a fixed path.
  `package-lock.json` is gone; it had also fallen behind `package.json`.
* **Phase 1 binds loopback only**; TLS arrives with the node link in Phase 2.
* **MCP:** use the official `rmcp` crate (verified: crates.io `rmcp`, repo
  `modelcontextprotocol/rust-sdk`, 3.8.0 as of 2026-10-26). Pin at Phase 4 start.
* **Sidebar areas dependencies (2026-11-08)**, all added at once before the parallel work
  so `Cargo.toml` and `Cargo.lock` never conflicted, each with minimal features:
  * **`croner` 4.0.2** (`jiff` feature) for routine schedules: exactly 5 cron fields, in an
    IANA time zone through `jiff`, with the DST gap and overlap rules in
    `routines/schedule.rs`. 4.0.3 was days old (two-week rule). It brings `derive_builder`,
    `darling` and `strum` at build time (duplicate-version warnings in `cargo deny`, not
    failures). `jiff-cron` was the fallback, unused.
  * **`aws-lc-rs` 1.19.0** used directly for the blob store: AES-256-GCM with a random key
    **per blob** held only in master.db, in 64 KiB STREAM segments with the blob id as AAD, so
    a range read decrypts only its segments. Deleting a file deletes its key under
    `secure_delete` and truncates the WAL (crypto-shredding of the live files, not of SSD
    remnants). The same version was already linked through rustls.
  * **`infer` 0.22.0** (no default features) for sniffing by magic bytes; never by extension.
  * **`tar` 0.4.46** (no xattr), **`flate2` 1.1.10** (`rust_backend`) and **`walkdir`
    2.5.0** on the node, to pack a published folder into `.tar.gz` (writing only; the master
    never unpacks).
  * **`base64` 0.23.1** as a workspace dependency (file chunks in protocol messages),
    **`nix` gains `fs`** on master and node (`statvfs` for the free-disk floor, open flags
    for the node's `O_NOFOLLOW` reads), **`unicode-segmentation` 1.13.3** for
    `search::first_sentence` (sentence and grapheme boundaries).
  * **Hand-written, by exception to "use maintained dependencies":** the HTTP `Range`
    parser, the image-header readers (PNG, JPEG, GIF, WebP, AVIF, BMP dimensions, read
    without decoding) and the ELF/PE/Mach-O checks in `sniff.rs`. Each is a few dozen lines
    over bounded input and avoids pulling in a decoder; the header readers have a fuzz test,
    the range parser table and random-range tests.

## 6. Licences of references

None of their code is copied. Ideas only. Verified: this repo is MIT.
Moltis, esper-relay are stated MIT and vibe-kanban Apache-2.0 in the brief;
Mjolnir GPL-3.0 (docs only). **Not verified by me:** sci-pi's licence (brief
says none visible — treat as all-rights-reserved), agentic-harness maturity,
DeepSeek Harness UI licence. If any pattern from the DeepSeek UI is ported
literally, its MIT notice goes in `web/THIRD_PARTY.md`.

## 7. Open risks

* **R1** Master compromise = approval forgery within node policy (T6).
* **R2** *Closed by FIX_PLAN 2.7* (bash and graphify in a Landlock sandbox,
  T4). What the sandbox does not cover, accepted or open:
  * **OPEN: pathname Unix sockets before Landlock ABI 9 (Linux 7.1).** On such
    kernels (Ubuntu 24.04's 6.8 among them) `connect()` to a socket outside
    the grants is not mediated: `/run/user/<uid>/bus` (`systemd-run --user`
    runs anything unconfined), `/var/run/docker.sock` (`docker run -v
    /:/host`), an ssh-agent socket. Mitigation, the operator's job and in the
    README: run the node as a dedicated OS user without `docker` group
    membership and without a login session (no session bus). The test
    `pathname_unix_sockets_need_abi_9` records which side of the line a
    kernel is on.
  * UDP is not covered by Landlock: `network = "off"` stops TCP only; DNS and
    any UDP exfiltration still work.
  * Metadata is not hidden: `stat`/`test -e` on paths outside work (names,
    sizes, times), only opening, listing and changing are denied; `/proc`
    lists other processes and their fd numbers (not their targets,
    `environ` or `mem`: ptrace-class access outside the domain is denied).
  * `/etc` is readable (world-readable files only; `shadow` and host keys
    stay root-only by file mode).
  * The session folder's own `.git` is writable from bash, so a command can
    plant hooks or config that run the next time someone runs git there (the
    file tools refuse writes under `.git`; T19 keeps the harness's own git
    calls from running them). A sub-agent worktree's grant on the main
    `.git` also covers sibling worktrees there.
  * File descriptors the harness leaves open across `exec` are not
    re-checked by Landlock; the harness opens everything `O_CLOEXEC`.
  * The wrapper is the harness binary at the path resolved once at start; an
    upgrade that replaces it is used from the next command on, one that
    deletes it makes every command fail (closed).
  * `best-effort` and `off` are explicit, logged choices in node.toml;
    `best-effort` on a kernel without Landlock confines nothing.
* **R3** Coalesced deltas mean the UI can lag up to 75 ms; acceptable.
* **R4** OpenAI-compatible servers differ on streamed tool-call chunking
  (index vs id); the parser keys by `index` and tolerates missing ids. Tested
  against recorded chunk shapes, not every vendor.
* **R5** The node link has TLS since Phase 8, but the **web UI still does not**
  (`--insecure-bind` is plain HTTP). Remote browser access must go through a
  VPN/tailnet or a TLS-terminating proxy.
* **R6** Two-db embedded mode doubles disk usage on the master machine.
* **R7** `genai` has a single maintainer on crates.io. Mitigated by the exact pin
  and the `Provider` seam; a swap touches one file.
* **R8** Anthropic extended thinking + tool use requires replaying signed
  thinking blocks on the next request. Our history does not store them yet;
  untested against the real API (no key in this environment). Until fixed, do
  not enable `effort` for Anthropic models used with tools.
* **R9** `genai` enables serde_json `preserve_order` workspace-wide (feature
  unification). Harmless (map equality ignores order), but anything comparing
  JSON *text* must compare values instead; the fixture test does.
* **R10** `Temml.woff2` (shipped with Temml's `Temml-Local.css`, used for
  `\mathscr` and primes) is a modified KaTeX_Script font under the SIL OFL 1.1,
  but its own licence field says it was made with FontCreator's home edition
  and "may not be used for commercial purposes". Undecided: keep it, ask
  upstream, or drop the font (script capitals and primes then come from
  whatever font the browser falls back to). See THIRD_PARTY_NOTICES.md.
* **R11** Rich blocks are tested with the mock provider only: how often real
  models write valid `ui:` blocks, and follow "end your reply after it", is not
  measured.

---

## 8. Phases and gates

Revised 2026-11-02 for the v2 direction (§0). Every phase also checks the harness idle RSS
against the 50 MB soft budget.

* **Phase 0** (done): workspace, CI, this plan.
* **Phase 1** (done): master + embedded node, native loop, OpenAI-compatible streaming,
  tools with approvals, event log, coding SPA. **Gate:** SIGKILL mid-turn → restart → WS
  resume from cursor → identical log and fold.
* **Phase 2: Foundations** (backend). Additive event fields (`author`, agent snapshot,
  `agent_changed`, `profile`/`agent_id`/`incognito` on `session_created`, answer variants);
  `users` + roles + `owner_id`, Argon2 login, first-run claim code replacing the shared token;
  0600 secrets file; Anthropic API + OpenAI-compatible providers via a maintained crate if one
  fits, model capability registry; chat runtime on the master reusing `NodeRuntime` without
  filesystem tools; agent definitions + built-ins; `cargo-deny` in CI. **Gate:** Phase 1
  acceptance still green; tests for claim/login, schema round-trips, both providers against
  local mock servers.
* **Phase 3: UI shell** per the mockup: sidebar, greeting, bar (agent selector, model + effort,
  Search/Ask/Agentic, incognito, node picker, @mentions), chat view with a maintained
  sanitizing markdown renderer, tool cards and approvals; Search redirect. **Gate:** Playwright
  flows; first-paint and bundle-size budget.
* **Phase 4: Bar intelligence.** FTS5 similar-chats panel; in-browser mode classifier (rules
  cold start, Tab labels, accuracy logged). **Gate:** a repeated question surfaces the old chat.
* **Phase 5: Onboarding.** Claim → account → provider (test call, model list) → name,
  timezone, optional country/city, search engine → memory preset placeholder. **Gate:** fresh
  data dir → onboarding → first chat, in Playwright.
* **Phase 6: Chat capability.** Web search tool, document attachments, token meter, rolling
  history summary, incognito. **Gate:** incognito leaves no rows after TTL.
* **Phase 7: Memory service.** Evaluate OpenViking vs two others, `MemoryStore` adapter,
  managed/LAN/cloud config, benchmarked presets, Memory page, confirmed location proposals.
* **Phase 8: Remote nodes + orchestration.** `node join`, Ed25519 pairing, WSS, network-drop
  tests, remote cancel; `delegate` tool and the Ultracode agent with caps.
* **Phase 9: Later.** Parallel-answers UI + merge, voice, worktrees/checkpoints, MCP + skills,
  external harnesses (ACP), Projects, Library.

---

## Status

### 2026-11-02 — Phase 0 and Phase 1: done

**Done**
* Workspace (8 crates), CI (`fmt --check`, `clippy -D warnings`, `test`, web typecheck/test/build), `unsafe_code = "forbid"` workspace-wide.
* Event log tests written before the code that uses them (`crates/store/tests/event_log.rs`),
  including a 50-round lossy-link simulation (drops, duplicates, reordering) that must converge byte-identically.
* Native loop + OpenAI-compatible streaming provider (tested against a local SSE server) + mock provider.
* Tools: bash (process group, SIGTERM→SIGKILL, timeout, background children reaped, secret env vars removed),
  read, write, edit (exact match, uniqueness enforced), grep, glob; workspace confinement incl. symlink escapes.
* Approvals with node-local policy clamping; policy-denied tools; cancel during approval and during a running process.
* Master mirror with the replication rule, command routing with acks, token→cookie auth, Host/Origin checks, CSP/frame-deny.
* SPA: login, session list with node badges, streaming chat, tool cards (diff view for edits), approval prompts, Stop, drafts.

**Evidence**
* `cargo test --workspace`: 63 tests green; web: reducer fixture test green.
* Acceptance (`crates/cli/tests/acceptance_restart.rs`): SIGKILL mid-stream and mid-approval → restart → WS resume
  from cursor → client log == server log, folds equal, contiguous seqs, turn closed as `interrupted`. Passed 10/10 runs.
* `crates/master/tests/mirror.rs`: an empty and a stale mirror both catch up on attach via hello→resume→snapshot.
* Rust and TS reducers are pinned to one fixture (`web/src/__fixtures__/session.json`).
* Manual browser run (Playwright): approval → allow → output; kill -9 mid-stream → UI goes offline, reconnects,
  shows identical partial text plus "turn interrupted".

**Risks / known gaps**
* stdout/stderr are interleaved in `tool_output_delta` (always tagged `stdout`); the final output is correct.
* Each turn loads the session's whole history from SQLite; fine now, revisit with compaction (Phase 5).
* No OS sandbox for bash (R2; closed later by FIX_PLAN 2.7). The UI token is single-user (R5).
* Not tested against real vendors yet (no keys in CI); SSE parsing is tested against recorded chunk shapes (R4).

**Next:** Phase 2. Threat model T7/T8 first, then `rizzy-harness node join`, Ed25519 pairing, WSS bridge
onto the existing `NodeLink`, network-drop tests.

### 2026-11-02 — Phase 2 (foundations): done

**Done**
* Additive event schema: `author`, agent snapshot per turn, `agent_changed`,
  `profile`/`agent_id`/`incognito` on `session_created`, answer variants.
  v1 payloads round-trip byte-identically (test).
* Accounts: claim code, Argon2id login, hashed cookie sessions, `owner_id`,
  visibility enforced on REST and WS. API token kept for scripts.
* Providers through `genai` (Anthropic + OpenAI-compatible), `secrets.toml`,
  model registry with effort gating, `GET /api/models`.
* Agents: built-ins General/Coder/Writer/Scout, user overrides, agent tool
  lists enforced in the loop, `@mention` per turn, session agent switch,
  `GET /api/agents`, `POST /api/sessions/{id}/agent`.
* Chat runtime on the master; agentic sessions refuse cleanly with no worker.
* `cargo-deny` job.

**Evidence**
* `cargo test --workspace`: 86 passing; clippy `-D warnings`, fmt, web
  typecheck + fixture test green; `cargo deny check` green.
* Phase 1 acceptance (SIGKILL + resume) still green.
* Provider tests against local servers speaking both wire formats.
* Smoke on the real binary: both runtimes register; a chat session runs as
  Writer with the agent recorded on the turn.

**Risks**
* R8 (Anthropic thinking + tools replay) open; R7 (genai single maintainer).
* The web UI does not yet expose agents, profiles or models: that is Phase 3.
* Ultracode/orchestration agent deliberately not shipped until `delegate`
  exists (Phase 8), so no agent promises a tool it cannot have.

**Next:** Phase 3, the UI shell per the mockup.

### 2026-11-03 — Multi-account (admin-gated): done

**Done:** admin role from the claim; admin-only account creation with temporary
passwords and forced change; per-user privacy (admins too); per-user agentic grant;
admin API (`/api/users`, `/api/me/password`); last-admin/self guards; sign-out on
disable/delete/reset; login throttle; minimal Users panel, password screens and a
Chat/Agentic choice in the web UI. The schema is created complete.

**Evidence:** `cargo test --workspace` green incl. 5 account HTTP tests (forced
change, privacy, grant, disable/reset/delete, last-admin, throttle); Playwright on
the real binary: claim → admin creates bob → bob's forced password change → bob
sees none of the admin's chats, has no Users link, Agentic disabled with reason,
chat works.

**Risks:** username lockout by throttle (accepted, T12); no TLS for remote (R5).

**Next:** Phase 3 UI shell.

### 2026-11-03 — Phase 3 (UI shell): done

**Done**
* ChatGPT-style shell per the mockup: collapsible sidebar (drawer on mobile) with
  New chat, chat search (title filter), Recents with running dot and node badge,
  user menu → Settings / Log out.
* Home: localized time-of-day greeting with the user's name (stable per day) + the
  composer.
* Composer: agent selector, `@` mention popup (↑/↓/Enter/Esc) with a
  "→ @agent for this message" chip, model selector, effort only for models that
  declare it, Search / Ask / Agentic switch (Tab / Shift+Tab), node + folder row
  for Agentic (recent folders remembered), Agentic disabled with the reason.
  Search redirects to the per-user search URL (default DuckDuckGo).
* Conversation: header (title, agent, model, node, folder), markdown via
  react-markdown + remark-gfm (HTML escaped, safe links, copy buttons), agent
  labels on turns run by another agent, agent switches shown, tool cards and
  approvals carried over, Stop.
* Settings: Preferences (search URL, default agent/model), Password, Users (admins).
* Backend: `GET/PUT /api/me/settings`, per-message `effort` (gated).
* Hidden until they work: attachments, mic, Incognito. Classifier and
  similar-chats panel are Phase 4.

**Evidence**
* Vitest 12 (logic modules + reducer fixture); Rust 94; `cargo deny` green.
* Bundle budget enforced in CI (`npm run size`): initial 74.8 KB gzip (≤ 110),
  total 126.2 KB (≤ 300); conversation, markdown and settings load lazily.
* `web/e2e/phase3.cjs` against the real binary: claim → greeting → Ask streams →
  Tab to Agentic → approval → allow → output → `@writer` routes one turn (exact
  text asserted) → agent switch → Shift+Tab to Search redirects → admin creates
  bob → bob sees Agentic disabled and none of alex's chats → kill -9 mid-stream
  recovers → mobile layout. No page errors.
* Bugs found by the run and fixed: mention caret restored a frame late (first
  typed character landed at the end); "Log out" wrapping; mobile send button wrap.

**Risks / gaps**
* Session model cannot be changed mid-chat (pick at start; effort per message).
* Code blocks are not syntax-highlighted (weight); revisit with a lazy highlighter.
* The e2e script is run manually, not in CI (needs the built binary + Chromium).

**Next:** Phase 4, bar intelligence (FTS similar-chats panel, mode classifier).

### 2026-11-03 — Phase 4 (bar intelligence): done

**Done**
* FTS5 chat index on the mirror (title/body columns, filled in the append transaction,
  incognito never indexed, rebuildable from the log); queries built from quoted tokens so
  FTS5 syntax in user input is inert.
* "You may have asked this before" panel under the bar: >= 3 meaningful words, not
  time-sensitive (word list shared between Rust and web), >= 60% word coverage, at most 3,
  owner-only; ↓ moves into the list, Enter opens, Esc returns; never auto-opens.
* Sidebar search is full-text (messages, not just titles), with highlighted snippets.
* Mode prediction: rules (URL/short → Search, question/greeting → Ask, path/code/dev verb
  → Agentic, never Agentic when blocked) plus an online logistic-regression model
  (hashed features, 4096×3) that takes over only after 30 labels and only while more
  accurate than the rules on the same 100-message window. A manual choice sticks for the
  message; an "auto" marker shows when the prediction is in charge.
* Only weights and accuracy windows are stored (`/api/me/classifier`, validated); no
  typed text; Search-mode text never leaves the browser.

**Evidence**
* Rust 103 (store search, query building incl. injection inputs, suggestion rules,
  privacy, classifier validation); Vitest 18 (44-example rules table, model learns a
  user-specific split > 90%, earned switch, serialization).
* `web/e2e/phase4.cjs` and `phase3.cjs` green on the real binary; initial JS 77.6 KB gzip.
* Found by the Phase 3 re-run and fixed: "hello there" was predicted as Search
  (greetings/thanks now count as conversational).

**Risks**
* Suggestion quality is lexical (no synonyms); embeddings come with the memory service
  (Phase 7). The coverage threshold (0.6) is a guess to tune with real use.
* The classifier learns per browser session and saves debounced; two tabs typing at once
  can overwrite each other's update (last write wins; harmless but lossy).

**Next:** Phase 5, onboarding.


### 2026-11-03 — Phase 5 (onboarding): done

**Done**
* First run: claim code → admin account (Phase 2) → wizard. Admins on an unconfigured
  server get three steps (provider, about you, search engine); everyone else two.
  "Skip" keeps the built-in mock so the UI stays usable with no key.
* Provider setup without restart: Anthropic or any OpenAI-compatible endpoint (presets for
  OpenAI, DeepSeek, OpenRouter, Ollama, custom). "Test connection" calls `GET {base}/models`
  with the typed key; the admin picks which models to offer, a default, and which accept
  effort. Apply = validate → build → write `config.toml` → key into `secrets.toml` (0600,
  never in `config.toml`, never returned by the API) → hot-swap the provider behind both
  runtimes. The new key joins the shared redaction set at once, so tool output and logs
  scrub it without a restart.
* Profile: display name, IANA time zone (prefilled from the browser, validated server-side),
  optional country/city; the greeting uses the saved zone, not the browser's.
* Settings gains Profile / Preferences (search engine, default agent/model, accent colour) /
  AI provider (admin) tabs, sharing the form components with the wizard.

**Evidence**
* Rust 107 (fake OpenAI server: wrong key → 401 surfaced, model list, hot swap, persisted
  config, key file mode, runtime-added key scrubbed); Vitest 19.
* `web/e2e/phase5.cjs`: fresh data dir → claim → wizard with a fake provider (bad key error,
  model pick) → first chat answered by the fake provider's default model → key only in
  `secrets.toml` (0600) → search engine redirect → accent → a member gets two steps.
  `phase3.cjs`/`phase4.cjs` still green. Initial JS 78.0 KB gzip.
* Found by the e2e and fixed: the bar kept the model chosen before the provider switch
  ("mock"), so the first chat went to the demo; the bar now resets a model the server no
  longer offers. Checkbox rows in forms rendered as columns (CSS specificity).

**Not done (on purpose)**
* The memory preset step: there is no memory service until Phase 7, and a picker that
  changes nothing would be a lie. It lands with Phase 7.
* Location is never inferred; "learned later, with confirmation" is Phase 7 too.

**Risks**
* Provider apply is admin-only but affects every account at once; there is no per-user
  provider or key yet (teams/workspaces later).
* The wizard depends on `GET /models`; a gateway without it cannot be set up from the UI
  (only via `config.toml`). A typed-model fallback is a small follow-up if it bites.

**Next:** Phase 6, chat capability (web search tool, attachments, token meter, rolling
summary, incognito TTL).

### 2026-11-03 — Phase 6 (chat capability): done

**Done**
* **Web tools** (new crate `harness-web`), offered to chats when configured in `[web]`:
  `web_search` (self-hosted SearXNG, or Brave with a key) and `web_fetch`. `web_fetch`
  is the risky one, so: every hostname goes through a resolver that drops loopback,
  private, link-local, CGNAT/tailnet, NAT64 and other non-public addresses (redirects
  too; IP literals and `localhost` checked up front; env proxies ignored so they cannot
  bypass it), and a URL is fetched only if it appears in text from the user or a tool,
  never one the model composed (closes "fetch evil.com/?q=<your data>"). Pages become
  text (html2text), capped at 2 MB read / 20k characters.
* **Attachments**: `+` in the bar uploads text, code, Markdown, CSV, JSON or PDF (text
  layer). Text is extracted on the master (PDFs in the confined `pdf-text` helper
  process since FIX_PLAN 3.4, T23), capped at 100k characters, and travels *inside* the
  `user_message` event so the log alone rebuilds what the model saw. Uploads are owned,
  single-use, and expire unsent after the incognito TTL.
* **Token meter**: `turn_ended.usage` gains `context_tokens` (input of the turn's last
  model call); the conversation header shows total tokens and current context size.
* **Rolling summary**: once the history (since the last summary) is estimated above
  `[chat] summarize_after_tokens` (default 24k), everything before the last two turns is
  summarised by the same model into a `history_summarized` event; later turns send
  system prompt + summary + recent turns. Skipped when the old part is small (otherwise
  two huge turns would trigger a summary call on every turn). The UI shows the latest
  summary as a collapsible marker.
* **Incognito** (chat only): toggle in the bar; banner in the chat; hidden from recents,
  search and suggestions; deleted by the chat runtime *and* the master after
  `[chat] incognito_ttl_minutes` idle (default 24 h), or at once with "Delete now".
* **Deleting chats**: `DELETE /api/sessions/{id}` (trash icon). Deletes overwrite
  (`secure_delete`), merge the FTS index (`optimize`) and truncate the WAL; the mirror
  keeps a tombstone so a node that was offline cannot replay the chat back.
* **Context line**: each prompt carries the user's name, local date/time in their saved
  zone and optional city/country. Time zones are now validated against a bundled tz
  database (`jiff`); the Phase 5 check was only a character filter.

**Evidence**
* Rust 123. Gate test `incognito_leaves_no_rows_after_ttl`: after the TTL the chat is
  gone from both databases and a marker string is not found anywhere in the db/WAL
  files (the same check first proves it *does* find live text). Also: SSRF table
  (14 address classes), provenance, redirect-inward refused, attachment ownership and
  single use, summary reuse.
* `web/e2e/phase6.cjs` on the real binary with a fake SearXNG: search tool, refused local
  fetch, attachment text reaching the model, meter + summary, incognito hidden /
  unsearchable / deleted. Phases 3–5 e2e still green. Initial JS 79.1 KB gzip.
  Idle RSS (release) 17 MB.
* Found and fixed: debug info for ~30 test binaries filled the disk (18 GB); dev profile
  now keeps line tables only (1.6 GB).

**Risks**
* `web_fetch` provenance is lexical: a URL the user pasted can still carry data the user
  typed. Pages are untrusted text and can still steer the model inside the chat.
* No web search out of the box: SearXNG must be run (a "service = URL") or a Brave key
  given. Web settings need a restart (no UI yet).
* Summaries are written by the chat model itself and can drop details; the full log is
  kept and the summary is visible, but the model only sees the summary.
* Token estimates (4 chars/token) decide when to summarise; real counts differ by model.
* PDF parsing of hostile files runs in-process (blocking thread, size-capped); a
  subprocess sandbox would be stronger. *Done in FIX_PLAN 3.4 (T23).*

**Next:** Phase 7, memory.

### 2026-11-03 — Phase 7 (memory): done

**Evaluation (research agent, sources in its report; PyPI release data, repo files):**

| | OpenViking | mem0 server | Graphiti |
|---|---|---|---|
| Licence | AGPL-3.0 + README "no rewrite/port" clause | Apache-2.0 | Apache-2.0 |
| Latest | 0.5.3, 2026-11-02 (pre-1.0, churning) | 2.2.1, 2026-09-25 | 0.31.2, 2026-10-20 |
| Infra | 1 container, embedded storage | API + Postgres/pgvector | API + Neo4j/FalkorDB |
| Fits CRUD trait | poorly (context filesystem) | directly | partly (no list/update) |
| Own LLM needed | yes (VLM + embeddings) per write | no with `infer:false` (embeddings still) | yes |

**Decision:** the built-in store is the default; **mem0** is the optional external backend
(Apache-2.0, maps 1:1 onto our trait, `infer:false` means no second LLM bill or data
path). OpenViking was the first candidate and is rejected: a VLM call on every memory
write, a filesystem model we would have to emulate CRUD on, and a licence clause against
porting. mem0 notes: telemetry is on by default (`MEM0_TELEMETRY=false`), routes are
unversioned, pin the image.

**Done**
* Crate `harness-memory`: `MemoryStore` trait; built-in SQLite store in its own
  `memory.db` (FTS5; vectors from any OpenAI-compatible `/embeddings` endpoint, fused
  with keyword hits by reciprocal rank; vectors backfilled lazily per model; a failing
  endpoint degrades to keywords); `Mem0Memory` adapter (our status/kind/source/dedupe in
  `metadata`).
* Memories are **proposed, then confirmed**: chats get a `remember` tool whose call does
  nothing on the node; the master sees the mirrored call and files a proposal for the
  chat's owner (deduped by call id, so replays cannot duplicate). Only confirmed facts
  are recalled (top 6, into the system prompt with "may be outdated").
* Incognito chats get neither recall nor the tool (`AgentSpec.deny_tools`).
* Memory page (Settings → Memory): review proposals (Remember / Discard), add, edit,
  forget, link to the source chat; Home shows "N new memories to review".
* Admin recall settings, live: keywords only, or an embeddings endpoint with presets
  Fast / Balanced / Best (`all-minilm`, `nomic-embed-text`, `mxbai-embed-large`; sizes
  are download sizes). Saving makes one test embedding call first. Also an onboarding
  step for admins.

**Evidence**
* Rust 128: store CRUD, owner scoping, proposals not recalled, edit re-indexes, FTS
  syntax inert, dedupe, embeddings find "puppy"→"dog" where keywords cannot, dead
  endpoint falls back; mem0 adapter against a fake server (asserts `infer:false`, bad
  key fails); HTTP: propose → confirm → recalled into the next prompt, incognito
  neither reads nor writes, another user gets 404 on edit/delete.
* `web/e2e/phase7.cjs`: onboarding refuses a dead embeddings endpoint and saves a working
  one; model proposes; Home review link; unconfirmed not recalled; confirmed recalled by
  meaning; forget. Phases 3–6 e2e green. Initial JS 79.4 KB gzip.

**Not done / risks**
* The presets are **not benchmarked** here (no models in this environment): sizes are
  published model sizes, not measured RAM/latency. No reranker yet (the plan had one for
  Balanced/Best).
* The mem0 adapter was only tested against a fake built from mem0's source; check a real
  instance's `/docs` before relying on it. Switching backends needs a restart and does
  not migrate facts.
* Recall quality depends on the model writing good `remember` sentences; a model can
  still propose junk (the user filters it). Location facts are not copied into the
  profile's city/country automatically.
* Vector search is brute force over a user's facts: fine for thousands, not millions.

**Next:** Phase 8, remote nodes and orchestration.

### 2026-11-03 — Phase 8 (remote nodes + orchestration): done

**Done**
* **Node link** (new crate `harness-link`), on its own port (`serve --node-listen ADDR`)
  that speaks only the node protocol, so the UI can stay on loopback:
  - TLS with a self-signed master certificate. Nodes **pin its SHA-256 fingerprint** at
    join (no CA is trusted; a mis-issued certificate does not help an attacker, T8).
  - Each node has an **Ed25519 key** (aws-lc-rs, `keys/node.key`, 0600). Every
    connection answers a fresh challenge whose signed message includes the master's
    certificate fingerprint (channel binding: no replay to, or relay through, another
    endpoint).
  - **Join** with a single-use token an admin creates in Settings → Nodes (hashed,
    15 min). The UI prints the two commands (`node join … --token --fingerprint`,
    `node run`). A key cannot join twice; ids `local`/`master` are never handed out.
    Decision: the admin-issued token *is* the approval (the plan had a separate approve
    click); the node's key fingerprint is shown in the list.
  - The bridge accepts frames only after auth, requires the first one to be `hello`
    for the authenticated id, and closes on a second hello. Events for sessions owned
    by another node are still refused by the mirror (T7).
  - **Revoke** (UI) marks the key revoked and drops the live link; the node's reconnect
    is rejected and `node run` exits.
  - Node side: reconnect with backoff (250 ms → 30 s); each reconnect re-sends `hello`
    with heads, so the master resumes from its cursor.
* **Orchestration:** agents may list `delegates` (+ `max_subagents`,
  `max_subagent_tokens`). Such an agent gets a `delegate` tool; each call runs the
  specialist **in its own child session on the same node** (own log, own approvals,
  `parent_session` on `session_created`, owner inherited only from a parent on the same
  node). All delegations of one step run **in parallel**; caps are enforced per turn
  (default 4 sub-agents, 200k tokens) and shown in the tool description. Depth is one
  (sub-agents get no delegates). Cancelling the parent cancels its children. The child's
  final answer comes back as the tool result (Infinity's "report back as a tool
  result"); the tool card links to the child session as soon as it starts. Built-in
  **Ultracode** delegates to Coder, Scout and Writer. Sub-agent sessions are hidden from
  Recents and link back to their parent.

**Evidence**
* Rust 133. `remote_nodes.rs`: wrong fingerprint fails at TLS; tokens single use; keys
  cannot re-join; reserved ids; signing as another node is rejected; **a link cut
  mid-turn through a TCP proxy converges to identical node and mirror logs**; remote
  cancel; revoke disconnects. `orchestration.rs`: five delegations → four run and one hits
  the cap; children overlap in time; depth one; owner inheritance; cancel cascades.
* `web/e2e/phase8.cjs` with **two real processes**: join from the UI's command (a wrong
  fingerprint is refused), node online, an approved `bash` call writes a file on the
  remote node, Ultracode's two sub-agents in child sessions, revoke stops the node
  process. Phases 3–7 e2e green.

**Risks**
* The **web UI still has no TLS** (R5); only the node link does.
* A compromised master can still approve anything within node policy (T6/R1).
* Approvals for a sub-agent's tools appear in the child session; the parent card links
  there, but the user must open it. No cost cap in money, only tokens.
* Delegation runs on the parent's node only (no cross-node fan-out yet) and sub-agents
  share the workspace without worktrees, so parallel coders can collide on files.
* Remote nodes need their own provider config and key (`config.toml` on the node).
* No rate limit on failed node handshakes (each costs a signature check; audit logged).

**Next:** Phase 9 (later items).

### 2026-11-03 — Phase 9 (later items): partly done, the rest deliberately not

**Done**
* **Checkpoints + undo.** Agentic turns in a git repository get before/after snapshots:
  commits written through a *temporary* index (untracked files included, ignored files
  not) and kept under `refs/harness/<session>/<turn>/` (superseded 2026-11-10: they now
  live in a shadow repository, see §5 and T19). The user's index, HEAD and
  branches are never touched (asserted byte-for-byte in a test). `checkpoint` events
  carry the changed files; the UI shows "N files changed" under the turn. **Undo** puts
  back only the files that turn touched and refuses if any of them changed after the
  turn ("Undo anyway" forces it). New phase `restored`; the fold (Rust + TS) tracks
  checkpoints per turn.
* **Worktrees for sub-agents** (fixes the Phase 8 risk). A delegated sub-agent that can
  write gets a detached worktree inside the git dir, seeded with the current state
  (uncommitted edits included). When it ends, its diff is applied to the main folder,
  serialized per node, all or nothing; a conflicting result is kept under a ref and the
  tool result says how to apply it. Read-only sub-agents share the folder.
* **Parallel answers.** "compare: vs <model>" in the bar sends one prompt to two models
  at once (chat, no tools). The answers are variants of one group (now carrying their
  model), shown side by side; "Continue with this" records `variant_selected`, and later
  turns' history contains only the chosen answer (the first by default). Merging
  answers is not done.
* **Voice input.** A mic button records in the browser and sends audio to the server,
  which forwards it to an OpenAI-compatible `/audio/transcriptions` endpoint from
  `[voice]` (a local Whisper server, or a cloud API); audio is not stored. No browser
  speech API (Chrome's sends audio to Google). Shown only when configured.

**Not done, on purpose**
* **MCP and skills.** Running third-party MCP servers on a node is running arbitrary
  code with the node's privileges; it needs its own design first: a node.toml allowlist
  of servers (command, args, env from secrets), tools namespaced `mcp__<server>__<tool>`
  and treated as mutating (ask) by default, output scrubbed like other tools, per-server
  enable in the UI. Shipping a half-version would widen T4/T5 for little gain.
* **External harnesses over ACP** (Claude Code, Codex, OpenCode). The `AgentBackend`
  trait is ready for it, but none of those tools are installed here, so it could not be
  tested, and it is not shipped untested.
* **Projects, Library, group chats, voice output (TTS):** product features that need a
  design pass with the user; not started.

**Evidence**
* Rust 138: git snapshot/restore round trip (staged, untracked, ignored, deleted and
  created files; conflict refusal; index unchanged), undo over the protocol, worktree
  isolation with merge-back and a real conflict, compare + history selection,
  transcription forwarding (multipart, key, auth required, off until configured).
* `web/e2e/phase9.cjs`: dictation with Chromium's fake microphone through a fake
  transcription server; two answers side by side, the choice survives a reload; an
  approved `bash` turn changes two files, Undo restores them and `git status` is clean.
  Phases 3–8 e2e still green. Initial JS 80.1 KB gzip.

**Risks**
* Checkpoints add two `git add -A` passes per agentic turn: cheap with the copied stat
  cache, slow in huge repos with many untracked files. Very large untracked files are
  snapshotted too (they are not ignored); refs under `refs/harness/` are never pruned yet.
* Undo restores files, not side effects (commands that touched databases, networks,
  or files outside the repository).
* Applying sub-agent results is `git apply` without three-way merge: any overlap is a
  conflict that a human or the orchestrator resolves.

### 2026-11-03 — Follow-ups: WireGuard, checkpoints for every folder, "waiting for you"

**Remote access = WireGuard (decided).** `serve --bind <wg address>` needs no flag
when the address belongs to a WireGuard interface (kernel `DEVTYPE=wireguard`); any
other non-loopback address still needs `--insecure-bind`, and binding to all
interfaces is refused unless forced. **But WireGuard alone is not enough for the
browser:** plain HTTP on a non-localhost address is not a *secure context*, so
browsers block the microphone (dictation) and notifications. Measured with Chromium:
`http://<ip>` → `isSecureContext=false`, no `getUserMedia`; `--tls` → both available.
Hence `serve --tls`: HTTPS with a self-signed certificate for localhost, the bind
address, the hostname and `--allowed-host` names (regenerated when they change; trust
`keys/ui-cert.pem` once). Cookies get `Secure`. R5 is closed for WireGuard + `--tls`.
Not tested here: a real WireGuard interface (no kernel module in this container); the
uevent check and the refusal of a real non-WireGuard interface are tested.

**Checkpoints for every folder, latest 10.** Folders inside a repository use its git
(refs under `refs/harness/`, index/HEAD/branches untouched; superseded 2026-11-10:
every folder now uses a shadow repository, see §5 and T19). Other folders get a
private shadow repository in the node's data dir (`checkpoints/<hash>.git`; nothing
written into the folder; dependency/build dirs excluded). `node.toml
checkpoints_keep` (default 10, 0 = off) keeps only the latest N checkpoints per folder;
order is logged in the git dir because commit dates have one-second resolution (a
test caught pruning the wrong one). Shadow repos are gc'd right away. **Rewind** undoes a
turn and every later turn in one step; undo/rewind record the restored state; an
expired checkpoint says so. Snapshots give up after 60 s instead of hanging a turn.

**"Waiting for you".** The mirror tracks pending approvals and turns that ended with a
question (last answer ends in `?`), per user "seen" marks, and `/api/attention`.
Home shows these chats under the bar in yellow; as soon as the typed text has
similar-chat results, those replace the list. Opening a question marks it seen;
approvals stay until decided. Sub-agent approvals appear here too (fixes the Phase 8
risk). The tab title shows the count. Browser notifications (opt-in in Preferences)
fire when a chat starts waiting and you are not looking at it.

**Limits, stated plainly:** notifications work only while a tab is open (no Web Push
server) and only on localhost or `--tls`. "Asked a question" is a heuristic (trailing
`?`): it misses questions phrased without one and flags rhetorical ones.

**Evidence:** Rust 142 (shadow repo, keep-latest pruning, rewind; attention for
questions/approvals, seen, re-trigger). `web/e2e/attention.cjs` (yellow list,
notification recorded, replaced by search results while typing, cleared on open);
phases 3–9 e2e green.

### 2026-11-03 — Node setup over SSH (automatic and manual)

Settings → Nodes → *Set up a server over SSH*. New crate `harness-provision` (russh).

**Both modes start the same way.**
1. *Check the server's identity*: harness reads the SSH host key fingerprint without
   logging in; the user compares it with the provider's console and ticks "It
   matches". Every later connection refuses any other key (no silent TOFU: a
   password must not go to a machine in the middle).
2. The user's password or private key logs in **once**, installs harness's own
   ed25519 key (`keys/ssh/<host>_<port>`, 0600), and harness proves it on a **new**
   connection. From then on only harness's key is used; the password/key is dropped
   and never written (a test scans the whole data dir for it).

**Automatic** (fresh VPS / node-only): refuses servers that are not Debian/Ubuntu
with systemd, lack root/passwordless sudo, have a different CPU or older glibc than
this build, or already run ufw, firewalld, Docker or other firewall rules ("not a
fresh server, use manual setup"). Then: packages (`wireguard-tools`, `nftables`);
key-only SSH (a `10-rizzy-harness.conf` drop-in, which wins over cloud-init's `50-…`,
checked with `sshd -t`; the user's own public key is added first so they keep
access); firewall (own `inet rizzy_harness` nftables table: inbound only SSH and UDP
51820; outbound open for agents); WireGuard (node key generated on the node; master
peer only); then the user brings the master's side up once with root (shown
command, then *continue*); then harness installs itself as an unprivileged
`rizzy-harness` user with a hardened systemd unit, joins over the tunnel and waits for the
node to come online. **SSH and firewall changes arm a systemd timer that reverts them
after 120 s; they are confirmed only after a new key login works**, so a bad rule
cannot lock the user out (tested: on a lost connection the confirm never runs).

**Manual** (servers with other things on them): inspection is read-only; the only
change is adding harness's key. A checklist shows what is in place and, for the rest,
the exact commands (with the same revert timers and "log in from a new terminal,
then confirm" instructions). With an existing ufw/firewalld it gives rules for *their*
firewall instead of ours. *Check again* re-inspects with harness's key; a node
WireGuard key that appeared is registered with the master automatically.

**Decisions and pushback**
* nftables directly, not ufw or iptables: on current Debian/Ubuntu `iptables` is a
  shim over nftables and ufw sits on top; one own table does not fight other rules.
* Topology: each node listens on UDP 51820 and the master dials out
  (`PersistentKeepalive`), so a master at home behind NAT works. Creating the
  master's interface needs root once; harness does not run as root, so it prints
  `sudo wg-quick up …` (or `wg syncconf` for later nodes) instead.
* Copying the model provider's API key to a node is opt-in: nodes run model calls
  themselves. It lands in the node's `secrets.toml` (0600), not `config.toml`.
* russh without its `rsa` feature (RUSTSEC-2023-0071, unfixed); RSA-only private keys
  are not accepted; use the password once or an ed25519/ECDSA key.

**Evidence:** SSH client against an in-process russh server (probe, wrong pin, wrong
password, exec, key bootstrap); inspection script run on a real Ubuntu 24.04;
firewall rules validated with `nft -c`; master WireGuard keys equal `wg pubkey`;
runner tests (order, lockout → never confirmed, refusal of non-fresh servers, manual
checklist); HTTP test (no credential on disk, re-check with harness's key after
password login is turned off); `web/e2e/setup.cjs` in the browser.
Found and fixed on the way: checkpoint snapshots copied the git index with a fresh
mtime, defeating git's racy-clean check, so a same-size edit in the same second
could be missed by undo (reproduced 3/25, now 0/40).

**Not tested here, stated plainly:** the automatic steps on a real VPS (apt,
systemd timers, sshd reload, nftables, WireGuard), because this container has no
systemd, no WireGuard kernel module and no VPS. Real interop with OpenSSH servers was
not tested (a test that created a system user was blocked by the environment, rightly).
Run a first automatic setup on a throwaway VPS before trusting it with a real one.

### 2026-11-03 — Node setup: users, distros, container engine

**Users (who is who on a node).**
* `rizzy-harness-admin`: harness's own SSH login, its own ed25519 key, locked password.
  *Automatic* mode **requires the root login** (refused in the form, the API and again
  after login if uid ≠ 0). With root, harness creates `rizzy-harness-admin` with
  passwordless sudo for the setup, proves the key login, adds the user's own public
  key to root if given, and drops the root connection. Everything after that runs as
  `rizzy-harness-admin` via `sudo -n`. **At the end harness removes its own sudo** (opt-in
  to keep it): a master holding passwordless root on every node would turn a master
  compromise into root everywhere, which breaks T5. Re-running automatic setup then
  needs root again.
* *Manual* mode should get a **normal user**. Root is accepted with a warning. After the
  login, harness asks: *Create harness's own user* (creates `rizzy-harness-admin` **without**
  sudo, switches to it, removes its key from root) or *Keep using root* (flagged).
* `harness`: the unprivileged runtime user the node service runs as. Agents' commands
  run as it. It never has sudo, and it is a different user from `rizzy-harness-admin`. If
  the agent's user could sudo, a prompt injection would mean root.

**Distributions.** Fully supported: **Debian 13** and **Ubuntu 26.04 LTS** (constant
`MAIN_SUPPORT`; update when a new LTS ships). Other Debian/Ubuntu releases and
Debian-based systems (`ID_LIKE` debian/ubuntu: Mint, Raspberry Pi OS, …) run with a
warning ("works but is not tested"). Anything else is refused before any change.

**Container engine** (both modes): **Podman, rootless (default)**: `podman uidmap`,
a subuid/subgid range for `harness` (next free range computed from
`/etc/subuid`/`/etc/subgid`), `loginctl enable-linger`, checked with `podman info`
(Rootless = true); the unit gets `XDG_RUNTIME_DIR` and drops `NoNewPrivileges`
(newuidmap is setuid). **Docker**: distribution `docker.io`, `harness` joins the
`docker` group, checked with `docker info`. **Warning shown in the form and the
step: docker-group members are effectively root** (`docker run -v /:/host`), so an
injected agent can take the server over. **None** is also possible. The runtime user is
now a normal (non-system) user so it can get subordinate ids.

**Evidence:** distro tier and engine-readiness tests; scripts parse (`bash -n`) and
carry their guards; the subuid range pick and key removal ran against real files;
scripted-server tests for: root required, order (admin user → … → engine →
WireGuard → install → drop sudo), all post-bootstrap commands as `rizzy-harness-admin`,
Docker and kept-sudo flagged, distro tiers, manual root question (create: no sudo, key
off root, re-check as rizzy-harness-admin; keep: flagged); HTTP (automatic with non-root
refused, only offered answers accepted); `web/e2e/setup.cjs` (warnings, root
question, engine step). **Still not run on a real Debian 13 / Ubuntu 26.04 VPS**
(no systemd here): the apt, podman-rootless and docker steps are untested in reality.

### 2026-11-03 — Node setups after the fact; first real run on this machine

**Saved setups.** Every setup that got past the login is saved (kv `node_setup:<id>`):
host, port, pinned fingerprint, harness's login user, **mode**, engine, what was
applied (key-only SSH, firewall), what is watched, whether harness kept sudo, node id,
last check and problems. Never a password or key. Settings → Nodes → *Servers set up
over SSH* manages them.

* **Automatic mode has opt-outs** (both on by default): *key-only SSH* and *firewall*.
  Off means the step is skipped and shown as skipped, not silently missing.
* **Mode can be switched both ways at any time.** It only changes what harness may
  do later: *manual* = harness shows commands, never runs a change; *automatic* =
  harness runs the change itself, using its kept sudo or a **root login given for that
  one action** (used once, never stored; same rule as setup). Switching to automatic
  does not give harness root, by design.
* **Watchdog** (master, every 10 min, plus *Check now*): read-only. SSH: an SSH
  `none` auth probe lists the methods the server offers; `password` or
  `keyboard-interactive` = problem. Firewall: `sudo -n nft list table inet rizzy_harness`
  through a sudo rule that allows **only that command** (added for `rizzy-harness-admin` in
  both modes). It reports, it never repairs. Problems go to *Waiting for you* under the
  bar (admins only) and to a browser notification. The toggles switch the watch only.
* **Firewall off/on** button in automatic mode (dead-man revert when turning on).
  In manual mode the API refuses (409) and the UI shows the commands.
* **SSH protection is changed by hand only.** Pushback on "or via agentic chat": agents
  run as `harness` without root, deliberately, so a chat cannot do it; giving chats that
  power would let one injected prompt reopen password logins on every node. The UI
  shows the commands for both directions.

**First run on a real machine** (`HARNESS_LOCAL_TESTS=1 cargo test -p harness-provision
--test local_machine`, this VPS: Ubuntu 24.04, root, no systemd, no WireGuard module):
inspection facts; the nftables table loads, outbound still works, and it reverts cleanly;
the SSH drop-in passes `sshd -t`/`sshd -T` (password and kbd-interactive off, root
`prohibit-password`); `rizzy-harness-admin` with sudo, then sudo dropped, locked password; the
watch-only sudo rule allows `nft list table inet rizzy_harness` and nothing else; the russh
auth probe against the real OpenSSH matches the OpenSSH fingerprint and sees password on
and off; rootless Podman as `harness` (container uid 0 → a subuid on the host); Docker
group access.
**Not testable here:** systemd timers/units (dead-man revert, node service, linger),
WireGuard, apt on Debian 13/Ubuntu 26.04. These need a throwaway VPS.

**Evidence:** provision tests (opt-outs skip their steps), master HTTP tests (record
saved, commands listed, manual firewall 409, mode switch, per-action root required,
watchdog problem → attention, watch toggle clears it), `web/e2e/setup.cjs` (record
shown as manual, no firewall button, *Check now* reports password logins, problem under
the bar, watch toggle, switch to automatic asks for a root login).

### 2026-11-03 — Central server in Docker

**What runs in the container:** the master (web UI, chats, node setup, watchdog) and
nothing else. `HARNESS_NO_LOCAL_NODE=1` (`serve --no-local-node`) skips the embedded
agentic node; agentic sessions go to remote nodes, and without one the UI already says
"set one up". **Nodes are not containerised, by design**: a node is the host's
WireGuard peer, its firewall, its user's files and tools; in a container it would
either see nothing useful or need so many privileges that the container is a fiction.

**Image** (`Dockerfile`): web build (node 22) → `cargo build --release --locked`
(rust 1.99) → `debian:trixie-slim`, no apt (only the CA bundle is copied in), runs as
uid 10001, data in `/data` (0700), `STOPSIGNAL SIGINT` (serve shuts down on SIGINT;
PID 1 ignores SIGTERM without a handler). Debian 13 on purpose: the master uploads
**its own binary** to nodes, so its glibc must not be newer than theirs (checked by
the setup already). `REGISTRY` build arg for Docker Hub mirrors.

**Networking, two ways:**
* `compose.yaml` default, **host networking**: the master reaches nodes through the
  host's `rizzy-harness` and binds loopback / `10.77.0.1` exactly as on bare metal. The
  `sudo wg-quick up …` command printed during node setup is run on the host;
  `HARNESS_HOST_DATA_DIR` maps the printed path to the host's (`./data`).
* Own network (UI and chats only): inside the container `0.0.0.0` is allowed
  (`HARNESS_CONTAINER=1`, set by the image, never guessed), and exposure is decided by
  publishing; the docs say `-p 127.0.0.1:7777:7777`, never `0.0.0.0` (Docker's
  published ports bypass the host's ufw/iptables rules). **Guard:** if a host-only
  interface (`docker0`, `podman0`, `cni-podman0`) is visible, the container is on the
  host's network and `0.0.0.0` is refused; best effort, documented as such.
* `HARNESS_ALLOWED_HOSTS` (comma-separated) for a different published port.

**Evidence (run here, real Docker 29):** image built from this Dockerfile (163 MB; only
a CA-proxy shim added for this sandbox's egress, in a scratch copy); in bridge mode:
UI served, wrong Host → 403, runs as uid 10001, claim code via `docker exec`,
`web/e2e/container.cjs` (claim, onboarding, chat answered by the master, Agentic
disabled with "No worker node is online", only the `master` node listed), `docker stop`
exits 0 in 0.2 s, data kept across restarts; host networking: `0.0.0.0` refused
("docker0 is visible"), `127.0.0.1` served; `compose.yaml` as shipped: up, listening on
loopback, claim code, host data path set. Found and fixed: `HARNESS_NO_LOCAL_NODE=1` was
rejected by clap (bool flags only took true/false), and ANSI colours in `docker logs`.
CI builds the image and repeats the curl/claim/host-network checks. **Not tested:** a
node joining a containerised master over a real WireGuard tunnel (no WireGuard here).

### 2026-11-03 — Published image and releases

CI job `publish` runs only after `rust`, `deny`, `web` and `image` pass, on pushes to
`main` and `v*` tags. `main` → `ghcr.io/lyuksovannyy/rizzy-harness:edge` (+ `sha-…`);
tag `vX.Y.Z` → `:X.Y.Z`, `:X.Y`, `:latest`, and a GitHub Release with the Linux x86_64
binary (taken out of the image, so it is the same build) and its sha256. A tag that
differs from the Cargo.toml version fails the job. `GITHUB_TOKEN` only, no stored
secrets. amd64 only: the master uploads its own binary to nodes, and an arm64 build
under QEMU would cost ~an hour per run for no current user.

### 2026-11-05 — Token savings (every method switchable)

Research first (four named tools read from source, plus a search; details in the
session): **none of graphify, rtk, headroom or caveman is a dependency.** rtk has no
library API and part of its default build is BUSL-1.1; headroom's proxy has telemetry on
by default and would hold our API key; graphify is retrieval, not compression; caveman
measures 3% fewer output tokens than a plain "be concise" (35% for its terse level). The
biggest saving was missing entirely: **no prompt caching**, and memory recall was pasted
into the system prompt on every turn, which would have defeated any cache.

**Settings.** `TokenSavings` (`[savings]`, Settings → Token savings) is resolved per turn
into `Savings` (`for_turn(chat)`) and sent with `session.prompt` (absent = everything
off). Chat turns never get agentic methods and are capped at the `lite` style.
Sub-agents inherit the parent turn's settings. Per-turn context (memory recall, page
context) is a separate `context` field placed after the history, never in the system
prompt.

**Methods.** (1) Prompt caching: genai `CacheControl::Ephemeral` on the system prompt
(covers tools + system) and on the newest message; cache reads/writes are counted in
`Usage` and shown in the token meter's tooltip. (2) Observation masking (`mask.rs`):
large tool outputs beyond the newest `keep` are replaced by a note naming the call; the
hidden set only changes every `keep` outputs and is a pure function of the history, so
the cache breaks rarely and later turns mask identically. (3) Shell filter (`filter.rs`):
ANSI/OSC stripping, `\r` progress bars, build/download noise, passing test lines
(cargo, unittest, pytest, go, jest/vitest) → a count, repeat folding; failures, panics
and summaries kept; full output in a bounded in-memory store behind `tool_output`.
(4) Bounded reads/grep. (5) `web_fetch` readability (dom_smoothie, MIT) + Markdown
(htmd, Apache-2.0), raw text fallback when extraction finds < 200 chars. (6) Answer
styles (`style.rs`): `lite` is our text; `full`/`ultra` adapted from caveman with
attribution (THIRD_PARTY_NOTICES.md); the "write plain sentences for warnings,
irreversible actions, questions" rules are kept in every level. (7) `harness-codemap`:
tree-sitter tags for Rust/Python/JS/TS/Go → `outline` tool and a PageRank-ranked repo
map (built once per folder per hour, so it does not churn the cached prefix).
(8) graphify: `graph_query` shells out to the pinned `graphifyy==0.9.102` (installed by
`rizzy-harness graphify install` via uv or pipx, or the settings button for the built-in
node). It needs the node's own `graphify = true` (node-local policy stays
authoritative; the master toggle can only turn it off), writes its graph under the
node's data dir via `GRAPHIFY_OUT`, and gets the tool env with secrets removed.

**Real-model measurement** (`crates/agent-native/examples/savings_bench.rs`,
gemini-flash-latest via an OpenAI-compatible router, 2026-11-05): one bug-fix task (a
Python library with a wrong percentage, 40 unittest tests, padding code), 3 runs per
setting, solved = the test suite passes afterwards.

| setting | mean input tokens | mean output | solved |
|---|---|---|---|
| everything off | 89,729 | 838 | 3/3 |
| defaults | 38,910 (−57%) | 695 | 3/3 |
| defaults + repo map + `full` style | 24,978 (−72%) | 451 | 3/3 |

Honest limits: one task, one model, n=3, and high run-to-run variance (the "off" runs
ranged 40k–126k). The router reported no cached tokens, so prompt caching is not part of
these numbers (it is covered by the Anthropic wire test, not measured live). Treat the
direction as solid and the percentages as indicative.

### 2026-11-07 — Rich messages

Built in eleven steps (commits from `1d96c07`). Model output is untrusted, so the
renderer, not the model, decides what links, images, code, math and cards may do.
Everything rich comes from the reply text or from tool results already in the log:
no new event kinds, no new protocol command, no pending state across a restart.
Three additive optional fields carry it (`assistant_message.blocks`,
`user_message.reply`, `tool_call_finished.sources`); the Rust fold and `fold.ts`
copy them identically (shared fixture).

**What was built**
* **Markdown** (`web/src/md/`): hidden characters (bidi controls, zero-width, BOM,
  private use, tags, variation selectors and the other default-ignorable code points
  no script needs) stripped before parsing and again after it,
  since micromark decodes `&#x202E;`; links only for http(s), mailto and in-page
  fragments, always showing the real host in an isolated LTR `<bdi>`, with a
  full-URL confirm for odd URLs that neither the user nor a web result brought;
  images never load (a chip that follows the link rules); GitHub callouts; tables
  with sticky sortable headers and Copy CSV (formula cells defused); code with its
  language, Copy and collapse past 60 lines; mermaid as labelled source; Copy message.
* **Highlighting:** refractor (Prism grammars), 19 languages, each its own lazy chunk,
  run in a module worker once the message is final. A block gets 1,500 ms, then the
  worker is stopped and the block stays plain (Prism took 33 s on 48k chars of
  hostile tsx). Caps: 20,000 chars, 1,000 lines, 1,000 chars per line. A walker
  emits only `<span class>` and text.
* **Math:** `\( \)` and `$$` (never single `$`, so prices stay text), Temml in a lazy
  chunk with trust off and expansion/size limits, then a pass that keeps only MathML
  layout attributes and strips hidden characters. An inline formula paints only
  inside its own box (a negative `\kern` or `\smash`+`\raisebox` cannot reach a
  card's harness lines); display math scrolls in its own. Shown as source while streaming,
  on a TeX error, past 4,000 chars, and (step 11) for `\ref`/`\eqref`, which Temml
  leaves empty for a post-processor we do not run.
* **Citations:** web results are labelled `[S<n>]` from a session-wide book (shared
  with delegated sub-tasks) and recorded as `sources`. A label in a reply is a pill
  naming the host (`host +k` for a run), with title and URL on hover or focus, a
  "Sources (n)" footer, and muted text for a label with no source. Web calls are
  harness-worded rows ("Searched “q” · N results", "Read host") that open to
  host · title and to what the model read; a run of them folds into "Researched".
* **`ui:` blocks:** `choices`, `form`, `progress`, `chart`, validated and normalized
  once in Rust (`harness_core::ui`). The browser never parses model JSON: it draws
  the stored spec, as React text and SVG, in a "from <agent>" frame, from lazy
  chunks (`UiBlock`, `Chart`). Progress: within a turn the latest block of an id is
  the card and earlier ones are snapshot lines; a stopped turn stops the card; a
  message cut off inside a block shows a note, never half a JSON body.
* **Answers:** a question or form ends the turn; the answer is the next user message,
  checked by the node (below), shown as a reply bubble, and the card locks.
* **Memory proposals:** an accepted `remember` call is a note, "Suggested a memory …
  Review in Memory", instead of a JSON card.
* **Agent setting** `ui: rich | display | off` (frontmatter, default `rich`, part of
  `prompt_hash`), enforced in the agent loop; delegated sub-tasks get `display` at
  most. The block instructions (about 230 tokens) follow the level.
* **Attention:** a message with an open question marks the chat as waiting.
* **Keyboard and screen readers:** closing a link confirm or a citation card puts
  focus back on the link or pill; a card answered from the keyboard keeps focus on
  its body, a group named by its question or title; fields say they are required
  and wrong; sort arrows are not read; text colours (syntax, callouts, muted labels)
  are checked at 4.5:1 in `theme.test.ts`.

**Grammar of `ui:` fences** (`ui::scan`; the browser's `md/remark-ui.ts` marks the same)
* Scanned on the text as stored: secrets scrubbed, the hidden set stripped, CR and
  CRLF read as LF.
* Opener at column 0: three or more backticks or tildes, then exactly `ui:<type>`
  (`choices`, `form`, `progress`, `chart`) and optional spaces. Anything else after
  the run (a meta string, an unknown type) makes an ordinary fence.
* Ordinary fences (indent 0-3) are skipped to their closer, so ui fences inside them
  are not blocks; neither are indented, list or quote fences.
* Closer: indent 0-3, the same character, a run at least as long, then only spaces.
  An unterminated fence is no block and ends the scan.
* Body: the lines between, joined with `\n`, one JSON object, at most 16 KiB. At most
  8 blocks per message, at most one choices or form, and that only in a message that
  calls no tool ("a question must end the reply").
* A block is stored as `{ordinal, hash, type, id, spec | error}`; the hash is FNV-1a
  32 over the body. The browser shows a spec only when ordinal, hash and type all
  match the fence micromark found, so where the two parsers disagree (an HTML block
  swallowing a fence) it fails closed to code. Errors are harness text
  (`options[0].label: longer than 80 characters`), never model text, shown under the
  code as "Could not show this question: …".

**Block schemas in brief.** Every object rejects unknown keys; ids match
`^[a-z][a-z0-9_-]{0,31}$`; every string goes through `clean_text` and is measured in
characters; defaults are written out in the normalized spec.

| Block | Keys | Rules |
|---|---|---|
| `ui:choices` (input) | `id`, `question`, `options`, `multi`, `other`, `min`, `max` | question ≤200; 2-8 options, each `{id?, label ≤80, description? ≤200}` or a label string (ids `o1`… when missing), unique; `min`/`max` only with `multi`; `other` adds a typed answer ≤500 that counts toward both; a question or option that asks for a secret is an error |
| `ui:form` (input) | `id`, `title`, `description`, `fields` | title ≤120, description ≤400 (either asking for a secret is an error); 1-12 fields of `id`, `label` ≤120, `type`, `required`, `help` ≤200, and per type: `text` (`multiline`, `max_length` 1-4000, default 500, `placeholder` ≤100, never a default), `number` (`min`, `max`, `step`, `default` on the grid, `unit` ≤16), `choice` (2-20 options, `multi`, `default`, `min`/`max`), `toggle` (`default`, never required), `date` (real `YYYY-MM-DD`, min ≤ default ≤ max). A field whose id, label, help, placeholder, unit or options ask for a secret is kept only as `{id, label, type, refused}`: a warning, never an input |
| `ui:progress` (display) | `id`, `title`, `status`, `value`, `steps` | title ≤120; status running, done or failed; value 0-1; 1-20 steps of `label` ≤120, `status` (pending, active, done, failed, skipped), `detail` ≤200 |
| `ui:chart` (display) | `id`, `kind`, `title`, `x`, `y`, `series`, `stacked` | bar or line; title ≤120; 1-100 x values, all strings ≤40 or all numbers; y label ≤60, unit ≤12; 1-6 series, name ≤40, one number or null per x value; `stacked` only for bars |

`off` makes no blocks (fences stay code); `display` turns choices and forms into
the error "interactive blocks are off for this agent".

**How answers return to the model**
1. The card posts `{"text": note, "reply": {"message_id", "block_id", "values"}}` to
   the existing owner-checked `POST /api/sessions/{id}/prompt`. Values: choices
   `{"selected": [ids], "other"?: text}`; forms `{field id: string | number | boolean
   | [ids]}`. 400 for a reply with attachments or compare, or over 32 KiB; empty text
   is allowed only with a reply.
2. The master runs it as the agent that asked (the message's author), never reroutes
   by @mention, and recalls memory from the note alone. `Prompt.reply` is additive;
   the protocol stays v1.
3. The node reserves the turn slot, then `ui::check_reply` checks the reply against
   its own log: the message exists, no user message follows it, it is the chosen
   parallel answer, the session is no sub-task, the block is a shown choices or form,
   and the values pass every rule (types, required, ranges and step grid, offered
   ids, min/max, `other`, real dates, nothing for a refused field). A failure frees
   the slot and returns 409 with the reason; holding the slot means nothing is
   appended between the check and the turn.
4. The recorded text, which the model sees, is the canonical answer plus the note:
   only ids, harness-written numbers, dates and yes/no, and the user's typing
   JSON-quoted, never a model label (`Choice answer · colour: blue`; `Form answer ·
   trip`, `- people: 3`, `- dest: lis`, `- not answered: notes`, `- not shown (asks
   for a secret): key`).
5. A reload folds the same log: answered cards stay locked with their values, and
   other tabs lock over the WebSocket. Approvals are untouched: replies never answer
   one.

**Security rules (in code, not in the prompt)**
1. No model HTML, script or iframe: raw HTML stays escaped; blocks are Rust-validated
   JSON drawn as text nodes; highlighting goes through the span-only walker; Temml
   runs with trust off and a clean pass; charts are our SVG. The CSP is unchanged
   (Temml's CSS and font come from 'self').
2. Model strings never become user-role text, so a form cannot launder a model URL
   into the web_fetch provenance set (an IPv4 shorthand such as "1.5" no longer
   counts as an address either).
3. Phishing: no password or hidden field, no model-chosen button label (always
   "Send"), no text defaults; secret-asking questions and fields refused in Rust, with
   the lexicon read as a person sees it (NFKD base letters, a look-alike table, any
   other non-ASCII letter in a word with ASCII letters read as any letter, every
   default-ignorable code point dropped);
   harness chrome on every card ("Question from <agent>", "Sends your answers as a
   message. Never type passwords or keys here.", "Asked after reading: hosts" after
   web content); every answer is a real click, and keys 1-8 only move between options.
4. Links and images: protocol allowlist; the real host always visible; a confirm with
   the full URL for a URL outside provenance (user text and web results only) with a
   query, fragment, long path, `;param`, token-like run, long or many host labels,
   punycode, credentials or an odd mail address (a mail domain shows as punycode
   too); `noopener noreferrer nofollow` and
   no referrer; images never fetched; no favicons, previews or unfurls.
5. Citations resolve only to sources our SSRF-safe web tools recorded; titles are
   cleaned text; a fragment counts only exactly.
6. Hidden text is stripped in Markdown, math output and `clean_text`; the memory note
   shows such characters as codes, since they are what would be remembered.
7. The node is the authority on answers; `ui/form.ts` mirrors the rules for inline
   errors only and is held to Rust by `ui.json`.
8. Limits: body 16 KiB, 8 blocks and 1 input per message, 8 options (20 in a form),
   12 fields, 20 steps, 6 series × 100 points, TeX 4,000 chars, the highlighting caps
   and budget above, replies 32 KiB.
9. Blocks come from the scrubbed text, and whole payloads are scrubbed on append.

**Threat notes (T14).** Injected web or repo content can steer the model into
asking: the card names the hosts read since the user's message and secret questions
are refused, but a model can still ask in prose, as before. A block cannot pose as the
harness: every one, snapshot lines included, sits in a "from <agent>" frame, and
approvals live only in tool cards. Exfiltration needs a click on a link whose real
host is shown; the confirm is UX, not a barrier, and a user who presses Open sends
what the URL carries. Hostile input cannot freeze the tab (worker budget, TeX limits,
block caps). A stale or replayed answer is refused against the log; an answer runs as
the agent that asked, at its ui level.

**Out of scope, and why**
* Sandboxed HTML/JS widgets, artifacts, MCP Apps: they need a second origin (today one
  port, `frame-ancestors 'none'`, no frame-src) and are the largest phishing surface.
  (The **Artifacts** area of 2026-11-08 is not these: it stores files agents publish and
  serves them as downloads; nothing in it is ever rendered or run.)
* Mermaid: 194-251 KB gzip even lazy, against the remaining budget; shown as source.
* A blocking mid-turn `ask_user` tool, tool-driven cards: new kinds, a route, restart
  recovery and about 1k more prompt tokens; forms that end the turn cover both modes.
* Reasoning summaries and provider-native citations: provider streaming plumbing of
  their own; `[S<n>]` works for every provider and the mock.
* Model buttons that call tools, suggestion chips: they bypass the model and
  approvals, or turn model text into user text in one click.
* Remote images, an image proxy, favicons, unfurls: the exfiltration channel, plus
  SSRF and tracking.
* remend streaming repair, single-dollar math, a `ui:table` block, highlight.js,
  Shiki, KaTeX, a WYSIWYG composer: cosmetic, ambiguous with prices, a duplicate of
  GFM tables, or larger than what was chosen.
* Branching, edit, regenerate, read aloud; weather, maps, image generation; tool
  progress events; an auto-repair loop for invalid blocks; more chart kinds; inline
  Keep/Discard for memories: separate features, third-party data, model cost, or a
  new API.

**Measured bundle sizes** (`npm run size`, gzip, after this step): initial JS
86.1 KB (budget 110; 86.2 before rich messages), total JS 267.5 KB (budget 300;
153.1 before, about 262 projected). Every chat loads the Conversation chunk, 64.3 KB
with remark-math and the md plugins. On demand only: math (Temml) 58.7 KB, the
highlighter worker 9.6 KB plus 20.5 KB of grammars in 25 chunks, UiBlock 4.2 KB, the
form rules 3.2 KB, Chart 4.1 KB. Outside the JS budget: the math stylesheet 1.8 KB
and Temml.woff2 9.2 KB. `size.mjs` prints every
file and fails if KaTeX or mermaid reach any chunk, or Temml or the highlighter reach
the initial set or the Conversation chunk; each marker must also match where its
library lives, so a renamed string cannot turn a check into a pass. (The markers are
`mermaidAPI`, since our own code names mermaid fences, and `Unknown language` /
`languages.extend`, since refractor's build has no `Prism.languages`.)

**Deviations from the plan, as built:** a refused field is named in the answer
text, so the model does not ask again in other words; "Other" counts toward a
choice's minimum and maximum; a question must end the reply; answers run as the
asking agent; the secret lexicon also catches "key" and "token" alone, look-alike
spellings, units and option text; snapshot lines have a compact "from" frame;
unclosed fences in finished messages are "cut off" notes; the 32 KiB cap is also
checked in the browser; keys 1-8 never send.

**Tests.** Rust: scanner, `clean_text`, every schema and limit, levels, the secret
lexicon, `check_reply` and canonical goldens, fold and protocol round trips, node
session (stale, running turn, slot freed on error), master HTTP (200/400/404/409),
store attention; `ui.json` is shared with vitest, which also covers the Markdown
pipeline, links, citations, highlighting (fake worker), math (real Temml under
happy-dom), cards, charts and themes. `web/e2e/rich.cjs` (sections in
`web/e2e/rich/`) checks each item of the plan's e2e list once, in about 40 s. In this
step it was mutation-checked: removing any of fourteen features one at a time (the
image chip, the link confirm, math and highlighting waiting for the final message,
the highlight budget, the streaming skeleton, the lazy chart chunk, hidden-character
stripping before and after parsing, the `\eqref` fallback, the memory note, the
refused field, the stale-answer check, the secret lexicon) made it fail. Open: R10
(the Temml font's licence) and R11 (no real model run).

### 2026-11-07 — Calm visual system: themes and the minimal restyle

The first part of the motion pass (the "Calm" spec, sections 1 and 2): tokens, a
theme preference, and the restyle. The per-mode home backgrounds come next and
plug into the slot left for them.

**What was built.** `styles.css` starts with the dark tokens (still the first
`:root` block), then light from the OS unless dark is forced
(`:root:not([data-theme="dark"])` inside the media query), then the same light
block for a forced light theme, then the scales that do not change with the theme
(type 11-30 px, a 4-based spacing scale, radii xs-2xl and full, durations and
easings) and the accent selection: `data-accent` only points `--accent` at an entry
of a per-theme ramp, so every accent has a light and a dark value. The theme
preference (`theme`: `system`, `light`, `dark`; the web sends null for system) is
validated in `prefs.rs` like the accent. `public/theme.js` (a file, since the CSP
allows scripts from this origin only) applies the localStorage mirrors before the
first paint; `lib/appearance.ts` sets `<html>`, the mirrors and the browser's
theme colour, and cross-fades a theme change with a view transition unless motion
is reduced. Settings → Preferences opens with Appearance: Theme (System, Light,
Dark) and the accent swatches; a pick previews at once, Save keeps it, leaving the
tab without saving goes back. The restyle keeps every class, label and ARIA
attribute: one control height and radius, filled inputs with an accent focus
border, a sidebar told apart by tone alone, an opaque composer with the only
raised shadow, popovers on their own surface that rise in, a mode switch whose
thumb slides to the chosen mode (measured in Composer; no slide on first paint),
tabs with a growing bar, tables with horizontal rules in a framed scroller, and
cards, code and tools as flat tone with a hairline. The greeting rises in; the
composer does not move.

**Slot for the backgrounds.** `bg/Backdrop.tsx` renders `.backdrop[data-mode]`
behind the home column (`.home` is a stacking context; the column sits above).
Composer reports each mode change with who made it (`onModeChange(mode, by)`), and
Home follows a user's pick at once and the classifier's only after it has held for
700 ms. The base `.backdrop` rules (contain, pause, hidden under forced colours,
more contrast and print) and the art tokens (`--art-max`, `--art-peak`, `--glow`,
`--mag-*`) are in place; theme.test.ts checks the art contrast budget.

**Deviations.** Shell applies the theme only once the preferences have loaded (the
spec's effect would clear the pre-painted theme for a moment). A preview does not
write the mirrors, so a reload before Save comes back to what is saved. The reduced-motion overrides for the
greeting and the running dot sit next to their rules, since an earlier rule of the
same specificity would lose. `select.pill` sizes to the chosen option
(`field-sizing: content`). The ETag for files outside `assets/` is left for the
backgrounds step, where the wall data makes it matter.

**Measured bundle sizes** (`npm run size`, gzip): initial JS 86.8 KB (86.1 before),
total 269.2 KB (267.7 before); the main stylesheet is 10.3 KB.

**Tests.** theme.test.ts: the two light blocks are equal declaration for
declaration; `--fg`, `--fg-2` and the status colours on nine surfaces; each of the
six accents as text on three surfaces and on its 9% and 16% tints, under
`--accent-fg`, and as a focus ring; syntax, chart and callout pairs as before; the
art budget. Vitest: `appearance` (attributes, mirrors, preview, theme colour), and
Composer's mode attribution and thumb. Rust: the theme list, prefs
without a theme loading with the defaults, typos still rejected, and the settings API accepting dark and
system and refusing others. e2e (phase5): preview, revert on leaving, Save, the
mirror, no flash (a new page in a light OS with the settings request held back is
dark from the start) and System handing the theme back to the OS.

### 2026-11-07 — Calm visual system: the mode backgrounds

The second part of the motion pass (the "Calm" spec, sections 3 and 4): each
mode in the home bar gets its own faint background.

**What was built.** `bg/Backdrop.tsx` sits behind the home column and keeps one
layer per mode: on a change the old layer plays its exit while the new one enters
140 ms later, at most two layers live at a time, and one cut short mid-entrance
just fades. A new chat (a new seed from Shell, also when New chat is pressed on
Home) re-rolls the wall and the field the same way. Zones (the column grown by
24 px, the top bar) are measured with a ResizeObserver, once per frame at most;
nothing is ever drawn inside them. Search (`SearchBg`, CSS only) drops a 2.5D
magnifier in from above the top edge: ten copies of the glyph 3 px apart in a 3D
context make a real side face; it overshoots, settles, then floats (7 s) and tilts
(11 s) while a glint crosses the lens every 9 s and the lens warms while the
composer has text. Ask (`WallBg`) lays out 8-64 queries in five depth tiers
(three on phones) with Mitchell's best candidate, seeded per visit, rippling out
from the composer; every 2.6 ± 0.8 s one line older than 8 s fades and another
takes a free spot, one in three mid-tier replacements typed out; the near tiers
drift against the mouse. The text is CSS `content: attr(data-text)`, so it stays
out of the accessibility tree, find-in-page and copy. The lists are fetched only
when the wall shows, once per page load. Topics are inferred in the browser from
the sidebar's titles (`bg/topics.ts`; a topic needs two titles; recent chats count
more), and their share grows from 0 at two matched titles to 60% at thirty
(`bg/mix.ts`); a bundled line equal to one of the user's titles is skipped.
Agentic (`BinaryBg`) draws a seeded field of 0s and 1s on a canvas from a glyph
atlas, redrawing only changed cells: 1.2% flip per second, up to three streaks
fall like rain, and a soft light follows the mouse as a porthole onto a brighter
copy, moved by transforms only. The clock (`bg/loop.ts`) asks for a frame only
when a tick is due: 30 a second while the light moves, 15 while it rests. Under
"Still", reduced motion or data saving, everything renders its rest frame and
changes cross-fade; a hidden tab pauses all of it. Settings → Preferences →
Appearance adds **Home background** (Animated, Still, Off; mirrored in
localStorage so a reload never starts what was turned off) and **Also in chats**
(`ChatGutter`: a still, half-strength wall or field in gutters of 180 px or more).
`assets.rs` now sends an ETag for files outside `assets/` and answers 304, so the
wall lists, `theme.js` and `cities.json` are not downloaded again unchanged.

**Deviations.** The Backdrop host is a lazy chunk too (1.6 KB), loaded only when
a background is on, which keeps the first load at 87 KB instead of 89. Backdrop
finds the column through the DOM: a later sibling's ref is not attached yet when
its layout effect runs. Exits are timed by Backdrop, so `onExited` is gone from
the contract. Hooks live in `bg/hooks.ts`, not `lib/appearance.ts`. Placement
checks collisions directly (64 lines at most) instead of a spatial hash. Lines
over 44 characters are dropped from the lists rather than cut. Topic phrases go
up to three words ("two weeks notice"). The field's noise is stretched so its
density clouds read. The magnifier's shadow breathes on a pseudo-element: two
animations of one property on one element kept Chrome from compositing it.

**Measured** (headless Chromium, no GPU, 1280×860): Agentic tick 0.2 ms on
average (budget 2); main thread about 29 ms/s with the light resting, most of it
Chrome copying the software canvas to the compositor (about 0.9 ms a frame, which
a GPU-backed canvas avoids); Search idle 3 ms/s and Ask idle 1.2 ms/s against
about 1.2 ms/s for the page alone. Bundle: initial JS 87.4 KB (86.8 before),
total 286 KB (269 before), with the host 1.6, Search 1.3, Ask 2.0 + 5.0 shared,
Agentic 1.8 + 2.0 shared and the gutter 1.1 KB, all lazy (size.mjs checks).

**Tests.** Vitest: rng, placement (no overlaps with margins, clear of zones,
bounds, determinism, 80% moved by a new seed, the ripple, a full zone, 64 lines
under 5 ms), the magnifier's branches, topics, mixing (share, quotas, never a
title, no duplicates), the list parser and fetch-once, the field (zone mask, flip
rate ± 10%, streak limits), the Backdrop layer machine and the mirror. Rust: the
backdrop list and the prefs defaults, the settings API, the ETag. e2e (`backdrop.cjs`):
each mode's art, aria-hidden, nothing under the column or composer, one fetch,
the 700 ms debounce, a new wall per new chat, never the user's title, pause when
hidden, contrast over every background in both themes, no light on touch,
reduced motion, Off and Still, and the gutter only beside wide chats.

### 2026-11-07 — Several providers, model parameters, per-agent defaults

`config.toml` holds `[[providers]]` (id = model prefix, preset, kind,
base_url, key reference, models) plus `default_model` and `[agent_models]`;
an agent's entry is `{ model, fallbacks }`. Memory embeddings come from a
provider or from Ollama. The router (`agent-native/src/llm.rs`) resolves
`<prefix>::<raw>` and strips the prefix; an id without a known prefix is an
unknown model. It is swapped whole on any change.

Presets live in `master/src/presets.rs`; fixed ones pin their URL (a client URL
is ignored). Keys are stored per provider as `provider-<id>`, reused for a test
or a re-save only when the base URL is unchanged, and deleted with the provider.
`/models` metadata is parsed from what each provider reports (OpenRouter,
Anthropic with paging, Mistral, Groq, Together, Gemini, vLLM field names), plus
Ollama's `/api/show` (four at a time, 20 s in all); numbers are clamped and names
cleaned (bodies capped at 8 MiB, `/api/show` at 256 KiB, 5,000 models), and
redirects are followed only within the same origin, since reqwest keeps
`x-api-key` on cross-origin hops. Anthropic models never get effort (R8): the
model list and the effort check both enforce it, whatever the file says.
Plain http is for `localhost`, `127.0.0.1` and `[::1]` only, checked on the
parsed URL; a URL with userinfo is refused.

Config changes (providers, agent defaults, memory, savings) take one edit lock
from reading the config to saving it. The memory save tests its embedding
first, then saves onto the config as it is now and answers 409 when the
memory settings or the provider it relied on changed meanwhile. A new key is
written to `secrets.toml` before the save that uses it and restored when that
save fails. A preset never replaces another provider holding its id (409);
custom entries may switch API kind.

The admin's per-agent default applies when a session is created without a
model, or with the model the defaults would give it anyway (clients always
send the model they show); such a session is marked in the mirror
(`session-model-auto:<sid>`) and follows the current defaults on every turn and
agent switch, so the node's startup `default_model` is only a last resort. A provider's default `effort`
now goes on the turn's spec from the master instead of the node's config.

**Deviations.** `custom_openai` needs no key (local servers often take none).
Deleting the last provider brings the demo provider back. The ids `demo` and
`test` are reserved (their paths are actions); PUT on them answers 400 and
DELETE removes a provider that has one.

Favicons (`/api/favicon`): https on every hop, redirects included; one user
runs at most two uncached fetches, a free slot is waited for at most 2 s (503,
not cached), and misses never evict cached icons.

### 2026-11-08 — Workspaces, roles and four settings tiers; pnpm

**Done**
* Every account has a Personal workspace, created with the account; the master
  also creates missing ones when it opens its store, so existing data dirs pick
  them up (`ensure_personal_workspaces`). Team workspaces are created from the
  sidebar switcher (`[workspaces] members_can_create`; server admins always) and
  members are added by username.
* Roles are fixed sets of permissions (`agentic`, `settings`, `members`): owner,
  admin, member, guest. Code checks permissions; only owners delete a workspace or
  make and remove owners. Nobody assigns a role above their own or changes their
  own role. While a workspace has other members its last enabled owner stays,
  checked inside the store transaction, and role writes only apply to the role
  the guard saw. The last member leaving deletes the workspace.
* Chats stay private to their owner in every workspace. A chat's workspace is a
  master-side mapping (`session_workspaces`, a new table: the `sessions` schema
  shared by master.db, chat.db and node.db is unchanged), ignored once the owner
  is no longer a member, else the owner's Personal workspace. Leaving, removal and
  deletion therefore return chats to Personal without touching them. Sub-agent
  sessions inherit the mapping from the same node only (T7).
* Settings in four sections, `#/settings/<section>/<tab>`: Account; Workspace
  (everyone in it: role, members, effective defaults, "use my memory here",
  leave); Workspace admin (name, default agent, per-agent model chains with
  General's as the default model, members, delete); Server (everything
  server-wide plus every workspace with owner recovery and deletion). A tab the
  viewer may not see falls back to the first one; before, members could open
  admin panels by URL.
* One `Resolver` (server config plus the workspace overlay) drives create,
  prompt, agent switches, `session-model-auto`, `/api/models`, `/api/agents` and
  notices; `?scope=server` gives the server's own values.
* Agentic work needs the account grant AND the workspace role, checked at
  create and before every prompt, allowing approval and restore (this also closes
  the gap where revoking the grant left existing agentic chats usable). Losing
  either cancels the affected running agentic turns; Deny and cancel always work,
  and the chat shows why it is read-only.
* `remember` is denied on delegates too (incognito and memory-off workspaces).
* pnpm 12 for `web/` (see §5).

**Evidence**
* `cargo test --workspace` 377 passed (store 13 new tests, master
  `workspaces_http.rs` and `workspaces_resolve_http.rs`); clippy and rustfmt
  clean. Vitest 553 passed (59 files). `pnpm size`: initial 96.1 KB gzip (≤ 110),
  total 304.9 KB (budget raised 300 → 310 for the switcher and the workspace
  panels).
* Browser walkthrough on the real binary (scratch data dir, demo provider):
  claim → alex creates Acme from the switcher → a chat in Acme stays out of
  Personal; switching home and following the chat's link switches back → Members:
  add bob, own row locked, bob as Guest → bob sees only his own chats, Agentic is
  off with the role's reason, his agentic chat is read-only, his Settings has
  only Account and Workspace, `#/settings/server/users` falls back to Profile;
  API: bob gets 404 on alex's chat, no search hits, 403 on settings and admin
  routes, 409 changing his own role. The default data dir started cleanly and
  got its Personal workspace.
* Bugs found by the walkthrough and fixed: the switcher never rendered (the web
  read the active workspace inside `user`, `/api/me` sends it beside it); the
  live connection died after a switch (an inline `onLogout` re-ran Shell's data
  effect, whose cleanup closed `Live` for good).
* Three design reviews (security, feasibility, product) before coding and a
  four-lens review with adversarial verification after it (14 of 17 findings
  confirmed, all fixed: atomic username-lookup throttle, role writes conditional
  on the checked role, a tab keeping its own workspace when another tab switches,
  stale `blocked` after a role change, panel saves re-reading the settings,
  notice attribution, docs).

**Deviations from the first design.** Custom and editable roles, per-workspace
node access, a per-workspace token-savings override and moving chats between
workspaces were dropped: node access and savings could be bypassed through
Personal, and savings is the server owner's cost control.

**Risks / gaps**
* Workspace managers learn whether a username exists (throttled, T12); members
  are added without an invitation step.
* Owner and workspace mapping live only in master.db, like `owner_id` before.
* The login throttle keeps its check-then-record pattern (pre-existing).

**Next:** shared chats and shared memory inside a workspace; custom roles;
per-user node access.

### 2026-11-08 — Sidebar areas: routines, media, artifacts, search

Built by four implementers in parallel on one frozen contract (JSON shapes, SQL, Live topics,
protocol messages, the `publish` description and every cross-area signature), in a scaffold
phase and three merged phases. Every dependency and every seam one area needed in another's
file landed first, so the phases merged without touching shared lines.

**Done**
* **Sidebar** (`Sidebar.tsx`): New chat, Agents, Routines, Media, Artifacts (each while
  `/api/features?workspace=` says `show` for this viewer, with `why` for empty pages),
  Connections as a disabled "Later" entry, the Projects and Recent sections (open or closed as
  left), the account button; a 56px rail when collapsed, a drawer at 760px and below. Every row
  is a link with `aria-current`.
* **Account menu** (lazy chunk, preloaded on pointer, focus or touch): Profile, Settings, Log
  out and the Workspaces submenu that replaced the switcher (Personal first, team workspaces
  with role and badge, "+ New workspace…"), as an APG menu button.
* **Search panel** (lazy chunk, Ctrl/Cmd K or "/"): `GET /api/search?in=all|chats|projects|
  media|artifacts` with one provider per area behind one `Provider` signature, FTS syntax
  inert, tiers, an All merge, cursors, three-line rows (title, first sentence of the latest
  message, an excerpt only when needed) and same-origin thumbnails only (`search::checked`).
  Chats are the caller's own for everyone (T10); routine run chats are found by a query but not
  listed empty; incognito and sub-agent chats never.
* **Routines** (docs/routines.md): schedules (cron in a time zone, interval, once; DST rules)
  or one webhook; templates with `event.*` for webhooks; runs as new chats with results in
  Recent, Home, attention and notifications; agentic runs in `ask`/`ask_all` with a 24 h
  approval timeout; untrusted runs (T16) whose flag sticks to the chat; retention of 30 seen
  run chats per routine; caps in `[routines]`.
* **Blob store** (docs/media.md): per-blob AES-256-GCM keys in master.db, crypto-shredding of
  the live files, sniffing by magic bytes, the T17 serving rules, quotas and a free-disk floor
  in `[storage]`, upload slots and timeouts, hourly sweeps.
* **Media**: a private library of images, video and audio with browser-made thumbnails;
  images in chats for vision models (the node's `images` capability, a 7 MiB frame guard,
  "Still in N chats" on delete); `[media] enabled` now defaults to true.
* **Artifacts** (docs/artifacts.md): the `publish` tool (Coder, Writer and any agent that may
  edit files or run commands; approved in `ask`) and "Save as artifact"; the node's confined stage
  pipeline and `[artifacts]` in node.toml; pull-only, verified transfer (T18); versions with
  retention (5 plus up to 10 pinned), sharing with the workspace, one Media item per image,
  video or audio lineage.
* **Settings → Server → Storage**: the limits and each account's usage (numbers only).
* **Bundle**: the login form is its own lazy chunk, so signed-in visits never load it. From
  this round on the total is a soft limit (each feature raises its own line with a reason);
  the first load keeps a hard ceiling of 104 KB gzip, because every first visit waits for it.

**Evidence**
* Tests per area: store, unit and HTTP tests for routines (clock seam; the webhook order and
  throttles; untrusted turns with a real chat runtime), the blob store and sniffing (STREAM
  round trips, the key-shredding gate), media, artifacts (a hostile fake node, confinement,
  the capability gate, retention), search (`search_http.rs`, incl. real routine run chats,
  media and published artifacts with their thumbnails and visibility) and the web (sidebar,
  account menu, search panel, shell, the three pages, chat cards).
* End to end on the debug binary (Playwright): `phase3`–`phase9`, `rich`, `setup`, `savings`,
  `backdrop`, `container` (the binary in container mode), `routines`, `media`, `artifacts`
  and `attention` pass; the older scripts were brought up to date with the onboarding and
  provider forms, the composer's agent picker and the account menu, and now wait for the
  server instead of sleeping.
* Bundle: first load 102.3 KB gzip (ceiling 104), total 359.5 KB after Phase B, before the
  last phase's images-in-chats code (measured on each merge; `web/scripts/size.mjs`).

**Deviations from the contract (accepted)**
* A fifth routines table, `routine_untrusted_chats`: the untrusted flag outlives the routine
  and its runs and goes only with the chat.
* All-failed artifact lineages wait for the hourly sweep (their reason stays on the chat
  card), hidden from lists, search and caps; `latest` falls back to the newest version of any
  status while none is ready.
* The Artifacts "Shared" filter is client-side; "Save as artifact" reads `/api/features` once
  per agentic chat; the webhook sample event is never saved.

**Risks / gaps**
* SSD and copy-on-write remnants of master.db can keep deleted keys (README, docs/media.md).
* Webhooks reach only where the master is reachable (loopback by default).
* A chat's untrusted banner comes from its routine mark, so it goes with a deleted routine
  while the chat's turns stay untrusted.
* The 30-per-hour stage throttle is in memory and resets with the master.

**Next:** Connections (a separate host name, sandboxed); trusted routines through
`runner::approval_for`; more event sources (`impl EventSource`); shared Media in workspaces.

### 2026-11-09 — Node setup over SSH removed; graphify install and node.toml endpoints removed

FIX_PLAN 1.4 (AUDIT D4, Q2) and 1.2 (AUDIT S13, Q3). **Removed:** `crates/provision`
(russh, the setup scripts, WireGuard config generation, the watchdog), the master's
`provision.rs` / `node_setups.rs`, `/api/admin/provision*` and `/api/admin/node-setups*`,
the server problems in `/api/attention`, the *Set up a server over SSH* and *Servers set up
over SSH* UI, `e2e/setup.cjs` and `fake_sshd`; `russh` is gone from `Cargo.lock`. Why: it
was a separate product (users, nftables, sshd, WireGuard on someone else's server) where a
bug locks people out of their server or opens it, for a v0.1 with no users; Ansible or
cloud-init do it with a track record. Nodes are now set up by hand (README "Set up a node by
hand": user, folder, `node.toml`, the join command, a systemd unit to copy); hardening the
host is the operator's job. The status entries of 2026-11-03 about node setup are history.
Also removed: `POST /api/admin/graphify/install`, `PUT /api/admin/graphify` and the
hand-written `node.toml` editor; graphify is allowed only by `graphify = true` in
`node.toml`, read at start, and installed with `rizzy-harness graphify install` on the node.
The settings card is read-only and shows those two steps, so T5 holds for HTTP too. Kept:
remote nodes, *Create join command*, `node join` / `node run`, revocation, the node link and
the WireGuard interface check for `--bind`. Not done here: the node's start-up self-checks
(sandbox, data dir outside the workspace roots), which are FIX_PLAN 2.7 and 2.1.
**Evidence:** `settings_http.rs::no_endpoint_installs_software_or_edits_node_toml` (the
removed routes answer 404/405 with a built-in node wired in, node.toml unchanged);
`savings.test.tsx` (no install button or allow switch, the steps shown);
`cargo tree -p harness -i russh` finds nothing.
