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
  local ML model. Breaking it needs a written reason here.
* **Everything heavy is a service = a URL.** Memory server, embedding model, reranker, later
  speech. Each is *managed* (the master installs, starts and health-checks it), *LAN* (another
  own machine) or *cloud API*. Locality only changes the URL; the code path is identical. Local
  models run in existing runtimes with OpenAI-compatible APIs (e.g. Ollama, llama.cpp server).
* **Use maintained dependencies instead of hand-rolling.** Exact pins, committed lockfiles,
  `cargo-deny` in CI (licences, advisories, duplicates), APIs checked on docs.rs before use.
  Phase 1's hand-rolled SSE parser, markdown renderer and provider client get re-evaluated
  against maintained libraries (the renderer must sanitize).
* **Multiple accounts, admin-gated; team workspaces later.** The first account is an
  admin created with a one-time claim code. Only admins create accounts (temporary password,
  changed at first login). Everyone sees only their own conversations. Agentic sessions (a
  shell on a worker node) need a per-user grant. Localhost by default; remote only with an
  explicit flag. Shared team workspaces are a later phase.
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

`$HARNESS_DATA_DIR` or `~/.local/share/harness/` (`dirs` crate):
`master.db`, `node.db`, `ui-token` (0600), `harness.toml`, `node.toml`,
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
* The master mirror stores `kind` and `payload` **opaquely**. An older master
  can mirror events from a newer node; it just renders unknown kinds generically.
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
| `approval_requested` | `approval_id, call_id, tool, input, reason` |
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

Pairing (Phase 2): Ed25519 identities on both sides. Node runs
`harness node join <master-url>`; master shows the node fingerprint; operator
approves it in the UI (or `harness pair approve <fp>`); key is pinned. Each
connection is a challenge-response over the TLS channel (both sides sign a
nonce bound to the session). The node pins the master's key on first use
(SSH-style) and refuses a changed key loudly.

---

## 4. Threat model

The master can make nodes run shell commands. Treat it as an RCE service with a
login page.

**Assets:** shell on every node; source code; provider API keys; the UI login.

**Adversaries and mitigations**

| # | Threat | Mitigation |
|---|---|---|
| T1 | Network attacker reaches the master UI | Bind `127.0.0.1` by default. Non-loopback bind requires TLS (Phase 2) or the explicit `--insecure-bind` flag (tailnet/WireGuard mode), which logs a warning on every start. |
| T2 | Malicious website in the user's browser (CSRF, DNS rebinding, cross-site WS) | Accounts with Argon2id passwords; the first (admin) account needs a one-time claim code from a 0600 file. Login sets a random `HttpOnly; SameSite=Strict` cookie whose SHA-256 alone is stored; unknown usernames are verified against a dummy hash so timing does not reveal them. A separate 256-bit API token (0600 file) serves scripts as `Bearer`. Every request checks `Host` against an allowlist (anti-rebinding) and every state-changing request and the WS upgrade check `Origin`. Token compared in constant time. |
| T3 | Secrets leak via logs, URLs, event log | Provider keys are referenced by env-var name in config, held in `Secret<String>` (redacted `Debug`), never put in events. Tool output is scrubbed of known secret values before it is stored. No token in any URL (no `?token=`). `tracing` never logs headers. |
| T4 | Prompt injection from repo content makes the model run harmful commands | Default approval mode `ask`: every mutating tool (bash, edit, write) needs a human click; read-only tools are confined to the session workspace. No network sandbox in v1 (open risk R2). |
| T5 | Compromised master widens node permissions | **Node-local policy (`node.toml`) is authoritative** and is not settable over the protocol: allowed workspace roots, max approval mode (`ask` default; master can request stricter, never looser), tool denylist, whether remote-created sessions are allowed at all. The node logs every command it received in its own store. |
| T6 | Compromised master approves things on nodes | Accepted in v1: the master is the approval channel, so a compromised master can approve anything inside node policy. Bounded by T5. Future: approvals signed by a user device key verified on the node (R1). |
| T7 | Rogue node / stolen node key | Node keys pinned after operator approval; revocation deletes the pin and drops the link. A node only writes its own sessions; the master rejects events for sessions owned by another node. |
| T8 | MITM on the node link | WSS plus Ed25519 mutual challenge-response with pinned keys, so a mis-issued TLS cert alone is not enough. |
| T10 | Another account reads or drives someone else's work | Every session carries `owner_id`; REST and WS show a session only to its owner, admins included (admins manage accounts, not chats). Only the API token, the machine owner's key on disk, sees all. |
| T11 | A low-trust account gets a shell | Agentic sessions run bash on a node, so they need a per-user `can_agentic` grant (default off; admins always on). Chat sessions have no file or shell tools. |
| T12 | Password guessing / account takeover | Login throttle (5 failures per username per 15 min → 429). Trade-off accepted: an attacker can lock out a known username for 15 minutes. No self-signup; admin-created accounts start with a temporary password that must be changed; disabling, deleting or resetting a password signs the user out everywhere. The last active admin cannot be removed or demoted, and admins cannot change their own access. |
| T13 | Credentials sniffed on the network | Default bind is 127.0.0.1. `--insecure-bind` serves plain HTTP with a loud warning (VPN/tailnet only); real TLS arrives with the node link (Phase 8). |
| T9 | Runaway processes | Bash runs in its own process group; cancel/timeout sends SIGTERM then SIGKILL to the group. Cancel propagates master → node as `session.cancel`. Output is capped. |

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
* **Agents travel with commands.** The master owns agent definitions and puts
  the resolved `AgentSpec` on `session.create` / `session.prompt` /
  `session.set_agent`; nodes never store definitions. Effort is stripped by the
  master unless the model is registered with `effort = true`.
