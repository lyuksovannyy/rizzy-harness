# Oh My Pi (`omp`): sessions, remote operation and extensibility (Round 2, source code)

## 1. Repository

- Repository: `can1357/oh-my-pi`, branch `main`, pinned commit `e325ffc73ab86e03bdd20a91b77d8e2b3964c906`, commit date 2026-11-10 (`packages/coding-agent/package.json` says version 18.18.7).
- Read: the session package (`packages/coding-agent/src/session/`: entries, manager, context rebuild, loader, storage, paths, migrations, persistence, `agent-session.ts` tree/branch/fork/rewind paths), the RPC and ACP modes (`modes/rpc`, `modes/acp`), print mode, the MCP client (`src/mcp/`), extension, hook and plugin loading (`src/extensibility`, `src/discovery`, `src/capability`), the memory back ends (`src/memory-backend`, `src/hindsight`, `src/mnemopi`, `src/sharpshooter`), collab (`src/collab`, `packages/wire`, the offline relay stand-in in `packages/collab-web/scripts/local-relay.ts`), the auth broker and gateway (`packages/ai/src/auth-broker`, `auth-gateway`), the stats package (`packages/stats`), the browser relay server and the `ovnfs` client.
- Not read in depth: the compaction algorithms themselves (agents cluster), the TUI, the Rust natives, the hosted relay (its Go source is not in the repository; `docs/collab.md` says so, I did not rely on that page for any code claim).
- Docs were used only to find the way. Every claim below cites code.

## 2. Architecture as it bears on this cluster

