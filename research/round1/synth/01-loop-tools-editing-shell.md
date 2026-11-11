# Cross-harness synthesis 01: agent loop, built-in tools, code editing, terminal and shell

Research date: 2026-11-10. Synthesis only: nothing here comes from outside the survey and review files.

## 1. Scope

Areas covered: **Area 1** (agent loop and execution model), **Area 2** (built-in tools), **Area 3** (code editing and file operations), **Area 4** (terminal and shell integration). Step limits, loop detection, interruption and steering, parallel tool calls and output capping are placed here.

Files and ranges read (line numbers are in the survey files):

| File | Read |
|---|---|
| `claude-code.md` | Areas 1-4 (119-531), sections 3-7 (5202-5340); targeted greps for tool search (1194, 1218) |
| `claude-code.review-1.md` | whole (it covers Areas 1-8); `review-2.md` and `review-3.md` only for corrections touching these areas |
| `codex.md` | 34-200, 986-1082 (sections 3, 6, 7), 1159+ (review) |
| `opencodex.md` | 27-57, 146 (Remote Workspace), 195-230 |
| `opencode.md` | 28-147, 153, 458-512, 562+ |
| `claude-desktop.md` | 15-69, 469-528, 560+ |
| `deepseek-harness.md` | 21-116, 170-178 (spill), 261 (jobs), 533-596, 616+ |
| `oh-my-pi.md` | 7-124, 549-620, review |
| `pi.md` | 5-120, 561-662, review |
| `cursor.md` | 15-72, 456-524, 595+ |
| `antigravity.md` | 28-100, 147, 304-311, 494-560, 584+ |
| `openhands.md` | 7-90, 168-185, 459-545, review |
| `openclaw.md` | 14-110, 596-662, 749+ |
| `goose.md` | 14-83, 164-178, 518-578, 646+ |
| `hermes-agent.md` | 15-142, 679-740, 783+ |
| `zed-acp.md` | A1-A4, A12, B3-B6, 355-420, 447+ |
| `warp.md` | 19-120, 546-640 |
| `cline.md` | 16-74, 476-534, 593+ |
| `own-rizzy-harness.md` | sections 1-4, 7, (a)-(c) and the unmerged-branch notes |

Not read: the other areas of every survey, except where a feature in my areas is documented elsewhere (OpenCode `doom_loop` note, goose and Hermes tool search, Cursor `loop_limit`). Nothing could not be opened.

Conventions used in this file:

- Hermes evidence paths are given as the full URL (docs base `https://hermes-agent.nousresearch.com/docs` plus the path the survey cites). OpenHands evidence is the full URL (`D:` in the survey means `https://docs.openhands.dev`). Warp pages are cited by their canonical URL (the survey read them through section files).
- Check values: `reviewed` = the survey entry or the review file says it was verified; `corrected` = corrected in review (the corrected text is used); `unreviewed` = not in the review's checked set; `unverified` = the survey or review could not confirm it.
- Claude Desktop's survey says its Code tab runs the same engine as Claude Code and documents nothing separate for most of my areas. Claude Desktop cells in the matrix are therefore `-` unless the Desktop survey itself documents the feature; read Claude Code's rows as applying to the Code tab.
- DeepSeek harness is a developer preview and every release is a prerelease, so its status is `preview` (matrix `B`) everywhere.
- OpenCodex is a provider gateway: the client runs the loop and tools. It appears only where it has something real (a bounded web-search loop, a stall deadline, Remote Workspace).
- "Ours" rows come only from `own-rizzy-harness.md`, which describes commit `a268048` on `main`. Branches `integration/gate3` and `feat/agent-roster` are mentioned only as "branch only".

## 2. Features

### Area 1. Agent loop and execution model

#### Loop structure, turns and stop reasons
What it is: how the harness defines one unit of work (turn, step, item) and how a run reports why it ended.
Variants: Codex, DeepSeek, OpenHands and Claude Code expose typed event units (item, step, ActionEvent, message types) that clients fold over. Zed/ACP defines the stop reasons as a protocol contract. Pi and OpenClaw add a "settled" or lifecycle signal because the visible end of a turn is not always the end of work. Warp bounds a conversation by shell commands and idle time rather than by the model.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Thread made of turns made of items (messages, command runs, file changes, tool calls); clients drive turns and stream item events through the app-server. | GA (app-server experimental) | https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| Claude Code | Loop of gather context, act, verify until the model answers without tool calls. A turn runs from a user message to the model finishing. The SDK stream has five message kinds and result subtypes `success`, `error_max_turns`, `error_max_budget_usd`, `error_during_execution`, `error_max_structured_output_retries`, plus `stop_reason`. | GA | https://code.claude.com/docs/en/how-claude-code-works.md, https://code.claude.com/docs/en/agent-sdk/agent-loop.md, https://code.claude.com/docs/en/glossary.md | unreviewed |
| Claude Desktop | Code tab runs the same engine as the CLI; nothing loop-specific is documented separately. | GA | https://code.claude.com/docs/en/platforms, https://claude.com/docs/cowork/overview.md | unreviewed |
| DeepSeek harness | A turn holds steps; a step is one model request plus its tool calls. Turns end with a recorded reason (completed, aborted, blocked, error, max-tokens, interrupted, forked). | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session | unreviewed |
| Pi | A turn is request, streamed reply, tool calls, results; more turns run while tool results or queued messages need another request. `agent_end` is not the end: `agent_settled` says nothing more will run automatically. | GA | https://pi.dev/docs/latest/how-pi-works, https://pi.dev/docs/latest/rpc, https://pi.dev/docs/latest/json, https://pi.dev/docs/latest/sdk | unreviewed |
| OpenHands | Stateless `Agent.step()`, atomic: run approved actions, optionally condense, query the model, emit ActionEvents or a MessageEvent (ends the step), check confirmation, execute, emit ObservationEvents. | GA | https://docs.openhands.dev/sdk/arch/agent.md, https://docs.openhands.dev/sdk/arch/conversation.md | unreviewed |
| OpenClaw | The `agent` RPC validates, persists metadata and returns a `runId` immediately; lifecycle phases start, finishing, end, error; `agent.wait` returns ok, error or timeout. | GA | https://docs.openclaw.ai/concepts/agent-loop | reviewed |
| goose | GDK models the loop as a state machine of ordered steps; the application owns persisted state so a fresh machine can resume; cancellation drops the running step and lets operations finalise interrupted tool responses. Release notes say the legacy loop was deleted in v1.55.0 and replaced in v1.58.0, so the docs may lag. | unclear | https://goose-docs.ai/docs/gdk/concepts/state-machine.md, https://goose-docs.ai/docs/gdk/index.md | unreviewed |
| Hermes Agent | One core class runs the loop (build or reuse system prompt, preflight compression, interruptible API call, tools, repeat) behind CLI, gateway, ACP server, API server and library entry points; three wire modes (chat completions, Responses-style, Anthropic Messages). | GA | https://hermes-agent.nousresearch.com/docs/developer-guide/architecture, https://hermes-agent.nousresearch.com/docs/developer-guide/agent-loop | corrected |
| Cline | `@cline/agents` is a browser-compatible stateless loop with `run`, `continue`, `abort`, `subscribe`, `restore`, `snapshot`; `@cline/core` adds sessions, tools and persistence. | unclear (no maturity label) | https://docs.cline.bot/sdk/architecture/overview.md, https://docs.cline.bot/sdk/overview.md | unreviewed |
| Zed/ACP | ACP prompt turn: `session/prompt` streams `session/update`, ends with stop reason `end_turn`, `max_tokens`, `max_turn_requests`, `refusal` or `cancelled`. Zed's native agent loop internals are not documented. | GA (ACP) | https://agentclientprotocol.com/protocol/v1/prompt-turn.md, https://zed.dev/docs/ai/zed-agent.md | unreviewed |
| Warp | Multi-step agent in a real terminal session. A new conversation starts after a shell command, after 3 hours idle, or manually; a context indicator shows above 20% usage; auto-summarise only once the window is exceeded. | GA | https://docs.warp.dev/agents/local-agents/interacting-with-agents/ | reviewed |
| OpenCodex | Not a loop: a translator with bounded exceptions (see step limits). | GA | https://opencodex.me/getting-started/how-it-works/ | reviewed |
| rizzy-harness (ours) | One native loop: build messages, stream a completion, run tool calls, repeat until a message has no tool calls. Turn ends are events; usage summed per turn into `turn_ended.usage`. | works | `crates/agent-native/src/lib.rs:542-877`, `crates/agent-native/src/llm.rs:246-253` | from own inventory |

#### Where the loop runs relative to the tools
What it is: which process owns the loop, which owns tool execution, and what the clients are.
Variants: opposite answers to our invariant. Cursor's cloud and self-hosted modes keep loop and inference in the vendor cloud and put only tool execution on the customer machine. Claude Code's self-hosted runner has the vendor control plane send each session's tool list and permission rules with the work. Cline's hub does not run the loop; spokes do. OpenCode, goose and Codex run the loop in a server on the machine with the code and treat UIs as clients. Zed/ACP lets the client execute fs and terminal operations on the agent's behalf.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | One app-server (JSON-RPC thread/turn/item API) used by TUI, IDE, desktop and SDKs; background daemon; remote TUI over WebSocket or UDS. | experimental (app-server) | https://learn.chatgpt.com/docs/app-server.md | reviewed |
| OpenCode | A background server owns sessions, plugins, permissions and state; TUI, web and desktop are clients; `--standalone` uses a private server; shared service registered in `~/.local/state/opencode/service.json`. | GA (V2) | https://opencode.ai/docs/troubleshooting/, https://opencode.ai/docs/cli/, https://opencode.ai/docs/cli/commands/ | reviewed |
| OpenCodex | Loop and tools run in the client; the gateway sees every prompt in transit. Remote Workspace (separate): the hub runs the agent, an executor on another machine supplies files and runs commands in bubblewrap over an outbound WebSocket. | experimental (Remote Workspace) | https://opencodex.me/getting-started/how-it-works/, https://opencodex.me/guides/remote-workspace/ | corrected |
| Claude Code | Always a `claude` process: on the user's machine, in a per-session Anthropic VM, or as a child of a customer-run self-hosted runner. Control plane and queueing are Anthropic-hosted in every mode. Per review: the control plane sends each session's tool list and permission rules with the work, and also model, connectors and lifecycle hooks. | GA (self-hosted beta) | https://code.claude.com/docs/en/how-claude-code-works.md, https://code.claude.com/docs/en/web-quickstart.md, https://code.claude.com/docs/en/self-hosted-environments.md, https://code.claude.com/docs/en/self-hosted-environments-configuration.md | corrected |
| Claude Desktop | A Code-tab session runs locally, on Anthropic cloud VMs, over SSH, or in WSL 2. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | Execution location is a swappable capability seam (changing the provider moves bash, PTY and LSP together). Whether the loop and web server share one process is an inference, not stated. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams | unverified |
| Cursor | Local agent loop runs in the Node process (SDK page); for Cloud Agents and Self-Hosted Machines the loop, inference and planning run in Cursor's cloud and only tool execution is placed on the VM or the customer worker, over one outbound HTTPS connection. | GA (cloud); self-hosted status not stated | https://cursor.com/docs/cloud-agent/self-hosted, https://cursor.com/blog/self-hosted-machines, https://cursor.com/docs/sdk/typescript | reviewed |
| Antigravity | One harness across App, CLI, IDE and SDK; where the local loop executes is not documented (a local `language_server.log` hints at a sidecar, inference only). | GA (App/CLI); SDK unclear | https://antigravity.google/docs/overview | unverified |
| OpenHands | `Conversation` becomes a `LocalConversation` (in-process) or `RemoteConversation` (execution on an Agent Server over HTTP/WebSocket) with one API; swapping the workspace argument switches modes. | GA | https://docs.openhands.dev/sdk/arch/conversation.md, https://docs.openhands.dev/sdk/guides/agent-server/overview.md | unreviewed |
| OpenClaw | Runtime is separate from provider and channel: built-in `openclaw`, a bundled `codex` plugin that runs the Codex app-server, opt-in `copilot` and `muse`, CLI backends, and ACP/acpx external harnesses. Once a harness starts, failures are not retried on another runtime. | GA (runtimes vary) | https://docs.openclaw.ai/concepts/agent-runtimes | unreviewed |
| goose | The loop and tools run inside `goose serve` / `goose acp` on the machine with the code; desktop, editors and custom UIs are ACP clients. | unclear | https://goose-docs.ai/docs/gdk/index.md | unreviewed |
| Hermes Agent | The agent process stays on the host; terminal and all file tools go through the selected environment's `execute()` (see execution backends). | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tools | reviewed |
| Cline | Per-machine hub daemon routes events and approvals and brokers capabilities but does not run the loop; spoke workers run `@cline/core` and the loop and outlive clients; clients attach over WebSocket (default `127.0.0.1:25463`). Docs say nothing about hub authentication. | GA as documented; `remote` mode unclear | https://docs.cline.bot/sdk/architecture/hub-spoke.md, https://docs.cline.bot/cli/cli-reference.md | corrected |
| Zed/ACP | Zed's agent runs in the editor process; external agents are spawned by the client over stdio. In ACP the client performs fs reads/writes and runs terminals for the agent and applies its own policy. | GA | https://zed.dev/docs/ai/agents.md, https://agentclientprotocol.com/protocol/v1/file-system.md, https://agentclientprotocol.com/protocol/v1/terminals.md | reviewed |
| Warp | Same agent in the app, in the `warp` CLI and as cloud runs. | GA | https://docs.warp.dev/agents/ | unreviewed |
| rizzy-harness (ours) | The agentic loop and tools run on the node that owns the session; the master's chat runtime has only web tools and `remember`. A panic ends the turn, not the node; a restart mid-turn closes the turn as `interrupted`. | works | `crates/cli/src/main.rs:255-259`, `crates/cli/src/main.rs:447-451`, `crates/node/src/session.rs:415-449` | from own inventory |

#### Step, turn and budget limits
What it is: hard caps on model calls, turns, spend or wall-clock time inside one run.
Variants: Claude Code and OpenCode cap per run but differ in what happens at the cap (error result versus forced text-only final step). goose and Hermes ask or pause. Most harnesses document no cap. Spend caps (not just call counts) exist only in the Claude Code SDK and Codex goal budgets. Ours is a fixed 50 with no cost cap.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | SDK `maxTurns` and `maxBudgetUsd`, both unlimited by default; budget includes subagent spend; checked after each response, so spend can overshoot by one response; at the cap spawning a subagent fails and background subagents stop. | GA | https://code.claude.com/docs/en/agent-sdk/agent-loop.md | unreviewed |
| Codex | Goals carry `tokenBudget`, `tokensUsed`, `timeUsedSeconds`; `goals.max_goal_token_budget` caps it. A separate whole-rollout token budget with reminder thresholds is experimental and off by default. | GA (goals); experimental (rollout budget) | https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | reviewed |
| OpenCode | Per-agent `steps` caps model steps; on the final step tools are removed and the model must answer in text. | GA | https://opencode.ai/docs/agents/ | reviewed |
| OpenCodex | Web-search sidecar loops up to `maxSearchesPerTurn` (default 3), then forces an answer; real client tools end the turn so the client receives them. | GA | https://opencodex.me/guides/sidecars/ | reviewed |
| OpenClaw | Per-attempt timeout default 48 h (0 removes the limit); model-idle timeout 120 s cloud, 300 s self-hosted; each fallback gets a fresh budget. | GA | https://docs.openclaw.ai/concepts/agent-loop | reviewed |
| goose | Max turns without user input, default 1000, then asks whether to continue (`GOOSE_MAX_TURNS`, desktop setting, `--max-turns`); CLI also lists `--max-tool-repetitions`, unexplained. | GA | https://goose-docs.ai/docs/guides/sessions/smart-context-management.md, https://goose-docs.ai/docs/guides/goose-cli-commands.md | reviewed |
| Hermes Agent | Iteration budget default 500 (`agent.max_turns`); subagents get their own so totals can exceed the parent cap. The agent-loop page says the subagent cap is 50, the delegation page says `delegation.max_iterations` is 250. | GA | https://hermes-agent.nousresearch.com/docs/developer-guide/agent-loop | corrected |
| Cursor | Docs state there is no limit on tool calls per task. | GA | https://cursor.com/docs/agent/overview | unreviewed |
| Cline | CLI `--retries` sets consecutive mistakes allowed before exit, default 3. | GA | https://docs.cline.bot/cli/cli-reference.md | reviewed |
| Zed/ACP | Protocol stop reason `max_turn_requests` exists; Zed's own limit is not documented. | GA (ACP) | https://agentclientprotocol.com/protocol/v1/prompt-turn.md | unreviewed |
| rizzy-harness (ours) | Hard cap of 50 model calls per turn, error "stopped after 50 steps", not configurable. No token or cost budget for the main agent (the `budget` agent field is parsed and never read). Sub-agent count and token caps exist. | works (cap); stub (`budget`) | `crates/agent-native/src/lib.rs:33`, `crates/agent-native/src/lib.rs:650`, `crates/core/src/agent.rs:34-40` | from own inventory |

#### Stuck-loop and stalled-run detection
What it is: automatic detection of a run that repeats itself or has stopped making progress.
Variants: OpenHands detects semantic repetition (same action, error loops, monologue, ping-pong). OpenClaw and Claude Code detect stalls in time (a quiet run or a quiet stream), not repetition. Nothing documents a repetition detector in the others.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenHands | On by default. Flags repeating action/observation (4+), repeating action/error (3+), monologue (3+ messages), ping-pong (6+ cycles) and repeated context-window errors, over events after the last user message. Exact halt behaviour is not stated. | GA | https://docs.openhands.dev/sdk/guides/agent-stuck-detector.md | reviewed |
| OpenClaw | A 2-minute threshold flags `session.long_running`/`stalled`/`stuck` only when diagnostics are enabled; the controller watchdog (semantic-stall threshold of at least 5 minutes and 3x the warning threshold) runs even with diagnostics off and requests Stop. | GA | https://docs.openclaw.ai/concepts/agent-loop | corrected |
| Claude Code | Four independent streaming watchdogs abort a quiet response so dead connections retry (first-byte deadline, event-level 300 s, byte-level, body idle). Separately `/goal` stops with a warning if Claude goes several turns with no tool use. | GA | https://code.claude.com/docs/en/network-config.md, https://code.claude.com/docs/en/goal.md | unreviewed |
| goose | CLI flag `--max-tool-repetitions` is listed but not explained. | GA | https://goose-docs.ai/docs/guides/goose-cli-commands.md | reviewed |
| OpenCode | The V2 permissions page says `doom_loop` is not a V2 action; the survey does not say what it did before. | unclear | https://opencode.ai/docs/permissions/ | unverified |
| rizzy-harness (ours) | Nothing in the inventory beyond the 50-step cap. | none | not in the inventory | from own inventory |

