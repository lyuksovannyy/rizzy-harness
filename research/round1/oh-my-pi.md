# Oh My Pi (`omp`) - Round 1 public-surface survey

Surveyed 2026-11-10. Public surface only (omp.sh docs, README, GitHub releases page, one benchmark blog post). No source code read.

Abbreviations for evidence URLs: D = https://omp.sh/docs ; R = https://github.com/can1357/oh-my-pi (README).

## 1. Identity

- Name: Oh My Pi, command `omp`. Maintainer: can1357 (Can Boluk) / Stencil Labs, Inc. Tagline: "a coding agent with the IDE wired in".
- Relationship to Pi: README says it is a fork of Pi (badlogic/pi-mono, Mario Zechner); the README contains no explicit upstream-vs-fork diff, so "added by omp" below is inferred from what the README presents as omp features, except where a doc says so. Compat traces: the older `pi.extensions` package.json key and `pi.typebox` helper still work; env vars use `PI_*` prefixes alongside `OMP_*` (D/extension-authoring, D/env).
- Surfaces: terminal TUI, one-shot CLI, JSON stream, RPC (NDJSON stdio), SDK (Bun/TS), ACP (Zed etc.), collab (shared live sessions with browser client), GitHub integration. Chrome extension (browser relay). No web dashboard or desktop app documented.
- Open: MIT. TypeScript + Rust (README: ~80k lines Rust, six crates plus an N-API addon). Third-party notices shipped per component.
- Version surveyed: release v18.18.6 (2026-11-09) per 00-targets.md and releases page; releases ship several per day (8 in Nov 6-9). Confirmed.
- Correction to 00-targets.md: none found. Note: README says "31 built-in tools", docs/tools lists a different grouping; coreutil counts vary (46/58/67) within the README.

## 2. Features by area

### 1. Agent loop and execution model

### Turn loop with steering and follow-up queue
- What: Enter during a running turn steers it (applied at a safe step boundary); Ctrl+Q/Ctrl+Enter queues a follow-up; queued messages can be pulled back. Esc interrupts; `.`/`c` continues previous intent; Alt+R retries a failed turn. Steering/follow-ups default to one-at-a-time.
- Status: GA
- Evidence: D/using https://omp.sh/docs/using ; D/settings https://omp.sh/docs/settings
- Notes: v18.x.

### `/loop` re-submission
- What: Re-submits a prompt after each turn, unbounded, N times, or until a wall-clock deadline. `loop.mode`: `prompt` (same conversation), `compact` (compact before each replay), `reset` (fresh session each replay).
- Status: GA
- Evidence: https://omp.sh/docs/modes

### `/force` single-tool forcing and `/fast`
- What: `/force <tool>` makes the next model call invoke one named tool then ends the turn. `/fast` requests provider priority serving, per family (OpenAI, Anthropic, Google), auto-disabled if rejected.
- Status: GA
- Evidence: https://omp.sh/docs/modes

### Goal mode
- What: One durable objective pursued across turns with automatic continuation until verified complete, paused, dropped or token budget hit. Objective/status/usage persisted in session; survives compaction; cold-restart restores as paused. Optional total token budget (input+output+cache-write). `/guided-goal` interviews first. Completion only after checking each deliverable against repo state.
- Status: GA
- Evidence: https://omp.sh/docs/goal
- Notes: automatic continuation default only in interactive mode (`goal.continuationModes`). (verified in review)

### Magic keywords
- What: One-turn prompt words: `ultrathink` (max reasoning effort), `orchestrate` (decompose and delegate to subagents then verify), `workflowz` (deterministic multi-agent workflow; needs subagent and a persistent-evaluation capability). Case-sensitive, ignored in code spans/paths. Each toggleable.
- Status: GA
- Evidence: https://omp.sh/docs/magic-keywords ; R. README also lists `chillax` (minimal reasoning); the docs page does not list it (discrepancy).

### Prewalk (mid-session model handoff)
- What: A strong model investigates, writes todos and makes the first file edit, then the session switches once to a cheaper model (default `smol`). Armed with `--prewalk`, `--prewalk-into`, `prewalk.enabled`, or `/prewalk`. Fires once; does not apply to subagents unless configured per agent.
- Status: GA
- Evidence: https://omp.sh/docs/prewalk
- Notes: (verified in review)

### 2. Built-in tools

### Tool inventory (docs/tools)
- What: Explore: `read`, `glob`, `grep`, `ast_grep` (off), `lsp`, `inspect_image` (auto), `web_search`, `github` (off). Change/run: `edit`, `write`, `ast_edit`, `bash`, `eval` (Python/JS on; Ruby/Julia opt-in), `debug` (DAP), `browser`, `computer` (off), `security_scan` (off), `generate_image` (off), `tts` (off). Coordinate: `ask`, `todo`, `task`, `hub`, `checkpoint`/`rewind` (off). Memory (off): `recall`, `retain`, `reflect`, `memory_edit`, `learn`, `manage_skill`. Each tool is classed Read/Write/Exec for approvals. `/settings` toggles tools; `--tools a,b` pins a set; `--no-tools` disables all.
- Status: GA (individual gates noted)
- Evidence: https://omp.sh/docs/tools ; R
- Notes: (verified in review) docs/tools says `ast_grep` off by default but the editing page says structural editing (ast_edit) is on by default and the README lists 50+ grammars; tool counts differ between README (31) and docs.

### `xd://` on-demand tool catalog
- What: Extra tools sit behind `xd://` devices; `read xd://` lists them, `write xd://<tool>` runs one when `tools.xdev` (on by default) is enabled. Affects presentation (smaller default tool surface), not permissions. Also `xd://report_issue` (auto-QA reports, consent on first use, `dev.autoqa`) and `xd://resolve` for accepting staged edits.
- Status: GA
- Evidence: https://omp.sh/docs/tools ; R
- Notes: how xd is meant to reduce prompt size is not explained in the docs I read. docs/tools says `tools.xdev` moves less-used and dynamically discovered tools into an on-demand catalog (presentation only). (verified in review)

### Internal URL schemes
- What: README says sixteen schemes (`pr://`, `issue://`, `agent://`, `skill://`, `ssh://`, `conflict://`, `artifact://`, `security://` seen in docs) usable in file-shaped tools; `agent://<id>/findings.0.path` extracts fields from subagent output.
- Status: GA
- Evidence: R ; https://omp.sh/docs/github ; https://omp.sh/docs/security ; https://omp.sh/docs/compaction
- Notes: the skills docs page says it does not document a `skill://` scheme although README lists it.

### 3. Code editing and file operations

### Hashline edits
- What: Every line returned by read/grep is tagged with a short content hash (example form `2:f1|...`). Edits cite tags rather than reproduce old text; if the file changed, hashes mismatch and the edit is rejected before corruption. Author's benchmark: 16 models, 3 edit formats (apply_patch, str_replace, hashline), React-file mutation fixtures, 3 runs x 180 tasks; hashline beat patch for 14 of 16 models (~+15 points avg); Grok Code Fast 1 6.7%->68.3%; Grok 4 Fast -61% output tokens.
- Status: GA
- Evidence: https://stencil.so/blog/the-harness-problem ; R
- Notes: (verified in review: README section "Hashline: edit by content hash"; blog dated 2026-02-12, author Can Bolukbasi/Boluk is the omp maintainer, so not independent; blog title says 15 LLMs but body and table list 16; blog also describes a hashline v2 that improved 12 of 16 models; code and per-run reports said to be public) vendor's own benchmark, not independently verified. docs/editing and docs/files do NOT describe hashline; only the blog/README do. Release notes mention an alternate edit tool named `sloppy` (v18.18.2) with no explanation in docs read.

