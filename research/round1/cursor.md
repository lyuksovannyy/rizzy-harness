# Cursor (Round 1 public-surface survey)

Surveyed 2026-11-10 from cursor.com docs, help centre, changelog and blog. Closed source: every claim below comes from vendor pages. Doc pages were read through a summarising fetcher, so exact wording and numbers should be re-checked on the page before being relied on. Base URL abbreviation: `D` = https://cursor.com/docs, `H` = https://cursor.com/help.

## 1. Identity

- Name: Cursor. Vendor: Anysphere, acquired by SpaceX (completion announced 2026-08-14 per 00-targets.md; the blog post https://cursor.com/blog/joining-spacex, dated 2026-08-14, says Cursor "has officially been acquired by SpaceX" and that this completes a process begun in April; verified in review. The post says "Cursor", not Anysphere).
- Surfaces: desktop editor (VS Code-based IDE), Agents Window (agent-first workspace, GA with Cursor 3, 2026-04-02), CLI (`agent`), Cloud Agents (formerly Background Agents) in Cursor-managed VMs, Self-Hosted Machines (workers), web agent view (cursor.com/agents), iOS app, Bugbot, Security Agents, Rollouts, Automations, Projects, TypeScript and Python SDKs plus SDK Bridge, Agent SDK, Origin (git hosting), Grok Bot, integrations (Slack, Linear, GitHub, GitLab, Bitbucket, Azure DevOps, Teams, Jira, Notion, JetBrains, Xcode, Tailscale).
- Open or closed: closed. No licence found on docs. Pricing: see area 22.
- Version: no single product version. Desktop references seen: Cursor 3 (2026-04-02), 3.5+, 3.7+, 3.9.8+, 3.11+, 3.23+, 3.25+. CLI newest release v2026.10.22 (https://cursor.com/docs/cli/changelog). Newest changelog entry 2026-11-06 (https://cursor.com/changelog).
- Correction to 00-targets.md: memories question is settled (see area 7). Android is "planned" per the mobile page (the cloud-agent page says Android users can install cursor.com/agents as a PWA; the mobile page does not mention a PWA).

## 2. Features by area

### Area 1. Agent loop and execution model

### Agent (editor, local)
- What: the editor agent runs tool calls against the local workspace; docs state there is no limit on tool calls per task. Messages sent while it runs are queued by default; a "steer" option injects at the next tool call; Cmd+Enter appends to the latest user message.
- Status: GA
- Evidence: https://cursor.com/docs/agent/overview (Agent Overview)
- Notes: loop location for local agent not stated on the page; the SDK page says the local runtime "runs the agent loop in your Node process" with inference on Cursor hosted models (https://cursor.com/docs/sdk/typescript).

### Cloud/managed loop location
- What: for Cloud Agents and Self-Hosted Machines the loop, inference and planning run in Cursor's cloud; only tool execution is placed on the VM or the customer's worker.
- Status: GA (cloud) ; self-hosted release status not stated on the page
- Evidence: https://cursor.com/docs/cloud-agent/self-hosted ; https://cursor.com/blog/self-hosted-machines (dated 2026-09-02) ; https://cursor.com/docs/cloud-agent/self-hosted/choose-runtime
- Notes: see Section 4 for the split in detail. (verified in review)

### Long-lived goal
- What: `/goal [objective]` in the CLI gives the agent an objective to pursue until complete; the agent overview mentions pairing `/goal` with Custom Modes.
- Status: experimental (the CLI page says "rolling out")
- Evidence: https://cursor.com/docs/cli/reference/slash-commands ; https://cursor.com/docs/agent/overview

### Area 2. Built-in tools

### Tool list (editor agent)
- What: search files/folders, web search, fetch rules, read files (images for vision models), edit files, run shell, browser, image generation (saved in `assets/`), ask-questions tool, canvas, and a Task tool for subagents.
- Status: GA
- Evidence: https://cursor.com/docs/agent/overview ; https://cursor.com/docs/agent/tools/canvas ; https://cursor.com/docs/subagents (Task tool referenced)
- Notes: hook matcher tool names are `Shell`, `Read`, `Write`, `Grep`, `Delete`, `Task`, `MCP:<tool>` (https://cursor.com/docs/hooks).

### Canvas
- What: interactive rendered views (sections, stats, tables) beside chat; reopenable and editable; shareable read-only snapshot on paid plans with data-storing privacy mode. Cloud agents can build canvases too.
- Status: GA (inferred; no label); Cloud Agent Canvases announced 2026-10-16
- Evidence: https://cursor.com/docs/agent/tools/canvas ; https://cursor.com/changelog

### Area 3. Code editing and file operations

### Direct file edits and protected paths
- What: agent writes files without approval, except config files (workspace settings); writes are immediate, so docs recommend version control. Reads are not approval-gated; `.cursorignore` blocks files from the agent.
- Status: GA
- Evidence: https://cursor.com/docs/agent/security ; https://cursor.com/help/customization/ignore-files
- Notes: `.cursorignore` does not bind terminal commands or MCP tools (page says they may still read ignored files). Defaults already ignore `.env`, `.git/`, lock files.

### Agent Review (local diff review)
- What: dedicated review of local changes, automatically after agent tasks or via `/agent-review` or the Source Control tab; Quick and Deep depths; reads `BUGBOT.md` rules.
- Status: GA (no label)
- Evidence: https://cursor.com/docs/agent/agent-review

### Debug mode
- What: hypothesise causes, insert log statements that post to a local debug server in a Cursor extension, have the user reproduce, analyse logs, apply a targeted fix, then remove instrumentation.
- Status: GA (no label)
- Evidence: https://cursor.com/docs/agent/debug-mode

### Area 4. Terminal and shell integration

### Terminal tool and CLI shell mode
- What: commands run in the user's default terminal profile under the chosen Run Mode; the `CURSOR_AGENT` env var lets shell configs detect agent sessions. CLI has `/shell` Shell Mode, and a sudo prompt that sends the password straight to sudo and not to the model.
- Status: GA
- Evidence: https://cursor.com/docs/agent/tools/terminal ; https://cursor.com/docs/cli/overview ; https://cursor.com/docs/cli/reference/slash-commands
- Notes: docs for background processes/timeouts not found on the terminal page (see Unverified).

### Area 5. Permissions, approvals and sandboxing

### Run Modes
- What: three modes in Settings > Agents > Approvals & Execution. Auto-review (recommended): allowlisted calls run, others run in sandbox if possible, otherwise an LLM classifier on Cursor's backend decides. Allowlist: only allowlisted actions run unprompted (optional sandbox). Run Everything: no sandbox, no classifier. Local agents only; Cloud Agents never ask for approval.
- Status: GA
- Evidence: https://cursor.com/docs/agent/security/run-modes
- Notes: docs themselves say Auto-review is not a security boundary. Classifier named: Gemini 3.5 Flash Lite, fallback Claude 4.5 Haiku; it may do read-only ReadFile/Grep/Glob/ListDir on the user's machine. Plain-English tuning via `allow_instructions` and `block_instructions` in `~/.cursor/permissions.json` or `<project>/.cursor/permissions.json`; team dashboard config overrides both. (verified in review: three modes, recommended default, classifier models, "not a security boundary", runs on Cursor backend, read-only tools on the user's machine, Cloud Agents never ask for approval)

### OS sandbox
- What: terminal commands run read/write in the workspace only, protected paths (`.git/config`, `.git/hooks`, `.vscode`, `.cursorignore`) denied, network restricted. macOS uses Seatbelt; Linux uses Landlock plus seccomp (kernel 6.2+, Landlock v3, unprivileged user namespaces, otherwise falls back to asking approval); AppArmor package needed for remote environments and the CLI when userns restricted. Network modes: allowlist only, allowlist plus defaults (default), allow all. `sandbox.json` user and project files merge; team policy and hardcoded rules win. Env vars `CURSOR_SANDBOX`, `CURSOR_ORIG_UID`, `CURSOR_ORIG_GID`.
- Status: GA (macOS requires Cursor 2.0+)
- Evidence: https://cursor.com/docs/agent/security/run-modes
- Notes: Linux maps the user UID to 0 inside the namespace. Windows sandboxing is not documented on the page (omission in first draft). On Linux the page says an unmet kernel requirement falls back to asking approval; a `bubblewrap` status value also appears in the env-var table without explanation. (corrected in review: added Windows gap and bubblewrap note; macOS Seatbelt and 2.0+, Landlock plus seccomp, kernel 6.2+, Landlock v3, userns, AppArmor, network modes, merge rule, env vars verified)

### Read boundary
- What: "Read access" setting (Cursor 3.23+). System (default) lets the agent read outside the workspace; Workspace requires approval outside it, hides non-workspace paths from sandboxed commands, with a Read Allowlist. Admins can enforce Workspace.
- Status: GA (no label)
- Evidence: https://cursor.com/docs/agent/security/run-modes
- Notes: (verified in review: Cursor 3.23+, System default, Workspace mode)

### Extra protections
- What: Browser Protection, File-Deletion Protection and External-File Protection can force approval even in auto modes.
- Status: GA
- Evidence: https://cursor.com/docs/agent/security/run-modes

### Default security posture and workspace trust
- What: sensitive actions need approval; no arbitrary network requests by default (tools reach GitHub, link fetch, search providers); every MCP connection and each MCP tool call needs approval unless on an MCP allowlist. Workspace Trust is off by default; restricted mode breaks AI features.
- Status: GA
- Evidence: https://cursor.com/docs/agent/security
- Notes: docs call run-mode guardrails "best-effort", not a hard boundary. (verified in review)

### CLI permissions
- What: `Type(pattern)` rules in `allow` and `deny` arrays; types Shell (first token, `command:args`), Read, Write, WebFetch (domains), Mcp (`server:tool`). Deny beats allow. Global `~/.cursor/cli-config.json`, project `.cursor/cli.json`; only permissions can be set per project. `approvalMode` is `allowlist`, `auto-review` or `unrestricted`; `--sandbox enabled|disabled`, `/sandbox`.
- Status: GA
- Evidence: https://cursor.com/docs/cli/reference/permissions ; https://cursor.com/docs/cli/reference/configuration ; https://cursor.com/docs/cli/overview

### Area 6. Context management and compaction

### Summarisation
- What: CLI `/summarize` (alias `/compress`) reduces context; CLI release notes say sessions that hit the context limit summarise and continue, the latest user request is kept verbatim, and on failure the earlier conversation is kept. A `preCompact` hook exists (observation only). Cloud and subagent models allow choosing context window size on supported models.
- Status: GA (no label)
- Evidence: https://cursor.com/docs/cli/reference/slash-commands ; https://cursor.com/docs/cli/changelog (v2026.10.08, v2026.09.22) ; https://cursor.com/docs/hooks ; https://cursor.com/docs/cloud-agent
- Notes: no dedicated compaction doc page found. (partly verified in review: v2026.10.08 summarise-and-continue and verbatim last request)

### @-mentions
- What: `@` attaches files, folders, terminals, previous chats, git diffs (uncommitted or branch vs main), browser content. The agent otherwise finds files via its own search.
- Status: GA
- Evidence: https://cursor.com/help/customization/context
- Notes: codebase indexing and embeddings are NOT covered on that page (see Unverified).

### Max Mode
- What: larger context window; available only on legacy request-based plans, billed at API rate plus 20%.
- Status: GA, legacy-only
- Evidence: https://cursor.com/docs/models-and-pricing
- Notes: (verified in review)

### Area 7. Memory and persistent instructions

### Rules
- What: Project Rules (`.cursor/rules/*.mdc` with `description`, `globs`, `alwaysApply` frontmatter; plain `.md` ignored), User Rules (Customize > Rules), Team Rules (dashboard, Team and Enterprise plans, enforceable). Four application modes: always, intelligent (by description), by glob, manual (@-mention). Precedence on conflict: Team, then Project, then User. Docs recommend rules under 500 lines. Rules do not affect Tab.
- Status: GA
- Evidence: https://cursor.com/docs/rules
- Notes: (verified in review)

### AGENTS.md
- What: plain markdown in project root or subdirectories, no frontmatter; nested files scope to their directory, more specific wins. Cloud setup docs suggest a "Cursor Cloud specific instructions" section.
- Status: GA
- Evidence: https://cursor.com/docs/rules ; https://cursor.com/docs/cloud-agent/setup

### Memories (editor agent): settled as UI removed, status unclear
- What: Memories launched in Cursor 1.0 as a beta (changelog entry dated 2025-06-04, verified in review at https://cursor.com/changelog/1-0); I could not find a Memories page in the current docs index, and the current docs rules page does not mention them. A forum reply dated 2026-01-10 from user deanrie (not labelled staff on the page) says only the management UI was removed in 2.1.x and the feature still exists; other posters on the thread believe it is gone or report the agent saying the tool does not exist. The "remember..." and "Delete all memories" chat commands and the "Export memories" to `.mdc` advice are not on the page I opened and are unverified.
- Status: unclear (UI removed per a community reply; not in docs)
- Evidence: https://forum.cursor.com/t/memory-tool-dissapeared/148405 (opened; one reply attributed to "deanrie", not explicitly labelled staff on the page) ; https://forum.cursor.com/t/memories-not-showing/143820 and https://forum.cursor.com/t/are-my-memories-gone/144057 (seen only in search summaries, not opened) ; https://cursor.com/docs/rules (no mention)
- Notes: `third-party`/community sources only; no official docs page or changelog removal entry found. Treat as: not a documented feature today. (corrected in review: the reply was not shown as staff; the two other threads were not opened, so their claims are unverified)

### Automation memory
- What: automations keep notes across runs of the same automation, default `MEMORIES.md`, on by default, viewable, editable, disableable. Docs warn untrusted input can poison them.
- Status: GA (no label)
- Evidence: https://cursor.com/docs/cloud-agent/automations

### Agent SDK memory
- What: a hook that appends one JSON record per turn to `memory/journal.jsonl` (rotated to `journal-*.jsonl`); no recall tool, the model reads the journal with normal file tools when instructions say so. One journal shared across all sessions and users; docs say treat records as untrusted. Pluggable `MemoryBackend`.
- Status: unclear (Agent SDK has no status label; announced 2026-10-20)
- Evidence: https://cursor.com/docs/agent-sdk/reference/memory

### Grok Bot memory
- What: each named Bot keeps its own memory, preferences and summaries across sessions; Bots share one cloud computer.
- Status: unclear
- Evidence: https://cursor.com/docs/grok-bot

### Area 8. Planning, tasks and modes

### Modes: Agent, Ask, Plan, Debug, Custom
- What: CLI has Agent (default), Plan, Ask (read-only); Debug via `/debug`. Custom Modes can be backed by a skill. Mode picker or Shift+Tab switches.
- Status: GA
- Evidence: https://cursor.com/docs/cli/overview ; https://cursor.com/docs/cli/reference/slash-commands ; https://cursor.com/docs/skills ; https://cursor.com/docs/agent/plan-mode

### Plan mode
- What: agent asks clarifying questions, researches the codebase and writes an editable plan before any code; user clicks to build. Plans save to the home directory by default; "Save to workspace" moves them in. May suggest itself on complex prompts. Plan can run "Build in Parallel", running independent steps at once.
- Status: GA
- Evidence: https://cursor.com/docs/agent/plan-mode ; https://cursor.com/help/ai-features/multi-agent
- Notes: first appears in the changelog in 2.3 (2025-12-22) and CLI 2026-01-16 per a summary of https://cursor.com/changelog/page/13.

### Projects (coordinator agent)
- What: a Project in the Agents Window is steered via a coordinator agent that plans and delegates to cloud agents without writing code itself; shared context; subscriptions (Slack channel, schedule). Not on Free, Plus, legacy Hobby, or Privacy Mode (Legacy).
- Status: unclear
- Evidence: https://cursor.com/docs/agent/projects

### Area 9. Sub-agents and multi-agent orchestration

### Subagents
- What: each runs in its own clean context and returns only a final summary. Built-in Explore (faster model, parallel searches) and Browser. Custom ones are markdown with frontmatter (`name`, `description`, `model` default `inherit`, `readonly`, `is_background`) in `.cursor/agents/`, `.claude/agents/`, `.codex/agents/` (project and `~/`). Foreground blocks parent; background writes output to `~/.cursor/subagents/` and can be resumed by ID. Since 2.5 a subagent can launch children but only one level deep (main agent plus direct subagents may launch). Parallel via multiple Task calls. Cloud subagents get their own VM and branch.
- Status: GA
- Evidence: https://cursor.com/docs/subagents
- Notes: on legacy request-based plans without Max Mode, subagents use Composer regardless of `model`. (verified in review)

### Parallel agents and best-of-n
- What: `/multitask` runs async subagents in parallel; `/best-of-n` runs one task across several models, each in its own worktree; comparing does not merge.
- Status: GA (no label)
- Evidence: https://cursor.com/help/ai-features/multi-agent ; https://cursor.com/docs/configuration/worktrees

### Agent-to-agent (Agent SDK peers)
- What: a peer is an independent deployed agent reachable via MCP tools `ask`, `check`, `call_tool`; `ask` returns `{status, sessionId, reply}`. Routing is one-way, declared in the caller's instructions. Distinct from subagents.
- Status: unclear
- Evidence: https://cursor.com/docs/agent-sdk/guides/agent-to-agent

### Area 10. Background, long-running and scheduled work

### Cloud Agents
- What: agents in isolated cloud VMs with a full dev environment; many in parallel; keep running when the user's computer is off; push work to a separate branch and PR. Started from desktop (Cloud option), iOS, cursor.com/agents, Slack, Linear, `@cursor` on GitHub or Bitbucket PRs/issues, API, CLI (`&` prefix). Formerly "Background Agents".
- Status: GA; requires Premium or higher, Teams, Enterprise or a legacy paid plan (Free and Plus excluded)
- Evidence: https://cursor.com/docs/cloud-agent ; https://cursor.com/docs/cli/overview ; https://cursor.com/docs/models-and-pricing
- Notes: Feb 2026 changelog entry calls "Long-running Agents" a research preview for Ultra, Teams, Enterprise (summary of https://cursor.com/changelog/page/13). (verified in review: Free and Plus excluded, formerly Background Agents, @cursor on GitHub and Bitbucket; CLI & prefix not found on the cloud-agent page)

### Automations
- What: cloud agents run on triggers: cron schedule (may start late, never early), webhook (private endpoint plus API key), GitHub, GitLab, Bitbucket Cloud, Slack (public channels only), Linear, PagerDuty, Sentry. Tools on by default: PR creation, computer use; optional: PR comments, request reviewers, Slack send/read, MCP, memories. Environments can be none, single repo or multi-repo. Billed as cloud agent usage, to the creator or a team service account. Always uses the model's max context.
- Status: GA (no label; Premium and above per the plans page)
- Evidence: https://cursor.com/docs/cloud-agent/automations ; https://cursor.com/docs/models-and-pricing

### Security Agents, Approval agent, Rollouts
- What: Security Review (PR events) and Vulnerability Remediation (cron) run on the Automations platform; findings flow flagged, verified, tests PR, fix PR; Cursor never merges. PR Routing and Approval requests reviewers and may approve low-risk PRs under `APPROVAL_POLICY.md` files (GitHub and Origin only). Rollouts (Teams, Enterprise) writes a rollout plan on PR open then checks telemetry (needs e.g. Datadog) at deploy, 20 min, 1 h, 1 d, 3 d; it opens issues but never merges or rolls back.
- Status: unclear (no labels)
- Evidence: https://cursor.com/docs/security-agents ; https://cursor.com/docs/approval-agents (page titled around "PR Routing & Approval") ; https://cursor.com/docs/rollouts

### Area 11. Sessions: persistence, resume, fork, checkpoints, undo

### Checkpoints
- What: created automatically before significant changes; restore reverts files only, messages stay; stored locally, separate from Git.
- Status: GA
- Evidence: https://cursor.com/docs/agent/overview

### CLI sessions
- What: `agent ls`, `agent resume`, `--continue`, `--resume=<id>`; slash commands `/resume`, `/fork`, `/rewind`, `/rename`, `/clear`.
- Status: GA
- Evidence: https://cursor.com/docs/cli/overview ; https://cursor.com/docs/cli/reference/slash-commands

### Local/cloud handoff and Remote Control
- What: move a local agent to cloud (Jan 2026 changelog "Handoff to Cloud Agents") or back to local; `/in-cloud` and `/autopilot` hand work to a cloud subagent; `/remote-control` lets a phone drive the computer (Cursor 3.9.8+, Agents Window, admin enable on Team/Enterprise).
- Status: GA (no label)
- Evidence: https://cursor.com/docs/agent/agents-window ; https://cursor.com/docs/cloud-agent/mobile

### Cloud transcripts and retention
- What: conversation state kept indefinitely by default (deletable via the Delete Agent API; Enterprise can cap); live VM recycled after idle; VM snapshots (include cloned code) deleted after 30 days of inactivity and cannot be deleted on demand.
- Status: GA
- Evidence: https://cursor.com/docs/cloud-agent/security
- Notes: contradicts a statement elsewhere (see Section 6). (verified in review: indefinite by default, 30-day snapshot inactivity, snapshots not deletable on demand)

### Area 12. Git and version control

### Worktrees
- What: Agents Window gives each agent its own checkout; IDE uses `/worktree`, `/apply-worktree`. `.cursor/worktrees.json` keys `setup-worktree-unix`, `-windows`, `setup-worktree` run setup commands (`$ROOT_WORKTREE_PATH` available). Cleanup (3.5+) via `cursor.worktreeCleanupIntervalHours` and a machine-wide cap `cursor.worktreeMaxCount`, default 25, also deletes worktrees made outside Cursor.
- Status: GA
- Evidence: https://cursor.com/docs/configuration/worktrees

### Bugbot
- What: automated PR review on GitHub (incl. Enterprise Server), GitLab (incl. self-hosted), Bitbucket (incl. Data Center), Azure DevOps Services, Origin. Runs on each PR update or on `cursor review`/`bugbot run` comments. Rules from team rules, repo rules and `.cursor/BUGBOT.md` (nested), plus learned rules from merged/closed PRs and `@cursor remember`; caps 30,000 chars per rule and 100,000 combined; `.mdc` rules do not apply. Config `.cursor/config/bugbot.yaml` read from the base branch so a PR cannot change its own review settings. Autofix launches a Cloud Agent (max three attempts per PR). Check status defaults to neutral. Usage-based billing; MCP tools for Bugbot on Team/Enterprise only.
- Status: GA
- Evidence: https://cursor.com/docs/bugbot
- Notes: (verified in review; also: config files over 64 KB are ignored and Create New Branch autofix is GitHub and Origin only)

### Signed commits and attribution
- What: cloud agent commits are signed with an HSM-backed Ed25519 key and show Verified on GitHub. CLI attributes commits and PRs to the agent by default (`attribution.*`, default true).
- Status: GA
- Evidence: https://cursor.com/docs/cloud-agent/security ; https://cursor.com/docs/cli/reference/configuration
- Notes: signing is a vendor statement.

### Origin
- What: Cursor's own Git hosting with CLI, PRs, stacks, app marketplace, mirror from GitHub.
- Status: unclear
- Evidence: https://cursor.com/llms.txt (index only); https://cursor.com/changelog (Origin App Marketplace, 2026-10-13)
- Notes: pages themselves not opened.

### Area 13. Extensibility

### MCP
- What: stdio, SSE and Streamable HTTP; `.cursor/mcp.json` (project) and `~/.cursor/mcp.json` with `${env:NAME}`, `${workspaceFolder}` interpolation; OAuth code flow with PKCE, dynamic registration or static client id; elicitation; MCP Apps (interactive UI). Enterprise allowlists by command, URL and tool. Cloud agents: only HTTP and stdio (not SSE or `mcp-remote`); HTTP keeps credentials out of the VM; stdio runs inside the VM; OAuth per user.
- Status: GA
- Evidence: https://cursor.com/docs/mcp ; https://cursor.com/docs/cloud-agent/capabilities
- Notes: ACP sessions load project and user MCP config but not team dashboard servers (https://cursor.com/docs/cli/acp).

### Hooks
- What: spawned processes read JSON on stdin and write JSON on stdout; exit 0 uses output, exit 2 blocks, other codes fail open unless `failClosed`. Events: `sessionStart/End`, `preToolUse`, `postToolUse`, `postToolUseFailure`, `subagentStart/Stop`, `before/afterShellExecution`, `before/afterMCPExecution`, `beforeReadFile`, `afterFileEdit`, `beforeSubmitPrompt`, `preCompact`, `stop`, `afterAgentResponse`, `afterAgentThought`, Tab hooks, `workspaceOpen`. Matchers are regex. Sources by priority: enterprise MDM file, team (Enterprise, dashboard, sync about every 30 min, no `failClosed`/`matcher`), project, user, then plugins (plugins run command hooks only); for permission decisions any deny beats ask beats allow regardless of source, but for other fields the last response wins, so a lower-priority source can override a higher one. Prompt-based hooks (LLM returns `{ok, reason}`) are not available in cloud agents. Loads Claude Code hooks; `CLAUDE_PROJECT_DIR` aliases `CURSOR_PROJECT_DIR`; default `loop_limit` 5.
- Status: GA
- Evidence: https://cursor.com/docs/hooks
- Notes: (corrected in review: precedence and conflict rule; exit codes, failClosed, 30-minute team sync, no per-hook options on dashboard hooks, cloud agents command hooks only, loop_limit 5 and event list verified)

### Skills
- What: folder with `SKILL.md` (agentskills.io standard) in `.agents/skills/`, `.cursor/skills/`, user equivalents, plus `.claude/skills/` and `.codex/skills/`. Auto-applied by description, `/name` manual, `disable-model-invocation`, `paths` scoping. Cloud agents only get synced `~/.cursor/skills/`.
- Status: GA
- Evidence: https://cursor.com/docs/skills

### Plugins and marketplace
- What: bundles of rules, skills, agents, commands, MCP servers and hooks (`.cursor-plugin/plugin.json`; also the open Agent Plugins `plugin.json`). Marketplace listings manually reviewed, must be open source, updates reviewed. Team marketplaces on Teams (one plus Default) and Enterprise (unlimited), with Default Off/On/Required per plugin.
- Status: GA (no label)
- Evidence: https://cursor.com/docs/plugins

### Custom subagents, rules, commands
- What: see areas 7 and 9. A `.cursor/commands` format was not documented on pages I opened (plugins reference "commands").
- Status: unclear
- Evidence: https://cursor.com/docs/plugins

### SDKs
- What: `@cursor/sdk` (Node 22.13+) and Python `cursor-sdk`. `Agent.create` with `local` (loop in your process, inference on Cursor) or `cloud` runtime; `send`, `stream`, `wait`, `resume`, custom tools, MCP, subagents, hooks, local sandbox. SDK Bridge is a loopback Connect/protobuf (`sdk.v1`) server for other languages, per-process bearer token, not classic gRPC.
- Status: unclear (the TypeScript page has no status label)
- Evidence: https://cursor.com/docs/sdk/typescript ; https://cursor.com/docs/sdk/bridge

### Agent SDK
- What: define agents as code folders (`bot/` Markdown plus TypeScript), evals, channels, schedules, hooks, deploy to Cursor hosting or own infrastructure; Cursor says the same harness powers Bugbot and Security Review. Npm package renamed (`@cursor/bdk` then `@grok/agent-sdk`).
- Status: unclear (announced 2026-10-20)
- Evidence: https://cursor.com/docs/agent-sdk ; https://cursor.com/changelog

### Cloud Agents API and webhooks
- What: v1 REST (create agent, runs, SSE stream with `Last-Event-ID`, follow-up or `delivery: "steer"`, cancel, artifacts via 15-minute presigned URLs, environments, secrets, worker pools). Basic or Bearer auth. Webhooks: v1 "coming soon"; the legacy v0 webhook fires only `statusChange` (ERROR or FINISHED), HMAC-SHA256 signed.
- Status: beta (v1 "Public beta"; `envVars` beta; steering returns 403 if not enabled)
- Evidence: https://cursor.com/docs/cloud-agent/api/endpoints ; https://cursor.com/docs/cloud-agent/api/webhooks
- Notes: (partly verified in review: v1 public beta, 15-minute presigned URLs, 403 steering)

### ACP server
- What: `agent acp` speaks JSON-RPC 2.0 over stdio, auth method `cursor_login`, modes agent/plan/ask, used by JetBrains, Neovim (avante.nvim), Zed.
- Status: unclear (no label)
- Evidence: https://cursor.com/docs/cli/acp

### Deeplinks and other integrations
- What: deeplinks reference exists; Notion, Jira, Teams, Azure DevOps, JetBrains, Xcode, Tailscale integration pages exist.
- Status: unclear
- Evidence: https://cursor.com/llms.txt (index only, pages not opened)

### Area 14. Models, providers and inference

### Model lineup and Cursor models
- What: Cursor-labelled models are Grok 4.7/4.6/4.5 and Composer 2.5 (weekly pool); third parties include Claude, GPT-5.x/5.6, Gemini, Kimi, GLM, DeepSeek, Meta Muse. Many third-party models hidden by default. Model docs pages for each exist.
- Status: GA
- Evidence: https://cursor.com/docs/models-and-pricing ; https://cursor.com/llms.txt
- Notes: Claude Fable 5/5.1/5.2: Anthropic retains inputs and outputs for harm review; Enterprise/Privacy Mode customers need admin approval (https://cursor.com/docs/enterprise/privacy-and-data-governance).

### Auto and Cursor Router
- What: Auto has Cost (classic), Balance, Intelligence. Balance and Intelligence use a classifier router, only on Teams and Enterprise (Enterprise: off by default, admin enables; Grok 4.6 must be enabled).
- Status: GA (no label)
- Evidence: https://cursor.com/docs/cursor-router ; https://cursor.com/docs/models-and-pricing

### BYOK
- What: not available on Plus; on Teams and Enterprise the Cursor Token Rate applies to BYOK usage with an exemption for cheap-cache models.
- Status: GA
- Evidence: https://cursor.com/docs/models-and-pricing
- Notes: page is brief; Bedrock configuration exists in the CLI (`/bedrock`, feature-flagged). (verified in review)

### Area 15. UI surfaces

### Surfaces
- What: IDE; Agents Window (multi-workspace, diffs view, cloud handoff, worktrees); CLI TUI; web cursor.com/agents; iOS app (iOS 26+, Live Activities for up to eight agents, push, Siri voice); Voice Chat (2026-11-06 changelog); Design Mode; Android planned. Web-only tasks: secrets, environments, MCP management, automations, admin.
- Status: GA (Agents Window GA with Cursor 3); iOS GA (no label); Android planned
- Evidence: https://cursor.com/docs/agent/agents-window ; https://cursor.com/docs/cloud-agent/mobile ; https://cursor.com/changelog
- Notes: (partly verified in review: iOS 26+, eight Live Activities, Android planned, /remote-control needs 3.9.8+; the cloud-agent page does describe an Android PWA)

### Area 16. Browser automation and computer use

### Browser tool
- What: web view pane or window controlled by an MCP server shipped as an extension; navigate, click, type, scroll, screenshot, console, network (network monitoring only in the Agent panel for now). Approval modes manual/allowlist/auto-run; per-session auth token and per-tab IDs; storage isolated per workspace; Enterprise origin allowlist (must be enabled by the account team), described as best-effort. A built-in Browser subagent filters noisy DOM output.
- Status: GA
- Evidence: https://cursor.com/docs/agent/tools/browser ; https://cursor.com/docs/subagents

### Computer use
- What: cloud agent VMs have a desktop; agents click, screenshot and drive browsers; user can take over via remote desktop. Self-hosted workers: `--computer-use` (macOS helper app needing Accessibility and Screen Recording; Linux X11 with xdotool and ffmpeg), `--share-desktop` (Linux only; isolated worker-created desktop, input filter, clipboard blocked, pixels over the worker's outbound connection). Enterprise admins can disable it (MCP Configuration).
- Status: GA (no label)
- Evidence: https://cursor.com/docs/cloud-agent/capabilities ; https://cursor.com/docs/cloud-agent/self-hosted/computer-use

### Area 17. Remote, cloud, headless, CI and distributed execution

### Cloud environment setup
- What: resolution order personal environment, team environment, then `.cursor/environment.json` (single-repo only). Saved environments built by agent-led setup, snapshot, or Dockerfile. `install` runs at Build time (idempotent); a successful Build captures disk state only, not processes. Multi-repo environments (up to 300 repos per 2026-10-16 changelog). Builds use the default branch config. Computer use supports Debian/Ubuntu images. Port forwarding, Tailscale (userspace), Cloudflare Tunnel. Cursor-hosted VMs are x86_64 only.
- Status: GA; Environments API beta (per changelog 2026-10-16)
- Evidence: https://cursor.com/docs/cloud-agent/setup ; https://cursor.com/docs/cloud-agent/self-hosted/choose-runtime ; https://cursor.com/changelog

### Cloud secrets
- What: dashboard Secrets tab, exposed as env vars. Types `runtime_secret`, `environment_variable`, `build_secret`; scopes environment, team, personal. Precedence: run `envVars`, environment, personal, team. Max 1,000 per scope. Values never returned. Encrypted with KMS; runtime secrets kept out of transcript, tool output, commits and "never reach the model" (vendor statement). Running agents do not see new secrets. TOTP secrets supported so the agent can generate 2FA codes. OIDC tokens (5 min, RS256, issuer `https://api.cursor.com`, via local Unix socket `CURSOR_AGENT_SOCKET`, 30 mints/min) and AWS role assumption avoid long-lived keys.
- Status: GA
- Evidence: https://cursor.com/docs/cloud-agent/setup ; https://cursor.com/docs/cloud-agent/api/endpoints ; https://cursor.com/docs/cloud-agent/security ; https://cursor.com/docs/cloud-agent/identity
- Notes: (verified in review: three secret types, 1,000 per scope, precedence, never returned, KMS, never reaches the model, 5-minute RS256 OIDC, 30 mints per minute, socket, issuer; one-hour sub-token confirmed on the API page)

### Cloud isolation and egress
- What: Firecracker microVM per agent in a separate AWS account; egress modes allow-all, defaults plus allowlist, allowlist only, set per user, environment or team, Enterprise admins can lock the network policy org-wide (the page does not use the name "Lock Network Access Policy"; the setting name is on the Secrets & Network page, not opened); self-hosted not covered; git egress proxy for IP allowlisting; published IP ranges at https://cursor.com/docs/ips.json. Private connectivity: AWS PrivateLink or Cloudflare Tunnel for GHES, GitLab Enterprise, Artifactory etc (Enterprise, contact sales).
- Status: GA
- Evidence: https://cursor.com/docs/cloud-agent/security ; https://cursor.com/docs/cloud-agent/security-network ; https://cursor.com/docs/cloud-agent/private-connectivity
- Notes: isolation claims are `vendor-claim`.

### Self-Hosted Machines (worker model)
- What: see Section 4 for the full split. Two modes: My Machines (personal credential, user-owned machine) and Team Pools (Enterprise only, service-account key, named pools, controller autoscaling). Worker is `agent worker start`. My Machines needs a team admin to enable Allow Personal Machines or Allow Self-Hosted Machines (self-hosted page).
- Status: unclear (no release label on docs or blog)
- Evidence: https://cursor.com/docs/cloud-agent/self-hosted ; https://cursor.com/docs/cloud-agent/self-hosted/pool ; https://cursor.com/docs/cloud-agent/self-hosted/my-machines ; https://cursor.com/docs/cloud-agent/self-hosted/choose-runtime ; https://cursor.com/docs/cloud-agent/self-hosted/integrations ; https://cursor.com/blog/self-hosted-machines
- Notes: blog dated 2026-09-02; Team Pools Enterprise only. (verified in review: loop, inference and planning in Cursor cloud; one outbound HTTPS connection, no inbound ports; data sent list; four hostnames; 10,000 vs 200/1,000 limit contradiction real; no release label; Team Pools Enterprise-only; blog dated 2026-09-02; 5-minute claim token, 7-day backstop, 3600 s idle default, 20 --worker-dir, one agent per worker)

### Headless CLI and CI
- What: `agent -p` print mode; edits are only proposed unless `--force`/`--yolo`; formats `text`, `json`, `stream-json` (`--stream-partial-output`); `CURSOR_API_KEY`. A GitHub Actions page exists in the index (not opened).
- Status: GA
- Evidence: https://cursor.com/docs/cli/headless ; https://cursor.com/docs/cli/overview

### Integrations: Slack, GitHub, Linear
- What: Slack: `@Cursor` starts or follows up cloud agents, inline options (`repo`, `env`, `branch`, `model`, `autopr`, `pool`, `self_hosted`), posts a short plan first for non-trivial tasks, about 19 Slack scopes, Slack Connect summary toggle. GitHub app: clone, branches, PRs, checks, Actions re-runs, branch protection read; GHES 3.8+; Protected Git Scopes; Cloud Agent IP allow list. Linear: delegate issue to Cursor or `@Cursor` comment; repo chosen by issue text, labels, project, default.
- Status: GA (no label)
- Evidence: https://cursor.com/docs/integrations/slack ; https://cursor.com/docs/integrations/github ; https://cursor.com/docs/integrations/linear
- Notes: the GitHub integration page I read did not document `@cursor` PR comments, though the cloud-agent page lists them.

### Origin and Grok Bot (adjacent products)
- What: Grok Bot gives persistent named Bots on a shared cloud computer with browser, filesystem, terminal; included with paid individual and Teams plans; also routines, secrets, private networks, inference hooks pages exist. Treat as separate product in the same docs.
- Status: unclear
- Evidence: https://cursor.com/docs/grok-bot

### Area 18. Collaboration, teams and enterprise administration

### Admin and enterprise
- What: SSO, SCIM, org groups, pooled usage, service accounts, billing groups, spend limits, Admin/Analytics/AI Code Tracking APIs, audit logs to SIEM/webhook/S3, OpenTelemetry export, MDM-delivered policy (hooks, workspace trust, allowed team IDs), CMEK for Cloud Agent data, Cursor Blame, tenancy restriction. Admin controls over run modes, sandbox network, read policy, model access, MCP allowlists, browser allowlist.
- Status: GA (page titles from index; details only where cited)
- Evidence: https://cursor.com/llms.txt (index) ; https://cursor.com/docs/agent/security/run-modes ; https://cursor.com/docs/hooks ; https://cursor.com/docs/mcp ; https://cursor.com/docs/enterprise/privacy-and-data-governance ; https://cursor.com/docs/cloud-agent/security

### Sharing
- What: share cloud run URLs read-only with teammates; admins can enable team follow-ups; "Allow VM sharing" gives teammates terminal and file access to self-hosted workers when live workspace is enabled.
- Status: GA
- Evidence: https://cursor.com/docs/cloud-agent ; https://cursor.com/docs/cloud-agent/self-hosted

### Area 19. Observability

### Usage and telemetry
- What: usage per run and in total via the API; Analytics API (agent edits, MCP/skills/plan adoption, Bugbot analytics); OpenTelemetry export for enterprise (and an OTel guide for Agent SDK); CLI `/logs`, request-ID copy; Cursor Cloud MCP lets an agent inspect runs, transcripts and setup logs.
- Status: GA (no labels); details beyond index titles not opened
- Evidence: https://cursor.com/docs/cloud-agent/api/endpoints ; https://cursor.com/docs/cloud-agent/capabilities ; https://cursor.com/docs/cli/reference/slash-commands ; https://cursor.com/llms.txt

### Area 20. Configuration and customisation

### Config files
- What: `.cursor/rules/*.mdc`, `AGENTS.md`, `.cursor/mcp.json`, `.cursor/hooks.json`, `.cursor/agents/`, `.cursor/skills/`, `.cursor/worktrees.json`, `.cursor/environment.json`, `.cursor/permissions.json`, `sandbox.json`, `.cursor/cli.json` and `~/.cursor/cli-config.json`, `.cursor/BUGBOT.md`, `.cursor/config/bugbot.yaml`, `.cursorignore`. Reads Claude and Codex folders for skills, subagents and hooks.
- Status: GA
- Evidence: pages cited above (rules, hooks, mcp, subagents, skills, worktrees, run-modes, bugbot, cli configuration)
- Notes: CLI project config only carries permissions.

### Area 21. Security and privacy

### Privacy Mode and data handling
- What: Privacy Mode means no training by Cursor or providers; on by default for Enterprise; most models under zero-data-retention terms; Cloud Agents do not support Privacy Mode (Legacy). Prompts and code context go to model providers. US-only data residency for Enterprise (10% uplift, with exceptions including Bugbot, BYOK, MCPs, some Slack/web-triggered cloud agents). TLS 1.2+ in transit, AES-256 at rest. Cursor staff cannot access agent VM contents (vendor claim).
- Status: GA
- Evidence: https://cursor.com/docs/enterprise/privacy-and-data-governance ; https://cursor.com/docs/cloud-agent/security
- Notes: telemetry and codebase indexing are not covered on those pages. (corrected in review: the Legacy Privacy Mode statement is on the cloud security page, not the privacy page; the privacy page confirms ZDR wording, Enterprise default, 10% US residency uplift, TLS 1.2+, AES-256, temporary repository copy deleted after the run, Anthropic harm-review retention for Fable models)

### Vulnerability reporting
- What: email security-reports@cursor.com, acknowledged within 5 business days.
- Status: GA
- Evidence: https://cursor.com/docs/agent/security
- Notes: (verified in review)

### Area 22. Licence, pricing, distribution

### Plans
- What: Free $0; Plus $8; Premium $30 (adds Cloud Agents, Automations, Bugbot, third-party models); Super $100; Ultra $200; Teams $30/$100/$200 per seat; Enterprise custom (pooled usage, SCIM, Team Pools). One weekly pool for Cursor models shared across editor, CLI, Cloud Agents and Grok. Legacy Pro/Pro+/Ultra/Hobby and request-based plans persist. Team Pools and private connectivity are Enterprise. Closed source.
- Status: GA
- Evidence: https://cursor.com/docs/models-and-pricing
- Notes: prices as stated on the page on 2026-11-10; the page says see cursor.com/pricing for full comparison (not opened). (verified in review: prices and Cloud Agents from Premium up)

### Area 23. Anything else

### Voice chat, Design Mode, Live Apps
- What: realtime voice with agents (2026-11-06; dictation previously "Voice Mode"); cloud agents can build and host internal apps behind Cursor sign-in (2026-11-05); Design Mode in mobile for marking up images.
- Status: unclear
- Evidence: https://cursor.com/changelog ; https://cursor.com/docs/cloud-agent/mobile

## 3. Distinctive or unusual

- Self-hosted workers invert the usual split: loop and inference in the vendor cloud, tools on the customer machine over one outbound HTTPS connection (see Section 4).
- Claim-then-spawn controller: pending requests are queued, a controller claims each one (API with SSE watch), spawns a machine, and the worker connects with a single-claim session token. Warm pool and hibernation (`claimed_offline`, wake timeout) built in. Interesting as a ready-made scheduler design.
- Per-claim OIDC identity tokens: spawn hooks get a nonce-bound JWT naming agent, owner and worker ID, with a separate issuer for claims.
- Bugbot reads its config from the base branch so a PR cannot loosen its own review.
- Auto-review: an LLM classifier on the vendor backend approves or blocks commands, with read-only tools on the user's machine, tuned by plain-English instructions.
- Cross-tool compatibility: loads `.claude/` and `.codex/` agents, skills and Claude Code hooks.
- Hook merging across enterprise, team, project, user, plugin sources: permission deny beats ask beats allow regardless of source; other fields last-response-wins (not simply "most restrictive wins"; corrected in review).
- Agent SDK memory as a plain JSONL journal read with ordinary file tools (no recall tool).
- Rollouts: post-merge monitoring against telemetry at fixed intervals.
- Coordinator "Projects" agent that does not write code, only delegates.

## 4. Architecture and UX notes

Self-hosted split, as documented:
- Cursor runs the agent loop, inference and planning. The customer worker runs file edits, terminal commands, computer-use tools and stdio MCP servers (https://cursor.com/docs/cloud-agent/self-hosted ; blog https://cursor.com/blog/self-hosted-machines).
- Transport: `agent worker start` opens one long-lived outbound HTTPS connection; Cursor sends tool calls over it and the worker returns results for the next inference round. No inbound ports, public IPs or VPN. Proxy via `HTTPS_PROXY`. Health endpoint `/readyz` and a management address exist (CLI changelog v2026.10.17). Workers reconnect after 90 s of silence (v2026.09.28). The wire protocol itself is not documented beyond this (the docs page says so). A CLI option `network.useHttp1ForAgent` switches agent connections from HTTP/2 to HTTP/1.1 with SSE (https://cursor.com/docs/cli/reference/configuration), but the docs do not say this is what the worker uses.
- Outbound hosts: `api2.cursor.sh`, `api2direct.cursor.sh`, `agent-stores.s3.us-east-1.amazonaws.com`, `cloud-agent-artifacts.s3.us-east-1.amazonaws.com`, plus `cursor.blob.core.windows.net` only for live workspace.
- Data leaving the customer network: file contents, terminal output, diffs, screenshots, local MCP results and routing metadata during a run; tool outputs "may contain code"; transcripts may be processed and stored by Cursor; artifacts (screenshots, video, log references) go to Cursor-managed storage unless both artifact hosts are blocked; optional desktop sharing and live-workspace relay. Stays local: full checkout, build cache, machine credentials. Blog states self-hosting does not keep code or transcripts inside the network. Privacy Mode applies.
- Auth: My Machines use the user's login or personal key (or a one-hour `POST /v1/sub-tokens` token); pools use a service-account key; per-claim session tokens (7-day expiry backstop, one claim, one worker ID) keep the key on the controller; claim identity tokens last 5 minutes.
- Routing: requests wait in a named pool until a worker claims; after claim all activity for that chat goes to the same worker. Repo-backed pools bind to repos; any-repo pools use `--clone-git-repos`. A trigger request only reaches machines owned by the triggering user; a wrong-repo request is rejected.
- Worker config applied: `.cursor/hooks.json` in the worker's workspace (plus team and enterprise hooks on Enterprise); `sessionStart`/`sessionEnd` fire on claim and release; dashboard secrets only via `--sync-dashboard-secrets` plus admin enablement; environment-scoped and build secrets never reach workers. stdio MCP runs on the machine, HTTP/SSE MCP runs on Cursor's backend (my-machines page).
- Limits stated: 200 workers per user and 1,000 per team (pool page) versus 10,000 each (self-hosted page): the pages disagree. One agent at a time per worker (pools); at most 20 `--worker-dir`. Idle release default 3600 s. Live workspace Linux only, needs Cursor 3.25+, runs a verified-signature server build on 127.0.0.1.
- Security statement: workers run as the OS user; anyone who can open the agent with live workspace gets terminal/file access.

Other:
- Local editor agent: loop location not stated in docs; the SDK local runtime runs it in the caller's process, with inference at Cursor.
- Local agents, CLI and cloud agents share one backend; Agents Window, web and iOS view the same cloud agents.
- Auto-review classifier runs on Cursor's backend (Run Modes page).

## 5. Relevance to a self-hosted master/node harness

- Cursor's workers (area 17, Self-Hosted Machines) keep the loop at the vendor; rizzy's design keeps the loop on the node. The per-run exposure list (file contents, terminal output, diffs, screenshots) shows exactly what a loop-at-master design leaks; rizzy's inverse avoids it but loses central inference routing.
- Outbound-only long-lived connection with a pool/claim queue (pool page, API endpoints page) is a good fit for nodes behind NAT: nodes dial the master, no inbound ports. Worth copying the claim then single-use session token idea.
- Per-claim scoped credentials and a nonce-bound identity token for spawn hooks (pool page) map to a master that mints a node-scoped token per session instead of handing nodes a long-lived key.
- "Node decides" invariant: Cursor lets worker-side `.cursor/hooks.json` and team/enterprise hooks apply, but Cursor's server drives the tool calls; a hostile server could issue arbitrary tool calls limited only by the OS user. Docs rely on running as an unprivileged OS user and a fresh machine per agent; rizzy's `node.toml` authority is stronger.
- Sandbox design (Run Modes page): Seatbelt on macOS, Landlock plus seccomp on Linux with graceful fallback to approval, protected paths (`.git/config`, `.git/hooks`) and a read boundary. Direct reference for the node's sandbox.
- Hooks (hooks page): JSON over stdin/stdout, exit 2 blocks, layered sources with deny-beats-ask-beats-allow for permissions (other fields last-response-wins), fail-open default with `failClosed` option. Note the fail-open default is weaker than a security tool should have.
- Bugbot reading its settings from the base branch (bugbot page) is a cheap pattern against self-modifying PR config; relevant to any review routine.
- Cloud secrets model (setup and endpoints pages): scoped, never returned, never to the model, injected at start only. Pair with the rule that secrets stay out of events.
- Hibernation and wake (`claimed_offline`) show how to keep session ownership stable across node restarts: a master that needs "one writer per session" can reuse the idea of reclaim by worker ID.
- Auto-review LLM classifier (Run Modes page) sends command context to a vendor backend; for a self-hosted harness this is a data-leak path unless the classifier model is local.

## 6. Overhyped, stale or contradictory

- Worker limits: pool page says 200 per user and 1,000 per team; self-hosted overview says 10,000 each. Contradiction.
- Cloud data retention: the privacy page says Cloud Agents store encrypted repository copies only while running, then delete; the cloud security page says VM snapshots with cloned code are kept 30 days after inactivity and cannot be deleted on demand.
- Memories: agent overview and rules pages are silent; the removal is documented only on community forum posts; two forum replies describe it differently (UI removed vs feature removed).
- Mobile: cloud-agent page suggests Android via PWA; mobile page says Android planned and does not mention a PWA.
- GitHub integration page omits `@cursor` PR/issue mentions that the cloud agent page lists.
- "Cursor never connects into your network" and "meets the security requirements of most teams" are vendor claims; the blog gives no authentication or encryption detail beyond HTTPS.
- Auto-review is described as a classifier "not a security boundary"; marketing recommends it as default.
- Docs note the BYOK and Max Mode pages as brief; many "status" labels absent (SDK, Agent SDK, Projects).

## 7. Unverified

- (Resolved in review) SpaceX acquisition: confirmed at https://cursor.com/blog/joining-spacex.
- Forum threads https://forum.cursor.com/t/memories-not-showing/143820 and https://forum.cursor.com/t/are-my-memories-gone/144057 and the claim that staff advise "Export memories": not opened, unverified (moved here in review).
- Codebase indexing, embeddings, `.cursorindexingignore`: not covered on pages opened. Settle with the indexing docs page if one exists (not in llms.txt index I saw).
- Whether Memories still produce agent-visible memories in current versions: only forum evidence. Settle with a changelog or docs statement, or a test in a current client.
- Terminal background processes, timeouts, output truncation: not on the terminal page.
- Worker wire protocol and encryption details: docs state it is not documented.
- Worker release status (beta/GA): no label found.
- Pages in the index not opened: cli/github-actions, cli/installation and authentication, voice-chat, design-mode, origin/*, grok-bot subpages, enterprise subpages (SSO, SCIM, OTel export, Analytics API), cloud-agent/best-practices, cloud-agent/builds, apps, metadata, agent-sdk reference subpages, `.cursor/commands` format, Third Party Hooks reference.
- Exact CLI release dates (changelog lists versions only).
- Fetch note: pages were summarised by a small model; the cloud API endpoints page was read in two chunks.

## 8. Sources

All opened via the fetcher unless marked.
- https://cursor.com/llms.txt (doc index)
- https://cursor.com/docs/cloud-agent/self-hosted (Self-Hosted: architecture, data flow, setup, limits)
- https://cursor.com/blog/self-hosted-machines (blog, 2026-09-02)
- https://cursor.com/docs/cloud-agent/self-hosted/pool (pools, controller, tokens, limits)
- https://cursor.com/docs/cloud-agent/self-hosted/my-machines
- https://cursor.com/docs/cloud-agent/self-hosted/choose-runtime
- https://cursor.com/docs/cloud-agent/self-hosted/computer-use
- https://cursor.com/docs/cloud-agent/self-hosted/integrations
- https://cursor.com/docs/cloud-agent/api/endpoints (Cloud Agents API, two chunks)
- https://cursor.com/docs/cloud-agent/api/webhooks
- https://cursor.com/docs/cloud-agent (Cloud Agents)
- https://cursor.com/docs/cloud-agent/setup
- https://cursor.com/docs/cloud-agent/security
- https://cursor.com/docs/cloud-agent/security-network
- https://cursor.com/docs/cloud-agent/private-connectivity
- https://cursor.com/docs/cloud-agent/identity
- https://cursor.com/docs/cloud-agent/capabilities
- https://cursor.com/docs/cloud-agent/automations
- https://cursor.com/docs/cloud-agent/mobile
- https://cursor.com/docs/agent/overview
- https://cursor.com/docs/agent/agents-window
- https://cursor.com/docs/agent/plan-mode
- https://cursor.com/docs/agent/security
- https://cursor.com/docs/agent/security/run-modes
- https://cursor.com/docs/agent/tools/browser
- https://cursor.com/docs/agent/tools/terminal
- https://cursor.com/docs/agent/tools/canvas
- https://cursor.com/docs/agent/debug-mode
- https://cursor.com/docs/agent/agent-review
- https://cursor.com/docs/agent/projects
- https://cursor.com/docs/rules
- https://cursor.com/docs/hooks
- https://cursor.com/docs/subagents
- https://cursor.com/docs/configuration/worktrees
- https://cursor.com/docs/mcp
- https://cursor.com/docs/skills
- https://cursor.com/docs/plugins
- https://cursor.com/docs/cli/overview
- https://cursor.com/docs/cli/headless
- https://cursor.com/docs/cli/acp
- https://cursor.com/docs/cli/reference/permissions
- https://cursor.com/docs/cli/reference/slash-commands
- https://cursor.com/docs/cli/reference/configuration
- https://cursor.com/docs/cli/changelog
- https://cursor.com/docs/bugbot
- https://cursor.com/docs/security-agents
- https://cursor.com/docs/approval-agents
- https://cursor.com/docs/rollouts
- https://cursor.com/docs/sdk/typescript
- https://cursor.com/docs/sdk/bridge
- https://cursor.com/docs/agent-sdk
- https://cursor.com/docs/agent-sdk/reference/memory
- https://cursor.com/docs/agent-sdk/guides/agent-to-agent
- https://cursor.com/docs/models-and-pricing
- https://cursor.com/docs/cursor-router
- https://cursor.com/docs/enterprise/privacy-and-data-governance
- https://cursor.com/docs/integrations/slack
- https://cursor.com/docs/integrations/github
- https://cursor.com/docs/integrations/linear
- https://cursor.com/docs/grok-bot
- https://cursor.com/changelog (newest five entries)
- https://cursor.com/changelog/page/13 (older entries, dates for first appearances)
- https://cursor.com/help/customization/context
- https://cursor.com/help/customization/ignore-files
- https://cursor.com/help/ai-features/multi-agent
- https://forum.cursor.com/t/memory-tool-dissapeared/148405 (community forum; memories)
- Seen only as web-search result summaries, not opened: https://forum.cursor.com/t/memories-not-showing/143820 , https://forum.cursor.com/t/are-my-memories-gone/144057

## 9. Review

Reviewed 2026-11-10 by an independent reviewer, public pages only. Entries checked: 28 of about 72 (some partly; marked as such). The author's warning that pages were read through a summarising fetcher applied to my reads as well, so I asked for exact quotes on every fact below.

| Entry | Verdict | Reason |
|---|---|---|
| Identity: SpaceX acquisition (2026-08-14) | confirmed | Blog says Cursor "has officially been acquired by SpaceX"; was marked unverified |
| Self-Hosted Machines: what runs where | confirmed | Loop, inference and planning in Cursor cloud; edits, terminal, computer use, local MCP on the worker |
| Self-hosted connection | confirmed | One long-lived outbound HTTPS connection; no inbound ports, IPs or VPN; `HTTPS_PROXY` |
| Self-hosted data leaving the network | confirmed | File contents, terminal output, diffs, screenshots, local MCP results, routing metadata; transcripts may be stored |
| Self-hosted limits | confirmed | 10,000 per user and team (self-hosted page) vs 200 per user and 1,000 per team (pool page): real contradiction |
| Team Pools plan | confirmed | Enterprise-only; My Machines needs team admin enablement (added) |
| Pool and controller design | confirmed | Claim-then-spawn, warm pool, 5-minute claim token, 7-day backstop, 3,600 s idle, 20 `--worker-dir` |
| Worker release status | confirmed | No beta/GA label on docs, blog or CLI changelog |
| CLI worker details (/readyz, 90 s reconnect, v2026.10.22) | confirmed | CLI changelog v2026.10.17, v2026.09.28; versions only, no dates |
| Run Modes and Auto-review | confirmed | Models, "not a security boundary", Cursor backend, read-only tools |
| OS sandbox | confirmed-with-correction | Windows not documented; bubblewrap status value unexplained |
| Read boundary | confirmed | Cursor 3.23+, System default |
| Hooks sources and precedence | confirmed-with-correction | Not "most restrictive wins": deny>ask>allow for permissions, last response wins otherwise |
| Hooks exit codes and options | confirmed | Exit 2 blocks, fail-open unless `failClosed`, 30-minute team sync, loop_limit 5 |
| Bugbot configuration source | confirmed | `.cursor/config/bugbot.yaml` read from base branch; 64 KB cap added |
| Bugbot caps and autofix | confirmed | 30,000 and 100,000 characters, three attempts |
| Memories (editor) | confirmed-with-correction | 1.0 beta launch confirmed in changelog; the forum reply is not labelled staff; two threads unopened |
| Rules and precedence | confirmed | Team, Project, User; 500 lines; `.mdc` fields; plain `.md` ignored |
| Subagents | confirmed | One level of nesting, frontmatter, directories |
| Cloud transcripts and retention | confirmed | Indefinite; 30-day snapshots; contradiction with privacy page is real in wording |
| Privacy Mode and data handling | confirmed-with-correction | Legacy Privacy Mode statement is on the cloud security page only |
| Cloud isolation and egress | confirmed-with-correction | Firecracker, separate AWS account, signing verified; "Lock Network Access Policy" is not the page's wording |
| Cloud secrets | confirmed | Types, 1,000, precedence, OIDC numbers all match |
| Plans and Max Mode and BYOK | confirmed | Prices and statements match |
| Default security posture; vulnerability reporting | confirmed | Workspace Trust off; 5 business days |
| Mobile / Android | confirmed | Mobile page says planned; cloud-agent page does describe a PWA: the contradiction is real |
| CLI summarisation | confirmed | v2026.10.08 |
| Cloud Agents plans; Cloud Agents API | confirmed | Free and Plus excluded; v1 public beta |
| Cloud Agents CLI `&` prefix | not-supported (not found) | Not on the cloud-agent page; cited to cli/overview, not opened |

Borrowed features: none found. The hooks merge rule is the one place a Claude Code-like "most restrictive" idea was stated where the page says otherwise.

Notable omissions:
- Cursor Tab autocomplete, the product's origin feature, has no entry (https://cursor.com/help/ai-features/tab.md).
- Shared Transcripts (https://cursor.com/help/ai-features/shared-transcripts.md).
- Cursor Blame (https://cursor.com/docs/integrations/cursor-blame.md).
- Cloud Agent Apps and Origin Apps (https://cursor.com/docs/cloud-agent/apps.md, https://cursor.com/docs/origin/apps.md).
- Enterprise documentation hub (https://cursor.com/docs/enterprise.md).
- Search tool page (https://cursor.com/docs/agent/tools/search.md) and codebase indexing, which the author already notes as not found.
- Cursor Router and Composer 2.5 model pages (https://cursor.com/docs/models/cursor-composer-2-5.md).

Trust judgement: the self-hosted worker description, the sandbox, the classifier, Bugbot and the number-heavy secrets and pricing entries are accurate, including the two contradictions the author flagged. One real error (hooks precedence) and a few attribution slips were fixed. The unchecked remainder (integrations, SDK, Agent SDK, Origin, Grok Bot, Rollouts, security agents, automations) is likely reliable but should be treated as lightly verified; the main systematic gap is missing coverage (Tab), not wrong claims.
