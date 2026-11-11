# DeepSeek Harness (`dsh`) - Round 1 public-surface survey

Surveyed 2026-11-10. Public surface only: docs site, README, SAFETY.md, CLI README, landing page, GitHub and npm metadata. No source code was read.

Docs base used in the Evidence lines below: `https://deepseek-harness.github.io/deepseek-harness/en/` (abbreviated here as DOCS; every Evidence line spells out the full URL).

## 1. Identity

- Name: DeepSeek Harness, command `dsh`. Vendor: DeepSeek AI.
- Official: confirmed. Repository `https://github.com/deepseek-ai/deepseek-harness` sits in the `deepseek-ai` organisation (GitHub API: owner type Organization, description "DeepSeek Harness: Everything is a Plugin."). The landing page `https://deepseek.com/harness` is on DeepSeek's own domain and links the same project. npm maintainers include a `@deepseek.com` address (npm registry, `@deepseek-ai/dsh`).
- Status: **developer preview**. The README warns of compatibility-breaking changes. The landing page labels it 预览版 (preview). `SAFETY.md` says it has had no security audit and must not be treated as secure or production-ready. The docs site nav and Python SDK page carry a "Preview" label.
- Surfaces: Web UI (`npx @deepseek-ai/dsh web`, default `http://127.0.0.1:3080`); Electron desktop app (macOS Apple silicon and Windows x64 per the landing page); headless one-shot; ACP over stdio; SDK mode (JSON-RPC over stdio) used by a Python SDK; GitHub PR-review webhook overlay. No TUI is documented.
- Open or closed: open source, MIT (GitHub API licence MIT; npm 0.2.2-rc.1 licence MIT; README says third-party licences are in `THIRD_PARTY_NOTICES.md`). Note: the very first npm version `0.0.1-rc.1` listed BSD-3-Clause; the current one is MIT.
- Language and stack: TypeScript on Node, built on the Cordis plugin framework ("everything is a plugin"). Web client is React.
- Repository: default branch `master`, created 2026-08-13, about 278k stars, last push 2026-11-04.
- Version: npm `latest` and `next` dist-tags both `0.2.2-rc.1` (published 2026-11-04); `alpha` is `0.2.2-alpha.3`.
- **Corrections to `00-targets.md`:** it says "No GitHub releases". That is wrong: the GitHub releases API lists releases, all marked prerelease, newest `dsh-v0.2.2-rc.1` (2026-11-04), then `dsh-v0.2.2-alpha.3` (2026-11-03), `dsh-v0.2.1-rc.5`, and so on. There is no stable (non-prerelease) release among those seen, and no CHANGELOG page in the docs. The npm version, flagged unverified in the targets file, is `0.2.2-rc.1`.

Evidence for the identity block: https://github.com/deepseek-ai/deepseek-harness (README); https://api.github.com/repos/deepseek-ai/deepseek-harness ; https://api.github.com/repos/deepseek-ai/deepseek-harness/releases ; https://registry.npmjs.org/@deepseek-ai/dsh ; https://deepseek.com/harness ; https://github.com/deepseek-ai/deepseek-harness/blob/master/SAFETY.md

## 2. Features by area

Status convention: "GA" is never used for this product, because everything is a preview. Where a source marks something experimental I say so; "preview" means documented and shipped but under the product-wide preview label; "unclear" means the page gives no maturity label.

### Area 1. Agent loop and execution model

### Turn and step loop
- What: A turn holds zero or more steps; a step is one model request plus the tools it calls. The loop assembles the prompt and tool schemas, streams the response, runs tools through pre/post hooks, and repeats while work remains. Turns end with a recorded reason (completed, aborted, blocked, error, max-tokens, interrupted, forked).
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/ (Architecture); https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session (Sessions)
- Notes: v0.2.2-rc.1 era docs.

### Event-sourced session log as the single source of truth
- What: The session is an append-only typed event log, and the model context is derived from it; anything the model sees must be logged so requests can be rebuilt. Durable session events are separate from live `agent/*` events (steps, streaming, steering). Streaming chunks are transient; a hard crash loses un-settled streams.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/agent-lifecycle (Agent lifecycle); https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session (Sessions)
- Notes: Close in spirit to the reader's own "UI is a fold over the log" invariant.

### Steering and interception of the loop
- What: A pre-step hook can intercept or alter a step; steering messages and injected context pass through the same waterfall and are claimed at the next step boundary. The pre-step decision is authoritative.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/agent-lifecycle (Interception and steering)
- Notes: Exact signatures are in a generated Cordis catalog that I did not open.

### Tool execution pipeline
- What: Ordered stages: `tools/pre-execute` waterfall (hooks and approval policy), approval resolution, monotonic guards (deny only, cannot re-allow), `tools/execute` (timeouts wrap here), `tools/post-execute`, content projection, lossless result snapshot, `finalizeContent`, then an observe-only `tools/result`.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-execution-pipeline (Tool execution); https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/tools (Tools)
- Notes: The page does not cover concurrency or output limits.

### Programmatic tool calling (PTC) mode
- What: A `run_code` tool runs a model-written TypeScript program that calls other tools as async functions; sub-calls go through the same pipeline and approvals and are logged as `tool/ptc-dispatch`. The runtime treats programs as hostile, caps logs and values, and reports failure kinds (exception, timeout, abort, output-limit, sandbox-unavailable...). A Python PTC runtime exists as experimental.
- Status: preview (Node runtime); experimental (Python runtime)
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/ptc-runtime (PTC runtime); https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-execution-pipeline ; https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams
- Notes: Enabled via the `ptc` preset (named on the schedule page).

### Presets of the agent
- What: Agent presets named `minimal`, `standard`, `cordis`, `ptc` (and a `code` preset mentioned on the web page) select which tools mount; for example schedule tools are absent in `minimal`. `agentPreset` can also select an independent composition for a child agent.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/schedule (Scheduled reminders); https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web (Caveats); https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent
- Notes: No single page lists what each preset contains (I could not find one).

### Area 2. Built-in tools

### Tool inventory (generated catalogue)
- What: Documented tools: `bash`, `pwsh` (plus persistent-shell variants), `read`, `write`, `edit`, `read_image`, `str_replace_editor` (alternative editor), `glob` (cap 100 results), `grep` (ripgrep, cap 250), `lsp`, `run_code`, `ask_user_question`, `exit_plan_mode`, `present` (declares final deliverable files, max 4 per call), `skill`, `todo_write`, `working_directory`, `load_workspace_dependencies`, `plugin_manager`, `plugin_market`, MCP resource tools (`list_mcp_resources`, `list_mcp_resource_templates`, `read_mcp_resource`), `web_search`, `web_fetch`, `schedule_*` (4), `create_goal`/`get_goal`/`update_goal`, `subagent`, `subagent_fork`, `list_subagent_models`, `list_agents`, `send_message`, `interrupt_agent`, `job_list`/`job_output`/`job_kill`, `ralph`, `workflow`, and opt-in groups below.
- Status: preview; opt-in groups flagged separately
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog (Tool schemas)
- Notes: Catalogue is generated and large; a tool existing in the catalogue does not mean it is mounted by default. (verified in review) (glob 100, grep 250, present max 4; catalogue is over 100,000 characters and the last 25,000 were not read in review)

### Opt-in and experimental tool groups
- What: `terminal_*` PTY tools (6, opt-in, experimental package); `stagehand_*` browser tools (6, experimental); session-history tools `session_search`, `session_trace`, `session_event_search/read/trace` (opt-in, experimental package); agent-team tools (experimental, disabled in the shipped base bundle); `create_worktree` (experimental, explicit composition only).
- Status: experimental
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog (Terminals, Browser, Session history, Agent teams)
- Notes: Docs say these are opt-in; I did not verify how a user opts in from the UI. (verified in review)