### Structural edits (`ast_edit` / `ast_grep`)
- What: Syntax-aware codemods via ast-grep (50+ tree-sitter grammars per README). Changes are staged as Proposed; Accept/Reject card; accepting reruns the rewrite on current files; partial/stale previews apply what matches and flag count mismatch (not transactional). Search cap 1,000 files (`PI_MAX_AST_FILES`).
- Status: GA
- Evidence: https://omp.sh/docs/editing ; R

### Read capabilities
- What: `read` handles directories, large-file structural overview, PDF/docx/pptx/xlsx/EPUB, notebooks as cells, images (>20 MiB rejected), many archive types, SQLite (tables/rows/bounded queries), http(s) pages, `ssh://` paths (remote read capped 1 MiB), GitHub/session resources. `.gitignore` respected. Archive (zip/tar/asar), SQLite rows and notebook cell writes supported.
- Status: GA
- Evidence: https://omp.sh/docs/files

### Git conflict resolution as URLs
- What: Complete conflict blocks get session-local `conflict://N`; writing `@ours`/`@theirs`/`@base` resolves; `conflict://*` bulk. Reference invalid after edit until file re-read.
- Status: GA
- Evidence: https://omp.sh/docs/editing ; R

### LSP-backed diagnostics on write
- What: Diagnostics after whole-file writes on by default; after incremental edits and format-on-write off by default; dedup of already-shown diagnostics.
- Status: GA
- Evidence: https://omp.sh/docs/code-intelligence ; https://omp.sh/docs/files

### 4. Terminal and shell integration

