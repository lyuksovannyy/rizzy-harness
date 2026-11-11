# Google Antigravity: Round 1 public-surface survey

Surveyed 2026-11-10 from antigravity.google docs (pages opened via WebFetch, which returns model-summarised text, not raw HTML; wording of details such as defaults was taken from those summaries), the Google Developers Blog, and two GitHub repository README pages. No source code read. Closed source: every claim below is a vendor statement unless marked otherwise. The vendor docs are the only evidence for behaviour, so mechanism claims are "as documented", not tested.

Common base URL: `https://antigravity.google/docs/` (abbreviated `D/` below where the URL is unambiguous, always expanded in the Sources section).

## 1. Identity

- Name: Google Antigravity. Vendor: Google (Google DeepMind models, Google Cloud for enterprise). Closed core.
- Surfaces (verified in D/overview, llms.txt, product pages):
  - Antigravity App (desktop, "2.x"; docs say it evolved from the Agent Manager, listed as v2.5+). Changelog latest: v2.31.0, 2026-11-09.
  - Antigravity CLI, binary `agy` (listed v1.2+; changelog shows CLI 1.5.3 on 2026-11-07; the 1.5.5 / 2026-11-10 figure in 00-targets.md was not independently seen by me).
  - Antigravity IDE (standalone desktop editor; no origin/lineage statement on the product page; no built-in browser). Enterprise sign-in is NOT supported in the standalone IDE.
  - IDE extensions: VS Code, Visual Studio, JetBrains, Zed, Xcode, Eclipse (Public Preview), others over ACP. Targets file lists five; the extensions page lists six (Eclipse added).
  - Python SDK (`pip install google-antigravity`, Python 3.10+), plus a separately mentioned managed cloud REST/gRPC API (not documented in what I opened).
  - Remote Agents (Google-managed environment) and Remote Control (browser dashboard to your machine).
- Licence: no licence stated in docs. `google-antigravity/antigravity-cli` GitHub repo is public but contains only README, CHANGELOG, examples, issue templates; no licence file seen; terms apply. `antigravity-sdk-python` is Apache-2.0 but depends on a compiled runtime binary shipped in PyPI wheels (the README says cloning is not sufficient to run it).
- Targets file corrections: (a) Gemini CLI transition post is dated 2026-05-19; the 2026-06-18 date is when consumer service stopped, not when the CLI launched. (b) Surface count: docs overview says four surfaces (App, CLI, IDE extensions, SDK) while llms.txt lists five products (adds IDE). (c) The targets file said the docs index has no memory page: confirmed, and see Section 2 area 7 for what exists instead.

