# Warp (round 1, public-surface survey)

Research date: 2026-11-10. Sources are docs.warp.dev section files (`_llms-txt/*.txt`, opened and read in chunks, the page URLs below are the canonical paths printed inside them), a few individually opened pages, the Warp blog post and the GitHub repository front page. No source code was read. Several large section files were not read to the end (see Unverified).

Citation convention: "Evidence" gives the page URL; where the page was read through a section file, the section file is named as `via <file>`.

## 1. Identity

- Name: Warp, described in its docs as four products: the desktop terminal, the Warp Agent, the Automation Platform (cloud agents), and Warp Factories (team workflows). Vendor: Denver Technologies (from `00-targets.md`; not re-verified here).
- Surfaces: desktop app (macOS, Windows, Linux packages: deb, rpm, Arch, openSUSE, AppImage), the `warp` terminal agent CLI (works over SSH, app not required), the `oz` CLI (marked a supported legacy interface), the Oz web app, the Factories web app/dashboard, a Warp Drive web view, Python and TypeScript SDKs, a REST "Warp Platform API" (OpenAPI spec published), a hosted Factory MCP endpoint, a GitHub Action (`oz-agent-action`), Slack, Linear, Jira, GitHub, GitLab, Azure DevOps, Bitbucket integrations. (https://docs.warp.dev/ via getting-started.txt; https://docs.warp.dev/platform/architecture/)
- Open or closed: partly open. Confirmed: the Warp **client** was open-sourced 2026-04-28 under AGPL v3; repo https://github.com/warpdotdev/warp, default branch `master`, about 66.0k stars. The `warpui_core` and `warpui` UI-framework crates are MIT; the rest of the repo is AGPL v3. (https://www.warp.dev/blog/warp-is-now-open-source ; https://github.com/warpdotdev/warp)
- What is NOT stated to be open: the blog post and README say nothing about the server, the control plane, Oz/cloud orchestration, model routing or the Factories backend. They are described as Warp-operated services (https://docs.warp.dev/platform/architecture/ says the control plane is "operated by Warp"). Treat them as closed/unstated; the docs never say they are open. The self-hosted worker `oz-agent-worker` is a documented daemon, but its licence was not checked (unverified). SDK repos are named `oz-sdk-python` and `oz-sdk-typescript` (licence not checked). A skills collection `warpdotdev/oz-skills` is mentioned.
- Contribution model: agent-driven. Public issues are source of truth; maintainers label `ready-to-spec` or `ready-to-implement`; humans contribute ideas and verification, agents (Oz preferred, others allowed) do the coding; OpenAI is named founding sponsor. Build: `./script/bootstrap`, `./script/run`, `./script/presubmit`; `AGENTS.md` is the engineering guide. (blog; README)
- Versions: release cadence is weekly (Thursdays). Newest changelog entry seen: 2026.10.22, `v0.2026.10.21.08.25`. Scout's note that GitHub tags stop at `v0.2026.06.09` was not re-checked. (https://docs.warp.dev/changelog ; https://docs.warp.dev/changelog/2026/ , first 100k characters only)
- Correction to `00-targets.md`: none found; the table row (AGPL, MIT UI crates, branch `master`, 66.0k) matches. The scout's "Oz" naming is partly stale: the changelog says the "Oz agent UI" was renamed to "Warp Agent" on 2026.08.13, and Oz CLI docs say "supported legacy interface" with Factories recommended for new work.

## 2. Features by area

### Area 1. Agent loop and execution model

### Warp Agent (local, in the terminal app)
- What: a multi-step agent that runs in a real terminal session and waits for approval before running commands or applying diffs (depending on profile). The same agent exists in the app, in the `warp` CLI and as cloud runs.
- Status: GA
- Evidence: https://docs.warp.dev/agents/ via agents.txt
- Notes: SOC 2 and ZDR claims on the page are `vendor-claim`.

### Terminal mode vs Agent mode, input classification
- What: a minimal terminal for shell commands and a dedicated conversation view with model, voice and image controls. `#` at the prompt generates a command; auto-classification of natural language vs shell exists (toggle in the CLI as `/natural-language-detection`).
- Status: GA
- Evidence: https://docs.warp.dev/agents/local-agents/interacting-with-agents/terminal-and-agent-modes/ (truncated in source); https://docs.warp.dev/agents/local-agents/generate/ via agents.txt; https://docs.warp.dev/agents/cli/input-and-shell-commands/
- Notes: in-CLI command generation (inside psql, vim) is legacy, replaced by Full Terminal Use.

### Conversation lifecycle
- What: multi-turn conversations tied to a session. A new conversation starts after a shell command, after 3 hours of inactivity, or manually. A context-window indicator shows above 20% usage, a breakdown view shows what fills the window, and Warp auto-summarizes once the limit is exceeded.
- Status: GA
- Evidence: https://docs.warp.dev/agents/local-agents/interacting-with-agents/ via agents.txt
- Notes: (verified in review: three hours of inactivity or a shell command starts a fresh conversation; indicator hidden below 20% of the window; auto-summarize only once the window is exceeded)

### Prompt queueing
- What: follow-up prompts can be queued while the agent responds (toggle or `/queue`); sent one at a time after each successful response; errors pause the queue without discarding it; cleared on restart. Prompts queue by default during agent-driven long-running commands.
- Status: GA
- Evidence: https://docs.warp.dev/agents/local-agents/interacting-with-agents/prompt-queueing/ via agents.txt

### Agent questions
- What: the agent can pause and ask multiple-choice questions (single or multi select, optional "Other"). Governed by a profile permission: never ask, ask unless auto-approve, always ask.
- Status: GA
- Evidence: https://docs.warp.dev/agents/local-agents/interacting-with-agents/agent-questions/ via agents.txt

### Active AI recommendations
- What: proactive suggestions from terminal activity: prompt suggestions, next-command predictions, suggested code diffs for errors. Each can be disabled. Suggested diffs have monthly caps by plan.
- Status: GA
- Evidence: https://docs.warp.dev/agents/local-agents/active-ai/ via agents.txt

### Area 2. Built-in tools

### Tool inventory
- What: no page lists the agent's tools by name. Documented capability areas are: slash commands, skills, planning, task lists, rules, full terminal use, computer use, MCP, codebase context, profiles and permissions, web search. Permission types hint at tools: apply diffs, read files, create plans, execute commands, interact with running commands, ask questions.
- Status: unclear (no tool reference)
- Evidence: https://docs.warp.dev/agents/capabilities/ ; https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/
- Notes: nothing documented as a tool list.

### Web search
- What: models with native search tools (certain Claude and OpenAI models) may search the web; toggled per profile ("Call web tools"); each search has a small fixed credit cost plus a cost proportional to retrieved content.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/web-search/ via agents.txt

### Task lists
- What: the agent breaks complex requests into tasks automatically; each task shows current, completed, not started or cancelled; a chip opens the list.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/task-lists/ via agents.txt

### Area 3. Code editing and file operations

### Agent code diffs
- What: a built-in diff editor for agent changes: navigate hunks and files, accept, refine with natural language, or edit manually. Changes apply only on explicit acceptance, unless Apply Code Diffs is "Always allow".
- Status: GA
- Evidence: https://docs.warp.dev/agents/local-agents/code-diffs/ via agents.txt

### Built-in code editor
- What: native editor with syntax highlighting, tabbed viewer, find and replace (regex, preserve case), Vim keybindings, file tree. Files open from terminal output, palette, tree or agent diffs. Multiple views of one file stay in sync.
- Status: GA
- Evidence: https://docs.warp.dev/code/code-editor/ ; .../find-and-replace/ ; .../file-tree/ ; .../code-editor-vim-keybindings/ (all via code.txt)

### LSP support
- What: hover, go-to-definition, references, inline diagnostics, format on save for Rust, Go, Python, TS/JS, C/C++. Enabled per Git repo root. Local sessions only, one server per language, no custom server configuration yet.
- Status: GA (limitations as stated)
- Evidence: https://docs.warp.dev/code/code-editor/language-server-protocol/ via code.txt

### Area 4. Terminal and shell integration

### Blocks
- What: each command and its output form a block that can be copied, shared (permalink or iframe), bookmarked, filtered by text or regex, found. Background-process output gets its own block. A sticky header pins the running command.
- Status: GA
- Evidence: https://docs.warp.dev/terminal/blocks/ and subpages via terminal.txt

### Input editor and completions
- What: IDE-style multi-line input editor, Vim mode, alias expansion (off by default), command inspector, syntax and error highlighting, autosuggestions from history and completion specs, tab completions, native shell completions alongside built-in specs (changelog 2026.09.16), command palette.
- Status: GA
- Evidence: https://docs.warp.dev/terminal/editor/ ; https://docs.warp.dev/terminal/command-completions/ ; https://docs.warp.dev/changelog/2026/

### Full Terminal Use
- What: the agent reads the live terminal buffer and types into interactive programs (database shells, editors, debuggers). The user can take over or hand back at any time. Each action can be approved once or auto-approved for the session; global setting: ask on first write, always ask, always allow.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/full-terminal-use/ via agents.txt

### Blocks and other items as agent context
- What: attach terminal blocks, images (resized to model limits, each image costs extra requests), a single public URL (page text is scraped, no crawling), `@` references to files, folders, symbols, Warp Drive objects and blocks from other sessions, and editor or diff selections with line numbers. `@` search is rooted at the Git repo, respects `.gitignore`, and works without indexing.
- Status: GA
- Evidence: https://docs.warp.dev/agents/local-agents/agent-context/ and subpages via agents.txt

### SSH
- What: coding features (file tree, editor, code review, indexing, native diffs) work in "Warpified" SSH sessions via an SSH extension on macOS and Linux hosts. LSP, agent orchestration and computer use are not available over SSH. The `warp` CLI also runs over SSH.
- Status: GA with stated gaps
- Evidence: https://docs.warp.dev/code/ssh-feature-support/ via code.txt

### Terminal appearance and misc
- What: themes (YAML, image-based), prompt chips, input position (3 modes), tabs, panes, performance benchmarks page. Not agent related.
- Status: GA
- Evidence: https://docs.warp.dev/terminal/appearance/ via terminal.txt

### Area 5. Permissions, approvals and sandboxing

### Agent profiles
- What: created in Settings > Agents > Profiles; each has a name, base model (also used for planning unless a separate planning model is set) and autonomy and permissions. Every user gets a default profile; new profiles copy it. Profiles sync to each host and can be applied from the CLI (`oz agent run --profile <ID>`). The default CLI profile is broadly permissive, with an empty MCP allowlist.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ ; https://docs.warp.dev/agents/cli/oz-cli/agent-profiles/

### Per-action autonomy levels
- What: each permission (apply diffs, read files, create plans, execute commands, interact with running commands, ask questions) is one of: Agent decides, Always ask, Always allow, Never. For diffs, "Agent decides" behaves like "Always ask". All "Always allow" is full autonomy ("YOLO"). In the `warp` CLI the values are `always_ask`, `always_allow`, `agent_decides`; `agent_decides` is the default for shell commands and file reads; auto-approve is off per new conversation unless `warp --auto-approve`.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ ; https://docs.warp.dev/agents/cli/permissions-and-profiles/ via agents.txt
- Notes: the page does not state the default autonomy per permission for the app. (verified in review: four levels; for diffs "Agent decides" currently behaves like "Always ask")

### Command allowlist and denylist (regex)
- What: both lists take regular expressions. Allowlist is empty by default; matches auto-run even if not read-only. Denylist is pre-populated (examples: curl, wget, rm, eval); matches always prompt, overriding allowlist, "Agent decides" and "Always allow". Admin-set denylist entries (Admin Panel) can never be bypassed.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ ; https://docs.warp.dev/knowledge-and-collaboration/admin-panel/
- Notes: regex matching on the command string is a coarse mechanism; the docs state no other protection against dangerous commands. (verified in review: allowlist empty by default, denylist ships with curl, wget, rm, eval, denylist beats allowlist and Agent decides, admin entries never bypassed)

### Run until completion (auto-approve)
- What: shortcut that auto-runs every suggested command until done or Ctrl+C. By default it bypasses the user's denylist (setting "Allow auto-approve to bypass command denylist" can turn that off); admin denylist is never bypassed.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/
- Notes: default bypass of the denylist is a notable weak point. (verified in review: the page says the shortcut (Cmd/Ctrl+Shift+I) also runs commands matching the denylist, and the setting "Allow auto-approve to bypass command denylist" under Settings > Agents > Warp Agent > Input turns it off. This is about the explicit "Run until completion" shortcut, not the default autonomous mode in general: "Always allow" still respects the denylist)

### MCP permissions
- What: allowlist (agent may call without asking), denylist (always ask, wins over allowlist), agent decides. File-based `~/.warp/.mcp.json` servers auto-spawn; project-scoped and third-party servers need approval; config edits need approval.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ ; https://docs.warp.dev/agents/capabilities/mcp/

### Directory allowlist (admin)
- What: Admin Panel can set directories where agents may read files without asking, plus enforce Apply Diffs, Create Plans, Execute Commands, Read Files, and command lists across the team.
- Status: GA (Business and Enterprise)
- Evidence: https://docs.warp.dev/knowledge-and-collaboration/admin-panel/

### Sandboxing of local agents
- What: nothing documented. Local agents run commands in the user's own terminal session; the docs describe approval gating, not an OS sandbox. Sandboxing is documented only for cloud runs (fresh isolated container per run, Warp-hosted or self-hosted).
- Status: nothing documented (local)
- Evidence: https://docs.warp.dev/agents/ ; https://docs.warp.dev/platform/architecture/

### Area 6. Context management and compaction

### Compaction and summarization
- What: automatic summarization when the context limit is exceeded; `/compact [instructions]` in the CLI; `/fork-and-compact` command; a provider-level "prompt summarization caching" admin toggle lets the LLM provider cache summaries.
- Status: GA
- Evidence: https://docs.warp.dev/agents/local-agents/interacting-with-agents/ ; https://docs.warp.dev/agents/cli/agent-conversations/ ; https://docs.warp.dev/agents/capabilities/slash-commands/ ; admin panel page. All via agents.txt or direct.

### Codebase Context (embeddings index)
- What: embeddings index of Git-tracked files, built on first open, periodically, at the start of each conversation and on manual sync. Agents do not use it until indexing finishes. Worktrees are indexed separately. Free plan: up to 5,000 files per repo. Respects `.gitignore`, `.warpindexingignore`, `.cursorignore`, `.cursorindexingignore`, `.codeiumignore`. Page states no code is stored on Warp servers; it does not say where embeddings are computed. Over SSH the remote repo is indexed on the remote machine.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/codebase-context/
- Notes: "no code stored" is `vendor-claim`; compute location unstated. (verified in review: 5,000 files on Free, ignore files, indexing triggers; page says code is never stored on Warp servers)

### Area 7. Memory and persistent instructions

### Rules (global and project)
- What: Global Rules (managed in Warp Drive, optional name and description) and Project Rules in `AGENTS.md` (default; legacy `WARP.md` still works and wins if both exist; filename must be all caps). Root and current-directory files are applied automatically; subdirectory files best-effort. Precedence: subdirectory file, root file, Global Rules. `CLAUDE.md` is not read directly; `/init` can link it, `.cursorrules`, `GEMINI.md`, `.clinerules`, `.windsurfrules`, `.github/copilot-instructions.md`.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/rules/
- Notes: no numeric limits documented.

### Agent Memory
- What: persistent memory shared across harnesses (Warp Agent, Claude Code, Codex). Extracted after a conversation ends, retrieved in the background, organized into personal, agent and team stores with read-only or read-write attachments. On by default for new agents created in the Oz web app. Third-party harnesses only as cloud agents.
- Status: preview (research preview, per-team waitlist)
- Evidence: https://docs.warp.dev/agents/agent-memory/ via agents.txt
- Notes: (verified in review: research preview, enabled per team for design partners with a waitlist; extraction runs in the background after a conversation ends; auto-memory on by default for agents created in the Oz web app; third-party harnesses only as cloud agents)

### Skills
- What: `SKILL.md` with YAML frontmatter (`name`, `description`) in its own directory, with supporting files. Discovered by walking from the working directory to the repo root across `.agents/skills/` (recommended), `.warp/`, `.claude/`, `.codex/`, `.cursor/`, `.gemini/`, `.copilot/`, `.factory/`, `.github/`, `.opencode/` skills dirs, and the same under home. Invoke with `/skill-name`; `$ARGUMENTS`, `$0`, `$1` placeholders. Cloud agents find skills in repos in their environment; `WARP_SKILL_DIRS` adds dirs. Bundled skills exist (for example `/modify-settings`, `/tui-migrate-setup`). Skills are file-based, not cloud-synced.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/skills/ ; https://docs.warp.dev/knowledge-and-collaboration/warp-drive/ai-objects/
- Notes: (verified in review: the ten directory names, home-directory equivalents, `WARP_SKILL_DIRS` comma-separated and cloud-only, `$ARGUMENTS`/`$N` placeholders)

### Area 8. Planning, tasks and modes

### Planning
- What: `/plan` or natural language produces an editable plan in a rich-text editor with version history (each agent update is a version, restorable); run all or a section; plans sync to a Plans folder in Warp Drive, export to Markdown, share by link; `@plans` reopens. A separate planning model can be set in the profile. In the CLI, `/plan <task>`, Ctrl+Shift+P toggles.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/planning/ ; https://docs.warp.dev/agents/cli/agent-conversations/

### Slash commands
- What: built-in menu (`/plan`, `/compact`, `/fork`, `/handoff`, `/model`, `/orchestrate`, and more) plus saved Warp Drive prompts and skills; commands marked with an asterisk use metered inference.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/slash-commands/ via agents.txt

### Area 9. Sub-agents and multi-agent orchestration

### Multi-agent orchestration
- What: one parent and one or more children, exactly one level deep (children cannot spawn children). Each child is an independent run with its own transcript and usage. Placement: local to local, local to cloud, cloud to cloud, cloud to cloud-local (shares parent's environment). Children can use a different harness (Claude Code, Codex). Agents talk through a durable server-backed message bus with per-agent inboxes, across harnesses and locations; terminal-state children can wake on a message; lifecycle events avoid polling. Children launch only after user approval of the proposed breakdown (`/orchestrate`, or `/plan` with an orchestration config block; API `mode: orchestrate|plan`). The docs advise one worktree and branch per agent. Run states: INPROGRESS, SUCCEEDED, FAILED, BLOCKED, ERROR, CANCELLED. Cancelling a parent does not cancel children.
- Status: GA (no beta label on the page); Factories is separately labelled Early Access
- Evidence: https://docs.warp.dev/platform/orchestration/ ; https://docs.warp.dev/platform/orchestration/multi-agent-runs/ via automation-platform.txt
- Notes: no numeric limits on children or concurrency documented. (corrected in review: depth, placements, cross-harness children, server-backed per-agent inbox bus, approval before launch, run states, and that cancelling a parent does not cancel children are confirmed on /platform/orchestration/ and /multi-agent-runs/. The "wake on message" and "one worktree per agent" advice are not on those pages (the bus page only says a terminal-state child can still receive follow-up messages; worktree advice is deferred to a separate guide); the page states no status label, so GA is the survey's inference)

### Orchestration in the `warp` CLI
- What: an approval card ("Can I start additional agents for this task?") unless permission "Run orchestrated agents" is Always allow; Never blocks child agents. Local children are Warp Agent sessions; cloud children can use other harnesses but are status-only in the CLI (input via the Oz web app). Navigation through an "Agents:" bar; Ctrl+C kills a child.
- Status: GA
- Evidence: https://docs.warp.dev/agents/cli/cloud-and-orchestration/

### Conversation forking
- What: branch a conversation into a new thread with original context (`/fork`, `/fork-and-compact`, `/fork-from`, from any earlier response); default layout split pane. Continuing someone else's shared conversation creates a fork.
- Status: GA
- Evidence: https://docs.warp.dev/agents/local-agents/interacting-with-agents/conversation-forking/ ; .../cloud-conversations/ via agents.txt

### Factories (agent team workflow)
- What: a factory is a team of cloud agents attached to repositories. A "foreman" agent coordinates and talks to people; default agents for triage, planning, implementation, review, with human checkpoints on spec, code and product; custom agents possible, models and harnesses configurable. Never merges on its own; repo permissions and branch protection govern. Work items keep source context from intake to handoff. Definitions (repos, agents, automations, runners, skills, MCP servers) are files that can live in a customer GitHub repo and go through PR review. Dashboard shows PR activity, autonomy, cycle time, cost; scorers, benchmarks and self-improvement proposals exist. Sizing advice: one factory per set of repos that ship together.
- Status: preview (the vendor label is "Early Access"; changelog 2026.08.27: "now live in Early Access")
- Evidence: https://docs.warp.dev/factories/ via factories.txt ; https://docs.warp.dev/changelog/2026/
- Notes: (corrected in review: status relabelled from beta to the vendor's own "Early Access"; foreman, default agents, never merges, repo-hosted definitions reviewed by PR and dashboard of PR activity, autonomy, cycle time, cost verified on /factories/) dashboards, scorers and self-improvement were only summarized from section-file text; their pages were not read in full.

### Area 10. Background, long-running and scheduled work

### Scheduled agents
- What: cloud agents on a 5-field cron schedule with a fixed prompt or skill; each run starts a fresh session. `oz schedule create|list|get|pause|unpause|update|delete`; updates affect future runs only; a CLI-created schedule runs as its creator (Agent setting changed in the web app). Page does not state timezone, limits or overlap behaviour. Factories also have scheduled automations.
- Status: GA (via the Oz CLI, marked legacy)
- Evidence: https://docs.warp.dev/platform/triggers/scheduled-agents/ ; https://docs.warp.dev/factories/ via factories.txt

### Triggers overview
- What: cloud runs start from schedules, CLI, API and SDK, integrations, GitHub mentions, GitHub Actions, custom webhooks, and Handoff from the app.
- Status: GA
- Evidence: https://docs.warp.dev/platform/triggers/ via automation-platform.txt

### Cloud agent run lifecycle
- What: trigger, task record, configuration resolution (environment, runner, host, model, harness), provisioning (clone repos, run setup commands, inject allowed secrets), agent loop, outputs (branch, PR, reply in originating Slack/Linear/Jira), persistence, visibility, teardown with optional workspace snapshot. Runs have a maximum runtime (error `infrastructure_timeout`). Unattended CLI agent runs end as blocked when the agent stops to ask for input (changelog 2026.10.07).
- Status: GA
- Evidence: https://docs.warp.dev/platform/architecture/ ; https://docs.warp.dev/factories/api-and-sdk/troubleshooting/errors/ via factories.txt ; changelog

### Area 11. Sessions: persistence, resume, fork, checkpoints, undo

### Conversation history, resume, export
- What: conversations listed (Conversations menu, panel with Active and Past). CLI: `/conversations`, `warp --resume <TOKEN>`, `/export-to-clipboard`, `/export-to-file`. Conversations sync to the Warp account; "Store AI conversations in the cloud" setting lets conversations be shared by link (team, individuals, anyone), storage limits per plan, oldest cloud copies removed for free users, enterprise admins can disable it.
- Status: GA
- Evidence: https://docs.warp.dev/agents/cli/agent-conversations/ ; https://docs.warp.dev/agents/local-agents/cloud-conversations/ via agents.txt

### Handoff local/cloud
- What: `/handoff [prompt]` forks a local Warp Agent conversation to a cloud run and snapshots uncommitted changes including untracked files; also cloud to cloud (follow-ups to ended runs, needs terminal state and snapshot) and cloud to local. Blocked while a command runs or child agents are active. Snapshot contents configurable via declaration scripts or static JSONL; cloud agents periodically save handoff checkpoints (changelog 2026.10.15) and on interruption (2026.09.23). Personal BYOK keys are not copied to handoff runs.
- Status: GA
- Evidence: https://docs.warp.dev/platform/handoff/ and subpages via automation-platform.txt ; https://docs.warp.dev/agents/cli/cloud-and-orchestration/ ; https://docs.warp.dev/platform/handoff/local-to-cloud/
- Notes: (corrected in review: `/handoff` (or `&`), snapshot of tracked and untracked changes, local-to-cloud, cloud-to-cloud and cloud-to-local (`/continue-locally`), BYOK keys and endpoint configs not copied, and the requirement to enable "Store AI conversations in the cloud" are confirmed; the "blocked while a command runs or child agents are active" detail was not on the handoff pages I opened and is unverified; the 2026.10.15 and 2026.09.23 checkpoint changelog claims were not checked)

### Checkpoints, undo and rewind of file edits
- What: nothing documented in the parts read (only handoff snapshots, plan version history and per-hunk revert in the Code Review panel).
- Status: nothing documented
- Evidence: checked agents.txt, code.txt; https://docs.warp.dev/code/code-review/

### Area 12. Git and version control

### Code Review panel
- What: reviews uncommitted changes, changes vs `main`/`master`, or vs any branch. Entry points: Git diff chip, "Review changes" in conversations, toolbelt. Attach diffs as agent context, revert individual hunks, discard all, open in editor. Inline comments can be sent to running agents, including third-party CLI agents. On remote repos the panel supports commit, push, PR creation.
- Status: GA
- Evidence: https://docs.warp.dev/code/code-review/ ; https://docs.warp.dev/code/ssh-feature-support/ via code.txt
- Notes: the "Interactive Code Review" page was referenced but not read.

### Git worktrees
- What: native support; each worktree gets its own Code Review panel, status indicators, file watching and independent indexing.
- Status: GA
- Evidence: https://docs.warp.dev/code/git-worktrees/ via code.txt

### Computer-use artifacts in pull requests
- What: screenshots (up to 10 per PR) and recordings attached automatically to PR descriptions when a run creates or updates a PR. Team admin setting: Disabled, Link only (default, private links with access check), Embed (public URLs).
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/computer-use/artifacts-in-prs/ via agents.txt

### Git host integrations
- What: GitHub (mention `@warp-agent` in issues, PRs, review comments; Warp Factories GitHub App), GitHub Actions (`oz-agent-action` for PR review, issue fixes, CI fixes), GitLab (GitLab.com native; self-managed via PAT secret), Azure DevOps (PAT, Services or Server), Bitbucket Cloud and Data Center (tokens as managed secrets plus clone setup command). Repo identity: user-triggered runs use that user's GitHub token; agent API key runs use Team GitHub authorization; integration-started runs act as the GitHub App.
- Status: GA (Factories-linked parts early access)
- Evidence: https://docs.warp.dev/platform/integrations/ and subpages via automation-platform.txt ; https://docs.warp.dev/platform/architecture/

### Area 13. Extensibility

### MCP client
- What: CLI (stdio) servers and Streamable HTTP or SSE servers, configured by UI or JSON; stored under Warp Drive > MCP Servers or `~/.warp/.mcp.json` (CLI: `~/.warp_cli/.mcp.json` on macOS). Credentials and logs stored locally; sharing a server scrubs environment values. MCP tool calls now time out instead of waiting forever (changelog 2026.10.07). Docs list a catalog of examples (GitHub, Sentry, Grafana, Linear, Chroma, Figma, Slack, Atlassian, Notion). Cloud: URL-backed servers (including OAuth) go through Warp's proxy; command servers run in the execution environment. `--mcp` on `oz agent run` takes a UUID, an integration ID (`linear`, `slack`, `jira`), inline JSON or a file.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/mcp/ ; https://docs.warp.dev/agents/cli/oz-cli/mcp-servers/ ; https://docs.warp.dev/platform/architecture/

### Hosted Factory MCP
- What: a hosted MCP endpoint through which clients can interact with a factory; Claude Code and Codex runs can access it automatically (changelog 2026.09.09, relayed second-hand via the changelog summary).
- Status: beta (Factories early access)
- Evidence: https://docs.warp.dev/platform/architecture/ ; https://docs.warp.dev/changelog/2026/
- Notes: the Factory MCP tool list was not read. (corrected in review: the changelog says the built-in Factory MCP server arrived in 2026.08.18; 2026.09.09 is the entry where Claude Code and Codex runs gained automatic access; /platform/architecture/ confirms "the hosted Factory MCP endpoint" as a control-plane entry point)

### SDKs and API
- What: Python (`warp-platform-sdk`) and TypeScript (`@warp-dot-dev/warp-platform-sdk`) SDKs and a REST Platform API with OpenAPI spec (`/openapi.yaml`, `/openapi.json`, the spec files themselves were not opened). Endpoints: create, list, monitor, follow up on, cancel runs; factory endpoints send work to a factory; agent identities CRUD (`/agent/identities`). Documented error codes (25 pages). Demo: webhook service turning Sentry alerts into draft PRs.
- Status: GA
- Evidence: https://docs.warp.dev/platform/transitioning-from-oz/ ; https://docs.warp.dev/factories/api-and-sdk/ and subpages via factories.txt ; https://docs.warp.dev/platform/agents/ via automation-platform.txt

### Plugins and hooks for third-party agents
- What: Warp ships plugins that let Claude Code (`warp@claude-code-warp`), Codex (`codex-warp` marketplace) and OpenCode (`@warp-dot-dev/opencode-warp`) emit notifications to Warp. Warp itself documents no hook system, plugin API or custom-command format for its own agent beyond skills, slash commands and saved prompts.
- Status: GA
- Evidence: https://docs.warp.dev/agents/cli-agents/claude-code/ ; .../codex/ ; .../opencode/ via agents.txt
- Notes: hooks and plugins for the Warp Agent itself: nothing documented.

### ACP
- What: the changelog mentions "ACP agents" and "ACP-driven parent runs" (2026.10.22, including Pi) but no ACP documentation page was read.
- Status: unclear
- Evidence: https://docs.warp.dev/changelog/2026/
- Notes: unverified beyond changelog wording.

### Federated identity (OIDC)
- What: `oz federate issue-token` issues short-lived OIDC tokens from inside a running agent (5 min to 3 hours, default 1 hour, configurable subject template) so it can authenticate to cloud providers without static credentials. Cloud providers page covers AWS and GCP setup.
- Status: GA
- Evidence: https://docs.warp.dev/agents/cli/oz-cli/federate/ ; https://docs.warp.dev/platform/integrations/cloud-providers/ via agents.txt and automation-platform.txt

### Area 14. Models, providers and inference configuration

### Model choice and Auto models
- What: curated list grouped by provider with `model_id` values for CLI and platform; effort-level suffixes for most families; Auto models Responsive, Cost-efficient, Genius, Open-weights. Fallback routing switches to a comparable model during outages. Some models lack ZDR and are off by default for Enterprise. Specific model names were not recorded here (the list churns).
- Status: GA
- Evidence: https://docs.warp.dev/agents/inference/model-choice/ via agents.txt

### Bring your own API key
- What: Anthropic, OpenAI, Google keys stored on the device (secure storage in CLI), local runs only; Warp-credit fallback off by default; Auto models always use Warp inference; cloud runs do not support personal keys. Available to individuals and orgs of 10 or fewer; larger orgs need Business or Enterprise.
- Status: GA
- Evidence: https://docs.warp.dev/agents/inference/bring-your-own-api-key/ ; https://docs.warp.dev/agents/cli/models-and-usage/ ; pricing FAQ via support.txt

### Custom inference endpoint
- What: any OpenAI-compatible Chat Completions endpoint (OpenRouter, LiteLLM, z.ai) for local agents; must be publicly reachable (localhost and private addresses rejected); no cloud runs, no Auto routing.
- Status: GA
- Evidence: https://docs.warp.dev/agents/inference/custom-inference-endpoint/ via agents.txt
- Notes: the localhost restriction matters for local model users. (verified in review: public URL required, local and private addresses rejected, requests relayed through Warp, no personal endpoints in cloud runs, Auto models ignore the endpoint)

### Subscription sign-in (ChatGPT, SuperGrok)
- What: sign in with a ChatGPT plan to use eligible OpenAI models in the app (requests count against ChatGPT limits; a saved OpenAI key takes precedence; changelog 2026.09.23); connect an X Premium or SuperGrok subscription by OAuth (tokens on device; Warp cannot enforce ZDR for those requests).
- Status: GA
- Evidence: https://docs.warp.dev/agents/inference/chatgpt-subscription/ ; .../grok-subscription/ via agents.txt ; changelog

### Custom model routers
- What: YAML files in `~/.warp/custom_model_routers/` define routers that choose a model per task, by complexity (easy, medium, hard) or ordered natural-language rules; targets must be Warp-supported models; unparsable files are skipped with a non-blocking error.
- Status: GA
- Evidence: https://docs.warp.dev/agents/inference/custom-routers/ via agents.txt

### Enterprise BYOLLM
- What: route Warp Agent inference through the org's own cloud account: AWS Bedrock (Anthropic and OpenAI models; interactive use via each member's AWS CLI session; cloud agents assume an IAM role by OIDC federation) and Gemini Enterprise / Vertex AI (Gemini and Claude partner models via Workload Identity Federation; interactive only, cloud agents not yet). Team-managed keys and endpoints also exist (page not read).
- Status: GA (Enterprise)
- Evidence: https://docs.warp.dev/enterprise/enterprise-features/bring-your-own-llm/ ; .../byollm-aws-bedrock/ ; .../byollm-gemini-enterprise/ via enterprise.txt

### Third-party harness choice for cloud runs
- What: `--harness oz|claude|codex` on `oz agent run-cloud`; Claude Code and Codex call their providers directly from the execution environment, with auth via secrets (`--claude-auth-secret`, `--codex-auth-secret`).
- Status: GA
- Evidence: https://docs.warp.dev/agents/cli/oz-cli/ ; https://docs.warp.dev/platform/architecture/

### Area 15. UI surfaces

### Desktop app, Warp Agent CLI, web
- What: desktop GUI (macOS, Windows, Linux); `warp` TUI-style agent CLI with transcript, slash menu, themes, configurable statusline, Vim mode, image paste, voice (not in the official Linux build); Oz web app (runs, agents, skills, schedules, environments, secrets, integrations); Factories web app and dashboard; Warp Drive on the web (view and edit objects and shared sessions, including on mobile; no command execution). Agent Management Panel and notifications (complete, request, error; toasts, mailbox, tab badges) for agents including Claude Code, Codex, OpenCode via plugins.
- Status: GA (Factories Early Access)
- Evidence: https://docs.warp.dev/agents/cli/ ; https://docs.warp.dev/platform/oz-web-app/ ; https://docs.warp.dev/platform/managing-cloud-agents/ ; https://docs.warp.dev/knowledge-and-collaboration/warp-drive/web/ ; https://docs.warp.dev/agents/capabilities/agent-notifications/

### Voice
- What: voice input in the app and CLI (Ctrl+S, `/voice`) and in the rich input editor.
- Status: GA
- Evidence: https://docs.warp.dev/agents/cli/input-and-shell-commands/ ; https://docs.warp.dev/agents/cli-agents/rich-input/

### Third-party CLI agents inside Warp
- What: Warp auto-detects Claude Code, Codex, OpenCode, Amp, Gemini CLI and others (changelog adds Muse Code and Kiro CLI), shows an agent toolbelt, a rich input editor (Ctrl-G) with `@` mentions, code-review comments sent to the running agent, and `/remote-control` which publishes a third-party session to the cloud with a shareable link (view or edit access). Notifications only for Claude Code, Codex, OpenCode. These sessions appear in vertical tabs, not the Conversation panel.
- Status: GA
- Evidence: https://docs.warp.dev/agents/cli-agents/overview/ ; .../remote-control/ ; .../rich-input/ via agents.txt ; changelog

### Area 16. Browser automation and computer use

### Computer use
- What: agents see the screen, click and type, only inside sandboxed cloud environments (not local sessions). On by default for cloud runs on Warp's built-in harness, off by default for third-party harnesses; runs from the app follow a "Computer use in Cloud Agents" setting that is off by default. Supported models: Anthropic Claude, selected automatically. Docs recommend avoiding sensitive data and limiting internet access.
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/computer-use/ via agents.txt
- Notes: (verified in review: cloud-only, on by default for the Warp harness, off for Claude Code and Codex, app-started runs follow an off-by-default setting, Anthropic Claude models)

### Browser use and recordings
- What: every computer-use environment bundles Chromium and the Playwright CLI; visual (screenshot) or scripted modes; CDP attach for advanced users. Sessions can be recorded (idle time cut, action overlays), capped at 10 minutes and 1 GiB per recording; Windows cloud runners can record video (changelog 2026.10.07).
- Status: GA
- Evidence: https://docs.warp.dev/agents/capabilities/computer-use/browser-use/ ; .../testing-and-recordings/ via agents.txt

### Area 17. Remote, cloud, headless, CI and distributed execution

### Environments
- What: a cloud environment is a Docker image plus repos plus setup commands; setup runs fresh on each run and should be idempotent; each run starts in a fresh isolated container. Created with `/create-environment` or `oz environment create`. glibc images are required (Alpine/musl is listed as incompatible). Pre-warmed sandboxes exist (changelog 2026.10.22).
- Status: GA
- Evidence: https://docs.warp.dev/agents/cli/oz-cli/integration-setup/ ; https://docs.warp.dev/agents/cli/oz-cli/troubleshooting/ via agents.txt

### Headless and CI via the `oz` CLI
- What: `oz agent run` (local), `oz agent run-cloud` (remote; flags for environment, model, skill, MCP, host, attachments up to 25 files of 10 MB, `--harness`, `--computer-use`). Named agents, run listing and message commands (`oz run message list|read|watch|send`), artifacts, API keys. Auth by `WARP_API_KEY`: personal keys, agent keys, or self-hosted worker keys; expiry 1 day, 30 days, 90 days or never; raw key shown once. Marked a supported legacy interface; Factories recommended for new multi-stage workflows.
- Status: GA, labelled legacy
- Evidence: https://docs.warp.dev/agents/cli/oz-cli/ ; .../api-keys/ ; .../artifacts/ via agents.txt
- Notes: (verified in review: /agents/cli/oz-cli/ calls `oz` "a supported legacy interface" and recommends Factories for new multi-stage work; `--harness` is oz, claude (alias claude-code) or codex; `--attach` max 25 files of 10 MB. That page lists no key expiry options, so the 1 day/30 days/90 days/never figures were not confirmed there. /platform/transitioning-from-oz/ does NOT call `oz` legacy, it lists it as current: the pages disagree)

### Warp Agent CLI as the local headless-capable agent
- What: `warp` command in any terminal, installed by script, PowerShell or Homebrew, device-code browser login; settings in `settings.toml` (per-OS paths); migrates settings and global MCP from the app (not credentials). Whether it has a non-interactive print mode: nothing documented in the parts read; `oz agent run --prompt` is the documented headless route.
- Status: GA
- Evidence: https://docs.warp.dev/agents/cli/ ; .../quickstart/ ; .../reference/ via agents.txt

### Self-hosted execution (managed worker)
- What: `oz-agent-worker` daemon on customer infra, outbound-only connection, authenticated with a self-hosted worker API key, addressed by worker ID (`--host`). Backends: Docker (default), Kubernetes Job, Direct (on host), Command (dispatch to external job API or queue), One-shot Direct. Repo clones and workspaces stay local; transcripts and artifacts default to Warp-managed storage unless self-hosted storage is configured; control-plane metadata stays with Warp. Workers can export OpenTelemetry metrics. Requires an Enterprise plan. Unmanaged mode: run `oz agent run` in CI or on hosts you own.
- Status: GA (Enterprise)
- Evidence: https://docs.warp.dev/factories/self-hosting/ ; https://docs.warp.dev/platform/architecture/ ; https://docs.warp.dev/enterprise/enterprise-features/architecture-and-deployment/ via enterprise.txt
- Notes: (verified in review: worker `oz-agent-worker`, five backends Docker (default), Kubernetes, Direct, Command, one-shot Direct, outbound-only, Enterprise plan required, transcripts and artifacts in Warp-managed storage by default, OpenTelemetry metrics; the page states no licence for the worker; the page says Kubernetes, not "Kubernetes Job")

### Integrations as triggers (Slack, Linear, Jira, GitHub)
- What: Slack (tag @Warp or DM the bot; replies in thread; identity mapping), Linear (tag @warp or assign the issue; posts updates, opens PRs), Jira Cloud (add label `warp-agent`), GitHub (mention `@warp-agent`), plus Microsoft Teams, GitLab, Azure DevOps and custom webhooks listed as Factory work sources. Set up via web app or `oz integration create|update`.
- Status: GA (Teams and some Factory sources early access)
- Evidence: https://docs.warp.dev/platform/integrations/slack/ ; .../linear/ ; .../jira/ ; .../github/ via automation-platform.txt ; https://docs.warp.dev/factories/ via factories.txt

### Pre-run secrets
- What: team and personal secrets via `oz secret` CLI (now "legacy"; a factory secrets workflow supersedes it); managed secrets are encrypted, fetched just-in-time with run-scoped tokens, audit-logged; locally injected secrets never transit Warp.
- Status: GA, legacy page
- Evidence: https://docs.warp.dev/platform/secrets/ ; https://docs.warp.dev/platform/architecture/

### Area 18. Collaboration, teams and enterprise administration

### Warp Drive
- What: synced workspace (personal and team) for Workflows (parameterized commands, enum args), Notebooks (markdown with runnable blocks), Prompts (parameterized agent queries), Environment variables (static, or dynamic via 1Password, LastPass or custom commands so only the command is stored), Rules, MCP servers. Agents pull context from these, with citations; toggle in settings. Sharing: team, individuals by email, or link (including non-account holders); roles can view, can edit, full access; permissions inherited from folders; moving to a team workspace is one-way; offline items stay local. Secrets cannot be saved in plain text in workflows, MCP servers, prompts.
- Status: GA
- Evidence: https://docs.warp.dev/knowledge-and-collaboration/warp-drive/ and subpages via knowledge-and-collaboration.txt ; https://docs.warp.dev/support-and-community/privacy-and-security/secret-redaction/

### Session sharing
- What: agent session sharing and cloud agent session sharing (the old page now redirects to them); `/remote-control` for third-party CLI agents. Not covered by secret redaction (page says treat like screen sharing).
- Status: GA
- Evidence: https://docs.warp.dev/knowledge-and-collaboration/session-sharing/ via knowledge-and-collaboration.txt ; secret-redaction page

### Admin Panel
- What: team admins set enforced policies: AI in remote sessions, prompt summarization caching, apply diffs, create plans, execute commands, read files, directory allowlist, command allowlist and denylist (regex), UGC data collection, Enterprise Secret Redaction (regex, unconditional), codebase context, spend limits and auto-reload, direct link and anyone-with-link sharing. Free: most fixed; Business: most toggleable; Enterprise: full control. Currently only the team creator is the designated admin.
- Status: GA
- Evidence: https://docs.warp.dev/knowledge-and-collaboration/admin-panel/ ; https://docs.warp.dev/knowledge-and-collaboration/teams/

### SSO, roles, compliance
- What: SSO (Okta, Entra ID, Google Workspace, OneLogin, SAML 2.0 and OIDC) with enforceable MFA through the IdP; roles Team Owner, Team Admin, Member; SOC 2 Type II (reports on request); TLS 1.2+ and AES-256; ZDR agreements with Anthropic, OpenAI, Google, xAI. SCIM only named, no detail read. Audit logs: not documented as a feature (closest: agent actions logged when cloud conversation storage is enabled).
- Status: GA (Enterprise)
- Evidence: https://docs.warp.dev/enterprise/ via enterprise.txt ; https://docs.warp.dev/enterprise/security-and-compliance/security-overview/
- Notes: all compliance statements are `vendor-claim`.

### Private networking
- What: connect private LLM gateways and private MCP servers to Warp-hosted services via Private Service Connect, PrivateLink, Azure Private Link, on-prem interconnect or IPsec VPN.
- Status: GA (Enterprise)
- Evidence: https://docs.warp.dev/enterprise/enterprise-features/private-networking/ via enterprise.txt

### Identity mapping and billing by key type
- What: Slack, Linear and GitHub identities map to Warp users; individual vs team access differ in which integrations and self-hosting are available; cloud runs billed to the team or individual depending on trigger.
- Status: GA
- Evidence: https://docs.warp.dev/platform/team-access-billing-and-identity/ via automation-platform.txt

### Area 19. Observability: usage and cost, logs, tracing, debugging

### Usage and cost display
- What: per-conversation usage popover in the agent footer (changelog 2026.09.23), `/usage` and `/cost` in the CLI, statusline model and usage, Agent Management Panel and Oz Runs page with status, filters, parent/child nesting; cost data attached to the run record; dashboard in Factories (cost, cycle time).
- Status: GA
- Evidence: https://docs.warp.dev/agents/cli/models-and-usage/ ; https://docs.warp.dev/platform/managing-cloud-agents/ ; changelog

### Enterprise Analytics API
- What: three read-only HTTPS endpoints (`summary`, `users`, `events`); needs a team admin's personal API key and the Enterprise Usage Reporting toggle; events cover message-level activity with a 365-day window; calls cost no credits.
- Status: GA (Enterprise)
- Evidence: https://docs.warp.dev/enterprise/enterprise-features/analytics-api/ via enterprise.txt

### Network log
- What: per-session log of the client's network requests and responses opened from the Command Palette; crash-report traffic not captured.
- Status: GA
- Evidence: https://docs.warp.dev/support-and-community/privacy-and-security/network-log/ via support.txt

### Worker telemetry
- What: self-hosted workers can export OpenTelemetry metrics.
- Status: GA
- Evidence: https://docs.warp.dev/factories/self-hosting/

### Tracing of the agent's tool calls, or hooks for external logging
- What: nothing documented (the Analytics API is usage-level).
- Status: nothing documented

### Area 20. Configuration and customisation

### Settings files and locations
- What: `warp` CLI uses `settings.toml` (`~/.warp_cli/` on macOS, `~/.config/warp-terminal/cli/` on Linux, `%LOCALAPPDATA%\warp\Warp\config\cli\` on Windows); most values hot-reload; invalid values fall back to defaults; settings can be changed by file, slash command, or by asking the agent. The app uses a settings UI (Settings > Agents, Privacy, Code). MCP: `~/.warp/.mcp.json`. Routers: `~/.warp/custom_model_routers/`. Env var `WARP_API_KEY`, `WARP_TUI_DISABLE_AUTOUPDATE`, `WARP_SKILL_DIRS`.
- Status: GA
- Evidence: https://docs.warp.dev/agents/cli/configuration/ ; .../reference/ via agents.txt
- Notes: no precedence document across app, CLI, admin and project levels was found beyond rules precedence and "admin enforced overrides user".

### Migration guides
- What: guides for moving from Claude Code, Cursor, Ghostty, iTerm2 (built-in importer), macOS Terminal, VS Code terminal, Windows Terminal.
- Status: GA
- Evidence: https://docs.warp.dev/getting-started/migrate-to-warp/ via getting-started.txt

### Area 21. Security and privacy

### Telemetry controls
- What: two toggles: "Help improve Warp" (analytics via Rudderstack, plus AI data) and crash reports (Sentry). Disabled state is signalled by an `X-Warp-Telemetry-Enabled` header. Business and Enterprise covered by ZDR, with no AI or console data collected. Telemetry payloads have built-in redaction regardless of the secret-redaction setting. A full event table is published. Account deletion from Settings > Privacy.
- Status: GA
- Evidence: https://docs.warp.dev/support-and-community/privacy-and-security/privacy/ via support.txt

### Secret redaction
- What: regex-based masking of secrets, passwords, API keys and PII before terminal or AI content is sent to Warp servers or LLM providers; recommended pattern list (IPs, phone numbers, AWS/GitHub/Stripe/OpenAI/Anthropic keys, JWTs) plus custom patterns; off by default; shown struck through or as asterisks in the UI; does not apply to session sharing. Enterprise can enforce it team-wide. Cloud platform doc says known secret values are redacted at output boundaries and that this is a backstop, not a replacement for narrow scopes.
- Status: GA, off by default
- Evidence: https://docs.warp.dev/support-and-community/privacy-and-security/secret-redaction/ ; https://docs.warp.dev/knowledge-and-collaboration/admin-panel/ ; https://docs.warp.dev/platform/architecture/
- Notes: off by default for individuals is a point of contrast with a "no secret in a log line" rule.

### Cloud data boundaries
- What: Warp-hosted clones are destroyed with the sandbox; Warp states it does not persistently store or train on code; code placed in prompts, transcripts or artifacts transits Warp and may persist as run data; inference credentials never enter the sandbox for Warp Agent and BYO LLM; command-based MCP credentials live in the execution environment, URL-based ones with Warp's proxy; all traffic passes the control plane (prompts, tool outputs, MCP calls, transcripts, model requests).
- Status: GA
- Evidence: https://docs.warp.dev/platform/architecture/
- Notes: `vendor-claim` for non-storage and non-training. (verified in review: /platform/architecture/ says Warp operates the control plane and routes inference, Claude Code and Codex call providers directly, URL-backed MCP via Warp's proxy, redaction is "a backstop, not a substitute", and Warp does not persistently store clones or train on code while prompts, transcripts and artifacts may persist)

### Auth
- What: browser device-code login for the CLI; API keys of three kinds; short-lived OIDC federation tokens for cloud providers; SSO for teams.
- Status: GA
- Evidence: https://docs.warp.dev/agents/cli/quickstart/ ; .../oz-cli/api-keys/ ; .../oz-cli/federate/

### Area 22. Licence, pricing and distribution

### Licence and distribution
- What: client AGPL v3 (UI crates MIT); packages for macOS (Homebrew cask), Windows (winget), Linux (deb, rpm, Arch, openSUSE, AppImage) and `oz` via Homebrew cask and apt/yum/pacman. A Preview build channel exists.
- Status: GA
- Evidence: https://docs.warp.dev/getting-started/quickstart/installation-and-setup/ via getting-started.txt ; https://docs.warp.dev/agents/cli/oz-cli/ ; repo README

### Plans and pricing
- What: Free, Build, Max, Business, Enterprise. Paid self-serve plans include monthly usage per seat (resets monthly, no rollover); Free has no bundled agent usage; purchased usage rolls over and expires after 12 months. Platform usage billed at $3 per agent hour on the dollar-billed tiers; hosted compute metered per minute by vCPU or half memory (Linux $0.0015 per minute per unit, Windows twice that). Three cost components: inference, compute, platform. Self-hosting needs Enterprise; BYOK size restriction as above.
- Status: GA
- Evidence: https://docs.warp.dev/support-and-community/plans-and-billing/ and subpages via support.txt
- Notes: figures dated 2026-11-10; the support.txt summary said exact tier prices live on the pricing page, not read.

### Area 23. Anything else

### Oz web app and migration to Factories
- What: the Oz web app remains for standalone cloud-agent workflows; Factories is the newer web experience. Packages renamed (`warp-platform-sdk`) while repos keep `oz-*` names.
- Status: GA / early access mix
- Evidence: https://docs.warp.dev/platform/transitioning-from-oz/ ("last updated Nov 9, 2026")

### Federation of conversation data into the cloud
- What: conversation sync is opt-in ("Store AI conversations in the cloud") but required for local-to-cloud handoff.
- Status: GA
- Evidence: https://docs.warp.dev/agents/local-agents/cloud-conversations/ ; https://docs.warp.dev/platform/handoff/local-to-cloud/

## 3. Distinctive or unusual

- Agent-hosting terminal: Warp hosts third-party CLI agents (Claude Code, Codex, OpenCode, Amp, Gemini, Kiro, others) with a rich input editor, review-comment injection and notifications, i.e. it is a harness for harnesses (agents/cli-agents).
- Cross-harness orchestration with a durable inbox-per-agent message bus, including children that run other harnesses and wake on messages (platform/orchestration).
- Cross-harness Agent Memory (personal, agent, team stores) shared by Warp Agent, Claude Code and Codex (research preview).
- Local-to-cloud handoff with workspace snapshot of uncommitted and untracked files, plus periodic cloud checkpoints (platform/handoff).
- Regex denylist that overrides "Always allow", but auto-approve bypasses the user denylist by default; admin denylist is non-bypassable.
- Skills read from ten tools' directories (`.claude`, `.codex`, `.cursor`, ...), a deliberate compatibility layer.
- Custom model routers by complexity or natural-language rules in YAML.
- Factories: definitions as code in a customer repo reviewed by PR, foreman agent, never merges itself.
- Short-lived OIDC identity tokens issued inside agent runs for AWS and GCP.
- Computer-use screenshots and videos attached to PRs with an admin-controlled sharing mode.
- Agent-driven open-source contribution model (agents code, humans specify and verify) in the OSS repo.

## 4. Architecture and UX notes

- Five layers: clients, APIs, a Warp-operated control plane (run coordination, shared config, model routing, run history), a data plane (transcripts, artifacts, attachments in Warp-managed storage), an execution plane (Warp-hosted sandbox or customer infrastructure). All clients (app, CLI, web, dashboard, Factory MCP) use the same control-plane APIs. (platform/architecture)
- Self-hosted worker connects outbound only; control plane pushes the task, resolved config and scoped runtime credentials down that connection. Model traffic for the Warp Agent still routes through Warp, unless team-managed keys or BYO LLM are used; Claude Code and Codex call their providers directly from the worker.
- Local agent loop: runs in the desktop app or the `warp` CLI process (CLI "manages its own PTY"); the loop location for the app was not described beyond that. Whether model calls from local runs always go via Warp's servers (non-BYOK): consistent with ZDR statements, but the page does not spell out a proxy for local runs, so treat as unverified.
- Run record is the unit: every trigger yields a run with state, inputs and provenance; parent and child runs are separate records.
- Agent identity: named agents, service accounts via agent API keys, per-plan agent count limits.

## 5. Relevance to a self-hosted master/node harness

- Control plane / execution plane split with an outbound-only worker (Self-hosted execution) is the same shape as master and node; Warp's choice of outbound connection and run-scoped credentials sent per task is worth comparing with the node link. Its weakness for the reader: the control plane still sees prompts and transcripts.
- Backend variety for a worker (Docker, Kubernetes Job, Direct, Command, one-shot) shows what node operators ask for; a "Command" backend that hands tasks to an external job runner is cheap to offer.
- Per-agent inbox message bus with lifecycle events and one-level depth (Multi-agent orchestration) is a concrete, limited design to compare with sub-agent plans; note its explicit advice of one worktree per agent.
- Profiles plus regex allow and deny lists, with admin-enforced non-bypassable denylist (Profiles, Admin Panel): shows the node-decides principle in product form, but Warp's user-level auto-approve bypasses the denylist by default, a pattern to avoid.
- Handoff snapshots of uncommitted changes (Handoff local/cloud) solve moving a session between machines; relevant to node migration and resume.
- Skills compatibility across ten tools' directories (Skills) and `AGENTS.md` as the default rules file (Rules) are low-cost interoperability to copy.
- Secret redaction off by default and not applied to session sharing (Secret redaction) is a counter-example to the "no secret in an event" invariant; the architecture page's "backstop, not replacement" framing is honest and quotable.
- Run artifacts, screenshots and recordings with link-only default sharing (Artifacts in PRs) is a model for an events-plus-files UI.
- OIDC token issuance inside runs (Federated identity) avoids storing cloud keys on nodes.
- AGPL means ideas only, per CLAUDE.md; docs-level behaviour (regex lists, orchestration states, handoff) is safe to learn from.

## 6. Overhyped, stale or contradictory

- Naming churn: "Oz" agent renamed "Warp Agent" (changelog 2026.08.13) while the Oz CLI, Oz web app and `oz-*` repos keep the name; CLI docs call `oz` "legacy" yet it is the documented CI and headless route and the transitioning page says it is "current". Treat the migration state as unsettled.
- Marketing "AI-native/agentic development environment" language was avoided in entries; SOC 2, ZDR and "no code stored" are vendor claims.
- Codebase Context page says no code is stored on Warp servers but does not say where embeddings are computed; do not read it as "local".
- Open source: the "open" claim covers the client only; the cloud platform that gives most agent features (Oz, Factories, ZDR model routing) is not described as open anywhere.
- Orchestration page does not label beta while Factories is Early Access and parts of the CLI doc are truncated; maturity of orchestration is unclear.
- Admin Panel page says only the team creator is the admin, yet enterprise pages describe Team Owner, Team Admin and Member roles; the pages may describe different plan tiers.

## 7. Unverified

- Server, control plane and worker licensing: README and blog silent; settle by reading the `oz-agent-worker` repo licence and a Warp statement.
- GitHub tags and release state: scout reports tags stop at `v0.2026.06.09`; not re-checked; settle by reading the releases list.
- Where local (non-cloud) model calls go and whether the client can run fully without Warp servers when using BYOK or custom endpoint (page says AI features can be turned off; login requirement was not read).
- Tool list of the Warp Agent: no public tool reference found.
- Checkpoint or undo for agent file edits: none found.
- Hooks for the Warp Agent: none found.
- ACP support details (only changelog mentions).
- Non-interactive print mode of the `warp` CLI: not seen; `oz agent run` is the headless route.
- Scheduled agent overlap and timezone behaviour: page silent.
- Factory MCP, automations and triggers, factory agents, dashboard, scorers, definition-file formats: only overview read; pages in factories.txt beyond 100k characters not read.
- Parts of agents.txt (last 40k characters), automation-platform.txt (last 20k), enterprise.txt (second 120k, including SSO/SCIM, enforced settings, team-managed keys), terminal.txt (after 100k, including AI command suggestions, session restoration), support.txt and getting-started.txt (second halves), changelog 2026 (after 100k) were not read.
- OpenAPI spec content (`/openapi.yaml`) not opened.
- Interactive Code Review page not read.
- SCIM and audit logs: SCIM named only; audit logs not documented as a feature in what was read.

## 8. Sources

All fetched with WebFetch; section files are summaries of large text dumps, read in 100k-character chunks.

- https://docs.warp.dev/llms.txt : index of section files.
- https://docs.warp.dev/_llms-txt/agents.txt (chunks 0 to 500k): agents, capabilities, CLI, Oz CLI, inference, local agents, CLI agents.
- https://docs.warp.dev/_llms-txt/warp-agent-cli.txt (first 100k): CLI pages.
- https://docs.warp.dev/_llms-txt/automation-platform.txt (chunks 0 to 300k): platform, integrations, orchestration, triggers, handoff.
- https://docs.warp.dev/_llms-txt/factories.txt (first 100k): Factories and API error pages.
- https://docs.warp.dev/_llms-txt/code.txt: editor, code review, worktrees, SSH.
- https://docs.warp.dev/_llms-txt/enterprise.txt (first 100k): enterprise, analytics, BYOLLM, private networking.
- https://docs.warp.dev/_llms-txt/knowledge-and-collaboration.txt: Warp Drive, admin panel, teams.
- https://docs.warp.dev/_llms-txt/terminal.txt (first 100k): terminal pages.
- https://docs.warp.dev/_llms-txt/support.txt (first 100k): pricing, privacy, telemetry, network log.
- https://docs.warp.dev/_llms-txt/getting-started.txt (first 100k): install, migration.
- https://docs.warp.dev/platform/architecture/ : architecture and data boundaries.
- https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ : permissions.
- https://docs.warp.dev/agents/capabilities/planning/ ; .../rules/ ; .../skills/ ; .../codebase-context/ : opened directly.
- https://docs.warp.dev/support-and-community/privacy-and-security/secret-redaction/ : secret redaction.
- https://docs.warp.dev/platform/transitioning-from-oz/ ; https://docs.warp.dev/platform/orchestration/ ; https://docs.warp.dev/platform/triggers/scheduled-agents/ ; https://docs.warp.dev/factories/ ; https://docs.warp.dev/factories/self-hosting/ : opened directly.
- https://docs.warp.dev/knowledge-and-collaboration/admin-panel/ ; https://docs.warp.dev/enterprise/security-and-compliance/security-overview/ : opened directly.
- https://docs.warp.dev/changelog ; https://docs.warp.dev/changelog/2026/ (first 100k): release notes.
- https://www.warp.dev/blog/warp-is-now-open-source : open-source announcement, 2026-04-28.
- https://github.com/warpdotdev/warp : README, licence, build, contribution flow.
- Failed (HTTP 404): https://docs.warp.dev/agents/third-party-cli-agents/ ; https://docs.warp.dev/agents/capabilities/model-choice/ (real path is `/agents/inference/model-choice/`).

## 9. Review

Date: 2026-11-10. Entries checked: 28 of about 95. The reviewer opened each cited page directly (not the section files) and asked for the specific fact.

| Entry | Verdict | Reason |
|---|---|---|
| Identity: open source, licences | confirmed | README: AGPL-3.0 plus MIT for `warpui_core`/`warpui`, 66.0k stars, `master`; blog (2026-04-28) says only "AGPL" and "client"; neither says anything about server or cloud |
| Contribution model, OpenAI sponsor | confirmed | Blog |
| Per-action autonomy levels | confirmed | Four levels; diffs "Agent decides" = "Always ask" |
| Allowlist and denylist | confirmed | Defaults, precedence, admin entries |
| Run until completion bypasses denylist | confirmed-with-correction | Applies to the explicit shortcut, default bypass, switchable; not a general property of Always allow |
| MCP permissions | confirmed | Allowlist, denylist wins, Agent decides |
| Multi-agent orchestration | confirmed-with-correction | Wake-on-message and one-worktree advice not on cited pages; no status label stated |
| Message bus | confirmed | Durable, server-backed, per-agent inbox |
| Cancel parent does not cancel children | confirmed | /multi-agent-runs/ |
| Factories status | status-wrong | Vendor label is "Early Access"; relabelled preview |
| Handoff local/cloud | confirmed-with-correction | "Blocked while command runs/child agents active" not found; rest confirmed |
| Hosted Factory MCP | confirmed-with-correction | Built-in server arrived 2026.08.18, auto-access for Claude Code/Codex 2026.09.09 |
| Oz to Warp Agent rename | confirmed | Changelog 2026.08.13 |
| Oz CLI "legacy" | confirmed-with-correction | oz-cli page says legacy; transitioning page lists it as current; key expiry options not on the page |
| Self-hosted workers and backends | confirmed | Five backends, outbound-only, Enterprise |
| Agent Memory | confirmed | Research preview, stores, defaults |
| Conversation lifecycle | confirmed | 3 hours, 20%, summarize on overflow |
| Architecture and data boundaries | confirmed | Control plane, direct provider calls, proxy MCP |
| Computer use | confirmed | Cloud only, defaults |
| Custom inference endpoint | confirmed | Public URL, no cloud, no Auto |
| Skills | confirmed | Ten directories, `WARP_SKILL_DIRS` |
| Codebase Context | confirmed | 5,000 files, ignore files |
| Self-hosted/legacy `oz` flags (`--harness`, `--attach`) | confirmed | oz-cli page |
| Changelog newest entry 2026.10.22 | confirmed | First entry on /changelog/2026/ |
| Transitioning-from-oz date and package names | confirmed | "Last updated Nov 9, 2026"; page does not explain the rename |
| Scheduled agents, secret redaction, BYOK, pricing, Warp Drive, SSO, analytics API | not checked | Left as written |

Borrowed features: none found. Cross-tool skills directories and AGENTS.md rules are Warp's own documented behaviour.

Notable omissions: not assessed beyond the survey's own admission that large sections (second halves of enterprise.txt, terminal.txt, factories.txt, support.txt; changelog after 100k) were unread. Reviewer did not skim the navigation for further omissions.

Trust judgement: errors found were small and concentrated in secondary details (dates of a changelog item, an unfound sub-claim, a status label); the headline permission, orchestration, self-hosting and open-source claims are accurate. The unchecked remainder looks reliable at that rate; treat second-hand changelog dates and numeric billing figures with care.