### Area 3. Code editing and file operations

### Read-before-edit freshness policy
- What: The filesystem policy tracks, per session, what the model has observed. An `edit` on a file never read fails (`FS_NOT_OBSERVED`); a version mismatch fails (`FS_STALE_VERSION`). Any windowed read authorises a later edit if the file is unchanged. `write` may create unseen files. The policy plugin is optional; without it writes and edits are unconditional.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/filesystem (Filesystem)
- Notes: Docs say a deployment loading `dsh-tool-fs` is expected to load the policy too.

### Edit and file tools
- What: `edit` replaces literal text (optional `replace_all`); `write` creates or overwrites; `read` returns numbered lines with offset and limit; `read_image` returns images to image-capable models; an alternative `str_replace_editor` offers view/create/replace/insert. Oversized `readBytes` fails with `FS_TOO_LARGE` instead of truncating.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/filesystem
- Notes: Changed-file cards and an HTML preview policy appear as web UI preferences (see area 15).

### LSP navigation
- What: An `lsp` tool offers four queries (go to definition, references, implementation, hover) through stdio language-server providers selected by file extension. Symbols and call hierarchy are excluded. Without a matching provider it errors `LSP_UNAVAILABLE`.
- Status: preview (optional capability)
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/lsp (LSP navigation)
- Notes: Supported language servers and default timeouts are not documented on that page.

### Area 4. Terminal and shell integration

### Bash and PowerShell executors
- What: `bash` runs a command in a fresh shell (`description`, `command`, optional `timeoutMs`, `workdir`, `run_in_background`); `pwsh` on Windows. Results report exit code, signal, timedOut and aborted separately. Output per stream keeps the tail and spills the full stream to a private file. Default kill on timeout. A persistent-shell variant exists as an alternate package; the SDK minimal profile uses it with a 300 s timeout.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/shell (Bash execution); https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog ; https://deepseek-harness.github.io/deepseek-harness/en/guide/python-sdk
- Notes: Page covers the local executor only.

### Shell environment hygiene
- What: `DSH_*` variables are harness-owned and rebuilt per model shell call; the local executor scrubs ambient credentials before merging env. The MCP stdio bridge strips credential-style and all `DSH_*` env vars before launching children.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/shell (Environment handling); https://deepseek-harness.github.io/deepseek-harness/en/guide/mcp-memory (How MCP servers are configured)
- Notes: The proxy page notes that proxy variables, including a password in a proxy URL, are inherited by every child command.

### PTY terminal sessions (opt-in)
- What: Persistent terminals the agent can open, send to, read, signal and close. One active send per session; bounded retained scrollback; ownership bound to the owning agent; signals limited to allowed POSIX names and sent to the foreground process group. PTY state is process-local; only model input and bounded output are durable.
- Status: unclear (the page gives no maturity label; the tool catalogue calls the package experimental and opt-in)
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/terminal (PTY sessions); https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog
- Notes: Sources disagree on maturity labelling by omission.

### Area 5. Permissions, approvals and sandboxing

### Process sandbox with three modes
- What: Filesystem-only confinement. Modes: `read-only`, `workspace-write` (workspace root plus backend temp area), `danger-full-access` (bypass; never reaches the sandbox). Backends: bwrap and Landlock on Linux, Seatbelt on macOS, an ACL/restricted-token backend on Windows; SSH remote uses a separate remote backend with the same policy. Network and process visibility are explicitly outside the sandbox vocabulary. If no usable backend exists, confinement fails with `SANDBOX_UNAVAILABLE`; silent unconfined passthrough is not allowed for a confined policy. Backends report `full` or `partial` enforcement (older Landlock ABIs, Windows hard-link and unconfined-read gaps are listed as partial).
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/sandbox (Sandboxing)
- Notes: The default mode value is not stated on the page. Mode precedence: explicitly approved mode, then the session's last `sandbox/mode` event, then deployment default. (verified in review) (default mode value indeed not stated; SSH backend confirmed)

### One-shot widening of the sandbox
- What: The model may request a strictly wider, one-shot retry via `sandbox_permissions` and `justification`; `ctx.approval` must grant that exact call before it runs.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/shell (Sandbox integration); https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog (`run_code`)
- Notes: Similar idea to escalation requests elsewhere; here it is tied to a logged approval event.

### Approval service
- What: Each request appends an `approval/asked` audit event, checks the session policy, then a waterfall of answerers (UI, auto-review, etc.) decides. Outcomes: allowed-once, rejected, cancelled, unavailable; anything but allowed-once means deny (fail closed). Policy is `ask` (default) or `never` (reject everything, enforced before dispatch so a later answerer cannot bypass it). The effective policy is the last `approval/policy` event. Requests carry agent, tool name and reason but deliberately not tool arguments. A failed audit append rejects the request rather than returning an unlogged decision.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval (Approvals)
- Notes: No "allow always" or rule-list outcome is documented; only allowed-once. Auto review and rules are not described on this page. (verified in review)

### Permission presets
- What: A named bundle of sandbox mode plus approval policy, shown as one Permissions selector. Defaults: `workspace-write` (sandbox workspace-write, approval ask) and `danger-full-access` (full access, never ask). `custom` is derived when no preset matches; `auto` is registered at runtime by the Auto review integration (full access plus ask). The GitHub review overlay uses a `read-only` preset. Presets are recorded in a log-only `permission/preset` event.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/permission-presets (Permission presets); https://deepseek-harness.github.io/deepseek-harness/en/guide/github-review
- Notes: Note that `danger-full-access` pairs with approval `never`, i.e. no prompts, by default table. The Python SDK pins `danger-full-access`. (verified in review) (preset pairings, log-only `permission/preset` event, `auto` and `custom` verified; the `sandbox/mode` and `approval/policy` events are named on the sandbox and approval pages)

### Auto review (automatic authorization review)
- What: A preset `auto` is published by an "Auto review" integration that answers approval requests instead of the user. The landing page lists "automatic authorization review" among experimental features. The mechanism of the reviewer is not described in the pages I opened.
- Status: experimental
- Evidence: https://deepseek.com/harness (Experimental features); https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/permission-presets (Auto Preset)
- Notes: No dedicated Auto review page found; details unverified. (verified in review) (landing page lists it as experimental; preset page says `auto` is reserved for that integration)

### Safety statement
- What: SAFETY.md states that sandboxing, approvals and permission controls reduce risk but do not guarantee isolation, and should not be the only control for untrusted workloads; it recommends disposable VMs or containers, minimum privileges and backups.
- Status: preview
- Evidence: https://github.com/deepseek-ai/deepseek-harness/blob/master/SAFETY.md
- Notes: No security audit.

### web_fetch is unconfirmed by default
- What: The Cordis, Code and Standard presets allow `web_fetch` in every sandbox and approval mode without per-call confirmation; the file sandbox does not restrict network. SSRF checks block private addresses but do not stop exfiltration to a public URL (the docs say this themselves).
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web (Caveats)
- Notes: Honest caveat worth copying as a docs practice. (verified in review)

### Area 6. Context management and compaction

### Compaction (`dsh-compaction-basic`)
- What: Triggers: pressure before a step, provider-confirmed context overflow (recovery retries the request only if the surface advanced), manual `compactNow`, and explicit range. Optional tool-result pruning runs first, then range selection (preserves tool call/result pairs, can compact early steps of one oversized turn), then a locked event sequence start, summary, replacement user message, end. One compaction per session at a time; a `start` with no `end` is an orphaned lock from a crash. Compaction events are log-only; the replacement message changes the model-visible surface.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/compaction (Compaction)
- Notes: Numeric thresholds are not documented on that page. Compaction costs an extra model request (Python SDK page).

