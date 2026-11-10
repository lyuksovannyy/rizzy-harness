# Cline (round 1 public-surface survey)

Surveyed 2026-11-10. Public docs and GitHub README/releases only; no source read. Docs base: https://docs.cline.bot/ (pages fetched as `.md`).

## 1. Identity

- Name: Cline. Vendor: Cline Bot Inc. (copyright line in repo README).
- Surfaces: VS Code extension (also Cursor, Windsurf, Antigravity, Zed per overview), JetBrains plugin (not open-sourced), CLI/TUI (`npm i -g cline`), Desktop app (macOS, Windows beta; Tauri shell + Bun sidecar + Next.js UI per README), Kanban (`npx kanban`, research preview), Cloud Agents (browser, GitHub), SDK (`@cline/sdk`, Node 22+), ACP mode (`cline --acp`), Enterprise.
- Open or closed: open, Apache-2.0, `cline/cline`, 71.5k stars (README). JetBrains plugin not open.
- Version: GitHub releases show extension `v4.1.26` (26 Oct), SDK `v0.2.0` (26 Oct), Desktop `v0.0.49`, CLI `v3.0.74` (all 26 Oct). Matches `00-targets.md`. The release page shows no year (day/month only).
- Structure per README: `sdk/`, `apps/cli/`, `apps/vscode/`, `apps/examples/desktop-app/`, `docs/`.
- Cline describes itself as one shared agent engine under all surfaces (overview; SDK overview).

## 2. Features by area

### 1. Agent loop and execution model

### Shared agent core (SDK) under every surface
- What: All apps sit on `@cline/core` (Node runtime: sessions, built-in tools, persistence, hub, scheduling, teams). `@cline/agents` is a browser-compatible stateless loop (`run`, `continue`, `abort`, `subscribe`, `restore`, `snapshot`); `@cline/llms` is provider gateway and catalogs; `@cline/shared` holds types and hook contracts.
- Status: GA (no maturity label on SDK page; unclear)
- Evidence: https://docs.cline.bot/sdk/architecture/overview.md (Packages); https://docs.cline.bot/sdk/overview.md (Cline SDK: Overview)
- Notes: SDK page states no version, licence or stability. The repo is Apache-2.0.

### Hub / spoke / client process model
- What: A per-machine singleton "hub" daemon coordinates sessions, routes events and approvals, runs schedules, brokers capabilities. It does not run the loop. "Spoke" workers run `@cline/core` and the loop, owned by the daemon so sessions outlive clients. Clients (CLI, VS Code, JetBrains, custom) connect over WebSocket, register capabilities (shell, file edit, diff view); sessions can have several participants, each tracked with a role creator/participant/observer. Hub fans events out to all attached clients and routes capability requests (e.g. diff to IDE, shell to CLI). Default `127.0.0.1:25463`; backend modes `auto|hub|remote|local`.
- Status: GA (as documented); unclear for `remote`
- Evidence: https://docs.cline.bot/sdk/architecture/hub-spoke.md (Hub-Spoke Architecture); https://docs.cline.bot/cli/cli-reference.md (Environment Variables)
- Notes: Docs say nothing about authentication or how a remote endpoint is configured; `remote` mode is listed but unexplained. (corrected in review: roles are per session participant, not per client; the no-authentication/no-endpoint-config statement, loop-in-spoke, 127.0.0.1:25463 and four backend modes were verified)

### Retry and mistake limit
- What: CLI `--retries` sets consecutive mistakes allowed before exit (default 3). Release notes: model-request retries reduced from 5 to 3 in v4.1.26.
- Status: GA
- Evidence: https://docs.cline.bot/cli/cli-reference.md (Global Flags); https://github.com/cline/cline/releases

### 2. Built-in tools

### ClineCore built-in tool list
- What: `read_files`, `search_codebase`, `run_commands`, `fetch_web_content`, `apply_patch`, `editor`, `skills`, `ask_question`, `submit_and_exit`. Plus `spawn_agent` (subagents) and `teams` (toggleable in CLI `/settings` Tools tab), MCP tools, plugin custom tools.
- Status: GA
- Evidence: https://docs.cline.bot/tools-reference/all-cline-tools.md (Tools); https://docs.cline.bot/features/subagents.md; https://docs.cline.bot/cli/agent-teams.md
- Notes: Page gives no parameters. Custom tools apply to SDK, CLI and Kanban only, not IDE extensions. Page mentions "legacy tool names" (e.g. `write_to_file`, `read_file` still appear in other pages) without listing them: possible mixed old/new naming across docs.

### 3. Code editing and file operations

### Model-routed edit tool (`editor` vs `apply_patch`)
- What: A session offers either `editor` or `apply_patch`, not both. In Act mode `apply_patch` is routed to GPT/Codex models and all `openai-native` models; `editor` to others. Plan mode offers neither. SDK callers can override via `toolRoutingRules`.
- Status: GA
- Evidence: https://docs.cline.bot/tools-reference/all-cline-tools.md (editing tool routing)

### Context additions: @ mentions, files, images
- What: `@/path` adds a file, `@/dir/` folder; multi-root uses `@workspace:/path`. Supports text, images (multimodal models), PDFs, CSV, Excel. Editor context menu: Add/Fix/Explain/Improve with Cline; lightbulb Quick Fix; Source Control "Generate Commit Message" (conventional commits).
- Status: GA (VS Code)
- Evidence: https://docs.cline.bot/core-workflows/working-with-files.md (Adding Context)
- Notes: Page does not describe diff/edit mechanics or limits. VS Code specific.

### Jupyter notebooks
- What: Generate/Explain/Improve Cell via cline.jupyter* commands, preserving cell metadata and outputs; needs VS Code Jupyter extension; less efficient on very large notebooks.
- Status: GA
- Evidence: https://docs.cline.bot/features/jupyter-notebooks.md

### Multi-root workspaces
- What: Listed in docs nav and used in @ mentions and per-root `.clineignore`; page itself not read (fetch of the Jupyter page returned nothing on it).
- Status: unclear
- Evidence: https://docs.cline.bot/core-workflows/working-with-files.md (multi-root mention only)
- Notes: Dedicated page https://docs.cline.bot/features/multiroot-workspace.md not opened.

### 4. Terminal and shell integration

### Shell command execution and approval flag
- What: `run_commands` tool. The model sets a `requires_approval` flag per command; there is no fixed allowlist. Safe examples: builds, tests, read-only; unsafe: installs, `rm -rf`, `mv`, `sed -i`. Notification fires when an auto-approved command has run 30 s. Overview says Cline watches long-running processes such as dev servers (README).
- Status: GA
- Evidence: https://docs.cline.bot/features/auto-approve.md (Command Safety Judgment); https://github.com/cline/cline (README)
- Notes: Safety is model-judged, not rule-based. CLI has no built-in allow/deny list; use a PreToolUse hook (CLI Overview). 30 s notification applies only if notifications are enabled. (verified in review)

### 5. Permissions, approvals and sandboxing

### Per-category auto-approve (IDE)
- What: Toggles: read project files / read all files, edit project / all, execute safe / all commands, use browser, use MCP, notifications. "All" variants only extend their base toggle. Recommended start: read project files only.
- Status: GA
- Evidence: https://docs.cline.bot/features/auto-approve.md (Auto Approve & YOLO Mode)
- Notes: (verified in review) Doc says "Use the browser" category exists, but no browser-tool page exists in the current docs nav (see Unverified).

