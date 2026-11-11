# Hermes Agent: sessions, remote operation and extensibility (Round 2, source reading)

## 1. Repository

- Repository: `NousResearch/hermes-agent`, branch `main`, pinned SHA `95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02`, commit date 2026-11-10 (clone at `.claude/research-repos/hermes-agent`, 401 MB).
- Cluster: sessions (session store, checkpoints, client/server split, remote workers, headless and scheduled operation, MCP client, extension loading, accounts).
- What I read: `hermes_state_*.py` (SQLite store: schema, FTS, search, WAL, runtime admissions, branch, compress, rewind), `hermes_cli/sessions_cmd.py`, `hermes_cli/main.py` (resume), `tools/checkpoint_manager.py` and its callers in `agent/`, `gateway/` (`delivery_ledger.py`, `platforms/base.py`, `session_authority.py`, `session_unattended_unknown.py`, `control_socket.py`, `runtime_bootstrap.py`, `relay/`, `owner_identity.py`, `platforms/api_server.py`), `cron/` (tick, authority, script gate, prompt scan), `hermes_cli/kanban_*`, `tools/mcp_tool_*.py` and `tools/tool_search.py`, `acp_adapter/`, `hermes_cli/dashboard_auth`, `plugins/dashboard_auth`, `tools/plugin_guard.py`, `tools/skills_guard.py`.
- What I did not read: the connector side of the relay (a separate Node repository, `NousResearch/gateway-gateway`, named in `gateway/relay/__init__.py` docs, not cloned); the Kanban worker's own agent loop; the 28 platform adapters except `base.py`; skills install/curator code beyond the scanner verdict; the TUI/desktop clients.
- Scale note: this is a very large Python code base (hundreds of `hermes_state_*`, `gateway/*`, `cron/*` modules). Many comments cite issue numbers; the code is shaped by a long list of production incidents. Everything below is Python; there is no compiled component in what I read.

## 2. Architecture as it bears on this cluster

- **One SQLite file per profile, many processes.** `state.db` in `HERMES_HOME` holds sessions, messages, FTS indexes, the per-session input queue (`session_admissions`), worker executions, gateway routing, delivery obligations and more. The CLI, TUI, gateway, dashboard, ACP viewer and cron share it; correctness rests on SQLite transactions plus a `runtime_epoch` fence, not on a single process. Schema version 33 (`hermes_state_common.py`).
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L294-L295 - `SCHEMA_VERSION = 33`.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L467-L568 - the `sessions` and `messages` tables.
- **The gateway is the owner of execution; everything else is a viewer.** The CLI/TUI/Desktop/ACP/dashboard attach to the profile gateway over a local control socket (peer-uid authenticated) to mint a single-use 30 s ticket, then dial the gateway WebSocket with the ticket carried in a `Sec-WebSocket-Protocol` subprotocol. The gateway process runs the agent loop for those sessions, the cron ticker and the Kanban dispatcher.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/control_socket.py#L1-L9 - control-socket contract.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/session_authority.py#L1450-L1467 - boot: new runtime epoch, then lost turns fenced `unknown`.
- **Agent core is a Python class `AIAgent` (`run_agent.py`)**, also reached from `hermes chat -q`, batch runner and library use. The `safety` cluster file carries the loop detail; this file only covers persistence and transport around it.
- **Profiles are separate homes.** Each profile has its own `state.db`, `cron/jobs.json`, checkpoint store, kanban boards. One gateway process may multiplex several profiles using a context-local secret scope.

## 3. Findings

### Session storage

#### SQLite `state.db` is a relational transcript with soft-delete flags, not an append-only event log
- What the code does: `messages` rows carry `active`, `compacted`, `api_content`, `display_*`, `message_uid`, reasoning columns and a `platform_message_id`; `sessions` carries parent link, end reason, tokens/cost, cwd, git branch, title, archived/pinned flags, compression cooldown counters and a `transcript_revision` that a trigger bumps on any in-place row rewrite. Rows are updated and soft-deleted (`active = 0`) rather than only appended; there is no `seq`-contiguous event stream.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L467-L535 - `sessions` columns.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L537-L568 - `messages` columns incl. `active`, `compacted`, `message_uid`.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L793-L802 - triggers bump `transcript_revision` on in-place rewrites.
- Edge cases and failure handling: a derived-data poller keys on `transcript_revision` plus the active row count and highest id, so rewrites that change neither are still seen. A trigger refuses to delete a session that has runtime history when an older build is pointed at a newer database (downgrade guard): https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L786-L792.
- Round 1: `confirmed` ("SQLite `state.db` (WAL; FTS5) with full messages, tool calls, system prompt snapshot"); the system prompt is deduplicated in a `system_prompts` table keyed by hash (L462-L465), which Round 1 does not mention.
- For rizzy-harness: the opposite design (our contiguous-`seq` log, UI as a fold) is cleaner for one-writer-per-session; Hermes' own comments show what mutable rows cost (revision triggers, display-order triggers, repair paths). Keep the log append-only and derive any "rewind" view.
- Confidence: read the definition.

#### Journal mode: WAL is the default but is refused or avoided on three kinds of storage
- What the code does: `resolve_journal_mode()` defaults to `wal` (configurable to `delete`). `apply_wal_with_fallback` never enables WAL on a SQLite build with the known WAL-reset bug (new databases get DELETE mode), refuses to enable it on virtiofs/9p bind mounts (silent corruption), and never live-downgrades a database already in WAL. A 64 MiB `journal_size_limit` bounds the WAL file.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_wal.py#L157-L167 - default and fail-safe to `wal`.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_wal.py#L301-L304 - vulnerable SQLite goes to DELETE.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_wal.py#L331-L339 - cross-VM filesystem refusal.
- Edge cases and failure handling: if the on-disk mode cannot be read (locked), nothing is touched and the connection inherits the header's mode. Round 1 records the Docker virtiofs warning from the docs; the code enforces it.
- Round 1: `corrected` ("WAL" stated flatly): code comments say that on the bundled SQLite 3.50.4 there is no evidence WAL is safer and new databases are kept out of WAL while the bug exists (L270-L275 of the same file).
- For rizzy-harness: our SQLite on the master will sit on arbitrary volumes; add the same startup probe (filesystem type, journal mode read-back) and write the choice to the log.
- Confidence: traced end to end (checked in review).

#### Full-text search: external-content FTS5 over a stable projection view, plus a trigram index
- What the code does: `messages_fts` is an FTS5 table over columns `content, tool_name, tool_calls`, whose external content is the view `messages_fts_src`. The view truncates `role='tool'` rows to the first 8,192 characters, so the stored index and what FTS5's integrity-check reads back always agree (the comment says the earlier moving-boundary design caused repeated "checksum mismatch" corruption). A second table, `messages_fts_trigram`, indexes non-tool rows of non-cron, non-subagent sessions for substring and CJK matching. Triggers keep both in step with `messages`; they are gated by two `state_meta` markers (`fts_rebuild_high_water`, `fts_rebuild_progress`) so a chunked background rebuild never issues an FTS 'delete' for a row that is not in the index.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L318-L334 - layout version 3 and the 8,192-char tool-row prefix.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L956-L989 - view, virtual table, insert trigger with the rebuild predicate.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L1063-L1090 - trigram source view and table.
- Edge cases and failure handling: a full structural rebuild is guarded by a cross-process flock (`<db>.fts_rebuild.lock`), fail-closed, because two concurrent rebuilds "structurally corrupted state.db in production": https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L1231-L1249. A corrupt index puts the store in an `fts_stale` mode in which searches fall back to LIKE scans (see next finding).
- Round 1: `confirmed` (FTS5 over raw messages); new detail: tool output is indexed only as a bounded prefix, and search is a database feature with no LLM.
- For rizzy-harness: if we add search, index the event text, cap tool-result bytes in the index, and do not use triggers conditioned on mutable markers. The lesson is to make the indexed projection a pure function of the row.
- Confidence: read the definition.

