# goose: Round 2 source investigation, cluster "safety"

## 1. Repository

- `aaif-goose/goose`, branch `main`, pinned SHA `14bf51817b79f6e98ca73c6eda61c11d9d5cc892`, commit date 2026-11-10T19:00:38Z (from `research/round2/00-repos.tsv`; `git log -1` agrees).
- Read: `crates/goose-agent/src/{machine,tool_approval,tool_call_state,pre_tool_call_hook}.rs` (the generic loop, the "GDK"); `crates/goose/src/agents/{state_machine_builder,services,subagent_handler,retry,container}.rs`, `agents/state_machine/{mod,session,tool_approval,tool_confirmation,ops_toolcalling,ops_maxturns}.rs`; `tool_inspection.rs`, `tool_monitor.rs`; `permission/*`; `config/{permission,base}.rs` (secrets, mode); `security/*` (all six files); `hooks/mod.rs` (loader, chain, runner); `plugins/discovery.rs`; `agents/extension_malware_check.rs`, `agents/extension_manager/{stdio,builtin,mod}.rs` (connect path); `agents/platform_extensions/{mod,code_execution}.rs` and `developer/{mod,shell,shell_process,edit}.rs`; `subprocess.rs`; `providers/{claude_acp,codex_acp}.rs` and `acp/provider.rs` (mode mapping, permission requests); `providers/private_file.rs`; Desktop `ui/desktop/src/{main.ts,allowlistFetcher.ts,components/ExtensionInstallModal.tsx,components/settings/extensions/deeplink.ts}`; `documentation/blog/2026-02-23-goose-v1-25-0/index.md` (for the sandbox history).
- Not read in depth: providers' own HTTP retry, the roaming crate, the ACP server dispatch, compaction (other clusters).

## 2. Architecture as it bears on this cluster

- **Process model.** One Rust binary. The agent loop, the permission inspectors and the built-in tools all run inside the `goose` process (`goose session`, `goose serve`/`goose acp`, or the server the Desktop app launches). Built-in "platform" extensions (Developer: `shell`, `write`, `edit`, `tree`, `read_image`; Code Mode; Summon; Todo; ...) are in-process `McpClientTrait` objects; other extensions are MCP servers spawned as child processes or reached over HTTP.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/mod.rs#L932-L968 : platform extensions are constructed in-process from `PLATFORM_EXTENSIONS`; `Builtin` goes through `builtin::connect`; `Stdio` spawns a child with `AGENT_SESSION_ID` added.
- **The loop is an ordered list of operations plus one inference step.** `StateMachine::step` loads the session, tries each step in order, and returns on the first that applies; `run` repeats load/step/apply until a step yields or the run is cancelled. Every decision (approval, hook verdict, execution) is persisted into the conversation as message content or tool-request metadata, then the next pass re-derives state from it.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/machine.rs#L154-L212 : `step`: iterate steps, first `Applied` wins, races each step against the cancel token.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/machine.rs#L233-L248 : `run`: load, step, apply effects, loop while `Continuing`.
- **The step order is the security pipeline.** Slash commands, then `ToolApprovalOperation`, `PreToolCallHookOperation`, skills, recipes, `ToolExecutionOperation`, remaining/unhandled calls, subagents, steering, `!` shell, compaction, retry, stop hook, max turns, and finally inference.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine_builder.rs#L127-L166 : approval and the PreToolUse hook are placed before execution.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine_builder.rs#L182-L225 : the rest of the ops, then the single `Step::Inference` last.
- **Where permission decisions live.** `ToolInspectionManager` holds five inspectors in order: Security (patterns plus optional classifier), Egress (log only), Adversary (LLM reviewer), Permission (mode plus per-tool levels plus LLM read-only judge), Repetition. `GooseToolApproval` turns their results into Allow / Deny / Ask per tool call.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/services.rs#L564-L592 : the five inspectors and their order.
- **Compaction** is two operations in the same list (`CompactionOperation`, `ToolPairCompactionOperation`), skipped when the provider manages its own context (state_machine_builder.rs L167-L188); details belong to the agents cluster.

## 3. Findings

### The agent loop and tool dispatch

#### Cancellation drops the running step and back-fills "interrupted" tool results
- What the code does: every step's future is raced against the run's `CancellationToken` with a biased `select!`. On cancel the step future is dropped, the index of the interrupted step is remembered, and `finalize` first lets that step's `finalize_cancellation` write its effects, then appends an error tool response "Tool call was interrupted before completing" for every tool request that has no response, then lets every other step finalize.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/machine.rs#L181-L193 : the race; a cancelled step is recorded in `interrupted_step`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/machine.rs#L250-L285 : `finalize`: interrupted step first, then synthetic responses, then the others.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/machine.rs#L74-L88 : the synthetic "interrupted" error result for each open call.
- Edge cases and failure handling: dropping the tool future is what kills a running shell: `ShellProcess`'s `Drop` sends SIGKILL to the command's process group while it is still armed (see the shell findings). An error that arrives after cancel is swallowed and reported as `Cancelled` (L188-L192).
- Round 1: `confirmed` ("cancellation drops the running step and lets operations finalize interrupted tool responses"); this answers the Round 2 question on cancellation and in-flight tool calls.
- For rizzy-harness: the pattern (persist state, re-derive on every pass, close every open tool call on cancel) is the same shape as our fold over the log; copy the "every open call gets a terminal result" rule.
- Confidence: traced end to end.

#### Approval state is a flag in the persisted tool-request metadata, re-derived every pass
- What the code does: a tool call's state is computed from the messages: a response means `Responded`; `goose.executable: true` in `tool_meta` means `Executable`; no flag means `Unchecked`; `false` plus a confirmation request means `AwaitingUser` or `UserAnswered(permission)` once the confirmation-response message exists. Approval writes the flag through a `PatchToolRequestMeta` effect.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/tool_call_state.rs#L58-L77 : the state derivation.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/tool_call_state.rs#L87-L96 : `request_executable` and `mark_executable` (the `goose.executable` key).
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/tool_approval.rs#L161-L193 : `ToolApprovalOperation::run`: a user answer is applied first, otherwise all `Unchecked` calls are checked as a batch.
- Edge cases and failure handling: the number of decisions must equal the number of calls or the operation errors (tool_approval.rs L106-L111). A user answer of anything other than `AllowOnce`/`AlwaysAllow` (that is `DenyOnce`, `AlwaysDeny`, `Cancel`) produces the declined error result (L84-L91). I found no provider format that writes `tool_meta`, so the model cannot pre-set the flag through a provider response; I did not audit every provider.
- Round 1: `new (not documented)`.
- For rizzy-harness: keeping the approval as an event bound to the tool-call id matches our design; note that goose binds it to the call id only, not to a hash of the arguments.
- Confidence: traced end to end.

#### A tool call only runs if its name was advertised to the model in that inference, and approvals expire with the extension lease
- What the code does: `request_was_advertised` checks the tool name against the list of tools recorded on the assistant message that produced it; unadvertised calls are never inspected or executed. Before executing, the operation needs the current "extension lease"; if the process restarted and the lease is gone, already-approved model calls are answered "Tool approval expired because its extension lease is no longer available. Request the tool again." Only operation-generated calls without approval history may resolve a fresh lease.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L705-L742 : `call_will_not_run` and `request_was_advertised`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L999-L1039 : no lease, no execution; every pending call gets the expired-approval error.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L782-L818 : the "can resolve a lease" rule (operation-generated, no approval history).
- Edge cases and failure handling: names are re-checked against the lease's current tool set and unknown names are dropped from the batch (L1041-L1058). Chat mode answers every call with a canned "explain the plan instead" success result (L1060-L1081).
- Round 1: `new (not documented)`.
- For rizzy-harness: "an approval does not survive a restart" is a cheap, good rule for us too: a node that restarts should not execute calls approved before the restart without asking again.
- Confidence: traced end to end (checked in review)