* **YAML via `serde-saphyr`** (pure Rust, released 2026-10) for agent and,
  later, skill frontmatter; `serde_yaml` is deprecated and its forks idle.
* **`cargo-deny`** in CI: permissive licences only; MPL-2.0 allowed solely for
  `option-ext` (via `dirs`, unmodified); internal crates are `publish = false`.
* **Model registry** in `harness.toml` (`[[models]] id, effort, vision`);
  effort is only sent to models listed with `effort = true`.
* **Process-group kill via `nix::sys::signal::killpg`** (safe API, no `unsafe`
  in our code); `tokio::process::Command::process_group(0)`.
* **Approval modes:** `ask_all` (every tool), `ask` (default: mutating tools),
  `auto`. Decisions: `allow`, `deny`, `allow_session` (this tool, rest of session).
* **UI ⇄ master:** REST for commands, one WebSocket for streams. Client sends
  `subscribe{session_id, after_seq}`; server subscribes to live broadcast
  *before* reading the DB backlog, then drops live events `<= last sent seq`, so
  there is no window where an event is missed or doubled.
* **Mock provider** (`provider = "mock"`) ships in the binary: deterministic
  scripted replies with tool calls. Used by tests and for trying the UI without
  an API key.
* **Frontend:** React 19 + Vite + TypeScript 5.9 (not TS 7 — boring choice),
  no UI framework, hand-written CSS. `web/dist` is embedded via `rust-embed`;
  a `build.rs` creates an empty `web/dist` if missing so `cargo test` works
  without Node; the server then serves a "UI not built" page.
* **Phase 1 binds loopback only**; TLS arrives with the node link in Phase 2.
* **MCP:** use the official `rmcp` crate (verified: crates.io `rmcp`, repo
  `modelcontextprotocol/rust-sdk`, 3.8.0 as of 2026-10-26). Pin at Phase 4 start.

## 6. Licences of references

None of their code is copied. Ideas only. Verified: this repo is MIT.
Moltis, esper-relay are stated MIT and vibe-kanban Apache-2.0 in the brief;
Mjolnir GPL-3.0 (docs only). **Not verified by me:** sci-pi's licence (brief
says none visible — treat as all-rights-reserved), agentic-harness maturity,
DeepSeek Harness UI licence. If any pattern from the DeepSeek UI is ported
literally, its MIT notice goes in `web/THIRD_PARTY.md`.

