# OpenAI Codex: round 1 public-surface survey

Survey date: 2026-11-10. Sources: OpenAI's docs at `learn.chatgpt.com/docs` (read through their Markdown twins, `<page>.md`), the docs changelog, the `openai/codex` README and GitHub release notes. No source code was read. Abbreviation used below: **D** = `https://learn.chatgpt.com/docs/`. Every "Evidence" URL is written out in full.

---

## 1. Identity

- **Name / vendor:** Codex, by OpenAI. It is a family of products: an open-source terminal agent plus closed IDE, desktop, cloud and mobile surfaces (https://learn.chatgpt.com/docs/open-source.md).
- **Surfaces, and the label each entry below uses:**
  - **CLI**: the open-source Rust TUI `codex`, plus `codex exec` for headless runs.
  - **App-server / SDK**: the JSON-RPC `codex app-server`, the TypeScript SDK `@openai/codex-sdk` and the Python SDK `openai-codex`.
  - **IDE**: the extension for VS Code, Cursor and Windsurf. Xcode and JetBrains ship their own integrations.
  - **Desktop**: the ChatGPT desktop app for macOS, Windows and Linux (Linux is a preview), with Codex chosen inside it. The changelog still versions it as "Codex app 26.xxxx".
  - **Cloud**: Codex Cloud with reusable cloud environments. "Codex Cloud (Legacy)" still runs Code Review and the Linear and GitHub integrations.
  - **Mobile**: the ChatGPT iOS and Android apps, driving Cloud or a connected computer.
  - **Integrations**: the GitHub Action, GitHub and GitLab review, Slack, Linear.
- **Open vs closed:**
  - Open: the CLI, the SDK and the App Server (all in `openai/codex`), the Codex Security CLI and SDK, the skills and plugins repositories, and the universal cloud image.
  - Closed: the IDE extension and Codex Cloud are explicitly "not open source". The desktop app is not listed among the open components (https://learn.chatgpt.com/docs/open-source.md).
- **Repository:** https://github.com/openai/codex.
  - Apache-2.0, about 134.7k stars, last push 2026-11-10 (GitHub API). Description: "Lightweight coding agent that runs in your terminal".
  - Latest stable release is **`rust-v0.167.0`, 2026-11-06**. Prereleases: `rust-v0.169.0-alpha.7` (2026-11-09) and also `rust-v0.167.0-alpha.4.1` (2026-11-09) (GitHub releases API).
  - The project accepts issues but **no external pull requests** (https://learn.chatgpt.com/docs/open-source.md).
- **Corrections to `00-targets.md`:** none of substance; the version, date, licence and open/closed split all check out.
  - Nuance 1: the README still links to `developers.openai.com/codex`, which now redirects.
  - Nuance 2: a second prerelease line (`0.167.0-alpha.4.1`) ran in parallel.
  - Nuance 3: besides the VS Code-family extension, the IDE surface is reached through Xcode's and JetBrains' own integrations.

---

## 2. Features by area

### 1. Agent loop and execution model

### Threads, turns, items
- What: The core model is a **thread** (a conversation) made of **turns** (one user request and the work that follows), which are made of **items** (messages, command runs, file changes, tool calls). Clients drive turns and stream item events.
- Status: GA (the app-server itself is labelled experimental; see section 6)
- Evidence: https://learn.chatgpt.com/docs/app-server.md (Core primitives, Lifecycle overview)
- Notes: Surface: App-server, CLI, IDE, Desktop (the IDE extension is powered by the app-server).

### Steering an active turn
- What: The user can send input while a turn runs. `turn/steer` appends it to the in-flight turn without starting a new one. In the TUI, a follow-up sent mid-response steers immediately, and an interrupt also stops active Code Mode cells.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/app-server.md (API overview); https://learn.chatgpt.com/docs/changelog (Codex CLI 0.163.0)
- Notes: Surface: CLI, App-server, Desktop.

### Queued messages and queued slash commands
- What: A slash command typed while a turn runs can be queued with Tab and is parsed when it runs. `codex queue --thread <id> --message ...` queues text for an existing session; with `--resume` it also reloads that session through a daemon or a remote server.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (Built-in slash commands; `codex queue`)
- Notes: Surface: CLI. `--resume` was added in 0.166.0 (https://learn.chatgpt.com/docs/changelog). (verified in review)

### Goals with token budget and automatic continuation
- What: `/goal <objective>` attaches a persistent objective to a chat, and the goal can be paused, resumed, edited or cleared. The app-server exposes `thread/goal/set|get|clear` with `tokenBudget`, `tokensUsed` and `timeUsedSeconds`. The `features.goals` flag is described as "persisted goals and automatic continuation". `goals.max_goal_token_budget` caps the budget. An objective is at most 4,000 characters.
- Status: GA (flag marked stable, on by default)
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`/goal`); https://learn.chatgpt.com/docs/app-server.md (Manage a thread goal); https://learn.chatgpt.com/docs/config-file/config-reference.md (`features.goals`, `goals.max_goal_token_budget`); https://learn.chatgpt.com/docs/long-running-work.md
- Notes: Surface: CLI, Desktop, App-server. (verified in review)

### Rollout budget (experimental)
- What: A token budget for a whole rollout with reminder thresholds and weighting of sampled versus prefill tokens.
- Status: experimental ("under development and off by default")
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md (`features.rollout_budget.*`)

### Code Mode
- What: A mode in which the model drives tools through scripts that run in an embedded JavaScript (V8) runtime. Tools can be routed to `direct`, `deferred` (tool search) or `code_mode` surfaces per MCP server. The app-server can share a **remote Code Mode host** over HTTP/2 gRPC (`--code-mode-host`).
- Status: unclear. The config reference says "under development and off by default", while recent releases ship many Code Mode features: bounded output, server-side truncation, ranked tool search.
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md (`features.code_mode.*`, MCP `omit` surfaces); https://learn.chatgpt.com/docs/app-server.md (Connect a remote Code Mode host); https://learn.chatgpt.com/docs/changelog (0.162.0, 0.167.0)
- Notes: Surface: CLI, App-server. (verified in review)

### Parallel read-only tools
- What: Codex can run independent read-only tools in parallel, including skill, memory and conversation-history lookups.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/changelog (Codex CLI 0.163.0)

### Asynchronous safety monitoring that can pause a task
- What: With GPT-6 Astra, a monitor runs asynchronously and can pause a task when it detects potentially unsafe model behaviour, possibly after the triggering activity.
  - On clients with a findings-and-resume flow, the user reviews findings and then resumes.
  - On the **CLI and mobile, and under ZDR or non-US data residency, the task simply ends**.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-approvals-security.md (Safety monitoring and paused tasks)
- Notes: (verified in review)

### Retry and overload handling
- What: Retries honour the server's `Retry-After`. A WebSocket transport falls back to HTTP. When a provider reports the model was not found, Codex stops retrying.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/changelog (0.161.0, 0.162.0, 0.165.0)

### 2. Built-in tools

No single "tools reference" page exists. The tool names below come from the hooks, config and app-server pages.

### Shell and unified exec (`exec_command`, `write_stdin`)
- What: Command execution with resumable processes (unified exec). A later `write_stdin` poll can deliver output from a running command. Managed requirements can disable resumable processes and leave only one-shot `exec_command`. Hooks match these as `Bash`.
- Status: GA (`features.unified_exec` marked stable; user opt-outs are ignored)
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md (`features.unified_exec`, `features.shell_tool`); https://learn.chatgpt.com/docs/hooks.md (Tool coverage)

### `apply_patch`
- What: The file-edit tool. Hooks match it as `apply_patch`, `Edit` or `Write`. Since 0.162.0 it preserves existing CRLF line endings by default.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/hooks.md (Tool coverage); https://learn.chatgpt.com/docs/changelog (0.162.0)

### `view_image`
- What: A built-in tool for opening local images; on by default.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md (`features.view_image`)

### `update_plan`
- What: The local function tool for plans. It is named as a hook-matchable tool.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/hooks.md (Tool coverage)

### Multi-agent tools (`spawn_agent`, `send_input`, `resume_agent`, `wait_agent`, `close_agent`)
- What: The model-facing tools behind sub-agents. Hooks also match `spawn_agent` as `Agent`.
- Status: GA (stable, on by default)
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md (`features.multi_agent`); https://learn.chatgpt.com/docs/hooks.md

### `request_permissions`
- What: The model asks for a subset of filesystem or network permissions. The app-server surfaces this as `item/permissions/requestApproval`. Custom agents can disable it.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/app-server.md (API overview); https://learn.chatgpt.com/docs/agent-configuration/subagents.md (`features.request_permissions_tool`)

### `tool/requestUserInput` (ask the user)
- What: The model prompts the user with one to three short questions, optionally with a free-form "other" choice.
- Status: experimental
- Evidence: https://learn.chatgpt.com/docs/app-server.md (API overview)

### Web search (hosted)
- What: The search modes are `cached` (the default: an OpenAI-maintained index, chosen to reduce prompt-injection exposure), `indexed` (external access gated by the index), `live` (same as `--search`) and `disabled`. Full-access modes default to live. Search does not go through the command sandbox or proxy, and hooks do not see it.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-basic.md (Web search mode); https://learn.chatgpt.com/docs/agent-approvals-security.md; https://learn.chatgpt.com/docs/hooks.md (hosted tools not hooked)

### Tool search / deferred tools
- What: MCP tools can be kept out of the initial tool list and found through tool search ("deferred"). 0.167.0 turned "deferred tool world state" on by default.
- Status: GA (mechanism only partly documented)
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md (MCP `omit` surfaces); https://learn.chatgpt.com/docs/changelog (0.167.0, #58410)

### Managed worktree tools
- What: Tools for creating and listing managed Git worktrees from trusted local projects, when the worktrees feature is enabled.
- Status: GA (behind a feature)
- Evidence: https://learn.chatgpt.com/docs/changelog (Codex CLI 0.162.0)

### Per-tool output budgets
- What: `tool_output_token_limit` sets how much tool output is stored in history. MCP servers can set a per-tool `output_token_limit`.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md; https://learn.chatgpt.com/docs/extend/mcp.md

### 3. Code editing and file operations

### Diff review and inline patch expansion
- What:
  - `/diff` shows the Git diff, including untracked files.
  - Since 0.167.0, patch details can be expanded in the transcript while an approval prompt is open.
  - The Desktop diff pane stages or reverts hunks and files, commits, pushes and opens PRs, and accepts inline comments that Codex addresses.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`/diff`); https://learn.chatgpt.com/docs/changelog (0.167.0); https://learn.chatgpt.com/docs/environments/local-environment.md (Use built-in Git tools)
- Notes: Surface: CLI, Desktop.

### App-server filesystem API
- What: `fs/readFile`, `writeFile`, `createDirectory`, `getMetadata`, `readDirectory`, `remove`, `copy` and `watch`/`unwatch` on absolute paths, so clients can browse a remote host.
- Status: GA (within the experimental app-server)
- Evidence: https://learn.chatgpt.com/docs/app-server.md (API overview)

### File checkpoints / undo
- What: **Nothing documented** for automatic file checkpoints in the CLI. The docs tell users to create Git checkpoints themselves. `thread/revert` removes turns from history but explicitly "doesn't revert local file changes". `thread/rollback` was removed.
- Status: n/a
- Evidence: https://learn.chatgpt.com/docs/codex/cli.md (step 3); https://learn.chatgpt.com/docs/app-server.md (Revert thread history)

### 4. Terminal and shell integration

### Background terminals
- What: `/ps` lists background terminals and their recent output; `/stop` stops them all. The app-server can list, clean and terminate them (experimental).
- Status: GA (CLI); experimental (API)
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`/ps`, `/stop`); https://learn.chatgpt.com/docs/app-server.md

### User shell commands outside the sandbox
- What: `thread/shellCommand` runs a user-initiated command **outside the sandbox with full access**. Presets beginning with `!` are shell commands.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/app-server.md (API overview); https://learn.chatgpt.com/docs/config-file/config-reference.md (`tui.presets`)

### Shell environment policy
- What: `shell_environment_policy` sets which variables reach spawned commands:
  - `inherit = all|core|none`;
  - case-insensitive include and exclude glob filters;
  - `set` values.
  - **`ignore_default_excludes` defaults to `true`**, so variables named `*KEY*`, `*SECRET*` or `*TOKEN*` are passed through unless the user opts in to filtering.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-advanced.md (Shell environment policy); https://learn.chatgpt.com/docs/config-file/config-basic.md (Command environment)
- Notes: (verified in review)

### Shell snapshot, login shell, credential masking
- What: The shell environment is snapshotted to speed up repeated commands. `allow_login_shell = false` rejects login shells. Credentials are masked in supported remote Bash and Zsh snapshot sessions. The patched zsh was dropped in 0.163.0.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md; https://learn.chatgpt.com/docs/changelog (0.163.0, 0.164.0)

### Integrated terminal (Desktop)
- What: Each desktop chat has a terminal scoped to its project or worktree, and ChatGPT can read its output. Project "actions" (such as run or test) appear in the top bar and run in that terminal.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/integrated-terminal.md; https://learn.chatgpt.com/docs/environments/local-environment.md

### 5. Permissions, approvals and sandboxing

### Sandbox modes (legacy model)
- What: The modes are `read-only`, `workspace-write` (the default for local work; network off by default is not confirmed, see Notes) and `danger-full-access`. The sandbox applies to **spawned commands**, not only built-in file operations.
  - The workspace includes the cwd and temp directories such as `/tmp`.
  - `sandbox_workspace_write.writable_roots` adds directories.
  - `network_access = true` turns network on.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/sandboxing.md; https://learn.chatgpt.com/docs/agent-approvals-security.md (Defaults and recommendations)
- Notes: Surface: CLI, IDE, Desktop. (corrected in review: the cited sandboxing page does not say network is off by default for workspace-write and the permissions page gives no default for the built-in `:workspace` profile; the 'network off' default was not confirmed. Spawned-command scope, writable_roots and the default-mode wording were confirmed)

### Approval policies
- What:
  - `on-request` asks only to cross the sandbox boundary.
  - `never` never prompts.
  - A **granular** policy toggles prompt categories independently (sandbox approvals, execpolicy-rule prompts, MCP elicitations, `request_permissions`, skill-script approvals) and auto-rejects the others.
  - `untrusted` is **retired** as an approval policy and can stop the client from starting. The stricter behaviour survives as `[projects."<path>"] trust_level = "untrusted"`.
- Status: GA; `untrusted` deprecated or removed
- Evidence: https://learn.chatgpt.com/docs/agent-approvals-security.md (Migrate from the retired `untrusted`; Run without approval prompts)
- Notes: (verified in review)

### Approval decision scopes
- What: Command approvals return `accept`, `acceptForSession`, `decline`, `cancel`, or **`acceptWithExecpolicyAmendment`** (approve and persist a prefix rule). File-change approvals use the first four. Requests carry the thread and turn ids, the reason, a proposed amendment and network context.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/app-server.md (Approvals)
- Notes: (verified in review)

### Folder trust and onboarding default
- What: On launch Codex detects version control. It recommends Auto (workspace-write plus on-request) for Git folders and read-only for others. It may start read-only until the directory is trusted. Untrusted projects skip the project `.codex/` layers (config, hooks, rules).
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-approvals-security.md (Defaults and recommendations); https://learn.chatgpt.com/docs/config-file/config-basic.md (Configuration precedence)
- Notes: (verified in review)

### Protected paths inside writable roots
- What: Under `workspace-write`, these stay recursively read-only:
  - `<root>/.git` (including the resolved gitdir of a `.git` pointer file);
  - `<root>/.agents`;
  - `<root>/.codex`;
  - `<root>/.aws`.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-approvals-security.md (Protected paths in writable roots)
- Notes: (verified in review)

### Permission profiles
- What: A named policy combining filesystem rules (`read|write|deny` per path) and network rules.
  - Built-ins are `:read-only`, `:workspace` and `:danger-full-access`. A profile can `extends` a built-in or another profile, but not full access; cycles and unknown parents are rejected.
  - Special paths: `:root`, `:minimal`, `:workspace_roots`, `:tmpdir`, `:slash_tmp`, `~/...`, absolute paths.
  - On the same path `deny > write > read`, and the more specific path wins.
  - Profiles **do not compose with** `sandbox_mode`; any `sandbox_mode` anywhere makes Codex use the legacy model.
  - Layers can extend one profile across org and user config.
- Status: beta
- Evidence: https://learn.chatgpt.com/docs/permissions.md
- Notes: (original wording withdrawn: 0.138.0 is not a profile requirement). (corrected in review: the page does not say profiles require 0.138.0; it says a mixed-version rollout may keep `allowed_sandbox_modes` until every client runs 0.138.0 or later. Beta label, built-ins, extends rules and deny > write > read were confirmed at https://learn.chatgpt.com/docs/permissions.md)

### Deny-read globs and their limit
- What: `"**/*.env" = "deny"` under `:workspace_roots`. On Linux, WSL and Windows, matches are **enumerated when each sandboxed process starts**, so files created later are readable by an already-running process. `glob_scan_max_depth` bounds pre-expansion.
- Status: beta
- Evidence: https://learn.chatgpt.com/docs/permissions.md (Deny reads with exact paths or globs)
- Notes: (verified in review)

### OS enforcement: macOS
- What: Seatbelt through `sandbox-exec` with a mode-specific profile, plus a curated platform policy. If a policy cannot be enforced, Codex refuses to run the command rather than run it unsandboxed.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-approvals-security.md (OS-level sandbox); https://learn.chatgpt.com/docs/permissions.md (How enforcement works)
- Notes: (verified in review)

### OS enforcement: Linux and WSL2
- What: bubblewrap plus seccomp. Codex uses the first `bwrap` on PATH, otherwise a bundled helper that needs unprivileged user namespaces. The docs give AppArmor steps for Ubuntu 24.04. Legacy Landlock is rejected for filesystem-restricted policies because it can't isolate app-server Unix sockets.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/sandboxing.md (Prerequisites); https://learn.chatgpt.com/docs/permissions.md; https://learn.chatgpt.com/docs/agent-approvals-security.md
- Notes: (corrected in review: bwrap on PATH, bundled helper needing unprivileged user namespaces, the Ubuntu 24.04 AppArmor steps and the Landlock rejection are confirmed at https://learn.chatgpt.com/docs/sandboxing.md and https://learn.chatgpt.com/docs/permissions.md. The claim 'WSL1 was dropped at 0.115' was not found on either page or in the changelog text read, and is removed as not supported)

### OS enforcement: native Windows
- What: Three implementations:
  - `mxc` (Microsoft Execution Containers): native process isolation, no admin setup, needs Windows 11 24H2/25H2 builds.
  - `elevated` (legacy): dedicated low-privilege sandbox users, ACLs, firewall rules.
  - `unelevated` (legacy): weaker network isolation, no denied reads.
  
  `features.prefer_mxc` selects MXC when available. `codex sandbox uninstall` removes the legacy accounts and rules.
- Status: GA (MXC is recommended on compatible devices)
- Evidence: https://learn.chatgpt.com/docs/windows/windows-sandbox.md; https://learn.chatgpt.com/docs/developer-commands.md (`codex sandbox`)
- Notes: (verified in review)

### Containers and devcontainers
- What: Where Docker blocks namespaces or seccomp, the docs say to make the container the boundary and run `danger-full-access` inside it. They warn that a malicious repo can then exfiltrate Codex credentials from the container.
- Status: GA (guidance)
- Evidence: https://learn.chatgpt.com/docs/agent-approvals-security.md (Run Codex in Dev Containers)

### `codex sandbox` test harness
- What: `codex sandbox macos|linux|windows [--permissions-profile] [--log-denials] CMD` runs a command under the same policy Codex uses. The aliases are `codex debug`, `seatbelt` and `landlock`.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-approvals-security.md (Test the sandbox locally)
- Notes: (verified in review)

### Command rules (execpolicy, Starlark)
- What: `.rules` files in `rules/` next to any config layer hold `prefix_rule(pattern, decision=allow|prompt|forbidden, justification, match, not_match)`.
  - The most restrictive decision wins.
  - `match`/`not_match` are inline tests checked at load time.
  - `bash -lc` scripts are split with tree-sitter when they are plain linear chains; anything with redirects, substitutions, variables, globs or control flow is evaluated as one opaque invocation.
  - Approvals saved from the TUI go to `~/.codex/rules/default.rules`.
  - "Smart approvals" can propose a prefix rule.
  - `codex execpolicy check` tests rules.
- Status: experimental
- Evidence: https://learn.chatgpt.com/docs/agent-configuration/rules.md
- Notes: (verified in review)

### Auto-review ("Approve for me", internally "Guardian")
- What: With `approvals_reviewer = "auto_review"`, eligible approval requests go to a separate reviewer agent instead of the user.
  - What the reviewer sees: a compact transcript and the exact request. It can make read-only checks. Hidden reasoning is excluded.
  - Failures: prompt-build, session and parse failures **fail closed**.
  - Circuit breaker: the CLI interrupts the turn after 3 consecutive or 10-of-50 denials (or on the first denial for cyber models).
  - Override: `/approve` grants one retry of one exact denied action, recorded as a developer-scoped marker, and still re-reviewed.
  - Policy: local `[auto_review].policy`/`extra_policy` or managed `guardian_policy_config`/`guardian_extra_policy`.
  - Cost: it uses extra model calls.
- Status: GA (desktop mode availability varies)
- Evidence: https://learn.chatgpt.com/docs/sandboxing/auto-review.md; https://learn.chatgpt.com/docs/agent-approvals-security.md (Automatic approval reviews)
- Notes: (verified in review)

### MCP and app tool approvals by annotation
- What: App or MCP tools that advertise side effects can trigger approval. A destructive annotation always requires approval unless a read annotation is present. Per server and per tool, `approval_mode` can be `auto`, `prompt`, `writes` or `approve`.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-approvals-security.md; https://learn.chatgpt.com/docs/extend/mcp.md (Other configuration options)

### Network proxy for sandboxed commands
- What: The proxy is opt-in (`features.network_proxy`). It is separate from turning network on; with network on and the proxy off, egress is unrestricted.
  - Rules: allowlist-first domain rules (`*.x` for subdomains, `**.x` for apex plus subdomains, `*` allow-only); deny wins.
  - Local guard: local and private destinations are blocked unless exact literals are allowed.
  - DNS checks: a best-effort check blocks failed lookups and non-public resolutions. The docs say it reduces DNS rebinding but doesn't eliminate it.
  - Listeners: SOCKS5 and HTTP proxy listeners on loopback; a Unix socket allowlist; `dangerously_*` escape hatches.
  - What it does not filter: web search, apps, MCP, browser or computer use, cloud, or Codex's own model traffic.
- Status: experimental (off by default)
- Evidence: https://learn.chatgpt.com/docs/agent-approvals-security.md (Network access); https://learn.chatgpt.com/docs/permissions.md (Network permissions; What the network proxy does not control)
- Notes: 0.163.0 made it check permission before DNS lookup and reject duplicate Host headers (https://learn.chatgpt.com/docs/changelog). (verified in review)

### Network approval prompts
- What: Requests to destinations that need approval prompt the user. Concurrent prompts for one endpoint are serialised, and prompts close when the request ends. Managed `network_proxy.review_domains` forces review even for allowed hosts.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/changelog (0.167.0); https://learn.chatgpt.com/docs/config-file/config-reference.md (`network_proxy.review_domains`)

### Subagents inherit permissions
- What: Subagents inherit the parent turn's sandbox and approval settings, including `/permissions` and `--yolo` changes. A custom agent file cannot override `sandbox_mode`, `approval_policy`, `model_provider` or `mcp_servers`. Approvals from inactive agent threads surface in the CLI overlay with the source thread label (`o` opens that thread). In non-interactive runs a needed approval fails back to the parent.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-configuration/subagents.md (Approvals and sandbox controls)
- Notes: (verified in review)

### 6. Context management and compaction

### Automatic and manual compaction
- What: Compaction has three triggers:
  - `/compact` summarises the visible chat after a confirmation;
  - `model_auto_compact_token_limit` (scope `total` or `body_after_prefix`) triggers auto-compaction;
  - `model_post_turn_compact_threshold_percent` compacts at the end of a turn.
  
  The prompt can be overridden (`compact_prompt`, `experimental_compact_prompt_file`). Providers can declare `remote_compaction = v2` to compact server-side through Responses. `thread/compact/start` triggers it over the API. `PreCompact` and `PostCompact` hooks exist.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`/compact`); https://learn.chatgpt.com/docs/config-file/config-reference.md; https://learn.chatgpt.com/docs/app-server.md; https://learn.chatgpt.com/docs/hooks.md
- Notes: 0.164.0 fixed failed compaction leaving partial summaries; 0.167.0 fixed token estimates (https://learn.chatgpt.com/docs/changelog).

### `/recap` and automatic recaps
- What: `/recap` summarises the chat without compacting it. `tui.auto_recap` (on by default) generates recaps while the terminal is unfocused.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`/recap`); https://learn.chatgpt.com/docs/config-file/config-reference.md (`tui.auto_recap`)

### `/side` ephemeral side chats
- What: An ephemeral fork with its own transcript, opened without leaving the main chat. The parent's status stays visible.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`/side`)
- Notes: (verified in review)

### Skills catalog budget
- What: The initial list of skills uses at most 2% of the context window, or 8,000 characters when the window is unknown. Descriptions are shortened first, then skills are omitted with a warning. `skills.max_context_tokens` sets the budget, capped at 10,000.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/build-skills.md
- Notes: (verified in review)

### Large hook output offload
- What: When a hook's `additionalContext` exceeds `additionalContextLimit` (default 2,500 tokens), the full text is saved to disk and only a preview is sent.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/hooks.md (Large hook output)
- Notes: (verified in review)

### MCP schema compaction
- What: An ordinary MCP tool's input schema is compacted when it exceeds a byte threshold (default 5,000).
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md

### 7. Memory and persistent instructions

### AGENTS.md discovery chain
- What: The chain has three levels:
  - global: `$CODEX_HOME/AGENTS.override.md`, else `AGENTS.md`;
  - project: every directory from the project root down to the cwd, checking `AGENTS.override.md`, then `AGENTS.md`, then `project_doc_fallback_filenames`;
  - merge: files are concatenated root-first, so closer files come later.
  
  Empty files are skipped. Inclusion stops at `project_doc_max_bytes` (32 KiB default). The chain is rebuilt each run or session. `project_root_markers` changes how the root is found.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-configuration/agents-md.md; https://learn.chatgpt.com/docs/config-file/config-advanced.md (Project root detection)
- Notes: Pages disagree on whether the byte limit is per file or combined; see section 6.

### `/init`
- What: Generates an `AGENTS.md` scaffold in the current directory.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (Built-in slash commands)

### Instruction overrides
- What: `model_instructions_file` replaces the built-in instructions. `developer_instructions` adds to them. Managed `additional_developer_instructions` can add up to 10,000 estimated tokens.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md

### Local memories
- What: When enabled, Codex turns eligible idle past chats into local memory files under `$CODEX_HOME/memories_v2/` (or `memories/`). Generation:
  - skips active or short sessions;
  - **redacts secrets**;
  - runs in the background;
  - pauses when the rate-limit remainder is low.
  
  `/memories` controls use and generation per chat. `memories.disable_on_external_context` keeps chats that used MCP or web search out of generation. The extract and consolidation models are configurable. External-agent memories can be imported.
- Status: GA; **off by default**
- Evidence: https://learn.chatgpt.com/docs/customization/memories.md; https://learn.chatgpt.com/docs/changelog (0.165.0: memory v2 default for fresh profiles)
- Notes: Surface: CLI, Desktop, IDE (uses the host store). (verified in review)

### Computer History (Desktop, macOS)
- What: An opt-in feature that turns activity across allowed apps and sites into memories and a timeline.
- Status: unclear
- Evidence: https://learn.chatgpt.com/docs/customization/memories.md (mention only; the dedicated page was not read)

### 8. Planning, tasks and modes

### Plan mode
- What: `/plan [prompt]` switches the chat to plan mode. `plan_mode_reasoning_effort` overrides the effort. It is unavailable while Codex is working. The app-server exposes "collaboration mode" presets.
- Status: GA (collaboration modes over the API are experimental)
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`/plan`); https://learn.chatgpt.com/docs/config-file/config-reference.md; https://learn.chatgpt.com/docs/app-server.md (`collaborationMode/list`)

### Presets
- What: Ten numbered slots (`/presets`, `tui.presets.<0-9>`) hold reusable prompts or slash commands of up to 8,000 bytes.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md; https://learn.chatgpt.com/docs/config-file/config-reference.md

### Custom prompts
- What: Markdown files in `~/.codex/prompts/` become `/prompts:<name>` with `$1`–`$9`, `$ARGUMENTS` and named placeholders.
- Status: deprecated (use skills)
- Evidence: https://learn.chatgpt.com/docs/custom-prompts.md

### 9. Sub-agents and multi-agent orchestration

### Subagent workflows
- What: Codex spawns parallel agents when asked or when AGENTS.md or skill instructions request it, waits, and consolidates their results.
  - CLI: `/subagents` switches threads.
  - IDE: shows a background-agent panel.
  - Desktop: surfaces subagent threads.
  
  The docs warn about higher token use and about parallel write-heavy work.
- Status: GA (on by default)
- Evidence: https://learn.chatgpt.com/docs/agent-configuration/subagents.md

### Built-in and custom agents
- What: The built-ins are `default`, `worker` and `explorer`.
  - Custom agents are TOML files in `~/.codex/agents/` or `.codex/agents/`.
  - Required fields: `name`, `description`, `developer_instructions`.
  - Optional: model, effort, summary and verbosity.
  - A custom agent can disable `features.shell_tool`, `apps`, `plugins`, `memories` and `request_permissions_tool`, and individual skills. It cannot widen permissions.
  - `[agents]` sets `max_concurrent_threads_per_session`, default subagent model and effort, and `interrupt_message`. `agents.<name>.config_file` points a role at its own config layer.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-configuration/subagents.md (Custom agents); https://learn.chatgpt.com/docs/config-file/config-reference.md (`agents.*`)
- Notes: (verified in review)

### Agent command center
- What: `codex agents` or `/agents` is an overview of chats on a shared local or remote server. It groups by directory or repo, pins tasks with `p`, marks unread results, and sorts by recency within status groups.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`codex agents`, `/agents`); https://learn.chatgpt.com/docs/changelog (0.162.0, 0.165.0, 0.167.0)

### Subagent lifecycle persistence
- What: Archiving, unarchiving or deleting a thread cascades to spawned descendants. `SubagentStart` and `SubagentStop` hooks exist.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/app-server.md (`thread/archive`, `thread/delete`); https://learn.chatgpt.com/docs/developer-commands.md (`codex archive`); https://learn.chatgpt.com/docs/hooks.md

### 10. Background, long-running and scheduled work

### Scheduled tasks (Desktop and web)
- What: Two kinds:
  - standalone tasks, where each run opens a new chat and findings go to a Scheduled inbox;
  - tasks inside an existing chat ("heartbeat").
  
  On the desktop, a task can run in the local project or a background worktree, needs the app running, and runs **unattended with the default sandbox settings**. The CLI has no scheduled-task management UI.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/automations.md; https://learn.chatgpt.com/docs/glossary.md (Heartbeat)
- Notes: (verified in review)

### Background server (daemon)
- What: The CLI can connect to a managed local background app-server that stays running between sessions. `/daemon` restarts or updates it. `--no-daemon` bypasses it. `features.daemon_auto_start = false` disables auto-start. Its Unix socket is host-local.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`/daemon`); https://learn.chatgpt.com/docs/environments/network-file-systems.md