#### Tool calls in one batch are dispatched sequentially, their streams merged; no per-call timeout in the loop
- What the code does: `ToolExecutionOperation::run` awaits `dispatch_tool_call` for each executable request in turn (each returns a result future plus notification streams), then merges the streams with `select_all` and drains them; results are recorded into one batch message. The only time limits are inside the tools (shell timeout, MCP extension timeout, Code Mode timeout) and the turn limit.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L1083-L1157 : dispatch loop, merged stream, one response message.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L413-L474 : `dispatch_tool_call`: extended pre-hooks, `lease.call`, error to a tool result.
- Edge cases and failure handling: a dispatch error becomes an error tool result, not a loop error (L451-L458). An empty batch is an error ("tool batch produced no responses", L1155).
- Round 1: `new (not documented)`.
- For rizzy-harness: nothing to copy specifically; note the absence of a loop-level wall clock per call.
- Confidence: read the definition.

#### Turn limit: 1000 assistant turns, then a canned "continue?" message
- What the code does: `MaxTurnsOperation` counts assistant turns since the kickoff message; at the limit it yields with "I've reached the maximum number of actions I can do without user input. Would you like me to continue?". From half the budget it injects `<turn-budget>N/M used</turn-budget>` into the per-turn context. Default is `GOOSE_MAX_TURNS` or 1000.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_maxturns.rs#L20-L67 : budget hint and the yield.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L1358-L1360 : default 1000.
- Edge cases and failure handling: `max_turns == 0` suppresses the hint (L21) but `assistant_turn_count < 0` is never true, so 0 yields immediately.
- Round 1: `confirmed` (default 1000).
- For rizzy-harness: the half-budget hint to the model is a cheap idea.
- Confidence: traced end to end.

#### `--max-tool-repetitions` and the RepetitionInspector are inert
- What the code does: `RepetitionInspector` is registered with `max_repetitions: None`, and its `inspect` checks a temporary clone of its state without ever updating `self`, so even with a limit it would never count calls across turns. The CLI flag is stored in the session builder config and not passed to the inspector.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/services.rs#L588-L589 : registered with `None`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_monitor.rs#L99-L134 : `inspect` works on `temp_inspector`; `self` is never mutated.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_monitor.rs#L51-L63 : with `None` every call passes.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/session/builder.rs#L185 : the CLI field; a grep finds no other non-test use.
- Edge cases and failure handling: none; it never denies.
- Round 1: `corrected`. Round 1 listed the flag "but not explained"; the code shows it has no effect.
- For rizzy-harness: a stuck-loop guard needs state that persists across turns (in our case, a fold over the log), not an inspector-local clone.
- Confidence: traced end to end (checked in review)

### Approvals and permission rules

#### Four modes, default `auto`, and what each does in the Permission inspector
- What the code does: `GooseMode` is `Auto` (default), `Approve`, `SmartApprove`, `Chat`. `resolve_goose_mode` returns `Auto` when `GOOSE_MODE` is not set. In the Permission inspector: `Auto` allows every call; `Approve`/`SmartApprove` check the user's per-tool level first; then (SmartApprove only) a tool annotated `readOnlyHint: true` is allowed; `platform__manage_extensions` always asks; then SmartApprove sends uncached tools to the LLM judge; anything else asks. `Chat` short-circuits before inspection: no call runs.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-provider-types/src/goose_mode.rs#L22-L32 : the enum, `Auto` is `#[default]`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L1316-L1323 : unset means `Auto`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_inspector.rs#L155-L196 : the decision ladder per mode.
- Edge cases and failure handling: when the Permission inspector produced no result for a call (for example its `tool_call` failed to parse), `process_inspection_results` puts it in `needs_approval` "for safety" (L110-L113). In `Approve` mode the read-only annotation and the smart-approve cache are ignored (tests at L327-L329).
- Round 1: `confirmed` (four modes, default autonomous).
- For rizzy-harness: the default is the opposite of ours; nothing to copy except the "no result means ask" fallback.
- Confidence: traced end to end (checked in review)

#### Per-tool permissions are keyed by tool name only, global to the user, in `permission.yaml`
- What the code does: `PermissionManager` holds `{category: {always_allow, ask_before, never_allow}}` with two categories, `user` and `smart_approve`, in `~/.config/goose/permission.yaml`. Lookup is by exact tool name; `never_allow` wins over `always_allow` over `ask_before`. Answering "Always allow" or "Always deny" to a prompt writes the tool name into the `user` category; there is no argument, project or session scoping. Writes take an exclusive `fs2` lock, re-read the file, mutate and write.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/permission.rs#L19-L44 : levels, config shape, the two categories.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/permission.rs#L134-L157 : lookup precedence by name.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/tool_approval.rs#L108-L126 : `on_confirmation`: AlwaysAllow/AlwaysDeny become a name-wide user level.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/permission.rs#L196-L222 : locked read-modify-write.
- Edge cases and failure handling: a corrupt `permission.yaml` panics the process on purpose ("Refusing to start with corrupted permission config", L224-L238). A symlinked file is followed and the target written (L240-L258). "Always allow" on `shell` therefore allows every future shell command in every project.
- Round 1: `confirmed` (three levels, `permission.yaml`), with the binding detail added.
- For rizzy-harness: this is exactly the "per tool name" binding our plan wants to move away from; goose is a reference for what not to do.
- Confidence: traced end to end.

#### `permissions/tool_permissions.json` (argument-hashed records with expiry) is dead code
- What the code does: `ToolPermissionStore` would key decisions by `tool_name:blake3(arguments)` with optional expiry and write `~/.config/goose/permissions/tool_permissions.json` atomically. It is re-exported from `permission/mod.rs` and used nowhere else in the workspace.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_store.rs#L79-L127 : `check_permission`, `record_permission`, `hash_tool_context`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/mod.rs#L10 : the only reference outside the file (a re-export).
- Edge cases and failure handling: `check_permission` unwraps `tool_call` and would panic on a parse-error request (L81), moot since it is not called.
- Round 1: `corrected`. The config page lists the file as "Runtime permission decisions (auto-managed)" (`documentation/docs/guides/config-files.md` line 21); the code never writes it.
- For rizzy-harness: the abandoned design (hash of arguments plus expiry) is the right direction; we should finish what goose did not.
- Confidence: traced end to end (grep over `crates`, `ui/desktop/src`) (checked in review)