#### Search routing: sanitised query, then FTS, trigram on a miss, OR-relaxed on a second miss, LIKE when stale
- What the code does: the query is length-capped, tokens are quoted to neutralise FTS5 syntax (keeping AND/OR/NOT), then `_search_messages_impl` routes: explicit tool-role search -> LIKE over canonical rows; stale index -> LIKE; CJK -> trigram or LIKE; otherwise `messages_fts`. A zero-result Latin query retries the trigram index, then retries with the terms OR-ed. Rewound rows (`active=0, compacted=0`) are excluded; compaction-archived rows are included. While a deferred rebuild is pending, a bounded LIKE scan supplements results for the unindexed id range.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_search.py#L188-L198 - token quoting.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_search.py#L1199-L1207 - tool-role and stale routes to LIKE.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_search.py#L1237-L1246 - unindexed-gap supplement.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_search.py#L1252-L1270 - trigram retry and OR-relaxed retry.
- Edge cases and failure handling: a `sqlite3.OperationalError` (syntax error despite sanitising) returns no results; a `DatabaseError` (corruption) detaches the derived indexes and answers from canonical rows. Slow searches (default 1 s) are logged with the route taken.
- Round 1: `confirmed` (raw FTS5 excerpts, no LLM, at the store level); the fallback ladder is new. I did not read `tools/session_search_tool.py`, so what the model-facing tool adds on top is not settled.
- For rizzy-harness: copy "never return an empty answer from a failing index": a canonical-table fallback path, tested.
- Confidence: traced end to end.

#### Resume resolves to the deepest descendant holding messages and restores the recorded cwd
- What the code does: `--resume latest`, title or id is normalised in `main.py`; the resolved session's recorded `cwd` is `chdir`-ed to (opt-out `--no-restore-cwd`; skipped under `--worktree`; a vanished directory only warns). `resolve_resume_session_id` follows the compression chain to its tip, then walks `parent_session_id` forward up to 32 levels to the deepest node with messages, skipping branch/delegate/reset/tool children, so delegate and branch children never hijack a resume.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/main.py#L1427-L1440 - `--in`, `latest`, `--continue` normalisation.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/main.py#L1464-L1478 - cwd restore.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_messages.py#L1468-L1509 - lineage-aware redirect.
- Edge cases and failure handling: depth cap 32 and a `seen` set stop cycles; on any query error the original id is returned. Resume re-reads current configuration, not the one from the original run (Round 1; not re-verified here).
- Round 1: `confirmed` (resume restores cwd).
- For rizzy-harness: store the cwd per session on the node and treat "directory gone" as a warning, not an error.
- Confidence: traced end to end.

#### Compaction defaults to in-place: the same session id, older rows soft-archived
- What the code does: with `compression.in_place: True` (the default) a compaction commit calls `archive_on_connection` on the same session id; older turns are kept as `active=0, compacted=1` and remain searchable. With `in_place: False` a new child session is published with `model_config._compressed_from`, and the parent ends with `end_reason='compression'`. The commit bumps `runtime_generation` so any stale worker is fenced.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L667-L671 - default and rationale ("session-rotation bug cluster").
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_mutation_compress.py#L12-L21 - in-place versus rotating branch.
- Edge cases and failure handling: resume code still understands the rotating chain (previous finding), so older sessions keep working.
- Round 1: `confirmed` (Round 1 listed the in-place versus child-session contradiction; code says in-place is default, the sessions-page sentence describes the legacy path).
- For rizzy-harness: matches our "compaction is a marker in the log"; the cost Hermes paid was a bug cluster from rotating ids, so never change the session id on compaction.
- Confidence: read the definition (checked in review).

#### Branch (fork) copies active rows up to a boundary inside one transaction, with a fresh frozen policy
- What the code does: `branch_in_transaction` requires the parent to be idle, derives a child id from `(profile, principal, request_id)`, publishes a child `sessions` row with `parent_session_id` and `model_config._branched_from`, clones the active message rows up to `through_message_id` (extended over the tool results that answer a boundary assistant message), writes a routing row and a policy receipt, and titles it `"<title> #N"` unless the caller names it. The child policy is the parent's, except edit approval resets to `ask` and editor MCP servers are rebound under the child's own scope.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_mutation_branch.py#L12-L31 - boundary selection.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_mutation_branch.py#L34-L49 - child policy: approval reset, MCP rebinding.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_mutation_branch.py#L52-L104 - transaction body.
- Edge cases and failure handling: a missing policy receipt raises `runtime_coordination_required`; a mismatch between the prepared child and the derived id raises `revision_conflict`. Branch is the same code ACP `session/fork` calls (see ACP finding).
- Round 1: `confirmed` (fork and branch exist); the "approval resets to Default on a branch" rule is new.
- For rizzy-harness: a fork should not inherit approvals or live connections; copy the "child starts at ask" rule.
- Confidence: traced end to end.

#### Rewind (`/undo`, `/retry`) is a soft-delete of the active rows from a chosen user turn onward
- What the code does: `SessionRewindMixin` plans a rewind against the warm in-memory history and the durable rows (`RewindPlan` carries `expected_active_ids` and the expected content of the target row, so a history that changed since the plan is refused as `session history changed before the rewind could be persisted`), then sets the model-side rows to `active=0` in one write transaction. A refused (content-policy) turn is removed from the model context but kept visible via `compacted=1`.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_rewind.py#L64-L77 - `RewindPlan` with expected ids.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_rewind.py#L92-L93 - mixin contract.
- Edge cases and failure handling: rewind changes the conversation only; files are untouched (that is the checkpoint feature).
- Round 1: `confirmed` ("`/undo` rewinds the conversation only").
- For rizzy-harness: a compare-and-swap on the visible tail (expected ids) is a cheap guard for rewind racing a running turn.
- Confidence: read the definition.

#### Export and retention
- What the code does: `hermes sessions export` renders `jsonl`, `md`/`qmd`, `html` or `trace`. Exports include archived and pinned rows. A per-session `sessions.max_export_messages` guard runs before any rows are loaded into memory (0 disables). `trace` writes Claude Code JSONL with redaction on by default (`--no-redact` opts out) and refuses to export if redaction itself fails. Auto-prune of ended sessions is off by default (`retention_days: 90`); auto-archive (hide, never delete) is off by default; VACUUM after a prune only when more than 25% of the file is free.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/sessions_cmd.py#L372-L376 - export command.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/sessions_cmd.py#L408-L413 - size guard before load.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/sessions_cmd.py#L500-L503 - trace export, redaction default.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/sessions_cmd.py#L552-L554 - redaction failure aborts.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L2252-L2262 - retention defaults.
- Edge cases and failure handling: the `on_session_delete` hook fires after the delete commits and is fail-open (`hermes_state_common.py#L417-L451`).
- Round 1: `confirmed` (formats, redact, auto-prune off by default).
- For rizzy-harness: refuse an export that fails redaction rather than falling back to raw; do not delete by default.
- Confidence: read the definition.

### Checkpoints and undo of files

#### Shadow-git store: one shared bare repository, one ref and one index per project, git isolated from user config
- What the code does: `CheckpointManager` keeps `~/.hermes/checkpoints/store/` as a single bare git repo with `refs/hermes/<hash16>` per working directory, a per-project index file under `store/indexes/` and a metadata JSON per project; objects are deduplicated across projects and worktrees. Every git call sets `GIT_DIR` to the store, `GIT_WORK_TREE` to the project, `GIT_INDEX_FILE` to the per-project index, and points `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` at `/dev/null` with `GIT_CONFIG_NOSYSTEM=1`, so user signing config, hooks configured globally and credential helpers cannot fire. The store's own config sets a fake identity and disables gpg signing and `gc.auto`. Nothing is written into the project directory.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L13-L39 - layout and rationale (pre-v2 per-project repos cost about 40 MB per worktree).
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L400-L438 - environment isolation.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L635-L656 - `git init --bare` and per-store config.
- Edge cases and failure handling: no `core.hooksPath` or `core.fsmonitor` override is set here (those overrides exist for other internal git calls in `hermes_cli/_subprocess_compat.py#L287-L300`), but the store is a fresh bare repo that Hermes configures itself and project-level config is not read because `GIT_DIR` is the store; I infer hooks and filters cannot run, but I did not find a test that proves it. `git` missing returns "not installed" and checkpoints silently do nothing.
- Round 1: `confirmed` (one shared bare repo, a ref per project); `new (not documented)`: environment isolation details.
- For rizzy-harness: the design matches our hardened shadow repo; also set `core.hooksPath=/dev/null` and `core.fsmonitor=false` explicitly rather than rely on the store being bare.
- Confidence: traced end to end (isolation claim for hooks and filters is inferred; checked in review: the environment variables and `GIT_DIR`/`GIT_WORK_TREE`/index redirection are as described, the hooks and filters inference is unchanged and still untested).

