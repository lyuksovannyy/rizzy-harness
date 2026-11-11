### The agent loop, tool dispatch and durability

From [round2/synth/01-loop-dispatch-durability.md](round2/synth/01-loop-dispatch-durability.md).

Cell labels follow the project subsections. "None" means the files read report none (and where a reviewer confirmed the absence, the cell says so). "n/r" means not in the code that was read.

##### 3.1 Where the loop lives, and limits

| Project | Loop lives in | Step or turn cap (default) | Wall-clock or idle limit | Per-tool deadline | Provider retry |
|---|---|---|---|---|---|
| Codex | Rust; `run_turn` in the engine, in process (TUI, `exec`) or in a long-lived app-server daemon | None (absence confirmed); optional rollout token budget; reviewer-denial breaker | Stream idle 5 min; connection loss retried forever with a notice | Unified exec yields 250 ms to 30 s per call; background terminals 300 s default | 5 stream retries, 200 ms doubling, plus or minus 10% jitter; server advice first |
| OpenCode | TypeScript (Effect); `drain` in one server process per machine | `agent.steps` unset by default; when set, `toolChoice: none` plus refusal at dispatch | None reported | None in the loop | 10 retries, 2 s doubling to 10 s, jitter, retry-after up to 15 min; separate 1 h no-network budget |
| OpenClaw | TypeScript; `runLoop` in a Gateway process, or in a worker that dials the Gateway | None | 48 h per run; 120 s model-stream idle | n/r | n/r in the files read |
| Hermes Agent | Python; in the gateway owner process per profile | None (`max_turns: None`); sub-agents 250; 50 web searches and 50 sub-agents per turn | Cron inactivity watchdog 600 s | Foreground shell over 600 s promoted to background | 3 attempts, jittered, `Retry-After` honoured (heading only) |
| Oh My Pi | TypeScript plus Rust crates; in process, one session per process | None reported | None reported | n/r | 10 retries, 500 ms base, backoff capped 8 s, up to 25% jitter |
| Pi | TypeScript; in process; shipped path has no server | None (absence confirmed) | None | None in the loop; per-tool | Agent level 3 tries, 2 s doubling to 60 s, no jitter; provider level 0 |
| DeepSeek harness | TypeScript plugin composition in one host process | None (absence confirmed) | None | n/r | Per-provider policy, jitter, long `Retry-After` fails fast |
| goose | Rust; state machine in the process serving the session | 1000 assistant turns, then a canned "continue?" message | None in the loop | Shell 300 s default, model sets it, 0 disables | 3 retries, 1 s doubling to 30 s, jitter 0.8 to 1.2; not mid-stream |
| Cline | TypeScript SDK in process; hub is a local WebSocket host | None by default (SDK option `maxIterations`) | CLI `--timeout` is a host timer | `timeoutMs` metadata never read by the loop; shell 30 s | 3 retries, 1 s doubling to 15 s, no jitter; not after output streamed |
| OpenHands | Not in this repository (server SDK) | Client sends 500 and `stuck_detection: true` | n/r | n/r | n/r |
| Zed | Rust, in the editor process | n/r | n/r | Terminal output 16 KiB cap | Strategy by error kind: retry-after or backoff; 3 fixed for read/send errors |
| Warp | Server side; client executes tools | None client-side | n/r | n/r | 3 recovery attempts per request; fresh budget per tool-result round trip |
| OpenCodex | No agent loop; bounded helper loops inside a request | Web search 3 per turn, hard cap `maxSearches + 3` | Upstream stall 300 s (public upstreams) | 60 s per search | 3 attempts per logical request, charged to a workflow-root counter |

Evidence per row is in the project subsections. Sources: Codex `core/src/session/turn.rs`; OpenCode `session/runner/llm.ts`; OpenClaw `agent-loop.ts`; Hermes `agent/iteration_budget.py`; Pi `agent-loop.ts`; goose `ops_maxturns.rs`; Cline `agent-runtime.ts`; OpenHands `agent-server-adapter.ts`.

##### 3.2 Loop detection

| Project | Exists | On by default | Trigger and action | Where wired |
|---|---|---|---|---|
| Codex | No (absence confirmed) | n/a | Only a reviewer-denial breaker and a token budget | n/a |
| OpenCode | No in the step loop (absence confirmed; only the V1 `doom_loop` key). `/goal` reviewer has a stall counter | `/goal` only | 3 stalls of 50 rounds pause the goal | `plugin/goal.ts` |
| OpenClaw | Yes, rolling detector over 30 calls with outcome hashes; plus an identical-error breaker | Rolling: off. Error breaker: on | Warn 10, block 20, circuit breaker 30; third identical error ends the run | Admission before dispatch; fails closed if the check throws |
| Hermes Agent | Yes, signatures of tool, args hash and result; cycles of 2 to 4 over 64 entries; failure counters | Yes; warn only when attended, hard stop forced on for gateway and cron | Notice at 3rd repeat; exact-args failure warn 2, block 5; same-tool failure warn 3, halt 8 | Call site not traced (author note) |
| Oh My Pi | Only identical no-op edits (3 in a row) and stream rules fire-once | Yes | `NoChange` rejection after 3 | Edit tool |
| Pi | No | n/a | None | n/a |
| DeepSeek harness | Reminder only, identical consecutive calls | Yes (base bundle) | Notices at 3, 5, 8; never blocks; A, B, A, B not detected | `tools/post-execute` listener |
| goose | Exists but dead: `RepetitionInspector` has no limit, counts on a clone, flag not passed (checked in review) | No | None effective | Registered with `None` |
| Cline | Yes: mistake counter (SDK 6, CLI 3) and identical-call detector | Yes by code (docs say off) | 3rd identical call adds a notice; 5th forces the mistake limit, which asks the host callback | Runs from the `tool-started` event, after the call was issued |
| OpenHands | `stuck_detection: true` always sent; detector n/r | Yes | n/r | Server side |
| Zed, Warp | n/r | n/r | n/r | n/r |
| OpenCodex | Bounded helper loops only | n/a | Hard cap on its own search loop | `web-search/loop.ts` |

##### 3.3 Input during a turn

| Project | Steer, queue or reject | Where accepted input is stored | Persisted before acknowledgement | Survives a crash |
|---|---|---|---|---|
| Codex | `turn/steer` queued, drained at the next loop iteration (after in-flight calls); separate SQLite queue for idle dispatch | Steer: not described (unchecked). Queue extension: SQLite | Queue extension: yes; steer: unknown | Queue: yes; steer: not established |
| OpenCode | Inbox "promoted" at step boundaries | Not described | Not described | Not described |
| OpenClaw | Modes steer (default), followup, collect, interrupt; steer falls back to follow-up; cap 20, `summarize` | Channel messages: SQLite `channel_ingress_events` (unique key). `agent` turns: staged `session_pending_inputs` row. In-flight steering: in-memory queue | Yes for channel and `agent` paths; ACK sent after the claim | Yes for the SQLite paths; the in-memory steer queue only covers a failed run (corrected in review) |
| Hermes Agent | Intent `queue`, `steer` or `redirect` on a durable FIFO, 32 per session | SQLite `session_admissions` | Yes (row committed first, unique key) | Yes; a started-but-unsettled row becomes `unknown`, never replayed |
| Oh My Pi | RPC has `steer` and `follow_up`; steering is cooperative | Not described | Not described | Not described |
| Pi | Steering and follow-up queues, one at a time; `prompt()` while running throws | In-memory `PendingMessageQueue` | No (inferred) | No (inferred) |
| DeepSeek harness | Claimed from an inbox at each pre-step | Log (`user/message`, `agent/inbox/spliced`) once spliced; inbox partly process-local | Idempotent by `requestId` found in the log after splice | Only once spliced into the log (inferred) |
| goose | Rejected: a second run for the session returns `AgentRunExists` | n/a | n/a | n/a |
| Cline | `run()` while running throws; no steering found | n/a | n/a | n/a |
| OpenHands | Server side n/r; client sends over socket, falls back to HTTP, or holds in browser memory | Browser memory (Cloud, paused sandbox) | No for held messages | No for held messages |
| Zed, Warp, OpenCodex | n/r | n/r | n/r | n/r |

##### 3.4 What is durable before a tool runs, and what a restart does

| Project | Durable before a tool body runs | Flush rule | Turn running at crash |
|---|---|---|---|
| Codex | The model's tool-call item (recorded when its output item completes); no separate intent event; approval requests and begin events are not persisted | Writer flushes after every line | Not described for a crash; planned daemon restart writes `loaded-threads.json` and local turns can auto-continue |
| OpenCode | `Execution.Started` plus a claim row, in one transaction with the event; no per-tool intent | Event committed in the same transaction as projections | Boot sweep re-drains the turn with a synthetic "continue without repeating completed work" message; abort after more than 10 attempts; at-least-once |
| OpenClaw | Accepted input row; writer claim; no per-tool intent for in-process tools | CAS claim, fenced write inside the transaction | Fixed resume prompt, "check state before repeating"; lost worker: "effects may be unknown", never auto-continue; cron retries only if delivery had not started |
| Hermes Agent | Admission row `started` with epoch and generation; outbound reply obligation `attempting` before send | Claim and settle in one write transaction; settle names epoch and generation | Marked `unknown`, never requeued; person decides; unattended turns auto-settle `interrupted` |
| Oh My Pi | The appended message line | Synchronous append on the hot path; failure latches | Not described |
| Pi (shipped) | The user prompt, and each message on `message_end`; no intent | Append per finished message | Partial assistant message lost; dangling tool call not repaired (inferred) |
| Pi (pi-durable, experimental) | A checkpoint `{phase: execute, arguments, replay}` before the tool runs | Transactional commit per step | Running task goes back to `pending` with `restarts + 1`; tool re-run only if replay policy is `safe`, else "may have partially run" |
| DeepSeek harness | Every event up to the tool request, flushed first | Awaited flush before each model request, each top-level tool body, each pre-step; 200 ms write-behind otherwise | Resume closes the open turn: synthetic "outcome unknown" results, `turn/end` reason `interrupted`; nothing re-run |
| goose | Stored messages; approval decision persisted as a message before it takes effect | SQLite `BEGIN IMMEDIATE` per message | Re-derive from the conversation; unanswered confirmation continues; stale approvals expire with the extension lease |
| Cline | Assistant message in a JSON file rewritten whole (no atomic rename) | None | Not described; a crash mid-write can truncate the transcript |
| OpenHands, Warp, Zed, OpenCodex | n/r | n/r | n/r |

##### 3.5 Parallel calls, cancel and truncation

| Project | Parallel tool calls | Cancel of a running tool | Output truncation, and where it is enforced |
|---|---|---|---|
| Codex | Per-tool opt-in to a read lock; others exclusive; started while the stream continues | Aborts admission wait and handler | Per poll 10,000 tokens, 1 MiB per process, head and tail; recorder truncates old output |
| OpenCode | Each call forked immediately, no cap | Cooperative `context.cancel`, wait, then interrupt; plain decline is a defect | Bounded once at call time; 3 `length` stops fail the step |
| OpenClaw | Default parallel; sequential batches skip the tail on steering | `interrupt` aborts and waits for the writer to settle | 30% of window and 16k, 32k or 64k characters per result |
| Hermes Agent | 8 workers; prompts serialised; human wait excluded from deadline | Pending futures cancelled; running tools get 3 s | Spill to disk with per-result and per-turn budget |
| Oh My Pi | Shared by default, exclusive on resolver failure; steering aborts only `interruptible` tools | SIGTERM tree, grace, re-walk, SIGKILL | 50 KiB spill, head 20 KiB plus tail 20 KiB, artifacts 16 MiB |
| Pi | Preparation serial, execution `Promise.all` | One `AbortSignal`; process-tree kill; steering never interrupts | Each built-in tool (2000 lines, 50 KB); loop applies no cap |
| DeepSeek harness | Only when `isConcurrencySafe(args)` is exactly true; max 10 | Started bodies drain; skipped calls get synthetic results | Spill above 12,500 tokens, fails open |
| goose | Dispatched in order, result streams merged (see Section 7) | Race with a cancel token, drop the future, back-fill "interrupted" results | Shell 2,000 lines or 50,000 bytes, tail; collection unbounded; 200,000 characters per block |
| Cline | `Promise.all` for `parallel` tools; `run_commands` batch concurrent under one approval | One `AbortController`; group SIGKILL | 48,000 characters at the executor; 8,000 or 50,000 at request build; cache URI |
| OpenHands, Zed, Warp, OpenCodex | n/r (Zed: 16 KiB terminal cap) | n/r | OpenCodex Remote Workspace results 256 KiB |

##### 3.6 Unattended and scheduled runs

| Project | Mechanism | Approval behaviour | Missed or interrupted run |
|---|---|---|---|
| Codex | `codex exec` (same engine); SQLite queue; no scheduler in the engine | Approvals rejected as errors (`never`) | n/a |
| OpenCode | `schedule` table, loop in every server process | `run` rejects asks, or `--auto` answers once | Skip missed "every" slots, at most once per due row |
| OpenClaw | Cron in the Gateway; heartbeat job every 30 min | Headless runs deny; cron standing grants for an exact job | Restore finished; retry interrupted agent turn only if delivery had not started; no attempt counter |
| Hermes Agent | Cron JSON store, 60 s ticker, per-fire journal | `cron_mode: deny` by default; toolset denylist | Advance before dispatch (at-most-once); unknown outcome pauses the job; unattended `unknown` settles interrupted |
| Oh My Pi | None (print and JSON modes) | `yolo` default; fails closed only if a prompt would be needed | n/a |
| Pi | None | No permission system | n/a |
| DeepSeek harness | Host-resident schedule service; headless profile | `ask` with no answerer fails closed | Latest-only catch-up; at-least-once in the flush-to-receipt window (inferred) |
| goose | In-process cron, `schedule.json`, `--enable-scheduler` | Forced `Auto` mode | Not caught up; can overlap itself |
| Cline | Cron runner, hub | Default `yolo`, auto-approve | n/r |
| OpenHands | Separate Automation Server | n/r | n/r |
| Warp | Autonomous runs | "Would need approval" becomes `Denylisted` for every denial | n/r |

### Approvals and permission rules

From [round2/synth/02-approvals-permission-rules.md](round2/synth/02-approvals-permission-rules.md).

The cells condense the accounts in section 2. The permalinks and trust labels
are in section 2.

##### 3.1 Default mode and the rule model