#### Smart Approve: an LLM read-only judge that fails closed, with name-wide negative caching
- What the code does: uncached tools in SmartApprove go to `detect_read_only_requests`, which calls the session's own provider and model with a system prompt from `permission_judge.md` and one user message `UNTRUSTED TOOL REQUEST DATA (JSON)` listing request id, tool name and arguments, and forces a `platform__tool_by_tool_permission` tool whose argument is the list of read-only request ids. It sees no conversation. Ids returned are allowed for this call only; any tool judged not read-only is cached name-wide as `ask_before` in the `smart_approve` category, so it is never judged again.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_judge.rs#L41-L89 : the judge tool and its read-only definition ("If you cannot make the decision, then it is not read-only").
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/prompts/permission_judge.md#L1 : the system prompt (treat ids, names, arguments as untrusted data).
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_judge.rs#L145-L185 : every failure (no model config, provider error, no tool call) returns an empty list.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_inspector.rs#L229-L270 : not read-only means `RequireApproval` and the negative cache write.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_inspector.rs#L22-L34 : only negative decisions are cached.
- Edge cases and failure handling: fails closed (empty list means ask). No timeout of its own; it inherits the provider's. An older cached `always_allow` in `smart_approve` is re-judged (test L361-L373). One judgement per batch, so one provider call per batch of uncached tools.
- Round 1: `corrected`. Round 1 said "criteria are not described"; the code shows the criteria (the tool description), what it sees (only name and arguments), and that it fails closed.
- For rizzy-harness: if we ever add a model approver, copy the shape: forced tool output, ids not names, untrusted-data framing, fail closed, cache only "not safe".
- Confidence: traced end to end.

#### SmartApprove trusts MCP servers' own `readOnlyHint`
- What the code does: before each inference, tool annotations from every extension are applied: tools with `readOnlyHint: true` go into an in-memory set that SmartApprove allows without asking; tools with `readOnlyHint: false` are written to the `smart_approve` cache as `ask_before`. The annotation comes from the extension's `list_tools` response.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_inspector.rs#L52-L64 : read-only set from annotations.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_inspector.rs#L172-L176 : annotation alone allows the call in SmartApprove.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/inference_preparation.rs#L58 : called while preparing each request.
- Edge cases and failure handling: a third-party MCP server that labels a destructive tool read-only gets it auto-approved in SmartApprove. The set is one per services instance, overwritten on each inference preparation.
- Round 1: `new (not documented)`.
- For rizzy-harness: never let a tool's self-description lower its approval level on the node; annotations may only raise it.
- Confidence: traced end to end (checked in review)

#### How inspector results combine: deny beats ask beats allow, and a failing inspector is skipped
- What the code does: the Permission inspector's result is the baseline per call; every other inspector's result is then applied as an override: `Deny` moves the call to denied, `RequireApproval` moves an approved call to needs-approval, `Allow` never overrides. `inspect_tools` runs inspectors sequentially; if one returns `Err` it is logged and skipped.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_inspection.rs#L84-L118 : sequential run; an error is logged, "Continue with other inspectors even if one fails".
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_inspection.rs#L213-L257 : the override rules.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/tool_approval.rs#L57-L105 : denied calls get `DECLINED_RESPONSE`; asked calls carry the inspector's warning as the prompt.
- Edge cases and failure handling: a Security-inspector error (for example a classifier failure propagated with `?`) silently removes that layer for the batch. A denied call's result text is always the generic `DECLINED_RESPONSE` ("The user has declined to run this tool. DO NOT attempt to call this tool again..."), even when the adversary denied it, so the model is told the user declined.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/tool_execution.rs#L151-L153 : the text.
- Round 1: `new (not documented)`.
- For rizzy-harness: an approval layer that errors must deny or ask, not vanish; log which layer decided.
- Confidence: traced end to end (checked in review)

#### No shell parsing anywhere in the permission path
- What the code does: per-tool levels match the tool name. The pattern scanner, the egress inspector and the adversary all read the raw `command` string (regexes or an LLM); nothing splits chains, substitutions or wrappers before a decision.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/scanner.rs#L403-L421 : the scanner takes `arguments.command` as one string.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/egress_inspector.rs#L293-L305 : egress takes `command`/`cmd`/`script`/`input` as one string.
- Edge cases and failure handling: n/a.
- Round 1: `new (not documented)`.
- For rizzy-harness: goose offers no model for command rules; look to OpenCode and Codex for that.
- Confidence: read the definition.

#### Adversary mode: LLM reviewer of `shell` calls, enabled by a file, fails open
- What the code does: enabled when `~/.config/goose/adversary.md` exists. Optional `tools: a, b` before a `---` line selects reviewed tool names (default `shell`); the rest is the rules (default: block exfiltration, destructive beyond project scope, malware or obfuscated code, unnecessary privilege escalation, download-and-execute; "Err on the side of ALLOW"). For each reviewed call it calls the session's provider and model, no tools, with a fixed system prompt ("adversarial security reviewer, protecting the user in case the other agent is rogue") and a user message holding the first user message (truncated to 500 chars), up to the last 4 user text messages (each truncated to 200 chars), the tool name plus pretty-printed arguments, and the rules. It sees no tool results and no assistant text.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/adversary_inspector.rs#L14-L27 : default tool list, default rules, 4 recent messages.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/adversary_inspector.rs#L84-L117 : file existence enables it; an unreadable file falls back to the defaults (still enabled).
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/adversary_inspector.rs#L201-L247 : what context it extracts (user text only).
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/adversary_inspector.rs#L276-L306 : the user message and system prompt, `provider.complete(..., &[])`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/adversary_inspector.rs#L320-L346 : verdict parsing: BLOCK only if the reply starts with or has a line starting with BLOCK; anything else allows.
- Edge cases and failure handling: fail open on three paths: no session or provider returns `(true, "No provider available")` (L257-L262); a provider error becomes an `Allow` result with confidence 0 (L446-L464); an unparseable reply allows. No timeout in the inspector. A BLOCK is `Deny` (no user prompt), reported to the model as "user declined". The config is read once per process (`OnceLock`). Not reviewed: Code Mode's inner tool calls (see Code Mode finding), so `execute_javascript` calling `shell` bypasses it with the default tool list.
- Round 1: `confirmed` (file-enabled, `shell` default, `tools:` header, fails open), with mechanism added. The doc comment's example `developer__shell` (L42) does not match today's unprefixed `shell` name.
- For rizzy-harness: a second-opinion reviewer is cheap but must fail closed for anything it was asked to review, and must review calls made from inside code-execution tools.
- Confidence: traced end to end (checked in review)

#### ACP providers: goose's mode is mapped onto the other agent's mode, and `auto` answers its permission prompts itself
- What the code does: Claude via ACP: `auto` -> `bypassPermissions`, `approve` -> `default`, `smart_approve` -> `acceptEdits`, `chat` -> `plan`. Codex via ACP: `auto` -> `agent-full-access`, `smart_approve` -> `agent`, `approve` and `chat` -> `read-only`. When the adapter sends a permission request, goose answers `AllowOnce` itself in `auto`, `RejectOnce` in `chat`, and only forwards it to the user in the two approval modes.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/providers/claude_acp.rs#L74-L83 : Claude mapping.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/providers/codex_acp.rs#L74-L77 : Codex mapping.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/provider.rs#L1028-L1054 : permission request handling; a dropped confirmation channel becomes `Cancel`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/provider.rs#L2388-L2394 : the mode-to-decision table.
- Edge cases and failure handling: in these providers the tools run inside the other agent, so goose's own inspectors (adversary, patterns) do not see them; only the adapter's mode and goose's yes/no on its prompts apply.
- Round 1: `confirmed` (mapping), and it answers the Round 2 question on how modes are translated.
- For rizzy-harness: relevant for any `AgentBackend` that hosts another harness: the hosted agent's own permission prompts need a route to the node's policy, not an automatic yes.
- Confidence: read the definition (checked in review)

