# Synthesis 05: sessions (Area 11) and git and version control (Area 12)

Research date 2026-11-10. Synthesis only: every fact below comes from the survey files in `research/round1/`. Nothing was fetched from the web.

## 1. Scope

Areas covered: Area 11 (sessions: persistence, resume, fork, checkpoints, undo) and Area 12 (git and version control), across the seventeen harnesses plus `Ours`.

Files and line ranges read:

| File | Lines read |
|---|---|
| `codex.md` | 160-168 (undo), 505-585 (areas 11-12), 810-816 (Handoff between hosts), 986-1196 (sections 3, 6, 7, 9; keyword-filtered) |
| `opencodex.md` | 81-88, section 3/6/7/9 filtered |
| `opencode.md` | 255-289, sections 3/6/7/9 filtered |
| `claude-code.md` | 1846-2160 (Areas 11, 12 and the code-review entries inside), 2167, 3555-3580 (cloud/security neighbours, skimmed), 3616-3621 (Ultrareview); keyword grep elsewhere. The 900 KB file was not read whole. |
| `claude-code.review-1/2/3.md` | keyword-filtered; review-2 "Corrections to apply" and "Move to Unverified" read in full for the range 1846-2160. Review-1 flags the checkpoint limits (survey line 1902-1974) as unchecked by itself; review-2 then confirms them. |
| `claude-desktop.md` | 208-239, sections 3/6/7/9 filtered |
| `deepseek-harness.md` | 273-318, sections 3/6/7/9 filtered |
| `oh-my-pi.md` | 267-316, sections filtered |
| `pi.md` | 253-286, sections filtered |
| `cursor.md` | 217-264, sections filtered |
| `antigravity.md` | 271-300, sections filtered |
| `openhands.md` | 217-250, sections filtered |
| `openclaw.md` | 312-351, sections filtered |
| `goose.md` | 264-297, sections filtered |
| `hermes-agent.md` | 365-410, 280-285 (importers), sections filtered |
| `zed-acp.md` | 126-146 (A11, A12), 235-249 (B3), sections filtered |
| `warp.md` | 247-287, sections filtered |
| `cline.md` | 244-269, sections filtered |
| `own-rizzy-harness.md` | sections 6, 7 (worktree row), 11, 12, parts of 3, 13, and the "(b)" and gaps lists |

Not read or limited:
- Sections 3/6/7/9 were read by keyword filter, not line by line, for every file. Items outside my areas were ignored.
- Claude Desktop's "Chat search over past conversations" (Area 7, chat product) and Cowork project handling are not in this file's scope; the Claude Desktop search cell is `-` only for the Code tab.
- Hermes Agent evidence paths are relative (`/user-guide/sessions`), exactly as the survey gives them; the survey does not state the host. OpenHands paths use the survey's `D:/` prefix for its docs host, copied as written.
- `00-targets.md` was not needed.
- OpenCodex is a provider gateway: it appears only in the session-storage entry, in the matrix and in the notes.

Trust labels: `reviewed` = entry says verified in review; `corrected` = corrected in review (corrected text used); `unreviewed`; `unverified`. Status labels as in the surveys. For the "Ours" rows the check value is always "from own inventory".

## 2. Features

### Area 11: Sessions

#### Session storage: format, location and shape
What it is: where a conversation lives on disk (or server) and whether it is a structured event log, a transcript, a database, or a set of files.
Variants: three families. (a) Append-only JSONL files: Codex (one file per process plus SQLite index), Claude Code (internal transcript format, explicitly unstable), Oh My Pi and Pi (documented tree-structured JSONL with a versioned schema), DeepSeek harness (checksummed zstd frames). (b) A database: OpenCode (engine unnamed), OpenClaw (per-agent SQLite, optional PostgreSQL), goose, Hermes (SQLite with FTS5), Cline (per-session folder plus SQLite index). (c) File-per-event: OpenHands. Only Pi, Oh My Pi, DeepSeek and OpenCode document the log as a durable, readable interface for other programs; Claude Code tells integrators not to parse its transcripts. Ours is an append-only per-session event log in SQLite with the UI as a fold over it, closest to DeepSeek harness and OpenCode in intent.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `rollout-*.jsonl` files, each written by one process, plus a SQLite index (WAL locally, DELETE journaling on Linux NFS, `codex migrate-nfs-db`); index also holds goals and memory-processing state. | GA | https://learn.chatgpt.com/docs/environments/network-file-systems.md; https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| OpenCodex | Gateway, not a session owner: the client owns sessions. Thread-to-account affinity is in memory only; sync/restore keeps a backup manifest of each thread's original provider metadata; `ocx recover-history --legacy-openai --yes` is a broad history rewrite. | GA | https://opencodex.me/reference/architecture/; https://opencodex.me/guides/codex-integration/; https://opencodex.me/reference/adapters/ | unreviewed |
| OpenCode | Database under `~/.local/share/opencode/` (`opencode.db`, engine not named). API exposes a durable per-session event log readable after a sequence number with `follow=true` live tailing (`GET /api/experimental/session/{id}/log`); `GET /api/event` is volatile by contract. | GA; log and import/export routes experimental | https://opencode.ai/docs/troubleshooting/; https://opencode.ai/docs/api/; https://opencode.ai/docs/cli/commands/ | reviewed |
| Claude Code | Plaintext JSONL at `~/.claude/projects/<project>/<session-id>.jsonl` (project = cwd with non-alphanumerics replaced by `-`, truncated at 200 chars plus hash). Format is internal and changes between versions; use `/export`, `-p --output-format json`, hook `transcript_path` or the SDK. 30-day default retention (`cleanupPeriodDays`); `claude purge` deletes a project's transcripts, history and file-edit history. | GA | https://code.claude.com/docs/en/sessions.md; https://code.claude.com/docs/en/how-claude-code-works.md; https://code.claude.com/docs/en/cli-reference.md | reviewed |
| Claude Desktop | Same engine as Claude Code per the survey; Desktop-specific storage not documented on the pages opened. | - | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | Per-session append-only JSONL, by default checksummed concatenated zstd frames, atomic materialisation, per-batch fsync, torn-tail truncation; metadata stored separately. Old v0/v1/v2 formats translated on read, newer refused. Storage root not on that page (SDK page: `<dsh_home>/sessions` uncompressed for the minimal profile). | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/persistence | reviewed |
| Oh My Pi | Auto-saved JSONL under `~/.omp/agent/sessions/` (profiles under `~/.omp/profiles/<name>/`). Documented format: version 3, one header, id/parentId tree entries (message, model_change, compaction, branch_summary, label, ...); large images in a content-addressed blob store. | GA | https://omp.sh/docs/sessions; https://omp.sh/docs/session-format | reviewed |
| Pi | JSONL under `~/.pi/agent/sessions/` grouped by directory; entries have `id`/`parentId`, current leaf is the active branch; v1 linear, v2 tree, v3, old files auto-migrate. Documented for post-processing. `--no-session` is in-memory. | GA | https://pi.dev/docs/latest/sessions; https://pi.dev/docs/latest/session-format; https://pi.dev/docs/latest/how-pi-works | reviewed |
| Cursor | Local session storage not documented in the pages read. Cloud agents: conversation state kept indefinitely by default (Delete Agent API; Enterprise can cap); VM snapshots including cloned code deleted after 30 days of inactivity, not deletable on demand. | GA (cloud) | https://cursor.com/docs/cloud-agent/security | reviewed |
| Antigravity | CLI histories scoped to the working directory; SDK takes `save_dir` and `conversation_id` (min 32 chars). Storage path and retention not documented. | GA | https://antigravity.google/docs/cli/conversations; https://antigravity.google/docs/sdk/lifecycle | unreviewed |
| OpenHands | Default `workspace/conversations/<id>/` with `base_state.json` (rewritten whole on every change) and `events/` (one JSON file per event). Agent Server keeps `bash_events/`, `conversations/`, `project/`; Enterprise uses PostgreSQL plus object storage. Survey does not say whether persisted secrets are encrypted; agent-server page says `OH_SECRET_KEY` encrypts sensitive values. | GA | D:/sdk/guides/convo-persistence.md; D:/sdk/arch/agent-server.md; D:/enterprise/architecture.md | unreviewed |
| OpenClaw | Per-agent SQLite `~/.openclaw/agents/<agentId>/agent/openclaw-agent.sqlite`; shared state `~/.openclaw/state/openclaw.sqlite` (PostgreSQL optional). Defaults `pruneAfter` 30d, `maxEntries` 5000; `openclaw sessions cleanup --dry-run`. Gateway owns all session state; session keys route DMs, groups, cron and webhooks. | GA | https://docs.openclaw.ai/concepts/session; https://docs.openclaw.ai/tools/exec-approvals | reviewed (defaults, dmScope) |
| goose | SQLite `sessions.db` under `~/.local/share/goose/sessions/` (since 1.10.0), shared by desktop and CLI; ids `YYYYMMDD_<COUNT>`; auto-named from the first prompt. | GA | https://goose-docs.ai/docs/guides/sessions/session-management.md; https://goose-docs.ai/docs/guides/goose-cli-commands.md | unreviewed |
| Hermes Agent | SQLite `state.db` (WAL, FTS5) with full messages, tool calls, system prompt snapshot, tokens. WAL is unsafe on virtiofs/9p mounts (Docker page). | GA | /user-guide/sessions; /developer-guide/gateway-internals | reviewed |
| Zed/ACP | Zed keeps a Thread History across projects. ACP defines `session/new`, `session/load` (replays full history as updates), `session/resume` (no replay), `session/close`, `session/list`, `session/delete`; the agent decides persistence and retention. No storage format specified. | GA | https://zed.dev/docs/ai/parallel-agents.md; https://agentclientprotocol.com/protocol/v1/session-setup.md | unreviewed |
| Warp | Conversations listed in the app and CLI; synced to the Warp account; "Store AI conversations in the cloud" enables link sharing, plan-based limits, enterprise admins can disable. Local format not documented. | GA | https://docs.warp.dev/agents/cli/agent-conversations/; https://docs.warp.dev/agents/local-agents/cloud-conversations/ | unreviewed |
| Cline | `~/.cline/data/sessions/<id>/` with SQLite index `data/db/sessions.db`. The prompt-storage page names `~/.cline/data/tasks/`; possibly different layouts for extension and SDK (survey flags it). | GA | https://docs.cline.bot/cli/cli-reference.md; https://docs.cline.bot/sdk/architecture/hub-spoke.md | unreviewed |
| rizzy-harness (ours) | Append-only SQLite event log per session (WAL, fsync Full on the chat db, Normal on the mirror); UI view is a fold over events; streaming deltas are stored and kept forever (AUDIT D5, no compaction of the log). Node is the only writer, master mirrors and rejects events for sessions another node owns. | works; log growth partial | `crates/store/src/lib.rs:253-262`, `crates/core/src/view.rs:113-147`, `crates/core/src/event.rs:59-80`, `crates/node/src/session.rs:194-230` | from own inventory |

#### Resume and continue
What it is: reopening an earlier session and continuing it with its history, and what state comes back with it.
Variants: Claude Code is the most detailed (restores model, agent, permission mode with `bypassPermissions` never restored, active `/goal`; does not restore `--mcp-config`, `--add-dir` and similar flags). Codex restores saved permissions and network approvals. Hermes restores cwd but keeps the model. Closers for interrupted work differ: Claude Code does not re-run interrupted tools; DeepSeek and Ours append synthetic closers; Hermes marks a crashed gateway turn `unknown` and blocks the session until discarded. Ours has no "resume" command at all (a chat continues by sending a prompt) because it has no CLI chat.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `codex resume [ID] [--last] [--all]`, `/resume`; `--last` is scoped to the cwd unless `--all`; asks (or `tui.resume_cwd` decides) when cwd differs; restores saved permissions and network approvals subject to overrides and managed policy; `codex exec resume`. Since 0.167.0 a chat whose provider changed can be read and re-prompted in a new chat. | GA | https://learn.chatgpt.com/docs/developer-commands.md; https://learn.chatgpt.com/docs/non-interactive-mode.md; https://learn.chatgpt.com/docs/changelog | unreviewed |
| Claude Code | `--continue`/`-c`, `--resume`/`-r` (picker, ID, name or `.jsonl` path), `--from-pr`, `/resume`. Same session ID, appends. Restores history (from the last compaction row), model, `--agent`, permission mode (never `bypassPermissions`), `/goal`, unexpired session cron tasks; interrupted tool calls are not rerun; dead background work shows as "didn't finish" notes. Resuming in two terminals without forking interleaves both into one transcript. | GA | https://code.claude.com/docs/en/sessions.md; https://code.claude.com/docs/en/cli-reference.md | reviewed |
| Claude Desktop | Session sidebar with group, sort, filter, archive. Resume semantics beyond that not stated on the pages opened. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | Resume reads the log with a write handle and appends closers (missing tool errors, open step end, synthetic interrupted turn end). One live writer (`SessionAlreadyOwnedError`). | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/persistence | reviewed |
| Oh My Pi | `omp -c`, `-r [id]`, `/resume`, auto-resume setting; picker searches deep text; interrupted/aborted/error labels; `--session-dir`. | GA | https://omp.sh/docs/sessions | reviewed |
| Pi | `-c` latest in cwd; `-r`/`/resume` picker; `--session <path\|id>`; resumes in the original cwd, and if that is gone offers continue here, fork, or cancel. | GA | https://pi.dev/docs/latest/sessions; https://pi.dev/docs/latest/cli | reviewed |
| Cursor | CLI `agent ls`, `agent resume`, `--continue`, `--resume=<id>`, `/resume`. | GA | https://cursor.com/docs/cli/overview; https://cursor.com/docs/cli/reference/slash-commands | unreviewed |
| Antigravity | `/resume` picker, `-c`, `--conversation <id>`; histories scoped to the working directory. | GA | https://antigravity.google/docs/cli/conversations; https://antigravity.google/docs/cli/reference | unreviewed |
| OpenHands | Resume by recreating a `Conversation` with the same id and persistence dir. | GA | D:/sdk/guides/convo-persistence.md | unreviewed |
| OpenClaw | Gateway-owned sessions; reset policy none by default (compaction manages growth), optional daily (04:00), idle or manual `/new`. | GA | https://docs.openclaw.ai/concepts/session | reviewed |
| goose | `goose session -r`; `--edit` opens history as YAML. | GA | https://goose-docs.ai/docs/guides/sessions/session-management.md | unreviewed |
| Hermes Agent | `-c` latest per terminal; `-r` by id, title or `latest`; `--in <dir>`; restores cwd, re-reads current config but keeps the model. Crash mid-turn marks the turn `unknown`, not replayed, session blocks until discarded. Goal and heartbeat state restored by `/resume`. | GA | /user-guide/sessions; /developer-guide/gateway-internals; /user-guide/features/goals | reviewed |
| Zed/ACP | ACP `session/load` replays history, `session/resume` restores without replay; Zed Thread History. | GA | https://agentclientprotocol.com/protocol/v1/session-setup.md; https://zed.dev/docs/ai/parallel-agents.md | unreviewed |
| Warp | `warp --resume <TOKEN>`, `/conversations`. | GA | https://docs.warp.dev/agents/cli/agent-conversations/ | unreviewed |
| Cline | `cline --id <id>`, `cline history`; ACP clients can restore threads; can attach to a task already running in another Cline app. | GA | https://docs.cline.bot/cli/cli-reference.md; https://docs.cline.bot/sdk/architecture/hub-spoke.md | unreviewed |
| rizzy-harness (ours) | A chat continues by sending another prompt; history rebuilt from the log, interrupted tool calls closed synthetically. Link reconnect replays events by `Resume{after}`. No "resume last session" flag. | works | `crates/agent-native/src/lib.rs:333-392`, `crates/node/src/lib.rs:323-330`, `:590-624`, `crates/master/src/ws.rs:1-8` | from own inventory |

