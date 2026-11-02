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
* **Single owner now, built for teams.** `users` table with roles (only `owner` used in v1),
  `owner_id` on every conversation, node and memory row, username + password (Argon2) replacing
  the shared UI token, a first-run claim code printed to the console. No team UI in v1.

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
* Payload changes are additive only (new optional fields). A breaking change is
  a new kind.

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
| T2 | Malicious website in the user's browser (CSRF, DNS rebinding, cross-site WS) | Auth is a 256-bit random token exchanged once via `POST /api/login` for an `HttpOnly; SameSite=Strict` cookie. Every request checks `Host` against an allowlist (anti-rebinding) and every state-changing request and the WS upgrade check `Origin`. Token compared in constant time. |
| T3 | Secrets leak via logs, URLs, event log | Provider keys are referenced by env-var name in config, held in `Secret<String>` (redacted `Debug`), never put in events. Tool output is scrubbed of known secret values before it is stored. No token in any URL (no `?token=`). `tracing` never logs headers. |
| T4 | Prompt injection from repo content makes the model run harmful commands | Default approval mode `ask`: every mutating tool (bash, edit, write) needs a human click; read-only tools are confined to the session workspace. No network sandbox in v1 (open risk R2). |
| T5 | Compromised master widens node permissions | **Node-local policy (`node.toml`) is authoritative** and is not settable over the protocol: allowed workspace roots, max approval mode (`ask` default; master can request stricter, never looser), tool denylist, whether remote-created sessions are allowed at all. The node logs every command it received in its own store. |
| T6 | Compromised master approves things on nodes | Accepted in v1: the master is the approval channel, so a compromised master can approve anything inside node policy. Bounded by T5. Future: approvals signed by a user device key verified on the node (R1). |
| T7 | Rogue node / stolen node key | Node keys pinned after operator approval; revocation deletes the pin and drops the link. A node only writes its own sessions; the master rejects events for sessions owned by another node. |
| T8 | MITM on the node link | WSS plus Ed25519 mutual challenge-response with pinned keys, so a mis-issued TLS cert alone is not enough. |
| T9 | Runaway processes | Bash runs in its own process group; cancel/timeout sends SIGTERM then SIGKILL to the group. Cancel propagates master → node as `session.cancel`. Output is capped. |

---

## 5. Decisions taken (beyond section 2 of the brief)

* **SQLite via `rusqlite` (bundled)**, not sqlx: synchronous, simple, no
  compile-time DB, single static binary. Calls go through `spawn_blocking`.
* **HTTP client `reqwest` 0.13 with rustls**; SSE parsed by hand (≈40 lines)
  rather than an unmaintained `eventsource-stream` (last release 2022).
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
* **R5** The UI token is a single shared secret (single-user v1). Multi-user is
  out of scope.
* **R6** Two-db embedded mode doubles disk usage on the master machine.

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