### What happened to Gemini CLI (verified)
- Google Developers Blog post, 2026-05-19: Gemini CLI is "folded into" Antigravity CLI. Antigravity CLI available to everyone from 2026-05-19 (the post does not use the word GA; corrected in review), built in Go, shares the harness with the 2.0 desktop app, keeps skills, hooks, subagents, extensions (extensions become plugins) but "not 1:1 parity".
- 2026-06-18: Gemini CLI and Gemini Code Assist IDE extensions stop serving consumer users (free, Google AI Pro/Ultra). Gemini Code Assist for GitHub: no new installs from that date.
- Enterprise carve-out: Gemini Code Assist Standard/Enterprise licences and Code Assist for GitHub via Google Cloud keep access; Gemini CLI still usable with paid Gemini / Vertex API keys. Antigravity CLI also available for Google Cloud projects.
- The blog post does not state what happens to the open-source repository.
- The repository itself (https://github.com/google-gemini/gemini-cli README, opened): still public, Apache-2.0; its top notice says it is "Enterprise Code Assist only", no free tier or Google AI plans, not tuned for Gemini 4+ and not to be used for benchmarking; keeps weekly stable / weekly preview / nightly channels. Per 00-targets.md it still had releases (v0.67.0, 2026-11-04); confirmed in review on the releases page (v0.67.0 "Latest" on 04 Nov, plus a v0.68.0-preview.0 pre-release the same day); the repository is not archived. The README banner also says Gemini CLI is "not for model benchmarks" and not tuned for the Gemini 4 series (verified in review).
- Migration doc (D/cli/gcli-migration) does not say Gemini CLI is deprecated; it states backward compatibility for GEMINI.md, `~/.gemini/skills/`, MCP config (inline in `~/.gemini/settings.json` must be moved to `mcp_config.json`, `url`/`httpUrl` become `serverUrl`), import of extensions with `agy plugin import gemini`, and OS-keyring token migration. Workspace skills in `.gemini/skills/` are NOT auto-migrated (move to `.agents/skills/`). Custom terminal themes may not carry over.

## 2. Features by area

### Area 1: Agent loop and execution model

### Shared harness across surfaces
- What: Every surface runs one agent harness (stated as co-trained with Gemini models), supporting synchronous turn-by-turn editing, async background subagents (optionally in git worktrees), cron-like sidecars and verification loops. The CLI is Go; the SDK ships a compiled binary and "discovers" it.
- Status: GA (App/CLI); SDK status unclear
- Evidence: https://antigravity.google/docs/overview (Choose your Antigravity); https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/ ; https://github.com/google-antigravity/antigravity-sdk-python
- Notes: "co-trained" and "highly optimized" are vendor-claim. Where the loop executes for local use (inside the app process vs a local daemon) is not documented; Enterprise page mentions a local `language_server.log` for the App, suggesting a language-server-style sidecar process (inference only, unverified).

### /goal (long-running autonomous loop)
- What: Records an objective and installs a stop hook that blocks the agent from ending its turn until it has mapped each deliverable to evidence (files, tests, build logs). One active goal per conversation; no iteration cap; waits for running subagents/background tasks before forcing continuation. Permissions and artifact review still apply.
- Status: GA (App, CLI, IDE)
- Evidence: https://antigravity.google/docs/commands/goal
- Notes: Changelog v1.0.14 (2026-06-30) removed the max limit. Completion is the model's own declaration, so no independent check. (verified in review: stop hook, one goal at a time where a new /goal cancels the previous one, no iteration cap, no forced continuation while dependent subagents/background tasks run, App/CLI/IDE; changelog quote confirmed)

### Slash-command modes: /plan, /grill-me, /boost, /teamwork, /swarm
- What: See areas 8 and 9. Common mechanism: slash commands launch prompt-plus-orchestration recipes.
- Status: see entries
- Evidence: https://antigravity.google/docs/slash-commands
- Notes: The catalog page calls itself "complete" yet omits /agents, /context, /diff etc. (docs lag).

### Area 2: Built-in tools

### Tool inventory (partial, inferred from docs)
- What: No single tool-reference page exists. Tool names appearing in docs: `view_file`, `replace_file_content`, `grep_search`, `run_command`, `invoke_subagent`, `define_subagent`, `call_mcp_tool`, `evaluate_script` (browser), `schedule`, `manage_task`, `manage_inbox`; `/codesearch` command; CLI `/tasks` tracks background processes. Built-in subagents: `research`, `browser`, `self`. Image generation via an unselectable "Nano Banana 2" model.
- Status: unclear
- Evidence: https://antigravity.google/docs/subagents ; https://antigravity.google/docs/commands/browser ; https://antigravity.google/docs/commands/schedule ; https://antigravity.google/docs/sdk/policies ; https://antigravity.google/docs/models ; https://antigravity.google/docs/cli/commands/context (tool lists per category only)
- Notes: names from examples, not a spec. Changelog v1.2.7 says legacy search tools were retired from the default toolset. Nothing documented on web search/fetch tool internals beyond "Network Access Rules"/`read_url`.

### Area 3: Code editing and file operations

### Diff review before write (CLI) and inline diffs
- What: In CLI `default` mode every file create/edit shows an inline diff; accept (1/Enter), reject (2), or Tab to type an alternative instruction; `f` opens full-screen diff. A pending edit cannot be dismissed with Esc. Files outside the workspace add an "outside workspace" reason and an "always allow non-workspace access" option. `accept-edits` and `auto` skip the prompt for workspace edits.
- Status: GA
- Evidence: https://antigravity.google/docs/cli/modes
- Notes: Edit mechanism (patch format, search/replace) not documented; tool `replace_file_content` seen in subagent example.

### App review pane and per-turn diffs
- What: Views Uncommitted, Branch, Agent Edits; per-turn filter from a turn's "N files changed" card; split/stacked, whitespace hiding; line comments in diffs go to the agent; stage/unstage/discard/commit/amend/push in app.
- Status: GA
- Evidence: https://antigravity.google/docs/review-changes ; https://antigravity.google/docs/features
- Notes: Discard of unstaged changes is immediate without confirm and deletes untracked files (as documented).

### Notebooks, PDFs, attachments
- What: Jupyter notebook viewer with inline cell comments, PDF viewer with Comment/Quote, attachments of any file type (v2.28.0).
- Status: GA
- Evidence: https://antigravity.google/docs/features ; https://antigravity.google/docs/changelog
- Notes: App only.

### Area 4: Terminal and shell integration

### Command permission rules (allow/ask/deny)
- What: Every sensitive operation is a resource `action(target)`: `read_file`, `write_file`, `read_url`, `execute_url`, `command`, `mcp`, (legacy `unsandboxed`). Rules in Deny / Ask / Allow lists; precedence Deny > Ask > Allow; explicit rules always beat presets/modes. `command` matches literal word-token prefixes, or `regex:` anchored `^(?:...)$`. Constructs that can hide execution (`$(...)`, backticks, process substitution, brace expansion, non-literal command names, network/fd redirects, flags that run subcommands like `git -c core.pager=`, `tar --to-command`) disable prefix matching for the whole line, requiring an exact or full-regex match. Pipelines, `&&`, `||`, `;`, quoted literals, simple redirects and wrappers (`timeout`, `nohup`, `nice`, `env`) are still prefix-matched per segment. On Windows commands that can't be split cleanly need exact match.
- Status: GA
- Evidence: https://antigravity.google/docs/permissions
- Notes: this is one of the more detailed shell-parsing policies documented by any vendor; the claim of how it is parsed is the docs' only. (verified in review; the page also says a `command(git)` rule lets `git status && git log` run unprompted, and `git log $(whoami)` prompts)

### Terminal sandbox
- What: OS-level isolation of agent shell commands: Linux kernel namespaces (docs elsewhere mention bubblewrap for CLI), macOS `sandbox-exec` Seatbelt, Windows AppContainer (CLI only; needs Windows 11 24H2 plus Aug 2026 update). Writable: project folders, temp, common build caches. Readable: system dirs. Blocked: `~/.ssh`, `.env`. No network by default; `read_url` domains become the outbound allowlist; optional `sandboxProxy` HTTP proxy setting in CLI. No VM or Docker. `read_file`/`write_file` rules populate the read-only/read-write mounts.
- Status: GA for personal accounts; CLI off by default (`enableTerminalSandbox` false, `auto` mode turns it on); App on by default. The earlier claim of a "Preview" label for business/enterprise was not found in review.
- Evidence: https://antigravity.google/docs/sandbox ; https://antigravity.google/docs/permissions ; https://antigravity.google/docs/settings ; https://antigravity.google/docs/enterprise
- Notes: Unavailable where unprivileged user namespaces are blocked (Linux containers), Windows 10; then `proceed-in-sandbox` degrades to `request-review`. Page does not cover resource limits, logging, MCP tools or non-shell tools. Changelog v1.4.6 "protect more credentials", v1.5.1 plugin OAuth token protection in sandbox. (corrected in review: the "Preview" label for business/enterprise was not found on the settings or enterprise pages, so it was removed from Status; the read_file/write_file-to-mounts and read_url-to-network-allowlist wiring is confirmed on the App docs of /permissions and /sandbox, but the CLI docs do not spell it out; bubblewrap is confirmed only on /enterprise ("seatbelt on macOS, bubblewrap on Linux"), not on /sandbox or /cli/modes; the Windows 11 24H2 plus Aug 2026 requirement is confirmed on /sandbox)

### Out-of-sandbox command evaluation ("Auto")
- What: In Auto mode, commands that must leave the sandbox are evaluated automatically; build/test/lint/format and other safe, reversible ones run, risky or irreversible ones prompt. The evaluator's mechanism (separate model call? heuristics?) is not stated; changelog v1.4.9 says the review includes the working directory's repository status; v1.4.8 mentions review timeouts.
- Status: GA in later versions ("Preview" label in app v2.29.0 and earlier); not available to business/enterprise
- Evidence: https://antigravity.google/docs/permissions ; https://antigravity.google/docs/cli/modes ; https://antigravity.google/docs/changelog
- Notes: Requires sandbox, personal account. The "Preview" label and "not available to business/enterprise" detail could not be confirmed on /enterprise (which says nothing about Auto availability); treat as unverified.

### Area 5: Permissions, approvals and sandboxing

### App permission presets
- What: Request Review (sandbox off, always ask, workspace only), Default (sandbox on, allow in sandbox, ask outside), Auto, Turbo (sandbox off, everything allowed, full filesystem). Global, per project (default "Inherit Global"), or from a message-box picker (v2.23.0+). Business/enterprise accounts instead use "previous settings" (terminal command auto-execution Require Review / Proceed in Sandbox / Always Proceed; non-workspace access off by default).
- Status: GA
- Evidence: https://antigravity.google/docs/permissions ; https://antigravity.google/docs/agent-settings
- Notes: Default falls back to Request Review if sandbox unavailable.

### CLI modes and toolPermission
- What: Modes `default`, `accept-edits`, `auto`, `plan`, cycled with Shift+Tab; `toolPermission` `request-review` (default), `proceed-in-sandbox`, `strict`, `always-proceed` (= `--dangerously-skip-permissions`); `strict` and `always-proceed` override `auto`. `allowNonWorkspaceAccess` default off. Rules live in `~/.gemini/antigravity-cli/settings.json` under `permissions.allow/ask/deny`; `/permissions` edits them. Settings file written with mode 0600.
- Status: GA
- Evidence: https://antigravity.google/docs/cli/modes ; https://antigravity.google/docs/permissions ; https://antigravity.google/docs/settings
- Notes: Sources disagree on sandbox default wording: features page says CLI sandbox off by default (enabled by `auto`); consistent with settings page. (corrected in review: /cli/modes does not mention `toolPermission` at all (it speaks of "strict tool permissions" as a global override) and gives `default` as the starting mode; the `request-review` default and value list were not confirmed on that page; `trustedWorkspaces` is confirmed on /settings as "folders you chose to trust", added automatically, default empty, so the Workspace trust entry's "semantics not documented" is out of date)

### Editable approval scope
- What: In an Ask prompt for files/URLs you can edit the target string (e.g. file to its directory) before Allow; not for terminal commands.
- Status: GA
- Evidence: https://antigravity.google/docs/permissions

### Hook-driven permission overrides
- What: `PreToolUse` hooks can return `allow`, `auto_approve` (only when the default would prompt, honouring explicit user rules), `deny`, `ask`, `force_ask`, `deny_unless_prior_grant`, plus `overwrite` of args and `permissionOverrides`.
- Status: GA
- Evidence: https://antigravity.google/docs/hooks
- Notes: (verified in review; `allow` defers to the next hook or default policy)

### Browser JavaScript and URL policy
- See area 16.

### Workspace trust
- What: CLI `trustedWorkspaces` setting lists folders you chose to trust when the CLI asked; the CLI adds entries automatically; default empty array.
- Status: GA
- Evidence: https://antigravity.google/docs/cli/reference ; https://antigravity.google/docs/settings
- Notes: what trust unlocks is still not stated. (corrected in review: semantics partly documented on /settings)

### SDK policies
- What: Declarative `allow(tool)`, `deny(tool)`, `ask_user(tool, handler)`, `safe_defaults(handler)` evaluated as a list in order (example places `deny("*")` first); custom functions and read-only tools allowed by default; `run_command` needs explicit permission; `Agent` is read-only unless `CapabilitiesConfig()` is passed.
- Status: GA (docs); SDK maturity unclear
- Evidence: https://antigravity.google/docs/sdk/policies ; https://github.com/google-antigravity/antigravity-sdk-python
- Notes: Ordering semantics when several rules match are explicitly not documented.

### Area 6: Context management and compaction

### Automatic compaction and /compact
- What: Older turns are automatically summarised into checkpoint summaries; `/context` shows a grid of the window by category (user, agent, tool calls, system prompt, customizations, MCP tools, free space), tracked knowledge/artifact/skill files and a count of compactions. `/compact` exists (App v2.24.0 changelog; CLI v1.4.4-1.4.6). `/rewind` rolls back to an earlier checkpoint.
- Status: GA
- Evidence: https://antigravity.google/docs/cli/commands/context ; https://antigravity.google/docs/changelog
- Notes: the /context page says it does not cover `/compact` while the changelog does. Compaction algorithm and thresholds not documented (SDK local models config mentions a `compaction_config` and a default threshold sized for 64k KV cache).

### Rule and MCP token budgets
- What: Rules: 24,000 bytes per file, 20,000-token aggregate for global/always_on rules; larger files reduced to pointers read on demand. MCP tool schemas lazy-loaded by default.
- Status: GA
- Evidence: https://antigravity.google/docs/rules ; https://antigravity.google/docs/mcp

### Side questions
- What: `/btw` asks a side question kept out of main history.
- Status: GA
- Evidence: https://antigravity.google/docs/cli/commands/context

### Area 7: Memory and persistent instructions

### Memory feature: settled, none documented
- What: I checked the docs index, rules, features, settings, FAQ, /learn, context page and the whole changelog text I could page through (about 480k characters, read in five chunks). No "memory" or "knowledge base" feature is documented. The only hits for "memory" are RAM leak fixes. "Knowledge" appears only as "Knowledge Work" layout (v2.30.0) and model-launch copy. The /context page mentions "knowledge files" as a tracked file category, so some internal knowledge-file concept may exist, but it is not user-documented. The /learn page says directly that agents do not remember conversations and persistence is via rule/skill files.
- Status: not documented
- Evidence: https://antigravity.google/docs/commands/learn ; https://antigravity.google/docs/rules ; https://antigravity.google/docs/changelog ; https://antigravity.google/llms.txt ; https://antigravity.google/docs/cli/commands/context
- Notes: Whether an earlier (2025) "Knowledge" feature existed and was removed cannot be settled from these sources (changelog starts at v1.0.0 2026-01-01 and I saw no removal note). Unverified. (verified in review: the /learn page says agents do not remember conversations; the docs index shows no page titled or described as memory or knowledge)

### /learn (session-to-rules/skills with approval)
- What: Reflects on the session, classifies lessons as rule (constraints) or skill (procedures), prefers editing existing files, drafts a `learning_proposal.md` artifact, and writes only after explicit approval. Workspace or global scope chosen by the agent.
- Status: GA (App, CLI, IDE)
- Evidence: https://antigravity.google/docs/commands/learn
- Notes: An approval-gated memory-writing design, not auto-memory. (verified in review: page states the agent never edits configuration files immediately and writes only after explicit approval)

### Rules and instruction files
- What: `AGENTS.md` or `GEMINI.md` (plain Markdown, always on) in workspace folders and each parent up to the repo root (not subfolders), also `<dir>/.agents/AGENTS.md`; modular rules `.agents/rules/*.md` need YAML frontmatter with `trigger` (`always_on`, `model_decision`, `glob`, `manual`), `description`, `globs`; missing/invalid frontmatter silently discards the file. Global: `~/.gemini/AGENTS.md`, `~/.gemini/GEMINI.md`, `~/.gemini/config/rules/*.md`. Rules are cumulative (no override order). `@[label](path)` inlines a file; `rules.json` manifest for nested/external rules. Re-read before each turn.
- Status: GA
- Evidence: https://antigravity.google/docs/rules ; https://antigravity.google/docs/customization-discovery
- Notes: `manual` rules cannot be mentioned in the CLI. Subfolder AGENTS.md not loaded (unlike some other harnesses).

### Discovery manifests and personal folder in shared repo
- What: `skills.json`, `rules.json`, `agents.json`, `plugins.json` add or exclude items from outside standard folders; `inherits` other manifests. A repo-level `.gemini/config.json` can set `personal_customization_dir` (supports `{user}`), whose contents take priority over `.agents/`. Name-clash priority: personal-in-repo, workspace, personal global, built-in (CLI `~/.gemini/antigravity-cli/skills/` and `~/.gemini/skills/` override all).
- Status: GA
- Evidence: https://antigravity.google/docs/customization-discovery
- Notes: changelog v2.17.0: `.agents/settings.json` no longer read; moved to `.gemini/config.json`.

### Workflows (deprecated)
- What: Markdown files with title, description and steps, run as `/workflow-name`, 12,000 characters max; the page says they are deprecated in favour of skills, with `/migrate-workflows` (App 2.12.0+).
- Status: deprecated
- Evidence: https://antigravity.google/docs/ide/workflows ; https://antigravity.google/docs/rules
- Notes: the rules page says use a skill for multi-step workflows, consistent. Workflow storage path not documented.

### Skills
- What: Folder with required `SKILL.md` (YAML `description` required, `name` optional) plus optional `scripts/`, `examples/`, `resources/`. Progressive disclosure: names+descriptions every turn, full file on activation. App: auto or `/<skill-name>`; CLI: every skill is a slash command, `/skills reload`. Locations `.agents/skills/`, parents to repo root, `~/.gemini/config/skills/`, plugin skills (named `<plugin>:<skill>`).
- Status: GA
- Evidence: https://antigravity.google/docs/skills ; https://antigravity.google/docs/customization-discovery
- Notes: Skills page does not state precedence; discovery page does (above).

### Area 8: Planning, tasks and modes

### /plan and Implementation Plan artifact
- What: `/plan` explores read-only, asks structured questions, and drafts an Implementation Plan with task breakdown and verification checkpoints; the agent always stops for approval after `/plan` regardless of review policy. Comments attach to the next message as a chip ("2 Comments"); Proceed sends them. The agent either revises and re-requests review, or implements. CLI `--mode=plan` restricts to read-only search/view tools until approval.
- Status: GA
- Evidence: https://antigravity.google/docs/plan ; https://antigravity.google/docs/implementation-plan ; https://antigravity.google/docs/cli/modes
- Notes: No separate "task list" artifact is documented. Plan storage location not documented.

### Plan Review Policy (artifactReviewPolicy)
- What: App Settings > General: Request review ("Always Ask", recommended) or Always Proceed, applies in Planning mode only (not Fast mode). CLI key `artifactReviewPolicy`: `asks-for-review`, `agent-decides`, `always-proceed`. Even "Always Proceed" does not skip `/plan` approval.
- Status: GA
- Evidence: https://antigravity.google/docs/artifact-review ; https://antigravity.google/docs/settings
- Notes: CLI removed `/planning` and `/fast` in v1.1.0; the App page still mentions "Fast mode" (possible docs lag).

### /grill-me
- What: Socratic interview, one question at a time with a recommended answer, explores the codebase instead of asking answerable questions; combinable as `/plan /grill-me <task>`.
- Status: GA (App, CLI)
- Evidence: https://antigravity.google/docs/commands/grill-me

### Area 9: Sub-agents and multi-agent orchestration

### Custom subagents
- What: Markdown files with YAML frontmatter (`name`, `description`, `tools` allowlist, `mainAgent`, `subagent`, `model` `inherit|flash|pro`, `commandExecutionPolicy` `off|auto|eager|sandbox` default `sandbox`, `mcpServers`, `skills`, `plugins`) in `.agents/agents/`, `~/.gemini/config/agents/`, plugins; or `define_subagent` at runtime. Each subagent starts with a clean context in its own conversation, runs async in parallel; nesting depth max 10; no documented cap on concurrency/timeout/budget. States: Running, Idle (auto-wakes), Killed. Workspace modes `inherit`, `branch` (isolated git worktree, removed on kill), `share`. Inherit parent's allowed command prefixes, file scopes and sandbox; approvals bubble up. Transcripts are JSONL logs.
- Status: GA
- Evidence: https://antigravity.google/docs/subagents
- Notes: Known issue: a misspelled tool name may hang the subagent; validation "planned". (verified in review: depth 10 "strictly enforced", policy values and default `sandbox`, workspace modes, approvals bubble up, states)

### /boost
- What: Three-phase pipeline (orchestrator formulates subtasks, subagents implement/investigate/verify in ephemeral isolated worktrees, synthesis with iteration on failures). Paid plans.
- Status: GA (paid)
- Evidence: https://antigravity.google/docs/boost

### /teamwork
- What: Scoping interview produces a reviewable prompt artifact; then a Project Orchestrator runs roles (Sentinel, Orchestrator, read-only Explorers, Workers with exclusive file ownership, adversarial Critic, Challenger, Auditor, Success Auditor) via three artifacts (Request, Project plan, Progress). Integrity modes `development`/`demo`/`benchmark`. Work in a dedicated dir (default `~/teamwork_projects/{PROJECT_NAME}`); orchestrator hands off to a fresh successor between milestones. Paid plans (App and CLI); the teamwork page describes work that "spans multiple days".
- Status: GA (paid)
- Evidence: https://antigravity.google/docs/teamwork ; https://antigravity.google/docs/boost
- Notes: "Persistent worktrees per milestone" is in the /boost comparison table; the teamwork page itself says dedicated project directories. (corrected in review: the horizon was written "hours to days" but the teamwork page says multiple days; roles, artifacts, integrity modes with development as default, default directory and successor hand-off verified)

### /swarm
- What: A coordinator creates named workers (plus `@verifier` roughly per two to four workers and an idle `@monitor`) that coordinate only through one shared message board with `@name/@group/@all` mentions; coordinator may give each worker its own working copy; verifier sends problems back to the author; quota exhaustion pauses a member without retry; killed workers cannot be revived.
- Status: preview (Google AI Pro/Ultra; App v2.31.0, 2026-11-09)
- Evidence: https://antigravity.google/docs/swarm ; https://antigravity.google/docs/changelog
- Notes: No merge process documented; swarms consume quota much faster. (verified in review: page says "in preview", Pro/Ultra, App and CLI, verifier per two to four workers, monitor, no private worker messaging, working copies deleted on worker removal, quota pause without auto-restart, killed workers not revivable)

### SDK subagents
- What: `enable_subagents=True` for dynamic self-cloning subagents inheriting permissions; or static `SubagentConfig` with own tools (tools must also be listed in the parent).
- Status: GA (docs)
- Evidence: https://antigravity.google/docs/sdk/subagents

### Agent Manager / parallel conversations
- What: The App is a "command centre" to launch and monitor multiple agents across projects; Overview panel lists changes, artifacts, subagents, links; window can be split to work two conversations side by side. Each project can use Local mode or New Worktree mode (one git worktree per agent session, non-git folders used directly).
- Status: GA
- Evidence: https://antigravity.google/docs/overview ; https://antigravity.google/docs/features ; https://antigravity.google/docs/projects ; https://antigravity.google/product/antigravity-2
- Notes: The word "Agent Manager" appears on the IDE product page ("new agent-first Agent Manager experience") and overview lineage; the changelog (v1.0.0 onward) never mentions it, so the exact relation of App to Manager is by statement only. Worktree cleanup not documented. Knowledge Work layout hides the worktree selector.

### Area 10: Background, long-running and scheduled work

### /schedule (conversation-scoped timers and cron)
- What: The agent uses a `schedule` tool for one-shot timers (`DurationSeconds`, optional cancel condition) or five-field cron (`MaxIterations`, `IsDaemon`). Firing delivers a high-priority notification to the same conversation. Non-daemon schedules die with the owning task.
- Status: GA (App, CLI, IDE)
- Evidence: https://antigravity.google/docs/commands/schedule
- Notes: Where timers run and whether they survive closing the app is not stated.

### Automations
- What: App: scheduled conversations on a time trigger (fresh conversation each run), model tiers Flash Lite / Flash (default) / Pro, start offset up to 10% of interval (cap 15 min), Run Now, "Create with Prompt". Renamed from Scheduled Tasks in v2.21.1.
- Status: GA
- Evidence: https://antigravity.google/docs/features ; https://antigravity.google/docs/changelog

### Sidecars
- What: Background processes launched and restarted (`always|on-failure|never`) by the App; each directory under `~/.gemini/config/sidecars/` or plugin `sidecars/` has `sidecar.json` (`command` or builtin `schedule` with a cron first arg, `args`, `env`). Disabled by default; enabled in `~/.gemini/config/config.json`. Provide an `agentapi` CLI (`new-conversation`, `send-message`) to create or message conversations. Runtime data under `~/.gemini/antigravity/sidecar_data/<id>/` (data, logs, events).
- Status: GA (App)
- Evidence: https://antigravity.google/docs/sidecars
- Notes: Interesting as a local, user-defined event/cron source driving the agent. (verified in review: restart_policy defaults to `always`; plugin sidecars live under `~/.gemini/config/plugins/<name>/sidecars/`; `new-conversation` needs a `projectId`; the page carries no GA label, so the GA status is inferred)

### Remote Agents
- See area 17.

### Area 11: Sessions

### Conversations, resume, fork, rewind
- What: CLI histories are scoped to the working directory; `/resume` picker, `-c`, `--conversation <id>`; `/fork` (alias `/branch`) copies conversation up to current turn (not the git checkout) and `/fork <project_id>` copies into another project; `/rewind` (`/undo`) rolls back to a checkpoint; `/rename`, `/clear`. Storage path and retention not documented on the conversations page. SDK: `save_dir`, `conversation_id` (min 32 chars) restore a session.
- Status: GA
- Evidence: https://antigravity.google/docs/cli/conversations ; https://antigravity.google/docs/features ; https://antigravity.google/docs/cli/reference ; https://antigravity.google/docs/sdk/lifecycle
- Notes: Whether /rewind restores files or just conversation is not stated on pages I opened (best-practices says "roll back to a stable checkpoint"). Unverified.

### App conversation search and organisation
- What: Cmd/Ctrl+K full-text search over prompts and responses, project files and commands; drag-and-drop sidebar; conversation navigator; archived filter.
- Status: GA
- Evidence: https://antigravity.google/docs/features ; https://antigravity.google/docs/changelog

### Area 12: Git and version control

### Git worktree isolation
- See areas 9 and projects entry; subagent `branch` workspace mode; App "New worktree" mode.
- Status: GA (App)
- Evidence: https://antigravity.google/docs/projects ; https://antigravity.google/docs/subagents ; https://antigravity.google/docs/faq
- Notes: FAQ says worktrees supported in the App.

### Commit, amend, push in the review pane
- What: Commit with auto-generated message when empty, amend, push to upstream or publish to origin; push never forces.
- Status: GA (App)
- Evidence: https://antigravity.google/docs/review-changes
- Notes: No documented PR creation (Remote Agents page does not cover PRs either).

### GitHub integration for remote agents
- What: OAuth to GitHub; repos cloned into the remote workspace; private repos in one project must share an owner.
- Status: GA (limited rollout)
- Evidence: https://antigravity.google/docs/remote-agents

### Area 13: Extensibility

### MCP
- What: Config `mcp_config.json` (global `~/.gemini/config/`, workspace `.agents/`, plugins). `mcpServers` map; stdio (`command`) or remote Streamable HTTP (`serverUrl`/`url`); legacy SSE and `httpUrl` not supported in the app/CLI (SDK supports SSE). Fields `args, env, cwd, headers, authProviderType, oauth, disabled, enabledTools, disabledTools, tools, timeoutSeconds`. Auth: Google ADC (`google_credentials`) or OAuth 2.0 with PKCE (manual client, CIMD, or DCR; redirect `https://antigravity.google/oauth-callback`); tokens in `~/.gemini/antigravity/mcp_oauth_tokens.json`. Default timeouts 300s connect/list, 180s per call. Lazy tool loading by default (schemas cached on disk, called through `call_mcp_tool`), `eager: true` per tool to register schema upfront; per-tool `background` setting. Unconfigured MCP tools are Ask; policy `mcp(server/tool)`.
- Status: GA
- Evidence: https://antigravity.google/docs/mcp ; https://antigravity.google/docs/permissions
- Notes: Lazy-by-default tool schemas is notable for context cost.

### Plugins and Marketplace
- What: `plugin.json` (or `.antigravity-plugin/`, `.gemini-plugin/`) bundling skills, MCP servers (namespaced `<plugin>_<server>`), agents, rules, hooks (hooks "run automatically"; docs call them the main execution risk). Sources: official marketplace `antigravity-plugins-official`, GitHub URL, local dir, import from Gemini CLI extensions and from Claude Code (`~/.claude/plugins/` via `agy plugin import`). `agy plugin validate --strict`. Plugins needing config install disabled. Partner plugins (Figma, Notion, Stripe, Datadog, Jira etc.) added v2.28.2.
- Status: GA
- Evidence: https://antigravity.google/docs/plugins ; https://antigravity.google/docs/features ; https://antigravity.google/docs/changelog
- Notes: Marketplace vetting process and how permissions apply to plugin components are not documented. App v2.30.0 imports MCP/skills from other coding agents.

### Hooks
- What: `hooks.json` (`.agents/` or `~/.gemini/config/`, or plugin); events `SessionStart`, `PreToolUse`, `PostToolUse`, `PreInvocation`, `PostInvocation`, `Stop`; handler type `command` only, default 30s timeout; JSON on stdin and stdout; hooks can inject steps (system/user/ephemeral/error messages), overwrite tool args, force-continue the loop, or force-stop. Exit codes not documented; control is via stdout JSON.
- Status: GA
- Evidence: https://antigravity.google/docs/hooks
- Notes: Plugins page says hooks can also be "prompt evaluations"; hooks page says only `command`. Docs contradict.

### SDK
- What: Python `google.antigravity`: `Agent`/`LocalAgentConfig`, custom tools, MCP, hooks (`pre_turn`, `on_tool_error`), triggers (`triggers.every`), policies, subagents, structured output, personas, streaming of thoughts/tool calls, multimodal input, persistence, `usage_metadata`. Three layers: Agent, Conversation (+HookRunner, ToolRunner, TriggerRunner), Connection/LocalConnection (adapter, suggests a pluggable transport). Needs compiled binary from PyPI wheels.
- Status: unclear (public repo, 767 commits; no stability statement)
- Evidence: https://antigravity.google/docs/sdk/overview ; https://antigravity.google/docs/sdk/lifecycle ; https://github.com/google-antigravity/antigravity-sdk-python
- Notes: Apache-2.0 for the Python wrapper only.

### Agent Client Protocol (ACP)
- What: `agy acp` starts an ACP server (JSON-RPC over stdio) that editors (Zed, JetBrains, Android Studio, Xcode) launch as a subprocess; uses CLI auth, tools and settings.
- Status: GA
- Evidence: https://antigravity.google/docs/cli/acp ; https://antigravity.google/docs/ide/extensions
- Notes: Docs do not say whether native extensions use ACP too.

### Custom slash commands
- What: Not offered as a separate feature; skills become slash commands (CLI/App). Gemini CLI custom commands convert to skills on plugin import.
- Status: GA (via skills)
- Evidence: https://antigravity.google/docs/slash-commands ; https://antigravity.google/docs/cli/gcli-migration

### Area 14: Models, providers, inference configuration

### Model catalogue and plan gating
- What: Gemini 4 Carbon, Gemini 4 Argon, Gemini 3.8/3.7/3.6 Flash, Gemini 3.1 Pro; Claude Sonnet 5.5, Opus 5.5, Sonnet 4.6, Opus 4.6 (thinking); GPT-OSS 120B. Gating by Free / Plus / Pro / Ultra / Enterprise; Claude 4.6 and GPT-OSS marked for removal 2026-11-18. Third-party Claude requires admin opt-in on Enterprise. Gemini 4 Argon launched 2026-10-13 and Carbon 2026-11-10 (changelog). Switching mid-run applies from the next turn.
- Status: GA
- Evidence: https://antigravity.google/docs/models ; https://antigravity.google/docs/overview ; https://antigravity.google/docs/enterprise ; https://antigravity.google/docs/changelog
- Notes: Model names as documented; I cannot verify them externally.

### Effort
- What: Low/Medium/High per turn (CLI `--effort`, `/effort`); the CLI reference table says `low` through `max` (inconsistent with headless page listing low/medium/high). Some Flash models show "Fast".
- Status: GA
- Evidence: https://antigravity.google/docs/models ; https://antigravity.google/docs/cli/reference ; https://antigravity.google/docs/cli/headless
- Notes: contradiction noted.

### Bring your own key / providers
- What: Plans page says bring-your-own-key or endpoint for extra rate limits is not supported. But the CLI exposes `modelProvider` (with `GEMINI_API_KEY` when `"gemini"`), `customModelsConfig`, `gcp` settings, and the SDK supports Gemini API key, Vertex, local LiteRT (Gemma 4) and any OpenAI-compatible server (Ollama, LM Studio, vLLM). Third-party tools accessing consumer plans is a ToS violation per FAQ.
- Status: unclear for CLI; GA for SDK
- Evidence: https://antigravity.google/docs/plans ; https://antigravity.google/docs/cli/reference ; https://antigravity.google/docs/settings ; https://antigravity.google/docs/sdk/local-models ; https://antigravity.google/docs/faq
- Notes: `customModelsConfig` format not documented in pages I opened.

### Quotas and credits
- What: Weekly and five-hour limits, shown per model family (Gemini vs Claude/GPT) in the picker and `/usage`; AI Credits overage setting ("Never"/"Always"); CLI `/credits`, `useG1Credits`.
- Status: GA
- Evidence: https://antigravity.google/docs/plans ; https://antigravity.google/docs/models ; https://antigravity.google/docs/cli/commands/credits (listed in index)
- Notes: No prices stated.

### Area 15: UI surfaces

- What: Desktop App (macOS, Windows, Linux, "Googlebook"), TUI (alt-screen, themes, Vim mode, status line and title scripts fed live metadata, keybindings.json), IDE, extensions, voice (App Gemini Live voice mode, live dictation Ctrl+M; CLI `/voice live`, `/voice dictate`, `agy mic-serve` over SSH; dictation unavailable for business/enterprise), Knowledge Work vs Software Engineering layout (v2.30.0, personal accounts), side panel with artifacts/files/diffs/terminal/web pages as tabs, built-in browser (App), integrated terminal, PDF viewer, notebook viewer, mobile web via Remote Control.
- Status: GA (voice and Remote Agents "rolling out" v2.29.0 to 2.30.0)
- Evidence: https://antigravity.google/docs/features ; https://antigravity.google/docs/cli/using ; https://antigravity.google/docs/changelog ; https://antigravity.google/product/antigravity-2
- Notes: "Googlebook" is named on product pages without explanation.

### Area 16: Browser automation and computer use

### Browser subagent (/browser)
- What: The main agent delegates via `invoke_subagent` to a hidden built-in `browser` subagent that drives the user's own running Google Chrome (v144+) through the Chrome DevTools MCP server, which Antigravity starts with `npx`. User enables `chrome://inspect/#remote-debugging`; Chrome asks permission on connect. Capabilities: navigation and tabs, click/hover/drag/type/forms/upload/dialogs, accessibility snapshots, screenshots, console and network, JS eval, performance traces, Lighthouse, heap snapshots, viewport/device/network/CPU emulation. Own context window. Recording as `recording.webm` only when it interacts with a page (not guaranteed). No Chrome extension; no separate profile, so it has the user's existing cookies and sign-ins.
- Status: GA (App, CLI, IDE)
- Evidence: https://antigravity.google/docs/commands/browser ; https://antigravity.google/docs/ide/browser ; https://antigravity.google/docs/ide/separate-chrome-profile ; https://antigravity.google/docs/ide/browser-recordings
- Notes: (verified in review: Chrome 144+, remote-debugging toggle, DevTools MCP started automatically, same cookies and sign-ins; /slash-commands says "sandboxed Chrome browser subagent" while /commands/browser and /ide/separate-chrome-profile do not call it sandboxed) Slash-command catalog says "sandboxed Chrome" but the docs' own description is the user's live Chrome profile; this contradicts "sandboxed" wording (overview/subagents pages also say "sandboxed Chromium browser automation"). Treat "sandboxed" as vendor-claim. Page says admins can disable browser access.

### Browser actuation rules and JS policy
- What: Allow/Ask/Deny list of URLs ("Execute URLs"); on an unruled site a prompt offers allow this time, allow in this conversation, always allow, or no. Rules match domain and subdomains, not paths; deny beats allow; Google data sites (accounts, mail, docs, drive) need their own rules. Org admins can set allowed domains. `Browser Javascript Execution Policy`: Request Review (default), Always Proceed, Disabled. Separate `Network Access Rules` govern non-browser page reads (`read_url`). IDE strict mode forces allowlist/denylist.
- Status: GA
- Evidence: https://antigravity.google/docs/ide/allowlist-denylist ; https://antigravity.google/docs/commands/browser ; https://antigravity.google/docs/settings
- Notes: App v2.31.0 added browser download approvals.

### Built-in browser (App)
- What: Side-panel browser with separate cookies per Antigravity account, persistent; Comment and Quote send page context to the agent.
- Status: GA (App v2.28.0)
- Evidence: https://antigravity.google/docs/features ; https://antigravity.google/docs/ide/separate-chrome-profile ; https://antigravity.google/docs/changelog

### Area 17: Remote, cloud, headless, CI and distributed execution

### Headless mode (`agy -p`)
- What: One prompt, response to stdout, diagnostics to stderr. Flags `--output-format text|json|stream-json`, `--input-format text|stream-json` (NDJSON, one `{"event":"user",...}` per line, needs stream-json output), `--json-schema` structured output, `--model`, `--effort`, `--agent`, `-c`, `--conversation`, `--print-timeout`, `--sandbox`, `--dangerously-skip-permissions`. JSON envelope has conversation_id, status (`SUCCESS`, `ERROR`, `CANCELED`, ...), usage tokens, duration, num_turns. Stream events `init`, `step_update`, `result`. Uses cached credentials; unauthenticated CI exits with an error. Un-obtainable approvals are soft-denied (run continues, exit code 0, notice on stderr). `--mode=auto` falls back to default in headless.
- Status: GA
- Evidence: https://antigravity.google/docs/cli/headless ; https://antigravity.google/docs/cli/reference
- Notes: FAQ: wrapping `agy` in third-party agents or switching accounts is unsupported under consumer plans. No documented API-key auth for CI except for the `gemini` provider; no GitHub Action documented (I did not find one).

### Remote Control
- What: Web dashboard (antigravity.google.com) controls a session on your machine through a reverse tunnel opened by the host; tool approvals can be answered remotely; commands run on the host. CLI: `/remote-control`, `--remote-control`, or `agy remote-control start` which registers an OS service (systemd user service, LaunchAgent, Scheduled Task). Same Google account required. Settings cannot be changed from the web.
- Status: GA (disable-able by policy)
- Evidence: https://antigravity.google/docs/remote-control ; https://antigravity.google/docs/features
- Notes: Relay host and encryption are not described (docs only say "securely"): vendor-claim. (verified in review: interactive mode is a per-session reverse tunnel, headless daemon registers as an OS service, approvals answerable from either end; the page has no formal status label)

### Remote Agents
- What: Sessions run in a Google-managed isolated environment, persist when your machine is offline; GitHub repos cloned in, Drive files read on demand, uploads (8 MiB per file, 50 files/100 MiB per project, 2 GiB per account) staged at `/home/antigravity/upload`. Default preset there is Turbo (always proceed); always default execution mode (no plan/auto modes). OAuth creds encrypted at rest, tokens injected on call. Personal AI Pro/Ultra accounts only; CLI 1.4.12+; no print mode, `/diff`, `/fork`, `/rewind`, local MCP/plugins/skills.
- Status: GA rolling out (v2.29.0 start, v2.30.0 to all eligible)
- Evidence: https://antigravity.google/docs/remote-agents ; https://antigravity.google/docs/changelog
- Notes: Retention and training policy not described on the page.

### SSH
- What: CLI "runs in local shells, remote cloud environments and SSH sessions"; sign-in prints a URL over SSH; `agy mic-serve` forwards microphone over SSH. 00-targets.md notes no dedicated SSH-node feature found.
- Status: unclear
- Evidence: https://antigravity.google/docs/overview ; https://github.com/google-antigravity/antigravity-cli ; https://antigravity.google/docs/features

### Area 18: Collaboration, teams and enterprise

- What: Enterprise via Google Cloud project billing or Gemini Enterprise Standard/Plus seats (IAM roles `agentspaceAdmin`/`agentspaceUser`); SSO/BYOID via Workforce Identity Federation; regional enablement (global/us/eu); VPC Service Controls, PSC, mTLS; admin opt-in for Claude; org allowed browser domains, browser disable; request/response logging into the customer's Cloud project; Auto and some presets not available to business accounts; telemetry always off for business accounts in CLI. Standalone IDE unsupported for enterprise. No shared-session or multi-user collaboration feature documented; `.agents/` in git is the sharing mechanism. Docs recommend Default/Request Review, `--sandbox`, plan mode for enterprise.
- Status: GA (JetBrains/Zed/Xcode/Eclipse enterprise sign-in: preview)
- Evidence: https://antigravity.google/docs/enterprise ; https://antigravity.google/docs/overview ; https://antigravity.google/docs/ide/extensions
- Notes: Enterprise page omits retention periods and training policy itself (defers to Cloud docs); overview and extensions pages assert no training on prompts/code/transcripts: vendor-claim.

### Area 19: Observability

- What: `/usage` (quotas), `/credits`, `/context`, token usage in headless JSON and SDK `usage_metadata`, local logs (`~/.gemini/antigravity-cli/cli.log`, `~/Library/Logs/Antigravity/language_server.log`, `--log-file`), subagent JSONL transcripts, hook stdin includes `transcriptPath` and `artifactDirectoryPath`, enterprise request/response logging to the customer's Cloud project. No tracing/OpenTelemetry documented.
- Status: GA
- Evidence: https://antigravity.google/docs/enterprise ; https://antigravity.google/docs/cli/reference ; https://antigravity.google/docs/subagents ; https://antigravity.google/docs/hooks ; https://antigravity.google/docs/cli/headless
- Notes: "nothing documented" for OTel/tracing.

### Area 20: Configuration and customisation

- What: CLI `~/.gemini/antigravity-cli/settings.json` (only non-default values written, 0600, unknown keys preserved, `$schema` for autocomplete), `keybindings.json`; shared `.agents/` and `~/.gemini/config/` trees (rules, skills, agents, plugins, `hooks.json`, `mcp_config.json`, manifests); `~/.gemini/config/config.json` (sidecars, remote control hostnames); repo `.gemini/config.json` (`personal_customization_dir`). Scan is one level deep, up to repo root; `.agent/`, `_agents/` legacy names. Permission scopes combine Global + Project + Shared. App settings are UI based. Precedence tables: see areas 5 and 7.
- Status: GA
- Evidence: https://antigravity.google/docs/settings ; https://antigravity.google/docs/customization-discovery ; https://antigravity.google/docs/cli/using
- Notes: Settings link `/docs/cli/settings` in the migration page points to a page absent from nav (docs lag).

### Area 21: Security and privacy

### Data collection and telemetry
- What: README and docs state interactions are collected to improve Antigravity and models, with an opt-out (App Settings > Account > Enable Telemetry; CLI toggle saved with the account, not settings.json; always off for business accounts). IDE legacy setting `antigravity.enableTelemetry` deprecated (IDE extension changelog 1.9.0). Enterprise: not used to train (vendor-claim).
- Status: GA
- Evidence: https://antigravity.google/docs/settings ; https://github.com/google-antigravity/antigravity-cli ; https://antigravity.google/docs/faq ; https://antigravity.google/docs/enterprise
- Notes: For consumer accounts the data retention, review and training terms are in Terms/Privacy pages I did not open. Default on/off state of telemetry not stated in the pages opened.

### Safety warnings and credential handling
- What: CLI README warns of autonomous execution, exfiltration and prompt injection. CLI credentials in OS keyring (fallback Google Sign-In). Sandbox blocks `~/.ssh`, `.env`. Generative UI widgets run in an isolated frame, no file access, no external scripts except Tailwind. Anti-prompt-injection design is not documented.
- Status: GA
- Evidence: https://github.com/google-antigravity/antigravity-cli ; https://antigravity.google/docs/sandbox ; https://antigravity.google/docs/commands/generative-ui
- Notes: nothing documented on prompt-injection defences beyond the warning.

### Third-party access rule
- What: Using third-party tools to access Antigravity consumer service violates ToS and can lead to suspension.
- Status: GA (policy)
- Evidence: https://antigravity.google/docs/faq
- Notes: strongly limits anyone wanting to drive `agy` from their own harness.

### Area 22: Licence, pricing, distribution

- What: Plans: Free (weekly quota), Google AI Plus/Pro (five-hour quota with weekly limit) and Ultra, AI credits for overage, Enterprise via Google Cloud; no prices on the plans page. Under 18 not allowed; country list; work/school Google accounts cannot use personal sign-in. Installers via `curl ... | bash` from antigravity.google/cli/install.sh; app auto-updates (switchable). Closed binary.
- Status: GA
- Evidence: https://antigravity.google/docs/plans ; https://antigravity.google/docs/faq ; https://github.com/google-antigravity/antigravity-cli ; https://antigravity.google/releases
- Notes: pricing page itself (https://antigravity.google/pricing) not opened.

### Area 23: Anything else or unusual

### Artifacts (plans, walkthroughs, screenshots, recordings, widgets)
- What: "Artifacts" are agent deliverables reviewed asynchronously instead of watching tool calls: implementation plans, diffs, diagrams, images, browser recordings, interactive HTML widgets (App only). Types documented: Implementation Plan, Walkthrough (post-task summary with changes and validation results, App/IDE), screenshots (saved to artifacts on request; drag-select region plus comment, held as pending and sent with next message or via Review button), `recording.webm`, `learning_proposal.md`, teamwork Request/Plan/Progress artifacts. No task-list artifact is documented (I looked). Storage location and retention not documented.
- Status: GA
- Evidence: https://antigravity.google/docs/artifacts ; https://antigravity.google/docs/implementation-plan ; https://antigravity.google/docs/walkthrough ; https://antigravity.google/docs/screenshots ; https://antigravity.google/docs/ide/browser-recordings ; https://antigravity.google/docs/commands/generative-ui
- Notes: Comment flow in the App is documented for plans and screenshots; the artifact-review page itself explains nothing about how comments are interpreted.

### CLI artifact picker
- What: `ctrl+r` opens a full-screen picker; `y`/`n` approve/reject per file, Shift+A / Shift+R all; `p` preview; `c` adds a line comment, `d` deletes; media in a drawer opened in the OS viewer; Mermaid rendered as Kitty image, ASCII or raw. On Esc approvals and rejections are sent back to the agent thread. How line comments reach the agent is not explained.
- Status: GA
- Evidence: https://antigravity.google/docs/cli/artifacts
- Notes: Approval is per file, not per hunk.

### /diff comment loop
- What: CLI `/diff` views All Changes, Per Turn, Commit Tree; `C` adds a line comment; on exit unsent comments are sent as the next prompt (Y) or discarded (N).
- Status: GA
- Evidence: https://antigravity.google/docs/review-changes ; https://antigravity.google/docs/features

### Generative UI
- See area 23 artifacts; `/generative_ui` builds single-file HTML widgets in isolated frames, inline up to 500 px.
- Status: GA (App only)
- Evidence: https://antigravity.google/docs/commands/generative-ui

### Machine kept awake
- What: Antigravity prevents the computer from sleeping while an agent runs.
- Status: GA
- Evidence: https://antigravity.google/docs/faq

## 3. Distinctive or unusual

- Command-permission parser that disables prefix matching on shell constructs that can hide execution (`$(...)`, `git -c`, `tar --to-command`): the allow-list is robust against obvious bypasses by design (area 4).
- Deny > Ask > Allow with allow rules feeding the sandbox mounts and network allowlist: one policy language drives both prompts and OS isolation (areas 4 and 5).
- Out-of-sandbox command evaluation ("Auto") instead of a binary approve/deny.
- `/learn` writes memory as reviewable rule/skill files behind an approval artifact rather than hidden memory (area 7).
- Lazy MCP tool schemas by default with per-tool eager override (area 13).
- Hooks that can inject steps, rewrite arguments and force the loop to continue; `/goal` is implemented on a stop hook (areas 13, 1).
- Subagent frontmatter with per-agent `commandExecutionPolicy` and `model: inherit|flash|pro`; depth limit 10.
- `/teamwork` roles with adversarial verification and integrity modes (`benchmark` forbids mock passes).
- Message-board swarm where workers cannot message each other privately.
- Sidecars plus an `agentapi` CLI so local scripts can create or message agent conversations (area 10).
- Browser agent drives the user's real Chrome over DevTools MCP rather than a managed profile or extension.
- Staged pending comments on artifacts sent with the next message.

## 4. Architecture and UX notes (public statements only)

- One harness shared by App, CLI, IDE extensions and SDK; the CLI is Go. SDK launches a compiled binary through a Connection abstraction (LocalConnection). Whether App and extensions talk to a local server is not documented.
- Clients can attach to a remote host: Remote Control uses a host-initiated reverse tunnel to a Google-hosted web dashboard; the daemon is an OS service with its own CLI credentials; execution stays on the host.
- ACP server mode (`agy acp`) for editors.
- Customization is plain files in `.agents/` and `~/.gemini/config/`, reread every turn.
- Hooks, sidecars and plugins are local executables run with user privileges (plugin hooks run in the background when enabled).
- Enterprise logs to the customer's Cloud project.

## 5. Relevance to a self-hosted master/node harness

- Permission language `action(target)` with Deny > Ask > Allow, applied before presets (area 5); a node-side `node.toml` could use the same layering, with the node as the authority.
- Allow rules populating sandbox read/write/network lists (area 4): one source of truth for approval and isolation; sandbox default-off in the CLI is a cautionary example.
- Command-prefix matching with an exact-match fallback for hiding constructs (area 4): worth copying exactly as a requirement for any allow-list.
- Remote Control reverse tunnel and OS-service daemon (area 17): the node-initiated outbound connection model matches a master/node link; approvals answerable from either end.
- Artifact review with pending comments sent with the next message, plus per-file approve/reject (areas 8 and 23): a UI pattern a web dashboard can reproduce.
- Stop-hook driven `/goal` with evidence-based completion (area 1).
- Lazy MCP schema loading and `/context` category breakdown (areas 6 and 13) help the light-harness invariant.
- Subagent inheriting parent permissions and surfacing approvals in the main UI (area 9).
- Headless NDJSON stream with `init`/`step_update`/`result` plus soft-deny of approvals (area 17): a concrete protocol shape to compare to.
- Third-party-access ToS restriction (area 21) shows the gap a self-hosted harness can fill; Remote Agents and Remote Control both depend on Google's cloud (areas 17).

## 6. Overhyped, stale or contradictory

- "Sandboxed" browser: the docs say the subagent uses the user's real Chrome profile with live sign-ins; "sandboxed Chromium/Chrome" appears in overview and slash-command pages.
- Hooks: plugins page says shell commands or prompt evaluations; hooks page documents only `command`.
- Effort: `max` in the CLI reference vs low/medium/high elsewhere.
- Plans page says no BYOK/custom endpoints, yet CLI settings list `modelProvider`/`customModelsConfig`/`GEMINI_API_KEY` and SDK supports Gemini keys, Vertex and local servers.
- Slash command catalog calls itself complete but omits many commands. `/context` page does not mention `/compact` (changelog does). Artifact review page mentions "Fast mode" though `/fast` was removed in CLI v1.1.0.
- Overview says four surfaces; llms.txt lists five. Migration page links to a missing settings page.
- Overview and product pages state "co-trained", "highly optimized", "securely": vendor-claim.
- Auto mode was "Preview" in v2.29.0 and earlier; sandbox still "Preview" for business accounts.
- Docs claim enterprise data is never used for training: vendor-claim; the enterprise page itself says retention/training detail lives in linked Cloud docs.

## 7. Unverified

- Whether an earlier Knowledge/memory feature existed and was removed (changelog pre-2026 absent). Settle with Wayback copies of earlier docs or launch blog posts.
- What `knowledge files` in `/context` are.
- Whether `/rewind` restores file state or only conversation state.
- Where artifacts, conversations and checkpoints are stored on disk; retention.
- Whether the Gemini CLI repository will be archived; the repo is still public and Apache-2.0 as of this survey.
- CLI release `1.5.5` on 2026-11-10 (not seen; changelog showed 1.5.3 on 2026-11-07). GitHub releases page not opened.
- Whether Jules is merged into Antigravity (not searched here beyond the targets file).
- Whether the App runs the loop in-process or via a local language server.
- Mechanism of Auto-mode command evaluator (separate model call?).
- `customModelsConfig` format; `trustedWorkspaces` semantics; `sandboxProxy` details.
- IDE origin (VS Code fork or not): product page silent; the docs mention VS Code extension settings in the IDE changelog but I cannot infer lineage.
- Antigravity CLI GitHub repository contents beyond README (not allowed: source).
- Terms/Privacy pages (consumer data retention and training) not opened: https://antigravity.google/terms.
- `docs/cli/credits`, `docs/marketplace`, `docs/build-plugins`, `docs/side-panel`, `docs/at-mentions`, `docs/voice-mode`, `docs/jupyter-notebooks`, `docs/ide/tab`, `docs/sdk/{personas,tools,mcp,structured-output}`, `docs/firebase-studio-migration`, `docs/build-with-google`, `docs/install`, `docs/getting-started`: indexed but not opened by me.

## 8. Sources (all opened with WebFetch)

- https://antigravity.google/llms.txt : docs index.
- https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/ : Gemini CLI transition (2026-05-19).
- https://github.com/google-gemini/gemini-cli : README notices, licence.
- https://github.com/google-antigravity/antigravity-cli : README, install, licence absence.
- https://github.com/google-antigravity/antigravity-sdk-python : SDK README, Apache-2.0, binary.
- https://antigravity.google/docs/overview : surfaces, harness, tiers.
- https://antigravity.google/docs/features : features by surface.
- https://antigravity.google/docs/permissions : rules, presets, modes.
- https://antigravity.google/docs/sandbox : sandbox mechanism.
- https://antigravity.google/docs/artifacts ; /artifact-review ; /implementation-plan ; /walkthrough ; /screenshots ; /cli/artifacts ; /plan ; /review-changes : artifacts and review.
- https://antigravity.google/docs/ide/browser ; /ide/browser-recordings ; /ide/allowlist-denylist ; /ide/separate-chrome-profile ; /commands/browser : browser agent.
- https://antigravity.google/docs/commands/learn ; /rules ; /ide/workflows ; /customization-discovery ; /skills : memory and instructions.
- https://antigravity.google/docs/faq ; /settings ; /agent-settings ; /plans ; /models ; /enterprise : data handling, settings, models, plans.
- https://antigravity.google/docs/mcp ; /plugins ; /hooks : extensibility.
- https://antigravity.google/docs/subagents ; /teamwork ; /swarm ; /boost ; /commands/goal ; /commands/grill-me ; /commands/generative-ui ; /commands/schedule ; /sidecars ; /projects : agents, scheduling.
- https://antigravity.google/docs/cli/headless ; /cli/acp ; /cli/modes ; /cli/reference ; /cli/using ; /cli/conversations ; /cli/commands/context ; /cli/best-practices ; /cli/gcli-migration : CLI.
- https://antigravity.google/docs/remote-agents ; /remote-control : remote.
- https://antigravity.google/docs/sdk/overview ; /sdk/policies ; /sdk/local-models ; /sdk/subagents ; /sdk/lifecycle : SDK.
- https://antigravity.google/docs/ide/extensions : extensions.
- https://antigravity.google/docs/changelog : release notes, read in five chunks of 100k characters (v1.0.0 2026-01-01 to v2.31.0 2026-11-09).
- https://antigravity.google/docs/slash-commands ; /product/antigravity-2 ; /product/antigravity-ide ; /releases : catalogue and product claims.

## 9. Review

Date: 2026-11-10. Entries checked: 27 of about 85 (counting the Gemini CLI transition, identity and surfaces claims as entries). Reviewer opened each cited page and asked for the specific fact.

| Entry | Verdict | Reason |
|---|---|---|
| Command permission rules (grammar, precedence, shell constructs, wrappers) | confirmed | /permissions states `action(target)`, Deny > Ask > Allow, presets beneath rules, whole-line match for `$(...)`, backticks, `git -c`, `tar --to-command`; pipelines and wrappers prefix-match |
| Allow rules feed sandbox mounts and network allow-list | confirmed-with-correction | True in App docs (/permissions, /sandbox); the CLI docs do not spell it out |
| Terminal sandbox | confirmed-with-correction | "Preview" label for business not found; bubblewrap only on /enterprise; defaults App on, CLI off |
| Out-of-sandbox Auto | not-supported (detail) | Preview label and business unavailability not found; core behaviour not re-checked |
| CLI modes and toolPermission | confirmed-with-correction | /cli/modes never mentions `toolPermission`; value list and default unconfirmed there |
| Workspace trust | confirmed-with-correction | `trustedWorkspaces` semantics are on /settings |
| Hook-driven permission overrides / Hooks | confirmed | Events, `command` only, 30 s timeout, decisions listed |
| /goal | confirmed | Stop hook, no cap, one goal at a time; changelog v1.0.14 quote |
| /learn | confirmed | Proposal artifact, approval before write |
| Memory feature: none documented | confirmed | /learn says agents do not remember; no memory or knowledge page in index |
| /teamwork | confirmed-with-correction | Horizon is "multiple days", not "hours to days" |
| /swarm | confirmed | Preview, Pro/Ultra, board, verifier ratio, monitor, quota pause |
| Sidecars | confirmed | Paths, fields, default `always`, disabled by default, agentapi |
| Custom subagents | confirmed | Depth 10, policy default `sandbox`, modes, bubbling approvals |
| Browser subagent | confirmed | Chrome 144+, own live profile, "sandboxed" only in slash-command catalog |
| Browser "sandboxed" contradiction | confirmed | Catalog says sandboxed; other pages do not |
| Remote Control | confirmed | Reverse tunnel, OS service, remote approvals |
| Gemini CLI transition dates | confirmed-with-correction | 2026-05-19 and 2026-06-18 correct; post says "available to everyone", not GA |
| Gemini CLI repo status | confirmed | Public, Apache-2.0, not archived, v0.67.0 on 04 Nov, enterprise-only banner |
| Surfaces list (App v2.5+, CLI v1.2+, IDE extensions 6, SDK) | confirmed | Overview |
| IDE extension statuses | confirmed | Eclipse Public Preview; enterprise preview for JetBrains, Zed, Xcode, Eclipse |
| Changelog latest (App v2.31.0, 2026-11-09) | confirmed | CLI 1.5.3/1.5.5 not seen in the part read |
| Rule/skill/discovery, MCP, headless, models, quotas, enterprise details | not checked | Left as written |

Borrowed features: none found. Wording on /goal, /grill-me and skills echoes other harnesses but the mechanisms cited match this vendor's pages.

Notable omissions (from https://antigravity.google/llms.txt): `/agents` command page; `/codesearch`; Mentions and quoting (at-mentions); Search and command palette; Side panel and Overview panel; Marketplace and Build plugins pages (opened by nobody; listed as unopened in Section 7); Firebase Studio migration; CLI troubleshooting; Vim mode and status line customization pages; `/changelog` command.

Trust judgement: the permission, sandbox, subagent, hook, swarm and sidecar entries that were checked are accurate in detail; errors were small (labels, one duration, one setting name). The unchecked remainder is probably reliable at about the same rate, but status labels ("GA") are the survey's inference where pages carry none, and the Auto/business "Preview" detail is unverified. The file is usable.