#### Fork: copy a session into a new session
What it is: creating a second, independent session from a prefix (or all) of an existing one.
Variants: what is copied differs. Antigravity and OpenHands say explicitly that the working tree is not forked (shared workspace). Claude Code `/branch` switches the same process; `--fork-session` starts a new process without session grants; `/fork` makes a background session that creates its own worktree. Codex can fork into a managed worktree. DeepSeek marks inherited events with a `session/end-seed` marker. goose offers fork as one of two outcomes of editing a message. Ours has none.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `codex fork` / `/fork`; `--worktree` forks into a managed Git worktree (needs explicit id). API `thread/fork` copies history up to `lastTurnId` or makes an in-memory `ephemeral` fork. Attached thread forks experimental. | GA (attached forks experimental) | https://learn.chatgpt.com/docs/developer-commands.md; https://learn.chatgpt.com/docs/app-server.md; https://learn.chatgpt.com/docs/codex-sdk.md | unreviewed |
| OpenCode | Sessions can be forked before a message (and moved to another directory). | GA (routes experimental) | https://opencode.ai/docs/api/; https://opencode.ai/docs/cli/commands/ | reviewed |
| Claude Code | `/branch [name]` copies the transcript into a new session ID in the same process (original stays); `--continue\|--resume --fork-session` forks into a new process; `/fork [prompt]` copies into a new background session that creates its own worktree before editing; `/subtask` is a forked background subagent. SDK `forkSession` rewrites ids and remaps UUIDs. A fork shares the real working tree. | GA | https://code.claude.com/docs/en/sessions.md; https://code.claude.com/docs/en/commands.md; https://code.claude.com/docs/en/agent-sdk/sessions.md | reviewed |
| DeepSeek harness | `fork` creates a child session from an inclusive prefix; cut inside an open turn rejected; child records parent and inherited count; `session/end-seed` marker separates inherited from own events. No user-facing "fork from here" UI documented. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session | unreviewed |
| Oh My Pi | `/fork` clones the whole session to a new file; `omp --fork`; `/branch` rewinds to before a message (see session trees). | GA | https://omp.sh/docs/session-tree | unreviewed |
| Pi | `/fork` creates a new file from an earlier user message; `/clone` copies the active branch; `--fork`. | GA | https://pi.dev/docs/latest/sessions; https://pi.dev/docs/latest/cli | reviewed |
| Cursor | `/fork` slash command in the CLI. | GA | https://cursor.com/docs/cli/reference/slash-commands | unreviewed |
| Antigravity | `/fork` (alias `/branch`) copies the conversation up to the current turn, not the git checkout; `/fork <project_id>` copies into another project. | GA | https://antigravity.google/docs/cli/conversations; https://antigravity.google/docs/features | unreviewed |
| OpenHands | SDK `fork()` deep-copies events, agent and skill state into a new idle conversation sharing the same workspace (metrics reset); `POST /api/conversations/{id}/fork`; Canvas "Branch from here". Workspace is shared, so file state is not forked. | GA | D:/sdk/guides/convo-fork.md; D:/openhands/usage/agent-canvas/conversations.md | unreviewed |
| goose | CLI `--resume --fork` copies a session; desktop message edit offers "Edit in Place" (deletes later messages) or "Fork Session". | GA | https://goose-docs.ai/docs/guides/sessions/in-session-actions.md; https://goose-docs.ai/docs/guides/sessions/session-management.md | unreviewed |
| Hermes Agent | `/rewind` transcript browser: press `f` to fork a new session before a prompt; `/branch [title]` in gateway CLI mode. Sessions page has no dedicated branching section. | GA | /user-guide/cli; /user-guide/sessions | reviewed (sessions/resume) |
| Warp | `/handoff` forks a local conversation to a cloud run (a fork toward another host, not a local branch). | GA | https://docs.warp.dev/platform/handoff/ | corrected |
| Zed/ACP | Fork not documented; "New From Summary" and edit-and-resubmit exist. ACP session fork exists only as an RFD title in the index. | - | https://zed.dev/docs/ai/parallel-agents.md; https://agentclientprotocol.com/llms.txt | unverified (RFD title only) |
| Cline | Fork or branching of a conversation not documented. | - | https://docs.cline.bot/cli/cli-reference.md | unreviewed |
| rizzy-harness (ours) | None. Parallel answers are variants of one message, not forks; no edit-and-re-run from here. | none | grep for fork finds nothing (own inventory section 6) | from own inventory |

#### In-file session trees (branching without copying)
What it is: the session log is a tree; rewinding and continuing creates a sibling branch in the same file instead of a new session.
Variants: only the two Pi-lineage harnesses. Both use `id`/`parentId` per entry and a navigator. Oh My Pi adds labels and branch summaries; Pi keeps all branches and one active leaf. Neither reverts file changes when you move between branches.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Oh My Pi | `/tree` navigator with filters, search, labels (bookmarks), branch summaries (none/summarize/custom prompt); `/branch` rewinds to before a message. Tool-made file changes are not reverted by branching. | GA | https://omp.sh/docs/session-tree; https://omp.sh/docs/session-format | unreviewed |
| Pi | `/tree` navigates inside one file keeping all branches (selecting a user message loads it into the editor); one active leaf, branches never deleted; double Escape defaults to `tree`; entries include `context_edit` and `branch_summary`. | GA | https://pi.dev/docs/latest/sessions; https://pi.dev/docs/latest/settings | reviewed |
| rizzy-harness (ours) | None; log is linear per session. | none | not in the inventory beyond "fork none" (`own-rizzy-harness.md` section 6) | from own inventory |

#### Conversation rewind and edit-and-resubmit
What it is: cutting the conversation back to an earlier message and continuing, possibly with an edited prompt, without necessarily touching files.
Variants: Codex `thread/revert` drops turns and explicitly does not touch files; Claude Code, Cline and Cursor tie it to a checkpoint menu where files are an option; goose "Edit in Place" deletes later messages; OpenCode `/undo` restores files and returns the prompt to the composer in one step. OpenClaw's docs say rewinding does not undo external actions.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | API `thread/revert` drops a turn and every later turn from history; does not touch files; not supported for ephemeral threads. `thread/rollback` was removed. | GA | https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| OpenCode | `/undo` stages a rollback of file paths and returns the prompt to the composer; resubmitting commits it. Rejected while a session runs. | GA | https://opencode.ai/docs/troubleshooting/ (snapshots page: https://opencode.ai/docs/snapshots/) | reviewed |
| Claude Code | `/rewind` (aliases `/checkpoint`, `/undo`) or double Esc: Restore code and conversation; Restore conversation; Restore code; Summarize from here; Summarize up to here. After restoring conversation the prompt is put back in the input. Rewound messages can return if you exit before the next prompt. | GA | https://code.claude.com/docs/en/checkpointing.md | reviewed |
| Oh My Pi | Agent-facing `checkpoint` and `rewind` tools (off by default, mainly top-level sessions) mark and return to a conversational checkpoint; no file revert. | GA (off by default) | https://omp.sh/docs/tools | unreviewed |
| Pi | Selecting a message in `/tree` loads it into the editor to resend. | GA | https://pi.dev/docs/latest/sessions | reviewed |
| Cursor | `/rewind` in the CLI; IDE checkpoint restore reverts files only, messages stay. | GA | https://cursor.com/docs/cli/reference/slash-commands; https://cursor.com/docs/agent/overview | unreviewed |
| Antigravity | `/rewind` (`/undo`) "rolls back to a checkpoint"; whether it restores files or only conversation is not stated. | GA; behaviour unverified | https://antigravity.google/docs/cli/conversations; https://antigravity.google/docs/features | unverified |
| OpenHands | Canvas "Branch from here" on a user message starts at that message's parent and loads the text for editing (a branch, not a destructive rewind). | GA | D:/openhands/usage/agent-canvas/conversations.md | unreviewed |
| OpenClaw | Control UI can edit and resend failed replies; docs say rewinding does not undo external actions. | GA | https://docs.openclaw.ai/web/control-ui | unreviewed |
| goose | Desktop "Edit in Place" deletes later messages; CLI `--edit` opens history as YAML. | GA | https://goose-docs.ai/docs/guides/sessions/in-session-actions.md | unreviewed |
| Hermes Agent | Esc-Esc or `/rewind` transcript browser: edit a prior prompt to cut back; `/undo` rewinds the conversation only. | GA | /user-guide/cli; /user-guide/checkpoints-and-rollback | reviewed |
| Zed/ACP | Message edit-and-resubmit in the Agent Panel; "New From Summary". | GA | https://zed.dev/docs/ai/parallel-agents.md | unreviewed |
| Cline | Restore "Task only" drops later messages and keeps files; editing an earlier message with "Restore All" reverts files then resubmits. | GA | https://docs.cline.bot/core-workflows/checkpoints.md | reviewed |
| rizzy-harness (ours) | None for the conversation ("no edit-and-resume from an earlier message"); turn-level file undo/rewind exists (see checkpoints). | none | `own-rizzy-harness.md` gap list; `crates/node/src/session.rs:567-651` | from own inventory |

#### Export of a session
What it is: writing a session out as a file for reading, archiving or analysis.
Variants: formats differ widely: Codex `/export` Markdown; Claude Code a rendered plain-text transcript (and discourages parsing the JSONL); Oh My Pi self-contained HTML and a `/dump` of transcript plus request JSON; Pi HTML or JSONL; Hermes JSONL, Markdown, HTML or a Claude-Code-compatible `trace` for Hugging Face, with `--redact`; OpenCode `session export --sanitize`; goose Markdown, JSON, YAML and HTML (v1.54.0); OpenHands Markdown or HTML generated client-side.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `/export` as Markdown; `codex archive`/`unarchive`/`delete` (`--force` delete works only by UUID). | GA | https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| OpenCode | CLI `session export --sanitize` and `session import`. | GA (import/export routes experimental) | https://opencode.ai/docs/cli/commands/ | reviewed |
| Claude Code | `/export [filename]` writes or copies a rendered plain-text transcript; `-p --output-format json\|stream-json`; hook `transcript_path`; Agent SDK helpers list/read/rename/tag/fork. | GA | https://code.claude.com/docs/en/sessions.md | unreviewed |
| Oh My Pi | `/export` self-contained HTML; `/dump` copies transcript plus request JSON (may contain secrets; that warning is not confirmed on the sessions page). | GA | https://omp.sh/docs/sessions; https://omp.sh/docs/slash | unverified (dump warning) |
| Pi | `/export` writes HTML or JSONL; `pi --export <input> [output]`. Docs warn exports can contain prompts, file contents and tool arguments. | GA | https://pi.dev/docs/latest/sessions; https://pi.dev/docs/latest/cli | reviewed |
| OpenHands | Export as Markdown or self-contained HTML (client-side from loaded events); a "Download trajectory" API endpoint exists. | GA | D:/openhands/usage/agent-canvas/conversations.md | unreviewed |
| goose | Export markdown/JSON/YAML (desktop JSON; HTML from v1.54.0); import desktop only; diagnostics report. | GA | https://goose-docs.ai/docs/guides/sessions/session-management.md; https://github.com/aaif-goose/goose/releases | unreviewed |
| Hermes Agent | Export as `jsonl`, `md`/`qmd`, `html` or `trace` (Claude Code JSONL for Hugging Face), `--redact`, per-prompt-only options; prune/archive/pin commands. | GA | /user-guide/sessions | reviewed |
| Warp | CLI `/export-to-clipboard`, `/export-to-file`. | GA | https://docs.warp.dev/agents/cli/agent-conversations/ | unreviewed |
| Cline | `cline history` (list, export, delete, update). | GA | https://docs.cline.bot/cli/cli-reference.md | unreviewed |
| Cursor | A "Shared Transcripts" help page is listed; it was not opened. | unclear | https://cursor.com/help/ai-features/shared-transcripts.md | unverified |
| rizzy-harness (ours) | None (only copy-message and "Copy CSV" for tables). | none | no route in `crates/master/src/http.rs:26-175` | from own inventory |