#### Sub-agents inherit the parent's mode, and their approval prompts are forwarded to the parent
- What the code does: a Summon sub-agent session is created with the parent session's `goose_mode` (`Auto` only when there is no parent). Confirmation and elicitation requests produced by the sub-agent get a new forwarded id registered with the parent, so the parent's user answers them.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/summon.rs#L410-L436 : mode inherited from the parent session.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/subagent_handler.rs#L227-L245 : ids of ToolConfirmation and Elicitation rewritten to forwarded ids.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/action_required_manager.rs#L70-L84 : the forwarding table.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine_builder.rs#L157-L163 : concurrency default 5 (`GOOSE_MAX_CONCURRENT_SUBAGENTS`).
- Edge cases and failure handling: I found no check in the Summon path that refuses delegation outside `auto`; I did not read the whole 2,800-line `summon.rs`.
- Round 1: `refuted` at the code level for "Subagents are disabled in non-autonomous modes" (the docs page says so; the code inherits the mode and forwards approvals). Settle by reading `summon.rs` fully or running `delegate` in `approve` mode.
- For rizzy-harness: inherit-and-forward is the shape we want for children; it keeps one approver.
- Confidence: read the definition (checked in review)

#### Code Mode: inner `tools.call()` bypasses approval, inspectors and PreToolUse hooks
- What the code does: Code Mode (platform extension, off by default) exposes `execute_javascript`. Inside Boa, `tools.call(name, args)` checks the name against the lease's tool catalog and calls `lease.call` directly. `ExtensionLease::call` resolves and runs the tool; it has no permission check. So the approval pipeline sees one call, `execute_javascript`; the shell, write and MCP calls it makes are not shown to the Permission, Security or Adversary inspectors nor to `PreToolUse` hooks.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L100-L146 : the callback: catalog membership, then `dispatch_tool`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L172-L233 : `dispatch_tool` calls `dispatching_lease(&ctx).call(...)`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/lease.rs#L653-L663 : `call` = resolve + `call_resolved`, no inspection.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/mod.rs#L156-L168 : `default_enabled: false`.
- Edge cases and failure handling: `execute_javascript` is annotated not read-only (code_execution.rs L279), so in Approve/SmartApprove the user is asked once per script; in Auto nothing asks. The JS engine has a 2,000,000-instruction budget, 100,000 loop iterations, recursion 128, 1 MB output and the extension timeout (default 300 s) (`javascript.rs` L27-L31, L231-L232; read by grep only).
- Round 1: `new (not documented)`; answers the Round 2 question "how `tools.call()` enforces per-tool permissions": it does not.
- For rizzy-harness: any tool that calls other tools must route each inner call through the same node-side approval path; otherwise it is an approval bypass.
- Confidence: traced end to end (checked in review)

### Prompt-injection detection and other inspectors

#### "Prompt injection detection" scans only `shell` commands, off by default; only Critical patterns trip the default threshold
- What the code does: the Security inspector is enabled by `SECURITY_PROMPT_ENABLED` (default false; env `SECURITY_PROMPT_ENABLED_OVERRIDE` wins). It scans only calls named exactly `shell`, takes the `command` argument, matches 36 case-insensitive regexes with confidences Critical 0.95, High 0.75, Medium 0.60, Low 0.45, and flags the call when the final confidence is at or above `SECURITY_PROMPT_THRESHOLD` (default 0.8). The flag is `RequireApproval` with a "Security Alert" text and a `SEC-<uuid>` finding id; it never denies.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/mod.rs#L46-L55 : enable switch, default false.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/scanner.rs#L132-L144 : non-`shell` tools are skipped.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/scanner.rs#L424-L426 : `is_shell_tool_name` is `"shell"` only.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/patterns.rs#L36-L45 : risk-to-confidence table.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/patterns.rs#L107-L113 : SSH key exfiltration is `High` (0.75), below the 0.8 default.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/security_inspector.rs#L27-L36 : malicious means `RequireApproval`, never `Deny`.
- Edge cases and failure handling: in pattern-only mode, High patterns (SSH key exfiltration, `/etc/shadow` reads) cannot reach 0.8 on their own, so only Critical patterns (rm -rf of root or home, dd to a disk, mkfs, curl piped to a shell) prompt. Unknown tools (MCP shells with other names, Code Mode) are not scanned.
- Round 1: `corrected`. Round 1 said parameter text of tool calls is matched; the code limits it to the `shell` tool and the default threshold excludes the High patterns it lists (SSH key exfiltration).
- For rizzy-harness: regex scanners of command strings are a weak signal; if we keep one, make it an "ask" layer and test that its threshold actually fires on the patterns it lists.
- Confidence: traced end to end (checked in review)

#### The pluggable classifier: Hugging Face text-classification over HTTP; the "prompt" classifier scans user messages, not tool output
- What the code does: two optional classifiers. The command classifier gets the shell command; the prompt classifier gets the last 10 user text messages (tool responses are excluded by `effective_role`), at most 3 requests in flight. Each is configured by `SECURITY_<KIND>_CLASSIFIER_MODEL` (looked up in the `SECURITY_ML_MODEL_MAPPING` env JSON) or `SECURITY_<KIND>_CLASSIFIER_ENDPOINT` plus a secret token. The client POSTs `{inputs, parameters}`, 5 s timeout, softmaxes scores if they are not probabilities, and maps labels `INJECTION`/`LABEL_1` to the score, `SAFE`/`LABEL_0` to 1 - score, anything else to 0. Confidences are combined (tool primary; context adds weight only when both are high).
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/scanner.rs#L65-L124 : classifier configuration and precedence (model mapping, endpoint, command-type fallback).
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/scanner.rs#L221-L265 : `scan_conversation` over user messages.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/scanner.rs#L378-L401 : `extract_user_messages` filters `EffectiveRole::User`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-provider-types/src/conversation.rs#L645-L654 : a user-role message holding a tool response is `EffectiveRole::Tool`, so tool output is never scanned.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/classification_client.rs#L141-L220 : request, normalisation, label mapping.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/scanner.rs#L267-L288 : confidence combination.
- Edge cases and failure handling: a classifier HTTP error or timeout is logged and treated as "no ML score" (scan_with_classifier L301-L307), so detection falls back to patterns: fail open. If ML was requested but neither classifier initialises, the scanner falls back to patterns with a warning (mod.rs L112-L129). An unknown label scores 0. The classifier receives the full command and up to 10 user messages: data egress to the configured URL.
- Round 1: `corrected`. Round 1 said the classifier gets "tool-call content and recent messages"; the code sends recent user messages only, never tool results, which is where indirect prompt injection arrives.
- For rizzy-harness: an "injection detector" that never looks at tool output does not detect indirect injection; do not copy the name or the design.
- Confidence: traced end to end (checked in review)

#### Egress inspector: always on, log only
- What the code does: for tools named like a shell (`shell`, `bash`, `*__shell`, ...) or a fetcher, it extracts URLs, `git@host:`, `s3://`, `gs://`, scp/rsync/ssh targets, docker push/login registries, generic network commands and `npm`/`cargo publish`, guesses a direction, logs each destination at info level and returns `Allow` with confidence 0.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/egress_inspector.rs#L49-L187 : destination extraction.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/egress_inspector.rs#L317-L387 : log and `Allow`.
- Edge cases and failure handling: it logs the full destination string, so a URL with a token in its query lands in the log. It cannot block anything.
- Round 1: `new (not documented)`.
- For rizzy-harness: an egress log is useful telemetry, but log hosts, not full URLs.
- Confidence: traced end to end.

