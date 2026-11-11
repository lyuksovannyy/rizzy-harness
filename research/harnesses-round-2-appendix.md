# AI coding harnesses: Round 2 research, appendix

Companion to [`harnesses-round-2.md`](harnesses-round-2.md). Everything below is copied unchanged from the ten synthesis files in [`round2/synth/`](round2/synth/), grouped by the same ten feature groups. The recommendations in section C were written per group; section 10 of the report consolidates and ranks them.

## Contents

- A. Patterns and trade-offs, per feature group
- B. Documentation versus code: every Round 1 claim tested, per feature group
- C. For rizzy-harness: the full recommendations, per feature group
- D. Not settled, per feature group

## A. Patterns and trade-offs, per feature group

### The agent loop, tool dispatch and durability

From [round2/synth/01-loop-dispatch-durability.md](round2/synth/01-loop-dispatch-durability.md).

##### 4.1 Where authorisation sits in dispatch

| Approach | Projects | What it costs or fails to cover |
|---|---|---|
| One pipeline at the dispatch boundary: freeze args, policy waterfall, approval, guards, body; a throwing listener denies | DeepSeek harness (`pipeline.ts`), Pi (find, repair, validate, hook, execute), OpenClaw (`before_tool_call` chain fails closed), Oh My Pi (deny, handlers, re-resolve on the effective input) | DeepSeek validates arguments after policy (a policy sees raw strings); Pi lets a hook mutate args after validation with no re-check; DeepSeek's shipped waterfall has no asking listener (the ask lives inside bash, fs and plugin tools) |
| Authorisation inside each tool | OpenCode (`permission.assert` in each tool), Cline (each tool validates its own input after approval), Hermes (dangerous-command check inside the terminal tool) | A new or plugin tool that forgets the check runs unchecked; OpenCode's own note says "catalog visibility, not execution authorization" |
| Approval keyed to the call id, persisted, expiring with the process | goose (`goose.executable` flag in stored metadata; approvals expire with the extension lease) | Binds to the call id, not a hash of arguments; works because state is re-derived from stored messages |
| Approval bound to "whatever is pending" | OpenHands client (`{accept, reason}` only) | A stale or double click answers the wrong action (corrected in review: only client-side de-duplication) |

Common good rule: policy runs after the last mutation of the call. Oh My Pi re-resolves approval on the effective input after extension handlers run; Hermes' `modify` hook runs before the terminal tool's own check, so the rewritten command is what is checked.

##### 4.2 Parallelism

- **Per-tool flag to a shared lock** (Codex): reads share, everything else is exclusive; the flag is set per tool (shell is parallel-capable; `apply_patch` is not). Cost: the flag is easy to set wrongly for shell.
- **Fail-closed classifier per call** (DeepSeek `isConcurrencySafe(args)` exactly `true`, max 10; Oh My Pi unset is shared but a throwing resolver is exclusive). Cost: in DeepSeek a tool that does not say it is safe runs alone; in Oh My Pi an unset tool runs shared, so a forgotten declaration means parallel.
- **Serial preparation, parallel execution** (Pi, Cline, Hermes, OpenClaw): all approval prompts appear one at a time, then approved tools run together; Hermes also serialises prompts and excludes human-wait time from the batch deadline.
- **Unbounded fork per call** (OpenCode): each call starts while the model still streams, no cap.
- **Speculative pre-execution** (Oh My Pi): only auto-allowed `local_read` effects, and only when no extension lifecycle handler is loaded.
- **Sequential** (Rizzy on main; goose dispatches in order and merges result streams).

##### 4.3 Stuck loops

Five distinct designs exist, and the cost of each is visible in the code:

| Design | Projects | Cost |
|---|---|---|
| None | Codex, Pi, OpenCode (outside `/goal`), Oh My Pi (outside edits), Zed, Warp (client) | A looping model runs until the user aborts or the context fills. Codex and Pi say so (`agent-loop.ts`: "nothing in the loop counts calls or cost") |
| Hash of tool plus arguments, consecutive repeats, notice only | DeepSeek (3, 5, 8, never blocks), Cline (3rd call notice) | A looping call is not stopped; A, B, A, B is missed |
| Hash of outcome too, thresholds, block | OpenClaw (rolling, off by default; error breaker on) | Heuristic that can block good work, hence off; the always-on breaker covers only identical errors |
| Attended warns, unattended stops | Hermes (`hard_stop_enabled` forced on for gateway and cron) | Needs a notion of "attended" per run |
| Dead code | goose (`RepetitionInspector` on a clone, no limit, flag not passed) | The lesson is that the counter must live where it persists across steps: a fold over the log, not a local object |

Cline's detector runs from the `tool-started` event, so the repeated call has already begun when the stop fires; OpenClaw's runs at admission, before dispatch.

##### 4.4 Step and time limits

- **No cap, rely on the user** (Codex, Pi, DeepSeek, OpenClaw, Hermes, Cline by default): all state it, and Pi, DeepSeek, Codex notes call it a runaway risk.
- **Hard cap plus a canned stop message** (goose 1000; also a half-budget `<turn-budget>` hint to the model).
- **Cap that keeps the cache and is enforced** (OpenCode): keep tool schemas, send `toolChoice: none` with an instruction, refuse any late call at dispatch. Reviewed and corrected from Round 1 ("tools are removed").
- **Client-set cap** (OpenHands Canvas sends 500): the server enforces; the client can change it.
- **Idle rather than total timeouts** (OpenClaw 120 s model idle plus 48 h run; Hermes cron 600 s of inactivity; Codex 5 min stream idle; OpenCodex 300 s stall): these catch a hung provider without capping honest long work.
- **Timeouts as metadata nobody reads** (Cline `timeoutMs`): the dispatcher does not wrap the call.
- **Model-set timeouts**: goose lets the model set `timeout_secs` with 0 meaning none; Hermes promotes an over-limit foreground call to a tracked background job instead of refusing it (to avoid retry loops).

##### 4.5 Steering and queueing

| Approach | Projects | Cost |
|---|---|---|
| Reject while a turn runs | goose (`AgentRunExists`), Cline (`run()` throws), Pi core (`prompt()` throws, the session layer converts to steer or follow-up), Rizzy on main | No loss of accepted input, but the user waits; a UI must hold the text |
| Queue and deliver at a step boundary, never kill a tool | Codex (`turn/steer`), Pi, Oh My Pi, OpenClaw (default), DeepSeek (inbox claimed at pre-step), OpenCode | Coarse: in parallel batches steering waits for the whole batch (OpenClaw, Pi). Pi's repository doc says only "enters after the current assistant turn"; Round 1's "interrupts remaining tools" was wrong |
| Steer with fallback to follow-up, collect, interrupt as separate modes | OpenClaw (`steer` default, 500 ms debounce, cap 20, `summarize`, a durable "dropped" receipt per dropped input) | The most complete policy; also the most configuration |
| Interrupt the turn | OpenClaw `interrupt` (abort and wait for the writer to settle); Oh My Pi cooperative `steeringSignal` aborts only `interruptible` tools | Aborting a half-done edit loses work; omp and Pi deliberately avoid killing side-effecting tools |
| Durable queue with idempotent admission | Hermes (`session_admissions`, 32 per session, `queue`, `steer`, `redirect` intents), OpenClaw (`session_pending_inputs` with request hash), DeepSeek (client `requestId` in the logged message), Codex queue extension (SQLite, 100 per thread) | More schema; the payoff is a retried HTTP call is an exact replay and an accepted message survives a crash |
| In-memory queue | Pi, OpenClaw's in-flight steering list | Only protects against a failed run, not a process crash (corrected in review for OpenClaw) |

A reliability lesson from OpenHands: a message written to a socket that the server has not yet subscribed can be lost; the client falls back to HTTP, and holds messages in browser memory when the sandbox is paused. Acknowledging only after a durable append removes the window.

##### 4.6 Crash safety: four families

1. **Write intent first, replay by policy** (pi-durable): a checkpoint `{phase: execute, arguments, replay: safe|unsafe}` is committed before the tool runs; on resume only `safe` tools re-run; a restart counter per task, reset on progress (default limit 5). Cost: only experimental, only in a separate package, and every tool must declare a policy; built-in file and shell tools do not.
2. **Flush the log, then close the turn with "outcome unknown"** (DeepSeek, Rizzy on main): fail-closed flush before each model request and each top-level tool body; on resume synthetic error results ("outcome is unknown ... do not retry blindly") and `turn/end` with reason `interrupted`; tools are never re-run. Cost: up to 200 ms of events can be lost on a hard crash; a tool that ran and whose result was lost is reported as unknown.
3. **Re-drive the turn**: OpenCode re-drains with "continue without repeating completed work" (at-least-once, abort after more than 10 attempts); OpenClaw injects a fixed prompt to check state first; goose re-derives from the stored conversation and continues an unanswered approval. Cost: the model decides what is safe to repeat, and a started shell command may run again.
4. **Fence the claim** (Hermes epoch plus generation on every settle; OpenClaw `activeWriterRunId` checked in the write transaction; Codex and DeepSeek use a file lock and flock with no expiry; OpenCode relies on "the old owner is dead"; goose has none). Cost: OpenClaw's fence is skipped when a write carries no expected writer (corrected in review); the file locks assume one host.

Hermes alone separates an `unknown` execution from `interrupted`, and never requeues it. A person's unknown turn pauses the session until a decision; an unattended one settles itself so the queue does not stall.

##### 4.7 Output truncation: who enforces it

| Layer | Projects |
|---|---|
| Each tool (loop applies nothing) | Pi: an extension or MCP tool returning 10 MB goes straight into context |
| Tool plus recorder | Codex (tool yields and caps; recorder truncates old output at record time); goose (shell cap, plus a 200,000 character per block backstop in dispatch) |
| Dispatcher, proportional to context | OpenClaw (30% of window and 16k, 32k, 64k characters per result) |
| Executor cap plus request-build cap plus recoverable cache | Cline (48,000 at executor, 8,000 or 50,000 at request, `cline://cache` URI the model can page; stale re-reads rewritten) |
| Spill to a file or artifact with head and tail preview | Oh My Pi (50 KiB spill), DeepSeek (12,500 tokens, fails open), goose (slot files, can be overwritten), Pi bash (temp file) |

Collection must be bounded while reading: goose holds a gigabyte-printing command in memory before truncating; Codex keeps 1 MiB per process with head and tail; OpenClaw bounds sandbox bridges at 100 MB.

##### 4.8 What changes in unattended runs

- **Same engine, a client that cannot answer approvals** (Codex `exec`: approvals become errors; DeepSeek headless: `ask` with no answerer fails closed; Oh My Pi: fails closed only when a prompt would be needed).
- **Permissive defaults that make a headless run never prompt** (Oh My Pi `yolo`, Cline cron `yolo`, goose forced `Auto`).
- **Deny by default for unattended** (Hermes `cron_mode: deny`, a toolset denylist, loop hard-stop forced on).
- **Schedule semantics**: Hermes advances `next_run_at` before dispatch (at-most-once); OpenCode and DeepSeek skip or collapse missed slots; goose skips and lets runs overlap; OpenClaw retries an interrupted turn only if delivery had not started, with no attempt count (a repeat crash qualifies again).
- Warp reports every unattended refusal as `Denylisted`, hiding the actual reason from the model.

### Approvals and permission rules

From [round2/synth/02-approvals-permission-rules.md](round2/synth/02-approvals-permission-rules.md).

##### 4.1 Five shapes of policy

**1. No gate, or a gate that is open by default.**
- Pi has no gate at all. DeepSeek harness's pipeline reaches approval only when a plugin returns `ask`, and none does by default.
- OpenCode's base rule is `*:* allow`. Oh My Pi defaults to `yolo`, and so does an invalid value. goose defaults to `Auto`.
- Cline's CLI, Desktop and cron surfaces approve everything. OpenHands through Canvas sends `NeverConfirm`. OpenClaw's host exec ships `security=full, ask=off`.
- Cost: the approval code exists but is not the boundary. Whatever these projects do contain comes from a sandbox (DeepSeek's write-only kernel sandbox, OpenCodex's bubblewrap and capability set) or from nothing (Pi, OpenCode, Oh My Pi: no OS sandbox).
- Evidence: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/schema/src/agent.ts#L46-L51 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L97-L113 ; https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L1316-L1323 ; https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/tools/src/pipeline.ts#L197-L206 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-defaults.ts#L171-L188

**2. Per-tool-name levels.**
- Who: goose (`permission.yaml`), Cline (`{enabled, autoApprove}`), and our own `main` (`ask` covers mutating tools).
- What it is: simple and easy to show in a UI.
- Cost: it cannot tell `git status` from `git push --force`, so every "always" on the shell tool is total. goose's "always" on `shell` allows every command in every project: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/tool_approval.rs#L108-L126

**3. Pattern rules over the command.** Each project matches differently:

| Project | Pattern type | Precedence |
|---|---|---|
| Codex | argv-prefix Starlark | maximum decision |
| OpenCode | wildcard | last match wins |
| Zed | regex | deny > confirm > allow, with allow needing every sub-command |
| Oh My Pi | glob | deny per segment, allow on the whole command |
| Warp | anchored regex | denylist first |
| Hermes Agent | regex classes | hardline floor and `approvals.deny` first |
| OpenClaw | allow-list over resolved executable paths | — |

  The cost is always the same: the rule is only as good as the parser in front of it (4.2).
- Two precedence models exist:
  - **A lattice (Codex, Zed).** It is order-independent and easy to reason about: a deny anywhere wins.
  - **An ordered list (OpenCode).** A later, wider layer silently overrides an earlier one. OpenCode then needs a separate administrator pass to restore a ceiling: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/policy.ts#L98-L113

**4. A capability set fixed per session.**
- Who: OpenCodex (`accessMode`, session id and root id enforced on the executor per RPC), and OpenClaw's session modes, which intersect to the stricter.
- Cost: no per-command decision at all. This is the right shape for a ceiling, not for interactive approvals: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-rpc.ts#L298-L346

**5. Model-in-the-loop.** The model either reviews the call or rates its own call (4.4).

##### 4.2 Shell parsing: what each approach cannot see

| Approach | Who | What it cannot see |
|---|---|---|
| Real AST, allow-list of node kinds; anything else becomes one opaque argv | Codex (tree-sitter-bash), OpenClaw (tree-sitter WASM with size and time limits), Zed (brush-parser) | Wrappers (`bash -c`, `sudo`, `env`, `xargs`, `find -exec`) are not unwrapped by any of the three. Zed parses PowerShell and cmd as POSIX. Substitutions are denied (Zed) or prompt (OpenClaw). |
| Hand-written scanner | OpenCode (POSIX and PowerShell), Warp (`decompose_command`) | OpenCode ignores redirects and declarations as resources. Warp does not unwrap wrappers or expand `$VAR`. Both are large hand-written code paths with parts nobody read (OpenCode's 2,565-line scanner). |
| Regex over string variants | Hermes Agent (normalised, quote-masked, `-c` payloads, deobfuscated), goose, Cline plan-mode blacklist, Pi example gate | Anything the regex author did not think of. Cline's own comment admits `python -c` and `bash -c` pass: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/command-guard.ts#L10-L17 |
| No parsing | DeepSeek harness, OpenCodex (argv only), OpenHands client | Not applicable: they rely on a sandbox or on nothing. |

Two good asymmetries are worth keeping:

- **Allow must cover the whole command; deny may hit any segment.** Oh My Pi and Zed both do this:
  - https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L311-L321
  - https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tool_permissions.rs#L208-L260
- **Deny projects each segment to its executable and sees through wrappers.** Hermes Agent does this, and the review confirmed it: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_detection.py#L1623-L1664

On parse failure:

- **Fail closed (no auto-allow):** Codex (opaque argv, not split), OpenCode, OpenClaw and Zed.
- **Fail open:** Cline's plan-mode guard.
- **Only on size limits:** Hermes Agent.

##### 4.3 Binding: from "whatever is pending" to "this exact executable"

Ordered from weakest to strongest:

1. **Nothing.** OpenHands Canvas `{accept}` answers whatever the server holds pending: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/event-service/event-service.types.ts#L1-L4
2. **An opaque request or approval id, with any authenticated client allowed to answer.**
   - Cline: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/approval-handlers.ts#L123-L161
   - OpenCode: any password holder.
   - DeepSeek harness: `eventId`.
3. **A persisted call id, with duplicate and conflicting answers rejected.** goose's approval also dies with the extension lease, so a restart cannot run a stale one: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L999-L1039
4. **A tool name or a pattern class.**
   - goose "always", and our `main` "allow for this session".
   - Hermes Agent: the "recursive delete" class: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L474-L487
   - Zed: a prefix regex.
   - Oh My Pi: `subject\0tier` / `tier\0path`.
5. **Exact canonical argv, cwd and the requested permission delta.** Codex's session key: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/runtimes/unified_exec.rs#L87-L98
6. **Level 5, plus everything below.** OpenClaw adds:
   - the agent, the session and the node;
   - the requester device;
   - a hash of the environment overrides;
   - the executable's real path, with a SHA-256 when the file is mutable;
   - cwd by inode.

   It also re-checks for drift before launch and consumes the approval once:
   - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L72-L90
   - https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-invoke-system-run-approval.ts#L197-L252

   Even OpenClaw leaves a small window between hashing and exec [checked in review].

"Always" semantics follow the binding:

- **Permanent and wide:** Zed, Codex, Hermes Agent, goose and OpenCode. The grant goes to user-level config with no expiry, keyed on a prefix, a class or a name.
- **Narrow:** OpenClaw's "always" is the exact executable plus a hash of cwd and arguments.
- **No "always" at all:** DeepSeek harness, Cline and OpenHands.
- **Codex's side effect:** a remembered rule also takes the command out of the sandbox, the most surprising behaviour found: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L492-L512

##### 4.4 Model approvers and model self-assessment

Two different things share the name.

**Separate reviewer.** A second model call judges the proposed call:

| Project | Reviewer | On error | On timeout |
|---|---|---|---|
| Codex | Guardian | deny | human |
| OpenClaw | `auto` reviewer | ask | ask |
| Hermes Agent | smart approver | human | human |
| goose | SmartApprove | fails closed | — |
| goose | adversary | fails open | — |
| goose | injection classifier | fails open | — |
| DeepSeek harness | Auto review | deny | no timeout |

What separates the good designs:

- **Sees only what it needs.** It never sees tool output (DeepSeek harness, goose classifier). Codex gives it a compact transcript without reasoning.
- **Structured output.** Strict closed-key JSON (OpenClaw), or a forced tool call (goose).
- **Injection pre-filter.** A regex that denies text aimed at the reviewer before the model sees it (OpenClaw).
- **A reviewer approval covers only what was reviewed.** It never pre-approves an unsandboxed retry (Codex).
- **A denial circuit breaker.** Codex counts per turn; OpenClaw keeps a 256-session in-memory LRU. Both lose state on restart.

What goes wrong:

- **Failing open.** goose's adversary and classifier.
- **Trusting the repository.** Codex treats repository `AGENTS.md` as authorisation context.
- **Widening on disposal.** DeepSeek harness widens to full access when the plugin is removed.

**Self-assessment.** The acting model labels its own call, and the label lowers the gate:

- **Warp:** `is_risky == Some(false)` skips the allowlist and the redirection check: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L972-L1008
- **OpenHands:** `ConfirmRisky` keyed on an inline `security_risk` (likely, [unchecked]).
- **goose:** an MCP server's own `readOnlyHint` auto-allows the call.
- **DeepSeek harness:** `approvedBuilds` on `plugin_manager`.

A prompt-injected model sets whichever label gets it through. Cline removed its `requires_approval` field; only a legacy string remains: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/schemas.ts#L133-L157

A model-supplied *request* is a different and acceptable thing. Examples are Zed's `reason`, and Codex's and DeepSeek harness's `sandbox_permissions` plus justification. The request widens nothing until the normal check and a human approve it, once.

##### 4.5 Plan and read-only modes

- **Prompt only.** Codex (and an empty `explorer` role), Hermes Agent and DeepSeek harness.
- **Code-enforced for edits only.** OpenCode (shell, sub-agents and Code Mode stay open) and Oh My Pi (bash and eval stay open). Cline removes the edit tools but keeps a bypassable blacklist, and lets the CLI model end plan mode itself through `switch_to_act_mode` with an unchecked "user approved" claim: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/runtime/interactive/mode.ts#L31-L68
- **Enforced by capability.**
  - OpenHands: a separate planner conversation with three tools.
  - goose: Chat mode stubs every tool result, though the tools are still advertised.
  - OpenClaw: the `read-only` session mode.
  - OpenCodex: `accessMode: read-only` on the executor.
  - Pi's plan-mode example: a tool set plus a bash veto.
- Only capability enforcement survives a model that ignores the prompt.

##### 4.6 Sub-agents

| Approach | Who |
|---|---|
| Narrow-only, parent re-applied last | Codex (roles cannot override sandbox, approval policy, provider or MCP). Hermes Agent (tools are the intersection; dangerous prompts auto-deny). |
| Inherit the mode and forward the child's prompts to the parent's user | goose |
| Forced permissive, user deny rules kept | Oh My Pi (`yolo` overlay; prompts fail closed) |
| Approval pinned to `never` | DeepSeek harness (in-process children). OpenClaw (on a node, ask becomes deny). |
| Unchecked | Cline (no policies, no approver, unbounded nesting). OpenHands (no gate, no cap). DeepSeek harness external children and Warp's Claude and Codex children (launched with the foreign harness's safety off). |

##### 4.7 Hooks

| Fails | Who |
|---|---|
| Closed | OpenClaw (`before_tool_call`, any throw), Oh My Pi (throw, 30 s timeout, abort; policy re-resolved on the rewritten input), Pi (`tool_call` throw), DeepSeek harness (throwing listener) |
| Open | Codex, Cline (file hooks), OpenHands (load failure drops all hooks), goose (unless `on_failure: block`), Hermes Agent (`pre_tool_call` crash) |

Hermes Agent adds one good rule: a hook may escalate a call to the human but never approve it: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/plugins.py#L1198-L1227

Repository-shipped hooks load with no trust prompt in Oh My Pi, goose, Cline and OpenHands. Codex hashes the hook declaration but not the referenced repository script.

##### 4.8 Bypass paths that recur

- **Scripting tools whose inner calls skip the gate.** goose's Code Mode `tools.call` reaches `lease.call` directly: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L172-L194
  Others get it right. OpenCode's nested Code Mode tools enforce their own permissions. Pi's `runToolCall` is re-entrant.
  Hermes Agent's `execute_code` runs on the host kernel.
- **User-typed or template commands outside the gate.** Pi `!` (`user_bash`), and OpenCode `` !`cmd` `` templates.
- **A credential that can answer approvals sitting in the agent's reach.** OpenHands `OPENHANDS_AUTOMATION_API_KEY`, and the OpenCode server password ([inferred]).
- **Client- or caller-supplied policy.**
  - The OpenHands browser composes `confirmation_policy`.
  - Cline hub clients supply a session's policy.
  - OpenCode HTTP session rules are appended last.
  - The Codex exec-server uses the caller's sandbox.
  - The OpenClaw Gateway's approval overrides the node's allow-list.
  - Warp's `--sandboxed` is self-declared.
- **Unattended means "approve".**
  - Hermes Agent: `-z` and library context.
  - Cline: cron runs, plus any unknown mode, become yolo.
  - goose: scheduled jobs are forced to `Auto`.
  - Warp: run-to-completion drops the user denylist by default.

  The opposite, correct choice also exists:
  - Hermes Agent: cron, `-q`, API and webhook runs deny.
  - Warp: headless "would ask" becomes a denial.
  - Oh My Pi: headless runs throw.
  - OpenCode: headless `run` rejects.

### Sandboxing and where commands run

From [round2/synth/03-sandbox-containers-execution.md](round2/synth/03-sandbox-containers-execution.md).

**A. Namespace tool plus syscall filter (Codex).** bubblewrap builds the filesystem view and the
namespaces; seccomp after it closes sockets, `ptrace` and `io_uring`.

- Cost: a helper binary to locate and verify, a two-stage exec, and an architecture-specific
  filter (x86_64 and aarch64 only).
- What it still fails to cover: it is a denylist with default allow. `sendmsg` is left open
  (review). Deny globs are a snapshot taken at launch. Missing writable roots are skipped
  silently.
- It is the only design in the set that blocks UDP and Unix-socket connects when network is off.

**B. Namespace tool alone (Zed, OpenCodex, DeepSeek's bwrap rung).** The coverage depends
entirely on the flags.

- Zed's `--ro-bind / /` leaves every file readable.
- DeepSeek has no `--unshare-net`, so the network is open.
- OpenCodex mounts a minimal set and unshares the network, but lacks `--unshare-user`,
  `--cap-drop` and seccomp.
- Missing paths cannot be bound, so "protect `.git` before it exists" silently fails on Linux
  (Zed, inferred).

**C. Landlock self-restriction (DeepSeek's second rung; our PR 4).** It needs no helper binary
and no user namespace.

- DeepSeek grants read everywhere and sets no network or scope rights, so it confines writes
  only. It still reports `full` on ABI 6 and later kernels.
- Codex refuses Landlock because it cannot hide a same-uid control socket. That is the code's
  statement; the claim that Landlock cannot block `connect()` is the author's inference.
- Our module header (PR 4) notes that ABI 9 adds pathname Unix-socket connect control, so on new
  kernels that gap closes. That fact comes from our code, not from the input files.

**D. Seatbelt.**

- Deny by default with a Mach allow-list (Zed) versus allow by default with only writes denied
  (DeepSeek). The first confines; the second does not.
- `sandbox-exec` is deprecated by Apple, but it is the only per-command option in the set.

**E. Containers (OpenClaw, Hermes, Warp `sbx`, OpenHands upstream).** The host boundary is the
container.

- Strength depends on three things: who controls the flags and mounts, whether pass-through
  arguments exist, and whether the harness relaxes its own checks inside.
- OpenClaw is the most careful: validated binds, `--pull=never`, a recreate epoch, `--network
  none` by default.
- Hermes appends operator extras last and skips its safety floor.
- goose's `--container` does not cover the shell at all.
- OpenHands lets the client set limits and puts the control-plane key in the container.

**F. No sandbox, by policy (OpenCode, Oh My Pi, Pi, goose, Cline, Warp, Hermes local).** Pi and
Cline say outright that the sandbox is the user's job. Warp's `--sandboxed` and Cline's
`CLINE_SANDBOX` use the word for things that confine nothing.

**G. Escalation shapes.**

| Project | Shape | Costs and gaps |
|---|---|---|
| Codex | retry after a denial, with fresh approval | keyword denial detection on a path that can lead to unsandboxed execution; an `allow` rule removes the sandbox |
| DeepSeek harness | the model declares a wider mode plus a justification, one-shot | no proof of a prior denial |
| Zed | the model declares hosts, paths or `unsandboxed` plus a reason, with three grant lifetimes | a thread-lifetime unsandboxed grant spreads silently |

**H. Executor trust direction.**

| Executor | Trust direction | Weak point |
|---|---|---|
| Codex exec-server | trusts the caller's policy | no ceiling of its own |
| pi-env | trusts its stdin | no policy at all |
| OpenClaw node | decides | lets a Gateway approval override its allow-list |
| OpenCodex executor | validates everything itself and refuses `exec` without a sandbox | capabilities are self-declared at enrolment, with no operator decision |
| DeepSeek SSH helper | sandboxes with its own chain | self-reports its hash and its enforcement |

**I. Killing.**

- Process-group kill is universal and leaks `setsid` children: Cline and goose say so; Oh My Pi
  and Pi are inferred.
- Oh My Pi's re-walk before SIGKILL narrows the leak.
- A PID namespace (`--die-with-parent`, the container's init) or a cgroup closes it. Codex and
  Zed (via bwrap) and the container backends get this; the plain-spawn harnesses do not.

### Credentials, secrets and authentication

From [round2/synth/04-credentials-secrets-auth.md](round2/synth/04-credentials-secrets-auth.md).

**4.1 Where the key lives relative to the agent.** Four positions exist.

- *Key in the agent's process and environment* (OpenCode, Oh My Pi, Pi, goose, Cline, Codex by
  default, OpenClaw host exec). Cheapest; every shell command can read it. Every one of these
  also keeps the key in a 0600 file the same user's agent can open (OpenCode, Oh My Pi, Pi,
  Cline, DeepSeek), so scrubbing the environment alone does not hide it (DeepSeek finding 21).
- *Key in the harness process, environment scrubbed* (Hermes terminal and `execute_code`,
  DeepSeek, OpenClaw sandbox and SSH). Blocklists miss names they do not know (Hermes: "any user
  secret not on the list ... reaches local terminal children"; DeepSeek: `DATABASE_URL`,
  `GITHUB_PAT`). Allow-lists (Hermes `execute_code` and MCP, OpenClaw sandbox, OpenCodex bwrap
  `--clearenv`) do not have that failure.
- *Key held by a broker on the same machine, dummy or token in the child* (Codex credential
  broker, OpenClaw secret egress proxy, Hermes iron-proxy). Costs a MITM CA inside the sandbox
  (Codex, Hermes, OpenClaw all install one). Coverage is the weak point: only Codex pairs it with
  a network namespace that makes the proxy the only path; OpenClaw's and Hermes' are cooperative
  (a process that ignores `HTTPS_PROXY` goes direct), Hermes covers Docker only and its own model
  calls bypass it, and SigV4 or service-account credentials cannot be substituted (Hermes lists
  them as uncovered).
- *Key on the control side, worker gets none* (OpenClaw inference by model reference, Oh My Pi
  auth gateway, OpenCodex Home/Child relay, Warp server, OpenHands MARS bridge). The only
  position where a compromised worker cannot spend a key except through the control side's
  rules. Costs a data path on the control side that must be bounded (OpenCodex shows how much
  code that is), and a scope check that must run after routing (OpenCodex 14) and refuse
  worker-chosen profiles (OpenClaw refuses `@profile`).

A fifth, distinct case: *key sent per session to a worker that keeps it in memory* (Cline SSH
remote; OpenClaw GitHub token). Cline drops the refresh token but its lifetime on the remote was
not found; OpenClaw writes the GitHub token to a per-turn profile directory, readable by the agent.

**4.2 Control-plane credentials inside the agent's reach.** OpenHands puts the master session key
in the agent's environment on purpose and tells the model to use it; OpenCode leaves the server
password in the environment outside stdio mode; DeepSeek keeps the cookie-signing secret in a file
confined bash can read; Oh My Pi's broker token file is unprotected. In every case the credential
can answer the agent's own approvals or reach settings. The counter-examples are OpenCode's stdio
mode and Cline's core, which capture the spawn secret into memory and delete it from the
environment before any child starts, and Codex's always-strip list.

**4.3 Redaction.** Approaches: none (OpenCode, Pi, goose, Cline on model-bound text); regex
patterns (Codex, Warp, Hermes, Cline marketplace); by known value (OpenCodex logs, DeepSeek MCP
credential set, OpenClaw registering minted credentials and GitHub tokens); reversible placeholders
(Oh My Pi). Regex misses unprefixed values; value-based needs the full set and should fail closed
when it cannot enumerate it (OpenCodex). Reversible placeholders hide secrets from the provider
but deobfuscate into tool arguments, so they are not a leak control. Ordering matters: Hermes
redacts after an output plugin hook, which sees raw output.

**4.4 Worker credentials.** Range from none (pi-env, trusts SSH), to a shared static bearer
(Oh My Pi broker), to a hashed device bearer with no expiry and latest-connection-wins fencing
(OpenCodex), to OpenClaw's per-dispatch, domain-separated, claim-bound hash with a 10-minute TTL,
per-frame revalidation and a global never-reused epoch. Codex checks bearer credentials once at
upgrade and says so; OpenClaw re-checks on every frame. OpenClaw stores device tokens in plaintext
even though it hashes worker credentials.

**4.5 Local server authentication.** Approaches, weakest first:

- No auth on loopback (Codex app-server and exec-server defaults, OpenCodex data plane, Oh My Pi
  `--no-auth`, goose `--dangerously-unauthenticated`, OpenHands MARS without a key, Pi and Oh My
  Pi RPC on stdio by construction). Any local process, including the agent's own shell with network
  on, can drive it.
- `Origin` as authentication (Cline hub). `Origin` is a client-supplied header.
- One shared secret with no identity (OpenCode, goose, Hermes API server, OpenHands, OpenCodex data
  keys), sometimes in a URL (`?auth_token=`, `?token=`, `?roomSecret=`, `#token=`, VS Code token).
- Launch token then a signed cookie (DeepSeek, Pi `apps/p2`); revocation only by rotating the
  signing secret.
- Short single-use tickets bound to purpose and capability over a same-user socket (Hermes).
- Device keys signing a challenge plus the token and scopes (OpenClaw).

Refusing a wider bind without auth is common (Codex app-server, Hermes dashboard with no override
flag, OpenCodex, DeepSeek refusing wildcard binds, Cline dashboard); the Codex exec-server and
goose show the gap when one listener of several is left out.

**4.6 Pairing codes.** OpenCode: 128-bit code, 5 minutes, atomic consume, but the resulting token
is derived from the server password and cannot be revoked alone. OpenCodex: 60-bit code, 10
minutes, hash stored, rate-limited by source, `Origin` refused, grant deleted before metadata
validation, device bearer hashed. OpenClaw: setup tokens plaintext with 10-minute TTL, plus
SSH-verified auto-approval that rests on network position and a shell profile (inferred). goose:
no code, a 128-bit fingerprint for out-of-band comparison and a trust book.

**4.7 Trust gates.** Hard-coded open (Oh My Pi), absent (OpenCode, goose, Cline, OpenHands
hooks), path boolean without content binding (Pi), per-layer with hashes the repository cannot
write (Codex, but script bytes and skills uncovered), "project servers never auto-start" (Warp).
None binds trust to the content of everything that will execute.

### Links, workers and client protocols

From [round2/synth/05-links-workers-protocols.md](round2/synth/05-links-workers-protocols.md).

##### 4.1 Where the loop sits relative to the tools

- **Loop on the controlling side, tools remote**: Codex exec-server, OpenCode workspace
  drivers, Pi pi-env, DeepSeek SSH, Hermes SSH/Modal/Daytona, OpenCodex Remote Workspace,
  Cline client-contributed executors, and Warp (loop on the vendor's server, tools on the
  client). The executor is a "dumb" tool host, so the *policy* travels with each request
  (Codex `sandbox` intent, OpenCodex capability set chosen at session creation). The cost is
  that the executor either trusts the request (Codex, pi-env, Warp's client trusts the
  server) or has to re-implement path and identity checks itself (OpenCodex's executor does,
  thoroughly). Every one of these needs a round trip per tool call, and a disconnect loses
  every in-flight call (pi-env `lost: true`, DeepSeek "loss is final", OpenCodex sessions die
  with the connection).
- **Loop and tools on the executing side**: OpenClaw cloud workers (with inference brokered
  back to the Gateway), Cline SSH remote (a whole hub on the host), OpenCode SSH (a whole
  server on the host), goose roaming (the host owns everything), OpenHands Agent Server, and
  rizzy-harness. The controlling side then needs fencing and a commit protocol for what the
  executor writes back; only OpenClaw built one.
- **Single process**: Oh My Pi, Pi, DeepSeek's web profile, Hermes' gateway with viewers.
  No link to fence; one-host locks suffice.

##### 4.2 Who dials

- **Executor dials out** (no listener on the executor): Codex exec-server `--remote`,
  Codex remote control, OpenClaw workers and nodes, Hermes relay, OpenCodex executors,
  Oh My Pi collab, DeepSeek LiveShare. All of them need reconnect semantics; only OpenClaw
  fences a reconnect with a generation and a per-request re-check. OpenCodex and Codex fence
  by "one connection at a time" (latest wins, or first wins with a retention window).
- **Controller dials over SSH**: OpenCode, Cline, Pi, DeepSeek, Hermes, Codex `program`,
  and OpenClaw provisioning. The executor's lifetime is tied to the SSH pipe (pi-env kills its
  process groups on EOF or 30 s silence; DeepSeek's helper holds a 30 s lease), which fences
  stale executors for free but makes the session die with the network. The flag sets differ a
  lot: pi-env's (`ClearAllForwardings`, `SendEnv=-*`, `RemoteCommand=none`, private
  known-hosts, explicit `scanHostKey`) and OpenClaw's (`-F none`, `IdentityAgent=none`) are
  strict; Hermes and Oh My Pi use `accept-new`. Several SSH designs then move a secret back
  over the channel: OpenCode reads the remote password from `service.json`, Cline reads the
  hub token from remote stdout.

##### 4.3 Fencing mechanisms

| Approach | Projects | What it covers | What it misses |
|---|---|---|---|
| None, assume the old owner is dead | OpenCode restart sweep, goose | single process restarts | two processes on one database; any network split |
| Pipe or lease lifetime | pi-env, DeepSeek SSH helper, OpenCode stdio child, Codex `exit-on-stdin-close` | a dead controller cannot leave a live executor | the session cannot survive a disconnect |
| OS lock with no expiry | DeepSeek `flock`, Codex thread lock, Oh My Pi lease | two processes on one filesystem | anything across hosts; network filesystems |
| One attachment plus retention | Codex exec-server (80 s) | a second client | a stale client that still holds the socket, up to the detection time |
| Latest connection wins | OpenCodex Remote Workspace; our master (`nodes.rs:208-212` per the design note) | a reconnect replaces a dead socket | frames already read from the old socket; a stolen key flapping |
| Monotonic epoch, re-checked per request, plus writer CAS inside the write | OpenClaw | stale worker after re-assignment, rotation, destroy; duplicate commits (ledger) | unfenced writes that carry no expected writer |
| Epoch plus per-session generation, checked at settle | Hermes | a restart's lost turns; a stale settle | it is one host; it fences settlement, not a remote writer |

The only design that fences a remote executor's writes is OpenClaw's, and it relies on three
pieces together: a global, never-reused epoch; a per-dispatch credential whose hash is bound
to the epoch and turn claim; and an authority check inside the same database transaction as
the write.

##### 4.4 Per-dispatch credentials and where model keys live

- **Keys stay on the controller; the executor holds a proxy credential**: OpenClaw
  (`worker.inference.start` by model reference), OpenCodex Remote Link (link token; the
  relay strips caller credentials), Oh My Pi auth gateway (scoped client digests). Cost: model
  traffic goes through the controller.
- **Keys go to the executor per session**: Cline SSH (API key or access token in every
  `session.create`), OpenHands (`LookupSecret` resolved at spawn), and our design note's
  `session.grant`. Cost: the executor's operator can read them; removal cannot un-send them.
- **Keys go to the controller from the client**: Warp (BYO keys in every request to Warp's
  server).
- **Credential distributors**: Oh My Pi's auth broker hands clients API keys and access
  tokens.
- **Leaks that defeat the model**: OpenClaw's worker receives a GitHub token that its agent
  can read; OpenHands puts the stack's session key in the agent's environment; OpenCode leaves
  its server password in the shell environment outside stdio mode. In each case the agent can
  reach the control plane.

##### 4.5 Resume and replay

- **Cursor over a durable log**: Cline `sinceSequence` (global sequence, 7-day replay aid),
  OpenHands `after_seq` (per conversation, gap-tolerant because the server skips unreadable
  events), OpenCode's log API (exclusive cursor, subscribe first, watermark, `log.synced`
  marker; unused by its own clients), Warp `since=<seq>` with the cursor stored server-side,
  Oh My Pi `get_entries {since}`, Codex remote-control subscribe cursor and exec-server
  `process/read` cursor.
- **Snapshot per reconnect**: DeepSeek (`follow` yields a fresh snapshot; client repairs
  gaps by paging), OpenCode's clients (refetch over REST). Cost: a full read per reconnect and
  a race the client must hide.
- **Replay the conversation**: goose `session/load`.
- **Acknowledge after durable commit**: Hermes (`inbound_ack` after admission), OpenClaw (ACK
  cursor after the authority check, cached terminal replies).
- **Idempotency keys**: DeepSeek (`requestId` stored in the logged message), OpenClaw
  (pending inputs unique on idempotency key with a request hash; commit idempotency keys from
  session, epoch, seq, index), Hermes (admission unique key with payload digest).
- **What is never replayed**: a tool call whose outcome is unknown (DeepSeek closers, Pi
  durable `replay: "unsafe"`, OpenClaw `canContinue = false`, Hermes `unknown`, pi-env `lost`),
  a send that may have started (OpenClaw delivery queue, Hermes ledger with a visible duplicate
  marker), and progress frames (OpenHands).

##### 4.6 Durable versus transient frames

OpenHands is the only protocol that names the distinction on the wire: `durable` frames carry
a `seq` and are replayed; `transient`, `item_started`, `delta` and `item_aborted` are never
persisted or replayed, and deltas are keyed by the id the durable event will carry. DeepSeek
gets the same effect with durable events plus assistant frames that carry a dense `revision`.
OpenClaw separates `worker.transcript.commit` (sequenced, ledgered) from `worker.live-event`.
OpenCode's live bus is volatile by contract and its durable log is optional. Cline's log is a
replay aid behind canonical message files.

##### 4.7 Back-pressure

Codex rejects *requests* with `-32001` when its 128-entry inbound queue is full but never drops
responses or notifications (they wait). OpenCodex makes the SSE bridge a pull stream so a slow
client stops the upstream read, and sheds body admission with 503 plus `Retry-After`. OpenCode
gives each subscriber a 4,096-entry queue and fails that subscriber on overflow. OpenClaw caps
the worker socket at 16 queued frames and 32 MiB. Oh My Pi sheds a lagging collab guest. Pi's
RPC awaits stdout back-pressure. DeepSeek's per-follower `Deque` is unbounded: a stalled client
grows host memory.

##### 4.8 What the executing side can refuse

From weakest to strongest: nothing (Codex exec-server, pi-env, Zed's `terminal/create`, Warp's
`fs/read_text_file`, Cline's hub against a client's `toolPolicies`, goose against a roaming
peer's config methods); trusts a report (DeepSeek's SSH sandbox `enforcement`); re-decides but
yields to a forwarded approval (OpenClaw node: only local `security=deny` holds); refuses by its
own rules and intersects what is requested (OpenCodex executor: path, symlink, hard-link and
root-identity checks, CAS writes, no `exec` without a sandbox, capability intersection at the
handshake).

##### 4.9 Listener authentication patterns

- **Refuse a wider bind without auth**: Codex app-server, OpenCode (refuses without a password
  at all), Hermes dashboard (`--insecure` ignored), OpenCodex data plane, goose (refuses without
  a secret unless a "dangerously" flag). DeepSeek goes further and refuses wildcard binds.
- **Where the credential travels**: HTTP upgrade header (Codex), first WebSocket message
  (OpenHands), WebSocket subprotocol (Hermes tickets, Cline hub token), cookie after a one-time
  code (OpenCode, DeepSeek, Pi p2), and the query string (goose `?token=`, OpenCode
  `?auth_token=`, DeepSeek launch URL, Cline dashboard, Oh My Pi browser relay).
- **Browser defences**: Codex rejects any `Origin`; DeepSeek checks Host, then
  `Sec-Fetch-Site`, then Origin; OpenCodex's pairing and agent routes refuse `Origin`;
  Cline *accepts* a loopback `Origin` as a credential, which is the anti-pattern.

##### 4.10 ACP as agent and as client

As **agent**: goose (its whole UI runs over ACP), Oh My Pi (`omp acp`, with `bash` forced to
ask under a permission-capable client), Hermes (a thin viewer that never runs tools), Cline's
ACP mode (auto-approve off unless explicit). As **client**: Zed (serves fs and terminal; applies
path checks to fs, none to terminal, and does not apply its own tool permissions), Warp (fs
only, auto-allows everything except denylist and protected writes, prefers `AllowAlways`),
goose providers (inherits the whole environment; answers prompts itself in `auto`), DeepSeek's
`subagent-acp` (empty client capabilities, no MCP servers, rejects permission requests by
default), and the OpenHands Agent Server (not in the repository). The pattern across clients:
ACP's permission request is advisory, the agent describes its own action, and most clients
answer mechanically. Only DeepSeek's client starts from "grant nothing".

### Session storage, resume, fork and checkpoints

From [round2/synth/06-sessions-fork-checkpoints.md](round2/synth/06-sessions-fork-checkpoints.md).

**4.1 Where the truth lives.**
1. *An append-only log is the truth and the UI folds it.* DeepSeek harness (contiguous `seq`, flush barriers), Codex (JSONL with ordinals), Oh My Pi and Pi (JSONL tree), OpenClaw (`transcript_events`). Costs: unbounded growth (only OpenClaw bounds disk by default); Pi and Oh My Pi rebuild a path from the whole entry list; DeepSeek loads the whole log into memory on resume.
2. *Mutable rows are the truth.* goose, Hermes, Cline, OpenCode (projections). goose has no sequence and works around it with a timestamp clamp; Hermes needs revision triggers and repair paths; Cline rewrites the whole messages file and can truncate it on a crash.
3. *Both, with the log optional.* OpenCode: projections are authoritative, the event table is empty in the shipped CLI, so "event-sourced" is true only for embedders. A destructive revert (rows deleted) then has no recovery source.

**4.2 Durability and crash classification.**
- Write-behind plus barriers (DeepSeek): loses at most about 200 ms and never performs an effect before its request event is durable (checked). Pi appends per finished message without fsync; a crash loses the streaming message and can leave an unanswered tool call. Codex flushes per line and drops transient events by policy.
- Intent before effect with a replay policy (pi-durable): the `execute` phase is committed first; tools not marked safe are not re-run and the model is told "may have partially run".
- Claim before turn (OpenCode, Hermes): a durable "turn started" record plus an attempt counter or epoch lets a restart classify a turn as lost. Then the policies differ: OpenCode re-prompts with a fixed message (at-least-once, 10 attempts); Hermes never requeues and needs a person (or the unattended path) to settle it; OpenClaw sends a fixed "check state first" prompt; DeepSeek closes the turn with synthetic results; goose re-validates a pending approval and answers with a synthetic error if the extension lease is gone.

**4.3 Fork: five representations.**
1. *Pointer over an immutable file* (Codex paginated mode; pi-durable): cheap, shares storage; needs the parent to stay immutable and present. Codex pins the parent only during prepare, so a later deletion relies on a fallback path that was not traced. Pi's `parentSession` is a bare file path and orphans on rename.
2. *Copy into a new physical log with an inherited marker* (DeepSeek): self-contained, the parent can be deleted or exported alone; cost proportional to the prefix. `session/end-seed` and `inheritedEventCount` let the fold, duplicate-prompt detection and counters tell inherited from new history.
3. *Copy rows into a new session* (OpenCode, goose, Hermes, OpenClaw, Cline): the same trade as 2. goose keeps the same message ids in parent and child (not unique); OpenCode regenerates ids as `<forkevent>_<seq>`. OpenClaw refuses forks above a model-dependent token ceiling; Hermes requires an idle parent; goose's truncate-by-timestamp does not check for an active run (unchecked).
4. *In-file branch by moving a leaf* (Oh My Pi `/tree`, Pi `/tree`): zero copy, siblings stay reachable, needs a parent pointer per entry. Failure: the active leaf is derived from the last physical entry, so a leaf move with no following write is lost on restart (Pi: inferred, reviewer agreed; Oh My Pi: inferred, with an `activeLeafId` patch for off-branch writes). Both also keep a copy-based operation (`/fork`, `/branch`) for a new file.
5. *No fork, only rewind:* Codex `thread/revert` swaps the file; Hermes `/undo` soft-deletes rows with a compare-and-swap on expected ids; OpenCode revert commit deletes rows.

No fork copies working-tree state, with partial exceptions: Cline couples a message fork with a workspace restore from the checkpoint ref, Warp's handoff uploads a diff snapshot while forking the server conversation, and DeepSeek attaches the child to a forked workspace "if the parent had one". Forking inside an open turn: DeepSeek closes it with `forked` closers; OpenClaw excludes the active tail; OpenCode skips unfinished rows; Hermes requires idle; Pi and Oh My Pi refuse or queue while busy.

**4.4 Recording the active branch.** Inferred from "last entry" (Pi; Oh My Pi with an `activeLeafId` exception); a pointer swap in an index row with compare-and-set (Codex); a per-row flag (`active` in Hermes, `agent_visible` in goose); implicit (linear logs: DeepSeek, OpenClaw windows). Only Codex records the active history explicitly and atomically. Nobody writes a "head moved" event.

**4.5 Checkpoint families.**
1. *Stash commits in the user's repository under a private ref* (Cline). Cheap (blobs dedupe; stat cache from a persistent private index); no second repository. Costs: pollutes the user's object database, refs live until the session is deleted, no size or count cap, git-only, uses the user's git config except two flags.
2. *Shadow repository outside the user's tree* (OpenCode per-project store with alternates; Hermes one shared bare repo; DeepSeek's display-only variant; Rizzy). Does not touch the user's refs or config; covers shell-made changes because it diffs the tree; costs one scan of the tree per capture. OpenCode's mitigations lower constants only. Hermes bounds cost by skipping the snapshot above 50,000 files, which turns a big repository into "no undo" with only a debug log line.
3. *Temp-index git commits without a retained scratch repo* (Zed archive checkpoint): used for archive only; `git add --all` per call; no timeout.
4. *Write ledger or action log built from the agent's own tool calls* (Hermes ledger for restore filtering; Zed action log; Warp handoff touched paths; DeepSeek whole-file captures for non-git paths). Cost is independent of repository size; coverage stops at what the tools report. Hermes' ledger made the default restore skip exactly the files a shell command changed. Zed's rewind rejects every unreviewed edit, and its log is lost on restart.
5. *Per-mutation before/after manifests with content-addressed blobs* (Hermes skill ledger): precise, restorable per entry, fails closed; scoped to one file tree, not a working repository.
6. *No mechanism:* Codex (removed), goose, Oh My Pi (conversation rewind only; its tool text claims git), Pi (example only), DeepSeek (display only). Zed removed per-prompt checkpoints, and the surviving recipe has the O(tree) shape the shadow repositories have.

**4.6 What a checkpoint misses.**
- Ignored files: OpenCode, Cline, Hermes (default excludes), Warp handoff (non-ignored only). Large files: OpenCode untracked over 2 MiB, Hermes over 10 MB (restore keeps the file), Warp over 25 MiB marked skipped, Cline unbounded (the diff skips blobs over 50 MB).
- Nested repositories: Hermes refuses a whole-tree rollback; OpenClaw refuses to snapshot; Cline relies on git's gitlink behaviour.
- Anything outside the working tree: installed packages, databases, files outside the repository (Cline, stated). Container writes: Hermes excludes container backends entirely (no snapshot, no ledger entry); the input shows no other project handling container-made writes explicitly.
- Shell-made changes: covered by tree diff (OpenCode, Cline, DeepSeek display, Rizzy); only by command-text regex (Hermes); not at all (Zed; Warp beyond written paths; Pi's stash covers tracked files only).

**4.7 Restore safeguards seen.** Snapshot the present before destroying it (Cline transaction; Hermes pre-rollback; OpenCode `stage` keeps the original for `clear`); refuse when HEAD moved plus `update-ref` compare-and-swap (Cline); refuse while a turn runs (OpenCode busy; Hermes idle); compare-and-set on the visible history (Hermes `expected_active_ids`; Codex path swap); restore only attributed paths (OpenCode); skip user edits by comparing to a recorded hash (Hermes, with the shell gap); refuse nested repositories (Hermes); keep files no snapshot holds (Hermes); strict ref-name regex (OpenClaw); fail closed when the safety capture fails (Hermes ledger, Cline `AggregateError`); roll back the started session if the new session fails (Cline).

**4.8 Fencing a stale writer.** Advisory file lock (Codex, Oh My Pi, DeepSeek; single host only); claim with compare-and-swap inside the write transaction (OpenClaw; skipped when a write carries no fence); epoch plus generation checked on every settle (Hermes, OpenClaw workers); nothing beyond process luck (OpenCode, goose, Pi).

### Context: prompts, compaction, instruction files, memory

From [round2/synth/07-context-compaction-memory.md](round2/synth/07-context-compaction-memory.md).

##### 4.1 Keeping dynamic state out of the cached prompt

Four approaches exist.
1. **Volatile text below a boundary or in tiers.** OpenClaw (`SYSTEM_PROMPT_CACHE_BOUNDARY`, prefix cached by hash), Hermes (stable, context and volatile tiers, rebuilt only at compaction), Oh My Pi (one trailing `<project-context>` block). Cheap and simple. It does not tell the model that something changed mid-session, and a changed fact either waits for the next rebuild (Hermes: a new skill or memory write reaches the prompt at the next compaction or session) or breaks the prefix.
2. **A per-step or per-turn tail message.** goose (`<turn-context>`, persisted so the turn reuses identical bytes, skipped under a 32,000-token limit), DeepSeek harness (`runtime-context` user message appended only when the rendered text differs), Cline (mode tag on each user message). The prefix never moves. Each approach needs a rule for what happens to old snapshots at compaction: goose carries the latest forward; DeepSeek's findings do not say.
3. **Keyed sources with a baseline and diffs written into the history.** OpenCode (hash delta and already-rendered text frozen in a durable event; first observation rendered into `system[1]`; later changes become system messages; baseline re-based at compaction), Codex (typed sections with persisted snapshots and JSON merge-patch; fresh windows render everything), Pi (system-prompt sections patched by system messages appended to the transcript; tool additions and removals likewise). This is the only approach that makes "the model has seen X" a recorded fact, and the rendered text is stored so replay does not re-render. Cost: more code and special cases (Codex has legacy-fragment matchers; OpenCode refuses to start a session whose unavailable source was never initialised).
4. **Prompt text as data per model.** Codex (catalog row), OpenCode (overlay files chosen by model-id substring, no provider check, so a gateway-served model with "gpt" in its id gets the GPT prompt), Hermes (guidance gated on model-name substrings), Cline (one template; only the tool list differs per model). Pi and goose have no per-model prompt branching.

A related risk: Hermes and Oh My Pi note that tool descriptions can change the tool bytes, so Hermes' review fork inherits the parent's tool list and restricts only at dispatch. Codex rebuilds the tool list every step from (model row, flags, environment).

##### 4.2 Compaction trigger

Three formulas. A fraction of the window: Codex 0.9, goose 0.8 capped at 225,000, Zed 0.9 of capacity, DeepSeek 0.8 with a 65,536-token headroom floor, Hermes 0.50 raised to 0.75 for windows under 512K (a hard cap at 85%). Window minus a reserve: OpenCode (15% or 16k), Pi (16,384), Oh My Pi (15% or 16,384), OpenClaw (reserve of at least 20k, also capped at 75%). A fraction of the input limit: Cline (0.9, which is about 0.81 of the window when only the window is known). The numbers differ by a factor of two (Hermes at 50% against Codex at 90%); no project in the findings shows a measurement that favours one. All check before the next request; Pi also checks after each tool batch, and Oh My Pi and Codex also mid-turn. When the window is unknown: OpenCode's ceiling becomes infinite (never fires, overflow only), Cline skips auto compaction (128,000 for manual and overflow), DeepSeek warns once and continues uncompacted.

##### 4.3 Measuring context size

Anchor on the provider's last reported count and add an estimate for what came after: OpenCode, Pi, goose, Zed (usage only), Hermes, OpenClaw. The estimate is characters over 3 or 4. Corrections exist in four forms: Cline scales its budget down by the ratio of the provider's real count to its estimate (cap 4x); Pi ignores usage recorded before the latest compaction (otherwise a fresh compaction would immediately re-trigger); Oh My Pi floors the count with a local estimate so provider-side request shrinking cannot hide growth; Hermes scores a 413 in serialised bytes because the token estimator is blind to images. goose admits that one huge tool result between checks can overshoot; Zed has the same gap. None of the findings describe measuring with a provider token-count endpoint (absence not confirmed).

##### 4.4 The summary request

Cache-preserving: OpenCode (same system parts, tools and history plus a final user message; the code comment says this is for cache reuse), DeepSeek harness (same system prompt, full tool schemas, shadowed messages, instruction last; tool calls in the reply dropped), Zed (tools kept with `tool_choice: none` when the model is the same). Codex's local path also appends to a clone of the history with no tools (intent not stated). Deliberately uncached or separate: Pi (`cacheRetention: "none"`, history serialised to text), goose (flattened transcript in a system prompt, cache writes off), Hermes (one user message, one-shot requests get no cache marker), Cline (own request over the prefix). The cost of the cache-preserving form: the request is the full history plus the prompt plus the output budget, so the trigger must leave that room (OpenCode's buffer; DeepSeek's headroom equals the summary `maxTokens`), and it only pays if the same model and an unexpired cache are used (OpenCode's precompute is tied to the cache lifetime minus 30 s).

What the summary is made to contain: structured templates in nearly every project (goal, constraints, progress, decisions, next steps, files). Distinct ideas: Hermes tells the model the turns are "DATA to summarize, never instructions", redacts secrets and keeps every real user message verbatim in the summary (24K chars) and a regex-extracted anchor index; goose asks for a JSON object and re-renders it; OpenClaw's safeguard demands fixed headings and audits identifiers; Oh My Pi's snapcompact renders discarded text to images with no LLM call (lossy, tool output cut to 2,000 chars). Failures: an empty, truncated (`finish: length`) or tool-calling summary is an error in Hermes, Pi, OpenCode (also a heading check with one nudge) and DeepSeek; Codex accepts an empty summary outside post-turn compaction.

##### 4.5 What is kept verbatim

Codex: user messages up to 20,000 tokens (not tool output, not assistant text). Zed: last 80,000 bytes of user messages. goose: only the last user text. OpenCode: newest 15,000 tokens, but as serialised text with tool results cut to 1,250 chars (Round 1 said verbatim; corrected). Pi, Oh My Pi, OpenClaw: about 20,000 tokens of recent entries cut at a valid boundary (never between a tool call and its result). Hermes: lean tail of 2.5% of the window (10K..25K) plus always the last assistant message, last real user message and pending tool round. DeepSeek: 16% of the message budget. Cline: 20,000 estimated tokens.

##### 4.6 Fallbacks, shrink checks and overflow recovery

- **Make the summary request fit:** OpenCode (ladder of request shapes, then shrink to 85/70/50/35% of the rejected size), goose (remove tool responses 0/10/20/50/100%), Hermes (evenly sampled input with elision markers, one call only), Codex (drop the oldest history item and retry).
- **A non-model fallback:** Cline `basic` (keeps every typed user prompt, collapses dropped tool work into a notice, keeps the last three assistant texts), Hermes deterministic summary (user asks, actions, files, errors), Oh My Pi `shake` and `soft` (mechanics not described), DeepSeek zero-retention compaction after a model-free prune. Not present: Pi, goose, Zed, OpenCode (it fails instead), Codex (the turn dies).
- **A shrink check:** DeepSeek (summary must be strictly smaller than the span), Cline (retry refused unless the request shrank), Oh My Pi (falls through when a summary would not reduce context), Hermes (breaker after two ineffective compactions), OpenHands UI (waits for a token drop). Not present: Codex, OpenCode, Pi, goose, Zed.
- **The recovery loop:** compact and retry the same step once (OpenCode, Pi, Cline); up to three (goose, OpenClaw, Hermes); drop the failed turn first (Pi `context_edit`, Oh My Pi); mark usage full and end the turn (Codex sampling path); only reroute (OpenCodex combo failover, which does not shrink anything); nothing (Zed). Distinct signals handled by Hermes: 413 body-size versus "prompt too long" versus "output cap too large", each with its own action, and adopting a provider-reported limit but never a guessed one.
- **Abort and preserve:** Hermes aborts compression on terminal network, truncation, empty-content and auth failures rather than replacing the middle with a placeholder; Cline ignores a malformed sidecar; Pi writes nothing on failure.

##### 4.7 Tool-output pruning

Three families. Nothing between compactions, only a cut when the result is produced: Codex, Pi, Zed, OpenCode (V2). A cut at production with the full text kept where the model can fetch it: OpenCode (disk for 7 days), Hermes (spill with a 1,500-char preview, aggregate 200,000 chars per turn), Cline (`cline://cache` URI, 16 MiB), DeepSeek (spill store; fails open to inline), goose (200,000-char temp file). A pass that shrinks old results later: DeepSeek (model-free prune over 8,192 chars, run first under pressure), Hermes (at compaction), Oh My Pi (`supersedeReads` and `dropUseless` every turn, waiting for a cold cache), OpenClaw (cache-TTL soft trim then hard clear, frozen and recorded so a restart rebuilds the same prefix; off except for Anthropic), Cline (stale re-reads rewritten, batched at 64 KB), goose (LLM one-line tool-pair summaries, off by default). The cache-aware ones (Oh My Pi, OpenClaw, Cline) exist because rewriting old bytes breaks the prefix cache.

##### 4.8 Recording compaction in the log

- **A new entry on a branch of a tree; nothing deleted:** Pi (`compaction` entry with `firstKeptEntryId`, `tokensBefore`, snapshot of the system prompt and tool state), Oh My Pi (also `method`, `preserveData`).
- **A complete replacement base in an append-only file:** Codex (`replacement_history` item; resume replays only the suffix).
- **Events around a replace operation:** DeepSeek (`compaction/start`, `summary`, replacing `user/message` citing shadowed seqs, `compaction/end`; a start with no end blocks further compaction until a later `session/end-seed`; if the summary committed but the close failed the surface is replaced while the lock stays open).
- **A flag flip on existing rows:** goose (`agent_visible`), Hermes (`active=0, compacted=1`, still searchable, same session id; a config comment names a "session-rotation bug cluster" as the reason it left the child-session design).
- **A pointer message plus an epoch:** OpenCode (`keepID`, instruction baselines re-based).
- **A sidecar:** Cline (`<id>.compaction.json` with a hash of the source prefix; stale or malformed sidecars are ignored).
- **A condensation event:** OpenHands (`forgotten_event_ids`, `summary`, `summary_offset`).

##### 4.9 Instruction files

Discovery: global then root-to-cwd (Codex, goose, DeepSeek, Hermes for the `AGENTS.md` chain); global then every ancestor to the filesystem root (Pi, no repository boundary); nearest-wins per depth with a priority table across harness formats (Oh My Pi); fixed workspace file set (OpenClaw); first matching type wins (Hermes). Delivery: system prompt (Pi, goose, OpenClaw, Hermes), user-role message (Codex, DeepSeek), keyed instruction source with diffs (OpenCode). Nested files after a `read`: OpenCode (ledger is synthetic messages in history, so they are re-injected after compaction), DeepSeek (on `read`, `write`, `edit`). Budgets: Codex 32 KiB combined; OpenClaw 20,000 per file and 60,000 total with a digest of must/never lines; Hermes 20,000 or 6% of window; DeepSeek 64 KiB per chain; none in Pi, OpenCode, goose (main file), Cline, Oh My Pi. Trust: only Codex (untrusted project returns nothing) and Hermes (regex scan) gate project instruction files; Pi gates `.pi/` resources but not context files; goose, Cline, Oh My Pi, DeepSeek and OpenHands have no gate for the content they read. After compaction: OpenCode re-bases; Codex fresh windows render everything; DeepSeek's compaction path does not re-inject (nested files re-project when touched); goose subdirectory hints drop out until the model touches those paths again (inferred).

##### 4.10 Memory

- **Chat model writes directly:** goose (prompt-only "confirm with the user"), OpenClaw (workspace files via ordinary tools plus a flush turn before compaction), Hermes (`memory` tool, 2,200 and 1,375 chars).
- **Background model writes:** Codex (two phases, consolidation agent in a locked profile, no approval), OpenClaw dreaming (score threshold, 25% removal guard), Oh My Pi (Hindsight, Mnemopi, Sharpshooter), Hermes background review.
- **A person approves:** Hermes only, and off by default (`memory.write_approval`, `skills.write_approval`).
- **A ledger and rollback:** Hermes only (skills); OpenClaw Skill Workshop is "versioned and undoable" (not traced).
- **No memory:** OpenCode, Pi, Cline, DeepSeek (absence confirmed for DeepSeek only).

Session search exists in Hermes (FTS5, no LLM, compaction-archived rows still found), OpenClaw (hybrid vector and keyword over memory files), DeepSeek (opt-in, in-memory in the web profile).

### Edit tools and the mechanics behind the scenes

From [round2/synth/08-edit-tools-mechanics.md](round2/synth/08-edit-tools-mechanics.md).

##### 4.1 Edit formats

1. **Exact string replace, must match once** (Claude-style): Cline `editor`, goose, DeepSeek, OpenCode (plus invisible-character tiers), Pi and OpenClaw (batched, plus a fold). Cheap and safe; the failure mode is a loud "not found". It costs a retry and, with no hint, a re-read (Pi, goose and DeepSeek differ in how much they return on a miss; OpenCode and OpenClaw return nearby lines).
2. **Patch envelope** (`*** Begin Patch`): Codex, OpenCode `patch`, Cline `apply_patch`, OpenClaw. Multi-file with move and delete in one call. Two of three read implementations route it by model family (OpenCode by `gpt-` in the id; Cline by GPT/Codex), so it is a model-fit feature, not a better format. Codex's freeform transport needs provider support for custom tools.
3. **Hash-anchored line ranges**: Oh My Pi only. The saving is that the model does not retype old text. The cost is a per-file tag, a retained snapshot store, a seen-line guard and (optionally) 565 lines of rebase code plus a tree-sitter dependency. Its own authors downgrade it for eight model families, which is the strongest evidence that it is not universally better. The Round 1 benchmark claims were not tested.
4. **Streamed fuzzy buffer edit with review**: Zed. The edit is applied to the live buffer as the model streams, so the user sees it; the cost is that "reject" is a write back to disk, because the file is saved when the tool finishes.
5. **Whole-file write with a content-hash precondition**: OpenCodex `write_file` (`expectedSha256`), Hermes `write_file` (read-state refusal). The strongest staleness rule found, for the most destructive edit.

##### 4.2 Match ladders: what each rung costs

- Tiers that change only invisible characters (trailing whitespace, curly quotes, dashes, NBSP) are used by Codex, OpenCode, Pi, OpenClaw, Hermes and Oh My Pi. OpenCode's mapping is one character to one character, so match offsets stay valid. Nobody reports a corruption bug from this class in the findings.
- **Pi's and OpenClaw's NFKC fold** can change string length. Pi works in normalised space and widens each replacement to whole lines, which **silently rewrites touched lines to their normalised form** (checked in review). OpenClaw splices into the original bytes and errors on an ambiguous Unicode boundary (unchecked). Same fold, different damage.
- **Indentation-insensitive rungs** (`trim` in Codex tier 3, OpenCode `patch`, Hermes `indentation_flexible`, Oh My Pi 0.98) match a block at the wrong nesting level; OpenCode's `edit` author deliberately leaves the rung out. Hermes compensates by re-indenting the replacement to the matched region.
- **Similarity rungs** (Zed 0.8, Cline 0.66, Hermes 0.5 to 0.8, Oh My Pi 0.95) accept a near-duplicate block as the unique candidate. Controls that exist: Hermes refuses `replace_all` after a similarity hit; Oh My Pi demands a 0.08 margin over the second candidate; Cline reports the fuzz level; Zed shows the diff live. Codex has no uniqueness requirement at any tier, so its first-match-wins rule applies with no margin at all.
- **Tier order matters**: Codex's review correction shows an exact match later in the file beats an earlier fuzzy one, because the search is tier by tier, not position by position.
- **Failure hints**: OpenCode (line vote, gated on read permission), OpenClaw (three candidates at 0.45 plus current contents), goose ("Did you mean" plus 20-line preview), Hermes (third-miss escalation, gutter strip), Codex (expected lines verbatim). Pi and DeepSeek return only an error string.

##### 4.3 Staleness and read-before-edit: four postures

| Posture | Who | What it costs or misses |
|---|---|---|
| **Absent**: rely on the old text still matching exactly once | OpenCode, Pi, OpenClaw (verifies bytes after writing), goose, Cline, Codex, DeepSeek in shipped config | Safe when the match is unique; unsafe when `oldString` matches a different, newly introduced location (OpenCode finding). No protection against edits built from a stale mental model that happen to match |
| **Soft**: warn, still apply | Hermes `patch` (mtime, `_warning`), Zed (sentence appended to a *failure* message only) | The warning is advisory; Hermes' mtime test misses a same-mtime change |
| **Hard, tag or hash**: refuse on mismatch | Oh My Pi hashline (with a rebase escape), OpenCodex CAS on `write_file`, Hermes `write_file`, DeepSeek policy plugin (compare-and-swap inside the lock) | Needs a per-session read record. Oh My Pi's and DeepSeek's records are in memory, so they vanish on restart. DeepSeek's guard **ships disabled** and the prompt still tells the model to read first |
| **Prompt only** | Oh My Pi `replace` ("errors on edit before read"; no code), Zed and DeepSeek tool descriptions, goose instructions | Nothing in code enforces it (`absence confirmed` for Oh My Pi; searched `edit/`, `pi-edit/src`, `write.ts`) |

Where each guard ships off or unenforced: DeepSeek `fs-observation-policy` (`disabled: true`, no bundle re-enables it), DeepSeek `apply_patch` (off), Oh My Pi `bashInterceptor` and `gitGuard` (off), Hermes checkpoints (off), OpenClaw rolling loop detection (off), goose repetition inspector (inert), Cline `.clineignore` (not enforced by any running path).

##### 4.4 Atomicity of multi-file patches

| Approach | Who | Residual |
|---|---|---|
| Match all in memory; write only if every hunk anchored; then write files one by one | Cline, OpenCode `patch`, Codex (re-parses and re-matches at apply time) | A failure after the first write leaves a partial result. Codex reports an `AppliedPatchDelta`; OpenCode returns "Completed before failure"; none rolls back (OpenCode's source lists multi-file rollback as a TODO) |
| Stage all sections in memory before any write | Oh My Pi hashline | Commit atomicity across files is not stated |
| Per-file atomic write under a per-target lock | DeepSeek (atomic write), OpenCodex (temp file, rename, serial chain) | Single-file only |
| Apply what matches, reject the rest | Oh My Pi `sloppy` | Partial by design |
| Verify after write | OpenClaw (re-reads bytes) | Confirms the bytes, does not give atomicity |
| Approval not bound to applied bytes | Codex | The approved diff and applied bytes can differ if the file changes between verification and write |

No implementation read does a rollback across files.

##### 4.5 Post-edit steps

- **Formatter**: OpenCode (extension-matched, first success wins, failure ignored; the returned diff is against the formatted file); Zed (format-on-save setting).
- **Lint delta**: Hermes only, new errors versus pre-edit, by line-set difference (a renumbering linter gives false "new" errors). Round 1 documents an LSP layer; the Round 2 author did not read it.
- **Parse check**: Oh My Pi refuses a *shifted* stale remap that breaks a file that parsed (not offset-0 recovery).
- **Hooks**: goose `AfterFileEdit`; Codex `PostToolUse` (not read).
- **None found**: Codex, Pi, goose, Cline, DeepSeek (negative searches, unchecked). Oh My Pi's diagnostics were not traced.

##### 4.6 Tracking edits for review

Three designs: (a) **tool-reported edit ledger with a per-file baseline** (Zed; Hermes' write ledger as a restore filter): cost proportional to files touched, no repository scan, blind to shell-made changes; (b) **git tree snapshots around a step or turn** (OpenCode, Cline, DeepSeek, our checkpoint): catches shell effects, costs O(tree) per snapshot unless seeded (OpenCode) or bounded (Hermes 50,000-file skip); (c) **none** (Codex, goose, Pi). Zed **deleted** design (b) per prompt; the code does not say why, and the large-repository cost reason is `inferred`. Round 1 documents the release note.

##### 4.7 Stream rules (TTSR) and loop guards

Only Oh My Pi watches the live token stream and aborts. Its termination bound is "fire once per session by default"; it has no retry counter, runs author-supplied JavaScript regexes on the event loop, and pins fire-once state but not rule text through compaction (the text part `inferred`). Nothing else in the Round 2 files aborts a stream on content. Other mechanisms at the same layer:

- **Refuse tool calls from a length-truncated message**: Pi (checked). OpenCode instead nudges and retries, failing after three consecutive `length` stops.
- **Identical-call detection**: OpenClaw (default-on identical-error breaker at 3; rolling detector off by default), Hermes (warn attended, halt unattended), DeepSeek (reminder at 3/5/8, never blocks), Oh My Pi (three identical no-op edits), Cline (guards on failed turns and identical calls), goose (inert).
- **Tool-call argument repair**: OpenCode, Pi, Cline (lenient inputs).

##### 4.8 Shell: what survives

- **Kill scope**: whole process group (goose, Cline, Pi, our `main`, OpenCode); tree walk with a re-walk before SIGKILL (Oh My Pi); Windows Job Object created suspended (goose); `bwrap --die-with-parent` (OpenCodex). Everyone who uses `setsid` or group kill is escaped by a child that calls `setsid` (goose, Cline; Oh My Pi `inferred`).
- **After a normal exit**: goose deliberately leaves backgrounded children alive; Oh My Pi keeps a shell with live jobs past session teardown; Cline's detached command outlives the run.
- **Agent crash**: goose's shell gets no `PR_SET_PDEATHSIG` (its MCP servers do); Pi's SIGKILL leaves children; Hermes hands the group to an owner-death supervisor.
- **A model-controlled timeout of 0 means "no deadline"** in goose and Oh My Pi. Pi has no default at all.
- **Unbounded collection**: goose reads output into an unbounded `Vec` before truncating (unchecked).
- **Promote instead of refuse**: Hermes turns an over-limit foreground timeout into a tracked background job.

### Sub-agents, orchestration, model roles, fallback and retry

From [round2/synth/09-subagents-models-retry.md](round2/synth/09-subagents-models-retry.md).

##### 4.1 How a child is created and scheduled

1. **A child session record, run by something other than the tool call.** goose (`delegate` writes a `SubAgent` session; a pipeline step runs it), OpenCode (child session with `parentID`, job), OpenClaw (child session plus gateway run), and rizzy-harness `main` (`delegate` starts a child session with `parent_id`). Gains: the child's events live in the same log shape, restart can re-derive what is pending (goose records "delivered" notes), the child can be resumed by id (OpenCode, goose). Costs: needs a scheduler and a delivery rule; goose's step blocks the parent's pipeline until children finish, so it is not fire-and-forget.
2. **An in-process task or thread.** Codex (child thread plus status streams), Hermes (thread pool), Cline, DeepSeek, OMP. Simple and fast; the cost is that cancellation, cleanup and caps must be written carefully (Codex's `PendingSpawn` guard; OMP's worker threads; Cline has none of these).
3. **A spawned operating-system process.** Pi's example (`pi --mode json -p --no-session`), Warp's hidden-pane CLI children. Strong separation, but nothing is inherited unless passed; Pi's example passes the parent's whole environment and has no depth limit.
4. **Another vendor's agent.** goose ACP providers, DeepSeek (Codex over `app-server`, Claude Code over the Agent SDK, generic ACP), Warp (PTY CLI or ACP), OpenHands (Agent Server spawns the CLI), Zed (ACP client). Cost: the vendor tool runs its own tools; the host controls only the permission reply and the working directory (see 4.3).
5. **Client-driven spawning over a server bus.** Warp, OpenHands (browser executes the launch). The authority sits with the server, not the node; the opposite of rizzy-harness's invariant that the node decides.

##### 4.2 Context and delegation prompt

- **Fresh context with an explicit brief** is the norm: Codex (default), OpenCode, OpenClaw (`isolated`), Hermes, OMP, goose, Cline, OpenHands, DeepSeek spawn. **Fork** is an explicit opt-in that copies the parent's history: Codex `fork_context` (forbids a role override), OpenClaw `fork` (same agent only), DeepSeek `subagent_fork` (log prefix to the last `turn/end`, no model arguments, to keep the cache). It costs tokens and buys a cache hit.
- **The tool description is the delegation prompt.** Codex puts the spawn gate in the description (spawn only if the user or `AGENTS.md` asks; disjoint write sets; call `wait_agent` sparingly) and the reviewer confirmed it is advice, not a check. OpenCode's GPT prompt says the same. DeepSeek's prompt section says start independent delegations together, end the turn rather than poll, collect before answering. OpenHands tells the model the child cannot see history and that siblings need independent scope. Hermes tells the parent that child summaries are "SELF-REPORTS, not verified facts".
- **Which instruction files reach the child differs and is not consistent:** OpenClaw gives `AGENTS.md` only; OMP excludes files named `AGENTS.md` but passes `CLAUDE.md`; Hermes passes project context files but sets `skip_memory`; OpenCode runs the normal instruction machinery. The reviewer noted OMP's exclusion by file name is inconsistent.

##### 4.3 Permission relation to the parent

| Approach | Projects | What it costs or fails to cover |
|---|---|---|
| Inherit, narrow only, re-apply the parent's policy last | Codex (a role is a patch type whose fields cannot express widening) | Needs a typed patch; the explorer role is empty, so "read-only" is only a label |
| Agent rules plus the parent session's rules (session last) | OpenCode | A child agent definition can still be broader than the parent agent (`general` allows edits even when the parent is `plan`) |
| Tools are an intersection with the parent's; blocked set subtracted again | Hermes (5 tools blocked), OpenClaw (inherited allow and deny envelopes; hard lists for hidden children and leaves) | Says nothing about approval mode (OpenClaw findings do not describe it) |
| Pin approval to "never" and write the policy into the child's log | DeepSeek (in-process) | A background child cannot ask; with a parent at `danger-full-access` the child is a full-access child |
| Inherit the mode and forward the child's approval prompts to the parent's user | goose | A hung child holds the parent until the user answers or cancels; the docs' "autonomous mode only" is not enforced |
| Force the child to the most permissive mode | OMP (`yolo`, with `tools.approval` denies still applying and prompts failing closed) | The parent's `task` call is the only boundary; a per-spawn `overrides` spread after the yolo key can change it |
| No policy at all for the child | Cline (`spawn_agent` approval covers everything), Warp CLI children (`--dangerously-skip-permissions`, `--dangerously-bypass-approvals-and-sandbox`, trust pre-written), OpenHands (policy from global settings) | The approval of "delegate" becomes approval of everything the child does |
| External child, fixed permission config unrelated to the parent | DeepSeek external providers | A read-only parent can delegate to a Codex child configured to bypass approvals and sandbox |

External agents over ACP add a second problem: the permission request is advisory. Zed documents that deny rules take precedence over external agents but a reviewer confirmed the code does not read `tool_permissions` for them. Warp auto-answers requests (only denylisted commands and protected writes are denied; `AllowAlways` is preferred). goose maps its modes to each adapter's modes with gaps (Amp in Chat gets `default`; Pi gets nothing). DeepSeek answers every approval request with decline or empty.

##### 4.4 Isolation

- **None** (shared directory): Codex, OpenCode, goose, Cline, DeepSeek, Pi. DeepSeek's prompt warns that concurrent writes are not conflict-checked.
- **Git worktree:** Hermes (opt-in, off by default, degrades silently), OpenHands (default `worktree`, silent fallback to `shared` with a note to the agent), OpenClaw (a worktree handed to session creation; snapshot refs; GC keeps when unsure), rizzy-harness `main` (writer children get a worktree seeded with the working state, changes applied back as a patch, conflicts reported).
- **Overlay or clone plus patch-back:** OMP (backend chosen from ovnfs or platform clone; delta captured as a patch and applied through a temporary index: plain, then `--3way`, then replaying parent work-in-progress; failure throws).
- All are filesystem isolation in one process; OMP says so explicitly ("not a security boundary").

##### 4.5 Result return

- **Last text only:** Codex (status carries the last assistant message), OpenCode (`<subagent>` wrapper, last 20 messages searched), Pi example, Cline. Loses intermediate findings unless the child wrote them into its last message.
- **An explicit finish tool with a schema:** goose (`final_output`, default `{summary}`), OMP (`yield`), OpenClaw (`structured_output`, one retry), DeepSeek (`outputSchema`). Predictable size and shape; needs a failure state for a child that never calls it (goose: `Failed` with the last text).
- **A capped, flagged summary:** Hermes (24,000 chars, head 75% and tail 25%, remainder spilled to a file, with `status`, `exit_reason`, `truncated`).
- **Delivery timing:** queue-only note read at the next turn boundary (Codex mailbox with `trigger_turn = false`), a notice that wakes an idle parent or steers into a busy one (DeepSeek, OpenCode background, OpenClaw `announce`), a blocking wait (Codex `wait_agent`, OpenCode foreground, goose step).
- **Safety of delivery:** OpenClaw wraps the result as data, uses an idempotency key, never replays an ambiguous steer, and never steers a private completion into a public turn; goose strips a forged child marker from external MCP output and checks ownership.

##### 4.6 Caps

- Depth 1 is the common default (Codex, OpenCode, goose, DeepSeek, Hermes); OMP 2; OpenClaw 5. Concurrency defaults: goose 5, Codex 6 (V1) or 3 (V2), OpenClaw 5 per spawner plus lane width 8, DeepSeek 8, Hermes 10, OMP 32, Cline's team 2.
- **Missing caps are the failure mode:** Cline and OpenHands (client) have no depth or count cap; Pi's example has no depth limit; goose and Cline have no wall clock; Codex has no per-child time or token budget.
- **Three separate limits** (OpenClaw: depth, active children per parent, concurrent runs per parent) with error text that says which one tripped (also Codex, DeepSeek).
- The cap can be held in the wrong place: DeepSeek's workflow cap of 1,000 is enforced in the guest process, with the host's only limit being the pool of 8.
- Child step and time bounds: OpenCode `steps` (with `tool_choice: none` and the tool schemas kept for the cache); goose turns 1,000; Hermes iterations 250 plus heartbeat staleness (450 s idle, 1,200 s in one tool); OMP soft request budget 200.

##### 4.7 Teams and buses

- DeepSeek: a team is a task board (compare-and-set on `revision`, dependency validation, advisory `write_scopes`, 256 tasks) plus ordinary sub-agents and `send_message`; off per session by default. `write_scopes` are advisory.
- Cline: SQLite `teams.db` mailbox, 2 concurrent runs by default.
- OpenClaw: sessions may message each other; visibility defaults to `all` and agent-to-agent to enabled (opposite of rizzy-harness's private chats).
- Warp: server-hosted REST mailbox plus SSE with a monotonic sequence and a server-side cursor; peer messages are injected into a Claude child labelled "user-level".

##### 4.8 Model resolution and fallback

| Approach | Projects | Notes |
|---|---|---|
| One model per session; no fallback | Codex, goose, Cline, DeepSeek, OpenCode, Zed | Retry only; errors end the turn. goose and Codex say so in code or docs |
| A fallback chain on the agent | OpenClaw (requested, fallbacks, primary; explicit empty list means strict), OMP (per-role, exact-model, wildcard and role keys), rizzy-harness `main` (per-agent ordered fallbacks, only before output) | OMP's invariant: never replay a turn that already ran a tool |
| A router function | Pi virtual models (reason, previous model, failed attempt, state) | Fallback is an extension point; no default |
| A gateway with named combos | OpenCodex (strategy, eligibility, cooldown, metered consent) | A proxy design; the same logic could live on a node |
| Auxiliary slots with their own chains | Hermes (19 slots) | The most complete slot model in the input |
| Roles that grow | OMP (11 chat roles plus 5 model-kind roles, plus custom names) | The hard-coded per-role model name lists age quickly |

Cost of a hard-coded model list: OpenCode picks the title model from the families `gpt-luna`, `gemini-flash-lite`, `gemini-flash`, `claude-haiku`; OMP's priority chains name specific model ids; OpenHands embeds a benchmark table in a classifier prompt. The reviewers did not contest these; the author's comments call them brittle.

##### 4.9 Retry, `Retry-After`, cooldowns and billing

- **Classification.** Codex has one function from error to policy (never: auth, invalid request, model-not-found, context overflow, usage limits; server-advice-only; retry-with-backoff). Hermes puts hints on a typed `FailoverReason`. OpenCode has `isRetryable` plus an `x-should-retry` header override. Pi decides on the error text with a roughly 100-pattern regex, and a separate quota regex wins first. OpenCodex has three functions: may another target be tried, should this target cool, and with what scope.
- **Where `Retry-After` is live or dormant:** Codex (wake instant, cannot extend count), OpenCode (capped 15 minutes), DeepSeek (longer than max delay: give up), OMP (above 300 s: fail fast), goose (used as given), Hermes (capped), Zed (honoured), OpenCodex (cooldown duration, cap one day). **Dormant:** Pi's provider layer (default 0 retries, so never read); Pi's agent layer ignores it; Cline does not handle it in the findings.
- **Jitter:** present in Codex, OpenCode, OMP, DeepSeek, goose, Hermes; absent in Pi (a comment claims jitter, the code has none) and Cline.
- **Safe-to-retry rule:** retry only when nothing irreversible happened: Cline (no retry once text or a tool call streamed, nor after provider-executed tool activity), OMP (no replay-unsafe output unless tool calls provably never ran), OpenCode (before output re-runs the same message; after output only interrupted streams, with a fresh message), OpenCodex (byte-bounded preflight buffer that commits on the first non-control event).
- **Billing:** Hermes treats 402 as non-retryable after rotation and eager fallback ("retrying only burns paid requests on a depleted balance"); OpenClaw disables the credential with a window doubling from 10 minutes to 24 hours; OpenCodex hops on 402 and cools the provider for 10 minutes; Pi and OpenCode classify billing and quota as non-retryable.
- **Cooldown state:** persisted in OpenClaw (SQLite auth-profile store) and OpenCodex (`combo-cooldowns.json`, absolute epochs, merge keeps the later expiry); in memory for OpenClaw's sticky fallback and probe throttle; Hermes pool persisted in `auth.json`.
- **Per-request accounting:** OpenCodex charges retries to the logical request and to a workflow root so a fan-out cannot multiply upstream sends; Warp's counter is per request, reset on each tool-result round trip.

### Extensibility and unattended operation

From [round2/synth/10-extensibility-unattended.md](round2/synth/10-extensibility-unattended.md).

##### 4.1 Environment of a stdio MCP server

Three approaches exist. **Allow-list**: Hermes starts a server with eight named variables, `XDG_*` and the server's own configured env; DeepSeek drops credential-shaped names and stale internal ones. **Inherit and filter what the config adds**: goose adds the config's `envs` to the whole environment and refuses 31 dangerous names (`LD_PRELOAD`, `NODE_OPTIONS`, `PYTHONPATH`) in the config; the inherited provider keys still reach the child. **Full inheritance**: Oh My Pi, Pi (whose library has an `inheritEnv === false` switch the coding agent never sets), Cline and Warp, and Zed for external ACP agents. Their own reviews state the consequence: a provider key in an environment variable reaches every MCP server. The allow-list costs a support burden (a server that needs `AWS_*` must be told so explicitly); the Hermes passthrough guard (`tools/env_passthrough.py`) shows the second half of that design: a name added by configuration or by a skill must not be able to register a managed credential, and the check must fail closed.

##### 4.2 Who may start a server or load code

No project makes the node's local policy the only place that can add a stdio server. The nearest approaches: Warp (project-scoped MCP files never auto-spawn; global files do), Pi (a path-keyed trust prompt, whose default can be set only in global settings), Codex (project trust disables the project layer; trust state for hooks is read only from the user layer). Opposite: Oh My Pi (`enableProjectConfig` default true, `isProjectTrusted()` hard-coded true), goose (plugins auto-enable on discovery), Cline (workspace hooks and plugins, no trust code anywhere), OpenCode (in-process plugins from `.opencode`, no trust prompt), and DeepSeek (any holder of the owner cookie can add an MCP stdio server through the settings RPC, so the cookie is a code-execution credential). The deeplink allow-list in goose is a prefix match and only in the desktop installer; Cline's enterprise allow-list compares a name the user chooses. An allow-list that names a command string, not an executable identity, is not a policy.

##### 4.3 Trusting what a server says about itself

goose lets `readOnlyHint` from the server's own `list_tools` response grant auto-approval in SmartApprove; a hostile server can mislabel a destructive tool. Oh My Pi ignores annotations for approval (every MCP tool is `write` tier, which auto-approves in `write` mode, so it is lenient in a different way). Hermes treats a cached row with missing annotations as write-capable and runs a description scan whose result is thrown away (cosmetic). Warp's analogous mistake is the model, not the server: `is_risky` and `is_read_only` come from the model and the client trusts them. The consistent lesson: a hint may tighten a decision, never loosen it.

##### 4.4 Keeping tool lists out of the context

Seven designs. (a) **Search tool, flag per model** (Codex): small local BM25 index, no extra model call, but no automatic threshold and models without the flag get everything. (b) **Search tool, always on** (OpenClaw, Hermes): Hermes needed an explicit fix so that parallel admission and approval look through the `tool_call` wrapper to the real tool. (c) **Per-server exposure property** (Pi): one registry field; `deferred` loads by search, `codemode` by script. (d) **Code mode as the default surface** (OpenCode; Codex by catalog row): a 2,000-token catalog plus `search()` in-script bounds the cost for any number of servers, at the price of a language the model must write. OpenCode's own findings note a simpler deferral gets most of the benefit. (e) **Names only plus an enable call** (goose): the name list is uncapped. (f) **Unmounting behind `xd://`** (Oh My Pi): there is no search tool; it relies on `read` and `write` tiers carrying tool calls. (g) **Nothing** (DeepSeek, Cline, Warp): the full schema cost every step.

How the change is recorded divides into three: **an event or transcript entry** (Pi: system-message delta with `toolsAdded`; OpenCode: durable event with frozen text; Codex: persisted world-state item); **derived from the conversation** (goose: a fold over `enable_tools` results; no side table, so resume works); **stateless rebuild** (Hermes: "a session-keyed one drifts and silently drops tools"). Pi's deferred activation is the only one whose findings state that fork and `/tree` restore the loaded set per branch. Cache-stability tricks that recur: sorted names (omp, goose), frozen rendered text (OpenCode), updates appended not edited (OpenCode, Pi), `list_changed` counted in transport order (Codex).

##### 4.5 Code mode: isolation and approvals

Isolation is language-level everywhere it exists: own interpreter in the server process (OpenCode), QuickJS-wasm in a thread (Pi), Boa (goose), a `vm` context inside a sandboxed process (DeepSeek). Pi's own finding says it: "isolation of JavaScript capability, not of authority": whatever tools the script holds run with full host authority. Hermes is the weakest: a Python kernel on the host with only an env allow-list. On approvals there are three outcomes: **re-entrant pipeline** (Pi, OpenCode: every inner call is a normal tool call with hooks and its own permission assert, and a blocked inner call rejects inside the script), **bypass** (goose: inner calls reach `lease.call` with no inspector or hook, so one approval for `execute_javascript` covers every shell and MCP call it makes), and **whole-script approval** (Hermes, on gateway and ask surfaces only). Two further holes: OpenCode's denying `execute` makes the tools direct instead of removing them, and its guest `fetch` has no loopback or private-range guard and no response cap; Pi lets the script set its own wall-clock deadline (default infinite).

##### 4.6 Skills

All projects that load skills use the same shape: a short catalog in context, the body fetched on demand. What differs is the budget (OpenClaw 150 skills and 18,000 characters with graceful degradation; Hermes 60 characters per description; OpenCode and Pi none), the gate for repository skills (Pi: path-keyed trust; Codex and OpenCode: none, so a hostile repository's skill text is prompt injection in the first request), and who may write skills. OpenClaw's Skill Workshop is on by default and writes with no human step (versioned and undoable after the fact); Hermes confines the writer fork at dispatch time and supports staged approval, but the replay has no base-hash check, so a stale approval can overwrite a newer full rewrite. Nothing in the skills code itself executes at load time in any project; the executable parts nearby are OpenCode's `` !`cmd` `` command templates (outside the permission flow, and run on user-typed arguments as well) and the passthrough env names Hermes had to guard.

##### 4.7 Plugin and hook trust

Five stances: **none** (Oh My Pi, OpenCode, goose, Cline, OpenHands); **path boolean** (Pi: a later `git pull` that adds `.pi/extensions/` to a trusted folder loads without a prompt); **content hash of the declaration** (Codex: strongest, with trust state stored where the repository cannot write, a plugin-tree hash re-verified at each run, and the weakness that a repo hook's referenced script is not hashed and that remote workspace plugins are auto-trusted); **install-time scanner** (Hermes: regex patterns, evidence for review, not a boundary); **home-only discovery** (Cline's Agent Plugins lane, Warp's project-scoped servers). The word "sandbox" is misused: Cline's `CLINE_PLUGIN_MODE=sandbox` is a child process with a copy of the environment and no restrictions. In-process plugin code (OpenCode, Pi, Oh My Pi, Cline worker mode, Hermes default) has the server's rights and credentials; Hermes' `isolation: host` (a separate plugin-host process per profile that refuses other in-process user-module paths) is the only mode found whose stated purpose is to keep third-party code out of the main process.

##### 4.8 Hook failure and what hooks may do

Fail-open is the majority (Codex, Hermes plugin hooks, goose by default, Cline, OpenHands at load). Fail-closed: OpenClaw, Oh My Pi, Pi's `tool_call`. Codex's choice means a crashing, slow or malformed policy hook silently stops enforcing; goose's silent drop of a rule whose matcher regex is invalid is the same failure by another route. On capability: Hermes lets a hook only escalate to the human gate or block; OpenCode lets `permission.evaluate` turn ask into allow; Oh My Pi re-resolves the full approval gate against the rewritten input so a hook cannot rewrite a call past a prompt; Pi lets a handler mutate arguments with no re-validation. Timeouts that kill only the direct child (goose, Cline) leave grandchildren running.

##### 4.9 Unattended runs

Three authorisation models and a bad default. **Deny by default with an explicit allow set**: OpenCode's `Unattended` ruleset (every resource of a request must be allowed) and Hermes' `cron_mode: deny`. **Grant bound to the thing authorised**: OpenClaw's standing grant (job, config revision, operation) consumed at the spawn boundary, falling back to a prompt. **Approve everything**: Cline (cron, unknown modes become `yolo`), goose (`Auto` whatever the user chose), Oh My Pi (default mode `yolo` in headless), Warp (ACP allows all but a denylist; foreign CLIs launched with their own checks disabled). Warp's variant reports every denial to the model as "denylisted", hiding the real reason, and its `is_risky` flag is model-supplied.

Run semantics differ: at-most-once by advancing the schedule before dispatch (Hermes), latest-only catch-up with a slot held until idle (DeepSeek), skip missed slots (OpenCode), no catch-up and overlap allowed (goose), recovery with conditions but no attempt counter (OpenClaw). Journals: Hermes fsyncs a per-fire journal before submitting and pauses the job when the outcome is unknown; DeepSeek commits a receipt only after a durable flush and matches the outstanding delivery by message id on restart (a crash in between can deliver twice); OpenClaw's delivery queue refuses blind replay of a send that may have started. Webhooks: OpenClaw keeps idempotency keys in a process map (lost on restart); DeepSeek stores the delivery id and never checks it. Scanning: Hermes' prompt scan is a deny-list of eight patterns the code calls a tripwire; the effective protections are the deny default and the toolset denylist. Loop prevention (Hermes: a cron-spawned agent cannot create jobs unless configured) is stated in the code to be "not a security boundary". Cline's agent-created tasks start `pending_approval`, a human step that is the only such gate found.

##### 4.10 Importing

Two kinds. **Data**: transcripts converted into the harness's own session format under a fresh id (goose, Oh My Pi, Cline); agent, skill and MCP definitions read from `.claude` and `.agents` (OpenCode, goose, Cline). **Executable**: Oh My Pi reads hooks and custom tools as TypeScript from `.claude/` and `.codex` and runs them in process, so cloning a repository is code execution before any approval; the Claude provider also expands `${VAR}` in every string of a project MCP file. goose does not deduplicate re-imports.

## B. Documentation versus code: every Round 1 claim tested, per feature group

### The agent loop, tool dispatch and durability

From [round2/synth/01-loop-dispatch-durability.md](round2/synth/01-loop-dispatch-durability.md).

| Project | What the documentation said (Round 1) | What the code shows | Verdict | Permalink |
|---|---|---|---|---|
| Codex | "Independent read-only tools" run in parallel | Per-tool opt-in to a shared lock: shell is parallel-capable, file edits are not | corrected (checked in review) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/parallel.rs#L292-L304 |
| Codex | Retry-After, WebSocket-to-HTTP fallback, stop on model-not-found | As claimed; jitter is plus or minus 10% | confirmed (corrected detail) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/responses_retry.rs#L58-L167 |
| Codex | `codex exec` refuses outside Git unless skipped; approvals forced off | As claimed | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec/src/lib.rs#L1074-L1100 |
| Codex | A queue exists; no scheduler documented | Queue confirmed; no scheduler in the engine, but a desktop automation flag exists | confirmed (partly corrected in review) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/queue/src/service.rs#L90-L105 |
| OpenCode | "On the final step tools are removed" | Tool definitions stay, `toolChoice: none`, late calls refused | corrected (checked in review) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L259-L281 |
| OpenCode | A durable per-session event log with sequence numbers | Real code, but `persist` defaults to false and the shipped CLI never sets it; projections are the truth | corrected (checked in review) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L214-L214 |
| OpenCode | V1 had `doom_loop`; unknown whether V2 has any repetition guard | V2 has none outside `/goal` | confirmed (absence checked in review) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/goal.ts#L15-L19 |
| OpenClaw | Hooks can intercept `before_tool_call` and block | Also: any failure in the chain blocks | confirmed (checked in review) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.before-tool-call.policy.ts#L397-L408 |
| OpenClaw | Loop detection exists (thresholds unverified) | Thresholds 10, 20, 30 over 30 calls; rolling detector off unless enabled; only the three-identical-errors breaker is always on | corrected (checked in review) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-admission.ts#L103-L109 |
| OpenClaw | Steer is the default queue mode; 500 ms, cap 20, `summarize` | As claimed; steering is checkpoint-only and never aborts a running tool | confirmed (checked in review) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/auto-reply/reply/queue/settings.ts#L10-L41 |
| OpenClaw | Accepted input is stored before the Gateway acks | Channel ingress table and staged pending-input row; `chat.send` not traced | confirmed (checked in review) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/channels/message/ingress-drain.ts#L484-L499 |
| OpenClaw | Writer claim checked in the same transaction as every transcript write | Checked in the write transaction only for fenced writes; the claim is last-writer-wins, not a lock | corrected (corrected in review) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-transcript-write-snapshot.ts#L104-L130 |
| OpenClaw | Interrupted time-scheduled jobs get one retry | Conditions, no counter | corrected (checked in review) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/cron/store/run-recovery.kernel.ts#L191-L239 |
| OpenClaw | 48 h run timeout, 120 s model idle | As claimed | confirmed (checked in review) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/timeout.ts#L13-L14 |
| Hermes Agent | Default turn cap 500 (and sub-agent 50) | Default unlimited; sub-agents 250 | refuted (checked in review) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L63-L66 |
| Hermes Agent | Pre-hook flow with the dangerous-command check in the executor | The check is inside the terminal tool; only `pre_tool_call` blocks | corrected (checked in review) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_executor.py#L662-L676 |
| Hermes Agent | Foreground commands "move to the background" on an interrupt | Promotion is triggered by a timeout over the cap; interrupt path not settled | corrected (checked in review) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/terminal_tool.py#L1240-L1266 |
| Hermes Agent | Durable per-session queue; crashed turn `unknown`, never replayed | Confirmed; fence is a monotone epoch plus per-session generation; `unknown` resolves `interrupted` unless the same epoch captured the result | confirmed (corrected in review) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L657-L694 |
| Hermes Agent | Delivery ledger: 3 attempts in 24 h, stored before send; a send that never started is resent plainly | A crash always leaves `attempting`, so redelivery carries the "may be a duplicate" marker | confirmed and corrected (checked in review) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/platforms/base.py#L4136-L4168 |
| Hermes Agent | Cron: `jobs.json`, `executions.db`, 60 s tick | Plus at-most-once advance, headless refusal, estop | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler_tick.py#L62-L72 |
| Oh My Pi | No scheduling | No scheduler or daemon; headless fails closed only when a prompt is needed; default `yolo` | confirmed (checked in review) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L446-L470 |
| Pi | Steering "interrupts remaining tools" (homepage) | Steering polled after the whole tool batch; abort is the only interrupt | corrected (checked in review) | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L295-L308 |
| Pi | Parallel by default, per-tool sequential opt-out, `terminate` only if all set it | As claimed | confirmed (checked in review) | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L596-L670 |
| Pi | pi-durable: four backends, checkpoint per step, restart limit 5 | As claimed; limit is per task and reset by progress; tool intent committed with replay policy | confirmed (checked in review) | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/durable/src/harness/scheduler.ts#L1946-L1960 |
| DeepSeek harness | An ordered pipeline with approval resolution as a stage | Approval is reached only when an experimental plugin asks; the ask for bash and fs is inside their own bodies | corrected (checked in review) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/tools/src/pipeline.ts#L197-L232 |
| DeepSeek harness | Per-batch fsync | Plus the checkpoint policy is the actual write-ahead rule; 200 ms write-behind is the loss bound | confirmed (checked in review) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-checkpoint-policy/src/index.ts#L25-L36 |
| DeepSeek harness | Resume reads the log with a write handle and appends closers | As claimed | confirmed (checked in review) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-loop/src/index.ts#L783-L816 |
| DeepSeek harness | Tool results spill to a file (`maxInlineTokens`) | 12,500 tokens; fails open | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/spill/spill-policy/src/index.ts#L95-L131 |
| DeepSeek harness | No webhook deduplication | `deliveryId` stored, never checked | confirmed (checked in review) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/experimental/webhook/src/index.ts#L126-L160 |
| goose | Cancellation drops the running step and lets operations finalise interrupted tool responses | As claimed | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/machine.rs#L250-L285 |
| goose | Default max turns 1000 | As claimed | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L1358-L1360 |
| goose | `--max-tool-repetitions` listed as a loop guard | The flag is not wired; the inspector never counts | refuted or corrected (checked in review) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_monitor.rs#L99-L134 |
| goose | Cron for recipes, `--enable-scheduler` | As claimed; missed runs not caught up; can overlap; forced Auto | confirmed (checked in review) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/scheduler/full.rs#L171-L255 |
| goose | No generic retry or fallback | A generic retry exists; no fallback chain | refuted for retry, confirmed for fallback (checked in review) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-provider-types/src/retry.rs#L90-L158 |
| Cline | `--retries` is consecutive mistakes allowed (default 3) | True for the CLI; the SDK default is 6 and the loop detector feeds the same counter | corrected (checked in review) | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/orchestration/session-runtime-orchestrator.ts#L474-L474 |
| Cline | Documented iteration limit | None by default (CLI sets none) | not in code | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/agents/src/agent-runtime.ts#L849-L866 |
| Cline | Schedules default to yolo | As claimed | confirmed (checked in review) | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/cron/runner/cron-runner.ts#L728-L750 |
| Cline | Loop detection default | Docs say off; code runs it unless exactly `false` | docs and code disagree (read, not run; checked in review) | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/orchestration/session-runtime-orchestrator.ts#L1524-L1554 |
| OpenHands | Stuck detection default on | Client always sends it; detector, thresholds and the model for `/goal` are not here | confirmed from the client; thresholds not in code | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1232-L1236 |
| Warp | Where the loop runs (not documented) | Server side | new | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api/impl.rs#L65-L121 |
| OpenCodex | 300 s upstream stall deadline; heartbeat comment every 2 s | 300 s confirmed; default heartbeat is a typed event, comment is opt-in | confirmed, corrected detail | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/bridge/sse.ts#L1606-L1662 |

### Approvals and permission rules

From [round2/synth/02-approvals-permission-rules.md](round2/synth/02-approvals-permission-rules.md).

Every Round 1 claim in this group that a Round 2 file checked against the code. Verdicts:

- **confirmed:** the code does what the documentation said.
- **corrected:** the claim is partly right; the code column gives the correct version.
- **refuted:** the code contradicts the claim.
- **new:** the documentation said nothing and the code shows the behaviour.
- **not in code:** the documented thing could not be found in this repository.

Rows come from each repository file's section 4, with the reviewers' corrections applied. Hermes Agent's source verdict "not in code" for `execute_code` sandboxing is entered as "refuted". goose's Code Mode row is entered as "corrected", where the source says "confirmed, but it calls every tool without per-call approval".

| project | what the documentation said | what the code shows | verdict | permalink |
|---|---|---|---|---|
| Codex | Rules: most restrictive decision wins | `max()` over all matches | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/execpolicy/src/policy.rs#L577-L595 |
| Codex | `match`/`not_match` checked at load | yes; a failure drops all user and project rules | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L729-L744 |
| Codex | `bash -lc` split only for linear chains, otherwise one opaque invocation | opaque = whole argv prefix-matched; assignments, globs, escapes also refuse | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/shell-command/src/bash.rs#L29-L98 |
| Codex | TUI approvals saved to `~/.codex/rules/default.rules` | appended and hot-reloaded; also bypasses sandbox | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L516-L564 |
| Codex | "Smart approvals" propose a prefix rule | ban list; must cover whole command | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L1065-L1127 |
| Codex | Auto-review interrupts after 3 consecutive or 10-of-50 denials, 1 for cyber | counted per turn | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/guardian-reviewer/src/circuit_breaker.rs#L3-L98 |
| Codex | Auto-review fails closed on prompt-build, session, parse failures | yes; timeout is a separate retryable outcome | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/guardian-reviewer/src/completion.rs#L86-L170 |
| Codex | Reviewer sees compact transcript without reasoning, read-only checks | as claimed | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/guardian-reviewer/src/settings.rs#L43-L65 |
| Codex | Hooks trusted by hash; changed hook needs re-review | declaration and plugin bundle hashed; repo scripts not | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L972-L998 |
| Codex | PreToolUse hooks can deny | yes; fail open on error | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/events/pre_tool_use.rs#L211-L300 |
| Codex | `untrusted` approval policy retired | still the internal mode for untrusted projects | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/sandboxing.rs#L195-L236 |
| Codex | Non-loopback WebSocket listeners unauthenticated by default | app-server refuses to start; exec-server listener has no refusal | refuted | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L160-L174 |
| Codex | Custom agent cannot override sandbox, approval policy, provider, MCP | as claimed | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/role.rs#L33-L121 |
| Codex | Subagents inherit parent's sandbox and approvals | live profile re-applied last | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/child_config.rs#L197-L218 |
| Codex | (synthesis) `explorer` is a read-only agent | `explorer.toml` is empty; no restriction | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/role.rs#L316-L376 |
| Codex | Plan mode `/plan` via presets | enforcement is prompt-only | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/handlers/plan.rs#L87-L91 |
| Codex | Resume restores saved permissions subject to overrides | as claimed | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/request_processors/persisted_resume_settings.rs#L20-L45 |
| Codex | (not documented) explicit `allow` rule runs outside the sandbox | `Skip { bypass_sandbox: true }` | new | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L492-L512 |
| Codex | (not documented) `requiredHumanApproval` / `HumanCommand` | approval routed only to opted-in connections | new | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/outgoing_message.rs#L414-L430 |
| OpenCode | Ordered rules, last match wins, no match asks | as documented | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L94-L104 |
| OpenCode | Several resources: any deny denies, else any ask asks | as documented | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L184-L189 |
| OpenCode | Base policy allow all, ask on `*.env` | as documented; consequence: no shell approval by default | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/schema/src/agent.ts#L46-L51 |
| OpenCode | Agent rules append after global rules | built-in agent rules come before; per-agent config rules after all top-level rules, including managed | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/agent.ts#L64-L107 |
| OpenCode | Plan denies edits except `~/.opencode/plan`; README says it asks before bash | edit tools only; shell, subagent, execute allowed | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L33-L42 |
| OpenCode | Explore is read-only | `shell *` allowed; read-only shell is prompt-only | refuted | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L80-L93 |
| OpenCode | Custom subagent uses only its own permissions | its agent's rules plus a creation-time copy of the parent's session rules | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L311-L314 |
| OpenCode | Subagent depth default one | as documented | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/subagent.ts#L135-L139 |
| OpenCode | Shell scanner for Bash, Zsh, sh, PowerShell; unparseable input does not run | as documented | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/parse.ts#L162-L171 |
| OpenCode | Dynamically evaluated commands error | substitutions are scanned; only eval-like forms refused | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/scan.ts#L402-L438 |
| OpenCode | `git status *` matches bare `git status` | as documented | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/util/wildcard.ts#L11 |
| OpenCode | `always` saves proposed patterns, never overrides deny; `reject` rejects all pending in session | as documented, but patterns are often `*` or first-word | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L317-L363 |
| OpenCode | `external_directory` allowed by default | falls under base `*:* allow` | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/file-access.ts#L120-L137 |
| OpenCode | Policies only tighten; a policy allow can lift an earlier policy deny | only within one document; across documents the higher authority decides | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/managed-policy.ts#L9-L39 |
| OpenCode | Denied `tool.use` overrides "Allow always" | policy hook runs after saved rules | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/policy.ts#L98-L113 |
| OpenCode | Console and policy plugins cannot be disabled | as documented | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/internal.ts#L307-L309 |
| OpenCode | Console policy changes apply about once a minute | as documented; in memory only | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/provider/opencode.ts#L222-L224 |
| OpenCode | Denying `execute` removes Code Mode and tools reachable only through it | they become direct tools | refuted | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool.ts#L265-L270 |
| OpenCode | Nested tools in Code Mode enforce their own permissions | as documented | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/tool.ts#L126-L145 |
| OpenCode | Command `` !`cmd` `` runs outside the permission flow | as documented; arguments substituted before execution | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/command.ts#L190-L215 |
| OpenCode | `permission.evaluate` can change allow/ask but not configured deny | as documented | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/permission.ts#L184-L201 |
| OpenCode | Plugins run unsandboxed | in-process, auto-discovered from project dirs, no trust prompt | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/source.ts#L125-L136 |
| OpenCode | No OS sandbox | as documented | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L370-L382 |
| OpenCode | (not documented) session rules over HTTP appended last | any password holder can widen a session | new | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/handlers/session.ts#L101-L120 |
| OpenCode | (not documented) `--auto` behaviour of headless `run` | default reject with feedback; `--auto`/`--yolo` answer `once` | new | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/run/noninteractive.ts#L97-L112 |
| OpenClaw | `security` full on gateway/node, deny on sandbox; `ask` off; `askFallback` deny | Host values right; the sandbox `deny` default does not block sandboxed exec | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-defaults.ts#L171-L188 |
| OpenClaw | Approvals can only tighten config | True, except admin-only `full` session mode | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-defaults.ts#L195-L203 |
| OpenClaw | Approval bound to argv, cwd, env (when present), pinned executable | Also agentId, sessionKey, nodeId, requester device; env is a hash of overrides only | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L72-L90 |
| OpenClaw | Content hash when the executable is writable | Hashed when file, directory or symlink component is writable or owned by the user | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-mutable-file-policy.ts#L19-L66 |
| OpenClaw | Resolution re-checked before launch; drift denies | True; a small hash-to-exec window remains | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L550-L620 |
| OpenClaw | Allow-list globs on resolved paths; allow-always binds exact argv and cwd | Realpath globs, basename only for PATH lookups; allow-always is SHA-256 of cwd and argv[1..] | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-command-resolution.ts#L222-L231 |
| OpenClaw | `awk`, `sed`, `jq` always denied as safe bins; `/bin`, `/usr/bin` trusted | True | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-safe-bin-semantics.ts#L10-L18 |
| OpenClaw | Login or interactive shells skip the reviewer | True | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L631-L643 |
| OpenClaw | Three consecutive denials escalate to human | Gateway host only; in-memory LRU of 256 sessions; reset only by an allow | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L96-L110 |
| OpenClaw | Session modes read-only, guarded, workspace (reviewer), full | True | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/session-permission-exec-mode.ts#L13-L18 |
| OpenClaw | Node command gate is on the Gateway | Node also re-evaluates `system.run`, but trusts a Gateway approval over its own allow-list | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117 |
| OpenClaw | Chat approval UIs check approver lists | An empty list authorizes same-chat senders | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/approval-auth-helpers.ts#L78-L81 |
| OpenClaw | Sub-agents have no session or message tools by default | Hidden children lose `message`, `sessions_send`; leaves lose `sessions_*` | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.policy.ts#L57-L89 |
| OpenClaw | No dedicated plan mode | True; `read-only` session mode is the nearest | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/session-permission-exec-mode.ts#L13-L18 |
| OpenClaw | (not documented) `before_tool_call` failure behaviour | Any throw blocks the call | new | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.before-tool-call.policy.ts#L397-L408 |
| OpenClaw | (not documented) reviewer internals | Temp 0, 30 s, 16k chars, strict JSON, failure means ask, injection regex means deny | new | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-auto-reviewer.ts#L396-L516 |
| Hermes Agent | `approvals.mode` default `smart` | default `smart`; code falls back to `manual` if unset | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_context.py#L307-L315 |
| Hermes Agent | Prompt timeout 300 s, fail-closed | 300 s for the human; guardian's own timeout is 30 s | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_context.py#L318-L336 |
| Hermes Agent | Smart auto-approves low risk, auto-denies dangerous | only regex-flagged commands; DENY overridable once by a present human | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L882-L911 |
| Hermes Agent | Headless modes default deny | true for cron, `-q`, API, webhooks; `-z` and bare library context auto-approve | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1300-L1308 |
| Hermes Agent | Hardline refused even under YOLO/off | holds on local and SSH; skipped on container backends | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1282-L1287 |
| Hermes Agent | Piping URLs into `sh` is hardline | an approvable dangerous pattern | refuted | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_detection.py#L124-L157 |
| Hermes Agent | Fails closed on unparseable quoting | only on unlexable grep operands and parser size/separator limits | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_detection.py#L365-L375 |
| Hermes Agent | `approvals.deny` before YOLO, all backends, sees through sudo/env/sh -c | as documented | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_detection.py#L1623-L1664 |
| Hermes Agent | Container backends skip dangerous-command checks | yes, except Docker with host bind mounts | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1161-L1175 |
| Hermes Agent | `execute_code` sandboxed beyond env stripping | local runs a host kernel; no sandbox | refuted | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/code_execution_tool.py#L787-L802 |
| Hermes Agent | Per-call flow: pre hook, dangerous check, execute, post hook | the dangerous check is inside the terminal tool | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_executor.py#L679-L752 |
| Hermes Agent | (not documented) approvals bound to a pattern class; "always" writes it to config | class key persisted in `command_allowlist` | new | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L474-L487 |
| Hermes Agent | Memory and skill `write_approval` stage writes; default false | as documented; gate fails open on import failure | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/write_approval.py#L201-L216 |
| Hermes Agent | Skill lock blocks the agent incl. terminal writes | yes, by command-text parsing, not a sandbox | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/skill_lock.py#L1-L13 |
| Hermes Agent | Children cannot use clarify, memory, send_message, cronjob; depth 1 | as documented; dangerous prompts auto-deny | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/delegate_tool_toolsets.py#L14-L22 |
| Hermes Agent | (not covered) plan mode | prompt-only; no tool removed or gated | new | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/plan_prompt.py#L1-L27 |
| Hermes Agent | Dangerous commands denied in cron by default | `cron_mode: deny` | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L1658-L1662 |
| Hermes Agent | `hermes acp` approvals once/session/always/deny, 300 s timeout denies | options confirmed; timeout returns `"timeout"`, reported separately from a denial | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/permissions.py#L117-L124 |
| Oh My Pi | default approval mode is `yolo` | default `yolo` | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L314-L319 |
| Oh My Pi | three modes: always-ask, write, yolo | fourth mode `always-ask-cached` | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L43-L45 |
| Oh My Pi | per-tool allow/prompt/deny; deny always wins | tool deny, then user deny, first | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L426-L444 |
| Oh My Pi | `bash.patterns`: deny/prompt per segment, allow whole command | as stated; wrappers not unwrapped | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L311-L321 |
| Oh My Pi | critical bash patterns skipped in yolo | `override` ignored in yolo | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L657-L660 |
| Oh My Pi | undeclared custom tools exec; MCP tools write | as stated | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/tool-bridge.ts#L733-L735 |
| Oh My Pi | headless fails closed when a prompt is needed | throws without UI; error suggests yolo | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L448-L471 |
| Oh My Pi | provider safety checks always prompt, even in yolo | as stated | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L446-L455 |
| Oh My Pi | subagents run yolo but inherit `tools.approval` | `derived` yolo overlay | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L1259-L1262 |
| Oh My Pi | no OS sandbox | none found by search | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/file-write-fallback.ts#L1-L15 |
| Oh My Pi | no repo trust prompt | `isProjectTrusted` returns true | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/types.ts#L926-L937 |
| Oh My Pi | `--trusted-extension` narrows loading | as stated | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/main.ts#L1782-L1800 |
| Oh My Pi | hooks live in `.omp/hooks/pre|post/` or user dir | also project `.claude/hooks/` (and Codex, plugins) | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L377-L422 |
| Oh My Pi | `tool_call` hooks fail closed on error/timeout, 30 s | as stated | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L2338-L2364 |
| Oh My Pi | hooks cannot bypass approval | policy re-resolved after hooks | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L355-L374 |
| Oh My Pi | `mcp.enableProjectConfig` narrows project MCP | default true; project layer can re-enable | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/config.ts#L131-L134 |
| Oh My Pi | (not documented) project settings loosen approval mode, policy, restricted folders | merged with no key filter | new | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L2800-L2814 |
| Oh My Pi | (not documented) ACP treats default yolo as always-ask for bash | as stated | new | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-tools.ts#L1174-L1217 |
| Oh My Pi | (not documented) cached grants keyed tier+path in `approval-cache.db` | as stated | new | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval-cache.ts#L18-L27 |
| Oh My Pi | (not documented) plan mode | write guard in code; bash prompt-only | new | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/plan-mode-guard.ts#L89-L116 |
| Pi | No built-in permission system; runs with the user's OS permissions | No modes, rules or prompts; plain spawn | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L719-L743 |
| Pi | A failing `tool_call` handler blocks the tool call | A throw becomes an error result (fail closed) | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L735-L786 |
| Pi | Extensions can mutate tool calls | In-place mutation, never re-validated | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/docs/extensions.md#L981-L986 |
| Pi | MCP calls pass through the normal pipeline; extension gates apply | Nested calls re-enter via `runToolCall` | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L719-L742 |
| Pi | Trust order CLI, extensions, saved, `defaultProjectTrust` (`ask`), global only | As documented; cwd-only `.pi` check | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/project-trust.ts#L47-L97 |
| Pi | Context files load regardless of trust | Unconditional load | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/resource-loader.ts#L679-L685 |
| Pi | Headless: `always` loads, `ask`/`never` skip | No UI plus `ask` means untrusted | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/project-trust.ts#L78-L89 |
| Pi | No built-in sub-agents or plan mode | Absent from core; examples only | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/README.md#L19-L19 |
| Pi | (not documented) `beforeToolCall` is serial in parallel batches | Prompts never interleave | new | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L606-L627 |
| Pi | (not documented) Trust has no content binding | Path-keyed boolean | new | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/trust-manager.ts#L193-L215 |
| DeepSeek harness | Default mode not stated | `workspace-write`, env override `DSH_PERMISSION_MODE` | new | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L243-L262 |
| DeepSeek harness | Mode precedence: approved mode, last `sandbox/mode`, deployment default | as stated | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-policy/src/index.ts#L188-L205 |
| DeepSeek harness | Approval fails closed; only allowed-once grants | as stated | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/user-approval/src/index.ts#L293-L333 |
| DeepSeek harness | `never` enforced before any answerer | as stated | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/user-approval/src/index.ts#L296-L301 |
| DeepSeek harness | Failed audit append rejects the request | as stated | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/user-approval/src/index.ts#L224-L261 |
| DeepSeek harness | Requests omit tool arguments | omitted; but the UI recovers and shows a bash `command` | corrected | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/ui-chat/src/client/chat/ApprovalCommand.tsx#L16-L40 |
| DeepSeek harness | Approval resolution is a stage for every tool call | reached only when a plugin returns `ask`; none does by default | corrected | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/tools/src/pipeline.ts#L197-L206 |
| DeepSeek harness | Preset pairings; `auto` = full access + ask; `custom` derived | as stated; base also ships `read-only` | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/permission-presets/src/index.ts#L75-L86 |
| DeepSeek harness | Auto review mechanism unknown | LLM reviewer, fixed prompt, same model, fail-to-deny | new | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/experimental/auto-review/src/index.ts#L670-L693 |
| DeepSeek harness | Plan mode is soft guidance; sandbox ignores plan state | as stated | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/plan/plan-mode/src/index.ts#L1-L22 |
| DeepSeek harness | Python SDK profile pins `danger-full-access` | as stated | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/sdk-minimal/cordis.patch.yml#L70-L74 |
| DeepSeek harness | Sub-agent permission inheritance undocumented | explicit sandbox override inherited; approval pinned `never` | new | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L249-L256 |
| DeepSeek harness | No "allow always" documented | absent in code too (only experimental computer-use card) | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/ui-approval/src/client/ApprovalCard.tsx#L79-L89 |
| DeepSeek harness | MCP OAuth/approval undocumented | no OAuth; no approval hook in MCP code | new | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/mcp/mcp-client/src/tools.ts#L126-L195 |
| DeepSeek harness | Loop and web server share a process (inference) | confirmed by bundle composition | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L514-L533 |
| goose | Default mode is Autonomous; four modes | `Auto` default when `GOOSE_MODE` unset | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L1316-L1323 |
| goose | Per-tool levels in `permission.yaml` | three levels keyed by tool name only | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/permission.rs#L13-L44 |
| goose | `permissions/tool_permissions.json` holds runtime decisions | argument-hashed store exists but is dead code | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_store.rs#L79-L115 |
| goose | Smart Approve criteria not described; classified through the provider | criteria in judge tool description; sees name and arguments only; fails closed | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_judge.rs#L41-L89 |
| goose | Subagents disabled in non-autonomous modes | child inherits mode, approvals forwarded; no mode check | refuted | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/summon.rs#L416-L425 |
| goose | Subagents cannot spawn subagents or change extensions | depth one; extension management hidden | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/summon.rs#L874-L876 |
| goose | Adversary: file-enabled, `shell` default, fails open | as stated; sees user text only; BLOCK denies without asking | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/adversary_inspector.rs#L446-L464 |
| goose | Prompt injection: tool parameters matched; classifier gets tool call and recent messages; threshold 0.8 | `shell` only; classifier gets user messages only; High patterns below threshold | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/scanner.rs#L132-L144 |
| goose | Prompt injection default off | `SECURITY_PROMPT_ENABLED` default false | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/mod.rs#L46-L55 |
| goose | Extension allowlist: install rejected unless command matches exactly | prefix match, Desktop deeplink only | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/ExtensionInstallModal.tsx#L195-L199 |
| goose | Deeplinks accept `jbang, npx, uvx, goosed, docker` | `cu, docker, jbang, npx, uvx, goose, npx.cmd` | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/settings/extensions/deeplink.ts#L16-L24 |
| goose | Hooks: fail-allow default, `on_failure: block` for PreToolUse, 30 s | as stated; block covers timeouts; hooks run after approval | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/hooks/mod.rs#L853-L871 |
| goose | `--max-tool-repetitions` limits repeated tool calls | flag not wired; inspector inert | refuted | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_monitor.rs#L109-L131 |
| goose | ACP providers map goose modes onto agent modes; `auto` skips Claude's checks | confirmed; Amp Chat unrestricted, Pi unmapped, extra `copilot-acp` | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/providers/amp_acp.rs#L69-L75 |
| goose | Code Mode JS runtime is isolated (no fs/shell/net) | isolated, but inner `tools.call` skips approval, inspectors, hooks | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L172-L194 |
| goose | `--dangerously-unauthenticated` for local only | nothing restricts it to loopback | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L1821-L1834 |
| goose | (not documented) approvals expire with the extension lease; only advertised tools run | restart cannot execute a stale approval | new | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L999-L1039 |
| goose | (not documented) SmartApprove trusts MCP `readOnlyHint` | annotation alone auto-allows | new | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_inspector.rs#L172-L176 |
| goose | (not documented) failing inspector is skipped | error logged, layer removed for the batch | new | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_inspection.rs#L84-L118 |
| goose | (not documented) project plugins and hooks auto-enable | `.agents/plugins/*` enabled without prompt | new | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/plugins/discovery.rs#L123-L152 |
| Cline | The model sets `requires_approval` per command; no fixed allowlist | No such field in the SDK or extension; only a legacy string and the docs page | refuted | [schemas.ts L133-L157](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/schemas.ts#L133-L157) |
| Cline | CLI `--auto-approve` defaults to true; false in ACP | As documented | confirmed | [main.ts L802-L811, L885-L895](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/main.ts#L885-L895) |
| Cline | IDE per-category toggles; "All" variants extend their base | External/all fields are legacy and unread; reads, edits, browser and MCP default on, commands off | corrected | [sdk-tool-policies.ts L48-L68](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/sdk/sdk-tool-policies.ts#L48-L68) |
| Cline | A YOLO toggle in Settings auto-approves everything, including Plan-to-Act | The VS Code toggle was removed and migrated; CLI `--yolo` is a smaller preset; the CLI model can switch to act itself | corrected | [vscode-to-file-migration.ts L282-L313](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/hosts/vscode/vscode-to-file-migration.ts#L282-L313) |
| Cline | `--yolo` is not in the CLI reference | It exists in code and help | corrected | [program.ts L29-L32](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/commands/program.ts#L29-L32) |
| Cline | SDK tools with no policy are enabled and auto-approved | As documented | confirmed | [agent-runtime.ts L216-L234](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/agents/src/agent-runtime.ts#L216-L234) |
| Cline | Scheduled runs approve their own calls and default to yolo | As documented; any unknown mode also becomes yolo | confirmed | [cron-runner.ts L84-L108, L729](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/cron/runner/cron-runner.ts#L84-L108) |
| Cline | Plan mode removes edit tools and blocks mutating commands; `python -c` can still write | The code's own comment says the same | confirmed | [command-guard.ts L10-L17](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/command-guard.ts#L10-L17) |
| Cline | `CLINE_SANDBOX` enables a sandbox; purpose unclear | It only relocates the data directory | corrected | [helpers.ts L583-L605](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/utils/helpers.ts#L583-L605) |
| Cline | `beforeTool` hooks can skip, rewrite input, force approval or stop | True for in-process hooks; file hooks can only cancel, which stops the whole run, and `review` is unused | corrected | [hook-file-hooks.ts L577-L606](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/hook-file-hooks.ts#L577-L606) |
| Cline | Hooks have a 3 s timeout and are marked degraded, then failed, after 5 failures | True for plugin hooks only; file hooks have 120 s (SDK) or 30 s (VS Code) | corrected | [plugin-registry.ts L36-L40](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/plugin/plugin-registry.ts#L36-L40) |
| Cline | VS Code needs "Enable Hooks" | Hooks default to true | corrected | [hooks-utils.ts L6-L8](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/core/hooks/hooks-utils.ts#L6-L8) |
| Cline | CLI hooks are disabled under `--yolo` | Only in-process dispatch hooks are dropped; file hooks not settled | corrected | [hooks.ts L176-L181](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/utils/hooks.ts#L176-L181) |
| Cline | Approving `spawn_agent` approves everything the sub-agent does; no documented caps | Policies never reach the child; no depth or count cap in code | confirmed | [delegated-agent.ts L120-L147](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/team/delegated-agent.ts#L120-L147) |
| Cline | `yoloModeAllowed:false` disables the toggle org-wide | No code reads it | refuted | [schema.ts L199-L201](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/shared/src/remote-config/schema.ts#L199-L201) |
| Cline | MCP entries have `autoApprove` tool lists | No such field in the SDK schema; approval goes by tool-name policy | refuted | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/mcp/config-loader.ts#L46-L56 |
| Cline | The docs say nothing about hub authentication | A token plus a loopback-Origin fallback | corrected | [hub-websocket-server.ts L686-L696](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L686-L696) |
| Cline | Not documented: hub approvals answerable by any attached client | `approval.respond` checks only the approval id | new | [approval-handlers.ts L123-L161](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/handlers/approval-handlers.ts#L123-L161) |
| Cline | Not documented: the model-callable `switch_to_act_mode` | The model can end plan mode with an unchecked "user approved" claim | new | [mode.ts L31-L68](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/runtime/interactive/mode.ts#L31-L68) |
| Cline | Not documented: one approval for a concurrent command batch | `Promise.all` over `run_commands` entries | new | [definitions.ts L180-L196](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/definitions.ts#L180-L196) |
| Cline | 30 s notification for auto-approved commands | Not found in the SDK-based path | refuted (not in code) | none |
| OpenHands | `ConfirmRisky` is the default confirmation policy | Canvas sends `NeverConfirm` unless stored settings enable confirmation; `ConfirmRisky/HIGH/confirm_unknown` only with confirmation on and analyzer `llm` | corrected | [agent-server-adapter.ts L777-L789](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L777-L789), [settings.ts L52-L56](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/services/settings.ts#L52-L56) |
| OpenHands | Analyzers: LLM, pattern, policy-rail, ensemble | three selectable, no ensemble; only LLM changes which actions are confirmed | corrected | [agent-server-adapter.ts L791-L802](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L791-L802) |
| OpenHands | No hard-deny layer; two mechanisms | no command parsing, rule grammar or deny layer | confirmed | [agent-server-adapter.ts L777-L802](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L777-L802) |
| OpenHands | Hooks fail open | hook load errors drop all hooks; also no trust prompt for repo `hooks.json` | confirmed | [hooks-service.ts L30-L42](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/hooks-service.ts#L30-L42) |
| OpenHands | LLM analyzer: inline annotation vs separate LLM call (docs disagree) | type comment supports inline model-predicted `security_risk`; not settled | not in code | [action-event.ts L44-L61](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/types/agent-server/core/events/action-event.ts#L44-L61) |
| OpenHands | Secret values stay out of tool arguments and the agent's reach | the session key (which can answer approvals) is in the agent's environment and secret store | corrected | [dev-with-automation.mjs L975-L985](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985) |
| OpenHands | Child conversations: launch inputs validated | argument shape only; no approval, no depth or count cap | corrected | [child-conversation-launch.ts L511-L551](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/services/child-conversation-launch.ts#L511-L551) |
| OpenHands | No plan mode | read-only planner sub-conversation with three tools, `NeverConfirm` | refuted | [agent-server-adapter.ts L1352-L1363](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1352-L1363) |
| OpenHands | DelegateTool "retired" and replaced | no DelegateTool; `task_tool_set`, `workflow_tool_set` and the child-conversation client tool; "replaced" not shown | corrected | [profile-tools.ts L38-L46](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/constants/profile-tools.ts#L38-L46) |
| OpenHands | ACP approvals undocumented | no client handling of permission requests; a test fixture carries `acp_session_mode: "bypassPermissions"` | not in code | [acp-tool-call-event.ts L27-L60](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/types/agent-server/core/events/acp-tool-call-event.ts#L27-L60) |
| OpenHands | (not documented) how confirm/reject is wired | body `{accept, reason?}`, no action id; answers whatever is pending | new | [event-service.types.ts L1-L4](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/event-service/event-service.types.ts#L1-L4) |
| OpenHands | (not documented) server-level bash | key holders run host commands outside any conversation or policy | new | [bash-service.api.ts L24-L44](https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/bash-service/bash-service.api.ts#L24-L44) |
| Zed | Precedence: built-in, deny, confirm, allow, tool default, global default | Same order in `from_input` | confirmed | [tool_permissions.rs L208-L260](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tool_permissions.rs#L208-L260) |
| Zed | Global default confirm; Rust regex, case-insensitive by default | Same | confirmed | [default.json L1259-L1264](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/assets/settings/default.json#L1259-L1264) |
| Zed | Terminal chains split and each part checked, across many shells | One POSIX parser for every shell kind; no wrapper unwrapping; substitutions denied | corrected | [shell.rs L186-L203](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/util/src/shell.rs#L186-L203) |
| Zed | Hardcoded protection only for recursive forced delete of `/`, home, cwd, parent | Five `rm` regexes; they match `rm` with any or no flags | confirmed (flags detail corrected in review) | [tool_permissions.rs L21-L61](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tool_permissions.rs#L21-L61) |
| Zed | Approval card: once, always for tool, always for pattern (MCP tool-level only) | Same; "always" writes a prefix regex into settings | confirmed | [thread.rs L6985-L7075](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/thread.rs#L6985-L7075) |
| Zed | Docs do not say how rules apply to external agents | Settings comment claims deny/confirm precedence; no enforcement in the ACP client | corrected | [settings_content/agent.rs L403-L413](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/settings_content/src/agent.rs#L403-L413) |
| Zed | Sandbox covers terminal and fetch | `fetch` runs in process: shared host grants plus forbidden-IP gate | corrected | [fetch_tool.rs L45-L60](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/fetch_tool.rs#L45-L60) |
| Zed | Linux may fall back to unsandboxed with a warning | Three-way prompt (retry, run unsandboxed, deny); macOS fails closed | corrected | [terminal_tool.rs L722-L846](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L722-L846) |
| Zed | Sandbox status unclear | Feature flag enabled for all | corrected | [flags.rs L107-L121](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/feature_flags/src/flags.rs#L107-L121) |
| Zed | Sandbox grants: once, thread, always | Same | confirmed | [sandboxing.rs L292-L400](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/sandboxing.rs#L292-L400) |
| Zed | ACP permission requests are advisory; client must apply its own fs and terminal checks | Zed checks worktree paths for fs, nothing for terminals | confirmed (as spec) | [v1.rs L10549-L10590](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/v1.rs#L10549-L10590) |
| Zed | (not documented) | Sandbox escalation needs a non-empty `reason`; tool result says when it ran unsandboxed | new | [terminal_tool.rs L678-L705](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L678-L705) |
| Zed | (not documented) | Symlink escapes and `.zed`/skills paths always prompt, even under allow | new | [tool_permissions/tool_permissions.rs L627-L735](https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/tool_permissions.rs#L627-L735) |
| Warp | Allow and deny lists are regexes; allowlist empty by default; denylist pre-populated | Anchored regexes. GUI allowlist empty; CLI default allowlist has six entries | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/cloud_object_models/src/ai_execution_profile.rs#L313-L341 |
| Warp | Denylist ships with curl, wget, rm, eval | 18 entries: also shells, ssh, scp, rsync, telnet, dig, nslookup, host, exec, source | corrected | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/cloud_object_models/src/ai_execution_profile.rs#L322-L341 |
| Warp | Denylist beats allowlist and "Agent decides" / "Always allow" | As stated | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L948-L964 |
| Warp | Run until completion bypasses the user denylist; a setting turns it off | Setting defaults to true; organisation denylist stays; not in sandboxed mode; also auto-allows reads/writes, MCP, terminal writes | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L938-L956 |
| Warp | Four levels including "Never" per permission | Three levels plus `Unknown`; no `Never` | corrected | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/cloud_object_models/src/ai_execution_profile.rs#L18-L28 |
| Warp | "Agent decides" is the default for shell commands | GUI default `AlwaysAsk`; true only for the TUI | refuted | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/cloud_object_models/src/ai_execution_profile.rs#L399-L431 |
| Warp | "Agent decides" means the agent chooses the safest path | The model's self-reported `is_risky`/`is_read_only` booleans decide; the client does not verify them | new | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L972-L1008 |
| Warp | Admin denylist entries can never be bypassed | True on the client; server enforcement not visible | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L949-L953 |
| Warp | Nothing documented for local sandboxing | No OS sandbox; `--sandboxed` is a self-declared flag; dogfood Docker sandbox exists | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_terminal/src/local_tty/unix.rs#L65-L99 |
| Warp | Orchestration is one level deep | A dogfood feature flag, not a structural limit | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_features/src/lib.rs#L1079-L1079 |
| Warp | (not documented) child CLI agents' permissions | Claude/Codex children launched with bypass flags and pre-seeded trust | new | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/pane_group/pane/local_harness_launch.rs#L120-L137 |
| Warp | MCP: global file-based servers auto-spawn; project-scoped servers need approval | As stated | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/mcp/file_based_manager.rs#L396-L416 |
| Warp | ACP only in changelog wording | An ACP client is implemented, with an auto-approving permission policy | corrected | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/acp/mod.rs#L1-L16 |
| OpenCodex | Remote Workspace is read-only by default; write needs a command-sandbox probe | The probe gates `workspace.exec`; read and write are always offered; read-only is the hub's per-session default `accessMode` | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L372-L384 |
| OpenCodex | A disconnected executor never falls back to the hub filesystem | True as a prompt instruction plus absence of a local tool path, not a hard sandbox guarantee | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-tools.ts#L212-L225 |
| OpenCodex | One-time pairing code; hub stores only a hash of the bearer; ten failures in ten minutes then generic 429 | As stated; the code lives 10 minutes and the grant is deleted before metadata validation | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L377-L412 |
| OpenCodex | Model scope `allowedModels` returns 403 when out of scope | Checked against the resolved route, after aliases and combos | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/admission-model-scope.ts#L71-L76 |
| OpenCodex | Unauthenticated loopback companion listener lets any local user use the hub credentials | As stated, with a start-up warning | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index.ts#L790-L802 |
| OpenCodex | Metered-target consent | Per key (`allowMeteredComboTargets`) or per combo on loopback; no human prompt; env token never qualifies | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/responses/core-combo.ts#L663-L678 |
| OpenCodex | Telemetry on by default since v2.100.0 | Needs a notice or decision; agent-driven and non-interactive starts stay off | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/telemetry/consent.ts#L90-L127 |
| OpenCodex | (not documented) | Remote Workspace agent runs `approvalPolicy: "never"` with an auto-approved MCP bridge; the capability set is the real policy | new | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-codex-runtime.ts#L500-L513 |

### Sandboxing and where commands run

From [round2/synth/03-sandbox-containers-execution.md](round2/synth/03-sandbox-containers-execution.md).

| Project | Round 1 (documentation) said | The code shows | Verdict | Permalink |
|---|---|---|---|---|
| Codex | Linux sandbox is bubblewrap plus seccomp | two-stage helper, seccomp after bwrap | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L265-L321 |
| Codex | first `bwrap` on PATH, else the bundled helper | PATH entries the sandbox can write are skipped; the bundled binary is digest-pinned and run by fd | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/sandboxing/src/bwrap.rs#L190-L207 |
| Codex | Landlock is rejected for restricted policies (app-server sockets) | the rejection is confirmed; the stated reason is "isolate app-server sockets"; "Landlock cannot block connect" is an inference | confirmed (reason partly inferred) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L386-L393 |
| Codex (as cited in `harnesses-round-1.md`) | our residuals (pathname sockets, UDP, DNS) "are why Codex rejects Landlock" | the code names only socket isolation from the app-server daemon; UDP and DNS are not mentioned | corrected | same as above |
| Codex | `workspace-write` network off by default (unconfirmed) | `network_access` serde default false | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/protocol.rs#L1294-L1316 |
| Codex | `.git`, `.agents`, `.codex`, `.aws` protected | the four names | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/permissions.rs#L49-L54 |
| Codex | deny-read globs are a per-launch snapshot | yes | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/bwrap.rs#L470-L481 |
| Codex | refuses a command whose sandbox cannot be enforced | refusal string on the exec-server | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec-server/src/process_sandbox.rs#L226-L230 |
| Codex | proxy opt-in; with network on and proxy off, egress unrestricted | yes | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/linux_run_main.rs#L430-L441 |
| DeepSeek | the sandbox is filesystem only | and reads are not confined either | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/profiles.ts#L11-L60 |
| DeepSeek | no backend means `SANDBOX_UNAVAILABLE`, no passthrough | yes | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox-local/src/index.ts#L499-L524 |
| DeepSeek | backends report `full` or `partial` | computed and stored on the tool result, shown to nobody | corrected | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/shell/bash-sandbox/src/index.ts#L115-L124 |
| DeepSeek | SSH helper checked against a configured hash; loss is final | yes; the hash is self-reported by the remote | confirmed with correction | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/index.ts#L312-L318 |
| DeepSeek | sub-agent permission inheritance undocumented | inherits the sandbox override; approval pinned to `never` | settled | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L249-L256 |
| Zed | the sandbox covers terminal and fetch | fetch is gated in-process, not OS-sandboxed | corrected | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/fetch_tool.rs#L45-L60 |
| Zed | Linux bubblewrap "may fall back to unsandboxed with a warning" | a three-way prompt; macOS fails closed | corrected | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L722-L846 |
| Zed | sandbox maturity unclear | the flag is enabled for all | corrected | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/feature_flags/src/flags.rs#L107-L121 |
| Zed | defaults: reads allowed, writes to the project and temp, git never writable, network blocked | confirmed; on Linux the `.git` protection skips a path that does not exist yet | confirmed with caveat | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/linux_bubblewrap.rs#L246-L352 |
| Zed | Windows only inside WSL; native commands unconfined | yes | confirmed | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/sandbox/src/windows_wsl.rs#L1-L40 |
| OpenClaw | sandbox off by default | `mode ?? "off"` | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L212-L227 |
| OpenClaw | the gate is on the Gateway | the node also enforces its own policy, but a Gateway approval satisfies its allow-list | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117 |
| OpenClaw | cloud workers are outbound-only (inferred) | the worker dials, but the Gateway SSHes into the box | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291 |
| Hermes | the hardline floor is refused even under YOLO, everywhere | skipped on container backends | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1161-L1175 |
| Hermes | Docker: cap-drop ALL plus 3, no-new-privileges, pids limit 256 | pids 2048, gated on cgroups; `SETUID`/`SETGID` unless `--user` | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L336-L343 |
| Hermes | one long-lived container, `sleep infinity`, `docker exec` | yes | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L1022-L1037 |
| Hermes | iron-proxy for remote and Docker sandboxes | Docker only | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker_egress.py#L29-L47 |
| Hermes | the local terminal backend is not a sandbox | no OS sandbox code | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/local.py#L1149-L1168 |
| Oh My Pi | whether bash can run remotely: open | no remote exec; SSH is file I/O only | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/ssh/connection-manager.ts#L193-L209 |
| Oh My Pi | no OS sandbox | absence confirmed | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/file-write-fallback.ts#L1-L15 |
| Pi | runs with the account's permissions | plain `spawn` | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/bash.ts#L124-L131 |
| Pi | Docker, Docker Sandboxes and OpenShell isolation patterns | documentation only | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/docs/containerization.md#L9-L16 |
| Pi | pi-env: restricted SSH options, strict host keys; "host-key scan wording not found" | flags confirmed; `scanHostKey` exists | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/ssh.ts#L59-L123 |
| Pi | how pi-env connects to the CLI: not stated | it does not; only `apps/p2` uses it | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/apps/p2/package.json#L29-L30 |
| goose | an opt-in macOS Desktop Seatbelt sandbox (unverified) | removed; only a blog post remains | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/documentation/blog/2026-02-23-goose-v1-25-0/index.md#L19-L34 |
| goose | `--container` runs extensions in Docker | stdio and built-in extensions only; the Developer shell stays on the host | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/mod.rs#L932-L948 |
| Cline | no OS sandbox; `CLINE_SANDBOX` meaning unclear | none; `CLINE_SANDBOX` moves the data directory | confirmed / settled | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/utils/helpers.ts#L583-L605 |
| Cline | SSH tunnel timeout 45 s | 40 s | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L177-L182 |
| OpenCode | nothing documented | nothing in code | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell.ts#L370-L382 |
| OpenHands | host process, no isolation | yes | confirmed | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L727-L736 |
| OpenHands | Docker hardening flags and the outer proxy | not in this repository | not in code | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-safe.mjs#L1009-L1028 |
| OpenHands | Docker runtime mode binds all interfaces | yes | confirmed | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L528-L543 |
| OpenHands | Docker whole-stack: container, mounted directories only | true toward the host; inside, the agent reads every credential | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/Dockerfile#L188-L206 |
| Warp | no local sandbox | confirmed; `--sandboxed` is a trusted flag; a dogfood Docker sandbox exists | confirmed, plus new | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_cli/src/agent.rs#L621-L623 |
| OpenCodex | bubblewrap-sandboxed commands | yes, with the flag list in section 2 | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-command-runner.ts#L23-L47 |
| OpenCodex | read-only by default; writing needs a command-sandbox probe | the probe gates `exec`; writing needs no probe | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L372-L384 |
| OpenCodex | a disconnected executor never falls back to the hub filesystem | an instruction plus the absence of a local tool path, not a hard guarantee | confirmed (weaker) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-tools.ts#L212-L225 |

### Credentials, secrets and authentication

From [round2/synth/04-credentials-secrets-auth.md](round2/synth/04-credentials-secrets-auth.md).

| Project | What the documentation said (Round 1) | What the code shows | Verdict | Permalink |
|---|---|---|---|---|
| Codex | Non-loopback WebSocket listeners allow unauthenticated connections during rollout | App-server refuses to start; flag removed. Exec-server `--listen` still allows it | refuted (app-server), corrected in review for exec-server | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L160-L175 |
| Codex | `ignore_default_excludes = true` | Confirmed; `OPENAI_API_KEY` reaches commands | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/shell_environment_policy.rs#L133-L136 |
| Codex | Placeholder credential substitution (Codex Cloud only) | A local credential broker exists, off by default | new | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/network-proxy/src/config.rs#L185-L228 |
| Codex | DNS check is "best effort" | Dialed address re-checked; gap only with an upstream proxy | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/network-proxy/src/connect_policy.rs#L199-L223 |
| Codex | A changed hook needs review again | True for declarations and plugin bundles, not for scripts a repo hook points at | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L836-L882 |
| OpenCode | Password protected by default | One shared secret, no accounts, also accepted as `?auth_token=` | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/middleware/authorization.ts#L13-L40 |
| OpenCode | Pairing: single-use, 5 minutes, 30-day cookie | Confirmed; token derived from the password, no individual revocation | confirmed, with new detail | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/auth.ts#L39-L55 |
| OpenCode | `--remote` is end-to-end encrypted | Encryption lives in an external package | not settled | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/services/remote-tunnel.ts#L7-L47 |
| OpenClaw | GitHub publication via a Gateway broker (no GitHub credential on the worker) | A turn-scoped GitHub token reaches the worker and its shell | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/launch-descriptor.ts#L183-L211 |
| OpenClaw | Workers are outbound-only | Agent link is outbound; the Gateway SSHes in for bootstrap and sync | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291 |
| OpenClaw | Control UI token kept in session storage per tab | True for the shared token; device key and device tokens in `localStorage` | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/lib/nodes/index.ts#L239-L251 |
| OpenClaw | Per-dispatch credential stored as a hash; inference proxied | Confirmed; hash also binds the turn claim | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential.ts#L9-L60 |
| OpenClaw | Egress proxy with sentinels | Confirmed; cooperative, not enforced by a namespace (inferred) | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/egress-proxy/stream-substitution.ts#L26-L31 |
| OpenClaw | Billing failures disable a credential 10 minutes | 10 minutes doubling to 24 hours | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/usage-failure-state.ts#L18-L46 |
| Hermes Agent | iron-proxy is for remote and Docker sandboxes (docstring: Docker/Modal/SSH) | Wired into Docker only; own model calls bypass it | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L786-L812 |
| Hermes Agent | iron-proxy binary is pinned | A binary on PATH wins over the pinned one | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/proxy_sources/iron_proxy.py#L162-L177 |
| Hermes Agent | Non-loopback dashboard fails closed without auth; single-use 30 s tickets | Confirmed; `--insecure` is ignored | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/main_dashboard.py#L640-L662 |
| Hermes Agent | Stdio MCP servers get a safe env baseline | Confirmed | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_config.py#L199-L207 |
| Oh My Pi | Auth broker is loopback-only | Loopback default; `--bind 0.0.0.0` accepted without warning | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/types.ts#L177-L184 |
| Oh My Pi | Gateway has its own inbound bearer | Also scoped clients by digest, model and account allow-lists | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/clients.ts#L1-L60 |
| Oh My Pi | Hooks live in `.omp/hooks/` or the user dir | Also imported from the project `.claude/hooks/` | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L377-L396 |
| Oh My Pi | No trust prompt documented | `isProjectTrusted()` returns `true` | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/types.ts#L926-L937 |
| Pi | Trust order: CLI, extensions, saved, default `ask`; context files load regardless | Confirmed; no content binding; cwd-only `.pi` check | confirmed, new detail | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/project-trust.ts#L47-L97 |
| Pi | pi-env: credentials stay local | True because only the tools move and the client env is not forwarded | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/remote-env.ts#L55-L58 |
| DeepSeek harness | Launch token, signed cookie, Host/Origin fence | Confirmed; 30-day persistent secret, no revocation; wildcard bind refused | confirmed, new detail | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/connection/src/browser-auth.ts#L182-L199 |
| DeepSeek harness | Credential-style names scrubbed from children | Confirmed; defeated by unrestricted reads of the store | confirmed, with gap | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subprocess/subprocess/src/index.ts#L40-L75 |
| goose | Secret in `X-Secret-Key` | Also the `token` query parameter | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/auth.rs#L9-L36 |
| goose | Fingerprint printed, otherwise trust on first use | TOFU only in the desktop client; server checks nothing | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/main.ts#L305-L379 |
| goose | Extension allowlist command must match exactly | Prefix match, Desktop deeplinks only | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/ExtensionInstallModal.tsx#L177-L204 |
| goose | Keyring preferred, plaintext fallback, env precedence | Confirmed; one blob; silent fallback sets a process-wide flag | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L1262-L1281 |
| goose | `GOOSE_TERMINAL=1`, `AGENT=goose` set for commands | Only for recipe retry checks | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/retry.rs#L51-L64 |
| Cline | No hub authentication documented | Per-start token plus a loopback-Origin shortcut | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L676-L722 |
| Cline | Provider key sent over the tunnel per session, not written to host settings | Sent in `session.create`; on-disk persistence not traced | confirmed in part | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/examples/desktop-app/sidecar/chat-session.ts#L921-L980 |
| Cline | Keys plaintext under `~/.cline/data/settings/`; encryption not documented | Not encrypted | confirmed | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/services/storage/provider-settings-manager.ts#L186-L214 |
| Cline | `CLINE_PLUGIN_MODE=sandbox` restricts plugins | Crash isolation only | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/tools/worker-sandbox.ts#L267-L276 |
| Cline | Remote config `yoloModeAllowed` | Read by no code | confirmed (unenforced) | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/shared/src/remote-config/schema.ts#L199-L201 |
| OpenHands | Browser embeds the key only with explicit Docker opt-in | Embedded by default on loopback | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/static-server.mjs#L389-L407 |
| OpenHands | Docker whole-stack isolates to mounted dirs | Inside it, agent and keys share one uid | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docker/Dockerfile#L188-L206 |
| OpenHands | Whether `X-Session-API-Key` is checked on WebSocket upgrades | The client sends it as a first frame instead | corrected (server side not in repo) | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/websocket-auth.ts#L1-L18 |
| Warp | Redaction is off by default, regex-based | Confirmed; coverage list new | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/terminal/safe_mode_settings.rs#L74-L82 |
| Warp | Do local model calls go through Warp? | Yes, always, with BYO keys | new | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api/impl.rs#L59-L63 |
| OpenCodex | Keys are env references or pasted; OAuth in separate stores, not `config.json` | Pasted keys land in `config.json` as plaintext unless keychain | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/providers/api-key-resolve.ts#L26-L29 |
| OpenCodex | One-time pairing code, hash of bearer, 429 after ten failures | Confirmed; lifetime 10 minutes | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L377-L425 |
| OpenCodex | Read-only default; write needs a command-sandbox probe | The probe gates `exec`; writes are offered without it | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L372-L384 |
| OpenCodex | 4 MiB WebSocket relay frame cap | 4 MiB is the SSE frame cap; live-sideband WS frame cap is 50 MiB | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/sse-frame-buffer.ts#L4-L4 |
| OpenCodex | Cooldown memory-only | Combo cooldowns are persisted | refuted | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/combos/cooldown-store.ts#L1-L25 |

Our own design note `docs/nodes-credentials-environments.md` §7 cited documentation-level
patterns. The code changes two of them: "Key sent per session, never written" (Cline) is only
confirmed in part, and no lifetime on the remote was found; "Inference proxied by the control
side" is confirmed for OpenClaw, Oh My Pi's gateway and OpenCodex, and OpenClaw also keeps a
worker-local mode.

### Links, workers and client protocols

From [round2/synth/05-links-workers-protocols.md](round2/synth/05-links-workers-protocols.md).

| Project | What Round 1 (documentation) said | What the code shows | Verdict | Permalink |
|---|---|---|---|---|
| Codex | Non-loopback WebSocket listeners are unauthenticated by default "during rollout" | app-server refuses to start without auth; exec-server `--listen` has no such refusal | refuted (app-server), corrected (exec-server) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L160-L175 |
| Codex | WebSocket auth before `initialize`; `Origin` gets 403 | at the HTTP upgrade | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/websocket.rs#L99-L130 |
| Codex | Bounded queues answer `-32001` | requests only; other messages block | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/mod.rs#L231-L262 |
| Codex | Daemon Unix socket is host-local | 0700 dir, 0600 socket, verified symlink | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/unix_socket.rs#L322-L352 |
| Codex | `environment_id` runs an MCP server on a remote executor | MCP stdio through the executor | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rmcp-client/src/executor_process_transport.rs#L1-L20 |
| Codex | Handoff moves chat and code; worktree from HEAD plus uncommitted changes | HEAD only; Handoff is closed Desktop | not in code | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/worktree/src/lib.rs#L62-L140 |
| OpenCode | Pairing link single-use, 5-minute expiry; 30-day cookie | as stated; token is password-derived, no per-token revocation | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/pairing.ts#L7-L34 |
| OpenCode | Web UI password-protected by default | refuses to start without a password; one shared secret | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/process.ts#L55-L58 |
| OpenCode | `--remote` is end-to-end encrypted via OpenTunnel | encryption in the external package | not in code | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/cli/src/services/remote-tunnel.ts#L26-L47 |
| OpenCode | Durable per-session log at `/log`, exclusive sequence, `follow=true` | API exists; shipped service writes nothing to the event table | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L214-L214 |
| OpenCode | `/api/event` volatile; slow consumer overflows | queue 4,096, then the subscriber fails | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/event-feed.ts#L66-L68 |
| OpenCode | One shared service, no node fleet | no node or worker dial-in code | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/workspace/driver.ts#L88-L88 |
| OpenCode | SSH forward is a manual option | the desktop automates SSH bootstrap and tunnelling | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/gui-plugins/src/ssh/bootstrap.ts#L76-L100 |
| OpenClaw | Worker dials the Gateway's public worker route | WebSocket or Unix socket | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/node-worker-supervisor-commands.ts#L173-L204 |
| OpenClaw | Worker connection is outbound-only | the Gateway also SSHes into the box | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291 |
| OpenClaw | Closed method allow-list on the worker route | closed, feature-gated, default reject | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L54-L66 |
| OpenClaw | Each dispatch mints a credential, only the hash stored | domain-tagged hash bound to turn claim | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential.ts#L41-L60 |
| OpenClaw | Owner-epoch fencing rejects stale workers | per-request revalidation; bump limited to some transitions | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L218 |
| OpenClaw | Provider keys stay on the Gateway; inference by model ref | confirmed, plus a worker-local mode | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-model.ts#L91-L124 |
| OpenClaw | GitHub publication via a Gateway broker | the worker receives a GitHub token | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/launch-descriptor.ts#L183-L211 |
| OpenClaw | Writer claim checked in the same transaction as every transcript write | only for fenced writes; claim supersedes the holder | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-scope.ts#L79-L97 |
| OpenClaw | Every client and node signs the challenge nonce | when a device block is sent | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/role-policy.ts#L13-L16 |
| OpenClaw | SSH-verified auto-approval, default on | compares device id and key | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-pairing-ssh-verify.ts#L190-L240 |
| OpenClaw | Gateway allow-list is the gate for node commands | the node also re-evaluates `system.run`, but trusts a Gateway approval over its allow-list | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117 |
| OpenClaw | Control UI token in session storage per tab | device key and device tokens in `localStorage` | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/ui/src/lib/nodes/index.ts#L112-L126 |
| OpenClaw | Accepted input stored before the Gateway acks | staged rows and an ingress table | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L897-L914 |
| Hermes | CLI attaches with single-use 30 s tickets via a control socket | ticket in a WebSocket subprotocol | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/runtime_bootstrap.py#L25-L75 |
| Hermes | Non-loopback dashboard fails closed without auth | and `--insecure` is ignored | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/web_server_boundary.py#L111-L119 |
| Hermes | Relay outbound-only, per-gateway secret and delivery key | gateway side confirmed; connector buffering not in repo | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/gateway/relay/ws_transport.py#L442-L494 |
| Hermes | Crashed turn `unknown`, never replayed | and unattended ones auto-settle | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L453-L464 |
| Hermes | Kanban worker per OS process | a thin client; the agent runs in the gateway | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/kanban_db_dispatch.py#L2697-L2700 |
| Hermes | `hermes acp` serves ACP over stdio with persistent sessions | true, but it is a viewer of the gateway | corrected (process model) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/acp_adapter/entry.py#L205-L215 |
| Oh My Pi | RPC NDJSON, v1 1 MiB, v2 64 MiB chunks | as stated; v1 frames shrunk, not rejected | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/modes/rpc/rpc-frame.ts#L5-L9 |
| Oh My Pi | Whether `bash` runs over SSH is unknown | no remote exec back end; `ssh://` file I/O only | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/ssh/connection-manager.ts#L700-L709 |
| Oh My Pi | Auth broker loopback-only by default | a default bind; any `--bind` accepted | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-broker-cli.ts#L145-L157 |
| Oh My Pi | Auth gateway: own bearer, clients get no credentials | and scoped clients by SHA-256 digest | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/clients.ts#L1-L30 |
| Oh My Pi | Collab: relay, AES-256-GCM, key in fragment, no accounts | as stated | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/collab/crypto.ts#L1-L10 |
| Pi | RPC command list including tree navigation | no `navigate_tree` command | corrected | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/modes/rpc/rpc-types.ts#L22-L74 |
| Pi | pi-env: Rust daemon, framed stdio, SHA-256-named, strict host keys, no forwarding | as stated | confirmed | [ssh.ts#L77-L123](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/ssh.ts#L77-L123) |
| Pi | pi-env keeps worker, storage and credentials local | only tools move | confirmed | [remote-env.ts#L871-L874](https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/env/src/remote-env.ts#L871-L874) |
| Pi | `pi-web-ui` is probably gone | not a package, but `apps/p2` is a web UI | corrected | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/apps/p2/README.md#L1-L24 |
| DeepSeek | SSH remote: same policy, separate back end | helper hash is self-attestation | confirmed, with correction | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/ssh/ssh/src/helper.ts#L103-L119 |
| DeepSeek | Browser surface is "fetch/SSE" | HTTP POST plus a WebSocket mux | corrected | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/gateway/src/stream-protocol.ts#L6-L7 |
| DeepSeek | Loop and web server share one process | bundle composition settles it | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/web-app/cordis.patch.yml#L116-L136 |
| goose | `goose serve` is HTTP at `/acp` plus WebSocket, port 3284 | as stated | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L860-L905 |
| goose | Secret as `X-Secret-Key`; `token` query for browser WebSockets | as stated; constant-time on `/acp` | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/auth.rs#L15-L36 |
| goose | Startup prints a fingerprint for pinning, otherwise TOFU | TOFU is desktop-only | confirmed | [main.ts L360-L379](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/main.ts#L360-L379) |
| goose | `--dangerously-unauthenticated` is for local use only | nothing restricts it to loopback | corrected | [cli.rs L1821-L1834](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L1821-L1834) |
| goose | Roaming: revoke closes connections within seconds | 2 s poll | confirmed | [node.rs L324-L355](https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/node.rs#L324-L355) |
| goose | Roaming: spawned OS processes survive revocation | shell children are killed by `ShellProcess::drop`; `setsid` and MCP children not covered (review answer) | not settled in full | see section 7 |
| Cline | "Spoke" workers run the loop, owned by the daemon | the hub runs the loop in-process; no spoke code | refuted | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-server-transport.ts#L334-L343 |
| Cline | The hub has no documented authentication | random token plus a loopback-Origin fallback | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-websocket-server.ts#L676-L722 |
| Cline | Roles creator, participant, observer per participant | a type; no authorisation reads it | confirmed as a type only | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/shared/src/hub.ts#L225-L230 |
| Cline | SSH: helper under `~/.cline/remote`, loopback hub, `ssh -L` to a random port | helper is the CLI binary | confirmed | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L325-L400 |
| Cline | Tunnel closes after about 45 s of network loss; provider key sent per session | 15 s x 3 keepalives; the separate startup timeout constant is 40 s; key sent, refresh token removed | confirmed, with correction | [remote-environments.ts L177-L182](https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/remote/remote-environments.ts#L177-L182) |
| OpenHands | Whether `X-Session-API-Key` is checked on WebSocket upgrades | Canvas sends the key as the first message, not on the upgrade; server enforcement not in repo | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/websocket-auth.ts#L1-L18 |
| OpenHands | Request path is browser to Canvas to selected backend | the browser calls the Agent Server directly | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/no-direct-agent-server-calls.test.ts#L5-L10 |
| OpenHands | Browser embeds the key only with explicit Docker opt-in | on a loopback bind the key is always embedded | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/bind-host.mjs#L50-L77 |
| OpenHands | Docker runtime mode binds all interfaces | as stated | confirmed | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L528-L543 |
| OpenHands | DigitalOcean sessions appear as backends, PAT auth | as stated, opt-in | confirmed | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/mars-tunnel-bridge.mjs#L1-L18 |
| Zed | Docs do not say how tool permissions apply to external agents | the settings comment claims precedence; no enforcement found | corrected | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/settings_content/src/agent.rs#L403-L413 |
| Zed | ACP permission requests are advisory; clients apply their own checks | Zed applies path checks to fs only, none to terminal | confirmed as spec | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/v1.rs#L10549-L10590 |
| Warp | ACP support appears only in changelog wording | Warp implements an ACP client for cloud and headless runs | corrected | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/acp/bridge.rs#L1-L22 |
| Warp | Where the loop runs (unstated) | on Warp's server; the client executes tools | new | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api/impl.rs#L16-L26 |
| Warp | Hand-off blocked while a command runs or children are active | as stated | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/handoff/pipeline.rs#L440-L461 |
| Warp | Self-hosted workers | not in this repository | not in code | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/specs/REMOTE-1373/TECH.md#L87-L112 |
| OpenCodex | Remote Workspace: Ed25519-signed ECDH, AES-256-GCM | as stated | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/crypto.ts#L289-L312 |
| OpenCodex | One-time pairing code; hub stores a hash of the bearer; 429 after ten failures | as stated | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L377-L412 |
| OpenCodex | Read-only default; write needs a command-sandbox probe | the probe gates `exec`; writes need none | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-device.ts#L372-L384 |
| OpenCodex | A disconnected executor never falls back to the hub filesystem | instruction plus absence of a local tool path | confirmed (not a hard guarantee) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-tools.ts#L212-L225 |
| OpenCodex | Remote Workspace may be the `ocx-child` relay | `ocx-child` is Remote Link | refuted | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/native/ocx-child/src/lib.rs#L1-L20 |
| OpenCodex | WebSocket relay 4 MiB per frame | 4 MiB is the SSE frame cap; live-sideband WebSocket cap 50 MiB | corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/sse-frame-buffer.ts#L4-L4 |
| OpenCodex | Unauthenticated loopback companion listener exposes hub credentials to local users | as stated | confirmed | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/index.ts#L790-L802 |

### Session storage, resume, fork and checkpoints

From [round2/synth/06-sessions-fork-checkpoints.md](round2/synth/06-sessions-fork-checkpoints.md).

| Project | What Round 1 said | What the code shows | Verdict | Permalink |
|---|---|---|---|---|
| Codex | `thread/fork` copies history up to `lastTurnId` | Paginated forks store a position into the parent's file (a pointer); the legacy copy case was not traced | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/protocol.rs#L3463-L3477 |
| Codex | `thread/revert` drops turns, does not revert files | Same; implemented as a new immutable file plus CAS | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-protocol/src/protocol/v2/thread.rs#L1316-L1326 |
| Codex | No automatic file checkpoints | A snapshot feature existed and was removed; old records are only stripped | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/rollout_migration/line_parser.rs#L47-L58 |
| Codex | Events saved in `rollout-*.jsonl`, each by one process | Advisory file lock with coordinator | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/rollout/src/writer_lock.rs#L45-L95 |
| Codex | Resume restores saved permissions subject to overrides | Last `ThreadSettingsApplied`, owner rule | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server/src/request_processors/persisted_resume_settings.rs#L20-L45 |
| Codex | Managed worktree starts from HEAD plus uncommitted changes; Handoff moves chat and code | HEAD only; Handoff not in the open-source tree | not in code | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/worktree/src/lib.rs#L62-L140 |
| OpenCode | Durable per-session log at `/api/experimental/session/{id}/log` | A real API, but in the shipped CLI service nothing is written to the event table | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L214-L214 |
| OpenCode | Snapshots use a Git store separate from the repo | Confirmed, with alternates to the project's objects | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/snapshot.ts#L95-L97 |
| OpenCode | Snapshot before and after each model step; paths attributed | As described | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/step.ts#L278-L296 |
| OpenCode | Untracked files up to 2 MiB | As described | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/snapshot.ts#L141-L141 |
| OpenCode | `/undo` rejected while a session runs | Busy error | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/handlers/session.ts#L445-L458 |
| OpenCode | Fork before a message | As described | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L339-L382 |
| OpenCode | `session export --sanitize` | As described | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/transfer.ts#L210-L239 |
| OpenCode | Session sharing discontinued; V2 schema still has `share` | Removed in v2.0.30; `share` parsed, no consumer found | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/CHANGELOG.md#L1236-L1236 |
| OpenCode | `GET /api/event` volatile by contract | Queue 4,096, then the subscriber fails | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/server/src/event-feed.ts#L66-L68 |
| OpenClaw | `pruneAfter` 30 d, `maxEntries` 5000 | Confirmed, plus a 10 GiB disk budget | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/store-maintenance.ts#L26-L34 |
| OpenClaw | Accepted input is stored before the Gateway acks | Pending-inputs table | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/state/openclaw-agent-schema.sql#L897-L914 |
| OpenClaw | Restart resume injects one resume input | Fixed prompt | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/session-restart-resume.ts#L78-L85 |
| OpenClaw | Managed worktrees snapshot to `refs/openclaw/snapshots/<id>` | As described | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/worktrees/snapshot-inventory.ts#L431-L434 |
| OpenClaw | Writer claim checked in the same transaction as every transcript write | Checked in the write transaction only for fenced writes; the claim supersedes the holder | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/sessions/session-accessor.sqlite-scope.ts#L79-L97 |
| OpenClaw | Owner-epoch fencing rejects stale workers | Confirmed; the bump is limited to certain source states | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L218 |
| Hermes | SQLite `state.db` with WAL and FTS5 | WAL avoided on affected SQLite builds, refused on virtiofs and 9p | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_wal.py#L301-L304 |
| Hermes | Compaction in place by default | Yes | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L667-L671 |
| Hermes | Resume restores cwd and finds the latest session | Yes | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/main.py#L1464-L1478 |
| Hermes | Shadow-git checkpoints opt-in, one shared bare repo, ref per project | Yes | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L13-L39 |
| Hermes | Snapshots before `write_file`/`patch` and destructive terminal commands, once per directory per turn | Yes; the terminal trigger is a command-text regex with known misses | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_executor.py#L1030-L1046 |
| Hermes | Caps 20 / 500 MB / 10 MB / 7 days / 50,000 files | Yes | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L485-L503 |
| Hermes | Never snapshots `/` or `$HOME` | Also every ancestor of `$HOME` | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L872-L875 |
| Hermes | `/rollback` skips files the user edited since unless `--all` | True, but the ledger only tracks file-tool writes | confirmed with a caveat | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L986-L1050 |
| Hermes | Nested git repos stored as gitlinks, "not fully recoverable" | Whole-tree rollback is refused | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1376-L1385 |
| Hermes | Checkpoints disabled on container backends | Yes | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L940-L952 |
| Hermes | Crashed turn marked `unknown`, never replayed, session blocks until discarded | Yes; unattended ones auto-settle | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_state_runtime.py#L453-L464 |
| Hermes | Export `jsonl`, `md`, `html`, `trace` with `--redact` | Trace redaction on by default, aborts on failure | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/sessions_cmd.py#L500-L503 |
| Oh My Pi | Session JSONL v3, 256-byte title record, one header | Yes | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-entries.ts#L17-L35 |
| Oh My Pi | Branching makes siblings in the same file | Yes; active leaf is the last entry except `activeLeafId` | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4048-L4055 |
| Oh My Pi | `/branch` rewinds; `/fork` clones to a new file | `/branch` also creates a new file | confirmed with a detail | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-manager.ts#L4144-L4226 |
| Oh My Pi | Checkpoint and rewind tools off by default; no file checkpoints | Yes; the tool text claims git, the code does not | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/checkpoint.ts#L53-L131 |
| Oh My Pi | Images in a content-addressed blob store | Yes | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/blob-store.ts#L7-L30 |
| Pi | Session JSONL with `id`/`parentId`, current leaf is the active branch | The leaf is derived as the last entry | confirmed with a caveat | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/session-manager.ts#L1104-L1123 |
| Pi | Format versions v1 linear, v2 tree, v3; old files auto-migrate | Rewrite in place, no temp file | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/session-manager.ts#L286-L347 |
| Pi | `/tree` navigates in one file; `/fork` creates a new file; `/clone` copies the active branch | Fork and clone copy the path only | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session-runtime.ts#L262-L350 |
| Pi | `/export` HTML or JSONL; `/share` | HTML has all branches, JSONL the current branch | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/session-export.ts#L9-L30 |
| Pi | No built-in checkpoint; `git-checkpoint.ts` example | In-memory, tracked files only, fork only | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/examples/extensions/git-checkpoint.ts#L10-L48 |
| Pi | pi-durable restart limit default 5, checkpoint per step, commit-before-display | Per task, reset on progress | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/durable/src/harness/agent.ts#L60-L64 |
| DeepSeek | Checksummed concatenated zstd frames, atomic materialisation, per-batch fsync, torn-tail truncation | Yes | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-persistence-jsonl/src/zstd.ts#L18-L20 |
| DeepSeek | Metadata stored separately | The header is the first record of the same file | corrected | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-persistence-jsonl/src/format.ts#L79-L94 |
| DeepSeek | Old formats v0, v1, v2 translated; newer refused | Writer is v4; v0 to v3 translated | corrected | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/docs/session-format-status.md#L41-L49 |
| DeepSeek | Resume appends closers | Yes | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/repair.ts#L53-L98 |
| DeepSeek | One live writer per session | `flock`, no expiry | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-persistence-jsonl/src/lease.ts#L70-L116 |
| DeepSeek | A fork cut inside an open turn is rejected | Accepted; the open turn is closed with `forked` closers | refuted | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/fork.ts#L21-L30 |
| DeepSeek | Fork child records parent and inherited count | Count and marker yes; `parentSession` only for sub-agents | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/types.ts#L107-L112 |
| DeepSeek | SQLite full-text index backs session search | In-memory and disabled by default in the web profile | corrected | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/web-app/cordis.patch.yml#L22-L30 |
| DeepSeek | Undo, checkpoints, rewind: nothing documented | Nothing in code either | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/deliverables/workspace-changes/src/index.ts#L1-L8 |
| goose | SQLite `sessions.db` shared by desktop and CLI | Yes | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/session/session_manager.rs#L74-L75 |
| goose | Ids `YYYYMMDD_<COUNT>` | Yes | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/session/session_manager.rs#L1858-L1900 |
| goose | Fork copies a session; edit deletes later messages | Yes | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/session/session_manager.rs#L2965-L3013 |
| goose | Import is desktop only; exports Markdown, JSON, YAML, HTML | The CLI has import too; it converts Claude Code, Codex and Pi | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/session/import_formats/mod.rs#L150-L157 |
| goose | Checkpoints and file undo: nothing | Nothing in code | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/session/session_manager.rs#L2669-L2706 |
| Cline | Checkpoints use a shadow git repo separate from the project's | Stash commits in the user's repo under `refs/cline/checkpoints/...` | refuted | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/checkpoint-hooks.ts#L624-L640 |
| Cline | A commit after each tool use | One snapshot per user run, at the first model call | refuted | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/checkpoint-hooks.ts#L658-L708 |
| Cline | Checkpoints on by default | Opt-in in the core, on in the CLI, true by default in VS Code | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/runtime/defaults.ts#L17-L24 |
| Cline | Checkpoints work on non-git folders | Not for the built-in snapshotter (a host callback can) | refuted | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/checkpoint-hooks.ts#L515-L533 |
| Cline | Restore offers Files only, Task only, Files and Task | `workspace`, `task`, `taskAndWorkspace` | confirmed | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/sdk/SdkController.ts#L1952-L1955 |
| Cline | "Task only" drops later messages | Starts a forked new session; the old one is kept | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/session/session-versioning-service.ts#L307-L346 |
| Cline | Sessions at `~/.cline/data/sessions/<id>/` with `db/sessions.db` | Yes | confirmed | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/services/session-artifacts.ts#L63-L92 |
| Cline | Fork of a conversation not documented | Fork metadata and restore-forks exist | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/session/fork-metadata.ts#L1-L29 |
| OpenHands | A fork does not fork file state | Fork sends only `from_event_id` and title | confirmed | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/conversation-service/agent-server-conversation-service.api.ts#L1301-L1340 |
| OpenHands | On-disk `base_state.json` plus one JSON file per event | Not in this repository | not in code | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/config/defaults.json#L25-L31 |
| Zed | Per-prompt Git checkpoints removed in 1.27.0 | Mechanism absent; only archive checkpoints remain | confirmed | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/git/src/repository.rs#L1075-L1088 |
| Zed | Undo is by rejecting edits or using Git | Rewind rejects all unreviewed edits in the thread; the log is in memory only | corrected | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/acp_thread/src/acp_thread.rs#L8028-L8070 |
| Warp | Handoff snapshots tracked and untracked changes; blocked while a command runs or children are active | Yes; adds the 500-call scan and 100-file and 25 MiB caps | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/snapshot.rs#L1723-L1756 |

### Context: prompts, compaction, instruction files, memory

From [round2/synth/07-context-compaction-memory.md](round2/synth/07-context-compaction-memory.md).

Round 1 claims in this group, as each repository's findings checked them. "Corrected" means the code differs in a detail that matters; "new" means the documentation did not describe it.

| Project | What the documentation said | What the code shows | Verdict | Permalink |
|---|---|---|---|---|
| Codex | Compaction triggers: `/compact`, `model_auto_compact_token_limit` (scopes `total`, `body_after_prefix`), post-turn percent | Confirmed; the auto limit has a 90% default; post-turn percent defaults to 0 | confirmed (checked in review) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/session/context_window.rs#L62-L120 |
| Codex | Prompt overridable by `compact_prompt` | As described | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/compact.rs#L127-L133 |
| Codex | Providers can declare `remote_compaction = v2` | Confirmed; default on for OpenAI and Azure | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/model-provider/src/capabilities.rs#L33-L58 |
| Codex | 0.164.0 fixed failed compaction leaving partial summaries | History is replaced atomically and a failed attempt changes nothing; an empty summary is accepted outside post-turn | corrected (corrected in review; the author's own file said "refuted" in the finding and "corrected" in the table) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/compact.rs#L439-L455 |
| Codex | AGENTS.md chain: global override-else-default, project root-to-cwd, root first | Confirmed; one file per directory | confirmed (checked in review) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agents_md.rs#L199-L309 |
| Codex | Empty files skipped; stop at `project_doc_max_bytes` (32 KiB); two pages disagree on per-file versus combined | Budget is combined across project files and excludes the global file | confirmed and resolved: combined (checked in review) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agents_md.rs#L148-L182 |
| Codex | Local memories off by default; idle only; rate-limit pause; redaction; configurable models | Confirmed | confirmed (checked in review) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/memories/write/src/start.rs#L87-L97 |
| Codex | `disable_on_external_context` keeps MCP and web chats out of generation | Confirmed; applies at generation, not reading | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/stream_events_utils.rs#L176-L192 |
| Codex | "Memory v2 default for fresh profiles" is about the store version | Confirmed | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/config/mod.rs#L3773-L3786 |
| Codex | Code Mode "under development and off by default" | Feature off, but the bundled catalog sets `code_mode_only` for most models | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/mod.rs#L66-L76 |
| Codex | Skills catalog budget: 2% of window, 8,000 chars fallback, cap 10,000 | Confirmed | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/skills/src/render.rs#L23-L27 |
| OpenCode | Compaction automatic at window minus buffer (default 15%, min 16k) | Confirmed; 16k floor only for windows of at least 32k | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L1163-L1171 |
| OpenCode | Keeps roughly last `keep.tokens` (15,000) verbatim | Serialised text, tool results cut to 1,250 characters, reasoning dropped | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/recent-text.ts#L6-L85 |
| OpenCode | Later compactions update the summary using only newer conversation | Update prompt and history-since-checkpoint | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L214-L245 |
| OpenCode | Summary request reuses the normal request's prompt cache | Same system, tools, history plus one user message | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L451-L470 |
| OpenCode | Overflow errors trigger compact-and-retry once | Once per step, plus 85/70/50/35% shrink schedule | confirmed (extended) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L126-L127 |
| OpenCode | Summary model overridable via `agents.compaction.model` | `selectModel` | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L277-L299 |
| OpenCode | `settings.compaction.type` native vs summary | `compactNatively` | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L565-L603 |
| OpenCode | Background compaction pre-writes the summary; lost on restart | In-memory `Map` | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L265-L265 |
| OpenCode | V1 tail-turn and prune behaviour ignored in V2 | No pruning in V2 | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool-output.ts#L69-L120 |
| OpenCode | Nested AGENTS.md load on read, not re-injected while in history | Ledger is synthetic messages in history; re-injected after compaction | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/instructions.ts#L100-L113 |
| OpenCode | `instructions` entries: files, globs, `~/`, URLs with 5 s timeout | As described | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/instruction.ts#L151-L179 |
| OpenCode | Keyed session instructions do not alter the system prompt | They alter the instruction block at baseline; changes become mid-conversation system messages; compaction re-bases | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/instruction-state.ts#L126-L140 |
| OpenCode | Cache warming at 90% of TTL or 4 min, 30 min window, not for subagents | As described | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/warming.ts#L36-L80 |
| OpenCode | Durable per-session log as a product feature | Off by default in the shipped CLI server | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L186-L192 |
| OpenCode | `small_model` replaced by `agents.title.model` | Per-provider family list when unset | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/model.ts#L307-L318 |
| OpenClaw | Prompt modes full, minimal (sub-agents), none | Also minimal for cron and any restricted tool allowlist | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/attempt-system-prompt-prepare.ts#L250-L255 |
| OpenClaw | Bootstrap files capped 20,000 each and 60,000 total | As stated | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-helpers/bootstrap.ts#L87-L90 |
| OpenClaw | `USER.md` capped at 4,000 | As stated | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-helpers/bootstrap.ts#L87-L90 |
| OpenClaw | Workspace files include `TOOLS.md` and `HEARTBEAT.md` | Not in the injected set | refuted | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/workspace-bootstrap-policy.ts#L30-L37 |
| OpenClaw | Sub-agents get only `AGENTS.md` | As stated | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/workspace.ts#L859-L859 |
| OpenClaw | Auto-compaction at the smaller of 75% of window or window minus reserve | As stated, plus 20k reserve floor and 25% cap | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/harness/compaction/compaction-threshold.ts#L1-L14 |
| OpenClaw | `keepRecentTokens` 20k | As stated | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sessions/settings-manager.ts#L139-L141 |
| OpenClaw | Default mode `safeguard` audits structure, pending requests, identifiers; marks degraded | Quality guard on in code; schema comment says off | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/extensions.ts#L28-L47 |
| OpenClaw | Compaction on overflow error: compact and retry | 3-attempt cap, truncation step, pre-exits | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/run/overflow-context-recovery.ts#L279-L296 |
| OpenClaw | A silent pre-compaction turn saves notes | Also triggered by 2 MiB transcript; write wrapped to one path | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/flush-plan.ts#L22-L38 |
| OpenClaw | Pruning soft-trim 1500/1500 of over 4000, hard clear near 50%, protects last three assistant turns | Hard clear also needs 50,000 eligible characters | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/embedded-agent-runner/tool-result-truncation.ts#L222-L281 |
| OpenClaw | Pruning off by default except Anthropic auth seeds `cache-ttl` 1h | As stated | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/anthropic/config-defaults.ts#L205-L222 |
| OpenClaw | Dreaming thresholds `minScore`, `minRecallCount`, `minUniqueQueries` (numbers not on page) | 0.75, 3, 3 | confirmed and filled in | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/memory-host-sdk/dreaming.ts#L37-L39 |
| OpenClaw | A rewrite dropping more than 25% of prior entries is rejected | As stated | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/extensions/memory-core/src/dreaming-consolidation.ts#L288-L293 |
| OpenClaw | Dreaming runs as a cron job, default 03:00 | As stated | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/memory-host-sdk/dreaming.ts#L22-L22 |
| OpenClaw | `memory_search` hybrid vector plus keyword | 0.7 / 0.3, MMR, 30-day decay, hard-coded | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/memory-search.ts#L162-L170 |
| OpenClaw | Summariser ~40k input cap, `maxSummaryChars` | Value not located | unsettled | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/harness/compaction/compaction.ts#L225-L296 |
| Hermes Agent | Prompt not changed mid-conversation; memory writes show only next session | Prompt, memory and plugin sections are rebuilt at every compaction | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/system_prompt.py#L829-L847 |
| Hermes Agent | Skills index tier inconsistent in docs | Volatile tier, first position | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/system_prompt.py#L798-L800 |
| Hermes Agent | Compression at 50%, floor 0.75 under 512K, per-model thresholds | As stated | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/context_compressor.py#L2751-L2755 |
| Hermes Agent | Protect the first 3 messages and a tail | Head decays to the system prompt only after the first compaction | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/context_compressor.py#L4560-L4579 |
| Hermes Agent | Summary sections: Goal, Constraints, Progress, Key Decisions, Relevant Files, Next Steps, Critical Context | Longer, different list (Historical Task Snapshot, Active State, Blocked, Errors and Fixes, and others) | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/context_compressor.py#L4024-L4078 |
| Hermes Agent | A smaller summary model drops middle turns silently | Trigger lowered to the aux window with a notice; hard reject below the minimum | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/conversation_compression.py#L1911-L1933 |
| Hermes Agent | Failure ladder 60/300/900 s then deterministic fallback | Confirmed; an abort-and-preserve branch also exists | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/context_compressor.py#L4136-L4160 |
| Hermes Agent | Compaction in place by default vs sessions page "child session" | In place is the default; child session is the legacy path | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L667-L671 |
| Hermes Agent | Manual compaction not documented | `/compress [focus]` exists, `force=True` | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/conversation_compression.py#L3966-L3980 |
| Hermes Agent | MEMORY.md 2,200 and USER.md 1,375 chars; add/patch/remove | As stated | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/agent_init.py#L1194-L1202 |
| Hermes Agent | `write_approval` stages writes; default false | As stated | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/write_approval.py#L201-L216 |
| Hermes Agent | Background review on its own configurable cheaper model | Main model by default; digest replay only when routed elsewhere | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/background_review.py#L253-L291 |
| Hermes Agent | Curator 168 h, 2 h idle, 14/30 days, consolidation off | As stated; consolidation runs on the main model unless `auxiliary.curator` is set | confirmed (model corrected) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/curator.py#L31-L35 |
| Hermes Agent | Curator hash ledger with per-entry rollback | Content-addressed blobs, not a hash chain; not tamper-evident | confirmed (meaning corrected) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/skill_ledger.py#L569-L633 |
| Hermes Agent | Lock blocks the agent incl. restricted terminal writes | Command-text parsing, not a sandbox | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/skill_lock.py#L1-L13 |
| Hermes Agent | Backups only for consolidation, keep 2 | Snapshot only for consolidation; keep count not read | confirmed (keep count unchecked) | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/curator.py#L951-L962 |
| Hermes Agent | Session search with LLM summarization (README) vs no LLM (docs) | No LLM; README stale | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/session_search_tool.py#L1-L9 |
| Hermes Agent | Context files, memory and cron prompts scanned | Regex scan; blocks repo context files, warns on SOUL.md | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/prompt_builder.py#L99-L111 |
| Oh My Pi | Compaction default order remote, snapcompact, handoff, shake, soft | As stated | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/compaction-methods.ts#L44-L51 |
| Oh My Pi | Reserve max(16,384, 15%); keepRecent 20,000; idle off | As stated | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/compaction/compaction.ts#L328-L330 |
| Oh My Pi | Snapcompact archives history as dense images "by a vision model" | Rendered locally in Rust; the vision model only reads | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-natives/src/snapcompact.rs#L1067-L1073 |
| Oh My Pi | TTSR injections survive compaction | Fire-once suppression state survives; rule text is not pinned | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L368-L374 |
| Oh My Pi | Instruction priority native > claude > agents = codex > gemini > opencode > github > agents-md; Claude project file only `.claude/CLAUDE.md` in the launch dir | Priority confirmed; a standalone `CLAUDE.md` walk-up provider (priority 10) was missing from Round 1 | confirmed (partly corrected) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude-md.ts#L24-L30 |
| Oh My Pi | Memory back ends: local, mnemopi, hindsight | A fourth, `sharpshooter`, exists | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/memory-backend/types.ts#L16-L16 |
| Oh My Pi | Hindsight retains every 3 user turns, mnemopi every 4 | As stated | confirmed (checked in review) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/hindsight/settings.ts#L173-L178 |
| Oh My Pi | Hindsight default `localhost:8888` | As stated; also an outbox with TTL and quarantine not in the documentation | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/hindsight/outbox.ts#L9-L16 |
| Oh My Pi | Sub-agents get no parent conversation | Confirmed; also `AGENTS.md` excluded by basename, other context files pass | confirmed (checked in review) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L4993-L4996 |
| Pi | Compaction triggers at window minus `reserveTokens` (16,384) | `contextTokens > contextWindow - reserveTokens` | confirmed (checked in review) | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/compaction/compaction.ts#L267-L270 |
| Pi | Keeps about `keepRecentTokens` (20,000), walking back from newest | Walk back by estimated tokens to the closest valid cut point | confirmed (checked in review) | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/compaction/compaction.ts#L804-L816 |
| Pi | Checked between tool batches and turns, before prompts, after overflow | Three check points as described | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L837-L846 |
| Pi | Overflow: compact and retry once | Exactly one attempt per run | confirmed (checked in review) | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L3165-L3202 |
| Pi | Cuts never land on tool results; oversize single turn gets two merged summaries | Same | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/compaction/compaction.ts#L351-L364 |
| Pi | Summary sections Goal, Constraints, Progress, Key Decisions, Next Steps, Critical Context, file lists | Same | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/compaction/compaction.ts#L507-L538 |
| Pi | Thinking omitted; tool results truncated to 2,000 characters when summarising | Same | confirmed (checked in review) | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/compaction/utils.ts#L94-L156 |
| Pi | `modelOverrides` set per-model thresholds | Resolution order model override, setting, default | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/settings-manager.ts#L882-L893 |
| Pi | Branch summary reserve 16,384, attached to the branch entered | Same | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/compaction/branch-summarization.ts#L320-L328 |
| Pi | 50 KB / 2,000-line limit is "expected of tools" | Enforced by every built-in tool; convention only for extension tools | corrected | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/truncate.ts#L9-L11 |
| Pi | AGENTS.md / CLAUDE.md load from agent dir, cwd and parents; `AGENTS.override.md` replaces only in the same directory; loaded without trust | Same | confirmed (checked in review) | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/resource-loader.ts#L212-L231 |
| Pi | SYSTEM.md replaces, APPEND_SYSTEM.md appends, trusted project file wins, never merged | Same | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/resource-loader.ts#L1245-L1271 |
| Pi | Default prompt plus tools under 1,000 tokens | About 2,000 characters for four tools (estimate) | confirmed as an estimate | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/system-prompt.ts#L152-L169 |
| Pi | Skills listed by name and description; full text on demand | The list also carries the file path; the model's own `read` loads the body | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/skills.ts#L358-L388 |
| Pi | `context_edit` entry exists; creation not described | Created by internal recovery omission and extension hooks, not by any UI | answered | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L1022-L1025 |
| Pi | Compaction and branch_summary entries; originals stay in the tree | Same | confirmed (checked in review) | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/session-manager.ts#L91-L116 |
| DeepSeek harness | Compaction is logged as a locked start/end sequence | Confirmed; an orphan start is released by `session/end-seed`; failure appends an `end` with `error` | confirmed (checked in review) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/region.ts#L173-L276 |
| DeepSeek harness | Prompt assembled from ordered sections and tool schemas | Central order table, per-scope shadowing; dynamic facts in a user message | confirmed; extended (new) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/system-prompt/src/index.ts#L136-L172 |
| DeepSeek harness | Round 1 asked for compaction thresholds | 0.8 ratio, 65,536 headroom, 0.16 retain | confirmed (checked in review) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/config.ts#L68-L110 |
| DeepSeek harness | Tool results over a threshold spill to a file (`maxInlineTokens`) | Value 12,500; `read` exempt; fails open | confirmed (checked in review) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/spill/spill-policy/src/index.ts#L95-L131 |
| DeepSeek harness | A SQLite full-text index over messages backs session search | In the shipped web profile it is in-memory and disabled until first use | corrected (checked in review) | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/web-app/cordis.patch.yml#L22-L30 |
| DeepSeek harness | No model-written memory feature (Round 1 notes none) | None in code | confirmed, absence confirmed | n/a (search of `packages/`) |
| goose | Compaction triggers at min(0.8 x window, 225,000); 0.0 disables | Same | confirmed (checked in review) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/context_mgmt/mod.rs#L21-L53 |
| goose | Older tool outputs summarised in the background by default (`GOOSE_TOOL_CALL_CUTOFF`) | Opt-in via undocumented `GOOSE_TOOL_PAIR_SUMMARIZATION`, in line, not background | corrected (checked in review) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/context_mgmt/mod.rs#L55-L59 |
| goose | `GOOSE_CONTEXT_STRATEGY`: summarise, truncate, clear, prompt | No code reads it | not in code (absence confirmed) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_compaction.rs#L105-L116 |
| goose | `AGENTS.md` / `.goosehints` need the Developer extension | Loaded for every session, no gating | corrected (checked in review) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/prompt_manager.rs#L90-L100 |
| goose | Precedence of AGENTS.md vs .goosehints not stated | No precedence; both concatenated, local after global | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/hints/load_hints.rs#L232-L311 |
| goose | Nested hint files from cwd up to git root; subdirectory files when touched | Same, with the mechanism (replay of tool requests) | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/hints/load_hints.rs#L26-L103 |
| goose | MOIM env vars re-injected each turn, 64 KB cap | Persisted per-turn message, reused within the turn; 65,536 bytes | confirmed with nuance | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/tom.rs#L65-L88 |
| goose | "Top of Mind" equals MOIM (unverified) | `tom` is the MOIM source | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/tom.rs#L14-L16 |
| goose | Memory extension "remembers preferences" (no mechanism) | Text files per category, local `.goose/memory`, global config dir; global loaded into instructions | settled (checked in review) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-mcp/src/memory/mod.rs#L118-L172 |
| goose | Templates overridable from `~/.config/goose/prompts/` | Confirmed | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/prompt_template.rs#L106-L132 |
| goose | "Full history stays visible but only the compacted version is active context" | Old rows kept with `agent_visible=false` | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/context_mgmt/mod.rs#L187-L239 |
| Cline | Auto Compact summarises the whole conversation and replaces history | Prefix summary; recent tail and last user turn kept; sidecar, not replacement | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/context/agentic-compaction.ts#L52-L90 |
| Cline | Falls back to rule-based truncation for models lacking support | Falls back to `basic` when the summariser throws or is skipped | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/context/compaction.ts#L568-L585 |
| Cline | No compaction threshold documented | Threshold is 0.9 of the input budget (about 81% of window when only the window is known) | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/context/compaction-shared.ts#L19-L19 |
| Cline | `--compaction agentic\|basic\|off`, default agentic | CLI enables it; core is opt-in | confirmed | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/cli/src/utils/compaction-mode.ts#L33-L45 |
| Cline | `/newtask` starts a fresh context with a packaged hand-off | Alias of `/compact`, same session | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/shared/slashCommands.ts#L8-L20 |
| Cline | Model-request retries reduced from 5 to 3 | Value 3 confirmed: 3 retries after first attempt, retryable classes only | confirmed as a value | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/agents/src/agent-runtime.ts#L87-L91 |
| Cline | Conditional rules (frontmatter `paths`) | Evaluator in legacy VS Code module only; not on the SDK path | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/apps/vscode/src/core/context/instructions/user-instructions/rule-conditionals.ts#L1-L10 |
| Cline | Focus Chain (todo list kept in context) | Legacy reader only; agenda tool compiled off | not in code | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hub/server/hub-server-transport.ts#L144-L152 |
| OpenHands | Condenser defaults 120 / 4 / 60 | Client default `max_size` 240; planner 100 / keep_first 6; server default not in this repo | corrected (checked in review) | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/services/settings.ts#L16-L17 |
| OpenHands | Skills with triggers | Keyword triggers built from the catalog; matching is in the SDK | confirmed (unchecked) | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L866-L905 |
| OpenHands | Microagents as a feature | Only a legacy path marker; renamed to skills | not in code | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/utils/skill-scope.ts#L11-L19 |
| OpenHands | Hook config file location | `<workspace>/.openhands/hooks.json` read through the Agent Server, no trust prompt, fails open | answered (checked in review) | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/hooks-service.ts#L17-L58 |
| Zed | `auto_compact` default on, 90%, three threshold forms | Same | confirmed | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/assets/settings/default.json#L1294-L1306 |
| Zed | Separate `compaction_model`; banner under 80k | Confirmed; auto-compaction is also off below 80k | confirmed | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/thread.rs#L4588-L4600 |
| Zed | (not documented) | 80,000-byte user carry-forward, tool-less cached-prefix request, no reactive overflow recovery | new | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/thread.rs#L5286-L5315 |
| Warp | Round 1 could not say where the loop (and so the prompt) runs | The loop, prompt and summarisation run on Warp's server; the client sends one protobuf request per turn | new (checked in review, absence confirmed) | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api/impl.rs#L65-L121 |
| Warp | (not documented) MCP tool lists | Every connected server's tools sent every turn; no deferral in the client | new (unchecked) | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent/api.rs#L267-L345 |
| OpenCodex | 32 MiB native-compaction body ceiling | `COMPACT_RESPONSE_MAX_BYTES` 32 MiB (the 4 MiB value is the SSE frame cap) | confirmed (unchecked in review) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/responses/compact.ts#L243-L243 |
| OpenCodex | Routed compaction is a tool-free summariser | Tool-free one-shot turn that must return exactly one non-empty summary | confirmed (unchecked in review) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/responses/compaction-summary.ts#L6-L45 |
| OpenCodex | Hop errors are 401/403/404/408/429/5xx; context overflow moves on without cooling | Also 402, 425 and many code and prose cases; overflow hops with cooldown scope none | corrected (unchecked in review) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/combos/failover.ts#L1177-L1230 |

### Edit tools and the mechanics behind the scenes

From [round2/synth/08-edit-tools-mechanics.md](round2/synth/08-edit-tools-mechanics.md).

| Project | What the documentation said (Round 1) | What the code shows | Verdict | Permalink |
|---|---|---|---|---|
| Oh My Pi | Every line is tagged with a short content hash (`2:f1|...`) | One 16-bit xxh32 tag per file; lines are bare numbers | Refuted | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/store.rs#L70-L91 |
| Oh My Pi | A changed file means the edit is rejected before corruption | Rebased when every anchor maps by the same offset with neighbour and syntax checks; offset-0 recovery applies to the new text with a warning | Corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/recovery.rs#L503-L565 |
| Oh My Pi | Hashline is the edit format; `sloppy` exists | Five modes; hashline is the default but is replaced by `replace` for eight model families; `sloppy` confirmed | Corrected / confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/utils/edit-mode.ts#L24-L49 |
| Oh My Pi | The `replace` prompt says edit before read is an error | No read check in the `replace` engine | Not in code (absence confirmed) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/replace.rs#L177-L195 |
| Oh My Pi | TTSR aborts, injects, regenerates; fire-once default | Confirmed; default scope excludes thinking; no retry counter | Confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/ttsr.ts#L137-L146 |
| Oh My Pi | TTSR injections survive compaction | Suppression state survives; rule text is not pinned | Corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L368-L374 |
| Oh My Pi | Embedded persistent bash (Rust) | Persistent native shell per key, fork for concurrent calls, SIGTERM-walk-SIGKILL kill | Confirmed, with detail | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-shell/src/proctree.rs#L843-L884 |
| Codex | `apply_patch` preserves CRLF since 0.162.0 | First line ending seen for inserted lines; context lines verbatim; adds a trailing newline | Confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/apply-patch/src/text_file.rs#L35-L109 |
| Codex | Parallel read-only tools (0.163.0) | Per-tool opt-in to a read lock; shell is parallel, `apply_patch` is exclusive | Corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/parallel.rs#L292-L304 |
| Codex | Plan mode (`/plan`) | Prompt-enforced; `apply_patch` still runs in plan mode if the sandbox allows it | Confirmed (no enforcement) | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/handlers/plan.rs#L87-L91 |
| OpenCode | `edit` is exact match; errors show closest lines | Three tiers (exact, Unicode, trailing-ws lines); hint is a line vote, gated on read permission; no staleness check | Corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/edit.ts#L168-L188 |
| OpenCode | `patch` only for supported GPT models | Confirmed (`gpt-` in id, not `oss`, not `gpt-4`) | Confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/patch.ts#L288-L298 |
| OpenCode | Formatters are off by default (config `formatter`) | A formatter runs after write in `commit`; the config gate was not re-read in the Round 2 files | Not settled | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/file-write.ts#L83-L90 |
| OpenCode | Snapshots: separate Git store, 2 MiB untracked limit | Confirmed; cost guards and no retention are new | Confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/snapshot.ts#L133-L148 |
| OpenCode | Shell default 2 min; background none | Confirmed by the claims table | Confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/shell.ts#L229 |
| DeepSeek | Read-before-edit: `FS_NOT_OBSERVED` / `FS_STALE_VERSION`; plugin optional | True but the shipped base bundle disables it; prompt still asks for a read | Corrected | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L292-L294 |
| DeepSeek | Alternative `str_replace_editor` (view, create, replace, insert) | Not located in `tool-fs`; `apply_patch` exists but is off | Not found | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/tool-fs/src/index.ts#L50-L55 |
| DeepSeek | Spill policy `maxInlineTokens` | 12,500; `read` exempt; failure keeps content inline | Confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/bundle/base/cordis.patch.yml#L419-L425 |
| Hermes | `patch` does fuzzy matching and returns a diff | Fuzzy confirmed (nine strategies); the diff output was not traced | Confirmed in part | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/fuzzy_match.py#L289-L304 |
| Hermes | After each edit: syntax check, then LSP; only new errors | Lint output diffed against pre-edit lint (new errors only); LSP layer not read | Confirmed in part | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/file_operations_lint.py#L222-L243 |
| Hermes | An interrupt moves foreground terminal commands to the background | Promotion happens for an over-cap timeout (600 s) or a trailing `&`; the interrupt path was not settled | Corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/terminal_tool.py#L1240-L1266 |
| Hermes | Checkpoints: triggers, caps, `/rollback` skips user edits | Confirmed; "never `/` or `$HOME`" is also every ancestor of home; the skip ledger sees only file-tool writes | Corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L986-L1050 |
| Cline | Shadow git repository, separate from the project's | Stash commits in the user's own repository under private refs; no shadow repository | Refuted | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/checkpoint-hooks.ts#L535-L649 |
| Cline | Checkpoints work on non-git folders | Built-in snapshotter returns nothing outside git; a host callback can bypass | Corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/checkpoint-hooks.ts#L538-L544 |
| Cline | `run_commands`: the model sets `requires_approval` | No such field in any current code path | Refuted (checked in review; the docs page still describes the old flag) | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/schemas.ts#L133-L157 |
| Cline | A notification fires after 30 s for auto-approved commands | The 30 s is the default timeout; no notification found in this path | Not found | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/executors/bash.ts#L837-L843 |
| Cline | Watches long-running processes (dev servers) | Only a human can detach a running command; the model cannot | Confirmed, with mechanism | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/executors/bash.ts#L850-L897 |
| Cline | Plan mode is not a security boundary | Confirmed; MCP and web fetch also stay open in plan mode | Confirmed | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/command-guard.ts#L10-L17 |
| goose | `GOOSE_TERMINAL=1` and `AGENT=goose` set when goose runs commands | Set only by the recipe-retry check runner, not the Developer shell | Corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/retry.rs#L51-L64 |
| goose | Developer tool names not documented; hooks `BeforeReadFile` | Five tools, no read tool, so `BeforeReadFile` never fires | Corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L117-L124 |
| goose | `GOOSE_MAX_TOOL_RESPONSE_SIZE` default 200,000 chars to a temp file | Generic spill at 200,000 characters confirmed; the shell has its own 2,000-line / 50,000-byte cap | Confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L161-L166 |
| Pi | Steering interrupts remaining tools | Steering is polled after the whole batch; abort is the only interrupt | Corrected | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L272-L304 |
| Pi | 50 KB / 2,000-line limit is expected of tools | Enforced by each built-in tool; convention only for extension tools | Corrected | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/truncate.ts#L9-L11 |
| Pi | `withFileMutationQueue` is an extension-author recommendation | Built-in `edit` and `write` also use it | Confirmed and extended | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/edit.ts#L158-L158 |
| Zed | Per-prompt Git checkpoints removed in 1.27.0 | No per-prompt checkpoint code anywhere; only thread-archive checkpoints | Confirmed | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/git/src/repository.rs#L1075-L1088 |
| Zed | Undo is by rejecting edits or using Git | Rewind rejects all unreviewed edits in the thread; the log is memory only | Corrected | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/acp_thread/src/acp_thread.rs#L8028-L8070 |
| Zed | Review Changes: per-hunk accept or reject | Confirmed | Confirmed | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/action_log/src/action_log.rs#L641-L710 |
| Warp | Four permission levels incl. "Never"; read-files "Agent decides" | Three levels plus Unknown; read-files "Agent decides" always reads without asking | Corrected | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L746-L750 |
| OpenCodex | Read-only default; write needs a command-sandbox probe | The probe gates `exec`; `write_file` is offered without it (root-confined and CAS) | Corrected | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-command-runner.ts#L706-L747 |
| OpenHands | `file_editor` commands incl. `undo_edit` | Not in this repository | Not in code read | (no permalink: the file states the engine is in another repository) |

### Sub-agents, orchestration, model roles, fallback and retry

From [round2/synth/09-subagents-models-retry.md](round2/synth/09-subagents-models-retry.md).

| Project | What the documentation said (Round 1) | What the code shows | Verdict | Permalink |
|---|---|---|---|---|
| Codex | Explorer is a read-only agent | `explorer.toml` is 0 bytes; no tool or sandbox restriction | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/role.rs#L316-L376 |
| Codex | A custom agent cannot override sandbox, approval, provider, MCP servers | Closed override struct; parent policy re-applied last | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/child_config.rs#L197-L218 |
| Codex | Subagents inherit the parent's sandbox and approvals | Confirmed (live profile re-applied) | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/child_config.rs#L97-L97 |
| Codex | Multi-agent is stable and on by default; no default documented for the thread cap | V1 flag is Deprecated (default on); V2 chosen by the model catalog; caps 6 (V1), 4 (V2); depth default 1 | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/config/mod.rs#L1741-L1758 |
| Codex | Retries honour `Retry-After`; WebSocket falls back to HTTP; model-not-found stops retries | All three confirmed; `Retry-After` never extends the count | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/responses_retry.rs#L58-L167 |
| Codex | Independent read-only tools run in parallel | Per-tool opt-in to a shared lock, not a read-only classification | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/parallel.rs#L292-L304 |
| OpenCode | Subagent children use their own permissions | Agent rules plus the parent session's rules | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/context.ts#L142-L146 |
| OpenCode | `explore` is read-only; plan denies edits | `explore` allows shell; plan denies only edit tools, shell and `subagent` open | refuted as enforced (prompt-only for shell) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L74-L93 |
| OpenCode | `general` cannot launch subagents; depth default one | Both confirmed (`subagent: deny`; depth `?? 1`) | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool/plugin/subagent.ts#L123-L139 |
| OpenCode | Hidden `summary` agent is a working maintenance agent | No consumer found in core, server, cli, tui | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L110-L116 |
| OpenCode | On the final step tools are removed | Tools kept, `toolChoice: none`, calls refused | corrected | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/llm.ts#L259-L281 |
| OpenCode | (Round 1 synthesis) per-agent fallback chains with a `model_fallback` event | Not true of OpenCode: none exists | refuted | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/runner/model.ts#L81-L121 |
| OpenClaw | Default sub-agent concurrency 8 per spawning session | Confirmed (per-owner lanes of width 8); depth 5 and 5 children now filled in | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/child-admission.ts#L84-L109 |
| OpenClaw | Billing failures disable a credential for 10 minutes | First step only; doubles to a 24-hour cap; `auth_permanent` has its own ladder | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/usage-failure-state.ts#L18-L46 |
| OpenClaw | Rate-limit cooldown follows provider headers | Confirmed; the only rule for rate limits | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/auth-profiles/usage-failure-state.ts#L80-L108 |
| OpenClaw | Next turns prefer the working fallback for up to 30 minutes | In memory only; 10 minutes idle plus 30-minute cap | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/model-fallback-sticky.ts#L12-L66 |
| OpenClaw | Sub-agents get only `AGENTS.md` and a minimal prompt; fork shares the transcript | Confirmed | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/subagents/spawn/subagent-spawn-context.ts#L195-L216 |
| Hermes Agent | Parent iteration budget 500; delegation default 50 or 250 left open | Parent unlimited by default; children 250; 50 is stale text (and a 50-per-turn spawn guard) | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/delegate_tool.py#L106-L106 |
| Hermes Agent | Fallback on 429 and 5xx after retries, 401/403/404 immediately | 429 and billing are eager; 5xx and connection errors after 2 retries; 401/403 after credential refresh | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/turn_recovery.py#L1897-L1934 |
| Hermes Agent | Children cannot use clarify, memory, send_message, cronjob; 10 concurrent, depth 1, worktree off | Confirmed | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/delegate_tool_toolsets.py#L14-L123 |
| Hermes Agent | Per-task auxiliary models default to the main model, each with a fallback | Confirmed; full slot list and route-as-a-unit rule new | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L712-L788 |
| Oh My Pi | Ten roles (docs) including `designer`; nine (README) | 16 built-in roles, no `designer` | refuted | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/model-roles.ts#L56-L91 |
| Oh My Pi | Bundled agents include `designer` and `librarian` | Six: scout, reviewer, security-reviewer, seance, task, sonic | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/agents.ts#L45-L74 |
| Oh My Pi | Sub-agents run in `yolo`; depth 2 | Confirmed | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L1259-L1278 |
| Oh My Pi | Isolation setting `task.isolation.mode` | The setting is `task.isolation.enabled` | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/settings.ts#L27-L79 |
| Oh My Pi | "Schema-validated typed results" | Validated against the agent's or caller's `outputSchema` through `yield` | corrected (unchecked) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/prompts/system/subagent-system-prompt.md#L54-L75 |
| Pi | 3 retries, 2,000 ms base, 60,000 ms maximum, provider retries 0 | Confirmed; "60,000" names two caps; agent layer has no jitter and ignores `Retry-After`; provider layer inert at 0 | confirmed with correction | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/settings-defaults.ts#L18-L26 |
| Pi | Sub-agents and plan mode are skipped by design | Confirmed; example extension spawns `pi` processes | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/README.md#L19-L19 |
| DeepSeek harness | Team tools disabled in the shipped base bundle | Base mounts them enabled; the per-session Team mode is off | corrected | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/team/agent-team/src/index.ts#L46-L112 |
| DeepSeek harness | Sub-agent permission inheritance undocumented | In-process: sandbox override copied, approval pinned `never` | settled by code | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/subagent/subagent/src/child-agent.ts#L249-L256 |
| goose | Isolated subagents spawned via `delegate`; failed or timed-out subagents return nothing | Child session plus pipeline step; failure returns `Subagent X failed: <reason>` | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/subagent.rs#L82-L101 |
| goose | Delegation disabled in non-autonomous mode; default timeout 5 minutes | No mode check; no sub-agent timeout (300 s is the shell and extension default) | refuted | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/summon.rs#L866-L876 |
| goose | Max 5 concurrent, no recursion | Confirmed | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine_builder.rs#L157-L163 |
| goose | No generic retry or fallback documented (Bedrock, Databricks have retry env vars) | A generic retry exists in the provider layer (3, 1 s doubling to 30 s); no fallback | refuted (retry); confirmed (no fallback) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-provider-types/src/retry.rs#L14-L17 |
| goose | `--max-tool-repetitions` is a loop guard | The flag is not wired; the inspector is registered with no limit | refuted | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_monitor.rs#L99-L134 |
| goose | Four ACP providers; modes mapped | Confirmed; a fifth (`copilot-acp`) exists; Pi's mode is not translated | confirmed with correction | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/providers/pi_acp.rs#L69-L81 |
| Cline | Approving `spawn_agent` approves everything the subagent does; no documented caps | Confirmed, stronger: policies never reach the child; no caps in code | confirmed | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/team/configured-agent-tool.ts#L162-L165 |
| Cline | `--retries` is the number of consecutive mistakes allowed (default 3) | True for the CLI; SDK default 6; same counter fed by the loop detector | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/orchestration/session-runtime-orchestrator.ts#L474-L474 |
| Cline | Three model-request retries | Confirmed (1 s, 2 s, 4 s; cap 15 s; no jitter) | confirmed | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/agents/src/agent-runtime.ts#L72-L91 |
| OpenHands | Launch inputs validated for child conversations | Validated for argument shape only; no approval, depth limit or policy inheritance in the client | corrected | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/services/child-conversation-launch.ts#L511-L551 |
| OpenHands | DelegateTool "retired" | No `DelegateTool` in the tree; `task_tool_set` and `workflow_tool_set` exist; no evidence of a one-to-one replacement | partly confirmed | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/constants/profile-tools.ts#L38-L46 |
| OpenHands | How ACP agent permission requests reach the user | Not in this repository (a test fixture shows an ACP session-mode field, effect unknown) | not in code | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/docs/ACP_AGENTS.md#L8-L31 |
| Zed | Deny and confirm rules take precedence over external agents' own permissions | Comment says so; no code consults `tool_permissions` for ACP agents | corrected (absence confirmed) | https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent_servers/src/acp/v1.rs#L10172-L10225 |
| Warp | ACP support appears only in changelog wording | Warp implements an ACP client for cloud and headless runs (Claude, Codex, Gemini, Pi) | corrected | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/acp/bridge.rs#L1-L22 |
| Warp | Orchestration is one level deep | A feature gate (`MultiLevelOrchestration`, dogfood flag), not a structural limit | corrected | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/warp_features/src/lib.rs#L1079-L1079 |
| OpenCodex | Cooldowns are memory-only (implied) | Persisted to `combo-cooldowns.json` | refuted (checked in review) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/combos/cooldown-store.ts#L1-L25 |
| OpenCodex | Hop errors are 401/403/404/408/429/5xx; context overflow does not cool | Also hops on 402, 425 and many code cases; context overflow hops with cooldown scope none | corrected (unchecked) | https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/combos/failover.ts#L1177-L1230 |

### Extensibility and unattended operation

From [round2/synth/10-extensibility-unattended.md](round2/synth/10-extensibility-unattended.md).

| Project | What the documentation said | What the code shows | Verdict | Permalink |
|---|---|---|---|---|
| Codex | MCP tools can be routed direct, deferred or code_mode per server | default choice is per **model**; per-server `omit_tools_from` is subtractive | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/mcp_tool_exposure.rs#L136-L170 |
| Codex | Code Mode is under development and off by default | feature flag is off, but the bundled catalog sets `code_mode_only` for most models | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/mod.rs#L66-L76 |
| Codex | MCP schema compaction default 5,000 bytes | confirmed | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/tools/src/json_schema/compaction.rs#L15-L15 |
| Codex | Codex MCP server removed | confirmed | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/cli/src/main.rs#L166-L175 |
| Codex | hooks trusted by hash; a changed hook needs review again | true for the declaration and plugin bundles; **not** for a script a non-plugin hook references | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/engine/discovery.rs#L972-L998 |
| Codex | PreToolUse hooks can deny | confirmed; failure fails open (not documented) | confirmed, new detail | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/events/pre_tool_use.rs#L211-L300 |
| Codex | skills from `.agents/skills`, `~/.agents/skills`, `/etc/codex/skills` | first two confirmed; Admin root is the System-layer config dir (path not verified) | confirmed in part | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/skills/src/host_roots.rs#L127-L190 |
| Codex | scheduling does not exist | none in the engine, but a desktop automation flag and thread source exist | corrected | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/ext/queue/src/service.rs#L90-L105 |
| Codex | `codex exec` has approvals forced off | default `Never`; approval requests rejected | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/exec/src/lib.rs#L622-L640 |
| Codex | project trust gates config, hooks and exec policies | confirmed | confirmed | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/config/src/loader/mod.rs#L1100-L1170 |
| OpenCode | denying `execute` removes Code Mode and the tools reachable only through it | tools become direct tools | **refuted** | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/tool.ts#L265-L270 |
| OpenCode | nested tools in Code Mode enforce their own permissions | confirmed | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/tool.ts#L126-L145 |
| OpenCode | Code Mode has no filesystem, imports or timers, only `fetch` | confirmed (own interpreter) | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/codemode/tool.ts#L278-L290 |
| OpenCode | MCP timeouts 30 s / 30 s / 12 h; `pool: true` | confirmed (an attached-mode 10 minute timeout also exists) | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/mcp/client.ts#L37-L39 |
| OpenCode | `permission.evaluate` can change allow or ask but not a configured deny | confirmed | confirmed | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/plugin/src/effect/permission.ts#L10-L19 |
| OpenCode | command `` !`cmd` `` runs outside the permission flow | confirmed, and runs on user-typed arguments | confirmed, extended | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/command.ts#L190-L215 |
| OpenCode | plugins run unsandboxed | confirmed; no trust prompt for project plugins | confirmed, new detail | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/config/plugin/source.ts#L125-L136 |
| OpenClaw | interrupted time-scheduled agent-turn jobs get one retry | conditional retry, no counter | corrected | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/cron/store/run-recovery.kernel.ts#L211-L226 |
| OpenClaw | heartbeat replaced by a 30-minute proactive job | confirmed | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/cron/default-proactive-job.ts#L30-L50 |
| OpenClaw | webhook token via header, query rejected, 429 on failures | confirmed; idempotency cache is in memory only | confirmed, new detail | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/hooks-request-handler.ts#L220-L256 |
| OpenClaw | skill load precedence workspace, `.agents`, personal, managed, workshop, bundled, extra | confirmed (personal only on the default state dir) | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/workspace-skill-sources.ts#L134-L186 |
| OpenClaw | skill gating by `metadata.openclaw` `requires.bins/env/config`, `os` | confirmed | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/config.ts#L105-L141 |
| OpenClaw | `before_tool_call` hooks can intercept and block | confirmed; failure also blocks | confirmed, new detail | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.before-tool-call.policy.ts#L397-L408 |
| Hermes | cron jobs in `jobs.json`, history in `executions.db`, 60 s tick | confirmed; at-most-once advance and headless refusal are new | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/jobs.py#L95 |
| Hermes | dangerous commands denied in cron by default | confirmed (`cron_mode: deny`) | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L1658-L1662 |
| Hermes | cron cannot create jobs by default | true, but loop prevention, not a security boundary | confirmed with caveat | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/cron/scheduler.py#L418-L439 |
| Hermes | headless modes default deny | true for cron, `-q`, API and webhooks; `-z` and the bare library context auto-approve | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1300-L1308 |
| Hermes | MCP include and exclude globs; stdio servers get a safe env baseline | confirmed; the description scan has no blocking effect | confirmed, new detail | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_registration.py#L247-L262 |
| Hermes | tool search bridge (`tool_search`, `tool_describe`, `tool_call`) | confirmed | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/tool_search.py#L1-L8 |
| Hermes | plugin isolation optional, in-process by default | confirmed | confirmed | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/hermes_cli/config_defaults.py#L1715 |
| Hermes | background review runs on its own cheaper model | main model by default | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/background_review.py#L253-L291 |
| Hermes | per-call flow: pre hook, dangerous check, execute, post hook | the dangerous check is inside the terminal tool; only `pre_tool_call` can block | corrected | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_executor.py#L679-L752 |
| Oh My Pi | hooks live in `.omp/hooks/` (pre and post directories) or the user dir | also imported from project `.claude/hooks/` and Codex and plugin dirs as executable modules | corrected | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L377-L396 |
| Oh My Pi | `tool_call` hooks fail closed on error or timeout, 30 s | confirmed | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L2338-L2364 |
| Oh My Pi | hooks cannot bypass approval | confirmed (approval re-resolved on rewritten input) | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L355-L374 |
| Oh My Pi | undeclared custom tools are exec; MCP tools write | confirmed | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/tool-bridge.ts#L733-L735 |
| Oh My Pi | project MCP config and extensions load with no trust prompt | confirmed; `isProjectTrusted` hard-coded true | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L1814-L1814 |
| Oh My Pi | `xd://` on-demand tool catalog | confirmed (default on, docs mode `catalog`) | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L1014-L1040 |
| Oh My Pi | marketplace `autoUpdate` `notify` only logs at debug; `auto` upgrades unprompted | confirmed | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/plugins/marketplace-auto-update.ts#L36-L48 |
| Oh My Pi | no scheduling | confirmed (no scheduler in code) | confirmed | search of `src` for cron and schedule |
| Pi | MCP SSE not supported | confirmed (legacy SSE explicitly rejected) | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/mcp/src/config.ts#L124-L125 |
| Pi | codemode default contradiction ("off by default" vs `mode: on`) | tool inactive; `mode` is presentation; MCP auto-activates it | corrected | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/extensions/codemode/index.ts#L26-L45 |
| Pi | MCP exposure modes `codemode` (default), `deferred`, `direct`, `hidden` | confirmed; `codemode` is core-`deferred` internally | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/extensions/mcp/tools.ts#L46-L48 |
| Pi | MCP calls pass through the normal tool pipeline | confirmed, including nested codemode calls | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/agent-session.ts#L719-L742 |
| Pi | a failing `tool_call` handler blocks the call | confirmed | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/extensions/runner.ts#L1256-L1274 |
| Pi | project trust order and headless behaviour; `ask` default | confirmed | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/project-trust.ts#L47-L97 |
| Pi | packages: npm or git, no vetting | confirmed; scripts not disabled | confirmed | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/package-manager.ts#L2026-L2048 |
| DeepSeek | MCP stdio and streamable-http, `mcp__<server>__<tool>`, reconnect | confirmed, with a hashed-suffix fallback | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/mcp/mcp-client/src/tools.ts#L91-L97 |
| DeepSeek | MCP OAuth and approval undocumented | settled: no OAuth, no approval hook in MCP code | settled | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/mcp/mcp-client/src/tools.ts#L126-L195 |
| DeepSeek | GitHub webhook: signed, no dedupe, 202 before work | confirmed | confirmed | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/experimental/webhook-github/src/handler.ts#L99-L120 |
| DeepSeek | approval resolution is a stage for every tool call | reached only when a plugin returns `ask`; none does by default | corrected | https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/tools/src/pipeline.ts#L197-L206 |
| goose | scheduler: cron for recipes, `--enable-scheduler` | confirmed; no catch-up, overlap allowed, forced Auto | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/scheduler/full.rs#L142-L183 |
| goose | tool deferral: lazy MCP schemas via an env switch | names only, derived from the conversation, off under Code Mode | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L344-L352 |
| goose | hooks fail allow by default | confirmed (`on_failure: block` covers timeouts, PreToolUse only) | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/hooks/mod.rs#L185-L191 |
| goose | malware check on external extensions | confirmed (OSV, fails open) | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_malware_check.rs#L44-L58 |
| goose | extension allow-list requires an exact command match | prefix match, desktop deeplink only | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/ExtensionInstallModal.tsx#L177-L204 |
| goose | deeplink command list `jbang, npx, uvx, goosed, docker` | `cu, docker, jbang, npx, uvx, goose, npx.cmd` | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/settings/extensions/deeplink.ts#L15-L59 |
| goose | session import is desktop only | CLI imports too; Claude Code, Codex, Pi formats | corrected | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/session/import_formats/mod.rs#L150-L157 |
| goose | Code Mode per-tool permission enforcement (open question) | inner calls skip approval, inspectors and hooks | settled | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L100-L146 |
| Cline | schedules default to yolo | confirmed | confirmed | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/cron/runner/cron-runner.ts#L728-L750 |
| Cline | streamable HTTP recommended, SSE fallback if `type` omitted | no fallback; omitted means SSE | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/mcp/config-loader.ts#L172-L206 |
| Cline | MCP entries have `autoApprove` tool lists | no such field in the SDK schema | not in code | no line cited (absence in the SDK settings schema) |
| Cline | `beforeTool` can skip, rewrite, force approval; 3 s timeout; VS Code needs "Enable Hooks" | true for in-process hooks only; file hooks 120 s or 30 s, can only cancel (which stops the run), all failures allow; VS Code hooks default on | corrected | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/hook-file-hooks.ts#L415-L454 |
| Cline | `CLINE_PLUGIN_MODE=sandbox` restricts plugins | crash isolation only | confirmed as non-restriction | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/runtime/tools/subprocess-sandbox.ts#L308-L327 |
| Cline | model sets `requires_approval` | no such field in the SDK | refuted | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/schemas.ts#L127-L157 |
| OpenHands | automations have cron and event triggers with JMESPath filters | confirmed; `continue_conversation` destination is new | confirmed | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/types/automation.ts#L1-L28 |
| OpenHands | where the hook config file lives | `<workspace>/.openhands/hooks.json`; no trust prompt | answered | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/hooks-service.ts#L17-L58 |
| OpenHands | Canvas Apps: no sandbox for app code | confirmed | confirmed | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/extensions/canvas-extension-module-loader.ts#L8-L42 |
| Warp | ACP support appears only in changelog wording | ACP client for headless and cloud runs; Claude, Codex, Gemini, Pi driven | corrected | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/agent_sdk/driver/harness/acp/mod.rs#L880-L891 |
| Warp | MCP credentials stored locally | confirmed (secure storage); stdio servers inherit the full environment | confirmed | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/mcp/src/oauth.rs#L113-L190 |

## C. For rizzy-harness: the full recommendations, per feature group

### The agent loop, tool dispatch and durability

From [round2/synth/01-loop-dispatch-durability.md](round2/synth/01-loop-dispatch-durability.md).

Our state, from `research/round1/own-rizzy-harness.md` (as of `main`): one native loop in `crates/agent-native/src/lib.rs`; a fixed cap of 50 model calls per turn, not configurable; tool calls run one after another (only `delegate` runs in parallel); a prompt sent while a turn runs is rejected ("a turn is already running", `crates/node/src/session.rs:297-299`); a restart mid-turn closes the turn as `interrupted`, denies pending approvals and closes interrupted tool calls synthetically (`session.rs:154-191`); 600 s per tool call; no loop detector; no provider retry (pull request 7 adds it); cancel does not abandon a tool future that ignores the token (fixed only on `integration/gate3`); routines run agentic only in `ask` or `ask_all`. Feature freeze applies until Gate 3 of `FIX_PLAN.md`: items marked "fix" below are repairs of the loop's safety; items marked "feature" cost against the plan and need the owner's decision to break the freeze.

Ranked by importance. Each point is stated as a verdict.

##### 6.1 Do: make the order "append, flush, then act" a tested rule (fix)

- **Copy from DeepSeek harness** (`packages/session/session-checkpoint-policy/src/index.ts`, `packages/core/agent-loop/src/index.ts` `interruptedTurnClosers` in `packages/core/session/src/repair.ts`): an awaited, fail-closed flush before each model request and before each top-level tool body; a failed flush means the tool never runs.
- **Check in our code:** is the event that records "tool call X with arguments A is about to run" committed (not just sent down the node link) before the tool body starts? Our log is written by the node's session actor; the master persists via the link. Name the exact point (the `approval_requested` or the tool-start event in `session.rs` around `:955-1012` and the loop call site `agent-native/src/lib.rs:744-870`). If the append is queued behind the back-pressure channel (pull request 4's bounded link) the flush rule is false.
- **Test that fails first:** kill -9 the node while a `bash` call is running; assert the log holds the call's arguments and, after restart, a closing event.
- **Closing wording:** copy DeepSeek's synthetic result ("outcome is unknown ... retry only if read-only or idempotent ... do not retry blindly") and OpenClaw's restart text (check files, processes and external actions before repeating). Our synthetic close already exists; check that it says "unknown", not "cancelled".
- **Avoid:** Cline's whole-file transcript rewrite with no atomic rename, and Pi's "persist only on `message_end`" (a crash loses the partial assistant message and leaves a dangling call). Our append-only events are ahead of both.

##### 6.2 Do: a default-on repeated-call guard, checked before dispatch (fix)

Evidence that the gap is real: Codex and Pi have none and say so; goose's is dead code; OpenCode has none outside `/goal`.
- **Copy from Hermes** (`agent/tool_guardrails.py`): tool plus arguments hash, a result hash (so a changing timestamp does not hide a loop, as OpenClaw masks volatile tokens in `tool-loop-detection.ts`), warn at the 3rd repeat, block at 5, and `hard_stop` on for routines and off for attended chats. Hash arguments so the guard cannot leak them.
- **Copy from OpenClaw** (`tool-loop-admission.ts`): decide at admission, before the call runs; if the guard itself errors, block. Cline runs its detector from `tool-started`, after the call was issued: do not do that.
- **Copy from Cline** (`mistake-tracker.ts`): a turn where every tool call failed is a "mistake"; any success resets it.
- **Where the state lives:** derive it from the event log fold, not an object local to the loop (goose's `RepetitionInspector` counts on a clone and never accumulates). The node can then rebuild it after a restart.
- **Do not copy** DeepSeek's notice-only reminder as the only guard: it never stops a loop. Add it as the warning stage, with the block after it.
- **Test:** a scripted provider issuing the same failing `read` call; assert the 5th call is refused with a visible reason and the 3rd carried a warning.

##### 6.3 Do: step, time and tool limits in `node.toml`, enforced in the dispatcher (fix)

- The cap of 50 is hard-coded; the node is the authority, so move the cap and a per-turn wall-clock into `node.toml` `[limits]` (today only `tool_stream_bytes`) with defaults, as Hermes does for routines (a finite default per routine) and goose does with `GOOSE_MAX_TURNS` plus a half-budget `<turn-budget>` hint.
- **Copy from OpenCode** (`session/runner/llm.ts`, `step.ts`): at the cap, keep tool schemas in the request, set `tool_choice: none`, append an instruction, and refuse any late tool call at dispatch. It keeps the provider cache and is enforced, not requested.
- **Enforce each tool's deadline in the dispatcher, once** (Cline's `timeoutMs` metadata is never read; goose lets the model set `timeout_secs` and 0 disables). Clamp any model-supplied timeout to a `node.toml` maximum; never let 0 mean infinite. Our bash cap of 3600 s is the model-visible ceiling: check it is a node setting.
- Prefer idle limits to total limits for the provider stream (OpenClaw 120 s, Codex 5 min, OpenCodex 300 s), which pull request 7 should set.

##### 6.4 Do: never run tool calls from a response that stopped on the length limit (fix)

Pi fails every tool call in a `length`-stopped message ("was not executed: the response hit the output token limit, so its arguments may be truncated") because streamed arguments are completed by a JSON salvage parser (`packages/agent/src/agent-loop.ts` L273-L279, L488-L513, checked in review). OpenCode separately fails the step after three consecutive `length` stops. A truncated `write` or `bash` argument is a data-loss and safety hazard.
- **Check in our code:** how does `agent-native` assemble arguments from stream chunks, and what does it do with a `length` finish? Test: a provider fixture ending a `write` mid-JSON.

##### 6.5 Do: provider retry rules (pull request 7), from Codex and others (fix, already in flight)

- Copy Codex's single error-to-policy function (`protocol/src/error.rs` classification; `core/src/responses_retry.rs`): never retry context overflow, auth, model-not-found, quota; server advice before local backoff; `Retry-After` cannot extend the configured count.
- **Add the rule Cline and Oh My Pi have and Codex only implies:** never retry a turn after it already streamed visible output, or after a tool ran ("no replay-unsafe committed output unless every tool call provably never ran", `turn-recovery.ts`). Check that PR 7's retry cannot re-run a step whose tool calls were already issued.
- Cap provider-requested waits (Oh My Pi `maxDelayMs` 300 s): a three-hour sleep in a node is a stuck session.
- OpenCodex's rule fits master-side proxies: charge retries to the logical request, not each hop.

##### 6.6 Do: fence every settle with a claim generation (fix, ties to the nodes design note)

The invariant is "events are appended only by the owning node's session actor". Nothing in the files read shows a design that survives a stale node better than Hermes: `claim_session_input` bumps a per-session `runtime_generation`; `settle_session_input` accepts only a matching epoch and generation (`hermes_state_runtime.py` L206-L274); at start a global epoch is incremented and every `started` row of an earlier epoch becomes `unknown` and is never requeued (L453-L464). OpenClaw's `activeWriterRunId` compared inside the write transaction (`session-accessor.sqlite-scope.ts` L79-L97) does the same at the storage level, but is skipped when a write carries no expected writer: make the unfenced path impossible. OpenCode has no fence and says its sweep "assumes every orphaned claim's owner is dead"; Codex and DeepSeek use file locks that only protect one host.
- **For us:** when a workspace node reconnects or a session is moved to another node (the design note's worktree-per-session and node-per-workspace), give each claim a generation; the master rejects an append that names an old one. Test: two nodes append to one session; the older one is refused and the log stays contiguous.
- Also copy the OpenClaw lost-worker message: "tool effects may be unknown"; never auto-continue a turn after a lost node.

##### 6.7 Do: unattended runs close themselves and record why (fix, routines)

- **Copy from Hermes** (`gateway/session_unattended_unknown.py`, `cron/scheduler_tick.py`, `cron/scheduler_authority.py`): advance the routine's next-run time before dispatch (at-most-once), write an idempotency key for each fire before it runs, pause the routine when the outcome is unknown, and settle a crashed routine turn as `interrupted` with a reason so the next run is not blocked; a person's own crashed turn waits for a decision. OpenClaw's cron retry has no attempt counter (a run that crashes again before delivery qualifies again): store an attempt count on the receipt.
- **Default for unattended:** keep our "routines run agentic only in `ask` or `ask_all`" rule; Hermes' `cron_mode: deny` is the same idea. Do not follow Oh My Pi, Cline or goose (default `yolo` or `Auto` for scheduled runs).
- **Return the real refusal to the model** on unattended runs (Warp tells the model "denylisted" for every denial, which misleads it).
- Skip-not-burst on catch-up (OpenCode, DeepSeek's latest-only) and an explicit overlap rule (goose lets a job overlap itself).

##### 6.8 Do: a dispatcher-level ceiling on every tool result (fix, small)

Pi enforces truncation per built-in tool; an extension tool returning 10 MB goes straight into context. Our shell filter and the 2 MB `tool_output` store exist; pull request 4 adds capped file tools. **Check:** `web_fetch` (2 MiB body cap, 20,000 characters), MCP results and any future tool pass through one cap in the dispatcher, ideally proportional to the context window (OpenClaw: 30% of the window and 16k, 32k or 64k characters), with a recoverable handle (Cline's `cline://cache` URI; our `tool_output` id is the equivalent). Cap while reading, not after (goose holds a gigabyte in memory first; our idle RSS budget makes this mandatory).

##### 6.9 Do: cancel and orphan processes (fix; mostly gate3)

- Cancel semantics to copy: Goose's "every open tool call gets a terminal result" on cancel (`machine.rs` L250-L285); OpenCode's cooperative cancel, grace, then interrupt, and its "decline is a defect, feedback is a failure" split so a tool cannot swallow a refusal (`model-request.ts`, `permission.ts`).
- Kill: Oh My Pi re-walks the process tree between SIGTERM and SIGKILL (`proctree.rs`); a cgroup or pid namespace is stronger (goose and Cline children that call `setsid` escape a group kill).
- **Node crash leaves children running** (goose: shells get no `PR_SET_PDEATHSIG`). Check what our node does with a `bash` child when the node is killed; with the Landlock launcher in pull request 4, test that a restarted node does not leave an earlier shell running against the same worktree.

##### 6.10 Decide, feature (freeze): input during a turn

Today: reject. That is safe; no accepted input is lost. If the owner breaks the freeze, the evidence says:
- Persist the message as an event or row with an idempotency key before acknowledging (Hermes `session_admissions`, OpenClaw `session_pending_inputs`, DeepSeek's `requestId` in the logged message). Pi's and OpenClaw's in-memory queues do not survive a crash.
- Deliver at a step boundary and never kill a running tool (Codex, Pi, Oh My Pi). In parallel batches this waits for the batch; if we stay sequential it is fine. Make interrupt a separate explicit action.
- When a queued message is dropped, write an explicit receipt (OpenClaw).
- Avoid the OpenHands shape: a message written to a socket the server has not subscribed, or held in browser memory, can be lost; the node should append the user message to the log before any "run" decision.
- Cost against the plan: a new event type, a node protocol message and UI state (not in `Shell.tsx`). I would not do it before Gate 3.

##### 6.11 Avoid

- Per-tool authorisation (OpenCode `permission.assert`, Cline per-tool validation after approval): one forgotten check is an unchecked tool. Our dispatcher should authorise once, with tools declaring resources.
- Validation after policy (DeepSeek): validate arguments before policy so a reviewer sees a normalised shape, and re-validate after any hook mutates them (Pi does not).
- Tool results as user-role text (OpenHands client tools): results are events written by the owning node.
- Persistence as an option (OpenCode's durable log is off in the shipped server): our log is the truth.
- Parallelism by name ("read-only"): Codex's lesson is that a shell command can write. If we add parallel execution, key it on declared effects, serialise authorisation and parallelise execution (Pi, Cline), and keep shell exclusive.
- Resume cost growing with age: DeepSeek reads the whole log (`read(0)`). Codex's rule (latest full-history compaction is a base, replay only the suffix) makes resume independent of session age; our fold needs the same checkpoint record. This ties to pull request 7's compaction.

##### 6.12 Relation to open pull requests and the design note

| Point | main | PR 4 (sandbox, bounded link) | PR 6 (agent roster) | PR 7 (retry, compaction) | Design note |
|---|---|---|---|---|---|
| 6.1 append, flush, act | needs the check above | bounded channels could delay appends; verify | none | none | node-per-workspace: the writer is still the node |
| 6.2 loop guard | missing | none | per-agent limits could carry thresholds | retry must not mask a loop | none |
| 6.3 limits in `node.toml` | cap hard-coded | `[limits] tool_stream_bytes` exists | agents already carry `max_subagents` | idle timeouts belong here | per-turn grants: key lifetime equals turn |
| 6.4 length stop | unknown | none | none | natural home | none |
| 6.5 retry | none | none | none | in flight | none |
| 6.6 fencing | one node per session | none | server-side enforcement of where an agent may run | none | needs a claim generation when sessions move |
| 6.7 routines | ask-only | none | none | none | deletion flow must stop unattended runs |
| 6.8 truncation | filter plus store | capped file tools | none | none | none |
| 6.9 cancel | partial (gate3) | Landlock child; check orphans | none | none | none |

### Approvals and permission rules

From [round2/synth/02-approvals-permission-rules.md](round2/synth/02-approvals-permission-rules.md).

##### Where we stand

- **Main branch.** Three modes: `ask_all`, `ask` (default; asks for mutating tools) and `auto`. The node clamps a request with `min(requested, node max)` (`crates/core/src/policy.rs:19-23`). There are no per-command rules and no persistent "always".
- **How a request flows.** A request is an `approval_requested` event carrying `{approval_id, call_id, tool, input, reason}` (`crates/core/src/event.rs:446-452`). The node makes `approval_id` with `new_id()` (`crates/node/src/session.rs:958`, `:979`). The answer is `approval.respond {req_id, session_id, approval_id, decision}`, where `decision` is `Allow | Deny | AllowSession` (`crates/protocol/src/lib.rs:217-222`, `crates/core/src/policy.rs:42-47`). Restart or cancel denies anything pending.
- **"Allow for this session".** It is keyed on the tool name and lives for one `run_turn` (AUDIT L10, `crates/agent-native/src/lib.rs:625`). One click therefore allows every later `bash` command in that turn. PR 4 (commit `0d5cd95`) narrows this: the allowance is honoured only after a first approval of that tool in the turn, and it is recorded node-side. It still keys on the tool name.
- **Delegation.** It uses the same approval mode, with depth one.
- **Not built yet.** We have no hooks, no plan mode and no model approver (PLAN only). PR 6 adds server-side enforcement of where an agent may run. PR 4 adds a fail-closed Landlock sandbox for the shell.

By the evidence above, our answer sits at level 2 of 4.3: an opaque id. Our "always" sits at level 4: a tool name. Both are below Codex and OpenClaw.

##### Ranked recommendations

**1. Bind every approval to an action digest, and make the answer name it.**

This is the single most important change. It is a protocol and event change, and it is security code, so it gets the full stress test and an abuse test.

What to do:

- **Compute a digest on the node when the request is made.** `ApprovalRequested` gains `action_digest`: a SHA-256 over a length-prefixed canonical encoding of:
  - session id, node id, call id;
  - tool name;
  - the canonical input JSON (for `bash`: the exact command string, plus the parsed argv per segment once item 5 exists);
  - cwd, by realpath;
  - the names and hashes of any environment beyond the allow-list;
  - the sandbox profile and any requested permission delta;
  - a hash of the node policy snapshot.
- **Make the answer echo it.** `approval.respond` carries `approval_id` and `action_digest`. The node rejects an answer whose digest differs from the pending request.
- **Use each approval once.** The node consumes it on first use and rejects a replay.
- **Re-ask if policy moved.** If the policy snapshot changed between request and answer, the node asks again. Copy OpenClaw's "policy snapshot changed, approve again" check: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L349-L357
- **Check who answers on the master.** The master accepts an answer only from the chat's owner (chats are private). Copy OpenClaw's Gateway replay guard (`sanitizeSystemRunParamsForForwarding`, `consumeAllowOnce`): https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-invoke-system-run-approval.ts#L197-L252
- **Bind executables the way OpenClaw does.** Bind the executable's real path, add a content hash when the file is writable by the user, and bind cwd by inode: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L366-L458
  OpenClaw leaves a window between hashing and exec. Close it by hashing from an open file descriptor and executing that descriptor. Codex does this for its bundled bubblewrap: it checks the digest, then execs through the fd (codex.safety.md review row "Which bwrap"). Whether this works without `unsafe` in our crate set is my own open question, not a finding.
- **Show what is being approved.** The UI renders the full input. The card stays deny-only until the whole command has been shown: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_gateway_wait.py#L182-L188
  That card is [unchecked]; build it from the idea, not the code. Redact secrets in the display only, never in the digest input.

What to avoid:

- OpenHands `{accept}`, Cline "any client by `approvalId`", DeepSeek harness `{clientId, eventId, outcome}` and OpenCode "any password holder". All four answer an id, not an action.
- OpenClaw's empty approver list meaning "anyone in the chat" (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/approval-auth-helpers.ts#L72-L89). Ours must mean "the owner only".

The node keeps the last word. An approval can only turn an `ask` into a run. It can never turn a `node.toml` deny into a run. That is the opposite of OpenClaw's `approvedByAsk`, where a forwarded approval satisfies the node's own allow-list: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117

Tests, written first:

- an answer with a wrong digest is refused;
- a second answer to the same id is refused;
- an answer from a non-owner is refused at the master, and the node never sees it;
- a policy change between request and answer forces a new request;
- a `node.toml`-denied tool stays denied after an `Allow` answer.

**2. Redefine "always", and keep it narrow.**

Replace `AllowSession`-by-tool-name.

- **"Allow this again in this session."** This is the only remembered decision the UI offers by default.
  - Keyed on Codex's session key: canonical argv, cwd and permission delta. Read `UnifiedExecApprovalKey` and the cache check: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/runtimes/unified_exec.rs#L87-L98 and https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/sandboxing.rs#L72-L117
  - Held on the node and written as an event, so a fold over the log rebuilds it after a restart.
  - Ends with the session, and never crosses into a child session.
  - Never cached for network access, permission escalation, or future MCP tools. Codex leaves these three out too.
- **Persistent "always" is a rule, never a click.** It may exist only as an owner-written rule in a node-side rules file next to `node.toml`. The UI can propose the exact rule text (Codex's "smart approvals" proposal has a ban list and must cover the whole command: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L1065-L1127). The owner must see and confirm the exact rule.
- **Limits on any rule:**
  - It is scoped to one project root on one node.
  - It is never a first-word prefix for wrappers or interpreters: `sudo`, `env`, `bash`, `sh`, `zsh`, `python*`, `node`, `perl`, `ruby`, `xargs`, `find`, `git -c`. Compare OpenCode's `sudo *` and `bash *`: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/parse.ts#L603-L609
  - It never lifts the sandbox. Codex's `Skip { bypass_sandbox: true }` is the counter-example (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L492-L512). Leaving the Landlock profile of PR 4 is a separate `node.toml` grant.
  - It is never written by the agent. Our file tools already refuse the data directory. Add the rules file to the always-prompt class, as Zed does for `.zed`: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/thread.rs#L6218-L6235
  - No repository file can add or loosen a rule. Oh My Pi (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L2800-L2814) and OpenCode are the counter-examples.
- **Avoid permanent global grants.** That rules out Zed's prefix regex written to user settings (https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/pattern_extraction.rs#L53-L95), goose's bare tool name, and Hermes Agent's class key in agent-reachable config.

Per-command rules are a feature, so they wait for Gate 3. The session-key replacement for AUDIT L10 is a fix and does not wait.

Tests:

- approving `git status` does not allow `git push`;
- approving in turn 1 does allow the identical argv in turn 2 of the same session;
- a child session does not inherit the parent's grant;
- a restart rebuilds the grants from the log.

**3. Make these failure modes fail closed, each with a test.**

| Failure | Required outcome | Counter-example / model |
|---|---|---|
| Policy or rules file fails to parse | The node refuses to start, or refuses agentic sessions. Never drop the other rules silently. | Codex `load_exec_policy_with_warning` drops every user and project rule: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L729-L744 |
| Unknown mode or setting value | Treat it as the strictest value. We already reject unknown keys; also reject unknown values. | Oh My Pi falls back to `yolo`; Cline cron turns unknown modes into yolo. |
| Shell parse failure or unanalysable construct | No auto-allow: ask. Deny if unattended. | Zed `from_input`, OpenCode `parse.ts#L162-L171` (model); Cline plan guard (counter). |
| No human reachable (routine, link down, no UI) | Deny. Never "nobody is here, so approve". | Hermes Agent `-z` (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1300-L1308), goose scheduled `Auto`, Cline cron (counters); OpenClaw node ask→deny (model). |
| Approver callback missing or throws | Deny. | Cline `prepareToolExecution` (model): https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/agents/src/agent-runtime.ts#L2539-L2571 |
| Audit event for ask or decide cannot be appended | Reject the request. Our one-writer rule applied to approvals. | DeepSeek harness `user-approval` (model): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/user-approval/src/index.ts#L224-L261 |
| Restart or cancel with an approval pending | Deny, and the old approval can never run later. We already do this. | goose lease expiry (model): https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L999-L1039 |
| Model approver errors, times out or returns invalid output | Escalate to the human. Deny when unattended. | OpenClaw, Hermes Agent (models); goose adversary and classifier (counters). |
| A future permission hook crashes, times out or returns bad output | Block the call (not the whole run). | Oh My Pi runner (model): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L2338-L2364 ; Codex, Cline, goose, OpenHands (counters). |
| Any review layer errors | Record which layer decided, and never drop the layer. | goose skips a failing inspector: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_inspection.rs#L84-L118 |
| Sandbox unavailable | Refuse. PR 4 does this with `[sandbox] mode = required`. Keep it. | Zed macOS (model); Zed Linux "run unsandboxed for this thread" prompt (counter). |

Return the real reason to the model and to the UI. Warp reports every refusal as "denylisted", and goose reports every denial as "the user declined".

**4. Put one gate at the dispatch boundary, and route every path through it.**

Our loop checks before dispatch. Keep it that way, and write a test per path. The paths that must reach the gate:

- every tool;
- `delegate`;
- any future code-mode or scripting tool, call by call (goose Code Mode is the counter-example; OpenCode `codemode/tool.ts#L126-L145` and Pi's re-entrant `runToolCall` are the models);
- any future MCP tool;
- any user-typed command (Pi `!` is the counter-example);
- any template (OpenCode `` !`cmd` ``).

No execution endpoint may exist outside a session. OpenHands' server-level bash is the counter-example: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/bash-service/bash-service.api.ts#L24-L44

Validate the arguments before authorising them (Pi). Re-authorise after anything rewrites them (Oh My Pi `ExtensionToolWrapper`): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L355-L374

Authorise calls one at a time even when they run in parallel. Read Pi's `executeToolCallsParallel`: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L596-L670

**5. If per-command rules come (after Gate 3), parse with a maintained parser.**

- **Allow rule:** it applies only when every simple command matches and the parse contains no unanalysable node: substitution, process substitution, `eval`, heredoc, a function definition, or a redirect to a path outside the session folder.
- **Deny rule:** it matches any segment, after unwrapping `sudo`, `env`, `bash -c`, `sh -c`, `xargs`, `nohup`, `timeout`, `command`, `exec` and absolute paths. Copy Hermes Agent's per-segment executable projection: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_detection.py#L1623-L1664
- **Splitting:** read Codex's node-kind allow-list (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/shell-command/src/bash.rs#L29-L98) and Zed's `extract_commands` (https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/shell_command_parser/src/shell_command_parser.rs#L559-L650). Zed treats redirects as pseudo-commands.
- **Parser crate:** `tree-sitter-bash` or `brush-parser`. Both are crates, so they need the dependency check: licence, age and `cargo deny`.
- **What we only run is bash.** Never parse PowerShell as POSIX (Zed).
- **Test:** one test per refused construct.

Rules are a convenience layer. The Landlock sandbox from PR 4 stays the containment. Do not repeat DeepSeek harness's "sandbox instead of rules" while reads and network stay open, and do not trust rules to contain anything.

**6. The model never lowers its own gate.**

- **Never let a model flag skip a check.** No `is_risky`, `security_risk`, `requires_approval`, `readOnlyHint` or `approvedBuilds` style field may skip an ask. Warp and OpenHands are the counter-examples.
- **Model-requested escalation must:**
  - carry a non-empty reason;
  - pass the normal check first;
  - be approved by a human once, bound by item 1;
  - be reported in the tool result when it ran with wider rights.
  Read Zed's `terminal_tool.rs` (https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L678-L705) and DeepSeek harness's `approveEscalation` (https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox/src/escalation.ts#L172-L206).
- **An escalated retry is a new approval.** Copy Codex's rule that a reviewer approval never pre-approves an escalated retry: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/orchestrator.rs#L491-L501

**7. Keep every credential that can answer an approval away from the agent.**

- **What we already do.** Our environment allow-list (main, extended in PR 4) keeps the master session cookie, node keys and provider keys out of `bash`.
- **Add a test.** No variable in the child environment can authenticate to the master or answer `approval.respond`.
- **The node cannot answer.** The master accepts an answer only from a browser session of the chat owner, never from the node link. A prompt-injected node session therefore cannot approve itself.
- **Counter-examples:** OpenHands (https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985) and the OpenCode server password.
- **Design note.** It grants provider keys to nodes per turn. Keep those grants from ever carrying master authority.

**8. Sub-agents inherit a ceiling, not a copy, and never ask for themselves.**

- **How children work today.** Our children use the same approval mode, with depth one.
- **Ceiling.** Make the child's ceiling the parent's effective policy at every call, not a creation-time copy (OpenCode's copy is the counter-example: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L311-L314).
- **Roles narrow only.** An agent definition is a patch type that can only narrow. Copy Codex `role.rs` and `child_config.rs`: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/child_config.rs#L197-L218
- **Prompts reach the parent's owner.** A child's approval prompts go to the parent chat's owner, as goose does (`summon.rs`, `subagent_handler.rs`), carrying the item-1 digest.
- **Unattended children deny.**
- **Grants do not travel.** "This turn" and "this session" grants do not pass between parent and child in either direction.
- **Counter-examples:** Oh My Pi forced `yolo`, Cline's policy-less children, and Warp and DeepSeek harness foreign children with safety off.
- **PR 6.** It enforces where an agent may run on the server. Check that it is also enforced on the node, since the node decides.

**9. Plan or read-only mode, when built, is a capability, not a prompt.**

This is a feature, so it waits for Gate 3.

- **Enforce it in three places:**
  - the tool registry: remove the mutating tools from the request, not only refuse them;
  - the dispatcher: refuse at execution;
  - the sandbox: a read-only Landlock profile for `bash`.
- **Leaving it.** Leaving plan mode is a user-only event. A model-callable tool cannot do it; Cline's `switch_to_act_mode` is the counter-example.
- **Models.** OpenHands' separate planner conversation with three tools (https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1352-L1363) and OpenClaw's `read-only` session mode.
- **Counter-example.** Codex's prompt-only `/plan` and its empty `explorer.toml`.
- **Check our own agents.** The read-only Scout and Reviewer agents: are they enforced by tool narrowing and node policy, or only by description? Our inventory says the `tools:` list narrows what is offered, and that node policy decides execution. Add a test that a Scout cannot execute a tool outside its list, even when the model names that tool.

**10. A judge model, if it is ever built (PLAN §0), follows these rules.**

- **What it sees.** The digest-bound action and the user's own messages, never tool output. It runs in a read-only, rule-less context.
- **Output.** Strict closed-key JSON (OpenClaw `exec-auto-reviewer.ts`: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-auto-reviewer.ts#L152-L191).
- **Pre-filter.** A regex that denies text aimed at the reviewer before the model sees it.
- **Any failure goes to the human.**
- **Scope of its approval.** It covers only what it reviewed, and cannot approve an escalation.
- **Denial counter.** It lives in the session log, not in memory per turn (Codex per turn, OpenClaw LRU).
- **Repository instructions are not authorisation.**
- **Its decision is an event naming the judge model,** so the UI can show "approved by judge".

**11. Hooks, if ever added, fail closed and can only restrict.**

- A hook may deny, or escalate to the human. It may never approve (Hermes Agent).
- Policy is re-resolved on any rewritten input (Oh My Pi).
- Hooks shipped in a repository need a node-side trust record bound to a content hash of the hook and every script it references (Codex hashes only the declaration).
- Crash and timeout block.

##### What not to copy at all

- Open defaults: OpenCode, Oh My Pi, goose, Cline CLI, OpenClaw host exec.
- Independent security and ask axes, where a combination silently never prompts (OpenClaw `exec-approvals-policy.ts`).
- A "run to completion" switch that drops deny rules (Warp `auto_approve_bypasses_command_denylist`, default true).
- A command-line flag that loosens policy (Warp `--sandboxed`, Codex `--ignore-rules`).
- Hosting foreign agents with their safety flags off.

### Sandboxing and where commands run

From [round2/synth/03-sandbox-containers-execution.md](round2/synth/03-sandbox-containers-execution.md).

These points are ranked. Each says what to do, whom to copy from, and how it relates to `main`,
pull request 4 (Landlock, on `integration/gate3`) and the design note
(`docs/nodes-credentials-environments.md` §4.6).

##### 6.1 The recommended rootless Podman invocation

The node builds this argv and nothing else contributes to it. The design note's §4.6 is close;
the changes are marked.

```
podman run --detach
  --name rh-<env>-<session>
  --label rizzy.env=<env> --label rizzy.session=<session>
  --label rizzy.argv=<sha256 of the canonical argv below>            # ADD
  --userns=keep-id --user <node uid>:<gid>
  --cap-drop=all --security-opt=no-new-privileges
  --read-only
  --tmpfs /tmp:size=1g,mode=1777,nosuid,nodev
  --tmpfs /run:size=64m,nosuid,nodev,noexec                          # ADD sizes and flags
  --network=none | --network=pasta                                   # see the network row
  --memory=<m> --memory-swap=<m> --cpus=<n> --pids-limit=<p>
  --ulimit core=0                                                    # ADD
  --mount type=bind,src=<root>/<env>,dst=<root>/<env>,relabel=private
  --workdir <root>/<env>/worktrees/<session>
  --env HOME=<root>/<env>/home --env NAME …                          # names only; values in podman's env
  --init --pull=never --log-driver=none
  <image@sha256:…> sleep infinity
```

Each `bash` call runs as `podman exec --user <uid>:<gid> --workdir … --env NAME … <ctr> <wrapper> bash -c <cmd>`.

Why each flag is fixed, and the project it comes from:

| Flag | Reason | Evidence |
|---|---|---|
| Node-built argv; no `extra_args` key in `node.toml`, now or later | Hermes appends `docker_extra_args` last, so a `-v /:/host` overrides everything, and its host-access check never reads extras. OpenClaw's three `dangerously*` booleans are the same hole, behind a name. | Hermes `docker.py#L741-L745`, `approval.py#L1161-L1175`; OpenClaw `validate-sandbox-security.ts#L23-L53` |
| `--userns=keep-id --user` | Hermes runs as root with `DAC_OVERRIDE` over host binds; do not. With keep-id, files written in the volume are owned by the node user, which the node's file tools and checkpoint git need. | Hermes `docker.py#L321-L343`; design note §4.6 |
| `--cap-drop=all`, nothing added back | Hermes re-adds `DAC_OVERRIDE`, `CHOWN`, `FOWNER`, `SETUID`, `SETGID`. OpenClaw keeps `ALL` dropped by default. | Hermes `docker.py#L321-L343`; OpenClaw `container-allocation.ts#L71-L167` |
| `--security-opt=no-new-privileges` | The one flag OpenClaw's configuration cannot remove. Hermes drops it in `docker_snap_compat`. | OpenClaw `container-allocation.ts#L71-L167` |
| No `--security-opt seccomp=unconfined` or `apparmor=unconfined`, ever | OpenClaw rejects both. Keep Podman's default seccomp profile. | OpenClaw `validate-sandbox-security.ts#L274-L356` |
| `--read-only` plus sized tmpfs | OpenClaw sizes every tmpfs. The design note's `/run` has no size, so a full `/run` is RAM pressure on the node. | OpenClaw `config.ts#L91-L117`; Hermes tmpfs `noexec` on `/run` |
| `--network=none` by default | OpenClaw's default is `none`; Hermes's default of on is the mistake. When `network = "on"`, Hermes shows that an env-var proxy is not egress control: raw sockets bypass it. So "on" means really on. Any allow-list needs a namespace with only a proxy route, which Codex's bridge and Zed's proxy show. | OpenClaw `config.ts#L91-L117`; Hermes `docker_egress.py#L29-L76`; Codex `proxy_routing.rs#L131-L206` |
| Never `--network=host` or `container:*` without `allow_host_network` | OpenClaw rejects both outright. | OpenClaw `validate-sandbox-security.ts#L93-L132` |
| No published ports; nothing on the host listens for the container | OpenHands binds `0.0.0.0` so containers can reach the host, putting the whole API on the LAN. | OpenHands `dev-with-automation.mjs#L528-L543` |
| Limits from `node.toml` only; a session may lower them, never raise them | OpenHands forwards client-supplied memory and CPU with no bound. | OpenHands `agent-server-adapter.ts#L804-L835` |
| `--ulimit core=0` | OpenClaw. A core dump of a process holding a secret lands on the volume. | OpenClaw `config.ts#L91-L117` |
| One `--mount`, source validated | OpenClaw rejects socket paths, credential directories and system directories, and checks the real path of the nearest existing ancestor. Our single source must pass `check_protected` and must not be, or contain, the data directory, `$XDG_RUNTIME_DIR` (the rootless Podman socket lives there) or `~/.ssh`. | OpenClaw `validate-sandbox-security.ts#L23-L53` |
| `--env NAME` with the value in the `podman` child's environment | Hermes keeps values out of `/proc/*/cmdline` this way; goose puts `-e K=V` on argv, and the host process list shows it. | Hermes `docker.py#L539-L542`; goose `stdio.rs#L40-L53` |
| `--pull=never`, image by digest from the allow-list | OpenClaw `--pull=never`. The image is never chosen by the master, the project or the repository. | OpenClaw `container-allocation.ts#L71-L167` |
| `--label rizzy.argv=<hash>`; recreate on mismatch | OpenClaw bumps `createArgsEpoch` so an old container never runs with stale flags after an upgrade. | OpenClaw `constants.ts#L62-L64` |
| Check that the created id is 64 hex digits | OpenClaw. It rejects a confused CLI output before the id is used in later commands. | OpenClaw `container-allocation.ts#L214-L273` |
| `--init` | OpenClaw and Hermes. It reaps zombies and gives the container a real PID 1, so stopping the container kills every descendant (see 6.4). | as cited |
| No node or master credential in the container | OpenHands puts the session key, which can approve the agent's own actions, into every container. The link key and any grant must never be in the volume or the environment. | OpenHands `dev-with-automation.mjs#L975-L985` |

Network caveat (`not settled`): no input file says what rootless `pasta` exposes to the container.
In particular, it is not known whether the host loopback (the master on `127.0.0.1:7777`, the
node's own sockets) is reachable. Codex's reviewer flagged the same shape: with network on, an
unauthenticated loopback listener is reachable from the sandbox (`inferred`). Write the test
before shipping `network = "on"`.

##### 6.2 What to add to the Landlock path (PR 4): a seccomp layer, rule by rule

Our Landlock path already confines reads, which DeepSeek's does not. Its residuals are UDP, DNS
and pathname sockets below ABI 9. Codex's `Restricted` table fills exactly that gap.

Install the filter inside `sandbox-exec`: after `restrict_self` and `no_new_privs`, immediately
before `exec`. The order follows Codex's inner stage, which also checks that no capabilities
remain first.

| # | Rule | Mode | Closes | Caveat |
|---|---|---|---|---|
| 1 | `socket`: allow only `AF_UNIX`; `EPERM` otherwise | network off | UDP and DNS, raw sockets, `AF_VSOCK`, and anything Landlock ABI 4 does not cover | Codex accepts this in `Restricted`. Test that the tools we expect (git, cargo, compilers) still run. |
| 2 | `socketpair`: `AF_UNIX` only | network off | nothing new; matches Codex | none |
| 3 | `connect`, `bind`, `listen`, `accept`, `accept4`: deny | network off | pathname Unix stream sockets (Docker, D-Bus, ssh-agent) on kernels below ABI 9 | Breaks any legitimate local socket use under network off. Codex accepts that. |
| 4 | `sendto`, `sendmmsg`, `recvmmsg`: deny | network off | addressed datagrams | `recvfrom` stays open: Codex keeps it for socketpair users (cargo clippy). |
| 5 | `sendmsg`: decide explicitly | network off | an addressed `sendmsg` to a pathname datagram socket (for example a syslog or notify socket) | Codex leaves `sendmsg`/`recvmsg` open (the reviewer's correction). Seccomp cannot read `msg_name` behind the pointer, so denying `sendmsg` also breaks socketpair IPC that uses it. Whether Landlock ABI 9 covers datagram sends is not in the input. Either deny it and test the toolchain, or keep it open and list it as a named residual in `PLAN.md` R2. Do not leave it unmentioned. |
| 6 | `getsockopt`, `setsockopt`, `getpeername`, `getsockname`, `shutdown`: deny | network off | nothing on its own; it is part of Codex's set | Codex denies these and its tools work. Copy the set, and drop a rule only if a test shows breakage. |
| 7 | `io_uring_setup`, `io_uring_enter`, `io_uring_register`: deny | always | io_uring ops do not pass through the seccomp checks on `connect` and the rest; Codex blocks io_uring in every mode | none known |
| 8 | `socket(AF_VSOCK)`: deny | always, including network on | host services over vsock; on WSL2, Windows process launch | Codex's stated reason |
| 9 | `ptrace`, `process_vm_readv`, `process_vm_writev`: deny | always | reads of another sandboxed process's memory | Landlock already denies ptrace-class access to processes outside the domain; this is defence in depth inside the domain |
| 10 | Default action allow; denied calls return `EPERM`, not `SIGSYS` | | | A denylist, as in Codex. Use `EPERM`, never a kill: Codex treats 128 plus SIGSYS as a "sandbox denial" that can lead to an unsandboxed retry. |
| 11 | Unknown architecture: `Refused` in `required` mode | | | Codex supports only x86_64 and aarch64 and panics elsewhere. Our equivalent must be a refusal, not a silent skip. The filter must also check the syscall architecture field (the 32-bit compat syscall path); that requirement is ours and is not in the input. |

How to report it: extend `Enforced` to include "seccomp filter installed". The status line
should say which of rules 1 to 9 are active. DeepSeek computes `full`/`partial` and shows it to
no one; ours must appear in the node status and in the tool result (as Zed tells the model when
it ran unsandboxed). With rules 1 to 4, the PR 4 residuals "UDP, DNS" close under network off,
and "pathname sockets below ABI 9" closes for stream sockets. The datagram case stays open unless
rule 5 denies `sendmsg`.

Dependency: a seccomp crate must go through the `CLAUDE.md` dependency check (exact pin, docs.rs,
licence, two weeks old, `cargo deny`). No input file evaluates one. Our own code must stay free
of `unsafe`.

##### 6.3 Also on the Landlock path

1. **Protected subpaths inside writable roots.**
   - Codex keeps `.git`, `.agents`, `.codex` and `.aws` read-only.
   - DeepSeek has none, so `.git/hooks` is writable and a later unconfined `git` runs the hook.
   - Zed's Linux protection skips a `.git` that does not exist yet.
   - Check whether PR 4 protects `.git/hooks` and `.git/config` in the session folder. If not,
     add Landlock rules that leave them read-only. Test the "`.git` did not exist at start"
     case.
2. **The node's own sockets.** Codex had to mask its daemon directory because a same-uid process
   could drive an unauthenticated control socket.
   - With `network = "on"`, below ABI 9, a Landlock-confined command can connect to any pathname
     socket the node user owns.
   - Inventory every socket the node and the master open: master WebSocket, node link, MCP
     bridges, the Podman socket. Each must be authenticated or outside `SYSTEM_READ`.
   - Test: a sandboxed command cannot connect to any of them.
3. **One root derivation.** DeepSeek's file fence and its bwrap backend disagree about `/tmp`.
   Our file tools and the sandbox should derive their roots from one function. The design note's
   "same absolute mount path" requirement for containers is the same rule.
4. **Kill on Landlock.** A process-group kill leaks `setsid` children (Cline, goose; inferred for
   Oh My Pi and Pi). Oh My Pi's re-walk narrows the leak; a cgroup closes it. On the host path,
   consider a per-session cgroup or systemd scope. At least test that a `setsid sleep 1000`
   started by the agent is gone after cancel.

##### 6.4 Killing inside the container

The design note assumes that killing the `podman exec` client does not kill the process inside.
No input settles that.

Keep the design's three steps, all three:

1. A wrapper prints its process-group id.
2. `podman exec <ctr> kill -TERM -<pgid>`, a grace period, then `-KILL`.
3. If that fails, `podman stop`. That step is the guarantee: with `--init`, stopping the
   container kills its whole cgroup, the same reason Codex and Zed use `--die-with-parent` and a
   PID namespace.

Add Pi-env's liveness rule: when the node loses a session's actor, it stops that session's
containers. Do not leave them for a later sweep: Warp's `sbx` cleanup is a TODO and stopped
containers pile up.

##### 6.5 Do not relax checks inside a container

- Hermes skips the hardline floor, the sudo guard and the dangerous-pattern prompts on container
  backends.
- OpenClaw's default lets sandboxed exec run without approval.
- Warp's `--sandboxed` empties the denylist.
- OpenHands skips hooks for container workspaces.

A container with network can still push to a remote or exfiltrate. Keep the approval path, the
floor and checkpoints identical for host and container sessions. Add a test that a command
denied on the host is also denied in a container session.

##### 6.6 Executor policy and escalation

Our node is the executor and the decider. Three models to set against it:

- Do not copy Codex's exec-server, which applies whatever sandbox intent arrives and runs
  unsandboxed when none does.
- Do not copy OpenClaw's rule that a Gateway-forwarded approval satisfies the node's allow-list.
- Copy OpenCodex's executor rules:
  - validate every argument on the executor;
  - refuse `exec` when no sandbox exists;
  - check file identity after open;
  - refuse hard links;
  - re-check the root's identity on each call.

For escalation, copy Zed and DeepSeek, not Codex:

- The model asks for a specific widening with a required reason. The grant is one-shot and bound
  to that call. A sub-agent cannot escalate (DeepSeek pins it to `never`).
- The result tells the model the truth about its isolation.
- Never let an `allow` rule remove the sandbox (Codex). Never let a "for this thread" answer
  silently unsandbox later commands (Zed).
- Never decide "denied by sandbox" from stderr keywords (Codex). Use our exit code 125 and the
  runner's own signature (DeepSeek).

##### 6.7 Unavailability

- Keep PR 4's fail-closed `required` mode.
- Copy DeepSeek's functional probe: really restrict and run `true`, never a version check. Run it
  once at node start, and announce the result.
- For containers, the design note's self-test (run, exec, kill, `keep-id` ownership) is the
  matching probe. Add `podman inspect` assertions to it: no privilege, `CapEff` empty, one mount,
  `ReadonlyRootfs`, the expected network mode, the argv label.
- Zed's "Run unsandboxed" fallback prompt is a design we should not offer. A node either has a
  confinement kind or refuses the session.

##### 6.8 Pass-through flags for foreign agents

If rizzy ever runs Codex or Claude Code as a sub-agent, Warp shows the failure:
`--dangerously-bypass-approvals-and-sandbox` plus pre-written trust. The foreign CLI must run
inside our confinement (Landlock or the container), never with its own sandbox turned off on the
bare node.

### Credentials, secrets and authentication

From [round2/synth/04-credentials-secrets-auth.md](round2/synth/04-credentials-secrets-auth.md).

Our position, for reference: on `main` the node starts children from an empty environment plus an
allow-list and `env_pass` globs, always stripping provider key vars, `.env` names and `HARNESS_*`
(`crates/tools/src/env.rs`); known secret values are scrubbed from tool output, model text and
events (a courtesy, `crates/node/src/session.rs:194-215`); the master has accounts, cookie
sessions stored as SHA-256 with `HttpOnly; SameSite=Strict`, a Host allow-list and Origin check,
and an admin-equivalent API bearer; the node dials out over TLS to a pinned certificate and signs
a nonce; each node holds its own provider key in `config.toml` (`research/round1/own-rizzy-harness.md`).
Pull request 4 adds the Landlock shell sandbox, the empty allow-listed child environment and
redaction across chunk boundaries. The design note moves keys to sealed per-workspace storage on
the master, hands them to workspace nodes per turn (`session.grant`), reserves `ModelAccess::Proxy`
for per-user nodes, and replaces join tokens with a pair code.

Ranked by importance.

**1. The agent must not be able to reach any control-plane credential. Test it.** The worst
failures in this group are not provider keys; they are the harness's own credentials in the
agent's reach: OpenHands puts the master session key in the agent's environment and prompt;
OpenCode leaves the server password in the environment outside stdio mode; DeepSeek's
cookie-signing secret is readable by confined bash, so the agent can forge a login. On a node
that means three things, each with an abuse test written first:

- The node's link private key (`node.key` or `$CREDENTIALS_DIRECTORY/node.key`), its data dir and
  `node.db` are unreadable under the Landlock profile from pull request 4. Test: a confined `cat`
  of each path fails. DeepSeek is the counter-example (finding 21).
- Grant keys live only in the node process's memory and never in a child's environment, argv or
  a file. Test: start a turn with a grant, run `env` and `cat /proc/self/environ` and
  `cat /proc/$PPID/environ` in the shell, assert the key bytes are absent.
- The node process is non-dumpable, so a same-uid child cannot read its memory through
  `/proc/<pid>/mem` or ptrace. Codex has `pre_main_hardening` but does not apply it to its main
  binary (absence confirmed in review):
  https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/process-hardening/src/lib.rs#L7-L61.
  We forbid `unsafe`, so use a safe `prctl(PR_SET_DUMPABLE, 0)` wrapper (for example `rustix`,
  which needs the dependency check in CLAUDE.md). Test: the shell's `cat /proc/$PPID/mem` or a
  `gdb -p` attach fails. Note this is not enough without the sandbox; whether Landlock alone stops
  ptrace was not in the code read (section 7).

On the master side, the same rule applies to the local node and the master's chat runtime, which
run in the master process (design note §4.3): the master's `api-token`, `secrets.toml`,
`keys/secrets.key` and `master.db` must be unreadable from the embedded node's shell. That embedded
node is the case OpenHands' host mode shows going wrong.

**2. Shape the per-turn grant like OpenClaw's worker credential, not like a long-lived key
drop.** Read `src/gateway/worker-environments/credential.ts`, `admission.ts`,
`inference-model.ts` and `store-transitions.ts` in OpenClaw. Concretely:

- *Lifetime.* The design note's `expires_ms` default of 6 hours renewed while a turn runs is
  longer than it needs to be. OpenClaw's credential lives 10 minutes and is renewed while the
  owner is current, with a timer at expiry that re-runs validation; Hermes' tickets live 30 s and
  are single-use. A grant TTL does not stop the node from copying a provider key (the design note
  says so, and only rotation fixes that), but it bounds what an attacker who later steals the
  node's link key receives, and what a crashed turn leaves in memory. Recommendation: a short TTL
  (minutes), renewed by the master on the link while the turn is live, dropped on `grant.drop`,
  on turn end, on link loss after the running turn ends (A3), and when a newer grant for the same
  session is sent. Minting a new grant should cancel any in-flight model call that used the old
  one, as OpenClaw's mint does
  (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential-broker.ts#L105-L126).
- *Scoping.* Bind the grant to `(node_id, session_id, turn or epoch)`. The node must refuse to use a
  grant for any other session, and the master must refuse to send one to a node whose
  `workspace_id` differs from the session's effective workspace. Scope it to the providers the
  resolver computed, and check that list against the *resolved* models after alias expansion, not
  the names the agent spec carries (OpenCodex finding 14:
  https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/admission-model-scope.ts#L8-L29).
  Never let the node name a key or profile (OpenClaw refuses `@profile`).
- *What to store.* On the node: nothing. On the master: `node_key_grants` with name and times, as
  designed, plus the grant id and its hash only if the grant itself becomes a bearer (it will for
  the proxy, item 3). Sealed `workspace_secrets` should use AES-GCM associated data that binds
  `workspace_id` and `name`, so a row copied from one workspace or name to another fails to open.
  Warp binds `{actor_uid, secret_name, secret_type}` into HPKE sealing
  (https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/managed_secrets/src/envelope.rs#L33-L80),
  and Oh My Pi binds the broker URL into its cache's AAD. The design note does not say what the AAD
  is; it should.
- *Register every granted value with the node's `SecretSet` at receipt* (the design note does this),
  the way OpenClaw registers each minted credential and GitHub token for redaction.
- *Revalidate per frame.* Codex checks bearer credentials once at upgrade and documents that expiry
  is not enforced after
  (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L286-L330);
  OpenClaw re-checks hash, expiry, state and epoch before every worker request. Our link should
  refuse a `session.prompt` whose `grant_id` is expired or revoked on the node, and the master
  should drop the link of a node removed mid-turn (the design note's `grant.drop` then link drop
  does this; test that a prompt sent after removal is refused).
- *Forge tokens* will be readable by the agent if they are in git's environment inside the
  sandbox (OpenClaw corrected finding). Hand them only to the git child, scoped to `[forge] hosts`,
  short-lived, and expect the agent can read them when it can run git.

**3. The proxy path (`ModelAccess::Proxy`) must be bounded before it is built.** OpenCodex is the
reference; read `src/server/inbound-body-admission.ts`, `src/lib/translator-budget.ts`,
`src/bridge/sse.ts`, `src/server/responses/request-send-budget.ts`,
`src/server/responses/combo-stream-preflight.ts` and `src/combos/failover.ts`. Oh My Pi's
`auth-gateway/clients.ts` is the reference for scoped tokens. Our process must stay under 50 MB
idle, and OpenCodex says its byte budgets "do not cap RSS" because bodies are materialised several
times. So:

- Stream the request body through without materialising it more than once; cap it well below
  OpenCodex's 256 MiB (model requests with images are the only large ones); admit against a shared
  byte budget with a bounded wait queue and shed with 503 plus `Retry-After`.
- Make the response a pull stream with a buffer of one event and cancel the upstream on client
  drop, as `bridgeToResponsesSSE` does. Cap per-event and per-turn retained bytes.
- Put the retry budget on the logical request and on the turn (OpenCodex charges a workflow root so
  fan-out cannot multiply sends). Pull request 7's provider retry with backoff must count against
  this budget when the call goes through the proxy.
- Fail over only before output, with a byte-bounded replay buffer that commits on any event it does
  not recognise; separate "may try another target", "cool this target" and "cooldown scope", and
  persist cooldowns as absolute times.
- Check scope after routing; refuse metered or paid targets unless the workspace flagged them
  (OpenCodex finding 25).
- The proxy token is a bearer. Store only its hash with a domain tag (OpenClaw), bind it to one
  session and one node, give it the grant's TTL, and test that it does not work for another session
  (design note phase 8 already lists that test). Oh My Pi's gateway tokens have no expiry; do not
  copy that.
- The relay side on a per-user node should strip any credential header the agent's request carries
  and add only its own, as `ocx-child` does
  (https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/native/ocx-child/src/relay.rs#L225-L246).
  For our link the node already authenticates to a pinned master, so the mutual HMAC challenge is
  not needed.

**4. Refuse these authentication shortcuts outright.** Each has a project in this group that ships
it, and each would break an invariant or our multi-user model:

- **`Origin` as authentication** (Cline hub). Origin is a CSRF check on top of a credential, never
  instead of one. Our master already requires the cookie; keep any future local socket the same.
- **Unauthenticated loopback** (Codex app-server and exec-server, OpenCodex data plane and
  companion listener, Oh My Pi `--no-auth`, goose `--dangerously-unauthenticated`, OpenHands MARS
  without a key). The agent's shell is a local process; a sandboxed command with network on reaches
  loopback (Codex finding, inferred). No route on the master or node may be open on loopback,
  including a future proxy or health route that does more than return `ok`.
- **Any credential in a URL**: `?auth_token=` (OpenCode), `?token=` (goose desktop, DeepSeek launch
  URL, Oh My Pi relay), `?roomSecret=` (Cline dashboard), the VS Code editor token (OpenHands),
  `#token=` (OpenClaw). Our invariant already forbids it; the claim code and the pair code must
  travel in a POST body, never as a link. OpenClaw's endpoint schema (no userinfo, no query) is a
  test to copy for `node join` URLs.
- **Credentials in served HTML or browser storage** (OpenHands injects its key into `index.html`
  and `localStorage`; OpenClaw keeps a device seed in `localStorage`). Our UI must hold nothing but
  the `HttpOnly` session cookie; if we ever add browser device keys, use a non-extractable WebCrypto
  key.
- **Non-constant-time comparison of any secret** (OpenCode `===`, Cline dashboard `===`, Oh My Pi
  `Set.has` and `!==`, goose MCP-app proxy `!=`). Grep our code for `==` on secret types; compare
  hashes in constant time as the design note's pair-code step 4 says.
- **Tokens derived from one shared secret with no per-token revocation** (OpenCode pairing tokens
  are an HMAC of the password; DeepSeek cookies are HMACs of one persistent secret). Keep
  server-side session rows, as we do.
- **One shared admin-equivalent bearer** (Hermes API server key, goose secret, OpenHands session
  key, OpenCodex env token, which carries no scope). Our `api-token` is admin-equivalent and "only
  the API token sees all" chats (`own-rizzy-harness.md`), which is the same weakness and sits
  against "chats are private to their owner, for admins too". This is outside the design note;
  it should get its own finding: per-user API tokens, stored as hashes, scoped, revocable.
- **Bearers stored in plaintext** (OpenClaw device tokens, OpenCodex data keys in `config.json`).
  Store hashes of anything we issue; OpenClaw does it for worker credentials and not for device
  tokens, which shows how easy it is to be inconsistent.
- **Approvals answerable by any connection** (Cline `approval.respond`). Bind an approval to the
  session, the owner and the tool-call identity on the master; the node already decides policy.
- **A silent fallback to a weaker store** (goose keyring to plaintext; OpenHands in-memory key).
  OpenCodex shows the alternative: fail closed with a clear error ("no plaintext fallback"; 503 if a
  token file cannot be hardened). The design note's "systemd credentials, else 0600 file" must be an
  explicit, logged choice at join, not a fallback on error.
- **Trust on first use with a blank fingerprint** (goose desktop) and SSH-verified auto-approval by
  network position (OpenClaw). Our pairing binds to a pinned certificate and a typed code; keep it.
- **Secrets on the command line** (Pi `--api-key`, goose's container path `-e KEY=VALUE`). Codex
  reads its remote token only from a named environment variable and only over `wss` or loopback;
  Hermes passes Docker values by name only.

On pairing specifically, OpenCodex's rules to copy into design note §4.4: store only the code's
hash; delete the pending request before validating anything else (one shot even on rejected
metadata); refuse requests carrying an `Origin` header; rate-limit by the kernel-observed peer
address; give every failure one generic message. Our design inverts the direction (the node shows
the code, the admin types it), so an attacker who reaches the node port can open pending requests
and print codes on its own terminal. The defence is that the admin compares the key prefix. The
design note shows 16 hex characters (64 bits); goose uses a 128-bit fingerprint in eight groups for
out-of-band comparison
(https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/card.rs#L56-L75).
Show 128 bits, grouped, on both sides.

**5. Check our environment allow-list and redaction against these lists.** Tests to add in
`crates/tools/src/env.rs` and the redaction code (pull request 4):

- *Always strip, even if `env_pass` names it.* Codex keeps a non-inheritable list that `set` cannot
  re-add (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/shell_environment.rs#L14-L28);
  Hermes had a published advisory (GHSA-rhgp-j443-p4rf) because a passthrough name could register a
  managed credential, and now re-checks at use time and fails closed
  (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/env_passthrough.py#L46-L79).
  Test: `env_pass = ["*"]` and `env_pass = ["*_KEY", "HARNESS_*"]` still pass no provider key, no
  `HARNESS_*`, no link credential.
- *Secret-name substrings for any future blocklist layer:* Hermes `execute_code` drops names
  containing `KEY`, `TOKEN`, `SECRET`, `PASSWORD`, `CREDENTIAL`, `PASSWD`, `AUTH`, `DSN`,
  `WEBHOOK`, `CREDS`, `BEARER`, `APIKEY`, `COOKIE`
  (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/code_execution_env.py#L57-L92).
  DeepSeek's shorter pattern lets `DATABASE_URL`, `GITHUB_PAT` and proxy URLs with passwords
  through; use those three as test cases for `env_pass` warnings.
- *Injection variables:* OpenClaw's ~110 blocked-everywhere keys and prefixes (`NODE_OPTIONS`,
  `PYTHONPATH`, `BASH_ENV`, `GIT_*` executables, `LD_`, `DYLD_`, `BASH_FUNC_`)
  (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/host-env-security.ts#L38-L65)
  and goose's 31-key disallow list
  (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension.rs#L91-L122).
  Test that `env_pass` cannot pass any of them and that `PATH` cannot be overridden from a request.
- *Git location variables:* Codex and Oh My Pi strip `GIT_DIR`, `GIT_CONFIG*`, `GIT_WORK_TREE`;
  goose adds `-c safe.bareRepository=explicit -c core.fsmonitor=false`. Our checkpoint git already
  clears the environment; the forge git path (design note §4.7) must too.
- *Project `.env`:* Oh My Pi strips values that came from the project's `.env`; DeepSeek accepts a
  provider key from the cwd `.env`. Test: a repository `.env` neither enters a child environment
  nor supplies a key.
- *Our own spawn secrets:* capture them into memory and delete them from the process environment
  before any child is spawned (Cline core; OpenCode stdio mode). Test: `HARNESS_*` and the node key
  path variable are absent from `/proc/<child>/environ`.
- *MCP and future plugin children:* start them from the same allow-list (Hermes:
  `PATH, HOME, USER, LANG, LC_ALL, TERM, SHELL, TMPDIR, XDG_*` plus declared env), not inherited
  (Cline, Pi, Warp, Zed and Oh My Pi inherit everything).
- *Redaction, by value first.* OpenCodex's log guard checks text against every value it holds,
  rebuilds the set on a content digest, and fails closed when it cannot enumerate it
  (https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/configured-credentials.ts#L1-L30).
  Our `SecretSet` should be the same: grant keys, forge tokens and the link key added at receipt,
  and the scrubber should refuse to emit (not emit unredacted) when the set is known to be
  incomplete.
- *Redaction patterns as a second layer:* Hermes always masks vendor-prefixed keys, auth headers,
  JWTs and private keys and runs a `KEY=value` pass on env-dump commands; for file reads it writes a
  non-reusable sentinel so the agent cannot copy a masked value back into config
  (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/redact.py#L1491-L1530).
  Warp's default pattern list (Google, OpenAI, Anthropic, `sk-`, AWS, Slack, GitHub, Stripe, JWT and
  others) is a test corpus
  (https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/secret_redaction/src/lib.rs#L296-L360).
- *Ordering and coverage:* redact before any hook or plugin sees output (Hermes redacts after an
  output hook); redact before compaction summaries are stored (Hermes redacts summariser inputs and
  output; pull request 7's compaction should be tested with a key in a tool result); and never use
  reversible placeholders that turn back into real values in tool arguments (Oh My Pi).

**6. Trust gates, for what nodes load from a repository.** The node will eventually load project
files that execute or steer (skills, MCP servers, agent definitions, setup scripts). Copy Codex's
rule that trust state lives only in a layer the repository cannot write
(`codex-rs/hooks/src/config_rules.rs`), Pi's rule that the default cannot be relaxed from the
project (`settings-manager.ts` L1020-L1023), Pi's two-pass startup (decide with trusted code only),
and Warp's "project MCP servers never auto-start". Add what none of them has: bind the decision to a
hash of every file that will execute, including scripts a hook points at (Codex's gap), and store it
in `node.toml`'s directory, not on the master. Oh My Pi's `isProjectTrusted() => true` is the
anti-pattern; OpenCode and goose show what a cloned repository can do without a gate.

**7. Smaller items.**

- Refuse to start on a loose file mode for every secret file (DeepSeek's `.credentials.yaml`
  check; our AUDIT L7 notes the `api-token` mode is not enforced on read on `main`).
- Describe parse errors of secret files without quoting lines (DeepSeek).
- Keep idempotency and webhook tokens in headers only; OpenClaw rejects `?token=` with 400 and does
  not exempt loopback from failure throttling. Ours already uses a bearer header and stores hashes.
- Credential pools (Hermes, OpenClaw auth profiles, OpenCodex) belong on the master per workspace
  once keys move there; key the backoff on the failure reason, separate "disabled until a human
  acts" (billing, permanent auth) from "cooling", and never record pool state in the event log.

### Links, workers and client protocols

From [round2/synth/05-links-workers-protocols.md](round2/synth/05-links-workers-protocols.md).

Our position, for reference. On `main`: the node dials the master over TLS with a pinned
self-signed master certificate; the node's Ed25519 key signs a nonce bound to the certificate
fingerprint; 8 MiB frames; 10 s handshake steps; 32 concurrent pre-auth handshakes
(`research/round1/own-rizzy-harness.md`, "Link"). The owning node is the only writer; the
master mirrors its events and rejects events for sessions another node owns; a link reconnect
replays with `Resume{after}`. Streaming deltas are stored as events forever (AUDIT D5). On
`main` the master reads node frames into an unbounded channel (AUDIT L1); pull request 4
bounds it (8 inbound parsed messages, 64 outbound commands). The design note
(`docs/nodes-credentials-environments.md`) adds pair codes (§4.4), per-turn `session.grant`
(§4.3), and fork by `session.seed` batches written as `history_imported` (§4.9), and records
that "a second connection replaces the first (`nodes.rs:208-212`)" (§6).

The surveyed code says one thing clearly: **only OpenClaw fences a remote executor's writes**,
and every system that lets the controlling side send policy ends up letting it widen the
executing side. Our invariants put us on the right side of both; what we lack is the fence
itself and a resume protocol that keeps deltas out of the log.

Feature freeze note: items 1 to 4 below harden the node link and the approval path (security
code under `CLAUDE.md`); items 8 and later are features and wait for Gate 3. Check each against
`FIX_PLAN.md` before scheduling; this file does not know the plan's task numbers.

##### 6.1 (Rank 1) Fence the node link and every session write

**What we have is "latest connection wins"**, the same as OpenCodex Remote Workspace
(`workspace-hub.ts` `attachConnection`, finding 31) and weaker than OpenClaw. It replaces a
dead socket, but it does not refuse frames that were already read from the old socket and
are still queued (pull request 4's 8-message inbound channel is exactly such a queue), and it
says nothing about a session that was archived, deleted or forked while its node was offline.
Codex's "one attachment plus retention" leaves the same gap (review: "no epoch or fencing term
found anywhere in exec-server").

**Design. Two counters, both in `master.db`, both checked inside the transaction that writes.**

1. `nodes.link_gen` (u64). Incremented in the transaction that accepts a node connection,
   after the signature check. Sent to the node in `Accepted { node_id, link_gen }`. The master's
   in-memory registry holds the current value per node.
   - *Checked where:* in the master's reader pump, on **every** inbound frame, before
     dispatch: the frame's connection must still be the registry's current `link_gen` for that
     node; otherwise drop the frame and close that socket with a `superseded` code. This is
     OpenClaw's per-request revalidation (`admission.ts` L188-L218, `worker-connection.ts`
     L326-L351) reduced to one integer compare.
   - *Bumped when:* each accepted connection; node revocation; node re-pairing.
2. `sessions.owner_epoch` (u64), drawn from one global counter that never goes backwards and
   never reuses a value (OpenClaw: `max(...) + 1`, `store-mutations.ts` L30-L50; a single
   `meta.epoch_counter` row does the same with one `UPDATE ... RETURNING`).
   - *Set when:* the session is created on its node.
   - *Bumped when:* the session is archived, deleted, its node is revoked or removed from the
     workspace, or the source of a "continue elsewhere" fork is closed (6.2). **Not** bumped by a
     reconnect of the same node: the node is still the owner and must be able to resume.
   - *Carried:* in `session.create`/`session.open` to the node, and in every node-to-master
     event frame as `(session_id, owner_epoch, seq, hash)`.
   - *Checked where (master):* in the same SQLite transaction as the mirror insert, as one
     predicate: `owner_node = conn.node_id AND owner_epoch = frame.epoch AND state = 'active'
     AND seq = last_seq + 1`. For `seq <= last_seq`, compare the stored event hash: equal means
     a duplicate from a resend, acknowledge it; different means divergence (a node restored from
     an old disk image, a cloned VM with the same key): refuse with `diverged` and mark the
     session for attention. This is OpenCode's `replay` rule ("Replay diverged", "Sequence
     mismatch", owner refused; `bus.ts` L329-L378) applied to a real link.
   - *Checked where (node):* the session actor stores the epoch it was given and refuses any
     master command for that session that carries a different epoch. On a `fenced` reply to an
     append, the actor stops, kills the running tool's process group, marks the session
     read-only locally, and does **not** retry (OpenClaw's worker treats `owner-epoch-mismatch`
     as terminal: `worker-connection-contract.ts` L22-L26).
3. Grants (`session.grant`, design note §4.3) are bound to `(session_id, owner_epoch,
   turn req_id)`; the node drops a grant on `grant.drop`, at expiry, on a `fenced` reply, or
   when the epoch it holds changes. OpenClaw binds its credential hash to the durable turn
   claim for the same reason (`credential.ts` L9-L60).

**How this breaks, and what the counters do about it.** A stolen node key: both copies get
`link_gen` bumps, each kills the other's socket, frames from the loser are dropped at dispatch;
the design note's "revoke after 3 flaps in 10 minutes" (§6) then bumps every session's epoch. A
node restored from a backup: its replayed events diverge by hash and are refused. A session
archived while its node was offline: the node's resume replays events stamped with the old
epoch and is told `fenced`. A master restored from an older backup: it may hold a smaller
`last_seq` than the node; the node's resend fills the gap (hashes match), which is the desired
outcome.

**Tests that fail on `main` and pass after.** (a) Queue two event frames on connection A,
connect B for the same node before A's frames are dispatched: A's frames are refused and not
mirrored. (b) Archive a session while its node is disconnected; reconnect; the node's resume is
refused with `fenced` and the node marks the session read-only. (c) Resend an already-mirrored
event with the same hash: acknowledged, not duplicated. (d) Resend `seq` N with a different
payload: refused with `diverged`. (e) A grant is not usable after the session's epoch changes.

##### 6.2 (Rank 1, same change) Session fork without live migration

The design note forbids live migration and forks by re-seeding history (§4.9). The fence above
makes that safe; the protocol:

1. The master, in one transaction: chooses `through_seq` as the highest **contiguous mirrored**
   seq of the source session S (never an unmirrored tail); records `session_forks(S', S,
   through_seq, hash_at_through_seq)`; creates S' with a fresh `owner_epoch` and its target node.
   For "continue elsewhere" (source closed), the same transaction sets `S.state = archived` and
   bumps `S.owner_epoch`, so any later append from S's node is refused.
2. If S's node is online and S has a running turn, refuse "continue elsewhere" until the turn
   ends, or close it: pending approvals in S become synthetic closers (goose answers a pending
   approval whose lease is gone with "Tool approval expired", `ops_toolcalling.rs` L1010-L1031;
   DeepSeek's closers say the outcome is unknown, `repair.ts` L53-L98). Events S's node wrote
   after `through_seq` and never delivered are refused by the fence; the UI must say how many
   (the master knows the node's reported head from its last resume).
3. Seeding: `session.seed { session_id: S', batch_no, events, last }`. The target node writes
   each batch as `history_imported` events with its own seq, keyed by `batch_no` so a resend is a
   no-op, and acknowledges only after the batch is durable (Hermes acknowledges a relay frame only
   after durable admission, `ws_transport.py` L971-L1004). S' refuses prompts until the last batch
   is acknowledged. The first event of S' records `forked_from { parent: S, through_seq,
   parent_hash }`.
4. No files cross. The design note already says files come back from the forge. Codex offers
   nothing to copy here except the "owner id inside `.git`" marker (`worktree/src/metadata.rs`
   L1-L90), which is worth copying so two sessions never share one worktree.

##### 6.3 (Rank 2) Nothing the master sends may widen the node

Every design surveyed that lets the controlling side send policy lets it widen the executing
side. Do **not** copy these properties:

| Project | Property | Why it widens the executing side |
|---|---|---|
| Codex exec-server | sandbox intent sent per request; no `sandbox` means unsandboxed; no executor ceiling | the caller's policy is the executor's policy |
| OpenClaw node | `approved === true` from the Gateway satisfies the node's ask and allow-list miss; only `security=deny` holds | a master-side approval overrides the node's own list |
| OpenClaw worker | a GitHub token in the launch descriptor, written where the agent's shell reads it | the controller hands the agent a credential |
| OpenClaw provisioning | the Gateway SSHes into the worker box | the controller dials the executor, so the executor must listen |
| Cline hub | `session.create` takes `toolPolicies`, `autoApprove`, `cwd`, `systemPrompt` from the client; approvals answerable by anyone | the client sets the executing side's policy |
| Cline hub | loopback `Origin` accepted as a credential | any local page or process becomes a controller |
| goose roaming and serve | a peer gets the full ACP surface, including `config/extensions/add` (a stored stdio command) and an unforced working-dir update | the controller adds code to the host and moves its root |
| OpenHands | start request carries `tool_module_qualnames` (imported by the server) and container limits (forwarded uncapped); a server-level bash endpoint | the controller chooses code and resources |
| OpenHands, OpenCode | the stack's own key reachable from the agent's shell | the executed agent can act as the controller and approve itself |
| DeepSeek external sub-agents | the child's permission mode is fixed per provider, independent of the parent | delegation escapes the parent's limits |
| Warp | unattended CLI children launched with their own permission systems off; ACP policy allows every non-denylisted request and prefers `AllowAlways` | the controller turns off the executor's guards |
| Zed | its `tool_permissions` are not applied to external agents; `terminal/create` runs unconfined | the client's own policy does not bind the tools it serves |
| Hermes SSH | sync-back writes remote files into the host's skills tree | the executor writes into the controller's instructions |
| pi-env | the daemon trusts every request on stdin | no executor-side policy at all |

The node must treat every `MasterToNode` field as a request that `node.toml` can only narrow:
`approval.respond` (an allow from the master must still pass `node.toml`'s `denied_tools` and
`max_approval_mode`), `session.create` (cwd inside `workspace_roots`, agent spec within what the
node allows), grants (refused when `model_keys` is `local` or `none`, as the design note says),
container and resource requests (caps from `[containers]`, as OpenHands should have done), and
anything that names code to load (refuse outright). Pull request 6's server-side enforcement of
where an agent may run is master-side narrowing; good, but the node must check again.

Copy the opposite cases: OpenCodex's executor intersects requested capabilities with what it
allows at the per-session handshake and refuses a presence that claims more than was enrolled
(`crypto.ts` L214-L286, `workspace-hub.ts` L427-L463); OpenClaw's node refuses a delayed approval
when its own policy snapshot changed and refuses PATH overrides (`invoke-system-run.ts` L349-L357,
L265-L290). Note that OpenCodex's reviewer suggested the hub should decide the capability set at
enrolment; for us that is backwards. Our node declares its ceiling in `node.toml`, the master
narrows.

**Tests.** A master `approval.respond` that allows a tool `node.toml` denies is refused and
logged by the node. A `session.create` whose cwd is outside `workspace_roots` is refused. A
grant sent to a node with `model_keys = "none"` gets an ack error. A container request above the
node's cap is clamped or refused, never applied.

##### 6.4 (Rank 3) Resume: durable events by cursor, deltas transient

**Durable stream, node to master.**

- Every event frame carries `(session_id, owner_epoch, seq, hash)`. The node's sender is a
  **cursor over the node's own log**, not an in-memory queue: it keeps `next_to_send` and
  `acked` per session and reads from the log. With at most W unacknowledged frames per link
  (a count and a byte cap), a slow master makes the sender wait, while the agent loop keeps
  appending locally because the node's log is the truth. This removes any need for an unbounded
  buffer on either side (AUDIT L1) and makes pull request 4's bounded channel safe to fill.
- The master acknowledges `(session_id, seq)` only after its mirror transaction commits,
  including the fence predicate (OpenClaw updates its ACK cursor only after `assertCurrent`,
  `worker-turn-rpc.ts` L279-L320; Hermes acks after durable admission).
- On reconnect the master sends `resume { session_id, after_seq }` with its mirrored head
  (we have `Resume{after}`); the node rewinds the cursor; duplicates are absorbed by the hash
  rule in 6.1.

**Transient stream.** `delta { session_id, owner_epoch, item_id, rev, text }` and tool
progress: no `seq`, not stored, coalesced latest-wins per `item_id`, dropped first under
back-pressure, never replayed. The durable `assistant_message` event carries the same
`item_id` and supersedes the deltas. This is OpenHands' envelope (`session-frames.ts` L1-L79:
`durable` with `seq`, `transient` and `delta` without) and DeepSeek's dense `revision` on
assistant frames. It also fixes AUDIT D5 (every delta stored forever): stop writing deltas
into the event log. Our own inventory says deltas are stored today
(`crates/core/src/event.rs:59-80`); the brief says the design separates them. Settle that
contradiction in code before building resume on top of it.

**Master to browser.** `subscribe { session_id, after_seq }`; the master registers the listener
first, reads the mirror up to a captured watermark, sends `sync { through_seq }`, then live
events (OpenCode's log contract, `bus.ts` L829-L878; DeepSeek's "listener armed before the
snapshot"). Each socket gets a bounded queue; on overflow close it with a "lagging" code and let
the client reconnect with its cursor (OpenCode fails the subscriber at 4,096; DeepSeek's
unbounded `Deque` is the bug to avoid). Deltas reach the browser only live; on reconnect the
client discards open streaming slots and waits for the durable event (OpenHands
`conversation-websocket-context.tsx` L1305-L1312). Because our seq is contiguous per session
with one writer, the cursor is "highest contiguous seq"; we never need OpenHands' 1,024-hole
heuristic.

**Master to node commands.** Every `session.prompt` carries a client-generated `req_id`
stored in the node's `user_message` event; the node acknowledges a repeat as already accepted
(DeepSeek `commands.ts` L389-L413). Every `approval.respond` names the `approval_id` and the
session epoch; the node accepts it only if an `approval_asked` event exists without a decision,
and rejects a second, conflicting decision (goose `tool_confirmation.rs` L148-L186). Pending
approvals are derived from the log, so a reconnecting UI rebuilds them (DeepSeek keeps them in a
process `Map` and loses them on restart).

**Node restart mid-turn.** Append closers with fixed wording (DeepSeek: "outcome is unknown ...
do not retry blindly", `repair.ts` L31-L41); never re-run a non-idempotent tool on resume (Pi
durable commits `replay: "unsafe"` before the effect, `tool.ts` L195-L227); never auto-continue
a turn whose node was lost (OpenClaw `canContinue = false`, `worker-node-loss.ts` L3-L15);
settle unattended routine turns as interrupted at boot so a routine queue does not stall
(Hermes `session_unattended_unknown.py` L41-L66), while a person's turn waits for the person.

**Tests.** Kill the link mid-turn, reconnect: the mirror has every durable event exactly once
and no deltas. Stall the master's mirror writes: the node keeps appending, memory stays bounded,
and the sender catches up after. A browser that stops reading is disconnected, reconnects with
its cursor, and sees no gap and no duplicates. A prompt resent with the same `req_id` after a
reconnect produces one user message.

##### 6.5 (Rank 4) Credentials on the node and the link key

- The node's link key and any grant must be unreachable from the agent's process tree.
  OpenHands puts its stack key in the agent's environment on purpose
  (`dev-with-automation.mjs` L975-L985); OpenCode leaves its password in shells outside stdio mode
  (`server-process.ts` L85-L97); in both, a prompt-injected agent can approve itself. Cline
  captures its spawn secret into memory and deletes it from the environment before spawning
  anything (`host-bridge-auth.ts` L4-L47); pull request 4's allow-listed empty child environment
  is the stronger form. Test: the key file and `$CREDENTIALS_DIRECTORY` are not readable from a
  sandboxed shell, and no grant value appears in a child's environment.
- Any node-local socket (a future control socket, an MCP bridge) must authenticate its peer or
  be hidden from the sandbox. Codex hides its unauthenticated daemon directory with bubblewrap and
  refuses legacy Landlock for that reason (`linux_run_main.rs` L386-L393); that Landlock cannot
  stop `connect()` to an existing Unix socket is the author's inference, and it matches pull
  request 4's recorded residual (pathname Unix sockets before Linux 7.1). Test: a sandboxed
  command cannot connect to any socket the node opens.
- Grants: OpenClaw keeps keys on the controller and brokers inference by model reference,
  refusing an unapproved model or a profile suffix (`inference-runtime.ts` L122-L153), and it
  also ships a worker-local mode. The design note's `ModelAccess::Proxy` slot is the same idea;
  keep it reserved, and bind each `Direct` grant to session, epoch and turn as in 6.1.
- Store only hashes of any bearer the master issues. OpenClaw hashes worker credentials but keeps
  device tokens in plaintext (`pairing-token.ts` L4-L18); OpenCodex hashes device bearers
  (`workspace-hub.ts` L377-L425). Our node identity is a key pair, so this applies to pair codes
  (the design note already compares hashes) and to any future per-client API tokens.

##### 6.6 (Rank 5) Pairing

The design note's pair code (§4.4) is in line with the best surveyed (OpenCodex: single use,
10 minutes, hashed, rate-limited per source; OpenCode: atomic consume). Copy three details:
**consume the code before validating the rest of the request** (OpenCodex deletes the grant
first, so a rejected request still burns it; `workspace-hub.ts` L377-L425); refuse any request
carrying an `Origin` header on the pairing path (OpenCodex `serve-options.ts` L484-L513); and
answer every failure with one opaque reason while charging only failures to the limiter
(OpenClaw `worker-connection.ts` L218-L262). Never let a pairing flow print a secret on a remote
stdout and read it back (Cline's SSH remote does, `remote-environments.ts` L1436-L1490), and never
put a secret in a URL or a flag value (Codex accepts a token only from a named environment
variable and only over `wss://` or loopback, `cli/src/main.rs` L2649-L2676).

##### 6.7 (Rank 6) Back-pressure without deadlock

Pull request 4 bounds the link (8 inbound, 64 outbound). Two rules from Codex to check against it:
reject *new requests* with a retryable error when a queue is full, but never drop *responses or
events* (Codex `transport/mod.rs` L231-L262); and make sure a full queue of tool output cannot
block an approval reply or a cancel (our per-session lanes answer approvals inline; add a test
that a node streaming at the cap still processes a cancel within the grace period). With the
cursor sender of 6.4, the node-to-master direction never needs to drop anything.

##### 6.8 (Rank 7) Listener authentication on the master

Our master already re-checks login on long-lived sockets (60 s recheck, close on logout); Codex
does not re-check expiry after the upgrade, so we are ahead there. Copy DeepSeek's fence order on
every `/api` request and upgrade: Host (loopback, bind address or `--allowed-host`), then
`Sec-Fetch-Site: cross-site` refused, then `Origin` must equal the Host authority, then the
credential (`api-request-trust.ts` L121-L151). Consider DeepSeek's refusal of wildcard binds
(it names one interface instead), which is stricter than our "explicit, logged" rule. The node
opens no listener; keep it that way (OpenClaw's provisioning SSH and Cline's remote hub both
require the executor to listen).

##### 6.9 (Rank 8) Version gate at admission

OpenClaw refuses a worker whose bundle hash or version differs from the expected build
(`admission.ts` L79-L186); OpenHands refuses an Agent Server older than `minimumAgentServer`
(`agent-server-compatibility.ts` L253-L318). Our `PROTOCOL_VERSION = 1` should become a minimum
plus a feature list in `node.hello`, so the master refuses an incompatible node before it
receives sessions and the UI can show which optional features a node supports.

##### 6.10 (After Gate 3) Protocol and client surfaces

- Generate the web UI's TypeScript types from the Rust protocol types and fail CI on drift
  (Codex `export.rs` L125-L150; OpenCode generates clients from one API definition).
- Put the ordering key of each master API method in the method table, not the handler (Codex
  `common.rs` L129-L139).
- If we ever offer ACP, copy Hermes' shape: a thin adapter that attaches to the node-owned
  session and never runs tools (`acp_adapter/entry.py` L205-L215). If we ever host another
  vendor's agent as an ACP client, start from DeepSeek's: empty client capabilities, no MCP
  servers, decline every approval by default, scrubbed environment, and derive the child's mode
  from the node policy rather than fixing it per provider.
- A "human approval" class that headless clients structurally cannot answer (Codex
  `outgoing_message.rs` L414-L430) is a cheap primitive for routines.

### Session storage, resume, fork and checkpoints

From [round2/synth/06-sessions-fork-checkpoints.md](round2/synth/06-sessions-fork-checkpoints.md).

State today (`research/round1/own-rizzy-harness.md`, `docs/nodes-credentials-environments.md`): an append-only per-session event log written only by the owning node's actor; resume rebuilds history from the log and closes interrupted tool calls synthetically; no fork; a shadow repository (cleared environment, reading the user's objects through alternates, 120 s git timeout) snapshots the folder before and after each agentic turn (`git add -A` into a private index), keeps the latest 10 (`checkpoints_keep`), and restores exact bytes for a turn or "from that turn on" with a forced restore when files changed since. The design note adds a worktree per session, the shadow repository keyed by work-tree path, a master-side `session_forks(session_id, parent_id, through_seq)` table, and a fork that writes imported history as `history_imported` events on the new node and copies no files. PRs 4, 6 and 7 do not touch the log or checkpoints; PR 4's bounded link matters only for item 9.

Ranked by importance.

**1. Fork: copy the prefix into the child's own log, and record lineage inside that log.** (design note)

Our log is linear with one writer per node, so fork does not need in-file branching. Record, written by the child's owning node as its first events:
1. `SessionForked { parent_session_id, root_session_id, parent_node_id, through_seq, through_turn_id, cut: "completed_turn" | "closed_open_turn", lossy: bool, files: "none" | "checkpoint" | "forge", files_ref }`. `through_seq` mirrors the master's `session_forks.through_seq`; the log copy survives the master row being lost or the parent being deleted.
2. `history_imported` batches as in the design note, each event tagged `inherited: true`.
3. `SeedEnded { inherited_count }` (DeepSeek's `session/end-seed`).

Why: DeepSeek's `isSeeded`, `inheritedEventCount` and the end-seed marker are what let duplicate-prompt detection and the fold distinguish inherited from new history (`packages/api/session-controller/src/commands.ts` L404-L407; `packages/core/session/src/fork.ts`). OpenCode zeroes the child's cost and tokens in the fork projector (`projector.ts` L121-L160), and goose carries non-unique message ids; usage totals and idempotency keys need the same care in ours. Copy semantics keep deletion simple (Codex pins the parent only during prepare; Pi's path-only parent link orphans on rename). Pointer-over-file (Codex) is not worth it for us: the child runs on a different node than the parent in the design note and cannot read the parent's file.

Cut rules, each from a project that got it right or wrong:
- Default cut is the last completed turn (DeepSeek's web default). A cut inside an open turn closes it with fork-specific synthetic results and `cause: forked` (`repair.ts` L31-L98, `fork.ts`), so the fold does not render an "interrupted" turn.
- Refuse while the parent turn runs unless the cut is at or before the last completed turn (Hermes requires an idle parent; goose's truncate checks nothing).
- The design note's import is lossy (tool calls cut to one line, 2 MiB per batch). `lossy: true` lets the UI say so and keeps a later export from presenting the child as the full transcript.
- The child starts at approval `ask` with no live connections (Hermes `hermes_state_mutation_branch.py` L34-L49). Check that our fork does not carry the parent's PR 4 "allow for this turn" state.

**2. Store the head explicitly; do not infer it from the last row.** (our log; applies to the existing "rewind from that turn on" and to any later edit-and-resubmit)

Evidence: Pi derives the leaf from the last entry, so a navigation with no following write is lost (inferred; reviewer agreed); Oh My Pi patched the same flaw with `activeLeafId` on off-branch writes; only Codex records the active history explicitly and swaps it by compare-and-set. Recommendation:
- Turn events carry `turn_id` and `parent_turn_id`. The active path is the chain from `head_turn_id` to the root; ordinary appends extend it.
- Add `HeadMoved { to_turn_id, reason: rewind | edit | fork_cut, expected_head }`, appended by the node actor, so the move is a compare-and-swap inside the node (Hermes `RewindPlan.expected_active_ids`, `hermes_state_rewind.py` L64-L77; Codex `replace_rollout_path_if_current`, `revert_thread.rs` L138-L156). The master keeps the head as a projection, never as the authority.
- Events that must not move the head (late usage, a background result for a finished turn, title updates) name their turn or carry `off_head: true` (Oh My Pi `activeLeafId`, `session-manager.ts` L3595-L3628; its `model_usage` re-journaling for discarded turns, L4089-L4108, is the model for billing).
- Rewind never deletes events (OpenCode's commit deletes rows and leaves the log as the only recovery, and only when persistence is on): the fold skips turns off the active path; abandoned turns stay for audit and export.
- Check first in our code: today a rewind restores files; does the fold still show the rewound turns as active after a restart? If so, files and conversation disagree after a restart, and a `HeadMoved` event fixes it even if we never build in-log branching.

**3. Restore: add the safeguards our forced restore lacks.** (destructive write to the user's tree: full stress test per `CLAUDE.md`, plus an abuse-case test)

1. Snapshot the present first and make it a checkpoint the user can restore to (Cline `checkpoint-restore.ts` L50-L154; Hermes pre-rollback, `checkpoint_manager.py` L1387-L1414; OpenCode `stage` and `clear`). If the snapshot or its rollback fails, refuse to continue (Cline throws `AggregateError`; Hermes' curator continues unprotected, `curator.py` L951-L958, the choice to avoid). Record `Restored { from_tree, to_tree, pre_tree, paths_changed, paths_skipped, forced }` so restore is itself undoable. Check whether our restore already takes a pre-restore snapshot; the inventory does not say.
2. Compare-and-swap on the tree we believe is there. The request carries `expected_tree` (the last post-turn snapshot). The node takes a fresh capture; if the tree id differs, it returns the changed paths (user edits, or work outside a turn) and requires an explicit choice per the existing forced flag. This is Cline's `update-ref HEAD new old` idea applied to our tree id. It is better than Hermes' write ledger, which only knew file-tool writes and therefore skipped exactly the shell-made files (`checkpoint_manager.py` L986-L1050, with the three exceptions in section 2).
3. Refuse while a turn runs on the session, and while another session shares the worktree. OpenCode returns a busy error (`handlers/session.ts` L445-L458); Hermes needs an idle parent. Add a test for restore during a turn.
4. Restore only attributed paths when "from that turn on" spans turns: the union of `Checkpoint.files` of the turns being undone, taking each path's earliest start tree (OpenCode `revert.ts` L22-L56). This bounds restore cost; unattributed paths are left alone and reported.
5. Never half-restore. Refuse a whole-tree restore when the tree holds a nested repository at a captured path (Hermes `checkpoint_manager.py` L1376-L1385; OpenClaw `snapshot-inventory.ts` L406-L434). Check what our `git add -A` does with a nested `.git` directory.
6. Keep what no snapshot holds. Files over our size cap are not in a tree; restore must not delete them (Hermes `checkpoint_manager.py` L1429-L1452). Record skipped oversize files in the checkpoint event so the UI can warn before a restore that would delete one.
7. Validate every ref name and path that reaches git from a stored event or the protocol (OpenClaw's `^refs/openclaw/worker-results/[A-Za-z0-9-]+$` at `session-repository-checkpoint-refs.ts` L9-L23). The node decides: a restore request must name a tree id that exists in the node's own shadow repository, never a path.
8. Say in the UI what a restore does not cover, as Cline does: files outside the work tree, installed packages, databases, ignored files (the shadow repository honours ignore rules, so `node_modules` and `.env` are not captured), and anything a container wrote outside the bind-mounted worktree.

**4. Measure and bound checkpoint cost without giving up shell and container coverage.** (our `crates/node/src/git.rs`, `session.rs`; the design note's per-session worktree)

Where we are: a full `git add -A` scan twice per agentic turn. What the input shows: OpenCode's mitigations lower constants but each capture stays O(working tree); Zed removed its per-prompt version and its surviving recipe has the same `git add --all` shape (the reason is inferred); Hermes bounds cost by skipping above 50,000 files, which loses coverage silently; Cline has no cap; DeepSeek bounds only its display recorder. No project in the input bounds a full-coverage checkpoint without losing coverage or accepting O(tree), and nobody measured it.

Step 1, measure before changing anything. Add to the `Checkpoint` event: `{phase, scan_ms, git_calls, files_stat, files_changed, bytes_hashed, objects_written, index_reused, timed_out, coverage}`. Add a node benchmark over synthetic trees (for example 1k, 10k, 100k, 500k files; warm and cold page cache; 0, 10 and 1,000 changed files) and run it in CI as a regression guard. Field data comes from the event, aggregated per node. Set budgets after seeing the numbers; this report does not propose a number it cannot cite.

Step 2, apply the cheap wins the input shows (all `unchecked`; measure each):
- Keep the private index between turns, seeded from the user's index; set `core.untrackedCache=true`, `feature.manyFiles=true`, index v4, `core.fsmonitor=false` (OpenCode `git.ts` L28-L42, L503-L520; Cline persists its scratch index so git's stat cache skips unchanged files, `checkpoint-hooks.ts` L249-L338). Check whether our private index persists across turns today.
- Write the index to a temp file and rename it; rebuild from the user's index if unreadable (OpenCode `git.ts` L704-L714).
- Coalesce concurrent captures and return the previous tree when nothing changed (OpenCode `git.ts` L760-L762, L845-L871).
- Take the pre-turn snapshot lazily, just before the first write-class or shell tool of the turn; record "no checkpoint: read-only turn" otherwise. Hermes and Cline tie the snapshot to the first write-class call or first model call; our turn-start snapshot pays the scan for read-only turns. The post-turn snapshot is needed only if a write-class or shell tool ran.
- Reuse the previous post-turn tree as the next pre-turn tree when the work tree cannot have changed between turns. That needs a cheap "unchanged" signal. Candidates not in the input and therefore unverified: git's built-in filesystem monitor (as distinct from a hook program, which stays off) or an inotify flag kept by the node. Test before relying on either.
- Batch git plumbing (Cline `checkpoint-diff.ts` L67-L105 uses two `git cat-file --batch` processes).

Step 3, bound it with a degradation ladder, not a skip. Every limit produces a recorded `coverage` value on the `Checkpoint` event; none may silently drop the snapshot (Hermes' 50,000-file skip is the pattern to avoid):
1. `full`: the scan finishes within the budget. Covers tool-made, shell-made and container-made changes to the bind-mounted worktree, because the node runs git on the host against the worktree and container writes appear as ordinary files (the design note's `keep-id` mount). This is the coverage Hermes gives up by excluding container backends.
2. `partial`: the scan exceeds the budget. Stop, then fall back to paths we know changed: paths written by our file tools (Warp derives its handoff set from the last 500 tool calls; Zed's action log keeps a baseline at first touch) plus a bounded walk for files newer than a marker. The walk is our proposal, not an input finding; test its cost. Record `coverage: partial` and show in the UI that shell-made changes may be missing for this turn.
3. `files_only`: the tool-reported fallback only. This is Zed's level: no shell coverage, no repository scan.
Expose the budget in `node.toml` (the node decides) with a conservative default and a hard per-command timeout. Our 120 s per git call is too long to sit inside a turn; DeepSeek uses 30 s.

Step 4, bound storage as well as time. `checkpoints_keep` counts turns; add a byte cap for the shadow repository and a per-file size cap; a file over the cap is excluded from the tree and listed in the event (so restore keeps it, item 3.6). Garbage-collect while keeping objects reachable from retained refs: OpenCode's store has no refs, so `git gc` would delete everything and it packs by explicit object list; Hermes turns `gc.auto` off and trims by snapshot count and bytes. Test pruning against a repository with refs and with alternates.

Step 5, test the alternates risk. OpenCode's inferred risk: if the user's repository runs `git gc --prune` after history is rewritten, objects that a snapshot tree references only through the alternate can disappear and a restore fails. Our shadow repository also uses alternates (own inventory, Checkpoints row). Write the test: snapshot, rewrite history and gc the user repository, restore. If it fails, copy referenced blobs into our own object directory at snapshot time (cost: disk) or stop using alternates for tracked content.

Step 6, keep per-turn pairs. OpenCode snapshots at the start and end of every model step; Cline once per user run; Hermes once per directory per turn. Do not go per-step unless measurement shows the scan is cheap.

**5. Resume: copy three things, keep one thing of ours.** (our log, resume path)
- Flush before effect, fail closed. DeepSeek awaits a durable flush before the model request and before every top-level tool body and treats a failed flush as an error (`session-checkpoint-policy/src/index.ts` L29-L83; `checked in review`). Check that our node's append is durable before tool dispatch and before the provider request; test a full disk.
- Close the open turn with a log event and fixed wording. We already close interrupted calls synthetically. Compare our wording with DeepSeek's ("outcome is unknown ... retry only if read-only or idempotent ... do not retry blindly", `repair.ts` L31-L41) and OpenClaw's prompt telling the model to check files, processes and external actions first (`session-restart-resume.ts` L78-L85). The closer must be an event so the fold is the same for live and recovered sessions.
- A bounded attempt counter on restart. OpenCode caps resume at 10 attempts and then ends the turn `aborted`; pi-durable counts crashes per task and resets on progress. A node that crashes on the same tool must not loop. Do not re-run a tool with a side effect (pi-durable defaults to `unsafe`).
- Keep our rule that the owning node is the only writer. Codex's and DeepSeek's locks are single-host mechanisms; OpenCode, goose and Pi have nothing across processes; OpenClaw's owner epoch (global, never reused, checked on every worker request) is the model if a session ever moves between nodes. We do not need it now; we will if restore on a different node or node replacement is built, so put a generation in the node-link binding before then.
- Fold cost: Codex replays only from the newest full compaction; DeepSeek loads the whole log; Pi rebuilds from all entries per projection. Start our fold at the newest compaction event. If folds are cached (DeepSeek's projection cache), bind the cache to the log's identity (`createdAt`, `cwd`, inherited count, format version).

**6. Retention, export and import.** (AUDIT D5 growth; `CLAUDE.md`: no secret in an event)
- Retention: OpenClaw is the only project with a disk-budget trigger (30 days, 5,000 entries, 10 GiB, oldest first, `enforce` by default). Codex compresses after 7 days and never deletes. Oh My Pi and Hermes keep forever unless the user opts in. For us: compress cold sessions and cap total bytes per node; deletion stays explicit (the design note's deletion flow).
- Export: refuse an export if redaction fails rather than falling back to raw (Hermes `sessions_cmd.py` L552-L554). Pi's HTML export embeds every branch plus the system prompt and tool schemas with no redaction (unchecked); DeepSeek's ZIP has none (`absence confirmed`); OpenCode's `--sanitize` redacts metadata, not message text.
- Import of other harnesses' transcripts: goose converts Claude Code, Codex rollout and Pi JSONL; Oh My Pi converts Claude Code and Codex; Cline converts Claude Code, Codex and OpenCode. If we copy it, write one converter per format into `history_imported` events with `inherited: true`, run every imported string through the same redactor as live output, and never resolve code or tool names from imported content. OpenHands' stored `tool_module_qualnames` is a load-time code-execution surface; our resume path must never resolve code from stored session data.

**7. Avoid, with the evidence.**
- Do not make the log optional. OpenCode's shipped server leaves it off and its revert commit deletes projection rows.
- Do not decide when to snapshot from command text (Hermes: misses `bash -c`, `find -delete`, `python -c`, `tee`; `2>&1` triggers needless snapshots).
- Do not build a restore filter from a ledger fed only by file tools (Hermes).
- Do not store checkpoints as refs in the user's repository without a cap (Cline: no size, count or total cap; refs live until the session is deleted).
- Do not rewrite a log in place on migration without a temp file and rename (Pi). We have no tag yet, so per `CLAUDE.md` the schema changes in place; decide before the first tag whether unknown events are refused unless marked ignorable (DeepSeek) or skipped (Codex).
- Do not describe a checkpoint the code does not take. Oh My Pi's `checkpoint` tool text says "git-based" while it only rewinds the conversation. Ours should say, per rewind, which of conversation and files were restored.
- Do not count on fork to carry files; no project does it well (item 8).

**8. Fork with files: use our own checkpoints.** (the design note says "a fork copies no files; files come back from the forge")
No input fork restores files from its own checkpoints; Cline's restore-with-messages is the closest. We hold a tree id per turn in the shadow repository. For a same-node fork, seed the child's new worktree from the tree recorded at the cut turn (`git read-tree` and checkout, as Zed's archive restore uses `read-tree`), and record `files: "checkpoint"` with `files_ref: <tree id>` on `SessionForked`. For a cross-node fork the objects are not on the child's node: keep the forge route as the default; a bundle transfer of the tree would be new work no input project demonstrates (`inferred`, untested). Decide `files` explicitly at fork time and show it; the failure to avoid is a forked chat whose history says files exist that the child's worktree does not have.

**9. Reconnect cursor.** (small; uses PR 4's bounded link) OpenCode's first-party clients refetch everything on reconnect because the cursor API goes unused; DeepSeek's `follow` has no cursor in the request (docs and code disagree) and an unbounded listener queue; OpenHands and Cline resume by sequence number. Our `Resume{after}` exists on the node link; give the browser stream the same contiguous-`seq` cursor and OpenCode's order: subscribe first, capture a watermark, replay to it, emit a sync marker, then follow (`bus.ts` L765-L878, `checked in review`). Add a per-client bounded queue with a lag disconnect; it composes with PR 4's back-pressure.

### Context: prompts, compaction, instruction files, memory

From [round2/synth/07-context-compaction-memory.md](round2/synth/07-context-compaction-memory.md).

State assumed: `main` has no compaction (inventory not re-read here); pull request 7 (open, not merged) has compaction keyed to the model's context window, one overflow recovery, provider retry with backoff, and a summariser that reads in pieces; pull request 4 adds the Landlock shell sandbox, an allow-listed child environment and secret redaction; pull request 6 the agent roster; the design note proposes per-turn provider key grants. Claims about our code come from the task description, not from reading it.

##### 6.1 What pull request 7 should still adopt, ranked

**1. A cache-preserving summary request, with the "pieces" reader kept only as a lower tier.** Copy OpenCode (`packages/core/src/session/compaction.ts`: the request builder around L451-L470 and L853-L870 sends the primary agent's own system parts, tools and history plus one final user message with the summary prompt) and DeepSeek harness (`packages/compaction/compaction-basic/src/summarizer.ts` L120-L181: same prefix, instruction last, full tool schemas still sent, tool calls in the reply dropped, empty, image-only or `max-tokens`-truncated output an error). Zed (`crates/agent/src/thread.rs` L4764-L4835) keeps the tools with `tool_choice: none` for the same reason. Three conditions follow from the code: leave headroom for prompt plus output in the trigger (OpenCode's buffer, DeepSeek's 65,536 that also equals the summary `maxTokens`); only expect a cache hit with the same model and an unexpired cache (OpenCode ties its precompute to the cache lifetime minus 30 s); and give the fallback tiers below, because the full-prefix request is the largest request you make. OpenCode's `deliver` ladder is the model for tiers (unchanged request, history minus the kept tail, then a flattened transcript with progressive clipping: tool output to 2,000 chars, then tool output dropped, then user text only, then oldest exchanges omitted). A reader that summarises in pieces fits that last tier. None of the findings describe a map-then-merge chunked summariser as a primary path: Hermes sends exactly one call with an evenly sampled input and elision markers, Pi splits only a split turn into two summaries, Cline refuses its own agentic summariser during overflow. Treat the pieces reader as a hypothesis to test, not an established pattern (absence not confirmed). Pi (`cacheRetention: "none"`, history re-sent and re-billed uncached) and goose (cache writes off) are the examples not to follow. Related to our design: a summariser override model needs its own per-turn key grant (inferred from the design note).

**2. A deterministic fallback after an overflow, so a context-length error never ends in a dead turn.** Copy Cline: `sdk/packages/core/src/extensions/context/basic-compaction.ts` L452-L512 (`basic`: no model call, keeps every typed user prompt, collapses dropped tool work into a notice, keeps the last three assistant texts, strips stale attachments) and `sdk/packages/agents/src/agent-runtime.ts` L1378-L1423 (one forced compaction and one retry; during overflow `agentic` is never used because its own request could overflow). Add Hermes' rule from `agent/context_compressor.py` L4080-L4175: terminal failures (network, truncation, empty content, auth) abort and leave the session unchanged instead of inserting a placeholder, other failures insert a deterministic summary of user asks, actions, files and errors (L3502-L3510). Codex's oldest-first drop inside compaction (`core/src/compact.rs` L347-L359) is the last tier. Do not copy Codex's sampling path (the turn ends; `core/src/session/turn.rs` L2028-L2031), Pi (a second failure stops) or goose (the fourth overflow gives up). Order for us: model summary (cache-preserving), shrink request, deterministic `basic`, drop oldest, typed error. Test: a summary request that itself returns "too long" must end in the deterministic tier with the history shrunk, never in an error loop.

**3. A check that compaction made the context smaller, at two levels.** DeepSeek `region.ts` L417-L468: a summary is rejected unless its framed estimate is strictly smaller than the span it replaces, and automatic compaction also demands the surface be unchanged during summarisation. Cline `agent-runtime.ts` L2254-L2306: the retry is refused unless the compacted request shrank. Oh My Pi falls through to the next method when a summary would not reduce context (`packages/coding-agent/src/session/session-maintenance.ts`; the fall-through ranges are listed in its account in section 2). Add Hermes' breaker (`context_compressor.py` L3015-L3022): two compactions that did not bring usage under the threshold stop the automatic attempts for 300 s, which prevents a compaction loop. Codex, OpenCode, Pi, goose and Zed do not check (OpenCode checks only the summariser's input). Test: a summary longer than the kept span, and a history where the kept tail alone is above the threshold (Pi keeps a single recent tool result over 20,000 tokens in full, so its tail can still exceed the trigger).

**4. A self-correcting estimator.** Copy Cline `sdk/packages/core/src/extensions/context/compaction.ts` L211-L230: scale the budget by the ratio of the provider's real count for the previous request to the estimate when the real count is higher (cap 4x). Add Pi's rule (`packages/coding-agent/src/core/compaction/compaction.ts` L298-L349): ignore provider usage recorded before the last compaction. Add Oh My Pi's floor (`packages/agent/src/compaction/compaction.ts` L355-L381): never let the count fall below a local estimate of the stored conversation. Count images explicitly (OpenCode: 1,500 per image and 2,000 per PDF; Pi: 4,800 characters per image) and score a request-too-large (413) in bytes, not tokens, as Hermes does (`agent/turn_overflow.py` L219-L275). Adopt a provider-reported limit from an error message, never a guessed one (L315-L340). Decide what an unknown window does: OpenCode's infinite ceiling means auto compaction never fires and is the worst default; Cline's 128,000 fallback for manual and overflow is safer.

**5. Recover from overflow by compacting and restarting the same step, once, and hide the failed attempt.** OpenCode (`session/runner/step.ts` L186-L197, `llm.ts` L316-L322: once per step, only if no output started) and Pi (`agent-session.ts` L3132-L3202: `context_edit` entries hide the failed assistant message from the model while the raw log keeps it; a second failure stops with a clear message). In an append-only log this is the natural form: the failed attempt stays recorded, the projection hides it.

**6. Check the summariser's window, but not the way Hermes does it.** Hermes (`agent/conversation_compression_feasibility.py` L64-L127, `conversation_compression.py` L2028-L2117) probes the summariser's window, lowers the trigger to it and refuses below a minimum; the reviewer noted the lowered trigger equals the whole summariser window, leaving no headroom for prompt and output. Oh My Pi's candidate order (configured model, current, role models, largest context) avoids the check by selection. We should do both: pick a summariser whose window is at least the history plus the prompt plus output, else go to the fitted-request tiers in item 1.

**7. Record compaction so a one-writer log can replay it, and test the crash case.** Our log is append-only with contiguous `seq`, written by the owning node. Pi's entry (`firstKeptEntryId`, `tokensBefore`, system-prompt snapshot; written only after the summary exists; the model context is a pure function of the entries) and Codex's `replacement_history` base (resume replays only the suffix; `core/src/session/rollout_reconstruction.rs` L205-L260) fit that. DeepSeek's start and end events show the cost of a two-event protocol: a start with no end blocks further compaction until a later `session/end-seed`, and a summary committed with a failed close leaves the surface replaced and the lock open. Write one event after the summary exists, or make the closing event idempotent. Re-base instruction state at compaction (OpenCode `projector.ts` L1119-L1122 and `instruction-state.ts` L126-L140; Codex renders everything in a fresh window) and test that AGENTS.md, the environment block and nested instruction files are present after compaction; DeepSeek's compaction path does not re-inject them and goose's subdirectory hints drop out.

**8. Keep the prompt prefix stable with keyed sources and logged diffs.** Prompt and tool changes as transcript entries is the shape that fits our event log. Copy OpenCode (`session/instruction-state.ts` L49-L89, `message-updater.ts` L154-L166: a hash per canonical-JSON value, a durable event holding the delta and the already-rendered text) and Codex (`core/src/session/world_state.rs`, `core/src/session/mod.rs` L4180-L4207: stable section ids, snapshot, render the diff; fresh windows render everything). Avoid Codex's pile of legacy-fragment matchers and, from OpenCode, the model-id substring match without a provider check.

**9. Instruction files: read on the node, gated by the node.** The node's `node.toml` decides. Copy Codex's `core/src/agents_md.rs`: an untrusted project returns nothing (L65-L67), root markers are read only from non-project layers (L207-L212), one combined budget with a warning on truncation (L69-L76, L148-L182). Add a per-file cap (Hermes: 20,000 chars or 6% of window; OpenClaw: 20,000 and 60,000 with a digest of must/never lines so a cut file still keeps its rules). Avoid Pi's rule (no cap, walk to the filesystem root, loaded regardless of trust, so a hostile repository's AGENTS.md steers the model and the model can then write `.pi/extensions/x.ts`) and Codex's empty-override shadowing. Hermes' regex scan blocks a whole file on one false positive; use a scan as a warning, not as the gate. Whatever the file says, nothing in it may widen node policy (invariant).

**10. Tool output.** Keep what we have (capped file tools, bounded node link); if long sessions need it, Oh My Pi's cache-aware prune (`context-settings.ts`, `session-maintenance.ts` L817-L852) and OpenClaw's frozen projection recorded in the transcript are the designs that do not break the prefix cache. DeepSeek's spill fails open to inline on a full disk (`spill-policy/src/index.ts` L95-L131); fail closed with a truncation notice instead.

##### 6.2 What the memory designs here imply for four scopes and approval

We do not know our four scopes beyond "human approval for shared memory". The input implies:

1. **Approval must be enforced in code, not in the prompt.** goose's "always confirm with the user before saving" is prompt-only and unenforced (`crates/goose-mcp/src/memory/mod.rs` L118-L172; checked in review). Hermes enforces it in the write path (`tools/write_approval.py` L201-L248, `tools/memory_tool.py` L64-L90) but the gate fails open if its module cannot be imported and a disk failure while staging loses the write. Shared memory must fail closed: no staged record, no write.
2. **Approve what the person saw.** Hermes pins the matched entry at staging time for `replace` and `remove`, so approval applies to the text the person reviewed. Its skills replay has no base hash, version or mtime check (corrected in review: a staged targeted `patch` still needs its `old_string` to match, so a stale patch normally fails, but full rewrites are unguarded). Our staged write should carry the base version of the entry (event seq or content hash) and be refused if it moved.
3. **Unattended writers must not remove.** Hermes' background review can stage `patch` and `remove` but not apply them; with approval on, every background write is staged. Codex's consolidation and OpenClaw's dreaming write without a person; OpenClaw bounds it with a rule that a plan removing more than 25% of prior entries is rejected. For shared scopes copy the staged-only rule; for personal scopes the 25% bound is a cheap second guard.
4. **A background writer's sandbox is not enough, and the shell can bypass the store.** Codex's consolidation agent has approvals `Never`, no network and memory-root-only writes, but inherits an unsandboxed profile if the parent is unsandboxed (`memories/write/src/phase2.rs` L339-L359). Hermes withholds `terminal` from the curator because a shell `mv` would leave no ledger entry, and states that `terminal` writes bypass its ledger and lock. On a node, the agent's shell can write wherever pull request 4's Landlock policy allows: shared and personal memory stores must live outside the shell's writable set and be written only through the approval path (inferred from our sandbox and the two findings).
5. **Chat-time writes are an injection path into persistent memory.** Codex's ad-hoc note tool and goose's memory tool both let model output become later system-prompt text. Hermes strict-scans writes and blocks matching entries from the prompt snapshot (the live list keeps raw text so the user can delete it); Codex marks threads that touched MCP or web content "polluted" and excludes them from extraction (`core/src/stream_events_utils.rs` L176-L192). Both ideas apply: scan on write, and exclude threads with external content from automatic extraction.
6. **Rollback needs a ledger that is a gate, not telemetry.** Hermes' ledger (`tools/skill_ledger.py` L298-L323, L557-L633) stores before and after manifests with sha256, content-addressed blobs and a `pre-rollback` entry, restores or deletes accordingly, and does nothing if the safety capture fails. But a failed ledger append is logged and the mutation proceeds, so the audit trail can have gaps, and sha256 is content addressing, not a hash chain. With an event log we can make the memory write and its inverse the same event (inferred): rollback is a new compensating event and cannot be skipped.
7. **Scope filtering belongs in the prompt builder.** OpenClaw drops `MEMORY.md` for sub-agent, cron, group and channel sessions (`src/agents/workspace.ts` L896-L917). Our four scopes need the same filter per chat type, tested for the "chats are private to their owner" invariant.
8. **Keep memory small and frozen per session.** Hermes caps notes at 2,200 and 1,375 chars, refuses an `add` over the limit and returns the live entries so the model consolidates; the prompt gets a per-session snapshot refreshed at compaction. goose loads global memories once at construction. Codex injects only a 2,500-token summary and lets the model read detail on demand. All three keep the prefix stable.
9. **Session search is not a substitute.** Hermes' FTS5 `session_search` finds rows archived by compaction without an LLM (`tools/session_search_tool.py`), which means archived history stays reachable after a summary; chats being private per owner means any such search must be scoped to the caller.

##### 6.3 What to check or test in our own code

- Compaction with the summary larger than the span; with an unknown window; with images dominating the size; with a tool result bigger than the kept tail.
- The summariser itself overflowing; the overflow retry landing again on overflow (stops, deterministic tier runs once).
- Crash between summary and log write; restart with a half-recorded compaction.
- After compaction: instruction files, environment and memory snapshot present; no stale diff notices.
- Whether an edited AGENTS.md is re-read mid-session, and whether that is what we want (Codex: not until restart, safe against self-modification but surprising).
- Estimator error with provider-reported usage absent (first request, aborted response).

### Edit tools and the mechanics behind the scenes

From [round2/synth/08-edit-tools-mechanics.md](round2/synth/08-edit-tools-mechanics.md).

**Our state.** On `main`: `edit` is an exact replace that must match once unless `replace_all`, with a diff card; no read-before-edit, no staleness check, no patch tool, no diagnostics; `write` and `edit` open without following links and refuse `.git/**`; per-turn shadow-repo checkpoints scan the whole tree; bash has its own process group, SIGTERM then SIGKILL after 2 s, 600 s default, 3600 s cap on the model's `timeout_secs`, allow-listed env, 30,000 characters head and tail, filtered full text retrievable with `tool_output`. Pull request 4 adds file-tool caps and a Landlock shell; pull request 6 adds the new agent roster (relevant to which agents get edit tools); pull request 7 adds compaction keyed to the window. The design note's worktree-per-session reduces outside edits but does not remove them (shell commands and the user still write).

Ranked by importance. Items 1 and 2 are the design the task asked for.

##### 6.1 A seen-version guard in code, on by default (highest value)

**Evidence.** Seven of the eleven read implementations that have an edit tool have no staleness check (Codex, OpenCode, OpenClaw, Pi, goose, Cline, DeepSeek in the shipped config). The two soft ones only warn (Hermes `patch`, Zed). The hard ones are OpenCodex (`expectedSha256` on `write_file`), Hermes `write_file`, Oh My Pi hashline, and DeepSeek's plugin, which ships disabled while its prompt still demands a read. A guard that is only in the prompt is not a guard (Oh My Pi `replace`, Zed, DeepSeek).

**Design (ours, assembled from the sources; not from one project).**

1. The node keeps a per-session table, keyed by canonical path (after the same symlink-safe resolution `edit` already does): `{ sha256 of normalised content, dev:ino:size:mtime_ns:ctime_ns, ranges shown in full, a bounded copy of the shown text }`. It is written by `read` (and by `grep` or `glob` only as "partial: lines N..M"), and rewritten after every successful agent write with the new hash. Version token shape from DeepSeek `fsio.ts` (`dev:ino:size:mtimeNs:ctimeNs`); hash from OpenCodex and Oh My Pi. Use at least 128 bits of hash, not Oh My Pi's 16 (Oh My Pi's own review: a live collision bypasses recovery).
2. `edit` and `write`-over-existing take the per-path lock first, then check the version **inside the lock, before matching** (DeepSeek `fs-local/src/index.ts#L250-L277`: stale read reports `FS_STALE_VERSION`, not a misleading not-found). Creating a new file needs no prior read (DeepSeek, Hermes).
3. `edit` with no table entry: refuse with "read this file first" and return nothing else (enforced in code; Hermes `write_file` and Oh My Pi hashline do this; Oh My Pi `replace` does not). `write` over an existing file needs a **full** read, not a partial one (Hermes `file_tools_write_guards.py`).
4. Table-entry present, content hash differs from live (file changed since the model last saw it). **What to do on a stale file, in order:**
   a. Re-run the match against the live text. If the matched span is not unique, or does not match exactly, refuse and return the failure with the current nearby lines (OpenCode `missHint`, gated by read permission).
   b. If it matches uniquely and the matched span is byte-identical to the same span in the retained snapshot (the model saw exactly these lines), apply, and say in the result that the file had changed outside the session and which line ranges differ. This is the string-replace analogue of Oh My Pi's "provably safe" rebase, without the line-diff machinery: a content match has no line numbers to remap.
   c. If the span is not in the snapshot (the model never saw it, or the snapshot was evicted), refuse and return a bounded unified diff between the snapshot and live content. That saves the re-read, which Hermes and Pi make the model do.
   d. With no snapshot (hash only), refuse on any mismatch.
   Why not warn-only: Hermes and Zed show the warning being the only effect; a same-mtime change passes Hermes' check. Why not Oh My Pi's full rebase: it is 565 lines plus a tree-sitter dependency, and its own research file recommends rejecting on mismatch until retries are shown to be wasted.
5. Persist what the table needs in the event log: each `read` tool-result event carries `path`, `sha256`, `ranges`. On node restart the table is rebuilt by folding the log (our invariant: the UI and state are a fold over the log). Oh My Pi's and DeepSeek's tables are memory-only, and after a restart their provenance is gone. The snapshot *text* is not logged; after a restart the guard degrades to branch (d), which is safe.
6. **On compaction, clear "ranges shown in full"** (keep hashes). After compaction the model no longer has the text the table says it saw. This is `inferred` from Oh My Pi's TTSR note (fire-once state survives compaction while rule text does not); none of the files test it for read tables. Relates to pull request 7: clear in the same change that lands compaction.
7. Bound the table: Oh My Pi uses 4 versions per path, 256 paths, 64 MiB, files over 4 MiB not snapshotted. Our idle budget is 50 MB RSS, so keep the snapshot text on disk in the session scratch directory and hold only hashes in memory, or cap memory copies at a few MiB.
8. Track our own writes so the agent is not refused for its own edits: after a successful write the entry is replaced with the new hash and the shown range is the touched lines (Hermes' ledger records hashes after its own writes for the same reason). A formatter or shell `sed -i` that changes the file will make the next edit refuse and return the diff. That is the intended behaviour.

**Line-anchored edits.** For a string-match `edit` the match itself proves the model knows the text, so Oh My Pi's seen-line guard adds little. It becomes necessary only if we add line-number anchors, where it is the real per-line protection ("catches edits built from `grep -n` output or hallucinated line numbers"). Do not ship hashline in this round: Oh My Pi disables it for eight model families, its benchmark is the maintainer's own, and its stale-tag recovery is the largest code in the stack.

**Test that fails before and passes after.** (a) read file, change it from outside, `edit` an unrelated span: succeeds with a changed-outside note (4b); (b) same, `edit` a span that changed: refused with a diff (4c); (c) `edit` without any read: refused; (d) restart the node mid-session and `edit`: guard degrades safely (4d); (e) `write` over an existing file after only a partial `read`: refused.

##### 6.2 The match ladder to adopt, and its order

Tier-first, as Codex does (the corrected review text: tiers are tried in order across the whole file). Order, each tier must give **exactly one** match or the next tier is tried; more than one match at the winning tier is an error that lists line numbers (Zed) and never a first-wins (Codex):

1. **Exact** bytes, on an LF view with offsets mapped back (OpenCode `matchView`; DeepSeek CRLF collapse).
2. **Trailing-whitespace-insensitive, line-wise** (Codex tier 2; OpenCode tier 3).
3. **Unicode punctuation fold, one character to one character** (curly quotes to straight, en and em dashes and minus to hyphen, NBSP and exotic spaces to space) so offsets stay valid (OpenCode `normalizeForMatch`; Codex tier 4). Avoid NFKC (Pi, OpenClaw): it changes length.

Stop there for `edit`. Not in the first version: a trim-both-sides rung (it ignores indentation; OpenCode's `edit` deliberately omits it and its own finding says that is where other harnesses had corruption bugs), and any similarity rung. If similarity is ever added, require: a unique candidate, a margin over the second (Oh My Pi 0.08), never with `replace_all` (Hermes), and a flag in the result.

Rules that go with the ladder:
- **Report the tier that matched** in the result text and in the event ("matched after Unicode normalisation"). Only Cline reports fuzz; Codex, Pi and OpenCode give no sign, and Pi's fuzzy mode rewrites whole lines silently.
- **Fuzzy finds, it does not rewrite**: splice the replacement into the original bytes (OpenClaw). Do not copy Pi's `widenReplacementsToLines`.
- **Line endings and BOM**: preserve per line (OpenCode `withRegionEndings`; Codex first-seen-ending for inserted lines). Drop Codex's habit of adding a trailing newline.
- **Multi-edit batch, one call**: match every `oldText` against the **original** content, require disjoint spans, apply in reverse order, write once (Pi `edit-diff.ts#L343-L406`). All-or-nothing in memory is cheap and removes partial states inside one file. Add `edits[]` next to the single-edit form (Pi `prepareEditArguments` accepts both).
- **Failure hints**: closest lines gated by "read is already permitted" (OpenCode); "newString already present, edit may already be applied" (OpenCode); strip a pasted `N|` gutter (Hermes); escalate the hint at the third miss on the same path (Hermes); refuse the third identical no-op (Oh My Pi `NOOP_HARD_LIMIT`). Hash the outcome, not just the arguments (OpenClaw).
- **Argument repair before validation**: a `prepare_arguments` step that accepts stringified arrays and a single object where an array is expected (Pi, OpenCode), and tells the model what ran (OpenCode appends a note).

**Patch tool**: not now. Route by model family only if GPT-family models are in scope (OpenCode and Cline both do), and keep a JSON-schema fallback because Codex's freeform grammar needs custom-tool support. When added: copy Codex's verify-all-then-apply and fix its two gaps (6.3).

##### 6.3 Atomic writes and multi-file apply

1. Per-file: write to a temp file in the same directory, `fsync`, rename over the target, preserving mode (DeepSeek, OpenCodex). Our `fs.rs` opens without following links, which must be kept: open the temp with `O_EXCL`, rename without following (OpenCodex adds `fstat` identity and refuses files with more than one hard link).
2. Per-path async mutex for edit and write on the same canonical path (Pi, OpenClaw, DeepSeek, OpenCode), and make the **version check run inside it**.
3. If a patch tool is added: bind approval to content. At verification time record `sha256` of each target; at write time recheck under the lock and refuse on mismatch (Codex re-parses and re-matches at apply time with no hash binding). Write all temps first, then rename in order; on a failed rename restore the already-renamed targets from the originals kept in memory. State plainly that this is not crash-atomic across files, and log the applied delta as an event (Codex's `AppliedPatchDelta` is the shape).
4. Verify after write like OpenClaw (re-read and compare), at least in debug builds; it catches a silent short write.
5. Scheduling: edits are exclusive, reads shared (Codex `RwLock` scheme, flagged per tool, fail-closed on a throwing classifier as in DeepSeek and Oh My Pi). Set the flag from effects ("writes the workspace"), not from the name "read-only"; shell is exclusive unless proven read-only.

##### 6.4 An edit ledger for review, rewind and cheaper checkpoints

**Problem (evidence).** Our checkpoint is `git add -A` into a private index every agentic turn. Zed deleted that design (per-prompt checkpoints hung large repositories; reason `inferred`) and replaced it with an in-memory baseline per touched file. Hermes bounds it (skip over 50,000 files, drop files over 10 MB, 20 snapshots, 500 MB) and is off by default. OpenCode keeps O(tree) cost per step and no retention. Cline has no caps.

**Do.**
- Reuse the seen-version table (6.1) as an edit ledger: per path, the baseline at first agent touch (hash plus a blob), then each own write's hash. It feeds three things: the staleness guard, a per-turn "files changed" view with a real diff (our UI shows only an `edit` call's old and new strings today), and Hermes-style safe restore ("restore only if the file still has the hash we wrote").
- Fix Hermes' hole: its safe-restore ledger sees only file-tool writes, so shell-changed files are silently skipped or silently restored depending on ledger state. Our node sees every `bash` call, so for any shell call that is not provably read-only, diff the tree (or the paths in the Landlock writable set from pull request 4) before and after and record those paths in the ledger too. Do not decide by a command-text regex (Hermes' regex misses `find -delete`, `tee`, `python -c`).
- Use the full-tree snapshot only when a turn contained a write-class or non-read-only shell call. A turn of reads and a pure `edit` can use the ledger alone.
- From OpenCode: seed the shadow index from the real index so tracked files are not rehashed; write the index to a temp file and rename; coalesce concurrent captures; set `fsmonitor=false`, `untrackedCache=true`, `feature.manyFiles=true`. Add what OpenCode lacks: a retention policy. From OpenCode also: skip untracked files over 2 MiB. From Hermes: set `core.hooksPath` and `core.fsmonitor` explicitly rather than rely on the store being bare (Hermes' own reviewer left that inference untested).
- From Cline's restore transaction: snapshot the present before destroying it, refuse if `HEAD` moved, and refuse (do not half-restore) when a nested repository is present (Hermes refuses; Cline's untracked-files clean runs only when untracked files were captured).
- Decide explicitly whether "rewind to message N" reverts only edits after N. Zed reverts everything unreviewed (corrected in review).

**Check in our code.** Run a checkpoint on a repository with a very large tree and time it (Round 1's own inventory notes cost on a large repository was not measured); confirm a turn that only read files takes no snapshot.

##### 6.5 Shell mechanics: gaps relative to what the sources do

1. **Orphans that leave the group.** Our group kill misses a child that calls `setsid` (goose, Cline confirmed this class; Oh My Pi `inferred`). Pull request 4's Landlock does not contain it. Options: a PID namespace or cgroup per command on Linux; or Oh My Pi's walk-then-re-walk before SIGKILL as a cheaper step. Check whether we should kill leftovers after the command's own process exits, unless the call was explicitly background (goose leaves them alive on purpose; Cline's detach does too; both are choices we should make deliberately, not inherit).
2. **Model-supplied timeout.** Ours is already capped at 3,600 s and cannot be 0. Keep it. goose and Oh My Pi let the model pass 0 for "no deadline", and Pi has no default.
3. **Cap while reading.** goose reads output into an unbounded `Vec` first. Our stream budget on `main` does bound it; keep the ring-buffer-plus-spill shape (Codex 1 MiB head and tail per process; Oh My Pi 20 KiB head plus 20 KiB tail with a 16 MiB artifact cap; Pi tail accumulator). Tail-biased previews suit build logs (goose, Pi); head and tail suit both (Cline 24k/24k, Oh My Pi).
4. **Background jobs, if added.** Promote an over-limit foreground call to a tracked background job instead of refusing it (Hermes avoids refusal loops). Pair each job's PID with a process-start token against PID reuse (Cline). Put them in a per-session table with a cap and LRU eviction that protects the most recent (Codex: 64, 8 protected). A backgrounded job gets no further approval check in Oh My Pi; ours should keep the node's policy and cancel path attached. Decide what survives a node restart; none of the sources says it is "nothing", but our event log means a restart must be visible as an event.
5. **Node crash.** goose's shell has no `PR_SET_PDEATHSIG`; its MCP servers do. `prctl` from a `pre_exec` closure needs `unsafe`, which our workspace forbids. Check for a crate that does this without our own `unsafe`, or use a supervisor process as Hermes does ("owner-death supervisor"); until then children survive a SIGKILL of the node, as with Pi.
6. **Do not decide safety from command text.** Hermes' destructive-command regex (snapshot decisions) and `skill_lock.py` (write lock), Cline's plan-mode blacklist (its header admits `python -c` escapes) are advisory. Use effects: Landlock writable set, tool class, ledger.

##### 6.6 Post-edit step: parse delta, not formatter

- Run a parse check on the node and report **only errors the edit introduced** (Hermes "pre-existing errors filtered out"; its line-set diff misfires when a linter renumbers lines, so compare error count and error node kinds, not line text). We already use tree-sitter for `outline`; no new dependency. Skip files whose language has no grammar.
- Do not run a formatter after the agent's edit by default. OpenCode does and then returns a diff against the formatted file; the model's picture of the file no longer matches what it wrote, so the version table must take the post-format hash or the next edit refuses.
- LSP diagnostics: no verified implementation to copy (Hermes' LSP layer and Oh My Pi's were not read). Leave it.

##### 6.7 Tool-result caps and loop guards

- Scale per-result caps with the window (OpenClaw: min of 30% of the window and 16k/32k/64k characters) instead of our fixed 30,000; this follows pull request 7's window awareness.
- Exempt `read` from spill so reading a spilled output is not re-spilled (Hermes pins `read_file` to infinity; DeepSeek exempts `read`); give the model a handle it can page (`tool_output` already does; Cline's `cline://cache/...` and OpenCode's path are alternatives).
- Do not fail open when spilling fails (DeepSeek keeps the full text inline on any error). Truncate inline instead.
- Add a generic ceiling in the dispatcher in addition to per-tool limits (Pi's extension tools have none).
- Refuse every tool call in a message that ended with `length` (Pi `failToolCallsFromTruncatedMessage`). Check our `agent-native` loop for this today: it is the cheapest safety fix in this list. Also fail after three consecutive `length` stops (OpenCode).
- Loop guards at tool level: copy the three-identical-no-op rule, an always-on identical-error breaker at 3 (OpenClaw), and Hermes' dual default (warn when attended, halt when unattended), which fits our routines. Hash outcomes. Do not copy goose's inert inspector or Hermes' regex tripwires.

##### 6.8 Stream rules (TTSR): later, and only in this shape

The feature freeze applies. If built after Gate 3: run rules on the node with the Rust `regex` crate (linear time) so an untrusted repository's rule file cannot stall a session; make the retry bound explicit (fire once per session, as Oh My Pi; there is no counter); because our log is append-only and one-writer, record the abort as an event that supersedes the partial assistant message rather than deleting it (Oh My Pi's `contextMode: discard` drops the message); reset the fire-once state on compaction or pin the rule text, because Oh My Pi keeps suppression without the text (`inferred`). Rules come from project files, so they are untrusted input: label the injected excerpt as data as Oh My Pi's template does. Skip judged rules (an extra model call per output).

##### 6.9 What not to copy

- **Similarity matching at low thresholds** (Cline 0.66, Zed 0.8 per line, Hermes 0.5 for block anchors): wrong-place edits on repetitive code; all four tools apply the best unique candidate silently except Cline.
- **First-match-wins with no uniqueness** (Codex).
- **A staleness check that only warns** (Hermes `patch`, Zed) and **a guard that ships disabled** (DeepSeek). Check what we ship, not what we document.
- **Prompt-only read-before-edit** (Oh My Pi `replace`, Zed, DeepSeek, goose).
- **A 16-bit tag as the only guard** (Oh My Pi).
- **NFKC folding with whole-line rewrite** (Pi).
- **Absolute paths accepted by edit tools** (Cline `editor` and `apply_patch` for absolute paths, goose `resolve_path`, Pi): our node-decides invariant and `workspace_roots` already forbid this; keep the identity-after-open check from OpenCodex.
- **`edit` that saves on tool completion even when a later edit failed** (Zed).
- **Rewinding a conversation without files, or files without the conversation, without saying which** (Oh My Pi conversation-only rewind, Zed whole-log reject).
- **Letting the model detach a command silently** (Oh My Pi 60 s auto-background gives the job no further check).

##### 6.10 Quick checks in our own tree (each is a test)

1. `edit` after an outside change to the same file (the case in 6.1).
2. Two parallel `edit` calls on one path: serialised, second sees the first's hash.
3. CRLF, BOM and a file with no trailing newline round-trip unchanged for an `edit` that touches the middle.
4. A response that stops with `length` and contains a half-written `write` call: nothing runs.
5. A shell call that changes a file the agent had read, then an `edit` on it: refused with a diff.
6. A command that calls `setsid` and sleeps: still running after cancel? (document the answer).
7. A turn with only reads: no snapshot taken.
8. Compaction followed by `edit` on a file read before the compaction.

### Sub-agents, orchestration, model roles, fallback and retry

From [round2/synth/09-subagents-models-retry.md](round2/synth/09-subagents-models-retry.md).

Ranked by importance. Where a point relies on an unchecked finding I say so. I have not seen the code of pull requests 4, 6 or 7; statements about them come from the task brief and the design note. `CLAUDE.md` says there is a feature freeze until Gate 3: aliases and slots are new features, so before building them, state the cost against `FIX_PLAN.md` and ask the owner whether to break the freeze. The permission and caps points below are fixes to existing behaviour, not new features.

##### 6.1 How a child's permissions should relate to its parent's

1. **Make the child's authority an intersection, computed on the node, and never a parameter the model or a role can raise.** Read Codex `core/src/agent/role.rs` (`AgentRoleOverrides`) and `child_config.rs` (`apply_spawn_agent_runtime_overrides`): the role is a patch type with narrowing fields only, and the parent's live approval policy and permission profile are re-applied last. Copy both halves: the type that cannot express widening, and the last-step re-apply as a second guard. Add Hermes's rule from `delegate_tool_toolsets.py`: explicit toolsets are intersected with the parent's. `main` already says a child gets the same approval mode and `tools:` narrows what is offered, with node policy deciding execution (`own-rizzy-harness.md`, section 7); that is the right shape. Relation to our state: the node's `node.toml` stays the ceiling for every child.
2. **Do not copy the three failure modes in the input.** OMP forces `yolo` for every child (`task/executor.ts`); Cline builds children with no policy and no approval callback; Warp types `--dangerously-skip-permissions` into a hidden pane and pre-writes trust into the vendor's config. In each, approving the delegation approves everything. The same logic applies to PR 4: a child must run under the same Landlock profile or tighter, an allow-listed environment no wider than the parent's, and capped file tools.
3. **Decide approval routing for children explicitly, with two cases.** A foreground child whose parent has a person watching: forward the child's approval request to the parent's chat owner (goose, `subagent_handler.rs`, remaps ids so the parent's user answers). A background or unattended child: reject, and write the child's effective policy into the child's own log at creation (DeepSeek `child-agent.ts`, `captureDelegatedPolicyOverrides`; the reviewer confirmed it). Either way, "allow for this turn" from PR 4 should not carry into a child by default: DeepSeek copies only the explicit session override and not one-shot grants, and that is the safer reading. That last step is my inference; test it.
4. **"Read-only" must be a tool filter plus a sandbox profile, not a description.** Two projects ship a read-only role that is not read-only in code: Codex `explorer.toml` is a zero-byte file (reviewer confirmed); OpenCode `explore` allows `shell *` and relies on a prompt. PR 6's Scout and Reviewer are read-only by roster. Test that each has no write tool and that its shell is absent or under a read-only Landlock profile, and that a call to a tool outside the agent's `tools:` list is refused at execution, not only hidden from the model (the inventory says node policy "still decides execution"; confirm it covers the agent's list). A table-driven test per roster entry (the inventory says one exists for the tools) should also assert the sandbox profile.
5. **Project-supplied agent files are prompt and permission injection.** goose reads `.claude/agents` and `.goose/agents`; OpenCode reads `.opencode`, `.claude`, `.agents` with no trust prompt (a repository file can set `permissions: [* allow]`); Cline reads workspace `.cline/agents` as tools; Pi's example gates project agents on an interactive confirm. Our agents live in the data directory and the master database, which is safer. Do not add repository-read agent files without a trust step, and never let such a file name a model, tools or `delegates`.
6. **External vendors' agents: not now.** The node's Landlock sandbox does not cover a vendor's process, and the input shows the permission request is advisory (Zed does not apply its deny list to ACP agents; Warp auto-allows). If it is ever added, copy DeepSeek's unattended table (decline every approval, empty user input, decline elicitation, empty client capabilities, `mcpServers: []`, environment scrubbed) and derive the vendor agent's mode from the node policy so a delegation cannot widen it (the opposite of DeepSeek's fixed per-provider config); test the "chat only" gap that goose has for Amp and Pi.

##### 6.2 Caps and defaults

Most harnesses converge on a short list; the first three rows are the minimum.

| Cap | Where to enforce | What the input suggests as a default | Source |
|---|---|---|---|
| Depth | In the spawn handler, from the persisted depth in the child's record (a resumed parent must not reset it) | 1 (Codex, OpenCode, goose, DeepSeek, Hermes) | DeepSeek `child-agent.ts` L49-L58 takes the max of persisted and runtime depth |
| Active children per parent, and concurrent children per node | Node, at admission; error names which limit | 4 per parent is already our default; per node 8 (DeepSeek) is the middle of the range 5 to 10 | OpenClaw `child-admission.ts` has three separate limits |
| Wall-clock and idle limit per child | Node supervisor | No project in the input has a good default: goose and Cline have none; Hermes marks stale at 450 s idle or 1,200 s in one tool; OMP parks idle children after 420 s | Hermes `delegate_tool.py` L107-L121 |
| Step cap per child | In the loop, keeping tool schemas and sending `tool_choice: none` | OpenCode `steps` (unset by default); goose 1,000; Hermes 250 | OpenCode `runner/llm.ts` L259-L281 |
| Token cap per child | Already present (200,000 combined, checked between steps so a step can overshoot) | Keep; add a per-step bound | `own-rizzy-harness.md` section 7 |
| Total sends including retries per turn | Node, a shared counter charged by every child | OpenCodex: 3 attempts per logical request, children inherit the parent's spend | OpenCodex finding 7 (unchecked) |

Also copy: the cap message tells the model what to do next ("Solve the task yourself", Codex; "wait or do the work directly", DeepSeek). Check our `depth is one (children have no delegates)` is a code check at spawn and not only a property of the built-in definitions: a custom agent with `delegates` used as a child is the case to test (OpenCode needs both a depth check and `subagent: deny` on its two built-in children).

##### 6.3 How to return results

1. **Give the child a finish tool with a default one-field schema, an optional caller schema, and a failure state.** goose `subagent_handler.rs` and `summon.rs` (`final_output`, default `{summary}`, failure with a reason and the last text); OMP `yield`; OpenClaw `structured_output`. This makes the parent's context cost predictable and answers "did it finish". The current `main` returns the child's final answer text.
2. **The result is a log event, and the parent folds it.** The child's session is already a session record on the node. Append one result event to the parent's log with the child's session id, a typed status and exit reason (Hermes `exit_reason`, `truncated`), and a capped summary: Hermes caps at 24,000 characters (head 75%, tail 25%), redacts, and spills the rest to a file. This keeps the single-writer invariant: the same node's session actor writes both.
3. **Delivery is queue-only at the next step boundary, with an idempotency key.** Codex queues a note with `trigger_turn = false`; DeepSeek wakes an idle parent or steers a busy one; OpenClaw uses an idempotency key and never replays an ambiguous delivery. Because our node may restart mid-operation, key the result event by the child session id so a retry cannot deliver twice; goose re-derives pending children from persisted "delivered" notes, and OpenCode fails a running `subagent` tool call after a crash so the model can continue the child by id. Copy one of the two (goose's is the closer fit for an event log).
4. **Mark the result as untrusted data and as an unverified self-report.** OpenClaw wraps it as data; Hermes tells the parent summaries are "SELF-REPORTS, not verified facts" and asks for a verifiable handle. Our inventory records that sub-agent task text enters the web-fetch "observed" set (AUDIT L11, `unchecked` here); do not let a child's result widen that set.
5. **Strip or validate any in-band marker that triggers orchestration** (goose strips `subagent_session_id` from external MCP output and verifies ownership). If a result event can start something, only the node's own session actor may write it.

##### 6.4 The alias and slot data model, and from whom

The design note already has a per-workspace `model_aliases` table (ordered list of `<provider>::<raw>`), `::` to tell a model from an alias, built-ins `small`, `vision`, `long-context`, an alias expanding to its models in order, a chain capped by `MAX_FALLBACKS`, and slots `title`, `summary`, `vision_bridge` with a capability check at save time. Measured against the input, this is close to the right shape. What to copy or check:

1. **Hermes `auxiliary_task_config.py` and `config_defaults.py` for the slot shape.** One block per side task, all the same shape: `provider`, `model`, credentials, `timeout`, `reasoning_effort`, `fallback_chain`. Default every slot to "the turn's own model"; walk a global discovery chain only when the user configured no policy. Add `timeout` and reasoning effort to our slot record: a title call on a reasoning model at full effort is the cost OpenCode avoids with a minimal-reasoning variant.
2. **Keep the route as one unit** (Hermes: provider, model, base URL and key travel together, so one vendor's key is never sent to another's endpoint). Our `provider::raw` form does this for the model; check that a slot or child model on a different provider than the turn's gets its own key grant on the node, and that the node never receives a key the turn did not need (this follows the design note's "granted per turn"; I did not verify the code).
3. **OpenClaw's option type for "strict".** A defined empty fallback list (`fallbacksOverride = []`) is how a user's explicit model choice fails instead of silently switching, and the primary is not appended. Model this as `Option<Vec<_>>`, not a default value; it distinguishes unset from none. Unchecked in review; the call sites were not traced.
4. **Resolution order for a child's model.** Codex: tool argument, config, parent, with a role pin overriding all; goose: recipe, env, parameter, config, parent, provider default (and a quirk where a mismatched provider silently falls through). Recommended for us: the agent definition's model or alias, else the parent's current model. Whether to let the model pass a model argument is a cost question: Codex allows it (validated against the model list, error lists up to 5 models); DeepSeek restricts it to the parent's route unless policy allows more. I would not give the model a model argument at first (my judgement, not an input finding).
5. **Do not hard-code model names or families.** OMP's role priority lists, OpenCode's title families and OpenHands's embedded benchmark table age quickly. Our design (empty built-in aliases proposed from reported capabilities) already avoids it; keep the proposal a suggestion the admin confirms.
6. **Slot names as strings in a known set, validated by the master.** OMP went from nine to 16 roles and accepts custom names; Hermes has 19 slots and plugins can add more. We need three slots now plus a child-model role; do not bake the set into the protocol as an enum.
7. **Summary slot window rule.** The design skips a summary model whose window is smaller than the main model's. Hermes instead lowers the compaction trigger to the summary model's window (reviewer confirmed it leaves no headroom for template and output); skipping is simpler and keeps the trigger.
8. **A slot failure must not fail the turn.** OpenCode retries a failed title on the primary model and keeps the fallback title on failure; Hermes falls back to the main model with a warning when a configured aux route cannot be resolved; Hermes's summary failure ladder (one retry on the main model, cooldowns, then abort and preserve the session unchanged) is worth copying for `summary`.
9. **Groups with strategy belong to a later design.** OpenCodex combos (failover, weighted round-robin, least-used, reset-window; metered consent) are for a pool of accounts; one ordered alias is enough until we hold several keys per provider.

##### 6.5 What pull request 7's retry should still adopt

I have not seen PR 7. Compare it with this list; each item has at least one working example in the input.

1. **One function from error to policy, on structured fields, not message text.** Codex `protocol/src/error.rs` `retry_delay` (never retry auth, invalid request, model-not-found, context overflow, usage and quota limits; server-advice-only; backoff). Hermes `error_classifier.py` puts `retryable`, `should_compress`, `should_rotate_credential`, `should_fallback` on the error type. Pi's roughly 100-pattern text regex is the example to avoid. Add a test table: status and code in, policy out.
2. **Honour `Retry-After` with a cap, never let it extend the count, and fail the step or fall back when it exceeds the cap.** Codex sets a wake instant and keeps the count; OpenCode caps at 15 minutes; DeepSeek gives up when it exceeds `maxDelayMs`; OMP fails fast above 300 s ("a 3-hour sleep in a node is a stuck session"). Make sure the header is read where the response arrives; Pi shows how it can be dormant (`provider.maxRetries` 0 means it is read nowhere).
3. **Jitter.** Pi and Cline have none (Pi's comment says there is). The range in the input is plus or minus 10% (Codex, DeepSeek) to 25% (OMP, down only).
4. **Numbers.** Count: 3 to 5 for the main schedule (goose 3, Hermes 3, Cline 3, Pi 3, Codex 5, DeepSeek 5), 10 where the cap is low (OpenCode and OMP, about 84 s total at 2 s to 10 s). Delays: base 200 ms to 2 s; cap 8 s to 60 s.
5. **Retry only when it is safe.** Do not retry once output or a tool call has been committed unless the stream was interrupted (Cline, OMP, OpenCode). In our event log record the failed attempt as an event with the wake-up time and delay (DeepSeek `llm/retry` events, OpenCode `RetryScheduled`) and decide whether partial assistant text is kept or hidden (Pi hides it with `context_edit` entries; DeepSeek keeps it flagged interrupted on abort). `main` already falls back only before output; keep that rule.
6. **A separate patient schedule for no network.** OpenCode: `ENOTFOUND`, `EAI_AGAIN`, `ENETUNREACH` and similar use a one-hour window with its own allowance; Codex retries pure connection loss forever (5 s to 60 s) with a visible state. A node on a laptop needs this.
7. **Per-reason cooldown and fallback integration.** Trigger fallback eagerly on rate limit and billing and after N retries on transport (Hermes `turn_recovery.py`); restore the primary at the next turn (Hermes) or probe it half-open once per interval (OpenClaw `model-fallback-cooldown.ts`: at most one probe per 30 s). Key the cooldown on the reason (OpenClaw table in 3.2) and on the (credential, model) pair (Hermes). Keep cooldown state on the node, not in the event log; persist it as absolute expiry (OpenCodex `cooldown-store.ts`) so a restart does not re-offer a cooled target. Never replay a turn that already executed a tool when falling back (OMP).
8. **Billing is its own class.** Do not retry; disable the credential or model with a long window (OpenClaw: 10 minutes doubling to 24 hours), and show the user a clear message (Hermes: "retrying only burns paid requests on a depleted balance").
9. **Retry accounting across children.** A parent with 4 children each retrying 5 times is 20 sends. OpenCodex charges retries to the logical request and to a workflow root; do the same with a per-turn counter shared by children.
10. **Use the same helper for every model call**, not only the turn: title, summary and compaction calls (Pi `retryAssistantCall`; Hermes `transient_retries`). Add an idle timeout on the stream (OpenClaw: 120 s model-stream idle, 48 hours per run).
11. **Empty responses** are a distinct retryable class (goose: 3 retries; DeepSeek `EMPTY_RESPONSE`).
12. **Cancellation aborts the sleep and emits an end event** (Pi, DeepSeek, Cline).

##### 6.6 Things to test in our own code

- A child cannot call a tool its definition does not list, even if the model names it (Codex explorer, OpenCode explore).
- A child under a read-only parent policy cannot write; its shell sandbox is no wider than the parent's (PR 4 and PR 6 together).
- A child created from a custom agent with `delegates` cannot spawn at depth 1.
- A worktree creation failure for a writing child does not silently run it in the shared tree: OpenHands and Hermes fall back to shared; the input shows that is the wrong default for a node (two writers on one tree). Fail the spawn or run read-only. Copy OMP's apply ladder (plain apply, `--3way`, replay the parent's work-in-progress) and OpenClaw's rule that every uncertain garbage-collection branch means "keep".
- A restart between a child finishing and the parent recording the result delivers exactly once.
- A `Retry-After` of one hour does not block a worker for an hour.
- A 402 does not retry; a 401 does not rotate to an unconfigured provider (Hermes: "never silently hop to an unconfigured provider on auth error").

### Extensibility and unattended operation

From [round2/synth/10-extensibility-unattended.md](round2/synth/10-extensibility-unattended.md).

State used for these points: `main` has routines and webhooks that cannot act unattended without a human click, and no MCP, skills, hooks or plugins. Pull request 4 (Landlock sandbox that fails closed, an allow-listed empty child environment, secret redaction across output chunks, "allow for this turn" honoured only after a first approval) gives the base every item below plugs into. Pull request 6 enforces where an agent may run. Pull request 7 adds compaction keyed to the context window. The design note gives per-turn credential grants to nodes and rootless Podman environments with a worktree per session. `CLAUDE.md` puts a feature freeze in force until Gate 3 of `FIX_PLAN.md`; I did not read `AUDIT.md` or `FIX_PLAN.md`, so each step below needs the owner's decision to break the freeze, and step 1 is the one that fixes something that exists today.

##### 6.1 The order, with the safeguards each step needs from day one

| Step | Add | Safeguards that ship in the same change | Whose code shows why |
|---|---|---|---|
| 1 | **Unattended authorisation for routines and webhooks** | (a) an unattended run has an explicit per-routine allow set, created by a person, content-hashed and recorded as an event; anything not in it is **denied**, and the real reason is returned to the model; the node's `node.toml` is still the ceiling. (b) an agent-created routine starts `pending_approval`. (c) the agent cannot choose model or provider for unattended spend, and a routine agent cannot create routines. (d) advance the schedule before dispatch, write a fsynced dispatch record with an idempotency key before the first model call, pause (not guess) when the outcome is unknown. (e) missed runs: skip or latest-only, chosen and recorded; overlap: refuse or queue, chosen explicitly. (f) webhook idempotency keys in the database, not memory; webhook runs start from a read-only policy recorded as an event; the webhook listener is separate from the UI API. (g) never default to auto-approve; a prompt scan is a tripwire, not the gate. | OpenCode `core/src/permission.ts` (`Unattended` in `evaluateInput`); Hermes `cron/scheduler_tick.py`, `cron/scheduler_authority.py`, `tools/cronjob_tools.py`, `hermes_cli/config_defaults.py` (`cron_mode`); OpenClaw `bash-tools.exec-host-gateway.ts` (standing grant) and `delivery-queue-recovery.ts`; DeepSeek `schedule/src/runtime.ts` (flush then receipt), webhook `session.ts` (preset as log event) and its isolated listener; Cline `tasks/store/sqlite-task-store.ts` (agent-created tasks pending). Negative: Cline `cron-runner.ts`, goose `scheduler/full.rs`, Warp `execute.rs` |
| 2 | **Event vocabulary for context changes** (prerequisite for 3 and 4) | a prompt-section delta event and a tool-loadout event in the session log; the UI and the node both fold them; sorted tool names; changes appended, never edited into the head of the prompt | Pi `core/system-prompt.ts` (`diffSystemPromptSections`) and `agent-session.ts` (`_restoreToolsFromTranscript`); OpenCode `instructions/index.ts` and `session/instruction-state.ts`; Codex `core/session/world_state.rs` |
| 3 | **Skills** (text only) | roots are node-controlled paths; repository skills load only under the same per-project trust decision as repository config (Codex does not do this); catalog capped by count and characters with degradation to short descriptions; body fetched by a tool call that goes through the approval path; nothing executes at load; remote sources, if any: same origin, safe relative paths, atomic replace, content hash recorded; skill-declared environment names cannot widen the child environment; agent-written skills are `propose`-only and an approval is bound to the base content hash | OpenClaw `skills/loading/skill-prompt-limits.ts`; OpenCode `skill/discovery.ts`, `tool/plugin/skill.ts`; Hermes `tools/env_passthrough.py`, `skill_manager_tool.py` (staging) and `background_review.py` (dispatch-time restriction); negatives: Codex `ext/skills`, OpenClaw `extensions/skill-workshop` |
| 4 | **MCP client, with deferral** | (a) **only the node's own policy file may add a stdio server**; no repository file, HTTP call, model tool or master setting can. (b) the child's environment is built by the PR 4 allow-listed builder, never inherited; secrets reach it only as named per-server entries granted from the node's config. (c) stdio servers start inside the session's environment (Podman) and under the same sandbox as the shell. (d) approval is per `server + tool`, bound to the server's executable identity (path plus digest); a server's `readOnlyHint` may only tighten; unknown tools default to "needs approval". (e) tool names `mcp__<server>__<tool>` with a hash suffix on collision; a server's tools register all or none; per-line, per-result and per-call size and time caps. (f) deferral: a local BM25 search tool, switched on by **measured tool-list size**, with a hard cap on any name listing; activation recorded as a loadout event (step 2) so fork, resume and a node restart reproduce it; approval decided on the underlying tool, not on the search or bridge call. (g) `list_changed` bumps a per-server revision and specs are re-resolved before the next step. (h) Streamable HTTP only; OAuth callback hosted where a browser can reach it (the master, not the node), the token granted to the node per turn like a provider key, keyed by user and server URL, never in an event, log or URL; remote responses capped; auth stripped on cross-origin redirect. (i) refuse sampling by default | Hermes `tools/mcp_tool_config.py` (`_build_safe_env`), `mcp_tool_registration.py` (filter, collisions), `tool_search.py`, `tool_dispatch_helpers.py` (bridge peeling); DeepSeek `mcp-client/src/tools.ts` (`publicToolName`, generation swap) and `transport.ts` (`scrubbedParentEnv`); Pi `extensions/mcp/index.ts` and `tool-search/tool.ts` (recorded activation); Codex `rmcp-client/src/tool_list_changes.rs`, `bounded_stdio_transport.rs`; omp `mcp/tool-cache.ts` (cache by config hash), `mcp/client.ts` (roots only); Warp `file_based_manager.rs` (project servers never auto-start); OpenCode `mcp/pool.ts` (key includes credential). Negatives: goose `permission_inspector.rs` (`readOnlyHint`), omp `stdio.ts` and Pi `stdio.ts` (full env), Cline `McpHub.ts` (name allow-list) |
| 5 | **Hooks** | run on the node only; command hooks run under the PR 4 sandbox with the empty environment; trust is a hash of event, matcher, handler **and the bytes of every file the command references in the repository**, stored in a place only the node owner writes, re-hashed at every execution; untrusted or modified hooks do not run, and a project cannot write its own trust; **fail closed**: crash, timeout (process group killed), bad output, a rule that fails to load, or an invalid matcher all block, never silently drop; a hook may add a denial or escalate to the human, never approve; policy is re-resolved after any rewrite of the call; no hooks imported from other harnesses' directories | Codex `hooks/src/engine/discovery.rs`, `plugin_fingerprint.rs`, `plugin_trust.rs`, `config_rules.rs`; Hermes `hermes_cli/plugins.py` (`approve` escalation); omp `extensibility/extensions/wrapper.ts` (re-resolve), `runner.ts` (fail closed, 30 s); goose `hooks/mod.rs` (`classify_output` taxonomy: policy deny versus failure). Negatives: Codex `pre_tool_use.rs` (fail open), goose `hooks/mod.rs` (invalid matcher drops rule), Cline `hook-file-hooks.ts`, OpenHands `hooks-service.ts` |
| 6 | **Plugins as bundles, never in-process** | a bundle is skills, MCP definitions and hooks only; installed by an explicit human action in the UI, never a tool argument; pinned by commit hash or content hash; package-manager install with scripts disabled; `GIT_*` variables scrubbed for any Git call on an untrusted checkout; updates visible and pinned, never silent; no code runs in the master or in the node process | Codex `core-plugins/src/git_policy.rs`, `marketplace.rs` (sha pin); Hermes `plugin_guard.py` (scanner as evidence only); DeepSeek `plugin-manager/src/tools.ts` (negative: `approvedBuilds` as a model flag); Pi `package-manager.ts` (negative: no `--ignore-scripts`); omp `marketplace-auto-update.ts` (negative: silent `auto`) |
| 7 | **Code mode**, if the numbers justify it | only after steps 4 and 5: every inner call is a normal tool call through the one approval path, with the parent call id on the event and hook visibility; runs on the node under the shell's sandbox, not in the master; host-set wall-clock, memory and call-count caps the script cannot raise; no `fetch`, or an SSRF-guarded one with per-hop re-check; denying the tool must not expose the inner tools as direct tools; a failed script's completed inner calls are in the log | Pi `agent-loop.ts` (`runToolCall`), `agent-session.ts` (`_executeNestedToolCall`), `codemode/runtime/vm.ts`; OpenCode `codemode/tool.ts` and `fetch.ts`. Negatives: goose `code_execution.rs` (`lease.call` bypass), Pi `execute.ts` (script-set deadline), OpenCode `tool.ts` (denial exposes tools) |
| 8 | **Import (data only)** | per-source converters from Claude Code, Codex and OpenCode transcripts into a **new** session with a fresh id and a dedupe key; read skills and MCP definitions from other harnesses' directories as data that still passes steps 3 and 4; never read executable hooks, tools or plugins from them; never expand environment variables into imported config | goose `session/import_formats/mod.rs`; Cline `services/session-import/index.ts`; omp `session/foreign-session-import.ts`. Negative: omp `discovery/claude.ts` (`.claude/hooks` imported as code), `discovery/helpers.ts` (`${VAR}` expansion) |

The order rests on risk and on what exists. Step 1 is already needed (routines are blocked today) and adds no execution surface. Step 2 is cheap and is what makes 3, 4 and 7 safe against the prompt cache and against fork and restart. Skills add no process. MCP is the first step that starts a program the user did not write; deferral ships with it because the context cost arrives with the first few servers (Cline, DeepSeek and Warp pay it on every step). Hooks come after MCP because the node policy already gates tool use; hooks only add project-specific restrictions. Plugins are packaging over steps 3 to 5 and are last among the safe items. Code mode is last of the engine features because it is the only one that can defeat the approval path (goose) and its benefit is covered, for most users, by deferral.

##### 6.2 What to avoid, and why

- **In-process plugins on the master or in the node process** (OpenCode, Pi, Oh My Pi, Cline worker mode, Hermes default). They hold every credential the process holds.
- **Reading executable hooks, tools or plugins from other harnesses' directories** (Oh My Pi). Cloning a repository becomes code execution before any approval.
- **Auto-enabling anything discovered in a repository** (goose plugins). Trust must be a recorded decision.
- **A path-keyed trust boolean** (Pi) as the only gate for executable project content: a later pull changes what it covers. Bind to content.
- **Trusting server or model self-description for approval** (goose `readOnlyHint`, Warp `is_risky` and `is_read_only`).
- **Full-environment inheritance for MCP children** (Oh My Pi, Pi, Cline, Warp). It contradicts our PR 4 environment rule.
- **A scanner that only logs** (Hermes MCP description scan) and **a phrase deny-list used as a gate** (Hermes prompt scan). Use them as evidence or tripwires; a hit should demote a tool to needs-approval.
- **Prefix or name-based allow-lists for servers** (goose, Cline).
- **Fail-open hooks in the approval path** (Codex, Hermes, goose default, Cline, OpenHands).
- **Unattended mode equal to auto-approve** (Cline, goose, Oh My Pi default, Warp ACP), and hiding the real denial reason from the model (Warp).
- **Idempotency state in memory** (OpenClaw webhooks) or no check of a delivery id (DeepSeek).
- **Interpolating user or model text into anything scanned for executable markup** (OpenCode command templates).
- **Agent-written skills with no human step** (OpenClaw Workshop default) and **an approval replayed without a base hash** (Hermes).
- **Word "sandbox" for crash isolation** (Cline plugin mode). Do not name a feature after a property it lacks.

##### 6.3 What to check or test in our own code

Each is a test that fails on a naive implementation.
1. A routine run that reaches an `ask` returns a denial with the real reason, never waits, and writes the denial to the log.
2. A webhook delivered twice, with a restart between, creates one run.
3. Kill the node between "dispatch recorded" and "first model call": the routine pauses or resumes once, it does not fire twice.
4. A stdio MCP test server that prints its own environment sees none of the provider keys or `GIT_*` variables, only the entries granted in `node.toml`.
5. A repository containing `.mcp.json`, `.claude/hooks/*`, `.agents/plugins/*` and `.agents/skills/*` starts nothing and loads nothing without a node-side decision; a hostile repository skill is not in the first request when untrusted.
6. Tool activation by search, then node restart, fork and compaction (PR 7): the loaded set is identical, and the provider request prefix is byte-identical when nothing changed.
7. Approval for a deferred or bridged tool is decided on the underlying name (the class of bug Hermes had to fix).
8. A hook that crashes, times out, prints junk, or has a malformed matcher blocks the call. Editing a script a hook references invalidates trust.
9. If code mode is built: an inner call to `shell` in a script triggers the approval prompt; a script cannot extend its own deadline; denying the code tool does not make inner tools callable.
10. The OAuth authorisation code travels in a redirect URL, which our "no secret in a URL" invariant forbids as written: decide how the callback is handled (single-use, short-lived, never logged) before step 4(h). This is my inference, not a finding.

##### 6.4 Relation to the current state

- **main**: routines and webhooks (step 1) are the live gap; nothing else here exists.
- **PR 4**: the empty child environment and the Landlock shell are the substrate for MCP stdio, hooks and any code mode; reuse the same builder and the same sandbox entry point, or the new features become the unsandboxed path (this is what Pi, omp and Zed external agents are). "Allow for this turn" after a first approval is the right shape for unattended standing grants (OpenClaw binds them to job, revision and operation).
- **PR 6**: server-side enforcement of where an agent may run should also say which agents may run unattended and which may use MCP, as Hermes denies the `cronjob` toolset for cron agents.
- **PR 7**: compaction must keep the loadout and the skill bodies the model still needs; Hermes prunes old skill bodies and forces reload pointers into the summary; Pi's compaction entry carries a snapshot of system messages; OpenCode re-bases its instruction baseline at compaction. Goose's claim that a conversation fold needs no side table under compaction should be tested against our compaction, not assumed.
- **Design note**: per-turn credential grants to nodes extend naturally to MCP OAuth tokens; rootless Podman environments are the place stdio servers should run (OpenCode runs them through the location's environment, Codex through an executor `environment_id`, goose through `docker exec`). Our git-worktree-per-session helps import: imported transcripts become new sessions.

## D. Not settled, per feature group

### The agent loop, tool dispatch and durability

From [round2/synth/01-loop-dispatch-durability.md](round2/synth/01-loop-dispatch-durability.md).

- **goose tool batches.** `goose.safety.md` says calls in one batch are "dispatched sequentially, their streams merged"; `goose.agents.md` says calls "run concurrently". Both cite the same lines (`ops_toolcalling.rs` L1083-L1158). I did not open the code (no clone was available). Both can be true if the loop starts each call in order but the returned futures run when merged. A decision that depends on whether goose executes tools in parallel should confirm this.
- **Steering storage in several projects.** The input files do not say whether steered or queued input is stored before it is acknowledged for Codex (`turn/steer`), OpenCode (inbox), Oh My Pi (RPC `steer`) or DeepSeek (inbox, partly process-local). My table entries for Codex and DeepSeek are inferences.
- **What a crash mid-turn does** in Codex (other than a planned daemon restart), Oh My Pi, Cline, Pi's shipped session (a dangling tool call is not repaired; the provider conversion that may drop orphans was not traced) and Hermes' in-process interrupt path were not described. No input file reports an "intent before effect" record for built-in tools in any shipped path except pi-durable (experimental) and DeepSeek's flush-before-tool rule.
- **Hermes loop guard call site** was not traced (author note). Its thresholds are read from the definition.
- **Hermes steering injection.** The admission row has intent `steer`; how a running turn consumes it was not read.
- **OpenClaw web-UI `chat.send` path** was not traced; the claim "stored before ack" is checked for the channel and `agent` paths only.
- **OpenCode** `restart.ts` resumes from the claim, but a workerd comment says boot-time resume replays the log (a discrepancy the reviewer noted). The shipped server does not persist the log.
- **Cline detector default**: the docs say off and the code runs it; the reviewer read it but nobody ran it.
- **Unchecked findings a decision would rest on:** OpenClaw restart-resume text and delivery-queue recovery (read the definition only); DeepSeek schedule double-delivery window (inferred); Hermes cron journal receipt recovery (not read); Pi durable codemode replay (read the definition); pi-durable has not been wired into the shipped CLI, so its replay policy is evidence of a design, not of a behaviour users have; OpenCodex Remote Workspace RPC v2 (traced, not reviewed).
- **Not cloned or not in the repositories:** OpenHands server SDK and Automation Server; Warp's server loop and worker; Zed (topic not covered). No claim in this file about their loops, steering or crash recovery should be taken from here.
- **Zed**: the approval wait is a loop in `thread.rs` L6872-L6985; its interplay with cancel was not read.

### Approvals and permission rules

From [round2/synth/02-approvals-permission-rules.md](round2/synth/02-approvals-permission-rules.md).

##### Remote answers: is the answer bound to the exact action?

No input file traces this, beyond an id, for these projects:

| Project | What is known | Label |
|---|---|---|
| Codex | The answer is a JSON-RPC response. | [inferred] |
| Hermes Agent | Gateway card and ACP: not traced. | — |
| Oh My Pi | ACP, RPC `extension_ui_request` and collab: not traced. | — |
| Zed | ACP option ids are passed through. | — |
| Pi | Bound to the dialog id; seen in the clone (section 1). | — |

OpenClaw's node path is the only fully traced binding.

##### Engines outside the repositories read

- **OpenHands.** How `ConfirmRisky` is enforced is not settled. Neither is whether `LLMSecurityAnalyzer` makes its own model call or reads the acting model's `security_risk`. Both need `OpenHands/software-agent-sdk`. The lesson "the model rates its own risk" rests on an [unchecked] reading.
- **Warp.** How Warp's server sets `is_risky` and `is_read_only`, and whether the server re-checks calls or enforces the organisation denylist.

##### Failure behaviour no one traced

- **OpenCode:** whether a throwing `permission.evaluate` hook fails closed. `plugin/promise.ts` was not read.
- **DeepSeek harness:** the hook bridges' behaviour on crash or timeout.
- **Cline:** whether `--yolo` removes file `PreToolUse` hooks.
- **Codex:** whether a bubblewrap start failure is misread as a sandbox denial, and so offers an unsandboxed retry. `sandboxing/src/denial.rs` matches keywords.
- **Codex:** what a `HumanCommand` approval does when no eligible connection exists.

##### Bypasses read from code but never run

- **Cline:** a hub connection authorised by `Origin` alone that creates an auto-approved session.
- **OpenCode and OpenHands:** self-approval through a readable credential.
- **Oh My Pi:** `bash -c` past a deny rule.
- **goose:** whether `delegate` runs in `approve` mode.
- **goose:** whether an ACP provider can set `goose.executable` and so pre-approve a call.

##### Unchecked findings a decision here would rest on

- **Codex:** the session approval key fields. They are the model for recommendation 2; re-read `unified_exec.rs#L87-L98` before copying.
- **goose:** the Smart Approve judge.
- **OpenClaw:**
  - heredoc and inline-eval forcing;
  - the reviewer's skip conditions and its 64-candidate cap;
  - cron standing grants.
- **Hermes Agent:** the redacted, deny-only approval card and the ACP bridge.
- **OpenCodex:** the prepare/grant/cancel RPC binding (finding 33).
- **Warp:**
  - the organisation denylist merge;
  - profile inheritance;
  - MCP project trust.
- **Zed:**
  - the file-tool symlink and sensitive-path prompts;
  - the ACP `terminal/create` path.
- **Pi:** the two-pass startup.

##### Not covered by the files read

- **Zed:**
  - how user, project and managed settings layer, and so whether `.zed/settings.json` can loosen user rules;
  - plan mode, hooks and sub-agent rule inheritance.
- **OpenClaw:**
  - whether sub-agents inherit exec mode and approvals;
  - whether MCP, code mode and client-side tools pass the exec approval path;
  - which model the reviewer uses by default;
  - who may call `exec.approval.resolve`.
- **Codex:**
  - how Code Mode (the default tool surface for most catalogued models) meets approvals;
  - the scope of the `/approve` override.

##### Repositories not studied

- **Not cloned at all:** Claude Code, Claude Desktop and Codex Desktop, because they are closed source. Their approval behaviour is known only from Round 1 documentation.
- **Not read:** OpenCodex's Claude Code and Pi remote-workspace runtimes.

### Sandboxing and where commands run

From [round2/synth/03-sandbox-containers-execution.md](round2/synth/03-sandbox-containers-execution.md).

- **Rootless `pasta` reachability.** Whether the host loopback and the node's sockets are
  reachable from a `network = "on"` container. No input covers Podman networking.
- **`podman exec` signal delivery.** Whether killing the client reaches the process inside. No
  input covers it (open in the design note too).
- **Podman's default seccomp profile.** Whether it denies `io_uring` and `AF_VSOCK`. Not in any
  input. OpenClaw only infers that the engine default applies.
- **`sendmsg` to pathname datagram sockets** under Codex's `Restricted` filter. The reviewer
  flagged it and nobody tested it. Whether Landlock ABI 9 covers datagram sends is also not in
  the input.
- **Why Codex rejects Landlock.** The code states only "isolate app-server sockets". The
  `connect()` explanation is inferred. Whether the daemon can spawn unsandboxed processes was
  not traced.
- **Why Zed moved from Landlock to bubblewrap.** No reason is written in the code.
- **Codex:** the Seatbelt base profile, the Windows restricted-token and MXC backends, and
  whether a `bwrap` start failure can be misread as a denial and lead to an unsandboxed retry.
  The policy-selection table and the proxy bridge internals are unchecked.
- **OpenHands:** the real `docker run` invocation, its hardening flags, server-side clamping of
  the client limits, and which names `sanitized_env()` strips. All need
  `OpenHands/software-agent-sdk`, not cloned.
- **Warp:** the self-hosted worker (`warpdotdev/oz-agent-worker`, not cloned). The `sbx` Docker
  sandbox finding is unchecked.
- **OpenClaw:** the browser sandbox and the SSH sandbox backend are unchecked. How `docker exec`
  children are killed is not in the input. The claim that a host-exec agent can edit
  `openclaw.json` is inferred.
- **Oh My Pi:** the kill semantics in `pi-shell` were not checked in review. A decision on the
  re-walk rests on the author's reading alone.
- **goose:** the process-cleanup finding (process group, Job Object, no PDEATHSIG for the shell)
  is unchecked.
- **DeepSeek:** the Windows ACL backend is unchecked, and its token code was not read. The claim
  that confined commands can read `.credentials.yaml` is inferred.
- **OpenCodex:** the macOS and Windows helper was not read. That host sockets are invisible in
  its bubblewrap argv is our inference from the mount list.

### Credentials, secrets and authentication

From [round2/synth/04-credentials-secrets-auth.md](round2/synth/04-credentials-secrets-auth.md).

- **OpenHands Agent Server**: whether it closes a WebSocket that never sends the auth frame, which
  names `sanitized_env()` strips, whether `base_state.json` persists the session key carried in
  `LookupSecret` headers, and server-side output masking. Needs `OpenHands/software-agent-sdk`.
- **Warp**: server-side redaction, the self-hosted worker's authentication (`oz-agent-worker`).
- **Codex**: `credential_broker.rs` was not opened by the reviewer (defaults only); the netns proxy
  bridge internals are unchecked; whether Codex guards an unauthenticated loopback listener from a
  sandboxed command with network on was not found; remote-control pairing-code expiry is
  server-side.
- **OpenClaw**: the secret egress proxy's bypass by a process that ignores proxy variables is
  inferred (no namespace found on that path); plugin install trust and the browser sandbox CDP token
  were not read in this round; the POSIX permission check on the GitHub profile directory was not
  seen.
- **Hermes Agent**: the terminal blocklist, the output redaction finding and credential pools are
  unchecked; the relay connector's buffer and use of the delivery key are not in the repository.
- **Oh My Pi**: no gateway token expiry was found, and routing after the gateway's auth check was not
  traced; MCP OAuth and export redaction are unchecked; the CORS attack on `--no-auth` is inferred.
- **OpenCode**: `--remote` encryption lives in `@opentunnel/*`; MCP OAuth was not read; the
  self-approval path with the leaked password is inferred.
- **Cline**: the provider key's lifetime in the remote hub's memory and whether it reaches disk;
  the Origin-shortcut exploit was not run against a hub.
- **OpenCodex**: the Home side of the Remote Link relay was not traced end to end; the value-based
  log guard (finding 11), admin-token rules (16), the unauthenticated loopback listener (15), the
  stream preflight (24), failover ordering (22), the send budget (7) and metered consent (25) are
  unchecked. Recommendations in section 6 item 3 rest on several of these; re-read
  `combo-stream-preflight.ts`, `request-send-budget.ts` and `configured-credentials.ts` before
  copying numbers.
- **Zed**: whether `download_server_binary` refuses a `None` checksum; Zed's provider-key storage
  and collaboration auth were not studied.
- **goose**: the WebSocket Origin policy and the desktop's token-in-URL flow are unchecked.
- **For our own design**: whether Landlock (pull request 4) blocks `ptrace` and `/proc/<pid>/mem`
  of the node process from a confined child, and whether `systemd-creds` works for user services,
  were not in any code read; both need a test on our side before item 1 is claimed done.
- Repositories not cloned that would settle open points: `OpenHands/software-agent-sdk`,
  `warpdotdev/oz-agent-worker`, the `@opentunnel` packages, and the vendor relays and registries
  named above.

### Links, workers and client protocols

From [round2/synth/05-links-workers-protocols.md](round2/synth/05-links-workers-protocols.md).

- **OpenClaw transcript-commit ledger and protocol frames** were not checked in review; the
  fencing and credential findings that 6.1 relies on were.
- **Codex exec-server Noise channel internals** (stream numbers, reordering window) and the
  **remote-control** transport were read from definitions only; the relay, registry and pairing
  expiry are hosted services not in the repository.
- **Codex: what happens when no connection qualifies for a human-only approval** (the remainder
  of `outgoing_message.rs` after L430) was not traced.
- **Hermes relay** (ack after admission, 4401 semantics) and **Kanban** were not checked; the
  connector's buffering is outside the repository.
- **OpenCodex Remote Workspace RPC v2** (prepare/grant) and the handshake's security reading
  were not checked (crypto only spot-checked); the Home side of Remote Link was not traced;
  the macOS and Windows helper (`native/remote-workspace-helper`) was not read.
- **DeepSeek `follow` and reconnect, prompt idempotency and LiveShare** were not checked;
  LiveShare's relay is not in the repository.
- **goose**: whether `setsid` children and MCP server children survive a roaming revocation
  (shell children are killed). The iroh relay's visibility into ACP content was not verified.
- **OpenHands**: whether the Agent Server closes a socket that never sends the auth frame,
  how it orders and skips events, its fork and persistence: all in `software-agent-sdk`, not
  cloned.
- **Warp**: the server-side loop, inbox durability and ordering, and the self-hosted worker
  (`warpdotdev/oz-agent-worker`) were not available.
- **Zed**: registry download verification when `sha256` is absent (`download_server_binary` not
  read); a reader of `tool_permissions` in a crate outside the three searched is not ruled out;
  the ACP schema is in `agentclientprotocol/agent-client-protocol`, not cloned.
- **Cline**: the Origin-only exploit (create an auto-approved session in any directory) is code
  reading, not exercised; where a provider key is dropped from the remote hub's memory after a
  session was not found.
- **Oh My Pi**: the real collab relay and the approval wiring of guest prompts.
- **Pi**: how `apps/p2` authenticates its workers to its server (`owner-link.ts`) was not read.
- **Our own code**: whether `nodes.rs:208-212` drains or drops frames already queued from a
  replaced connection, and whether deltas are still written to the log on the current branches,
  must be checked in our tree before 6.1 and 6.4 are written; this synthesis cites them only
  from the design note and the Round 1 inventory.

### Session storage, resume, fork and checkpoints

From [round2/synth/06-sessions-fork-checkpoints.md](round2/synth/06-sessions-fork-checkpoints.md).

- **Codex:** the trigger of the first `persist()`; legacy-mode (copy) fork; the `rollout_lineage` fallback when a parent is missing. Only the writer lock, revert and the absence of checkpoints were reviewed; rollout format, resume, fork pointers and retention were not.
- **OpenCode:** behaviour of the log endpoint in a non-persisting server (inferred; a test with `persist:false` would settle it); whether `git gc` in the user's repository can invalidate snapshot objects reached through alternates (inferred; needs a gc-then-restore test); the implementation of the "inherited prefix" for forked aggregates. The cost guards, export, fork and revert findings were not reviewed.
- **OpenClaw:** what the worktree snapshot scans and its cost guards; whether the removal path always snapshots first; fork, schema, pending inputs and restart-resume were not reviewed.
- **Hermes:** whether hooks or filter drivers can fire in the checkpoint store (inferred not; a hostile `.gitattributes` plus a global filter would settle it); whether a command the regex misses loses its snapshot in practice (inferred); resume, branch, rewind, export and the schema were not reviewed.
- **Oh My Pi:** whether `/tree` navigation without a later write persists (inferred non-durable; a branch, exit and resume test would settle it); the Rust crates `pi-edit` and `pi-vcs` were not read, so a file-level undo there is not excluded; most storage plumbing (title slot, id generation, lease, 500,000-character cap, pluggable backends, resume, export, foreign import) was not reviewed.
- **Pi:** the restart consequence of a lost leaf is inferred (reviewer agreed); fork, resume, export, durable forks and the branch-summary path were not reviewed; durable's storage and task-commit code were not read.
- **DeepSeek:** log core append mechanics, projection cache, reconnect and prompt admission were not reviewed; `flock` on a shared network directory was not examined.
- **goose:** fork, truncate, session types and the run registry were not reviewed.
- **Cline:** a size or count cap for checkpoints was searched for in `checkpoint-hooks.ts` and not found; nothing was run, so behaviour on a large repository is unmeasured; restore-forking, the compaction sidecar and the checkpoint diff were not reviewed.
- **OpenHands and Warp:** the engine, storage format, event persistence and fork internals are in repositories that were not cloned (`OpenHands/software-agent-sdk`; Warp's server and `warp-proto-apis`). Warp's local SQLite conversation storage and resume were not read.
- **Zed:** the shallow clone has no history, so why per-prompt checkpoints were removed and what they did cannot be read; the cost reasoning is inference. The thread database was not described.
- **OpenCodex:** the hub session persistence (`workspace-sessions.ts` `persist()`) was not read.
- **Gaps a decision rests on:** no project bounds a full-coverage checkpoint in a very large repository without losing coverage or accepting O(tree), and nobody measured it. The input shows no project handling container-made writes other than excluding the container backend (Hermes). Nothing in the input confirms git's built-in filesystem monitor or an inotify flag as a safe "tree unchanged" signal for item 4; the bounded newer-file walk (item 4) and the bundle transfer for cross-node fork files (item 8) are proposals, not input findings.

### Context: prompts, compaction, instruction files, memory

From [round2/synth/07-context-compaction-memory.md](round2/synth/07-context-compaction-memory.md).

Cross-cutting:
- No input file measures compaction quality or cost; trigger fractions (0.5 to 0.9) and keep sizes (10K to 25K tokens) are choices, not tested values.
- Cache-preserving summary requests: OpenCode states the intent in a design comment, DeepSeek and Zed implement it; no finding measures the hit rate. Hermes quotes a vendor cost saving for its review fork (about 26%) that was not measured.
- A map-then-merge chunked summariser: not found in the findings (absence not confirmed).
- Whether any project verifies a summary against the original (facts preserved): only OpenClaw's audit of headings and identifiers is described.
- Findings that no reviewer checked and a decision might rest on are marked `unchecked` in section 2; the main ones are OpenCode's background precompute and compaction prompt text, DeepSeek's summariser and what-is-kept details, goose's compaction internals and spill, Oh My Pi's method chain, snapcompact and overflow handling (the reviewer's own "not checked" list), Hermes' provider-overflow ladder and prompt caching, OpenClaw's overflow recovery, summary generation, pruning and memory flush, and Codex's world-state diffing, local and remote compaction and memory read path. Re-read the code before copying a number from any of these.

Per project:
**Codex**
- How the context size is measured for the trigger (provider usage or estimate): not described in the findings.
- Whether a mid-session edit of a project `AGENTS.md` is ever re-read: concluded from the cache key; a run would settle it.
- `redact_secrets` rules: not read.
- The per-model prompt wording beyond the first catalog entry, and the server-side catalog refresh.
- Local and remote compaction details, the memory read path and the ad-hoc note tool were not checked in review.

**OpenCode**
- What happens when `agents.compaction.model` names a model with a smaller window than the session model; the input files do not describe a window check for the summariser.
- Whether any check exists that compaction output is smaller than its input (none described).
- Size limits on AGENTS.md and instruction URLs: none seen in `config/plugin/instruction.ts` or `instruction-discovery.ts`; a provider-side limit may exist. API-attached `InstructionEntry` has `MaxValueBytes` (`instruction-entry.ts` L18).
- Trust prompts for project plugins and skills outside `packages/core` (desktop or TUI).
- Memory: not searched beyond the three cluster files and their headings; no reviewer searched for absence.
- Unchecked findings a decision would rest on: instruction sources and nested AGENTS.md (descriptive, "same author care" per the review), background precompute, native compaction, the compaction prompt text. Re-read the code before copying a number.

**OpenClaw**
- The 40,000-token summariser input cap and `maxSummaryChars` 16,000 (value of `MAX_COMPACTION_SUMMARY_CHARS`, `sessions/compaction/request-budget.ts` not read).
- What a failed or interrupted compaction leaves in the transcript, and whether a compaction always opens a `session_windows` row with reason `compaction` (the sessions cluster did not read compaction).
- Context-engine plugin slot and its `legacy` default: how `ownsCompaction` changes the compaction path was not read.
- The workshop skill review prompt and writer; the consolidation (dreaming) prompt text.
- No check that compaction shrank the context and no summariser-window check were found (not searched for absence by a reviewer).
- Unchecked findings a decision would rest on: prompt assembly and cache prefix, summary generation, overflow ladder, memory flush, pruning, memory search, who writes memory. The review says to trust constants and structure and re-check anything that says a lane, queue or claim is global, durable or unsettled.

**Hermes Agent**
- Gateway "hygiene" compaction at 85%: file found (`gateway/run_turn_hygiene.py`), threshold not traced.
- Micro-compaction and `compression.prepare_ahead`: named, not traced; they may add a between-compaction pruning or precompute path.
- `curator.backup.keep` default (Round 1 says 2): `get_keep` not read.
- Prompt caching, the provider-overflow ladder, the lock parser and the instruction-file loader were not in the reviewer's checked set.
- Whether the summary request can share the main conversation's cache prefix: not found; the call is a single user message on an aux slot.
- What "provider-confirmed ineffective" means exactly in the breaker (no reviewer check).
- Reviewer caveat: the lowered trigger equals the whole summariser window, leaving no headroom for template and output.
- Whether `/skills approve` in `hermes_cli` adds a hash check beyond `skill_manage` (investigator found none at replay).

**Oh My Pi**
- The compaction algorithm itself (`prepareCompaction`, the summary prompt, what is kept verbatim beyond the 20,000-token keep-recent budget) and the meaning of `handoff`, `shake` and `soft`: not read in depth.
- Whether a summary request reuses the conversation's cache prefix; whether the trigger count is provider-reported or estimated beyond the stored-conversation floor.
- Whether a summariser with a window smaller than the history is detected anywhere other than by candidate ordering.
- Reviewer did not check: snapcompact, compaction triggers, context-length-error recovery, pruning, system-prompt assembly, instruction-file shadowing, compaction replay, sharpshooter. Decisions resting on those carry only the investigator's trace.
- Whether `ttsr-injection` messages are pinned through compaction: searched, but `prepareCompaction` and `tool-protection.ts` were not read fully.
- Local distillation memory, `learn`/`manage_skill` approval, and any session search: not traced.
- Instruction-file size budget: none reported; absence not confirmed by a reviewer.

**Pi**
- What happens when the summarisation input itself exceeds the window: no guard was found; the reading that it fails as a non-retryable overflow was not traced to a test (a test in `packages/coding-agent/test` would settle it; not read).
- Provider-specific `maxTokens` clamping for summary requests: the `_reserveTokens` parameter is unused, so any clamp is in `packages/ai`, not read.
- Findings 1.3, 1.4, 2.2, 2.3, 4.5, 4.6, 5.2 and 10.1 were not checked by a reviewer; only the numbers 2,000 lines and 50 KB were.
- The roughly 2,000-character default prompt size was not re-measured.
- Whether `todo.ts` state survives compaction was not tested.

**DeepSeek harness**
- What stops a hostile repository AGENTS.md: no trust gate in the code read; one could exist in the sessions or safety paths.
- Compaction findings on the summariser model, what-is-kept, recovery and model-switch details were not checked by a reviewer.
- The consequence of a committed summary with a failed `compaction/end` (lock open until `end-seed`) was not traced.
- What happens when the summarising model's window is smaller than the span: not covered in the input; the model-switch rule compacts against the new window and refuses the switch on failure.
- Whether the token meter prefers provider-reported usage over its own count: not stated in the input.
- Memory: a negative search cannot prove absence outside `packages/`.

**goose**
- Whether the deferral enabled-set counts messages hidden by compaction (a test in `compaction_lifecycle.rs` or `advertised_tools.rs` would settle it; not read).
- How the session model's `context_limit` is resolved for custom providers (`context_limit.rs`): not traced.
- The input describes no check that compaction shrank the context and no deterministic fallback; no reviewer searched for either.
- The summariser format, MOIM, context report and compaction-storage findings were not in the reviewed set; the 800-character and 48,000-byte numbers and the cutoff examples were not recomputed.
- Memory: no size limit, no background review and no session search are described; no reviewer searched for them specifically.
- The reactive-compaction cap is read from a comparison, not exercised; `compaction_lifecycle.rs` was not read.

**Cline**
- Per-model system prompts: only the two templates exist here; the Desktop and Kanban repositories were not read.
- Whether the summary request reuses the conversation's cache prefix: not described.
- Whether any size cap applies to rule files: none found, no reviewer confirmed absence.
- Conditional-rule evaluator on the SDK path: found no caller; a runtime test would settle it.
- Behaviour when the summariser model's window is smaller than the prefix: not described beyond "input budget exhausted".

**OpenHands**
- The condenser's trigger unit, summary prompt, summariser model, overflow behaviour and what a failed condense leaves: all in `OpenHands/software-agent-sdk`, not cloned.
- The base system prompt text and any caching: not in this repository.
- Memory: not checked by any reviewer; SDK.
- Skills catalog and allow-list rule were not checked in review.

**Zed**
- Whether a `PromptTooLarge` path elsewhere triggers compaction: inferred from `is_transient`, not run.
- Instruction files, system prompt text and memory in Zed: outside the study's scope.
- Output limits and retry findings were not checked in review.

**Warp**
- The prompt, tool descriptions, compaction, model routing and fallback: server-side, schema repo not cloned.
- Whether the server also compacts on `ContextWindowExceeded`: not visible.
- What the `summary` column of `agent_conversations` stores and who writes it.
- Rules files and memory: not covered by the study.

**OpenCodex**
- What the client sends as history and when it compacts: outside this code.
- Whether the gateway estimates context size before routing (to pick a model with a large enough window): not described; `jev` combo decision backend and `routing/cost.ts` not read.
- Finding 28 (compaction) and the failover finding were not checked in review.

### Edit tools and the mechanics behind the scenes

From [round2/synth/08-edit-tools-mechanics.md](round2/synth/08-edit-tools-mechanics.md).

- **Edit-tool findings no reviewer checked** that a decision here rests on: Hermes' nine-strategy chain, its mtime warning and its lint delta; OpenClaw `edit` and `apply_patch` (including post-write verification and the success-on-throw behaviour); Codex's freeform grammar and line-ending handling; Oh My Pi's edit grammar and `sloppy`; Pi's argument repair and output-cap numbers; goose's process cleanup and unbounded collection; Cline's background detach, VS Code terminal and cancel; Zed terminal output cap; Warp's PTY mechanics; OpenCode's tool-input repair, large-repository snapshot guards and undo; OpenCodex's RPC and tool set. Re-read the cited lines before building on any number.
- **Oh My Pi TTSR** matching, abort, judge and compaction findings were not checked beyond default settings; the 50 ms delay, the `contextMode: discard` default and the absence of a retry counter all come from the research author's read. The claim that a catastrophic regex stalls the session is `inferred` (no timeout was found).
- **Hashline collision consequence** (a live 16-bit collision bypassing recovery) is `inferred` from the code path; a test with two texts sharing a tag would settle it. The Round 1 benchmark claims (hashline beating patch for 14 of 16 models) were not run.
- **Post-edit diagnostics** in Oh My Pi (`packages/coding-agent/src/edit/index.ts`) and Hermes (LSP layer), and Round 1's claim that Claude Code runs diagnostics after each edit, are not settled by Round 2. Claude Code was not cloned.
- **Hermes' staleness path**: whether `patch` uses the byte-snapshot helper or only the mtime was not traced.
- **Whether git hooks or filter drivers can fire in Hermes' checkpoint store**: untested.
- **Why Zed's per-prompt checkpoints hung** is `inferred`; the history is not in the clone.
- **Process lifetime in Codex's unified exec** beyond the limits read; shell kill and background mechanics for OpenClaw `exec`, Zed's terminal, Warp's blocks and DeepSeek's `bash` tool were not in the findings.
- **OpenCode formatter default**: Round 1 says off unless enabled; the Round 2 files show a formatter running in `commit` but do not show the config gate.
- **Tool-result and read/search tool internals** (OpenClaw `read`, Codex tool caps, Oh My Pi `read` and `grep` caps, OpenCode's `grep`/`glob` limits): not in the findings; only Pi, Cline, OpenCode and our own numbers are known.
- **OpenHands** `file_editor` and its stuck detector (SDK repository), **Warp's** server-side edit logic and **Claude Code** were not cloned.
- **Cross-file atomicity**: no implementation read provides rollback; whether crash-safe multi-file commit matters for our users is a product call, not answerable from the code.
- **Whether clearing the "seen" table at compaction (6.1 item 6) helps or only adds re-reads** has no evidence in the files; measure it after pull request 7 lands.

### Sub-agents, orchestration, model roles, fallback and retry

From [round2/synth/09-subagents-models-retry.md](round2/synth/09-subagents-models-retry.md).

- **OpenClaw per-attempt retry** (`src/llm/utils/retry.ts`) and the per-run retry budget (`run/retry-budget.ts`) were not read by the author: no retry count, delay or jitter for OpenClaw. Its fallback loop body (lines 440 to 745) was skimmed, and its spawn path, child model, result announcement and fallback chain were not reviewed.
- **Zed** has no sub-agent section in the input; whether its native agent has a sub-agent tool is unknown here. Its retry numbers were not given (`retry_strategy_for`; "output limits and retry" was not reviewed).
- **Warp**: retry delays and `Retry-After`, model selection and fallback, the child's context and result are server-side; the message bus's ordering and durability are invisible.
- **OpenHands**: SDK sub-agent depth, concurrency, model and permission inheritance; retry and fallback; ACP permission handling. Only a captured snapshot (not line-citable) gives `num_retries 5`.
- **Hermes**: the child run loop, auxiliary slots, retry, fallback chain and credential pools were not reviewed (only the 250-versus-50 and child-permission findings were). The auto-recovery ladder and OAuth refresh were not read.
- **OMP**: alias resolution, chains, triggers and `yield` were not reviewed. The `IRC peer roster` in the child prompt is named but not described. `usageAwareFallback` behaviour is not described.
- **OpenCodex**: findings 7, 18 to 22, 24 and 28 were not reviewed; the Home side of the link, `jev`, and the thread-affinity store were not read.
- **Codex**: result return and spawn configuration details were not reviewed; the count of 9 of 12 catalogued models with `multi_agent_version: v2` was not recounted. Codex has no cooldown or credential pool in the findings.
- **DeepSeek**: Claude Code result collection was not read in full; the Auto-review reaction to a child at the `auto` preset was not traced; the `workflow` and pool interaction is `inferred`.
- **goose**: named-agent loading, parallel dispatch, `/goal` and the empty-response retry were not reviewed; the "two third-party classifiers" are referred to in a finding I did not read.
- **Pi**: the example router `jev-router.ts` was not read; model selection (6.1, 6.2) was not reviewed.
- **Our own code:** I did not read the PR 4, 6 or 7 branches. Whether PR 7 already has jitter, a `Retry-After` cap, a no-network schedule or per-reason classes is unknown to me. The alias and slot tables are from the design note only (the code on `main` has no title generator or vision bridge, per the design note).
- **Two points where input files disagreed**, resolved by the reviewers' corrections and used as corrected: OpenCode child permissions (the agents file after review says agent rules plus session rules; the original safety text said otherwise); OpenClaw's lane width (per spawning session, not gateway-wide). Pi's `Retry-After` (inert by default) was also corrected in review.
- **No repository was left uncloned**; the limits above are limits of reading, not of access.

### Extensibility and unattended operation

From [round2/synth/10-extensibility-unattended.md](round2/synth/10-extensibility-unattended.md).

- **Whether a stdio MCP child inherits the harness environment** in Codex and OpenCode: not stated in the findings. Whether an OpenClaw stdio server does: not read.
- **OAuth detail** for OpenCode (`mcp/oauth.ts`, 667 lines, not read), OpenClaw, Warp and Cline beyond token storage; Codex's PKCE is delegated to `rmcp` and not traced.
- **Codex Code Mode** runtime isolation and approval interaction were not traced; the catalog-dependent `code_mode_only` count was not recounted by the reviewer.
- **Findings no reviewer checked that a decision here rests on**: the whole of Codex MCP client, OAuth, deferral, plugins and `list_changed` (section 3.7 of its sessions file); OpenCode skills, schedules and instruction-source diffs; OpenClaw MCP, standing grants, heartbeat, webhooks, delivery queue and skill tiers; Hermes tool search, the cron admission journal and plugin trust; Oh My Pi tool cache, OAuth and foreign import; Pi skills discovery, the two-pass start-up, tool-search activation as a recorded loadout and deterministic replay; DeepSeek schedule and headless; goose deferral and MCP roots; Cline marketplace, skills and import; OpenHands skills, MCP and automation; Zed ACP registry install and spawn; Warp MCP tool lists and permissions.
- **Hook failure handling** in DeepSeek's `hooks-claude-code` and `hooks-codex` bridges was not read (`runPoint`).
- **Whether workflow children (DeepSeek) and code-mode inner calls (Codex)** go through approvals.
- **Whether a project-plugin or hook trust prompt exists in a UI layer** for OpenCode, goose and Cline: the findings are negative searches in the engine, and goose says a prompt could live in the UI.
- **Whether Cline file hooks run under `--yolo`** (code does not remove the file-hook extension; docs say hooks are disabled); **whether a Cline schedule created while the hub is stopped fires later**.
- **Whether Zed's registry download rejects a missing checksum**; Zed's `context_server` MCP crates and `agent_skills` were not read.
- **Repositories not available**: the OpenHands automation service and Software Agent SDK (hook semantics, MCP client, skill triggers), Warp's server-side loop and automations, OpenClaw's plugin install trust and ClawHub policy (not read), the ACP schema repository.
- **`AUDIT.md` and `FIX_PLAN.md`** were not read, so the freeze cost of each step is not assessed.