#### Interrupting a running turn
What it is: stopping the current model call and tools without losing the session.
Variants: who the stop is delivered to and what happens to in-flight work differ. Hermes abandons the API call and adds no partial response, and moves foreground terminal commands to the background rather than killing them. Claude Code's SDK returns a receipt that lists still-queued messages. ACP requires a specific `cancelled` answer. Ours kills the bash process group but cannot interrupt a non-bash tool that ignores cancel (fixed only on a branch).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Esc stops immediately and cancels the running tool call. SDK `interrupt()` resolves with a receipt listing `still_queued` message ids (`interrupt_receipt_v1`, v2.1.205+); a control request can cancel the queue instead (v2.1.219+); results carry `terminal_reason` `aborted_streaming` or `aborted_tools`. | GA | https://code.claude.com/docs/en/how-claude-code-works.md, https://code.claude.com/docs/en/agent-sdk/typescript.md | unreviewed |
| Codex | A mid-turn follow-up steers; an interrupt also stops active Code Mode cells (0.163.0). | GA | https://learn.chatgpt.com/docs/changelog | unreviewed |
| Oh My Pi | Esc interrupts; `.` or `c` continues the previous intent; Alt+R retries a failed turn. | GA | https://omp.sh/docs/using | unreviewed |
| Pi | Escape aborts and returns queued messages to the editor. | GA | https://pi.dev/docs/latest/usage | unreviewed |
| goose | Desktop: typing "stop" or "wait" keywords interrupts; CLI uses Ctrl+C. Cancellation drops the running step in the GDK. | GA | https://goose-docs.ai/docs/guides/sessions/in-session-actions.md | unreviewed |
| Hermes Agent | The API call runs in a background thread while the main thread watches for an interrupt (new message, `/stop`, signal); the abandoned call adds no partial response. `/stop` is a hard stop. | GA | https://hermes-agent.nousresearch.com/docs/developer-guide/agent-loop | unreviewed |
| OpenClaw | Queue mode `interrupt` aborts the active run and runs the newest message; the controller watchdog can request Stop. | GA | https://docs.openclaw.ai/concepts/queue | reviewed |
| Zed/ACP | `session/cancel`; the client must answer pending permission requests with `cancelled`; the agent must reply with the `cancelled` stop reason, not an error. | GA (ACP) | https://agentclientprotocol.com/protocol/v1/prompt-turn.md | unreviewed |
| DeepSeek harness | An `interrupt_agent` tool exists and `interrupted` is a recorded turn end reason. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| OpenHands | A normal message interrupts a `/goal` run; pause/resume guides exist but only as index titles. | GA (existence); details unclear | https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md | unreviewed |
| Cline | `abort` is part of the `@cline/agents` loop API. | unclear | https://docs.cline.bot/sdk/architecture/overview.md | unreviewed |
| rizzy-harness (ours) | Per-turn `CancellationToken`; bash kills its process group (SIGTERM, 2 s, SIGKILL). The loop does not abandon a running tool future, so a non-bash tool that ignores cancel can block the turn. | partial | `crates/node/src/session.rs:865-869`, `crates/agent-native/src/lib.rs:1145-1163`, `crates/tools/src/bash.rs:176-203` | from own inventory |

#### Steering and message queueing while a turn runs
What it is: accepting user input during a running turn, either injected into the turn (steer) or held for later (queue).
Variants: the default differs. Claude Code, Cursor and Zed queue by default and treat steering as an explicit action. OpenClaw and OpenCode steer by default. Hermes and OpenClaw make the mode a setting, and OpenClaw adds coalescing (`collect`) with a cap and overflow summary. Claude Code's SDK adds a per-message `priority` (`next`, `later`, `now`). Zed's steer works only for its own agent, not external ACP agents. Ours rejects the prompt.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Enter while working queues the message; it is read as soon as current tool calls finish, within the same turn. `Ctrl+Enter` sends now (backgrounds backgroundable work, otherwise interrupts). SDK messages carry `priority` `next` (default), `later`, `now`, `shouldQuery: false`. `/model` applies next request, `/effort` next message, `/fast` next turn. | GA | https://code.claude.com/docs/en/interactive-mode.md, https://code.claude.com/docs/en/agent-sdk/typescript.md | unreviewed |
| Codex | `turn/steer` appends input to the in-flight turn; a slash command can be queued with Tab; `codex queue --thread <id> --message ...` queues text for an existing session, with `--resume` (0.166.0). | GA | https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/changelog | reviewed |
| OpenCode | Enter while working steers; Ctrl+X then Enter queues; the API has an inbox of queued inputs that can be reprioritised or cancelled. | GA | https://opencode.ai/docs/cli/tui/, https://opencode.ai/docs/api/ | unreviewed |
| Oh My Pi | Enter steers at a safe step boundary; Ctrl+Q or Ctrl+Enter queues a follow-up; queued messages can be pulled back; one-at-a-time by default. | GA | https://omp.sh/docs/using, https://omp.sh/docs/settings | unreviewed |
| Pi | Enter sends a steering message (after the current assistant turn); Alt+Enter queues a follow-up; Alt+Up pulls queued messages back; `steeringMode`/`followUpMode` default one-at-a-time (alternative `all`). | GA | https://pi.dev/docs/latest/usage, https://pi.dev/docs/latest/settings, https://github.com/earendil-works/pi/blob/main/packages/agent/README.md | unreviewed |
| Cursor | Messages sent while the agent runs are queued by default; a "steer" option injects at the next tool call; Cmd+Enter appends to the latest user message. | GA | https://cursor.com/docs/agent/overview | unreviewed |
| Zed/ACP | Messages are queued by default; a queued message can be toggled to Steer (interrupt the Zed Agent at its next step) or sent now; items are editable. Steer is not available for external agents. | GA | https://zed.dev/docs/ai/agent-panel | reviewed |
| Warp | Follow-up prompts queue while the agent responds (`/queue`); sent one at a time after each successful response; an error pauses the queue without discarding it; cleared on restart. | GA | https://docs.warp.dev/agents/local-agents/interacting-with-agents/prompt-queueing/ | unreviewed |
| goose | Desktop queues messages typed while working (editable, reorderable); send-now interrupts. | GA | https://goose-docs.ai/docs/guides/sessions/in-session-actions.md | unreviewed |
| Hermes Agent | Busy-input modes: interrupt (default; foreground terminal commands move to the background), queue as a later turn, or steer (injected after the next tool call); `/queue` has list/remove/edit/move/clear and the queue lives on the gateway and survives restarts. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/messaging, https://hermes-agent.nousresearch.com/docs/user-guide/cli | unreviewed |
| OpenClaw | Queue modes `steer` (default), `followup`, `collect` (coalesce after a quiet period), `interrupt`; 500 ms debounce, cap 20 per session, overflow `summarize`; accepted input is stored before the Gateway acks. | GA | https://docs.openclaw.ai/concepts/queue | reviewed |
| DeepSeek harness | A pre-step hook can intercept or alter a step; steering messages and injected context pass the same waterfall and are claimed at the next step boundary. Exact signatures were not read. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/agent-lifecycle | unreviewed |
| OpenHands | A "send message while running" guide exists but only as an index title; mechanism not read. | unclear | https://docs.openhands.dev/llms.txt | unverified |
| rizzy-harness (ours) | None: a prompt while a turn runs is rejected with "a turn is already running". | none | `crates/node/src/session.rs:297-299` | from own inventory |

#### Parallel tool calls
What it is: running several tool calls from one model response at the same time.
Variants: opt-in by tool hint (Claude Code, `readOnlyHint`), opt-out by tool flag (Pi, `executionMode: "sequential"`), a thread pool with order-restored results (Hermes), or a numeric limit defaulting to 1 (OpenHands, experimental). Codex documents only read-only parallelism. Ours is sequential except `delegate`.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Read-only tools (Read, Glob, Grep, MCP tools marked read-only) may run concurrently; mutating tools (Edit, Write, Bash) run sequentially; custom tools are sequential unless they set `readOnlyHint`. | GA | https://code.claude.com/docs/en/agent-sdk/agent-loop.md | unreviewed |
| Codex | Independent read-only tools, including skill, memory and conversation-history lookups, run in parallel (0.163.0). | GA | https://learn.chatgpt.com/docs/changelog | unreviewed |
| Pi | Tool calls from one assistant message run in parallel by default; a tool can opt out with `executionMode: "sequential"` (one sequential tool forces its batch sequential); `terminate: true` skips the follow-up model call only if every result in the batch sets it. | GA | https://pi.dev/docs/latest/extensions, https://github.com/earendil-works/pi/blob/main/packages/agent/README.md | unreviewed |
| Hermes Agent | One call runs on the main thread; several run in a thread pool with results re-ordered to call order; interactive tools are always sequential; MCP tools are sequential unless a server opts in. | GA | https://hermes-agent.nousresearch.com/docs/developer-guide/agent-loop | unreviewed |
| OpenHands | `tool_concurrency_limit` on `Agent`, default 1 (sequential); no built-in locking; suggested 2-8; the page warns of race conditions. | experimental | https://docs.openhands.dev/sdk/guides/parallel-tool-execution.md | unreviewed |
| DeepSeek harness | The tool pipeline page does not cover concurrency. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-execution-pipeline | unreviewed |
| rizzy-harness (ours) | Tool calls in a step run one after another in order; only `delegate` calls in the same step run in parallel. | works (sequential) | `crates/agent-native/src/lib.rs:744-870`, `crates/agent-native/src/lib.rs:796-801` | from own inventory |

#### Goal and objective-continuation loops
What it is: a persisted objective that keeps the agent working across turns until a completion check passes.
Variants: who judges completion is the main difference. Claude Code and Hermes use a small or auxiliary model that sees the transcript. OpenHands uses a separate judge LLM with a round cap. Antigravity uses a stop hook and the model's own evidence mapping (no independent check). Codex and Oh My Pi attach token budgets. Hermes can require shell-command gates to pass first. Caps differ: OpenHands 10 rounds, Hermes 20 continuation turns, Antigravity no cap.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/goal <condition>` (up to 4,000 chars) starts a turn; after each turn a small fast model returns met, not yet or impossible; built as a session-scoped prompt Stop hook; evaluator calls no tools; transient errors retry 3 times then pause; check-ins after 30 min then 1 h then every 2 h; `claude -p "/goal ..."` runs the loop headless. | GA | https://code.claude.com/docs/en/goal.md | unreviewed |
| Codex | `/goal` attaches a persistent objective (4,000 chars max), pausable and editable; app-server `thread/goal/set`, `thread/goal/get`, `thread/goal/clear` with token budget and time used; `features.goals` "persisted goals and automatic continuation". | GA | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | reviewed |
| OpenCode | `/goal` keeps working until a reviewer confirms the objective; `pause`, `resume`, `clear`. Appears only in the changelog; how the reviewer works is not documented. | unclear | https://opencode.ai/changelog | reviewed |
| Oh My Pi | One durable objective with automatic continuation until verified, paused, dropped or token budget hit; survives compaction; cold restart restores it as paused; `/guided-goal` interviews first; automatic continuation default only in interactive mode. | GA | https://omp.sh/docs/goal | reviewed |
| Cursor | `/goal [objective]` in the CLI; the page says "rolling out". | experimental | https://cursor.com/docs/cli/reference/slash-commands, https://cursor.com/docs/agent/overview | unreviewed |
| Antigravity | `/goal` installs a stop hook that blocks ending the turn until each deliverable is mapped to evidence; one active goal; no iteration cap (limit removed in v1.0.14); waits for running subagents. Completion is the model's own declaration. | GA | https://antigravity.google/docs/commands/goal | reviewed |
| OpenHands | `/goal` runs until a separate judge LLM audits the transcript and says complete, or a round cap (default 10 in the SDK); invalid judge JSON counts as not complete. Separate experimental Critic scores the work and sends the agent back below a threshold (default 0.6, up to 3 retries). | GA (goal); experimental (critic) | https://docs.openhands.dev/sdk/guides/convo-goal.md, https://docs.openhands.dev/openhands/usage/agent-canvas/critic.md | reviewed |
| Hermes Agent | `/goal` after each turn asks an auxiliary judge (sees the goal and last ~4 KB of response) for done, continue, blocked or wait; default 20 continuation turns then auto-pause; optional shell-command quality gates (default 3 retries, 5 min timeout); judge errors fail open to continue. Continuations are plain user messages so the prompt cache is kept. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/goals | reviewed |
| DeepSeek harness | Goal tools `create_goal`, `get_goal`, `update_goal`; the survey's distinctive list adds revision compare-and-set and a human-authorised resume. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| OpenClaw | `get_goal`, `create_goal`, `update_goal` appear in the built-in tool list; no mechanism described. | GA (tool names only) | https://docs.openclaw.ai/tools | unreviewed |
| rizzy-harness (ours) | Not in the inventory (no goal or continuation mechanism; only routines). | none | not in the inventory | from own inventory |