#### Security scans log the full tool-call JSON
- What the code does: every scanned shell call is logged with `tool.call_json` (the serialised call, arguments included) at warn (finding) or info (passed).
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/mod.rs#L158-L203 : both log lines carry `tool.call_json`.
- Edge cases and failure handling: a command containing an inline secret is written to the log.
- Round 1: `new (not documented)`.
- For rizzy-harness: our invariant "no secret in a log line" forbids logging arguments; log ids and hashes.
- Confidence: read the definition.

### Sandboxing

#### There is no OS sandbox in the code; the macOS Seatbelt sandbox was removed
- What the code does: a search of the whole clone for `GOOSE_SANDBOX`, `sandbox-exec` and `seatbelt` finds only the v1.25.0 release blog post, which now carries an "Outdated" banner stating that the experimental macOS seatbelt sandbox "has been removed" and that the goose server process runs with the user's permissions, not sandboxed at the OS level. The developer shell spawns `bash -c` (or `GOOSE_SHELL`) directly; no wrapper, namespace, Landlock, seccomp or Seatbelt profile exists for any platform. The only "sandbox" in shell.rs is Flatpak detection, which wraps commands in `flatpak-spawn --host` to escape the Flatpak sandbox and run on the host.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/documentation/blog/2026-02-23-goose-v1-25-0/index.md#L19-L34 : the outdated banner and the description of what the removed sandbox did (opt-in `GOOSE_SANDBOX=true`, Desktop on macOS, filesystem limits, URL visibility).
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L575-L590 : the Unix command: `<shell> -c <line>`, or the Flatpak path.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L26-L53 : Flatpak: `flatpak-spawn --host --watch-bus` with the host login PATH.
- Edge cases and failure handling: n/a. Code Mode's JS context has no filesystem, shell or network of its own (code_execution.rs L24), but it can call every tool (previous section). The Codex CLI provider passes `--sandbox read-only` to Codex in some modes; that is Codex's sandbox, not goose's (`providers/codex.rs` L92-L105, read by grep).
- Round 1: `corrected`. Round 1 (after review) reported an unverified opt-in macOS Desktop Seatbelt sandbox with an egress proxy. The code at the pinned commit has none; the repository's own blog says it was removed. The history (which commit removed it) is not in this shallow clone.
- For rizzy-harness: goose is not a reference for sandboxing; a node-side Landlock layer already puts us ahead.
- Confidence: traced end to end (absence established by repository-wide grep) (checked in review)

#### `--container` runs MCP extensions in an existing Docker container, but not the Developer shell
- What the code does: `--container <id>` makes stdio extensions start as `docker exec -i [-e K=V ...] <id> <cmd> <args>` and goose-mcp built-ins as `docker exec ... goose mcp <name>`. Platform extensions, which include Developer (`shell`, `write`, `edit`), are matched first and built in-process, so they ignore the container and run on the host. goose does not create the container, choose the image or set mounts; the user supplies a running container id.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/mod.rs#L932-L948 : the platform arm wins before the `Builtin`/`Platform` container arms.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/mod.rs#L174-L184 : Developer is a platform extension.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/stdio.rs#L40-L53 : `docker exec -i`, env passed as `-e KEY=VALUE` arguments.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/builtin.rs#L19-L33 : built-ins via `goose mcp` inside the container.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-cli/src/cli.rs#L143-L148 : the flag's help text ("stdio and built-in").
- Edge cases and failure handling: extension env values (often API keys) appear on the `docker exec` command line, visible in the host process list. A grep finds no `container` reference under `developer/`.
- Round 1: `corrected`. Round 1 mentioned "Optional `--container` runs extensions in Docker"; the code shows it does not cover the built-in shell and file tools, which is what a user would expect it to isolate.
- For rizzy-harness: if we add a container back end, the shell tool is the first thing it must cover; pass secrets by env file or stdin, not argv.
- Confidence: read the definition (checked in review)

### How the Developer extension runs shell commands

#### Shell selection, environment and working directory
- What the code does: Unix uses `GOOSE_SHELL` if set, else `bash` if on PATH, else `sh`, invoked `<shell> -c <line>`; Windows uses `GOOSE_SHELL` or `cmd` (`/C` raw arg; PowerShell gets `-NoProfile -NonInteractive -Command`). The child inherits goose's full environment; the only change is setting (or removing) `AGENT_SESSION_ID`. stdin is `/dev/null`. The working directory is the session's leased working dir. `GOOSE_TERMINAL=1` and `AGENT=goose` are not set here; they are set only by the recipe-retry check runner.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L149-L159 : Unix shell choice.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L547-L596 : `build_shell_command` per platform.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L616-L622 : the only environment change.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/retry.rs#L51-L64 : `GOOSE_TERMINAL`/`AGENT` set for recipe retry checks only.
- Edge cases and failure handling: with no environment scrubbing, every provider key that goose read from the environment (env takes precedence over keyring, see Secrets) is visible to every command the model runs. On Windows `cmd`, multi-line commands are refused because cmd truncates at the first newline (L259-L267).
- Round 1: `corrected`. Round 1 said `GOOSE_TERMINAL=1` and `AGENT=goose` are set when goose runs commands; in code that is only true for recipe retry checks, not the Developer shell. Shell defaults confirmed.
- For rizzy-harness: scrub the child environment by default and pass an allowlist; goose shows the failure mode.
- Confidence: traced end to end (checked in review)

#### Timeouts: default 300 s, set per call by the model, and `timeout_secs: 0` disables it
- What the code does: `ShellParams` has an optional `timeout_secs` that the model fills. `resolve_shell_timeout` uses it, else `GOOSE_DEFAULT_EXTENSION_TIMEOUT`, else 300. The deadline is built only when the value is greater than 0. On timeout or cancel the process group is killed and the result says "Command timed out after N seconds".
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L182-L189 : model-settable `timeout_secs`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L408-L414 : resolution order.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L424-L426 : `.filter(|secs| *secs > 0)`: 0 means no deadline.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/extensions.rs#L10 : `DEFAULT_EXTENSION_TIMEOUT = 300`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L470-L485 : wait versus deadline versus cancel; both kill.
- Edge cases and failure handling: there is no upper bound on the model's value; with 0 a hung command runs until the user cancels. After exit, output drain gets 500 ms; if a backgrounded child still holds the pipes, output is marked truncated (L487-L511).
- Round 1: `new (not documented)`.
- For rizzy-harness: clamp model-supplied timeouts to a node-configured maximum; never let 0 mean infinite.
- Confidence: traced end to end (checked in review)

#### Output caps: 2,000 lines or 50,000 bytes, tail preview, full output to a temp file; collection itself is unbounded
- What the code does: stdout and stderr are read line by line into an unbounded channel and a `Vec` in memory, then split into stdout, stderr and interleaved text. Each that exceeds 2,000 lines or 50,000 bytes is written to one of 8 rotating slot files in a per-process temp dir and replaced by its last 50 lines, cut to the last 10,000 bytes, plus a notice telling the model the file path and to read it with `head`/`tail`/`sed`.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L161-L166 : the limits and 8 slots.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L663-L714 : `collect_tagged_lines` into an unbounded channel.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L747-L802 : truncation and the byte-capped tail preview.
- Edge cases and failure handling: a command that prints gigabytes is held fully in memory before truncation (no streaming cap). Slots rotate every 8 calls, so a saved file can be overwritten before the model reads it. The preview byte cap exists because progress bars without newlines "smuggle" unbounded lines (comment L789-L791).
- Round 1: `confirmed` that the tool description promises a 2,000-line limit; the byte cap and file spill are new.
- For rizzy-harness: cap while reading (ring buffer plus spill file), not after; our idle-RSS budget makes this mandatory.
- Confidence: traced end to end.