### Background hooks
- What: Hooks with `async = true` run without blocking, up to 8 concurrent per session, and cannot block or rewrite anything.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/hooks.md (Run hooks in the background)
- Notes: (verified in review)

### Prevent idle sleep
- What: `features.prevent_idle_sleep` keeps the machine awake while a turn runs.
- Status: experimental
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md

### 11. Sessions: persistence, resume, fork, checkpoints, undo

### Rollout files and SQLite index
- What: Conversation events are saved in `rollout-*.jsonl` files, each written by one process. A SQLite index lists conversations and stores goals and memory-processing state, in WAL mode locally. On Linux NFS, Codex switches to DELETE journaling, and `codex migrate-nfs-db` migrates existing databases.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/environments/network-file-systems.md; https://learn.chatgpt.com/docs/config-file/config-reference.md (`sqlite_home`, `unsafe_enforce_sqlite_journal_mode`)

### Resume
- What: `codex resume [ID] [--last] [--all]` and `/resume`. `--last` is scoped to the cwd unless `--all`.
  - If the cwd differs from the session's, Codex asks, or `tui.resume_cwd` decides.
  - Resume restores saved permissions and network approvals, subject to explicit overrides and managed policy.
  - `codex exec resume --last|<ID>` resumes headless runs.
  - Since 0.167.0, a chat whose provider changed can be read and re-prompted in a new chat.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`codex resume`); https://learn.chatgpt.com/docs/non-interactive-mode.md (Resume a non-interactive session); https://learn.chatgpt.com/docs/changelog (0.163.0, 0.167.0)