| Project | Default mode (real) | Rule model and precedence | Can repository files loosen the user's settings? |
|---|---|---|---|
| Codex | **CLI/TUI:** reviewer `user`. **`exec`:** forces `Never`, so approvals are rejected. **Untrusted projects:** the internal `UnlessTrusted` mode. | **Grammar:** Starlark `prefix_rule` over the argv prefix. **Decision:** the maximum of allow < prompt < forbidden across all layers, plus managed requirements. **Load failure:** one broken file drops every user and project rule. | Project rules load as a layer, but they can only add to the maximum. Repository skills load without a trust check. |
| OpenCode | **Default:** allow everything (shell, edit, web, MCP, external directories); only reads of `.env` ask. **Headless `run`:** rejects asks. **`--auto`:** answers `once`. | **Grammar:** a flat `{action, resource, effect}` list; last match wins; no match asks. **Order:** configured deny is checked first. **Later layers win:** config layers, per-agent rules and HTTP session rules. **Admin policy:** a separate last pass. | Yes. Project config is a later layer. Project plugins load with no trust prompt. |
| OpenClaw | **Host exec:** `security=full`, `ask=off`. **Sandbox:** off. **Sandbox `deny` default:** does not block sandboxed exec. | **Axes:** three independent ones (host, security, ask). **Approvals file:** only tightens. **Session modes:** intersect to the stricter. **Exception:** admin `full` skips the floors. | Not shown in the files read. |
| Hermes Agent | **Default:** `smart`; the code falls back to `manual`. **Deny:** cron, `-q`, API and webhooks. **Auto-approve silently:** `-z` and a bare library context. | **Order:** hardline floor > `approvals.deny` globs > YOLO/off > `command_allowlist` > unattended modes > grants > guardian > human. **Config:** one `config.yaml` per profile. | No project layer described. Grants are written into the agent-reachable `config.yaml`. |
| Oh My Pi | **Default:** `yolo`; an invalid value also falls back to `yolo`. **ACP:** forces `always-ask` for bash unless auto-approve was chosen explicitly. | **Deny:** tool deny > user deny, then the mode ladder. **Restricted folders:** always prompt. | Yes. The project layer can set `yolo` and loosen everything except `bash.patterns`. |
| Pi | **Default:** no approval system; every call runs. | **Rules:** none. **Extensions:** `tool_call` handlers run in load order; the first block wins. | The trust gate only decides which project files load, and a repository cannot relax `defaultProjectTrust`. |
| DeepSeek harness | **Default:** `workspace-write` plus ask; no ordinary call prompts. **sdk-minimal:** `danger-full-access`. | **Rules:** none. **Mode:** approved-for-call > last `sandbox/mode` event > deployment default. | Not described. |
| goose | **Default:** `Auto` on every surface. **Scheduled jobs:** forced to `Auto`. | **Levels:** per tool name in the user-global `permission.yaml`: never_allow > always_allow > ask_before. **Inspectors:** can only tighten. | No project layer. Project plugins auto-enable, and the agent can rewrite its own policy files. |
| Cline | **CLI, Desktop and cron:** approve everything. **VS Code:** prompts only for shell commands. **ACP:** prompts. | **Policy:** `{enabled, autoApprove}` per tool name, plus `"*"`. **Missing policy:** allow. **Enterprise config:** fails open. | Workspace hooks, plugins and agents load untrusted. |
| OpenHands (client) | **Default:** `NeverConfirm`, unless the stored settings enable confirmation. **With confirmation and the `llm` analyzer:** `ConfirmRisky{HIGH, confirm_unknown}`. | **Policy:** one of three kinds, chosen by the browser per conversation. **Rules:** none. | Repository `hooks.json` is attached with no trust prompt. |
| Zed | **Global:** `default: confirm`. **Terminal:** OS-sandboxed by default for local projects. **External ACP agents:** use their own modes. | **Per-tool regex lists:** hardcoded `rm` deny > invalid regex = deny > substitution deny > deny > confirm > allow (every sub-command must match) > tool default > global default. | Not in the files read. Edits to `.zed/` always prompt. |
| Warp | **GUI:** commands `AlwaysAsk`. **TUI:** commands `AgentDecides`. **Headless:** "would ask" becomes `Denylisted`. | **Lists:** anchored regex allow and deny lists. **Precedence:** the denylist beats everything; the organisation list merges first. **Enforcement:** client-side only. | Not described. A self-declared `--sandboxed` argument weakens the policy. |
| OpenCodex | **Tool approvals:** none. **Remote Workspace:** `approvalPolicy: "never"`, sessions `accessMode: read-only`. | **Capabilities:** a set the executor declares, narrowed per session. | Not applicable. |

##### 3.2 Shell parsing, binding and "always"

| Project | Shell parsing before matching | What an approval is bound to | "Always" scope |
|---|---|---|---|
| Codex | **Splitting:** a tree-sitter-bash node and operator allow-list for `sh -c`. **Fallback:** anything else is one opaque argv. **Wrappers:** not unwrapped. | **Session key:** environment id, executable, canonical argv, cwd, tty, sandbox and extra permissions. **Patches:** per path. **Never cached:** MCP and network. | **Writes:** an `allow` prefix rule to `default.rules`. **Lasts:** permanently. **Side effect:** also skips the sandbox. |
| OpenCode | **Parser:** a hand-written POSIX and PowerShell scanner. **Splits:** chains, and scans `$(...)`. **Refuses:** eval forms and unparseable input. **Ignores:** redirects, declarations and wrappers. | The request id only. | **Stored:** in SQLite, permanently, project-wide, for all agents. **Pattern:** often `*` or a first-word prefix (`sudo *`). |
| OpenClaw | **Parser:** tree-sitter-bash WASM, limited to 128 KiB and 500 ms. **Unanalyzable:** substitutions, redirects, heredocs, control flow. **Parse failure:** prompts. | argv, cwd (realpath and inode), agentId, sessionKey, a hash of the environment overrides, the real-path executable (SHA-256 if mutable), nodeId and the requester device. | **Persisted entry:** the exact executable path plus a SHA-256 of cwd and argv[1..]. **Cron:** job-bound grants. |
| Hermes Agent | **Matching:** regex over several string variants. **Deny:** per-segment executable projection that sees through `sudo`, `env` and `sh -c`. **Fails closed:** only on size limits. | A pattern class (a rule description such as "recursive delete"). | **Writes:** the class to `command_allowlist`, for every session, permanently. **"Session":** in memory only. |
| Oh My Pi | **Patterns:** anchored `*` globs. **Allow:** the whole command, with no shell control characters. **Deny or prompt:** per segment. **Wrappers:** not unwrapped. | **Session answer:** `subject\0tier`; for bash the subject is the whole tool. **Cached grant:** `tier\0path`. | **Only in** `always-ask-cached` mode. **Stored:** SQLite, no expiry. **Never offered:** for critical calls. |
| Pi | None in core. | Not applicable. | Trust only: per folder, in `trust.json`. |
| DeepSeek harness | None. The string runs as `bash -c`. | One call id. Neither the request nor the audit event carries the arguments. | None. Only `allowed-once`. |
| goose | None. Regexes and models read the raw `command` string. | A once flag on the persisted tool-call id, without the arguments. It expires with the extension lease. | **Key:** the bare tool name. **Stored:** user-global `permission.yaml`, no expiry. |
| Cline | **Approvals:** none. **Plan-mode blacklist:** splits on separators; fails open on parse errors. | One `toolCallId`. One approval covers a whole `run_commands` batch. | None. Only live category toggles. |
| OpenHands (client) | None in this repository. | **None.** `{accept}` answers whatever is pending. | None. |
| Zed | **Parser:** a brush-parser POSIX AST for every shell, PowerShell included. **Redirects:** treated as pseudo-commands. **Wrappers:** not unwrapped. **Parse failure:** disables allow. | The tool plus a prefix regex of the command word and subcommand. | **Written to:** user `settings.json`, for all threads and projects, permanently. |
| Warp | **Decomposes:** `$( )`, `( )` and chains. **Strips:** a leading `KEY=VAL`. **Not handled:** wrapper unwrapping, `$VAR`. | **Per call.** The model's `is_risky` and `is_read_only` can pre-approve. | **Run-to-completion:** for the conversation. **Lasting rules:** the profile lists only. |
| OpenCodex | None. `exec` takes an argv only, inside bubblewrap. | Each RPC is bound to the handshake session id, root id and capability set. | A capability set for one session. |

##### 3.3 Model approvers, plan mode and sub-agents

| Project | Model approver and its failure mode | Model-supplied flags | Plan or read-only mode | Sub-agent inheritance |
|---|---|---|---|---|
| Codex | **Guardian:** a read-only sandboxed thread, 90 s default, risk thresholds. **Errors:** deny. **Timeout or over budget:** back to the human. **Breaker:** per turn. | `sandbox_permissions` and a justification on escalation. | **Prompt only.** The `explorer` role file is empty. | **Roles:** can only narrow; the parent's policy is re-applied last. **Depth:** 1. **Exception:** the memory agent runs with approvals off. |
| OpenCode | None found. | No such flags were found. | **Code-enforced:** edit tools only. **Prompt only:** shell, sub-agents, Code Mode. | **Rules:** a creation-time copy of the parent's session rules plus the child agent's own (often allow-all). **Depth:** 1. |
| OpenClaw | **`auto` reviewer:** temperature 0, 30 s, 16k characters, strict JSON. **Any failure:** ask. **Injection regex match:** deny. **On a node:** ask becomes deny. | None in the files read. | **No plan mode.** The `read-only` session mode is enforced in code. | **Inherits:** the parent's tool allow and deny envelopes. **Exec mode:** not in the files read. |
| Hermes Agent | **Auxiliary model:** sees flagged commands only, answers one word, temperature 0, 30 s. **Any failure:** to the human. **DENY:** overridable once. | None described. | Prompt only. | **Tools:** the parent's minus five. **Dangerous prompts:** auto-deny unless `subagent_auto_approve`. |
| Oh My Pi | **None in the approval path.** The TTSR judge only warns. | None described. | **Code-enforced:** write and edit. **Prompt only:** bash and eval. | **Forced `yolo`,** but the user's deny and prompt rules are kept. **Prompts:** fail closed. **Depth:** 2. |
| Pi | None. | None. | Example extension only. | Example only: a fresh process with the parent's environment. |
| DeepSeek harness | **Experimental Auto review:** the same model, never sees tool results. **Failure:** deny. **Timeout:** none. **Removing the plugin:** widens to full access. | `sandbox_permissions` plus a justification. `approvedBuilds` on `plugin_manager`. | Prompt only. | **In-process:** approval pinned to `never`. **External children:** a fixed, parent-independent permission. |
| goose | **SmartApprove:** fails closed. **Adversary reviewer:** fails open. **Injection classifier:** fails open, never sees tool output. | MCP `readOnlyHint` auto-allows. | Chat mode is enforced at execution, but the tools are still advertised. | **Mode:** inherited from the parent. **Approvals:** forwarded to the parent's user. **Depth:** 1. |
| Cline | None [absence confirmed]. | `requires_approval` removed. | **Code-enforced in part:** edit tools are removed. **Blacklist:** can be bypassed. **CLI:** the model can switch to act itself. | **Policies and approver:** none, so everything runs. **Nesting:** unbounded. |
| OpenHands (client) | Inline `security_risk`, probably; the rest is in the SDK. | `security_risk` on each action. | A separate planner conversation with three tools. | **Takes:** the global settings. **Limits:** no cap, no narrowing. |
| Zed | None. | A non-empty `reason` is required for sandbox escalation. | Not in the files read. | Not in the files read. |
| Warp | **None on the client.** The model's `is_risky=false` auto-runs the command. | `is_risky` and `is_read_only`. | Not in the files read. | **Native children:** inherit the profile. **Claude and Codex children:** launched with their safety flags off. |
| OpenCodex | None. | None. | `accessMode: read-only`, enforced on the executor. | Not in the files read. |

##### 3.4 Bypass paths, hooks and remote answers

| Project | Known bypass paths | Hook failure mode | Does a remote answer name the action? |
|---|---|---|---|
| Codex | `allow` rules skip the sandbox. `--ignore-rules`. A hook-trust bypass flag. The exec-server runs the caller's sandbox, or none. | **Fails open** on error, timeout and bad JSON. **Precedence:** deny wins. **Hook allow:** pre-approves the unsandboxed retry. | **Not shown.** It is a JSON-RPC response; binding to the action is [inferred]. **`HumanCommand`:** routed to opted-in connections only. |
| OpenCode | Plugin tools without `assert`. `` !`cmd` `` templates. Shell redirects. The server password is readable, which enables self-approval [inferred]. | **`permission.evaluate`:** can loosen asks. **A throw:** inferred to fail the call. | **No.** Any password or pairing-token holder answers by request id. |
| OpenClaw | Open defaults. Cron standing grants. A Gateway approval overrides the node's allow-list. | **Fails closed:** any throw in `before_tool_call`. | **Node exec:** yes (runId, node, device, binding, single use). **Chat `/approve` with an empty approver list:** anyone in the chat. |
| Hermes Agent | Container backends skip the floor and the prompts. `execute_code`. `-z`. | **`pre_tool_call` crash:** fails open. **`approve` escalation:** fails closed. | **Card:** shows the redacted command and stays deny-only until the full command is visible. **Request-id binding:** not in the files read. |
| Oh My Pi | `eval` kernels. MCP at the `write` tier. Project settings can set `yolo`. Auto-loaded project hooks and tools. | **Fails closed:** a throw, a 30 s timeout or an abort blocks. **After a rewrite:** policy is re-resolved. | Not in the files read. |
| Pi | `!` commands. Install scripts. Hook argument mutation without re-validation. | **`tool_call` throw:** blocks the call. **Observers:** fail open. | **RPC dialog:** answered by dialog id, showing the extension's message text (clone check, section 1). |
| DeepSeek harness | MCP. In-process tools. `approvedBuilds`. External children. | **A throwing listener:** fails the call closed. **The hook bridges:** not read. | **No.** The answer is `{clientId, eventId, outcome}`. |
| goose | Code Mode `tools.call` skips approval, inspectors and hooks. ACP-hosted agents. `readOnlyHint`. Writes to the policy files. | **Default:** fails open. **PreToolUse `on_failure: block`:** fails closed, timeouts included. | **Call id:** persisted first; conflicting answers rejected. **Arguments:** not bound. |
| Cline | Sub-agents. Hub client tools. MCP. Untrusted workspace hooks. Origin-only hub sessions. | **Fails open** on a crash, a timeout or bad output. A cancel aborts the whole run. | **Opaque `approvalId` only.** Any attached client may answer. |
| OpenHands (client) | Server-level bash. The session key is in the agent's environment. Client tools. `tool_module_qualnames`. | **Load failure or 5 s timeout:** all hooks are dropped. | **No.** The body is `{accept, reason?}`. |
| Zed | External ACP agents ignore `tool_permissions`. ACP `terminal/create` is unsandboxed. Allow-all skips the substitution check. | Not in the files read. | **ACP:** the agent's options are passed through. **Binding:** not shown. |
| Warp | Run-to-completion (default on). `--sandboxed`. CLI children with their safety flags off. | No hooks. | **ACP:** answered by policy, which prefers `AllowAlways`. |
| OpenCodex | An unauthenticated loopback listener. Executor-declared capabilities. | No hooks. | No remote answering. |

### Sandboxing and where commands run

From [round2/synth/03-sandbox-containers-execution.md](round2/synth/03-sandbox-containers-execution.md).

##### 3.1 Mechanism per OS and behaviour when unavailable

