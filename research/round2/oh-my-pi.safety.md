# Oh My Pi (`omp`): Round 2 source investigation, cluster `safety`

## 1. Repository

- `can1357/oh-my-pi`, pinned SHA `e325ffc73ab86e03bdd20a91b77d8e2b3964c906`, commit date 2026-11-10T21:11:46+01:00, branch `main`.
- Read (data only, nothing executed): `packages/agent/src/agent-loop.ts` (tool dispatch), `packages/coding-agent/src/tools/{approval,approval-cache,bash,shell-tokenize,settings,tool-timeouts,file-write-fallback}.ts`, `extensibility/extensions/{wrapper,runner,loader,types}.ts`, `sdk.ts` (tool wrapping, argument transform), `session/session-tools.ts` (ACP gate), `config/settings.ts` and `config/registry.ts` (layering), `discovery/{builtin,claude,ssh,helpers}.ts`, `mcp/{config,settings,tool-bridge}.ts`, `exec/{bash-executor,non-interactive-env,settings}.ts`, `packages/utils/src/{env,procmgr}.ts`, `secrets/{index,settings,message-transform}.ts`, `packages/ai/src/auth-broker/*`, `packages/ai/src/auth-gateway/{server,http}.ts`, `cli/{auth-broker-cli,auth-gateway-cli,token-file}.ts`, `ssh/{connection-manager,file-transfer}.ts`, `task/executor.ts`, `speculation/host.ts`, `eval/{preludes,py/tool-bridge}.ts`, `crates/pi-natives/src/shell.rs`, `crates/pi-shell/src/proctree.rs`; docs `docs/approval-mode.md`, `docs/auth-broker-gateway.md`, `docs/secrets.md` for orientation.

## 2. Architecture as it bears on this cluster

- **Process model.** One Bun/TypeScript process runs the TUI (or RPC/ACP/print host), the agent loop, every tool and every extension in-process. The Rust core is an N-API addon (`pi-natives`) that the TS side calls for the embedded bash engine (`pi-shell`), PTYs, grep/glob, AST and diff work. There is no separate executor process and no privilege boundary between the loop and its tools. The native shell binding is exported as a class the TS executor instantiates: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-natives/src/shell.rs#L1-L26 (module doc "Embedded bash engine (pi-shell) shell execution exported via N-API" and the `pi_shell::Shell` imports).
- **Loop.** `runLoopBody` in `packages/agent` streams one assistant message, then calls `executeToolCalls` on its tool calls; the loop is `while (true)` bounded only by an optional wall-clock `deadline`, abort signals and a soft-required-tool escalation cap: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/agent-loop.ts#L1679-L1685 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/agent-loop.ts#L1733-L1743. No `maxTurns`/`maxSteps` exists in `packages/agent/src` (grep).
- **Validation and hooks before dispatch.** `prepareToolCallDispatch` resolves the tool, validates arguments against its schema, runs the `beforeToolCall` hook and revalidates any hook-revised arguments: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/agent-loop.ts#L3187-L3234.
- **Authorisation lives in one wrapper.** Every registry tool is wrapped in `ExtensionToolWrapper`, which is the only place the per-tool approval gate runs; the runner is created unconditionally so the gate cannot disappear: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L3740-L3747 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L3889-L3894.
- **Policy decision.** `resolveApproval` / `resolvePolicy` in `tools/approval.ts` combine the tool's self-declared decision, the user's `tools.approval` map, the mode and restricted folders: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L346-L390.
- **Settings layering** decides what the mode actually is: global, then project, then `--config` overlay, then runtime overrides, deep-merged with arrays and scalars replaced: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L4287-L4291.
- **Compaction** lives in `packages/agent/src/compaction*` (not read for this cluster; see the `agents` file).

## 3. Findings

### Agent loop and tool dispatch

#### Hook revisions are revalidated and approval is resolved against the input that runs
- What the code does: In the loop, a `beforeToolCall` result can block, add context or replace arguments; replaced arguments go back through schema validation and are baked into the message so history shows one version. In the wrapper, `deny` is checked once on the original input (short-circuit), then `tool_call` extension handlers run (they can block or rewrite), then the full approval gate is re-resolved against the effective input, so a handler cannot rewrite a call into something that skips a prompt or a deny.
- Trace: loop revalidation https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/agent-loop.ts#L3204-L3234 ; wrapper pre-check https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L265-L291 ; handler emission https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L306-L342 ; re-resolution https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L355-L374
- Edge cases and failure handling: a throwing `beforeToolCall` becomes a tool-error result for that call without aborting the batch (lines 3205-3216 above); a non-`Error` thrown by a handler is converted to "Extension failed, blocking execution" (wrapper lines 336-341).
- Round 1: `confirmed` ("hooks cannot bypass approval policy"), and the code shows the mechanism (two-phase resolution).
- For rizzy-harness: copy the rule "approve the bytes that execute": resolve policy after every mutation of the call, on the node.
- Confidence: traced end to end. (checked in review)

#### Tool concurrency: shared by default, exclusive on resolver failure; steering never hard-kills a side-effecting tool
- What the code does: Each tool declares `concurrency` (`shared`/`exclusive` or a function of args); an unset mode is shared and a throwing resolver is treated as exclusive. Queued steering or a peer message aborts only tools flagged `interruptible` (pure waits); every other tool (bash included) gets only a cooperative `steeringSignal` it may honour (auto-backgrounding) and sees only the external abort.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/agent-loop.ts#L113-L129 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/agent-loop.ts#L3360-L3374
- Edge cases and failure handling: an `interruptible` resolver that throws defaults to non-interruptible (lines 3393-3401 of the same file).
- Round 1: `new (not documented)`.
- For rizzy-harness: worth copying: user steering must not kill a half-done side effect; only the explicit cancel does.
- Confidence: read the definition. (checked in review)