#### When snapshots are taken and what guards cost
- What the code does: before each `write_file` or `patch` call, and before a `terminal` call whose command matches a destructive regex (`rm`, `rmdir`, `cp`, `install`, `mv`, `sed -i`, `truncate`, `dd`, `shred`, `git reset|clean|checkout`, or an overwrite redirect `>`), the executor calls `ensure_checkpoint(dir, reason)`. A per-turn set makes it at most one snapshot per directory per turn. The directory must not be `/`, `$HOME` or any ancestor of `$HOME`; more than 50,000 candidate files (counted after pruning the default excludes) skips the snapshot; files over `max_file_size_mb` (10) are dropped from the index; container backends are excluded; the snapshot is skipped when nothing changed against the ref tip. Each take then trims to `max_snapshots` (20) and drops the oldest snapshot per project until the store is under `max_total_size_mb` (500). A background sweep (retention 7 days, orphans never deleted unattended) runs outside the startup path. Checkpoints are off by default (`checkpoints.enabled: False`).
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_executor.py#L1030-L1046 - trigger sites.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_dispatch_helpers.py#L52-L65 - destructive-command regex; `#L102-L104` the predicate.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1052-L1080 - `ensure_checkpoint`: broad-directory and per-turn dedup.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1557-L1571 - take: init, file-count guard.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L828-L875 - file counting and broad-directory test.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L485-L503 - defaults.
- Edge cases and failure handling: `ensure_checkpoint` never raises; every error is logged at debug. Because the destructive-command test is a regex, `rm` inside a shell wrapper such as `bash -c "..."` after a non-matching prefix, `find -delete`, `python -c` writes, or `tee` are not detected: a destructive terminal command that the regex misses gets no snapshot (read from the pattern list in review; not run). `git rm` is not in that list of misses: the pattern's `(?:^|\s|...)rm\s` alternative matches the ` rm ` inside `git rm file` (checked by applying the quoted regex to that string). The overwrite-redirect pattern `[^>]>[^>]` also matches any single `>`, including `2>&1`, so many harmless commands do trigger a snapshot (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_dispatch_helpers.py#L52-L65).
- Round 1: `confirmed` (triggers, one per directory per turn, caps 20/500 MB/10 MB/7 days, 50,000 files, container backends excluded); `corrected`: the "never snapshots `/` or `$HOME`" is also every ancestor of home.
- For rizzy-harness: snapshot decisions on the node must not depend on a command-text regex; we know the tool call, so snapshot before any tool in the "can write" class and before any shell call not provably read-only.
- Confidence: traced end to end (corrected in review: `git rm` was listed as undetected but the destructive-command regex does match it; the other misses, `bash -c "..."` after a non-matching prefix, `find -delete`, `python -c`, `tee`, hold).