#### Session sharing by link
What it is: giving someone else a view of a session via a URL.
Variants: client-side encryption with the key in the URL fragment (Oh My Pi); upload to a private gist or Radius artifact (Pi); server-side visibility controls (Claude Code cloud sessions, OpenHands Cloud, Warp); OpenCode removed it.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | Discontinued in v1.19.1 (15 Oct 2026): existing share links and commands are gone, import from share URLs unsupported. The V2 config schema still carries a `share` field; a search snippet says it is accepted but not implemented. | deprecated (removed) | https://opencode.ai/changelog/v1.19.1; https://opencode.ai/docs/api/; https://opencode.ai/docs/llms.txt | corrected |
| Claude Code | Cloud sessions: Team/Enterprise Private or Team visibility with repo-access verification on by default; Pro/Max/Ultra Private or Public (any logged-in claude.ai user), verification off by default, warning that sessions may contain credentials. Shared views are snapshots. | GA | https://code.claude.com/docs/en/claude-code-on-the-web.md | unreviewed |
| Oh My Pi | `/share` encrypts client-side, key in URL fragment, uploads a blob; `omp share --gist`. | GA | https://omp.sh/docs/sessions; https://omp.sh/docs/slash | reviewed |
| Pi | `/share` uploads and returns a viewer link via a Radius artifact when configured or a private GitHub gist; `sessionShareDestination` forces one; separate `pi-share-hf` tool publishes to Hugging Face (README mention only). | GA | https://pi.dev/docs/latest/sessions; https://pi.dev/docs/latest/cli; https://github.com/earendil-works/pi | reviewed |
| OpenHands | Cloud sharing: public link for a personal workspace, org-only for a team org. | GA | D:/openhands/usage/agent-canvas/conversations.md | unreviewed |
| Warp | With "Store AI conversations in the cloud", conversations can be shared by link (team, individuals, anyone); enterprise admins can disable. | GA | https://docs.warp.dev/agents/local-agents/cloud-conversations/ | unreviewed |
| rizzy-harness (ours) | None; chats are private to their owner by invariant. | none | not in the inventory (no share route in `crates/master/src/http.rs:26-175`) | from own inventory |

#### Search across sessions and the session picker
What it is: finding an earlier session by text, title, branch or PR.
Variants: full-text index (Hermes FTS5 store, DeepSeek SQLite FTS index, Ours FTS5) versus picker filtering (Claude Code, Oh My Pi, Zed fuzzy title). DeepSeek also exposes search as opt-in model-facing history tools; OpenClaw has a `sessions_search` tool. Claude Code can find a session by pasting a PR URL.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Picker defaults to the current worktree; keys for all projects, all worktrees, current branch; type to search (paste a PR/MR URL to find the session that created it); `Space` preview. A "Resume this conversation?" dialog estimates usage-limit cost of resuming after a cache-expiring break. | GA | https://code.claude.com/docs/en/sessions.md | reviewed |
| DeepSeek harness | SQLite full-text index over messages, tool calls and results, todos and status details (reasoning and stream chunks excluded); cross-session and in-session search with cursor paging; model-facing history tools opt-in. | experimental (model tools), unclear (index) | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session-query; https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| Oh My Pi | Resume picker searches deep text; pinned sessions. | GA | https://omp.sh/docs/sessions | reviewed |
| Antigravity | App Cmd/Ctrl+K full-text search over prompts and responses, project files and commands; conversation navigator; archived filter. | GA | https://antigravity.google/docs/features; https://antigravity.google/docs/changelog | unreviewed |
| OpenClaw | `sessions_search` transcript search tool. | GA | https://docs.openclaw.ai/concepts/session-tool | unreviewed |
| goose | Search across sessions limited to 10 messages in desktop; no regex. | GA | https://goose-docs.ai/docs/guides/sessions/session-management.md | unreviewed |
| Hermes Agent | SQLite `state.db` has FTS5; a user-facing search command is not described in the sessions entry; titles auto-generated. | GA | /user-guide/sessions | reviewed |
| Zed/ACP | Thread History across projects with fuzzy title search. | GA | https://zed.dev/docs/ai/parallel-agents.md | unreviewed |
| rizzy-harness (ours) | FTS5 over titles/messages per workspace, own chats only, plus Media/Artifacts/Projects tabs and "similar earlier chats". Incognito chats are excluded. | works | `crates/master/src/search.rs:1-6`, `:371`, `docs/sidebar-search.md` | from own inventory |

#### Moving a session between machines or surfaces (handoff)
What it is: continuing the same task on another host or surface (local, cloud, SSH host, messaging channel), with or without the git state.
Variants: Codex moves chat plus Git state between the local machine and a connected remote host (needs a saved project for the same repo on both; Codex Cloud not supported). Warp snapshots tracked and untracked changes and forks to a cloud run. Claude Code teleports one way (cloud to terminal, needs clean git state and the same repo) and Desktop pushes local to cloud; the copy then diverges. Cursor hands off local to cloud and back. Hermes hands a live CLI session to a messaging channel. Claude Code's self-hosted runners resume a released session on a fresh runner from a fresh clone, losing unpushed work. Ours has no session migration between nodes in the inventory.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Handoff moves a chat and its Git state between the local computer and a connected remote host; running response interrupted first; Codex Cloud not supported. Separately (Desktop, CLI `/worktree`) Handoff moves a chat between Local and Worktree. | GA | https://learn.chatgpt.com/docs/remote-connections.md; https://learn.chatgpt.com/docs/environments/git-worktrees.md | corrected |
| OpenCode | Sessions can be moved to another directory (not another machine). | GA | https://opencode.ai/docs/cli/commands/ | reviewed |
| Claude Code | `--teleport`/`/teleport` pulls a cloud session branch and conversation into the terminal (clean git, same repo not a fork, branch pushed, same account; compacted-away messages not copied; later local work does not appear in the cloud). CLI-to-cloud is one-way from CLI per the survey. Self-hosted runners (beta): released sessions resume on a fresh runner from a fresh clone; `--push-outcome-on-release` pushes branches best-effort. | GA (self-hosted beta) | https://code.claude.com/docs/en/claude-code-on-the-web.md; https://code.claude.com/docs/en/self-hosted-environments-deploy.md | unreviewed |
| Claude Desktop | "Open in" sends a local session to the cloud; `/desktop` moves a CLI session into the app. | GA | https://code.claude.com/docs/en/desktop; https://claude.com/download | unreviewed |
| Oh My Pi | `/move` relocates a session (directory). | GA | https://omp.sh/docs/sessions | unreviewed |
| Cursor | Local to cloud ("Handoff to Cloud Agents", Jan 2026) or back; `/in-cloud`, `/autopilot`; `/remote-control` lets a phone drive the computer. | GA (no label) | https://cursor.com/docs/agent/agents-window; https://cursor.com/docs/cloud-agent/mobile | unreviewed |
| Hermes Agent | `/handoff <platform>` moves a live CLI session to a messaging home channel. | GA | /user-guide/sessions | reviewed |
| Warp | `/handoff [prompt]` forks a local conversation to a cloud run and snapshots tracked and untracked changes; also cloud-to-cloud and cloud-to-local (`/continue-locally`). Requires "Store AI conversations in the cloud"; BYOK keys not copied. Cloud agents periodically save handoff checkpoints (changelog claim, not checked). | GA | https://docs.warp.dev/platform/handoff/; https://docs.warp.dev/agents/cli/cloud-and-orchestration/; https://docs.warp.dev/platform/handoff/local-to-cloud/ | corrected |
| Cline | Hub/spoke: sessions outlive clients and several UIs can attach; CLI can attach to a task running in another app. | GA | https://docs.cline.bot/sdk/architecture/hub-spoke.md; https://github.com/cline/cline/releases | unreviewed |
| rizzy-harness (ours) | Not in the inventory. Sessions are owned by one node; no migration between nodes is listed. | none | not in the inventory | from own inventory |