## 7. Open risks

* **R1** Master compromise = approval forgery within node policy (T6).
* **R2** No OS sandbox for bash in v1 (landlock / bubblewrap later).
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
* No OS sandbox for bash (R2). The UI token is single-user (R5).
* Not tested against real vendors yet (no keys in CI); SSE parsing is tested against recorded chunk shapes (R4).

**Next:** Phase 2. Threat model T7/T8 first, then `harness node join`, Ed25519 pairing, WSS bridge
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
Chat/Agentic choice in the web UI. Removed every compatibility shim (migrations,
role alias, token-file rename); the schema is created complete.

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
  effort. Apply = validate → build → write `harness.toml` → key into `secrets.toml` (0600,
  never in `harness.toml`, never returned by the API) → hot-swap the provider behind both
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
  (only via `harness.toml`). A typed-model fallback is a small follow-up if it bites.

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
  layer). Text is extracted on the master (PDF parsing in a blocking task, so a parser
  panic is an error, not a crash), capped at 100k characters, and travels *inside* the
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
  subprocess sandbox would be stronger.

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
* Remote nodes need their own provider config and key (`harness.toml` on the node).
* No rate limit on failed node handshakes (each costs a signature check; audit logged).

**Next:** Phase 9 (later items).

### 2026-11-03 — Phase 9 (later items): partly done, the rest deliberately not

**Done**
* **Checkpoints + undo.** Agentic turns in a git repository get before/after snapshots:
  commits written through a *temporary* index (untracked files included, ignored files
  not) and kept under `refs/harness/<session>/<turn>/`. The user's index, HEAD and
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
(refs under `refs/harness/`, index/HEAD/branches untouched). Other folders get a
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
key-only SSH (a `10-harness.conf` drop-in, which wins over cloud-init's `50-…`,
checked with `sshd -t`; the user's own public key is added first so they keep
access); firewall (own `inet harness` nftables table: inbound only SSH and UDP
51820; outbound open for agents); WireGuard (node key generated on the node; master
peer only); then the user brings the master's side up once with root (shown
command, then *continue*); then harness installs itself as an unprivileged
`harness` user with a hardened systemd unit, joins over the tunnel and waits for the
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
  themselves. It lands in the node's `secrets.toml` (0600), not `harness.toml`.
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
* `harness-admin`: harness's own SSH login, its own ed25519 key, locked password.
  *Automatic* mode **requires the root login** (refused in the form, the API and again
  after login if uid ≠ 0). With root, harness creates `harness-admin` with
  passwordless sudo for the setup, proves the key login, adds the user's own public
  key to root if given, and drops the root connection. Everything after that runs as
  `harness-admin` via `sudo -n`. **At the end harness removes its own sudo** (opt-in
  to keep it): a master holding passwordless root on every node would turn a master
  compromise into root everywhere, which breaks T5. Re-running automatic setup then
  needs root again.
* *Manual* mode should get a **normal user**. Root is accepted with a warning. After the
  login, harness asks: *Create harness's own user* (creates `harness-admin` **without**
  sudo, switches to it, removes its key from root) or *Keep using root* (flagged).
* `harness`: the unprivileged runtime user the node service runs as. Agents' commands
  run as it. It never has sudo, and it is a different user from `harness-admin`. If
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
WireGuard → install → drop sudo), all post-bootstrap commands as `harness-admin`,
Docker and kept-sudo flagged, distro tiers, manual root question (create: no sudo, key
off root, re-check as harness-admin; keep: flagged); HTTP (automatic with non-root
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
  `keyboard-interactive` = problem. Firewall: `sudo -n nft list table inet harness`
  through a sudo rule that allows **only that command** (added for `harness-admin` in
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
`prohibit-password`); `harness-admin` with sudo, then sudo dropped, locked password; the
watch-only sudo rule allows `nft list table inet harness` and nothing else; the russh
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
  host's `wg-harness` and binds loopback / `10.77.0.1` exactly as on bare metal. The
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
