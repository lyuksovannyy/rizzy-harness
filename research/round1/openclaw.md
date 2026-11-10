# OpenClaw: Round 1 public-surface survey

Surveyed 2026-11-10 from docs.openclaw.ai pages and the GitHub README (no source code read). All URLs below were opened by WebFetch on that date. Page text was summarised by a fetch tool, so mechanism details are as good as that summary; items marked `unverified` were not confirmed on a page.

## 1. Identity

- Name: OpenClaw. Vendor: the OpenClaw Foundation, described in the README as an independent 501(c)(3) that employs the core team and signs releases; README says no paid tier, hosted service or token. Donors named in the README include Amazon, OpenAI, Red Hat (https://github.com/openclaw/openclaw).
- What: self-hosted personal/shared assistant. One long-lived Gateway process connects chat channels, a web Control UI, CLI/TUI, companion apps and device "nodes" to an agent runtime. Coding is one capability among many (README, docs home).
- Surfaces: Gateway daemon, Control UI (web), CLI, TUI, macOS menu bar app, iOS, Android, Windows Hub (plus a Tauri app), Linux companion, 40+ chat channels (https://docs.openclaw.ai/platforms, https://docs.openclaw.ai/concepts/features).
- Open/closed: open, MIT (README). TypeScript, Node.js 24.16+ or 26.1+ required (README, getting-started).
- Version: `00-targets.md` says `v2026.11.2` (2026-11-08). https://docs.openclaw.ai/releases agrees v2026.11.2 is the latest listed, "November 2026", no exact day. Confirmed, date not independently confirmed.
- Correction to targets file: none found. The targets file says "personal assistant first, coding second"; docs agree (Why OpenClaw page), though there is substantial coding support (Codex/Claude Code harness hosting, managed worktrees, cloud workers, ACP).

## 2. Features by area

### Area 1. Agent loop and execution model

### Embedded agent loop (default runtime `openclaw`)
- What: The `agent` RPC validates, resolves the session, persists metadata and returns `runId` immediately; `runEmbeddedAgent` reserves the session turn, resolves model and auth profile, streams assistant/tool deltas and enforces a per-attempt timeout. Lifecycle phases are start, finishing, end, error; `agent.wait` returns ok/error/timeout for a runId. Default per-attempt timeout is 48h (172800s), model-idle 120s cloud / 300s self-hosted.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/agent-loop (Agent loop)
- Notes: Page does not name an underlying library. Docs version as of 2026-11. (verified in review: 48h, 120s/300s; each fallback gets a fresh budget; 0 removes the limit)

### Pluggable agent runtimes (openclaw, codex, copilot, muse, CLI backends, ACP)
- What: Runtime is separated from provider, model and channel. `openclaw` is built in; `codex` is a bundled plugin that runs OpenAI models through the Codex app-server (Codex owns loop and compaction, OpenClaw mirrors the transcript and bridges tools); `copilot` and `muse` are opt-in external plugins. CLI backends route a normal model ref through a local CLI (e.g. `claude-cli`). ACP/acpx runs external harnesses (Claude Code, Gemini CLI, OpenCode, Cursor, Droid). Selection order: model-scoped policy, provider-scoped policy, `auto` plugin claim, fallback `openclaw`. Once a harness starts, failures are not retried on another runtime.
- Status: GA (individual runtimes vary; `copilot`/`muse` opt-in)
- Evidence: https://docs.openclaw.ai/concepts/agent-runtimes (Agent runtimes)
- Notes: Status output shows "Execution" and "Runtime" separately.

### Per-session turn serialization, durable writer claim
- What: Turns are serialized per session before global capacity admission. Each admitted run records a durable active-writer claim (`activeWriterRunId`) before streaming, and the synchronous commit transaction for transcript writes verifies it still matches, so a superseded run cannot commit stale data. A state-directory lock preventing two Gateways owning the same state is described on the getting-started/install side, not on the queue page.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/agent-loop (Queueing and concurrency); https://docs.openclaw.ai/concepts/queue (Command queue)
- Notes: Close analogue of the reader's "one writer per session" invariant. (corrected in review: the page says the commit transaction verifies the claim, not that the claim is written in the same transaction; the state-dir lock is not on the queue page, so that sub-claim is unverified)

### Command queue modes (steer, followup, collect, interrupt)
- What: Inbound messages during an active run are handled per mode: `steer` (default) injects into the active run, `followup` queues a separate turn, `collect` coalesces after a quiet period, `interrupt` aborts and runs the newest. Defaults: 500 ms debounce, cap 20 per session, overflow `summarize`. Accepted input is stored before the Gateway acks. Sub-agent concurrency default 8; background lanes are separate from user replies.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/queue (Command queue)
- Notes: Settable per session via `/queue`, per channel in config. (verified in review: steer default, 500 ms, cap 20, summarize, concurrency 8)

### Stuck-run watchdog and stuck-session diagnostics
- What: A 2-minute threshold flags `session.long_running`/`stalled`/`stuck` (only when diagnostics are enabled); the controller watchdog (semantic-stall threshold at least 5 minutes and 3x the warning threshold, runs even with diagnostics off) requests Stop.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/agent-loop (Stuck-session diagnostics)
- Notes: (corrected in review: 2-minute threshold applies only with diagnostics enabled; 5 minutes is a floor)

### Reply shaping: NO_REPLY, empty-reply finalization
- What: A token-only `NO_REPLY` is a valid silent finish for external turns; an empty required reply after tools can trigger a tool-free finalization pass; a malformed tool call can trigger one continuation.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/agent-loop (Tool execution and reply shaping)
- Notes: none

### Area 2. Built-in tools

### Built-in tool set
- What: Runtime: `exec`, `process`, `terminal` (shared operator terminal in Control UI), `code_execution` (provider-backed Python). Files: `read`, `write`, `edit`, `apply_patch`. Human input: `ask_user`, `secrets` (get a credential without exposing it to the agent). Web: `web_search`, `x_search`, `web_fetch`. `browser`, `message`, `sessions_*` (incl. `sessions_spawn`), `agents_wait`, `subagents`, `agents_list`, `session_status`, `get_goal`/`create_goal`/`update_goal`, `automations` (alias `cron`), `gateway`, `nodes`, `plugins`, media tools (`view_image`, `image_generate`, `music_generate`, `video_generate`, `tts`), UI tools (`screen`, `theme`, `progress_card`), and `personal_instructions` for multi-user. `computer` tool exists separately (see Area 16).
- Status: GA (individual tools vary)
- Evidence: https://docs.openclaw.ai/tools (Tools); https://docs.openclaw.ai/nodes/computer-use
- Notes: Tool names as listed on the page 2026-11-10.

### Tool Search for large catalogs
- What: `tool_search`, `tool_describe`, `dispatch_action` let the model discover and call many eligible tools without every schema being sent; docs experimental list says Tool Search is on by default when unset.
- Status: GA
- Evidence: https://docs.openclaw.ai/tools; https://docs.openclaw.ai/concepts/experimental-features
- Notes: Listed in "Experimental features" yet described as on by default: wording is inconsistent.

### Tool policy layers (profiles, allow/deny)
- What: Five layers: tool profile, provider profile, global/per-agent allow and deny, provider allow/deny, sandbox tool policy. `deny` always wins; non-empty `allow` blocks all unlisted. Tools removed by policy are not sent to the model. Policy does not inspect what `exec` does internally.
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/sandbox-vs-tool-policy-vs-elevated; https://docs.openclaw.ai/tools
- Notes: Profile contents (e.g. `coding`, `messaging`, `minimal`) not fully listed on pages read. Loop-detection thresholds live at /gateway/config-tools/built-in-tools, not read: unverified.

### Plugin-provided tools
- What: Diffs renderer, `show_widget` (inline SVG/HTML in chat), LLM Task (JSON-only workflow steps), Lobster (typed workflows with resumable approvals), Tokenjuice (compacts noisy exec output), Canvas (macOS widget panel).
- Status: GA/unclear per plugin
- Evidence: https://docs.openclaw.ai/tools (Plugin-provided tools)
- Notes: Lobster and Tokenjuice pages not opened.

### Area 3. Code editing and file operations

### File tools and workspace boundary
- What: `read`, `write`, `edit`, `apply_patch`. The workspace is the default cwd, not a sandbox: absolute paths can reach the host unless sandboxing is on. Session permission modes add a filesystem boundary (see Area 5).
- Status: GA
- Evidence: https://docs.openclaw.ai/tools; https://docs.openclaw.ai/concepts/agent-workspace
- Notes: Security docs also mention root-bounded file access and atomic writes (https://docs.openclaw.ai/gateway/security, index only).

### Changes panel (git diffs in Control UI)
- What: Control UI has a Changes panel for Git diffs.
- Status: GA
- Evidence: https://docs.openclaw.ai/web/control-ui
- Notes: Detail not documented on that page.

### Area 4. Terminal and shell integration

### exec, process, background exec, shared operator terminal
- What: `exec` runs shell commands, `process` manages running ones, `terminal` is a shared operator terminal in the Control UI; a Background exec page exists (`/gateway/background-process`, not opened). TUI via `openclaw tui`. Exec host can be sandbox, gateway or a paired node.
- Status: GA
- Evidence: https://docs.openclaw.ai/tools; https://docs.openclaw.ai/tools/exec-approvals
- Notes: Background process page not read.

### Area 5. Permissions, approvals and sandboxing

### Exec approvals (policy, allowlist, ask)
- What: Commands on a real host run only if policy, allowlist and any required approval permit. Settings: `tools.exec.mode` (deny, allowlist, ask, auto where an LLM reviewer grants/denies/escalates, full); `security` (default `full` on gateway/node hosts, `deny` on sandbox); `ask` (default `off`); `askFallback` (default `deny`); `strictInlineEval` (default false). Approvals can only tighten config. Allowlist entries are per-agent globs on resolved binary paths with optional regex `argPattern`; `allow-always` entries bind to exact argv and cwd. Approved node-host runs are bound to cwd, exact argv, env (when present) and pinned executable path (when applicable); the Gateway binds every resolved command-segment executable before review and re-checks it before launch, and a changed resolution (e.g. a new executable earlier on PATH) denies the run. Protected executables use real-path identity, writable ones also a content hash. In `auto` mode login/interactive shell wrappers skip the reviewer and need a human.
- Status: GA
- Evidence: https://docs.openclaw.ai/tools/exec-approvals; https://docs.openclaw.ai/tools/exec-approvals-advanced
- Notes: Defaults mean host exec is unrestricted unless the operator configures otherwise (Trust model page: unprompted host exec is the intended single-operator default). Page says approvals are not a per-user auth boundary. (verified in review; refined: env binding is conditional and re-check is of resolved executables)

### Approval delivery: Control UI, macOS app, chat channels
- What: Gateway broadcasts `exec.approval.requested` to operator clients; resolved via `exec.approval.resolve`. `approvals.exec` can forward prompts to chat targets and users reply `/approve <id> allow-once|allow-always|deny`. Native approval UIs on Discord, Slack, Telegram, Matrix, QQ bot, Google Chat check approver lists. Resolving needs scope `operator.approvals`.
- Status: GA
- Evidence: https://docs.openclaw.ai/tools/exec-approvals; https://docs.openclaw.ai/tools/exec-approvals-advanced
- Notes: (verified in review) Cron approvals go only to connected exec-approval clients; "always allow" there mints a standing grant tied to job, command, cwd, env until revoked (optional `grantExpiryDays`).

### Safe bins and shell parsing rules
- What: `tools.exec.safeBins` auto-allows stdin-only filters (cut, head, tail, wc...) with per-binary flag profiles; `awk`, `sed`, `jq` are always denied; interpreters must not be added; safe bins resolve from `/bin` and `/usr/bin` by default. Chains need every segment allowed; redirections and command substitution rejected; `allow-always` stores the inner executable behind `env`/`nice`/`timeout` wrappers. Writable executables are also bound by content hash. Page admits substitutions made during a remote approval wait for inner shell commands are not detected.
- Status: GA
- Evidence: https://docs.openclaw.ai/tools/exec-approvals-advanced
- Notes: Self-stated gap noted. (verified in review)

### Session permission modes
- What: Four per-session modes: `read-only` (no mutation tools, exec denied), `guarded` "Ask me" (allowlist then human), `workspace` "Auto-review" (LLM reviewer allows/denies/escalates; three consecutive denials escalate to human), `full` (no reviewer, needs `operator.admin`). Boundary is the session root or workspace. Changing modes mid-task is supported on native runtime and Codex, rejected for CLI-backed runs. Pending approvals from the old mode are cancelled.
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/permission-modes
- Notes: Independent of elevated exec. Mode labels "Read only", "Ask me", "Auto-review", "Full access" and the three-denial rule quoted on the page. (verified in review)

### Sandboxing (Docker, Podman, SSH, OpenShell, Crabbox)
- What: Only tool execution moves into the sandbox; the Gateway stays on the host. Modes `off`, `non-main`, `all`; scope e.g. `session`; `workspaceAccess` none/ro/rw. Off by default. Docs say it is "not a perfect security boundary". GitHub identity excluded from sandbox unless `tools.github.allowInSandbox`. `openclaw sandbox list|explain|recreate`. OpenShell backend: OpenClaw drives the `openshell` CLI and runs commands over SSH, mirror or remote workspace mode, no sandbox browser support.
- Status: GA (OpenShell page gives no maturity level)
- Evidence: https://docs.openclaw.ai/gateway/sandboxing; https://docs.openclaw.ai/gateway/openshell; https://docs.openclaw.ai/gateway/sandbox-vs-tool-policy-vs-elevated
- Notes: Maturity scorecard rates "Browser automation, exec, and sandbox tools" M3 Beta (https://docs.openclaw.ai/maturity/scorecard). Sandbox off by default and "not a perfect security boundary" verified in review; the modes `off/non-main/all` and workspaceAccess values are on subpages (modes-scope-and-backend) not checked.

### Elevated exec escape hatch
- What: `tools.elevated` lets `exec` run outside the sandbox for senders in `tools.elevated.allowFrom.<provider>`; exec-only, does not add tools or bypass deny; `/elevated full` skips approvals for the session; cannot bypass a creator role's required sandbox.
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/sandbox-vs-tool-policy-vs-elevated
- Notes: none

### Node command policy
- What: A node command runs only if declared by the node, in the node's approved set on its paired record, and in the Gateway's per-platform allowlist. Sensitive commands (camera.snap/clip, screen.record, contacts/calendar/reminders add, health.summary, sms.*) need a persistent opt-in in `gateway.nodes.commands.allow`; `deny` overrides everything. Linux allows only `system.notify` and `computer.act` by default.
- Status: GA
- Evidence: https://docs.openclaw.ai/nodes/command-policy
- Notes: `system.run` and `screen.snapshot` become available only after an operator approves a pairing that declares them. (verified in review; page words this for desktop host commands including system.run, screen.snapshot only implied)

### Operator scopes and named roles
- What: Operator scopes: read, sessions.read, sessions.write, write, cron, admin, pairing, approvals, questions, talk, talk.secrets. Named roles under `gateway.roles.definitions` can set visibility of others' sessions (none/view/suggest/write), allowed agents, scope ceiling, sandbox inherit/required, model policy. Roles assignable by explicit RPC, GitHub login mapping, or default. Shared-secret auth equals full operator. Docs say scopes are a guardrail within one trust domain, not tenant isolation.
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/operator-scopes
- Notes: (verified in review; scope names are prefixed `operator.`; scope ceiling can only filter, not add)

### Security audit command
- What: `openclaw security audit` reports findings by `checkId` and severity (critical/warn/info) across fs perms, gateway bind/auth, hooks, browser, sandbox, exec/elevated, skills/plugins, channels, exposure. Auto-fix only for filesystem permission checks. `--deep` adds plugin/skill code scans (500 files cap, skips >1 MiB) and gateway probes.
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/security/audit-checks; https://docs.openclaw.ai/gateway/security
- Notes: none

### Channel sender access control (DM policy, groups)
- What: `dmPolicy`: pairing (default; unknown senders get an 8-char code, expiring in 1 hour, max 3 pending per channel account), allowlist, open (needs explicit `"*"`), disabled. Group allowlists, mention gating, `contextVisibility` (all/allowlist/allowlist_quote). Slash commands authorized via `commands.allowFrom` or allowlists. Pairing state lives in SQLite.
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/security/access-control; https://docs.openclaw.ai/channels/pairing
- Notes: none

### Prompt-injection controls
- What: Project says model choice is the first mitigation, but hard limits come from tool policy, approvals, sandboxing, allowlists. External content is wrapped in boundary markers and chat-template tokens are stripped; outbound replies pass a sanitizer. Guidance: read-only reader agent for untrusted content, keep `allowUnsafeExternalContent` off.
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/security/prompt-injection
- Notes: Cites a 2026 attack-success test (about 0.5% to 8.5% across models); source of that test not given: vendor-claim.

### Area 6. Context management and compaction

### Compaction
- What: Auto-compaction on by default at the smaller of 75% of window or window minus reserve, or on overflow error (compact and retry). Single summarization request capped around 40k input tokens; tool-call/result pairs kept together; invalid summaries rejected. Default mode `safeguard` audits the summary (structure, pending requests, identifiers) and marks degraded results. Config: separate summarizer `model`, `maxSummaryChars` (16k), `identifierPolicy` strict, `keepRecentTokens` 20k. Manual `/compact [focus]`. Native compact endpoints for OpenAI Responses/xAI; Anthropic server-side compaction. Plugins may register compaction providers.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/compaction (Compaction)
- Notes: Full history stays on disk. (verified in review: 75%/reserve, safeguard, 16k, 20k, strict, 40k cap)

### Session pruning (tool-result trimming)
- What: Client-side removal of old tool results from the prompt: after cache TTL, soft-trim large results (keep first/last 1500 chars of >4000), hard-clear at roughly 50% usage; last three assistant turns and everything before the first user message are protected. For direct Anthropic API-key calls it delegates to server-side clearing. Off by default except Anthropic auth seeds `cache-ttl` 1h.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/session-pruning
- Notes: none

### Context engine plugin slot
- What: A pluggable engine owns ingest, assemble, compact, after-turn (plus optional bootstrap, maintain, subagent hooks). Default `legacy`. `ownsCompaction` decides who compacts. A failing plugin engine is quarantined and falls back to legacy.
- Status: GA (plugin APIs are described as experimental in the experimental-features page)
- Evidence: https://docs.openclaw.ai/concepts/context-engine; https://docs.openclaw.ai/concepts/experimental-features
- Notes: none

### System prompt assembly and cache boundary
- What: Built per run in layers; sections include Tooling, Execution Bias, Promised Work, Care, Skills, Messaging, Sandbox etc. Prompt modes `full`, `minimal` (sub-agents), `none`. Stable content sits above a cache boundary; date/time are in a per-turn message so day rollover does not bust the prefix. Bootstrap files capped 20,000 chars each and 60,000 total.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/system-prompt
- Notes: none

### Area 7. Memory and persistent instructions

### Markdown workspace memory
- What: Memory is Markdown in the agent workspace (default `~/.openclaw/workspace`): `MEMORY.md` (curated long-term, loaded at session start), `memory/YYYY-MM-DD.md` daily notes (today/yesterday loaded on `/new`), `USER.md` (capped 4,000 chars), `DREAMS.md`. Tools: `memory_search` (hybrid vector plus keyword; SQLite default, PostgreSQL on some runtimes; Honcho and LanceDB options), `memory_get`, `intent`. A silent pre-compaction turn saves notes (can be disabled). Control UI can import Markdown memory from Codex, Claude Code and Hermes into `memory/imports/`.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/memory; https://docs.openclaw.ai/concepts/agent-workspace
- Notes: Project says memory guides behaviour but enforces no policy. Why-OpenClaw page admits promoted memories have no time-based retention and deletion has documented gaps.

### Dreaming (background memory consolidation)
- What: Default-on `memory-core` pipeline with phases light, REM, deep. Only deep writes `MEMORY.md`: candidates scored on relevance, frequency, query diversity, recency, consolidation, richness and must pass `minScore`, `minRecallCount`, `minUniqueQueries`; a model proposes add/merge/supersede with append-only fallback; a rewrite dropping more than 25% of prior entries is rejected; prior `MEMORY.md` saved in SQLite. Runs as a cron job, default 03:00 daily. Untrusted content is kept out of promotion.
- Status: GA (not called experimental on page)
- Evidence: https://docs.openclaw.ai/concepts/dreaming; https://docs.openclaw.ai/concepts/memory
- Notes: Page `memory-provenance` exists, not opened. (verified in review: default-on, phases, 25% guard, 03:00, SQLite backup of prior MEMORY.md; numeric thresholds not on page)

### Workspace instruction files
- What: `AGENTS.md` (operating rules), `SOUL.md` (persona), `IDENTITY.md`, `USER.md`, `BOOT.md` (startup checklist), `BOOTSTRAP.md` (first-run ritual, deleted afterward). Injected into the system prompt; sub-agents get only `AGENTS.md`. Workspace can be a private git repo.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/agent-workspace; https://docs.openclaw.ai/concepts/system-prompt
- Notes: The page's summary lists `TOOLS.md` and `HEARTBEAT.md` but its file map does not; heartbeat doc says runtime no longer reads `HEARTBEAT.md`.

### Standing intents (event-triggered reminders)
- What: Owner-created instructions (via `intent` tool, only `commands.ownerAllowFrom` senders on authenticated channels) that fire when a user turn matches keywords. Matching is a rule-based full-text prefilter with no model call, max 256 scoped candidates per turn. Defaults: cooldown 24h, max 3 fires, expiry 90 days. Lifecycle pending/armed/fired/done/cancelled/expired in per-agent SQLite.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/standing-intents
- Notes: none

### Standing orders
- What: Convention, not a feature flag: permanent authority for defined programs (scope, triggers, approval gates, escalation) written in `AGENTS.md`; automations supply the "when". Retries capped at three before escalation.
- Status: GA
- Evidence: https://docs.openclaw.ai/automation/standing-orders
- Notes: Pure prompt guidance; enforcement relies on tool policy.

### Area 8. Planning, tasks and modes

### Goals and Workboard
- What: Tools `get_goal`/`create_goal`/`update_goal`. A bundled Workboard plugin tracks cards (statuses triage, backlog, todo, scheduled, ready, running, review, blocked, done); a dispatch loop promotes ready cards and starts sub-agent workers (default 3 per pass, one per owner). Worker "proof" statuses are self-reports.
- Status: unclear (bundled plugin; no maturity label)
- Evidence: https://docs.openclaw.ai/cli/workboard; https://docs.openclaw.ai/tools
- Notes: A separate dedicated plan mode: nothing documented on pages read (session permission modes cover read-only).

### Advisor, decision models
- What: An advisor model reviews agent work and can send one correction; "decision assistance" needs a Decision model. Both off by default.
- Status: experimental
- Evidence: https://docs.openclaw.ai/concepts/experimental-features
- Notes: Only flag-level description read.

### Area 9. Sub-agents and multi-agent orchestration

### Sub-agents
- What: Background runs spawned from a run via `sessions_spawn`, each in its own session, announcing results to the requester; no session or message tools by default; nesting depth configurable; separate model via `agents.defaults.subagents.model`; `context: "fork"` shares the parent transcript; `sessions_yield` ends the turn to await results. Control UI Subagents panel shows child transcripts view-only. Hidden children vs `visible: true` interactive sessions; thread-bound sessions on channels.
- Status: GA
- Evidence: https://docs.openclaw.ai/tools/subagents; https://docs.openclaw.ai/concepts/session-tool; https://docs.openclaw.ai/concepts/queue
- Notes: Depth cap numbers on a subpage not opened. Default concurrency 8 per spawning session (queue page). (verified in review; the sub-agents index page itself states no numbers, and `sessions_yield` is only listed there)

### Multi-agent routing in one Gateway
- What: Multiple isolated agents per Gateway, each with workspace, `agentDir` (auth profiles, model registry) and its own SQLite session store. Bindings route by channel, account, peer, guild/team with specificity ordering; unbound messages are not sent to an arbitrary agent. Per-agent sandbox and tool policy. Cross-agent session access is on by default (`tools.agentToAgent`).
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/multi-agent; https://docs.openclaw.ai/gateway/security/trust-model
- Notes: Docs explicitly warn the default session visibility is `all`.

### Delegate architecture
- What: A delegate is a named agent with its own organizational identity (mail, calendar) acting under standing orders; three tiers (read/draft, send-on-behalf, proactive); hard blocks in `SOUL.md`/`AGENTS.md`, gateway tool policy and IdP-side permissions enforce limits.
- Status: GA (guidance)
- Evidence: https://docs.openclaw.ai/concepts/delegate-architecture
- Notes: none

### Group-thread agent coordination and bot-loop protection
- What: Multi-agent group threads have coordinator limits (max 4 rounds, 32 participant turns per inbound message, in memory). Bot-to-bot loop guard: default 20 events per 60s per pair then 60s cooldown.
- Status: GA
- Evidence: https://docs.openclaw.ai/channels/bot-loop-protection
- Notes: none

### Hosting other harnesses via ACP
- What: `/acp spawn` launches external harnesses (Codex, Claude Code, Gemini CLI, OpenCode, Cursor, Droid) through acpx. Separately `openclaw acp` is an ACP server over stdio bridging IDEs (Zed documented) to a Gateway session. Bridge lacks per-session MCP servers, client fs/terminal methods; usage approximate, no cost.
- Status: GA/partial (explicit limitation list)
- Evidence: https://docs.openclaw.ai/cli/acp; https://docs.openclaw.ai/concepts/agent-runtimes
- Notes: none

### Area 10. Background, long-running and scheduled work

### Automations (cron)
- What: Built-in scheduler inside the Gateway; jobs, state and history persist in the shared SQLite DB. `openclaw automations` (alias `cron`). One-shot (`--at`) jobs auto-delete after delivery. Payload kinds include agent turns, command and script jobs. Isolated agent-turn jobs have a 60-min watchdog; command 10 min; script 5 min. Restart recovery: completed results restored without rerun; interrupted time-scheduled agent-turn jobs get one retry; command/script/event jobs not replayed. The agent can propose promoting a repeated request into a job, run it once as a visible test and delete it on failure. Delivery to a chat channel, webhook or nowhere.
- Status: GA
- Evidence: https://docs.openclaw.ai/automation/cron-jobs; https://docs.openclaw.ai/automation/cron-jobs/how-it-works
- Notes: Gateway must be running for schedules to fire. (verified in review: 60/10/5 min, one-shot deletion on confirmed delivery, restart recovery)

### Heartbeat (retired, now an ordinary automation)
- What: The separate heartbeat monitor was retired; migration creates a "Proactive check" job (default every 30 min) with `activeHours`, `delivery.target: "owner"`, and `NO_REPLY`/`no_change` quiet results. Runtime no longer reads `HEARTBEAT.md`; doctor imports it into job scratch. Migration advances the shared DB to schema 30 and older Gateways cannot run it.
- Status: GA (migration notice); older docs may still describe heartbeat
- Evidence: https://docs.openclaw.ai/gateway/heartbeat (Heartbeat migration)
- Notes: Home page and features page still say "heartbeat scheduling": docs lag.

### Inbound webhooks
- What: Disabled by default. `POST /hooks/wake` (system event to main session), `POST /hooks/agent` (submit an agent turn, isolated by default, `Idempotency-Key`, optional `waitForCompletion`), `POST /hooks/<name>` via mappings with templates or trusted JS/TS transforms. Dedicated token (not the Gateway token); query-string tokens rejected; 429 on repeated auth failures. Fan-out is not exactly-once. Content is wrapped as untrusted but wrapping does not remove tool access.
- Status: GA
- Evidence: https://docs.openclaw.ai/automation/cron-jobs/webhooks
- Notes: none

### Other triggers: Gmail Pub/Sub, IMAP, MCP events
- What: Pages exist for Gmail Pub/Sub triggers (restricted reader model), IMAP email trigger and MCP Events.
- Status: unclear
- Evidence: https://docs.openclaw.ai/llms.txt (page list only)
- Notes: unverified beyond titles; prompt-injection page says to route Gmail hook mail to a restricted reader agent.

### Internal hooks and plugin hooks
- What: Internal hooks are JS/TS handlers in the Gateway process for events (`/new`, `/reset`, `/stop`, `gateway:startup`, `message:received`); bundled `boot-md`, `bootstrap-extra-files`, `command-logger`, `compaction-notifier`, `session-memory`. No timeout, retry or durable queue. Plugin hooks (`api.on`) can modify prompts, intercept `before_tool_call`, `message_sending`, with blocking terminal. Hooks are trusted code with full Gateway access.
- Status: GA
- Evidence: https://docs.openclaw.ai/automation/hooks; https://docs.openclaw.ai/concepts/agent-loop
- Notes: none

### Restart resume
- What: On graceful stop the Gateway marks running root sessions and on startup injects one resume input so the agent can check files, processes and external actions; crashes can still resume eligible sessions.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/session
- Notes: none

### Area 11. Sessions: persistence, resume, fork, checkpoints, undo

### Session keys and routing
- What: Gateway owns all session state. DMs share one `main` session by default; groups are isolated per group; cron gets a fresh session per run; webhooks isolated per hook. `session.dmScope`: main, per-peer, per-channel-peer (recommended for shared inboxes), per-account-channel-peer. `session.identityLinks` joins one person across channels. Reset policies: none (default; compaction manages growth), daily (default 04:00), idle, manual `/new`/`/reset`.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/session; https://docs.openclaw.ai/gateway/security/access-control
- Notes: The shared-DM default is a documented privacy hazard for multi-user inboxes. (verified in review: dmScope main, reset none by default, daily hour 4 if enabled)

### Session storage and maintenance
- What: Per-agent SQLite at `~/.openclaw/agents/<agentId>/agent/openclaw-agent.sqlite`; shared state in `~/.openclaw/state/openclaw.sqlite` (PostgreSQL optional). Defaults `pruneAfter` 30d, `maxEntries` 5000. `openclaw sessions cleanup --dry-run`. Transcript search tool `sessions_search`.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/session; https://docs.openclaw.ai/tools/exec-approvals; https://docs.openclaw.ai/concepts/session-tool
- Notes: PostgreSQL operator guide page exists, not opened.

### Incognito sessions
- What: Control UI "New thread" option; kept in memory, expire after 24h or restart, not archived. Docs warn Codex runtime's own log still records messages.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/session
- Notes: none

### Edit and resend, no undo of external effects
- What: Control UI can edit and resend failed replies; docs say rewinding does not undo external actions. File checkpoints/rewind: nothing documented on pages read (managed worktree snapshots cover git state, see Area 12).
- Status: GA / nothing documented for checkpoints
- Evidence: https://docs.openclaw.ai/web/control-ui
- Notes: Unverified whether a checkpoint feature exists elsewhere.

### Area 12. Git and version control

### Managed worktrees
- What: Each agent task gets its own checkout under `<state-dir>/worktrees/<repo-fingerprint>/<name>` on branch `openclaw/<name>`; recorded in the state DB. Before removal, tracked and untracked non-ignored files are snapshotted as a commit under `refs/openclaw/snapshots/<id>` kept 30 days. `.worktreeinclude` copies ignored files; `.openclaw/worktree-setup.sh` runs on create; copy-on-write on APFS/Btrfs/ReFS/XFS. Cap 4096; idle >7 days snapshotted and removed even if dirty. `openclaw worktrees list|gc|restore`.
- Status: GA (page has no maturity label)
- Evidence: https://docs.openclaw.ai/concepts/managed-worktrees
- Notes: Page text was truncated (30k chars unread). (verified in review: path, branch, snapshot ref, 30 days, cap 4096, 7 days dirty; maturity label not on page)

### Pull requests, commits, review
- What: Nothing documented on pages read for built-in commit/PR/review flows. Changes panel shows diffs; cloud workers publish via a Gateway broker.
- Status: unclear
- Evidence: https://docs.openclaw.ai/gateway/cloud-workers/security-model; https://docs.openclaw.ai/web/control-ui
- Notes: See Unverified.

### Area 13. Extensibility

### Plugins
- What: Native plugins (`openclaw.plugin.json`, in-process runtime) add channels, providers, tools, skills, speech. Bundle compatibility maps Codex/Claude/Cursor/Agent Plugin layouts. Install sources: ClawHub (default), npm, git, local, marketplace. `plugins.deny` beats allow; workspace-origin plugins disabled by default; `security.installPolicy` can allow/warn/block. Exclusive slots for `memory` and `contextEngine`. Project says treat plugin installs like running code.
- Status: GA
- Evidence: https://docs.openclaw.ai/tools/plugin; https://docs.openclaw.ai/concepts/experimental-features
- Notes: Docs say plugin APIs are all experimental. Why-OpenClaw page says plugins are not sandboxed and the surface is about 150 public entrypoints.

### Skills
- What: Directory with `SKILL.md` (YAML frontmatter `name`, `description`; optional user-invocable, disable-model-invocation, command-dispatch). Load precedence: workspace, project `.agents/skills`, personal `~/.agents/skills`, managed, workshop (agent-learned), bundled, extra dirs/plugins. Gating by `metadata.openclaw` (`requires.bins/env/config`, `os`). Eligible skills snapshotted per session into a compact list in the prompt; full body loaded on demand; `skills_search`/`skills_read` for omitted ones. Per-agent allowlists limit visibility only. File watcher plus 30-min rescan. Per-skill env injected on host for a run, not into sandbox. Project says to treat third-party skills as untrusted code.
- Status: GA
- Evidence: https://docs.openclaw.ai/tools/skills
- Notes: Skill folder layout is the same family as other harnesses but with different frontmatter keys.

### ClawHub registry
- What: Public registry (clawhub.ai) for skills and plugins; versioned bundles; separate `clawhub` CLI for login/publish/delete; publishing requires a GitHub account meeting an age requirement; automated scans on releases with held/blocked states; reporting and moderation; install telemetry opt-out `CLAWHUB_DISABLE_TELEMETRY=1`.
- Status: GA
- Evidence: https://docs.openclaw.ai/clawhub; https://docs.openclaw.ai/clawhub/security
- Notes: Scan methodology is on /clawhub/security-audits, not opened.

### MCP client and server
- What: `openclaw mcp serve` exposes OpenClaw channel conversations to external MCP clients; `openclaw mcp add/set/login/probe/doctor...` manages saved outbound servers (transports and OAuth, MCP Apps, Control UI editor at `/settings/mcp`); servers can be scoped to the Codex app-server. ACP bridge rejects per-session MCP injection.
- Status: GA
- Evidence: https://docs.openclaw.ai/cli/mcp; https://docs.openclaw.ai/cli/acp
- Notes: none

### Embedding, HTTP APIs and SDK-like surfaces
- What: OpenAI-compatible `/v1/chat/completions`, `/v1/models`, `/v1/embeddings` (off by default); `/v1/responses` separate. `model` selects an agent (`openclaw/<agentId>`); session via `user` field or `x-openclaw-session-key`. Docs say treat as full operator access. Pages for "Building a Gateway client" and "Embedding OpenClaw" exist.
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/openai-http-api; https://docs.openclaw.ai/gateway/protocol
- Notes: Embedding and client-building pages not opened. A Gateway "tools invoke" HTTP API appears in the index but was not confirmed.

### Gateway wire protocol (TypeBox schemas)
- What: JSON over WebSocket; first frame must be `connect`; request/response/event frames; side-effecting methods need idempotency keys; events not replayed; schemas defined in TypeBox with generated JSON Schema and Swift models; N-1 node version window documented.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/architecture; https://docs.openclaw.ai/gateway/protocol
- Notes: none

### Area 14. Models, providers and inference configuration

### Providers and model refs
- What: Model refs are `provider/model` in JSON5 (`agents.defaults.model.primary`). Provider list includes Anthropic (API and Claude CLI), OpenAI (API and Codex), Google, Bedrock, Mistral, xAI, DeepSeek, Groq, Moonshot, Qwen, Z.AI, OpenRouter, LiteLLM, Vercel AI Gateway, Cloudflare AI Gateway, GitHub Copilot, plus local (Ollama, LM Studio, vLLM, SGLang, llama.cpp) and custom OpenAI/Anthropic-compatible endpoints. Subscription OAuth is supported (OpenAI Codex).
- Status: GA
- Evidence: https://docs.openclaw.ai/providers; https://docs.openclaw.ai/concepts/features
- Notes: Providers page does not list auth methods.

### Auth profile rotation and model fallback
- What: First rotate auth profiles within a provider (order: per-agent override, `auth.order`, configured, stored; OAuth before keys; least-recently-used first), then walk `agents.defaults.model.fallbacks`. Profile pinned per session for cache warmth. Rate-limit cooldown follows provider headers; other errors back off 30s, 1m, 5m cap; billing failures disable a credential 10 minutes. A user-chosen model is strict (no masking fallback). After a rate-limit fallback the next turns prefer the working fallback for up to 30 min.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/model-failover
- Notes: none

### Local models and per-person credentials
- What: Local model pages and an experimental `localModelLean` override exist. Multi-user mode lets each person save model credentials tied to their Gateway profile.
- Status: GA / experimental
- Evidence: https://docs.openclaw.ai/concepts/experimental-features; https://docs.openclaw.ai/concepts/multi-user
- Notes: none

### Area 15. UI surfaces

### Control UI (web)
- What: Vite plus Solid SPA served by the Gateway at `http://<host>:18789/` (optional base path), connecting to the same-port WebSocket. Chat with nested child sessions and auto titles, agent selector and team mode, Subagents and background-process panels, Changes (git diff) panel, automation status, Settings (identity, appearance, plugins, updates, MCP, devices), Usage page, Labs flags, Config form generated from schema with raw JSON editor. Keeps drafts during disconnect but does not replay actions. Browser requirements Chrome 147+/Firefox 150+/Safari 26.2+.
- Status: GA
- Evidence: https://docs.openclaw.ai/web/control-ui; https://docs.openclaw.ai/gateway/configuration
- Notes: Token kept in session storage per tab; password in memory only.

### Native companion apps
- What: macOS menu bar app (can act as a node), iOS and Android nodes (pairing, chat, voice, camera, screen, location), Windows Hub (recommended) and a Tauri app, Linux companion. Apple Watch node via signed HTTPS polling.
- Status: GA/unclear per platform; Maturity scorecard rates Windows App/Node M3 Beta with 19% quality, flagged inconsistent.
- Evidence: https://docs.openclaw.ai/platforms; https://docs.openclaw.ai/nodes; https://docs.openclaw.ai/maturity/scorecard
- Notes: Scorecard page truncated (135k chars unread).

### Chat channels as UI
- What: Telegram, WebChat, A2A, Reef in core; official plugins for Discord, Slack, WhatsApp, Signal, iMessage, Matrix, Teams, Google Chat, IRC, LINE, Mattermost, Nostr, Twitch, SMS, Voice Call and more; external plugins for WeChat, Yuanbao, Zalo ClawBot. Threads map to sessions on Discord, Slack, Telegram, Matrix. Ambient room events let an agent read unmentioned messages silently and speak only via the `message` tool. Telegram Mini App and Discord Activities exist.
- Status: GA (per channel varies)
- Evidence: https://docs.openclaw.ai/concepts/features; https://docs.openclaw.ai/channels/ambient-room-events; https://docs.openclaw.ai/llms.txt
- Notes: none

### TUI and CLI
- What: `openclaw tui`, extensive CLI (`doctor`, `security`, `sandbox`, `sessions`, `approvals`, `devices`, `nodes`, `cron`, `skills`, `plugins`, `mcp`, `memory`, `backup`, `update`, `triage`, `policy`).
- Status: GA
- Evidence: https://docs.openclaw.ai/llms.txt (CLI section); https://docs.openclaw.ai/start/getting-started
- Notes: `openclaw policy` described as inspecting and testing tool policy (security page).

### Voice: Talk, voice wake, voice call
- What: Talk mode is a continuous loop (platform speech recognition, send to session, speak via TTS: ElevenLabs, local MLX, system). Realtime variants: OpenAI WebRTC (client-owned), Google, Gateway-relay; Apple Watch standalone. `interruptOnSpeech` default true; silence timeout 700/900 ms. Limits: 8 concurrent GPT-Live sessions, 30-min TTL. Reply may begin with a JSON voice directive line. Voice wake: one Gateway-owned global word list (defaults `openclaw`, `claude`, `computer`), routing rules to session/agent, detection on device. Discord voice channels and a Voice Call plugin exist.
- Status: GA/unclear per surface
- Evidence: https://docs.openclaw.ai/nodes/talk; https://docs.openclaw.ai/nodes/voicewake; https://docs.openclaw.ai/concepts/features
- Notes: Voice-call and Discord voice pages not opened.

### Area 16. Browser automation and computer use

### Browser tool
- What: Dedicated Chrome/Brave/Edge/Chromium profile controlled via a loopback-only control service in the Gateway. `openclaw` profile is isolated; built-in `user` profile attaches to the signed-in Chrome via DevTools MCP. Tabs, click, type, drag, snapshots, screenshots, PDFs, downloads; remote and hosted CDP; strict-by-default SSRF policy (security page). Node browser proxy exists (`browser.proxy` needs admin to approve).
- Status: GA
- Evidence: https://docs.openclaw.ai/tools/browser; https://docs.openclaw.ai/gateway/security; https://docs.openclaw.ai/gateway/operator-scopes
- Notes: Browser subpages not opened.

### Computer use (desktop control)
- What: One `computer` tool, targets gateway desktop or a paired node. Uses `computer.act` and `screen.snapshot`; actions carry a `frameId` and are blocked on stale frames. macOS default provider Peekaboo (CUA alternative); Windows/Linux experimental opt-in `cua-computer` plugin. No per-action confirmation. Screenshots go only to the model. Docs warn on-screen content can carry prompt injection.
- Status: GA on macOS, experimental on Windows/Linux
- Evidence: https://docs.openclaw.ai/nodes/computer-use
- Notes: (verified in review)

### Canvas (hosted widgets on a Mac panel)
- What: The agent's `show_widget` can target a macOS node panel; the document is hosted under `/__openclaw__/canvas/` and the panel is render-only; falls back to inline chat. Admin can disable.
- Status: GA
- Evidence: https://docs.openclaw.ai/platforms/mac/canvas; https://docs.openclaw.ai/concepts/architecture
- Notes: Page did not cover A2UI.

### Camera, screen, location, SMS, device commands
- What: Nodes expose `camera.*`, `screen.record`, `location.get`, `sms.*`, contacts/calendar/reminders, `health.summary` behind opt-in command policy.
- Status: GA
- Evidence: https://docs.openclaw.ai/nodes/command-policy; https://docs.openclaw.ai/concepts/architecture
- Notes: none

### Area 17. Remote, cloud, headless, CI and distributed execution

### Gateway and node topology
- What: One Gateway per host owns channels, sessions, auth, state. Clients (`role: operator`) and nodes (`role: node`) connect over the same WebSocket (default `127.0.0.1:18789`). Nodes declare capabilities and commands and run `node.invoke`; they are peripherals, not Gateway hosts. Headless node host can run foreground, as service, or via SSH tunnel; a macOS app is a single node. One-time setup link `oc-pair://` carries a 10-minute bootstrap token. Exec can be routed to a node. Nodes can host MCP servers and skills and OpenClaw sessions (section titles).
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/architecture; https://docs.openclaw.ai/nodes; https://docs.openclaw.ai/gateway/pairing
- Notes: A Gateway-held model loop plus remote tool execution is the documented `remote-exec` mode.

### Pairing and authentication
- What: Gateway auth (token/password, Tailscale Serve identity, trusted-proxy headers; mode `none` disables). Every client and node sends a device identity and signs the `connect.challenge` nonce; new devices need approval and receive a device token. Loopback is auto-approved by default; LAN/tailnet need explicit approval. Two layers for nodes: device pairing (requests expire in 5 min) and node capability approval (pending ones do not expire). Optional auto-approval: SSH-verified (default on; Gateway SSHes to the node and compares device id and key) and trusted CIDRs; scope or key changes always need explicit approval. The standalone node pairing store was retired from the connect path in January 2026; `node.pair.request`/`node.pair.verify` and their per-node token were removed in 2026.7. Trusted-CIDR auto-approval is off unless `gateway.nodes.pairing.autoApproveCidrs` is set. Rotate with `openclaw devices rotate`.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/architecture; https://docs.openclaw.ai/gateway/pairing
- Notes: Two layers, 5-minute device expiry, non-expiring capability requests, SSH-verified default, 10-minute bootstrap token and loopback auto-approval all confirmed. (corrected in review: per-node token removal dated Jan 2026 for the store and 2026.7 for the RPC methods)

### Remote access options
- What: Tailscale Serve (Gateway stays on loopback), direct LAN/tailnet WebSocket, SSH tunnel, Cloudflare Tunnel plus Access, and "OpenClaw ID" relay (Labs) with six-digit approval. Non-loopback binds require auth; plaintext `ws://` allowed for loopback/private hosts, public must be `wss://`; TLS fingerprint pinning supported. A headerless SSH forward looks like local access.
- Status: GA; OpenClaw ID experimental
- Evidence: https://docs.openclaw.ai/gateway/remote; https://docs.openclaw.ai/concepts/experimental-features
- Notes: (verified in review)

### Cloud workers (Crabbox)
- What: Disposable machines run a session's coding work. `worker-turn` mode runs the agent loop on the box (inference proxied by the Gateway); `remote-exec` (Codex) keeps the loop on the Gateway with shell on the box. The worker dials the Gateway's authenticated public worker route (the page does not use the words "outbound-only" or "WebSocket"; outbound direction is an inference); worker route has a closed method allowlist and cannot call operator RPCs; each dispatch mints a new worker credential and the Gateway stores only its hash; credential rotation plus owner-epoch fencing rejects stale workers; GitHub publication via a Gateway broker; provider keys stay on the Gateway (inference proxied by model reference by default). Placement from a repo URL, managed worktree or empty workspace; warm snapshots; default suggested lease 4h total/45 min idle. Requires Crabbox 0.82.0+. Page says same-OS-user processes can read worker state, and Codex remote exec is not an OS sandbox.
- Status: beta (scorecard: "Containers and cloud execution" M3 Beta, quality Alpha 67%, completeness Beta 71%, no support); page frontmatter `status: active` is not a GA label
- Evidence: https://docs.openclaw.ai/gateway/cloud-workers; https://docs.openclaw.ai/gateway/cloud-workers/security-model; https://docs.openclaw.ai/maturity/scorecard
- Notes: Closest public analogue to the reader's master/node split. 4h/45min is the suggested `cloud` profile, not a universal default (Crabbox default max 4h, policy up to 24h). Warm images Linux only. (corrected in review: status GA -> beta; "outbound-only" is not stated by the page; "desktop viewing is experimental" not found on the pages read)

### Install and hosting
- What: Install script, npm, Docker (`ghcr.io/openclaw/openclaw`, browser variants), Nix, Podman, Kubernetes and many cloud recipes; daemon as LaunchAgent, systemd user unit or Windows Scheduled Task. Container images default to an exposed bind and need auth configured (security page). Gateway lock prevents two Gateways on one state dir; `openclaw doctor --fix` migrates state on upgrade.
- Status: GA
- Evidence: https://docs.openclaw.ai/start/getting-started; https://docs.openclaw.ai/install/docker; https://docs.openclaw.ai/gateway/security
- Notes: Docker page does not clearly state the default bind; security page says container images default exposed.

### Area 18. Collaboration, teams and enterprise administration

### Multi-user mode
- What: Several trusted people operate one agent: session creator (immutable), owner (assignable), participants; presence; mentions inbox; per-person model credentials; personal instructions. Docs state ownership and visibility are usability features, not security boundaries.
- Status: GA (schema 18 shipped in v2026.8.1)
- Evidence: https://docs.openclaw.ai/concepts/multi-user
- Notes: none

### Policy files and OpenClaw policy CLI
- What: `openclaw policy` authors and checks policy files with scoped overlays and attestation; `security.installPolicy` for installs. Details not read.
- Status: unclear
- Evidence: https://docs.openclaw.ai/llms.txt (CLI/policy pages); https://docs.openclaw.ai/gateway/security
- Notes: unverified beyond titles.

### Area 19. Observability

### OpenTelemetry, Prometheus, diagnostics export
- What: `diagnostics-otel` plugin exports metrics, traces, logs over OTLP/HTTP protobuf; W3C `traceparent` on provider calls; exporters attach only when enabled. `diagnostics-prometheus` plugin; Prometheus route behind Gateway auth. Diagnostics export, audit history, `openclaw logs`, `/trace` and `/verbose` (can expose internals).
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/opentelemetry; https://docs.openclaw.ai/install/docker; https://docs.openclaw.ai/gateway/security/prompt-injection
- Notes: Default content-capture privacy not read.

### Usage and cost tracking
- What: Provider-reported quota windows and spend, not estimated billing; `/status`, `/usage off|tokens|full|cost`, `openclaw status --usage`, Control UI Usage page. Subscription sessions hide per-token dollars. Anthropic/OpenAI admin keys add organization cost history.
- Status: GA
- Evidence: https://docs.openclaw.ai/concepts/usage-tracking
- Notes: none

### Area 20. Configuration and customisation

### Config file and hot reload
- What: `~/.openclaw/openclaw.json` (JSON5), schema-validated (unknown keys stop the Gateway), hot reload on edit, editable via CLI, Control UI form/raw, or Config RPC. Guard against accidental data-loss writes. No automatic last-known-good restore except `doctor --fix`. State dir `~/.openclaw` overridable via env vars. Read-only config modes (`OPENCLAW_CONFIG_READONLY`, Nix).
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/configuration; https://docs.openclaw.ai/web/control-ui; https://docs.openclaw.ai/start/getting-started
- Notes: none

### Secrets management (SecretRefs)
- What: Credentials can be SecretRefs resolved from env, file, exec or stores (examples for 1Password, Bitwarden, Vault, pass, sops); shared SQLite secret store with an egress proxy and sentinels injected at egress; startup fails fast and falls back to last-known-good. Plaintext in config/`.env` remains readable by the agent. The `secrets` tool lets the agent obtain a credential without seeing it.
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/secrets; https://docs.openclaw.ai/tools
- Notes: none

### Doctor, triage, update, backup
- What: `openclaw doctor` (with `--fix`, lint and post-upgrade modes, SQLite maintenance, legacy state migration), `openclaw triage` (read-only sanitized diagnosis, can hand off to a coding agent), update run history and rollback, `openclaw backup`.
- Status: GA
- Evidence: https://docs.openclaw.ai/start/getting-started; https://docs.openclaw.ai/llms.txt (CLI section)
- Notes: none

### Area 21. Security and privacy

### Project-stated trust model and risks
- What: One Gateway equals one trust boundary (single operator or mutually trusting team); not a hostile multi-tenant boundary; use separate Gateways, ideally separate OS users or hosts. Anyone who can message a tool-enabled agent shares its tool authority; anyone who can edit `~/.openclaw` is a trusted operator. `sessionKey` is a routing selector, not an auth token. Default `tools.sessions.visibility: all` and agent-to-agent on. Sandboxing and exec approvals are off by default; Why-OpenClaw page calls default OpenClaw a trusted single-operator assistant. Prompt injection alone, without a policy/auth/sandbox bypass, is closed as no-action in the project's triage list. Reports to security@openclaw.ai.
- Status: GA (documented position)
- Evidence: https://docs.openclaw.ai/gateway/security/trust-model; https://docs.openclaw.ai/gateway/security; https://docs.openclaw.ai/start/why-openclaw
- Notes: The standing "no-action" list is a useful list of what the project will not treat as bugs. (verified in review; the trust-model page itself does not say sandbox is off by default, but the sandboxing and Why-OpenClaw pages do)

### Defaults and hardening guidance
- What: Host install binds to loopback; channel DMs use pairing; groups allowlisted, mention-gated. Hardened baseline configs, exposure runbook, rate limiting, incident response (contain, rotate, audit, collect evidence), dependency locking, threat model mapped to MITRE ATLAS (page list only).
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/security
- Notes: Hardened-baselines page not read (URL 404 on one attempt; details unverified).

### Telemetry
- What: Default: daily version check to `telemetry.openclaw.ai` (User-Agent has version, OS, Node, arch; approximate country/region/city recorded by Cloudflare, kept 3 months), update-outcome report, identifier-free install count. Opt-in anonymous feature stats. Never sent: message content, prompts, keys, paths, hostnames. `openclaw telemetry off` and `DO_NOT_TRACK=1` do not stop the daily check; `update.checkOnStart: false` or `OPENCLAW_NO_AUTO_UPDATE=1` do. `OPENCLAW_TELEMETRY_ENDPOINT` redirects.
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/telemetry
- Notes: (verified in review; CI environments send nothing)

### Prompt and secret handling for agents
- What: GitHub identity excluded from sandbox by default; skill env not injected into sandbox; `sessions_history` returns text without secret masking; webhook tokens separate from Gateway token.
- Status: GA
- Evidence: https://docs.openclaw.ai/gateway/sandboxing; https://docs.openclaw.ai/tools/skills; https://docs.openclaw.ai/concepts/session-tool; https://docs.openclaw.ai/automation/cron-jobs/webhooks
- Notes: none

### Area 22. Licence, pricing and distribution

### Licence and governance
- What: MIT; stewarded by the OpenClaw Foundation; no paid tier or hosted service per README; no separate enterprise edition per Why-OpenClaw. Signed releases; release channels incl. `latest`, `main`, `extended-stable` image tags.
- Status: GA
- Evidence: https://github.com/openclaw/openclaw; https://docs.openclaw.ai/start/why-openclaw; https://docs.openclaw.ai/install/docker
- Notes: Funding statements are vendor-claim.

### Area 23. Anything else or unusual

### Personal agent benchmark pack, mantis, QA channels
- What: Pages exist for a personal agent benchmark pack, "Mantis" (Slack desktop runbook) and a QA/e2e automation suite with Slack, WhatsApp and a QA channel.
- Status: unclear
- Evidence: https://docs.openclaw.ai/llms.txt
- Notes: unverified beyond titles.

### Progress drafts, streaming, typing indicators
- What: Chat-channel UX concepts: `progress_card` tool for parent progress, streaming/chunking of long replies, typing indicators on enqueue.
- Status: GA
- Evidence: https://docs.openclaw.ai/tools; https://docs.openclaw.ai/concepts/queue; https://docs.openclaw.ai/concepts/agent-loop
- Notes: none

## 3. Distinctive or unusual

- Gateway-owned wake words and voice routing broadcast to all nodes (https://docs.openclaw.ai/nodes/voicewake): voice is a Gateway feature, not a client one.
- LLM reviewer exec mode (`mode=auto`, session mode "Auto-review") with human escalation after three denials: a middle path between prompting and full trust.
- Approvals bound to argv, cwd, env, pinned executable path (content hash if writable), re-checked before launch: stronger than name-based allowlists.
- Two-layer node pairing: device identity first, then separate approval of the declared command set; `system.run` needs admin to approve; dangerous commands need a persistent config opt-in.
- Cloud-worker design: worker has a closed RPC method list, dials the Gateway (page does not say "outbound-only"), per-dispatch hashed credential, Gateway-held inference and GitHub publication (https://docs.openclaw.ai/gateway/cloud-workers/security-model).
- Managed worktrees that snapshot to a git ref before removal and fall back on `restore` (https://docs.openclaw.ai/concepts/managed-worktrees).
- Dreaming: scored, gated background promotion from daily notes into long-term memory, with a loss-fraction guard on rewrites.
- Standing intents use a model-free FTS prefilter, with cooldown, fire budget and expiry.
- Durable "active writer claim" checked in the same transaction as every transcript write.
- Project publishes its own list of findings it will close as no-action, plus a maturity scorecard with percentages.
- The agent can be hosted by, or can host, other harnesses (Codex app-server, Claude CLI, ACP) as runtimes.
- Idle-time `skills` "workshop" tier of agent-learned skills in the load precedence (page lists it; mechanism not documented on the page read).

## 4. Architecture and UX notes

- Process model: single Gateway daemon (Node), systemd/LaunchAgent/Scheduled Task service, state in SQLite (shared `openclaw.sqlite` plus per-agent DB), PostgreSQL optional. Gateway keeps all channel connections, sessions, cron, approvals and secrets; tools execute in-process on the Gateway, in a sandbox, or on a node.
- Protocol: JSON over WebSocket on one port that also serves the Control UI, canvas documents and HTTP hooks/APIs. Roles `operator` and `node`; scopes; TypeBox schemas.
- Agent loop location: Gateway by default; with cloud `worker-turn` mode the loop runs on the remote box with inference proxied back.
- Trust: Gateway plus paired nodes are one operator trust domain (https://docs.openclaw.ai/gateway/security/trust-model).
- Control UI is a thin client over the same WebSocket, with no separate backend.

## 5. Relevance to a self-hosted master/node harness

- Nodes declare commands, master approves the set per node, and a per-platform allowlist plus a deny override gates each one (Node command policy, Pairing): maps directly to "the node decides" and to keeping `node.toml` the authority. Note OpenClaw's Gateway-side allowlist is the opposite place for authority; the reader's invariant is stricter.
- Exec approvals bind argv/cwd/env/executable and re-check at launch (Exec approvals, advanced): worth copying for the approval path; copy the binding, skip name-glob allowlists alone.
- Defaults of `security: full`, `ask: off`, sandbox off (Exec approvals, Why-OpenClaw) are what the project itself calls a trusted single-operator posture; the reader should default closed and treat OpenClaw's complaints-by-design as a threat model.
- Durable writer claim checked inside the transcript-write transaction (Agent loop) matches the one-writer-per-session invariant; take the idea of failing superseded runs at the storage layer.
- Cloud workers' closed method list, outbound-only connection, per-dispatch hashed credential, Gateway-brokered GitHub publication (Cloud worker security model) is a ready design for worker nodes.
- DM sender pairing and `dmScope` defaults (Session, Access control): the shared-main default is an admitted hazard; the reader's per-owner private chats avoid it.
- Operator scopes plus named roles with a scope ceiling (Operator scopes) are a template for master-side authorization, but OpenClaw states scopes are not tenant isolation.
- Managed worktrees with pre-removal snapshot refs and restore (Managed worktrees) suit coding sessions on nodes.
- Compaction `safeguard` mode that audits summaries for identifiers and pending asks and marks degraded results (Compaction) is a cheap quality control to copy.
- Secrets with egress-time injection of sentinels (Secrets, Cloud workers) fits the reader's "no secret in event/log" invariant.
- Footprint: Node 24+ daemon with SQLite and many plugins; no idle-memory figure documented, so no evidence it meets a 50 MB idle target (unverified).

## 6. Overhyped, stale or contradictory

- Home page and features list still say "heartbeat scheduling", while the heartbeat page says the monitor is retired (https://docs.openclaw.ai/concepts/features vs /gateway/heartbeat).
- Workspace page says `TOOLS.md`/`HEARTBEAT.md` exist in a summary but not in its file map (https://docs.openclaw.ai/concepts/agent-workspace).
- Tool Search appears in "Experimental features" yet is "on by default" and listed as a built-in tool (experimental-features, tools pages).
- Maturity scorecard shows Windows App/Node as M3 Beta with 19% quality, which the page's own bands call Experimental (https://docs.openclaw.ai/maturity/scorecard).
- Why-OpenClaw is a comparison with Hermes Agent written by the project: marketing/positioning, self-reported (`vendor-claim`).
- Why-OpenClaw says a "single-process design relying on the OS" for Hermes: not verified here.
- `telemetry off` and `DO_NOT_TRACK` still leave the daily check on (documented, but easy to misread as "no phone-home").
- Docker page does not state the default bind; security page says containers default to an exposed bind.
- Prompt-injection page cites an attack-success test by model (0.5% to 8.5%) with no primary source; treat as unverified.
- Memory: "dreaming" is default-on yet "promoted memories have no time-based retention limit" (Why-OpenClaw), a privacy gap by the project's own admission.
- Docs are huge and move fast (page-level anchors redirected, schema bumps; heartbeat migration needs schema 30 and older Gateways cannot run it).

## 7. Unverified

- Tool loop-detection thresholds and exec defaults: /gateway/config-tools/built-in-tools not opened.
- Sub-agent depth cap, thread binding and tool-loss list: subpages under /tools/subagents not opened.
- ClawHub scan methodology (/clawhub/security-audits), moderation, namespace claims: not opened.
- Security threat model page (MITRE ATLAS mapping), hardened baselines (one URL returned 404), exposure runbook, rate-limit table: not read.
- Embedding OpenClaw, "Building a Gateway client", Tools invoke HTTP API, OpenResponses API: not opened.
- Checkpoint or file-rewind feature, built-in commit/PR/review flows: nothing found; would be settled by the CHANGELOG at https://github.com/openclaw/openclaw/blob/main/CHANGELOG.md (not opened) and the Control UI feature reference.
- Idle memory footprint of the Gateway: nothing documented on pages read.
- Lobster, Tokenjuice, Honcho, memory-wiki, LanceDB details; Gmail/IMAP/MCP Events triggers; Mantis; Policy CLI details.
- Windows Hub and Tauri app maturity.
- Exact day of v2026.11.2 release (`00-targets.md` says 2026-11-08; releases page gives only November 2026).
- README text after character 100000 and the maturity scorecard after 100000 chars were not read.

## 8. Sources

All fetched 2026-11-10. Title is as given by the fetch summary or page index.

- https://docs.openclaw.ai (home; nav) - identity, docs map
- https://docs.openclaw.ai/llms.txt - full page index (first 100k chars of 225k)
- https://docs.openclaw.ai/concepts/architecture - Gateway architecture
- https://docs.openclaw.ai/gateway/security - Security
- https://docs.openclaw.ai/gateway/security/trust-model - Trust model
- https://docs.openclaw.ai/gateway/security/prompt-injection - Prompt injection
- https://docs.openclaw.ai/gateway/security/audit-checks - Audit checks
- https://docs.openclaw.ai/gateway/security/access-control - Access control
- https://docs.openclaw.ai/gateway/security/hardened-baselines - 404 (not usable)
- https://docs.openclaw.ai/tools/exec-approvals - Exec approvals
- https://docs.openclaw.ai/tools/exec-approvals-advanced - safe bins, forwarding
- https://docs.openclaw.ai/concepts/agent-loop - Agent loop
- https://docs.openclaw.ai/concepts/agent-runtimes - Agent runtimes
- https://docs.openclaw.ai/concepts/session - Session management
- https://docs.openclaw.ai/concepts/memory - Memory overview
- https://docs.openclaw.ai/concepts/dreaming - Dreaming
- https://docs.openclaw.ai/concepts/compaction - Compaction
- https://docs.openclaw.ai/concepts/session-pruning - Session pruning
- https://docs.openclaw.ai/concepts/context-engine - Context engine
- https://docs.openclaw.ai/concepts/system-prompt - System prompt
- https://docs.openclaw.ai/concepts/agent-workspace - Agent workspace
- https://docs.openclaw.ai/concepts/multi-agent - Multi-agent routing
- https://docs.openclaw.ai/concepts/multi-user - Multi-user mode
- https://docs.openclaw.ai/concepts/delegate-architecture - Delegate architecture
- https://docs.openclaw.ai/concepts/standing-intents - Standing intents
- https://docs.openclaw.ai/concepts/queue - Command queue
- https://docs.openclaw.ai/concepts/session-tool - Session tools
- https://docs.openclaw.ai/concepts/model-failover - Model failover
- https://docs.openclaw.ai/concepts/usage-tracking - Usage tracking
- https://docs.openclaw.ai/concepts/features - Features
- https://docs.openclaw.ai/concepts/experimental-features - Experimental features
- https://docs.openclaw.ai/concepts/managed-worktrees - Managed worktrees
- https://docs.openclaw.ai/concepts/storage-locations - Storage locations (backup destinations only; no state layout)
- https://docs.openclaw.ai/tools - Tools
- https://docs.openclaw.ai/tools/subagents - Sub-agents index
- https://docs.openclaw.ai/tools/skills - Skills
- https://docs.openclaw.ai/tools/plugin - Plugins
- https://docs.openclaw.ai/tools/browser - Browser
- https://docs.openclaw.ai/nodes - Nodes
- https://docs.openclaw.ai/nodes/command-policy - Node command policy
- https://docs.openclaw.ai/nodes/computer-use - Computer use
- https://docs.openclaw.ai/nodes/talk - Talk mode
- https://docs.openclaw.ai/nodes/voicewake - Voice wake
- https://docs.openclaw.ai/platforms - Platforms
- https://docs.openclaw.ai/platforms/mac/canvas - Canvas
- https://docs.openclaw.ai/gateway/pairing - Node pairing
- https://docs.openclaw.ai/gateway/operator-scopes - Operator scopes
- https://docs.openclaw.ai/gateway/permission-modes - Session permission modes
- https://docs.openclaw.ai/gateway/sandboxing - Sandboxing
- https://docs.openclaw.ai/gateway/sandbox-vs-tool-policy-vs-elevated - Sandbox vs tool policy vs elevated
- https://docs.openclaw.ai/gateway/openshell - OpenShell
- https://docs.openclaw.ai/gateway/remote - Remote access
- https://docs.openclaw.ai/gateway/protocol - Gateway protocol
- https://docs.openclaw.ai/gateway/cloud-workers - Cloud workers
- https://docs.openclaw.ai/gateway/cloud-workers/security-model - Cloud worker security model
- https://docs.openclaw.ai/gateway/heartbeat - Heartbeat migration
- https://docs.openclaw.ai/gateway/secrets - Secrets management
- https://docs.openclaw.ai/gateway/configuration - Configuration
- https://docs.openclaw.ai/gateway/config-tools - Tools config index
- https://docs.openclaw.ai/gateway/opentelemetry - OpenTelemetry export
- https://docs.openclaw.ai/gateway/openai-http-api - OpenAI chat completions
- https://docs.openclaw.ai/gateway/telemetry - Usage telemetry
- https://docs.openclaw.ai/gateway/cli-backends - CLI backends
- https://docs.openclaw.ai/automation/cron-jobs - Automations
- https://docs.openclaw.ai/automation/cron-jobs/how-it-works - How automations work
- https://docs.openclaw.ai/automation/cron-jobs/webhooks - Inbound webhooks
- https://docs.openclaw.ai/automation/standing-orders - Standing orders
- https://docs.openclaw.ai/automation/hooks - Hooks
- https://docs.openclaw.ai/channels/pairing - Pairing
- https://docs.openclaw.ai/channels/bot-loop-protection - Bot loop protection
- https://docs.openclaw.ai/channels/ambient-room-events - Ambient room events
- https://docs.openclaw.ai/cli/acp - ACP
- https://docs.openclaw.ai/cli/mcp - MCP
- https://docs.openclaw.ai/cli/workboard - Workboard CLI
- https://docs.openclaw.ai/clawhub - ClawHub
- https://docs.openclaw.ai/clawhub/security - ClawHub Security
- https://docs.openclaw.ai/web/control-ui - Control UI
- https://docs.openclaw.ai/install/docker - Docker
- https://docs.openclaw.ai/start/getting-started - Getting started
- https://docs.openclaw.ai/start/why-openclaw - Why OpenClaw
- https://docs.openclaw.ai/releases - Releases
- https://docs.openclaw.ai/maturity/scorecard - Maturity scorecard
- https://github.com/openclaw/openclaw - README (licence, foundation, install, telemetry)

## 9. Review

Reviewed 2026-11-10 by an independent reviewer, public docs only. Checked 28 entries of 89 (each cited page re-fetched and asked for the specific fact; fetch tool summarises, so quotes are as faithful as that summary).

| Entry | Verdict | Reason |
|---|---|---|
| Cloud workers (Crabbox) | confirmed-with-correction, status-wrong | Closed method allowlist, hashed per-dispatch credential, fencing, Gateway broker confirmed; "outbound-only/WebSocket" not stated (page says the worker "dials" the public worker route); status is beta per scorecard, not GA; 4h/45min is the suggested profile |
| Distinctive: cloud-worker design | confirmed-with-correction | same wording fix |
| Exec approvals | confirmed (refined) | modes, defaults, binding and pre-launch re-check, tighten-only confirmed; env binding conditional |
| Exec approvals advanced / safe bins | confirmed | awk/sed/jq denied, /bin and /usr/bin, content hash, stated gap |
| Approval delivery, cron grants | confirmed | standing grant bound to job, command, cwd, env; `grantExpiryDays` unset by default (is on exec-approvals page) |
| LLM reviewer / session permission modes | confirmed | four modes, three consecutive denials escalate, full needs operator.admin, mode change rules |
| Node pairing, two layers | confirmed-with-correction | per-node token removal: store retired Jan 2026, RPC methods removed 2026.7 |
| Node command policy | confirmed | three conditions, opt-in list, deny wins, Linux defaults |
| Default security posture | confirmed | exec `full`/ask `off`; sandbox off; approvals off per Why-OpenClaw |
| Sandboxing | confirmed (partial) | off by default, "not a perfect security boundary", GitHub identity excluded; mode/scope values not on that page |
| Project trust model | confirmed | quotes match; security@openclaw.ai |
| Sub-agents | confirmed | numbers only from queue page; depth defaults not on the index page |
| Per-session writer claim | confirmed-with-correction | commit transaction verifies the claim; state-dir lock not on queue page |
| Embedded loop timeouts | confirmed | 48h, 120s/300s |
| Stuck-run watchdog | confirmed-with-correction | 2-min threshold only with diagnostics on; 5 min is a floor |
| Command queue | confirmed | steer, 500 ms, 20, summarize, 8 |
| Dreaming | confirmed | default-on, 25% guard, 03:00 |
| Compaction | confirmed | numbers match |
| Managed worktrees | confirmed | numbers match (last 30k chars of page unread) |
| Operator scopes / roles | confirmed | |
| Computer use | confirmed | |
| Remote access | confirmed | |
| Telemetry | confirmed | |
| Sessions defaults / incognito / resume | confirmed | |
| Cron | confirmed | |
| Experimental features / Tool Search | confirmed | wording inconsistency is real |
| Maturity scorecard | confirmed | Windows 19% quality labelled M3 Beta; cloud quality Alpha 67% |
| Why-OpenClaw claims | confirmed | ~150 entrypoints, no retention bound, no enterprise edition |

Borrowed-feature findings: none found. Survey generally keeps OpenClaw's own mechanism.

Notable omissions (not checked in depth): subpages under https://docs.openclaw.ai/gateway/sandboxing (modes-scope-and-backend, workspace-access); https://docs.openclaw.ai/tools/subagents/nesting, /tool-policy, /operations; https://docs.openclaw.ai/gateway/security/hardened-baselines (survey saw a 404; not retried); the rest of the scorecard beyond 100k chars. The llms.txt index (225k chars) was not skimmed fully, so up to ten omissions could not be listed reliably.

Trust judgement: the unchecked remainder is likely reliable. Errors found were small wording or status issues, not invented features. Weakest spots are entries the author already marked unverified or page-index-only (other triggers, policy CLI, QA pages), and status labels, which lean toward GA where the project says beta.