### Fork
- What: `codex fork` or `/fork` branches a session into a new chat. `--worktree` forks into a managed Git worktree; it needs an explicit id. The API `thread/fork` copies history up to `lastTurnId`, or makes an in-memory `ephemeral` fork. "Attached thread forks" are experimental. The Python SDK forks to switch providers.
- Status: GA (attached forks experimental)
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`codex fork`); https://learn.chatgpt.com/docs/app-server.md; https://learn.chatgpt.com/docs/codex-sdk.md; https://learn.chatgpt.com/docs/changelog (0.165.0)

### Archive, delete, rename, export
- What: `codex archive`/`unarchive`/`delete`, `/rename` and `/export` (as Markdown). `--force` delete works only by UUID, so an ambiguous name cannot be deleted silently.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md

### History revert
- What: `thread/revert` drops a turn and every later turn from history. It doesn't touch files and isn't supported for ephemeral threads.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/app-server.md (Revert thread history)

### Ephemeral runs and history retention
- What: `codex exec --ephemeral` skips the rollout files. `history.persistence = "none"` and `history.max_bytes` control `history.jsonl`.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/non-interactive-mode.md; https://learn.chatgpt.com/docs/config-file/config-advanced.md (History persistence)

### 12. Git and version control

### Git repository requirement
- What: `codex exec` refuses to run outside a Git repo unless `--skip-git-repo-check` is passed.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/non-interactive-mode.md (Git repository required)
- Notes: (verified in review)