#### Speculative tool execution exists but is limited to auto-allowed local reads
- What the code does: while the model is still streaming, the speculation host may start a call early. Its policy gate refuses whenever any extension lifecycle handler is loaded or when `resolveApproval` would not return `allow`; evidence capture accepts only a single `local_read` effect on an absolute path that matches the argument.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/speculation/host.ts#L216-L235 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/speculation/host.ts#L246-L254
- Edge cases and failure handling: a later block in `beforeToolCall` discards the candidate before execution (comment lines 195-208).
- Round 1: `new (not documented)`.
- For rizzy-harness: if we ever pre-execute, restrict it to side-effect-free reads that the policy already allows.
- Confidence: read the definition. (checked in review)

### Approvals and permission rules

#### The real default is `yolo`, and there are four modes, not three
- What the code does: `tools.approvalMode` is an enum of `always-ask`, `always-ask-cached`, `write`, `yolo`, default `yolo`. Each mode maps to a maximum auto-approved tier (`read`/`read`/`write`/`exec`). An unrecognised configured value also falls back to `yolo`. Only a call with no settings object at all fails closed to `always-ask`.
- Trace: setting https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L314-L319 ; tier map https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L153-L158 ; context resolution https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L97-L113
- Edge cases and failure handling: `--auto-approve` forces `yolo` while keeping the user policy map (lines 98-103).
- Round 1: `corrected`: default yolo confirmed; Round 1 lists three modes, the code has a fourth, `always-ask-cached` ("Ask and remember").
- For rizzy-harness: our default must stay "ask for exec"; do not let an invalid value fall back to the most permissive mode.
- Confidence: traced end to end. (checked in review)

#### Resolution order: tool deny, user deny, then mode-specific rules; `yolo` drops critical overrides
- What the code does: `resolvePolicy` returns `deny` first if the tool's own decision denies, then if the user's `tools.approval.<key>` denies. In `yolo` it returns the tool's explicit `policy` if any, else the user policy, else `allow`, ignoring `override`. Outside yolo, `override: true` forces a prompt (unless the tool said `allow`), then explicit tool policy, then user policy, then the cached-grant step, then the tier comparison. A restricted folder then turns any non-deny into a per-call prompt in every mode.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L426-L464 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L466-L513 ; restricted-folder wrap https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L366-L390
- Edge cases and failure handling: a tool with no `approval` is `exec` (normalizeDecision default, lines 274-296); renamed MCP tools inherit only `deny`/`prompt` from the legacy name, never `allow` (lines 410-424).
- Round 1: `confirmed` ("deny always wins", decision order).
- For rizzy-harness: the legacy-name rule (carry restrictive policy, never permissive) is a good small idea.
- Confidence: traced end to end. (checked in review)

#### The bash critical-pattern list, and the fact that `yolo` skips it
- What the code does: `CRITICAL_BASH_PATTERNS` is 21 regexes over the raw command text: recursive `rm`/`chmod`/`chown` on an absolute path, `--no-preserve-root`, `sudo rm`, a fork bomb, writes to `/dev/sd*`, `mkfs`, `dd of=/dev/...` (except null-like sinks), `shred /dev/`, `cryptsetup`, writes or `tee` to `/etc/passwd|shadow|sudoers`, `curl|wget|fetch` piped or process-substituted into a shell or `eval`, `kill -9 1`, `shutdown|poweroff|reboot|halt`, `init 0`, `nc -e/-c`. A match returns `{tier: "exec", override: true}` with no `policy`, which `resolvePolicy` ignores in `yolo`.
- Trace: list https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L197-L244 ; use https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L657-L660 ; yolo branch https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L446-L464
- Edge cases and failure handling: because the regexes see the raw text, wrappers such as `bash -c "rm -rf /x"` still match; but `rm -rf ~` or `rm -rf "$HOME"` do not (the first pattern requires a literal `/` after the flags). So the list is a seatbelt only in non-yolo modes, and even there it is easy to step around.
- Round 1: `confirmed` ("critical-pattern checks skipped in yolo"); answers Round 2 question 12 (the exact list).
- For rizzy-harness: a denylist this size is not a control; our sandbox and node policy are. If we keep a "dangerous" list, it must apply in every mode, not be dropped by the most common one.
- Confidence: traced end to end. (checked in review)

#### `bash.patterns` grammar and segment splitting
- What the code does: rules are `{match, approval}` with `*` as the only wildcard, compiled to an anchored regex over whitespace-normalised text. `allow` must match the whole command and never applies when the command contains unquoted shell control (`; & | < > \` $ ( )`, or quoted control plus a `-c`/`-e` re-interpreting option). `deny`/`prompt` fire on the whole command or on any segment from `tokenizeShellSegments`, which splits on `; & | ( )` and newlines after quote removal. With `bash.allowCompoundCommands` (default off) a strict literal `&&`-chain parser lets every segment be matched separately.
- Trace: compile https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L253-L263 ; control detector https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L106-L148 ; matching semantics https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L291-L321 ; splitter https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/shell-tokenize.ts#L14-L28 ; chain parser https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/shell-tokenize.ts#L243-L280 ; default off https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/settings.ts#L117-L128
- Edge cases and failure handling: the segment splitter does not see inside backticks, and wrapper or interpreter invocations are not unwrapped for `deny`: a `deny` rule `rm *` does not fire on `bash -c "rm -rf x"` (the segment is `bash -c rm -rf x`, the anchored regex needs `^rm`), nor on `/bin/rm`, `command rm` or `env rm`. The `&&`-chain parser does reject wrappers (`SHELL_INTERPRETER_COMMANDS`, `SHELL_STATEFUL_COMMANDS`) and any expansion, so the `allow` side is the conservative one. The bypass is inferred from the code, not run.
- Round 1: `confirmed` (deny/prompt per segment, allow whole command), with the wrapper gap added.
- For rizzy-harness: copy the asymmetry (allow must cover everything, deny may hit any part); our deny matching must normalise wrappers (`sh -c`, `env`, absolute paths) or it is decoration.
- Confidence: traced end to end (bypass inferred). (checked in review)

