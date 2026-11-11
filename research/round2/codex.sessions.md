# Codex: sessions, remote operation and extensibility (Round 2)

## 1. Repository

- `openai/codex`, branch `main`, pinned SHA `82e6bacb9c6950d3b56cb153847fa80bdd698053`, commit date 2026-11-10T19:52:49Z (`research/round2/00-repos.tsv`).
- Read (code, not docs, except where stated): `codex-rs/{rollout,history,thread-store,state,core/src/session,app-server,app-server-protocol,app-server-transport,app-server-daemon,app-server-client,websocket-auth,exec-server,exec-server-protocol,exec-server-cli,exec,worktree,codex-mcp,rmcp-client,hooks,ext/skills,ext/queue,core-plugins,config,tui(lib.rs),cli(main.rs)}`.
- All paths below are under `codex-rs/` unless stated. Permalinks are shortened in prose by naming the file; every finding carries full permalinks.

## 2. Architecture as it bears on this cluster

- **Language and process model.** Rust workspace of about 100 crates. There is no master/node split. One process (`codex` TUI, `codex exec`, or `codex app-server`) embeds an **app-server** (JSON-RPC) that owns the agent loop for every loaded thread. The TUI and `exec` talk to it **in process** through one client facade; IDEs and SDKs talk to a separate `codex app-server` over stdio, a Unix socket or a WebSocket. Tools run in the same process unless an **environment** (exec-server) is selected, in which case the loop stays central and only process, filesystem and MCP-stdio work is sent to the executor.
  - In-process client facade used by TUI and exec: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-client/src/lib.rs#L1-L22
  - Transport enum (`stdio`, `unix`, `ws://IP:PORT`, `off`): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/mod.rs#L84-L89
- **Persistence.** `codex-rollout` writes JSONL "rollouts"; `codex-thread-store` is the storage-neutral trait (only `LocalThreadStore` and an in-memory store implement it); `codex-state` is a SQLite index plus, in the newer "paginated" history mode, a materialised copy of history.
  - Trait: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/store.rs#L129-L135
  - Module doc ("local rollout files, RPC requests, or any other backing store"): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/lib.rs#L1-L5
- **Protocol.** One Rust macro defines every client request, its wire name, params, response and a per-request serialisation scope; `ts-rs` and `schemars` derive TypeScript and JSON Schema from the same types.
  - https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/common.rs#L215-L241
- **Key decision points.** Rollout filtering `rollout/src/policy.rs`; writer ownership `rollout/src/writer_lock.rs`; resume/fork/revert `thread-store/src/local/*`; history replay `core/src/session/rollout_reconstruction.rs`; transport and auth `app-server-transport`, `websocket-auth`; remote executor `exec-server`; tool exposure `core/src/mcp_tool_exposure.rs`; hook trust `hooks/src/engine/discovery.rs`.

## 3. Findings

### 3.1 Session storage

#### Rollout file: one JSON object per line, items tagged by type, with a per-file ordinal
- What the code does: a rollout is `~/.codex/sessions/YYYY/MM/DD/rollout-<UTC timestamp>-<thread uuid>.jsonl`. Each line is `{timestamp, ordinal?, type, payload}` (the `RolloutItem` enum is flattened into the envelope). Item kinds: `SessionMeta` (first line, with cwd, git commit/branch/url, base instructions, provider, history mode), `ResponseItem` (raw Responses API items), `TurnContext`, `Compacted` (with optional full `replacement_history`), `TokenUsageRecord`, `WorldState`, `EventMsg`, inter-agent messages, and others. The ordinal is a per-file counter assigned by the writer.
- Trace:
  - Directory layout doc: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/list.rs#L427-L428
  - Item enum: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/history/src/lib.rs#L237-L256
  - Line envelope (deliberately no `Deserialize`; readers must use the canonical parser so numbers survive the flatten): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/history/src/lib.rs#L394-L403
  - Writer stamps timestamp, ordinal, and flushes after every line: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/recorder.rs#L2034-L2074
  - Git info is collected into the first line at materialisation: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/recorder.rs#L1948-L1980
- Edge cases and failure handling: the writer repairs a missing final newline before appending (crash-torn tail): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/recorder.rs#L2019-L2032 . A write error drops the file handle, reopens and retries once, keeping unwritten items in memory; only then does it surface an error: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/recorder.rs#L1786-L1830
- Round 1: `confirmed` (rollout-*.jsonl). Round 1 did not describe the item taxonomy, ordinals or the replacement-history field.
- For rizzy-harness: a single ordered log with typed items is the same idea as the "events are the truth" invariant; the ordinal and torn-tail repair are cheap to copy. Do not copy "raw provider items as the durable format": it couples the log to one provider's schema.
- Confidence: traced end to end.

#### File creation is deferred and the log is filtered at write time
- What the code does: a new recorder does not create the file; it keeps `SessionMeta` and queued items in memory (`deferred_creation: true`) until `persist()` is called. Persistence is selective: `persisted_rollout_item` drops streaming deltas, begin/updated events, approval requests, errors, warnings, `ExecCommandEnd`, `McpToolCallBegin`, and so on (explicit "Transient, non-durable" list). In the new paginated history mode, command output and MCP results inside completed items are truncated to 64 KiB for persistence.
- Trace:
  - Deferred state: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/recorder.rs#L1004-L1015
  - `persist()` materialises and retries: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/recorder.rs#L1098-L1122
  - Filter entry point and caps: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/policy.rs#L16-L52
  - Legacy-only versus transient events: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/policy.rs#L136-L165
  - Truncation in paginated mode: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/policy.rs#L231-L287
- Edge cases and failure handling: if materialisation fails the pending items stay in memory and a later `persist()` or `flush()` retries (doc comment at the `persist` definition). I did not trace which caller triggers the first `persist()`.
- Round 1: `new (not documented)`.
- For rizzy-harness: a "no file until the first real turn" rule avoids thousands of empty sessions; a per-item write policy keeps token deltas out of the log. Record the cap on stored tool output as a documented property.
- Confidence: read the definition (trigger of first persist not traced).

#### One writer per thread: a file lock in a shared directory, not a database lease
- What the code does: every live writer takes an exclusive advisory file lock `<CODEX_HOME>/thread-writer-locks/<thread>.lock` through a coordinator that also holds a `.coordination.lock`. A second writer gets `WouldBlock` ("already has an active writer"). Stale lock files are swept by whichever writer first gets the coordination lock exclusively. The coordination wait is bounded to 30 s and its error text mentions a process "suspended on this or another host".
- Trace:
  - Lock dir and coordinator: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/writer_lock.rs#L20-L27
  - `acquire`, `WouldBlock`: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/writer_lock.rs#L45-L95
  - 30 s wait and message: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/writer_lock.rs#L137-L158
  - The writer task, not the caller, owns the guard until queued writes finish: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/recorder.rs#L147-L152
- Edge cases and failure handling: an advisory lock only fences processes that share one filesystem (the error text hints at network homes). Codex ships a WAL-to-DELETE journal migration for NFS because SQLite WAL is unsafe there: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/state/src/nfs_migration.rs#L1-L3
- Round 1: `confirmed` ("each written by one process").
- For rizzy-harness: our "one writer per session" is stronger by construction (the owning node is the only appender). Codex's lock is the fallback you need only if two processes can open the same log; we should not need it, but a generation counter in the event store would be the equivalent for stale nodes.
- Confidence: traced end to end (checked in review)

#### Resume: reverse replay of the log, with compaction as a base and rollback markers
- What the code does: resume loads the rollout items and `ContextManager::reconstruct_rollout` replays them. It picks the newest surviving compaction carrying `replacement_history` as a complete base ("older rollout items do not affect rebuilt history"), then scans the suffix backwards. A `ThreadRolledBack` marker means "drop the newest N user turns", implemented as skipping N user-turn segments while scanning in reverse. Token usage totals are restored from a snapshot stored in the compaction item so resume need not scan arbitrarily far.
- Trace:
  - Entry point: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/session/rollout_reconstruction.rs#L205-L260
  - Rollback as reverse skip: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/session/rollout_reconstruction.rs#L300-L306
  - Compaction snapshot fields for resume: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/history/src/lib.rs#L318-L336