#### Time-driven re-submission loops
What it is: re-running a prompt in the same session on a timer or after each turn, independent of an objective check.
Variants: Claude Code self-paces through a model-called wakeup; Oh My Pi re-submits after every turn with three context modes; Hermes backs off while replies are unchanged.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/loop` self-paced: `ScheduleWakeup` picks the next run 1 min to 1 h out or `stop: true`; `CronCreate` schedules recurring or one-shot prompts inside the session, restored on resume if unexpired. | GA | https://code.claude.com/docs/en/tools-reference.md, https://code.claude.com/docs/en/goal.md | unreviewed |
| Oh My Pi | `/loop` re-submits after each turn, unbounded, N times, or until a deadline; `loop.mode` `prompt`, `compact` (compact before each replay) or `reset` (fresh session each replay). | GA | https://omp.sh/docs/modes | reviewed |
| Hermes Agent | `/loop` on a fixed or self-paced interval (1 min floor backing off to 15 min while replies are unchanged) with `LOOP_COMPLETE`, `--times`, `--until` (judged), `loops.max_ticks` default 100; `/heartbeat` attaches one recurring idle-only instruction (60 s minimum) restored after gateway restart. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/loops, https://hermes-agent.nousresearch.com/docs/user-guide/features/heartbeat | unreviewed |
| rizzy-harness (ours) | Not in the inventory (routines exist but are separate runs, not a loop in a session). | none | not in the inventory | from own inventory |

#### Retry, backoff and stream-failure handling
What it is: what the loop does when the provider fails or a stream stalls.
Variants: Claude Code is the most elaborate (typed failure classes, unattended watchdog, mid-stream continuation). Pi separates agent-level retry (on by default) from provider-level (0). Codex honours the server's `Retry-After` and stops when the model is not found. Ours has fallback models only.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Transient failures retried up to 10 times with exponential backoff (`CLAUDE_CODE_MAX_RETRIES`, cap 15); `CLAUDE_CODE_RETRY_WATCHDOG=1` retries 429/529 indefinitely for unattended runs; in non-interactive sessions a response cut off mid-stream with text and no tool calls is continued; a malformed tool call is dropped from the retry context. | GA | https://code.claude.com/docs/en/errors.md, https://code.claude.com/docs/en/whats-new/2026-w35.md, https://code.claude.com/docs/en/changelog.md | unreviewed |
| Codex | Retries honour `Retry-After`; a WebSocket transport falls back to HTTP; no more retries when a provider reports the model was not found. | GA | https://learn.chatgpt.com/docs/changelog | unreviewed |
| Pi | Agent-level retry on by default: 3 retries, 2000 ms base delay, 60000 ms max; provider-level retries default 0; http idle timeout 300000 ms. | GA | https://pi.dev/docs/latest/settings | reviewed |
| Cline | Model-request retries reduced from 5 to 3 in v4.1.26; mistake limit separate (see step limits). | GA | https://github.com/cline/cline/releases | reviewed |
| OpenClaw | Per-attempt and idle timeouts (see step limits); a malformed tool call can trigger one continuation; an empty required reply after tools can trigger a tool-free finalisation pass. | GA | https://docs.openclaw.ai/concepts/agent-loop | unreviewed |
| OpenCodex | 300 s upstream stall deadline for public upstreams. | GA | https://opencodex.me/reference/architecture/ | reviewed |
| rizzy-harness (ours) | No retry or backoff. A model failure before any output moves to the next fallback model; otherwise the turn errors. | none (fallback only) | `crates/agent-native/src/lib.rs:693-701`, `crates/agent-native/src/provider.rs:104-116` | from own inventory |

#### Multi-model turn strategies (handoff, mixture, forcing)
What it is: using more than one model inside a single run or turn.
Variants: Oh My Pi switches models once mid-session. Hermes runs reference models without tools and lets one aggregator act. Ours runs up to four models in parallel for the user to pick from, and falls back along a chain.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Oh My Pi | Prewalk: a strong model investigates, writes todos and makes the first edit, then the session switches once to a cheaper model (default `smol`); fires once. `/force <tool>` makes the next call invoke one named tool and ends the turn. | GA | https://omp.sh/docs/prewalk, https://omp.sh/docs/modes | reviewed |
| Hermes Agent | Mixture of Agents presets appear as models under a virtual `moa` provider: reference models run without tool schemas and give text guidance to an aggregator that writes the response and emits tool calls; `fanout` `user_turn` (default since July 2026), `per_iteration` or `every_n`. Benchmark numbers are the vendor's own. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/mixture-of-agents | unreviewed |
| rizzy-harness (ours) | Parallel answers from up to 4 models with a pick, and fallback chains with a visible note. | works | `crates/agent-native/src/lib.rs:930-1030`, `crates/agent-native/src/lib.rs:503-534` | from own inventory |

#### Event log as the loop's source of truth, one writer per session
What it is: the session is an append-only log the model context and UI are derived from, with a defined writer.
Variants: OpenClaw states the single-writer rule most explicitly (durable `activeWriterRunId` claim checked at commit). DeepSeek states the log is the single source of truth and anything the model sees must be logged. OpenHands is event-sourced but states no single-writer rule. OpenCode keeps a durable log with sequence numbers separate from a volatile broadcast stream.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenClaw | Turns serialised per session before global admission; each admitted run records an active-writer claim before streaming and the commit transaction verifies it still matches, so a superseded run cannot commit stale data. The state-directory lock is not on the queue page. | GA | https://docs.openclaw.ai/concepts/agent-loop, https://docs.openclaw.ai/concepts/queue | corrected |
| DeepSeek harness | Append-only typed event log; model context is derived from it; durable session events are separate from live `agent/*` events; streaming chunks are transient so a hard crash loses un-settled streams; persistence is single-writer with checksummed frames. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/agent-lifecycle, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/session | reviewed |
| OpenHands | `EventLog` described as an immutable append-only store; state updates run under a FIFO lock; docs state no single-writer rule. | GA | https://docs.openhands.dev/sdk/arch/conversation.md | unreviewed |
| OpenCode | Server-side durable per-session event log with sequence numbers and `follow` tailing, separate from the volatile broadcast stream. | GA (API pages call the surface experimental) | https://opencode.ai/docs/api/ | reviewed |
| goose | Application owns persisted state; a fresh state machine can resume a saved conversation; no exactly-once for external actions. | unclear | https://goose-docs.ai/docs/gdk/concepts/state-machine.md | unreviewed |
| Claude Code | Append-only local JSONL transcript recording the system prompt, tool definitions and request parameters. | GA | https://code.claude.com/docs/en/sessions.md | unreviewed |
| Pi | Session JSONL tree with in-file branching (`/tree`) and an append-only `context_edit` entry. | GA | https://pi.dev/docs/latest/session-format | unreviewed |
| Oh My Pi | Session tree inside one JSONL file (id/parentId) with documented on-disk schema. | GA | https://omp.sh/docs/session-format | unreviewed |
| rizzy-harness (ours) | One turn at a time per session; one writer appends events with contiguous `seq`. | works | `crates/node/src/session.rs:36`, `crates/node/src/session.rs:194-230` | from own inventory |

#### Harness-injected context and message provenance
What it is: content the harness adds to the conversation, and whether the model or SDK can tell it from user text.
Variants: only Claude Code documents provenance typing (and states which fields are forgeable). DeepSeek requires injected content to be logged.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | The harness adds CLAUDE.md, output-style text, file-changed notes and attribution lines as system reminders. SDK user messages carry `origin` (`human`, `channel`, `peer`, `task-notification`, `coordinator`, `auto-continuation`, `unclassified`); apps may set only `human` or a declared scheduled run; non-user text is sent as `isSynthetic`. | GA | https://code.claude.com/docs/en/how-claude-code-works.md, https://code.claude.com/docs/en/agent-sdk/typescript.md | unreviewed |
| DeepSeek harness | Dynamic runtime contexts are logged as a user-role message when changed; anything the model sees must be in the log. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/system-prompt | unreviewed |
| rizzy-harness (ours) | Per-turn context (memory, project brief) is appended after the history to keep the prefix byte-stable; no provenance tagging in the inventory. | partial | `crates/agent-native/src/lib.rs:600-608` | from own inventory |

#### Evaluator outside the main loop (asynchronous monitor, critic)
What it is: a second process judging the run's safety or success without being the approver of each action.
Variants: Codex's monitor can pause a task after the triggering activity and, on CLI and mobile, simply ends it. OpenHands' critic scores success and can loop the agent back.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | With GPT-6 Astra a monitor runs asynchronously and can pause a task on potentially unsafe behaviour, possibly after the activity; on CLI, mobile, ZDR or non-US residency the task simply ends. | GA | https://learn.chatgpt.com/docs/agent-approvals-security.md | reviewed |
| OpenHands | Critic model scores the likelihood the work succeeded on message or finish; with iterative refinement a score below threshold sends the agent back (default 0.6, up to 3). Hosted critic is free "currently". | experimental | https://docs.openhands.dev/openhands/usage/agent-canvas/critic.md | unreviewed |
| rizzy-harness (ours) | Not in the inventory (judge-model approval is docs-only). | none | not in the inventory | from own inventory |

#### Tool-call execution pipeline
What it is: the ordered stages a tool call passes between the model's request and the result.
Variants: DeepSeek has the most explicit pipeline (hooks, approval, monotonic deny-only guards, execution with timeouts, post-processing, lossless snapshot). Hermes intercepts some tools before the registry. OpenHands puts a confirmation check between parse and execute.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| DeepSeek harness | `tools/pre-execute` waterfall (hooks, approval policy), approval resolution, monotonic guards (deny only), `tools/execute` (timeouts wrap here), `tools/post-execute`, content projection, lossless snapshot, `finalizeContent`, observe-only `tools/result`. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-execution-pipeline | unreviewed |
| Hermes Agent | Per call: handler, pre-tool hook, dangerous-command check, execute, post-tool hook; `todo`, `memory`, `session_search` and `delegate_task` are intercepted before the registry. | GA | https://hermes-agent.nousresearch.com/docs/developer-guide/agent-loop | unreviewed |
| Pi | A tool result with `terminate: true` skips the follow-up model call (see parallel calls); extensions can override built-in tools by name. | GA | https://pi.dev/docs/latest/extensions | unreviewed |
| OpenHands | Tools follow Action/Observation/Executor; confirmation policy and risk analyzers sit before execution. | GA | https://docs.openhands.dev/sdk/arch/tool-system.md | unreviewed |
| rizzy-harness (ours) | A mutating call emits `approval_requested` and blocks on a oneshot until the UI answers; every decision is an event. | works | `crates/node/src/session.rs:955-1012` | from own inventory |

#### Deliberate turn-ending semantics (forced answer, silent finish, finish tools)
What it is: ways a turn ends by design rather than by the model simply replying.
Variants: OpenCode removes tools at the step cap to force text. OpenClaw allows a silent finish. Claude Code can end the whole session. OpenHands and Cline have explicit finish tools.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | On the final allowed step tools are removed and the model must answer in text. | GA | https://opencode.ai/docs/agents/ | reviewed |
| OpenClaw | A token-only `NO_REPLY` is a valid silent finish for external turns; an empty required reply after tools can trigger a tool-free finalisation pass. | GA | https://docs.openclaw.ai/concepts/agent-loop | unreviewed |
| Claude Code | `EndConversation` ends and locks the session after sustained abuse following a warning; only on Opus 4.8, Sonnet 5, Fable 5 or later, interactive terminal sessions, direct API only; cannot be removed by deny rules while any other tool remains. | GA | https://code.claude.com/docs/en/tools-reference.md | unreviewed |
| Pi | `terminate: true` on every result in a batch skips the follow-up model call. | GA | https://pi.dev/docs/latest/extensions | unreviewed |
| OpenHands | `finish` and `think` are named built-in tools. | GA | https://docs.openhands.dev/sdk/arch/tool-system.md | unreviewed |
| Cline | `submit_and_exit` is in the built-in tool list. | GA | https://docs.cline.bot/tools-reference/all-cline-tools.md | reviewed |
| DeepSeek harness | `present` declares final deliverable files (max 4 per call). | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | reviewed |
| rizzy-harness (ours) | The turn ends when a message has no tool calls or the 50-step cap errors; no finish tool. | none | `crates/agent-native/src/lib.rs:872-876` | from own inventory |

### Area 2. Built-in tools

#### Built-in tool inventory (comparison)
What it is: which tools ship with the agent, by category. Only Claude Code, OpenCode, Oh My Pi, Pi, Zed, OpenClaw, Cline and Hermes publish a tool list; Codex, Antigravity, OpenHands, goose, DeepSeek (generated catalogue, not default-mounted) and Warp give partial or no list.
Variants: tool granularity differs widely (Claude Code splits Read, Glob, Grep, Edit, Write, NotebookEdit; Codex has `exec_command` plus `apply_patch`; Oh My Pi folds many capabilities into URL schemes read by `read`). Whether a tool is present by default also differs (see tool-set composition). The category table below uses `Y` documented tool, `B` partial, named only in examples, or opt-in, `?` implied but unconfirmed, `-` nothing documented in the survey. It is the survey's evidence, not a statement that the capability is absent.

Category table (columns: Codex, OpenCodex, OpenCode, Claude Code, Claude Desktop, DeepSeek, Oh My Pi, Pi, Cursor, Antigravity, OpenHands, OpenClaw, goose, Hermes, Zed/ACP, Warp, Cline, Ours):

| Category | Codex | OpenCodex | OpenCode | Claude Code | Claude Desktop | DeepSeek | Oh My Pi | Pi | Cursor | Antigravity | OpenHands | OpenClaw | goose | Hermes | Zed/ACP | Warp | Cline | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Read file | - | - | Y | Y | - | B | Y | Y | Y | B | Y | Y | ? | Y | Y | ? | Y | Y |
| Glob / grep search | - | - | Y | Y | - | B | Y | B | Y | B | Y | - | ? | Y | Y | ? | Y | Y |
| Exact-replace edit | - | - | Y | Y | - | B | - | ? | ? | B | Y | ? | ? | Y | Y | ? | ? | Y |
| Patch-format edit | Y | - | Y | - | - | - | - | - | - | - | ? | Y | - | - | - | - | Y | - |
| Write / create file | - | - | Y | Y | - | B | Y | Y | Y | ? | Y | Y | ? | Y | Y | ? | Y | Y |
| Shell command | Y | - | Y | Y | - | B | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y |
| Background-process control | Y | - | Y | Y | - | B | Y | - | ? | B | B | Y | - | Y | - | B | B | - |
| Web fetch | - | - | Y | Y | - | B | Y | - | - | ? | - | Y | - | Y | Y | - | Y | B |
| Web search | Y | Y | Y | Y | - | B | Y | - | Y | ? | - | Y | - | Y | B | Y | - | B |
| Ask-the-user tool | B | - | Y | Y | - | B | Y | - | Y | - | - | Y | - | Y | - | Y | Y | - |
| Todo / task list | Y | - | B | Y | - | B | Y | - | - | B | Y | - | B | Y | - | Y | - | - |
| Sub-agent spawn | Y | - | Y | Y | - | B | Y | - | Y | B | Y | Y | Y | Y | Y | ? | Y | Y |
| Code intelligence (LSP/AST/outline) | - | - | - | Y | - | B | Y | - | - | - | - | - | Y | - | B | - | - | Y |
| Browser | - | - | Y | - | Y | B | Y | - | Y | B | Y | Y | - | Y | - | - | ? | - |
| Notebook edit | - | - | - | Y | - | - | Y | - | - | - | - | - | - | - | - | - | - | - |
| Image view | Y | - | Y | Y | - | B | Y | - | Y | ? | - | Y | - | Y | - | - | - | - |
| Image generation | - | - | B | - | - | - | B | - | Y | ? | - | Y | - | Y | - | - | - | - |
| MCP resource tools | - | - | Y | Y | - | B | - | - | - | - | - | - | - | - | - | - | - | - |
| Tool search / deferred | Y | - | - | Y | - | - | Y | Y | - | Y | - | Y | B | Y | - | - | - | - |
| Model-written script tool | B | - | Y | - | - | B | B | B | - | - | - | - | B | Y | - | - | - | - |
| Schedule tool | - | - | - | Y | - | B | - | - | - | B | - | Y | - | Y | - | - | - | - |
| Memory tools | ? | - | - | - | - | - | B | - | - | - | B | ? | Y | Y | - | - | - | B |

Notes on the table: Claude Code's Glob and Grep are absent by default on macOS, Linux and WSL (the model uses `find` and `grep` through Bash) and on by default on Windows; Claude Code's browser capability is a separate extension surveyed under another area. DeepSeek cells are `B` because the whole product is a preview and the catalogue lists tools that are alternate packages or opt-in. Claude Desktop cells are `-` because its survey documents nothing separate from Claude Code.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | No tools reference page. Names come from hooks, config and app-server pages: `exec_command`/`write_stdin`, `apply_patch`, `view_image`, `update_plan`, `spawn_agent`/`send_input`/`resume_agent`/`wait_agent`/`close_agent`, `request_permissions`, `tool/requestUserInput` (experimental), hosted web search, tool search, managed worktree tools. | GA (partial list) | https://learn.chatgpt.com/docs/hooks.md, https://learn.chatgpt.com/docs/config-file/config-reference.md, https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| OpenCode | `read`, `glob`, `grep`, `edit`, `write`, `patch` (supported GPT models only), `shell`, `webfetch`, `websearch`, `question`, `skill`, `subagent`, `execute` (Code Mode), experimental `todowrite` and `experimental_image_generate`, MCP resource tools. Since v2.2.5 a call to a tool named `bash` runs as `shell`. | GA | https://opencode.ai/docs/tools/, https://opencode.ai/docs/mcp-servers/, https://opencode.ai/changelog | reviewed |
| OpenCodex | Ships no coding tools; the only synthetic tool is `web_search`; the client's tools (shell, `apply_patch`, MCP namespaces, `tool_search`) are relayed, and the docs claim they round-trip. Integer-typed arguments are repaired against the tool schema. | GA | https://opencodex.me/getting-started/how-it-works/, https://opencodex.me/reference/adapters/ | unreviewed |
| Claude Code | Prompt-required: Artifact, Bash, Edit, EnterWorktree, ExitPlanMode, Monitor, NotebookEdit, PowerShell, ShareOnboardingGuide, Skill, WebFetch, WebSearch, Workflow, Write. No prompt: Agent, AskUserQuestion, Cron tools, EndConversation, EnterPlanMode, Glob, Grep, ListAgents, LSP, MCP resource tools, PushNotification, Read, RemoteTrigger, ReportFindings, ScheduleWakeup, SendFeedback, SendMessage, SendUserFile, SubagentHandback, Task tools, TodoWrite, ToolSearch, WaitForMcpServers. `TaskOutput` was removed in v2.1.277 (Claude now reads the output file with Read). | GA | https://code.claude.com/docs/en/tools-reference.md | reviewed |
| Claude Desktop | Nothing desktop-specific beyond Claude Code's tools; adds routing (connector, then Bash, then Chrome, then iOS Simulator, then computer use). Cowork reads and writes local files and produces Office-style deliverables. | GA | https://code.claude.com/docs/en/desktop, https://claude.com/docs/cowork/overview.md | unreviewed |
| DeepSeek harness | Catalogue: `bash`, `pwsh`, `read`, `write`, `edit`, `read_image`, `str_replace_editor`, `glob` (cap 100), `grep` (ripgrep, cap 250), `lsp`, `run_code`, `ask_user_question`, `exit_plan_mode`, `present` (max 4 files), `skill`, `todo_write`, `web_search`, `web_fetch`, MCP resource tools, `schedule_*`, goal tools, `subagent`/`subagent_fork`, `send_message`, `interrupt_agent`, `job_*`, `ralph`, `workflow`; opt-in groups `terminal_*` PTY (6), `stagehand_*` browser (6), session-history tools, agent teams, `create_worktree`. A tool in the catalogue is not necessarily mounted. | preview; opt-in groups experimental | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | reviewed |
| Oh My Pi | Explore: `read`, `glob`, `grep`, `ast_grep` (off), `lsp`, `inspect_image` (auto), `web_search`, `github` (off). Change/run: `edit`, `write`, `ast_edit`, `bash`, `eval`, `debug` (DAP), `browser`, `computer` (off), `security_scan` (off), `generate_image` (off), `tts` (off). Coordinate: `ask`, `todo`, `task`, `hub`, `checkpoint`/`rewind` (off). Memory tools off. Each tool is classed Read, Write or Exec for approvals. README says 31 tools; the docs group them differently. | GA | https://omp.sh/docs/tools | reviewed |
| Pi | Built-ins `read`, `bash`, `powershell` (Windows), `edit`, `write`, `grep`, `find`, `ls`; default active set is `read`, `bash`, `edit`, `write`. No per-tool schemas or behaviour in the pages read. | GA | https://pi.dev/docs/latest/cli, https://pi.dev/docs/latest/settings, https://mariozechner.at/posts/2025-11-30-pi-coding-agent/ | reviewed |
| Cursor | Search files/folders, web search, fetch rules, read files, edit files, run shell, browser, image generation, ask-questions tool, canvas, Task tool for subagents. Hook matcher names: `Shell`, `Read`, `Write`, `Grep`, `Delete`, `Task`, `MCP:<tool>`. | GA | https://cursor.com/docs/agent/overview, https://cursor.com/docs/hooks | unreviewed |
| Antigravity | No tool reference. Names seen in docs and examples: `view_file`, `replace_file_content`, `grep_search`, `run_command`, `invoke_subagent`, `define_subagent`, `call_mcp_tool`, `evaluate_script` (browser), `schedule`, `manage_task`, `manage_inbox`; changelog v1.2.7 retired legacy search tools from the default toolset. | unclear | https://antigravity.google/docs/subagents, https://antigravity.google/docs/commands/browser, https://antigravity.google/docs/sdk/policies | unreviewed |
| OpenHands | `finish`, `think`, `execute_bash`, `file_editor`, `glob`, `grep`, `task_tracker`, a browser tool set and the task tool; tools follow Action/Observation/Executor with MCP-style annotations. Docs do not state which tools an agent gets by default. A GPT-5 preset using `ApplyPatchTool` is mentioned only in the index. | GA | https://docs.openhands.dev/sdk/arch/tool-system.md, https://docs.openhands.dev/sdk/guides/task-tool-set.md | unreviewed |
| OpenClaw | `exec`, `process`, `terminal`, `code_execution` (provider-backed Python); `read`, `write`, `edit`, `apply_patch`; `ask_user`, `secrets`; `web_search`, `x_search`, `web_fetch`; `browser`, `message`, `sessions_*`, goal tools, `automations`, `gateway`, `nodes`, media tools; `computer` separately. | GA (varies per tool) | https://docs.openclaw.ai/tools | unreviewed |
| goose | Tools come from extensions: built-in Developer (default on), Computer Controller, Memory, Tutorial, Auto Visualiser; platform extensions Analyze, Apps, Chat Recall, Code Mode, Extension Manager, Skills, Summon, Todo, Top of Mind. Individual Developer tool names are not listed (hook docs name `shell` and `edit`). | GA | https://goose-docs.ai/docs/getting-started/using-extensions.md | unreviewed |
| Hermes Agent | Registry of 70+ tools in about 28 toolsets: `terminal`, `process_manage`, `read_file`, `write_file`, `patch`, `search_files`, `web_search`, `web_extract`, browser tools, `computer_use`, `execute_code`, `delegate_task`, `memory`, `session_search`, `skill_*`, `cronjob_manage`, `todo_list`, `clarify`, vision/image/TTS, 14 `kanban_*` tools. | GA | https://hermes-agent.nousresearch.com/docs/reference/tools-reference | unreviewed |
| Zed/ACP | `diagnostics`, `fetch`, `find_path`, `grep`, `list_directory`, `read_file`, `search_web` (Zed Pro on the Zed provider only), `copy_path`, `create_directory`, `delete_path`, `edit_file`, `move_path`, `write_file`, `terminal`, `skill`, `spawn_agent`. | GA | https://zed.dev/docs/ai/tools.md | reviewed |
| Warp | No tool reference. Capability areas: slash commands, skills, planning, task lists, rules, full terminal use, computer use, MCP, codebase context, profiles, web search; permission types hint at apply diffs, read files, create plans, execute commands, interact with running commands, ask questions. | unclear | https://docs.warp.dev/agents/capabilities/, https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ | unreviewed |
| Cline | `read_files`, `search_codebase`, `run_commands`, `fetch_web_content`, `apply_patch`, `editor`, `skills`, `ask_question`, `submit_and_exit`, plus `spawn_agent`, `teams`, MCP and plugin tools. Docs also mention legacy names (`write_to_file`, `read_file`). | GA | https://docs.cline.bot/tools-reference/all-cline-tools.md | reviewed |
| rizzy-harness (ours) | Agentic nodes: `bash`, `read`, `write`, `edit`, `grep`, `glob`, `tool_output`, `outline`, `publish`, optional `graph_query`, synthesised `delegate`. Master chat runtime: `web_search`, `web_fetch`, `remember`. Chats have no file or shell tools; agentic sessions have no web or memory tools. | works | `crates/tools/src/lib.rs:385-396`, `crates/cli/src/main.rs:255-259`, `crates/cli/src/main.rs:447-451` | from own inventory |

#### Tool-set composition: defaults, presets and per-model gating
What it is: how the set of tools actually offered to the model is chosen.
Variants: Claude Code and OpenCode gate by model (task tools for Claude models; `patch` for GPT models). Cline routes between two edit tools by model. Pi and Oh My Pi keep a small default set and use flags. DeepSeek uses presets. OpenClaw removes tools by policy before the model sees them. Ours narrows by agent file and the node's `denied_tools`.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Glob and Grep omitted by default on macOS/Linux/WSL, restored by `--tools`/`--allowedTools` or when Bash is removed; task tools (TaskCreate/Get/List/Update, TodoWrite) provided by default only on Opus 4.7, Sonnet 4.6, Haiku 4.5 and earlier (v2.1.268+), opt in with `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`. | GA | https://code.claude.com/docs/en/tools-reference.md | reviewed |
| OpenCode | `patch` is exposed only for supported GPT models (others get edit and write); denying `execute` for `*` removes Code Mode and the tools reachable only through it; `todowrite` opt-in via `experimental.todo`. | GA | https://opencode.ai/docs/tools/, https://opencode.ai/docs/config/ | reviewed |
| Codex | `features.unified_exec` and `features.shell_tool` flags; managed requirements can disable resumable processes leaving only one-shot `exec_command`; custom agents can disable `request_permissions`. | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md, https://learn.chatgpt.com/docs/agent-configuration/subagents.md | unreviewed |
| DeepSeek harness | Presets `minimal`, `standard`, `cordis`, `ptc` (and a `code` preset mentioned on one page) select which tools mount; no page lists each preset's contents. Opt-in groups (PTY, browser, session history, agent teams, worktree) are experimental. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/schedule, https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| Oh My Pi | `/settings` toggles tools; `--tools a,b` pins a set; `--no-tools` disables all; `tools.xdev` (default on) moves less-used tools behind `xd://`. | GA | https://omp.sh/docs/tools | reviewed |
| Pi | `defaultTools` setting (replace, or add/remove with `+name`/`-name`); `-t/--tools` allowlist with `*` patterns and `mcp__` entries (1.0.4); `-xt` exclude; `-nbt` drop built-ins keeping extension tools; `-nt` disable everything including MCP. | GA | https://pi.dev/docs/latest/cli, https://pi.dev/docs/latest/settings, https://pi.dev/changelog | reviewed |
| OpenHands | Agent profiles pick "standard" or hand-chosen tools; removing a tool removes the capability. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/agent-profiles.md | unreviewed |
| OpenClaw | Five policy layers (tool profile, provider profile, global/per-agent allow and deny, provider allow/deny, sandbox tool policy); `deny` always wins; a non-empty `allow` blocks all unlisted; tools removed by policy are not sent to the model; policy does not inspect what `exec` does internally. | GA | https://docs.openclaw.ai/gateway/sandbox-vs-tool-policy-vs-elevated, https://docs.openclaw.ai/tools | unreviewed |
| goose | Extensions are toggled per session; some platform extensions are default on (Analyze, Extension Manager, Skills, Summon). | GA | https://goose-docs.ai/docs/getting-started/using-extensions.md | unreviewed |
| Hermes Agent | Toolsets chosen per run (`--toolsets`) or per platform (`hermes tools`); delegated children inherit the parent's toolsets. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tools, https://hermes-agent.nousresearch.com/docs/reference/tools-reference | unreviewed |
| Cline | Session offers either `editor` or `apply_patch`; routing overridable with `toolRoutingRules`; `spawn_agent` and `teams` toggled in CLI `/settings`. | GA | https://docs.cline.bot/tools-reference/all-cline-tools.md, https://docs.cline.bot/features/subagents.md | reviewed |
| rizzy-harness (ours) | Agent `tools:` list narrows what is offered; node policy (`denied_tools`) still decides execution. Tools are not loaded on demand. | works | `crates/agent-native/src/lib.rs:1089-1097`, `crates/node/src/session.rs:957-975` | from own inventory |

#### Deferred tool loading (tool search)
What it is: keeping most tool schemas out of the prompt and letting the model look them up.
Variants: Claude Code, Hermes, OpenClaw and Codex use a search tool over a name manifest (Claude Code loads up to 5 matches; Hermes uses BM25 with a 5% context budget). goose loads schemas on demand via `enable_tools`. Oh My Pi uses a different layer, `xd://` devices, that changes presentation only. OpenCode instead routes MCP tools through a script tool (next entry). Default state differs: on in Claude Code, Hermes, OpenClaw and Antigravity; off unless set in goose.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | On by default: only names and server instructions load; `ToolSearch` loads up to 5 matches that stay until their discovery turn is compacted; `ENABLE_TOOL_SEARCH` unset/`true`, `auto`, `auto:N`, `false`; core tools always load; model-gated (Opus 4 / Sonnet 4 / Haiku 4.5 and later). | GA | https://code.claude.com/docs/en/mcp.md, https://code.claude.com/docs/en/agent-sdk/tool-search.md | unreviewed |
| Codex | MCP tools can be kept out of the initial list and found through tool search; per-server surfaces `direct`, `deferred`, `code_mode`; 0.167.0 turned "deferred tool world state" on by default. | GA (mechanism partly documented) | https://learn.chatgpt.com/docs/config-file/config-reference.md, https://learn.chatgpt.com/docs/changelog | unreviewed |
| Pi | A `tool_search` extension finds and declares deferred tools; MCP tools have exposure modes `codemode` (default), `deferred`, `direct`, `hidden`. The CLI page says codemode and tool_search are off by default while settings says on. | GA | https://pi.dev/docs/latest/cli, https://pi.dev/docs/latest/mcp | unreviewed |
| Oh My Pi | Extra tools sit behind `xd://` devices: `read xd://` lists them, `write xd://<tool>` runs one (`tools.xdev` on by default); presentation only, not permissions. | GA | https://omp.sh/docs/tools | reviewed |
| Antigravity | MCP tool schemas lazy-loaded by default (schemas cached on disk, called through `call_mcp_tool`) with `eager: true` per tool to register the schema upfront. | GA | https://antigravity.google/docs/mcp, https://antigravity.google/docs/rules | unreviewed |
| OpenClaw | `tool_search`, `tool_describe`, `dispatch_action`; listed under experimental features yet on by default when unset. | GA | https://docs.openclaw.ai/tools, https://docs.openclaw.ai/concepts/experimental-features | reviewed |
| goose | `GOOSE_TOOL_DEFERRAL=lookup` (off unless set) publishes MCP tool names and enables schemas on demand through `enable_tools`. | unclear | https://goose-docs.ai/docs/guides/config-files.md, https://github.com/aaif-goose/goose/releases | corrected |
| Hermes Agent | MCP and non-core plugin tools hide behind `tool_search` (BM25 with stemming), `tool_describe`, `tool_call`; hooks, guardrails and approvals still apply to the real tool name; three tiers; default budget 5% of context. Called opt-in but `auto` behaves as on. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tool-search | unreviewed |
| rizzy-harness (ours) | None: every allowed tool's schema is sent each step (the plan's "schemas loaded on demand" is docs-only). | none | `crates/tools/src/lib.rs:385-396` | from own inventory |

#### Model-written scripts that call tools (code mode, programmatic tool calling)
What it is: the model writes a short program that calls other tools, so many calls cost one turn and only the script output re-enters the context.
Variants: language and sandbox differ (JavaScript in V8, QuickJS or Boa; TypeScript; Python over a Unix socket), and so does whether sub-calls still pass approvals. OpenCode and Pi make this the default route for MCP tools. DeepSeek and Hermes keep sub-calls inside the normal pipeline. Statuses are mixed or contradictory for Codex and Pi.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | `execute`: JavaScript that calls and combines catalog tools (including in parallel); no filesystem, imports or timers, only `fetch`; MCP tools reached through it by default; nested tools still enforce their own permissions. | GA | https://opencode.ai/docs/tools/, https://opencode.ai/docs/mcp-servers/ | reviewed |
| Codex | Code Mode: model drives tools through scripts in an embedded V8 runtime; remote Code Mode host over HTTP/2 gRPC. Config says "under development and off by default" while releases ship many Code Mode features. | unclear | https://learn.chatgpt.com/docs/config-file/config-reference.md, https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/changelog | reviewed |
| DeepSeek harness | `run_code`: model-written TypeScript calling tools as async functions; sub-calls go through the same pipeline and approvals and are logged as `tool/ptc-dispatch`; programs treated as hostile with capped logs and values; Python runtime experimental. | preview; Python experimental | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/ptc-runtime | unreviewed |
| Pi | `codemode`: JavaScript in QuickJS with no Node APIs, filesystem, network or timers; 256 MB per VM; at most two workers; MCP tools default to this exposure. Review removed the claims that failed scripts do not roll back and the output-cap detail. Default state contradicted across pages. | GA (default unclear) | https://pi.dev/docs/latest/codemode, https://pi.dev/docs/latest/mcp | corrected |
| goose | Code Mode extension: three meta-tools (`list_functions`, `get_function_details`, `execute_javascript`), JS in a fresh Boa context with `tools.call()`; no fs, shell, network or env; text-only results; recommended for 5+ extensions. | unclear | https://goose-docs.ai/docs/guides/managing-tools/code-mode.md | reviewed |
| Hermes Agent | `execute_code`: Python script importing a generated `hermes_tools` stub; calls travel over a Unix socket; only `print()` output returns; allowed calls are web, file and foreground terminal tools; no recursion, delegation or MCP; 300 s, 50 calls, 50 KB inline; persistent kernel per session (idle 1800 s, max 4). | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/code-execution | reviewed |
| Oh My Pi | `eval`: persistent Python and JS cells (Ruby, Julia opt-in); README says cells can call agent tools over a loopback bridge; no dedicated docs page, so isolation is unknown. | GA | https://omp.sh/docs/tools | unreviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Ask-the-user tool
What it is: a tool call that pauses the loop to ask the user a structured question.
Variants: Claude Code has an optional idle timeout that auto-continues the question (permission prompts never auto-resolve). DeepSeek can deliver the answer as a later turn even after a host restart. Warp governs asking by a profile permission. ACP standardises it as elicitation (form or url mode). Ours uses message-embedded `ui:` blocks instead of a tool.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `AskUserQuestion`: multiple choice with an "Other" row and notes; waits forever by default; `askUserQuestionTimeout` (`60s`, `5m`, `10m`) submits selections and tells Claude the user may be away. | GA | https://code.claude.com/docs/en/tools-reference.md | unreviewed |
| Codex | `tool/requestUserInput`: one to three short questions with optional free-form choice. | experimental | https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| OpenCode | `question` pauses to ask structured multi-choice questions; the client must support the form. | GA | https://opencode.ai/docs/tools/ | unreviewed |
| DeepSeek harness | `ask_user_question`, timed; the answer can arrive as a later turn even after a host restart. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| Oh My Pi | `ask` is in the coordinate group of the tool list. | GA | https://omp.sh/docs/tools | reviewed |
| Cursor | An ask-questions tool is in the editor agent's tool list. | GA | https://cursor.com/docs/agent/overview | unreviewed |
| OpenClaw | `ask_user`, and `secrets` to obtain a credential without exposing it to the agent. | GA | https://docs.openclaw.ai/tools | unreviewed |
| Hermes Agent | `clarify`: one to five questions, single, multi or open. | GA | https://hermes-agent.nousresearch.com/docs/reference/tools-reference | unreviewed |
| Warp | Multiple-choice questions (single or multi select, optional "Other"); a profile permission sets never ask, ask unless auto-approve, or always ask. | GA | https://docs.warp.dev/agents/local-agents/interacting-with-agents/agent-questions/ | unreviewed |
| Cline | `ask_question` in the built-in list. | GA | https://docs.cline.bot/tools-reference/all-cline-tools.md | reviewed |
| Zed/ACP | ACP elicitation: form mode (flat JSON Schema, non-secrets only) or url mode (out-of-band page; client must show the full URL and get consent); accept, decline or cancel. Stabilised 2026-07-22. | GA (ACP) | https://agentclientprotocol.com/protocol/v1/elicitation.md | unreviewed |
| rizzy-harness (ours) | No `ask_user` tool; questions are `ui:choices` and `ui:form` blocks in message text, validated in Rust, answers checked against the log. | partial | `crates/core/src/ui/mod.rs:56-83`, `crates/node/src/session.rs:345-379` | from own inventory |

#### Todo and task-list tool
What it is: a model-maintained task list shown to the user.
Variants: Claude Code withdrew its task tools for newer models (the tool definitions cost context and newer models track work without a checklist). Pi lists todos as a deliberate non-feature. Warp and goose inject or show the list in the UI.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | TaskCreate/Get/List/Update (TodoWrite disabled in their favour) default only on Opus 4.7, Sonnet 4.6, Haiku 4.5 and earlier; withdrawn on Opus 4.8, Sonnet 5, Fable 5 and newer in 2.1.233; background and cloud sessions get them on every model. | GA | https://code.claude.com/docs/en/tools-reference.md, https://code.claude.com/docs/en/changelog.md | reviewed |
| Codex | `update_plan`, a hook-matchable local function tool. | GA | https://learn.chatgpt.com/docs/hooks.md | unreviewed |
| OpenCode | `todowrite`, opt-in via `experimental.todo`. | experimental | https://opencode.ai/docs/config/ | unreviewed |
| DeepSeek harness | `todo_write` in the catalogue. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | reviewed |
| Oh My Pi | `todo` in the coordinate group; prewalk has the strong model write todos before handoff. | GA | https://omp.sh/docs/tools, https://omp.sh/docs/prewalk | reviewed |
| Pi | Documented non-feature: no todos (substitute `TODO.md`); the 2025 post's reason is that todos confuse models. | GA (absent by design) | https://mariozechner.at/posts/2025-11-30-pi-coding-agent/ | corrected |
| Antigravity | `manage_task` seen in docs and examples. | unclear | https://antigravity.google/docs/subagents | unreviewed |
| OpenHands | `task_tracker` tool. | GA | https://docs.openhands.dev/sdk/arch/tool-system.md | unreviewed |
| goose | Todo platform extension; the todo list is also injected each turn with timestamp and working directory. | unclear | https://goose-docs.ai/docs/getting-started/using-extensions.md, https://goose-docs.ai/docs/guides/context-engineering/using-persistent-instructions.md | unreviewed |
| Hermes Agent | `todo_list`, intercepted before the registry. | GA | https://hermes-agent.nousresearch.com/docs/reference/tools-reference | unreviewed |
| Warp | Automatic task lists with per-task states and a chip that opens the list. | GA | https://docs.warp.dev/agents/capabilities/task-lists/ | unreviewed |
| rizzy-harness (ours) | No todo or plan tool for the model; the project board is user-edited. | none | not in the inventory | from own inventory |

#### Web fetch and web search tools (with per-session caps)
What it is: tools that read a URL or query the web.
Variants: who does the extraction. Claude Code's WebFetch runs a second model call and returns only that result. Codex's search is hosted, defaults to a cached OpenAI-maintained index chosen to reduce prompt-injection exposure, and bypasses the command sandbox and hooks. OpenCodex gives non-OpenAI models a synthetic search tool run by the gateway. Zed's `search_web` is Pro-only. Caps differ: Claude Code limits WebSearch to 200 calls per session and WebFetch to a 5-minute download deadline. Ours has both tools only in the master chat runtime.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | WebFetch: HTML to Markdown then usually a separate extraction model call (lossy); refuses localhost and dotless hosts; up to 100,000 chars; 15-minute cache; 5-minute download deadline; cross-host redirects returned as text; domain safety check regardless of rules. | GA | https://code.claude.com/docs/en/tools-reference.md | reviewed |
| Claude Code | WebSearch: Anthropic server-side search returning titles and URLs; up to eight backend searches per call; allowed or blocked domains (not both); session cap 200 calls shared with subagents, refilling about 100 per hour (v2.1.290+); per turn in `-p`/SDK. | GA | https://code.claude.com/docs/en/tools-reference.md | reviewed |
| Codex | Hosted search modes `cached` (default), `indexed`, `live`, `disabled`; full-access modes default to live; does not go through the command sandbox or proxy and hooks do not see it. | GA | https://learn.chatgpt.com/docs/config-file/config-basic.md, https://learn.chatgpt.com/docs/agent-approvals-security.md | unreviewed |
| OpenCodex | For routed models the hosted `web_search` is replaced by a synthetic tool; the proxy runs searches on a configured backend, wraps results as untrusted, dedups, caps at 8,000 chars and loops up to 3 times. | GA | https://opencodex.me/guides/sidecars/ | reviewed |
| OpenCode | `webfetch` gets one http(s) URL as markdown, text or html (textual responses only); `websearch` uses a configured provider. | GA | https://opencode.ai/docs/tools/, https://opencode.ai/docs/websearch/ | unreviewed |
| DeepSeek harness | `web_search` and `web_fetch`: default 8 results, pinned addresses; `web_fetch` is unconfirmed in every sandbox and approval mode (SSRF and exfiltration caveat). | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web | reviewed |
| Oh My Pi | `web_search` tool; `read` also fetches http(s) pages. | GA | https://omp.sh/docs/tools, https://omp.sh/docs/files | unreviewed |
| Cursor | Web search is in the agent's tool list. | GA | https://cursor.com/docs/agent/overview | unreviewed |
| OpenClaw | `web_search`, `x_search`, `web_fetch`. | GA | https://docs.openclaw.ai/tools | unreviewed |
| Hermes Agent | `web_search` and `web_extract`; truncation with full text saved to disk. | GA | https://hermes-agent.nousresearch.com/docs/reference/tools-reference | unreviewed |
| Zed/ACP | `fetch` (URL to Markdown); `search_web` only for Zed Pro on the Zed provider, others use an MCP server. | GA | https://zed.dev/docs/ai/tools.md | reviewed |
| Warp | Models with native search tools may search; toggled per profile ("Call web tools"); each search has a small fixed credit cost plus a content-proportional cost. | GA | https://docs.warp.dev/agents/capabilities/web-search/ | unreviewed |
| Cline | `fetch_web_content` in the built-in list. | GA | https://docs.cline.bot/tools-reference/all-cline-tools.md | reviewed |
| rizzy-harness (ours) | `web_search` (SearXNG or Brave, citation labels) and `web_fetch` (readability to Markdown, provenance and SSRF guards), master chat runtime only; page output capped at 20,000 chars. | works (chat only) | `crates/web/src/lib.rs:436-540`, `crates/web/src/lib.rs:296-420` | from own inventory |

#### Code-navigation tools (LSP, structure, AST, outline)
What it is: tools that give the model symbols, definitions or structure instead of raw text search.
Variants: LSP-backed (Claude Code, DeepSeek, Oh My Pi), tree-sitter or structure-based (goose `analyze`, Oh My Pi `ast_grep`, ours `outline`). Claude Code's LSP tool is inactive until a code-intelligence plugin is installed.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `LSP` tool: definition, references, type info, file and workspace symbols, implementations, call hierarchies; inactive until a code-intelligence plugin and its server binary are installed; not started in cloud sessions. | GA | https://code.claude.com/docs/en/tools-reference.md, https://code.claude.com/docs/en/plugins/code-intelligence.md | unreviewed |
| DeepSeek harness | `lsp` with four queries (definition, references, implementation, hover); symbols and call hierarchy excluded; `LSP_UNAVAILABLE` without a provider. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/lsp | unreviewed |
| Oh My Pi | `lsp` and `ast_grep` (off by default; README lists 50+ grammars) and a DAP `debug` tool. | GA | https://omp.sh/docs/tools | reviewed |
| goose | `analyze`: structure (directory), semantic (file) and focus (symbol with callers and callees) modes; results over 50,000 chars warn unless `force=true`. | GA | https://goose-docs.ai/docs/guides/codebase-analysis.md | unreviewed |
| Zed/ACP | `diagnostics` tool in the built-in list. | GA | https://zed.dev/docs/ai/tools.md | reviewed |
| rizzy-harness (ours) | `outline`: tree-sitter definitions with line numbers for Rust, Python, JS, TS, Go; offered only with `outline_tool` and not in chats. | works | `crates/tools/src/outline.rs:10-60`, `crates/codemap/src/lib.rs:1-14` | from own inventory |

#### Tool result size limits and spill-to-file
What it is: capping what a tool result adds to the context and keeping the rest retrievable.
Variants: spill with a locator the model re-reads (DeepSeek, goose, Claude Code for Bash, ours via `tool_output`) versus truncation with tail helpers (Pi) versus a history-storage cap (Codex). Units differ: tokens (Codex, Claude Code Read), characters (goose, ours), bytes and lines (Pi, OpenCode).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `tool_output_token_limit` caps how much tool output is stored in history; MCP servers can set a per-tool `output_token_limit`. | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md, https://learn.chatgpt.com/docs/extend/mcp.md | unreviewed |
| Claude Code | Read: 25,000 tokens per call and refuses whole files over 256 KB; tool results saved to disk capped at 1 GB (2.1.265); MCP tool descriptions and instructions capped at 2,048 chars (2.1.280). | GA | https://code.claude.com/docs/en/tools-reference.md, https://code.claude.com/docs/en/changelog.md | reviewed |
| OpenCode | `tool_output.max_lines` and `max_bytes` configure retention; read pages 2,000 lines or 50 KiB. | GA | https://opencode.ai/docs/config/, https://opencode.ai/docs/tools/ | reviewed |
| DeepSeek harness | A spill policy replaces text or image results over `maxInlineTokens` with head, tail and a locator the model reads; spill files in a per-session hashed directory, 0700/0600, exclusive creation against symlink planting; on save failure the inline result is kept. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/spill | unreviewed |
| Pi | Tools are expected to truncate: documented limit 50 KB or 2,000 lines, with `truncateHead` (reads, search) and `truncateTail` (logs, commands) helpers for extension authors. | GA | https://pi.dev/docs/latest/extensions | unreviewed |
| goose | `GOOSE_MAX_TOOL_RESPONSE_SIZE` (default 200000 characters) sends over-size tool responses to a temp file. | GA | https://goose-docs.ai/docs/guides/environment-variables.md | corrected |
| Hermes Agent | `execute_code` stdout 50 KB inline with full output cached on disk; completed terminal results keep a 200,000-char redacted tail. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/code-execution, https://hermes-agent.nousresearch.com/docs/user-guide/features/tools | reviewed |
| rizzy-harness (ours) | Model sees at most 30,000 chars per tool call (head and tail); live stream per call up to 1 MiB (configurable 4 KiB-1 GiB); web page 20,000 chars; read window 400 or 2,000 lines; filtered output stored 2 MB and re-readable with `tool_output`. | works | `crates/tools/src/lib.rs:37`, `crates/tools/src/stream.rs:16-20`, `crates/web/src/lib.rs:32-35` | from own inventory |

#### Tool risk classification metadata
What it is: tool-level metadata saying whether a call reads, writes or executes, used for approvals or concurrency.
Variants: per tool (Oh My Pi, OpenCode action names, ours) versus protocol hints (ACP kinds, MCP-style annotations in OpenHands). ACP's hints are UI-only.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Tool table marks which tools need a permission prompt; MCP `readOnlyHint` decides whether a tool may run concurrently. | GA | https://code.claude.com/docs/en/tools-reference.md, https://code.claude.com/docs/en/agent-sdk/agent-loop.md | unreviewed |
| OpenCode | Each tool maps to a permission action (read, glob, grep, edit, shell, webfetch, websearch, question, skill, subagent, execute). | GA | https://opencode.ai/docs/tools/ | reviewed |
| Oh My Pi | Each tool is classed Read, Write or Exec for approvals. | GA | https://omp.sh/docs/tools | reviewed |
| OpenHands | Tools carry MCP-style annotations (read-only, destructive, idempotent, open-world). | GA | https://docs.openhands.dev/sdk/arch/tool-system.md | unreviewed |
| Zed/ACP | ACP tool-call `kind` (read, edit, delete, move, search, execute, think, fetch, switch_mode, other) and permission option kinds are UI hints only; the agent enforces. | GA (ACP) | https://agentclientprotocol.com/protocol/v1/tool-calls.md | reviewed |
| rizzy-harness (ours) | Per-tool risk (ReadOnly or Mutating), not per argument; `ask` mode asks for every `bash` call including `ls`. | works | `crates/tools/src/lib.rs:385-396` | from own inventory |

#### Internal URL schemes inside file tools
What it is: virtual paths (`pr://`, `conflict://`, `agent://`, `xd://`) that file-shaped tools read and write instead of dedicated tools.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Oh My Pi | README says sixteen schemes usable by file-shaped tools (`pr://`, `issue://`, `agent://`, `skill://`, `ssh://`, `conflict://`, `artifact://`, `security://` seen in docs); `agent://<id>/findings.0.path` extracts fields from subagent output. The skills page says it documents no `skill://` scheme. | GA | https://omp.sh/docs/github, https://omp.sh/docs/security, https://omp.sh/docs/compaction | unreviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

### Area 3. Code editing and file operations

Edit formats and failed-edit handling are kept as separate features below. Harnesses whose survey says the edit mechanism is not documented (Pi `edit`, Antigravity `replace_file_content`, Cline `editor`, OpenClaw `edit`, goose, Cursor) have no row in the format entries; that is "nothing documented", not evidence of a particular format.

#### Edit format: exact search/replace of a string
What it is: the model sends the old text and the new text; the tool replaces an exact match.
Variants: uniqueness rule (must match once unless `replace_all`) is common to Claude Code, OpenCode, DeepSeek and ours. Hermes is the outlier: fuzzy matching. OpenHands and DeepSeek's alternative editor add view, create and insert commands. Claude Code states there is no regex or fuzzy matching.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `Edit` replaces `old_string` with `new_string`; no regex or fuzzy matching; must match exactly and be unique, or `replace_all: true`. | GA | https://code.claude.com/docs/en/tools-reference.md | unreviewed |
| OpenCode | `edit` replaces an exactly-once-matching string, or all matches with `replaceAll`. | GA | https://opencode.ai/docs/tools/ | unreviewed |
| DeepSeek harness | `edit` replaces literal text (optional `replace_all`); an alternative `str_replace_editor` offers view, create, replace, insert. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| OpenHands | `file_editor` with view, create, str_replace, insert and undo_edit commands (named in the isolation cookbook). | GA | https://docs.openhands.dev/cookbook/workspace-isolation.md | unreviewed |
| Hermes Agent | `patch` does targeted find-and-replace with fuzzy matching and returns a diff. | GA | https://hermes-agent.nousresearch.com/docs/reference/tools-reference | unreviewed |
| Zed/ACP | `edit_file` replaces specific text. | GA | https://zed.dev/docs/ai/tools.md | reviewed |
| rizzy-harness (ours) | Exact-string replace; must match once unless `replace_all`; the tool card shows a diff of old and new strings. | works | `crates/tools/src/fs.rs:150-220`, `web/src/components/ToolCard.tsx:125-131` | from own inventory |

#### Edit format: patch (apply_patch)
What it is: the model emits a multi-file patch (add, update, delete) that the tool applies.
Variants: OpenCode and Cline offer it only for some model families (GPT and Codex models) and route other models to a replace tool. Codex and OpenClaw name it as the file-edit tool.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `apply_patch` is the file-edit tool (hooks match it as `apply_patch`, `Edit` or `Write`); since 0.162.0 it preserves existing CRLF line endings by default. | GA | https://learn.chatgpt.com/docs/hooks.md, https://learn.chatgpt.com/docs/changelog | unreviewed |
| OpenCode | `patch` applies a multi-file Add/Update/Delete patch and is exposed only for supported GPT models; permission falls under `edit`. | GA | https://opencode.ai/docs/tools/ | reviewed |
| OpenClaw | `apply_patch` in the file tools list. | GA | https://docs.openclaw.ai/tools | unreviewed |
| Cline | `apply_patch` in Act mode for GPT/Codex models and all `openai-native` models; Plan mode offers neither editor nor patch. | GA | https://docs.cline.bot/tools-reference/all-cline-tools.md | reviewed |
| OpenCodex | Freeform `apply_patch` passthrough is stated to work (vendor claim; the client page says no client has runtime-verified results). | vendor-claim | https://opencodex.me/getting-started/how-it-works/ | unreviewed |
| OpenHands | A GPT-5 preset using `ApplyPatchTool` is named in the docs index only. | unclear | https://docs.openhands.dev/llms.txt | unverified |
| rizzy-harness (ours) | None: no multi-edit or patch tool. | none | not in the inventory | from own inventory |

#### Edit format: hash-anchored lines (hashline)
What it is: every line shown to the model carries a short content hash; edits cite the hash instead of reproducing old text; a changed file makes hashes mismatch and the edit is rejected.
Variants: only one harness; its evidence is the author's own benchmark (16 models, three formats, 180 tasks per run: hashline beat patch for 14 of 16 models, about +15 points on average). The omp docs pages on editing and files do not describe hashline; only the blog and README do. Release notes mention an alternate edit tool named `sloppy` (v18.18.2) with no explanation.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Oh My Pi | Lines returned by read and grep carry tags of the form line number, colon, short hash (example `2:f1`, then a separator and the line text); edits cite tags; hash mismatch rejects the edit before corruption. Numbers match the blog; the blog title says 15 models, body and table list 16; dated 2026-02-12; the author is the omp maintainer, so not independent. | GA | https://stencil.so/blog/the-harness-problem, https://github.com/can1357/oh-my-pi | corrected |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Edit format: structural (AST) edits
What it is: syntax-aware codemods instead of text replacement.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Oh My Pi | `ast_edit` and `ast_grep` via ast-grep (50+ tree-sitter grammars per README). Changes are staged as Proposed with an Accept/Reject card; accepting reruns the rewrite on current files; partial or stale previews apply what matches and flag a count mismatch (not transactional); search cap 1,000 files (`PI_MAX_AST_FILES`). `ast_grep` is off by default while `ast_edit` is on per the editing page. | GA | https://omp.sh/docs/editing, https://github.com/can1357/oh-my-pi | reviewed |
| rizzy-harness (ours) | None (tree-sitter is used only for the `outline` tool and repo map). | none | not in the inventory | from own inventory |

#### Whole-file write and create
What it is: creating a file or replacing it entirely.
Variants: whether an existing file must have been read first (Claude Code on older models, DeepSeek's optional freshness policy) or not (DeepSeek `write` may create unseen files; newer Claude models may overwrite unread files). Cursor's agent writes directly without approval except for configuration files.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `Write` creates or fully overwrites (no append); read-before-overwrite always on Opus 4.6, Haiku 4.5 and older, relaxed on newer models; notebooks and partially read files always need a read; new files need no read. | GA | https://code.claude.com/docs/en/tools-reference.md, https://code.claude.com/docs/en/changelog.md | unreviewed |
| OpenCode | `write` creates or overwrites. | GA | https://opencode.ai/docs/tools/ | unreviewed |
| DeepSeek harness | `write` creates or overwrites and may create unseen files; `edit` needs a prior read when the policy plugin is loaded. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/filesystem | unreviewed |
| Pi | `write` is a default tool; extensions that write files are told to use `withFileMutationQueue()`. | GA | https://pi.dev/docs/latest/settings, https://pi.dev/docs/latest/extensions | unreviewed |
| Oh My Pi | `write` also supports archive, SQLite row and notebook cell writes. | GA | https://omp.sh/docs/files | unreviewed |
| OpenClaw | `write` in the file tools; the workspace is the default cwd, not a sandbox. | GA | https://docs.openclaw.ai/tools, https://docs.openclaw.ai/concepts/agent-workspace | unreviewed |
| Hermes Agent | `write_file` with no approval prompt but hard-blocked paths (see write confinement). | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | unreviewed |
| Zed/ACP | `write_file`, plus `copy_path`, `move_path`, `delete_path`, `create_directory`. | GA | https://zed.dev/docs/ai/tools.md | reviewed |
| Cursor | Agent writes files without approval except config files; writes are immediate, so docs recommend version control. | GA | https://cursor.com/docs/agent/security | unreviewed |
| OpenHands | `file_editor` create command. | GA | https://docs.openhands.dev/cookbook/workspace-isolation.md | unreviewed |
| rizzy-harness (ours) | `write` creates or overwrites, makes parents, opens without following links, refuses anything under `.git`. | works | `crates/tools/src/fs.rs:103-148`, `crates/tools/src/lib.rs:149-181` | from own inventory |

#### Model-routed choice of edit tool
What it is: the harness picks which edit tool a session offers based on the model family.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Cline | A session offers either `editor` or `apply_patch`, never both; Act mode routes `apply_patch` to GPT/Codex and all `openai-native` models; SDK callers override with `toolRoutingRules`. | GA | https://docs.cline.bot/tools-reference/all-cline-tools.md | reviewed |
| OpenCode | `patch` exposed only for supported GPT models; others get `edit` and `write`. | GA | https://opencode.ai/docs/tools/ | reviewed |
| Claude Code | The read-before-edit requirement itself is model-gated (older models always require the read). | GA | https://code.claude.com/docs/en/tools-reference.md | unreviewed |
| rizzy-harness (ours) | None: one edit tool for all models. | none | not in the inventory | from own inventory |

#### Failed-edit handling, staleness and read-before-edit guards
What it is: what the tool does when the old text is missing or ambiguous, when the file changed since the model read it, or when parallel edits race.
Variants: Claude Code and DeepSeek track what the model has read (DeepSeek's check is a plugin and is unconditional without it). Oh My Pi's hashes detect staleness without a read record. OpenCode improves the error message (closest lines). Pi pushes concurrency safety onto extension authors with a mutation queue. The survey documents no staleness check for ours.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Read-deny paths refused first; the file must have been read in the conversation (a `PARTIAL view` read does not count); `old_string` must be exact and unique. A file changed on disk since the read can still be edited if `old_string` matches current content unambiguously and it can be read without a prompt; the result flags the change. Viewing via `cat`, `head`, `sed -n` and similar counts as a read. | GA | https://code.claude.com/docs/en/tools-reference.md | unreviewed |
| DeepSeek harness | `edit` on a never-read file fails `FS_NOT_OBSERVED`; a version mismatch fails `FS_STALE_VERSION`; any windowed read authorises a later edit if unchanged. The policy plugin is optional; without it writes and edits are unconditional. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/filesystem | unreviewed |
| OpenCode | Edit errors show the closest lines; read errors name the missing folder (v2.2.5). | GA | https://opencode.ai/docs/tools/, https://opencode.ai/changelog | unreviewed |
| Oh My Pi | Hash mismatch rejects the edit before corruption; stale `ast_edit` previews apply what matches and flag the mismatch (not transactional). | GA | https://stencil.so/blog/the-harness-problem, https://omp.sh/docs/editing | corrected |
| Pi | Extensions that write files should use `withFileMutationQueue()` to avoid lost updates from parallel tool calls. | GA | https://pi.dev/docs/latest/extensions | unreviewed |
| Hermes Agent | `patch` uses fuzzy matching and returns a diff; failure behaviour is not documented. | GA | https://hermes-agent.nousresearch.com/docs/reference/tools-reference | unreviewed |
| OpenHands | `undo_edit` is among the `file_editor` commands. | GA | https://docs.openhands.dev/cookbook/workspace-isolation.md | unreviewed |
| rizzy-harness (ours) | Replace must match once unless `replace_all`; no read-before-edit or staleness check in the inventory. | partial | `crates/tools/src/fs.rs:150-220` | from own inventory |

#### Encoding and line-ending safety
What it is: avoiding silent corruption when a tool rewrites a file that is not plain UTF-8 or uses CRLF.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Edit and NotebookEdit refuse non-UTF-8 files (except UTF-16LE with BOM) so undecodable bytes are not replaced with U+FFFD; Write can still replace such a file unless the new content contains U+FFFD. | GA | https://code.claude.com/docs/en/tools-reference.md | unreviewed |
| Codex | `apply_patch` preserves existing CRLF line endings by default (0.162.0). | GA | https://learn.chatgpt.com/docs/changelog | unreviewed |
| Hermes Agent | `read_file` transcodes UTF-16 and refuses files over 10 MB and binaries. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tools | unreviewed |
| rizzy-harness (ours) | Not in the inventory. | none | not in the inventory | from own inventory |

#### Post-edit diagnostics and formatters
What it is: feeding type errors or formatter results back after a write.
Variants: new-errors-only (Hermes) versus dedup of already-shown diagnostics (Oh My Pi); on by default (Hermes, Oh My Pi whole-file writes) versus plugin-dependent (Claude Code) versus formatter-only (OpenCode, off by default). Warp's LSP is an editor feature for the human.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | With a code-intelligence plugin the server starts lazily on the first edit and Claude gets diagnostics after each edit or write (`Found N new diagnostic issues in M files`); terminal sessions only. | GA | https://code.claude.com/docs/en/plugins/code-intelligence.md | unreviewed |
| Oh My Pi | Diagnostics after whole-file writes on by default; after incremental edits and format-on-write off by default; already-shown diagnostics deduplicated. | GA | https://omp.sh/docs/code-intelligence, https://omp.sh/docs/files | unreviewed |
| Hermes Agent | After each `write_file` or `patch`: in-process syntax check, then LSP diagnostics if clean; only new errors against pre-edit content, in a separate `lsp_diagnostics` field; about 30 servers; workspaces untrusted by default until in `lsp.trusted_workspaces`. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/lsp | unreviewed |
| OpenCode | Formatters off by default; `"formatter": true` enables about 30 built-ins, each only if its binary is present; custom formatters are argv arrays with `$FILE`; first success wins. | GA | https://opencode.ai/docs/formatters/ | unreviewed |
| Warp | Editor LSP gives hover, definitions, inline diagnostics and format on save for Rust, Go, Python, TS/JS, C/C++; local sessions only; the survey does not say the agent receives them. | GA | https://docs.warp.dev/code/code-editor/language-server-protocol/ | unreviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Diff review and in-app file viewing or editing
What it is: showing what the agent changed and letting the user accept, reject or edit.
Variants: approval-time diffs (Antigravity CLI, Zed, Warp, Claude Code in VS Code) versus after-the-fact views (Codex, Claude Code `/diff`, OpenHands, OpenClaw). Claude Code's per-turn views come from the file tools' edits, not git, so shell-made changes appear only under `Current`. Ours shows only the old and new strings of an edit call.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `/diff` shows the Git diff including untracked files; patch details can be expanded while an approval prompt is open (0.167.0); the Desktop diff pane stages or reverts hunks, commits, pushes, opens PRs and takes inline comments. | GA | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/changelog, https://learn.chatgpt.com/docs/environments/local-environment.md | unreviewed |
| Claude Code | `/diff` panel or dialog with per-turn views and a comparison-base cycle; VS Code inline diff with accept or reject per change (v2.1.275), whole-file only above 100 changes; the user can edit the proposed content and Claude is told. | GA | https://code.claude.com/docs/en/interactive-mode.md, https://code.claude.com/docs/en/vs-code.md | unreviewed |
| Claude Desktop | Local sessions edit files in an in-app pane (auto-save default on, Override or Discard on external change); cloud, SSH and WSL sessions are view-only. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| Cursor | Agent Review of local changes automatically after agent tasks or via `/agent-review`; Quick and Deep depths; reads `BUGBOT.md` rules. | GA | https://cursor.com/docs/agent/agent-review | unreviewed |
| Antigravity | CLI `default` mode shows an inline diff for every create or edit (accept, reject, or Tab to type an alternative; a pending edit cannot be dismissed with Esc); App review pane with Uncommitted, Branch and Agent Edits views, per-turn filter and line comments sent to the agent. | GA | https://antigravity.google/docs/cli/modes, https://antigravity.google/docs/review-changes | unreviewed |
| Zed/ACP | Review Changes opens a multi-buffer tab where each hunk, or all, can be accepted or rejected; `agent.single_file_review` keeps inline keep/reject; "Follow" jumps the editor to each touched file. | GA | https://zed.dev/docs/ai/agent-panel | unreviewed |
| Warp | Built-in diff editor: navigate hunks, accept, refine with natural language or edit manually; changes apply only on acceptance unless Apply Code Diffs is "Always allow"; native code editor with tabs, file tree and Vim keys. | GA | https://docs.warp.dev/agents/local-agents/code-diffs/, https://docs.warp.dev/code/code-editor/ | unreviewed |
| OpenClaw | Control UI Changes panel for Git diffs; detail undocumented. | GA | https://docs.openclaw.ai/web/control-ui | unreviewed |
| OpenHands | Canvas Files drawer and a combined Commits and uncommitted-changes drawer. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md | unreviewed |
| rizzy-harness (ours) | Tool card shows old and new strings of an `edit` call; per-turn checkpoint stores a file list; no whole-turn diff or file view. | partial | `web/src/components/ToolCard.tsx:125-131`, `crates/node/src/session.rs:509-530` | from own inventory |

#### Write confinement and protected paths for file tools
What it is: limits on where the file tools may write, enforced in the tool rather than by an OS sandbox.
Variants: Claude Code and ours confine to working directories; Codex and Hermes block named sensitive paths even inside writable roots; Cursor's guard is a list of protected config files plus an ignore file that does not bind the terminal. Hermes says the terminal tool can bypass its guards.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `permissions.blockWritesOutsideWorkingDirectories` makes Write, Edit and NotebookEdit refuse paths outside the working directories in every mode (v2.1.314); read counterpart `blockReadsOutsideWorkingDirectories` (v2.1.257). | GA | https://code.claude.com/docs/en/changelog.md, https://code.claude.com/docs/en/whats-new/2026-w36.md | reviewed |
| Codex | Protected subpaths inside writable roots (`.git`, `.agents`, `.codex`, `.aws`, including a resolved gitdir) stop the agent rewriting its own config or Git hooks. | GA | https://learn.chatgpt.com/docs/agent-approvals-security.md | reviewed |
| Cursor | Writes need no approval except config files (workspace settings); `.cursorignore` blocks files from the agent but not terminal commands or MCP tools; defaults ignore `.env`, `.git/`, lock files. | GA | https://cursor.com/docs/agent/security, https://cursor.com/help/customization/ignore-files | unreviewed |
| Hermes Agent | `write_file` and `patch` hard-block `~/.ssh` (except config, approval-gated), `~/.aws`, `~/.kube`, sudoers, `~/.netrc` and Hermes secret stores (also read-denied); `HERMES_WRITE_SAFE_ROOT` restricts writes to listed prefixes (auto-set in the official Docker image); docs call it defence in depth, not a sandbox. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | unreviewed |
| rizzy-harness (ours) | Paths walked component by component with lstat; dangling or escaping symlinks refused; `write` and `edit` do not follow a final link and refuse `.git/**`; the data directory is refused in every mode. | works | `crates/tools/src/lib.rs:111-181`, `crates/tools/src/fs.rs:222-262` | from own inventory |

#### Read-tool capabilities and limits
What it is: what the read tool returns (text, images, PDFs, notebooks, remote files) and how it bounds it.
Variants: Oh My Pi folds many formats and virtual paths into `read`; Claude Code and OpenCode bound by tokens, bytes and lines and send images and PDFs as media; DeepSeek fails on oversized reads instead of truncating; ACP's `fs/read_text_file` returns the editor's unsaved state.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Line-numbered text; images (downscaled, refused over 128 MB); PDFs (over 10 pages via `pages` ranges up to 20 per call, needs poppler); notebooks (error over 256 KB of cells, refuse over 100 MB); refuses binary extensions; 25,000 tokens per call, whole files over 256 KB refused with a `PARTIAL view`; `allow_large` (v2.1.296+). | GA | https://code.claude.com/docs/en/tools-reference.md | reviewed |
| OpenCode | Text 2,000 lines or 50 KiB per page, or a non-recursive directory listing; PNG/JPEG/GIF/WebP/PDF go to the model up to 20 MiB. | GA | https://opencode.ai/docs/tools/ | reviewed |
| Oh My Pi | Directories, structural overview of large files, PDF/docx/pptx/xlsx/EPUB, notebooks as cells, images (over 20 MiB rejected), archives, SQLite (tables, rows, bounded queries), http(s) pages, `ssh://` (capped 1 MiB), GitHub and session resources; `.gitignore` respected. | GA | https://omp.sh/docs/files | unreviewed |
| DeepSeek harness | `read` returns numbered lines with offset and limit; `read_image` for image-capable models; oversized `readBytes` fails `FS_TOO_LARGE` instead of truncating. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/filesystem | unreviewed |
| Hermes Agent | Paginates with line numbers, transcodes UTF-16, refuses files over 10 MB and binaries. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tools | unreviewed |
| Zed/ACP | ACP `fs/read_text_file`: absolute path, optional 1-based line and limit, returns editor state including unsaved changes; the client does the I/O and should check paths against the session root. | GA | https://agentclientprotocol.com/protocol/v1/file-system.md | reviewed |
| rizzy-harness (ours) | Text with line numbers and offset/limit; 400-line window when bounded reads are on; a 32 MiB streaming cap exists on the gate3 branch only. | works | `crates/tools/src/fs.rs:15-100` | from own inventory |

#### File mentions and attachments
What it is: attaching files, folders, URLs or images to a prompt from the UI.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | Attachments are `file:` or `data:` URIs only (no http URLs); text and directory listings inlined; images and PDFs sent as media; 20 MiB per item; images auto-resized (default 2000x2000, 5 MiB base64). Separate "references" make named directories or Git repos usable as context. | GA | https://opencode.ai/docs/attachments/, https://opencode.ai/docs/references/ | unreviewed |
| Claude Desktop | Right-click a file path to attach it, open it in VS Code, Cursor or Zed, reveal or copy. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| Antigravity | Attachments of any file type (App, v2.28.0); notebook and PDF viewers with inline comments. | GA | https://antigravity.google/docs/features, https://antigravity.google/docs/changelog | unreviewed |
| Pi | `@` searches and attaches files; Tab completes paths; images pasted or dragged; `pi @file prompt`; piped stdin prepended; RPC rejects `@file`. | GA | https://pi.dev/docs/latest/usage, https://pi.dev/docs/latest/cli | unreviewed |
| goose | Desktop `@` fuzzy file search, 5 levels deep, skipping `.git`, `node_modules`, `target`, `dist`, `build`. | GA | https://goose-docs.ai/docs/guides/file-management.md | unreviewed |
| Hermes Agent | `@` attaches files, folders, git diffs and URLs (one-line listing only; the dedicated page was not opened). | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/overview | unreviewed |
| Warp | Attach terminal blocks, images, a single public URL, and `@` references to files, folders, symbols, Warp Drive objects and other sessions' blocks; `@` search is rooted at the Git repo, respects `.gitignore`, needs no indexing. | GA | https://docs.warp.dev/agents/local-agents/agent-context/ | unreviewed |
| Cline | `@/path` adds a file, `@/dir/` a folder, `@workspace:/path` for multi-root; text, images, PDFs, CSV, Excel. | GA (VS Code) | https://docs.cline.bot/core-workflows/working-with-files.md | unreviewed |
| rizzy-harness (ours) | Not in the inventory. | none | not in the inventory | from own inventory |

#### Notebook support
What it is: cell-level notebook reading or editing.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `NotebookEdit` by `cell_id`; modes replace, insert, delete; refuses non-UTF-8; notebooks always require a prior Read. | GA | https://code.claude.com/docs/en/tools-reference.md | unreviewed |
| Oh My Pi | `read` shows notebooks as cells; notebook cell writes supported. | GA | https://omp.sh/docs/files | unreviewed |
| Cline | Generate, Explain, Improve Cell commands preserving cell metadata and outputs; needs the VS Code Jupyter extension. | GA | https://docs.cline.bot/features/jupyter-notebooks.md | unreviewed |
| Antigravity | Notebook viewer with inline cell comments (App only); no agent-edit statement. | GA | https://antigravity.google/docs/features | unreviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Git conflict resolution through virtual paths
What it is: resolving merge conflicts by writing to session-local URLs rather than editing marker text.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Oh My Pi | Complete conflict blocks get a session-local `conflict://N`; writing `@ours`, `@theirs` or `@base` resolves it; `conflict://*` for bulk; a reference is invalid after an edit until the file is re-read. | GA | https://omp.sh/docs/editing | unreviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Debug mode with log instrumentation
What it is: a guided loop that inserts logging, has the user reproduce, reads logs, fixes, and removes the instrumentation.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Cursor | Hypothesise causes, insert log statements that post to a local debug server in a Cursor extension, user reproduces, analyse logs, apply a targeted fix, remove instrumentation. | GA | https://cursor.com/docs/agent/debug-mode | unreviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Inline selection edits and edit prediction (editor-native)
What it is: prompt-driven in-place transforms of a selection, and tab-style completions.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Zed/ACP | Inline Assistant transforms a selection (ctrl-enter, multi-cursor, can fan out to several models via `inline_alternatives`; not for external agents). Edit Prediction: default provider Zeta, alternatives Copilot, Mercury Coder, Codestral, Ollama; modes `eager` and `subtle`. | GA | https://zed.dev/docs/ai/inline-assistant.md, https://zed.dev/docs/ai/edit-prediction.md | unreviewed |
| rizzy-harness (ours) | None (web UI, no editor integration). | none | not in the inventory | from own inventory |

#### Per-session scratch directory
What it is: a temp directory the agent may use without permission prompts.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/tmp/claude-<uid>/<project>/<session-id>/scratchpad/` (Linux), no prompts for create, edit and read; path passed to hooks as `scratchpad_dir`; deleted with the transcript; only present with a claude.ai sign-in on the Anthropic API and `enableArtifact` not false. | GA | https://code.claude.com/docs/en/claude-directory.md | unreviewed |
| rizzy-harness (ours) | A per-session TMPDIR exists only on the `integration/gate3` branch; on `main` the inventory lists none. | none (on main) | not in the inventory | from own inventory |

#### Office-document deliverables
What it is: producing spreadsheets, decks and documents rather than code files.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Desktop | Cowork reads and writes local files and produces spreadsheets with formulas, presentations and formatted documents (`/docx /pdf /pptx /xlsx`); the new-chat Output picker can create Google Docs, Sheets and Slides; files up to 50 MB in a project (reviewed). "Polished" quality is a vendor claim. | GA | https://claude.com/docs/cowork/overview.md, https://claude.com/docs/cowork/changelog.md, https://claude.com/docs/cowork/guide/projects.md | reviewed |
| rizzy-harness (ours) | No Office-format generation in the inventory; `publish` only stages a file or folder for the master to pull into Artifacts. | none | `crates/tools/src/publish.rs:48-80` | from own inventory |

### Area 4. Terminal and shell integration

Sandboxing and approval rules for shell commands belong to the permissions area and are not repeated here.

#### Shell execution model (process per command or persistent shell)
What it is: how one shell tool call maps to processes, and what state (cwd, env, aliases) survives between calls.
Variants: Claude Code starts a separate process per command and replays a startup snapshot of aliases and shell options; cwd carries over only inside the project. DeepSeek's default is a fresh shell with a persistent variant as an alternate package. Oh My Pi ships its own bash implementation in Rust with persistent sessions. Codex has resumable processes. Warp runs inside a real terminal session. Hermes' `terminal` is non-interactive with no TTY unless asked.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `exec_command` with resumable processes (unified exec); a later `write_stdin` poll can deliver output; shell snapshot speeds repeated commands; `allow_login_shell = false` rejects login shells; hooks match it as `Bash`. | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md, https://learn.chatgpt.com/docs/hooks.md | unreviewed |
| OpenCode | `shell` runs in the host user's shell with `command`, `workdir`, `timeout` (default 2 min) and `background`; a built-in scanner extracts each command for permission checks. | GA | https://opencode.ai/docs/tools/, https://opencode.ai/docs/permissions/ | reviewed |
| Claude Code | Each command is a separate process; `cd` carries over only inside the project or added directories (otherwise reset with a notice); env vars do not persist; aliases, functions and shell options are captured once at session start from the rc files and applied to every command; persistent env via `CLAUDE_ENV_FILE` or a SessionStart hook. | GA | https://code.claude.com/docs/en/tools-reference.md | unreviewed |
| DeepSeek harness | `bash` runs in a fresh shell (`description`, `command`, `timeoutMs`, `workdir`, `run_in_background`) and reports exit code, signal, timedOut and aborted separately; a persistent-shell variant is an alternate package (SDK minimal profile, 300 s). | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/shell, https://deepseek-harness.github.io/deepseek-harness/en/guide/python-sdk | unreviewed |
| Oh My Pi | README: bash 5.3 implemented in Rust (`pi-shell`) with persistent sessions, in-process coreutils (46, 58 or 67 depending on README section) and a minimizer; ripgrep, glob and find run in-process; no WSL needed on Windows; `PI_SHELL_PREFIX` wraps every command. | GA | https://github.com/can1357/oh-my-pi, https://omp.sh/docs/env | unreviewed |
| Pi | Default shell Bash (Git Bash on native Windows); `shellPath`, `shellCommandPrefix` settings; the bash tool takes a spawn hook that can change command, cwd or environment. | GA | https://pi.dev/docs/latest/windows, https://pi.dev/docs/latest/settings, https://pi.dev/docs/latest/extensions | unreviewed |
| Cursor | Commands run in the user's default terminal profile under the chosen Run Mode. | GA | https://cursor.com/docs/agent/tools/terminal | unreviewed |
| Antigravity | `run_command` appears in docs and examples. | unclear | https://antigravity.google/docs/sdk/policies | unreviewed |
| OpenHands | `execute_bash`; workspace `execute_command` returns stdout, stderr, exit code, timeout flag and duration; the Agent Server persists `bash_events/` and exposes endpoints to start and search bash commands. | GA | https://docs.openhands.dev/sdk/arch/agent-server.md, https://docs.openhands.dev/sdk/arch/workspace.md | unreviewed |
| OpenClaw | `exec` runs shell commands; the exec host can be the sandbox, the gateway or a paired node. | GA | https://docs.openclaw.ai/tools, https://docs.openclaw.ai/tools/exec-approvals | unreviewed |
| goose | `GOOSE_SHELL` overrides the shell for Developer extension commands (default bash else sh on Unix, cmd on Windows); `GOOSE_SEARCH_PATHS` prepends PATH entries. | GA | https://goose-docs.ai/docs/guides/environment-variables.md | unreviewed |
| Hermes Agent | `terminal` runs non-interactive commands (no TTY unless `pty=true`); a trailing `&` is auto-backgrounded; `nohup`, `disown` and command-substitution forms are refused; sudo via a masked prompt cached for the session or `SUDO_PASSWORD` in `.env`. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tools | unreviewed |
| Zed/ACP | Agent `terminal` tool runs a command and returns combined output. | GA | https://zed.dev/docs/ai/tools.md, https://zed.dev/docs/ai/terminal-threads.md | reviewed |
| Warp | The agent runs in a real terminal session and, depending on profile, waits for approval before running commands. | GA | https://docs.warp.dev/agents/ | unreviewed |
| Cline | `run_commands`: the model sets a `requires_approval` flag per command; no fixed allowlist; a notification fires when an auto-approved command has run 30 s. | GA | https://docs.cline.bot/features/auto-approve.md, https://github.com/cline/cline | reviewed |
| rizzy-harness (ours) | `bash -c` in the session folder, own process group, allow-listed env, combined stdout and stderr, exit code. No sandbox on `main`; Landlock only on a branch. | works | `crates/tools/src/bash.rs:86-259` | from own inventory |

#### Command timeouts and auto-backgrounding
What it is: how long a command may run and what happens at the limit.
Variants: Claude Code moves a foreground command that hits its timeout to the background instead of killing it (unless it starts with `sleep`); DeepSeek kills by default; ours kills at 600 s with a hard cap on the model-chosen timeout.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Per-call `timeout`; `BASH_DEFAULT_TIMEOUT_MS` 2 min and `BASH_MAX_TIMEOUT_MS` 10 min ceiling; a foreground command at its timeout is moved to the background (result gives task id and output path); unattended sessions have a 30 min default limit or up to 2 h (10 min in text-prompt `-p`); interactive local sessions have none. | GA | https://code.claude.com/docs/en/tools-reference.md | reviewed |
| OpenCode | Default timeout 2 min; `background` calls notify the session on completion. | GA | https://opencode.ai/docs/tools/ | reviewed |
| DeepSeek harness | Optional `timeoutMs`; default behaviour kills on timeout. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/shell | unreviewed |
| rizzy-harness (ours) | Default 600 s per tool call; bash `timeout_secs` argument capped at 3600. | works | `crates/agent-native/src/lib.rs:68`, `crates/tools/src/bash.rs:103-121` | from own inventory |

#### Background processes
What it is: starting a long-running command without blocking the loop, then reading, polling or killing it.
Variants: Claude Code writes output to a file the model reads with Read; DeepSeek keeps a bounded in-memory ring with a consuming cursor (process-local, not restored after restart); Hermes polls with notify-on-complete after two unchanged polls; Codex lets a later `write_stdin` poll collect output. Pi deliberately has no background bash and points to tmux. Ours has none.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `run_in_background`; `/tasks` lists and stops; `Ctrl+B` moves a running command to the background; output goes to a file read with Read; tasks cleaned up at exit and detached processes killed on macOS and Linux; killed above 5 GB output; stopped under OS memory pressure after 30 minutes idle on macOS and Linux. | GA | https://code.claude.com/docs/en/tools-reference.md, https://code.claude.com/docs/en/interactive-mode.md | reviewed |
| Codex | `/ps` lists background terminals and recent output; `/stop` stops them all; the app-server can list, clean and terminate them (experimental). | GA (CLI); experimental (API) | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| OpenCode | Background shells with live output and a stop control (desktop activity pill). | GA | https://opencode.ai/changelog, https://opencode.ai/docs/tools/ | unreviewed |
| DeepSeek harness | Job registry (running, stopping, completed, killed, failed); bounded output ring (live 262144 bytes, settled 16384 by default); at most 10 concurrent jobs per owner session; `job/registered` and `job/settled` go to the session log; registry is process-local. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/jobs | reviewed |
| Oh My Pi | `hub` supervises shared processes (needs `launch.enabled`); `omp ps` lists, inspects, logs, stops and restarts them; `/jobs` for background jobs. | GA | https://omp.sh/docs/tools, https://omp.sh/docs/cli | unreviewed |
| Pi | Documented non-feature: no background bash; substitute tmux. | GA (absent by design) | https://mariozechner.at/posts/2025-11-30-pi-coding-agent/ | reviewed |
| Antigravity | CLI `/tasks` tracks background processes. | GA | https://antigravity.google/docs/cli/commands/context | unreviewed |
| OpenHands | The Agent Server persists bash events; no background-process mechanism is described. | unclear | https://docs.openhands.dev/sdk/arch/agent-server.md | unreviewed |
| OpenClaw | `process` manages running commands; a Background exec page exists but was not opened. | GA | https://docs.openclaw.ai/tools | unreviewed |
| Hermes Agent | `background=true` returns a session id and pid; `process_manage` (list, poll, wait, log, kill, write); after two unchanged polls notify-on-complete turns on; newest 64 completed results kept up to 7 days with a 200,000-char redacted tail. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tools | unreviewed |
| Warp | Background-process output gets its own block; prompts queue by default during agent-driven long-running commands. | GA | https://docs.warp.dev/terminal/blocks/, https://docs.warp.dev/agents/local-agents/interacting-with-agents/prompt-queueing/ | unreviewed |
| Cline | README says Cline watches long-running processes such as dev servers. | GA | https://github.com/cline/cline | unreviewed |
| Cursor | Background processes and timeouts are not on the terminal page. | unclear | https://cursor.com/docs/agent/tools/terminal | unverified |
| rizzy-harness (ours) | None: no long-running spawned background tasks inside a chat. | none | not in the inventory | from own inventory |

#### Agent-driven interactive terminal (PTY)
What it is: the agent opens a real terminal, types into it and reads the screen, so it can drive REPLs, editors and debuggers.
Variants: Warp reads the live terminal buffer and lets the user take over and hand back. DeepSeek's PTY tools are an opt-in experimental package with ownership bound to the owning agent. In ACP the client runs the terminal for the agent.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Warp | Full Terminal Use: the agent reads the live terminal buffer and types into interactive programs (database shells, editors, debuggers); the user can take over or hand back; each action approved once or auto-approved for the session. | GA | https://docs.warp.dev/agents/capabilities/full-terminal-use/ | unreviewed |
| DeepSeek harness | `terminal_*` tools (six, opt-in): persistent terminals the agent can open, send to, read, signal and close; one active send per session; bounded scrollback; ownership bound to the owning agent; PTY state process-local. | experimental (package); page gives no label | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/terminal, https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| Hermes Agent | `pty=true` on `terminal`; `process_manage write` for input. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tools | unreviewed |
| Oh My Pi | Optional PTY for bash (`--no-pty` to turn off). | GA | https://github.com/can1357/oh-my-pi | unreviewed |
| Codex | `write_stdin` writes to a running resumable command. | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| Zed/ACP | `terminal/create` (command, args, env, absolute cwd, `outputByteLimit`) returns immediately; `output`, `wait_for_exit`, `kill`, `release`; the client runs the command and applies its own policy and neutralises control sequences. | GA (ACP) | https://agentclientprotocol.com/protocol/v1/terminals.md | reviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### User-facing terminal pane in the app
What it is: an interactive terminal the user can use alongside the agent.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Each desktop chat has a terminal scoped to its project or worktree, and ChatGPT can read its output; project "actions" run in it. | GA | https://learn.chatgpt.com/docs/integrated-terminal.md, https://learn.chatgpt.com/docs/environments/local-environment.md | unreviewed |
| OpenCode | Desktop terminals belong to the session, keep scrollback across server restarts and sync with the TUI; the API has a persistent PTY with WebSocket streaming and single-use connect tickets (endpoints under `experimental`). | GA / experimental (PTY API) | https://opencode.ai/changelog, https://opencode.ai/docs/api/ | unreviewed |
| Claude Desktop | Ctrl plus backtick toggles a terminal sharing the session's environment, in local and SSH sessions (limits on Windows remote machines). | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| OpenHands | Legacy UI has a Terminal tab shared by user and agent; Canvas shows terminal output from the server. | GA | https://docs.openhands.dev/openhands/usage/key-features.md | unreviewed |
| OpenClaw | `terminal` is a shared operator terminal in the Control UI. | GA | https://docs.openclaw.ai/tools | unreviewed |
| Warp | The product is the terminal (blocks, input editor, completions). | GA | https://docs.warp.dev/terminal/blocks/, https://docs.warp.dev/terminal/editor/ | unreviewed |
| rizzy-harness (ours) | None: no file browser, editor or terminal pane in the UI. | none | not in the inventory | from own inventory |

#### User shell escape (`!` commands)
What it is: the user runs a command directly from the prompt without a model turn, optionally adding the output to context.
Variants: Codex and Claude Code run it outside the sandbox. Oh My Pi and Pi distinguish `!` (output goes to the model) from `!!` (it does not). Claude Code answers automatically unless `respondToBashCommands: false`. Cursor's CLI sends a sudo password straight to sudo rather than to the model.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `! cmd` runs without Claude approving it, streams output, adds command and output to context; Claude responds (costs a normal prompt) unless `respondToBashCommands: false`; Ctrl+B backgrounds; user `!` commands run outside the sandbox except in strict sandbox mode. | GA | https://code.claude.com/docs/en/interactive-mode.md | unreviewed |
| Codex | `thread/shellCommand` runs a user-initiated command outside the sandbox with full access; presets beginning with `!` are shell commands. | GA | https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| OpenCode | `!` at the start of an empty prompt runs a command in the session directory with output kept in the session. | GA | https://opencode.ai/docs/cli/tui/ | unreviewed |
| Oh My Pi | `!cmd` adds output to context, `!!cmd` excludes it; `$ code` and `$$ code` do the same for a shared Python kernel; not sandboxed. | GA | https://omp.sh/docs/using | unreviewed |
| Pi | `!cmd` includes output in the conversation; `!!cmd` does not; `user_bash` extension event; still Bash on Windows. | GA | https://pi.dev/docs/latest/usage | unreviewed |
| Cursor | CLI `/shell` Shell Mode; the sudo prompt sends the password straight to sudo and not to the model. | GA | https://cursor.com/docs/cli/overview, https://cursor.com/docs/cli/reference/slash-commands | unreviewed |
| Hermes Agent | `!cmd` in the CLI runs a shell command without a model turn. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/cli | unreviewed |
| Warp | Terminal mode for shell commands; `#` at the prompt generates a command; input is auto-classified as natural language or shell (toggle `/natural-language-detection` in the CLI). | GA | https://docs.warp.dev/agents/local-agents/interacting-with-agents/terminal-and-agent-modes/, https://docs.warp.dev/agents/cli/input-and-shell-commands/ | unreviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Shell output limits and spill
What it is: the cap on command output the model sees and where the rest goes.
Variants: separate caps for success and failure (Claude Code: more inline for success, head and tail for failure), spill to a file with a path (Claude Code, DeepSeek), a retained redacted tail (Hermes), or head and tail with retrieval by id and range (ours). ACP lets the client set a byte limit that drops the earliest output.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Output streams to a working file, killed past 5 GB; success: inline up to about 30,000 chars, otherwise a file path (truncated past 64 MiB) plus a 2,000-char preview; failure: inline up to about 10,000 chars, otherwise a head-and-tail excerpt, no file path; `BASH_MAX_OUTPUT_LENGTH` (default 30,000, ceiling 150,000), `bashOutputMaxChars` up to 128,000 (v2.1.261+). Exit 1 is benign only for `grep`, `rg`, `egrep`, `fgrep`, `find`, `diff`, `test`, `[`, `git diff`, `git grep`. | GA | https://code.claude.com/docs/en/tools-reference.md | reviewed |
| DeepSeek harness | Per stream keeps the tail and spills the full stream to a private file. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/shell | unreviewed |
| Hermes Agent | Newest 64 completed background results kept up to 7 days with a 200,000-char redacted tail. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tools | unreviewed |
| Pi | Command output is expected to be truncated from the tail (50 KB or 2,000 lines, `truncateTail`). | GA | https://pi.dev/docs/latest/extensions | unreviewed |
| Zed/ACP | `terminal/create` takes `outputByteLimit`; `output` returns a `truncated` flag; truncation drops earlier output. | GA (ACP) | https://agentclientprotocol.com/protocol/v1/terminals.md | reviewed |
| Cursor | Output truncation is not on the terminal page. | unclear | https://cursor.com/docs/agent/tools/terminal | unverified |
| rizzy-harness (ours) | Model sees at most 30,000 chars per call (head and tail); live stream up to 1 MiB per call; full filtered text stored (2 MB, per-session random ids) and re-readable by id, range or `contains`. | works | `crates/tools/src/lib.rs:37`, `crates/tools/src/stream.rs:16-20`, `crates/tools/src/outputs.rs:15-25` | from own inventory |

#### Command-output filtering and minimisation
What it is: rewriting command output to cut noise before it reaches the model.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Oh My Pi | README lists a "minimizer" in the embedded bash engine; mechanism not documented. | GA | https://github.com/can1357/oh-my-pi | unreviewed |
| OpenClaw | The Tokenjuice plugin compacts noisy exec output; its page was not opened. | unclear | https://docs.openclaw.ai/tools | unreviewed |
| rizzy-harness (ours) | Output filter strips ANSI, keeps the last state of `\r` progress lines, drops build and download noise, replaces passing tests with a count, collapses repeats; full text retrievable with `tool_output`; each saving independently switchable. | works | `crates/tools/src/filter.rs:1-40`, `crates/tools/src/bash.rs:206-232` | from own inventory |

#### Environment given to spawned commands
What it is: which environment variables a tool-spawned process inherits, and which markers it receives.
Variants: Codex passes `*KEY*`, `*SECRET*` and `*TOKEN*` variables through by default (`ignore_default_excludes` defaults to true). DeepSeek, Hermes (for scripts) and ours scrub. Ours starts from an empty environment plus an allow-list. Several set markers (`AGENT=goose`, `AI_AGENT=pi`, `CURSOR_AGENT`) so shell configs can detect the agent.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `shell_environment_policy`: `inherit` all, core or none; include and exclude globs (case-insensitive); `set` values; `ignore_default_excludes` defaults to `true`, so keys, secrets and tokens are passed unless the user opts in to filtering. | GA | https://learn.chatgpt.com/docs/config-file/config-advanced.md, https://learn.chatgpt.com/docs/config-file/config-basic.md | reviewed |
| Claude Code | Shell state snapshotted at session start; persistent env via `CLAUDE_ENV_FILE` or a SessionStart hook; the exact list removed by `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` is not documented (print it by CLI command). | GA | https://code.claude.com/docs/en/tools-reference.md | unverified |
| DeepSeek harness | `DSH_*` variables are harness-owned and rebuilt per model shell call; the local executor scrubs ambient credentials before merging env; the MCP stdio bridge strips credential-style and all `DSH_*` vars; proxy variables, including a password in a proxy URL, are inherited by every child command. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/shell, https://deepseek-harness.github.io/deepseek-harness/en/guide/mcp-memory | unreviewed |
| Hermes Agent | `execute_code` child environment strips names containing KEY, TOKEN, SECRET, PASSWORD, CREDENTIAL, PASSWD, AUTH. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/code-execution | reviewed |
| Pi | Sets `PI_SESSION_ID`, `PI_SESSION_FILE`, `PI_PROVIDER`, `PI_MODEL`, `PI_REASONING_LEVEL` for shell tool commands (unless disabled) and `AI_AGENT=pi`, `PI_CODING_AGENT=true` for child processes. | GA | https://pi.dev/docs/latest/environment-variables | unreviewed |
| goose | Sets `GOOSE_TERMINAL=1` and `AGENT=goose` when running commands. | GA | https://goose-docs.ai/docs/guides/environment-variables.md | unreviewed |
| Cursor | The `CURSOR_AGENT` env var lets shell configs detect agent sessions. | GA | https://cursor.com/docs/agent/tools/terminal | unreviewed |
| rizzy-harness (ours) | Children start from an empty env plus `PATH HOME USER LOGNAME SHELL LANG LANGUAGE LC_* TERM COLORTERM TZ TMPDIR` and `env_pass` globs; provider key vars, `.env` names and `HARNESS_*` always stripped. | works | `crates/tools/src/env.rs:1-135` | from own inventory |

#### Resource limits for commands
What it is: capping memory or CPU of tool-spawned processes.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `CLAUDE_CODE_TOOL_MEMORY_LIMIT` (for example `4G`, v2.1.233+, Linux/WSL) caps memory for all of a session's Bash, PowerShell and Monitor commands via a memory cgroup; if the cgroup cannot be created, commands run uncapped; OOM kills are not attributed in the tool result; permission-gating hooks and the MCP servers they call are always exempt. | GA | https://code.claude.com/docs/en/tools-reference.md | reviewed |
| Hermes Agent | Docker backend default container resources 1 CPU, 5120 MB memory, 51200 MB disk. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/docker | reviewed |
| rizzy-harness (ours) | None in the inventory. | none | not in the inventory | from own inventory |

#### Pluggable execution backends for shell and file tools
What it is: choosing where commands and file operations run (local, container, SSH, remote worker) without changing the agent.
Variants: Hermes routes every file tool and the shell through the environment's `execute()`, so files and commands see the same sandbox. DeepSeek swaps a capability seam. Cursor and OpenHands run tools on a worker the loop talks to. ACP puts execution in the client. OpenCodex's Remote Workspace is a separate experimental executor that never falls back to the hub filesystem. Ours has the node as the single execution place.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Hermes Agent | `terminal.backend` local (default), docker, ssh, modal, daytona, vercel_sandbox, plus plugin backends (singularity, sprites); the agent process stays on the host; terminal, `read_file`, `write_file`, `patch`, `search_files` and `execute_code` go through the environment's `execute()`. Docker keeps one long-lived container; SSH is recommended because the agent cannot modify its own code. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tools, https://hermes-agent.nousresearch.com/docs/user-guide/docker | reviewed |
| Hermes Agent | Plugin interface: a backend is a provider class (`is_available`, `create_environment`, `is_remote`, `is_container`, optional `probe`, `strip_env_keys`) plus an environment object (`execute(command, timeout)`, `cleanup`); plugins cannot shadow built-in names; background processes are marked `lost` after cleanup; refused under `plugins.isolation: host`. | GA | https://hermes-agent.nousresearch.com/docs/developer-guide/terminal-environment-plugin | unreviewed |
| Pi | Built-in tools accept pluggable operations (for SSH, containers and similar); extensions can replace built-ins by name; docs show isolation patterns where the credential proxy stays outside the sandbox. | GA | https://pi.dev/docs/latest/extensions, https://pi.dev/docs/latest/containerization | unreviewed |
| DeepSeek harness | Capability seams: swapping one provider (for example an SSH sandbox) changes bash, PTY and LSP together; the SSH backend keeps the harness local and runs execution remotely (page on GitHub, not on the docs site). | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams, https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/ssh.md | corrected |
| OpenClaw | The exec host can be the sandbox, the gateway or a paired node. | GA | https://docs.openclaw.ai/tools/exec-approvals | unreviewed |
| OpenHands | Swap the workspace argument to switch between local and remote execution; per-conversation Docker containers with an outer server proxying to an inner agent server; sandbox backends and trust boundaries confirmed in review; the FAQ states there is no built-in authentication, isolation or scalability for the single-user local setup. | GA | https://docs.openhands.dev/sdk/guides/agent-server/overview.md, https://docs.openhands.dev/openhands/usage/agent-canvas/backend-setup/docker-execution.md | reviewed |
| Cursor | Self-hosted workers: loop and inference in Cursor's cloud, edits, terminal, computer use and local MCP on the customer worker over one outbound HTTPS connection; no inbound ports. | GA (cloud); worker status not labelled | https://cursor.com/docs/cloud-agent/self-hosted, https://cursor.com/blog/self-hosted-machines | reviewed |
| Zed/ACP | The client performs fs I/O and runs terminals for the agent and applies its own policy to command, args and env. | GA | https://agentclientprotocol.com/protocol/v1/terminals.md, https://agentclientprotocol.com/protocol/v1/file-system.md | reviewed |
| OpenCodex | Remote Workspace: the hub runs the agent; an executor on a second machine supplies files from one approved root and, on Linux only, runs commands in bubblewrap over an outbound WebSocket with a one-time pairing code; read-only by default; a disconnected executor never falls back to the hub filesystem. | experimental | https://opencodex.me/guides/remote-workspace/ | corrected |
| rizzy-harness (ours) | The node runs the tools on the machine that owns the session, as the node's OS user; no choice of backend. | partial | `crates/tools/src/bash.rs:86-259`, `crates/cli/src/main.rs:447-451` | from own inventory |

#### PowerShell and Windows shell support
What it is: running the shell tool natively on Windows rather than through WSL.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `PowerShell` tool: auto-on on Windows without Git Bash, opt-in elsewhere (`pwsh` 7+); `-ExecutionPolicy Bypass` at process scope; a Bash deny rule on Windows with Git Bash silently turns PowerShell off unless explicitly enabled or separately scoped; profiles not loaded; no sandboxing on Windows. | preview | https://code.claude.com/docs/en/tools-reference.md | unreviewed |
| DeepSeek harness | `pwsh` on Windows. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | reviewed |
| Oh My Pi | Embedded bash needs no WSL on Windows. | GA | https://github.com/can1357/oh-my-pi | unreviewed |
| Pi | Bash via Git Bash by default; an optional `powershell` tool on native Windows (prefers pwsh.exe, no profile, non-interactive, execution policy bypassed unless admin-enforced). | GA | https://pi.dev/docs/latest/windows | unreviewed |
| goose | Default shell on Windows is `cmd`; `GOOSE_SHELL` overrides. | GA | https://goose-docs.ai/docs/guides/environment-variables.md | unreviewed |
| rizzy-harness (ours) | Not in the inventory (`bash -c`). | none | not in the inventory | from own inventory |

#### Shell-rc and terminal-emulator integration
What it is: hooking the agent into the user's own terminal or telling the terminal what the agent is doing.
Variants: goose adds shell aliases and per-terminal sessions. Claude Code emits a status protocol sequence for terminals that implement it. Pi documents terminal and tmux requirements. Warp is itself the terminal.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| goose | `goose term init <shell>` defines `@goose`/`@g` aliases; each terminal gets its own session unless `--name` attaches a persistent one; `--default` makes goose handle unresolvable commands (zsh, bash, Nushell); history capture off by default, `--dangerous-include-history` sends commands since the last question to the provider; `goose term info` shows context use. | GA | https://goose-docs.ai/docs/guides/terminal-integration.md | unreviewed |
| Claude Code | Program Status Protocol (OSC 7501) lets terminals show working, waiting or done (2.1.295, documented only in terminal-config.md). | GA | https://code.claude.com/docs/en/changelog.md | unreviewed |
| Pi | Needs extended-key support for Shift+Enter and Alt+Enter; tmux config (`extended-keys on`, csi-u for tmux 3.5+); lists working and non-working terminals; Termux supported without image paste. | GA | https://pi.dev/docs/latest/terminal-setup, https://pi.dev/docs/latest/tmux, https://pi.dev/docs/latest/termux | unreviewed |
| Warp | Blocks (command plus output, shareable, filterable), IDE-style input editor, completions; the agent is hosted in the terminal. | GA | https://docs.warp.dev/terminal/blocks/, https://docs.warp.dev/terminal/editor/ | unreviewed |
| rizzy-harness (ours) | None: web UI and admin CLI only, no TUI. | none | not in the inventory | from own inventory |

#### Monitor: background command output as conversation events
What it is: a tool that watches a command or socket and turns each output line into an event the model sees mid-conversation.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `Monitor` runs a command in the background and feeds each output line to Claude as an event, or opens a WebSocket where each text message is one event; every watch has a deadline (5 min default, 30 min max, 10 min in single-prompt `-p`); uses Bash permission rules; WebSocket URLs to private, link-local and cloud-metadata addresses are denied; added in 2.1.98, extended to Bedrock, Vertex, Foundry in 2.1.310. | GA | https://code.claude.com/docs/en/tools-reference.md, https://code.claude.com/docs/en/changelog.md | reviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Hosting other CLI agents inside a terminal
What it is: running a third-party agent CLI as a first-class thread or session inside the host app.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Zed/ACP | Terminal Threads host an agent CLI or TUI (Claude Code, Amp, OpenCode, Pi, Codex named) in a terminal listed as a thread; `agent.terminal_init_command` auto-types a command; bell rings produce notifications; closed threads are not kept in history; Zed profiles, permissions, skills and MCP settings do not apply. | GA | https://zed.dev/docs/ai/terminal-threads.md | reviewed |
| Warp | Hosts third-party CLI agents (Claude Code, Codex, OpenCode, Amp, Gemini, Kiro and others) with a rich input editor, review-comment injection and notifications. | GA | https://docs.warp.dev/_llms-txt/agents.txt | unreviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

## 3. Availability matrix

Legend: `Y` documented and released; `B` beta, experimental, preview or partial (every DeepSeek cell is `B` because the whole product is a developer preview); `P` planned or vendor-claim only; `-` nothing documented in the survey; `?` unverified or implied but unconfirmed. Cells come from the feature rows above, so a `-` means the survey found no page, not that the capability is absent. Area codes: A1 loop, A2 tools, A3 editing, A4 shell. Notes: Claude Desktop's Code tab shares Claude Code's engine, so its `-` cells mostly mean "nothing documented separately". OpenCodex is a gateway; its `B` cells are Remote Workspace, and its `Y` cells are the bounded web-search loop, the stall deadline and the synthetic search tool. Cursor's step-limit cell is `-` because its docs state there is no limit. Harnesses whose edit mechanism is undocumented (Pi, Antigravity, Cline `editor`, OpenClaw `edit`, goose, Cursor) are `-` in the edit-format rows.

| Feature | Codex | OpenCodex | OpenCode | Claude Code | Claude Desktop | DeepSeek | Oh My Pi | Pi | Cursor | Antigravity | OpenHands | OpenClaw | goose | Hermes | Zed/ACP | Warp | Cline | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A1 Loop structure, turns, stop reasons | Y | - | - | Y | Y | B | - | Y | - | - | Y | Y | B | Y | Y | Y | B | Y |
| A1 Loop placement relative to tools | B | B | Y | Y | Y | B | - | - | Y | ? | Y | Y | B | Y | Y | Y | Y | Y |
| A1 Step, turn and budget limits | Y | Y | Y | Y | - | - | - | - | - | - | - | Y | Y | Y | Y | - | Y | Y |
| A1 Stuck-loop and stalled-run detection | - | - | ? | Y | - | - | - | - | - | - | Y | Y | B | - | - | - | - | - |
| A1 Interrupting a running turn | Y | - | - | Y | - | B | Y | Y | - | - | B | Y | Y | Y | Y | - | B | B |
| A1 Steering and queueing while a turn runs | Y | - | Y | Y | - | B | Y | Y | Y | - | ? | Y | Y | Y | Y | Y | - | - |
| A1 Parallel tool calls | Y | - | - | Y | - | - | - | Y | - | - | B | - | - | Y | - | - | - | - |
| A1 Goal and objective-continuation loops | Y | - | B | Y | - | B | Y | - | B | Y | Y | B | - | Y | - | - | - | - |
| A1 Time-driven re-submission loops | - | - | - | Y | - | - | Y | - | - | - | - | - | - | Y | - | - | - | - |
| A1 Retry, backoff, stream-failure handling | Y | Y | - | Y | - | - | - | Y | - | - | - | Y | - | - | - | - | Y | - |
| A1 Multi-model turn strategies | - | - | - | - | - | - | Y | - | - | - | - | - | - | Y | - | - | - | Y |
| A1 Event log as source of truth, one writer | - | - | Y | Y | - | B | Y | Y | - | - | Y | Y | B | - | - | - | - | Y |
| A1 Injected context and message provenance | - | - | - | Y | - | B | - | - | - | - | - | - | - | - | - | - | - | B |
| A1 Evaluator outside the main loop | Y | - | - | - | - | - | - | - | - | - | B | - | - | - | - | - | - | - |
| A1 Tool-call execution pipeline | - | - | - | - | - | B | - | Y | - | - | Y | - | - | Y | - | - | - | Y |
| A1 Deliberate turn-ending semantics | - | - | Y | Y | - | B | - | Y | - | - | Y | Y | - | - | - | - | Y | - |
| A2 Built-in tool inventory published | B | - | Y | Y | - | B | Y | Y | Y | B | Y | Y | B | Y | Y | - | Y | Y |
| A2 Tool-set composition (defaults, presets, gating) | Y | - | Y | Y | - | B | Y | Y | - | - | Y | Y | Y | Y | - | - | Y | Y |
| A2 Deferred tool loading (tool search) | Y | - | - | Y | - | - | Y | Y | - | Y | - | Y | B | Y | - | - | - | - |
| A2 Model-written scripts that call tools | B | - | Y | - | - | B | B | B | - | - | - | - | B | Y | - | - | - | - |
| A2 Ask-the-user tool | B | - | Y | Y | - | B | Y | - | Y | - | - | Y | - | Y | Y | Y | Y | B |
| A2 Todo and task-list tool | Y | - | B | Y | - | B | Y | - | - | B | Y | - | B | Y | - | Y | - | - |
| A2 Web fetch and web search tools | Y | Y | Y | Y | - | B | Y | - | Y | - | - | Y | - | Y | Y | Y | Y | B |
| A2 Code-navigation tools (LSP, AST, outline) | - | - | - | Y | - | B | Y | - | - | - | - | - | Y | - | Y | - | - | Y |
| A2 Tool result size limits and spill | Y | - | Y | Y | - | B | - | Y | - | - | - | - | Y | Y | - | - | - | Y |
| A2 Internal URL schemes in file tools | - | - | - | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - |
| A2 Tool risk classification metadata | - | - | Y | Y | - | - | Y | - | - | - | Y | - | - | - | Y | - | - | Y |
| A3 Edit format: exact search/replace | - | - | Y | Y | - | B | - | - | - | - | Y | - | - | Y | Y | - | - | Y |
| A3 Edit format: patch (apply_patch) | Y | P | Y | - | - | - | - | - | - | - | ? | Y | - | - | - | - | Y | - |
| A3 Edit format: hash-anchored lines | - | - | - | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - |
| A3 Edit format: structural (AST) | - | - | - | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - |
| A3 Whole-file write and create | - | - | Y | Y | - | B | Y | Y | Y | - | Y | Y | - | Y | Y | - | - | Y |
| A3 Model-routed choice of edit tool | - | - | Y | B | - | - | - | - | - | - | - | - | - | - | - | - | Y | - |
| A3 Failed-edit handling and staleness guards | - | - | Y | Y | - | B | Y | B | - | - | B | - | - | B | - | - | - | B |
| A3 Encoding and line-ending safety | Y | - | - | Y | - | - | - | - | - | - | - | - | - | Y | - | - | - | - |
| A3 Post-edit diagnostics and formatters | - | - | Y | Y | - | - | Y | - | - | - | - | - | - | Y | - | B | - | - |
| A3 Diff review and in-app file view/edit | Y | - | - | Y | Y | - | - | - | Y | Y | Y | Y | - | - | Y | Y | - | B |
| A3 Write confinement and protected paths | Y | - | - | Y | - | - | - | - | Y | - | - | - | - | Y | - | - | - | Y |
| A3 Read-tool capabilities and limits | - | - | Y | Y | - | B | Y | - | - | - | - | - | - | Y | Y | - | - | Y |
| A3 File mentions and attachments | - | - | Y | - | Y | - | - | Y | - | Y | - | - | Y | Y | - | Y | Y | - |
| A3 Notebook support | - | - | - | Y | - | - | Y | - | - | B | - | - | - | - | - | - | Y | - |
| A3 Git conflict resolution via virtual paths | - | - | - | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - |
| A3 Debug mode with log instrumentation | - | - | - | - | - | - | - | - | Y | - | - | - | - | - | - | - | - | - |
| A3 Inline selection edits / edit prediction | - | - | - | - | - | - | - | - | - | - | - | - | - | - | Y | - | - | - |
| A3 Per-session scratch directory | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| A3 Office-document deliverables | - | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - |
| A4 Shell execution model | Y | - | Y | Y | - | B | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y |
| A4 Command timeouts and auto-backgrounding | - | - | Y | Y | - | B | - | - | - | - | - | - | - | - | - | - | - | Y |
| A4 Background processes | Y | - | Y | Y | - | B | Y | - | ? | Y | B | Y | - | Y | - | Y | B | - |
| A4 Agent-driven interactive terminal (PTY) | Y | - | - | - | - | B | Y | - | - | - | - | - | - | Y | Y | Y | - | - |
| A4 User-facing terminal pane in the app | Y | - | Y | - | Y | - | - | - | - | - | Y | Y | - | - | - | Y | - | - |
| A4 User shell escape (! commands) | Y | - | Y | Y | - | - | Y | Y | Y | - | - | - | - | Y | - | Y | - | - |
| A4 Shell output limits and spill | - | - | - | Y | - | B | - | Y | ? | - | - | - | - | Y | Y | - | - | Y |
| A4 Command-output filtering and minimisation | - | - | - | - | - | - | Y | - | - | - | - | B | - | - | - | - | - | Y |
| A4 Environment given to spawned commands | Y | - | - | B | - | B | - | Y | Y | - | - | - | Y | Y | - | - | - | Y |
| A4 Resource limits for commands | - | - | - | Y | - | - | - | - | - | - | - | - | - | Y | - | - | - | - |
| A4 Pluggable execution backends | - | B | - | - | - | B | - | Y | Y | - | Y | Y | - | Y | Y | - | - | B |
| A4 PowerShell and Windows shell support | - | - | - | B | - | B | Y | Y | - | - | - | - | Y | - | - | - | - | - |
| A4 Shell-rc and terminal-emulator integration | - | - | - | Y | - | - | - | Y | - | - | - | - | Y | - | - | Y | - | - |
| A4 Monitor: command output as events | - | - | - | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| A4 Hosting other CLI agents in a terminal | - | - | - | - | - | - | - | - | - | - | - | - | - | - | Y | Y | - | - |

## 4. Common across the ecosystem

Counted over the 16 real harnesses (OpenCodex excluded as a gateway; Claude Desktop and Zed/ACP included), as documented-and-released `Y` plus partial `B`, and at least 8 of 16. Counts come from the matrix and carry its caveat: `-` means nothing documented.

| Feature | Y + B of 16 | Note |
|---|---|---|
| Shell execution model | 15 (13 + 2) | Universal; the difference is process per command versus persistent shell. |
| Built-in tool inventory published | 14 (10 + 4) | Codex, Antigravity, OpenHands, goose, DeepSeek and Warp give partial or no list. |
| Loop placement relative to tools | 13 (10 + 3) | See the entry: opposite answers to our "node runs the loop". |
| Loop structure, turns and stop reasons | 12 (9 + 3) | |
| Steering and queueing while a turn runs | 12 (11 + 1) | Default queue versus default steer differs. Ours has none. |
| Interrupting a running turn | 11 (8 + 3) | |
| Tool-set composition (defaults, presets, gating) | 11 (10 + 1) | |
| Ask-the-user tool | 11 (9 + 2) | Ours uses `ui:` blocks. |
| Web fetch and web search tools | 11 (10 + 1) | |
| Background processes | 11 (8 + 3) | Pi has it as a deliberate non-feature. Ours has none. |
| Todo and task-list tool | 10 (6 + 4) | Claude Code withdrew it for newer models. |
| Goal and objective-continuation loops | 10 (6 + 4) | Every one of them was added recently; mechanisms differ (who judges completion). |
| Whole-file write and create | 10 (9 + 1) | |
| Diff review and in-app file view or edit | 9 (9 + 0) | |
| Step, turn and budget limits | 8 (8 + 0) | Two of the eight are only a protocol stop reason (Zed/ACP) and a mistake limit (Cline). |
| Event log as source of truth with one writer | 8 (6 + 2) | |
| Deferred tool loading | 8 (7 + 1) | On by default in Claude Code, Hermes, OpenClaw, Antigravity. |
| File mentions and attachments | 8 (8 + 0) | |
| User shell escape (`!`) | 8 (8 + 0) | |

Edit formats are not in this list because six harnesses do not document how their edit tool matches text. Among those that do, exact search/replace appears in 6 (Claude Code, OpenCode, DeepSeek, OpenHands, Hermes, Zed), patch format in 4 (Codex, OpenCode, OpenClaw, Cline), and hash-anchored lines in 1.

## 5. Only one harness has it

| Feature | Harness | Why it is or is not interesting for us |
|---|---|---|
| Hash-anchored line edits | Oh My Pi | Interesting: edit failure is a cost driver and a hash mismatch is a cheap staleness check. The only evidence is the maintainer's own benchmark (16 models), not independent, and the omp docs pages do not even describe it. |
| Structural AST edits with staged accept | Oh My Pi | Interesting for large refactors; not transactional on stale previews; lower priority than basic edit guards. |
| Internal URL schemes in file tools | Oh My Pi | Interesting as a way to avoid adding tools (fewer schemas per step); the README and the docs disagree on the list. |
| Git conflict resolution by writing to `conflict://N` | Oh My Pi | Niche but cheap; a good example of replacing a tool with a path convention. |
| `Monitor` (background output as conversation events) | Claude Code | Interesting for a node: it turns a log tail or CI poll into events with a mandatory deadline. Other harnesses only notify on completion (OpenCode, Hermes). |
| Per-session scratch directory with no prompts | Claude Code | Cheap, and ours has a per-session TMPDIR only on a branch. |
| Per-message `priority` (`next`, `later`, `now`) and an interrupt receipt listing still-queued messages | Claude Code | Interesting protocol detail for a web UI that can lose its connection. |
| Message provenance typing (`origin`) | Claude Code | Interesting because injected text and human text are separated by type, with the forgeable fields stated. |
| Memory cgroup for all commands | Claude Code | Interesting for a node that runs untrusted model output; Linux only and falls back to uncapped. |
| Foreground command moved to background at timeout instead of killed | Claude Code | A behaviour change that avoids lost dev-server runs; ours kills. |
| Asynchronous safety monitor that can pause or end a task | Codex | Unusual; the task simply ends on CLI and mobile, so the user loses work. |
| Default env pass-through of `*KEY*`, `*SECRET*`, `*TOKEN*` | Codex | A counter-example: `ignore_default_excludes` defaults to true. Ours starts from an empty env. |
| Model-routed edit tool (`editor` or `apply_patch`) | Cline | Interesting if we ever support GPT-family models; otherwise skip. |
| Debug mode that inserts and removes logging | Cursor | Editor-specific; skip. |
| Inline Assistant and Edit Prediction | Zed | Editor-native; not relevant to a web dashboard. |
| Office-document deliverables | Claude Desktop (Cowork) | Outside coding; skip. |
| Mixture of Agents turn | Hermes Agent | Vendor benchmark only; cost multiplier; skip. |
| Prewalk (strong model starts, cheap model finishes) | Oh My Pi | Interesting cost lever for a master that routes models; once per session. |
| Silent finish (`NO_REPLY`) and tool-free finalisation pass | OpenClaw | Relevant only for channel-style assistants. |
| Goal objects with compare-and-set revisions and human-authorised resume | DeepSeek harness | Good fit for an event-sourced log; preview and survey-only. |
| Programmatic tool calling inside the same approval pipeline | DeepSeek harness | The only design that says sub-calls go through approvals and are logged individually. Others document the sandbox, not the approval path. |

## 6. Ours against the field

Ranked by how much it matters for a self-hosted master/node harness. Feature names refer to the entries above.

1. **No mid-turn steering or queueing (Steering and queueing).** 12 of 16 harnesses accept input during a turn; ours rejects it with "a turn is already running". For a web dashboard driving a remote node this is the biggest gap: the user's only recourse for a wrong turn is cancel. The cheapest design from the field is Claude Code's or OpenClaw's: store the message first, deliver it at the next step boundary, and record it as an event.
2. **Interruption is partial (Interrupting a running turn).** The loop never abandons a running tool future, so a non-bash tool that ignores cancel can block the turn; the fix lives only on `integration/gate3`. Hermes abandons the call and adds no partial response; ACP specifies the exact `cancelled` handshake. On a node that people cannot walk up to, a wedged turn is an outage.
3. **No retry, no backoff, no stuck detection, one fixed limit (Retry; Stuck-loop; Step limits).** Provider failure ends the turn unless a fallback model exists; there is no repetition detector (OpenHands has four patterns on by default); the 50-step cap is not configurable and there is no token or cost budget (the `budget` field is a stub). Worst case is 50 calls at up to 600 s each. Claude Code's unattended retry watchdog and budget cap, and Codex's goal token budget, are the references.
4. **Shell without background processes, PTY, timeout-to-background or user escape (Background processes; Timeouts; PTY; `!` escape).** 11 of 16 have background processes. On `main` an agent cannot run a dev server or watcher without blocking until the timeout kills it. Pi's counter-argument (use tmux) only works where the user is present. No terminal pane in the UI either.
5. **Edit tooling is exact-replace only (Edit formats; Failed-edit handling; Post-edit diagnostics; Diff review).** No staleness or read-before-edit guard in the inventory (Claude Code and DeepSeek have one; Oh My Pi's hashes make it implicit), no patch tool, no post-edit diagnostics, and the UI shows only the old and new strings of one call. The node already snapshots each turn into a shadow repo, so a stale-file check and a whole-turn diff are cheap additions. Hashline is promising but its evidence is the maintainer's own benchmark.
6. **Tool set is thin and not deferred (Tool inventory; Deferred tool loading; Todo; Ask-the-user).** All allowed schemas are sent each step (the plan's on-demand loading is docs-only); no todo tool, no ask tool (the `ui:` blocks are arguably a better fit for a web UI, and validated in Rust), no code navigation beyond `outline`, and agentic sessions have no web tools. Deferred loading matters only once MCP arrives; today the cost is small.
7. **Sequential tool calls (Parallel tool calls).** Only 4 of 16 document parallel calls, mostly read-only. Ours is a defensible choice given per-call approvals; low priority.
8. **Where we are ahead.**
   - Command output: a filter (ANSI, progress lines, passing tests, repeats) plus a retrievable full copy with per-session random ids (Command-output filtering; Shell output limits). Claude Code and DeepSeek spill to a file or locator; Oh My Pi's minimizer and OpenClaw's Tokenjuice plugin also compact output, but the survey documents no mechanism for either.
   - Environment: an empty env plus allow-list with provider keys always stripped (Environment given to spawned commands). Codex passes secrets by default; DeepSeek and Hermes scrub; no harness in the survey is documented as starting from an empty environment.
   - File-tool confinement in the tool itself: per-component lstat, no-follow final link, refusing `.git` and the data directory (Write confinement). Claude Code confines by directory, Codex and Hermes block named paths.
   - Event log with one writer, contiguous `seq`, and a restart that closes the turn as `interrupted` (Event log). OpenClaw and DeepSeek are the only others that state a writer rule.
   - A node that is the authority plus crash isolation of the backend task. The closest analogues in these areas are OpenClaw (a gateway with paired nodes), Cline (hub and spokes) and OpenCode (one server per machine); Cursor's workers and Claude Code's self-hosted runner invert it by keeping the loop in the vendor cloud.
   - Caveat: the shell has no sandbox on `main` (Landlock only on a branch), which the permissions area covers; do not read the output-and-env strengths as containment.

## 7. Corrections, contradictions and unverified claims

What the reviews changed (only items in these areas):

- Claude Code: review 1 confirmed the Bash timeout, background, memory-cgroup, Read, Glob/Grep, WebFetch, WebSearch and Monitor entries and found no correction in Areas 1-4 (the unchecked remainder was sampled and "looked reliable"). Review 3 corrected the self-hosted runner description: the control plane sends each session's tool list and permission rules with the work, and also model, connectors and lifecycle hooks; it is not just "policy".
- Hermes Agent: the agent-loop page does not use the word "synchronous"; the iteration budget (500, `agent.max_turns`) is confirmed. The 50 versus 250 subagent cap disagreement between pages is real.
- OpenClaw: the writer-claim entry was corrected (the commit transaction verifies the claim; the state-directory lock is not on the queue page); the 2-minute stuck threshold applies only with diagnostics enabled and 5 minutes is a floor.
- Cline: roles in the hub are per session participant, not per client; the no-authentication statement for the hub was verified.
- goose: tool-output unit is characters; tool deferral mechanism is described on the config-files page. The review also found the sandbox "nothing documented" claim wrong in another area, so treat every goose "nothing documented" (including edit mechanism) as needing a second look.
- Pi: the survey's claims that failed codemode scripts do not roll back and an output cap exist were not found on the page and were removed from the verified set.
- Oh My Pi: hashline numbers match the blog, which says 15 models in the title and 16 in the body, is dated 2026-02-12 and is written by the omp maintainer.
- OpenCodex: "runs no agent loop" is a reading of the how-it-works and architecture pages, not a quoted claim; Remote Workspace details (pairing rate limit, outbound connection) were corrected.
- DeepSeek harness: the SSH page is on GitHub, not on the docs site; one-process shape is an inference; the last 25,000 characters of the generated tool catalogue were not read, so default-mounted tool sets and defaults are unconfirmed.
- Antigravity: Auto-mode and sandbox "Preview" details could not be confirmed; its `/goal` entry was confirmed.

Contradictions inside vendors' own pages:

- Pi: the CLI page says `codemode` and `tool_search` are off by default; the settings page says on; the MCP page says MCP tools default to `codemode` exposure. Not reconciled without a run.
- Oh My Pi: README says 31 tools and different coreutil counts in different places; `ast_grep` is off in the docs but a headline tool in the README; README lists a `skill://` scheme that the skills page says it does not document; `chillax` keyword in README only. Default approval mode is `yolo`.
- Codex: `Code Mode` is "under development and off by default" in the config reference while releases ship user-facing Code Mode features; the app-server is "experimental and not supported for production" yet carries every client; no tools reference page exists.
- OpenClaw: Tool Search is listed under experimental features but described as on by default.
- Hermes Agent: Tool Search is called opt-in yet the default `auto` behaves as on; the architecture page says seven terminal backends in a diagram and six in prose.
- Cline: docs mention legacy tool names (`write_to_file`, `read_file`) next to the new names.
- Claude Code: the weekly digest stops at Week 37 and lags the changelog; `TaskOutput` was deprecated in 2.1.83 and removed in 2.1.277; task tools were withdrawn on newer models in 2.1.233; `taskOutputMaxChars` lived about two weeks.
- goose: docs still name the server `goosed` in places; the loop was replaced in v1.55.0 and v1.58.0 so the GDK pages may lag.
- Antigravity: the slash-command catalogue calls itself complete but omits commands; no tools reference exists.

Not confirmed by anyone (unverified):

- Claude Code: the exact list `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` removes and the read-only Bash allowlist (the docs say to print them with a CLI command); how the server-side probe of tool results works.
- Cursor: terminal background processes, timeouts and output truncation (not on the terminal page).
- OpenCode: how the `/goal` reviewer works; what `doom_loop` did in V1.
- OpenHands: the stuck detector's halt behaviour; send-message-while-running, pause and resume mechanics (index titles only); `ApplyPatchTool` GPT-5 preset (index only).
- Pi: edit matching rules, read limits and grep/find implementation (no tools reference page located).
- Oh My Pi: the hashline tool schema and fallback; `eval` kernel isolation and the tool-callback bridge; whether bash can run over SSH.
- goose: the Developer extension tool names and edit mechanism; `--max-tool-repetitions` semantics.
- DeepSeek harness: default tool presets and sandbox mode; the exact signatures of the steering interception.
- Codex: the full built-in tool list and each tool's parameters; remote executors and `codex exec-server`.
- Warp: any public tool list; checkpoints or undo for agent edits.

## 8. Questions for Round 2

Only source code can settle these. Closed products (Claude Code, Claude Desktop, Cursor, Antigravity) are omitted except where a CLI command would settle it.

- **Codex, parallel read-only tools and `turn/steer`:** which tools count as read-only, how parallelism is scheduled, and at which point of the loop a steered message is injected relative to running tool calls.
- **Codex, `apply_patch`:** the patch grammar, how a failed hunk is reported to the model, and how CRLF preservation is implemented.
- **Codex, unified exec:** process lifetime, process-count limit, output buffering per `write_stdin` poll, and what `tool_output_token_limit` truncates.
- **Codex, Code Mode:** what the real default is, and how nested tool calls interact with approvals.
- **OpenCode, steering and inbox:** where a steered message enters the loop and what priority and cancellation semantics the inbox implements.
- **OpenCode, edit errors:** how "closest lines" are computed, and whether any fuzzy matching exists.
- **OpenCode, `/goal`:** how the reviewer works, and what `doom_loop` was in V1 and whether V2 has any repetition guard.
- **OpenCode, shell scanner:** how commands are extracted and why unparseable input is refused.
- **DeepSeek harness, pipeline and steering:** how pre-step interception and steering claims work at a step boundary; how monotonic guards are enforced; which tools each preset mounts; whether loop and web server share a process.
- **DeepSeek harness, filesystem policy:** what the default is when the policy plugin is absent, and how version freshness is computed.
- **DeepSeek harness, spill:** thresholds for `maxInlineTokens` and retention.
- **Oh My Pi, hashline:** the tag format, how hashes are computed, behaviour on a duplicated line or a deleted line, and what `sloppy` is.
- **Oh My Pi, steering:** what a "safe step boundary" means in code.
- **Oh My Pi, `eval` and minimizer:** kernel isolation, the loopback tool bridge, and the output minimizer rules.
- **Pi, `edit`:** the matching rules; and how parallel execution, `terminate`, and `withFileMutationQueue` are implemented; plus what the real default for `codemode` is.
- **Pi, output truncation:** whether the 50 KB or 2,000 line limit is enforced by the harness or only by convention.
- **OpenHands, stuck detector:** what the agent does after a detection and whether it is configurable.
- **OpenHands, `file_editor` and parallel execution:** failure modes of `str_replace`, and how `tool_concurrency_limit` serialises shared state.
- **OpenClaw, queue and writer claim:** how `steer` is injected into an active run, and how the `activeWriterRunId` check is done in the commit transaction.
- **OpenClaw, loop detection:** the thresholds on the built-in-tools config page that nobody opened.
- **goose, state machine:** how cancellation interacts with in-flight tool calls and what `--max-tool-repetitions` does; the Developer extension's tool list and edit mechanism; how the deferral and Code Mode caps are enforced.
- **Hermes Agent, interruption and parallelism:** how an interrupt interacts with the tool thread pool and why foreground terminal commands move to the background; which of 50 or 250 is the real subagent cap.
- **Hermes Agent, `patch`:** the fuzzy matching algorithm and its failure behaviour; how the `&`, `nohup` and `disown` rewriting works.
- **Hermes Agent, execution backends:** how the environment `execute()` contract is enforced across file tools.
- **Zed, queue and tools:** how Steer is delivered to the native agent at a step; `edit_file` matching and failure behaviour; how the terminal tool applies the hardcoded rules.
- **Warp (open client), Full Terminal Use:** how the agent reads and writes the terminal buffer, and how a user takeover is signalled.
- **Cline, `editor` and hub:** the edit tool's matching rules; how the hub routes capability requests; the `requires_approval` check and the retry and mistake-limit counters.
- **OpenCodex, sidecar and Remote Workspace:** how the bounded search loop hands control back to the client, and how the executor enforces its approved root.
- **Claude Code (CLI, if run):** `claude auto-mode defaults` and the other documented print commands would settle the read-only Bash allowlist and the env-scrub list.
