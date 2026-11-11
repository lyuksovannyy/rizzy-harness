# Claude Code (Anthropic): round 1 public-surface survey

Research date: 2026-11-10. Sources: the public docs at code.claude.com (all
228 pages listed in https://code.claude.com/docs/llms.txt, fetched as their
`.md` twins on 2026-11-10), the docs changelog, the weekly "What's new"
digests, three linked Claude Tag pages on claude.com, and the raw GitHub
`CHANGELOG.md`. No product source code was read.

**Method note.** The documented surface is about 11 MB of markdown, so the
pages were split into thirteen slices, each read by a separate agent with the
same instructions as this brief. Their entries were merged here unedited
except for heading tags and expanded URL abbreviations. Because the slices
overlap at the edges, some features appear twice under one area: once from
the CLI pages and once from the SDK, admin or changelog pages. They are kept
when they add different detail. Every Evidence URL is a page that was fetched.
Large reference pages (`settings-reference`, `env-vars`, `errors`, the SDK
TypeScript and Python references, `plugins/troubleshooting`) were read by
structure and targeted sections, not line by line.

## 1. Identity

- **Name / vendor:** Claude Code, by Anthropic. An agentic coding tool. One engine (agent loop, tools, permissions, hooks, skills, plugins, MCP, memory) runs in the terminal CLI, the VS Code and JetBrains extensions, the Desktop app's Code tab, cloud ("web") sessions, and through the Agent SDK (https://code.claude.com/docs/en/platforms.md, https://code.claude.com/docs/en/how-claude-code-works.md).
- **Surfaces covered here:**
  - CLI/TUI, with an optional fullscreen renderer
  - VS Code and JetBrains extensions
  - cloud sessions at claude.ai/code, plus mobile
  - Remote Control, which drives a local session from a browser or phone
  - self-hosted cloud runners
  - Agent SDK (Python, TypeScript) and headless `claude -p`
  - GitHub Actions, GitLab CI/CD, GitHub Enterprise Server
  - managed Code Review and `/code-review` / ultrareview
  - Slack (legacy app) and Claude Tag
  - Chrome integration and CLI computer use (macOS)
  - Claude apps gateway (self-hosted policy and inference proxy, built into the same binary)
- **Desktop, in one line:** the Desktop app's Code tab is the same Claude Code engine with a GUI shell (parallel sessions, panes, scheduled tasks, SSH); it is surveyed separately.
- **Open or closed:** closed product.
  - The product repo `anthropics/claude-code` is public but "All rights reserved". It carries the changelog, plugins and the issue tracker (per `00-targets.md`; not re-checked here).
  - Agent SDK use falls under Anthropic's Commercial Terms (https://code.claude.com/docs/en/agent-sdk/overview.md, License and terms).
  - The docs say the CLI runtime is JavaScript: `/heapdump` writes a JS heap snapshot, and Node/Bun are named (https://code.claude.com/docs/en/commands.md).
- **Version surveyed:** 2.1.322, released 2026-11-10.
  - The docs changelog dates it, and the raw GitHub CHANGELOG.md starts with the same entry (https://code.claude.com/docs/en/changelog.md, https://raw.githubusercontent.com/anthropics/claude-code/refs/heads/main/CHANGELOG.md).
  - Cadence: 21 releases between 2026-10-11 and 2026-11-10, and 26–28 per month since April 2026.
  - Some patch numbers are skipped (for example .297, .298, .313), so version numbers overstate the release count.
  - Major lines: 1.0.0 on 2025-05-22, 2.0.0 on 2025-09-29, 2.1.0 on 2026-01-07.
- **Corrections to `00-targets.md`:**
  - **"Claude Code SDK" is the old name.** The migration guide confirms the rename to Claude Agent SDK: package `@anthropic-ai/claude-code` became `@anthropic-ai/claude-agent-sdk`, and `claude-code-sdk` became `claude-agent-sdk` (https://code.claude.com/docs/en/agent-sdk/migration-guide.md). `00-targets.md` had this as not re-verified.
  - **The TypeScript "V2" SDK interface is gone.** It was a preview and is now titled "(removed)", removed in 0.3.142 (https://code.claude.com/docs/en/agent-sdk/typescript-v2-preview.md).
  - **The Claude Tag docs URL redirects.** https://code.claude.com/docs/en/claude-tag.md serves an HTML render of https://claude.com/docs/claude-tag/overview rather than markdown.
  - **The weekly digest lags.** The "What's new" index stops at Week 37 (September 7–11, 2026), about 47 releases behind the changelog. A Week 38 page returns 404.
  - **"Scheduled tasks" is now the name for cloud routines** (https://code.claude.com/docs/en/routines.md). The same term also covers session-scoped `/loop`/cron and Desktop scheduled tasks.
  - Otherwise the identity section of `00-targets.md` held up.

### Release timeline (from the changelog slice)

The most significant dated changes, as collected from the changelog
(https://code.claude.com/docs/en/changelog.md, section = version):

Notable additions, removals, renames and deprecations, newest first.

| Version | Date | Change |
|---|---|---|
| 2.1.322 | 2026-11-10 | Fast mode on Vertex (global, us, eu) |
| 2.1.321 | 2026-11-09 | Fable 5.2 becomes the default Fable model; mods' own tool calls raise no permission prompt; "Ultra" plan named |
| 2.1.319 | 2026-11-06 | MCP tasks run in the background; `permissions.denyInsteadOfPrompting`; Chrome `shortcuts_*` tools removed |
| 2.1.317 | 2026-11-03 | Read "unchanged file" stub removed; account-email egress check; SDK hook `onFailure: "block"` |
| 2.1.316 | 2026-10-31 | **Deprecated** fast mode on Opus 4.8 (removal 2026-11-30) |
| 2.1.314 | 2026-10-30 | `blockWritesOutsideWorkingDirectories`; idle compaction in VS Code and Desktop |
| 2.1.307 | 2026-10-21 | OTel `safety_event` |
| 2.1.302 | 2026-10-16 | Fork subagents on by default in -p, SDK, Desktop, VS Code; notice that standalone Claude Design closes 2026-12-14 |
| 2.1.300 | 2026-10-14 | **Removed** `--sandbox` / `--no-sandbox` from `remote-control`; hook `updatedInput` validated |
| 2.1.299 | 2026-10-14 | Idle compaction (terminal); mod `Text` `bold` **deprecated** for `weight` |
| 2.1.295 | 2026-10-08 | Command and HTTP hooks `onFailure: "block"`; OSC 7501 |
| 2.1.293 | 2026-10-07 | Haiku 5.5 |
| 2.1.292 | 2026-10-06 | MCP 2026-07-28 negotiation by default (stdio); Agent tool `effort` |
| 2.1.287 | 2026-10-01 | **Claude Mods**; "You should know" mod; 1M context by default on cloud providers |
| 2.1.284 | 2026-09-28 | Sonnet 5.5 |
| 2.1.280 | 2026-09-22 | Opus 5.5 becomes the default Opus model |
| 2.1.277 | 2026-09-18 | AGENTS.md fallback; **Removed** the TaskOutput tool |
| 2.1.269 | 2026-09-11 | `claude plugin eval` |
| 2.1.261 | 2026-09-04 | `keybindingFlavor` made a no-op (added only 2.1.238); `/skill-doctor` |
| 2.1.257 | 2026-09-01 | Fable 5.1; auto mode Containment Escape rule |
| 2.1.248 | 2026-08-27 | `--restricted` mode |
| 2.1.247 | 2026-08-26 | SendFeedback tool |
| 2.1.233 | 2026-08-14 | Todo/Task tools withdrawn on new models; Bash memory cgroup |
| 2.1.232 | 2026-08-13 | Fork subagent on by default (terminal) |
| — | 2026-08-14 | Auto mode becomes the default permission mode for Pro, Max, Team (https://code.claude.com/docs/en/whats-new/2026-w32.md) |
| 2.1.224 | 2026-08-07 | Cross-session messaging; self-hosted environments (public beta); 200-spawn cap **removed** |
| 2.1.222 | 2026-08-04 | **Removed** Ultraplan |
| 2.1.219 | 2026-07-24 | Opus 5; nesting depth 3; Opus 4.7 **removed** from fast mode |
| 2.1.214 | 2026-07-18 | EndConversation tool |
| 2.1.212 | 2026-07-17 | `/fork` redefined, in-session fork **renamed** `/subtask`; Task `mode` param **deprecated** |
| 2.1.208 | 2026-07-14 | Screen reader mode |
| 2.1.198 | 2026-07-01 | Chrome GA; subagents in background by default; **Removed** the `/agents` wizard |
| 2.1.197 | 2026-06-30 | Sonnet 5 |
| 2.1.170 | 2026-06-09 | Fable 5 |
| 2.1.160 | 2026-06-02 | Dynamic-workflow keyword **renamed** `workflow`→`ultracode`; `..._OPUS_4_6_FAST_MODE_OVERRIDE` **removed** |
| 2.1.154 | 2026-05-28 | Dynamic workflows introduced |
| 2.1.147 | 2026-05-21 | `/simplify` **renamed** `/code-review` (cleanup behaviour removed) |
| 2.1.144 | 2026-05-19 | "extra usage" **renamed** "usage credits" (`/extra-usage`→`/usage-credits`) |
| 2.1.139 | 2026-05-11 | Agent view (research preview) |
| 2.1.92 | 2026-04-04 | **Removed** `/tag`, `/vim` |
| 2.1.83 | 2026-03-25 | Auto mode research preview (Week 13); TaskOutput **deprecated** |
| 2.1.77 | 2026-03-17 | `/fork` **renamed** `/branch` |
| 2.1.75 | 2026-03-13 | **Removed** the Windows `C:\ProgramData` managed-settings path (deprecated in 2.1.2) |
| 2.1.73 | 2026-03-11 | `/output-style` **deprecated** (reinstated as a command in 2.1.269) |
| 2.1.68 | 2026-03-04 | Opus 4 and 4.1 **removed** from the first-party API |
| 2.1.15 | 2026-01-21 | npm installation **deprecated** |
| 2.0.70 | 2025-12-15 | `#` memory shortcut **removed** |
| 2.0.62 | 2025-12-09 | `attribution` setting; `includeCoAuthoredBy` **deprecated** |
| 2.0.30 / 2.0.32 | 2025-10-30 / 11-04 | Output styles **deprecated**, then **un-deprecated** 5 days later |
| 2.0.25 | 2025-10-21 | Legacy SDK entrypoint **removed** (move to `@anthropic-ai/claude-agent-sdk`) |
| 2.0.8 | 2025-10-04 | `.claude.json` allowedTools / ignorePatterns / env / todoFeatureEnabled **removed** |
| 1.0.7 | 2025-06-02 | `/allowed-tools` **renamed** `/permissions`; `claude config` **deprecated** |
| 0.2.125 | 2025-05-21 | `DEBUG=true` **removed** for `ANTHROPIC_LOG=debug` |
| 0.2.82 | 2025-04-25 | Tools **renamed** (LSTool→LS, View→Read) |

## 2. Features by area

## Area 1. Agent loop and execution model

### Agentic loop: gather context, act, verify
- What: The loop is described as three blended phases (gather context, take action, verify results) repeated until done; the model picks each next tool based on the previous result and can chain dozens of actions. Claude Code is the "harness" around the model: it supplies tools and manages the context the model sees. The loop is stated to be identical across terminal, desktop, IDE, web, Remote Control, Slack and CI; only where code executes changes.
- Status: GA
- Evidence: https://code.claude.com/docs/en/how-claude-code-works.md (The agentic loop; Environments and interfaces)
- Notes: Three execution environments named: Local (your machine, default), Cloud (Anthropic-managed VMs or self-hosted environments an org operates), Remote Control (your machine, driven from a browser).

### Interrupt and steer mid-turn (Esc and queued messages)
- What: Esc stops Claude immediately, cancelling the running tool call; queued messages are sent next. Typing a correction and pressing Enter without stopping queues the message; if Claude is running tool calls, it reads the message as soon as those calls finish, within the same turn, before its next step.
- Status: GA
- Evidence: https://code.claude.com/docs/en/how-claude-code-works.md (Interrupt and steer)
- Notes: Commands sent while Claude is responding are queued until the turn ends, except a set that runs immediately (`/status`, `/tasks`, `/usage`; dialog commands in fullscreen since v2.1.234). https://code.claude.com/docs/en/commands.md (intro). Messages sent mid-turn get no checkpoint of their own (see checkpoint entry).

### Context Claude Code injects on its own (system reminders)
- What: Besides the user's messages, the harness adds CLAUDE.md contents, output-style instructions, a note when a previously read file changes on disk, and commit/PR attribution lines. `attribution` and `includeGitInstructions: false` remove the git-related parts.
- Status: GA
- Evidence: https://code.claude.com/docs/en/how-claude-code-works.md (Context Claude Code adds on its own)
- Notes: Docs tell users to check system reminders when Claude follows a rule they did not write.

### Message queue semantics while Claude works
- What: Enter while Claude works queues rather than interrupts; queued entries show gray until Claude starts on them. Queued messages are delivered as soon as current tool calls finish within the same turn; remaining ones go out in order after the turn. Queued slash commands and `!` commands run one at a time after the turn. `Ctrl+Enter` / `Ctrl+X Ctrl+S` (v2.1.275+) sends queued messages now: if Claude is running backgroundable work (shell, subagents) that work moves to the background and Claude reads the messages in the same turn; otherwise the turn is interrupted (always interrupted before v2.1.281). `Up` from the first row takes queued items back into the input. Esc interrupts and sends the queue. `/model` applies from the next request in the current turn, `/effort` from the next message, `/fast` from the next turn. A queued message keeps the IDE selection it had when queued.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Queue messages while Claude works; When Claude Code sends what you queued; Take back what you queued)
- Notes: Backgrounding a session (`←`) carries queued messages, but not queued commands or IDE-selection messages (`Cannot open agents` notice).

### Harness terminology: turn, system reminder, verification loop
- What: Glossary defines a turn as one complete response from a user message until Claude finishes, with any number of tool calls; Stop hooks fire at each turn end. System reminders are harness-inserted messages (CLAUDE.md, output-style instructions, hook `additionalContext`, skills list, file-changed notes, attribution lines), wrapped in `<system-reminder>` tags inside a user message or as a separate system-role message on some models; CLAUDE.md and output style are not part of the system prompt. A verification loop (a runnable check Claude iterates against) is described as the prerequisite for `/goal`, unattended runs and workflows.
- Status: GA
- Evidence: https://code.claude.com/docs/en/glossary.md (Turn; System reminder; System prompt; Verification loop)
- Notes: Glossary also states permission rules evaluate deny then ask then allow, first match wins (Permission rule), and settings precedence managed > `--settings` > local > project > user, arrays merging (Settings layers).

### Streaming idle watchdogs
- What: Four independent timers abort a quiet streaming response so dead connections retry instead of hang: first-byte deadline (180 s direct API, 300 s Claude Platform on AWS/Bedrock, plus 1 s per 32 KB of request body; not through gateways); event-level watchdog (300 s, all providers; keep-alive bytes extend it for a limited time); byte-level watchdog (180 s direct, 300 s elsewhere); body idle timeout (5 min, on providers without the byte watchdog). Tunables: `CLAUDE_ENABLE_STREAM_WATCHDOG`, `CLAUDE_ENABLE_BYTE_WATCHDOG`, `CLAUDE_STREAM_IDLE_TIMEOUT_MS` (min 5 min, byte cap 30 min), `CLAUDE_BYTE_STREAM_IDLE_TIMEOUT_MS` (10 s-30 min), `CLAUDE_STREAM_FIRST_BYTE_TIMEOUT_MS`, `API_TIMEOUT_MS`, `API_FORCE_IDLE_TIMEOUT`, `CLAUDE_ENABLE_BYTE_WATCHDOG_BEDROCK`. Aborts are treated as mid-stream failures (retry, keep partial output with an incomplete notice, or end); headless and subagent responses may be prompted to continue.
- Status: GA
- Evidence: https://code.claude.com/docs/en/network-config.md (Streaming idle watchdogs)

### /goal: evaluator-driven multi-turn loop
- What: `/goal <condition>` sets a completion condition (up to 4,000 chars) and starts a turn right away. After each turn the configured small fast model gets the condition plus the conversation and returns one of three verdicts with a reason. "not yet met" starts another turn, using the reason as guidance. "met" clears the goal and records it as achieved. "impossible" clears it and records it as failed. One goal per session. A new goal replaces the old one. `/goal` with no argument shows status: condition, elapsed time, turns evaluated, token spend, last reason. `/goal clear` removes it; aliases are stop/off/reset/none/cancel, and `/clear` also removes it. The evaluator calls no tools, so the condition must be something Claude's own transcript output can show. To bound a run, the docs suggest putting "or stop after N turns" in the condition itself.
- Status: GA
- Evidence: https://code.claude.com/docs/en/goal.md (Use /goal; How evaluation works; Write an effective condition)
- Notes: Built as a session-scoped prompt-based Stop hook. So it follows workspace trust like settings hooks, and it is unavailable under `disableAllHooks` or managed `allowManagedHooksOnly`. If Claude goes several turns with no tool use, the loop stops with a warning and the goal stays set. `ANTHROPIC_DEFAULT_HAIKU_MODEL` changes the evaluator model, but it also changes the `haiku` alias and other background tasks such as title generation. Evaluator tokens are described as typically negligible.

### /goal failure handling, check-ins, resume and headless use
- What: Four errors clear the goal and need you to fix something: an auth failure (only when Claude Code manages its own credentials), exhausted credit balance, a context overflow that auto-compact could not clear, and an unavailable model. Transient errors (overloaded server, dropped connection) retry automatically up to three times, then the goal pauses. Rate limits, usage limits and a hook ending the turn pause it straight away. If a subagent or background shell is still running when a turn ends, that evaluation is skipped. A "check-in" falls due after 30 minutes of background waiting, then after 1 hour, then every 2 hours (the backoff is capped at 4x). In interactive sessions at most three idle check-ins start on their own between user prompts. `CLAUDE_CODE_GOAL_CHECKIN_MINUTES` sets the first interval; 0 turns off both check-ins and retries. Resume restores an active goal on every route, but resets its turn count, timer and token baseline. `claude -p "/goal ..."` runs the whole loop in one invocation. Use `--output-format stream-json --verbose` to see progress.
- Status: GA
- Evidence: https://code.claude.com/docs/en/goal.md (When a turn fails; Background work defers evaluation; Resume with an active goal; Run non-interactively)
- Notes: Retry/pause notices need v2.1.269+. Idle check-ins need v2.1.236+ and were capped from v2.1.246. Check-ins need v2.1.234+. Resume via the picker was fixed in v2.1.239. The page compares /goal with /loop (time-driven) and Stop hooks (settings-scoped, can run scripts).

### Where the agent loop runs, per mode
- What: Three execution placements share one UI (claude.ai/code and the Claude app). Local session: loop and tools on the developer's machine. Local + Remote Control: still entirely local; web/mobile are only a window and the `claude` process must stay running. Cloud session: the loop runs in an Anthropic-managed VM by default, or on a customer runner process when routed to a self-hosted environment. The quickstart has a comparison table: code runs on, where you start it, where you chat from, whether local config is used (cloud: repo only), whether it survives disconnect (cloud yes, local no, RC only while the local process lives), permission modes, network.
- Status: GA (self-hosted: public beta)
- Evidence: https://code.claude.com/docs/en/web-quickstart.md (Compare ways to run Claude Code); https://code.claude.com/docs/en/remote-control.md (intro, Remote Control vs cloud sessions); https://code.claude.com/docs/en/self-hosted-environments.md (How self-hosted environments work)
- Notes: Docs claim Claude Code "behaves the same everywhere"; differences are where it runs and which config is available.

### Cloud session run sequence
- What: Clone repo into a fresh VM, run the environment's setup script (or use the cached snapshot), apply the network access level, Claude works, then pushes a branch at a stopping point. The session stays open after the push; PR creation and further edits happen in the same conversation. Closing the tab doesn't stop it: it finishes the current task then idles. A question left unanswered can be answered later, up to environment expiry.
- Status: GA
- Evidence: https://code.claude.com/docs/en/web-quickstart.md (How sessions run; Session keeps running after closing the tab); https://code.claude.com/docs/en/claude-code-on-the-web.md (Start a cloud session from your terminal)
- Notes: Each task gets its own session and own branch; parallelism = many sessions, no worktree management by the user.

### Runner-child control channel (stdin, fd 3)
- What: The child's stdin is the runner's control channel: token rotations and session-end signals arrive there. fd 3 is a pipe carrying the child's activity signals, used for idle and startup timeouts; fds 4 and 5 reserved from v2.1.309. stderr tail is posted to the session (visible to the user) on non-zero exit. Backgrounding the child with bare `&` severs stdin; the session then fails with 401 when the initial ~30-minute OAuth token expires. System prompts arrive as `--system-prompt-file`/`--append-system-prompt-file` paths (v2.1.281+) to avoid Linux's 128 KB single-argument limit.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-configuration.md (Keep stdin and file descriptor 3 attached; Pass the system prompt flags through)
- Notes: This is the only place the docs describe the runner-to-child protocol shape; message format is not documented.

### Automatic API retries and retry tuning
- What: Transient failures (5xx, overloaded, timeouts before any streamed output, dropped connections, temporary 429 throttles, stalled streams, a context-overflow rejection that can be retried with smaller `max_tokens`, a tool-call announcement without a tool call) are retried up to 10 times with exponential backoff before an error is shown. Mid-response failures are handled case by case (for example at most two quick re-sends after thinking finished but before text or a tool call). Tunables: `CLAUDE_CODE_MAX_RETRIES` (default 10, capped at 15), `CLAUDE_CODE_RETRY_WATCHDOG=1` for unattended runs (retries 429/529 indefinitely, raises other retries to 300, about three hours), `API_TIMEOUT_MS` (default 600000, time to response headers), first-byte deadline `CLAUDE_STREAM_FIRST_BYTE_TIMEOUT_MS`, 529 backoff base/max delays (500 ms / 32000 ms). A gateway spend-limit 429 is not retried.
- Status: GA
- Evidence: https://code.claude.com/docs/en/errors.md (Automatic retries; Tune retry behavior)
- Notes: Many behaviours carry version notes (v2.1.199, v2.1.239, v2.1.284, v2.1.307).

### Agent SDK: Claude Code as a library (process model)
- What: Python and TypeScript libraries that embed Claude Code's agent loop, built-in tools, permissions, sessions, hooks, skills, plugins and context management in your own process. The library runs the Claude Code binary: both SDKs bundle a native Claude Code binary (TS via npm optional dependencies; Python via platform wheels), so most installs need no separate CLI. Fallbacks: Python sdist installs (e.g. ARM64 Windows) find `claude` on `PATH`; TS installs that skip optional deps need `pathToClaudeCodeExecutable`. Other languages are told to drive the CLI as a subprocess with `-p --output-format json`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/overview.md (Compare the Agent SDK to other Claude tools; Capabilities); https://code.claude.com/docs/en/agent-sdk/quickstart.md (Setup, install note); https://code.claude.com/docs/en/agent-sdk/agent-loop.md (intro)
- Notes: Prereqs Node.js 18.14+ or Python 3.10+. Overview positions four products: Agent SDK (you operate the process), CLI, Client SDK (you write the tool loop), Managed Agents (Anthropic-hosted loop, cloud or self-hosted sandbox).

### Agent loop, turns and the five core message types
- What: Loop = prompt -> model -> tool calls executed by the SDK -> results fed back, until a response with no tool calls. Stream yields `SystemMessage` (subtypes `init`, `compact_boundary`, `informational`, `worker_shutting_down`; hook lifecycle messages precede `init`), `AssistantMessage` (one per content block, blocks of one response share a message ID), `UserMessage` (tool results and mid-loop user input), `StreamEvent` (only with partial messages on), and `ResultMessage` (final text, usage, cost, session id, `subtype`). A few trailing system events (e.g. `prompt_suggestion`) can arrive after the result, so consumers must drain the stream. TS wraps the API message in `.message`; Python uses classes and `isinstance`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/agent-loop.md (The loop at a glance; Turns and messages; Message types; Handle messages)
- Notes: In TS each system subtype other than `init` is its own type in the `SDKMessage` union. Also observability events (rate-limit status, task notifications) exist beyond the five.

### Result subtypes, stop_reason and error semantics
- What: `ResultMessage.subtype` is one of `success`, `error_max_turns`, `error_max_budget_usd`, `error_during_execution`, `error_max_structured_output_retries`; `result` text only on `success`. Every subtype carries `total_cost_usd`, `usage`, `num_turns`, `session_id`. `stop_reason` gives `end_turn`/`max_tokens`/`refusal` (null on a crash-synthesized result). A single-shot `query()` yields the error result then raises (the underlying process exits nonzero); a streaming-input session stays alive after an error result except after a crash.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/agent-loop.md (Handle the result); https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode.md (Single Message Input)
- Notes: After a crash cost fields may be zeroed. `usage` covers only the main loop; `modelUsage` is whole-tree.

### Turn and budget caps
- What: `maxTurns` (turns with tool calls plus one for the final text) and `maxBudgetUsd` (estimated spend), both unlimited by default. Budget includes subagent spend; at the cap spawning a subagent fails with `Budget limit reached` and running background subagents are stopped (v2.1.217+). The cap is checked after each response arrives, so spend can overshoot by one response plus in-flight subagent spend. With streaming input, a message queued when max-turns hits starts a new turn with a fresh count; budget accumulates across messages until `/clear`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/agent-loop.md (Turns and budget; Budget headroom)
- Notes: Docs recommend a budget as the production default.

### Parallel tool execution by read-only hint
- What: Read-only tools (`Read`, `Glob`, `Grep`, MCP tools marked read-only) may run concurrently within a turn; mutating tools (`Edit`, `Write`, `Bash`) run sequentially. Custom tools are sequential unless they set the MCP `readOnlyHint` annotation.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/agent-loop.md (Parallel tool execution)
- Notes: Concurrency is opt-in per tool via an annotation, not a global flag.

### Streaming input vs single message input
- What: Two input modes. Streaming input (recommended): `prompt` is an async iterable of `SDKUserMessage`, giving a long-lived session with queued messages, interrupts, image attachments, permission requests and mid-session control methods. Single message: a string prompt, one-shot, continued via `continue`/`resume`; no images, no queueing, no real-time interrupt. Malformed image blocks are replaced with a text note to the model rather than erroring.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode.md (Streaming Input Mode; Single Message Input)
- Notes: Failure modes documented: in TS a throwing generator surfaces as the generic `Claude Code process aborted by user`; in Python a generator exception is logged at debug level and the session silently stalls.

### Interrupts, message queueing and per-message priority (streaming input)
- What: In streaming input, `interrupt()` (TS Query; Python `ClaudeSDKClient`) stops the turn; on CLIs advertising `interrupt_receipt_v1` (v2.1.205+) it resolves with a receipt listing `still_queued` message UUIDs; a raw control request with `cancel_queued: true` (`interrupt_cancel_queued_v1`, v2.1.219+) cancels them instead. Each `SDKUserMessage` can carry `priority` (`next` default: read after current tool calls finish; `later`: new turn after this one; `now`: interrupt, or with `origin: {kind:"human"}` on v2.1.286+ move running shell/subagent/MCP/web calls to the background and read immediately), `shouldQuery: false` (append context without a model call), `client_composed`/`verbatimPrompts` (no `@path` expansion or `/command` dispatch), `isSynthetic`, and paste markers. Close messages may be merged into one turn; results echo `user_message_uuid(s)` so the host can match replies. `backgroundTasks()` is the programmatic Ctrl+B.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (Query object; SDKControlInterruptResponse; SDKUserMessage; user_message_uuid; user_message_uuids; backgroundTasks()); https://code.claude.com/docs/en/agent-sdk/python.md (ClaudeSDKClient, Example - Using interrupts)
- Notes: Python `interrupt()` does not clear the buffer; the interrupted turn's messages and result must be drained first. Interrupted results carry `terminal_reason` `aborted_streaming` or `aborted_tools`. A message a `UserPromptSubmit` hook blocks mid-turn gets no result at all; docs advise a timeout.

### Message provenance (origin) and declared scheduled runs
- What: User-role messages carry `origin` kind `human`, `channel` (MCP channel), `peer` (teammate or cross-session peer, with `verifiedPeerPid` from the kernel socket and sender-asserted `from`/`fromSession`), `task-notification` (subkinds `scheduled-trigger`, `peer-send-message` when server-verified), `coordinator`, `auto-continuation`, `unclassified`. Apps may set only `human` or a declared scheduled run (`CLAUDE_CODE_HOST_SCHEDULED_RUN=1`, framed to the model as an assigned task). Text the user did not write should be sent as `isSynthetic` + `client_composed`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (SDKMessageOrigin; Task-notification subkinds; Declare a scheduled run; Peer origin fields)
- Notes: A typed provenance model that separates human input from injected content, with explicit statements about which fields are forgeable.

### Wire protocol facts: control requests, capabilities, reattach
- What: SDK and CLI exchange newline-delimited JSON over the child's stdin/stdout; permission prompts travel as `control_request`/`control_response` with a `request_id`. The init `SDKSystemMessage` reports tools, MCP status, model, permissionMode, slash commands, skills, plugins (+`plugin_errors`), `claude_code_version` and an open `capabilities` set for feature detection. `reinitialize()` (v2.1.195+) re-sends `initialize` after a transport gap; the response carries `pending_permission_requests` (v2.1.268+), which the SDK redispatches, so callbacks must be idempotent per request ID. A repeated initialize over stdin replaces hooks; to a remote session it does not (`hooks_applied: false`). Results carry `result_index` so gaps reveal lost results, and `terminal_reason` (19 values).
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (Query object; SDKControlInitializeResponse; CanUseTool; SDKSystemMessage; SDKResultMessage); https://code.claude.com/docs/en/agent-sdk/python.md (Transport); https://code.claude.com/docs/en/agent-sdk/hosting.md (The subprocess model)
- Notes: Python exposes an abstract `Transport` (connect, write JSON+newline, read_messages, close, is_ready, end_input) marked a low-level internal API that may change. TS lists `remote transport` as a case where `updateSettings()` rejects. The protocol itself is not documented as a public spec; only these fragments are.

### Startup failure reporting
- What: On known startup refusals the CLI writes an `error_during_execution` result with `startup_failure_reason` (21 values incl. `bypass_root`, `sandbox_required_unavailable`, `config_file_invalid`, `cwd_unavailable`, `session_held_by_background`, `worktree_resume_refused`, org/gateway policy failures) and zeroed totals; `CLAUDE_CODE_STARTUP_FAILURE_RESULTS=1` extends this to every reason, otherwise many failures just exit nonzero with stderr.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (startup_failure_reason)
- Notes: Requires Agent SDK v0.3.274+.

### Unattended retry watchdog
- What: unattended retry mode (`CLAUDE_CODE_RETRY_WATCHDOG`) waits out 429 and 529 errors. Its knobs:
  - `..._MAX_WAIT_MS` caps the total wait (2.1.295).
  - `..._MAX_USAGE_LIMIT_WAIT_MS` ends a request when the usage limit resets too far away (2.1.301).
  - `..._USAGE_LIMIT_RECHECK_MS` notices a limit lifted before its reset (2.1.305).
  - Separately, `CLAUDE_CODE_OVERLOADED_RETRY_BASE_DELAY_MS` / `..._MAX_DELAY_MS` tune the 529 backoff (2.1.292, 2.1.296), and `CLAUDE_CODE_NONSTREAMING_TIMEOUT_RETRIES` caps re-sends of a non-streaming fallback request (2.1.285).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.285, 2.1.292, 2.1.295, 2.1.296, 2.1.301, 2.1.305)
- Notes: env-vars page covers RETRY_WATCHDOG. Related: the streaming idle watchdog is on by default for all providers; it aborts and retries after 5 minutes without stream events (`CLAUDE_ENABLE_STREAM_WATCHDOG=0` turns it off). https://code.claude.com/docs/en/whats-new/2026-w27.md.

### Mid-stream continuation and malformed tool-call retry
- What: in non-interactive sessions (-p, SDK, cloud), a response cut off mid-stream (server error, dropped connection, stall) is continued when the partial output holds text and no tool calls. A malformed tool call is dropped from the retry context.
- Status: GA
- Evidence: https://code.claude.com/docs/en/whats-new/2026-w35.md (Other wins); https://code.claude.com/docs/en/changelog.md (2.1.251)
- Notes: introduced in Week 35 (Aug 24–28, 2026).

### EndConversation tool
- What: Claude can end a session with a highly abusive user or a jailbreak attempt, as claude.ai has done since 2025.
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.214)
- Notes: 2.1.214, July 18, 2026. Also named in the cli-reference, hooks and settings-reference pages.

## Area 2. Built-in tools

### Built-in tool inventory with permission requirement
- What: Full list from the tools reference ("Permission required" = prompts in Manual mode for paths inside the working directory; read tools still prompt outside working/additional dirs; Bash runs a built-in read-only command set without a prompt):
  - Prompt required (Yes): `Artifact`, `Bash`, `Edit`, `EnterWorktree`, `ExitPlanMode`, `Monitor`, `NotebookEdit`, `PowerShell`, `ShareOnboardingGuide`, `Skill`, `WebFetch`, `WebSearch`, `Workflow`, `Write`.
  - No prompt (No): `Agent`, `AskUserQuestion`, `CronCreate`, `CronDelete`, `CronList`, `EndConversation`, `EnterPlanMode`, `ExitWorktree`, `Glob`, `Grep`, `ListAgents`, `ListMcpResourcesTool`, `LSP`, `PushNotification`, `Read`, `ReadMcpResourceTool`, `RemoteTrigger`, `ReportFindings`, `ScheduleWakeup`, `SendFeedback`, `SendMessage`, `SendUserFile`, `SubagentHandback`, `TaskCreate`, `TaskGet`, `TaskList`, `TaskStop`, `TaskUpdate`, `TodoWrite`, `ToolSearch`, `WaitForMcpServers`.
  - Removed: `TaskOutput` (removed in v2.1.277; Claude now reads a background task's output file with `Read`).
- Status: GA (individual tools gated as noted in other entries; PowerShell has a "preview" limitations section)
- Evidence: https://code.claude.com/docs/en/tools-reference.md (tool table)
- Notes: Advisor is a server tool run by the API, not a Claude Code tool, and has no name for rules or matchers. Tool sets vary by provider, platform and settings; users are told to ask Claude or run `/mcp` for exact MCP names (Check which tools are available).

### Tool-name rule format shared across permissions, flags, SDK, skills, hooks
- What: One rule syntax `ToolName(specifier)` is accepted in `permissions.allow/deny`, `/permissions`, `--allowedTools/--disallowedTools`, SDK `allowedTools/disallowedTools`, skill `allowed-tools` frontmatter, and hook `if` conditions. Specifier families: command patterns (`Bash(npm run *)` covers Bash and Monitor; `PowerShell(...)`), path patterns (`Read(...)` covers Read, Grep, Glob, LSP; `Edit(...)` covers Edit, Write, NotebookEdit), `Skill(name *)`, `Agent(Explore)`, `WebFetch(domain:...)`, bare `WebSearch`. Other tools take only a bare name. Hook `matcher` fields use bare names, not the parenthesized form.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Configure tools with permission rules and hooks)
- Notes: An `Edit(...)` allow also grants read on that path; a `Read(...)` deny also blocks Edit and Write there, including creating new files (Write side needs v2.1.228+).

### Agent tool (subagents, teammates, forks)
- What: Spawns a subagent in a separate context window; the parent sees only the final result, not intermediate tool calls. A call with `name` can launch an agent-team teammate; in fork mode it launches a fork inheriting the full conversation. Tool resolution: neither `tools` nor `disallowedTools` set inherits all subagent-available tools; `tools` only = listed; `disallowedTools` only = parent minus listed; both = disallowed wins. `maxTurns` caps turns and marks the result partial; the subagent can be resumed. Launching does not prompt; each subagent tool call is checked against rules. Subagents run in the background by default; background subagents surface permission prompts in the main session naming the subagent, and Esc denies just that call.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Agent tool behavior)
- Notes: A `tools` list where every entry fails to match yields an error rather than a zero-tool agent.

### AskUserQuestion with optional idle auto-continue
- What: Multiple-choice questions with an "Other" row and notes field; free-text answers are relayed with neutral wording. By default questions wait forever; `askUserQuestionTimeout` (`60s`, `5m`, `10m`) closes an unanswered question, submits any selected options, and tells Claude the user may be away. Countdown shown for last 20 s; any key restarts; the timer does not run while the terminal reports focus, in background sessions, in screen-reader mode, or under Remote Control.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (AskUserQuestion tool behavior; Question auto-continue timeout)
- Notes: Permission prompts, including plan approval, never auto-resolve on idle.

### EndConversation tool (model can end an abusive session)
- What: Ends and locks the session after sustained abuse following a warning, or when the user asks and confirms. Afterwards only `/clear`, `/resume`, `/help`, `/exit`, `/feedback` run; resume restores the lock; `-p` resume of an ended session exits 1. It never prompts, PreToolUse hooks do not run for it, and deny rules, `--disallowedTools` and `--tools` cannot remove it while any other tool remains (removed only if deny rules remove everything and also match it).
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (EndConversation tool behavior)
- Notes: Only on Opus 4.8, Sonnet 5, Fable 5 or later; only interactive terminal sessions (not `-p`, SDK, VS Code panel, GitHub Actions, cloud, `--bare`); not on Bedrock, Claude Platform on AWS, Vertex/Agent Platform, Foundry or cloud gateway. Subagents never get it.

### Read tool
- What: Returns file contents with line numbers; absolute paths. Images (PNG, JPEG, GIF, WebP) returned as visual content, downscaled to model limits, refused over 128 MB. PDFs: short ones whole; over 10 pages read via `pages` ranges up to 20 pages per call, rendered with `pdftoppm` (poppler) which must be installed. Notebooks: all cells and outputs; error over 256 KB of cells or token limit; refuse files over 100 MB. No directories (use `ls`). Refuses binary extensions (`.xlsx`, `.parquet`, `.heic`, `.zip`) and suggests a skill or script. Default cap 25,000 tokens per call (`CLAUDE_CODE_FILE_READ_MAX_OUTPUT_TOKENS`) and refuses whole files over 256 KB; oversize whole-file read returns first page with a `PARTIAL view` notice; `allow_large: true` (v2.1.296+) sizes the read against remaining context window instead.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Read tool behavior; Large files)
- Notes: Empty file and offset-past-end return explicit notices rather than empty output.

### Glob and Grep (absent by default on macOS/Linux/WSL)
- What: On macOS, Linux and WSL the default tool set omits Glob and Grep; Claude uses `find` and `grep` via Bash, which run embedded `bfs` and `ugrep` and reach hooks/permissions as Bash calls. They return when named in `--tools`/`--allowedTools` (naming either in `--allowedTools` restores both; a settings allow rule does not), when Bash is removed by deny rule, `--disallowedTools` or `--restricted`, or when a subagent lists them and omits Bash. Glob: standard glob with `**`, sorted by mtime, capped at 100 results with a truncation flag, does not respect `.gitignore` by default (`CLAUDE_CODE_GLOB_NO_IGNORE=false` to respect). Grep: ripgrep syntax, modes `files_with_matches` (default), `content`, `count`; `glob`/`type` filters; `multiline`; `head_limit`/`offset`; respects `.gitignore`; rejected patterns return ripgrep's diagnostic (since v2.1.208; before, falsely `No files found`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Glob tool behavior; Grep tool behavior)
- Notes: Permission is decided before existence checks, so a prompt for a path does not imply it exists. Null bytes in pattern/path are rejected. On Windows both tools are default.

### LSP tool (code intelligence)
- What: Uses a running language server: after each edit it auto-reports type errors and warnings; Claude can also call it for definition, references, type info at position, file symbols, workspace symbol search, implementations, and call hierarchies. Inactive until a code-intelligence plugin for the language is installed; user installs the server binary. Not started in cloud sessions.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (LSP tool behavior)
- Notes: Returns an error result per call when the server cannot start.

### WebFetch (lossy extraction by a second model call)
- What: Fetches a URL, converts HTML to Markdown, then usually runs the caller's extraction prompt in a separate model call and returns only that result (lossy by design; use `curl` for raw). Refuses `localhost` and dotless hostnames; upgrades HTTP to HTTPS; reads up to 100,000 characters per call (v2.1.290+ reports unread remainder); 15-minute response cache (`CLAUDE_CODE_WEBFETCH_CACHE_TTL_MS`); 5-minute download deadline (`CLAUDE_CODE_WEBFETCH_DEADLINE_MS`, 0 = none); cross-host redirects are returned as text rather than followed. User-Agent starts with `Claude-User`; Accept prefers Markdown. Prompts in Manual and acceptEdits modes except for allowed/denied domains and a built-in preapproved docs-domain set; "don't ask again" saves `WebFetch(domain:...)` to `.claude/settings.local.json`. A domain safety check runs regardless of rules. `CLAUDE_CODE_DISABLE_WEB_FETCH=1` (v2.1.285+) disables it; Team/Enterprise sessions withhold it until org policy loads from `api.anthropic.com`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (WebFetch tool behavior; WebFetch availability)
- Notes: Sandboxed commands do not inherit the preapproved domain set, and WebFetch does not read the sandbox allowlist.

### WebSearch
- What: Queries Anthropic's server-side web search, returns titles and URLs only (no page fetch); up to eight backend searches per call; `allowed_domains` or `blocked_domains` (not both). Backend not configurable (add an MCP search server instead). Interactive session cap of 200 calls shared with subagents, refilling about 100/hour (v2.1.290+); `CLAUDE_CODE_MAX_WEB_SEARCHES_PER_SESSION`, `CLAUDE_CODE_WEB_SEARCH_REFILLS_PER_HOUR`; `/clear` resets. In `-p` and SDK the limit is per turn. At the cap, calls do nothing and Claude is told to continue with what it has.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (WebSearch tool behavior; Session search limit)
- Notes: Not on Bedrock; on Vertex/Agent Platform only with Opus 4, Sonnet 4, Haiku 4.5 and later.

### Monitor tool (background event stream into the conversation)
- What: Runs a command in the background and feeds each output line to Claude as an event mid-conversation (tail logs, poll CI/PR, watch dirs), or opens a WebSocket (`ws` input with `url`, optional `protocols`; cannot combine with `command`) where each text message is one event, binary frames become a placeholder line, a message over 1 MiB or a socket close ends the watch. Every watch has a deadline: 5 min default, 30 min max, 10 min in a single-prompt `-p` run; at the deadline Claude gets one notice and may restart it. Uses Bash permission rules; in auto mode, allow rules naming Monitor are set aside so the classifier reviews it. WebSocket URLs to private, link-local, cloud-metadata addresses (including DNS that resolves to one) and `sandbox.network.deniedDomains` are denied; a proxy bypasses the address check. A deny rule on all Bash removes Monitor; an ask rule on all Bash forces a human (or PermissionRequest hook) approval that allow rules, PreToolUse, bypass or auto cannot grant.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Monitor tool; WebSocket source; Addresses and hosts denied; Bash rules that deny or prompt)
- Notes: Windows needs Git Bash; on Bedrock/AWS/Vertex/Foundry or with telemetry/nonessential traffic disabled needs v2.1.310+. Plugins can declare auto-start monitors. Stopping a subagent stops its monitors.

### NotebookEdit
- What: Edits Jupyter notebooks one cell at a time by `cell_id`; modes `replace` (default), `insert` (after target, or at start with no id; needs `cell_type`), `delete`. Refuses non-UTF-8 files. Governed by `Edit(...)` path rules.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (NotebookEdit tool behavior)
- Notes: Notebooks always require a prior Read before Write on every model.

### Task-tracking tools (TaskCreate/Get/List/Update, TodoWrite) are model-gated
- What: Provided by default only on Opus 4.7, Sonnet 4.6, Haiku 4.5 and earlier (v2.1.268+ default set); omitted on newer models and unknown model IDs because newer models track work without a checklist and the tool definitions cost context. Opt in with `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`, naming a tool in `--allowedTools`, or listing them in `--tools`. `TodoWrite` is disabled in favor of the four Task tools; `CLAUDE_CODE_ENABLE_TASKS=0` brings it back. Background and cloud sessions get them on every model.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Task tool availability; Task-tracking tools in subagents and agent teams)
- Notes: Split-pane teammates are separate processes that inherit `--tools` and deny rules but not `--allowedTools` or exported env vars.

### Session-scoped scheduling tools (CronCreate/Delete/List, ScheduleWakeup) and RemoteTrigger
- What: `CronCreate` schedules recurring or one-shot prompts inside the current session; tasks are restored on `--resume`/`--continue` if unexpired. `ScheduleWakeup` is called by Claude at the end of each self-paced `/loop` iteration to choose the next run 1 min to 1 h out, or `stop: true`. `RemoteTrigger` creates/updates/runs/lists cloud scheduled tasks on claude.ai (backs `/schedule`), plan-gated and unavailable on third-party clouds.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (tool table)
- Notes: Pending wakeup appears in `session_crons` in Stop hook input.

### Messaging and delivery tools (SendMessage, ListAgents, SendUserFile, PushNotification, SubagentHandback)
- What: `SendMessage` messages a teammate, a resumed subagent by ID/name, or another Claude Code session locally or remotely (v2.1.224+), with optional `summary` (truncated at 200 chars). `ListAgents` lists reachable agents (only where cross-session messaging is enabled). `SendUserFile` delivers files with `display` render/attach when Remote Control or cloud is connected. `PushNotification` sends a desktop notification, or a phone push via Remote Control/cloud. `SubagentHandback` delivers a local non-fork subagent's final report in auto mode only, reviewed by the classifier before delivery (v2.1.271+), and is force-added even if excluded from `tools`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (tool table; Agent tool behavior)
- Notes: Push and SendUserFile run through Anthropic-hosted infra, so unavailable on Bedrock/Vertex/Foundry.

### Other tools: ToolSearch, WaitForMcpServers, MCP resource tools, Skill, Workflow, ReportFindings, SendFeedback, Artifact, ShareOnboardingGuide, Enter/ExitPlanMode
- What: `ToolSearch` loads deferred tools when tool search is on; `WaitForMcpServers` waits on still-connecting MCP servers only when tool search is off. `ListMcpResourcesTool`/`ReadMcpResourceTool` list/read MCP resources (MCP Apps UI resources excluded). `Skill` runs a skill in the main conversation; `Workflow` runs a dynamic workflow script orchestrating many background subagents. `ReportFindings` renders structured review findings (file, summary, failure scenario, optional category). `SendFeedback` drafts a feedback report queued locally under `~/.claude/feedback/drafts/` (v2.1.238+; max 10 drafts, 30-day expiry, at most three cards per session; nothing sent until the user sends). `Artifact` publishes HTML/Markdown to claude.ai (plan- and login-gated). `ShareOnboardingGuide` uploads `ONBOARDING.md` for `/team-onboarding`. `EnterPlanMode` (no prompt) and `ExitPlanMode` (prompt: presents plan for approval).
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (tool table; SendFeedback tool behavior)
- Notes: SendFeedback is omitted in `-p`, SDK, cloud, third-party providers, gateways with own keys, ZDR and HIPAA orgs.

### Built-in tools exposed to SDK agents
- What: Same tools as Claude Code: `Read`, `Edit`, `Write`, `Glob`, `Grep`, `Bash`, `WebSearch`, `WebFetch`, `ToolSearch`, `Agent`, `Skill`, `AskUserQuestion`, `TaskCreate`, `TaskUpdate` (task tools opt-in on some models). Core tool schemas load every request; MCP and other built-in tools are deferred behind tool search by default.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/agent-loop.md (Built-in tools; What consumes context)
- Notes: Full list lives in tools-reference (other slice).

### Tool set changes (additions and removals)
- What:
  - **Monitor tool** (2.1.98, Apr 9, 2026): streams events from a background script into the conversation. Extended to Bedrock, Vertex, Foundry and telemetry-off sessions in 2.1.310.
  - **TaskOutput**: deprecated in 2.1.83 (Mar 25, 2026), removed in 2.1.277 (Sep 18, 2026). Claude now uses Read on the task's output file; `taskOutputMaxChars` and `TASK_MAX_OUTPUT_LENGTH` became no-ops.
  - **Todo/task tools** (TaskCreate/Get/Update/List, TodoWrite): withdrawn on Opus 4.8, Sonnet 5, Fable 5, Mythos 5 and newer in 2.1.233 (Aug 14, 2026). `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` brings them back.
  - **SendFeedback** (2.1.247): Claude drafts a feedback report that you review and send.
  - **SendMessage / ListAgents**: cross-session messaging (2.1.224).
  - **EndConversation** (2.1.214).
  - **Read**: `allow_large` option (2.1.296). The "file unchanged since last Read" stub was removed in 2.1.317, so re-reading returns the content.
  - **Claude in Chrome**: `shortcuts_list` / `shortcuts_execute` removed (2.1.319).
- Status: GA (changes as listed)
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.98, 2.1.83, 2.1.277, 2.1.233, 2.1.247, 2.1.224, 2.1.214, 2.1.296, 2.1.317, 2.1.319, 2.1.310); https://code.claude.com/docs/en/whats-new/2026-w33.md
- Notes: tools-reference.md, agent-sdk_python.md and agent-sdk_typescript.md all record the TaskOutput removal.

### Session-wide tool caps
- What:
  - WebSearch is capped at 200 calls per session (`CLAUDE_CODE_MAX_WEB_SEARCHES_PER_SESSION`, 2.1.212).
  - WebFetch has a 5-minute download deadline (`CLAUDE_CODE_WEBFETCH_DEADLINE_MS`, 0 removes it; Week 37) and a 15-minute session URL cache (TTL via `CLAUDE_CODE_WEBFETCH_CACHE_TTL_MS`, 2.1.233). `CLAUDE_CODE_DISABLE_WEB_FETCH` turns WebFetch off (2.1.285).
  - Tool results saved to disk are capped at 1 GB (2.1.265).
  - MCP tool descriptions and server instructions are capped at 2,048 characters (`CLAUDE_CODE_MAX_MCP_DESCRIPTION_LENGTH`, 2.1.280).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.212, 2.1.233, 2.1.265, 2.1.280, 2.1.285); https://code.claude.com/docs/en/whats-new/2026-w37.md
- Notes: cheap runaway-loop protections worth copying.

## Area 3. Code editing and file operations

### Edit tool: exact string replacement with read-before-edit
- What: Replaces `old_string` with `new_string`; no regex or fuzzy matching. Checks: Read-deny paths refused first; file must have been read this conversation (a `PARTIAL view` read does not count); `old_string` must match exactly and be unique, or `replace_all: true`. Opus 4.6, Haiku 4.5 and older always require the read; newer models may edit unread files when reading would not need a prompt and Read is available. A file changed on disk since the last read can still be edited if `old_string` matches current content unambiguously and it can be read without a prompt; the result flags the external change. Viewing a single file via `cat`, `nl`, `bat`, `head`, `tail`, `sed -n 'X,Yp'`, `grep`, `rg` (no pipes/redirects) counts as a read and also loads subdirectory CLAUDE.md and path-scoped rules.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Edit tool behavior)
- Notes: Before v2.1.228 Write always required a read for existing files.

### Non-UTF-8 file guard (Edit, NotebookEdit, Write)
- What: Edit and NotebookEdit refuse files that are not valid UTF-8 (except Edit on UTF-16LE with BOM), because saving back as UTF-8 would replace undecodable bytes with U+FFFD; the error steers Claude to an encoding-preserving shell command or to ask. Write can still replace such a file unless the new content contains U+FFFD (guard against writing back garbled text it read), and saves as UTF-8.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Non-UTF-8 files)
- Notes: Covers legacy encodings (Windows-1252, Shift-JIS), binaries, invalid UTF-8.

### Write tool
- What: Creates or fully overwrites a file (no append/merge). Read-before-overwrite: required always on Opus 4.6/Haiku 4.5 and older; newer models may overwrite unread files under the read-before-edit conditions; notebooks and partially read files always require a read. New files need no read. Partial changes should use Edit.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Write tool behavior)
- Notes: Bash viewing satisfies the read requirement, as for Edit.

### /diff panel and dialog with per-turn views
- What: `/diff` shows working-tree changes including Claude's edits. In fullscreen it opens a live side panel (git repo, at least 110 columns, v2.1.287+; auto-opens when Claude edits and the terminal is at least 144 columns) with a file list and line counts, `ask` button to attach a file's diff to the next prompt, a `source` picker for per-turn edits (`T1`, `T2`, ...) or `Current`, hidden test/generated files, and `Ctrl+X B` to cycle comparison base (session changes, uncommitted, since branch split from default) remembered per repo. Classic renderer opens a dialog against `HEAD`. Turn views come from Claude's file-tool edits, not git, so shell-made changes show only under `Current`. Submodules appear as one entry. During merge/rebase the panel freezes until the operation ends.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Review changes with /diff; Diff panel; Diff dialog)
- Notes: Implemented by built-in mod `cc-plugin-diff`; disabling it falls back to an older viewer.

### Session scratchpad directory
- What: A per-session temp directory (`/tmp/claude-<uid>/<project>/<session-id>/scratchpad/` on Linux, `/private/tmp/...` on macOS, `%TEMP%\claude\...` on Windows, or under `CLAUDE_CODE_TMPDIR`) where Claude can create, edit and read files without permission prompts instead of using `/tmp`; path passed to hooks as `scratchpad_dir`. Deleted with the transcript by the sweep; not touched by `claude purge`; may be cleared by the OS.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-directory.md (Session scratchpad directory)
- Notes: Only present with a claude.ai sign-in on the Anthropic API and `enableArtifact` not false.

### LSP servers / code intelligence via plugins
- What: `.lsp.json` maps server name to a strict config: `command` (binary from user's PATH; plugin does not install it), `extensionToLanguage` (required), `args`, `transport` (`socket` accepted but everything runs over stdio), `env`, `initializationOptions`, `settings` (sent via `workspace/didChangeConfiguration`), `workspaceFolder`, `startupTimeout`, `shutdownTimeout`, `requestTimeout` (default 60 s, v2.1.288+), `restartOnCrash` (default true), `maxRestarts`, `diagnostics` (push diagnostics into context after edits, default true). One server per extension (first registered wins, warning in Errors tab). stdout is protocol-only: headers up to 64 KiB, body up to 32 MiB; violations disconnect and count as a crash. Any invalid entry skips the whole `.lsp.json`; `claude plugin validate` does not read the file.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/components.md (LSP servers); https://code.claude.com/docs/en/plugins/manifest-reference.md (lspServers)
- Notes: See the code-intelligence entry for official LSP plugins and runtime behaviour.

### Code intelligence (official LSP plugins, `LSP` tool, diagnostics)
- What: Official plugins connect language servers for C/C++ (clangd), C#, Go (gopls), Java (jdtls), Kotlin, Liquid (Shopify-maintained), Lua, PHP (intelephense), Python (pyright), Ruby, Rust (rust-analyzer), Swift (sourcekit-lsp), TypeScript/JS; the binary is installed separately. The server starts lazily the first time Claude edits a matching file. Claude gets diagnostics after each edit/write (summarised as `Found N new diagnostic issues in M files`, Ctrl+O to expand) and a read-only `LSP` tool for symbol navigation. An "LSP plugin recommendation" dialog appears at most once per session when a matching binary is on PATH but the plugin is not installed (official marketplace preferred); 30 s timeout counts as ignored; after 5 ignores recommendations stop (state in `~/.claude.json`: `lspRecommendationDisabled`, `lspRecommendationIgnoredCount`, `lspRecommendationNeverPlugins`).
- Status: GA (terminal sessions only; cloud sessions do not start plugin language servers)
- Evidence: https://code.claude.com/docs/en/plugins/code-intelligence.md (Install a code intelligence plugin; See what Claude gains; Accept or dismiss the recommendation dialog); https://code.claude.com/docs/en/plugins/troubleshooting.md (Language server uses too much memory)
- Notes: Docs suggest disabling memory-heavy servers (rust-analyzer, pyright) and relying on built-in search. An LSP plugin counts as "used" when it delivers diagnostics.

### VS Code inline diff review with per-change accept/reject
- What: In Manual mode, a proposed edit opens as a side-by-side diff with a permission prompt (accept, reject, or tell Claude what to do instead). If the user edits the proposed content in the diff before accepting, Claude is told the file was modified. Per-change "Accept this change" and "Reject this change" buttons arrived in v2.1.275, also available as Command Palette commands at the cursor. Diffs with more than 100 changes are reviewed only as a whole file. The `autosave` setting (default on) saves files before Claude reads or writes them.
- Status: GA
- Evidence: https://code.claude.com/docs/en/vs-code.md (Review changes; Extension settings)

### Write without prior Read; Bash edit diffs
- What: on newer models, Write may overwrite an existing file without a Read in the same session, matching Edit's rules (older models still need the read). With `bashEditDiffEnabled`, the Bash tool result includes a diff of the files the command changed.
- Status: GA
- Evidence: https://code.claude.com/docs/en/whats-new/2026-w33.md (Other wins); https://code.claude.com/docs/en/changelog.md (2.1.269)
- Notes: Week 33 (Aug 2026) and 2.1.269 (Sep 11, 2026).

### Write-confinement setting
- What: `permissions.blockWritesOutsideWorkingDirectories` makes Write, Edit and NotebookEdit refuse paths outside the working directories in every permission mode. It complements `blockReadsOutsideWorkingDirectories`, which is offered by a one-time prompt in auto mode.
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.314, 2.1.257); https://code.claude.com/docs/en/whats-new/2026-w36.md
- Notes: 2.1.314, Oct 30, 2026. Documented in settings.md, errors.md and permission-modes.md.

## Area 4. Terminal and shell integration

### Bash tool execution model
- What: Each command runs in a separate process. `cd` in the main session carries over only while inside the project or added directories; leaving them resets to the project dir and appends `Shell cwd was reset to <dir>`; subagents never carry cwd; `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR=1` disables carry-over. Env vars do not persist; aliases, functions and shell options are captured once at session start by sourcing `~/.zshrc`/`~/.bashrc`/`~/.profile` and applied to every command. Persistent env via `CLAUDE_ENV_FILE` or a SessionStart hook.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Bash tool behavior; What persists between commands)
- Notes: Users are told to activate venv/conda before launching.

### Bash timeouts, output limits and benign exit codes
- What: Model sets per-call `timeout`; `BASH_DEFAULT_TIMEOUT_MS` (2 min default) and `BASH_MAX_TIMEOUT_MS` (10 min default ceiling). Output streams to a working file; killed past 5 GB. Valid result: inline up to about 30,000 chars, otherwise a file path in the session directory (truncated past 64 MiB) plus a 2,000-char preview. Failure: inline up to about 10,000 chars, otherwise a head-and-tail excerpt, no file path. `BASH_MAX_OUTPUT_LENGTH` sets read-back window (30,000 default, 150,000 ceiling); `bashOutputMaxChars` setting (up to 128,000, v2.1.261+) sizes inline ceiling and window together. Exit 1 is treated as benign only for `grep`, `rg`, `egrep`, `fgrep`, `find`, `diff`, `test`, `[`, `git diff`, `git grep`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Timeout and output limits; Output limits)
- Notes: Other benign exit-1 commands (`pgrep`, `jq -e`, `cmp`) are reported as failures.

### Background Bash commands and auto-backgrounding on timeout
- What: Claude sets `run_in_background: true` for dev servers and watchers; `/tasks` lists and stops them. A foreground command that hits its timeout is moved to the background rather than killed (unless it starts with `sleep`), and the result says so with task ID and output-file path; `cd` inside a moved command never carries over and the result states the cwd is unchanged. Commands from a foreground subagent stop when that subagent ends; from the main conversation or a background subagent they outlive the final response. `-p` runs stay open after the result until main-conversation background commands exit. Unattended sessions (`-p`, SDK, CI, cloud) impose a time limit (v2.1.285+; all sessions before v2.1.288): 30 min default or Claude's `timeout` up to 2 h; 10 min in a text-prompt `-p` run; raise via the two timeout env vars (cannot shorten). Interactive local sessions have no limit. `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` or bare mode disables auto-backgrounding.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Background commands; When a background command stops; Time limit; Foreground commands that move to the background)
- Notes: Interactive keyboard shortcut to background a running command is covered in the interactive-mode entry.

### Tool memory cap via cgroup (Linux/WSL)
- What: `CLAUDE_CODE_TOOL_MEMORY_LIMIT` (e.g. `4G`, v2.1.233+) caps memory for all of a session's Bash, PowerShell and Monitor commands together via a memory cgroup; if the cgroup cannot be created, commands run uncapped and `--debug` says why; result held until relaunch; OOM kills are not attributed in the tool result. `CLAUDE_CODE_TOOL_MEMORY_CGROUP_EXCLUDE` (v2.1.246+) chooses which other process kinds are capped (`mcp`, `lsp`, `hooks`, `plugin`, `helper`, `agent`; `none`, `all-new`); when unset, the set comes from server-delivered config. Permission-gating hooks and MCP servers they call are always exempt so an OOM kill cannot turn a block into an allow.
- Status: GA
- Evidence: https://code.claude.com/docs/en/tools-reference.md (Memory limit on Linux and WSL)
- Notes: Monitor commands ran outside the cap before v2.1.246.

### PowerShell tool
- What: Native PowerShell execution. Windows without Git Bash: auto-on; with Git Bash: on by default for claude.ai/Console accounts, opt-in via `CLAUDE_CODE_USE_POWERSHELL_TOOL=1` on Bedrock/Vertex/Foundry; Linux/macOS/WSL: opt-in, needs `pwsh` 7+. Spawns with `-ExecutionPolicy Bypass` at process scope (does not override Group Policy); `CLAUDE_CODE_POWERSHELL_RESPECT_EXECUTION_POLICY=1` to respect it. A Bash deny rule (even scoped) on Windows with Git Bash silently turns PowerShell off unless PowerShell is explicitly enabled or has its own scoped rule, to stop bypass of Bash rules. `defaultShell: powershell` routes `!` commands; hooks and skills have `shell: powershell`.
- Status: preview (page has a "Preview limitations" section: profiles not loaded; no sandboxing on Windows)
- Evidence: https://code.claude.com/docs/en/tools-reference.md (PowerShell tool; Bash deny rules also turn off the PowerShell tool; Preview limitations)
- Notes: Windows-specific encoding fixes listed (UTF-8 redirection on 5.1, stdin EOF instead of hang, robocopy exit 1-7 benign).

### Shell mode with `!` prefix
- What: `! cmd` runs a command directly without Claude approving it, streams output, adds command and output to context, and Claude responds automatically (costs a normal prompt; `respondToBashCommands: false` adds output without a reply). Supports `Ctrl+B` backgrounding, history-based Tab completion of earlier `!` commands per project, and live path completion on tokens containing `/`. Pasting text starting with `!` into an empty prompt enters shell mode.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Shell mode with ! prefix)
- Notes: User `!` commands run outside the sandbox even when sandboxing is on, except in strict sandbox mode sessions. `defaultShell: powershell` routes them to PowerShell (tools-reference).

### Background Bash: interactive controls and cleanup
- What: Background tasks get an ID; output goes to a file Claude reads with Read. `Ctrl+B` moves a running Bash invocation (or agent) to the background (twice under tmux). Tasks are cleaned up at exit; on macOS/Linux, stopping a task from `/tasks` or at exit also kills processes that detached from its shell (`setsid`, `timeout`). Backgrounding the session keeps tasks running there. Killed above 5 GB output. On macOS/Linux, running background tasks are stopped under OS critical memory pressure if the session has been idle at least 30 minutes with no turn or subagent running (`CLAUDE_CODE_DISABLE_BG_SHELL_PRESSURE_REAP=1` to disable). `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` disables backgrounding, `Ctrl+B` and background skills.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Background Bash commands; How backgrounding works)
- Notes: `Ctrl+X Ctrl+K` (press twice within 3 s) stops all running background subagents (General controls).

### Egress proxy authentication via runner loopback listener
- What: `--proxy-authorization-command` or `--proxy-authorization-file` (v2.1.238+): runner starts a forward proxy on 127.0.0.1, rewrites `HTTPS_PROXY`/`HTTP_PROXY` for itself, hooks and sessions to point at it, and adds a freshly read `Proxy-Authorization` header on every upstream connection (rotation without restart). Sessions get `ALL_PROXY` removed and `NO_PROXY` pinned. Refuses to start if both flags, no proxy URL, or used on the orchestrator. Failure answers 502 and logs why; header never logged.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Authenticate to an egress proxy)
- Notes: Sessions can only reach the proxy through the listener.

### Shell output and resource limits
- What:
  - `bashOutputMaxChars` (up to 128,000) raises how much command output Claude gets inline before it spills to a file (2.1.261).
  - `CLAUDE_CODE_TOOL_MEMORY_LIMIT` (Linux/WSL) puts Bash and PowerShell tool commands in a memory cgroup (2.1.233).
  - The Bash shell clears inherited `which` aliases and functions (2.1.314).
  - Background commands started by subagents lost their one-hour limit (2.1.260).
- Status: GA (the cgroup limit is opt-in)
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.261, 2.1.233, 2.1.314, 2.1.260); https://code.claude.com/docs/en/whats-new/2026-w33.md, https://code.claude.com/docs/en/whats-new/2026-w36.md
- Notes: Windows without Git Bash falls back to the PowerShell tool (Week 18, April 27 – May 1, 2026).

### Program Status Protocol (OSC 7501)
- What: terminals that implement OSC 7501 can show whether Claude Code is working, waiting on the user, or done.
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.295)
- Notes: Oct 8, 2026. Documented only in terminal-config.md.

## Area 5. Permissions, approvals and sandboxing

### Permission and tool-restriction CLI flags
- What: `--permission-mode default|manual|acceptEdits|plan|auto|dontAsk|bypassPermissions` (overrides `defaultMode`); `--dangerously-skip-permissions` (= bypassPermissions; persists across supervisor restarts for `--bg` sessions); `--allow-dangerously-skip-permissions` (adds bypass to the Shift+Tab cycle without starting in it). `--allowedTools` (no-prompt rules, except network-path reads), `--disallowedTools` (bare name removes the tool from context; `"*"` all, `"mcp__*"` all MCP; scoped rule only denies matching calls), `--tools` (restrict built-ins; `""` none, `"default"`; does not affect MCP tools). `--permission-prompt-tool mcp__server__tool` delegates prompts in `-p` to an MCP tool (waits up to `MCP_TIMEOUT`, 30 s; cannot approve tools marked as requiring user interaction, v2.1.199+). `--permission-prompts host|none` (v2.1.259+) denies prompts when nobody can answer. `--restricted` (v2.1.248+) removes command/code-running tools and WebFetch unless named individually, confines file tools to working dirs, loads only managed and `--settings` settings, refuses bypass and cloud sessions; intended for evaluation harnesses on shared machines. `--safe-mode` disables all customizations but keeps auth, models, built-in tools and permissions (managed policy still applies).
- Status: GA
- Evidence: https://code.claude.com/docs/en/cli-reference.md (CLI flags)
- Notes: `--enable-auto-mode` removed in v2.1.111. `--setting-sources user,project,local` limits loaded sources; `--settings` file must be a regular file of at most 2 MiB.

### Permission modes as surfaced on core pages
- What: `Shift+Tab` cycles Manual (`default`), `acceptEdits` (edits plus common filesystem commands like `mkdir`, `mv` without asking), `plan` (explore and propose without editing source), `bypassPermissions` (when enabled at launch) and `auto` (a separate classifier model reviews actions; tool results are stripped from what it sees). `dontAsk` refuses anything that would prompt (recommended with `--allowedTools` in scripts). Auto is the built-in starting mode for interactive terminal and VS Code sessions on v2.1.283+ (earlier, only on Pro/Max/Ultra/Team). Deny and ask rules and PreToolUse hooks apply before the classifier; deny rules hold in every mode including auto and bypass. Auto mode pauses back to asking after three consecutive or twenty total blocks (in `-p` a block denies that action and the run continues). Boundaries stated in conversation ("don't push until I've reviewed") are treated as block signals by the classifier but can be lost to compaction.
- Status: GA
- Evidence: https://code.claude.com/docs/en/how-claude-code-works.md (Control what Claude can do); https://code.claude.com/docs/en/best-practices.md (Change the permission mode; Limit access with permission rules and the sandbox; Run Claude from a script); https://code.claude.com/docs/en/glossary.md (Auto mode; Permission mode)
- Notes: Full permission semantics are another slice's pages; recorded here only as stated on these pages. `autoMode` config is honored only from user settings, not project settings, and entries replace built-in lists unless `"$defaults"` is kept.

### `@` file mentions bypass PreToolUse hooks
- What: `@path` inlines a file (up to the Read token limit, 25,000 tokens; text files over 256 KB excluded) or a directory listing into the message without a tool call, so PreToolUse hooks do not run; `@` also adds CLAUDE.md from that file's directory and parents. Read deny rules still block inclusion. `CLAUDE_CODE_DISABLE_FILE_MENTIONS=1` (v2.1.299+, ignored in project and local settings) sends only paths with a note to read them via tools, so hooks run. `@server:resource` pulls MCP resources.
- Status: GA
- Evidence: https://code.claude.com/docs/en/common-workflows.md (Reference files and directories)
- Notes: Directly relevant to hook-based read policies.

### Tiered approval model per tool type
- What: Built-in tiers decide what prompts in Manual (default) mode: reads/Grep need no approval inside the working and additional directories; Bash prompts except a built-in read-only set; file edits prompt; WebFetch prompts except a preapproved documentation-domain list; WebSearch prompts. "Yes, and don't ask again" persists differently per tier: Bash per repository and command, WebFetch per repository and domain, WebSearch per repository, file edits only until session end.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Permission system)
- Notes: Permanent approvals are written to `.claude/settings.local.json` at the git repository root, resolved through worktrees to the main checkout, so they apply in subdirectories and worktrees. Outside a repo or on Windows a different location is used. "Don't ask again" options are offered only when the prompt can show everything the saved rule would allow; otherwise only a one-time approval is offered (e.g. exec wrappers like `watch`, `find -delete`).

### Permission prompt UX: comments on approve/deny, live /permissions editing
- What: On most prompts (Bash, PowerShell, file, MCP) the user can press Tab on Yes/No to attach a comment. On Yes the comment is sent to Claude after the tool result; on No it is sent as the denial reason and Claude keeps working. Choosing No with no comment in the main conversation stops the turn. Prompts also offer "Yes, and switch to auto mode" (not on every prompt). `/permissions` lists every rule with its source settings file; edits apply from the next tool call in the same turn (v2.1.234+; before that queued to end of turn).
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Add a comment when you answer a permission prompt; Manage permissions)
- Notes: WebFetch and browser prompts have no comment field; session-wide or rule-saving options take no comment. A v2.1.235 fix: Shift+Tab inside the field used to approve for the session and drop the comment.

### Rule evaluation order: deny > ask > allow, first match wins
- What: Rules are `allow`, `ask`, `deny`. Evaluation is deny, then ask, then allow; specificity never reorders, so a broad deny beats a narrow allow and an ask beats a more specific allow. A bare tool name in deny (e.g. `Bash`, or `Bash(*)`) removes the tool from the model's context entirely; a scoped deny (e.g. `Bash(rm *)`) keeps the tool and blocks matching calls. Rules are enforced by the harness, not the model.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Manage permissions; Match all uses of a tool)
- Notes: Exception: the `EndConversation` tool cannot be removed by deny while any other tool remains and is never prompted by ask. Tool labels in the UI (e.g. "Stop Task") differ from canonical names (`TaskStop`); rules match canonical names only and deny/ask rules naming no known tool produce a startup warning.

### Rule syntax: `Tool`, `Tool(specifier)`, parameter matching, tool-name globs
- What: Format `Tool` or `Tool(specifier)`; parentheses inside are literal. Deny and ask rules (not allow) can match any top-level scalar input parameter of a built-in tool with `Tool(param:value)`, e.g. `Agent(model:opus)`, `Bash(run_in_background:true)`; `*` wildcards in the value; omitted params never match; literal pre-normalization comparison. Primary content fields (`command`, `file_path`, `path`, `url`, `notebook_path`) cannot be matched this way because compound commands would bypass it; such rules are ignored with a warning. Deny/ask rules accept full-name globs in the tool position (`*`, `mcp__*`); allow globs only after a literal `mcp__<server>__` prefix, and unanchored allow globs are skipped.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Permission rule syntax; Match by input parameter; Tool name wildcards)
- Notes: Allow rules deliberately do not support param matching because one param value does not prove a call safe. MCP param rules only via `--disallowedTools`; parenthesized `mcp__` rules in settings files are skipped and reported.

### Bash rule matching: wildcards, compound commands, wrappers
- What: Bash rules match the whole command text; `*` matches any text including spaces, anywhere in the rule. A trailing ` *` (space then star, as the only wildcard) also matches the bare command; the space is significant (`ls *` does not match `lsof`, `ls*` does). `:*` suffix is equivalent to trailing ` *`. Commands are split on `&&`, `||`, `;`, `|`, `|&`, `&`, newlines; every subcommand must be allowed. Deny/ask rules also apply to commands nested in subshells, command substitution and loop bodies. A dangling `&&`/`||` makes the command unparseable for allow matching. Fixed, non-configurable wrapper stripping: `timeout`, `time`, `nice`, `nohup`, `stdbuf`, `command`, `builtin`, zsh `noglob`, flagless `xargs`, and leading assignments of known-safe env vars (allow rules); deny/ask rules match past any leading assignment.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Wildcard patterns; Compound commands; Wrappers)
- Notes: Startup warning for allow rules with `*` before the subcommand (e.g. `Bash(git * main)`, which also matches `git -c core.fsmonitor=<script> diff main`). Approving a compound command saves one rule per subcommand needing approval (max 5 rules). Env runners (`direnv exec`, `devbox run`, `mise exec`, `npx`, `docker exec`) are not stripped, so `Bash(devbox run *)` allows anything; write runner+inner-command rules.

### Bash rule limits and documented bypasses
- What: The docs state plainly that a Bash deny/ask rule is not a security boundary around a program: `Bash(curl *)` does not stop `/usr/bin/curl ...` or `sh -c 'curl ...'`; `Bash(git push *)` does not stop `git -C . push` or `git -c push.default=current push`. Argument-constraining patterns (e.g. a URL prefix for curl) are fragile against option reordering, protocol changes, redirects and variables. Recommended mitigations: deny curl/wget and use `WebFetch(domain:...)`, pair with the sandbox network allowlist, or use a PreToolUse hook. Prefix rules can never auto-approve exec wrappers (`watch`, `setsid`, `ionice`, `flock`) or `find` with `-exec`/`-delete`/`-fprint`/`-files0-from`; only an exact-match rule without `*` or a hook can.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (What a Bash rule doesn't match; Exec wrappers and find actions; warning box under Read-only commands)
- Notes: WebFetch rules alone do not prevent network access if Bash is allowed.

### Built-in read-only command set and its prompt exceptions
- What: A non-configurable built-in set (`ls`, `cat`, `echo`, `pwd`, `head`, `tail`, `grep`, `find`, `wc`, `which`, `diff`, `stat`, `du`, `cd`, read-only `git`, etc.) runs without prompts in every mode. Unquoted globs are allowed for commands whose flags are all read-only. Still prompts in Manual mode for: unquoted globs with write/exec-capable commands (`find`, `sort`, `sed`, `git`) since a glob could expand to `-delete`; `docker` with a daemon-selecting flag (`-H`, `--context`, Podman `--url`); `file -m/-f`; `ps` with an `e` option (prints env vars; dashed form v2.1.290+); `which -i/--read-functions`; Windows UNC paths; writes to special shell vars like `PATH`/`IFS`; unparseable commands; any command over 10,000 characters. `cd` inside the working dirs is read-only, but `cd` to another dir combined with `git` prompts (repo hooks could run), as does `cd` with a redirect whose target can't be resolved.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Read-only commands)
- Notes: To force a prompt for a read-only command, add ask/deny. In auto mode these may still wait for classifier review.

### Redirection and `tee` target checks
- What: Output redirects (`>`, `>>`, `2>`) are checked against Edit allow/deny rules, protected paths and working directories as if Claude wrote the file; a Bash allow rule approves the command, not the target. Targets starting with `~` or containing globs need approval. Input redirects (`<`) are checked against Read rules and working dirs (v2.1.257+); glob targets or relative paths after a `cd` always need approval. `tee` destinations are checked the same way (v2.1.269+). `/dev/null`, fd forms (`2>&1`), here-docs and here-strings are not checked.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Redirections)

### PowerShell rules
- What: Same shape as Bash rules. Common aliases are canonicalized (`PowerShell(Get-ChildItem *)` also matches `gci`, `ls`, `dir`); case-insensitive. The PowerShell AST is parsed and each subcommand split on `|`, `;`, and on PS7+ `&&`/`||` must be allowed independently.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (PowerShell)

### Read/Edit path rules (gitignore syntax, anchors, depth, negation)
- What: Read and Edit rules use gitignore patterns with four anchors: `//path` absolute, `~/path` home, `/path` relative to the settings source (project/local settings and CLI: primary working dir; user settings: `~/.claude`; `--settings` file: its directory), `path`/`./path` relative to cwd. Bare filenames match at any depth. A single-directory-segment relative pattern like `src/**` matches only `<cwd>/src` as an allow rule but any `src` at any depth as deny/ask. `!` negations in deny/ask carve out of earlier `path`/`./path` rules from the same source only, cannot reach anchored rules, and cannot reopen a file inside a wholly blocked directory. Edit rules apply to all built-in edit tools; Read deny is best-effort for Grep/Glob, `@file` mentions, IDE context, pasted image paths and directory listings (v2.1.290+), and also blocks Edit/Write at that path (Write v2.1.228+). `Write(...)`, `Glob(...)`, `Grep(...)` path rules are accepted but never match; use Edit/Read.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Read and Edit; How path patterns match; Where a pattern anchors; How deep a pattern matches; Special characters and ! negation)
- Notes: `.claudeignore` has no effect. Footgun documented: `/Users/alice/file` is not absolute; `Read(/secrets/**)` in user settings means `~/.claude/secrets`. Windows paths normalized to `/c/Users/...`. Read/Edit denies also apply to recognized Bash file commands (`cat`, `head`, `tail`, `sed`, `tee`) and redirection targets, but not to `grep -r .` or scripts that open files themselves; the docs point to the sandbox for OS-level enforcement. NotebookEdit is not covered by Read deny.

### Symlink handling in permission checks
- What: For a path through a symlink both the requested and resolved paths are checked: allow requires both to match, deny applies if either matches. On macOS/Linux deny/ask rules written through a symlinked directory also apply at the real location (v2.1.268+, e.g. `/etc` vs `/private/etc`). Edit/Write refuse to write a path that is itself a symlink and redirect Claude to the target. Writes through a symlinked directory or via shell that resolve outside working dirs are not auto-approved in acceptEdits and prompt (instead of classifier) in auto mode. Unresolvable paths (loops) are refused; at open time the tool re-checks that the path still resolves to the approved location (TOCTOU guard).
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Symlinks; Writes through a symlink; Paths that can't be resolved or that change)

### Network-path read check (UNC, /net)
- What: File-tool reads of network paths (Windows UNC, macOS/Linux `/net` automounts) get their own prompt because lookup can contact the host and, on Windows, leak credentials. From v2.1.292 allow rules (even bare `Read`), PreToolUse `allow` and auto mode all leave the prompt in place; dontAsk denies; bypassPermissions runs it. Most network paths cannot be added as working directories; map/mount locally instead.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Network paths; Working directories)

### WebFetch domain rules
- What: `WebFetch(domain:host)` matches the hostname case-insensitively, trailing dot stripped. `*.example.com` matches any-depth subdomains but not the apex; a `*` elsewhere matches only one label (so `example.*` cannot match `example.evil.com`, stopping registrable-suffix attacks). Wildcards need v2.1.172+. Bare `WebFetch` vs `WebFetch(domain:*)` differ: only the `domain:` form also feeds the sandbox network allow/deny list; bare deny removes the tool, `domain:*` deny keeps it and refuses each fetch.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (WebFetch; Allow or deny every fetch)
- Notes: Reads of claude.ai artifacts are governed by `domain:` rules covering `claude.ai`/`*.claudeusercontent.com` or an `Artifact` rule, not by a bare WebFetch rule (changed v2.1.268).

### MCP, Agent and Cd rules
- What: MCP: `mcp__<server>` or `mcp__<server>__*` for a whole server, full name for one tool; server-name characters like `.` become `_`; plugin servers use `mcp__plugin_<plugin>_<server>__*`; allow rules for MCP tools must have no specifier. Org-level claude.ai connector tools set to `ask` prompt on every call even in auto and bypassPermissions. `Agent(Name)` deny rules disable specific subagents. `Cd` rules govern the user-only `/cd` command: bare deny disables it; any allow rule switches to allowlist mode; deny checks every spelling including each symlink hop; whole-path matching with `*` one segment, `**` many.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (MCP; Agent (subagents); Cd)
- Notes: Desktop Cowork: whole-tool `Bash`/`WebFetch` deny rules also apply to `mcp__workspace__bash`/`web_fetch`, but allow rules do not carry over.

### Hooks and mods in the permission pipeline
- What: PreToolUse hooks run before the prompt for every tool (except EndConversation) and can deny, force a prompt, or allow. A hook `allow` never overrides deny or ask rules. A hook exiting with code 2 blocks before rules are evaluated, so it wins over allow rules (documented pattern: allow bare `Bash` plus a blocking hook). A mod handling `tool.check` answers after PreToolUse hooks; when managed settings, `managed-mcp.json`, or a Team/Enterprise login is present, a mod approval cannot override deny, ask, hook blocks/prompts, `blockReadsOutsideWorkingDirectories`, or the `--permission-prompt-tool`/SDK `canUseTool` callback. Without those, a mod can approve calls a deny rule or hook blocked.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Extend permissions with hooks; Precedence among rules, hooks, and mods; Hook approvals in plan mode)
- Notes: In plan mode hook/mod `allow` has no effect on non-read-only calls (except ExitPlanMode). Mod approval in auto mode skips the classifier. Tools requiring user interaction (`AskUserQuestion`, MCP `requiresUserInteraction`) still prompt regardless of mod (v2.1.292+) or hook allow. The mod-overrides-deny behaviour in unmanaged setups is a notable weakness.

### Working directories, additional directories, and out-of-tree blocks
- What: Access defaults to the launch directory (primary working dir), extended by `--add-dir`, `/add-dir`, or `additionalDirectories` in settings. `permissions.blockReadsOutsideWorkingDirectories` and `permissions.blockWritesOutsideWorkingDirectories` (v2.1.314+) make file tools refuse outside paths in every mode including bypassPermissions; the write key covers Write/Edit/NotebookEdit only, not shell, MCP or hooks. `/cd <path>` moves the primary working dir mid-session, applies the new directory's project settings, hooks, `.mcp.json`, plugins, skills, subagents and `env` (v2.1.246+), and shows a trust prompt listing what it would activate.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Working directories; Block file-tool reads and writes outside the working directories; Move the session to another directory; Additional directories grant file access, not configuration)
- Notes: `--add-dir` directories load skills, commands, subagents and only `enabledPlugins`/`extraKnownMarketplaces` settings; CLAUDE.md only with `CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD=1`. Settings-file `additionalDirectories` grant file access only.

### Permissions vs sandbox interplay (`autoAllowBashIfSandboxed`)
- What: Permissions cover all tools; the sandbox enforces OS-level filesystem/network limits on Bash, PowerShell and Monitor commands and their children. Paths/domains from both are merged into the sandbox config. With sandboxing on and `autoAllowBashIfSandboxed` at default `true`, sandboxed Bash runs without prompting even under a bare `Bash` ask rule; content-scoped ask rules like `Bash(git push *)` still prompt. Plan mode skips this substitution.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (How permissions interact with sandboxing)

### Permission modes (six) and what each auto-approves
- What: `default` (labelled Manual; `manual` alias): reads only. `acceptEdits`: reads, file edits and `mkdir`, `touch`, `rm`, `rmdir`, `mv`, `cp`, `sed` on in-scope paths (also with safe env prefixes/wrappers, except `touch`); PowerShell `Set-Content`/`Add-Content`/`Clear-Content`/`Remove-Item` when that tool is on. `plan`: research and plan only, edits blocked until plan approval. `auto`: classifier reviews instead of the user. `dontAsk`: anything that would prompt is denied; only pre-approved and no-approval actions run (for CI). `bypassPermissions`: everything except the "no mode auto-approves" list; allow rules have no effect there. Deny rules apply in every mode including bypass.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Available modes; Auto-approve file edits with acceptEdits mode; Allow only pre-approved tools with dontAsk mode; Skip all checks with bypassPermissions mode); https://code.claude.com/docs/en/permissions.md (Permission modes)
- Notes: acceptEdits PowerShell positional args containing a quote still prompt (quoted vs unquoted readings differ). dontAsk denies ask-rule matches, `AskUserQuestion`, MCP `requiresUserInteraction` tools and org `ask` connector tools even if allowed. Cloud sessions ignore `dontAsk` and `bypassPermissions` from settings files (silently), so checked-in settings cannot start a cloud session in bypass.

### Actions no mode auto-approves
- What: In every mode, including bypassPermissions, these still prompt or are denied: explicit ask-rule matches; org connector tools set to `ask`; `AskUserQuestion` and MCP `requiresUserInteraction` tools; recognized `rm`/`rmdir` critical-path removals and commands flagged "Dangerous rm operation not ruled out"; cross-session messaging safeguards; reads outside working dirs when `blockReadsOutsideWorkingDirectories` is on (v2.1.257+), including untraceable commands that `cd` more than once or use subshells unless the sandbox enforces the block; writes outside when `blockWritesOutsideWorkingDirectories` is on (v2.1.314+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Actions no mode auto-approves)

### Starting-mode selection
- What: Terminal sessions take the first of: `--permission-mode` / `--dangerously-skip-permissions`; `permissions.defaultMode` from settings; built-in default. `auto` and `bypassPermissions` in project `.claude/settings.json` or `.claude/settings.local.json` are ignored (auto falls to the built-in default, not to the user-level value; bypass falls to Manual). Built-in default: Manual if any settings file disables auto, for `claude -p`/Agent SDK, and for HIPAA-configured orgs (v2.1.285+); otherwise `auto` for interactive terminal and VS Code sessions from v2.1.283 (earlier: auto only on Pro/Max/Ultra/Team with feature-flag fetching). If auto is selected but unavailable (settings, model, server-side kill switch) the session starts in Manual. First auto start shows a one-time notice.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Which mode a session starts in; Start in a different permission mode; Permission modes with the HIPAA configuration)
- Notes: VS Code extension uses its own order: `claudeCode.initialPermissionMode` (cannot be `auto`), last picked mode (Manual/Edit automatically/Auto only), managed or user `defaultMode`, built-in default; never reads project settings. Desktop remembers the picked mode per folder. Built-in auto default needs v2.1.228+ (v2.1.233+ native Windows). From v2.1.302 `-p`/SDK with no mode set start in Manual and deny anything needing approval unless allow rules, hooks, `canUseTool` or `--permission-prompt-tool` approve; before that they could start in auto when not fetching feature flags (e.g. third-party providers, `DISABLE_TELEMETRY`).

### Mode switching (Shift+Tab cycle, prompt shortcut, per-surface controls)
- What: CLI Shift+Tab cycles `default -> acceptEdits -> plan`, then optional modes (`bypassPermissions` first, `auto` last); from auto the first press goes to default. `bypassPermissions` only enters the cycle if the session was launched with a bypass flag, `--allow-dangerously-skip-permissions` (adds to cycle without activating) or `defaultMode: bypassPermissions` in user/`--settings`/managed settings; it cannot be entered from a session not started with it. `dontAsk` never appears in the cycle. Bash prompts in Manual/acceptEdits offer "Yes, and switch to auto mode" (v2.1.247+), except on prompts forced by ask rules or hooks. Status bar shows the mode.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Switch permission modes)
- Notes: Cloud sessions offer Accept edits/Plan/Auto only (file edits always pre-approved; no bypass). Remote Control offers Manual/Accept edits/Plan/Auto; requires claude.ai login, not API keys. VS Code bypass needs an extension toggle. Desktop has its own selector (one line; surveyed elsewhere).

### Auto mode: availability, model gating and kill switches
- What: A separate classifier model reviews actions before they run, blocking escalation beyond the request, unrecognized infrastructure, or actions apparently driven by hostile content. Ask rules still prompt. Available on all plans; on by default for Team/Enterprise; admins disable via managed `permissions.disableAutoMode: "disable"` (a running session leaves auto when the setting arrives from an admin source, v2.1.251+). Model support: Anthropic API/Claude Platform on AWS: Opus 4.6+, Sonnet 4.6+, Haiku 5.5, Fable models; Bedrock, Google Agent Platform, Foundry, Claude apps gateway: Sonnet 5+, Opus 4.7+, Haiku 5.5 (v2.1.293+ there), Fable. Anthropic can turn it off server-side; once a session gets that answer it stays off until the session ends. Auto mode also nudges Claude not to stop for clarifying questions.
- Status: GA (the docs warn it "does not guarantee safety")
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Eliminate permission prompts with auto mode; Auto mode on Bedrock, Agent Platform, or Foundry)
- Notes: Classifier also reviews every `SendMessage` to another agent (v2.1.222+).

### Auto mode decision order and what the classifier sees
- What: Fixed order, first match wins: (1) allow/ask/deny rules resolve, except protected-path writes still go to the classifier, critical-path rm never approved by allow, `requiresUserInteraction`/org-ask connector tools prompt, commands carrying per-command sandbox domains go to the classifier, content-scoped ask rules prompt, symlink-to-protected writes and network-path reads prompt; (2) read-only actions and working-dir edits auto-approve (but with server-side review, read-only and sandboxed shell commands wait for review); (3) everything else to the classifier; (4) a block returns a reason naming the matched rule (e.g. `[Data Exfiltration]`). Entering auto drops broad code-exec allow rules (`Bash(*)`, `PowerShell(*)`, wildcarded interpreters like `Bash(python*)`, package-manager run commands, `Agent` and `Monitor` allow rules) and restores them on exit. Before destructive commands Claude Code runs `git status` itself and shows the classifier the uncommitted state.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (How the classifier evaluates actions)
- Notes: Prompt-injection defence: in client-side classifier requests the classifier sees user messages, non-read-only tool calls and CLAUDE.md, but tool results are stripped. A PostToolUse hook can add `classifierContext` (v2.1.236+). A separate server-side probe scans incoming tool results for suspicious content. Monitor allow rules were not dropped before v2.1.236 (a classifier bypass).

### Auto mode default block and allow lists
- What: The classifier trusts the working directory and remotes configured at session start (remotes added/repointed mid-session are not trusted). Blocked by default (abridged): `curl | bash`; sending sensitive data out; prod deploys/migrations; cloud mass deletion; IAM/repo permission grants; shared infra changes; irreversible destruction of pre-existing files; force push; commits/pushes that route secrets to new destinations or widen what a deploy publishes; `git reset --hard`/`checkout -- .`/`clean -fd`/`stash drop`; amending commits not made in the session or already pushed (v2.1.198+); IaC destroy; secret-manager/DNS/TLS writes; merging unapproved PRs or self-approving; bot command comments (`atlantis apply`); prod feature flags; DaemonSets/admission webhooks; public tunnels/reverse shells; printing live credentials; package installs routed around an internal registry; `--insecure`-style flags; launching unsandboxed autonomous agents (`--dangerously-skip-permissions`, `--yes-always`); writing to Claude Code session transcripts; sending keystrokes to its own tmux pane; disabling security tests; `rm -rf "$VAR"` where the variable's value is not visible to the classifier. v2.1.257+ adds instance-metadata credential requests, indirect routes to public hosts, host-owned credentials, scanning sibling containers. v2.1.261+ adds paste-service links whose URL carries content. Allowed by default: working-dir file ops, installing declared deps, reading `.env` and sending creds to the matching API, read-only HTTP, pushing to any branch of the working repo (not deploy-named branches), deleting jobs it created, security code work, intra-session agent messages, data flow to listed trusted infra.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (What the classifier blocks by default)
- Notes: `claude auto-mode defaults` prints full rules as JSON. Pushes and PRs run without prompt unless blocked; add `permissions.ask` rules for a human checkpoint.

### Conversation-stated boundaries and approvals in auto mode
- What: Boundaries stated in chat ("don't push") are a block signal until lifted by the user; Claude's own judgment cannot lift them. They are not stored as rules but re-read from the transcript each check, so compaction can drop them (docs recommend deny rules for hard guarantees). User approvals can clear a block only if they name the action and the dangerous specifics (e.g. the branch of a force push), cover one action unless given as standing, and cannot reach some blocks.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Boundaries you state in conversation; Approvals you state in conversation)

### Auto mode fallbacks and repeated-block thresholds
- What: A blocked action shows a notification and appears in `/permissions` "Recently blocked" where it can be approved for retry. 3 consecutive or 20 total blocks pause auto mode and resume prompting (not configurable; total counter persists per session). Unparseable classifier response or a safety refusal of the classifier request blocks silently (not counted). Under server review, no verdict means deny; ten no-verdict responses in a row can stop the turn. Switching mode mid-check discards a verdict the new mode would not request. In `-p` without a prompt tool, threshold hits just skip the action and the run continues.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (When auto mode falls back; Repeated-block thresholds)

### First read outside working directories prompt (auto mode)
- What: With `blockReadsOutsideWorkingDirectories` off, auto mode reads outside the working dirs run, but the first Read/Grep/Glob outside them asks once with four answers: keep allowing (recorded), block from now on (sets the block key to true in user settings), refuse and ask next time, allow and ask next time. Not shown in `-p` or background sessions.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (The first read outside the working directories)

### Server-side classifier review
- What: In auto mode Claude Code can have the API server review actions as part of the session's normal model requests instead of separate classifier calls. Applies (rolling out) to direct Anthropic API sessions (interactive v2.1.271+ on Pro/Max/Ultra/Team, v2.1.278+ Enterprise/API; `-p`/VS Code v2.1.278+; SDK/desktop v2.1.281+; any session not fetching feature flags from v2.1.282), cloud providers and LLM gateways (v2.1.278+), Claude apps gateway (v2.1.280+), and Anthropic-hosted cloud sessions. If the server does not review (gateway strips it, unsupported region) the client falls back to its own classifier calls and may show a billing notice. If the server gives no verdict for an action, the action is denied. A hook or mod that changes input after review can cause a no-verdict denial. `CLAUDE_CODE_AUTO_MODE_SERVER=0` forces client-side; `=1` opts in; `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS=1` also stops server review. `/status` shows "Auto mode server".
- Status: GA, staged rollout ("as Anthropic rolls it out")
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Server-side classifier review)

### bypassPermissions safeguards
- What: Bypass skips prompts including protected-path writes, but: first interactive use shows an accept-responsibility dialog (sets `skipDangerousModePermissionPrompt: true` in `~/.claude/settings.json`; decline exits); refuses to start as root/sudo on Linux/macOS unless inside a recognized sandbox; refused under `--restricted` (v2.1.248+); background sessions require the acceptance in user or managed settings (local settings ignored, then bypass request ignored with a notice); cross-session messaging safeguards (`isolatePeerMachines` prompt, inbound messages held unless the sender also bypasses) still apply. Docs say it offers no protection against prompt injection and should be used only in isolated containers/VMs without internet.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Skip all checks with bypassPermissions mode; Bypass permissions ignored in a background session)

### Protected paths
- What: Writes to a fixed set of paths are never auto-approved except in bypassPermissions (and interactive plan mode with bypass available): prompted in default/acceptEdits, classifier in auto, denied in dontAsk; under `--restricted` the classifier cannot approve them. `permissions.allow` rules do not pre-approve them (check runs before allow rules). Directories: `.git`, `.config/git`, `.vscode`, `.idea`, `.husky`, `.cargo`, `.devcontainer`, `.yarn`, `.mvn`, `.claude` (exceptions: `.claude/worktrees/`, own plan files, background job scratch dir, auto-memory and subagent-memory markdown unless `--restricted`), any `--plugin-dir`, `~/.claude/dev-mods/`. Files: `.gitconfig`, `.gitmodules`, shell rc/profile files, `.envrc`, `.npmrc`, `.yarnrc*`, `.pnp.*`, `.pnpmfile.cjs`, `bunfig.toml`, Bazel rc files, pre-commit/lefthook configs, gradle/maven wrapper properties, `.devcontainer.json`, `.ripgreprc`, `pyrightconfig.json`, `.mcp.json`, `.claude.json`, stored server-managed settings and their signature/sync/consent files, files passed via `--settings`/`--mcp-config`/`--agents`, and the target of a symlinked settings file.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Protected paths)
- Notes: Prompts for `.claude/` or `~/.claude/` writes can offer a session-scoped "allow editing this folder" option. With `blockWritesOutsideWorkingDirectories` on, settings files are refused in every mode. The list targets files that execute code or alter config (git hooks, shell rc, package-manager rc, devcontainer).

### Critical-path rm/rmdir check
- What: Before running, `rm`/`rmdir` targets are checked; recognized critical-path removals are never approved by allow rules or PreToolUse `allow`. Critical: filesystem root, its direct children, home (incl. Windows 8.3 short name), drive roots and their children (incl. `\\?\C:\`, `\\localhost\C$`), the working dir and parents, additional dirs and parents when the removal is a glob under them. Also flagged: globs/trailing slash under a shell variable or unset positional param (`rm -rf "$DIR"/*`), variable followed by a top-level dir name, variables assigned from `$(pwd)`/`git rev-parse`, recursive rm of pure command-substitution output, trailing substitution after a critical path, backslash-only targets, volume-GUID paths, some `/*/` forms. Detection looks inside subshells, brace groups, `$()`, backticks, process substitution and `sh -c` scripts (with quoting-aware expansion). Unparseable commands get "Dangerous rm operation not ruled out". Recursive rm on mounted-drive shapes (`/Volumes/*`, `/media/<user>/*`, `/run/media/...`, `/mnt/*`) gets a separate prompt.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Critical paths; Which paths are critical; Other targets that count as critical paths; Removals inside nested commands and inline scripts; The Dangerous rm operation not ruled out prompt; The mounted drive prompt)
- Notes: Docs warn the check can miss commands and other programs can delete too; it is not an OS control. Guidance tells Claude to rewrite with `${DIR:?}` guards. Escape env vars (launch env only, ignored in settings `env`): `CLAUDE_CODE_DISABLE_SUBSTITUTION_RM_PROMPT`, `CLAUDE_CODE_DISABLE_INLINE_SHELL_RM_PROMPT`, `CLAUDE_CODE_DISABLE_MOUNTED_DRIVE_RM_PROMPT`.

### Critical-path removals per mode, countdown, and the "even more dangerous" override
- What: default/acceptEdits/plan ask; dontAsk denies; auto asks in the terminal with a two-minute countdown and denies elsewhere; bypass asks in the terminal with countdown and denies in VS Code/Desktop unless a PermissionRequest hook allows; in `-p`/SDK the prompt tool or `canUseTool` decides. Expired countdown denies and tells Claude an alternative; three expiries in a session switch to immediate denial until the next user message (v2.1.281+). `CLAUDE_CODE_DISABLE_DANGEROUS_RM_TIMEOUT=1` removes the timer in bypass and hands the decision to the classifier in auto. `CLAUDE_CODE_EVEN_MORE_DANGEROUSLY_SKIP_PERMISSIONS=1` (v2.1.311+, launch env only) lets bypass run critical-path removals unprompted; it is disabled by a long list of conditions (any PermissionRequest hook or prompt tool, VS Code/Desktop, any Bash/Edit/Read deny or ask rule, block-reads key, nested inside another session, background/team/Remote Control sessions, unreadable managed settings or Windows registry) and prints a startup line.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Critical-path removals in each permission mode; Time limits and denials in auto and bypassPermissions modes; Turn off the critical-path removal prompt in bypassPermissions mode; Why the critical-path removal prompt stays on)
- Notes: Docs explicitly say not to rely on the nested-session limit as a security boundary.

### PowerShell Remove-Item and cmd removal checks
- What: With the PowerShell tool, `Remove-Item` on system paths (root, top-level dirs, drive roots, home) and on wildcard targets (`*`, `/*`, `\*`, `$dir/*`) is denied in every mode before the classifier sees it; `-Recurse` on the working dir or parents is handled as a normal approval. `cmd /c rd|rmdir|del|erase` on system paths is denied by default (v2.1.283+), treating a PowerShell variable after literal text as empty; disable with `CLAUDE_CODE_DISABLE_POWERSHELL_CMD_RM_DENY=1`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Remove-Item in PowerShell)

### Auto mode configuration: `autoMode.environment` trusted infrastructure
- What: `autoMode.environment` is an array of prose entries (not patterns) telling the classifier what is inside the trust boundary; anything not listed is a potential exfiltration target. `claude auto-mode defaults` shows three entry kinds: context slots (Organization, Primary use, Cloud providers, Repository visibility (assumed private unless evidence the classifier can read says otherwise), Internal sharing, Org-specific CLIs, Secrets management, CI/CD deploy targets, Network posture, Host containment (v2.1.257+; until it names an identity, requests for host credentials are blocked), Protected deployment namespaces, Data retention); trust slots (Trusted repo, Source control, Trusted internal domains, Trusted cloud buckets, Key internal services, Internal package registry; only repo and its remotes trusted by default); sensitivity slots (Sensitive data locations & audiences, Sensitive remote targets, Protected IaC scopes; default heuristics, e.g. names containing `prod`). The literal `"$defaults"` splices the built-in entries at that position.
- Status: GA
- Evidence: https://code.claude.com/docs/en/auto-mode-config.md (Define trusted infrastructure)
- Notes: Repository visibility evidence must be something the classifier reads (user message), because command output like `gh repo view` never reaches it.

### Where `autoMode` is read from (project settings excluded)
- What: The classifier reads `autoMode` only from user settings, managed settings, and `--settings`/SDK inline JSON; it deliberately ignores `.claude/settings.json` and `.claude/settings.local.json` so a checked-in repo or build step cannot inject allow rules. Scopes are combined additively; developers can add but not remove managed entries, yet a developer `allow` entry can override an org `soft_deny` because allow is an exception inside the classifier, so the docs say this is not a hard policy boundary and point to managed `permissions.deny` for that. The classifier also reads the same CLAUDE.md content Claude loads.
- Status: GA
- Evidence: https://code.claude.com/docs/en/auto-mode-config.md (Where the classifier reads configuration)
- Notes: Interesting tension: the classifier reads project CLAUDE.md (repo-controlled) but not project `autoMode`.

### Classifier rule lists: `hard_deny`, `soft_deny`, `allow`, precedence, explicit intent
- What: Prose rule arrays replace or extend (`"$defaults"`) the built-in lists. Precedence inside the classifier: `hard_deny` blocks unconditionally; `soft_deny` blocks unless an `allow` exception or explicit user intent applies; explicit intent means the user's message directly and specifically describes the exact action (e.g. "force-push this branch", not "clean up the repo"). Omitting `"$defaults"` drops the whole built-in section (for `soft_deny`: force push, `curl | bash`, prod deploys, auto-mode bypass; for `hard_deny`: the built-in data exfiltration rule). Sections are independent. Built-in rules have labels with "must name" requirements (e.g. "Git Destructive"). `autoMode.classifyAllShell: true` suspends every Bash/PowerShell allow rule while in auto so every shell command is classified (except critical-path rm), trading latency and calls for coverage.
- Status: GA
- Evidence: https://code.claude.com/docs/en/auto-mode-config.md (Override the block and allow rules; Route all shell commands through the classifier; Inspect the defaults and your effective config)
- Notes: Docs concede that narrow allow rules (default behaviour) can let a destructive argument through without the classifier seeing it.

### Human checkpoints in auto mode
- What: Three firmness levels: content-scoped `permissions.ask` (always prompts, classifier cannot approve; but `git -C dir push` evades a `Bash(git push *)` rule), `permissions.deny` (blocks before classifier, nothing overrides), and conversational boundaries (session-only, lost on compaction). Default auto mode allows pushes to any branch of the working repo and PR creation, except deploy-named branches (`production`, `release`, `gh-pages`) which are judged on their own.
- Status: GA
- Evidence: https://code.claude.com/docs/en/auto-mode-config.md (Common boundaries; Add a human checkpoint)

### `/auto-mode-setup` draft generator
- What: Built-in command that drafts `autoMode.environment` (and sometimes rule) entries from the project's CLAUDE.md, README, config files and git remotes, existing `autoMode` and `permissions.allow`, and hosts/buckets/command names from commands Claude ran in recent sessions (never user messages); two opt-in scans add the first word of each shell-history command and remote hosts of repos under home. Draft is accepted or discarded whole and written to `~/.claude/settings.json`; afterwards it offers to remove allow rules auto mode ignores or that auto-approve destructive commands. After several blocks with no environment entries a "Teach auto mode about your environment?" dialog offers it.
- Status: GA
- Evidence: https://code.claude.com/docs/en/auto-mode-config.md (Generate environment entries with /auto-mode-setup; What /auto-mode-setup reads; Review and save the draft; Turn off /auto-mode-setup)
- Notes: Pro/Max/Ultra/Team v2.1.228+ (v2.1.233+ Windows); Enterprise/Console API key v2.1.283+; not available when feature-flag fetching is off before v2.1.304; unavailable in cloud sessions. Disable via `skillOverrides: {"auto-mode-setup": "off"}`.

### `claude auto-mode` CLI and the `/permissions` Auto mode tab
- What: `claude auto-mode defaults [--label prefix]` prints built-in rules as JSON; `config` prints effective rules; `critique` gives AI feedback flagging ambiguous, redundant or false-positive-prone custom entries; `reset [--yes]` removes `autoMode` from user settings only. `/permissions` Auto mode tab (v2.1.246+) edits rules, showing managed/`--settings` entries read-only and saving to user settings.
- Status: GA
- Evidence: https://code.claude.com/docs/en/auto-mode-config.md (Inspect the defaults and your effective config; Edit rules from /permissions)

### Reviewing and retrying auto mode denials
- What: `/permissions` "Recently blocked" tab: `r` approves and lets Claude retry, `Enter` approves for later, `Enter` again revokes, double Ctrl+C discards. Users are told to judge by the command/input, not the row description (written by Claude or the tool). Approval adds no rule and the retry is classified again. Rows that cannot show the full input cannot be approved there. Denial notices look like `bash blocked by auto mode · /permissions · Auto mode: [Data Exfiltration]`. A `PermissionDenied` hook receives the exact `tool_input`. "Not run" rows mean no verdict (classifier error), distinct from "Blocked by auto mode".
- Status: GA
- Evidence: https://code.claude.com/docs/en/auto-mode-config.md (Review denials; Fix a denial with an allow rule, an environment entry, or a retry; Fix repeated denials)

### OS sandbox for shell commands: mechanism and defaults
- What: An OS-enforced boundary around Bash, PowerShell and Monitor commands and all their child processes. Off by default; enable via `/sandbox` (saves to `.claude/settings.local.json`, which is added to the global gitignore) or `sandbox.enabled: true`. Mechanism: macOS Seatbelt; Linux and WSL2 bubblewrap plus `socat` relay (optional seccomp filter adds Unix-socket blocking; installed via `npm i -g @anthropic-ai/sandbox-runtime`); WSL1 and native Windows unsupported (commands run unsandboxed). Built on the open-source `@anthropic-ai/sandbox-runtime`. Defaults: writes to working dir, a per-user temp dir (`$TMPDIR` is pointed at it) and added dirs; reads of most of the machine including `~/.ssh` and `~/.aws/credentials`; no direct network, only via a local proxy with an empty allowlist; environment inherited including secrets.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (What the sandbox restricts; Get started; Set up Linux and WSL2; OS-level enforcement; Temporary directories)
- Notes: If unsupported or dependencies missing, commands silently run unsandboxed unless `sandbox.failIfUnavailable: true` (then exit at startup). Self-test: ask Claude to run `touch ~/sandbox-probe` and `curl --noproxy '*' https://example.com`; typing them at `!` does not test anything because `!` commands usually run unsandboxed. Linux AppArmor userns restriction (Ubuntu) blocks bwrap; docs give an AppArmor profile or `sysctl` workaround; `sandbox.bwrapPath` (managed) pins the binary.

### What runs outside the sandbox
- What: Built-in file and web tools (Read, Edit, Write, WebFetch, WebSearch) follow permission rules instead (a `denyRead` does not stop Read; `allowedDomains` does not limit WebFetch). Hooks, local MCP servers, plugin monitors, LSP servers, status line and `apiKeyHelper` run with full user access. Mods and processes they spawn are unsandboxed. User-typed `!` commands run unsandboxed in most sessions, as do `excludedCommands` matches and approved unsandboxed retries. To cover all of these, run the whole Claude Code process in a container, VM, or the sandbox runtime.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (What runs outside the sandbox; Scope)
- Notes: Subagents share the parent's process and sandbox config; background sessions are separate processes using their own settings; computer use runs on the real desktop with per-app prompts.

### Sandbox modes: auto-allow vs regular permissions
- What: Auto-allow (default when enabled, `autoAllowBashIfSandboxed: true`) runs any command that executes inside the sandbox without prompting, even in Manual mode where file tools would prompt. Still enforced: Bash deny rules, critical-path rm checks, content-scoped ask rules; a bare `Bash` ask rule is skipped for sandboxed commands (not in plan mode). Monitor commands are not auto-approved. Commands where a leading env assignment, `make VAR=`, or `eval` could change which program runs fall back to the normal permission flow (still sandboxed). Regular-permissions mode prompts for everything even when sandboxed.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (Sandbox modes; Auto-allow mode; Regular permissions mode)
- Notes: Exceptions to independence from permission mode: plan mode, auto-mode commands carrying per-command domains, and server-side classifier review of sandboxed commands. An allow rule that matches a command also approves its unsandboxed retry.

### Unsandboxed retry escape hatch and strict sandbox mode
- What: When a command fails in the sandbox (the result names a denied host), Claude may retry with the `dangerouslyDisableSandbox` parameter. Approval: bypass runs it; Manual/acceptEdits prompt "Bash command (unsandboxed)"; auto mode sends it to the classifier; dontAsk denies. An ask rule `Bash(dangerouslyDisableSandbox:true)` forces a prompt even in auto and bypass. `allowUnsandboxedCommands: false` ("Strict sandbox mode") makes the parameter ignored. A `false` in user, `--settings` or managed settings beats a project `true` (v2.1.285+; before, project won). Under strict mode, `!` commands are sandboxed only in background sessions or Linux sessions with `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` (before v2.1.260, in every session).
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (The unsandboxed retry escape hatch; Turn off the retry with strict sandbox mode)
- Notes: Critical-path docs note that by default an allow rule or bypass mode can approve the retry with no prompt, so strict mode matters for the rm safeguards too (https://code.claude.com/docs/en/permission-modes.md (Critical paths)).

### Sandbox filesystem rules
- What: `sandbox.filesystem.allowWrite`, `denyWrite`, `denyRead`, `allowRead` (re-opens inside a denied region); narrower path wins when read rules overlap, and a deny holds inside a wider allow (including wildcard denies like `~/**/.env`). Arrays merge across scopes (union) unless a lock excludes them. Paths use normal conventions (`/abs`, `~/home`, relative to the settings file's base), unlike permission rules' `//` and `/`. Edit allow rules grant sandbox writes the same as `allowWrite`; Read/Edit deny rules block. Edits apply to the running session at the next command. Excluding a settings source with `--setting-sources` drops its sandbox filesystem entries and relevant rules (v2.1.246+). In linked git worktrees writes to the main repo's shared `.git` are allowed except `hooks/` and `config`. `blockReadsOutsideWorkingDirectories` also strips sandboxed reads of home and user-file dirs.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (Configure sandboxing; Filesystem isolation; Permission rules)

### Sandbox protected paths (write-denied even inside writable dirs)
- What: Separate from the permission system's protected paths, enforced on running commands: `.claude` settings files and `.claude/skills|agents|commands|hooks`, `.mcp.json`, `.claude/workflows`, `.claude/scheduled_tasks.json` in the working dir and its ancestors; shell startup files, `.gitconfig`, `.vscode`, `.idea`, `.git/hooks` and `.git/config` in the working dir; files that would turn the working dir into a bare git repo (`HEAD`, `objects`, `refs`, `config`); most of `~/.claude` plus `~/.claude.json` and `.credentials.json`. `allowWrite` or Edit allow rules cannot override; only `filesystem.disabled` does. New top-level `HEAD`/`objects`/`refs` created during the session are deleted after each sandboxed command (rules refined in v2.1.307). On macOS denials follow symlinks to real locations. On Linux, write-denied nonexistent files are held by 0-byte read-only placeholders, which can be left behind if a session is killed (`claude doctor` lists them).
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (Protected paths; When the sandbox deletes HEAD, objects, or refs; Denied paths that pass through a symlink; 0-byte read-only files appear at .claude settings paths)
- Notes: Rationale stated: stop a command granting itself permissions or adding a hook/MCP server that would run outside the sandbox. The bare-repo defence targets git's discovery of a crafted repo in the working dir.

### `excludedCommands`
- What: Patterns (Bash-rule syntax) whose matching calls run fully unsandboxed, then go through the normal permission flow (read-only and allow-rule matches run unprompted; auto mode classifies; bypass runs unless an ask rule). Every command in a chained call must match; matching is on call text (scripts that call docker internally, or `/usr/local/bin/docker`, do not match); redirects to files, `cd`, `$(...)` keep the call sandboxed; `git clone` coverage only for relative targets without `..`; `unshare` always stays sandboxed. Ignored from repo settings when the sandbox is admin-required.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (Run commands outside the sandbox with excludedCommands)
- Notes: Docs warn that excluding a tool that runs a file in the working dir (interpreter, `docker compose`) lets Claude write that file and execute it unsandboxed.

### Disabling filesystem isolation
- What: `sandbox.filesystem.disabled: true` keeps network isolation but gives unrestricted host filesystem access; honored only from user, managed and `--settings` (never project); only managed can set it when managed settings configure `sandbox.filesystem` or a credential-file `deny`; ignored everywhere when `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` is set. With it off, `denyRead` and credential-file denies stop applying, env-var denies/masks and file masks still apply; `$TMPDIR` reverts to the shell's.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (Disable filesystem isolation; Which settings can disable it; What changes when filesystem isolation is off)
- Notes: Docs warn a command could then write shell rc, `$PATH` executables or `~/.claude/settings.json` to widen its own access next run.

### Network isolation and the sandbox proxy
- What: Linux/WSL2 commands run in a separate network namespace with no connectivity; macOS Seatbelt allows only the connection to the proxy. A host-side proxy (outside the sandbox) is advertised via `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`; it checks the hostname against allowed/denied domains (no port = all ports). Tools ignoring proxy env (plain ssh, most DB drivers) cannot connect; UDP, QUIC and ICMP cannot leave. Approving a host at a prompt allows it for the session; "don't ask again" saves a `WebFetch(domain:...)` allow rule to local settings (user settings when admin-required). `WebFetch(domain:...)` allow rules pre-allow domains (only `*.x` and bare `*` wildcards honored for the sandbox; bare `*` v2.1.186+). `strictAllowlist` (user/managed/`--settings`, v2.1.219+) refuses instead of prompting. After allowlisting, the proxy refuses names that resolve only to local addresses (loopback, link-local incl. 169.254.169.254, own addresses), except `localhost`/`*.localhost` (v2.1.284+); private ranges like 10/8 are allowed; listing the IP lifts it. Upstream corporate proxies via `HTTPS_PROXY` are honored after allowlist checks. IPv6 entries use brackets (v2.1.229+); ambiguous entries are denied broadly and allowed narrowly.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (Network isolation; Hosts outside your allowed domains; Hostnames that resolve to local addresses; IPv6 addresses in domain lists)
- Notes: Per-mode handling of unlisted hosts: bypass allows (unless strict/managed lock), Manual/acceptEdits/plan prompt, auto refuses unless listed per-command and classifier-approved, dontAsk refuses. `deniedDomains` always refuse. SSH git works on Linux via proxy tunnel but fails on macOS. On Linux a sandboxed command's localhost is private; macOS needs `allowLocalBinding` to reach host services.

### Per-command allowed domains in auto mode
- What: With sandbox on in auto mode (v2.1.271+), each sandboxed Bash/PowerShell/Monitor command can carry a list of extra hosts (domain, `*.` wildcard or IP, optional port); the classifier reviews the hosts with the command, and an approved list opens those hosts only for that command while it runs, adding nothing to session or settings. Unlisted hosts are refused without prompt or classifier call; the refusal names the host so Claude re-runs with it added. `deniedDomains` still block; strict or managed domain locks refuse per-command lists. A command carrying hosts always goes to the classifier, bypassing allow rules and auto-allow.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (Per-command allowed domains in auto mode)
- Notes: Elegant capability-scoping design: network grants are per-invocation and reviewed alongside the command.

### Credential protection: deny and mask (sentinel substitution)
- What: `sandbox.credentials.files` and `.envVars` entries with `mode: "deny"` block file reads (filesystem layer) or unset env vars before each sandboxed command; deny entries merge from all scopes and can't be removed. No built-in credential deny list. `mode: "mask"` gives the command a per-session sentinel placeholder; the proxy substitutes the real value on outbound requests to allowed hosts (`injectHosts` narrows which). Requires `network.tlsTerminate` (experimental) so the proxy can see request contents; without it the sentinel goes out unchanged and auth fails (`claude doctor` warns). Mask, `tlsTerminate`, `allowPlaintextInject`, `awsPairs`, `sigv4` honored only from user/managed/`--settings`; server-managed delivery triggers a security approval dialog. Structured values supported via `extract`, `decode`, `maskClaims`, `onExtractNoMatch` (default `warn` skips the entry, leaving the real file readable), `maskDuplicates`. AWS: mask key id and secret together; proxy detects SigV4 by sentinel and re-signs (v2.1.221+); streaming uploads, presigned URLs and SigV4A fail closed (or forward via `sigv4`, v2.1.224+). File masks (v2.1.221+): Linux/WSL2 read a sentinel copy; macOS blocks the file outright. Masks fall back to deny for directories, globs, files over 8 MiB or non-UTF-8.
- Status: GA, with `network.tlsTerminate` labelled experimental
- Evidence: https://code.claude.com/docs/en/sandboxing.md (Protect credentials; Mask credentials; Mask environment variables; Re-sign AWS requests; Mask credential files)
- Notes: `sandbox.credentials` affects sandboxed Bash only; `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` strips credentials from all subprocesses. Very relevant pattern for a node-side secret broker.

### Custom sandbox proxy
- What: `network.httpProxyPort` / `socksProxyPort` direct sandboxed traffic to a user-run localhost proxy for inspection/logging. Once set, all Claude Code network controls (allow/deny lists, strict allowlist, prompts, local-address check) stop applying to that traffic, and an upstream `HTTPS_PROXY` is not chained. Which files may set ports depends on locks (managed only with `allowManagedDomainsOnly`; managed/`--settings`/user when admin-required; otherwise any file; before v2.1.285 any file).
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (Custom proxy configuration)

### Documented sandbox limitations and weakening switches
- What: Domain filtering uses the client-supplied hostname without TLS inspection, so broad domains like `github.com` allow exfiltration and domain fronting can reach unlisted hosts; stronger TLS-aware isolation is "an active area of development". `allowUnixSockets` can open escalation paths (e.g. Docker socket). Broad `allowWrite` to `$PATH` dirs or shell rc files enables escalation. `enableWeakerNestedSandbox` (bind-mounts the container's `/proc` for unprivileged containers) considerably weakens Linux isolation. `allowAppleEvents` removes code-execution isolation on macOS. `allowMachLookup` services are unconfined. `allowNestedUserNamespaces` (v2.1.305+) lets code escape the sandbox. Docs state the sandbox "reduces risk but is not a complete isolation boundary" and that both filesystem and network isolation are needed together.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (Limitations; Security limitations; Scope; Bubblewrap fails to start inside a container; Chromium or rootless Podman fails inside the sandbox on Linux)
- Notes: Known incompatibilities: watchman/jest, docker, Go TLS on macOS, clipboard tools, SSH git on macOS, non-proxy-aware clients.

### `Sandbox is required` refusal behaviour
- What: With `failIfUnavailable`, if the sandbox fails after startup each sandboxed command is refused with `Sandbox is required but failed to initialize` or `... stopped being available in this session`; the command never falls back to unsandboxed, and `dangerouslyDisableSandbox` is ignored. Excluded commands and `!` commands keep running (refused in background sessions, and all refused on Linux with `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (Commands are refused with Sandbox is required)

### Isolation options compared (sandbox environments)
- What: Comparison of six approaches: built-in sandboxed Bash tool (shell commands only), sandbox runtime (whole Claude Code process incl. file tools, MCP, hooks; no Docker), dev container, custom container, VM (strongest; e.g. Firecracker, Docker Sandboxes microVM), and Anthropic-hosted cloud sessions. Guidance: `--dangerously-skip-permissions` must always run inside a container/VM/runtime as non-root; auto mode is a per-action control, not isolation, so isolation is defence in depth there but not required; the Bash sandbox alone is insufficient for fully unattended runs; untrusted repos belong in a dedicated VM or a cloud session. Layering is supported (Bash sandbox inside a container needs `enableWeakerNestedSandbox` in unprivileged containers).
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandbox-environments.md (Compare sandboxing approaches; Choose an approach; How isolation relates to permission modes; Custom container; Virtual machine)
- Notes: Page stresses isolation does not change what is sent to the model provider. Only the built-in Bash sandbox is enforceable by Claude Code itself; dev containers are a "convention" and containers/VMs need MDM/allowlisting.

### Sandbox runtime (`srt`) wrapping the whole process
- What: `@anthropic-ai/sandbox-runtime` wraps an entire process (launch with `srt claude`) in the same Seatbelt/bubblewrap isolation. Config in `~/.srt-settings.json` or `--settings`. Default: no network and writes only to built-in runtime paths (`/tmp/claude`, `~/.npm/_logs`, `~/.claude/debug`). Users must grant writes to the project, `~/.claude`, `~/.claude.json`, the temp dir (`/tmp`, or `/private/tmp` on macOS), and domains `api.anthropic.com` (kept even on third-party providers for the WebFetch domain safety check unless `skipWebFetchPreflight: true`), `claude.ai`, `platform.claude.com` (OAuth). Built-in mandatory denies at the project root: `.git/hooks`, `.git/config` (unless `allowGitConfig`), `.mcp.json`, `.claude/commands`, `.claude/agents`, shell startup files; `denyWrite` beats `allowWrite`. On macOS denies are checked at write time (covering nested repos created later); on Linux the deny list is built once at launch with a shallow scan (`mandatoryDenySearchDepth`) and misses repos created later (`git init`/`clone`). Missing settings file starts anyway with defaults; empty/invalid file refuses to start.
- Status: beta ("beta research preview", config format may change)
- Evidence: https://code.claude.com/docs/en/sandbox-environments.md (Sandbox runtime; Set up and launch the runtime; What the runtime blocks on its own; After unattended runs)
- Notes: Docs warn other config paths Claude Code loads must be `denyWrite`n or a session can persist unsandboxed hooks/permissions/MCP servers for the next launch. On Linux write grants only apply to paths that already exist. Runtime should be installed outside writable paths.

### Cloud session isolation (summary)
- What: Cloud sessions run in an isolated Anthropic-managed VM; a network proxy enforces a default allowlist; a separate proxy holds the GitHub token outside the sandbox and issues scoped credentials inside. Self-hosted environments shift isolation and egress responsibility to the operator. Requires a Claude subscription; GitHub needed unless `claude --cloud` bundles and uploads a local repo.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandbox-environments.md (Cloud sessions)
- Notes: Details are on the claude-code-on-the-web page (outside this slice).

### Hooks versus permission modes
- What: A PreToolUse hook's `deny` blocks the tool in every permission mode, including `bypassPermissions` and `--dangerously-skip-permissions`; PreToolUse fires before any mode check, `dontAsk` included. A hook's `allow` skips the prompt but never overrides settings deny/ask rules, and cannot suppress prompts for network-path reads, MCP tools marked `requiresUserInteraction`, SDK `always_ask` tools, or org-set `ask` connector tools. In plan mode a hook `allow` for a file edit gets the same treatment as no hook. Multiple PreToolUse decisions combine as deny > defer > ask > allow.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks-guide.md (Hooks and permission modes); https://code.claude.com/docs/en/hooks.md (PreToolUse decision control)
- Notes: A mod (in-process JS plugin) can approve a call a PreToolUse hook blocked in some unmanaged setups (no managed settings, no Team/Enterprise login); read-only calls only in plan mode. The `if` filter is described as best-effort, so docs say to enforce hard allow/deny with permission rules, not hooks.

### Workspace trust gating for hooks, status line, skills
- What: Interactive sessions hold back hooks from every settings file (even the user's own) until the workspace trust dialog is accepted. `-p` and SDK sessions never show the dialog and treat the folder as trusted, so repository hooks run in never-trusted folders. Project-subagent frontmatter hooks are stricter (need the dialog; `-p` does not count). Project-skill frontmatter hooks and a project skill's `allowed-tools` apply even in an untrusted `-p` run. `statusLine` and `autoMemoryDirectory` from project settings follow the hook trust rule.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Workspace trust; Hooks in skills and agents); https://code.claude.com/docs/en/skills.md (Pre-approve tools for a skill); https://code.claude.com/docs/en/statusline.md (Workspace trust required); https://code.claude.com/docs/en/memory.md (Storage location)
- Notes: Docs recommend `--bare` or `--settings '{"disableAllHooks": true}'` before scripting `claude -p` over an unknown repository.

### Read deny rules to keep generated/vendored code out
- What: `permissions.deny` entries like `Read(./**/dist/**/*)` block the built-in file tools and recognised Bash file commands (`cat`, `head`, `grep`, `find`, redirections) when the denied path is an argument; Grep/Glob results are filtered best-effort. A Bash `grep -r` over a directory containing denied files still returns them, and subprocesses opening files are not covered. Relative patterns anchor at the session cwd; `//` prefix gives absolute.
- Status: GA
- Evidence: https://code.claude.com/docs/en/large-codebases.md (Block reads of generated and vendored code)
- Notes: Content searches respect `.gitignore` by default.

### Project `.mcp.json` approval and workspace trust
- What: Project-scoped servers require an interactive approval prompt the first time, so a cloned repo cannot launch processes without consent (`claude mcp reset-project-choices` resets). Approvals committed in a repo's `.claude/settings.json` (`enableAllProjectMcpServers`, `enabledMcpjsonServers`) are ignored until the user accepts the workspace trust dialog; approvals from user settings, managed settings or `--settings` apply even untrusted. Untracked `.claude/settings.local.json` approvals apply only after trust, since a git "is it tracked" check runs only in trusted folders (fixed in v2.1.207). `disabledMcpjsonServers` always rejects.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Project server approvals and workspace trust; Project scope); https://code.claude.com/docs/en/mcp-quickstart.md (Edit .mcp.json directly)
- Notes: Important weakness: in `claude -p`, Agent SDK, and cloud sessions outside project threads, and in `bypassPermissions` with `skipDangerousModePermissionPrompt`, project servers load WITHOUT asking. Mitigations named: `disabledMcpjsonServers`, `--setting-sources`, `--strict-mcp-config` (v2.1.246+ stops waiting on approvals for unloaded servers).

### Server-declared approval and safety annotations
- What: `_meta["anthropic/requiresUserInteraction"]: true` (JSON boolean only) forces a permission prompt on every call, even in `acceptEdits`, `auto` and `bypassPermissions`; no "don't ask again"; allow rules do not skip it; `dontAsk` denies it; `--permission-prompt-tool` allow results are converted to deny; Agent SDK `canUseTool` may approve. Remote Control and SDK one-tap approval is withheld for such tools. Standard `annotations.readOnlyHint` lets Claude Code run calls in parallel with other read-only calls; `readOnlyHint` or `idempotentHint` lets it replay a pending call once on a new connection to an HTTP server or connector after a drop.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Require approval for a specific tool; Mark tools that are safe to call twice)
- Notes: The same one-tap withholding applies to any permission request only the terminal dialog can render in full.

### Disable/enable servers per project without removing them
- What: `/mcp` toggles record per-project lists in `~/.claude.json`: `disabledMcpServers` (opt-out for user, plugin, managed-provided, fetched connectors, default-on built-ins) and `enabledMcpServers` (opt-in for default-off built-ins such as `computer-use`). Each server consults exactly one list. Separate from `enabledMcpjsonServers`/`disabledMcpjsonServers` (project file approvals).
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Disable a server without removing it)

### Plugin security model (what is and is not sandboxed)
- What: Docs state plainly that an installed plugin can run arbitrary code with user privileges. Hooks, monitors, MCP servers, LSP servers and mod-started processes run outside the sandbox and are not subject to permission rules. Only Claude's tool calls are gated: calls to plugin MCP tools and Bash commands running plugin `bin/` executables go through permission rules. Skills/commands/agents are context (instructions). Auto-update can change reviewed files on disk. Installing a plugin also enables it unless `defaultEnabled: false`. Uninstall leaves cached files for 14 days unless removed manually.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/security.md (Understand what a plugin can do; Remove a plugin you no longer trust)
- Notes: Repository (project-scope) plugins load only after the folder's own trust dialog (not parent trust, not `-p`), their MCP servers need per-server approval like `.mcp.json`, MCPB or out-of-dir MCP configs are skipped, and monitors do not load (https://code.claude.com/docs/en/plugins/loading.md, Trust check and component limits for repository plugins).

### Mod approvals vs permission rules (`tool.check` precedence)
- What: A mod's `tool.check` hook runs after `PreToolUse` hooks and before the permission prompt, auto-mode classifier or `PermissionRequest` hook; it can return allow/ask/deny. Where the built-in guard loads (machine has managed settings or `managed-mcp.json`, or the credential is a Team/Enterprise `/login`), a user mod's allow cannot lift a deny rule, an ask rule, a `PreToolUse` block/prompt, `blockReadsOutsideWorkingDirectories`, or bypass `--permission-prompt-tool`/`canUseTool`. Without that setup (plain API key, Bedrock/Vertex/Foundry without managed settings, long-lived OAuth token) a mod CAN approve past deny rules and blocking hooks. In auto mode a mod-approved call skips the classifier (unless a prompt tool/callback is in use with the guard loaded). Plan mode: a mod can tighten but never loosen non-read-only calls. Tools requiring user interaction (AskUserQuestion, `requiresUserInteraction` MCP tools) still prompt even if a mod approves (v2.1.292+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Precedence among rules, hooks, and mods; Hook approvals in plan mode); https://code.claude.com/docs/en/plugins/mods/admin.md (Know what happens by default); https://code.claude.com/docs/en/plugins/mods/events.md (Approve or refuse a tool call before the user is asked)
- Notes: `allowModsToOverrideDenyRules` (managed, guard option) re-enables override. `e.isReadOnly` (v2.1.304+) is the tool's self-report; docs warn WebFetch and Agent report read-only. A deny of `Read(.env)` does not stop a mod's own `$.fs.read` of that file.

### Mod `tool.call` interception (deny, answer, rewrite, withhold, hold-for-user)
- What: `tool.call` fires before every tool run (subagents and MCP included). A hook can pass changed arguments to `next`, retry by calling `next` again, answer with `{ result }` so the tool never runs and no permission prompt appears, return `{ deny }` (Claude reads it as the tool result), or `{ deny }` after `next` to withhold a result (nothing undone; note prefixes like "Bash ran, and a plugin withheld its result"). A hook can `await $.ui.ask(...)` to hold the call while the user picks an option in the question dialog; waiting inside mods API calls does not count against the 10 s hook budget.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/events.md (Guard or change a tool call; Hold a tool call until the user decides); https://code.claude.com/docs/en/plugins/mods/reference.md (Tools)
- Notes: Failure mode is fail-open: a hook that throws/times out before `next` is skipped and the call proceeds; `.catch(handler)` (1 s budget) makes it fail closed. In auto mode, a hook that rewrites input after the classifier reviewed it causes a denial ("a hook changed this call's input after the model wrote it"). Managed `PreToolUse` hooks run before any mod and run again on a mod-rewritten call; non-managed `PreToolUse` hooks run after the last mod, so a mod answering without `next` skips them.

### Channel permission relay
- What: A two-way channel that declares `capabilities.experimental['claude/channel/permission']: {}` receives `notifications/claude/channel/permission_request` with `request_id`, `tool_name`, `description`, `input_preview` whenever a tool-approval dialog opens; it replies with `notifications/claude/channel/permission` `{request_id, behavior: allow|deny}`. Terminal dialog and remote prompt are both live; first answer wins. Only tool approvals relay; trust and MCP-consent dialogs never do.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/channels-reference.md (Relay permission prompts; Permission request fields)
- Notes: request_id = 5 lowercase letters excluding `l`; verdicts with unknown or repeated IDs are dropped silently. v2.1.211+ sanitizes fields (bidi/invisible chars neutralized, whitespace folded, 3,500 code-point head+tail elision with a counted marker; per top-level field for input_preview). v2.1.234+ masks recognizable provider tokens as `[REDACTED]` but never masks spans containing shell, path or URL syntax; unprefixed secrets and multi-line keys are not masked. v2.1.234+ sends requests only to servers registered as channels for the session (before that, `false` for the capability counted as declared). Description for Bash may be just "Run shell command", so docs tell implementers to render input_preview.

### Subagent permission-mode inheritance
- What: If `permissionMode` is unset, the subagent inherits the parent's mode. If the parent is in bypassPermissions, acceptEdits or auto, the subagent runs in that mode regardless of its frontmatter. Under auto, the classifier uses the main session's rules and also reviews the subagent's work and final report before delivery. If the parent is in plan, the subagent stays in plan; with `dontAsk` set, plan-mode asks become denials. If the parent is in default or dontAsk, the frontmatter value applies, with two exceptions: `bypassPermissions` cannot escalate (v2.1.267+), and `auto` falls back when unavailable. Background subagents send every permission prompt to the main session, named by subagent. Esc denies that one call without stopping the subagent. A "rest of session" grant applies to the whole session, the main conversation included.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Permission modes; Tools, permissions, and results of background subagents)
- Notes: This is a no-escalation rule. A subagent can only be stricter than, or equal to, a permissive parent.

### Agent teams permissions and plan approval
- What: Per-teammate modes cannot be set at spawn. Teammates start in the lead's mode, including bypassPermissions; a dontAsk lead gives Manual teammates. While the lead is in acceptEdits, auto or bypass, an in-process teammate runs in that mode even if its status bar says Manual. In-process teammates follow the lead's live allow/deny rules and CLI flags. Split-pane teammates get `--disallowedTools`/`--tools` (v2.1.309+) but not `--allowedTools`; use `--settings` permissions instead. All teammate permission prompts surface in the lead. Plan mode: a teammate spawned while the lead is in plan mode sends a plan-approval request, and Claude Code auto-approves it when it arrives without the lead reviewing it, unless you switched that teammate's mode manually.
- Status: experimental
- Evidence: https://code.claude.com/docs/en/agent-teams.md (Permissions; Have teammates plan before implementing)

### Worktree isolation enforcement
- What: While a session or subagent is isolated in a worktree, four checks apply: (1) Edit, Write and NotebookEdit targeting the main checkout are blocked; (2) Bash, PowerShell and Monitor commands whose cwd resolves, or cannot be verified not to resolve, to the main checkout are blocked; (3) git redirects into main via `-C`, `--git-dir`, `GIT_DIR`/`GIT_WORK_TREE` or `cd` are blocked; (4) commands whose text cannot be shown to keep git inside the worktree are refused (computed command names, unparsable syntax, `${!name}`, `${ cmd; }`), and Claude is told how to rewrite them. Check 4 cannot be turned off. The docs state that writes by non-git commands (`cp`, redirects) are not tracked and fall to normal permissions and sandbox. PowerShell gets only check 2. Every subagent spawned from an isolated session inherits the checks. Before adopting any directory as a worktree, its git identity is verified: it is refused if its `.git` or `core.worktree` resolves into the main checkout, if it contains the protected checkout, if it has a network-path spelling, or if its metadata is unreadable.
- Status: GA
- Evidence: https://code.claude.com/docs/en/worktrees.md (How Claude Code enforces isolation; Claude Code refuses to use a worktree); https://code.claude.com/docs/en/sub-agents.md (Write subagent files)

### Headless permissions
- What: `--allowedTools` uses rule syntax (`Bash(git diff *)`; the space before `*` matters). In auto mode a bare `Bash` allow is dropped as too broad. From v2.1.302, `-p` starts in Manual mode when nothing sets a mode. A call that needs approval is then denied unless a `PermissionRequest` hook or `--permission-prompt-tool` (an MCP tool) approves it. `--permission-mode auto|dontAsk|acceptEdits|...` sets the baseline. `dontAsk` denies anything that would prompt; mods can still approve some calls; AskUserQuestion, `requiresUserInteraction` MCP tools, org-`ask` connectors and network-path reads are always denied. `--permission-prompts none` (v2.1.259+) stops consulting the host (`canUseTool` / prompt tool), denies anything unresolved, tells Claude not to retry, removes AskUserQuestion and cancels unanswered MCP elicitations. Denials appear as `permission_denied` events and in `permission_denials` in the final result.
- Status: GA
- Evidence: https://code.claude.com/docs/en/headless.md (Auto-approve tools; Turn off permission prompts in unattended runs; Create a commit)

### Cloud network access levels and allowlist
- What: Four levels: None, Trusted (default allowlist of package registries, GitHub, cloud SDKs, container registries; about 200 domains listed), Full, Custom (own list, `*.` wildcard, optional include defaults). Changes apply to running sessions within about a minute. Always reachable regardless of level: GitHub via its proxy, enabled MCP connectors (traffic through Anthropic's servers), network-secret hosts, and the Anthropic API (even at None, noted as a possible exfiltration path). All outbound traffic passes an HTTP/HTTPS security proxy for abuse prevention and rate limiting. No org-level allowlist push; use a shared environment.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cloud-environments.md (Network access; Access levels; Allow specific domains; Security proxy; Default allowed domains); https://code.claude.com/docs/en/claude-code-on-the-web.md (Security and isolation)
- Notes: Bun has known incompatibility with the proxy. Allowlist edits trigger a cache rebuild.

### Auto-mode rule lists from the control plane
- What: The control plane can send auto-mode classifier rule lists (`environment`, `soft_deny`, `allow`), possibly admin-authored. `--server-auto-mode-lists all|no-allow|none` (default `no-allow`, v2.1.295+) decides which the runner applies; default withholds `allow` exceptions but `environment` entries can still loosen.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-reference.md (Auto mode rule lists)
- Notes: A node-side filter over server-sent policy; the docs acknowledge the default doesn't rule out every loosening.

### Computer use: per-app, per-session approvals and app tiers
- What: Enabling the `computer-use` server grants nothing on its own. The first time Claude needs an app in a session, the terminal shows a prompt listing the apps it wants, any extra permissions (for example clipboard), and how many other apps will be hidden. The choices are "Allow for this session" or "Deny". Approvals end with the session. Apps with broad reach carry a warning ("sentinel warnings"): terminals and IDEs count as shell access, Finder can read or write any file, System Settings can change system settings. These apps are warned about, not blocked. The level of control also depends on the app category: browsers and trading platforms are view-only, terminals and IDEs are click-only, everything else gets full control.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/computer-use.md (Approve apps per session; Safety and the trust boundary)
- Notes: The full tier list is said to be on the Desktop page and was not read here. The CLI has no denied-apps list yet; Desktop does.

### Chrome integration: site permissions and prompts
- What: Site-level permissions come from the Chrome extension's own settings. A permission dialog starting "Claude in Chrome wants to" offers to allow all actions on that site for the session. In auto mode, when the auto-mode classifier approves a browser call, the extension skips its own per-site check, unless permission rules deny some site to Claude in Chrome. In plan mode, recording a GIF, opening a tab, running a shortcut, `tabs_context_mcp` with `createIfEmpty`, or a `browser_batch` containing any of those all prompt first. When Chrome connects at start (Enabled by default), VS Code sessions ask before acting on a site you have not allowed, in every mode including Bypass, until you type `@browser`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/chrome.md (Manage site permissions; Permission prompts in VS Code sessions; Browser tools in plan mode)
- Notes: Per the page, Claude pauses at login pages and CAPTCHAs and asks the user to handle them.

### Project settings cannot turn on the browser
- What: Claude Code ignores `CLAUDE_CODE_ENABLE_CFC=1` when it is set in the `env` block of a project's `.claude/settings.json` or `.claude/settings.local.json`, and prints a warning. The stated reason is that a checked-out repository must not be able to connect Claude to the user's browser. Only `--chrome`, `/chrome`, or the user-level "Enabled by default" setting can turn it on.
- Status: GA
- Evidence: https://code.claude.com/docs/en/chrome.md (Project settings can't turn on Chrome)
- Notes: A clear example of a capability that repository-scoped config is not allowed to widen.

### JetBrains acceptEdits risk via IDE config files
- What: In `acceptEdits` mode under a JetBrains IDE, Claude may edit IDE configuration files that the IDE runs automatically. That can sidestep the Bash permission prompts. The docs recommend Manual mode for edits and note that both acceptEdits and auto mode approve edits inside the working directory, except in protected paths.
- Status: GA (documented risk)
- Evidence: https://code.claude.com/docs/en/jetbrains.md (Security considerations)
- Notes: An auto-approved write that turns into code execution through a third party (the IDE).

### Jupyter execution requires native confirmation
- What: `mcp__ide__executeCode` inserts the code as a new cell at the end of the active notebook, scrolls it into view, and shows a native VS Code Quick Pick (Execute or Cancel). Cancel or Esc returns an error. The tool refuses when there is no active notebook, the Jupyter extension is missing, or the kernel is not Python. An allow rule or `PreToolUse` hook only lets Claude propose the run. The Quick Pick is what actually authorises it. JetBrains exposes no code-execution tool.
- Status: GA
- Evidence: https://code.claude.com/docs/en/vs-code.md (Jupyter execution always asks first); https://code.claude.com/docs/en/jetbrains.md (Tools exposed to the model)
- Notes: A second, UI-owned approval layer that the agent's permission system cannot bypass.

### Permission lockdown keys
- What: `allowManagedPermissionRulesOnly` (managed-only) makes managed settings the only source of allow/ask/deny rules: user, project, local, `--settings` rules and `--allowedTools` are ignored, "always allow" choices are hidden, no new rules are saved; on v2.1.282+ it also ignores `allowed-tools` frontmatter in user/project skills and commands. `--disallowedTools` and session deny/ask rules still apply because they only restrict. `permissions.disableBypassPermissionsMode: "disable"` rejects `--dangerously-skip-permissions` and ignores a subagent's `permissionMode: bypassPermissions`. `disableAutoMode: "disable"` removes auto mode from the cycle and starts would-be auto sessions in `default`. `permissions.defaultMode` values `auto` and `bypassPermissions` are ignored from project/local files (since v2.1.257). `permissions.denyInsteadOfPrompting` (user/managed/`--settings` only, v2.1.319+) auto-denies anything that would prompt so unattended sessions keep running; it also removes AskUserQuestion/plan tools, cancels MCP elicitation, and ends an auto-mode turn after 20 consecutive refusals.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-reference.md (allowManagedPermissionRulesOnly; permissions.defaultMode; permissions.denyInsteadOfPrompting; permissions.disableBypassPermissionsMode; disableAutoMode); https://code.claude.com/docs/en/managed-settings.md (Deploy a managed settings file)
- Notes: `skipDangerousModePermissionPrompt` is honoured only from user, local or managed files, so an untrusted repo cannot skip the bypass confirmation. Deny rules block in every mode including bypass.

### Strict-value-wins exceptions to precedence (one-way ratchets)
- What: For a fixed list of security keys a restrictive value from a lower scope beats a managed value: `disableClaudeAiConnectors: true`, `enableArtifact: false`/`disableArtifact: true`, `isolatePeerMachines: true`, `permissions.blockReadsOutsideWorkingDirectories: true` (v2.1.257+), `permissions.blockWritesOutsideWorkingDirectories: true` (v2.1.314+) from any scope; `denyInsteadOfPrompting` and `autoMode.classifyAllShell` true from user or `--settings`; `remoteControlAtStartup: false` from project/local; `crossSessionInbound` stricter on an accept<hold<refuse ladder from project/local; `useAutoModeDuringPlan`, `syncClaudeAiSkills`, `syncClaudeAiPlugins` false from managed/`--settings`/user/local (not the shared project file); `maxEffortLevel` lowest cap from any scope.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings.md (Exceptions to managed settings precedence)
- Notes: Note the deliberate asymmetry: some "off" switches are not accepted from the committed project file, so a repo cannot silently disable a user-chosen feature, while others are accepted from project to let a repo tighten.

### Organization policy controls via managed settings (admin decision map)
- What: Admins enforce policy through managed settings that outrank user/project config. The admin page maps each control to keys: permission rules (`permissions.allow/deny`), lockdown (`allowManagedPermissionRulesOnly`, `permissions.disableBypassPermissionsMode`), starting mode (`permissions.defaultMode`, `permissions.disableAutoMode`), OS sandbox (`sandbox.enabled`, `sandbox.network.allowedDomains`), MCP control (`allowedMcpServers`, `deniedMcpServers`, `allowManagedMcpServersOnly`, `managedMcpServers`, `managed-mcp.json`), plugin marketplace control (`strictKnownMarketplaces`, `blockedMarketplaces`, `disableSideloadFlags`, `disableCommandPluginSources`, `pluginSuggestionMarketplaces`), `strictPluginOnlyCustomization` (skills/agents/hooks/MCP only from plugins or managed), hook restrictions (`allowManagedHooksOnly`, `allowedHttpHookUrls`), `disableAgentView`, `processWrapper`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/admin-setup.md (Decide what to enforce)
- Notes: Page states plainly that denying WebFetch does not stop `curl` via Bash; only the OS sandbox domain allowlist closes that gap. Array settings merge across sources (developers can add, not remove); `fallbackModel`, `availableModels`, `modelPicker` from managed replace lower layers.

### Permission evaluation order (SDK)
- What: Each tool call passes, in order: (1) hooks (can deny; an `allow` does not skip deny/ask rules and cannot approve critical-path `rm`), (2) deny rules (`disallowedTools` plus loaded settings; wins even in `bypassPermissions`), (3) ask rules (fall to `canUseTool` even in bypass; also `AskUserQuestion`, MCP tools with `_meta["anthropic/requiresUserInteraction"]` (v2.1.199+), org `ask` connector tools), (4) permission mode, (5) allow rules (plus calls the tool self-approves, e.g. reads inside working dirs, read-only Bash), (6) `canUseTool` callback. Network-path reads and recognized critical-path removals are never approved by an allow rule.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/permissions.md (How permissions are evaluated)
- Notes: TS `permissionPrompts: 'none'` skips the callback at step 6 and lets a `PermissionRequest` hook decide, else deny (v2.1.259+). TS emits a one-time Node process warning `CLAUDE_SDK_CAN_USE_TOOL_SHADOWED` when `bypassPermissions` or a bare-name `allowedTools` entry means the callback will never see those calls.

### allowedTools / disallowedTools semantics
- What: `allowedTools` pre-approves (does not restrict: unlisted tools stay available and fall through). `disallowedTools` with a bare name removes the tool definition from the request; with a specifier (`Bash(rm *)`) keeps the tool and denies matching calls in every mode (matched as written; `/bin/rm` slips through to the mode). Deny rules accept globs (`*`, `mcp__*`); allow rules accept globs only after a literal `mcp__<server>__` prefix, and unanchored `*` allow entries are ignored. `Edit(path)` rules govern all file-writing tools; `Write(path)` rules match nothing. `//path` = absolute, `/path` = inside the working dir; Edit deny also covers Bash `tee`/redirection but not scripts that open files themselves.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/permissions.md (Allow and deny rules; Path patterns in rules)
- Notes: Docs warn explicitly that `allowedTools` does not constrain `bypassPermissions` and that auto-approved calls never reach `canUseTool`; recommended locked-down pattern is `allowedTools` + `permissionMode: "dontAsk"`.

### Permission modes in the SDK
- What: `default` (needs-approval calls go to `canUseTool`; no callback = deny), `dontAsk` (every would-prompt becomes a deny), `acceptEdits` (Edit/Write/NotebookEdit and `mkdir`/`touch`/`rm`/`rmdir`/`mv`/`cp`/`sed` inside working dirs, excluding protected and critical paths and `touch` with options/globs/prefixes), `bypassPermissions` (all approved except actions no mode auto-approves; TS needs `allowDangerouslySkipPermissions: true`; refuses to start as root/sudo outside a recognized sandbox), `plan` (file-tool edits always reach the callback even if allowed or hook-allowed; when auto mode is available, shell/web/read-only-MCP calls go to the auto classifier, toggled by `useAutoModeDuringPlan`), `auto` (model classifier). Mode can change mid-session with `setPermissionMode()`.
- Status: GA (auto mode availability gated, see permission-modes page)
- Evidence: https://code.claude.com/docs/en/agent-sdk/permissions.md (Permission modes; Mode details); https://code.claude.com/docs/en/agent-sdk/agent-loop.md (Permission mode)
- Notes: From v2.1.302 an SDK app starts in `default` unless set or `permissions.defaultMode` is in settings. Switching an `auto` session to an unsupported model drops it to `default` with a `status` system message (v2.1.299+).

### Subagent permission-mode inheritance
- What: Subagents inherit `plan`, `acceptEdits`, `auto` or `bypassPermissions` from the parent; a subagent asking for `acceptEdits` under a `plan` parent still runs in plan. Under a `default`/`dontAsk` parent the subagent's own mode applies, but a subagent-declared `bypassPermissions` is never applied (v2.1.267+) and `auto` only if available.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/permissions.md (Available modes, Subagent inheritance warning)
- Notes: Designed so delegation cannot escape plan review; but inheriting bypass gives subagents full access.

### canUseTool approval callback and AskUserQuestion routing
- What: Async callback `(toolName, input, options)` that replaces the interactive prompt and pauses the loop until it resolves (no timeout; can stay pending indefinitely). Returns allow (optionally `updatedInput`, silently rewritten input the model is not told about, and `updatedPermissions` to persist a suggested rule, e.g. destination `localSettings` writes `.claude/settings.local.json`) or deny (`message` fed back to the model, optional `interrupt`). TS `options` carry `signal`, `suggestions`, `blockedPath`, `mcpServer` provenance, `decisionReason`, `defaultToNo` (UI must not preselect approve), `suppressAlwaysAllowRule`, `toolUseID`, `agentID`, `requestId`. The same callback receives `AskUserQuestion` calls (1-4 questions, 2-4 options, header <= 12 chars, `multiSelect`); the app returns `answers` keyed by question text, or a free-form `response`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/user-input.md (Detect when Claude needs input; Respond to tool requests; Handle clarifying questions; Limitations); https://code.claude.com/docs/en/agent-sdk/typescript.md (CanUseTool; PermissionResult)
- Notes: Allow results with more than 4,096 permission items are treated as invalid and denied. Returning `null` (v2.1.199+) tells the SDK the app already answered the `control_request` out-of-band (e.g. signed HTTP POST echoing `requestId`); returning null otherwise blocks forever. For long waits the docs recommend a `PreToolUse` hook returning `defer` so the process can exit and resume later. Python examples need a dummy `PreToolUse` hook as a "required workaround" to keep the stream open for `can_use_tool`. `AskUserQuestion` is unavailable inside subagents. TS option previews (`toolConfig.askUserQuestion.previewFormat` = `markdown`|`html`; SDK strips `<script>`, `<style>`, doctype before the callback).

### Programmatic sandbox settings (SDK)
- What: `sandbox` option mirrors the CLI sandbox: `enabled`, `failIfUnavailable` (default true: start fails with `error_during_execution` / `sandbox_required_unavailable`), `autoAllowBashIfSandboxed` (default true), `excludedCommands`, `allowUnsandboxedCommands` (default true: model may set `dangerouslyDisableSandbox`, which falls back to `canUseTool`), network (`allowedDomains`, `deniedDomains`, `strictAllowlist`, `allowLocalBinding`, `allowUnixSockets`, proxy ports), filesystem (`allowWrite`, `denyWrite`, `denyRead`). Linux needs bubblewrap and socat.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (Sandbox Configuration; SandboxNetworkConfig; Permissions Fallback for Unsandboxed Commands)
- Notes: Docs warn that `bypassPermissions` + `allowUnsandboxedCommands` lets the model escape the sandbox silently, that allowing `/var/run/docker.sock` equals host access, that the proxy filters on hostname without TLS inspection (domain fronting possible), and that sandbox network rules do not cover WebFetch.

### Per-MCP-server permission override and custom-tool policies
- What: TS `setMcpPermissionModeOverride(server, 'default'|'auto'|null)` makes one server's tools stricter than the session mode; it only takes effect while the session is in `bypassPermissions` or `auto`, so it can never widen. Custom in-process tools can carry `permission_policy` `always_ask` (allow rules stop approving) or `always_deny` (denied in every mode), added on the config returned by `createSdkMcpServer()` and changeable mid-session via `setMcpServers()` (v0.3.301+, CLI v2.1.301+; older CLIs silently ignore the list).
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (setMcpPermissionModeOverride(); McpServerToolPolicy; Permission modes that can approve an always_ask call); https://code.claude.com/docs/en/agent-sdk/custom-tools.md (Require approval for specific tools)
- Notes: Footgun documented: for `always_ask` calls an unset `destructiveHint` is read as false (contrary to MCP's default true), so the auto classifier can approve them unless `destructiveHint: true` is set; in a cloud session (`CLAUDE_CODE_REMOTE=true`) bypass approves `always_ask` calls without the callback.

### Auto mode milestones
- What: a permission classifier replaces most prompts.
  - Research preview in 2.1.83 (Week 13, March 2026); opened to Pro in Week 21; on Bedrock, Vertex and Foundry in Week 23. The opt-in variable was dropped there in 2.1.207.
  - Default permission mode for new sessions on Pro, Max and Team from August 14, 2026 (https://code.claude.com/docs/en/whats-new/2026-w32.md). Classifier calls stopped counting toward usage limits.
  - Classifier engine: server-side by default on a direct API connection with telemetry off (2.1.282). The local classifier is the default on Bedrock, Vertex and Foundry for now (2.1.273). `CLAUDE_CODE_AUTO_MODE_MODEL` picks the client-side classifier model (2.1.302); the default is Sonnet 5 for external sessions (2.1.210).
  - Over-long conversations are compacted for the classifier instead of every call being denied (2.1.288, 2.1.302).
  - Rules added over time: transcript-tamper block (2.1.205); "Containment Escape" (cloud metadata credentials, egress evasion, cross-tenant reach; 2.1.257); `hard_deny` (2.1.136).
  - Tooling: Auto mode tab in `/permissions` (2.1.246); `/auto-mode-setup` guided setup (2.1.304); `claude auto-mode reset` (2.1.212).
- Status: GA by default since Aug 14, 2026 (the digests no longer label it a research preview)
- Evidence: https://code.claude.com/docs/en/whats-new/2026-w13.md, https://code.claude.com/docs/en/whats-new/2026-w21.md, https://code.claude.com/docs/en/whats-new/2026-w23.md, https://code.claude.com/docs/en/whats-new/2026-w29.md, https://code.claude.com/docs/en/whats-new/2026-w32.md, https://code.claude.com/docs/en/whats-new/2026-w36.md; https://code.claude.com/docs/en/changelog.md (2.1.136, 2.1.205, 2.1.207, 2.1.210, 2.1.212, 2.1.246, 2.1.257, 2.1.273, 2.1.282, 2.1.288, 2.1.302, 2.1.304)
- Notes: the rule label "Containment Escape" appears in no docs page in the dump. Week 36 describes the behaviour without the name.

### Unattended denial instead of prompting
- What:
  - `--permission-prompts none` (2.1.259): for headless hosts, anything that would prompt is denied and the active mode keeps running.
  - `permissions.denyInsteadOfPrompting` (2.1.319): the same in terminal sessions, subagents and teammates; the turn continues.
  - `onFailure: "block"` makes a command or HTTP hook that fails to start, times out, or exits oddly block the action (2.1.295). The same option exists for Agent SDK hook callbacks (2.1.317).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.259, 2.1.319, 2.1.295, 2.1.317)
- Notes: fail-closed hooks only arrived in October 2026. Before that, a failing hook let the action through.

### Sandbox additions
- What:
  - `sandbox.network.strictAllowlist`: deny non-allowlisted hosts without prompting (2.1.219).
  - `sandbox.filesystem.disabled`: network control only (2.1.216).
  - Credential masking: `mode: "mask"` serves sandboxed commands a sentinel copy while the proxy substitutes the real value on egress; options include `extract`, JWT `decode`/`maskClaims`, and AWS SigV4 re-signing (2.1.221, 2.1.224).
  - Per-command `allowed_domains` for Bash, PowerShell and Monitor in auto mode with sandboxing (2.1.271).
  - `sandbox.allowNestedUserNamespaces` on Linux (2.1.305); the changelog says this lets commands get around the sandbox's file protections.
  - `--sandbox` / `--no-sandbox` removed from `claude remote-control` (2.1.300).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.216, 2.1.219, 2.1.221, 2.1.224, 2.1.271, 2.1.300, 2.1.305); https://code.claude.com/docs/en/whats-new/2026-w32.md
- Notes: sentinel-plus-egress-substitution credential masking is the most copyable idea in this period.

### Permission rule matching changes
- What:
  - `Tool(param:value)` deny and ask rules, for example `Agent(model:opus)` (2.1.178, Week 25).
  - "Always allow" rules save at the repository root, so they apply across worktrees (Week 29).
  - Startup warnings for `Write(path)`, `NotebookEdit(path)` and `Glob(path)` rules (2.1.210), and for `Bash(git * main)`-style wildcards (2.1.246).
  - Bash permission checks fail closed on more shell forms, including commands over 10,000 characters (Week 30).
  - A Bash command can no longer hide part of itself from checks, and invisible-Unicode or tab padding no longer hides text in the approval dialog (Week 32).
  - `defaultMode: "bypassPermissions"` in a project's `.claude/settings*.json` is ignored (Week 36).
- Status: GA
- Evidence: https://code.claude.com/docs/en/whats-new/2026-w25.md, https://code.claude.com/docs/en/whats-new/2026-w29.md, https://code.claude.com/docs/en/whats-new/2026-w30.md, https://code.claude.com/docs/en/whats-new/2026-w32.md, https://code.claude.com/docs/en/whats-new/2026-w36.md; https://code.claude.com/docs/en/changelog.md (2.1.210, 2.1.246)
- Notes: these are security hardenings shipped as "Other wins" lines. Several are abuse-case fixes.

### Account-email egress check
- What: before a web or shell tool call sends the user's account email to the network, Claude Code asks. Claude is also told to use the email only to identify the user (2.1.234).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.317, 2.1.234)
- Notes: Nov 3, 2026.

## Area 6. Context management and compaction

### In-session context commands: /clear, /compact, /context, /autocompact, /btw
- What: `/clear [name]` (aliases `/reset`, `/new`) starts an empty context, saving the old session for `/resume`; keeps a user-set name but not a generated title. `/compact [instructions]` replaces history with a focused summary. `/context [all]` shows usage as a colored grid with suggestions and an over-limit warning. `/autocompact [auto|<tokens>]` (v2.1.221+) sets how full the window gets before auto-compaction, saved to user settings; `--autocompact` sets it per session. `/btw` asks a side question without adding it to history. Auto-compaction keeps requests and key snippets; a "Compact Instructions" section in CLAUDE.md steers it; if a single huge output refills context immediately, auto-compaction stops after a few attempts with a thrashing error rather than looping. MCP tool definitions are deferred via tool search so only names and server instructions cost context.
- Status: GA
- Evidence: https://code.claude.com/docs/en/commands.md (/clear, /compact, /context, /autocompact, /btw); https://code.claude.com/docs/en/sessions.md (Manage context within a session); https://code.claude.com/docs/en/how-claude-code-works.md (When context fills up)
- Notes: Compaction detail belongs to another slice; recorded here only as surfaced on these pages.

### /btw side questions
- What: Answers a side question from what is already in context, with no tool access, without adding to history; runs concurrently with the main turn (sees all but the in-progress reply). Replays the newest 20 earlier side exchanges with each ask; thread kept in memory until `x` or exit. `f` forks the question and answer into a background subagent with full tools. With a subagent transcript open, answers from that transcript (v2.1.314+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Side questions with /btw; Ask /btw about a subagent)
- Notes: If the model writes tool calls as text, the answer notes nothing was executed.

### Idle compaction before cache expiry
- What: On the one-hour cache, a session past about 200K tokens that sits idle is compacted shortly before the cache expires, so the user resumes from a summary. Most current models are stated to have a 1M-token window with compaction near the top.
- Status: GA
- Evidence: https://code.claude.com/docs/en/best-practices.md (Keep cost and context down)
- Notes: Setting is `idleCompaction` per the glossary link (https://code.claude.com/docs/en/glossary.md Compaction); not read in detail.

### What loads into context at startup
- What: Before the first prompt the context holds CLAUDE.md (or AGENTS.md), auto memory index, MCP tool names (definitions deferred via tool search by default), skill descriptions, plus optional output style and `--append-system-prompt` text. File reads, path-scoped rules and hook output are appended as work proceeds; subagents keep large reads in their own window and return only a summary plus metadata trailer.
- Status: GA
- Evidence: https://code.claude.com/docs/en/context-window.md (What the timeline shows); https://code.claude.com/docs/en/how-claude-code-works.md (When context fills up)
- Notes: `/context` gives a live per-category breakdown; it adds an estimate for messages since the last response, so it can read higher than the status line.

### Auto-compaction and its window
- What: When the conversation approaches the auto-compact window Claude Code summarises the history. Default: compact at the model's context limit, except cloud sessions (compact as it approaches), Sonnet/Opus 4.6 without extended context (200K), `CLAUDE_CODE_DISABLE_1M_CONTEXT=1` (200K), native-1M models (about 967K), and unrecognised model IDs (assumed 200K). Window is configurable per model via `/autocompact 500k` (saved under `modelSettings`), globally via `autoCompactWindow`, per launch via `--autocompact`, or via `CLAUDE_CODE_AUTO_COMPACT_WINDOW` (highest priority); range 100K to 1M, capped at model window. `autoCompactEnabled: false` or `DISABLE_AUTO_COMPACT` turns it off (manual `/compact` still works). If a single huge output refills context right after each summary, auto-compaction stops after a few attempts with a thrashing error rather than looping.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Context window and auto-compaction; Default auto-compact thresholds); https://code.claude.com/docs/en/settings-reference.md (autoCompactEnabled); https://code.claude.com/docs/en/how-claude-code-works.md (When context fills up)
- Notes: Summarisation request inherits the session's extended-thinking setting (v2.1.198+). Unsuffixed env value is raw tokens (`200` = 200 tokens) while `/autocompact 200` means 200K, a footgun the docs call out. Per-subagent window exists (sub-agents page, not read).

### Manual and partial compaction
- What: `/compact [focus]` summarises with user-chosen focus; a "Compact Instructions" section in CLAUDE.md also steers what is kept. `/rewind` offers "Summarize from here" / "Summarize up to here" for partial compaction. A PreCompact hook can block compaction or supply summary instructions via stdout. `/clear` starts fresh; `/recap` writes a display-only summary without replacing history.
- Status: GA
- Evidence: https://code.claude.com/docs/en/context-window.md (When your context fills up); https://code.claude.com/docs/en/how-claude-code-works.md (When context fills up); https://code.claude.com/docs/en/prompt-caching.md (Running /recap)
- Notes: Blocking an auto-compaction that was triggered to recover from an API context-limit error makes the request fail.

### Idle compaction
- What: Compacts a long conversation you stepped away from so the next message sends a summary instead of re-sending (and re-caching) the whole history. Default on when: interactive terminal, VS Code panel, or Desktop Code tab on local/SSH; auto-compact on; conversation on the one-hour cache TTL; context at least 200,000 tokens (`CLAUDE_CODE_IDLE_COMPACT_MIN_TOKENS`, floor 100,000); about an hour idle and machine awake. Disable with `idleCompaction: false` (v2.1.290+) or `/config` row (v2.1.302+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-reference.md (idleCompaction); https://code.claude.com/docs/en/prompt-caching.md (Compacting the conversation)
- Notes: Shows a notice "Compacted while idle, before the prompt cache expired". Requires feature-flag fetching on versions before v2.1.314.

### What survives compaction
- What: System prompt and output style keep applying; project-root CLAUDE.md, unscoped rules and auto memory are re-read from disk; skill descriptions rebuilt (v2.1.304+); a fresh git status snapshot if the session started with one; the plan-mode plan file is re-injected; up to five most-recently-modified files Claude read/edited are re-read (files over 5,000 tokens come back as a path reference only); invoked skill bodies re-attached at 5,000 tokens each, 25,000 total, newest first; background commands and subagents keep running with a reminder. Path-scoped rules and nested CLAUDE.md are summarised away and reload on demand; hook-added context is summarised; SessionStart hooks with matcher `compact` run again.
- Status: GA
- Evidence: https://code.claude.com/docs/en/context-window.md (What survives compaction); https://code.claude.com/docs/en/memory.md (Instructions seem lost after /compact); https://code.claude.com/docs/en/skills.md (Skill content lifecycle)
- Notes: Docs advise putting the most important skill instructions at the top because truncation keeps the start.

### Prompt cache layout (prefix ordering)
- What: Every turn re-sends the full context; the API caches by exact prefix match, so any change recomputes everything after it. Claude Code orders requests as system prompt + tool definitions, then project context (CLAUDE.md, auto memory, unscoped rules), then conversation. Plan mode, skill loading, output-style switches and file-change notices are appended as messages so the prefix stays intact. Model, speed (fast mode) and, on most models, effort level each have separate caches.
- Status: GA
- Evidence: https://code.claude.com/docs/en/prompt-caching.md (How the cache is organized; Actions that keep the cache)
- Notes: Effort changes keep the cache on Fable 5.1+, Opus 5.5, Sonnet 5.5, Haiku 5.5 with API key or subscription (not Bedrock/Agent Platform/gateway/HIPAA). The advisor tool's definition sits after the cache breakpoint so toggling it is cache-safe.

### Actions that invalidate the cache
- What: Switching models (including `opusplan` plan toggles, automatic model fallback, a skill's `model` frontmatter for that turn); changing effort on most models; toggling fast mode; adding/removing tool definitions when tool search is off (with tool search on, the first request's tool list is frozen and late servers arrive as deferred tools); denying an entire tool by bare name when tool search is off; compaction (conversation layer only); pruning old images/PDFs when per-request limits are hit (done in batches so there is one slow turn per batch); upgrading Claude Code (applied on next launch only). `/model` asks for confirmation only while the cache is warm.
- Status: GA
- Evidence: https://code.claude.com/docs/en/prompt-caching.md (Actions that invalidate the cache)
- Notes: Plugin skills, commands, agents, hooks, monitors and themes never invalidate the cache; they are appended. `/reload-plugins` refuses a reload that would force a full re-read unless `--force`. Disabling a plugin restores the earlier request shape and can hit the older cache entry.

### Actions that keep the cache
- What: Editing repository files (a `<system-reminder>` notes the change; earlier reads are not rewritten), editing CLAUDE.md mid-session (does not apply until `/clear`, `/compact` or restart), changing permission mode, switching output style, invoking skills/commands, `/recap`, `/rewind` (truncates back to an already-cached prefix), spawning a subagent (separate cache; parent untouched). A conversation fork reads the parent's cache.
- Status: GA
- Evidence: https://code.claude.com/docs/en/prompt-caching.md (Actions that keep the cache; Subagents and the cache)
- Notes: Nested CLAUDE.md and path-scoped rules edited before they load do take effect.

### Prompt cache TTL selection
- What: Two buckets: main conversation, and everything else (subagents, workflows, teammates, forks, compaction, titles). Default 1h for the main conversation only on a Claude subscription within plan usage; 5m otherwise, and 5m as soon as usage credits are consumed. Overrides: `promptCacheTtl` / `CLAUDE_CODE_PROMPT_CACHE_TTL` and `subagentPromptCacheTtl` / `CLAUDE_CODE_SUBAGENT_PROMPT_CACHE_TTL` (`5m` or `1h`, v2.1.242+). Precedence: `FORCE_PROMPT_CACHING_5M` > bucket env > bucket setting > subagent `experimental.cacheTtl` > `ENABLE_PROMPT_CACHING_1H` > default.
- Status: GA
- Evidence: https://code.claude.com/docs/en/prompt-caching.md (Cache lifetime; Which TTL each request gets; Choose the TTL yourself)
- Notes: Verify with `claude -p "hello" --output-format json` reading `usage.cache_creation.ephemeral_1h_input_tokens`. Workflow fan-outs hold all but the first same-prefix agent up to 5 s so they can read its cache.

### Cache scope and gateways
- What: The cache is effectively per machine and directory because the system prompt embeds auto memory paths and the conversation opens with cwd, platform, shell and OS; sequential sessions share only if the startup git status snapshot matches. Through a gateway: if `cache_control` markers are forwarded, caching works; if the gateway returns 400 naming `cache_control`, Claude Code moves the marker to the last message for the rest of the run; if it silently strips markers, the whole history bills uncached every turn. Caching can be disabled per model family via `DISABLE_PROMPT_CACHING*` env vars.
- Status: GA
- Evidence: https://code.claude.com/docs/en/prompt-caching.md (Where the cache lives; Cache scope; Disable prompt caching)
- Notes: SDK fleets can move auto memory location out of the system prompt to share caches across machines (SDK page, not read).

### MCP output limits and persisting large results to disk
- What: Warning above 10,000 tokens per MCP tool output (fixed); limit 25,000 tokens default (`MAX_MCP_OUTPUT_TOKENS`). Successful non-image results over the limit, or over 50,000 characters regardless of tokens, are saved to the session's `tool-results` dir under `~/.claude/projects/` and replaced by a message naming the path, so Claude reads it on demand. Tools can raise their own threshold with `_meta["anthropic/maxResultSizeChars"]` up to a hard ceiling of 500,000 characters (images still bound by tokens). `isError` text over about 11,000 chars keeps first 5,000 and last 5,000 with a removed-count marker. HTTP/SSE response bodies (or single SSE events) over 16 MiB after decompression fail; a WebSocket message over 16 MiB closes the connection. Images (PNG/JPEG/GIF/WebP) are shown inline (possibly downscaled) and the original bytes saved to `tool-results` with the path given to Claude (v2.1.283+, not with `--no-session-persistence`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (https://code.claude.com/docs/en/mcp.md output limits and warnings; Raise the limit for a specific tool; Images in tool results)

### MCP tool search (deferred tool definitions) and thresholds
- What: On by default: only tool names and server instructions load at session start; full definitions load on demand through a `ToolSearch` tool, so there is no fixed per-server tool cap. `ENABLE_TOOL_SEARCH`: unset/`true` defer all; `auto` loads upfront while definitions total under 10% of the context window and switches to search at 10% or more (stays on once on); `auto:N` custom percent; `false` loads all upfront. Denying `ToolSearch` in permissions or leaving it out of `--tools` also loads upfront. Model gating: Opus 4 / Sonnet 4 / Haiku 4.5 and later (on Google Agent Platform only 4.5 generation and later); off under `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS` (managed settings can keep it on, v2.1.227+) and under HIPAA configuration; Azure-hosted Foundry rejects it server-side and Claude Code falls back to upfront. Behind an `ANTHROPIC_BASE_URL` gateway the gateway must forward `defer_loading` and `tool_reference`; on by default behind gateways since v2.1.304, with automatic retry without tool search after 400/422 on v2.1.305+.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Scale with https://code.claude.com/docs/en/mcp.md tool search; Configure tool search; Tool search through an LLM gateway; Tools that stay deferred below the threshold)
- Notes: `/context` labels tools `(deferred)` / `(loaded on-demand)`. QS notes every server's tool names and instructions still cost context in every session.

### Context management in cloud sessions
- What: `/compact` (with focus text), `/context`, `/clear` work. Cloud sessions set `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` themselves so compaction triggers partway through the auto-compact window; a user-set value is overridden. `CLAUDE_CODE_AUTO_COMPACT_WINDOW` or `/autocompact` changes the window. Subagents behave as locally; `.claude/agents/` loads. Agent teams need `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`.
- Status: GA (agent teams: experimental)
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (Manage context)
- Notes: The override percentage itself is not stated.

### Auto-compact window per model
- What: The auto-compact window is the fill level at which the conversation is compacted. Set per model with `/autocompact <size>` (saved under `modelSettings` in user settings), for all models with `autoCompactWindow`, per launch with `--autocompact`, or via `CLAUDE_CODE_AUTO_COMPACT_WINDOW` (highest precedence). Accepted range 100K-1M tokens (plain count, `k`/`M` suffix, or bare 100-1000 meaning thousands for the command/flag only; the env var reads a bare number as tokens). Always capped at the model's context window. Defaults: compact at the model limit; native-1M models compact at about 967K; 4.6 models without `[1m]` at 200K; cloud sessions compact as the limit approaches. Warnings quote out-of-range env values and say which window is used instead.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Context window and auto-compaction; Set the auto-compact window; Default auto-compact thresholds; Values and warnings for CLAUDE_CODE_AUTO_COMPACT_WINDOW)
- Notes: Per-model saving since v2.1.288 (previously one global value). Status line `used_percentage` measures against the full window, not the compaction threshold. `idleCompaction` setting also exists (not read in detail). Per-subagent window exists (sub-agents.md, not read in detail).

### Context window handling for unknown / gateway model IDs
- What: For a model ID it does not recognise, Claude Code assumes 200K (1M if the ID carries `[1m]`) and shows a startup warning. Fixes: update, map with `modelOverrides` or a `modelPicker` row with `behavesAs`, append `[1m]`, or set `CLAUDE_CODE_MAX_CONTEXT_TOKENS`. The latter applies directly only to unrecognised non-`claude-` spellings without `[1m]`; for recognised IDs it needs `DISABLE_COMPACT`. `CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT=1` makes it compact only after the API returns a recognised too-long error. Behind a gateway, known models get their Anthropic API window; Claude Code cannot detect a lower gateway limit, so set `CLAUDE_CODE_AUTO_COMPACT_WINDOW=200000`. A mistyped ID close to a known alias gets a "did you mean" warning but is still sent as written.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Correct the window for a gateway or custom model ID; Which IDs CLAUDE_CODE_MAX_CONTEXT_TOKENS applies to; Context window behind an LLM gateway; Fix a mistyped model name)
- Notes: The rules are intricate (three cases by ID shape); a harness builder should note this is the cost of matching model IDs by pattern. A gateway that rewrites the too-long error breaks reactive compaction (llm-gateway-connect.md troubleshooting table).

### Tool search (deferred tool schemas)
- What: On by default: MCP tools and non-core built-ins are withheld from context; the model sees a compact name list, calls `ToolSearch`, and up to 5 matching tools load and stay until the turn where they were discovered is compacted. `ENABLE_TOOL_SEARCH` = unset/`true` (on), `auto` / `auto:N` (activate when deferrable definitions reach 10% / N% of context), `false` (all upfront). Core tools (Bash, Read, Edit) always load. Per-tool/server `alwaysLoad` exempts. Limits: 10,000 tools in the catalog.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/tool-search.md (How tool search works; Configure tool search; Optimize tool discovery; Limits); https://code.claude.com/docs/en/agent-sdk/agent-loop.md (What consumes context)
- Notes: Docs quantify: 50 tools ~10-20K tokens; selection accuracy degrades beyond 30-50 loaded tools; below ~10 tools upfront is faster. Forced off on pre-Opus 4/Sonnet 4 models, older Vertex models, Azure-hosted Foundry, `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS`, and HIPAA sessions. Search matches names and descriptions, so naming advice is given.

### Compaction and context control in the SDK
- What: Automatic compaction summarizes older history near the limit and emits `compact_boundary`; `/compact` can be sent as a prompt; `PreCompact` hook (trigger manual/auto) and TS `PostCompact`. CLAUDE.md is re-injected every request, and the compactor reads CLAUDE.md, so a free-form "what to preserve" section steers summaries. `/clear` resets context (old conversation stays on disk, resumable). TS subagents can lower their auto-compact window (`autoCompactWindow`). `seedReadState()` (TS) marks a file read so Edit works after the app pruned an earlier Read.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/agent-loop.md (Automatic compaction; Keep context efficient); https://code.claude.com/docs/en/agent-sdk/skills.md (Compact history with /compact; Reset context with /clear); https://code.claude.com/docs/en/agent-sdk/typescript.md (seedReadState())
- Notes: Compaction can lose early instructions; docs recommend persistent rules in CLAUDE.md rather than the first prompt.

### Idle compaction
- What: a conversation of 200K+ tokens left idle about 54 minutes is compacted before its one-hour prompt cache expires, so the next message does not reprocess everything.
  - Terminal: 2.1.299, Oct 14, 2026.
  - VS Code and the Desktop Code tab: 2.1.314; the summary is used if the cache has expired.
  - Controls: `CLAUDE_CODE_IDLE_COMPACT_MIN_TOKENS`, `"idleCompaction": false`, and a `/config` row (2.1.302).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.299, 2.1.302, 2.1.314)
- Notes: covered in model-config.md, env-vars.md and settings-reference.md. Tied to cache-TTL economics.

### Prompt cache controls and diagnostics
- What:
  - `promptCacheTtl` / `subagentPromptCacheTtl` (`5m` / `1h`; 2.1.243) and per-agent `experimental.cacheTtl` (2.1.248).
  - `ENABLE_PROMPT_CACHING_1H` (2.1.108; the `_BEDROCK` variant is deprecated).
  - `/cost` prompt-cache line: hit ratio, misses, warm or cold, and a likely cause for a miss (2.1.251, 2.1.260). A `prompt_cache` status-line object.
  - OTel splits cache writes into 5m and 1h tokens (2.1.300).
  - Resume warns Pro/Max users when resuming an idle conversation would use a noticeable share of the 5-hour limit (2.1.304).
- Status: GA (`cacheTtl` is under `experimental.`)
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.108, 2.1.243, 2.1.248, 2.1.251, 2.1.260, 2.1.300, 2.1.304); https://code.claude.com/docs/en/whats-new/2026-w35.md, https://code.claude.com/docs/en/whats-new/2026-w36.md

### Compaction tweaks
- What:
  - `autoCompactWindow` in subagent frontmatter, so a subagent compacts earlier (2.1.296).
  - Switching to a smaller-context model summarizes first with the previous model (2.1.301).
  - The compaction summary lists the tools called earlier (2.1.304).
  - The Rewind menu offers "Summarize up to here" (Week 20).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.296, 2.1.301, 2.1.304); https://code.claude.com/docs/en/whats-new/2026-w20.md

## Area 7. Memory and persistent instructions

### Memory surfaces as stated on core pages
- What: CLAUDE.md (project `./CLAUDE.md` or `./.claude/CLAUDE.md`, user `~/.claude/CLAUDE.md`, managed policy) are concatenated broadest to most specific and delivered as a user message after the system prompt; project-root CLAUDE.md is re-read from disk after compaction. AGENTS.md is read when no CLAUDE.md exists (v2.1.277+; `/config` Project instructions can read both). Auto memory: notes Claude writes per git repo under `~/.claude/projects/<project>/memory/`, shared by worktrees; first 200 lines or 25 KB of `MEMORY.md` load each session. `.claude/rules/*.md` with `paths:` load only when matching files are touched. `/memory` edits these and toggles auto memory.
- Status: GA
- Evidence: https://code.claude.com/docs/en/glossary.md (CLAUDE.md; AGENTS.md; Auto memory; Rules); https://code.claude.com/docs/en/how-claude-code-works.md (What Claude can access)
- Notes: Memory detail belongs to another slice.

### CLAUDE.md scopes and load order
- What: Four scopes, loaded broadest first: managed policy (`/etc/claude-code/CLAUDE.md` on Linux/WSL, `/Library/Application Support/ClaudeCode/CLAUDE.md` macOS, `C:\Program Files\ClaudeCode\CLAUDE.md`), user `~/.claude/CLAUDE.md`, project `./CLAUDE.md` or `./.claude/CLAUDE.md`, local `./CLAUDE.local.md`. Files in cwd and every ancestor load at launch and are concatenated (not overriding), root-down, with `CLAUDE.local.md` after `CLAUDE.md` at each level. Subdirectory CLAUDE.md files load on demand the first time Claude reads/writes/edits a file there (Bash `cat`/`head` count as reads). Content is delivered as a user message after the system prompt, not in it.
- Status: GA
- Evidence: https://code.claude.com/docs/en/memory.md (Choose where to put CLAUDE.md files; How CLAUDE.md files load; Claude isn't following my CLAUDE.md)
- Notes: Recommended under 200 lines per file. Launch-time files over 4 MiB are skipped entirely; on-demand files (subdir CLAUDE.md, their imports, subdir rules, path-scoped rules) are truncated at 40,000 chars on 200K windows, 150,000 or 200,000 chars on 1M windows depending on model, with a note telling Claude to Read the rest (v2.1.309+). Block-level HTML comments are stripped before injection. Startup and `/status` warn on oversize files and on combined size.

### CLAUDE.md imports
- What: `@path` anywhere in a CLAUDE.md expands the file inline; relative paths resolve against the importing file; recursion up to four hops; backslash-escape spaces; code spans and fenced blocks are skipped. Imports outside the working directory in project-level files trigger a one-time approval dialog per project; declining disables them permanently. User-scope files' imports load without a dialog (except Cowork desktop sessions, which skip out-of-cwd imports and symlinked user files).
- Status: GA
- Evidence: https://code.claude.com/docs/en/memory.md (Import additional files)
- Notes: Imports do not reduce context cost. Symlinks to network paths (UNC, `/net`, `/Network`) are never followed because the lookup could contact the host.

### Rules directory with path scoping
- What: `.claude/rules/**/*.md` (recursive) are modular instruction files. Without frontmatter they load at launch with `.claude/CLAUDE.md` priority. With `paths:` globs (only frontmatter field read) they load when Claude uses Read/Write/Edit, or a read-counting Bash command, on a matching file. Patterns are relative to the directory containing `.claude` (project) or the primary working directory (user `~/.claude/rules/`, loaded before project rules). Brace expansion is budgeted at 1,000 expanded patterns and 4 MiB per rule. Symlinked rule dirs/files are supported, with cycle detection; targets outside the working directory need the external-import approval and then only unscoped ones load.
- Status: GA
- Evidence: https://code.claude.com/docs/en/memory.md (Organize rules with .claude/rules/; Path-specific rules; Rule frontmatter reference; Share rules across projects with symlinks; User-level rules)
- Notes: Malformed YAML makes the rule load unscoped. Rules respect `--setting-sources` (fixed for on-demand rules in v2.1.211). Path-scoped rules do not persist across compaction until a matching file is touched again.

### Managed CLAUDE.md and exclusions
- What: Orgs deploy a managed CLAUDE.md file or put content inline via the `claudeMd` key in managed settings (ignored elsewhere). It loads before user/project and cannot be excluded. `claudeMdExcludes` (glob on absolute paths, arrays merge across user/project/local/managed) skips other CLAUDE.md and rules files, e.g. other teams' packages in a monorepo; for symlinked rules either the link path or target matches (v2.1.239+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/memory.md (Deploy organization-wide CLAUDE.md; Exclude specific CLAUDE.md files); https://code.claude.com/docs/en/large-codebases.md (Exclude irrelevant CLAUDE.md files)
- Notes: Docs are explicit that CLAUDE.md is guidance, not enforcement; enforcement belongs in managed settings (`permissions.deny`, `sandbox.enabled`).

### AGENTS.md support
- What: Implemented as a built-in plugin `cc-plugin-agents-md@builtin` (v2.1.277+). Default mode `claude-md-or-agents-md`: reads `AGENTS.md` and `.claude/AGENTS.md` in cwd and ancestors only when no `CLAUDE.md`, `.claude/CLAUDE.md` or `CLAUDE.local.md` exists there (user and managed CLAUDE.md and rules do not count); subdirectory `AGENTS.md` loads on Read when that subdir lacks CLAUDE.md files. Other modes: `claude-md-and-agents-md` (both, dedup), `claude-md`, `managed-only`. Set via `/config` "Project instructions" or `pluginConfigs` in user/`--settings`/managed settings only. Not read: `AGENTS.local.md`, `AGENTS.override.md`, `.agents/`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/memory.md (AGENTS.md; Choose which instruction files load; Where AGENTS.md differs from CLAUDE.md)
- Notes: AGENTS.md read directly does not fire InstructionsLoaded, is not loaded from `--add-dir` dirs, and its external imports load only if already approved. Plugin ID was `agents-md@builtin` before v2.1.285. Unavailable in some first-session-after-upgrade cases. `/init` reads Cursor/Copilot rules (and with `CLAUDE_CODE_NEW_INIT=1`, AGENTS.md, Devin, Windsurf, Cline); `/import` copies another agent's config once.

### CLAUDE.md from additional directories
- What: `--add-dir`/`/add-dir` directories grant file access and load their skills, but their CLAUDE.md, `.claude/CLAUDE.md`, rules and CLAUDE.local.md load only with `CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD=1`. `permissions.additionalDirectories` in settings grants file access only and loads nothing.
- Status: GA
- Evidence: https://code.claude.com/docs/en/memory.md (Load from additional directories); https://code.claude.com/docs/en/large-codebases.md (Grant access across packages or repositories)
- Notes: Project `.claude/settings.json` is not inherited from parent directories, unlike CLAUDE.md.

### Auto memory
- What: Claude writes its own notes, typed in frontmatter as `user`, `feedback`, `project` or `reference`, skipping anything derivable from code or already in CLAUDE.md. Stored at `~/.claude/projects/<project>/memory/` where `<project>` derives from the git repo (shared across worktrees and subdirs; machine-local, not synced). Layout: `MEMORY.md` index (one line per memory) plus topic files read on demand with normal file tools. First 200 lines or 25KB of `MEMORY.md` load each session; after a write Claude Code checks the limits, nudges Claude to shorten when near, and returns an error telling it to rewrite the index when over. Writes to frontmatter files get a `modified` ISO 8601 timestamp. On by default locally; toggle in `/memory` (`autoMemoryEnabled`), `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`, relocate with `autoMemoryDirectory` (absolute or `~/`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/memory.md (Auto memory; Storage location; How it works)
- Notes: Memory files are exempt from the `cleanupPeriodDays` transcript sweep. Off by default in self-hosted environments. The toggle cannot re-enable memory inside a background session or a Claude-spawned `claude`. Not loaded into subagents (except forks); subagents can have their own via a `memory` field. Under `blockReadsOutsideWorkingDirectories`, a repo-chosen memory dir is neither read nor written.

### Memory management commands
- What: `/memory` lists CLAUDE.md/CLAUDE.local.md and memory locations (including not-yet-existing ones, created on select), toggles auto memory and opens the folder in your editor. "Remember X" goes to auto memory; "add this to CLAUDE.md" goes to the file. `/init` generates or improves a CLAUDE.md (interactive multi-phase flow with `CLAUDE_CODE_NEW_INIT=1` that can also set up skills and hooks). `/doctor prompt-audit [path]` (v2.1.283+) audits CLAUDE.md, AGENTS.md, rules, skills, commands, subagents, output styles for stale/conflicting instructions and proposes edits. `/context all` lists loaded memory files.
- Status: GA
- Evidence: https://code.claude.com/docs/en/memory.md (View and edit with /memory; Set up a project CLAUDE.md; Audit your instruction files)
- Notes: prompt-audit runs through the bundled `/claude-api` skill and is unavailable if that skill is disabled.

### Subagent persistent memory
- What: `memory: user|project|local` gives the agent a directory: `~/.claude/agent-memory/<name>/`, `.claude/agent-memory/<name>/` or `.claude/agent-memory-local/<name>/`. Its prompt then gets read/write instructions plus the first 200 lines or 25KB of that directory's `MEMORY.md`, and Read/Write/Edit are force-enabled. The docs recommend `project`, so the memory is shared through version control. It is tied to auto memory: if auto memory is disabled, the field does nothing.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Enable persistent memory)

### REVIEW.md customisation and its trust rules
- What: `REVIEW.md` is free-form Markdown that holds review-only instructions. Both the finder and the verifier agents receive it, and the step that writes the summary consults it. Typical uses: redefine Important, cap nits, skip paths or categories, add rules, require `file:line` evidence for a class of claim. Code Review also reads the root `CLAUDE.md` plus the `CLAUDE.md` in each parent directory of every changed file. The local `/code-review` command reads `CLAUDE.md` but not `REVIEW.md`. Subdirectory `REVIEW.md` files add checks but do not apply under test, example, generated, or vendored directory names (an explicit list, matched as whole names in any case). Subdirectory files share a 150,000-byte limit.
- Status: GA
- Evidence: https://code.claude.com/docs/en/code-review-customize.md (Use CLAUDE.md for project-wide rules; Use REVIEW.md for review-only rules; When a subdirectory REVIEW.md doesn't apply; Choose what to write in REVIEW.md)
- Notes: A cap like "at most five Nits" is documented as honoured.

### REVIEW.md branch-protection trust model and includes
- What: `REVIEW.md` is read only from the base or default branch, never from the PR's own changes. How much of it is honoured depends on branch protection:
  - Protected default branch, or a base branch protected by a ruleset: the root file, its includes, and most subdirectory files apply.
  - Unprotected default and base: only the root file.
  - Protection cannot be confirmed: nothing.
  Lowering severity, excluding paths, and capping nits are honoured only from a protected default branch. "Protected" means a classic rule or a ruleset on the default branch, and a ruleset only on other branches. The ruleset must restrict updates or require PRs with at least one approval that is dismissed on a new push, or that must come after the last push.
  Includes: a line with only `@file` pulls in a file from the repo root. The limits are 8 files, 150,000 bytes combined (a file that would cross the limit is skipped whole), and one level deep. Symlinks are not followed. More than 64 Markdown links (http, mailto, and anchors excluded) causes the whole root `REVIEW.md` to be skipped, and so does a link to a file the PR modifies when it cannot be cut out.
  A skill can be included as `@.claude/skills/<folder>/SKILL.md` (30,000-byte limit, at most 8 skill lines). Only its body is used: `!` commands become placeholders and `$ARGUMENTS` stays as written. Some characters are rewritten (for example backticks to ˋ, `|` to ¦, `[`→`(`, `@alice`→`@ alice`). A skill's text cannot be the reason to exclude findings or cap nits.
- Status: GA
- Evidence: https://code.claude.com/docs/en/code-review-customize.md (Which branch REVIEW.md is read from; What counts as branch protection; What a review reads for each branch setup; Include other files in REVIEW.md; Include a skill file; Characters that Code Review changes in skill text); https://code.claude.com/docs/en/code-review-troubleshooting.md (The check run mentions REVIEW.md)
- Notes: A carefully designed defence against a PR author weakening the review by editing its instructions. The character rewriting looks like a defence against @-mention spam or markup injection, but the page does not say so.

### Claude Tag repo config, memory and personal connectors
- What: Sessions start with no repo. When the message names a repository granted to the channel, it is cloned, and `CLAUDE.md`, `.claude/CLAUDE.md`, `.claude/rules/*.md`, and `.claude/skills/` load. Repo hooks in `.claude/settings.json` do not run. `.mcp.json` is not loaded. Local `~/.claude`, personal `settings.json`, and environment variables never reach the session. Admin counterparts: per-channel default model (switchable in a thread), `!fast` if allowed, environment variables and setup script on the environment (readable in every session, so not for secrets), and secrets as connections. Effort is not configurable. Sessions always run in auto mode (the classifier reviews each action), with admin "auto mode allow rules" replacing interactive prompts. Memory: notes per channel; workspace notes written only from public channels and read everywhere; anyone in the channel can correct them. Personal connectors are used in a channel only for the owner's own task after the owner allows it, and others' messages in that thread are treated as information, not instructions. On Enterprise, admins can require the owner to review results before they post.
- Status: public beta
- Evidence: https://claude.com/docs/claude-tag/concepts/for-claude-code-users.md (What happens when a session starts; Admin counterparts for local settings; Whose credentials a session uses); https://claude.com/docs/claude-tag/concepts/how-it-works.md (Channel and workspace memory); https://claude.com/docs/claude-tag/concepts/security-and-data.md (Service accounts)

### settingSources and filesystem configuration loading
- What: `settingSources` (default all = `["user","project","local"]`) controls loading of settings.json, hooks, CLAUDE.md and `.claude/rules/*.md` (cwd and all parents), CLAUDE.local.md, and `.claude/` skills, commands, agents (cwd up to repo root, plus `additionalDirectories`). `[]` gives a programmatic-only agent. Read regardless: managed policy (endpoint and server-managed), `~/.claude.json`, auto memory at `~/.claude/projects/<project>/memory/` (written by the agent with plain Write/Edit), claude.ai connectors (when logged in), and sandbox credential deny/mask entries. Precedence: managed > programmatic options (`settings`, `agents`, `allowedTools`) > local > project > user.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/claude-code-features.md (Control filesystem settings with settingSources; What settingSources does not control; Project instructions); https://code.claude.com/docs/en/agent-sdk/typescript.md (SettingSource; Settings precedence); https://code.claude.com/docs/en/agent-sdk/migration-guide.md (Settings sources default)
- Notes: CLAUDE.md levels are additive with no hard precedence; conflicts are left to the model. Python SDK 0.1.59 and earlier treated `[]` as "omitted". The page warns explicitly not to rely on default options for multi-tenant isolation. TS `projectConfigRoot` (v2.1.275+) reads project config from a trusted checkout while `cwd` is a worktree. TS `resolveSettings()` (alpha) returns the merged settings with per-key provenance without spawning the CLI.

### System prompt options and caching
- What: Default is a minimal prompt (tool calling only, no safety instructions), unlike `claude -p`. Options: `claude_code` preset (+`append`), custom string, TS array with `SYSTEM_PROMPT_DYNAMIC_BOUNDARY` (static part cached separately; only on the direct API or Claude Platform on AWS), Python `{"type":"file"}` (string prompts go on argv: ~128 KB Linux arg limit, ~32 KB Windows). `excludeDynamicSections` moves per-user context (auto-memory path) into the first user message so fleets share a cache entry. The system prompt is recorded on a session's first request and reused until compaction (`snapshot: false` to rebuild each request). Output styles (`~/.claude/output-styles/`, `.claude/output-styles/`, `keep-coding-instructions`) activated via `settings.outputStyle`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/modifying-system-prompts.md (How system prompts work; Output styles; Append; Improve prompt caching; Custom system prompts; Change the prompt of an existing session); https://code.claude.com/docs/en/agent-sdk/python.md (SystemPromptFile)
- Notes: To change instructions mid-session, send them in the next user message or via hook `additionalContext`, not by changing `systemPrompt`.

### System reminders injected outside the system prompt
- What: Claude Code adds context as conversation messages (in `<system-reminder>` tags or system-role messages): CLAUDE.md content (introduced as overriding defaults), output style, commit/PR attribution, hook `additionalContext`, skill and subagent lists, task-list nudges, file-changed notes. They are not in the SDK message stream; see them via `OTEL_LOG_RAW_API_BODIES=file:<dir>` or a logging gateway. Switches: `includeGitInstructions: false` / `CLAUDE_CODE_DISABLE_GIT_INSTRUCTIONS`, `attribution.commit/pr`, `CLAUDE_CODE_DISABLE_CLAUDE_MDS`, `CLAUDE_CODE_DISABLE_ATTACHMENTS`, `CLAUDE_CODE_DISABLE_FILE_MENTIONS`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/modifying-system-prompts.md (Context Claude Code adds outside the system prompt; Turn off the context your agent replaces; See what Claude received)
- Notes: Docs tell custom-prompt authors to explain what a system reminder is, or the model cannot tell app-injected text from user text.

### AGENTS.md fallback
- What: in a project with no CLAUDE.md, Claude Code reads AGENTS.md instead. This is configurable under "Project instructions" in `/config`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.277)
- Notes: Sep 18, 2026. Also `omitClaudeMd` in agent frontmatter (2.1.271) and an ISO `modified` timestamp in memory frontmatter (2.1.214). The `#` quick-memory shortcut was removed in 2.0.70 (Dec 15, 2025).

### CLAUDE.md hygiene tools
- What:
  - `/doctor` proposes trimming checked-in CLAUDE.md content that Claude could derive from the codebase (2.1.206), and dedupes local against checked-in files (Week 28).
  - `/doctor prompt-audit` (alias `/checkup prompt-audit`) audits CLAUDE.md, skills, agents and commands for prompting patterns written for older models (2.1.283).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.206, 2.1.283); https://code.claude.com/docs/en/whats-new/2026-w28.md

## Area 8. Planning, tasks and modes

### Task list (to-do checklist) display
- What: `Ctrl+T` toggles Claude's checklist (up to five shown) in sessions that have the task tools; tasks persist across compaction; expanded state restored on resume; `CLAUDE_CODE_TASK_LIST_ID=<name>` shares one list across sessions via `~/.claude/tasks/<name>`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Task list)
- Notes: Separate from `/tasks` (background work view).

### Plan mode entry points
- What: `claude --permission-mode plan`, `/plan [description]`, or `Shift+Tab` until `⏸ plan mode on`; Claude reads and proposes without edits until approval via `ExitPlanMode` (which requires a prompt). Plan files are written to `~/.claude/plans/`. Accepting a plan renames the session after the plan.
- Status: GA
- Evidence: https://code.claude.com/docs/en/common-workflows.md (Plan before editing); https://code.claude.com/docs/en/commands.md (/plan); https://code.claude.com/docs/en/claude-directory.md (Cleaned up automatically); https://code.claude.com/docs/en/sessions.md (Name your sessions)
- Notes: `/ultraplan` (cloud planning) was removed.

### Completion enforcement: prompt, /goal, Stop hook, fresh reviewer
- What: Docs rank ways to make Claude verify: ask in the prompt; `/goal <condition>` where a separate model checks the condition after each turn and sends Claude back; a `Stop` hook whose exit code 2 blocks ending the turn with stderr fed back as the reason (overridden after eight consecutive blocks with no tool call between; `CLAUDE_CODE_STOP_HOOK_BLOCK_CAP`); `/code-review` in a fresh subagent that never saw the conversation. `prompt`/`agent` hooks are model-judged and not deterministic; `onFailure: "block"` makes a crashing or timed-out command/http hook block the action.
- Status: GA
- Evidence: https://code.claude.com/docs/en/best-practices.md (Give Claude a check to run; Enforce the check with a Stop hook; Enforce steps with hooks)
- Notes: The page frames instructions as non-deterministic (prompt, CLAUDE.md, skills) versus deterministic (hooks, deny rules, managed settings, sandbox strict mode).

### Plan mode gating and plan approval
- What: Plan mode lets Claude read and run exploratory shell commands but blocks source edits until a plan is approved. During planning: if auto mode is available and `useAutoModeDuringPlan` (default on), the classifier reviews shell commands; otherwise non-read-only commands prompt (even with sandbox auto-allow). Approval options: "Yes, and use auto mode" (or "auto-accept edits" if auto unavailable, or bypass if launched with it), "Yes, manually approve edits", "No, keep planning"; Ctrl+G edits the plan in an editor; optional clear-context-on-accept. Accepting generates a session title.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Analyze before you edit with plan mode; Review and approve a plan; Set plan mode as the default)
- Notes: In interactive terminal sessions with bypass available, plan-mode blocks are not enforced (Claude is only instructed not to edit); headless, SDK and VS Code chat keep the blocks.

### Permission modes in cloud sessions
- What: Cloud sessions offer Auto (classifier reviews actions), Accept edits, Plan; no Manual and no Bypass. Mode selectable at creation and while running. Cloud sessions pre-approve file edits regardless of mode. Self-hosted default config pre-approves routine tool calls including Bash; unapproved calls prompt through the session UI (no terminal).
- Status: GA
- Evidence: https://code.claude.com/docs/en/web-quickstart.md (Choose a permission mode); https://code.claude.com/docs/en/claude-code-on-the-web.md (Permission modes in cloud sessions); https://code.claude.com/docs/en/self-hosted-environments-configuration.md (Permissions and tool approval)
- Notes: A repo can't grant itself auto mode: `defaultMode: auto` is honoured only from image-wide or user-level settings (SHC, Repository-committed permission rules).

### VS Code plan review as an editable document
- What: In Plan mode, VS Code opens the plan as a full Markdown document where the user adds inline comments as feedback before Claude starts. `/plan` switches mode, `/plan <task>` starts planning that task, and `/plan open` opens the plan file (v2.1.280+). Resuming a conversation that ended in plan mode restores plan mode (v2.1.246+), except when `initialPermissionMode` or a carried-over mode choice applies, when `claudeProcessWrapper` is set, or when `ExitPlanMode` is denied.
- Status: GA
- Evidence: https://code.claude.com/docs/en/vs-code.md (Use the prompt box; Resume past conversations)

### Task/todo tracking tools in the SDK
- What: `TaskCreate`/`TaskGet`/`TaskUpdate`/`TaskList` (or `TodoWrite` with `CLAUDE_CODE_ENABLE_TASKS=0`) are on by default only for Opus 4.7, Sonnet 4.6, Haiku 4.5 and earlier; newer models need opt-in (`allowedTools`, `tools`, or `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`). Lifecycle pending -> in_progress -> completed, or `deleted`. Apps render progress from `tool_use` blocks and read assigned IDs from `tool_use_result` on the paired result.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/todo-tracking.md (Model availability; Todo lifecycle; Examples)
- Notes: Streamed tool input is the raw model output; Claude Code silently repairs near-miss keys (`task_id` -> `taskId`) before execution, so readers must be defensive. Docs say newer models track multi-step work without a written list.

### Plan mode instructions override
- What: TS `planModeInstructions` replaces the plan-mode workflow body while the CLI keeps its read-only enforcement preamble and ExitPlanMode footer.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (Options)
- Notes: Plan-mode permission semantics are in the [area 5] permission modes entry.

### Planning and mode renames
- What:
  - The "default" permission mode was renamed "Manual"; `--permission-mode manual` is accepted (Week 27).
  - Entering plan mode stopped needing approval (2.1.0).
  - A one-time note appears when plan mode won't ask before edits because bypass is available (2.1.299).
  - `/goal` keeps Claude working across turns until a condition holds (2.1.139). With background tasks pending it checks in after 30 minutes (`CLAUDE_CODE_GOAL_CHECKIN_MINUTES`, Week 34).
  - Ultraplan was removed (2.1.222, Aug 4, 2026).
- Status: GA; Ultraplan removed
- Evidence: https://code.claude.com/docs/en/whats-new/2026-w27.md, https://code.claude.com/docs/en/whats-new/2026-w20.md, https://code.claude.com/docs/en/whats-new/2026-w34.md, https://code.claude.com/docs/en/whats-new/2026-w32.md; https://code.claude.com/docs/en/changelog.md (2.1.0, 2.1.222, 2.1.299)

## Area 9. Sub-agents and multi-agent orchestration

### Default subagent delegation behavior
- What: Claude delegates broad searches to the built-in Explore agent, forks side tasks when fork mode is on (stated as the default), and runs subagents in the background by default. Subagents cost their own requests against the same usage; a fresh subagent may be slower because it re-gathers context; by default it is the same or a smaller model, so it gives parallelism or fresh eyes, not a smarter answer. Prompt cache lasts about one hour on subscriptions and five minutes on API keys, cloud providers or usage credits; subagents get five minutes by default.
- Status: GA
- Evidence: https://code.claude.com/docs/en/best-practices.md (Know what subagents cost; Keep cost and context down)
- Notes: Built-in subagents named in the glossary: Explore, Plan, general-purpose (https://code.claude.com/docs/en/glossary.md Subagent). Agent teams are experimental and disabled by default (Glossary: Agent teams).

### Auto mode review of subagents
- What: The classifier checks subagent work at spawn (delegated task description), per action (same decision order; subagent frontmatter `permissionMode` ignored), and at completion (work and final report reviewed before the parent reads it). Flagged reports are still delivered with a prepended security warning; if the classifier is unavailable the report carries a verify-before-acting note.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (How auto mode handles subagents)

### Cross-session messaging (ListAgents / SendMessage between your sessions)
- What: Claude can discover your other sessions with `ListAgents` and send plain-text messages with `SendMessage`, unprompted or when asked; users can target with `@session-name` typeahead (v2.1.232+). Recipients: subagents of this session, agent-team teammates, other local sessions (including background and `claude -p` sessions that bound an inbox), cloud sessions and Remote Control sessions on other machines (only while this session is connected to Remote Control). A message is text only, never history or files; `@file` mentions in it are not expanded. Delivered between tool calls in an active turn, or starts a new turn when idle; counts toward usage like a prompt.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cross-session-messaging.md (Message another session; Message delivery; See which sessions Claude can reach)
- Notes: Requires v2.1.224+ (macOS/Linux/WSL2), v2.1.234+ native Windows; on Bedrock/Vertex/Foundry or with flag fetching off, same-machine messaging needs v2.1.248+. `/list-agents` (alias `/peers`). Ambiguous names get short identifiers. Container and host sessions cannot reach each other; WSL2 and native Windows cannot either.

### Cross-session inbound trust model and controls
- What: Receiver is told the message is from another session, not the user: it can't answer permission prompts, can't justify config/CLAUDE.md/permission changes, slash commands in it arrive as text and never run, and the receiver's own permission prompts still fire. `crossSessionInbound` = `accept` / `hold` / `refuse`. When unset, the default compares permission-mode classes: a prompting receiver accepts unless the sender claims to bypass permissions; a bypassing receiver holds unless the sender also bypasses. Held messages open an approval dialog (Approve/Deny) that expires after `dialogExpiry` (default 5 min); at most 100 held, oldest dropped.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cross-session-messaging.md (How a session treats an incoming message; Control inbound messages; Non-interactive sessions); https://code.claude.com/docs/en/settings-reference.md (crossSessionInbound)
- Notes: Project/local `crossSessionInbound` applies only when stricter. Unrecognized value in user/project/local/--settings forces hold; in managed settings it is treated as refuse. Sender name is self-asserted (mods docs warn not to trust it). `-p` sessions hold for the expiry then report expiry to the sender; `dialogExpiry: "never"` keeps them. Senders are instructed never to ask peers for actions denied locally.

### Cross-session transport: per-session inbox socket
- What: Each session with messaging binds a Unix domain socket (macOS/Linux/WSL2) or named pipe (Windows). Path shown in `/status` "Peer address" (`uds:` prefix) and exported as `CLAUDE_CODE_MESSAGING_SOCKET`; a per-session `CLAUDE_CODE_MESSAGING_TOKEN` is exported too. Scripts/hooks may post into their own session; first line `{"type":"auth","token":...}` optional on Unix, required on Windows. Connections idle >30 s without a complete line are closed.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cross-session-messaging.md (The session's inbox socket)
- Notes: Socket restricted to the OS user; refuses directories owned by others and falls back to `/tmp/cc-socks-<uid>`; if no acceptable dir, session runs without an inbox. "Own-child" messages (hooks/Bash posting back) are auto-delivered when unset, verified by process evidence (Linux even after exit; macOS only while running; none in PID-1 containers or Windows) or by the token. Sandboxed Bash reaches the socket only per `sandbox.network.allowUnixSockets`/`allowAllUnixSockets`. Bare mode binds no socket.

### Cross-machine and cloud messaging
- What: Same-machine messages never touch Anthropic servers; messages to other machines go via Anthropic servers over the target's Remote Control connection; cloud sessions are reached directly via Anthropic. `offline` targets get the message after reconnect. `isolatePeerMachines: true` requires explicit user approval for any message leaving the machine, even in bypassPermissions; any scope can turn it on, none can turn it off.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cross-session-messaging.md (Message sessions on other machines; Require approval for cross-machine messages; Replies from other machines and cloud sessions)
- Notes: Needs claude.ai login (not API key/Bedrock/Vertex/Foundry). Cloud sessions cannot reply via SendMessage to a cross-machine message; messages sent while not connected to Remote Control carry no reply address. Session lists are paged newest-first and bounded, so old sessions can be unreachable by name. Cross-machine conversation start needs v2.1.225+.

### Idle/exit notices between sessions (`notify_when_idle`)
- What: Claude can subscribe (via `SendMessage`'s `notify_when_idle` input, alone or attached to a message) to one notice when a same-machine session next finishes a turn with nothing queued, or exits. Subscribing alone costs no tokens/turn in the watched session; notice is immediate if already idle. Notice may include finish time and a one-line status.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cross-session-messaging.md (Get a notice when another session goes idle)
- Notes: v2.1.236+ both sides. Subscriptions drop after 12 h. Only the main conversation can subscribe, only to local sessions; other targets refuse the whole call. `refuse` on either side means nothing; `hold` strips the status line.

### Cross-session messaging limits and loop protection
- What: Plain text only; same-machine message cap about one million serialized characters (refused at sender); rapid bursts to one session refused at sender once its inbox limit is reached; receiver rate-limits repeats per sender, drops identical repeats in a short window, and queues at most 50 accepted messages, so two-session message loops self-terminate.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cross-session-messaging.md (Limitations)
- Notes: Off switches: `crossSessionInbound: "refuse"` for receive; permission deny rules `SendMessage` and `ListAgents` for send/list (denying SendMessage also removes subagent/teammate messaging). A refusing session shows no visible change in `/status` or others' listings.

### `/fork` copies a session into a new background session
- What: `/fork [prompt]` copies the conversation so far into a new agent-view background session while the original keeps running; carries model, permission mode, effort, added directories and "don't ask again" grants. The copy is instructed to create its own worktree before editing and to stay out of the original's worktree, basing its branch on the original's when building on it. Independent afterwards; may message the original via cross-session messaging.
- Status: research preview (part of agent view)
- Evidence: https://code.claude.com/docs/en/agent-view.md (Copy the session with /fork; Where the copy runs; Sessions that can't be forked)
- Notes: Sessions launched with flags the copy can't inherit (replaced system prompt, `--tools` allowlist) refuse to fork. With agent view off, `/fork` makes a forked subagent; old forked-subagent behaviour moved to `/subtask` (v2.1.212).

### Subagent definition format and scopes
- What: Subagents are Markdown files with YAML frontmatter; the body becomes the system prompt. Only `name` and `description` are required. They are looked up by priority: managed settings (1), the `--agents` CLI JSON (2), project `.claude/agents/` (3), user `~/.claude/agents/` (4), plugin `agents/` (5). Project dirs are found by walking up from the working directory to the repo root, and the closest definition wins. Directories are scanned recursively, but identity comes only from `name`. In plugins the subfolders become part of the scoped id (`plugin:review:security`). Two files with the same name in one directory make the choice depend on filesystem read order, and `/doctor` flags this. A file watcher hot-reloads edits within seconds. A restart is needed for a newly created agents dir, for `--add-dir` dirs, and under `--disable-slash-commands`. `.claude/agents/` from `--add-dir` directories also load.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Choose the subagent scope; Write subagent files)
- Notes: Plugin subagents ignore `hooks`, `mcpServers` and `permissionMode` "for security reasons". In headless mode `--agents` also accepts a JSON file path (v2.1.281+). An empty `prompt` is allowed (v2.1.281+). Names must be at most 256 chars, cannot contain `:` and cannot start with `-`. On v2.1.198+, `/agents` only prints a reminder; the old interactive wizard is gone.

### Subagent frontmatter fields
- What: The fields are:
  - `tools` (allowlist) and `disallowedTools` (applied first; an entry like `Bash(git push *)` with a specifier still removes the whole tool)
  - `model` (sonnet / opus / haiku / fable, a full id, or inherit)
  - `permissionMode`
  - `maxTurns` (output is returned marked partial and can be resumed)
  - `autoCompactWindow` (100k to 1M tokens; must be a plain integer, because "200k" parses as 200)
  - `skills` (full content of up to 32 skills injected at start)
  - `mcpServers` (inline definition or reference by name)
  - `hooks`
  - `memory` (user, project or local)
  - `background`
  - `omitClaudeMd`
  - `effort`
  - `isolation: worktree`
  - `color`
  - `initialPrompt` (used only when the agent runs as the main session)
  - `experimental.cacheTtl` (5m or 1h)

  Unknown fields are silently ignored. `claude --debug` logs up to five unrecognized names with suggestions (v2.1.296+). Files are skipped silently when they have no name, when the `---` is not on the first line, or when the YAML is invalid; a name without a description goes to the debug log. `claude plugin validate <dir>` checks parse errors (v2.1.233+).
- Status: GA; `experimental.cacheTtl` is marked experimental
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Frontmatter reference; Subagent files Claude Code skips)
- Notes: MCP wildcard entries are supported (`mcp__github__*`, `mcp__*`). Server names that contain `__` cause prefix collisions in `disallowedTools`, and the docs flag this explicitly. If `tools` resolves to nothing, the launch fails with an error.

### Built-in subagents
- What: The built-in subagents are:
  - Explore: read-only, Write/Edit denied. Claude passes a thoroughness of quick, medium or very thorough. It runs on the main model, or on the `opus` alias when the main model is Fable on first-party/gateway.
  - Plan: read-only; used for research in plan mode.
  - general-purpose: all subagent tools.
  - `claude`: a catch-all; also the default agent for dispatched background sessions.
  - statusline-setup (Sonnet) and claude-code-guide (Haiku).

  Explore and Plan start without CLAUDE.md or the git status snapshot. They are one-shot (no agent ID returned, cannot be resumed). A user or project agent named `Explore` overrides the built-in one. You can disable them three ways: `permissions.deny: ["Agent(Explore)"]`, `CLAUDE_CODE_DISABLE_EXPLORE_PLAN_AGENTS=1` (v2.1.198+), or, in headless/SDK, `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS=1`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Built-in subagents)
- Notes: If an Agent call has no `subagent_type` and no general-purpose agent exists, it fails with `subagent_type is required`. Startup warns when the combined descriptions of custom agents exceed 15,000 tokens.

### Subagent context isolation (what loads at startup)
- What: A non-fork subagent starts with a fresh context containing:
  - its own system prompt plus environment details (not the Claude Code system prompt)
  - the delegation message Claude writes
  - the full CLAUDE.md hierarchy, including AGENTS.md and managed files
  - a git status snapshot (for a worktree subagent, the worktree's)
  - any preloaded skills
  - a "sibling roster" system reminder naming other named agents, present only if it has SendMessage

  Never inherited: conversation history, already-invoked skills, files already read, output style, the main auto memory, and the parent's context window size (the subagent's own model decides that). `omitClaudeMd: true` drops user/project/local CLAUDE.md but keeps managed files (v2.1.271+). Even then, subdirectory CLAUDE.md files and path-scoped rules still load when the subagent touches files there.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (What loads at startup)
- Notes: Which agents get git status cannot be configured; only Explore and Plan skip it. In headless mode `--append-subagent-system-prompt[-file]` appends text to every subagent prompt except forks (v2.1.205+ / v2.1.261+).

### Subagent tool filtering (hard-removed tools, background reduction)
- What: Subagents inherit built-in and MCP tools, then two filters apply.
  - Removed from every subagent, even if listed: `Agent` at the depth limit, AskUserQuestion, EndConversation, EnterPlanMode, ExitPlanMode (unless `permissionMode: plan`), ScheduleWakeup, WaitForMcpServers, Workflow.
  - Background subagents keep all MCP tools but only these built-ins: Read, Grep, Glob, LSP, Bash, PowerShell, Edit, Write, NotebookEdit, WebFetch, WebSearch, TodoWrite, Skill, ToolSearch, Enter/ExitWorktree, Monitor, TaskStop, SendMessage, Artifact, plus SubagentHandback.

  So one definition can resolve to different tool sets in the foreground and the background, with no error. Forks skip both filters. Teammates also keep the TaskCreate/Get/List/Update and CronCreate/Delete/List tools.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Available tools; Restrict which tools a subagent can use)
- Notes: Background LSP was added in v2.1.280. In v2.1.63 the Task tool was renamed `Agent`; `Task(...)` still works as an alias.

### Restricting spawnable subagent types
- What: An agent running as the main thread (`claude --agent X`) can use `tools: Agent(worker, researcher)` to allow only those types; other types fail, and only the allowed ones appear in its prompt. Leaving `Agent` out of `tools` disables spawning. Inside a subagent definition the parenthesised list is ignored: `Agent` only enables nesting. Specific types can be blocked with `permissions.deny: ["Agent(name)"]` or `--disallowedTools "Agent(Explore)"`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Restrict which subagents can be spawned; Disable specific subagents)

### Subagent model resolution
- What: The model is chosen in this order: (1) a per-invocation `model` param on the Agent call (aliases only), (2) frontmatter `model` (`inherit` means the main model), (3) `CLAUDE_CODE_SUBAGENT_MODEL`, (4) the main model. A mod's `agent.spawn` hook can replace (1). A family alias resolves to the main conversation's exact model, including the `[1m]` suffix, when the main model is in that family. The alias in the env var always resolves to the alias target. `CLAUDE_CODE_SUBAGENT_MODEL_FORCE=1` (v2.1.257+) applies one model to every subagent, teammate and workflow agent; the frontmatter `model` and the call param are then ignored, except that forks and `model: inherit` skills stay on the main model. The `availableModels` allowlist is enforced: a blocked alias falls to the newest permitted version (v2.1.222+), and anything else blocked falls to the inherited model, with a warning. `/tasks` shows each subagent's model and effort (v2.1.242+). Subagents inherit the parent's extended-thinking setting (v2.1.198+); there is no per-subagent thinking setting.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Choose a model; Run every subagent on one model; Choose an effort level)
- Notes: Before v2.1.251 the env var took precedence over everything. Effort order: `CLAUDE_CODE_EFFORT_LEVEL` env, then the per-call param (v2.1.292+), then the frontmatter `effort`, then the session effort (applied only if the model is the same).

### Subagent MCP scoping and folder-trust gating
- What: In `mcpServers`, an inline entry (stdio/http/sse/ws, same schema as `.mcp.json`) connects when the subagent starts and disconnects when it finishes, so the parent's context never carries those tool descriptions. A name reference shares the parent's connection. Inline servers and frontmatter hooks from project `.claude/agents/` (or `--add-dir` agents dirs) load only after that exact folder is trusted. Trusting a parent folder does not count, and neither does the automatic trust `-p` gives settings hooks. Until then they are skipped with a debug-log entry naming the `hasTrustDialogAccepted` key. User-level, `--agents`/SDK and managed definitions skip the trust check. `--strict-mcp-config`, `--bare`, managed MCP and allow/deny policies all apply to frontmatter servers too. `--strict-mcp-config` does not filter `--agents`/SDK inline servers.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Scope MCP servers to a subagent; Trust required for inline MCP servers; Hooks in subagent frontmatter)
- Notes: The trust check for inline servers was added in v2.1.238. Before that they loaded without one.

### Foreground vs background subagents, fork mode
- What: By default, subagents that Claude spawns from the main conversation start in the background. In interactive CLI, VS Code and desktop, Claude cannot wait for a subagent while "fork mode" is on. Fork mode is on by default from v2.1.302, and in the interactive terminal from v2.1.232. To let Claude wait, set `CLAUDE_CODE_FORK_SUBAGENT=0`, then use Ctrl+B to background a waiting one. In `-p` and the SDK, Claude may wait on its own judgment. `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` makes Claude always wait and also disables background commands. `background: true` in frontmatter forces background. Background results arrive as a completion notification in a later turn, marked as an automated event.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Run subagents in foreground or background; Which subagents run in the background; Let Claude wait for a subagent)
- Notes: Finished rows leave the panel immediately (v2.1.232+). `/tasks` keeps completed ones for 30 seconds (v2.1.208+). Failed or stopped ones stay 30 seconds; press `x` to clear.

### Forked subagents (/subtask)
- What: A fork inherits the full conversation, the same system prompt, tools and model, and shares the parent's prompt cache, so its first request is a cache hit. Only its final result returns to the main thread. You start one with `/subtask <task>` (named from the first words), or Claude spawns one when fork mode is on. A fork can take `isolation: "worktree"` but cannot spawn further forks. Running forks appear in a panel: arrow keys, Enter opens the transcript for follow-ups, `x` stops, Esc goes back. In a viewed agent, `/compact`, `/clear` and `/rewind` ask for confirmation because they act on main; `/model` and `/fast` are blocked. Ctrl+Enter delivers a message mid-work and backgrounds whatever the agent is waiting on (v2.1.286+). To block forks only, deny `Agent(fork)`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Fork the current conversation; How forks differ from other subagents; Turn fork mode off)
- Notes: With agent view on, `/fork` copies the whole session into a new background session. With agent view off, `/fork` is the forked subagent and `/subtask` does not exist.

### Nesting depth and concurrency limits
- What: By default subagents nest up to three layers below main. At the limit the `Agent` tool is withheld; a fork keeps it but the call errors. `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH` changes the depth; 1 turns nesting off. At most 20 subagents run concurrently by default. Spawn number 21 fails with `Concurrent subagent limit reached` and an instruction not to retry. `CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS` changes it (v2.1.217+). The concurrency limit is not enforced while ultracode is active. `/subtask` forks and resumes take slots without being blocked, so the running count can exceed the limit. There is no cap on total spawns per session. Workflows and teams have their own limits. In interactive mode a parent waits for its nested subagents. In `-p` with fork mode off, nested ones run in the background and may outlive their parent, reporting to main. The panel shows the tree with `(+N)` descendant counts.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Let subagents spawn their own subagents; Concurrent subagent limit)

### Subagent API-error handling
- What: If a stream is cut with text but no tool calls, the subagent is prompted to continue until its continuations are used up. In the foreground, a cut after text output returns the partial output with a note; with no text it fails with `Agent terminated early due to an API error`. In the background the subagent is marked failed, and the end notification includes the error and the last output. A fallback model chain can switch the subagent's model; the report then opens with a "switched to <model>" warning line.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (API errors in subagents)

### Running a whole session as an agent (--agent / agent setting)
- What: `claude --agent <name>`, or `"agent"` in settings, makes the main thread take that agent's tools, model and prompt. A non-empty prompt replaces the default system prompt like `--system-prompt` does. CLAUDE.md still loads, even with `omitClaudeMd`. In `-p` with its own prompt, the git status snapshot is dropped. The choice persists across resume. If the agent has been deleted, the session falls back to the default tools with a warning. The startup header shows `@<name>`. For plugin agents a bare name is enough unless it is ambiguous.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Invoke subagents explicitly)
- Notes: Other ways to invoke: natural language (Claude decides), and `@"name (agent)"` / `@agent-<name>`, which guarantees the agent runs for one task while Claude still writes the task prompt.

### Agent teams (lead, teammates, mailbox, task list)
- What: Agent teams are off by default. `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` turns them on. Only interactive sessions can spawn teammates; in `-p`/SDK, named subagents stay plain subagents. One lead session spawns teammates, each a full independent Claude Code instance with its own context. Teammates get CLAUDE.md, MCP and skills from the project, plus the spawn prompt, but not the lead's history. Messaging is by name through per-agent JSON mailbox files at `~/.claude/teams/{team}/inboxes/{agent}.json`. Entries are validated on read and malformed ones dropped. A send counts as successful only after the file write succeeds, so a full disk surfaces as an error. There is no broadcast; you send one message per recipient. A teammate sends an idle notification with its final answer to the lead. The shared task list has three states (pending, in progress, completed) and dependencies: blocked tasks are not self-claimed and auto-unblock. Claiming uses file locking. Team config is at `~/.claude/teams/{team}/config.json` (members array, lead type `team-lead`) and is removed at session end. Tasks persist in `~/.claude/tasks/{team}/` until `cleanupPeriodDays`. The team name is `session-` plus the first 8 chars of the session ID.
- Status: experimental
- Evidence: https://code.claude.com/docs/en/agent-teams.md (Enable agent teams; Architecture; Context and communication; Assign and claim tasks)
- Notes: Side effect: with teams on, any subagent Claude names launches as a teammate (unless it is a fork or passes `isolation` on the call), with no confirmation. Teams can therefore form unasked. Setting `=0` reverts without a restart.

### Agent teams display modes and teammate configuration
- What: There are two display modes. In-process (default, any terminal) shows an agent panel: Enter to view or message, `x` to stop, Ctrl+T for the task list, idle rows hide after 30 seconds, more than 3 idle collapse. Split panes need tmux or iTerm2 with the `it2` CLI. `teammateMode` (`in-process`|`auto`|`tmux`|`iterm2`) or the hidden flag `--teammate-mode` selects it. Teammate model order: spawn prompt, then subagent definition, then `CLAUDE_CODE_SUBAGENT_MODEL`, then the lead's model. Model and fast mode are fixed at spawn. Reusing a subagent definition as a teammate applies `tools`; in-process teammates always get SendMessage and the Task tools added. `model` applies in both modes. `disallowedTools`, `effort` and `autoCompactWindow` apply in-process only. The body is appended in-process and replaces the prompt in split-pane. `skills` is never applied. `mcpServers` applies in split-pane only. Team quality-gate hooks: `TeammateIdle`, `TaskCreated` and `TaskCompleted`, where exit 2 blocks the action and sends feedback.
- Status: experimental
- Evidence: https://code.claude.com/docs/en/agent-teams.md (Choose a display mode; Specify teammates and models; Use subagent definitions for teammates; Enforce quality gates with hooks)
- Notes: `teammateDefaultModel` was removed in v2.1.234. Split-pane is unsupported in the VS Code terminal, Windows Terminal and Ghostty. In-process teammate caches use a 5-minute TTL unless `subagentPromptCacheTtl: 1h`.

### Agent teams limitations
- What: The documented limitations are:
  - `/resume` and `/rewind` do not restore in-process teammates.
  - Task status can lag.
  - Shutdown waits for the current request (shutdown is a request a teammate can reject with an explanation).
  - One team per session.
  - No nested teams.
  - In-process teammates cannot run background subagents; `background: true` errors.
  - The lead is fixed for life.
  - No worktree isolation between teammates; partition files instead.
  - No hard cap on team size; the docs suggest 3 to 5 teammates and 5 to 6 tasks each.
- Status: experimental
- Evidence: https://code.claude.com/docs/en/agent-teams.md (Limitations; Choose an appropriate team size; Avoid file conflicts); https://code.claude.com/docs/en/agents.md (Choose an approach)
- Notes: The troubleshooting section admits the lead sometimes implements tasks itself instead of waiting, or ends early. Orphaned tmux sessions may need a manual `tmux kill-session`.

### Dynamic workflows (script-orchestrated subagents)
- What: Claude writes a JavaScript script, which a runtime executes in the background in an isolated environment. The script has no filesystem or shell access and no `import()`, and `Date.now()`, `Math.random()` and argless `new Date()` throw so replays stay deterministic. The API is `agent(prompt, {schema, label, stallMs})`, `pipeline(items, fn)`, `parallel()`, `phase()`, `log()` and a global `args`. Saved scripts start with `export const meta = {name, description, phases?}`, which must be a literal. Intermediate results live in script variables, not Claude's context. A `schema` forces JSON output; contradictions are detected statically before launch, and validation retries 5 times (`MAX_STRUCTURED_OUTPUT_RETRIES`). Limits: 16 concurrent agents by default (fewer on low-CPU hosts; `CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS` 1 to 256), 4,096 items per parallel or pipeline call, 1,000 agents per run. In a fan-out, matching agents are held until the first agent's response begins so they share its cache prefix (`CLAUDE_CODE_WORKFLOW_PREFIX_STAGGER_MS`, 5000ms). Workflow prompts are marked as computed text, so the auto-mode classifier does not treat them as user requests.
- Status: GA on paid plans, API, Bedrock, Agent Platform and Foundry; on Pro it is opt-in via `/config`
- Evidence: https://code.claude.com/docs/en/workflows.md (What the saved script looks like; Edit a saved script; How a workflow runs; Behavior and limits; Prompt caching in a fan-out)
- Notes: The bundled workflow is `/deep-research` (needs WebSearch). Saving writes to `.claude/workflows/` or `~/.claude/workflows/` and refuses symlinks. Plugins can ship workflows, namespaced `/plugin:name`. The run script is written under `~/.claude/projects/` and Claude can only launch scripts it may read.

### Workflow triggering, approval and size guidance
- What: A workflow runs when you ask in plain words, use the `ultracode` keyword in a typed prompt, or turn on `/effort ultracode`, where Claude plans a workflow for every substantive task. `--effort ultracode` also sets xhigh. The keyword is ignored in `-p` prompts, SDK input not stamped `origin: human`, scheduled prompts, webhooks and PR comments. Approval depends on mode. Auto: first launch only (consent saved to user settings), or skipped under ultracode. Manual/acceptEdits: every run, unless "don't ask again" was chosen for a named workflow and project. Bypass: none. `-p`/SDK: ordinary permission evaluation of the `Workflow` tool (`Workflow` or `Workflow(<name>)` allow rules, hooks, `--permission-prompt-tool`, `canUseTool`). In plan mode a hook's approval alone is not enough. The size guideline (`workflowSizeGuideline`) is advice, not a cap: unrestricted, small <5, medium <10 (the default; Pro defaults to small), large <50. A `Large workflow` warning appears above 25 agents or 1.5M projected tokens. Off switches: `/config`, `disableWorkflows`, `CLAUDE_CODE_DISABLE_WORKFLOWS=1`, or managed settings.
- Status: GA
- Evidence: https://code.claude.com/docs/en/workflows.md (Have Claude write a workflow; Where the keyword works; Approve the plan before it runs; Set a size guideline; Turn workflows off; Cost)
- Notes: Ultracode also bypasses the 20-subagent concurrency limit. No managed setting can turn off ultracode alone except by disabling workflows.

### Overview of parallel-work options
- What: The docs list five ways to run work in parallel: subagents, agent view (`claude agents`, research preview), agent teams (experimental), dynamic workflows, and Projects (cloud, public beta). Supporting tools are worktrees, cross-session messaging (local, other machines, cloud) and `/batch`, a skill that splits one change into 5 to 30 worktree-isolated subagents. Status views are `/tasks`, `/workflows`, `claude agents` and @-mention typeahead.
- Status: GA page; sub-features as marked
- Evidence: https://code.claude.com/docs/en/agents.md (Run agents in parallel; Choose an approach; Check on running work)

### Per-subagent and teammate model selection
- What: Subagent model is resolved in order: per-invocation `model` parameter on the Agent tool (aliases only: sonnet/opus/haiku/fable), the definition's `model` frontmatter (alias, full ID, or `inherit`), `CLAUDE_CODE_SUBAGENT_MODEL`, then the main conversation's model. A family alias that matches the main model's family resolves to the main model's exact ID (including `[1m]`). `CLAUDE_CODE_SUBAGENT_MODEL_FORCE=1` applies one model to all subagents, teammates and workflow agents and ignores frontmatter and per-call model (forks and `inherit` skills still use the main model). `/model` switches also reach subagents that inherit. Subagents inherit the main session's thinking on/off; there is no per-subagent thinking setting. `/tasks` shows each subagent's model and effort. Allowlist-blocked values are substituted (newest permitted version of the family, else inherited model) with a warning.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Choose a model; Run every subagent on one model); https://code.claude.com/docs/en/model-config.md (Setting your model; Environment variables; Restrict model selection)
- Notes: Order changed in v2.1.251 (env var used to come first). FORCE requires v2.1.257. A mod's `agent.spawn` hook can override the per-invocation model.

### Effort level of subagents and teammates
- What: A subagent or teammate without its own `effort` runs at the session-picked or `--effort` level only when it runs on the main conversation's model; on another model that level does not apply. Subagent and skill `effort` frontmatter override the session level while they run (skill until end of turn), still bounded by `maxEffortLevel` and org caps; `CLAUDE_CODE_EFFORT_LEVEL` beats them.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Effort level of subagents and teammates; Set the effort level)
- Notes: none

### Programmatic subagents (AgentDefinition)
- What: `agents: {name: AgentDefinition}` with `description` (routing), `prompt`, `tools`/`disallowedTools` (server-level `mcp__server__*` patterns), `model` alias or `inherit`, `skills` (preload, first 32), `memory`, `mcpServers`, `maxTurns` (output marked partial at the limit, v2.1.246+), `background`, `effort`, `permissionMode`; TS-only `autoCompactWindow` (100k-1M tokens), `omitClaudeMd`, `initialPrompt` (main-thread use), `criticalSystemReminder_EXPERIMENTAL`. Programmatic definitions override same-named `.claude/agents/` files. Built-in `general-purpose` is always available unless `CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS=1`. Forks (`subagent_type: "fork"`, v2.1.302+) copy the conversation; block with an `Agent(fork)` deny rule.
- Status: GA (`criticalSystemReminder_EXPERIMENTAL` experimental)
- Evidence: https://code.claude.com/docs/en/agent-sdk/subagents.md (AgentDefinition configuration; What subagents inherit; Forked subagents; Resume subagents; Tool restrictions); https://code.claude.com/docs/en/agent-sdk/typescript.md (AgentDefinition)
- Notes: Non-fork subagents get only their own prompt, the Agent tool's prompt string, project CLAUDE.md and tool definitions, not the parent's history or system prompt; they inherit the thinking config. Default is background launch (`status: "async_launched"`, report in a later turn). Resume needs the parent `session_id` plus the `agentId` parsed from the tool result text; built-in Explore/Plan are one-shot. Tool appears as `Agent` in tool_use blocks but `Task` in the init tools list. Opus 5 note: the `claude_code` preset adds a line discouraging unrequested Agent calls; a custom prompt does not.

### Subagent depth, concurrency and spend caps
- What: `CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH` (default 3 layers below main; 1 disables nesting), `CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS` (default 20; refusal `Concurrent subagent limit reached`; ultracode sessions exempt), and `maxBudgetUsd` (refuses new spawns, stops background subagents, ends with `error_max_budget_usd`). Stall watchdog `CLAUDE_ASYNC_AGENT_STALL_TIMEOUT_MS`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/subagents.md (Cap subagent depth, concurrency, and spend); https://code.claude.com/docs/en/agent-sdk/typescript.md (Handle slow or stalled API responses)
- Notes: Requires TS SDK v0.3.219 / Python v0.2.127 (bundling CLI v2.1.219). For dozens to hundreds of agents the docs point to the `Workflow` tool (TS SDK v0.3.149+), which runs orchestration as a script outside the conversation.

### Subagent defaults and limits (fast-moving)
- What:
  - Background by default: 2.1.198 (Jul 1, 2026).
  - Fork subagent on by default in the interactive terminal: 2.1.232 (Aug 13). It inherits the full conversation and prompt cache; `CLAUDE_CODE_FORK_SUBAGENT=0` turns it off. Extended to -p, the SDK, Desktop and VS Code in 2.1.302 (Oct 16).
  - `/fork` became "copy into a new background session" and the old in-session fork became `/subtask` (2.1.212).
  - Concurrency cap: 20 running subagents by default (`CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS`, 2.1.217).
  - Spawn cap of 200 per session added in 2.1.212 and removed in 2.1.224.
  - Nesting depth went 5 → 0 → 3 (see Contradictions); the current default is 3 layers (`CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH`).
  - Agent tool `effort` parameter (2.1.292). `CLAUDE_CODE_SUBAGENT_EFFORT` (2.1.316). `CLAUDE_CODE_SUBAGENT_MODEL_FORCE` (2.1.257).
  - The Task tool's `mode` parameter is deprecated and ignored (2.1.212).
  - The `/agents` wizard was removed (2.1.198); the "Default teammate model" setting was removed (2.1.234).
  - The Explore agent inherits the session model, capped at Opus, instead of Haiku (Week 27).
  - A subagent that hits `maxTurns` returns partial output that can be resumed with SendMessage (Week 35).
  - `/tasks` gained an "Earlier in this session" row for finished subagents (2.1.319).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.198, 2.1.212, 2.1.217, 2.1.219, 2.1.224, 2.1.232, 2.1.234, 2.1.257, 2.1.292, 2.1.302, 2.1.316, 2.1.319); https://code.claude.com/docs/en/whats-new/2026-w24.md, https://code.claude.com/docs/en/whats-new/2026-w27.md, https://code.claude.com/docs/en/whats-new/2026-w29.md, https://code.claude.com/docs/en/whats-new/2026-w33.md, https://code.claude.com/docs/en/whats-new/2026-w35.md; https://code.claude.com/docs/en/sub-agents.md (Let subagents spawn their own subagents)
- Notes: `CLAUDE_CODE_SUBAGENT_EFFORT` appears in no docs page in the dump.

### Dynamic workflows ("ultracode")
- What: Claude writes a script that orchestrates tens to hundreds of background agents (2.1.154, May 28, 2026).
  - The trigger keyword was renamed from `workflow` to `ultracode` (2.1.160).
  - Advisory size guideline: `/config` row (2.1.202), `workflowSizeGuideline` key (2.1.219).
  - Per-run concurrency up to 256 (`CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS`, 2.1.269). `CLAUDE_CODE_WORKFLOW_SUBAGENT_MODEL` (2.1.296).
  - The Workflow tool's prompt footprint was cut from about 5.7k to about 1k tokens by moving the reference into a bundled `workflow-authoring` skill (2.1.248).
  - OTel attributes `workflow.run_id` and `workflow.name` (2.1.202).
- Status: GA (the changelog says nothing about preview)
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.154, 2.1.160, 2.1.202, 2.1.219, 2.1.248, 2.1.269, 2.1.296); https://code.claude.com/docs/en/whats-new/2026-w22.md
- Notes: `CLAUDE_CODE_WORKFLOW_SUBAGENT_MODEL` has no docs-page mention in the dump.

### Cross-session messaging
- What: sessions find each other with `ListAgents` and send text with `SendMessage`. The payload is text Claude writes, never history or files.
  - Launched on macOS and Linux in 2.1.224 (Aug 7); Windows in 2.1.239; Bedrock, Vertex, Foundry and telemetry-off in 2.1.248.
  - `crossSessionInbound` (accept / hold / refuse) and `dialogExpiry`: messages to a bypass-permissions session are held for approval (2.1.224).
  - `notify_when_idle` sends a one-shot idle notice (2.1.236). `@name` mentions (Week 33).
  - The socket directory on shared `/tmp` refuses pre-planted symlinks (2.1.232).
- Status: GA
- Evidence: https://code.claude.com/docs/en/whats-new/2026-w32.md, https://code.claude.com/docs/en/whats-new/2026-w33.md, https://code.claude.com/docs/en/whats-new/2026-w34.md; https://code.claude.com/docs/en/changelog.md (2.1.224, 2.1.232, 2.1.236, 2.1.239, 2.1.248); has its own page cross-session-messaging.md
- Notes: the changelog says sessions "on any of your machines", but the digest and the 2.1.248 line say same machine (see Contradictions).

## Area 10. Background, long-running and scheduled work

### Background sessions and supervisor CLI (agent view)
- What: `claude --bg "task"` starts a background agent and returns, printing the ID (checks workspace trust; cannot combine with `-p`); `--bg --exec 'cmd'` runs a PTY-backed shell job instead. `claude agents` opens agent view (`--json`, `--all`, `--cwd`, defaults for dispatched sessions). `claude attach|logs|stop (kill)|respawn [--all]|rm <id>`; `rm` refuses over worktrees with unpushed commits and prints `--discard-unpushed` / `--force-remove-worktree` options; transcript stays for `--resume`. A supervisor daemon hosts background sessions: `claude daemon logs|run|status|stop --any [--keep-workers]`, log at `~/.claude/daemon.log`. `/background` detaches the current session.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cli-reference.md (CLI commands; --bg, --exec); https://code.claude.com/docs/en/commands.md (/background, /stop)
- Notes: Agent view itself is another slice's page; recorded here as surfaced by the CLI reference.

### Automatic continue after usage-limit reset
- What: Interactive claude.ai-subscription sessions (v2.1.234+) hit by a usage limit wait in place and, at reset, prompt Claude to resume the task (does not resend the last message). Handles laptop sleep (over about 30 min asks for Enter). Stops after three automatic continuations; cancel with Esc/Ctrl+C or `/rate-limit-options`. Ends on new prompt, exit, `/login`, clear, rewind, resume, teleport, restart, move to Desktop/background/cloud, or a blocking UserPromptSubmit hook. Not auto-started for Remote Control, teammates, resets over 24 h, or family-specific limits; not offered in background sessions, `-p`, API keys or cloud providers. `autoContinueAtUsageLimit: false` disables; `/config key=value` can disable but not enable because it grants unattended execution.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Wait for a usage limit to reset; Cancel the wait; Start a wait yourself; Turn automatic continue off)
- Notes: Permission prompts still apply during the continued task.

### Automatic backgrounding of long MCP tool calls
- What: A main-conversation MCP call still running after 2 minutes becomes a background task: Claude gets a task ID and continues; the result arrives as a task notification. Visible and stoppable in `/tasks` (stopping cancels the call but cannot guarantee the server stops). Does not survive exiting. Not applied to subagent calls, IDE servers, non-interactive mode (unless `CLAUDE_AUTO_BACKGROUND_TASKS=1`), or while an elicitation dialog is open. Threshold `CLAUDE_CODE_MCP_AUTO_BACKGROUND_MS` (0 = off); `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` turns it and background shell commands off.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Automatic backgrounding of long tool calls)

### MCP tasks (server-side long jobs, tasks extension)
- What: Supports the MCP tasks extension: a server answers `tools/call` with `resultType: "task"`, `taskId` (max 128 printable ASCII, must be valid across connections), `status` (`working|input_required|completed|failed|cancelled`). Claude Code polls `tasks/get` at `pollIntervalMs` (clamped 250-30000, default 2000), answers `input_required` via `tasks/update` (up to 16 input requests; serves `elicitation/create` only interactively and `roots/list` everywhere), sends `tasks/cancel` on stop (not on exit). Remote tasks survive exit and are re-followed when the same session is resumed with unchanged server name/settings; unknown-ID errors (-32602) retried for 5 minutes, then "outcome unknown". Footer shows `1 MCP task`. Requirements: protocol 2026-07-28, flag-fetching session (not Bedrock etc.), terminal CLI, Agent SDK or GitHub Actions (not VS Code or desktop), main conversation only, interactive or stream-json with `CLAUDE_AUTO_BACKGROUND_TASKS=1`.
- Status: beta (public beta, v2.1.304+)
- Evidence: https://code.claude.com/docs/en/mcp.md (https://code.claude.com/docs/en/mcp.md tasks that run on the server; What a call needs to become an https://code.claude.com/docs/en/mcp.md task; https://code.claude.com/docs/en/mcp.md tasks for server authors; Status messages on an https://code.claude.com/docs/en/mcp.md task)
- Notes: `tasks/result`, `tasks/list`, `notifications/tasks` are not used. Before v2.1.317 missing optional-ish fields caused `Unsupported result type`. A task in the background has no Claude Code time limit by default. Non-interactive sessions cancel tasks that ask for input, without running Elicitation hooks.

### Plugin monitors (background watchers that notify the model)
- What: `monitors/monitors.json` entries (`name`, `command`, `description`, optional `when: "always" | "on-skill-invoke:<skill>"`) run a shell command in the background for the session; each line printed reaches Claude as a notification. Run in the session cwd with full user permissions outside the sandbox. Not started in `-p` mode; never receive `${user_config.*}` or `CLAUDE_PLUGIN_OPTION_*`; keep running if the plugin is disabled mid-session; need a restart (not reload) to pick up an update. Not loaded at all for repository (project-scope) plugins.
- Status: experimental (manifest key `experimental.monitors`)
- Evidence: https://code.claude.com/docs/en/plugins/components.md (Monitors); https://code.claude.com/docs/en/plugins/manifest-reference.md (monitors); https://code.claude.com/docs/en/plugins/loading.md (Trust check and component limits for repository plugins)
- Notes: On Bedrock/Vertex/Foundry or with telemetry off, requires v2.1.310+.

### Agent view (`claude agents`): dashboard of background sessions
- What: Full-terminal TUI listing every background session across projects (or `--cwd` scoped), grouped Pinned / Ready for review / Needs input / Working / Completed (or by directory with Ctrl+S). Each row: state icon, name (Haiku-generated, renamable), one-line summary, age, PR label. Bottom input dispatches a new session per Enter; `Space` opens a peek panel (latest output or pending question) where you can reply or `!cmd`; Enter/→ attaches full-screen; ← on empty prompt detaches. ← in any foreground session backgrounds it and opens agent view. Filters `a:`, `s:`, `n:`, `o:`, `#PR`, URL. `/resume` picker brings past sessions back as rows.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/agent-view.md (Quick start; Monitor sessions with agent view; Peek and reply; Attach to a session; Organize the list; Filter sessions; Keyboard shortcuts)
- Notes: Introduced v2.1.139. States: Working, Needs input, Idle, Completed, Failed, Stopped; shape icons distinguish running process vs exited (`∙`, restarted on reply) vs sleeping `/loop` (`✢`). Footer `← N agents` count of waiting sessions. Notifications via `preferredNotifChannel` and `Notification` hook types `agent_needs_input`/`agent_completed` (v2.1.198). Peek replies to a working session queue; `/model`, `/effort`, `/rename`, `/usage` run immediately; exactly `/stop` stops. Peek cannot answer permission dialogs (must attach). Prompt under 4 chars rejected. `defaultToAgentsView` makes plain `claude` open it.

### Background session supervisor (daemon) and process lifecycle
- What: A per-user supervisor process (started on first `--bg`/agent view) hosts each background session as its own Claude Code process with its own terminal host process. Working, dialog-blocked, attached or pinned sessions keep running; finished/waiting sessions unattached ~1 h get their process stopped and are resumed from disk on attach/reply. Crashed processes are restarted. After auto-update the supervisor restarts onto the new version and migrates idle sessions. If the supervisor dies, sessions stop unless a new supervisor connects within ~1 minute (macOS/Linux). Sleep is survived; shutdown stops sessions (shown failed within 48 h, stopped after).
- Status: research preview
- Evidence: https://code.claude.com/docs/en/agent-view.md (How background sessions are hosted; The supervisor process; Where state is stored; Troubleshooting)
- Notes: State under `~/.claude/daemon.log`, `daemon/roster.json`, `jobs/<id>/state.json`, `jobs/<id>/tmp/` (writes there skip permission prompts; `CLAUDE_JOB_DIR` exported). `CLAUDE_CONFIG_DIR` gives a separate supervisor instance. `claude daemon status|logs|stop --any [--keep-workers]`. Low-memory heuristic note on silent early exits; under memory pressure it stops idle (then pinned idle) sessions. Background shell commands and background subagents carry across process restarts (disable with `CLAUDE_CODE_DISABLE_BG_EXIT_HANDOFF=1`); monitors stop. Two processes can't write one transcript: opening a session already resumed elsewhere is refused.

### Dispatch from the shell and shell jobs
- What: `claude --bg "<prompt>"` (or `--background`) starts a background session and prints short ID plus attach/logs/stop commands; `--agent`, `--name`, `--model`, `--resume <id> --bg` (continue in place or copy with a `note:`, v2.1.257+), `--fork-session`. `--bg` with `-p` is rejected. `claude --bg --exec 'pytest -x'` (or `!` in the dispatch input) runs a PTY-backed shell job row with no model; output kept in memory only and the row is cleaned ~5 min after exit. Management: `claude attach|logs|stop|respawn [--all]|rm`.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/agent-view.md (Dispatch an agent from your shell; Run a shell command; Manage sessions from the shell; Prompts that contain --bg)
- Notes: Workspace trust is checked before `--bg` (v2.1.281); in scripts it errors `Workspace not trusted`. Ambiguous `--bg` flag position is refused rather than guessed (v2.1.299, fixing a bug where a prompt of `--bg` could spawn sessions repeatedly). Shell jobs are never auto-restarted (would rerun the command).

### Machine-readable session state (`claude agents --json`)
- What: Prints a JSON array of sessions (`--all` adds completed, `--cwd` filters). Fields: `cwd`, `kind`, `startedAt`, `id`, `state` (working/blocked/done/failed/stopped), `pid`, `status` (busy/waiting/idle), `waitingFor` (permission prompt, input needed, sandbox request, worker request, dialog open), `sessionId`, `name`. Documented as the supported interface; `~/.claude/jobs` files are explicitly unstable.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/agent-view.md (List sessions as JSON; Read session state from a script)
- Notes: `done` vs `blocked` distinction: finished-and-waiting-for-next-prompt is `done`; `blocked` always means user action required.

### Row summaries by a small model
- What: Each row's status line is written by a Haiku-class model at end of each turn and rewritten every few minutes during long turns; between rewrites (at most every 15 s) it uses the session's own recent output with no model call. Session names are also Haiku-generated from the prompt.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/agent-view.md (Row summaries; Dispatch an agent from agent view)
- Notes: Billed through the normal provider under the same data terms; falls back to the main model if no Haiku-class model configured (`ANTHROPIC_DEFAULT_HAIKU_MODEL`). Naming replies that answer instead of label are discarded (v2.1.211).

### Mod background work: timers, prompt submission, interruption rules
- What: Mods run periodic work with `$.clock.every/after` (no `setTimeout`), started from `session.start`; callbacks run outside events, between turns, errors go to debug log. `$.prompt.submit({text})` waits for idle and starts a turn, with Claude told the mod is the sender unless `asUser: true`. After the user interrupts (Esc/Stop), calls that start new work (`prompt.submit`, `command.run`, `agent.spawn`, `session.send`, background Bash/Monitor/CronCreate via `$.tool.call`) are refused until the user's next input; queued prompts/commands are dropped with specific reason strings; nothing auto-retries.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/api.md (Run work in the background; Start a turn from a background job; Calls and background work after the user interrupts a turn; Calls refused in claude -p runs)
- Notes: Interruption handling v2.1.304+ (except prompt.submit/command.run). In `-p`/SDK runs, mod turns and mod-spawned subagent turns count against `--max-turns` (v2.1.299 / v2.1.304) and `--max-budget-usd`; docs warn a mod that submits every turn makes `-p` never exit. A `Stopped:` toast tells the user a mod was blocked post-Stop.

### Workflow resume, pause and stall recovery
- What: The runtime records each agent's result. A relaunch replays agents in start order: completed agents return cached results until the first agent whose prompt differs, and from there on everything reruns. A failed agent, or one stopped alone, reruns along with every later agent. Agents that were running when the run was stopped start over. On a usage limit (v2.1.271+), the run pauses and continues on its own after reset. This only works in interactive or Code-tab sessions on a subscription without usage-based billing, with `autoContinueAtUsageLimit` on, a reset within 24 hours, and no more than two waits; otherwise the agent fails. Stalled agents restart automatically up to 5 times (6 attempts), with errors `stalled on all 6 attempts`, `lost its reply` or `abandoned`. `stallMs` sets the window per agent and `CLAUDE_ASYNC_AGENT_STALL_TIMEOUT_MS` globally. Inside parallel or pipeline a failure becomes `null`; awaited directly it ends the run. Exiting the session stops the run, but `claude --resume` plus a relaunch restores it from saved results. Otherwise you get `nothing to resume`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/workflows.md (Resume after a pause; Which agents run again; When a run hits your usage limit; When an agent stalls and restarts)
- Notes: The `/workflows` TUI has keys `p` pause, `x` stop, `r` restart agent, `s` save and `f` filter, plus per-agent token usage. Cloud sessions store workflow results with the conversation, so they survive VM reclaim.

### Session-scoped scheduling: /loop and cron tools
- What: There are three `/loop` forms. With an interval, the interval is converted to cron; seconds are rounded up to a minute and odd steps are rounded. With only a prompt, Claude self-paces with a delay of 1 minute to 1 hour chosen each iteration and prints it, or may use the Monitor tool instead of polling. Bare `/loop` runs a built-in maintenance prompt: continue unfinished work, tend the branch's PR, then cleanup passes. It starts no new initiatives, and irreversible actions happen only if the transcript already authorised them. `.claude/loop.md` or `~/.claude/loop.md` replaces that default (truncated at 25,000 bytes, re-read every iteration). A loop prompt can be a skill, but built-in commands, `disable-model-invocation` skills and MCP prompts arrive as plain text. The tools are `CronCreate` (5-field cron, recurring or one-shot), `CronList` and `CronDelete`, with 8-char IDs and up to 50 tasks per session. Natural-language one-shot reminders delete themselves. The scheduler checks every second and fires between turns at low priority, in local timezone, with vixie-cron day-of-month/day-of-week OR semantics; there is no `L/W/?` and no names. `CLAUDE_CODE_DISABLE_CRON=1` disables it.
- Status: GA
- Evidence: https://code.claude.com/docs/en/scheduled-tasks.md (Run a prompt repeatedly with /loop; Manage session-scoped scheduled tasks; How session-scoped scheduled tasks run; Cron expression reference)
- Notes: Stopping a self-paced loop: Esc, or Ctrl+C when idle, or Claude calls `ScheduleWakeup stop:true`. If an iteration neither reschedules nor stops, a fallback wakeup fires about 20 minutes later. Fixed-interval loops must be cancelled by asking; Esc does not stop them.

### Cron jitter, expiry and restore semantics
- What: Recurring tasks run late by up to half their period, capped at 30 minutes. The offset is fixed per task for evenly spaced schedules. `*/5` runs at 4m45s so the 5-minute prompt cache is still warm. One-shot tasks at :00 or :30 run up to 90 seconds early. Recurring tasks expire after 7 days: they fire once more, then delete themselves. Missed fires are not caught up; a fire that came due while Claude was busy runs once. On resume, unexpired cron tasks are restored, but self-paced loops, background Bash and monitors are not. With feature-flag fetching off, persistent tasks go in `.claude/scheduled_tasks.json`, refuse symlinks, and run only in the folder where they were created. Backgrounding a session carries its `/loop` tasks along.
- Status: GA
- Evidence: https://code.claude.com/docs/en/scheduled-tasks.md (Jitter; Seven-day expiry; Limitations)
- Notes: Jitter is meant to spread API load. The `next run` display needs v2.1.299+.

### Routine limits and run outcomes
- What: Hourly caps: 100 scheduled runs per account (excess waits), 30 Run-now plus API fires per task, 100 Run-now per account, and 100 API fires per account (excess fails). There is no overage on these caps. GitHub events have unspecified per-task and per-account hourly caps, and excess events are dropped. Usage draws on the subscription, with optional metered usage credits. Run outcomes are Succeeded, Failed or Cancelled and reflect only how the session ended, not task success. There is a table of "The last run..." warnings: org concurrency cap, environment missing, policy block, repo or branch unavailable, and usage limit.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/routines.md (Usage and limits; View and interact with runs; "The last run" warnings)
- Notes: In auto mode a run can publish an artifact without asking. In Accept-edits mode it republishes without asking only under a list of seven conditions. Team/Enterprise owners can toggle the feature org-wide, and custom roles need the permission.

### Mobile push notifications from Remote Control
- What: While RC is active Claude can push to the phone when a long task finishes, when a decision is needed, or when asked in the prompt. Two `/config` toggles: "Push when Claude decides" and "Push when actions required". Suppressed while typing in or focused on the terminal; `CLAUDE_CLIENT_PRESENCE_FILE` marker file extends suppression (e.g. screen-lock listener).
- Status: GA
- Evidence: https://code.claude.com/docs/en/remote-control.md (Mobile push notifications); https://code.claude.com/docs/en/mobile.md (Get push notifications)
- Notes: No per-event configuration beyond the two toggles.

### On-demand runners (orchestrator + `spawn-runner` hook)
- What: `claude self-hosted-runner orchestrator` is stateless, polls Anthropic for spawn requests (one per queued session without a runner), and runs `spawn-runner` per request, which must submit a workload (K8s Job, EC2, Nomad) asynchronously and return within `--hook-timeout` (60 s). Hook env: work-order JWT file (deleted after hook), `CLAUDE_RUNNER_ORDER_ID` (idempotency key), session IDs (empty for pre-warm), attempt, server time, pool ID, account ID/email, primary repo URL/revision, all repo sources JSON, correlation ID, client platform, spawn reason (v2.1.318+: `session_created`, `no_runner_registered`, `hook_failed`, `runner_lost`, `runner_reported_failure`, `resumed`, `requeued`, `server_retry`, `prewarm`). Contract: idempotent on order ID (not session ID), don't retry the workload, exit 0 submitted / 1 retryable / >=2 non-retryable (10 consecutive retryable failures also block), `--expected-spawn-seconds` (default 120, 10-3600) >= p99 spawn-to-register, same value on all replicas. Multiple replicas allowed; each request claimed server-side by one.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-configuration.md (On-demand runners; The spawn-runner hook; Find out why a spawn request was sent; Keep transient failures retryable in a shell hook); https://code.claude.com/docs/en/self-hosted-environments-reference.md (Orchestrator CLI flags)
- Notes: Other flags: `--hook-concurrency` (4), `--min-idle` (pre-warm standby runners), `--shutdown-drain-sec` (v2.1.299+), `--debug-dir`. Blocked sessions show Failed with the last 512 chars of hook stderr; Owner can Retry. Docs supply a `set -e` trap recipe so unexpected exit codes stay retryable.

### Runner idle release, retire, drain and hold
- What: `--release-idle-session-min` releases an idle session slot (session resumes on next user message, on a fresh runner, losing unpushed work); `--kill-session-after-min` backstop with 15-minute grace (v2.1.260+ releases rather than kills); `--exit-if-unused-min` for standby runners; `--retire-at <epoch>` for hosts killed at a known time (stop new work, release sessions after turns finish, wait up to 60 s for background tasks, exit 0); `--defer-shutdown-max-min` keeps serving after first SIGTERM; `--drain-wait-sec` lets turns finish; `--hold-idle-file` + `--max-hold-min` (v2.1.311+) holds idle sessions while host-side processes run, via a file the host touches every <3 min. Background tasks and approvals requested inside a running tool call don't count as idle.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments.md (Runner lifecycle); https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Shutdown timing; Release idle sessions on a fixed fleet; Keep an idle session while its processes run; Some sessions don't count as idle); https://code.claude.com/docs/en/self-hosted-environments-reference.md (Runner CLI flags)
- Notes: Logged stop-timeout budget at defaults: 0 + 5 + 60 + 15 = 80 s (plus 30 with push-outcome). Without `--retire-at`, a signal-less kill looks like a crash.

### VS Code background tasks and agent map
- What: "Run in background" under a tool call (v2.1.287+) appears after a command has run for about 2 seconds, or when the main agent waits on a subagent. Claude stops waiting, and the task notifies it when done. The agent map (`/tasks`, or the agent count in the prompt box) draws the subagent tree with status, elapsed time, and tokens, plus background shells and monitors with live output. Any item can be stopped there. From v2.1.286, Stop or Esc ends the turn but background agents keep running.
- Status: GA
- Evidence: https://code.claude.com/docs/en/vs-code.md (Agent map; Run work in the background)

### Background tasks and Monitor from the SDK
- What: Subagents default to background launch; background Bash commands and subagents report through `task_started` / `task_progress` / `task_updated` / `task_notification` messages (with `run_id`, `spawn_depth`, `is_backgrounded`). `stopTask()`, `backgroundTasks()`. In single-message mode the CLI waits for still-running background subagents (up to a cap) before emitting the result. The `Monitor` tool streams events from a command's stdout lines or a WebSocket's frames (`timeout_ms` default 300000, effective max 30 min) under Bash permission rules.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (Query object; Monitor; SDKTaskStartedMessage); https://code.claude.com/docs/en/agent-sdk/cost-tracking.md (Get the total cost of a query); https://code.claude.com/docs/en/agent-sdk/subagents.md (Choose whether Claude waits for subagents)
- Notes: Tool-input type list also includes CronCreate/Delete/List, ScheduleWakeup, RemoteTrigger, PushNotification, Workflow, EnterWorktree/ExitWorktree, Artifact, Projects (listed in the TS types; behavior documented elsewhere).

### Background sessions and agent view
- What:
  - `claude agents` agent view (research preview at 2.1.139, May 11, 2026) lists every session.
  - Background sessions (`--bg`) are retired and woken by a daemon; flags persist across retire→wake (2.1.143, 2.1.169). Empty idle sessions are retired after 5 minutes (2.1.141).
  - `claude attach|logs|stop|respawn|rm` (2.1.251); `attach` and `logs` accept part of a name (2.1.290).
  - Background agents in worktrees commit, push and open a draft PR (Weeks 27 and 32).
  - MCP tool calls over 2 minutes move to the background (`CLAUDE_CODE_MCP_AUTO_BACKGROUND_MS`, Week 29). MCP-protocol tasks are backgrounded immediately (2.1.319; `CLAUDE_CODE_DISABLE_MCP_TASK_BACKGROUND`).
- Status: agent view was labeled research preview at launch; the rest is GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.139, 2.1.141, 2.1.143, 2.1.169, 2.1.251, 2.1.290, 2.1.319); https://code.claude.com/docs/en/whats-new/2026-w20.md, https://code.claude.com/docs/en/whats-new/2026-w27.md, https://code.claude.com/docs/en/whats-new/2026-w29.md, https://code.claude.com/docs/en/whats-new/2026-w32.md

### Loops, routines, scheduled tasks
- What:
  - `/loop` self-paces when the interval is omitted (Week 15).
  - Routines on the web run templated cloud agents on a schedule, a GitHub event or an API call (Week 16).
  - `/usage` Loops breakdown (2.1.243).
  - `/schedule` warns when a new cloud task gets connectors you did not ask for (2.1.317).
  - `/loop` in cloud sessions notes that the timer dies with the container (2.1.319).
- Status: GA
- Evidence: https://code.claude.com/docs/en/whats-new/2026-w15.md, https://code.claude.com/docs/en/whats-new/2026-w16.md, https://code.claude.com/docs/en/whats-new/2026-w35.md; https://code.claude.com/docs/en/changelog.md (2.1.243, 2.1.317, 2.1.319)

## Area 11. Sessions: persistence, resume, fork, checkpoints, undo

### Session storage as local JSONL transcripts
- What: Every message, tool use and result is written continuously to plaintext JSONL at `~/.claude/projects/<project>/<session-id>.jsonl`, where `<project>` is the cwd path with non-alphanumerics replaced by `-` (truncated to 200 chars plus a hash of the full path). Lines are messages, tool uses/results, records of model requests, or metadata; the format is internal and changes between versions, so scripts should use `/export`, `-p --output-format json`, hooks' `transcript_path`, or the SDK instead. Controls: `CLAUDE_CONFIG_DIR` (move storage), `CLAUDE_CODE_PROJECT_DIR_NAME` (fixed project dir name, 1-64 chars, only with `CLAUDE_CONFIG_DIR`, read once from the launching shell, v2.1.234+; intended for hosts embedding Claude Code per tenant), `cleanupPeriodDays` (30-day default retention), `CLAUDE_CODE_TRANSCRIPT_LOCAL_GC` (cap `-p`/SDK transcript growth), `CLAUDE_CODE_SKIP_PROMPT_HISTORY` (no transcript writes, any mode), `--no-session-persistence` (one `-p` run).
- Status: GA
- Evidence: https://code.claude.com/docs/en/sessions.md (Where transcripts are stored; Name the project directory yourself); https://code.claude.com/docs/en/how-claude-code-works.md (Work with sessions)
- Notes: Sessions are independent: each new session starts with a fresh context; cross-session carry-over is via auto memory and CLAUDE.md only. `claude purge [path]` deletes a project's transcripts, task lists, debug logs, file-edit history, prompt history lines and `~/.claude.json` entry (`--dry-run`, `-y`, `-i`, `--all`) (https://code.claude.com/docs/en/cli-reference.md CLI commands).

### Resume entry points and lookup rules
- What: `claude --continue`/`-c` (most recent in cwd), `claude --resume`/`-r` (picker, or by ID, name, or absolute `.jsonl` path), `claude --from-pr <number|URL>` (picker filtered to sessions linked to a PR; links are made automatically when Claude creates the PR; GitHub, GHE, GitLab MR, Bitbucket), `/resume` in-session (alias `/continue`). Resume reuses the same session ID and appends. ID lookup searches the current project and its worktrees, then every other project; the cross-project match resolves only when exactly one other project holds it (duplicates report not-found). Name lookup spans the repo and its worktrees; ambiguous names open the picker pre-filtered (`--resume`) or error (`/resume`). `-p` and SDK sessions and sessions whose first prompt was `/loop` are excluded from the picker and `--continue` but included by `claude -p --continue`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sessions.md (Resume a session; Resume by session ID or name; /loop, -p, Agent SDK, and background sessions); https://code.claude.com/docs/en/cli-reference.md (CLI flags: --resume, --continue, --from-pr)
- Notes: `--session-id <uuid>` sets the ID for a new conversation. Resuming the same session in two terminals without forking interleaves both into one transcript.

### What a resumed session restores (and does not)
- What: Restores history (opening at the last `Conversation compacted` row: last summary plus unsummarized and newer messages), model, `--agent` agent (searched in the original and the current dir), permission mode (rules below), an active `/goal` (counters reset), and unexpired session-scoped cron tasks. Interrupted tool calls are not rerun. Background Bash, monitors, background subagents and workflows that died with the previous process appear as "didn't finish" notes that Claude reads with the next prompt, without starting a turn. Not restored: `--mcp-config`, `--settings`, `--plugin-dir`, `--fallback-model`, `--add-dir`, mid-session `/add-dir`; settings files are re-read at launch.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sessions.md (What a resumed session restores)
- Notes: The system prompt is recorded on a conversation's first request and reused until compaction, including across `--resume`; changed system-prompt flags apply after compaction or with `--system-prompt-snapshot off` (https://code.claude.com/docs/en/cli-reference.md System prompt flags in resumed conversations).

### Permission mode on resume (security-relevant rules)
- What: Terminal resume by `--continue`, ID or unique name restores the stored mode, except `bypassPermissions` is never restored (must be re-enabled at launch) and `auto` only if the account still qualifies; Manual is restored only where the default would be auto. `-p` resume starts in the mode a new `-p` run would, except a session ended in plan mode resumes in plan when a `--permission-prompt-tool` is passed, no `--permission-prompts none`, no mode flags, no `--fork-session`, not via channels, and `ExitPlanMode` not denied. Picker and `/resume`: start in the mode a new session (or the current session) would, except plan mode is restored. If a deny rule removes `ExitPlanMode`, plan mode is never restored.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sessions.md (Permission mode on resume; Resume in plan mode with -p)
- Notes: Plan restore on non-interactive and VS Code paths needs v2.1.246+.

### Resuming a still-running background session
- What: `claude --resume <session>` on a running background session runs `claude attach` instead of loading the transcript; a prompt on the command line is delivered as its next turn first. It refuses (exit 1, prints the attach command) when input/output is piped, session-configuring flags (`--permission-mode`, `--model`, `--settings`), output-reading flags (`--output-format json`, `--json-schema`), or limit flags (`--max-turns`, `--max-budget-usd`) are present, or agent view is off. Prompts beginning with `/` or `!`, or any prompt while the session awaits an answer, are not sent. `/resume` in-session moves the current conversation to the background and attaches. `--fork-session` resumes a copy instead.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sessions.md (Resume a running background session)
- Notes: Before v2.1.285 resume refused and told the user to attach or stop. `claude --continue` opens finished background sessions (v2.1.257+) but exits with an ID if the most recent one is still running.

### Session picker
- What: Default scope is the current worktree, including background sessions (marked `bg`) and sessions that `/add-dir`-ed the current dir. Keys: arrows/`j``k`, `Enter`, `1`-`9`, `Space` preview, `Ctrl+R` rename, `/` or typing to search (paste a GitHub/GitLab/Bitbucket PR or MR URL to find the session that created it), `Ctrl+A` all projects, `Ctrl+W` all worktrees, `Ctrl+B` current git branch only. Rows show name or generated title, time since activity, branch, file size; duplicate entries for one session group under one row. Selecting a session from another worktree resumes in place (or in cwd if the worktree is gone); from an unrelated project copies a `cd`+resume command to the clipboard. `/cd` relocates a session into the new directory's storage.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sessions.md (Where the session picker looks; Use the session picker; Sessions in other worktrees and projects)
- Notes: A "Resume this conversation?" dialog on Pro/Max/Ultra estimates the share of the 5-hour usage limit a long conversation will cost when resumed after a cache-expiring break or on another model (The "Resume this conversation?" dialog).

### Session naming and generated titles
- What: Name at start (`-n`/`--name`), with `/rename` (shown on prompt bar; control/invisible characters replaced, 200-char cap, v2.1.221+), `Ctrl+R` in picker, from claude.ai for Remote Control sessions, or by accepting a plan (plan-based title). Unnamed interactive sessions get a non-resumable display name (`<dirname>-<2 chars>`) and a resumable title generated by a background call to the small/fast (Haiku-class) model from the first prompt; direct `claude -p` runs get no title.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sessions.md (Name your sessions)
- Notes: `/clear <name>` labels the conversation being left.

### Fork and branch a session
- What: `/branch [name]` copies the transcript up to now into a new session ID and switches the same process to write to it; the original stays on disk. Inherited: history, "allow for this session" grants (same process), in-flight background subagents and Bash (their output lands in the new branch), Remote Control connection. `claude --continue|--resume --fork-session` forks into a new process without session grants. `/fork [prompt]` copies the conversation into a new background session while you keep working here, instructed to create its own worktree before editing (v2.1.221+); `/subtask <task>` spawns a forked background subagent whose result returns to this conversation.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sessions.md (Branch a session); https://code.claude.com/docs/en/commands.md (/branch, /fork, /subtask); https://code.claude.com/docs/en/how-claude-code-works.md (Resume or fork sessions)
- Notes: Without a name, `/branch` names the branch after the first prompt. With agent view off, `/fork` spawns a forked subagent and `/subtask` is unavailable.

### Export and scripted access to sessions
- What: `/export [filename]` writes or copies a rendered plain-text transcript (messages and tool outputs as readable text). For scripts: `claude -p --output-format json|stream-json` (result, session ID, usage, cost), `claude -p --resume <id> "..."` to ask an existing session, `transcript_path` in hook and status-line input (e.g. archive on SessionEnd), or the Agent SDK.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sessions.md (Export and locate session data; Access conversations from scripts)
- Notes: Docs explicitly discourage parsing the JSONL directly.

### Checkpoints and rewind
- What: A checkpoint is captured before each user prompt that starts a turn; snapshots taken before Claude's file-editing tools change a file. Keeps snapshots for the 100 most recent checkpoints per session (older unreferenced snapshot files deleted, except each file's first snapshot, kept as VS Code's diff baseline). Saved with the conversation, so `/rewind` works after resume; deleted by the retention sweep about 30 days after last save (`cleanupPeriodDays`), after which rewind can fail with `No files were restored`. `/rewind` (aliases `/checkpoint`, `/undo`) or double Esc on an empty prompt opens a menu of prior prompts with actions: Restore code and conversation; Restore conversation; Restore code; Summarize from here; Summarize up to here (each with optional focus text); Never mind. Code options only appear when tracked changes exist. After restoring conversation the original prompt is put back in the input.
- Status: GA
- Evidence: https://code.claude.com/docs/en/checkpointing.md (How checkpoints work; Rewind and summarize; Guide a summary); https://code.claude.com/docs/en/how-claude-code-works.md (Undo changes with checkpoints)
- Notes: Summarize is a targeted compaction; original messages stay in the transcript file. Rewound messages can come back if you exit before the next prompt, because the rewound state is saved together with that prompt. After `/clear`, the rewind menu offers `/resume <id> (previous session)` until exit.

### What checkpoints do not track
- What: Files changed only by Bash are not tracked (but once Claude edits a file with its tools, later Bash changes to that file are undone by rewind). Subagent edits are usually not captured: only a foreground forked skill (`context: fork`, `background: false`) is restored; other subagents' edits to files the main conversation did not already edit stay, including background forked skills and background `/code-review --fix`. Manual edits and edits from concurrent sessions are not captured unless they touch the same files. Messages typed mid-turn have no checkpoint (marked "No code restore"). Symlinked and hard-linked paths are skipped on restore with a `Restored the code, but skipped N files` warning (dotfile-manager symlinks, pnpm hard links); `/debug` logs list skipped paths in `~/.claude/debug/<session-id>.txt`. Remote side effects (DBs, APIs, deployments) cannot be checkpointed. Checkpoints are separate from git and not a VCS replacement.
- Status: GA
- Evidence: https://code.claude.com/docs/en/checkpointing.md (Limitations); https://code.claude.com/docs/en/how-claude-code-works.md (Undo changes with checkpoints)
- Notes: Rewind menu counts per-prompt changes to already-edited files whatever made them.

### Background session persistence and restore
- What: Permission mode, model, effort, carried config flags (`--mcp-config`, `--settings`, `--setting-sources`, `--add-dir`, `--plugin-dir`, `--fallback-model`, `--allow-dangerously-skip-permissions`), names and stashed prompts persist across supervisor restarts. Deleting a row keeps the transcript (`claude --resume`). Undeliverable replies are saved and sent as next prompt on restart (not `!`/`/` replies). Attaching to a stopped session shows the transcript tail while the process starts; a recap is posted on attach.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/agent-view.md (What persists across restarts; What carries over when you background; A session is slow to respond after attaching; Opening a session says the conversation is already open)
- Notes: Effort taken from settings is re-read on every process start. A gateway `ANTHROPIC_BASE_URL` exported only in the shell is forwarded to workers only in narrow same-directory cases and never written to disk; docs recommend settings `env` instead.

### Subagent resume, SendMessage addressing, transcripts
- What: Each invocation is a new instance. Resume is done with `SendMessage` to the agent ID or name; the agent resumes in the background, keeping its original tool set and its warm cache. SendMessage does not need agent teams. A stopped subagent (via `x` or SDK `stop_task`) will not auto-resume, and messages to it are refused as cancelled. Name rebinding is protected: if a newer agent has taken a name, the send is refused and the error names the new holder. The check resets on `/clear`. After a restart, names given since the last compaction survive (v2.1.299+); otherwise use the ID from `agent-{id}.meta.json`. Transcripts live at `~/.claude/projects/{project}/{sessionId}/subagents/agent-{agentId}.jsonl`. They are untouched by main compaction and deleted after `cleanupPeriodDays` (30 by default). Subagents auto-compact on their own windows and log a `compact_boundary` record with `preTokens`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Resume subagents; Message a subagent by name after a restart; Auto-compaction)
- Notes: Before v2.1.304, a subagent resumed right after it finished could lose its most recent work. If a subagent resumes another subagent, the results go back to the one that resumed it.

### Worktree session resume and refusal handling
- What: Resuming (interactive, `-p --continue/--resume`, SDK) returns the session to its worktree after re-verifying it. A deleted worktree means resuming in the launch dir and clearing the binding. `--fork-session` starts in the launch dir. Interactive refusals continue without isolation, with messages such as "no longer exists", "Could not verify", "Did not re-enter" and "Could not re-enter". In `-p`/SDK, every refusal except a gone worktree aborts with a stderr error. With stream-json it is a `result` message with subtype `error_during_execution`, an `errors` array and `startup_failure_reason` set to `worktree_unverified` or `worktree_resume_refused` (v2.1.274+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/worktrees.md (Resume a worktree session; The session resumes outside its worktree)
- Notes: Non-interactive resume into a worktree was fixed in v2.1.212. The `result` message for this case was added in v2.1.260.

### Teleport: pull a cloud session into the terminal
- What: `claude --teleport [id]`, `/teleport` or `/tp` picker, `t` in `/tasks`, "Open in > Terminal" from web, or `/teleport` inside the cloud session prints the command. Checks: clean git state (offers stash), same repo not a fork (owner/name match for SSH aliases with confirmation), branch pushed, same account. Fetches and checks out the branch and loads the conversation. Fetch never prompts (password, passphrase, new SSH host fail). Compacted-away messages are not copied; the terminal continues from the summary. The terminal gets its own copy: later local work doesn't appear in the cloud session.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (Continue a cloud session in your terminal; Teleport requirements; --teleport is unavailable)
- Notes: Handoff from CLI is one-way (cloud to terminal); Desktop can push local to cloud. `--resume` doesn't list cloud sessions. Teleport uses the Remote Control session infrastructure, so errors use RC wording.

### Cloud session persistence, idle pause and expiry
- What: After a few minutes idle the VM pauses with files saved; the next message restores the same VM. A paused VM can be reclaimed; reopening provisions a fresh VM with conversation history restored but not running shell commands, subagents (Claude is told which stopped and which can be resumed), or session-scoped scheduled tasks and `/loop`. Waiting on an MCP approval or MCP sign-in counts as inactive. Permission mode is restored on reopen. Users can't pause a VM themselves.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (Environment expired; Permission modes in cloud sessions); https://code.claude.com/docs/en/cloud-environments.md (Set environment variables; Time limits)
- Notes: Env var edits reach a running session only at restore or rebuild.

### Share, archive, delete cloud sessions
- What: Team/Enterprise: Private or Team visibility, repo-access verification on by default, display name shown; Slack sessions auto-shared with Team. Pro/Max/Ultra: Private or Public (any logged-in claude.ai user), verification off by default, warning that sessions may contain code and credentials. Shared views are snapshots, not live. Archive hides from sidebar (Archived tab at claude.ai/code/sessions to unarchive); Delete is permanent with confirmation.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (Share sessions; Archive sessions; Delete sessions)
- Notes: Archived sessions reject `--cloud` follow-ups.

### Resume across runners and work loss
- What: A released or requeued session resumes on a fresh runner from a fresh clone, so unpushed work is lost. `--push-outcome-on-release` makes a best-effort push of session branches before release; resumed sessions start from those (warning: restrict push to `claude/*` refs because the runner fetches without verifying who pushed). Self-hosted runners don't install the commit-and-push Stop hook that Anthropic-hosted sessions have; docs provide a reference Stop-hook script (once per turn, checks uncommitted and unpushed, JSON-escapes branch names to prevent hook-output injection). Same `--base-dir` and `--capacity` required on all runners so recorded absolute paths stay valid after resume.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Resumed sessions lose unpushed work; Keep the base directory and capacity identical across runners); https://code.claude.com/docs/en/self-hosted-environments-configuration.md (Prompt sessions to push their work)
- Notes: Rewind to earlier messages on a runner older than v2.1.246 fails (https://code.claude.com/docs/en/claude-code-on-the-web.md, Can't rewind to this message).

### VS Code session resume, reload continuation, and concurrency guard
- What: After a window reload, editor-tab conversations come back. Sidebar conversations come back only if active within the last 10 minutes. With `continueAfterReload` (v2.1.274+), an interrupted step continues unless it is more than an hour old or open elsewhere. The extension strips `CLAUDE_CODE_RESUME_*` env vars from panel sessions. A conversation that failed to finish opening twice is not reopened automatically (a crash-loop guard). If another Claude process has the conversation open, a notice warns that two writers could mix up its messages and offers "Open here anyway". A pre-restart process usually exits within about 10 minutes. Setting `claudeProcessWrapper` skips this check. Checkpoint rewind offers fork, rewind code, or both. The Web tab downloads a copy of a claude.ai cloud session (one-way, with no sync back) or opens a matching local Remote Control session.
- Status: GA
- Evidence: https://code.claude.com/docs/en/vs-code.md (Continue conversations after a reload; The conversation is still open somewhere else; A conversation was not reopened; Rewind with checkpoints; Resume cloud sessions from claude.ai)
- Notes: The single-writer warning is advisory. The user can override it.

### Sessions in the SDK: continue, resume, fork, persistence off
- What: Sessions are JSONL transcripts under `~/.claude/projects/<encoded-cwd>/` (non-alphanumerics replaced by `-`, names over 200 chars truncated plus hash; `CLAUDE_CONFIG_DIR` and `CLAUDE_CODE_PROJECT_DIR_NAME` relocate them). `continue` picks the newest session in the cwd; `resume` takes an ID; `forkSession` with `resume` branches to a new ID, leaving the original untouched; `resumeSessionAt` resumes at a message UUID (TS), with `resumeDropsTurn` as a guard that refuses truncation if the dropped range has unrelated content (v2.1.223+). TS `persistSession: false` keeps the session in memory only; Python uses `CLAUDE_CODE_SKIP_PROMPT_HISTORY`. Helpers list/read/rename/tag/fork sessions on disk.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/sessions.md (Choose an approach; Resume by ID; Fork to explore alternatives; Resume across hosts); https://code.claude.com/docs/en/agent-sdk/typescript.md (Options)
- Notes: Sessions persist the conversation, not the filesystem; a fork shares the real working tree. Resume lookup searches beyond the current project dir from v2.1.223 (earlier: project dir and its worktrees), but the file must be on the same machine. Python `ClaudeSDKClient` keeps one session across `query()` calls; TS has no session object and uses `continue: true` or a streaming prompt. Docs suggest that passing captured results into a fresh session is "often more robust" than shipping transcripts.

### Pluggable SessionStore adapters (external transcript storage)
- What: A `SessionStore` object with required `append(key, entries)` and `load(key)` plus optional `listSessions`, `listSessionSummaries`, `delete` (must cascade to subkeys and summary), `listSubkeys` (needed to restore subagent transcripts). Key = `{projectKey (encoded cwd), sessionId, subpath?}`; subagents live under `subagents/agent-<id>`. Entries are opaque JSON that must load back deep-equal and in order. Summaries are built with an exported pure fold helper inside `append`, with the adapter responsible for serializing concurrent read-fold-write. Ships `InMemorySessionStore`, copyable reference adapters (S3 part-files, Redis lists, Postgres rows) and a conformance test suite (Python in-package, TS file to copy).
- Status: GA (some knobs alpha: TS `loadTimeoutMs` default 60000, `sessionStoreFlush` `batched`|`eager`)
- Evidence: https://code.claude.com/docs/en/agent-sdk/session-storage.md (The SessionStore interface; Reference implementations; Validate your adapter; Behavior notes; Supported on)
- Notes: Dual-write: the CLI subprocess always writes local disk first and the SDK mirrors each batch; on a resume-from-store the SDK materializes the transcript into a temporary `CLAUDE_CONFIG_DIR` (seeding credentials, `.claude.json`, and in TS a scrubbed user settings.json) and deletes it at run end, so the store becomes the only copy. Mirror writes are best-effort: up to 3 attempts, timeouts not retried, then a `system/mirror_error` message and the batch is dropped while the query continues; adapters must dedupe by `entry.uuid`. Incompatible with `persistSession: false` and with file checkpointing (throws at startup). SDK never deletes from the store; retention is the adapter's job. `forkSession` rewrites session IDs and remaps UUIDs instead of byte copying. `getSessionMessages` returns the post-compaction chain (e.g. 503 raw entries -> 18 messages).

### File checkpointing and rewind
- What: With `enableFileCheckpointing`, the CLI backs up files before Write/Edit/NotebookEdit modify them; with the hidden `replay-user-messages` extra arg, each streamed user message carries a UUID usable as a restore point. `rewindFiles(uuid, {dryRun})` deletes files Claude created and restores modified ones; it does not rewind the conversation. Rewind after the stream ends requires resuming the session with an empty prompt (calling after the loop ends fails with `ProcessTransport is not ready for writing`). CLI equivalent: `claude -p --resume <id> --rewind-files <uuid>` with `CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING=true` (flag undocumented in `--help`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/file-checkpointing.md (How checkpointing works; Implement checkpointing; Limitations; Troubleshooting); https://code.claude.com/docs/en/agent-sdk/typescript.md (Query object)
- Notes: Not tracked: Bash changes (unless the file was previously edited by a file tool in the main conversation), subagent edits (except foreground `context: fork` skills), directory create/move/delete, remote files. Checkpoints are per session. From v2.1.216 rewind skips symlinks, hard links, non-regular files, moved parents and unreadable backups, counting them in `skippedLinks`; before that it wrote through links (a past bug class worth noting).

### Session resume and fork changes
- What:
  - `/fork` was renamed `/branch` (2.1.77, alias kept), then `/fork` was redefined as "copy into a background session" (2.1.212). Copies make their changes in their own worktree (Week 32).
  - `/rewind` can resume from before `/clear` (Week 26).
  - Resume by PR URL (2.1.122).
  - `claude --resume <id> --bg` continues the same ID (2.1.257).
  - 2.1.318 removed the 2.1.288 fix that kept earlier thinking when resuming sessions from 2.1.286 or older.
  - Transcript-write failures (for example a full disk) now warn instead of losing transcripts silently (2.1.217).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.77, 2.1.122, 2.1.212, 2.1.217, 2.1.257, 2.1.318); https://code.claude.com/docs/en/whats-new/2026-w26.md, https://code.claude.com/docs/en/whats-new/2026-w32.md

## Area 12. Git and version control

### Worktree and PR flags
- What: `--worktree`/`-w [name]` starts in `<repo>/.claude/worktrees/<name>` (generated name if omitted); pass `#<n>`, a GitHub PR URL or GitLab MR URL to fetch it from `origin` and branch from it. `--tmux` creates a tmux session for the worktree (iTerm2 native panes when available, `--tmux=classic`). `EnterWorktree` tool: creating new worktrees and entering paths under `.claude/worktrees/` does not prompt; other paths prompt (since v2.1.206); inside a worktree only `.claude/worktrees/` targets allowed; both worktree tools are removed when `permissions.blockWritesOutsideWorkingDirectories` is on.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cli-reference.md (--worktree, --tmux); https://code.claude.com/docs/en/tools-reference.md (EnterWorktree tool behavior)
- Notes: `ExitWorktree` is not given to subagents already pinned to a worktree.

### PR/MR status badge and issue links in footer
- What: Footer shows a clickable `PR #N` badge colored by review state (approved, pending, changes requested, draft), refreshed after a successful `git push` or state-changing `gh pr` command; needs a GitHub token (`GH_TOKEN`, `GITHUB_TOKEN`, `gh auth login`; GHE via `GH_HOST` tokens). GitLab `MR !N` badge (v2.1.234+) via authenticated `glab` CLI only (ignores `GITLAB_TOKEN`). `owner/repo#123` references become links for the remote's host (GitHub/GHE/unknown hosts and gitlab.com; none for bitbucket, codeberg, gitea). Disabled under `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (PR review status; GitLab merge requests; Issue reference links)
- Notes: `FORCE_HYPERLINK=0/1` controls link rendering.

### Git workflows and PR-to-session linking
- What: Git is driven conversationally (status, commit with generated message, branch, resolve conflicts, create PR). Claude Code links a session to a PR when Claude creates it with `gh pr create` or `glab mr create` or works on an existing PR; `--from-pr` and pasting the PR URL in the picker find it. Worktrees need at least one commit; `.worktreeinclude` (gitignore syntax) copies small gitignored files like `.env` into each new worktree; `worktree.symlinkDirectories` for large dirs. Unnamed empty worktree sessions are removed on exit; named or dirty ones prompt; subagent worktrees vanish if unchanged; leftovers swept after `cleanupPeriodDays`; `claude -p --worktree` must be cleaned up manually.
- Status: GA
- Evidence: https://code.claude.com/docs/en/common-workflows.md (Create pull requests; Run parallel sessions with worktrees); https://code.claude.com/docs/en/quickstart.md (Step 5: Use Git with Claude Code); https://code.claude.com/docs/en/best-practices.md (Run sessions in parallel)
- Notes: Built-in commit/PR instructions and attribution trailers can be turned off (`includeGitInstructions`, `attribution`).

### Sparse worktrees and shared directories
- What: `worktree.sparsePaths` makes `--worktree` and subagent worktrees use git sparse-checkout for the listed directories plus all root-level files; lists merge across scopes (local can add, not remove). `worktree.symlinkDirectories` symlinks e.g. `node_modules` back to the main checkout. Both are read from the starting directory before creation; inside the worktree, project settings come from the worktree root.
- Status: GA
- Evidence: https://code.claude.com/docs/en/large-codebases.md (Check out only the directories you need)
- Notes: Claude Code enables `extensions.worktreeConfig` in the shared `.git/config` while a sparse worktree exists and removes it after the last one only if it added it. WorktreeCreate/WorktreeRemove hooks can replace git worktrees entirely (see hook entries).

### Background worktree isolation, auto-commit and safe delete
- What: Dispatched background sessions start in the working dir and move into a git worktree under `.claude/worktrees/` before editing; isolation is then enforced for the session and its subagents. Skipped for sessions backgrounded with ←, sessions already in a linked worktree, non-git dirs without a `WorktreeCreate` hook, and writes outside the cwd. `worktree.bgIsolation: "none"` disables. On finish in its own worktree, Claude commits without asking, pushes if a remote exists, opens a draft PR when the task calls for it; never pushes to main/master, force-pushes or merges; user git instructions in task/CLAUDE.md/memory override. Deleting a session removes its worktree, but refuses when another session uses/locks it, when commits are unpushed (second delete or `--discard-unpushed` discards), or when git/`WorktreeRemove` hook fails (`--force-remove-worktree` under strict checks).
- Status: research preview
- Evidence: https://code.claude.com/docs/en/agent-view.md (How file edits are isolated; What deleting a session removes; Version history)
- Notes: Agent view delete removes uncommitted changes; `claude rm` keeps a worktree with uncommitted changes. Backgrounded sessions hold `git worktree lock` (v2.1.248). Commits already on local default branch checked out in the main checkout count as saved.

### Pull request status on agent-view rows
- What: When a session opens or works on a PR (detected from `gh` command output, `gh pr checkout`, or pushes followed by `gh pr view` lookups retried after up to five later git/gh/glab/curl commands), the row shows `#N` (GitHub) or `!N` (GitLab MR) as a hyperlink colored by status: yellow (checks/review pending or failing), green (passing, no blocking review), purple (merged), grey (draft/closed). Multiple PRs show a count. `Ready for review` group collects sessions with PRs needing review or failing checks.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/agent-view.md (Pull request status; Organize the list)
- Notes: `gh pr merge` usually creates no link because its output goes only to a TTY. `FORCE_HYPERLINK=0` renders plain text.

### Worktree sessions (--worktree / -w)
- What: `claude -w <name>` creates `.claude/worktrees/<name>/` on branch `worktree-<name>` (a random name if omitted) and starts the session there. Interactive use needs workspace trust; `-p` skips it. `EnterWorktree` and `ExitWorktree` let Claude create or switch worktrees mid-session. Entering a path outside `.claude/worktrees/` always prompts, even with allow rules, unless in bypass mode. Both tools are removed when `permissions.blockWritesOutsideWorkingDirectories` is on. Hooks keep `${CLAUDE_PROJECT_DIR}` at the original root, while `cwd` in the hook input follows the worktree. The transcript is re-homed to the new cwd. Reusing a name reopens the worktree. With `fresh` base, a clean worktree with no own commits, or whose PR was merged and branch deleted, resets to the default branch.
- Status: GA
- Evidence: https://code.claude.com/docs/en/worktrees.md (Start Claude in a worktree; Ask Claude to create a worktree; Reuse a worktree name)
- Notes: `--worktree "#1234"` or a GitHub PR / GitLab MR URL fetches the change's head from `origin` (pull/N/head or merge-requests/N/head, tried by host) into `pr-<N>`. Fetches never wait for interactive auth (v2.1.233+ for GitLab).

### Worktree base branch, .worktreeinclude, LFS/filter-driver neutralisation
- What: `worktree.baseRef` is `fresh` (default: remote default branch; fetched if older than 24h, 5-second cap, non-interactive, falls back to local HEAD) or `head` (current HEAD, or the worktree's own HEAD when inside one). Branch names are not accepted. `.worktreeinclude` (gitignore syntax) copies files into new git-created worktrees, but only files that are gitignored, for example `.env`. Security behaviour: the repo's own filter drivers (such as `git lfs install --local`) are not run during creation, because any writer to `.git/config`, Claude included, could plant one. Creation is refused when config cannot be read, a driver name cannot be neutralised, there is an `includeIf` in the repo config, or there are LFS keys naming programs (`lfs.customtransfer.*.path`). `GIT_NO_REPLACE_OBJECTS=1` is forced. Symlinked `.claude` or `.claude/worktrees` paths are refused.
- Status: GA
- Evidence: https://code.claude.com/docs/en/worktrees.md (Choose the base branch; Copy gitignored files into worktrees; Git LFS files are pointer files...; Worktree creation fails on a symlinked path; A worktree Claude Code created has the original content of replaced objects)
- Notes: Before v2.1.247 filter drivers did run. Before v2.1.212 committed symlinks could redirect worktree creation outside the repo.

### Subagent worktrees and cleanup/locking
- What: With `isolation: worktree`, or when asked, each subagent gets a temporary worktree based on the same baseRef (the default branch, not the parent's HEAD, unless baseRef is `head`). It is removed if unchanged. A changed one stays until a periodic sweep removes subagent and background-session worktrees older than `cleanupPeriodDays`. The sweep skips worktrees with changes, untracked files, unpushed commits or dirty submodules, un-backgrounded `--worktree` sessions, and user-created worktrees (it uses a marker in git metadata). `git worktree lock` is held while an agent runs. The sweep releases locks of dead processes but never locks the user set. Interactive exit checks for changes: a clean unnamed worktree is auto-removed, otherwise you are prompted. `-p` runs never clean up and leave the lock. Subagent worktrees take instruction files from the main conversation, not the worktree branch.
- Status: GA
- Evidence: https://code.claude.com/docs/en/worktrees.md (Isolate subagents with worktrees; Clean up subagent and background-session worktrees; Worktree locks and manual removal; Clean up worktrees)
- Notes: Shared with main: `.git`, project-scope plugins, and "don't ask again" approvals (saved to main's `settings.local.json`). Untracked `.claude/skills`, `agents` and `commands` are read through from main when the worktree lacks them (skills: v2.1.277+).

### Local /code-review (bundled skill)
- What: `/code-review [effort] [--fix] [--comment] [--max-findings n|all|default] [target]` (alias `/review`). In a terminal it runs as a background forked subagent; in `-p`/SDK it runs in the foreground. Targets: current branch plus uncommitted changes (default: commits ahead of upstream, else main), PR number, branch range, revision range, or path. `--fix` applies findings, but background edits are outside checkpoints, so `/rewind` does not undo them. `--comment` posts inline GitHub PR comments, or a GitLab MR note via `glab` (v2.1.257+). Effort `low` reports high-confidence findings only; higher levels broaden coverage. The last typed level is remembered across sessions, except levels passed in `-p`. Hosts can request structured output through the `ReportFindings` tool (max 32), and fixed findings are re-reported as fixed, skipped or no_change_needed. It follows CLAUDE.md, not REVIEW.md. Claude can start it on its own, as can scheduled tasks; `skillOverrides: {"code-review": "user-invocable-only"}` prevents that.
- Status: GA
- Evidence: https://code.claude.com/docs/en/code-review-local.md (Choose what to review; What the review reads and edits; How findings are reported; Tune effort and arguments; Let Claude start the review)
- Notes: The command was called `/simplify` before v2.1.147. `/simplify` now names a separate cleanup-only command. A project skill named `code-review` overrides the bundled one, while `/review` still runs the bundled one.

### GitHub connection methods for cloud sessions
- What: GitHub App (OAuth during onboarding; public repos plus private repos where the App is installed; needed for Auto-fix and project threads) or `/web-setup`, which reads `gh auth token`, asks to confirm, and sends it to Anthropic, stored encrypted against the claude.ai account; gives access to whatever the token can reach. Removing: disconnect at claude.ai connectors (deletes stored creds; token stays valid on GitHub) or revoke on GitHub. Warns if the token lacks `workflow` scope.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (GitHub authentication options); https://code.claude.com/docs/en/web-quickstart.md (Connect GitHub; Connect from your terminal; Remove the /web-setup token)
- Notes: On Team/Enterprise `/web-setup` is hidden until an Owner enables Quick setup, and the GitHub connector must be enabled by an Owner. GitHub Enterprise Server supported on Team/Enterprise.

### GitHub proxy (Anthropic-hosted)
- What: All GitHub operations from the VM go through a dedicated proxy; real credentials never enter the VM. git inside the VM uses a scoped credential that the proxy verifies and swaps for the real token. `GH_TOKEN`/`GITHUB_TOKEN` read as the placeholder `proxy-injected` unless you set your own (which then passes through and is readable by all env users). Restrictions: rejects branch deletion and non-branch pushes (tags); doesn't limit which branches; API only for repos attached to the session (403 otherwise, including archive/codeload URLs of public repos); rejects all GraphQL (so `gh pr`, `gh issue` fail; use `gh api` REST), regardless of credentials.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cloud-environments.md (GitHub proxy; Work with GitHub issues and pull requests); https://code.claude.com/docs/en/claude-code-on-the-web.md (Security and isolation)
- Notes: Built-in GitHub tools (read issues, list PRs, diffs, comments) authenticate via the proxy. Self-hosted runners can opt in to the same model (`--use-anthropic-git-proxy`).

### Auto-fix pull requests
- What: Claude subscribes to GitHub activity on a PR; on a CI check failure or review comment it investigates and pushes a fix if clear, asks you if ambiguous or architecturally significant, notes duplicates/no-ops. Turn on from the Changes card menu, `/autofix-pr` in the terminal on the PR branch (detects PR with `gh`, spawns a cloud session, enables auto-fix), by telling Claude on mobile, or by pasting a PR URL into any session. Replies to review threads are posted under the user's GitHub account but labelled as Claude Code.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (Auto-fix pull requests; How Claude responds to PR activity)
- Notes: Requires the Claude GitHub App. Can't react to merge conflicts because GitHub emits no webhook when the base advances. Warning: its comments can trigger `issue_comment` automations (Atlantis, Terraform Cloud).

### Session provenance links in commits and PRs
- What: Commits created in a cloud session carry a `Claude-Session: <url>` trailer and PR bodies include the session URL; disable with `attribution.sessionUrl: false`. `CLAUDE_CODE_REMOTE_SESSION_ID` (`cse_` prefix) is available; replace with `session_` for the transcript URL.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cloud-environments.md (Link output back to the session); https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Let the runner configure git)
- Notes: Same behaviour on self-hosted runners.

### Self-hosted git options
- What: `--configure-git` writes identity `Claude <noreply@anthropic.com>`, SSH-format commit signing via a runner shim that calls Anthropic's signing service with session credentials (verifiable on GitHub), `push.negotiate`, and commit-msg hooks adding a `Co-authored-by:` for the creator. Or ship your own config. Runner disables prompts (`GIT_TERMINAL_PROMPT=0`, SSH `BatchMode=yes`, `GCM_INTERACTIVE=never`, clears `core.askPass`). `--git-host-rewrite from=to` (split-horizon DNS) and `--git-ssh-rewrite host`. `--use-anthropic-git-proxy`: github.com only, capacity 1, git 2.32+, v2.1.267+, Anthropic serves clone/fetch/push using the creator's stored GitHub OAuth token (or org GitHub App token for bot sessions) over a WebSocket to `api.anthropic.com`; deletes and replaces the runner user's global git config with no backup; branch pushes only; built-in `gh api`-only shim (v2.1.287+). Non-GitHub hosts: dispatch with `claude -p ... --environment ccpool_...` from a checkout (path must be `owner/name`).
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Configure git; Let the runner configure git; Ship git config in your image; Use the Anthropic git proxy; Rewrite git URLs for private networks; Start a session on a repository outside GitHub)
- Notes: Git floors: 2.24 base, 2.29 push-outcome, 2.32 proxy, 2.34 signing. Per-git-op watchdog: 120 s no-progress, 30-minute hard cap.

### Managed Code Review service (pipeline)
- What: This is a managed PR reviewer that runs on Anthropic infrastructure in four stages. (1) Find: several agents read the diff and the surrounding code in parallel. (2) Verify: a verification step checks candidate findings against the code and drops false positives. (3) Rank: the remaining findings are deduplicated and ranked by severity. (4) Post: inline comments, a summary in the review body, and a check run named "Claude Code Review". The default focus is correctness (bugs that would break production), not style or test coverage. Inside a review, repository access is read-only. A review never pushes, builds, or runs code. It never approves or blocks a PR, and its check run never concludes as failure (neutral, or skipped). Code Review picks the models itself; admins cannot choose one.
- Status: GA (Team and Enterprise only)
- Evidence: https://code.claude.com/docs/en/code-review.md (How reviews work; What a review can access; Who controls a review)
- Notes: Not available with Zero Data Retention, the HIPAA configuration, or CMEK, and not covered by the BAA. A Claude org on Team or Enterprise is needed even if developers use Bedrock, Vertex, or Foundry. Supports github.com, GHES, and GHE Cloud with data residency (ghe.com). Does not run in self-hosted environments.

### Code Review levels and triggers
- What: There are three review levels: Quick, Standard (the default), and Deep. Cost and duration rise in that order. The level is set for the org and can be overridden per repository. The per-repo "Review runs" setting is one of: Once after PR creation (opened, reopened, or marked ready); After every push (auto-resolves threads when an issue is fixed; this is the default for newly added repos); Manual. Automatic reviews skip drafts and forks. Owners can cover a whole GitHub org at once, but only on github.com, only if the connected account owns that org, and only with a monthly spend limit set. "Tip comments", on by default, explain on the PR why no review started (Manual repo, fork, a commenter without write access, org not linked).
- Status: GA
- Evidence: https://code.claude.com/docs/en/code-review-setup.md (Set the default review level; Set when reviews run; Cover a whole GitHub organization; Turn tip comments on or off)
- Notes: A new level applies only to reviews that start after the change.

### Code Review comment commands and PR interaction
- What: The comment commands are `@claude review` (one review, no subscription), `@claude review once` (same), `@claude review always` (review now and on every later push), and `@claude review stop|off|pause` (end the subscription; Claude replies with a comment). To count, a command must be a new top-level PR comment with the command at the start of its first line, from someone with write, maintain, or admin access (checked with GitHub per commenter). Bot comments never count, and the PR must be open. Edits to earlier comments are ignored. A misspelled `always` or `stop` falls back to a single review. An accepted command produces a check run titled "Code review queued..." and then "...in progress...". A 👀 reaction is not proof that a review started. If a review is already running on the latest commit, a new request does not start a second one. On forks, write access to the base repo is what counts, and `always` does not subscribe. Before a July 2026 update, `@claude review` subscribed by default.
- Status: GA
- Evidence: https://code.claude.com/docs/en/code-review-pull-requests.md (Comment commands; Review drafts and pull requests from forks)
- Notes: When the Claude GitHub Action is also installed with its default `@claude` trigger, an `@claude review` comment also starts the Action. The fix is to change the Action's `trigger_phrase`.

### Code Review findings, severities, re-review and dismissal
- What: There are three severities: 🔴 Important (fix before merge), 🟡 Nit, and 🟣 Pre-existing (a bug not introduced by the PR). A finding can include a collapsed "Why this was flagged" section with the reasoning and how it was verified. A reply does not dismiss a finding, and Claude does not answer replies right away. The next review reads the replies and may resolve the thread or post one follow-up. Resolving the thread, or pushing a fix that the next review judges fixed, clears the finding. Later automatic reviews focus on what changed and post fewer nits and pre-existing findings. The review body counts what it left out ("outside the scope of this re-review", "found and not posted"). A manual `@claude review` covers the whole PR again. A push can "keep the earlier result" without re-reviewing when the PR still adds and removes the same lines (empty commit, clean merge or rebase) and nobody has resolved an Important thread. That shows as "skipped — no new changes" or "not repeated — earlier findings still open". An interrupted review is retried once automatically.
- Status: GA
- Evidence: https://code.claude.com/docs/en/code-review-pull-requests.md (Severity levels; Respond to findings; When a push keeps the earlier result; What a later review covers); https://code.claude.com/docs/en/code-review-troubleshooting.md (Check how far the review got; Find issues that aren't showing as inline comments)
- Notes: Findings GitHub rejects as inline comments appear as annotations under Files changed, or under "Additional findings" in the review body. The check run Details lists them all.

### Code Review machine-readable result for CI gating
- What: The last line of the check run's `output.text` is an HTML comment holding severity counts as JSON, for example `<!-- bughunter-severity: {"nit":1,"normal":2,"pre_existing":0} -->`, where `normal` is the Important count. The docs give `gh api` plus jq recipes and an order for handling cases: a "not repeated" title (no severity line, so treat as gate-fail), a "no new changes" title (pass), `normal > 0` (fail), and a missing line, which means no verdict, not zero findings. Failed, timed-out, canceled, and superseded runs have no line. Parse only the last line, because the lines above it can contain PR file names.
- Status: GA
- Evidence: https://code.claude.com/docs/en/code-review-pull-requests.md (Read the result from a script)
- Notes: The internal name "bughunter" leaks into the format. The docs also warn about injection through file names in the text above the count line.

### Claude Code GitHub Action (claude-code-action@v1)
- What: A GitHub Action built on the Agent SDK that runs Claude Code in workflows.
  - Interactive mode (no `prompt` input): waits for the trigger phrase (default `@claude`) in issue or PR comments, PR reviews, or a new issue's title or body, then reports progress in a comment it keeps updating.
  - Automation mode (`prompt` set): runs on any event, including cron. Output goes to the run log unless the prompt and its tools post somewhere.
  - Main inputs: `prompt` (text or `/skill`), `claude_args` (any CLI flags such as `--max-turns`, `--model`, `--mcp-config`, `--allowedTools`, `--append-system-prompt`), `anthropic_api_key`, `claude_code_oauth_token`, `github_token`, `plugin_marketplaces`, `plugins`, `settings` (JSON or a path), `trigger_phrase`, `use_bedrock`, `use_vertex`, `use_foundry`.
  - Tools: in automation mode with a plain prompt, Claude has no shell or GitHub API access until tools are granted with `--allowedTools` or a `permissions.allow` rule in `settings`. A skill gets the tools in its `allowed-tools` frontmatter. The inline-comment MCP server starts only when `--allowedTools` names `mcp__github_inline_comment__create_inline_comment`.
- Status: GA (v1; `@beta` is superseded, with a migration guide)
- Evidence: https://code.claude.com/docs/en/github-actions.md (Interactive and automation modes; Run a skill; Run on a schedule; Action parameters; Pass CLI arguments; Upgrade from beta)
- Notes: The review workflow posts to the PR from v2.1.229. Before that it wrote only to the run log.

### Comparison of review options (official matrix)
- What: The docs compare five options. Managed Code Review: Team or Enterprise, on Anthropic infrastructure, triggered by PR activity or comments. Local `/code-review`: any plan, runs in the user's session, reads only `CLAUDE.md`. Ultrareview: a cloud sandbox, needs a claude.ai login. Your own CI with GitHub Actions or GitLab. A custom reviewer subagent or skill.
- Status: GA
- Evidence: https://code.claude.com/docs/en/code-review.md (Compare ways to get a review)
- Notes: Ultrareview and `/code-review` pages were not read in this slice.

### Git-related controls exposed to SDK hosts
- What: The built-in commit/PR instructions (part of the Bash tool description) and git status snapshot can be removed with `includeGitInstructions: false`; commit trailer and PR footer come from `attribution.commit` / `attribution.pr`. TS `projectConfigRoot` (v2.1.275+) lets a session run in a worktree `cwd` while reading project settings, `.mcp.json` and `.claude/` content from the trusted main checkout. Resume refuses or flags sessions whose worktree cannot be verified (`worktree_resume_refused`, `worktree_unverified`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/modifying-system-prompts.md (Turn off the context your agent replaces); https://code.claude.com/docs/en/agent-sdk/typescript.md (Options; startup_failure_reason)
- Notes: No SDK-specific PR or review workflow documented in this slice.

### Git hosting: GitLab parity
- What:
  - GitLab MR URLs in `--worktree` and agent view (2.1.233).
  - Footer and status-line `MR !N` badge via `glab` (2.1.234). `glab mr` commands are recognised (2.1.259).
  - Bare gitlab.com marketplace URLs (2.1.232).
  - Redaction of GitLab token families (2.1.232).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.232, 2.1.233, 2.1.234, 2.1.259); https://code.claude.com/docs/en/whats-new/2026-w33.md

### Code review commands
- What:
  - `/simplify` was renamed `/code-review` (2.1.147, May 21, 2026), changing from cleanup to a correctness-bug report at an effort level; `--comment` posts inline PR comments.
  - It runs as a background subagent (Week 30). `/review` is an alias (2.1.223). `--max-findings` (2.1.288). `/code-review ultra` runs the deep cloud review.
  - `/ultrareview` was a research preview in Week 17; its browser link was removed in 2.1.286.
- Status: GA (ultrareview was labeled research preview at launch)
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.147, 2.1.223, 2.1.286, 2.1.288); https://code.claude.com/docs/en/whats-new/2026-w17.md, https://code.claude.com/docs/en/whats-new/2026-w30.md, https://code.claude.com/docs/en/whats-new/2026-w32.md
- Notes: the current system skill list shows `/simplify` again as a separate quality-cleanup skill. That observation comes from the environment, not the docs, so it is unverified as docs.

## Area 13. Extensibility: MCP, plugins, skills, hooks, custom commands, SDKs, protocols

### Slash command system (built-ins, bundled skills, bundled workflows, MCP prompts)
- What: Commands are recognized only at the start of a message; trailing text is the argument. Up to six skills can be chained in one message (`/skill-a /skill-b do X`). Entries are of three kinds: built-in (coded into the CLI), bundled Skill (a prompt handed to Claude, like user skills), and bundled Workflow (fans out across subagents in the background). MCP servers' prompts appear as commands. The `/` menu filters by name/alias prefix or word start ignoring `:`, `_`, `-` (v2.1.236+); on typos nothing is highlighted and Enter reports Unknown command; unavailable commands are omitted; a few (e.g. `/heapdump`) are hidden until typed in full. Availability varies by platform, plan, provider and org policy.
- Status: GA
- Evidence: https://code.claude.com/docs/en/commands.md (intro; All commands; How the command menu matches what you type; MCP prompts)
- Notes: User-defined commands are skills.

### Slash command families (complete list grouped)
- What: From the commands table (Skill = bundled skill; WF = bundled workflow):
  - Session and history: `/clear` (`/reset`, `/new`), `/resume` (`/continue`), `/branch`, `/fork`, `/subtask`, `/rename`, `/export`, `/rewind` (`/checkpoint`, `/undo`), `/recap`, `/copy [N]`, `/exit` (`/quit`), `/background` (`/bg`), `/stop`, `/teleport` (`/tp`), `/desktop` (`/app`), `/restart` (`/update`), `/cd`, `/add-dir`.
  - Context and model: `/compact`, `/context`, `/autocompact`, `/btw`, `/model`, `/effort` (incl. `ultracode`), `/fast`, `/advisor`, `/output-style`, `/goal`, `/plan`.
  - Permissions and safety: `/permissions` (`/allowed-tools`), `/sandbox`, `/auto-mode-setup`, `/fewer-permission-prompts` (Skill), `/privacy-settings`.
  - Parallel and background: `/tasks` (`/bashes`), `/batch` (Skill), `/loop` (Skill, `/proactive`), `/schedule` (`/routines`), `/workflows`, `/workflow-authoring` (Skill), `/list-agents` (`/peers`), `/agents` (now prints a reminder; interactive UI only on v2.1.197 and earlier), `/deep-research` (WF).
  - Code review and verification: `/diff`, `/code-review` (Skill, `/review`; levels low..max, `ultra`, `--fix`, `--comment`, `--max-findings`), `/ultrareview`, `/security-review`, `/simplify` (Skill, four parallel review agents), `/run`, `/verify`, `/run-skill-generator` (Skills), `/autofix-pr`.
  - Config and extensibility: `/config` (`/settings`, `key=value` form), `/memory`, `/init`, `/import [codex|gemini|cursor]`, `/hooks`, `/mcp`, `/plugin`, `/reload-plugins`, `/reload-skills`, `/skills`, `/skill-doctor`, `/plugin-authoring`, `/update-config` (Skill), `/keybindings`, `/statusline`, `/theme`, `/color`, `/tui`, `/focus`, `/scroll-speed`, `/terminal-setup`, `/voice`, `/ide`, `/chrome`, `/claude-in-chrome` (Skill).
  - Diagnostics and account: `/doctor` (Skill, `/checkup`, `prompt-audit`), `/debug` (Skill), `/heapdump` (hidden), `/status`, `/usage` (`/cost`, `/stats`), `/insights`, `/release-notes`, `/feedback`, `/bug` (`/share`), `/login`, `/logout`, `/upgrade`, `/usage-credits`, `/rate-limit-options`, `/passes`, `/stickers`, `/radio`, `/powerup`, `/mobile` (`/ios`, `/android`).
  - Cloud, remote, integrations: `/remote-control` (`/rc`), `/remote-env`, `/web-setup`, `/install-github-app`, `/install-slack-app`, `/team-onboarding`, `/setup-bedrock`, `/setup-vertex` (hidden unless provider env set).
  - Artifacts and design: `/artifacts`, `/artifact-capabilities`, `/artifact-diagramming`, `/design`, `/design consent|revoke`, `/design-login`, `/design-sync`, `/slides`, `/dataviz`, `/claude-api` (Skills).
  - Removed: `/pr-comments` (v2.1.91), `/vim` (v2.1.92, now `/config` Editor mode), `/ultraplan`.
- Status: GA (individual commands version- or plan-gated as stated on the page)
- Evidence: https://code.claude.com/docs/en/commands.md (All commands)
- Notes: Several commands work in `-p`: `/color`, `/config key=value`, `/effort` (v2.1.205+), `/model <m>` (not saved as default), `/rename`, `/mcp` (prints summary), `/reload-plugins`, `/import` (lists). `/design-login` and `/design-sync` target standalone claude.ai/design, which the page says closes by December 14, 2026.

### Extension layer map and context cost per feature
- What: Extensions are positioned by where they plug into the loop: CLAUDE.md (always-on context), output styles (role/tone for the session), skills (descriptions at start, body on use), code intelligence (LSP), MCP (tool names at start, schemas deferred via tool search, on by default), subagents (isolated context returning summaries), dynamic workflows (a script Claude writes that runs many subagents and returns one result), cross-session messaging, hooks (command, HTTP, MCP tool, prompt, or subagent handlers on lifecycle events; zero context cost unless they return output), artifacts, and plugins/marketplaces as the packaging layer. The docs state guardrails belong in hooks because CLAUDE.md or skill instructions are requests, not guarantees.
- Status: GA
- Evidence: https://code.claude.com/docs/en/features-overview.md (Overview; Match features to your goal; Compare similar features; Context cost by feature; Understand how features load)
- Notes: Advised CLAUDE.md size: under 200 lines. Subagents load their own system prompt (not Claude Code's), listed skills in full, CLAUDE.md and git status (Explore and Plan get neither; `omitClaudeMd` drops them); forks load the parent conversation, system prompt and tools.

### MCP trust model
- What: Project servers live in checked-in `.mcp.json`, but other scopes, claude.ai connectors and plugins also add servers, so reviewing `.mcp.json` is not a full inventory. Anthropic reviews directory connectors against listing criteria but does not security-audit or manage any MCP server; `managed-mcp.json` restricts servers org-wide.
- Status: GA
- Evidence: https://code.claude.com/docs/en/security.md (MCP security)

### Security guidance plugin (in-session self review)
- What: Official plugin (`security-guidance@claude-plugins-official`) built entirely on hooks that reviews Claude's own changes at three depths: per-edit deterministic pattern match on Edit/Write/NotebookEdit (e.g. `eval(`, `pickle`, `innerHTML`, `.github/workflows/` edits), warning once per pattern per file per session, no model call; end-of-turn background model review of the git diff of everything changed in the turn (up to 30 files, at most three consecutive re-prompts); agentic commit/push review (reads callers and sanitizers; only for commits Claude makes via Bash; capped at 20 per rolling hour). Reviews are a separate Claude call with fresh context. Nothing blocks writes or commits; findings re-prompt Claude. Extensible with `.claude/claude-security-guidance.md` (user/project/local, concatenated, 8 KB cap) and `.claude/security-patterns.yaml|yml|json` (regex or substrings, path globs, max 50 rules, catastrophic-backtracking regexes skipped). Defaults to an Opus model (`SECURITY_REVIEW_MODEL`, `SG_AGENTIC_MODEL`); per-layer env toggles.
- Status: GA (available on all plans)
- Evidence: https://code.claude.com/docs/en/security-guidance.md (What the plugin checks; Review independence and limits; Add your own rules; Usage cost; Disable or uninstall; How the plugin integrates with Claude Code)
- Notes: Requires Python 3.7+ (3.10+ for agentic review); creates a venv under `~/.claude/security/` and installs the Agent SDK on first run (needs network). Hook events used: SessionStart, UserPromptSubmit (baseline), PostToolUse, Stop. Not loaded in cloud sessions from user or repo settings. Guidance is additive; a rule saying "ignore X" does not suppress findings.

### Claude Security plugin (multi-agent vulnerability scan)
- What: `/claude-security` plugin runs a multi-agent scan of a whole repo or a diff (branch, PR, commit): architecture map, threat model, vulnerability hunting, and independent verifier agents per finding; scan effort low/medium (default)/high. Writes a timestamped `CLAUDE-SECURITY-<timestamp>/` dir with Markdown report, JSONL, SARIF 2.1.0 (CWE-classified), and a revision stamp (commit, effort, dirty tree, verification depth), with its own `.gitignore`. "Suggest patches" drafts per-finding patches in a scratch copy, has an independent agent review each (runs tests when present) and writes `F<n>.patch` only if it can vouch for fix, no new vulnerability and unchanged behaviour; patches are never auto-applied and stale findings are skipped. Uses the session model (mapping uses `sonnet`); built on dynamic workflows.
- Status: GA (managed Claude Security product is Enterprise-only, separate)
- Evidence: https://code.claude.com/docs/en/claude-security.md (Scan and fix your codebase; Choose a scan effort; Read the scan results; Fix findings; Models and providers)
- Notes: Needs a paid plan, API access or third-party provider; Python 3.9+ stdlib only; git for change scans. Docs state scans are nondeterministic. On Fable models, cybersecurity classifiers may flag requests and re-run them on Opus via automatic fallback.

### Hooks: model and configuration
- What: User-defined handlers run at lifecycle events in every surface (terminal, IDE, Desktop, cloud). Three-level config: event -> matcher group -> handler list. Locations: `~/.claude/settings.json`, `.claude/settings.json`, `.claude/settings.local.json`, managed policy, plugin `hooks/hooks.json`, skill frontmatter, subagent frontmatter. Entries merge across levels (never replace); an identical handler defined in several settings files runs once. Matching hooks for an event run in parallel (max 12 prompt/agent hooks concurrently), all run to completion before results merge, so a sibling's deny does not stop another hook's side effects. Settings hooks also fire inside subagents with `agent_id`/`agent_type`. File watcher picks up edits live. Five events have per-session/per-turn/per-tool cadence; 31 events total.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Hook lifecycle; Configuration; Hook locations; Hook handler fields); https://code.claude.com/docs/en/hooks-guide.md (Combine results from multiple hooks)
- Notes: Cloud sessions do not read `~/.claude/settings.json`. Mods (in-process JS plugin hooks) are a separate mechanism covered elsewhere. Multiple PreToolUse `updatedInput` rewrites are not merged and the winner is unspecified.

### Hook matchers and the `if` filter
- What: `matcher` of `*`, empty or omitted matches all; only letters/digits/`_`/`-`/space/`,`/`|` means exact string or list; anything else is an unanchored JavaScript regex (`Edit.*` also matches `NotebookEdit`). Each event matches a different field (tool name, session source, notification type, agent type, compaction trigger, model canonical name, config source, error type, load reason, command name, MCP server name). Matchers on events without matcher support are silently ignored. Per-handler `if` uses one permission-rule pattern (`Bash(git *)`, `Edit(*.ts)`) on tool events only; Bash matching strips leading `VAR=` assignments, checks every subcommand and `$()`/backtick contents, and runs the hook anyway when it cannot parse the command. `if` on a non-tool event means the hook never runs.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Matcher patterns; Common fields; How if patterns match Bash commands; Match MCP tools; Match Monitor commands)
- Notes: MCP tools are `mcp__<server>__<tool>`; plugin MCP tools are `mcp__plugin_<plugin>_<server>__<tool>`; `mcp__memory` alone matches nothing (exact path). A `Bash` matcher also sees Monitor-tool shell commands but its `allow`/`updatedInput` do nothing there (v2.1.314+). `FileChanged`/`StopFailure` use a narrower exact-match charset. Matchers are case-sensitive.

### Hook handler types
- What: Five types: `command` (stdin JSON, exit code + stdout), `http` (POST JSON body, response body JSON), `mcp_tool` (call a tool on a configured MCP server; its text output parsed like stdout), `prompt` (single-turn LLM returning `{ok, reason, impossible}`), `agent` (subagent with tools, up to 50 turns). Common fields: `type`, `if`, `timeout`, `statusMessage` (spinner text), `once` (remove after first success, honoured only in skill frontmatter). Not every event supports every type: SessionStart and Setup take only `command` and `mcp_tool`; PermissionRequest takes all but `agent`; 18 events (ConfigChange, Notification, PreCompact, PreModelSwitch, WorktreeCreate, etc.) take only command/http/mcp_tool; 12 tool/turn events take all five.
- Status: GA (agent hooks: experimental)
- Evidence: https://code.claude.com/docs/en/hooks.md (Hook handler fields; Common fields; Prompt-based hooks)
- Notes: `mcp_tool` hooks are skipped at launch-time SessionStart and all Setup events because MCP servers are not yet connected; they never start OAuth.

### Command hooks: exec form, shell form, environment
- What: With `args`, `command` is spawned directly as an executable with no shell (placeholders substituted verbatim, no quoting issues); without `args`, it runs via `sh -c` (Git Bash or PowerShell on Windows; `shell` field selects). Placeholders `${CLAUDE_PROJECT_DIR}` (session start root; stays put in worktrees), `${CLAUDE_PLUGIN_ROOT}`, `${CLAUDE_PLUGIN_DATA}` are also exported as env vars. Plugin `${user_config.*}` substitution works only in exec form; shell-form references error (use `$CLAUDE_PLUGIN_OPTION_<KEY>`). Hooks run in the current cwd; if it was deleted they fall back to session start dir, project root, home, then temp. Hooks run without a controlling terminal (no `/dev/tty`). Environment is inherited minus `OTEL_*` exporter vars, minus `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` vars, minus Anthropic credentials under HIPAA config.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Command hook fields; Exec form and shell form; Reference scripts by path; Hook input and output; Common input fields)
- Notes: Before v2.1.207 shell-form plugin hooks substituted `${user_config.*}` (an injection risk the change removed). No `$CLAUDE_MODEL` env var exists. `$CLAUDE_CODE_REMOTE` is `"true"` in remote web sessions.

### Hook exit-code and stdout semantics
- What: Exit 0 = success, JSON fields apply. Exit 2 = blocking error on events that can block; stderr is the reason (JSON decision reason overrides it), and even a valid `allow` JSON cannot lift it. Any other exit or failure (cannot start, invalid JSON) = non-blocking error and the action proceeds, shown as `<hook name> hook error`. Valid JSON with a non-0/2 exit counts as success (fields decide) unless `onFailure: block`. Stdout is JSON only if it is a single object with nothing but whitespace around it; otherwise plain text. Plain stdout reaches Claude only on UserPromptSubmit, UserPromptExpansion, SessionStart, PostModelSwitch (as context) and PreCompact (summary instructions); elsewhere it goes to the debug log. Stderr on exit 0 is never shown to Claude.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Exit code output; Exit code 0; Exit code 2; Other exit codes; Exit code 2 behavior per event)
- Notes: Docs warn that exit 1 is non-blocking, so policy hooks must use exit 2. Exceptions: WorktreeCreate fails on any non-zero; WorktreeRemove fails on non-zero if the dir remains; Stop/SubagentStop/TaskCompleted/plugin UserPromptSubmit exit 2 with a "No such file" stderr is treated as non-blocking. A hook that prints an allow then leaves a background child with unredirected stdio is blocked (`stdio went quiet`).

### Exit 2 effect per event
- What: Blocks: PreToolUse (tool call), UserPromptSubmit (prompt never reaches Claude), UserPromptExpansion, Stop/SubagentStop/TeammateIdle (keeps working), TaskCreated (rolls back), TaskCompleted (stays open), ConfigChange (except policy), PostToolBatch (stops loop before next model call), PreCompact, PreModelSwitch, Elicitation/ElicitationResult (decline), WorktreeCreate/Remove (any non-zero). Does not block: PermissionRequest (ignored; use decision object), PostToolUse/PostToolUseFailure (stderr shown to Claude), PermissionDenied, Notification, Setup, InstructionsLoaded, StopFailure (ignored), SessionStart/SubagentStart/SessionEnd/CwdChanged/FileChanged/PostCompact/PostModelSwitch (stderr to user only), DirectoryAdded (debug log), MessageDisplay (original shown).
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Exit code 2 behavior per event)
- Notes: Exit 2 on PostToolUse is the documented way to make Claude see a warning after the tool already ran.

### Hook JSON output: universal fields and size caps
- What: Universal fields: `continue` (false stops Claude entirely, overrides event decisions), `stopReason`, `suppressOutput` (accepted, no effect), `systemMessage` (warning to the user; SDK gets `SDKInformationalMessage`), `terminalSequence` (escape sequence Claude Code writes for you, allowlisted to OSC 0/1/2/9/99/777 and BEL; anything else voids the field). Event control goes in top-level `decision`/`reason` or in `hookSpecificOutput` with `hookEventName`. `additionalContext`, `systemMessage`, `initialUserMessage` and plain stdout are each capped at 10,000 chars; overflow is saved to a session file and replaced by its path plus a 2,000-char preview, with no setting to raise it.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (JSON output; Emit terminal notifications)
- Notes: `terminalSequence` works only interactively, only from non-async hooks; tmux needs `allow-passthrough`. Misplaced fields (e.g. top-level `permissionDecision`) are silently ignored; debug log says `Hook JSON output had unrecognized keys`.

### additionalContext delivery
- What: Wrapped in a `<system-reminder>` and inserted where the hook fired: conversation start (SessionStart, SubagentStart), alongside the prompt (UserPromptSubmit, UserPromptExpansion), next to the tool result (Pre/PostToolUse, PostToolUseFailure, PostToolBatch), end of turn with continuation (Stop, SubagentStop), next request (PostModelSwitch). Values from several hooks are all delivered. Any `<system-reminder>` tag inside the string has `<` escaped to `&lt;`. Saved in the transcript and replayed on resume rather than re-run, so dynamic values go stale; SessionStart re-runs on resume.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Add context for Claude)
- Notes: Docs advise factual phrasing because text framed as system commands can trip Claude's prompt-injection defences.

### Hook decision control summary
- What: Top-level `decision: "block"` + `reason`: UserPromptSubmit, UserPromptExpansion, PostToolUse, PostToolUseFailure, PostToolBatch, Stop, SubagentStop, ConfigChange, PreCompact. `hookSpecificOutput.permissionDecision` allow/deny/ask/defer: PreToolUse; allow/deny/ask: PreModelSwitch. `decision.behavior` allow/deny: PermissionRequest. `retry`: PermissionDenied. Path return: WorktreeCreate. `action`/`content`: Elicitation, ElicitationResult. `displayContent`: MessageDisplay. Context only: SessionStart, SubagentStart, PostModelSwitch. None: Setup, Notification, SessionEnd, PostCompact, InstructionsLoaded, StopFailure, CwdChanged, DirectoryAdded, FileChanged. Rewrites: PreToolUse `updatedInput`, PermissionRequest `decision.updatedInput`, PostToolUse `updatedToolOutput`; UserPromptSubmit cannot replace the prompt.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Decision control)
- Notes: PreToolUse top-level `decision`/`reason` (`approve`/`block`) is deprecated.

### Hook timeouts and onFailure
- What: Default `timeout` (seconds): 600 for command/http/mcp_tool, 30 for prompt, 60 for agent; lowered to 30 on UserPromptSubmit, PreModelSwitch, PostModelSwitch and 10 on MessageDisplay; SessionEnd hooks share a 1.5 s budget (raised to the largest per-hook timeout up to 60 s, or set by `CLAUDE_CODE_SESSIONEND_HOOKS_TIMEOUT_MS`). A timed-out hook's output is discarded, so on PreToolUse it does not block (fail-open) except PreModelSwitch, which blocks on timeout. `onFailure: "block"` (command and http, v2.1.295+) makes start failure, odd exit code, HTTP error/non-2xx, timeout or invalid output behave like exit 2 (deny on PermissionRequest). No effect on Stop/SubagentStop/TaskCompleted/TeammateIdle or async hooks.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Common fields; Timeouts; Block the action when a hook fails; SessionEnd); https://code.claude.com/docs/en/hooks-guide.md (Limitations)
- Notes: Agent SDK callback hooks fail closed instead (PreToolUse and UserPromptSubmit timeouts block). On Windows without Git Bash, a shell-form PreToolUse/PermissionRequest hook with `onFailure` unset blocks on failure.

### Async and asyncRewake hooks
- What: `async: true` (command only) runs the hook in the background; it cannot block or decide, and `timeout` is not enforced. On exit its `additionalContext` and `systemMessage` are delivered to Claude on the next turn (not shown to the user); JSON may be alone on stdout (multi-line from v2.1.295) or on one line of its own; wrong-typed fields are dropped. `asyncRewake: true` also runs in the background but wakes Claude immediately on exit 2, delivering stderr (or stdout) as a system reminder; its timeout is enforced. In `-p` mode async hooks still running at teardown are killed as `cancelled`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Run hooks in the background; How async hooks execute; Limitations; Command hook fields)
- Notes: Repeated async SessionStart output identical to the last delivery is skipped (v2.1.295+). Completion notices only in verbose mode.

### Prompt-based hooks
- What: `type: "prompt"` sends the prompt (with `$ARGUMENTS` replaced by the hook input JSON, or input appended if absent) to the background model (overridable `model`) and expects `{ok, reason, impossible}`. On `ok: false`: Stop/SubagentStop continue with reason as next instruction (unless `impossible: true`, which allows the stop); PreToolUse denies and by default ends the turn (`continueOnBlock: true` returns reason as tool error instead); PostToolUse ends turn by default (`continueOnBlock` feeds back); PostToolBatch/UserPromptSubmit/UserPromptExpansion end the turn; PostToolUseFailure/TaskCreated return a tool error and continue; TeammateIdle stops by default; PermissionRequest and PermissionDenied ignore it.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Prompt-based hooks; Response schema); https://code.claude.com/docs/en/hooks-guide.md (Prompt-based hooks)
- Notes: `/goal` is a built-in shortcut for a session-scoped prompt-based Stop hook.

### Agent-based hooks
- What: `type: "agent"` spawns a subagent with the prompt and hook JSON; it can use Read, Grep, Glob and other tools for up to 50 turns, then returns `{ok, reason}`. `ok: false` is handled as a prompt hook with `continueOnBlock: true`. Default timeout 60 s; no `continueOnBlock` or `impossible`. Same events as prompt hooks except PermissionRequest.
- Status: experimental (page: "experimental and may change")
- Evidence: https://code.claude.com/docs/en/hooks.md (Agent-based hooks); https://code.claude.com/docs/en/hooks-guide.md (Agent-based hooks)
- Notes: Docs recommend command hooks for production.

### HTTP hooks
- What: POST the hook JSON with `Content-Type: application/json` to `url`; optional `headers` with `$VAR` interpolation limited to names in `allowedEnvVars` (others become empty). Response: 2xx empty = success, 2xx JSON object = parsed like command output, 2xx other body = non-blocking error (text not added to context), non-2xx or connection failure = non-blocking error unless `onFailure: block`. Admin controls: `allowedHttpHookUrls` allowlist (applies to every source including managed) and `httpHookAllowedEnvVars`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (HTTP hook fields; HTTP response handling; Configuration); https://code.claude.com/docs/en/hooks-guide.md (HTTP hooks)
- Notes: Not supported on SessionStart or Setup.

### MCP tool hooks
- What: `type: "mcp_tool"` with `server` (plugin servers as `plugin:<plugin>:<server>`), `tool` and `input` whose string values support `${path}` substitution from the hook JSON (e.g. `${tool_input.file_path}`). The tool's text output is parsed like stdout; `isError: true` is a non-blocking error. If the answer was too large to read, PreToolUse refuses the checked call (fail-closed) while other events treat it as non-blocking. On blocking-capable events it waits for a connecting server up to `MCP_TIMEOUT` within the hook timeout; observational events do not wait.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (MCP tool hook fields; How the tool's result is read; When the server is still connecting; Events that fire before MCP servers are available)
- Notes: Interesting asymmetry: unreadable MCP-hook answers fail closed on PreToolUse, while command hook timeouts fail open.

### Hooks in skills and subagent frontmatter
- What: Same schema in YAML frontmatter. Subagent hooks run only while that subagent runs, and a `Stop` hook there becomes `SubagentStop`. Skill hooks register on invocation and stay for the rest of the session (`once: true` removes after first success). Per source of skills (project, personal folder, each plugin) at most 1,024 hooks and 16,384 matcher characters per session (v2.1.304+); a skill past the limit registers none of its hooks. `strictPluginOnlyCustomization` with `hooks` drops frontmatter hooks except for plugin, bundled and managed-directory skills (and drops such skills' `allowed-tools`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Hooks in skills and agents; Limits on skill hooks); https://code.claude.com/docs/en/skills.md (When managed settings lock hooks to plugins)
- Notes: `/hooks` lists skill hooks only after the skill has been invoked.

### Hook event: SessionStart
- What: Fires on new session, resume, `/clear`, compaction, fork (matchers `startup`, `resume`, `clear`, `compact`, `fork`). Only command and mcp_tool types. Input adds `source`, optional `model`, `agent_type`, `session_title`; on resume/fork also `seconds_since_last_response`, `context_tokens`, `prompt_cache_likely_expired`, `estimated_cache_write_usd` (v2.1.251+). Output: `additionalContext` (or plain stdout), `initialUserMessage` (first user turn in `-p` mode), `sessionTitle`, `watchPaths` (for FileChanged), `reloadSkills` (re-scan skill dirs after hooks finish). Can write `CLAUDE_ENV_FILE`. Cannot block.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (SessionStart; SessionStart input; SessionStart decision control; Reload skills that a hook installs; Persist environment variables)
- Notes: At launch and on `/clear` the hooks run in the background so you can type immediately, but Claude's first response waits for them; in-session `/resume` waits. `Esc` takes the queued prompt back.

### Hook event: Setup
- What: Fires only with `--init-only`, or `-p --init` / `-p --maintenance` (matchers `init`, `maintenance`; input `trigger`). For one-time CI dependency install or cleanup. Command hooks only (mcp_tool always skipped). Cannot block; all JSON output discarded; output appears only as `hook_response` events with `--output-format stream-json --verbose`. Has `CLAUDE_ENV_FILE`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Setup)
- Notes: `--init-only` runs Setup plus SessionStart `startup` hooks then exits.

### Hook event: InstructionsLoaded
- What: Fires asynchronously whenever a CLAUDE.md or `.claude/rules/*.md` is loaded: at session start, on nested traversal, on path-glob match, via `@` include, after compaction (matcher on `load_reason`). Input: `file_path`, `memory_type` (User/Project/Local/Managed), `load_reason`, `globs`, `trigger_file_path`, `parent_file_path`. No decision control; output discarded; observability only.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (InstructionsLoaded)
- Notes: Does not fire for an AGENTS.md read directly through the Project instructions setting.

### Hook event: UserPromptSubmit
- What: Before Claude processes a prompt, including turns Claude Code starts itself (session scheduled tasks and `/loop`, background subagent reports, cross-session messages). Input `prompt` (pasted placeholders expanded, possibly wrapped in `<pasted_content>` markers; slash commands arrive as typed, not expanded), `session_title`. Output: plain stdout or `additionalContext` as context, `decision: block` + `reason` (shown to user, not to Claude), `sessionTitle`, `suppressOriginalPrompt`. Default timeout 30 s; timeout discards context and lets the prompt through unless `onFailure: block`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (UserPromptSubmit; What a blocked prompt leaves behind; Esc during a prompt check)
- Notes: A blocked prompt's text still lands in the transcript and prompt history on disk, so blocking is not a secret-scrubbing mechanism. SDK callback timeouts here block the prompt (fail-closed).

### Hook event: UserPromptExpansion
- What: When a user-typed slash command (skill/custom command) or MCP prompt expands into a prompt; covers the direct `/skill` path that PreToolUse on the Skill tool misses. Matcher on `command_name`. Input: `expansion_type` (`slash_command` or `mcp_prompt`), `command_name`, `command_args`, `command_source`, `prompt`. Output: `decision: block` + `reason`, `additionalContext`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (UserPromptExpansion)
- Notes: Example uses: gate `/deploy` on an approval file, append a review checklist.

### Hook event: MessageDisplay
- What: Runs as assistant text streams to the screen, once per batch of newly completed lines (non-interactive: once per message with the whole text). Input `turn_id`, `message_id`, `index`, `final`, `delta`. Output `displayContent` replaces only what is rendered; transcript and model context keep the original. No matcher; no decision control; 10 s default timeout; failure shows the original text.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (MessageDisplay)
- Notes: Intended for markdown stripping, SDK display transforms, or redacting keys/hostnames on screen. Verbose mode shows the original.

### Hook event: PreToolUse
- What: After Claude builds tool parameters, before execution; matches any tool name except `EndConversation` (built-ins and MCP). Input: `tool_name`, `tool_input`, `tool_use_id`, and for MCP an `mcp_server` object with `name` and `source` provenance (v2.1.274+; docs say trust `source`, not the name). File-tool paths always absolute and normalised (`~`/relative expanded before hooks). Output: `permissionDecision` allow/deny/ask/defer, `permissionDecisionReason`, `updatedInput` (replaces whole input; permission rules evaluated against the rewritten input), `additionalContext`. `ask` prompts show the hook's source label (`[settings]`, `[plugin:<name>]`, `[skill]`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (PreToolUse; PreToolUse input; PreToolUse decision control; Tools that require user interaction)
- Notes: Does not fire for `@`-mentioned files (inserted while building the prompt); block via Read deny rules or `CLAUDE_CODE_DISABLE_FILE_MENTIONS=1` (v2.1.299+). Tool input schemas documented for Bash, PowerShell, Write, Edit, Read, Glob, Grep, WebFetch, WebSearch, Agent, AskUserQuestion, ExitPlanMode. AskUserQuestion/ExitPlanMode need `allow` plus `updatedInput` carrying the answers. In auto mode a rewrite without a decision can be denied because the server classifier reviewed the original input; settings/plugin command hooks may rewrite Bash/PowerShell/Monitor/Edit/Write/NotebookEdit without that denial.

### PreToolUse "defer" for headless integrations
- What: In `-p` mode with exactly one tool call in the turn, `permissionDecision: "defer"` ends the turn with `stop_reason: "tool_deferred"` and a `deferred_tool_use` {id, name, input} in the result; the caller collects input in its own UI and runs `claude -p --resume <id>`, where the same PreToolUse fires again and can return `allow` with `updatedInput`. No timeout or retry cap; session persists until the retention sweep (30 days default). Ignored with a warning in interactive sessions or with parallel tool calls. If the tool is gone on resume, `tool_deferred_unavailable`; drop with `CLAUDE_CODE_DROP_UNAVAILABLE_DEFERRED_TOOL=1`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Defer a tool call for later; Resume when a deferred tool is unavailable)
- Notes: Resumed `-p` runs do not restore the stored permission mode; pass it again.

### Hook event: PermissionRequest
- What: When Claude Code is about to show a permission prompt (or would auto-deny in a session that cannot prompt; with no hook decision there it denies). Input like PreToolUse minus `tool_use_id`, plus `permission_suggestions`. Output `decision`: `behavior` allow/deny, `updatedInput` (re-checked against deny/ask rules), `updatedPermissions` (entries `addRules`, `replaceRules`, `removeRules`, `setMode`, `addDirectories`, `removeDirectories` with destination `session`, `localSettings`, `projectSettings`, `userSettings`), `message`, `interrupt`. Exit 2 is ignored. With a `--permission-prompt-tool` or SDK `canUseTool`, hook and host race; first decision wins.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (PermissionRequest; PermissionRequest decision control; Permission update entries); https://code.claude.com/docs/en/hooks-guide.md (Auto-approve specific permission prompts)
- Notes: `setMode: bypassPermissions` is a no-op unless bypass was made available at launch, and is never persisted as `defaultMode`. Not run for sandboxed network-request prompts. Fires immediately, unlike the ~6 s Notification `permission_prompt`.

### Hook event: PermissionDenied
- What: Fires only when auto mode denies a call (classifier verdict or no-verdict safety denial), not for manual denials, hook blocks or deny rules. Input adds `reason` (e.g. `[Data Exfiltration]`, `Classifier unavailable`). Output `hookSpecificOutput.retry: true` adds a message telling the model it may retry; the denial itself stands. Ignored for no-verdict denials. Prompt/agent hooks run but cannot set `retry`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (PermissionDenied)
- Notes: none

### Hook event: PostToolUse
- What: After a tool succeeds. Input `tool_input`, structured `tool_response`, `tool_use_id`, `duration_ms`, `mcp_server`. Output: `decision: block` + `reason` (added next to the result; original still seen), `additionalContext`, `classifierContext` (note for the auto-mode classifier, v2.1.236+, 2,000-char cap per call shared across hooks, treated as unverified unless from an in-process SDK callback), `updatedToolOutput` (replaces what Claude sees; must match the tool's output schema for built-ins, MCP output unvalidated), `updatedMCPToolOutput` (legacy). Bash results can carry `tool_response.bashEditDiff` listing files a Bash command changed in the repo (v2.1.269+).
- Status: GA (`bashEditDiff`: public beta)
- Evidence: https://code.claude.com/docs/en/hooks.md (PostToolUse; PostToolUse decision control; Annotate a result for the auto mode classifier; Bash)
- Notes: `updatedToolOutput` does not undo side effects, and OTel spans capture the original. Foreground Agent-tool results include `agentId`, `resolvedModel`, `modelsUsed`, `totalTokens` (final request only), `totalDurationMs`, `totalToolUseCount`, `usage`. After `Ctrl+Enter` interrupts, PostToolUse hooks get 10 s then are killed (v2.1.319+). Edit|Write matchers miss Bash or external rewrites; use FileChanged.

### Hook event: PostToolUseFailure
- What: When a started tool throws or an MCP tool returns an error. Not fired for unknown tools, schema validation failures (`tool_use_error`, which also skip PreToolUse) or permission denials. Input `error` (Bash starts with `Exit code N`, may be middle-truncated), `is_interrupt`, `duration_ms`. Output `additionalContext`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (PostToolUseFailure)
- Notes: Docs say to treat the error text as display text, not a stable format.

### Hook event: PostToolBatch
- What: Once after every tool call of a (parallel) batch resolves, before the next model request. No matcher. Input `tool_calls` array of {tool_name, tool_input, tool_use_id, tool_response} where `tool_response` is the serialized tool_result content the model sees (differs from PostToolUse's structured output). Output `additionalContext` injected once; `decision: block` or `continue: false` stops the agentic loop.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (PostToolBatch)
- Notes: Useful for context that depends on the set of tools run, not a single call.

### Hook event: Notification
- What: When Claude Code sends a notification; matcher on type: `permission_prompt` (~6 s unanswered, keystrokes defer it), `idle_prompt` (~60 s after finish with no typing or running background agents), `auth_success`, `auth_storage_failure`, `elicitation_dialog`, `elicitation_url_dialog`, `elicitation_complete`, `elicitation_response`, `agent_needs_input`, `agent_completed`, `quota_auto_resume_fired`/`_stale`/`_disabled`, `plugin_notification`. Input `message`, `title`, `notification_type`. Cannot block; `systemMessage`/`continue` discarded but `terminalSequence` still emitted.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Notification)
- Notes: Fires even when desktop notifications are off. In `canUseTool`-hosted sessions (Desktop, VS Code) `permission_prompt` fires at ~6 s without keystroke deferral (v2.1.233+), disable with `CLAUDE_CODE_DISABLE_PERMISSION_PROMPT_NOTIFY_HOOKS=1`.

### Hook events: SubagentStart and SubagentStop
- What: SubagentStart fires when the Agent tool spawns or resumes a subagent and when an in-process teammate handles a message; matcher on agent type (plugin agents as `plugin:name`, regex path, anchor it). Output `additionalContext` injected into the subagent's own context, de-duplicated across re-runs to keep its cache. SubagentStop fires when a subagent finishes; input `stop_hook_active`, `agent_id`, `agent_type`, `agent_transcript_path`, `last_assistant_message`, `background_tasks`, `session_crons`; decision control like Stop (block keeps the subagent running with reason as next instruction). Also fires for internal agents (prompt suggestions, `/btw`) with the session's own agent type or empty string.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (SubagentStart; SubagentStop)
- Notes: With `SubagentHandback` (auto mode, v2.1.271+), the real report is in that tool's `tool_input.message`, not `last_assistant_message`; a PreToolUse deny on it makes the subagent retry up to three times.

### Hook events: TaskCreated and TaskCompleted
- What: TaskCreated fires on `TaskCreate`; exit 2 or `decision: block` deletes the task and returns the message to Claude as a tool error (`continue: false` ignored). TaskCompleted fires when a task is marked complete via TaskUpdate or when a team teammate ends its turn with in-progress tasks; exit 2 keeps it open with stderr fed back; `continue: false` stops a teammate (ignored for TaskUpdate). Inputs: `task_id`, `task_subject`, optional `task_description`, `teammate_name`, deprecated `team_name`, `agent_id` (v2.1.290+). No matchers.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (TaskCreated; TaskCompleted)
- Notes: Example: block completion until `npm test` passes.

### Hook event: Stop
- What: When the main agent finishes responding (not on user interrupt; API errors fire StopFailure). Input `stop_hook_active`, `last_assistant_message`, `background_tasks` (id, type, status, description, command, agent_type, server, tool, name) and `session_crons` (id, schedule, recurring, prompt) so a hook can tell "done" from "paused awaiting background work". Output `decision: block` + required `reason` (continue the turn) or `additionalContext` (continue labelled as Stop hook feedback, not an error). Loop guard: after 8 consecutive continuations without an intervening tool call, Claude Code overrides the block (`CLAUDE_CODE_STOP_HOOK_BLOCK_CAP`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Stop; Stop input; Stop decision control); https://code.claude.com/docs/en/hooks-guide.md (Stop hook hits the block cap)
- Notes: Use `last_assistant_message` instead of reading the transcript, which is written asynchronously and may lag.

### Hook event: StopFailure
- What: Instead of Stop when the turn ends on an API error; matcher on `error` (`rate_limit`, `overloaded`, `authentication_failed`, `oauth_org_not_allowed`, `account_on_hold`, `billing_error`, `invalid_request`, `model_not_found`, `server_error`, `max_output_tokens`, `cloud_credential_error`, `unknown`). Input `error`, `error_details`, `last_assistant_message` (the API error text). Output and exit code ignored except `terminalSequence`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (StopFailure)
- Notes: `cloud_credential_error` needs v2.1.267+.

### Hook event: TeammateIdle
- What: When an agent-team teammate is about to go idle. No matcher. Input `teammate_name`, deprecated `team_name`, `agent_id`. Exit 2 makes it keep working with stderr as feedback; `continue: false` stops it entirely.
- Status: GA (agent teams themselves surveyed elsewhere)
- Evidence: https://code.claude.com/docs/en/hooks.md (TeammateIdle)
- Notes: none

### Hook event: ConfigChange
- What: When a settings file, managed policy file (`managed-settings.json` or `managed-settings.d/`) or skill file changes during a session; matcher `user_settings`, `project_settings`, `local_settings`, `policy_settings`, `skills`. Input `source`, `file_path`. Exit 2 or `decision: block` stops the new settings applying to the running session, silently (only a debug line). `policy_settings` changes cannot be blocked.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (ConfigChange); https://code.claude.com/docs/en/hooks-guide.md (Audit configuration changes)
- Notes: Not run for server-managed settings, macOS managed preferences, Windows registry policy, or WSL-inherited Windows policy.

### Hook events: CwdChanged, FileChanged, DirectoryAdded
- What: CwdChanged fires when a main-conversation shell command changes cwd (input `old_cwd`, `new_cwd`; no matcher; can return `watchPaths`; `CLAUDE_ENV_FILE` vars persist until the next CwdChanged clears them). FileChanged fires from a filesystem watcher whatever changed the file (tool, Bash, external process); `matcher` is split on `|` into literal filenames in cwd to watch and also filters handlers by basename; input `file_path`, `event` (`change`, `add`, `unlink`); can return `watchPaths` (replaces the dynamic list). DirectoryAdded fires after `/add-dir` or SDK `register_repo_root` (matcher `slash_command`/`register_repo_root`), runs in the background unsandboxed with 600 s timeout; for `slash_command` its `systemMessage` is delivered to Claude next turn.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (CwdChanged; DirectoryAdded; FileChanged); https://code.claude.com/docs/en/hooks-guide.md (Reload environment when directory or files change)
- Notes: Canonical use is direnv: `direnv export bash > "$CLAUDE_ENV_FILE"`. A `*` FileChanged matcher watches a file literally named `*`. Docs warn a rewriting FileChanged hook can loop forever without a tight guard.

### Hook events: WorktreeCreate and WorktreeRemove
- What: WorktreeCreate fires for `--worktree`, `EnterWorktree`, subagent `isolation: "worktree"`, workflow agents, background-session isolation and `remote-control` worktree mode, and replaces git worktree creation entirely (so SVN/Perforce/Mercurial work; `.worktreeinclude` not processed). Command hooks print the path as the last non-empty stdout line (ANSI stripped); HTTP hooks return `worktreePath`. Paths with `.`/`..` segments or passing through an in-repo symlink are refused. WorktreeRemove fires on exit-and-remove, unchanged unnamed sessions, background-session delete, `ExitWorktree`, remote-control cleanup; input `worktree_path`; non-zero exit fails removal if the directory still exists. Without a WorktreeRemove hook Claude Code falls back to `git worktree remove --force` semantics.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (WorktreeCreate; WorktreeCreate output; WorktreeRemove; WorktreeRemove input)
- Notes: Not run for `git worktree add` via Bash. Subagent/workflow hook-created worktrees are kept, not removed. Claude Code never deletes branches of hook-created worktrees. `ExitWorktree` needs `discard_changes: true` for hook worktrees, making the hook the last safety check.

### Hook events: PreCompact and PostCompact
- What: PreCompact fires before `/compact` (`manual`) or auto/idle compaction (`auto`); input `trigger`, `custom_instructions` (user's `/compact` text or null). Exit 2 or `decision: block` blocks; stdout on exit 0 becomes extra instructions for the summary (cut at 10,000 chars per hook). PostCompact fires after, with `compact_summary`; no control.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (PreCompact; PreCompact output; PostCompact)
- Notes: Blocking a proactive auto-compact continues uncompacted; blocking a recovery compaction surfaces the context-limit error.

### Hook events: PreModelSwitch and PostModelSwitch
- What: PreModelSwitch (v2.1.251+) runs before user/client-requested switches (`/model`, picker, `/config`, fast mode toggle that changes model, SDK `set_model`/`apply_flag_settings`, Remote Control), not automatic ones. Matcher compares the canonical model name (aliases, dated IDs and Bedrock IDs collapse to one; unknown IDs run every hook). Input `from_model`, `to_model`, `requested_model`, `source`, `context_tokens`, `prompt_cache_warm`, `cache_ttl`, `estimated_cache_write_usd`, `pricing`. Output `permissionDecision` allow (skips the warm-cache confirmation)/deny/ask, or `decision: block`; `ask` works only for interactive `/model`, elsewhere it is a refusal; timeout (30 s default) blocks. PostModelSwitch runs after any model change (also `auto` fallback and `resume`), cannot block; stdout or `additionalContext` is delivered with the next request (if not ready within 5 s, the following request).
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (PreModelSwitch; PreModelSwitch input; PreModelSwitch decision control; PostModelSwitch)
- Notes: Precedence deny > ask > allow. Intended uses: block retired models, show re-cache cost before switching, inject model-specific guidance.

### Hook events: Elicitation and ElicitationResult
- What: Elicitation fires when an MCP server requests user input (form or URL mode); matcher on server name; input `mcp_server_name`, `message`, `mode`, `url`, `elicitation_id`, `requested_schema`. Returning `action` accept (with `content`), decline or cancel answers without a dialog; no output leaves it to the user. ElicitationResult fires after the user responds (skipped when an Elicitation hook answered) and can let it through, replace `content` (whole object), decline (withhold values) or cancel. Exit 2 or top-level `decision: block` declines; a decline from any hook overrides others.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Elicitation; Elicitation output; Other ways to decline an elicitation; ElicitationResult)
- Notes: Top-level `decision` on these events was ignored from v2.1.105 until v2.1.284. Docs warn an ElicitationResult hook that returns accept without checking the user's action turns declines into accepts.

### Hook event: SessionEnd
- What: When a session terminates; matcher/`reason`: `clear`, `resume`, `logout`, `prompt_input_exit`, `other` (`bypass_permissions_disabled` removed in v2.1.234). No decision control; output discarded. Shares a 1.5 s budget (see timeouts).
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (SessionEnd)
- Notes: Plugin-provided timeouts do not raise the budget.

### CLAUDE_ENV_FILE environment persistence
- What: SessionStart, Setup, CwdChanged and FileChanged hooks get a `CLAUDE_ENV_FILE` path; lines written there are sourced as a shell preamble before every later Bash command. PowerShell commands receive the variables (v2.1.296+) only if every line is a plain assignment with safe quoting; otherwise none (debug: `Session environment is not all plain assignments`). Sandboxed PowerShell gets none; hooks running in PowerShell do not get the variable.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Persist environment variables; Persisted variables in PowerShell commands)
- Notes: Running arbitrary file contents as preamble to every command is a notable trust surface.

### Skills: format and standard
- What: A skill is a directory with `SKILL.md` (YAML frontmatter + markdown instructions) and optional supporting files and scripts. Follows the Agent Skills open standard (agentskills.io) with Claude Code extensions (invocation control, subagent execution, dynamic context injection). Legacy `.claude/commands/*.md` files still work and are merged into the same system (a skill wins a name clash; commands lack `name` and `paths`). Directory or `name` becomes `/name`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (intro; Create your first skill; Add supporting files)
- Notes: Docs advise SKILL.md under 500 lines with detail in referenced files. Only six fields (`name`, `description`, `license`, `compatibility`, `metadata`, `allowed-tools`) are accepted for claude.ai uploads, the Skills API and `package_skill.py`; extra keys are a hard error there.

### Skill locations and name resolution
- What: Enterprise (managed settings dir `.claude/skills/`), personal `~/.claude/skills/`, project `.claude/skills/` (cwd and every parent up to repo root; in worktrees up to the worktree root, falling back to the main checkout's skills if absent, v2.1.277+), nested `<subdir>/.claude/skills/` (loaded the first time Claude touches a file there; clashes exposed as `/apps/web:deploy`), `--add-dir` dirs (prefixed on clash, v2.1.309+), plugins (`/plugin:skill`), claude.ai account. Precedence enterprise > personal > project; a user skill replaces a bundled skill or built-in command but not its aliases. Symlinked skill folders allowed and deduplicated. Reserved names: `synced`, `anthropic-skills`. Names with zero-width, bidi, private-use, control chars or `<`/`>` are refused.
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (Choose where skills load; Load skills in monorepos and subdirectories; Load skills from a directory outside the project; Resolve skills that share a name; How a skill gets its command name; Characters a skill name can't hold)
- Notes: A skill folder with `.claude-plugin/plugin.json` loads as a plugin `<name>@skills-dir` (project ones need workspace trust). Cloud/Cowork sessions do not read `~/.claude/skills/`.

### Skill frontmatter fields
- What: `name`, `description` (recommended; falls back to first body line), `when_to_use`, `argument-hint`, `arguments` (named positional), `disable-model-invocation`, `user-invocable`, `allowed-tools`, `disallowed-tools`, `model` (rest of the turn), `effort` (rest of the turn), `context: fork`, `agent`, `background`, `hooks`, `paths` (globs relative to cwd limiting auto-activation), `shell` (bash/powershell for injected commands), `metadata`, `license`, `compatibility`. Unknown fields ignored silently; frontmatter only read if `---` is line 1; malformed YAML loads the body with no metadata. Booleans accept yes/no/on/off/1/0.
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (Frontmatter reference)
- Notes: Combined `description` + `when_to_use` truncated at 1,536 chars in the listing (`skillListingMaxDescChars`). Angle brackets in descriptions are escaped to `&lt;`/`&gt;`.

### Skill invocation control and progressive loading
- What: Default: name and description always in context, full body loads on invocation by user (`/name`) or by Claude (Skill tool). `disable-model-invocation: true` removes it from Claude's context entirely (user-only; also blocks subagent preload and scheduled-task use; if Claude tries anyway it is blocked and told not to reproduce the steps). `user-invocable: false` hides from the `/` menu (Claude-only). A `/name` after plain text grants permission for that message rather than running it. Up to six skills can be stacked at the start of a terminal message (SDK/`-p` load only the first).
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (Control who invokes a skill; Where you write the skill's name; Stack several skills in one message)
- Notes: Subagents with preloaded skills get full content injected at startup.

### Skill listing budget and visibility overrides
- What: The listing of names and descriptions has a character budget of 1% of the model's context window (`skillListingBudgetFraction`, or `SLASH_COMMAND_TOOL_CHAR_BUDGET`); on overflow, descriptions of the least-invoked skills are dropped first while all names stay. `skillOverrides` in settings sets per-skill state `on`, `name-only`, `user-invocable-only`, `off` (written by the `/skills` menu to settings.local.json); not applicable to plugin skills. `/skill-doctor` (v2.1.252+) reports per-skill context cost and usage and flags never-invoked skills.
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (Skill descriptions are cut short; Override skill visibility from settings; Find unused skills)
- Notes: Usage-ranked eviction of descriptions is an unusual detail. `/context` Skills row reports post-budget size (fixed in v2.1.196).

### Skill arguments and substitutions
- What: `$ARGUMENTS` (whole string), `$ARGUMENTS[N]`/`$N` (shell-style quoted, 0-based), `$name` from `arguments`, `${CLAUDE_SESSION_ID}`, `${CLAUDE_EFFORT}`, `${CLAUDE_SKILL_DIR}`, `${CLAUDE_PROJECT_DIR}`, and in plugins `${CLAUDE_PLUGIN_ROOT}`/`${CLAUDE_PLUGIN_DATA}`. If no placeholder receives arguments they are appended as `ARGUMENTS: <value>`. Argument values are inserted literally (not re-expanded). `${CLAUDE_SKILL_DIR}` etc. are also substituted in `allowed-tools` Bash rules so a bundled script runs without a prompt.
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (Available string substitutions; Pass arguments to skills)
- Notes: Backslash escapes argument placeholders only, not `${CLAUDE_*}`.

### Skill dynamic context injection
- What: `` !`cmd` `` (at line start or after whitespace) and ```` ```! ```` fenced blocks run shell commands before the skill content reaches Claude; output replaces the placeholder in a single non-recursive pass. Commands run through the Bash (or PowerShell) tool in the session cwd with its 2-minute timeout and output ceiling; stderr merged; any non-zero exit (except exit 1 from search/compare commands) aborts the entire invocation. Commands never prompt: each is checked against permission rules and anything other than allow aborts the invocation (in auto mode Claude is instead told to run it itself). `disableSkillShellExecution` replaces commands with a placeholder for user/project/plugin/add-dir skills (bundled and managed unaffected).
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (Inject dynamic context; How injected commands run; When an injected command fails; Permission checks on injected commands)
- Notes: Skills synced from claude.ai never run `!` commands locally and do not get `@`/placeholder expansion outside cloud sessions (v2.1.228+).

### Skills in a forked subagent
- What: `context: fork` runs the skill body as the prompt of a new subagent of type `agent` (default `general-purpose`; Explore/Plan start without CLAUDE.md and git status). No conversation history is passed. Runs in the background by default (`background: false` waits); forced foreground in `-p`/SDK, with background tasks disabled, on re-invocation while running, and from scheduled tasks. Backgrounded forks get the narrower background tool set and their edits sit outside checkpoints, so `/rewind` does not revert them.
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (Run skills in a subagent)
- Notes: Docs warn the name is misleading: this is not a fork of the conversation.

### Skill content lifecycle and tool grants
- What: Invoked skill content enters as one message and stays across turns without re-reading the file; re-invocation with identical rendered content adds only a "already loaded" note. `allowed-tools` pre-approves listed tools only for the invoking turn (clears on the next user message) and does not restrict other tools. `disallowed-tools` removes tools for that turn. `Skill` permission rules (`Skill(name)`, `Skill(name *)`, bare `Skill` deny) control which skills Claude may invoke; deny rules also cover aliases and nested variants.
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (Skill content lifecycle; Pre-approve tools for a skill; Restrict Claude's skill access)
- Notes: `allowManagedPermissionRulesOnly` disables `allowed-tools` from project/personal skills (v2.1.282+). Docs say to move must-always-hold rules from a skill into a hook.

### Skill live reload
- What: Skill directories (personal, project, `--add-dir` `.claude/skills/`) are watched; adding/editing/removing a SKILL.md applies within the session. New top-level skill dirs need `/reload-skills`. Plugin-folder changes (hooks, `.mcp.json`, agents, output styles) need `/reload-plugins`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (Edit a skill during a session)
- Notes: Not in bare mode. `.claude/commands/` and `.claude/agents/` in added dirs are not watched.

### Skills synced from claude.ai
- What: Terminal sessions signed in with a claude.ai account download account-enabled skills to `~/.claude/skills/synced/` in the background (v2.1.273+), recheck about every 10 min active and 40 min idle, and add/update/remove them live. Download-only; local edits are overwritten. Full name `/anthropic-skills:<name>`, short name only if free. Display text sanitized; `!` commands never run locally. Disabled by `syncClaudeAiSkills: false` (moves synced skills to `.trash/`), API-key auth, Bedrock, no feature-flag fetch, bare/safe mode, plugin-only lock.
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (Skills synced from claude.ai; How Claude Code handles the frontmatter of a synced skill; How Claude Code handles the body of a synced skill)
- Notes: `CLAUDE_CODE_SYNC_SKILLS=1` makes a `-p` run wait for the download.

### Bundled skills
- What: Prompt-based skills shipped with Claude Code, e.g. `/doctor` (setup checkup that diagnoses and fixes, plus `prompt-audit`), `/code-review`, `/batch`, `/debug`, `/loop`, `/claude-api` (auto-activates on `anthropic` imports; subcommands `migrate`, `upgrade`, `api-version`, `managed-agents-onboard`, `managed-agents-secure`, `prompt-audit`, `cost-optimize`, `build-eval`, `hillclimb`, `preserved-thinking-migration`), `/run`, `/verify`, `/run-skill-generator` (records a launch recipe as a committed project skill), `/workflow-authoring` (only with dynamic workflows). A project skill named `verify` or `simplify` makes the built-in commit instructions tell Claude to run it before each non-docs/tests commit (v2.1.286+). `disableBundledSkills` turns them off.
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (Bundled skills; Check your setup with /doctor; Run and verify your app; Run your checks before each commit; Work on Claude API projects)
- Notes: `/verify` writes `.claude/skills/verify/SKILL.md` itself when it had to discover the recipe, and edits it only when a run went wrong.

### Skill evaluation tooling
- What: Recommended baseline comparison of fresh sessions with and without the skill. `claude plugin eval` for plugin skills (graders, CI threshold); the `skill-creator` plugin runs evals in-session with `evals/evals.json`, isolated subagent runs, `grading.json`, `benchmark.json`, blind A/B between versions, and description tuning for trigger accuracy. `claude plugin validate <dir>` finds unparseable frontmatter (v2.1.233+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/skills.md (Evaluate and iterate on a skill; Run evals with skill-creator; Skill not triggering)
- Notes: OTel `skill_activated` event with `invocation_trigger` tracks usage (large-codebases page).

### MCP transports: stdio, HTTP (streamable), SSE, WebSocket, SDK in-process
- What: Four configurable transports. `http` (alias `streamable-http`) is the recommended remote transport; `sse` is deprecated, and on v2.1.265+ `--transport http` auto-falls back to SSE when the server refuses HTTP. `stdio` spawns a local process (everything after `--` is passed untouched). `ws` (WebSocket) is configurable only via JSON (`add-json` or `.mcp.json`), supports `url`, `headers`, `headersHelper`, `timeout`, `alwaysLoad`, but no OAuth and no `--transport ws` flag. `type: "sdk"` in-process servers can be registered only by an SDK host (Agent SDK app, desktop app); such entries in config files are skipped with an error. A JSON entry with `url` but no `type` is an error because missing `type` means stdio.
- Status: GA (SSE deprecated)
- Evidence: https://code.claude.com/docs/en/mcp.md (Option 1: Add a remote HTTP server; Option 2: Add a remote SSE server; Option 3: Add a local stdio server; Option 4: Add a remote WebSocket server)
- Notes: In `--output-format stream-json`, skipped `--mcp-config` entries are reported in the `system/init` event's `mcp_server_errors` field. Server names restricted to letters, digits, `-`, `_`.

### Stdio server environment, roots and shutdown sequence
- What: Claude Code sets `CLAUDE_PROJECT_DIR` (stable project root) in the spawned server's environment. Servers that restrict their own filesystem access should use MCP `roots/list`, which Claude Code answers with the launch dir plus every `--add-dir`/`additionalDirectories` directory, and it sends `notifications/roots/list_changed` when that set changes. Shutdown on macOS/Linux: SIGINT, then SIGTERM after 100 ms, then SIGKILL after a further 400 ms. On Windows: close stdin, then kill the process tree after 300 ms (before v2.1.296 it killed immediately).
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Option 3: Add a local stdio server; How Claude Code stops a stdio server)
- Notes: `${CLAUDE_PROJECT_DIR}` in `.mcp.json` command/args needs a default (`${CLAUDE_PROJECT_DIR:-.}`) because it is set only in the child's env; plugin MCP configs substitute it directly.

### MCP installation scopes and precedence
- What: Three user scopes: local (default; `~/.claude.json` under the project path, private), project (`.mcp.json` at repo root, committed), user (`~/.claude.json` top-level `mcpServers`, all projects). Precedence when the same server appears in several places: local > project > user > plugin > claude.ai connector; the whole entry from the winning source is used, no field merging. Scopes match by name; plugins and connectors are de-duplicated by endpoint (URL normalised for scheme/host case, default port, trailing slash). Org `managedMcpServers` outranks all (v2.1.259+). Changing scope means remove and re-add.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (https://code.claude.com/docs/en/mcp.md installation scopes; Scope hierarchy and precedence); https://code.claude.com/docs/en/mcp-quickstart.md (Find your configuration on disk; Change server scope)
- Notes: Claude Code does not read paths such as `~/.claude/mcp.json`; malformed entries are skipped individually with a parse warning in `claude mcp list` (QS Troubleshooting).

### `claude mcp` CLI and `/mcp` panel
- What: `claude mcp add|add-json|add-from-claude-desktop|list|get|remove|login|logout|reset-project-choices|serve`. `list` connects and shows health (`Connected`, `Needs authentication`, `Failed to connect` with HTTP status/error text appended), plus config-only statuses that do not connect (`Pending approval`, `Rejected`, `Disabled for this project`, `Blocked by enterprise managed policy`). `/mcp` in-session panel: tool counts, reconnect, authenticate, clear auth, disable toggle; `/mcp reconnect all` (v2.1.284+). `add-from-claude-desktop` works on macOS and WSL only and skips names with disallowed characters; duplicates get `_1` suffix.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Managing your servers; Server status; Retry failed servers yourself; Import https://code.claude.com/docs/en/mcp.md servers from Claude Desktop)
- Notes: Removing a remote server deletes stored OAuth tokens and client registration locally but does not revoke them at the AS; `claude mcp logout` revokes where supported.

### MCP client runtimes v1/v2 and protocol revision 2026-07-28
- What: Two runtimes: v1 on MCP TypeScript SDK 1.x, v2 on SDK 2.0 adding protocol revision 2026-07-28. v2 is chosen per launch: when feature flags are fetched, v2.1.232+; otherwise default from v2.1.274 (Bedrock, Vertex/Agent Platform, Foundry, gateway sign-in, telemetry off). On v2 Claude Code negotiates the new revision with HTTP, stdio and connector servers, keeps a `list_changed` notification stream open, rejects OAuth responses with unexpected issuer, and refuses to send OAuth credentials to non-HTTPS token endpoints other than loopback. Override with `MCP_SDK_GENERATION=v1|v2` and `MCP_PROTOCOL_NEGOTIATION=auto|legacy` (stdio server negotiation can be skipped for 7 days after a failure).
- Status: GA (gated by flag rollout)
- Evidence: https://code.claude.com/docs/en/mcp.md (https://code.claude.com/docs/en/mcp.md client runtimes; Notification streams on the v2 runtime)
- Notes: Channel servers that negotiate 2026-07-28 are not registered as channels because that revision cannot carry channel messages. Notification stream reopen limits: 3 reopens if it closes within 10 s; after 5 reopens in an hour of longer-lived streams, wait about 6 hours. Anthropic can pin individual servers to the old protocol via a fetched flag.

### Dynamic tool updates (`list_changed`)
- What: On `list_changed`, interactive sessions refetch tools, prompts and resources from that server; `-p` and Agent SDK refresh only tools. A failed refresh keeps the previous lists. With tool search on, tools from a server that finishes connecting mid-turn are announced to Claude on its next request in the same turn.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Dynamic tool updates; Tools from a server that is still connecting)

### MCP connection lifecycle: background connect, reconnect, retries, discovery cache
- What: Servers connect in the background by default (`MCP_CONNECTION_NONBLOCKING=0` makes startup wait). If a request needs a still-connecting server, the wait happens inside `ToolSearch`, or via a `WaitForMcpServers` tool when tool search is off. After resume, a call to a still-connecting server is held up to 10 s on its first connection attempt. Remote drops: up to 5 reconnects with exponential backoff from 1 s; in `-p`/SDK after transient errors HTTP/SSE retry in background for about 30 minutes. First-connect transient failures of HTTP/SSE: up to 3 retries (not stdio, not ws, not auth/404 errors unless a `headersHelper` supplies Authorization). Discovery requests (`tools/list` etc.) retried 3 times on transient errors. Stdio in `-p`/SDK is restarted on next tool call if it exits mid-session; plugin stdio servers that failed are skipped for 15 minutes in new `-p` runs. Optional discovery cache (v2.1.221+, `MCP_DISCOVERY_CACHE`, off by default since v2.1.238) loads a remote server's tool list from a previous session and connects lazily on first call (`cached 2h ago · connects on first use`).
- Status: GA (discovery cache in gradual rollout, off by default)
- Evidence: https://code.claude.com/docs/en/mcp.md (Automatic reconnection; Failed first connections; Stdio servers that exit or fail to start; Failed discovery requests; Server status detail; Tools from a server that is still connecting; Exempt a server from deferral)
- Notes: With tool search on, Claude is told which servers failed and why (also included in empty ToolSearch results); without tool search, failures are not reported to the model. Startup timeout `MCP_TIMEOUT` defaults to 30 s per QS; see Contradictions for the "5-second connect timeout".

### MCP tool-call timeouts and idle timeout
- What: Per-server `timeout` (ms) is a hard wall-clock limit per call that progress notifications do not extend; values under 1000 are ignored. Global `MCP_TOOL_TIMEOUT` defaults to about 28 hours. HTTP/SSE/connector servers also have a per-request time-to-first-byte timer = max(60 s, tool timeout, `MCP_TIMEOUT`). Idle timeout (no response and no progress) aborts: default 5 min for remote servers, 30 min for stdio, not applied to IDE or SDK servers; `CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT` (0 disables); per-server `timeout` acts as a floor on idle.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Time limits on https://code.claude.com/docs/en/mcp.md tool calls)

### Plugin-provided MCP servers and tool naming
- What: Plugins declare servers in `.mcp.json` at plugin root or inline `mcpServers` in `plugin.json`; they start when the plugin is enabled and connect/disconnect on reload or `/cd` (v2.1.246+). Placeholders `${CLAUDE_PLUGIN_ROOT}`, `${CLAUDE_PLUGIN_DATA}`, `${CLAUDE_PROJECT_DIR}` substituted in stdio `command/args/env` and remote `url/headers/headersHelper`. Tool names are `mcp__plugin_<plugin>_<server>__<tool>` (non `[A-Za-z0-9_-]` replaced by `_`); server registers as `plugin:<plugin>:<server>`. Hook matchers on the bare server key never fire for plugin servers. Reload keeps live connections whose config is unchanged. In cloud sessions an unconnected plugin server is started on demand.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Plugin-provided https://code.claude.com/docs/en/mcp.md servers)
- Notes: In a session without an interactive terminal, `/reload-plugins` does not connect/disconnect plugin MCP servers.

### Per-server and per-tool load control (`alwaysLoad`)
- What: Server config `alwaysLoad: true` loads all its tools upfront regardless of tool search and makes startup wait for that server (capped at a 5-second connect timeout; a valid discovery-cache entry avoids the wait). `alwaysLoad: false` forces all tools deferred, including author-marked ones (v2.1.287+). Server authors can set `_meta["anthropic/alwaysLoad"]` per tool: `true` loads upfront without blocking startup; `false` keeps the tool deferred even when the user sets server-level `alwaysLoad: true`, but only for `--mcp-config`, SDK and plugin servers (v2.1.285+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Exempt a server from deferral; Defer a server's tools; Mark a tool to load upfront or stay deferred)

### Tool description and server-instruction length caps
- What: Server instructions and upfront-loaded tool descriptions are cut at 4,096 chars; descriptions of tools loaded via tool search at 16,384. Tool search matches only within the shorter limit, so keywords belong near the start. `CLAUDE_CODE_MAX_MCP_DESCRIPTION_LENGTH` sets both limits to one value (v2.1.280+). Some Anthropic-hosted servers at `api.anthropic.com` are exempt.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Tool search for https://code.claude.com/docs/en/mcp.md server authors; Change the description length limits)
- Notes: Before v2.1.296 the shorter cap was 2,048; before v2.1.295 it applied to all descriptions.

### Tool schema normalisation and exclusion
- What: Root-level `anyOf`/`oneOf`/`allOf` in a tool input schema (not accepted by the Claude API) is flattened into one object; `allOf` keeps each branch's required list, `anyOf`/`oneOf` move branch requirements into a sentence prepended to the description. Tools are excluded (others from the server stay) when top-level property names break the 1-64 char `[A-Za-z0-9_.-]` rule or the schema fails JSON Schema draft 2020-12 meta-validation; Claude is told which tools were excluded and why. Tools whose full name exceeds 128 characters are dropped. If the API refuses a schema at request time, Claude Code resends with the tool declared parameterless (v2.1.300+; before, every turn 400'd).
- Status: GA (some checks depend on fetched remote config; skipped air-gapped)
- Evidence: https://code.claude.com/docs/en/mcp.md (Tool input schemas with a root-level combinator; Tools with invalid input schemas; Tools whose input schema the API refuses; Tools with names longer than 128 characters)
- Notes: Checks introduced v2.1.216. In cloud sessions `/mcp` can show `3 of 13 tools available` due to deny rules, long names, or `_meta.ui.visibility` lacking `model`.

### MCP elicitation (form and URL modes)
- What: Servers can request structured input mid-task. Form mode shows a server-defined form; URL mode asks to open a link (sign-in etc.), passing it to the OS URL handler with a length cap (about 8,000 chars plain, about 4,000 if heavily percent-escaped; over the cap the user can only decline). Declared capability `elicitation: {form: {}, url: {}}` on 2026-07-28 connections and on older-revision connections to user-configured servers (v2.1.287+); bare `elicitation: {}` (form only) elsewhere, e.g. claude.ai connectors. `Elicitation` hook can auto-respond. In cloud sessions requests appear as cards on claude.ai/code or the Desktop Code tab (Submit / Open link + "I'm done, continue" / Decline).
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Respond to https://code.claude.com/docs/en/mcp.md elicitation requests; Answer an elicitation request in a cloud session; Elicitation modes Claude Code declares)
- Notes: Java MCP SDK 0.17.0 and earlier can reject the form+url declaration.

### MCP resources via @-mentions
- What: Typing `@` lists resources from connected servers alongside files (fuzzy-searchable); `@server:protocol://path` attaches the resource. Claude Code auto-provides list/read resource tools when servers support resources. MCP Apps UI resources (`ui://` or `text/html;profile=mcp-app`) are excluded from suggestions and listing but readable by URI.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Use https://code.claude.com/docs/en/mcp.md resources)

### MCP prompts as slash commands
- What: Server prompts appear as `/servername:promptname (MCP)` and also run as `/mcp__server__prompt`; arguments are whitespace-split, one token each; prompt results are injected into the conversation. Prompts from a server named `anthropic-skills` are hidden (reserved for synced claude.ai skills).
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Use https://code.claude.com/docs/en/mcp.md prompts as commands)

### Channels: MCP servers pushing messages into a session
- What: A server declaring the `claude/channel` capability and opted in with `--channels` can push messages (CI results, alerts, chat) into the session.
- Status: unclear (details on channels pages, outside this slice)
- Evidence: https://code.claude.com/docs/en/mcp.md (Push messages with channels)
- Notes: Not registered as a channel if it negotiates revision 2026-07-28; `MCP_PROTOCOL_NEGOTIATION=legacy` keeps stdio channel servers on the old handshake.

### claude.ai connectors in Claude Code
- What: With a claude.ai subscription login, connectors added at claude.ai/customize/connectors (admin-only on Team/Enterprise) appear automatically in `/mcp` labelled as claude.ai; Anthropic also supplies some itself (e.g. `claude.ai Claude Docs`). Not loaded when an API key, `apiKeyHelper`, third-party provider, Anthropic profile/federation, or `setup-token` OAuth token is active. Never-used connectors collapse behind `Show unused connectors`. Fetch retried 3 times on network failure. A local server at the same URL hides the connector. `session token rejected` state requires `/login`, not re-authorising. How connectors arrive differs: terminal/IDE/SDK fetch them; cloud sessions receive them from the host via a URL-rewriting proxy; desktop local/SSH sessions get them in-process as `sdk` servers (no MCP setting reaches them).
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Use https://code.claude.com/docs/en/mcp.md servers from claude.ai; How connectors reach Claude Code)
- Notes: Off switches: `disableClaudeAiConnectors` (any-source-true; a project `false` cannot re-enable), `ENABLE_CLAUDEAI_MCP_SERVERS=false`, per-project `/mcp` toggle, `deniedMcpServers` by display name such as `claude.ai Slack`.

### Claude Code as an MCP server (`claude mcp serve`)
- What: `claude mcp serve` runs Claude Code as a stdio MCP server exposing its own tools (Read, Edit, Bash etc.) to another MCP client such as Claude Desktop; it prints nothing on start. The client is responsible for per-call user confirmation. Bash/PowerShell results for backgrounded commands include `backgroundOutputPath` (v2.1.295+); the docs suggest appending `; echo DONE` to detect completion and reading the tail with `Read` offset/limit, since nothing marks the end or notifies the client.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Use Claude Code as an https://code.claude.com/docs/en/mcp.md server; Read a background command's output as an https://code.claude.com/docs/en/mcp.md client)
- Notes: Exposes tools only; no mention of exposing the agent loop itself as a tool.

### Plugin model: unit of packaging, always-on footprint
- What: A plugin is a directory of components (skills, commands, agents, hooks, a JS "hooks module" making it a mod, MCP servers, LSP servers, monitors, output styles, themes, workflows, `bin/` executables, default settings, channels) with an optional manifest at `.claude-plugin/plugin.json`, installed and loaded as one unit, usually from a marketplace. An enabled plugin is part of every session: names and descriptions of model-invocable skills/agents/commands are in context every turn, MCP servers run, hooks fire; full skill/agent text loads only when used. Official-marketplace listings show a "Context cost" estimate; an Installed-tab group "Not used recently" suggests plugins to disable.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/overview.md (Understand what a plugin is; What an enabled plugin adds to your sessions)
- Notes: Same plugin format also installs on claude.ai and Cowork with a different component set. Cloud sessions do not load local-settings plugins.

### Plugin components and their default locations
- What: `skills/<name>/SKILL.md` (command `/<plugin>:<dir>`; a lone root `SKILL.md` loads as one skill); `commands/*.md` (legacy, subdirs add `:` segments); `agents/*.md` (recursive, subfolders join with `:`; supported frontmatter `name, description, model, effort, maxTurns, autoCompactWindow, tools, disallowedTools, skills, memory, background, omitClaudeMd, isolation` (only `worktree`), `color`, `experimental.cacheTtl`; ignored: `permissionMode`, `hooks`, `mcpServers`, `initialPrompt`); `hooks/hooks.json` (same shape as settings hooks; fires from load regardless of skill use); `.mcp.json`; `.lsp.json`; `monitors/monitors.json`; `output-styles/`; `themes/` (read-only, edits copied to user dir); `workflows/*.js` (subagent orchestration scripts); `bin/` (appended to the Bash tool's PATH after the user's entries, so it cannot shadow system commands); `settings.json` (only `agent` and `subagentStatusLine` honoured; `agent` runs a plugin agent as the main thread). A `CLAUDE.md` at plugin root is not loaded (validator warns).
- Status: GA (themes, monitors, evals under the `experimental` manifest key)
- Evidence: https://code.claude.com/docs/en/plugins/components.md (Explore the plugin directory; Skills; Commands; Agents; Hooks; Executables; Default settings; Themes and output styles); https://code.claude.com/docs/en/plugins/manifest-reference.md (Standard layout)
- Notes: Two enabled plugins with the same manifest name: only one registers `hooks/hooks.json` hooks and one loads a hooks module; `/plugin` Errors tab names which (v2.1.296+).

### Plugin manifest (`plugin.json`) and validation
- What: Optional; only `name` required (kebab-case, no spaces/`@`/`:`/separators/control/bidi chars). Fields include `displayName`, `version` (free string; pins users to that version), `description`, `author`, `homepage` (must parse), `license`, `keywords`, `metadata` (ignored), directory-listing fields (`icon`, `documentationUrl`, ... ignored by Claude Code), `defaultEnabled` (default true), `dependencies`, `settings`, `userConfig`, `types` (mod `.d.ts`), `channels`, component keys (`skills` adds to default; `commands`/`agents`/`outputStyles`/`workflows`/themes/monitors replace default; `hooks`/`mcpServers`/`lspServers` merge, later names win), `experimental.{themes,monitors,evals}`. Unknown top-level keys are stripped with a warning; unknown keys inside `userConfig`, `channels`, `lspServers`, `monitors` entries are errors (plugin does not load). Paths must start with `./`, stay inside the plugin root (`..`, escaping symlinks, and on macOS/Linux any backslash are rejected) and exist. `claude plugin validate [--strict]` is the authoritative check, including MCP entry checks (v2.1.281+): errors for entries that would be dropped or undeclared `${user_config.KEY}`; warnings for `http://`/`ws://` to non-loopback and literal-looking credentials in headers.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/manifest-reference.md (Manifest file; Unrecognized fields; Validate the manifest; Fields; Path rules; Containment and existence; How each key combines with its default location)
- Notes: Reserved-name check: names starting `claude-`, `anthropic-`, `anthropics-`, `cc-plugin-`, or exact `claude`, `claude-code` etc., or `official` beside claude/anthropic are errors; whole-word `claude` elsewhere is a warning. `init`/`tag` refuse them, but Claude Code still installs and loads such plugins.

### Plugin path variables and persistent data dir
- What: `${CLAUDE_PLUGIN_ROOT}` (per-version install dir, changes on update; do not write state), `${CLAUDE_PLUGIN_DATA}` (`~/.claude/plugins/data/<id>/`, survives updates, created on first reference, deleted on uninstall from last scope unless `--keep-data`), `${CLAUDE_PROJECT_DIR}`. Substituted in skill/command/agent bodies, hook and monitor commands, MCP and LSP configs; exported to hook, MCP and LSP processes, but not to Bash tool commands. Docs show a `SessionStart` hook that diffs `package.json` and runs `npm install` into the data dir.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/manifest-reference.md (Environment variables; Where each variable resolves); https://code.claude.com/docs/en/plugins/components.md (Reference plugin paths and store data)

### MCPB bundles inside plugins
- What: `mcpServers` may point at a `.mcpb` (or legacy `.dxt`) bundle path or `https://` URL; it is extracted/downloaded into `.mcpb-cache/` under the plugin root; the bundle's own `user_config` must be satisfied or the server does not start (`/plugin` Configure, or `claude plugin install --config <server>.<key>=<value>`, v2.1.285+). Skipped for repository plugins.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/components.md (Include a packaged MCPB server); https://code.claude.com/docs/en/plugins/manifest-reference.md (mcpServers); https://code.claude.com/docs/en/plugins/loading.md (Trust check and component limits for repository plugins)

### Marketplace file (`marketplace.json`)
- What: `.claude-plugin/marketplace.json` with required `name`, `owner`, `plugins`; optional `description`, `version`, `metadata.pluginRoot` (bare-name sources, v2.1.239+), `forceRemoveDeletedPlugins` (uninstall removed plugins on users' machines), `allowCrossMarketplaceDependenciesOn`, `renames` (old name to new or `null`). Each entry is validated independently (one bad entry does not fail the catalog); unknown keys ignored with validator warnings. Entries take every `plugin.json` field plus `source`, `category`, `tags`, `strict` (default true), `relevance`, `headers`/`headersHelper` for archive downloads (v2.1.238+). Strict mode: with a `plugin.json` present and `strict: true`, entry component fields append (entry hook matchers replace per event); with `strict: false` any entry component field is a hard conflict; without `plugin.json` the entry is the manifest. Entry display fields and `defaultEnabled` override the manifest; manifest `version` overrides the entry.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/marketplace-reference.md (Top-level fields; Plugin entries; How an entry combines with plugin.json; Strict mode)
- Notes: One marketplace per name per user. Large reserved-name list (official names valid only from `github.com/anthropics/`; impersonation and non-ASCII names refused; `npm`, `pip`, `cargo`, `github` etc.; JS prototype member names such as `constructor`; `claudeai-` prefix; another registered GitHub marketplace's `<owner>-<repo>` download-folder name).

### Plugin sources (where a plugin is fetched from)
- What: Relative path (inside marketplace root, `./` required, no `..`, no backslash on Unix); `github` (`repo`, `ref`, `sha`); `url` (any git URL); `git-subdir` (sparse partial clone of one subdirectory of a monorepo); `npm` (`package`, `version` range or dist-tag, `registry`; fetched with the user's npm client with install scripts never run; refuses git/file/alias specs, tarball links on github.com/gitlab.com/bitbucket etc., and `http` except the user's own default registry); `archive` (HTTPS zip only, no loopback/link-local/cloud-metadata host, optional `sha256` digest enforced); `command` (v2.1.229+: a shell command printing the plugin dir, shown to the user for review before running, max 500 printable ASCII chars, timeout 1-600 s default 60, `copy` mode (hash-versioned, limit 256 MiB / 20,000 entries) or `link` mode (in place, not on Windows), re-run at install/update and once per session in background). `sha` must be a full 40-char lowercase commit; with both `ref` and `sha`, `sha` is checked out.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/marketplace-reference.md (Plugin sources; npm plugin source; archive plugin source; command plugin source; Copy mode and link mode)
- Notes: Command output rejected if it is the session's own dir or a parent, a UNC path, or lacks plugin content. Admin switch `disableCommandPluginSources` (defaults to the value of `allowManagedHooksOnly`). Marketplace clone fetches only submodules that hold plugin files (v2.1.295+).

### Marketplace sources and registration
- What: How the catalog itself is fetched: `url` (direct link to `marketplace.json`, with `headers`/`headersHelper`; relative plugin paths cannot resolve), `github` (`repo`, `ref`, `path`, `sparsePaths`), `git` (`url`, `ref`, `path`, `sparsePaths`), `file`, `directory`, `settings` (inline catalog in settings, object sources only); `npm` marketplace source exists in the schema but fails with "not yet implemented". Policy-only values: `hostPattern`, `pathPattern` (regexes tested before fetch), `skills-dir`, `github` `owner/*`. Registered via `claude plugin marketplace add` (also written to user `extraKnownMarketplaces`) or settings `extraKnownMarketplaces` (alias `additionalMarketplaces`, v2.1.232+).
- Status: GA (npm marketplace source not implemented)
- Evidence: https://code.claude.com/docs/en/plugins/marketplace-reference.md (Marketplace sources; Fields by type; Source values valid only in policy lists; Source objects in settings)

### Plugin name-conflict resolution
- What: Same manifest name across origins resolves: managed-settings id > `--plugin-dir`/`--plugin-url` (silently replaces a marketplace plugin, logged only in `--debug`) > installed marketplace plugin > skills-dir plugin (user dir beats project dir) > claude.ai synced. Set `"<name>@inline": false` to stop a session-only plugin shadowing.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/loading.md (Name conflicts; Keep a session-only plugin from loading)

### Plugin versioning, updates and cache keys
- What: Computed version (also the cache dir name) for non-command sources: manifest `version` first, then entry `version`, else source-derived: 12-char commit SHA for git sources (git-subdir adds a path hash), 12-char sha256 for archives, commit SHA of the dir for relative paths in git marketplaces, `unknown` for local non-git dirs and npm. Pinning a manifest version freezes all users until it changes. Command sources: content hash or `<version>-<hash>`. Auto-update: in interactive sessions, after the first message, a random delay up to 10 minutes, then refresh auto-update marketplaces and update plugins on disk; running session keeps old versions until `/reload-plugins` (monitors need restart). Defaults: on for official marketplaces and claude.ai-hosted ones, off for `knowledge-work-plugins`, `first-party-plugins` and all third-party. Per-marketplace `autoUpdate` in settings or `/plugin` toggle; `DISABLE_AUTOUPDATER`/`DISABLE_UPDATES`/nonessential-traffic off disable it unless `FORCE_AUTOUPDATE_PLUGINS=1`. `name@marketplace` installs refresh that marketplace first (skipped within 30 s, for local/settings/seed marketplaces, or nonessential traffic off).
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/loading.md (Versions and updates; How Claude Code computes the version; When Claude Code refreshes a marketplace before an install; When auto-update runs; When a command source re-runs)
- Notes: Auto-update skips entries with a `headersHelper`. Command-source plugins re-run per session and hot-reload when output changes, unless that would invalidate the prompt cache.

### Plugin-to-plugin dependencies with semver constraints
- What: `dependencies` entries: `"name"`, `"name@marketplace"`, or `{name, version, marketplace}` with npm-semver ranges (`~2.1.0`, `^2.0`, `>=1.4`, `=2.1.0`; pre-releases only with `-0` suffix). Constraints resolve to the highest git tag named `<plugin>--v<version>` on the repo hosting the dependency; `claude plugin tag [--push] [--dry-run]` creates tags after validating, checking manifest/entry version agreement, a clean tree, and tag non-existence. Multiple dependents' ranges intersect; non-overlap fails the install (`has conflicting version requirements`); auto-update stays within the intersection. Cross-marketplace dependencies are refused unless the root marketplace lists the target in `allowCrossMarketplaceDependenciesOn` (or the dependency is already enabled at that scope). Non-git sources (npm, archive, command) check the constraint only at load. Command-source and `headersHelper` dependencies are never auto-installed. Bundles: a plugin with only `name` + `dependencies` installs a curated set. `claude plugin prune` removes orphaned auto-installed dependencies.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/dependencies.md (Declare a dependency with a version constraint; Bundle plugins for a team; Depend on a plugin from another marketplace; Create a release tag; How a constraint resolves against tags; Combine constraints from several plugins)
- Notes: Resolved versions show as `2.1.0-8713c5b11005`. Relative-path dependency with no matching tag falls back to the current copy and the dependent is disabled if out of range.

### Plugin install UX and scopes (`/plugin` panel)
- What: `/plugin` (aliases `/plugins`, `/marketplace`) opens a TUI with Discover, Installed, Marketplaces, Errors and (where available) Stats tabs. `/plugin install <name>@<mkt>` opens the details pane rather than installing: description, "Will install" component list, last-updated, and for official plugins a Context cost estimate (Every turn / When invoked). User picks a scope: user (`~/.claude/settings.json`), project (`.claude/settings.json`; collaborators must still install per clone), local (`.claude/settings.local.json`); local > project > user. Install summary states whether it is active now, needs a bundled-server config, needs a reload, or failed. Installed tab: filter, Space toggles, `f` favourites, Configure options, Update now, Uninstall; "Not used recently" group (unused 14 days and 10 sessions). Uninstalling a project-enabled plugin asks "disable for me" (writes local false) vs "uninstall for everyone". `/plugin install <p> --marketplace <source>` adds and installs (v2.1.275+, asks to confirm the resolved source). In `-p` `/plugin` is unavailable.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/install.md (Install a plugin; Choose an install scope; Manage installed plugins; Uninstall a plugin the project enables; Find plugins you no longer use; Plugins with dependencies); https://code.claude.com/docs/en/plugins/cli-reference.md (/plugin in a session); https://code.claude.com/docs/en/plugins/measure.md (Not used recently in /plugin)
- Notes: Terminal, desktop local sessions and the VS Code extension share settings files, so a user-scope install appears in all three. Cloud sessions load no local plugins.

### `claude plugin` shell CLI
- What: Subcommands `init` (scaffold under `~/.claude/skills/<name>/`, `--with skills|agents|hooks|mcp|lsp|output-style|channel`), `install` (`--scope`, `--config k=v`, `-y`, `--accept-command <sha256>`, `--json`, `--marketplace <source>` v2.1.292+), `uninstall` (`--keep-data`, `--prune`), `enable`, `disable`, `update` (also `managed` scope), `list [--json]`, `details` (component inventory + token cost), `configure` (`--values-stdin`), `prune` (orphaned dependencies, `--dry-run`), `eval`, `eval init`, `tag`, `test` (mod tests, `*.test.ts(x)`, no network), `validate [--strict] [--json]`, and `marketplace add|list|remove|update`. Common exit codes 0/1 (`validate` adds 2). `--json` prints one result object on the last stdout line with `command`, `outcome`, `message`, optional `failureCode`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/cli-reference.md (claude plugin commands; plugin install; JSON result format; plugin uninstall; plugin prune; plugin tag; plugin test; plugin validate)
- Notes: Command-source or `headersHelper` installs ask `Run this command now? [y/N]`; non-TTY without `-y`/`--accept-command` refuses; `-y` is ignored when Claude itself runs the command via Bash or a hook, so the model cannot self-approve. `--accept-command` binds approval to the exact command, plugin and catalog (sha256).

### `/reload-plugins` and prompt-cache-aware hot reload
- What: Applies pending plugin installs/updates/enables/disables/disk edits without restart and prints `Reloaded: N plugins · N skills · N agents · N hooks · N plugin MCP servers · N plugin LSP servers`; installs missing dependencies and reloads again. If the reload would add/remove a plugin MCP server or the `LSP` tool and thereby invalidate the prompt cache, it refuses and asks for `--force` ("your next message will re-read the whole conversation"). Closing `/plugin` with pending changes runs it automatically. Runs in non-interactive sessions (v2.1.260+) only when typed directly, not via Remote Control or Slack-relayed messages, and there it does not connect/disconnect MCP servers. Plugin folder passed via `--plugin-dir` is watched: subfolders added/removed mid-session load/unload (held if it would break the cache).
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/cli-reference.md (/reload-plugins; Reloads that change https://code.claude.com/docs/en/mcp.md tools; Sessions without an interactive terminal); https://code.claude.com/docs/en/plugins/create.md (Load a folder of plugins)

### Session-only and skills-directory plugin loading
- What: `--plugin-dir <dir|.zip>` (repeatable; a folder of plugins loads each child with a manifest, v2.1.265+), `--plugin-url <zip url>`, `CLAUDE_CODE_PLUGIN_DIRS` env (v2.1.280+, cannot be set from project/local settings), Agent SDK `plugins` option: all load as `<name>@inline` for that session only and override same-named installed plugins. Folders with `.claude-plugin/plugin.json` under `~/.claude/skills/` or the project's `.claude/skills/` load every session as `<name>@skills-dir` with no install (project ones only after folder trust and only from the primary working directory, not parents). All blocked by `disableSideloadFlags` / `strictPluginOnlyCustomization` / allowlists missing `skills-dir` respectively.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/create.md (Develop without a marketplace; Make a plugin load in every session); https://code.claude.com/docs/en/plugins/cli-reference.md (Flags that load a plugin for one session); https://code.claude.com/docs/en/plugins/loading.md (Check why a repository plugin didn't load)
- Notes: A skills-dir plugin's root `SKILL.md` is invoked as `/<name>`, nested skills as `/<name>:<skill>`. Hooks migrated from settings into a plugin run twice until removed from settings (no prefix to dedupe).

### Marketplace maintenance: renames, removals, release channels
- What: `renames` map (old to new or `null`) migrates users: settings keys in `enabledPlugins` and `pluginConfigs` are rewritten in user/project/local scopes (not managed), with a one-time notice; chains followed from the oldest name; validator rejects cycles. `forceRemoveDeletedPlugins: true` uninstalls delisted plugins at each session start (except managed-installed) and lists them under a Flagged heading. No deprecation state, audience field, or auto-update field in `marketplace.json`; release channels are two marketplaces with different names pointing at different refs. Changing a `command` source's command requires each user to re-accept it; until then background runs stop and an Errors-tab entry shows the update command.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/host-marketplace.md (Rename or remove a plugin; Migrate users with a renames map; Uninstall removed plugins from users' machines; Run release channels; Change the command of a command source; Work around what a marketplace can't do)

### Anthropic's marketplaces
- What: Official `claude-plugins-official` (repo `anthropics/claude-plugins-official`; auto-registered on first interactive terminal session unless policy or `CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL`; mostly partner and third-party plugins plus Anthropic ones such as `commit-commands`, `code-review`, `feature-dev`, LSP plugins). Community `claude-community` (repo `anthropics/claude-plugins-community`, added manually, third-party submissions, SHA-pinned). Topic marketplaces such as `anthropics/skills`, `anthropics/knowledge-work-plugins`. Third-party marketplaces are not reviewed by Anthropic.
- Status: GA; the `claude-code-plugins` marketplace in `anthropics/claude-code` is deprecated
- Evidence: https://code.claude.com/docs/en/plugins/anthropic-marketplaces.md (Anthropic's marketplaces; Deprecated claude-code-plugins marketplace; Find plugins in the official marketplace; Third-party marketplaces)

### Plugin relevance suggestions (`relevance` signals)
- What: Marketplace entries can declare `relevance: {topic, signals}` with `cwd` globs (only signal that matches at session start), `cli` (first token of shell commands, after env assignments and `sudo`), `hosts` (hostnames in URLs in Bash commands), `filesRead` globs (also files written/edited and auto-loaded CLAUDE.md), `manifestDeps` (regex on manifest path + contents, manifests over 512 KB skipped); each with count/length limits. Matching is local with no network or reporting. Suggestions appear as a spinner tip, a session-start notification (cwd), and a pinned Discover entry; at most once every three sessions, notification max twice, never after install; never auto-installs. Shown only for marketplaces an admin lists in `pluginSuggestionMarketplaces` with a declared source, so an impostor source under an allowlisted name cannot get suggestions.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/relevance.md (Understand how plugin relevance works; Field reference; Enable suggestions in managed settings; Preview what the user sees)
- Notes: Over-limit recognised fields invalidate the whole entry. Spinner tips toggles (`spinnerTipsEnabled`) suppress tips and notification but not the Discover pin.

### CLI hints (`<claude-code-hint />` from third-party CLIs)
- What: A CLI that detects `CLAUDECODE` (set in Bash/PowerShell tool and hook subprocesses; also IDE terminals) or `CLAUDE_CODE_CHILD_SESSION` (v2.1.172+, only Claude-started subprocesses) can print `<claude-code-hint v="1" type="plugin" value="name@marketplace" />` on its own stderr line. Claude Code strips the line from tool output before the model sees it and may show a one-time "Plugin recommendation" dialog naming the first word of the command. Only for plugins in official-name marketplaces, interactive local attended sessions, analytics on; limits: one prompt per session, once ever per plugin, none after 100 plugins prompted; not in `-p`, subagents, hook output, cloud/Remote Control/agent-team sessions.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/cli-hints.md (Emit the hint; Hint format; Check when the prompt appears; Preview what the user sees)
- Notes: Stripping happens even for unrecognised `v`/`type`, so the tag cannot be used to inject text into the model through this channel.

### Mods: in-process JS/TS extension model
- What: A mod is a plugin whose `hooks/hooks.json` has a `modules` key pointing to one ES-module hooks file (`.js/.mjs/.cjs/.jsx/.ts/.mts/.cts/.tsx`, no build step). It exports `register(on, options)`; each `on(event, [matcher], handler)` registers a middleware hook `($, e, next)`. Hooks observe (`return next(e)`), rewrite (`next({...e, ...})`; events are deeply frozen), or answer (return without `next`). All hooks in a module share module-level variables. The hooks module has no Node APIs, timers, network or file access of its own; all side effects go through the `$` mods API. Standard web APIs (URL, TextEncoder, AbortController, crypto.subtle) are available.
- Status: GA (on by default from v2.1.287 terminal, v2.1.286 Desktop)
- Evidence: https://code.claude.com/docs/en/plugins/mods/overview.md (How a mod works; What a hook can do with an event); https://code.claude.com/docs/en/plugins/mods/create.md (Write a mod yourself); https://code.claude.com/docs/en/plugins/mods/reference.md (Files; The hook function)
- Notes: Runs in CLI, Desktop Code tab, VS Code extension; hooks run in `-p`, Agent SDK, Remote Control and cloud sessions (if the plugin reaches them) but drawing appears only in terminal/Desktop/VS Code. Not in Desktop WSL sessions. Installed mods share one "hooks worker" thread; a mod that blocks or crashes it is unloaded, and three untraceable crashes unload all non-built-in mods for the session (MTS). Matchers: field values, arrays, regexes; wildcard `'*'` and `'classic.*'`. Registering the same event twice without a matcher fails load. `next.signal`, `next.origin {plugin,tier}`, `next.budget`, `next.to(e, tier)` (org mods only), `next.error/called` in `.catch`.

### Mod event catalogue
- What: Tools (`tool.call`, `tool.check`, `tool.describe`); prompts (`prompt.submit/fill/suggest/edit/autocomplete/compose/section/context/attachment/mention`, `skill.prompt`, `attribution.text`); commands/config (`command.run/describe`, `config.set/describe`, `settings.read`); turns (`turn.start`, `turn.step` async-generator streaming each model request, can swap `model`/`effort` or answer without calling the model; `turn.complete`); session (`session.start/end/compact/receive/send/append/attach/detach/measure`); subagents (`agent.offer`, `agent.spawn` incl. resumes); interface (`ui.render/resolve/press/input/select/focus/scroll/close/message/fault`); other mods (`plugin.register`, `engine.create`); telemetry (`telemetry.log/mark`, must filter `{to:'collector'}`); every settings-hook event as `classic.<Event>` with stdin JSON as `e`; and every mods API call as an event (e.g. `fs.read`, `http.fetch`, `model.complete`) interceptable by earlier mods.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/reference.md (Events); https://code.claude.com/docs/en/plugins/mods/events.md (Follow a turn; Change or refuse a settings read; Handle the settings hook events)
- Notes: `session.append` can rewrite each stored conversation row (text blocks, tool_result content). `session.compact` can `{skip}` compaction. `prompt.section`/`tool.describe` changes in a running conversation need `breakPromptCache: true` (v2.1.321+) and cost a cache reset. `turn.step` result exposes `usage` and `serverToolUses` (v2.1.290+). `settings.read` on Claude Code's own read (v2.1.304+) can replace keys in merged settings if schema-valid, but permission rules and `env` returned there are not applied. The page says the generated `.d.ts` is authoritative over the docs.

### Mods API (`$`) namespaces
- What: `$.plugin`, `$.ui`, `$.theme`, `$.command` (register/run/list), `$.tool` (register/call/check/list), `$.agent` (register/spawn/list with statuses), `$.model` (complete/fork/classify), `$.prompt` (submit/read/fill/suggest/compose), `$.turn.abort`, `$.session` (messages, cwd, root, model, turns, id, repo, surfaces, usage, version, compact, send, append, authorize), `$.config`, `$.settings.read` (merged, by source user/project/local/flag/policy, or by key), `$.env` get/set (names must be literals), `$.fs` (read/write/list/exists/stat/ancestors; write non-atomic), `$.store` (JSON KV shared by all sessions of the machine), `$.state` (reactive), `$.clock`, `$.http.fetch`, `$.process.run/spawn` (argv, no shell), `$.mcp` call/connect, `$.audio` play/speak, `$.telemetry`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/reference.md (Mods API methods; Limits); https://code.claude.com/docs/en/plugins/mods/api.md (Reach files, processes, and the network)
- Notes: Limits: hook own execution 10 s (50 ms for `prompt.edit`), `.catch` 1 s, `session.end` hooks share the SessionEnd budget (1.5 s default); `$.process.run` 30 s default/10 min max, 4 MiB stdout/stderr each; `$.fs` 4 MiB/file; `$.http.fetch` 4 MiB req/resp, 30 s, 5 redirects, cross-origin redirect strips all but accept/accept-language/content-type/user-agent; `$.store` 4 MiB; `$.session.messages()` newest 4,096; names 64 chars `[A-Za-z0-9_-]`. `$.env.set` affects every command and MCP server started afterward.

### Mod-registered commands and tools
- What: `$.command.register({name, description, argumentHint, immediate})` in `session.start` adds a `/command` answered by a `command.run` hook returning `{text}` (printed with plugin prefix; Claude reads it) or `{}`; runs with no Claude turn, and with `immediate: true` even while Claude is working. `$.tool.register({name, description, inputSchema, isDeferred})` exposes a tool to Claude named `mcp__<plugin>__<name>`, handled by a `tool.call` hook filtered on that name.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/api.md (Add a command or a tool)
- Notes: Registering a name a built-in uses throws (and the rest of that hook is skipped). `isDeferred: false` (v2.1.293+) exempts the tool from MCP tool search deferral. Unanswered command prints "registered /x but no command.run hook answered it".

### Mod model calls (`$.model.complete` / `$.model.fork`)
- What: `complete` sends a standalone prompt (model you pass, attribution block plus optional system, no tools, no CLAUDE.md). `fork` replays the current conversation (session model, system prompt, project context, tools present but not callable) with your prompt appended, so it mostly hits the prompt cache. Both bill to the user's plan/API key/cloud. Prompt and system accept arrays of `{text, cache}` blocks to set cache breakpoints (v2.1.292+). Results: `isAnswered`, `text`, `reason`, `usage` with cache fields; API failures resolve with `isAnswered:false` rather than reject.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/api.md (Call a model; Use prompt caching; What a model.complete hook receives)
- Notes: maxTokens default 1024. Cache TTL from user settings (`subagentPromptCacheTtl`). Docs advise putting shared prefix in `prompt` not `system` on Bedrock/Vertex/Foundry/gateways because the attribution block fingerprint varies. A `model.complete` hook from another mod that changes the start of the prompt drops breakpoints.

### Mod messaging API between sessions
- What: `$.session.send({to, text})` uses the same delivery as SendMessage; `to` is `{sessionId}`, `{agentId}` (subagent/teammate from `$.agent.list()`), or a received sender address; resolves `{isDelivered}` or `{isDelivered:false, reason}`. `session.receive` hook sees inbound messages (origin kinds `peer`, `peer-send-message`, `task-notification`, `scheduled-trigger`) before Claude and can `{consumed}` them; `session.send` sees outbound from model or plugin.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/api.md (Send and receive messages between sessions); https://code.claude.com/docs/en/plugins/mods/reference.md (Session)
- Notes: Messages refused by `crossSessionInbound: refuse` never reach the hook, but held (unapproved) messages do, so a mod can read messages the user has not approved yet. Sender name is self-asserted.

### Mod authoring loop: Claude-written mods, hot reload, type generation, static analysis
- What: A built-in `plugin-authoring` skill lets Claude write mods into `~/.claude/dev-mods/<session-id>/<mod>/` (a protected path: each file needs approval in default/acceptEdits). On first save the user is asked whether to enable hot reloading for the session; mods then load at turn end and reload after each turn that changes them. `--plugin-dir` directories are watched and hot-reloaded on save (module re-runs `register`, module variables reset). Claude Code writes `.claude-plugin/types/` with `.d.ts` for the exact running build (events, API, built-in tool inputs, connected MCP tool inputs, dependency plugins) plus a tsconfig. `claude plugin validate` runs the same static analysis used at load and prints `hooks:`, `calls:`, `env reads/writes:`, `state reads/writes:`, and gating hooks without `.catch`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/create.md (Ask Claude for a mod; Sessions where a mod Claude writes can't load; Get type definitions for your version; Check what Claude Code reads from your mod)
- Notes: Static-analysis rules enforced at load: `$.ns.method` written in full (no aliasing/destructuring/computed access; passing `$` only to top-level functions in the same file), event names as string literals, no shadowed `on`, only relative imports plus bare `claude-code`, no dynamic `import()`, ES modules only, scope nesting ≤2,000. Claude-written mods don't load with nobody to approve (`-p` outside auto/bypass, `dontAsk`), untrusted workspace, safe/bare mode, `disableAllHooks`, or managed block. Dev-mods folders deleted after `cleanupPeriodDays`. After three unanswered hot-reload questions it stops asking. `CLAUDE_CODE_PLUGIN_DIRS` and `CLAUDE_CODE_PLUGIN_DIR_WATCH=1` for apps/headless.

### Mod test kit (`claude plugin test`)
- What: Runs `*.test.ts(x)` files with no session, sign-in or network. `test(name, [opts], async ($, on) => ...)`: the test's `$` acts as Claude Code and fires events through the mod's hooks (`$.tool.call`, `$.command.run`, `$.prompt.submit`, `$.session.start`, `$.turn.step`, `$.classic.*`...); `on` registers stubs answering in Claude Code's place (API stubs return `{value}` or `{deny}`). Mocks: `mock.clock` (advance/set/settle/sleep), `mock.store`, `mock.env`, `mock.session`. `$.ui.mount` renders a site per surface and exposes `press/input/select/find/unmount` by element key. `tier('prepend')` and inline `plugins` test policy mods.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/test.md (Write a test; Stub what Claude Code would answer; Test a timer; Test a drawing; Test a policy mod)
- Notes: Exits 1 on failure (CI-usable); 5 s per test default. Stubs must be registered before the first `$` call; `session.start` isn't fired automatically. `expect` has a small Jest-like matcher set. Also usable as a probe: run in an empty dir to learn whether mods can load (reports rollout switch off, policy, etc).

### Built-in mods
- What: Some Claude Code features are themselves mods listed under Built-in in `/plugin`: `cc-plugin-agents-md` (loads AGENTS.md), `cc-plugin-diff` (takes over `/diff` pane), `cc-plugin-plugin-authoring` (skill only), `cc-plugin-sec-default` (the guard), `cc-plugin-telemetry` (sends analytics records), `cc-plugin-you-should-know` (side agent notes, off by default). Source of diff, agents-md, sec-default and telemetry is public in the claude-code repo `mods/` directory.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/overview.md (Mods built into Claude Code; Read the source of built-in mods)
- Notes: `disableAllHooks`, `--bare`, `--safe-mode` do not stop built-in mods. Telemetry records are only sent when Claude Code or a built-in mod calls `$.telemetry`.

### Channels: MCP servers that push events into a running session
- What: A channel is a local stdio MCP server (spawned by Claude Code) declaring `capabilities.experimental['claude/channel']: {}` and emitting `notifications/claude/channel` with `content` and optional `meta` (identifier keys only; others silently dropped). Claude sees `<channel source="<server>" k="v">content</channel>`; the terminal shows a one-line `← source: ...`. Server `instructions` are delivered as context on connect. Two-way channels add an ordinary MCP reply tool. Events queue in order and are batched into the next turn if Claude is busy; no acknowledgement; dropped silently if not registered/blocked.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/channels-reference.md (Overview; Server options; Notification format; Expose a reply tool); https://code.claude.com/docs/en/channels.md (Push events into a running session)
- Notes: Must be opted in per session: `claude --channels plugin:<name>@<marketplace>` (space-separated list); being in `.mcp.json` is not enough. Flags hidden from `--help` during preview. Requires claude.ai or Console auth; not on Bedrock/Vertex/Foundry. Official plugins: Telegram, Discord (poll platform APIs, token in `~/.claude/channels/<x>/.env`), iMessage (reads `~/Library/Messages/chat.db`, replies via AppleScript, macOS only), fakechat demo on localhost:8787; all Bun scripts. On the v2 MCP client runtime a server negotiating protocol revision 2026-07-28 isn't registered as a channel (`MCP_PROTOCOL_NEGOTIATION=legacy` workaround, per mcp.md). In `-p` mode, tools needing terminal input (questions, plan approval) are disabled. Reply text isn't shown in terminal, only the tool call and "sent".

### Channel development flag and allowlist
- What: During preview only channels on an Anthropic-maintained allowlist (the `claude-plugins-official` channel plugins) or an org's `allowedChannelPlugins` register. `--dangerously-load-development-channels plugin:x@y | server:<name>` bypasses the allowlist per entry after a full-screen warning; ignored in `-p` and SDK; does not bypass `channelsEnabled`.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/channels-reference.md (Test during the research preview; Package as a plugin); https://code.claude.com/docs/en/channels.md (Research preview)
- Notes: Community marketplace is not on the allowlist; third-party channels require partner coordination or org allowlisting.

### Artifacts pulling live data via claude.ai connectors
- What: A published artifact may declare which claude.ai connectors it can call; on each view the page calls them through the viewer's own account (claude.ai makes the call, page never sees credentials), after a per-viewer permission ask. Calls can have side effects, executed as the viewer. Responses cached in the viewer's browser. Local MCP servers (`.mcp.json`) can feed data at build time but cannot be called by the published page.
- Status: GA (v2.1.209+)
- Evidence: https://code.claude.com/docs/en/artifacts.md (Pull live data with MCP connectors; How connector calls work for viewers; The page shows no live data for a viewer)
- Notes: No connector calls for anonymous/public or out-of-org viewers. v2.1.265+ checks declared tool names against available connector tools and refuses a publish when none match. Org toggle "Enable artifact connectors". Declaring runtime capabilities triggers a publish permission prompt.

### Subagent-scoped hooks and lifecycle events
- What: Frontmatter `hooks` run only while that agent is active, including when it is the main session via `--agent`. A frontmatter `Stop` hook becomes `SubagentStop`. Session-wide hooks from settings, managed policy and plugins all fire inside subagents. `SubagentStart` and `SubagentStop` matchers take the agent `name`; plugin-scoped names contain `:` and are treated as unanchored regexes, so the docs advise anchoring them with `^...$`. Hooks get `agent_type`. The example pattern is a PreToolUse script that exits 2 to block SQL writes.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Define hooks for subagents; Project-level hooks for subagent events; Conditional rules with hooks)

### Non-git VCS via WorktreeCreate/WorktreeRemove hooks
- What: A `WorktreeCreate` hook replaces the git logic. It reads `name` from stdin JSON and prints the directory path. `.worktreeinclude` is then not processed. Pair it with `WorktreeRemove`. The hook's directory must be outside any git repo, or it is refused. It also enables `/batch` outside git (v2.1.281+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/worktrees.md (Non-git version control; Replace worktree creation with a hook)

### MCP in cloud sessions and the built-in "Claude Code Remote" server
- What: Enabled claude.ai MCP connectors work without allowlisting because traffic goes through Anthropic. The control plane attaches its own MCP server ("Claude Code Remote") whose tools create cloud scheduled tasks, start and steer other cloud sessions, attach repositories, and follow PR activity. It registers under one of three names (`Claude_Code_Remote`, `claude-code-remote`, a UUID), so deny rules must list all three.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cloud-environments.md (Network access note); https://code.claude.com/docs/en/self-hosted-environments-configuration.md (Turn off built-in session tools)
- Notes: This server is how auto-fix and "add repo" happen from inside a session.

### Runner wrapper scripts (`--exec-path`) and `command` hook
- What: The runner starts your wrapper instead of the binary on every start, resume and requeue; wrapper must end with `exec "$CLAUDE_RUNNER_CLAUDE_BIN" "$@"`. Environment provided: session JWT, creator email (unverified, label only), client platform (`web_claude_ai`, `desktop_app`, `ios`, `claude_code_cli`, `scheduled_trigger`), session IDs in `cse_`/UUID form, Slack thread/channel IDs for Claude Tag, ingress token file path, per-session `CLAUDE_CONFIG_DIR`, delivered MCP config path, `ANTHROPIC_BASE_URL`, a ~30-minute inference `CLAUDE_CODE_OAUTH_TOKEN`, alive interval. Typical use: decode token, exchange `act.sub` for short-lived cloud creds (example uses `jq -re`, refuses `agent:` subjects), then exec. `--permission-mode auto` or `--allowed-tools` appended after `"$@"` (last mode wins; list flags accumulate).
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-configuration.md (Wrapper scripts; Provision credentials scoped to the session creator; Pin a permission mode from your wrapper; Pre-approve specific tools; command)
- Notes: Docs warn about quoting runner-passed values (JSON, permission rules) and never interpolating them into `eval`/`sh -c`.

### Runner lifecycle hooks: `checkout`, `post-session`
- What: `--hooks-dir` with well-known executable names; missing hooks fall back to built-ins. `checkout` replaces clone/fetch per repo on every start/resume (vars: repo URL after rewrites, ref, checkout path `<owner>-<repo>`, session IDs, API base URL, session token); must leave a `.git` unless `CLAUDE_RUNNER_SKIP_GIT_VERIFY=1` (Perforce/tarball). Failure fails the session for the push target, skips read-only repos. `post-session` runs after the child exits, before teardown; only chance to save uncommitted work; gets `CLAUDE_RUNNER_EXIT_REASON` (completed, failed, interrupted, abandoned), workspace paths, debug log path; 60 s timeout; exit status ignored; does not fire on abrupt kill.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-configuration.md (Lifecycle hooks; checkout; post-session; Hook timing when the runner releases a session)
- Notes: Hooks run with the runner's privileges and the session shares that UID, so the hooks dir must be read-only to sessions.

### MCP on self-hosted runners
- What: Sources: `claude mcp add --scope user` baked into the image; enterprise `managed-mcp.json` (exclusive; then control-plane-delivered servers and connectors are skipped); `managedMcpServers` in managed settings (v2.1.259+); repo `.mcp.json` (auto-approved in cloud sessions); `--mcp-config` from the wrapper. claude.ai connectors are delivered server-side to interactively created sessions only and their calls run from Anthropic's infrastructure, not the runner. Startup wait 5 s for `alwaysLoad` HTTP/SSE servers, first-turn wait 2 s for stdio.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-configuration.md (MCP servers; Wait for MCP servers before the first turn); https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Connector traffic leaves your network)
- Notes: A same-named local server is not connected when a connector of that name is delivered.

### Plugin marketplaces on GHES
- What: How credentials are handled depends on the surface:
  - CLI, Desktop, and managed `extraKnownMarketplaces`: clone with the machine's git credentials.
  - claude.ai org plugin settings: the backend fetches with the org GitHub App.
  - claude.ai user settings: the user's own GHES connection.
  - Cloud sessions: not reliable, because the sandbox's git credentials are scoped to the session's repos.
  The `owner/repo` shorthand always means github.com. Git runs non-interactively and rejects SSH hosts missing from `known_hosts`, so HTTPS with a credential helper is recommended. A marketplace without credentials is registered but never installed, and its plugins then show as "not found". `strictKnownMarketplaces` accepts a `hostPattern` regex source to allow a whole host.
- Status: GA
- Evidence: https://code.claude.com/docs/en/github-enterprise-server.md (Plugin marketplaces on GHES; Pre-register GHES marketplaces; Allowlist GHES marketplaces in managed settings)

### Built-in IDE MCP server (IDE-to-CLI bridge)
- What: The VS Code extension or JetBrains plugin runs a local MCP server named `ide`. The CLI connects to it automatically when started in the integrated terminal, or with `/ide` from an external terminal. The server is hidden from `/mcp`. Through it the CLI opens diffs in the native diff viewer, reads the selection, and (in VS Code) runs Jupyter cells.
  - Transport: unencrypted `ws://` on loopback. VS Code picks a random port in 10000–65535; JetBrains uses an OS-assigned ephemeral port. Neither port is configurable.
  - Auth: each activation writes a fresh random token to `~/.claude/ide/<port>.lock` (file 0600 inside a 0700 directory, under `$CLAUDE_CONFIG_DIR/ide/` if set). The CLI sends it as the `X-Claude-Code-Ide-Authorization` header. The docs argue TLS would add nothing on loopback.
  - Tools the model can see: the server hosts about a dozen RPC tools, but only `mcp__ide__getDiagnostics` (read-only language-server or inspection diagnostics, optionally for one file) and, in VS Code only, `mcp__ide__executeCode` are visible to the model. The rest are filtered out of the tool list.
  - JetBrains option: "Accept connections from all network interfaces" exposes the port and the cleartext token on the LAN, for WSL2 NAT or remote setups. Mirrored WSL networking is recommended instead.
- Status: GA (the JetBrains plugin is labelled "[Beta]" in its settings path and Marketplace name)
- Evidence: https://code.claude.com/docs/en/vs-code.md (The built-in IDE MCP server); https://code.claude.com/docs/en/jetbrains.md (The built-in IDE MCP server; Listening interface)
- Notes: An org that allow-lists MCP tools with a `PreToolUse` hook must account for the `ide` server.

### Extensibility supply-chain locks (managed)
- What: `allowManagedHooksOnly` runs only managed hooks, Agent SDK in-process hooks, and hooks of plugins force-enabled by managed `enabledPlugins` (matched on full `plugin@marketplace`); blocks user/project/local/agent-frontmatter hooks, other plugin monitors (v2.1.310+), command-sourced plugins and marketplace `headersHelper` commands, and narrows `statusLine`/`fileSuggestion` to managed values. `disableAllHooks` in managed settings disables every hook including managed ones; elsewhere only non-managed hooks. `strictPluginOnlyCustomization` (`true` or array of `skills`/`agents`/`hooks`/`mcp`) restricts those surfaces to plugins and managed sources; unknown surface names are ignored so admins can pre-deploy new names. `strictKnownMarketplaces` (allowlist) and `blockedMarketplaces` (checked before download). `disableSideloadFlags` (v2.1.193+) rejects `--plugin-dir`, `--plugin-url`, `--agents`, `--mcp-config` and `CLAUDE_CODE_PLUGIN_DIRS`. MCP: `allowManagedMcpServersOnly`, `allowedMcpServers`, `deniedMcpServers` (merges from all sources), `managedMcpServers` (v2.1.259+, provides http/sse servers without exclusive control).
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-reference.md (allowManagedHooksOnly; disableAllHooks; strictPluginOnlyCustomization; disableSideloadFlags); https://code.claude.com/docs/en/managed-settings.md (Keys only a managed source can set)
- Notes: `/goal` cannot run while `allowManagedHooksOnly` or `disableAllHooks` is set because it depends on hooks.

### Gateway protocol reference endpoint
- What: A running gateway serves `GET /protocol` describing the endpoints it exposes to clients (SSO sign-in, inference, managed settings, model discovery, telemetry, and since v2.1.227 the usage-limit headers and 429 body). A separate compatibility guide documents what Claude Code expects from any third-party LLM gateway. Breaking changes announced in advance; indefinite backwards compatibility not guaranteed.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway.md (Other gateway implementations); https://code.claude.com/docs/en/claude-apps-gateway-spend-limits.md (How enforcement works)

### In-process MCP custom tools
- What: Tools are defined with TS `tool(name, description, zodShape, handler, extras)` or Python `@tool(name, description, schema)` (dict of types, `Annotated` for descriptions, or raw JSON Schema), grouped by `createSdkMcpServer`/`create_sdk_mcp_server` into an MCP server that runs inside the host application process (not a separate process) and passed via `mcpServers`. Names become `mcp__<serverKey>__<tool>` (`.`/spaces -> `_`). Handlers return MCP `CallToolResult` (`content` blocks text/image/audio/resource/resource_link, optional `structuredContent`, `isError`). Uncaught handler exceptions are converted to error results and the loop continues. Annotations (`readOnlyHint` enables parallel calls; `destructiveHint`, `idempotentHint`, `openWorldHint`) are hints, not enforcement. TS extras: `searchHint`, `alwaysLoad`; server options `instructions`, `timeout` (ms, >= 1000, overrides `MCP_TOOL_TIMEOUT`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/custom-tools.md (Create a custom tool; Call a custom tool; Add tool annotations; Handle errors; Return images and resources; Return structured data); https://code.claude.com/docs/en/agent-sdk/typescript.md (tool(); createSdkMcpServer())
- Notes: Language gaps: Python drops audio and binary resource blocks (warning), flattens resource links, and cannot return `structuredContent` from `@tool` (use a standalone MCP server). TS saves audio to disk and passes a path. Images must be inline base64 (no URLs); non PNG/JPEG/GIF/WebP images are saved to disk and passed as a path. When TS sets `structuredContent`, text blocks in `content` are not sent to the model. The init handshake carries `sdkMcpServerManifests` for in-process servers; `tools/list_changed` from an SDK server relists its tools (v2.1.286+). Unconvertible Zod schemas drop only that tool with warning `CLAUDE_SDK_MCP_TOOL_SCHEMA_UNCONVERTIBLE` (from v0.3.286; earlier the whole server listing failed).

### External MCP servers in the SDK
- What: `mcpServers` accepts stdio (`command`, `args`, `env`), `sse` and `http` (`url`, `headers`) and `sdk` configs; `.mcp.json` loads via the `project` setting source; `strictMcpConfig` ignores every source except `mcpServers` (including plugin servers and claude.ai connectors). MCP tools need explicit permission (`allowedTools` wildcard `mcp__server__*` recommended over `acceptEdits`, which does not cover MCP, or bypass). No interactive OAuth: servers needing auth report `needs-auth`/`failed` and the run continues without them; the app must run OAuth itself and pass the token in `headers`. Status values `pending|connected|failed|needs-auth|disabled`; remote drops go back to `pending`, five failed reconnects -> `failed`, with background retries for HTTP/SSE. Runtime control: `mcpServerStatus()`, `reconnectMcpServer()`, `toggleMcpServer()`, `setMcpServers()`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/mcp.md (Add an MCP server; Connection timing; Allow MCP tools; Transport types; Authentication; Error handling; Troubleshooting); https://code.claude.com/docs/en/agent-sdk/typescript.md (Options; Query object; McpServerConfig)
- Notes: Startup timing is intricate: stdio and in-process servers in `mcpServers` delay the first turn up to `MCP_TIMEOUT` (30 s); settings/plugin servers wait up to 2 s only if named in allow rules or `alwaysLoad`; cached tool lists let remote servers skip the wait; `CLAUDE_CODE_MCP_STARTUP_WAIT_MS` (v2.1.274+) overrides. Output cap: successful non-image results over 25,000 tokens (`MAX_MCP_OUTPUT_TOKENS`) or 50,000 chars are saved to a file and replaced by an error naming the path so the agent reads it in portions.

### SDK callback hooks
- What: Hooks are in-process callbacks registered as `{event: [{matcher, hooks[], timeout, onFailure}]}`; they run in the app process (no context cost) alongside shell hooks from loaded settings files. Matchers follow settings-file rules (exact list `A|B` or regex) and match only the tool name. Inputs share `session_id`, `cwd`, `hook_event_name`, plus `agent_id`/`agent_type` inside subagents and a tool-use ID to correlate Pre/Post. `PreToolUse` returns `permissionDecision` `allow|deny|ask|defer` with `updatedInput`; `PostToolUse` can add `additionalContext` or replace output with `updatedToolOutput`. Multiple hooks run in parallel; precedence deny > defer > ask > allow. Async output (`async: true`) for fire-and-forget side effects.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/hooks.md (How hooks work; Available hooks; Configure hooks; Callback functions; Block the action when a callback fails; Hook timeout; Fix common issues)
- Notes: Event coverage differs: Python supports about 10 events (PreToolUse, PostToolUse, PostToolUseFailure, UserPromptSubmit, Stop, SubagentStart/Stop, PreCompact, PermissionRequest, Notification); TS adds 23 more (PostToolBatch, UserPromptExpansion, MessageDisplay, StopFailure, PostCompact, Pre/PostModelSwitch, PermissionDenied, SessionStart/End, Setup, TaskCreated/Completed, Elicitation*, ConfigChange, InstructionsLoaded, Worktree*, CwdChanged, FileChanged, DirectoryAdded, TeammateIdle). Fail-open by default: a throwing or empty-return callback lets the action proceed with an informational `hook_callback_failed`; `onFailure: "block"` (TS v0.3.317+, Py v0.2.177+) makes it fail closed. Default timeout 600 s (30 s for prompt/model-switch hooks, 10 s MessageDisplay, 1.5 s SessionEnd); a `PreToolUse` timeout blocks the call. `defer` ends the turn with `stop_reason: "tool_deferred"` and `deferred_tool_use` so the app can resume later. TS `classifierContext` lets a PostToolUse hook annotate results for the auto-mode classifier. Notification hook fires `permission_prompt` after ~6 s waiting on `canUseTool`.

### Structured outputs
- What: `outputFormat: {type: "json_schema", schema}` makes Claude Code add a synthetic `StructuredOutput` tool; the agent may use any tools, then calls it; mismatches return an `is_error` tool result and the agent retries; validated data lands in `structured_output` on the result. Failure gives `error_max_structured_output_retries` (also when a model fallback retracts a completed output). Schemas validated as JSON Schema draft-07 (newer drafts rejected; Zod needs `target: "draft-7"`); `format` is an annotation only.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/structured-outputs.md (Quick start; Output format configuration; Recognize the StructuredOutput tool call; Error handling)
- Notes: A `success` subtype does not guarantee output: check `is_error` and presence of `structured_output`. Since v2.1.205 invalid schemas fail at startup (before, silently ignored). Apps are told to hide `StructuredOutput` calls from tool-call UIs by matching `tool_use_id`.

### Skills and commands in SDK sessions
- What: Skills are filesystem-only (`.claude/skills/<name>/SKILL.md`; no programmatic registration), discovered via setting sources (user, project up to repo root, `additionalDirectories`). `skills` option: omitted = all discovered, `"all"`, exact-name list (`plugin:skill` for plugin skills; wildcards rejected before spawn), or `[]`; setting it auto-adds `Skill` to `allowedTools`. Unlisted skills are hidden from the model and rejected by the Skill tool, but `/<name>` dispatch still runs them and their files stay readable. Commands are sent as prompt text (`/compact`, `/clear`, bundled `/code-review`, user skills, legacy `.claude/commands/*.md`); init `slash_commands` lists those that work headless. Unknown `/x` now becomes an ordinary message (v2.1.274+); unavailable built-ins return a fixed text without a model turn.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/skills.md (How skills work; Use skills; Commands in Agent SDK sessions; Pre-approve tools for skills; Troubleshooting); https://code.claude.com/docs/en/agent-sdk/claude-code-features.md (Skills)
- Notes: `allowedTools` and skill `allowed-tools` are ignored under managed `allowManagedPermissionRulesOnly`. TS `reloadSkills()` picks up edits mid-session; results report `local_command` for turns a command completed without the loop.

### Plugins in the SDK
- What: `plugins: [{type: "local", path, skipMcpDiscovery?}]` loads a plugin directory (skills, agents, hooks, MCP servers, legacy commands; manifest `.claude-plugin/plugin.json` optional). Only local paths; marketplace plugins must be downloaded first (or point at `~/.claude/plugins/`). Relative paths resolve against `cwd`; `~` is not expanded; missing paths are skipped silently, with failures reported in init `plugin_errors`. TS `reloadPlugins({addPlugins})` adds plugins mid-session (v0.3.301+); Python sends `/reload-plugins` as a prompt.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/plugins.md (Loading plugins; Reload plugins; Add plugins to a running session; Troubleshooting); https://code.claude.com/docs/en/agent-sdk/typescript.md (SdkPluginConfig; SDKSystemMessage)
- Notes: Plugin skills are namespaced `/plugin-name:skill-name`; plugin MCP server names start with `plugin_`.

### Tool aliasing to replace built-ins (TS)
- What: `toolAliases: { Bash: 'mcp__workspace__bash' }` routes the model's calls for a built-in tool to an MCP implementation the host provides.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (Options)
- Notes: One-line option description only; combined with `spawnClaudeCodeProcess` this is how a host could execute tools elsewhere. Also exported: `BUILTIN_TOOL_NAMES`, `LEGACY_TOOL_NAME_ALIASES` (alpha; `Task`->`Agent`, `KillShell`/`KillBash`->`TaskStop`).

### Elicitation, MCP Apps resources and host file reads (TS)
- What: `onElicitation` callback answers MCP server user-input requests (declined automatically if absent); `readMcpResource()` (alpha) fetches MCP Apps `ui://` widget resources; `readFile()` reads files in the session's working dirs through the CLI (1 MB default, 10 MB ceiling, base64 option, null on denial).
- Status: GA (`readMcpResource` alpha)
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (Options; Query object; What readFile() can read)
- Notes: `readFile()` lets a host with a remote CLI process read files without its own filesystem access.

### Claude Mods (plugins that modify deeper behavior)
- What: introduced in 2.1.287 (Oct 1, 2026). Plugins can register hooks at gating sites such as:
  - `tool.check` (with `decider: "person"`, `e.isReadOnly`, `ceiling` and `agentId`);
  - `agent.spawn` (can rewrite a workflow agent's prompt and model);
  - `prompt.submit` / `prompt.change` / `prompt.autocomplete`, `session.append`, `turn.step`, `skill.prompt`.
  
  APIs include `$.model.complete` (with caching), `$.model.fork` (with `effort`), `$.tool.register` (`isDeferred`), `$.tool.call` / `$.mcp.call` (no permission prompt, since 2.1.321), `$.ui.*` (notify, selection, view), `$.session.identity()` and `$.prompt.media` (managed-settings mods only), plus terminal UI components (`Text`, `Box`, `Button`, `Input`). A built-in mod, "You should know", runs a side agent that flags things you or Claude might miss.

  Admin side: mods Claude writes turn on without asking in auto and bypass modes and do not load in `dontAsk` (2.1.321); validate reports skipped and gating hooks.
- Status: GA per the changelog (no preview label)
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.287, 2.1.288, 2.1.290, 2.1.292, 2.1.293, 2.1.295, 2.1.299, 2.1.300, 2.1.301, 2.1.304, 2.1.318, 2.1.321); own pages plugins_mods_*.md
- Notes: absent from every digest, because it shipped after Week 37. `decider: "person"` (2.1.304) appears in no docs page in the dump. That a mod's own tool calls skip permission prompts is a security-relevant default.

### Plugin tooling
- What:
  - `claude plugin eval` scores a plugin against cases, with a no-plugin baseline; `eval init` drafts the suite (2.1.269, Week 37). `--disallow-tools` (2.1.299).
  - `claude plugin validate --json` (2.1.259) with MCP checks (2.1.281).
  - Install from `.zip` / URL (Week 19).
  - New sources: `archive` (HTTPS zip with SHA-256 pin, 2.1.224) and `command` (2.1.229).
  - `--accept-command <sha256>` (2.1.271). `claude plugin configure` (2.1.285). `--marketplace` on install (2.1.275, 2.1.292).
  - `--plugin-dir` accepts a folder of plugins (2.1.265).
  - `/skill-doctor` reports per-skill context cost (2.1.261).
  - Skills and plugins enabled on the claude.ai account sync to the terminal (2.1.275; `syncClaudeAiSkills` / `syncClaudeAiPlugins` opt out).
  - `${user_config.*}` is rejected in shell-form commands as a shell-injection fix (2.1.207).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.207, 2.1.224, 2.1.229, 2.1.259, 2.1.261, 2.1.265, 2.1.269, 2.1.271, 2.1.275, 2.1.281, 2.1.285, 2.1.292, 2.1.299); https://code.claude.com/docs/en/whats-new/2026-w19.md, https://code.claude.com/docs/en/whats-new/2026-w37.md

### Hooks additions
- What:
  - `PreModelSwitch` / `PostModelSwitch` (2.1.251). `DirectoryAdded` (2.1.219). `PermissionDenied` with `{retry:true}` (2.1.89).
  - Conditional `if` hooks (2.1.85).
  - The Notification hook fires `agent_needs_input` / `agent_completed` (2.1.198).
  - Since 2.1.300, `updatedInput` from PreToolUse and PermissionRequest is validated like model input: unknown keys are dropped and rejected input denies the call.
  - Hyphenated matchers exact-match (Week 27).
  - PreToolUse auto-allow no longer bypasses restrictions in internal side tasks (Week 32).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.85, 2.1.89, 2.1.198, 2.1.219, 2.1.251, 2.1.300); https://code.claude.com/docs/en/whats-new/2026-w27.md, https://code.claude.com/docs/en/whats-new/2026-w32.md, https://code.claude.com/docs/en/whats-new/2026-w36.md

### MCP protocol and client changes
- What:
  - Negotiates MCP protocol 2026-07-28 by default: stdio in 2.1.292, claude.ai connectors in 2.1.295, cloud and Remote Control in 2.1.299, with `MCP_PROTOCOL_NEGOTIATION=legacy` as the opt-out. The v2 MCP client was extended to Bedrock, Vertex, Foundry and telemetry-off installs in 2.1.274.
  - URL-mode elicitation (2.1.281, 2.1.287; `bareElicitationCapability` is a compatibility escape).
  - MCP tasks run in the background (2.1.319).
  - Re-auth when a server asks for more OAuth scope (2.1.288). `claude mcp login` / `logout` (Week 26). `/mcp reconnect all` (2.1.284).
  - `managedMcpServers`: org-provided HTTP/SSE servers only (2.1.259).
  - `roots/list` includes added directories (2.1.203).
  - @-mentioning to toggle servers was removed (2.1.6).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.6, 2.1.203, 2.1.259, 2.1.274, 2.1.281, 2.1.284, 2.1.287, 2.1.288, 2.1.292, 2.1.295, 2.1.299, 2.1.319); https://code.claude.com/docs/en/whats-new/2026-w26.md

## Area 14. Models, providers and inference configuration

### Model, effort and system-prompt CLI flags
- What: `--model` (aliases `sonnet`, `opus`, `haiku`, `fable`, or full name; overrides `model` setting and `ANTHROPIC_MODEL`); `--fallback-model a,b` ordered chain on overload/unavailability (persist via `fallbackModel`); `--effort low|medium|high|xhigh|max|ultracode` (session-only); `--advisor <model>` enables the server-side advisor tool; `--betas` (API-key users). System prompt: `--system-prompt[-file]` replaces; `--append-system-prompt[-file]` appends (file text first; both forms combinable v2.1.283+); `--system-prompt-snapshot on|off` controls reuse of the prompt recorded on the first request; a line `__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__` in replacement text splits cached static part from dynamic part (v2.1.275+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/cli-reference.md (CLI flags; System prompt flags; System prompt flags in resumed conversations)
- Notes: Docs warn that replacing drops default tool guidance and safety instructions. `/model` saves as default for new sessions; press `s` in the picker for session-only (https://code.claude.com/docs/en/commands.md /model).

### Default model and effort
- What: New sessions run Claude Opus 5.5 at medium effort on most plans and providers; Fable 5.2, Opus 5.5, Sonnet 5.5 and Haiku 5.5 default to `medium`. `ultrathink` in a prompt raises reasoning for one turn without changing the setting; phrases like "think hard" are ordinary text. `max` effort is session-only and said to overthink ordinary tasks.
- Status: GA
- Evidence: https://code.claude.com/docs/en/best-practices.md (Change a default; Change the model and effort); https://code.claude.com/docs/en/commands.md (/effort)
- Notes: Effort is supported on Fable models, Opus 4.6+, Sonnet 4.6+, Haiku 5.5 (https://code.claude.com/docs/en/glossary.md Effort level).

### Classifier model selection
- What: Client-side classifier requests default to Claude Sonnet 5, overridden by an Anthropic-configured model, by Sonnet 4.6 if the session runs on it, or by `ANTHROPIC_DEFAULT_SONNET_MODEL` (unless that is Sonnet 5.5/Opus 5.5). Fallback is the session model (Opus in Fable sessions). `CLAUDE_CODE_AUTO_MODE_MODEL` (v2.1.302+) sets a full model ID; Opus 5, Sonnet 5.5, Opus 5.5 or policy-excluded models are ignored; an unservable model makes every check fail and deny. Debug log line `classifier_request_started model=...` verifies.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Default classifier model; Choose the classifier model; Check which model the classifier used)

### Advisor tool (stronger model consulted mid-task)
- What: A server-side Anthropic tool. Claude decides when to call it; it tends to consult before committing to an approach, on recurring errors, and before declaring done. The advisor gets the full conversation, including all tool calls, and returns guidance that Claude applies, surfacing a conflict when its own evidence contradicts the advice. It is set with `/advisor <model>` (saved to `advisorModel`), the `advisorModel` setting, or the hidden `--advisor` flag (exits at launch on a bad pairing; background sessions start without it instead). A ranking table requires the advisor to rank at or above the main model. A lower-ranked advisor is not attached; an API refusal of the pairing makes Claude Code resend without the advisor, silently. Subagents inherit the advisor and re-check the pairing against their own model. The transcript shows `Advising`, then reviewed, declined or `Advisor unavailable (<code>)`; Ctrl+O shows the guidance. There is no cap or force setting. Toggling the advisor does not invalidate the main prompt cache. The advisor's read is never cached, so each call re-reads the full transcript.
- Status: experimental (Anthropic API only; not Bedrock, Claude Platform on AWS, Agent Platform or Foundry; gateways only if they pass the request through)
- Evidence: https://code.claude.com/docs/en/advisor.md (Enable the advisor; Choose an advisor model; When Claude consults the advisor; Impact on prompt caching; Requirements)
- Notes: Needs feature-flag fetching, so it is off under `DISABLE_TELEMETRY` and similar. `CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1` kills it. Headless `/advisor` works in v2.1.260+. Fable as advisor needs usage-credit consent on some plans. Billing is at the advisor model's rates.

### Self-hosted model routing to Bedrock / Agent Platform
- What: Set `CLAUDE_CODE_USE_BEDROCK` or `CLAUDE_CODE_USE_VERTEX` (exactly one) on all runners; sessions call models in your cloud account. The event stream with prompts, responses and tool results still goes to `api.anthropic.com`. Consequences: server-managed settings and org policies don't reach these sessions (use image managed settings), no account skills, no attachments or `SendUserFile`, `ANTHROPIC_MODEL` stripped (model comes from control plane; alias pinning variables work), no web search on Bedrock, no fast mode. Warning that any code in the session, including via prompt injection, can use the cloud credentials.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-configuration.md (Send model requests to Bedrock or Agent Platform; What differs from sessions on the Anthropic API); https://code.claude.com/docs/en/self-hosted-environments.md (Availability and limitations)
- Notes: By default inference uses an Anthropic-issued, session-scoped OAuth token and an endpoint delivered by the control plane; don't override `ANTHROPIC_BASE_URL`.

### Model aliases and provider-specific resolution
- What: The `model` value can be an alias or a full model name (Anthropic ID, Bedrock inference profile ARN, Foundry deployment name, Agent Platform version name). Aliases: `default` (clears override, returns to account default), `best` (same as `opus`), `fable`, `sonnet`, `opus`, `haiku`, `sonnet[1m]`, `opus[1m]`, `opusplan`. Aliases resolve to different versions per provider: Anthropic API Opus 5.5/Sonnet 5.5/Haiku 5.5; Claude Platform on AWS Opus 5.5/Sonnet 4.6/Haiku 4.5; Bedrock and Agent Platform Opus 5.5/Sonnet 4.5/Haiku 4.5; Foundry Opus 4.6/Sonnet 4.5/Haiku 4.5. `fable` is Fable 5.2 except Fable 5 in Claude apps gateway sessions. Pinning: full ID or `ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU,FABLE}_MODEL`. A version-history table records the CLI version at which each alias moved.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Model aliases; Version history)
- Notes: Version gates: Opus 5.5 v2.1.280+, Sonnet 5.5 v2.1.284+, Haiku 5.5 v2.1.293+, Fable 5.2 v2.1.321+. `ANTHROPIC_SMALL_FAST_MODEL` deprecated in favour of `ANTHROPIC_DEFAULT_HAIKU_MODEL`. `ANTHROPIC_BASE_URL` changes where requests go, not which model answers.

### Model selection precedence and persistence
- What: Priority: `/model` in session, `--model`, `ANTHROPIC_MODEL`, `model` setting, then `ANTHROPIC_DEFAULT_MODEL` (default for new sessions, used only if nothing else selects; v2.1.236+). `/model <name>` or Enter in the picker saves to user settings; `s` in the picker applies to this session only. `-p` mode `/model` is session-only. Resumed sessions keep the transcript's model unless retired/excluded or the provider uses deployment IDs (Bedrock/Vertex/Foundry: not restored). Startup header names the settings file when project/managed settings set the model. Mistyped `--model`/env/setting values are not rejected up front; first request errors. Retirement/remap warnings go to the startup notice or stderr in `-p` text mode; JSON output reports the real model in `modelUsage`. SDK/app `setModel()` switches validate the ID with the provider (v2.1.268+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Setting your model; Set a default model for new sessions; A new session starts on a different model than you picked)
- Notes: Behaviour flipped twice: v2.1.144-152 `/model` was session-only with `d` to save. Enterprise accounts record the `/model` choice server-side (v2.1.280+). `PreModelSwitch` hooks can block or confirm a switch; if managed plugin hooks cannot be determined the switch is refused (fail closed).

### Fable models and usage-credit consent
- What: Fable is never the account default; selected explicitly. On some plans Fable bills to usage credits, shown as "Requires usage credits" in the picker; an interactive consent prompt appears (continue on Fable or switch to default). In unattended contexts (Remote Control, background sessions, teammates) the mid-session prompt waits up to `dialogExpiry` (5 minutes default) then ends the turn without sending. In `-p` and SDK apps that do not show the prompt, Claude Code bills without asking.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Work with Fable; Fable and usage credits)
- Notes: A saved `claude-fable-5-1` is migrated to the `fable` alias on Pro/Max/Ultra at first run of v2.1.321+. Fable rows stay visible but greyed under ZDR. Vendor guidance on prompting Fable ("describe the outcome") is marketing-flavoured.

### opusplan hybrid alias
- What: `opusplan` uses `opus` in plan mode and switches to `sonnet` for execution; each phase uses its family's context window; `opusplan[1m]` requests 1M for both. With an allowlist excluding newest Opus, planning uses newest permitted Opus (Anthropic API and Claude Platform on AWS only); on provider-ID platforms plan mode stays on the session model. A Haiku session upgrades to Sonnet in plan mode similarly.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (opusplan model setting)
- Notes: The page points to a separate advisor tool for mid-task consultation of a second model (advisor.md not in this slice).

### Effort levels
- What: Effort controls adaptive reasoning. Levels by model: Fable and 5.x/Opus 4.7-4.8: low/medium/high/xhigh/max; Opus 4.6 and Sonnet 4.6: low/medium/high/max; unlisted models have no effort. Unsupported level falls back to the highest supported at or below. Resolution: `CLAUDE_CODE_EFFORT_LEVEL`, in-session pick (`/effort`, `/model` slider, SDK `applyFlagSettings()`), `--effort`, settings (`modelSettings` per-model or `effortLevel`), model default (medium on Fable 5.2/Opus 5.5/Sonnet 5.5/Haiku 5.5, xhigh on Opus 4.7, high elsewhere; org default can apply). Saved per model under `modelSettings`. `max` is session-only unless set by env. `CLAUDE_CODE_EFFORT_LEVEL=auto|unset` sends no effort (API default applies); `/effort status` confirms. A new level takes effect from the next message.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Adjust effort level; Choose an effort level; Set the effort level; When a new effort level starts; Send no effort level); https://code.claude.com/docs/en/settings-reference.md (modelSettings)
- Notes: Scale is calibrated per model, so the same name differs across models. Top-level `effortLevel` in user settings is ignored for Opus 5.5 and later (legacy key). Before v2.1.304 `/effort status` misreported.

### Ultracode toggle and ultrathink keyword
- What: Ultracode is a Claude Code setting (not an effort level) that makes Claude orchestrate dynamic workflows for substantive tasks; toggled via `/effort ultracode [off]`, Tab in the slider, `--effort ultracode` (also sets xhigh), or `"ultracode": true`. Unavailable when workflows are off or the model lacks xhigh. `ultrathink` anywhere in a prompt adds an in-context instruction and, typed in the terminal, can raise that one turn to xhigh (rainbow highlight indicates it will). Other phrases like "think hard" are plain text.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Adjust effort level; Use ultrathink for one-off deep reasoning)
- Notes: Ultrathink-raises-effort needs v2.1.311+; not effective on Bedrock (page suggests setting effort instead). Ultracode independent of level since v2.1.284.

### Extended thinking controls and adaptive reasoning
- What: Toggle thinking per session with Option/Alt+T, globally via `/config` (`alwaysThinkingEnabled`), or `MAX_THINKING_TOKENS=0`. Thinking cannot be turned off on Opus 5.5, Sonnet 5.5, Haiku 5.5 or Fable (UI shows "Thinking can't be turned off"). Fable, Sonnet 5+, Haiku 5.5 and Opus 4.7+ always use adaptive reasoning; on Opus 4.6/Sonnet 4.6 `CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING=1` reverts to a fixed `MAX_THINKING_TOKENS` budget. Thinking display is collapsed (Ctrl+O verbose); interactive Anthropic-API sessions receive redacted thinking unless `showThinkingSummaries: true`. All thinking tokens are billed. With thinking off, Claude Code downgrades effort to `high` for models that reject higher effort without thinking.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Adaptive reasoning and fixed thinking budgets; Extended thinking); https://code.claude.com/docs/en/costs.md (Adjust extended thinking)
- Notes: On third-party providers `MAX_THINKING_TOKENS=0` omits the thinking parameter, and adaptive models may still think.

### Extended (1M) context window
- What: 1M window supported by Fable, Sonnet 5+, Haiku 5.5, Opus 4.6+, Sonnet 4.6. Fable, Sonnet 5+, Haiku 5.5 and Opus 4.7+ use 1M by default (including Bedrock, Agent Platform, Foundry, apps gateway). Opus/Sonnet 4.6 need the `[1m]` variant, with plan gating (e.g. Sonnet 4.6 1M needs usage credits on every subscription). No long-context premium on the API except Haiku 5.5 above 100K prompt tokens. `CLAUDE_CODE_DISABLE_1M_CONTEXT=1` holds every model to 200K (compacts at 200K, or errors if auto-compact off). The `[1m]` suffix is stripped before sending and is read per variable on third-party providers.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Extended context; Turn off 1M context; Haiku 5.5 context window and pricing; Pin models for third-party deployments)
- Notes: Plan checks run only against the direct Anthropic API; behind a gateway the gateway decides. Before v2.1.287 Bedrock/Vertex/Foundry ran these models at 200K by default.

### Fallback model chains (availability-based)
- What: On overload, unavailability or other non-retryable server errors, Claude Code tries configured fallbacks in order, with a notice; the switch lasts for the current turn only. Auth, billing, rate-limit, request-size, transport errors and org policy denials never trigger it; Bedrock/Agent Platform "model not invocable" refusals do. Configure with `--fallback-model a,b` (session) or `fallbackModel: [...]` (settings); flag wins. Max three models after dedupe; `"default"` expands to the default model. Entries outside `availableModels` are dropped; during compaction it will not fall back to a smaller-context model. Applies to subagents too (subagent continues on the accepting model).
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Fallback model chains)
- Notes: Not shown in `/status` and not confirmed at startup; the first switch notice is the only visible sign (a discoverability gap). Subagent coverage since v2.1.247.

### Automatic content-based model fallback (safety classifiers)
- What: Fable, Opus 5.5, Sonnet 5.5 and Opus 5 run safety classifiers (mostly cybersecurity and biology). Flagged requests re-run on a category-specific fallback: Fable/Opus 5.5 bio to Opus 5, cyber to Opus 4.8; Sonnet 5.5 cyber to Sonnet 5, bio refuses; Opus 5 cyber to Opus 4.8, bio refuses. The session then stays on the fallback model (unlike availability chains) and keeps the flagged request's effort level. `switchModelsOnFlag: false` / "Ask each time" prompts (switch or edit-and-retry); `-p`, SDK without prompt, and Agent-tool subagents under "ask" end with a refusal. Blocked targets (allowlist) mean no fallback. On Bedrock/Vertex/Foundry fallback requires identifiable source and target IDs (pins via `ANTHROPIC_DEFAULT_*`), and a target window at least as large as the session's.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Automatic model fallback; Effort level after a fallback; Check what triggered fallback; Ask before switching; Subagents switch or fail without asking; Enable fallback on Bedrock, Agent Platform, and Foundry; Security research and biology workloads)
- Notes: Category-based since v2.1.219. First request can trip it from CLAUDE.md/git status alone; `claude --safe-mode` disables customisations to diagnose. Page frames frequent fallback in security/bio work as expected routing. Subagent behaviour changed v2.1.318.

### Custom model options, modelPicker lineup and pinned-model capabilities
- What: `ANTHROPIC_CUSTOM_MODEL_OPTION` (+ `_NAME`, `_DESCRIPTION`) adds one unvalidated picker entry. The `modelPicker` setting (user/managed/`--settings` only, not project) lists rows with `model`, `label`, `description`, `behavesAs` (apply a known model's capabilities to a newer ID), optionally `replaceBuiltInOptions`. Rows the session cannot serve are dropped or greyed. On third-party providers, `ANTHROPIC_DEFAULT_*_MODEL_NAME/_DESCRIPTION/_SUPPORTED_CAPABILITIES` set display and declare capabilities (`effort`, `xhigh_effort`, `max_effort`, `thinking`, `adaptive_thinking`, `interleaved_thinking`); when set, unlisted capabilities are disabled. Capability detection otherwise matches model ID patterns.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Add a custom model option; Customize pinned model display and capabilities); https://code.claude.com/docs/en/settings-reference.md (modelPicker)
- Notes: `_SUPPORTED_CAPABILITIES` has no effect behind an `ANTHROPIC_BASE_URL` gateway (llm-gateway-protocol.md). `modelPicker` v2.1.242+, `behavesAs` v2.1.257+. Project scope excluded so a cloned repo cannot relabel the picker.

### modelOverrides (per-version provider ID mapping)
- What: Settings map from Anthropic model ID to provider-specific string (Bedrock application inference profile ARN, Vertex version name, Foundry deployment). Used by picker rows and by IDs passed via `--model`, `ANTHROPIC_MODEL`, `ANTHROPIC_DEFAULT_*` (since v2.1.200). Unknown keys ignored. Allowlist is evaluated against the Anthropic ID, not the override value. When `availableModels` is managed, only managed `modelOverrides` apply to directly passed IDs; enforced Default resolves only through managed overrides. Also used to teach Claude Code what model a gateway alias is (context window, capabilities) and to silence the unrecognised-model diagnostic.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Override model IDs per version); https://code.claude.com/docs/en/amazon-bedrock.md (Map each model version to an inference profile)
- Notes: Managed-only restriction prevents a user remapping an allowed ID to an arbitrary endpoint string.

### Pin models for third-party deployments; startup model checks
- What: Docs warn to pin `ANTHROPIC_DEFAULT_*_MODEL` before rollout, because built-in provider defaults lag and may not be enabled. On Bedrock and Agent Platform, Claude Code checks model access at startup: if an older pin is set and a newer default is invocable, it prompts to update the pin (writes user settings and restarts; decline remembered); if unpinned and the default is unavailable, it falls back for the session (earlier versions, then Sonnet). Refusals are remembered on the machine up to a day (`CLAUDE_CODE_SKIP_MODEL_ACCESS_MEMORY=1` off). Mid-session loss of access switches to another model for unpinned tiers (`CLAUDE_CODE_DISABLE_MODEL_ACCESS_FALLBACK=1` to fail instead). Foundry has no startup check, so requests just fail. Managed `env` pins and inference-profile ARNs are not checked or prompted.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Pin models for third-party deployments); https://code.claude.com/docs/en/amazon-bedrock.md (Startup model checks; Remembered model refusals; When a model is disabled mid-session); https://code.claude.com/docs/en/google-vertex-ai.md (Startup model checks); https://code.claude.com/docs/en/microsoft-foundry.md (4. Pin model versions)
- Notes: With `enforceAvailableModels`, startup checks use only allowlisted IDs (v2.1.287+). Managed-pin prompt bug fixed in v2.1.314.

### Background (small/fast) model per provider
- What: Background tasks such as session titles use the small/fast model (Haiku-class). On Bedrock and Agent Platform the default Sonnet is used instead because Haiku may not be enabled; selecting a primary model (or pinning only Opus) moves background tasks to that model. On Foundry it defaults to the primary model. `ANTHROPIC_DEFAULT_HAIKU_MODEL` sets it explicitly. On Bedrock `ANTHROPIC_SMALL_FAST_MODEL_AWS_REGION` can put it in another region. Through an Anthropic-format gateway the main model is used, or default Haiku with a Console key.
- Status: GA
- Evidence: https://code.claude.com/docs/en/amazon-bedrock.md (4. Pin model versions); https://code.claude.com/docs/en/google-vertex-ai.md (5. Pin model versions); https://code.claude.com/docs/en/microsoft-foundry.md (4. Pin model versions); https://code.claude.com/docs/en/llm-gateway-protocol.md (Requests and defaults by connection method)
- Notes: Pages warn unpinned Bedrock/Vertex primary bills at Opus rate.

### Fast mode
- What: Same model with a speed-prioritising API configuration, up to 2.5x faster at higher per-token cost; supported on Fable 5.2, Opus 5.5, Opus 5, Opus 4.8 only. Toggle with `/fast` (switches to Opus if current model unsupported), `fastMode: true` in user settings, or VS Code command; `↯` icon shows it. Persists across sessions by default; `fastModePerSessionOptIn: true` makes it per-session. Follows model switches (off on unsupported models). Pricing per MTok in/out: Fable 5.2 $16/$80, Opus 5.5 $8/$40, Opus 5 and 4.8 $10/$50, flat across 1M context. Enabling mid-conversation re-reads the whole context uncached (separate cache). Separate rate limits: on limit it falls back to standard speed with grey icon and re-enables after cooldown; on credit exhaustion it retries at standard speed. Only Anthropic API / subscriptions (via usage credits only), not Bedrock/Vertex/Foundry/Claude Platform on AWS. Team/Enterprise off until Owner enables.
- Status: GA for Fable 5.2 and Opus 5.5; research preview for Opus 5 and Opus 4.8; Opus 4.8 fast mode deprecated 2026-10-30, removal 2026-11-30; Opus 4.7 fast mode removed 2026-07-24.
- Evidence: https://code.claude.com/docs/en/fast-mode.md (all sections)
- Notes: Availability check calls `api.anthropic.com` directly, ignoring `ANTHROPIC_BASE_URL`; skip vars `CLAUDE_CODE_SKIP_FAST_MODE_NETWORK_ERRORS`, `CLAUDE_CODE_SKIP_FAST_MODE_ORG_CHECK`; `CLAUDE_CODE_DISABLE_FAST_MODE=1` disables. Skip vars change only the client check; the API still enforces. In `-p`, `/fast` works only if launched with fastMode in `--settings` (v2.1.205+). Cloud sessions: `/fast on` session-only (v2.1.271+).

### Prompt caching toggles
- What: Prompt caching is automatic on all providers. `DISABLE_PROMPT_CACHING=1` disables globally; per-tier `DISABLE_PROMPT_CACHING_{HAIKU,SONNET,OPUS,FABLE}`. `ENABLE_PROMPT_CACHING_1H=1` requests 1-hour TTL (billed higher) on Bedrock, Vertex, Foundry, Claude Platform on AWS.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Prompt caching configuration); https://code.claude.com/docs/en/amazon-bedrock.md (4. Pin model versions); https://code.claude.com/docs/en/third-party-integrations.md (Compare deployment options)
- Notes: Caching mechanism details live in prompt-caching.md (not in this slice).

### Amazon Bedrock provider
- What: Enabled with `CLAUDE_CODE_USE_BEDROCK=1`. Interactive setup wizard (login prompt "3rd-party platform" or `/setup-bedrock`) picks credentials (profile, Bedrock API key, access key, env), region, verifies invocable models, pins them into user settings `env`. Uses the AWS SDK default chain; `AWS_BEARER_TOKEN_BEDROCK` bypasses the chain. Resolved credentials cached in memory until 5 min before expiry (1h if no expiry); 60 s resolve timeout (`CLAUDE_CODE_AWS_CHAIN_RESOLVE_TIMEOUT_MS`). `awsAuthRefresh` runs on detected expiry (after an STS GetCallerIdentity check, via proxy); `awsCredentialExport` runs at start and on reload and must print credential JSON. Region order `AWS_REGION`, `AWS_DEFAULT_REGION`, profile region, `us-east-1`; malformed values treated as unset. Cross-region inference profile prefix derived from region (`us.`, `eu.`, `apac.`, `us-gov.`, `global.`), override `ANTHROPIC_BEDROCK_REGION_PREFIX`. Service tiers via `ANTHROPIC_BEDROCK_SERVICE_TIER` (header `X-Amzn-Bedrock-Service-Tier`). Guardrails via `ANTHROPIC_CUSTOM_HEADERS`. IAM policy given (InvokeModel, InvokeModelWithResponseStream, List/GetInferenceProfile, marketplace subscribe).
- Status: GA
- Evidence: https://code.claude.com/docs/en/amazon-bedrock.md (Sign in with Bedrock; 2. Configure AWS credentials; Credential caching and resolution timeout; Advanced credential configuration; 3. Configure Claude Code; Cross-region inference profile prefixes; IAM configuration; Service tiers; AWS Guardrails)
- Notes: WebSearch unavailable on Bedrock; `/logout` unavailable. Missing GetInferenceProfile costs one retry per new model. Streaming must keep `application/vnd.amazon.eventstream` content-type through any proxy (`CLAUDE_CODE_DISABLE_BEDROCK_CONTENT_TYPE_GUARD`, `..._DEFAULT`). `/context` zero counts fixed in v2.1.196.

### Amazon Bedrock Mantle endpoint
- What: Mantle serves Claude on Bedrock in the native Anthropic API shape. `CLAUDE_CODE_USE_MANTLE=1` plus region; separate IAM actions `bedrock-mantle:CreateInference` and `CountTokens`. Model IDs like `anthropic.claude-sonnet-5` (no version suffix). Can run alongside the Invoke API (both vars set): Mantle-format IDs route to Mantle, others to Invoke; `/status` shows "Amazon Bedrock + Amazon Bedrock (Mantle)". `anthropic.`-prefixed `availableModels` entries become picker options. Gateway mode: `CLAUDE_CODE_SKIP_MANTLE_AUTH=1` + `ANTHROPIC_BEDROCK_MANTLE_BASE_URL`.
- Status: GA (page gives no status label; model access is allowlisted by AWS account team)
- Evidence: https://code.claude.com/docs/en/amazon-bedrock.md (Use the Mantle endpoint; Mantle endpoint errors); https://code.claude.com/docs/en/model-config.md (Mantle model IDs)
- Notes: Service tier header has no effect on Mantle.

### Google Cloud's Agent Platform (formerly Vertex AI) provider
- What: `CLAUDE_CODE_USE_VERTEX=1`, `CLOUD_ML_REGION` (global, multi-region `eu`/`us`, or region; fallback `us-east5`), `ANTHROPIC_VERTEX_PROJECT_ID` (always wins over other project sources). Wizard via login prompt or `/setup-vertex`. Standard ADC, including X.509 Workload Identity Federation. `gcpAuthRefresh` command (5 s pre-check, 3 min timeout; in project settings runs under the folder-trust rule). Per-model region overrides `VERTEX_REGION_CLAUDE_*`. Same startup model checks, remembered refusals and mid-session fallback as Bedrock. MCP tool search on by default only for Opus/Sonnet/Haiku 4.5 and later. IAM: `roles/aiplatform.user` (`aiplatform.endpoints.predict`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/google-vertex-ai.md (all sections)
- Notes: Login prompt still labels it "Google Vertex AI". Model access approval may take 24-48 h (page claim). Dated `@` IDs must be written with the date in allowlists.

### Microsoft Foundry provider
- What: `CLAUDE_CODE_USE_FOUNDRY=1` plus `ANTHROPIC_FOUNDRY_RESOURCE` (name only; URLs refused) or `ANTHROPIC_FOUNDRY_BASE_URL`. Auth: `ANTHROPIC_FOUNDRY_API_KEY`, Entra ID via Azure default credential chain, or `ANTHROPIC_FOUNDRY_AUTH_TOKEN` bearer (takes precedence). No setup wizard and no startup model check, so env vars are the only path and unpinned unavailable defaults simply fail. Model variables are deployment names; 1M window only when the deployment name matches a model ID or `modelOverrides` maps it. RBAC roles Azure AI User / Cognitive Services User.
- Status: GA
- Evidence: https://code.claude.com/docs/en/microsoft-foundry.md (all sections)
- Notes: Default `default` model on Foundry is Sonnet 4.5; `opus` alias Opus 4.6 (model-config.md).

### Claude Platform on AWS provider
- What: The Anthropic-operated Claude API with AWS auth, IAM and Marketplace billing; same models and features as the Claude API on the same schedule. `CLAUDE_CODE_USE_ANTHROPIC_AWS=1`, required `ANTHROPIC_AWS_WORKSPACE_ID` (sent as `anthropic-workspace-id`), region; base URL `https://aws-external-anthropic.{region}.api.aws` (override `ANTHROPIC_AWS_BASE_URL`). Auth by SigV4 from the AWS chain or `ANTHROPIC_AWS_API_KEY` (x-api-key, takes precedence). `awsAuthRefresh` supported, plus a `/login` refresh option. Gateway mode `CLAUDE_CODE_SKIP_ANTHROPIC_AWS_AUTH=1`. Bedrock and Foundry take precedence in provider routing.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-platform-on-aws.md (all sections)
- Notes: Feature-flag-gated client features are off by default here and the advisor tool is unavailable. Marketplace subscription creates a separate Anthropic organization; credentials do not transfer.

### LLM gateway: connecting a client
- What: Point `ANTHROPIC_BASE_URL` at the gateway and set one credential: `ANTHROPIC_AUTH_TOKEN` (Authorization: Bearer), `ANTHROPIC_API_KEY` (x-api-key, needs one-time interactive approval), or `apiKeyHelper` (command; output cached 5 min, `CLAUDE_CODE_API_KEY_HELPER_TTL_MS`; sent in both headers; must print only the key from v2.1.227). Docs give a curl verification request and `/status` lines to check. Settings-file `env` beats shell export. Provider-format routes: `ANTHROPIC_BEDROCK_BASE_URL` + `CLAUDE_CODE_SKIP_BEDROCK_AUTH`, `ANTHROPIC_VERTEX_BASE_URL` + `CLAUDE_CODE_SKIP_VERTEX_AUTH`, `ANTHROPIC_FOUNDRY_BASE_URL`, `ANTHROPIC_AWS_BASE_URL`. Extra headers via `ANTHROPIC_CUSTOM_HEADERS`. Per-surface notes: VS Code `claudeCode.environmentVariables`; GitHub Actions; Agent SDK `env` (TS replaces env, Python merges). Slack/cloud sessions do not use the gateway; Remote Control and voice dictation are unavailable with a gateway credential.
- Status: GA
- Evidence: https://code.claude.com/docs/en/llm-gateway-connect.md (all sections)
- Notes: Large troubleshooting table: WAF XSS body rules causing 403, rewritten too-long errors breaking compaction, fast-mode check failures, auth-conflict warnings, `forceLoginMethod` incompatibility with gateway credentials. `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` also disables auto-updates.

### LLM gateway protocol requirements (compatibility guide)
- What: Supported client formats: Anthropic Messages (`/v1/messages`, optional `/v1/messages/count_tokens`), Bedrock InvokeModel (invoke, invoke-with-response-stream, count-tokens), Agent Platform rawPredict/streamRawPredict; Foundry and Claude Platform on AWS use Messages. Must forward unchanged: `anthropic-version`, `anthropic-beta` (verbatim, never allowlisted; carries the OAuth capability for subscription logins), `anthropic-workspace-id` for Claude Platform on AWS; Bedrock-format equivalents in body fields. Gateway may consume `x-claude-code-session-id`, `x-claude-code-agent-id`, `x-claude-code-parent-agent-id`. Must stream event-by-event (no buffering), pass full event sequence through `message_stop`, keep SSE `ping`s (idle watchdog). Response headers: `content-type`, integer-seconds `retry-after` (above 60 s stops retries), `x-should-retry`, `anthropic-ratelimit-unified-*`. Forward error bodies unmodified so client recovery can match wording, or use `capability_rejected:` tokens. Treat headers/body fields as open lists. Startup probes (`HEAD /api/hello`, Bedrock inference-profile GETs) may be rejected.
- Status: GA
- Evidence: https://code.claude.com/docs/en/llm-gateway-protocol.md (API formats; Optional endpoints and startup traffic; Streaming; Request headers; Forward as open lists; Response headers)
- Notes: Gateway sessions are not HIPAA-eligible. Format mismatch (Messages client, Bedrock upstream) is the gateway's job to bridge. The apps gateway serves its own `GET /protocol` reference.

### Gateway client behaviour by connection method; feature pass-through and auto-retry
- What: Bedrock/Vertex format sends provider IDs and only the beta subset those providers accept; Messages format sends the full set and treats unknown IDs as current models (adaptive thinking, effort, context management); apps gateway sign-in sends the cloud-safe subset. Header/body pairs table: context management, beta tool fields (`strict`, `defer_loading`), `output_config` (effort, structured outputs, task budget), mid-conversation `role: "system"` messages, per-turn effort, `cache_control` (stripping it silently bills uncached), token counting (fallback to char estimate). Client auto-retries with the rejected part removed for: thinking type, system-role messages, cache_control placement, thinking signatures, advisor tool, effort, context_management, named beta values, tool search; with stated durations for how long a value stays omitted (until exit, per model, or per conversation). `CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS=1` stops pre-release capabilities (exact in/out lists given).
- Status: GA
- Evidence: https://code.claude.com/docs/en/llm-gateway-protocol.md (How the connection method changes client behavior; Feature pass-through; Automatic retry and error forwarding; Retry without a rejected beta value; How long a rejected beta value stays out; Retry without tool search; Disable pre-release capabilities)
- Notes: Many of these retries are recent (v2.1.280-v2.1.314). The page is an unusually candid admission that third-party gateways break with each release.

### Gateway hint headers
- What: Optional per-request routing hints (v2.1.273+): `x-claude-code-request-class` (main/subagent/workflow/compaction/auxiliary), `x-claude-code-agent-type` (built-in names, `custom`, `teammate`, `fork`; never user-chosen names), `x-claude-code-compaction` (auto/manual/reactive), `x-claude-code-context-compacted` (first request after compaction, signal to drop prefix cache), `x-claude-code-prev-tool-durations` (`Name=ms;...`, max 32 entries/4 KB, percent-encoded), `x-claude-code-prompt-id` (random UUID per user prompt, v2.1.283+). Sent by default only to the direct Anthropic API; behind a custom base URL or other providers requires `CLAUDE_CODE_GATEWAY_HINT_HEADERS=1`; `0` disables everywhere. Values printable ASCII, no prompt text.
- Status: GA
- Evidence: https://code.claude.com/docs/en/llm-gateway-protocol.md (Gateway hint headers)
- Notes: Off by default behind proxies because some reject unknown headers.

### System prompt attribution block
- What: Claude Code prepends an attribution block (client version plus conversation-derived fingerprint) as the first system block. `api.anthropic.com` strips it positionally; any other upstream sees it in the prompt. Gateways must keep the `system` array unchanged and the block as its own first entry, or set `CLAUDE_CODE_ATTRIBUTION_HEADER=0`. Stable per conversation through custom base URLs since v2.1.181 (previously changed per request, defeating gateway caches). Kept on auto-mode classifier requests to `api.anthropic.com` even when set to 0.
- Status: GA
- Evidence: https://code.claude.com/docs/en/llm-gateway-protocol.md (System prompt attribution block)
- Notes: Page states the variable is for caching compatibility, not a privacy control.

### Gateway model discovery
- What: With `CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1` and a Messages-format `ANTHROPIC_BASE_URL`, Claude Code calls `GET /v1/models?limit=1000` at startup (3 s timeout, `CLAUDE_CODE_GATEWAY_MODEL_DISCOVERY_TIMEOUT_MS`; any redirect is treated as failure so credentials cannot leak). Keeps entries whose `id` contains `claude` or `anthropic`; uses `display_name`/`description`; filtered by `availableModels`; deduplicated against alias rows. Cached at `~/.claude/cache/gateway-models.json`, falls back to cache or built-in list. Off by default so a shared-key gateway does not expose every model to every user. Does not run when any `CLAUDE_CODE_USE_*` is set.
- Status: GA
- Evidence: https://code.claude.com/docs/en/llm-gateway-protocol.md (Model discovery); https://code.claude.com/docs/en/llm-gateway-connect.md (Add gateway models to the model picker)
- Notes: Sends both Authorization and x-api-key (v2.1.248+).

### Gateways overview, Claude apps gateway, subscriptions
- What: A gateway holds the provider credential; developers hold per-developer gateway credentials. Claude Code ships its own self-hosted gateway ("Claude apps gateway") in the `claude` binary: IdP sign-in via `/login`, per-IdP-group model access and managed settings, OTLP usage metrics, released in lockstep with the CLI so no forwarding list to maintain; no service-token flow, so CI cannot use it. With a gateway credential, subscription limits do not apply and usage bills per token to the credential owner; with only `ANTHROPIC_BASE_URL` set, the claude.ai login remains active and subscription billing applies. Version checks and downloads still go direct to Anthropic.
- Status: GA (page gives no label)
- Evidence: https://code.claude.com/docs/en/gateways.md (all sections); https://code.claude.com/docs/en/llm-gateway.md (What a gateway provides; Subscriptions and gateways)
- Notes: Apps gateway details live in claude-apps-gateway*.md, not in this slice. Anthropic does not support routing to non-Claude models through any gateway.

### Deployment option comparison
- What: Table comparing Teams/Enterprise, Console, Bedrock, Claude Platform on AWS, Agent Platform, Foundry on billing, regions, prompt caching (on by default everywhere), auth, cost tracking (dashboards vs cloud cost tools), whether Claude on web is included, and enterprise features (IAM/CloudTrail, Cloud Audit Logs, Azure Monitor). Distinguishes corporate proxy (`HTTPS_PROXY`) from LLM gateway (base URL vars). For Bedrock and Vertex, `claude` login offers a "3rd-party platform" wizard.
- Status: GA
- Evidence: https://code.claude.com/docs/en/third-party-integrations.md (Compare deployment options; Configure proxies and gateways)
- Notes: Page is partly sales collateral ("Talk to sales"). CMEK does not cover operational telemetry on gateway sessions; `DISABLE_TELEMETRY` via managed settings.

### Model allowlists and locks
- What: A managed `model` is only a starting default; the locks are `availableModels` (constrains `/model`, `--model`, `model` in user files, subagents, skills, advisor; prefix match by default so `claude-opus-5` also allows Opus 5.5), `availableModelsMatch: "exact"` (managed-only, v2.1.283+), `deniedModels` (managed-only, v2.1.283+, blocks even if allowed), and `enforceAvailableModels` (remaps the Default choice into the allowlist, v2.1.175+). `maxEffortLevel` caps effort; lowest cap wins.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-reference.md (availableModels; availableModelsMatch; deniedModels; enforceAvailableModels); https://code.claude.com/docs/en/managed-settings.md (What a developer can change)
- Notes: The managed `autoCompactWindow` is also only a default, overridable per session.

### Provider allowlist with endpoint pinning
- What: `allowedProviders` (managed, v2.1.285+) lists permitted providers (`anthropic`, `bedrock`, `vertex`, `foundry`, `anthropicAws`, `mantle`, `customEndpoint`, `gateway`); a session on an unlisted provider is refused at startup, login and at next API contact, so switching mid-session is refused too. `customEndpoint` and off-service Bedrock endpoints are admitted only if the session's endpoint value equals a value pinned in a managed `env` block. A machine-level list and a server-managed list intersect (server can narrow, never widen). An empty or wholly invalid list refuses every provider.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-reference.md (allowedProviders; Endpoints that need a pin in managed env)
- Notes: The list does not judge credentials, proxies or certificates.

### Feature availability by provider
- What: CLI, Agent SDK, IDE extensions, subagents, hooks, skills, plugins, MCP, checkpoints, sandboxing, workflows, OTel and the managed settings file work on every provider. Subscription-only: cloud sessions and mobile, Slack, Desktop, cloud scheduled tasks, Ultrareview, Code Review (Team/Enterprise), Remote Control, Chrome extension, computer use, artifacts, voice. Varies: web search (not Bedrock), fast mode (subscription/Console only), auto mode (third-party clouds only on newer models), advisor and channels (subscription/Console only), server-managed settings and analytics dashboard (not on Bedrock/Vertex/Foundry/Claude Platform on AWS). Any `ANTHROPIC_BASE_URL` not pointing at api.anthropic.com turns off Remote Control and server-managed settings regardless of what the gateway forwards. Telemetry and error reporting are off by default on third-party clouds.
- Status: GA
- Evidence: https://code.claude.com/docs/en/feature-availability.md (Availability by model provider; Summary by provider)
- Notes: Cross-session messaging is same-machine only on API-key and cloud-provider auth.

### Provider choice and provider/model/effort restrictions for orgs
- What: Five provider options (Claude Teams/Enterprise default recommendation, Console pay-as-you-go, Bedrock, Google Agent Platform, Microsoft Foundry). `allowedProviders` refuses sessions on unlisted providers at startup, login and next API contact (v2.1.285+). `availableModels` + `enforceAvailableModels` restrict picker and default model; `maxEffortLevel` caps effort per model on every provider. Enterprise orgs on claude.ai/API also get server-side org model restrictions, org default model, and per-role effort limits, which do not reach Bedrock/Vertex/Foundry/Claude Platform on AWS sessions.
- Status: GA
- Evidence: https://code.claude.com/docs/en/admin-setup.md (Choose your API provider; Decide what to enforce)
- Notes: Cloud sessions, cloud routines, Code Review, Remote Control and the Chrome extension need a claude.ai account; unavailable on Console keys or cloud credentials alone.

### Gateway upstreams, model routing and failover
- What: Ordered `upstreams` list (anthropic, bedrock, mantle v2.1.283+, anthropicAws v2.1.198+, vertex, foundry), distinct `name` per duplicate provider. Failover on 5xx, 429, 401, 403, 404 (v2.1.198+), 501 and timeout; other 4xx returned. No circuit breaker: a down upstream is retried by every request. Built-in catalog auto-translates model IDs (`auto_include_builtin_models`); `models:` maps IDs per upstream for non-US Bedrock, provisioned-throughput ARNs, Foundry deployment names, and custom IDs that scope routing. Auth per provider prefers workload identity (AWS default chain/IRSA, ADC/Workload Identity, DefaultAzureCredential); Anthropic supports API key, OAuth bearer, or Workload Identity Federation. Static `headers:` per upstream (v2.1.277+, reserved names refused). `forward_user_identity` adds `x-claude-gateway-user-id/email` and `x-litellm-end-user-id` headers for a proxy you run (refused when base_url is Anthropic API); that proxy's 429s are not failed over.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-config.md (upstreams; Multiple upstreams; Static headers on upstream requests; Per-user identity headers)
- Notes: Docs warn failover across providers changes the governing agreement and geography. Cloud-upstream error text (account IDs, ARNs) is logged but replaced for developers with generic copy or `capability_rejected:` tokens (v2.1.233+), which Claude Code treats like "prompt is too long" for auto-compaction. `x-claude-code-*` headers are never passed upstream. `anthropic-beta` values forwarded; moved into body `anthropic_beta` for Bedrock.

### Bedrock-specific gateway features: guardrails, assume_role, cost attribution
- What: Per-upstream Bedrock `guardrail` (id, version) applied to every request (v2.1.281+), all-or-none across bedrock upstreams, refuses mixing with Mantle; `input_tagging` (v2.1.316+: last_message, user_messages, all_messages, all_messages_and_system); `guardrail_trace` (v2.1.314+) writes `guardrail.trace` audit events without matched text and strips trace from replies. `assume_role` signs via STS one-hour credentials, cross-account; `session_name: email|sub` gives per-developer AWS sessions (one STS call per developer per hour per replica) for IAM-principal cost allocation. Per-team attribution alternative: tagged application inference profiles mapped to team-specific model IDs pinned by policy.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-config.md (Apply an Amazon Bedrock guardrail; Add guardrail input tags; Log which guardrail filter fired; Bedrock in another AWS account; Per-developer AWS cost attribution); https://code.claude.com/docs/en/claude-apps-gateway-on-aws.md (Cost attribution)
- Notes: Docs state plainly that guardrails do not evaluate tool results, tool inputs, images or untagged parts, and that a deliberate user can place text where no tag goes.

### Gateway session feature differences
- What: Available: inference forwarding, 1M context default for Fable, Sonnet 5+, Opus 4.7+ (v2.1.287+ clients, compacts about 967K), standard prompt caching (5-minute TTL default; 1-hour via `promptCacheTtl`/`subagentPromptCacheTtl` policy keys, fails if any upstream rejects it), auto mode under third-party provider rules. Not available: server-side WebSearch (disabled since CLI cannot verify upstream support), Remote Control, `/design-sync`, feature-flag-dependent features like `/import`, first-party optimizations (global cache scope, token-efficient tools).
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway.md (Availability and limitations); https://code.claude.com/docs/en/claude-apps-gateway-config.md (Context window in terminal sessions; One-hour prompt cache)

### Models, providers and inference options via the SDK
- What: `model` (alias or full ID), `fallbackModel` chain, `effort` (`low`..`max`, `xhigh` on supported models; per subagent too), `thinking` (adaptive by default on supported models; `display` controls receiving thinking text), alpha `taskBudget` (API-side token budget told to the model). Providers via env: `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_ANTHROPIC_AWS` + `ANTHROPIC_AWS_WORKSPACE_ID`, `CLAUDE_CODE_USE_VERTEX`, `CLAUDE_CODE_USE_FOUNDRY`, or `ANTHROPIC_BASE_URL` gateways. Refusal fallback messages (`model_refusal_fallback`).
- Status: GA (`taskBudget` alpha)
- Evidence: https://code.claude.com/docs/en/agent-sdk/agent-loop.md (Effort level); https://code.claude.com/docs/en/agent-sdk/configuration.md (Choose a model); https://code.claude.com/docs/en/agent-sdk/quickstart.md (Set your API key); https://code.claude.com/docs/en/agent-sdk/typescript.md (Options; SDKMessage)
- Notes: Per-provider caveats appear in tool search (Vertex, Foundry) and prompt-cache splitting pages.

### Model releases and defaults (last 5 months)
- What (default changes and prices as the changelog states them):
  - Fable 5 (2.1.170, Jun 9). Sonnet 5 (2.1.197, Jun 30). Opus 5 (2.1.219, Jul 24).
  - Fable 5.1 (2.1.257, Sep 1; $10/$50).
  - Opus 5.5 (2.1.280, Sep 22; default Opus; 1M context; $4/$20).
  - Sonnet 5.5 (2.1.284, Sep 28; $2/$10).
  - Haiku 5.5 (2.1.293, Oct 7; $0.10/$0.50, $0.50/$2.50 over 100K).
  - Fable 5.2 (2.1.321, Nov 9; default Fable; $8/$40). Saved Fable 5.1 picks migrate to `fable`.
  - 1M context is the default for Opus 4.7+ and Fable on Bedrock, Vertex, Foundry and the gateway (2.1.287; `CLAUDE_CODE_DISABLE_1M_CONTEXT`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.170, 2.1.197, 2.1.219, 2.1.257, 2.1.280, 2.1.284, 2.1.287, 2.1.293, 2.1.321); https://code.claude.com/docs/en/whats-new/2026-w27.md, https://code.claude.com/docs/en/whats-new/2026-w30.md, https://code.claude.com/docs/en/whats-new/2026-w36.md
- Notes: the Opus 4 and 4.1 removal is in the Timeline.

### Model governance settings
- What:
  - `modelPicker` (2.1.243). `ANTHROPIC_DEFAULT_MODEL` (2.1.236). `fallbackModel` with up to 3 entries (Week 24).
  - `maxEffortLevel`, top-level or per model (2.1.267).
  - `availableModelsMatch: "exact"` (2.1.283). `deniedModels` (2.1.283).
  - `allowedProviders` managed setting (2.1.285).
  - `modelPricing` with a multiplier up to 10 for chargeback (2.1.243, 2.1.271).
  - Deprecated-model warnings (2.1.183).
  - `CLAUDE_CODE_DISABLE_ULTRATHINK` (2.1.316).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.183, 2.1.236, 2.1.243, 2.1.267, 2.1.271, 2.1.283, 2.1.285, 2.1.316); https://code.claude.com/docs/en/whats-new/2026-w24.md, https://code.claude.com/docs/en/whats-new/2026-w35.md, https://code.claude.com/docs/en/whats-new/2026-w37.md
- Notes: `CLAUDE_CODE_DISABLE_ULTRATHINK` has no docs-page mention in the dump.

### Fast mode lifecycle
- What:
  - Opus 4.6 → 4.7 (Week 20) → 4.8 (Week 22) → Opus 5 at $10/$50 (2.1.219). Opus 4.7 support removed in 2.1.219.
  - Opus 4.8 fast mode deprecated Oct 30, removal Nov 30, 2026 (2.1.316); after that, requests error with no fallback.
  - The "(research preview)" label was dropped for Opus 5.5 (2.1.304).
  - Vertex fast mode in global, us and eu (2.1.322).
  - In Remote sessions (2.1.271).
- Status: GA on Fable 5.2 and Opus 5.5; research preview on Opus 5 and 4.8 (per fast-mode.md)
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.219, 2.1.271, 2.1.304, 2.1.316, 2.1.322); https://code.claude.com/docs/en/fast-mode.md (top notes, Requirements)

### Advisor
- What: `/advisor` with a text form for headless and Remote Control (2.1.260). Available on Claude Platform on AWS, where no advisor runs until one is picked (`advisorModel`, `--advisor`; `CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1`; 2.1.300).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.260, 2.1.300); own page advisor.md

## Area 15. UI surfaces

### TUI keyboard model and input features
- What: General controls include `Ctrl+C` (interrupt / clear / exit on second press), `Ctrl+D` (exit with 800 ms confirm), `Ctrl+G` or `Ctrl+X Ctrl+E` (edit prompt in `$EDITOR`, optionally with last reply as commented context), `Ctrl+L` redraw, `Ctrl+O` transcript viewer (per-message timestamp and model; expands collapsed MCP-call and cross-session lines), `Ctrl+R` history search, `Ctrl+V`/`Alt+V` image paste as `[Image #N]` chip, `Ctrl+B` background, `Ctrl+T` task checklist, `Ctrl+S` stash/restore prompt, `Ctrl+Z` suspend, `Shift+Tab` cycle permission modes (Manual, acceptEdits, plan, bypass if enabled, auto), `Alt+P` switch model, `Alt+T` extended thinking, `Alt+O` fast mode, `Tab` on a permission option opens a comment field. Readline editing (`Ctrl+A/E/K/U/W/Y`, `Alt+B/F/D/Y`, `Ctrl+_` undo; readline word rules since v2.1.261, not remappable). Multiline via `\`+Enter, Option+Enter, Shift+Enter, `Ctrl+J`, or paste. Prefixes: `/` command, `!` shell, `@` file mention (and live session names with cross-session messaging, v2.1.232+), `:` emoji, `?` help. Mid-prompt `/` completion inserts but does not run commands.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Keyboard shortcuts; Text editing; Multiline input; Quick commands; Complete a command mid-prompt)
- Notes: Extended thinking toggle has no effect on Opus 5.5, Sonnet 5.5, Haiku 5.5 or Fable models, which always think.

### Vim editor mode
- What: `/config` -> Editor mode. NORMAL/INSERT/VISUAL modes with motions (`hjkl`, `w/e/b`, `0/$/^`, `gg/G`, `f/F/t/T`, `;`, `,`, `%` v2.1.301+), operators and text objects (`iw`, `aW`, quotes, brackets), visual char/line mode (no block mode), `.` repeat, `u` undo. `vimInsertModeRemaps` maps a two-char INSERT sequence (e.g. `jj`) to Esc within a 1-second window; read only from user, `--settings` and managed settings so a checked-out repo cannot remap keystrokes.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Vim editor mode; Remap INSERT-mode key sequences)
- Notes: `/vim` command was removed in v2.1.92.

### Prompt history, draft recovery and reverse search
- What: Input history is stored per working directory and Up recalls prompts from past sessions of the same project; duplicates collapse; recalled prompts with pasted text resend the full paste. Up recovers a draft cleared with `Ctrl+C` (immediately v2.1.288+; last ten cleared drafts with pastes and images per session v2.1.292+). `Ctrl+R` search: classic renderer searches all projects inline; fullscreen opens a dialog with preview and `Ctrl+S` to cycle scope session/project/all. `!` history expansion off by default.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Command history; Bring back a cleared draft; Reverse search with Ctrl+R)
- Notes: none.

### Prompt suggestions and "You should know" side agent
- What: A grayed example prompt is chosen from recent git history at session start; after each response a next-prompt suggestion is generated by a short background request to the session's model (mostly cache reads). Skipped when cache is cold, after errors, in plan mode, near usage limits, in teammate sessions, and when many go unused. Off by default where feature flags are not fetched. Controls: `/config`, `promptSuggestionEnabled`, `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION` (env wins); `-p --prompt-suggestions` emits `prompt_suggestion` events. "You should know" (built-in mod, off by default, v2.1.287+) runs a tool-less side agent that periodically rereads the conversation and surfaces one missed tradeoff or suspect result as a note; the main agent never sees notes unless the user forwards them; known topics stored under `~/.claude/plugins/store/`.
- Status: GA (You should know: off by default)
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Prompt suggestions; When Claude Code skips suggestions; You should know suggestions; Cost of checks)
- Notes: Each "You should know" check bills several thousand instruction tokens plus reply in full.

### Session recap
- What: After at least three minutes since the last completed turn with the terminal unfocused, and at least three turns, a one-line recap is generated in the background so it is ready on return; never twice in a row; capped at 400 chars; `/recap` on demand; off in `-p`. Refuses to run when `/recap` arrives via a relayed Slack/Teams/thread message or a cloud scheduled task prompt.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Session recap)
- Notes: Toggle in `/config`.

### Spell checking in the prompt
- What: Off by default (v2.1.235+); uses the first of aspell, hunspell, ispell on PATH; underlines unknown words after a pause, skipping code-like tokens, paths, flags, CJK and some SE Asian scripts; never edits text. `spellcheck` read only from managed, `--settings`, user settings (in that order, not merged), never project files. Stops after two checker failures or three 15-second timeouts.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Check spelling as you type)
- Notes: Disabled in screen-reader mode, shell mode, history search and dictation.

### Fullscreen (alt-screen) renderer
- What: Alternative TUI renderer drawn on the terminal's alternate screen buffer (like vim/htop) that renders only visible messages, so memory stays flat in long conversations and flicker drops; input box fixed at the bottom. Adds mouse support (click to place cursor, pick menu items, expand collapsed tool results, Cmd/Ctrl-click URLs and file paths, drag-select with copy-on-release via pbcopy/wl-copy/xclip/Set-Clipboard, tmux buffer, or OSC 52 over SSH), in-app scrolling with auto-follow and a `Jump to bottom` button, transcript mode with less-style search (`/`, `n/N`, `{`/`}`), `[` to dump the conversation into native scrollback, `v` to open it in `$EDITOR`. `/tui fullscreen|default` saves the `tui` setting and relaunches with the conversation intact (carrying permission mode, effort, model, allowed/disallowed tools, agent and system-prompt-append flags); it refuses to relaunch when a `--system-prompt` replacement, `--tools` allowlist, `--setting-sources` or session-only deny/ask rules could not be passed to the new process. `CLAUDE_CODE_NO_FLICKER`, `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN`, `CLAUDE_CODE_DISABLE_MOUSE`, `CLAUDE_CODE_DISABLE_MOUSE_CLICKS`, `CLAUDE_CODE_SCROLL_SPEED` (up to 20), `CLAUDE_CODE_ALT_SCREEN_FULL_REPAINT` (ConPTY fix).
- Status: research preview
- Evidence: https://code.claude.com/docs/en/fullscreen.md (intro; Enable fullscreen rendering; What changes; Use the mouse; Search and review the conversation; Keep native text selection; Research preview)
- Notes: Default renderer depends on a decision table (env vars, failed starts, iTerm2 `tmux -CC` or SSH-to-Windows forces classic, saved setting, feature-flag fetching, first use on or after May 6, 2026 = fullscreen). Two failed fullscreen starts (no first frame plus 10 s or clean exit) pin classic until update. Attached background sessions always render fullscreen. UNC paths are rendered unlinked to avoid leaking Windows credentials.

### Terminal integration: newlines, notifications, status, tmux
- What: Newline via `Ctrl+J` or `\`+Enter everywhere; Shift+Enter native in Ghostty, Kitty, iTerm2, WezTerm, Warp, Apple Terminal, Windows Terminal and kitty-protocol terminals (v2.1.269+); `/terminal-setup` writes a keybinding for VS Code, Cursor, Devin Desktop, older Alacritty and Zed (Zed keymap backed up then merged; also disables VS Code GPU acceleration). Notifications when Claude finishes or waits on a prompt while the user appears away: desktop notification in Ghostty/Kitty/iTerm2, bell or nothing elsewhere; `preferredNotifChannel` (`terminal_bell`, `iterm2`, `kitty`, `ghostty`, useful over SSH); Notification hooks run alongside. OSC 7501 Program Status Protocol reports working/waiting/done per session (v2.1.295+; not inside tmux or background sessions). tmux needs `allow-passthrough`, `focus-events`, `extended-keys`. `maxProseWidth` caps prose width. `CLAUDE_CODE_FORCE_SYNC_OUTPUT=1` for unflagged synchronized-output terminals.
- Status: GA
- Evidence: https://code.claude.com/docs/en/terminal-config.md (Enter multiline prompts; Get a terminal bell or notification; See session status in your terminal; Configure tmux; Cap response width)
- Notes: Windows `^H` Backspace handling via `CLAUDE_CODE_BS_AS_CTRL_BACKSPACE`.

### Voice dictation (CLI)
- What: `/voice [hold|tap|off]` enables live speech-to-text into the prompt. Audio streams to Anthropic servers (not local); requires a claude.ai sign-in (not API key, Bedrock, Vertex, Foundry) and a local microphone (not cloud or SSH; WSL needs WSLg). Hold mode (default) detects a held Space via key-repeat with a short warmup; tap mode toggles and auto-submits transcripts of at least three words; recording stops after 15 s silence or 2 min. `autoSubmit` option for hold mode. Vocabulary tuned for coding, with project and branch names added as recognition hints. 20 languages via the `language` setting (falls back to English). Native audio module, Linux fallback to `arecord` or SoX `rec`. Rebind via `voice:pushToTalk`. Pauses after three failures in 10 s.
- Status: GA
- Evidence: https://code.claude.com/docs/en/voice-dictation.md (Requirements; Enable voice dictation; Hold to record; Tap to record and send; Change the dictation language; Rebind the dictation key; Troubleshooting)
- Notes: Transcription does not consume tokens or count toward `/usage` limits. Admin policy can disable it. Also works in agent view dispatch/peek inputs (hold mode).

### Screen reader mode and accessibility settings
- What: `--ax-screen-reader`, `CLAUDE_AX_SCREEN_READER`, or `axScreenReader` (precedence in that order) switch the TUI to flat linear text: no box drawing, no color-only cues, static spinners, tables as `Header: value`, diffs as `+`/`-` lines, labeled lines (`you:`, `claude:`, `thinking:`, `tool:`, `tool error:`, `error:`, `warning:`, `Permission Required:`, `Cost:`), menus as numbered lists, yes/no as typed `y`/`n`, terminal bell when a reply finishes, a prompt needs an answer, or a tool over 5 s completes; OSC 133 markers at turn boundaries; only changed characters echoed while typing; deleted text announced. Project settings cannot turn it on (ignored with a startup line). Other options: `CLAUDE_CODE_ACCESSIBILITY` (visible cursor for magnifiers), `prefersReducedMotion`, daltonized themes, `CLAUDE_AX_STARTUP_QUIET_MS`, `CLAUDE_AX_PREPARK_MS`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/accessibility.md (Turn on screen reader mode; Project settings can't turn on screen reader mode; What your screen reader hears; Answer menus and prompts; Hear when Claude Code needs you; Known limitations)
- Notes: Not auto-detected; `/plan` mode changes are not announced; attached background sessions still use the alt screen.

### Surfaces and where they differ
- What: Same engine across CLI, Desktop, VS Code, JetBrains (plugin runs the CLI in the IDE terminal), web (cloud sessions), mobile (thin client into cloud or Remote Control sessions). The CLI is called the most complete surface; scripting and the Agent SDK are CLI-only; computer use from the CLI is macOS-only on Pro/Max/Ultra. Configuration, project memory and MCP servers are shared across local surfaces; cloud sessions start from a fresh clone and do not read local `~/.claude`. Integrations: Chrome, GitHub Actions, GitLab CI/CD, Code Review, Slack (per-user, Pro/Max/Ultra), Claude Tag (shared org identity, Team/Enterprise). Desktop: diff viewer, app preview, Dispatch (one line; surveyed elsewhere).
- Status: GA (Desktop on Linux is beta per overview)
- Evidence: https://code.claude.com/docs/en/platforms.md (Where to run Claude Code; Connect your tools; Work when you are away from your terminal); https://code.claude.com/docs/en/overview.md (Get started; Use Claude Code everywhere); https://code.claude.com/docs/en/glossary.md (Surface)
- Notes: JetBrains Marketplace plugin slug includes "beta" (overview link), but no status is stated in text.

### Status line
- What: `statusLine: {type: "command", command, padding, refreshInterval, hideVimModeIndicator}` runs a shell command with session JSON on stdin and renders stdout (multiple lines, ANSI colours, OSC 8 links) in its own row above the footer. Runs at session start/resume, on new assistant message, after `/compact`, permission-mode change, vim toggle, command change, `refreshInterval` timer (min 1 s), rate-limit reset time, and prompt-cache expiry time. Debounced 300 ms; a new trigger cancels the in-flight script; output shown only on exit 0. `COLUMNS`/`LINES` env give terminal size. `/statusline <description>` generates the script.
- Status: GA
- Evidence: https://code.claude.com/docs/en/statusline.md (Set up a status line; How status lines work; When the status line updates; What your script can output; Script errors or hangs)
- Notes: Runs locally, no tokens. Gated by workspace trust, `disableAllHooks` and `allowManagedHooksOnly` like hooks. Hides most footer key hints when configured.

### Status line input JSON
- What: Fields: `model.id`/`display_name`; `cwd`, `workspace.current_dir`, `project_dir`, `added_dirs`, `git_worktree`, `repo.{host,owner,name}`; `cost.total_cost_usd` (client-side estimate), `total_duration_ms`, `total_api_duration_ms`, `total_lines_added/removed`; `context_window.{total_input_tokens,total_output_tokens,context_window_size,used_percentage,remaining_percentage,current_usage}` (used_percentage is input-only: input + cache creation + cache read); `exceeds_200k_tokens`; `fast_mode`; `effort.level`; `thinking.enabled`; `rate_limits.{five_hour,seven_day,spend_limit}` (subscribers or gateway spend limits); `prompt_cache` {warm, caching_observed, ttl, expires_at, requests, misses, expected_rebuilds, hit_ratio, cache_write_tokens, miss_recache_tokens, last_miss_at, last_miss_cause, miss_causes, recache_tokens_if_cold}; `session_id`, `session_name`, `prompt_id`, `transcript_path`, `version`, `output_style.name`, `vim.mode`, `agent.name`, `pr.{number,url,review_state,kind}`, `worktree.{name,path,branch,original_cwd,original_branch}`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/statusline.md (Available data; Context window fields; Spend limit fields; Prompt cache fields; Last miss cause)
- Notes: Many fields optional or null early (current_usage null after `/compact`). Cache-miss definition: re-processed more than 5% and at least 2,000 tokens of cacheable content with no compaction or tool-result clearing to explain it. Miss causes include `tools_changed`, `system_prompt_changed`, `ttl_expired_5m`, `likely_server_side` (v2.1.260+). `prompt_cache` v2.1.251+.

### Subagent status lines
- What: `subagentStatusLine` command receives all visible subagent rows in one JSON (base hook fields, `columns`, `tasks[]` with id, name, type, agentType, status, description, label, startTime, model, effort, contextWindowSize, tokenCount, tokenSamples (last 16), cwd) and writes one JSON line `{id, content}` per row to override; empty content hides a row.
- Status: GA
- Evidence: https://code.claude.com/docs/en/statusline.md (Subagent status lines; Task fields)
- Notes: Plugin-supplied values do not run under `allowManagedHooksOnly` even if force-enabled.

### Terminal notifications from hooks
- What: Because hooks have no tty, a hook returns `terminalSequence` and Claude Code writes the allowlisted escape (window title OSC 0/1/2, OSC 9 incl. taskbar progress, Kitty OSC 99, OSC 777, BEL) through its own write path, race-free and Windows-safe. CSI, palette, OSC 8, OSC 52 clipboard and OSC 1337 are rejected.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Emit terminal notifications; Get terminal notifications inside tmux or GNU screen)
- Notes: Allowlisting OSC 52 out blocks a clipboard-write vector from hook scripts.

### Mod UI: render sites and element trees
- What: `ui.render` fires for each render site; hooks return a tree of elements from `$.ui.resolve(e)`: Box (flex, borders, spacing steps), Text, Button, Link, Code (incl. diff), Markdown, Input (multi-row v2.1.301+), Select, Svg (Desktop/VS Code), Client (separate file for animation/pointer input; no mods API; talks via `ui.message`), Raster (terminal cell grid, base64 packed), Image (kitty/Ghostty/iTerm2). Own sites: Pane (sidebar in wide fullscreen terminal or framed region above prompt), AbovePrompt band (shared by all mods), Placed transcript rows (v2.1.314+), Backdrop (Desktop/mobile, `$.theme.set`, v2.1.319+). Built-in sites a mod can restyle or replace: UserMessage, AssistantMessage, ToolUse/ToolResult/ToolGroup, CommandOutput, AskUserQuestion, Spinner, ToolProgress, TurnDuration, InfoNotice, SessionMode, PromptHint. `next(e)` at built-in sites returns an engine reference that can be embedded.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/interface.md (Pick where to draw; Change what Claude Code already draws; Build a tree from elements; Place a row in the transcript); https://code.claude.com/docs/en/plugins/mods/reference.md (Render sites; Elements); https://code.claude.com/docs/en/plugins/mods/gallery.md
- Notes: The permission prompt is NOT a render site and cannot be changed by any mod. AskUserQuestion trees must include the engine reference exactly once. Trees are validated in the host (invalid prop -> hook skipped with reason). Limits: 32 nesting levels, 20,000 elements, 100,000 drawn chars, 262,144 Raster cells, 2 MiB images, Svg source 131,072 chars, Link href 2,048. Redraw throttled to 10/s (30/s for visible terminal pane/band/hint), coalesced. Desktop/VS Code draw `Link` as plain text unless https or http://localhost, no `@`, canonical form. VS Code labels mod-drawn content "From/Edited by the X plugin". Unsolicited panes (from timers/turn hooks) only appear at >=144 columns (110 after the user opened it once).

### Mod UI interaction, focus and lightweight surfaces
- What: Controls call `onPress`, `onSubmit`/`onInput`, `onSelect` in the mod; other mods see `ui.press/input/select` first (by element `key`) and can alter or block. Mods never read the keyboard; keys reach a control only when its pane/band has focus (Ctrl+X Tab, click in fullscreen, or `focus: true` from a user action and only while prompt is empty). Hotkeys are single digit/lowercase letter; Tab/arrows cannot be rebound. Users can collapse the band (Ctrl+X Ctrl+A) and the mod cannot detect it. Non-blocking outputs: `$.ui.status` (persistent line under prompt with ⚠ + mod name), `$.ui.toast` (4 s default), `$.ui.log` (dim transcript line Claude doesn't read, or `{to:'debug'}`), `$.ui.notify` (terminal notification via `preferredNotifChannel`, returns isSent/reason), `$.ui.ask` (question dialog).
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/interface.md (Respond to presses and typing; Keyboard focus and hotkeys; When the user collapses the band; Open a pane at the right time; Hold toasts behind a dialog); https://code.claude.com/docs/en/plugins/mods/api.md (Show something without starting a turn)
- Notes: `focus`, `closeOnEscape`, `holdToasts`, `autoFocus` accept only `true` (false throws). Pane width capped at 45% of terminal. VS Code: no hotkeys, focus only when panel focused/idle/empty prompt, `$.prompt.suggest` shows nothing, status cut at 200 chars, toast 500. Fullscreen draws at most 3 toasts at once.

### Mod state tiers (`module var` / `$.state` / `$.store`)
- What: Module variables reset on every reload; `$.state` is reactive per-session state (declared in `types/index.d.ts` `PluginState`, accessed via `atom/read/update/derive/memberOf` helpers; reading in `ui.render` subscribes and auto-redraws; survives module reload; reset by `/clear`, `/resume`, `/branch`); `$.store` is a 4 MiB JSON KV file per plugin under `~/.claude/plugins/store/`, shared by all sessions on the machine, expiring after `cleanupPeriodDays` of non-use.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/interface.md (Keep state; Keep a value in $.state; Load a saved value again after /clear; Save from more than one session)
- Notes: `$.store` get/set is not atomic across sessions; docs recommend per-item keys and read-before-write. `ui.render` hooks can read but not write state. `session.start` does not re-fire after `/clear`; reload via `classic.SessionStart` with source clear/resume/fork.

### Artifacts: publish session output as hosted live pages
- What: Claude (unprompted when output suits a page, or on request) writes a single HTML or Markdown file (default in a temp dir outside the project) and publishes it with the `Artifact` tool to a private URL on claude.ai; the browser opens to it. Republishing updates the same URL in place for open viewers, and each publish is a version. `/artifacts` lists owned and shared artifacts (Enter attach/open, `o`, `c` copy link, `d` delete), `Ctrl+]` reopens the latest, a `⧉` pill sits under the prompt. Another session updates an artifact only when given its URL or it is attached.
- Status: GA
- Evidence: https://code.claude.com/docs/en/artifacts.md (Create an artifact; Update an artifact; Find an artifact again; What you can build)
- Notes: Plans Pro/Max/Ultra/Team/Enterprise with claude.ai `/login`; not API key, gateway token or cloud-provider credentials; not Bedrock/Vertex/Foundry; not with CMEK/HIPAA/ZDR orgs. Off by default in Agent SDK, GitHub Action, MCP-server contexts, `-p`, and with `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`. `CLAUDE_CODE_ARTIFACT_AUTO_OPEN=0`. Built-in design skill picks palette/typography and prefers project design tokens. Desktop needs app 1.13576.0+.

### claude.ai/code web UI for cloud sessions
- What: Browser client: repo selector (multiple repos per session), branch picker, environment selector (only until first message is sent; later via "Edit cloud environment" on a running session), permission-mode dropdown, session sidebar, Changes card with +/- counts, diff view, Create PR / Create draft PR, Archive and Delete.
- Status: GA
- Evidence: https://code.claude.com/docs/en/web-quickstart.md (Start a task; Review and iterate); https://code.claude.com/docs/en/claude-code-on-the-web.md (Work with sessions); https://code.claude.com/docs/en/cloud-environments.md (Configure your environment)
- Notes: Plans: Pro, Max, Ultra, Team, Enterprise premium or Chat+Claude Code seats; custom roles need a cloud-sessions permission.

### Diff view with inline comments and branch compare
- What: Diff vs the session's base branch by default, or "Compare against" any branch. Hover a line, click +, type a comment; comments queue and are bundled with the next message as file:line references. Diffs are computed from raw git blob content, so repo diff drivers and `textconv` don't apply. Before a push, the list shows only Claude's own edits (not changes made by commands such as formatters); after a push, only pushed commits.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (Review changes; Troubleshoot a missing file in the diff view); https://code.claude.com/docs/en/web-quickstart.md (Leave inline comments)
- Notes: Interesting gap: tool-side file changes are invisible until pushed.

### Pre-fill session URL parameters
- What: claude.ai/code accepts `prompt` (alias `q`), `prompt_url` (fetched cross-origin, for long prompts; ignored if `prompt` set), `repositories` (alias `repo`, comma-separated owner/repo), `environment` (name or ID). Intended for "open in Claude" buttons in issue trackers.
- Status: GA
- Evidence: https://code.claude.com/docs/en/web-quickstart.md (Pre-fill sessions)
- Notes: Only pre-fills the form; user still submits.

### Mobile client (Claude app Code tab)
- What: No separate Claude Code app; the Code tab in the Claude iOS/Android app is a client for cloud sessions, projects, Remote Control sessions, and Dispatch (Desktop). Start sessions (repo, branch, task), steer, answer questions, ask Claude to watch a PR. `/mobile` (also `/ios`, `/android`) shows a QR for claude.ai/mobile. Requires a claude.ai account (no Console API key, no Bedrock etc.).
- Status: GA
- Evidence: https://code.claude.com/docs/en/mobile.md (all sections)
- Notes: Permission modes from mobile: cloud = Accept edits, Plan, Auto; RC = Manual, Accept edits, Plan, Auto; never Bypass. Dispatch is Pro/Max/Ultra only (Desktop feature; not surveyed here).

### Attachments from phone/browser into a local session
- What: Over Remote Control, photos are given to Claude directly and also saved under `~/.claude/uploads/` with the path told to Claude; other files are downloaded to the machine and passed as `@` references. If a download fails, Claude gets a note like "[1 of 3 attachments did not arrive]".
- Status: GA
- Evidence: https://code.claude.com/docs/en/mobile.md (Continue a local session with Remote Control); https://code.claude.com/docs/en/remote-control.md (intro; Limitations)
- Notes: Self-hosted: attachments fetched by subprocesses using `CLAUDE_SESSION_INGRESS_TOKEN_FILE`; not available on Bedrock/Vertex runners (SHC).

### Queued message take-back and plugin suggestion cards
- What: A message sent while Claude works queues; click the X to take it back to the input box until Claude reads it. Claude may unprompted show plugin-suggestion cards with an Add button that enables the plugin on the claude.ai account; disable by denying `SearchPlugins` and `SuggestPluginInstall`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (Take back a queued message; Plugin suggestions in the conversation)
- Notes: Repo deny rules ignored when managed `allowManagedPermissionRulesOnly` is set.

### VS Code extension: surface and shared CLI
- What: A graphical panel (Spark icon) for VS Code 1.94+ and forks (Cursor, Devin Desktop, Kiro, Open VSX). It bundles a private copy of the CLI, and the extension version equals the bundled Claude Code version. It does not put `claude` on PATH. The extension and the CLI share history (`claude --resume`) and `~/.claude/settings.json`. Features:
  - Several conversations in tabs or windows; the tab's dot is blue for a pending permission and orange for finished-while-hidden.
  - Session list: groups saved per workspace, Active and status filters, AI-generated titles, auto-archive after 14 days (configurable).
  - Plugin manager with scope choice.
  - `/mcp` add and remove (v2.1.261+).
  - Customize dialogs for hooks, permissions, memory, CLAUDE.md, sandbox, output styles (v2.1.257–2.1.280+).
  - Focus view; `/btw` side panel; bookmarks; LaTeX rendering; `/usage` with attribution by skill, subagent, plugin, and MCP; `/export`.
  - Not available versus the CLI: the `!` bash shortcut, tab completion, and some commands. A terminal mode is available (`useTerminal`).
  - Screen-reader announcements are on for everyone (v2.1.236+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/vs-code.md (Prerequisites; Install the extension; Use the prompt box; Run multiple conversations; Organize sessions into groups; VS Code extension vs. Claude Code CLI; Use a screen reader)
- Notes: Version gates are given per feature throughout. Third-party providers lose claude.ai-only features (plan usage bars, voice dictation, Web tab).

### Editor selection and open-file context
- What: VS Code sends the current selection and the open file's name with each prompt automatically. The footer shows the line count. Option/Alt+K inserts an `@file.ts#5-10` reference. `attachSelection` (v2.1.300+) and `attachOpenFile` (v2.1.271+) turn each off, and both also govern the CLI in the integrated terminal. The chat panel withholds selected text from files matched by `files.exclude` or `search.exclude`, and from gitignored files when `search.useIgnoreFiles` and `respectGitIgnore` are on, sending only the path. This filter does not apply to the CLI in the terminal, where a `Read` deny rule is needed. A queued message keeps the selection from the moment Enter was pressed. `@terminal:name` brings in terminal output.
  JetBrains: the selection or current tab is shared automatically, Cmd+Option+K / Alt+Ctrl+K inserts `@src/auth.ts#L1-99`, and `Read` deny rules block sharing.
- Status: GA
- Evidence: https://code.claude.com/docs/en/vs-code.md (Automatic context from your selection and open file; Selected text from excluded or ignored files; Include terminal output in prompts; The built-in IDE MCP server); https://code.claude.com/docs/en/jetbrains.md (Features)

### Diagnostics sharing
- What: Claude reads the editor's Problems or inspections through `getDiagnostics`. In the VS Code chat panel (v2.1.285+), a separate built-in server `claude-vscode` exposes `mcp__claude-vscode__getDiagnostics` for one file or for all files that have diagnostics. Rules and hooks must name both `mcp__ide__getDiagnostics` and `mcp__claude-vscode__getDiagnostics`, and a `Read` deny rule covers neither. JetBrains: one file per call, and Claude Code does not request diagnostics on its own after edits.
- Status: GA
- Evidence: https://code.claude.com/docs/en/vs-code.md (Diagnostics in the chat panel); https://code.claude.com/docs/en/jetbrains.md (Features; Tools exposed to the model)

### JetBrains plugin
- What: Supports IntelliJ IDEA, PyCharm, Android Studio, WebStorm, PhpStorm, GoLand, and others. Unlike VS Code, the plugin does not bundle the CLI. It runs `claude` in the IDE terminal (the command is configurable, for example a `wsl -d ... -- bash -lic "claude"` wrapper) and connects to it. Features: quick launch (Cmd/Ctrl+Esc), diffs in the IDE viewer (`diffTool` set to `auto` or `terminal` in `/config`, shown only while connected), selection sharing, file-reference shortcut, diagnostics. For Remote Development the plugin must be installed on the host. The WSL2 "No available IDEs detected" problem is fixed with a firewall rule or mirrored networking.
- Status: beta (the plugin is named "Claude Code [Beta]")
- Evidence: https://code.claude.com/docs/en/jetbrains.md (Supported IDEs; Features; Installation; Configuration; Special configurations)

### Deep links / URI handlers into the IDE
- What: `vscode://anthropic.claude-code/open?prompt=...&session=...` opens a tab with a prefilled prompt (never auto-submitted) or resumes a session, including worktrees of the same repo from v2.1.300. `vscode://anthropic.claude-code/install-plugin?plugin=&marketplace=` opens the plugin dialog at the scope choice and installs nothing until the user picks. It validates its inputs strictly: the plugin name is limited to 100 characters with a restricted charset; the marketplace must be GitHub `owner/repo`, `https://` without credentials or query, or git SSH (no local path or `http://`); a `#ref` must be written `%23`; `anthropics` marketplaces cannot be pinned.
- Status: GA
- Evidence: https://code.claude.com/docs/en/vs-code.md (Launch a VS Code tab from other tools; Share a plugin install link)

### VS Code extension settings (family)
- What: `useTerminal`, `initialPermissionMode` (read only from user settings, not the workspace, since v2.1.225), `preferredLocation`, `lockEditorGroups`, `autosave`, `attachOpenFile`, `attachSelection`, `useCtrlEnterToSend`, `scrollToBottomOnSend`, `showMessageTimestamps`, `spinnerVerbs`, the two new-conversation and reopen shortcuts, `archiveInactiveSessions` (1, 2, 7, or 14 days; 0 turns it off), `continueAfterReload`, `hideOnboarding`, `focusView`, `respectGitIgnore`, `usePythonEnvironment`, `environmentVariables` (an absolute `CLAUDE_CONFIG_DIR` only), `disableLoginPrompt` (for third-party providers), `allowDangerouslySkipPermissions` (adds Bypass to the mode picker), `claudeProcessWrapper`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/vs-code.md (Extension settings)
- Notes: Ignoring workspace values for the permission mode stops a repository from choosing a looser starting mode.

### Gateway serving Claude Desktop (one line)
- What: Desktop connects via `bootstrapUrl` = `<public_url>/user/bootstrap`, opt-in per policy with a `desktop` key (404 otherwise), gets derived model list, disabled tools and egress allowlist plus a validated `desktop` overlay; `code` policy key extends Claude Code settings into Desktop's Code tab under conditions.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-config.md (Claude Desktop overlay; Choose cli or code)
- Notes: Desktop-specific; surveyed elsewhere.

### Output streaming (partial messages)
- What: Default stream yields one complete `AssistantMessage` per content block. `includePartialMessages` adds raw API events (`message_start`, `content_block_*`, `message_delta`, `message_stop`, `text_delta`, `input_json_delta`) wrapped as `stream_event`; the app accumulates deltas itself. Stream events exist only for the main session (`parent_tool_use_id` always null); subagent text arrives as complete messages only with `forwardSubagentText`. A cut-off stream still ends with `message_stop` (v2.1.290+); an unclosed tool block means incomplete input.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/streaming-output.md (Enable streaming output; StreamEvent reference; Message flow; Handle a stream that's cut off; Known limitations)
- Notes: Structured output streams as unvalidated `input_json_delta`; only `structured_output` on the result is validated. Gateway keep-alive `ping` events should be treated as liveness.

### Prompt suggestions and session title
- What: `promptSuggestions: true` makes the CLI emit a `prompt_suggestion` message with a predicted next user prompt after a turn (skipped near usage limits; can arrive after the result). `title` sets a display title (persisted title wins on resume); `renameSession()`/`tagSession()` edit metadata.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (Options; renameSession(); tagSession()); https://code.claude.com/docs/en/agent-sdk/agent-loop.md (Message types)
- Notes: Small UI affordances a dashboard can reuse.

### Terminal UI additions
- What:
  - Fullscreen renderer: a live `/diff` panel needing at least 110 columns (2.1.260); mouse support across lists; scrollbars; click to send a queued message (2.1.312).
  - Send-now key, Ctrl+Enter, interrupts and sends all queued messages (2.1.275).
  - `maxProseWidth` (2.1.282). `timeFormat` / `timeZone` (2.1.257). `spellcheck` via aspell, hunspell or ispell (2.1.235). Emoji shortcodes (2.1.217).
  - Readline word keys for everyone (2.1.261). `vimInsertModeRemaps` (2.1.208). Vim `%` motion (2.1.301).
  - Draft recovery after Ctrl+C with Up (2.1.288).
  - Screen reader mode `--ax-screen-reader` (2.1.208).
  - `/focus` view. Concise output style (2.1.237). `/output-style` command restored (2.1.269).
  - `/radio` lo-fi stream (Week 27).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.208, 2.1.217, 2.1.235, 2.1.237, 2.1.257, 2.1.260, 2.1.261, 2.1.269, 2.1.275, 2.1.282, 2.1.288, 2.1.301, 2.1.312); https://code.claude.com/docs/en/whats-new/2026-w27.md, https://code.claude.com/docs/en/whats-new/2026-w29.md, https://code.claude.com/docs/en/whats-new/2026-w34.md, https://code.claude.com/docs/en/whats-new/2026-w36.md

### IDE and Desktop handoff
- What:
  - `claude --desktop` opens the desktop app on the current directory or session (2.1.285).
  - VS Code: Focus view (Week 32), session groups (Week 33), agent map (Week 37), message timestamps on by default (2.1.290), idle compaction (2.1.314).
  - Windsurf was renamed Devin Desktop in the menus (2.1.162).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.162, 2.1.285, 2.1.290, 2.1.314); https://code.claude.com/docs/en/whats-new/2026-w32.md, https://code.claude.com/docs/en/whats-new/2026-w33.md, https://code.claude.com/docs/en/whats-new/2026-w37.md
- Notes: Desktop-only items (in-app browser, iOS Simulator, pop-out panes, background computer use, Linux beta) are out of scope and get one line here.

## Area 16. Browser automation and computer use

### Browser and computer use on these pages
- What: Only cross-references: `--chrome`/`--no-chrome`, `/chrome`, `/claude-in-chrome` skill (Claude in Chrome extension using the user's logged-in browser), computer use on macOS from the CLI on Pro/Max/Ultra.
- Status: GA (as referenced)
- Evidence: https://code.claude.com/docs/en/cli-reference.md (--chrome); https://code.claude.com/docs/en/commands.md (/chrome, /claude-in-chrome); https://code.claude.com/docs/en/platforms.md (Where to run Claude Code)
- Notes: Mechanism not documented on S1 pages.

### Claude in Chrome integration (mechanism)
- What: Claude Code drives the user's real Chrome or Edge (and Brave, Arc, Vivaldi, Opera) through the Claude in Chrome extension (v1.0.36+). The browser keeps its logged-in state, so the agent can act on any site the user is signed in to. Actions run in a visible window. Mechanism as documented: on first enable, Claude Code installs a native-messaging-host manifest, `com.anthropic.claude_code_browser_extension.json`, in each browser's NativeMessagingHosts directory (registry keys on Windows), which Chrome reads at startup. Connectivity also needs `bridge.claudeusercontent.com`. The extension and Claude Code must be signed in to the same claude.ai account and organization. On Windows a named pipe is used (EADDRINUSE conflicts are possible). Tabs Claude opens go into a Chrome tab group tied to the session: `/clear` closes the group, while `/resume` or exit closes it only if it contains nothing but empty tabs. The tools are exposed as MCP server `claude-in-chrome` (listed under `/mcp` > View tools). Capabilities include console and DOM reading, form filling, data extraction, GIF recording, screenshots to disk, and file upload.
  - Upload limits: 10 MB total per upload. Files must be readable under the session's Read rules. Hard-linked files are refused. Credential-like paths (`.env`, `.pem`, `.key`, `.ssh`) are refused from v2.1.293. Claude Code reads the file and sends its contents to the browser, so uploads also work in remote sessions.
- Status: GA (the page does not mark it beta)
- Evidence: https://code.claude.com/docs/en/chrome.md (intro; Prerequisites; Upload files to web pages; Extension not detected; Windows-specific issues; Common error messages)
- Notes: Gated to direct Anthropic plans with `/login`. It stays off with API keys or `setup-token` tokens and with third-party providers. Not supported in WSL. Off by default in HIPAA Enterprise orgs. Managed `deniedMcpServers` or an exclusive `managed-mcp.json` suppresses the install prompt.

### Chrome enablement and context cost
- What: Start with `claude --chrome`, or set "Enabled by default" through `/chrome` (shared by the CLI and VS Code). A startup dialog that detects the extension sets the same flag. If the extension is missing, an install prompt appears at most once per session (Install, Not now, or Don't ask again), with guided install and resume in the same session. With the default on, browser tool definitions are always loaded, which the docs say raises context use. VS Code otherwise connects lazily on `@browser`. The extension's service worker can idle and drop the connection; "Reconnect extension" restarts it (from v2.1.290).
- Status: GA
- Evidence: https://code.claude.com/docs/en/chrome.md (Get started in the CLI; Install the extension when Claude asks; Enable Chrome by default; Connection drops during long sessions); https://code.claude.com/docs/en/vs-code.md (Automate browser tasks with Chrome)

### CLI computer use (macOS)
- What: A built-in MCP server, `computer-use`, off by default and enabled per project through `/mcp`. It needs the macOS Accessibility and Screen Recording permissions. Claude prefers MCP, then Bash, then Chrome, and uses screen control only for native apps, simulators, and tools with no API.
  - One session at a time: a lock is taken at the first action and released when the session exits, not when the task ends. A crashed holder is detected and its lock released.
  - While Claude works, other apps are hidden and restored at the end of the turn (always on in the CLI).
  - The terminal is excluded from screenshots, so the session cannot read its own output.
  - Screenshots are always downscaled (for example 3456×2234 to about 1372×887), with no setting.
  - A macOS notification announces control. Esc anywhere aborts, and the keypress is consumed so on-screen content cannot use it to dismiss dialogs. Ctrl+C also works.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/computer-use.md (When computer use applies; Enable computer use; How Claude works on your screen; Safety and the trust boundary; Differences from the Desktop app; Troubleshooting)
- Notes: Gating: macOS only, Pro, Max, or Ultra only (not Team or Enterprise), claude.ai auth only (no Bedrock, Vertex, or Foundry), interactive only (not `-p`). "Claude checks each action and flags potential prompt injection" has no mechanism described (`vendor-claim`). Desktop shares the engine and adds Windows, a denied-apps list, and Dispatch (one line, per scope).

### Browser automation and computer use
- What: Nothing documented in the Agent SDK pages beyond connecting MCP servers (the MCP page mentions browsers as an example category).
- Status: unclear
- Evidence: https://code.claude.com/docs/en/agent-sdk/agent-loop.md (Built-in tools: "MCP servers (databases, browsers, APIs)")
- Notes: Finding: no built-in browser tool is listed for the SDK.

### Claude in Chrome and computer use
- What: computer use in the CLI was a research preview (Week 14). Claude in Chrome became GA in 2.1.198 (Jul 1, 2026). `allowClaudeInChromeWithManagedMcp` (2.1.282). The shortcut tools were removed (2.1.319).
- Status: Chrome GA; CLI computer use was labeled research preview at launch
- Evidence: https://code.claude.com/docs/en/whats-new/2026-w14.md, https://code.claude.com/docs/en/whats-new/2026-w27.md; https://code.claude.com/docs/en/changelog.md (2.1.198, 2.1.282, 2.1.319)

## Area 17. Remote, cloud, headless, CI and distributed execution

### Non-interactive / print mode and structured I/O flags
- What: `claude -p "query"` runs once and exits; piped stdin is accepted. `--output-format text|json|stream-json`; `--input-format text|stream-json`; `--include-partial-messages`, `--include-hook-events`, `--replay-user-messages`, `--prompt-suggestions`, `--forward-subagent-text` (emit foreground subagent text/thinking with `parent_tool_use_id`, v2.1.211+) all need stream-json. `--json-schema` returns validated JSON after the run. `--max-turns` (no default limit; a queued stream-json message starts a new turn with its own limit) and `--max-budget-usd` (client-side estimate, includes subagents, can overshoot; once hit, new subagents fail with `Budget limit reached` and background subagents stop, v2.1.217+). `--no-session-persistence`. `--init`, `--maintenance`, `--init-only` run Setup/SessionStart hooks. `--bare` skips discovery of hooks, skills, commands, subagents, plugins, MCP, auto memory and CLAUDE.md, leaving Bash, read and edit tools. `--exclude-dynamic-system-prompt-sections` moves per-user context to the first user message for cross-user prompt-cache reuse.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cli-reference.md (CLI commands; CLI flags)
- Notes: `claude --help` does not list every flag. `--append-subagent-system-prompt[-file]` applies only with `-p`.

### Cloud, remote and self-hosted runner CLI
- What: `--cloud "task"` creates a cloud session, or with a session ID/URL plus `-p` queues a follow-up (`--remote` is a deprecated alias); `--teleport` / `/teleport` pulls a cloud session's branch and conversation into the terminal; `--remote-control`/`--rc` and `claude remote-control` (server mode) expose a local session to claude.ai/mobile; `--environment ccpool_...` plus `--ref` dispatch to a self-hosted environment; `claude self-hosted-runner [setup|doctor|orchestrator]` (v2.1.224+) registers a machine to host cloud sessions on your infrastructure; `claude gateway --config gateway.yaml` starts a self-hosted apps gateway for SSO and policy in front of Bedrock/Vertex/Foundry.
- Status: GA except `--channels` (research preview)
- Evidence: https://code.claude.com/docs/en/cli-reference.md (CLI commands; CLI flags)
- Notes: `claude setup-token` prints a long-lived OAuth token for CI without saving it. `claude auth status` prints JSON (`authMethod`, `configDirectory`), exit 0/1.

### Non-interactive mode sessions are persisted by default
- What: `-p`/`--print` (formerly "headless mode") runs one prompt and exits, but the run is still saved as a resumable session unless `--no-session-persistence` is passed. Bare mode is recommended for CI for reproducible results across machines.
- Status: GA
- Evidence: https://code.claude.com/docs/en/glossary.md (Non-interactive mode; Bare mode; Deprecated and renamed terms)
- Notes: "Custom commands" renamed to skills (`.claude/commands/` still works); "slash commands" now just "commands".

### Dev container: install, persistence, policy, egress, bypass
- What: The Claude Code Dev Container Feature (`ghcr.io/anthropics/devcontainer-features/claude-code:1.0`; tag pins the install script, not the CLI) installs the latest Claude Code (auto-updates by default) and the VS Code extension in VS Code/Codespaces. Persist auth by mounting a named volume at `~/.claude` and setting `CLAUDE_CONFIG_DIR` to it so `.claude.json` lands in the volume; `${devcontainerId}` isolates per project. Codespaces: `ANTHROPIC_API_KEY` or `CLAUDE_CODE_OAUTH_TOKEN` as secrets. Managed policy at `/etc/claude-code/managed-settings.json` (highest precedence), but anyone with repo write can change the Dockerfile, so server-managed or MDM is recommended for non-bypassable policy. Reference container adds `init-firewall.sh` default-deny iptables egress (needs `NET_ADMIN`/`NET_RAW`) and runs non-root, enabling `--dangerously-skip-permissions`. Pin CLI via `npm install -g @anthropic-ai/claude-code@X.Y.Z` plus `DISABLE_AUTOUPDATER=1`.
- Status: GA (reference container is "a working example rather than a maintained base image")
- Evidence: https://code.claude.com/docs/en/devcontainer.md (Add Claude Code to your dev container; Persist authentication and settings across rebuilds; Enforce organization policy; Restrict network egress; Run without permission prompts; Try the reference container)
- Notes: Docs warn that with bypass a malicious project can exfiltrate anything in the container, including Claude Code credentials in `~/.claude`; avoid mounting host `~/.ssh` or cloud credentials. Cloud-provider creds should come via `containerEnv`/workload identity, not mounted files.

### Cloud session and Remote Control security
- What: Cloud sessions: isolated Anthropic VM per session; network limited by default (configurable to disabled or allowlist); GitHub credentials stored encrypted server-side and never enter the VM, which holds a short-lived session-scoped credential while an Anthropic GitHub proxy attaches the real credential; the proxy rejects branch deletions and non-branch pushes (tags), with branch protection enforced by GitHub; all operations audit-logged; VMs reclaimed after inactivity; sessions deletable. All outbound traffic goes through a security proxy for audit and abuse prevention. Remote Control: execution stays local; traffic through the Anthropic API over TLS; transcript stored on Anthropic servers while connected; multiple short-lived, narrowly scoped credentials that expire independently.
- Status: GA
- Evidence: https://code.claude.com/docs/en/security.md (Cloud execution security); https://code.claude.com/docs/en/data-usage.md (Cloud execution: Data flow and dependencies)
- Notes: Credential-out-of-VM proxy pattern is directly relevant to keeping provider/git secrets off worker nodes.

### Long-lived OAuth token for CI (`claude setup-token`)
- What: `claude setup-token` runs the browser flow and prints a one-year OAuth token (not saved) to set as `CLAUDE_CODE_OAUTH_TOKEN`. Requires a Pro/Max/Ultra/Team/Enterprise plan; can only make model requests (no Remote Control, no claude.ai connectors). Bare mode does not read it (use `ANTHROPIC_API_KEY` or `apiKeyHelper`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/authentication.md (Generate a long-lived token)
- Notes: `setup-token` and `/install-github-app` enforce only `forceLoginMethod`, so they can mint tokens in another org.

### Background agent supervisor and corporate launcher (network angle)
- What: Background sessions (`claude agents`, `--bg`, `/background`) are hosted by a per-user supervisor process that outlives the shell and inherits the environment of whichever shell started it first, so network variables must be set in settings `env`, not shell exports. `processWrapper` / `CLAUDE_CODE_PROCESS_WRAPPER` prefixes the supervisor and its workers with a corporate launcher (they start Claude Code from a fixed path, bypassing PATH wrappers); `claude daemon stop --any` restarts it with new config.
- Status: GA
- Evidence: https://code.claude.com/docs/en/network-config.md (Apply network settings to background agents; Configure a corporate launcher as a setting)

### Plugin seed directories for containers and CI
- What: Build a plugins dir at image build time with `CLAUDE_CODE_PLUGIN_CACHE_DIR=/opt/seed claude plugin marketplace add ... / install ...`, then set `CLAUDE_CODE_PLUGIN_SEED_DIR` (multiple paths `:`/`;`) at runtime; marketplaces register and caches load in place without cloning. Seeds are read-only, auto-update forced off, seed entries overwrite same-named user entries each startup, update/remove fail, allow/block lists still apply. Plugins in a seed still need `enabledPlugins`. In `-p`/CI, installs run in the background unless `CLAUDE_CODE_SYNC_PLUGIN_INSTALL=1`; verify via the stream-json `init` event's `plugins` list.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/org.md (Seed containers and CI; Confirm the rollout)
- Notes: For private marketplaces in CI the docs suggest `GH_TOKEN` + `gh auth setup-git`.

### Plugin evals in CI
- What: Recommended invocation pins `--model` and `--judge-model`, uses `--trust-plugin`, `--json results.json`, `--threshold`, `--no-publish`, `--max-cost-usd`. Exit codes: 0 pass; 1 below threshold/load error/untrusted/API-error run; 2 partial (cost ceiling or auth failure); 130 interrupted; 143 terminated. Delta never affects the exit code. JSON result is versioned (`schemaVersion: 1`, camelCase, additive). `-j` up to 8 concurrent runs. Self-contained `report.html`, optionally published as a private claude.ai artifact.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugin-evals.md (Run evals in CI; Command options; JSON result; HTML report)
- Notes: Cost ceiling is a list-price estimate checked before each run, so spend can overshoot by in-flight runs.

### Cloud scheduled tasks (formerly routines)
- What: A saved configuration (prompt, repos, cloud environment, connectors, model and effort) that runs as a full cloud Claude Code session on Anthropic infrastructure or an org's self-hosted environment. It has three trigger types, which can be combined: schedule (presets or custom cron; more often than hourly is rejected; cron in UTC unless `CRON_TZ=`; one-off runs supported), API (a per-task `/fire` endpoint with a bearer token shown once, plus regenerate and delete), and GitHub (pull request, release and issue events; the Claude GitHub App is required; filters on author, title, body, base or head branch, labels, draft and merged, with operators that include full-match regex). Each run clones the repos fresh and pushes to `claude/` branches. A PR trigger starts from the PR head. Sessions start in auto mode and stop only on ask-rule matches or repeated-block thresholds. Tasks belong to the individual account: actions appear as that user, and runs are not shared.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/routines.md (Configure triggers; Add an API trigger; Add a GitHub trigger; Permission prompts during a run; Repositories and branch permissions)
- Notes: Plans: Pro, Max, Ultra, Team, Enterprise. Created at claude.ai/code/routines, in Desktop, or with CLI `/schedule` (alias `/routines`; needs a claude.ai login, not an API key or third-party provider). The beta header is `experimental-cc-routine-2026-04-01`, and the two previous header versions keep working. If GitHub is disconnected, runs are skipped for up to 72 hours, after which the task turns off.

### Projects (coordinator conversation plus parallel cloud threads)
- What: One long-running coordinator conversation routes incoming work. It answers quick questions in place, or starts or reuses "threads": separate cloud sessions, each with its own context and branch, that open PRs and watch them with auto-fix (CI fixes, review comments). On request a thread can run on the user's machine via Remote Control (`claude remote-control [--spawn worktree]`, v2.1.280+). The coordinator sees thread reports, not every step. Standing context: project memory (`MEMORY.md` index read by every cloud thread), project instructions (up to 16,000 chars), repos, uploaded files under `/mnt/project-files`, and the cloud environment. Cloud threads load repo-root CLAUDE.md, `.claude/CLAUDE.md` and `.claude/rules/` only. They do not load subdir CLAUDE.md, skills, agents, commands, `.mcp.json`, plugins, permission rules, hooks or env from the repo. The Overview pane groups threads as Ready for review, Waiting on you, Working, Landing, Idle and Resolved (auto-resolved after a week idle). Defaults: Opus everywhere, high effort for threads, low for the coordinator. Threads run in auto mode, and approvals happen inside each thread.
- Status: public beta (Pro, Max, Ultra; rolling out; not Team or Enterprise; not CLI, VS Code or JetBrains; Anthropic provider only)
- Evidence: https://code.claude.com/docs/en/claude-projects.md (How a project is organized; Give a project standing context; What threads pick up from your repositories; Run a thread on your own computer; Unblock a thread waiting on approval)
- Notes: The enforced limit is 500 new threads per day; thread-count preferences you give Claude are not enforced. Upload limits: 1,000 files and 2 GB per upload, 480 MB per file, 10 folders. A cloud sandbox pauses between turns, and if it cannot resume, the thread restarts from a fresh clone and can lose uncommitted work. Projects are single-user with no sharing and no org controls during the beta. Threads can add repos only from GitHub owners the project already uses.

### Ultrareview (cloud multi-agent review)
- What: `/code-review ultra [base|PR|note] [--post]` (alias `/ultrareview`) runs a fleet of reviewer agents in a cloud sandbox. Every finding is independently reproduced or verified. A branch review bundles and uploads repo state, with special rules for credential-like files. A PR review clones from github.com and uploads nothing. Limits: 500 files and 8,000 changed lines by default. Fallbacks: a first commit or a missing merge base reviews the whole repo, but only after confirmation. Billing: 3 free runs per account (one-time), then usage credits, typically $5 to $25 per review; it takes 5 to 15 minutes. `--post` posts one plain PR comment from your GitHub account, server-side through the Anthropic API, in v2.1.227+. `claude ultrareview [target] [--json] [--timeout 45] [--post] [--trust-workspace]` blocks, prints findings to stdout and progress to stderr, and exits 0, 1 or 130. Claude never starts it on its own. If Claude runs the subcommand via Bash, the whole-repo fallback is refused, because running it yourself is what counts as consent.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/ultrareview.md (Run ultrareview from the CLI; Diff limits and fallbacks; Pricing and free runs; Run ultrareview non-interactively; Post findings to the pull request)
- Notes: Needs a claude.ai login. Not available on Bedrock, Agent Platform or Foundry, or with ZDR, HIPAA or CMEK; in those cases it falls back to a local review (CMEK cannot run it at all). `claude -p '/code-review ultra'` launches the review and exits without findings. Stopping a review archives it with no partial findings.

### Headless / print mode (-p) basics and bare mode
- What: `claude -p` (`--print`) runs non-interactively. It reads stdin (capped at 10MB) and exits 0 on success, non-zero on failure. Flag errors go to stderr; failures inside the run are printed as the result on stdout. `--bg` is rejected; `--cloud <session> -p` queues a message to a cloud session. Without `--bare`, `-p` loads hooks, skills, agents, plugins, MCP and CLAUDE.md just as an interactive session does, and runs a project's `.claude/settings.json` hooks and `.mcp.json` servers even in untrusted folders, with no trust dialog. `--bare` skips all auto-discovery, never reads OAuth or the keychain (needs `ANTHROPIC_API_KEY` or `apiKeyHelper`), sends no system reminders, connects only CLI-supplied MCP, has no background tasks, and runs no hooks, even ones passed via `--settings` or plugins. Context is passed explicitly via `--append-system-prompt[-file]`, `--settings`, `--mcp-config`, `--agents`, `--plugin-dir` and `--plugin-url`.
- Status: GA; `--bare` becoming the `-p` default is planned
- Evidence: https://code.claude.com/docs/en/headless.md (Basic usage; Start faster with bare mode)
- Notes: Bare-mode restrictions were made complete in v2.1.286. `--add-dir` in bare mode still loads its `.claude/skills/`.

### Headless output formats and stream-json events
- What: `--output-format text|json|stream-json`. `json` includes `result`, `session_id`, `total_cost_usd`, per-model cost (client-side estimates, cumulative across `--continue`/`--resume`) and, with `--json-schema`, `structured_output`. That field is absent if Claude declined; `format` keywords are annotations only; an invalid schema is an error. `stream-json --verbose --include-partial-messages` gives NDJSON events, with `stream_event` text deltas and a final `result`. Event types: `system/init` (model, tools, `mcp_servers` with status, `mcp_server_errors`, `plugins`, `plugin_errors` with `path`, and a `capabilities` array for feature detection such as `interrupt_receipt_v1`), `system/api_retry` (attempt, max_retries, retry_delay_ms, error_status, error category, `no_response`), `system/plugin_install`, hook events, and `permission_denied`. Subagent messages carry `parent_tool_use_id` (the Agent or Skill tool_use ID; `forked-command-*` for `/skill` prompts), so the nesting tree can be rebuilt. `--forward-subagent-text` or `CLAUDE_CODE_FORWARD_SUBAGENT_TEXT` adds subagent text and thinking. On exit, Claude Code drains queued output for up to 30 seconds.
- Status: GA
- Evidence: https://code.claude.com/docs/en/headless.md (Get structured output; Stream responses; Follow subagent messages; Handle API retries; Read session metadata; Fail CI when a plugin or MCP server doesn't load)
- Notes: With `--mcp-config`, Claude Code waits for pending servers up to `MCP_TIMEOUT` (30s) before the first turn (v2.1.221+). Invalid `--mcp-config` entries are skipped silently when stderr is captured, so the docs say CI should gate on `mcp_server_errors`.

### Headless lifecycle: background work at exit, signals, continuation
- What: After the final turn, `-p` waits for background subagents and workflows (indefinitely), main-conversation background commands (until exit or time limit), Monitor watches, and self-paced `/loop` wakeups (past the cap). Other idle waiting stops after 10 minutes, set by `CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS`; 0 means no cap. `--max-budget-usd` stops background work. `-p` does not wait for MCP background tasks or auto-backgrounded MCP calls; the latter's results are lost. SIGTERM: exit 143, the turn is left unrecorded, Bash process trees are killed, `SessionEnd` hooks run, and nothing new starts. SIGINT or SDK `interrupt()` ends the turn cleanly. `CLAUDE_CODE_RESUME_INTERRUPTED_TURN=1` continues an interrupted turn on resume. A deleted cwd keeps the session alive with warnings. `--continue` resumes the most recent session in this directory (including finished background sessions, v2.1.257+). `--resume <id>` works from any directory on the machine, and also accepts a `.jsonl` transcript path. In `-p`, user skills and custom commands expand. Terminal-only built-ins are unavailable. `/model X`, `/effort`, `/config key=value` and `/output-style` take arguments.
- Status: GA
- Evidence: https://code.claude.com/docs/en/headless.md (Background tasks at exit; Stop a run with SIGTERM; If the working directory is deleted; Continue conversations; Create a commit)
- Notes: Since v2.1.295, text output prints each background-triggered turn's result.

### Remote Control: relay of a local session to web/mobile
- What: Connects claude.ai/code or the Claude app to a session on your machine. Execution, filesystem, MCP servers, local config stay local; `@` autocompletes local paths. Conversation, subagent and workflow progress sync across all connected devices; input can come from terminal, browser and phone interchangeably. Stopping a subagent from the device stops it on the machine.
- Status: GA
- Evidence: https://code.claude.com/docs/en/remote-control.md (intro; Connect from another device)
- Notes: Plans Pro, Max, Ultra, Team, Enterprise; Team/Enterprise need an Owner toggle; custom roles need both cloud-sessions and "Claude Code remote control" permissions. Not available with API keys, Bedrock, Agent Platform, Foundry, a non-default `ANTHROPIC_BASE_URL`, an enterprise apps gateway, or when `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`/`DISABLE_GROWTHBOOK` is set (feature flags required).

### Remote Control transport and wire model
- What: Local process makes outbound HTTPS only and never opens inbound ports. On start it registers with the Anthropic API and polls for work; when a device connects, the server routes messages between the client and the local session over a streaming connection. All traffic via the Anthropic API over TLS, using several short-lived credentials, each single-purpose and expiring independently. While connected the transcript (messages, responses, tool activity) is stored on Anthropic servers to keep devices in sync and allow reconnect after network drops; execution stays local.
- Status: GA
- Evidence: https://code.claude.com/docs/en/remote-control.md (Connection and security)
- Notes: Exact protocol (SSE vs WebSocket) not stated for RC; self-hosted pages state SSE for the session stream. Full-scope login required: `claude setup-token` / `CLAUDE_CODE_OAUTH_TOKEN` tokens are inference-only and rejected (RC, Troubleshooting).

### Remote Control invocation modes and server mode
- What: `claude remote-control` (alias `claude rc`) is server mode: a long-running process serving multiple sessions, showing URL and QR (spacebar). `claude --remote-control [name]` / `--rc` is an interactive session also reachable remotely. `/remote-control` (`/rc`) attaches the current conversation including history. Also from VS Code and Desktop. Server flags: `--name`, `--remote-control-session-name-prefix` (default hostname), `-c/--continue`, `--session-id`, `--spawn same-dir|worktree|session`, `--capacity N` (default 32), `--[no-]create-session-in-dir` (default on), `--permission-mode`, `--chrome/--no-chrome` (v2.1.273+), `--debug`, `--debug-file`, `--verbose`. `--sandbox` removed in v2.1.300 (use sandbox settings instead).
- Status: GA
- Evidence: https://code.claude.com/docs/en/remote-control.md (Start a Remote Control session)
- Notes: `worktree` spawn gives each on-demand session its own git worktree; press `w` to toggle and the choice is saved per project. Global flags before `remote-control` are refused unless known-harmless (e.g. `--verbose`, `--model`), to avoid silently not applying e.g. `--settings`. One-time consent prompt; workspace trust prompt (never saved for home dir; non-TTY exits with error).

### Remote Control resume, reconnect and takeover rules
- What: After Ctrl+C on the server, sessions are not archived; rerun in the same dir to bring all back, `--continue` for the original one, `--session-id` for one; works for about four hours. Interactive sessions reconnect on resume via `claude --resume/--continue`. A second terminal resuming the same conversation does not steal RC; it prints a notice and `/remote-control` moves it. "Another connection took over this session" is a distinct disconnect reason. Server mode gives up after about 10 minutes of network outage and exits; interactive mode retries indefinitely. HTTP 403 from network middleboxes retried up to 3 minutes. Presence heartbeat failure about 30 minutes disconnects. Crashed server-mode sessions are re-served when a device sends a message (v2.1.238+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/remote-control.md (Resume sessions after stopping the server; Check connection status; Limitations)
- Notes: If compaction rewrote the conversation or `/resume` switched it while disconnected, reconnect archives the old server session.

### What connected devices see (sync semantics)
- What: Devices mirror the terminal live, including compaction progress and `/clear`. `/resume` and `/teleport` switch the conversation without sending its earlier history to the device. Diff pane shows changes since branch split from default (incl. uncommitted), or only uncommitted on default branch. Model picked on device applies to the session (v2.1.238+); `/model <name>` from device also sets the default; effort can be set unless pinned by env var. Dialogs other than permission prompts and AskUserQuestion expire after 5 minutes (`dialogExpiry`, v2.1.224+) with the no-action default.
- Status: GA
- Evidence: https://code.claude.com/docs/en/remote-control.md (What connected devices see; Limitations)
- Notes: Title order: explicit name, `/rename`, last meaningful message, auto `host-adjective-noun`.

### Remote commands from mobile/web
- What: Terminal-UI-only commands (`/plugin`, `/resume`) are local-only. Text-output commands work (`/compact`, `/clear`, `/context`, `/usage`, `/exit`, `/recap`, `/reload-plugins` etc.). Picker commands take arguments instead (`/model sonnet`, `/effort high`, `/autocompact 500k`, `/advisor opus`, `/output-style`, `/focus`). `/mcp` gives a text summary on mobile; `reconnect/enable/disable` subcommands work. `/config key=value` on mobile; opens Settings on web. A `!` line from the device is sent as a message, not shell.
- Status: GA
- Evidence: https://code.claude.com/docs/en/remote-control.md (Limitations; Authorize a connector again from your shell)
- Notes: Re-authorizing a claude.ai connector requires `claude mcp login "<name>" --no-browser` on the host, then `/mcp reconnect`.

### Remote Control auto-connect setting
- What: `remoteControlAtStartup` (or `/config` "Enable Remote Control for all sessions") with true/false/default. Honoured as true only from user or managed settings; project/local settings can only force false, so a checked-in file can't enable RC for everyone who opens the repo. Each interactive process registers one remote session; server mode for many.
- Status: GA
- Evidence: https://code.claude.com/docs/en/remote-control.md (Enable Remote Control for all sessions)
- Notes: `disableRemoteControl` setting turns it off entirely; ZDR and HIPAA orgs can't enable it.

### `--cloud`: start a cloud session from the terminal
- What: `claude --cloud "task"` creates a new cloud session for the current repo. The VM clones the GitHub remote at the current branch, not the local checkout. CLI shows a live provisioning checklist and queues messages typed meanwhile. One repo at a time; each invocation is an independent parallel session. `--remote` is a deprecated alias. If the branch isn't on GitHub, it starts from a pushed ancestor or the default branch and says so.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (Start a cloud session from your terminal; Branch is not on GitHub)
- Notes: Requires claude.ai sign-in (not API key or Console), no third-party provider (an `ANTHROPIC_BASE_URL` gateway is tolerated), and org policy `allow_remote_sessions`. Recommended pattern: plan locally in plan mode, commit the plan, then `--cloud` to execute.

### Local repo bundle upload for `--cloud`
- What: With no git remote, or a github.com repo without the Claude GitHub App, the CLI bundles the local repo (all branches' history plus uncommitted changes to tracked files) and uploads it. `CCR_FORCE_BUNDLE=1` forces it. Limit under 100 MB, falling back to current branch only, then a squashed working-tree snapshot, then failure. Untracked files excluded. On macOS/Linux/WSL, uncommitted changes to credential-like files (`.env`, `*.tfvars`, `id_rsa`, `*.pem`) and LFS/filter-managed files are left out with a "Left on this machine:" notice; native Windows uploads everything as is. Requires git 2.31+ and a plain clone layout (refuses submodule, `--separate-git-dir`, `--shared`, `--reference`, `core.worktree`, reftable, sparse linked worktree, in-tree include configs).
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (Send local repositories without GitHub)
- Notes: The only way to use GitLab/Bitbucket with Anthropic-hosted cloud sessions; such sessions can't push back.

### Follow-ups to a running cloud session from any CLI
- What: `claude -p "msg" --cloud <session-id|url>` (or stdin) queues one message and exits without waiting; works from any logged-in machine, sends no local state. `--output-format json` gives `{ok, session_id, url}` or `{ok:false, session_id, error}`; stream-json unsupported. Without `-p` it's rejected ("Attaching ... not enabled").
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (Send follow-ups from the CLI; Errors when sending to a cloud session)
- Notes: Accepts `session_...` or `cse_...` IDs. Refuses to send if org policy hasn't loaded rather than assuming allowed (fail closed).

### Cloud environments (saved config objects)
- What: An environment holds network access level, env vars (`.env` format), network secrets, and a setup script. Same environments apply from Desktop, mobile, web, `--cloud`, cloud scheduled tasks and Claude Tag. Personal environments plus Owner-created organization-shared environments (read-only to members) under Personal/Organization headings. Archive only (no delete) for Anthropic-hosted ones; archiving blocks new sessions but running ones continue. CLI picks via `/remote-env` (saved to `remote.defaultEnvironmentId`), else Anthropic-hosted, else first non-bridge environment; `--environment ccpool_...` overrides for self-hosted (v2.1.224+; rejects `env_` IDs).
- Status: GA
- Evidence: https://code.claude.com/docs/en/cloud-environments.md (The Default environment; Configure your environment; Select an environment from the CLI; Archive an environment; Organization-shared environments)
- Notes: Remote Control registers a "bridge environment" representing your own machine in the environment list. A `ccpool_` ID in `remote.defaultEnvironmentId` follows a stricter settings-source rule. Default environment: Trusted network, no vars, no script.

### Setup scripts and environment caching
- What: Bash script run as root on Ubuntu 24.04 before Claude Code launches; up to 64 KB from file. Must exit zero (else session fails or starts degraded), should finish in about five minutes, runs before env vars are set. If it completes in time, the filesystem is snapshotted and reused for later sessions (skipping the script); processes are not kept. Cache rebuilds when the script or allowed hosts change, or after about seven days. Script doesn't rerun on idle restore. Contrast with SessionStart hooks: configured in repo settings, run after launch on every start including resume, local and cloud; scope to cloud with `CLAUDE_CODE_REMOTE=true`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cloud-environments.md (Setup scripts; Script requirements; Environment caching; Setup scripts vs. SessionStart hooks; Install dependencies with a SessionStart hook); https://code.claude.com/docs/en/web-quickstart.md (New sessions hang or time out during setup)
- Notes: Replacing the base image is "not supported yet"; use docker compose for custom images. Docs include an idempotent hash-guarded `npm install` hook example.

### Anthropic-hosted VM image, resources and time limits
- What: Fresh VM per session, Ubuntu 24.04 x86_64. Pre-installed: Python 3 (pip, poetry, uv, ruff...), Node 20/21/22 (22 on PATH; npm, yarn, pnpm, bun), Ruby 3.1-3.3, PHP 8.3, OpenJDK 21, Go, Rust, GCC/Clang/cmake, Docker and compose, PostgreSQL 16 and Redis 7 (not started), Chrome for Testing (`CHROME_PATH`), git, gh, jq, ripgrep, tmux. `check-tools` command lists versions. Approximate ceilings: 4 vCPU, 16 GB RAM, 30 GB disk. Bash defaults: 2 min foreground, up to 10 min; timed-out commands move to background for up to 30 more minutes (except `sleep`); SessionStart command hooks 600 s.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cloud-environments.md (What's available in cloud sessions; Installed tools; Resource limits; Time limits)
- Notes: No shell access into the VM; Claude runs every command. Limits "may change over time".

### Self-hosted environments: architecture
- What: Three objects: Environment (named destination grouping runners, created in claude.ai admin, API name `pool`, ID `ccpool_...`), Runner (customer-run process, like a self-hosted CI runner, part of the standard `claude` binary as `claude self-hosted-runner`), Session (one task; executed as a child Claude Code process spawned by the runner). Anthropic's control plane queues the session on the environment; a runner claims it, clones the repo, spawns the child. Every connection is outbound from the customer network; Anthropic never connects in. Control plane, queueing, orchestration, and the claude.ai UI stay Anthropic-hosted.
- Status: beta (public beta, Team and Enterprise, off by default)
- Evidence: https://code.claude.com/docs/en/self-hosted-environments.md (How self-hosted environments work; Key concepts; What stays on your infrastructure)
- Notes: Enabled by Owner toggle "Allow self-hosted environments"; requires cloud sessions enabled; unavailable with ZDR or HIPAA. Surfaces: web, mobile, desktop, scheduled tasks, `--cloud`, `--environment` dispatch, Claude Tag (without Access bundles). Not yet: Claude Security, Code Review. Billing same as hosted.

### Self-hosted session lifecycle: claim, lease, heartbeat, requeue
- What: Runner with free capacity claims a session and holds a lease; the child streams events over HTTPS while the runner keeps polling, each poll refreshing the lease and acting as heartbeat. Lease lapses after about 60 s without polls; requeue can take minutes. Poll request timeout 10 s; on timeout, loss, or unparseable response the runner keeps serving and retries after 1-2 s with doubling backoff up to 20 s, shortened near lease expiry. Malformed responses (e.g. captive portal) are rejected rather than read as empty queue (fixed v2.1.246).
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments.md (Session lifecycle); https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Troubleshooting, malformed poll response)
- Notes: A command cut off by a lost runner can run again on resume; Claude sees the tool call with an "outcome unknown" result (https://code.claude.com/docs/en/self-hosted-environments-deploy.md, Additional limitations).

### Self-hosted network paths and egress list
- What: Runner polls `api.anthropic.com` and posts setup-progress and failure events; runner clones/pushes to your git host (HTTPS or SSH); child holds the session event stream to `api.anthropic.com` (SSE; proxies must not buffer; stream `GET /v1/code/sessions/<id>/worker/events/stream`, open up to an hour, keep-alives at least every 15 s), plus inference and git. Required: `api.anthropic.com:443` and your git host. Conditional: `api.github.com`, `downloads.claude.ai`, `storage.googleapis.com`, `code.claude.com`, `*.frame.claudeusercontent.com`, npm, registries, Datadog (opt-in), Bedrock/Vertex endpoints. Not needed: statsig, sentry, claude.ai, mcp-proxy. Corporate proxies and mTLS env vars honoured and inherited by sessions; IP allowlisting is not a control.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments.md (Network paths); https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Network requirements; A session is picked up, but Claude never answers its first message)
- Notes: Docs give a log-line table (`SSE connect failed`, `SSE liveness timeout`) for diagnosing buffering proxies.

### Self-hosted deployment recipes and sizing
- What: No pre-built image; Dockerfile example (debian slim, git, jq, python3 for Claude Tag hooks, binary from `downloads.claude.ai` by version, linux-x64/arm64/musl). Kubernetes Deployment (3 replicas, capacity 4, secret volume, probes, grace 90 s) and Compose (restart always, grace 90 s, evaluation only because restarts reuse the filesystem). Sizing per session: 4 GiB memory request=limit, 2 CPU request/4 limit; capacity doesn't divide resources so one session can OOM others; pids limits explained with measurement steps. Version pinning: child runs the runner's own binary with auto-update off.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Build the runner image; Size CPU and memory for sessions; Kubernetes; Docker Compose; Pin the version)
- Notes: Docs flag that their own recipes (capacity 4) don't meet their hardening advice (capacity 1).

### Pre-warmed checkouts
- What: Without a checkout hook, the runner keeps one canonical clone per repo at `<base-dir>/<owner>/<repo>`; capacity 1 fetches and hard-resets (never `git clean`, so untracked files from earlier sessions of the same owner persist); capacity >1 uses per-session worktrees. Supply a full (not shallow) clone in the image or a persistent volume on pre-locked runners. Per-session dirs under `_sessions/` (config with transcript copy, uploads) persist unless `--remove-session-state`. Default fresh-clone depth 50 (`CLAUDE_RUNNER_FETCH_DEPTH`).
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Reuse a pre-warmed checkout; Confirm that the runner uses your clone)
- Notes: With the git proxy the clone's `.git` is sanitized before each session (index deleted, reftable dir removed).

### CI dispatch and end-to-end testing of runner images
- What: `claude -p "<prompt>" --environment ccpool_... [--ref <branch>] --output-format json` creates a session from a checkout and exits (`{"ok":true,"session_id",...}`); follow-ups via `-p --cloud`. Read-back recipe: a Stop hook on the test runner appends `last_assistant_message` to `$E2E_REPLY_DIR/<session>.txt` (or POSTs it to a URL). Environments can be created and deleted programmatically: `POST/DELETE https://api.anthropic.com/v1/code/runners/self-hosted/pools` with header `anthropic-beta: ccr-byoc-2025-07-29` and an Owner's claude.ai OAuth token; response contains `pool_secret`.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-testing.md (all sections)
- Notes: No machine identity: dispatch needs a human claude.ai OAuth login; the `user:sessions:claude_code` scope is capped at 30 days; `setup-token` and the environment secret don't work. `CLAUDE_CODE_OAUTH_REFRESH_TOKEN` + `CLAUDE_CODE_OAUTH_SCOPES` for ephemeral CI.

### GitHub Action with Bedrock, Vertex (Agent Platform), Foundry
- What: Set `use_bedrock`, `use_vertex`, or `use_foundry` and trust GitHub's OIDC token on the cloud side, so no static cloud credential is stored. Bedrock: an IAM OIDC provider for `token.actions.githubusercontent.com` with audience `sts.amazonaws.com`, a role scoped by `repo:org/repo:*`, and a policy for InvokeModel and inference profiles. GCP: a Workload Identity Pool and a service account with `roles/aiplatform.user`. Azure: an Entra app or managed identity with a federated credential and the `Azure AI User` role. The GitHub identity can be the official app, a custom app (`APP_ID` and `APP_PRIVATE_KEY`, used with `actions/create-github-app-token`), or `GITHUB_TOKEN`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/github-actions-cloud-providers.md (Choose your provider; Set up the integration)

### GitLab CI/CD integration
- What: No packaged action. The user adds a job to `.gitlab-ci.yml` that installs the CLI (`curl -fsSL https://claude.ai/install.sh | bash`) and runs `claude -p "$AI_FLOW_INPUT" --permission-mode acceptEdits --allowedTools "Bash Read Edit Write mcp__gitlab"`, optionally after starting a GitLab MCP server. Mention-driven triggers are left to the user: a project webhook on "Comments (notes)" goes to their own listener, which calls the pipeline trigger API with `AI_FLOW_INPUT`, `AI_FLOW_CONTEXT`, and `AI_FLOW_EVENT`. GitLab API access uses `CI_JOB_TOKEN` or a Project Access Token with `api` scope. Bedrock and Vertex work through GitLab `id_tokens` OIDC (`aws sts assume-role-with-web-identity`, or a WIF credential-config file used through ADC). Foundry is not documented here.
- Status: beta (the page says it is maintained by GitLab, not Anthropic)
- Evidence: https://code.claude.com/docs/en/gitlab-ci-cd.md (Setup; Manual setup; Configuration examples)
- Notes: The "sandboxed execution ... strict network and filesystem rules" claim in How it works is not backed by anything in the example jobs (`vendor-claim`).

### Claude Code in Slack (legacy, individual plans)
- What: The Claude Slack app routes @mentions in channels (not DMs) to Claude Code cloud sessions on claude.ai/code, running under the mentioning user's own account, repositories, and plan limits. Routing mode is "Code only" or "Code + Chat" (an intent classifier picks the path, with a "Switch to code repo..." escape). Context is the thread, or the channel's recent messages plus its name, topic, and description. The repository is auto-selected, or the user is asked. Progress updates appear in the thread. On completion there are "View session" and "Create PR" buttons (the latter opens GitHub's compare page) plus a DM. Access control: workspace admin install, Enterprise Grid distribution, and which channels Claude is `/invite`d to. For Team and Enterprise, sessions are visible to the org.
- Status: legacy. It works only for Pro, Max, and Ultra in workspaces not connected to Claude Tag. In connected workspaces it is retired effective October 5, 2026.
- Evidence: https://code.claude.com/docs/en/slack.md (warning banner; How it works; Access and permissions; Troubleshooting; Current limitations)
- Notes: The page warns that Claude may follow instructions found in other messages in the context, so it should be used only in trusted conversations. Limits: GitHub only, one PR per session.

### Claude Tag session model and sandbox lifecycle
- What: Each Slack thread is bound to its own session with its own ephemeral sandbox, which runs the same engine as Claude Code on the web. A channel worked at top level also has a channel session that reads top-level messages and starts thread sessions for real tasks. Lifecycle: tag → sandbox built → work loop (a checklist message edited in place) → result in the thread (reply, file or chart, a page kept current, or a hosted claude.ai artifact; draft PRs come from the Claude GitHub App) → quiet period. A few minutes after a turn, the sandbox is released while the transcript is kept on Anthropic's side, and the next reply resumes in a fresh sandbox. Files that existed only in the sandbox do not survive. The channel session is replaced after about 1 hour idle, after a config change, or at about 1 day old once quiet. `@Claude !restart` archives a session and rereads the thread. Anyone in the thread can steer by replying, and Stop keeps the session. Edits reach Claude as before/after notes. Deleting a reply does not reach Claude. Deleting the thread's first message closes the session (archived, not deleted). With "Respond automatically" on, Claude stops reading top-level messages after about 100 messages without it posting.
- Status: public beta
- Evidence: https://claude.com/docs/claude-tag/concepts/how-it-works.md (Lifecycle of a request; Reply in the thread to steer; What survives between replies; Conversation context); https://code.claude.com/docs/en/claude-tag.md (Where Claude Tag runs)
- Notes: Thresholds are stated as approximate defaults. Mentioning Claude mid-thread gives it only a window of the thread, with other bots' messages filtered out.

### Which settings reach a cloud session
- What: Cloud sessions read only the committed `.claude/settings.json` (single-repo sessions outside project threads) and server-managed settings; user and local files and device MDM/file policy do not reach them. Multi-repo Anthropic-hosted sessions read only `enabledPlugins` and `extraKnownMarketplaces` from each repo. Project threads apply none of a repo's permissions, hooks or env. Self-hosted environments also read the managed settings file in the runner image. `permissions.defaultMode` is honoured only for `acceptEdits`, `plan`, `default`, `auto` in cloud sessions. `/config` in the browser opens claude.ai settings instead.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings.md (Settings in cloud sessions); https://code.claude.com/docs/en/settings-reference.md (permissions.defaultMode)
- Notes: Claude Tag sessions run in cloud environments but do not receive server-managed settings (managed-settings.md, Where and when a policy applies).

### Gateway deployment topologies and operations
- What: Single stateless Linux native binary (no Windows server; macOS dev only; refuses to run under Node) coordinating via PostgreSQL 11+ (single host URL; Postgres-protocol-compatible DBs unsupported). Boot is fail-closed on config, Postgres, OIDC discovery and upstream client build; migrations run at boot under an advisory lock, append-only so rollback is safe. Deploy as a Kubernetes Deployment (no Helm chart), ECS Fargate, EKS, Cloud Run (`min-instances: 1`), or Compose; not Lambda/Cloud Functions. Own container image built from a pinned, GPG-manifest-verified release binary, glibc base, writable `CLAUDE_CONFIG_DIR`. SIGTERM drain window 25 s default (`CLAUDE_GATEWAY_DRAIN_TIMEOUT_MS`). SSE `ping` every about 15 s of silence on non-Anthropic upstreams (v2.1.229+). Connection-pool sizing formula and large-rollout rate-limit sizing worked out.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway.md (Quickstart); https://code.claude.com/docs/en/claude-apps-gateway-deploy.md (Deployment; Container image; Kubernetes; Cloud Run; Upgrades; Postgres; Large rollouts)
- Notes: AWS (ECS Fargate/EKS, RDS, Secrets Manager, internal ALB, Okta) and GCP (Cloud Run/GKE, Cloud SQL private IP, Secret Manager, Google Workspace) worked examples are explicitly "a working example... rather than a supported production deployment"; companion `setup.sh` and Terraform in `anthropics/claude-code` `examples/gateway/aws` and `/gcp`. Security fixes reach the gateway only when the operator bumps the pinned image.

### Gateway and non-interactive / CI use
- What: No service-token flow; gateway sign-in is always the browser device flow, so CI must use the provider directly and should not get the force-login keys (or API-key sessions exit at startup). Once a developer has signed in, `claude -p` and Agent SDK sessions on that machine use the gateway session and policy.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway.md (CI pipelines and remote machines)

### Hosting patterns: subprocess-per-session
- What: Each `query()` spawns a `claude` subprocess spoken to over stdio that owns the shell, cwd and transcripts; one session = one subprocess, so concurrency is bounded by host RAM (`agents per host = (RAM - overhead) / per-session peak RSS`; starting point 1 GiB RAM, 5 GiB disk, 1 CPU per agent). Four lifecycle patterns: ephemeral (container per task), long-running (container hosts many live sessions, TS `streamInput()`/`startup()`/`prewarm()`, Python `ClaudeSDKClient`), hybrid (ephemeral containers hydrate from a SessionStore; store mandatory), multi-agent container (separate cwds and isolated settings). Horizontal scale: pin sessions to containers by consistent hashing on `sessionId`. The subprocess never listens on the network; inbound auth belongs at a gateway.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/hosting.md (The subprocess model; Choose a session pattern; Resources; Scaling and concurrency; Known limitations)
- Notes: Known limitations stated: no top-level session timeout, memory grows over long sessions (recycle processes), wide parallel subagent fan-outs hit rate limits, no per-subagent wall-clock deadline (only `maxTurns` and a stall watchdog). Bundled CLI is pinned to the SDK version; SDK follows semver. Docs claim token cost usually dominates container cost by an order of magnitude (~$0.05/h container).

### Pre-warming and spare processes (TS)
- What: `startup()` spawns the CLI and completes the initialize handshake before a prompt exists (default `initializeTimeoutMs` 60000), returning a one-shot `WarmQuery`. Alpha `prewarm()` (v0.3.282+) starts an unbound spare in a temp dir and `claim({prompt, options:{cwd,...}})` binds it later; only some options (cwd, model, permissionMode, settings overlay, appendSystemPrompt, agents, env tokens...) can be set at claim, the rest (mcpServers, hooks, canUseTool, settingSources, systemPrompt, plugins) are fixed per spare. A spare holds about 230-260 MB. Claims can be refused (`not_claimed`, `option_not_applied`).
- Status: GA (`startup()`), alpha (`prewarm()`)
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (startup(); prewarm(); SpareProcess)
- Notes: Gives a concrete per-process memory figure for the CLI.

### Custom process spawning (remote / container transport)
- What: TS `spawnClaudeCodeProcess(SpawnOptions) => SpawnedProcess` lets the host run the CLI in a VM, container or remote machine; the contract is a Node `ChildProcess`-like object with `stdin`/`stdout` streams, kill and exit events. `SpawnOptions` = `command`, `args`, `cwd`, `env`, `signal`. On abort the SDK first closes stdin and waits about 2 s for clean shutdown before firing the signal.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (Options; SpawnedProcess; SpawnOptions)
- Notes: Confirms the transport is plain stdio streams of JSON messages, so any byte pipe (ssh, vsock, container exec) can carry it.

### Self-hosted environments (runner)
- What: `claude self-hosted-runner` turns your own machines or containers into the place where web, mobile and desktop cloud sessions run. Public beta on Team and Enterprise from 2.1.224 (Aug 7, 2026); guided `setup`.
  - Operational flags: `--defer-shutdown-max-min`, `--proxy-authorization-command` / `-file`, `--client-label`, `--drain-marker-file`, `--remove-session-state`, `--hold-idle-file` / `--max-hold-min`, `--retire-at`, `--checkout-hook-timeout-min` (default 30 minutes, 2.1.307), and orchestrator `--shutdown-drain-sec`.
  - Server-supplied hooks (2.1.229).
  - A refused on-demand runner exits 0 (2.1.321).
  - `CLAUDE_CODE_DISABLE_NESTED_SESSION_ID` (2.1.317).
- Status: public beta
- Evidence: https://code.claude.com/docs/en/whats-new/2026-w32.md, https://code.claude.com/docs/en/whats-new/2026-w34.md; https://code.claude.com/docs/en/changelog.md (2.1.224, 2.1.229, 2.1.238, 2.1.248, 2.1.268, 2.1.271, 2.1.299, 2.1.307, 2.1.311, 2.1.317, 2.1.321); own pages self-hosted-environments*.md
- Notes: the closest analogue to a master/node split. Anthropic's cloud is the control plane and the customer's runner executes the loop.

### Remote Control
- What: `claude remote-control` makes a machine a device card in the mobile app's Code tab. It left research preview in Week 34.
  - Settings per started session: `--settings` (2.1.321).
  - Forking a remote-control session from the app (2.1.273).
  - Live streaming of foreground subagent tool calls (2.1.251).
  - Project settings can no longer turn on auto-connect (Week 32).
- Status: GA (out of research preview, Week 34)
- Evidence: https://code.claude.com/docs/en/whats-new/2026-w34.md, https://code.claude.com/docs/en/whats-new/2026-w32.md; https://code.claude.com/docs/en/changelog.md (2.1.251, 2.1.273, 2.1.300, 2.1.321)

### Headless and CI controls
- What:
  - `--restricted` / `CLAUDE_CODE_RESTRICTED=1` (2.1.248): removes command- and code-running tools and WebFetch, confines file tools, loads only managed and `--settings`, and refuses bypass.
  - `--forward-subagent-text` (2.1.211).
  - `CLAUDE_CODE_MCP_STARTUP_WAIT_MS` (2.1.274).
  - stream-json `mcp_server_errors` (2.1.219) and `plugin_errors.path` (2.1.283).
  - A stderr line says what a `-p` run is waiting for (2.1.295).
  - Credential-expiry fields `credential_expires_at` / `loginExpiresAt` (2.1.318).
  - `--system-prompt-snapshot off` (2.1.267).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.211, 2.1.219, 2.1.248, 2.1.267, 2.1.274, 2.1.283, 2.1.295, 2.1.318); https://code.claude.com/docs/en/whats-new/2026-w35.md

## Area 18. Collaboration, teams and enterprise administration

### Collaboration and enterprise on these pages
- What: Only cross-references: managed settings override user/project settings; `/team-onboarding` builds a guide from 30 days of usage and can share a link on Team/Enterprise; `/usage-credits` lets members without billing access request credits from admins; org policy can disable voice, WebFetch, feedback, `/design`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/commands.md (/team-onboarding, /usage-credits); https://code.claude.com/docs/en/best-practices.md (Choose who a rule applies to)
- Notes: Detailed admin docs are in another slice.

### Managed permission policy and precedence
- What: Managed settings sit at the top of precedence; no level, including CLI flags, can override a managed permission rule. A deny at any level cannot be allowed by any other level (`--allowedTools` cannot beat a managed deny; `--disallowedTools` can add). `allowManagedPermissionRulesOnly` makes managed settings the only source of rules. `disableBypassPermissionsMode` / `disableAutoMode` set to `"disable"` lock those modes out, from any scope. Embedding hosts can pass extra managed policy via the SDK `managedSettings` option.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Managed settings; Settings precedence; Permission modes)

### Enforcing and locking the sandbox org-wide
- What: Managed `sandbox.enabled`, `failIfUnavailable`, `allowUnsandboxedCommands: false` enforce it. Managed booleans win; arrays merge so developers can widen unless locked. Weakening keys (`enableWeakerNestedSandbox`, `enableWeakerNetworkIsolation`, `network.allowAllUnixSockets`, `network.allowLocalBinding`, `allowAppleEvents`, `allowNestedUserNamespaces`) should be set `false` in managed. `allowManagedReadPathsOnly` and `allowManagedDomainsOnly` lock reads and domains to managed values. "Admin-required" sandbox (managed or `--settings` `allowUnsandboxedCommands: false`, or managed `allowManagedDomainsOnly`) makes Claude Code ignore loosening keys in repository settings: `excludedCommands`, `ignoreViolations`, `allowedDomains`, Unix sockets, Mach lookups, proxy ports, `allowWrite`/Edit allow/`additionalDirectories` write grants, `WebFetch(domain:)` hosts for the sandbox, weakening booleans, repo `false` for enabled. Narrower locks without admin-required (v2.1.285+): managed/`--settings` denied domains block repo proxy ports; `strictAllowlist` blocks repo domain/port entries; managed read denies block repo allowRead/allowWrite under those paths.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (Enforce sandboxing with managed settings; Keep developers from widening the policy; Repository settings under an admin-required sandbox; Locks that apply without an admin-required sandbox)
- Notes: Windows fleets: `failIfUnavailable` makes Claude Code exit on native Windows. Developers' own `~/.claude/settings.json` can still loosen most keys (no managed-only lock for `excludedCommands`, `allowWrite`). `!` commands remain unsandboxed in most sessions.

### HIPAA configuration: eligibility, server-pushed org policy, feature restrictions
- What: An Enterprise org setting for BAA holders that applies to Claude Code (local mode: terminal and Desktop Code tab) and Cowork; VS Code/JetBrains extensions keep working but are not covered. Takes effect only for Claude Enterprise sign-ins connecting directly to the Claude API (not cloud providers, apps gateway, LLM gateways/custom `ANTHROPIC_BASE_URL`, `ANTHROPIC_AUTH_TOKEN`/`apiKeyHelper` without an Enterprise sign-in, Console keys or federation credentials). Claude Code fetches the org policy from `api.anthropic.com` at start and about hourly. Requires v2.1.285+ (Desktop v2.19675.0+); servers reject older versions with an API error and the minimum rises over time. Effects: WebFetch off (web search still works), `--cloud`/`/teleport`/Remote Control refused, `/feedback`/`/bug` off, artifact publishing off, MCP tool search off (tools load upfront), Anthropic credentials (`ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`) stripped from shell commands, hooks and MCP servers (cloud/GitHub creds are not), status persists until restart even after `/login` elsewhere; built-in start mode becomes Manual. Startup notice and a `HIPAA configured` footer tag; `/status` shows it.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hipaa-setup.md (Check how developers sign in and connect; Update Claude Code and Claude Desktop; Allow network access; What developers see in Claude Code; Anthropic credentials in commands, hooks, and MCP servers; Confirm the configuration on a computer); https://code.claude.com/docs/en/permission-modes.md (Permission modes with the HIPAA configuration)
- Notes: Recommended managed keys: `forceLoginMethod: "claudeai"`, `forceLoginOrgUUID`, `allowedProviders: ["anthropic"]` (v2.1.285+; refuses to start on providers/gateways), `cleanupPeriodDays`, plus `requiredMinimumVersion: "2.1.285"` for v2.1.163-v2.1.284. With force-login keys set, sessions using `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` or `apiKeyHelper` are refused. A wrong org UUID makes Claude Code exit for everyone.

### Restricting login org and API providers
- What: Managed `forceLoginMethod` (`claudeai`, `console`, `gateway`) and `forceLoginOrgUUID` (string or array) restrict sign-in; Claude Code exits at startup if the claude.ai credential is from an unlisted org. Console logins are only pre-selected, not verified. With either key set, sessions using `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` or `apiKeyHelper` are blocked at startup (membership can't be verified); cloud provider sessions are blocked only while such leftover creds exist; profile/federation creds aren't org-checked. Every login path applies `forceLoginMethod` from v2.1.212, but the interactive login screen only pre-selects it, so a Console login can still be completed there. Server-managed settings arrive only after authenticating, so first-login redirection needs MDM. `allowedProviders` (v2.1.285+; values such as `anthropic`, `bedrock`, `customEndpoint` with a managed `env` URL pin) refuses other providers at startup and on the next request after a switch; server-managed-only lists are a convenience, not enforcement.
- Status: GA
- Evidence: https://code.claude.com/docs/en/authentication.md (Restrict login to your organization; Restrict which API providers a machine may use)
- Notes: Documented gaps: Console credential org not verified; interactive login screen does not enforce `forceLoginMethod`.

### Admin controls over hooks and customisation
- What: `allowManagedHooksOnly` blocks user/project/local/plugin hooks (force-enabled managed plugins exempt), narrows `statusLine`, `fileSuggestion`, `subagentStatusLine` to managed values, and disables `command`-source plugins and marketplace `headersHelper` unless `disableCommandPluginSources: false`. `strictPluginOnlyCustomization` can lock hooks and skills to plugins/managed sources. `disableAllHooks` outside managed settings cannot disable managed hooks. Managed `disableSkillShellExecution`, `claudeMd`, managed CLAUDE.md, managed skills and output styles directories, managed `skillOverrides` via aliases (restrict only).
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Hook locations; Disable or remove hooks; Limits on skill hooks); https://code.claude.com/docs/en/skills.md (Inject dynamic context; Override skill visibility from settings); https://code.claude.com/docs/en/memory.md (Deploy organization-wide CLAUDE.md)
- Notes: `/hooks` shows "Only hooks from managed settings run here" under the lock.

### Organization per-tool controls on connectors (`ask` / `blocked`)
- What: Org admins set per-tool controls on claude.ai connectors that Claude Code reads at startup and enforces locally. `ask`: prompt on every call with an org reason, even in `acceptEdits`/`auto`/`bypassPermissions`, no remember option, allow rules do not skip it, `dontAsk` denies. `blocked`: filtered out before the model sees it (shown in `/mcp` as `disabled by your organization`). Desktop local/SSH sessions withhold blocked tools before delivery but do not receive `ask`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Organization controls on connector tools)

### Managed MCP: exclusive control with `managed-mcp.json`
- What: A system file (`/Library/Application Support/ClaudeCode/`, `/etc/claude-code/`, `C:\Program Files\ClaudeCode\`) in `.mcp.json` format. When present, only its servers, `managedMcpServers` entries, in-process host servers, and optionally Claude in Chrome load; plugin servers, user servers and `--mcp-config` servers are refused, and fetched claude.ai connectors are suppressed unless `allowAllClaudeAiMcps` (admin tiers only). Empty `mcpServers: {}` disables MCP. `--mcp-config` with the file present exits on a workstation; in cloud sessions it silently drops host-delivered servers (warning only on stderr). `--strict-mcp-config` exits. Cannot be delivered by server-managed settings. Claude in Chrome blocked unless device-level `allowClaudeInChromeWithManagedMcp` (v2.1.282+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/managed-mcp.md (Choose a pattern; Exclusive control with managed-mcp.json; Disable https://code.claude.com/docs/en/mcp.md entirely; Allow claude.ai connectors alongside the managed set; Allow Claude in Chrome alongside the managed set; Validate the configuration)
- Notes: Docs warn the file is world-readable, so no secrets in `env`; use `${VAR}`, OAuth, or `headersHelper`. Previously configured servers vanish with no policy notice. Managed servers skip the allowlist unless they use `${VAR}` outside `env` (behaviour changed v2.1.259 and v2.1.305; docs warn admins that servers they hid via allowlist will start loading silently after upgrade).

### Managed MCP: `managedMcpServers` (provided servers)
- What: Managed-settings key (v2.1.259+) giving every user extra remote servers alongside their own. Entries must be `http`/`sse`, `https://` only (no `http://`, even localhost), no `command/args/env/headersHelper`, no `${VAR}` (not expanded), safe names and no control/invisible chars; failing entries are dropped with a `/status` notice. Outranks same-name local/project/user servers and same-URL plugin/connector servers; skips allowlist; denylist (including a user's own) still applies; users cannot remove them but can toggle off; URL shown as host only, header values hidden. Read only from admin sources (never HKCU, embedding host parent settings, or user/project files). Server-managed delivery withholds cached copies until confirmed for the session; live edits connect/reconnect/disconnect running interactive sessions without restart.
- Status: GA
- Evidence: https://code.claude.com/docs/en/managed-mcp.md (Provide servers through managed settings; What an entry can contain; How provided servers load; What users can see and change; Where managedMcpServers applies; When provided servers connect)
- Notes: Claude Desktop has a same-named key with an incompatible array shape.

### Managed MCP allowlist / denylist
- What: `allowedMcpServers` and `deniedMcpServers` are lists of single-key entries: `serverUrl` (wildcards, case-insensitive host, trailing-dot ignored, case-sensitive path; full hostname matches only the default port, a `*` in the host matches every port), `serverCommand` (exact argv array match, `env` not compared), `serverName` (exact label; docs say explicitly it is not a security control). Evaluation order: merge lists (allowlist from all scopes unless `allowManagedMcpServersOnly: true`; denylist always merges from every scope so users can self-block), deny wins absolutely, then allowlist by type (remote must match a `serverUrl` if any exist; stdio a `serverCommand` if any exist; name counts only otherwise). Unset allowlist = all allowed; empty array = none. Re-evaluated on reconnect/re-enable. Applies to `--mcp-config` servers too. Policy-entry `${VAR}` expansion uses a pinned startup environment (v2.1.219+) so project/user settings cannot change what an allowlist entry means; an allow entry whose expansion would change scheme/host/path scope is ignored, a deny entry still matches. Skips: org servers, built-ins (Chrome, `ide`), Claude Tag Slack tools, `sdk` servers.
- Status: GA
- Evidence: https://code.claude.com/docs/en/managed-mcp.md (Policy-based control with allowlists and denylists; Match servers by URL, command, or name; How a server is evaluated; Servers that skip the allowlist check; Restrict the allowlist to managed settings only; How restrictions appear to users)
- Notes: Also: `strictPluginOnlyCustomization` with `mcp` allows only plugin-provided servers. No built-in MCP registry; docs suggest a managed plugin marketplace as the catalog. Blocked servers silently disappear from `/mcp`. `OTEL_LOG_TOOL_DETAILS=1` adds MCP server/tool names to OpenTelemetry events for usage monitoring.

### Organization plugin policy (managed settings)
- What: Delivery via server-managed settings (claude.ai admin console, Owner role, one config per org), MDM (plist / registry JSON), or `managed-settings.json` (+ `managed-settings.d/` drop-ins); by default only the first source that delivers any policy key applies (server > MDM > file) unless `managedSourcesBehavior: "merge"`. Keys: `extraKnownMarketplaces` + `enabledPlugins` (force-install/enable or block-and-hide); `strictKnownMarketplaces` (allowlist of sources, alias `allowedMarketplaces`; `[]` blocks all incl. official), `blockedMarketplaces` (checked first, wider canonicalised matching); `disableSideloadFlags` (rejects `--plugin-dir`, `--plugin-url`, `--agents`, SDK `plugins`, `CLAUDE_CODE_PLUGIN_DIRS`, and non-sdk `--mcp-config`); `disableCommandPluginSources`; `allowManagedHooksOnly`; `strictPluginOnlyCustomization` (`true` or subset of `skills/agents/hooks/mcp`); `syncClaudeAiPlugins`; `pluginSuggestionMarketplaces`; `pluginTrustMessage`; `allowedChannelPlugins`; `allowManagedModsOnly`; env `CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL`. Lists apply before any download and again at session start (already-installed plugins from now-disallowed sources stop loading). Managed `autoUpdate` locks the user toggle.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/org.md (Choose a delivery mechanism; Require a marketplace and its plugins; Control matrix; Allowlist with strictKnownMarketplaces; Blocklist with blockedMarketplaces; Allow the official marketplace and your own; Set update policy)
- Notes: Allowlist matching is exact (ref/path must match; `.git` suffix or ssh form differ); docs recommend `hostPattern`. A machine that once ran under a policy blocking the official marketplace remembers it and never auto-retries. Documented gaps: no per-user/group targeting from server-managed settings, no per-entry restriction inside an allowed marketplace except `enabledPlugins: false`, no key to hide `/plugin`.

### Repository-required plugins and release channels
- What: A repo's `.claude/settings.json` can set `extraKnownMarketplaces` (applied only after the workspace trust dialog; silently ignored otherwise; `-p` honours only already-trusted folders) and `enabledPlugins` (does not install; each contributor runs `claude plugin install --scope project`). Release channels are done by hosting two marketplaces pointing at different refs and giving user groups different endpoint-managed settings or gateway policies.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/org.md (Require plugins per repository; When each surface applies the plugin keys; Assign release channels to user groups)

### Plugins synced from claude.ai accounts
- What: Plugins a user or their org turns on for their claude.ai account load in Claude Code as `<name>@synced`, with skills, agents, hooks, MCP and LSP servers and the same trust as an installed marketplace plugin. Terminal sessions sync once in the background per launch (v2.1.273+); changes need `/reload-plugins`. Not synced with API-key/token/`apiKeyHelper` auth, without feature-flag fetching, in bare or `--safe-mode`, or when `--setting-sources` omits `user`. Opt out per plugin (`"<name>@synced": false`) or all (`syncClaudeAiPlugins: false`, moves synced plugins to `.trash/`); org-required plugins cannot be disabled.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/loading.md (Plugins synced from claude.ai; Control which synced plugins load)

### Distribution through claude.ai org settings and Anthropic's directory
- What: Team/Enterprise orgs can sync a marketplace repo through their GitHub/GitLab connection in claude.ai Organization settings > Plugins & skills (repo must be private/internal on github.com/gitlab.com; plugins with a top-level `bin/` are rejected); those reach Claude Code as `@synced`. Anthropic's directory (claude.ai/directory) takes submissions from paid plans via a developer portal and reaches Claude Code via account sync. `claude-plugins-official` does not take portal submissions. Users can also add claude.ai-hosted marketplaces with `claude plugin marketplace add --claudeai <name>` (local names prefixed `claudeai-`, v2.1.273+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/host-marketplace.md (Distribute through organization settings); https://code.claude.com/docs/en/plugins/publish.md (Choose how to distribute; Submit to Anthropic's directory; How a listed plugin reaches Claude Code users); https://code.claude.com/docs/en/plugins/install.md (Add a marketplace from claude.ai)

### Mod administration: managed settings, built-in guard, ordering
- What: Built-in guard `sec-default@builtin` loads ahead of user mods when the machine has managed settings/`managed-mcp.json` or the credential is Team/Enterprise `/login`. It protects managed hooks, system prompt, managed CLAUDE.md, settings reads, managed MCP tools/descriptions, and skips user-mod hooks on `classic.*`, `prompt.compose/section/context/attachment`, `skill.prompt`, `attribution.text`, `session.compact`, `settings.read`, some `session.append` doors. Options under `pluginConfigs["cc-plugin-sec-default@builtin"].options`: `allowManagedModsOnly`, `allowModsToOverrideDenyRules` (managed only; fails closed if it can't read managed settings). Chain order: guard + `prependPlugins` + other org mods, then user mods (a mod before its dependencies), then `appendPlugins`, then other built-ins. "Organization's mod" = managed `enabledPlugins` true + marketplace declared in managed settings as an absolute local directory + plugin listed by relative path (loaded in place); anything copied to cache (GitHub/git/URL/npm) counts as a user's.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/admin.md (Stop user-installed mods from loading; Know what happens by default; Install your organization's mods and set the order; Set options on the built-in guard); https://code.claude.com/docs/en/plugins/mods/reference.md (Settings and environment variables)
- Notes: Policy spectrum: `allowManagedModsOnly` (user settings hooks keep working), `allowManagedHooksOnly` (also blocks user settings hooks), `disableAllHooks` (everything incl. managed hooks), `disableSideloadFlags` (rejects `--plugin-dir`, `--plugin-url`, `--agents`, `--mcp-config`, Claude-written mods). Setting `prependPlugins` replaces the default and must re-list the guard; repos cannot set either list. Admin docs require marketplace directory writable only by admins. Settings to stop all mods per user: disable plugin, `--safe-mode`, `disableAllHooks`. `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS` is ignored from v2.1.287.

### Policy mods (`plugin.register`, API-call interception)
- What: An org mod in `prependPlugins` receives `plugin.register` for each other module about to load, with `e.tier` and `e.uses` (events, calls, env, state from static analysis) and can `{refuse}` it; it can also hook any API call event (e.g. `fs.write`, `http.fetch`, `process.run`) to log or `{deny}` other mods' calls, using `next.origin.plugin` to attribute. A `.catch` on `plugin.register` is needed to fail closed.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/admin.md (Enforce a policy with a mod of your own; Refuse mods when your check fails); https://code.claude.com/docs/en/plugins/mods/test.md (Test a policy mod)
- Notes: Docs concede a session can run without the policy mod: three worker crashes unload all non-built-in mods (until `/reload-plugins`), and `--safe-mode` drops installed mods including org ones. Network controls: org WebFetch-disallow policy blocks `$.http.fetch`, but `CLAUDE_CODE_DISABLE_WEB_FETCH` or a WebFetch deny rule does not; `$.process.run` reaches the network unrestricted.

### Channel enterprise controls
- What: Managed `channelsEnabled` master switch (blocks all channels incl. dev flag when off) and `allowedChannelPlugins` list of `{marketplace, plugin}` that replaces the Anthropic allowlist. claude.ai Team/Enterprise: blocked until an Owner enables (admin console or managed setting). Console API-key orgs: allowed unless the org deploys managed settings. Pro/Max/Ultra without org: no checks.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/channels.md (Enterprise controls; Restrict which channel plugins can run)
- Notes: Empty `allowedChannelPlugins` blocks allowlisted plugins but the dev flag still works; unset `channelsEnabled` blocks everything. When blocked, MCP server still connects and tools work, messages just don't arrive.

### Artifact sharing, editors and comments
- What: Artifacts start private. Share control in page header: specific org members or whole org (Team/Enterprise), shared folders, or public link (off by default on Team/Enterprise). Viewer vs editor roles; editors republish via Claude by URL, and stale-copy publishes are refused so Claude merges onto the newer version. Org viewers see the author; anonymous/public viewers see "Content is user-generated and unverified". Comments: org viewers comment; Claude can read threads, and reply/resolve only threads an editor activated via "Send to Claude" or `@claude`. The publishing session watches for activated comments and auto-replies when the permission mode allows; capped at 60 per artifact per hour; Ctrl+C at idle prompt pauses, `/tasks` stop is persistent.
- Status: GA (comments v2.1.221+, auto-reply v2.1.228+)
- Evidence: https://code.claude.com/docs/en/artifacts.md (Share an artifact; Let someone edit with you; Collect comments on an artifact; Let Claude reply to comments on its own)
- Notes: Version picker controls which version viewers see; `#anchor` deep links. Comment watching requires feature-flag fetching. Publicly shared artifacts prompt once per conversation before changes and before each comment reply.

### Artifact organization administration
- What: Owners toggle artifacts org-wide (and per role with RBAC on Enterprise), toggle artifact connectors separately, enable external/public sharing (disabling later blocks existing public links without changing audiences), set separate retention periods for private vs shared artifacts, see `claude_artifact_*` audit events, and use the Compliance API (`GET /v1/compliance/apps/code/artifacts`, `GET .../{id}/versions/{version_id}`, `DELETE .../{id}`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/artifacts.md (Manage artifacts for your organization; List and delete artifacts with the Compliance API)
- Notes: Org policy is fetched from api.anthropic.com; if unreachable, artifacts are unavailable. Users can disable via `/config` (`enableArtifact: false`), `CLAUDE_CODE_DISABLE_ARTIFACT=1`, or deny rule `Artifact`; once disabled by `--settings`, env or managed, no file re-enables. Project/local `enableArtifact: true` cannot re-enable (v2.1.242+).

### Admin controls for cloud and remote features
- What: Owner toggles: cloud sessions policy `allow_remote_sessions`; Quick setup (enables `/web-setup`, skips App prompt, auto-creates Default; off by default on Team/Enterprise); GitHub connector; Remote Control toggle (off by default on Team/Enterprise); Require trusted devices; Allow self-hosted environments; hide Anthropic-hosted environments org-wide; org default environment; shared environments and their network secrets; environment status messages before restarts; Activity tab with Retry and Revoke token per session. Custom roles gate cloud sessions and RC.
- Status: GA (self-hosted parts beta; Trusted Devices beta)
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (Quick setup for Team and Enterprise); https://code.claude.com/docs/en/web-quickstart.md (Connect GitHub); https://code.claude.com/docs/en/remote-control.md (Requirements; Trusted Devices); https://code.claude.com/docs/en/cloud-environments.md (Organization-shared environments); https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Harden your deployment; Post a status message before a restart); https://code.claude.com/docs/en/self-hosted-environments-configuration.md (spawn-runner hook)
- Notes: ZDR, HIPAA and CMEK organizations can't use cloud sessions (CMEK also can't save network secrets); ZDR/HIPAA can't use RC or self-hosted.

### Claude GitHub App permission set (shared app)
- What: A single GitHub App serves the Action, Code Review, and web auto-fix, and GitHub does not let an installer accept a subset of its permissions. The full set: Actions RW, Administration R, Checks RW, Contents RW, Discussions RW, Issues RW, Members R, Merge queues R, Metadata R, Pull requests RW, Repository creation RW, Repository hooks RW, Statuses R, Workflows RW. The set can grow ahead of the features that use it. GitHub then asks the owner to approve, and the old permissions stay until they do. For least privilege, create a custom app with Contents, Issues, and PRs only. A custom app covers only the Action, not Code Review or auto-fix.
- Status: GA
- Evidence: https://code.claude.com/docs/en/github-actions.md (GitHub App permissions)
- Notes: The breadth (Workflows RW, Repository creation RW, Administration R) is a meaningful blast radius and is openly documented.

### GitHub Enterprise Server support
- What: Team and Enterprise plans. An Owner connects the GHES instance once at Organization settings > Git providers. The guided flow generates a GitHub App manifest and redirects to GHES. Fields: a display name up to 20 characters, the hostname, and an optional private CA cert. A manual flow takes app ID, client ID and secret, webhook secret, private key, optional port, CA, and read-replica hosts. Works on GHES: cloud sessions (`claude --cloud`; the host is detected from the git remote), teleport, Code Review, Claude Security (beta), and plugin marketplaces. Actions work with manual workflows only. Not supported: contribution metrics and the GitHub MCP server (use `gh auth login --hostname` instead). Anthropic-hosted sessions and hosted pre-session flows (the repo picker) need GHES reachable from Anthropic's egress IPs. Self-hosted environments clone from inside the network.
- Status: GA (Claude Security listed as public beta)
- Evidence: https://code.claude.com/docs/en/github-enterprise-server.md (What works with GitHub Enterprise Server; Admin setup; GitHub App permissions; Network requirements)
- Notes: The manifest's permission set (Contents, PRs, Issues, Checks RW; Actions and Statuses R; Repository hooks RW for marketplace sync; Members R) and events (pull_request, issue_comment, pull_request_review_comment, pull_request_review, check_run, status) are listed. An app created from an older manifest keeps its old permissions.

### Claude Tag (organisation-managed @Claude in Slack)
- What: An Anthropic-hosted Slack agent for Team and Enterprise, first-party only, not on individual plans. An Owner pairs the workspace once with `@Claude connect` and a pairing code at claude.ai/admin-settings/claude-tag, sets a spend limit, adds channels, and launches. After that, anyone in a channel can use it with no per-user setup and no Claude seat. Owners can restrict use to org members on Team, or to roles with the "Claude Tag in Slack" capability on Enterprise. Billing: channel, thread, and group-DM work draws from a usage balance funded by the org, under a spend limit. One-to-one DMs run on the sender's own claude.ai seat, or bill the org for a limited time if the sender has no account.
- Status: public beta
- Evidence: https://code.claude.com/docs/en/claude-tag.md (served as https://claude.com/docs/claude-tag/overview: Plans that include Claude Tag; Who needs a Claude seat; Billing and spend limits; Set Claude Tag up once for everyone); https://claude.com/docs/claude-tag/concepts/security-and-data.md (Member access)
- Notes: Not available with ZDR, CMEK, or the HIPAA local-mode configuration, because it retains channel memory and transcripts.

### Claude Tag cross-channel actions and visibility
- What: Claude can read other public channels it belongs to (not when the asking channel has guests) and post into them only when asked from a public channel. From a private channel or group DM it cannot post elsewhere. From a 1:1 DM it can post new top-level messages, each after "Approve and post". Every cross-posted message carries an attribution line ("Sent by Claude in #x on behalf of @y"). It cannot post to Slack Connect or cross-workspace shared channels. "Follow" watches another public channel and acts in the asking conversation; a follow set from a thread ends after 7 days without a new message. Admins have no page showing individual messages. Analytics show spend per channel, and an Activity page lists scheduled work, memory files, and network events. On Enterprise, transcripts are available through the Compliance API. Hosted artifacts are visible to anyone with access to the source channel, with no share setting.
- Status: public beta
- Evidence: https://claude.com/docs/claude-tag/concepts/how-it-works.md (What Claude can do in other channels; What admins can see of your conversations with Claude); https://claude.com/docs/claude-tag/concepts/security-and-data.md (Artifact visibility)

### Model allowlist (availableModels) and enforcement
- What: Managed `availableModels` entries match a family (`sonnet`), a version prefix (also permits later IDs extending it), or a full ID. Applies to every model-selection path: `/model`, `--model`, env, settings, resumed model, alias env redirects, `/fast`, subagent/teammate/skill/command models, advisor, background dispatch, auto-mode classifier model. Blocked family aliases substitute the newest permitted version with a notice (Anthropic API and Claude Platform on AWS); other blocks: `/model` errors, startup values replaced with a warning, subagents fall back, skill overrides ignored, advisor disabled, `--advisor` exits. Hidden from picker. A specific entry disables its family's wildcard. Managed list is never merged with lower scopes (since v2.1.175). `enforceAvailableModels: true` extends the list to the Default option. `deniedModels` and `availableModelsMatch: "exact"` (v2.1.283+, managed only) block versions a prefix would admit; Default steps down family, then cheaper family, then first list entry, else refuses to start. Fallback chains, plan-mode upgrades, content fallback and fast mode all respect it.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Restrict model selection; Default model behavior; Enforce the allowlist for the Default model; Control the model users run on; Merge behavior; Block specific models or versions)
- Notes: The `model` setting is explicitly an initial selection, not enforcement. Picker greying on claude.ai/Desktop is a convenience only. Third-party provider prefixes like `us.anthropic.` are not stripped during matching.

### Allowlist delivery coverage by surface
- What: Matrix of which delivery mechanism (server-managed settings vs MDM/managed files) reaches CLI/IDE, Desktop, cloud sessions, SDK/headless, Cowork. Cloud sessions need server-managed settings (device files do not reach Anthropic VMs); third-party providers and API-key fleets do not receive server-managed settings, so use files there. The server rejects starting a cloud session on an excluded model.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Surface coverage)
- Notes: Claude Tag (Slack) sessions do not receive server-managed settings.

### Organization model restrictions, default model and effort limits (Enterprise)
- What: Enterprise admins disable models org-wide or per custom role in the claude.ai console; delivered with entitlements and also enforced server-side at session creation; Haiku cannot be disabled; changes apply within about a minute. Org default model (org-wide or per role, most capable wins across roles) resolves the Default option; optionally overrides user `/model` choices (flag, env and managed settings still win). Per-role per-model max effort caps (least restrictive across roles); `maxEffortLevel` managed setting caps on any provider, lowest across scopes wins; per-model caps via `modelSettings`. Lower of org and client cap applies.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Organization model restrictions; Organization default model; Organization effort limits); https://code.claude.com/docs/en/settings-reference.md (maxEffortLevel)
- Notes: Org restrictions reach only Anthropic API and LLM gateway sessions; org default only Anthropic API sessions. Org service keys are exempt from restrictions. Version gates v2.1.187, v2.1.195, v2.1.196, v2.1.267.

### allowedProviders (provider lockdown)
- What: Managed list of permitted inference paths: `anthropic`, `bedrock`, `vertex`, `foundry`, `anthropicAws`, `mantle`, `customEndpoint`, `gateway`. Enforced at startup, login and next API contact. `customEndpoint` admits only the exact `ANTHROPIC_BASE_URL` (or provider endpoint var) pinned in a managed `env` block; Bedrock `AWS_ENDPOINT_URL*` overrides also need pins. Machine sources and server-managed lists intersect (server can only narrow). Empty or all-unrecognised list refuses every provider.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-reference.md (allowedProviders); https://code.claude.com/docs/en/llm-gateway.md (Roll out a gateway); https://code.claude.com/docs/en/third-party-integrations.md (Configure security policies)
- Notes: v2.1.285+. Does not judge credentials or proxy settings.

### LLM gateway rollout checklist (admin)
- What: Five steps with curl checkpoints: confirm routing per model name, issue one key per developer, test Claude Code (`claude -p`), distribute via managed settings `env` + `apiKeyHelper`, verify streaming from a developer machine. Managed `ANTHROPIC_BASE_URL` cannot be overridden by a shell export. Do not combine `forceLoginMethod`/`forceLoginOrgUUID`/`forceLoginGatewayUrl` with gateway credentials. Server-managed settings do not reach gateway sessions. Maintenance table (new betas, new models, credential rotation), retry sizing (up to 10 with backoff honouring Retry-After), and version pinning via `requiredMaximumVersion`/`DISABLE_UPDATES` because feature-flag defaults, capability assumptions and alias targets are baked into each version.
- Status: GA
- Evidence: https://code.claude.com/docs/en/llm-gateway-rollout.md (all sections)
- Notes: Gateway requirements list includes exempting the path from WAF body inspection.

### Managed settings delivery mechanisms and OS paths
- What: Four delivery paths carrying the same JSON keys: (1) server-managed from the claude.ai admin console or a self-hosted Claude apps gateway, fetched at startup and polled hourly; (2) MDM/OS policy: macOS configuration profile in the `com.anthropic.claudecode` preferences domain, or Windows `HKLM\SOFTWARE\Policies\ClaudeCode` value `Settings` (REG_SZ/REG_EXPAND_SZ JSON), checked every 30 minutes; (3) file: `managed-settings.json` (+ optional `managed-settings.d/`, + `managed-mcp.json`) in `/Library/Application Support/ClaudeCode/` (macOS), `/etc/claude-code/` (Linux, WSL), `C:\Program Files\ClaudeCode\` (Windows), reloaded on change; (4) user-writable `HKCU\SOFTWARE\Policies\ClaudeCode`, used only when no admin document is present. Symlinked files are followed; a link leaving the system directory still applies but warns in `/status` and `claude doctor`. Legacy `C:\ProgramData\ClaudeCode\` is not read. Embedding hosts can pass parent settings through the SDK `managedSettings` option.
- Status: GA
- Evidence: https://code.claude.com/docs/en/managed-settings.md (Deploy a managed settings file; Choose a delivery mechanism; Where each mechanism stores the policy)
- Notes: MDM templates for Jamf, Iru, Intune, Group Policy are linked in the anthropics/claude-code examples repo (not opened). One policy per delivery target; per-group policy only through separate files/profiles or a self-hosted gateway (per IdP group).

### Drop-in directory merge rules
- What: `managed-settings.json` is merged first, then every `*.json` in `managed-settings.d/` in alphabetical order (hidden and non-.json files ignored). Scalars: later replaces earlier. Lists: union with duplicates removed. Nested objects (`env`, `sandbox`): merged key by key recursively. `fallbackModel` and `modelPicker`: later replaces whole. `extraKnownMarketplaces` and `managedMcpServers`: same-name entry replaced whole.
- Status: GA
- Evidence: https://code.claude.com/docs/en/managed-settings.md (Split a file-based policy across teams)
- Notes: Recommended naming with numeric prefixes (`10-telemetry.json`).

### Ranking of managed sources: first-wins vs merge, cross-source keys
- What: Sources ranked: remote (server-managed/gateway) > MDM (plist/HKLM) > managed files > HKCU. Default `managedSourcesBehavior: "first-wins"` uses the highest source that carries at least one policy key and ignores the rest, silently (only `/status` shows `Skipped sources`). `"merge"` (v2.1.242+) composes all admin sources by key kind: lists union, locks take strictest, restriction allowlists (`availableModels`, `allowedMcpServers`, `allowedProviders`, `strictKnownMarketplaces`, `allowedChannelPlugins`, `fallbackModel`) are taken whole from the highest source, credential helpers/`modelPicker`/`defaultMode`/`policyHelper` only from the highest source. The merge key is read only from the highest source, so a lower source cannot opt itself in. A set of cross-source keys is read from every admin source even under first-wins: sandbox locks (`allowManagedDomainsOnly`, `allowManagedReadPathsOnly`) with allowlist union, `allowManagedMcpServersOnly`, `deniedMcpServers`, `disableClaudeAiConnectors`, sandbox binary paths, `forceRemoteSettingsRefresh`, deny-only keys, `maxEffortLevel`, commit-trailer opt-outs, and `env` merged per variable (v2.1.223+). Login keys (`forceLoginOrgUUID`, `forceLoginMethod`) come from MDM/file ahead of remote.
- Status: GA
- Evidence: https://code.claude.com/docs/en/managed-settings.md (How Claude Code combines managed sources; Keys read from every admin source; Compose every managed source; Which managed source supplies login keys); https://code.claude.com/docs/en/settings-reference.md (managedSourcesBehavior)
- Notes: A document "counts as present" even if it sets a value Claude Code cannot read, or exists but is unreadable; that blocks the HKCU fallback.

### Server-managed settings (claude.ai console)
- What: Owners/Primary Owners on Team or Enterprise edit JSON at Organization settings > Claude Code > Managed settings; all settings.json keys are supported except OS-only ones (`policyHelper`, `wslInheritsWindowsSettings`) and `managed-mcp.json`. The editor validates against the public JSON schema but saves anyway with an "Update with errors" label. Clients fetch at startup and poll hourly; a fresh sign-in waits up to 5 s for the fetch, otherwise the session opens and restrictions land a moment later. A local cache (`~/.claude/remote-settings.json`) applies on later starts, except `modelPricing`, `managedMcpServers` (waits up to 30 s), plugin-guard changes, and sensitive env vars, which wait for server confirmation. Delivery needs a direct connection to api.anthropic.com and an eligible credential (Team/Enterprise OAuth, `CLAUDE_CODE_OAUTH_TOKEN`, direct API key, `user_oauth` profile); `apiKeyHelper` keys and Workload Identity Federation do not trigger the fetch. A Console API key belongs to a different org, so CI jobs need `claude setup-token` or an endpoint-managed file.
- Status: GA
- Evidence: https://code.claude.com/docs/en/server-managed-settings.md (Configure server-managed settings; Fetch and caching behavior; Platform availability; Current limitations)
- Notes: Exporting any `CLAUDE_CODE_USE_*` provider variable or a non-default `ANTHROPIC_BASE_URL` in the shell skips the fetch entirely; a server env block cannot undo it (chicken-and-egg), and an endpoint-managed env block does not restore it because eligibility is checked first. Audit events for settings changes are available through the compliance API or audit log export.

### Fail-open by default, fail-closed options
- What: By default a failed fetch continues on the cache, or with no server policy (endpoint-managed still applies), with a warning. `forceRemoteSettingsRefresh: true` blocks startup until a fresh fetch and exits on failure; it is cached so it self-perpetuates, and is honoured from any admin source, so an MDM or file copy enforces it on first launch. `claude auth` subcommands are exempt so users can re-authenticate. Gateway clients always wait and exit if the gateway is unreachable (startup fail-closed), while the hourly background refresh is fail-open on both channels. Anthropic can also require an org's settings server-side for Team/Enterprise logins, in which case the session cannot start without a fresh or recent cached copy.
- Status: GA
- Evidence: https://code.claude.com/docs/en/server-managed-settings.md (Enforce fail-closed startup; Fetch and caching behavior); https://code.claude.com/docs/en/errors.md (Claude Code couldn't load your organization's settings; Remote managed settings failed to load)
- Notes: The fetch sends `Cache-Control: no-cache` to defeat stale proxy caches.

### Approval dialog for server-delivered risky settings
- What: In interactive sessions, server-delivered settings that run commands (`apiKeyHelper`, `statusLine`, `otelHeadersHelper`), any hook, sandbox binary paths, sandbox settings that weaken isolation or let the proxy read/reroute/authenticate traffic (v2.1.251+), and some `env` variables require the user to approve a dialog that names them; rejecting exits Claude Code. Env vars are classified: feature toggles, model selection, numeric limits apply silently; proxy/base-URL, OTLP endpoint and `CLAUDE_CODE_DISABLE_1M_CONTEXT` always need approval; four privacy toggles need approval only when set to a non-truthy value; `ANTHROPIC_CUSTOM_HEADERS` lines are inspected by header-name words (credential, tenant, routing, beta). Approval is remembered per org (claude.ai login), per gateway, or per cached payload (other credentials). Non-interactive runs (`-p`, SDK, VS Code panel, Desktop Code tab) apply such settings for that run only and do not record approval.
- Status: GA
- Evidence: https://code.claude.com/docs/en/server-managed-settings.md (Security approval dialogs; Approval memory; Environment variables and the approval dialog)
- Notes: A managed `claudeMd` no longer needs approval since v2.1.260 (instructions, not commands). Stripped invalid entries are never shown for approval nor executed.

### Invalid managed settings: repair, fail-closed, refuse-to-start
- What: Managed payloads are validated leniently: repairable entries are skipped with warnings; top-level keys with one restrictive value (`allowManagedPermissionRulesOnly`, `disableAutoMode`, etc.) read as that restrictive value when unreadable (v2.1.282+). If a `deny`/`ask` list cannot be read, `allow` and `additionalDirectories` are withheld so grants never apply without their restrictions (same for `autoMode` and sandbox deny lists). Allowlists that are invalid become empty allowlists (`allowedMcpServers`, `allowedProviders`, `strictKnownMarketplaces`, `availableModels`, hook URL lists); `crossSessionInbound` reads as `refuse`; `forceLoginOrgUUID` invalid blocks every login. Denylists that are wholly invalid are dropped rather than denying everything. `requiredMinimum/MaximumVersion` and `sandbox.failIfUnavailable` fail open by design. A managed file, drop-in, plist or HKLM value that is present but not a JSON object makes Claude Code refuse to start, even if another source is valid. Unreadable source: permission-denied starts without it; other I/O errors exit at startup.
- Status: GA
- Evidence: https://code.claude.com/docs/en/managed-settings.md (Find entries Claude Code dropped; Keys that fail closed; Invalid values inside sandbox); https://code.claude.com/docs/en/errors.md (Managed settings document could not be parsed)
- Notes: User/project/local files stay strict: invalid JSON rejects the whole file. `disableAllHooks` invalid is dropped, since enforcing it would unload the admin's own hooks.

### Policy helper (computed policy)
- What: `policyHelper` (from plist, HKLM or managed file only) names an absolute-path executable run with no arguments at startup (env `CLAUDE_CODE_VERSION`), with `timeoutMs` (default 10000, min 1000) and optional `refreshIntervalMs` (0 or >=60000). It must print one JSON object under `managedSettings` (stdout and stderr capped at 1 MiB); that object becomes the only managed source for the session, replacing MDM/file/HKCU and parent settings. Any failure (missing file, non-zero exit, timeout, oversized output, unrepairable schema violation) refuses startup for interactive, `-p`, SDK and background sessions; failed background refreshes keep the last good policy. Ignored while server-managed settings deliver a policy; runs immediately if a fetch later reports them removed.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-reference.md (policyHelper; Write the helper output; Helper failures; policyHelper.timeoutMs; policyHelper.refreshIntervalMs)
- Notes: A bare settings object without the `managedSettings` envelope silently applies nothing. Docs advise the helper to serve its own cache for outage resilience.

### Parent settings from embedding hosts
- What: A host (Desktop, IDE extension, SDK app) can pass managed settings via the SDK `managedSettings` option. They are ignored whenever an admin source is present unless the admin sets `parentSettingsBehavior: "merge"`, and then only restrictive values are kept (with the documented gap that host allow rules and sandbox allowlists still apply unless the `allowManaged*Only` locks are set). Admin `allowManagedPermissionRulesOnly` drops host allow rules; admin `forceLoginOrgUUID`, `allowedMcpServers`, `availableModels`, `strictKnownMarketplaces`, `allowedProviders` block host-supplied ones; host `blockedMarketplaces` adds to the admin blocklist. `CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST` lets a host's model/provider routing override managed model keys.
- Status: GA
- Evidence: https://code.claude.com/docs/en/managed-settings.md (Let an embedding host add policy); https://code.claude.com/docs/en/settings-reference.md (parentSettingsBehavior); https://code.claude.com/docs/en/settings.md (Exceptions to managed settings precedence)
- Notes: `/status` shows `(parent process)` when a host supplied the policy.

### Login and organization pinning
- What: `forceLoginMethod` (`claudeai`, `console`, `gateway`) restricts account type on every first-party login path (VS Code, SDK, `claude setup-token`, `/install-github-app`), except the interactive login screen, which only pre-selects. `forceLoginOrgUUID` (single UUID or array) is enforced only from a managed source; elsewhere it only pre-selects. Gateway login keys (`forceLoginGatewayUrl`, `gatewayInternalNetworks`, `"gateway"` method) are read only from on-machine sources, never server-managed. With `"gateway"`, leftover logins, API keys and helper credentials are refused.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-reference.md (forceLoginMethod; forceLoginOrgUUID; forceLoginGatewayUrl; gatewayInternalNetworks); https://code.claude.com/docs/en/managed-settings.md (Which managed source supplies login keys)
- Notes: Login keys are deliberately taken from MDM/file ahead of remote settings (v2.1.299+), since remote settings arrive after login.

### Version range enforcement
- What: `requiredMinimumVersion` and `requiredMaximumVersion` (managed-only, v2.1.163+) make Claude Code exit at startup outside the range; `claude update`, `claude install`, `claude doctor` keep working so users can recover; auto-update skips versions above the ceiling. Check is startup-only, so long-running sessions lag. `minimumVersion` (any file) only constrains updates (prevents downgrade). Invalid version strings are ignored (fail open).
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-reference.md (requiredMaximumVersion; requiredMinimumVersion; minimumVersion); https://code.claude.com/docs/en/managed-settings.md (Where and when a policy applies)
- Notes: Elsewhere than managed, these keys are ignored without warning.

### WSL inheritance of Windows policy and HKCU fallback
- What: `wslInheritsWindowsSettings: true`, honoured only from HKLM or a file under `C:\Program Files\ClaudeCode\` (both need Windows admin), makes WSL read the Windows chain, with `/etc/claude-code` (user-writable on WSL) used only when no Windows admin document exists. HKCU joins on WSL only if it also sets the key. A source holding only this key does not count as a policy source.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-reference.md (wslInheritsWindowsSettings); https://code.claude.com/docs/en/managed-settings.md (When an admin document counts as present)
- Notes: Requires v2.1.282+.

### Cowork / Desktop interaction with policy (one line)
- What: Cowork sessions never fetch server-managed settings; local Cowork reads device MDM/file policy, full-VM and remote Cowork see no device policy; claude.ai itself enforces marketplace allow/block lists there.
- Status: GA
- Evidence: https://code.claude.com/docs/en/managed-settings.md (Where and when a policy applies)
- Notes: Desktop-only detail; surveyed elsewhere.

### Managed settings delivery channels and priority
- What: Four managed sources in priority order: server-managed (claude.ai admin console, or a self-hosted Claude apps gateway for gateway sign-ins), plist/HKLM registry policy, file-based `managed-settings.json` (per-OS paths), HKCU registry (lowest, user-writable, "convenience default" not enforcement). Server-managed settings are fetched at startup and refreshed hourly. WSL reads only `/etc/claude-code` unless `wslInheritsWindowsSettings: true` is set in an admin-only Windows source.
- Status: GA
- Evidence: https://code.claude.com/docs/en/admin-setup.md (Decide how settings reach devices)
- Notes: claude.ai console delivery needs Teams/Enterprise. Mixed-provider orgs are told to combine server-managed with a file/plist fallback. Verification: `/status` "Setting sources" line names the winning source. Desktop-only WSL policy (`disableWslSessions` under HKLM, Desktop v1.19367.0+) mentioned; out of scope.

### Login enforcement (forceLoginMethod / forceLoginOrgUUID)
- What: Restricts login to a method or a specific Anthropic org; applies across terminal, VS Code extension, Agent SDK, `claude setup-token`, `/install-github-app` (all since v2.1.212; terminal-only before). When set, sessions authenticated by `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` or `apiKeyHelper` are blocked at startup. Org verification applies to claude.ai logins, not Console logins or gateway sign-in.
- Status: GA
- Evidence: https://code.claude.com/docs/en/admin-setup.md (Decide what to enforce, Login enforcement row)
- Notes: Interactive login screen only pre-selects the method, enforcement happens elsewhere.

### Version floor and required version range
- What: `minimumVersion` stops auto-update installing below an org floor; `requiredMinimumVersion`/`requiredMaximumVersion` refuse to start outside an approved range.
- Status: GA
- Evidence: https://code.claude.com/docs/en/admin-setup.md (Decide what to enforce)

### Cloud-session admin surfaces and connected GitHub accounts
- What: Owners create organization-shared cloud environments (network access level, env vars, setup script) and pick the org default environment at claude.ai admin settings. The Git providers page lists GitHub orgs/accounts linked via the Claude GitHub App, shared by Claude Code, Claude Tag and Claude Security; only the Owner role (not Admin) can open it. Linking happens via Owner connect or implicitly when a member connects GitHub (links accounts they own where the App is installed). Unlink leaves the App installed and relinks on next owner connect.
- Status: GA
- Evidence: https://code.claude.com/docs/en/admin-setup.md (Decide what to enforce; Connected GitHub accounts)
- Notes: Enterprise Compliance API emits `github_app_installation_linked`/`_unlinked` activity types. Shows an Owner vs Admin role split; no fuller RBAC description on this page.

### Seats, SSO, SCIM (pointer only)
- What: Admin page states SSO, SCIM provisioning and seat assignment are configured at the Claude account level and links out to the Enterprise Administrator Guide and a support article. A seat without Claude Code access yields "You haven't been added to your organization yet".
- Status: GA (as described by pointer)
- Evidence: https://code.claude.com/docs/en/admin-setup.md (note at top; Verify and onboard)
- Notes: Mechanism not documented in the Claude Code docs; see Unverified.

### Claude apps gateway: what it is
- What: A self-hosted service, built into the `claude` binary (`claude gateway --config gateway.yaml`), sitting between Claude Code/Claude Desktop clients and a model provider. Developers sign in with the corporate OIDC IdP and get short-lived bearer tokens; the gateway holds the upstream credential, enforces model access server-side and delivers managed settings by IdP group, relays OTLP telemetry to the org's collectors, and translates Anthropic Messages API to Bedrock, Bedrock Mantle, Claude Platform on AWS, Google Agent Platform, Microsoft Foundry or the Anthropic API with failover.
- Status: GA (no beta label on pages)
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway.md (intro; Why Claude apps gateway)
- Notes: Requires v2.1.195+ on both gateway server and developer machines. Positioned for orgs that must route through their own cloud (data residency); page itself suggests Claude Enterprise if not. Gateway sign-in replaces a Teams/Enterprise login (one sign-in at a time); gateway sessions do not fetch claude.ai server-managed settings. No separate licence or per-seat fee (deploy page).

### Gateway RBAC: managed.policies by IdP group or email domain
- What: `managed.policies` is an ordered list; each `match` (by `groups`, `email_domain`, both, or `{}` catch-all) carries a `cli` or `code` block that is a full managed-settings document. Default `first_match`: the first matching policy is merged onto the `match: {}` base. Merge rules: allow-lists (`availableModels`, `permissions.allow`) replace; deny-lists and hook arrays (`permissions.deny/ask`, `deniedMcpServers`, `blockedMarketplaces`, `deniedModels`, hooks) union so org-wide denies cannot be dropped; record keys (`env`, `modelOverrides`, `skillOverrides`) shallow-merge. `policy_mode: layered` (v2.1.316+) applies every matching policy down to the first catch-all. Served per user at `GET /managed/settings` with ETag/304.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-config.md (managed; Apply every matching policy; What goes in cli)
- Notes: A user matching no policy gets every model and no managed settings, so a catch-all is advised. Docs warn that layered mode can compose looser rules (e.g. Bash allowed + sandbox off) and refuse some combos at boot (`!` negations in Read/Edit deny rules; hook limits outside catch-all). Group matching is exact and case-sensitive, nested groups not expanded. Policies are schema-validated at boot against the gateway's bundled CLI schema; unknown keys fail boot (so a newer key needs a gateway upgrade first). Empty matcher values fail boot since v2.1.232; earlier they could silently match everyone or grant admin.

### Gateway server-side model enforcement
- What: `availableModels` is also checked at `/v1/messages`, returning 400 for non-granted models (a patched client cannot bypass it); empty list denies all. `deniedModels`/`availableModelsMatch` (gateway check v2.1.299+) block specific versions and hide them from `/v1/models`. The check also covers the session's startup default model, so admins must pair `availableModels` with `enforceAvailableModels: true` (and `model` when the list has only custom IDs) or sessions 400 until the user runs `/model`. Gateway validates `model` field type/presence before relaying.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-config.md (managed; Start sessions on a model the policy allows; What goes in cli)

### Gateway has no user directory and no SCIM
- What: The gateway authorizes each request from the IdP token's claims; it keeps no roster, so there is nothing to pre-create and no SCIM endpoint. Lifecycle runs in the IdP. Policy edits reach clients on next hourly poll; group membership changes apply at the next silent token refresh (bounded by `session.ttl_hours`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-config.md (managed, note)
- Notes: Spend-limit tables do keep last-seen email/name/groups (`principal_emails`), see spend limits.

### Gateway per-team upstream routing (`groups` on upstreams)
- What: Each upstream can carry a `groups` list so only those IdP groups reach it, including on failover (v2.1.314+). Open upstreams (no list) should be listed last. If every upstream has a list and the user is in none, the gateway returns 400 "you aren't in a group that can use this gateway" without contacting a provider.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-config.md (Send each team to its own upstream)
- Notes: Rollback hazard documented: removing only the `groups` key opens that upstream to everyone.

### Gateway spend limits (per user, group, org)
- What: With an `admin:` block, the gateway serves `/v1/organizations/spend_limits` (mirrors Anthropic Admin API wire shapes) and enforces caps live on every `/v1/messages`. Caps are `{scope, amount, period}`: scope `user` (OIDC sub), `rbac_group`, `organization`; amount in USD cents as string or `null` (unlimited), `"0"` blocks all; periods daily/weekly/monthly reset at UTC boundaries, each enforced independently. Resolution per period: per-user override, then most restrictive group cap (`group_limit_mode: max` flips), then org default, then unlimited. Group caps are per-seat defaults; `pooled_budget` (v2.1.319+) adds a shared monthly group pool with `pooled_conflict_rule` ordering. Over-cap returns 429 `billing_error`, `x-should-retry: false`, `retry-after`, reset date, plus optional `blocked_message`. `count_tokens` never blocked.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-spend-limits.md (Set a cap; How enforcement works); https://code.claude.com/docs/en/claude-apps-gateway-config.md (admin; Pooled budget order)
- Notes: Public Anthropic POST is user-only today; the gateway accepts all three scopes. Only Admin API surface served is spend limits (no increase-request queue).

### Spend metering and pricing overrides
- What: After each response a usage meter adds cost to daily/weekly/monthly counters without touching client bytes. Rate selection: `pricing.overrides` row for the serving upstream, then list price for upstream model ID, then list price of the mapped `models[].id`, then an unknown-model tier of $5/$25 per M tokens (never free). `pricing.multiplier` (0 to 10; >1 is markup, v2.1.271+). Client aborts are billed using about four characters per output token as a floor; Bedrock `CountTokens` is used to count abandoned input. Haiku 5.5 long-prompt rate applied (v2.1.293+). Rates can be pushed to clients as `modelPricing` so `/usage`, status line and OTel show contracted rates.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-spend-limits.md (How requests are priced); https://code.claude.com/docs/en/claude-apps-gateway-config.md (pricing; Send the rates to signed-in clients)
- Notes: Explicitly an estimate/circuit breaker, not an invoice.

### Spend-limit admin auth, admin API endpoints and admin UI
- What: Admin auth by named static `x-api-key` (`write_keys`, `read_keys`, keys at least 32 chars, audited as `admin-key:<id>`) or a gateway JWT whose groups include `admin.admin_groups` (audited as `oidc:<sub>`). Endpoints: list/create-or-replace/get/delete caps, `/effective` (resolved cap and to-date spend per principal, with `groups` extension, `sort=spend_desc`, `q` search), `/audit` (mutation trail with before/after). Every mutation writes an `admin_audit` row in the same transaction. The only admin UI is in Claude Desktop 4.23118.0+ for `admin_groups` members, limited to spend limits; all other config is YAML plus redeploy.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-spend-limits.md (Authenticate to the admin API; Admin API reference); https://code.claude.com/docs/en/claude-apps-gateway.md (Availability and limitations, Admin UI row)
- Notes: Page warns `q=` and `user_ids[]=` in GET query strings land in proxy access logs (PII).

### Spend-limit fail-open/closed and client-side usage warnings
- What: Pre-check queries Postgres with a two-second timeout; default fails open (request proceeds, warning logged). `enforcement.fail_closed_on_error: true` returns 429 "spend limit unavailable". Claude Code warns at 75% and 95% of the most-consumed cap, shows a Spend limit bar in `/usage` and a `rate_limits.spend_limit` object to status-line scripts; the gateway emits `anthropic-ratelimit-unified-*` headers describing the developer's own cap and strips upstream rate-limit headers.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-spend-limits.md (Postgres availability; Usage warnings in Claude Code)
- Notes: Per-feature version table (v2.1.225 to v2.1.319). The dollar-amount fetch is sent even with `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` since v2.1.295.

### Corporate launcher (processWrapper / CLAUDE_CODE_PROCESS_WRAPPER)
- What: Routes every process Claude Code starts from its own binary through a mandatory launcher: the on-demand background service, agent-view terminal hosts and sessions (including warm standbys), respawns after update/crash, self-relaunch for updates, Remote Control session processes and agent-team tmux/iTerm2 panes (last two v2.1.210+). Value is an absolute path plus optional args, parsed as an argv list (whitespace/double quotes, or a JSON array starting with `[`), no shell syntax; operators like `;` `|` `$(` are rejected. Env var v2.1.208+, `processWrapper` setting v2.1.210+; env var wins if both set.
- Status: GA
- Evidence: https://code.claude.com/docs/en/corporate-launcher.md (intro; What the launcher covers; Format of the launcher value)
- Notes: Fail-closed: if the launcher cannot run, Claude Code refuses to start the process. Ignored on Windows (no `exec`), with only a debug-log warning. Project/local settings cannot set it (repo files must not inject a binary). Delivered remotely, `processWrapper` appears in the security approval dialog. Must be set in a settings `env` block, not a shell export, because the detached service never re-reads shell profiles; `claude daemon stop --any` to restart the service; `/status` Self-exec entry and `claude daemon status` verify.

### Launcher contract
- What: Script must end with `exec "$@"`; not reorder or add Claude's args (strip its own leading args); pass every inherited env var (per-session auth tokens and provider selection travel in env); reach `exec` within about 3 seconds (can run 3 times in series before first output); tolerate nested invocation; print nothing to stdout/terminal before exec; not depend on `--flag value` vs `--flag=value`; exec from the starting directory; not read stdin; call programs by absolute path. A launcher that forks and exits gets the session marked failed and orphans reaped.
- Status: GA
- Evidence: https://code.claude.com/docs/en/corporate-launcher.md (The launcher contract)
- Notes: Not covered: user-started terminal sessions (use a `claude` shim earlier on PATH), `claude-cli://` deep links (set `disableDeepLinkRegistration`), `--worktree --tmux` relaunch, Chrome native-messaging host. Distinct from `CLAUDE_CODE_SHELL_PREFIX`, which wraps tool/hook/MCP shell commands as one quoted string in `$1`.

### Rollout kits: champion kit and communications kit
- What: Non-technical playbooks. Champion kit: three behaviors (share wins, answer with a runnable prompt, grow the circle), a time budget (about 40 minutes/week), a thirty-day plan (seed channel, weekly show-and-tell, pairing and FAQ, hand off to a second champion), and scripted responses to common objections. Communications kit: pre-launch checklist (channel, tested install, data-handling link, one concrete first task, 48-hour owner, exec sponsor), announcement templates and variants, a tips drip campaign, FAQ one-liners and prompt templates.
- Status: GA (documentation)
- Evidence: https://code.claude.com/docs/en/champion-kit.md (skimmed); https://code.claude.com/docs/en/communications-kit.md (skimmed)
- Notes: `vendor-claim`: "exec-sent launches consistently see higher first-week adoption" stated with no data.

### Multi-tenant isolation inside one container
- What: To stop one tenant's config leaking into another: `settingSources: []`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` (auto memory loads regardless of settingSources), per-tenant `CLAUDE_CONFIG_DIR` (separate `~/.claude.json`), per-tenant `cwd` on every call, per-tenant egress rules at the proxy.
- Status: GA (guidance)
- Evidence: https://code.claude.com/docs/en/agent-sdk/hosting.md (Multi-tenant isolation)
- Notes: TS `env` replaces the subprocess environment (must spread `process.env`); Python `env` merges. A classic footgun the docs call out repeatedly.

### Claude apps gateway (org-run gateway)
- What: an org-operated gateway with a large share of recent changelog lines:
  - Spend limits, including pooled group budgets (2.1.316) with conflict and priority rules (2.1.319).
  - Layered policies (`managed.policy_mode: layered`, 2.1.316).
  - Upstreams: Bedrock with guardrails and `assume_role`, `mantle`, per-upstream `models` / `groups` / `headers`.
  - OIDC options, a cost-report admin API (2.1.317), JSON logs (2.1.319), `load_test_mode` (2.1.283), and a `code` policy that also applies in the Desktop Code tab (2.1.296).
- Status: GA (as the changelog presents it)
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.283, 2.1.295, 2.1.296, 2.1.314, 2.1.316, 2.1.317, 2.1.319); own pages claude-apps-gateway*.md
- Notes: another slice covers the gateway in depth. Recorded here only as a trend: a large share of Sep–Nov changelog lines are gateway admin.

### Claude Tag (Claude in Slack)
- What: changelog lines prefixed `[Claude Tag]`:
  - admin settings (Guests setting, custom connectors);
  - Slack Connect channels, off by default (2.1.300);
  - retired-model thread migration (2.1.282);
  - artifacts created from Claude Tag sessions (Week 29).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.274, 2.1.282, 2.1.300); https://code.claude.com/docs/en/whats-new/2026-w29.md; own page claude-tag.md

## Area 19. Observability: usage and cost, logs, tracing, debugging

### Debug and diagnostics flags and commands
- What: `--debug[=categories]` with include/exclude filters (only bound in `=` form), `--debug-file <path>` (overrides `CLAUDE_CODE_DEBUG_LOGS_DIR`), `--verbose` (overrides `viewMode`), `claude doctor` (read-only install/settings/Remote Control diagnostics), `/doctor` (Skill that proposes and applies fixes after confirmation), `/debug` (Skill that turns on logging mid-session and reads the log), `/heapdump` (JS heap snapshot plus memory breakdown; the `.heapsnapshot` contains conversation and credentials), `/status` (version, model, account, connectivity, `Session kind`), `/usage` (cost, plan limits, stats), `/insights` (HTML report on recent local sessions).
- Status: GA
- Evidence: https://code.claude.com/docs/en/cli-reference.md (CLI commands; CLI flags); https://code.claude.com/docs/en/commands.md (All commands)
- Notes: The process is a JavaScript runtime per `/heapdump` wording.

### Classifier cost and latency
- What: Each client-side check sends part of the transcript plus the pending action, adding a round trip before execution; reads and in-tree edits skip it so overhead is mainly shell and network actions. On Enterprise, API and cloud-provider accounts classifier calls count toward token usage. Sandboxed network access adds no per-connection classifier calls (hosts are reviewed once with the command).
- Status: GA
- Evidence: https://code.claude.com/docs/en/permission-modes.md (Cost and latency)

### Auto mode classifier billing notice and gateway requirements
- What: Where server-side checks run, classification is part of the session's own model requests at no charge. When they do not reach the session, Claude Code makes its own billed classifier requests and shows a notice (never on Pro/Max/Ultra/Team) that holds the first checked action until Enter (continue, billed; suppressed 24h on that machine if a gateway was named) or Esc/Ctrl+C (cancel turn). Headless prints to stderr; `stream-json` emits a `system` warning. For gateways to pass server checks they must forward the `anthropic-beta` header and the `safeguards` body field together (Bedrock InvokeModel: `anthropic_beta` body field) and return responses unchanged, keeping `safeguard_results` and tool-use IDs. `/status` "Auto mode server" Enabled/Disabled shows the state.
- Status: GA; `CLAUDE_CODE_AUTO_MODE_SERVER` described as a temporary setting that may be removed
- Evidence: https://code.claude.com/docs/en/auto-mode-classifier-billing.md (Respond to the notice; Make the session eligible; Why the notice appears)

### Hook debugging and inspection
- What: `/hooks` is a read-only browser listing each hook with its source and full command. `claude --debug` writes `~/.claude/debug/<session-id>.txt` (or `--debug-file <path>`, `/debug` mid-session); v2.1.296+ logs one `Hooks:` line per command hook with event:match, command, plugin, outcome and duration (`finished with status`, `timed out after`, `cancelled`, `moved to the background`, `failed to start`). `CLAUDE_CODE_DEBUG_LOG_LEVEL=verbose` adds matcher counts. `Ctrl+O` transcript view shows outcomes. `prompt_id` in hook input matches OTel `prompt.id`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (The /hooks menu; Debug hooks; Common input fields); https://code.claude.com/docs/en/hooks-guide.md (Check what a hook did; Hook JSON has no effect)
- Notes: `/doctor` flags slow hooks.

### Prompt cache statistics
- What: `/usage` shows a `Prompt cache (main)` line (hit ratio, misses, warm/cold, likely cause of last miss) from v2.1.251/2.1.260; status-line `prompt_cache` object exposes the same; OTel exporter reports cache read/creation tokens per user/session. SessionStart (on resume) and PreModelSwitch hook inputs include estimated re-cache cost in USD.
- Status: GA
- Evidence: https://code.claude.com/docs/en/prompt-caching.md (Check cache performance); https://code.claude.com/docs/en/statusline.md (Prompt cache fields); https://code.claude.com/docs/en/hooks.md (SessionStart input; PreModelSwitch input)
- Notes: Statistics exclude subagent requests.

### Plugin telemetry and analytics
- What: OpenTelemetry events `claude_code.plugin_installed` and `claude_code.plugin_loaded` (with `plugin.scope` such as `org` or `user-local`); names of non-official plugins redacted to `third-party` or omitted unless `OTEL_LOG_TOOL_DETAILS=1`. Enterprise Analytics API `GET /v1/organizations/analytics/plugins` gives per-plugin per-day install and invocation counts across Claude Code and Cowork, groupable by user or RBAC group.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/org.md (Audit and review); https://code.claude.com/docs/en/plugins/security.md (Find plugins in telemetry)

### Plugin cost measurement and usage signals
- What: `claude plugin details <name>` prints a component inventory and projected token cost: "Always-on" tokens (names + `description` + `when_to_use` of skills, agents, commands) added to every session, and per-component always-on vs on-invoke costs; hooks are marked harness-only (no context cost), MCP tool schemas not counted (use `/context`). Official listings highlight Every-turn cost at 2,000+ tokens. Usage signals for users: Installed-tab "Not used recently" and `Last used:`, `/skill-doctor` (never-invoked skills, Stats tab), `/doctor` (recommends disabling unused skills/MCP/plugins), `/usage` share by skill/subagent/plugin/MCP on paid plans. Fleet: OTel `plugin_installed`, `plugin_loaded`, `skill_activated`, `hook_plugin_metrics` (official plugins only), and plugin attribution on the cost counter.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/measure.md (Measure what a plugin costs; Lower the always-on figure; Cost shown to users before install; Check whether a plugin is used; Measure across a fleet; Redacted plugin names in your backend)
- Notes: Claude Code does not report usage back to plugin authors.

### Plugin evals (`claude plugin eval`)
- What: Behavioural test harness for plugins (v2.1.269+). Cases live in `evals/<case>/` with `prompt.md` (frontmatter `max_turns` default 10 max 200, `timeout_seconds` default 300 max 3600, `allowed_tools`, `model`, `tags`, `runs` default 3 max 50, `env` keys must be `EVAL_*`) and `graders/*.md`, optional `case.yaml` (`context.scaffold_script` run only with `--scaffold`, 120 s; `context.history_file` to resume a `.jsonl` transcript; `context.add_dirs` read-only). Grader types: `regex`, `tool_used` (min/max, `input_match`), `tool_order`, `file_exists`, `llm` (judge, 2-of-3 votes), `baseline` (compare to a reference transcript); no custom-code graders; targets `last_message`, `trace`, `files`, a file's contents (images shown to the judge), `mock_calls`. Each case runs with the plugin and without (no-plugin baseline) to report WITH, W/OUT and delta; graders that cannot pass without the plugin (e.g. `tool_used: Skill`) are excluded from scoring in both arms. MCP mocks under `evals/mocks/<server>/<tool>.md` (fixed or `type: agent` judge-answered, `expect:` input guards with a restricted regex dialect, `_tools.json` for real schemas, recordings replayable from `.replay/`); real servers only with `--allow-real-servers` or `--mocks off`. `claude plugin eval init` runs an interactive interview where Claude proposes and trial-runs cases.
- Status: GA (manifest key `experimental.evals`; older builds said "early access"; Anthropic can switch it off server-side)
- Evidence: https://code.claude.com/docs/en/plugin-evals.md (How an eval run works; Create your first eval suite; Choose and weight graders; Score against the no-plugin baseline; Mock https://code.claude.com/docs/en/mcp.md servers; Grader types; prompt.md frontmatter; case.yaml fields); https://code.claude.com/docs/en/plugins/cli-reference.md (plugin eval; plugin eval init)
- Notes: Isolation: each run is a `claude -p` child with temp home, cwd and config; no user settings, hooks, CLAUDE.md, MCP servers, other plugins or project `.claude/`/`.mcp.json` load; only an env allowlist passes; eval dir hidden from the agent; Artifact tool off; tools beyond a read-only set (Read, Glob, Grep, Skill, Agent, TodoWrite, task tools...) need `--allow-tools`; any Bash grant runs under the OS sandbox (writes confined, home unreadable, network only to granted domains) and runs are refused if no sandbox backend exists (native Windows). Docs say isolation is not a boundary against the plugin's own hooks/servers.

### Mod diagnostics and debug log
- What: When a module fails to load, a hook is skipped (`<mod>: <event> hook skipped: <reason>`), or a mod is refused, one line naming the mod is written: dim transcript line in hot-reload sessions, debug log (`claude --debug` / `--debug-file`) otherwise, stderr in `-p --plugin-dir`. Load line lists worker, environment, tier and events. Docs enumerate refusal strings (rollout switch served off remotely, disableAllHooks, allowManagedHooksOnly, --bare, name collision), guard messages, tree-validation messages, toast drop reasons, settings.read resolution lines. `/plugin` shows "N mod active · names" and an Errors tab with notes on guard-skipped hooks (v2.1.299+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/troubleshoot.md (Find out why a mod does nothing; Check whether mods can load; Refusal messages; Read the debug log)
- Notes: Docs reveal a remote rollout switch: Anthropic can turn installed mods off ("the rollout switch served off"), and the last value is cached by earlier sessions. Mods can write their own debug lines with `$.ui.log(msg, {to:'debug'})`. Reload failures keep the previous version running.

### Background session observability
- What: `claude logs <id|name>` prints recent output; `claude daemon status` reports reachability, pid, version, socket dir, worker count and warns on version skew; `claude daemon logs` tails `~/.claude/daemon.log`; `/status` shows `Session kind` (background job attached/unattended vs interactive, v2.1.221). Rows surface hook validation errors on pending permission requests (v2.1.248).
- Status: research preview
- Evidence: https://code.claude.com/docs/en/agent-view.md (Manage sessions from the shell; Where state is stored; How background sessions are hosted; Peek and reply)
- Notes: Supervisor stall recovery: `claude daemon stop --any --keep-workers` then `claude agents` within ~1 minute.

### Runner and orchestrator health and Prometheus metrics
- What: `/healthz` and `/metrics` on `--health-port` (8080). Runner health returns 200 whenever alive (liveness only) with `last_poll_age_ms`; orchestrator health always 200 with `connected`, `queue_counts`, `last_error`. Runner series: info, capacity, active sessions, locked_account{email}, last_poll_age_seconds, poll_errors_total{error_kind}, sessions started/completed/failed/interrupted by client_platform, init duration histogram, init errors, SessionStart hook errors, per-session idle seconds. Orchestrator: connected, poll age, queue pending/backing-off/circuit-broken, pool pending/active (use MAX across replicas), spawn hook results and duration, queue-wait histogram, clock skew. Session-child OTel metrics can be re-exposed on the runner endpoint with session labels (capacity >1). Sample alert rules and KEDA `ignoreNullValues: "false"` advice provided.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-reference.md (Health endpoint; Prometheus metrics; Sample alert rules; Pass through session-child metrics; Session lifecycle counter semantics)
- Notes: Docs warn terminal counters are nearly invisible on one-shot runners because the process exits before a scrape; they document which series to use instead.

### Runner diagnostics: `doctor`, `setup`, logs
- What: `claude self-hosted-runner setup` is a guided interactive session that walks environment creation, starts a runner, confirms registration and writes `./runner-setup/CHEAT-SHEET.md`. `claude self-hosted-runner doctor` starts an interactive Claude session with the runner's logs and state attached (richer when signed in). Plain-text logs: lifecycle to stdout, debug to stderr; `--log-file` mirrors (0600). Failed-session debug logs kept on disk unless `--remove-session-state`. Detailed log-line catalogue for failures.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-quickstart.md (Run the guided setup); https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Troubleshooting; When the runner exits)
- Notes: Using the agent itself as the setup wizard and diagnostician is notable.

### Code Review analytics
- What: claude.ai/analytics/code-review shows PRs reviewed per day, month-to-date cost, "Feedback" (comments auto-resolved because the developer fixed the issue), findings by severity per day, and a per-repo breakdown with estimated cost. The admin Repositories table shows the average cost per review. Analytics costs are list-price estimates and can differ from the invoice.
- Status: GA
- Evidence: https://code.claude.com/docs/en/code-review-costs.md (View usage; Estimate your spend)
- Notes: Auto-resolved comments are used as the quality signal.

### /usage (aliases /cost, /stats) session cost and plan usage
- What: `/usage` shows a Session block: total cost (computed locally from token counts at list price, or at `modelPricing` rates with a note), API and wall duration, lines changed, per-model input/output/cache read/cache write tokens. Totals reset on `/clear` (since v2.1.211). Data-residency 1.1x multiplier applied, also to status line and `--max-budget-usd`. `Prompt cache (main)` line: request count, % input from cache, misses with likely cause, expected rebuilds (compaction, tool-result clearing), warm/cold with TTL (v2.1.251+; main conversation only). Subscribers see plan bars, attribution by skill/subagent/plugin/MCP server, recommendations (rows covering >=10% of usage), heaviest loops; `d`/`w` toggles 24h/7d; computed from local history only. `f` applies fixable recommendations (runs `/effort` rows, prompts Claude for others). Falls back to last-known bars within 60 min when the usage endpoint is rate-limited. Usage-credits spend row.
- Status: GA
- Evidence: https://code.claude.com/docs/en/costs.md (Using the /usage command; Prompt cache statistics; Plan usage breakdown; Apply recommendations with the fix shortcut; Check your usage-credits spend; When the usage request fails); https://code.claude.com/docs/en/commands.md (/cost, /stats, /usage)
- Notes: `/cost` and `/stats` are aliases of `/usage` (`/stats` opens on a Stats tab). Cost is an estimate, not billing.

### Spinner token counter, /insights, /context, /status
- What: The spinner line shows elapsed time and an approximate live count of output tokens for the current turn (including thinking and foreground subagents), resetting each turn and on compaction. `/insights` analyses up to 200 unseen recent local sessions and writes an HTML report to `~/.claude/usage-data/report.html` (timestamped copies; deleted per `cleanupPeriodDays`, default 30), runs through the user's provider and costs tokens. `/context` shows a context grid. `/status` shows version, model, account, provider, base URL, credential source and region.
- Status: GA
- Evidence: https://code.claude.com/docs/en/costs.md (Read the token count beside the spinner; Analyze your usage patterns); https://code.claude.com/docs/en/commands.md (/context, /insights, /status)
- Notes: `/insights` is not available in cloud sessions.

### modelPricing (report at contracted rates)
- What: Managed-only setting with `multiplier` (0-10; below 1 discount, above 1 markup since v2.1.271) and per-model `overrides` of input/output/cacheRead/cacheWrite per MTok. Applies to `/usage`, status line, SDK `total_cost_usd`, `--max-budget-usd`, OTel cost metric and events. Rows keyed by built-in IDs apply to all snapshot and provider IDs of that model; other keys (gateway aliases) apply exactly. Picker prices stay list price.
- Status: GA
- Evidence: https://code.claude.com/docs/en/costs.md (Report spend at your contracted rates); https://code.claude.com/docs/en/settings-reference.md (modelPricing)
- Notes: v2.1.242+. Rates entered by hand, not fetched. Ignored in user/project settings and HKCU.

### Organization cost controls and reporting by setup
- What: Teams/Enterprise: per-seat allowance on rolling 5-hour and weekly windows shared with chat and Cowork; spend report (daily, CSV) once usage credits are on; spend limits at org/group/member; Enterprise Analytics API (`read:analytics` key). Console: auto-created "Claude Code" workspace (no API keys can be created in it), workspace spend and rate limits, Console dashboard, Claude Code Analytics API with Admin key. Cloud providers: cloud billing only; no metrics flow to Anthropic analytics; use OTel, Claude apps gateway (per-user spend limits) or an LLM gateway such as LiteLLM. Per-user TPM/RPM sizing table by team size. Guidance on what each limit message means. `/usage-credits` opens or requests usage credits.
- Status: GA
- Evidence: https://code.claude.com/docs/en/costs.md (Manage costs for your organization; Claude for Teams and Enterprise; Claude Console; Rate limit recommendations; Cloud providers; When a developer asks about a limit; Add usage credits to your subscription)
- Notes: The average-cost figures ($13 per active developer-day, $150-250/month, 90% under $30/day) are vendor claims without method. LiteLLM is labelled unaudited and unaffiliated.

### Background token usage and why long sessions get expensive
- What: Lists idle-time token spenders: session recap, idle compaction, prompt suggestions (cache-read heavy, skipped near limit), session-scoped scheduled tasks, cross-session messages (`crossSessionInbound: hold`), goal check-ins (max three idle per goal; `CLAUDE_CODE_GOAL_CHECKIN_MINUTES=0`), subagents and workflows, teammates, and compaction itself. Agent teams use about 7x tokens in plan mode. Cost-reduction advice: `/clear`, compaction instructions, smaller models for subagents, CLI tools over MCP, hooks to pre-filter output, move CLAUDE.md content into skills.
- Status: GA
- Evidence: https://code.claude.com/docs/en/costs.md (Background token usage; Why usage climbs in a long session; Reduce token usage; Agent team token costs)
- Notes: Agent teams require `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` (experimental per this page).

### OpenTelemetry export: configuration
- What: Opt-in with `CLAUDE_CODE_ENABLE_TELEMETRY=1`; exporters `OTEL_METRICS_EXPORTER` (otlp, prometheus on :9464, console, none) and `OTEL_LOGS_EXPORTER` (otlp, console, none); no default OTLP protocol (must set grpc, http/json or http/protobuf). Standard per-signal endpoint/protocol/header variables (headers merge). Metric interval default 60 s, logs 5 s, delta temporality by default. mTLS: http uses `CLAUDE_CODE_CLIENT_CERT/KEY`, grpc uses OTEL client key/cert. Dynamic headers via `otelHeadersHelper` script (JSON output; refreshed every 29 min; http protocols only; failure surfaced in UI, `/status`, debug log, stderr). `OTEL_RESOURCE_ATTRIBUTES` custom keys copied onto every datapoint and event (strict format; a space silently drops all keys). Cardinality toggles for session.id, version, account UUID, entrypoint, resource attributes, repository. Exporter failures logged as `[3P telemetry]` in `--debug-file`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/monitoring-usage.md (Quick start; Common configuration variables; mTLS authentication; Metrics cardinality control; Dynamic headers; Multi-team organization support; Example configurations)
- Notes: `OTEL_*` vars are not passed to subprocesses (Bash tool, hooks, MCP, LSP). Repository `.claude/settings*.json` cannot enable telemetry, choose destination or capture content; it can only turn a signal off.

### Managed settings lock the OTLP destination
- What: When managed settings set an OTLP endpoint, protocol or credential, Claude Code removes conflicting developer-set per-signal variables at startup (setting a managed credential also removes every developer endpoint so credentials cannot reach another collector). Exporter selectors follow normal precedence (developer can still disable a signal). A desktop app or self-hosted runner that names an endpoint pins it the same way (v2.1.251+). Also removes a developer `BETA_TRACING_ENDPOINT` when managed settings decide logs/traces destination.
- Status: GA
- Evidence: https://code.claude.com/docs/en/monitoring-usage.md (Administrator configuration; How managed settings lock the OTLP destination)
- Notes: Before v2.1.217 a user per-signal endpoint could redirect away from the managed collector (an exfiltration path that was closed).

### OpenTelemetry metrics
- What: Meter `com.anthropic.claude_code`. Metrics: `claude_code.session.count` (attr `start_type` fresh/resume/continue/agents_view), `lines_of_code.count` (type, model), `pull_request.count`, `commit.count`, `cost.usage` (USD), `token.usage` (type input/output/cacheRead/cacheCreation; input excludes cache), `code_edit_tool.decision` (tool_name, decision, source, language), `active_time.total` (s; type user/cli). Cost and token counters carry model, `query_source` (main/subagent/auxiliary), `speed`, `effort`, `agent.name`, `skill.name`, `plugin.name`, `marketplace.name`, `mcp_server.name`, `mcp_tool.name` (redacted to custom/third-party by default). Prometheus-only export drops unit strings. Streaming usage counted exactly once even when a gateway streams usage progressively.
- Status: GA
- Evidence: https://code.claude.com/docs/en/monitoring-usage.md (Metrics; Metric details; Cost monitoring; Map input tokens to OpenTelemetry GenAI semantic conventions)
- Notes: Does not set `gen_ai.usage.*`; page explains how to compute them. Cost metrics are approximations.

### OpenTelemetry events (logs)
- What: Events: `user_prompt`, `assistant_response`, `tool_result`, `api_request`, `api_error`, `api_refusal` (with server fallback hop, category flags), `api_request_body`/`api_response_body` (opt-in), `tool_decision` (decision, source, classifier verdict/category, permission_mode, tool_source), `safety_event` (category, reason codes, rule source, sandbox violation counts; v2.1.307+), `permission_mode_changed`, `auth`, `mcp_server_connection`, `internal_error` (class and errno only), `plugin_installed`, `plugin_loaded`, `skill_activated`, `at_mention`, `api_retries_exhausted`, `hook_registered`, `hook_execution_start`/`_complete`, `hook_plugin_metrics`, `compaction` (pre/post tokens, precompute reuse), `subagent_completed`, `feedback_survey`, `retention_sweep`, `managed_settings_resolved`, plus `system_prompt` under detailed beta tracing. Correlation: `prompt.id`, per-process `event.sequence`, `message.uuid` (joins transcript), `request_id` (or `x-amzn-requestid` on Bedrock), `client_request_id`, `tool_use_id`, `agent_id`/`parent_agent_id`, `workflow.run_id`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/monitoring-usage.md (Events; Event correlation attributes; each event section)
- Notes: Transcript-join fields are explicitly version-specific, not a stable contract. `api_error` fires only once after retries are exhausted; `CLAUDE_CODE_MAX_RETRIES` default 10, cap 15.

### OpenTelemetry traces
- What: Requires `CLAUDE_CODE_ENABLE_TELEMETRY=1` plus `CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1` and `OTEL_TRACES_EXPORTER`. Spans: `claude_code.interaction` (root per prompt), `llm_request` (model, gen_ai.* aliases, query_source_safe, agent ids, speed, effort, ttft_ms, token counts, request ids, attempts, stop_reason), `tool` (tool_name_safe, bash_command_class/argv0 from a fixed list, result_tokens, gated file_path/full_command), children `tool.blocked_on_user` (permission wait, decision) and `tool.execution`; subagent spans nest under the parent's tool span; `hook` span only under detailed beta tracing. W3C `TRACEPARENT` injected into Bash/PowerShell subprocesses; `traceparent` sent on model and HTTP MCP requests only to the Anthropic API by default (`CLAUDE_CODE_PROPAGATE_TRACEPARENT=1` for proxies). SDK and `-p` sessions accept inbound `TRACEPARENT`/`TRACESTATE` to parent under the caller's trace; interactive sessions ignore ambient values. Deferred tool calls rejoin the original turn's trace on resume.
- Status: beta
- Evidence: https://code.claude.com/docs/en/monitoring-usage.md (Traces (beta); Span hierarchy; Span attributes)
- Notes: Detailed beta tracing (`ENABLE_BETA_TRACING_DETAILED` + `BETA_TRACING_ENDPOINT`) is debug-only, high volume, and needs org allowlisting in interactive CLI.

### Repository attributes on telemetry
- What: `OTEL_METRICS_INCLUDE_REPOSITORY=true` adds `vcs.repository.url.full`, `vcs.owner.name`, `vcs.repository.name`, `vcs.provider.name` derived once per session from the `origin` remote (lowercased; credentials, query and fragment removed; omitted for home-dir repos). Can be overridden via `OTEL_RESOURCE_ATTRIBUTES`. `tool_result` also carries the commit SHA and branch of successful `git commit` with tool details on.
- Status: GA
- Evidence: https://code.claude.com/docs/en/monitoring-usage.md (Repository attributes; Tool result event)
- Notes: v2.1.269+. Anthropic's own telemetry drops every `vcs.*` key.

### Security auditing with OTel events (SIEM)
- What: Docs position OTel events as the audit source: identity attributes on every event, a table mapping security questions to events (tool allow/deny, which check refused, classifier verdicts, mode escalation, hook blocks, sandbox network refusals, auth, MCP connects, plugin installs, managed-settings health), and a table mapping egress paths (Bash, MCP, hooks, plugins, WebFetch, upload tools, Remote Control, retention) to managed controls and events. Claude Code emits raw events only; detection and alerting are the backend's job. Example SIEM config with `OTEL_LOG_TOOL_DETAILS=1`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/monitoring-usage.md (Audit security events; Attribute actions to users; Audit MCP activity; Map security questions to events; Map egress paths to managed controls and events; Send events to a SIEM; Check the retention sweep)
- Notes: With API key or cloud-provider auth only `user.id` (random install ID) and `session.id` identify the user; admins must inject identity via `OTEL_RESOURCE_ATTRIBUTES`. Remote Control has no dedicated event.

### Analytics dashboards
- What: Teams/Enterprise dashboard (claude.ai/analytics/claude-code, Admins and Owners): lines accepted, suggestion accept rate, DAU and sessions, contribution metrics (PRs with CC, lines with CC, % PRs), PRs per user, leaderboard top 10, CSV export of all users. Contribution metrics require the Claude GitHub app and Owner toggles; PR attribution matches session edits to merged PR diffs within 21 days before to 2 days after merge, normalises lines, ignores >20% rewrites, excludes lock/generated/build files and lines over 1000 chars; labels PRs `claude-code-assisted` in GitHub. Console dashboard (platform.claude.com/claude-code, UsageView permission): lines accepted, accept rate, active users, sessions, daily spend, per-member team insights.
- Status: GA; contribution metrics public beta
- Evidence: https://code.claude.com/docs/en/analytics.md (all sections)
- Notes: Contribution metrics unavailable under ZDR, for GitHub Enterprise Server, and for API customers. Page calls the metrics deliberately conservative. Code Review has its own dashboard (not read).

### Inspecting effective configuration
- What: `/status` shows a `Setting sources` line listing loaded files and, for managed policy, the delivery path in parentheses (`remote`, `plist`, `HKLM`, `file`, `drop-ins`, `... merged`, `HKCU`, `parent process`, `helper`) plus `Skipped sources` (v2.1.242+). It does not show which file supplied each key. `claude doctor` (shell, read-only, no session) lists rejected entries with source and field, remote fetch outcome (`Managed settings (remote)` line, v2.1.248+), `Organization policy` line, update status, search backend. `/doctor` inside a session proposes fixes. `/context` shows context usage by category; `/memory`, `/skills`, `/hooks`, `/mcp`, `/permissions` show each surface. `/debug` and `--debug`/`--debug-file`/`DEBUG=1` write logs to `~/.claude/debug/<session-id>.txt`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/debug-your-config.md (See what loaded into context; Check resolved settings); https://code.claude.com/docs/en/managed-settings.md (Read the source in /status); https://code.claude.com/docs/en/server-managed-settings.md (Verify settings delivery); https://code.claude.com/docs/en/settings.md (Confirm what loaded)
- Notes: `OTEL_LOG_MANAGED_SETTINGS=1` adds redacted resolved managed settings plus a SHA-256 digest to an OTel event (v2.1.274+).

### Safe mode and clean-configuration test
- What: `claude --safe-mode` (or `CLAUDE_CODE_SAFE_MODE=1`) disables CLAUDE.md, skills, plugins, hooks, MCP, custom commands/agents, output styles, workflows, themes, keybindings, status line, LSP, auto memory; managed policy and managed hooks still apply, but managed plugins/skills/CLAUDE.md/MCP do not. For a deeper test: empty directory + `CLAUDE_CONFIG_DIR=<fresh dir>` + `--setting-sources user`, then bisect by copying files back. `--bare` / `CLAUDE_CODE_SIMPLE` is a separate minimal mode (Bash, read, edit tools only; no OAuth/keychain).
- Status: GA
- Evidence: https://code.claude.com/docs/en/debug-your-config.md (Test against a clean configuration); https://code.claude.com/docs/en/env-vars.md (Variables: CLAUDE_CODE_SAFE_MODE, CLAUDE_CODE_SIMPLE)
- Notes: Common-cause table covers hook matcher arrays, case-sensitive tool names, `.mcp.json` placement, `mcpServers` not read from settings.json, Bash rules matching literal strings.

### Performance troubleshooting
- What: A warning appears past 2.5 GB heap; remedy is restart plus `claude --continue`. `/heapdump` writes a heap snapshot (contains conversation and credentials, never share) and a shareable `-diagnostics.json`, and prints JS-heap vs native split and leak indicators. Auto-compaction stops with a "thrashing" error when output refills the context repeatedly. Markdown tables render at most 200 rows. Bundled ripgrep can be replaced via `USE_BUILTIN_RIPGREP=0`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/troubleshooting.md (High CPU or memory usage; Auto-compaction stops with a thrashing error; Search and discovery issues)
- Notes: `/heapdump` is hidden from the command menu.

### Error reference taxonomy
- What: A single error reference page with sections: automatic retries, server errors, usage limits, authentication, network/connection, request errors, command-line errors, plugin errors, tool errors, background session errors, wrapper and IDE errors, rewind, session saving, configuration warnings (about 35 entries including managed-settings parse/read/approval/provider/model-block errors, untrusted workspace, network-path cwd, two config files), "responses seem lower quality", and how to report. Each entry quotes the literal message text.
- Status: GA
- Evidence: https://code.claude.com/docs/en/errors.md (section headings); https://code.claude.com/docs/en/troubleshoot-install.md (Find your error)
- Notes: The install troubleshooting page has a symptom-to-fix lookup table keyed on literal error strings. Skimmed, not read in full.

### Usage visibility for orgs
- What: OpenTelemetry export (all providers); analytics dashboard (Teams/Enterprise at claude.ai/analytics with adoption metrics and leaderboard; Console at platform.claude.com with per-user usage and spend); programmatic reporting via Enterprise Analytics API or Claude Code Analytics API (Console); spend controls via org settings, Console workspace limits, cloud budget tools, or gateway spend limits. Per-user spend on Teams/Enterprise comes from the spend report, not the dashboard.
- Status: GA
- Evidence: https://code.claude.com/docs/en/admin-setup.md (Set up usage visibility)

### Gateway telemetry relay (OTLP/HTTP)
- What: With `telemetry.forward_to` and `listen.public_url` set, the gateway pushes six env vars to clients via `/managed/settings` (`CLAUDE_CODE_ENABLE_TELEMETRY=1`, per-signal `OTEL_*_EXPORTER` otlp/none, endpoint = gateway public URL, protocol http/protobuf) and relays exports verbatim to each destination. Per-destination opt-in: metrics default; logs and traces opt-in (can carry full Bash commands, tool inputs, file paths). CLI stamps `user.id`, `user.email`, `user.groups` from the gateway JWT; Desktop/Cowork use `enduser.id`/`enduser.sub`. `resource_attributes` adds fixed labels (reserved names refused). Locally set OTEL endpoints are overridden in gateway sessions.
- Status: GA (traces need `CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1`, i.e. beta)
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-config.md (telemetry; Add your own labels; Group changes during an open session)
- Notes: No buffering or retry: failed exports are dropped and the client still gets success; after five consecutive failures forwarding pauses in 30-second stretches. OTLP/gRPC unsupported. Option to export directly to a collector named in a policy (v2.1.265+ clients), with checks (must come from gateway, https, not gateway host, no `otelHeadersHelper`); session token never sent to such collectors. Pushing the endpoint triggers the client security approval dialog.

### Gateway audit log and operational logs
- What: Two stderr streams. Audit events are single-line JSON with `ts`/`evt`, always emitted: `config.load` (with config sha256), `session.mint`, `session.refresh`, `device.*`, `auth.denied`, `access.denied`, `access.public_client`, `inference` (upstream and status), `managed.serve`, `desktop_bootstrap.*`, `spend.blocked`, `admin.denied` (no key material), `admin.limit.upsert/delete`, `guardrail.trace`. Operational logs are `[gateway]`-prefixed, level via `CLAUDE_GATEWAY_LOG_LEVEL`, JSON via `CLAUDE_GATEWAY_LOG_FORMAT=json` (v2.1.319+). Debug logs claim names, not values.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-deploy.md (Logs); https://code.claude.com/docs/en/claude-apps-gateway.md (Steps, step 4)
- Notes: Gateway does not log or store prompt or completion content (overview, What the organization can see). Audit records identities; no request-body audit.

### Gateway health, concurrency and load-test mode
- What: `/healthz` liveness, `/readyz` readiness (checks Postgres; `store.readiness_grace_seconds` keeps ready through a failover), discovery document doubles as an end-to-end boot check. Each replica sends at most 256 concurrent upstream requests (`BUN_CONFIG_MAX_HTTP_REQUESTS`), excess waits in-process and logs `client requests are open`. `load_test_mode` (v2.1.282+) signs and discards provider requests and streams canned replies; refuses to run against a database with recorded spend; `x-load-test-user` header simulates distinct users.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-deploy.md (Health; Concurrent upstream requests); https://code.claude.com/docs/en/claude-apps-gateway-config.md (load_test_mode)
- Notes: The env var name implies the server runs on Bun (inference from the name only).

### Cost and usage accounting
- What: `total_cost_usd` and per-model `modelUsage` (with `costBasis` list/managed/unknown) on every result are client-side estimates from a bundled price table (or `modelPricing`), with US data-residency x1.1 modeled. `usage` covers only the main loop; `modelUsage` covers subagents and internal calls (compaction, Workflow) but not classifier or token-count helpers. Per-step assistant `usage` must be deduped by message ID and its `output_tokens` is a placeholder from `message_start`. Streaming-mode totals are cumulative until `/clear`/`/reset`/`/new` (TS emits `SDKConversationResetMessage`). Resumed sessions restore earlier totals (v2.1.277+). Crash results may be zeroed; recovery procedure documented. Cache TTL: 5 min default for API key/cloud; `ENABLE_PROMPT_CACHING_1H`, `CLAUDE_CODE_PROMPT_CACHE_TTL`, `CLAUDE_CODE_SUBAGENT_PROMPT_CACHE_TTL`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/cost-tracking.md (Understand token usage; Track costs in streaming input mode; Get the total cost; Track per-step and per-model usage; Handle errors, caching, and output token counts)
- Notes: Docs say explicitly not to bill end users from these fields.

### OpenTelemetry export from the CLI child
- What: The SDK emits no telemetry itself; the CLI child exports metrics (tokens, cost, sessions, LOC, tool decisions), log events (prompts, API requests/errors, tool results, `safety_event`, `permission_mode_changed`, MCP connections) and beta traces (`claude_code.interaction` > `llm_request` / `tool` (> `blocked_on_user`, `execution`) / `hook`; subagent spans nest under the parent's tool span). Enabled by `CLAUDE_CODE_ENABLE_TELEMETRY=1` plus exporters; traces need `CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1`. The SDK injects W3C `TRACEPARENT`/`TRACESTATE` into the child (and the CLI forwards it to Bash commands) so agent spans join the host trace. Content is off by default; opt-ins `OTEL_LOG_USER_PROMPTS`, `OTEL_LOG_TOOL_DETAILS`, `OTEL_LOG_TOOL_CONTENT` (60 KB truncation), `OTEL_LOG_RAW_API_BODIES` (inline or `file:<dir>`).
- Status: GA (traces beta)
- Evidence: https://code.claude.com/docs/en/agent-sdk/observability.md (How telemetry flows; Enable telemetry export; Read agent traces; Link traces to your application; Attribute actions to your end users; Control sensitive data in exports); https://code.claude.com/docs/en/agent-sdk/hosting.md (Observability)
- Notes: Export errors are silent unless `CLAUDE_CODE_OTEL_DIAG_STDERR=1`. The `console` exporter must not be used because stdout is the SDK message channel. Defaults: metrics every 60 s, traces/logs every 5 s; short-lived processes can lose batches. Per-end-user attribution is done by percent-encoded `OTEL_RESOURCE_ATTRIBUTES` per call.

### Error surfaces and troubleshooting
- What: TS has one typed error (`AbortError`); process exits reject iteration with plain `Error` messages (`Claude Code process exited with code N`, `terminated by signal`, `Claude Code returned an error result: ...`). Python has `ClaudeSDKError`, `CLINotFoundError`, `CLIConnectionError`, `ProcessError` (`exit_code`; its `stderr` is fixed text, use the `stderr` callback), `ResultError` (subclass, carries the error result in `data`), `CLIJSONDecodeError`. Python on Windows refuses `.bat`/`.cmd` CLI paths because `cmd.exe` re-parses arguments (injection risk), since 0.2.124.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/troubleshooting.md (CLI startup; CLI process exit; Structured outputs); https://code.claude.com/docs/en/agent-sdk/typescript.md (AbortError); https://code.claude.com/docs/en/agent-sdk/python.md (Error Types)
- Notes: `debug`/`debugFile`/`stderr` options for diagnostics; `getContextUsage()` returns the `/context` breakdown (makes uncounted token-counting requests).

### OpenTelemetry additions
- What:
  - `claude_code.safety_event`: one per tool call stopped, prompted or sent back by a rule, auto mode, a check, a hook or the sandbox (2.1.307).
  - `tool_decision` gains `classifier_verdict`, `permission_mode` and `classifier_category` (2.1.301).
  - GenAI semantic-convention span attributes (2.1.300). `anthropic.plan`, `seat_type`, `provider` and `custom_base_url` (2.1.317).
  - `managed_settings_resolved` (2.1.274). `OTEL_METRICS_INCLUDE_REPOSITORY` (2.1.269). `prompt_text` copy (2.1.287).
  - Content truncation limit of 60 KB (`CLAUDE_CODE_OTEL_CONTENT_MAX_LENGTH`, 2.1.214).
  - Opt-in gateway hint headers: `x-claude-code-prompt-id`, request-class headers (2.1.273, 2.1.283).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.214, 2.1.269, 2.1.273, 2.1.274, 2.1.283, 2.1.287, 2.1.300, 2.1.301, 2.1.307, 2.1.317)

### Usage diagnostics
- What:
  - `/usage` breakdown by skill, subagent, plugin and MCP server (Weeks 16 and 21); Loops (2.1.243).
  - Warnings for max effort, workflows, always-on context and large tool results (2.1.299), each with a one-key fix `f` (2.1.305).
  - `/insights` estimates how many prompts auto mode could have handled (2.1.281).
  - Spend-limit bar for gateway users (2.1.251, 2.1.284).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.243, 2.1.251, 2.1.281, 2.1.284, 2.1.299, 2.1.305); https://code.claude.com/docs/en/whats-new/2026-w16.md, https://code.claude.com/docs/en/whats-new/2026-w21.md

## Area 20. Configuration and customisation

### Config import from other agents
- What: `/import [codex|gemini|cursor] [--dry-run] [--yes]` and `claude import [source]` (v2.1.213+) bring instruction files, MCP servers, commands, subagents and skills from OpenAI Codex, Gemini CLI or Cursor (v2.1.265+); `/init` offers it when it detects Codex or Gemini config.
- Status: GA
- Evidence: https://code.claude.com/docs/en/commands.md (/import, /init); https://code.claude.com/docs/en/cli-reference.md (claude import)
- Notes: Unavailable on third-party providers, gateways, or with feature-flag fetching off.

### Custom themes as watched JSON files
- What: `/theme` offers built-in presets (auto light/dark, daltonized, ANSI) plus custom themes in `~/.claude/themes/<slug>.json` with `name`, `base`, `overrides` of named color tokens (text, status, mode accents, diff backgrounds, fullscreen backgrounds, usage meter, eight subagent colors, shimmer pairs). Invalid tokens or values are ignored; the directory is watched and reloaded live. Plugins can contribute themes.
- Status: GA
- Evidence: https://code.claude.com/docs/en/terminal-config.md (Match the color theme; Create a custom theme)
- Notes: Needs one restart if the themes directory did not exist at launch.

### Keybindings file (contexts, actions, chords)
- What: `~/.claude/keybindings.json` (opened by `/keybindings`, hot-reloaded) holds a `bindings` array of `{context, bindings: {keystroke: action|null}}` blocks. About 30 contexts (Global, Chat, Autocomplete, Confirmation, Transcript, HistorySearch, Task, MessageSelector, DiffDialog, DiffPanel, Select, Plugin, Pane, Agents, Scroll, etc.) and `namespace:action` names (e.g. `chat:submit`, `chat:queueSubmit` v2.1.247+, `chat:sendNow`, `chat:killAgents`, `voice:pushToTalk`, `command:<name>` to bind any slash command). Chords such as the `ctrl+x` family need each key within 3 s; unbinding all chords on a prefix frees it. Reserved and unbindable: Ctrl+C, Ctrl+D, Ctrl+M, Ctrl+[, Ctrl+I, Ctrl+H, Caps Lock. Bare printable keys bound globally still type into focused text fields. Validation writes warnings to the debug log (parse errors, misspelled modifiers, `ctrl+ k` spacing, unknown contexts/actions, reserved conflicts, duplicates); unknown actions are skipped and defaults kept.
- Status: GA
- Evidence: https://code.claude.com/docs/en/keybindings.md (Configuration file; Contexts; Available actions; Chords; Reserved shortcuts; Text fields; Validation)
- Notes: Vim motions and readline editing keys are not remappable here. On Windows without VT input mode (older Node/Bun), Shift+Tab defaults to Meta+M, which indicates the CLI runs on Node or Bun.

### Layering rules for same-named features
- What: CLAUDE.md files are additive across levels (cwd and ancestors at launch, subdirectories as accessed; conflicts reconciled by model judgment). Skills override by name with priority managed > user > project; subagents managed > CLI flag > project > user > plugin; plugin skills are namespaced. MCP servers override by name local > project > user. Hooks merge and all matching hooks fire regardless of source.
- Status: GA
- Evidence: https://code.claude.com/docs/en/features-overview.md (Understand how features layer)
- Notes: Settings precedence itself (managed > `--settings` > local > project > user; arrays merge) is in https://code.claude.com/docs/en/glossary.md (Settings layers).

### `.claude` directory layout and frontmatter by file type
- What: Project files: `CLAUDE.md` (root or `.claude/`), `.claude/rules/*.md` (optional `paths` gating), `settings.json`, `settings.local.json` (personal, gitignored when Claude Code writes to it), `.mcp.json` (project-only MCP), `.worktreeinclude` (gitignored files copied into new worktrees), `skills/<name>/SKILL.md`, `commands/*.md`, `output-styles/*.md`, `agents/*.md`, `workflows/*.js` (saved dynamic workflow scripts, each becomes `/<name>`), `agent-memory/<name>/`. Global-only: `~/.claude.json` (app state, account, UI toggles, personal MCP servers), `projects/<project>/memory/` (auto memory), `keybindings.json`, `themes/*.json`. Also `CLAUDE.local.md` (private, manually gitignored), `AGENTS.md` (read in place of CLAUDE.md), `managed-settings.json` at an OS-level path, plugins under `~/.claude/plugins`. Frontmatter: skills (`name`, `description`, `when_to_use`, `argument-hint`, `arguments`, `disable-model-invocation`, `user-invocable`, `allowed-tools`, `disallowed-tools`, `model`, `effort`, `context`, `agent`, `background`, `hooks`, `paths`, `shell`, `metadata`, `license`, `compatibility`); agents (`name`, `description`, `tools`, `disallowedTools`, `model`, `permissionMode`, `maxTurns`, `autoCompactWindow`, `skills`, `mcpServers`, `hooks`, `memory`, `background`, `effort`, `isolation`, `color`, `initialPrompt`, `omitClaudeMd`, `experimental`); output styles (`name`, `description`, `keep-coding-instructions`, `force-for-plugin`); rules (`paths`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-directory.md (What's not shown; Choose the right file; File reference; Frontmatter fields by file)
- Notes: `CLAUDE_CONFIG_DIR` relocates every `~/.claude` path and `.claude.json`.

### Enterprise network configuration: proxies, CAs, mTLS
- What: Honors `https_proxy`/`HTTPS_PROXY`/`http_proxy`/`HTTP_PROXY` (first set wins) and `NO_PROXY` (space or comma lists, `*`); basic auth in the URL; no SOCKS support; WebSocket connections to loopback never proxied. Trusts bundled Mozilla CAs plus the OS store by default (`CLAUDE_CODE_CERT_STORE=bundled,system`; OS store needs native install or Node 22.15+); `NODE_EXTRA_CA_CERTS` for custom CAs with warnings when partially unreadable; `CLAUDE_CODE_GZIP_REQUEST_BODIES=0` for TLS-inspecting proxies that mishandle gzip. mTLS via `CLAUDE_CODE_CLIENT_CERT`, `CLAUDE_CODE_CLIENT_KEY`, `CLAUDE_CODE_CLIENT_KEY_PASSPHRASE`; re-read on settings apply and on connection-level failures (v2.1.232+) for rotation without restart, keeping the old pair if the new one is mismatched; OTLP exporters keep the first cert until restart. Only the proxy URL is validated at startup. Cloud sessions ignore TLS/mTLS/CA vars from settings `env`; Desktop-managed-provider sessions read TLS and proxy vars only from managed and user settings so a repo cannot redirect them (v2.1.217+).
- Status: GA
- Evidence: https://code.claude.com/docs/en/network-config.md (Proxy configuration; CA certificate store; Custom CA certificates; mTLS authentication; Verify your configuration)
- Notes: Recommends an LLM gateway for NTLM/Kerberos proxies.

### Network allowlist requirements
- What: Hosts Claude Code needs: `api.anthropic.com` (API, WebFetch safety check, feature flags, telemetry), `claude.ai`, `claude.com`, `platform.claude.com` (OAuth token exchange/refresh/revocation for both account types), `sso.claude.com`, `mcp-proxy.anthropic.com` (claude.ai connectors, on by default; disable with `ENABLE_CLAUDEAI_MCP_SERVERS=false` or `disableClaudeAiConnectors`), `downloads.claude.ai` (installer, updater, plugin executables), `storage.googleapis.com`, `registry.npmjs.org`, `bridge.claudeusercontent.com` (Chrome bridge), `*.frame.claudeusercontent.com` (artifact reads), `github.com`, `raw.githubusercontent.com` (changelog), `*-review.googlesource.com` (Desktop Gerrit lookup), Datadog intake hosts for telemetry and error reports, `formulae.brew.sh`, `code.claude.com`. On cloud providers/gateways model traffic and auth go there, but WebFetch preflight and the fast-mode availability check still go to `api.anthropic.com`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/network-config.md (Network access requirements; Cloud provider and LLM gateway routing; Organization IP allowlists and proxy egress; GitHub allow lists and firewalls)
- Notes: Telemetry goes to Datadog (`http-intake.logs.us5.datadoghq.com`, `browser-intake-us5-datadoghq.com`) only on direct Anthropic API use.

### Output styles
- What: Session-wide instruction sets for role, tone and format. Built-ins: Default (no style), Proactive (start work, assume routine decisions, no plan mode unless asked, still confirms destructive/shared actions), Concise (result first, no narration, v2.1.237+), Explanatory (`Insight` blocks), Learning (Insights plus `TODO(human)` handoffs where Claude stops for you to write code). Selected via `/output-style <name>` (works in `-p`, SDK, Remote Control for built-ins, v2.1.269+), `/config`, VS Code menu, or `outputStyle` (case-sensitive; mismatch silently gives Default). Custom styles: markdown in `~/.claude/output-styles`, `.claude/output-styles` (all levels between cwd and repo root, nearest wins), managed dir, or plugin `output-styles/`; frontmatter `name`, `description`, `keep-coding-instructions` (default false: drops the built-in software-engineering section of the full system prompt), `force-for-plugin`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/output-styles.md (Built-in output styles; Change your output style; Create a custom output style; Frontmatter reference; How output styles work)
- Notes: Applies to main conversation and forks only, not other subagents. Mid-session switches apply from the next message (v2.1.251+) and keep the cache. Custom style files are read at startup (restart after edits). `keep-coding-instructions` has no effect on the shorter system prompt; `CLAUDE_CODE_SIMPLE_SYSTEM_PROMPT=0` forces the full one.

### Disabling hooks
- What: `disableAllHooks: true` turns off hooks without deleting them; the value after settings precedence wins (a project `false` overrides a user `true`), so `--settings '{"disableAllHooks": true}'` is the reliable per-run off switch. No way to disable a single hook in place. Also disables a non-managed status line.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (Disable or remove hooks); https://code.claude.com/docs/en/statusline.md (Status line not appearing)
- Notes: The fact that a repository's project settings can re-enable hooks a user disabled is a notable precedence choice.

### Plugin `userConfig` (typed prompts, secure storage)
- What: Manifest-declared options (`type`: string/number/boolean/directory/file; `title`, `description`, `required`, `default`, `options` picker (v2.1.271+), `multiple`, `sensitive`, `min`/`max`). Prompted in a dialog on interactive install/enable or `/plugin configure`; shell `claude plugin install` never prompts but takes `--config KEY=VALUE`, and `claude plugin configure --values-stdin` accepts JSON. Non-sensitive values go to `pluginConfigs` in user settings; `sensitive` values go to the OS secure credential store, and are masked. Each non-sensitive option also appears in `/config` (v2.1.269+). Substitution: `${user_config.KEY}` in MCP and LSP configs, exec-form hook `args`, skill/agent content (sensitive values become placeholders there); `CLAUDE_PLUGIN_OPTION_<KEY>` exported to hooks. Fields that pass through a shell (shell-form hook commands, monitor commands, MCP `headersHelper`) reject `${user_config.*}` to prevent re-parsing injection.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/manifest-reference.md (User configuration; Where values are stored; Reference a saved value; Fields that run through a shell); https://code.claude.com/docs/en/plugins/components.md (Ask the user for configuration values)

### Plugin enablement layers, precedence and on-disk state
- What: Three stages: declared (settings `enabledPlugins`, `extraKnownMarketplaces`), fetched (`~/.claude/plugins/`: `known_marketplaces.json`, `installed_plugins.json`, `cache/<marketplace>/<plugin>/<version>/`, `data/`, `marketplaces/`, `synced/`, `.trash/`, `flagged-plugins.json`), loaded (startup or `/reload-plugins`; startup uses no network, marketplaces are checked in background afterwards). `enabledPlugins` sources by rising precedence: `--add-dir` (true only), user, project, local, `--settings` flag, managed (true force-enables, false blocks, unoverridable); merge per plugin id. Plugin ids are `<name>@<marketplace>`, `@inline` (`--plugin-dir`/`--plugin-url`/`CLAUDE_CODE_PLUGIN_DIRS`/SDK), `@skills-dir` (plugin folders under `~/.claude/skills/` or `.claude/skills/`), `@synced` (claude.ai). Plugins with external sources are fetched only when user, untracked local, flag or managed settings enable them (not project settings alone). Root overridable with `CLAUDE_CODE_PLUGIN_CACHE_DIR`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/loading.md (Check which stage a plugin reached; Find where a plugin came from; Find where a plugin is enabled; Enabled in project settings but not installed; Find plugins on disk)
- Notes: Explicit UI message when a user-level disable is overridden by a higher source. Old versions marked `.orphaned_at` and swept after 14 days so running sessions keep working. Corrupt `installed_plugins.json` is rebuilt with dated set-aside copies.

### Agent view configuration and dispatch defaults
- What: `claude agents` accepts `--permission-mode`, `--model`, `--effort` (incl. `ultracode`), `--agent`, `--dangerously-skip-permissions`, `--allow-dangerously-skip-permissions`, `--restricted` (v2.1.248), and passes `--settings`, `--setting-sources`, `--add-dir`, `--plugin-dir`, `--mcp-config`, `--strict-mcp-config` to dispatched sessions. `/model`, `/effort`, `/fast` in the dispatch input set per-window defaults (not saved). Dispatch prefixes: first-word or `@agent` runs a custom subagent as main agent, `@repo` targets a child repo or worktree, `/skill`, `!cmd`. Settings: `defaultToAgentsView`, `leftArrowOpensAgents`, `worktree.bgIsolation`, `disableAgentView` / `CLAUDE_CODE_DISABLE_AGENT_VIEW`. Keybindings in an `Agents` context.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/agent-view.md (Dispatch defaults; Set the model, effort, and fast mode; Settings, plugins, and MCP servers; Dispatch an agent from agent view; Turn off agent view)
- Notes: Permission-mode resolution for sessions dispatched from a ←-opened view: target dir `permissions.defaultMode` (auto/bypass only honored from managed, `--settings`, or user file; project files may not select a mode more permissive than the origin session), else origin session's mode. Bypass requires the disclaimer acceptance recorded in `~/.claude/settings.json`, else the session falls back to Manual with a notice. Background sessions inherit the dispatching shell's PATH, provider selection, `ANTHROPIC_DEFAULT_*_MODEL`, `CLAUDE_CODE_EXTRA_BODY`.

### Mod settings and environment variables
- What: `CLAUDE_CODE_PLUGIN_DIRS` (load dirs like `--plugin-dir`), `CLAUDE_CODE_PLUGIN_DIR_WATCH=1`, `prependPlugins`/`appendPlugins` (managed; user-level only on unmanaged non-Team/Enterprise machines), `allowManagedModsOnly`, `allowModsToOverrideDenyRules`, `allowManagedHooksOnly`, `disableAllHooks`, `disableSideloadFlags`, `pluginConfigs` (userConfig values keyed `plugin@marketplace` or `name@inline`, validated against the manifest's `userConfig`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/reference.md (Settings and environment variables; Commands)
- Notes: Commands: `/plugin`, `claude plugin validate [--strict] [--json]`, `claude plugin test`, `--plugin-dir`, `/reload-plugins`.

### What configuration carries into cloud sessions
- What: From the clone: `CLAUDE.md`, `.claude/rules/`, skills/agents/commands, and (single-repo sessions only) `.claude/settings.json` hooks and permissions and `.mcp.json`. Server-managed settings fetched at start (not in Claude Tag). Not carried: user `~/.claude/*`, user-scope plugins, local/user MCP servers, repo-declared plugins and marketplaces, transport env vars (`NODE_EXTRA_CA_CERTS`, mTLS) which are ignored and logged, interactive auth such as AWS SSO, MDM-delivered managed settings. claude.ai-enabled skills load automatically. Personal prefs: setup script writing `~/.claude/CLAUDE.md`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cloud-environments.md (What carries over from your setup; Add personal preferences without committing to the repo); https://code.claude.com/docs/en/claude-code-on-the-web.md (Manage context)
- Notes: Multi-repo sessions load no repo hooks (Anthropic-hosted). Project threads load only root CLAUDE.md and non-path rules. Env vars readable by anyone using the environment; `OTEL_*` withheld from commands.

### Per-session config assembly on runners
- What: Runner snapshots the host `~/.claude/` once at startup (settings.json, CLAUDE.md, hooks, agents, commands, skills; not `projects/`) and seeds each session's isolated `CLAUDE_CONFIG_DIR`; changes need a runner restart. `--host-config-snapshot disk` verifies every seeded file against an in-memory digest per session and refuses sessions on tampering; `memory` caps at 64 MiB. Only `mcpServers` from host `.claude.json` is seeded. Repo `.claude/settings.json` layers on top (single-repo, capacity 1 only), managed-settings file in the image applies. Control-plane-supplied hooks go to a reserved `hooks/.ccr-launcher/` via a separate `--settings` file and stay subject to `disableAllHooks`. Auto memory off by default (except Claude Tag). User's claude.ai skills downloaded for interactive sessions.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-configuration.md (How each session's config is assembled; MCP servers; Repository settings in sessions with several repositories); https://code.claude.com/docs/en/self-hosted-environments-reference.md (Runner CLI flags, --host-config-snapshot)
- Notes: `forceRemoteSettingsRefresh: true` in image managed settings makes sessions fail closed if server-managed settings can't load (but breaks Claude Tag sessions).

### Settings files and scopes
- What: Four JSON files: user `~/.claude/settings.json`; shared project `.claude/settings.json` (committed); project local `.claude/settings.local.json` (personal); managed (`managed-settings.json` and other managed sources). A fifth file `~/.claude.json` is app state: account, MCP server configs, per-project trust, and "global config" keys that only work there. Installing creates no settings file; `/config` creates the user file, and "Yes, and don't ask again" creates the local file. `CLAUDE_CONFIG_DIR` relocates the home directory files. The settings index tags every key with a Scope (`Any file`, `User or managed`, `User, local, or managed`, `Managed`, `Global config`).
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings.md (Settings files and who they affect; Find or create your settings files); https://code.claude.com/docs/en/settings-reference.md (Settings index)
- Notes: Settings reference groups about 250 keys into families: model and responses, permissions, sandbox, memory and context, interface and terminal, git and attribution, hooks and automation, plugins and skills, MCP, agents/sessions/worktrees, remote/desktop/notifications, authentication and providers, updates and versioning, tools (desktop only), privacy and telemetry, enterprise and managed, global config.

### Precedence order
- What: Highest first: managed > command line `--settings` > project local > shared project > user. `--settings` takes inline JSON or a file path, can set any user-settable key but not managed or global-config keys, lasts one session. Dedicated flags (`--model`, `--effort`, `--permission-mode`) are outside the stack and decided per key. Environment variables are not a level either: each variable/key pair has its own rule (`ANTHROPIC_MODEL` beats any `model` key; `ANTHROPIC_DEFAULT_MODEL` applies only when no file sets `model`; `CLAUDE_CODE_EFFORT_LEVEL` beats `--effort`). An `env` block in a file follows file precedence.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings.md (Settings precedence; Change a setting for one session); https://code.claude.com/docs/en/env-vars.md (Precedence)
- Notes: Each key's reference entry lists its per-session overrides.

### Merge semantics for lists
- What: List keys (e.g. `permissions.allow`) concatenate across files rather than override. Exceptions: `fallbackModel` taken whole from the highest file; `modelPicker` taken whole from the highest of managed/`--settings`/user and ignored in project/local (v2.1.242+); a managed `availableModels` replaces lower lists (non-managed scopes merge); `modelSettings` resolved per model. `autoMode` arrays concatenate, with a `"$defaults"` token to keep built-in rules at a chosen position. `allowedHttpHookUrls` and `httpHookAllowedEnvVars` merge across all files.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings.md (Lists merge instead of overriding); https://code.claude.com/docs/en/settings-reference.md (autoMode; allowedHttpHookUrls)
- Notes: Permission rule evaluation order (an `ask` in project beats a local `allow`) is in permissions.md, not this slice.

### Hot reload and startup-only keys
- What: Settings files are watched; most edits (permissions, hooks, `apiKeyHelper`) apply to the running session, including files created mid-session if the folder existed (the project `.claude/` folder even if created mid-session, v2.1.257+). A `ConfigChange` hook fires for file changes but not MDM/server updates, and cannot block a policy change. Startup-only: `model`, `effortLevel`/`modelSettings`, OTel exporter config, removal of an env var, `requiredMinimumVersion`, `forceRemoteSettingsRefresh`. Removing `blockReadsOutsideWorkingDirectories` mid-session does not lift the block until restart.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings.md (When edits take effect); https://code.claude.com/docs/en/env-vars.md (Set variables in settings files); https://code.claude.com/docs/en/server-managed-settings.md (Security considerations)
- Notes: After `/cd` (v2.1.246+) project files are re-read from the new directory.

### Broken settings handling
- What: Files are strict JSON (no comments, no trailing commas). Invalid JSON or schema-rejected value in a user/project/local file: interactive "Settings Error" dialog offering fix-with-Claude, exit, or continue without it; individual bad entries give a "Settings Warning" and are skipped. `-p` runs skip broken files silently (check `claude doctor`). A corrupt `~/.claude.json` is backed up to `~/.claude/backups/.claude.json.corrupted.<timestamp>`; `-p` exits, interactive asks to exit or reset. Settings files can carry `$schema` pointing to schemastore for editor validation; the schema can lag releases.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings.md (Fix a broken settings file; Edit a settings file)
- Notes: none.

### Local settings file placement and auto-gitignore
- What: Standing approvals are written as allow rules to `.claude/settings.local.json` at the git repository root (main checkout root for worktrees), not the start directory (v2.1.211+). On first write in a repo that does not ignore it, Claude Code adds `**/.claude/settings.local.json` to the global git excludes file (`core.excludesFile` or `$XDG_CONFIG_HOME/git/ignore`). Falls back to the start directory outside git, when the root is `$HOME`, on Windows, or when root/.git/.claude are not owned by the user. Its allow rules skip the workspace-trust step only while the file is untracked.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings.md (Keep personal settings out of a repository; Where Claude Code keeps the local file in a git repository)
- Notes: Writing to the user's global git config is a side effect worth noting. The Agent SDK `resolveSettings()` still reads the start directory.

### Trust-gated and repo-ignored keys
- What: From a committed project file, `permissions.allow`, `additionalDirectories`, `extraKnownMarketplaces`, project-sourced `apiKeyHelper` and most `env` values wait until each user trusts the folder; `deny` and `ask` apply immediately. Keys scoped `User`, `User or managed`, `Managed` or `Global config` never apply from a repo file. Inside `env`, project/local files cannot set: config/temp directory variables (`CLAUDE_CONFIG_DIR`, `HOME`, `TMPDIR`, `XDG_*`), Windows program-location variables, session-content exporters, OTel exporter/endpoint/content variables (except values that turn telemetry off), process wrapper and sync variables, OAuth token variables, accessibility and dialog-timer variables. Some variables (`CLAUDE_CODE_RESTRICTED`, dangerous-rm toggles, `CLAUDE_CODE_EVEN_MORE_DANGEROUSLY_SKIP_PERMISSIONS`, hosting identity vars) are ignored from every file and read only from the launch environment.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings.md (A committed key doesn't reach teammates); https://code.claude.com/docs/en/settings-reference.md (Variables Claude Code ignores in env; When Claude Code applies env values)
- Notes: Repo-ignore of OTel variables since v2.1.282; of directory and content-export variables since v2.1.251. Prevents a cloned repo from exfiltrating telemetry or redirecting config.

### env block semantics
- What: `env` maps names to strings copied verbatim (no `~` or `$HOME` expansion) into the process and every subprocess; it overrides the shell export, except when Desktop or a self-hosted runner builds the launch environment, where the launch env wins. A variable cannot be removed from a file, only set to `""`, which counts as unset for provider selection. `NO_COLOR`/`FORCE_COLOR` in `env` reach only subprocesses. Values are plaintext, so helpers are recommended for credentials.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-reference.md (env; How env values interact with your shell); https://code.claude.com/docs/en/env-vars.md (Set variables in settings files; Precedence)
- Notes: none.

### Environment variable catalogue and parsing conventions
- What: About 400 variables. Families: auth and endpoints (`ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL`, `CLAUDE_CODE_OAUTH_TOKEN`, profile/federation), provider selection (`CLAUDE_CODE_USE_BEDROCK/VERTEX/FOUNDRY/MANTLE/ANTHROPIC_AWS`, per-provider base URLs, skip-auth flags, `VERTEX_REGION_*`), model mapping (`ANTHROPIC_MODEL`, `ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU,FABLE}_MODEL[_NAME/_DESCRIPTION/_SUPPORTED_CAPABILITIES]`, custom model option, subagent model), network/timeouts/retry (`API_TIMEOUT_MS`, proxies, mTLS client certs, stream watchdogs), tools and limits (Bash timeouts and output length, MCP timeouts and batch sizes, web search caps, glob settings, subagent concurrency/depth/count), context and caching (auto-compact, 1M context, prompt-cache TTLs), feature disables (`CLAUDE_CODE_DISABLE_*`, about 60), commands hiding (`DISABLE_*_COMMAND`), telemetry/OTel, updates, terminal/UI, background tasks, plugins and sync. Booleans accept 1/true/yes/on and 0/false/no/off; numerics accept `2e3` and `64_000` (v2.1.211+). Exceptions where any non-empty value, even `0`, means on: `DISABLE_TELEMETRY`, `DISABLE_ERROR_REPORTING`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`, `IS_DEMO`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/env-vars.md (Variables)
- Notes: Feature names and gating appear only in this table for many variables; `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` and `CLAUDE_CODE_ENABLE_OPUS_4_7_FAST_MODE` suggest gated or experimental features (row text not read in full).

### Example settings files
- What: Three annotated examples: a personal user file (model, per-model effort, vim mode, status line command, allow rules, stable channel, 20-day retention); a team project file (allow/ask/deny, a PreToolUse hook script using `${CLAUDE_PROJECT_DIR}`, team marketplace and enabled plugin, sandbox with write path and domain allowlist, plans directory); an org managed file (login method and org pin, model allowlist with enforcement, deny rules, bypass disabled, managed-only permission and MCP rules, MCP pinned by URL, one marketplace, sandbox with `failIfUnavailable` and no unsandboxed retry and locked domains, minimum version, 7-day retention, announcement).
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-example.md (Your own settings; A team's shared settings; An organization's managed settings)
- Notes: The page states these are illustrative, not recommended baselines. It flags that `Bash(git push *)` does not match `git -C . push`.

### gateway.yaml structure and validation
- What: One YAML file read once at boot; five required sections (`listen`, `oidc`, `session`, `store`, `upstreams`), optional `login_notice`, `admin`, `enforcement`, `pricing`, `models`/`auto_include_builtin_models`, `managed`, `telemetry`, `access_control`, `limits`, `timeouts`, `rate_limits`, `load_test_mode`. Schema-validated at boot; unknown keys fail boot. Secrets via `${VAR}` (boot fails if undefined) or `${file:/path}` (whole-value only). Config edits require restart/redeploy.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-config.md (File structure; Secret expansion)
- Notes: Many keys are version-gated and an older gateway refuses to start when it sees them, so docs repeatedly say "upgrade every replica first, remove the key before rollback".

### Client-side settings that point Claude Code at a gateway
- What: `forceLoginMethod: "gateway"`, `forceLoginGatewayUrl`, `parentSettingsBehavior: "merge"` in a managed source (file, plist, HKLM, policy helper) open `/login` on the Cloud gateway screen. No gateway option exists in the login picker. User settings (`~/.claude/settings.json` only, v2.1.295+) can name a gateway when no managed settings exist. `allowedProviders: ["gateway"]` blocks bypass via provider env vars or custom `ANTHROPIC_BASE_URL` (v2.1.285+). Login keys must not be put in the gateway payload.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway.md (Set the gateway URL; Set the gateway URL in user settings); https://code.claude.com/docs/en/claude-apps-gateway-config.md (Client-side managed settings)
- Notes: A plist/registry policy replaces the file rather than merging, so all three keys must sit in the winning source.

### Parent settings from embedding hosts and the allowManaged*Only locks
- What: Settings a launching process (Claude Desktop, Agent SDK `managedSettings`, IDE extension) passes are "parent settings"; ignored on machines with admin-deployed managed settings unless the selected source sets `parentSettingsBehavior: "merge"`. They are filtered to restrictive keys, but some allow-direction keys pass unless the five locks are set (`allowManagedPermissionRulesOnly`, `allowManagedMcpServersOnly`, `allowManagedHooksOnly`, `sandbox.network.allowManagedDomainsOnly`, `sandbox.filesystem.allowManagedReadPathsOnly`). Parent `sandbox.credentials` entries are forwarded stripped (deny-only, sentinel masks, no re-signing `awsPairs`). Some keys still pass with all locks (`forceLoginOrgUUID`, `allowedMcpServers`, `availableModels`, `allowedProviders`, `strictKnownMarketplaces`, `blockedMarketplaces`, `strictPluginOnlyCustomization`) when the winning source does not set them.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway.md (Deliver policy to Claude Desktop sessions; Restrict parent settings; Settings the locks don't cover; How hook and skill keys in parent settings take effect)
- Notes: Documented silent-failure trap: Desktop-only machines without the merge opt-in run embedded sessions without the egress allowlist "and nothing warns you". `policyHelper` fleets never merge parent settings.

### Options object, env semantics and runtime reconfiguration
- What: Every option is optional; key families: model/fallbackModel (comma list, primary retried each user turn), effort, thinking, cwd/additionalDirectories, env, limits, tools/permissions, hooks, MCP, agents, skills, plugins, sessions, sandbox, outputFormat. No `temperature`/`top_p`/`max_tokens` options. In streaming mode `setModel()`/`setPermissionMode()` (both SDKs) and TS `applyFlagSettings()` (any settings keys on the flag layer; only some take effect mid-session, system prompt never) and `updateSettings()` (persist one allowlisted key: `outputStyle` to local, `effortLevel` to user). `maxTurns: 0` means unlimited, `maxBudgetUsd: 0` is rejected.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/configuration.md (Pass options; Choose a model; Set environment variables; Limit turns and spend; Change configuration mid-session); https://code.claude.com/docs/en/agent-sdk/typescript.md (Options; applyFlagSettings(); updateSettings())
- Notes: TS `env` replaces the child environment, Python merges: documented on at least five pages. A mid-session model switch loses the prompt cache. `applyFlagSettings` is TS-only. No setter for `cwd`.

### Configuration: `/config key=value`, safe mode, project-scope limits
- What:
  - `/config key=value` sets any setting from the prompt, in -p, and over Remote Control (2.1.181).
  - `--safe-mode` starts with all customizations off (2.1.169).
  - Project settings are limited for safety: no `bypassPermissions` default (Week 36), no Remote Control auto-connect (Week 32).
  - Telemetry variables in a project's settings are reported when ignored (2.1.282).
  - Managed settings: version requirements (Week 23); a "Skipped sources" status line (2.1.243); a warning on a symlink outside the folder (2.1.290).
  - `CLAUDE_CODE_DISABLE_FEATURE_FLAGS` stops flag fetching without disabling telemetry (2.1.319).
- Status: GA
- Evidence: https://code.claude.com/docs/en/whats-new/2026-w23.md, https://code.claude.com/docs/en/whats-new/2026-w24.md, https://code.claude.com/docs/en/whats-new/2026-w25.md, https://code.claude.com/docs/en/whats-new/2026-w32.md, https://code.claude.com/docs/en/whats-new/2026-w36.md; https://code.claude.com/docs/en/changelog.md (2.1.243, 2.1.282, 2.1.290, 2.1.319)
- Notes: `CLAUDE_CODE_DISABLE_FEATURE_FLAGS` appears in no docs page in the dump (only 4 days old). Feature-flag gating also explains why some features, such as `/skill-doctor`, are missing in telemetry-off sessions (https://code.claude.com/docs/en/whats-new/2026-w36.md).

## Area 21. Security and privacy

### Invisible-character stripping in prompts
- What: On Enter, Claude Code removes Unicode tag characters, bidi controls and zero-width spaces from the prompt and from collapsed pasted-text references, keeping Persian/Indic joiners and emoji selectors. If anything was removed, the first Enter sends nothing and shows `Removed N invisible characters · review and press Enter to send`. For CLI-argument prompts it strips and sends with a notice, unless the result starts with `/`, which is put in the input for review.
- Status: GA
- Evidence: https://code.claude.com/docs/en/interactive-mode.md (Invisible characters in prompts)
- Notes: Stated purpose is to stop copied text from carrying hidden instructions.

### Pasted text marked as untrusted to the model
- What: Pastes over 800 characters or three lines collapse to `[Pasted text #N +L lines]` and are sent in full. On submit, Claude sees paste contents marked as pasted from elsewhere and is told to follow instructions inside a paste only where the typed message asks it to. Paste contents are cached under `~/.claude/paste-cache/` so recalled prompts resend them; after the retention sweep, a missing paste is dropped from plain prompts but cancels submission for `!` and `/` commands (removal would change what runs).
- Status: GA
- Evidence: https://code.claude.com/docs/en/terminal-config.md (Paste large content; How Claude treats pasted text; Recall a prompt that had pasted text)
- Notes: Pastes are not marked in sessions that do not fetch feature flags (third-party providers, telemetry off).

### Local application data, plaintext storage and retention sweep
- What: `~/.claude` holds plaintext data written during sessions: transcripts that include not only messages and tool results but also the system prompt, tool definitions, request parameters and loaded instruction files; subagent transcripts under `<session>/subagents/`; spilled large tool outputs under `<session>/tool-results/`; MCP task state; `file-history/<session>/` checkpoint snapshots; `plans/`; `debug/`; `paste-cache/`; `uploads/` for Remote Control attachments; `dev-mods/`; `session-env/`; `tasks/`; `shell-snapshots/` (removed on clean exit); `backups/` of `~/.claude.json` (five newest); feedback bundles/drafts; `usage-data/`. All are deleted after `cleanupPeriodDays` (default 30, minimum 1; 0 is a validation error); the sweep pauses if retention cannot be determined (unparseable settings) and shows a `/status` warning; skipped in `--bare`. Kept until deleted: `history.jsonl` (typed prompts with project path), `stats-cache.json`, `remote-settings.json` (server-managed settings cache, checked at startup and hourly), `policy-limits.json`. State to keep: `.credentials.json`, `agent-memory/`, `jobs/`, `daemon/`. Nothing is encrypted at rest; any secret a tool reads or prints lands in the transcript. Mitigations: lower `cleanupPeriodDays`, `CLAUDE_CODE_SKIP_PROMPT_HISTORY`, `--no-session-persistence`, SDK `persistSession: false` (TypeScript only), deny rules on credential files.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-directory.md (Application data; Cleaned up automatically; Kept until you delete them; State files to keep; Plaintext storage)
- Notes: Auto memory files are not swept. A `sessions/` directory holds one file per running session to detect concurrent sessions and crashes. Superseded or orphaned transcripts are set aside rather than overwritten.

### `claude purge` (per-project local data deletion)
- What: Deletes a project's transcripts and auto memory, per-session `tasks/`, `debug/`, `file-history/`, its lines in `history.jsonl`, and its `~/.claude.json` entry; prints a plan and asks `[y/N]`; `--dry-run`, `--yes`, `-i`, `--all` (deletes `history.jsonl` outright). Leaves `shell-snapshots/`, `backups/`, temp-dir images and scratchpads, and `/heapdump` snapshots (which contain conversation and credentials).
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-directory.md (Clear local data)
- Notes: Renamed from `claude project purge` in v2.1.288.

### Workspace trust gating repository-supplied config
- What: Project `.claude/settings.json` `permissions.allow` and `additionalDirectories` take effect only after the interactive workspace-trust dialog (which lists them); deny/ask apply immediately. Trust is keyed on the git repo root (main checkout for worktrees), excluding nested repos; outside a repo on the start directory; in the home directory it is session-only. `.claude/settings.local.json` is treated as repo-supplied (needs trust) if tracked in git or if `.claude` is a symlink. `claude -p` and SDK sessions never show the dialog: repo allow rules are not used (stderr warning), but hooks, `env`, `apiKeyHelper`, skill `allowed-tools` and `.mcp.json` servers ARE used/connected without asking. `claude --bg` checks trust and errors in scripts.
- Status: GA
- Evidence: https://code.claude.com/docs/en/permissions.md (Project allow rules and workspace trust; When your local settings file needs trust; What runs before you trust a folder)
- Notes: Hardening advice for running `-p` on untrusted repos: `--setting-sources user`, `--bare`, `--settings '{"disableAllHooks": true}'` (user settings alone are insufficient because project settings outrank them), `disabledMcpjsonServers`. Manual trust: `projects["<path>"].hasTrustDialogAccepted` in `~/.claude.json`. Notable: headless mode on an untrusted repo still runs that repo's hooks.

### `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB`
- What: Environment variable that strips credentials from all subprocesses regardless of sandboxing; on Linux it also forces every command (including `!` and excluded commands) to run sandboxed and makes `filesystem.disabled` ignored from all sources, and makes `allowNestedUserNamespaces`-style fixes ineffective.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sandboxing.md (What the sandbox restricts; Turn off the retry with strict sandbox mode; Which settings can disable it; Chromium or rootless Podman fails inside the sandbox on Linux)
- Notes: Exact list of scrubbed variables not on these pages (env-vars page not in slice).

### Prompt-injection defences (as documented)
- What: Layers listed: permission prompts in Manual mode; `curl`/`wget` and network-fetching commands never auto-approved by default; WebFetch usually runs a separate model call over the page and Claude receives that call's answer rather than raw page content; workspace trust dialog and separate `.mcp.json` server approval; "command injection detection" asks before any Bash command it cannot fully analyze, even if a partial allow rule like `Bash(git *)` matches; fail-closed matching (unmatched commands need approval). In auto mode, tool results are stripped from client-side classifier requests, and a separate server-side probe scans incoming tool results for suspicious content before Claude reads them.
- Status: GA
- Evidence: https://code.claude.com/docs/en/security.md (Protect against prompt injection; Core protections; Additional safeguards); https://code.claude.com/docs/en/permission-modes.md (How the classifier evaluates actions)
- Notes: Docs also recommend VMs for scripts and external web interaction, and `/feedback` for suspicious behaviour. Windows WebDAV warning: enabling it can let Claude Code trigger network requests that bypass the permission system. No details given on the server-side probe beyond its existence (links to an external blog/engineering post not opened).

### Working directory boundary is a prompt, not an enforcement boundary
- What: In Manual mode file tools prompt before reading or writing outside the launch folder, but an approved Bash command can write anywhere the user can; OS enforcement needs the sandbox.
- Status: GA
- Evidence: https://code.claude.com/docs/en/security.md (Built-in protections)

### Credential storage (summary from security page)
- What: API keys and tokens are stored in the macOS Keychain when available; on Linux in a file with mode `0600`; on Windows in a file inheriting the user profile directory ACLs.
- Status: GA
- Evidence: https://code.claude.com/docs/en/security.md (Additional safeguards)
- Notes: See the authentication entry for full detail.

### Security operations and team practices
- What: Anthropic Trust Center (SOC 2 Type 2, ISO 27001); vulnerability reports via HackerOne; recommended team practices include managed settings, version-controlled permission configs, OpenTelemetry monitoring, and `ConfigChange` hooks to audit or block settings changes during a session.
- Status: GA (certifications are `vendor-claim` on this page)
- Evidence: https://code.claude.com/docs/en/security.md (Security foundation; Team security; Reporting security issues)

### Data training policy and retention
- What: Consumer (Free/Pro/Max/Ultra): training on user data only when the user's setting is on; retention 5 years if allowed, 30 days if not. Commercial (Team/Enterprise/API/3rd-party platforms/Claude Gov): no training on Claude Code prompts or code unless opted in (e.g. Development Partner Program, first-party API only); standard 30-day retention; ZDR for qualified Claude for Enterprise orgs. `/feedback`, `/bug`, `/share` transcripts retained 5 years. Local transcripts stored in plaintext under `~/.claude/projects/` for 30 days by default (`cleanupPeriodDays`); Desktop/Cowork-continued sessions exempt by default. Cloud sessions deletable (permanently removes event data).
- Status: GA
- Evidence: https://code.claude.com/docs/en/data-usage.md (Data training policy; Development Partner Program; Feedback using the /feedback command; Data retention)
- Notes: Plaintext local transcripts are a notable default for a harness that runs on shared machines.

### Session quality survey and transcript-share follow-up
- What: The "How is Claude doing this session?" rating records only the rating. An optional follow-up asks to upload the transcript, subagent transcripts and raw session log; known API key/token patterns are redacted but code and content go as-is; retained up to 6 months; on Bedrock/Agent Platform/Foundry/apps gateway it writes a local archive under `~/.claude/feedback-bundles/` instead. Never shown under ZDR, org-disabled feedback, or `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`. Disable with `CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY=1` (also by `DISABLE_TELEMETRY`, `DO_NOT_TRACK`); `feedbackSurveyRate` sets probability; `CLAUDE_CODE_ENABLE_FEEDBACK_SURVEY_FOR_OTEL=1` routes ratings only to an org OTel collector.
- Status: GA
- Evidence: https://code.claude.com/docs/en/data-usage.md (Session quality surveys)

### Telemetry: metrics, error reports, feedback, and per-provider defaults
- What: Metrics (latency, reliability, usage; never code, prompts or file paths) go to Anthropic and third-party logging over TLS; opt out `DISABLE_TELEMETRY=1`. Error reports (internal stack traces with secrets, paths, emails redacted client-side) to a third-party error tracker; on only for Pro/Max/Ultra sign-ins on v2.1.198+ direct to the Claude API without ZDR/HIPAA; opt out `DISABLE_ERROR_REPORTING=1`. `/feedback` sends conversation history incl. code to Google Cloud Storage (choice of current session, or also 24h/7d of project sessions), optionally a public GitHub issue; opt out `DISABLE_FEEDBACK_COMMAND=1`; on third-party providers or with no Anthropic creds it writes a local redacted bundle. `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` disables all non-essential traffic; it and `DISABLE_TELEMETRY` also disable feature-flag evaluation (breaking Remote Control and other flag-gated features). On Bedrock, Agent Platform, Foundry and Claude Platform on AWS, metrics, error reports and feedback are off by default (unless `CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST` turns metrics on); apps gateway credentials disable analytics with no override. Surveys and the WebFetch check run on every provider.
- Status: GA
- Evidence: https://code.claude.com/docs/en/data-usage.md (Telemetry services; Default behaviors by API provider)
- Notes: Official marketplace auto-install has its own opt-out `CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL`.

### WebFetch domain safety check (preflight)
- What: Before WebFetch fetches a URL, only the hostname is sent to `api.anthropic.com` for a check against an Anthropic-maintained blocklist; passes are cached five minutes, blocked or failed hostnames re-checked next time. Runs on every provider and is not disabled by `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`; if `api.anthropic.com` is blocked WebFetch fails until allowlisted or `skipWebFetchPreflight: true`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/data-usage.md (WebFetch domain safety check)

### Data flow and encryption at rest by provider
- What: Claude Code runs locally and sends all prompts and model outputs to the provider over TLS 1.2+. At rest: Anthropic API AES-256 disk encryption (ZDR for no persistence); Bedrock AES-256 with AWS keys or KMS CMK; Agent Platform Google keys or CMEK; Foundry depends on hosting (Hosted on Azure keeps prompts in Azure except usage metadata and safety-flagged content).
- Status: GA
- Evidence: https://code.claude.com/docs/en/data-usage.md (Local Claude Code: Data flow and dependencies)

### Zero Data Retention (ZDR)
- What: For qualified Claude for Enterprise orgs, enabled per organization by Anthropic (not from admin settings; new orgs need separate enablement; enablement audit-logged). Prompts and responses are processed in real time and not stored, except for legal requirements or misuse (flagged sessions retained up to 2 years). Covers Claude Code inference only on Anthropic's direct platform; applies only to requests authenticated into the ZDR org (personal accounts or other-org API keys are not covered; enforce with `forceLoginMethod` / `forceLoginOrgUUID`). Not covered: claude.ai chat, Cowork, artifacts, analytics metadata, seat management data, third-party integrations/MCP. Disabled at the backend under ZDR: cloud sessions, unified Chat/Cowork, Claude Tag, Artifacts in Claude Code, Claude Docs, `/feedback`/`/bug`/`/share`, Remote Control. Fable 5.x models require retention by default and are governed by Covered Models policy (hidden/disabled, server rejects).
- Status: GA (sales-gated)
- Evidence: https://code.claude.com/docs/en/zero-data-retention.md (ZDR scope; Route Claude Code traffic to your ZDR organization; What ZDR does not cover; Features disabled under ZDR; Model availability under ZDR; Data retention for policy violations)
- Notes: Pattern of server-side feature gating "regardless of client-side display" is worth copying: policy enforced at the backend, not just hidden in the UI.

### Healthcare (BAA) coverage
- What: A BAA extends to Claude Code either via the HIPAA configuration (CLI or Desktop Code tab with Claude for Enterprise, HIPAA enabled and the configuration applied to Claude Code local mode and Cowork local mode) or via ZDR. Under the HIPAA configuration, cloud sessions, Remote Control, mobile, and third-party cloud providers or gateways are not covered.
- Status: GA
- Evidence: https://code.claude.com/docs/en/legal-and-compliance.md (Healthcare compliance (BAA)); https://code.claude.com/docs/en/zero-data-retention.md (intro)

### Local session data management and purge
- What: Local session data on each machine is the org's responsibility. The retention sweep (`cleanupPeriodDays`) runs only when Claude Code starts, so idle machines keep data. `claude purge --all --yes` (v2.1.288+; `claude project purge` from v2.1.126) deletes all projects' transcripts and auto memory, `tasks/`, `debug/`, `file-history/`, `history.jsonl` and project entries in `~/.claude.json`; it leaves e.g. `paste-cache/`. Removing a seat, or `/logout`, deletes nothing locally. Under HIPAA, Desktop deletes Code tab sessions inactive beyond `cleanupPeriodDays` (removing clean worktrees).
- Status: GA
- Evidence: https://code.claude.com/docs/en/hipaa-setup.md (Manage local session data; Delete session data right away; Offboard a developer)

### Authentication methods and login flow
- What: First launch opens a browser login (press `c` to copy URL; paste-code fallback), skipped when `ANTHROPIC_API_KEY` is set and approved or an earlier `claude auth login` exists. Account types: Pro/Max/Ultra (claude.ai), Teams/Enterprise (claude.ai org invite), Claude Console (with or without creating an API key), cloud providers (Bedrock, Google Agent Platform, Foundry via env vars or a "3rd-party platform" wizard), and a self-hosted Claude apps gateway (corporate SSO via `/login`; gateway token is the only credential). `/logout` also resets first-launch state. Multiple accounts via separate `CLAUDE_CONFIG_DIR` directories (except keyless Console sign-ins, stored outside the config dir).
- Status: GA
- Evidence: https://code.claude.com/docs/en/authentication.md (Log in to Claude Code; Login prompt still appears after an earlier sign-in; Log in with multiple accounts; Set up team authentication)
- Notes: Console roles: "Claude Code" role can only create Claude Code API keys; "Developer" any key.

### Keyless Console sign-in (Anthropic profile) and federation credentials
- What: Since v2.1.242, Console users can sign in without creating an API key: the OAuth token is stored as an Anthropic profile (refreshed automatically) instead of a static key (the "legacy" option). Profiles live in `~/.config/anthropic` (or `%APPDATA%\Anthropic`), auth mode `user_oauth` or `oidc_federation` (Workload Identity Federation via `ANTHROPIC_FEDERATION_RULE_ID` + `ANTHROPIC_ORGANIZATION_ID`, `ANTHROPIC_IDENTITY_TOKEN_FILE`). Source order: `ANTHROPIC_PROFILE`, federation variables, active profile (`active_config` or `default`). The keyless option is not offered on cloud providers, when `forceLoginOrgUUID` or `forceLoginMethod: "claudeai"` is set, or when managed settings exist but are unreadable. Profiles are not read in bare mode, Desktop or cloud sessions; claude.ai-only features (connectors, `/schedule`) are unavailable while a profile is selected.
- Status: GA
- Evidence: https://code.claude.com/docs/en/authentication.md (Sign in without an API key; Stop offering the keyless sign-in on managed machines; Anthropic profiles and federation credentials)
- Notes: WIF is a good model for keyless CI/worker auth.

### Authentication precedence
- What: Order: (1) cloud provider creds when `CLAUDE_CODE_USE_BEDROCK|VERTEX|FOUNDRY` set; (2) `ANTHROPIC_AUTH_TOKEN` (Bearer, for gateways); (3) `ANTHROPIC_API_KEY` (`X-Api-Key`; interactive approval prompt once, remembered, toggle in `/config`; always used in `-p`); (4) `apiKeyHelper` output; (5) `CLAUDE_CODE_OAUTH_TOKEN`; (6) Anthropic profile / federation credentials (a `user_oauth` active profile ranks below a working `/login`); (7) subscription OAuth from `/login`. A signed-in Claude apps gateway session outranks everything including cloud provider flags. Managed `forceLoginMethod: "gateway"` or `forceLoginGatewayUrl` makes the gateway sign-in the only accepted credential (v2.1.261+/v2.1.265+). Cloud sessions always use subscription credentials; env keys don't override.
- Status: GA
- Evidence: https://code.claude.com/docs/en/authentication.md (Authentication precedence)
- Notes: Docs warn that a stale `ANTHROPIC_API_KEY` silently wins over a subscription once approved; `/status` marks the unused credential.

### Credential storage, helper scripts and expiry
- What: macOS: encrypted Keychain; if the Keychain rejects the write (e.g. locked over SSH) falls back to `~/.claude/.credentials.json` mode `0600`. Linux: `~/.claude/.credentials.json` mode `0600`. Windows: `%USERPROFILE%\.claude\.credentials.json` inheriting profile ACLs. `CLAUDE_CONFIG_DIR` relocates the file and keys the Keychain entry per directory. `apiKeyHelper` runs a shell script that returns a key (slow-helper notice after 10 s; requests fail within three attempts if it errors, times out or prints nothing). `apiKeyHelper`, `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` apply to CLI, VS Code, Agent SDK and GitHub Actions but not Desktop or cloud sessions. Startup warning three days before a `/login` expires; `/status` `Login` row shows expired state (v2.1.210+); unattended background and Remote Control sessions stall once the credential expires.
- Status: GA
- Evidence: https://code.claude.com/docs/en/authentication.md (Credential management; Renew an expiring login)
- Notes: Supported types listed: claude.ai, Claude API, Foundry, Bedrock, Vertex, Anthropic profile/WIF, apps gateway tokens. Linux storage is a plaintext 0600 file (no keyring/libsecret integration documented).

### Hook and skill input hardening details
- What: File-tool paths are absolutised before hooks so `~`/relative spellings cannot bypass path checks; `<system-reminder>` tags inside hook context are escaped; synced-skill descriptions have control chars removed and angle brackets escaped; WorktreeCreate refuses `..` and in-repo symlink paths; CLAUDE.md symlinks to network paths are not followed; HTTP hook env interpolation requires explicit allowlisting; plugin user_config values substituted only in exec form. Docs note a blocked prompt still persists in transcript/history on disk.
- Status: GA
- Evidence: https://code.claude.com/docs/en/hooks.md (PreToolUse input; Add context for Claude; WorktreeCreate output; HTTP hook fields; Exec form and shell form; What a blocked prompt leaves behind); https://code.claude.com/docs/en/skills.md (How Claude Code handles the frontmatter of a synced skill); https://code.claude.com/docs/en/memory.md (Share rules across projects with symlinks)
- Notes: Security best-practice list in the hooks reference is generic (quote variables, block `..`, skip `.env`).

### MCP config hygiene and secret redaction in output
- What: Warnings for hidden leading/trailing whitespace in `command`, `url`, `args`, `env`/`headers` keys and values (named, value not echoed, not trimmed); same name in several scopes with different endpoints; reserved names (`workspace`, `claude-in-chrome`, `computer-use`, `Claude Preview`, `Claude Browser`) skipped; missing `${VAR}` with no default (loads with literal text). Failure detail is credential-redacted and never includes the expanded URL; `Connection error` gets no detail at all because exception text can embed the URL. Post-auth failure messages show only the origin (scheme/host/port), never path or query. `${VAR}` references are shown unexpanded in `/mcp` (v2.1.268+) and `list`/`get`. Plugin stdio/ws server values are shown as `[REDACTED]` in `claude mcp get`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Configuration warnings; Connection failures and unconfigured servers; How references appear in /mcp and CLI output; Plugin-provided https://code.claude.com/docs/en/mcp.md servers)
- Notes: An empty `url` shows `not configured` and is not an error, which lets plugins ship placeholder connectors.

### Credential variables blanked toward remote servers
- What: In a remote server's `url` and `headers`, `${VAR}` references to credential variables (Claude Code's own such as `ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN`, cloud provider credentials such as `AWS_BEARER_TOKEN_BEDROCK`, and others such as `HTTPS_PROXY`, `NPM_TOKEN`) always expand to empty, and `:-default` is ignored, so a repo `.mcp.json` or plugin cannot exfiltrate them to a server it names. `ANTHROPIC_BASE_URL` still expands unless it embeds userinfo. A debug-log line flags attempts.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Credential variables that read as empty; Environment variable expansion in .mcp.json)
- Notes: Expansion syntax `${VAR}` and `${VAR:-default}` in `command`, `args`, `env`, `url`, `headers`; empty string counts as set. Users must copy a covered secret into a differently named variable to pass it deliberately.

### MCP OAuth 2.0 client
- What: Remote servers returning 401/403 are flagged "needs authentication" and signed in via `/mcp` (browser) or `claude mcp login <name>` (prints URL on headless/SSH, `--no-browser`, paste-back of redirect URL). Discovery: RFC 9728 protected-resource metadata, then RFC 8414 AS metadata; `WWW-Authenticate` pointer honoured; supports Dynamic Client Registration and Client ID Metadata Documents (CIMD); pre-registered clients via `--client-id`, `--client-secret` (masked prompt or `MCP_CLIENT_SECRET`), `--callback-port` (redirect `http://localhost:PORT/callback`, random port by default). Overrides in the entry's `oauth` object: `authServerMetadataUrl` (https only), `scopes` (space-separated, pins requested scopes; `offline_access` appended if advertised; without it Claude Code does not request the full `scopes_supported` catalog). 401 with stored token: refresh and retry once; no refresh token: retry unauthenticated once. 403 `insufficient_scope` names the needed scope. Client secret stored in macOS keychain or a credentials file, not config. Startup notice counts servers needing sign-in (once per server). `-p`/SDK cannot run OAuth; must sign in interactively first.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Authenticate with remote https://code.claude.com/docs/en/mcp.md servers; Authenticate from the command line; Use a fixed OAuth callback port; Use pre-configured OAuth credentials; Override OAuth metadata discovery; Restrict OAuth scopes)
- Notes: A configured `Authorization` header (static or helper) disables OAuth fallback. OAuth sign-ins stored per endpoint. v2.1.229 regression sent `127.0.0.1` redirect; fixed v2.1.231. Gateway/proxy headers (except Authorization) are added to sign-in, refresh and revocation requests to the server's own origin from v2.1.299. Some Anthropic-hosted connectors (Microsoft 365, Gmail, Google Calendar) cannot do local OAuth; must be connected via claude.ai.

### `headersHelper` dynamic auth headers
- What: A shell command that prints a JSON object of string headers; run fresh at each connect and reconnect (no caching), 10 s timeout, output overrides static headers of the same name; re-run and retried once on a 401/403 tool call. Env passed: `CLAUDE_CODE_MCP_SERVER_NAME`, `CLAUDE_CODE_MCP_SERVER_URL`, `CLAUDE_PLUGIN_ROOT`. Working directory depends on config origin (plugin root; project dir for project/local; session cwd for agent files, SDK, `--mcp-config`; `~/.claude` for user, managed, connectors). For project `.mcp.json` and local-scope servers the helper runs only after the folder's own trust dialog is accepted (parent-folder trust does not count; before v2.1.238 `-p`/SDK ran them unchecked); until then the server connects with static headers only. Helpers from repo or plugin sources run with credential-looking env vars stripped (names containing TOKEN, SECRET, PASSWORD, KEY, AUTH, any case, plus a fixed list such as `ANTHROPIC_CUSTOM_HEADERS`; Git `GIT_CONFIG_KEY_<n>` excepted), and the URL passed to them has such values replaced by `REDACTED`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/mcp.md (Use dynamic headers for custom authentication; Where the helper runs; Which variables a helper can read; Trust a folder before its headersHelper runs)
- Notes: Plugin `headersHelper` cannot use `${user_config.*}` (shell-parsed; changed in v2.1.207); put such values in `headers` instead.

### Marketplace trust tiers and anti-impersonation
- What: Three tiers by marketplace name: official (e.g. `claude-plugins-official`, `claude-code-plugins`, `knowledge-work-plugins`...), community (`claude-community`, `claude-plugins-community`, `healthcare`), third-party (everything else). Official and community names are accepted only when sourced from `github.com/anthropics/`; otherwise the marketplace and its installed plugins stop loading ("registered from an untrusted source"). The community catalog pins nearly every entry to a commit SHA and Claude Code refuses a different commit. `/plugin` shows the same trust warning for every plugin regardless of tier (org can append text via `pluginTrustMessage`). The details pane lists a "Will install" component summary; `claude --plugin-dir <dir> plugin details <name>` prints a component inventory without starting a session.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/security.md (Identify Anthropic's marketplaces by name; Review a plugin before you install; Recognize when Claude Code refuses or warns); https://code.claude.com/docs/en/plugins/overview.md (Tell Anthropic's marketplaces from third-party ones)
- Notes: Telemetry redacts community/third-party plugin names to `third-party` unless `OTEL_LOG_TOOL_DETAILS=1`.

### Plugin Node.js dependency install (hardened)
- What: When a plugin is copied into the cache and its root has `package.json` plus a supported lockfile (`bun.lock`, `npm-shrinkwrap.json`, `package-lock.json` v2/v3), Claude Code installs dependencies automatically with: registry packages only, exact pinned versions, `https` downloads (except user's default registry), a separate install folder so `.npmrc`/`.env`/`bunfig.toml` are not read, frozen resolution, `--ignore-scripts`, no npm `overrides` / Bun `patchedDependencies`, 60 s timeout. Yarn/pnpm/`bun.lockb` are skipped. Cannot be turned off. Failure leaves the plugin loaded with a note.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/loading.md (Node.js package dependencies; Limits on the dependency install; When the dependency install fails or is skipped)

### Private marketplace access and git hardening
- What: Claude Code holds no git token; it runs the user's `git` non-interactively (prompts and askpass, including `GIT_ASKPASS`, disabled; credential helpers allowed but cannot prompt). SSH requires a passphrase-less/agent key and an existing `known_hosts` entry. GitHub `owner/repo` probes `ssh -T git@github.com` then falls back to HTTPS (`CLAUDE_CODE_PLUGIN_PREFER_HTTPS=1` skips). Refuses a `git` found only in the current directory (Windows). Clone timeout 120 s (`CLAUDE_CODE_PLUGIN_GIT_TIMEOUT_MS`); `--sparse` checkout for monorepos. Background auto-update with an unreachable or unauthenticated remote re-clones and keeps the old checkout if that fails (`CLAUDE_CODE_PLUGIN_KEEP_MARKETPLACE_ON_FAILURE=1` skips the re-clone). Git LFS content is never fetched.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/host-marketplace.md (Grant access to a private marketplace; What background auto-update does with credentials; Keep plugin files out of Git LFS); https://code.claude.com/docs/en/plugins/install.md (Add a private marketplace); https://code.claude.com/docs/en/plugins/troubleshooting.md (Command 'git' not found or is in an unsafe location; Git clone timed out after 120s)
- Notes: In plugin evals, git hooks and credential helpers are switched off via `GIT_CONFIG_COUNT`, which is why evals require git 2.31+ (https://code.claude.com/docs/en/plugin-evals.md, "is too old for claude plugin eval").

### Download limits and archive/header hygiene for hosted marketplaces
- What: `url` marketplace `marketplace.json`: max 5 MiB, 10 s response, cross-origin redirects must be https and not to loopback/link-local/cloud-metadata hosts. Archive zips: max 256 MiB, 120 s, at most 5 redirects (all https, same host restrictions); extraction limits 100,000 entries, 512 MiB per file, 1 GiB total, 50x compression ratio. Configured headers are never forwarded on cross-origin redirects. Entry headers drop routing/identity names (`Host`, `Cookie`, `X-Forwarded-*`). Archive `headersHelper` (v2.1.238+): requires `strict: false` on the entry, is accepted by the user on each single-plugin install/update (refused in bulk installs, dependency installs, suggestions and auto-update), command text max 500 printable ASCII, 10 s, cwd `~/.claude`, credential-like env vars stripped when declared in `marketplace.json` or project settings; marketplace-level helpers from project settings run only after folder trust, and from server-managed settings only after the security approval dialog. Symlinks: kept if inside the plugin, dereferenced if elsewhere in the same marketplace, skipped if outside.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/host-marketplace.md (Stay within the download limits for hosted files; Share files within a marketplace with symlinks; Authenticate archive downloads; Write the headersHelper command; When Claude Code skips a headersHelper command or drops its output; How users accept a headersHelper command; Installs and updates that refuse a command instead of asking; When a marketplace url source's command runs)

### Mod trust model: unsandboxed, full user authority
- What: Docs state plainly that mods aren't sandboxed: they run with the user's permissions, can read/write any file, start processes, make network requests, read env vars and settings (including API keys), see every prompt and tool call, rewrite prompts/tool calls, submit prompts as the user, message other sessions, approve tool calls before the user is asked (subject to the guard), and spend the user's model quota. The Bash sandbox does not contain processes a mod starts. Mitigations offered: install from trusted marketplaces, review `claude plugin validate` `hooks:`/`calls:` output before installing, workspace trust prompt gates loading, `--safe-mode`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/overview.md (Decide whether to trust a mod; What a mod can reach; List what a mod does before you install one); https://code.claude.com/docs/en/plugins/mods/admin.md (Review what a mod can do; Know which controls still apply)
- Notes: Static analysis is the enforcement basis for review: Claude Code refuses to load a mod whose `$` usage the scanner can't read, so `calls:` is a complete list of API surfaces used (not of arguments). `validate` fails names that look like Anthropic's own (e.g. starting `claude-`).

### Channel sender gating and prompt-injection guidance
- What: Docs call an ungated channel a prompt-injection vector and require implementers to check sender identity (not chat/room id, which would admit everyone in a group) against an allowlist before emitting. Official Telegram/Discord bootstrap the allowlist via pairing code approved in the session, then `/x:access policy allowlist`; iMessage auto-admits the user's own handles and adds others with `/imessage:access allow`. The allowlist also gates permission relay, so anyone allowlisted can approve tool use.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/channels-reference.md (Gate inbound messages); https://code.claude.com/docs/en/channels.md (Security)
- Notes: Example webhook server binds 127.0.0.1 only. Docs suggest `--dangerously-skip-permissions` for unattended channel use "only in trusted environments".

### Artifact hosting, CSP and data handling
- What: Content stored on Anthropic infrastructure, visible only to authenticated org members and people shared with unless public. Viewer loads pages from sandboxed `*.claudeusercontent.com` origin under strict CSP: scripts only from cdnjs, unpkg, cdn.tailwindcss.com, code.jquery.com, selected jsDelivr paths; fonts only Google Fonts; no external images/styles; fetch/XHR/WebSocket only to own origin and Google Fonts; page-initiated downloads blocked (need declared downloads capability). Single page, no backend, no relative links, 16 MiB rendered limit; source must be `.html/.htm/.md`, valid UTF-8 (or BOM UTF-16LE), not on a network-share path.
- Status: GA
- Evidence: https://code.claude.com/docs/en/artifacts.md (Page constraints; Allowlist the viewer domain; Manage artifacts for your organization)
- Notes: Reading someone else's artifact goes through a WebFetch-like summarizer that reports embedded instructions rather than relaying them, with the raw source saved locally. Extra approval prompts for: another org's public artifact (even in auto mode; impossible in bypass), unconfirmed owner, cloud sessions without network, plan mode. `WebFetch(domain:claude.ai)` rules apply to artifact reads. Publish prompts trigger in plan mode, for runtime capabilities, for public artifacts, for scheduled-task turns, and for unreadable files.

### Subagent output scanning and inter-agent authority rules
- What: Each subagent's final report is scanned before the parent reads it. A backslash is inserted into text that imitates harness output (`<system-reminder>`, lines starting `Human:`/`Assistant:`). A `[harness: subagent output matched instruction-shaped pattern(s): ...` marker line is prepended when the report imitates tags or mentions `bypassPermissions` / `--dangerously-skip-permissions`. Nothing is removed. Reports arrive under a header saying the instructions in them carry no user authority. No message from any agent counts as user approval or can change permissions, CLAUDE.md or config. In auto mode the classifier treats relayed approval claims as untrusted and reviews each inter-agent message before delivery; a blocked message is never delivered.
- Status: GA
- Evidence: https://code.claude.com/docs/en/sub-agents.md (Subagent output scanning; Resume subagents); https://code.claude.com/docs/en/agent-teams.md (Messages between agents)
- Notes: The docs say plainly that the scan does not judge malice and does not replace tool restriction.

### Routine prompt trust model and fire-payload wrapping
- What: The saved prompt is treated as the run's assignment, attested only as stored earlier by an authorised session. It is not live user input and cannot serve as consent for actions during the run. `/fire` `text` (and Run-now text) is wrapped in `<routine-fire-payload>` and labelled untrusted. The routine prompt has to opt in explicitly to act on it, so text from a leaked token arrives as data. Network: the Default environment uses "Trusted" allowlist egress; other hosts get 403 with `x-deny-reason: host_not_allowed`. Connector traffic goes through Anthropic's servers instead of that path. Env vars are visible to anyone using the environment, so the docs recommend network secrets.
- Status: research preview
- Evidence: https://code.claude.com/docs/en/routines.md (Create from the web, step 2 and step 5; Trigger a scheduled task; Environments and network access)

### Trusted Devices for Remote Control
- What: Optional requirement that viewing or steering an RC session needs an enrolled device credential plus a sign-in no older than 18 hours; refresh via Face ID/Touch ID/Windows Hello/passkey step-up. Enrollment only offered shortly after a full sign-in. Only public key and metadata stored. The CLI host gets its credential automatically at sign-in. Devices listed and revocable in account settings; admins can "Sign out everywhere". Not retroactive to running sessions.
- Status: beta
- Evidence: https://code.claude.com/docs/en/remote-control.md (Trusted Devices; What members see; Manage enrolled devices)
- Notes: Off by default; Team/Enterprise Owner toggle at Capabilities > Remote sessions; Pro/Max/Ultra self-enable. Also covers Cowork Computer access. With Trusted Devices required, `DISABLE_TELEMETRY`/`DO_NOT_TRACK` make RC unavailable.

### Network secrets (credential injection by egress proxy)
- What: Store an API key on an environment; Anthropic's agent proxy attaches it to requests for listed hosts after they leave the VM, so the key is never in the VM's env or files. Types: Bearer with custom headers (name/prefix/value), and others shared with Claude Tag connections; non-admin Team/Enterprise members get a one-host, no-wildcard form (Token or Username and password). Values can't be viewed after saving; change = delete and re-add. Listed hosts become reachable even if the access level wouldn't allow them. Never attached to GitHub, `api.anthropic.com`, public registries (npm, jsr, PyPI, crates, Go proxy), setup-script requests (proxy connects after Claude Code launches), or telemetry export. Overlapping-but-unequal host patterns: only one secret sent, no marker.
- Status: GA
- Evidence: https://code.claude.com/docs/en/cloud-environments.md (Add network secrets; Which requests get the secret; Requests that never get the secret)
- Notes: Not available in self-hosted environments or with CMEK. Target API must accept connections from Anthropic's network.

### One-owner-per-runner lock
- What: The first session a runner claims locks it to that session's owner (user account, or the Claude Tag agent identity for channel sessions); it then serves only that owner up to `--capacity`. At default `--drain-grace-sec 0` the runner exits when its sessions finish, so the orchestrator restarts it with a fresh disk for any owner. Minimum fleet size equals concurrent active owners. `--lock-to-account` pre-locks (never serves Claude Tag).
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments.md (Key concepts; Runner lifecycle); https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Scale the fleet); https://code.claude.com/docs/en/self-hosted-environments-reference.md (Runner CLI flags)
- Notes: Isolation of checked-out code between owners without wiping disk. Claude Tag caveat: one agent owner covers many Slack users, so capacity>1 mixes people.

### Self-hosted hardening checklist
- What: Ephemeral per-session container or VM at `--capacity 1` and drain grace 0; no broad credentials in the image; keep host GitHub creds unreadable; keep the environment secret off session-running hosts (prefer orchestrator); default-deny egress at your boundary (Bash is pre-approved, so shell egress needs no prompt); least-privilege host IAM; block cloud metadata endpoint inside the container (IMDSv2 hop limit 1, GKE metadata concealment, deny 169.254.169.254); read-only hooks dir, wrapper and host `~/.claude/`; note that any org member can dispatch to any environment (no per-environment ACL) and Claude Tag can admit anyone in the Slack workspace.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Harden your deployment; Default-deny egress)
- Notes: Docs say plainly the product "can't verify or enforce" egress restriction. Session children share the runner's UID; daemonized leftovers are not signalled on session stop.

### Repo-settings confine guard
- What: `--confine-repo-settings warn|enforce|off` (default warn) scans each repo's committed settings for grants that resolve outside the session workspace (`additionalDirectories`, bare `Edit`/`Write`/`NotebookEdit` allow rules, `sandbox.filesystem.allowWrite/allowRead`), a non-empty `env` block, and posture overrides (`sandbox.enabled: false`, `disableAllHooks`). Enforce refuses the session. Doesn't cover repo hooks, `.mcp.json` or Bash rules. `--trust-workspace false` drops repo-committed permission grants entirely.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Harden your deployment); https://code.claude.com/docs/en/self-hosted-environments-reference.md (Runner CLI flags); https://code.claude.com/docs/en/self-hosted-environments-configuration.md (Repository-committed permission rules)
- Notes: Recommended `enforce` for Bedrock/Vertex runners.

### Environment secret, runner tokens, work orders, rotation
- What: Environment secret ("environment key") shown once at creation, expires after 365 days, used by runners to register and receive a runner token. Rotation: create new on Configuration tab, roll out, revoke old; runners on a revoked secret fail the next poll (`poll auth failed`) and exit. Suspected compromise: first "Revoke token" on the suspect session in the Activity tab so no runner can resume it, then replace the secret. Orchestrator mode keeps the secret on the orchestrator host only; each spawned runner gets a single-use signed work-order JWT that registers exactly one runner and expires with the spawn lease.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-quickstart.md (Create an environment); https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Harden your deployment); https://code.claude.com/docs/en/self-hosted-environments-configuration.md (On-demand runners)
- Notes: All these token kinds (environment secret, runner token, work order, session token) are signed by the same JWKS and distinguished by `ccr:role`.

### Session identity JWT for internal services
- What: Each self-hosted session gets `CLAUDE_CODE_SESSION_ACCESS_TOKEN` = `sk-ant-cc-` + ES256 JWT. Public JWKS at `https://api.anthropic.com/v1/code/.well-known/jwks.json` (cache 300 s, keys rotate, refetch once on unknown `kid`). Verify: prefix, ES256 signature, `iss == ccr`, `aud` contains your `ccpool_` ID, `ccr:role == session_worker`, `exp`. Lifetime 4 h default, 8 h max, refreshed and pushed to the session. Identity in RFC 8693 `act` chain: creator (`user:<id>` or `agent:<id>`, optional email) -> runner (`ccr:runner:`) -> pool (`ccr:pool:`) -> identity that created the secret. Hosted-session tokens use `sk-ant-si-` and a different key set.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-identity.md (all sections)
- Notes: Docs state explicitly what it does not prove (which process presents it), that there is no revocation feed, and that derived credentials must be scoped to one session's needs and bounded by `exp`; audit on `ccr:session_id` and `jti`. `decode-token` subcommand verifies signature and expiry only, not `iss`/`aud`/role.

### Git config hardening inside lifecycle hooks
- What: Because hooks hold the session token and their git reads session-writable config (`~/.gitconfig`, `.git/config`), the runner injects `GIT_CONFIG_COUNT/KEY/VALUE` overrides that outrank config files: `core.hooksPath=/dev/null`, empty `core.fsmonitor`, `GIT_ALLOW_PROTOCOL=https:http:ssh`, ignore `core.sshCommand`/`core.askPass` from files, runner-set gpg program paths, signing off unless `--configure-git`. Credential helpers, `insteadOf` rewrites and filter drivers in session-writable files still apply; docs tell operators to mint push credentials that can do no more than the one push.
- Status: beta
- Evidence: https://code.claude.com/docs/en/self-hosted-environments-configuration.md (Git configuration inside lifecycle hooks; post-session example)
- Notes: v2.1.280+. Startup logs a `[runner:git] lifecycle hooks:` line showing what is in effect.

### Data handling across modes
- What: RC: transcript stored on Anthropic servers while connected. Cloud: GitHub credentials encrypted server-side, never in VM. Self-hosted: checkouts, artifacts, secrets and files stay on your machines, but prompts, responses and tool results go to `api.anthropic.com` and Anthropic stores the transcript (even when inference goes to Bedrock/Vertex). Telemetry from runner children: operational only, no code; operator's telemetry env vars re-asserted over server-provided ones; Datadog opt-in via `CLAUDE_CODE_BYOC_ENABLE_DATADOG=1`.
- Status: GA / beta
- Evidence: https://code.claude.com/docs/en/remote-control.md (Connection and security); https://code.claude.com/docs/en/claude-code-on-the-web.md (GitHub authentication options); https://code.claude.com/docs/en/self-hosted-environments.md (What stays on your infrastructure); https://code.claude.com/docs/en/self-hosted-environments-reference.md (Telemetry)
- Notes: Self-hosted is execution locality, not data locality.

### GitHub Action trigger checks (who can run Claude)
- What: Two checks run before Claude starts. (1) Write access: on issue and PR events the triggering user must have write access. `allowed_non_write_users` relaxes this but requires your own `github_token`. Events with no author, such as `schedule`, skip it. (2) Human actor: on every event, bot actors are rejected unless listed in `allowed_bots`, which prevents loops. Scheduled runs count as the user who last changed the cron, so a bot there must be allow-listed.
  Known gap: on public repos with cloud-provider OIDC, the credential steps run before the Action's write-access check. Any commenter's trigger phrase therefore mints an App token and signs in to the cloud, leaving audit entries and using minutes. The docs advise adding your own write-access check step first.
  On public repos, GitHub withholds secrets from fork PR runs, so the review example works only for same-repo branches.
- Status: GA
- Evidence: https://code.claude.com/docs/en/github-actions.md (Who can trigger runs; Run a skill); https://code.claude.com/docs/en/github-actions-cloud-providers.md (Create the workflow file, warning)
- Notes: The Action's own security doc (github.com/anthropics/claude-code-action/docs/security.md) is linked but was not opened.

### GitHub Action authentication options
- What: There are four ways to authenticate:
  - `ANTHROPIC_API_KEY` secret.
  - `CLAUDE_CODE_OAUTH_TOKEN`, a long-lived subscription token from `claude setup-token` (Pro, Max, Ultra, Team, Enterprise). It is tied to the person who generated it, so an API key is recommended for org-shared secrets.
  - Workload identity federation: the Action exchanges GitHub's OIDC token for Claude API access through a Claude Console service account. Inputs: `anthropic_federation_rule_id` (`fdrl_...`), `anthropic_organization_id`, optional `anthropic_service_account_id` (`svac_...`), optional `anthropic_workspace_id`. Needs `id-token: write`.
  - Cloud providers through OIDC (next entry).
  For GitHub itself, the Action acts as the Claude GitHub App by default. A `GITHUB_TOKEN` can be passed instead, but commits made with it do not trigger CI.
  `/install-github-app` (github.com only; needs the `gh` CLI) installs the app, stores the secret, pushes a branch with the workflows, and opens a PR. Esc stops after the step in progress and lists what was done. Deleting a secret does not revoke the key itself.
- Status: GA
- Evidence: https://code.claude.com/docs/en/github-actions.md (Quick setup; Manual setup; Set up for an organization; Uninstall; CI not running on Claude's commits)

### Claude Tag agent identity, Agent Proxy and default-deny egress
- What: In channels, Claude acts under its own service accounts (the Slack app, the Claude GitHub App, and per-service connector credentials), not as the person who asked. Credentials sit in a separate credential store and never enter the sandbox. Agent Proxy, at the network boundary, injects the credential when an outbound request matches a rule, and scoping can go down to host, path prefix, or read-only methods. Setup screens are write-only. Egress from a hosted sandbox is default-deny except GitHub. Allowed are a credential's allowed websites, the channel's allowed domains, and the environment's network level ("Limited access" by default, covering package registries). A `*` entry allows any host without credentials, but private and internal addresses and cloud metadata stay blocked. GitHub API and git traffic go through a separate GitHub proxy limited to repositories granted to the channel, though public repos can still be cloned without credentials. Access is scoped to all of Slack, a workspace, a channel, or channel-name pattern rules. Bundles, skills, plugins, and instructions are locked at thread start, while connectors and domain rules are enforced on every request.
- Status: public beta
- Evidence: https://claude.com/docs/claude-tag/concepts/security-and-data.md (How a request travels; Credential storage; Network egress; Service accounts; Isolate credentials between channels); https://claude.com/docs/claude-tag/concepts/how-it-works.md (Channel access)
- Notes: Workspace notes saved from public channels are read in every channel, so credential isolation does not isolate knowledge. A Slack Connect channel gets only bundles attached directly to it, and its connector credentials can be used for the other company's people too. Self-hosted environments cannot use the admin's connectors.

### Telemetry content redaction defaults and opt-ins
- What: By default no prompt text, response text, tool arguments, tool output, file contents or raw API bodies are exported; only lengths and categories. Opt-ins: `OTEL_LOG_USER_PROMPTS` (prompt on `user_prompt` as `prompt` and `prompt_text`, span `user_prompt`), `OTEL_LOG_ASSISTANT_RESPONSES` (falls back to the prompts flag when unset; set 0 to keep responses redacted), `OTEL_LOG_TOOL_DETAILS` (Bash commands untruncated, MCP server/tool names, skill names, tool_input truncated to 512 chars per value and ~4K total, real agent/skill/plugin/MCP names on counters, error messages, file paths), `OTEL_LOG_TOOL_CONTENT` (tool output span events; tracing required), `OTEL_LOG_RAW_API_BODIES` (`1` inline truncated, or `file:<dir>` untruncated files plus `index.jsonl`; implies all other content consent; thinking always redacted), `OTEL_LOG_MANAGED_SETTINGS` (schema-shaped settings with string values redacted plus SHA-256 digest; ignored in project/local settings). Content limit `CLAUDE_CODE_OTEL_CONTENT_MAX_LENGTH` default 61440 UTF-16 units with `_truncated`/`_original_length` markers. Default name redaction: user-defined agents to `custom`, third-party plugin names to `third-party`, user MCP tools to `mcp_tool`, user workflow names to `custom`. `user.email` is always included when signed in (sent only to the configured endpoint).
- Status: GA
- Evidence: https://code.claude.com/docs/en/monitoring-usage.md (Common configuration variables; Security and privacy; Cost counter; Audit MCP activity; Managed settings resolved event)
- Notes: Exception: Claude Desktop built-in server names emit with the flag off (host-authored). Raw-body and detailed-tracing vars are ignored in project/local settings so a cloned repo cannot turn on content capture. Digest is opt-in because a short policy can be recovered by hashing guesses.

### Telemetry in cloud sessions
- What: Cloud and Claude Tag sessions run on Anthropic-hosted environments, so device files do not configure them; use server-managed `env` (not reaching Claude Tag) or the cloud environment's variables (readable by any environment user, so no credentials; network secrets are not applied to telemetry export). Collector domain must be on the environment's network allowlist. Filter on `ccr.session.id`; `app.entrypoint=claude-in-slack` identifies Claude Tag; Tag sessions use a shared org identity.
- Status: GA
- Evidence: https://code.claude.com/docs/en/monitoring-usage.md (Telemetry from cloud sessions and Claude Tag; Attribute telemetry to cloud sessions)
- Notes: none

### Telemetry and nonessential traffic opt-outs
- What: Operational telemetry goes to Anthropic by default on Anthropic API sessions (direct, gateway or custom base URL); off by default on Bedrock, Vertex, Foundry, Claude Platform on AWS. Opt-outs: `DISABLE_TELEMETRY` (events exclude code, paths, commands per page), `DO_NOT_TRACK` (standard boolean), `DISABLE_ERROR_REPORTING`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` (also auto-updates, `/feedback`, release notes, PR badge checks, availability checks, background plugin command re-runs), `DISABLE_GROWTHBOOK` (feature flags only). Telemetry, DNT and nonessential-traffic switches also disable feature-flag fetching, which removes Remote Control, cross-machine messaging, `/import`, synced skills/plugins, advisor, MCP tasks, artifact comments and more. Org-wide: put `DISABLE_TELEMETRY: "1"` in managed `env` (applies without approval dialog); this also stops the org analytics dashboard feed. `feedbackSurveyRate` and `feedbackDrafts` control surveys and Claude-drafted feedback; `skipWebFetchPreflight` stops sending each WebFetch hostname to api.anthropic.com.
- Status: GA
- Evidence: https://code.claude.com/docs/en/managed-settings.md (Turn telemetry off for your organization); https://code.claude.com/docs/en/env-vars.md (Variables; Features that need feature-flag fetching); https://code.claude.com/docs/en/settings-reference.md (Privacy and telemetry); https://code.claude.com/docs/en/feature-availability.md (Summary by provider)
- Notes: Official marketplace auto-install is not covered by the nonessential switch (`CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL`). A first session after install or upgrade may lack flag-gated features until flags are cached.

### Server-managed settings is a client-side control
- What: The docs state server-managed settings are not a security boundary: users can edit the cache (applies until next fetch), delete it, run old or modified binaries, use another org, set a third-party provider or custom base URL (which bypasses delivery), or intercept TLS. Endpoint-managed settings on MDM devices are recommended for stronger guarantees, plus network Tenant Restrictions.
- Status: GA
- Evidence: https://code.claude.com/docs/en/server-managed-settings.md (Security considerations); https://code.claude.com/docs/en/managed-settings.md (What a developer can change)
- Notes: Managed settings bind only Claude Code, not other API clients; local admins can edit endpoint sources too.

### Withholding cached network and credential env values
- What: From the server-managed cache, Claude Code withholds proxy/TLS vars, API routing and provider selection, auth credentials, `CLAUDE_CONFIG_DIR`, federation/profile selectors and OS directory vars (`HOME`, `XDG_CONFIG_HOME`, `APPDATA`, `USERPROFILE`) until a live fetch confirms the payload, so a tampered cache cannot redirect or re-authenticate the very fetch that would correct it. Endpoint-managed sources are not affected and can supply a proxy for the first fetch.
- Status: GA
- Evidence: https://code.claude.com/docs/en/server-managed-settings.md (Fetch and caching behavior)
- Notes: Extended to selectors in v2.1.223. Telemetry exporter variables are treated as one unit per source so an endpoint from one source never pairs with credentials from another.

### Subprocess credential scrub
- What: `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1` removes credentials (recognised by name or value shape) from Bash, hook and stdio MCP environments, strips user:password from package index URLs, keeps GitHub tokens and proxy vars; on Linux it also runs Bash in an isolated PID namespace so `/proc` of host processes is unreadable (side effect: `ps`/`kill` cannot see host processes). Set automatically by `claude-code-action` when non-write users are allowed.
- Status: GA
- Evidence: https://code.claude.com/docs/en/env-vars.md (What the subprocess environment scrub removes)
- Notes: Documented as one layer, not a sole control. Expanded in v2.1.251.

### Local data retention controls
- What: `cleanupPeriodDays` (default 30, min 1; 0 fails validation) runs a silent background sweep deleting transcripts and other app data; `desktopSessionCleanupPeriodDays` for Desktop/Cowork transcripts (default no limit). `CLAUDE_CODE_SKIP_PROMPT_HISTORY=1` writes no history or transcripts. `CLAUDE_CODE_TRANSCRIPT_LOCAL_GC=1` trims long `-p`/SDK transcripts after compaction once over 5 MB. `IS_DEMO` hides email and org name.
- Status: GA
- Evidence: https://code.claude.com/docs/en/settings-reference.md (cleanupPeriodDays; desktopSessionCleanupPeriodDays); https://code.claude.com/docs/en/env-vars.md (Variables)
- Notes: none.

### Release signing and binary integrity
- What: Each release publishes a `manifest.json` with SHA-256 per platform binary, signed with an Anthropic GPG key (fingerprint published, signatures from v2.1.89); macOS binaries are signed and notarized, Windows Authenticode-signed, Linux binaries unsigned individually (verify via manifest or signed apt/dnf/apk repos). Updates fail with `Release verification failed` when checks fail.
- Status: GA
- Evidence: https://code.claude.com/docs/en/setup.md (Binary integrity and code signing); https://code.claude.com/docs/en/troubleshooting.md (symptom table)
- Notes: none.

### Gateway identity: OIDC device flow, sessions, revocation
- What: CLI-to-gateway uses RFC 8628 device grant; gateway-to-IdP uses authorization code with PKCE S256 by default and `response_mode=query`. Any OIDC IdP (Okta, Entra, Google Workspace, Keycloak, Dex, PingFederate); SAML/LDAP unsupported; one issuer per gateway. `allowed_email_domains`, `allowed_groups`, `groups_claim` (JSON Pointer supported), `email_claim`, `google_groups` (Directory API lookup via domain-wide delegation), `userinfo_fallback`, `scope_on_refresh`, `private_key_jwt` certificate client auth (v2.1.284+), `ca_cert_pem`, `use_proxy`. `email_verified: false` always rejected. Gateway mints HS256 JWTs (`session.jwt_secret`, array for rotation; `ttl_hours` default 1); CLI refreshes silently; IdP deprovision takes effect within TTL when refresh fails.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-config.md (oidc; Certificate client authentication; session); https://code.claude.com/docs/en/claude-apps-gateway-deploy.md (Identity provider setup; Threat model summary; JWT secret rotation)
- Notes: No per-session revocation: tokens validate locally, so force-logout means rotating the JWT secret (all sessions). The built-in gateway advertises no `revocation_endpoint`, so `/logout` is client-local only. No service-token flow for CI; CI must talk to the provider directly. Device flow works over SSH (approve on laptop). Page states a sign-in token identifies the approving developer, not the program; `User-Agent` must not be trusted as proof of Claude Code.

### Gateway trust: private-address rule and TLS pinning
- What: At `/login`, Claude Code requires every address of the gateway host (and any applicable HTTPS proxy host) to be private (RFC 1918, link-local, CGNAT, ULA, loopback), because a trusted gateway can push settings that run commands on developer machines. Orgs on owned public IPv4 space declare up to four non-overlapping blocks in `gatewayInternalNetworks` (v2.1.268+), with checks that the gateway resolves only inside the block, the client connects from inside it, and the connection is direct. CLI pins the gateway's TLS leaf fingerprint per hostname on first connect (trust prompt shows first 16 hex chars), rechecks on sign-in, refresh and settings fetches (not inference); approval memory for sensitive settings is keyed to the pinned cert. `http://` accepted only for loopback.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway.md (Prerequisites; Connect developers; Allow a gateway on public address space you own); https://code.claude.com/docs/en/claude-apps-gateway-deploy.md (Choose an address for the gateway; Troubleshooting)
- Notes: An invalid `gatewayInternalNetworks` value blocks every new gateway sign-in on the machine (fail-closed). Requests via an HTTPS proxy skip the pin check. Documentation ranges refused.

### Security approval dialog for network-delivered settings
- What: Because gateway (and server-managed) settings arrive over the network, interactive clients show an approval dialog before applying `hooks`, approval-requiring `env` vars (proxy, base-URL, `OTEL_EXPORTER_OTLP_ENDPOINT`), shell-execution settings (`apiKeyHelper`, `statusLine`), sandbox binary paths, and sandbox settings that intercept traffic or weaken isolation. Declining exits the session. Non-interactive runs (`claude -p`, SDK, Desktop Code tab) apply for that run without recording approval.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-config.md (What goes in cli)
- Notes: Page frames the dialog as protecting the developer from a hostile gateway, not the org from the developer.

### Gateway server-side hardening
- What: SSRF guard on IdP, OTLP and `provider: anthropic` requests (resolve DNS, block link-local/metadata/loopback, pin resolved IP; RFC 1918 allowed); other providers have `base_url` checked at config load. `CLAUDE_GATEWAY_ALLOW_LOOPBACK=1` relaxes loopback; `CLAUDE_GATEWAY_PROXY_IS_EGRESS_BOUNDARY=1` (env-only so config cannot relax it, v2.1.277+) moves the check to a forward proxy. `access_control.allow_cidrs/deny_cidrs` (deny first; empty means serve anyone, with boot and first-public-client warnings and an `access.public_client` audit event). `public_url` required and never derived from `X-Forwarded-*`; `trusted_proxies` governs XFF. Device page enforces same-origin POST, CSP `form-action`, per-IP rate limits (30/600s start, 10/600s verify); user code is 8 chars from a 20-char alphabet, 10-minute expiry. Request body 32 MiB, header 256 KiB caps.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-deploy.md (Threat model summary; User-code brute-force resistance); https://code.claude.com/docs/en/claude-apps-gateway-config.md (listen; HTTP tuning; Proxy-only egress)
- Notes: Out of scope per docs: compromised gateway host (equivalent to MDM control) and malicious IdP. Gateway does not refuse plain HTTP itself. Bypass (developer with own credential) is a network-policy problem, not enforced by the gateway.

### Gateway data flow and compliance posture
- What: Gateway data plane sends nothing to Anthropic unless the Anthropic API is an upstream; telemetry, audit, identity and settings never go to Anthropic. Since v2.1.227 the gateway host process sends no startup telemetry or `/api/hello` HEAD. Client CLI disables analytics, error reporting and survey uploads while signed in to a gateway, but sends startup events before first sign-in (deliver `DISABLE_TELEMETRY` client-side) and WebFetch hostname checks and version checks unless `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` and `skipWebFetchPreflight: true`. Plugin marketplace registration on first start fetches from `downloads.claude.ai`/GitHub unless blocked via marketplace lists or `CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-deploy.md (Data flow; Compliance posture; Plugin marketplace requests); https://code.claude.com/docs/en/claude-apps-gateway.md (note under Why Claude apps gateway)
- Notes: Gateway sessions are not eligible for the HIPAA configuration (admin-setup, Review data handling).

### Data handling summary for admins
- What: No training on code/prompts on Team, Enterprise, API and cloud-provider plans; ZDR for qualified Enterprise accounts; HIPAA config for HIPAA-enabled Enterprise orgs turns some local features off. For request-level audit logging or sensitivity routing, docs recommend a gateway (Claude apps gateway or another LLM gateway). Telemetry opt-out via managed `env` `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/admin-setup.md (Review data handling)

### Gateway data retention and DSAR
- What: Postgres tables: `kv` (device grants 10-minute TTL, rate-limit counters, swept every 30 s), `spend` (13 months default), `spend_limits`, `admin_audit` (365 days), `principal_emails` (last-seen email, name, groups; PII; 90 days since last activity). Hourly sweep. Immediate erasure is a manual `DELETE FROM principal_emails WHERE principal = '<sub>'`; other tables reference pseudonymous `sub` only. Without spend limits only `kv` is written.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-deploy.md (Postgres); https://code.claude.com/docs/en/claude-apps-gateway-spend-limits.md (Data lifecycle)

### Secure deployment guidance
- What: Threat model is prompt injection plus model error; defense in depth. Built-ins listed: permission rules, critical-path `rm` gate, web search summarization (raw results not passed to context), Bash sandbox. Isolation options compared: sandbox-runtime (bubblewrap/Seatbelt, own network namespace, built-in allowlist proxy; shares host kernel; no TLS inspection), hardened Docker (`--cap-drop ALL`, `no-new-privileges`, seccomp, `--read-only`, tmpfs, `--network none`, memory/pids limits, non-root, read-only workspace, proxy via mounted Unix socket), gVisor (I/O-heavy work up to 10-200x slower), Firecracker microVMs with vsock-only networking. Cloud: private subnet, egress only to a proxy (e.g. Envoy `credential_injector`), minimal IAM, log at the proxy.
- Status: GA (guidance)
- Evidence: https://code.claude.com/docs/en/agent-sdk/secure-deployment.md (Threat model; Built-in security features; Isolation technologies; Cloud deployments; Filesystem configuration)
- Notes: Lists credential files to strip before mounting code read-only (`.env`, `~/.git-credentials`, `~/.aws/credentials`, gcloud ADC, `~/.kube/config`, `.npmrc`, `*.pem`...). Suggests overlay filesystems to review writes before persisting.

### Credential proxy pattern
- What: Keep credentials outside the agent boundary and inject them in a proxy. For model traffic, `ANTHROPIC_BASE_URL` points the CLI at a plaintext-HTTP proxy that adds the key; `HTTP_PROXY`/`HTTPS_PROXY` route all traffic but HTTPS is an opaque CONNECT tunnel unless a TLS-terminating proxy with a trusted CA is used. For other services, prefer a custom tool/MCP server that forwards to an outside service which adds auth (no TLS interception needed). Notes that not all programs honor proxy env vars (Node `fetch` needs `NODE_USE_ENV_PROXY=1` on Node 24+; else proxychains or iptables transparent proxy).
- Status: GA (guidance)
- Evidence: https://code.claude.com/docs/en/agent-sdk/secure-deployment.md (Credential management; Configuring Claude Code to use a proxy; Credentials for other services; Traffic forwarding); https://code.claude.com/docs/en/agent-sdk/hosting.md (Auth and secrets)
- Notes: Named proxies: Envoy, mitmproxy, Squid, LiteLLM. Warns that within an allowlist the agent can still send data to any allowed domain.

### Subagent output scanning before the parent reads it
- What: From v2.1.210 the final subagent message is scanned for instruction-shaped patterns: harness-only control tags such as `<system-reminder>` are neutralized in place (a backslash inserted after `<`), lines starting `Human:`/`Assistant:` get a backslash before the colon, permission-config mentions (`bypassPermissions`, `.claude/settings.json`) are kept as written; control-tag or permission matches get a prepended `[harness: ...]` marker line. Nothing is deleted or reworded. API errors that end a subagent early are never delivered as its result.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/subagents.md (What subagents inherit, note on output scanning)
- Notes: A concrete, cheap anti-prompt-injection measure at an agent-to-agent trust boundary.

### Authentication and claude.ai login restriction
- What: SDK reads `ANTHROPIC_API_KEY` from the process env (no `.env` loading), or cloud provider credentials. Third-party developers may not offer claude.ai login or rate limits in their products unless approved. `CLAUDE_CODE_OAUTH_TOKEN` from `claude setup-token` is shown for running under one's own subscription (model requests only, no connectors).
- Status: GA (policy)
- Evidence: https://code.claude.com/docs/en/agent-sdk/overview.md (Get started note); https://code.claude.com/docs/en/agent-sdk/quickstart.md (Set your API key); https://code.claude.com/docs/en/agent-sdk/session-storage.md (Dual-write architecture)
- Notes: `CLAUDE_AGENT_SDK_CLIENT_APP` identifies the host app in the User-Agent.

### Security hardening lines worth tracking
- What:
  - Secret redaction for GitLab token families (2.1.232).
  - Background task notifications state that no human input occurred, to stop fabricated in-transcript approvals (Week 28).
  - DNS-cache commands removed from auto-allow (2.1.90).
  - Docker daemon-redirect flags prompt (2.1.214).
  - `EnterWorktree` asks before entering a worktree outside `.claude/worktrees/` (Week 28).
  - Worktree isolation blocks Bash and git redirects into the main checkout (Week 32).
  - Skill names with hidden or bidi characters are refused (2.1.299).
  - Windows PATH lookup skips relative and UNC entries (2.1.321).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.90, 2.1.214, 2.1.232, 2.1.299, 2.1.321); https://code.claude.com/docs/en/whats-new/2026-w28.md, https://code.claude.com/docs/en/whats-new/2026-w32.md

## Area 22. Licence, pricing and distribution

### Installation and update channels
- What: Native installer (`curl -fsSL https://claude.ai/install.sh | bash`, PowerShell `irm .../install.ps1 | iex`, CMD script) auto-updates in the background; Homebrew casks `claude-code` (stable, about a week behind, skips releases with major regressions) and `claude-code@latest`; WinGet `Anthropic.ClaudeCode`; apt/dnf/apk. Homebrew and WinGet do not auto-update. `claude install [version|stable|latest]`, `claude update`, `/restart` applies an installed update (v2.1.288+, Pro/Max/Ultra native installs on macOS/Linux). Requires a Claude subscription (Pro, Max, Ultra, Team, Enterprise), Console account (auto-creates a "Claude Code" workspace for cost tracking), third-party cloud provider, or a self-hosted apps gateway with SSO.
- Status: GA
- Evidence: https://code.claude.com/docs/en/overview.md (Get started: Terminal tab); https://code.claude.com/docs/en/quickstart.md (Before you begin; Step 2); https://code.claude.com/docs/en/cli-reference.md (claude install, claude update); https://code.claude.com/docs/en/commands.md (/restart)
- Notes: Git for Windows recommended on native Windows for the Bash tool; otherwise PowerShell is the shell tool.

### Terms for embedding Claude Code and credential-use restrictions
- What: Consumer vs Commercial terms apply by plan; existing 1P/3P commercial agreements cover Claude Code. Embedding Claude Code in a product (hosted sandboxes, agent infrastructure) requires Commercial Terms, an unmodified binary with no built-in auth method removed or restricted, and each end user authenticating with their own API key, subscription or 3P credential (no reselling or intermediating usage). OAuth (claude.ai login) is only for subscription purchasers using Claude Code and native Anthropic apps; third-party developers, including Agent SDK products, must use Console API keys or cloud providers and may not offer claude.ai login, route requests through Free/Pro/Max/Ultra credentials, or collect/store/intermediate claude.ai credentials or session tokens. Pro/Max/Ultra usage limits assume ordinary individual use. Anthropic may enforce without notice.
- Status: GA (legal terms)
- Evidence: https://code.claude.com/docs/en/legal-and-compliance.md (Commercial agreements; Can customers offer Claude Code in their products?; Acceptable use; Authentication and credential use)
- Notes: Directly constrains any third-party harness: it cannot use claude.ai subscription OAuth on users' behalf; it must use API keys or cloud provider credentials. Naming/logo restrictions also apply.

### Artifact and channel plan/provider gating (brief)
- What: Artifacts: Pro, Max, Ultra, Team, Enterprise with claude.ai login, Anthropic API only. Channels: claude.ai or Console auth, not Bedrock/Vertex/Foundry. Cross-machine messaging: claude.ai login via Remote Control. Agent view: no plan gate stated; each session consumes quota independently.
- Status: GA / research preview as per features
- Evidence: https://code.claude.com/docs/en/artifacts.md (Availability); https://code.claude.com/docs/en/channels.md (note at top); https://code.claude.com/docs/en/cross-session-messaging.md (Availability); https://code.claude.com/docs/en/agent-view.md (Limitations)
- Notes: Mods work on every provider (MO, Where mods run).

### Pricing and plan gating for cloud features
- What: Cloud sessions count toward plan usage limits with no separate VM charge and share rate limits with all other Claude usage. Self-hosted sessions consume org usage the same way. Plans: cloud on Pro/Max/Ultra/Team and Enterprise premium or Chat+Claude Code seats; RC on Pro through Enterprise; self-hosted Team and Enterprise only; Dispatch Pro/Max/Ultra only.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-code-on-the-web.md (intro; Limitations); https://code.claude.com/docs/en/web-quickstart.md (intro); https://code.claude.com/docs/en/self-hosted-environments.md (Availability and limitations); https://code.claude.com/docs/en/mobile.md (Limitations)
- Notes: vendor-claim on "no separate charge" (no numbers given).

### Code Review pricing and spend limits
- What: There is no flat price. Reviews are billed by tokens through usage credits, separately from plan usage, to the org rather than the requester, and on the Anthropic bill even for orgs using cloud providers for other features. Cost scales with level, diff size, surrounding complexity, and the number of candidates verified. Review count scales with the Review runs mode. A monthly Code Review spend limit can be set, alongside the org spend limit and the credit balance. A review is skipped when any of these runs out or when what is left is below the expected cost of one review. The PR then gets a "⚠️ Code review skipped" comment naming the cause, and sometimes a "Code review declined" check. The docs recommend a pilot with Once or Manual mode.
- Status: GA
- Evidence: https://code.claude.com/docs/en/code-review-costs.md (Pricing; How often reviews run; Limit your spend); https://code.claude.com/docs/en/code-review-troubleshooting.md (Review didn't run and the PR shows a budget message)

### CI cost controls (Actions and GitLab)
- What: Runs consume runner minutes plus API tokens, or the subscription when an OAuth token is used. Documented levers: `--max-turns` in `claude_args`, workflow or job `timeout`, concurrency limits, a short `CLAUDE.md` (read on every run), specific requests, and issue templates.
- Status: GA
- Evidence: https://code.claude.com/docs/en/github-actions.md (Manage costs); https://code.claude.com/docs/en/gitlab-ci-cd.md (CI costs)

### Fast mode and model pricing exposure
- What: `/model` picker shows prices only when talking to the Anthropic API directly or through a proxying gateway; display only. Fast mode pricing listed in the fast mode entry. Subscription fast mode bills only to usage credits, not plan limits. Usage credits allow continuing past plan limits; Pro/Max/Ultra set monthly spend limits in-CLI.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Setting your model); https://code.claude.com/docs/en/fast-mode.md (Understand the cost tradeoff; Requirements); https://code.claude.com/docs/en/costs.md (Add usage credits to your subscription)
- Notes: Before v2.1.206 some rows showed the wrong model's price.

### Install methods and system requirements
- What: Native installer (`curl ... install.sh | bash`, PowerShell `install.ps1`, CMD `install.cmd`) installs to `~/.local/share/claude/versions/` with a launcher symlink at `~/.local/bin/claude`; Homebrew casks `claude-code` (stable) and `claude-code@latest`; WinGet `Anthropic.ClaudeCode`; signed apt/dnf/apk repos with stable and latest channels; npm `@anthropic-ai/claude-code` (Node 22+, installs the same native binary through per-platform optional dependencies; the binary does not run Node). Installer accepts `stable`, `latest` or an exact version. Requirements: macOS 13+, Windows 10 1809+/Server 2019+, Ubuntu 20.04+, Debian 10+, Alpine 3.19+ (needs bash, curl, libgcc, libstdc++, system ripgrep), 4 GB RAM, x64 or ARM64. Native Windows has no sandboxing; WSL 2 does. Git for Windows provides Git Bash for the Bash tool, otherwise PowerShell is the shell tool.
- Status: GA
- Evidence: https://code.claude.com/docs/en/setup.md (System requirements; Install Claude Code; Set up on Windows; Install with Linux package managers; Install with npm)
- Notes: Network-storage guidance: a running session re-reads its executable, so deleting it (npm in-place upgrade on shared NFS) crashes sessions on other machines (`Bus error`); the native installer keeps the two newest versions plus in-use ones. A custom launcher disables version cleanup.

### Auto-update and release channels
- What: Native installs check at startup and periodically, download in the background, apply on next start. `autoUpdatesChannel`: `latest` (default) or `stable` (about one week old, skipping releases with major regressions); enforceable via managed settings. Switching to stable in `/config` offers a downgrade or pins `minimumVersion`. `DISABLE_AUTOUPDATER` stops background updates only; `DISABLE_UPDATES` also blocks `claude update`/`claude install`. Homebrew, WinGet, apt/dnf/apk do not auto-update (Homebrew/WinGet can opt in with `CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE=1`; channel is picked by cask or repo). A new model can require a newer version than stable serves.
- Status: GA
- Evidence: https://code.claude.com/docs/en/setup.md (Update Claude Code; Configure release channel; Pin a minimum version; Disable auto-updates); https://code.claude.com/docs/en/settings-reference.md (autoUpdatesChannel)
- Notes: Known issue documented: update notices can precede package-manager availability. `claude doctor` reports the last update attempt.

### Feature availability by subscription plan
- What: Claude Code requires Pro, Max, Ultra, Team, Enterprise or Console (not the free plan) or a third-party provider. Plan table: cloud sessions and cloud scheduled tasks on all paid plans (Enterprise needs premium or Chat+Code seat); Remote Control and Channels admin-enabled on Team/Enterprise; computer use and Dispatch only Pro/Max/Ultra; Code Review, analytics dashboard, server-managed settings, SSO only Team/Enterprise; SCIM, Compliance API, Analytics API only Enterprise; ZDR only for qualified Enterprise with separate enablement. Ultra is the renamed Max 20x, Max the renamed Max 5x; same features.
- Status: GA
- Evidence: https://code.claude.com/docs/en/feature-availability.md (Availability by subscription plan; Ultra plan); https://code.claude.com/docs/en/setup.md (Authenticate)
- Notes: HIPAA-configured Enterprise orgs lose some features (not enumerated on this page).

### Gateway cost
- What: No separate licence or per-seat fee; part of the `claude` binary; customers pay inference via their existing commitment plus compute.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-deploy.md (Deployment)

### Packaging, licence, versioning and branding
- What: npm `@anthropic-ai/claude-agent-sdk` (platform binaries as optional deps, e.g. `-darwin-arm64`, linux glibc and musl variants; `/core` entry for bundlers, v0.3.282+; `extractFromBunfs()` for `bun build --compile`) and PyPI `claude-agent-sdk`. SDK version tracks the bundled CLI (SDK v0.3.191 bundles CLI v2.1.191); semver, changelogs on GitHub. Governed by Anthropic Commercial Terms. Branding: may say "Claude Agent" or "{Name} Powered by Claude", may not say "Claude Code" or mimic its visuals.
- Status: GA
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript.md (Installation; Compile to a single executable; Import the /core entry); https://code.claude.com/docs/en/agent-sdk/overview.md (Changelog; Branding guidelines; License and terms); https://code.claude.com/docs/en/agent-sdk/hosting.md (Runtime dependencies)
- Notes: Docs do not state an open-source licence for the SDK packages here; only Commercial Terms "except" components with their own LICENSE.

### Rename from Claude Code SDK (migration)
- What: Packages renamed `@anthropic-ai/claude-code` -> `@anthropic-ai/claude-agent-sdk` and `claude-code-sdk` -> `claude-agent-sdk`; Python `ClaudeCodeOptions` -> `ClaudeAgentOptions`. Breaking in v0.1.0: the Claude Code system prompt is no longer the default (minimal prompt; opt in with the `claude_code` preset). The v0.1.0 change to load no filesystem settings by default was reverted: omitting `settingSources` loads user/project/local again.
- Status: GA (rename done)
- Evidence: https://code.claude.com/docs/en/agent-sdk/migration-guide.md (What's Changed; Breaking changes)
- Notes: Rationale given: broader than coding. Also links an OpenAI Agents SDK migration recipe.

### TypeScript V2 session API (removed)
- What: Experimental `unstable_v2_createSession` / `unstable_v2_resumeSession` / `unstable_v2_prompt` with an `SDKSession` (`send()`, `stream()`, `close()`, `await using`) replaced generator coordination with explicit send/stream cycles. Removed in TS SDK 0.3.142 (version numbers jumped from 0.2.x to 0.3.142); pin `@anthropic-ai/claude-agent-sdk@0.2` to keep it. It lacked fork support.
- Status: deprecated (removed)
- Evidence: https://code.claude.com/docs/en/agent-sdk/typescript-v2-preview.md (warning; Installation; API reference; Feature availability); https://code.claude.com/docs/en/agent-sdk/sessions.md (TypeScript: continue: true)
- Notes: The slice brief calls it a "V2 preview"; the page now documents it as removed. Migration: `query()` with an AsyncIterable prompt or `resume`.

Areas 3 (code editing), 4 (terminal) and 23: nothing SDK-specific documented in this slice beyond the built-in tools, sandbox and checkpointing entries above; their mechanisms live on CLI pages surveyed by other slices.

### Distribution changes
- What:
  - npm install deprecation notice (2.1.15, Jan 21, 2026). Native binaries (2.1.113, Week 16).
  - zstd-compressed download, about 75 MB instead of 340 MB on Linux x64, and on-demand code loading saving about 40–70 MB per session (2.1.243).
  - Six syntax languages removed to save 2.5 MB (2.1.251).
  - Auto-update streams to disk, cutting peak memory by about 400 MB (Week 28).
  - Plan naming now includes "Ultra" alongside Pro and Max (2.1.321).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.15, 2.1.113, 2.1.243, 2.1.251, 2.1.321); https://code.claude.com/docs/en/whats-new/2026-w16.md, https://code.claude.com/docs/en/whats-new/2026-w28.md, https://code.claude.com/docs/en/whats-new/2026-w35.md

## Area 23. Anything else or unusual

### Deep links (`claude-cli://open`)
- What: A custom URL scheme opens Claude Code in a new local terminal window with an optional `cwd` (absolute path; UNC, `..` and invisible/bidi characters rejected), or `repo=owner/name` (resolved to the clone or worktree where `claude` was most recently run; home dir if unknown; branch unchanged), and `q` pre-filled prompt (URL-encoded, max 5,000 chars). The prompt is never sent automatically; a `Prompt from an external link` warning stays below the input until sent or cleared, with a scroll-and-review warning for prompts over 1,000 chars. Normal permissions, CLAUDE.md and trust prompts apply. Handler registers on first sent message in an interactive session (or agent view in a trusted folder), at user-level locations only (macOS `~/Applications/...app`, Linux `.desktop` under `$XDG_DATA_HOME`, Windows HKCU). `disableDeepLinkRegistration: "disable"` (enforceable via managed settings).
- Status: GA
- Evidence: https://code.claude.com/docs/en/deep-links.md (How deep links work; What a launched session shows; Build a link; Registration and supported platforms)
- Notes: GitHub Markdown strips non-http schemes, so links render as plain labels there. VS Code has its own `vscode://anthropic.claude-code/open` handler; Desktop uses `claude://`.

### Prompt library (patterns, not features)
- What: A curated set of copy-paste prompts with stated patterns: describe the outcome, give a self-check, point at a reference file, state a measurable target, paste the artifact, specify answer format.
- Status: GA
- Evidence: https://code.claude.com/docs/en/prompt-library.md (What makes these prompts work)
- Notes: Documentation only; no mechanism.

### Artifact templates: Slides, Design, Docs
- What: Claude can start artifacts from claude.ai templates: Claude Slides (`/slides <brief>`), Claude Design (`/design <brief>`, artboards on a canvas, PNG/PDF export), Claude Docs (reached as the `claude.ai Claude Docs` connector; document requests route there instead of a page). Each opens in its own editor on claude.ai and exports to PowerPoint/PDF/Word.
- Status: beta
- Evidence: https://code.claude.com/docs/en/artifacts.md (Start from a Slides, Design, or Docs template)
- Notes: On by default for Pro/Max/Ultra/Team; Enterprise Owners enable per template. `/slides` and `/design` need v2.1.265+.

### Mod audio, theme backdrop and settings rewriting (unusual surfaces)
- What: `$.audio.play/speak`; `$.theme.set({backdrop})` lets one mod per session set a backdrop the Desktop/mobile app draws via a `Backdrop` render site (replaced `$.ui.backdrop`, removed v2.1.319); `settings.read` hooks can rewrite the merged session settings Claude Code itself uses (schema-checked per key).
- Status: GA
- Evidence: https://code.claude.com/docs/en/plugins/mods/reference.md (Mods API methods; Render sites); https://code.claude.com/docs/en/plugins/mods/interface.md (Put a backdrop behind the conversation); https://code.claude.com/docs/en/plugins/mods/events.md (Change or refuse a settings read)
- Notes: Any mod's `$.theme.clear()` removes any mod's backdrop.

### Remote Control vs other away-from-terminal options
- What: The RC page tabulates Dispatch (Desktop, from phone), Remote Control, Channels (push events from Telegram/Discord/own server into a CLI session), Slack (@Claude, Anthropic cloud), self-hosted environments, and scheduled tasks (CLI, Desktop or cloud), by trigger, where Claude runs, setup, best use. Cross-session messaging reuses the RC connection to carry messages between your sessions on different machines.
- Status: GA
- Evidence: https://code.claude.com/docs/en/remote-control.md (Remote Control vs cloud sessions; What connected devices see)
- Notes: Projects combine cloud threads with RC to run a thread on your own computer (not read in detail; other slice).

### Fallback-triggered safe-mode diagnosis
- What: `claude --safe-mode` disables customisations (CLAUDE.md, skills, MCP servers, hooks) to test whether workspace context trips the safety classifier; `plugin_loaded` and `hook_registered` events report `safe_mode`.
- Status: GA
- Evidence: https://code.claude.com/docs/en/model-config.md (Check what triggered fallback); https://code.claude.com/docs/en/monitoring-usage.md (Plugin loaded event; Hook registered event)
- Notes: Git status and directory names are still sent.

### Login notice
- What: `login_notice` (up to 500 chars, v2.1.319+ on server and client for terminal) shown on the verification page and terminal sign-in screen before sign-in, readable by anyone who can reach the gateway.
- Status: GA
- Evidence: https://code.claude.com/docs/en/claude-apps-gateway-config.md (login_notice)

### Claude-drafted feedback, You should know, /claude-api subcommands
- What:
  - SendFeedback card and `/feedback` drafts queue (2.1.247, Week 35).
  - Built-in "You should know" side-agent mod (2.1.287); first-party sessions with telemetry on.
  - `/claude-api` skill subcommands: `cost-optimize` (2.1.247), `upgrade` (2.1.239), `prompt-audit` (2.1.221), `managed-agents-onboard` (2.1.290), `managed-agents-secure` (2.1.304), `api-version` (2.1.307), `latency-optimize` (2.1.319).
- Status: GA
- Evidence: https://code.claude.com/docs/en/changelog.md (2.1.221, 2.1.239, 2.1.247, 2.1.287, 2.1.290, 2.1.304, 2.1.307, 2.1.319); https://code.claude.com/docs/en/whats-new/2026-w35.md
- Notes: `latency-optimize` has no docs-page mention in the dump.

## 3. Distinctive or unusual

Each item links to an entry above by its heading.

- **Mods: in-process JS/TS middleware over almost every engine decision** (area 13, "Mods: in-process JS/TS extension model"; area 18, "Policy mods").
  - Mods can intercept tool calls, permission decisions, model requests, system-prompt sections, stored transcript rows, settings merge and other mods' API calls.
  - Claude Code's own `/diff`, AGENTS.md support and security guard are built on mods.
  - A mod must use the host-provided `$` API in a way that static analysis can enumerate, which makes unsandboxed plugins auditable.
- **Credential masking at the sandbox proxy** (area 5, "Credential protection: deny and mask (sentinel substitution)"; area 21, "Network secrets (credential injection by egress proxy)").
  - Sandboxed commands see a sentinel value; the host-side egress proxy substitutes the real secret, with TLS termination and SigV4 re-signing.
  - The agent's processes never hold the secret. Cloud sessions and Claude Tag's Agent Proxy apply the same pattern.
- **Idle compaction timed to the prompt-cache TTL** (area 6, "Idle compaction before cache expiry"): an idle conversation is compacted at about 54 minutes, before its 1-hour cache expires, so the next turn does not re-cache the whole history. Compaction is scheduled by cost, not only by context size.
- **Pull-only execution with leases** (area 17, "Self-hosted session lifecycle: claim, lease, heartbeat, requeue"; area 21, "One-owner-per-runner lock").
  - Self-hosted runners and Remote Control make only outbound HTTPS calls. The poll doubles as heartbeat and lease renewal.
  - Each session gets a single-use work order and a child process; a runner is locked to one owner and exits after draining.
- **Auto mode: a second model as approver** (area 5, auto mode entries).
  - Rules are written in prose tiers (`hard_deny`, `soft_deny`, `allow`).
  - Tool results are stripped from classifier input so injected text cannot address the classifier.
  - A circuit breaker hands control back to a human after 3 consecutive or 20 total blocks.
  - Broad code-exec allow rules are dropped while auto mode is on.
  - Server-side review is folded into the main model request, and it fails closed if no verdict arrives.
- **Delegation hygiene** (area 21, "Subagent output scanning and inter-agent authority rules"; area 9, "Auto mode review of subagents").
  - Before the parent reads a subagent's report, text that imitates harness control tags or turn markers is rewritten.
  - Subagents cannot escalate past the parent's permission mode.
  - The classifier reviews task descriptions at spawn time.
- **Strict-value-wins settings and fail-closed parsing** (area 5, "Strict-value-wins exceptions to precedence (one-way ratchets)"; area 20, "Trust-gated and repo-ignored keys").
  - For a fixed set of security keys, a lower scope can tighten a managed value but never loosen it.
  - Unreadable lock files read as their strict value.
  - Repo settings cannot enable `autoMode`, disable filesystem isolation, or start a session in bypass mode.
- **Hook `defer`** (area 13, "PreToolUse "defer" for headless integrations"): a PreToolUse hook can end the turn with the tool call pending, so a headless host can exit and resume after a human decides, with no timeout. The SDK adds out-of-band `canUseTool` answers and redelivers pending requests after a reconnect.
- **Pluggable `SessionStore` with a conformance suite** (area 11, "Pluggable SessionStore adapters (external transcript storage)"): the store is a best-effort mirror of the local JSONL transcript, with explicit dedupe and error semantics.
- **One audit stream for the approval path** (area 19): the `claude_code.safety_event` OTel event fires for every action a rule, classifier, hook or sandbox stopped, prompted or sent back.
- **Workflows with deterministic replay** (area 9): in the scripted multi-agent runtime, `Date.now()` and `Math.random()` throw, so cached agent results can be replayed by prompt order.
- **Worktree hardening** (area 12, "Worktree base branch, .worktreeinclude, LFS/filter-driver neutralisation"): worktree creation neutralises repo-local filter drivers, `includeIf` and LFS custom transfer, because it treats `.git/config` as attacker-writable. Commands are refused when static analysis cannot show that git stays inside the worktree.
- **REVIEW.md trust** (area 7, "REVIEW.md branch-protection trust model and includes"): review instructions come only from the base branch, and how much of them is honoured depends on branch protection. A PR author cannot soften their own review.

## 4. Architecture and UX notes (public sources only)

- **Where the loop runs.** It always runs in a `claude` process: on the user's machine (terminal, IDE, Desktop, Remote Control), in a per-session Anthropic VM (cloud sessions, routines, Claude Tag, Code Review), or in a customer-run child process under `claude self-hosted-runner`. In every mode the control plane, queueing and claude.ai UI are Anthropic-hosted (https://code.claude.com/docs/en/how-claude-code-works.md; https://code.claude.com/docs/en/self-hosted-environments.md).
- **Local process model.**
  - Each Bash command is a separate process. Shell state is snapshotted once at startup and replayed for each command.
  - Built-in file and web tools run inside the Claude Code process.
  - The OS sandbox (Seatbelt on macOS; on Linux, namespaces with `socat` to a host proxy, built on `@anthropic-ai/sandbox-runtime`) wraps only spawned shell processes.
  - Hooks, MCP servers, LSP servers and mods run unsandboxed as the user (https://code.claude.com/docs/en/sandboxing.md; https://code.claude.com/docs/en/plugins/security.md).
- **Background sessions.** A per-user supervisor daemon (`claude daemon`) hosts each background session as its own process with a PTY host. Agent view and `claude attach` are clients of the daemon. Idle processes are reaped after about an hour and resumed from the transcript (https://code.claude.com/docs/en/agent-view.md).
- **State.**
  - Each session has an append-only local JSONL transcript that records the system prompt, tool definitions and request parameters.
  - Subagent transcripts are nested under the session.
  - File checkpoints are kept separately under `~/.claude/file-history/` (https://code.claude.com/docs/en/sessions.md; https://code.claude.com/docs/en/checkpointing.md).
- **Remote Control.**
  - The local CLI registers with the Anthropic API and polls for work. Messages then flow over a "streaming connection"; the wire format is not documented.
  - The transcript is persisted server-side for multi-device sync.
  - Server mode hosts up to 32 sessions in one process (https://code.claude.com/docs/en/remote-control.md).
- **Self-hosted runner.**
  - A long-lived supervisor polls with a lease of about 60 s.
  - Each session runs as a child `claude` process with its own SSE event stream, inference and git connections.
  - Supervisor-to-child signalling uses stdin for token rotation and end signals, and fd 3 for activity.
  - Per-session policy (tools, rules, MCP, hooks, model) arrives with the work payload (https://code.claude.com/docs/en/self-hosted-environments-reference.md).
- **SDK.**
  - The SDK spawns the bundled CLI as a child process, one per session, speaking NDJSON over stdin/stdout.
  - Permission prompts and initialisation are `control_request`/`control_response` envelopes.
  - In-process SDK MCP tools and callbacks run in the host process.
  - Hosts can swap the transport: TypeScript has `spawnClaudeCodeProcess`, Python has a `Transport` class (https://code.claude.com/docs/en/agent-sdk/hosting.md).
- **IDE bridge.** The extension hosts a loopback `ws://` MCP server, discovered through `~/.claude/ide/<port>.lock` and protected by a per-activation token. The CLI connects to it (https://code.claude.com/docs/en/vs-code.md).
- **Feature flags.** Many behaviours are gated by flags fetched from Anthropic (GrowthBook). Sessions that skip flag fetches (third-party providers, telemetry off) fall back to compiled-in defaults (https://code.claude.com/docs/en/llm-gateway-rollout.md).
- **Claude apps gateway.**
  - It ships inside the same binary (`claude gateway`): a stateless HTTP server backed by PostgreSQL.
  - Clients sign in with an RFC 8628 device grant; the gateway does OIDC with PKCE upstream.
  - It serves managed settings per IdP group, relays OTLP and fails over across inference providers (https://code.claude.com/docs/en/claude-apps-gateway.md; https://code.claude.com/docs/en/claude-apps-gateway-deploy.md).

## 5. Relevance to a self-hosted master/node harness

- **The self-hosted runner is the closest public design to master/node.** Look at the pull-only lease and heartbeat, single-use work orders, one owner per runner, drain/retire/hold, and per-session child processes with an fd-based control channel (area 17, "Self-hosted session lifecycle: claim, lease, heartbeat, requeue"; area 10, "Runner idle release, retire, drain and hold"). It inverts our invariant, though: Anthropic's control plane pushes policy to the runner with each work payload. "The node decides" is a real difference worth keeping and stating loudly.
- **Copy the credential-injection proxy.** Secrets live at the egress boundary, never in the agent's environment (area 5, "Credential protection: deny and mask (sentinel substitution)"). This fits a node that must keep `Secret<…>` out of tool processes.
- **Repo-scoped config must not widen policy.** Their explicit list of keys a checked-in `.claude/settings.json` cannot set, and the strict-value-wins ratchets (area 20, "Trust-gated and repo-ignored keys"; area 5, "Strict-value-wins exceptions to precedence (one-way ratchets)"), map directly onto `node.toml` authority. Their headless gap is a warning, not a model: `-p` treats any folder as trusted and runs repo hooks and MCP servers (area 5, "Workspace trust gating for hooks, status line, skills").
- **Bash deny rules are not a security boundary,** by the vendor's own statement (area 5 permission rule entries). Approvals for shell work need a sandbox behind them; string matching on commands is UX only.
- **Approval audit trail.** A single `safety_event` per stop, prompt or bounce-back, with provenance (area 19), fits an event log naturally: approval decisions are events the UI folds over.
- **Resumable approvals.** Hook `defer` and the SDK's out-of-band `canUseTool` with redelivery after reconnect (area 13; area 5) solve "the approver is on another device and the link dropped". That is our master/node approval path exactly.
- **Remote Control's server-side transcript and multi-device takeover rules** (area 17, "Remote Control resume, reconnect and takeover rules") are the UX bar for a web dashboard watching node sessions.
- **Prompt-cache-aware context management.** Idle compaction, refusing mid-conversation MCP tool changes without `--force`, and cache-miss attribution in `/usage` (areas 6, 13, 19) are cheap wins that users notice in cost.
- **Delegation boundary scanning** (area 21, "Subagent output scanning before the parent reads it") is a cheap prompt-injection layer for any sub-agent or cross-node report.
- **Legal constraint.** A third-party harness may not offer claude.ai login or route requests through users' Free/Pro/Max/Ultra credentials; it must use API keys or cloud-provider credentials (area 21, "Authentication and claude.ai login restriction"; https://code.claude.com/docs/en/legal-and-compliance.md, Authentication and credential use). This decides how rizzy-harness can offer Claude models.

## 6. Overhyped, stale or contradictory

- **Docs lag the changelog.**
  - The weekly digest index stops at Week 37 (2026-09-11), and Week 38 returns 404. Opus/Sonnet/Haiku 5.5, Fable 5.2, Mods, idle compaction, AGENTS.md support and the TaskOutput removal have no digest.
  - `fast-mode.md` still says fast mode is unavailable on Google Cloud, a day after 2.1.322 added it on Vertex.
  - The status line JSON sample shows version 2.1.90.
  - The mods reference says it is current "as of" v2.1.290/2.1.299 while citing features up to 2.1.321.
  - The settings JSON schema is acknowledged to lag the CLI.
  - Anchors in `server-managed-settings.md` point to renamed sections.
- **Default-model advice contradicts the defaults.** `how-claude-code-works.md` and `costs.md` say Sonnet handles most work and Opus should be reserved, while every plan defaults to Opus 5.5 (https://code.claude.com/docs/en/model-config.md). `costs.md` describes thinking budgets that `model-config.md` says current models ignore.
- **Churn.**
  - Subagent nesting went from 5 levels, to off, to 3 within six weeks. The digest still says 5.
  - `keybindingFlavor` lived 23 days and `taskOutputMaxChars` about 2 weeks.
  - Output styles were deprecated and then un-deprecated 5 days later.
  - `/simplify` was renamed `/code-review`, and `/simplify` now names a different command.
  - `/fork` has meant three things over time, and today it behaves differently depending on whether agent view is on.
- **Status labels disagree.**
  - Fullscreen is "research preview" yet is the default renderer for users since 2026-05-06.
  - `claude agents` is "research preview" on one page and unlabelled elsewhere.
  - Plugin evals, monitors and themes are presented as available but live under `experimental.*` manifest keys.
  - Auto mode is "available to all users on every provider" on one page and model- and provider-gated on another.
- **Pages disagree on mechanism.**
  - Spend limits: a group cap is "not a shared pool" on one page, while the config reference documents `pooled_budget`.
  - Cross-session messaging works across "any of your machines" (changelog 2.1.224) or on the same machine (digest).
  - MCP startup timeout is 30 s (quickstart) against a 5 s connect cap (mcp.md).
  - Two MCP statements disagree on whether "Clear authentication" revokes upstream tokens.
  - `self-hosted-environments-deploy` recommends `--capacity 1` but its own recipes use 4.
- **Vendor claims with no mechanism or numbers.**
  - Fast mode "up to 2.5x faster".
  - Ultrareview "every finding independently reproduced", at $5–25.
  - Enterprise cost averages.
  - GitLab "sandboxed execution", while the example jobs configure no sandbox.
  - Computer use "flags prompt injection".
  - The communications kit says the CLI talks directly to Anthropic's API, which is false for gateway and cloud-provider deployments.
- **The vendor admits security limits, which is useful.**
  - Bash rules are "not a security boundary".
  - The sandbox "is not a complete isolation boundary".
  - Auto mode "does not guarantee safety".
  - Policy mods can be bypassed by `--safe-mode` and worker crashes.
  - Server-managed settings are not a security boundary.

## 7. Unverified

- **Wire protocols.** Neither the Remote Control wire protocol (SSE or WebSocket, message schema) nor the full SDK control-request protocol is specified publicly. Settling either needs a protocol spec or a network trace.
- **API-level context editing.** No Claude Code page says when Claude Code clears old tool results server-side. Only a `context_management` field in the gateway protocol and a status-line cache note hint at it.
- **Exact lists the docs say to print with a CLI command.** The built-in auto-mode classifier rules and the read-only Bash allowlist (`claude auto-mode defaults`), the variables `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` removes, and the preapproved WebFetch domains. Running the commands would settle them.
- **How the server-side probe of incoming tool results works,** and how WebFetch's summarising model resists injection. Only named, not described.
- **Cloud VM isolation technology** (microVM, container or full VM), idle-pause thresholds, and Remote Control transcript retention.
- **Mods' JavaScript engine and isolation level.** The docs say only "worker thread".
- **Rewind and files that `Write` created.** The docs do not say whether rewinding deletes them.
- **SDK package licences.** The Python SDK is MIT and the TypeScript SDK is "All rights reserved" per `00-targets.md`. The repos were not reopened in this pass.
- **Claude Tag pages not opened** (agent identity, retention, models): Claude Tag retention periods remain unknown.
- **Linked but unopened material:** the full input list for `claude-code-action` (repo docs) and the MDM templates in the examples repo.
- **Changelog-only items with no docs page,** such as `CLAUDE_CODE_DISABLE_FEATURE_FLAGS`, `CLAUDE_CODE_SUBAGENT_EFFORT` and the mod `decider: "person"`. They exist per the changelog, but their behaviour is not documented.
- **Partial changelog check.** The GitHub CHANGELOG.md was matched against the docs changelog only on its first two entries.

## 8. Sources

All fetched 2026-11-10. Docs pages were fetched as `.md` from
code.claude.com; the title is the page's first heading. Each is listed with
the slice that used it (S1 core loop and tools; S2 permissions and security;
S3 hooks, skills, memory, context; S4 MCP and plugins; S5 mods, channels,
artifacts, agent view; S6 subagents, background, headless; S7a cloud, remote,
self-hosted; S7b CI, IDE, Slack, Tag, browser; S8 models, cost, telemetry;
S9a settings; S9b admin and gateway; S10 Agent SDK; S11 changelog).

- https://code.claude.com/docs/llms.txt (Claude Code Docs index): full page list used to split the survey.
- https://code.claude.com/docs/en/how-claude-code-works.md (How Claude Code works): S1
- https://code.claude.com/docs/en/features-overview.md (Extend Claude Code): S1
- https://code.claude.com/docs/en/claude-directory.md (Explore the .claude directory): S1
- https://code.claude.com/docs/en/overview.md (Overview): S1
- https://code.claude.com/docs/en/quickstart.md (Quickstart): S1
- https://code.claude.com/docs/en/platforms.md (Platforms and integrations): S1
- https://code.claude.com/docs/en/tools-reference.md (Tools reference): S1
- https://code.claude.com/docs/en/interactive-mode.md (Interactive mode): S1
- https://code.claude.com/docs/en/commands.md (Commands): S1
- https://code.claude.com/docs/en/cli-reference.md (CLI reference): S1
- https://code.claude.com/docs/en/fullscreen.md (Fullscreen rendering): S1
- https://code.claude.com/docs/en/terminal-config.md (Configure your terminal for Claude Code): S1
- https://code.claude.com/docs/en/keybindings.md (Customize keyboard shortcuts): S1
- https://code.claude.com/docs/en/voice-dictation.md (Voice dictation): S1
- https://code.claude.com/docs/en/accessibility.md (Use Claude Code with a screen reader): S1
- https://code.claude.com/docs/en/sessions.md (Manage sessions): S1
- https://code.claude.com/docs/en/checkpointing.md (Checkpointing): S1
- https://code.claude.com/docs/en/common-workflows.md (Common workflows): S1
- https://code.claude.com/docs/en/best-practices.md (Best practices for Claude Code): S1
- https://code.claude.com/docs/en/deep-links.md (Launch sessions from links): S1
- https://code.claude.com/docs/en/glossary.md (Glossary): S1
- https://code.claude.com/docs/en/prompt-library.md (Prompt library): S1
- https://code.claude.com/docs/en/permissions.md (Configure permissions): S2
- https://code.claude.com/docs/en/permission-modes.md (Choose a permission mode): S2
- https://code.claude.com/docs/en/auto-mode-config.md (Configure auto mode): S2
- https://code.claude.com/docs/en/auto-mode-classifier-billing.md (Auto mode classifier request charges): S2
- https://code.claude.com/docs/en/sandboxing.md (Configure the sandboxed Bash tool): S2
- https://code.claude.com/docs/en/sandbox-environments.md (Choose a sandbox environment): S2
- https://code.claude.com/docs/en/devcontainer.md (Development containers): S2
- https://code.claude.com/docs/en/security.md (Security): S2
- https://code.claude.com/docs/en/security-guidance.md (Catch security issues as Claude writes code): S2
- https://code.claude.com/docs/en/claude-security.md (Scan your codebase for vulnerabilities): S2
- https://code.claude.com/docs/en/data-usage.md (Data usage): S2
- https://code.claude.com/docs/en/zero-data-retention.md (Zero data retention): S2
- https://code.claude.com/docs/en/legal-and-compliance.md (Legal and compliance): S2
- https://code.claude.com/docs/en/hipaa-setup.md (Set up Claude Code (local mode) for a HIPAA-ready organization): S2
- https://code.claude.com/docs/en/network-config.md (Enterprise network configuration): S2
- https://code.claude.com/docs/en/authentication.md (Authentication): S2
- https://code.claude.com/docs/en/hooks.md (Hooks reference): S3
- https://code.claude.com/docs/en/hooks-guide.md (Automate actions with hooks): S3
- https://code.claude.com/docs/en/skills.md (Extend Claude with skills): S3
- https://code.claude.com/docs/en/memory.md (How Claude remembers your project): S3
- https://code.claude.com/docs/en/context-window.md (Explore the context window): S3
- https://code.claude.com/docs/en/prompt-caching.md (How Claude Code uses prompt caching): S3
- https://code.claude.com/docs/en/output-styles.md (Output styles): S3
- https://code.claude.com/docs/en/statusline.md (Customize your status line): S3
- https://code.claude.com/docs/en/large-codebases.md (Set up Claude Code in a monorepo or large codebase): S3
- https://code.claude.com/docs/en/model-config.md (Model configuration): S3
- https://code.claude.com/docs/en/settings-reference.md (All settings): S3
- https://code.claude.com/docs/en/llm-gateway-protocol.md (Claude Code gateway compatibility guide): S3
- https://code.claude.com/docs/en/mcp.md (Connect Claude Code to tools via MCP): S4
- https://code.claude.com/docs/en/mcp-quickstart.md (Connect to MCP servers): S4
- https://code.claude.com/docs/en/managed-mcp.md (Control MCP server access for your organization): S4
- https://code.claude.com/docs/en/plugins/overview.md (Plugins overview): S4
- https://code.claude.com/docs/en/plugins/components.md (Add components to a plugin): S4
- https://code.claude.com/docs/en/plugins/manifest-reference.md (Plugin manifest reference): S4
- https://code.claude.com/docs/en/plugins/marketplace-reference.md (Marketplace reference): S4
- https://code.claude.com/docs/en/plugins/loading.md (Plugin loading reference): S4
- https://code.claude.com/docs/en/plugins/install.md (Install and manage plugins): S4
- https://code.claude.com/docs/en/plugins/security.md (Plugin security and trust): S4
- https://code.claude.com/docs/en/plugins/dependencies.md (Plugin dependencies): S4
- https://code.claude.com/docs/en/plugins/org.md (Manage Claude Code plugins for your organization): S4
- https://code.claude.com/docs/en/plugins/host-marketplace.md (Host and maintain a marketplace): S4
- https://code.claude.com/docs/en/plugins/code-intelligence.md (Code intelligence plugins): S4
- https://code.claude.com/docs/en/plugins/cli-hints.md (Recommend your plugin from your CLI): S4
- https://code.claude.com/docs/en/plugins/relevance.md (Recommend plugins for your org): S4
- https://code.claude.com/docs/en/plugins/measure.md (Measure plugin cost and usage): S4
- https://code.claude.com/docs/en/plugin-evals.md (Test plugins with evals): S4
- https://code.claude.com/docs/en/plugins/cli-reference.md (Plugin commands reference): S4
- https://code.claude.com/docs/en/plugins/create.md (Create a Claude Code plugin): S4
- https://code.claude.com/docs/en/plugins/publish.md (Publish and distribute a plugin): S4
- https://code.claude.com/docs/en/plugins/anthropic-marketplaces.md (Anthropic's marketplaces): S4
- https://code.claude.com/docs/en/plugins/create-marketplace.md (Create a marketplace): S4
- https://code.claude.com/docs/en/plugins/troubleshooting.md (Troubleshoot plugins): S4
- https://code.claude.com/docs/en/plugins/mods/overview.md (Mods overview): S5
- https://code.claude.com/docs/en/plugins/mods/create.md (Create a mod): S5
- https://code.claude.com/docs/en/plugins/mods/api.md (Use the mods API): S5
- https://code.claude.com/docs/en/plugins/mods/events.md (React to events with a mod): S5
- https://code.claude.com/docs/en/plugins/mods/interface.md (Draw in the interface with a mod): S5
- https://code.claude.com/docs/en/plugins/mods/reference.md (Mods reference): S5
- https://code.claude.com/docs/en/plugins/mods/admin.md (Manage mods for your organization): S5
- https://code.claude.com/docs/en/plugins/mods/test.md (Test a mod): S5
- https://code.claude.com/docs/en/plugins/mods/gallery.md (Interface gallery for mods): S5
- https://code.claude.com/docs/en/plugins/mods/troubleshoot.md (Troubleshoot a mod): S5
- https://code.claude.com/docs/en/channels.md (Push events into a running session with channels): S5
- https://code.claude.com/docs/en/channels-reference.md (Channels reference): S5
- https://code.claude.com/docs/en/cross-session-messaging.md (Message your other Claude Code sessions): S5
- https://code.claude.com/docs/en/artifacts.md (Share session output as artifacts): S5
- https://code.claude.com/docs/en/agent-view.md (Manage multiple agents with agent view): S5
- https://code.claude.com/docs/en/sub-agents.md (Create custom subagents): S6
- https://code.claude.com/docs/en/agent-teams.md (Orchestrate teams of Claude Code sessions): S6
- https://code.claude.com/docs/en/agents.md (Run agents in parallel): S6
- https://code.claude.com/docs/en/workflows.md (Orchestrate subagents at scale with dynamic workflows): S6
- https://code.claude.com/docs/en/worktrees.md (Run parallel sessions with worktrees): S6
- https://code.claude.com/docs/en/goal.md (Keep Claude working toward a goal): S6
- https://code.claude.com/docs/en/scheduled-tasks.md (Run prompts on a schedule): S6
- https://code.claude.com/docs/en/routines.md (Automate work with scheduled tasks): S6
- https://code.claude.com/docs/en/claude-projects.md (Let Claude coordinate ongoing work with Projects): S6
- https://code.claude.com/docs/en/advisor.md (Escalate hard decisions with the advisor tool): S6
- https://code.claude.com/docs/en/ultrareview.md (Find bugs with ultrareview): S6
- https://code.claude.com/docs/en/code-review-local.md (Review locally with /code-review): S6
- https://code.claude.com/docs/en/headless.md (Run Claude Code programmatically): S6
- https://code.claude.com/docs/en/claude-code-on-the-web.md (Use Claude Code in the cloud): S7a
- https://code.claude.com/docs/en/web-quickstart.md (Get started with Claude Code in the cloud): S7a
- https://code.claude.com/docs/en/cloud-environments.md (Configure cloud environments): S7a
- https://code.claude.com/docs/en/remote-control.md (Continue local sessions from any device with Remote Control): S7a
- https://code.claude.com/docs/en/mobile.md (Claude Code on mobile): S7a
- https://code.claude.com/docs/en/self-hosted-environments.md (Self-hosted environments): S7a
- https://code.claude.com/docs/en/self-hosted-environments-quickstart.md (Self-hosted environments quickstart): S7a
- https://code.claude.com/docs/en/self-hosted-environments-configuration.md (Customize sessions in self-hosted environments): S7a
- https://code.claude.com/docs/en/self-hosted-environments-deploy.md (Deploy self-hosted environments to production): S7a
- https://code.claude.com/docs/en/self-hosted-environments-identity.md (Verify session identity in self-hosted environments): S7a
- https://code.claude.com/docs/en/self-hosted-environments-reference.md (Self-hosted environments reference): S7a
- https://code.claude.com/docs/en/self-hosted-environments-testing.md (Test self-hosted environments end to end): S7a
- https://code.claude.com/docs/en/github-actions.md (Claude Code GitHub Actions): S7b
- https://code.claude.com/docs/en/github-actions-cloud-providers.md (Use Claude Code GitHub Actions with cloud providers): S7b
- https://code.claude.com/docs/en/gitlab-ci-cd.md (Claude Code GitLab CI/CD): S7b
- https://code.claude.com/docs/en/github-enterprise-server.md (Claude Code with GitHub Enterprise Server): S7b
- https://code.claude.com/docs/en/code-review.md (Code Review): S7b
- https://code.claude.com/docs/en/code-review-setup.md (Set up Code Review): S7b
- https://code.claude.com/docs/en/code-review-customize.md (Customize Code Review): S7b
- https://code.claude.com/docs/en/code-review-pull-requests.md (Read and request reviews on a pull request): S7b
- https://code.claude.com/docs/en/code-review-costs.md (Code Review costs and usage): S7b
- https://code.claude.com/docs/en/code-review-troubleshooting.md (Troubleshoot Code Review): S7b
- https://code.claude.com/docs/en/slack.md (Claude Code in Slack): S7b
- https://code.claude.com/docs/en/claude-tag.md (Work with Claude Tag, HTML render of https://claude.com/docs/claude-tag/overview): S7b
- https://claude.com/docs/claude-tag/overview (Claude Tag docs (claude.com)): S7b
- https://claude.com/docs/claude-tag/concepts/how-it-works.md (Claude Tag docs (claude.com)): S7b
- https://claude.com/docs/claude-tag/concepts/for-claude-code-users.md (Claude Tag docs (claude.com)): S7b
- https://claude.com/docs/claude-tag/concepts/security-and-data.md (Claude Tag docs (claude.com)): S7b
- https://code.claude.com/docs/en/vs-code.md (Use Claude Code in VS Code): S7b
- https://code.claude.com/docs/en/jetbrains.md (JetBrains IDEs): S7b
- https://code.claude.com/docs/en/chrome.md (Use Claude Code with Chrome): S7b
- https://code.claude.com/docs/en/computer-use.md (Let Claude use your computer from the CLI): S7b
- https://code.claude.com/docs/en/fast-mode.md (Speed up responses with fast mode): S8
- https://code.claude.com/docs/en/costs.md (Manage costs effectively): S8
- https://code.claude.com/docs/en/monitoring-usage.md (Monitoring): S8
- https://code.claude.com/docs/en/analytics.md (Track team usage with analytics): S8
- https://code.claude.com/docs/en/amazon-bedrock.md (Claude Code on Amazon Bedrock): S8
- https://code.claude.com/docs/en/google-vertex-ai.md (Claude Code on Google Cloud's Agent Platform): S8
- https://code.claude.com/docs/en/microsoft-foundry.md (Claude Code on Microsoft Foundry): S8
- https://code.claude.com/docs/en/claude-platform-on-aws.md (Claude Code on Claude Platform on AWS): S8
- https://code.claude.com/docs/en/llm-gateway.md (Other LLM gateways): S8
- https://code.claude.com/docs/en/llm-gateway-connect.md (Connect Claude Code to an LLM gateway): S8
- https://code.claude.com/docs/en/llm-gateway-rollout.md (Roll out an LLM gateway for your organization): S8
- https://code.claude.com/docs/en/gateways.md (Run Claude Code through a gateway): S8
- https://code.claude.com/docs/en/third-party-integrations.md (Enterprise deployment overview): S8
- https://code.claude.com/docs/en/settings.md (Settings files and precedence): S9a
- https://code.claude.com/docs/en/settings-example.md (Example settings files): S9a
- https://code.claude.com/docs/en/env-vars.md (Environment variables): S9a
- https://code.claude.com/docs/en/managed-settings.md (Deploy managed settings): S9a
- https://code.claude.com/docs/en/server-managed-settings.md (Configure server-managed settings): S9a
- https://code.claude.com/docs/en/debug-your-config.md (Debug your configuration): S9a
- https://code.claude.com/docs/en/feature-availability.md (Feature availability): S9a
- https://code.claude.com/docs/en/setup.md (Advanced setup): S9a
- https://code.claude.com/docs/en/troubleshooting.md (Troubleshooting): S9a
- https://code.claude.com/docs/en/troubleshoot-install.md (Troubleshoot installation and login): S9a
- https://code.claude.com/docs/en/errors.md (Error reference): S9a
- https://code.claude.com/docs/en/admin-setup.md (Set up Claude Code for your organization): S9b
- https://code.claude.com/docs/en/claude-apps-gateway.md (Claude apps gateway for Amazon Bedrock, Claude Platform on AWS, Google Cloud, and Microsoft Foundry): S9b
- https://code.claude.com/docs/en/claude-apps-gateway-config.md (Claude apps gateway configuration): S9b
- https://code.claude.com/docs/en/claude-apps-gateway-deploy.md (Claude apps gateway deployment and operations): S9b
- https://code.claude.com/docs/en/claude-apps-gateway-spend-limits.md (Claude apps gateway spend limits): S9b
- https://code.claude.com/docs/en/claude-apps-gateway-on-aws.md (Deploy Claude apps gateway on AWS): S9b
- https://code.claude.com/docs/en/claude-apps-gateway-on-gcp.md (Deploy Claude apps gateway on Google Cloud): S9b
- https://code.claude.com/docs/en/corporate-launcher.md (Run Claude Code behind a corporate launcher): S9b
- https://code.claude.com/docs/en/champion-kit.md (Champion kit): S9b
- https://code.claude.com/docs/en/communications-kit.md (Communications kit): S9b
- https://code.claude.com/docs/en/agent-sdk/overview.md (Agent SDK overview): S10
- https://code.claude.com/docs/en/agent-sdk/quickstart.md (Quickstart): S10
- https://code.claude.com/docs/en/agent-sdk/agent-loop.md (How the agent loop works): S10
- https://code.claude.com/docs/en/agent-sdk/claude-code-features.md (Use Claude Code features in the SDK): S10
- https://code.claude.com/docs/en/agent-sdk/configuration.md (Configure your agent): S10
- https://code.claude.com/docs/en/agent-sdk/cost-tracking.md (Track cost and usage): S10
- https://code.claude.com/docs/en/agent-sdk/custom-tools.md (Give Claude custom tools): S10
- https://code.claude.com/docs/en/agent-sdk/examples.md (Examples): S10
- https://code.claude.com/docs/en/agent-sdk/file-checkpointing.md (Rewind file changes with checkpointing): S10
- https://code.claude.com/docs/en/agent-sdk/hooks.md (Intercept and control agent behavior with hooks): S10
- https://code.claude.com/docs/en/agent-sdk/hosting.md (Hosting the Agent SDK): S10
- https://code.claude.com/docs/en/agent-sdk/mcp.md (Connect to external tools with MCP): S10
- https://code.claude.com/docs/en/agent-sdk/migration-guide.md (Migrate to Claude Agent SDK): S10
- https://code.claude.com/docs/en/agent-sdk/modifying-system-prompts.md (Modifying system prompts): S10
- https://code.claude.com/docs/en/agent-sdk/observability.md (Observability with OpenTelemetry): S10
- https://code.claude.com/docs/en/agent-sdk/permissions.md (Configure permissions): S10
- https://code.claude.com/docs/en/agent-sdk/plugins.md (Plugins in the SDK): S10
- https://code.claude.com/docs/en/agent-sdk/python.md (Agent SDK reference - Python): S10
- https://code.claude.com/docs/en/agent-sdk/secure-deployment.md (Securely deploying AI agents): S10
- https://code.claude.com/docs/en/agent-sdk/session-storage.md (Persist sessions to external storage): S10
- https://code.claude.com/docs/en/agent-sdk/sessions.md (Work with sessions): S10
- https://code.claude.com/docs/en/agent-sdk/skills.md (Extend agents with skills): S10
- https://code.claude.com/docs/en/agent-sdk/streaming-output.md (Stream responses in real-time): S10
- https://code.claude.com/docs/en/agent-sdk/streaming-vs-single-mode.md (Streaming Input): S10
- https://code.claude.com/docs/en/agent-sdk/structured-outputs.md (Get structured output from agents): S10
- https://code.claude.com/docs/en/agent-sdk/subagents.md (Subagents in the SDK): S10
- https://code.claude.com/docs/en/agent-sdk/todo-tracking.md (Track todos): S10
- https://code.claude.com/docs/en/agent-sdk/tool-search.md (Scale to many tools with tool search): S10
- https://code.claude.com/docs/en/agent-sdk/troubleshooting.md (Troubleshoot the Agent SDK): S10
- https://code.claude.com/docs/en/agent-sdk/typescript.md (Agent SDK reference - TypeScript): S10
- https://code.claude.com/docs/en/agent-sdk/typescript-v2-preview.md (TypeScript SDK V2 session API (removed)): S10
- https://code.claude.com/docs/en/changelog.md (Claude Code changelog): S11
- https://raw.githubusercontent.com/anthropics/claude-code/refs/heads/main/CHANGELOG.md (Claude Code CHANGELOG.md (raw, GitHub)): S11
- https://code.claude.com/docs/en/whats-new/2026-w27.md (Week 27 · June 29 – July 3, 2026): S11
- https://code.claude.com/docs/en/whats-new/2026-w13.md (Week 13 · March 23–27, 2026): S11