### Embedded bash engine and in-process utilities
- What: README: bash 5.3 implementation in Rust (`pi-shell`) with persistent sessions, in-process coreutils (46/58/67 stated in different README places), minimizer; ripgrep/glob/find run in-process; no WSL needed on Windows. Optional PTY (`--no-pty`), background jobs, `ps`/`pkill` etc.
- Status: GA
- Evidence: R ; https://omp.sh/docs/tools
- Notes: `PI_SHELL_PREFIX` can wrap every shell command (https://omp.sh/docs/env).

### User shell/Python prefixes
- What: `!cmd` runs shell and adds output to context, `!!` excludes from context; `$ code` / `$$ code` same for a shared Python kernel. Not sandboxed.
- Status: GA
- Evidence: https://omp.sh/docs/using

### `eval` persistent kernels
- What: Python and JS cells (Ruby, Julia opt-in) with persistent state; README says they can call back into agent tools (read/search/task) over a loopback bridge.
- Status: GA
- Evidence: https://omp.sh/docs/tools ; R
- Notes: no dedicated eval doc page; sandboxing and kernel lifecycle not documented in pages read.

### Supervised processes
- What: `hub` tool supervises shared processes (needs `launch.enabled`); CLI `omp ps` lists/inspects/logs/stops/restarts supervised processes; `/jobs` for background jobs.
- Status: GA
- Evidence: https://omp.sh/docs/tools ; https://omp.sh/docs/cli

### 5. Permissions, approvals and sandboxing

### Approval modes and per-tool policy
- What: `tools.approvalMode`: `always-ask` (reads free, writes/exec prompt), `write` (exec prompts), `yolo` (nothing prompts; DEFAULT). Per-tool `tools.approval.<name>` = allow/prompt/deny (deny always wins). `bash.patterns` ordered rules (deny/prompt match one segment of compound commands; allow must match whole command). Decision order documented. Built-in bash critical-pattern checks (recursive deletion of absolute paths, curl-to-shell, shutdown, sensitive account files) skipped in yolo. Tools self-declare read/write/exec; undeclared custom tools are exec; MCP tools are write-tier. Headless without UI fails closed when a prompt is needed.
- Status: GA
- Evidence: https://omp.sh/docs/approvals
- Notes: Default yolo is a notable choice; docs suggest `write`. Not a sandbox ("do not sandbox allowed commands"). (verified in review: docs/approvals and docs/tools both state yolo is the built-in default)

### Subagent approval semantics
- What: Subagents run tier decisions as yolo (approval of parent `task` call is the boundary) but inherit the `tools.approval` map (deny blocks, prompt rejects).
- Status: GA
- Evidence: https://omp.sh/docs/approvals
- Notes: docs/subagents adds that each assignment is delegated authority and workers cannot pause for approval. (verified in review)

### Sandboxing
- What: Nothing documented as an OS sandbox. Isolation for subagents is filesystem clone/overlay, explicitly "not a security sandbox". Security scan sessions run restricted (no extensions/MCP/shell). Docs advise a restricted account or container for unattended jobs.
- Status: nothing documented (sandbox)
- Evidence: https://omp.sh/docs/subagents ; https://omp.sh/docs/approvals ; https://omp.sh/docs/env

### Repo trust
- What: README: opening a repo loads its settings, extensions, hooks, tools, commands, skills, rules and project MCP config; narrowing flags `--no-extensions`, `--trusted-extension <path>`, `--no-tools`, `mcp.enableProjectConfig: false`. No trust prompt documented.
- Status: GA (as flags)
- Evidence: R

### Provider safety checks (computer use)
- What: Provider-originated safety checks always require interactive confirmation even in yolo; fail closed without UI.
- Status: GA
- Evidence: https://omp.sh/docs/approvals

### 6. Context management and compaction

### Compaction method chain
- What: Auto-compaction on by default (post-response, mid-turn, after overflow, optionally idle; async pre-preparation). Default `methodOrder`: remote (provider-native) -> snapcompact (older history archived as dense images; needs vision model; no summarisation call) -> handoff (structured summary) -> shake (strip bulk, no summary) -> soft (model-written summary). Pre-step prunes superseded reads and useless output. Threshold by tokens/percent or reserve (larger of 16,384 or 15% of window). `keepRecentTokens` defaults to 20000 and is a target for summarising compaction. Idle compaction is OFF by default (`idleEnabled` false; 200000 tokens / 300 s when on); mid-turn is on by default. Models can define `contextPromotionTarget` (switch to larger model on overflow before compaction).
- Status: GA
- Evidence: https://omp.sh/docs/compaction ; https://omp.sh/docs/custom-models
- Notes: snapcompact is unusual (see section 3); it also needs text its bundled image font can represent. (corrected in review: idle is off by default, not "optionally on"; 20k is a summarising-compaction target)

### `/shake`, `/compact`, `/context`, `/handoff`
- What: `/shake` modes `elide` (heavy output replaced; saved as `artifact://` refs), `images`, `thinking`. `/compact [soft|remote|snapcompact]`. `/context` shows token budget by source. `/handoff` compacts in place with a continuation brief.
- Status: GA
- Evidence: https://omp.sh/docs/compaction ; https://omp.sh/docs/handoff

### Session GC
- What: `omp gc` reports/applies storage cleanup; `--strip` removes provider replay data from idle sessions (v18.18.2).
- Status: GA
- Evidence: https://omp.sh/docs/cli ; https://github.com/can1357/oh-my-pi/releases

### 7. Memory and persistent instructions

### Instruction file discovery (reads other harnesses)
- What: Plain Markdown. Native: `AGENTS.md`, `.omp/AGENTS.md`, `RULES.md` (always-on), `APPEND_SYSTEM.md`, `SYSTEM.md` (replaces built-in body). Compat providers: Claude (`.claude/CLAUDE.md`), `.agent(s)/AGENTS.md`, Codex (`~/.codex/AGENTS.md`), Gemini (`GEMINI.md`), OpenCode, Copilot (`.github/copilot-instructions.md`), plain `AGENTS.md`. One provider wins per depth (shadowing, not concatenation), priority native > claude > agents = codex (tied) > gemini > opencode > github > agents-md. Several compat sources are launch-directory-only or user-wide-only (e.g. codex and opencode are user-wide only; the Claude project file is `.claude/CLAUDE.md` in the launch dir only). (corrected in review: agents and codex are tied, not ordered; scope limits added) Cursor rules, Cline rules, `*.instructions.md` treated as conditional rules. `@path` imports (5 hops, cycle-safe).
- Status: GA
- Evidence: https://omp.sh/docs/context-files ; R (README: "eight existing formats")

### Memory backends
- What: Off by default. `local` (batch distillation of past sessions into `memory_summary.md`/`MEMORY.md`/`learned.md`/`skills/` per project; uses default and smol roles), `mnemopi` (local SQLite, prompt-driven remember/recall/forget, per-project scope, LLM mode smol/remote/none, optional vector/graph/temporal "polyphonic" recall), `hindsight` (remote server, default http://localhost:8888, mental models, retains user/assistant text every 3 user turns; mnemopi default is every 4). `/memory` commands: view, stats, diagnose, enqueue, clear, `mm` (Hindsight mental models). Scoping global/per-project/per-project-tagged.
- Status: GA (autolearn: experimental)
- Evidence: https://omp.sh/docs/memory ; https://omp.sh/docs/tools
- Notes: docs/memory says memory is driven by prompts and slash commands and "does not document named tools", while docs/tools and README list `retain`/`recall`/`reflect`/`memory_edit`/`learn` tools. Internal inconsistency. Per docs/tools: `recall`/`retain`/`reflect` exist only with the hindsight or mnemopi backend, `memory_edit` only with mnemopi, `learn` needs autolearn plus a backend. (corrected in review: per-backend tool availability and mnemopi retain interval added)

### Autolearn and managed skills
- What: `autolearn.enabled` (default false, experimental) captures explicit lessons into `learned.md`; `learn` tool can turn a lesson into a managed skill; `manage_skill` creates/improves/retires auto-learned skills; managed skills in `~/.omp/agent/managed-skills/`.
- Status: experimental
- Evidence: https://omp.sh/docs/memory ; https://omp.sh/docs/tools ; https://omp.sh/docs/skills

### 8. Planning, tasks and modes

### Plan mode
- What: `/plan` (Alt+Shift+P). Read-only turns on the `plan` role model; draft stored as session artifact; full-screen Plan Review with annotate/delete/edit-in-$EDITOR; approve options: execute in fresh session with only the plan, compact then implement, keep full context (blocked >95% of window), refine, or save and quit. Choose implementation role at approval. Headless `--plan-yolo`. Exclusive with goal and vibe.
- Status: GA
- Evidence: https://omp.sh/docs/plan

### Todo tool with phases
- What: `todo` maintains a checklist (blocked/active/complete); README says phase tracking.
- Status: GA
- Evidence: https://omp.sh/docs/tools ; R

### Vibe mode (director + workers)
- What: One session becomes a director with read-only toolset coordinating persistent `fast` (small model) and `good` (strong model) workers sharing one working tree; steer, queue, stop individual workers. Director must verify. Cannot start/fork/handoff while active.
- Status: GA
- Evidence: https://omp.sh/docs/vibe ; R
- Notes: README calls vibe "director mode with read-only toolset"; slash page says "read-only vibe mode". vibe page says "read-oriented". (verified in review)

### Advisor
- What: Second model (role `advisor`) watches each turn in the background, can read/grep/glob, injects notes at levels nit/concern/blocker (blockers can interrupt; `advisor.immuneTurns` default 3). `WATCHDOG.md` for priorities; `WATCHDOG.yml` for a roster of specialist advisors. Print mode waits for review. Own billed requests.
- Status: GA
- Evidence: https://omp.sh/docs/advisor
- Notes: default tools are read/grep/glob; a roster can grant edit, write, bash, eval, browser to advisors. (verified in review)

### `/btw` and `/tan`
- What: `/btw` asks a side question not saved to the conversation; `/tan` runs tangential work in a background agent.
- Status: GA
- Evidence: https://omp.sh/docs/slash
- Notes: one-line description only. (verified in review: slash page)

### 9. Sub-agents and multi-agent orchestration

### `task` tool and bundled agents
- What: Main session decides fan-out and writes each assignment; workers get no parent conversation. Bundled: `scout` (read-only exploration, @smol), `designer` (@designer), `reviewer` (@slow), `security-reviewer` (read-only), `librarian` (external library research, @smol), `task` (general, @task), `sonic` (mechanical edits, @smol). Background by default (`async.enabled`). Limits: `task.maxConcurrency` 32, `maxRecursionDepth` 2, `softRequestBudget` 200 (hard partial at 1.5x), `maxRuntimeMs` 0, idle park after 7 min. Setting `Prefer Task Delegation`: default/preferred/always.
- Status: GA
- Evidence: https://omp.sh/docs/subagents ; https://omp.sh/docs/agents-and-roles
- Notes: README says "schema-validated typed results"; docs/subagents says it does not define a structured results schema; schema is via the agent `output` frontmatter (subagent-authoring). `security-reviewer` has no role and uses the session model fallback. (verified in review)

### Isolated workspaces
- What: `task.isolation.mode` (default none; auto picks clone/overlay backend: APFS/btrfs/zfs reflinks, overlayfs, projfs, rcopy per README). Changes captured and applied to parent as patches (`task.isolation.apply` true, `merge` patch). Needs git repo; unavailable in plan mode; not a security sandbox; isolated workers not revivable.
- Status: GA
- Evidence: https://omp.sh/docs/subagents ; R

### Agent Hub
- What: Alt+A roster/tree of workers with status (running/idle/parked/aborted), model, tokens, requests, tool calls, cost, lineage; open transcripts, steer running workers, follow up idle ones, revive parked (r), kill (x). Persisted across resume.
- Status: GA
- Evidence: https://omp.sh/docs/subagents ; https://omp.sh/docs/agents-and-roles

### Custom agents
- What: Markdown + YAML frontmatter in `.omp/agents/` or `~/.omp/agent/agents/`. Fields: name, description, tools (capability boundary), model (concrete, `@role`, or ordered list), thinking-level, spawns, autoload-skills, read-summarize, output (JSON Schema), blocking, prewalk, advisor. Project beats user beats bundled; `.claude/agents` NOT scanned. `omp agents unpack` writes bundled definitions out. Per-agent overrides: `task.agentModelOverrides`, `agentPrewalk`, `agentAdvisor`.
- Status: GA
- Evidence: https://omp.sh/docs/subagent-authoring ; https://omp.sh/docs/agents-and-roles ; https://omp.sh/docs/cli

### `/review` parallel reviewers
- What: See area 12.
- Status: GA
- Evidence: https://omp.sh/docs/review

### `/cleanse`
- What: Finds project diagnostics and fixes them with parallel workers (also `omp cleanse`).
- Status: GA
- Evidence: https://omp.sh/docs/slash ; https://omp.sh/docs/cli
- Notes: one-line descriptions only.

### 10. Background, long-running and scheduled work

### Background tasks and jobs
- What: Subagents run async; `wait` tool collects background results/peer messages; `bash` can background; `/jobs`; `/pause` pauses agent, subagents and advisor. `/green` watches and repairs CI until branch green. GitHub Actions run-watch via `github` tool.
- Status: GA
- Evidence: https://omp.sh/docs/subagents ; https://omp.sh/docs/slash ; https://omp.sh/docs/github ; R
- Notes: `/green` has one-line description only.

### Scheduling
- What: nothing documented (no cron/scheduled routines found; `/loop` is in-session only).
- Status: nothing documented
- Evidence: https://omp.sh/docs (index)

### 11. Sessions: persistence, resume, fork, checkpoints, undo

### Session persistence and resume
- What: Auto-saved JSONL under `~/.omp/agent/sessions/` (profiles under `~/.omp/profiles/<name>/`). `omp -c`, `-r [id]`, `/resume`, auto-resume setting, picker searches deep text, pinned sessions, interrupted/aborted/error labels, `/rename`, `/move`, `--session-dir`, `--no-session`. Import with `--from-claude`/`--from-codex` and `/resume @claude`.
- Status: GA
- Evidence: https://omp.sh/docs/sessions

### Session tree (branching in one file)
- What: Entries have id/parentId, so rewinding and continuing makes sibling branches in the same file. `/tree` navigator with filters, search, labels (bookmarks), branch summaries (none/summarize/custom prompt). `/branch` (rewind to before a message), `/fork` (clone whole session to new file), `omp --fork`.
- Status: GA
- Evidence: https://omp.sh/docs/session-tree ; https://omp.sh/docs/session-format

### Checkpoint / rewind tools
- What: Agent-facing `checkpoint` and `rewind` tools (off by default, mainly top-level sessions) mark and return to a conversational checkpoint.
- Status: GA (off by default)
- Evidence: https://omp.sh/docs/tools
- Notes: docs/sessions says it describes no file checkpoints or undo; tool-made file changes are not reverted by branching. File-level undo: nothing documented.

### Export and share
- What: `/export` self-contained HTML; `/share` encrypts client-side, key in URL fragment, uploads blob; `omp share --gist`; `/dump` copies transcript plus request JSON (may contain secrets).
- Status: GA
- Evidence: https://omp.sh/docs/sessions ; https://omp.sh/docs/slash

### On-disk format
- What: Documented: JSONL, version 3, optional 256-byte title record, one session header, tree entries (message, model_change, compaction, branch_summary, label, ttsr_injection, custom...). Large images in a content-addressed blob store. Documented for integrators.
- Status: GA
- Evidence: https://omp.sh/docs/session-format

### 12. Git and version control

### `omp commit`
- What: Generates Conventional Commit messages. Agentic default: an agent inspects files/hunks and proposes one commit or a dependency-ordered split (each change exactly once; cycles rejected; lock files excluded from analysis but committed with their manifest; confirmation required for interactive splits). Falls back to a mechanical single commit on agent failure. Validates summary (<=72 chars, past-tense verb, type/file mismatch checks), updates nearest `CHANGELOG.md` unreleased section, runs git hooks, `--push`, `--dry-run`, `--context`, `--model`, `--legacy`. Uses `commit` role; agent uses `smol`.
- Status: GA
- Evidence: https://omp.sh/docs/commit ; R

### `/review` and `/annotate`
- What: Read-only review of base-branch diff, uncommitted Git or JJ changes, a commit, or a GitHub PR; filters lock/generated/vendored files; 1 to up to 16 parallel reviewer slices. Findings P0-P3 with confidence 0.0-1.0, file+line, overall verdict correct/incorrect. `/annotate` pins notes to diff lines (README only; not on the review page).
- Status: GA
- Evidence: https://omp.sh/docs/review ; R

### GitHub as filesystem and `github` tool
- What: Uses `gh` CLI (no separate token). `pr://482`, `issue://731`, `pr://482/diff/all`, list queries with filters; these work without enabling the tool. `github` tool (off by default) can check out a PR into a `pr-<n>` worktree under `~/.omp/wt`, create PRs, push only to branches it checked out, watch Actions with failed-log artifacts. Cache in `~/.omp/cache/github-cache.db`.
- Status: GA
- Evidence: https://omp.sh/docs/github

### Worktrees
- What: Agent-managed worktrees (isolation, PR checkout) under `~/.omp/wt` (`worktree.base`, `OMP_WORKTREE_DIR`); `omp worktree` lists/clears them.
- Status: GA
- Evidence: https://omp.sh/docs/github ; https://omp.sh/docs/cli

### 13. Extensibility

### MCP
- What: Transports stdio, Streamable HTTP, legacy SSE. Config `.omp/mcp.json`, `~/.omp/agent/mcp.json`, profile file, project-root `mcp.json`/`.mcp.json` (lowest). Auto-imports server definitions from Claude Code, Codex, Gemini CLI, OpenCode, Cursor, Windsurf, VS Code and plugins; `/mcp disable` overrides without editing the other app's config. OAuth via `/mcp reauth` stored per profile+URL; `!command` header values; tools named `mcp__<server>_<tool>`; prompts become `/server:prompt`; resources and optional notifications. `/mcp list|test|reload|reconnect|resources|prompts|notifications`. No MCP-specific approval beyond write-tier.
- Status: GA
- Evidence: https://omp.sh/docs/mcp ; https://omp.sh/docs/mcp-authoring ; https://omp.sh/docs/approvals
- Notes: in ACP mode on-disk MCP config is not read; the client supplies servers. docs/mcp: first definition found for a duplicate server wins, no merging; project-root `mcp.json` lowest. The MCP page itself does not state the write-tier (that is on docs/approvals). (verified in review)

### Plugins and marketplaces
- What: Plugin = bundle (extension code, skills, commands, hooks, tools, agents, rules, prompts, MCP/LSP/DAP config). Install from npm, git shorthand (github/gitlab/bitbucket/codeberg/sourcehut), URL, local dir (symlink) or marketplace entry; user or project scope; optional features; `omp plugin doctor`; typed settings. Marketplace catalog `.omp-plugin/marketplace.json` with fallback to Claude-compatible `.claude-plugin/marketplace.json`. No signing, sandboxing or central review; `marketplace.autoUpdate` off/notify(default)/auto.
- Status: GA
- Evidence: https://omp.sh/docs/plugins ; https://omp.sh/docs/marketplace
- Notes: `autoUpdate` `notify` only writes to the debug log (no TUI notice); `auto` lets a publisher ship code without approval; plugins page says "trusted code, not a sandbox". (verified in review)

### Extensions API
- What: TS/JS module factory receiving `ExtensionAPI`: registerTool, registerCommand, registerShortcut, registerFlag, message/thinking renderers, composer shape, registerProvider, file write/delete fallbacks, UI (dialogs, widgets, TUI components). Entry declared as `omp.extensions` in package.json (legacy `pi` key accepted).
- Status: GA
- Evidence: https://omp.sh/docs/extension-authoring ; https://omp.sh/docs/plugins

### Hooks
- What: Hooks are trusted TS/JS modules (not JSON config) in `.omp/hooks/pre|post/` or user dir, or `--hook`. Many events: input, user_bash, tool_call (can block/rewrite input; fails closed on error/timeout, default 30 s), tool_result, context, before_provider_request, before_agent_start, session_before_switch/branch/compact/tree, session_stop (can force a continuation), plus notification events (tool approvals, compaction, retry/fallback, ttsr_triggered, goal_updated, mcp_notification...). Hooks cannot bypass approval policy.
- Status: GA
- Evidence: https://omp.sh/docs/hooks
- Notes: docs do not mention compatibility with Claude Code hook JSON. `session_stop` can request only ONE continuation, not an open-ended loop; non-`tool_call` hook errors do not block. (corrected in review: session_stop limited to a single continuation)

### Skills
- What: `SKILL.md` directories, one level deep; frontmatter description (required), name, hide/disable-model-invocation. Discovered in `.omp/skills`, `~/.omp/agent/skills`, managed-skills, plus `.agent(s)`, `.claude`, `.codex`, `.opencode`, `.github` skills dirs. Natural matching or `/skill:<name>`; allow/ignore lists; per-provider switches.
- Status: GA
- Evidence: https://omp.sh/docs/skills

### Custom tools, prompt templates, slash commands
- What: Custom tools: TS modules at `.omp/tools/<name>/index.ts` (also scans `.claude/tools`, `.codex/tools`), zod/arktype/typebox schemas, declared approval class (default exec). Prompt templates: markdown in `.omp/prompts/` become slash commands with `$1`, `$@`, Handlebars helpers. Also MCP prompts and file commands.
- Status: GA
- Evidence: https://omp.sh/docs/custom-tools ; https://omp.sh/docs/prompt-templates ; https://omp.sh/docs/slash

### TTSR (time-traveling stream rules)
- What: Rule files (`.omp/rules/*.md|.mdc`) with `condition` (JS regex) and/or `astCondition`, `scope` (text, thinking, tool, `tool:name(glob)`), `globs`, `interruptMode`. Regex is matched against the streaming output; on match the stream is aborted, the rule body injected as a reminder and the response regenerated (edit/write calls are stopped before executing). Default fire-once per session (persisted in session); `repeatMode: after-gap`. Bundled built-in rules; `omp ttsr test|list|scan`; `/omfg` turns a complaint into a rule.
- Status: GA
- Evidence: https://omp.sh/docs/ttsr ; https://omp.sh/docs/slash
- Notes: README claim "injections survive compaction" not verified on the TTSR page (page confirmed silent on it). The "zero standing context cost" wording in section 3 is the reviewer-noted inference, not stated by the page. `/omfg` is documented on the slash page, not the TTSR page. (verified in review)

### SDK, RPC, ACP
- What: SDK is Bun 1.3.14+ only (`createAgentSession`, events, custom tools); not Node. RPC: `omp --mode rpc` NDJSON, ready frame, protocol v1 (1 MiB lines) and v2 (chunked frames up to 64 MiB), commands for prompt/steer/follow_up/abort, model, sessions, branch, handoff, compact, bash, subagent inspection, host tools and host URI schemes, extension UI requests (`rpc-ui`). ACP via `omp acp`: editor-owned files/terminals/permissions, session list/load/fork, Plan mode via client; client supplies MCP.
- Status: GA
- Evidence: https://omp.sh/docs/sdk ; https://omp.sh/docs/rpc ; https://omp.sh/docs/acp

### LSP configuration
- What: 14 operations (README). Config `lsp.json|yaml` merged from home, plugin, user dirs (omp, Claude, Codex, Gemini), project; custom servers need command/fileTypes/rootMarkers; servers shared across sessions through a daemon broker; project-local bin dirs take precedence. Selection needs a marker in the launch directory (no parent search).
- Status: GA
- Evidence: https://omp.sh/docs/code-intelligence

### Debugger (DAP) tool
- What: Launch/attach (PID or host:port), conditional breakpoints, continue/step/pause, inspect threads/frames/scopes/variables, evaluate, terminate. One root session at a time. Built-in adapters: gdb, lldb-dap, codelldb, debugpy, dlv, rdbg, js-debug, netcoredbg, kotlin, php, bash, dart/flutter, elixir-ls. Config `dap.json`; adapter chosen by extension and root markers.
- Status: GA
- Evidence: https://omp.sh/docs/debugging
- Notes: README says 28 DAP operations and lists lldb-dap/dlv/debugpy only; docs list many more adapters.

### 14. Models, providers and inference configuration

### Model roles and resolution
- What: Ten roles: default, smol, slow, vision, plan, designer, commit, tiny, task, advisor (README lists nine, omits `designer`). Each maps to a model selector, optional ordered comma-separated alternatives, `@role` aliases, and thinking suffix (off..max, auto). With empty `modelRoles`, unassigned roles auto-resolve from authenticated models (advisor will not inherit default). Hub `/model` (Alt+M) edits roles, storage global or project (`.omp/config.yml`). Ctrl+P cycles `cycleOrder` (default smol, default, slow). Launch overrides `--smol/--slow/--plan` and `PI_*_MODEL`. Agent->role->model chain; per-agent overrides.
- Status: GA
- Evidence: https://omp.sh/docs/roles ; https://omp.sh/docs/agents-and-roles ; R
- Notes: ten roles per docs/roles and docs/agents-and-roles; README says nine (verified in review)

### Fallback chains and usage-aware fallback
- What: `retry.fallbackChains` keyed by role, exact `provider/model`, `provider/*`, or prefix; entries tried in order; `fallbackRevertPolicy` cooldown-expiry (default) or never; `retry.usageAwareFallback` (default false) moves between accounts/models using coding-plan quota reports with a reserve percentage and policy confirm/auto/fail-closed.
- Status: GA
- Evidence: https://omp.sh/docs/roles
- Notes: README also claims round-robin credentials with session affinity and per-credential backoff; docs/providers says it does not describe round-robin selection (`omp dry-balance` CLI exists to simulate OAuth account selection). (verified in review)

### Providers
- What: README claims 60+ providers: frontier APIs, coding-plan/subscription logins (Copilot, Cursor, GitLab Duo, Devin, Kimi, MiniMax, Z.AI, etc.), cloud (Bedrock, Azure, Vertex), gateways (OpenRouter, LiteLLM, Vercel, Cloudflare), local (Ollama, LM Studio, llama.cpp, vLLM). Credential source order: `--api-key`, models.yml key, stored OAuth, `/login` key, env var, others. Local engines auto-detected. Docs state subscription vs API products are separate provider IDs and do not discuss provider terms of service.
- Status: GA
- Evidence: https://omp.sh/docs/providers ; R
- Notes: using subscription OAuth logins inside a third-party harness may conflict with provider ToS; docs are silent (flagged in section 6). docs/providers states no provider total (about 69 IDs by count); "60+" is the README's. (verified in review)

### Custom models (`models.yml`)
- What: `providers` map with baseUrl, api family (openai-completions/responses, codex-responses, azure, anthropic-messages, bedrock-converse-stream, google-generative-ai/gemini-cli/vertex), apiKey (env/`!cmd`/literal), per-model reasoning/thinking mode, cost, context, tokenizer, discovery (ollama, llama.cpp, lm-studio, openai-models-list, litellm, proxy), large compat-flag matrix, `compactionModel`, `contextPromotionTarget`.
- Status: GA
- Evidence: https://omp.sh/docs/custom-models

### Thinking levels
- What: off/minimal/low/medium/high/xhigh/max/auto per model; Shift+Tab cycles; default level `high`.
- Status: GA
- Evidence: https://omp.sh/docs/roles ; https://omp.sh/docs/using ; https://omp.sh/docs/settings
- Notes: default `high` is stated on docs/settings, not docs/roles. (verified in review)

### Media models
- What: `generate_image` (Gemini, GPT, Grok), `tts` (xAI voice, local or provider), `inspect_image` via vision role, `/live` realtime voice, `omp say`.
- Status: GA (generate_image/tts off by default)
- Evidence: https://omp.sh/docs/tools ; https://omp.sh/docs/slash ; https://omp.sh/docs/cli ; R

### 15. UI surfaces

### TUI
- What: Terminal UI (`pi-tui`, differential rendering) with tool cards, status line (model, thinking, mode, path, git, context, cost, session), fuzzy `@` file mentions, image paste, themes (default dark `titanium`), Ctrl+R history search, `/hotkeys`, Alt+A agent hub, `/tree` navigator. Settings tabs: Appearance, Model, Interaction, Context, Memory, Files, Shell, Tools, Tasks, Providers.
- Status: GA
- Evidence: https://omp.sh/docs/using ; https://omp.sh/docs/settings ; R

### Editors and other surfaces
- What: Zed via ACP; collab browser client; Chrome browser-relay extension; release notes mention "Tern" with phone long-press turn menu (a mobile/remote UI) with no docs page.
- Status: GA (ACP, collab); unclear (Tern)
- Evidence: https://omp.sh/docs/acp ; https://omp.sh/docs/collab ; https://github.com/can1357/oh-my-pi/releases
- Notes: Tern appears only in v18.18.6 notes.

### 16. Browser automation and computer use

### Managed browser and relay
- What: `browser` tool (on by default): headless Chromium (downloaded on first use, omp profile) or visible; attach via `browser.cdpUrl`; README says stealth on by default and Electron supported, cmux mentioned in docs/tools. Browser Relay: Chrome extension (`omp browser-relay install`) on 127.0.0.1:9224 to drive your signed-in tabs; optional token; cannot attach to chrome:// or DevTools tabs; no context isolation.
- Status: GA
- Evidence: https://omp.sh/docs/web ; https://omp.sh/docs/tools ; R
- Notes: web docs page does not mention stealth or cmux; README/tools page do.

### Computer control
- What: `computer` tool (off by default; `/computer on`): accessibility tree, screenshots, native mouse/keyboard/scroll/clipboard text. macOS, Linux X11, Windows x64 full; Wayland limited (released binaries omit screenshot capture). Approval: `tools.approval.computer: prompt` recommended; on-screen text cannot grant authorization.
- Status: GA
- Evidence: https://omp.sh/docs/computer ; https://omp.sh/docs/approvals

### Web search and reading
- What: `web_search` chains up to 24 providers with fallthrough (keyed and keyless); `read` of URLs extracts article/PDF/JSON text without JS or cookies; specialised handlers (GitHub, package registries, arXiv, Stack Overflow, docs sites). `omp search` CLI.
- Status: GA
- Evidence: https://omp.sh/docs/web ; R

### 17. Remote, cloud, headless, CI and distributed execution

### SSH
- What: `read`/write/search `ssh://` paths on configured POSIX hosts (remote read cap 1 MiB, Windows targets unsupported); `/ssh` and `omp ssh` manage saved hosts; SSH search is Exec-class.
- Status: GA
- Evidence: https://omp.sh/docs/files ; https://omp.sh/docs/tools ; https://omp.sh/docs/cli
- Notes: no dedicated SSH page; whether commands run remotely through `bash` is not documented in pages read (see Unverified).

### Headless modes
- What: `-p` one-shot, `--mode json` event stream, rpc, rpc-ui, acp; approvals fail closed without UI.
- Status: GA
- Evidence: https://omp.sh/docs/modes ; https://omp.sh/docs/approvals

### Credential broker and gateway (multi-machine)
- What: `omp auth-broker` central credential vault (default 127.0.0.1:8765, loopback-only by default, bearer token, plain HTTP with no TLS of its own; docs advise a private network or TLS-terminating reverse proxy for remote use; clients get API keys and OAuth access tokens, refresh tokens stay on broker; health endpoint needs no bearer). `omp auth-gateway` forward proxy (default 127.0.0.1:4000, its own inbound bearer) so less-trusted clients call providers without receiving credentials; the operator can see request and response content.
- Status: GA
- Evidence: https://omp.sh/docs/secrets ; https://omp.sh/docs/cli ; https://omp.sh/docs/custom-models (`transport: pi-native`)
- Notes: (corrected in review: clients receive API keys as well as access tokens; added loopback default, no-TLS advice and gateway default address/port)

### Remote execution of the agent loop
- What: nothing documented (loop runs where `omp` runs; ACP/RPC/collab host it in one process; no cloud agents).
- Status: nothing documented
- Evidence: https://omp.sh/docs (index)

### 18. Collaboration, teams and enterprise administration

### Collab
- What: `/collab` publishes a live session via relay (default `wss://my.omp.sh`); full-control and view-only links (bearer secrets, no accounts); guests join by `omp join`, `/join` or browser. Host's omp still runs model and tools; guest prompts run under host policies. Content AES-256-GCM sealed client-side, key in URL fragment; relay sees metadata. Custom relay must implement omp's relay service; no production self-host relay shipped.
- Status: GA
- Evidence: https://omp.sh/docs/collab
- Notes: (verified in review)

### Enterprise administration
- What: nothing documented (no org policy, SSO, managed settings). Closest: `--config` overlay files, `PI_CONFIG_FILES`, `disabledProviders`, `OMP_AUTH_BROKER_ACCOUNT_POOL_FILE`, `PI_BLOCKED_AGENT`.
- Status: nothing documented
- Evidence: https://omp.sh/docs/settings ; https://omp.sh/docs/env

### 19. Observability

### Usage and stats
- What: `/usage` provider limits and resets; `/stats` and `omp stats` launch a local usage dashboard (`omp-stats` package per README; `-j` JSON, `-s` summary); `omp bench` model latency/throughput; status line shows cost; Agent Hub shows per-worker cost.
- Status: GA
- Evidence: https://omp.sh/docs/slash ; https://omp.sh/docs/cli ; R
- Notes: no dedicated stats docs page (omp.sh/docs/stats returns 404); dashboard contents unverified.

### OpenTelemetry and logging
- What: OTLP export (http/protobuf only) only when an endpoint env is set; optional message-content capture via GenAI env var (warned to expose prompts); logs under `~/.omp/logs/`; `PI_TIMING`, `--log-level debug`, various trace env vars.
- Status: GA
- Evidence: https://omp.sh/docs/env ; https://omp.sh/docs/acp

### Auto-QA reports
- What: `xd://report_issue` records reports when a tool seems to contradict its contract; on by default (`dev.autoqa`), first report asks for consent; `omp grievances` lists/cleans/pushes them.
- Status: GA
- Evidence: https://omp.sh/docs/tools ; https://omp.sh/docs/cli
- Notes: where "push" sends reports is not stated.

### 20. Configuration and customisation

### Config files and precedence
- What: YAML `~/.omp/agent/config.yml` (global or named profile, which replaces rather than inherits), project `<cwd>/.omp/config.yml` (launch dir only), `PI_CONFIG_FILES`/`--config` overlays, flags. Maps deep-merge, scalars/arrays replace. `omp config list|get|set|reset` writes global only. Invalid YAML moved aside as `*.broken-*` and startup stops. Profiles isolate settings, auth, caches, sessions. XDG support when dirs exist. Nix flake with home-manager/NixOS modules.
- Status: GA
- Evidence: https://omp.sh/docs/settings ; https://omp.sh/docs/env ; R

### Themes and keybindings
- What: Theme and keybinding pages exist in the docs index (not read in detail).
- Status: unclear
- Evidence: https://omp.sh/sitemap.xml
- Notes: pages themes and keybindings listed but not opened.

### 21. Security and privacy

### Credential storage
- What: Local SQLite `agent.db` (dir 0700, file 0600 on POSIX), not encrypted at rest; docs describe no automatic secret redaction in logs/transcripts/exports. Share redacts configured secrets by default. Broker caches an encrypted snapshot keyed from bearer+URL.
- Status: GA
- Evidence: https://omp.sh/docs/secrets ; https://omp.sh/docs/sessions

### Security scans
- What: Opt-in (`security.enabled`) read-only scans: native review by restricted bundled workers (no extensions, MCP, shell, fallback), plan/scan/validate/disposition/compare/export workflow with findings stored per project and SARIF export; or import from Codex Security cloud. Needs OAuth login for the provider (API key not enough).
- Status: GA (off by default)
- Evidence: https://omp.sh/docs/security ; R (README adds NVD/OSV/CISA KEV data)
- Notes: (verified in review)

### Telemetry and trust
- What: No default telemetry documented (quickstart/env pages mention none except opt-in OTEL, auto-QA reports, and update checks `omp update`). MCP, plugins, hooks, skills, custom tools all treated as trusted code; repo content auto-loaded.
- Status: GA
- Evidence: https://omp.sh/docs/env ; https://omp.sh/docs/plugins ; https://omp.sh/docs/mcp

### 22. Licence, pricing and distribution

### Distribution
- What: MIT. Install via curl script (prebuilt binary from build.stencil.so with SHA-256 check, or bun), Homebrew tap, bun global (needs bun >=1.4.4), Nix flake, mise, PowerShell. `omp update` with canary/stable. Platforms linux/macOS/Windows x64+arm64 (x64 AVX2 and baseline variants). PRs temporarily open to everyone. No pricing (BYO model/subscription).
- Status: GA
- Evidence: R ; https://omp.sh/docs/quickstart ; https://omp.sh/docs/cli
- Notes: SDK page says Bun 1.3.14+, README says bun >=1.4.4 for install (possible discrepancy/doc lag).

### 23. Anything else or unusual

### Native Rust core
- What: README: `pi-shell` (~38k LoC bash engine), `pi-natives` (~25k, N-API: desktop, grep, text, snapcompact, keys, ast, diff, pty, highlight, clipboard, tokens...), `pi-walker` (parallel walker/glob/regex), `pi-iso` (isolation backends), `pi-ast`, `pi-voice`, `pi-edit`, `pi-builtins`.
- Status: GA
- Evidence: R

### Other CLI utilities
- What: `omp compress` rewrites text files denser, `omp tiny-models` local small models, `omp render`, `omp gallery`, `omp token` (prints stored credential), `omp dry-balance`, `omp images`.
- Status: GA
- Evidence: https://omp.sh/docs/cli
- Notes: one-line descriptions only.

## 3. Distinctive or unusual

- Hashline edit anchors with a published 16-model benchmark: edit-format as a measured lever, not a given.
- TTSR: regex over the live token stream aborts and retries with an injected rule; zero standing context cost.
- Snapcompact: older context archived as images by a vision model with no summarisation call.
- Compaction as a configurable fallback chain (remote, snapcompact, handoff, shake, soft) plus idle and mid-turn triggers.
- Ten model roles with `@role` aliases, ordered selectors and per-provider/prefix fallback chains; usage-aware fallback across subscription quota.
- Advisor models (including a WATCHDOG.yml roster) reviewing a live session in the background.
- Session tree inside one JSONL file (id/parentId) with documented on-disk schema.
- Reads other harnesses' config natively: AGENTS/CLAUDE/GEMINI/Copilot/OpenCode instruction files, skills dirs, MCP configs, `.claude-plugin` marketplaces, Cursor/Cline rules; imports Claude/Codex history.
- Internal URL schemes (`pr://`, `conflict://`, `agent://`, `artifact://`, `security://`) in place of dedicated tools; `xd://` on-demand tool catalog.
- Auth broker plus auth gateway: credentials never reach untrusted clients.
- Collab with end-to-end sealed relay and view-only links.
- Native DAP debugger, LSP with project-local server selection and shared daemon, ast-grep codemods with preview/accept.

## 4. Architecture and UX notes

- Single process per session: TUI, agent loop and tools run on the machine of `omp` (Bun/TS runtime with Rust N-API addon). RPC/SDK/ACP/collab all embed or wrap that same process; collab host executes everything, guests are remote views (https://omp.sh/docs/collab).
- LSP servers shared across sessions via a local daemon broker (https://omp.sh/docs/code-intelligence); credentials optionally centralised in a broker (https://omp.sh/docs/secrets).
- Subagents are separate model sessions (process/launch command overridable via `PI_SUBPROCESS_CMD`), headless, persisted in the parent session's artifact directory (https://omp.sh/docs/env ; https://omp.sh/docs/session-format).
- Release cadence of several versions per day with occasional breaking changes (e.g. `ask` tool schema, v18.18.3).

## 5. Relevance to a self-hosted master/node harness

- Roles with aliases and fallback chains (area 14): a clean model-routing abstraction to copy; usage-aware fallback is the part to consider carefully.
- Auth broker/gateway (area 17): the same problem as master-holds-keys, node-never-sees-keys; note their broker is plain HTTP with a single admin bearer, which a master/node link with pinned TLS could improve on.
- Approval design (area 5): tier-based Read/Write/Exec classes plus ordered per-tool and bash patterns is a good model; default yolo and subagents inheriting yolo are what NOT to copy.
- Session tree in a single event log with documented schema (area 11): matches the "UI is a fold over the log" idea; branch summaries and labels are cheap UX wins.
- Hooks fail-closed on `tool_call` and cannot bypass approvals (area 13): good invariant.
- TTSR (area 13) is cheap to implement in a node-side stream filter and keeps rules out of the context.
- Compaction chain (area 6): `shake` (elide bulk to recoverable artifacts) is simple and does not need an LLM; consider that before summarisation.
- Subagent isolation via clone/overlay and patch merge (area 9): relevant to nodes; their own docs say it is not a sandbox.
- Collab sealed relay (area 18): a model for a master-relayed shared session; E2E would conflict with a master that must read the log, so a design choice to make.
- Instruction/skill/MCP import from other harnesses (areas 7, 13): low-cost adoption feature.

## 6. Overhyped, stale or contradictory

- README tool count (31) and coreutil counts (46/58/67) disagree internally; README lists nine roles, docs list ten (`designer`).
- README says `skill://` scheme exists; skills doc says it documents none. README lists `chillax`; magic-keywords page does not.
- docs/memory says no named memory tools; docs/tools and README list five.
- README: "schema-validated typed results" for task; subagents doc says no formal results schema is defined (schema only via agent `output` frontmatter).
- README claims round-robin credentials with session affinity; providers doc does not describe it.
- Hashline benchmark numbers are the author's own; docs/editing does not even mention hashline.
- "Time-traveling" is marketing: the mechanism is abort-and-retry, with partial output discarded by default.
- Default approval mode is `yolo`; the headline "IDE wired in" and 60+ providers are vendor claims.
- SDK requires Bun 1.3.14+ vs README install Bun >=1.4.4.
- docs/tools says `ast_grep` is off but README lists it as a headline tool.
- No docs page for stats, SSH, eval, themes read, keybindings read; the sitemap has no changelog.

## 7. Unverified

- Hashline exact edit-tool schema and fallback behaviour: only in the blog post/README; would need docs/tools reference or `/tools` output.
- Python/JS `eval` kernel isolation, tool-callback bridge security: no doc page; need an eval doc or README section.
- SSH: whether `bash` can run remotely, host trust, key handling; no SSH page.
- `omp stats` dashboard contents and where data lives; no page (404).
- Differences from upstream Pi beyond README feature list; needs a changelog or fork notes (none found).
- Tern UI (v18.18.6 release note) and `ovnfs` daemon (v18.17.0): mentioned only in release notes, no docs.
- Releases page beyond ~100k chars and v18.15/18.16 notes were not read.
- Themes, keybindings docs not opened in detail.
- Whether TTSR injections persist through compaction (README claim).
- Data sent to `my.omp.sh` relay and the share service beyond what collab/session docs state.

## 8. Sources

- https://github.com/can1357/oh-my-pi (README; features, tools, Rust crates, install, models)
- https://github.com/can1357/oh-my-pi/releases (release notes v18.17-18.18; first ~100k chars)
- https://stencil.so/blog/the-harness-problem (hashline benchmark)
- https://omp.sh/ (homepage, partial), https://omp.sh/sitemap.xml (page list)
- https://omp.sh/docs (index), /quickstart, /using, /modes, /settings, /sessions, /session-tree, /session-format, /memory, /compaction, /plan, /goal, /handoff, /files, /code-intelligence, /debugging, /editing, /review, /commit, /security, /subagents, /advisor, /vibe, /collab, /web, /computer, /github, /providers, /roles, /agents-and-roles, /custom-models, /prewalk, /context-files, /skills, /prompt-templates, /magic-keywords, /hooks, /custom-tools, /subagent-authoring, /mcp, /mcp-authoring, /ttsr, /plugins, /extension-authoring, /marketplace, /sdk, /rpc, /acp, /cli, /env, /secrets, /approvals, /tools, /slash (all under https://omp.sh/docs/)
- Not opened: /keybindings, /themes. 404: /docs/stats.

## 9. Review

Reviewed 2026-11-10 by an independent reviewer (public pages only, no source). Checked about 30 of roughly 95 entries, each by opening the cited omp.sh doc page (or README / blog) and asking for the specific fact.

| Entry | Verdict | Reason |
|---|---|---|
| Model roles and resolution | confirmed | 10 roles on docs/roles and agents-and-roles; README says nine |
| Fallback chains / usage-aware fallback | confirmed | chain keys, `cooldown-expiry` default, usageAwareFallback false, cycleOrder confirmed |
| Credential broker and gateway | confirmed-with-correction | clients also get API keys; loopback default, no TLS, gateway 127.0.0.1:4000 added |
| Hashline edits and benchmark | confirmed-with-correction | numbers match blog; blog title says 15 LLMs, body 16; dated 2026-02-12; author is the omp maintainer |
| TTSR | confirmed | fields, abort-and-regenerate, fire-once, repeatMode all on page; compaction survival not stated |
| Compaction method chain | confirmed-with-correction | idle compaction off by default; 20k keep-recent is a summarising target |
| `task` tool and bundled agents | confirmed | limits and bundled roles match |
| Subagent approval semantics | confirmed | docs/approvals as written |
| Approval modes (default yolo) | confirmed | "built-in default is yolo" |
| Sandboxing (none documented) | confirmed | isolation "not a general security sandbox" |
| Memory backends | confirmed-with-correction | per-backend tool availability and mnemopi retain interval added |
| Autolearn | confirmed | default false, experimental |
| Instruction file discovery | confirmed-with-correction | agents and codex are tied; scope limits |
| Tool inventory / ast_grep off | confirmed | defaults table matches |
| `xd://` catalog | confirmed | `tools.xdev` on by default |
| Goal mode | confirmed | budget, continuation, cold restart |
| Prewalk | confirmed | smol default, fires once |
| Advisor | confirmed | immuneTurns 3, WATCHDOG files |
| Vibe mode | confirmed | director read-oriented, shared tree |
| Hooks | confirmed-with-correction | session_stop gives a single continuation only |
| MCP | confirmed | import list, precedence, naming |
| Plugins and marketplaces | confirmed | autoUpdate notify default, logs only |
| Security scans | confirmed | opt-in, restricted, SARIF, OAuth |
| Collab | confirmed | relay URL, AES-256-GCM, no self-host relay |
| Providers | confirmed | README-only "60+" and round-robin claims flagged |
| Thinking levels | confirmed | default high on docs/settings |
| `/btw` `/tan` `/omfg` `/green` `/cleanse` `/loop` `/force` `/fast` | confirmed | slash page; `/annotate` not on it |
| Structural edits | confirmed | 1,000-file cap, staged accept/reject |
| Session persistence / import | confirmed | paths, `--from-claude`, share key in fragment |
| Marketplace fallback catalog | confirmed | `.claude-plugin/marketplace.json` fallback |

Not confirmed: the `/dump` secrets warning (sessions page does not mention it; not checked on slash page).

Borrowed features: none found. Hashline/TTSR descriptions match omp's own pages. One inference to treat with care: section 3's "zero standing context cost" for TTSR is not stated by the TTSR page.

Notable omissions (docs nav at https://omp.sh/docs): Keybindings and Themes pages (survey admits not opened); Session tree/format are covered. No major undocumented-by-survey feature found; stats, SSH, eval, python, LSP, IRC have no pages in the nav (confirms survey's "no page" notes). Release notes were not re-read.

Trust judgement: this file is reliable. All 30 checked entries had the feature on the cited page; 5 needed small detail corrections and none were wrong. The unchecked remainder is mostly one-line or README-derived entries; the author flagged README/docs conflicts honestly. Treat README-only claims (tool count 31, round-robin credentials, schema-validated results) as vendor claims.