#### Importing sessions from other harnesses
What it is: reading another harness's saved sessions.
Variants: Oh My Pi reads Claude Code and Codex session files; Cline Desktop imports Claude Code and Codex conversations; Zed imports from configured external agents (imported threads arrive archived; Cursor and Gemini CLI unsupported); Hermes' `agent-importers` plugin migrates instructions, MCP servers, skills and rules from several harnesses (not transcripts as such).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Oh My Pi | `--from-claude`, `--from-codex`, `/resume @claude`. | GA | https://omp.sh/docs/sessions | reviewed |
| Cline | Desktop imports Claude Code and Codex conversations. | GA | https://docs.cline.bot/usage/cline-desktop.md | unreviewed |
| Zed/ACP | Imports sessions from configured external agents; duplicates and sessions without a working dir skipped; imported threads arrive archived. | GA | https://zed.dev/docs/ai/external-agents.md | unreviewed |
| Hermes Agent | `agent-importers` plugin migrates from OpenClaw, Claude Code, Codex CLI, Cursor and others: instructions to memory, MCP servers and skills carried, per-item plan shown first, `--sync`. Secrets excluded unless `--migrate-secrets`. | GA | /user-guide/import-from-other-agents | unreviewed |
| OpenCode | `session import` (OpenCode's own export format). | GA (experimental route) | https://opencode.ai/docs/cli/commands/ | reviewed |
| rizzy-harness (ours) | None. | none | no import route (`own-rizzy-harness.md` section 6) | from own inventory |

#### Archive, delete, retention and ephemeral sessions
What it is: lifecycle controls for stored sessions.
Variants: Claude Code time-based sweep (30 days) and a purge command; Codex `--ephemeral` and `history.persistence`; OpenClaw `pruneAfter`/`maxEntries` plus incognito threads in memory; Ours has secure-delete overwrite and chat-mode incognito (excluded from search and memory, purged after idle TTL); Cursor and Warp have cloud retention rules. Claude Desktop auto-archives after the PR merges and removes worktrees about 30 days after archive.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `codex archive`/`unarchive`/`delete`, `/rename`; `codex exec --ephemeral` skips rollout files; `history.persistence = "none"` and `history.max_bytes` control `history.jsonl`. | GA | https://learn.chatgpt.com/docs/developer-commands.md; https://learn.chatgpt.com/docs/non-interactive-mode.md; https://learn.chatgpt.com/docs/config-file/config-advanced.md | unreviewed |
| Claude Code | 30-day default retention (`cleanupPeriodDays`), `claude purge`, `--no-session-persistence` for `-p`, `CLAUDE_CODE_SKIP_PROMPT_HISTORY`; `-p` runs are persisted by default. Cloud sessions: archive (hides) and permanent delete. | GA | https://code.claude.com/docs/en/sessions.md; https://code.claude.com/docs/en/glossary.md; https://code.claude.com/docs/en/claude-code-on-the-web.md | reviewed (storage) |
| Claude Desktop | Sessions list with archive and auto-archive after the PR merges or closes (cloud and running sessions excluded); worktrees removed about 30 days after archive (uncommitted changes can be lost). | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | Sidebar ordering, pinning and archive filter (archived sessions must be restored to open); LLM-generated titles. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-client | unreviewed |
| Oh My Pi / Pi | `--no-session` in-memory runs; pinned sessions (Oh My Pi). | GA | https://omp.sh/docs/sessions; https://pi.dev/docs/latest/sessions | reviewed |
| Cursor | Cloud conversation state indefinite by default, Delete Agent API, Enterprise cap; VM snapshots deleted after 30 days of inactivity, not on demand. The privacy page wording differs (see section 7). | GA | https://cursor.com/docs/cloud-agent/security | reviewed |
| OpenHands | Archive hides without deleting (server-side tags). | GA | D:/openhands/usage/agent-canvas/conversations.md | unreviewed |
| OpenClaw | `pruneAfter` 30d, `maxEntries` 5000; incognito threads kept in memory, expire after 24h or restart (Codex runtime's own log still records messages). | GA | https://docs.openclaw.ai/concepts/session | reviewed |
| goose | List, rename, remove (permanent). | GA | https://goose-docs.ai/docs/guides/sessions/session-management.md | unreviewed |
| Hermes Agent | Prune/archive/pin commands, optional auto-prune (off by default), `optimize`, journal-mode switching. | GA | /user-guide/sessions | reviewed |
| Zed/ACP | Archive/restore, permanent delete; archiving a thread saves worktree git state and removes the worktree. ACP `session/delete` leaves retention to the agent. | GA | https://zed.dev/docs/ai/parallel-agents.md; https://agentclientprotocol.com/protocol/v1/session-setup.md | unreviewed |
| Warp | Cloud copies subject to plan storage limits; oldest cloud copies removed for free users. | GA | https://docs.warp.dev/agents/local-agents/cloud-conversations/ | unreviewed |
| rizzy-harness (ours) | Deleting a chat overwrites events (secure delete) and clears media keys; incognito (chat mode only) excluded from search, memory, images, purged after `incognito_ttl_minutes` idle (default 1440). No archive listed. | works | `crates/node/src/lib.rs:537-571`, `crates/store/src/lib.rs:485-490`, `crates/node/src/lib.rs:572-589` | from own inventory |

#### Single writer, concurrency guard and crash recovery
What it is: preventing two processes from appending to one session and repairing a session after a crash.
Variants: Ours, DeepSeek and OpenClaw enforce one writer in the storage layer or by a durable claim; Claude Code's guard is advisory (the user can override, and two terminals can interleave); Codex uses one process per rollout file; Hermes marks crashed turns `unknown`.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Each rollout file written by one process (NFS guidance for the SQLite index). | GA | https://learn.chatgpt.com/docs/environments/network-file-systems.md | unreviewed |
| Claude Code | VS Code extension warns when another process has the conversation open ("Open here anyway"); the warning is advisory. Resuming the same session in two terminals interleaves them. Transcript-write failures now warn instead of losing data (2.1.217). | GA | https://code.claude.com/docs/en/vs-code.md; https://code.claude.com/docs/en/sessions.md; https://code.claude.com/docs/en/changelog.md | unreviewed |
| DeepSeek harness | One live writer per session (`SessionAlreadyOwnedError`); torn-tail truncation; closers appended on resume. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/persistence | reviewed |
| OpenClaw | Durable active-writer claim checked in the same transaction as every transcript write (state-dir lock not on the queue page). | GA | https://docs.openclaw.ai/concepts/session | corrected |
| Hermes Agent | Gateway crash mid-turn marks the turn `unknown`, not replayed; session blocks until discarded; one writer per Hermes home is a stated rule. | GA | /user-guide/sessions; /developer-guide/gateway-internals | reviewed |
| rizzy-harness (ours) | Only the owning node appends, contiguous `seq`; master rejects events for sessions another node owns; open turns closed as `interrupted` on load. | works | `crates/node/src/session.rs:154-191`, `:194-230`, `docs/security-node-link.md` | from own inventory |

#### Pluggable external session storage
What it is: letting the host replace local transcript files with its own store.
Variants: only Claude Code documents this, as an SDK feature.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | SDK `SessionStore` with required `append(key, entries)` and `load(key)`, optional list/delete/`listSubkeys`. Key is `{projectKey, sessionId, subpath?}`; entries are opaque JSON that must load back in order. Ships in-memory and reference adapters (S3, Redis, Postgres) and a conformance suite. CLI always writes local disk first and the SDK mirrors each batch (best-effort, 3 attempts, then `system/mirror_error` and the batch is dropped); on resume-from-store the transcript is materialised in a temporary config dir and deleted at run end. Incompatible with `persistSession: false` and with file checkpointing. | GA (some knobs alpha) | https://code.claude.com/docs/en/agent-sdk/session-storage.md | unreviewed |
| rizzy-harness (ours) | Not in the inventory; storage is the node/master SQLite pair. | none | not in the inventory | from own inventory |

#### File checkpoints and undo: the mechanism
What it is: automatic capture of file state around agent edits so a turn can be undone or a session rewound, separate from the user's own VCS.
Variants (the central comparison for this reader):
- Shadow git repository: Cline (full state incl. untracked, commit after each tool use, in a repo separate from the project), OpenCode (a separate Git object store in the data dir, snapshot before and after each model step, with changed paths recorded on the assistant message), Hermes (one shared bare repo with a ref per project, opt-in, snapshot before `write_file`/`patch` and destructive shell commands, at most one per directory per turn), Ours (a shadow repo per folder, snapshots before and after each agentic turn, never touching the user's `.git`, git calls with cleared env and hooks off).
- Non-git file backups: Claude Code (snapshots before each file-editing tool call, per prompt checkpoint, saved with the conversation, 100 most recent kept) and Cursor (checkpoints "stored locally, separate from Git"; mechanism not described). Claude Code's reviewers confirm "checkpoints" are file-edit snapshots, not git.
- Tool-level only: OpenHands `file_editor` has `undo_edit`; nothing for workspace snapshots.
- None documented: Codex (docs tell users to create Git checkpoints themselves), DeepSeek harness (each page says it does not cover this), Oh My Pi, Pi (example extension names `git-checkpoint.ts` only), OpenClaw, goose (advises Git), Warp (only per-hunk revert in Code Review and handoff snapshots), Zed (removed, next entry).
- Event-log difference: the Ours and Claude Code checkpoints are keyed to turns/prompts in the session log; OpenCode keys snapshots to model steps and attributes paths to steps, so it restores only paths changed by the assistant after the boundary (shell side effects and ignored files not reverted).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Nothing documented for automatic file checkpoints in the CLI; docs tell users to make Git checkpoints. `thread/revert` does not revert files. | n/a | https://learn.chatgpt.com/docs/codex/cli.md; https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| OpenCode | A Git object store separate from the repo (under the data dir) snapshots before and after each model step; `/undo` restores only paths attributed to assistant steps after the boundary. Covers the active directory in a git worktree; tracked files plus untracked non-ignored files up to 2 MiB. Outside git only conversation rollback. Rejected while a session runs. Snapshot objects contain file contents (treat the data dir as sensitive). Disable with `"snapshots": false`. | GA | https://opencode.ai/docs/snapshots/ (also https://opencode.ai/docs/troubleshooting/) | reviewed |
| Claude Code | Checkpoint captured before each user prompt that starts a turn; snapshots taken before file-editing tools change a file. 100 most recent kept (each file's first snapshot kept as VS Code diff baseline); saved with the conversation so `/rewind` works after resume; swept about 30 days after last save. Not git. | GA | https://code.claude.com/docs/en/checkpointing.md; https://code.claude.com/docs/en/how-claude-code-works.md | reviewed |
| Claude Desktop | Checkpoint/rewind in the desktop UI: nothing documented on the pages opened (cloud page mentions "Rewind to here" for self-hosted runners). | - | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | Nothing documented in persistence, session or filesystem pages; each says it does not cover it. | unclear | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/persistence | unreviewed |
| Oh My Pi | Nothing documented for file-level undo; the sessions page describes no file checkpoints. | unclear | https://omp.sh/docs/sessions | unreviewed |
| Pi | No built-in; example extensions `git-checkpoint.ts` and `auto-commit-on-exit.ts` (names only, contents not opened); docs advise version control or backups. | unclear | https://pi.dev/docs/latest/extensions; https://pi.dev/docs/latest/quickstart | unreviewed |
| Cursor | Checkpoints created automatically before significant changes; restore reverts files only, messages stay; stored locally, separate from Git. | GA | https://cursor.com/docs/agent/overview | unreviewed |
| Antigravity | `/rewind` mentions "a checkpoint"; storage location and whether files are restored are not documented. | GA; unverified | https://antigravity.google/docs/cli/conversations | unverified |
| OpenHands | Nothing documented for workspace checkpoints; the `file_editor` tool has `undo_edit`. | unclear | D:/cookbook/workspace-isolation.md | unreviewed |
| OpenClaw | Checkpoint/rewind: nothing documented on pages read; managed-worktree snapshot refs exist for git state (see worktree lifecycle). | unclear | https://docs.openclaw.ai/web/control-ui | unreviewed |
| goose | Nothing documented; file-management page advises Git. | unclear | https://goose-docs.ai/docs/guides/file-management.md | reviewed |
| Hermes Agent | One shared bare git repo `~/.hermes/checkpoints/store/` with a ref per project; snapshots before `write_file`/`patch` and before destructive terminal commands (`rm`, `mv`, `sed -i`, redirects, `git reset`...), at most one per directory per turn. `/rollback` lists, restores (skips files the user edited since unless `--all`), diffs, restores one file; a pre-rollback snapshot makes restore undoable. Never snapshots `/` or `$HOME`; disabled on container backends. Opt-in since v2. | GA (off by default) | /user-guide/checkpoints-and-rollback | reviewed |
| Zed/ACP | Removed (see next entry). | deprecated (removed) | https://zed.dev/releases/stable | corrected |
| Warp | Nothing documented for checkpoints/undo of agent edits; per-hunk revert in the Code Review panel and handoff snapshots only. | nothing documented | https://docs.warp.dev/code/code-review/ | unreviewed |
| Cline | A shadow git repo separate from the project's commits the full file state (including untracked) after each tool use. Compare opens a diff; Restore offers Files only, Task only, or Files and Task (IDE page names the options slightly differently). On by default; toggle in Feature Settings. SDK lists checkpoints; Desktop/CLI parity unclear; CLI `/undo` exists in the TUI, unexplained. | GA (IDE) | https://docs.cline.bot/core-workflows/checkpoints.md; https://docs.cline.bot/usage/ide.md | reviewed |
| rizzy-harness (ours) | Each agentic turn snapshots the folder before and after into `<data>/checkpoints/<hash>.git` (shadow repo, reads the user's objects via alternates); undo one turn or rewind to a turn; works for non-git folders; forced restore when files changed since; latest 10 kept (`checkpoints_keep`). Every git call has cleared env, no system/global config, hooks/fsmonitor/pager/credential helper off, 120 s timeout. | works | `crates/node/src/session.rs:476-530`, `:567-651`, `crates/node/src/git.rs:41-60`, `:431`, `:659-800` | from own inventory |

#### Checkpoint coverage, retention, costs and removals
What it is: what a checkpoint system does not restore, how long it keeps snapshots, and what it costs.
Variants: Zed removed per-prompt Git checkpoints (1.27.0, 2026-11-04) because taking one before every message slowed or hung the agent in large Git repositories. Cline documents large-repo slowness and does not document exclusions, caps, pruning or disk location. Hermes publishes hard caps. Claude Code publishes the most precise non-coverage list. Ours has a count cap but no size cap or exclusion list in the inventory.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Zed/ACP | "Restore Checkpoint" removed in 1.27.0 (2026-11-04); undo is by rejecting edits or using Git. The Agent Panel page is silent about checkpoints (no removal note). 1.25.2 (Oct 21 2026) and 1.23.3 (Oct 12 2026) notes still mention Git checkpoints. | deprecated (removed) | https://zed.dev/releases/stable; https://zed.dev/docs/ai/agent-panel | corrected |
| Claude Code | Not tracked: files changed only by Bash (unless the file was previously edited by a file tool), subagent edits (only a foreground `context: fork` skill is restored), manual and concurrent-session edits, mid-turn messages, remote side effects; symlinked and hard-linked paths skipped on restore with a warning; after the 30-day sweep rewind can fail with `No files were restored`. From v2.1.216 SDK rewind skips links (before that it wrote through them). Background `/code-review --fix` edits are outside checkpoints. | GA | https://code.claude.com/docs/en/checkpointing.md; https://code.claude.com/docs/en/agent-sdk/file-checkpointing.md | reviewed |
| OpenCode | Shell side effects and ignored files not reverted; untracked files only up to 2 MiB; outside git only conversation rollback. | GA | https://opencode.ai/docs/snapshots/ | reviewed |
| Hermes Agent | Caps: 20 snapshots per project, 500 MB total, 10 MB per file, 7-day retention; skips directories over 50,000 files; nested git repos stored as gitlinks and not fully recoverable; disabled on container backends; local and SSH backends unaffected. | GA | /user-guide/checkpoints-and-rollback | reviewed |
| Cline | Large repos may be slow or heavy. Not documented: exclusions, size caps, pruning, disk location, handling of files created after the checkpoint, manual edits. | GA | https://docs.cline.bot/core-workflows/checkpoints.md | reviewed |
| Cursor | Limits and retention of local checkpoints not documented in the pages read. | GA | https://cursor.com/docs/agent/overview | unreviewed |
| rizzy-harness (ours) | Latest 10 kept; forced restore when files changed since. Size caps, exclusions and large-repo behaviour are not in the inventory; sub-agent worktrees keep git-lfs filters running (PLAN T19 residual). | partial | `crates/node/src/git.rs:721-800`; `own-rizzy-harness.md` residuals note | from own inventory |

#### SDK file checkpointing and rewind
What it is: a programmatic restore point per user message, usable from a host application.
Variants: only Claude Code.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `enableFileCheckpointing` backs up files before Write/Edit/NotebookEdit; each streamed user message carries a UUID restore point (needs hidden `replay-user-messages`). `rewindFiles(uuid, {dryRun})` deletes files Claude created and restores modified ones, without rewinding the conversation. CLI: `claude -p --resume <id> --rewind-files <uuid>` with `CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING=true` (flag undocumented in `--help`). Not tracked: Bash changes, subagent edits, directory create/move/delete, remote files. | GA | https://code.claude.com/docs/en/agent-sdk/file-checkpointing.md; https://code.claude.com/docs/en/agent-sdk/typescript.md | unreviewed |
| rizzy-harness (ours) | Not in the inventory (rewind is UI/protocol `session.restore`). | partial | `crates/protocol/src/lib.rs:74-260` | from own inventory |

### Area 12: Git and version control

#### Worktrees as the isolation unit for parallel agents
What it is: running a session or sub-agent in its own git worktree so parallel agents do not edit the same files.
Variants:
- Who gets one: every chat/agent (Codex, Cursor Agents Window, OpenClaw "each agent task", Cline Kanban card) versus opt-in per session (Claude Code `--worktree`, Oh My Pi, Hermes `-w`, OpenCode `-w`, Zed title-bar picker, Cline CLI `--worktree`) versus only sub-agents (Ours, Claude Code `isolation: worktree`).
- Starting point: Codex starts from the selected branch HEAD plus uncommitted changes in detached HEAD; Claude Code defaults to a fresh remote default branch (fetched if older than 24 h) or `head`; Zed starts detached HEAD; Ours seeds the worktree with the working state.
- Where it lives: inside the repo (`.claude/worktrees/`, Hermes `.worktrees/`) versus the tool's data dir (`$CODEX_HOME/worktrees`, `~/.omp/wt`, `~/.cline/worktrees/`, OpenClaw `<state-dir>/worktrees`).
- Getting changes back: Ours applies a patch and reports conflicts; Cursor `/apply-worktree`; Claude Code agents commit, push and open a draft PR; OpenClaw/others leave the branch.
- Security: Claude Code refuses creation when git config contains filter drivers or `includeIf`; Hermes runs checkout with hooks, fsmonitor and filters off; Ours refuses when the repo config defines filter commands (git-lfs filters still run, PLAN T19). These are the three that treat repo git config as hostile.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Each chat can run in a Codex-managed worktree under `$CODEX_HOME/worktrees`, starting from selected branch HEAD plus uncommitted changes, detached HEAD. CLI `/worktree`; `codex fork --worktree`. | GA | https://learn.chatgpt.com/docs/environments/git-worktrees.md; https://learn.chatgpt.com/docs/environments/local-environment.md; https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| OpenCode | First-class: `worktree` config sets the parent dir; tools/API create, list, refresh, remove worktrees with pluggable strategies (plugins can register custom ones); desktop shortcuts choose worktree and base branch; `opencode run -w`. | GA | https://opencode.ai/docs/config/; https://opencode.ai/docs/build/sdk/; https://opencode.ai/changelog | unreviewed |
| Claude Code | `--worktree/-w [name]` creates `<repo>/.claude/worktrees/<name>` on branch `worktree-<name>`; `#<n>` or a PR/MR URL fetches the change head into `pr-<N>`; `EnterWorktree`/`ExitWorktree` tools; `baseRef` `fresh` (default) or `head`; sparse worktrees and `symlinkDirectories`; WorktreeCreate/Remove hooks can replace git worktrees. Subagent `isolation: worktree`. Resume returns the session to its worktree after re-verifying it. | GA | https://code.claude.com/docs/en/worktrees.md; https://code.claude.com/docs/en/cli-reference.md; https://code.claude.com/docs/en/large-codebases.md | reviewed (base ref, subagent) |
| Claude Code (background) | Dispatched background sessions move into a worktree under `.claude/worktrees/` before editing, isolation enforced for session and subagents; on finish commits without asking, pushes if a remote exists, opens a draft PR when the task calls for it; never pushes to main/master, force-pushes or merges. | research preview | https://code.claude.com/docs/en/agent-view.md | corrected (status) |
| Claude Desktop | Toggle Worktree before a session; stored in `<project>/.claude/worktrees/` by default; branch prefix configurable; `post-checkout` hooks no longer run on creation (v4.16336.0). | GA | https://code.claude.com/docs/en/desktop; https://claude.com/docs/cowork/changelog.md | unreviewed |
| DeepSeek harness | `create_worktree` tool creates a branch and worktree from a local commit, branch or tag and switches the session into it; needs explicit composition, runs under existing permissions. | experimental | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| Oh My Pi | Agent-managed worktrees under `~/.omp/wt` (`worktree.base`, `OMP_WORKTREE_DIR`) for isolation and PR checkout (`pr-<n>`); `omp worktree` lists/clears. | GA | https://omp.sh/docs/github; https://omp.sh/docs/cli | unreviewed |
| Cursor | Agents Window gives each agent its own checkout; IDE `/worktree`, `/apply-worktree`. | GA | https://cursor.com/docs/configuration/worktrees | unreviewed |
| Antigravity | App "New worktree" mode; subagent `branch` workspace mode; FAQ says worktrees are supported in the App. | GA (App) | https://antigravity.google/docs/projects; https://antigravity.google/docs/subagents; https://antigravity.google/docs/faq | unreviewed |
| OpenHands | Per-conversation runtime choice Local, Docker or Worktree (when a folder is open); Settings > Runtime sets "create worktrees from" and a branch prefix. | GA | D:/openhands/usage/agent-canvas/backend-setup/docker-execution.md; D:/openhands/usage/agent-canvas/conversations.md | unreviewed |
| OpenClaw | Each agent task gets its own checkout under `<state-dir>/worktrees/<repo-fingerprint>/<name>` on branch `openclaw/<name>`, recorded in the state DB; cap 4096; copy-on-write on APFS/Btrfs/ReFS/XFS. | GA (no maturity label on page) | https://docs.openclaw.ai/concepts/managed-worktrees | reviewed |
| Hermes Agent | `hermes -w` creates a disposable worktree under `<repo>/.worktrees/` on branch `hermes/hermes-<hash>`; checkout runs with the repo's git hooks, fsmonitor and filters off; LFS files appear as pointers. | GA | /user-guide/git-worktrees; /user-guide/cli | unreviewed |
| Zed/ACP | Threads can run in separate Git worktrees via a title-bar picker; new worktrees start detached HEAD; `create_worktree` task hook with `ZED_WORKTREE_ROOT`. | GA | https://zed.dev/docs/ai/parallel-agents.md | unreviewed |
| Warp | Native git worktree support; each worktree gets its own Code Review panel, status indicators, file watching and independent indexing. (Multi-agent page's "one worktree per agent" advice was not found on the cited pages.) | GA | https://docs.warp.dev/code/git-worktrees/ | corrected (advice claim) |
| Cline | CLI `--worktree` runs in a detached worktree under `~/.cline/worktrees/`; Kanban uses one worktree per card (gitignored dirs like `node_modules` symlinked from the main repo); Desktop selects a branch at session start. | GA (CLI), preview (Kanban) | https://docs.cline.bot/cli/cli-reference.md; https://docs.cline.bot/kanban/core-workflow.md; https://docs.cline.bot/usage/cline-desktop.md | unreviewed |
| rizzy-harness (ours) | A sub-agent that can write/edit/bash and sits in a git repo gets its own worktree seeded with the working state; changes applied back as a patch, conflicts reported not applied; refused if the repo config defines filter commands (git-lfs filters still run, PLAN T19). Not available as a user feature. | works (sub-agents only) | `crates/node/src/session.rs:765-856`, `crates/node/src/git.rs:861-960` | from own inventory |

#### Worktree setup and copying ignored files
What it is: making a fresh worktree usable (env files, dependencies) without checking secrets in.
Variants: a gitignore-syntax allowlist file (`.worktreeinclude` in Codex, Claude Code, Claude Desktop, OpenClaw); a setup script config (`.cursor/worktrees.json`, `.openclaw/worktree-setup.sh`, Codex per-worktree setup scripts, Zed `create_worktree` hook); symlinking big directories (Claude Code `worktree.symlinkDirectories`, Cline Kanban). Claude Code only copies files that are gitignored.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `.worktreeinclude` copies chosen ignored files such as `.env`; setup scripts run per worktree. | GA | https://learn.chatgpt.com/docs/environments/git-worktrees.md; https://learn.chatgpt.com/docs/environments/local-environment.md | unreviewed |
| Claude Code / Desktop | `.worktreeinclude` (gitignore syntax) copies only gitignored files; `worktree.symlinkDirectories`; `worktree.sparsePaths` for sparse-checkout. | GA | https://code.claude.com/docs/en/worktrees.md; https://code.claude.com/docs/en/large-codebases.md; https://code.claude.com/docs/en/desktop | reviewed (worktrees page) |
| Cursor | `.cursor/worktrees.json` keys `setup-worktree-unix`, `-windows`, `setup-worktree` run setup commands (`$ROOT_WORKTREE_PATH` available). | GA | https://cursor.com/docs/configuration/worktrees | unreviewed |
| OpenClaw | `.worktreeinclude` copies ignored files; `.openclaw/worktree-setup.sh` runs on create. | GA | https://docs.openclaw.ai/concepts/managed-worktrees | reviewed |
| Zed/ACP | `create_worktree` task hook with `ZED_WORKTREE_ROOT` and `ZED_MAIN_GIT_WORKTREE`. | GA | https://zed.dev/docs/ai/parallel-agents.md | unreviewed |
| Cline | Kanban symlinks gitignored dirs from the main repo (agent edits to them reach the main repo). | preview | https://docs.cline.bot/kanban/core-workflow.md | unreviewed |
| rizzy-harness (ours) | Worktree "seeded with the working state"; no ignored-file allowlist or setup script in the inventory. | partial | `crates/node/src/git.rs:861-960` | from own inventory |

#### Worktree lifecycle: cleanup, snapshots before removal, caps
What it is: what happens to worktrees when the session ends, is archived or goes stale; whether dirty work can be lost.
Variants: OpenClaw snapshots tracked and untracked files as a commit under `refs/openclaw/snapshots/<id>` (30 days) even when removing idle dirty trees; Hermes archives dirty or unpushed work as commits under `refs/hermes/archive/<tree>`; Zed saves worktree git state on archive and re-creates it on restore; Claude Code removes unchanged worktrees and sweeps others after `cleanupPeriodDays` but skips ones with changes or unpushed commits; Claude Desktop warns uncommitted changes can be lost about 30 days after archive; Cursor caps machine-wide count (default 25) and also deletes worktrees made outside Cursor; Codex keeps the latest 15.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Latest 15 managed worktrees kept. | GA | https://learn.chatgpt.com/docs/environments/git-worktrees.md | unreviewed |
| Claude Code | Interactive exit: clean unnamed worktree auto-removed, otherwise prompt; subagent worktrees removed if unchanged; periodic sweep after `cleanupPeriodDays` skips worktrees with changes, untracked files, unpushed commits, dirty submodules and user-created ones (marker in git metadata); `git worktree lock` held while an agent runs; `-p` runs never clean up. Agent-view delete removes uncommitted changes; refuses when unpushed commits unless `--discard-unpushed`. | GA (background: research preview) | https://code.claude.com/docs/en/worktrees.md; https://code.claude.com/docs/en/agent-view.md | reviewed (cleanup/locking) |
| Claude Desktop | Archived sessions' worktrees removed about 30 days after archive (uncommitted changes can be lost). | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| Cursor | `cursor.worktreeCleanupIntervalHours` and `cursor.worktreeMaxCount` (default 25, machine-wide); also deletes worktrees made outside Cursor. | GA | https://cursor.com/docs/configuration/worktrees | unreviewed |
| OpenClaw | Before removal, tracked and untracked non-ignored files are snapshotted as a commit under `refs/openclaw/snapshots/<id>` kept 30 days; idle >7 days snapshotted and removed even if dirty; `openclaw worktrees list\|gc\|restore`. | GA | https://docs.openclaw.ai/concepts/managed-worktrees | reviewed |
| Hermes Agent | On exit with dirty or unpushed work: keep, archive-and-remove, or discard; stale sweep every 6 h (24 h idle for auto trees, 72 h named); archives as commits under `refs/hermes/archive/<tree>`; `hermes worktree list\|prune\|restore`. | GA | /user-guide/git-worktrees | unreviewed |
| Zed/ACP | Archiving a thread saves worktree Git state and removes it from disk; restoring re-creates it. | GA | https://zed.dev/docs/ai/parallel-agents.md | unreviewed |
| Oh My Pi | `omp worktree` lists/clears. | GA | https://omp.sh/docs/cli | unreviewed |
| rizzy-harness (ours) | Sub-agent worktree changes are applied back as a patch and conflicts reported; cleanup and dirty-work protection are not described in the inventory. | partial | `crates/node/src/session.rs:765-856` | from own inventory |

#### Git repository requirement and non-git folders
What it is: whether the harness works (and what it loses) outside a git repository.
Variants: Codex `exec` refuses outside a repo unless `--skip-git-repo-check`; OpenCode snapshots outside git degrade to conversation rollback; Claude Code worktrees need at least one commit and non-git dirs need a `WorktreeCreate` hook for background isolation; Hermes never snapshots `/` or `$HOME`; Ours checkpoints work for non-git folders.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `codex exec` refuses to run outside a Git repo unless `--skip-git-repo-check`. | GA | https://learn.chatgpt.com/docs/non-interactive-mode.md | reviewed |
| OpenCode | Outside git, undo is conversation rollback only. | GA | https://opencode.ai/docs/snapshots/ | reviewed |
| Claude Code | Worktrees need at least one commit; background isolation skipped for non-git dirs without a `WorktreeCreate` hook. | GA / research preview | https://code.claude.com/docs/en/common-workflows.md; https://code.claude.com/docs/en/agent-view.md | unreviewed |
| rizzy-harness (ours) | Checkpoints work for non-git folders (shadow repo); sub-agent worktrees need a git repo. | works | `crates/node/src/git.rs:41-60`, `crates/node/src/session.rs:765-856` | from own inventory |

#### Commits and commit-message generation
What it is: the harness creating commits, or generating a message for the user's commit.
Variants: dedicated command with validation (Oh My Pi `omp commit`: agentic split into dependency-ordered commits, Conventional Commit validation, changelog update, runs hooks) versus UI button (Zed Git panel with a configurable model and instructions; Cline VS Code SCM; Antigravity auto-message when empty) versus conversational (Claude Code, OpenHands Commits drawer sends prompts). Several harnesses have nothing and rely on `bash`.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Oh My Pi | `omp commit`: Conventional Commit messages; an agent inspects files/hunks and proposes one commit or a dependency-ordered split (each change exactly once, cycles rejected, lock files committed with their manifest, confirmation for interactive splits); falls back to a mechanical single commit; validates summary (<=72 chars, past-tense verb); updates the nearest `CHANGELOG.md`; runs git hooks; `--push`, `--dry-run`. | GA | https://omp.sh/docs/commit | unreviewed |
| Claude Code | Git driven conversationally (commit with generated message, branch, conflicts, PR). Built-in commit/PR instructions are part of the Bash tool description; `includeGitInstructions: false` removes them. Background sessions commit on finish. | GA | https://code.claude.com/docs/en/common-workflows.md; https://code.claude.com/docs/en/quickstart.md; https://code.claude.com/docs/en/agent-sdk/modifying-system-prompts.md | unreviewed |
| Zed/ACP | AI commit messages from the Git panel (`agent.commit_message_model`, `commit_message_instructions`); uses the three latest commits as style examples (1.27.0). | GA | https://zed.dev/docs/ai/agent-settings.md; https://zed.dev/releases/stable | unreviewed |
| Antigravity | Review pane: commit with auto-generated message when empty, amend, push to upstream or publish to origin; push never forces. | GA (App) | https://antigravity.google/docs/review-changes | unreviewed |
| Cline | VS Code SCM "Generate Commit Message". | GA | https://docs.cline.bot/core-workflows/working-with-files.md | unreviewed |
| OpenHands | Commits drawer sends commit/pull/push/PR prompts to the agent (no dedicated tool). | GA | D:/openhands/usage/agent-canvas/conversations.md | unreviewed |
| Warp | On remote repos the Code Review panel supports commit, push, PR creation. | GA | https://docs.warp.dev/code/ssh-feature-support/ | unreviewed |
| OpenCode | No built-in commit or PR tool in the tool catalogue. | nothing documented | https://opencode.ai/docs/tools/ | unreviewed |
| DeepSeek harness | No built-in commit/PR/diff review; the agent uses `bash`. | nothing documented | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| rizzy-harness (ours) | No built-in commit, branch or PR tools or UI; the agent can run `git`/`gh` through `bash` subject to approval and the env allow-list (`SSH_AUTH_SOCK` and tokens not passed unless `env_pass` lists them). | none (via bash only) | `crates/tools/src/env.rs:20-30` | from own inventory |

#### Pull-request creation and PR checkout
What it is: opening a PR from the harness, or checking one out as a session.
Variants: Oh My Pi's `github` tool (off by default) pushes only to branches it checked out; Claude Code links the session to the PR it creates and offers `--from-pr`; cloud agents (OpenHands, Warp, Claude Code cloud, Cline cloud agents/Kanban) create PRs server-side. OpenClaw, Cursor, goose, Antigravity, Zed and Codex have no documented PR creation on the pages read.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Claude creates PRs via `gh pr create` or `glab mr create`; the session is linked to the PR (`--from-pr`, paste URL in picker; GitHub, GHE, GitLab MR, Bitbucket). Background sessions open a draft PR when the task calls for it. Cloud sessions put the session URL in the PR body. `--worktree "#1234"` or a PR/MR URL checks a PR out. | GA | https://code.claude.com/docs/en/common-workflows.md; https://code.claude.com/docs/en/cloud-environments.md; https://code.claude.com/docs/en/worktrees.md | unreviewed |
| Oh My Pi | `github` tool (off by default) uses the `gh` CLI (no separate token): checks a PR into a `pr-<n>` worktree under `~/.omp/wt`, creates PRs, pushes only to branches it checked out, watches Actions with failed-log artifacts. `pr://482` and `issue://731` work as a virtual filesystem without enabling the tool; cache `~/.omp/cache/github-cache.db`. | GA | https://omp.sh/docs/github | unreviewed |
| OpenHands | Cloud GitHub/GitLab: an `openhands` label or `@openhands` comment opens a PR and summarises (short-lived 8-hour tokens); Commits drawer sends PR prompts. | GA | D:/openhands/usage/cloud/github-installation.md; D:/openhands/usage/agent-canvas/conversations.md | unreviewed |
| Warp | PR creation from the Code Review panel on remote repos; cloud agents attach screenshots (up to 10 per PR) and recordings to PR descriptions (admin setting Disabled, Link only default, Embed). | GA | https://docs.warp.dev/code/ssh-feature-support/; https://docs.warp.dev/agents/capabilities/computer-use/artifacts-in-prs/ | unreviewed |
| Cline | Cloud agents and Kanban open PRs; Kanban overview mentions auto-commit/auto-PR settings while core-workflow does not describe auto-PR. | GA / preview | https://docs.cline.bot/usage/cloud-agents.md; https://docs.cline.bot/kanban/core-workflow.md | unreviewed |
| Codex | `codex apply` applies the latest cloud diff locally with `git apply` (non-zero exit on conflict); PR creation not documented on the pages read. | GA | https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| Antigravity | No documented PR creation (Remote Agents page does not cover PRs). | nothing documented | https://antigravity.google/docs/remote-agents | unreviewed |
| OpenClaw | Nothing documented for commit/PR/review flows; Changes panel shows diffs; cloud workers publish via a Gateway broker. | unclear | https://docs.openclaw.ai/gateway/cloud-workers/security-model; https://docs.openclaw.ai/web/control-ui | unverified |
| rizzy-harness (ours) | None built in; via `bash`/`gh` only. | none | `crates/tools/src/env.rs:20-30` | from own inventory |

#### PR status tracking, auto-fix and auto-merge
What it is: following a PR after it is opened: CI status, review comments, fixing and merging.
Variants: Claude Code (terminal footer badge, agent-view rows, cloud auto-fix via the Claude GitHub App) and Claude Desktop (CI auto-fix, review-comment fixes from trusted authors, auto-merge squash unless a merge queue; disabled in SSH and WSL). Cloud auto-fix cannot react to merge conflicts because GitHub sends no webhook when the base advances; replies post under the user's GitHub account labelled as Claude Code and can trigger `issue_comment` automations. Hermes Kanban PR completion contracts check required GitHub checks (guide pages not opened).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Footer `PR #N` badge colored by review state (needs a GitHub token; GitLab `MR !N` via `glab`); agent-view rows show PR status and a `Ready for review` group; cloud auto-fix subscribes to PR activity, fixes CI failures and review comments or asks (needs the GitHub App; `/autofix-pr`). | GA (agent view: research preview) | https://code.claude.com/docs/en/interactive-mode.md; https://code.claude.com/docs/en/agent-view.md; https://code.claude.com/docs/en/claude-code-on-the-web.md | unreviewed |
| Claude Desktop | With `gh` authenticated, auto-fix CI failures and review comments from trusted authors, enable GitHub auto-merge; auto-archive after the PR merges or closes. Docs warn replies can trigger `issue_comment` automation. | GA | https://code.claude.com/docs/en/desktop; https://code.claude.com/docs/en/claude-code-on-the-web.md | unreviewed |
| Cursor | Bugbot autofix launches a Cloud Agent (max three attempts per PR); Create New Branch autofix is GitHub and Origin only. | GA | https://cursor.com/docs/bugbot | reviewed |
| Hermes Agent | Kanban PR completion contracts check required GitHub checks. | GA (guides) | /user-guide/features/kanban | unreviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Local code review command
What it is: a read-only reviewer run on the working tree, a branch diff or a commit from inside the harness.
Variants: Codex `/review` is read-only and targets uncommitted changes, base branch, a commit, a custom prompt, or on the desktop the last turn. Claude Code `/code-review` has effort levels, can apply fixes (`--fix`, outside checkpoints) and post comments, and reads CLAUDE.md not REVIEW.md. Oh My Pi `/review` can slice the diff across up to 16 parallel reviewers with P0-P3 findings and confidence scores, and supports GitHub PRs and Jujutsu. Claude Desktop shows a code review card.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `/review`, `codex review`, API `review/start`: a dedicated reviewer reports prioritised findings without modifying the tree; `review_model` overrides the model. | GA | https://learn.chatgpt.com/docs/code-review.md; https://learn.chatgpt.com/docs/developer-commands.md; https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| Claude Code | `/code-review [effort] [--fix] [--comment] [--max-findings n\|all\|default] [target]` (alias `/review`); runs as a background forked subagent in a terminal; targets: branch plus uncommitted changes, PR number, ranges, path; `--comment` posts inline GitHub comments or a GitLab MR note; fixed findings are re-reported as fixed, skipped or no_change_needed. Renamed from `/simplify` in 2.1.147. | GA | https://code.claude.com/docs/en/code-review-local.md | unreviewed |
| Claude Desktop | `/code-review` produces a Code review card with "Walk through in diff" and "Apply fixes" (local, SSH, WSL only). | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| Oh My Pi | `/review`: read-only review of base-branch diff, uncommitted Git or JJ changes, a commit or a GitHub PR; filters lock/generated/vendored files; 1 to 16 parallel reviewer slices; findings P0-P3 with confidence 0.0-1.0 and an overall verdict. `/annotate` pins notes to diff lines (README only). | GA | https://omp.sh/docs/review | unreviewed |
| goose | `goose review [range]` reviews a git diff (default working tree vs HEAD) with `--files`, `--check-filter`, `--dry-run`, `--severity`, `--no-orchestrate`; flag semantics unexplained. | unclear | https://goose-docs.ai/docs/guides/goose-cli-commands.md | unverified |
| rizzy-harness (ours) | No review command. A read-only Reviewer agent definition exists (inventory names read-only Scout/Reviewer agents as the closest thing). | partial | `own-rizzy-harness.md` gap list ("Plan mode" row) | from own inventory |

#### Hosted PR review bots and CI actions
What it is: an automated reviewer running on a hosting-platform event (PR opened, push, comment) and posting to the PR.
Variants: trust boundary and who controls the review differ. Claude Code Managed Code Review never approves or blocks (check run is neutral or skipped), is read-only, picks its own models and runs only on Anthropic infrastructure; Cursor Bugbot reads its config from the base branch so a PR cannot change its own review settings; Codex limits GitHub review comments to P0 and P1 and follows `## Code Review Rules` in the nearest `AGENTS.md`; DeepSeek's overlay creates a read-only local review session from a signed webhook with no outbound GitHub access, but has no deduplication; Cline's sample splits authorize/analyze/post across three jobs with scoped tokens.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `@codex review` comment or automatic per-repo/per-person setting; posts a standard GitHub review limited to P0 and P1 issues; follows `## Code Review Rules` in the nearest `AGENTS.md`; `@codex security review` is the deeper Security Review (research preview). GitLab MR review: `@codex review` and automatic runs in Codex Cloud; self-managed and Dedicated GitLab need an admin template. Desktop Code Review plugin gives a PR inbox, summary, checks and stacked-PR navigation. | GA (Security Review preview); GitLab beta/preview | https://learn.chatgpt.com/docs/third-party/github.md; https://learn.chatgpt.com/docs/third-party/gitlab.md; https://learn.chatgpt.com/docs/code-review.md | reviewed (GitHub) |
| Claude Code | Managed Code Review (Team/Enterprise, GA): parallel finders, a verify step, dedupe/rank, then inline comments, a summary and a "Claude Code Review" check run; read-only; levels Quick/Standard/Deep; triggers once, every push or manual; severities Important/Nit/Pre-existing; `@claude review [once\|always\|stop]` needs write access and a new top-level comment; machine-readable severity counts in the check output; not available with ZDR, HIPAA or CMEK; does not run in self-hosted environments. `REVIEW.md` for review-only rules. GitHub Action `claude-code-action@v1` (interactive `@claude` and automation modes). | GA (Team/Enterprise) | https://code.claude.com/docs/en/code-review.md; https://code.claude.com/docs/en/code-review-setup.md; https://code.claude.com/docs/en/code-review-pull-requests.md; https://code.claude.com/docs/en/github-actions.md | unreviewed |
| DeepSeek harness | Opt-in overlay: signed GitHub webhook endpoint (`POST /github`, second listener default `127.0.0.1:3081`); when a PR moves from draft to ready it creates a titled root session in a local checkout and starts a read-only review with the `standard` preset and `read-only` permission preset; PR fields passed as untrusted metadata; no outbound GitHub access. 202 means accepted, not matched; deliveries not stored, so duplicates create more sessions. | preview (example overlay) | https://deepseek-harness.github.io/deepseek-harness/en/guide/github-review | reviewed |
| Cursor | Bugbot: automated review on GitHub (incl. Enterprise Server), GitLab (incl. self-hosted), Bitbucket (incl. Data Center), Azure DevOps Services, Origin; runs on each PR update or `cursor review`/`bugbot run` comments; rules from team rules, repo rules, nested `.cursor/BUGBOT.md`, plus learned rules; config read from the base branch; config files over 64 KB ignored; check status defaults to neutral; autofix launches a Cloud Agent (max three attempts); usage-based billing. | GA | https://cursor.com/docs/bugbot | reviewed |
| OpenHands | Cloud resolver: `openhands` label or `@openhands` comment on GitHub/GitLab; SDK GitHub workflow guides (assign reviews, PR review, TODO management). | GA | D:/openhands/usage/cloud/github-installation.md; D:/llms.txt | unreviewed |
| Warp | `@warp-agent` mentions in issues/PRs/review comments (Warp Factories GitHub App); `oz-agent-action` GitHub Action for PR review, issue fixes and CI fixes; GitLab, Azure DevOps, Bitbucket integrations. Repo identity differs by trigger (user token, team authorization, or the GitHub App). | GA (Factories-linked parts early access) | https://docs.warp.dev/platform/integrations/; https://docs.warp.dev/platform/architecture/ | unreviewed |
| Hermes Agent | Guides for a PR reviewer agent and webhook-triggered reviews (guide pages not opened). | GA (guides) | /docs/llms.txt; /user-guide/features/kanban | unreviewed |
| Cline | GitHub Actions samples: issue RCA on `@cline` comments (three-job split: authorize with a read-only token, analyze with `--auto-approve true --json`, post with a write-only issues token) and PR review samples. | sample | https://docs.cline.bot/cli/samples/github-integration.md | unreviewed |
| rizzy-harness (ours) | None. Webhook triggers exist for routines, not PR events; webhook-triggered chats lose memory and web tools. | none | `crates/master/src/http/hooks.rs:1-60`, `crates/master/src/lib.rs:257-297` | from own inventory |

#### Cloud multi-agent deep review (Ultrareview)
What it is: a fleet of reviewer agents in a cloud sandbox, each finding independently reproduced.
Variants: only Claude Code.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/code-review ultra [base\|PR\|note] [--post]` (alias `/ultrareview`) or `claude ultrareview`; each finding independently reproduced or verified; branch review uploads repo state (special rules for credential-like files), PR review clones from github.com; limits 500 files / 8,000 changed lines; 3 free runs then roughly $5-25 per review, 5-15 minutes; `--post` posts one plain PR comment as the user (v2.1.227+); exit codes 0/1/130; Claude never starts it on its own. Not on Bedrock, Agent Platform or Foundry, or with ZDR, HIPAA or CMEK. | research preview | https://code.claude.com/docs/en/ultrareview.md | reviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Diff views
What it is: viewing what the agent changed, per file, per turn or per branch, with the ability to comment, revert or apply.
Variants: Warp, Antigravity, Claude Desktop and Codex Desktop have review panes with inline comment or hunk-level actions; Cline diffs against a checkpoint; OpenCode and OpenHands expose diff only as API endpoints in the pages read; ACP carries `diff` content (path, oldText, newText) in tool-call updates. Ours shows only the old/new strings of an `edit` call and a per-turn file list.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Desktop Code Review plugin shows PR changes and checks; local review targets uncommitted changes. | GA | https://learn.chatgpt.com/docs/code-review.md | unreviewed |
| OpenCode | `/api/vcs` routes: init, branch info, inferred review base, status, branches, diff. | GA | https://opencode.ai/docs/api/ | unreviewed |
| Claude Code | `/diff` listed among verification commands; local Claude Code diff UI beyond that not described here. | GA | https://code.claude.com/docs/en/commands.md | unreviewed |
| Claude Desktop | Click line counts on the Changes card to open the diff; hover a line and `+` to comment, comments go with the next message; cloud diff compares to base or any branch, computed from raw blob content. | GA | https://code.claude.com/docs/en/desktop; https://code.claude.com/docs/en/claude-code-on-the-web.md | unreviewed |
| DeepSeek harness | Changed-file cards in the web UI; no diff review feature documented. | GA/unclear | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| Oh My Pi | `/annotate` pins notes to diff lines (README only; not on the review page). | GA | https://omp.sh/docs/review | unverified |
| Antigravity | Review pane with commit, amend and push. | GA (App) | https://antigravity.google/docs/review-changes | unreviewed |
| OpenHands | Git changes/diff API endpoints; commits drawer in Canvas. | GA | D:/openhands/usage/agent-canvas/conversations.md | unreviewed |
| OpenClaw | Changes panel shows diffs. | unclear | https://docs.openclaw.ai/web/control-ui | unreviewed |
| Zed/ACP | ACP tool calls carry `diff` content (path, oldText, newText); Zed's own review UI beyond "reject edits" not documented in the pages read. | GA | https://agentclientprotocol.com/protocol/v1/tool-calls.md | unreviewed |
| Warp | Code Review panel: review uncommitted changes, vs main/master or any branch; attach diffs as agent context, revert individual hunks, discard all; inline comments can be sent to running agents including third-party CLI agents. "Interactive Code Review" page not read. | GA | https://docs.warp.dev/code/code-review/ | unreviewed |
| Cline | Checkpoint Compare opens a diff; VS Code panel shows diffs with Approve/Reject. | GA | https://docs.cline.bot/core-workflows/checkpoints.md | reviewed |
| rizzy-harness (ours) | Per-turn "N files changed / Undo / Rewind" with a file list (`Checkpoint.files`); tool cards show old/new strings of an `edit` call; no whole-turn diff or file-tree of changes. | partial | `crates/node/src/session.rs:509-530`, `web/src/components/ToolCard.tsx:125-131`, `web/src/components/Conversation.tsx:43-80` | from own inventory |

#### Attribution, signing and provenance in commits and PRs
What it is: marking agent-made commits and PRs, and proving who made them.
Variants: Cursor attributes CLI commits and PRs to the agent by default (`attribution.*`, default true) and signs cloud-agent commits with an HSM-backed Ed25519 key (vendor statement). Claude Code has `attribution.commit`/`attribution.pr` (the older `includeCoAuthoredBy` setting was deprecated in 2.0.62), a `Claude-Session: <url>` trailer plus session URL in PR bodies for cloud sessions, and on self-hosted runners an identity `Claude <noreply@anthropic.com>`, SSH-format signing via a signing service (beta) and a commit-msg hook adding `Co-authored-by` for the creator.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Cursor | Cloud agent commits signed with an HSM-backed Ed25519 key and show Verified on GitHub (vendor statement); CLI attributes commits and PRs to the agent by default (`attribution.*`). | GA | https://cursor.com/docs/cloud-agent/security; https://cursor.com/docs/cli/reference/configuration | reviewed (signing is a vendor statement) |
| Claude Code | `attribution.commit`/`attribution.pr` set the trailer and footer; cloud-session commits carry `Claude-Session: <url>` and PR bodies include the URL (`attribution.sessionUrl: false` disables). Self-hosted `--configure-git`: identity `Claude <noreply@anthropic.com>`, SSH commit signing through a runner shim calling Anthropic's signing service, `Co-authored-by` for the creator. | GA (self-hosted git config: beta) | https://code.claude.com/docs/en/cloud-environments.md; https://code.claude.com/docs/en/self-hosted-environments-deploy.md; https://code.claude.com/docs/en/changelog.md | unreviewed |
| Warp | Not attribution as such: computer-use screenshots and recordings attached to PR descriptions. | GA | https://docs.warp.dev/agents/capabilities/computer-use/artifacts-in-prs/ | unreviewed |
| rizzy-harness (ours) | No commit attribution. Published artifact versions record the git commit and dirty state (`GitInfo`). | partial | `crates/node/src/artifacts.rs` (`GitInfo`, imported at `:23`) | from own inventory |

#### Git host integrations and credential handling
What it is: how the harness talks to GitHub/GitLab/others, and where git credentials live when work runs remotely.
Variants: Claude Code's Anthropic-hosted VMs never hold real GitHub credentials: a proxy swaps a scoped credential, rejects branch deletion and tag pushes, and rejects all GraphQL (so `gh pr` fails, `gh api` REST works). Warp's repo identity depends on the trigger (user token, team authorization, or the GitHub App). Oh My Pi uses the `gh` CLI with no separate token. OpenCode exposes a VCS API but no host integration is described.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Cloud: GitHub App or `/web-setup` token (stored encrypted); all git ops go through a proxy so real credentials never enter the VM (`GH_TOKEN` reads as `proxy-injected`); proxy rejects branch deletion and non-branch pushes, limits API to attached repos, rejects GraphQL. Self-hosted runners may opt in (`--use-anthropic-git-proxy`: github.com only, replaces the runner user's global git config without backup). GitLab parity: MR URLs in `--worktree`, `MR !N` badge via `glab`. | GA (self-hosted proxy: beta) | https://code.claude.com/docs/en/cloud-environments.md; https://code.claude.com/docs/en/claude-code-on-the-web.md; https://code.claude.com/docs/en/self-hosted-environments-deploy.md; https://code.claude.com/docs/en/changelog.md | unreviewed |
| Codex | GitHub and GitLab integrations for review; `codex apply` for cloud diffs. | GA / beta | https://learn.chatgpt.com/docs/third-party/github.md; https://learn.chatgpt.com/docs/third-party/gitlab.md | unreviewed |
| Oh My Pi | `gh` CLI as GitHub filesystem and `github` tool (see PR creation). | GA | https://omp.sh/docs/github | unreviewed |
| OpenCode | `/api/vcs` routes; no host integration listed. | GA | https://opencode.ai/docs/api/ | unreviewed |
| Cursor | Bugbot hosts (GitHub, GitLab, Bitbucket, Azure DevOps, Origin); Origin is Cursor's own Git hosting (CLI, PRs, stacks, mirror from GitHub); Origin pages themselves not opened. | unclear (Origin) | https://cursor.com/docs/bugbot; https://cursor.com/llms.txt; https://cursor.com/changelog | unverified (Origin) |
| Antigravity | OAuth to GitHub for remote agents; repos cloned into the remote workspace; private repos in one project must share an owner. | GA (limited rollout) | https://antigravity.google/docs/remote-agents | unreviewed |
| OpenHands | GitHub and GitLab resolver-style integrations; per-trigger short-lived tokens. | GA | D:/openhands/usage/cloud/github-installation.md | unreviewed |
| Warp | GitHub (`@warp-agent`, Factories App, `oz-agent-action`), GitLab (self-managed via PAT secret), Azure DevOps (PAT), Bitbucket Cloud and Data Center (tokens as managed secrets). | GA (Factories parts early access) | https://docs.warp.dev/platform/integrations/; https://docs.warp.dev/platform/architecture/ | unreviewed |
| goose | Recipes can load from a GitHub repo via `GOOSE_RECIPE_GITHUB_REPO` using the `gh` CLI; no worktree or PR feature in the CLI reference. | unclear | https://goose-docs.ai/docs/guides/recipes/recipe-reference.md | unreviewed |
| rizzy-harness (ours) | None; git/`gh` only via `bash` with a cleared env; tokens and `SSH_AUTH_SOCK` not passed unless `env_pass` allows. | none | `crates/tools/src/env.rs:20-30` | from own inventory |

## 3. Availability matrix

Legend: `Y` documented and released; `B` beta/experimental/preview/partial (including documented only for one surface or one narrow case); `P` planned or vendor-claim only; `-` nothing documented; `?` unverified. "Removed" features are `-`. OpenCodex is a gateway, so most cells are `-`. For Ours, `Y`/`B` follow the inventory status (works/partial).

| Feature | Codex | OpenCodex | OpenCode | Claude Code | Claude Desktop | DeepSeek harness | Oh My Pi | Pi | Cursor | Antigravity | OpenHands | OpenClaw | goose | Hermes Agent | Zed/ACP | Warp | Cline | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Session storage documented (format/location) | Y | - | Y | Y | - | B | Y | Y | B | B | Y | Y | Y | Y | B | B | Y | Y |
| Resume / continue | Y | - | - | Y | B | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| Fork to a new session | Y | - | Y | Y | - | B | Y | Y | Y | Y | Y | - | Y | Y | - | B | - | - |
| In-file session tree | - | - | - | - | - | - | Y | Y | - | - | - | - | - | - | - | - | - | - |
| Conversation rewind / edit-and-resubmit | Y | - | Y | Y | - | - | B | Y | Y | ? | Y | B | Y | Y | Y | - | Y | - |
| Export a session | Y | - | Y | Y | - | - | Y | Y | ? | - | Y | - | Y | Y | - | Y | Y | - |
| Share by link | - | - | - | Y | - | - | Y | Y | ? | - | Y | - | - | - | - | Y | - | - |
| Search across sessions / picker | - | - | - | Y | - | B | Y | - | - | Y | - | Y | B | B | Y | - | - | Y |
| Move session between machines/surfaces | Y | - | B | Y | Y | - | B | - | Y | - | - | - | - | B | - | Y | B | - |
| Import sessions from other harnesses | - | - | B | - | - | - | Y | - | - | - | - | - | - | Y | Y | - | Y | - |
| Archive / delete / retention / ephemeral | Y | - | - | Y | Y | B | B | B | Y | B | Y | Y | Y | Y | Y | Y | Y | Y |
| Single-writer / crash recovery | B | - | - | B | - | B | - | - | - | - | - | Y | - | Y | - | - | - | Y |
| Pluggable external session store | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| File checkpoints and undo | - | - | Y | Y | - | - | - | - | Y | ? | B | - | - | Y | - | - | Y | Y |
| Checkpoint limits / retention / removal documented | - | - | Y | Y | - | - | - | - | B | - | - | - | - | Y | Y | - | B | B |
| SDK file checkpointing | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| Worktrees for parallel agents | Y | - | Y | Y | Y | B | Y | - | Y | Y | Y | Y | - | Y | Y | Y | Y | B |
| Worktree setup / ignored-file copy | Y | - | - | Y | Y | - | - | - | Y | - | - | Y | - | - | Y | - | B | B |
| Worktree lifecycle / cleanup | Y | - | - | Y | Y | - | B | - | Y | - | - | Y | - | Y | Y | - | - | B |
| Commit / commit-message generation | - | - | - | Y | - | - | Y | - | - | Y | B | - | - | - | Y | B | Y | - |
| PR creation / checkout | B | - | - | Y | - | - | Y | - | - | - | Y | ? | - | - | - | Y | B | - |
| PR status, auto-fix, auto-merge | - | - | - | Y | Y | - | - | - | B | - | - | - | - | B | - | - | - | - |
| Local code review command | Y | - | - | Y | Y | - | Y | - | - | - | - | - | ? | - | - | - | - | B |
| Hosted PR review bot / CI action | Y | - | - | Y | - | B | - | - | Y | - | Y | - | - | B | - | Y | B | - |
| Cloud multi-agent deep review | - | - | - | B | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| Diff views | Y | - | B | B | Y | B | B | - | - | Y | B | B | - | - | B | Y | Y | B |
| Attribution / signing / provenance | - | - | - | Y | - | - | - | - | Y | - | - | - | - | - | - | - | - | B |
| Git host integrations / credential handling | Y | - | B | Y | - | B | Y | - | B | Y | Y | ? | B | - | - | Y | - | - |

Notes on cells: the Cursor `?` on export and sharing is the unopened "Shared Transcripts" page; the Antigravity `?` cells are the unverified `/rewind` behaviour; OpenClaw `?` for PR creation is the survey's "nothing found on pages read, CHANGELOG not opened"; goose `?` for review is the unexplained `goose review` flags. Cursor diff view is `-` only because the Agents Window and IDE diff UI were outside the survey's area 11-12 text.

## 4. Common across the ecosystem

"Documented" here means `Y` or `B` in the matrix. Denominator is the 16 real harnesses (OpenCodex excluded; Zed/ACP counted as one). Half = 8.

| Feature | Count of 16 | Notes |
|---|---|---|
| Resume / continue | 15 | Only OpenCode lacks it in the survey ("nothing documented", not absence). |
| Archive / delete / retention / ephemeral | 15 | Mostly basic list/delete; the interesting differences are retention defaults. |
| Session storage documented | 13 | Strong families: JSONL (Codex, Claude Code, Oh My Pi, Pi, DeepSeek), SQLite (OpenCode, OpenClaw, goose, Hermes, Cline), file-per-event (OpenHands). |
| Worktrees for parallel agents | 14 | Everyone except Pi and goose (and Ours as user feature). Differences are in who gets one and how cleanup works. |
| Fork to a new session | 12 | Several (Antigravity, OpenHands) explicitly do not fork file state. |
| Diff views | 12 | Mix of full review panes (Warp, Antigravity, Claude Desktop, Cline) and API-only (OpenCode, OpenHands). |
| Conversation rewind / edit-and-resubmit | 11 | Counting Oh My Pi and OpenClaw as `B`. |
| Git host integrations / credential handling | 10 | |
| Export a session | 10 | Format and redaction differ widely. |
| Move session between machines/surfaces | 9 | Codex, Claude Code, Claude Desktop, Cursor, Warp are the full ones. |
| Search across sessions / picker | 8 | Exactly half. |
| Hosted PR review bot / CI action | 8 | Exactly half; 5 are GA-style (Codex, Claude Code, Cursor, OpenHands, Warp). |
| Worktree lifecycle / cleanup | 8 | Exactly half, GA in most. |

Below half but notable: file checkpoints (6 of 16 including OpenHands `undo_edit` as `B`; GA in OpenCode, Claude Code, Cursor, Hermes (off by default), Cline), commit-message generation (7), worktree setup/ignored-file copy (7), share by link (5), PR creation (5-6), local review command (4).

The notable negative pattern: 10 of 16 harnesses document no file-level undo at all (Codex, DeepSeek, Oh My Pi, Pi, Claude Desktop, Antigravity (unverified), OpenClaw, goose, Warp, Zed after removal), and the ones that do split between shadow git (OpenCode, Hermes, Cline, Ours) and non-git file snapshots (Claude Code, Cursor).

## 5. Only one harness has it

| Feature | Harness | Why interesting (or not) |
|---|---|---|
| In-file session tree shared by two harnesses only | Oh My Pi, Pi (same lineage) | Pi-style `id`/`parentId` log maps cleanly onto "fold over an append-only log": branches are just different leaf pointers. Interesting for Ours: forking without copying events. Cost: branches change file state invisibly, neither reverts files on navigation. |
| Pluggable `SessionStore` adapters | Claude Code | Documents an explicit append/load contract (ordered, deep-equal, dedupe by entry uuid), conformance suite and dual-write caveats. Relevant to a master that mirrors node logs; the best-effort mirror that drops a batch after 3 failed attempts is a counter-example to copy from. |
| SDK `rewindFiles(uuid)` with dry-run | Claude Code | Programmatic restore point per user message; the survey notes a past bug class (writing through symlinks before v2.1.216). |
| Agent-facing `checkpoint`/`rewind` tools | Oh My Pi | The model itself marks and returns to conversational checkpoints (off by default, no file revert). Interesting as context management; not a safety feature. |
| Ultrareview (cloud fleet, independent reproduction, billing, exit codes) | Claude Code | Interesting as a verified-findings design; not applicable to a self-hosted harness unless nodes can run a reviewer fleet. Research preview. |
| `omp commit` with dependency-ordered splits and validation | Oh My Pi | The most developed commit-message feature in the survey; deterministic validation plus agentic split with a mechanical fallback. |
| Pre-removal snapshot refs for worktrees and 30-day restore | OpenClaw (Hermes has a similar archive-ref design) | Prevents the "deleted a dirty worktree" loss that Claude Desktop's 30-day archive removal can cause. Cheap to copy. |
| Bugbot reading its config from the base branch | Cursor | A PR cannot weaken its own review; transferable to any review feature that reads repo-supplied rules. |
| Cloud git credential proxy (credentials never in the VM, GraphQL rejected, no tag pushes or branch deletion) | Claude Code | Directly relevant to keeping git secrets off worker nodes; the "GraphQL rejected" side effect breaks `gh pr`. |
| Session-to-PR linking and PR-URL lookup in the picker | Claude Code | Cheap way to find a session from its PR; needs a PR record in the session log. |
| Client-side encrypted share with key in the URL fragment | Oh My Pi | Share without the server reading the content; conflicts with a master that must read the log. |
| Documented, removed checkpoint design | Zed | A real negative result (per-prompt Git snapshots stalled on large repos), not an absent feature. |
| Webhook-spawned read-only review sessions (no outbound GitHub access) | DeepSeek harness | Closest published analogue of "webhook triggers a read-only session on a node"; documents its own gaps (no dedupe, no retry). |
| Computer-use artifacts attached to PRs | Warp | Visual evidence in PRs; privacy toggle (link only vs embed). Not core. |

## 6. Ours against the field

Our design in these areas: an append-only SQLite event log per session (one writer, the owning node; the UI a fold over the log) and per-turn checkpoints in a shadow git repo that never touches the user's `.git`. Ranked by how much each matters for a self-hosted master/node harness:

1. **Checkpoints: ahead, with named risks.** Shadow-repo undo per turn that works on non-git folders, with git called under a cleared env, no config and no hooks (see "File checkpoints and undo: the mechanism"). Only OpenCode, Hermes and Cline document comparable shadow-git designs; Claude Code and Cursor use non-git snapshots; Codex, DeepSeek, Oh My Pi, Pi, goose, Warp and OpenClaw document none. The surveys document no comparable hardening of the git invocation for OpenCode or Cline (nothing documented, not proof of absence). Risks the field has already hit: Zed removed per-prompt Git checkpoints because they slowed or hung large repositories (see "Checkpoint coverage, retention, costs and removals"); Cline documents large-repo slowness; Hermes caps size (500 MB total, 10 MB per file, 50,000-file skip); Claude Code is explicit about what is not covered (Bash-only changes, sub-agent edits, symlinks). Our inventory shows only a count cap (latest 10) and no size cap, no file-count skip and no list of what is not restored. Expect the Zed failure at scale unless that is measured. Also note the OpenCode warning that snapshot objects contain file contents (data-dir sensitivity) and that Hermes turns checkpoints off on container backends; the second is a trap for a node-decides model.
2. **Event-log session storage with one writer: ahead on invariants, behind on lifecycle.** Single-writer enforcement matches DeepSeek (preview) and OpenClaw (a claim checked inside the write transaction); Claude Code's equivalent is advisory and lets two terminals interleave. Behind: the log keeps every streaming delta forever (AUDIT D5, no log compaction), while Codex, Claude Code (30-day sweep, `claude purge`), OpenClaw (`pruneAfter`/`maxEntries`) and Hermes (prune, optimize) all ship retention. DeepSeek's zstd frames with torn-tail truncation is the one design that directly addresses growth and crash-torn writes in an append-only log.
3. **Fork, branch and edit-and-resubmit: absent.** 12 of 16 harnesses fork, 11 offer some conversation rewind, and Oh My Pi and Pi do it inside the log without copying. Because our log is already a single append-only sequence, the lowest-cost shape is the Pi-style parent pointer. Beware the OpenHands and Antigravity pitfall: a session fork does not fork file state; our shadow-repo checkpoints are the missing half and would let a fork carry a consistent file baseline, which no other harness documents.
4. **Export, share and import: absent.** 10 of 16 can export; Hermes (`--redact`) and OpenCode (`--sanitize`) show the minimum bar for a secrets-aware export. Sharing: OpenCode removed it, and Pi's share docs carry only a minimal warning; our "chats are private to their owner" invariant argues against building link sharing. Export is the cheap win.
5. **Moving a session between nodes: not in the inventory.** Codex (chat plus Git state between hosts), Warp (snapshot of tracked and untracked changes) and Claude Desktop/Cursor show the demand; Warp's snapshot-of-uncommitted-state is the mechanism closest to what a node migration would need. Our single-owner rule and per-node `node.toml` authority make this a design decision, not a copy.
6. **Worktrees: behind as a user feature, with a hostile-config stance that is ahead.** 14 of 16 offer user-facing worktrees; ours exist only for sub-agents. We refuse repos whose config defines filter commands; Claude Code (filter drivers, `includeIf`, LFS custom transfers) and Hermes (hooks, fsmonitor, filters off) are the only others documented doing the same. Known residual: git-lfs filters still run in our sub-agent worktrees (PLAN T19). Missing: ignored-file copy or setup scripts (7 harnesses), cleanup and dirty-work protection (OpenClaw snapshot refs and Hermes archive refs are the models), and any lifecycle doc.
7. **Diff view: behind.** We show old/new strings of one `edit` call and a per-turn file list. Warp, Antigravity, Claude Desktop and Cline offer whole-change review with inline comment or hunk revert. The shadow repo already holds before/after states per turn, so a per-turn diff is derivable without new capture.
8. **Commit, PR, review and attribution: absent, deliberately thin.** We have `git`/`gh` through `bash`, and the env allow-list keeps tokens off. 7 harnesses generate commit messages, 5 create PRs, 8 run hosted review bots, and only Claude Code and Cursor mark commits (the Claude Code cloud credential proxy is the one design aimed at keeping tokens off the worker). Under the feature freeze this is the lowest priority; the only item that follows from our own invariants is attribution, because agent commits made via `bash` currently carry no marker tied to a session id.

## 7. Corrections, contradictions and unverified claims

Corrections applied (from the reviews):
- OpenCode session sharing: was "nothing documented"; it was discontinued in v1.19.1 (15 Oct 2026). The V2 config `share` field statement comes from a search snippet only and the V2 config API page returned 404 (corrected in review).
- Zed checkpoint removal: removed in 1.27.0 (2026-11-04); the Agent Panel page is silent (there is no removal note on it); 1.25.2 is dated Oct 21 2026 and 1.23.3 Oct 12 2026 (corrected in review).
- Codex Handoff: moves a chat and its Git state between the local computer and a connected remote host; the phrase "to a worktree" is not on the remote-connections page. (The Local/Worktree handoff comes from the git-worktrees page.)
- Warp Handoff: `/handoff` confirmed; the detail "blocked while a command runs or child agents are active" was not on the pages opened and is unverified; the 2026.10.15 and 2026.09.23 periodic-checkpoint changelog claims were not checked. Warp multi-agent "one worktree per agent" advice was not found on the cited pages.
- Claude Code agent-view related status: "GA" was wrong; background sessions, agent view, `--bg` and its background-worktree isolation and PR-status rows are research preview (review 2). Background worktree isolation is labelled research preview here.
- Claude Code checkpoint limits (100 checkpoints, 30-day sweep, six menu actions, Bash and subagent exclusions, symlinks) were first unchecked in review 1 and then confirmed by review 2.
- OpenClaw per-session writer claim: the commit transaction verifies the claim; a state-dir lock is not on the queue page (corrected in review).
- Cursor cloud retention: indefinite by default, 30-day VM snapshot inactivity deletion, snapshots not deletable on demand (reviewed).

Contradictions:
- Cursor: the privacy page says Cloud Agents store encrypted repository copies only while running, then delete; the cloud security page says VM snapshots with cloned code are kept 30 days after inactivity and cannot be deleted on demand. The wording contradiction is real (reviewed).
- Cline: the checkpoints page and the IDE page name the three restore options differently ("Files only / Task only / Files and Task" versus "Task and Workspace / Task Only / Workspace Only"). The prompt-storage page names `~/.cline/data/tasks/` while the CLI layout names `data/sessions/`.
- Codex: GitLab MR review is "beta" on the GitLab page and "preview" on the code-review page.
- Cline Kanban: the overview mentions auto-commit/auto-PR settings, core-workflow does not describe auto-PR.
- Zed: `session/resume` and `session/load` stabilised Feb-Jun 2026, but ACP session fork exists only as an RFD title in the index.
- DeepSeek harness: the persistence page does not give the storage root; the Python SDK page gives `<dsh_home>/sessions` for the minimal profile only.

Unverified or unconfirmed:
- Antigravity `/rewind` and `/undo`: whether files are restored; checkpoint storage location and retention.
- Oh My Pi `/dump` secrets warning (not on the sessions page); `/annotate` is README-only.
- Pi `git-checkpoint.ts` and `auto-commit-on-exit.ts`: names only, contents not opened.
- Cursor "Shared Transcripts" help page not opened; Cursor Origin pages not opened; Cursor commit signing is a vendor statement.
- Hermes PR reviewer and webhook guides not opened; Kanban PR completion contracts claim comes from the llms index.
- OpenClaw: no built-in commit/PR/review flows found; CHANGELOG not opened; the managed-worktrees page text was truncated (30k chars unread), the survey's numbers were confirmed in review.
- goose: `goose review` flag semantics; `--no-orchestrate` implies an unconfirmed orchestration default.
- Codex: `/export` and `/rename` details and the exact CLI flags (ConfigTable components do not render in the `.md` twins).
- Claude Code: changelog-derived version and week claims (for example the `/fork` rename history) were not independently verified; "Resume across runners" is beta and was not checked by the reviewers.
- Claude Desktop: the session sidebar, worktree defaults, PR monitoring and diff review were not checked in review (the author's text only).
- ACP: `session/fork`, `session/notices` and others are RFD titles only.
- Hermes `/developer-guide/gateway-internals` citation for crash behaviour: copied as the survey writes it (the evidence-path convention in the Hermes file is relative).

## 8. Questions for Round 2

Source code only can settle these. Open-source harnesses only.

Codex (open CLI):
- Rollout files plus SQLite index: what exactly is the write path and recovery after a crash mid-write; are rollout files append-only and how does the index get rebuilt? Is the `thread/rollback` removal documented anywhere in code, and what replaced it?
- Resume: how are saved permissions and network approvals applied against managed policy (precedence) when the session resumes?

OpenCode:
- Snapshot store: how does it attribute changed paths to assistant steps, what is the exact tracked/untracked/ignored rule, and what does the 2 MiB cap do to larger files? How does `/undo` handle a path changed by both the user and the assistant?
- Session database engine and schema, and the event-log contract behind `GET /api/experimental/session/{id}/log` (exclusive sequence, ordering, retention).
- Worktree "strategies" plugin contract and what it may do with repo git config.

Oh My Pi and Pi:
- Branch navigation (`/tree`): what happens to file state, what is stored in `branch_summary`, and how is the active leaf persisted? Is the v3 format append-only or are entries rewritten?
- `omp commit`: how the dependency-ordered split and cycle rejection are actually implemented and what the mechanical fallback commits.
- `github` tool: how "push only to branches it checked out" is enforced.
- Pi `git-checkpoint.ts` example: what it does and whether it is a real checkpoint design.

DeepSeek harness:
- Persistence: zstd frame and checksum format, torn-tail truncation algorithm, per-batch fsync cost, and how `session/end-seed` is used on replay. Whether the `create_worktree` tool hardens git config. The `POST /github` webhook: dedupe and failure behaviour in code.
- Whether undo/checkpoints exist in code even though no page documents them.

OpenHands:
- Event-per-file persistence: crash consistency of `base_state.json` (rewritten whole) versus `events/`; what `fork()` copies and shares; encryption of persisted secrets (`OH_SECRET_KEY`).

OpenClaw:
- Writer claim: the transaction that checks it, behaviour on a superseded writer, and the worktree snapshot ref logic (`refs/openclaw/snapshots/<id>`), including how dirty-idle (>7 days) deletion is guarded.

goose:
- `goose review` flag semantics and orchestration default; session `sessions.db` schema; whether any checkpoint/undo exists in code.

Hermes Agent:
- Shadow-git checkpoints: the destructive-command detection list, the one-per-directory-per-turn rule, how nested repos become gitlinks, the pre-rollback snapshot, and exactly how user edits since the snapshot are detected and skipped. `hermes -w`: how hooks, fsmonitor and filters are disabled at checkout.
- Gateway crash handling: how a turn is marked `unknown` and what discarding does.

Zed/ACP:
- What the removed checkpoint code did (which git operations, how often, on what size repos) and whether any replacement is planned; how the worktree archive restore re-creates git state; commit-message generation prompt and the three-commit style examples.

Warp (open client):
- Handoff snapshot contents (declaration scripts or static JSONL), how untracked files are captured, and the "blocked while a command runs" rule that was unverified.

Cline:
- Shadow git: location on disk, exclusion rules, size handling, commit-per-tool-use cost, what happens to files created after the checkpoint, and the SDK's "one git process" diff mechanism.
- Hub: how sessions are persisted and re-attached across clients.

Not answerable from code (closed): Cursor checkpoint mechanism and retention, Claude Code transcript and checkpoint internals (public docs only), Antigravity `/rewind` behaviour.