- Edge cases and failure handling: older rollouts go through a compatibility parser that rewrites retired shapes and skips retired record kinds: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/rollout_migration/line_parser.rs#L28-L58
- Round 1: `new (not documented)`.
- For rizzy-harness: the "latest full-history compaction is a base, replay only the suffix" rule makes resume cost independent of session age. Our fold over events needs the same checkpoint record.
- Confidence: read the definition.

#### Resume restores permissions from the log, with an ownership rule
- What the code does: on `thread/resume` the app-server reads the last `ThreadSettingsApplied` event (approval policy, reviewer, permission profile) from history. A running turn can record stale permissions, so it prefers an update whose `thread_id` owner is the resumed thread over legacy ownerless updates.
- Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/request_processors/persisted_resume_settings.rs#L1-L45
- Edge cases and failure handling: explicit overrides in the resume params win (`ThreadResumeParams` carries `approval_policy`, `sandbox`, `approvals_reviewer`): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L395-L404
- Round 1: `confirmed` ("Resume restores saved permissions ... subject to explicit overrides").
- For rizzy-harness: persisting the permission snapshot in the log is right; the node's `node.toml` must still be re-applied as a ceiling on resume (the node decides), which Codex does through managed requirements, not shown here.
- Confidence: read the definition.

#### Three ways to resume, and "rejoin" for a running thread
- What the code does: `thread/resume` accepts a thread id, a rollout path, or an inline `history` (marked "FOR CODEX CLOUD - DO NOT USE"). Precedence for non-running threads is history, then path, then id. If the id names a running thread the app-server rejoins it and treats a path only as a consistency check.
- Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L340-L374
- Edge cases and failure handling: for remote mobile clients the response redacts MCP arguments/results and drops image-generation items, a self-described "temporary bandaid" for payload size: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/request_processors/thread_resume_redaction.rs#L8-L40
- Round 1: `new (not documented)`.
- For rizzy-harness: a resume that attaches to an already-running thread (rather than failing) is what a dashboard needs when two browser tabs open the same chat.
- Confidence: read the definition.

#### Fork: new thread that references the parent's prefix instead of copying it
- What the code does: in the paginated history mode `thread/fork` builds a `HistoryPosition { rollout_id, end_ordinal_exclusive, end_byte_offset }` and stores it as `history_base` in the new thread's `SessionMeta`. The child's rollout therefore starts empty and reads the parent file up to that byte offset ("immutable prefix file"). Preparing a fork first persists the source and holds a lifecycle reservation on it so the parent cannot be reverted or deleted mid-prepare. Forks may cut at `lastTurnId` (inclusive) or `beforeTurnId`, or be `ephemeral` (in memory); an ephemeral fork of a loaded parent can reuse its tools and environments for prompt-cache reuse.
- Trace:
  - Position type: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/protocol.rs#L3463-L3477
  - `SessionMeta` fork fields: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/protocol.rs#L3496-L3510
  - Prepare: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/paginated_fork.rs#L18-L60
  - Params: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L543-L575