### Token metering
- What: `measure()` returns a snapshot with a baseline (`usage` from the last provider call, or `estimated`), per-node token prices and a delta. It counts tokens only; no currency cost or UI display is documented on that page.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/token-meter (Token metering)
- Notes: Cost display unverified.

### Spill of large tool results
- What: A spill policy replaces text or image results over `maxInlineTokens` with head and tail plus a locator; the model is told to `read` or `grep` the file. Local spill files live under a per-session hashed directory with 0700/0600 permissions and exclusive creation to defeat planted symlinks. On save failure the original inline result is kept.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/spill (Spill storage)
- Notes: No retention policy defined by the seam; inherited locators are not copied on fork.

### Area 7. Memory and persistent instructions

### Memory: none built in
- What: DSH has no built-in memory. Memory comes from a third-party MCP memory server that the user connects; three default-off example configs are given (Memorix 1.3.0, the MCP reference memory server 2026.7.4, Engram 1.20.0).
- Status: preview (as an integration pattern); built-in memory: nothing documented
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/guide/mcp-memory (Memory MCP)
- Notes: Confirmed finding: the docs explicitly state there is none.

### Instruction files (AGENTS.md and similar)
- What: The repository README links an `AGENTS.md`, but that is instructions for developers of the harness itself. The system-prompt page describes sections, runtime contexts and persona slots but does not mention loading AGENTS.md or similar project instruction files for the user's workspace.
- Status: unclear
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/system-prompt (System prompts); https://github.com/deepseek-ai/deepseek-harness (README)
- Notes: Nothing documented in the pages I opened; absence is a finding but not proof of absence in code.

### System prompt assembly
- What: Ordered, named prompt sections (plus persona prefix/suffix slots and scoped overrides) render into the first system message; dynamic runtime contexts are logged as a user-role message when changed. A single `complete` section can take over the whole prompt. Variables use `[a-z][a-z0-9_]*`.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/system-prompt
- Notes: Mostly relevant to plugin authors.

### Area 8. Planning, tasks and modes

### Plan mode
- What: Optional per-agent state logged as `plan/mode`; while active, a configured guidance section is added to each request (soft guidance only, sandbox and approval do not read plan state). `exit_plan_mode` stays registered always (the tool list never changes) and requires a markdown plan starting with a heading, presented for user review; approval, requested changes, or dismissal are distinct outcomes. `/plan`, `/plan <message>`, `/plan off` commands.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/plan (Plan mode)
- Notes: Keeping the tool list stable across modes helps prompt-cache reuse; the docs only say the list never changes.

### Todo list
- What: `todo_write` replaces the whole checklist (content and status pending/in_progress/completed). The quickstart says the agent maintains a plan.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog (Todo) ; https://deepseek-harness.github.io/deepseek-harness/en/guide/quickstart
- Notes: Dedicated page for todo behaviour not found.

### Goals (multi-round continuation)
- What: An event-sourced goal with objective, phase (active, paused, blocked, complete), revision for compare-and-set edits, and `maxGoalRounds`. Only admitted continuation user messages advance the round counter. `resume` is human-authorised; blocked carries a machine-routable code plus message. Activation (whether another round may start) is process-local and never persisted.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/goal (Goals); https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog
- Notes: Cap default comes from service config; value not given.

### `ralph` loop
- What: Runs a fresh-agent loop toward one objective with `maxRounds`.
- Status: experimental
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/workflow
- Notes: Only a one-line description is available. (verified in review)

### Structured user questions
- What: `ask_user_question` takes batches of questions with options, multi-select and free-text "Other"; a `plan-review` intent changes presentation only. Questions may be timed: after the bounded wait, the tool returns `pending` and a late answer arrives later as a new user message (source `user-question-reply`), including after a host restart. Child agents cannot ask the user.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/user-questions (User interaction)
- Notes: Distinctive: question survives past the tool call.

### Human commands
- What: A registry of plugin-defined slash commands run directly (no model message), logged as `command/run` and `command/done`. The page lists no specific commands; `/plan` and `/feedback` are mentioned elsewhere.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/commands (Human commands); https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/plan ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session-telemetry
- Notes: Full command list: nothing documented.

### Area 9. Sub-agents and multi-agent orchestration

### Subagents (local and external)
- What: A `subagent` tool delegates a self-contained task to a child agent with a durable child session, optional `cwd`, and optional model selection (off by default). Result has output, optional structured value, diagnostic (max 4096 bytes, must exclude tool inputs, file contents, credentials) and a stop reason. Running children take steering input at the next step boundary; idle ones start a turn; absent ones cold-resume. Delivery to the parent or caller. `maxDepth` caps delegation depth. External backends: Codex, Claude Code, ACP and the DSH SDK, each a single execution handle (follow-ups rejected).
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent (Subagents); https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams ; https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/reference/README.md
- Notes: Permission inheritance is not described on the page. Subagent bundles for Codex and Claude Code are separate npm packages. (verified in review) (external Codex/Claude Code/ACP/DSH SDK handles, follow-ups rejected, 4096-byte diagnostics, maxDepth; 'model selection off by default' is from the tool catalogue, not the subagent page; permission inheritance indeed undocumented)

### Workflows (model-written orchestration scripts)
- What: The `workflow` tool lets the model submit a JavaScript script with JSON `meta` that starts subagents via `agent`, `pipeline`, `parallel`, `phase`, `log`. Runs in a Node PTC process under the calling session's file policy; `maxTotalAgents` is a per-run cap that the script cannot change. Events `workflow/*`; the UI folds them into one `workflow-run` node. No overall time deadline.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/workflow (Workflows); https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog
- Notes: Page gives no maturity label. (verified in review) (maxTotalAgents per run, script cannot see it, no elapsed deadline, six workflow events)

### Agent teams
- What: A Lead plus named, durable teammates with direct peer inbox delivery and a shared task DAG with compare-and-set updates (claim, release, reassign, ...) and optional `write_scopes`.
- Status: experimental (disabled in the shipped base bundle; the landing page also lists agent teams as experimental)
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog (Agent teams) ; https://deepseek.com/harness ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/
- Notes: The dedicated `agent-team` page returned 404 at the URL I tried. (corrected in review: the `agent-team` page is not on the docs site; it is at https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/agent-team.md and says new sessions default to off, task DAG with `blockedBy`, `write_scopes` are advisory path prefixes, not locks; that page itself carries no experimental label, the label comes from the tool catalogue and landing page)

### Area 10. Background, long-running and scheduled work

### Background jobs
- What: A job registry with statuses running/stopping/completed/killed/failed, a bounded output ring per job (live 262144 bytes, settled 16384 by default), at most 10 concurrent jobs per owner session. The model reads output with a consuming cursor; the UI can follow and kill jobs. Records `job/registered` and `job/settled` go to the owner session log. The registry is process-local: the docs do not describe restoring live jobs after a restart, and a settled record can be missing after a crash.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/jobs (Background jobs)
- Notes: Job IDs are described as predictable; authorisation by owner session is the boundary. (verified in review) (262144 / 16384 / 10 per owner and predictable ids verified)

### Session reminders (scheduled tasks)
- What: The model creates reminders that reopen the original session and queue a follow-up message. Timing: relative seconds, absolute time, fixed interval (at least 60 s), daily, weekly, five-field cron with IANA zone. Persisted in a `schedule` storage domain; on restart an overdue one-shot or only the latest missed recurrence is delivered. Delivery is at-least-once, not exactly-once; a delivery receipt means "saved to inbox", not "work done". Subagents cannot schedule.
- Status: preview (the landing page lists scheduled tasks as experimental)
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/guide/schedule (Session reminders); https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/schedule (Scheduled reminders); https://deepseek.com/harness
- Notes: **The host process must be running**; closing it stops scheduling (guide page). Pause and a fresh session per run are not supported. Maturity label differs between pages (guide shows no label; landing page says experimental). (corrected in review: the guide page says exactly-once is not guaranteed and a crash can repeat a delivery; it does not use the words 'at-least-once'; host-must-be-running and 60 s floor verified; the guide page shows no maturity label)

