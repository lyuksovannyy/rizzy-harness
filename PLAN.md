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
  **One accepted excess (the owner, 2026-11-19: "Accept, write reason"; T85):** the budget is
  measured on a fresh server, and that number does not move: a release `serve` idles between
  about 35 and 45 MB from run to run, with or without the connector code, because nothing of
  connectors exists until first use. After a server's people use custom connectors heavily the
  process keeps up to about 10 MB more and does not give it back (+5.1 MB after the first
  check, +10.4 MB after 200 heavy calls; an upper bound from a release test process with the
  fake server in another process, not measured inside a real `serve`), so such a server can
  stand at about 45 to 55 MB while idle. Why it is not lower: all connector work already runs
  on one shared thread (six threads kept about 20 MB); what stays is what the first TLS use
  and that thread's runtime took from the allocator, and returning it needs either a smaller
  baseline or allocator control that is ruled out here (`unsafe`, or an environment variable
  the operator must set). The fresh-idle check of every phase stays as it is.
* **Everything heavy is a service = a URL.** Memory server, embedding model, reranker, later
  speech. Each is *managed* (the master installs, starts and health-checks it), *LAN* (another
  own machine) or *cloud API*. Locality only changes the URL; the code path is identical. Local
  models run in existing runtimes with OpenAI-compatible APIs (e.g. Ollama, llama.cpp server).
* **Use maintained dependencies instead of hand-rolling.** Exact pins, committed lockfiles,
  `cargo-deny` in CI (licences, advisories, duplicates), APIs checked on docs.rs before use.
  Phase 1's hand-rolled SSE parser, markdown renderer and provider client get re-evaluated
  against maintained libraries (the renderer must sanitize).
* **Multiple accounts, admin-gated; workspaces group them.** The first account is an
  admin created with a one-time claim code. Only admins create accounts (a temporary password the
  server makes and shows the admin once, changed at first login). Everyone sees only their own conversations, in every workspace (the one exception is the static API token, T29).
  Each account has a Personal workspace and may join team workspaces with a fixed role
  (owner, admin, member, guest); settings come in four tiers: account, workspace (what applies
  inside it), workspace admin (members, defaults) and server. Agentic sessions (a shell on a
  worker node) need a per-user grant, and the workspace role must allow them too. Localhost by
  default; remote only with an explicit flag. Shared chats and shared memory are not built.
* **A data directory in use is never wiped; no other compatibility code before the first
  release.** The owner's decision of 2026-11-21 (it replaces "the schema is changed in place
  and dev data directories are wiped"): the harness is in daily use before its first tag, and
  every schema edit made that data directory unusable. **Schema versions** (fix plan 4.3,
  `crates/store/src/migrate.rs`): every database (`master.db`, `chat.db`, `node.db` through
  `Store`, `memory.db` through `SqliteMemory`) carries `PRAGMA user_version` and is opened
  through numbered migrations. Version 1 is the whole schema as it was frozen: for `Store`
  the schema of commit `3f0859a3` (2026-11-19), which the data directory in use had, compared
  object by object; the `project_clones` table added after it is migration 2 and `session_forks` (a fork's lineage, C.2) migration 3, `chat_uploads` (T94) migration 4 and `session_files` (the files a plain chat may read, T95) migration 5. A recorded
  version (`crates/store/schema/<n>.sql`, `crates/memory/schema/<n>.sql`) is never edited and
  never recorded again: every change is a new migration with its own recorded file
  (`HARNESS_WRITE_SCHEMA=1` only fills a file that is missing or empty), the tests fail
  otherwise, and a test holds version 1's text fixed. The recorded text is
  what the migrations make (SQLite's own `sqlite_*` objects and FTS5's shadow tables are left
  out, so a SQLite update does not change it). **The version is trusted only with the schema:**
  a file at the latest version must have exactly its recorded schema, and a migrated
  one must end with it before its version is bumped, or it is refused unchanged. A database
  from a development build older than version 1 is refused, named and left as it is; it
  cannot be migrated. **Events:** a kind or a payload field is added, never renamed, removed
  or given another meaning, so a log written before a change still decodes and folds; the
  wire between master and node and the configuration files carry no aliases yet (a master
  and its nodes are updated together). **Open** (review R43-8):
  `master.db`, `chat.db` and `node.db` share one migration list, so a master-only change bumps
  every node's `node.db` (and a node rolled back afterwards is refused over a table it never
  reads), and node and chat files carry the account tables unused. Splitting the list per role
  is now a migration per role. A value that is
  queried or joined is a column; the `kv` table holds opaque documents only.

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
* Built-ins (clonable; summary and additional instructions editable), grouped by a
  `category` and reachable by `aliases`: **Assistant** (`general`, `@ai`, the default:
  web, user facts, project memory; agent only), **Smol** (no tools, no memory),
  **Researcher** (`@research`: web, project memory), **Software Developer** (`coder`,
  `@dev`: files, commands, web, publish; delegates to Scout, Researcher, Reviewer),
  **Senior Teamlead** (`teamlead`: the developer's tools plus user facts; delegates to
  Developer, Researcher, Writer, Scout, Reviewer with user-visible caps on sub-agent count
  and tokens per turn; agent only), **Writer**, **Scout** (read-only code search;
  sub-agent only) and **Reviewer** (read-only review). Ultracode was replaced by the
  Teamlead (2026-11). Users create agents and choose the defaults: one per mode, the
  Assistant for chats and the Software Developer for Agentic chats, each overridable per
  workspace (`default_agent`, `default_agentic_agent`) and per user; an Agentic default
  must have file or command tools.
* `available_as` is enforced by the master, not only the picker: every path that picks a
  chat's or a turn's agent (session creation, `set_agent`, a prompt's `agent_id`, a
  leading `@mention`, workspace and user defaults, routines) goes through
  `Resolver::top_level`, which refuses a sub-agent-only agent; a mention of one refuses
  the prompt rather than answering as another agent. Delegate rosters hold only agents
  available as sub-agents. A stale agent id (deleted, removed built-in) fails closed: the
  chat's next prompt is refused (`AgentGone`, naming the id) until another agent is chosen,
  a default setting naming one is refused when used (`BadDefault`, naming the setting), and
  a routine naming one is blocked. Nothing falls back to another agent.
* Ids and aliases are one namespace; events, sessions and settings store ids. An id beats
  an alias and the first claim of an alias wins (built-ins, files, custom agents), the
  loser's alias dropped and logged, so existing data keeps loading.
* The same agents serve chat and agentic sessions; tools the runtime lacks are unavailable.
  Node policy still wins: an agent's tool list is a request, not a permission.