- Languages and process model: TypeScript on Bun (`engines.bun >= 1.4.4`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/package.json#L563-L565) with a Rust N-API addon (`crates/`). One process per interactive session: TUI, agent loop, tools and the session writer live together. RPC, ACP, print and collab all embed that same process (`createAgentSession` takes the `SessionManager` and a `hasUI` flag: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L937-L952). There is no master/worker split in the product. The only separate server processes are optional: `omp auth-broker serve`, `omp auth-gateway`, `omp stats`, the browser relay, the LSP mux, and the external `ovnfs` daemon.
- Key types and where decisions live:
  - `SessionEntry` union, header and leaf rules: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L355-L397.
  - `SessionManager` (append, branch, fork, journal writing, ownership claim): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L1157-L1182 and following.
  - `buildSessionContext` (path walk and compaction replay): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L270-L323.
  - `AgentSession.navigateTree` / `branch` / `fork` / rewind: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L14717-L14762.
  - Memory back end interface: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/memory-backend/types.ts#L108-L123 (`MemoryBackend`), selection in `resolve.ts`.
  - Capability registry for foreign-harness discovery: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/capability/index.ts#L145-L180.

## 3. Findings

### Session storage: format, location, resume, fork, export, retention

#### One JSONL file per session; the first physical line is a fixed 256-byte title slot
- What the code does: a session file is JSONL. Line 1 is a mutable "title slot" padded to exactly 256 bytes so the title can be rewritten in place without rewriting the log. Line 2 is the `session` header (id, cwd, optional `parentSession`, `continuationOf`, `additionalDirectories`, `seanceSource`, prompt-cache keys). Every following line is a tree entry. Loaders strip and fold the slot into the header.
- Trace:
  - Slot size, header fields: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L17-L75.
  - Slot serialiser, asserts the line is exactly 256 bytes: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-title-slot.ts#L111-L122.
  - In-place overlay on a read prefix: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-title-slot.ts#L124-L141.
  - Parsing folds the slot into the header and counts malformed records: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-loader.ts#L167-L180.
- Edge cases and failure handling: malformed JSONL lines are skipped and counted, not fatal (`parseJsonlLenient`, `malformedRecords`); a file whose first record is not a valid header is flagged `invalidHeader` (same lines). Version migrations exist from v1 (no ids) and v2 (`hookMessage` role) to v3: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-migrations.ts#L15-L82.
- Round 1: `confirmed` (JSONL, version 3, optional 256-byte title record, one header).
- For rizzy-harness: the fixed-width in-place header slot is a cheap trick for a mutable title in an append-only file; with SQLite you do not need it. Note the project rule "no compatibility code before the first tag" runs against copying the migration ladder.
- Confidence: traced end to end.

#### Entries form a tree by `id`/`parentId`; every append is a child of the current leaf
- What the code does: entries carry an 8-hex-char `id` (collision-checked against the index) and `parentId`. `appendMessage` makes a child of the leaf and advances the leaf. Entry kinds: `message`, `model_usage`, `thinking_level_change`, `model_change`, `service_tier_change`, `compaction`, `branch_summary`, `custom`, `custom_message`, `label`, `title_change`, `ttsr_injection`, `session_init`, `mode_change`, `credential_pin`, `reset_boundary`. Every entry also records the omp build that wrote it (`runtime`).
- Trace:
  - Union and base fields: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L93-L111, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L355-L371.
  - Id generation, 8 hex chars, 100 retries then a snowflake: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-migrations.ts#L4-L10.
  - Append: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L3577-L3589; `#recordEntry` pushes to memory, indexes, then appends to disk: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L2158-L2176.
- Edge cases and failure handling: a parent cycle in a corrupt file is cut at the first repeat so load is bounded: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L313-L323. Records written after the manager was released are dropped with a warning: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L2158-L2162.
- Round 1: `confirmed` (entries have id/parentId); the Round 1 entry list is incomplete (see section 5).
- For rizzy-harness: our log is already contiguous `seq` with one writer; a parent pointer per event is the only addition needed to get in-log branching. Note that omp keeps a single "leaf" in memory and derives it on load.
- Confidence: traced end to end.

#### A branch inside one log is just a leaf move; the active path is rebuilt by walking parents from the leaf
- What the code does: `branch(id)` only moves the in-memory leaf; no entry is modified or deleted, so the next append becomes a sibling. `buildSessionContext` takes a leaf (default: the replayed leaf), walks `parentId` to the root, reverses, then folds settings (`thinking_level_change`, `model_change`, `service_tier_change`, `mode_change`, `ttsr_injection`, latest `compaction`) and emits messages. The leaf after load is defined by replaying the file: the last physical entry, unless that entry carries `activeLeafId` (an off-branch write such as late usage or a background result routed back to its branch), in which case that leaf is restored.
- Trace:
  - `branch` and `resetLeaf`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4048-L4060.
  - Leaf replay rules: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L373-L397.
  - Off-branch appends keep the active leaf: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L3595-L3628.
  - Path walk: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L284-L323; settings fold: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L343-L379.
- Edge cases and failure handling: the comment on `discardEntryDurably` states that the loader "reconstructs the active branch from the last physical journal entry, so changing the in-memory leaf alone is lost on reload": https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4062-L4075. I infer from this and `resolveSessionLeafId` that a `/tree` jump with no summary and no later message is not durable; the next appended entry makes it so. A captured leaf that no longer exists falls back to the newest self-selecting entry: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L391-L397.
- Round 1: `confirmed` (siblings in the same file); `new (not documented)`: the `activeLeafId` mechanism and the durability caveat.
- For rizzy-harness: if we add branching to the event log, store the active leaf explicitly (a `leaf_set` event) instead of inferring it from the last row, or a navigation without a following write is lost.
- Confidence: traced end to end for the rebuild; the non-durable-jump consequence is inferred from comments and `resolveSessionLeafId`. (checked in review)

#### Compaction never deletes entries; it is an entry on the path that changes what the rebuild emits
- What the code does: a `compaction` entry holds the summary, `firstKeptEntryId`, `tokensBefore`, `method`, optional provider-native replay payloads in `preserveData`. When building model context, the latest compaction on the path (or, given a model, the newest one that model can replay) emits the summary first, then the kept messages from `firstKeptEntryId` up to the compaction, then everything after. The display transcript can show every entry with compactions inline, or the collapsed tail only. Old entries stay on disk.
- Trace:
  - Entry shape: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L163-L187.
  - Readable-compaction selection by model: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L253-L268.
  - Emission order comment: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L392-L397; compaction replay and kept tail: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L538-L668.
  - Full-history transcript path: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L477-L517.
- Edge cases and failure handling: `reset_boundary` (written by `/clear`) is a durable marker; later of boundary or compaction wins, and the on-disk history stays: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L518-L537. `firstKeptEntryId` may be empty for provider-native snapshots, handled via `providerReplayThroughEntryId`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L636-L641.
- Round 1: `confirmed` (compaction in the entry list); `new (not documented)`: compaction replay is model-dependent, so switching model can re-expand history behind an unreadable compaction.
- For rizzy-harness: model the summary as an event on the path with a "first kept" pointer, never a rewrite. Add a `reset` marker the same way.
- Confidence: traced end to end for the fold; I did not read the emission of post-compaction entries after line 668.

#### Location, naming and the cwd-to-directory mapping
- What the code does: sessions live under `~/.omp/agent/sessions/` (via `dirs.agentSubdir(..., "sessions", "data")`), one subdirectory per project, named by an encoding of the cwd: home-relative, temp-root or absolute scope, with a hashed variant for legacy names. File name is `<timestamp>_<id>.jsonl`. Subagent sessions live in a sibling directory named after the parent file: `<parent>/<AgentId>.jsonl`, nested for grandchildren. A per-terminal "breadcrumb" records the last session for `--continue`.
- Trace:
  - Root: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/utils/src/dirs.ts#L1009-L1012.
  - Cwd encoding: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-paths.ts#L78-L110; default dir: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-paths.ts#L222-L224.
  - Subagent layout: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/sub-sessions.ts#L1-L7; breadcrumb resolve: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L382-L392.
  - Fork file name: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4164-L4166.
- Edge cases and failure handling: `continueRecent` honours a fresh-session breadcrumb whose file was never written, so `--continue` does not resurrect an older transcript; it re-roots a session only when the recorded cwd is gone and the location is the same directory inode: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4573-L4640.
- Round 1: `confirmed` (`~/.omp/agent/sessions/`); the cwd encoding and breadcrumb logic are `new (not documented)`.
- For rizzy-harness: not applicable to a DB-backed log, but the lesson is that resume-by-terminal and cwd moves (worktrees) generate most of the edge cases.
- Confidence: read the definitions.

#### Crash and concurrency discipline: append on the hot path, full rewrite when diverged, an ownership lease per session
- What the code does: the hot path appends one JSON line through a writer synchronously (`appendSync`) so a failure latches before the call returns. If the file is not current, in-memory entries diverged, or another live omp process owns the session, the whole file is rewritten (atomically) instead, and in the "owned elsewhere" case to a sibling file with a new id. Ownership is an OS file lock per session id that the kernel releases when the process dies. A non-owner never writes the owner's file.
- Trace:
  - Cold/hot paths: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L1946-L1989.
  - Claim and "owned elsewhere": https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L1157-L1182.
  - Lease implementation: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-storage.ts#L536-L541; acquire: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-storage.ts#L638-L640.
  - Header field for the sibling continuation: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L57-L63.
- Edge cases and failure handling: a disk failure sets `#diskFailure`, marks a rewrite required and retries on the next entry (same lines). A write conflict (file changed under the writer) is recovered or reported: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L1192-L1206.
- Round 1: `new (not documented)`.
- For rizzy-harness: this is the same "one writer per session" invariant, enforced with a lock plus a fork-to-sibling fallback rather than fencing. For a node link we need an explicit fence token, not a file lock.
- Confidence: read the definitions; traced the claim path.

#### Large content is truncated or externalised before it reaches the log
- What the code does: persisted strings are cut at 500,000 characters with a visible truncation notice; image base64 above 1 KiB is moved to a content-addressed blob store (`blob:sha256:<hash>`) and the entry keeps a reference; loaders resolve refs on read (snapcompact frames stay lazy).
- Trace:
  - Limits: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-persistence.ts#L11-L16.
  - Blob addressing and write-grace for gc: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/blob-store.ts#L7-L30.
- Edge cases and failure handling: strings damaged by an older truncation pass are detected by their suffix so migrations can treat them safely: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-persistence.ts#L28-L31.
- Round 1: `confirmed` (blob store for images); the 500k-char cap is `new (not documented)`.
- For rizzy-harness: the log keeps a lossy copy of tool output by design; check that our events cap output too and that the cap is visible to the model and the UI.
- Confidence: read the definitions.

#### Storage back ends are pluggable (file, memory, SQL, Redis) but only the file back end is wired into the CLI
- What the code does: `SessionStorage` is an interface; `FileSessionStorage` and an in-memory one are used by the product. `SqlSessionStorage` (Postgres, MySQL, SQLite via `Bun.SQL`) and `RedisSessionStorage` are exported from the package index for SDK embedders. Both emulate a file with compare-and-set on expected size (`SessionWriteConflictError`).
- Trace:
  - Interface: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-storage.ts#L155-L200.
  - SQL: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/sql-session-storage.ts#L9-L60; Redis: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/redis-session-storage.ts#L1-L50.
  - Exports only: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/index.ts#L46-L55 (no other reference to either class outside the two files, found with grep).
- Edge cases and failure handling: indexed back ends publish asynchronously, so `SessionManager` waits for confirmation before advancing its durable-size record: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-storage.ts#L155-L172.
- Round 1: `not in code` for Round 1 (the survey does not mention them): `new (not documented)`.
- For rizzy-harness: the idea of a narrow storage trait with compare-and-set on size is a reasonable shape for a log shared with a master; the product itself does not use it.
- Confidence: read the definitions; the "not wired" claim rests on a grep of the package.

#### Resume, `/branch`, `/fork`, `/tree` are four different operations
- What the code does:
  - `/tree` (`navigateTree`) moves the leaf inside the same file. Selecting a user message sets the leaf to its parent and returns the text to the editor; selecting anything else lands on that node. It can first summarise the abandoned branch (model call, or an extension's `session_before_tree` summary) and attach a `branch_summary` entry at the target. Hooks can cancel.
  - `/branch` (`AgentSession.branch`) only accepts a user message and creates a NEW file holding root-to-parent path (`createBranchedSession`), header `parentSession` set; labels are re-created fresh on the kept path.
  - `/fork` clones the whole session to a new file (`SessionManager.fork`), or with an entry id behaves like `branch` and copies artifacts.
  - `forkFrom` copies another session's history into the current project dir; a `seance` option makes a read-only consult fork whose artifacts stay in the source.
- Trace:
  - `navigateTree` entry and no-op rules: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L14717-L14777; summary and hook: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L14841-L14899; leaf choice: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L14917-L14970; leaf move: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L14972-L15009.
  - `branch`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L14438-L14455; `createBranchedSession`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4144-L4226.
  - `fork`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L2526-L2560 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L12391-L12404; `forkFrom` options: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4271-L4299.
- Edge cases and failure handling: tree navigation is queued while a session transition is in progress and can refuse when busy (`requireIdle`): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L14749-L14752, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L14901-L14902. Navigating to the entry that is already the leaf is a no-op, except when the target is a user message (the leaf moves to its parent) or the `ask` re-answer protocol applies (same lines, L14765-L14777); a move to any other entry is a real leaf move. The summary is anchored at the new leaf, not the old one: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L14972-L14986.
- Round 1: `confirmed` (`/tree`, `/branch` rewinds, `/fork` clones); `new (not documented)`: `/branch` creates a new file (Round 1 groups it with in-file rewinds), and the seance fork.
- For rizzy-harness: copy "summary attached at the destination, abandoned branch stays reachable". The `ask` re-answer protocol in `navigateTree` (a new sibling tool result) shows how much UI state hangs off tree moves.
- Confidence: traced end to end. (corrected in review: the no-op rule was overstated; it applies only when the target equals the current leaf)

#### Resume opens the recorded cwd only if it is enterable; foreign sessions import into the same format
- What the code does: `SessionManager.open(path)` loads the file, resumes in the header's cwd if the directory is accessible, else falls back to the launch cwd. `--from-claude`/`--from-codex` stores convert Claude Code and Codex JSONL into omp entries and persist them under a fresh session identity.
- Trace:
  - Open and cwd fallback: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4439-L4461.
  - Foreign import: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/foreign-session-import.ts#L8-L40; Claude record conversion: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/claude-session-store.ts#L1-L30.
- Edge cases and failure handling: a transcript deleted or replaced during the cwd probe fails closed for revive callers (re-read after the probe): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4463-L4469.
- Round 1: `confirmed` (`--from-claude`, `--from-codex`).
- For rizzy-harness: importing other harnesses' transcripts is a cheap adoption feature; the conversion layer is isolated in one store class per source.
- Confidence: read the definitions.

#### Export and share: client-side sealing with secret redaction
- What the code does: `/export` writes HTML; `/share` gzips the session JSON, seals it with a fresh AES-256-GCM key, and uploads the blob; the key travels only in the URL fragment. Before sealing, a `SecretObfuscator` rewrites header, system prompt, tool descriptions, entries and sub-sessions.
- Trace:
  - Design comment and options: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/share.ts#L1-L36, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/share.ts#L59-L97.
  - Redaction walk: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/share.ts#L260-L282.
  - CLI command: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/commands/share.ts#L1-L20.
- Edge cases and failure handling: untyped payloads (`compaction.preserveData`, extension data) are handled by the redaction code rather than skipped (same file, comment at L100-L110 region; I did not read the walker beyond L282).
- Round 1: `confirmed` (client-side encryption, key in fragment).
- For rizzy-harness: redact before sealing, and test that redaction reaches opaque extension payloads.
- Confidence: read the definitions.

#### Retention: nothing is deleted by default
- What the code does: `omp gc` has settings for stale-session deletion (`gc.stale` default false, keep newest 20 and 30 days), compression (default false), stripping (default false), blob and WAL cleanup (default true), cold archive after 30 days. Sessions are therefore kept indefinitely unless the user opts in.
- Trace:
  - Defaults: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/gc-settings.ts#L7-L41.
  - Command imports the lease check so gc skips live sessions: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/gc-cli.ts#L35-L36.
- Edge cases and failure handling: gc uses `tryAcquireSessionLease` per session, so a session held by a live process is not touched: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-storage.ts#L630-L640.
- Round 1: `confirmed` (session GC mentioned); the defaults are `new (not documented)`.
- For rizzy-harness: decide retention explicitly; omp's answer is "keep forever, offer gc".
- Confidence: read the definitions. (checked in review)

### Checkpoints and undo

#### `checkpoint`/`rewind` rewind the conversation only; there is no file checkpoint or undo
- What the code does: `checkpoint` records a goal; the agent explores; `rewind(report)` makes the session `branchWithSummary` from the checkpoint entry with the report as the summary and then adds a `rewind-report` custom message. Files changed during the exploration are not reverted. The tool is off by default (`checkpoint.enabled` default false) and its approval tier is `read`. The tool's own summary string says "Create a git-based checkpoint to save and restore session state", but the code touches no git and no files.
- Trace:
  - Tools: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/checkpoint.ts#L53-L131 (summary string at L57; `execute` only returns text).
  - Default off: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L747-L755.
  - Rewind applies a branch summary: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L13006-L13030; agent is forced to call `rewind` before yielding while a checkpoint is active: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L12963-L12978.
  - Context rebuild hides the duplicate: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L433-L450.
- Edge cases and failure handling: if the checkpoint entry is missing, rewind falls back to branching from root: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L13006-L13020 region ("Rewind branch checkpoint missing, falling back to root"). Sibling tool calls in the same batch are re-parented onto the new branch so their completed work stays visible (same function).
- Round 1: `confirmed` (off by default, "docs describe no file checkpoints or undo"); `corrected` detail: the tool description string claims git-based state, the code does not.
- For rizzy-harness: our checkpoint spec should say clearly whether files are restored; omp's answer is "no, the conversation is rewound and files are on you". I searched `undo`, `rollback`, `revert`, `shadow git`, `snapshot` under `packages/coding-agent/src` and `crates/` and found no file-level restore (only an edit-side file snapshot for hashline anchors, not an undo).
- Confidence: traced end to end for conversation rewind; "no file undo" is a negative result from grep. (checked in review)

### Client and server split, RPC, SDK, headless

#### The SDK is in-process; `createAgentSession` takes the session manager and returns a live session
- What the code does: `createAgentSession(options)` builds a session in the caller's process. `sessionManager` may be `SessionManager.inMemory()` or a file-backed one; `hasUI` only enables tool dialogs; `enableMCP` and `enableLsp` default on unless a tool allow-list restricts. SDK requires Bun (>= 1.4.4 in code).
- Trace:
  - Options and in-memory example: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L937-L968, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L1941-L1945.
  - Default file-backed manager: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L2126-L2127.
  - MCP default: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L2830-L2838.
  - In-memory manager: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4703-L4710.
- Round 1: `corrected`: Round 1 quoted "Bun 1.3.14+"; the code and `docs/sdk.md` say 1.4.4 (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/package.json#L563-L565).
- For rizzy-harness: not directly relevant (our node runtime is Rust).
- Confidence: read the definitions. (checked in review)

#### RPC mode: NDJSON over stdio, ready frame, optional protocol v2 chunking, no authentication
- What the code does: `omp --mode rpc` reads JSON commands on stdin and writes responses and events on stdout. First frame is `ready` (protocol 1, supported 1 and 2, max frame sizes). A `negotiate_protocol` command switches the encoder to v2, which splits oversize frames into `rpc_chunk` frames (256 KiB payloads, up to 64 MiB reassembled). A frame over 1 MiB in v1 is progressively shrunk (strings and arrays elided with an explicit marker). Stdout is protected from stray writes (notifications are switched off). There is no authentication: the trust boundary is the parent process that owns the pipes.
- Trace:
  - Limits and shrink passes: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-frame.ts#L5-L16, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-frame.ts#L88-L96.
  - Ready frame type: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-types.ts#L376-L394; emitted: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L1417-L1428.
  - Negotiation: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L2023-L2027; encoder switch: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L1429-L1432.
  - Command set (prompt, steer, follow_up, abort, session switching, branch, fork, navigate_tree, get_entries since, get_tree, compact, bash, host tools, host URI schemes, subagent inspection, login): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-types.ts#L48-L167.
  - Mode names: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/args.ts#L25.
- Edge cases and failure handling: stdin EOF disposes the session and fails pending side-channel requests: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L2956-L2975. A stuck stdout writer terminates the process (`outputWriter` failure handler): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L1412-L1416.
- Round 1: `confirmed` (v1 1 MiB, v2 up to 64 MiB); `new (not documented)`: v1 frames are shrunk by passes, not rejected.
- For rizzy-harness: our UI protocol runs over a socket; copy the explicit frame cap with "elided" markers rather than disconnects, and the `get_entries since` incremental read for reconnect.
- Confidence: read the definitions; traced negotiation. (checked in review)

#### RPC can pull the session log incrementally and expose host tools and URI schemes
- What the code does: `get_entries` returns session entries (optionally since an entry id) plus the leaf; `get_tree` returns the tree and leaf; `get_session_lease` probes whether a live process holds each listed session id. `set_host_tools` lets the client register tools executed by the client (the agent sends `host_tool_call` frames and waits for `host_tool_result`); `set_host_uri_schemes` does the same for URL schemes. RPC-registered tools default to `loadMode` "discoverable" unless a built-in name is re-registered.
- Trace:
  - `get_entries`, `get_tree`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L2341-L2360; lease: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L2661-L2666 over https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-storage.ts#L664-L672.
  - Host tool bridge: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/host-tools.ts#L1-L60.
  - Extension dialogs as `extension_ui_request`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L1226-L1250.
- Edge cases and failure handling: an unknown `since` id returns an `unknown_since` error rather than silently sending everything: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L2353.
- Round 1: `confirmed` (host tools, host URI schemes, extension UI requests).
- For rizzy-harness: host tools are the client-side counterpart of our node tools; the "client answers a request by id, cancel by id" pair maps to our approvals.
- Confidence: read the definitions.

#### Headless operation: print mode and `--mode json`; no UI means approvals fail closed
- What the code does: `omp -p` prints the final answer; `--mode json` streams all events. No scheduler or daemon exists (searched `cron`, `schedule` under `src`). When a tool needs approval and no UI exists, the tool wrapper throws "requires approval but no interactive UI available" and suggests `tools.approvalMode: yolo`. The default `tools.approvalMode` is `yolo`, so a default headless run never prompts.
- Trace:
  - Print mode doc and options: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/print-mode.ts#L1-L60.
  - Fail-closed branch: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L446-L470.
  - Default `yolo`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L314-L318.
  - Hook failing closed: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L293-L340.
- Edge cases and failure handling: the memory back end defaults to off for RPC and ACP hosts (`protocolDefault`): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/memory-backend/settings.ts#L11-L16.
- Round 1: `confirmed` (no scheduling; yolo default is stated for approvals in Round 1 section 5, not restated here).
- For rizzy-harness: unattended runs need either a policy that never asks or a fail-closed default; omp chooses permissive default plus fail-closed when a prompt would be required.
- Confidence: read the definitions. (checked in review)

#### ACP is a second stdio protocol; stray stdout is redirected to stderr
- What the code does: `omp acp` serves the Agent Client Protocol over stdio via `AgentSideConnection`. Before starting, `process.stdout` is replaced by a sink that writes to stderr and `console.log` is rebound, so extensions and dependencies cannot corrupt the JSON-RPC channel.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/acp/acp-mode.ts#L43-L72 (isolation), https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/acp/acp-mode.ts#L74-L90 (entry).
- Edge cases and failure handling: when stdin is a TTY the process prints a hint on stderr that no ACP client is attached.
- Round 1: `confirmed` (ACP via `omp acp`).
- For rizzy-harness: if we ever expose a stdio protocol, capture the real fd 1 first and detour everything else.
- Confidence: read the definitions.

### Remote workers, nodes and executors

#### No node/worker split; SSH is file access only, never a shell back end
- What the code does: the loop and tools always run in the process that owns the session. The `ssh://host/path` internal URL reads, lists and writes remote text files through an OpenSSH ControlMaster connection, for the read/search/write tools; files are UTF-8 and at most 1 MiB, directories list one level. No tool runs `bash` over SSH.
- Trace:
  - Handler: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/internal-urls/ssh-protocol.ts#L1-L35.
  - The only importers of `ssh/` outside the folder are the URL handler, the CLI config writer, the blob broker uploader and session teardown (`closeAllConnections`): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L259-L259 (searched with grep).
- Round 1: answers the open question (Round 2 brief: "whether `ssh://` support extends to `bash`"): `confirmed` as "not in code" for remote execution; Round 1 said "nothing documented".
- For rizzy-harness: the contrast is useful: omp has no remote executor, we do. Nothing to copy.
- Confidence: negative result from grep over `src`. (checked in review)

#### Collab: the host runs everything; the relay forwards sealed frames by room id, with no accounts
- What the code does: `/collab` publishes a live session. The host opens a WebSocket to `wss://my.omp.sh/r/<roomId>` (default), generates a 32-byte room key and a 16-byte write token, and puts `roomId`, key and token in the link (full link) or only the key (view link). All session payloads (`welcome`, `snapshot-chunk` of session entries, `entry`, `event`, `state`, `ui-request`, `transcript`) are JSON sealed with AES-256-GCM (`[12-byte IV][ciphertext+tag]`). The relay sees an envelope `[4-byte peer id][sealed bytes]` plus plain TEXT control messages `peer-joined`, `peer-left`, `room-closed`. A guest's first frame is `hello`; the host marks the peer writable only if the token matches (timing-safe compare). Prompts, aborts and agent commands from a read-only peer get a targeted error.
- Trace:
  - Protocol and frame list: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/protocol.ts#L1-L100.
  - Sealing: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/crypto.ts#L1-L59.
  - Constants and relay control types: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/wire/src/index.ts#L402-L467, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/wire/src/index.ts#L486-L489.
  - Socket URL has no query string; role carried separately: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/relay-client.ts#L99-L106.
  - Offline relay stand-in shows what the relay does: room id regex, first host wins (close 4009), guest to missing room 4004, peer id rewritten by the relay for guest frames, host disconnect closes the room: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/collab-web/scripts/local-relay.ts#L1-L50.
  - Token check and `hello`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/host.ts#L856-L861, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/host.ts#L882-L920; prompt gate: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/host.ts#L1127-L1135.
- Edge cases and failure handling: the host serialises the snapshot synchronously and chunks it, a guest that cannot keep up is shed and may auto-rejoin at most 3 times in 5 minutes: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/wire/src/index.ts#L450-L463. Protocol version mismatch is refused (`COLLAB_PROTO` 3): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/host.ts#L882-L892. Guest traffic is ignored while the session is starting or transitioning.
- Round 1: `confirmed` (relay URL, AES-256-GCM, key in fragment, relay sees metadata, no shipped self-host relay). Answers the Round 2 question "what metadata the relay sees": room id (16 bytes), peer ids, frame sizes and timing, join and leave events, and the connection IPs; not content. "How guest prompts are checked against host policy": a guest prompt becomes `promptCustomMessage` on the host session, so it runs under the host's approval mode (I traced the entry point, not the approval wiring).
- For rizzy-harness: a master-relayed shared view could copy "snapshot then entries over the same fold" (the guest ingests the host's entries verbatim via `ingestReplicatedEntry`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L3554-L3560). End-to-end sealing conflicts with a master that must read the log; decide that up front.
- Confidence: traced the host side end to end; the real relay is not in the repository, so its behaviour is read only from the stand-in. (checked in review)

#### Auth broker: one static bearer per broker, loopback default, token compared with a set lookup
- What the code does: `omp auth-broker serve` opens the local SQLite credential store, ensures a bearer token file (mode 0600, created if absent), and runs `startAuthBroker` on `127.0.0.1:8765` by default (`--bind` can change it; the code does not force loopback). Every route except `GET /v1/healthz` requires `Authorization: Bearer <token>`; the check is `Set.has` on the presented string. There are no per-client tokens, scopes or expiry on the broker bearer. Routes include `/v1/snapshot` (and an SSE stream), `/v1/usage*`, `/v1/credential` (POST, upsert, including real refresh tokens), `/v1/credential/:id/refresh`, disable and block routes. Snapshots replace OAuth refresh tokens with a sentinel; clients receive access tokens. A client caches the snapshot on disk, AES-GCM encrypted with a key derived from the bearer token and broker URL.
- Trace:
  - Server and auth check: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L58-L70, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L109-L116, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L810-L850, credential upload route https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L1054-L1078.
  - Serve command, token file, default bind: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-broker-cli.ts#L95-L160; default bind constant: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/types.ts#L178-L178.
  - Sentinel for refresh tokens: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/wire-schemas.ts#L55-L82.
  - Snapshot cache encryption: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/snapshot-cache.ts#L1-L12, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/snapshot-cache.ts#L348-L350.
- Edge cases and failure handling: unauthorised requests are logged with peer and route; `X-Forwarded-*` headers are honoured only with `--trust-proxy-headers`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L842-L850, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L62-L63. `isAuthorized` returns true when the token set is empty (the CLI always passes one token): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L109-L112.
- Round 1: `corrected`/confirmed: loopback default, bearer, plain HTTP, health route unauthenticated and access tokens to clients are all confirmed. Round 2's question "how the broker scopes and expires tokens it hands out": it does not scope or expire the bearer; access-token expiry is the provider's, and the broker refreshes in the background (default 5 minutes ahead, every 60 seconds): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L58-L80 (options) and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/types.ts#L180-L185.
- For rizzy-harness: our master/node link should use a per-node credential that can be revoked, not one shared bearer; compare with a constant-time function.
- Confidence: traced end to end for the server; client behaviour read from the cache file only. (checked in review)

#### Auth gateway: the broker's less-trusted sibling, with scoped clients stored as SHA-256 digests
- What the code does: `omp auth-gateway` is a forward proxy that accepts OpenAI chat, Anthropic messages and OpenAI Responses requests (plus images, speech and transcription) on `127.0.0.1:4000` and calls providers with broker credentials, so the caller never holds a key. It supports one gateway bearer, or scoped "clients" configured only by the SHA-256 digest of their token, each limited to a set of `provider/id` models and to named OAuth accounts per provider (restricted via a per-client session so no fallback to a sibling account). `--no-auth` allows unauthenticated calls.
- Trace:
  - Route list: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/server.ts#L1-L34.
  - Client scoping: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/clients.ts#L1-L60; digest: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/clients.ts#L126-L128.
  - Auth decision: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/server.ts#L1025-L1056; options: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/types.ts#L148-L162.
  - CLI `--no-auth` message: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-gateway-cli.ts#L455-L465.
- Edge cases and failure handling: a reload of the client table swaps scopes; requests already admitted keep the old restriction until they finish: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/types.ts#L166-L175.
- Round 1: `corrected`: Round 1 mentions only "its own inbound bearer"; scoped clients with digests, model and account allow-lists are `new (not documented)`. I did not find a token expiry.
- For rizzy-harness: per-client scope (models, accounts) with digest-stored tokens is a good model for a master that issues restricted keys to nodes.
- Confidence: read the definitions; routing logic after the auth check not traced. (checked in review)

#### Other servers: the stats dashboard, browser relay, LSP mux and `ovnfs` client
- What the code does:
  - `omp stats` serves a dashboard with a `--host` flag that defaults to `127.0.0.1` from a SQLite file `~/.omp/stats.db` built by parsing the session JSONL files incrementally (offset per file).
  - The browser relay server binds `127.0.0.1`, rejects any WebSocket `Origin` on the CDP side (so a web page cannot drive it), requires a `chrome-extension://` origin on the extension side and compares an optional `?token=` query parameter with `!==`; if no token is configured the check is skipped.
  - The LSP mux is a local IPC server (idle exit 15 minutes) that shares language servers across sessions.
  - `ovnfs` is an external daemon; omp only holds a client for its admin socket (u32-length-framed JSON, protocol version 5) found by running `ovnfs --json status`; no code for the daemon is in this repository and the client does no authentication beyond the socket path.
- Trace:
  - Stats flags: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/commands/stats.ts#L15-L25; server bind: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/stats/src/server.ts#L437-L452; schema with offsets: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/stats/src/db.ts#L235-L241; db path: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/utils/src/dirs.ts#L925-L928.
  - Browser relay: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/browser/relay/server.ts#L130-L160, default URL https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/browser/relay/kind.ts#L18-L18.
  - LSP mux constants: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/lsp/mux/server.ts#L18-L20.
  - ovnfs client: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/ovnfs/client.ts#L1-L47, daemon discovery: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/ovnfs/daemon.ts#L1-L20.
- Edge cases and failure handling: the relay token travels in a URL query, which would be a secret in a URL (local-only here). The stats sync runs in a child process because `bun:sqlite` is synchronous and froze the TUI on large databases: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/stats/activity-protocol.ts#L1-L12.
- Round 1: `confirmed` (browser relay on 127.0.0.1:9224 with optional token; stats dashboard); answers "where `omp stats` stores data": `~/.omp/stats.db`, derived from the session logs. "Daemon in release notes" (`ovnfs`): an external binary, client only.
- For rizzy-harness: the stats store is a rebuildable index over the log (offset per file); our event log in SQLite makes that a query instead. Never put a token in a URL.
- Confidence: read the definitions. (checked in review)

### MCP as a client

#### Three transports, a deliberately minimal client capability set, and server requests limited to `ping` and `roots/list`
- What the code does: server config `type` is `stdio` (default), `http` (Streamable HTTP) or `sse` (legacy). The `initialize` request advertises only `roots` capability (`listChanged: false`). Server-to-client requests other than `ping` and `roots/list` get `-32601`, so sampling and elicitation are not supported. stdio children inherit the full parent environment plus the configured `env`. Per-request timeout is 30 s (`OMP_MCP_TIMEOUT_MS`, 0 disables). HTTP transports never let configured headers override generated protocol headers and, for origin-locked servers, never forward configured headers across origins (redirects followed manually).
- Trace:
  - Transport switch: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/client.ts#L72-L87; initialize params: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/client.ts#L96-L112.
  - Request handler: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/client.ts#L40-L66; manager copy: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/manager.ts#L1362-L1383.
  - stdio environment: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/transports/stdio.ts#L593-L596.
  - Timeouts: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/timeout.ts#L1-L25.
  - Header policy: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/transports/header-policy.ts#L1-L50.
- Edge cases and failure handling: reconnect uses a ladder of 500/1000/2000/4000 ms, then retries at 15 s up to 5 minutes for network transports, and stops for stdio; five crashes in 30 seconds suspend automatic reconnects until `/mcp reconnect`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/manager.ts#L179-L190, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/manager.ts#L154-L155, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/manager.ts#L1768-L1802.
- Round 1: `confirmed` (stdio, Streamable HTTP, SSE); `new (not documented)`: no sampling or elicitation, full-env inheritance for stdio servers.
- For rizzy-harness: scrub the environment for stdio MCP servers on the node (omp does not); refuse server-initiated sampling by default.
- Confidence: read the definitions. (checked in review)

#### Tool lists: cached definitions, a 250 ms startup window, deferred wrappers, and `xd://` instead of schemas in every request
- What the code does: at startup omp waits at most 250 ms (`mcp.startupTimeoutMs`, env `OMP_MCP_STARTUP_TIMEOUT_MS`) for servers to connect; servers still pending are served from a per-server tool cache in `agent.db` (key `mcp_tools:<server>`, 30-day TTL, invalidated by a hash of the server config) as `DeferredMCPTool` wrappers whose connection resolves lazily; real tools replace them when the connection completes. Tool arrays are sorted by name so provider prompt caches stay valid regardless of connect order. MCP tool names are `mcp__<server>_<tool>` capped at 64 characters with a hash suffix. All MCP tools are declared with approval tier `write`, regardless of MCP annotations (the read-only hint is used only for prewalk). With `tools.xdev` on (default true), discoverable tools, including MCP and extension tools, are unmounted from the request's tool array and exposed as `xd://` devices driven through `read`/`write`; the default prompt docs mode is `catalog` (names only, docs fetched on demand).
- Trace:
  - Startup window and cache fallback: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/manager.ts#L1112-L1170; timeouts: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/timeout.ts#L4-L6.
  - Cache: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/tool-cache.ts#L10-L15, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/tool-cache.ts#L26-L60.
  - Sorting: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/manager.ts#L224-L227.
  - Name minting and cap: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/tool-bridge.ts#L522-L542.
  - Approval tier and read-only hint: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/tool-bridge.ts#L733-L735 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/tool-bridge.ts#L893-L893.
  - `xd://` design: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/xdev.ts#L1-L28; essentials that stay top-level: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/essential-tools.ts#L22-L36; defaults: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L1014-L1062 (`tools.xdev` default true, `tools.xdevDocs` default `catalog`).
- Edge cases and failure handling: a slow server no longer blocks startup (issue noted in the comment at manager.ts L1130-L1135); a tool call on a deferred wrapper waits for the connection with the request timeout.
- Round 1: `confirmed` (`xd://` catalog, `mcp__<server>_<tool>` names, write-tier approval); `new (not documented)`: 250 ms window plus 30-day cache, name sorting for cache stability, no search tool: omp limits context cost by moving schemas behind `xd://`, not by a search tool.
- For rizzy-harness: copy the cached tool list keyed by a config hash and the stable sort. The `xd://` trick (read lists, write executes) is clever but makes `read`/`write` approval tiers carry tool calls; check that our approval keys survive it.
- Confidence: read the definitions; traced the startup path.

#### OAuth for MCP: PKCE with dynamic client registration, credentials keyed by profile and server URL
- What the code does: `MCPOAuthFlow` extends a callback-server flow: authorization code with PKCE `S256`, a loopback callback listener (default port 3000, HTTPS loopback redirect allowed only with a separate callback port), dynamic client registration when no `client_id` is pinned. Credentials are stored in the auth store as `mcp_oauth:profile:<profile>:<serverUrl>` (URL used verbatim). The broker refreshes these rows with a generic refresh-token grant. `!command` header values are resolved through a separate config path.
- Trace:
  - Credential id scheme: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/oauth-flow.ts#L20-L35.
  - Flow, PKCE, registration: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/oauth-flow.ts#L345-L350, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/oauth-flow.ts#L477-L482, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/oauth-flow.ts#L612-L630.
  - Broker-side refresh of MCP rows: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-broker-cli.ts#L101-L123.
- Edge cases and failure handling: the manager stores the original config, not the resolved tokens, so rotating credentials are not cached into the tool-cache hash: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/manager.ts#L1010-L1016 region ("Store original config (without resolved tokens)"; I read these lines but did not take the exact range).
- Round 1: `confirmed` (OAuth per profile+URL).
- For rizzy-harness: key MCP credentials by server URL and user, never by name alone; the node should hold tokens, the master should not.
- Confidence: read the definitions; flow not traced to token storage.

#### Project MCP config is loaded without a trust prompt, and other harnesses' MCP files are read
- What the code does: `mcp.enableProjectConfig` defaults to true, so project MCP files start servers (stdio commands) when a session opens: `.claude/.mcp.json` and `.claude/mcp.json` (Claude provider), root `mcp.json` and `.mcp.json` (`discovery/mcp-json.ts` L171), `.omp/mcp.json` (`discovery/builtin.ts` L238-L239), plus `.cursor/` and `.vscode/` files. Servers defined for Claude Code, Codex, VS Code, Windsurf and others are merged through the capability registry; the first definition of a name wins.
- Trace:
  - Default true: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/settings.ts#L9-L19.
  - Claude provider file locations: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L92-L96.
  - Dedupe, first wins by priority: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/capability/index.ts#L285-L335.
- Edge cases and failure handling: none found: the `isProjectTrusted` hook handed to extensions is hard-coded to `() => true` in both construction sites: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L1814-L1814, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/agent-session.ts#L9771-L9771.
- Round 1: `confirmed` (no trust prompt documented); `new (not documented)`: the extension API exposes `isProjectTrusted()` but it always returns true.
- For rizzy-harness: project config that starts processes needs a per-project trust decision stored on the node; omp has none.
- Confidence: read the definitions. (checked in review) (corrected in review: the cited `claude.ts` lines are the `.claude/` paths, not the project root; root files come from a different provider)

### Skills, plugins and extension loading

#### Capability registry: many harnesses' directories are read through one prioritised provider mechanism
- What the code does: each content kind (skills, rules, slash commands, hooks, custom tools, MCP servers, context files, settings, system prompts, extension modules) is a "capability"; each source (native `.omp`, Claude, Codex, Gemini, OpenCode, Cline, Cursor, Windsurf, VS Code, GitHub, `.agents`, plugins) registers a provider with a priority. Loading merges all enabled providers, highest priority first, and dedupes by key (first wins; shadowed items are marked). Claude's provider registers hooks from `.claude/hooks/pre|post/` and custom tools from `.claude/tools/`, at user and project level.
- Trace:
  - Provider registration and ordering: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/capability/index.ts#L157-L180; dedupe: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/capability/index.ts#L285-L335.
  - Claude provider registrations for hooks and custom tools: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L618-L632 (the Codex provider registers the same capability kinds; I listed them with grep, not read them).
  - Claude hook loading: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L377-L422.
- Edge cases and failure handling: users can disable providers and user-level foreign sources: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/capability/index.ts#L408-L437.
- Round 1: `corrected`: Round 1 says hooks are in `.omp/hooks/pre|post/` or the user dir. The code also loads TS/JS hook modules from `.claude/hooks`, `.codex` and plugins' hook directories through the same capability, and they become executable extension modules (next finding).
- For rizzy-harness: reading other harnesses' directories is a large attack surface; omp reads executable hooks and tools from them, we should restrict foreign discovery to data (rules, skills text, MCP definitions) and gate anything executable.
- Confidence: read the definitions.

#### Extension modules: executable TS/JS, auto-discovered from native `.omp` only, but hooks from foreign dirs join them
- What the code does: extension modules are TS/JS files whose factory receives an `ExtensionAPI`. Auto-discovery of the `extension-module` capability is filtered to the `native` provider (project `<cwd>/.omp/extensions`, user agent dir, settings entries). The loader then appends every hook-capability file that is a TS/JS file (from any provider), installed plugin entries (`omp.extensions`/`pi.extensions` in a package manifest), and configured paths (`--extension`, settings). An extension can register tools, slash commands, shortcuts, CLI flags, assistant text and thinking renderers, transcript presenters, link schemes, composer shapes, model providers, and file-write and file-delete fallback handlers (consulted when a native write fails with EPERM/EACCES/EROFS); it can subscribe to lifecycle events including `tool_call`, `context`, `before_provider_request`, `session_before_tree`, `session.compacting`, `session_stop`.
- Trace:
  - Discovery order and native-only filter: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/loader.ts#L725-L795.
  - Hook files joined as extensions: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/loader.ts#L778-L790.
  - API surface: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/types.ts#L1990-L2123 (file fallbacks, commands, shortcuts, flags, renderers, link scheme, composer shape), https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/types.ts#L2238-L2246 (providers), https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/types.ts#L1875-L1920 (events).
- Edge cases and failure handling: handler timeout is 30 s: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L149-L149. A throwing `tool_call` handler blocks the tool ("Extension failed, blocking execution"): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L293-L340. Hooks cannot widen approvals: a `block` result throws before the tool runs, and approval is evaluated after hooks in the same wrapper. Load errors are collected per path without aborting other modules (documented in `docs/extension-loading.md`; the loader returns per-path errors).
- Round 1: `confirmed` (factory API, `omp.extensions` with legacy `pi`, hooks fail closed, 30 s default); `corrected`: foreign hook directories feed the same loader; Round 1 states the extension dirs are `.omp` only, true only for the `extension-module` capability.
- For rizzy-harness: `registerFileWriteFallback` is a notable design: a sandboxed host can broker writes the sandbox denies, with the symlink-resolved destination handed to the handler.
- Confidence: traced end to end for discovery; API read from the type definitions.

#### Plugins and marketplaces: git or npm sources, no signing, `notify` auto-update is silent
- What the code does: plugins install via `bun install` into `~/.omp/agent/plugins` (package name validated by regex and metacharacter ban), via local directory link, or from a marketplace catalog (git sources: `github`, `url`, `git-subdir`; npm sources in a catalog entry throw "not yet supported"). A marketplace entry may pin a `sha`; subdirectory paths that escape the clone are refused. `marketplace.autoUpdate` default is `notify`: it refreshes catalogs and checks for updates at startup, then only writes a debug log line; with `auto` it upgrades all plugins without prompting; errors are swallowed.
- Trace:
  - Installer and name validation: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/plugins/installer.ts#L8-L48; Bun CLI re-entry: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/plugins/bun-cli.ts#L11-L36.
  - Catalog sources and escape check: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/plugins/marketplace/source-resolver.ts#L60-L80, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/plugins/marketplace/source-resolver.ts#L180-L212.
  - Auto-update: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/plugins/marketplace-auto-update.ts#L10-L50; default `notify`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/settings.ts#L1330-L1345.
- Edge cases and failure handling: update failure (offline, corrupt) is silently ignored (`catch {}`) in the auto-update task.
- Round 1: `confirmed` (notify only writes debug log; `auto` ships code without approval; no signing); I found no integrity or signature verification other than an optional `sha`.
- For rizzy-harness: if we ever load third-party extensions, make updates visible and pin by hash.
- Confidence: read the definitions. (checked in review)

### Accounts, multi-user, server authentication

- Not in code for the product: there are no user accounts, roles or per-user sessions. Authentication exists only on the satellite services (auth broker bearer, gateway bearer or scoped clients, browser relay token, collab write token). Searched `account`, `user`/`login` in the session and RPC code; the `login` RPC command is provider OAuth login, not a user account (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-types.ts#L148-L151).
- Round 1: `confirmed` ("no org policy, SSO").
- For rizzy-harness: this is where we differ by design (private chats per owner); nothing to copy.
- Confidence: negative result from grep. (checked in review)

### Memory back ends

#### One interface, five mutually exclusive back ends, default off
- What the code does: `memory.backend` is `off` (default), `local`, `hindsight`, `mnemopi` or `sharpshooter`. `resolveMemoryBackend` returns exactly one. The `MemoryBackend` interface has `start` (non-throwing), `buildDeveloperInstructions` (markdown appended to the system prompt on rebuild), `clear`, `enqueue`, and optional `status`, `search`, `save`, `stats`, `diagnose`, `queuePreview`, `beforeAgentStartPrompt` (staged recall for the first turn with a commit step), `preCompactionContext` (text added to the compaction prompt) and `applySettings`. RPC and ACP hosts start with memory off.
- Trace:
  - Ids and interface: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/memory-backend/types.ts#L16-L16, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/memory-backend/types.ts#L100-L195.
  - Selection: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/memory-backend/resolve.ts#L14-L26.
  - Default off, protocol default: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/memory-backend/settings.ts#L11-L21.
- Edge cases and failure handling: backends "MUST be non-throwing" in `start`; recall that fails returns `undefined`, not an empty success, so a failed recall is not cached as empty: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/memory-backend/types.ts#L134-L139.
- Round 1: `corrected`: Round 1 lists three back ends (local, mnemopi, hindsight); there is a fourth, `sharpshooter` (next finding).
- For rizzy-harness: an interface with "prompt injection point", "pre-compaction hook" and "explicit search/save" is a good minimal surface; the commit-after-delivery step on recall avoids recording a recall the model never saw.
- Confidence: read the definitions.

#### Hindsight is a remote HTTP back end with an on-disk outbox; Mnemopi is local SQLite; both redact credentials before writing
- What the code does:
  - Hindsight: a hand-written fetch client for a Hindsight server (default `http://localhost:8888`, optional bearer API token); retains the conversation every 3 turns by default (Mnemopi: 4). Retain events are published to a file outbox, replayed in batches of 16, quarantined after 7 days undelivered, quarantine kept 30 days, at most 1000 undelivered events per destination (writes fail rather than evict).
  - Mnemopi: a local SQLite engine (`@oh-my-pi/pi-mnemopi`) with working and episodic memory, optional local ONNX embeddings or an OpenAI-compatible endpoint; LLM use is host-backed or remote.
  - The local, Hindsight, Mnemopi and Sharpshooter write paths all call `redactMemorySecrets` (grep: memories/index.ts, hindsight/outbox.ts, mnemopi/state.ts, sharpshooter/consolidate.ts), which removes provider token prefixes and credential-shaped runs after keywords like `secret`, `token`, `key`.
  - Writes are automatic (extraction by a model, or retain on a turn counter); no human approval step exists in the interface.
- Trace:
  - Hindsight client and auth header: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/hindsight/client.ts#L1-L47, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/hindsight/client.ts#L290-L293; URL default: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/hindsight/settings.ts#L17-L20; retain interval: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/hindsight/settings.ts#L173-L178; Mnemopi: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mnemopi/settings.ts#L282-L282.
  - Outbox limits: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/hindsight/outbox.ts#L9-L16; it also runs `redactMemorySecrets`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/hindsight/outbox.ts#L3-L3.
  - Redaction comment and list: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/memory-backend/redact.ts#L1-L25.
- Edge cases and failure handling: "a stored credential reaches every provider on every later turn" is the stated reason for redacting at write; redaction is pattern-based, so unknown secret formats pass.
- Round 1: `confirmed` (hindsight default `localhost:8888`, retain every 3 turns, mnemopi every 4); `new (not documented)`: outbox with TTL and quarantine, and shared redaction.
- For rizzy-harness: copy "redact on write" and the durable outbox for an unreachable memory server; add a human-approval queue, which omp does not have.
- Confidence: read the definitions. (checked in review)

#### Sharpshooter: a friction-gated project-decision memory
- What the code does: per user prompt, an extractor model emits deltas (architecture, product, style, constraint, rejected approach, correction) into a per-session queue; every 5 minutes a consolidation pass applies queued deltas per project into three markdown files (`architecture.md`, `product.md`, `style.md`) under `<agentDir>/memories/sharpshooter/<project bank>/`. A delta carries evidence that must be an exact substring of the user's prompt (enforced by the host), and consolidation admits a decision only when its lineage shows friction (user corrected, regression, or a subtle rule). Consolidation is single-writer through a lock file.
- Trace:
  - Types and gate: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sharpshooter/types.ts#L1-L60.
  - Storage layout and lock: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sharpshooter/paths.ts#L1-L52.
  - Extractor with schema validation: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sharpshooter/extract.ts#L1-L45.
- Edge cases and failure handling: not traced (consolidation prompt not read).
- Round 1: `new (not documented)`.
- For rizzy-harness: "evidence must be a literal substring of the user's message" is a cheap anti-hallucination check for any auto-memory.
- Confidence: read the definitions.

### Stats and usage store

#### Usage is journaled in the session log; the stats database is a derived index
- What the code does: model calls that are not part of the conversation (titles, memory, summaries) are recorded as `model_usage` entries, off-branch when needed; late usage and the usage of a discarded turn are re-journaled so totals do not lose them. `omp stats` parses all session logs incrementally (per-file offset and parser state) into SQLite (`messages`, `user_messages`, hourly rollups) and prices tokens with the public rate card (subscription models are shown as API-equivalent estimates; unpriced requests are counted separately).
- Trace:
  - `model_usage` entry: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L118-L135; appended on the initiating branch: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L3630-L3655; discarded-turn usage: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4089-L4108.
  - Stats schema and offsets: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/stats/src/db.ts#L235-L241; unpriced rule: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/stats/src/db.ts#L56-L63; parser consumes `model_usage`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/stats/src/parser.ts#L92-L92.
- Edge cases and failure handling: removing an assistant turn from the active path re-journals its billed usage so `omp stats` still counts it.
- Round 1: `confirmed` (local usage dashboard); `new (not documented)`: derived from session logs, `model_usage` entries.
- For rizzy-harness: usage as a first-class event with attribution to the branch that caused it, and any dashboard as a rebuildable fold, matches our event-sourcing direction.
- Confidence: read the definitions.

## 4. Round 1 claims checked

| Claim | Verdict | Citation |
|---|---|---|
| Session JSONL, version 3, optional 256-byte title record, one header | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L17-L35 |
| Entries have id/parentId; branching makes siblings in the same file | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4048-L4055 |
| `/branch` rewinds, `/fork` clones the whole session to a new file | confirmed, with a detail: `/branch` also creates a new file | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4144-L4226 |
| Checkpoint/rewind tools off by default; no file checkpoints or undo | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L747-L755, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/checkpoint.ts#L53-L131 |
| Large images in a content-addressed blob store | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/blob-store.ts#L7-L30 |
| RPC NDJSON, ready frame, protocol v1 1 MiB, v2 up to 64 MiB chunks | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-frame.ts#L5-L9 |
| SDK is Bun 1.3.14+ only | corrected: Bun 1.4.4 or newer | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/package.json#L563-L565 |
| MCP transports stdio, Streamable HTTP, legacy SSE | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/client.ts#L72-L87 |
| MCP tools named `mcp__<server>_<tool>`; approval write tier | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/tool-bridge.ts#L733-L735 |
| `xd://` on-demand tool catalog | confirmed (default on, docs mode `catalog`) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L1014-L1040 |
| Project MCP config and extensions load with no trust prompt | confirmed; API `isProjectTrusted` is hard-coded true | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L1814-L1814 |
| Hooks are trusted TS/JS; `tool_call` fails closed on error, 30 s default | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L293-L340 |
| Hooks only in `.omp/hooks/pre|post` or user dir | corrected: `.claude/hooks/pre|post` (and Codex, plugins) also feed the loader | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L377-L422 |
| Extension dirs `.omp` based | confirmed for `extension-module`; foreign hook modules still load | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/loader.ts#L760-L790 |
| Marketplace `autoUpdate` `notify` only logs at debug; `auto` upgrades unprompted | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/plugins/marketplace-auto-update.ts#L36-L48 |
| Memory back ends: local, mnemopi, hindsight | corrected: also `sharpshooter` | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/memory-backend/types.ts#L16-L16 |
| Hindsight retains every 3 user turns, mnemopi every 4 | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/hindsight/settings.ts#L173-L178, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mnemopi/settings.ts#L282-L282 |
| Auth broker: loopback default, bearer, plain HTTP, health route open, refresh tokens stay on broker | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L109-L116, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/wire-schemas.ts#L78-L82 |
| Auth gateway has its own inbound bearer | confirmed and extended: scoped clients by SHA-256 digest | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/clients.ts#L1-L30 |
| Collab: relay `wss://my.omp.sh`, AES-256-GCM, key in fragment, full and view links, no accounts | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/crypto.ts#L1-L10, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/wire/src/index.ts#L467-L467 |
| Browser relay on 127.0.0.1:9224, optional token | confirmed (token in query string, `!==` compare) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/browser/relay/server.ts#L130-L160 |
| `omp stats` local dashboard | confirmed; data in `~/.omp/stats.db` | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/utils/src/dirs.ts#L925-L928 |
| Scheduling: nothing documented | confirmed: no scheduler in code | search of `src` for cron and schedule |
| Default approval mode | confirmed `yolo` default | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L314-L318 |

## 5. In the code but not in the documentation

- `activeLeafId` on entries and the leaf-replay rule (off-branch appends do not move the resumed branch): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L98-L111.
- Ownership lease per session id and "move to a sibling file" when another live process owns the session; `continuationOf`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L1157-L1182.
- Pluggable `SessionStorage` with SQL and Redis implementations (SDK only): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/sql-session-storage.ts#L9-L60.
- Entry kinds missing from the Round 1 list: `model_usage`, `service_tier_change`, `thinking_level_change`, `title_change`, `session_init`, `mode_change`, `credential_pin` (an account hash recorded so a resumed session re-pins the same OAuth account for prompt-cache reuse), `reset_boundary`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L137-L330.
- Read-only seance fork of a session: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4271-L4285.
- Fourth memory back end `sharpshooter`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sharpshooter/types.ts#L1-L60.
- Gateway scoped clients with per-client model and account allow-lists: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/clients.ts#L1-L60.
- Model-dependent compaction replay (a compaction the active model cannot replay is skipped and history re-expanded): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L253-L268.
- Foreign transcripts importable and the capability registry reading Claude and Codex hook and tool directories as executable code: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L618-L632.
- MCP client advertises only `roots`; no sampling or elicitation; stdio servers inherit the full environment: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/client.ts#L96-L112, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/transports/stdio.ts#L593-L596.
- Hindsight outbox with TTL and quarantine: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/hindsight/outbox.ts#L9-L16.
- `checkpoint` tool summary string claims "git-based" state though no git is touched: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/checkpoint.ts#L57-L57.

## 6. Could not find or could not settle

- The hosted collab relay: its Go source is not in the repository. I read only the offline stand-in (`local-relay.ts`). What the real relay logs, how it limits room creation or rate, and whether a guest can claim `host` first on a known room id: unknown. Settling it needs the relay source or a test against `wss://my.omp.sh`.
- Whether a view-only guest holding the room key could forge frames: guest frames are forwarded only to the host and the host checks `canWrite` by the relay-assigned peer id, so I infer not; I did not trace the guest receive path in full.
- How the host's approval mode applies to a guest prompt: the prompt becomes a host-side custom message (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/host.ts#L1140-L1160 region), I did not trace approvals for the resulting tool calls.
- Whether `/tree` navigation without a later write is persisted: inferred non-durable from comments and `resolveSessionLeafId`, not tested. A test would branch, exit and resume.
- Post-compaction entry emission in `buildSessionContext` after line 668, and the compaction prompts themselves (agents cluster).
- MCP OAuth token persistence details (agent.db rows) and the `!command` header resolution path; only the credential id scheme and flow class were read.
- A file-level undo of any kind: searched `undo`, `revert`, `rollback`, `shadow`, `snapshot` and found none, but I did not read the Rust crates (`pi-edit`, `pi-vcs`) in full.
- Whether SQL/Redis storage is reachable from any CLI flag: grep found only the index exports.

## 7. Citation check

research/round2/oh-my-pi.sessions.md: 272 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit

## 8. Review

Independent review against the clone at the pinned SHA. Checked 17 of 34 findings (the ones about the session tree and leaf, checkpoints, approvals in headless mode, the broker and gateway, collab, MCP, project trust, plugins, retention and memory), plus the negative claims on SSH and file undo by search. Where a finding was checked only on its numbers or its central mechanism, the table says which.

| Finding | Verdict | Reason |
|---|---|---|
| Branch inside one log; leaf rebuilt from parents | supported | `session-entries.ts` L98-L111, L373-L397: the leaf is not stored; it is the last physical entry unless that entry carries `activeLeafId` (set only on off-branch appends, `session-manager.ts` L3595-L3628); `branch()` only calls `#setLeaf` in memory (L4048-L4060, L2149) |
| `/tree` leaf-only move is no-op | partly | the no-op is only for target == current leaf (`agent-session.ts` L14765-L14777); wording fixed |
| `checkpoint`/`rewind` restore no files | supported | `checkpoint.ts` L53-L131 only returns text; `#applyRewind` (`agent-session.ts` L13006-L13030) calls `branchWithSummary`; `checkpoint.enabled` default false; summary string says "git-based" |
| RPC mode | supported | `rpc-frame.ts` L5-L16: 1 MiB frame, 64 MiB reassembled, 256 KiB chunks |
| Headless: fail closed, default yolo | supported | `wrapper.ts` L446-L471; note a `/loop` repetition feature exists (`modes/loop-limit.ts`, `loop-condition.ts`) but no cron-style scheduler found |
| No node/worker split; SSH file access only | supported | only file transfer and host probes use `buildRemoteCommand`; no `ssh` tool in `tools/` |
| Collab host side | supported (token and write gate only) | `host.ts` L854-L861 timing-safe write token, L1127-L1135 read-only peers rejected; sealing and relay details not re-read |
| Auth broker | supported | `types.ts` L178 default bind, `server.ts` L109-L116 `Set.has` and empty-set open, L829-L849 healthz open, L1054-L1078 upsert; no loopback enforcement on `--bind`. Clients receive API keys and OAuth `access`; only `refresh` is replaced (`pool.ts` L715-L735) |
| Auth gateway | supported | `clients.ts` header: SHA-256 digest clients with model and account scopes; key resolved server side in `dispatch.ts` L128-L141 |
| Other servers (browser relay) | supported (relay only) | `relay/server.ts` L128-L162: any `Origin` rejected on `/cdp`, `chrome-extension://` required on `/ext`, token compared with `!==` only if configured |
| MCP client capabilities and env | supported | `client.ts` L40-L66, L96-L112 roots only, `-32601` otherwise; `stdio.ts` L593-L596 `{...Bun.env, ...config.env}`; default timeout 30 s, startup window 250 ms (`timeout.ts`) |
| Project MCP config without a prompt | partly | default true confirmed (`mcp/settings.ts` L9-L12); the file locations were attributed to the Claude provider's project root, but the cited lines are the `.claude/` paths; fixed. `isProjectTrusted` hard-coded `true` confirmed at both sites |
| Plugins and marketplaces | supported | `marketplace-auto-update.ts` L36-L48: `notify` logs at debug, `auto` upgrades all, errors swallowed; default `notify` |
| Retention | supported | `gc-settings.ts` defaults match; `gc` is an explicit command (`gc.autoCompress` only compresses, default false) |
| Hindsight / Mnemopi | supported (numbers) | retain every 3 / 4 turns; outbox batch 16, 7-day TTL, 30-day quarantine, 1000 events; redaction call sites not re-read |
| SDK in-process, Bun 1.4.4 | supported | `package.json` L563-L565 |
| Extension modules, foreign hook files | supported (hook joining only) | `loader.ts` L774-L786 imports every hook-capability path that is `.ts`/`.js` |

Not checked: session file format and title slot, id generation, compaction replay, location and cwd encoding, ownership lease, 500 000-character cap, pluggable storage, `/branch` vs `/fork`, resume, export and share, ACP, RPC host tools, the tool-list cache, MCP OAuth, sharpshooter, stats, foreign-session import and the Round 1 table rows not listed above.

Overall: the claims the brief singled out all hold. The active leaf is inferred from the last journal entry except for off-branch writes, the checkpoint tool restores no files, broker clients get API keys and access tokens while the gateway client gets none, and MCP stdio servers inherit the whole environment. Both errors found were location or scope wording, not mechanism. The unchecked half is mostly storage plumbing with line-level citations and explicit "inferred" labels where the author was unsure, so I would trust it at about the level of the checked half, but treat the collab relay behaviour (read only from a stand-in) and the not-traced approval path for guest prompts as open, as the file itself says.