### Area 11. Sessions: persistence, resume, fork, checkpoints, undo

### Persistence and crash recovery
- What: Per-session append-only JSONL log, by default as checksummed concatenated Zstandard frames, with atomic materialisation, per-batch fsync and torn-tail truncation; metadata stored separately. Resume reads the log with a write handle and appends closers (missing tool errors, open step end, synthetic interrupted turn end). One live writer per session (`SessionAlreadyOwnedError`). Historical v0/v1/v2 formats are translated on read; newer formats are refused.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/persistence (Session persistence)
- Notes: Storage root not described on that page; the Python SDK page says `<dsh_home>/sessions` uncompressed for the minimal profile. (verified in review)

### Fork
- What: `fork` creates a child session from an inclusive prefix of the source; a cut inside an open turn is rejected; child records parent and inherited count; a `session/end-seed` marker separates inherited from own events.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session (Branch and fork)
- Notes: No user-facing "fork from here" UI documented in pages opened.

### Session search and history tools
- What: A SQLite full-text index over messages, tool calls and results, todos and status details (reasoning blocks and stream chunks excluded) backs cross-session and in-session search with cursor paging; model-facing history tools are opt-in.
- Status: experimental (model tools), unclear (index)
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session-query ; https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog (Session history)
- Notes: A UI search box is named in the Web client page (sessions "list, search, create...").

### Undo, checkpoints, rewind
- What: nothing documented in the persistence, session or filesystem pages.
- Status: unclear
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/persistence ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/filesystem
- Notes: Each page states it does not cover this.

### Session archive, pin, titles
- What: Workspaces sidebar supports ordering, pinning and an archive filter (archived sessions greyed out and must be restored to open). Titles generated by an LLM from the first prompt (a variant from all prompts is experimental).
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-client (Views and features); https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams (`ctx.sessionTitle`)
- Notes: Titles page not opened.

### Area 12. Git and version control

### Worktree creation
- What: `create_worktree` creates a new git branch and worktree from a local commit, branch or tag and switches the session into it. Requires explicit composition and runs under existing permissions.
- Status: experimental
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/
- Notes: No built-in commit, PR or diff review feature documented; the agent uses `bash` for git. Changed-file cards exist in the web UI.

### GitHub PR review sessions
- What: An opt-in overlay adds a signed GitHub webhook endpoint (`POST /github` on a second listener, default `127.0.0.1:3081`). When a PR moves from draft to ready, it creates a titled root session in a local checkout workspace and starts a read-only review prompt using the `standard` preset and `read-only` permission preset. PR fields are passed labelled as untrusted metadata and the prompt forbids mutations. The webhook secret authenticates inbound data only; no outbound GitHub access is granted.
- Status: preview (example overlay; accepted repository in the example is `deepseek-ai/deepseek-harness`)
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/guide/github-review (GitHub review sessions)
- Notes: 202 means accepted, not matched. Deliveries are not stored, so duplicates create more sessions; a crash loses un-admitted calls. (verified in review) (port 3081, ready_for_review, read-only preset, no deduplication verified; webhook runtime page confirms no queue, retry or dedup)

### Area 13. Extensibility: MCP, plugins, skills, hooks, commands, SDKs, protocols

### Plugin architecture (Cordis)
- What: Everything (model adapter, tool registry, session log, loop) is a plugin on the Cordis framework; registrations are reversible effects unwound on unload. Plugins can add tools, skills and UI (client slots). Profiles are named compositions with bundles and YAML patches layered in fixed order; `--dump-config` shows the resolved tree; optional HMR recomposes on file change. Plugin development guides and a cookbook exist (new package, tool, LLM adapter, settings card).
- Status: preview (announced breaking changes)
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/ ; https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md ; https://deepseek-harness.github.io/deepseek-harness/llms.txt
- Notes: The landing page says core plugins and base APIs keep changing.

