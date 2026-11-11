# goose: Round 1 public-surface survey

Surveyed 2026-11-10 from public docs (goose-docs.ai, markdown twins of pages), the GitHub repository page, the releases page, GOVERNANCE.md and the Linux Foundation announcement. No source code read. Doc pages are fetched through a summarising fetcher, so wording is paraphrase; where a page was silent the entry says so.

## 1. Identity

- Name: goose. Vendor/steward: originally Block; now part of the Agentic AI Foundation (AAIF), hosted at the Linux Foundation. Block is listed as a platinum member on the AAIF announcement (checked in review).
- Surfaces: desktop app (macOS, Linux, Windows), CLI, an ACP server (`goose serve` over HTTP/WebSocket, `goose acp` over stdio) for embedding, and "GDK" Rust crates for building agents (README, docs index).
- Open source, Apache-2.0, Rust core (README).
- Repo: https://github.com/aaif-goose/goose (moved from `block/goose`; the redirect is stated in 00-targets.md, the README page itself did not mention the move).
- Version: the releases page shows **v1.59.0 on 10 Nov** as newest, then v1.58.1 on 10 Nov (page shows day/month only; year assumed 2026). `00-targets.md` lists v1.58.1 as newest; it is one release behind by the time of survey. Stars (56.4k) not re-verified.
- Governance confirmed (see entry "Governance move" below): the Linux Foundation announcement of 2025-12-09 names goose as a founding project of the AAIF; GOVERNANCE.md says it is "a Series of LF Projects, LLC".

## 2. Features by area

### Area 1. Agent loop and execution model