#### Process cleanup: own process group (Unix) or Job Object (Windows), killed on timeout, cancel or drop; left alone after a normal exit
- What the code does: Unix: `process_group(0)`, and termination is `kill(-pgid, SIGKILL)`. Windows: the shell is created suspended, assigned to a fresh Job Object with KILL_ON_JOB_CLOSE, then resumed, so grandchildren cannot escape before assignment. `Drop` kills the tree while armed. After the shell exits normally the tree is disarmed, so deliberately backgrounded work survives.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell_process.rs#L11-L49 : spawn, wait (disarm), kill, `Drop`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell_process.rs#L51-L92 : Unix process group and SIGKILL.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell_process.rs#L127-L151 : Windows suspended spawn, job assignment, resume.
- Edge cases and failure handling: a child that calls `setsid` leaves the group and survives a kill. The shell does not use `configure_subprocess`, so it gets no `PR_SET_PDEATHSIG`; if goose crashes, the command keeps running. Long-lived MCP servers do get `PR_SET_PDEATHSIG(SIGTERM)` on Linux (spawned from a dedicated thread so the death signal is not tied to a Tokio worker).
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/subprocess.rs#L11-L28 : PDEATHSIG for MCP subprocesses.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/subprocess.rs#L83-L113 : the dedicated spawner thread.
- Round 1: `new (not documented)`.
- For rizzy-harness: the Windows "create suspended, assign job, resume" sequence and the dedicated-thread PDEATHSIG trick are worth copying if we ever target those cases; on Linux, a cgroup or PID namespace would also catch `setsid` escapes.
- Confidence: traced end to end.