### Managed worktrees and Handoff (Desktop; CLI `/worktree`)
- What: Each chat can run in a Codex-managed worktree under `$CODEX_HOME/worktrees`, which starts from the selected branch HEAD plus any uncommitted changes, in detached HEAD.
  - Handoff moves the chat and its code between Local and Worktree.
  - `.worktreeinclude` copies chosen ignored files such as `.env`.
  - The latest 15 managed worktrees are kept.
  - Setup scripts run per worktree.
  - In the CLI, `/worktree` starts or continues a chat in a managed worktree.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/environments/git-worktrees.md; https://learn.chatgpt.com/docs/environments/local-environment.md; https://learn.chatgpt.com/docs/developer-commands.md (`/worktree`)

### Local review (`/review`, `codex review`)
- What: A dedicated reviewer reports prioritised findings without modifying the tree. The targets are uncommitted changes, base branch (merge base), a commit, a custom prompt, and on the desktop also the last turn. `review_model` overrides the model. The API is `review/start`.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/code-review.md (Review local changes); https://learn.chatgpt.com/docs/developer-commands.md (`codex review`); https://learn.chatgpt.com/docs/app-server.md (Review)

### GitHub PR review (`@codex review`, automatic)
- What: Reviews are triggered by an `@codex review` comment or an automatic per-repo or per-person setting. The review posts a standard GitHub review **limited to P0 and P1 issues** and follows `## Code Review Rules` sections in the nearest `AGENTS.md`. `@codex security review` requests the deeper Security Review (research preview).
- Status: GA (Security Review preview)
- Evidence: https://learn.chatgpt.com/docs/third-party/github.md; https://learn.chatgpt.com/docs/code-review.md
- Notes: (verified in review)

### GitLab MR review
- What: `@codex review` and automatic MR reviews run in Codex Cloud. Self-managed and Dedicated GitLab need an admin template.
- Status: beta (GitLab page) or preview (code-review page)
- Evidence: https://learn.chatgpt.com/docs/third-party/gitlab.md; https://learn.chatgpt.com/docs/code-review.md

### Desktop Code Review plugin
- What: A PR inbox (assigned, team, authored), PR summary, changes, checks and stacked-PR navigation, a "Review with Codex" chat, and custom review instructions.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/code-review.md (Pull request reviews)

### Apply cloud diff locally
- What: `codex apply` applies the latest diff from a cloud chat with `git apply` and exits non-zero on conflict.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`codex apply`)

### 13. Extensibility

### MCP client
- What:
  - Transports: STDIO servers, and Streamable HTTP with bearer token, static or env headers, a header-helper command, OAuth (CIMD, DCR, pre-registered clients, RFC 8707 resource), or the ChatGPT session for first-party servers.
  - Instructions: Codex reads the server's `instructions` (the first 512 characters should stand alone).
  - Config: `config.toml` holds `enabled_tools`/`disabled_tools`, timeouts, `required` and `startup_readiness`, per-tool approval mode and output limit.
  - Placement: `environment_id` runs a server on a remote executor.
  - Commands: `codex mcp add|list|login [--no-browser]`, `/mcp`, `/mcp login`.
  - Sharing: one config serves CLI, IDE and Desktop on the same host.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/extend/mcp.md

### Codex as an MCP server
- What: **Removed.** `codex mcp-server` and the `codex-mcp-server` binary are gone; integrations must move to the app-server, which is not MCP.
- Status: deprecated (removed)
- Evidence: https://learn.chatgpt.com/docs/mcp-server.md
- Notes: (verified in review)

### App-server protocol
- What: JSON-RPC 2.0 without the `jsonrpc` field.
  - Transports: stdio JSONL (default), WebSocket (experimental), Unix socket (WebSocket over UDS), or off.
  - Handshake: one `initialize` per connection, with capability negotiation (`experimentalApi`, notification opt-out, attestation).
  - Methods: threads, turns, review, `command/exec` (sandboxed) and `process/spawn` (unsandboxed, experimental), fs, models, skills, hooks, plugins and marketplaces, apps, MCP status and OAuth, config read and write, requirements read, external-agent import, auth and rate limits, feedback.
  - Server-initiated requests: approvals and elicitations.
  - Schemas: `generate-ts` and `generate-json-schema` produce version-matched schemas.
  - Overload: bounded queues answer `-32001` "Server overloaded; retry later".
- Status: experimental ("isn't supported for production workloads", per the MCP-removal page) although it powers the IDE extension
- Evidence: https://learn.chatgpt.com/docs/app-server.md; https://learn.chatgpt.com/docs/mcp-server.md
- Notes: (verified in review)

### App-server WebSocket auth
- What: The auth modes are `capability-token` (token file or SHA-256 verifier) and `signed-bearer-token` (shared secret, issuer, audience, clock skew). Auth is enforced before `initialize`. `/readyz` and `/healthz` respond, and requests with an `Origin` header get 403. **Non-loopback listeners allow unauthenticated connections by default "during rollout".**
- Status: experimental
- Evidence: https://learn.chatgpt.com/docs/app-server.md (Protocol)
- Notes: (verified in review)

### TypeScript SDK
- What: `@openai/codex-sdk` (Node 18+) provides `startThread()`, `run()`, `resumeThread(id, opts)`, a `threadSource` label, a structured `config` and raw `configOverrides`.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/codex-sdk.md
- Notes: (verified in review)

### Python SDK
- What: `openai-codex` (Python 3.10+) controls the local app-server over JSON-RPC and pins a CLI runtime.
  - Classes: `Codex` and `AsyncCodex`.
  - `ExternalMessage` passes third-party content at tool-output priority, below user and developer instructions.
  - `include_turns=False` resumes or forks without loading history.
  - Turn options: per-turn `Sandbox` presets and service tier.
- Status: GA ("stable release"; `--pre` for prereleases)
- Evidence: https://learn.chatgpt.com/docs/codex-sdk.md
- Notes: (verified in review)