### YOLO mode
- What: Auto-approves everything: edits anywhere, all commands, browser, MCP, and Plan-to-Act transitions. Enabled in Settings, Features, with no confirmation dialog. Page warns it disables all safety checks.
- Status: GA
- Evidence: https://docs.cline.bot/features/auto-approve.md
- Notes: (verified in review for the YOLO behaviour on auto-approve page; `--yolo` is not in the CLI reference) CLI hooks are disabled under `--yolo` (clineignore page); teams do not work with `--yolo` (agent-teams page). The CLI reference lists `--auto-approve` and `-p`, not `--yolo`; scheduling lists `--mode act|plan|yolo`. Flag naming is inconsistent across pages.

### CLI auto-approve default is ON
- What: In the CLI `--auto-approve` defaults to `true` (a prompt with no flags runs act mode with auto-approve on), but `false` in ACP mode. TUI: Shift+Tab toggles "auto-approve all".
- Status: GA
- Evidence: https://docs.cline.bot/cli/cli-reference.md (Commands, Global Flags); https://docs.cline.bot/usage/tui.md (Keyboard Shortcuts)
- Notes: Differs sharply from the IDE default (approve everything). CLI Overview recommends a clean git branch. (verified in review: CLI reference and CLI Overview both state `--auto-approve` default true, false in ACP. Reviewer caution: the "IDE default (approve everything)" contrast refers to YOLO, which is opt-in; the IDE's per-category toggles default to prompting, so the comparison is loose)

### SDK tool policies and approval callback
- What: Per-tool `toolPolicies` (`autoApprove` true/false, `enabled:false` hides tool). Tools with no policy are enabled and auto-approved by default. Approval is a `requestToolApproval` callback returning `{approved}`; docs advise whole-command matching, path checks rejecting `..`, and warn git can run programs from repo config. A denial returns a message to the model.
- Status: GA
- Evidence: https://docs.cline.bot/sdk/guides/permission-handling.md (Permission Handling)
- Notes: Default-allow for SDK embedders. (verified in review)

### Scheduled and connector runs auto-approve
- What: Scheduled runs approve their own tool calls in every mode and default to `yolo`. Telegram connector without `--allowed-user-id` or `--hook-command` auto-approves all messages; Discord reachable by anyone unless a hook is set.
- Status: GA
- Evidence: https://docs.cline.bot/cli/scheduling.md (Create options); https://docs.cline.bot/cli/connectors.md (Security)
- Notes: (verified in review)

### Plan mode is not a security boundary
- What: In subagents and scheduled runs, plan mode removes edit tools and blocks commands that commonly mutate (`rm`, `git commit`) but other commands (e.g. `python -c`) can still write.
- Status: GA
- Evidence: https://docs.cline.bot/features/subagents.md; https://docs.cline.bot/cli/scheduling.md
- Notes: (verified in review)

### Sandbox mode (CLI)
- What: `CLINE_SANDBOX` and `CLINE_SANDBOX_DATA_DIR` enable a sandbox for session storage; `hub dashboard --data-dir` enables sandbox. Purpose not otherwise described.
- Status: unclear
- Evidence: https://docs.cline.bot/cli/cli-reference.md (Environment Variables, hub dashboard)
- Notes: Docs do not say whether this is a process/OS sandbox or only data isolation. No OS-level command sandbox documented. (verified in review)

### `.clineignore` (deprecating) and enforcement hook
- What: `.gitignore`-syntax file filtering listings, auto context and search; explicit `@` mentions and shell bypass it. Docs call it not a security boundary and mark it deprecating. Replacement is a PreToolUse hook (guards `read_files`, `editor`, `apply_patch`, `run_commands`; token-heuristic shell check; symlinks not resolved) or the "Block Ignored File Access" plugin.
- Status: deprecated (soon)
- Evidence: https://docs.cline.bot/customization/clineignore.md; https://docs.cline.bot/resources/deprecations.md
- Notes: deprecations page gives no dates; replacement plugin uses workspace .gitignore and does not block shell commands. (verified in review)

### 6. Context management and compaction

### Auto Compact
- What: When near the model limit, Cline summarizes the whole conversation, replaces history with the summary, and continues. Shown as a tool call; reuses the prompt cache. With models lacking support it falls back to rule-based truncation. Checkpoints can restore pre-summary state.
- Status: GA
- Evidence: https://docs.cline.bot/features/auto-compact
- Notes: No threshold documented. The page was reached by a link in the Memory Bank page and is not in the llms.txt index I fetched (possibly stale/legacy).

### CLI compaction modes
- What: `--compaction agentic|basic|off`, default agentic.
- Status: GA
- Evidence: https://docs.cline.bot/cli/cli-reference.md (Global Flags)
- Notes: Semantics of `basic` vs `agentic` not documented. (verified in review)

### /smol (/compact) and /newtask
- What: `/smol` (alias `/compact`) compresses the current conversation in place. `/newtask` starts a fresh context with a packaged hand-off (plan, work done, relevant files, next steps), omitting tool-call noise.
- Status: GA
- Evidence: https://docs.cline.bot/core-workflows/using-commands.md (Using Commands)

### 7. Memory and persistent instructions

### Rules (.clinerules / .cline/rules)
- What: Markdown rule files, workspace (`.clinerules/` or `.cline/rules/`) and global (`~/Documents/Cline/Rules`, `~/.cline/rules`, `~/Cline/Rules`); also auto-detects `.cursorrules`, `.windsurfrules`, `AGENTS.md`, `~/.agents/AGENTS.md`. Per-rule toggles. Workspace wins on conflict. Created via UI or `/newrule`.
- Status: GA
- Evidence: https://docs.cline.bot/customization/cline-rules.md
- Notes: Config page does not define override order (https://docs.cline.bot/getting-started/config.md). Rules in VS Code, Desktop, CLI.

### Conditional rules
- What: YAML frontmatter `paths:` globs; rule loads only when matching files are in context (mentioned in message, open tabs, visible panes, files touched or about to be edited). Invalid YAML fails open (rule active). `paths: []` disables.
- Status: GA
- Evidence: https://docs.cline.bot/customization/cline-rules.md (Conditional Rules)

### Memory Bank (convention only)
- What: Not a built-in feature; a custom-instructions template using a `memory-bank/` directory of six markdown files (projectbrief, productContext, activeContext, systemPatterns, techContext, progress) read at the start of each task.
- Status: GA as a documented convention
- Evidence: https://docs.cline.bot/best-practices/memory-bank.md

### Skills
- What: Directory with `SKILL.md` (frontmatter `name` equal to dir name, `description` up to 1024 chars). Progressive loading: metadata always (~100 tokens each), body on trigger (<5k tokens), resources on demand; scripts' output only enters context. Triggered automatically by the `skills` tool or by `/skill-name`. Project locations: `.cline/skills/`, `.clinerules/skills/`, `.claude/skills/`; global `~/.cline/skills/`. Global wins over project on name clash. `cline skill` wraps `npx skills`.
- Status: GA
- Evidence: https://docs.cline.bot/customization/skills.md; https://docs.cline.bot/cli/cli-reference.md
- Notes: Skills are not supported in remote config for CLI/Desktop (remote-config table). (verified in review: skills page; precedence global-over-project, 1024 chars, ~100 tokens, <5k body)

### Configuration directory layout
- What: Global `~/.cline/` (`data/settings/{providers,global-settings,cline_mcp_settings}.json`, `data/sessions`, `data/db/*.db`, `data/logs`, `data/teams`; `rules workflows hooks skills agents plugins cron`) and project `.cline/`. Env: `CLINE_DIR`, `CLINE_DATA_DIR`, etc.
- Status: GA
- Evidence: https://docs.cline.bot/getting-started/config.md; https://docs.cline.bot/cli/cli-reference.md
- Notes: Docs say rules/hooks/skills/plugins must come from trusted sources since hooks and plugins execute code.

### 8. Planning, tasks and modes

### Plan and Act modes
- What: Plan can read and search but cannot modify files or run commands; Act can do both. History carries over. Optional separate model per mode (setting). Doc recommends planning first. Toggle in IDE panel, `Tab` in TUI, `-p/--plan` in CLI.
- Status: GA
- Evidence: https://docs.cline.bot/core-workflows/plan-and-act.md; https://docs.cline.bot/usage/tui.md
- Notes: Plan mode "cannot execute commands" on the Plan & Act page, yet the subagent/scheduling pages say a shell-mutation blocklist only; the CLI Overview says plan mode works headless. Treat plan mode as prompt/tool-gated, not a sandbox.

### /deep-planning
- What: Four steps: silent codebase investigation, clarifying questions, writes `implementation_plan.md`, then creates a new task with trackable steps. Prompt tuned per model family.
- Status: GA
- Evidence: https://docs.cline.bot/core-workflows/using-commands.md; https://docs.cline.bot/core-workflows/plan-and-act.md

### Focus Chain (VS Code todo list)
- What: Task todo-list feature; now deprecated with no replacement; the new harness is said to manage execution and context without it.
- Status: deprecated
- Evidence: https://docs.cline.bot/resources/deprecations.md
- Notes: The Auto Compact and Plan & Act pages still mention todo lists ("ask Cline to create a todo list"), possibly stale.

### Workflows
- What: Workflows directories exist (`.cline/workflows/`, `~/.cline/workflows/`, `~/Documents/Cline/Workflows`) and remote config can push "rules and workflows" (VS Code, JetBrains, CLI). No workflow doc page in llms.txt nav; `cline config workflows` lists them.
- Status: unclear
- Evidence: https://docs.cline.bot/getting-started/config.md; https://docs.cline.bot/enterprise-solutions/configuration/remote-configuration/overview.md; https://docs.cline.bot/cli/cli-reference.md
- Notes: How workflows are authored/invoked is not documented in any page I opened.

### 9. Sub-agents and multi-agent orchestration

### Subagents (`spawn_agent`)
- What: Spawns a separate agent with its own instructions and context window for a focused task; only its final reply returns. Parallel; nested allowed; no MCP tools; approving the `spawn_agent` call approves everything the subagent does (normal approval settings do not apply inside). On by default. CLI and Desktop only, not VS Code/JetBrains. Cost added to session.
- Status: GA
- Evidence: https://docs.cline.bot/features/subagents.md
- Notes: No documented caps on count, depth, timeout. (verified in review: approving spawn_agent approves everything the subagent then does; approval settings do not apply inside; CLI and Desktop only; no MCP tools)

### Agent teams
- What: `/team` prefix (or model's own decision) creates a coordinator plus teammates sharing a task board, mailbox and mission log in `~/.cline/data/db/teams.db`, keyed by session; resumes with `cline --id`. Not with `--yolo`; SDK/CLI/Kanban only. SDK enables via `enableAgentTeams`, `enableSpawnAgent`, `teamName`. Docs say a single agent is often more efficient.
- Status: GA/limited (docs: "currently only applies" to SDK, CLI, Kanban)
- Evidence: https://docs.cline.bot/cli/agent-teams.md; https://docs.cline.bot/sdk/guides/multi-agent-teams.md

### Kanban parallel agent board
- What: Board of cards, each running a CLI agent (Cline CLI, Claude Code, Codex, OpenCode named) in its own ephemeral git worktree and terminal; gitignored dirs like `node_modules` are symlinked from the main repo (so agent edits to them hit the main copy). Card links form dependency chains that auto-start the next task when one is trashed. Review shows diffs against base, checkpoint-scoped, with inline line comments fed back to the agent. Ship via Commit or Open PR (run as an agent prompt that also resolves conflicts). Trashing removes the worktree; resume ID retained. Sidebar chat agent can create/link/start cards.
- Status: preview (research preview)
- Evidence: https://docs.cline.bot/usage/kanban.md; https://docs.cline.bot/kanban/core-workflow.md
- Notes: Overview mentions auto-commit/auto-PR settings; core-workflow says auto-PR is not described. Mild contradiction.

### 10. Background, long-running and scheduled work

### Scheduling (cron)
- What: `cline schedule create "<name>" --cron --prompt --workspace` (5-field, local time); manage with list/get/upcoming/active/trigger/pause/resume/update/history/stats/delete, export/import. `--mode act|plan|yolo` (default yolo), `--timeout`, `--max-parallel` (default 1). Needs the hub (auto-started). Results can be posted to a chat via a running connector. SDK/CLI/Kanban only; Desktop has its own schedule UI.
- Status: GA
- Evidence: https://docs.cline.bot/cli/scheduling.md; https://docs.cline.bot/usage/cline-desktop.md (Scheduling); https://docs.cline.bot/sdk/guides/scheduled-agents.md (listed, not opened)
- Notes: Behavior when hub is stopped after creation not documented. Default mode yolo, --max-parallel default 1 verified in review.

### Zen / background hub runs
- What: `-z, --zen` runs a task in the background hub; `hub upgrade` explicitly does not wait for zen or scheduled tasks.
- Status: unclear
- Evidence: https://docs.cline.bot/cli/cli-reference.md
- Notes: Only a flag description is given.

### Cloud Agents
- What: Browser product (app.cline.bot, Agents tab): pick GitHub repo, branch, model; agent works in a per-session cloud sandbox with its own repo copy, on a `cline/` branch, never the default branch; commits and pushes as it goes; hands back a PR. Sandbox pauses after 15 min idle and resumes. Limits: 10 concurrent, 50 new sessions per day. Rules, skills, MCP and connectors not loaded in cloud sessions yet. Plan/Act, approvals/auto-approve, image input, follow-ups. No triggers (schedules, webhooks) documented. GitHub only.
- Status: preview/rolling out (release notes: "rolling out gradually", v4.1.25)
- Evidence: https://docs.cline.bot/usage/cloud-agents.md; https://github.com/cline/cline/releases
- Notes: v4.1.26 adds Local/Cloud toggle and sandbox progress card. (verified in review: 15 min idle, 10 concurrent, 50 per day, `cline/` branch, rules/skills/MCP/connectors not loaded; page does not say other Git hosts are unsupported, only describes GitHub)

### Connectors (chat bots)
- What: `cline connect` for Telegram, Slack (webhook or socket), Discord, Google Chat, WhatsApp, Linear. Each message creates/continues an agent session (Slack thread = session; Discord steers a running task). Hook script (stdin JSON, returns allow/deny) can gate actors. Discord commands include `/yolo`, `/tools`, `/cwd`, `/schedule`, `/abort`. Requires the hub. CLI only.
- Status: GA (CLI)
- Evidence: https://docs.cline.bot/cli/connectors.md; https://docs.cline.bot/cli/cli-reference.md
- Notes: (verified in review: connector list, allow/deny hook JSON, Telegram auto-approve without flags)

### 11. Sessions: persistence, resume, fork, checkpoints, undo

### Checkpoints
- What: A shadow git repo, separate from the project's, commits the full file state (including untracked files) after each tool use. Compare opens a diff; Restore offers Files only, Task only (drops later messages), or Files and Task. Editing an earlier message with "Restore All" reverts files then resubmits. On by default; toggle in Feature Settings; large repos may be slow/heavy.
- Status: GA (IDE); SDK lists checkpoints; Desktop/CLI parity unclear
- Evidence: https://docs.cline.bot/core-workflows/checkpoints.md; https://docs.cline.bot/usage/ide.md (names the three options slightly differently: Task and Workspace / Task Only / Workspace Only)
- Notes: Not documented: exclusions, size caps, pruning, disk location, handling of files created after the checkpoint, manual edits. Release notes mention SDK "checkpoint diffs by using one git process", consistent with the git approach. CLI `/undo` exists in TUI but is not explained. (verified in review: checkpoints page confirms shadow git repo, per-tool-use commits, three restore options, on by default, no exclusion/retention docs)

### Session storage and resume
- What: Sessions persist in `~/.cline/data/sessions/<id>/` with a SQLite index `data/db/sessions.db`; `cline history` (list, export, delete, update), `cline --id <id>` resume; ACP clients can restore threads; Desktop imports Claude Code and Codex conversations. CLI releases note attaching to a task already running in another Cline app.
- Status: GA
- Evidence: https://docs.cline.bot/cli/cli-reference.md; https://docs.cline.bot/sdk/architecture/hub-spoke.md; https://docs.cline.bot/usage/cline-desktop.md; https://github.com/cline/cline/releases
- Notes: Fork/branching of a conversation not documented.

### 12. Git and version control

### Git worktrees
- What: CLI `--worktree` runs in a detached git worktree under `~/.cline/worktrees/`; Kanban uses a worktree per card; Desktop lets you select a branch at session start.
- Status: GA (CLI), preview (Kanban)
- Evidence: https://docs.cline.bot/cli/cli-reference.md; https://docs.cline.bot/kanban/core-workflow.md; https://docs.cline.bot/usage/cline-desktop.md

### Commit messages and PRs
- What: VS Code SCM "Generate Commit Message". Cloud agents and Kanban open PRs. GitHub Actions samples: issue RCA on `@cline` comments (three-job split: authorize with read-only token, analyze with `--auto-approve true --json`, post with write-only issues token) and PR review samples.
- Status: GA / sample
- Evidence: https://docs.cline.bot/core-workflows/working-with-files.md; https://docs.cline.bot/cli/samples/github-integration.md; https://docs.cline.bot/usage/cloud-agents.md

### 13. Extensibility

### MCP
- What: Config `cline_mcp_settings.json` (`~/.cline/data/settings/`, override `CLINE_MCP_SETTINGS_PATH`); transports stdio, streamable HTTP (recommended), SSE (legacy; fallback if `type` omitted). Entries have `autoApprove` tool lists and `disabled`. CLI `cline mcp` wizard with OAuth authorize and `add/remove`; IDE has Configure and Remote Servers tabs. Subagents do not get MCP tools.
- Status: GA
- Evidence: https://docs.cline.bot/mcp/mcp-overview.md; https://docs.cline.bot/features/subagents.md
- Notes: Page does not describe OAuth/token storage, timeouts, or the `autoApprove` entry format.

### MCP marketplace
- What: Desktop has a Marketplace to browse/search skills, MCP servers and plugins with categories; enterprise `mcpMarketplaceEnabled` hides it and `allowedMCPServers` limits entries.
- Status: GA (Desktop; IDE per enterprise docs)
- Evidence: https://docs.cline.bot/usage/cline-desktop.md (Marketplace); https://docs.cline.bot/enterprise-solutions/configuration/infrastructure-configuration/control-other-cline-features/mcp-server-controls.md
- Notes: The MCP overview page says nothing about a marketplace. Desktop v0.0.47 notes a marketplace plugin bug (releases).

### Hooks
- What: Docs Hooks page is a stub pointing to SDK Plugins. SDK hooks: `beforeRun/afterRun/beforeModel/afterModel/beforeTool/afterTool/onEvent`; `beforeTool` can skip with a reason, rewrite input, force approval (`policy.autoApprove:false`), append context, or stop; `afterTool` can replace results. 3-second per-call timeout; `hookErrorMode` ignore (default) or throw; plugin becomes degraded then failed after 5 consecutive failures. CLI file hooks (`PreToolUse` script, `--hooks-dir`, `CLINE_HOOKS_DIR`; `cline hook` reads payload from stdin); VS Code needs "Enable Hooks" setting and `.clinerules/hooks/`.
- Status: GA
- Evidence: https://docs.cline.bot/customization/hooks.md; https://docs.cline.bot/sdk/plugins.md; https://docs.cline.bot/customization/clineignore.md; https://docs.cline.bot/cli/cli-reference.md
- Notes: File-hook event names beyond `PreToolUse` are not documented in pages I read.

### Plugins (SDK)
- What: `AgentPlugin` object (name, manifest capabilities, `setup`, hooks) adding tools and hooks. Installed via `cline plugin install` (npm, git, local, file URL), `config.extensions` or `pluginPaths`. Default runs in a per-session worker thread (crash isolation, explicitly not a security boundary); opt-in sandbox subprocess mode via `CLINE_PLUGIN_MODE=sandbox`. Release notes: SDK v0.0.94 removed `in_process` mode.
- Status: GA
- Evidence: https://docs.cline.bot/sdk/plugins.md; https://github.com/cline/cline/releases
- Notes: (verified in review: plugins page; the v0.0.94 release-note detail was not re-checked)

### Custom slash commands / skills as commands
- What: Built-ins: `/newtask`, `/smol`/`/compact`, `/newrule`, `/deep-planning`, `/reportbug`; enabled skills appear as `/skill-name`.
- Status: GA
- Evidence: https://docs.cline.bot/core-workflows/using-commands.md

### SDK
- What: `npm install @cline/sdk`, TypeScript, Node 22+; `Agent`/`ClineCore`, event streaming, cost in run results, custom tools, cron, subagents/teams, MCP, checkpoints. Also a "Cline API" (OpenAI-compatible chat completions at app.cline.bot).
- Status: GA (no maturity label; version 0.x per releases)
- Evidence: https://docs.cline.bot/sdk/overview.md; https://docs.cline.bot/getting-started/clinepass.md; https://github.com/cline/cline/releases

### ACP
- What: `cline --acp` over stdio; clients: Zed, JetBrains (`~/.jetbrains/acp.json`), Neovim (CodeCompanion, avante, agentic), Emacs agent-shell, others. Supports sign-in, Plan/Act, model picking, session resume, images, org switching. Default every edit/command goes through client permission UI; `--auto-approve true` launches pre-approved. Env `CLINE_API_KEY`, `CLINE_PROVIDER`, `CLINE_MODEL`.
- Status: GA
- Evidence: https://docs.cline.bot/usage/acp.md

### 14. Models, providers and inference configuration

### Providers
- What: Cline provider (300+ models in Desktop), ClinePass, Anthropic, OpenAI (Codex/subscription), Google, Bedrock (API key, IAM, CLI profile/SSO), Vertex, Azure, DeepSeek, Qwen, MiniMax, Z AI, Poolside, OpenRouter, OpenAI-compatible, "30+ other", Ollama/LM Studio local, LiteLLM (enterprise). CLI `-P/--provider`, `-m`, `-k`; `--thinking none..xhigh`.
- Status: GA
- Evidence: https://docs.cline.bot/llms.txt (nav); https://docs.cline.bot/usage/cline-desktop.md; https://docs.cline.bot/sdk/model-providers.md; https://docs.cline.bot/cli/cli-reference.md; https://docs.cline.bot/enterprise-solutions/overview.md
- Notes: Provider list taken from nav titles; I did not open each provider page.

### Model per mode, and multi-model orchestration
- What: Separate Plan/Act models. CLI samples route phases to different models using separate `--config` dirs (one per model) and `--thinking`; multi-model review consensus sample.
- Status: GA / sample
- Evidence: https://docs.cline.bot/core-workflows/plan-and-act.md; https://docs.cline.bot/cli/samples/model-orchestration.md

### ClinePass and free models
- What: $9.99/month subscription for open-weight coding models, "2-5x" usage of standard API rates (vendor-claim); limits tracked over 5-hour rolling, weekly and monthly windows; exact limits not published. Page lists reference per-token prices. Free models change over time (ClineFree).
- Status: GA
- Evidence: https://docs.cline.bot/getting-started/clinepass.md; https://docs.cline.bot/cline-overview.md; https://docs.cline.bot/usage/cloud-agents.md
- Notes: The model names on this page (e.g. DeepSeek V4.1, Kimi K3.1, Qwen3.8) were taken as shown; not verified elsewhere.

### Local models
- What: Desktop Settings, Provider, Local: Ollama or LM Studio; Cline can install/start them, recommend models for hardware, download, and show a memory indicator. v4.1.25: local requests no longer fail after 5 min silence.
- Status: GA (Desktop)
- Evidence: https://docs.cline.bot/usage/cline-desktop.md; https://github.com/cline/cline/releases

### 15. UI surfaces

### Surfaces
- What: VS Code panel (Plan/Act toggle, diffs with Approve/Reject, checkpoints), JetBrains tool window, Cursor/Windsurf/Antigravity/Zed per overview; TUI (`cline -i`: Tab plan/act, Shift+Tab auto-approve all, status area with model, context, cost, branch, diff stats; slash commands `/settings /model /account /mcp /compact /undo /clear /history`); Desktop (parallel sessions, Changes button with per-turn and session diffs, project files panel, integrated terminal in v0.0.49, voice input via e.g. ElevenLabs, web search toggle, Customize inventory); Kanban web board; Cline hub `dashboard` (default port 8787).
- Status: GA; Kanban preview; Desktop Windows beta
- Evidence: https://docs.cline.bot/usage/ide.md; https://docs.cline.bot/usage/tui.md; https://docs.cline.bot/usage/cline-desktop.md; https://docs.cline.bot/cli/cli-reference.md; https://github.com/cline/cline/releases
- Notes: TUI page does not explain what its slash commands do.

### 16. Browser automation and computer use

### Browser tool
- What: Nothing documented in the current docs. The auto-approve page lists a "Use the browser" category, the overview says Cline can "use a browser", and the ClineCore tool list has only `fetch_web_content`. A search result points to a legacy page `docs.cline.bot/exploring-clines-tools/remote-browser-support`, which now returns the tool reference instead.
- Status: unclear
- Evidence: https://docs.cline.bot/features/auto-approve.md; https://docs.cline.bot/tools-reference/all-cline-tools.md; https://docs.cline.bot/exploring-clines-tools/remote-browser-support (redirect/stub)
- Notes: See Unverified. Computer use: nothing documented.

### Web search / fetch
- What: `fetch_web_content` tool; Desktop web search is on by default and only honored by providers with built-in web search (Cline, ClinePass, OpenAI subscription, Anthropic, Gemini).
- Status: GA
- Evidence: https://docs.cline.bot/tools-reference/all-cline-tools.md; https://docs.cline.bot/usage/cline-desktop.md

### 17. Remote, cloud, headless, CI and distributed execution

### Headless CLI
- What: Headless auto-engages with `--json`, piped stdin, or stdout redirect. `--json` emits newline-delimited JSON with `type` and `ts` (`agent_event`, `hook_event`, `team_event`, `run_result`, `run_aborted`; errors on stderr). `-t` timeout (default none), `--retries`, `--thinking`. Exit codes not documented. Images via `@path`.
- Status: GA
- Evidence: https://docs.cline.bot/usage/cli-overview.md; https://docs.cline.bot/cli/cli-reference.md

### Multi-instance and isolation
- What: Isolation by `--config <dir>` / `CLINE_DIR` and `--data-dir` / `CLINE_DATA_DIR`; parallel runs are separate processes, each needing its own output file. One hub per machine shared by CLI sessions, connectors, schedules. Release notes: CLI/Desktop can attach to a task already running in another Cline app.
- Status: GA
- Evidence: https://docs.cline.bot/cli/samples/model-orchestration.md; https://docs.cline.bot/cli/cli-reference.md; https://github.com/cline/cline/releases
- Notes: No dedicated multi-instance guide; CLI Overview explicitly does not cover it.

### SSH remote development (Desktop)
- What: App stays local; agent, tools, files, MCP servers and session history run on the remote host. On first connect Cline uploads a ~30 MB self-contained helper to `~/.cline/remote/`, starts a hub bound to the host loopback, and reaches it via `ssh -L` to a random local port. Key-based login only, host key must already be trusted, one remote host at a time, strict host-key checking, non-interactive ssh. Provider key is sent to the host over the tunnel for the session and not written to host settings. Chat, approvals and provider settings stay on the local machine. Linux x64/arm64 and macOS hosts. Tunnel closes after ~45 s of network loss. SDK v0.2.0 adds ProxyJump alias support.
- Status: GA (as documented)
- Evidence: https://docs.cline.bot/usage/cline-desktop-ssh.md; https://github.com/cline/cline/releases
- Notes: Directly relevant topology: client UI local, execution remote via a hub. (verified in review: ~30 MB is the Linux helper size, cached per Cline version; macOS hosts only when Desktop runs on a Mac; ProxyJump in SDK v0.2.0 confirmed on releases page)

### Kanban remote access
- What: Binds `127.0.0.1:3484`; `--host 0.0.0.0` or `KANBAN_RUNTIME_HOST` exposes it. No built-in authentication documented; docs rely on Tailscale, SSH tunnel, ngrok, Cloudflare tunnel. Docs warn it gives full git and terminal access. AWS CDK example runs as root with token in user-data.
- Status: preview
- Evidence: https://docs.cline.bot/kanban/remote-access.md
- Notes: Weak security story; the doc's own example has issues the page partly flags. (corrected in review: the page does not flag the CDK example's issues; the root service and token-in-user-data are inferred from the script, not stated. Binding, port, no-auth and warnings verified)

### Hub dashboard
- What: `cline hub dashboard` (host default 127.0.0.1, port 8787, `--public-url`, `--room-secret` required for non-local hosts, `--no-open`).
- Status: unclear
- Evidence: https://docs.cline.bot/cli/cli-reference.md (hub dashboard)
- Notes: No page describes what the dashboard shows.

### 18. Collaboration, teams and enterprise administration

### Cline Enterprise
- What: SSO, role hierarchy Member/Admin/Owner, per-team model/tool access, remote configuration pushed from an admin console (app.cline.bot), selective audit logging of admin operations, cost breakdown by team/model, OpenTelemetry export (Datadog, Grafana, Splunk named). Bring-your-own inference (Bedrock, Vertex, Azure OpenAI, Anthropic, OpenAI, Cerebras, any OpenAI-compatible, LiteLLM); claims no markup. No codebase indexing. Agent and tools run on developer machine or chosen remote host.
- Status: GA
- Evidence: https://docs.cline.bot/enterprise-solutions/overview.md; https://docs.cline.bot/enterprise-solutions/configuration/remote-configuration/overview.md
- Notes: No tiers, pricing or certifications given. Audit log contents/retention not specified.

### Remote configuration coverage per surface
- What: VS Code/JetBrains get rules, workflows, skills, allowed providers, org lock-in, MCP controls, telemetry lock. CLI only gets rules and workflows. Desktop gets none. Remote config is written locally to `.cline/enterprise/` or `.cline/remote-config/` and may contain prompt-storage keys (advice: gitignore).
- Status: GA
- Evidence: https://docs.cline.bot/enterprise-solutions/configuration/remote-configuration/overview.md
- Notes: Enterprise policy is therefore not enforced on CLI/Desktop, which can pick other providers and MCP servers. (verified in review: table matches; "Allow Kanban" is also No everywhere; remote config files go to `.cline/remote-config/` for VS Code/JetBrains and `.cline/enterprise/` for CLI; Desktop location not stated)

### Enterprise controls: YOLO and MCP
- What: `yoloModeAllowed:false` disables the toggle org-wide; telemetry records YOLO toggling and per-task YOLO use. MCP: `mcpMarketplaceEnabled`, `allowedMCPServers` (by id), `remoteMCPServers` (name, url, `alwaysEnabled`, headers), `blockPersonalRemoteMCPServers`.
- Status: GA
- Evidence: https://docs.cline.bot/enterprise-solutions/configuration/infrastructure-configuration/control-other-cline-features/yolo-mode.md; https://docs.cline.bot/enterprise-solutions/configuration/infrastructure-configuration/control-other-cline-features/mcp-server-controls.md
- Notes: (verified in review) Contradiction: the YOLO page says the policy applies "in all interfaces", while the remote-config support table lists YOLO mode as "No" for all surfaces. Allowlist interaction with pushed remote servers undocumented.

### 19. Observability: usage and cost, logs, tracing, debugging

### Cost and usage display
- What: TUI status area shows model, context usage, cost; SDK run results carry input/output tokens and `totalCost`; model catalog carries pricing (input/output/cache read/cache write).
- Status: GA
- Evidence: https://docs.cline.bot/usage/tui.md; https://docs.cline.bot/sdk/model-providers.md

### Logs and diagnostics
- What: `cline doctor` (`fix` kills all Cline processes; `log` opens log), `~/.cline/data/logs/cline.log` and `hub-daemon.log`, `CLINE_LOG_PATH`, `/reportbug` collects config/errors/system info.
- Status: GA
- Evidence: https://docs.cline.bot/cli/cli-reference.md; https://docs.cline.bot/core-workflows/using-commands.md

### OpenTelemetry and prompt storage (enterprise)
- What: OTel export of metrics/logs to your platform. Prompt Storage (off unless configured) syncs each task's `api_conversation_history.json` (full tool inputs/outputs, so contains code) to your own S3 or R2 bucket; local copy at `~/.cline/data/tasks/<taskId>/` is source of truth; queue defaults `maxRetries` 5, `maxQueueSize` 1000, `maxFailedAgeMs` 7 days; no built-in erasure.
- Status: GA
- Evidence: https://docs.cline.bot/enterprise-solutions/monitoring/prompt-storage.md; https://docs.cline.bot/enterprise-solutions/monitoring/telemetry.md
- Notes: Prompt storage page names `~/.cline/data/tasks/` while the CLI config layout names `data/sessions/`: possibly different (extension vs SDK) storage layouts.

### 20. Configuration and customisation

### Config scopes and env vars
- What: See area 7 entry; CLI `cline config [workflows|rules|skills|agents|plugins|hooks|mcp|tools]` views/edits. `agents/` directory holds custom agent definitions (page does not describe the format).
- Status: GA
- Evidence: https://docs.cline.bot/getting-started/config.md; https://docs.cline.bot/cli/cli-reference.md
- Notes: Precedence between project and global is undefined on the config page; skills say global wins, rules say workspace wins.

### Multi-environment CLI flags
- What: `--system` override prompt, `--hooks-dir`, `--cwd`, `--worktree`, `--thinking`, `--compaction`.
- Status: GA
- Evidence: https://docs.cline.bot/cli/cli-reference.md

### 21. Security and privacy

### Telemetry
- What: On by default in all products, even with own API keys. Collects feature use, task start/finish, Plan/Act switches, errors, token counts, timings, model/provider names, version/OS/editor, device id, plus account/org id when signed in. Excludes code, file contents and prompts, except requests to Cline and ClinePass providers which can include prompt and response. Opt-out: IDE settings checkbox, Desktop toggle, CLI `telemetryOptOut` in `global-settings.json` (full effect needs `cline hub stop`). Still contacts Cline servers for sign-in, feature flags and Cline/ClinePass requests. Admins can force telemetry off in VS Code/JetBrains only.
- Status: GA
- Evidence: https://docs.cline.bot/enterprise-solutions/monitoring/telemetry.md
- Notes: (verified in review) Telemetry endpoint and retention not named. VS Code still sends some required events (e.g. the opt-out event) while editor telemetry is on.

### Secrets and credentials
- What: `providers.json` stores keys under `~/.cline/data/settings/`; CLI `-k` saves the key for the provider; SSH never stores passwords or key contents (host settings file owner-only).
- Status: GA
- Evidence: https://docs.cline.bot/cli/cli-reference.md; https://docs.cline.bot/usage/cline-desktop-ssh.md
- Notes: Whether `providers.json` is encrypted is nothing documented.

### 22. Licence, pricing and distribution

- What: Apache-2.0. VS Code Marketplace, npm `cline`, `@cline/sdk`, Desktop installers. Pricing: Cline (usage-billing) pay-as-you-go credits, ClinePass $9.99/month, BYOK free; enterprise by contact.
- Status: GA
- Evidence: https://github.com/cline/cline; https://docs.cline.bot/cline-overview.md; https://docs.cline.bot/getting-started/clinepass.md
- Notes: SDK docs page itself states no licence.

### 23. Anything else

### Cline-as-a-service API
- What: OpenAI-compatible chat-completions API at app.cline.bot using Cline API keys.
- Status: GA
- Evidence: https://docs.cline.bot/getting-started/clinepass.md (setup outside Cline); https://docs.cline.bot/llms.txt (Cline API section, pages not opened)

### Import from other agents
- What: Desktop imports Claude Code and Codex conversation history as context.
- Status: GA
- Evidence: https://docs.cline.bot/usage/cline-desktop.md

### Cross-tool compatibility
- What: Reads `.cursorrules`, `.windsurfrules`, `AGENTS.md`, `.claude/skills/`, `.agents/skills/`.
- Status: GA
- Evidence: https://docs.cline.bot/customization/cline-rules.md; https://docs.cline.bot/customization/skills.md; https://docs.cline.bot/getting-started/config.md

## 3. Distinctive or unusual

- Hub/spoke with capability brokering: the hub routes a request (diff, shell) to whichever attached client advertised it; sessions outlive clients and several UIs can attach. Close to a master/node shape.
- SSH remote by uploading a self-contained helper and tunneling a loopback hub; provider credential sent per session, never written on the host.
- Shadow-git checkpoints with three restore combinations including "task only" (rewind chat, keep files) and edit-message-and-restore.
- Model-judged `requires_approval` per command instead of a rule list; contrast: hooks/plugins as the enforcement layer.
- Skills with three-level progressive loading and "script output only" context cost.
- Conditional rules by file-path glob with fail-open behavior and activation notices.
- Messaging connectors (Telegram, Slack, Discord, WhatsApp, Linear, Google Chat) as first-class agent front-ends, with a hook protocol for allow/deny.
- Kanban of CLI agents (including competitors Claude Code, Codex, OpenCode) in per-card worktrees with symlinked gitignored dirs.
- Prompt Storage to customer-owned S3/R2 as the enterprise audit approach.
- Model-based edit tool routing (`apply_patch` vs `editor`).

## 4. Architecture and UX notes

- Single TypeScript codebase (SDK) powers CLI, Desktop, VS Code and JetBrains; the loop runs in a spoke worker, owned by a local hub daemon (WebSocket, loopback by default), never in the UI client.
- Storage is local: per-session folders plus SQLite (`sessions.db`, `cron.db`, `teams.db`). Hub logs separately.
- Remote execution is two ways: SSH helper (Desktop) and cloud sandboxes (Cline Cloud, GitHub-bound). Kanban is a separate local web server (3484) with no auth.
- Plugins run in worker threads (not a security boundary). Hooks have a 3 s timeout.
- Desktop is Tauri plus a Bun sidecar (README).
- The IDE extension and the SDK-based surfaces differ in features (subagents, teams, scheduling, custom tools, connectors are not in VS Code/JetBrains), so the docs repeatedly note per-surface support.

## 5. Relevance to a self-hosted master/node harness

- Hub/spoke split (area 1): same separation of coordinator and executor; Cline's hub does not run the loop, whereas the owner's invariant puts the loop on the node. Note Cline leaves auth and remote hub config undocumented, a gap to avoid.
- Capability brokering (hub entry): nodes could advertise capabilities and the master route requests, relevant to multi-client sessions.
- SSH helper and ProxyJump handling (area 17): the owner already cut SSH node setup; Cline's design (upload helper, loopback hub, forwarded port, strict host keys) is a concrete reference and its limitations (one host, key only) are known.
- Checkpoint model (area 11): shadow repo per task with restore variants; costs on big repos are documented complaints; no exclusion/pruning story is a gap to improve.
- Approval model (area 5): model-judged safety plus a default-on CLI auto-approve and default-allow SDK policies are weak defaults; hooks as `beforeTool` policy override is a good extensibility shape (skip, rewrite input, force approval).
- Subagent approval inheritance (area 9): approving `spawn_agent` approves everything underneath; a stricter design should carry policy into children.
- Connectors with an allow/deny hook (area 10): chat front-ends are risky; Telegram default is approve-everything. Good test case for a node-side policy that the protocol cannot widen.
- Remote config not covering CLI/Desktop (area 18): enterprise policy being per-surface is a gap; a node-enforced `node.toml` avoids it.
- Skills/rules loading cost (area 7): progressive loading numbers (~100 tokens per skill, <5k body) are useful targets.
- Telemetry (area 21): opt-out default, device id, prompts excluded except their own provider; the owner's "no secret in URL/log" stance is stricter.

## 6. Overhyped, stale or contradictory

- Overview says "Every action requires your explicit approval", but CLI defaults to auto-approve true, schedules default to yolo, SDK default-allows.
- YOLO enterprise control: page says all interfaces; remote-config matrix says no surface supports it.
- Docs call Plan mode unable to run commands, while subagent/scheduling pages say it only blocks mutating commands and not python -c style writes.
- Flag naming: `--auto-approve` (CLI reference) vs `--yolo` (agent-teams, clineignore pages); Plan/Act pages still mention todo lists though Focus Chain is deprecated.
- Checkpoint restore labels differ between the checkpoints page and the IDE page.
- Kanban overview mentions auto-commit/auto-PR settings; core-workflow says auto-PR not described. Kanban remote access has no auth.
- `.clineignore` described as saving 200k+ to under 50k tokens (vendor-claim); it is not a security boundary and is being deprecated.
- ClinePass "2-5x usage" is `vendor-claim`; no absolute limits published.
- Hooks docs page is a stub; docs reference old tool names; Auto Compact and Memory Bank pages look legacy.
- Release page shows no year; Cloud sessions "rolling out gradually".

## 7. Unverified

- Browser tool (Puppeteer/remote Chrome, screenshot loop): no current docs page found; legacy page appears removed. A search result mentioned a remote browser page only via third-party mirror. Settle by checking the extension changelog or the repo docs folder.
- MCP marketplace details (hosting, vetting) in the IDE: only enterprise config pages and Desktop mention it.
- Workflows authoring format and invocation.
- Focus chain mechanics (deprecated; not opened beyond the deprecation page).
- Checkpoint storage location, exclusions and retention.
- Pages not opened: Cline Free Models, Authorization, Installing, local models, individual provider pages, multiroot, SDK reference pages (Gateway, Tools API, Events, Types), SDK plugin guides, Writing Plugins, OpenTelemetry events/env, Enterprise API reference, Cline API pages, SSO setup, Kanban install detail, CLI samples other than GitHub integration and model orchestration, Poolside and others, ClineCore reference.
- Hub authentication and remote-mode endpoint configuration.
- Number of GA vs limited behaviors of Desktop on Linux (SSH page says local Desktop on Linux is supported, overview says macOS and Windows).

## 8. Sources

All fetched via WebFetch on 2026-11-10 (pages are .md exports unless noted).

- https://docs.cline.bot/llms.txt (docs index; navigation and page titles)
- https://github.com/cline/cline/releases (release notes, versions, dates)
- https://github.com/cline/cline (README: licence, structure, stars)
- https://docs.cline.bot/cline-overview.md (Cline Overview; surfaces, pricing claims)
- https://docs.cline.bot/features/auto-approve.md (Auto Approve & YOLO Mode)
- https://docs.cline.bot/core-workflows/checkpoints.md (Checkpoints)
- https://docs.cline.bot/tools-reference/all-cline-tools.md (Tools)
- https://docs.cline.bot/core-workflows/plan-and-act.md (Plan & Act Mode)
- https://docs.cline.bot/usage/cli-overview.md (CLI Overview)
- https://docs.cline.bot/cli/cli-reference.md (CLI Reference)
- https://docs.cline.bot/usage/cloud-agents.md (Cline Cloud Agents)
- https://docs.cline.bot/customization/cline-rules.md (Rules)
- https://docs.cline.bot/customization/hooks.md (Hooks stub)
- https://docs.cline.bot/customization/skills.md (Skills)
- https://docs.cline.bot/features/subagents.md (Subagents)
- https://docs.cline.bot/mcp/mcp-overview.md (MCP)
- https://docs.cline.bot/core-workflows/using-commands.md (Using Commands)
- https://docs.cline.bot/best-practices/memory-bank.md (Memory Bank)
- https://docs.cline.bot/sdk/architecture/hub-spoke.md (Hub-Spoke Architecture)
- https://docs.cline.bot/features/auto-compact (Auto Compact)
- https://docs.cline.bot/sdk/plugins.md (Plugins Overview; hooks)
- https://docs.cline.bot/usage/kanban.md (Kanban)
- https://docs.cline.bot/kanban/remote-access.md (Kanban Remote Access)
- https://docs.cline.bot/kanban/core-workflow.md (Kanban Core Workflow)
- https://docs.cline.bot/usage/cline-desktop.md (Cline Desktop)
- https://docs.cline.bot/usage/cline-desktop-ssh.md (Remote Development over SSH)
- https://docs.cline.bot/usage/ide.md (IDE)
- https://docs.cline.bot/enterprise-solutions/overview.md (Cline Enterprise)
- https://docs.cline.bot/resources/deprecations.md (Deprecations)
- https://docs.cline.bot/enterprise-solutions/monitoring/telemetry.md (Cline Telemetry)
- https://docs.cline.bot/enterprise-solutions/configuration/infrastructure-configuration/control-other-cline-features/yolo-mode.md (YOLO Mode, enterprise)
- https://docs.cline.bot/enterprise-solutions/configuration/infrastructure-configuration/control-other-cline-features/mcp-server-controls.md (MCP Server Controls)
- https://docs.cline.bot/enterprise-solutions/monitoring/prompt-storage.md (Prompt Storage)
- https://docs.cline.bot/enterprise-solutions/configuration/remote-configuration/overview.md (Remote Configuration Overview)
- https://docs.cline.bot/sdk/overview.md (Cline SDK)
- https://docs.cline.bot/sdk/guides/permission-handling.md (Permission Handling)
- https://docs.cline.bot/sdk/architecture/overview.md (Packages)
- https://docs.cline.bot/sdk/guides/multi-agent-teams.md (Multi-Agent Teams)
- https://docs.cline.bot/sdk/model-providers.md (Model Providers)
- https://docs.cline.bot/cli/agent-teams.md (Agent Teams)
- https://docs.cline.bot/cli/scheduling.md (Scheduling)
- https://docs.cline.bot/cli/connectors.md (Connectors)
- https://docs.cline.bot/usage/acp.md (ACP)
- https://docs.cline.bot/usage/tui.md (TUI)
- https://docs.cline.bot/core-workflows/working-with-files.md (Adding Context)
- https://docs.cline.bot/getting-started/config.md (Config)
- https://docs.cline.bot/customization/clineignore.md (.clineignore)
- https://docs.cline.bot/cli/samples/github-integration.md (GitHub Actions Integration)
- https://docs.cline.bot/cli/samples/model-orchestration.md (Model Orchestration)
- https://docs.cline.bot/getting-started/clinepass.md (ClinePass)
- https://docs.cline.bot/features/jupyter-notebooks.md (Jupyter Notebooks)
- https://docs.cline.bot/exploring-clines-tools/remote-browser-support (legacy URL; returned the tools reference)
- Web search for browser tool (only used to find that legacy URL; third-party mirrors not relied on)

## 9. Review

Reviewed 2026-11-10 by an independent reviewer (public pages only). Checked 25 entries out of about 75 by opening the cited docs.cline.bot pages and the GitHub releases page and asking for the specific fact.

| Entry | Verdict | Reason |
|---|---|---|
| Hub / spoke / client model | confirmed-with-correction | hub does not run loop, port 25463, four backend modes, no auth documented all confirmed; roles are per session participant, not per client |
| SSH remote development (Desktop) | confirmed | helper ~30 MB (Linux), `~/.cline/remote/`, loopback hub, `ssh -L`, key-only, one host, 45 s, provider key sent not stored |
| Shell command execution and approval flag | confirmed | model sets `requires_approval`; no fixed allowlist |
| Per-category auto-approve / YOLO | confirmed | categories and no-confirmation YOLO as stated |
| CLI auto-approve default ON | confirmed | `--auto-approve` default true, false in ACP |
| SDK tool policies | confirmed | unlisted tools enabled and auto-approved |
| Scheduled and connector auto-approve | confirmed | yolo default, every mode self-approves, Telegram without flags approves all |
| Plan mode not a security boundary | confirmed | `python -c` example on subagents and scheduling pages |
| Sandbox mode (CLI) | confirmed | only "enable sandbox mode" wording |
| `.clineignore` deprecation | confirmed | no dates given |
| CLI compaction modes / retries | confirmed | defaults agentic, 3; 4.1.26 note says 5 to 3 |
| Skills | confirmed | locations, 1024, token figures, global wins |
| Subagents (`spawn_agent`) | confirmed | approving the call approves everything underneath |
| Checkpoints | confirmed | shadow git, per tool use, three restore options |
| Cloud Agents | confirmed | 15 min, 10 concurrent, 50/day |
| Connectors | confirmed | six connectors, JSON allow/deny hook |
| Kanban remote access | confirmed-with-correction | auth absence and 127.0.0.1:3484 confirmed; CDK "root / token" is an inference not flagged by page |
| Telemetry | confirmed | on by default, opt-outs, admin force-off VS Code/JetBrains only |
| Remote configuration per surface | confirmed | table matches; Kanban also No |
| Enterprise YOLO and MCP controls | confirmed | the "all interfaces" vs table contradiction is real |
| SDK plugins and hooks | confirmed | 3 s, ignore default, 5 failures, worker thread, sandbox opt-in |
| Built-in tool list / edit routing | confirmed | exact names and routing |
| Focus Chain / deprecations | confirmed | no dates |
| ClinePass | confirmed | $9.99, 2-5x claim, three windows |
| Versions (releases page) | confirmed | extension 4.1.26, SDK 0.2.0, Desktop 0.0.49, CLI 3.0.74, 26 Oct, no year |

Borrowed features: none found; the file records Cline's own mechanisms and flags doc contradictions itself.

Notable omissions (from https://docs.cline.bot/llms.txt): Networking and Proxies page; Supply-Chain Scan Alerts CLI sample; GitHub Issue RCA and PR Review samples (only partly used); Cline Free Models; Enterprise API Reference and SSO Setup / Managing Members; OpenTelemetry Events and Environment Variables references; Cline API pages (Authentication, Models, Errors); SDK pages Going to Production, Building an Agent, Gateway and Tools API reference. The survey lists most of these as "not opened".

Trust judgement: reliable. All 25 checked entries were supported by the cited pages; only two small detail corrections. The author flagged contradictions and gaps (no hub auth, YOLO enterprise inconsistency, stub hooks page) honestly and accurately. Unchecked entries are mostly smaller UI and provider items and carry low risk.