- Edge cases and failure handling: lineage readers fall back when an ancestor is missing (`read_with_fallback` in `thread-store/src/local/rollout_lineage.rs`, not traced in depth). Compression (below) must `materialize_rollout_for_reference` before an ancestor can be referenced by byte offset.
- Round 1: `corrected`. Round 1 says fork "copies history up to lastTurnId". For paginated threads it stores a pointer, not a copy; legacy-mode threads are the copy case (not traced).
- For rizzy-harness: a fork as "parent id plus event seq cut" in SQLite is the same idea and cheaper than file offsets. Pin the parent against deletion while a fork exists (Codex's reservation exists only during prepare, so a deleted parent relies on the fallback path).
- Confidence: read the definition.

#### Revert: a new immutable rollout file under the same thread id, swapped by compare-and-set
- What the code does: `thread/revert` (before a turn id) does not rewrite the log. It creates a replacement rollout named `rollout-<ts>-<thread>_<rollout_id>.jsonl` whose `history_base` points at the retained prefix of the old file, then switches the SQLite row's rollout path with `replace_rollout_path_if_current(thread, expected_old_path, new_path)`. If the compare fails (someone else changed it) the new file is deleted and a `Conflict` is returned. The thread id stays stable; the rollout id changes. It first closes the live writer and takes the writer lock.
- Trace:
  - Doc and lock order: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/revert_thread.rs#L15-L45
  - Compare-and-set and cleanup: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/revert_thread.rs#L138-L156
  - Filename grammar: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/rollout_file_name.rs#L12-L38
  - API docs: "does not revert local file changes": https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L1316-L1326
- Edge cases and failure handling: only paginated threads can be reverted (`InvalidRequest` otherwise) and a live recorder must be absent (`ensure_live_recorder_absent`).
- Round 1: `confirmed` (history only, files untouched). Detail new: append-only files plus a pointer swap.
- For rizzy-harness: with an append-only event table, "revert" is a `ThreadReverted{before_turn}` event that the fold honours; no new file is needed. Keep the "does not touch the working tree" wording in the UI.
- Confidence: traced end to end (checked in review)

#### Snapshot validation before reusing cached history
- What the code does: a cached "model context" snapshot carries an opaque revision made from the canonical path, length, mtime, ctime, birth time, device and inode. The store captures it before and after reading; any change disables reuse, and `resume_thread` re-validates it after acquiring the writer lock.
- Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/history_revision.rs#L1-L28 ; trait contract: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/store.rs#L185-L200
- Edge cases and failure handling: "missing or unrecognized revisions must not be treated as proof that stored history is unchanged" (trait doc).
- Round 1: `new (not documented)`.
- For rizzy-harness: use the max `seq` of a session as the revision; a node that cached a fold must compare it under the writer's ownership.
- Confidence: read the definition.

#### Retention, compression and SQLite index
- What the code does: there is no age-based deletion in `rollout/` (searched `maintenance.rs`, `compression.rs`). A background worker compresses rollouts older than 7 days to zstd level 3 (`.zst`), two at a time, guarded by a run marker and a maintenance lock; readers accept plain or compressed files transparently. Active threads live in `sessions/`, archived ones in `archived_sessions/`. Session metadata (name, section, pin, project, goals, queue, history mode) is in SQLite with 62 numbered migrations; `history_mode` is a column.
- Trace:
  - Worker and constants: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/compression.rs#L368-L378
  - Age gate: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/compression.rs#L1006-L1012
  - Maintenance lock: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/maintenance.rs#L1-L20
  - History-mode column: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/state/migrations/0040_threads_history_mode.sql#L1-L1
- Edge cases and failure handling: compression is "best-effort", fire-and-forget, never blocks startup (doc at `compression.rs:46-51`, cited above at 368-378 for constants).
- Round 1: `confirmed` for index and WAL; compression and archive dirs are `new (not documented)`.
- For rizzy-harness: compress cold sessions rather than delete; keep a deletion path that is explicit (Codex `delete` needs a UUID).
- Confidence: read the definition.

### 3.2 Checkpoints and undo

#### There is no file undo or checkpoint; the old mechanism was removed and is only stripped on read
- What the code does: nothing snapshots or restores the working tree. The only code mentioning the former mechanism is the legacy parser, which skips `ghost_snapshot` response items and `undo_completed` events from old rollouts and the recorder strips `ghost_snapshot` entries (including inside `replacement_history`). `thread/revert` and the `ThreadRolledBack` marker change history only. `TurnDiffTracker` records the files touched by `apply_patch` per turn for diff display (100 ms diff budget); it has no restore path.
- Trace:
  - Retired record kinds: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/rollout_migration/line_parser.rs#L47-L58
  - Ghost snapshot stripping on read: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/recorder.rs#L1291-L1352
  - Diff tracker (apply_patch changes only): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/turn_diff_tracker.rs#L1-L22
- Edge cases and failure handling: searched `git stash`, `write-tree`, `commit-tree` in all `.rs` files: no hit outside tests. The cost guard for large repositories therefore does not exist (the removed snapshot feature is the one that needed one).
- Round 1: `confirmed` ("Nothing documented for automatic file checkpoints"). Code adds that a snapshot feature existed and was deliberately removed.
- For rizzy-harness: Codex's answer is "use Git". That is the weak spot of the whole field (see the competition): a node-side shadow-ref snapshot per turn, with a size guard, would be a real differentiator.
- Confidence: traced end to end (negative result by search) (absence confirmed in review: no restore path in `core`, `thread-store` or the app-server method table; `git-utils/src/baseline.rs` does build a git baseline repository, but it is used only by the memories workspace for diffing, and `apply_git_patch` with `revert` serves cloud-task apply)

### 3.3 Client and server split: the app-server protocol

#### One macro defines methods; schemas are generated from the same types, with experimental filtering
- What the code does: `client_request_definitions!` lists each method (`thread/start`, `thread/resume`, `thread/fork`, `thread/revert`, `turn/start`, `turn/steer`, `turn/interrupt`, `command/exec`, `process/spawn`, `fs/*`, `skills/list`, `plugin/*`, `mcpServer/*`, `config/*`, `remoteControl/*`, `environment/*` and about 170 more). Types derive `ts-rs::TS` and `schemars::JsonSchema`. `generate-ts` and `generate-json-schema` export from the types; `#[experimental(...)]` methods and fields are stripped from output unless experimental is requested. A checked-in `schema/` tree is regenerated and compared by tests. At runtime a request whose method has an experimental reason is rejected unless the connection declared `experimentalApi` at `initialize`.
- Trace:
  - Method table start: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/common.rs#L522-L530
  - Export: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/export.rs#L125-L150
  - Fixtures from env: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/schema_fixtures.rs#L32-L50
  - Runtime gate: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/message_processor.rs#L1040-L1052
  - CLI entry points `GenerateTs`, `GenerateJsonSchema`: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/cli/src/main.rs#L650-L657
- Edge cases and failure handling: `initialize` may be sent once (`Already initialized`), and any other request before it gets `Not initialized`.
- Round 1: `confirmed`.
- For rizzy-harness: generating the TypeScript types for `web/` from the Rust protocol types (and failing CI on drift) removes a whole class of UI bugs; copy it for the master HTTP/WS API.
- Confidence: traced end to end.
  - `Already initialized` check: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/request_processors/initialize_processor.rs#L138-L143

#### Per-request serialisation scopes: ordering is declared in the method table
- What the code does: each method declares how it is queued: `thread_id(params.thread_id)`, `global("key")`, `global_shared_read`, per process, per fs-watch, per MCP-OAuth server. The dispatcher turns the scope into a queue key and an access kind (exclusive or shared read), so two `thread/revert` and `turn/start` calls for one thread cannot interleave while unrelated threads run concurrently.
- Trace:
  - Scope enum: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/common.rs#L129-L139
  - Example (`thread/revert`): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/common.rs#L809-L813
  - Queue keys: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/request_serialization.rs#L25-L60
- Edge cases and failure handling: a connection gate closes when its auth is no longer current, rejecting queued handlers while started ones finish: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/connection_rpc_gate.rs#L11-L50
- Round 1: `new (not documented)`.
- For rizzy-harness: copy the idea. Put the ordering key in the API definition, not in each handler.
- Confidence: read the definition.

#### Overload: `-32001` is returned only for requests, when the inbound queue (128) is full
- What the code does: each connection feeds a bounded `mpsc` of `CHANNEL_CAPACITY = 128`. `enqueue_incoming_message` does `try_send`; when full and the message is a *request* it replies `{code:-32001, "Server overloaded; retry later."}` through the writer's own `try_send`; if that outbound queue is also full the overload reply is dropped with a warning. Non-request messages (responses to server requests, notifications) wait with an awaited `send` and apply backpressure instead. The WebSocket outbound queue is 32,768 entries. The in-process client facade uses an unbounded local event queue "so unread notifications cannot prevent request responses".
- Trace:
  - https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/mod.rs#L231-L262
  - Constants: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/mod.rs#L24-L24 and https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L55-L56
  - Unbounded consumer queue: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-client/src/lib.rs#L386-L392
- Edge cases and failure handling: the overload reply carries no `retry_after`, and one slow client can still stall its own responses once the 32k outbound queue fills.
- Round 1: `corrected`. Round 1 says "bounded queues answer -32001". Only request admission does; everything else blocks.
- For rizzy-harness: copy the shape (reject requests, never drop events) and add a retry hint.
- Confidence: traced end to end.

#### Transports, and where authentication happens
- What the code does: four transports. (a) stdio JSONL (default). (b) `unix://` : a private control socket, WebSocket framing over a Unix socket. (c) `ws://IP:PORT` : an axum listener. (d) `remote control`: the app-server dials out (below). WebSocket auth modes are `capability-token` (a token file or its SHA-256, compared in constant time) and `signed-bearer-token` (HS256 JWT with a shared secret of at least 32 bytes, optional issuer and audience, 30 s default skew). Auth is checked once, at the HTTP upgrade, before the socket exists, hence before `initialize`. Credentials are loaded once at startup. Any request carrying an `Origin` header (including health checks) gets 403, which blocks browser pages from connecting.
- Trace:
  - Upgrade handler calls `authorize_upgrade`: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L115-L130
  - Origin rejection middleware: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L99-L113
  - Constant-time token check and JWT: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L286-L330
  - "Loads credentials once; changes to secret files do not update": https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L236-L237
- Edge cases and failure handling: "This does not enforce expiry or revocation after a WebSocket has been established" (doc at `websocket-auth/src/lib.rs:286-287`): a JWT that expires mid-connection stays connected.
- Round 1: `confirmed` (modes, pre-`initialize`, Origin 403).
- For rizzy-harness: reject browser `Origin` on the node link if it is ever HTTP-reachable; re-check expiry on long-lived sockets, which Codex does not.
- Confidence: traced end to end (checked in review)

#### Non-loopback WebSocket without auth is refused at startup
- What the code does: `start_websocket_acceptor` returns `InvalidInput` ("refusing to start non-loopback websocket listener ... without auth") when the bind address is not loopback and the auth policy is empty. No flag or environment variable bypasses this for the app-server: the only caller passes `policy_from_settings(&auth)`; the former `--allow-unauthenticated-non-loopback-ws` flag was removed (CLI test `app_server_rejects_removed_insecure_non_loopback_flag`, `cli/src/main.rs` L4932-L4940). Review addition: this refusal belongs to the app-server transport only. The separate exec-server WebSocket listener (`exec-server/src/server/transport.rs`, `run_websocket_listener`, bound from `--listen ws://IP:PORT`) never calls `is_unauthenticated_non_loopback_listener`; with no auth flags `authorize_upgrade` returns Ok, and the module comment says "unauthenticated listeners remain supported for compatibility". That listener exposes process and file RPCs, so an unauthenticated non-loopback exec-server is possible. The startup banner prints "websocket auth is required for non-localhost listeners".
- Trace:
  - https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L160-L175
  - Predicate: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L279-L284
  - Only caller: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/lib.rs#L866-L876
- Edge cases and failure handling: the Unix control socket has no token at all; its protection is filesystem permissions (next finding). Loopback `ws://` with no auth is allowed, so any local process can drive the agent.
- Round 1: `refuted`. Round 1 says non-loopback listeners are unauthenticated by default "during rollout"; the code refuses to start.
- For rizzy-harness: this is the right default (fail closed on a wider bind), and it already matches the "loopback by default, wider bind is explicit" invariant. Note that unauthenticated loopback is still a local privilege boundary problem on shared machines.
- Confidence: traced end to end (checked in review) (corrected in review: true for the app-server listener only; the exec-server `--listen ws://IP:PORT` listener has no such refusal and its source comment says unauthenticated listeners remain supported for compatibility)

#### Background daemon: PID-file process, private socket directory, recovery snapshot
- What the code does: `codex app-server daemon start|restart|stop|version|bootstrap` manages one detached `app-server` process (the only `BackendKind` is `Pid`; it uses `setsid`), serialised by `daemon.lock`, with a pid file and a version-checked managed install. The control socket lives under a hashed path in a shared directory that must be mode 0700 and owned by the current user (root allowed for ownership checks on the parent); the socket itself is mode 0600; the advertised `app-server-control.sock` is a symlink to it. A client accepts the socket only if the symlink target equals the expected protected path and the directory is 0700 and owned by the euid, so an SSH-forwarded or attacker-bound socket at the advertised path is not trusted.
  - On a planned restart the daemon writes `loaded-threads.json` (loaded thread ids plus interrupted turn metadata) with `INTERRUPTION_PREFIX`; the new generation consumes and deletes it before serving, then restores runtimes in the background. Only local-environment turns can auto-continue.
- Trace:
  - Lifecycle commands: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-daemon/src/lib.rs#L74-L87
  - Backend: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-daemon/src/backend/mod.rs#L15-L19
  - Bind checks and symlink: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L71-L145
  - Client-side verification: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L322-L352
  - Recovery snapshot: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/daemon_thread_recovery.rs#L9-L50 and https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/daemon_recovery.rs#L12-L32
- Edge cases and failure handling: the snapshot is written on a plain thread "so a forced exit must not wait for file I/O in Tokio's blocking pool"; a malformed snapshot is still deleted (it belongs to one generation).
- Round 1: `confirmed` (managed background server, host-local Unix socket); the recovery of interrupted turns is `new (not documented)`.
- For rizzy-harness: the "verify the socket is ours before talking to it" logic and the interrupted-turn recovery snapshot are both worth copying for the node daemon.
- Confidence: traced end to end.

#### Remote TUI: `--remote ws[s]://host:port` or `unix://`, bearer token only via env var and only over a safe URL
- What the code does: `resolve_remote_addr` accepts `unix://[PATH]` or `ws(s)://host:port` with an explicit port and path `/` and no query. `--remote-auth-token-env NAME` reads the token from the named environment variable and is rejected unless the URL is `wss://` or loopback `ws://`; Unix endpoints cannot carry a token. The TUI connects with `required_human_approval: true`, `exec` with `false`.
- Trace:
  - https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/tui/src/lib.rs#L478-L524
  - Token rule: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/cli/src/main.rs#L2649-L2676
  - Capability flag: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/v1.rs#L43-L52
- Edge cases and failure handling: none beyond the URL validation.
- Round 1: `confirmed`.
- For rizzy-harness: never accept a token on a URL or a flag value; env or file only, and only over TLS or loopback.
- Confidence: read the definition (checked in review)

#### "Human approval" capability: some approvals can only be answered by a connection that opted in
- What the code does: `initialize` can declare `requiredHumanApproval` (needs `experimentalApi`). A `CommandExecutionApprovalKind::HumanCommand` request ("a managed checkpoint requiring a fresh human decision", only accept/decline/cancel valid) is routed only to connections that declared it; if none qualifies the request is not delivered to others. `exec` declares false, so it never sees these prompts.
- Trace:
  - Declaration: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/request_processors/initialize_processor.rs#L144-L148
  - Routing filter: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/outgoing_message.rs#L414-L430
  - Kind: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/v2/item.rs#L1693-L1705
- Edge cases and failure handling: I did not trace what happens when no eligible connection exists (the remaining lines of `outgoing_message.rs` after 430).
- Round 1: `new (not documented)`.
- For rizzy-harness: an approval class that an SDK or headless client structurally cannot auto-answer is a good primitive for "a person must decide" gates.
- Confidence: read the definition.

#### Remote control: the app-server dials a vendor relay; pairing and enrolment are server-side
- What the code does: `codex remote-control` / `remoteControl/*` makes the local app-server enrol with a ChatGPT-hosted service (`.../wham/remote/control/server/enroll`), receive a `remote_control_token` with `expires_at`, and keep an outbound WebSocket (protocol version "3"). Mobile clients reach the host through that relay. Messages are wrapped in envelopes with `client_id`, `stream_id` and `seq_id`, split into segments of about 100 KiB (max 150 KiB, reassembled max 100 MiB, 1,024 segments), and the socket resumes with an `x-codex-subscribe-cursor` header. Idle clients are dropped after 10 minutes. The configured relay URL must be a `chatgpt.com` / `chatgpt-staging.com` host or localhost.
- Trace:
  - Enrol request/response types: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/protocol.rs#L21-L56
  - Host allow-list: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/protocol.rs#L209-L226
  - Server events and envelope: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/protocol.rs#L154-L206
  - Segment limits: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/segment.rs#L19-L23
  - Protocol version and cursor header: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/websocket.rs#L70-L73
  - Idle timeout: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/remote_control/client_tracker.rs#L27-L27
- Edge cases and failure handling: pairing codes carry `expires_at` returned by the service; expiry enforcement is server-side and not in this repository, so Round 1's open question (how pairing codes expire) cannot be settled from code.
- Round 1: `confirmed` (remote control exists); the transport mechanics are `new (not documented)`.
- For rizzy-harness: this is our master/node link in miniature, but with a vendor as the master. Outbound-only, enrolment token with expiry, cursor-based resume and segmentation of big messages are all things the node link needs; the allow-listed relay host is a good guard against a poisoned config redirecting the token.
- Confidence: read the definition (transport internals not traced end to end).

### 3.4 Remote workers and executors

#### What runs where: the exec-server is a tool host, the agent loop stays on the harness side
- What the code does: `codex exec-server` is a JSON-RPC service exposing `process/start|read|write|signal|terminate`, `fs/*` (read, write, walk, copy, watch-missing, canonicalize), `environment/info|status`, `http/request` and `capabilityRoots/discoverV1` (find skills and plugin manifests on the executor). The harness (the process that talks to the model) holds an `Environment` per id (`local` plus any from `environments.toml` or `environment/add`) and routes shell, `apply_patch`, file reads, skill discovery and MCP stdio launches through it. The executor does not call the model.
- Trace:
  - Method list: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-protocol/src/protocol.rs#L25-L65
  - Environment config file: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/environment_toml.rs#L26-L50
  - Skills on the executor's filesystem (repo skills only for the local environment): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/skills/src/host_service.rs#L76-L87
  - MCP stdio through the executor: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/executor_process_transport.rs#L1-L20
- Edge cases and failure handling: this is the opposite split from rizzy-harness (our node runs the loop). It matches Round 1's "inverted" row, not its "matches ours" row for exec-server, although Codex's *remote TUI* is the "matches ours" case.
- Round 1: `new (not documented)` (Round 1 only mentions `environment_id`).
- For rizzy-harness: not a model to copy for the loop placement. Useful parts: a narrow tool-host RPC, per-connection process handles, `environment/info` carrying executor version for compatibility decisions.
- Confidence: read the definition.

#### The executor builds the sandbox; the harness sends an intent
- What the code does: `process/start` carries `sandbox: Option<FileSystemSandboxContext>` described as "Portable sandbox intent. Concrete wrapper argv is resolved by the exec-server", plus `enforce_managed_network` and optional proxy launch config. The executor selects the sandbox type from the profile and its own platform and builds the wrapper; if managed networking is required but details are absent it must fail closed. Credentials in the environment are masked before spawn (failure mode `Warn`).
- Trace:
  - Params: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-protocol/src/protocol.rs#L374-L414
  - Selection: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/sandbox_selection.rs#L21-L46
  - Masking mode: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L43-L45
- Edge cases and failure handling: the *policy* (which paths are writable) arrives from the harness in the request; the executor only translates it. Nothing in this path lets the executor-side owner cap what the harness asks for. That is the opposite of our invariant "the node decides". I did not find an executor-local allow-list (searched `exec-server/src/server/*`).
- Round 1: `new (not documented)`.
- For rizzy-harness: keep `node.toml` as a ceiling that intersects with whatever the master sends; Codex's design trusts the harness.
- Confidence: read the definition (executor-side refusal not found) (checked in review)

#### Executor dials out, authenticates with ChatGPT/agent/AWS credentials, and pairs through a registry
- What the code does: `codex exec-server --remote <registry URL> --environment-id ID` registers the executor with a hosted registry and keeps an outbound WebSocket to a relay (default transport `noise`; `direct` uses a direct registration). Registry auth sources: ChatGPT token from stdin, agent-identity token, AWS SigV4, a one-shot registration bearer from stdin, or "rotating executor credentials" read from stdin, where the process exits when the pipe closes. `exit-on-stdin-close` ties the executor lifetime to its parent. Alternatively `--listen ws://IP:PORT` serves directly with the same WebSocket auth modes as the app-server, but auth is optional there and there is no refusal of an unauthenticated non-loopback bind (and the auth flags are rejected together with `--remote`). A client can also reach an executor by `url` + `auth_bearer_token` or by launching a `program` (for example `ssh host codex exec-server --listen stdio`) from `environments.toml`.
- Trace:
  - CLI surface: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-cli/src/command.rs#L56-L160
  - Auth-flag conflict: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-cli/src/command.rs#L195-L195
  - Registry auth modes: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/remote.rs#L72-L88
  - `environments.toml` rules (`auth_bearer_token` requires `url`): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/environment_toml.rs#L120-L145
- Edge cases and failure handling: the scoped executor deliberately does not initialise telemetry exporters because "configured exporters may contain account-wide credentials" (comment in `command.rs`; the exclusion is https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-cli/src/command.rs#L253-L256). The relay and registry are hosted services; their server side is not in this repository.
- Round 1: `new (not documented)` (Round 1 asked what `--remote` is and how it authenticates).
- For rizzy-harness: credentials over a stdin pipe whose closure kills the process is a clean way to pass a per-node, rotating secret without files or argv; use it for node pairing.
- Confidence: read the definition (checked in review) (corrected in review: the direct `--listen` listener takes the same auth settings but they are optional there, see the 3.3 non-loopback finding)

#### Harness-to-executor channel: hybrid post-quantum Noise IK with registry authorisation, 128 streams, ack/resume framing
- What the code does: the harness opens a Noise `hybridIK` handshake (X25519 + ML-KEM-768, AES-GCM, SHA-256) to the executor's static key, which it **pins from the registry response**. The first message authenticates the harness static key to the executor; the executor then asks the registry whether that key is authorised (10 s timeout, up to 32 pending validations) before completing the handshake and opening a JSON-RPC stream. A denied key gets an authenticated rejection only for a registry 401, otherwise a reset. Up to 128 streams multiplex over one relay WebSocket (frames: protobuf `RelayMessageFrame` with stream id, `ack`, `ack_bits`, `Data{seq, segment}`, `Resume{next_seq}`, `Reset`, `Heartbeat`, `Handshake`). After 8 failed handshakes the executor cools down for 10 s. Ciphertext records are reordered in a window of 64 sequence numbers and 1 MiB, duplicates dropped; the sequence number never wraps.
- Trace:
  - Design comment and suite: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/noise_channel.rs#L1-L37
  - Completion only after registry authorisation: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/relay.rs#L640-L690
  - Limits: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/relay.rs#L56-L61
  - Wire format: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/proto/codex.exec_server.relay.v1.proto#L5-L43
  - Reordering: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/noise_relay/ordered_ciphertext.rs#L1-L40
- Edge cases and failure handling: the relay is untrusted by design: it sees only ciphertext; frames are capped at 256 KiB before parsing (`noise_relay/mod.rs:14-20`). Handshake diagnostics are rate-limited to 100 per minute.
- Round 1: `new (not documented)`.
- For rizzy-harness: for an outbound node link through a relay, end-to-end encryption with pinned node key and a master-side authorisation check is the pattern; but our direct TLS link with pinned keys already gives most of it without a relay.
- Confidence: traced end to end (executor side).

#### Reconnect and fencing: session id, single attachment, 80 s retention
- What the code does: `initialize` can carry `resumeSessionId`. The executor keeps a `SessionEntry` per session (with its process table) and attaches at most one connection to it. Attaching a session that already has an active connection returns `SESSION_ALREADY_ATTACHED`; attaching after the 80 s `RECOVERY_RETENTION` returns "unknown session id" and shuts its processes down. On disconnect a session becomes `Detached{expires_at}` and its processes survive until then. The client side retries reconnecting for 50 s, reading missed process output with `process/read` from a sequence cursor.
- Trace:
  - Registry and retention: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/server/session_registry.rs#L15-L130
  - Initialize params: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-protocol/src/protocol.rs#L90-L106
  - Client recovery window: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/client_recovery.rs#L57-L62
- Edge cases and failure handling: this fences a *second client*, not a stale one: a stale harness that still holds the connection blocks the new attach until the executor notices the disconnect (the 80 s budget is sized for 25 s of detection skew, per the comment). There is no epoch or fencing token on commands.
- Round 1: `new (not documented)`.
- For rizzy-harness: our master should hand the node a monotonically increasing session epoch and reject commands from older epochs; Codex's "one attachment at a time plus retention window" leaves a gap of up to the detection time.
- Confidence: traced end to end (checked in review)

### 3.5 Moving a chat with its Git state ("Handoff")

#### Not in the open-source tree; the nearest pieces are worktree creation and a per-thread owner marker
- What the code does: searched all of `codex-rs`, `sdk/`, `codex-cli/`, `docs/` for "handoff": hits are unrelated (analytics, an OAuth token handoff, a rollout migration step, an SDK enum). No code packages Git state, stashes or diffs a dirty tree, or moves a chat between Local and Worktree. What exists: `WorktreeManager::create` runs `git worktree add --detach --no-checkout`, then `git reset --hard <head sha>` in the new tree (so it checks out committed HEAD only; no uncommitted changes are copied here). `bind_thread` writes `codex-thread.json` (`{version, ownerThreadId}`) in the worktree's Git metadata with a no-clobber atomic rename and fails if another thread already owns the checkout; `remove` refuses ignored local files.
- Trace:
  - Create: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/worktree/src/lib.rs#L62-L140
  - Owner marker: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/worktree/src/metadata.rs#L1-L90
  - Bind/owner API: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/worktree/src/lib.rs#L274-L284
- Edge cases and failure handling: the marker is the only "stale side" guard that exists, and it only prevents two threads from sharing one checkout. "Desktop-compatible" in the crate docs says the closed Desktop app defines the full contract.
- Round 1: `not in code`. Round 1 said a worktree "starts from HEAD plus any uncommitted changes" and that Handoff moves chat and code; the OSS crate does the first half only for committed HEAD, and Handoff is closed-source (Desktop).
- For rizzy-harness: design this ourselves. A safe protocol is: the source node commits a hidden ref (or `git stash create`) as the transfer artefact, the master records `session.owner = target` with an epoch bump *before* the target starts, and the source refuses further appends once it sees a higher epoch. Codex gives us nothing to copy for the protocol itself; copy only the "owner id inside `.git`" marker.
- Confidence: traced end to end (negative result by search) (absence confirmed in review: ripgrep for "handoff" over the clone finds only unrelated hits)

### 3.6 Headless and scheduled operation

#### `codex exec` is the app-server in process, with approvals forced off
- What the code does: `exec` builds an `InProcessAppServerClient` (the same engine as the TUI) and starts or resumes a thread. Defaults: `approval_policy = Never`; it exits with an error if not inside a Git repository unless `--skip-git-repo-check` or `--yolo`; `--ephemeral` starts the thread without persistence; `--output-schema FILE` constrains the final answer; `--output-last-message FILE` is opened and bound before the agent starts so sandboxed activity cannot redirect the final write; `--json` prints a JSONL event stream (`thread.started`, `turn.started`, `turn.completed`, `turn.failed`, `item.started|updated|completed`, `error`). Server-initiated requests are handled automatically: command and file-change approvals and `request_user_input` and dynamic tool calls are rejected with an error, MCP elicitations are cancelled. `--ignore-user-config` and `--ignore-rules` skip `config.toml` and execpolicy rules; `--trust` saves project trust (and does not override an explicit untrusted).
- Trace:
  - Default policy: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec/src/lib.rs#L622-L640
  - Git check and bound output: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec/src/lib.rs#L1074-L1100
  - Auto-reject of server requests: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec/src/lib.rs#L2228-L2300
  - Flags: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec/src/cli.rs#L36-L60
  - Event types: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec/src/exec_events.rs#L11-L37
  - History mode for non-ephemeral threads: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec/src/lib.rs#L1520-L1545
- Edge cases and failure handling: an `AutoReview` reviewer configuration makes `exec` rebuild the config without the headless "never" default (`build_exec_config`, `exec/src/lib.rs:844-878`), so a model-based approver can run headless. Resume works via `codex exec resume <id>|--last`.
- Round 1: `confirmed` (Git check, `--ephemeral`, resume).
- For rizzy-harness: copy "headless = the same engine, a client that cannot answer approvals, so any approval fails the tool call". It removes a second code path for non-interactive runs.
- Confidence: traced end to end (checked in review)

#### Scheduling does not exist in the engine; a durable queue with a 10 s poll does
- What the code does: searched `codex-rs` for cron, schedules and automations: none in the engine. `codex queue --thread` and `thread/queue/*` store user messages (max 100 per thread) in SQLite. A watcher polls SQLite's `data_version` every 10 s, reads a revision index, and dispatches queued messages to threads when idle, so another process can enqueue work for a thread loaded in a daemon.
- Trace:
  - Watcher: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/queue/src/service.rs#L90-L105
  - Install: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/queue/src/lib.rs#L1-L21
  - Limit: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/state/src/lib.rs#L122-L122
- Edge cases and failure handling: independent dispatch tasks so a blocked thread cannot starve others (comment in the watcher).
- Round 1: `confirmed` (queue exists); no scheduler is documented and none is in code.
- For rizzy-harness: our routines are ahead here; the idle-dispatch queue per session is worth copying for "message arrives while the node is busy".
- Confidence: read the definition (checked in review) (corrected in review: no scheduler in the engine, but a desktop-app feature flag `in_app_local_automation` and an "automation" thread source exist, so scheduling lives in the closed desktop app, not nowhere)

### 3.7 MCP as a client

#### Transports, timeouts and size limits
- What the code does: two configured transports: `Stdio {command,args,env,env_vars,cwd}` and `StreamableHttp {url, bearer_token_env_var, http_headers, env_http_headers, http_headers_helper}`. The helper is a local shell command that prints JSON headers (doc warns it is visible to process inspection). Stdio lines are capped at 8 MiB (stderr lines at 1 MiB). Defaults: 30 s startup, 300 s per tool call. A stdio server can be launched on the executor chosen by `environment_id`, in which case bytes flow through the exec-server `process/write` and pushed output events.
- Trace:
  - Config enum: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/mcp_types.rs#L658-L690
  - Timeouts: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/codex-mcp/src/rmcp_client.rs#L121-L122
  - Local stdio cap: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/bounded_stdio_transport.rs#L24-L26
  - Executor stdio: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/executor_process_transport.rs#L46-L56
- Edge cases and failure handling: the client library is `rmcp` (the official Rust MCP SDK); transport wrappers exist only to bound input and to observe notifications.
- Round 1: `confirmed`.
- For rizzy-harness: cap per-line size on stdio MCP and put a deadline on both startup and calls; run stdio MCP servers on the node, never the master.
- Confidence: read the definition.

#### OAuth: loopback callback with state, CIMD or DCR registration, keyring with file fallback
- What the code does: an interactive login starts a `tiny_http` server on `127.0.0.1` (random or configured callback port), parses `code` and `state` from the redirect, with a 300 s default timeout. Client registration is `Auto` (prefer a ChatGPT-hosted native Client ID Metadata Document, else Dynamic Client Registration), or forced `Cimd` or `Dcr`. Tokens are stored in the OS keyring; if it is unavailable, `CODEX_HOME/.credentials.json`. Mode `Auto` pins the store chosen at first load for the client's lifetime "so refreshes cannot switch to a possibly stale credential source". The `File` mode doc says the file is readable by any process of the same user.
- Trace:
  - Store comment: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/oauth.rs#L1-L17
  - Store modes: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/types.rs#L150-L166
  - Registration: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/oauth_client_registration.rs#L17-L30
  - Callback server and timeout: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/perform_oauth_login.rs#L379-L410 and https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/perform_oauth_login.rs#L655-L665
- Edge cases and failure handling: the callback listener is on the machine running Codex; for a server-side node a browser on another machine cannot reach it (`--no-browser` prints the URL). I did not trace the PKCE parameters (delegated to `rmcp`).
- Round 1: `confirmed` (CIMD, DCR, keyring).
- For rizzy-harness: an MCP OAuth flow must run where the browser is, not where the node is: the master (or the browser itself) should host the callback and ship the token to the node over the link.
- Confidence: read the definition.

#### Keeping tool lists out of the context: per-model deferral behind a BM25 `tool_search`
- What the code does: when building the tool registry, `append_mcp_tools` registers every MCP tool with `ToolExposure::Deferred` if the *model* has `supports_search_tool`, else `Direct`. Deferred tools are omitted from the initial tool list and found through a `tool_search` tool: the handler builds a BM25 index (`bm25` crate, English) over each tool's search text, returns up to 8 results by default (`TOOL_SEARCH_DEFAULT_LIMIT`) as loadable specs, and rejects empty queries. Per-server `omit_tools_from = [direct|deferred|code_mode]` removes a server from surfaces. Tools from plugin-provided "agent plugin" servers are `Hidden` if one spec exceeds 8,000 bytes or the total exceeds 64,000 bytes. Every ordinary MCP input schema is compacted when it exceeds 5,000 bytes (per-server `tool_input_schema_max_bytes` overrides).
- Trace:
  - Exposure decision: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/mcp_tool_exposure.rs#L136-L170
  - Agent-plugin budgets: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/mcp_tool_exposure.rs#L212-L236
  - Model capability input: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/spec_plan.rs#L150-L160
  - Surface overrides: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/spec_plan.rs#L229-L265
  - Index: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/handlers/tool_search.rs#L150-L186
  - Query handling and limit: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/handlers/tool_search.rs#L209-L282
  - Search limit constant: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/tools/src/tool_discovery.rs#L7-L7
  - Exposure enum: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/tools/src/tool_executor.rs#L49-L80
  - Schema compaction default: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/tools/src/json_schema/compaction.rs#L15-L15 and https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/tools/src/json_schema.rs#L36-L39
- Edge cases and failure handling: the switch is a model capability flag, not a token-count threshold; there is no automatic "defer when the list is larger than N". Models without the flag always get every tool directly.
- Round 1: `corrected` / `confirmed`. Round 1 says tools "can be routed to direct, deferred or code_mode per MCP server": the per-server control is `omit_tools_from` (subtractive), and the default choice between direct and deferred is made per **model**. Schema compaction 5,000 bytes is `confirmed`.
- For rizzy-harness: copy deferral behind a search tool, but make it depend on measured tool-list size (and keep a hard cap), since a flag per model will not cover local models. A local BM25 index is tiny and has no extra model call.
- Confidence: traced end to end.

#### Tool-list staleness: `list_changed` is recorded in transport order and the catalog cache is LRU+TTL
- What the code does: the MCP transport is wrapped so each `tools/list_changed` notification increments an atomic revision "in transport order, before a later tool response can let inference continue". A process-wide `McpToolCatalogCache` (32 entries, 30 min TTL, keyed by a hash of the server connection identity) reuses tool definitions across sessions; the definition handlers are shared by `Weak` references and swept on miss.
- Trace:
  - Revision tracking: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/tool_list_changes.rs#L1-L40
  - Cache constants: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/codex-mcp/src/tool_catalog_cache.rs#L34-L40
  - Shared definitions: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/mcp_tool_exposure.rs#L52-L104
- Edge cases and failure handling: the cache key includes credential policy so one account's tool list is not reused for another (see `McpServerConnectionIdentity` imports in `tool_catalog_cache.rs:25-27`); I did not trace the key construction.
- Round 1: `new (not documented)`.
- For rizzy-harness: bump a per-server revision on `list_changed` and re-resolve tool specs before the next model step; cache by (server identity, credentials) not by name.
- Confidence: read the definition.

#### Codex as an MCP server: removed; a different MCP server exists for the agent message board
- What the code does: the CLI subcommand list has `Mcp(McpCli)` (client management only) and no `mcp-server`; no `mcp-server` crate is in the workspace. The only MCP *server* code is `message-board-mcp`, which exposes the inter-agent message-board tools over MCP (in-memory and HTTP) for sub-agents; it is not the coding agent.
- Trace:
  - CLI subcommands: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/cli/src/main.rs#L166-L175
  - Message-board adapter: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/message-board-mcp/src/lib.rs#L1-L4
- Edge cases and failure handling: none.
- Round 1: `confirmed` (removed); the message-board server is `new (not documented)`.
- For rizzy-harness: a coding-agent-as-MCP-server was dropped for the typed protocol; this supports keeping our WS API and not offering MCP-out.
- Confidence: traced end to end (negative result by search).

### 3.8 Skills, plugins and hooks: loading and trust

#### Skill roots: config layers plus `.agents/skills` walking up to the project root; repo skills load with no trust check
- What the code does: roots come from (a) each config layer's folder: `Project` layers contribute `<.codex>/skills` as scope `Repo`, `User` contributes `$CODEX_HOME/skills` (deprecated), `~/.agents/skills` and a system cache dir (scope `System`), `System` layer contributes `<dir>/skills` (scope `Admin`); (b) plugin skill roots; (c) extra roots; and (d) `.agents/skills` in every directory from the cwd up to the project root (markers configurable), queried through the repository's `ExecutorFileSystem`. `SKILL.md` discovery recurses to depth 6; names are limited to 64 characters. Repo-scoped roots are loaded only for the local environment.
- Trace:
  - Assembly: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/skills/src/host_roots.rs#L97-L125
  - Per-layer roots: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/skills/src/host_roots.rs#L127-L190
  - Upward `.agents` walk: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/skills/src/host_roots.rs#L192-L260
  - Local-only gate: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/skills/src/host_service.rs#L76-L87 and https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/skills/src/host_service.rs#L435-L440
  - Scan constants: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/skills/src/loader/mod.rs#L21-L26
  - Layer iteration includes disabled layers: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/state.rs#L742-L757
- Edge cases and failure handling: project trust disables "project-local config, hooks, and exec policies" (`config/src/loader/mod.rs:1151`), and `all_layers_high_to_low` includes disabled layers. I found no trust check in `ext/skills/src` (grepped `trust`, `is_disabled`, `disabled_reason`): an untrusted repository's skills are still discovered and listed. Skill *scripts* run through the normal sandbox/approval path (safety cluster). This is an inference from absence; a test with an untrusted project would settle it.
- Round 1: `confirmed` for scopes (the `/etc/codex/skills` path was not verified: the Admin root is the `skills` dir beside the System-layer config file); the missing trust gate is `new (not documented)`.
- For rizzy-harness: skill text goes into the prompt, so an untrusted repo's skills are prompt injection. Gate repo-level skills on the same trust decision as repo config and hooks.
- Confidence: read the definition (trust inference by absence) (checked in review)

#### Hook trust: hash of the normalised hook, stored only in user config; plugin hooks add a bundle hash
- What the code does: for each discovered hook, `hook_hash` serialises (event name, matcher group with that single normalised handler) to TOML and hashes it (`version_for_toml`). Status is `Trusted` if the user's stored hash equals it, `Modified` if a hash exists but differs, `Untrusted` if none; managed (system, MDM, enterprise) hooks are always `Managed` and built-ins `Trusted`. A hook is added to the runnable set only if it is enabled and one of: `--dangerously-bypass-hook-trust` is set (documented in `shared_options` as "DANGEROUS. Intended only for automation that already vets hook sources"), or it is a plugin hook with trust state but no content hash, or its status is `Managed` or `Trusted`; `Untrusted` and `Modified` hooks are skipped. Trust state is read from `[hooks.state.<key>]` only in the **User** and **SessionFlags** layers: "Project, managed, and plugin layers can discover hooks, but they do not get to write user hook state". For plugin hooks the compared value is `<declaration_hash>/<content_hash>` where the content hash is SHA-256 over the plugin directory tree (names, sizes, bytes; skips `__pycache__`, `.venv`, `*.pyc`, `*.egg-info`; fails if a file changes while read). It is re-verified at execution time.
- Trace:
  - Hash and status: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L981-L1027
  - Declaration plus content hash: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L836-L880
  - Run gate (bypass flag, plugin case, Managed or Trusted): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L915-L925
  - Bypass flag definition: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/utils/cli/src/shared_options.rs#L61-L64
  - Layers allowed to hold trust state: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/config_rules.rs#L10-L30
  - Content hash: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/plugin_fingerprint.rs#L1-L40
  - Execution-time verification: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/plugin_trust.rs#L28-L50
- Edge cases and failure handling: the content hash "detects source changes, not tampering inside those runtime artifacts" (doc at `plugin_fingerprint.rs:3-5`), so `.venv` and bytecode are an open door. Hashes cover the handler definition, not the bytes of a script a `command` handler points at outside a plugin; a repo hook that runs `./scripts/x.sh` is trusted by command string, so editing `x.sh` does not re-trigger review. I read the hash input and did not find script bytes included for non-plugin hooks.
- Round 1: `confirmed` (trust by hash, user-side). Answers Round 1's open question: the hash is the normalised definition (plus plugin bundle bytes for plugin hooks), stored in user config, and a plugin update changes the content hash so it re-triggers review.
- For rizzy-harness: copy "trust state lives only in a layer the repository cannot write", and hash the referenced script bytes too, which Codex does not for repo hooks.
- Confidence: traced end to end (checked in review)

#### Plugins: manifest bundles from local, Git (pinned sha) or npm sources, behind an admin source allow-list
- What the code does: a marketplace lists plugins whose source is `Local{path}`, `Git{url,path,ref,sha}` or `Npm{package,version,registry}`. An allow-list policy from requirements restricts marketplaces to specific Git URLs, host patterns (regex) or local paths. Automatic background Git operations remove repository-scoped variables (`GIT_DIR`, `GIT_CONFIG*`, `GIT_WORK_TREE`, and others) from the child environment so a hostile launch project cannot redirect them; user-initiated operations keep them. A plugin can contribute skills, MCP servers and hooks (and apps); plugin MCP specs are size-budgeted (above).
- Trace:
  - Sources: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core-plugins/src/marketplace.rs#L128-L143
  - Allow-list: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core-plugins/src/marketplace_policy.rs#L36-L52
  - Git env scrubbing: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core-plugins/src/git_policy.rs#L7-L50
- Edge cases and failure handling: I did not trace install-time review, only the policy and the hook/trust side.
- Round 1: `confirmed` (manifest, Git/local/remote marketplaces, admin sync); npm sources and Git env scrubbing are `new (not documented)`.
- For rizzy-harness: scrub `GIT_*` from any Git call made on behalf of an untrusted checkout; it is cheap and closes a known class of attacks.
- Confidence: read the definition.

#### Project trust gates config, hooks and exec policies for a directory
- What the code does: `[projects.<path>] trust_level` in user config is looked up by directory, project root and repo root. If not trusted, the project layer is *disabled* with a reason string saying "To load project-local config, hooks, and exec policies, add `<key>` as a trusted project in `<user config>`". `codex exec --trust` writes the decision.
- Trace: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/loader/mod.rs#L1100-L1170
- Edge cases and failure handling: an explicitly `untrusted` entry changes only the message, and managed config can set trust.
- Round 1: `confirmed` (project config blocklist is the safety cluster; whether the blocklist is enforced for all clients is not settled here).
- For rizzy-harness: same model; do the same for repo-provided skills (previous finding).
- Confidence: read the definition (checked in review)

## 4. Round 1 claims checked

| Claim (Round 1) | Verdict | Citation |
|---|---|---|
| Events saved in `rollout-*.jsonl`, each written by one process | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/writer_lock.rs#L45-L95 |
| SQLite index in WAL mode; NFS uses DELETE journaling | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/state/src/nfs_migration.rs#L1-L3 |
| `thread/revert` drops turns, does not revert files | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L1316-L1326 |
| `thread/rollback` removed | confirmed (no such method; marker only read from old logs) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/session/rollout_reconstruction.rs#L300-L306 |
| No automatic file checkpoints in the CLI | confirmed (feature existed and was removed) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/rollout_migration/line_parser.rs#L47-L58 |
| Resume restores saved permissions subject to overrides | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/request_processors/persisted_resume_settings.rs#L20-L45 |
| `thread/fork` copies history up to `lastTurnId` | corrected (paginated forks reference the parent prefix by position) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/protocol.rs#L3463-L3477 |
| Non-loopback WebSocket listeners are unauthenticated by default "during rollout" | refuted (startup refused without auth) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L160-L175 |
| WebSocket auth enforced before `initialize`; `Origin` gets 403 | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L99-L130 |
| Bounded queues answer `-32001` | corrected (requests only; other messages block) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/mod.rs#L231-L262 |
| Daemon Unix socket is host-local | confirmed (0700 dir, 0600 socket, verified symlink) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L322-L352 |
| Schemas via `generate-ts` / `generate-json-schema`, version-matched | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/export.rs#L125-L150 |
| `environment_id` runs an MCP server on a remote executor | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/executor_process_transport.rs#L1-L20 |
| MCP tools can be routed to direct/deferred/code_mode per server | corrected (default is per model; per server is subtractive `omit_tools_from`) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/mcp_tool_exposure.rs#L136-L170 |
| MCP schema compaction default 5,000 bytes | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/tools/src/json_schema/compaction.rs#L15-L15 |
| Codex MCP server removed | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/cli/src/main.rs#L166-L175 |
| Hooks trusted by hash; changed hook skipped until reviewed | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L1000-L1016 |
| Managed worktree starts from HEAD plus uncommitted changes; Handoff moves chat and code | not in code (HEAD only; Handoff is closed Desktop) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/worktree/src/lib.rs#L62-L140 |
| `codex exec` refuses to run outside Git unless skipped | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec/src/lib.rs#L1074-L1084 |
| Skills from `.agents/skills` cwd to repo root, `~/.agents/skills`, `/etc/codex/skills` | confirmed for the first two; Admin root is the System-layer config dir (path not verified) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/skills/src/host_roots.rs#L127-L190 |
| Compaction/records: `rollout compress` not mentioned | n/a (see section 5) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/compression.rs#L368-L378 |

## 5. In the code but not in the documentation

- Paginated history mode: forks and reverts are pointer files over immutable prefix rollouts; compare-and-set of the rollout path in SQLite.
- Noise (X25519 + ML-KEM-768) relay for remote executors with registry-side authorisation of the harness key, 128 multiplexed streams and ack/resume framing.
- Executor sessions: `resumeSessionId`, one attachment at a time, 80 s retention.
- Per-method serialisation scopes in the protocol macro.
- `requiredHumanApproval` capability and `HumanCommand` approval kind.
- Daemon recovery snapshot of loaded and interrupted threads.
- Reverse-replay resume with compaction as a base; `ThreadRolledBack` markers.
- zstd compression of rollouts older than 7 days.
- MCP tool-catalog LRU cache (32 entries, 30 min) and transport-ordered `list_changed` revision.
- Agent-plugin MCP spec byte budgets (8,000 per spec, 64,000 total) that hide tools.
- Plugin sources include npm; automatic plugin Git calls scrub repository-local `GIT_*` variables.
- Remote-control relay host allow-list (`chatgpt.com`, localhost).
- Executor credential pipes that exit the process when the parent closes stdin.

## 6. Could not find or could not settle

- **Handoff packaging.** Not in this repository; the Desktop app is closed. What would settle it: the Desktop source, or a protocol trace of a Local-to-Worktree move.
- **Remote-control pairing code expiry and the registry/relay server behaviour.** Only the client side is open; `expires_at` is returned by the service.
- **Executor-side refusal.** I found no allow-list on the exec-server that caps what the harness may ask for. A search of `exec-server/src/server/` for policy checks would be needed to be sure; I read the process and sandbox preparation paths only.
- **Repo-skill trust.** Inferred from the absence of any trust check in `ext/skills`; an integration test with `trust_level = "untrusted"` would settle it.
- **First `persist()` trigger** for a new rollout (which turn event materialises the file).
- **Legacy-mode fork** (copy semantics) and `rollout_lineage` fallback were not traced.
- **Whether a missing eligible connection for `HumanCommand` blocks or auto-declines** (remaining lines of `outgoing_message.rs` after 430).
- **`/etc/codex` as the Admin skills path:** the code uses the System config layer's parent directory; the concrete path comes from the config loader, not read here.
- Needed from outside this repo: the MCP OAuth specification details (PKCE, resource indicators) are delegated to the `rmcp` crate, which was not cloned.

## 7. Citation check

`research/round2/codex.sessions.md: 181 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit`

## 8. Review

Independent review against the clone at the pinned SHA. Checked 15 of 37 findings. Verdicts: 9 supported, 3 absence confirmed, 3 partly (corrected in text), 0 wrong, 0 not supported.

| Finding | Verdict | Reason |
|---|---|---|
| One writer per thread (file lock) | supported | `writer_lock.rs` `acquire`: coordination lock, `WouldBlock` on second writer |
| Revert via compare-and-set | supported | `replace_rollout_path_if_current`, replacement file deleted and `Conflict` on failure |
| No file undo or checkpoint | absence confirmed | no restore path; retired `ghost_snapshot` and `undo_completed` are only skipped; a git baseline repo exists for memories diffs only |
| Non-loopback WebSocket refused at startup | partly | true for the app-server; the exec-server listener has no refusal and may be unauthenticated |
| Remote exec: executor builds sandbox from caller intent | supported | `params.sandbox` absent means no sandbox; no executor-side ceiling found (only MCP requirements) |
| Executor dials out, pairing | partly | direct `--listen` auth is optional, not "the same" guarantee |
| Reconnect and fencing | supported | `RECOVERY_RETENTION` 80 s, `SESSION_ALREADY_ATTACHED`; no epoch or fencing term found anywhere in exec-server |
| Handoff not in open-source tree | absence confirmed | ripgrep for handoff finds only unrelated hits |
| Scheduling does not exist | partly | engine has none, but a desktop automation feature flag and thread source exist |
| `codex exec` approvals forced off | supported | default `AskForApproval::Never`; approval requests rejected |
| Remote TUI token only over safe URL | supported | `resolve_remote_endpoint` requires `wss://` or loopback |
| Transports and where auth happens | supported | Origin middleware, `authorize_upgrade` at upgrade, no post-upgrade expiry check |
| Skill roots, no trust check | absence confirmed | roots built from `all_layers_high_to_low` including disabled project layers; no trust code in `ext/skills/src` |
| Hook trust hashing the declaration | supported | `hook_hash` covers event, matcher and handler config only |
| Project trust gates config, hooks, exec policies | supported | `disabled_reason_for_decision` text and logic |

Not checked: rollout file format, deferred creation, resume reverse replay, permission restore, fork pointers, snapshot validation, retention and SQLite index, protocol macro and serialisation scopes, overload, daemon details, human-approval capability, remote control, Noise stream numbers, all of MCP client (3.7), plugins.

Overall: the verdicts that were checked hold. The main correction comes from stopping at the app-server: the exec-server has its own WebSocket listener without the refusal (the scheduling correction is separate wording). Treat claims of the form "no flag or variable bypasses this" as scoped to the component named. The unchecked remainder (mostly read-the-definition storage and MCP detail) is probably reliable but has not been confirmed.