### Hooks
- What: The events are `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `PreModelRequest`, `PostModelRequest`, `SubagentStart`, `SubagentStop`, `Stop` and `Interrupt`.
  - Sources: `hooks.json` or `[hooks]` in any config layer, and plugins. Every source's hooks run; layers don't replace one another.
  - Handlers: `command` and `mcp_tool` are supported (`prompt` and `agent` are parsed but skipped). Matching command hooks run concurrently.
  - Trust: non-managed hooks must be reviewed and are **trusted by hash**, so a changed hook needs review again. `--dangerously-bypass-hook-trust` skips this.
  - PreToolUse can deny (exit 2 or JSON), add context or rewrite input (`updatedInput`).
  - Managed hooks: set in `requirements.toml`; `allow_managed_hooks_only` ignores all others.
- Status: GA (on by default)
- Evidence: https://learn.chatgpt.com/docs/hooks.md
- Notes: (corrected in review: the hooks page states no maturity label; it only says hooks are on by default and can be disabled with `[features] hooks = false`, so 'GA' is the survey's inference. Everything else on trust, handler types, events, the 8-hook async cap and the 2,500-token default was confirmed at https://learn.chatgpt.com/docs/hooks.md)

### Skills
- What: A folder with `SKILL.md` (name and description) plus optional scripts and references, loaded by progressive disclosure.
  - Scopes: `.agents/skills` from the cwd up to the repo root, `~/.agents/skills`, `/etc/codex/skills`, and bundled system skills (`skill-creator`, `skill-installer`).
  - Invocation: explicit with `$skill` or implicit by description.
  - `agents/openai.yaml` sets UI metadata, `allow_implicit_invocation` and tool dependencies.
  - Symlinks are followed.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/build-skills.md; https://learn.chatgpt.com/docs/skills-and-plugins.md
- Notes: (verified in review)

### Plugins and marketplaces
- What: An installable bundle of skills, MCP servers and hooks (manifest `.codex-plugin/plugin.json`).
  - Catalog: one "universal" directory shared with ChatGPT.
  - CLI: `/plugins` and `codex plugin marketplace`.
  - Marketplaces can be Git, local or remote, and admins can sync one from GitHub.
  - The IDE extension **doesn't support plugins**.
  - Plugin hooks get `PLUGIN_ROOT`/`PLUGIN_DATA` and also `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA` for compatibility.
- Status: GA (the app-server plugin install API is "under development")
- Evidence: https://learn.chatgpt.com/docs/plugins.md; https://learn.chatgpt.com/docs/hooks.md (Plugin-bundled hooks); https://learn.chatgpt.com/docs/app-server.md

### Apps (connectors)
- What: Connector-backed tools from plugins, browsed with `/apps` and inserted as `$app-slug`. Their traffic doesn't pass the command proxy. Per-app `approvals_reviewer` is configurable.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`/apps`); https://learn.chatgpt.com/docs/config-file/config-reference.md (`features.apps`, `apps.<id>.approvals_reviewer`)

### Import from other agents
- What: `/import` (CLI) or Settings > Import (Desktop) migrates config, skills, AGENTS.md, plugins, MCP config, projects and up to 50 chats from the last 30 days (CLI; see Notes).
  - Sources: Claude Code or Cursor; the desktop app also accepts Claude Cowork.
  - Behaviour: the existing agent's setup is not changed.
  - API: `externalAgentConfig/detect|import`.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/import.md; https://learn.chatgpt.com/docs/developer-commands.md (`/import`); https://learn.chatgpt.com/docs/app-server.md
- Notes: (corrected in review: the 50-chat/30-day cap is stated for the CLI; the desktop page states no cap. Project memories are imported only from Claude Code, and hooks, slash commands and subagents are also imported, per https://learn.chatgpt.com/docs/import.md; the memories page itself does not mention import)

### `notify` external program
- What: Runs a program on `agent-turn-complete`, separate from the built-in TUI notifications (OSC 9 or BEL). It cannot be set from project config.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-advanced.md (Notifications)

### 14. Models, providers and inference configuration

### Model lineup
- What: GPT-6 Astra (most capable), GPT-6.1 Sol (changelog claims it became the default in the bundled and Bedrock catalogs in 0.161.0; not confirmed), GPT-6.1 Terra (fast, vision and computer use) and GPT-6 Luna (fast and cheap). GPT-5.5 left ChatGPT-sign-in Codex on 2026-10-14; it stays available with API keys.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/models.md; https://learn.chatgpt.com/docs/changelog (0.161.0; GPT-5.5 retirement)
- Notes: (corrected in review: the four model descriptions and the GPT-5.5 retirement on 2026-10-14 are confirmed at https://learn.chatgpt.com/docs/models.md, but that page names no default model; the claim that GPT-6.1 Sol became the default in the bundled and Bedrock catalogs in 0.161.0 was not confirmed)

### Reasoning effort, verbosity, Fast and Ultrafast tiers
- What:
  - Effort levels: `low` through `ultra`, depending on the model. Effort can change mid-conversation (default since 0.165.0).
  - Fast mode: `/fast` or `service_tier = "fast"` speeds up the model and costs 2.5x included limits (2x on credits).
  - Ultrafast: Astra and Sol on eligible plans.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-configuration/speed.md; https://learn.chatgpt.com/docs/agent-configuration/subagents.md; https://learn.chatgpt.com/docs/changelog (0.165.0)

### Custom providers
- What: `[model_providers.<id>]` sets `base_url`, `env_key`, headers, a command-backed bearer token (`auth.command`, refresh interval configurable; 300 s is only the doc example), native gateway OAuth and capability flags.
  - Built-ins: `openai`, `ollama`, `lmstudio`, `amazon-bedrock`, `gcp` (Vertex).
  - The provider must speak the **Responses API**; `wire_api = "chat"` is not supported.
  - `openai_base_url` points the built-in provider elsewhere.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-advanced.md (Custom model providers); https://learn.chatgpt.com/docs/models.md (Other models)
- Notes: (corrected in review: 'Responses API only, `wire_api = "chat"` unsupported' is confirmed at https://learn.chatgpt.com/docs/models.md, not on the config-advanced page. The 300 s figure is only the example `refresh_interval_ms` value; it is configurable and 0 means refresh only after an auth retry)

### Local models (`--oss`)
- What: `--oss` with `--local-provider` or `oss_provider` (ollama or lmstudio). `codex exec` errors if none is set.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-advanced.md (OSS mode)

### Model catalog override
- What: `model_catalog_json` points at a custom catalog, and a profile can override it. `model/list` and `modelProvider/capabilities/read` exist over the API.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-advanced.md (Profiles); https://learn.chatgpt.com/docs/app-server.md

### 15. UI surfaces

### TUI
- What:
  - Display: fullscreen or scrollback mode (`/tui`, `/raw`), Kitty and Sixel image previews, `/statusline` and `/title` customisation.
  - Keys: a configurable keymap with a leader key (Ctrl+X), Vim mode, transcript selection and search.
  - Other: themes, terminal "pets", voice (`/voice`), and `/config` search.
- Status: GA (voice experimental)
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md; https://learn.chatgpt.com/docs/config-file/config-reference.md (`tui.*`); https://learn.chatgpt.com/docs/changelog (0.163.0–0.166.0)

### IDE extension
- What: Ships for VS Code, Cursor and Windsurf (`openai.chatgpt`). It attaches open files and selections, reviews diffs in place, and delegates to the cloud. `chatgpt.runCodexInWindowsSubsystemForLinux` keeps the agent in WSL. Xcode and JetBrains have their own Codex integrations.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/codex/ide.md; https://learn.chatgpt.com/docs/agent-approvals-security.md (OS-level sandbox)

### Desktop (ChatGPT desktop app, Codex mode)
- What:
  - Chat placement: projects, parallel chats, Local, Worktree or Cloud.
  - Built in: diff pane with Git, integrated terminal, built-in browser, file previews, scheduled tasks, plugins and skills.
  - Composer: suggests a next prompt ("composer predictions", beta, Pro).
  - `codex app` opens it from the terminal, and `/app` moves the session there.
  - On Linux the app is a preview.
- Status: GA (Linux preview)
- Evidence: https://learn.chatgpt.com/docs/app.md; https://learn.chatgpt.com/docs/linux/linux-app.md; https://learn.chatgpt.com/docs/developer-commands.md (`codex app`, `/app`); https://learn.chatgpt.com/docs/changelog (Composer predictions)

### Mobile
- What: Cloud tasks, or a paired Mac or Windows host reached through a QR code and a relay. The phone can start, steer, approve and review.
- Status: GA (rollout-dependent)
- Evidence: https://learn.chatgpt.com/docs/mobile.md; https://learn.chatgpt.com/docs/remote-connections.md

### Codex Micro (hardware)
- What: A limited-run keypad from Work Louder with "Agent Keys", a dial and voice. It drives desktop chats over USB-C or Bluetooth.
- Status: GA (limited run)
- Evidence: https://learn.chatgpt.com/docs/features/codex-micro.md

### 16. Browser automation and computer use

### Built-in browser
- What: Desktop only; not in the CLI or IDE. Admins can restrict origins, uploads, downloads and developer tools. Page content is treated as untrusted.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/browser.md

### Computer Use
- What: Operates GUIs on macOS and Windows from the desktop app, with per-app access rules. Windows rules match signed publisher and product. App-level prompts go to the user, and individual tool calls can go to Auto-review.
- Status: GA (region-dependent)
- Evidence: https://learn.chatgpt.com/docs/computer-use.md; https://learn.chatgpt.com/docs/config-file/config-reference.md (`computer_use.windows.*`); https://learn.chatgpt.com/docs/sandboxing/auto-review.md

### Browser use via extension / CDP
- What: Codex can operate existing Chrome tabs through the ChatGPT browser extension, and full CDP access is available ("Developer mode"). Each can be switched off by requirement.
- Status: GA (as admin-controllable features)
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md (`features.browser_use_external`, `features.browser_use_full_cdp_access`)

### 17. Remote, cloud, headless, CI and distributed execution

### `codex exec`
- What:
  - Default sandbox: read-only.
  - Output: progress on stderr, the final message on stdout. `--json` emits a JSONL event stream (`thread.started`, `turn.*`, `item.*`, `error`, with usage). `--output-schema` enforces a JSON Schema on the final answer, and `-o` writes it to a file.
  - Input: stdin as context, or `-` as the prompt.
  - Isolation flags: `--ephemeral`, `--ignore-user-config`, `--ignore-rules`.
  - `--approve-for-me` routes approvals to Auto-review. `--full-auto` was removed. A failing `required` MCP server makes it exit.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/non-interactive-mode.md
- Notes: (verified in review)

### GitHub Action
- What: `openai/codex-action@v1` installs the CLI, starts a **Responses API proxy** so the job never sees the raw key, and runs `codex exec`. By default `safety-strategy: drop-sudo` removes sudo for the rest of the job; `unprivileged-user` is the other option. `allow-users` and `allow-bots` gate who can trigger it. The documented auto-fix pattern splits a read-only Codex job from a write job that opens the PR.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/github-action.md; https://learn.chatgpt.com/docs/non-interactive-mode.md (Autofix CI failures)
- Notes: (corrected in review: https://learn.chatgpt.com/docs/github-action.md confirms the default `drop-sudo` and `unprivileged-user`, but also lists `read-only` and `unsafe` (required on Windows), and does not itself explain the proxy; https://learn.chatgpt.com/docs/non-interactive-mode.md says the proxy keeps the key from being handed directly to the job's shell steps. By default only users with write access can trigger it)

### Remote TUI against an app-server
- What: `codex app-server --listen ws://…|unix://…` on one machine, and `codex --remote ws(s)://…|unix://… --remote-auth-token-env VAR` on another. The docs say to use plain ws only on localhost or through an SSH tunnel.
- Status: experimental
- Evidence: https://learn.chatgpt.com/docs/app-server.md (Connect the CLI terminal UI)

### Remote control and pairing
- What: `codex remote-control [start|stop|pair] [--json]` starts the daemon with remote control enabled and prints short-lived pairing codes. Admins can disable it with `allow_remote_control = false`.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`codex remote-control`); https://learn.chatgpt.com/docs/config-file/config-reference.md
- Notes: (verified in review)

### SSH remote projects (Desktop)
- What: The app reads concrete host aliases from `~/.ssh/config` and starts the **remote Codex app-server over SSH** in the remote user's login shell. Chats then run on the remote filesystem and shell.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/remote-connections.md (Connect to an SSH host)