#### `write`/`edit` accept any absolute path
- What the code does: `resolve_path` returns absolute paths unchanged and joins relative ones onto the working dir. There is no containment check, ignore file or protected-path list in the Developer extension.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/edit.rs#L170-L181 : `resolve_path`.
- Edge cases and failure handling: the model can write `~/.config/goose/permission.yaml` or `adversary.md` with `write` (both subject only to the mode's approval). A grep finds no `.gooseignore` handling in `crates`.
- Round 1: `new (not documented)`.
- For rizzy-harness: the node must protect its own policy files from the agent's tools.
- Confidence: read the definition.

### Hooks in the permission path and trust

#### PreToolUse hooks run after approval; failures allow unless `on_failure: block`, which also covers timeouts
- What the code does: hooks are `sh -c` commands from `<plugin>/hooks/hooks.json`, regex-matched, given the JSON context on stdin with `PLUGIN_ROOT` set, 30 s default timeout per action. Verdicts: exit 2 or stdout `{"decision":"block"}` is a policy deny; exit 0 with empty stdout or `{"decision":"allow"}` is allow; everything else (other exit codes, a signal, invalid UTF-8, a timeout, a spawn error, failure to deliver stdin) is a hook failure. A failure allows unless the action is a PreToolUse with `on_failure: "block"`; the option is ignored on other events. The first deny ends the chain.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/hooks/mod.rs#L42-L43 : default timeout 30 s.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/hooks/mod.rs#L897-L947 : `classify_output`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/hooks/mod.rs#L853-L871 : `apply_verdict`: block-on-failure only for PreToolUse.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/hooks/mod.rs#L1031-L1046 : the timeout becomes an error, which `emit_blocking` turns into a `HookFailure` (L725-L740).
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/hooks/mod.rs#L995-L1013 : `on_failure` parsed only for PreToolUse; invalid values reject the file.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/pre_tool_call_hook.rs#L55-L92 : the operation runs only on `Executable` (already approved) calls.
- Edge cases and failure handling: the timeout kills only the `sh` child (`kill_on_drop`), not its process group (L1055-L1059, L1093-L1098). An invalid matcher regex skips the rule with a warning, which silently drops a policy hook (L979-L991). Hooks inherit goose's full environment.
- Round 1: `confirmed` (fail-allow default, 30 s), and answers the Round 2 question: `on_failure: block` does cover timeouts.
- For rizzy-harness: the verdict taxonomy (policy deny versus hook failure) and "failure blocks only when asked" are a clean model; for node-side policy hooks we should default to block.
- Confidence: traced end to end (checked in review)

#### Project plugins (and their hooks) under `.agents/plugins/` auto-enable without a trust prompt
- What the code does: the hook manager is loaded once per process from the process's current directory. Plugin discovery lists `<project>/.agents/plugins/*` and the user plugin dir; a newly seen plugin is added to `config.yaml`'s `plugins` map with `enabled: true` and kept, unless a settings file lists it as disabled. Project plugins sort before user plugins and win on a name clash. Their `hooks.json` is then loaded and run (for example on `SessionStart`).
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/services.rs#L382-L384 : `HookManager::load(std::env::current_dir())`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/plugins/discovery.rs#L56-L102 : project and user discovery, sorting, name de-duplication.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/plugins/discovery.rs#L123-L152 : unknown plugins are recorded and enabled.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/plugins/discovery.rs#L154-L183 : default `true`; project dir is `.agents/plugins`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/hooks/mod.rs#L422-L450 : every enabled plugin's `hooks/hooks.json` is loaded.
- Edge cases and failure handling: running goose inside a cloned repository that ships `.agents/plugins/x/hooks/hooks.json` runs that repository's shell commands on goose's events with the user's environment, without asking. I did not trace whether other plugin parts (skills, MCP servers in `plugins/mcp_servers.rs`) are gated differently.
- Round 1: `new (not documented)`. Round 1 described hooks and plugins but no trust gate; there is none for project plugins.
- For rizzy-harness: repository-shipped hooks must be trusted explicitly (by hash, on the node) before they run; goose is the counter-example.
- Confidence: traced end to end for discovery and loading; the event that first fires was not traced (checked in review)

### Extensions: allowlist, deeplinks and malware check

#### The extension allowlist is a Desktop-only prefix match on deeplink installs
- What the code does: Electron's main process fetches the YAML at `GOOSE_ALLOWLIST` (HTTPS only, at most 10 redirects, 30 s timeout) and returns the `command` strings. The install modal for a `goose://extension` deeplink builds `cmd + " " + args` (or the remote URL) and marks it trusted if it `startsWith` any allowed entry. No allowlist exists in the Rust crates, so the CLI, `goose serve`, config-file edits and the agent's own `manage_extensions` tool are not covered.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/main.ts#L3086-L3093 : `getAllowList`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/allowlistFetcher.ts#L6-L29 : the YAML shape (`extensions[].command`).
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/ExtensionInstallModal.tsx#L137-L150 : the string compared.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/ExtensionInstallModal.tsx#L177-L204 : `command.startsWith(allowedCmd)`; `GOOSE_ALLOWLIST_WARNING` turns blocking into a warning; a fetch error blocks ("policy-error").
- Edge cases and failure handling: prefix matching means an entry `npx -y @org/server` also admits `npx -y @org/server-evil`. The policy is fetched again on each check (no caching seen). A case-insensitive grep for "allowlist" under `crates/` finds no extension-install allowlist; its hits are the roaming crate's peer list, the messaging gateway's user allowlist (`gateway/handler.rs` L131), a per-check tool allowlist for `goose review` (`checks/mod.rs` L53), recipe extension tool allowlists (`goose-sdk-types/src/custom_requests/recipe.rs`), an OTel attribute allowlist and a comment on ACP origin allowlists.
- Round 1: `corrected`. Round 1 said the command must "match exactly"; the code matches a prefix, and only in the Desktop deeplink flow.
- For rizzy-harness: enforce extension policy where the process is spawned (the node), with exact argv matching.
- Confidence: traced end to end (corrected in review: the sentence about a grep for "allowlist" was wrong, other unrelated allowlists exist in `crates/`; the finding that no extension allowlist exists in Rust was confirmed).

#### Deeplink installs: base command allowlist is `cu, docker, jbang, npx, uvx, goose, npx.cmd`
- What the code does: a stdio deeplink must use one of seven commands; `npx` with `-c` is refused; env vars from the link are created with empty values (the user fills them).
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/settings/extensions/deeplink.ts#L15-L59 : the list, the `npx -c` refusal, env keys only.
- Edge cases and failure handling: `docker` is accepted with arbitrary args (`docker run -v /:/host ...`), so the list restricts little.
- Round 1: `corrected`. Round 1 listed `jbang, npx, uvx, goosed, docker`; the code has `cu`, `goose` and `npx.cmd`, not `goosed`.
- For rizzy-harness: a base-command list is not a policy when the commands themselves run arbitrary code.
- Confidence: read the definition (checked in review)

#### Malware check: OSV `MAL-*` advisories for `npx`/`uvx` packages, fail open
- What the code does: before spawning a stdio extension, `deny_if_malicious_cmd_args` maps `npx` to npm and `uvx` to PyPI (by executable basename), parses the first package token (with `uvx --from` handling and version parsing), and queries `https://api.osv.dev/v1/query` (override `OSV_ENDPOINT`), following pagination. Any advisory id starting `MAL-` blocks the start with the advisory ids and summaries. Every other command is skipped.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/stdio.rs#L14-L21 : called first in `connect`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_malware_check.rs#L44-L72 : ecosystem detection; unknown commands skipped ("fail open").
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_malware_check.rs#L397-L478 : query loop; request, HTTP or JSON errors return `Ok(())` ("failing open").
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_malware_check.rs#L480-L491 : 10 s timeout.
- Edge cases and failure handling: offline means no check. `docker`, `jbang`, binaries, remote HTTP extensions and project plugins' MCP servers are never checked. It runs on every extension start, so each start makes a network call to osv.dev that reveals the package name.
- Round 1: `corrected` (mechanism was "not documented"): data source is OSV `MAL-*`, offline behaviour is fail open. Answers the Round 2 question.
- For rizzy-harness: a cheap supply-chain check worth having as an advisory signal; it is not a control.
- Confidence: traced end to end (checked in review)

### Secrets

#### Secrets: env first, then one JSON blob in the OS keyring, silent fallback to a 0600 `secrets.yaml`
- What the code does: `get_secret` returns the upper-cased environment variable if set, else the value from storage. Keyring storage keeps all secrets as one JSON object under service `goose`, user `secrets`. If a keyring error looks like an availability error (contains "keyring", "dbus", "org.freedesktop.secrets", ...), goose sets `GOOSE_DISABLE_KEYRING=1` in its own process environment and writes or reads plaintext `~/.config/goose/secrets.yaml`, written through a 0600 temporary file and atomic rename. `GOOSE_DISABLE_KEYRING` (env or config) or a build without the `system-keyring` feature uses the file directly.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L942-L956 : env before storage.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L31-L34 : keyring service and user name.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L999-L1020 : load: keyring, fall back to file when missing or unavailable.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L1262-L1281 : availability error sets `GOOSE_DISABLE_KEYRING` and writes the file.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/providers/private_file.rs#L240-L257 : 0600 temp file, sync, persist.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L219-L223 : startup choice of storage.
- Edge cases and failure handling: the fallback is silent apart from a warn log, and the `GOOSE_DISABLE_KEYRING=1` it sets is inherited by every child process. Secrets in memory are plain `String`/`serde_json::Value` in a cache (no zeroising wrapper seen). Child processes (shell, hooks, MCP servers) inherit goose's environment, so env-supplied keys reach them. I found no redaction of secrets in tool output.
- Round 1: `confirmed` (keyring preferred, plaintext fallback, env precedence), with the single-blob layout and the process-wide flag added.
- For rizzy-harness: keep keys in `Secret<...>` and never in the environment of the agent's children; a silent plaintext fallback should be a loud, logged choice.
- Confidence: traced end to end (checked in review)

#### Other small hardening worth noting
- What the code does: git is run with `-c safe.bareRepository=explicit -c core.fsmonitor=false` so a repository cannot trigger an fsmonitor hook or an implicit bare repo. The `goose serve` secret is compared with `subtle::ConstantTimeEq` and accepted from the `X-Secret-Key` header or a `token` query parameter.
- Trace:
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/subprocess.rs#L34-L45 : `git_command`.
  - https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/acp/transport/auth.rs#L9-L31 : constant-time comparison; header or query token.
- Edge cases and failure handling: the query-parameter form puts the secret in URLs (and possibly access logs); not traced further (sessions cluster).
- Round 1: `new (not documented)` for git; the serve auth question belongs to the sessions cluster.
- For rizzy-harness: copy the git flags in our checkpoint code.
- Confidence: read the definition (checked in review)

## 4. Round 1 claims checked

| Claim (Round 1) | Verdict | Citation |
|---|---|---|
| Default mode is Autonomous; four modes | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L1316-L1323 |
| Per-tool levels in `permission.yaml` | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/permission.rs#L13-L44 |
| `permissions/tool_permissions.json` holds runtime decisions | corrected (dead code) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_store.rs#L79-L115 |
| Smart Approve criteria not described; classification through the provider | corrected (criteria in tool description; fails closed) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/permission/permission_judge.rs#L41-L89 |
| Subagents disabled in non-autonomous modes | refuted at code level (mode inherited, approvals forwarded) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/summon.rs#L416-L425 |
| Opt-in macOS Seatbelt sandbox via `GOOSE_SANDBOX` | refuted for current code (removed; blog marked outdated) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/documentation/blog/2026-02-23-goose-v1-25-0/index.md#L21-L23 |
| `--container` runs extensions in Docker | corrected (not the built-in shell/file tools) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_manager/mod.rs#L932-L948 |
| Adversary: file-enabled, `shell` default, fails open | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/adversary_inspector.rs#L446-L464 |
| Prompt injection: tool parameters matched against patterns; classifier gets tool call and recent messages; threshold 0.8 | corrected (shell only; user messages only; High patterns below threshold) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/scanner.rs#L132-L144 |
| Prompt injection default off | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/security/mod.rs#L46-L55 |
| Allowlist: install rejected unless command matches exactly | corrected (prefix match, Desktop deeplink only) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/ExtensionInstallModal.tsx#L195-L199 |
| Deeplinks accept `jbang, npx, uvx, goosed, docker` | corrected (`cu, docker, jbang, npx, uvx, goose, npx.cmd`) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/ui/desktop/src/components/settings/extensions/deeplink.ts#L16-L24 |
| Malware check mechanism undocumented | corrected (OSV `MAL-*`, npx/uvx only, fail open) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension_malware_check.rs#L420-L442 |
| Secrets: keyring, plaintext `secrets.yaml` fallback, env precedence | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L942-L956 |
| `GOOSE_SHELL` default bash else sh; cmd on Windows | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/developer/shell.rs#L149-L159 |
| `GOOSE_TERMINAL=1` and `AGENT=goose` set when goose runs commands | corrected (recipe retry checks only) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/retry.rs#L51-L64 |
| Hooks: fail-allow default, `on_failure: block` for PreToolUse, 30 s timeout | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/hooks/mod.rs#L853-L871 |
| `GOOSE_MAX_TURNS` default 1000 | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/config/base.rs#L1358-L1360 |
| `--max-tool-repetitions` exists (unexplained) | corrected (no effect) | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_monitor.rs#L109-L131 |
| Cancellation drops the step and finalizes interrupted tool responses | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-agent/src/machine.rs#L250-L285 |
| ACP providers: `auto` skips Claude's checks, `approve` maps to read-only for Codex | confirmed | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/providers/codex_acp.rs#L74-L77 |
| Code Mode JS runtime is isolated (no fs/shell/net) | confirmed, but it calls every tool without per-call approval | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L172-L194 |

## 5. In the code but not in the documentation

- Approvals expire when the extension lease is gone (restart), and only advertised tools can run (ops_toolcalling.rs L705-L780).
- A failing inspector is skipped, not treated as deny (tool_inspection.rs L95-L116).
- SmartApprove auto-allows any tool an MCP server annotates `readOnlyHint: true` (permission_inspector.rs L172-L176).
- Code Mode inner tool calls bypass approval, inspectors and hooks (code_execution.rs L172-L233).
- The Egress inspector (log-only, always on) and full tool-call JSON in security logs (egress_inspector.rs; security/mod.rs L158-L203).
- The model may set `timeout_secs: 0` to disable the shell timeout (shell.rs L424-L426).
- Shell output is collected unbounded in memory before truncation; saved output files rotate over 8 slots (shell.rs L161-L166, L663-L714).
- Windows Job Object containment with suspended spawn; Linux PDEATHSIG for MCP servers but not for shell commands (shell_process.rs, subprocess.rs).
- Project plugins in `.agents/plugins/` and their hooks are auto-enabled without a prompt (discovery.rs L123-L183).
- `write`/`edit` accept any absolute path (edit.rs L170-L181).
- `docker exec -e KEY=VALUE` exposes extension env on the host command line (stdio.rs L40-L53).
- The keyring fallback sets `GOOSE_DISABLE_KEYRING=1` process-wide (base.rs L1262-L1281).
- git is hardened against fsmonitor and implicit bare repositories (subprocess.rs L34-L45).

## 6. Could not find or could not settle

- **When and why the Seatbelt sandbox was removed.** The shallow clone has one commit; the blog says "experimental and has been removed" without a reason. Settle with `git log -S GOOSE_SANDBOX` on a full clone or the release notes after v1.25.0.
- **Whether Summon refuses delegation outside `auto`.** I read the session-creation and forwarding paths, not all of `summon.rs` (about 2,800 lines). Settle by reading `delegate`'s handler or running it in `approve` mode.
- **Which hook event first runs a project plugin's command, and whether project-plugin MCP servers or skills are gated.** Settle by tracing `plugins/mcp_servers.rs` and `run_turn_start_hooks` to the `SessionStart` emit.
- **Whether any provider can place `goose.executable` in `tool_meta`.** No provider format sets `tool_meta` in `goose-provider-types/src/formats` (grep), but I did not audit `crates/goose/src/providers/*` (ACP providers build tool requests themselves). Settle by grepping every `ToolRequest {` constructor.
- **Environment scrubbing for MCP servers.** I saw `envs` added and `PATH` overridden (stdio.rs L33-L35, L66-L68) but not whether `TokioChildProcess` clears the inherited environment; the `Command` is not `env_clear`ed in the code I read.
- **Egress proxy or credential broker:** none found (searched `security/`, `subprocess.rs`, `providers/api_client` was not read). The removed sandbox reportedly had an egress proxy; no proxy code remains.

## 7. Citation check

`research/round2/goose.safety.md: 154 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit`

## 8. Review

Checked 22 of 35 findings (including all findings tied to the review checklist: sandbox absence, Code Mode, fail-open layers, readOnlyHint, project plugins, shell environment and timeout, sub-agent mode, secrets).

| Finding | Verdict | Reason |
|---|---|---|
| No OS sandbox / Seatbelt removed | absence confirmed | repo-wide grep for sandbox-exec, seatbelt, landlock, bwrap, bubblewrap, seccomp, GOOSE_SANDBOX, unshare: only the v1.25.0 blog, a pattern in `security/patterns.rs` L254 and unrelated hits; Codex `--sandbox` flag and Flatpak only |
| Code Mode inner calls skip approval | supported | `dispatch_tool` calls `lease.call` directly; `emit_extended_pre_hooks` is only in `ops_toolcalling.rs` `dispatch_tool_call`, so pre hooks are skipped too (and post hooks are not applied) |
| Adversary mode fails open | supported | `adversary_inspector.rs` returns Allow for no provider, errors and unparseable replies |
| Injection detection (shell only, 0.8 threshold, High patterns 0.75) | supported | `scanner.rs` shell-only gate, `patterns.rs` table; 36 patterns counted |
| Classifier (user messages, fail open) | supported | `effective_role` makes tool-response messages `Tool`; errors become `None` |
| Malware check fails open | supported | OSV request errors return `Ok(())` |
| SmartApprove trusts `readOnlyHint` | supported | `permission_inspector.rs` L52-L64, L172-L176; false hints cached ask_before |
| Project plugins auto-enable | supported | `filter_by_config` inserts `enabled: true`; project dir sorted first |
| Shell inherits whole environment | supported | no `env_clear` anywhere in `crates`; only `AGENT_SESSION_ID` touched |
| `timeout_secs: 0` removes limit | supported | `.filter(|secs| *secs > 0)` |
| Sub-agents inherit mode, approvals forwarded | supported | `summon.rs` L416-L425, `forwarded` in `subagent_handler.rs`; no mode refusal found in `summon.rs` (grep); round-1 "refuted" is a code-level absence, still not run |
| Hooks fail allow unless `on_failure: block` | supported | `apply_verdict`, `classify_output`, timeout bails into failure |
| Four modes / default auto | supported | ladder read |
| tool_permissions.json dead code | supported | only a re-export references it |
| `--max-tool-repetitions` inert | supported | `inspect` mutates a temp clone; registered with `None` |
| Approvals expire with lease | supported | EXPIRED_APPROVAL_RESPONSE path read |
| Inspector result combination | supported | `tool_inspection.rs` errors logged and skipped; Deny/RequireApproval override |
| ACP mode mapping | supported | tables read |
| `--container` skips Developer | supported | no `container` reference under `developer/` |
| Extension allowlist | partly | claim holds, but the "grep finds only the roaming crate" sentence was wrong; corrected |
| Deeplink command list | supported | `deeplink.ts` L15-L24 |
| Secrets/keyring fallback | supported | `base.rs` env first; fallback sets `GOOSE_DISABLE_KEYRING` |
| serve auth constant-time header or query | supported | `auth.rs` |

Overall: the checked claims are accurate and carefully hedged; the one error was a descriptive grep sentence. The unchecked remainder (cancellation, shell output caps, process cleanup, egress inspector, logging, `write`/`edit` paths, per-tool permission file mechanics) are mostly definitional reads of nearby code and can be trusted at the same level, but numbers such as the 8 slots and 50 KB cap were only skimmed. Note for the reader: the Round 1 "refuted" verdict on subagents rests on absence of a guard in `summon.rs`, which I confirmed by grep but nobody ran.