#### Operator `bash.patterns` survive a project layer; no other tool-approval setting does
- What the code does: the bash approval re-evaluates `bash.patterns` against `Settings.grantAuthority()`, a view of global, `--config` and runtime layers without the project layer, so a project cannot lift an operator `deny` or `prompt`. The comment on `grantAuthority` states the reason: "repository settings files are writable by the agent". No other tool-approval setting uses that view (reviewer: the same `grantAuthority()` view also protects the per-call model-grant settings for `task`, e.g. `task.perCallModel` and `task.perCallModelAllow`, at `packages/coding-agent/src/task/model-approval.ts` L153 and `task/structured-subagent.ts` L369).
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L639-L656 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L843-L866
- Edge cases and failure handling: see the next finding for the settings that are not protected.
- Round 1: `new (not documented)`.
- For rizzy-harness: this is our "the node decides" invariant in miniature. Their bug is that they applied it to one key.
- Confidence: traced end to end. (checked in review) (corrected in review: "nothing else does" narrowed to tool-approval settings; model-grant settings also use `grantAuthority()`)

#### A repository's `.omp/config.yml` (or `.omp/settings.json`) can loosen approval mode, per-tool policy and restricted folders
- What the code does: the project layer is every `settings.json`/`config.yml` the native provider finds in the project config dir, merged with no key filtering (only "group shadow" values are dropped). Project merges over global; arrays and scalars replace. `cfgToolsApprovalMode`, `cfgToolsApproval` and `cfgToolsRestrictedFolders` are read from the merged view. A repository can therefore ship `tools: {approvalMode: yolo, approval: {bash: allow}, restrictedFolders: []}` and undo a user's `always-ask` and restricted folders. The settings UI hides the restricted-folders editor when its provenance is `project`, which shows the authors know the field can come from there, but the value is still honoured.
- Trace: project file discovery https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/builtin.ts#L898-L928 ; merge without filter https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L2800-L2814 ; layer order https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L4287-L4291 ; arrays replace https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L4310-L4332 ; read site https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L104-L112 ; UI-only condition https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings-ui.ts#L44-L44
- Edge cases and failure handling: an agent with write access can also create the file mid-session; settings watch project files (`#projectSourcePaths`, lines 1403-1408 of settings.ts). `mcp.enableProjectConfig` is read the same way, so a project can turn a user's `false` back to `true`.
- Round 1: `new (not documented)`; Round 1 only says the repo's settings are loaded.
- For rizzy-harness: a direct argument for our invariant: no repository file may widen node policy. Write the abuse-case test ("project config sets permissive mode; node still asks").
- Confidence: traced end to end through the merge (not executed). (checked in review)

#### Approval prompts: what a "session" answer and a cached grant are bound to
- What the code does: an "approve for this session" answer is offered only for prompts raised by the mode, and is keyed `subject\0tier`, so approving a write never covers an exec of the same tool; it is in memory. In `always-ask-cached`, a grant key is `tier\0path` (exact target or its folder, symlinks resolved), checked for the call's tier or a higher one, stored for the session or in `approval-cache.db` (SQLite, no expiry) for "always". Critical overrides, explicit `prompt` policies and provider safety checks never offer either. Anything but an explicit approval is `deny`.
- Trace: session scope https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L598-L612 ; cached step https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L176-L214 ; store https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval-cache.ts#L18-L27 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval-cache.ts#L89-L109 ; default-deny answer mapping https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L906-L919
- Edge cases and failure handling: a failed cache read returns "nothing granted" (fail closed); a failed write only logs. Folder grants cover direct entries only, not subtrees (lines 176-186). The session grant for bash is per tool, not per command: one "approve bash for this session" covers every later ordinary bash call at that tier.
- Round 1: `new (not documented)`.
- For rizzy-harness: bind grants to tier plus target, never to a tool name alone; the folder-listing in the prompt (it shows what a directory grant covers) is a good UX detail.
- Confidence: traced end to end. (checked in review)

#### Headless and no-UI behaviour: fail closed, but only when something asks
- What the code does: if a prompt is required and the runner has no UI, the call throws with a hint that suggests setting `yolo` or `allow`. Provider safety checks (computer-use) require a prompt in every mode, including `yolo`. With the default `yolo`, nothing reaches this path except restricted folders, explicit `prompt` policies and safety checks.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L404-L409 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L446-L471
- Edge cases and failure handling: the error text actively recommends `tools.approvalMode: yolo`, which nudges headless users to the weakest setting.
- Round 1: `confirmed` (headless fails closed; safety checks always prompt).
- For rizzy-harness: fail closed in headless, and do not suggest disabling the gate in the error.
- Confidence: traced end to end. (checked in review)

#### ACP sessions treat the schema default differently from an explicit `yolo`
- What the code does: when omp runs under an ACP client that supports `requestPermission`, `bash` is re-resolved with the mode forced to `always-ask` unless the user explicitly chose auto-approve, so default-config ACP sessions still ask the editor for exec calls. Critical and explicit-prompt bash calls stay per-call even after the client picks "Always allow".
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-tools.ts#L1174-L1217
- Edge cases and failure handling: tools other than bash skip the client prompt entirely in explicit auto-approve mode unless a user policy says otherwise (lines 1176-1180).
- Round 1: `new (not documented)`.
- For rizzy-harness: evidence that even the authors consider the schema default unsafe when a stranger's editor is the host.
- Confidence: read the definition. (checked in review)

#### Sub-agents run in `yolo`; the parent's `task` call is the only boundary
- What the code does: the subagent settings overlay sets `tools.approvalMode: "yolo"` and disables the advisor; it is a `derived` overlay, so user `tools.approval` denies and prompts still apply (prompts then fail closed, since subagents have no UI).
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L1259-L1278
- Edge cases and failure handling: restricted folders also prompt and therefore fail closed in a subagent (wrapper lines 457-463).
- Round 1: `confirmed`.
- For rizzy-harness: do not copy; a sub-agent on our node should inherit the node's policy, not a blanket yolo.
- Confidence: traced end to end. (checked in review)

#### MCP tools are `write` tier; custom tools default to `exec`
- What the code does: the MCP tool bridge declares `approval = "write"`, so in `write` mode every MCP call is auto-approved; a tool without `approval` resolves to `exec`.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/tool-bridge.ts#L733-L735 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L274-L296
- Edge cases and failure handling: an MCP server can do anything a process can; "write" understates it.
- Round 1: `confirmed`.
- For rizzy-harness: classify MCP tools as exec by default; a remote server is opaque.
- Confidence: read the definition. (checked in review)