### Handoff between hosts
- What: Moves a chat **and its Git state** between the local computer and a connected remote host that has a saved project for the same repo. A running response is interrupted first. Handoff to Codex Cloud is not supported.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/remote-connections.md (Hand off a chat between hosts)
- Notes: (corrected in review: the page says Handoff moves a chat and its Git state between the local computer and a connected remote host, needs a saved project for the same repo on both, and stops a running response first; the phrase 'to a worktree' is not stated on https://learn.chatgpt.com/docs/remote-connections.md)

### Relay for connected hosts
- What: A "secure relay layer" keeps paired machines reachable across the user's devices without exposing them to the internet.
- Status: GA (`vendor-claim`: mechanism not described)
- Evidence: https://learn.chatgpt.com/docs/remote-connections.md

### Remote executors
- What: An MCP server can run "on a remote executor" through `environment_id`. `env_vars` with `source = "remote"` read the executor's environment. `codex exec-server --remote` accepts `CODEX_API_KEY`. A standalone executor ships in the CLI and app-server packages since 0.166.0.
- Status: unclear (no dedicated page)
- Evidence: https://learn.chatgpt.com/docs/extend/mcp.md (Other configuration options); https://learn.chatgpt.com/docs/config-file/environment-variables.md; https://learn.chatgpt.com/docs/changelog (0.166.0)

### Codex Cloud (current)
- What: Each task gets its own workspace and keeps running while the user's computer sleeps.
  - Default environment: starts without a repo.
  - Reusable environments: Codex inspects the repos, installs and tests, and records an **install script** and a **start skill**. Publishing captures the prepared filesystem.
  - Refresh: repositories are refreshed in the background with caches kept.
  - Network: public internet is all, package-manager preset, or extra domains. Tailscale VPN gives access to private networks. A published egress IP feed exists.
  - Secrets: env vars, or **network secrets**, where programs see a placeholder and the proxy substitutes the real value only for allowed HTTPS:443 destinations. A personal vault exists, and environments can be shared within a workspace.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/cloud.md; https://learn.chatgpt.com/docs/environments/cloud-environments.md; https://learn.chatgpt.com/docs/environments/modes.md
- Notes: (verified in review)

### Codex Cloud (Legacy)
- What: A two-phase runtime: setup has network and secrets, the agent phase runs offline by default, and secrets are removed before the agent phase. Still used for Code Review and the Linear and GitHub integrations; OpenAI plans to deprecate it.
- Status: deprecated (planned)
- Evidence: https://learn.chatgpt.com/docs/agent-approvals-security.md (Sandbox and approvals); https://learn.chatgpt.com/docs/cloud/internet-access.md; https://learn.chatgpt.com/docs/environments/cloud-environments.md

### Cloud from the CLI
- What: `codex cloud` opens a picker, `codex cloud exec` submits a task, and `codex cloud list [--json]` lists recent tasks with id, status, environment, `is_review` and `attempt_total`.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md (`codex cloud`)

### Slack and Linear
- What:
  - Slack: `@ChatGPT` in Slack can delegate repository work to a Codex Cloud task; this needs a published, workspace-shared environment.
  - Linear: assigning an issue or mentioning `@Codex` creates a cloud chat. Triage rules can auto-assign. The repo is chosen from Linear's suggestion or the most recent environment.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/third-party/slack.md; https://learn.chatgpt.com/docs/third-party/linear.md

### 18. Collaboration, teams and enterprise administration

### Managed requirements (`requirements.toml`, MDM, Agent Security)
- What: Admins enforce:
  - allowed approval policies, sandbox modes, reviewers, permission profiles (omitted ones are denied, including future built-ins) and web search modes;
  - deny-read paths, command rules and per-host sandbox overrides (`remote_sandbox_config`);
  - pinned feature flags, managed hooks and network requirements.
  
  On local machines MDM outranks Agent Security, which outranks the system requirements file.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/enterprise/managed-configuration.md (headings); https://learn.chatgpt.com/docs/config-file/config-reference.md; https://learn.chatgpt.com/docs/sandboxing.md

### Self-hosted configuration server
- What: `[config_server] base_url` in the system `requirements.toml` makes the CLI and desktop app fetch defaults and requirements from an HTTPS server the organisation runs, optionally behind company OAuth.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/enterprise/configuration-server.md; https://learn.chatgpt.com/docs/config-file/config-reference.md (`config_server.oauth.*`)

### Access tokens, service accounts, workload identity
- What: Enterprise Codex access tokens for trusted automation (`CODEX_ACCESS_TOKEN`), service accounts, and OIDC or SPIFFE workload identity variables. `forced_login_method` and `forced_chatgpt_workspace_id` restrict sign-in.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/auth.md (Use Codex access tokens); https://learn.chatgpt.com/docs/config-file/environment-variables.md; https://learn.chatgpt.com/docs/config-file/config-reference.md
- Notes: The service-accounts and access-token pages were fetched but only skimmed.

### Compliance logs client identity
- What: App-server clients must set `clientInfo.name` for the OpenAI Compliance Logs Platform. Enterprise integrations are asked to register with OpenAI.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/app-server.md (Initialization)

### 19. Observability

### OpenTelemetry export
- What: Off by default.
  - Exporters: OTLP HTTP or gRPC for logs, traces and metrics, with TLS client certificates.
  - Events: conversation start, API requests, SSE and WebSocket events, prompts, tool decisions and tool results.
  - Redaction defaults: prompts are redacted by default; `log_agent_responses` and `log_guardian_assessments` are opt-in, and tool results are capped at 2,048 bytes.
  - Project config cannot set `otel`.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-approvals-security.md (Monitoring and telemetry); https://learn.chatgpt.com/docs/config-file/config-reference.md (`otel.*`); https://learn.chatgpt.com/docs/config-file/config-advanced.md (Observability and telemetry)
- Notes: (verified in review)

### Analytics (Statsig)
- What: The built-in metrics exporter is Statsig, which follows `analytics.enabled`. Setting it to false disables Statsig while explicit OTLP exporters keep working.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-reference.md (`analytics.enabled`, `otel.metrics_exporter`)

### Usage and status
- What:
  - `/status` shows the model, policies, writable roots and token use.
  - `/usage` shows account usage and can use a rate-limit reset.
  - `/debug-config` prints config layers and requirements, and `/warnings` lists retained warnings.
  - `codex doctor [--network-only]` runs diagnostics.
  - The app-server exposes rate limits and token usage.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/developer-commands.md; https://learn.chatgpt.com/docs/app-server.md (Auth endpoints); https://learn.chatgpt.com/docs/changelog (0.166.0)

### Logs and feedback
- What: `log_dir` enables a plaintext `codex-tui.log`. `/feedback` uploads logs, and `feedback.enabled` switches it off.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-configuration/agents-md.md (Verify your setup); https://learn.chatgpt.com/docs/config-file/config-reference.md

### 20. Configuration and customisation

### Config layers and precedence
- What: From highest to lowest: CLI flags and `-c`; project `.codex/config.toml` files from root to cwd (trusted projects only); the profile file `~/.codex/<name>.config.toml`; the user file `~/.codex/config.toml`; cloud-managed defaults; `/etc/codex/config.toml`; built-ins. `requirements.toml` constrains all of these.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-basic.md (Configuration precedence)

### Profiles as separate files
- What: `--profile name` overlays `~/.codex/name.config.toml` on the user config. Since 0.134.0 the `[profiles.x]` tables and the `profile =` selector are no longer read.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-advanced.md (Profiles)

### Project config key blocklist
- What: Project `.codex/config.toml` **cannot** set `openai_base_url`, `chatgpt_base_url`, `model_provider(s)`, `notify`, `profile(s)`, `otel` or the realtime base URL. Codex prints a warning when it ignores them.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/config-file/config-advanced.md (Project config files)
- Notes: (corrected in review: the blocklist on https://learn.chatgpt.com/docs/config-file/config-advanced.md also includes `apps_mcp_product_sku`; the rest was confirmed)

### Feature flags
- What: `[features]` flags carry maturity labels (under development, experimental, beta, stable, deprecated), and `/experimental` toggles them. `codex features` exists as a command. Feature maturity is defined on its own page.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/feature-maturity.md; https://learn.chatgpt.com/docs/config-file/config-basic.md (Feature flags)

### Config write API
- What: `config/read`, `config/value/write` and `config/batchWrite` (atomic edits) give programmatic access to the user's `config.toml`.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/app-server.md (API overview)

### 21. Security and privacy

### Credential storage
- What: `cli_auth_credentials_store = file|keyring|auto|ephemeral`. The `file` option writes plaintext `~/.codex/auth.json`, which the docs say to treat like a password. MCP OAuth has its own store setting. Device-code login is in beta.
- Status: GA (device code beta)
- Evidence: https://learn.chatgpt.com/docs/auth.md (Login caching; Credential storage; Login on headless devices)

### API keys in CI
- What: The docs warn against setting `OPENAI_API_KEY` or `CODEX_API_KEY` job-wide where repository code runs. They say to set it inline for one invocation, or use the Action's proxy.
- Status: GA (guidance)
- Evidence: https://learn.chatgpt.com/docs/non-interactive-mode.md (Authenticate in automation)

### Prompt-injection posture
- What: The defaults favour cached web search; the network proxy guards local and private destinations; browser content is treated as untrusted. `ExternalMessage` in the Python SDK gives third-party content lower priority. Auto-review targets exfiltration and credential probing.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/agent-approvals-security.md; https://learn.chatgpt.com/docs/codex-sdk.md; https://learn.chatgpt.com/docs/sandboxing/auto-review.md

### 22. Licence, pricing and distribution

### Distribution
- What: Standalone installers (`install.sh` and `install.ps1` from `chatgpt.com/codex`, falling back to GitHub Releases), npm `@openai/codex`, a Homebrew cask, and static musl Linux binaries. `codex update` self-updates.
- Status: GA
- Evidence: https://raw.githubusercontent.com/openai/codex/main/README.md; https://learn.chatgpt.com/docs/codex/cli.md; https://learn.chatgpt.com/docs/developer-commands.md (`codex update`)

### Pricing
- What: Codex is included in ChatGPT plans (Free, Go, Plus, Pro, Business, Edu, Enterprise) and shares usage and credits with ChatGPT Work. API-key use is billed at API rates.
- Status: GA
- Evidence: https://learn.chatgpt.com/docs/pricing.md; https://learn.chatgpt.com/docs/auth.md

### 23. Anything else

### Codex Security (separate product)
- What: A vulnerability scanning family with a CLI, a TypeScript SDK, a cloud service, a plugin and a desktop workbench. It is out of scope for this survey and was not read beyond the index.
- Status: research preview / beta (per index descriptions)
- Evidence: https://learn.chatgpt.com/docs/llms.txt

### Cyber access programs ("Daybreak")
- What: Eligible accounts can route requests to approved cyber models. `--cyber-access-program` works per turn. Cyber models force Auto-review behaviour: the turn interrupts on the first denial and `allow` prefix rules are ignored.
- Status: GA (opt-in)
- Evidence: https://learn.chatgpt.com/docs/changelog (0.161.0, 0.163.0); https://learn.chatgpt.com/docs/sandboxing/auto-review.md

---
- Notes: (verified in review)

## 3. Distinctive or unusual

- **A reviewer agent replaces the human approver, with a circuit breaker and a one-shot override.** Auto-review fails closed, stops the turn after 3 consecutive or 10-of-50 denials, and `/approve` grants exactly one retry that is itself re-reviewed. It is a real mechanism, not just a toggle.
- **Command rules carry inline unit tests.** `match`/`not_match` are validated at load. Compound `bash -lc` scripts are split with tree-sitter only when provably linear, and are otherwise judged as one opaque command.
- **Hooks are trusted by hash.** A changed hook, or a changed plugin script with the same definition, is skipped until re-reviewed. Admins can force managed-only hooks. This closes the "repo ships a hook" attack.
- **Placeholder substitution for cloud network secrets.** Processes see a placeholder; the egress proxy swaps in the real credential only for allowlisted HTTPS hosts, so the raw secret never lands in a process or file.
- **Project config cannot redirect credentials.** Provider, base URLs, `notify`, `otel` and profiles are ignored in repo-local config, with a warning.
- **Protected subpaths inside writable roots** (`.git`, `.agents`, `.codex`, `.aws`, including a resolved gitdir) stop an agent from rewriting its own config or Git hooks.
- **The docs are candid about the limits of their own security controls.** DNS-rebinding protection is "best effort"; deny-read globs are a snapshot per process; the proxy does not cover search, MCP or apps.
- **Every client uses one app-server protocol.** The TUI, IDE, desktop and SDKs share the JSON-RPC thread/turn/item API, with a background daemon, a remote TUI over WebSocket or UDS, and version-matched schema generation.
- **Goals with token and time budgets** are first-class persisted thread state with "automatic continuation".
- **Chat and Git state move between hosts** (Handoff): a chat plus its worktree can go from laptop to SSH host and back.
- **Asynchronous safety monitor** that can pause a task after the fact; this is separate from per-action review.
- **Claude Code compatibility shims.** `CLAUDE_PLUGIN_ROOT` is set for plugin hooks, and `/import` reads Claude Code and Cursor setups and chats.

---

## 4. Architecture and UX notes (public sources only)

- **Where the loop runs.**
  - Local surfaces (CLI, IDE, Desktop) run the agent loop and tools on the user's machine inside an OS sandbox.
  - Codex Cloud runs it in OpenAI-managed per-task workspaces.
  - Mobile runs nothing locally; it drives Cloud or a connected host.
  
  (https://learn.chatgpt.com/docs/sandboxing.md, https://learn.chatgpt.com/docs/remote-connections.md)
- **Client/server split.** The app-server is "the interface Codex uses to power rich clients (for example, the Codex VS Code extension)". The CLI can attach to a long-lived background app-server daemon or to a remote one through `--remote`. The desktop app starts a remote app-server over SSH for remote projects (https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/remote-connections.md, https://learn.chatgpt.com/docs/environments/network-file-systems.md).
- **Protocol.**
  - Wire: JSON-RPC 2.0 over stdio JSONL, WebSocket or UDS. One initialize per connection, with capability negotiation and an explicit `experimentalApi` gate.
  - Server-initiated requests: approvals, elicitations, permission requests.
  - Notifications: per-item deltas and turn lifecycle.
  - Schemas are generated per version (https://learn.chatgpt.com/docs/app-server.md).
- **Persistence.** Append-only `rollout-*.jsonl` per session, each written by a single process. A SQLite index (WAL) covers listing, goals and memory state. History can be reverted in the log; files are not reverted (https://learn.chatgpt.com/docs/environments/network-file-systems.md, https://learn.chatgpt.com/docs/app-server.md).
- **Configuration.** Layered TOML plus enforced `requirements.toml`. MCP config is shared by every client on one host (https://learn.chatgpt.com/docs/config-file/config-basic.md, https://learn.chatgpt.com/docs/extend/mcp.md).
- **Remote execution primitives.** Code Mode host over gRPC, remote executors with `environment_id`, and `exec-server --remote` all exist; none has a design page (https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/extend/mcp.md).

---

## 5. Relevance to a self-hosted master/node harness

- **App-server protocol as a reference for the master–node link and UI fold.**
  - What to copy: the thread/turn/item model, server-initiated approval requests carrying `availableDecisions`, the `experimentalApi` capability gate, and generated schemas.
  - What to beat: Codex labels the protocol experimental while depending on it.
  - (Section 2: App-server protocol.)
- **A negative example for "loopback by default".** Codex's non-loopback WebSocket listener accepts unauthenticated connections during rollout. Its capability-token or signed-bearer auth enforced before `initialize`, and Origin-header rejection, are worth copying. (App-server WebSocket auth.)
- **"The node decides" has a direct analogue.**
  - `requirements.toml` outranks all config. Omitted profiles are denied, including future built-ins.
  - Project config can't touch provider, credentials, `notify` or `otel`.
  - Subagents can't widen permissions.
  
  (Managed requirements; Project config key blocklist; Subagents inherit permissions.)
- **Permission profiles and protected paths.** `deny > write > read` with the most specific path winning, built-in parents, and `.git`/`.codex` read-only inside writable roots. Their honest note about deny-glob snapshots is a trap to avoid repeating silently. (Permission profiles; Deny-read globs; Protected paths.)
- **The network proxy's "does not control" list** is a good checklist for our own egress story: search, MCP, apps and model traffic each need separate controls. (Network proxy.)
- **Secret handling.**
  - Weak default: `shell_environment_policy.ignore_default_excludes = true` leaks `*TOKEN*` variables to commands by default. Rizzy's containment should be stricter.
  - Strong idea: cloud network-secret placeholder substitution matches our "no secret in an event" invariant.
  
  (Shell environment policy; Codex Cloud.)
- **Hooks trusted by hash, plus managed-only mode,** is the right shape for any node-side hook or plugin system. (Hooks.)
- **Auto-review** shows what an automated approver costs (extra model calls), how it fails (closed), and how a breaker and a one-shot override keep a human in charge. It could run on the node so that "the node decides". (Auto-review.)
- **Session storage:** a single-writer JSONL rollout plus a SQLite index, and their NFS corruption guidance, back our "one writer per session" design and warn against sharing a node data dir over NFS. (Rollout files and SQLite index.)
- **Handoff and remote app-server over SSH** show demand for moving a session between machines. With master/node this is a session moving between nodes with its Git state. (Handoff between hosts; SSH remote projects.)

---

## 6. Overhyped, stale or contradictory

- **`project_doc_max_bytes` is described two ways.** The AGENTS.md page says files stop being added once the *combined* size reaches it. The advanced config page calls it "how much to read from each AGENTS.md file" (https://learn.chatgpt.com/docs/agent-configuration/agents-md.md vs https://learn.chatgpt.com/docs/config-file/config-advanced.md).
- **`codex agents --no-daemon`.** The CLI reference says `codex agents` doesn't support `--no-daemon`. The 0.167.0 release notes (2026-11-06) say to open the command center with `codex agents --no-daemon` (https://learn.chatgpt.com/docs/developer-commands.md vs https://learn.chatgpt.com/docs/changelog). The reference may lag the release.
- **App-server maturity.** It is called "experimental and isn't supported for production workloads", while it is the only replacement for the removed MCP server and the stable Python SDK is built on it (https://learn.chatgpt.com/docs/mcp-server.md, https://learn.chatgpt.com/docs/codex-sdk.md).
- **GitLab review** is "preview" on the code-review page and "beta" on the GitLab page (https://learn.chatgpt.com/docs/code-review.md, https://learn.chatgpt.com/docs/third-party/gitlab.md).
- **Code Mode** is "under development and off by default" in the config reference while releases keep shipping user-facing Code Mode features (https://learn.chatgpt.com/docs/config-file/config-reference.md vs https://learn.chatgpt.com/docs/changelog).
- **The `untrusted` migration text is self-contradictory.** It says the value is retired and can stop clients starting. In the same section it says managed `allowed_approval_policies` "must include `untrusted`" to permit the project-derived policy (https://learn.chatgpt.com/docs/agent-approvals-security.md).
- **Broken in-page anchors.** The CLI and IDE pages link to `cloud#use-codex-cloud-from-the-cli` and `cloud#delegate-from-the-ide-extension`, but neither section exists in the Markdown of https://learn.chatgpt.com/docs/cloud.md. They may render only in HTML; not checked.
- **The Speed page still cites GPT-5.6** (a "1.5x" speed-up) beside GPT-6 models, which looks stale (https://learn.chatgpt.com/docs/agent-configuration/speed.md).
- **The docs index is titled "Codex"** but most pages are ChatGPT, Work, dots or Space, and many Codex pages are interleaved with "Local computer access with Work Cloud" text (https://learn.chatgpt.com/docs/llms.txt).
- **The README links to `developers.openai.com/codex`,** which now redirects to the unified docs (https://raw.githubusercontent.com/openai/codex/main/README.md).
- **Marketing-level claim:** the "secure relay layer" for mobile and remote hosts has no technical description (https://learn.chatgpt.com/docs/remote-connections.md).
- **Memories default.** "Off by default" (memories page) sits next to "Enabled memory v2 by default for fresh profiles" (0.165.0). The second refers to the store version, not enablement, but reads as a contradiction.

---

## 7. Unverified

- **(moved in review) WSL1 support dropped at 0.115, and workspace-write network off by default.** Looked for both on sandboxing.md, permissions.md, agent-approvals-security.md and the changelog text read (0.163 to 0.166 and part of the rest); neither statement was found. The changelog entry for 0.115 or the relevant older release notes would settle the first; a "Defaults" table in the CLI docs would settle the second.
- **(moved in review) Presets limit of 8,000 bytes, and `codex sandbox uninstall`.** Not on developer-commands.md (presets) or windows-sandbox.md (uninstall); may be in config-reference.md pages not read.
- **The full built-in tool list and each tool's parameters.** There is no tools reference page. A tools page, or the Code Mode docs, would settle it, and round 2 could read the source.
- **How compaction works in detail** (the summarisation prompt, what is retained). Settled only by source or the `compact_prompt` default.
- **Exact CLI flags.** The ConfigTable components (global flags, `exec`, `resume`, `fork`, `cloud`, `sandbox` options) do not render in the `.md` twins. The HTML page would settle it.
- **Remote executors and `codex exec-server`:** what they are and how they authenticate. Only fragments were found; a dedicated page or the app-server README in the repo would settle it.
- **Computer History, Appshots, Record & Replay, voice:** these pages were not read.
- **The IDE extension's full settings and command list** (developer-settings was skimmed only) and the JetBrains or Xcode integration specifics (those are third-party docs).
- **The relay's architecture and encryption** for mobile and remote control. Not documented.
- **Whether Codex Cloud environments document a per-task VM or container isolation model,** beyond "its own workspace" and "VM". The legacy universal image (`openai/codex-universal`) README was not opened.
- **The enterprise pages** for service accounts, access tokens, gateways and Bedrock were fetched but only skimmed, so no detailed claims are made from them.

---

## 8. Sources

All were opened on 2026-11-10. Docs pages were fetched as Markdown twins.

| URL | Title | Used for |
|---|---|---|
| https://learn.chatgpt.com/docs/llms.txt | Codex docs index | Page map, Codex Security scope |
| https://learn.chatgpt.com/docs/changelog | Changelog (HTML) | CLI releases 0.159–0.167, app releases, model notices |
| https://raw.githubusercontent.com/openai/codex/main/README.md | Codex CLI README | Install, distribution |
| https://api.github.com/repos/openai/codex/releases?per_page=8 | Releases API | Versions and dates, 0.167.0 notes |
| https://api.github.com/repos/openai/codex | Repo metadata | Licence, stars, last push |
| https://learn.chatgpt.com/docs/codex/cli.md | Codex CLI | CLI overview |
| https://learn.chatgpt.com/docs/open-source.md | Open Source | Open/closed split, no external PRs |
| https://learn.chatgpt.com/docs/sandboxing.md | Sandbox | Modes, bwrap, approvals_reviewer |
| https://learn.chatgpt.com/docs/permission-modes.md | Permissions (modes) | Desktop/IDE modes |
| https://learn.chatgpt.com/docs/agent-approvals-security.md | Agent approvals & security | Approvals, network, OS sandbox, OTel |
| https://learn.chatgpt.com/docs/permissions.md | Permissions (profiles, beta) | Permission profiles |
| https://learn.chatgpt.com/docs/sandboxing/auto-review.md | Auto-review | Reviewer agent |
| https://learn.chatgpt.com/docs/agent-configuration/rules.md | Rules | Starlark execpolicy |
| https://learn.chatgpt.com/docs/agent-configuration/subagents.md | Subagents | Multi-agent, custom agents |
| https://learn.chatgpt.com/docs/agent-configuration/agents-md.md | Custom instructions with AGENTS.md | Instruction discovery |
| https://learn.chatgpt.com/docs/customization/memories.md | Memories | Local memories |
| https://learn.chatgpt.com/docs/hooks.md | Hooks | Hook events, trust, handlers |
| https://learn.chatgpt.com/docs/extend/mcp.md | Model Context Protocol | MCP client config |
| https://learn.chatgpt.com/docs/mcp-server.md | Codex MCP server removal | MCP server removed |
| https://learn.chatgpt.com/docs/skills-and-plugins.md | Skills & Plugins | Concepts |
| https://learn.chatgpt.com/docs/build-skills.md | Build skills | Skill locations, budget |
| https://learn.chatgpt.com/docs/plugins.md | Plugins | Plugin surfaces |
| https://learn.chatgpt.com/docs/custom-prompts.md | Custom Prompts | Deprecated prompts |
| https://learn.chatgpt.com/docs/non-interactive-mode.md | Non-interactive mode | `codex exec` |
| https://learn.chatgpt.com/docs/codex-sdk.md | Codex SDK | TS and Python SDKs |
| https://learn.chatgpt.com/docs/github-action.md | Codex GitHub Action | CI action |
| https://learn.chatgpt.com/docs/app-server.md | Codex App Server | Protocol, API, auth |
| https://learn.chatgpt.com/docs/developer-commands.md | Developer commands | Subcommands, slash commands |
| https://learn.chatgpt.com/docs/config-file/config-basic.md | Config basics | Precedence, web search, env |
| https://learn.chatgpt.com/docs/config-file/config-advanced.md | Advanced Configuration | Profiles, providers, project config, history |
| https://learn.chatgpt.com/docs/config-file/config-reference.md | Configuration Reference | Keys and features |
| https://learn.chatgpt.com/docs/config-file/environment-variables.md | Environment variables | CODEX_API_KEY, exec-server |
| https://learn.chatgpt.com/docs/glossary.md | Glossary | Terms and surfaces |
| https://learn.chatgpt.com/docs/cloud.md | Codex Cloud | Cloud overview |
| https://learn.chatgpt.com/docs/environments/modes.md | Codex environments | Local/Worktree/Cloud |
| https://learn.chatgpt.com/docs/environments/cloud-environments.md | Cloud environments | Env setup, secrets, VPN |
| https://learn.chatgpt.com/docs/cloud/internet-access.md | Codex Cloud (Legacy): internet access | Legacy two-phase |
| https://learn.chatgpt.com/docs/environments/git-worktrees.md | Worktrees | Managed worktrees, handoff |
| https://learn.chatgpt.com/docs/environments/local-environment.md | Local environments | Setup scripts, Git tools |
| https://learn.chatgpt.com/docs/environments/network-file-systems.md | Run Codex with an NFS home | SQLite/rollouts, daemon |
| https://learn.chatgpt.com/docs/remote-connections.md | Remote connections | Mobile, SSH, handoff |
| https://learn.chatgpt.com/docs/mobile.md | Codex on mobile | Mobile |
| https://learn.chatgpt.com/docs/code-review.md | Code review | Local and PR review |
| https://learn.chatgpt.com/docs/third-party/github.md | Review GitHub pull requests | @codex review |
| https://learn.chatgpt.com/docs/third-party/gitlab.md | Review GitLab merge requests | GitLab beta |
| https://learn.chatgpt.com/docs/third-party/slack.md | Use ChatGPT in Slack | Slack delegation |
| https://learn.chatgpt.com/docs/third-party/linear.md | Use Codex in Linear | Linear |
| https://learn.chatgpt.com/docs/codex/ide.md | Codex IDE extension | IDE |
| https://learn.chatgpt.com/docs/developer-settings.md | Developer settings | Skimmed |
| https://learn.chatgpt.com/docs/app.md | ChatGPT desktop app | Desktop |
| https://learn.chatgpt.com/docs/linux/linux-app.md | Desktop app for Linux | Linux preview |
| https://learn.chatgpt.com/docs/windows/windows-sandbox.md | Windows sandbox | MXC/elevated/unelevated |
| https://learn.chatgpt.com/docs/auth.md | Authentication | Credential storage, device code |
| https://learn.chatgpt.com/docs/models.md | Models | Lineup, providers, retirements |
| https://learn.chatgpt.com/docs/agent-configuration/speed.md | Speed | Fast/Ultrafast |
| https://learn.chatgpt.com/docs/feature-maturity.md | Feature Maturity | Maturity labels |
| https://learn.chatgpt.com/docs/automations.md | Scheduled tasks | Scheduled tasks |
| https://learn.chatgpt.com/docs/computer-use.md | Computer Use | Computer use |
| https://learn.chatgpt.com/docs/browser.md | Browser | Built-in browser |
| https://learn.chatgpt.com/docs/features/codex-micro.md | Codex Micro | Hardware |
| https://learn.chatgpt.com/docs/import.md | Import from another agent | Import |
| https://learn.chatgpt.com/docs/long-running-work.md | Long-running work | Goals |
| https://learn.chatgpt.com/docs/integrated-terminal.md | Integrated terminal | Desktop terminal |
| https://learn.chatgpt.com/docs/pricing.md | Pricing | Plans |
| https://learn.chatgpt.com/docs/enterprise/managed-configuration.md | Managed configuration | Headings only |
| https://learn.chatgpt.com/docs/enterprise/configuration-server.md | Self-host Codex configuration | Config server |
| https://learn.chatgpt.com/docs/whats-new/devday-2026.md | DevDay 2026 | Cloud environments, Security Cloud |

Fetched but not used beyond a skim (no claims rest on them alone): https://learn.chatgpt.com/docs/reference/commands.md, https://learn.chatgpt.com/docs/windows/windows-app.md, https://learn.chatgpt.com/docs/reference/settings.md, https://learn.chatgpt.com/docs/build-plugins.md, https://learn.chatgpt.com/docs/cli-customization.md, https://learn.chatgpt.com/docs/config-file/config-sample.md, https://learn.chatgpt.com/docs/customization/overview.md, https://learn.chatgpt.com/docs/enterprise/access-tokens.md, https://learn.chatgpt.com/docs/enterprise/connect-to-a-gateway.md, https://learn.chatgpt.com/docs/enterprise/service-accounts.md, https://learn.chatgpt.com/docs/environments/cloud-environment.md, https://learn.chatgpt.com/docs/extend/record-and-replay.md, https://learn.chatgpt.com/docs/model-selection.md, https://learn.chatgpt.com/docs/notifications.md, https://learn.chatgpt.com/docs/windows/wsl.md, https://learn.chatgpt.com/docs/whats-new.md, https://learn.chatgpt.com/docs/enterprise/workspace-model-availability.md, https://learn.chatgpt.com/docs/enterprise/security-cookbook.md, https://learn.chatgpt.com/docs/enterprise/agent-security.md, https://learn.chatgpt.com/docs/amazon-bedrock.md, https://learn.chatgpt.com/docs/enterprise/gateway-compatibility.md, https://learn.chatgpt.com/docs/security-administration.md. The URL https://learn.chatgpt.com/docs/changelog.md returned 404; the HTML changelog was used instead.

## 9. Review

Reviewer: independent agent, 2026-11-10. Public pages only; every page was fetched with a question about the specific claim.

Entries checked: 47 of 138 (some entries were checked in groups). Verdict counts: confirmed 37, confirmed-with-correction 10, not-supported 0 whole entries (2 sub-claims moved to Unverified), wrong 0, status-wrong 1 (Hooks, folded into the corrections), unreachable 0.

| Entry | Verdict | Reason |
|---|---|---|
| Auto-review | confirmed | 3 consecutive or 10 of 50 denials, first denial for cyber models, fail-closed on prompt-build/session/parse failures, `/approve` one retry, policy keys all on auto-review.md and agent-approvals-security.md. The page states no maturity label. |
| Command rules (Starlark) | confirmed | tree-sitter split of linear `&&`, `||`, `;`, `|` chains; opaque for redirects, substitutions, variables, globs, control flow; forbidden > prompt > allow; experimental. |
| Hooks | confirmed-with-correction | Trust-by-hash, bypass flag, managed-only, handler types, 8 async, 2,500 tokens all confirmed; page gives no GA label. |
| App-server protocol | confirmed | Experimental and unsupported for production (app-server.md, mcp-server.md); transports, -32001 confirmed. |
| App-server WebSocket auth | confirmed | Quote found: non-loopback listeners allow unauthenticated connections by default during rollout; auth before `initialize`; Origin gives 403 (stated for health probes). |
| Sandbox modes | confirmed-with-correction | "network off by default" not found. |
| macOS, Windows enforcement | confirmed | Refusal-to-run wording found on permissions.md for macOS; MXC/elevated/unelevated confirmed. |
| Linux and WSL2 enforcement | confirmed-with-correction | WSL1 0.115 claim not found; Landlock rejection confirmed. |
| Permission profiles | confirmed-with-correction | Not a 0.138.0 requirement; beta label confirmed. |
| Deny-read globs, protected paths, network proxy | confirmed | Snapshot-at-start, four protected paths, proxy off by default and experimental (config-reference). |
| Approval policies, decision scopes, folder trust | confirmed | `untrusted` retired text, `acceptWithExecpolicyAmendment`, Auto vs read-only recommendation. |
| Subagents inherit permissions; built-in and custom agents | confirmed | Cannot override sandbox_mode, approval_policy, model_provider, mcp_servers; disableable features list. No default is documented for max_concurrent_threads_per_session. |
| Local memories | confirmed | Off by default, memories_v2, redaction, idle-only, rate-limit pause, disable_on_external_context. |
| Handoff between hosts | confirmed-with-correction | "to a worktree" not stated. |
| Shell environment policy | confirmed | `ignore_default_excludes` defaults to true (config-advanced and config-reference). |
| Goals, Code Mode, safety monitoring, MCP-server removal | confirmed | 4,000-char objective, code_mode "under development", CLI/mobile task simply ends. |
| TypeScript and Python SDK, Skills, skills budget | confirmed | Node 18+, Python 3.10+, 2% / 8,000 chars / 10,000 cap. |
| GitHub Action | confirmed-with-correction | Other safety strategies exist; proxy explained on non-interactive page. |
| GitHub PR review, scheduled tasks, Cloud network secrets | confirmed | P0/P1 only; unattended with default sandbox; placeholder substitution for HTTPS 443. |
| Import from other agents | confirmed-with-correction | 50-chat cap is CLI-only. |
| Custom providers | confirmed-with-correction | Responses-only is on models.md; 300 s is only an example. |
| Project config key blocklist | confirmed-with-correction | Also blocks `apps_mcp_product_sku`. |
| Model lineup | confirmed-with-correction | Default-model claim not confirmed. |
| `codex sandbox`, cyber access, OTel, queue, `/side`, `codex exec`, remote-control, git check | confirmed | Aliases, 2,048-byte default, `--approve-for-me`, read-only default exec found. |

**Borrowed features.** None found. The Claude Code compatibility items (`CLAUDE_PLUGIN_ROOT`, import) are documented as stated. Hooks, skills and AGENTS.md read similarly to Claude Code's but the details the survey records (hash trust, `prompt`/`agent` handlers skipped) are this vendor's.

**Notable omissions.** I did not do a full navigation sweep; these were visible in pages I read or in the survey's own skim list and deserve entries: (1) Record & Replay, https://learn.chatgpt.com/docs/extend/record-and-replay.md; (2) notifications, https://learn.chatgpt.com/docs/notifications.md; (3) Security cookbook and Agent Security, https://learn.chatgpt.com/docs/enterprise/security-cookbook.md and https://learn.chatgpt.com/docs/enterprise/agent-security.md; (4) Amazon Bedrock, https://learn.chatgpt.com/docs/amazon-bedrock.md; (5) gateway compatibility, https://learn.chatgpt.com/docs/enterprise/gateway-compatibility.md; (6) service accounts and access tokens, https://learn.chatgpt.com/docs/enterprise/service-accounts.md; (7) WSL guide, https://learn.chatgpt.com/docs/windows/wsl.md.

**Trust of the unchecked remainder.** High for the security-relevant material: 37 of 47 checked entries were exact, and every correction was a detail (a default, a version, a cap), not an invented feature. The weak spots are numeric defaults and version-by-version changelog claims (0.115, 0.161.0 and similar), which I could only partly reach because the changelog is very long; treat those as unverified. Status labels are sometimes the survey's inference where pages carry none.