#### Restore: pre-rollback snapshot, "safe" mode skips user edits using a write ledger, but the ledger only sees file-tool writes
- What the code does: `/rollback` restores through `CheckpointManager.restore(..., safe=not restore_all)`. A pre-rollback snapshot is taken first (so the restore is undoable). In safe mode the current tree is staged into the project index and diffed against the chosen commit; each changed path is restored only if its current SHA-256 equals the hash recorded when Hermes last wrote it (the agent-write ledger, capped at 2,000 entries per project), or the file is now missing; paths whose content differs, and paths Hermes never wrote, are listed in `skipped_user_edits`. Files created by Hermes and absent from the snapshot are deleted, except oversize files, which are kept because no snapshot holds them. If the ledger is empty it falls back to a full restore. The ledger is fed from `file_mutation_result_landed` for `write_file`/`patch` results only.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L958-L984 - `record_agent_write`.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L986-L1050 - `_safe_restore_plan`.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1387-L1414 - safe plan use and pre-rollback snapshot.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1429-L1452 - checkout versus delete targets, oversize kept.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/turn_explainers.py#L141-L148 - ledger fed from landed file mutations.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/slash_commands.py#L767 and https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/cli_commands_mixin.py#L478-L479 - callers pass `safe=` unless `--all` or a single file.
- Edge cases and failure handling: a whole-tree safe restore treats a path Hermes never wrote as user-owned and skips it. The ledger is fed only for `write_file` and `patch` results ([tool_result_classification.py#L10-L10](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_result_classification.py#L10-L10)), so a file that a shell command changed is skipped by the default restore when it is not in the ledger or its hash no longer matches the recorded one, even though the snapshot before the terminal command exists; the user has to restore it explicitly (`--all` or per file). Three exceptions, traced in review: (1) a file the agent first wrote with a file tool and later deleted with the shell is in the ledger, reads as missing, and is restored ([checkpoint_manager.py#L1041-L1047](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1041-L1047)); (2) if the agent has never used `write_file` or `patch` in that project the ledger is empty and the restore falls back to a full restore ([checkpoint_manager.py#L1387-L1405](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1387-L1405)), so the skipping needs at least one recorded file-tool write; (3) container-backend writes are not recorded at all (they have no host path). This follows from reading `_safe_restore_plan` and the single call site of `record_agent_write`; I did not run it.
- Round 1: `confirmed` ("skips files the user edited since unless `--all`", "pre-rollback snapshot"); the terminal-command gap is `new (not documented)`.
- For rizzy-harness: copy the ledger idea (hash after our own write), but record writes from every path including shell-detected changes (diff the tree before and after instead), or the safe mode silently protects the very files the agent damaged.
- Confidence: traced end to end for the call graph; the consequence is inferred (checked in review; the author's inference was right in the main case but needed three exceptions, see above) (corrected in review: file deleted by shell after a file-tool write is restored, and an empty ledger means a full restore).

#### Nested git repositories are stored as gitlinks and a checkpoint containing one refuses whole-tree rollback
- What the code does: after `write-tree`, `ls-tree -r` finds mode-160000 entries; the snapshot subject gets an "uncaptured" note listing them. At restore time, a checkpoint with any nested repo refuses to roll back (whole-tree, or a file whose path intersects one) with `nested_repositories` in the result.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1659-L1671 - note added at take time.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1376-L1385 - refusal at restore.
- Edge cases and failure handling: a file-scoped restore under a nested repo is also refused; a file elsewhere is allowed via a disposable index.
- Round 1: `corrected` ("nested git repos stored as gitlinks and not fully recoverable"): the code is stricter, it refuses the rollback instead of doing a partial one.
- For rizzy-harness: either snapshot nested repos' files or refuse; never half-restore.
- Confidence: traced end to end (checked in review).

### Client and server split

#### Local control socket mints single-use 30 s tickets; the WebSocket carries the ticket in a subprotocol, not the URL
- What the code does: the gateway listens on `$HERMES_HOME/gateway.sock` (or a pointer-file fallback; a named pipe on Windows), one JSON request per connection. On POSIX the peer's uid is read from the socket; only a same-user peer gets a `session-ticket`. `TicketStore.mint` stores a SHA-256 of a `secrets.token_urlsafe(32)` ticket with a 30 s expiry, a purpose (`interactive`, `native-http`, `worker-adoption`, ...), a capability set, a subject and a scope (`profile` or `host`), capped at 1,024 outstanding tickets; `redeem` deletes it on first use and checks profile and purpose. Clients dial the WebSocket with subprotocols `hermes-gateway-v1` and `hermes-gateway-ticket.<ticket>`; the server accepts exactly one ticket protocol.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/control_socket.py#L381-L409 - `_session_ticket` validation and 30 s.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/control_socket.py#L411-L425 - peer uid check, home mode checks.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/runtime_bootstrap.py#L9-L21 - purposes and capability sets.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/runtime_bootstrap.py#L25-L75 - mint, redeem, single use.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/web_server_boundary.py#L156-L173 - subprotocol parsing.
- Edge cases and failure handling: request size is capped at 64 KiB and response 512 KiB; a read timeout of 2 s; a stale `instance_id` is refused (`stale runtime instance`); the forwarded subject must be the structured login form `auth:v1:[provider, issuer, user]`, a bare label fails closed.
- Round 1: `confirmed` ("single-use 30 s tickets via a control socket"); `new (not documented)`: the subprotocol transport and the capability sets per purpose.
- For rizzy-harness: our node link needs the same: a short-lived single-use credential bound to purpose and instance, and never in a URL (we already ban secrets in URLs).
- Confidence: traced end to end (checked in review).

#### Non-loopback dashboard requires an auth provider; `--insecure` is ignored; one login model, one owner
- What the code does: `should_require_auth(host)` is true for any host not in the loopback set (RFC 1918 and link-local count as public); the legacy `allow_public` parameter is accepted and ignored. Start-up prompts for a provider if none is registered and a TTY exists. Providers: Nous OAuth (default), `basic` (single configured username and a scrypt hash, HMAC-signed stateless session tokens, 12 h access TTL, 30-day refresh), `self_hosted`, `session_token`. Cookies are HttpOnly with `__Host-` or `__Secure-` prefixes depending on proxying. All authenticated non-native principals act as one owner: `gateway/owner_identity.py` states "Hermes is single-user".
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/web_server_boundary.py#L111-L119 - gate predicate and the 0-day note.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/main_dashboard.py#L640-L662 - provider prompt, "`--insecure` does not bypass this".
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/plugins/dashboard_auth/basic/__init__.py#L1-L9 - basic provider properties.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/owner_identity.py#L1-L12 - single owner.
- Edge cases and failure handling: the Host header is checked against the bound interface or declared public hosts (DNS-rebinding fence), and a non-loopback `dashboard.public_url` engages the gate even on a loopback bind. A Desktop-spawned loopback backend with a per-spawn token is exempted only when all of loopback, `HERMES_DESKTOP=1` and an operator credential hold.
- Round 1: `confirmed` ("non-loopback dashboard fails closed without auth"); `new (not documented)`: `--insecure` is dead and the reason in the source is a named incident (June 2026).
- For rizzy-harness: we have real multi-user accounts; Hermes does not, so there is nothing to copy for ownership. Copy the "any non-loopback bind means auth gate, no override flag".
- Confidence: traced end to end (checked in review).

#### The OpenAI-compatible API server: fixed key, loopback by default, 10 concurrent runs
- What the code does: the API server binds `127.0.0.1:8642`, refuses to start without a sufficiently strong `API_SERVER_KEY`, compares bearer tokens with `hmac.compare_digest`, and returns 429 beyond `max_concurrent_runs` (default 10, 0 disables). Runs started over it are an "unattended platform" for approvals (default deny).
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/platforms/api_server.py#L154-L155 - default host and port.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/platforms/api_server.py#L1428-L1435 - constant-time compare.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/platforms/api_server.py#L3907-L3931 - refuse to start without a strong key.
- Edge cases and failure handling: one shared key, no per-client scopes. The `config_defaults.py` comment names the API server among platforms with no `/approve` channel (L1639-L1642).
- Round 1: `confirmed` (bearer required, default bind, 10 runs).
- For rizzy-harness: a single shared bearer is the weak spot to avoid; we should issue per-client tokens bound to a user.
- Confidence: read the definition (checked in review).

### Messaging gateway reliability

#### Delivery ledger: the reply is persisted as an obligation before the send, `attempting` is set immediately, and recovery sends it again with an explicit "may be a duplicate" prefix
- What the code does: a `delivery_obligations` row (id = hash of session key, inbound message id and content) records the final response text, platform, chat and thread, a state, an attempt count, and the owning process's `(pid, start time)`. `_record_delivery_obligation` writes the row as `pending` and then immediately `mark_attempting`; `mark_delivered` only when the platform `SendResult.success`, otherwise `mark_failed`. On boot `sweep_recoverable` claims rows whose owner process is dead (a compare-and-set that re-stamps the owner and bumps `attempts`), abandons rows with 3 attempts or older than 24 h, and returns the rest for redelivery with a marker prefix unless the row is a never-claimed `pending` one. Rejections are retried with 30 s and 120 s backoffs; a flood-control refusal waits the platform's stated time; a dead chat (blocked bot, deleted group) is never retried. Retention is 7 days and 500 rows.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/delivery_ledger.py#L1-L11 - state machine as designed.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/delivery_ledger.py#L188-L209 - schema.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/platforms/base.py#L4136-L4168 - write before send; `mark_attempting` at L4164.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/delivery_ledger.py#L368-L446 - boot claim and abandon rules.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/delivery_ledger.py#L33-L36 - 3 attempts, 24 h, 7 days, 500 rows.
- Edge cases and failure handling: all ledger failures are swallowed so a ledger error never blocks a send (best-effort; at-least-once, not exactly-once). Slash-command replies and ephemeral replies are not ledgered. `record_crash_left_reply` adopts a reply a killed process had persisted but not ledgered, as `attempting`. The ledger can be turned off with `gateway.delivery_ledger: false` (default on): https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/delivery_ledger.py#L555-L563.
- Round 1: `confirmed` (3 attempts in 24 h, stored before send) and `corrected`: Round 1 says a send that never started is resent plainly; in code `pending` is overwritten by `attempting` in the same function before the await, so in practice a crash always leaves `attempting` and the reply is re-sent with the marker. A plain redelivery is only possible for a pending row an older build left.
- For rizzy-harness: if the master ever bridges chat platforms, persist the owed reply with a liveness-stamped owner before sending. Keep the visible "may be a duplicate" prefix; hiding duplicates is worse.
- Confidence: traced end to end (checked in review).

#### Durable per-session FIFO (`session_admissions`) with an epoch fence; a turn lost in an unknown state is never replayed
- What the code does: each inbound message is first committed as a `session_admissions` row `(principal, target_session, request_id)` with a unique key (so a retried delivery is an exact replay, and a changed payload under the same key raises `admission_conflict`), a payload digest, an intent (`queue`, `steer`, `redirect`) and a status `queued -> started -> terminal | unknown`. At most 32 inputs may be queued per session. `claim_session_input` takes the FIFO head inside one write transaction: it refuses (`unknown_execution`) if any admission or worker for the session is `unknown`, does nothing if one is still `started`, bumps the session's `runtime_generation`, and marks the row `started` with `(owner_epoch, generation)`. `settle_session_input` only accepts the row when status, epoch and generation all match (`stale_generation` otherwise). At gateway start `begin_runtime_epoch` increments a singleton epoch under the profile ownership lock, and `recover_session_inputs` marks every `started` admission from an earlier epoch `unknown` and every non-terminal worker execution `unknown`, and never requeues them. Only an explicit `resolve_unknown_session_input` (the person's Discard, or the boot-time unattended settlement) settles an unknown row, never back to `queued`; the outcome is `interrupted`, except that when the same owner epoch captured the finished result before its settlement write failed (`_captured`), that row can resolve as `completed` or `failed` with the stored answer ([hermes_state_runtime.py#L663-L694](https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L663-L694)); it also writes a transcript boundary in the same transaction, so the FIFO cannot advance past an unclosed turn.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_common.py#L597-L615 - `session_admissions` table and index.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L126-L177 - queue cap and admission.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L206-L242 - FIFO claim, unknown blocks.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L245-L274 - settle with epoch and generation check.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L453-L464 - `recover_session_inputs`: "Never replay started work".
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L657-L694 - explicit resolution to `interrupted`.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/session_authority.py#L1450-L1467 - boot order: epoch, recover, settle unattended.
- Edge cases and failure handling: a `steer` that was handed to a lost claim is settled `interrupted`, not replayed. The table's `CHECK` forbids a `started` row without an epoch and generation. The delete guard (earlier finding) plus `ON DELETE RESTRICT` stop a session with runtime history from being removed by an old build.
- Round 1: `confirmed` (durable per-session queue; crashed turn `unknown`, never replayed, session blocked until discard); `new (not documented)`: the fence is a monotone epoch plus per-session generation, checked inside every state change, and `unknown` also covers worker executions. Round 1 asked how `unknown` is recorded; this is it.
- For rizzy-harness: this is the closest published mechanism to our "restart closes the turn as interrupted" invariant. We should keep our simpler rule (close as interrupted and let the person resend) but adopt the generation check: every settle must name the claim it settles.
- Confidence: traced end to end (corrected in review: an unknown row resolves to `interrupted` unless the same epoch captured the finished result, then to `completed`/`failed`; the epoch and generation fence, the never-requeue rule and the queue cap of 32 were checked and hold).

#### Unattended turns that end in `unknown` settle themselves as interrupted so a queue does not stall behind nobody
- What the code does: an admission is "unattended" when its principal is the cron principal or starts with `automation:`, or its payload carries `local_automation_v1`, `kanban_card_v1` or a webhook-delivery marker. At boot, right after the fence, `settle_recovered_unattended` resolves each such `unknown` row through the same `resolve_unknown_session_input` a person's Discard uses, with an "automation interrupted" transcript notice; a person's `unknown` still pauses the session. The comment records that a Bot Chat sat behind one for ten hours.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/session_unattended_unknown.py#L23-L38 - what counts as unattended.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/session_unattended_unknown.py#L41-L66 - boot settlement.
- Edge cases and failure handling: a row that cannot be resolved (`RuntimeStoreError`) stays `unknown` with a warning.
- Round 1: `new (not documented)`.
- For rizzy-harness: our routines will hit this: a crashed unattended run should auto-close as interrupted and record why; only human-sent turns wait for a decision.
- Confidence: traced end to end (checked in review).

#### Relay: the gateway dials out with a per-gateway HMAC upgrade token and acknowledges buffered inbound frames after durable admission
- What the code does: `WebSocketRelayTransport` dials `wss://.../relay` (keepalive ping 30 s, pong deadline 60 s) and sends one `hello` per fronted identity; the connector replies with a capability descriptor. The upgrade bearer is `base64url(gateway_id:exp:HMAC-SHA256(gateway_id:exp, secret))` with a 300 s TTL; the connector keeps a rotating list of secrets. Frames are newline-delimited JSON: gateway-to-connector `hello`, `outbound`, `interrupt`, `going_idle`, `inbound_ack`; connector-to-gateway `descriptor`, `inbound`, `outbound_result`, `interrupt_inbound`, `going_idle_ack`, `passthrough_forward`. Inbound events carry a `bufferId` when replayed from the connector's buffer; the gateway sends `inbound_ack` only after the message was admitted into `session_admissions` (or permanently refused), so a crash before admission leaves the frame unacknowledged and the connector redelivers. Admission runs off the socket reader but in a per-chat lane to keep order. A close with code 4401 after a successful handshake is treated as revocation (terminal, no reconnect); an expired token is retried once with a fresh one.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/auth.py#L1-L9 and https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/auth.py#L36-L40 - token format.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/ws_transport.py#L442-L494 - dial, hellos.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/ws_transport.py#L971-L1004 - ack after admission.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/inbound_admission.py#L10-L52 - per-chat lanes, permanent refusals acked.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/ws_transport.py#L66 - revocation close code.
- Edge cases and failure handling: pending outbound requests are settled on reader exit; media sends settle as "possibly delivered", not failed, to avoid double posting. The `deliveryKey` is obtained from `/relay/provision` and kept in the process environment (`GATEWAY_RELAY_DELIVERY_KEY`): https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/__init__.py#L665-L672. What the connector does with it, and its buffer persistence, are not in this repository.
- Round 1: `confirmed` (outbound-only, single WebSocket, per-gateway secret and delivery key, buffering while offline); the buffering is the connector's and is not verifiable here; `new (not documented)`: ack-after-admission, HMAC token form, 4401 semantics.
- For rizzy-harness: our node dials out already; add "ack after durable commit" to the node-to-master event stream.
- Confidence: traced end to end for the gateway side only.

#### Bot-loop guard caps bot-to-bot hops at four
- What the code does: a configuration dataclass sets `max_hops: int = 4`; each bot message after a human message is a hop.
- Trace: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/bot_loop_guard.py#L34 - default.
- Edge cases and failure handling: outcomes include `disabled`, `human` (chain reset), `ok`, `max_hops`, `duplicate`, `reply_to_reply` (docstring); I did not trace the call sites.
- Round 1: `confirmed` (loop guard 4 hops).
- For rizzy-harness: not needed unless we bridge chat rooms.
- Confidence: read the definition.

### Scheduling and unattended operation

#### Cron: a JSON job store, a 60 s ticker, at-most-once advance before dispatch
- What the code does: jobs live in `~/.hermes/cron/jobs.json` per profile (locked across processes), run history in `cron/executions.db` (opened with `synchronous_full`). The ticker (default every 60 s) takes a per-home tick lock, runs an admission reconcile, reads due jobs, and advances `next_run_at` for recurring jobs before any dispatch; if the store refuses the write it drops the recurring jobs rather than risk a re-fire after a crash ("at-most-once"). A headless tick (system crontab) refuses agent jobs when no gateway owns the home instead of spawning one. Jobs run in a parallel pool (`cron.max_parallel_jobs`); an inactivity watchdog (default 600 s idle, not wall clock) interrupts stalled jobs.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/jobs.py#L2 and https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/jobs.py#L95 - store path.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/executions.py#L57-L59 - history DB.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_tick.py#L62-L72 - advance or drop.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_tick.py#L92-L100 - tick contract, lock, headless.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_tick.py#L160-L164 - advance before submit; `#L134-L136` headless refusal.
- Edge cases and failure handling: `hermes pause` is an estop checked at the top of every tick; a store that is unwritable is marked degraded and dispatch is skipped; a stale-code process yields to a fresher gateway.
- Round 1: `confirmed` (jobs.json, executions.db, 60 s tick); the at-most-once advance, headless refusal and estop are `new (not documented)`.
- For rizzy-harness: "advance the schedule first, then run" is the right at-most-once rule for unattended work; pair it with a recorded missed-run reason.
- Confidence: traced end to end (checked in review).

#### A cron run is admitted through the same per-session queue, journalled to disk, and a fire of unknown outcome pauses the job
- What the code does: `run_canonical_job` writes a per-fire admission journal `cron/admissions/<sha256(job, request)>.json` (mode 0600, fsync of the directory) containing the request parameters and a receipt, then submits the fire to the gateway owner (principal = the cron principal). A retry with different parameters under the same request id raises `CronExecutionUnknown`. If the owner may have accepted a fire but the result is unverified, `pause_for_unknown` pauses the job and records that exact pause on the journal so only a verified terminal receipt can lift it (an operator's own pause is never lifted).
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_authority.py#L8-L10 - the unknown-outcome exception.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_authority.py#L29-L51 - pause and lift.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_authority.py#L55-L90 - journal then submit.
- Edge cases and failure handling: an unattended `unknown` admission also auto-settles at boot (earlier finding).
- Round 1: `new (not documented)`.
- For rizzy-harness: a routine run should have an idempotency key persisted before dispatch; pause the routine rather than guess when the outcome is unknown.
- Confidence: read the definition (journal and submit traced; the receipt-recovery path `reconcile_pending` not read).

#### How unattended runs are authorised: deny by default, a tool-set denylist, a regex tripwire on the prompt, scripts only from one directory
- What the code does: (1) `approvals.cron_mode` defaults to `deny` (also `single_query_mode` and `unattended_mode`): a dangerous command in a cron run is blocked instantly instead of waiting for a human; `approve` is an explicit opt-in. (2) A cron-spawned agent never receives `messaging` or `clarify`; the `cronjob` toolset is denied too unless `cron.allow_agent_scheduling: true` (a comment calls that "loop prevention, not a security boundary"), and `agent.disabled_toolsets` is layered on top so a per-job `enabled_toolsets` cannot widen it. (3) Model-facing `cronjob` cannot set `model`/`provider`/`base_url` (so the agent cannot redirect unattended spend). (4) The user's prompt is scanned at create/update (and again at runtime) against eight regexes (prompt-injection phrases, `cat ...\.env|id_rsa`, `authorized_keys`, `/etc/sudoers`, `rm -rf /`) plus invisible-Unicode; the scan of the assembled prompt with skill text uses only the first four. (5) A job's `script` must resolve inside `HERMES_HOME/scripts/` (resolved path relative-to check), never a command line. (6) A script's last stdout line `{"wakeAgent": false}` skips the agent run entirely (no LLM, no delivery); anything else wakes it.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L1636-L1662 - approval modes and the three `deny` defaults.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L706-L728 - the cron and unattended contexts.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler.py#L418-L439 - disabled toolsets.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/cronjob_tools.py#L1233-L1242 - model pins withheld from the agent.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/cronjob_prompt_scan.py#L27-L37 - tripwire patterns.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_script.py#L327-L363 - script confinement.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_prompt.py#L21-L35 - wake gate.
- Edge cases and failure handling: the prompt scan is a deny-list of phrases; a paraphrased instruction passes, and the code comments call the strict set a "tripwire". The effective protection is the deny default plus the toolset denylist. A job is authorised by whoever created it (user or an agent in an interactive session); there is no separate "approve this scheduled job" step in what I read.
- Round 1: `confirmed` (script-only cron, wake gating, deny default, scripts under one directory, prompts scanned at create); the model-pin withholding and the toolset denylist are `new (not documented)`. Round 1's note that "cron cannot create jobs by default" is true but is a loop-prevention default, not security.
- For rizzy-harness: copy the defaults (an unattended run cannot get approval, so it denies), and copy "the agent may not choose the model for unattended spend". Do not rely on a phrase scanner.
- Confidence: traced end to end (checked in review).

#### Kanban: the "worker process" is a thin client; the agent runs inside the owner gateway
- What the code does: the dispatcher claims a task (`ready -> running` inside a write transaction, re-checks parents, opens a run, claim TTL default 15 min), then `_default_spawn` starts a detached subprocess (`start_new_session=True`, stdout to a per-task log) whose argv is only `python -m hermes_cli.kanban_worker_client`. That client connects to the assigned profile's gateway owner, submits `kanban.run` with `(board, task_id, run_id, claim_lock)`, polls `prompt.receipt` every 0.2 s and exits with the receipt's code. The owner runs the agent as an ordinary admitted turn (principal marker `kanban_card_v1`). The subprocess environment is built per assigned profile (secrets scrubbed when the home is routed or multiplexed, `HERMES_HOME`, board DB path, task id, run id, claim lock, `TERMINAL_CWD` pinned to the workspace). A lost owner is waited for up to 180 s from its last answer before the worker fails honestly. A clean exit while the task is still `running` is classed a protocol violation (corrective text goes to the retry); a quota wall is requeued without counting a failure; the default failure limit is 2.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_db.py#L2171-L2203 - atomic claim.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_db_dispatch.py#L2697-L2700 - `_worker_argv`: "The profile owner executes; this subprocess only waits for its receipt."
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_db_dispatch.py#L2754-L2762 and https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_db_dispatch.py#L2878-L2887 - spawn.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_worker_client.py#L1 and https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_worker_client.py#L121-L135 - submit and poll.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_db_dispatch.py#L1103-L1114 - clean exit as protocol violation.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/session_unattended_unknown.py#L33 - `kanban_card_v1` is an unattended marker.
- Edge cases and failure handling: a claim carries a `claim_lock`, so the worker's later `complete` or `block` transitions can be rejected if the claim was reclaimed; `owner_reclaim_paused` stops TTL reclaim once the owner has admitted the run (comment in the worker client). Isolation between workers is therefore the owner's per-session isolation, not an OS-process boundary for the agent.
- Round 1: `corrected` ("Kanban with worker-per-OS-process"): there is a process per card, but it does not run the agent or its tools; the agent and tools run in the gateway owner's session. The failure ladder (reclaim, protocol violation) is `confirmed`; the exact statuses and the 60 s tick I did not re-verify.
- For rizzy-harness: the shape (claim with a lock, run on the owning node, receipt back, worker is a thin waiter) is close to our master/node split; the failure to avoid is the second authority: here the board DB and the owner both hold state, and a whole file of reconciliation exists (`kanban_owner_recovery.py`).
- Confidence: traced end to end for spawn and claim; the agent-side execution not read.

### MCP client

#### Tool filter (include wins over exclude, globs), name collisions fail closed, and the description scan only warns
- What the code does: per server, `tools.include` is a whitelist (empty list registers nothing) and `tools.exclude` a blacklist, entries exact or `fnmatch` globs (`*`, `?`, `[`), case-sensitive; include takes precedence. Registered names are `mcp_<server>_<tool>`. Name collisions: exact duplicates dropped, a utility shadowed by a native tool, any other multi-origin collision skips every colliding entry. A description-injection scan runs on every tool (live and cached) against ten regexes (ignore previous instructions, role tags, `curl http`, `eval(`...), but `_tool_candidates` discards its result: findings are only logged and the tool is still registered. Cached tool rows from the user-writable schema cache with missing annotations are treated as write-capable.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_registration.py#L247-L262 - filter construction.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_schema.py#L312-L321 - glob matching.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_registration.py#L288-L300 - scan call whose return value is unused (L297).
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_schema.py#L17-L41 - patterns and warn-only function.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_registration.py#L315-L330 - collision rules.
- Edge cases and failure handling: a server that does not advertise `resources`/`prompts` capabilities does not get the utility stubs for them (schema.py comment at L323-L330 region).
- Round 1: `confirmed` (include/exclude globs); `new (not documented)`: the injection scan has no blocking effect.
- For rizzy-harness: a scanner that only logs is cosmetic; if we scan tool descriptions, make a hit demote the tool to needs-approval.
- Confidence: traced end to end (checked in review).

#### Stdio MCP servers start from a small allow-listed environment; HTTP bodies are capped; errors are redacted
- What the code does: `_build_safe_env` keeps only `PATH, HOME, USER, LANG, LC_ALL, TERM, SHELL, TMPDIR` and `XDG_*` plus whatever the server's own `env:` config lists. The stdio command is run through an OSV malware preflight and an npx cache swap before `StdioServerParameters` is built; the server's stderr goes to a log file. HTTP transports wrap httpx in a body-cap transport at 10 MiB, strip auth on cross-origin redirects, support mTLS client certs and proxies. Error text passes `_sanitize_error` (credential regex redaction and stripping of Unicode tag characters); content blocks strip tag characters from type/MIME/name fields.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_config.py#L147-L150 and https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_config.py#L199-L207 - safe env.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_transport.py#L376-L388 - stdio spawn inputs and preflight.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_errors.py#L332 - body cap constant.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_common.py#L85-L88 - error sanitising.
- Edge cases and failure handling: orphaned stdio children of failed attempts are reaped before each spawn (SIGTERM, wait, SIGKILL); on Windows the process is attached to a kill-on-close job object.
- Round 1: `confirmed` ("Stdio servers get only a safe env baseline", error redaction, tag-character stripping).
- For rizzy-harness: copy the allow-list for MCP child environments; add a response size cap on remote MCP.
- Confidence: read the definition (checked in review).

#### Tool search: three bridge tools replace deferred schemas; the catalog is rebuilt, not stored per session
- What the code does: when any deferrable tool (MCP, plugin or a curated set of core tools) exists, those tool schemas are replaced in the model-visible list by `tool_search`, `tool_describe` and `tool_call`. The catalog is stateless and rebuilt from the live tool definitions at every assembly ("a session-keyed one drifts and silently drops tools"). An optional manifest is embedded within a token budget (`min(4000 tokens, threshold% of context)`) and degrades from full to names to bare. Search accepts at most 7 queries per call and describe at most 10 names. The batch planner peels a `tool_call` wrapper to decide parallel admission on the underlying tool.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/tool_search.py#L1-L8 - invariants.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/tool_search.py#L38-L63 - limits and config.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_dispatch_helpers.py#L137-L150 - bridge peeling.
- Edge cases and failure handling: `enabled: "auto"` is an alias of `on` in the current code, so the "opt-in" wording in Round 1's contradiction list matches the code: it is on whenever any tool is deferrable.
- Round 1: `confirmed` (bridge tools, tiered manifest, "auto behaves as on").
- For rizzy-harness: approvals must be decided on the underlying tool name, not `tool_call`; the code needed an explicit fix for parallel admission, which is the class of bug to test for.
- Confidence: read the definition.

### Other client protocols

#### ACP: `hermes acp` is a viewer of the profile gateway, not an in-process agent
- What the code does: `HermesACPAgent.__new__` returns `GatewayACPAgent` unless a session manager is passed, and `entry.main` constructs it with none, so the stdio ACP process never runs an agent or MCP itself ("MCP discovery and execution belong to the daemon; this process is a viewer"). `session/new` calls the owner's `runtime.describe`, requires the `acp-editor-policy-v1` capability, then `session.create` with `source="acp"`, the translated cwd, the editor's MCP servers and an edit-approval policy of `ask`. `session/fork` calls the same `branch` mutation as the branch finding. The adapter uses the Python `acp` SDK over stdio with logging forced to stderr.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/server.py#L284-L288 - dispatch to the gateway-backed agent.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/entry.py#L205-L215 - viewer construction and `acp.run_agent`.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/gateway_server.py#L271-L291 - `session/new` via owner RPC.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/gateway_server.py#L459-L470 - fork as branch.
- Edge cases and failure handling: the in-process `HermesACPAgent(session_manager=...)` path still exists; it is not the one `hermes acp` takes. ACP sessions end when the last viewer detaches with an idle queue (gateway side), not on adapter exit.
- Round 1: `corrected` ("`hermes acp` serves ACP over stdio ... sessions persist, listed, resumed, forked"): all true, but the process model is a thin front end on the gateway; the docs imply an agent per process.
- For rizzy-harness: this is the pattern we want for an ACP front end: a thin adapter that attaches to the node-owned session and never runs tools itself.
- Confidence: traced end to end.

#### ACP permission bridge: once, session, always, deny; timeout returns "timeout", not deny
- What the code does: Hermes dangerous-command approvals become ACP `request_permission` calls with option ids `allow_once`, `allow_session`, `allow_always`, `deny`, `deny_always` (ACP has no session scope, so `allow_session` is sent as kind `allow_always` and the meaning lives in the id). An unrecognised option id or a non-allowed outcome maps to deny. The wait is `approvals.timeout` (default 300 s, replacing an earlier hard-coded 60 s that denied under a still-open card); on timeout the callback returns `"timeout"`, which the approval layer reports separately from a user denial.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/permissions.py#L17-L19 - option mapping.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/permissions.py#L67-L74 - outcome mapping.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/permissions.py#L117-L124 - timeout resolution.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_context.py#L318-L328 - 300 s default.
- Edge cases and failure handling: a client that auto-approves `allow_once` defeats manual mode; this is client behaviour (Round 1 notes Buzz Desktop), not enforceable here.
- Round 1: `confirmed` (once/session/always/deny, 300 s timeout denies).
- For rizzy-harness: scope names belong to us; map ACP's narrower options onto node-side scopes and keep the scope id in our own field.
- Confidence: read the definition.

### Accounts, profiles and extension trust

#### Single user by design; profiles separate homes and secrets; one process can serve several
- What the code does: Hermes states it is single-user: every authenticated non-native principal collapses to the owner subject. Isolation is per profile: separate `HERMES_HOME`, `state.db`, cron store and checkpoint store. When `gateway.multiplex_profiles` is on, one process serves many profiles using a context-local `secret_scope`; `get_secret` raises, rather than falling back to `os.environ`, if multiplexing is active and no scope is bound. Routing of senders to profiles is by additive specificity (user 16, thread 8, chat 4, guild 2).
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/owner_identity.py#L1-L12 - single-user statement.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/secret_scope.py#L1-L10 - fail-closed secret scope.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/profile_routing.py#L1-L8 - route specificity.
- Edge cases and failure handling: a secondary profile's Kanban worker or cron run re-binds that profile's scope before building its environment, so it does not inherit the launch profile's keys (`kanban_db_dispatch.py#L2780-L2793`).
- Round 1: `confirmed` (multiplexing, per-profile state, secret reads fail closed).
- For rizzy-harness: Hermes has no multi-user model to copy. Its fail-closed context-local secret scope is a good guard for any process that serves more than one user.
- Confidence: read the definition.

#### Plugin and skill trust: a regex scanner at install, verdicts safe/caution/dangerous, in-process by default
- What the code does: `hermes plugins install/update` runs the `skills_guard` scanner (plugin variant `plugin-guard-v12`); findings of severity critical give `dangerous` (blocked; `--force` does not override), high gives `caution` (needs confirmation), medium and low alone are `safe`. Plugin code runs in the Hermes process by default (`plugins.isolation: in_process`); `host` moves third-party plugin code into a separate plugin-host process per profile and refuses other in-process user-module paths, which is the property a pooled deployment needs. Bundled plugins stay in-process.
- Trace:
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/skills_guard.py#L953-L956 - verdict function.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/plugin_guard.py#L1-L9 - install/update scan rules.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/plugin_isolation.py#L1-L18 - isolation modes.
  - https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L1715 - default `in_process`.
- Edge cases and failure handling: the scanner is a pattern engine, so it is evidence for review, not a boundary; test trees step down one severity. Hooks and lifecycle events in plugins are fail-open at `emit_session_deleted`; I did not trace other hook points here (safety cluster).
- Round 1: `confirmed` (plugin trust model, optional host-process isolation); the verdict rule answers Round 1's "how `dangerous` verdicts are computed".
- For rizzy-harness: skills and plugins on a node should load only from node-controlled paths and be unable to widen `node.toml`; a scanner can inform the owner but cannot be the gate.
- Confidence: read the definition.

## 4. Round 1 claims checked

| Claim (Round 1) | Verdict | Citation |
|---|---|---|
| Sessions in SQLite `state.db` with WAL and FTS5 | confirmed, with correction: WAL avoided on affected SQLite builds and refused on virtiofs/9p | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_wal.py#L301-L304 |
| Session search returns raw FTS5 excerpts, no LLM | confirmed at the store level; model-facing tool not read | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_search.py#L1177-L1189 |
| Compaction is in place by default (keeps the session id, archives rows) | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L667-L671 |
| Resume restores cwd and finds the latest session | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/main.py#L1464-L1478 |
| Export `jsonl`, `md`, `html`, `trace` with `--redact` | confirmed (trace redaction default on, aborts on failure) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/sessions_cmd.py#L500-L503 |
| Auto-prune off by default | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L2252-L2260 |
| Shadow-git checkpoints opt-in, one shared bare repo, ref per project | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L13-L39 |
| Snapshots before `write_file`/`patch` and destructive terminal commands, one per directory per turn | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_executor.py#L1030-L1046 |
| Caps 20 snapshots, 500 MB, 10 MB per file, 7 days, 50,000 files | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L485-L503 |
| Never snapshots `/` or `$HOME` | corrected: also every ancestor of home | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L872-L875 |
| `/rollback` skips files the user edited since unless `--all` | confirmed, but the ledger only tracks file-tool writes | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L986-L1050 |
| Pre-rollback snapshot makes restore undoable | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1413-L1414 |
| Nested git repos are gitlinks, "not fully recoverable" | corrected: whole-tree rollback is refused | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1376-L1385 |
| Checkpoints disabled on container backends | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L940-L952 |
| Delivery ledger: replies stored before send, 3 attempts in 24 h | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/delivery_ledger.py#L33-L36 |
| A send that never started is resent plainly; a mid-send one gets a duplicate marker | corrected: `pending` is overwritten with `attempting` before the await | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/platforms/base.py#L4156-L4165 |
| Crashed turn marked `unknown`, never replayed, session blocks until discarded | confirmed (and unattended ones auto-settle) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L453-L464 |
| Durable per-session FIFO | confirmed (`session_admissions`, cap 32) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L126-L177 |
| Relay: outbound-only, per-gateway secret and delivery key | confirmed for the gateway side; connector buffering not in this repo | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/ws_transport.py#L442-L494 |
| Bot-to-bot loop guard 4 hops | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/bot_loop_guard.py#L34 |
| Cron jobs in `jobs.json`, history in `executions.db` | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/jobs.py#L95 |
| Scheduler ticks every 60 s | confirmed (default interval parameter) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_provider.py#L152 |
| Dangerous commands denied in cron by default | confirmed (`cron_mode: deny`) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L1658-L1662 |
| Cron cannot create jobs by default | confirmed, but it is a loop-prevention toolset denial, not a security boundary | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler.py#L418-L439 |
| Scripts must live under `$HERMES_HOME/scripts/` | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_script.py#L327-L363 |
| `{"wakeAgent": false}` last line skips the tick | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_prompt.py#L21-L35 |
| Cron prompts scanned at create time | confirmed (regex tripwire) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/cronjob_prompt_scan.py#L27-L37 |
| Kanban worker per OS process | corrected: a thin client process; the agent runs in the gateway owner | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_db_dispatch.py#L2697-L2700 |
| Kanban: clean exit without completion is a protocol violation | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_db_dispatch.py#L1103-L1114 |
| MCP include/exclude globs | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_registration.py#L247-L262 |
| Stdio MCP servers get only a safe env baseline | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_config.py#L147-L150 |
| Tool Search bridge (`tool_search`, `tool_describe`, `tool_call`) | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/tool_search.py#L1-L8 |
| `hermes acp` stdio server, approvals once/session/always/deny, 300 s timeout denies | confirmed; process model corrected (viewer of the gateway) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/entry.py#L205-L215 |
| CLI attaches over a local WebSocket with single-use 30 s tickets via a control socket | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/runtime_bootstrap.py#L25-L75 |
| Non-loopback dashboard fails closed without auth | confirmed (and `--insecure` is ignored) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/web_server_boundary.py#L111-L119 |
| API server: bearer key required, 127.0.0.1:8642, 10 concurrent runs | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/platforms/api_server.py#L3907-L3931 |
| Multiplexed gateway: secret reads fail closed without a scope | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/secret_scope.py#L1-L10 |
| Plugin isolation optional, in-process by default | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L1715 |

## 5. In the code but not in the documentation

- Epoch plus generation fence on the session queue and on worker executions; recovery marks lost work `unknown`, never queued (`hermes_state_runtime.py`).
- Unattended `unknown` admissions auto-settle as interrupted at boot (`gateway/session_unattended_unknown.py`).
- A downgrade guard trigger and `ON DELETE RESTRICT` that stop an older build deleting sessions with runtime history.
- The FTS projection-view design (stable function of the row, rebuild markers, cross-process rebuild lock) and the LIKE/trigram/OR-relaxed fallback ladder.
- WAL avoided on affected SQLite builds; refused on cross-VM filesystems.
- Branch resets approval policy to `ask` and rebinds editor MCP servers under the child's scope.
- Checkpoint safe-restore ledger is fed only by file-tool writes; destructive terminal commands are not in it.
- Cron admission journal with pause-on-unknown; the cron `cronjob` toolset denial is documented in-code as not a security boundary; model pins are withheld from the agent.
- The ticket rides in the WebSocket subprotocol, with a capability set per purpose.
- `--insecure` is accepted and ignored on the dashboard (named 2026 incident).
- MCP description-injection scan is warn-only.

## 6. Could not find or could not settle

- Connector side of the relay (buffer persistence, how the delivery key is scoped, replay order): lives in `NousResearch/gateway-gateway`, not cloned. Settles by reading that repository.
- Whether git hooks or filter drivers can fire in the checkpoint store: no explicit `core.hooksPath` override in `_git_env`; the store's config is Hermes-written and `GIT_DIR` is the store, so I infer they cannot. A test with a hostile `.gitattributes` and a global filter would settle it.
- Whether a destructive terminal command missed by the regex loses its snapshot in practice: inferred from the pattern; not exercised.
- `hermes -w` worktree creation with hooks, fsmonitor and filters disabled: the internal git environment (`noninteractive_git_env`, `core.hooksPath=/dev/null`, `core.fsmonitor=false`) is in `hermes_cli/_subprocess_compat.py#L287-L300`; I did not trace the worktree creation call to confirm it uses it at checkout.
- Kanban statuses, tick interval and the owner-side recovery (`kanban_owner_recovery.py`): not read.
- Model-facing `session_search` tool (what it adds above `search_messages`): not read.
- Delegation worktree isolation and the curator: outside this cluster.

## 7. Citation check

`research/round2/hermes-agent.sessions.md: 177 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit`

(Short in-text references such as `file.py#L287-L300` without a full URL are not covered by the checker; I read those ranges.)

## 8. Review

Checked 16 of 32 findings (the epoch and generation fence, unknown-turn handling, delivery ledger, shadow-git restore and snapshot triggers, nested repos, control-socket tickets, dashboard and API-server gates, cron at-most-once and unattended defaults, MCP filter and env, WAL, in-place compaction). Unchecked: SQLite schema and FTS, search routing, resume, branch, rewind, export, relay, bot-loop guard, the cron journal, Kanban, tool search, ACP, profiles, plugin trust.

| Finding | Verdict | Reason |
|---|---|---|
| WAL and journal mode | supported | vulnerable SQLite goes to DELETE; virtiofs/9p refuses WAL |
| Compaction in place by default | supported | `"in_place": True` |
| Shadow-git store isolation | supported | `GIT_DIR`, `GIT_WORK_TREE`, per-project index, config vars to devnull; the hooks inference stays untested |
| When snapshots are taken | partly | `git rm` is detected by the regex; `bash -c "rm ..."`, `find -delete`, `python -c`, `tee` are not |
| Restore and write ledger | partly | ledger fed only by `write_file`/`patch`; shell-changed files are skipped, but a file written by a file tool then deleted by the shell is restored, and an empty ledger means a full restore |
| Nested git repositories | supported | restore refuses with `nested_repositories` |
| Control socket and tickets | supported | 30 s, single use, 1,024 cap, peer uid check, purpose binding |
| Dashboard auth gate | supported | non-loopback always requires auth, `allow_public` ignored |
| API server key | supported | refuses to start without a 16-character non-placeholder key, fail closed if the check cannot run |
| Delivery ledger | supported | `record_obligation` then `mark_attempting`; 3 attempts, 24 h, marker rule found; a crash between the two calls would leave `pending` and a plain resend, a tiny window |
| Durable FIFO with epoch fence | partly | fence, never-requeue and cap 32 confirmed; discard resolves to `interrupted` only unless the same epoch captured the result |
| Unattended unknown settles itself | supported | boot order epoch, recover, settle; unresolvable rows stay unknown |
| Cron at-most-once | supported | advance before dispatch; unwritable store drops recurring jobs |
| Unattended authorisation | supported | three deny defaults, eight patterns, first four on assembled prompts, cron toolset denial as loop prevention |
| MCP tool filter and warn-only scan | supported | return value of `_scan_mcp_description` is discarded |
| Stdio MCP safe environment | supported | `_SAFE_ENV_KEYS` plus `XDG_*` and configured env |

The claim to check closely, that files changed by shell commands are skipped, is true only with the three exceptions above; the author marked it inferred and it was a fair inference, but categorical wording would have misled a design decision. Overall: reliable on mechanism and numbers; the remainder should be trusted similarly except for "never"/"only" statements about regex coverage, which I found one wrong example of.
