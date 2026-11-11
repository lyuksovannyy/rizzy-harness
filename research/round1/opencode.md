# OpenCode: round 1 public-surface survey

Surveyed 2026-11-10. Public surface only (docs, README, changelog, releases page). No source code read. Fetches were done through a summarising fetch tool, so details are as reported by that tool from each page; where a page was truncated this is noted.

## 1. Identity

- Name: OpenCode. Maintained under the `anomalyco` GitHub organisation (formerly `sst`); the site footer reads "(c) 2026 Anomaly" (https://opencode.ai/product). Company relationship to "sst" not verified.
- Surfaces: terminal UI/CLI, desktop app (Electron per third-party report; docs say beta in README), web app served by the same server, mobile (iPhone via TestFlight, Android "coming soon"), SDK, ACP mode, HTTP API, plugins for server/TUI/GUI.
- Open source, MIT (README says MIT; https://github.com/anomalyco/opencode). About 225.9k stars, default branch `dev` (README page; branch from 00-targets).
- **Which version line is current: V2.** The docs at `/docs` describe V2 and the V1 docs banner states the latest docs are V2 (the changelog itself only lists v2.x above v1.x and never says "current"; corrected in review); the newest releases are `v2.2.7` (10 Nov 2026), `v2.2.6`, ... `v2.2.0` (27 Oct 2026). The V1 docs are still served under `/v1/docs/` and say "latest docs are V2". The GitHub "latest" flag points to `v1.19.5` (28 Oct 2026, per 00-targets; the changelog also lists v1.19.5 and v1.19.4 as V1 maintenance releases, bun runtime bump and a TUI worker error fix). So V1 receives small maintenance releases in parallel. I could not verify the flag myself (the releases page I fetched did not show badges). The releases list also contains `process-daemon v0.x` tags (a separate component; not described in docs I read).
- Confirms 00-targets: correct. Note one correction to 00-targets: docs say `@opencode/client` / `@opencode/sdk` / `@opencode/plugin` are the V2 packages; the README (which may be stale) still lists npm `@opencode/cli`.

### What changed from V1 to V2 (from the migration guide)
Source: https://opencode.ai/docs/migrate-v1/ and https://opencode.ai/docs/build/plugins/migrate-v1/
- Three declared breaking changes: new plugin API, new server API and client contracts, new terminal client config file (`~/.config/opencode/cli.json`, replacing layered `tui.json`).
- Config: grouped per-tool `permission` map becomes an ordered `permissions` array of `{action, resource, effect}`; `tools` map becomes permission rules; `agent`/`mode` become `agents`; MCP, compaction, skills, commands, providers, models all re-shaped. V2 reads the same file locations and normalises in memory without rewriting.
- V2 does NOT run language servers, expose LSP tools or produce diagnostics (LSP config kept but unused). Docs recommend lint/typecheck commands instead.
- `external_directory` became allow-by-default in V2 (V1 prompted).
- `CLAUDE.md` fallback is not supported; only `AGENTS.md`.
- V1 plugins do not run in V2.
- Server auth: `OPENCODE_SERVER_USERNAME` removed; username always `opencode`.
- Todos are off by default (`experimental.todo`).
- The V2 index has no GitHub page; the GitHub Action "is no longer published" (changelog v2.2.0).
- Architecture change (third-party, https://thenewstack.io/opencode-2-shared-service/, via search snippet only, not opened): one shared background service for terminal, web and desktop.

## 2. Features by area

### 1. Agent loop and execution model

### Client/server split with shared background service
- What: A background server owns sessions, plugins, permissions and application state; TUI, web and desktop are clients. The CLI discovers or auto-starts a shared service; `--standalone` uses a private server instead; `opencode serve` runs a foreground server; `opencode service start|stop|restart|status|get|set|unset` manages the shared one. Registration lives in `~/.local/state/opencode/service.json`.
- Status: GA (V2)
- Evidence: https://opencode.ai/docs/troubleshooting/ (Troubleshooting); https://opencode.ai/docs/cli/ (CLI intro); https://opencode.ai/docs/cli/commands/
- Notes: V1 differed: running `opencode` started a TUI plus its own random-port server, and `opencode serve` started a separate one (https://opencode.ai/v1/docs/server/). (verified in review: service.json path, `--standalone` on the CLI page, service subcommands on the commands page)

### Agent step limit with forced summary
- What: Per-agent `steps` caps model steps; on the final step tools are removed and the model must answer in text.
- Status: GA
- Evidence: https://opencode.ai/docs/agents/ (Per-agent options)
- Notes: (verified in review)

### Mid-run steering and queued prompts
- What: In the TUI, Enter while the agent is working steers the active session; Ctrl+X then Enter queues a prompt. The API has an inbox of queued inputs that can be reprioritised or cancelled.
- Status: GA
- Evidence: https://opencode.ai/docs/cli/tui/ ; https://opencode.ai/docs/api/ (Session endpoints, Inbox)

### `/goal` objective loop
- What: `/goal <objective>` keeps the session working until a reviewer confirms the objective is met; `pause`, `resume`, `clear` subcommands.
- Status: unclear (appears in changelog only; no docs page found)
- Evidence: https://opencode.ai/changelog (v2.2.1)
- Notes: how the "reviewer" works is not documented in what I read.

### Cache warming
- What: Optional keep-alive requests that refresh a provider's prompt cache during pauses (at 90% of cache TTL, or every four minutes; stops 30 minutes after the last real request; not for subagent sessions; responses discarded). Costs real tokens.
- Status: GA, off by default
- Evidence: https://opencode.ai/docs/warming/
- Notes: (verified in review)

### 2. Built-in tools
All from https://opencode.ai/docs/tools/ (V2). Permission action in brackets.
### read [read]
- What: file text (2,000 lines / 50 KiB per page) or non-recursive directory listing; PNG/JPEG/GIF/WebP/PDF go to the model up to 20 MiB.
- Status: GA
- Evidence: https://opencode.ai/docs/tools/
### glob [glob] and grep [grep]
- What: glob default 100 files, 30 s timeout; grep regex or literal, default 100 matches. Permission resource for grep is the pattern, not the path.
- Status: GA
- Evidence: https://opencode.ai/docs/tools/
### edit, write, patch [all under `edit`]
- What: edit replaces an exactly-once-matching string (or all with `replaceAll`); write creates/overwrites; `patch` applies a multi-file Add/Update/Delete patch and is exposed only for supported GPT models (others get edit/write).
- Status: GA
- Evidence: https://opencode.ai/docs/tools/
### shell [shell]
- What: runs in the host user's shell; params command, workdir, timeout (default 2 min), `background`. Background calls notify the session on completion. A built-in scanner extracts each command for permission checks. (Since v2.2.5, calls to a tool named `bash` run as `shell`.)
- Status: GA
- Evidence: https://opencode.ai/docs/tools/ ; https://opencode.ai/docs/permissions/ (Shell Scanner); https://opencode.ai/changelog (v2.2.5)
### webfetch [webfetch] and websearch [websearch]
- What: webfetch gets one http(s) URL as markdown/text/html (textual responses only). websearch uses a configured provider (see area 14/websearch entry).
- Status: GA
- Evidence: https://opencode.ai/docs/tools/ ; https://opencode.ai/docs/websearch/
### question [question]
- What: pauses to ask the user structured multi-choice questions; client must support the form.
- Status: GA
- Evidence: https://opencode.ai/docs/tools/
### skill [skill]
- What: loads a skill by ID.
- Status: GA
- Evidence: https://opencode.ai/docs/tools/ ; https://opencode.ai/docs/skills/
### subagent [subagent]
- What: starts a child session with a configured subagent (foreground or `background`, resumable with `sessionID`); default nesting depth one (`experimental.subagent_depth`).
- Status: GA
- Evidence: https://opencode.ai/docs/tools/ ; https://opencode.ai/docs/config/
### execute (Code Mode) [execute]
- What: the model writes JavaScript that calls and combines catalog tools (including in parallel); no filesystem, imports or timers, only `fetch`. MCP tools are by default reached through it. Denying `execute` for `*` removes Code Mode and tools reachable only through it; nested tools still enforce their own permissions. Namespaces: sessions (`session_rename`, `session_move`), worktrees (`worktree_create/list/remove`), and `browser` (attached by the desktop app: tabs, snapshots, clicking, screenshots, console/network inspection, traces, Lighthouse; 5 MiB per file transfer).
- Status: GA (no experimental label on the tools page)
- Evidence: https://opencode.ai/docs/tools/ ; https://opencode.ai/docs/mcp-servers/
- Notes: unusual design; see section 3. (verified in review: no fs/imports/timers, deny-removes-Code-Mode, 5 MiB; the session/worktree namespace names were not re-checked)
### todowrite
- What: task-tracking tool, opt-in via `experimental.todo`.
- Status: experimental
- Evidence: https://opencode.ai/docs/config/ (Experimental Flags)
### experimental_image_generate
- What: creates or edits images when a connected provider has an image model; runs in background and notifies with the saved path.
- Status: experimental
- Evidence: https://opencode.ai/docs/tools/ ; https://opencode.ai/changelog (v2.2.1, v2.2.2: enabled by default when a provider supports it)
### list/read MCP resources
- What: built-in tools `list_mcp_resources` and `read_mcp_resource`, permissions `opencode_list_mcp_resources` / `opencode_read_mcp_resource`.
- Status: GA
- Evidence: https://opencode.ai/docs/mcp-servers/

### 3. Code editing and file operations
### Edit semantics, formatting and error messages
- What: exact-match replace; formatters can run after write/edit/patch; edit errors show closest lines and read errors name the missing folder (v2.2.5). Tool output retention is configurable via `tool_output.max_lines/max_bytes`.
- Status: GA
- Evidence: https://opencode.ai/docs/tools/ ; https://opencode.ai/docs/config/ ; https://opencode.ai/changelog
### Formatters
- What: off by default. `"formatter": true` enables ~30 built-ins (gofmt, prettier, oxfmt, biome, ruff, uv, rustfmt, rubocop, clang-format, ktlint, shfmt, etc.), each only if its binary/project requirement is present. Custom formatters are an argv array with `$FILE`, matched by final file extension; first successful one wins.
- Status: GA
- Evidence: https://opencode.ai/docs/formatters/
### Attachments
- What: `file:`/`data:` URIs only (no http URLs); text and directory listings inlined; images and PDFs sent as media if the model supports them; 20 MiB per item; images auto-resized (default max 2000x2000, 5 MiB base64); type detected from content.
- Status: GA
- Evidence: https://opencode.ai/docs/attachments/
### References
- What: named directories or Git repos (with optional branch) usable as context outside the project; repo checkouts stored in the global data dir, refreshed in background at most every 24 h; grant no extra tool access.
- Status: GA
- Evidence: https://opencode.ai/docs/references/
### Cross-project web artifacts
- What: agents can publish and serve HTML/static artifacts (API routes under `/api/experimental/artifacts`).
- Status: experimental
- Evidence: https://opencode.ai/changelog (v2.2.4); https://opencode.ai/docs/api/

### 4. Terminal and shell integration
### Shell mode and persistent terminals
- What: `!` at the start of an empty prompt runs a command in the session's directory with output kept in the session. Desktop terminals belong to the session, keep scrollback across server restarts and sync with the TUI; API has a persistent PTY with WebSocket streaming and single-use connect tickets.
- Status: GA / PTY endpoints under `experimental`
- Evidence: https://opencode.ai/docs/cli/tui/ ; https://opencode.ai/changelog (v2.2.4) ; https://opencode.ai/docs/api/
### Background shells
- What: long-running shell commands run in background with live output and a stop control (desktop activity pill).
- Status: GA
- Evidence: https://opencode.ai/changelog (v2.2.1) ; https://opencode.ai/docs/tools/
### `opencode mini`
- What: starts a minimal interactive interface instead of the full-screen TUI; supports `--continue`, `--model`, `--prompt`, `--session`.
- Status: GA
- Evidence: https://opencode.ai/docs/cli/ ; https://opencode.ai/docs/cli/commands/
- Notes: (corrected in review: flags are documented on the commands page; status label was "unclear")

### 5. Permissions, approvals and sandboxing
### Ordered permission rules (V2)
- What: ordered array of `{action, resource, effect}` with effects allow/deny/ask. `*` matches anything including `/`, `?` one char; last match wins; if an operation checks several resources any deny denies, else any ask asks; no match means ask. Base policy per agent: allow all, ask on reading `*.env`/`*.env.*`, allow `*.env.example`. Shipped agents add rules (plan denies edits except under `~/.opencode/plan`; explore is read-only; title/summary deny all). Agent rules append after global rules; a custom subagent uses only its own permissions, not its parent's. Plugins can define new action names (they are plain strings).
- Status: GA
- Evidence: https://opencode.ai/docs/permissions/
- Notes: `doom_loop` and `lsp` are not V2 actions (the page says so explicitly). V1 used grouped per-tool permissions. (verified in review: last match wins, any deny denies / any ask asks, no match means ask, `.env` base policy; glob details and shipped-agent rules not re-checked)
### Shell scanner
- What: built-in parser for Bash, Zsh, POSIX sh and PowerShell extracts every subcommand and `cd` target for rule matching; unparseable commands error and do not run; directory inference is "best effort" so docs recommend narrow allowlists. A rule `git status *` also matches bare `git status`.
- Status: GA
- Evidence: https://opencode.ai/docs/permissions/
- Notes: (verified in review: unparseable or dynamically evaluated commands return a scanner error and do not run; the list of four shells was not re-checked)
### Approval replies
- What: `once`, `always` (saves the tool's proposed patterns as project-scoped allow rules, e.g. shell prefixes; never overrides a configured deny) or `reject` (rejects this and every other pending request in the session, with optional feedback).
- Status: GA
- Evidence: https://opencode.ai/docs/permissions/
- Notes: (verified in review)
### `external_directory`
- What: checked before read/edit for paths outside the project/location; allowed by default in V2.
- Status: GA
- Evidence: https://opencode.ai/docs/permissions/ ; https://opencode.ai/docs/migrate-v1/
- Notes: (verified in review)
### Managed policies
- What: separate `policies` statements (actions `provider.use`, `tool.use`, `integration.connect/use`, `mcp.use`, `skill.use`, `plugin.use`, `websearch.use`) that only ever tighten what permissions and providers would otherwise allow (within the policy list itself, a `tool.use` allow can lift an earlier broader deny, but never grants access on its own). Sources, highest authority first: macOS managed preferences, system `opencode.json(c)` (`/etc/opencode` etc.), connected Console workspace, then global, project and `.opencode` files. Reverse of normal "nearest wins". Console policy changes apply on the next fetch, about once a minute. The Console and policy plugins cannot be disabled. A denied `tool.use` check overrides "Allow always" and fails with "Blocked by configuration policy". Docs state policies do not sandbox plugin code.
- Status: GA
- Evidence: https://opencode.ai/docs/policies/ ; https://opencode.ai/docs/config/
- Notes: (corrected in review: added the allow-can-lift-deny caveat; the claim that Console policies persist when the Console is unreachable was not found on the page and is dropped)
### OS-level sandbox
- What: nothing documented in the V2 pages I opened (tools, permissions, policies, config). Safety is rule- and scanner-based on the host user's account.
- Status: nothing documented
- Evidence: https://opencode.ai/docs/permissions/ (absence)

### 6. Context management and compaction
### Compaction (summary and native)
- What: automatic when the conversation reaches the window minus a buffer (default `"15%"`, min 16k; percent or token count). Summarises the whole conversation, keeps roughly the last `keep.tokens` (default 15,000) verbatim; later compactions update the summary using only newer conversation. The summary request reuses the normal request's prompt cache. Summaries are structured (objective, decisions, work, blockers, files). Overflow errors trigger compact-and-retry once. Manual compact via `POST /api/session/{id}/compact`. Summary model overridable via `agents.compaction.model`. Per-provider/model `settings.compaction.type` = `native` (provider-side opaque checkpoint, OpenAI Responses models) or `summary`.
- Status: GA
- Evidence: https://opencode.ai/docs/compaction/
- Notes: V1 had tail-turn and prune behaviour; those keys are ignored in V2. (verified in review: 15% buffer, keep.tokens 15000, retry once, native vs summary)
### Background compaction
- What: pre-writes the summary ahead of the threshold and swaps it in; held summaries lost on restart; unused ones cost tokens.
- Status: experimental
- Evidence: https://opencode.ai/docs/compaction/ ; https://opencode.ai/docs/config/
- Notes: (verified in review)
### Plugin compaction hook
- What: `compaction` session hook can modify or replace checkpoint summaries.
- Status: GA
- Evidence: https://opencode.ai/docs/build/plugins/server/
### Tool output retention
- What: `tool_output.max_lines/max_bytes`; shell and webfetch shorten large output and keep the full text in a managed file.
- Status: GA
- Evidence: https://opencode.ai/docs/config/ ; https://opencode.ai/docs/tools/

### 7. Memory and persistent instructions
### AGENTS.md and `instructions`
- What: global `~/.config/opencode/AGENTS.md`; upward discovery from the workspace; nested `AGENTS.md` files load when a file or directory beneath them is read (nearest first; not re-injected while in history; nested edits not detected mid-session). `instructions` array takes files, globs, `~/` paths, absolute paths and http(s) URLs (5 s fetch timeout). V2 reads only `AGENTS.md`. Sources are concatenated; conflicts are not resolved. `OPENCODE_DISABLE_PROJECT_CONFIG=1` skips project discovery.
- Status: GA
- Evidence: https://opencode.ai/docs/instructions/
### Plugin-added session instructions
- What: plugins can add keyed session instructions that do not alter the system prompt, so the prompt cache is not invalidated.
- Status: experimental/new (v2.2.0)
- Evidence: https://opencode.ai/changelog (v2.2.0)
### Cross-session memory
- What: no memory feature documented in the V2 pages opened.
- Status: nothing documented
- Evidence: https://opencode.ai/docs/llms.txt (no memory page)

### 8. Planning, tasks and modes
### Build and Plan agents
- What: `build` default full-access; `plan` explores without editing normal project files (edit denied except under `~/.opencode/plan`). Switch with Tab/Shift+Tab or `/agents`. README says plan asks before bash; V2 permissions page lists plan as denying edits and allowing questions.
- Status: GA
- Evidence: https://opencode.ai/docs/agents/ ; https://opencode.ai/docs/permissions/ ; https://github.com/anomalyco/opencode (README)
- Notes: README and V2 permissions page differ slightly on plan's shell behaviour (README is likely V1-era).
### Todos, `/btw`, `/goal`
- What: todo tool opt-in; `/btw <question>` asks a side question not added to the conversation; `/goal` see area 1.
- Status: experimental (todo) / GA (btw)
- Evidence: https://opencode.ai/docs/config/ ; https://opencode.ai/docs/cli/tui/

### 9. Sub-agents and multi-agent orchestration
### Primary, subagent and `all` modes; hidden system agents
- What: agent = system prompt + model + permissions + display info. Built-ins: `build`, `plan` (primary), `general` (research/multi-step; cannot launch subagents), `explore` (read-only). Hidden maintenance agents `compaction`, `title`, `summary` are not selectable. No built-in `scout` in V2. Subagents start with fresh context; a subagent with no model inherits the parent's. Parent's `subagent` permission controls which can be launched; the child uses its own permissions.
- Status: GA
- Evidence: https://opencode.ai/docs/agents/ ; https://opencode.ai/docs/permissions/
### Agent definition format
- What: Markdown in `.opencode/agents/` or `~/.config/opencode/agents/` (frontmatter + body as system prompt; nested paths become IDs) or JSONC under `agents`. Options: description, model (`provider/model#variant`), system, hidden, color, disabled, request, permissions, steps, `default_agent`.
- Status: GA
- Evidence: https://opencode.ai/docs/agents/ ; https://opencode.ai/docs/migrate-v1/
### Background subagents, resumable child sessions
- What: `background: true` returns immediately and notifies the parent; `sessionID` continues a prior child; depth default one.
- Status: GA (depth flag experimental)
- Evidence: https://opencode.ai/docs/tools/ ; https://opencode.ai/docs/config/
### Peer-agent orchestration across machines
- What: nothing documented beyond child sessions.
- Status: nothing documented

### 10. Background, long-running and scheduled work
### Schedules
- What: opt-in `experimental.schedules` runs scheduled prompts under `opencode serve`, firing once across multiple server processes. API: `/api/experimental/schedule` (create, list, pause, resume, delete) scoped to a session.
- Status: experimental
- Evidence: https://opencode.ai/changelog (v2.2.4) ; https://opencode.ai/docs/api/
- Notes: I found no schedules documentation page; the configuration page did not list the flag. (verified in review: changelog v2.2.4 and the four `/api/experimental/schedule` routes)
### Background shells and subagents
- What: see areas 2 and 9.
- Status: GA
### CI runs
- What: `opencode run -w` starts a scripted run in a new git worktree (`--worktree=<name>`), `--archived` hides scripted sessions from lists, `--auto` flag exists.
- Status: GA
- Evidence: https://opencode.ai/changelog (v2.2.6) ; https://opencode.ai/docs/cli/commands/

### 11. Sessions: persistence, resume, fork, checkpoints, undo
### Persistent sessions with durable event log
- What: sessions stored in a database under `~/.local/share/opencode/` (engine not named on the docs page). API exposes a durable per-session event log readable after a sequence number, with `follow=true` for live tailing. Sessions can be forked before a message, moved to another directory, exported/imported (CLI `session export --sanitize`, `session import`).
- Status: GA; log and import/export routes are marked experimental
- Evidence: https://opencode.ai/docs/troubleshooting/ ; https://opencode.ai/docs/api/ ; https://opencode.ai/docs/cli/commands/
- Notes: (verified in review: path `~/.local/share/opencode/opencode.db`, no engine named, log route `GET /api/experimental/session/{id}/log` with exclusive sequence and `follow=true`, `GET /api/event` volatile by contract, `session export --sanitize`)
### Snapshots, `/undo`, `/redo`
- What: a Git object store separate from the repo (under the data dir) takes a snapshot before each model step and after it; the assistant message records changed paths. `/undo` stages a rollback restoring only paths attributed to assistant steps after the boundary and returns the prompt to the composer; resubmitting commits it. Covers the active directory in a git worktree; tracked files plus untracked non-ignored files up to 2 MiB; shell side effects and ignored files are not reverted. Outside git, only conversation rollback. Rejected while a session runs. Docs warn snapshot objects contain file contents (treat data dir as sensitive). Disable with `"snapshots": false`.
- Status: GA
- Notes: (verified in review)
### Session sharing
- What: discontinued. The v1.19.1 changelog entry (15 Oct 2026) says session sharing has been discontinued, existing share links and share commands are no longer available, and importing sessions from share URLs is unsupported. The V2 docs index has no share page; the V2 config schema in the API reference still carries a `share` field (values manual, auto, disabled), which a search snippet of the V2 config page says is accepted but not implemented.
- Status: deprecated (removed)
- Evidence: https://opencode.ai/changelog/v1.19.1 ; https://opencode.ai/docs/api/ (Config schema `share` field) ; https://opencode.ai/docs/llms.txt
- Notes: (corrected in review: was "nothing documented"; the removal is documented in the changelog. The V2 config statement is from a search snippet only; the V2 config API page URL returned 404.)
### Session migration V1 to V2
- What: `GET /api/experimental/migration/v1` reports migration progress of V1 session history.
- Status: experimental
- Evidence: https://opencode.ai/docs/api/

### 12. Git and version control
### Worktrees
- What: first-class: `worktree` config sets parent directory; tools/API create, list, refresh, remove worktrees with pluggable "strategies" (plugins can register custom worktree strategies); desktop shortcuts choose a worktree and base branch; `opencode run -w`.
- Status: GA
- Evidence: https://opencode.ai/docs/config/ ; https://opencode.ai/docs/build/sdk/ ; https://opencode.ai/changelog (v2.2.5, v2.2.6)
### VCS API
- What: `/api/vcs` routes for init, branch info, inferred review base, status, branches, diff.
- Status: GA
- Evidence: https://opencode.ai/docs/api/
### Commits and PRs
- What: no built-in commit or PR tool listed in the tool catalogue.
- Status: nothing documented
- Evidence: https://opencode.ai/docs/tools/

### 13. Extensibility
### Server plugins (V2)
- What: `Plugin.define({id, setup(ctx)})`; `ctx` is a server client plus plugin-only methods (durable `ctx.storage`, options, location). Hook points: session `prompt`, `context`, `compaction`, `generate`, `title`, `experimental.model.resolve`, `model.request`, `http.request/response`, `experimental.ws.handshake/send/receive`, `retry`; `permission.evaluate`; `shell.create.before`; `tool.execute.before/after`. Tools registered via `ctx.tool.transform` (JSON Schema input, progress callback, abort signal); each model request captures a stable tool snapshot. Events via `ctx.event.subscribe`. `evaluate` can change allow/ask but not configured deny. Effect-based variant exists.
- Status: GA (some hooks marked experimental)
- Evidence: https://opencode.ai/docs/build/plugins/server/
### Plugin types and distribution
- What: four parts in one npm package: server, TUI (`./tui`, needs OpenTUI 0.8+), GUI (`./gui-experimental`, Electron desktop; panels, slots, themes, code-block actions, main-process access), RPC (typed contract). Install with `opencode plugin add|list|check|update|remove`, npm or Git specs (tags, commits), local directories, or auto-discovery in `.opencode/plugins/`. `-id` entries disable plugins. Docs have no dedicated plugin sandbox or security section.
- Status: server/TUI GA; GUI experimental
- Evidence: https://opencode.ai/docs/plugins/ ; https://opencode.ai/docs/build/plugins/ ; https://opencode.ai/docs/build/plugins/gui/
- Notes: V1 plugins (e.g. hooks `chat.message`, `tool.execute.before`) do not run in V2; mapping table in https://opencode.ai/docs/build/plugins/migrate-v1/. Three experimental V1 hooks have no V2 equivalent.
### Custom tools
- What: V2 custom tools come through plugins (`ctx.tool.transform`). A V1 "Custom Tools" page exists but was not opened.
- Status: GA via plugins
- Evidence: https://opencode.ai/docs/build/plugins/server/ ; https://opencode.ai/v1/docs/llms.txt (listing only)
### Custom commands
- What: Markdown or JSON; `$ARGUMENTS`, `$1..`; `` !`cmd` `` injects shell output before submission and runs OUTSIDE the permission flow (docs warn); `agent`, `model`, `subagent` frontmatter; nested folders namespace commands; commands can replace built-ins; command model precedence: command, then agent, then session.
- Status: GA
- Evidence: https://opencode.ai/docs/commands/
- Notes: (verified in review: shell blocks run outside the tool permission flow; model precedence)
### Skills
- What: `SKILL.md` directories discovered in `.opencode/skills`, `~/.config/opencode/skills`, and also `.claude/skills` and `.agents/skills` (global and project); HTTP catalogs via `index.json`; each step lists permitted skills (ID, name, description only), model loads the body via the `skill` tool; users load with `@skill-id`; permission action `skill`; `autoinvoke: false` hides from the model. `license`/`compatibility` frontmatter accepted but ignored.
- Status: GA
- Evidence: https://opencode.ai/docs/skills/
- Notes: (verified in review)
### MCP
- What: local (stdio) and remote (Streamable HTTP) servers under `mcp.servers`; OAuth on by default for remote with pre-registered client, Client ID Metadata Documents or dynamic registration; timeouts startup 30 s / catalog 30 s / execution 12 h; protocol `legacy` default, `auto`, or `2026-07-28`; `pool: true` shares one connection across directories with identical settings (2026-07-28 only); Code Mode on by default (tools grouped under server namespaces; `codemode:false` lists tools directly); server instructions injected as an `<mcp_instructions>` block; prompts become `/<server>:<prompt>`; CLI `opencode mcp add|list|auth|logout`. Per-agent MCP enablement not documented (use permission rules).
- Status: GA
- Evidence: https://opencode.ai/docs/mcp-servers/ ; https://opencode.ai/changelog (v2.2.6)
- Notes: (verified in review: Code Mode default, `codemode:false`, timeouts, protocol values and `legacy` default, `pool` default false, OAuth default)
### LSP
- What: V1 shipped ~30 language servers (some auto-downloaded) giving diagnostics; disabled by default there. V2 preserves LSP config but does not run language servers, expose LSP tools or produce diagnostics.
- Status: V1 GA; V2 not supported
- Evidence: https://opencode.ai/v1/docs/lsp/ ; https://opencode.ai/docs/migrate-v1/
- Notes: (verified in review: V2 accepts and preserves `lsp` config but runs no servers, exposes no LSP tools and produces no diagnostics)
### SDK
- What: `@opencode/sdk` embeds the full server in-process (no HTTP listener) via `OpenCode.create()`; `@opencode/client` is a network client with the same API (Promise values, AsyncIterable events, no replay or auto-reconnect), Node-only `@opencode/client/service` can discover/start the local service; `@opencode/plugin` for plugins; Effect variants; Cloudflare guide (Durable Objects, not read in detail). Changelog notes breaking changes in `@opencode/ai` (Promise API at root since v2.2.0).
- Status: GA
- Evidence: https://opencode.ai/docs/build/sdk/ ; https://opencode.ai/docs/build/client/ ; https://opencode.ai/changelog
### HTTP API
- What: described as experimental "HttpApi" with 170 operations and 497 schemas, OpenAPI 3.1 at `/docs/openapi.json`. Groups: server, location, agent/plugin/command/skill, session (prompt queue, compact, fork, revert, inbox, diffs, import/export), model/provider/integration/credential, generate, voice, MCP (runtime add/remove), project/form/permission, filesystem, PTY, event, worktree, VCS, websearch, usage, debug. `location` query param scopes requests to a project directory. SSE-style `GET /api/event` is volatile (slow consumer overflows and fails); durable per-session log for recovery. V1 used `/doc`, `/global/event`, `/tui/*` control endpoints.
- Status: experimental (as labelled by the page)
- Evidence: https://opencode.ai/docs/api/ (first ~200k of 265k characters read) ; https://opencode.ai/v1/docs/server/
### ACP
- What: `opencode acp` speaks newline-delimited JSON-RPC over stdio (ACP v1) for editors such as Zed; starts a private server, one prompt per session at a time; MCP over ACP unsupported.
- Status: GA
- Evidence: https://opencode.ai/docs/cli/acp/
### CLI `api` and `reload`
- What: `opencode api <method> <path|operationID>` calls the HTTP API from the shell; `opencode reload`.
- Status: GA
- Evidence: https://opencode.ai/docs/cli/commands/

### 14. Models, providers and inference configuration
### Provider catalogue and auth
- What: catalogue from models.dev plus native packages (OpenAI chat/responses, Anthropic, Google, Vertex, Azure, Bedrock, OpenRouter, xAI), npm or `file://` provider packages, `/connect` flow, local auto-discovery for Ollama, LM Studio and vLLM. Subscription OAuth for ChatGPT Plus/Pro and GitHub Copilot (device OAuth). "Claude Agent" provider uses a local Claude Code install (`claude auth login`).
- Status: GA
- Evidence: https://opencode.ai/docs/providers/ ; https://opencode.ai/docs/models/ ; https://opencode.ai/changelog (v2.2.3)
- Notes: use of consumer-subscription logins is governed by those vendors' terms; not assessed.
### Models, variants, caching
- What: `provider/model#variant` selectors; variants carry settings/headers/body; precedence provider, model, variant; per-provider/model `settings.cache.ttl`; `compatibility` block for API quirks; WebSocket transport per provider; `small_model` replaced by `agents.title.model`. Copilot "Auto" routing (v2.2.7).
- Status: GA
- Evidence: https://opencode.ai/docs/models/ ; https://opencode.ai/docs/config/ ; https://opencode.ai/changelog
### Web search providers
- What: Exa, Firecrawl, Parallel, Tavily, TinyFish; asks to enable on first use; `websearch.provider` or `"random"` with 429 cooldown failover.
- Status: GA
- Evidence: https://opencode.ai/docs/websearch/

### 15. UI surfaces
### TUI
- What: tabs, `/` command list, Ctrl+P palette, leader key Ctrl+X, `@` file attach with line ranges, `!` shell mode, external editor, themes and keybinds pages; built on OpenTUI.
- Status: GA
- Evidence: https://opencode.ai/docs/cli/tui/
### Desktop app
- What: Electron-based (third-party report) with sessions, tabs, diffs, built-in browser, persistent terminals, GUI plugins, worktree pickers, read/unread marking, quote/annotation comments. README labels it beta.
- Status: beta (README) / GA-style in changelog; unclear
- Evidence: https://opencode.ai/product ; https://opencode.ai/changelog ; https://github.com/anomalyco/opencode
### Web app, pairing and remote
- What: the web UI is served by the same server and is password-protected by default; `opencode pair` prints a single-use link (5-minute expiry) and QR code; opening it sets a 30-day session cookie; HTTP clients use Basic auth user `opencode`; `opencode pair --remote` exposes the server through "OpenTunnel" with end-to-end encrypted traffic (password still required); default port 49374 (28894 on Windows), localhost.
- Status: GA
- Evidence: https://opencode.ai/docs/cli/web/ ; https://opencode.ai/docs/
- Notes: (verified in review: single-use 5-minute link, 30-day sessions, `--remote` via OpenTunnel end-to-end encrypted with password still required, ports 49374/28894, localhost only by default, user always `opencode`; the CLI page adds that pairing needs the shared service)
### Mobile
- What: start, steer and approve sessions from a phone; iPhone via TestFlight, Android "coming soon".
- Status: beta/preview
- Evidence: https://opencode.ai/product ; https://opencode.ai/changelog (v2.2.6 composer dock)
- Notes: (verified in review: product page says iPhone via TestFlight, Android coming soon; the docs intro does not mention mobile)

### 16. Browser automation and computer use
### `browser` namespace in desktop
- What: desktop attaches a browser catalog reachable via `execute`: navigation, snapshots, clicking, forms, screenshots, console/network, traces, Lighthouse. No per-operation prompts; deny `browser` `*` removes it. A separate product "Browse" (agents using your real browser) is listed on the product page.
- Status: GA (desktop)
- Evidence: https://opencode.ai/docs/tools/ ; https://opencode.ai/product
- Notes: computer-use (screen control): nothing documented.

### 17. Remote, cloud, headless, CI and distributed execution
### Headless run and server
- What: `opencode run "<prompt>"` for scripts/CI (`--format json`, `--file`, `--agent`, `--session`, `--continue`, `--auto`, `--server`); `opencode serve` with `--hostname --port --cors`; `--server <url>` attaches any client to a chosen server; SSH port-forward recommended for localhost-only servers; Docker images `ghcr.io/anomalyco/opencode:<ver>`.
- Status: GA
- Evidence: https://opencode.ai/docs/cli/commands/ ; https://opencode.ai/docs/cli/ ; https://opencode.ai/docs/cli/web/ ; https://opencode.ai/docs/
### GitHub integration (V1 only)
- What: V1: `opencode github install` sets up the `opencode-agent` GitHub app and a workflow; `/opencode` or `/oc` mentions on issues, PRs and review lines; triggers `issue_comment`, `pull_request_review_comment`, `issues`, `pull_request`, `schedule`, `workflow_dispatch`; runs inside the Actions runner; OIDC token exchange or `use_github_token`. In V2 the GitHub Action is "no longer published" (v2.2.0) and the V2 docs have no GitHub page.
- Status: V1 GA; V2 removed/unclear
- Evidence: https://opencode.ai/v1/docs/github/ ; https://opencode.ai/changelog (v2.2.0)
- Notes: (verified in review: changelog says the GitHub Action is no longer published and to remove it from workflows; the V1 GitHub page details were not re-read)
- Notes: a GitLab page exists in the V1 index but was not opened.
### Distributed worker/node execution
- What: no master/worker model documented; one server per machine, clients attach. `process-daemon` release tags exist but are not described in docs I read.
- Status: nothing documented
- Evidence: https://opencode.ai/docs/build/ ; https://github.com/anomalyco/opencode/releases

### 18. Collaboration, teams and enterprise administration
(Commercial service, "OpenCode Console", optional.)
### Console workspaces, SSO, SCIM, budgets
- What: workspaces with members; SSO via OIDC or SAML 2.0 with domain verification and enforcement (included in every workspace); SCIM directory sync (page not opened); per-member, tagged and workspace budgets in USD resetting monthly 00:00 UTC, enforced as HTTP 429 at request start; usage breakdown with CSV export; team-wide policies pushed to every connected OpenCode client.
- Status: GA (commercial service)
- Evidence: https://opencode.ai/docs/console/ ; https://opencode.ai/docs/console/sso/ ; https://opencode.ai/docs/console/budgets/ ; https://opencode.ai/docs/policies/
### Local multi-user
- What: single `opencode` user and one password on the server; no per-user accounts documented for self-hosted use.
- Status: nothing documented

### 19. Observability
### Usage, logs, diagnostics
- What: `opencode stats --days --models --cost --project --json`; log at `~/.local/share/opencode/log/opencode.log` with `run` and `role` fields; `opencode debug heap-dump`, SIGPROF/SIGUSR1 profiles; `opencode debug paths|config|agents`; API `usage/limits` for Go allowance and Console budgets. `openTelemetry` config key is ignored in V2 (migration guide).
- Status: GA
- Evidence: https://opencode.ai/docs/cli/commands/ ; https://opencode.ai/docs/troubleshooting/ ; https://opencode.ai/docs/migrate-v1/
### Telemetry statement
- What: not covered by the pages opened. The product page says data is stored only when using the free models (vendor-claim).
- Status: unverified
- Evidence: https://opencode.ai/product

### 20. Configuration and customisation
### Config files and precedence
- What: JSON/JSONC; `$schema` https://opencode.ai/config.json; `{env:VAR}` and `{file:path}` substitution. Precedence low to high: global `~/.config/opencode/`, project files (farther first), `.opencode/` files, system config (`/etc/opencode`, `/Library/Application Support/opencode`, `%ProgramData%\opencode`), macOS managed preferences. Settings merge; `policies` evaluate in reverse (broader wins). System dir also loads agents/commands/skills/plugins for all users. Separate client file `~/.config/opencode/cli.json` for terminal client options.
- Status: GA
- Evidence: https://opencode.ai/docs/config/ ; https://opencode.ai/docs/migrate-v1/
### Experimental flags
- What: `todo`, `subagent_depth`, `background_compaction`, `image_generate.model`, `location_idle_timeout` (unloads idle project services after 60 min by default); changelog adds `schedules` and `artifacts`.
- Status: experimental
- Evidence: https://opencode.ai/docs/config/ ; https://opencode.ai/changelog
- Notes: (verified in review: defaults and the absence of schedules/artifacts from the config page)
### Network
- What: `HTTP_PROXY/HTTPS_PROXY/NO_PROXY` (must include localhost for the CLI-to-service link), `NODE_EXTRA_CA_CERTS`; `opencode service set env` persists variables for the background service.
- Status: GA
- Evidence: https://opencode.ai/docs/network/

### 21. Security and privacy
- Server auth: password (`OPENCODE_PASSWORD`), Basic auth user `opencode`, pairing codes, session token for JSON clients; loopback default. Evidence: https://opencode.ai/docs/cli/web/ ; https://opencode.ai/docs/api/ ; https://opencode.ai/docs/migrate-v1/. Status: GA.
- Plugin trust: no sandbox for plugin code (policies page says so). Evidence: https://opencode.ai/docs/policies/.
- Snapshot and heap-dump data may contain secrets (docs warn). Evidence: https://opencode.ai/docs/snapshots/ ; https://opencode.ai/docs/troubleshooting/.
- Privacy of hosted models: see area 22.
- Vulnerability disclosure, telemetry, data retention for the local app: nothing found in opened pages.

### 22. Licence, pricing and distribution
- Licence: MIT (README). Distribution: curl installer, Homebrew, npm, Bun, pnpm, Yarn, AUR, mise, Nix (README); standalone binaries for macOS/Windows/Linux incl. musl; desktop installers; Docker images. V2 curl installer replaces the V1 binary. Evidence: https://opencode.ai/docs/ ; https://github.com/anomalyco/opencode ; https://opencode.ai/docs/migrate-v1/.
### Hosted model offerings (COMMERCIAL SERVICE, optional)
- What: "OpenCode Console" gateway. (a) Pay-as-you-go catalogue (formerly "Zen"; `opencode.ai/zen` redirects to the console models page per 00-targets) billed per request against credits with a 4.6% + $0.31 credit-purchase fee; endpoints for OpenAI Responses/Chat, Anthropic Messages and Gemini formats with request translation; models hosted in US/EU; most providers zero-retention and no training, with exceptions (OpenAI and Anthropic APIs retain 30 days; some free models retain or train). (b) Go subscription: Go $10/month, Go Plus $40/month, Go Teams $20 per member; per-model monthly dollar allowances with 5-hour (20%), weekly (50%) and monthly limits; a broad list of open and third-party models; extra usage can fall back to credits. (c) Free models (time-limited, some region-limited, some data-training) hidden with `OPENCODE_DISABLE_FREE_MODELS=1`. (d) "Harnesses": use Console models from Claude Code, Codex, Hermes, Pi or Raycast through the gateway (`/inference` base URLs; `x-opencode-session` header for cache routing); free chat models work only inside OpenCode.
- Status: GA (commercial)
- Evidence: https://opencode.ai/docs/console/ ; https://opencode.ai/docs/console/models/ ; https://opencode.ai/docs/console/go/ ; https://opencode.ai/docs/console/harnesses/
- Notes: model names and prices on these pages change frequently; the product page's "100% free to use" statement is vendor-claim.

### 23. Anything else or unusual
### Locations
- What: API and SDK scope work by a "location" (directory plus optional workspace); idle locations unload after a timeout; `POST /api/location/reload` rebuilds them.
- Status: GA
- Evidence: https://opencode.ai/docs/api/ ; https://opencode.ai/docs/config/
### Other products in the same family
- What: Browse (real-browser agent), Hex (voice dictation), OpenTunnel (public HTTPS for local apps), OpenTUI (terminal UI library). Not surveyed.
- Status: unclear
- Evidence: https://opencode.ai/product

## 3. Distinctive or unusual

- Code Mode (`execute`): the model composes catalog and MCP tool calls in JavaScript, which cuts tool-schema context; denying it removes tools reachable only through it.
- Permission rules as a flat ordered list with last-match-wins plus a separate admin-only `policies` layer that only tightens (not purely deny: a policy allow can lift an earlier policy deny) and cannot be overridden by "always" approvals (corrected in review).
- Shell permission via a built-in multi-shell scanner (Bash, Zsh, sh, PowerShell) that checks every extracted subcommand and refuses unparseable input.
- Server-side durable per-session event log with sequence numbers and `follow` tailing, separate from the volatile broadcast stream.
- Pairing: single-use 5-minute link plus optional end-to-end-encrypted tunnel to reach the shared server from a phone.
- In-process SDK host with no HTTP listener, and a typed plugin RPC contract that clients can call.
- Plugins with four parts (server, TUI, GUI, RPC) in one package, and managed policies that can withhold plugins but are not a sandbox.
- Prompt-cache hygiene features: cache warming, keyed session instructions that do not break the cache, summary requests that reuse the main prompt cache, provider-native compaction.
- Snapshot store as a separate Git object DB with per-step before/after capture and attribution of paths to steps.
- Console gateway lets other harnesses (Claude Code, Codex, Pi, Hermes) use its models.

## 4. Architecture and UX notes

- One background server per machine owns sessions, plugins, permissions and state; TUI, desktop and web are clients on one HTTP API. The agent loop and tools run in that server (on the machine that holds the code). Source: https://opencode.ai/docs/troubleshooting/ ; https://opencode.ai/docs/build/.
- Data in a local database (`opencode.db`; engine unnamed in docs) plus log and state files. Source: troubleshooting page.
- Protocols: HTTP with OpenAPI 3.1, event stream, WebSocket for PTY and client-hosted MCP, JSON-RPC over stdio for ACP. Source: API and ACP pages.
- Remote use: localhost bind by default; password and pairing; SSH forward or OpenTunnel for reach. Source: web page.
- Config is layered with a hard admin layer (system/MDM/Console) above user and project files.

## 5. Relevance to a self-hosted master/node harness

- The shared-service plus attach model (area 1, area 17) is the closest public analogue to master plus nodes, but it is one server per machine with no node fleet; the gap is exactly what rizzy fills.
- Policy layer above project config that only tightens (area 5, Managed policies) mirrors "the node decides"; note they also ship Console-delivered policies that outrank repo files.
- Permissions as an ordered rule list with `once/always/reject`, where reject cancels all pending requests in the session (area 5), is a UX worth comparing.
- Durable per-session log with sequence numbers and tail-follow (area 11, API) parallels the "UI is a fold over the log" design; their separate volatile broadcast stream shows the failure mode of slow consumers.
- Pairing code and cookie/token flow (area 15) is a concrete pattern for web access without accounts.
- Plugin hooks surface (area 13) lists exactly which points third parties wanted (permission evaluate, shell create, tool before/after, compaction, context); useful scope reference, and the lack of a plugin sandbox is the cautionary note.
- Snapshot design and its stated limits (area 11) are a checklist of what undo does not cover.
- Compaction details (keep tokens, percentage buffer, cache reuse) (area 6) are directly reusable ideas.
- Commands' `` !`cmd` `` injection bypassing the permission flow (area 13) is an approval-path hole to avoid.
- Hosted gateway for other harnesses (area 22) shows a commercial model; irrelevant to self-hosting but explains some product choices (free models, Console policies).

## 6. Overhyped, stale or contradictory

- Version confusion: the docs say V2 current; GitHub "latest" still flags v1.19.5 (per 00-targets); a third-party Korean post says "2.0" referred to Desktop v2 while the CLI stayed 1.18.x (via search snippet, not opened). See https://opencode.ai/changelog.
- README (https://github.com/anomalyco/opencode) lists npm `@opencode/cli`, describes plan as asking before bash, calls the desktop app beta and has no client/server note: likely lags V2.
- The API page calls the whole surface experimental while the SDK pages present it as the supported contract.
- Config page does not list `experimental.schedules` or `experimental.artifacts`, which the changelog introduces; `/goal` is in the changelog but has no docs page in what I read.
- The V2 docs omit sharing and GitHub integration entirely; V1 pages remain live and could mislead. Sharing was discontinued in v1.19.1 (changelog) while a V1 share page may still be indexed by search engines (https://opencode.ai/en/docs/share, seen in a search result only). (added in review)
- Product page: "One agent, every surface", "100% free to use", and a "Degraded performance" status banner at the time of fetch (https://opencode.ai/product); marketing, vendor-claim.
- Console page does not map the three prices to plan names explicitly; the Go page does (Go $10, Go Plus $40, Go Teams $20/member).

## 7. Unverified

- (Resolved in review: session sharing was discontinued, see the Session sharing entry. Still unverified: the V2 config page statement that the `share` field is accepted but not implemented, seen only as a search snippet.)
- Custom Tools (V1 page), V1 Enterprise, V1 Rules, V1 GitLab, IDE pages: listed in the V1 index, not opened.
- SCIM, Console workspaces/members/billing/providers/web search pages: not opened.
- What `opencode mini` is beyond "minimal interactive interface"; `process-daemon` release tags; `/goal` reviewer mechanics; schedules and artifacts documentation.
- Database engine behind `opencode.db`; telemetry behaviour of the local app.
- Desktop app is Electron: only from the GUI plugin page (mentions an Electron main process) and a third-party article; Tauri-to-Electron claim is third-party.
- GitHub "latest" flag and release badges: not visible in the fetched releases page.
- Exact repository default branch and licence file: taken from 00-targets and README text, not independently re-read.

## 8. Sources

All opened 2026-11-10:
- https://opencode.ai/docs/llms.txt (V2 docs index)
- https://opencode.ai/docs/ (intro, install, surfaces)
- https://opencode.ai/docs/migrate-v1/ (V1 to V2 changes)
- https://opencode.ai/changelog (releases v2.2.0 to v2.2.7, v1.19.x)
- https://opencode.ai/docs/tools/ (built-in tools)
- https://opencode.ai/docs/permissions/ (rule system)
- https://opencode.ai/docs/policies/ (managed policies)
- https://opencode.ai/docs/agents/ (agents)
- https://opencode.ai/docs/api/ (HTTP API; read to 200k of 265k chars)
- https://opencode.ai/docs/cli/ (CLI intro, service)
- https://opencode.ai/docs/cli/commands/ (subcommand list)
- https://opencode.ai/docs/cli/web/ (web mode, pairing)
- https://opencode.ai/docs/cli/tui/ (TUI)
- https://opencode.ai/docs/cli/acp/ (ACP)
- https://opencode.ai/docs/build/ (Build overview)
- https://opencode.ai/docs/build/sdk/ (SDK)
- https://opencode.ai/docs/build/client/ (JS client)
- https://opencode.ai/docs/build/plugins/ (plugin overview)
- https://opencode.ai/docs/build/plugins/server/ (server plugin hooks)
- https://opencode.ai/docs/build/plugins/gui/ (GUI plugins)
- https://opencode.ai/docs/build/plugins/migrate-v1/ (hook mapping)
- https://opencode.ai/docs/plugins/ (plugin install/config)
- https://opencode.ai/docs/commands/ (custom commands)
- https://opencode.ai/docs/skills/ (skills)
- https://opencode.ai/docs/mcp-servers/ (MCP)
- https://opencode.ai/docs/compaction/ (compaction)
- https://opencode.ai/docs/snapshots/ (snapshots, undo)
- https://opencode.ai/docs/instructions/ (AGENTS.md)
- https://opencode.ai/docs/config/ (config reference)
- https://opencode.ai/docs/formatters/ (formatters)
- https://opencode.ai/docs/models/ (models and variants)
- https://opencode.ai/docs/providers/ (providers)
- https://opencode.ai/docs/websearch/ (web search)
- https://opencode.ai/docs/network/ (proxy, certs)
- https://opencode.ai/docs/references/ (references)
- https://opencode.ai/docs/attachments/ (attachments)
- https://opencode.ai/docs/warming/ (cache warming)
- https://opencode.ai/docs/troubleshooting/ (service, logs, storage)
- https://opencode.ai/docs/console/ , /console/models/ , /console/go/ , /console/harnesses/ , /console/sso/ , /console/budgets/ (commercial Console)
- https://opencode.ai/product (product surfaces, marketing)
- https://github.com/anomalyco/opencode (README)
- https://github.com/anomalyco/opencode/releases (release list)
- https://opencode.ai/v1/docs/server/ , /v1/docs/github/ , /v1/docs/lsp/ , /v1/docs/llms.txt (V1 docs)
- https://opencode.ai/v1/docs/share/ (404)
- Web search result (third-party, title and snippet only, pages not opened): https://thenewstack.io/opencode-2-shared-service/ ; https://byteiota.com/opencode-2-beta-unified-ai-coding-agent/ ; https://daleseo.com/opencode/

## 9. Review

Reviewed 2026-11-10 by an independent reviewer (public pages only, fetched through a summarising fetch tool with specific-fact prompts). Checked 31 entries or claim-groups out of about 95 entries in the file.

| Entry | Verdict | Reason |
|---|---|---|
| Identity: V2 current | confirmed-with-correction | Changelog does not say "current"; V1 docs banner says latest docs are V2 |
| Identity: v1.19.5 / v2.2.x dates, v1.19.4 | confirmed | Changelog dates match; "latest" badge not visible on releases page |
| V1 to V2 changes (LSP, external_directory, CLAUDE.md, username, todos, openTelemetry, cli.json) | confirmed | migrate-v1 page |
| Client/server split, service | confirmed | troubleshooting, CLI and commands pages |
| Step limit | confirmed | agents page |
| Cache warming | confirmed | warming page |
| read, shell, patch, subagent depth tool facts | confirmed | tools page |
| execute (Code Mode) | confirmed | tools page; namespace names not re-checked |
| Ordered permission rules | confirmed | permissions page |
| Shell scanner | confirmed | unparseable commands error; shell list not re-checked |
| Approval replies | confirmed | permissions page |
| external_directory | confirmed | allow by default in V2 |
| Managed policies | confirmed-with-correction | A policy allow can lift an earlier policy deny; "persist if Console unreachable" not found |
| Compaction, background compaction | confirmed | compaction page |
| Snapshots | confirmed | snapshots page |
| Session sharing | wrong | Discontinued in v1.19.1 (changelog), not merely undocumented |
| Durable session event log, volatile event stream | confirmed | API page |
| `opencode mini` | confirmed-with-correction | Flags are documented; status not "unclear" |
| MCP | confirmed | mcp-servers page |
| LSP | confirmed | migrate-v1 |
| Web app, pairing, remote | confirmed | cli/web page |
| Mobile | confirmed | product page |
| GitHub Action no longer published | confirmed | changelog v2.2.0 |
| Schedules, artifacts, /goal, v2.2.5 bash alias, `run -w`, keyed session instructions | confirmed | changelog and API routes |
| Custom commands (`!cmd` outside permission flow) | confirmed | commands page |
| Skills | confirmed | skills page |
| Experimental flags | confirmed | config page |
| Console budgets, Go prices | confirmed | budgets and go pages |
| Agents (built-ins, hidden agents, general cannot spawn) | confirmed | agents page |
| API: 170 operations, 497 schemas, experimental | confirmed | API page |
| Product page claims (100% free, data stored only for free models) | confirmed | product page, marked vendor-claim |

Borrowed features: none found. The descriptions follow OpenCode's own wording.

Notable omissions (documented, not in the survey as entries):
- Themes, keybinds, CLI config and CLI plugins pages: https://opencode.ai/docs/themes/ , https://opencode.ai/docs/cli/keybinds/ , https://opencode.ai/docs/cli/config/ , https://opencode.ai/docs/cli/plugins/
- Console API and inference endpoints: https://opencode.ai/docs/console/api/inference/
- Effect-based SDK, client and plugin variants: https://opencode.ai/docs/build/sdk/effect/
- Cloudflare SDK guide: https://opencode.ai/docs/build/sdk/cloudflare/
- Plugin RPC and TUI plugin pages: https://opencode.ai/docs/build/plugins/rpc/ , https://opencode.ai/docs/build/plugins/cli/
- Session sharing discontinuation: https://opencode.ai/changelog/v1.19.1 (now added)

Trust judgement: the entries I checked were accurate on mechanism, defaults and numbers, with one wrong status (sharing) and one overstated "can only deny" claim. The unchecked remainder (provider lists, Console models pricing, formatter list, TUI keys, plugin hook lists) is probably sound but the author's own warning about summarised fetches applies. Treat as mostly reliable; re-check anything load-bearing.
