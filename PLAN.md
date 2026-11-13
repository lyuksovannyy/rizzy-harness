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
  admin created with a one-time claim code. Only admins create accounts (a temporary password the
  server makes and shows the admin once, changed at first login). Everyone sees only their own conversations, in every workspace (the one exception is the static API token, T29).
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
  - Sub-agents (`delegate`, today the Teamlead's and Software Developer's) report back as synthetic tool results and each
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
| `turn_started` | `turn_id, agent?, cause?` — `cause: {callback: [bg_id]}` on a callback turn (T.8d step 3): started by the harness because those background commands ended while the chat was idle; it has no `user_message` after it, and the model reads one harness-written opening rebuilt from the log alone (`docs/background-commands.md` §13) |
| `user_message` | `turn_id, text` |
| `assistant_delta` | `turn_id, message_id, text` — coalesced, see below |
| `assistant_message` | `turn_id, message_id, text, tool_calls[{call_id, tool, input}]` — authoritative full text |
| `tool_call_started` | `turn_id, call_id, tool, input` |
| `tool_output_delta` | `call_id, stream (stdout/stderr), text` |
| `tool_call_finished` | `call_id, ok, output, exit_code?, truncated, note?` — `output` is what the model sees; `note` (the repeated-call guard's line, T40) is written only by the node's loop, sent to the model after the escaped output, and not shown in the UI |
| `approval_requested` | `approval_id, call_id, tool, input, reason, turn_allow` (`turn_allow` only when true: "for this turn" may be offered) |
| `approval_resolved` | `approval_id, decision (allow/deny/allow_session), by (user/policy/cancelled/interrupted)` |
| `checkpoint` | `turn_id, phase (before/after/restored/before_restore), git_ref, files?, excluded?, excluded_more?` — `before_restore`: the folder just before a restore, written before it changes a file; its `turn_id` (`restore-<id>`) names the restore, whose `after` follows, so restoring that id undoes the restore (T33); if the restore failed before its `after`, restoring that id puts its `files` back to their saved content (A.16). `excluded`: nested git repositories the snapshot left out (on `restored`: the ones the restore left alone), relative to the checkpoint's folder; at most 50 names, control characters replaced, `excluded_more` counts the rest; the fold keeps their union (and the largest count) per checkpoint; the web UI ignores both for now |
| `turn_ended` | `turn_id, reason (completed/cancelled/error/interrupted/stopped), usage?` — `stopped`: cancelled by a stop of the node (A.19, below) |
| `error` | `turn_id?, message` |
| `background_started` | `bg_id, call_id, turn_id, timeout_s, ends_by_ms` — a `bash` call with `background: true` still running after its 3 s first wait; appended in the turn, before that call's `tool_call_finished` (T.8d, `docs/background-commands.md`) |
| `background_ended` | `bg_id, reason (exited/timed_out/stopped_by_agent/stopped_by_user/access_revoked/node_stopping/node_restarted), exit_code?, signal?, confirmed (ended/still_alive/unconfirmed/unknown), ran_ms, output_bytes, tail, tail_cut, delivered` — once per command, by the session actor: at a step boundary of the running turn (`delivered`, the model saw its note there), between turns, or at the node's start (`node_restarted`); `tail` is at most 4 KiB of program output, scrubbed where the output entered the node; the model reads it quoted line by line (`| `) under one harness line, inside the step's last tool result or in front of the person's next message, never as a user-role message of its own. `session_deleted` exists as a reason and is never recorded (the log goes) |
| `callback_refused` | `bg_ids, reason` — the node did not start a callback turn for these ends on its own rules (its `callback_turns` budget, `callback_turns = 0`, a routine's chat, a revoked chat); recorded once per refusal, never shown to the model; the ends wait for the person's next message (T.8d step 3) |

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
and a failed snapshot or append restores nothing (T33). **A restore is all or
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
| T1 | Network attacker reaches the master UI | Bind loopback by default (`127.0.0.1` and `[::1]` of the port, T37). Non-loopback bind requires TLS (Phase 2) or the explicit `--insecure-bind` flag (tailnet/WireGuard mode), which logs a warning on every start. |
| T2 | Malicious website in the user's browser (CSRF, DNS rebinding, cross-site WS) | Accounts with Argon2id passwords; the first (admin) account needs a one-time claim code from a 0600 file. A login is two values (Gate 3 review P-1). The `HttpOnly; SameSite=Strict` cookie holds a random 256-bit session id. Browsers send a cookie to **every port** of its host (RFC 6265 §8.5; `SameSite` ignores the port), so another local server, such as one the agent started on `127.0.0.1:8000`, gets the cookie when the person opens it, and a non-browser client writes any `Origin` it likes. The login therefore also has a random 256-bit **login key**, returned once in the body of the answer that made it (login, the claim, a password change). The page keeps the key in `localStorage`, which is per origin with the port, and sends it as `x-harness-login` on every request. A custom header also forces a CORS preflight on any cross-origin script, and the server answers none. A WebSocket sends the key as its first message (`{"type":"login",…}`) within 5 s: nothing is subscribed or sent before it, and without it the socket is closed with 4401. Nothing about the cookie is looked up before the key arrives (R.4, fix review 2 R2-1): the upgrade is answered 101 for any cookie, live, dead or made up, and each ends the same way without the right key, so the cookie's holder learns nothing; a socket waiting for its key takes none of the user's 8 places (those are taken once the key is accepted; a ninth keyed socket is closed with 4429, one whose account must change its password with 4403), and the waiting ones are bounded apart, 64 in all, the oldest pushed out (1013, try again) rather than a new one refused, since a page sends its key at once. The key is never put in the URL or a subprotocol. Only the SHA-256 of each value is stored (`ui_sessions.token_hash`, `bind_hash`), so a leaked database does not leak logins. `authenticate` is one indexed lookup joined to `users`. The key's hash is compared in constant time before the session is used or slid. A missing, expired or disabled login, or a wrong or missing key, gets the same 401. Logout ends that login's row with the cookie alone (R.4, fix review 2 R2-3; before, only with the key, so a page that lost its key, storage cleared, could not end its login for 30 days): ending a login never harms its owner and the answer is the same whether the cookie was live, so the worst a holder of the cookie can do is sign the person out of that one login, which it can repeat only after they visit its page again. What the browser fetches by itself (`<img>`, `<video>`, `<audio>`, `<a download>`, the thumbnail maker's elements) carries a **link** from `POST /api/links`. A link is a MAC, under a per-process key, over the login's cookie hash, an expiry 30 min out, and the exact path and query. It works only with that login's cookie and only on the byte routes (media content and thumbnails, artifact downloads, previews and thumbnails; `auth/links.rs`). It holds no secret of its own, and only paths are ever logged. The engine favicon (`/api/favicon`, an `<img>`) is one of those byte routes since R.4 (R2-1): it used to take the cookie alone, which made the server fetch from the network for whoever held the cookie and told them whether the login was live. Tests: `master/tests/login_binding_http.rs` (`a_cookie_alone_takes_no_socket_place_and_learns_nothing`, `a_logout_with_the_cookie_alone_ends_that_login`), including the abuse case end to end (a second client that holds the cookie and forges `Origin` reads no events and answers no approval), and `media_http.rs` `an_element_fetches_by_link_and_the_cookie_alone_gets_nothing`; the web side in `web/src/api.test.ts`, `live.test.ts` and `lib/links.test.tsx`. **Residuals:** (1) a script running in the harness's **own origin** would hold both values; this is why no model HTML is ever rendered (T3) and why the key is no worse kept in `localStorage` than in `sessionStorage`. (2) A local process can still hold many waiting sockets (each for at most 5 s, 64 in all) and so delay, not block, a new socket of the person's page; it needs no cookie for that, as with any connection flood (T21). (3) A playing `<video>` keeps the link it loaded with, so seeking more than 30 min later fails until the preview is reopened. (4) A holder of the cookie can sign the person out of that login (R2-3, above). A session ends 30 days after its last use (the expiry is moved at most once a minute); logout deletes its row, and a password change or reset, disabling the account and deleting it delete all of the user's rows, so a copied cookie stops working at the next request; the same paths close every UI WebSocket opened with those rows at once (close code 4401; logout closes only that login's sockets). Each socket is registered under its user with its session's SHA-256 (never the cookie) before its first login check, and revocation flips that socket's own flag after the rows are deleted, so a socket still upgrading during a logout is caught by one or the other; it is not a broadcast, so a lagging socket cannot skip it. A socket also re-checks its login every 60 s and before every subscribe, read only (an open tab does not slide the expiry; only HTTP requests count as use), which covers expiry and disabled accounts; a store error closes it. Client WS messages and frames are refused above 64 KiB by the WebSocket layer before buffering; a user (the API token counts as one) has at most 8 sockets, the 9th upgrade gets 429 (refused rather than closing the oldest, which would make two tabs evict each other forever). API-token sockets have no login to end and live until they close; unknown usernames are verified against a dummy hash, under the same hash permit and throttle slot, so timing and status do not reveal them (login limits: T12). A separate 256-bit API token (0600 file) serves scripts as `Bearer`. Every request checks `Host` against an allowlist (anti-rebinding). Any `Origin` sent must be exactly `scheme://host[:port]` of an allowed entry with that entry's scheme: the server's own (`https` with `--tls`), or the one an entry names (`--allowed-host https://chat.example` for a TLS proxy in front; `X-Forwarded-Proto` is never read), so `null`, another port or the other scheme are refused. A cookie-authenticated (no `Bearer`) state-changing request (not GET/HEAD/OPTIONS) or WebSocket upgrade without `Origin` is refused (403 `origin required`); `Bearer` clients, cookie-less logins and webhooks need none (audit L6; `master/tests/hardening_http.rs`). Token compared in constant time. **Accepted (R.4)** from the Gate 3 reads: the page's `seen()` (`web/src/api.ts`) keeps a `login_key` from any JSON answer; only login, the claim and a password change return one today, and a route that echoed a node's JSON at top level would let that node sign the person out (fix review 2 N-1; it would change if such a route is added: then take the key only from those three calls). The key's safety in `localStorage` rests on no model or node script ever running in the origin, residual (1) above, which the CSP backs (fix review 2 N-2). Not observed in a real browser (no Playwright where it was built, R.2): that a browser sends the harness cookie to another local port, and that element links load and download as intended; the e2e scripts that log in or download were updated without being run. A Playwright run with two listeners would settle the first. **Accepted (R.4, fix review 6):** R4-2: a local process that opens 64 socket upgrades within the one round trip the page needs to send its key pushes the page's waiting socket out (1013) and the page reconnects; it needs no login, only a cookie-shaped header and a forged `Origin`, and keeping the page out means tens of thousands of upgrades a second on loopback, where no per-source limit can tell it from the page. N-1: the socket's key check uses the login like an HTTP request, so it slides the login's expiry, once per connect (never per message or per re-check); an open tab therefore keeps its login alive only by reconnecting. Either would change with a socket that carries the key in its upgrade request, which a browser cannot send. |
| T3 | Secrets leak via logs, URLs, event log | Provider keys are referenced by env-var name in config, held in `Secret<String>` (redacted `Debug`), never put in events. Tool output is scrubbed of known secret values before it is stored. No token in any URL (no `?token=`). `tracing` never logs headers. Tool child processes (bash, graphify) get an allow-listed environment: `env_clear()`, then `PATH HOME USER LOGNAME SHELL LANG LANGUAGE LC_* TERM COLORTERM TZ TMPDIR` and the names node.toml's `env_pass` matches; provider key variables, `.env` names and `HARNESS_*` are removed even when `env_pass` matches them. The embedded node and `node run` build it through the same code. Open: `git` children (`node/src/git.rs`) still inherit the node's environment. Redaction guards against accidents only: it replaces exact known values, so an agent (or a hostile prompt) that encodes, reverses, splits or otherwise transforms a secret gets past it; the control is keeping secrets out of the shell's reach. Within that limit it is exact on streams: streamed output (tool output and model text) goes through a `StreamScrubber` that holds back the last (longest secret − 1, at most 4096) bytes until the next chunk or the end of the stream, so a secret that straddles a flush is still replaced; matching is leftmost-longest, and event payloads are scrubbed field by field (never as serialized JSON, where a secret containing a quote or backslash is escaped and would be missed). Residual: a secret cut off by the output cap leaves its prefix before the cap marker, and a secret longer than 4096 bytes may leak if split. |
| T4 | Prompt injection from repo content makes the model run harmful commands | Default approval mode `ask`: every mutating tool (bash, edit, write) needs a human click; read-only tools are confined to the session workspace. **bash and graphify run in a Landlock sandbox** (`rizzy-harness sandbox-exec`, `[sandbox]` in node.toml, FIX_PLAN 2.7): read-write only the session folder and a per-session `TMPDIR` (a sub-agent works in its own node-owned folder instead, with read-only access to the repository's object store only, T32), read-only `/usr /bin /sbin /lib* /etc /opt /proc` and node.toml's `[sandbox] read`, read-write `[sandbox] write`, five device files (not `/dev/tty`: TIOCSTI); `no_new_privs` (setuid programs such as `sudo` fail); TCP connect/bind denied with `network = "off"` (default on, Q6); with the network off the contained shell (T9) also runs in a network namespace of its own (`bwrap --unshare-net`, ADOPTION_PLAN T.8b): only its own loopback, so no UDP or DNS leaves and the host's abstract Unix sockets are absent; the containment probe checks the namespace by identity (the command's `/proc/self/ns/net` is not the node's, in both forms of `/proc`; not by the interface list, which a tunnel module fills: review T8B-2). Where the probe with `--unshare-net` fails and the one without passes, the start tries once more; if that fails too, `[execution] sandbox = "strict"` does not offer the sandbox mode (N.10a), naming what failed (bubblewrap's message, or the probe's own identity check), and `"auto"` still contains (PID namespace, guard, Landlock inside) without it, its containment line a warning "network off WITH A GAP" (TCP stopped by Landlock, UDP and DNS out, abstract sockets as Landlock's ABI allows). An `"auto"` node in that gap keeps its bubblewrap and, when a shell command is started, probes the namespace again at most every 60 s (`STEP_UP_INTERVAL`), on a thread of its own, one at a time, each bounded by 2 s (`LATER_PROBE_BOUND`: then killed through the guard's channel and counted as failed); the command that noticed does not wait (re-check R-2). From the first that passes, commands started from then on are in a namespace of their own, logged; the node only steps up, never down (`StepUp` in its `Sandbox`). What remains of T8B-1: a node is below where it could be until its next shell command after the interval, and the command that triggers a probe, and every command already running, keep the mode they started in; a node whose probes all failed at start, or with no bubblewrap found, does not offer the sandbox mode and never probes again (N.10a removed re-check R-1's step up from Landlock alone: nothing would use it); `strict` does not offer the mode until restarted. Only the node's own probes decide, held in its own `Sandbox`; nothing per call or from the protocol. Below Landlock ABI 9 a resolver that answers over a named Unix socket is still reachable and can look names up for a command (review N-1, not tested; T.8c's file view closes it). Tests: `crates/cli/tests/network_off.rs`. With TCP on, the agent can start a server on another port of the person's machine. When they open it in the browser that is logged in to the harness, the browser sends the harness cookie along (cookies are not separated by port). Since Gate 3 P-1 the cookie alone authenticates nothing: approvals, events and admin actions need the login key that the page keeps in its own origin (T2). `network = "off"` would close the agent's side too, but it breaks dev servers and package installs, so it stays a per-node choice; abstract Unix socket and signal scoping (ABI 6) and pathname Unix socket connects (ABI 9) best-effort. `mode = "required"` (default) refuses agentic sessions (and `node run` refuses to start) unless a start-up self-test through the wrapper is denied a canary in the data directory and the kernel enforces Landlock ABI 3 in full (plus ABI 4 TCP rights with `network = "off"`); a `[sandbox]` entry or system folder that equals, contains or is inside the data directory stops the node. Tests: `crates/cli/tests/sandbox.rs`, `tests/data_dir.rs`. File tools walk every path with `lstat`: a symlink that dangles or leaves the workspace is refused, `write`/`edit` do not follow a link at the last component, and nothing is written under `.git` (its config and hooks run commands). Stored shell output (`tool_output`) has random ids and belongs to the session that produced it. **A session's folder is pinned by path, device and inode** (R.1, review S-2; `harness_core::folder::FolderPin`): taken when the session is made (and, after a restart, when it is loaded, from the canonical path the log recorded); `FolderPin::check` (the path still canonicalises to itself and is the same directory) runs before every use: `ToolCtx::resolve`/`resolve_for_write`/`workspace_dir` (every file tool, `grep` and `glob` without a path), `Confinement::command` before any grant is built (bash, graphify; the command then starts in the checked path), `Session::checked_folder` before every turn, restore, delegation, checkpoint and the write of an undo, and every publish. A replaced folder is refused with "this session's folder was replaced", and nothing is read, written or granted there. The per-call part of this held through the real agent loop only from 2026-11-13 (fix/turn-host-folder-pin): from 1220e1a (pin) and f83a87a (owned folder) until then the loop's `Logged` wrapper did not forward `TurnHost::folder_pin`/`owned_folder`, so every tool call in production ran with neither (no per-call pin check; a writing sub-agent's `write` and shell refused in its own folder); `agent-native/tests/host_forwarding.rs` now fails if a wrapper drops them. The same per-turn check also tests `workspace_roots` and the protected folders again (before, only the data directory). The review found that a sub-agent could rename its own worktree away and put a link to `~/.ssh` in its place, after which `read` (no approval), `write` and the next shell followed it; a sub-agent's folder is now node-owned and out of every other session's reach (T32). Tests: `node/tests/runtime.rs` `a_session_whose_folder_was_replaced_refuses_to_work`, `cli/tests/sub_agent_sandbox.rs` (`a_sandboxed_sub_agent_cannot_point_its_folder_at_another_one`, `the_file_tools_and_the_shell_refuse_a_replaced_folder`), `core/src/folder.rs`. Residual: the path is resolved again by whatever uses it after the check (an `open`, a `chdir`), so a swap inside that window (microseconds to a millisecond) is followed once; only a process that may write the folder's parent can swap it, and no sandboxed shell may (a session's folder is the top of its own grant; a sub-agent's folder lies in the node's protected sessions folder, `cli/tests/sub_agent_sandbox.rs`). **The node's own git reads a session's folder in the sandbox** (R.1, fix review 3 R1b-2): every git process of a checkpoint or sub-agent shadow (snapshots, the sub-agent's base and result, the fill of its folder, restore's reads, copies, gc, a published file's state) is started through `sandbox-exec` with read-only on the folder and on the borrowed object store, read-write on its own shadow (where every private index lives) and, while it fills a new sub-agent folder, on that folder; nothing else of the data directory or the home folder (`git::command_std`, `Sandbox::helper`; node.toml's `[sandbox] read`/`write` are not added). Before, a process the session left running could swap a folder in it for a link while a snapshot walked it, and the node's unsandboxed git read the outside file into the checkpoint, and into a sub-agent's folder with no click: built as a test, 4 of 300 snapshots and 2 of 100 sub-agent folders held a canary from outside the folder or from a fake data directory; confined, 0 and 0 (`cli/tests/snapshot_sandbox.rs`). A second line: a new file whose folder really lies outside the folder or in a protected one is left out of the snapshot (`git::inside_only`). A link inside the folder is stored as a link, never followed, confined or not. The cost: one more process per git call; a checkpoint of a 100,000-file folder took 1.66 s confined against 1.57 s (the next one 0.36 s against 0.34 s). The start-up log and `doctor` ("checkpoint git") say whether it is confined; with `mode = "off"` it is not, and the read window is open there (residual). Where the node itself writes into a folder a session can still change (an undo, a sub-agent's result written into the parent's folder), it writes relative to directories it opened without following links (T33), so there is no window there; the review's proof that a left-running process won the old window on 7 to 17 of 120 delegations (R1-2) now lands 0 of 120 (`git::child_tests::no_swap_during_the_child_folder_fill_writes_into_another_folder`). After a restart a folder replaced while the node was down by another real directory at the same path is not noticed (the inode is taken at load; storing it in the log would refuse a folder restored from a backup forever). An intermediate directory swapped for a link between check and open is followed (by the file tools; the sandbox still bounds bash); sandbox residuals in R2. "Allow for this turn" (`allow_session`) allows **this exact call** again in the turn, never the tool as a whole (final review A1, ADOPTION_PLAN A.2): the key is the tool and its canonical argument (`harness_tools::call_key`, a SHA-256; the argument is not kept): `bash`'s command string byte for byte with its other arguments, `write`/`edit`'s real target path and the rest of their input, any other tool's input as sorted-key JSON. Approving `ls` never lets `rm -rf x` run unasked. It is offered and honoured from the first request for a call, because it covers only the call shown on that card. The answer is bound by its approval id to that one request, and the loop records that call's own key. It lasts until the turn ends; a sub-agent starts without it; a single allow covers one call; a `node.toml` denial is resolved by the node and can never be covered. Tests: `tools/src/approval.rs`, `agent-native` (`turn_allowance_*`), `node/tests/runtime.rs` (`allow_for_this_turn_*`, `a_single_allow_does_not_cover_the_next_identical_call`, `a_sub_agent_does_not_inherit_allow_for_this_turn`, `a_policy_denied_call_cannot_be_allowed_for_the_turn`). **Residual:** a symlink on a `write`/`edit` path swapped between the key check and the tool's own resolve sends an allowed write elsewhere inside the workspace (the same window as the file tools' own residual above). **The approval card** (Gate 3 fix review 2, W-1; `web/src/components/CallInput.tsx`, `web/src/lib/callText.ts`) shows the call as it will run, for every tool: every key and value of the input (named fields for `bash`, `write`, `edit`, `delegate`, `publish`; any other tool, or a key the layout does not name, as a complete key-by-key list), rendered from the input object the event carried, strings as themselves (never escaped JSON, never markup). Nothing is cut silently: a text over 1,500 characters or 40 lines states its size, shows its first and its last part (an attacker picks the middle), says how many characters of the middle are not shown, and opens in place; while any part is collapsed, Allow stays usable and a line beside the buttons (their `aria-describedby`) says "You are approving N characters you have not expanded". Text wraps and no box has a fixed height. Runs of blanks are counted markers (`⟨500 spaces⟩`, `⟨60 line breaks⟩`; 8 spaces, 4 tabs or 3 breaks in a command or short value, 40/8/6 in file content and prose). Hidden, control and unusual blank characters are `<U+…>` codes (`showHidden`, W-5: a rule by Unicode category, not a list, in `web/src/md/hiddenChars.ts`: every control (Cc) except tab and line feed, format (Cf), private-use (Co), unassigned (Cn), lone-surrogate (Cs), space (Zs) except the plain one, line and paragraph separator (Zl, Zp) and default-ignorable character, plus a short named list of blanks that are letters or symbols by category (braille blank, Hangul and Khmer fillers, grapheme joiner, object replacement); a variation selector right after a pictograph is the one exception, and chat titles and names also keep the zero-width joiner of emoji sequences. Each marker is its own element, so text that merely looks like one is not one; a test checks that the strip's set is inside the marked set). In a command, a path or a short value, a word mixing scripts is named with its odd letters (`сurl`: Cyrillic с U+0441); a whole word in another script, and Japanese/Korean/Latin-with-CJK mixes, are not flagged, and there is no confusables table. `write`/`edit`/`publish` paths have their own wrapping line, with "absolute path", "outside this chat's folder" (lexical, against the chat's `cwd`) or "leaves this chat's folder through .." when the input shows it; links and the node's policy are not visible to the card. The buttons read "Allow once", "Allow this exact call for this turn" (only when the request carries `turn_allow`) and "Deny". **What the card cannot do:** make anyone read it (a person can approve without expanding, and is told so, not stopped), judge intent (a fully shown `curl … | sh` is still approvable), or show what the node resolves (a symlink's target, a path the policy will refuse). Tests: `web/src/components/approvalcard.test.tsx`, `review-gate3.test.tsx`. **A sandbox grant is checked by identity when it is opened** (R.1, fix review 4 R1c-1): every grant passed to `sandbox-exec` names what it expects there (`Grant`, `Expect`): the device and inode the node took when it pinned or made the folder (the session's folder from its pin, a sub-agent's folder and its `.git`, the borrowed object store when `user_repo` accepted it, a shadow, the temp and output folders), or the explicit marker that no session can write the path (the system folders, the device files, node.toml's `[sandbox] read`/`write`); the command line forces one of the two for every grant. The wrapper opens each path once (`O_PATH`, `O_NOFOLLOW` for an identity grant), compares the opened descriptor's device and inode, and builds the Landlock rule from that same descriptor; a mismatch refuses the whole command before anything runs, naming the grant ("the folder granted as the session's folder was replaced"). So no grant follows a link swapped in between a check and the sandbox's open, and the sandbox side of S-2's check-then-use window is gone. The fix review proved that a process the parent left running could replace `<repo>/.git/objects` by a link to `~/.ssh` and the child's shell then read the key (R1c-1); now the node leaves out a store that is no longer the one it accepted (the child loses history, gains nothing) and the wrapper refuses it if it changes after that check. Tests: `cli/tests/sub_agent_sandbox.rs` `a_replaced_object_store_is_never_granted_to_a_child` (`objects` and `.git` replaced, links to `~/.ssh` and to the data directory), `the_wrapper_refuses_a_grant_that_was_replaced_and_names_it` (the session's folder swapped after its pin check), `cli/tests/snapshot_sandbox.rs` `the_nodes_git_never_follows_a_replaced_object_store`. Residual: a path marked not session-writable is opened following links, so a future grant marked so wrongly would reopen the hole; `exposes` still judges the agent's grants by real path first, a cheap first line behind which the identity check stands. **node.toml's `[sandbox] read`/`write` entries are no longer marked so** (R.4, fix review 5 R1d-1): an entry may lie where a session can write (inside a workspace root, which by default is the whole home folder, or inside another `write` entry), so a process the session left running could replace it by a link and the next command was granted the link's target. At start the node records where each entry's path really leads (canonicalized), and nothing more; at each command it canonicalizes the entry again, grants it only if it still leads to that same place, and passes the identity it finds there now, which the wrapper checks on the descriptor it opens (`Extra::grant`). The identity is therefore taken per command, not at start. A link put in its place, or in place of a folder above it, gets nothing; a file rewritten by renaming a new one over it (as git does with `~/.gitconfig`) keeps its grant. Refused at start, naming the key and the path (`check_extras_against_roots`, `exposes`): an entry that equals or contains a workspace root; one that equals, contains or is inside a protected folder; one inside another `write` entry (whose folder every command may change; the outer entry already grants it), naming both (fix review 6 R4-1); and one below a workspace root whose path holds a symbolic link below the root, each component checked with `lstat` and the link named, with the advice to list the real path (R4-1: a chat may have put that link in place of a real folder while the node was down, and the next start would otherwise grant its target; a real toolchain folder such as `~/.cargo` passes). Not refused: an entry inside a workspace root. Refusing it, as the review proposed, would refuse every toolchain entry the README lists under the default root (`$HOME`), and the identity binding closes the swap without that. Tests: `tools/src/sandbox.rs` `an_extra_replaced_by_a_link_is_not_granted`, `an_extra_that_holds_a_workspace_root_is_refused`; `tools/tests/review_gate3_extras.rs` (the reviewer's R4-1 proof, inverted: `review_r4_1_an_extra_swapped_before_a_restart_is_refused_at_the_restart`, and `an_extra_inside_a_write_extra_is_refused_at_start`). Residual: an entry listed through a link that lies outside every workspace root and leads into one is checked only where it leads, so a swap there before a restart is not caught; listing the real path avoids it. Residual: graphify's install folders (its venv and Python) are still granted following links; they are found from the binary's path at each call, never taken from a session. Fix review 5's notes, accepted: the identity is device and inode, and an inode number can be reused after a delete (ext4); a session cannot delete the node-owned grants, and for its own folder a reused inode would hold its own content, so nothing is gained. `exposes` canonicalizes an extra at start, following a link present then: that is the operator's choice, judged once. **Accepted (R.4)** from the Gate 3 reads. *Writes outside the folder* (review N-2): with `allow_outside_workspace = true` the file tools may write anywhere the OS user may, `~/.bashrc` and `~/.gitconfig` included, which is code execution outside the sandbox at the user's next shell; the sandbox bounds only bash and graphify, and the protected folders are only the data directory and `protected_dirs`. Accepted because the key exists for exactly that, is off by default and is a node.toml choice; the README says so where the key is documented. It would change with a refused list of start-up files that holds even with the key on. *The card* (fix review 3): a person can approve a long input without expanding its middle; the card says how much is unread and does not stop them, a deliberate choice, since a forced expand is one more reflex click and pushes people towards `auto` (W-4). A word wholly in another script (`сору`) is not named, only mixed ones (N-1); text shaped like the card's own markers (`⟨500 spaces⟩`), which fix review 3 found told apart only by colour (N-2), is no longer mistaken for one since W-5 made each marker its own element; the card renders the tool call's input, not the approval event's copy, which the node sends from the same value (N-3; a fold-time comparison would guard the day that stops holding). Not checked without a real browser (R.6): that a wrapped command lays out with nothing off-screen, screen-reader output, the warning colour in the light theme. Any of these would be reopened by a card change or a browser run that shows otherwise. |
| T5 | Compromised master widens node permissions | **Node-local policy (`node.toml`) is authoritative** and is not settable over the protocol or over HTTP (no endpoint writes it or installs software; graphify is allowed only by `graphify = true` in it, read at start): allowed workspace roots, max approval mode (`ask` default; master can request stricter, never looser), tool denylist. The node logs every command it received in its own store. **Delegation too** (Gate 3 review P-2): a `delegate` call goes through the node's approval path like any tool, before any child session exists. `denied_tools = ["delegate"]` refuses it with the policy's reason. `ask_all` asks for it. Under `ask` it is not asked about: it changes nothing by itself, and the child's own calls are gated. `[limits] max_subagents` (default 4) and `max_delegation_depth` (default 1) bound the master's agent spec at every level: the spec may ask for fewer, never more (`LimitsPolicy::bound_delegation`, applied to every prompt). Tests: `node/tests/runtime.rs` `a_node_that_denies_delegate_refuses_it_before_any_child_exists` (the review's proof inverted), `ask_all_asks_before_a_delegation_and_ask_does_not`, `node_limits_clamp_the_masters_delegation_caps_and_depth`. The per-turn token cap of sub-agents still comes from the master alone. What the master forwards at create is checked there first (R.4, review N-5): a `model` the chat's workspace does not list is refused before anything reaches a node ("unknown model"), and `approval_mode` is a closed enum that the node clamps again; a command whose caller goes away mid-wait no longer leaves its pending-ack entry behind (a guard removes it however `send_command` ends; `master/tests/link_hardening.rs` `an_abandoned_command_leaves_no_ack_and_an_unknown_model_is_refused`). |
| T6 | Compromised master approves things on nodes | Accepted in v1: the master is the approval channel, so a compromised master can approve anything inside node policy. Bounded by T5. Future: approvals signed by a user device key verified on the node (R1). |
| T7 | Rogue node / stolen node key; two processes with one key (a copied data directory, a node restored from an old disk) | Node keys pinned after operator approval; revocation marks the key revoked and drops the link, and frames from a revoked node are refused in the append transaction, those already queued included. A node only writes its own sessions; the master rejects events for sessions owned by another node. A second writer under the same key is caught when its `seq` repeats one already stored with different content: the session is marked diverged and refused from then on, logged once at `error` without content, the node is sent `session.cancel`, and commands that would write to it are refused (§3). What a registered node can do to the master and to other users through what it sends (Gate 3 review, R.3): it cannot rewrite a chat's row after creation or change its owner (a later `session_created` is refused, L-2), cannot name another node's or another owner's chat as its session's parent to receive that chat's project context (L-1), and its strings and events are bounded (L-3, below). A node revoked while its handshake runs no longer stays linked (L-4): `node_connection` reads `revoked_ms` again once the link's stop token is registered, `add_link` refuses a revoked node (the store's record, and the in-memory set `revoke_node` writes under the same lock as the link table, so a link is either registered before a revoke, which takes it down, or refused after it), and `send_command` sends nothing to a revoked node (`a_node_revoked_while_it_reconnects_never_stays_linked`). **Failed handshakes** (R.4, review L-7): node ids are the lowercased hostname, so a stranger can guess one. A bad signature for a registered id writes an audit row only while that id and that peer address are each under 5 rows in 15 minutes (`nodes::AUTH_FAILURE_AUDITS`, the login path's `FailureMap`), so a flood writes a bounded number; an unknown id writes none. Both are refused in the same words ("unknown or revoked node"). Minting a join token is audited (`node.join_token`, who and when; never the token) (review N-5). **A node of another protocol version** is refused before `accepted` (A.19): the `auth` and `join` hellos carry the node's version (a required field: a hello without it is malformed and closed), compared only after its key is proved (and, for a join, its token found valid, before the token is spent), and answered `rejected` naming both versions and which side to upgrade; the node exits with it, status 77, instead of reconnecting (every `rejected` ends its loop: each needs a person; a master that is down is retried). The node prints a refusal as printable ASCII on one line, spaces folded, at most 300 characters (`wire::printable`), and accepts only an id in the master's own shape (`wire::is_node_id`, at `node join` and at every `accepted`; otherwise it refuses that master, also 77): a master cannot write escape sequences, Unicode separators or overrides, or forged log lines on the node's terminal. A peer without a registered key gets the same "unknown or revoked node" whatever it claims, so it learns nothing of the master's version; a registered node learns the master's protocol and crate version, which it runs against anyway (`master/tests/version_refusal.rs`, `cli/tests/stop_signal.rs`). Test: `master/tests/remote_nodes.rs` `a_flood_of_bad_signatures_writes_a_bounded_audit`. Residual: the time to refuse differs by one signature check (tens of microseconds) between an unknown id and a registered one, which tells a patient prober which hostnames are registered; the id is not secret (it is the hostname) and the key is. Tests: `store/tests/event_log.rs`, `master/tests/link_hardening.rs`, `master/tests/hostile_node.rs`. **Residual:** a copied key can freeze any session of that node's mirror by sending one differing frame (it could already do worse: write that node's new sessions); a second writer whose frames only extend the log (it starts at `head + 1` before the first writer does) is not detected until the first writer's frame collides, so up to that point the mirror shows the second writer's events; the node is not told why (it sees an ordinary cancel) and its own log keeps the session; the UI shows a chat that stops updating and refuses new messages with "diverged", with no banner yet (web follow-up). The full fence (a generation per connection, an epoch per session) comes with workspace nodes (ADOPTION_PLAN C.1). **Accepted (R.4)** from the R.3 residuals: a remote node may still declare the `chat` profile for a session it creates (the profile is checked against the known values, not against the link being remote); the row is that node's own chat on that node, so it gains nothing beyond a chat it could make anyway, and it would matter once the chat profile means a master-side runtime with wider rights. The revocation race test runs 150 barrier-started add-and-revoke pairs and cannot force the narrowest interleaving; the second read in `node_connection` needs TLS and is covered by reading only. |
| T8 | MITM on the node link | WSS with the master's certificate pinned by fingerprint on the node (no CA, so a mis-issued certificate is not enough), and the node's Ed25519 key pinned on the master: the node signs a fresh master nonce bound to that fingerprint, so its signature cannot be replayed to or relayed through another endpoint. The master authenticates only through TLS (it signs nothing itself). Both directions are bounded queues with back-pressure, and the node times out connect and handshake (§3), so neither end can make the other buffer without limit or hang it. A registered node cannot make the master store or list without limit either (Gate 3 review L-3): an event over `MAX_EVENT_BYTES` (4 MiB) is refused and freezes its session, the strings it supplies for the row are capped (title cut to 200 characters; `cwd`, model and agent id refused past 4096, 256 and 64 bytes), and session listings filter by owner in SQL instead of loading every account's rows (§3; `an_event_over_the_size_cap_is_refused`, `an_oversized_event_from_a_node_is_refused_and_freezes_the_chat`, `a_listing_of_one_owner_reads_only_their_rows`). The master's certificate is made only when neither `keys/master-cert.der` nor `keys/master-key.der` exists; any other read error, or half a pair, stops the server with the path and the reason, because a new pair would silently break every node's pin (R.4, review N-4; `link/src/tls.rs` `an_unreadable_master_key_stops_instead_of_making_a_new_one`). Residual: the board and project-memory pages still read every row to resolve card and proposal owners (bounded now by the caps); the event log itself grows as fast as a node writes in-order events (a quota per node is not built). |
| T9 | Runaway processes | **A command's processes outlive its `bash` call only as a background command the node holds (T.8d, T36), and never outlive its timeout, its session, its owner's access or the node, where Landlock ABI 9 (Linux 7.1) closes Unix sockets** (a background command is the same contained command, held in its session's table instead of the call; its timeout, `bash_stop`, the session's deletion, the master's `session.stop_background` for a revoked owner and the node's stop each stop it through its guard, and the node's death ends it as any command; tests `cli/tests/background.rs`) (ADOPTION_PLAN T.8a, T.8e; experiments B.1, B.11; condition from review T8A-1, `docs/security-review/2026-11-T8a-containment.md`): each `bash` call runs as `sandbox-guard --mark <temp root> --mark-session <id> -- bwrap --die-with-parent --new-session --unshare-pid --proc /proc --dev /dev <the file view> --remount-ro / -- /.rizzy-harness/rizzy-harness sandbox-exec … -- bash -c` (`tools/src/contain.rs`, `tools/src/guard.rs`), so it is a PID namespace whose init is bwrap's; when the init dies the kernel kills every process in it, `setsid`, `nohup`, double-forked and SIGTERM-ignoring children included. **The guard** (T.8e) is one short-lived process of our own binary per command, bubblewrap's parent, started from a descriptor of `/proc/self/exe` the node opens once at start and keeps (`/proc/self/fd/<n>`: the node's own running image, whatever is at the binary's path now: review T8E-5, tested with the binary replaced by a command; `/proc/self/exe` resolved at each spawn would name a test binary's own image in the tests). Its stdin is one end of a Unix socket pair whose other end the node holds, close-on-exec: the command never holds it (tested), but for the moment between `fork` and `exec` every child the node forks has a copy, and a child that waits there (its `chdir` into a folder that does not answer, as the node's git does into a session's folder) holds every guard's channel open past the node's death (review T8E-1). So the channel is the fast path, not the only one: the guard is given the node's pid (`--node`), checks `getppid()` against it before it starts bubblewrap and at least every 500 ms after, and takes a different parent (the kernel reparents it when the node process dies, whoever holds descriptors; a node thread that ends is not that, tested) as the kill order (`a_node_killed_while_a_child_waits_before_exec_leaves_nothing`: gone within 2 s, where before the command ran on until the folder answered). The channel closing is the kill order the kernel delivers whenever the node dies and nothing else holds it, `kill -9` included; the node half-closes it to cancel (a byte first asks for the graceful stop: SIGTERM to the command's session, the namespace init's, found as bwrap's only child in `/proc/<bwrap>/task/<bwrap>/children`, then 2 s), and dropping a `ContainedChild` anywhere closes it. The guard is a child subreaper, kills with `contain::kill_namespace` (stop bwrap's group, SIGKILL the init it names, then the group) and SIGKILLs its own children, and repeats until `waitpid` gives ECHILD; a namespace's init is reaped only after every process in it is released, so that is the proof nothing is left. It then reports "ended" to the node; if something is still not reaped 2 s after the kill (a process in uninterruptible I/O), it reports "still alive" once and keeps killing, and the tool result says so instead of "cancelled". A cancel or timeout returns only after that report, or after the node's own bound for a guard that does not answer (2 s grace, 2 s, 300 ms), when the result says the end is not confirmed; with the sweep below (300 ms) and the output drain (100 ms) that fits inside the agent loop's 5 s `TOOL_CANCEL_GRACE` (asserted against the constants; review T8E-3: the bound was 5 s and could never be recorded). A tool the loop does abandon at its grace is recorded as "stop ordered … abandoned: whether what it started has ended is not known", for every tool, not "[cancelled]". A confirmed contained stop says so ("its guard confirmed the end (N ms)"), distinct from the uncontained "process group killed"; a stop signal sent to the guard itself is reported as that, not as the command's exit code. **While the node lives** (review T8E-2): when a contained command's guard dies of a signal (in its first millisecond, before bubblewrap or its init set the death signal, the namespace would run on) or does not answer a stop, the tool runs the sweep for that one session before it returns (`contain::sweep_session`: orphans only; a namespace under a live guard, another command of the same session, is never touched, tested) and the result says what it found (`a_guard_killed_in_its_first_millisecond_leaves_nothing_once_the_tool_returned`: 0 left, where 61 of 260 ran on before). bubblewrap's `--die-with-parent` names the guard's only thread, so a guard killed by itself takes the namespace with it, and the spawner thread of T.8a is gone (a command spawned from a thread that ends at once runs to its end: tested). The guard starts in `/` (bubblewrap enters the folder), so the spawn itself does not enter the session's folder; but the bash tool resolves that folder in the node first, in its async task, and a folder that already hangs holds the call there before any guard exists, where neither the cancel nor the loop's grace reaches it (review N-1, observed; the same on `main`). A folder that stops answering after that check stalls bubblewrap under its guard, which kills and reports it still alive. It costs one more `exec` of the harness binary per command: about 1.4 ms (median of 40, debug build; unchanged by the 500 ms look at its parent) over bubblewrap started directly, and about 4 MB of private memory (debug) while its command runs; it does not exist otherwise. **Second line:** at start, before it serves, the node ends namespaces a dead node left (`contain::sweep`): processes of its own user whose command line carries `--mark <its temp root>`, that are not a guard and whose parent does not carry the mark (bwrap whose guard died, or an init whose bwrap died), each stopped, checked again (same start time), its child killed, then it; and it sends SIGTERM (the guard's kill order) to every guard with its mark whose node is dead. Each is named in the log, with whether it ended. "Ended" means the process and the children it was killed with have no `/proc` entry or are zombies (a killed init waiting for a process in uninterruptible I/O has an empty command line and is not ended: review T8E-4); what has not ended within 2 s is logged STILL ALIVE (killed; it ends when the kernel releases it) and never "ended"; nothing is kept for a later start, which kills nothing that carries no mark (re-check R-2: a file of pids to kill was a way to make the node kill other processes; `the_sweep_does_not_call_a_stuck_leftover_ended`, `a_planted_leftovers_file_kills_nothing`). A session text in the log comes from a process's own arguments and is printed escaped (`a_session_with_a_line_break_injects_nothing`). A guard is told to stop only when its parent is no longer the pid of its own `--node` (its node is dead), never by comparing with the process that is starting: a second start on a live node's data directory leaves that node's commands alone (re-check R-1, `a_second_start_leaves_the_live_nodes_commands_alone`); a guard without a readable `--node` is left alone and named. A process without the mark, with another data directory's, or under a live guard is never touched (tested with the user's own other bubblewraps running); the start's sweep is tested through `serve` (`the_node_start_sweeps_what_a_dead_node_left`). **The binary's path** (review T8E-5, **closed for contained commands by T.8f**, T.8c's bind of the image replaced): the guard opens its own running image (`/proc/self/exe`, the node's: it was started from it) `O_PATH`, not close-on-exec, and gives bubblewrap `/proc/self/fd/<n>` as the program to run; bubblewrap names no such descriptor, so it stays open in the command's process (bubblewrap 0.9.0 and 0.13.1 close extra descriptors only in their monitor and their pid-1 init, never in the process that `execvp`s the program, by their source; tested on 0.13.1) and `execve` runs that inode. No path of the harness binary is bound, resolved or executed for a contained command (where the node holds a descriptor of its image, below), so nothing at that path, a session's file, a rebuild or an upgrade, ever runs as `sandbox-exec` (`cli/tests/own_image.rs`: 200 races by path, 0; before, 2 of 200 ran the session's file before Landlock, review T8F-3b), and a binary replaced or deleted under the running node changes nothing for contained commands, whatever file systems the binary and the data directory are on (`a_binary_replaced_after_the_start_still_runs_from_the_old_image`, both forms, both binds, the network on and off; before, every command after it was refused, T8C-N3). `sandbox-exec` closes every descriptor above 2 before it reads its arguments (`close_inherited`), so the command holds nothing of the image, the view or the guard (review T8F-2: by path the command held the image's descriptor and could `chmod 000` the binary; `nothing_of_the_image_reaches_the_command`). A hard link of the image into the data directory was built first and dropped: it could not be made across file systems or for a root-owned binary, and by path it was itself bound by a path after the guard's check (review T8F-1 to T8F-4). **Residuals:** a node keeps running its own build after a rebuild or upgrade until it is restarted, and says nothing (T8F-9; `doctor` names the running node's pid since A.20, from the data directory's lock, but still does not compare that node's build with the file at its path); where a session can write a name of the file the node runs (a build tree), it can change its mode (`chmod 000`), and then no guard starts and the node cannot restart until the owner restores it (T8F-5; in `doctor`'s warning). The node's own helpers (checkpoint git, graphify) and a shell left to Landlock alone are started the same way, from the node's descriptor (`Sandbox::helper`, re-check R-4: before, after a deletion every checkpoint failed while commands ran on, and in a build tree a session's file ran as the helpers' wrapper; `own_image.rs` `the_helpers_and_the_uncontained_shell_run_from_the_image`, `a_node_keeps_its_checkpoints_after_a_rebuild_and_a_deletion`). The image is held `O_PATH`, opened once from `/proc/self/exe` at start (`hold_own_image`; execute permission is enough, re-check R-1: a binary of mode 311 had no descriptor and was run by its path), so once started the node executes its binary's path nowhere; where it could hold no descriptor it says so on its containment line and in `doctor`, starts them by the path, and `[execution] sandbox = "strict"` does not offer the sandbox mode. At mode 000 the agent is told that the node's own binary could not be executed and to restore its mode (re-check R-2). Where a workspace root or a `[sandbox] write` entry holds the binary, `doctor` and the log still warn (for the mode, T8F-5) and `[execution] sandbox = "strict"` does not offer the sandbox mode. **The file view** (T.8c, `contain.rs`): the command's root is bubblewrap's fresh tmpfs, made read-only once built, holding only the system read set (`SYSTEM_READ`; the top-level links such as `/bin -> usr/bin` as links), node.toml's `[sandbox] read` (read-only) and `write` (read-write) entries as they resolve now, the session's grants (its folder and temp folder read-write; a sub-agent's folder, git directory and borrowed object store as granted), a private `/dev` (`--dev`), `/proc` (its own, or the outer one bound in the shared form) and an empty `/tmp` (the image is not in it, T.8f). A path that is not granted does not exist for the command: a sibling session's folder, `~/.ssh`, the data directory's files and the target of a link out of the session folder are absent, with Landlock and with the wrapper taken out (`outside_the_grants_is_absent`, `…_without_landlock`, `…_in_every_form_and_bind`). Landlock inside keeps the same rules (the sandbox tests run both ways). **What is bound is what the node checked** (S-2): with bubblewrap 0.10.0 or newer (the release that added `--bind-fd`; v0.9.0's source has none) each grant with an identity is bound with `--bind-fd`/`--ro-bind-fd` from a descriptor the guard opens `O_PATH` without following a link and checks against the node's device and inode; a folder swapped since refuses the command (`a_folder_swapped_for_a_link_after_the_check_is_not_mounted`, with only the view too), and bubblewrap, which mounts a descriptor through the path it names now (`realpath` of `/proc/self/fd/<n>`: a descriptor of another user namespace cannot be mounted directly), compares the mount with the descriptor and refuses on a difference, so a swap after the guard's open gets the command refused, never another folder mounted; it closes the descriptor before the command starts (no descriptor of a host folder reaches the command, tested). With an older bubblewrap (Ubuntu 24.04's 0.9.0, CI's) the grant is bound by its path and `sandbox-exec`, which runs before every contained command with or without Landlock, opens the mounted path without following a link and refuses the command on a mismatch (B.11's fallback; `with_binds_by_path_a_swapped_folder_refuses_the_command`); there bubblewrap may first create an empty folder for a nested grant inside the swapped target. Which way is chosen from `bwrap --version`, once; `doctor` names it on its `file view` line, which lists what is bound. The view costs about 0.2 ms a command (median of 30, debug build: 4.5 ms against 4.3 ms with T.8a's `--dev-bind / /`, `the_view_costs_little_per_command`). The command sees its own `/proc` only. `[execution] sandbox` (`auto` default, `strict`, `off`) decides when the sandbox mode is offered, and a node without a working bubblewrap offers none (T39); the start-up probe tries the full form, then one without a fresh `/proc` (nested containers: still contained, but `/proc` lists the outer processes), and the log and `doctor` name the form. Only the agent's shell is contained: graphify, the node's git and the PDF reader run fixed programs, not agent commands, and keep Landlock alone. Tests: `cli/tests/contain.rs`. **Residual below Landlock ABI 9** (T8A-1; **narrowed by T.8c**): Landlock mediates pathname-socket connects only from ABI 9 and abstract sockets only from ABI 6, and a command that reaches a service of the node user can ask it to start a process (one `StartTransientUnit` on the session bus; a container runtime's or a tmux server's socket), which runs outside the namespace and the sandbox and outlives the kill. Since T.8c a pathname socket outside the grants is not in the file view (`a_socket_outside_the_grants_cannot_be_reached`, with this kernel's Landlock and with an ABI 4 stand-in: `FileNotFoundError`, nothing started; before, below ABI 9 the connect went through). **What is left below ABI 9:** a socket inside a granted folder (the session's own, a `[sandbox] read` or `write` entry), and, below ABI 6 with the network on, a service on an abstract socket (not a file: no view covers it; the network off closes it, T.8b). `cli/tests/contain.rs` `a_service_outside_is_out_of_reach_by_path_and_by_name_from_abi_6` records the kernel's side, `below_abi_6_only_an_abstract_socket_still_reaches_a_service_outside` shows the rest with an ABI 4 stand-in. The node knows the effective ABI from its Landlock probe: from 9 the `doctor` and log line make the full claim; below, it is a warning that names what is left, and `[execution] sandbox = "strict"` does not offer the sandbox mode there (not relaxed by T.8c: whether the view is enough for `strict`, and for `host_shell_shared`, is the owner's decision); `auto` contains with the gap named. **Residual on every kernel** (review R-3): a read-write grant includes Landlock's socket right, so a `[sandbox] write` entry that holds another program's socket (`/tmp`: tmux, X11; `/run/user/<uid>`) is the same way out; `doctor` warns for those two folders and does not look for sockets elsewhere (`sandbox.rs` `a_write_entry_with_other_programs_sockets_is_named`). **Residual:** in a `host` chat (T39: no bubblewrap; the only agent chats inside the published image) a detached process survives cancel, timeout and the node, and `/proc` lists every process of the machine (`tools/src/bash.rs` and `cli/tests/sandbox.rs` B.1 tests record it); **closed by T.8e:** a node killed in the first millisecond after a spawn left that command running in its namespace (review T8A-2: 20 of 20 at 0 to 500 µs; 170 of 200 at 0 to 1 ms in `cli/tests/guard.rs` on `main` before T.8e; the first count, 10 of 20, came from a helper whose kill could land before its spawn, found while fixing review T8E-1), because bubblewrap's init sets its own death signal late; with the guard, 0 of 200 (`a_kill_of_the_node_just_after_a_spawn_leaves_nothing`), and a kill at the init's `setsid` 0 of 60 (`a_kill_just_after_the_inits_setsid_ends_the_namespace`). **What is left of it:** a guard killed by someone else (not the node, not a command: it is outside the namespace, and a command can neither see nor signal it, tested) in its first millisecond, before bubblewrap or its init set the death signal: SIGKILL, or SIGTERM, SIGINT or SIGHUP before the guard has blocked them (their default action applies until then; `systemctl stop` signals every process of the unit at once) leaves that namespace until the tool that saw its guard die sweeps the session, at once; if the node dies in the same moment, until the node's next start. A node that dumps core releases its descriptors only after the dump, so the kill order is late by that long (not run). The sweep finds a leftover by its command line only: a process that copies the mark into its own command line is killed with them (only the node's user can, and it names this node), and a pid reused between the check and the kill is excluded by the stop-and-recheck of its start time. The "still alive" report is proved with a FUSE read the test holds (the reader in state D after SIGKILL, `a_process_the_kernel_cannot_kill_yet_is_reported_still_alive`); other uninterruptible waits (NFS, a dying disk) are the same kernel state but not exercised. Where `children` cannot be read the kill of bubblewrap's group still ends the namespace once the init has its death signal (`without_the_children_file_the_stop_still_ends_the_namespace`, without the guard), and the guard finds its own children (an init reparented to it) by their parent in `/proc` instead; that fallback has no test. Without bubblewrap there is no guard: a `host` chat (T39) keeps the process-group kill, which neither confirms nor reaches a detached process; the graceful SIGTERM's target is read from `/proc` and could name a reused pid only if bwrap's init were reaped and its number reused between the read and the signal; a command killed by a signal reports 128 + N, not "signal". Bash runs in its own process group; cancel/timeout sends SIGTERM then SIGKILL to the group. Cancel propagates master → node as `session.cancel`. Time and steps are the node's: a call's timeout is the `timeout_secs` it states, at least 5 s, clamped to `[limits] bash_timeout_max_s` (default and maximum 3600 s, which wins over the floor), 60 s for a foreground call that states none (T.8d; it was 600 s), and a background call that states none is refused, and a turn ends after `[limits] max_steps` model calls (each sub-agent's turn has the same cap of its own; the caps are not one shared budget) (default 50, 1 to 1000) with a message naming the key. A tool call in a message cut by the model's output limit never runs, even if its arguments parse; three such messages in a row end the turn (Status, 2026-11-11). Both come only from `node.toml` (out of range stops the node at start); no agent definition, prompt or protocol message carries a step or timeout value. Output is capped: the model's copy at 30 000 characters; the live stream (`ToolOutputDelta` events, stored and replicated) at `[limits] tool_stream_bytes` per call (default 1 MiB, 4 KiB–1 GiB), after which one marker is sent and the rest is read and dropped so the child never blocks. The stream's channels are bounded (8 pipe chunks, 32 sink chunks); a full channel back-pressures the pipe reader, and cancel and timeout are awaited alongside every send. **No tool can wedge a node** (AUDIT S10): file tools open regular files only, checked before opening (a device is never opened) and again with `fstat` on the handle, opened `O_NONBLOCK`/`O_NOCTTY` so a FIFO swapped in meanwhile never blocks; `read` streams through a fixed buffer, keeps only its line window and stops at 32 MiB with a `file truncated at N bytes` note; `grep` skips non-regular files (links are not followed) and files over 8 MiB, saying how many; `edit`/`outline` refuse files over 8 MiB. Every tool gets the turn's live cancel token (`read`, `grep`, `glob` check it per chunk or entry); after a cancel a tool has 5 s (`TOOL_CANCEL_GRACE`) to return before the loop abandons it (recorded as abandoned, its end not known), so a tool stuck in a syscall cannot hold the turn (its blocking thread may linger). The node's command loop answers `Cancel` and `ApprovalRespond` inline and hands every other command to its session's lane (a bounded queue of 64 and one task per session, ended after 30 s idle): one session's commands run in order, sessions never wait for each other, a full lane nacks at once; an inline cancel also leaves a marker in the lane so a prompt queued before it is stopped too. Events are still appended only through the session's `append` (one writer, contiguous `seq`). Residual: the type check before `open` and the open itself race, so a device swapped in at that instant is opened (and refused on `fstat`); a node restart loses queued commands, which were never acked. **A restore never holds what it writes** (R.1, fix review 3 R1b-1): an undo, and the automatic apply of a sub-agent's result, stream each blob from one `cat-file --batch` through a 64 KiB buffer into the file (`git::stream_writes`, `at::Top::write_from`), after every size was checked against a limit (`git::WriteLimits`: an undo 4 GiB a file and 16 GiB in all, what the folder held; a sub-agent's result 200 MiB a file and 2 GiB in all, the defaults of `[artifacts] max_mb` and `stage_max_mb`; a sparse file counts at its full size); over a limit nothing is written and the refusal names it. Measured in its own test process (`node/tests/restore_memory.rs`, `VmHWM`): an undo writing two 200 MiB files grew the node's peak memory by 444 KiB (before: every blob was read into a map, about twice the content). |
| T10 | Another account reads or drives someone else's work | Every session carries `owner_id`; REST and WS show a session only to its owner, admins included (server admins manage accounts, workspace admins and owners manage members and defaults; neither gets chats). A workspace only groups chats: its mapping is master-side (never asserted by a node, T7) and is ignored once the owner leaves, so a chat cannot be planted in or stranded outside its owner's workspaces. A chat's owner is the master's record too: no event from a node changes it once set, and a second `session_created` (which used to move a chat to the owner of the parent it named, or to nobody) is refused and freezes the chat (§3, Gate 3 review L-2; `the_row_is_set_at_seq_1_and_its_owner_never_changes`, `a_second_session_created_moves_nothing_and_freezes_the_chat`). Only the API token, the machine owner's key on disk, sees all (an accepted gap until per-user tokens exist: T29). **The data directory stays out of every session** (it holds the API token, the session keys, `secrets.toml` and `master.db` with everyone's chats): every node runtime protects its own data directory (for the embedded node, the master's), a session cwd that is inside it, equal to it or an ancestor of it (`$HOME` with the default `~/.local/share/rizzy-harness`) is refused at create and again before every turn and restore (sessions loaded from the log included), and `read`/`write`/`edit`/`outline`/`grep`/`glob` refuse any path whose real location (links and `..` resolved, a dangling link judged by its target) is inside it, `allow_outside_workspace` included; searches rooted above it skip it. The API token is in the redaction set. bash and graphify cannot open the data directory: the sandbox grants nothing in it but the session's own temp folder (`<data>/sandbox-tmp/<session>`) and graphify's own output folder (T4). Residual: on kernels before Landlock ABI 9 a command can reach pathname Unix sockets and through them start programs outside the sandbox (R2), which is why the embedded node stays admins-only by default (T11). **Every chat has an owner and an end** (R.4, review P-6): a create records who asked for it (`create-intent:<id>` in the mirror's kv) before the node hears of it, so a `session_created` that comes after the ack, or after the ack timed out, is claimed for that person when it lands (and the claim drops another owner's parent, as on the usual path), instead of staying a chat only the API token can see; the intent is removed once the chat is claimed, or when the create was refused or never sent. Deleting a user deletes every chat of theirs on the mirror (tombstoned) and on its node, at once or at the node's next hello. Tests: `master/tests/link_hardening.rs` `a_late_session_created_is_claimed_by_whoever_asked_for_it`, `master/tests/accounts_http.rs` `deleting_a_user_deletes_their_chats_on_the_master_and_the_node`. An intent lives one hour (`CREATE_INTENT_TTL_MS`, far past the 15 s ack timeout): it is removed when its chat is claimed, when the create is refused or never sent, and otherwise by the next create after the hour, and an older one claims nothing (fix review 6 N-2; `a_create_intent_that_never_lands_is_removed_and_claims_nothing`). Residual: a late chat is claimed but not linked into the project the create named. Fix review 1's R3-1 (a node that acks before `session_created` kept a cross-owner parent in the row) is closed by the same path: the late claim runs `claim_session`, which drops it. R3-2 (a node can make many tiny frozen placeholder rows, one refused first event each) is accepted: one small row per session id, bounded by the link's queue, and no worse than the node writing real sessions of its own; a per-node quota (T8) would bound both. |
| T11 | A low-trust account gets a shell | Agentic sessions run bash on a node, so they need a per-user `can_agentic` grant (default off; admins always on) — the security boundary. A workspace role must also allow agentic work there (an extra AND, never a grant). Both are checked at create and again before every prompt, allowing approval and restore, so revoking either stops further work; running agentic turns of affected users are cancelled, and the sweep runs again for a node's running sessions each time its link comes up (after its hello), so a turn on a node that was away during the revocation is cancelled on its return; a cancel the node's queue had no room for, or that it did not acknowledge, is tried up to 5 times over about 8 s (Gate 3 review P-3; `link_hardening.rs` `a_turn_that_lost_its_grant_while_its_node_was_away_is_cancelled_on_return`, `a_revocation_cancel_that_found_the_queue_full_is_retried`). Residual: the turn keeps running while its node stays away, which is the node's own policy to bound. Deny and cancel always stay possible: after losing either, on a diverged chat, and while the account must change its password (Gate 3 review P-7; `deny_always_http.rs`, `a_diverged_chat_refuses_an_allow_and_takes_a_deny`). Chat sessions have no file or shell tools. **The embedded node is for server admins** (`[nodes] local_node_users = "admins"`, the default): it runs as the server's own OS user beside its data directory; bash there is sandboxed (T4), but before Landlock ABI 9 the session bus or a Docker socket gets a command out of the sandbox (R2), so a shell there is still treated as admin-equivalent. Other accounts with the grant get 403 ("use a remote node, or ask the server admin") at create and at every prompt, allowing approval and restore on a `local` chat, are not offered the node in `/api/nodes`, and new chats without a chosen node go to a remote one; agentic routines on `local` are refused at save and at every fire; demoting an admin cancels their running `local` turns. The API token counts as admin. `"granted"` lets every account with the grant use it, for installs where those accounts are trusted like admins. **After a replay too** (R.4, fix review 2 R2-2): the sweep at a hello reads the mirror before the node's replay arrives, so a turn that started while the link was down, which only the replay shows as running, was missed. When a replay of a session ends (the snapshot reached the node's head, or stopped), that session is swept again (`workspaces::cancel_revoked_session`); a cancel retried because the node's queue was full first checks that the turn still runs and its owner is still blocked, so a grant given back in the meantime keeps its turn. Test: `master/tests/link_hardening.rs` `a_turn_known_only_from_the_replay_is_cancelled_when_it_lands`. **The workspace `agentic` role is not a boundary yet** (review P-4, accepted until the build of `docs/nodes-credentials-environments.md`): a member demoted in a workspace can leave it, and the chat falls back to Personal, where the same node and folder are usable again with the server grant. The signed-off nodes design closes it: a node serves named workspaces and a session's folder is derived per workspace, so leaving a workspace leaves its nodes and folders too. Closed by that build, not before; until then the server-wide `can_agentic` grant, not the workspace role, is what keeps a shell from an account. |
| T12 | Password guessing / account takeover | Login throttle (5 failures per username per 15 min → 429 with `Retry-After`; while locked even the right password is refused). Order of checks at `/api/login` (`Auth::login`): body over 16 KiB → 413 before parsing (also `/api/setup`); a username that fails the account format (1–32 of `a-z 0-9 _ . -`, checked on length before any copy) or a password over 1024 bytes → the same 401 body as a wrong password, with no map entry and no hash; a locked name → 429 without waiting; then one of **4 Argon2 permits** (wait at most 2 s, else 429 `Retry-After: 1`); then a failure slot is **reserved under one lock** before hashing (so 50 parallel guesses record 5 failures and the rest get 429), kept on failure, cleared on success, given back on a store error or a cancelled request. Well-formed unknown names take the same path (permit, slot, dummy hash) as known ones. Every Argon2 run in the process (login, setup, account creation, password change and reset) shares the 4 permits, so hashing uses at most 4 × 19 MiB; a permit moves into the blocking task, so a disconnect frees it when the hash ends and a panic frees it while unwinding. The failure map holds at most 65,536 names (about 10 MiB); when full, expired names go first, then the name whose latest failure is oldest. Residuals, accepted: (1) an attacker can lock out a known username for 15 minutes; (2) anyone who can reach the port can keep all 4 permits busy, so other logins, setups and password changes get 429 while the flood lasts (no per-IP limit yet; it ends when the flood does); (3) eviction: flushing a locked name needs 65,536 newer failed names, each a full hash through the 4 permits: at ~50 ms per hash (estimate for a current x86 core, not measured) that is ~80/s, about 14 min per flush, ~7 min on hardware twice as fast, during which (2) holds for everyone. That buys at most 5 extra guesses per flush, so a single name sees at most ~2–3× the 20 guesses/hour the throttle allows. Refusing new names when full was rejected: the same cost would lock every not-yet-listed username out for 15 minutes after the flood stops. `/api/setup` gets the format check first, the body limit and the permits, but no failure throttle: the claim code has ~100 bits, a wrong code costs no hash, and a global throttle would let anyone block the owner's claim. No self-signup; admin-created accounts start with a temporary password that must be changed. The server makes it (audit L14, `http/users.rs`, `auth::temporary_password`): 20 symbols of lower-case Crockford base32 from the OS generator (100 bits, no bias: 32 divides 256), shown as 4 groups of 5; only its Argon2id hash is stored; it is returned once, in the body of the create or `POST /api/users/{id}/password` answer, and never logged, put in an event or a URL (`master/tests/temp_passwords_http.rs` captures every log line and searches the data directory). A request that names a password is refused (400), so no admin picks a weak one or types it where it can be read; the claim (`/api/setup`) is the only place a password is chosen without an account. The UI shows it in a dialog with a copy button and forgets it on close. Login rules (throttle, lockout) apply to it like any password. An admin cannot reset their own (409: they use `/api/me/password`); disabling, deleting or resetting a password signs the user out everywhere, open WebSockets included (T2). The last active admin cannot be removed or demoted, and admins cannot change their own access. Workspace managers add members by username, so the answer tells them whether a username exists: failed adds are throttled per caller (10 per 15 min → 429), accepted as a residual leak. |
| T13 | Credentials sniffed on the network | Default bind is 127.0.0.1. `--insecure-bind` serves plain HTTP with a loud warning (VPN/tailnet only); real TLS arrives with the node link (Phase 8). |
| T14 | Model output (possibly steered by injected web or repo content) phishes, exfiltrates or poses as the harness through what the chat renders | No model HTML, scripts, iframes or remote images; links show their real host (with the port when it is not the scheme's default) and odd ones need a confirm with the full URL; a link to another port, scheme or loopback name of the app's own machine is another server (the agent may have started it), so it always shows its host and port marked "another server on this machine, not this app" and always asks (Gate 3 fix review 2, W-2, `leavesApp` in `web/src/md/links.ts`); chat titles, search snippets, node names, tool names, agent-published file names and a node's error and output text show hidden characters as `<U+…>` codes outside Markdown too (W-3; titles and names keep the emoji joiner); an in-page `#` link works only between one message's own footnotes and their references (ids prefixed per message, `m<n>-fn-…`, so they never collide and a link cannot reach an app route, a button or field id, or another message; anything else is text: audit L14, `web/src/md/anchors.test.tsx`); `ui:` blocks are validated in Rust and drawn from the stored spec in a "from <agent>" frame; secret-asking questions and fields are refused; answers carry ids, never model text; the node checks every answer against its log. Details: Status, "2026-11-07 — Rich messages". |
| T16 | Routines run without a human present, partly on external webhook data: injected text steers an agent or uses its web tools as a relay, a revoked user's routine keeps running, a webhook is abused or its sender locked out | The owner's account, membership, `routines` permission and, for agentic routines, the grant and `agentic` permission are re-checked at every fire; a refusal pauses the routine. Webhook data only fills the prompt, single pass, cleaned and capped, and marks the run untrusted: new chat, memory off, `web_fetch`/`web_search` and `remember` withheld, the routine's own agent, fixed title, an untrusted note; the flag sticks to the chat for every later turn (`routine_untrusted_chats`). Agentic routines run in `ask`/`ask_all` only (no `auto`); parked approvals, delegates' included, are cancelled after 24 h. Webhook secrets are 256-bit, header-only, stored as SHA-256, compared in constant time; throttles count failures only before authentication, and audit lines carry no content. Caps per user, per workspace and per day. Details: [docs/routines.md](docs/routines.md). |
| T17 | Files stored and served from the app origin (uploads, published builds) run script, phish, crash the viewer's tab, or outlive their deletion | Kinds by magic bytes; inline only for sniffed raster image, audio and video (SVG and HEIC only in `<img>` context, with `Vary` and a variant ETag); everything else attachment + octet-stream; a per-response sandbox CSP that the guard keeps; nosniff, CORP same-origin; owner-only access (or shared-artifact members); corrected download extension; sniffed thumbnails; dimensions from headers and no browser decoding above 40 MP; no server-side decoding or unpacking; no import by URL. Per-blob keys plus `secure_delete` and a WAL truncate remove deleted files and their keys from the live files (not from SSD or copy-on-write remnants of master.db): crypto-shredding, not encryption at rest, since the keys sit in master.db beside the files (audit L8; format and test vectors in docs/media.md); account deletion cleans files, FTS rows and the WAL at once. Details: [docs/media.md](docs/media.md). |
| T18 | A compromised node or steered agent uses publishing to fill the master's disk, pull secrets, plant files in another user's artifacts, or slip files past checks; a compromised master uses stage ids to read or delete node files | Publishing is a mutating tool (approval in `ask`) and node.toml `[artifacts] enabled` can turn it off; node-side confinement (session cwd or workspace roots, never the data dir, `O_NOFOLLOW` + fd re-check, regular files only, secret-like names refused or skipped); a staged copy; stage ids resolved through the node's own index, never as paths; pull-only, windowed transfers started by a validated event from the owning node and sent only to nodes that announce the `artifacts` capability; owner, workspace and project from the publishing session's own row, never `parent_id`; the path a version was published from (`meta.source_path`) is shown to the artifact's owner only, never to the members of a shared artifact (audit L5); exact size and sha256; quotas, a free-disk floor, per-node pending and hourly caps, and per-owner lineage caps. Details: [docs/artifacts.md](docs/artifacts.md). |
| T19 | A working copy arrives with a hostile git configuration (an unpacked archive, a shared folder): `.git/config` names `core.fsmonitor`, hooks, filter, diff or signing commands, and the harness runs git there before anything is approved (every turn starts with a checkpoint) | **Turn checkpoints, their diffs and undo never use the user's repository:** they run in a harness-owned shadow repository (`<data>/checkpoints/<hash>.git`, `--git-dir`/`--work-tree`), which reads the user's objects through `objects/info/alternates` (data only; new objects go to the shadow) and copies `.git/info/exclude`; the work tree's `.gitattributes` are switched off (`GIT_ATTR_SOURCE` = empty tree, `core.attributesFile=/dev/null`) and name no defined filter in any case. **Every git process** (`node/src/git.rs`): cleared environment (only `PATH`, `HOME`, `LANG`, `TMPDIR`; no inherited `GIT_*`), `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null` (one exception: a read-only `git config --global --includes --get core.excludesFile` that finds the user's global ignore file, whose patterns are copied into the shadow), no prompts or askpass, and `-c` overrides for `core.fsmonitor`, `core.hooksPath=/dev/null`, pager, editors, `core.askPass`, `credential.helper`, `commit.gpgSign`, `protocol.allow=never` (no ssh/proxy/`ext::`/lazy fetch), `submodule.recurse`, auto gc/maintenance; diffs pass `--no-ext-diff --no-textconv`; a 120 s timeout per call, and the process group is killed on timeout or cancel. **No git process of the node runs with the user's git directory as its git directory or common directory** (R.1, review S-1, and the fix review's R1-1). The first R.1 rule (no node git call that touches files reads config a session can write) left `rev-parse` and `worktree add` running in the user's repository, and the fix review proved that one line in the session's own `.git/commondir` then put a sub-agent's worktree, and its sandbox grant, inside another repository's `.git` (R1-1). Every redirect git honours in a `.git` (`commondir`, a gitfile, `core.worktree`, `extensions.*`, alternates) steers whatever git runs there, so none runs there. **What the node needs to know is read by our own code** (`node/src/git/user_repo.rs`): `<root>/.git` must be a real directory, not a link, without `commondir` or `gitdir` (a gitfile `.git`, a link, a `commondir`: the session folder is checkpointed as a plain folder and writing sub-agents are refused, with the reason); `HEAD` is read and one symbolic ref followed to a plain loose ref file or `packed-refs` (anything unusual: unknown); the object format is the length of that id; `info/exclude` and `index` are used only when they are regular files reached by no link. **The only use of the user's repository by the node's git** is as an object store borrowed read-only through a shadow's `objects/info/alternates`, pointing at the canonical `<root>/.git/objects` our code computed, and only when that is a real directory, not a link, with no `objects/info/alternates` or `info/http-alternates` of its own (git follows those chains); otherwise nothing is borrowed (checkpoints store everything themselves) and writing sub-agents are refused. What a session can still do through that door: the node's git reads objects only by ids it computed itself, from files it hashed in the session's folder or from trees it wrote, so a forged or chained object can change only what an undo writes back into that same folder, never reveal or write anything outside it; an alternates file created after the check is followed by git (residual, same bound); a crafted pack is parsed by git (git's own robustness, residual). **Sub-agents** get a shadow of their own (`<data>/checkpoints/<hash>-sub.git`) for their base, result and the patch between them, and a node-owned folder (T32); the result is written into the parent's folder with the restore machinery; `git apply` and `worktree add` are gone. **Every remaining git process of the node, by git directory:** the shadows (all snapshot, diff, restore, copy, gc calls); the child's folder is filled through its parent's sub-agent shadow (`read-tree`, `checkout-index` with the child's folder as work tree, and `read-tree` into the child's index file), all before the child starts; `config --global --get core.excludesFile` with a shadow as its git directory (only the user's global config is read; a session cannot write `~/.gitconfig` or the file it names unless `[sandbox] write` grants the home folder, and the file's lines are only ignore patterns). Whether a published file's repository is dirty is asked of its checkpoint shadow (`diff-index` with a private copy of the user's index); its commit and branch are read from the files. Tests: `node/tests/runtime.rs` `hostile_config_in_the_main_repository_never_runs_in_the_nodes_git`, `redirects_in_the_sessions_git_never_reach_another_repository` (`commondir`, a gitfile, `core.worktree`, `extensions.worktreeConfig` with `config.worktree`, a linked `objects`, alternates to another repository: a turn, a delegation and an undo; the other repository's canary is in no event and no file, its `.git` unchanged), `a_filter_a_sub_agent_defines_never_runs_in_the_nodes_git` (a filter, fsmonitor and three hooks in the child's own git directory), `node/src/git_child_tests.rs` (R1-1 inverted), `git_bounds_tests.rs` `a_git_entry_that_redirects_is_not_used_as_a_repository`. **Residual:** a sub-agent's folder holds the bytes of the folder, not what the repository's filters would check out (an LFS repository's real files, not pointers; no eol conversion), and its changes are applied as bytes; the published file's "dirty" flag compares bytes too, so a filtered file may count as changed; a `includeIf "gitdir:…"` in the user's global config no longer matches the folder when the global ignore file is looked up; the session's own shell may still write its repository's `.git/config` (R2), which now affects only the person's own later git use there, never the node; the user's object store is trusted as data, so an attacker who controls `.git/objects` can make a checkpoint record (and undo write back) a forged blob for content git believes it already has, which only rewrites files inside that same working copy; config keys only consulted by commands the harness never runs (`alias.*`, `merge.<x>.driver`, `uploadpack`/`receivepack`, `core.alternateRefsCommand`, `sendemail.*`) are not overridden. |
| T20 | A project member plants instructions in every other member's agent through shared project memory (cross-user prompt injection, audit S12) | Project facts reach every member's turns, so only active ones do and only a manager activates them: `PATCH {status: "active"}` needs `settings` unless a manager turned on the project's `members_activate_facts` (default off), and then a member confirms only their own proposal. Adding, rewording or deleting an active fact is always a manager's. A proposal is visible only to its source chat's owner. The agent path (`remember` scope `project`) only files proposals, for a manager's chat too: activation is a human request. Project context (instructions, board, recalled facts) goes only to chats in the project and to sub-agent sessions whose parent is such a chat on the same node with the same owner: a node that names another chat as its session's parent gets nothing (§3, Gate 3 review L-1; `a_node_cannot_borrow_another_users_chat_as_parent`). Residual: with the switch on, a member (guests included) can publish text to the project; a node that hosts a project chat already receives that project's context for its turns, and can start sub-agent sessions under it (that is delegation). [docs/projects.md](docs/projects.md). **Project settings reach every member's agentic turns** (review P-5, accepted): the project's instructions, board, default folder and default node go into every member's turn, and editing them needs the workspace `settings` role, not the agentic grant, so a member with `settings` but without `agentic` steers what other members' agents run. The same cross-user prompt-injection path as shared memory above, through text a person wrote rather than a fact an agent proposed; the agentic member's own approval mode and node policy still bound what runs. It would change if instructions and default node became agentic-only to edit, or were shown to each member as the project's, with the editor named, before a turn. |
| T21 | Slow or numerous clients hold the HTTP server's connections and handlers (slowloris, a silent socket, a TLS handshake that never completes, a handler that never returns) | **The serve loop is ours** (`master/src/serve.rs`, replacing `axum::serve` and the old accept-loop in `cli/src/net.rs`): hyper's HTTP/1 server with a header-read timeout of 10 s over the whole request head (half a request line, silence and a header a byte at a time all end at the same deadline; it is also the idle time of a keep-alive connection), and a 10 s timeout on the TLS handshake. **Connection cap** 512: the permit is taken before `accept`, so at the cap the server stops accepting and clients wait in the kernel backlog; the permit lives in the connection task and is released however the connection ends. An upgraded connection (WebSocket) releases its permit at the upgrade: sockets neither count against the HTTP cap nor hold shutdown; their number is bounded per user in `ws.rs`. **Request timeout** (`master/src/limits.rs`, a route layer keyed by the route template): 30 s on every route by default, 120 s on the routes that call a provider, none on the WebSocket, uploads, blob downloads and artifact saves; a timed-out request answers 504 `{"error":"request timed out"}` and its handler future is dropped. A new route is limited by default. **Shutdown**: Ctrl-C stops accepting, lets in-flight requests finish for up to 30 s, then closes. HTTP/2 is not enabled. Tests in `master/tests/server_limits.rs`. **Residual:** the request timeout bounds the handler's future, not the work it spawned (`spawn_blocking`, detached tasks); there is no write timeout, so a client that reads a response slowly holds its connection (and permit) until it finishes or disconnects; nothing limits connections per source address. |
| T22 | A prompt-injected model uses the harness's network clients as a relay or a way inward: `web_fetch` to an internal address or to a URL it built to carry data out, a redirect or DNS answer that points inside, a backend server that redirects a request carrying an API key | **`web_fetch`:** the public-address check runs on every address the client connects to (our DNS resolver drops non-public answers, so a rebinding name cannot reach an inner address; IP-literal URLs and every redirect hop are checked by `check_url`; environment proxies are ignored). `is_public` refuses loopback, private, link-local, CGNAT, multicast, documentation, reserved IPv4 and, for IPv6, whole ranges that embed or tunnel to IPv4 rather than decoding them: `::/96` (unspecified, loopback, IPv4-compatible), SIIT, NAT64 (`64:ff9b::/32`), 6to4 (`2002::/16`), `2001::/23` (Teredo and other protocol assignments), discard-only, unique-local, link/site-local, multicast, documentation (`2001:db8::/32`, `3fff::/20`), SRv6; IPv4-mapped addresses are judged as their IPv4 address. **Provenance:** a URL is fetched only if it appears, spelled the same (host case, default port, fragment and IDN folded by the `url` crate; path, query, port, trailing dot and userinfo not), in the user's own words this turn or earlier, or in the output of a *successful* `web_search`, `web_fetch` or `read` call. Never in: any tool's error text, `bash` output (and so `tool_output`), grep/glob/outline, memory recall and per-turn page context (they ride on the user message but are stripped from the provenance set), a sub-agent's task text (written by its parent model), or the model's own text and call arguments. A sub-agent inherits its parent's set as it stands when the task is delegated (the user's words and the parent's successful search/fetch/read results; passed by the node in-process as `TurnExtra::observed`, shared by `Arc`, never from the protocol, and grandchildren inherit it the same way) and adds its own successful results; a URL the parent could not fetch is refused in the child too, so an injected parent cannot launder one through the task. Residual: a page the model legitimately read can contain URLs, so the trusted set includes them; a file the model wrote and then `read` can plant one (writing needs approval in `ask`); `www.` and the bare domain count as the same host; a pasted URL can still carry data the user typed. **mem0 backend:** every id and owner id is one percent-encoded path segment or query value (`/`, `?`, `#`, `%` cannot change the path; `.` and `..` are never sent), and the client uses the same-origin redirect policy of the embeddings client, so a mem0 server cannot send `X-API-Key` to another host. Tests: `crates/memory/tests/mem0.rs`. **`read` vouches for URLs** (review N-6, a condition on ADOPTION_PLAN T.3): a URL that appears in a file `read` returned is treated as seen by the model's source tools (`crates/agent-native/src/lib.rs`), so a prompt-injected file can hand `web_fetch` a URL that carries data. Harmless while `web_fetch` and the file tools never share a runtime (web tools run in the chat runtime, file tools on nodes). T.3 puts them in one runtime; before it lands, `read` must stop vouching (provenance only from the model's own web results and the person's messages). |
| T23 | Any account (a chat-only guest included) uploads a hostile file the server parses: a PDF that loops, inflates without bound, overflows the stack, panics or exploits the parser, inside the process that holds every key; or floods uploads and paid dictation | **The server never parses a PDF** (FIX_PLAN 3.4, AUDIT S7; `crates/master/src/pdf.rs`). A file is a PDF only by `%PDF-` at offset 0, never by name (a `.pdf` text file is text). The server starts its own binary as `rizzy-harness pdf-text` (dispatched before any runtime: one thread, no `.env`) with an empty environment, cwd `/`, stderr discarded, in its own process group. Before reading a byte the helper confines itself with Landlock (ABI 3 file rights with **no** rule, so no file can be opened at all; ABI 4 TCP connect/bind denied; `no_new_privs`), then caps its address space (`RLIMIT_AS`) at 512 MiB above its start-up mappings, so a bomb's allocation fails and the helper aborts. The server: 2 helpers at once, a request waits at most 10 s for a slot (then 503), 20 s per file, stdout capped at 400 KB (then killed), the group SIGKILLed and the child reaped on timeout, excess output or a dropped request; slots are RAII permits. **Where Landlock or ABI 4 is missing (Linux before 6.7, non-Linux) PDFs are refused** (422 "cannot read PDFs safely"): an exploited parser that could read `<data>` could return `secrets.toml` as "extracted text" to the uploader, so timeout and memory limit alone are not enough. Nothing about a file (name, bytes, text, parser message) is logged. Unsent uploads: 20 and 8 MiB of text per account (counted and inserted under one lock; per owner, so one account never fills another's), swept after the incognito TTL; taking them for a message is one transaction, so the sweep never splits a send. Dictation: one at a time per account (429, not queued) and `[voice] daily_requests` / `daily_mb` (200 / 100 MiB per UTC day) counted in memory before forwarding; a restart resets the day's counts (only an admin restarts). Tests: `pdf.rs`, `attachments.rs`, `voice.rs`, `master/tests/pdf_http.rs`, `store/tests/attachments.rs`, `cli/tests/pdf_text.rs` (real helper: text, malformed file, a 768 MiB flate bomb aborts on its limit). **Residual:** `ttf-parser` is unmaintained (`deny.toml` names this row); the helper can still spend 20 s of one CPU and 512 MiB per slot; UDP is not covered by Landlock (the helper has nothing to send but the uploader's own file); a real parser-looping PDF is covered by the supervision test with a stand-in, not by a crafted file. **tree-sitter runs in the node process** (review N-1, accepted): `outline` and the repository map run tree-sitter's C parsers on files of up to 8 MiB inside the node process that holds the node key (and, on the embedded node, the server's), on files a session can write. No exploit is known; the PDF parser was moved out of process for the same class of risk. Moving it out would cost a helper process per file (as `pdf-text`: a confined `rizzy-harness` subcommand that reads one file and writes the outline), about a process start per `outline` call and a batch protocol for the repository map, and its result would have to be re-validated by the node. Not done now; it would change with a tree-sitter memory-safety advisory, or once the map runs on large untrusted repositories by default. |
| T24 | An admin session (a careless admin, a stolen login, a second admin) uses the provider and model tests to read the server's own network: cloud metadata, a router, a database's HTTP port (SSRF, audit L3) | `POST /api/admin/providers/test`, `/providers/models/test` (chat and embeddings) and every model listing (`refresh`, the daily refresh) go through `outbound::Outbound::provider_call` (`master/src/outbound.rs`): the URL must pass `url_allowed` (https, or plain http only to `localhost`/`127.0.0.1`/`[::1]` or a listed host; no userinfo), then the connection uses `web_fetch`'s public-only resolver, which checks the address actually connected to on every connection (DNS rebinding included); a private IP literal or `localhost` is refused before any connection. Redirects stay on the same origin (`same_origin_redirects`), so a listed host cannot bounce the call inward. Local model servers are allowed by name in `HARNESS_PLAIN_HTTP_HOSTS` (read at start), `localhost` included; the refusal names the variable and the value to add. Tests: `master/tests/hardening_http.rs`, `outbound.rs`. **Residual:** chat turns and memory embeddings use the saved provider URL without this check (it was checked by `url_allowed` when saved, but https to a private address passes that); the transcription client follows cross-origin redirects; a listed host may itself be a proxy to anywhere. |
| T25 | A response leaks internals or outlives its session: a 500 with SQL text or a path, an API answer or stored file left in the browser's disk cache after logout or delete, a downgrade to plain HTTP (audit L4, L6) | The outermost layer (`master/src/response.rs`) gives every response a random 64-bit request id (`x-request-id`, generated per request, never taken from the client); any 500 body is logged with that id and replaced by `{"error":"internal error","request_id":…}`, for every handler by construction, not per call site. Every `/api/*` response, errors, stored files and 304s included, gets `Cache-Control: no-store` (the hashed UI files keep their caching). With `--tls`, `Strict-Transport-Security: max-age=15552000` (180 days, no `includeSubDomains`: the harness does not own its host's siblings), not for `localhost` names (HSTS is per name, not per port, and would switch every other local dev server to https) or IP addresses. Tests: `master/tests/hardening_http.rs`. **Residual:** 5xx other than 500 (502 from a provider, 503 node offline) keep their messages; a panic in a handler closes the connection without a response; behind a TLS-terminating proxy the proxy must send HSTS. |
| T26 | Another local user reads a secret file the server uses (a restored backup, a copied `.env`, a key file with a loose umask) (audit L7) | At start `serve` and `node run` refuse any secret file whose mode has group or other bits (`mode & 0o077`), before reading it, with the path, the mode and `chmod 600 <file>`: `secrets.toml`, `api-token`, `claim-code` (both in `load_or_create_secret`, so every reader), private keys in `keys/` (`*-key.der`, `node.key`) and the `.env` `serve` loads. A symlink is judged by its target. Files the harness writes are created 0600 by `open(mode)`, never chmod-ed afterwards; `secrets.toml`'s temp file is removed and created anew, so a stale one cannot keep a looser mode. Unix only (other systems have no mode bits; nothing is checked there). Tests: `cli/tests/data_dir.rs` (`open_secret_files_stop_the_server`), `auth.rs`, `secrets.rs`. **Ownership** (R.4, review N-3): the same check refuses a secret file owned by another user, and the data directory is created 0700 (not created under the umask and chmod-ed after), refused when another user owns it, and made 0700 when it exists looser, a failing chmod stopping the start instead of being ignored (`secrets::check_data_dir`; tests `secrets.rs` `a_secret_file_of_another_user_is_refused`, `cli/tests/data_dir.rs` `a_data_dir_of_another_user_stops_the_server`). **Residual:** `master.db` (chats, blob keys) is protected by the 0700 data directory only; `link::write_private` reuses a stale temp file's mode. |
| T27 | An agent runs with more power than the one named: a sub-agent-only agent made a chat's agent (by `agent_id`, `@mention`, `set_agent`, a default or a routine), a removed agent id resolving to a stronger workspace default, an alias shadowing another agent, or a read-only agent writing | `available_as` is enforced on the master in `Resolver::top_level` for every path that picks a chat's or a turn's agent (typed `SubagentOnly`, 400); rosters keep only sub-agents, and a sub-agent's spec carries no delegates (depth one). A stale id fails closed: the chat's prompt is refused with `AgentGone` (409, naming the id; the chat stays readable and `set_agent` makes it usable), a default setting naming a missing or sub-agent-only agent with `BadDefault` (409, naming the setting), an unknown explicit id with `UnknownAgent` (400); routines naming one are blocked. Never a stand-in agent. One namespace of ids and aliases, refused on create, settled with ids winning on load. **A delegate is also never run with less than it names** (ADOPTION_PLAN A.11): on a node, a delegate that declares tools of which none is registered, enabled, allowed to it and not in the node's `denied_tools` is left out of the `delegate` tool's list and a call to it by name is refused before any child session exists ("@researcher was not started: none of its tools (web search, web fetch, memory) are available on this node..."). The web and memory tools exist only in the chat runtime, so this is Researcher on an agentic node today; the rule is about the resolved tool list, not the name, so it covers custom agents. An agent that declares no tools on purpose (`tools: []`, Smol) or `all` is unaffected, and one with some tools available runs with them. Undone when web and memory tools reach nodes (ADOPTION_PLAN T.3). Tests: `crates/master/tests/agent_roles_http.rs`, `orchestration.rs` (`researcher_is_not_offered_on_a_node_without_its_tools`), `crates/agent-native/src/delegate_availability_tests.rs`, the roster table in `crates/agents/tests/agents.rs`. **Residual:** Scout and Reviewer are read-only *by tool list only* (offered `read`, `grep`, `glob`, `outline`, `graph_query`; the loop refuses any other tool call). That is not a sandbox: a file or custom agent can give any agent `bash`, and there is no read-only sandboxed shell yet, so a reviewer cannot run `git diff` or the tests. Node policy still bounds every tool (T4, T5). |
| T28 | The node's store cannot append (full disk, I/O error, a corrupted or locked database, perhaps brought about on purpose by a command that fills the disk), and the agent keeps acting with nothing in the log: a tool runs, an approval is acted on, the model is called, none of it auditable or visible to the user (final review A6, A7) | **A failed append ends the turn** (§2, "A failed append ends the turn"): `TurnHost::emit` is fallible; the native loop (`agent-native`, `Logged`) stops at the first failure, cancels the turn's token (running tool and parallel delegations stop within the grace period) and fails every later append untried; the node's session denies an approval whose request or answer was not written, skips the after-turn checkpoint, tries the closing `error` and `turn_ended` once each, frees the turn slot and, if the end was not written, closes that turn before the next prompt. A prompt whose `turn_started` or message cannot be written is refused with the store's reason, so the sender sees it. One `tracing::error!` per turn, with the store's error only. Tests: `agent-native` (`a_tool_does_not_run_when_its_start_cannot_be_logged`, `no_model_request_after_a_result_cannot_be_logged`, `a_failed_append_cancels_the_parallel_delegations`), `node/tests/runtime.rs` (`a_tool_does_not_run_when_its_event_cannot_be_written`, a real SQLite store broken by a trigger). Also: an approval whose answer was not written is denied at the turn's end, so the log never leaves it pending; a sub-agent's worktree is not merged after its own or its parent's log failed, and the parent's result says the sub-agent stopped because its log could not be written (each session's log gates its own effects, §2); a staged file whose `artifact_staged` cannot be written is released and, during a turn, ends it; a `before` checkpoint ref whose event failed is deleted. More tests in `node/tests/runtime.rs` (`an_approval_whose_answer_was_not_written_is_closed_with_the_turn`, `a_sub_agent_whose_log_failed_changes_nothing_in_the_folder`, `a_failed_append_kills_the_running_command`, `an_unclosed_turn_is_closed_once_by_a_restart`) and `node/tests/artifacts.rs`. **Residual:** a tool that was already running when a later append failed (one of its output deltas) has had its effect, and its result is not recorded; a turn whose end could not be written shows as running in the UI until the next prompt or a node restart closes it, and for a sub-agent's session (which gets no further prompt) until a node restart; the mirror and the UI learn of a mid-turn failure only when the store works again (no other channel, no new protocol message). Undo/restore is under the rule since A.9 (T33). |
| T29 | A script, or anyone who gets a copy of the one static API token, reads or drives every user's chats (an accepted gap against "chats are private to their owner, for admins too", A4 of the final review) | **Not fixed; recorded.** A request with `Authorization: Bearer <token>` that matches the 0600 `<data>/api-token` file becomes `Principal::api_token()`: `Role::Admin`, `can_agentic`, `user_id: None` (`auth.rs:121-128`, `:740-751`), and `can_see` is true for `user_id: None` (`auth.rs:137-143`). So the token lists every chat (`list_sessions`, `http.rs:1062`), reads one and its events (`visible_session`, `http.rs:317-326`, used by `get_session`, `events`, `delete_session`), sends prompts (`prompt`, `http.rs:1314`), approves tool calls (`approval`, `http.rs:1435`), cancels, restores, and opens a WebSocket that streams any session (`ws.rs:190`, `may_see` at `:305`); routines, media, projects and artifacts use the same `can_see`. Its agentic grant is unconditional and workspace checks treat it as owner (`workspaces.rs:312`), so approving a tool in someone's chat makes that person's node run it. Every `require_admin` endpoint accepts it (`http.rs:295-301`): users, providers, nodes and join tokens, workspaces, storage, memory settings. It has no password, so `change_password` refuses it (`http.rs:562-572`); the webhook handler never accepts it (`hooks.rs:19`). The `Origin` rule does not apply: `needs_origin` is false whenever `Authorization: Bearer` is present (`auth.rs:804-820`) and `guard` lets it through without `Origin` (`:793-796`). A given `Origin` is still checked against the allowlist, as is `Host`. That is right for a non-cookie credential (a cross-site page cannot set `Authorization`, so there is nothing to forge), but it means the token is only as safe as its file and the hands holding it. A bad `Bearer` is refused outright, not retried as a cookie. At rest: `<data>/api-token`, mode 0600, checked at start (T26); 32 random bytes, constant-time compare. It is never read from a URL (only the header), is in the redaction set for events and tool output (`cli/main.rs:244-248`), and the data directory is closed to sessions (T10); `rizzy-harness token` prints it (`cli/main.rs:864-868`, file path `:633-635`). No per-request log line records which requests used it. **Accepted until per-user, scoped tokens exist** (adoption plan T.5, after fix plan 4.1); scoping this one now would break the README's script uses. **Operator, meanwhile:** treat the token as an admin password and as read access to every chat; do not give it to automation that needs one user's access; if no script needs it, do not print or copy it (the file is created at first start either way); rotate it by deleting `<data>/api-token` and restarting. **Committed test for T.5:** user A's token cannot read user B's chat. |
| T30 | A crash, `kill -9`, full disk or cancelled turn in the middle of `edit` or `write` leaves the user's file truncated or half-written, with nothing to restore it from if no checkpoint holds it (ADOPTION_PLAN A.8) | `edit` and `write` replace files atomically (`tools/src/fs.rs`, `replace_with`), in the node process (not in `sandbox-exec`): the parent directory is opened once and every step is relative to it; the target must be absent or a regular file opened `O_WRONLY\|O_NOFOLLOW` (a symlink is refused, a read-only file stays unchanged); the bytes go to `.rizzy-harness-write.<128-bit random>.tmp` created `O_CREAT\|O_EXCL`, mode 0600, which is flushed, given the target's permission bits (new file: 0666 less the umask read from `/proc/self/status`), `fsync`ed and `renameat`ed over the target, then the directory is `fsync`ed; the temp file is unlinked on every error path (RAII guard) and the work runs in `spawn_blocking`, so a dropped turn does not stop it halfway. Just before the rename the target is `fstatat`ed again without following links: a different inode, a symlink or a newly appeared file aborts with nothing replaced. A directory the node cannot write is an error, never an in-place fallback. Tests in `fs.rs` (`mod atomic`: a write failing halfway keeps the original byte-identical with no temp file; 0640 kept; symlink refused; link swapped in mid-write refused; new-file mode; read-only file and read-only directory refused). **Residual:** the re-check and the rename are two syscalls, so a symlink swapped in between is *replaced* by the new file (never written through; the link's target is untouched); `edit` reads by path and writes by the directory handle, so a change between its read and its write is lost (staleness is T.2); setuid/setgid bits, ownership, hard links (split), extended attributes and ACLs, and a directory's default ACL for a new file are not kept; a `kill -9` between create and rename leaves a recognisable temp file. |
| T32 | A sub-agent that can change files runs in the parent's folder because its worktree could not be made (not a repository, `worktree add` failing, a hostile repository config): parallel sub-agents and the parent overwrite each other's files, and a child whose log failed has already changed the folder, so T28's "not applied" rule cannot hold (final review A8) | **Isolation or no start** (§5, checkpoints): `Session::isolate` decides from the child's effective tools on this node (`write`, `edit` or `bash`, after the agent's own denials and the node's `denied_tools`); without a worktree the delegation is refused before the child session is created, with the reason and what would work as the parent's tool result, and a half-made worktree and base ref are removed. The fallback to the shared folder and its note are gone; no setting restores them. Read-only sub-agents still work in the parent's folder. Tests in `node/tests/runtime.rs`: `a_writing_sub_agent_is_refused_outside_a_repository` (`write`, `edit` and `bash`; no child session, folder unchanged), `a_writing_sub_agent_is_refused_when_its_folder_cannot_be_made` (`<data>/sessions` is a file; no ref left), `a_read_only_sub_agent_runs_outside_a_repository`, `a_writing_sub_agent_changes_are_applied_from_its_worktree` (from its own folder since R.1), `parallel_writing_sub_agents_do_not_see_each_other`. **A sub-agent's folder is node-owned** (R.1 follow-up, the fix review's R1-1 to R1-3; `git::SubRepo::make_folder`, `session.rs` `isolate`): `<data>/sessions/<child id>/files`, beside the checkpoint store, in the folder of per-session folders the signed-off node layout uses (docs/nodes-credentials-environments.md §4.11). The sessions folder is protected like the data directory (`NodePolicy::protect` at start; no session folder may be it, hold it or be inside it, no file tool reaches it, no sandbox grant exposes it) except for the one child each folder belongs to: the node passes that child its folder (`harness_core::folder::OwnedFolder`, through `TurnHost::owned_folder`), the file tools let exactly that folder through, and its sandbox gets read-write on it (and on its `.git` when it works in a sub-folder) and read-only on the user's `<root>/.git/objects`. Nothing is judged from a path or from a file the child can write. The node fills the folder from the sub-agent shadow (the base snapshot) and writes it a git directory of its own (`files/.git`: `HEAD` at the user's `HEAD` commit, a minimal `config`, the user's object store as its alternates, the index from `HEAD`'s tree made by the shadow) before the child starts; **the node never runs git with that directory**, so the child may write its config and hooks freely. No other session can reach the folder while it is filled or while the child works, so the paths the node's git uses are not swapped under it (R1-2). The child's grant on the user's repository is read-only `objects` only: no `worktree add`, no `worktrees/<id>`, no write on the objects. A writing sub-agent of a sub-agent is refused; a read-only one (where `[limits] max_delegation_depth`, T5, allows a second level at all) works in its parent's node-owned folder (`node/tests/runtime.rs` `a_nested_delegation_reads_in_the_childs_folder_and_never_writes`). The folders of sub-agents running when the node stopped are removed at the next start. Tests: `cli/tests/sub_agent_sandbox.rs` (`git status`, `add`, `commit`, `diff` and `log` with the user's history work in the child's folder; the child cannot write the user's `.git` config, hooks, `info/attributes`, `packed-refs` or objects, nor read its config; the child's own alternates pointed at another repository read nothing there; a sibling's folder is unreadable and unwritable to it; the child cannot rename its folder; the parent can neither list nor write the child's folder nor rename its own; the file tools agree), `node/src/git_child_tests.rs` (R1-1 and R1-2 inverted, 120 racing tries, 0 landed), `node/tests/runtime.rs` (`a_writing_sub_agent_is_refused_when_its_folder_cannot_be_made`, the delegation flows below). The child's own `git status`/`git diff` show the parent's uncommitted work as changes against `HEAD`. Since R.1 a writing sub-agent also needs the node's checkpoint store (`checkpoints_dir`, always set by `node run` and `serve`). Changes that conflict (a file the folder changed since the base) are kept in the sub-agent shadow, not applied; the note names the snapshot. **Residual:** delegating file changes in a folder that is not a plain git repository within the workspace roots no longer works (the parent makes the change itself, or the user runs `git init`); the child can read all of the repository's objects (its whole history, as the parent can); a child writing objects into its own `.git` can fill the disk (no quota). Since fix review 4 (R1c-1) every grant of the child carries the identity the node took: its folder and `.git` when made, the user's object store when accepted; a store replaced since is not granted (no history for the child), and the wrapper re-checks each identity on the descriptor it opens (T4). **Accepted (R.4)** from the R.1 residuals and fix review 3: `worktree add --orphan` (a repository with no commit) needs git 2.42 or newer; on older git a writing sub-agent there is refused. A child session's `cwd` is a path inside the data directory and goes into `session_created` and the system prompt, so the model and the chat learn where the data directory is (fix review 3 N-4); the data directory is closed to sessions by the sandbox and the file tools, so knowing its path gives nothing. `NodeRuntime::start` empties `<checkpoints>/../sessions` unconditionally (fix review 3 N-5); in the shipped binary that is `<data>/sessions`, but an embedding that pointed the checkpoint store elsewhere would empty another folder: tying it to the data directory explicitly, with a marker the node wrote, is the fix, not done here. |
| T33 | An undo or rewind overwrites work no checkpoint holds: the user's own edits since the turn, or anything else changed in the folder, discarded by "Undo anyway" (`force`) with nothing to bring it back; or the restore changes files and its events then fail to append, so the log does not show it (final review A14, missed 9) | **The checkpoint root is found on the file system, never from git's configuration** (R.1, review S-3): the nearest folder at or above the session folder that holds a `.git` entry (`git::find_repo`, one `lstat` per level), never above the innermost workspace root the session is in; git is never asked (T19), so `core.worktree`, `commondir`, a gitfile and the like have no effect on it; a `.git` that is not a plain repository directory makes the session folder a plain one. The review proved that one `git config core.worktree /home/u` from the agent's shell had moved every later checkpoint to the home folder: file names from other projects in `checkpoint` events, an undo writing and deleting there. A repository whose folder holds the node's data directory (one at `$HOME`), or a repository above the workspace root, is not used: the session folder alone is checkpointed as a plain folder, so undo keeps working for what the session may touch and nothing outside it is read; a sub-agent that changes files is then refused ("not a git repository"). **A restore never writes or deletes outside its folder** (`git::check_inside`, in `restore_plan` and again right before `restore_paths` writes): every path relative with plain components, the folder still at its real path, and the deepest existing folder on the way to each path really inside it and outside every protected folder; one failing path refuses the whole restore ("nothing was changed: the checkpoint names …"). Before, a forced undo deleted through a link the agent had put in place of a folder. Since the R.1 follow-up the writes and deletes themselves are made by our code relative to the folder's top, opened once, and to directories opened from it with `O_NOFOLLOW` (`node/src/git/at.rs`; contents from `cat-file --batch` in the shadow; `checkout-index` and deletes by path are gone), so a link swapped in for a folder at any moment is never written or deleted through: a link met where a directory is expected is replaced by a directory for a write and means "already gone" for a delete; the folder's top cannot be swapped by any sandboxed shell (its parent is in no grant, `cli/tests/sub_agent_sandbox.rs`). Test: `git_bounds_tests.rs` `a_folder_swapped_for_a_link_during_an_undo_is_never_written_through` (120 tries against a thread swapping the folder for a link to another one: nothing there is ever written or deleted). Tests: `node/src/git_bounds_tests.rs` (`core.worktree` has no effect; a gitfile, a linked `.git` and `commondir` are not used; a repository above the workspace root and one holding the data directory are not the root; names stay inside the folder; a restore through a link is refused). **A restore first saves the folder** (`session.rs`, `restore`): a snapshot of the folder as it is, at `refs/harness/<session>/restore-<id>/before`, recorded as `checkpoint{phase: before_restore, turn_id: restore-<id>, files: the paths it writes}` **before any file changes**. If the snapshot fails (an unreadable file, the 60 s limit) or its event cannot be appended, nothing is restored, the ref is deleted and the caller gets the reason. The unforced check (files changed since `expected`) uses the same snapshot, so only a forced restore costs one more scan of the folder. Then the files are written, the result is snapshotted at `.../restore-<id>/after` and recorded as `checkpoint{after}` for `restore-<id>`, then `restored` for each undone turn. A failure after the `before_restore` event names that checkpoint and ref in the error. **All or nothing (A.16):** a checkpoint in a git repository borrows the user's objects (`objects/info/alternates`), and the user's own history rewrite plus pruning (`gc --prune=now`, `repack -ad` after the reflog expired, about a month of git's defaults, a re-clone without unpushed work) can remove a blob or tree it needs; B.2 showed a restore then deleted the file (`checkout-index -f` unlinks first) and left the folder half-restored. Now the restore reads its diff before anything else (`git::restore_diff`; a failure because a tree is gone, found by `rev-list --objects --missing=print --filter=blob:none`, gets the plain reason), then takes the pre-restore snapshot (a ref only) and makes its plan, and **before the `before_restore` event** makes exactly the blobs the plan writes safe (`git::secure_blobs`; not those of paths it skips, such as inside a nested repository, which only the snapshot reveals): `cat-file --batch-check` on those blobs (machine-readable `<id> missing`), and when the shadow borrows, one `pack-objects` that writes them as a pack into the shadow's own `objects/pack` (read through the alternates; nothing written or touched in the user's repository; `hash-object -w` would only "freshen" the user's copy; a failed copy removes its own `tmp_*` files and index-less pack, nothing else). A missing one refuses the restore with "nothing was changed: this checkpoint depends on content that the git repository in this folder no longer has (…history rewritten … cleaned up by `git gc`)", naming the files; the folder is not touched, no event is written and the snapshot's ref is deleted. The write deletes the paths the turn added first (a path that is missing, under a parent that is no longer a directory, or now a directory, counts as already gone; any other failure is an error), then builds a private index from the blob ids only (`update-index -z --index-info`, then `checkout-index -f -z --stdin`), so it reads no tree and needs no other object; this order handles a file and a directory swapped under one name, at any depth. **A directory the restore replaces by a file** is walked first: files in it that the pre-restore snapshot does not hold (ignored ones, say) refuse an unforced restore, naming the directory; a forced one deletes them and the chat note after it names them as deleted and not saved; one holding a `.git` is refused either way. **An interrupted restore is restorable:** a `before_restore` checkpoint whose `after` never came (the restore failed while writing) is restored through the same call: its paths (the event's `files`) go back to their saved content over the folder as it is, after the folder is saved again; such a restore never refuses for edits made since the failure (it compares the folder with itself), so it overwrites them, and the chat note after it names the checkpoint that saved them. A turn without `after` stays "incomplete". **Self-repair (A.16):** a shadow index that still names a blob the user's repository pruned (an unchanged file after `git reset` plus gc) made `write-tree` fail on every later snapshot, so turns ran without checkpoints; now a failed `write-tree` (the step, never git's message) rebuilds the index from the folder once with the same staging (nested-repository rules kept) and retries; a second failure is reported. No checkpoint or ref is deleted. A turn that runs without a checkpoint for any reason gets an `error` note in the chat ("This turn has no checkpoint, so it cannot be undone…"), not only a log line. **Copies of what an undo needs (A.17):** after a turn's `after` checkpoint is logged, `git::pin_changed` takes one `diff-tree -r -t` of before and after (no folder scan), checks the objects with `cat-file --batch-check`, and copies the old blob of every changed or deleted path plus both snapshots' root trees and the trees on the way to each change into the shadow's own store with the same `pack-objects` as `secure_blobs` (nothing written or touched in the user's repository), without flushing the pack to disk (`core.fsync=none`: git's flush of a one-blob pack cost 0.2 to 1.4 s on a slow rotational disk, against 1 ms without; the copies are then as durable as the checkpoint's own loose objects and refs, which git does not flush either; a restore's copy keeps git's default). Established by test (`git/upkeep.rs`): the shadow's own `gc` (`repack -d -l`) drops a local copy of an object the alternate has packed, packed or loose (a loose user copy keeps it, by `--local`'s rules), so the copy is a pack with a `.keep` file, which `gc`, `gc --prune=now`, `repack -a -d -l` and `repack -A -d` all leave alone; the file's lines name the turn's checkpoint group (`harness-pin refs/harness/<session>/<turn>`), `git::prune` removes the lines of every group it no longer keeps and the file with the last one, and the next collection drops the copy. A `.keep` file with a line that is not ours is never touched. Only a repository folder gets copies (a plain folder's shadow owns everything and gc keeps what a ref reaches). A restore's own checkpoint is copied the same way (the versions it overwrote), so undoing a restore does not depend on the user's repository either. When an old version is already gone at the end of the turn, the chat gets an `error` note ("This turn's undo is incomplete: the earlier versions of `a.txt`, … were already gone …"), the turn ends normally and its undo is refused as above. **Garbage collection (A.18):** `git::prune` deletes the dropped groups' refs and releases their copies on every turn, and counts the dropped groups in `harness-gc` in the shadow (`last <seconds>`, `dropped <n>`; survives a restart); `git gc --prune=<its start − 1 s> --quiet` runs from a task spawned after `turn_ended` is written (and after a restore) when 16 groups were dropped since the last one, or `count-objects -v` shows 6,700 loose objects or 50 packs (git's own auto-gc thresholds), and not within 5 minutes of the last start. **It is serialised with everything that writes there (review F2):** one async read-write lock per shadow (`git::hold`); a snapshot, a copy and a whole restore take it shared, a collection takes it alone for its whole run, so a collection waits for them and a snapshot that starts while one runs (the next turn's "before", at most once in sixteen turns) waits for it to finish; shared holds are taken around a step by the session, never nested and never across anything that waits on a person. Without that, an old unreachable object a snapshot only "freshens" after gc counted it as expired could be pruned under the new checkpoint. It also prunes nothing younger than its start, runs with `gc.writeCommitGraph=false` and `repack.writeBitmaps=false` (the shadow needs neither; a commit-graph lock left by a killed gc made every later one fail, F1), and checks the rule again under the lock. A failed collection (a kept checkpoint reaches an object the user's repository lost, a damaged pack) is logged once with git's first line and retried after an interval that doubles per failure in a row, up to a day; a success resets it (`failures` in `harness-gc`, F3). At start-up every pack one of our `.keep` files protects is checked with `git verify-pack`, and the `.keep` of one git rejects (a copy damaged by a power loss) is released with a warning. **Changes between turns (review F5):** the person's own edits and commits between two turns are not the "before" of any turn's change; at the start of a turn, the diff between the session's last recorded state (the previous turn's `after` or a restore's result, kept in memory and read from the log at load) and the new "before" is copied the same way into the new turn's group, so a rewind across them survives a prune. A turn that changed nothing copies nothing; when the agent's prune took a snapshot's tree, the note says the content was already gone; a restore copies its saved checkpoint's content right after the `before_restore` event, so the way back from a restore that fails while writing is covered too. **Lock files (A.18):** a private index and its `.lock` are removed by a guard on every path, also when the call is dropped at the 60 s snapshot limit with git killed holding the lock; at start-up and before each collection, `*.lock` files at the shadow's top and under `refs/`, and `harness-index-*` files, older than 5 minutes (two `GIT_TIMEOUT`s plus a minute: no command of ours lives longer) are removed; younger ones may belong to a running command and stay. An undo checks that a ref still names the checkpoint (`git::exists`), since a dropped one's objects stay until the next collection. Tests: `node/tests/runtime.rs` (`shadow_checkpoints_keep_latest_and_rewind`: a dropped checkpoint is refused although its objects remain), `node/tests/checkpoint_upkeep.rs` (undo after the user rewrites and prunes; the agent pruning inside its turn, named in the chat, in a dirty and a clean folder; pruning in a later turn; copies survive the shadow's own gc; only changed files are copied and copies are released, 60 turns bounded; a plain folder gets none; 30 turns with two kept run one collection, counted by a stand-in `git`; bounded store in a plain folder; a rewind after a collection and a user prune), `node/src/git/upkeep.rs` (git's behaviour above; pinning and collecting run nothing of the user's config and change no file of the user's `.git`; release; the rule; the prune cutoff; a timed-out snapshot leaves no lock; the start-up sweep). Tests: `node/tests/checkpoint_gc.rs` (B.2's eighteen, now asserting this, plus: two files with one blob lost change neither, a borrowed blob is copied by the restore, a blob pruned between the check and the write (a stand-in `git` runs `gc --prune=now` when `checkout-index` starts) is still restored, a read-only folder fails the write after the event and the named checkpoint restores the folder, a turn without a checkpoint says so), `node/src/git.rs` (`securing_blobs_copies_them_and_writes_nothing_in_the_users_repository`, with fsmonitor and hook configured; `securing_a_missing_blob_is_refused_with_its_path`). **Undo of a restore** is an ordinary restore of `restore-<id>` (its `before` and `after` are a turn's), through the existing protocol and HTTP route; that restore saves the folder again first. A pre-restore checkpoint is one group under `checkpoints_keep`, counted like a turn and pruned after each successful restore. Tests: `node/tests/runtime.rs` (`a_forced_restore_can_be_undone`, `a_restore_whose_snapshot_cannot_be_logged_changes_nothing`, `a_restore_whose_snapshot_fails_changes_nothing`, `pre_restore_snapshots_obey_checkpoints_keep`). **Nested repositories (A.15):** a git repository inside the folder (a directory holding a `.git` directory, file or symlink, with or without a commit, registered submodule or not) is left out of every snapshot, never as a gitlink and never as files, so one `git init` no longer disables undo for the whole folder (`add -A` failed on a repository without a commit) and another repository's work tree is never captured without its history and restored over the user's uncommitted work. Only what git itself treats as a repository counts (`ls-files -o` lists it as `dir/`, a gitlink, or a tracked file replaced by a directory holding a `.git`, which git drops silently): a planted `.git` (empty directory, junk file, `git init` over tracked files) hides nothing, since git keeps entering a folder whose files the index tracks; the turn's change list and its undo cover every file there, and git never stages an entry named `.git`, so a restore never writes or deletes one. A restore skips exactly what a snapshot (before, after, or the folder just before the restore) recorded as left out, plus older gitlinks, and lists none as restored; `restored.excluded` names them. The change list leaves out paths under a repository either snapshot left out (not recorded, never "deleted"). The chat gets an `error` note, starting with what was restored, only when repositories appeared or went away during the undone turn, or a folder the undo wrote into holds a `.git`; repositories that were there all along appear in `excluded` alone. Names in events and notes are capped at 50 (`excluded_more` counts the rest), control characters replaced; the snapshot's commit message keeps every one (hex), because a restore must skip all of them. Sub-agent base and result snapshots use the same staging. The checkpoint directory is made absolute (git runs in the folder's top). Detection (`git.rs`, `stage_without_nested`) runs with the hardened base and parses `-z` output; nothing is run or opened inside a nested repository, its `.git` is never renamed or followed. Tests: `node/src/git_nested_tests.rs` (B.5's twelve, now asserting this; depth, spaces, non-UTF-8, `.git` symlinks, broken `.git`, ignored folders; a `.git` planted mid-turn as an empty directory, a junk file, `git init`, `git init` plus commit, over tracked files, in a repository and a plain folder; a file replaced by a repository; capped and cleaned names; a gitlink from an older snapshot; staging equal to `add -A` without nested repositories; a relative checkpoint directory), `node/tests/runtime.rs` (`nested_repositories_are_named_in_checkpoints_and_after_an_undo`, `hostile_nested_repo_config_does_not_run_on_checkpoint_or_undo`, `a_writing_sub_agent_runs_beside_a_nested_repository_without_a_commit`). **Busy folders (A.15):** a node-wide registry of folders being restored (`Inner::restoring`, one mutex, never held across an await; `session.rs`, `claim_folder`, `claim_turn`). A restore registers its folder (the checkpoint's: a repository's top) only when no other session has a turn running in an overlapping folder (same, containing or contained, by real path: "another chat is working in this folder right now; wait for it to finish or cancel it") and no overlapping restore is registered; a turn, a sub-agent's included, does not start in an overlapping folder while one is ("an undo is running in this folder; try again in a moment"). Check and insert are under the same lock as a turn's start; a guard removes the entry on every path. The session's own running turn already refuses its restore, which covers its sub-agents; another session's sub-agent works in its own node-owned folder, but only inside its parent's running turn, whose folder overlaps a restore there. Tests: `node/tests/runtime.rs` (`a_restore_is_refused_while_another_chat_works_in_the_folder`, `a_restore_is_refused_when_the_busy_chat_reaches_the_folder_through_a_symlink`, which since R.1 shows that a chat whose folder became a link cannot start a turn at all, `a_failed_restore_does_not_keep_the_folder_busy`), `node/tests/slow_restore.rs` (no turn and no second restore during a slow restore). **Residual:** the chat shows no undo button for a restore (`before_restore` is not a turn; the TS fold ignores the phase), so undoing one needs the API; an edit made between the snapshot and the write is lost (milliseconds, as before); **undo still depends on the user's repository for: a turn older than `checkpoints_keep` (its checkpoint is gone anyway); content already gone when the turn ended (the agent rewrote and pruned history inside its own turn: the chat says so and that undo is refused; in a clean folder the pruned tree also takes the turn's change list, and the turn says it has no checkpoint); the person's own changes between two turns when they were already pruned before the next turn started (they are copied at that next turn's start); and the files a turn did not change, which an undo never writes**; copying the old versions at the start of the turn would close the in-turn case, but it is not known in advance which files a turn will change, so it would mean copying the whole tree or intercepting writes (rejected); while a kept checkpoint reaches an object the user's repository lost, the shadow's own gc fails (logged, retried after 5 minutes doubling up to a day) until that checkpoint is pruned, and the store grows meanwhile; each turn in a repository folder adds one small pack (about the compressed size of the old versions), so up to `checkpoints_keep` kept packs plus the released ones awaiting the next collection; a dropped checkpoint's objects stay until that collection (16 drops or 5 minutes later); git's own concurrent-gc race (an object freshened between prune's check and its unlink) applies to a snapshot running during a collection, as with any git; the write itself is not atomic across files: with every blob local, git still writes file by file (unlink, then write), so a read-only folder, a full disk or a path git cannot replace fails partway and leaves earlier files written, and a file being written when the disk fills is lost from the folder; each case returns an error naming the pre-restore checkpoint, which restores the rest; a forced restore that replaces a directory by a file deletes the files in it that no checkpoint holds (ignored ones, say), and says which; it cannot bring them back; the blobs a restore copies (`secure_blobs`) stay in the shadow only until its next collection, which drops objects the user's repository has packed (`repack -l`), so that copy protects the restore; the checkpoint itself is protected by the turn's kept copies (A.17); a restore of an interrupted restore overwrites edits made since without asking (they are saved first, and the note names that checkpoint), and matches the paths its event lists by their lossy UTF-8 form, so of two names that differ only in bytes that are not UTF-8, both are put back; a missing tree is still refused for the whole restore even when it lies under a nested repository the restore would skip (the diff must read it); emptied directories of deleted files are left in place, as before; the one rebuild rehashes every file of the folder (as a first checkpoint does), and after it a repository made over tracked files counts as nested (left out), since the rebuilt index tracks nothing there; snapshots (not the copy) still "freshen" objects in the user's repository when they hash a file whose content git already has there (git updates the modification time of that existing object file; no object, ref, index or config is written there), one more input to O11; a `write-tree` that failed for another reason than a missing object (a full disk) also costs the one rehash, and if the retry fails the error carries both failures; a turn whose undo was itself undone still shows as undone (only rewind applies it again); files that ignore rules the turn itself wrote (a changed `.gitignore`) leave out are in neither snapshot, so an undo does not remove those the turn made: the chat says so, naming the changed `.gitignore` files, but not the files themselves (review S-6; computing exactly which files only the new rules ignore needs the old rules evaluated against the folder, not done); with `[sandbox] mode = "off"` (and `best-effort` on a kernel without Landlock) the node's git reads the folder unconfined, so the snapshot read window closed in T4 is open there; work inside a nested repository (its uncommitted changes, a repository the agent deleted) is in no checkpoint and undo cannot bring it back, it only says so (and only when one appeared or went away), and the chat shows the note but not the per-checkpoint `excluded` list; a repository made over tracked files keeps being captured as files while the index tracks paths there (git enters it), so later undos write into its work tree as into any folder; new files inside a repository that appeared where nothing was tracked are not recorded; a restore of a file whose name is not UTF-8 is not expected to work (since A.16 the write passes the path's bytes, but the unforced check and the event's `files` still use lossy text; untested; detection itself handles any bytes); two chats' turns in one folder are not serialised (a design question for later); sessions on other nodes, and programs outside the harness, are not seen by the registry. |
| T34 | The next person on a shared browser reads what the last one typed and never sent, or an incognito chat's words outlive it on disk (audit L14) | The reply box keeps its unsent text in `localStorage` under `harness.draft.<user id>.<chat id>` (`web/src/lib/drafts.ts`), so one account never reads another's; a question or form card keeps its answer in `sessionStorage` (`harness.ui.…`, `ui/Input.tsx`). **Sign-out clears them all, whoever's** (`App`: logout, any 401, a load that is not signed in; a socket closed with 4401 makes the shell check `/api/me` and sign out if the login is gone), together with the other traces that are the user's: recent folders, routine test events, Media picked for the next chat. Theme, accent, backdrop, notification and sidebar layout preferences stay (no content). **A chat's draft goes with the chat:** deleted here (`DeleteChat`, after the server's OK) or elsewhere (each chat-list load asks `GET /api/sessions/{id}` for drafts of chats it does not list, since they may be in another workspace, and drops the draft on 404). **An incognito chat has none:** the box keeps no draft until the chat is known not to be incognito, a draft it had is removed, and its cards keep no answer. Every storage read and write is guarded, so a private window, blocked or full storage only costs the draft. Tests: `web/src/lib/drafts.test.ts`, `components/conversation.test.tsx` (drafts), `app.test.tsx` (sign-out), `live.test.ts` (4401), `ui/inputs.test.tsx`. **Residual:** a browser that is closed without signing out keeps the drafts until the next visit there finds no login (then they are cleared) or the same user signs in; a card answer for a deleted chat stays in that tab's `sessionStorage` until the tab closes or the user signs out; the 404 check costs one request per draft of an unlisted chat on each chat-list load. Anyone with the signed-in browser profile reads everything (out of scope, as for any web app). |
| T36 | A steered agent keeps a process running or keeps acting after the person left: a listener, a miner, a chain of unattended turns, output that steers a later turn nobody watches (T.8d steps 1 to 3; step 3, callback turns, reviewed in `docs/security-review/2026-11-T8d-step3-callbacks.md`) | **On by default since step 2** (`background_per_session = 8`; 0 turns it off), because **the person sees and stops them** (review T8D-4): while a command of the open chat runs, the chat's strip shows its id, its command's head as text and its time left (the node's deadline held to at most the command's timeout and at least zero, whatever the browser's clock), with Stop and Stop all, drawn from the log (a reload and a second tab show the same); `POST /api/sessions/{id}/background[/{bg_id}]/stop` is **owner only** (anyone else, admins and workspace admins included, gets the 404 of a chat that does not exist, T10), needs the login key and the page's Origin like every cookie write (Gate 3 P-1, AUDIT L6), and needs no settled password or agentic grant (stopping is always allowed, as cancel); the master runs nothing: an end the log already shows is answered from the log, anything else is relayed as `session.stop_background { by: user }` to the owning node, which alone decides and records `stopped_by_user` (its refusal is passed on as 409; a node that is away is said plainly as 503, its commands ending with it or at their timeout; a node without `CAP_BACKGROUND` is sent nothing, 409). **Background commands only where the shell is contained** (bubblewrap and its guard; refused in a `host` chat, T39, and in a sub-agent's session), each the same contained command as a foreground one (its own PID namespace, file view, network namespace with the network off). **Each states its timeout** (`timeout_secs`, required for `background: true`, refused without before anything starts), at least 5 s, at most `bash_timeout_max_s`. **Counts are node.toml's** (`[limits] background_per_session`, `background_per_node` 64); no argument, event or protocol field carries a bound; a start at a cap is refused naming the chat's own running ids (never another chat's), nothing evicted; a node slot is given back only for a confirmed end (one whose guard still kills it keeps its slot until the guard exits). **Stopped** at its timeout, by `bash_stop` (read-only risk: it only ends this session's own), at the session's deletion (before the log goes), by the master's `session.stop_background` to **every** chat of an owner who lost access, on revocation and at the node's next hello, whatever the mirror shows (no count decides; the chat then refuses starts and stops a late hand-over until its owner prompts it), and at the node's stop (not waited for: stopped, recorded `node_stopping`; an end the log cannot take is retried on each pass and, once the commands are gone, named in the stop's result, which exits 1); the node's death ends them as any command, and the next start records `node_restarted` for each and starts no turn. A turn's cancel does not stop them (the owner's decision). **Output** is scrubbed where it enters (the streaming scrubber), kept in memory only: a running command's ring (head and tail, `background_output_kb`); an ended one's cut to 16 KiB of head and 16 KiB of tail, kept 30 minutes (pruned by time node-wide, every minute) and only for the newest `background_per_session` ended commands of a chat and `background_per_node` of the node: at most `background_per_node × (background_output_kb + 32 KiB)` in all (16.3 MiB at 64 and 256). The end's tail (4 KiB) reaches the model quoted line by line, every line behind `| ` and control characters replaced, under one harness line that says how many lines follow, inside the step's last tool result or in front of the person's next message under a fixed separator (never a user-role message of its own), and is scrubbed again by the loop; it is kept out of the URL provenance (`ChatMsg::Note`); **Callback turns (step 3).** An end that exited or timed out while the chat was idle resumes the agent once: the master's watcher (told of every event the mirror takes; its position is the log itself, so a restarted master asks again and the node refuses a second ask) admits it as a routine fire less `routines` (the owner's account exists, is enabled and owes no password change; agentic grant, permission and node as for the owner's own prompt; not a routine's, an untrusted or a sub-agent's chat (a team chat whose owner left the workspace falls back to their Personal one and is admitted there exactly as the owner could go on in it); not a chat without an owner account; an end at most 60 minutes old, counted from the older of the node's clock and the moment the master first saw it); a node with `callback_turns = 0` announces no `CAP_CALLBACK`, so nothing is asked or shown and sends `session.callback { bg_ids }` with the chat's agent and no text, context or memory (`Master::prompt`'s third caller). **The node decides:** it starts the turn only if every id names an end in its own log that may call back with no turn and no refusal after it, the chat is not `no_callbacks` (set for good at a routine chat's create), its table was not closed by a revocation, the node is not stopping, and fewer than `[limits] callback_turns` (3; 0: off) callback turns ran since the person's last `user_message`; otherwise it refuses (nothing to resume: quietly; on its rules: recorded as `callback_refused`, which the chat shows). A callback turn's cap is `min(max_steps, callback_max_steps)` (20), it gets no sub-agents, and it may start background commands whose ends spend the same budget, which only a person's message refills: **at most `callback_turns × callback_max_steps` steps (60) without the person after each message**. It runs in the session's own mode (clamped by the node): in `ask` its calls park with no timeout. Its opening is one user-role message written by the harness: a fixed first line saying the person has NOT written, then the ends' notes with every output line behind `| `, built from the log alone (no context, no images), the same bytes on every later request; it is `ChatMsg::Opening`, never URL provenance, never the person's words to memory (`file_proposal` files nothing from a turn whose `turn_started` has a cause), search, titles or the budget's refill; the demo provider never acts on program output (only `mock_obeys_output`, tests). Attention: one row per chat with callback turns the owner has not seen, or why it was not resumed (kv `callback-attention:<sid>`, owner's own); the chat list counts the running commands (`background`). A background start's approval card says the command keeps running and for how long, and with the network on that it can accept connections; it does not offer "for this turn", and such an answer allows that one start. An undo is refused while the chat's own background commands run. Tests: `cli/tests/callbacks.rs` and `callbacks_real.rs` (step 3: the node's checks over the protocol, the opening at the provider, the budget, the step cap, `callback_turns = 0`, `ask` on the real binary), `master/tests/callbacks_http.rs` (the watcher, admission, the 60-minute rule, a restart, attention), `master/tests/hooks_http.rs` (routine and untrusted chats), `agent-native/src/callback_tests.rs` (provenance, the cap), `cli/tests/background.rs` (the model's side read from the provider's requests; the person's stop over HTTP on the real binary), `master/tests/link_hardening.rs` (the sweep), `master/tests/background_stop_http.rs` (who may stop, the login key and Origin, an ended or unknown id, a node away or without the capability), `web/src/components/backgroundstrip.test.tsx`, `web/e2e/background.cjs`, `tools/src/background.rs`, `core/src/event.rs`, `agent-native/src/lib.rs`, `node/src/session.rs`. **Residual:** with the network on a background server bound to `0.0.0.0` is reachable from the LAN for its lifetime (the login key, Gate 3 P-1, keeps the master's cookie harmless); there are no cgroups, so CPU and memory are bounded only by count × timeout; with the network off a server cannot be reached by a later command; one user's chats can hold every slot of the node, and a chat refused at the node's cap learns that other chats fill it; an end that waits for a running turn's step boundary is recorded as `node_restarted` if the node dies first (its exit code and output are lost); a delete whose log removal fails has stopped the commands already, and their starts get `node_restarted` at the next start; an undo does not check another chat's background commands in the same folder; the strip's "stopping…" is the tab's own (a second tab sees the end, not the click), and the time left is off by the node's and the browser's clock skew within those bounds. Step 3: in `auto` a steered agent acts 60 steps without anyone after each message the person writes (the owner's Q2); quoted program output is still text the model reads and may obey; a node whose clock is more than 60 minutes behind gets no callback turns (the master cannot tell that from an end replayed late); an owner disabled in the instant between the master's admission and the node's `turn_started` gets that one callback turn (as their own prompt racing the disable would); a person's message that races a callback turn is refused "a turn is already running"; a callback turn is not resumed on the master's side for an ownerless (API-token) chat. Details: `docs/background-commands.md`. |
| T37 | A program of the same user (an agent's shell command, foreground in an `auto` turn or a background command where they are on, with the network on) binds a loopback address of the master's port that the master accepts as its own origin but does not hold. Browsers resolve `localhost` to `::1` first, so with the master on `127.0.0.1:<port>` only, a program on `[::1]:<port>` served `http://localhost:<port>`: the origin whose `localStorage` holds the login key (T2), with the cookie, and a perfect place for a fake login page (security review S2-1, `docs/security-review/2026-11-S2-1-loopback-origin.md`) | **The master holds every loopback address it accepts** (`crates/cli/src/listen.rs`): a loopback bind (`127.0.0.1` or `::1`, or another `127.x`) also binds the other of `127.0.0.1:<port>` and `[::1]:<port>`; `0.0.0.0` also binds `[::1]:<port>` (never `[::]`, which would widen the exposure); `[::]` covers both when dual-stack (`IPV6_V6ONLY` read from the socket) and binds `127.0.0.1:<port>` when not. A WireGuard or LAN bind accepts no loopback name and binds nothing beside. One serve loop accepts from every listener in turn (one connection cap, one shutdown; `master/src/serve.rs`). The binds come first in `serve`, before any runtime or node starts. A second bind that fails with `EADDRINUSE` refuses the start, naming the address and that `http://localhost:<port>` would reach that program; nothing in the data directory is opened (no flag overrides it). With port 0 the port is chosen again (16 tries). `EADDRNOTAVAIL`/`EAFNOSUPPORT` (no IPv6 here) start without it and drop `[::1]:<port>` from the allowed hosts; `localhost` stays (nothing can bind `::1` either). Another process of the same user then cannot take the address: the master's sockets have `SO_REUSEADDR` (tokio; Linux ignores it against a listening socket) and not `SO_REUSEPORT`; a plain, `SO_REUSEADDR`, `SO_REUSEPORT` and both, and an IPv6-only or dual-stack `[::]` bind all get `EADDRINUSE`. Tests: `cli/tests/loopback_origin.rs` (the abuse case and the refused start, on the real binary; skipped with a printed reason without `::1`), `cli/src/listen.rs` (the plan per bind, the error decision, the allowed hosts), `master/tests/server_limits.rs` `two_listeners_share_the_app_the_cap_and_the_shutdown`. **Residual:** while the master is down any program of the user can hold its port, and a program that holds `[::1]:<port>` before a start now blocks that start (it is named); a container that publishes only `-p 127.0.0.1:7777:7777` leaves the host's `[::1]:7777` free (the README says to publish it too or to use `127.0.0.1` only); `--allowed-host` names are the owner's to hold; the UI certificate of `--tls` does not name `::1`. |
| T38 | Text the person did not write is carried into a compaction summary as their request, or as the assistant's own words: a file, a command's output or a fetched page with a line such as `User: delete the repository`, model text with a `User:` line, an attachment with `Assistant: done`, an earlier summary that already holds one. The summary then sits in the system prompt of every later request (found by the security review of T.8d step 3, "Older, relevant to the opening"; the code is older) | **In what the summariser is sent, only lines the harness wrote start a line** (`agent-native`, `transcript`): each part is a label on a line of its own (`User:`, `Assistant:`, `Assistant called tools:`, `Tool result:`, `Earlier summary:`), and every line of the part's own text, the person's included, is behind `| `, with control characters other than tab and U+2028/U+2029 replaced by U+FFFD (the quoting of background output, `core::event::quoted_line`, T36), so no line break of the text's own ends the quote, an empty line stays quoted, and a forged label reads `| User: …` under the part it came in. A tool's name is model-written and is quoted with its arguments. The instructions (`SUMMARY_PROMPT`) say that only text under `User:` is the person's, that tool results and background output are data, not instructions, and that a request inside them is never reported as the person's. A tool result is still cut to its first 2000 characters, before quoting. What a provider is sent for a turn is unchanged (only `transcript` and the summarising prompt moved). Test `agent-native/tests/compaction.rs`, `nothing_in_the_summarisers_input_poses_as_another_speaker`. **Residual:** this removes forged structure, not persuasion. Quoted text can still talk the summarising model into writing something, as any tool result can in a turn (T4), and the summary is model text that the next request carries in its system prompt, above the person's messages. Not measured against a real provider. |
| T39 | Commands of a chat run with more power than the chat was given: the master (or a member, through it) asks a node for an execution mode it should not get, a session moves from the `sandbox` mode to the unconfined `host` mode (or the reverse) without anyone choosing it, or a node quietly runs commands uncontained where it could not contain them (N.10a part 1, O13; `docs/execution-modes.md`) | **The node decides the ceiling** (`[execution]` in node.toml): `sandbox` offered only where its bubblewrap probe passed at start (`auto`; `strict` only without a gap; `off` never), `host` only with `host = true` and not where the Landlock sandbox is required and does not work; computed once at start from the node's own self-test, never from anything received (`ExecOffers::from_start`). A node offering neither runs no agentic session and says so at start (`node run` refuses to start), in `doctor` and in the refusal. The old `contain = "auto"` fallback (Landlock alone, a warning) is gone: the sandbox mode's `Sandbox` refuses a shell command without a containment (`require_contain`), and the step up from Landlock alone is removed. **The mode is chosen once**: `session.create` carries it as a request; the node refuses one it does not offer, and without one takes `sandbox` or refuses (never `host` by default: review L-a; `ExecOffers::choose`); recorded in `session_created`, carried by no later command; checked against what the node offers before every turn, callback, restore and delegation (`checked_folder`), and a session whose mode is gone runs nothing, never moved in either direction; a sub-agent gets its parent's; chat sessions have none. The backend picks the confinement by the recorded mode (`NativeConfig::confinement`): `host` gets the uncontained twin (`Sandbox::uncontained`, which can never gain a containment), and the confinement of a mode the node does not offer refuses every command (`ExecOffers::confinement_for`, review L-f). A `host` session has no background table, so its model is not offered background commands (N-c); it never runs in approval mode `auto`, clamped to `ask` at create and at load whatever was asked or node.toml allows, and its approval card says it runs in the host shell (N-e). The start probes bubblewrap up to 3 times, 1 s apart, each failure logged; after that the start is the only moment that counts (L-b). `strict` does not offer the sandbox mode with any gap its line names: below ABI 9, the network-off gap, the shared-`/proc` form, an image not held, the binary in reach, a `[sandbox] write` entry holding `/tmp` or `/run/user/<uid>` (sockets elsewhere in a write entry are not looked for) or a folder on the node's `PATH` (L-c). The hello announces each gap (`gap:sandbox:netns`, `:sockets`, `:proc`; `gap:host:netns` where Landlock stops TCP, `gap:host:net-open` where nothing does: L-d, L-e) as a snapshot of the start, and the master shows modes and gaps only to accounts with the agentic grant (L-i). **The master, until part 2**: `host` only for a server admin, on every node (review H1: every account owns its Personal workspace and may own a team it made, so ownership was no reason), only when asked for by name; every agentic create carries a mode, `sandbox` unless asked; a connected node is sent a mode only if its hello announced it, and the refusal names what it offers and the remedy on the node; a node not linked now is "not connected" as before, so a routine retries it (L-g, RN-2, RN-3); the recorded mode is mirrored onto the chat's row and the create's rule is applied again on every prompt, approval, restore and callback admission, failing closed on an agentic row without a readable mode, so a demoted admin's `host` chats do no more work and the revocation sweep, which asks once per owner, workspace, node and recorded mode (RN-1), stops a running `host` turn and leaves the same owner's `sandbox` chats alone (L-h; `master/tests/background_stop_http.rs` `a_demotion_stops_the_host_chat_and_only_the_host_chat`); every create that asks for `host` writes one audit line whichever check refused it (`session.create.host`: user or `api token`, workspace and node as resolved, `granted` with the session id or the refusal's first words; RN-4), and the node's `session.create` audit line names the mode asked (N-d); a routine's chat always asks for `sandbox`; the UI shows the warning with the risks before the choice is kept, and the choice is bound to the node it was confirmed for (L-j). Tests: `node/tests/exec_modes.rs` (a test master asking for a mode the node does not offer; recorded and never changed; gone after a restart in both directions; sub-agent; chat), `node/src/exec.rs`, `tools/src/sandbox.rs` `without_bubblewrap_the_sandbox_mode_is_not_offered`, `the_sandbox_modes_shell_is_never_landlock_alone`, `cli/tests/exec_modes.rs` (both modes side by side on one node), `agent-native/tests/host_forwarding.rs`, `master/tests/exec_mode_http.rs`, `master/tests/routines_http.rs` (a routine's chat), `web/src/components/execchoice.test.tsx`, `web/e2e/exec-modes.cjs`. **Residual**: a data directory from before `sessions.exec_mode` does not open (no migration before the first tag; RN-6); the start retries a failing probe, so a slow failure is three times as slow and a hanging bubblewrap still hangs the start (RN-7); a stolen master can ask for any mode up to the ceiling (the modes protect against members, not against the master; T5); `host` is as strong as the node's OS user, and below Landlock ABI 9 the session bus gets a `host` command everything that user has (R2); a `sandbox` chat can write what a `host` chat in the same folder later runs (scripts, `.git/hooks`, `.git/config`), which the modes cannot close (said in the warning and the README; C.8's read-only `.git` binds narrow it); a node serving several workspaces does not refuse `host` yet, because it cannot know (workspaces are the master's; it belongs with the node's attached list, C.12 / N.1); `graph_query` in a `sandbox` chat starts graphify under Landlock alone, outside bubblewrap (review N-a, not changed here), so not every command of a sandbox chat is contained; the hello is a snapshot (a gap closed later by the step up shows until the next hello). |
| T40 | A stuck or steered model repeats one call until the step cap: the same failing command, the same read, the same denied approval asked of the person again and again, spending the person's money and attention (T.1) | The node's loop keeps a per-turn guard (`agent-native/src/repeat.rs`): the same call (`call_key`: tool, folder, canonical arguments) with the same result (a digest of `ok` and the logged output, less the harness's own tokens that differ on every run: the kept output's random `tool_output id=`, a stop's measured time) gets the harness's warning from the 2nd time, is not run the 5th (decided before approval and dispatch, so the person is not asked again and nothing starts), and a refused call asked for again in a later message (after the model was sent the refusal; the same call again in the refusing message is only refused again, T1-D2) ends the turn there: the rest of that message's calls are closed unrun and never offered for approval. A change starts every count again, each call keeping its last result: a new successful result of a call made before in the turn (a poll that moved, so a wait between polls is not refused; a failure in new words restarts only its own count, so a failing runner that prints its time does not un-guard the read or the denied approval between its runs, T1-D1), the first successful run of a tool that changes things, a sub-agent that completed, a background end told at the step boundary; calls in between that change nothing do not break a count. The model is told in `tool_call_finished.note`, a `[harness]` line placed after the result's escaped text (R-2), so tool output cannot forge it; it is its own message kind (`ChatMsg::CallNote`), so a callback turn's opening never takes it; the log rebuilds it byte for byte (the cache prefix does not move); the summariser's transcript carries it as a harness line. Only digests are kept, and the notes hold no argument or output. The state is the turn's alone, fed only what was logged; nothing the model writes is read as a switch. The masked words are `harness_tools` constants that `bash` formats with, so they cannot drift apart. One behaviour in chats and routines; the thresholds are constants, not `node.toml` keys, and there is no switch to turn the guard off (owner, 2026-11-13); the note is for the model only, whether the UI shows it is a later web task. Tests: `agent-native/src/repeat_tests.rs` (the real `bash` with filtering on, a failing runner whose text changes with a denied change between its runs, a failing build that moves after each edit, the same call twice in the refusing message, several calls a step, a completed and a failed sub-agent, a background end, a callback turn after a guard-ended turn), `agent-native/src/repeat.rs`. **Residual:** a model that changes one byte of the call starts a new count; a program whose own output changes every run (a test runner's "finished in 0.03s") is a new result every time, so the same failing test run of most runners is not caught itself; a wait between runs of a failing check whose text changes is warned from its 2nd run and refused at its 5th (the note says to wait inside one command); a successful call whose result is new every run (a background start with a new id) restarts the others; `max_steps` stays the bound. No test compacts mid-way through a callback turn with a call note before its trailing notes (the pop takes only `ChatMsg::Note`, by reading). Review: T1-R1 to R6, T1-D1, D2. |

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
  the same as before: `.gitignore`, `.git/info/exclude`, and the user's global ignore file
  (`core.excludesFile` read by one dedicated `git config --global --get`, the only call
  that sees the global config, else `$XDG_CONFIG_HOME/git/ignore` or
  `~/.config/git/ignore`), copied into the shadow's `info/exclude` at every checkpoint
  (regular files of at most 1 MiB only). Checkpoint
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
* **Approval modes:** `ask_all` (every tool), `ask` (default: mutating tools),
  `auto`. Decisions: `allow`, `deny`, `allow_session` (this exact call, for
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
* Chats stay private to their owner in every workspace, except to the static API token (accepted gap, T29). A chat's workspace is a
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
  summary. A turn larger than the window alone stops summarising there (logged), so nothing
  is dropped unread. With an unknown window it is one piece, as before.
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