| Project | Linux | macOS | Windows | When unavailable | Enforcement level surfaced? |
|---|---|---|---|---|---|
| Codex | bwrap + seccomp (two stages); legacy Landlock refused for restricted policies | Seatbelt (profile not read) | restricted token or MXC, only if enabled; otherwise the rule fallback prompts | Linux: the helper panics, nothing runs. Exec-server: refusal string. WSL1: refused | not found in the input |
| OpenCode | none | none | none | n/a | n/a |
| OpenClaw | Docker or Podman container (opt-in) | same | same | `sandbox: required`: throws. Otherwise host exec | not found |
| Hermes Agent | none locally; Docker, SSH, Modal or Daytona backends | same | same | n/a | n/a |
| Oh My Pi | none | none | none | n/a | n/a |
| Pi | none (Gondolin VM as an example extension) | none | none | n/a | n/a |
| DeepSeek harness | bwrap, then Landlock, probed in order | Seatbelt `(allow default)` | restricted token plus ACL SIDs | `SANDBOX_UNAVAILABLE`, nothing runs | computed (`full`/`partial`), stored on the result, shown to nobody |
| goose | none (Seatbelt removed) | none | Job Object (for killing only) | n/a | n/a |
| Cline | none | none | none | n/a | n/a |
| OpenHands | per-conversation Docker, upstream and not read | same | same | a failed probe falls back to a host workspace | not visible |
| Zed | bwrap (setuid refused) | Seatbelt `(deny default)` | bwrap inside WSL | Linux and WSL: a prompt (retry, run unsandboxed, deny). macOS: fails closed | the model is told when it ran unsandboxed |
| Warp | none (`sbx` Docker behind a dogfood flag) | none | none | n/a | `--sandboxed` is self-declared |
| OpenCodex | bwrap on the executor | native helper, probe fails | native helper, probe fails | `exec` not offered; file tools only | the capability list (`workspace.exec` present or not) |

##### 3.2 What the sandbox confines

| Project / mechanism | File reads | File writes | TCP/UDP | Unix sockets | Processes and signals |
|---|---|---|---|---|---|
| Codex bwrap+seccomp, network off | restricted-read policies start from `--tmpfs /`; deny paths masked; deny globs snapshotted at launch | writable roots only; `.git`, `.agents`, `.codex`, `.aws` read-only | netns; seccomp denies `connect`, `bind`, `sendto` and non-`AF_UNIX` `socket` | `connect` denied, so pathname stream sockets are blocked; `sendmsg` to a pathname datagram socket not denied (review); daemon directory masked | PID, user and IPC namespaces; `ptrace` and `process_vm_*` denied; `io_uring` denied |
| Codex, network on | as above | as above | open (unless proxy mode) | reachable except masked directories | `AF_VSOCK` and `io_uring` denied |
| DeepSeek bwrap | not confined | roots, plus a private `/tmp` | not confined | not confined (inferred) | `--unshare-pid` |
| DeepSeek Landlock | not confined | roots, plus the shared `/tmp` | not confined | not confined | none |
| DeepSeek Seatbelt | not confined | roots | not confined | not confined | none |
| Zed bwrap | `--ro-bind / /` (all readable) | exact writable binds; protected paths re-bound read-only (missing paths skipped) | netns when blocked or proxied; HTTP(S) proxy for host lists | not stated in the input for Linux | user, pid, ipc, uts and cgroup namespaces |
| Zed Seatbelt | all reads allowed | writable subpaths; protected denies last | none, the proxy port, or all | none allowed | Mach-service allow-list |
| OpenCodex bwrap | `/usr`, `/bin`, `/lib*`, a short `/etc` list, toolchain roots | one `/workspace` bind | `--unshare-net` always | host sockets not mounted (inferred) | pid, ipc and uts namespaces; no user namespace, no `cap-drop`, no seccomp |
| OpenClaw container | container filesystem plus validated binds | `--read-only` root, sized tmpfs, binds | `--network none` by default | Docker socket binds rejected | `--cap-drop ALL`, `no-new-privileges`, `--pids-limit 1024` |
| Hermes Docker | container filesystem plus persistent home and workspace binds | the same binds (as root with `DAC_OVERRIDE`) | on by default | not addressed | `cap-drop ALL` plus 3 to 5 added back; `no-new-privileges`; pids 2048 if cgroups allow |
| rizzy PR 4 Landlock (our code) | `SYSTEM_READ` list plus node.toml `read`; `/proc` readable | session folder, per-session `TMPDIR`, node.toml `write` | network off: TCP connect and bind denied (ABI 4); UDP and DNS open | pathname sockets open below ABI 9; abstract sockets scoped (ABI 6, best-effort) | signals scoped (ABI 6, best-effort); `no_new_privs` required |

##### 3.3 Container invocations: fixed flags and who controls them

| Flag or property | OpenClaw | Hermes Docker | goose `--container` | Warp `sbx` | OpenHands | rizzy design §4.6 |
|---|---|---|---|---|---|---|
| Who builds the invocation | Gateway, from `openclaw.json` | Hermes, from config | the user (an existing container) | the client, with fixed arguments | upstream SDK (not read) | the node, from `node.toml` |
| Image | config; default `openclaw-sandbox:trixie-slim` | config | the user's | Docker Sandboxes | `OH_CONVERSATION_IMAGE` env | allow-list pinned by digest |
| Pull | `--pull=never` | not stated | n/a | not stated | not visible | `--pull=never` |
| User | image default (inferred) | root, unless `--user` | the user's | not stated | not visible | `--userns=keep-id --user <uid>:<gid>` |
| Capabilities | `--cap-drop ALL` (default) | `ALL` dropped, then `DAC_OVERRIDE`, `CHOWN`, `FOWNER` (+`SETUID`, `SETGID`) | n/a | not stated | not visible | `--cap-drop=all` |
| `no-new-privileges` | always | yes, unless `docker_snap_compat` | n/a | not stated | not visible | yes |
| Root filesystem | `--read-only` (default) | writable | n/a | not stated | not visible | `--read-only` |
| tmpfs | `/tmp` 1 GB, `/var/tmp` and `/run` 256 MB | `/tmp` 512 MB, `/var/tmp` 256 MB, `/run` 64 MB | n/a | not stated | not visible | `/tmp` 1 GB, `/run` (no size) |
| Network | `none` (default); host and `container:*` rejected | on (default) | n/a | not stated | not visible; the launcher binds `0.0.0.0` so containers reach the host | pasta or none; host only with `allow_host_network` |
| Limits | `--pids-limit 1024`, `ulimit core=0`; memory and CPU optional | pids 2048, 1 CPU, 5120 MB if cgroups allow | n/a | not stated | the client sends unbounded memory and CPU | memory, swap, CPUs, pids from `node.toml` |
| Mounts | validated binds (blocked list, roots, realpath through the ancestor) | persistent home and workspace; read-only skills and creds; cwd optional | n/a | an empty host directory plus a read-only init directory | `/workspace` | one environment volume, `relabel=private` |
| Environment | `--env-file`, config-declared only | name-only `-e KEY` (values not in argv) | `-e K=V` on argv (leaks into `ps`) | not stated | every saved secret plus the session key | `--env` allow-listed names |
| Pass-through args | three `dangerously*` booleans | `docker_extra_args` appended last, override anything | n/a | none | n/a | none (by design) |
| Recreate on flag change | `createArgsEpoch` label | labels (task, profile, egress, env fingerprints) | n/a | n/a | n/a | not specified |
| Checks skipped inside | approval (sandboxed exec runs without one by default) | hardline floor, sudo guard, dangerous patterns | n/a | n/a | hooks skipped for container workspaces | none planned |

##### 3.4 Remote executors: what the executing side decides