* Switching agent mid-session is allowed between turns (history carries over; the prompt
  cache resets). The pick is the chat's state, kept in the master's session row, never an
  event; the node refuses `session.set_agent` while the chat works (its turn slot is held, or
  a `/compact` runs, or the chat is a sub-agent's) and records the switch as
  `agent_changed { agent, model }` right before the `turn_started` of the first turn under the
  chat's own agent (a message, a Retry, a callback turn; not one whose agent was named for it:
  an `@mention` and its Retry, an answer's asker, a routine's explicit agent, a queued
  message's inherited agent) whose agent or model differs from the last recorded one
  (`session_created`, else the last `agent_changed`). The mirror moves the row on
  `agent_changed` too, so a mirror rebuilt from the node's log ends on the last switch. The
  master runs one dispatch per chat at a time (`Master::chat_lock`): a prompt, retry or
  callback from its row read to the node's answer, and a pick from asking the node to writing
  the row; held at most until the node answers or `ACK_TIMEOUT` (15 s) fails the request,
  never for a turn. Residual: a pick no turn ran yet is master state only; it survives a
  master restart (the row is on disk), not the loss of the master's database (a mirror
  rebuilt from the node's log ends on the last switch that ran). One agent answers per turn; an `@mention` routes that turn to the named
  agent (mention popup lists agents; the selector shows the override). Several agents in one
  turn only through an orchestrator.
* Event fields: `author` (agent id) on assistant/tool events; agent snapshot (name + prompt
  hash) in `turn_started` so old turns stay explainable after an agent is edited.

### Home screen and bar (from the mockup)

* Sidebar v1: New chat, chat search, Recents, user menu. Projects and Library later; Images,
  Scheduled, Plugins and Explore cut.
* Sidebar since 2026-11-08: New chat, Agents, **Routines** (was "Scheduled"), **Media** (was
  "Images") and **Artifacts** (was "Library"), each shown per viewer and workspace; Plugins
  stay cut, as a disabled **Connections** entry ("Later"; since 2026-11-17 planned as a
  **Plugins** screen per workspace, a list of skills and connectors, ADOPTION_PLAN X.34);
  Projects and Recent sections; the
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
  - Sub-agents (`delegate`, today the Teamlead's and Software Developer's) report back as synthetic tool results; since
    2026-11-21 a writer works in the chat's folder unless the call asks for `folder: "own"` (its own worktree) or the node's
    `[limits] sub_agent_folder = "own"` says so (T32).
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
                        │  HTTPS/WS (cookie + login key, same-origin; T2)
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
  session actor. `seq` is assigned by that actor, contiguous from 1. One
  process per data directory keeps it so across processes (A.20): `serve`,
  `node run` and `node join` hold an exclusive `flock` on `<data>/lock` from
  right after the directory is resolved, before the start-up sweep, the temp
  root's reset, the sub-agents' folder's removal and `recover_interrupted`
  (which appends to every session with an open turn); a second start is
  refused before it touches anything (`cli/tests/data_dir_lock.rs`). Where the
  file system has no locks the start warns and is unprotected. The lock is
  per directory, not per role: a server and a node do not share one.
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
| `harness-mcp` | added in Phase 4, not scaffolded empty (skills live in `harness-tools::skills`, X.2) | 4 |

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
| `session_created` | `title, cwd, model, backend, node_id` — always seq 1 (**added**: needed so the log alone describes the session); a fork's also has `forked?: {parent, through_seq}` (`session_forked` below); a sub-agent that can change files also has `sub_folder?: "shared" | "own"` (where it works, 2026-11-21, T32: the chat's folder or a copy of its own; absent for a reader); `exec_mode?` is how the chat's commands first run (N.10a), moved only by `exec_mode_changed` below |
| `turn_started` | `turn_id, agent?, cause?, environment?, continued_by?, retry?` — `continued_by`: in a sub-agent's log, the parent's `delegate` call id that sent this turn's message as a follow-up (T68; the turn has its `user_message`, so it is not a `cause`); its harness line for the opening is recorded after the environment line in `environment`. `retry: {turn_id, reason, error?, failed_model?, model?}` on a person's Retry (ADOPTION_PLAN X.21, T69): `cause` is absent (it is the person's turn: it refills the callback budget, clears the waiting ends, files `remember` as theirs) and there is no `user_message` after it; it answers the message the failed turn answered, which stays in the log once, and the model reads the failed attempt's steps, then one harness-written opening (`retry_opening`: the attempt `reason` and which of its calls ran: those with a result ran, those without may or may not have, denied ones did not; the attempt's last `error` quoted line by line, cut to 600 characters, for an `error` end only), with the background notes logged since the failure in front and the turn's environment line and block, rebuilt from the event alone; `failed_model` and `model` are for the chat only. Only the node writes it, after `harness_core::retry::check` passed against its own log under the turn slot. `environment`: the node's one harness line about where the turn runs (its clock and time zone, OS, shell, the git branch read from `HEAD`; T48), which opens the turn's own message (after a callback turn's opening); recorded so later requests rebuild that message byte for byte. `cause: {callback: [bg_id]}` on a callback turn (T.8d step 3): started by the harness because those background commands ended while the chat was idle (the node names every end of the chat still waiting when the turn starts, at most 64; it covers exactly those); it has no `user_message` after it, and the model reads one harness-written opening rebuilt from the log alone (`docs/background-commands.md` §13) | **`cause: {mention: {message_seq, by: "person"}}`** (ADOPTION_PLAN X.37, T87) on the turn of an agent a person's message called after the first: the node starts it, after the earlier agent's turn of that message ended, with no `user_message` of its own; `message_seq` is the seq of the message that called it; `tags` (optional) are the message's tags this agent has among its own delegates, which its opening names (`mention::tags_line`). It is a person's turn for the chat's background ends (it clears them, reads the notes before it) and for "finished, unread"; it is the harness's start for everything about rights: no connector (the grant never grants a turn with a `cause`), no `remember` filed, no `recall`, no board (`OpenTurn::callback` is true for any cause), and it is not a callback for the budget (`callback_check` counts `cause.callback` only). The model's opening is `mention_opening(agent, cause)`: one harness paragraph (which agent it is, that the person's message above called it, that the earlier agents answered above), built from the event alone like a retry's, behind the notes, the environment line and the block, so every later request rebuilds it byte for byte. It cannot be retried (`retry::check`: it has no message of its own). Only the node writes it, from specs the master sent with the prompt and the node kept in memory.
| `user_message` | `turn_id, text, attachments?, files?, reply?, images?, queued?, uploads?, attached?` — `attached[{file_id, name, bytes, sha256, mime}]` (X.54, T95, additive): files the person attached to a chat without a folder, which the master keeps as the owner's blobs and the agent reads only through `attached_file`; written by the node from the master's references after its own checks, `file_id` kept as it is by the scrubber (the master serves only an id its own record holds for the chat, marked by this event), the rest scrubbed as any text; the model is sent one line after everything else in the message naming each file, its id and size and the tool (`ATTACHED_LEAD`, rebuilt from the event alone); a fork's copy of the event gives the fork a row for the same blob. `uploads[{name, path, bytes, sha256, mime}]` (X.53, T94, additive): files the person attached that the model cannot read as text, which the node saved into the chat's folder under `attachments/` (`path` relative to the folder, the name the node chose, `x (2).ext` when taken); written by the node from what it saved, never from the protocol, scrubbed as any text; the model is sent one line after everything else in the message naming each path and size (`UPLOAD_LEAD`, rebuilt from the event alone); `mime` as the browser said, informational only; a rewind leaves the files, a fork copies the event and shares the folder. `files[{path, state (file/cut/folder/missing/refused/binary/too_large), text?, lines?, shown?, bytes?, note?, seen?}]`: what the node found at each path the person mentioned with `@path` (T63), read by the node under the `read` tool's rules when it recorded the message, sent to the model after the attachments, each under one line of the harness's and quoted line by line; `seen` (a `FileSeen`) when the file was read whole and shown, for the read-first guard.  `queued: true` when the person sent it while turn `turn_id` ran (X.5): stored and acknowledged at once, at its place in the log (in that turn, or right after it when the turn ended meanwhile); the model reads it only where a `queued_delivered` names its seq. It is the person's message for everything else (search, URL provenance, an open question it closes, the callback budget it refills) | **`tags: [agent_id]`** (X.37): the agents that run only as sub-agents the message mentions (never called; the node writes them from the master's `session.prompt.tags`, only to a node with `peers`, at most 16, plain handles, each once). **`triggers: [agent_id]`** (X.37): the agents the message called by mention, first agent first, in the order they answer; written by the node only when the message called several (the first is the message's own turn; each later one has a `cause.mention` turn); the node's restart note is computed from it (`harness_core::mention::unasked`).
| `queued_delivered` | `turn_id, seqs` — the model read these queued messages here: before the next model request of the turn they were sent in (after the step's tool results and background notes), or right after the next turn's `turn_started` when that turn ended without another request (completed or failed, or a callback turn, which reads none: the node starts the person's turn by itself, with the agent's sub-agents and the normal step cap, and no per-turn context; stopped, cut by a restart: it waits for the person's next message). Each seq is named once. A summary that cuts between a message and its reading carries the message (`summary_messages`), and the kept part adds nothing for the reading, so it reaches the model once. The model gets each as a user-role message: the message, then one harness line saying it was written while the agent worked (after the text, so the person's own words stay theirs) |
| `assistant_delta` | `turn_id, message_id, text` — coalesced, see below |
| `assistant_message` | `turn_id, message_id, text, tool_calls[{call_id, tool, input}], thinking?{model, blocks[{type: signed{text, signature} / redacted{data} / text{text}}], prefix?, unsent?}` — authoritative full text; `thinking` is the step's thinking as the provider sent it, in its order (X.25, T73): `model` the prefixed model that wrote it, `prefix` (signed blocks) the SHA-256 of the request that made them, `unsent` its signed blocks never sent back (a signed block scrubbed, over 256 KiB, out of order, or the model's thinking was refused earlier in the chat), nor any later signed block of that model; browsers get the thinking without `signature`, `data` and `prefix` (`event::for_browser`) |
| `thinking_withheld` | `turn_id, model, status, reason?` — the provider refused a request that sent back thinking and the same request without any was answered (X.25, T73): `model`'s thinking before it is never sent again and its later steps record `unsent`; `reason` as `images_withheld`'s; recorded only by the node's loop, before the answer; both folds show nothing for it |
| `peers_skipped` | `message_seq, agents, reason` — agents a message called (`user_message.triggers`) that were not asked, in the order they would have run: `reason` `stopped` (the person's Stop ended the turn that ran and the rest), `restarted` (the node restarted with agents still to ask: their specs were in memory only; written once at the node's start, from the log alone) `unavailable` (the node could not start the next turn: its log, a restore, a stopping node) or `forked` (a fork was cut inside the list: written by the fork itself right after its divider, for the agents with no turn at the cut, so a restart has nothing to add; C.2). One quiet line in the chat; never shown to the model (X.37, T87) |
| `conversation_rewound` | `message_seq, turn_id, files` — the person took the chat back to their `user_message` at `message_seq` (the turn it opened is `turn_id`); every event with `message_seq <= seq <` this event's own seq is **rewound** (ADOPTION_PLAN X.46, T90, `docs/rewind.md`). Nothing is deleted. The model is not sent a rewound range (nor the `turn_started` of `turn_id`), the file tools' seen table does not count what it read, the view marks its items `rewound`, retry and the hand-off note treat its turns as absent; usage, checkpoints, background commands, grants, mode, step limit and agent (the person's own settings) fold as before; the task list goes back with the conversation (a rewound `todo_updated` does not count; the undo brings it back). `files`: the folder was put back first (the restore with `rewind`), recorded false when no rewound turn had a checkpoint. Written by the node's session actor only, for the chat's owner (`session.rewind`), after `harness_core::rewind::check` and with no turn, `/compact` or background work of the chat running. The first message after one with `files` false opens with a harness line naming the files kept (built from the log). |
| `conversation_rewind_undone` | `rewind_seq` — the range of the `conversation_rewound` at `rewind_seq` counts again; only before a message, a turn or a summary came after it (`rewind::check_undo`); files are never touched |
| `session_forked` | `parent, through_seq, turn_id` — the divider of a fork (ADOPTION_PLAN C.2, T92, `docs/fork.md`): right after the copied events, "forked from `parent` here"; written by the node only, in the same transaction as the copy, for the chat's owner (`session.fork`). A fork's `session_created` carries `forked: {parent, through_seq}` (not `parent_session`, which means "sub-agent of"): its events `2..=through_seq` are the parent's, copied whole at the same `seq` under the new id (no renumbering, so every `seq` inside a payload is still right); the first event is the parent's `session_created` with the fork's title, the folder the node chose and `forked`. A copy costs the fork nothing (usage skips it), grants nothing, and its checkpoints are not restorable in the fork; the master mirrors it and runs none of the side effects it ran for the parent (`Master::apply`'s one early branch), the search index does not index it again, and the file tools' seen table starts empty. Background commands running in the parent at the cut are closed by `background_ended` with reason `forked` (never calls back); the divider also clears what waited for a callback turn. |
| `message_pinned` | `message_seq, pinned` — the chat's owner bookmarked (or un-bookmarked) the `user_message` or `assistant_message` written at `message_seq` (ADOPTION_PLAN X.51, T93, `docs/pins.md`). The latest event for a seq wins; one that names no earlier message counts for nothing. A pin is for the person: the model is sent nothing about it and no reader of the conversation (rebuild, `history`, hand-off, search) looks at it. Written by the node's session actor only, for the chat's owner (`session.pin`, capability `pin`), after `harness_core::pins::check` (a message of the chat, not rewound, not already so, at most 50 pins) and also while a turn runs. A fork inherits the pins of the copied events. |
| `tool_call_started` | `turn_id, call_id, tool, input` — the calls of a parallel batch: all starts first, then the results in call order ("Event order of a parallel step" below) |
| `tool_output_delta` | `call_id, stream (stdout/stderr), text` |
| `tool_call_finished` | `call_id, ok, output, exit_code?, truncated, note?, seen?, image?, made?` — `made` (`doc_create`, `sheet_create`, `doc_edit`, `sheet_edit`, X.56, X.57, T97, T98, additive): `{name, size, sha256, file_id?, path?}`, the Word or Excel file the call made, a reference only: `path` in a chat with a folder, `file_id` in a chat without one, where it is the id of the master's kept copy and the line that makes that copy this chat's (the mirror marks only a row the master made for this chat with this hash as logged, `session_files`, and a fork's copy of the event gets a row of its own); written only by the node's loop from the tool's outcome, name and path scrubbed; `image` (`read` of an image file, X.23, T71): `{sha256, mime, width, height, bytes}`, a reference only; the bytes the call read are in the chat's `session_images` on the node, keyed by the hash, and every later request shows them from there (never from the file again, never from the log); written only by the node's loop, for a model of the turn's chain listed with vision (`AgentSpec.vision`); — `seen` (`read`, `edit`, `write`): `{path, sha256, base?}`, the real path and the hash the file tools' staleness guard keeps (T45), and for `edit`/`write` `base`, the hash of what the call replaced (`""`: no file), which an undo uses to tell the turn's own changes from another chat's (T57); `output` is what the model sees; `note` (the repeated-call guard's line, T40; on a step's last result also the harness's half-budget or wrap-up line for the next request, X.8) is written only by the node's loop, sent to the model after the escaped output, and not shown in the UI |
| `approval_requested` | `approval_id, call_id, tool, input, reason, turn_allow, chat_allow, preview` (`turn_allow`, `chat_allow` only when true; `preview` only when the tool has one: for the line form of `edit`, the rows it replaces or the place it inserts at, numbered as `read` numbers them, from the file as it is when the card is made, at most 40 lines and 8,000 characters. Plain text for the person, made from file content: scrubbed with the event like any text, never sent to the model, not part of the key a "for this turn/chat" grant covers, drawn as text only (T45)) |
| `approval_resolved` | `approval_id, decision (allow/deny/allow_session), by (user/policy/cancelled/interrupted)` |
| `checkpoint` | `turn_id, phase (before/after/restored/before_restore), git_ref, files?, excluded?, excluded_more?, head_moved?` — `before_restore`: the folder just before a restore, written before it changes a file; its `turn_id` (`restore-<id>`) names the restore, whose `after` follows, so restoring that id undoes the restore (T33); if the restore failed before its `after`, restoring that id puts its `files` back to their saved content (A.16). `excluded`: nested git repositories the snapshot left out (on `restored`: the ones the restore left alone), relative to the checkpoint's folder; at most 50 names, control characters replaced, `excluded_more` counts the rest; the fold keeps their union (and the largest count) per checkpoint; the web UI ignores both for now. `head_moved` (X.42; on `after` only, and only when it moved): `{from, to}`, each `{branch?, commit}`: where the person's repository's `HEAD` pointed when the turn began and when it ended, read by the node from `<folder>/.git` itself (no git process: `HEAD`, the one ref it names and `packed-refs`, each read bounded and without following a link; absent for a plain folder, a gitfile, a link, a `commondir`, an unborn branch or a repository above the folder). `branch` is absent when detached, is at most 200 bytes, and has control characters, the line and paragraph separators and the bidirectional controls replaced (as `excluded` names are, plus those separators and controls); `commit` is 40 or 64 hex digits. A note for the chat only: undo reads none of it. The fold keeps it per turn. No `head_moved` for a turn cut off by a restart (a Stop still gets one), for a HEAD that ends where it began, an unborn branch's first commit, `git init`, `rm -rf .git` or `git worktree add` in the turn; the readings are of the folder, so with several chats in it the note says the branch changed while the turn ran, not which chat changed it; two names that differ only in characters the cleaner replaces read as one. Alongside, both folds remember, for each of the last 100 `before_restore` ids, which turns were undone (`restored`, `saved`) just before that restore; a `restored` event whose target is a saved id returns every turn to that remembered state (a restore of a saved id returns the folder to just before the restore that saved it), a target that is a turn marks it undone with the latest saved id, and an id older than the 100 is no longer remembered |
| `turn_ended` | `turn_id, reason (completed/cancelled/error/interrupted/stopped/limit), usage?` — `stopped`: cancelled by a stop of the node (A.19, below); `limit`: a person's turn used its `max_steps` steps and its one extra request, with tools listed and `tool_choice: none`, gave its account of what is done and left (X.8; a call that arrives anyway is answered "not run"). `usage.window?` and `usage.summary_at?` are the turn's own limits, set once by the native loop (never summed, never on a single call's usage): the context window of the model the turn started on when the master knows it, and the history size at which the next turn summarises first (`0`: never); the chat header's meter reads them. `usage.by_model?` (`{<prefixed model>: {input_tokens, output_tokens, cache_read_tokens?, cache_write_tokens?}}`, ADOPTION_PLAN X.18) splits the sums by the model that used them: the provider router (`ModelRouter`) stamps each call's usage with the model it was asked for, and `Usage::add` merges the breakdowns of every call, retry, fallback, summary, sub-agent and parallel answer, at most 16 models (`MAX_USAGE_MODELS`; one past the cap stays in the sums, unattributed). Tokens in no bucket are unattributed; the UI's cost estimate (computed in the browser from these sums and the admin's current prices) shows them as unpriced. The view keeps the chat's sums (`usage`) and the latest turn's (`turn_usage`; none when that turn reported none, as an interrupted one does; the fold then keeps that turn's last `turn_usage` as its usage, in the chat's sums too). Model ids are the node's: both folds cap a breakdown where they read it (`Usage::capped`), admit models in key byte order, and the TS fold never indexes an object through its prototype with one ("__proto__", "constructor") (X.39). A turn's `usage` here REPLACES its running `turn_usage` figure in both folds (never adds to it); a `turn_ended` without `usage` keeps the last `turn_usage` as that turn's. |
| `turn_usage` | `turn_id, usage` (X.39) — written by the session actor after each model call of the turn that reported usage (the loop reports through `TurnHost::emit`, as for every event; a summary call, a sub-agent's result and a parallel answer count as the loop counts them), and only when the total changed. `usage` is the turn's RUNNING TOTAL so far (the same type as `turn_ended.usage`: `by_model`, `context_tokens`), not that call's share. Both folds show the chat's finished turns plus the running turn's latest figure; a `turn_usage` for a turn that is not running, a duplicate, or one below the last is ignored. Numbers and model ids only: `usage.model` and the `by_model` keys are the node's prefixed model ids (at most 256 bytes where an id is configured; both folds keep at most 16 models), no text and nothing the model wrote. A foreground sub-agent's cost enters the parent's LOGGED figure when the sub-agent ends, not while it runs (the parent writes no `turn_usage` meanwhile; the child's own chat has its own). The log is unchanged by that: the chat header (`web/src/lib/childcost.ts`) adds, on screen only, the latest known total of each foreground child of the running turn from when it is first seen running until its `delegate` call has a result, ended or not (from the child's own log the tab already follows; nothing new is requested); the node reports the `turn_usage` that takes a child in only AFTER that call's `tool_call_finished` (the delegate site in `crates/agent-native/src/lib.rs`), so nothing is counted twice in either arrival order; a floor (tokens and cost each) covers the gap to the parent's next total and is dropped when the turn ends (a failed child's cost leaves the header then). A routine run's record takes the end's usage, else the last `turn_usage` of its turn (`Bound.last_usage`, one value per bound run, dropped with it) |
| `exec_mode_changed` | `mode, from` — the chat's commands moved between `sandbox` and `host` (N.10b, T39): the owner's switch, written by the session actor only, on `session.set_exec_mode` (capability `exec-switch`; master route `POST /api/sessions/{id}/exec-mode`, owner only), which the node decides again under the turn slot (it offers the mode; no turn, `/compact`, background work or approval of the chat). The chat's mode is the latest of these, else `session_created.exec_mode` (`harness_core::exec_mode`, the one rule); an unknown mode makes the chat's mode unreadable and it runs nothing. State, not conversation: a rewind does not move it back, a fork's copy carries it up to the cut. Carries no account id (the audit line names the person) |
| `step_limit_changed` | `max_steps, requested?` — the chat's own step limit (X.14), the owner's change as the node clamped it to `1..=` its `[limits] max_steps` (`requested` when it gave something else); written by the session actor only, on `session.set_step_limit`; read before each model request of a person's turn (a sub-agent's cap is at most its parent chat's) |
| `model_retry` | `turn_id, model, attempt, of, at_ms, wait_ms, reason (rate_limit/overloaded/server_error/network/no_answer)` — a model call (a turn's, a parallel answer's or a summary's) failed for a passing reason and is sent again at `at_ms` (node clock; `wait_ms` after the event, the most a skewed browser shows). Written by the session's own loop just before the wait; only the reason's class, never the provider's body. The view keeps the latest until output resumes, a fallback answers or the turn ends |
| `error` | `turn_id?, message, notice?` — `notice: true` (written only when set; absent reads as a failure) marks information for the person, not a failure: the end-of-turn and after-undo notes of a turn that changed more than 100 files (T.9). The chat draws it muted, not red; the retry, a sub-agent's reported error and the avatar's error face do not read it as the turn's error |
| `background_started` | `bg_id, call_id, turn_id, timeout_s, ends_by_ms, child?` — a `bash` call with `background: true` still running after its 3 s first wait; appended in the turn, before that call's `tool_call_finished` (T.8d, `docs/background-commands.md`). `child: {session, agent_id}` (X.24, T72): a sub-agent `delegate` started with `background: true` instead; `call_id` is that call, `timeout_s` its wall-clock limit, `bg_id` from the same numbering |
| `background_ended` | `bg_id, reason (exited/timed_out/stopped_by_agent/stopped_by_user/access_revoked/node_stopping/node_restarted/forked), exit_code?, signal?, confirmed (ended/still_alive/unconfirmed/unknown), ran_ms, output_bytes, tail, tail_cut, delivered, child?` — once per command, by the session actor: at a step boundary of the running turn (`delivered`, the model saw its note there), between turns, or at the node's start (`node_restarted`); `tail` is at most 4 KiB of program output, scrubbed where the output entered the node; the model reads it quoted line by line (`| `) under one harness line, inside the step's last tool result or in front of the person's next message, never as a user-role message of its own. `session_deleted` exists as a reason and is never recorded (the log goes). A background sub-agent's (X.24, T72) has reason `finished` (its turn ended by itself; calls back like `exited`), `timed_out`, a stop's or a restart's, an empty `tail`, and `child: {session, agent_id, result, ok, usage?}`: `result` is what a foreground `delegate` would have returned (`subagent::render`, its report quoted), built once when the node recorded the end, after merging a writer's folder back; the model's note is built from it alone; `usage` is added to the chat's usage by both folds |
| `callback_refused` | `bg_ids, reason` — the node did not start a callback turn for these ends on its own rules (its `callback_turns` budget, `callback_turns = 0`, a routine's chat, a revoked chat); recorded once per refusal, never shown to the model; the ends wait for the person's next message (T.8d step 3) |
| `context_sent` | `turn_id, frame (person/task/callback/step), sections[{key, digest?, text?, note?}]?` — the harness's context block the turn's opening message carried (X.1, T52), section by section in the order sent: standing ones (`user_instructions` (the node operator's file, T62), `instructions`, `instructions:<path>` (one instruction file of a subfolder of the working folder, by its path from there, e.g. `instructions:web/AGENTS.md`: B.9, T65), `skills`, `memory` (the owner's facts), `memory_recall`, `workspace` (the workspace's memory), `workspace_recall`, `remember` (what `remember` does in the chat and in its project), `memory_notes` (the saved-memory counter and notes on earlier calls), `project` (the brief), `project_memory`, `project_recall`, `earlier_chats`, `board`, with the SHA-256 of their text, or an empty text and digest and, for a list, a note when withdrawn) and this turn's own (`turn`). Appended by the loop after the turn's message and before its first answer; a later record for the same turn replaces it (a summary mid-turn sent the opening again; an empty one: the block is no longer needed). Every later request rebuilds the opening from it with the same renderer. **The history holds at most the current version of each standing section:** a standing key is kept only in the last record that sent or withdrew it (and in none when the turn being built sends or withdraws it), so an agent whose memory is off reads none of the facts and a forgotten fact leaves the next request; that turn writes the cache from the earliest opening that held the section, and the turns after it rebuild byte for byte. The last digest of a key since the last summary is what the model has; a section with that digest is not sent again. **A `step` record** (B.9, T65) is not the turn's opening record: the loop appends it after a step's tool results (and the background notes recorded at that step's end) when the step's `read`, `edit` or `write` calls touched files under subfolders whose instruction files the history does not hold yet; it carries those files' `instructions:<path>` sections in call order (outermost folder first for one call) and `turn` lines naming files not loaded, and every later request rebuilds it where it stands as a user-role message of its own (`ChatMsg::StepContext`), with the same rule for standing keys (a later opening that replaces or withdraws a key takes it out of the step's block); an in-turn summary keeps it, clearing never touches it. Not shown: the UI's fold ignores it. Not in the summariser's transcript (`summary_messages`); compaction sizes with the blocks |
| `context_shortened` | `turn_id, cleared?{turn_id, results}, folded?{steps, text, todo?}, thinking?{turn_id, steps}, tokens_before, tokens_after` — `thinking` (X.25, T73): the thinking of every step of the turns before `thinking.turn_id` and of its first `steps` steps is not sent (oldest first; latest in force); what the model is sent was shortened in this turn (ADOPTION_PLAN X.15, T59, `docs/turn-compaction.md`); the log and the UI keep everything. `cleared`: every tool result of the turns before `cleared.turn_id` and its first `results` are sent as a one-line stub (the latest event's is in force; nothing when that turn is no longer carried). `folded`: this turn's first `steps` steps are sent as `text` (model-written, quoted under the harness's line, in a user-role message of its own right after the turn's opening, never inside the person's message) with the task list as it stood. Appended by the loop through the session writer before the request it shortens; `rebuild` applies the latest state, so the log alone rebuilds the same request. The UI shows one quiet line (`Item::Shortened`). |
| `todo_updated` | `turn_id, call_id, items[{content, status (pending/in_progress/done)}]` — the agent's task list (its `todo` tool), the whole list each time; the latest one is the list (an empty one clears it). Appended by the loop of the session that made the call, before that call's `tool_call_finished`, only for a list the tool accepted (at most 50 items, one line of at most 200 characters each, at most one `in_progress`); a sub-agent's goes to its own session's log. Model text: after a summary a turn that has `todo` among its tools gets the latest list at or before the summary's `through_seq` again, every item quoted (`| [status] content`) under one harness line beside the summary (unchanged until the next summary: the prompt cache holds), and the summariser reads it quoted under `Task list:` (T38). Risk `chat_only`: no approval mode asks about it, `ask_all` included; a node's `denied_tools` still refuses it |
| `handoff_written` | `through_seq, text?, model?, failed[{model, reason}]?, usage?` — the model part of the chat's hand-off note (ADOPTION_PLAN X.16, T60, `docs/handoff-notes.md`), written between turns by the session's one writer on the node when the master asks (`session.handoff`, after 10 minutes idle) and `harness_core::handoff::due` holds; appended only if nothing joined the log since `through_seq` (its seq is `through_seq + 1`) and no turn claimed the slot. `text`: at most 1,500 characters, scrubbed; absent when every model in `failed` failed (reason classes only: `unknown to this node`, `failed`, `empty answer`). `usage` counts in the chat's usage (with its `by_model`, the model that wrote it). The latest is the note. Never sent to the chat's own model; the fixed part (first ask, files changed, task list, last end) is never stored, every reader builds it from the log (`handoff::Facts`). |
| `history_summarized` | `turn_id, through_seq, text, manual?, focus?, usage?, tokens_after?` — every event with `seq <= through_seq` reaches the model as `text` (the latest one counts). Written by the loop at a turn's start or inside a turn (X.15), or, with `manual: true` and an empty `turn_id`, between turns for a person's `/compact` (ADOPTION_PLAN X.20, T64): then `focus` is the person's words as the summariser got them (scrubbed, at most `COMPACT_FOCUS_MAX_CHARS` = 2,000 characters), `usage` counts in the chat's usage and `tokens_after` is the estimated next request, which the context meter shows until a turn reports its own. A manual one is appended only if nothing joined the log since what it read. |
| `images_withheld` | `turn_id, calls, status, reason?` — the provider refused a request that carried images `read` returned and the same request without them was answered (X.23, T71): the images of these calls (the ones before it in the log) are withheld from then on, as the harness's line; `reason` is the provider's words, scrubbed and cut to 200 characters, for the person (both folds add a line to each call's row); recorded only by the node's loop, before the answer |
| `compact_failed` | `reason, usage?` — a person's `/compact` made no summary (the model failed, the summary was not smaller, the chat changed meanwhile, Stop); `reason` is the harness's sentence, never a provider's body; nothing was replaced. `usage` counts in the chat's usage. Two since the last turn or summary refuse the next `/compact` until a turn runs (X.20). |

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

**A stop is not a crash (decision, A.19).** SIGTERM and SIGINT stop `serve`
and `node run` alike. The runtime refuses new sessions, turns, restores
(checked again under the restore registry's lock), staged files and
sub-agents with `harness_protocol::NODE_STOPPING` (the master answers 503
and gives a refused message's uploads back); approval answers, cancels,
replays and deletes are still taken, and the approval wait takes the cancel
first, so an "allow" that arrives with the stop allows nothing. Every
running turn is marked as the stop's (unless something cancelled it
before), then cancelled through the normal cancel path (its command's guard
ends the process tree, a pending approval is denied `by: cancelled`), takes
its after-turn checkpoint and ends `turn_ended{reason: stopped}`; a turn
cancelled before the stop ends `cancelled`, one the model had completed
ends `completed`. The process waits at most `STOP_WAIT` = the agent loop's
`TOOL_CANCEL_GRACE` + the checkpoint's `SNAPSHOT_TIMEOUT` + 15 s (80 s,
`cli/src/stop.rs`, a constant until N.9) and exits 0, or 1 naming the
sessions still running or whose end could not be written, which the next
start closes as above. A second signal exits 1 at once: the guards end
their commands when the node is gone (T9), committed appends survive a
process exit, and the data directory's lock is held by `main`, outside the
runtime, until the process is gone (A.20); the runtime gets 2 s
(`RUNTIME_SHUTDOWN`, also after a panic) and the process then calls its
exit whatever threads remain; a thread stuck in an uninterruptible read
keeps the process (a zombie) and its lock until the read returns, and the
lock's refusal and `doctor` say so. `serve` keeps HTTP and
WebSockets up while its runtimes close their turns, starts no routine run
(the scheduler, Run now and webhooks: 503, no run recorded), then gives HTTP
its 30 s grace; a routine's run that a stop cut ends `interrupted`, not
counted and not run again by itself (`docs/routines.md`). A remote
node keeps its link up until its turns are closed and its queue is taken. A
stopping server's remote nodes are not told anything: they keep their
turns and reconnect. A refused node exits 77. The agent-asked drain (a note
to the model, `drain_answer`, `node_draining`) is N.9.

**Event order of a parallel step (ADOPTION_PLAN X.19).** The calls of one
assistant message are handled in groups, in call order. A group is either one
call, handled exactly as before (`tool_call_started`, the guard and the
early refusal, the approval, the run with its `tool_output_delta`s,
`tool_call_finished`), or a batch: a run of neighbouring calls that may run at
the same time. A call may join a batch only when its tool is one of our own
that declares itself read-only (`ToolRisk::ReadOnly`, the value the approval
path uses) **and** parallel safe (`Tool::parallel_safe`, default false: `read`,
`grep`, `glob`, `outline`; never `bash`, a web tool, `skill`, `todo`,
`remember`/`recall`, `delegate` or an MCP tool), its arguments are valid, the
node would run it with no card (`TurnHost::runs_unasked`: the chat's mode and
`denied_tools`), its call key is not already in the batch, and the
repeated-call guard has margin for it (`RepeatGuard::batchable`). Any other
call ends the batch and runs alone in its place; the calls after it may form
the next batch. In a batch: every `tool_call_started` in call order, each
call admitted by the guard, checked by its tool's early refusal and approved
by the host one after another in call order; then the approved calls run, at
most `MAX_PARALLEL_CALLS` (8) at once, with no output stream (so no
`tool_output_delta` of concurrent calls can interleave); then each
`tool_call_finished` in call order, as soon as it and every earlier call of
the batch are done. The log, and so the next request rebuilt from it, never
depends on which call finished first. A Stop cancels every running call; each
still gets its `tool_call_finished` (a call that had not started:
`[cancelled before it ran]`), and the turn ends after the batch. A tool that
panics fails only its own call.

**Durability.** The node store runs WAL with `synchronous=FULL`: an event is
fsynced before it is published to anyone, so a seq the master has seen can
never be reused after a node crash or power loss. The mirror uses
`synchronous=NORMAL` — it can always re-fetch from the node.

**A failed append ends the turn (decision).** The log is written first: an
event is appended before the effect it describes. When an append fails (full
disk, I/O error, a broken database), `TurnHost::emit` returns the error and
the turn ends: no further tool body runs and no further model request is made.
That covers the turn's events, deltas included, the start of a tool call, the
approval request and its answer (an approval the log does not hold allows
nothing), a file the agent publishes (its stage is dropped), and a tool result
(the model would otherwise get a history the log does not hold). The first
failure cancels the turn's token, so a tool still running or a delegation
running beside the one that failed stops through the usual cancel path and
grace period; the rest of the turn's appends fail without being tried. The
closing `error`, a denial (`by: cancelled`) for each approval whose answer was
not written, and `turn_ended` are each tried once; if any of them cannot be
written, `turn_ended` is not, and the next prompt first closes that turn
(`error`, denials, `turn_ended{reason: error}`) and is refused, with the
store's reason, while it cannot. `turn_started` and the user's message are
written before the prompt is acknowledged, so a broken log refuses the prompt
itself. The failure is logged once per turn, naming the store's error, never a
payload. **Each session's log gates that session's effects:** a sub-agent
whose log fails stops, and its parent's turn goes on, its own log intact,
with a result saying the sub-agent stopped because its log could not be
written; nothing of that sub-agent's worktree is merged into the shared
folder (the result keeps it as a ref to apply by hand), nor when the parent's
log has failed. **A restore (undo, rewind) saves the folder first:** its
snapshot is written as `checkpoint{before_restore}` before any file changes,
and a failed snapshot or append restores nothing (T33). **A restore never silently reverts another chat's work (T57):** files another chat in an overlapping folder (or a chat deleted since, through its write tombstone) wrote, or may have written, during the undone span refuse an unforced restore, naming the files and nothing of that chat, unless the undoing chat's own `edit`/`write` chain explains the whole change; a forced one tells that chat which of its files were put back. **A restore is all or
nothing as far as git allows, and a damaged checkpoint store repairs itself
(A.16, T33):** before the folder is touched or anything is logged, every blob
the restore writes is checked and copied into the checkpoint repository's own
objects; a checkpoint that depends on content the user's repository no longer
has is refused, with that reason, with nothing changed and nothing logged, and
an unforced restore that would delete files no checkpoint holds is refused. A
snapshot whose `write-tree` fails rebuilds the checkpoint index from the
folder once and retries. **Undo does not depend on the user's repository
keeping old versions (A.17, T33):** at the end of each turn the old versions
of the files it changed or deleted, and the trees a diff of the turn reads,
are copied into the checkpoint repository's own store (a pack with a `.keep`
file naming the turn, released when the turn's checkpoint is pruned); content
already gone when the turn ends (the agent rewrote and pruned history inside
its own turn) cannot be copied and the chat says so, naming the files.
**Garbage collection is rare (A.18):** dropping old checkpoints' refs stays
on every turn; the checkpoint repository's `git gc` runs only after 16
dropped checkpoints (or git's own loose-object and pack thresholds), at most
once in 5 minutes, after the turn has ended, alone: snapshots, copies and
restores wait for a running one, and it waits for them.
**Nested
repositories are outside checkpoints:** what git itself treats as a
repository below the folder (a directory its walk will not enter, which
`ls-files -o` lists as `dir/`: clones, submodules, worktrees, `git init`
without a commit; or a gitlink from an older snapshot) is never in a
snapshot, not as a gitlink and not as files, and never makes a snapshot
fail; a restore skips exactly what a snapshot (before, after, or the folder
just before the restore) recorded as left out. A `.git` git does not accept
(empty directory, junk file, dangling link) changes nothing, and a repository
made over files the index tracks is entered by git, so those files stay
recorded and undoable: a planted `.git` cannot hide a turn's changes. Git
never stages an entry named `.git`, so no restore writes or deletes one.
No git runs inside a nested repository (T33). **A restore holds its
folder:** a node-wide registry (one mutex, never held across an await)
records the folder each restore writes; a restore registers only when no
other session has a turn running in an overlapping folder (same, containing
or contained, real paths) and no overlapping restore is registered, and a
turn (a sub-agent's too) does not start in an overlapping folder while one
is (T33).

### Fold

`harness_core::SessionView::apply(&RawEvent)` is the single reference reducer
(messages, tool cards, pending approvals, turn status). The SPA has a TypeScript
port of the same reducer. "Replay to exact state" is tested as:
`fold(all events) == fold(events seen before crash ++ events after reconnect)`
with the seq sequence strictly contiguous.

**A payload of the wrong shape never breaks the TS fold** (ADOPTION_PLAN
A.25): the master relays a node's payload unread, so `web/src/fold.ts`
checks each one where it enters, against a table of each kind's fields that
`web/src/schema.test.ts` compares with the schema serde reads
(`web/src/__fixtures__/schema.json`, written by
`crates/core/tests/browser_schema.rs`, which fails when it is stale). A
payload that is not an object, a field serde requires that is missing or of
the wrong type, or an enum value outside its list, drops the event into
`unknown_events`, as in the Rust fold, with one console line naming only its
kind and seq. **A deliberate difference:** an optional field or an array
element of the wrong type is left out and the event still shows, where the
Rust fold drops the whole event; only a buggy or hostile node causes it, and
then the chat shows an event the server's views count as unknown. Fields it
does not read are left out. The view's maps keyed by a node's id are written
and read as own keys, and so is every table a row looks up by a string from
an event, a model, a node or the server (`own` in `web/src/emptyView.ts`).
What is kept as sent (a tool's `input`, an artifact's `meta`, a `ui:` block's
`spec`) is checked by the rows that read it: each `ui:` card's spec against
its shape (`web/src/ui/shape.ts`). A row that still throws costs that row
(`RowBoundary`; a waiting approval keeps only Deny), a screen that throws
that screen (`ScreenBoundary`). The fold loads with the chat's screen, not
with the first screen.

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
* `session.diff_result {req_id, diff?, error?}` — answers one `session.diff` (X.12), never sent unasked, at most `MAX_DIFF_JSON` (2 MiB)
* `session.background_output_result {req_id, output?, error?}` — answers one `session.background_output` (X.62, T36), never sent unasked, the last at most 28,000 bytes of one background command's output as plain text with its state; the master cuts anything over `MAX_BG_OUTPUT_BYTES` from the front
* `session.files_result {req_id, paths?, error?}` — answers one `session.files` (T63), never sent unasked, at most 50 paths relative to the chat's folder
* `project.created {req_id, path?, error?}` — answers one `project.create` (X.26, T77), never sent unasked: the absolute path of the folder the node made, or why it made none
* `project.mcp_result {req_id, list?, error?}` — answers one `project.mcp` (X.36, T82), never sent unasked: the folder's `.mcp.json` for the asking person (each entry's command, arguments, `env` names and values whole, its state, a digest to approve with), or why not
* `attached.request {req_id, session_id, request: {op: info|list|read, file_id, entry?, offset?, limit?, mode?}}` — an `attached_file` call of a session this node runs (X.54, T95): what to do and the file's id only, no user, workspace or chat; only from nodes announcing `attached-file`; answered by one `attached.result`
* `board.request {req_id, session_id, request: {action: list|add|move|note, …}}` — a `board` or `board_edit` call of a session this node runs (X.32, T78): what to do only, no project, workspace, user or chat; only from nodes announcing `board`; answered by one `board.result`

Master → node
* `master.welcome {protocol_version}`
* `session.create {req_id, session_id, title, cwd, model?}` — master mints the uuid v7
* `session.prompt {req_id, session_id, text, files?}` — `files`: paths the person mentioned with `@path` (T63; at most 64, 1,024 bytes each, only to nodes announcing `files`): names only, the node reads them
* `session.cancel {req_id, session_id}`
* `approval.respond {req_id, session_id, approval_id, decision}`
* `resume {session_id, last_seq}`
* `session.retry {req_id, session_id, turn_id, agent?, savings?, context?}` — a person's Retry of the chat's failed last turn (X.21, T69); only to nodes announcing `retry`, only from `POST /api/sessions/{id}/turns/{turn}/retry`; no text, files or images (the node's log has the message); refused unless `harness_core::retry::check` passes against the node's log, and while a turn or `/compact` runs
* `session.diff {req_id, session_id, turn_id, rewind?, all?}` — a turn's changes from its checkpoints (X.12, T53); with `all` (turn_id empty) the whole chat's changes, from its oldest kept checkpoint to the latest finished state (X.61), only to nodes announcing `diff-all`; only to nodes announcing `diff`; never acked, nothing logged
* `session.background_output {req_id, session_id, bg_id}` — the tail of one background command's output for the chat's strip (X.62, T36); only to nodes announcing `bg-output`; never acked, nothing logged or stored; the node reads the ring `bash_output` reads
* `session.files {req_id, session_id, query}` — the composer's `@` completion (T63); only to nodes announcing `files`; never acked, nothing logged
* `project.create {req_id, name}` — a new project's folder (X.26, T77): a display name only, no place; only to nodes announcing `projects`; never acked, answered by one `project.created`; nothing logged on the master
* `project.mcp {req_id, user_id, folder, request: {op: list} | {op: approve, name, digest} | {op: remove, name}}` — a person's request about a project folder's `.mcp.json` (X.36, T82); only to nodes announcing `project-mcp`, only from a person's HTTP call; names no program; never acked, answered by one `project.mcp_result`
* `session.upload` and `session.prompt.uploads` (X.53, T94; capability `uploads`, with `upload-max:<MiB>` and `uploads-per-message:<n>` from node.toml `[limits] upload_max_mb` (10) and `uploads_per_message` (10)): a file the model cannot read as text goes to the chat's node in acked chunks of at most 256 KiB (`MAX_CHUNK`), one in flight, in the session's lane ahead of the prompt, which names it by `{upload_id, name, mime, size, sha256}`; the node stages it in `<data>/uploads-stage` (protected, emptied at start, 16 files, an hour), checks size and sha256, and saves it into the folder under its own name rules; the master follows the announced limits and sends only to a node that listed the capability ("update the node" otherwise); migration 4 adds `chat_uploads` (owner-keyed rows on the owner's blobs, held while a send uses them, deleted when the node took the message, purged by age).
* `owner` on `session.prompt`, `session.retry` and `session.callback` — the chat's owner (its user id), set by the master in `send_command` from the chat's row whoever built the command (X.36): the node keeps it in memory for the chat's table of repository servers; nothing records it
* `attached.result {req_id, answer?, error?}` — answers one `attached.request` (X.54, T95): the file's description, a page of a listing or a window of its content, or why not; only to nodes announcing `attached-file`; never acked
* `docs.request {req_id, session_id, request: {kind: doc|sheet, name, markdown|spec, page?, landscape?, title?} | {kind: view, file, sheet?, from?, limit?} | {kind: doc_edit, file, edits[], save_as?} | {kind: sheet_edit, file, sheet, edits[], save_as?}}` (X.56, T97; the last three X.57, T98, additive: `file` is the id of one of the chat's own files) — a `doc_create`, `sheet_create`, `doc_view`, `doc_edit` or `sheet_edit` call of a session this node runs in a chat without a folder: the file's content and a name only, no user, workspace or chat; only from nodes announcing `doc-create`; answered by one `docs.result`
* `docs.result {req_id, answer?, error?}` — answers one `docs.request` (X.56, T97): `{file_id, name, bytes, sha256, mime, notes[], text?}` of the file the master made and keeps (not yet the chat's: its log names it; a view answers with `text` and no file), or why not; only to nodes announcing `doc-create`; never acked
* `session.prompt.attached` (X.54, T95; capability `attached-file`): the references `{file_id, name, mime, size, sha256}` of the files the master keeps for a plain chat; no byte travels; the node records them in `user_message.attached` after its own checks
* `board.result {req_id, answer?, error?}` — answers one `board.request` (X.32, T78): the board (`list`) or the change made, or why not; only to nodes announcing `board`; never acked

**Replication rule (master side):** for an incoming event with
`seq == mirror_head + 1` → store and fan out; `seq <= mirror_head` → compare
with the stored event at that `seq` (kind, timestamp, payload as parsed JSON,
so key order does not count): equal → drop (duplicate), different → the
session has **diverged**; `seq > mirror_head + 1` → drop and send `resume{last_seq:
mirror_head}` (at most one outstanding per session; the mark is cleared
whenever a snapshot batch does not all land or reaches the announced head,
so a short snapshot never blocks later resumes). On hello, the master
sends `resume` for every session of **that node** where `node.head_seq >
mirror_head` (not for a diverged one), and creates mirror rows for sessions
it has never seen; an entry naming another node's session, live or deleted,
gets nothing (no `resume` that would reveal its length, no `session.delete`).
The hello's list is read in batches of 256 sessions, one store read each,
and the pass stops as soon as the node has a newer link, so reconnects do
not stack passes. Re-sent deletes are keyed by node and session.
This makes the link self-healing with no ordering assumptions beyond
per-session FIFO.

**Divergence is sticky.** Two writers under one `seq` mean two processes hold
one node key, or a node came back from an old disk; the master cannot tell
which log is right, and refusing only the one frame would let the second
writer's next in-order frame through and mix the logs. So the first differing
frame records `sessions.diverged_seq` (in the same transaction), and from then
on every append for that session is refused, whoever sends it, until an
administrator deletes the chat (no repair flow). The master logs it once, at
`error`, with session, node and `seq` and no content, sends the node
`session.cancel` for it (again on later refused frames, one in flight at a
time), skips it on hello, and refuses every command that would make the node
write to it (`prompt`, an allowing `approval.respond`, `restore`, `set_agent`,
`select_variant`, `artifact.stage`: 409 "diverged"); cancel, delete and a deny still
go through. **A revoked node's frames are refused in the same transaction**
that would store them (`nodes.revoked_ms` checked per append), so the up to
eight frames already queued when the revoke lands are not applied. Payloads
are compared after a JSON round trip on both ends, so serde_json is built
with `float_roundtrip` (without it a float re-read from the node's log can
come back one ULP off and a plain resend would read as divergence). A full
fence (a generation per connection, an epoch per session) comes with the
features that move sessions between writers (ADOPTION_PLAN C.1, N.5).

**The row is set at `seq` 1 and its owner never changes.** The mirror's
`sessions` row is the session's creation record: `session_created` sets it
only as the session's first event. A `session_created` at any later `seq` is
**refused** as malformed (`AppendOutcome::Refused`, nothing stored) and the
session is frozen exactly as a diverged one (`Store::freeze`, logged once at
`error` without content, `session.cancel`, commands refused). Refused rather
than stored inert: no node writes one, every fold may rely on one creation
record per log, and a node that can freeze its own session gains nothing.
No event from a node changes `owner_id` once it is set; only the master's
`claim_session` stamps it after the create. `incognito` and `profile` are
fixed at creation. A node's own log applies the same rules, so an honest
node fails its own append first and never sends such an event.

**A parent is followed only under the same node and owner.** A sub-agent's
session names its parent in `session_created.parent_session`. The mirror
stores it only when the parent is another session of the same node; the
child then takes the parent's owner and workspace. When the master created
the session (`session.create`, which never carries a parent), its stamp
(`claim_session`) sets the owner it recorded for the create and drops a
parent whose owner differs, with the workspace mapping inherited from it.
A dropped parent is logged once at `warn` with the ids. Every place that
follows `parent_id` (the project and its context in `Master::prompt`,
`remember` proposals, a turn's memory switches, a routine run's sub-agent
approvals) checks node and owner again, so a parent that does not qualify
gives a plain top-level session with no project.

**Events and the row are bounded.** `Store::append` refuses
(`AppendOutcome::Refused`, nothing stored) an event whose payload JSON plus
kind passes `MAX_EVENT_BYTES` (4 MiB, half a frame; the largest legitimate
event is a `user_message` with its attachments, about 2.5 MB at worst, and
`Master::prompt` refuses a message that would not fit with 413), a kind
over 64 bytes, and in `session_created` a `cwd` over 4096 bytes, a `model`
over 256, an agent id over 64 (also in `agent_changed`) or a profile other
than `chat` and `agentic`: identifiers and paths are refused, not cut, since
a cut one names something else. The title is free text and is cut to 200
characters in the row and the search index (the event keeps it whole). On
the mirror a refused event freezes the session as above; in a node's own
log it fails that append, which stops the turn (tool output is cut to
30,000 characters and model output by its token limit, so the session actor
does not reach the cap in practice). Listings filter by owner in SQL
(`Store::sessions_of`).

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
node's own outgoing queue (256 messages and 1 MiB, `node/src/outbox.rs`)
makes event publishers wait; it never drops an event, and if the link dies
while they wait, the next hello and `resume` replay from the node's log. The
acks of the commands the node answers inline (cancel, approval answer,
`stop_background`) go ahead of that queue and never wait (at most 64 kept,
past that dropped), and the node's socket holds at most 128 KiB unsent
(`TCP_NOTSENT_LOWAT`), so a Stop is acknowledged within about a second on a
full 256 KiB/s uplink (A.21, T9). **The Stop's target:** the command ends at
once, within a fraction of a second of the node reading the Stop, however
full the uplink is: the `bash` tool kills the command's process group (or
its guard's namespace) when it sees the cancel; the agent loop's
`TOOL_CANCEL_GRACE` (5 s) is only the worst case, a tool that does not end
by itself (`node/tests/control_under_load.rs` `STOP_ROUNDS` holds a Stop to
it, counted in the command's own loop rounds so load cannot flip it). Frames are at most 8 MiB (`MAX_FRAME`,
refused from the header); `session.snapshot` batches are at most 500 events
and 512 KiB. Idle detection counts only time spent waiting on the peer, so a busy
link is not declared dead. The node gives up on a TCP connect after 10 s and
on TLS plus upgrade, and on the challenge exchange, after 10 s each.

---

## 4. Threat model

The master can make nodes run shell commands. Treat it as an RCE service with a
login page.

**Assets:** shell on every node; source code; provider API keys; the UI login.

**Adversaries and mitigations**

Each row's full text is in `docs/threats/`.

| # | Threat |
|---|---|
| [T1](docs/threats/T1.md) | Network attacker reaches the master UI |
| [T2](docs/threats/T2.md) | Malicious website in the user's browser (CSRF, DNS rebinding, cross-site WS) |
| [T3](docs/threats/T3.md) | Secrets leak via logs, URLs, event log |
| [T4](docs/threats/T4.md) | Prompt injection from repo content makes the model run harmful commands |
| [T5](docs/threats/T5.md) | Compromised master widens node permissions |
| [T6](docs/threats/T6.md) | Compromised master approves things on nodes |
| [T7](docs/threats/T7.md) | Rogue node / stolen node key; two processes with one key (a copied data directory, a node restored from an old disk) |
| [T8](docs/threats/T8.md) | MITM on the node link |
| [T9](docs/threats/T9.md) | Runaway processes |
| [T10](docs/threats/T10.md) | Another account reads or drives someone else's work |
| [T11](docs/threats/T11.md) | A low-trust account gets a shell |
| [T12](docs/threats/T12.md) | Password guessing / account takeover |
| [T13](docs/threats/T13.md) | Credentials sniffed on the network |
| [T14](docs/threats/T14.md) | Model output (possibly steered by injected web or repo content) phishes, exfiltrates or poses as the harness through what the chat renders |
| [T16](docs/threats/T16.md) | Routines run without a human present, partly on external webhook data: injected text steers an agent or uses its web tools as a relay, a revoked user's… |
| [T17](docs/threats/T17.md) | Files stored and served from the app origin (uploads, published builds) run script, phish, crash the viewer's tab, or outlive their deletion |
| [T18](docs/threats/T18.md) | A compromised node or steered agent uses publishing to fill the master's disk, pull secrets, plant files in another user's artifacts, or slip files past… |
| [T19](docs/threats/T19.md) | A working copy arrives with a hostile git configuration (an unpacked archive, a shared folder): `.git/config` names `core.fsmonitor`, hooks, filter, diff or… |
| [T20](docs/threats/T20.md) | A project member plants instructions in every other member's agent through shared project memory (cross-user prompt injection, audit S12) |
| [T21](docs/threats/T21.md) | Slow or numerous clients hold the HTTP server's connections and handlers… |
| [T22](docs/threats/T22.md) | A prompt-injected model uses the harness's network clients as a relay or a way inward: `web_fetch` to an internal address or to a URL it built to carry data… |
| [T23](docs/threats/T23.md) | Any account (a chat-only guest included) uploads a hostile file the server parses: a PDF that loops, inflates without bound, overflows the stack, panics or… |
| [T24](docs/threats/T24.md) | An admin session (a careless admin, a stolen login, a second admin) uses the provider and model tests to read the server's own network: cloud metadata, a… |
| [T25](docs/threats/T25.md) | A response leaks internals or outlives its session: a 500 with SQL text or a path, an API answer or stored file left in the browser's disk cache after logout… |
| [T26](docs/threats/T26.md) | Another local user reads a secret file the server uses (a restored backup, a copied `.env`, a key file with a loose umask) (audit L7) |
| [T27](docs/threats/T27.md) | An agent runs with more power than the one named: a sub-agent-only agent made a chat's agent (by `agent_id`, `@mention`, `set_agent`, a default or a routine)… |
| [T28](docs/threats/T28.md) | The node's store cannot append (full disk, I/O error, a corrupted or locked database, perhaps brought about on purpose by a command that fills the disk), and… |
| [T29](docs/threats/T29.md) | A script, or anyone who gets a copy of the one static API token, reads or drives every user's chats… |
| [T30](docs/threats/T30.md) | A crash, `kill -9`, full disk or cancelled turn in the middle of `edit` or `write` leaves the user's file truncated or half-written, with nothing to restore… |
| [T32](docs/threats/T32.md) | A sub-agent that can change files runs in the parent's folder (the default since 2026-11-21) or, for `own`, is not started when its worktree could not be made… |
| [T33](docs/threats/T33.md) | An undo or rewind overwrites work no checkpoint holds: the user's own edits since the turn, or anything else changed in the folder, discarded by "Undo anyway"… |
| [T34](docs/threats/T34.md) | The next person on a shared browser reads what the last one typed and never sent, or an incognito chat's words outlive it on disk (audit L14) |
| [T36](docs/threats/T36.md) | A steered agent keeps a process running or keeps acting after the person left: a listener, a miner, a chain of unattended turns, output that steers a later… |
| [T37](docs/threats/T37.md) | A program of the same user (an agent's shell command, foreground in an `auto` turn or a background command where they are on, with the network on) binds a… |
| [T38](docs/threats/T38.md) | Text the person did not write is carried into a compaction summary as their request, or as the assistant's own words: a file, a command's output or a fetched… |
| [T39](docs/threats/T39.md) | Commands of a chat run with more power than the chat, or its owner, was given: the master (or a member, through it) asks a node for an execution mode it… |
| [T40](docs/threats/T40.md) | A stuck or steered model repeats one call until the step cap: the same failing command, the same read, the same denied approval asked of the person again and… |
| [T41](docs/threats/T41.md) | A file attached to a message stands as the person's own words in what the model is sent: its text closes the frame it rides in and goes on… |
| [T42](docs/threats/T42.md) | An older build opens a database a newer build wrote (a downgrade, a node update rolled back, a copied data directory) and acts on it without what it cannot… |
| [T43](docs/threats/T43.md) | Text the person did not write stands as theirs in a user message (review of pull request 76, R76-1 and R76-3): the **per-turn context**… |
| [T44](docs/threats/T44.md) | The repository's instruction files (`AGENTS.md`, `CLAUDE.md`; ADOPTION_PLAN X.2), read by the node on every agentic turn and sent to the model, are a channel… |
| [T45](docs/threats/T45.md) | The agent overwrites changes it never saw: it rewrites a file whole, or edits it, from a picture of the file that is out of date… |
| [T46](docs/threats/T46.md) | Skills (ADOPTION_PLAN X.2; since X.34 step B, the owner's decision of 2026-11-17, the catalogue also takes text skills the master stores, T81, so skill… |
| [T47](docs/threats/T47.md) | An MCP server (ADOPTION_PLAN X.3) is a program the harness did not write, started on the node for a chat: a prompt, a master, a repository's `.mcp.json` or… |
| [T48](docs/threats/T48.md) | The environment line (ADOPTION_PLAN X.8): the node's clock, OS and the session repository's git branch reach the model in every agentic turn's message, and… |
| [T49](docs/threats/T49.md) | **Memory saves without a click** (owner's decision 2026-11-14, ADOPTION_PLAN T.3): a prompt-injected agent plants a false or steering fact that every later… |
| [T49](docs/threats/T49-2.md) | **Memory saves without a click** (owner's decision 2026-11-14, ADOPTION_PLAN T.3): a prompt-injected agent plants a false or steering fact that every later… |
| [T50](docs/threats/T50.md) | A chat runs edits or commands without a click that its node or its owner did not allow… |
| [T51](docs/threats/T51.md) | A person's answer does more than they meant, or someone else's does: a "for this chat" grant covers another call, another chat, a sub-agent, a background… |
| [T52](docs/threats/T52.md) | **The context block is in the log** (ADOPTION_PLAN X.1): to keep the provider's cache across turns, what a turn sends besides the person's words… |
| [T53](docs/threats/T53.md) | A turn's diff (X.12) shows file contents of a chat's folder to someone who is not its owner, carries the node's secrets or a file's content into a log, an… |
| [T54](docs/threats/T54.md) | **A workspace's shared memory** (the owner, 2026-11-14: "Workspace memory available to all users in it"; ADOPTION_PLAN T.3): one memory per workspace… |
| [T55](docs/threats/T55.md) | **Searching the person's earlier chats**… |
| [T56](docs/threats/T56.md) | A chat runs far longer than its owner meant (ADOPTION_PLAN X.14): the default step limit is 1,000… |
| [T57](docs/threats/T57.md) | An undo or rewind silently reverts work the turn did not do: two chats work in one folder (two people of a team, or one person with two chats), chat B changes… |
| [T58](docs/threats/T58.md) | A member learns when someone else works (a timing leak against "chats are private to their owner, in every workspace, for admins too"; live test, 2026-11-15)… |
| [T59](docs/threats/T59.md) | **Shortening what the model is sent inside a turn** (ADOPTION_PLAN X.15, `docs/turn-compaction.md`): old tool results are replaced by a stub and a turn's… |
| [T60](docs/threats/T60.md) | **Hand-off notes** (ADOPTION_PLAN X.16, `docs/handoff-notes.md`): text one chat's model writes that a later chat's model reads. (1) Injection that persists: a… |
| [T62](docs/threats/T62.md) | **The node operator's instruction file** (ADOPTION_PLAN X.2; node.toml `[instructions] user_file`, default `AGENTS.md` in the node's data directory): text the… |
| [T63](docs/threats/T63.md) | **`@` file mentions** (README "Mentioning a file"): the person types `@path` and the node puts that file's content into the message. (0) A pasted `@token`… |
| [T64](docs/threats/T64.md) | **A person's `/compact`** (ADOPTION_PLAN X.20): a summary of the chat made between turns at its owner's request, with words of theirs on what to keep. (1)… |
| [T65](docs/threats/T65.md) | **Instruction files of subfolders** (ADOPTION_PLAN B.9, X.2; T44's neighbour for folders below the session folder): `web/AGENTS.md`, `crates/tools/CLAUDE.md`… |
| [T66](docs/threats/T66.md) | **The read-only shell** (ADOPTION_PLAN T.7, docs/read-only-shell.md): a command that must not change anything (plan mode; read-only agents, T70) writes the… |
| [T67](docs/threats/T67.md) | **Plan mode** (ADOPTION_PLAN X.7, README "Approval modes"): a chat set to plan changes something anyway: a named `write`, `edit`, `publish`, `remember` or MCP… |
| [T68](docs/threats/T68.md) | **Following up a sub-agent** (ADOPTION_PLAN X.22; `delegate` with `session`): a parent sends a sub-agent it already ran its next message, and the child… |
| [T69](docs/threats/T69.md) | **A person's Retry of a failed turn** (ADOPTION_PLAN X.21, README "Retry"): the turn runs again. (1) Someone else retries a chat's turn, or a disabled or… |
| [T70](docs/threats/T70.md) | **One rule for "this agent can change things", and a shell for read-only agents** (ADOPTION_PLAN T.7): before, three places judged differently… |
| [T71](docs/threats/T71.md) | **Images `read` returns** (ADOPTION_PLAN X.23, README "Images" under the file tools): the agent reads a PNG, JPEG, WebP or GIF of the chat's folder and the… |
| [T72](docs/threats/T72.md) | **Background sub-agents** (ADOPTION_PLAN X.24, `docs/background-commands.md` §14): `delegate` with `background: true` returns at once and the child runs on… |
| [T73](docs/threats/T73.md) | **The model's thinking is kept and sent back** (ADOPTION_PLAN X.25, README "Thinking"): each step's thinking is recorded with it… |
| [T74](docs/threats/T74.md) | **Profile pictures, About you and personal instructions** (ADOPTION_PLAN X.29, README "Profile and Personalisation"): a person uploads an avatar and a banner… |
| [T75](docs/threats/T75.md) | **The stream scrubber's hold-back as a side channel** (ADOPTION_PLAN X.30, `crates/core/src/secret.rs` `SEEN_PREFIX`): a streamed answer or command output is… |
| [T76](docs/threats/T76.md) | **Checkpoints no longer follow the user's global git ignore file** (owner, 2026-11-17, ADOPTION_PLAN X.31): a file only `core.excludesFile` or… |
| [T77](docs/threats/T77.md) | **A node makes folders on request** (ADOPTION_PLAN X.26, README "Projects", `docs/projects.md`): the start screen's "New project" asks a node for a new… |
| [T78](docs/threats/T78.md) | **Agents change a project's task board** (ADOPTION_PLAN X.32, README "Agents on the board", docs/projects.md "Task board"): `board` lists a project's board to… |
| [T79](docs/threats/T79.md) | **A workspace's own agents** (ADOPTION_PLAN X.33, README "Workspace agents", docs/agent-pages.md "Workspace agents"): whoever holds a workspace's `settings`… |
| [T80](docs/threats/T80.md) | **Which agents of a workspace may use which skill or MCP server**… |
| [T81](docs/threats/T81.md) | **Skills stored on the master**… |
| [T82](docs/threats/T82.md) | **A repository's own MCP servers**… |
| [T83](docs/threats/T83.md) | **Sealed storage and a person's forge connections**… |
| [T84](docs/threats/T84.md) | **A workspace's custom connectors and each person's account**… |
| [T85](docs/threats/T85.md) | **Agents call a custom connector's tools through the master**… |
| [T86](docs/threats/T86.md) | **Cloning a forge repository into a new project's folder**… |
| [T87](docs/threats/T87.md) | **One message calls several agents by mention**… |
| [T88](docs/threats/T88.md) | **The agent's commits carry the person's identity** (the 2026-11-19 readiness trial, item 3; README "What an agent's commits are signed with"): every… |
| [T89](docs/threats/T89.md) | **An agent that holds a repository's files and a shell's output also has the web**… |
| [T90](docs/threats/T90.md) | **A person's rewind of the conversation changes what the model is sent, and what a chat's log means, by one event**… |
| [T91](docs/threats/T91.md) | **An agent reads its own chat's earlier turns in full**… |
| [T92](docs/threats/T92.md) | **A person's fork of a chat copies its log into a new chat that can run commands in the same folder**… |
| [T93](docs/threats/T93.md) | **A person's bookmark on a message is one more event the owner's click writes into a chat's log**… |
| [T94](docs/threats/T94.md) | **A person's file the model cannot read is written into a chat's folder by the node**… |
| [T95](docs/threats/T95.md) | **A person's file the model cannot read is parsed by the master for an agent in a chat that has no folder**… |
| [T96](docs/threats/T96.md) | **A person's Word, Excel, PowerPoint or OpenDocument file is parsed so that the model can read its text**… |
| [T97](docs/threats/T97.md) | **An agent makes a Word or Excel file from its own text, and that file is written into a folder or kept by the master for a person to open**… |
| [T98](docs/threats/T98.md) | **An agent edits a Word or Excel file somebody else wrote, and the result is a new file the person opens** (`doc_view`, `doc_edit`, `sheet_edit`): the file is hostile and is parsed only in the confined `doc-edit` helper, one part is patched by byte ranges and every other entry is copied as stored, hidden content is not addressable, an edit that would corrupt the file is refused, and all edits apply or none and are read back… |
| [T99](docs/threats/T99.md) | **A person previews a Word or Excel file in the page** (X.58): the route takes the download's checks (owner only, a plain chat's own logged file, the owner's blob), the file is read only by the confined `doc-edit` helper under the account's one slot and comes back as bounded structure and strings, never HTML, drawn only as text with no link made from the file, and every failure is a plain sentence… |
| [T100](docs/threats/T100.md) | **A person's request makes the harness write a file** (X.60): the page sends changes to a Word or Excel file and a copy is made, held on the master or written by the node beside the original; both routes are POSTs behind the Origin guard and the checks of the preview, nothing names a destination, each change is one of the edit tools' own operations in the confined helper, the node decides whether it writes at all and writes only through the create-only write of `doc_edit` to a name it makes, and nothing a helper, node or page says is trusted or forwarded… |

Numbers T15 and T31 were never used; rows are never renumbered.

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
* **Sealed secrets** in `master.db` (`crates/master/src/sealed.rs`, T83): a
  person's forge tokens, sealed with AES-256-GCM under `<data>/keys/secrets.key`,
  bound to their row by associated data. Provider keys stay in `secrets.toml`
  until they move (docs/nodes-credentials-environments.md §4.1).
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
  the bytes on disk (no eol/filter conversion), which makes undo exact. No object, ref,
  index or config is ever written into the user's repository; when a snapshot hashes a
  file whose content that repository already has, git updates the modification time of
  the existing object file there ("freshening"), which is one more input to O11 (whether
  checkpoints should stop borrowing). Ignore rules are
  the repository's own: its `.gitignore` files (read by git from the work tree) and
  `.git/info/exclude` (copied into the shadow's `info/exclude` at every checkpoint when it
  is a regular file reached by no link, at most 1 MiB); a plain folder gets the shadow's
  fixed list (`node_modules/`, `target/`, `.venv/`, …). The user's global ignore file
  (`core.excludesFile`, `~/.config/git/ignore`) is **not** followed (owner, 2026-11-17,
  ADOPTION_PLAN X.31): before, a file only it named (`.env`, `.idea/`) was in no
  checkpoint, so Undo could not put back an agent's edit to it and nothing said so; a
  sub-agent's folder lacked it. Cost and exposure: T76. Checkpoint
  commits have no parent. **A sub-agent that can change files gets its own worktree or
  is not started:** "can change files" means its tools, after its own denials and the
  node's `denied_tools`, include `write`, `edit` or `bash` (no node shell is read-only).
  When the folder is not a git repository, or the worktree cannot be made (a hostile
  repository config, `worktree add` failing, the worktree outside `workspace_roots`), the
  delegation fails before the child session exists, with a result that says why and what
  would work; whatever was half made (worktree, base ref) is removed. There is no
  fallback to the shared folder and no setting for one (T32). A sub-agent that only reads
  works in the parent's folder. A sub-agent's temporary worktree gets no turn checkpoints (it is
  deleted when the sub-agent ends). Sub-agent base/result refs stay under `refs/harness/`
  in the user's repository (`worktree add` needs the commit there) and are pruned to the
  latest `checkpoints_keep`. Every restore first snapshots the folder into a checkpoint of
  its own (`restore-<id>`, `before` and `after`), logged before any file changes, so a
  restore can be undone by restoring that checkpoint; it counts against
  `checkpoints_keep` like a turn (2026-11-11, A.9, T33). A git repository nested in the
  folder (as git itself recognises one) is outside checkpoints: never snapshotted, never
  touched by a restore, named in `checkpoint.excluded`, and in a note after an undo when
  one appeared or went away during the turn; a planted `.git` hides nothing. A restore
  holds its folder in a node-wide registry: it is refused while another chat has a turn
  running in the same, a containing or a contained folder, and no turn starts there while
  it runs (2026-11-11, A.15, T33). Checkpoints keep borrowing the user's objects (O11), a
  restore first copies the blobs it writes into the shadow and is refused, with the reason
  and nothing changed, when one is gone; a checkpoint index that names a pruned blob is
  rebuilt once from the folder (2026-11-11, A.16, T33). At the end of each turn the old
  versions of the files it changed, and the trees a diff of it reads, are copied into the
  shadow's own store as a pack with a `.keep` file, released when the turn's checkpoint is
  pruned, so undo no longer depends on the user's repository keeping them (2026-11-11,
  A.17, T33). The shadow's `git gc` runs rarely (16 dropped checkpoints, git's auto-gc
  thresholds, at most once in 5 minutes), after the turn has ended, holding the shadow
  alone (snapshots, copies and restores wait for it); a timed-out snapshot leaves no lock file
  (2026-11-11, A.18). Rejected: copying at the start of a turn (which files will change is
  not known; copying the tree or intercepting writes costs more than it closes).
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
* **Approval modes:** `plan` (nothing changes: refused, not asked; `bash`
  in the read-only shell; T67), `ask_all` (every tool), `ask` (mutating
  tools), `accept_edits`, `auto` (the default, T50). Decisions: `allow`, `deny`, `allow_session` (this exact call, for
  the rest of this turn, not the session; the UI says "Allow this exact call
  for this turn"). "This
  exact call" is the tool and its canonical argument
  (`harness_tools::call_key`): for `bash` the command string byte for byte
  with its other arguments (no parsing, so `ls` never covers `rm -rf x`);
  for `write` and `edit` the real path they write (two spellings of one file
  are one call) and the rest of the input (another edit to that file is
  another call); for every other tool its input as JSON with sorted keys.
  It is offered (`approval_requested.turn_allow`) and honoured from the
  first request for a call, because it covers only the call on that card,
  which the person is looking at. The approval id is bound to that one
  waiting request, and the loop records the key of the call it is running,
  so an answer cannot cover any other call. A single `allow` covers that
  one call only. A sub-agent starts with no allowance. A call `node.toml`
  denies is resolved by the node with `turn_allow` false and never reaches
  a person, so it is never allowed.
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
* **Services the master answers (2026-11-17, an invariant reworded with the owner's yes).**
  The rule was "the master never runs tools". The Gate 3 review could not confirm it as
  worded (`docs/security-review/2026-11-gate3.md`: the chat runtime's web and memory tools
  run in the server's process), and `recall` (X.13) already works another way: the node's
  tool files a typed query over the link and the master answers from the session alone.
  The rule now says what is meant: **the master runs no shell, no file tool and no program
  the model names; it may answer typed requests to a fixed list of built-in services, each
  a per-agent switch.** What a service must keep: the request names no user, workspace,
  project or chat (the master takes them from the session); the tool stays a node tool
  (its schema, risk class and approval are the node's, and a node may deny it in
  `node.toml`); the answer is bounded and is data for the model, never an instruction to
  the node; the tool result is appended by the session's own node (one writer). Built:
  `recall` (X.13), the task board (X.32, T78: `board` and `board_edit`) and stored skills'
  bodies (X.34 step B, T81: the `skill` tool's `skill.request`, text only), the files a
  person attached to a chat without a folder (X.54, T95: the `attached_file` tool's
  `attached.request`, read-only, bounded, the file found only in the session's own log), the
  Word and Excel files an agent makes in a chat without a folder (X.56, T97: `doc_create` and
  `sheet_create` file `docs.request` with the content only, the master builds the file from
  bounded text, keeps it as the owner's blob and records a row that the chat's own log must
  name before it is served), and a
  workspace's custom connectors (X.35, T84 for the connectors and accounts, T85 for the
  calls): a node's MCP tool `mcp__<connector>__<tool>` files `connector.request` (the
  session, the tool, the arguments), answered by the master with the chat owner's own
  account in their own signed-in turns and their routines' runs, under the agent's
  Connectors switch and the Shared / Assigned rule, with the node's approval, the result
  text only and framed as any tool result; the master speaks MCP to the remote server
  itself, so the token never reaches a node. Where a
  connector runs is decided by where its data lives: a repository, files or a local network
  mean the node (forge accounts X.27, `node.toml` servers, project servers X.36); an online
  account means the master, so its token never reaches a node.
* **`history` is node code, not a master service (2026-11-21, X.47, T91).**
  An agent reads its own chat's earlier turns through a tool that runs on the session's
  node against the node's own store: no request over the link, no switch on the master.
  The conversation it reads is the model's own rebuild with compaction and shortening off
  (`Rebuild::unshortened`), so what was taken out of the model's sight on purpose (a rewound
  range, a refused image) stays out; old text is quoted behind `| `; every answer is inside
  the tool-result budget. It is a per-agent switch like the others (`Features.history`, on
  when absent).

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
  * **OPEN where the shell is not contained, and for a socket inside a
    grant: pathname Unix sockets before Landlock ABI 9 (Linux 7.1).**
    *Closed for the contained shell by T.8c* for every socket outside the
    grants: its file view does not hold them (T9). Where the shell is not
    contained (a `host` chat, T39), for the node's own helpers, and for a socket inside a granted folder (a
    `[sandbox] write` entry holding `/tmp` or `/run/user/<uid>` is warned
    about), on such kernels (Ubuntu 24.04's 6.8 among them) `connect()` is
    not mediated: `/run/user/<uid>/bus` (`systemd-run --user`
    runs anything unconfined), `/var/run/docker.sock` (`docker run -v
    /:/host`), an ssh-agent socket. Mitigation, the operator's job and in the
    README: run the node as a dedicated OS user without `docker` group
    membership and without a login session (no session bus). The test
    `pathname_unix_sockets_need_abi_9` records which side of the line a
    kernel is on. Through such a socket a service of the node user (the
    user's systemd) can also start a process outside the shell's
    containment, which outlives the command (T9, review T8A-1); `doctor`
    warns about it below ABI 9.
  * UDP is not covered by Landlock. *Closed for the contained shell by
    T.8b:* with `network = "off"` it runs in a network namespace of its own
    (only its own loopback; no TCP, UDP or DNS out, the host's abstract
    sockets absent). **Still open** where bubblewrap runs but cannot make a
    network namespace (`[execution] sandbox = "auto"` contains without it
    and warns: "network off WITH A GAP"), in a `host` chat (T39) and
    for the node's own helpers (graphify, the checkpoint git), which keep
    Landlock alone: there `network = "off"` stops TCP only, and DNS and any
    UDP exfiltration still work. `doctor` and the start-up log say so on
    the containment line. T.6 (seccomp) is what would close it there.
  * **Closed by N.10a part 1 (review T8B-3, Low): after the start-up log
    line nobody was told that the network is not really off.** The node
    announces the gap at hello (`gap:sandbox:netns` where bubblewrap could
    not make a network namespace at start, `gap:host:netns` for `host`
    chats with the network off), the master keeps it in its node list,
    and the chat header says it in plain words ("UDP and DNS still go
    out"). What is left: the hello is the node's state at connect; a gap
    the step up closes later is still shown until the node's next hello,
    and nothing is appended to tool results.
  * **Closed by T.8c (re-check R-3, Note): a program on `PATH` inside a
    `[sandbox] write` entry decided the containment probe.** The probes are
    now `sandbox-exec`'s own checks after it confined itself (no program
    found by name; `a_planted_program_on_path_does_not_decide_the_probe`),
    and `doctor` and the log warn about a `write` entry that holds a folder
    on the node's `PATH` (the node's helpers, and a later command, still
    find programs by name there). As it was: The probes run `sh`,
    `cat` and `readlink` by name with the node's `PATH`, inside the
    wrapper, which grants node.toml's `write` entries; one that holds a
    folder on `PATH` ahead of the system's (`write = ["~/.cargo"]`) lets a
    command leave a `readlink` that makes every later probe fail: the node
    stays in the gap, or `doctor` reports one. Only in the failing
    direction (a probe runs only after bubblewrap made the namespace). The
    same folder lets a command replace `bash` for later calls, inside the
    sandbox. Planned in T.8c: the probe becomes a subcommand of our own
    binary, and `doctor` warns about a `write` entry holding a `PATH`
    folder.
  * Metadata is not hidden where the shell is not contained: `stat`/`test
    -e` on paths outside work (names, sizes, times), only opening, listing
    and changing are denied. Contained (T.8c), a path outside the grants
    does not exist; the folders above a grant exist in the view holding only
    the way down to it (the grant's own path), and Landlock still refuses
    to list them. Without
    the shell's containment (T9; no bubblewrap, or its shared-`/proc`
    form) `/proc` lists other processes, their command lines and fd
    numbers (not their targets, `environ` or `mem`: ptrace-class access
    outside the domain is denied); contained, it lists only the command's
    own.
  * `/etc` is readable (world-readable files only; `shadow` and host keys
    stay root-only by file mode).
  * The session folder's own `.git` is writable from bash, so a command can
    plant hooks or config that run the next time the person runs git there
    (the file tools refuse writes under `.git`). Since R.1 this no longer
    reaches the node: no git call of the node that touches files reads that
    config (T19), and the checkpoint root is found on the file system, not
    from it (T33). A sub-agent works in a node-owned folder with a git
    directory of its own; its only grant on the user's `.git` is read-only
    `objects` (T32).
  * A relative `--data-dir` is made absolute where the binary reads it, and
    the sandbox refuses a relative temp or protected folder (R.1, review
    S-4: the wrapper runs in the session folder, where a relative path, and
    in `best-effort` mode a link planted there, would have granted the data
    directory). Tests: `tools/src/sandbox.rs`
    `a_relative_data_dir_is_refused_in_every_mode`, `cli/tests/data_dir.rs`
    `a_relative_data_dir_is_made_absolute_for_the_sandbox`.
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
* **R8** Anthropic thinking + tool use requires sending the signed thinking
  blocks back on the next request (Anthropic "Thinking", "Preserving thinking
  blocks": "within a tool-use turn, pass thinking blocks back"). Since X.25
  the history stores them (`assistant_message.thinking`) and `llm::to_genai`
  sends them back (T73), proven against local servers speaking the wire
  format, not against the real API: the live checks of 2026-11-16 through the
  owner's router could not run (the Anthropic account answered "credit
  balance is too low" to every request, twice that day). DeepSeek's
  `reasoning_content` was checked live (T73). One way around the effort gate stays: genai
  reads an effort from a model id's last part (`claude-x-high`, `-low`,
  `-max`; genai 0.7.2 `ReasoningEffort::from_model_name`) when none is set,
  and now that thinking is captured it also asks for `display:
  "summarized"`; no Anthropic model id ends that way today, and an admin
  who names one so turns thinking on by hand. So `effort` stays off for Anthropic
  models (the model list and the effort check still enforce it): turning
  thinking on by request (adaptive `thinking` + `output_config.effort` on
  4.6+ models, `budget_tokens` on 4.5 and older, a 400 on 4.7+) is not
  verified. Models that think by default (Messages API: "On by default" on
  the newer models) are replayed under the same rule. To lift: one live
  two-step tool loop per family (a 4.5 model with `budget_tokens`, a 5.x
  model adaptive), then relax `master::config::effort_allowed`.
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
* `crates/master/tests/it/mirror.rs`: an empty and a stale mirror both catch up on attach via hello→resume→snapshot.
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
* R8 (Anthropic thinking + tools replay): replay built (X.25, T73), effort still off until checked live; R7 (genai single maintainer).
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
  layer), Word, Excel, PowerPoint or OpenDocument file (X.55, T96: the same helper
  mode, `doc-text`; the file is kept as well). Text is extracted on the master (PDFs and
  documents in the confined `pdf-text` and `doc-text` helper
  processes since FIX_PLAN 3.4, T23), capped at 100k characters, and travels *inside* the
  `user_message` event so the log alone rebuilds what the model saw. Uploads are owned,
  single-use, and expire unsent after the incognito TTL.
* **Token meter**: `turn_ended.usage` gains `context_tokens` (input of the turn's last
  model call); the conversation header shows total tokens and current context size,
  as a share of the model's window with the point where the summary is written
  (`usage.window`, `usage.summary_at`), or "window unknown" with how to set it.
* **Rolling summary**: once the history (since the last summary) is estimated above
  `[chat] summarize_after_tokens` (default 24k; since 2026-11-10 the model's context window, see below), everything before the last two turns is
  summarised by the same model into a `history_summarized` event; later turns send
  system prompt + summary + recent turns. Skipped when the old part is small (otherwise
  two huge turns would trigger a summary call on every turn). The UI shows the latest
  summary as a collapsible marker. The summariser reads the old turns as a plain transcript
  in which every line of a message, a tool call or a tool result is quoted behind `| `
  under a label the harness wrote, tool results cut to 2000 characters (T38). **Shrink check:** each piece's summary is kept only if
  its estimated tokens are at most 90% of the summary so far plus the piece it replaces;
  otherwise it is discarded with no event and counts as a failure. **Breaker:** after two
  failures (failed request, empty or non-shrinking summary) in one turn, no further
  summary is requested that turn; an overflow then ends the turn saying summarising was
  tried and did not help. A new turn starts with the count at zero.
  **Inside a turn (X.15, 2026-11-15, `docs/turn-compaction.md`):** every request is checked
  before it is sent against the window less its reserve (or a configured line above it):
  past it, the oldest tool results are sent as a one-line stub (no model call) down to half
  that line, never in the newest steps (Hermes' tail); when that leaves less than a quarter
  of the line free, earlier turns are summarised (all of them, the two kept ones too) and
  then the turn's steps before the tail, into a summary sent as a message of its own right after the turn's opening. With a known window the count-based mask (`keep_tool_outputs`) does not act. Each cut
  is a `context_shortened` event, so requests rebuild from the log; a single turn larger than
  the summariser's window is read with its results cut down to 250 characters. With an
  unknown window nothing changes inside a turn.
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
  node). All delegations of one step run **in parallel**; a writer works in the chat's
  folder (`folder: "shared"`, the default) or, when asked for or the node says so, on its
  own copy from the same base, blind to the others; the `delegate` description and the Senior
  Teamlead's prompt tell the model to start only independent work in one step and work
  that builds on another child's result in a later one; caps are enforced per turn
  (default 4 sub-agents, 200k tokens) and shown in the tool description. Depth is one
  (sub-agents get no delegates). Cancelling the parent cancels its children. A call
  with `session: <child id>` sends a follow-up to a child the caller started (T68): the
  same child session gets a new turn (`turn_started.continued_by`), keeps its
  conversation, not its files, and counts against the same caps. The child's
  turn gets the task, the repository's instruction files and its parent turn's
  per-turn context from the master (quoted as the parent's is), never the parent's
  history. A child ends with `finish {status: done|partial|failed, summary, details?}`
  (offered to children only, handled by the loop, taken only alone in its message; no
  new event: a `tool_call_started`/`tool_call_finished` pair the node reads back from the
  child's log, with the text of the message that made it, which leads the report). A
  child's last allowed step, and the step after the repeated-call guard stops it, offer
  `finish` alone after a harness note. The tool result is `session: <id>`, the harness's
  status line (with the child turn's own end text when it did not complete), the child's
  report with every line behind `| ` (over 24,000 characters: the first three quarters
  and the last quarter, a note between), then the harness's merge note, made one line;
  without `finish`, the last text with `done` for a completed turn; a turn that did not
  complete is `failed`, `finish` or not (`harness_core::subagent`). A child's context
  frame says a task follows, not the person's message. The status line
  carries no `[harness]` lead because tool results escape that lead (R-2); the quote
  alone keeps the child's text from passing as the harness's. The tool card links
  to the child session as soon as it starts and shows the status when it ends. Built-in
  **Ultracode** delegated to Coder, Scout and Writer (since 2026-11 the **Senior Teamlead**
  and the **Software Developer** delegate). Sub-agent sessions are hidden from
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
* **MCP.** (Skills, text only, shipped with ADOPTION_PLAN X.2: T46.) Running third-party MCP servers on a node is running arbitrary
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

**Checkpoints for every folder, latest 100.** Folders inside a repository use its git
(refs under `refs/harness/`, index/HEAD/branches untouched; superseded 2026-11-10:
every folder now uses a shadow repository, see §5 and T19). Other folders get a
private shadow repository in the node's data dir (`checkpoints/<hash>.git`; nothing
written into the folder; dependency/build dirs excluded). `node.toml
checkpoints_keep` (default 100 since the trial of 2026-11-19, which could not undo a turn a dozen short turns later; 0 = off) keeps only the latest N checkpoints per folder;
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
Sub-agents inherit the parent turn's settings. The master's context (`MasterContext`: this message's line, and the standing parts,
memory and project) is a separate `context` field placed after the history, never in the
system prompt.

**Methods.** (1) Prompt caching: genai `CacheControl::Ephemeral` on the system prompt
(covers tools + system), on the end of the history before this turn's user message, on
the end of the history before the previous user message, and on the newest message (at
most 4, Anthropic's limit; `llm::to_genai`). Cache reads/writes are counted in
`Usage` and shown in the token meter's tooltip. This turn's user message carries the
harness's context block, which is recorded (`context_sent`, X.1, T52): the next turn
rebuilds it and every step after it byte for byte and reads the cache up to this turn's
last tool result (its newest marker, two blocks back); a standing section (instruction
files, skills, memory, project) that did not change is not sent again. The second and
third markers stay for a step with many parallel results (Anthropic looks back 20
blocks). Before X.1 the next turn sent the message without its context, so entries ending
at it or after it were read only within the turn (review C43-1, T43). One request writes the history again after a summary
(it changes the system prompt), when an image leaves the newest `IMAGE_TURNS` and is
named instead, and when masking hides a new batch. The masked set is the same at a
turn's last step and the next turn's first, unless the turn ended at the step cap with
tool outputs it never sent. Notes waiting for the person go into this turn's message,
after the history marker. (2) Observation masking (`mask.rs`):
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
  fragments (a fragment only between the same message's footnotes and their
  references, whose ids carry a per-message prefix; any other `#…` is text, L14),
  always showing the real host in an isolated LTR `<bdi>`, with a
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
* Chats stay private to their owner in every workspace, except to the static API token (accepted gap, T29). What a server admin sees of other people is numbers per person and model, never per chat: the usage totals (`GET /api/admin/usage`) hold an account's id and name, a model id and token sums, a turn count and the period asked, and no session id, title, folder, agent, node, time of a turn, count of chats or per-day series. A chat's workspace is a
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
  the first load keeps a hard ceiling of 110 KB gzip (104 until 2026-11-22, raised by the owner), because every first visit waits for it.

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

### 2026-11-10 — Provider retry with backoff

Before, one 429, 5xx or dropped connection moved a turn straight to the next fallback, or
ended it when there was none; every other harness studied retries first
(`research/harnesses-round-1.md` §8 item 2). Now each model call (a turn's steps, compare
variants, the summary call) is retried in `agent-native` (`retry.rs`) up to 3 times, waiting
`2 s · 2ⁿ` capped at 60 s with equal jitter, or the provider's `retry-after-ms` /
`retry-after` (seconds; an HTTP date is ignored). A `Retry-After` longer than the 60 s cap is
not waited for and not retried (clamping it sent up to three requests that were certain to
fail and count against the limit): the next fallback is tried at once, or the turn ends with
"The provider of M asked to wait 5 minutes before the next request (rate limit)…" (no
response body; `RetryPolicy::explain`). Retried: 429
(except `insufficient_quota` / billing codes), 500, 502, 503, 504, 529, connection and
stream-start failures. Never: 4xx refusals (401, 403, 400, 404, 413…), `x-should-retry:
false`, or an attempt whose text has reached the user. After the retries the fallback chain
applies as before, each fallback with its own retries, so a turn makes at most
`4 × models in the chain` calls per step. The wait selects on the turn's cancel token. Only
this layer retries: `genai` 0.7.2 has none and reqwest 0.13 only resends HTTP/2 requests
the server refused unprocessed. `ProviderError::Status` carries a `RetryHint` read from the
response headers. Log lines name the status or failure kind only (`ProviderError::log_line`),
never the response body or a URL; the fallback warning used to log the body.
**No new event**: the wait shows as the turn still running. A "retrying in N s" notice
would need an event type (owner's call).
**Evidence:** `agent-native/tests/retry.rs` (429 then success with no fallback, 400/401/403
not retried, 4 attempts then the fallback, no retry after streamed text, cancel ends a 30 s
wait, `Retry-After` honoured and capped through the real `genai` client), `retry.rs` unit
tests (status table, quota 429, backoff bounds, header parsing, no body in the log line).

### 2026-11-10 — Compaction keyed to the context window; overflow recovery

Before, summarising triggered at a fixed 24k-token character estimate whatever the model,
and a provider's context-length error ended the turn (`research/harnesses-round-1.md` §8
item 3). Now (`agent-native/src/compact.rs`):
* **Trigger.** `AgentSpec` gains `context_windows` (chain model → tokens), filled by the
  master from the saved model list (`ModelConfig.context`, from listings or the admin; no
  network call). The trigger is the window less a reserve (`max(window/5, 16k)`, at most
  half), or `[chat] summarize_after_tokens` when set (now optional; 0 = never), or 24k when
  the window is unknown. The history is measured as the previous turn's reported
  `context_tokens` plus an estimate of its reply, falling back to characters / 4.
* **Overflow recovery.** A 400/413 recognised as a context overflow (error code
  `context_length_exceeded` / `request_too_large` / `exceed_context_size_error`, else the
  known messages of Anthropic, OpenAI-compatible servers, Gemini, llama.cpp, Ollama:
  `ProviderError::is_context_overflow`) is no longer "model-level" (it used to trigger a
  fallback because the message names "model"). The step is compacted (older turns
  summarised, all but the newest large tool output masked for the rest of the turn) and sent
  once more; a second overflow, or nothing left to shrink, ends the turn with a plain
  message naming the model. Once per turn.
* **Summariser window.** The summariser (the answering model) reads the old part in pieces
  at turn boundaries that each fit its window less 2k; each piece reads the previous
  summary. A turn larger than the window alone is read with its tool results cut to 1,000,
  500 or 250 characters (X.15); only one that does not fit even so stops summarising there
  (logged), so nothing is dropped unread. With an unknown window it is one piece, as before.
* **Log.** Each piece is its own `history_summarized` event (schema unchanged; a turn may now
  append several, and one may come mid-turn after an overflow), each covering everything up
  to its `through_seq` on a turn boundary, appended through the session writer. A cancel or
  failure between pieces leaves the last one valid; the turn then ends `cancelled`.
**Evidence:** `agent-native/tests/compaction.rs` (200k window does not compact at 32k
history, unknown window still does; 16k window compacts at 12k; configured value wins both
ways; overflow → one summary, one retry, success; second overflow fails without a loop and
without the provider body; pieces fit a 5k summariser and carry the earlier summary; a turn
bigger than the summariser's window is not summarised; cancel mid-compaction leaves one
valid summary or none), `compact.rs` and `provider.rs` unit tests (thresholds, reported usage,
overflow shapes of seven providers and five look-alikes), `model_cache_http.rs`
(`context_windows` on the spec).

### 2026-11-11 — Tool calls in a message cut by the output limit

Before, a tool call the model was writing when its answer hit the output-token limit was
refused only when its arguments did not parse, with "tool arguments must be a JSON object,
got: <the fragment>": the model was not told why and tended to resend the same oversized
call. A cut call whose arguments still parsed (complete by chance, or closed by a server)
ran. On Anthropic the cut call also went back in the history as a string `input`, which the
Messages API refuses, so the next request failed instead. (ADOPTION_PLAN A.6; final review
A5; `research/round2/synth/01-loop-dispatch-durability.md` §6.4: Pi refuses every call of a
`length` stop, OpenCode ends the step after three.)
* **Stop reason.** `Completion::stop` (`StopReason`: `End`, `ToolUse`, `OutputLimit`,
  `Other`) is mapped from the provider's raw value in `llm::stop_reason`, per API kind
  (OpenAI-compatible `finish_reason`: `stop`, `tool_calls`/`function_call`,
  `length`/`model_length`/`max_tokens`; Anthropic `stop_reason`: `end_turn`/`stop_sequence`,
  `tool_use`, `max_tokens`/`model_context_window_exceeded`). Anything else, or no value, is
  `Other`, never `End`.
* **The rule.** When a message stopped on `OutputLimit`, **none of its tool calls runs**,
  whether or not its arguments parse, delegations included; none is offered for approval.
  Each is recorded like any refused call (`tool_call_started`, then `tool_call_finished`
  with `ok: false`) with the fixed result `CUT_BY_OUTPUT_LIMIT`, which names the limit and
  says to split the change, and never contains the cut arguments. The message's text is
  kept. **Three such messages in a row** (`MAX_CUT_STEPS`) end the turn with an error naming
  the cause; a step that completes normally resets the count. Every append is checked as
  elsewhere: a failed one ends the turn.
* **History.** A call whose arguments are not a JSON object is sent back to the provider as
  `{}` (`to_genai`); the event log keeps what the model wrote.
**Evidence:** `agent-native` `lib.rs` tests (`a_call_cut_by_the_output_limit_says_why`,
`a_complete_call_in_a_cut_message_does_not_run`, `three_cut_messages_in_a_row_end_the_turn`),
`llm.rs` `stop_reasons_map_per_provider`, `tests/providers_http.rs`
(`openai_length_stop_is_the_output_limit`, `anthropic_max_tokens_stop_is_the_output_limit`,
`a_cut_call_in_the_history_is_sent_as_an_object`).

### 2026-11-15 — What a chat costs: per-model usage and admin-set prices

Before, the chat header showed tokens only, and a turn's usage summed every call (main
calls, fallbacks, summaries, sub-agents, hand-off notes) under one `model`, the window's
label, so it could not be priced per model (ADOPTION_PLAN X.18).
* **Recording.** `ModelRouter::complete` stamps each call's usage with the prefixed model
  it was asked (`Usage::stamp`); `Usage::add` merges `by_model`, capped at 16 models.
  Every path adds through it (the parallel-answers path summed fields by hand and lost the
  cache counts: fixed). Event field: `turn_ended.usage.by_model` (section 2).
* **Prices are data.** No price table ships (OpenHands' router table is "stale the moment
  models change", `research/round2/openhands.agents-sessions.md`): `[[providers.models]]
  price = {input, cached_input, cache_write, output}` per million tokens, each optional,
  and `[pricing] currency` (ISO 4217, one per server). Checked in `Config::validate` and on
  save (finite, 0 to 10,000; a bad value refuses the save or the start, never dropped). Set
  by server admins (`PUT /api/admin/providers/{id}` with the model list, `PUT
  /api/admin/pricing`); read by every signed-in user on `ModelInfo.price` with the
  currency (prices are not secret; a member sees their own chats' estimates). No
  provider's listing returns prices in a documented field we use (the owner's router's
  `/v1/models` has none), so nothing fills them automatically.
* **The estimate is computed where it is shown** (`web/src/lib/cost.ts`): per model,
  uncached input, cache reads, cache writes and output at their prices; a model is priced
  only when every class it used has a price, else its tokens and unattributed tokens are
  "unpriced" and the figure says so (Oh My Pi counts unpriced requests apart,
  `research/round2/oh-my-pi.sessions.md`; OpenCodex labels an "estimated cost (API
  list-price equivalent, not billing)", `research/round1/synth/07-models-observability.md`).
  Always labelled an estimate at current prices.
* **Removed:** `AgentSpec.budget` (`lean`/`standard`), parsed and never read (research 07
  §6: "worse than absent because it promises something"). An agent file still carrying
  `budget:` fails to load with the unknown field named.
* **Not built:** spending limits (per user or workspace: the master would need per-user
  usage sums kept by the store and a check before a turn starts, as OpenCode Console and
  Claude Code's gateway do), admin totals per user (the store keeps no usage aggregate; a
  scan of every log would be needed).
**Evidence:** `core/tests/usage_by_model.rs`, `agent-native/tests/usage_by_model.rs` (main
call, retry, fallback, summary between and inside turns, sub-agent on the small model,
parallel answers, callback turn, hand-off note), `llm.rs`
`router_stamps_each_calls_usage_with_its_model`, `master/tests/it/prices_http.rs`,
`web/src/lib/cost.test.ts`, `providerform.test.tsx`, the shared fold fixture.
