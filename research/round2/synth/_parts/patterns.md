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