| Project | Executor | Transport and authentication | Executor-side policy | Refusals |
|---|---|---|---|---|
| Codex | `codex exec-server` | Noise IK through a relay, registry-pinned key; direct `--listen` with optional auth | none: the caller's sandbox intent is materialised; no intent means no sandbox | "sandbox intent cannot be enforced on this executor" |
| DeepSeek harness | SSH helper | OpenSSH with fixed flags; TLS-PSK streams | runs its own platform sandbox chain for `confine` | `SANDBOX_UNAVAILABLE`; reported enforcement is self-reported |
| Pi (pi-env) | Rust daemon over SSH stdio | the SSH login only | none: any path, any command | unknown ops (`EINVAL`) only |
| OpenCodex | Remote Workspace executor | WSS bearer plus signed ECDH, AES-GCM | path, identity and hard-link checks; `exec` only with an OS sandbox; roots chosen on the executor | many (see section 2) |
| OpenClaw | paired node | Gateway WebSocket | own `security`/`ask`/allow-list; plan binding; policy-snapshot check | `SYSTEM_RUN_DENIED`; but a Gateway approval overrides the node allow-list |
| Hermes Agent | SSH, Modal, Daytona | `accept-new` SSH; vendor SDKs | none (guards skipped for cloud backends) | none |
| Cline | remote Cline hub over SSH | strict host keys, tunnel, token | a full remote agent with its own settings | its own |
| OpenCode | remote OpenCode server over SSH; empty driver seam | SSH tunnel plus service password | a full remote server | its own |
| OpenHands | DigitalOcean microVM agent-server | bearer port-forward tunnel | upstream, not read | not visible |
| goose, Oh My Pi, Zed, Warp | none read (OMP SSH is file I/O only; Warp's worker is not in the repository) | n/a | n/a | n/a |

##### 3.5 How child processes are killed, and what survives

| Project | Kill mechanism | What survives |
|---|---|---|
| Codex | `--die-with-parent`, `--new-session`, PID namespace; the inner stage forwards signals; LRU of 64 processes terminates pruned ones | not traced |
| OpenClaw / Hermes containers | `--init`; container lifetime (`sleep infinity`); Daytona cancel stops the sandbox | Hermes reuses containers across processes by label |
| Hermes local | `start_new_session` plus an owner-death supervisor | not stated |
| Oh My Pi | SIGTERM to the group and descendants, grace, re-walk, SIGKILL (unchecked) | a double-fork that changes session (inferred) |
| Pi CLI | SIGKILL to the process group; SIGSTOP first on macOS; tracked detached children killed on SIGHUP/SIGTERM | everything, if Pi itself is SIGKILLed |
| Pi pi-env | the daemon kills every process group it started on stdin EOF or 30 s of silence | n/a |
| DeepSeek SSH | the helper lease expires and remote managed processes are cleaned up | n/a |
| goose | `kill(-pgid, SIGKILL)`; Windows Job Object created suspended; `Drop` kills | `setsid` children; background work after a normal exit; everything if goose crashes (no PDEATHSIG for the shell) |
| Cline | SIGKILL to the detached group; `taskkill /T /F` | `setsid` grandchildren; human-detached commands; hook timeouts kill the direct child only |
| OpenCodex | bwrap `--die-with-parent --new-session`; kill on output overflow, timeout or abort | not stated |
| Zed | bwrap `--die-with-parent`; ACP children killed on drop | not stated |
| OpenCode | PTY daemon keeps terminals across server restarts | terminals, by design |

### Credentials, secrets and authentication

From [round2/synth/04-credentials-secrets-auth.md](round2/synth/04-credentials-secrets-auth.md).

##### 3.1 Keys, environment, redaction

| Project | Keys at rest | Keys reachable from the shell | Child environment (shell / MCP) | Redaction |
|---|---|---|---|---|
| Codex | MCP OAuth in keyring, else a user-readable file | Yes: `OPENAI_API_KEY` inherited by default; with the broker, a dummy | Inherit all; `*KEY*` excludes off by default; fixed always-strip list for launch secrets; Git vars scrubbed for plugin Git | Four regexes, not on tool output |
| OpenCode | SQLite JSON, 0600 | Yes: env, and the DB file; server password in env outside stdio mode | Whole `process.env`; workspace drivers start empty (none ship) | None |
| OpenClaw | Auth profiles in SQLite; config/`.env` readable | Host exec: yes for provider keys in env; sandbox: no; GitHub token on workers: yes | Inherit minus ~110 injection keys and registered credential names; sandbox: declared only; SSH: `*_API_KEY` etc. blocked | Worker credentials and GitHub tokens registered for log redaction |
| Hermes Agent | `auth.json` pools; per-profile homes | Terminal: unlisted secrets yes; managed keys no; Docker with iron-proxy: tokens only | Terminal blocklist; `execute_code` and MCP allow-list | On by default; pattern plus env-dump pass; sentinel on file reads |
| Oh My Pi | `agent.db` SQLite, 0600, unencrypted | Yes: env and `agent.db` | Whole env minus project `.env` values and Git location vars | Opt-in obfuscation to the provider, reversed before tools run |
| Pi | `auth.json` 0600 on creation | Yes: env and `auth.json`; `--api-key` in argv | Whole env; MCP inherits | None in core |
| DeepSeek harness | `.credentials.yaml` 0600, refuses loose mode; cwd `.env` accepted | Env scrubbed by name; store file readable by confined bash | Name-pattern scrub (`KEY|PASSWORD|SECRET|TOKEN`, `DSH_*`), MCP same | Resolved MCP credential values only |
| goose | Env first, keyring blob, silent plaintext fallback | Yes: env-supplied keys | Whole env; extension `envs` filtered by 31-key disallow list | None found |
| Cline | `providers.json`/`secrets.json` 0600 | Yes: env and file | Whole env for shell, hooks, plugins, MCP | Telemetry and marketplace output only |
| OpenHands (client) | Key files 0600; Docker: key beside ciphertext | The master session key is put in the agent's env on purpose | Server side, not in repo | Server side, not in repo |
| Zed | Not studied | Not studied | Inherited (terminal and ACP agents) | Not studied |
| Warp | OS keychain, then sent to Warp per request | Not shown (client) | MCP inherits whole env | Client regex, off by default |
| OpenCodex | Plaintext `config.json` unless keychain; OAuth plaintext 0600 | Gateway: n/a (no agent shell); Remote Link child: link key only | Remote Workspace `exec`: bwrap `--clearenv` plus `HOME`, `PATH`, `LANG`, `LC_ALL` | Logs refuse any configured credential value; fail closed |

##### 3.2 Brokers, proxies, workers, servers, trust

| Project | Broker or proxy, and coverage | Worker credentials | Server authentication model | Trust gate |
|---|---|---|---|---|
| Codex | Network-proxy credential broker (off by default, MITM), dummy env values, host-bound; netns bridge makes it the only path on Linux | Exec-server: registry auth via stdin, Noise IK with pinned key; caller's sandbox applies | WS: capability token or HS256 JWT at upgrade, `Origin` 403; loopback unauthenticated; non-loopback refused for app-server only; Unix socket by file mode | Project trust for config, hooks, exec policy; hook hash in user layer; skills ungated |
| OpenCode | None | None (SSH tunnel to a per-machine server) | One password (Basic, cookie or `?auth_token=`), `===` compare; pairing token HMAC of the password | None |
| OpenClaw | Secret egress proxy with sentinels and allowed hosts (cooperative); inference brokered by model ref | 32 bytes, domain-separated hash bound to the turn claim, 10 min TTL, per-frame revalidation, owner-epoch fencing | Challenge nonce, Ed25519 device proof over token and scopes; startup token; device tokens plaintext | Not read this round |
| Hermes Agent | iron-proxy token swap with `require: true`; Docker only; own model calls bypass | Relay HMAC token, 300 s | Same-user control socket, single-use 30 s tickets in a subprotocol; non-loopback dashboard needs auth; API server one key | Regex scanner at install; context files blocked on hits |
| Oh My Pi | Auth broker hands out keys; auth gateway proxies with digest-stored scoped clients (no expiry); wildcard CORS | None | Static bearers, `Set.has`; `--no-auth` | Hard-coded open |
| Pi | None (documented third-party only) | pi-env: none beyond SSH | stdio only; `apps/p2` one-use token, Host and Fetch-Metadata fence | Per-directory boolean, no content binding, context files ungated |
| DeepSeek harness | None | None | Launch token in URL, then 30-day HMAC cookie; Host, `Sec-Fetch-Site`, Origin fence; wildcard bind refused | `approvedBuilds` set by the model |
| goose | None | Roaming: key in trust book, re-read per connection, revocation closes live connections | Shared secret, header or `?token=`, constant time; `--dangerously-unauthenticated` | Project plugins auto-enabled |
| Cline | None | Provider key sent per session to a remote hub | Per-start token **or** loopback `Origin`; approvals answerable by anyone | None (home-only lane excepted) |
| OpenHands (client) | MARS bridge keeps a cloud token server-side | n/a in repo | One shared key, injected into HTML on loopback, kept in `localStorage`; first-frame WS auth | Hooks ungated; plugin checkbox |
| Zed | Egress allow-list proxy, no credential substitution | n/a | Not studied | ACP registry checksum optional |
| Warp | Model calls through Warp's server with the user's key | In uncloned repo | Server side | Project MCP never auto-starts |
| OpenCodex | Full model-call gateway with admission, budgets, scopes; Remote Link relay strips caller credentials | Pair code 60 bits, hash stored, deleted before validation; bearer hashed, no expiry | None on loopback; mandatory key off loopback; admin token distinct from data keys | n/a |

### Links, workers and client protocols

From [round2/synth/05-links-workers-protocols.md](round2/synth/05-links-workers-protocols.md).

##### 3.1 Topology, identity and refusal

"Loop" is the agent loop (model calls and tool-call decisions); "tools" is where tool calls
execute. "Refuses" is what the executing side can refuse on its own authority, whatever the
controlling side sends.

| System | Loop runs | Tools run | Who dials | Transport | Pairing and identity | Executing side refuses |
|---|---|---|---|---|---|---|
| Codex app-server | app-server | app-server, or an exec-server | client to server | stdio JSONL, `unix://`, `ws://` JSON-RPC | capability token or HS256 JWT at upgrade; none on loopback | n/a (single process) |
| Codex exec-server | harness | executor | executor to relay (`--remote`); or harness to `--listen` / `ssh ... --listen stdio` | relay WebSocket + Noise IK, 128 streams | executor key pinned from a registry; registry authorises harness key | nothing: the sandbox comes from the request; none means unsandboxed |
| Codex remote control | local app-server | local | app-server to vendor relay | WebSocket v3, segmented envelopes | enrolment token with `expires_at`; relay host allow-listed | n/a |
| OpenCode service | server | server (or SDK `WorkspaceDriver`) | client to server | HTTP + SSE | one shared password; pairing code to password-derived HMAC token | driver only executes; decision stays on server |
| OpenCode SSH (desktop) | remote server | remote server | desktop over SSH, then `ssh -L` | HTTP through tunnel | password read from remote `service.json` over SSH | n/a |
| OpenClaw cloud worker | worker (inference brokered by Gateway) | worker | worker to Gateway WS; Gateway also SSHes in | WebSocket JSON frames, closed method set | setup code binds box to environment; per-dispatch credential | worker checks the hello matches its request; model choice approved by Gateway |
| OpenClaw node | Gateway | node (`system.run` and others) | node to Gateway WS | WebSocket, protocol 4 | Ed25519 device proof over nonce + token; operator approval or SSH auto-approval | `system.run`: own security/ask, plan binding, PATH, policy drift; but a forwarded approval satisfies its allow-list |
| Hermes viewers (CLI, TUI, ACP) | gateway | gateway | viewer to gateway | Unix socket for a ticket, then WebSocket | peer uid; 30 s single-use ticket in a subprotocol | ACP viewer never runs tools |
| Hermes relay | gateway | gateway | gateway to relay | WSS, NDJSON | HMAC upgrade token (300 s), rotating secrets | n/a |
| Hermes SSH / Modal / Daytona | host | remote | host over SSH or SDK | `ssh bash -c` / SDK exec | `accept-new` (TOFU) | nothing |
| Oh My Pi RPC / ACP | embedded process | same | parent spawns child | NDJSON / ACP over stdio | pipe ownership | n/a |
| Oh My Pi collab | host | host | host and guests to relay | WSS, AES-256-GCM sealed frames | room key and write token in link fragment | guest prompts run under host's mode; read-only peers refused |
| Pi pi-env | controller | daemon | controller over SSH | length-prefixed binary frames on stdio | SSH strict host keys, private known-hosts, `scanHostKey` | nothing (no root, no allow-list) |
| DeepSeek SSH | local | remote helper | local over SSH | framed JSON RPC (protocol 3); TLS-PSK per stream | SSH config; self-reported helper hash | helper runs its own sandbox chain; reported enforcement trusted |
| DeepSeek web | host process | host process | browser to host | HTTP POST + one WebSocket mux | launch token to signed cookie | n/a |
| goose serve | server | server | client to server | HTTP / WebSocket ACP | one shared secret (header or `?token=`) | n/a |
| goose roaming | host | host | peer to host (iroh QUIC) | QUIC streams carrying ACP | key in a trust book; fingerprint compared out of band | host forces cwd on new/load/fork (bypassable by update) |
| Cline hub | hub | hub, or a client's contributed executors | client to hub | one WebSocket, JSON frames | per-start token, or loopback `Origin` | n/a; client sets tool policies |
| Cline SSH remote | remote hub | remote hub | desktop over SSH, then `ssh -L` | hub WebSocket through tunnel | token printed on remote stdout | n/a |
| OpenHands Canvas to Agent Server | Agent Server (not in repo) | Agent Server, or browser for client tools | browser to server | WebSocket envelope + REST | one shared key, first message | not in repo |
| Zed (ACP client) | external agent | agent, or Zed fs/terminal | Zed spawns agent | ACP stdio | none | fs limited to worktrees; terminal unconfined |
| Warp client | Warp server (not in repo) | client | client to Warp server | HTTPS POST + SSE protobuf | bearer | client policy gate |
| OpenCodex Remote Workspace | hub | executor | executor to hub | WSS + per-session signed ECDH, AES-GCM | single-use code to bearer (hash stored); pinned hub key | paths, symlinks, hard links, root identity, CAS writes, `exec` without sandbox; intersects capabilities |
| OpenCodex Remote Link | child's Codex | child | child relay to Home through SSH tunnel | HTTP | link token; mutual HMAC challenge | strips caller credentials |

##### 3.2 Fencing, credentials, resume, frames, listener auth

| System | Fencing of stale workers | Credential per dispatch | Resume mechanism | Durable vs transient frames | Listener authentication |
|---|---|---|---|---|---|
| Codex app-server | file lock per thread (one host) | none | `thread/resume` by id/path/history; rejoin running thread | none distinguished on the wire | non-loopback refused without auth; loopback none; Origin 403; no post-upgrade expiry |
| Codex exec-server | one attachment per session id, 80 s retention; no epoch | rotating executor credentials over stdin (registry) | `resumeSessionId`; `process/read` cursor; relay `Resume{next_seq}` | relay `Data{seq}` with ack bits | `--listen` auth optional, non-loopback allowed |
| Codex remote control | server side, not in repo | enrolment token | `x-codex-subscribe-cursor` | envelope `seq_id` | outbound only |
| OpenCode | none; restart sweep assumes owner dead | none | clients refetch REST; log API (`after`, watermark, `log.synced`) exists but `persist` off | durable event table opt-in; SSE volatile | password required; `===`; `?auth_token=` accepted; SDK router unauthenticated |
| OpenClaw cloud worker | global `owner_epoch` revalidated per request; writer claim CAS checked inside the write | 32 bytes, domain-tagged hash bound to turn claim, 10 min | same claim-bound credential; same call id; commit ledger `(session, epoch, seq)` caches replies | `transcript.commit` (sequenced, ledgered) vs `live-event` | opaque `invalid-handshake`, failures rate-limited |
| OpenClaw node | (pairing revision; per-run approval binding) | none | not traced | `event` frames carry optional `seq` | device proof; token default; mode `none` exists |
| Hermes gateway | `runtime_epoch` + per-session `runtime_generation`, checked at settle | none | lost turns marked `unknown`, never replayed | admissions durable before ack | control socket peer uid; non-loopback dashboard needs auth; API key required |
| Hermes relay | 4401 after handshake is terminal | HMAC token, 300 s | connector redelivers unacked `bufferId` | `inbound_ack` after durable admission | n/a (dials out) |
| Oh My Pi | OS file lock per session; sibling file on conflict | none | RPC `get_entries {since}`; collab snapshot then entries | v2 chunking; collab sealed | RPC none (pipe); broker one bearer `Set.has`; gateway digests, `--no-auth` |
| Pi pi-env | pipe lifetime; 30 s silence kills process groups | none (SSH) | none: lost requests are `lost: true`, handles die | n/a | SSH only; token is a sync marker |
| DeepSeek | `flock` with no expiry (one host); SSH helper lease 30 s | TLS-PSK key per stream | `follow` snapshot per reopen + client gap repair; no cursor | durable events vs assistant frames with `revision` | Host/Origin fence + cookie; wildcard bind refused; launch token in URL |
| goose | none | none | `session/load` replays whole conversation (or tail) | n/a | secret required unless `--dangerously-unauthenticated`; `?token=` accepted |
| goose roaming | trust book re-read per connect; 2 s revocation poll closes connections | none | as above | n/a | QUIC-TLS key identity |
| Cline hub | none (single daemon) | provider key sent per session (SSH) | `stream.subscribe {sinceSequence}` from a 7-day log | log is a replay aid; messages on disk canonical | token or loopback `Origin`; ProtoBus gRPC unauthenticated |
| OpenHands | not in repo | per-conversation `session_api_key` on Cloud | `after_seq` with gap-tolerant cursor; `sync` frame names the range | `durable` / `transient` / `delta` frames, explicitly | shared key, first message; ingress has none |
| Zed | n/a | n/a | n/a | n/a | no listener |
| Warp | server side | BYO keys sent in every request; HPKE-sealed managed secrets | inbox `since=<seq>` with cursor stored server-side | event `sequence` | client is never a listener (ACP bridge one-time token) |
| OpenCodex Remote Workspace | latest connection wins per device; bearer has no expiry | none per dispatch; device bearer | none: sessions die with the connection | counter-bound encrypted frames | pairing route unauthenticated (Origin refused, rate-limited); agent route bearer |

##### 3.3 Listeners that can run without authentication

| Project | Listener | Condition | Citation |
|---|---|---|---|
| Codex | app-server `ws://` | loopback bind, no auth configured (default) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L91-L96 |
| Codex | exec-server `--listen ws://IP:PORT` | any bind, auth optional | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server-cli/src/command.rs#L56-L160 |
| Codex | daemon control socket | always; protected by directory mode only | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L148-L246 |
| OpenCode | in-memory SDK router | built without a password | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/fetch.ts#L34-L36 |
| OpenCode | `/auth/connect/:code` | always (the code is the credential) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/middleware/authorization.ts#L87-L94 |
| OpenClaw | Gateway | auth mode `none` | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/handshake-auth-helpers.ts#L165-L187 |
| Hermes | control socket | same-uid peer only, no secret | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/control_socket.py#L411-L425 |
| Oh My Pi | auth gateway | `--no-auth` (with `Access-Control-Allow-Origin: *`) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-gateway-cli.ts#L452-L468 |
| Oh My Pi | browser relay | no token configured | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/browser/relay/server.ts#L130-L160 |
| Oh My Pi, Pi | RPC and ACP modes | always (stdio; the parent is the boundary) | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L401-L738 |
| goose | `goose serve` | `--dangerously-unauthenticated`, any bind | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L1821-L1834 |
| Cline | hub WebSocket | loopback bind plus a loopback `Origin` header | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L676-L722 |
| Cline | core ProtoBus gRPC `127.0.0.1:26040` | always | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/standalone/protobus-service.ts#L11-L36 |
| Cline | hub dashboard `127.0.0.1:8787` | loopback default, no `ROOM_SECRET` | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cline-hub/src/options.ts#L72-L84 |
| OpenHands | ingress | always (auth left to upstreams) | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/ingress.mjs#L185-L226 |
| OpenCodex | data plane | loopback bind (no token read) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/auth-cors.ts#L337-L339 |
| OpenCodex | unauthenticated loopback companion | when configured (logged warning) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index.ts#L790-L802 |
| OpenCodex | `/remote-workspace/pair` | always (the code is the credential; Origin refused, rate-limited) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index/serve-options.ts#L484-L513 |

Health and version probes are open almost everywhere (Codex `/readyz` `/healthz`, goose
`/health` `/status`, Cline `/health` `/version`, Oh My Pi broker `/v1/healthz`).

### Session storage, resume, fork and checkpoints

From [round2/synth/06-sessions-fork-checkpoints.md](round2/synth/06-sessions-fork-checkpoints.md).

##### 3.1 Storage, durability, integrity, resume, retention

| Project | Storage format | Durable by default | Integrity and locking | Resume method | Retention default |
|---|---|---|---|---|---|
| Codex | JSONL "rollout" per thread, typed items, per-file ordinal; SQLite index | Yes for normal threads; file created lazily; `--ephemeral` opts out | Advisory file lock per thread plus 30 s coordinator; revision from path, size, times, inode | Reverse replay from newest full compaction; rollback markers; rejoin a running thread | None; zstd after 7 days; `archived_sessions/` |
| OpenCode | SQLite projections are truth; optional `event` table | Projections yes; event log no in the CLI (`persist` false) | Seq under per-aggregate mutex inside the same transaction; no fencing token | Execution claim written with `Started`; boot sweep sends a "continue" message; 10-attempt cap | None for sessions; tool-output files age out |
| OpenClaw | SQLite: nodes, windows (generations), `transcript_events` (JSON or zstd) | Yes | Writer claim by CAS, checked inside fenced writes; global owner epoch for workers | Fixed "check state before repeating" prompt; pending-input table | 30 days, 5,000 entries, 10 GiB budget, `enforce` |
| Hermes Agent | SQLite `state.db`, mutable rows, soft delete | Yes | `transcript_revision` triggers; runtime epoch plus generation fence; WAL refused on risky storage | Deepest descendant with messages; restore cwd; `unknown` turns never replayed | Prune and archive off by default (`retention_days: 90` when on) |
| Oh My Pi | JSONL tree, 256-byte title slot, header, `id`/`parentId` entries | Yes | OS file lease per session; hot append vs atomic rewrite | Replay file; leaf = last entry unless `activeLeafId`; cwd fallback | Nothing deleted; `omp gc` opt-in |
| Pi | JSONL tree, header, `id`/`parentId` | Yes once a message exists (`--no-session` in memory) | No lock, no fsync; migrations rewrite in place without a temp file | Leaf = last entry; reopen by mtime, path or id | None found |
| DeepSeek harness | JSONL as checksummed zstd frames, header first, contiguous `seq` | Yes; write-behind 200 ms plus flush before model request and tool body | Frame checksums, contiguity; `flock` with no expiry; no hash chain | Lock, read all, append synthetic closers | None found |
| goose | SQLite `messages` rows, no seq | Yes | `BEGIN IMMEDIATE`, timestamp clamp; in-memory one-run guard only | `session/load` replays conversation; pending approvals re-validated | None |
| Cline | JSON files per session plus SQLite index | Yes | Whole-file `writeFileSync` (torn write possible); hub log separate | Reload messages; compaction sidecar checked by prefix hash | Hub log 7 days / 64 MiB; sessions none found |
| OpenHands | Not in this repository | Not in this repository | n/a | Client: `after_seq` cursor, highest contiguous seq | n/a |
| Zed | Thread DB not read; action log in memory only | Action log: no | n/a | n/a | n/a |
| Warp | Local SQLite table named; server holds the conversation | Not settled | n/a | Not settled | n/a |
| OpenCodex | Nothing read | n/a | n/a | n/a | n/a |

##### 3.2 Fork, branch, active-branch record, export and import

| Project | Fork representation | Cut rule | What the fork carries | Active-branch record | Export and import |
|---|---|---|---|---|---|
| Codex | Pointer: `history_base {rollout_id, end_ordinal, end_byte_offset}` over the parent's immutable file; revert = new file under the same thread id, path swapped by CAS | `lastTurnId`, `beforeTurnId` | Conversation only; `thread/revert` does not touch files | The file the SQLite row points to (inferred) | Not described |
| OpenCode | Copy of settled message rows into a new `session_v2` row; `fork_session_id`, `fork_boundary` | Newest or a given message; unfinished rows skipped | Conversation; cost zeroed; no files | None (revert deletes rows after the boundary) | Projection-based export; `--sanitize`; import refuses an existing id |
| OpenClaw | Copy of completed prefix; refused if over a token ceiling; windows with `reason` and `previous_session_id` | Completed prefix | Conversation | Window generations | Not described |
| Hermes Agent | Copy of active rows in one transaction; `parent_session_id`, `_branched_from` | A message id, extended over its tool results; parent must be idle | Conversation; approval reset to `ask`; no files | `active` flag per row | `jsonl`, `md`, `html`, `trace`; trace redacts and aborts if redaction fails |
| Oh My Pi | In-file leaf move (`/tree`); new file for `/branch` and `/fork` (`parentSession` in header) | User message for `/branch` | Conversation path; artifacts on `/fork` | Last physical entry unless `activeLeafId` | HTML export; sealed `/share` with redaction; imports Claude Code and Codex |
| Pi | In-file leaf move; new file for fork or clone (root-to-leaf path only; `parentSession` = path) | `before` a user message or `at` | One path, siblings dropped | Last entry in the file (not stored) | HTML with all branches, JSONL current branch; no redaction found |
| DeepSeek harness | Copy of exact prefix into a new log; `session/end-seed` marker; `isSeeded`, `inheritedEventCount` | Any seq; open turn closed with `forked` closers; web default is the last completed turn | Whole prefix on disk; forked workspace if the parent had one | Not applicable (linear logs) | ZIP of canonical JSONL; no redaction |
| goose | Copy of the whole conversation, then optional timestamp truncation; `"<name> (copy)"` | Timestamp; no run check | Conversation only | None | JSON, YAML, Markdown, HTML; imports Claude Code, Codex, Pi |
| Cline | Restore with messages starts a new session with fork metadata; old session kept | A user turn not folded into a compaction | Messages plus (optionally) the workspace restore | None | Imports Claude Code, Codex, OpenCode |
| OpenHands | Server-side fork from `from_event_id`; client copies metadata | Event id | No files | Not in this repository | Server trajectory download |
| Zed | Not in the input | n/a | n/a | n/a | n/a |
| Warp | Server fork of the conversation on handoff | n/a | Plus a workspace snapshot | n/a | n/a |
| pi-durable | Parent pointer over linear conversations, nothing copied; documents copied as of the fork | Entry id | Documents per fork policy | Per conversation | Not described |

##### 3.3 Checkpoints and undo

| Project | Mechanism | Trigger | What a checkpoint scans | Shell-made changes | Restore safeguards | Cost guards |
|---|---|---|---|---|---|---|
| Codex | None (removed; old records stripped on read) | n/a | n/a | n/a | n/a | n/a |
| OpenCode | Shadow Git store in the data dir; alternates to project objects; index seeded from the project index; tree id = snapshot id | Start and end of each model step | `diff-files` plus `ls-files --others` over every file (O(tree)) | Yes (tree diff) for git projects; ignored files and files outside the project no | Stage then clear or commit; busy refusal while running; only attributed paths restored; user edits to attributed files overwritten | 2 MiB untracked cap; temp index renamed; coalescing; fingerprint shortcut; `untrackedCache`, `manyFiles`, index v4; packing; no retention |
| OpenClaw | Refs `refs/openclaw/snapshots/<id>`; worker-result refs | Worktree removal; worker turns | Not described | Not described | Strict ref regex; nested repos refused; GC resolves uncertainty to keep | 7-day idle GC; owner check |
| Hermes Agent | One bare shadow repo, ref and index per project, isolated env | Before `write_file`/`patch`; before `terminal` if the command text matches a regex; once per directory per turn; off by default | Whole project tree via `git add` into the project index, with exclusions | Only when the regex matches the command; safe restore skips shell-changed files not in the ledger | Pre-rollback snapshot; safe mode with write ledger; nested repos refused; oversize files kept | 50,000-file skip; 10 MB per file; 20 snapshots; 500 MB; 7-day sweep; `/` and `$HOME` ancestors refused; containers excluded |
| Oh My Pi | None for files; `checkpoint`/`rewind` rewind the conversation | n/a | n/a | n/a | n/a | n/a |
| Pi | None built in; example extension uses `git stash create` in memory | Each `turn_start` | Tracked files only | No (untracked ignored) | `stash apply` offered on fork only with a UI | None |
| DeepSeek harness | None; per-turn tree recorder for display only (private index and object dir) | Turn start and end | `git add --all` into a private index | Yes for git paths (tree diff); outside git only file-tool edits | No restore exists | 30 s per git command; output cap; 500 files per summary |
| goose | None | n/a | n/a | n/a | n/a | n/a |
| Cline | `git stash create` commits in the user's repo under `refs/cline/checkpoints/<session>/<run>`; untracked files as a third parent via a private index | First model call of each user run; opt-in core, on in CLI and VS Code | Tracked changes via stash; untracked non-ignored via `ls-files` and a persistent private index | Yes (tree-based); ignored files and anything outside the repo no | Snapshot of the present first; `HEAD` guard; `update-ref` compare-and-swap; `clean -fd` only if untracked were captured; messages restore forks a session | 64 MiB `ls-files` buffer; persistent index; no size, count or total cap; refs removed only with the session |
| OpenHands | Not in this repository | n/a | n/a | n/a | n/a | n/a |
| Zed | Per-prompt checkpoints removed; archive checkpoint (two commits via a temp index) on thread archive; in-memory per-buffer baselines for review | Archive; first agent touch of a buffer | Archive: `git add --all` in a temp index copy; baselines: only buffers the tools touched | No: no hook from `terminal` | Reject restores base text; rewind rejects all unreviewed edits; archive restore needs HEAD at the original commit | Baselines cost nothing per repository size; archive has no timeout |
| Warp | Handoff snapshot (patches and manifest), not undo | `/handoff` | Repos derived from the last 500 tool calls: written paths plus cwds | Only paths the agent wrote | Refuses while a command or child is active; read-only path gate | 100 files, 25 MiB per file, 25 per request |
| OpenCodex | Nothing read | n/a | n/a | n/a | n/a | n/a |

### Context: prompts, compaction, instruction files, memory

From [round2/synth/07-context-compaction-memory.md](round2/synth/07-context-compaction-memory.md).

Cells are short; the account in section 2 has the detail and the permalinks. "n/d" means the input files do not describe it; "not in repo" means the code that was read does not hold it.

##### 3.1 Compaction: trigger, measurement, summariser

| Project | Trigger and numbers | How size is measured | Summariser model | Window check for the summariser |
|---|---|---|---|---|
| Codex | `min(config, 0.9 x window)`; pre-turn, mid-turn, post-turn (off by default); keeps user messages up to 20,000 tokens; remote V2 keeps up to 64,000 | n/d (status over active tokens; optional prefix baseline) | Same turn model, no tools; OpenAI and Azure use an opaque server-side item | n/d; downshift compacts on the previous model first |
| OpenCode | Before each step at `window - max(15% of window, 16k)` (16k floor only for windows of 32k or more); keeps newest 15,000 tokens; unknown window means it never fires | Last provider-measured count plus length/4 for the tail; images 1,500, PDFs 2,000 | Same model; `agents.compaction.model` overrides | None described; request is fitted to size by a ladder |
| OpenClaw | After each response at `min(window - reserve, 0.75 x window)`; reserve `max(configured, 20k)` capped at 25% of window; keeps 20,000 | Provider usage; estimate from last good usage after an error | Session model; `compaction.model` unless model-locked; fallback chain | None found |
| Hermes Agent | `(window - max_tokens) x 0.50`, floor 0.75 under 512K, cap 85% of usable window; lean tail 2.5% of window clamped 10K..25K | Provider prompt tokens; rough estimate before first response; 413 scored in serialised bytes | `compression` auxiliary slot (main model if unset); 300 s timeout floor | Feasibility probe; trigger lowered to the summariser window; hard error below a minimum |
| Oh My Pi | Window minus max(15% of window, 16,384), or a percentage, or fixed; keep 20,000; summary cap 16,384; 0.8 hysteresis; mid-turn on | Token count floored by a local estimate of stored conversation | `compactionModel`, current, role models, then largest-context model | Handled by selection; no explicit check |
| Pi | `tokens > window - 16,384`; keep about 20,000 | Last provider usage plus chars/4; usage older than the last compaction ignored; image = 4,800 chars | Session's own model | None; no input-size guard |
| DeepSeek harness | `min(0.8 x window, window - reserved output - 65,536)` before every step; keep 16% of message budget | Token meter over the logged request header (route-specific) | Conversation's own route unless overridden | Window of the routed model must be known; model switch compacts first or refuses |
| goose | `min(0.8 x window, 225,000)` before every inference; no verbatim tail, last user text only | Last provider `total_tokens` plus estimate; tiktoken `o200k_base` if no usage | Session model, thinking off, cache writes off | Staged tool-result removal (0, 10, 20, 50, 100%) if the summariser overflows |
| Cline | Before each request at 90% of input limit (about 81% of window when only the window is known); target 70% of trigger; keep last 20,000 estimated tokens; off in core, on in CLI | About 3 chars/token plus images, scaled down by the provider-count ratio (cap 4x); unknown limit means no auto compaction (128,000 fallback for manual and overflow) | Session model unless configured; thinking off; 8,192-token output | n/d; only "input budget exhausted" skips |
| OpenHands | Client `max_size` 240 (planner 100, `keep_first` 6); trigger unit not in repo | not in repo | Planner condenser has its own usage id; rest not in repo | not in repo |
| Zed | Before every request at 90% (or tokens used or remaining) of capacity; auto only if capacity is at least 80,000; keep last 80,000 bytes of user messages | Provider usage of the previous request | `agent.compaction_model`, else thread model | None found |
| Warp | not in repo (server) | not in repo | not in repo | not in repo |
| OpenCodex | The calling client decides; gateway only forwards or runs the summary turn | None | The routed target model | None |

##### 3.2 Compaction: request shape, fallbacks, shrink check, overflow, log

| Project | Summary request reuses the main prefix? | Staged fallback | Check that it shrank | Context-length error | Record in the log and failure midway |
|---|---|---|---|---|---|
| Codex | Full history plus one appended user message, no tools (cache intent not stated, inferred) | Oldest-item drop inside compaction | n/d | In compaction: drop oldest and retry; in sampling: mark full, end turn; guardian: one compact and retry | Compaction item with `replacement_history`; atomic replace; failure leaves history unchanged |
| OpenCode | Yes by design: same system, tools, history plus a final user message | `deliver` ladder: unchanged, history minus tail, flattened transcript clipped over five levels; shrink to 85/70/50/35% on "too long" | Not described (the ladder checks the input, not the output) | Compact once per step then restart the step; no recovery after output started | Message with `keepID`; `Compaction.Ended` advances the instruction epoch; failed auto compaction fails the step |
| OpenClaw | n/d | Safeguard: audit and regenerate (max 3), then structured fallback marked degraded | Quality audit of content, not size | Pre-exits, up to 3 compactions, one tool-result truncation pass | Transcript generation with reason `compaction` (inferred); failure state not described |
| Hermes Agent | No (single user message) | Failure ladder: retry on main model, cooldowns, abort-and-preserve for terminal errors, else deterministic summary | "Ineffective" breaker after two confirmed no-shrink compactions | 413: compress, strip images, retry; "too long": adopt provider limit; "output cap": clamp; max 3 | In place on the same session id; old rows `active=0, compacted=1`; abort leaves history unchanged |
| Oh My Pi | n/d | Method chain `remote, snapcompact, handoff, shake, soft`, each failure falls to the next | Yes: a summary that would not reduce context, or a shake that reclaims too little, falls through | Drop failed turn; optional model promotion; else compaction with reason `overflow` | `compaction` entry with `firstKeptEntryId`, `tokensBefore`, `method`; nothing deleted |
| Pi | No (`cacheRetention: "none"`, serialised text) | None | None | One compact-and-retry per run; failed attempt hidden by `context_edit`; second failure stops | `compaction` entry; old entries kept; failure leaves the log untouched except omission edits |
| DeepSeek harness | Yes: same system prompt, tools and shadowed messages, instruction as final user message | Prune, then compact with zero retention, then retry | Yes: summary rejected unless its framed estimate is strictly smaller than the span | `CONTEXT_WINDOW_EXCEEDED`: prune, compact with zero retention, retry (1 per idle period) | `compaction/start`, `summary`, replacing `user/message`, `end`; failure appends `end` with error; orphan blocks until `session/end-seed` |
| goose | No (flattened transcript in a system prompt) | Tool-result removal inside the summary request | n/d (absence not confirmed) | Hide error, compact, rerun; up to 3 per turn | Old rows kept, `agent_visible` flipped; summary row flagged |
| Cline | No (its own summariser request) | `agentic` then deterministic `basic`; overflow uses only `basic` or a strictly smaller custom compactor | Yes: retry refused unless the request shrank | One forced compaction and one retry | Transcript untouched; compacted view in sidecar `<id>.compaction.json` with source-prefix hash |
| OpenHands | not in repo | not in repo | UI waits for a token drop after manual condense | not in repo | `Condensation` event; log keeps everything |
| Zed | Yes when the same model: tools kept with `tool_choice: none` | None | n/d | None (error shown) | `Message::Compaction(Summary)` inserted; older messages stay for UI |
| Warp | not in repo | not in repo | not in repo | Client shows `ContextWindowExceeded` as an error | Local `agent_conversations` summary column (contents unknown) |
| OpenCodex | The internal turn goes through the normal pipeline | None | Structure only (non-empty, no tool calls) | Combo failover hops to the next target | None |

##### 3.3 Prompt assembly, cache strategy, pruning

| Project | Where prompt text lives | How dynamic state is kept out of the cached part | Old tool output |
|---|---|---|---|
| Codex | Per-model `instructions_template` inside the bundled `models.json`; sections in code | Per-section snapshots; only changed sections rendered as diffs into history; world state persisted in the rollout | Middle-cut at record time (10,000 tokens); never pruned later |
| OpenCode | `system.txt` (15 lines) plus per-model overlay files chosen by substring | Keyed instruction sources: frozen baseline in `system[1]`, changes appended as system messages, frozen in durable events; re-based at compaction | None; call-time cut (2,000 lines or 50 KiB, full text on disk 7 days); images bounded |
| OpenClaw | Code builders, no templates | Everything volatile placed below `SYSTEM_PROMPT_CACHE_BOUNDARY`; prefix cached by hash | Live cap 30% of window; optional cache-TTL soft trim then hard clear, frozen and recorded |
| Hermes Agent | Code plus data in `agent/`; three tiers | Stable, context, volatile tiers built once and rebuilt only at compaction; four Anthropic breakpoints; review fork reuses parent bytes | Model-free projection at compaction; call-time spill (100K chars, 1,500-char preview) |
| Oh My Pi | Handlebars templates | Static blocks first, one trailing `<project-context>` block; pruning waits for a cold cache | `supersedeReads`, `dropUseless` each turn, cache-aware; snapcompact caps results at 2,000 chars |
| Pi | Code sections; tool snippet next to each tool | Prompt and tool changes are system-message patches appended to the transcript | None between compactions; per-tool caps (2,000 lines, 50 KB) |
| DeepSeek harness | Ordered named sections from a table, per-deployment YAML | Dynamic facts in a per-step `runtime-context` user message, appended only when changed; system prompt re-committed only on change | Model-free prune of results over 8,192 chars; spill over 12,500 tokens |
| goose | `system.md` template (39 lines) plus keyed extras sorted by name | Volatile data in a persisted trailing `<turn-context>` message | Spill over 200,000 chars; optional LLM tool-pair summaries (off) |
| Cline | One template with slots; per-model difference only in the tool list | Mode tag on each user message; stale-read rewrites batched at 64 KB | Caps at executor and at request build with a recoverable `cline://cache` URI |
| OpenHands | Server (not in repo) | not in repo | not in repo |
| Zed | not in the study | Only the compaction request keeps tools to reuse the prefix | None separate; terminal capped at 16 KiB |
| Warp | Server (not in repo) | not in repo | not in repo |
| OpenCodex | Client | not applicable | None |

##### 3.4 Instruction files

| Project | Files and order | Budget | Trust |
|---|---|---|---|
| Codex | Global `AGENTS.override.md` or `AGENTS.md`; then root to cwd, one file per directory (`AGENTS.override.md`, `AGENTS.md`, fallbacks) | 32 KiB combined, warn on cut; global file separate; thread-level 10,000 tokens, rejected not cut | Untrusted project returns nothing; root markers only from non-project config layers |
| OpenCode | Global `AGENTS.md`, ancestors to project root, `instructions` globs and URLs; nested files injected after `read` | None found | No project trust prompt found; remote URL must be listed in config |
| OpenClaw | Six workspace files: `AGENTS.md, SOUL.md, IDENTITY.md, USER.md, BOOTSTRAP.md, MEMORY.md` | 20,000 each, 60,000 total; `AGENTS.md` cut with a policy digest | Per-session-type filter (`MEMORY.md` dropped for group, channel, sub-agent, cron); no trust gate described |
| Hermes Agent | First type wins: `.hermes.md`, `AGENTS.md` chain, `CLAUDE.md`, cursor rules | 20,000 chars per file or 6% of window (20K..500K); head 70%, tail 20% | Regex threat scan; a hit replaces a repo context file with `[BLOCKED]` |
| Oh My Pi | Many harness formats by priority, one file per depth, paragraph dedupe, `@path` imports | None reported | No project trust gate (`isProjectTrusted()` always true) |
| Pi | Global, then every ancestor to the filesystem root; one of five names per directory | None | Context files load regardless of project trust |
| DeepSeek harness | `AGENTS.md`, `CLAUDE.md` and `.local` variants; user-global then root to cwd; delivered as user messages | 64 KiB per chain, 1 MiB per file | No trust gate in code read |
| goose | `.goosehints`, `AGENTS.md`; global then git root to cwd; `@` imports | Main file uncapped; imports depth 3, 1 MiB | No trust prompt; Unicode tag strip only |
| Cline | Workspace `AGENTS.md`, `.clinerules`, `.cline/rules`, global `AGENTS.md`, global dirs | None found | No trust prompt for workspace plugins and hooks |
| OpenHands | Server (not in repo); skills from a bundle plus server; `.openhands/hooks.json` | not in repo | No prompt for hooks; fails open |
| Zed, Warp, OpenCodex | not covered | not covered | not covered |

##### 3.5 Memory

| Project | What is stored | Size limits | Who writes | Human approval | Review, ledger, rollback | Session search |
|---|---|---|---|---|---|---|
| Codex | `MEMORY.md`, `memory_summary.md`, rollout summaries, skills; ad-hoc notes | Injected summary 2,500 tokens | Background two-phase pipeline; chat note tool (off by default) | None | None described | None in findings |
| OpenCode | No memory feature found | n/a | n/a | n/a | n/a | None found |
| OpenClaw | `MEMORY.md`, `memory/*.md`, short-term recall | `MEMORY.md` 10,000 chars | Model tools, flush turn before compaction, nightly dreaming job | None | 25% removal guard on consolidation; Skill Workshop changes versioned and undoable | `memory_search` hybrid vector and keyword |
| Hermes Agent | `MEMORY.md` 2,200 chars, `USER.md` 1,375 chars; agent-written skills | As stated | Chat model; background review fork; curator | Off by default; when on: inline prompt or staging queue; background review always staged | Curator ledger with before/after hashes, per-entry rollback, tarball snapshots | FTS5 `session_search`, no LLM |
| Oh My Pi | Five back ends (default off): local, Hindsight, Mnemopi, Sharpshooter | Outbox: 1,000 undelivered events per destination | Automatic extraction or turn counter | None | None reported | Not found |
| Pi | No memory feature found | n/a | n/a | n/a | n/a | Not found |
| DeepSeek harness | No model-written memory (absence confirmed) | n/a | n/a | n/a | n/a | Optional full-text index, in-memory in the web profile |
| goose | `<category>.txt` files, local and global | None described | Chat model | "Confirm with user" in the prompt only | None | None |
| Cline | Not found | n/a | n/a | n/a | n/a | Not found |
| OpenHands | SDK (not in repo) | not in repo | not in repo | not in repo | not in repo | Event search through the server |
| Zed, Warp, OpenCodex | not covered | not covered | not covered | not covered | not covered | not covered |

### Edit tools and the mechanics behind the scenes

From [round2/synth/08-edit-tools-mechanics.md](round2/synth/08-edit-tools-mechanics.md).

Labels in cells follow section 2. "None found" means the author searched and the file says so; it is `absence confirmed` only where section 2 says so.

##### 3.1 Edit tools

| Project | Edit format | Match ladder | Staleness check | Read-before-edit | Atomicity | Post-edit step | Tracking for review |
|---|---|---|---|---|---|---|---|
| Codex | Patch envelope (`apply_patch`), multi-file, move, delete | 4 tiers, tier-first, first match from a cursor; no uniqueness | None; approval not bound to content hash | None | Verify all in memory, then apply sequentially from re-parsed text; partial writes stay | None found (hooks only) | None (file undo removed) |
| OpenCode | `edit` string replace; `patch` envelope for GPT-family models | `edit`: exact, Unicode fold, trailing-ws lines; `patch`: exact, trimEnd, trim, trim+Unicode | None (process-local per-path lock) | None | `edit` single file; `patch` prepares all, writes sequentially, reports partial | Formatter by extension (first success wins); LSP is a TODO | Shadow Git tree captured around each model step, path attribution, stage/commit undo |
| OpenClaw | `edit` multi-edit; `apply_patch` | Exact, then NFKC/trailing-ws/quote fold used for lookup only; unique; error lists similar lines | None; per-file queue; re-reads after write to verify | None | Per file (queue + verify) | None found | Worktree snapshot git refs (unchecked) |
| Hermes | `patch` (replace and V4A), `write_file` | 9 strategies incl. indentation and similarity (0.5/0.7, 0.8); `replace_all` refused for similarity | `patch`: mtime warning only; `write_file`: hard refusal | `write_file` enforced; `patch` warns | Not read | Lint output delta (new errors only); LSP unread | Shadow git per project, write ledger fed by file tools only; off by default |
| Oh My Pi | Hashline (default), `apply_patch`, `patch`, `replace`, `sloppy` | `replace`: exact, 0.99/0.98/0.975/0.97, fuzzy 0.95 with 0.97 and 0.08 margin | Hashline: 16-bit file tag; stale tag rebased if provably safe, else refused | Hashline: seen-line guard in code; `replace`: prompt only | Hashline: all sections staged before any write; `sloppy`: partial | Not traced | Conversation-only rewind; no file undo |
| Pi | `edit` multi-edit, matched against the original | Exact, then NFKC/ws/quote fold of the whole file; unique; whole touched lines rewritten from normalised text | None; per-file queue | None | All edits in memory, one write; failure leaves file untouched | None | None built in |
| DeepSeek | `edit` literal; `apply_patch` off | None (literal, CRLF-normalised); unique | CAS inside the lock via `dev:ino:size:mtimeNs:ctimeNs`, **plugin disabled in shipped base** | Same plugin, disabled; prompt still asks | Atomic write under per-target lock | None found | Per-turn git tree recorder, memory only, no restore |
| goose | `edit`, `write` | None (exact, unique) | None | None (no read tool) | `fs::write`, not atomic | None (`AfterFileEdit` hook exists) | None |
| Cline | `editor` replace; `apply_patch` (per model) | `editor`: none; patch: exact, trailing-ws, trim, similarity 0.66 | None | None | Patch: reject whole patch before writing, then sequential writes | None found | `git stash create` commits under private refs, per user turn; restore transaction |
| OpenHands | In another repository | Not read | Not read | Not read | Not read | Not read | Not read |
| Zed | `edit_file` fuzzy line matcher, streamed into the buffer | Exact, then DP alignment with Levenshtein 0.8 per line and 0.8 whole-match ratio, line hint | Soft: mtime vs last read, appended to failure text | None (prompt only) | Applied to buffer, saved on completion even on later-edit failure | Optional format-on-save | In-memory action log with per-buffer baselines, per-hunk keep/reject |
| Warp | On server | Not read | Not read | Not read | Not read | Not read | Diff review UI in client (documentation only) |
| OpenCodex | `write_file` with `expectedSha256` | n/a (whole file) | Hard: SHA-256 CAS | Implicit (hash needed) | Temp file + atomic rename, serial chain | None | None |

##### 3.2 Match ladders, tier by tier

`E` exact, `RS` trailing whitespace ignored, `T` trim both sides (ignores indentation), `U` Unicode fold (curly quotes, dashes, odd spaces; NFKC in Pi/OpenClaw), `S` similarity score. "Unique?" is whether a match found at the winning tier must be unique.

| Project / tool | E | RS | T | U | Other tiers | Unique? | Reports tier to the model? |
|---|---|---|---|---|---|---|---|
| Codex `apply_patch` | 1 | 2 | 3 | 4 (with T) | retry without trailing empty line | No (first match from cursor) | No |
| OpenCode `edit` | 1 | 3 (line-wise) | no | 2 | none | Yes; `replaceAll` | Tool description tells the model |
| OpenCode `patch` | 1 | 2 | 3 | 4 | none | Not stated | No |
| Pi `edit` | 1 | in fuzzy space | in fuzzy space | in fuzzy space (NFKC) | none | Yes (counted in normalised space) | No |
| OpenClaw `edit` | 1 | in fuzzy space | in fuzzy space | in fuzzy space (NFKC) | none | Yes | Not stated |
| Hermes `patch` | 1 | `line_trimmed` 2 | `whitespace_normalized` 3, `indentation_flexible` 4 | `unicode_normalized` 7 | `escape_normalized`, `trimmed_boundary`, `block_anchor` (0.5/0.7), `context_aware` (0.8) | Yes, else lists locations; no `replace_all` after similarity | Not stated |
| Oh My Pi `replace` | 1 | 0.99 | 0.98 | 0.97 | comment-prefix 0.975; fuzzy prefix and substring at 0.95 | Yes: fuzzy winner needs 0.97 and a 0.08 margin | Not stated |
| Cline `apply_patch` | 1 | fuzz 1 | fuzz 100 | canonicalised first | similarity 0.66 (fuzz 1000), EOF penalty | Not stated | **Yes, fuzz level in the success text** |
| Zed `edit_file` | 1 | no | no | no | Levenshtein 0.8 per line, 0.8 ratio, costs 1/3/10 | Yes via line hint, else line numbers | Internal `is_exact`; user sees diff |
| DeepSeek, goose, Cline `editor` | 1 | no | no | no | none | Yes | n/a |

##### 3.3 Hash-anchored edits: described versus implemented

| Property | How it was described (Round 1) | What the code does (Oh My Pi) | Label |
|---|---|---|---|
| Hash granularity | Every line carries a short content hash (`2:f1|...`) | One 16-bit xxh32 tag **per file**; lines are bare numbers | checked in review |
| What a tag detects | A changed file makes hashes mismatch and the edit is rejected "before corruption" | Tag mismatch triggers recovery; rejected only if a uniform rebase cannot be proved | corrected in review |
| Per-line protection | Implied by per-line hashes | The **seen-line guard**: every anchored line must have been shown in full by a read | checked in review |
| Collision | Not described | 1 in 65,536; a colliding live tag skips recovery entirely, guarded only by seen lines | checked in review (consequence inferred) |
| After restart | Not described | Snapshots and seen-line provenance are in memory and are lost; only over-long lines are refused | checked in review |
| Universality | Hashline presented as the edit format | Default, but substring-selected models (kimi, deepseek, glm flash, qwen 3.8 and others) get `replace` instead | checked in review |
| Read-before-edit | Prompt: "Tool errors on edit before read" | Enforced for hashline (tag and seen rows); not enforced in `replace` | absence confirmed |

##### 3.4 Shell execution mechanics

| Project | Spawn | Default timeout; model-settable? | Kill | Background | After normal exit | Output cap (model sees) | Environment |
|---|---|---|---|---|---|---|---|
| Codex | Unified exec, resumable processes | 250 ms to 30 s yield per call; 300 s background max | Pruned process terminated; table of 64, 8 recent protected | `write_stdin` polls | Held in process table | 10,000 tokens per call; 1 MiB per process buffer, head and tail | Inherits (safety file) |
| OpenCode | `-c`, detached group, `stdin` ignore | 2 min; background none | 3 s force kill | `background` flag; Rust PTY daemon survives restarts | Not stated | 2,000 lines / 50 KiB, full output file, 7-day cleanup | Not scrubbed |
| OpenClaw | Gateway/node/sandbox host | 48 h per run | Not read | Not read | Not read | 30% of window, hard cap 16k/32k/64k chars | Inherited minus injection keys |
| Hermes | Local; `base.py` | Over 600 s foreground promoted to background | Process group; owner-death supervisor | `background`, trailing `&`; `notify_on_complete` | Tracked | Spill at 100,000 chars; 1,500-char preview | Not covered here |
| Oh My Pi | Persistent native shell per key; fork for concurrent | 300 s default, 1 to 3600 s, **0 disables** | SIGTERM tree, grace, re-walk, SIGKILL; session quarantined | Auto after 60 s; `async`; services | Shell retained while jobs live | Spill at 50 KiB; 20 KiB head + 20 KiB tail; artifact cap 16 MiB | Inherited (safety file) |
| Pi | `spawn(shell, ["-c"], detached)` | **None** | SIGKILL group (SIGSTOP first on macOS) | None | Detached children killed on Pi SIGHUP/SIGTERM only | 2,000 lines / 50 KB tail, full file | Full parent |
| DeepSeek | Fresh shell (docs) | Documented only | `SANDBOX_UNAVAILABLE` mapping | Documented only | Not read | Spill at 12,500 tokens | Name-pattern scrub (safety file) |
| goose | `-c`, own group (Job Object on Windows) | 300 s; model sets `timeout_secs`; **0 disables**; no upper bound | `kill(-pgid, SIGKILL)` on timeout, cancel, drop | None | **Disarmed**: backgrounded work survives; `setsid` escapes; no PDEATHSIG for shell | 2,000 lines / 50,000 bytes, 8 rotating files, tail 50 lines; collection unbounded | Full inherited |
| Cline | `spawn`, `detached: true` | 30 s (metadata says 60 s, not enforced) | SIGKILL group; `setsid` escapes | Human-only "Proceed While Running", `unref` + log | Detached survives the run | 48,000 chars, 24k head + 24k tail | Full inherited |
| Zed | Terminal tool | Not read | Not read | Terminal threads | Not read | 16 KiB; `head_lines`/`tail_lines` | Not read |
| Warp | User's own PTY | Agent-set poll delay up to 120 s | Not read | Long-running blocks, control hand-off | Not read | Not read | User's shell env |
| OpenCodex | `bwrap` argv, no shell | 1 to 60 s | Kill on timeout, abort, output overflow; `--die-with-parent` | None | Not applicable | 256 KiB | `--clearenv` plus 4 variables |
| rizzy-harness (`main`) | `bash -c`, own process group, allow-listed env | 600 s per tool call; `timeout_secs` capped at 3600 | SIGTERM, 2 s, SIGKILL on the group | None | Not in inventory | 30,000 chars head and tail; stream 1 MiB; filtered text 2 MB retrievable | Allow-listed |

##### 3.5 Read, search and tool-result caps

| Project | Read / search caps | Whole-result cap | Overflow handling |
|---|---|---|---|
| Pi | `read` 2,000 lines / 50 KB head; `grep` plus 500 chars per match line | Each built-in tool; none in the loop or for extension tools | Bash spills full output to temp file |
| Cline | Reads 2,000 lines, 2,000 chars per line, 48,000 chars per window; search 48,000 per query | 8,000 chars (50,000 for reads) re-cut at request build; 6 MB aggregate | `cline://cache/...` URI, 16 MiB cache; stale re-reads rewritten |
| OpenCode | Read pages 2,000 lines / 50 KiB (Round 1 documentation) | 2,000 lines / 50 KiB per result | Full output file, 7 days; images 25 to 15 MiB |
| OpenClaw | None in findings | min(30% window, 16k/32k/64k chars) | Truncate with "narrow args" note |
| Hermes | `read_file` pinned to infinity (exempt from spill); `search_files` 4th identical call refused | 100,000 chars; 200,000 per turn | Preview 1,500 chars plus path |
| DeepSeek | `read` exempt | 12,500 tokens | Head/tail plus spill file; **fails open to inline** |
| goose | No read or search tool | 200,000 chars generic; shell 2,000 lines / 50,000 bytes | Temp file, 8 slots |
| Codex | Not in findings | 10,000 tokens per exec call | Middle truncation marker |
| rizzy-harness (`main`) | `read` 400-line window; 32 MiB streaming cap only on `integration/gate3`; `grep` 200 results | 30,000 chars | `tool_output` re-read by id |

##### 3.6 Tracking agent edits for review or undo

| Project | Mechanism | Granularity | Covers shell-made changes? | Cost control | Persisted? |
|---|---|---|---|---|---|
| OpenCode | Shadow git, tree before and after each model step | Per step, path list | Only inside git tree; ignored files and outside-project are not | Seeded index, alternates, coalescing, packing; O(tree) per step; no retention | In the store |
| Hermes | Shared bare repo, ref and index per project | Before write-class tool, or regex-matched destructive command; one per dir per turn | Partly (regex); **restore ledger ignores them** | 50,000-file skip, 10 MB file drop, 20 snapshots, 500 MB, 7 days; off by default | Disk |
| Cline | `git stash create` + untracked third parent, private refs | Once per user turn | Yes (whole tree), not gitignored | Persistent private index; no size caps | Disk |
| DeepSeek | Git trees at turn start and end, private index, alternates | Per turn | Yes within git | 30 s per git call, 500 files, output cap | Memory only |
| Zed | In-memory action log, baseline at first touch | Per buffer | **No** | Nothing scanned | **No** |
| OpenClaw | Worktree snapshot refs | Per worktree | Not read | Fail to keep | Refs |
| Codex, goose, Pi, Oh My Pi (files) | None | | | | |
| rizzy-harness (`main`) | `git add -A` into a private index of a shadow repo, before and after each agentic turn, latest 10 kept | Per turn, whole tree | Yes | Count cap only; scans the whole tree | Disk |

### Sub-agents, orchestration, model roles, fallback and retry

From [round2/synth/09-subagents-models-retry.md](round2/synth/09-subagents-models-retry.md).

##### 3.1 Sub-agents

Trust labels are in section 2; cells give the main fact only. "Shared" means the child works in the parent's directory.

| Project | Creation | Context given | Permission relation to parent | Isolation | Result return | Caps (real defaults) |
|---|---|---|---|---|---|---|
| Codex | In-process child thread from `spawn_agent`; guard object cleans up a cancelled spawn | Prompt only; `fork_context: true` copies full history | Inherits; a role patch can only narrow; parent policy re-applied last | None found (shared) | Last assistant message in a status value, plus a queue-only note in the parent's mailbox; no schema | Depth 1; 6 threads (V1) or 3 children (V2); no time or token budget |
| OpenCode | Child session row with `parentID`, run as a job | Fresh; prompt only | Child agent's rules plus the parent session's rules (session rules last); "read-only" `explore` allows shell | None found | Last completed assistant text in `<subagent>`; background mode posts a synthetic message that wakes the parent | Depth 1 (`experimental.subagent_depth`); no concurrency cap found; optional per-agent `steps` |
| OpenClaw | Child session plus gateway `agent` run | Isolated by default; `fork` copies transcript; `AGENTS.md` only | Inherits parent's allow and deny lists; hard deny list for hidden children; leaves cannot spawn | Optional `worktree` at session creation; idle GC 7 days, keeps when unsure | Four modes (`announce`, `collector`, `quiet`, `thread-direct`); steered into the parent's turn or a direct turn with an idempotency key | Depth 5; 5 active children per spawner; width-8 lane per spawner |
| Hermes Agent | `AIAgent` objects built on the parent's thread, run on a thread pool | Goal, parent-written `CONTEXT`, workspace path, project context files; no memory | Parent's toolsets minus 5 tools; dangerous commands auto-deny (opt-in approve) | Opt-in worktree (off by default), silent degrade off the local backend | Summary only: redacted, 24,000-char cap, JSON with `status` and `exit_reason`; background results re-enter as a message | 10 concurrent; depth 1 (no ceiling); 250 iterations per child; 50 spawns per turn |
| Oh My Pi | In-process `task` tool (main thread, then workers) | Role prompt plus parent-written `context`; no parent conversation; `AGENTS.md` excluded | Forced `yolo`; user deny and prompt rules still apply; prompts fail closed | Opt-in overlay clone with patch-back | Explicit `yield` tool, schema-validated | Depth 2; concurrency 32; 200 requests soft (1.5x stop); idle 420 s; no wall clock |
| Pi (example) | Spawned `pi` process | Task plus agent file | None: full env, default tools | None | First text of last message, 2,000 lines or 50 KB | 8 tasks, 4 at once; no depth limit |
| DeepSeek harness | In-process activation; or Codex, Claude Code, ACP process | Spawn: none; fork: parent's log up to last `turn/end` | Sandbox override copied; approval pinned `never`; written into the child's log; external children use fixed config independent of the parent | None (experimental worktree tool) | `subagent-settled` notice to the parent (queued or steered); final text or `outputSchema` | Depth 1; 8 active per tree; workflow 1,000 total and `min(16, cores-2)` concurrent |
| goose | Child session record; a pipeline step runs it | Fresh plus task; own system prompt | Inherits mode; child approvals forwarded to the parent's user | None (working dir inside parent's) | `final_output` tool, default `{summary}`; hidden user message | Depth 1; 5 concurrent; no wall clock; 1,000 turns |
| Cline | In-process `SessionRuntime` | `systemPrompt` and `task` only | None: no policies, no approval callback | None | `{text, iterations, finishReason, usage}` | None; declared `timeoutMs` never read |
| OpenHands (client only) | Browser executes `launch_child_conversation`, a new conversation on the Agent Server | Task brief only | Parent's profile; policy from global settings; no narrowing | Worktree by default, silent fallback to shared (agent told) | Hidden user message with child id and status | None in the client |
| Zed | Not in the input | | | | | |
| Warp (client only) | Client opens a hidden pane (Oz, Claude Code, Codex, OpenCode) or cloud child, with a server task row | Not seen; mail via a server bus | Inherits profile; CLI children launched with their permission systems off | None seen | Mailbox messages | No numeric cap; depth is a feature flag |
| OpenCodex | None (gateway) | | | | | Send budget keyed by workflow root |