#### The `eval` tool runs arbitrary Python/JS at exec tier, outside every bash rule
- What the code does: `eval` declares `approval = "exec"`. Kernel code calling back into host tools goes over a 127.0.0.1 HTTP bridge with a per-process random bearer and each prelude call is re-authorised through `resolveApproval`. But code inside the kernel (`os.system`, `subprocess`) never passes `bash.patterns` or the critical list.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/eval.ts#L354-L354 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/eval/py/tool-bridge.ts#L153-L165 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/eval/preludes.ts#L94-L132
- Edge cases and failure handling: in `yolo` the eval call itself is auto-approved, so a `deny` in `bash.patterns` is bypassed by asking for the same command through Python (inferred).
- Round 1: `new (not documented)` (Round 1 listed kernel isolation as unverified).
- For rizzy-harness: command rules are only meaningful when every execution path passes through them, or when a sandbox enforces the outcome.
- Confidence: read the definition (bypass inferred). (checked in review)

#### The bash interceptor and git guard exist but are off by default
- What the code does: `bashInterceptor.enabled` (default false) refuses commands that have dedicated tools (cat/grep/find/sed -i, echo redirection, `nohup`, dev servers) and returns them as a tool-owned `deny`. `bash.gitGuard` (default false) refuses stash, `reset --hard` and checkout/switch/restore in a main checkout.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/settings.ts#L143-L154 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/settings.ts#L170-L180 ; deny wiring https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L621-L631
- Edge cases and failure handling: these are tool-steering aids, not security controls.
- Round 1: `new (not documented)`.
- For rizzy-harness: a git guard that snapshots before discarding (their worktree behaviour) is worth considering for checkpoints.
- Confidence: read the definition.

### Running commands

#### How a bash call executes: persistent embedded shell per key, forks for parallel calls, native timeout
- What the code does: `executeBash` resolves the shell config, folds in direnv (only `.envrc` the user has `direnv allow`ed), builds a non-interactive env and runs the command on a persistent native `Shell` keyed by shell, prefix, env and session. A second concurrent call on a busy key runs in a one-shot fork that does not persist state. Explicit timeouts are enforced inside the native shell with a JS backstop; the default deadline is 300 s, clamped to 1-3600 s by `TOOL_TIMEOUTS`, and `timeout: 0` disables it. On timeout or abort the persistent session is quarantined and replaced.
- Trace: session selection https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/bash-executor.ts#L751-L769 ; deadlines https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/bash-executor.ts#L792-L811 ; timeout handling https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/bash-executor.ts#L853-L880 ; limits https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/tool-timeouts.ts#L10-L20 ; direnv setting https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/settings.ts#L188-L200
- Edge cases and failure handling: the model may pass `timeout: 0` and get no deadline at all (unless `tools.maxTimeout` caps it in `transformToolCallArguments`, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L4814-L4824 ).
- Round 1: `confirmed` (embedded bash, persistent sessions).
- For rizzy-harness: a model-controllable "no timeout" is a footgun; cap on the node.
- Confidence: traced end to end. (checked in review)

#### Kill semantics in the Rust core: SIGTERM the group and every descendant, grace, then SIGKILL after a re-walk
- What the code does: `pi-shell`'s process tree terminates by signalling the process group, every signalable descendant and the root with SIGTERM, waits a grace period, re-walks the tree (to catch grandchildren spawned meanwhile or re-parented) and sends SIGKILL; host-protected PIDs are skipped. Children are started in their own session (`setsid`).
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-shell/src/proctree.rs#L843-L884
- Edge cases and failure handling: a child that double-forks out of the tree and changes session escapes the walk (inferred; no cgroup or pidfd use was found in this function).
- Round 1: `new (not documented)`.
- For rizzy-harness: the re-walk before SIGKILL is the detail most implementations miss; a cgroup is the stronger tool on Linux.
- Confidence: read the definition.