### Agent loop as a state machine (GDK)
- What: GDK models an agent as an ordered list of steps (operations plus inference steps) composed into a `StateMachine`. Each pass loads the session via an app-supplied loader, tries steps in order until one applies, sends effects to an app-supplied handler, then repeats or yields. The application owns persisted state, so a fresh machine can resume a saved conversation; cancellation drops the running step and lets operations finalize interrupted tool responses. Persistence does not give exactly-once semantics for external actions.
- Status: unclear (page gives no maturity label; release notes show the loop was reworked recently)
- Evidence: https://goose-docs.ai/docs/gdk/concepts/state-machine.md (State machine); https://goose-docs.ai/docs/gdk/index.md (GDK)
- Notes: release notes say v1.55.0 (16 Oct) deleted the "legacy agent loop" and v1.58.0 (9 Nov) replaced Agent/AgentManager with one `StateMachineServices` per process (https://github.com/aaif-goose/goose/releases). Docs may lag.

### Turn limit (max turns)
- What: caps consecutive turns without user input; default 1000. On reaching the limit goose asks whether to continue. Set via `GOOSE_MAX_TURNS`, desktop settings, or `--max-turns`; subagents have a separate precedence chain (see area 9). CLI also has `--max-tool-repetitions`.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/sessions/smart-context-management.md (Max Turns); https://goose-docs.ai/docs/guides/goose-cli-commands.md (session flags)
- Notes: the `--max-tool-repetitions` flag is listed but not explained. (verified in review: GOOSE_MAX_TURNS default 1000)

### Interrupt and queue messages
- What: desktop queues messages typed while the agent works (editable, reorderable, send-now interrupts). Typing keywords like "stop" or "wait" interrupts; CLI uses Ctrl+C.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/sessions/in-session-actions.md (Queuing Messages, Interrupting a Task)
- Notes: keyword-based interruption heuristics, an unusual choice.

### Area 2. Built-in tools

### Built-in and platform extensions as the tool set
- What: tools come from extensions. Built-in: Developer (on by default), Computer Controller, Memory, Tutorial, Auto Visualiser. Platform extensions (global features): Analyze (default on), Apps, Chat Recall, Code Mode, Extension Manager (default on), Skills (default on), Summon (default on; knowledge loading and subagent delegation), Todo, Top of Mind.
- Status: GA
- Evidence: https://goose-docs.ai/docs/getting-started/using-extensions.md (Built-in Extensions, Platform Extensions)
- Notes: the individual tools inside Developer are not listed in any page I opened (quickstart and file-management pages do not name them). Hook docs refer to tool names `shell` and `edit`, and adversary mode to `shell`, so those exist (https://goose-docs.ai/docs/guides/context-engineering/hooks.md; https://goose-docs.ai/docs/guides/security/adversary-mode.md).

### Analyze tool (code structure)
- What: `analyze` has three modes: structure (directory), semantic (file: functions, classes, imports) and focus (symbol: definition plus callers/callees, `follow_depth` default 2, `max_depth` default 3). Results over 50,000 characters return a warning unless `force=true`.
- Status: GA (enabled by default; no label)
- Evidence: https://goose-docs.ai/docs/guides/codebase-analysis.md (Analyze Tool)
- Notes: supported languages not listed on the page.

### Todo and Chat Recall
- What: Todo manages task lists across sessions; Chat Recall searches conversations across session history. Todo list is also injected per turn alongside timestamp and working directory (persistent-instructions page).
- Status: unclear
- Evidence: https://goose-docs.ai/docs/getting-started/using-extensions.md; https://goose-docs.ai/docs/guides/context-engineering/using-persistent-instructions.md
- Notes: no dedicated page for either.

### Tool output size cap
- What: `GOOSE_MAX_TOOL_RESPONSE_SIZE` (default 200000) sends over-size tool responses to a temp file. Older tool outputs are also summarised in the background (see area 6).
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/environment-variables.md (Developer Extension Settings)
- Notes: the page states the unit is characters (corrected in review: unit is characters, not "not stated"; https://goose-docs.ai/docs/guides/environment-variables.md).

### Area 3. Code editing and file operations

### File access and editing
- What: nothing documented about the edit mechanism. The file-management page covers only desktop `@` fuzzy file search (5 levels deep, skips `.git`, `node_modules`, `target`, `dist`, `build`) and safe-practice advice. No `.gooseignore` is mentioned on that page.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/file-management.md
- Notes: approval-mode page mentions "text editor writes and edits" as write operations (https://goose-docs.ai/docs/guides/managing-tools/goose-permissions.md).

### Area 4. Terminal and shell integration

### `goose term` shell integration
- What: `goose term init <shell>` in the shell rc file defines `@goose`/`@g` aliases that ask goose questions from the prompt. Each terminal gets its own session by default; `--name` attaches a persistent named session; `--default` makes goose handle unresolvable commands (zsh, bash, Nushell). Command-history capture is off by default; `--dangerous-include-history` sends commands since the last question to the provider, including any secrets in them. `goose term info` shows context usage and model in the prompt. Shells: zsh, bash, fish, Nushell, PowerShell.
- Status: GA (no label)
- Evidence: https://goose-docs.ai/docs/guides/terminal-integration.md
- Notes: `AGENT_SESSION_ID` env var exposes the session id in the terminal.

### Shell used for tool commands
- What: `GOOSE_SHELL` overrides the shell for Developer extension commands (default bash else sh on Unix, cmd on Windows). `GOOSE_TERMINAL=1` and `AGENT=goose` are set when goose runs commands, so shell configs can detect it. `GOOSE_SEARCH_PATHS` prepends directories to PATH for extension commands.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/environment-variables.md
- Notes: none.

### Area 5. Permissions, approvals and sandboxing

### Four permission modes
- What: Autonomous (default; no approval, can modify and delete files), Manual Approval (confirm every tool/extension use), Smart Approval (auto-approve low-risk, flag others), Chat Only (no extensions or file changes). Each session has its own mode; changing the default affects only new sessions. CLI: `/mode auto|approve|smart_approve|chat`, or `GOOSE_MODE`.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/managing-tools/goose-permissions.md
- Notes: the page says read/write classification is best-effort and done through the LLM provider; Smart Approval criteria are not described. Prompts target write operations only (editor writes, `rm`/`cp`/`mv`). Subagents are disabled in non-autonomous modes (subagents page). (verified in review)

### Per-tool permissions
- What: per tool, three levels: Always Allow, Ask Before, Never Allow; overrides the mode default; applies only in Manual and Smart modes. Set in desktop (mode settings, per extension) or `goose configure` then goose settings, Tool Permission. Config reference lists `permission.yaml` (levels) and `permissions/tool_permissions.json` (runtime decisions, managed automatically).
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/managing-tools/tool-permissions.md; https://goose-docs.ai/docs/guides/config-files.md (Related files)
- Notes: the tool-permissions page does not say where settings are stored; the config page supplies the filenames. (verified in review: the filenames and levels; the three-level UI text on the tool-permissions page was not re-opened)

### Sandboxing
- What: no sandbox page is reachable today and none is in the docs map. The security index does not mention sandboxing; the extensions page says that with the Developer extension goose can run commands and modify files without approval in default mode, and does not mention a sandbox; the environment-variable page has no sandbox section. Optional `--container` runs extensions in Docker (devcontainer workflows). However, a search of goose-docs.ai returned a page `https://goose-docs.ai/docs/guides/sandbox` (also `block.github.io/goose/docs/guides/sandbox`, titled "macOS Sandbox for goosed") whose snippets say an opt-in macOS-only sandbox for Goose Desktop exists: `GOOSE_SANDBOX` (default false, set before launching the app) confines the goosed process with Apple Seatbelt (`sandbox-exec`) and routes network traffic through a local egress proxy; shipped in v1.25.0 per a third-party changelog mirror (Feb 2026). Both URLs returned HTTP 404 when fetched in review, so this is unverified from the page itself. An AAIF blog post dated 2026-11-06 says goose does not include sandboxing by default.
- Status: unclear (macOS Desktop opt-in sandbox reported by search snippets only; no sandbox documented for CLI, Linux or Windows in pages that open)
- Evidence: https://goose-docs.ai/docs/guides/security/index.md; https://goose-docs.ai/docs/getting-started/using-extensions.md; https://goose-docs.ai/docs/goose-docs-map.md; https://aaif.io/blog/give-goose-sandbox-powers-with-opensandbox-and-mcp (opened in review); search snippet only: https://goose-docs.ai/docs/guides/sandbox
- Notes: Code Mode's JavaScript runtime is isolated (no fs/shell/net); that is a different thing. (corrected in review: original "nothing documented" and the claim that the env-var page "states no sandbox variables exist" were not supported; a macOS Desktop sandbox with `GOOSE_SANDBOX` appears in search results but the page could not be opened, so it is also listed in Unverified)

### Adversary mode (LLM reviewer of tool calls)
- What: an independent reviewer, using the configured provider/model, sees the original task, recent messages and the tool call before each call and returns ALLOW or BLOCK against rules in `~/.config/goose/adversary.md`. Blocked calls cannot be retried. Reviewer errors fail open. By default only `shell` is reviewed; a `tools:` header line adds others. Enabled by the file existing.
- Status: unclear (no label)
- Evidence: https://goose-docs.ai/docs/guides/security/adversary-mode.md
- Notes: fail-open is a notable design choice (the page says a reviewer failure lets the call through). (verified in review)

### Prompt injection detection
- What: before a tool runs, parameter text is matched against threat patterns (deleting system files, remote script download-and-run, exfiltrating SSH keys, weakening security). Optional ML classifier gets tool-call content and recent messages via a configurable HTTP endpoint (Hugging Face text-classification format; spec published). Score at or above threshold (default 0.8, range 0.01 to 1.0) pauses execution with a finding id for the user to allow or deny; decisions are logged with `SEC-{uuid}` ids.
- Status: unclear; default off per the environment-variable page (`SECURITY_PROMPT_ENABLED` false)
- Evidence: https://goose-docs.ai/docs/guides/security/prompt-injection-detection.md; https://goose-docs.ai/docs/guides/security/classification-api-spec.md; https://goose-docs.ai/docs/guides/environment-variables.md (Security); https://goose-docs.ai/docs/guides/logs.md
- Notes: the detection page itself does not state a default; the env-var and config pages say false. Docs call it a safeguard, not a guarantee. (verified in review: threshold 0.8, range 0.01-1.0, patterns, classifier endpoint; default false confirmed on the env-var page)

### Extension allowlist
- What: an admin hosts a YAML list (`id`, `command`) at a URL and sets `GOOSE_ALLOWLIST`; install of an extension is rejected unless its command matches exactly. Fetched on first need and on restart. Unset means no restriction. Release v1.57.0 notes an HTTPS-only allowlist with redirect validation.
- Status: GA (no label)
- Evidence: https://goose-docs.ai/docs/guides/allowlist.md; https://github.com/aaif-goose/goose/releases (v1.57.0)
- Notes: it gates installation, not runtime; the page does not say it blocks already-configured extensions. (verified in review)

### Malware check on external extensions
- What: goose checks external extensions for known malware before activation and blocks flagged packages with an error.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/getting-started/using-extensions.md (Extension Types)
- Notes: one sentence only; mechanism and data source not documented in pages opened. (verified in review)

### Deeplink restriction for extension install
- What: `goose://extension` deeplinks accept only base commands `jbang`, `npx`, `uvx`, `goosed`, `docker`; all params URL-encoded.
- Status: GA
- Evidence: https://goose-docs.ai/docs/getting-started/using-extensions.md (Deeplinks)
- Notes: lists `goosed` as a command, so that name is still in use somewhere. (verified in review)

### Secrets storage
- What: provider API keys are not read from `config.yaml` (it causes an auth error). Preferred storage is the system keyring; fallback is plaintext `secrets.yaml` (no keyring, `GOOSE_DISABLE_KEYRING`, or keyring failure). Env vars take precedence.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/config-files.md (Secrets storage)
- Notes: none. (verified in review: keyring fallback to plaintext `secrets.yaml`, env precedence; the "auth error" detail not re-checked)

### Area 6. Context management and compaction

### Auto-compaction
- What: compaction triggers at min(0.8 x context window, 225,000 tokens) by default (`GOOSE_AUTO_COMPACT_THRESHOLD`, `GOOSE_AUTO_COMPACT_TOKEN_LIMIT`); disabled by threshold 0.0. Older conversation is summarised; full history stays visible but only the compacted version is active context. The `compaction.md` prompt template is editable. The cap is a trigger, not a request-size guarantee.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/sessions/smart-context-management.md (Auto-Compaction); https://goose-docs.ai/docs/guides/context-engineering/prompt-templates.md
- Notes: 225k default cap came in v1.55.0 (release notes). v1.58.1 reworded the compaction prompt because a model refused it (releases page). (verified in review: 80% / 225,000 trigger, 0.0 disables, /summarize deprecated alias)

### Tool output summarisation
- What: older tool-call outputs are summarised in the background, recent ones kept in full; cutoff derived from context limit and threshold, tunable with `GOOSE_TOOL_CALL_CUTOFF`.
- Status: GA (no label)
- Evidence: https://goose-docs.ai/docs/guides/sessions/smart-context-management.md (Tool Output Handling)
- Notes: none.

### Context-limit strategies
- What: summarise (desktop and CLI), truncate oldest, clear, or prompt (CLI only); `GOOSE_CONTEXT_STRATEGY`. Desktop always summarises; headless `goose run` summarises. Manual: `/compact` in CLI, "Compact now" in desktop.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/sessions/smart-context-management.md
- Notes: `/summarize` is deprecated.

### Context window override and indicators
- What: `GOOSE_CONTEXT_LIMIT` (env, CLI only) overrides the assumed window; precedence goes through custom provider config, provider discovery, canonical model metadata, default 128,000. Colour indicators for usage in desktop and CLI. v1.58.0 notes a context window report and viewer, and an opt-in "context relevance operation".
- Status: GA for override; context report unclear
- Evidence: https://goose-docs.ai/docs/guides/sessions/smart-context-management.md; https://github.com/aaif-goose/goose/releases (v1.58.0)
- Notes: the override affects only displayed usage per the page; I did not find docs for the context report or relevance operation. unverified detail.

### Tool deferral (lazy MCP tool schemas)
- What: setting `GOOSE_TOOL_DEFERRAL` to `lookup` (the only listed value; off unless set) publishes MCP tool names up front and enables schemas on demand through `enable_tools`. v1.59.0 notes mention "minimal deferred MCP tools".
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/config-files.md (Key settings); https://github.com/aaif-goose/goose/releases (v1.59.0)
- Notes: the environment-variable page does not cover it; no dedicated page found. (corrected in review: mechanism is described in one line on the config-files page: names up front, schemas on demand via `enable_tools`)

### Code Mode (tool-call batching via JavaScript)
- What: with the Code Mode platform extension, the model sees three meta-tools (`list_functions`, `get_function_details`, `execute_javascript`) and writes JS run in a fresh Boa context with `tools.call()`; independent calls can be batched, intermediate results stay local. No fs/shell/network/env access; text-only results; many caps. Recommended for 5+ extensions.
- Status: unclear (no label; "if included in your build")
- Evidence: https://goose-docs.ai/docs/guides/managing-tools/code-mode.md
- Notes: release v1.56.0 says Boa replaced an earlier "PCTX" code mode. (verified in review: meta-tool names, Boa, isolation, 5+ extensions; the PCTX line is from release notes, not re-checked)

### Area 7. Memory and persistent instructions

### .goosehints and AGENTS.md
- What: goose reads `AGENTS.md` and `.goosehints` by default from global (`~/.config/goose/`) and local locations; nested files from working directory up to the git root are loaded at start and subdirectory files when files there are touched, then stay active. `@path` includes a file's content; a plain path is a pointer. `CONTEXT_FILE_NAMES` (JSON array) changes the filenames (examples include `CLAUDE.md`, `.cursorrules`). Local beats global. Requires the Developer extension. Content goes into the system prompt each request.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/context-engineering/using-goosehints.md
- Notes: precedence of AGENTS.md versus .goosehints not stated.

### Persistent per-turn instructions (MOIM)
- What: `GOOSE_MOIM_MESSAGE_TEXT` and `GOOSE_MOIM_MESSAGE_FILE` are re-injected on every turn next to timestamp, cwd and todo list; read fresh each turn; capped at 64 KB. The page contrasts this with goosehints, which load at session start and can be forgotten.
- Status: GA (no label)
- Evidence: https://goose-docs.ai/docs/guides/context-engineering/using-persistent-instructions.md
- Notes: the "Top of Mind" platform extension is listed in the extensions page; the persistent-instructions page does not name it, so the link between them is inferred, unverified. (verified in review for the env vars, per-turn re-read, 64 KB cap; the Top of Mind link remains unverified)

### Memory extension
- What: built-in Memory extension "remembers your preferences over time".
- Status: unclear
- Evidence: https://goose-docs.ai/docs/getting-started/using-extensions.md
- Notes: no mechanism documented in pages I opened.

### Prompt template override
- What: system prompt and other templates (`system.md`, `compaction.md`, `subagent_system.md`) can be overridden by same-named files in `~/.config/goose/prompts/` (or Settings, Prompts in desktop); Jinja2; applies to new sessions.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/context-engineering/prompt-templates.md
- Notes: none.

### Area 8. Planning, tasks and modes

### Plan mode / planner
- What: nothing documented as a distinct plan mode. Chat Only mode is analysis without actions; the Todo extension tracks tasks. The multi-model page names turn-based and context-aware model selection but documents no planner configuration.
- Status: unclear (nothing documented)
- Evidence: https://goose-docs.ai/docs/guides/managing-tools/goose-permissions.md; https://goose-docs.ai/docs/guides/multi-model/index.md
- Notes: multi-model page says it is only a high-level overview.

### Auto thinking level
- What: v1.56.0 release notes list an automatic thinking level; Gemini 3 has Low/High thinking levels; Codex CLI provider has `CODEX_REASONING_EFFORT`.
- Status: unclear
- Evidence: https://github.com/aaif-goose/goose/releases (v1.56.0); https://goose-docs.ai/docs/getting-started/providers.md (Notes); https://goose-docs.ai/docs/guides/cli-providers.md
- Notes: release-note line only; mechanism unverified.

### Area 9. Sub-agents and multi-agent orchestration

### Subagents via `delegate` (Summon extension)
- What: goose spawns isolated subagents on request or on its own judgement (autonomous mode only). Sequential by default; parallel when prompt says so. Max 5 concurrent per parent (`GOOSE_MAX_CONCURRENT_SUBAGENTS`); default timeout 5 minutes; max turns default 1000 with precedence: `delegate` arg, child recipe settings, `GOOSE_SUBAGENT_MAX_TURNS`, `GOOSE_MAX_TURNS`. Subagents inherit parent extensions by default, can be restricted; cannot spawn subagents, change extensions or manage schedules. Return mode full or summary. Failed/timed-out subagents return nothing. `GOOSE_SUBAGENT_PROVIDER`/`_MODEL` select a different model. Tool calls shown inline with `[subagent:ID]`.
- Status: GA (no label)
- Evidence: https://goose-docs.ai/docs/guides/context-engineering/subagents.md; https://goose-docs.ai/docs/guides/recipes/recipe-reference.md (Settings)
- Notes: release v1.56.0 adds subagent resume and the concurrency variable; v1.59.0 resumes completed delegates; v1.55.0 removed "background subagents and orchestrator" (releases page). Hooks `SubagentStart/Stop` are not emitted (hooks page). (verified in review: concurrency 5, 5-minute timeout, no recursion, autonomous-only)

### External subagents
- What: external agents (example given: Codex run as an MCP server) can be configured under `subagent` in the config file.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/context-engineering/subagents.md
- Notes: one line only; config shape not shown. (verified in review: the page shows Codex run as an MCP server with a stdio config; the exact `subagent` config key was not confirmed)

### Sub-recipes
- What: a recipe lists `sub_recipes` (name, path, optional `values`, `sequential_when_repeated`); goose generates one tool per sub-recipe; each runs in its own session with no shared history/state; nesting not allowed; `values` override context-extracted params. Recipes with sub_recipes get `summon` automatically.
- Status: experimental (page says so)
- Evidence: https://goose-docs.ai/docs/guides/recipes/subrecipes.md; https://goose-docs.ai/docs/guides/recipes/recipe-reference.md
- Notes: CLI `goose run --sub-recipe` flag exists (CLI reference). (verified in review: experimental warning, no nesting, isolated sessions)

### Custom agents (markdown agent definitions)
- What: markdown files with frontmatter (`name` required, `description`, `model`) in `~/.agents/agents/` or `<project>/.agents/agents/`; also read from `.goose/agents/`, `.claude/agents/` and home equivalents. Invoked by `@name`, by delegation (separate session) or by load (instructions added to current conversation). Cannot define their own server list; cannot be scheduled (use a recipe).
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/context-engineering/custom-agents.md
- Notes: page says listing works "only in some setups".

### Area 10. Background, long-running and scheduled work

### Scheduler (cron for recipes)
- What: `goose schedule add|list|remove|sessions|run-now|cron-help` runs a recipe on a cron expression; desktop has a Scheduler view with background or foreground execution, run now, edit, pause. `goose serve --enable-scheduler` and `goose acp --enable-scheduler` turn the scheduler on in server mode. v1.57.0 notes say scheduled runs now use recipe settings.
- Status: unclear (no label)
- Evidence: https://goose-docs.ai/docs/guides/goose-cli-commands.md (goose schedule, serve, acp); https://goose-docs.ai/docs/guides/recipes/session-recipes.md (Scheduling); https://github.com/aaif-goose/goose/releases (v1.57.0)
- Notes: running-tasks page does not cover scheduling. Subagents cannot manage scheduled tasks.

### Headless runs
- What: `goose run -t`/`-i`/`--recipe`, `--no-session`, `--output-format text|json|stream-json`, `-q`, `--provider`, `--model`. Context overflow in headless mode summarises automatically.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/running-tasks.md; https://goose-docs.ai/docs/guides/goose-cli-commands.md
- Notes: `GOOSE_DISABLE_SESSION_NAMING` is suggested for CI.

### Recipe retry with shell success checks
- What: recipe `retry` block: `max_retries`, `checks` (shell commands that must exit 0), `timeout_seconds` default 300, `on_failure` shell command with timeout default 600. After a failed check goose runs `on_failure`, resets message history and restarts.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/recipes/recipe-reference.md (Retry and Success Checks)
- Notes: unusual: history reset between attempts. (verified in review: fields, 300/600 defaults, reset)

### Area 11. Sessions

### Session store and lifecycle
- What: SQLite `sessions.db` under `~/.local/share/goose/sessions/` (since 1.10.0), shared by desktop and CLI. Ids `YYYYMMDD_<COUNT>`. Auto-name from first prompt (disable with `GOOSE_DISABLE_SESSION_NAMING`). Resume (`goose session -r`), rename, list, remove, export (markdown/JSON/YAML; desktop JSON; v1.54.0 adds HTML export), import (desktop only), diagnostics report. Search across sessions limited to 10 messages in desktop; no regex.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/sessions/session-management.md; https://goose-docs.ai/docs/guides/goose-cli-commands.md; https://github.com/aaif-goose/goose/releases (v1.54.0)
- Notes: delete is permanent.

### Fork, duplicate, edit history
- What: CLI `--resume --fork` copies a session; `--edit` opens history as YAML in `$EDITOR`. Desktop message edit offers "Edit in Place" (deletes later messages) or "Fork Session" (new session up to the edited message).
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/sessions/in-session-actions.md; https://goose-docs.ai/docs/guides/sessions/session-management.md
- Notes: none.

### Checkpoints and file undo
- What: nothing documented. The file-management page only advises using Git.
- Status: unclear (nothing documented)
- Evidence: https://goose-docs.ai/docs/guides/file-management.md
- Notes: absence on that page only; I did not open every guide. (verified in review: file-management only advises Git; the docs map and CLI reference list no checkpoint, undo or rewind page or command; a web search found no goose checkpoint doc)

### Area 12. Git and version control

### `goose review`
- What: `goose review [range]` reviews a git diff (default working tree vs HEAD) with flags `--files`, `--check-filter`, `--dry-run`, `--severity`, `--no-orchestrate`.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/goose-cli-commands.md
- Notes: flag semantics not explained in the summary; `--no-orchestrate` implies an orchestration default (unverified).

### Worktrees, commits, PRs
- What: nothing documented in pages opened (no worktree or PR feature in the CLI reference). Recipes can be loaded from a GitHub repo via `GOOSE_RECIPE_GITHUB_REPO` using the `gh` CLI.
- Status: unclear (nothing documented)
- Evidence: https://goose-docs.ai/docs/guides/goose-cli-commands.md; https://goose-docs.ai/docs/guides/recipes/recipe-reference.md
- Notes: none.

### Area 13. Extensibility

### Extensions as MCP servers (types and management)
- What: all extensions are MCP. Types: `builtin`, `platform`, `stdio`, `streamable_http` (SSE no longer supported). Config under `extensions:` in `config.yaml` with name, cmd, args, enabled, envs/env_keys, timeout, `available_tools` (per-extension tool allowlist). Toggle in desktop sidebar, `goose configure`, `/extension`, `/builtin`, session flags `--with-*`. Extension Manager lets the agent search for and enable extensions mid-task (CLI asks approval; session-only). Per-extension timeout. Built-in extensions are standalone MCP servers usable by other agents (`goose mcp <name>`).
- Status: GA
- Evidence: https://goose-docs.ai/docs/getting-started/using-extensions.md; https://goose-docs.ai/docs/guides/config-files.md; https://goose-docs.ai/docs/guides/recipes/recipe-reference.md (Extensions)
- Notes: `--container` runs extensions in Docker.

### Remote MCP OAuth
- What: auto client registration via Client ID Metadata Documents or Dynamic Client Registration; pre-registered `client_id`/`client_secret_key` and `scopes` configurable; callback on 127.0.0.1 ephemeral port (`GOOSE_OAUTH_CALLBACK_PORT`).
- Status: GA (no label)
- Evidence: https://goose-docs.ai/docs/getting-started/using-extensions.md (Remote OAuth Extensions)
- Notes: none.

### MCP protocol features: elicitation, roots, MCP Apps
- What: Elicitation (form mode) enabled by default, 5-minute timeout. Roots: one root per session = working directory, updated on change. MCP Apps: early support for the draft spec, UI rendered in desktop; Apps and Auto Visualiser extensions.
- Status: elicitation GA (default on); roots GA; MCP Apps preview (draft spec)
- Evidence: https://goose-docs.ai/docs/guides/mcp-elicitation.md; https://goose-docs.ai/docs/guides/mcp-roots.md; https://goose-docs.ai/docs/guides/interactive-chat/index.md
- Notes: MCP Apps page does not describe sandboxing. Release v1.53.0 notes removal of MCP sampling code (releases page, indirect).

### Skills
- What: Agent Skills standard: directory with `SKILL.md` (name, description frontmatter) in `~/.agents/skills/`, `.agents/skills/`, plugin dirs, plus compatibility paths `.goose/skills/`, `.claude/skills/`. Names/descriptions added to instructions at session start; full text loaded when a request matches or by name; `goose skills list`, `/skills`. Built-in `web-search` skill.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/context-engineering/using-skills.md
- Notes: none.

### Plugins (skills plus hooks bundles)
- What: plugin directory with manifest (Open Plugins format or Gemini extension format) containing skills and/or `hooks/hooks.json`; `goose plugin install <git-url> [--auto-update]`, `goose plugin update`; user and project scope; disable via `disabledPlugins` in `settings.json`.
- Status: unclear (no label)
- Evidence: https://goose-docs.ai/docs/guides/context-engineering/plugins.md; https://goose-docs.ai/docs/guides/goose-cli-commands.md
- Notes: accepts Gemini CLI extension manifests; unusual interoperability.

### Hooks
- What: shell commands on events: SessionStart/End, Stop, UserPromptSubmit, PreToolUse, PreToolUseResult (observe-only), PostToolUse, PostToolUseFailure, BeforeReadFile, AfterFileEdit, BeforeShellExecution, AfterShellExecution. Regex matchers; JSON payload on stdin; only PreToolUse (exit 2 or JSON block) and Stop can block; hook failure allows by default, `on_failure: block` flips for PreToolUse; stop-block cap `GOOSE_STOP_HOOK_BLOCK_CAP`; default timeout 30s.
- Status: unclear (no label); SubagentStart/Stop not emitted
- Evidence: https://goose-docs.ai/docs/guides/context-engineering/hooks.md
- Notes: follows the Open Plugins hooks spec per page. (verified in review: event list, only PreToolUse/Stop block, fail-allow default, 30 s timeout)

### Custom slash commands
- What: map a command to a recipe (desktop Recipes menu or `slash_commands` list in config); one parameter; names unique, case-insensitive, cannot clash with built-ins.
- Status: GA (no label)
- Evidence: https://goose-docs.ai/docs/guides/context-engineering/slash-commands.md
- Notes: none.

### Tool shim
- What: for models without native tool calling, a separate interpreter model (default Ollama `mistral-nemo`, or a local backend) converts text tool calls to structured calls; `GOOSE_TOOLSHIM=true`.
- Status: experimental
- Evidence: https://goose-docs.ai/docs/guides/tool-shim.md
- Notes: none.

### GDK (build your own agent) and Rust crates
- What: reusable components: `goose-agent` (state machine, operations, tools), `goose-provider-types` (Provider trait), `goose-providers`, `goose-context-management`, `goose-local-inference`, `goose-download-manager`, `goose-sdk-types`, `goose-sdk`; Python and Kotlin bindings for provider calls. Hello-world is a Rust agent that runs shell commands, streams and saves its conversation. Runs on wasm32 per release notes. Demos include custom UIs.
- Status: unclear (no maturity label)
- Evidence: https://goose-docs.ai/docs/gdk/index.md; https://goose-docs.ai/llms.txt; https://github.com/aaif-goose/goose/releases (v1.53.0)
- Notes: docs.rs pages linked, not opened.

### Custom distributions
- What: goose is "designed to be forked"; options from low (env defaults, `config.yaml`, `init-config.yaml` first-run settings, declarative JSON providers, edited system prompts, recipes) to medium (bundled MCP extensions, desktop branding) to high (new UI on ACP). Guide is `CUSTOM_DISTROS.md` in the repo; must follow Apache-2.0.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/custom-distributions.md; https://github.com/aaif-goose/goose (README)
- Notes: CUSTOM_DISTROS.md itself not opened. `init-config.yaml` is not covered by the config-files page.

### Agent Client Protocol (server side)
- What: goose is an ACP agent: `goose acp` (stdio subprocess) and `goose serve` (HTTP at `/acp` plus WebSocket, default port 3284). Flow: initialize, create session with absolute cwd, prompt with streamed `session/update`, permission requests, `session/load` to resume, `session/cancel`. goose Desktop talks to its backend over ACP. Goose-specific ACP methods are in a generated reference. Listed in Zed's ACP registry. v1.55.0 passes W3C trace context from ACP prompts to MCP tool calls; v1.59.0 filters sessions by ACP client tags.
- Status: GA (no label)
- Evidence: https://goose-docs.ai/docs/gdk/acp/index.md; https://goose-docs.ai/docs/guides/zed.md; https://goose-docs.ai/docs/guides/goose-cli-commands.md; https://github.com/aaif-goose/goose/releases
- Notes: the permission callback "is not a sandbox" per the ACP page. The GDK ACP page names goose Desktop, Top Goose (`goose acp`) and Berd (`goose serve` over WebSocket) as users, and does not mention Zed; Zed use (via `goose acp`, installed through Zed's ACP registry) is on https://goose-docs.ai/docs/guides/zed.md. `goose serve` can authenticate with a `token` query parameter for browser WebSockets. (verified in review)

### Area 14. Models, providers and inference

### Provider catalogue
- What: direct APIs (Anthropic, OpenAI, Google Gemini, Groq, Mistral, xAI, Cerebras, and others), cloud platforms (Bedrock, SageMaker TGI, Azure AI Foundry and Azure OpenAI, Vertex AI, Snowflake, Tanzu), gateways (LiteLLM, OpenRouter, Tetrate and others), subscription providers (ChatGPT Codex, GitHub Copilot, Z.AI Coding Plan), local (Ollama, LM Studio, Docker Model Runner, Lemonade, Ramalama, Atomic Chat), CLI and ACP providers. Auth: API key, cloud credentials, browser PKCE sign-in, device flow, refreshable commands. Custom providers via JSON in OpenAI/Anthropic/Ollama formats with headers, model lists, per-model prices. Claude prompt caching is automatic through several routes.
- Status: GA
- Evidence: https://goose-docs.ai/docs/getting-started/providers.md
- Notes: README says 15+ providers; docs list well over 40, so the README figure is stale. New providers keep landing (v1.58.0: AIHubMix, RouterPlex, Lemonade).

### ACP providers (use other agents as the model backend)
- What: `amp-acp`, `claude-acp`, `codex-acp`, `pi-acp` use an existing subscription via the vendor's ACP adapter. They replace the deprecated CLI providers. goose modes map onto the agent's modes (e.g. goose `auto` skips Claude permission checks; `approve` maps to read-only for Codex). Extensions added with `--with-extension` are passed to the agent as MCP servers. Limits: `goose session resume`/`fork` unsupported; ACP session id differs from goose's.
- Status: GA (no label); resume/fork unsupported
- Evidence: https://goose-docs.ai/docs/guides/acp-providers.md; https://goose-docs.ai/docs/getting-started/providers.md
- Notes: the same page's troubleshooting mentions a `copilot` binary, but Copilot is not in the supported list. Docs inconsistency. (verified in review: the page lists four providers and the mode mappings; the troubleshooting binary is named `claude-agent-acp`, not `claude-acp`)

### CLI providers (deprecated except Cursor Agent)
- What: wrap other CLIs (`claude-code`, `codex`, `cursor-agent`, `gemini-cli`) as a provider; they use their own tools and do not get goose extensions (goose strips extension info from the system prompt). `claude-code`, `codex`, `gemini-cli`, `gemini_oauth` are deprecated; Cursor Agent is not marked so. In approve modes, Claude Code permission requests route through goose's confirmation UI.
- Status: deprecated (three of four); Cursor Agent unclear
- Evidence: https://goose-docs.ai/docs/guides/cli-providers.md; https://goose-docs.ai/docs/getting-started/providers.md
- Notes: the page admits its own Claude model-default docs are inconsistent. (verified in review: provider list and deprecations)

### Local models
- What: `goose local-models` (alias `lm`) searches, downloads, lists and deletes models (`user/repo:quant`); in-process inference via llama.cpp (GGUF) with optional MLX on Apple silicon in the GDK `goose-local-inference` crate.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/goose-cli-commands.md; https://goose-docs.ai/docs/gdk/concepts/providers.md
- Notes: how the desktop Local provider relates to these is not stated.

### Multi-model
- What: `/model` switches mid-session in CLI (the in-session page says CLI cannot change model via a setting); per-subagent provider/model; recipe `settings.goose_provider`/`goose_model`/`temperature`/`max_turns`. No lead/worker or fallback documented.
- Status: GA for per-subagent; the rest nothing documented
- Evidence: https://goose-docs.ai/docs/guides/multi-model/index.md; https://goose-docs.ai/docs/guides/goose-cli-commands.md; https://goose-docs.ai/docs/guides/sessions/in-session-actions.md
- Notes: the in-session page says the CLI does "not support" a model change while the CLI reference lists `/model`; docs disagree.

### Rate-limit handling
- What: docs recommend routing via Tetrate or OpenRouter for failover; Bedrock and Databricks have retry env vars. No generic retry or fallback documented.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/handling-llm-rate-limits-with-goose.md; https://goose-docs.ai/docs/guides/environment-variables.md
- Notes: none.

### Area 15. UI surfaces

### Desktop, CLI, IDE-via-ACP, voice
- What: desktop app (installer for Windows NSIS since v1.56.0; quick launcher shortcut; recipe library; scheduler view; sidebar customization page); CLI; Zed integration through ACP; desktop voice dictation (local provider, or ElevenLabs/Groq/OpenAI); v1.59.0 mentions live-voice instructions. No web UI or mobile documented in pages opened.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/sessions/session-management.md; https://goose-docs.ai/docs/guides/sessions/in-session-actions.md; https://goose-docs.ai/docs/guides/zed.md; https://github.com/aaif-goose/goose/releases
- Notes: a browser client exists for the experimental roaming transport (see area 17).

### Area 16. Browser automation and computer use

### Computer Controller and browser skill
- What: Computer Controller built-in extension (web scraping, file caching, automation; adversary-mode docs name `computercontroller__computer_control` for macOS UI automation). Browser automation is via installing the upstream browser-use skill.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/getting-started/using-extensions.md; https://goose-docs.ai/docs/guides/security/adversary-mode.md; https://goose-docs.ai/docs/guides/context-engineering/using-skills.md
- Notes: no dedicated page.

### Area 17. Remote, cloud, headless, CI, distributed execution

### Remote `goose serve`
- What: run `goose serve --host 0.0.0.0 --port N --tls` on a VM and point desktop at it (Settings, goose Server). Auth: shared secret in `GOOSE_SERVER__SECRET_KEY`, sent to `/acp` (`X-Secret-Key`); `--dangerously-unauthenticated` for local only. TLS with self-signed cert, startup prints `GOOSED_CERT_FINGERPRINT` for pinning, otherwise trust-on-first-use. Secret is stored in plaintext in the launch plist and desktop settings; rotation is manual. Cert paths via `GOOSE_TLS_CERT_PATH`/`KEY_PATH`.
- Status: unclear (no label)
- Evidence: https://goose-docs.ai/docs/guides/remote-goose-server.md; https://goose-docs.ai/docs/guides/environment-variables.md (TLS); https://goose-docs.ai/docs/guides/goose-cli-commands.md (serve defaults to 127.0.0.1:3284)
- Notes: the remote guide's example port is 3000 while CLI default is 3284; consistent, but one says `goosed` in logs and the other `goose serve`. The remote guide itself states no default port; the secret lives in the launchd plist environment on the server and in the Desktop setting on the client. (verified in review)

### Roaming agents (peer-to-peer ACP transport)
- What: reach a running agent from another machine over iroh (QUIC) with no open ports. Each node has a long-lived identity and a shareable "connection card" (public key plus reachability); two machines each accept the other's key; trust is all-or-nothing full ACP access; revoke force-closes connections in seconds. Commands: `roam share|id|connect|delegate|bridge|peers|pair|connections`; `goose serve --roam`. `goose-roaming` crate is independent of agent internals; wasm bindings give a browser client. Relays are goose-managed unless `GOOSE_ROAM_RELAYS` is set.
- Status: experimental, opt-in; not in released binaries (build with `--features roaming`)
- Evidence: https://goose-docs.ai/docs/guides/roaming-agents.md
- Notes: known limits: spawned OS processes survive revocation; multiple clients cannot co-drive one live session; TCP bridge does not authenticate its local client (it needs `--allow-remote-clients`). (verified in review: page states not included in released binaries, feature `roaming`; the page does not say goose Desktop uses it)

### Area 18. Collaboration, teams, enterprise administration

### Admin controls
- What: extension allowlist (above), config files and env vars for defaults, custom distributions, VMware Tanzu Platform and Azure/Databricks/Snowflake/Bedrock providers. No team, shared-session or RBAC feature documented. Session sharing: v1.54.0 notes removal of Nostr session sharing.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/allowlist.md; https://goose-docs.ai/docs/guides/custom-distributions.md; https://github.com/aaif-goose/goose/releases (v1.54.0)
- Notes: none.

### Governance move (Block to AAIF at the Linux Foundation)
- What: the Linux Foundation announced the AAIF on 2025-12-09 with MCP, goose and AGENTS.md as founding project contributions; Block's representative is quoted on neutral, community-driven stewardship. GOVERNANCE.md describes contributors, maintainers and 3 to 7 core maintainers, a one-week review for major changes, and creator Bradley Axen as deadlock tie-breaker; goose is a Series of LF Projects, LLC.
- Status: GA (fact)
- Evidence: https://aaif.io/press/linux-foundation-announces-the-formation-of-the-agentic-ai-foundation-aaif-anchored-by-new-project-contributions-including-model-context-protocol-mcp-goose-and-agents-md/ (announcement); https://github.com/aaif-goose/goose/blob/main/GOVERNANCE.md (Governance); https://github.com/aaif-goose/goose (README)
- Notes: a third-party search snippet puts the repo move to `aaif-goose/goose`; the README at that URL confirms it is the canonical repo. Block's platinum membership is listed on the primary announcement page. (verified in review; corrected in review: earlier text called it third-party only)

### Area 19. Observability

### OpenTelemetry export
- What: setting `OTEL_EXPORTER_OTLP_ENDPOINT` enables export; per-signal endpoints and exporters (`otlp`, `console`, `none`) for traces, metrics and logs; `OTEL_SDK_DISABLED`; `OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT` (default false) adds messages and tool args/results to traces; standard sampler variables. Config file keys `otel_exporter_otlp_endpoint`/`_timeout`. W3C trace context from ACP prompts reaches MCP tool calls (v1.55.0).
- Status: unclear (no label)
- Evidence: https://goose-docs.ai/docs/guides/environment-variables.md (OpenTelemetry); https://goose-docs.ai/docs/guides/config-files.md; https://github.com/aaif-goose/goose/releases (v1.55.0)
- Notes: the logs page does not mention OTel.

### Langfuse
- What: `LANGFUSE_PUBLIC_KEY`, `LANGFUSE_SECRET_KEY`, optional `LANGFUSE_URL`.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/environment-variables.md (Langfuse)
- Notes: no dedicated page; what is sent is not described.

### Local logs
- What: CLI logs, server (`goosed`) logs with JSON-RPC traffic, LLM request logs (raw request/response, last 10 kept as `llm_request.0..9.jsonl`), desktop log; date-based CLI/server logs deleted after two weeks; stored locally. `goose info -v`, `goose session diagnostics`, `--debug`.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/logs.md; https://goose-docs.ai/docs/guides/goose-cli-commands.md
- Notes: LLM request logs hold raw prompts, so they are sensitive on shared hosts (inference from the page).

### Usage and cost
- What: token usage indicators (desktop dot, CLI colours) and `/status` (session id, model, provider, mode, tokens). Per-model prices can be set on custom providers. No cost dashboard documented.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/sessions/smart-context-management.md; https://goose-docs.ai/docs/guides/goose-cli-commands.md; https://goose-docs.ai/docs/getting-started/providers.md
- Notes: none.

### Area 20. Configuration and customisation

### Config files and precedence
- What: `~/.config/goose/config.yaml` (YAML; `GOOSE_*` keys at root, `extensions:` block, `providers`); also `permission.yaml`, `secrets.yaml`, `permissions/tool_permissions.json`, `prompts/`, `adversary.md`, `settings.json` (plugin disable). Precedence: env vars, then config file, then defaults. Project-level `<project>/.config/goose/settings.json`. Edits usually need restart. `goose configure` is the interactive editor.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/config-files.md; https://goose-docs.ai/docs/guides/context-engineering/plugins.md
- Notes: `init-config.yaml` and custom providers dir not covered by that page.

### Recipes
- What: YAML/JSON recipes: `title`, `description`, `instructions` and/or `prompt`; typed `parameters` (string, number, boolean, date, file, select; required/optional/user_prompt) with Jinja templating and template inheritance; `extensions` (with `available_tools`); `settings` (provider, model, temperature, max_turns); `response.json_schema` to validate final output; `retry`; `sub_recipes`; desktop-only `activities`. Loaded from cwd, `GOOSE_RECIPE_PATH`, or a GitHub repo. `goose recipe validate|deeplink|list|open`; `goose run --recipe --params`. Desktop asks to trust a recipe on first run; shareable deeplinks.
- Status: GA (sub-recipes experimental)
- Evidence: https://goose-docs.ai/docs/guides/recipes/recipe-reference.md; https://goose-docs.ai/docs/guides/recipes/session-recipes.md; https://goose-docs.ai/docs/guides/goose-cli-commands.md
- Notes: `.yml` is not supported by the CLI per the page.

### Area 21. Security and privacy

### Telemetry
- What: opt-in anonymous usage data (OS, version, install method, provider/model, extension and tool names and counts, session duration, token usage, error types); no conversations, code, tool arguments or error messages. `GOOSE_TELEMETRY_ENABLED` default false per the env-var page.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/usage-data.md; https://goose-docs.ai/docs/guides/environment-variables.md
- Notes: the telemetry page does not mention OTel; those settings are separate.

### Security guidance
- What: the "Staying Safe" page points to adversary mode, prompt injection detection, the classifier spec, and blog posts about MCP server safety. It does not cover sandboxing or secrets.
- Status: GA
- Evidence: https://goose-docs.ai/docs/guides/security/index.md
- Notes: see areas 5 and 13 for the controls themselves.

### Area 22. Licence, pricing, distribution

### Licence and distribution
- What: Apache-2.0; free; desktop installers and CLI install script (`download_cli.sh`, `download_cli.ps1`) from GitHub releases ("stable" tag); `goose update [--canary]`; offline/air-gapped docs page exists. Governed by AAIF.
- Status: GA
- Evidence: https://github.com/aaif-goose/goose; https://goose-docs.ai/docs/guides/goose-cli-commands.md
- Notes: pricing not applicable beyond providers' own costs (not documented on pages opened).

### Area 23. Anything else

### Ask-AI docs bot and `goose-doc-guide`
- What: `GOOSE_DOCS_ROOT` configures a `goose-doc-guide` documentation root (default goose-docs.ai); repo has an `ask-ai-bot` service directory.
- Status: unclear
- Evidence: https://goose-docs.ai/docs/guides/config-files.md; https://github.com/aaif-goose/goose (file listing)
- Notes: minimal documentation.

## 3. Distinctive or unusual

- Adversary mode: an LLM reviewer of each shell call, rules in a plain-language file; fails open. Interesting as a cheap second opinion without a second runtime.
- Roaming: iroh/QUIC peer-to-peer ACP transport with key-exchange trust, revocation, and a wasm browser peer. Interesting for NAT-free node connection.
- Whole product is an ACP server: desktop, Zed and custom UIs all use `goose acp`/`goose serve`. One agent core, many frontends.
- ACP providers: goose can drive Claude/Codex/Pi/Amp through ACP while mapping its own permission modes onto theirs.
- Code Mode: JS meta-tools (list, describe, execute) in an embedded Boa engine to cut tool-schema context.
- Recipe `retry` with shell success checks and history reset; `response.json_schema` validation.
- Hooks and plugins follow the "Open Plugins" spec and accept Gemini extension manifests; skills and agents read `.claude/` paths for compatibility.
- GDK exposes the loop as a resumable state machine where the application owns state.
- Extension allowlist with exact command match, delivered by a URL the admin hosts.
- Instruction-file name list is configurable (`CONTEXT_FILE_NAMES`), so goose can read `CLAUDE.md` or `.cursorrules`.

## 4. Architecture and UX notes

- Single Rust binary `goose` with subcommands; release v1.53.0 describes a "lean ACP-only" binary (PR not read). Desktop starts its own `goose serve` (ACP over HTTP/WebSocket) and can be pointed at a remote one (remote-server page). Docs still use the name `goosed` in log paths and deeplink commands.
- The agent loop and tools run inside the `goose serve`/`goose acp` process on the machine with the code; the client (desktop, editor, custom app) sends prompts and answers permission requests over ACP.
- Session state is a local SQLite file shared by CLI and desktop.
- Extensions are separate MCP processes (stdio) or remote URLs, with a timeout per extension; platform extensions run in-process as "global features" (inference from the description; mechanism not documented).
- Repo layout per the README listing: Rust workspace `crates/`, a `ui/` directory, services such as `ask-ai-bot` and `oidc-proxy`.

## 5. Relevance to a self-hosted master/node harness

- Remote `goose serve` with a shared secret, TLS fingerprint pinning and TOFU (Remote `goose serve` entry) is the closest analogue of the node link; note its weak points (plaintext secret, manual rotation) as what not to copy.
- Roaming's key-card trust model, revoke-closes-connections and "spawned processes survive revoke" limit are directly relevant to node pairing and revocation.
- ACP as the single client protocol (ACP entry) shows a way for a web dashboard and editors to share one agent core; compare against the owner's own event log.
- The extension allowlist and `available_tools` per extension (extensions entries) map onto a node-side policy that the master cannot widen.
- Per-tool permission levels plus modes (Permissions entries) are a baseline; goose's approval classification via the LLM provider is explicitly best-effort, a weakness to avoid.
- No sandbox is documented for the CLI or in the default autonomous mode (Sandboxing entry; a macOS-Desktop-only opt-in `GOOSE_SANDBOX` is reported by search snippets but unverified), a gap a node-side policy can beat.
- Adversary mode and prompt-injection detection with pluggable classifier endpoint (Security entries) show two layered defences; the classifier sends content to a configurable URL, which is a data-egress consideration.
- Code Mode and tool deferral (area 6) address tool-schema bloat when many MCP servers are attached.
- Custom-agent files that read `.claude/` and `.agents/` paths (Custom agents, Skills) show the value of interoperating with other harnesses' instruction layouts.
- Optional OTel with a message-content switch default off (Observability) is a sound privacy default to copy.

## 6. Overhyped, stale or contradictory

- README says 15+ providers; providers page lists many more.
- Docs name the server `goosed` in several places (logs, deeplink commands) while the remote-server guide and CLI reference use `goose serve`; v1.53.0 notes a lean ACP-only binary.
- Multi-model page promises "dynamic, context-aware switching" strategies but documents only `/model` and subagent models.
- Model switching: in-session actions page says the CLI does not support it as a setting; the CLI reference lists `/model`. Likely a persistence distinction, but unclear.
- ACP providers page mentions a `copilot` binary not in its supported list.
- Prompt injection page does not state the default; config pages say off.
- The subagents page says subagents are disabled outside autonomous mode, so the default-autonomous mode is the only way to get delegation, which conflicts with using approvals and subagents together.
- Hook page: `SubagentStart/Stop` listed in docs of events but "not emitted".
- Context relevance operation and context report (v1.58.0) have no doc page I found.
- Release notes in this survey mention newer models (for example Opus 5.5, GPT-6); I read these only as release-note text.

## 7. Unverified

- Developer extension tool list (names, edit mechanism): not in pages opened. Would be settled by the repository's extension docs or the quickstart "extension details" page.
- Malware check mechanism (data source, offline behaviour): one sentence in the extensions page. Settle with a security blog post or docs search.
- `GOOSE_TOOL_DEFERRAL` / "minimal deferred MCP tools" behaviour: config table only.
- Smart Approval risk criteria: not described.
- `init-config.yaml` format and `CUSTOM_DISTROS.md` content: file not opened.
- Which of README stars/figures (56.4k) is current: not re-checked.
- Remote/roaming claims about release inclusion: only from the roaming page.
- macOS Desktop sandbox (`GOOSE_SANDBOX`, Seatbelt, egress proxy, v1.25.0): seen only in search snippets of https://goose-docs.ai/docs/guides/sandbox and a third-party changelog mirror; both goose-docs.ai and block.github.io URLs returned 404 in review and the docs map does not list it. Would be settled by the page at a working URL or the v1.25.0 release notes.
- (Resolved in review) Block platinum membership: confirmed on the AAIF announcement.
- Whether "Top of Mind" extension equals the MOIM persistent instructions: inferred.
- goose CLI `project`, `web` and `bench` subcommands: absent from the CLI reference; unknown if removed.

## 8. Sources

All opened 2026-11-10 (docs are markdown twins at goose-docs.ai):

- https://goose-docs.ai/llms.txt (docs index)
- https://goose-docs.ai/goose-docs-map.md (full page map)
- https://github.com/aaif-goose/goose (README, governance, licence, surfaces)
- https://github.com/aaif-goose/goose/releases (v1.53.0 to v1.59.0 highlights)
- https://github.com/aaif-goose/goose/releases/tag/v1.53.0 (ACP-only binary line)
- https://github.com/aaif-goose/goose/blob/main/GOVERNANCE.md (governance model)
- https://aaif.io/ (AAIF landing; no goose mention on the page)
- https://aaif.io/press/linux-foundation-announces-the-formation-of-the-agentic-ai-foundation-aaif-anchored-by-new-project-contributions-including-model-context-protocol-mcp-goose-and-agents-md/ (2025-12-09 announcement)
- Web search results (third-party, used only to find the announcement): thenewstack.io, siliconangle.com, redmondmag.com
- https://goose-docs.ai/docs/quickstart.md
- https://goose-docs.ai/docs/getting-started/using-extensions.md
- https://goose-docs.ai/docs/getting-started/providers.md
- https://goose-docs.ai/docs/guides/managing-tools/goose-permissions.md
- https://goose-docs.ai/docs/guides/managing-tools/tool-permissions.md
- https://goose-docs.ai/docs/guides/managing-tools/index.md
- https://goose-docs.ai/docs/guides/managing-tools/code-mode.md
- https://goose-docs.ai/docs/guides/recipes/recipe-reference.md
- https://goose-docs.ai/docs/guides/recipes/subrecipes.md
- https://goose-docs.ai/docs/guides/recipes/session-recipes.md
- https://goose-docs.ai/docs/guides/context-engineering/subagents.md
- https://goose-docs.ai/docs/guides/context-engineering/custom-agents.md
- https://goose-docs.ai/docs/guides/context-engineering/using-goosehints.md
- https://goose-docs.ai/docs/guides/context-engineering/using-persistent-instructions.md
- https://goose-docs.ai/docs/guides/context-engineering/using-skills.md
- https://goose-docs.ai/docs/guides/context-engineering/hooks.md
- https://goose-docs.ai/docs/guides/context-engineering/plugins.md
- https://goose-docs.ai/docs/guides/context-engineering/slash-commands.md
- https://goose-docs.ai/docs/guides/context-engineering/prompt-templates.md
- https://goose-docs.ai/docs/guides/sessions/index.md
- https://goose-docs.ai/docs/guides/sessions/session-management.md
- https://goose-docs.ai/docs/guides/sessions/in-session-actions.md
- https://goose-docs.ai/docs/guides/sessions/smart-context-management.md
- https://goose-docs.ai/docs/guides/security/index.md
- https://goose-docs.ai/docs/guides/security/prompt-injection-detection.md
- https://goose-docs.ai/docs/guides/security/adversary-mode.md
- https://goose-docs.ai/docs/guides/security/classification-api-spec.md
- https://goose-docs.ai/docs/guides/allowlist.md
- https://goose-docs.ai/docs/guides/acp-providers.md
- https://goose-docs.ai/docs/guides/cli-providers.md
- https://goose-docs.ai/docs/guides/custom-distributions.md
- https://goose-docs.ai/docs/guides/remote-goose-server.md
- https://goose-docs.ai/docs/guides/roaming-agents.md
- https://goose-docs.ai/docs/guides/running-tasks.md
- https://goose-docs.ai/docs/guides/goose-cli-commands.md
- https://goose-docs.ai/docs/guides/usage-data.md
- https://goose-docs.ai/docs/guides/logs.md
- https://goose-docs.ai/docs/guides/terminal-integration.md
- https://goose-docs.ai/docs/guides/config-files.md
- https://goose-docs.ai/docs/guides/environment-variables.md
- https://goose-docs.ai/docs/guides/multi-model/index.md
- https://goose-docs.ai/docs/guides/tool-shim.md
- https://goose-docs.ai/docs/guides/codebase-analysis.md
- https://goose-docs.ai/docs/guides/mcp-elicitation.md
- https://goose-docs.ai/docs/guides/mcp-roots.md
- https://goose-docs.ai/docs/guides/interactive-chat/index.md
- https://goose-docs.ai/docs/guides/file-management.md
- https://goose-docs.ai/docs/guides/handling-llm-rate-limits-with-goose.md
- https://goose-docs.ai/docs/guides/zed.md
- https://goose-docs.ai/docs/gdk/index.md
- https://goose-docs.ai/docs/gdk/acp/index.md
- https://goose-docs.ai/docs/gdk/concepts/state-machine.md
- https://goose-docs.ai/docs/gdk/concepts/providers.md

## 9. Review

Date: 2026-11-10. Independent review against the cited public pages. Entries checked: 29 of about 75 feature entries (all Section 3 items and the Section 5 entries they rest on, plus the entries on permissions, sandboxing, security, remote execution, sub-agents and numbers).

| Entry | Verdict | Reason |
|---|---|---|
| Governance move | confirmed-with-correction | AAIF announcement 2025-12-09, GOVERNANCE.md details all match; Block platinum IS on the primary announcement (survey called it third-party only) |
| `goose acp` / `goose serve` and users | confirmed-with-correction | stdio vs HTTP/WS, port 3284 correct; GDK ACP page names Desktop, Top Goose, Berd and does not mention Zed (Zed is on zed.md); token query param added |
| ACP providers | confirmed | four providers, mode mappings, no resume/fork all match; adapter binary is `claude-agent-acp` |
| Remote `goose serve` | confirmed | X-Secret-Key, env var, fingerprint log line, TOFU, plist storage, manual rotation all stated; no default port on that page |
| Roaming | confirmed | experimental, not in released binaries, `--features roaming`, trust and revoke text match |
| Adversary mode | confirmed | page says reviewer failure allows the call (fail open); default `shell`; file existence enables |
| Prompt injection detection | confirmed | 0.8, 0.01-1.0, patterns, endpoint; default off confirmed via env-var page, not detection page |
| Recipes retry / sub-recipes | confirmed | defaults 300/600, history reset; sub-recipes experimental, no nesting |
| Subagents / external subagents | confirmed | 5 concurrent, 5 min, autonomous-only, no recursion; Codex-as-MCP example present |
| Permission modes / per-tool | confirmed | four modes, default autonomous, best-effort classification, permission files |
| Sandboxing | confirmed-with-correction | "nothing documented" not supported: search results show a macOS Desktop sandbox page (`GOOSE_SANDBOX`, Seatbelt); page 404s on fetch, so unverified; env-var page claim about sandbox variables unsupported |
| Checkpoints | confirmed | no checkpoint/undo page in docs map or CLI reference; file-management advises Git |
| Tool output size cap | confirmed-with-correction | unit is characters |
| Tool deferral | confirmed-with-correction | mechanism is stated on config-files page |
| Turn limit, auto-compaction, Code Mode, MOIM, hooks, allowlist, malware check, deeplinks, secrets, CLI providers, built-in extensions | confirmed | numbers, names and defaults match |

Borrowed features: none found.

Notable omissions (docs map at https://goose-docs.ai/docs/goose-docs-map.md):
1. macOS Desktop sandbox (`GOOSE_SANDBOX`), see Sandboxing entry (page unreachable now).
2. MCP Apps page https://goose-docs.ai/docs/guides/interactive-chat/mcp-ui.md (survey cites only the index).
3. "Extensions in Containers" section of using-extensions (survey mentions `--container` only in passing).
4. `goose review` flags are broader than the survey lists (`--check-scope`, `--checks-only`, `--summary-only`, `--turn-limit`, `--override-model`, `--instructions`); `--severity` defaults to medium (https://goose-docs.ai/docs/guides/goose-cli-commands.md).
5. `goose serve` enables the `developer` extension when `--with-builtin` is omitted, and requires the secret unless `--dangerously-unauthenticated`.

Trust judgement: the file is generally reliable. Numbers, defaults and statuses I checked were right apart from the sandbox "nothing documented" claim (the one significant error, and it matters for the reader's security analysis) and two small unit/mechanism gaps. The summarising-fetcher wording is paraphrase; unchecked remainder (providers, UI, telemetry, logs, sessions, area 15 to 23) can be trusted at roughly the same level, but treat every "nothing documented" claim as needing a second look, since this one failed.