##### 3.2 Models, fallback, retry, cooldowns

| Project | Roles or slots | Fallback trigger | Retry numbers | `Retry-After` | Cooldowns and pools |
|---|---|---|---|---|---|
| Codex | No slot table; child model: call, config, then parent; role can pin | None (only startup, compaction retry, one WebSocket-to-HTTPS switch) | 200 ms doubling, plus or minus 10%; 5 stream retries (request 4); connection loss retries forever 5 s to 60 s | Sets the wake instant; cannot extend the count | None found |
| OpenCode | Session, agent, default; hidden `title`, `compaction`, (`summary` unused); small model by family list | None | 10 retries, 2 s to 10 s, jittered (about 84 s); no-network 2 s to 30 s up to 1 hour; 3 after timeouts | Raises the delay, capped at 15 minutes; `x-should-retry` overrides | None |
| OpenClaw | Primary plus `fallbacks`; `subagents.model` | Per candidate, with cooldown decision (skip, probe, suspend) | Not read | Rate-limit cooldown is provider timing only | Per reason, persisted: transient 30 s, 1 min, 5 min; billing 10 min to 24 h; auth_permanent 10 min to 1 h; sticky fallback 10 min idle, 30 min cap |
| Hermes Agent | Main plus 19 `auxiliary.<task>` slots, each with `fallback_chain` | Rate limit and billing at once; transport after 2 retries; auth after refresh | 3 attempts, jittered 2 s to 60 s; aux `transient_retries` 2 | Honoured, capped; 429 adaptive | Primary cooldown 60 s to 4 h; pool: 401 5 min, 429 1 hour, sole credential 60 s; per (credential, model) |
| Oh My Pi | 16 roles; `@role` aliases; per-key chains | Retryable errors when `modelFallback`; not stale usage limit, thinking loop, first socket drop | 10 retries, 500 ms doubling, cap 8 s, up to 25% downward jitter | Above 300 s fails fast | Failing selector suppressed for retry-after, reason backoff, or 5 min |
| Pi | Scoped list with thinking level; virtual-model router | None built in; router sees reason `retry` | Agent layer 3, 2 s doubling, cap 60 s, no jitter; provider layer 0 | Dormant by default | None |
| DeepSeek harness | Per-agent route in the log; title route | None | 5, 500 ms, cap 10 s, plus or minus 10% | Longer than max: give up | None |
| goose | One per session; sub-agent model; toolshim; two classifiers | None | 3 (4 attempts), 1 s doubling, cap 30 s, 0.8 to 1.2 jitter; empty response 3 | Used exactly | None |
| Cline | None | None | 3, 1 s, 2 s, 4 s, cap 15 s, no jitter | Not handled in the findings | None |
| OpenHands | Profiles, `title_llm_profile`, classifier router | Not in the client | Snapshot only: `num_retries 5`, multiplier 8, min 8 s, max 64 s | Not in the client | Not in the client |
| Zed | Thread model plus `compaction_model` | None | Rejections: `retry_after` or exponential; read and send errors 3 at a fixed delay | Honoured | None |
| Warp | Server-side | Server-side | 3 per request; delays not in the findings | Not in the client | None |
| OpenCodex | Routing ladder; named combos | `decideComboFailure`: hard stops, target-local hops, statuses | 3 attempts on 5xx per logical request including recovery legs | Cooldown duration: `Retry-After` (cap 1 day) first | Persisted absolute expiry; default 60 s, spent windows 10 min, rate limit 5 s; pool strategies |