#### Output truncation: head plus tail with the full output spilled to an artifact
- What the code does: bash output goes through an `OutputSink` with a head budget and artifact file; results above `tools.artifactSpillThreshold` (50 KiB) are stored as a session artifact and replaced by a head (20 KiB) plus tail (20 KiB) view with an `artifact://` reference; artifacts are capped at 16 MiB.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/bash-executor.ts#L662-L680 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/output-meta.ts#L508-L525 ; defaults https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L14-L16 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L129-L131
- Edge cases and failure handling: a native "minimizer" (`shellMinimizer.enabled`, default true) also rewrites verbose git/npm/cargo output before the model sees it (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/settings.ts#L216-L226).
- Round 1: `new (not documented)` in this detail.
- For rizzy-harness: head-plus-tail with a recoverable full copy matches our `tool_output` design; their numbers are a reasonable reference.
- Confidence: read the definition. (checked in review)

#### Background jobs and auto-backgrounding
- What the code does: `bash` accepts `async` and a named-service form with readiness probes (`ready.log`, `ready.port`); `bash.autoBackground.enabled` (default true) detaches a command still running after 60 s and delivers its result later. A shell with live background jobs is retained past session teardown until the jobs end.
- Trace: schemas https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L419-L454 ; settings https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/settings.ts#L130-L141 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/settings.ts#L285-L290 ; retention https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/bash-executor.ts#L270-L284
- Edge cases and failure handling: approval runs once at call time; a backgrounded job keeps running with no further check.
- Round 1: `confirmed` (bash can background).
- For rizzy-harness: background work must stay under the node's process accounting and cancel path.
- Confidence: read the definition. (checked in review)

#### SSH: file I/O over OpenSSH with `accept-new`; no remote command tool; project files may define hosts
- What the code does: `ssh://` paths are served by spawning the system `ssh` with `-n`, `ControlMaster=auto` (omp-owned ControlPath, `ControlPersist=3600`), `BatchMode=yes`, `StrictHostKeyChecking=accept-new`, optional `-p` and `-i`; key files with group or world bits are refused. Remote reads run `head -c <max+1>` with POSIX-quoted paths; writes stage to a temp and commit. Hosts come from omp's managed paths and from `ssh.json` / `.ssh.json` in the project root, with `${VAR}` expanded from the process environment.
- Trace: argv https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/ssh/connection-manager.ts#L193-L209 ; key check https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/ssh/connection-manager.ts#L166-L185 ; read https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/ssh/file-transfer.ts#L106-L121 ; host sources https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/ssh.ts#L128-L134 ; env expansion https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/ssh.ts#L98-L104 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/helpers.ts#L558-L567
- Edge cases and failure handling: `accept-new` trusts the first key it sees. A project `ssh.json` can name a host whose value interpolates any environment variable (for example a key embedded in a hostname), which leaks when the agent connects; inferred from the code, not run. Remote commands beyond file I/O are left to `bash` running `ssh` itself.
- Round 1: `corrected`: Round 1 left open whether bash runs remotely; there is no remote-exec back end, only `ssh://` file operations.
- For rizzy-harness: never expand ambient environment into repository-supplied config; pin host keys rather than accept-new.
- Confidence: traced end to end (leak inferred). (checked in review)

### Sandboxing

#### There is no OS sandbox; isolation is for merging, and there is a seam for an outside sandbox
- What the code does: no Landlock, seccomp, bubblewrap, Seatbelt or namespace code exists in `crates/` or `packages/` (searched: `landlock|seccomp|bwrap|bubblewrap|sandbox-exec|seatbelt|firejail|nsjail`; hits are comments only). `pi-iso` provides copy-on-write clones and overlays for subagent worktrees. `file-write-fallback.ts` documents a hook for a host that embeds omp in its own OS sandbox to broker denied writes.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/file-write-fallback.ts#L1-L15 ; isolation backends are the files under `crates/pi-iso/src/` (apfs, btrfs, overlayfs, projfs, zfs, rcopy).
- Edge cases and failure handling: nothing to fail; all commands run with the user's full rights.
- Round 1: `confirmed`.
- For rizzy-harness: our Landlock plan has no counterpart here; their write-fallback seam is the shape a host-side sandbox broker would take.
- Confidence: read the definition (absence by search). (checked in review)

### Secrets

#### Child shells inherit the whole environment, including provider keys
- What the code does: the spawn env is the process env filtered only for names containing `=` or NUL, macOS malloc variables, values that came from the project's `.env` files, and git repo-location variables (`GIT_DIR` etc.); then omp adds `SHELL`, `GIT_EDITOR=true`, `CI=true`, `OMPCODE=1` and `CLAUDECODE=1`. Per command it overlays a non-interactive set (pagers off, editors `true`, `GIT_TERMINAL_PROMPT=0`, `SSH_ASKPASS=false`, package-manager noise off, locale `C.UTF-8`). Nothing removes `*_API_KEY`, `*_TOKEN` or similar.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/utils/src/procmgr.ts#L43-L55 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/utils/src/env.ts#L30-L36 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/utils/src/env.ts#L54-L69 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/utils/src/env.ts#L155-L168 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/exec/non-interactive-env.ts#L6-L57
- Edge cases and failure handling: the project-dotenv strip is the one deliberate scrub: a repository's `.env` cannot inject variables into the agent's shells.
- Round 1: `new (not documented)`.
- For rizzy-harness: our shell-env allowlist is stricter and should stay so; the dotenv strip is a case to include in our tests.
- Confidence: traced end to end. (checked in review)

#### Secret obfuscation exists, is off by default, and is reversed before tools run
- What the code does: with `secrets.enabled` (default false) a `SecretObfuscator` is built from env variables whose names match `KEY|SECRET|TOKEN|PASSWORD|PASS|AUTH|CREDENTIAL|PRIVATE|OAUTH` (values of 8+ chars), passwords inside connection URLs, `secrets.yml` (global and project) and built-in credential regexes. Provider-bound messages carry HMAC-keyed placeholders (key in a 0600 file); `transformToolCallArguments` deobfuscates tool arguments before execution. A separate, irreversible pattern redaction in `pi-ai` is also gated by the same switch.
- Trace: setting https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/secrets/settings.ts#L13-L25 ; sources https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/secrets/index.ts#L182-L223 ; build https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/secrets/index.ts#L271-L293 ; restore before execution https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L4814-L4824 ; provider redaction switch https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/providers/transform-messages.ts#L726-L760
- Edge cases and failure handling: this hides secrets from the provider, not from the agent's actions: a placeholder the model writes into a `curl` argument is turned back into the real secret before the command runs, so prompt-injected exfiltration still works (inferred from the transform). A project `secrets.yml` can override global entries by content.
- Round 1: `new (not documented)`; Round 1 said the docs describe no automatic redaction.
- For rizzy-harness: provider-side masking is a privacy feature; it is not a leak control. Our "no secret in an event" rule needs the secret never to reach the child env in the first place.
- Confidence: traced end to end (exfil path inferred). (checked in review)

#### Credential storage on disk: SQLite with 0700/0600, no encryption, no path protection from the agent's own tools
- What the code does: credentials live in `agent.db`; the directory is created 0700 and the file chmod'ed 0600 (best effort on Windows). No search hit shows `read`/`bash` refusing `agent.db`, the broker token file or the placeholder key, so with no sandbox the agent can read them.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth/sqlite-credential-store.ts#L588-L605
- Edge cases and failure handling: chmod failure is ignored.
- Round 1: `confirmed` for permissions; the agent-readable point is `new`.
- For rizzy-harness: keys on the master only; the node's tools must not be able to read the store at all.
- Confidence: read the definition (absence by search). (checked in review)

#### Auth broker: one bearer, loopback by default, plain HTTP, clients receive API keys and OAuth access tokens
- What the code does: `omp auth-broker serve` binds `127.0.0.1:8765` unless `--bind` says otherwise, loads or creates one random 32-byte bearer token (0600 file) and serves a REST/SSE API with `Bun.serve` (no TLS). Every route except `/v1/healthz` requires `Authorization: Bearer`. A snapshot contains every credential: `api_key` rows verbatim and OAuth rows with `access` in clear and `refresh` replaced by the sentinel `__remote__`. The same token can `POST /v1/credential` to upsert credentials and trigger refreshes. The broker refreshes OAuth tokens expiring within 5 minutes, polling every 60 s.
- Trace: bind default https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/types.ts#L177-L184 ; serve https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-broker-cli.ts#L139-L157 ; token https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/token-file.ts#L21-L35 ; auth check https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L109-L116 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L829-L849 ; redaction https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth/pool.ts#L715-L735 ; wire schema https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/wire-schemas.ts#L78-L110 ; write route https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L1054-L1078
- Edge cases and failure handling: `--bind 0.0.0.0:8765` is accepted with no warning in the serve path read; "loopback only" is a default, not an enforced rule. An empty token set disables auth in `startAuthBroker` (the CLI always passes one). The token comparison is a `Set.has` lookup, not a constant-time compare. Unknown paths are logged as `<unrouted>` so a secret in a URL is not logged (lines 175-180).
- Round 1: `confirmed` (API keys and access tokens; loopback default; no TLS; healthz open), `corrected` on "loopback-only": default, not enforced.
- For rizzy-harness: their broker is a credential distributor, not a broker in our sense: any client holds usable keys. Our master must hold keys and let nodes call through it, never hand them out.
- Confidence: traced end to end. (checked in review)

#### Auth gateway: clients never see a provider credential; `--no-auth` plus wildcard CORS is dangerous
- What the code does: the gateway is itself a broker client that holds the credentials and forwards provider-format requests (OpenAI/Anthropic-style routes, `/v1/pi/stream`, media), resolving the key per request on the server side. Inbound auth is its own bearer (or scoped client tokens stored as digests, limited to models and accounts). Responses carry `Access-Control-Allow-Origin: *`, and `OPTIONS` is answered without auth. With `--no-auth` the token set is empty and every request is served.
- Trace: auth and routing https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/server.ts#L1022-L1068 ; key resolution on the server https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/dispatch.ts#L128-L141 ; CORS https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/http.ts#L306-L323 ; CLI https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-gateway-cli.ts#L378-L382 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-gateway-cli.ts#L452-L468
- Edge cases and failure handling: with `--no-auth` on loopback, any web page the user opens can call `http://127.0.0.1:4000` and read the response (CORS `*`, no cookie needed), spending the user's accounts; inferred from the headers, not tested. Gateway-wide routes (`/v1/usage`, `/v1/credentials/check`) are refused to scoped clients (server.ts lines 927-938). A bearer presented outside the `Authorization` header is rejected (lines 1064-1066).
- Round 1: `confirmed` (clients get no credentials; own bearer; 127.0.0.1:4000). Answers Round 2 question 13: no provider key reaches a gateway client; it does reach the gateway process and every direct broker client.
- For rizzy-harness: the gateway is the right shape for our master's provider proxy. Do not ship a wildcard-CORS unauthenticated mode; require auth even on loopback.
- Confidence: traced end to end (browser attack inferred). (checked in review)

#### Broker snapshot cache: AES-GCM keyed by SHA-256 of the bearer, URL as associated data
- What the code does: clients cache the snapshot on disk as `OMPS`+version+IV+ciphertext, key = SHA-256(token), AAD = magic, version and broker URL; file written 0600 via temp file.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/snapshot-cache.ts#L279-L301 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/snapshot-cache.ts#L339-L351
- Edge cases and failure handling: decrypt failure returns null and the client refetches. As the module doc says, anyone who can read the token can decrypt; it protects only a copied cache file.
- Round 1: `confirmed`.
- For rizzy-harness: not needed if nodes never cache keys.
- Confidence: read the definition. (checked in review)

### Hooks, extensions and trust

#### `tool_call` handlers fail closed with a 30 s default timeout
- What the code does: `emitToolCall` runs every extension's `tool_call` handlers in order with a timeout (`extensionHandlers.toolCallTimeoutMs`, default 30 000 ms); a timeout or throw becomes `{block: true}` with a reason; the first block wins; an abort while a handler is pending blocks.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L2332-L2379 ; default https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/settings.ts#L158-L161
- Edge cases and failure handling: handlers are in-process modules; a malicious one does not need to block anything, it already runs with full rights.
- Round 1: `confirmed`.
- For rizzy-harness: fail-closed hooks are right; the trust question is who may install them.
- Confidence: traced end to end. (checked in review)

#### No project trust gate: `isProjectTrusted()` always returns `true`
- What the code does: the extension API keeps upstream Pi's `isProjectTrusted()` for compatibility, and both implementations return `true`; the interface comment says OMP loads `.omp/extensions`, `.omp/config.yml` and other project inputs unconditionally and has no per-directory trust gate.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/types.ts#L926-L937 ; https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L1814-L1814
- Edge cases and failure handling: an extension written for Pi that checks this method will load project config believing the user approved it.
- Round 1: `confirmed` ("no trust prompt documented"), now settled from code. Upstream Pi does have a gate (per this comment); omp removed its effect.
- For rizzy-harness: every repository input is untrusted until a person on the node says otherwise.
- Confidence: read the definition. (checked in review)

#### Hooks and custom tools are imported from other harnesses' project directories as executable code
- What the code does: the hook capability includes `.claude/hooks/pre|post/*` from the project directory; the extension loader imports every discovered hook whose file ends in `.ts`/`.js` as a module (in-process code execution at session start). Custom tools are likewise loaded from project `.claude/tools/*.ts|js`. `--trusted-extension <file>` disables ambient discovery and loads only the named module files; `--no-extensions` keeps explicit roots only.
- Trace: Claude hook discovery https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L377-L396 ; import https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/loader.ts#L774-L786 ; extension test https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/loader.ts#L613-L615 ; custom tools https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L454-L467 ; trusted-extension flag https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/main.ts#L1782-L1800
- Edge cases and failure handling: Claude Code's own hooks are shell commands in JSON settings; omp's import ignores shell-script hooks (only `.ts`/`.js` pass the filter) but executes TypeScript a repository drops into `.claude/hooks/pre/`. A cloned repository is therefore code execution on open, before any tool approval.
- Round 1: `corrected`: Round 1 said hooks live in `.omp/hooks/pre|post/` or the user dir; the code also imports them from the project `.claude/hooks/`.
- For rizzy-harness: never execute repository-supplied code without an approval bound to its content hash.
- Confidence: traced end to end. (checked in review)

#### Project MCP servers from other harnesses' configs start without a prompt
- What the code does: MCP discovery reads project `.claude/.mcp.json` and `.claude/mcp.json` (Claude provider, `discovery/claude.ts` L92-L96), root `mcp.json`/`.mcp.json` (separate `mcp-json.ts` provider, L171), `.omp/mcp.json` (native provider, `builtin.ts` L238-L239), `.cursor/mcp.json`, `.vscode/mcp.json`, `.gemini/settings.json` and a root `mcp.json`; project entries are dropped only when `mcp.enableProjectConfig` is false (default true). Discovered servers are connected at session start, which spawns stdio server commands.
- Trace: switch https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/settings.ts#L9-L12 ; filter https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/config.ts#L131-L152 ; Claude sources https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L92-L96 ; connect at startup https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/sdk.ts#L2859-L2867
- Edge cases and failure handling: user-level `disabledServers` always wins; a project can re-enable `enableProjectConfig` through its own settings layer (see the layering finding).
- Round 1: `confirmed` (auto-import; no MCP-specific approval).
- For rizzy-harness: Claude Code asks before using a project `.mcp.json`; omp shows what happens without that prompt. Our node should require an explicit enable per server, stored on the node.
- Confidence: traced end to end (spawn inferred from `discoverAndConnect` naming; transport code not read). (checked in review) (corrected in review: file locations attributed to the right providers; stdio spawn is in fact confirmed, `mcp/transports/stdio.ts` `connect()` L593-L600 spawns the configured command with `Bun.env` plus `config.env`, and `sdk.ts` L2859-L2867 starts discovery at session creation)

## 4. Round 1 claims checked

| Claim (Round 1) | Verdict | Citation |
|---|---|---|
| Default approval mode is `yolo` | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/settings.ts#L314-L319 |
| Three modes: always-ask, write, yolo | corrected (fourth: `always-ask-cached`) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L43-L45 |
| Per-tool allow/prompt/deny; deny always wins | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/approval.ts#L426-L444 |
| `bash.patterns`: deny/prompt per segment, allow whole command | confirmed (wrappers not unwrapped) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L311-L321 |
| Critical bash patterns skipped in yolo | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/bash.ts#L657-L660 |
| Undeclared custom tools are exec; MCP tools write | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/tool-bridge.ts#L733-L735 |
| Headless fails closed when a prompt is needed | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L448-L471 |
| Provider safety checks always prompt, even in yolo | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L446-L455 |
| Subagents run yolo but inherit `tools.approval` | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L1259-L1262 |
| No OS sandbox | confirmed (search) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/file-write-fallback.ts#L1-L15 |
| No repo trust prompt | confirmed (`isProjectTrusted` returns true) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/types.ts#L926-L937 |
| `--trusted-extension` narrows loading | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/main.ts#L1782-L1800 |
| Hooks in `.omp/hooks/pre|post/` or user dir | corrected (also project `.claude/hooks/`) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L377-L396 |
| `tool_call` hooks fail closed on error/timeout, 30 s | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L2338-L2364 |
| Hooks cannot bypass approval | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L355-L374 |
| `PI_SHELL_PREFIX` wraps every shell command | confirmed (also `CLAUDE_CODE_SHELL_PREFIX`) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/utils/src/procmgr.ts#L101-L103 |
| `mcp.enableProjectConfig` narrows project MCP | confirmed (default true) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/config.ts#L131-L134 |
| Broker: 127.0.0.1:8765, bearer, no TLS, health open | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/server.ts#L829-L849 |
| Broker "loopback-only by default" | corrected (default bind only; any `--bind` accepted) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/cli/auth-broker-cli.ts#L145-L157 |
| Broker clients get API keys and access tokens; refresh stays | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth/pool.ts#L715-L735 |
| Gateway: own bearer, 127.0.0.1:4000, clients get no credentials | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-gateway/server.ts#L1022-L1068 |
| Broker snapshot cache encrypted, keyed from bearer + URL | confirmed (key = SHA-256 of token, URL as AAD) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth-broker/snapshot-cache.ts#L339-L351 |
| `agent.db` dir 0700, file 0600 | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/ai/src/auth/sqlite-credential-store.ts#L588-L605 |
| "No automatic secret redaction" | corrected (opt-in obfuscation exists, default off) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/secrets/settings.ts#L13-L25 |
| SSH: whether bash runs remotely is unknown | corrected (no remote exec back end; `ssh://` file I/O only) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/ssh/connection-manager.ts#L700-L709 |

## 5. In the code but not in the documentation (Round 1 survey)

- Project settings can loosen `tools.approvalMode`, `tools.approval` and `tools.restrictedFolders`; only `bash.patterns` is protected by `grantAuthority()`.
- `tools.restrictedFolders`: per-call prompts in every mode, non-grantable.
- `always-ask-cached` grants keyed by tier and path, persisted in `approval-cache.db`.
- ACP sessions treat the schema-default `yolo` as `always-ask` for bash.
- TypeScript hooks and custom tools imported from project `.claude/hooks/` and `.claude/tools/`.
- Child shell env: full inheritance, project-dotenv strip, `CLAUDECODE=1` injected.
- Secret obfuscation (off by default) and its restore-before-execution path.
- Speculative execution of auto-allowed local reads during streaming.
- `pi-shell` two-wave group-and-descendant kill.
- Gateway wildcard CORS and unauthenticated preflight; `--no-auth`.
- Project `ssh.json` with `${ENV}` expansion; `StrictHostKeyChecking=accept-new`.
- Model-settable `timeout: 0` disables the bash deadline.

## 6. Could not find or could not settle

- Whether `discoverAndConnect` spawns every stdio MCP server eagerly or lazily: transport code under `mcp/transports/` was not read; reading `mcp/manager.ts` connect path would settle it.
- Whether a bash command run inside the persistent shell can reach `PI_TOOL_BRIDGE_TOKEN` or the broker token through the environment: the eval bridge sets it for kernels (`eval/executor-base.ts`), the bash path was not traced. Grep `PI_TOOL_BRIDGE_TOKEN` and `OMP_AUTH_BROKER_TOKEN` usages to settle.
- The `wrapper.ts` behaviour when `context` is absent in direct (non-loop) execution: it inherits runner settings (schema default yolo) per the comment at approval.ts lines 91-95; not traced through Cursor exec handlers.
- Collab guest prompts against host policy and the browser-relay token (Round 2 questions in the sessions part) were not investigated in this cluster.
- The bypass examples (`bash -c` past a `deny`, eval past bash rules, CORS from a web page, env leak via `ssh.json`) were reasoned from code, not executed.

## 7. Citation check

`research/round2/oh-my-pi.safety.md: 143 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit`

## 8. Review

Independent review against the clone at the pinned SHA (`e325ffc7...`, confirmed with `git log`). Checked 30 of 32 findings (every finding except the two not opened: the bash interceptor and git guard, and the Rust kill semantics), plus the negative claims (no OS sandbox, no remote exec over SSH) by searching the clone myself.

| Finding | Verdict | Reason |
|---|---|---|
| Hook revisions revalidated | supported | `wrapper.ts` L265-L374: deny pre-check, then full re-resolve on `effectiveParams` |
| Tool concurrency / steering | supported | `resolveToolConcurrency` L113-L129 matches; steering lines not re-read |
| Speculative execution | supported | `host.ts` L216-L254 policy gate and `local_read` evidence |
| Default `yolo`, four modes | supported | `settings.ts` L314-L319, `approval.ts` L43-L45, L97-L113, L153-L158 |
| Resolution order | supported | `approval.ts` L426-L513, `resolveApproval` L366-L390 |
| Critical pattern list | supported | counted 21 regexes at `bash.ts` L197-L244; `rm -rf ~` not matched |
| `bash.patterns` grammar | supported | `bash.ts` L253-L321; `^`-anchored regex does not fire on `bash -c "rm ..."` (inferred, plausible) |
| Operator `bash.patterns` survive project layer | partly | `grantAuthority()` is also used for task model grants; wording narrowed |
| Project `.omp/config.yml` loosens approval | supported | `builtin.ts` L898-L928 reads project `settings.json`/`config.yml`; `settings.ts` L2800-L2814 merges with only `dropSettingsGroupShadows`; `projectLayerForMerge` filters only invalid `modelRoles`; UI-only hide at `settings-ui.ts` L44 |
| Session and cached grants | supported | `approval.ts` L176-L214, L598-L612; `approval-cache.ts` L18-L27 |
| Headless fail closed | supported | `wrapper.ts` L446-L471, error text recommends yolo |
| ACP default vs explicit yolo | supported | `session-tools.ts` L1170-L1217 |
| Sub-agents forced to yolo | supported | `task/executor.ts` L1259-L1278, `derived: true`; per-spawn `overrides` spread after it can still change the mode |
| MCP tools are `write`; unset is `exec` | supported | `tool-bridge.ts` L735, `approval.ts` L274-L296 |
| `eval` outside bash rules | supported | `eval.ts` L354 `exec`; bridge bearer is `crypto.randomUUID()` on 127.0.0.1 |
| Bash execution, 300 s, `timeout: 0` | supported | `tool-timeouts.ts` L13, `sdk.ts` L4814-L4824 |
| Output truncation numbers | supported | spill 50, max 16 (units not re-verified) in `tools/settings.ts` |
| Background jobs | supported | `exec/settings.ts` threshold 60 000 ms, retained shells in `bash-executor.ts` L270-L284 |
| SSH file I/O only | supported | no tool consumes `buildRemoteCommand` except file transfer and probes; no `ssh` tool in `tools/`; `accept-new` at `connection-manager.ts` L200 |
| No OS sandbox | absence confirmed | `git grep` for landlock, seccomp, bwrap, seatbelt, firejail, nsjail finds only unrelated `pi-walker` comments and `clone_new` |
| Child shells inherit env | supported | `procmgr.ts` L43-L55 `buildSpawnEnv`; filter removes only dotenv-sourced values |
| Secret obfuscation | supported | `secrets.enabled` default false; restore in `transformToolCallArguments` |
| Credential store perms | supported | `sqlite-credential-store.ts` L588-L605 |
| Auth broker | supported | bind default, `isAuthorized` empty set = open, `Set.has`, `/v1/healthz` unauthenticated, `POST /v1/credential`, snapshot redacts only `refresh` (`pool.ts` L715-L735); no loopback check on `--bind` |
| Auth gateway | supported | `server.ts` L1022-L1068, `http.ts` L306-L323 `*` CORS, preflight before auth, `--no-auth` empty token set |
| Broker snapshot cache | supported | key SHA-256(token), AAD magic+version+URL |
| `tool_call` handlers fail closed | supported | default 30 000 ms at `extensibility/settings.ts` L158-L161 |
| `isProjectTrusted()` always true | supported | `types.ts` L926-L937, `runner.ts` L1814, `agent-session.ts` L9771 |
| Hooks and custom tools from `.claude/` | supported | `claude.ts` L377-L396, L454-L467; `loader.ts` L613-L615, L774-L786 |
| Project MCP servers without prompt | partly | locations were attributed to the wrong provider (root `mcp.json` is `mcp-json.ts`, Claude reads `.claude/.mcp.json`); fixed. Spawn is now confirmed (`stdio.ts` L593-L600) |
| Bash interceptor and git guard | not checked | |
| Kill semantics in `pi-shell` | not checked | |

Notes found in passing: (1) section 6 says it could not tell whether stdio MCP servers are spawned at session start and whether they inherit the environment; the stdio transport in `mcp/transports/stdio.ts` L593-L596 builds `{...Bun.env, ...config.env}` and spawns, so both are settled (the sessions file already says so). (2) `grantAuthority()` also guards model grants, which section 5's bullet "only `bash.patterns` is protected" does not mention; it is accurate for tool approval.

Overall: the file's approval, settings-layering, broker and gateway claims all hold up against the code, including the two the owner will most want to rely on (a repository can undo the user's approval mode; sub-agents are yolo). The unchecked remainder is two low-risk findings; the "inferred" bypass examples (`bash -c` past a deny, eval past bash rules, CORS from a web page, `ssh.json` env leak) follow from the code read here but were not run, as the file says.