### Plugin markets and `plugin_manager`
- What: Third-party market indexes list plugins; install requires a confirmation dialog showing the source (npm registry, or git repo, commit, directory). Markets are untrusted: DSH validates the file format, not the claims; `http:` indexes can be tampered with. Install runs compatibility and registry checks; incompatible plugins need an exact-version exemption. The `plugin_manager` tool (list, enable, install, remove) requires danger-full-access or approval. A "creation mode" chat can write a plugin (landing page).
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/guide/plugin-market ; https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog (Plugins and market) ; https://deepseek.com/harness ; https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md
- Notes: Plugins run in-process with the host (the stack is Node plugins; no plugin sandbox is documented, and the page says nothing about signing). (verified in review) (market page: confirmation shows source, `http:` index tamperable; `plugin_manager` approval sentence is on the tool catalogue; signing and in-process claims are the survey's inference, the pages are silent)

### MCP client
- What: An MCP client plugin (`@deepseek-ai/dsh-mcp-client`) with stdio and streamable-http transports; tools appear as `mcp__<serverName>__<tool>`; resource tools are separate. Crashed stdio children reconnect with backoff, then tools unregister until reload. Configured as YAML entries inserted via a patch. A dedicated `mcp` subsystem page is mentioned in an index but returned 404.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/guide/mcp-memory ; https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog (MCP resources)
- Notes: OAuth and approval of MCP tools: nothing documented in the pages opened. (corrected in review: the `mcp` page is at https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/mcp.md, not on the docs site. It confirms stdio and streamable HTTP, server name in tool names, opt-in servers; it does not document OAuth, an approval flow, or the exact `mcp__<server>__<tool>` format, and says a failed refresh retains the previous tool generation. The CLI reference says no MCP server is enabled by default because server commands are trusted code outside the sandbox.)

### Skills
- What: Skill directories `<name>/SKILL.md` or flat `<name>.md`, kebab-case names, frontmatter flags `disable-model-invocation` and `user-invocable`. Roots by rank: `<project>/.dsh/skills`, `<project>/.agents/skills`, custom dirs, `<dshHome>/skills`, `<agentsHome>/skills`, bundled. Project root is the nearest `.git` ancestor. Names and descriptions (no bodies) are injected as a system reminder in the first step; `skill({name})` loads the body. A file watcher detects changes. No recursive discovery.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/skills (Skills)
- Notes: Reads the `.agents/skills` convention, compatible with other harnesses' layout.

### Hooks
- What: No user-facing hooks system documented. Plugin authors get pipeline waterfalls (`tools/pre-execute`, `tools/post-execute` and others). A "Claude Code mods via an experimental bridge" and "hook-bridge records" are mentioned in passing.
- Status: unclear
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/tools ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/ ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session
- Notes: Unverified whether shell-command hooks exist.

### Python SDK
- What: `pip install deepseek-harness-sdk` (Python 3.10+) bundles a native runtime wheel and `dsh`; `DeepSeekHarness` lazily starts `dsh --profile sdk-minimal` and reuses it; `harness.run(prompt, session_id=...)` returns `final_response`. The minimal profile has a persistent shell, `read_image`, `working_directory`, auto compaction and no file, web, subagent, settings or telemetry tools. It pins `danger-full-access`. The DeepSeek session-log contributor uploads unaccepted log events by default (up to 8 MiB per request) unless disabled in a patch.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/guide/python-sdk
- Notes: Linux x64/arm64, macOS 14+ arm64, Windows x64. (verified in review)

### Launch profiles: headless, SDK, ACP
- What: `dsh headless "job"` runs one persisted session, prints the final answer and exits; `dsh sdk` / `sdk-minimal` serve JSON-RPC over stdio; `dsh acp` serves ACP clients over stdio. All are profiles of one launcher, which the README calls the sole supported Node launcher. TypeScript SDK use is mentioned (architecture page says both SDKs use the same runtime).
- Status: preview
- Evidence: https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md ; https://deepseek-harness.github.io/deepseek-harness/en/reference/
- Notes: A TypeScript SDK page was not found in the docs index.

### Area 14. Models, providers and inference configuration

### Provider and model configuration
- What: Settings -> Models takes a DeepSeek API key; other shipped providers (`anthropic`, `openai`, `moonshotai`, `zai`, ...) via a catalogue; a custom model API supports OpenAI Chat Completions, OpenAI Responses and Anthropic Messages protocols with a base URL and manual model list or `GET /models` discovery. Picking a model sets the default for new sessions. Reasoning effort menu appears only where the model declares levels (`reasoningEfforts`); compat switches such as `supportsDeveloperRole`, `maxTokensField`, `thinkingFormat: deepseek`. Advanced settings live in `$DSH_HOME/profiles/<profile>/cordis.patch.yml` and apply on the next request. Vision via an Image input type.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/guide/providers
- Notes: Provider-managed credential files (AWS profiles, Google ADC) are not detected.

### Credentials handling
- What: Keys are write-only in the UI (redacted descriptor), stored in `$DSH_HOME/.credentials.yaml`; settings hold only a reference (an env-var name). Resolution order: saved credential first, then environment fallbacks. Resolved per model request so rotation needs no restart. No remote method returns a secret. Encryption at rest: nothing documented.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/guide/providers (Key storage); https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/credentials
- Notes: Plain-YAML storage is implied by the file name; I could not confirm permissions or encryption. (corrected in review: `$DSH_HOME/.credentials.yaml` and 'keys are write-only' are on the providers guide, not the credentials subsystem page, which gives no file location, permissions or encryption; 'no method returns a value' verified there)

### Network proxy
- What: Honours `HTTPS_PROXY`/`HTTP_PROXY` read at launch (from shell or `$DSH_HOME/.env`) for model calls, web tools and HTTP MCP; a project `.env` cannot set proxy variables (DSH refuses to start). No SOCKS support, no OS proxy settings, CIDR `NO_PROXY` ignored. Workflow and PTC workers get no proxy settings.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/guide/network-proxy
- Notes: Child processes (bash, git, MCP) inherit proxy variables including any password in the URL.

### Area 15. UI surfaces

### Web UI
- What: Browser app served by `dsh web` on the loopback by default. Documented views: workspace chooser and sidebar (ordering, pin, archive filter), session list with search, create, prompt, queue, cancel, pagination; two conversation views from the same event stream (`ui-chat` and `ui-trajectory`); background-job list with output panels; settings (Models, General, a Coding Tools preference covering diagnostic views, agent presets, changed-file cards, HTML preview policy); a right sidebar; an Automation tasks page; a trace view showing timelines and tool-call inputs/outputs (landing page). The composer is disabled until a workspace is selected. Plugins add UI through typed slots. After a disconnect, streams reopen and gaps are filled by paging.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-client (Web Client architecture); https://deepseek-harness.github.io/deepseek-harness/en/guide/quickstart ; https://deepseek.com/harness ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/slots
- Notes: There is no screenshot-level description in the docs I opened beyond the landing page; the contents of the right sidebar are not documented. (corrected in review: the right sidebar is documented at https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/sidebar-right : per-session docking surface with guide, text preview, file tree, browser and subagent-chat tabs; layout kept in memory only. The 'contents not documented' remark above is wrong.)

### Desktop app
- What: Electron app that bundles its own dsh runtime and manages its own profile; it does not use the HTTP server (loads built files over `file://`, requests go over an IPC bridge). Direct downloads: macOS Apple silicon `.dmg`, Windows x64 `.exe`; no Linux or Intel Mac build listed.
- Status: preview
- Evidence: https://deepseek.com/harness ; https://deepseek-harness.github.io/deepseek-harness/en/reference/ ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-server
- Notes: The `desktop` profile name is reserved in the CLI. (verified in review) (macOS Apple silicon and Windows x64 per landing page)

### Voice input and translation
- What: Experimental speech-to-text providers (including a SenseVoice one) and anonymous text translation are listed. Voice input is on the landing page's experimental list.
- Status: experimental
- Evidence: https://deepseek.com/harness ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/ ; https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams
- Notes: No mechanism detail read.

### Area 16. Browser automation and computer use

### Browser use and computer use
- What: Experimental browser seam with providers for Playwright MCP, Chrome DevTools MCP and a native Stagehand provider (tools `stagehand_navigate/act/observe/extract/screenshot/tabs`); an experimental computer-use seam with `cua-driver` providers.
- Status: experimental
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams ; https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog (Browser)
- Notes: Sandboxing of browser tools not documented in pages opened.

### Web search and fetch
- What: `web_search` (1-4 queries, `maxResults` default 8) via Exa, Perplexity or DeepSeek; `web_fetch` via a local HTTP provider. Fetch SSRF defence: only http/https, no embedded credentials, hostname resolved once, any non-public address rejected (including NAT64), connection pinned, same-origin redirects re-checked, cross-origin redirect needs a new tool call. Caps on redirects, bytes, characters and time.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web (Web access)
- Notes: Ambiguous provider choice fails with `WEB_PROVIDER_AMBIGUOUS` rather than picking one. (verified in review) (maxResults default 8 is the consumer's config default)

### Area 17. Remote, cloud, headless, CI and distributed execution

### Server and execution side: one process
- What: Per the docs, `dsh web` is a single Node process that serves the HTTP/WebSocket API and also hosts the agent loop and tools; the HTTP server is described as a carrier "not part of the agent loop". The browser is a thin client talking Typert RPC over `/api` plus a multiplexed WebSocket (`/api/remote.mux`). Both agent loop and tools run in the server process (on the machine that launched `dsh`); the process cwd is the default filesystem location. There is no documented separate worker or node daemon.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-server (HTTP server); https://deepseek-harness.github.io/deepseek-harness/en/reference/api-gateway ; https://deepseek-harness.github.io/deepseek-harness/en/guide/quickstart
- Notes: I did not find a statement that spells out "agent runs in the same process as the web server" in one sentence; it is inferred from the profile model (the `web` profile composes loop and server plugins) and the fact that the server has no agent concepts. (corrected in review: neither the web-server page nor the reference index states that the agent loop and HTTP server share a process; the web-server page says only that the HTTP server is 'not part of the agent loop'. The 'Per the docs' wording above is therefore an inference, as the Notes already say.)

### SSH remote execution backend
- What: A core `ctx.ssh` service owns an OpenSSH connection and the packages `fs-ssh`, `subprocess-ssh` and `sandbox-ssh` give a POSIX remote filesystem, subprocess and sandbox; the sandbox page says remote execution over SSH uses the same policy. So the loop runs locally and the tools can act on a remote host.
- Status: unclear (package names and a sandbox-page sentence only)
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams (Execution environments) ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/sandbox ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/
- Notes: The page `.../subsystems/ssh` returned 404, so host-key handling, auth and UX are unverified. This is the inverse of the reader's design (loop on the node). (corrected in review: the page exists at https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/ssh.md, not on the docs site. It states the harness, model transport and session storage stay on the host; auth and host keys come from existing OpenSSH config; helper checked against a configured SHA-256 hash; SSH loss is final, no reconnect; no maturity label. Status is now documented.)

### Public deployment behind a reverse proxy
- What: `dsh --profile web` listens on loopback; the documented path to remote access is a reverse proxy providing the public hostname, TLS and a path prefix. Flags: `--public-url` (display only), `--trusted-host` (repeatable, admits a Host/authority), `--tls-cert`/`--tls-key` (full chain plus unencrypted PEM key; never falls back to HTTP; read once, not reloaded), `--host` (must be one concrete IP; `0.0.0.0` is rejected; warns when a non-loopback bind uses plain HTTP), `--port` (default 3080). Proxy must forward Host unchanged, strip the mount prefix, pass WebSocket upgrades, rewrite cookie Path/Secure.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments ; https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/reference/README.md ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-server
- Notes: The page warns that neither `--public-url` nor the fence protects the listening port and that an `http://` public root exposes the launch token in plaintext. (verified in review) (`--host` wildcard rejected is on the CLI reference page, TLS-never-falls-back and port 3080 on the deployment page)

### Headless one-shot and ACP
- What: See area 13 (`headless`, `acp`, `sdk` profiles). No CI action or cloud-hosted offering documented.
- Status: preview
- Evidence: https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md
- Notes: Cloud/hosted agents: nothing documented.

### Area 18. Collaboration, teams and enterprise administration

### Multi-user support
- What: Nothing documented for multi-user operation. The web-server page does not address it; scopes concern registration visibility, not users; the workspace registry's archive and pin sets are described as registry-global; the credential store is one file in `$DSH_HOME`. The gateway has a notion of "Peers" and a role-based access decision for non-owner Peers (`ctx.remoteAccess.decide()`), which hints at per-peer access policy, but no user accounts, roles or sharing flow are documented. Authentication is a single launch-token plus signed session cookie for the one launched process.
- Status: unclear (no documented multi-user model)
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-server ; https://deepseek-harness.github.io/deepseek-harness/en/reference/api-gateway ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/scope ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/workspace ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/credentials
- Notes: The credentials page mentions a Platform session `userId`, which is about the DeepSeek account, not harness users. Enterprise administration, SSO, audit export: nothing documented. (verified in review) (web-server page does not address multi-user; Peers are in-process principals)

### Area 19. Observability

### Session telemetry (OpenTelemetry)
- What: An OTel backend mirrors every canonical session event (ledger) plus error and shutdown signals through a redaction waterfall; the seam ships no redaction rules, so with no listener the full payload (message text, tool arguments and results, workspace paths) is exported as captured. The CLI README says OTel session upload is feedback-gated with default mode `FEEDBACK_ONLY` (data released after explicit `/feedback`), `DSH_TELEMETRY_MODE=DISABLED` turns it off, `FULL` is rejected. Delivery is best effort.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session-telemetry ; https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/reference/README.md ; https://deepseek-harness.github.io/deepseek-harness/en/guide/network-proxy
- Notes: The two sources differ in tone: the subsystem page says no end-user opt-out is described and no default mode is stated; the CLI reference page states `FEEDBACK_ONLY` default. (verified in review) (FEEDBACK_ONLY default and FULL rejected are on the CLI reference page)

### DeepSeek session-log upload
- What: For official-API use, a DeepSeek session-log contributor uploads session-log events with requests by default; opt out in Settings -> General or by config `enabled: false`. The Python SDK page repeats this (up to 8 MiB per request).
- Status: preview
- Evidence: https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/reference/README.md (Telemetry) ; https://deepseek-harness.github.io/deepseek-harness/en/guide/python-sdk (Cautions)
- Notes: On by default, which is a privacy point for competitors to note. (verified in review)

### Trace view and logs
- What: Landing page describes a developer trace view with timelines, tool calls, inputs/outputs and timing. `$DSH_HOME` holds logs. Usage and cost display: nothing documented in the pages opened.
- Status: preview (vendor-claim for the trace view; no technical page found)
- Evidence: https://deepseek.com/harness ; https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/reference/README.md
- Notes: vendor-claim.

### Area 20. Configuration and customisation

### Profiles, bundles and patches
- What: Profile directory in `$DSH_HOME/profiles/<name>` holds `package.json` (manifest with ordered `bundles`) and `cordis.patch.yml`. Layer order: each bundle's patch, the profile patch, `$DSH_HOME/cordis.patch.yml`, then `--patch` overlays. `--from-default-profile` resets a profile from a template; `dsh rescue` exists. Shipped profiles: `web`, `headless`, `sdk`, `sdk-minimal`, `acp`. Settings UI edits go through the same patch files with optimistic revisions; secret fields are redacted on remote reads.
- Status: preview
- Evidence: https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/settings ; https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/reference/README.md
- Notes: Full config reference is a generated catalogue (`config-catalog`) that I did not read.

### Storage backends
- What: A storage hub with a JSON backend (human-readable file per unit) and a SQLite backend (one document per row); domains declare name, version and layout. Domains are in-process only.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/storage
- Notes: Default paths and per-user scoping: nothing documented.

### Environment variables
- What: `DSH_HOME` (default `~/.dsh`), `DSH_PERMISSION_MODE`, `DSH_WEB_URL`, proxy variables, `DSH_TELEMETRY_*`, `DSH_GITHUB_*` for the review overlay, `DSH_BUNDLED_SKILL_DIR`, provider credentials from env or `.env` files. SSH_CONNECTION or SSH_TTY suppress opening a browser.
- Status: preview
- Evidence: https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/reference/README.md ; https://deepseek-harness.github.io/deepseek-harness/en/guide/github-review ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/skills
- Notes: Not exhaustive.

### Area 21. Security and privacy

### Web authentication: launch token, cookie, host fence
- What: The printed startup URL carries a process launch token; visiting the mount entry exchanges it for a signed session cookie (`Secure` when TLS). A request fence then accepts only loopback, the listener's own bind address and `--trusted-host` authorities; other Hosts get 403 on every API call. Origin checks and browser-session authentication are enforced on every API route and stream. `authorizeIndex` either issues a token redirect or returns 401. Token format and lifetime are not documented. The bare mount URL is the only valid entry for the token exchange.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments (Authentication and access control); https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-server (Authentication and tokens)
- Notes: The CLI reference page says it does not describe a launch token; the two pages are consistent only if the launch token is documented on the deployment page alone. One credential per process, no accounts. (verified in review) (the term 'host fence' and the 403 behaviour are on the deployment page; the web-server page does not use the term)

### Tool arguments not shared with approvers
- What: Approval requests deliberately omit tool arguments (they carry agent, tool name, reason).
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval
- Notes: Interesting and arguably weak for UX: the user may have to see arguments elsewhere (tool call event); how the UI shows them is not documented. (verified in review)

### Disclosure of limits
- What: SAFETY.md and the docs openly state: no audit, sandbox is not a guarantee, `web_fetch` can exfiltrate, proxy passwords visible to commands.
- Status: preview
- Evidence: https://github.com/deepseek-ai/deepseek-harness/blob/master/SAFETY.md ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web ; https://deepseek-harness.github.io/deepseek-harness/en/guide/network-proxy
- Notes: n/a

### Area 22. Licence, pricing, distribution

### Licence and distribution
- What: MIT. Distributed via npm (`@deepseek-ai/dsh`, `npx @deepseek-ai/dsh web`), PyPI SDK (`deepseek-harness-sdk`), desktop installers (macOS arm64 dmg, Windows x64 exe), and source (`pnpm install`, `pnpm run build`, `pnpm dsh web`). The harness itself is free; model use needs a DeepSeek API key or another provider's. Pricing: nothing documented beyond that.
- Status: preview
- Evidence: https://github.com/deepseek-ai/deepseek-harness (README) ; https://deepseek.com/harness ; https://deepseek-harness.github.io/deepseek-harness/en/guide/python-sdk ; https://registry.npmjs.org/@deepseek-ai/dsh
- Notes: Version 0.2.2-rc.1 (2026-11-04).

### Area 23. Anything else or unusual

### DeepSeek-specific integration
- What: A DeepSeek account/platform plugin and DeepSeek LLM API extensions (including session-log upload and a package inventory contributor); `CLIENT_UPGRADE_REQUIRED` can be returned for models that need a newer client. The model picker in the landing page screenshot shows a DeepSeek model with effort level.
- Status: preview
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams (`ctx.deepseekAccount`, `ctx.deepseekLlmApiExtensions`) ; https://deepseek-harness.github.io/deepseek-harness/en/guide/providers
- Notes: n/a

### Office-style deliverables
- What: The `present` tool declares up to four files as final deliverables; the landing page shows live previews of Word, Excel, HTML, PDF and Markdown outputs; an `skill-office` skill package exists.
- Status: preview (live previews are a vendor-claim from the landing page)
- Evidence: https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog ; https://deepseek.com/harness ; https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams
- Notes: Positions it as a general work agent, not only a coding agent.

## 3. Distinctive or unusual

- Everything-is-a-plugin runtime with reversible registrations and hot reload: the loop, tool registry, LLM adapter and session log are all swappable by YAML patches; interesting but a heavy architecture, and it implies plugins run in-process.
- Capability "seams" (service definition, provider, consumer): swapping one provider (e.g. SSH sandbox) changes bash, PTY and LSP together. Clean way to make execution location pluggable.
- Strict log discipline: anything the model sees must be in the log; policy and mode changes are log events (`sandbox/mode`, `approval/policy`, `permission/preset`), so session state is a fold over the log. Matches the reader's invariant.
- Approval fail-closed with an audit trail: `approval/asked` and `approval/decided` events, and `never` enforced before any answerer runs.
- Compaction as a locked log sequence with crash-orphan detection (`start` without `end`).
- Timed `ask_user_question` whose answer can arrive as a later turn, even after a host restart.
- Goal objects with revision compare-and-set and a human-authorised resume.
- Subagents can be external harnesses (Codex, Claude Code, ACP, DSH SDK), not only in-process children.
- Model-written workflow scripts that spawn agents under a hard `maxTotalAgents` the script cannot see.
- Spill files created 0600 with exclusive creation to defeat symlink planting.
- Webhook-triggered read-only review sessions with PR fields marked untrusted in the prompt.
- Honest `full` vs `partial` sandbox enforcement status per backend.

## 4. Architecture and UX notes

- Process model: one Node process per `dsh` invocation. Profiles compose a plugin tree; the `web` profile includes an HTTP/WebSocket carrier, the connection and auth fence, an `/api` Typert RPC gateway, the agent loop, tools, storage and (optionally) schedulers. Electron bundles the same runtime and uses IPC instead of HTTP.
- Client/server split: browser React client with plugin-contributed slots; Host state mirrored into React-free client models; a multiplexed WebSocket carries logical streams; sessions use `follow()` plus `page()` so reconnects fill gaps. Docs describe reconnection as replacing the session window from a fresh snapshot.
- Where the loop runs: in the server process on the launching machine. Remote execution is by SSH tool backends (unverified detail), not by a remote agent node.
- Ports: UI/API 3080; the GitHub webhook overlay adds a separate loopback listener 3081 serving only `POST /github`.
- Deployment: loopback only by default; public exposure expected behind a TLS reverse proxy; one process credential (launch token) for everyone who has the URL.
- Persistence: JSONL zstd session logs per session, JSON or SQLite storage domains, a SQLite search index, a credentials YAML file, all under `$DSH_HOME` (default `~/.dsh`).
- Docs are Chinese-first with an English mirror; the docs site provides `llms.txt` and `.md` twins; much of the reference is generated.

## 5. Relevance to a self-hosted master/node harness

- Closest shape is the web UI plus a single server; but it has no master/node split and no multi-user model. The reader's separation (master never runs tools, node decides) is a gap competitors have not filled here (Architecture and UX notes; Multi-user support entry).
- Authentication is one launch token and a cookie for the process owner, with a Host/Origin fence (Web authentication entry). A multi-user master needs real accounts; DSH is a reference for the Host-fence and TLS-never-fall-back details, not for identity.
- The Host-header fence plus `--trusted-host` and rejecting `0.0.0.0` binds is a concrete, cheap pattern for the "loopback by default" invariant (Public deployment entry).
- Log-as-truth with policy changes as events and crash-closers on resume mirrors the reader's event-log invariant; worth comparing the closer-synthesis and torn-tail handling (Persistence entry; Event-sourced log entry).
- Approval design: fail closed, audit events, `never` enforced before answerers, arguments omitted from the request. Compare to the reader's approval path and decide whether to log args (Approval service entry).
- Sandbox honesty: filesystem-only confinement, `full`/`partial` enforcement labels and a hard `SANDBOX_UNAVAILABLE` rather than silent passthrough are worth copying; the lack of network confinement is the gap to beat (Process sandbox entry).
- The web_fetch SSRF recipe (resolve once, pin, re-check redirects) is a ready checklist (Web search and fetch entry).
- Reminders require the host to be running and are at-least-once; a master/node split could do better by keeping scheduling on the master and delivering to the node (Session reminders entry).
- Spill-to-file with a locator and retrieval hint, compaction as a locked log sequence, and per-session sandbox mode as an event are good small designs (Spill entry; Compaction entry).
- Plugin markets with a confirm-source dialog but in-process plugins and no signing show the risk of an open plugin model on a machine that runs shell commands (Plugin markets entry).

## 6. Overhyped, stale or contradictory

- `00-targets.md` says no GitHub releases; the releases API shows prerelease releases up to `dsh-v0.2.2-rc.1` (corrected above).
- The landing page presents agent teams, automatic authorization review, scheduled tasks and voice as experimental, while the docs site documents schedules in guide form without that label; maturity labels are inconsistent between pages.
- Landing-page statements ("out of the box", "explore the upper limit of intelligence", Pomodoro plugin built in 5 min 24 s, sourced research conclusions) are marketing; the build time is one demo.
- The subsystems index links `ssh`, `mcp`, `agent-team` and `webhook` as GitHub `docs/subsystems/*.md` files; the docs-site mirror has no such pages (404 at site URLs, and they are absent from `llms.txt`). Content exists, only the site mirror lags (corrected in review).
- The tool catalogue lists tools (persistent `bash`, `str_replace_editor`, PTY, session history) that are alternate packages or opt-in; the list does not tell which are mounted by default.
- The 278k star count for a repository created 2026-08-13 is the scout's and GitHub's figure; it says nothing about maturity (the project itself says developer preview).
- The npm first release listed BSD-3-Clause while later versions are MIT; I did not trace when it changed.
- The SDK page says the SDK profile pins `danger-full-access`, while the web quickstart says the Web UI asks for approval; these are different profiles, not a contradiction, but a reader skimming could confuse them.

## 7. Unverified

- (resolved in review) The `ssh`, `mcp`, `agent-team` and `webhook` subsystem pages are not on the docs site; they are in the repository at https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/{ssh,mcp,agent-team,webhook}.md and were read in review (see their entries and section 9).
- Whether the agent loop and web server really share one process in all web deployments (inferred). Settle with the web-profile config dump or the architecture note in `docs/`.
- Auto review: reviewer model, rules and prompt. Settle with a dedicated page or package README.
- Hooks (shell-command hooks) and `AGENTS.md`-style instruction loading for user workspaces. Settle with the system-prompt package README.
- Multi-user behaviour of the gateway's "Peer" and `remoteAccess` role policies. Settle with the connection plugin README.
- Default sandbox mode, compaction thresholds, default tool presets contents. Settle with `config-catalog` (generated config reference, not opened).
- Undo, checkpoints and rewind. Settle with the web UI or a session-history doc.
- Token cost display and usage UI.
- Whether any TypeScript SDK page exists.
- Desktop app signing and update mechanism.
- Stable (non-prerelease) release: none seen; the GitHub tags list (first 8 tags) showed only alpha and rc tags.

## 8. Sources

All opened in this pass (WebFetch unless noted):

- https://github.com/deepseek-ai/deepseek-harness - README; identity, install, status, licence.
- https://api.github.com/repos/deepseek-ai/deepseek-harness - repo metadata (curl).
- https://api.github.com/repos/deepseek-ai/deepseek-harness/releases and /tags - releases and tags (curl).
- https://registry.npmjs.org/@deepseek-ai/dsh and /0.2.2-rc.1 - npm versions, licence, publish date (curl).
- https://github.com/deepseek-ai/deepseek-harness/blob/master/SAFETY.md - safety notice.
- https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md - launcher, profiles, patches.
- https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/reference/README.md - web flags, env vars, telemetry, plugin commands.
- https://deepseek.com/harness - landing page; platforms and experimental list.
- https://deepseek-harness.github.io/deepseek-harness/llms.txt - docs index.
- DOCS guide pages: .../en/guide/quickstart, providers, network-proxy, public-deployments, python-sdk, github-review, schedule, mcp-memory, plugin-market (all under https://deepseek-harness.github.io/deepseek-harness/en/guide/).
- DOCS reference pages (under https://deepseek-harness.github.io/deepseek-harness/en/reference/): index (Architecture), agent-lifecycle, tool-execution-pipeline, tool-catalog (first 100000 characters and the remainder), capability-seams, api-gateway.
- DOCS subsystem pages (under https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/): index, session, session-query, persistence, spill, session-telemetry, token-meter, system-prompt, compaction, tools, shell, jobs, terminal, filesystem, lsp, ptc-runtime, web, skills, workflow, subagent, approval, permission-presets, sandbox, plan, user-questions, commands, goal, schedule, web-server, web-client, slots, storage, workspace, settings, credentials, scope.
- Returned 404 and unused: .../subsystems/ssh, mcp, agent-team, webhook.
- Not opened: the generated `config-catalog`, `persistence-catalog`, Cordis tutorials and API pages, develop/ pages, plugin development guides, the `sandbox-local` package README, SDK TypeScript docs, Discord and Discussions.


## 9. Review

Reviewed 2026-11-10 by an independent reviewer (public docs only; each cited page re-fetched with a question about the specific claim). Checked 30 entries plus the identity block out of about 77 feature entries (100 `###` headings, of which about 23 are area headings).

| Entry | Verdict | Reason |
|---|---|---|
| Identity: official, preview, MIT | confirmed | README: deepseek-ai org, "THERE WILL BE COMPATIBILITY-BREAKING CHANGES", MIT, tagline; org-type and npm maintainer details not re-checked |
| Identity: release state | confirmed | Releases API: `dsh-v0.2.2-rc.1` (2026-11-04), `alpha.3`, `0.2.1-rc.5`, all prerelease; correction to 00-targets is right |
| Process sandbox | confirmed | Modes, backends, `SANDBOX_UNAVAILABLE` ("silent passthrough never legal"), full/partial, network out of scope, precedence |
| Approval service | confirmed | ask/never, last `approval/policy` event, fail closed, args omitted, audit append failure rejects |
| Permission presets | confirmed | Pairings, log-only `permission/preset` event, auto and custom |
| web_fetch unconfirmed | confirmed | Every sandbox and approval mode, SSRF and exfiltration caveat |
| Plugin markets / plugin_manager | confirmed | Source-confirmation dialog, http index tamperable; plugin_manager rule on the tool catalogue; signing and in-process claims are inference |
| Workflows | confirmed | Per-run `maxTotalAgents` that the script cannot see, no deadline |
| Subagents | confirmed | External handles, follow-ups rejected; model-selection default is from the catalogue |
| Agent teams | confirmed-with-correction | Page found on GitHub; default-off, advisory write_scopes |
| Background jobs | confirmed | 262144 / 16384 / 10, predictable ids |
| Session reminders | confirmed-with-correction | "Exactly-once not guaranteed", not "at-least-once" wording |
| Persistence | confirmed | Zstd frames, fsync, torn tail, single writer, closers |
| Server: one process | confirmed-with-correction | Docs do not state it; inference (survey says so in Notes) |
| SSH remote backend | confirmed-with-correction | Not a 404-lost page: it is on GitHub; harness stays local |
| Public deployment | confirmed | Wildcard host rejected, TLS never falls back, 3080 |
| Web authentication | confirmed | Launch token, cookie, host fence, 403 |
| Multi-user | confirmed | Nothing documented |
| Credentials handling | confirmed-with-correction | File location is on providers page, not credentials page |
| Session telemetry | confirmed | FEEDBACK_ONLY default, FULL rejected |
| Session-log upload | confirmed | On by default, 8 MiB, opt-out |
| Python SDK | confirmed | Package, Python 3.10+, sdk-minimal, danger-full-access pin |
| Tool arguments omitted | confirmed | Approval page quote |
| Web search and fetch | confirmed | Default 8, pinned addresses |
| MCP client | confirmed-with-correction | Page found on GitHub; OAuth/approval undocumented; naming format not shown |
| Web UI | confirmed-with-correction | Right sidebar IS documented (sidebar-right page) |
| Tool groups and inventory, ralph | confirmed | Counts and caps match tool catalogue |
| GitHub PR review | confirmed | `127.0.0.1:3081`, ready_for_review, read-only, no dedup |
| Desktop app, Auto review | confirmed | Landing page platforms and experimental list |

Specific points requested by the caller:
- Official repository and release state: confirmed (see above); stable release: none seen in the first three.
- Plugin architecture ("everything is a plugin"): confirmed in the reference index.
- Policy and mode changes as session-log events: confirmed in substance (`sandbox/mode`, `approval/policy`, `permission/preset` events on three pages).
- Fail-hard sandbox behaviour and approval service: confirmed.
- External harnesses as sub-agents: confirmed.
- Workflow scripts and cap: confirmed.
- Single-process shape: NOT stated by any doc; keep as inference.
- Authentication model: confirmed; one process launch token plus cookie, no accounts.
- 404 pages: the pages are not on the docs site and not in `llms.txt`, but the subsystems index links them as GitHub files `docs/subsystems/ssh.md`, `mcp.md`, `agent-team.md`, `webhook.md` (all opened). The repository `docs/` folder also holds architecture, glossary, upgrade-guide, postmortem and defensive-patterns material.

Borrowed features: none found. Phrases like "plan mode", "skills" and "MCP" follow DSH's own docs.

Notable omissions:
- Webhook subsystem (provider deliveries, no queue/retry/dedup): https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/webhook.md
- Right sidebar: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/sidebar-right
- Session titles, session projections, session references: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session-title
- LLM streaming: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/llm-streaming
- Subprocess provider: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subprocess
- Typert RPC and client modules/resources: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/typert
- Conversation assembly: https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/conversation
- Repository docs: upgrade guide, defensive patterns, postmortem, session-format-status: https://github.com/deepseek-ai/deepseek-harness/tree/master/docs
- Event producer/consumer and persistence catalogue: same folder
- Cordis cookbook and tutorials: same folder

Overall judgement: largely reliable and unusually careful; every checked claim matched its page except six small corrections above (all fixed). Unchecked remainder (compaction, skills, plan mode, goals, storage, LSP, stats from the generated catalogue) is plausible, but the unread last 25,000 characters of the tool catalogue and the generated config catalogue mean "default" values should be treated as unconfirmed. Star count, org type and npm details in the identity block were not re-verified.