### Extensibility and unattended operation

From [round2/synth/10-extensibility-unattended.md](round2/synth/10-extensibility-unattended.md).

"Not stated" means the findings read do not say; it is not a claim that the project lacks it. Trust labels are in section 2.

##### 3.1 MCP transports, authentication and stdio environment

| Project | Transports | OAuth | Environment given to a stdio server | Annotations and approval | Limits |
|---|---|---|---|---|---|
| Codex | stdio, Streamable HTTP (`rmcp`); stdio can run on an executor | loopback callback with `state`; CIMD or DCR registration; keyring with file fallback | not stated | not stated | 30 s start, 300 s call, 8 MiB stdio line |
| OpenCode | stdio (through the location's `Environment`), Streamable HTTP | on by default; code not read | not stated (the agent shell gets full `process.env`) | no annotations used; every call asserts `<server>_<tool>` | 30 s / 30 s / 12 h, 16 MiB frame |
| OpenClaw | stdio, SSE, Streamable HTTP | per operator or per requester identity | not read | not read | not read |
| Hermes | stdio, HTTP (httpx) | not read | **allow-list**: `PATH, HOME, USER, LANG, LC_ALL, TERM, SHELL, TMPDIR`, `XDG_*`, plus the server's own `env:` | cached rows with missing annotations treated as write-capable; description scan warns only | 10 MiB HTTP body cap |
| Oh My Pi | stdio, HTTP, SSE | PKCE `S256`, loopback callback, DCR; keyed by profile and server URL | **full inheritance** (`{...Bun.env, ...config.env}`) | all MCP tools `write` tier regardless of annotations | 30 s call, 250 ms start-up window, 30-day tool cache |
| Pi | stdio, Streamable HTTP (SSE refused) | not stated (a provider-token option refused in project config) | **full inheritance** (`inheritEnv` exists, never set) | not stated; calls pass `tool_call` hooks | 16 MiB message, 60 s, 20 KiB result cut |
| DeepSeek | stdio, Streamable HTTP (static headers) | none (absence confirmed) | **scrubbed**: `scrubbedParentEnv()` plus configured env | none: no MCP code produces `ask` | reconnect 500 ms to 30 s, 10 attempts |
| goose | stdio, builtin, platform, Streamable HTTP (+ Unix socket); no SSE | `client_id`, `client_secret_key`, `scopes` in config | inherited plus configured; configured names filtered through a 31-key disallow list | **trusts server `readOnlyHint`** in SmartApprove | 300 s |
| Cline | stdio (own parsers), SSE (default when `type` omitted), Streamable HTTP | tokens refreshed on connect, no interactive flow; stored in a settings file | **full inheritance** | no per-tool `autoApprove`; all MCP tools approved by default in the IDE | not stated |
| OpenHands | config only (stdio, SSE, HTTP); client in SDK | OAuth2 probe in UI | not in this repository | not in this repository | not in this repository |
| Zed | not read | not read | ACP agents: full inheritance | none for external agents | not read |
| Warp | stdio, Streamable HTTP, SSE (`rmcp`) | OAuth, secure storage | **full inheritance** (no `env_clear`) | allow-list and deny-list by server | not stated |
| OpenCodex | no MCP client | n/a | n/a | n/a | n/a |

##### 3.2 Tool deferral, and how the change is recorded

| Project | Mechanism | Default | How a change reaches the model without breaking the prompt cache |
|---|---|---|---|
| Codex | BM25 `tool_search`; per-server `omit_tools_from` | on for models whose catalog row has `supports_search_tool` | `list_changed` revision in transport order; typed world-state sections diffed per step and persisted |
| OpenCode | Code Mode catalog (2,000-token budget, round-robin per namespace) plus an in-code `search()` | MCP and plugin tools are Code Mode unless `codemode:false` | keyed instruction sources hashed; durable `session.instructions.updated` event with frozen text; later changes appended as `system` messages |
| OpenClaw | tool search (`tools` or `directory` mode) | on (`enabled: setting ?? true`) | byte-stable cached prefix; change recording not read |
| Hermes | three bridge tools (`tool_search`, `tool_describe`, `tool_call`) | on whenever a deferrable tool exists | stateless catalog rebuilt at every assembly; skills index re-rendered only at compaction |
| Oh My Pi | `xd://` devices driven through `read` and `write`; no search tool | `tools.xdev` true, docs mode `catalog` | tool arrays sorted by name; tool cache keyed by config hash |
| Pi | per-server and per-tool exposure: `direct`, `model-only`, `codemode`, `deferred`, `hidden`; BM25 `tool_search` | `codemode` | system-message delta in the transcript with `toolsAdded` and `toolsRemoved`; restored on resume, `/tree` and fork |
| DeepSeek | none: every schema every step | n/a | whole-generation swap, all of a server's tools or none |
| goose | `enable_tools` lookup, names only | off (`GOOSE_TOOL_DEFERRAL=lookup` to enable) | enabled set recomputed by scanning the conversation; deferred names sorted; per-turn lease freezes the catalog |
| Cline | none | n/a | rules re-read per request (cache effect not examined) |
| Warp | none in the client | n/a | n/a |
| OpenHands, Zed, OpenCodex | not visible, not read, n/a | | |

##### 3.3 Code mode and the approval path

| Project | Runtime and isolation | Default | Do inner calls go through approval? | Limits |
|---|---|---|---|---|
| OpenCode | own interpreter over `acorn`, in the server process; language-level only; host `fetch` | on for MCP and plugin tools | **yes**: each nested call runs through the same `run` and its own `assert`; `fetch` asserts `webfetch` and re-checks redirect hops | caps on strings, arrays, promises, depth; timeout between steps |
| Pi | QuickJS-wasm in a worker thread, frozen built-ins; language-level only | tool inactive; MCP auto-activates it | **yes**: the tool pipeline is re-entrant (`runToolCall`); `tool_call` hooks see nested calls with the parent id | 256 MiB, two workers; **deadline `Infinity` unless the script sets one** |
| goose | Boa interpreter, embedded; no fs, network, shell | off (Cargo feature and runtime) | **no**: `lease.call` directly; inspectors and `PreToolUse` hooks skipped; one approval for `execute_javascript` | 2 M instructions, 4 workers, 300 s |
| Hermes | `execute_code`: per-session Python kernel on the host (local backend) | tool present | whole-script approval only on gateway and ask surfaces; script's `terminal()` calls are guarded per call, direct `subprocess` is not | env allow-list is the only containment |
| DeepSeek | `workflow`: JavaScript in a `vm` context inside the sandboxed PTC process under the caller's file policy; refuses to run if policy cannot be enforced | tool exists; shipped default not stated | not stated | 1,000 children, 5 s synchronous, no overall deadline |
| Codex | Code Mode runtime; `code_mode_only` set for 11 of 12 bundled models though the feature flag is off | by catalog row | not traced | not traced |
| OpenClaw, Oh My Pi, Cline, Warp, Zed, OpenHands | none found or not read | | | |

##### 3.4 Skill loading

| Project | Sources | What is in the prompt | Budget | Trust gate | Executed at load? |
|---|---|---|---|---|---|
| Codex | config layers, plugins, `.agents/skills` from cwd to project root | listing format not in findings | not stated | **none** (absence confirmed), repo skills load though project layer is untrusted | nothing found; scripts go through sandbox and approval |
| OpenCode | `.opencode`, `.claude`, `.agents`, remote `index.json` (same origin only) | id, name, description, filtered by permission | **none** | none found | skills: no; custom commands: `` !`cmd` `` runs unchecked |
| OpenClaw | six tiers; no ancestor walk | catalog, bodies on demand | **150 skills, 18,000 characters**, degrades to compact form | gating by OS, binaries, env; no trust prompt found | no; Skill Workshop writes skills by default |
| Hermes | skills directory, hub | 60-character descriptions | 60 characters each | install-time regex scan; agent writes: opt-in scan and opt-in approval | no; skill-declared env names cannot register managed credentials |
| Pi | `.pi/skills`, `.agents/skills` up to git root, user dirs | name, description, absolute path | **none** (500 skills means 500 entries) | project skills gated by path-keyed trust | no |
| Cline | `.clinerules`, `.cline`, `.claude`, `.agents`, user dirs | names inside the `skills` tool description | not stated | none | not stated |
| OpenHands | build-time catalog plus user and project skills from the server | decided server-side | n/a | allow-list for catalog, deny-list for user skills | n/a |
| Oh My Pi | capability registry reads skills from many harnesses' directories | not detailed | not stated | none | not stated |
| goose, DeepSeek, Zed | not read | | | | |

##### 3.5 Plugin and hook trust gate

| Project | What executable code a repository can bring | Gate | What a hook's trust is bound to |
|---|---|---|---|
| Codex | hook commands, plugins | project trust level disables the project layer; per-hook trust state in the user layer only | hash of `{event, matcher, handler}`; plugin tree hash re-verified at execution; **not the bytes of a script a repo hook calls** |
| OpenCode | in-process plugins from `.opencode`, `.claude`, `.agents`; command templates | none; admin `plugin.use` policy only | nothing |
| OpenClaw | `before_tool_call` plugin hooks; install trust not read | not read | not read |
| Hermes | in-process plugins; shell hooks | install-time regex scanner (blocks `dangerous`); shell hooks need first-use consent per (event, command) | (event, command) string |
| Oh My Pi | TS and JS hooks and tools imported from `.claude/`, `.codex`, plugins; project MCP | **none** (`isProjectTrusted()` is always true); `--trusted-extension` opt-in narrowing | nothing |
| Pi | `.pi` extensions, project `mcp.json`, skills, packages | path-keyed trust (`trust.json`), decided by user-level code first | nothing: a path boolean |
| DeepSeek | installed plugins in the host process | `plugin_manager` escalation per call | not read |
| goose | `.agents/plugins` hooks and MCP servers | **none**; auto-enabled on discovery | nothing |
| Cline | `.cline/hooks`, `.cline/plugins` | **none** (home-only lane for Agent Plugins) | nothing |
| OpenHands | `.openhands/hooks.json` | none; plugin-launch checkbox is a one-shot UI gate | nothing |
| Warp, Zed, OpenCodex | not applicable; Warp's cloud driver passes `--dangerously-bypass-hook-trust` to Codex | | |

##### 3.6 Hook failure mode

| Project | Hook crash, timeout or bad output | Who can loosen a decision |
|---|---|---|
| Codex | **fail open** for `PreToolUse` and `PermissionRequest` (run marked `Failed`); deny wins over allow | a `PermissionRequest` hook allow approves and pre-approves the sandbox retry |
| OpenCode | `permission.evaluate` has no failure channel (throw becomes a defect; inferred fail of the call); only `tool.execute.before` may fail | hooks can change ask to allow |
| OpenClaw | **fail closed**: "Tool call blocked because before_tool_call hook failed" | hooks can rewrite parameters |
| Hermes | plugin hook exception **fail open**; `approve` escalation fail closed; shell hooks fail open unless `fail_closed` | a hook may only escalate to the human gate (`approve`), or `block`; `modify` happens before the tool's own approval check |
| Oh My Pi | **fail closed**: timeout or throw blocks, 30 s | none: approval is re-resolved on the rewritten input |
| Pi | `tool_call` **fail closed**; all other events open; arguments mutable with no re-validation | none in the seam itself |
| goose | **fail open** unless `PreToolUse` with `on_failure: block`; invalid matcher silently drops the rule | policy deny wins |
| Cline | file hooks **fail open** on every failure; `cancel` stops the whole run; plugin hooks ignored after errors | `review` flag parsed and unused |
| OpenHands | load failure starts the conversation with **no hooks** | not in this repository |
| DeepSeek | `beforeTool` in pi-durable not applicable; its own hook bridges not read (unsettled) | n/a |

##### 3.7 Unattended authorisation, default mode, catch-up, overlap

| Project | How an unattended run is authorised | Mode when no human is there | Catch-up after downtime | Overlap | Dispatch journal / idempotency |
|---|---|---|---|---|---|
| Codex | `exec`: same engine, approval policy `Never`, approval and user-input requests rejected; no engine scheduler | reject | n/a | n/a | per-thread durable queue, dispatched when idle |
| OpenCode | `Unattended` ruleset: an ask is allowed only if the ruleset allows every resource, else denied; `run` rejects asks, `--auto` answers "once" | deny unless explicitly allowed | **skip** missed "every" slots, no burst | not stated | not stated |
| OpenClaw | cron standing grant bound to job, config revision and operation, minted by an earlier `allow-always`, consumed at spawn; falls back to a prompt | prompt or deny; risky shapes always prompt | restart: conditional retry, **no counter** | not stated | delivery queue records `platform_send_started_at`; webhook idempotency cache in memory only |
| Hermes | `cron_mode: deny`; toolset denylist; agent cannot set model or provider; prompt tripwire; scripts only under one directory | **deny** | **at-most-once**: advance `next_run_at` before dispatch | admission through the per-session queue; parallel pool | per-fire journal keyed by hash of job and request, fsynced; pause on unknown outcome |
| Oh My Pi | none (no scheduler); headless throws if a prompt is needed but default mode is `yolo` | yolo | n/a | n/a | n/a |
| Pi | none (no scheduler, no permission system) | allow | n/a | n/a | durable tasks: restart limit 5 |
| DeepSeek | schedule delivers a user message; webhook session gets a `read-only` preset as a log event; headless `ask` with no answerer fails closed | preset-dependent | **latest-only**, once | slot held until the agent is idle | receipt committed after durable flush; messageId matched in inbox; webhook `deliveryId` stored, never checked |
| goose | recipe schedule | **`Auto`** regardless of user mode | none | **allowed** (only `paused` checked) | none |
| Cline | cron spec; unknown mode becomes `yolo`; `autoApproveTools` default true; agent-created tasks start `pending_approval` | **yolo** | not settled | not stated | not stated |
| OpenHands | automation server (not in repo); runs as creator or a service account; default timeout 600 s | not in this repository | not in this repository | not in this repository | not in this repository |
| Warp | autonomous run: anything that would ask returns `Denylisted`; ACP route allows all but denylist and protected paths; foreign CLI children launched with their own permissions off | mixed | not in this repository | not in this repository | not in this repository |
| OpenCodex | Remote Workspace hub agent: `approvalPolicy: "never"` with a fixed capability set | fixed set | n/a | n/a | n/a |

##### 3.8 Importing other harnesses' configuration and transcripts

| Project | Configuration read from other harnesses | Transcripts imported |
|---|---|---|
| Oh My Pi | capability registry reads skills, rules, commands, **hooks**, custom tools, MCP servers, context files, settings from Claude, Codex, Gemini, OpenCode, Cline, Cursor, Windsurf, VS Code, GitHub, `.agents`, plugins; TS and JS hooks and tools become executable | Claude Code and Codex JSONL |
| goose | agents from `.claude/agents`, `~/.claude/agents`, `.agents/agents` | Claude Code, Codex, Pi JSONL (CLI and desktop) |
| Cline | skills from `.claude` and `.agents` | Claude Code, Codex, OpenCode |
| OpenCode | skills, agents, plugins from `.claude` and `.agents` roots | none found |
| Warp | global third-party MCP configs (`~/.claude.json`, Codex, Agents) auto-spawn if a toggle is on; project-scoped servers never auto-spawn | none |
| Pi, Codex, OpenClaw | `.agents/skills` | none found |
| Hermes, DeepSeek, OpenHands, Zed, OpenCodex | none found | none found |
