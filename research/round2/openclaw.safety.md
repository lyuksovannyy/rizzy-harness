# OpenClaw: Round 2 source investigation, cluster `safety`

## 1. Repository

- `openclaw/openclaw`, pinned SHA `8bac830dcb8ff99055ba71558dd369a989cc7881` (branch `main`, commit date 2026-11-10T20:27:14Z), clone at `.claude/research-repos/openclaw` (about 1 GB).
- Read: the exec approval path (`src/infra/exec-approvals-*`, `src/infra/system-run-*`, `src/agents/bash-tools.exec-*`), the LLM reviewer (`src/agents/exec-auto-reviewer.ts`, `src/infra/exec-auto-review.ts`), the node-host side of `system.run` (`src/node-host/invoke-system-run.ts`, `src/node-host/exec-policy.ts`), the Gateway guard on `node.invoke` (`src/gateway/node-invoke-system-run-approval.ts`), the Docker sandbox (`src/agents/sandbox/*`), host env scrubbing (`src/infra/host-env-security.ts`), the secret egress proxy (`src/secrets/egress-proxy/*`), the worker protocol (`src/gateway/server/ws-connection/worker-connection*.ts`, `src/gateway/worker-environments/{admission,credential,inference-runtime}.ts`), role and method authorization, the agent loop (`packages/agent-core/src/agent-loop.ts`), the `before_tool_call` chain and loop detection (`src/agents/tool-loop-*`).
- Not read: Windows (`cmd.exe`, PowerShell) paths beyond where they gate POSIX logic; OpenShell and Crabbox backends; the SSH sandbox transport beyond its defaults; channel plugins' native approval UIs.

## 2. Architecture as it bears on this cluster

- TypeScript on Node, one Gateway process plus workers it spawns. The repository is a pnpm monorepo: `src/` (Gateway, agents, infra, node host), `packages/` (`agent-core` loop, `gateway-protocol` TypeBox schemas), `extensions/` (plugins).
- **Agent loop**: `runLoop` in `packages/agent-core` is an unbounded `while (true)` that streams a model response, runs tool calls in batches and re-enters until no tool calls remain or the signal aborts (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L250-L253). It has no step cap; the stops are abort, per-attempt timeouts and optional loop detection.
- **Tool dispatch** goes through an ordered `before_tool_call` chain: loop admission, owner probes, trusted plugin policies, approvals, normal plugin hooks, final owner approval (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.before-tool-call.policy.ts#L1-L7).
- **Exec policy** has three axes: `host` (`auto|sandbox|gateway|node`), `security` (`deny|allowlist|full`) and `ask` (`off|on-miss|always`). `mode` (`deny|allowlist|ask|auto|full`) is a projection of those three (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-approvals-core.ts#L13-L17). Gateway-host decisions are made in `processGatewayAllowlist` (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L112-L130). Node-host decisions are made again on the node, in `evaluateSystemRunPolicyPhase` (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L334-L357).
- **Sandbox**: the Docker or Podman CLI, driven by `buildSandboxCreateArgs` (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/container-allocation.ts#L54-L75). The SSH, OpenShell and Crabbox back ends sit beside it.
- **Remote execution**: paired nodes connect with `role: node` and may call only `node.*` methods. Cloud workers use a separate, closed dispatcher (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection-dispatch.ts#L90-L121).
- Compaction is outside this cluster's file and was not traced here.

## 3. Findings

### The agent loop and tool dispatch

#### The core loop has no step limit
- What the code does: `runLoop` loops with `while (true)` around an inner `while (hasMoreToolCalls || pendingMessages.length > 0)`. Each pass streams one assistant message, executes its tool calls and continues. Termination comes from `stopIfAborted`, which persists an aborted assistant message so post-processing does not continue from a dangling `toolUse`, or from a response with no further tool calls.
- Trace: the loop (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L250-L253); abort handling (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L191-L222); tool batches run through `executeToolCalls` (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L315-L338).
- Edge cases and failure handling: a provider `error` or `aborted` stop drops the remaining tool calls (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/agent-core/src/agent-loop.ts#L341-L349). A search for `maxTurns`, `maxIterations` and `maxToolCalls` found only a headless code-mode budget and a CLI-backend terminal reason; there is no general cap.
- Round 1: `new (not documented)`. Round 1 lists timeouts but no step cap.
- For rizzy-harness: an unbounded loop is acceptable only if the stuck detector is on by default (OpenClaw's is not, see below). Keep our explicit step cap.
- Confidence: `read the definition`. (checked in review)

#### `before_tool_call` hooks fail closed
- What the code does: if any stage of the chain throws (loop admission, trusted policies, approval, plugin hooks), the call is blocked with "Tool call blocked because before_tool_call hook failed". A hook can also block explicitly or rewrite parameters. The parameters are cloned for hook isolation, and the approved snapshot is cloned separately.
- Trace: the ordering contract (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.before-tool-call.policy.ts#L1-L7); the failure reason (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.before-tool-call.policy.ts#L50-L51); the catch that returns `blocked: true` (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.before-tool-call.policy.ts#L397-L408).
- Edge cases and failure handling: a plugin hook's block reason falls back to "Tool call blocked by plugin hook" (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/agent-tools.before-tool-call.policy.ts#L318-L324).
- Round 1: `confirmed`. Round 1 says hooks can intercept `before_tool_call` with a blocking terminal; the code adds that failure blocks.
- For rizzy-harness: copy fail-closed for any hook in the approval path.
- Confidence: `read the definition`. (checked in review)

#### Tool results are capped by context size
- What the code does: a live tool result is capped at 30% of the context window (estimated at 4 characters per token) and at a hard cap of 16k, 32k or 64k characters for windows under 100k, at least 100k and at least 200k tokens.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-result-limits.ts#L6-L35
- Edge cases and failure handling: sandbox shell bridges also bound buffered output at 100 MB before it reaches the Gateway heap (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/constants.ts#L16-L18).
- Round 1: `new (not documented)`.
- For rizzy-harness: a context-proportional cap is better than our fixed one; consider it.
- Confidence: `read the definition`.

#### Loop detection thresholds, and rolling detection is off by default
- What the code does: the last 30 calls per session are hashed as tool plus stable-stringified arguments, with `exec`'s `title` ignored. Outcomes are hashed with volatile tokens (timestamps, PIDs, durations) masked. The thresholds are a warning at 10 and critical at 20, plus a global circuit breaker at 30 identical no-progress outcomes, 10 repeated unknown-tool calls, and `wait` going critical at 10. The detectors are generic repeat, poll with no progress, ping-pong (critical at 20 only with no-progress evidence) and argument churn. A critical result blocks the call (`critical-tool-loop`); a warning is injected once per key.
- Trace: constants (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-detection.ts#L56-L59) and the warning threshold (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-thresholds.ts#L1); volatile masking (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-detection.ts#L160-L185); the decision ladder (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-detection.ts#L563-L612); the critical block (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-admission.ts#L71-L85).
- Edge cases and failure handling: admission returns immediately unless `loopDetection.enabled === true` (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-admission.ts#L103-L109). The schema help states "Omit to keep rolling detection off and post-compaction protection on" (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/config/schema.help.runtime.ts#L458-L459). Batches are admitted atomically against a projected history (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-admission.ts#L131-L154).
- Round 1: `new (not documented)`. Round 1 left the thresholds unverified; the off-by-default status was not known.
- For rizzy-harness: the outcome-hash idea (block only when results prove no progress) is worth copying. Enable it by default.
- Confidence: `traced end to end`. (checked in review)

### Approvals and permission rules

#### Real defaults: host exec is `security=full`, `ask=off`; the default security value for a sandbox target is `deny`, but sandboxed exec is not refused by it
- What the code does: the default security is `deny` when the effective host is the sandbox and `full` otherwise; `ask` defaults to `off`. The approvals file constants are `DEFAULT_SECURITY = "full"`, `DEFAULT_ASK = "off"` and `askFallback = "deny"`. The approvals file can only tighten a layered policy (`minSecurity`, `maxAsk`), except for admin-only `full` session mode, which skips the host floors. `host` defaults to `auto`, which resolves to the sandbox when one is available and to the Gateway otherwise.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-defaults.ts#L171-L203 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-approvals-config.ts#L94-L96 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-target.ts#L96-L97 ; tightening only (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-approvals-policy.ts#L34-L42).
- Edge cases and failure handling (added in review): the `deny` default for a sandbox target does not stop sandboxed commands from running. The pre-run check throws `exec denied: ... security=deny` only when `host !== "sandbox"`, or the mode is explicitly `deny`, or security was set to `deny` explicitly (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-run.ts#L309-L317). A default-configured sandboxed agent therefore runs its commands inside the container with no approval. The original wording "the sandbox is `deny`" read as the opposite.
- Edge cases (continued): a session marked `sandbox: required` throws before execution if the sandbox is unavailable and refuses `elevated`, `gateway` and `node` targets (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-target.ts#L45-L66). The default approval timeout is 30 minutes (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-approvals-core.ts#L265-L266).
- Round 1: `confirmed` for `full` on host, `ask` off and `askFallback` deny; the `deny` on sandbox is only the default value of the approval axis and does not block sandboxed exec (see above).
- For rizzy-harness: this is the open default we must not copy.
- Confidence: `traced end to end` (corrected in review: the sandbox `deny` default does not block sandboxed exec; the host defaults, the tightening rule and the 30 minute timeout were checked)

#### `security=full` with `ask=on-miss` never prompts
- What the code does: `requiresExecApproval` prompts when `ask` is `always`. With `on-miss` it prompts only when `security` is `allowlist` and analysis fails or the allowlist misses. `full` plus `on-miss` therefore means no prompt. The `mode` mapping says the same: `ask` is `allowlist` plus `on-miss`, `auto` is the same plus the reviewer, and `full` is `full` plus `off`.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-approvals-policy.ts#L14-L32 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-approvals-core.ts#L100-L119
- Edge cases and failure handling: the display projection cannot express `full`/`on-miss` or `always`, so writes preserve the exact pair (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-approvals-core.ts#L88-L98).
- Round 1: `new (not documented)`.
- For rizzy-harness: avoid orthogonal knobs whose combinations surprise people; one ordered mode is clearer.
- Confidence: `read the definition`. (checked in review)

#### Session permission modes map to exec modes and tighten only
- What the code does: `read-only` maps to `deny`, `guarded` to `ask`, `workspace` to `auto` (the reviewer) and `full` to `full`. Every mode except `full` is workspace-only for file tools and `apply_patch`, and `full` alone bypasses the host approval floors. When a retained mode and a current mode differ they intersect to the stricter, and switching between an explicit and a default policy mid-turn throws.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/session-permission-exec-mode.ts#L13-L18 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/session-permission-exec-mode.ts#L29-L64
- Edge cases and failure handling: see the intersection throw at L45-L49 in the same file.
- Round 1: `confirmed` (four modes, `full` bypasses).
- For rizzy-harness: a ceiling that intersects across turns is a clean way to stop mode changes widening work already in flight.
- Confidence: `read the definition`. (checked in review)

#### Shell commands are parsed by tree-sitter-bash, with hard size and time limits
- What the code does: `planShellAuthorization` parses the command with `web-tree-sitter` and the `tree-sitter-bash` WASM grammar (at most 128 KiB, 500 ms progress deadline). The result is split into steps, groups and candidates. Risks are sorted into three sets. Prompt-only: `eval`, `source`, `alias`, carriers. Non-reusable: inline eval. Unanalyzable: command, process and here substitutions, heredocs, redirects, dynamic executables or assignments, line continuations, function definitions, syntax errors. An unanalyzable plan falls back to prompting for the whole command. `if`, `for`, `while`, `case`, subshells, groups and backgrounding are unsupported topology.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/command-explainer/tree-sitter-runtime.ts#L11-L58 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-authorization-plan.ts#L529-L560 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-authorization-risks.ts#L6-L36
- Edge cases and failure handling: a parse failure or timeout becomes `unanalyzablePlan`, which means a prompt, never an allow. Windows `cmd` is unanalyzable for this planner (L535-L541 of the plan file).
- Round 1: `confirmed`. Round 1 says chains need every segment allowed and redirections and substitution are rejected; the code shows that "rejected" means "forces approval".
- For rizzy-harness: tree-sitter-bash with a deadline is the same converged choice as Codex. A Rust crate exists (`tree-sitter-bash`); it would be a dependency to vet.
- Confidence: `read the definition`. (checked in review)

#### Allow-list matching: realpath globs, basename only for PATH lookups, and "always allow" is an argv-and-cwd hash
- What the code does: `matchAllowlist` matches patterns that contain a path selector against the resolved real path. Bare names match the executable's basename only when the raw token itself had no path separator, so `./tool` and `/tmp/tool` never match a bare `tool` grant. Hand-written `argPattern` regexes are matched against arguments joined with `\0`. Generated `allow-always` entries are not globs: they keep the executable path as the entry pattern and add an `argPattern` holding `sha256(len cwd | cwd | argc | len arg | arg ...)` where the arguments are `argv[1..]` only, so the pair authorizes one exact argument list in one exact cwd for that executable path (corrected in review: the hash does not cover argv[0]). Path-only generated entries from older versions are inactive.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-command-resolution.ts#L184-L231 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-command-resolution.ts#L321-L378
- Edge cases and failure handling: an invalid regex never matches (L301-L308 of the same file). A bare `*` matches any resolved command but is ignored for generated entries (L334-L341).
- Round 1: `corrected`. Round 1 says "per-agent globs on resolved binary paths with optional regex argPattern; allow-always entries bind to exact argv and cwd". The binding is a hash rather than a stored argv, and basename grants are refused for explicit paths.
- For rizzy-harness: copy "basename grants only for PATH lookups" and the length-prefixed hash; both close known prefix-rule holes.
- Confidence: `read the definition` (corrected in review: the allow-always hash covers cwd and `argv[1..]`; the executable identity is the entry's own path pattern)

#### Safe bins exclude interpreters and `awk`, `sed`, `jq` by name
- What the code does: safe bins must resolve from trusted directories (by default `/bin` and `/usr/bin`). `jq`, the awk family and `sed`/`gsed` are refused with specific reasons, and a separate list of shells and interpreters (bash, python, node, perl, busybox, toybox and others) cannot be safe bins.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-safe-bin-trust.ts#L12 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-safe-bin-semantics.ts#L4-L40 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-safe-bin-runtime-policy.ts#L21-L60
- Edge cases and failure handling: writable trusted directories log a warning once (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L114-L119).
- Round 1: `confirmed`.
- For rizzy-harness: if we add read-only auto-allows, take this deny list as the starting point.
- Confidence: `read the definition`. (checked in review)

#### What an approval is bound to, and how the executable is pinned
- What the code does: the binding has three layers.
  - Request binding (`SystemRunApprovalBinding`): exact argv, cwd, agentId, sessionKey, and `envHash`. The hash is SHA-256 over the sorted request env overrides, not the inherited environment. An empty override set means `envHash = null`, and supplying overrides against a null binding is refused.
  - File binding: before review, every resolved segment executable and every wrapper hop (`env`, `nice` and so on) is resolved without cache. If the path, its directory or any symlink component is writable or owned by the current process, the file's SHA-256 is stored (`kind: mutable`). Otherwise only its real path is stored (`kind: identity`). PATH and PATHEXT at review time are recorded, and script operands are hashed too.
  - Cwd binding: the cwd's `realpath` and `stat` (with bigint inode identity) are captured, and a symlinked cwd is refused, and so are symlink path components that sit in a directory the process can write (`hasMutableSymlinkPathComponentSync`); symlink components in directories it cannot write are accepted (corrected in review).

  Pinning is done by rewriting the command text: each executable span in the source is replaced with the planned argv[0], the resolved real path, so the shell no longer searches PATH at launch.
- Trace: env binding (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L42-L90); match (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L126-L165); executable identity (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L366-L458); the "mutable" test (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-mutable-file-policy.ts#L19-L66); the hash (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-file-snapshot.ts#L6-L39); cwd (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-cwd-binding.ts#L15-L61); the rewrite to real paths (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-approvals-analysis.ts#L46-L65) and the span replacement (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-authorization-render.ts#L144-L191).
- Edge cases and failure handling: binding is refused for `.`/`source`, commands after `cd`/`pushd` in a chain, shell startup env (`BASH_ENV`, `ENV`), pipelines with more than one candidate per group, and unresolved executables (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L299-L340 , https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L491-L534). An unreadable script is refused rather than treated as unbound (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-file-snapshot.ts#L27-L37). Shell expansion inside arguments makes the enforced render fail, so a prompt is forced (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-authorization-render.ts#L144-L146). Mutable operands downgrade "allow always" to one-shot (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L458-L459 , https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L561-L568).
- Round 1: `corrected`. The docs say "content hash when the executable is writable". The code hashes whenever the file, its parent directory or a symlink component is mutable by the current user, or is merely owned by them. The env binding covers request overrides only, and agentId and sessionKey are also bound.
- For rizzy-harness: this is the design to copy. Our node already resolves paths component by component; add the argv rewrite to absolute real paths, the inode check on cwd and the "mutable if the user can write the directory" rule.
- Confidence: `traced end to end` (corrected in review: only mutable symlink components are refused, not every symlink component; the env hash, the "mutable" rule, the real-path pinning and the node/device binding were checked)

#### Re-check at launch, and the remaining TOCTOU window
- What the code does: after approval (human, reviewer or fallback), `revalidateBeforeExecution` re-runs the file binding from the stored argv rather than re-parsing. It re-resolves each executable with the stored PATH and compares its real path and invocation path, re-hashes mutable files and re-stats the cwd. It runs in `launchLifecycle.prepare` just before `supervisor.spawn`; the spawn wrapper then asserts source, operator and policy currency synchronously.
- Trace: the gateway hook (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L594-L609); revalidation (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L550-L620); the denial wrapper (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-approval-binding.ts#L14-L49); placement before spawn (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-launch.ts#L32-L42 , https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-runtime.ts#L573-L587); the node side (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L653-L685).
- Edge cases and failure handling: the check reads the file by path with `readFileSync`, and the shell later `exec`s the same path. No file descriptor is held, so a write between hash and `execve` is not detected (inferred from the code: no `fexecve` or fd-passing path was found). The same applies to a mutable `cwd` swapped after `stat`. The window is small but real, which matches the docs' own admission.
- Round 1: `confirmed` (re-check before launch); the TOCTOU window is `new (not documented)` in code terms.
- For rizzy-harness: in Rust we can open the executable once, hash from the fd and exec through `/proc/self/fd/N` (or `execveat` with `AT_EMPTY_PATH`) to close the window. Do better than OpenClaw here.
- Confidence: `traced end to end` (Gateway); `read the definition` (node). (checked in review)

#### A replayed approval cannot cover another call: Gateway-side guard on `node.invoke system.run`
- What the code does: `sanitizeSystemRunParamsForForwarding` always strips the client's `approved` and `approvalDecision` flags. It re-adds them only from a server-side record found by `runId`, after checking all of these:
  - the record's `nodeId` equals the target;
  - the requester's device id equals the one recorded (or its connId when no device identity exists);
  - argv, cwd, agentId, sessionKey and envHash match the stored binding;
  - the stored decision is still active.

  `allow-once` is consumed atomically (`consumeAllowOnce`), so a second use fails. A reviewer `allow-once` is forwarded as `approvalSource: auto-review`, never as an operator flag.
- Trace: strip (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-invoke-system-run-approval.ts#L197-L252); node and device binding (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-invoke-system-run-approval.ts#L297-L340); binding match (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-invoke-system-run-approval.ts#L382-L401); consume (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-invoke-system-run-approval.ts#L434-L452).
- Edge cases and failure handling: a timed-out request can use `ask-fallback` only for clients with approval scope, only with a plan, and only once (L457-L480 of the same file).
- Round 1: `confirmed`, with more fields than documented (nodeId, device, one-shot consumption).
- For rizzy-harness: this is the answer to section 14 [2]. Approval id plus node plus requester plus exact binding plus single consumption is the pattern to copy for our approval events.
- Confidence: `traced end to end`. (checked in review)

#### Chat approvers: an empty approver list falls back to "same chat"
- What the code does: channel plugins authorize `/approve` against an approver id list from config. When that list is empty the helper returns `authorized: true`, tagged as an implicit same-chat fallback, so anyone in the originating chat can resolve the approval.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/approval-auth-helpers.ts#L72-L89
- Edge cases and failure handling: whether every channel honours the marker was not traced per channel (section 6).
- Round 1: `new (not documented)`. Round 1 says native UIs "check approver lists"; the empty-list default is weaker than that implies.
- For rizzy-harness: never default an empty approver set to "whoever is in the room".
- Confidence: `read the definition`. (checked in review)

#### The node enforces its own policy, but a Gateway-forwarded approval satisfies its allow-list
- What the code does: the node recomputes `security` and `ask` from its own config and its own approvals file (`minSecurity`/`maxAsk`). It refuses any `approvalSource`, `approved` or `approvalDecision` that does not come with a `systemRunPlan` exactly matching argv, command text, cwd, agentId and sessionKey. A delayed approval is refused if the node's policy snapshot changed since the request. In `evaluateSystemRunPolicy`, however, `approved === true` sets `approvedByAsk`, and that satisfies both "ask" and "allowlist miss". Only a local `security=deny` is final against a Gateway-asserted approval.
- Trace: local policy (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L121-L141); plan matching (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L233-L264); policy snapshot check (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L349-L357); decision (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117).
- Edge cases and failure handling: env overrides blocked by the host env policy, including PATH, are refused on the node (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L265-L290).
- Round 1: `corrected`. Round 1 and synthesis say "the gate is on the Gateway". The node does enforce (deny, local `ask=always`, binding, policy drift), but it trusts the Gateway's approval flag over its own allow-list.
- For rizzy-harness: this answers section 14 [4] (both sides check). Ours must go further: a master-asserted approval must not override `node.toml`'s allow-list, because "the node decides".
- Confidence: `traced end to end`. (checked in review)

#### Heredocs, inline eval and unbindable plans force approval; headless runs deny
- What the code does: in allowlist mode a heredoc segment requires the reviewer or a human even when the allow-list matched. `strictInlineEval` does the same for `python -c` and similar. When the Gateway cannot render an enforced command (for example because of shell expansion in arguments), a prompt is required, and with `ask=off` and `askFallback=deny` the call is denied before a request is registered. Non-interactive runs without the reviewer are denied ("approval_required").
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L361-L430
- Edge cases and failure handling: a timed-out approval falls back to current policy only if the allow-list still matches and an enforced plan exists (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L287-L303).
- Round 1: `new (not documented)`.
- For rizzy-harness: "allowed but not bindable" should mean ask, never allow.
- Confidence: `read the definition`.

#### The "auto" reviewer: prompt, input, limits and failure mapping
- What the code does: the reviewer is a separate one-shot completion with a fixed system prompt. "allow" covers routine development work; "deny" covers credential probing, exfiltration, persistence, destruction and the like; "ask" is reserved for high-risk but legitimate actions (force-push, publishing, production). It runs at temperature 0, with a 1024 output-token budget (raised by `adjustMaxTokensForThinking` for reasoning models, so it is not a hard ceiling there), a 30 s timeout (configurable, minimum 1 s) and a 16,000-character input cap.
- What the reviewer sees: command, `executionCommand`, argv, resolvedPath, cwd, env key names (no values), host, reason and analysis flags, inside `UNTRUSTED_EXEC_REQUEST_JSON` markers. Session identifiers are deliberately left out. A transcript can be added: up to 40 non-user entries, 4,000 characters per user or assistant entry, 1,000 per tool entry and 24,000 in total, each entry labelled with an origin (operator, channel, inter_session and so on) and scrubbed of identifiers.
- The response must be strict JSON with no extra keys, no duplicate keys and no `__proto__`. "allow" with high or unknown risk becomes "ask". The model is `tools.exec.reviewer.model` when set (with none set it presumably falls back to the agent's model, inferred).
- Trace: the prompt (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-auto-reviewer.ts#L26-L58); limits (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-auto-reviewer.ts#L73-L76); input (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-auto-reviewer.ts#L91-L150); parse (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-auto-reviewer.ts#L260-L315); transcript caps (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-auto-review-transcript.ts#L13-L18); the transcript is attached per call (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-run.ts#L200-L217).
- Edge cases and failure handling: timeouts, an unavailable model, a non-`stop` finish, malformed JSON, an oversized input and exceptions all become `ask` with risk unknown (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-auto-reviewer.ts#L396-L516 , https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-auto-review.ts#L124-L146). With no reviewer configured, every case is `ask` ("Auto mode must never become a static allowlist", https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-auto-review.ts#L148-L159). Before any model call, a regex heuristic for reviewer-directed text in the command, argv, path, cwd or env keys returns `deny` with risk high (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-auto-reviewer.ts#L152-L191 , L402-L411).
- Round 1: `confirmed` (an LLM reviewer that grants, denies or escalates); the details are `new (not documented)`.
- For rizzy-harness: if we ever add a model approver, copy the strict-schema parse (no duplicate keys, closed keys), "failure means ask", and origin-labelled transcript evidence.
- Confidence: `traced end to end` (corrected in review: the 1024 token limit is raised for reasoning models; temperature 0, 30 s, 16,000 characters, the deny-on-directive heuristic and failure-to-ask were checked)

#### The reviewer is skipped for login or interactive shells and unbindable dispatch chains, and is capped at 64 candidates
- What the code does: Gateway auto-review runs only when `ask != always`, the command has at most 64 candidates or segments, no POSIX login or interactive shell startup precedes the inline command, and the dispatch chain can be pinned. Otherwise a human is required, with a warning. A reviewer `allow-once` must still carry risk low or medium or it falls through to a human, and the binding is revalidated before the approval is committed.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L631-L643 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L699-L734 ; warnings (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-auto-review.ts#L104-L108)
- Edge cases and failure handling: an abort while reviewing rethrows and does not keep approval authority (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L679-L689).
- Round 1: `confirmed` (login or interactive wrappers skip the reviewer).
- For rizzy-harness: n/a beyond the previous finding.
- Confidence: `read the definition`.

#### The three-denial escalation is an in-memory counter on the Gateway host path only
- What the code does: `consecutiveAutoReviewDenials` is an LRU of 256 session keys holding a count capped at 3. Each reviewer `deny` increments it. At 3 the denial is turned into a human prompt instead of a returned denial, and only a reviewer `allow-once` deletes the entry. Once at 3, every further denial escalates until an allow.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L96-L110 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L690-L741
- Edge cases and failure handling: the counter is lost on restart and evicted after 256 concurrent sessions. Without a session key the count is always 1, so there is never an escalation. The node-host reviewer path has no counter: a `deny` there is a plain denial (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L544-L569). A search for `CONSECUTIVE_AUTO_REVIEW` outside the Gateway host file found nothing.
- Round 1: `corrected`. "Three consecutive denials escalate to human" holds only for Gateway-host exec and is not durable.
- For rizzy-harness: if we copy escalation, persist it in the session log so a restart cannot reset it.
- Confidence: `traced end to end`. (checked in review)

#### On a node, the reviewer runs on the node with the node's own config and no transcript
- What the code does: when the node's local mode is `auto`, `invoke-system-run` calls `createModelExecAutoReviewer` with the node's config and reviewer settings, so the node needs its own model credentials. It passes only command facts, with no transcript, and requires a single segment whose inner argv matches the request.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L491-L543
- Edge cases and failure handling: `ask` from the node reviewer ends as a denial with "deferred to human approval" text (L566-L573), because the node has no human route of its own.
- Round 1: `new (not documented)`.
- For rizzy-harness: shows the cost of a node-side model approver (keys on the node). Our node would need the same, or should keep human approval.
- Confidence: `read the definition`. (checked in review)

#### Cron standing grants bypass the prompt for an exact job and operation, and fall back to a prompt
- What the code does: for cron runs, an earlier `allow-always` mints a SQLite standing grant bound to job, config revision and operation. It is consumed at the pre-spawn hook and checked for expiry. Inline eval, heredocs, mutable operands, `ask=always` and `security=deny` always keep prompting or denying. A grant-store failure falls through to the normal prompt.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L460-L529
- Edge cases and failure handling: an invalidated grant denies that run and says the next occurrence will prompt (L519-L525).
- Round 1: `confirmed` (standing grant for job, command, cwd, env).
- For rizzy-harness: relevant for our routines; consumption at the spawn boundary is the right place.
- Confidence: `read the definition`.

### Sandboxing

#### The sandbox is off by default; Docker is the default back end
- What the code does: `mode` defaults to `off`, `workspaceAccess` to `none`, `backend` to `docker` and `scope` to the result of `resolveSandboxScope`. With `workspaceAccess: none` only the sandbox's own workspace directory is mounted; with `ro` or `rw` the agent workspace is also mounted at `/agent`, read-only for `ro`. Managed skill directories are mounted read-only on top.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L212-L227 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L247-L251 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/workspace-mounts.ts#L179-L202
- Edge cases and failure handling: a required sandbox that is unavailable throws rather than running on the host (see the defaults finding).
- Round 1: `confirmed` (off by default). The mode and `workspaceAccess` values Round 1 left unchecked are confirmed.
- For rizzy-harness: n/a.
- Confidence: `read the definition`. (checked in review)

#### The `docker create` invocation: what is fixed and what is configurable
- What the code does: `buildSandboxCreateArgs` always adds `--init`, identity labels (`openclaw.sandbox`, sessionKey, `configHash`, `createArgsEpoch`), `--security-opt no-new-privileges`, `--pull=never` for Docker and an `--env-file`. The rest comes from config, with these defaults: `--read-only` (`readOnlyRoot: true`), tmpfs `/tmp` (1 GB), `/var/tmp` and `/run` (256 MB each, with a size added automatically), `--network none`, `--cap-drop ALL`, `--pids-limit 1024` and `ulimit core=0`. `user`, memory, CPUs, GPUs, seccomp and AppArmor profiles, DNS and extra hosts are optional. The container runs `sleep infinity` (or `/bin/sh -c ... exec "$@"` for streams), and commands are executed into it. The created id must be a 64-hex immutable id.
- Trace: config defaults (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L91-L117); flags (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/container-allocation.ts#L71-L167); allocation (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/container-allocation.ts#L214-L273); image default `openclaw-sandbox:trixie-slim` (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/constants.ts#L10)
- Edge cases and failure handling: a change to the create flags bumps `SANDBOX_DOCKER_CREATE_ARGS_EPOCH`, so existing containers are recreated rather than kept with stale flags (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/constants.ts#L62-L64). No default seccomp profile is supplied, so the engine's default applies, and `user` is unset, so the image's user applies (inferred).
- Round 1: `new (not documented)` at this level of detail.
- For rizzy-harness: a good baseline list if we add a container back end. Note that `no-new-privileges` is the only flag config cannot remove.
- Confidence: `read the definition`. (checked in review)

#### Who can change the image and the binds, and what validation blocks
- What the code does: image, binds and network come from `agents.defaults.sandbox.docker` and per-agent overrides in `openclaw.json`; with `scope: shared` the per-agent overrides are dropped. At create time `validateSandboxSecurity`:
  - rejects non-absolute bind sources;
  - rejects binds of `/`, `/etc`, `/proc`, `/sys`, `/dev`, `/root`, `/boot` and `/run`;
  - rejects Docker socket paths;
  - rejects `~/.aws`, `.cargo`, `.config`, `.docker`, `.gnupg`, `.netrc`, `.npm` and `.ssh`, whether the source sits inside one of them or covers it;
  - rejects sources outside the workspace roots;
  - rejects targets over `/workspace` or `/agent`;
  - rejects `--network host` and `container:*`;
  - rejects seccomp or AppArmor `unconfined`.

  Each check is applied lexically and through the real path of the nearest existing ancestor. Three `dangerously*` booleans relax the roots, the reserved targets and namespace joins.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/validate-sandbox-security.ts#L23-L53 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/validate-sandbox-security.ts#L93-L132 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/validate-sandbox-security.ts#L274-L356 ; scope drops agent overrides (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L245-L258)
- Edge cases and failure handling: the file states its own threat model, "local-trusted config, but protect against foot-guns and config injection" (L1-L6). The agent's `gateway` tool exposes only `config.get` and `config.schema.lookup` plus maintenance actions, not config writes (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tools/gateway-tool.ts#L77). A host-exec agent with `security=full` can still edit `openclaw.json` directly (inferred).
- Round 1: `new (not documented)`.
- For rizzy-harness: if we bind-mount, copy the covers-or-targets rule and the realpath-via-ancestor check.
- Confidence: `read the definition`. (checked in review)

#### Sandbox tool policy: a default allow and deny set
- What the code does: inside a sandbox the default tool allow-list is exec, process, read, ls, write, edit, apply_patch, view_image, github read, search and publish, the session tools and the goal tools. The default deny covers browser, canvas, computer, mobile_ui, nodes, automations, gateway and every channel id.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/constants.ts#L20-L55
- Edge cases and failure handling: none read.
- Round 1: `confirmed` (a sandbox tool policy layer exists); the list is `new`.
- For rizzy-harness: n/a.
- Confidence: `read the definition`.

#### Browser sandbox: its own network, loopback-published ports, token-protected CDP, Chrome `--no-sandbox`
- What the code does: the browser container reuses `buildSandboxCreateArgs` with the browser image and network (default bridge `openclaw-sandbox-browser`, so it has egress). It publishes CDP and noVNC on random `127.0.0.1` host ports, sets a 24-byte random CDP auth token (sent as Basic auth) and a generated noVNC password, and sets `OPENCLAW_BROWSER_NO_SANDBOX=1`. The browser is disabled by default (`enabled: false`), and `allowHostControl` defaults to false.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/browser.ts#L362-L411 ; network override (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L58-L72); defaults (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L127-L149)
- Edge cases and failure handling: a CDP endpoint that does not answer within the autostart timeout (12 s) gets the container force-removed (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/browser.ts#L490-L497).
- Round 1: `new (not documented)`.
- For rizzy-harness: we have no browser sandbox. If we add one, the loopback port plus per-container token pattern is cheap.
- Confidence: `read the definition`.

#### SSH sandbox back end defaults to strict host keys
- What the code does: the SSH back end uses the `ssh` binary with `strictHostKeyChecking` and `updateHostKeys` true by default. Identity, certificate and known-hosts can be files or SecretRef data, and the remote root must be an absolute POSIX path (default `/tmp/openclaw-sandboxes`).
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L162-L198
- Edge cases and failure handling: SSH child processes get `sanitizeEnvVars(process.env)` (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/ssh.ts#L149).
- Round 1: `new (not documented)`.
- For rizzy-harness: n/a (we cut SSH).
- Confidence: `read the definition`.

### Running commands somewhere else

#### Node role: a closed method set enforced before scope checks
- What the code does: `connect.role` is parsed into `operator` or `node`; anything else is rejected. `isRoleAuthorizedForMethod` lets a node call only methods whose descriptor scope is `node` (`node.invoke.result`, `node.invoke.progress`, `node.event`, `node.pending.*`, `node.pluginTools.update`, `node.skills.update`, `skills.bins` and a few more), and denies operators those methods. Nodes skip operator scope checks entirely.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/role-policy.ts#L5-L24 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server-methods/method-authorization.ts#L44-L55 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/methods/core-descriptors.ts#L288-L300
- Edge cases and failure handling: pre-connect frames and `health` pass through without role checks (L39-L43 of method-authorization).
- Round 1: `new (not documented)`.
- For rizzy-harness: the same shape as our protocol split; worth a test that a node frame for an operator method is rejected.
- Confidence: `read the definition`.

#### Node command policy at `node.invoke`: allow-list plus declared set
- What the code does: `isNodeCommandAllowed` requires that the command is not private, is in the Gateway's resolved allow-list, and is in the node's declared command list. A node that declares no commands is refused.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-command-policy.ts#L460-L480
- Edge cases and failure handling: for `system.run`, the node re-evaluates its own policy as described above.
- Round 1: `confirmed`.
- For rizzy-harness: n/a.
- Confidence: `read the definition`. (checked in review)

#### Cloud worker RPC: a closed dispatcher with a fixed method list, feature gating and per-call fencing
- What the code does: the worker's WebSocket goes to `dispatchWorkerRequest`, which "never calls the generic gateway method registry". It handles only `worker.heartbeat`, `worker.transcript.commit`, `worker.live-event`, `worker.computer`, `worker.exec.approval`, `worker.gatewayTool.invoke` and `.cancel`, `worker.inference.start` and `.cancel`, and `worker.turn.admit`. Each method also needs its protocol feature in the worker's admitted identity, and its params must pass a TypeBox validator. Any other method rejects with `method-not-allowed` and closes the socket with code 1008. Before every request `validateWorkerConnection` re-checks the stored credential hash (constant-time), expiry, environment state and `ownerEpoch`, so a rotated credential or a newer owner fences a stale socket on its next frame. The expiry part has an exception: for an exact durable turn (`placement === "durable"`) a `credential-expired` result is ignored, so a worker holding a durable turn is not cut off at the 10-minute TTL (corrected in review; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/worker-turn-rpc.ts#L691-L716).
- Trace: method list (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/packages/gateway-protocol/src/schema/worker-admission.ts#L58-L66); dispatcher (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection-dispatch.ts#L90-L149 , https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection-dispatch.ts#L262-L265); per-RPC check (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L217)
- Edge cases and failure handling: the heartbeat echoes `ownerEpoch` and may refresh the GitHub binding, then re-validates ownership (L270-L303 of the dispatcher).
- Round 1: `confirmed` (closed method list, fencing).
- For rizzy-harness: per-frame revalidation of the credential and epoch is cheap and is exactly what our node link needs after re-enrolment.
- Confidence: `traced end to end` (corrected in review: per-frame expiry is waived for an exact durable turn; hash comparison, environment state and owner-epoch checks on every request, the closed method list and the 1008 close were checked)

#### Worker admission: a hashed, domain-separated credential bound to environment, session, epoch, bundle and version
- What the code does: a worker credential is 32 random bytes. Only `sha256(domain | [turn-claim |] credential)` is stored, with a 10-minute TTL, and the plaintext is registered for log redaction. Admission refuses the connection on any of these:
  - hash mismatch, or a credential for another environment;
  - session or run differing from the durable turn claim;
  - expired credential;
  - environment not ready;
  - bundle hash differing from the bootstrap receipt and the expected build;
  - version mismatch;
  - `ownerEpoch` differing from the credential's or the environment's;
  - RPC set version or protocol features differing.

  The worker dials the Gateway over WebSocket (`new WebSocket(target.url)`) for the protocol link, so no listening port on the worker is needed for it. The original text added "the Gateway never dials it"; that is wrong for provisioning: the Gateway opens outbound SSH to the worker box for bootstrap, tunnel control and workspace transfer, with pinned options (`-F none`, `StrictHostKeyChecking=yes`, `IdentityAgent=none`, no agent or X11 forwarding; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/ssh.ts#L259-L291).
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential.ts#L9-L60 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L79-L186 ; the worker client (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-admission.ts#L72-L83)
- Edge cases and failure handling: the worker checks the Gateway's hello against what it asked for (environment, session, epoch, RPC set, features) (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-admission.ts#L56-L66).
- Round 1: `confirmed` (per-dispatch credential stored as a hash, fencing). Outbound-only is confirmed in code: the worker is the WebSocket client.
- For rizzy-harness: copy the domain-separated hash and the "bundle hash must match the bootstrap receipt" check.
- Confidence: `traced end to end` (corrected in review: the Gateway does connect to the worker box over SSH; the credential hash, TTL, admission order and hello check were checked)

### Secrets

#### Inference is proxied by model reference; the worker never receives the key
- What the code does: `worker.inference.start` carries `modelRef` plus context. The Gateway rejects a model ref that differs from the approved one, and any `provider/model@profile` suffix (so a worker cannot pick an auth profile). It refuses a session mismatch or `runEpoch != ownerEpoch`. It then streams with `prepared.auth.apiKey`, held in the Gateway process.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-runtime.ts#L122-L153 ; key use (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-runtime.ts#L180-L194)
- Edge cases and failure handling: an invalid context returns `invalid-context`; the inference error reasons include `model-not-approved` and `epoch-mismatch`.
- Round 1: `confirmed`.
- For rizzy-harness: this is the credentials-on-control-side option. It conflicts with "the node decides" only if the master also picks the model; a node could still proxy through the master for keys.
- Confidence: `read the definition`. (checked in review)

#### Host exec inherits the Gateway's environment minus injection variables, not minus secrets
- What the code does: on Gateway or node hosts, `sanitizeHostExecEnvWithDiagnostics` copies `process.env` and drops:
  - about 110 "blocked everywhere" keys such as `NODE_OPTIONS`, `PYTHONPATH`, `BASH_ENV`, `GIT_*` executables, and the `LD_`, `DYLD_` and `BASH_FUNC_` prefixes;
  - node-service identity variables;
  - the env names the Gateway reads its own credentials from via `${NAME}` references.

  Only the hooks token and the database engine register those names. A generic secret-shaped name such as `ANTHROPIC_API_KEY` in the Gateway's environment is therefore inherited by `exec` children. Request overrides of `PATH` and other dangerous keys are rejected outright.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/host-env-security.ts#L38-L65 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/host-env-security.ts#L277-L309 ; base env is `process.env` (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-request-preparation.ts#L445) and the host path (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-request-preparation.ts#L496-L503); re-strip at launch (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-spawn.ts#L63-L70)
- Edge cases and failure handling: `setGatewayCredentialEnvKeys` callers are `src/gateway/hooks.ts` and `src/gateway/server-database-engine.ts` only (found by search). Sandbox runs get none of the inherited env: only config-declared vars, value-checked (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/sanitize-env-vars.ts#L113-L118). SSH children get `sanitizeEnvVars`, which blocks `*_API_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD` and known provider names (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/sanitize-env-vars.ts#L15-L31 , https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/secret-env-name.ts#L1-L2).
- Round 1: `confirmed`. Round 1 says plaintext in config or `.env` stays readable by the agent; the code adds that env-held provider keys reach host exec.
- For rizzy-harness: our shell already clears env. Keep the clear-then-allow approach rather than OpenClaw's inherit-then-deny for injection keys.
- Confidence: `traced end to end`. (checked in review)

#### Secret egress proxy: sentinels in the child, real values only at TLS egress to allowed hosts
- What the code does: a granted exec gets `HTTPS_PROXY`/`HTTP_PROXY` pointing at `http://openclaw:<token>@127.0.0.1:<port>` plus a local CA bundle (`NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`, `CURL_CA_BUNDLE`, `REQUESTS_CA_BUNDLE`, `GIT_SSL_CAINFO`). Each secret is a binding of name, sentinel and `allowedHosts`. The proxy rewrites sentinels in request bodies across chunk boundaries and refuses unresolved sentinels or hosts not on the binding's list. The per-process grant is revoked on cancel or exit.
- Trace: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/egress-proxy/environment.ts#L3-L14 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/egress-proxy/proxy-server.ts#L58-L67 ; https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/egress-proxy/registry.ts#L40-L47 ; refusals (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/egress-proxy/stream-substitution.ts#L26-L31); launch and revoke (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-runtime.ts#L593-L631)
- Edge cases and failure handling: durable exec, sandbox exec and PTY exec refuse secret-egress grants (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-runtime.ts#L588-L592). A process that ignores the proxy env bypasses it; this is cooperative, not enforced (inferred: no network namespace was found on this path).
- Round 1: `confirmed` (an egress proxy with sentinels).
- For rizzy-harness: this is the node-local credential broker design that keeps keys away from the agent without moving authority to the master. Pair it with a no-network sandbox so the proxy is the only way out.
- Confidence: `read the definition`. (checked in review)

### Hooks and trust prompts

- Plugin `before_tool_call` hooks fail closed (above). Plugin install trust, workspace-origin plugins and the ClawHub policy are in the sessions cluster; they were not read here.

## 4. Round 1 claims checked

| Claim | Verdict | Citation |
|---|---|---|
| Defaults: `security` full on gateway/node, deny on sandbox; `ask` off; `askFallback` deny | partly (changed in review): confirmed for host values; the sandbox `deny` default does not block sandboxed exec | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-defaults.ts#L171-L188 |
| Approvals can only tighten config | confirmed (except admin-only `full` session mode) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-defaults.ts#L195-L203 |
| Approval bound to argv, cwd, env (when present), pinned executable | corrected: also agentId, sessionKey, nodeId, requester device; env is a hash of overrides only | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L72-L90 |
| Content hash when the executable is writable | corrected: hashed when the file, its directory or a symlink component is writable or owned by the user | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-mutable-file-policy.ts#L19-L66 |
| Resolution re-checked before launch; drift denies | confirmed (a small hash-to-exec window remains) | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L550-L620 |
| Allow-list entries are globs on resolved paths; allow-always binds exact argv and cwd | corrected: realpath globs, basename only for PATH lookups; allow-always is a SHA-256 of cwd and argv | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-command-resolution.ts#L222-L231 |
| `awk`, `sed`, `jq` always denied as safe bins; `/bin`, `/usr/bin` trusted | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/exec-safe-bin-semantics.ts#L10-L18 |
| Auto mode: login or interactive shells skip the reviewer | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L631-L643 |
| Three consecutive denials escalate to human | corrected: Gateway host only, an in-memory LRU of 256 sessions, reset only by an allow | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/bash-tools.exec-host-gateway.ts#L96-L110 |
| Session modes read-only, guarded, workspace (reviewer), full | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/session-permission-exec-mode.ts#L13-L18 |
| Sandbox off by default; modes and workspaceAccess values | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/config.ts#L212-L227 |
| Node command runs only if declared, approved and allow-listed; gate on the Gateway | corrected: the Gateway gate exists, and the node also re-evaluates `system.run` locally, though it trusts a Gateway approval over its own allow-list | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117 |
| Cloud worker: closed method list, cannot call operator RPCs | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/server/ws-connection/worker-connection-dispatch.ts#L90-L121 |
| Per-dispatch credential stored as hash; owner-epoch fencing | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L217 |
| Worker connection is outbound (Round 1: "not stated") | partly (changed in review): the protocol link is outbound from the worker, but the Gateway also SSHes into the worker box for bootstrap and workspace sync | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-admission.ts#L72-L83 |
| Provider keys stay on the Gateway; inference proxied by model ref | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/inference-runtime.ts#L147-L153 |
| Chat approval UIs check approver lists | corrected: an empty list authorizes same-chat senders | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/approval-auth-helpers.ts#L78-L81 |
| Egress proxy with sentinels injected at egress | confirmed | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/secrets/egress-proxy/environment.ts#L3-L14 |
| Loop detection thresholds (unverified in Round 1) | settled: 10, 20, 30, unknown-tool 10; rolling detection off unless enabled | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-detection.ts#L56-L59 |

## 5. In the code but not in the documentation

- The reviewer is pre-screened by a regex for reviewer-directed text, and a hit is an automatic `deny` with risk high before any model call. The reviewer's JSON is checked for duplicate keys and `__proto__` (`src/agents/exec-auto-reviewer.ts` L152-L191, L271-L288).
- Executables are pinned by rewriting the command text to real paths, and arguments containing shell expansion make that impossible, which forces review (`src/infra/exec-authorization-render.ts` L144-L191).
- Cwd identity is bound by inode, and symlinked cwds are refused (`src/infra/system-run-cwd-binding.ts`).
- A node refuses a delayed approval if its own approvals-file policy snapshot changed in the meantime (`src/node-host/invoke-system-run.ts` L349-L357).
- The node-hosted reviewer runs on the node with the node's own model config and no transcript.
- The core agent loop has no step cap, and rolling loop detection is off by default.
- Host exec inherits the Gateway's `process.env`; secret names are scrubbed only for the sandbox and SSH paths.
- Docker create flags are versioned by an epoch label that forces recreation when hardening changes.
- The browser sandbox publishes CDP and noVNC on loopback only, with per-container random credentials; Chrome runs with its own sandbox disabled inside the container.

## 6. Could not find or could not settle

- **Executable pinning on nodes when the Gateway renders the command.** The Gateway rewrites the command text; the node re-resolves with `prepareSystemRunExecutableIdentityBinding`. I did not trace whether the node receives the rewritten absolute-path command or the original. Settle by reading `src/gateway/node-invoke-system-run-approval.ts` `resolveForwardedRawCommand` (L183-L195) and `src/agents/bash-tools.exec-host-node.ts`.
- **Which model the reviewer uses when `tools.exec.reviewer.model` is unset.** `prepareModel` is called with `modelRef: undefined`; settle in `src/agents/simple-completion-runtime.ts`.
- **Per-channel handling of the empty-approver fallback.** Only the shared SDK helper was read; settle by reading one channel's native approval handler (for example `extensions/telegram`).
- **Where auth profiles (provider keys) live on disk and whether host exec can read them.** Not found under the names searched (`auth-profiles.json`); settle via `src/agents/agent-auth-credentials.ts`.
- **`exec.approval.resolve` authority for operator clients** (the `operator.approvals` scope, the requester-only rule) was not traced beyond its descriptor (`src/gateway/methods/core-descriptors.ts` L60).
- **Node pairing and SSH-verified auto-approval (question 19)** belong to the sessions cluster and were not read.
- **OpenShell and Crabbox back ends' isolation flags** were not read.

## 7. Citation check

`research/round2/openclaw.safety.md: 163 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit`

## 8. Review

Checked 26 of the 34 findings against the pinned clone (reading the cited lines and following at least one hop). Verdicts: 20 supported (including one absence confirmed), 6 partly (corrected in the text above), 0 wrong, 0 not supported.

| Finding | Verdict | Reason |
|---|---|---|
| The core loop has no step limit | absence confirmed | searched `maxTurns`, `maxIterations`, `maxToolCalls`, `maxSteps` in `packages/agent-core` and `src/agents`: only a headless code-mode budget and a per-invocation tool budget used by code mode |
| `before_tool_call` hooks fail closed | supported | the catch returns `blocked: true` with the failure reason |
| Loop detection thresholds, off by default | supported | constants 30/10/20/30 and the `enabled !== true` early return confirmed |
| Real defaults | partly | host defaults right; the sandbox `deny` default does not block sandboxed exec (`bash-tools.exec-run.ts` L313-L316) |
| `full` with `on-miss` never prompts | supported | `requiresExecApproval` only prompts for `allowlist` on a miss |
| Session permission modes | supported | map and intersection function match |
| tree-sitter-bash limits | supported | 128 KiB and 500 ms constants |
| Allow-list matching | partly | the allow-always hash covers cwd and `argv[1..]`, the executable is the entry's path pattern |
| Safe bins | supported | awk family, sed, jq refused by name |
| What an approval is bound to | partly | env hash, mutable-file hash rule, real-path pinning, inode on cwd confirmed; only mutable symlink components are refused for cwd, not all |
| Re-check at launch and TOCTOU | supported | `readFileSync` by path, no fd held |
| Gateway-side guard on `system.run` | supported | flags stripped, node and device binding, binding match, `consumeAllowOnce` |
| Empty approver list | supported | returns `authorized: true` tagged as same-chat fallback |
| Node enforces own policy; Gateway approval satisfies allow-list | supported | `approvedByAsk` satisfies ask and allowlist miss; only `security=deny` is final |
| Auto reviewer | partly | 1024 tokens is raised for reasoning models; temperature 0, 30 s, 16,000 chars, directive heuristic confirmed |
| Three-denial escalation | supported | LRU of 256, cap 3, reset only by allow-once, no counter on the node path |
| Reviewer on a node | supported | node `ask` becomes a denial with deferred text |
| Sandbox off by default | supported | `mode ?? "off"`, `workspaceAccess ?? "none"` |
| `docker create` invocation | supported | flags and defaults match `buildSandboxCreateArgs` and `config.ts` |
| Bind validation | supported | blocked path lists and covers/targets rule match |
| Node command policy at `node.invoke` | supported | `isNodeCommandAllowed` |
| Cloud worker RPC | partly | closed list, feature gate, per-frame hash and epoch checks confirmed; per-frame expiry is waived for an exact durable turn |
| Worker admission | partly | hash, TTL, ordering confirmed; "the Gateway never dials it" is wrong: the Gateway SSHes into the worker box for bootstrap and workspace sync |
| Inference by model reference | supported | model ref, profile suffix, session and epoch checks; key from `prepared.auth.apiKey` |
| Host exec inherits the Gateway env | supported | 110 blocked-everywhere keys; only the hooks token and database engine register credential names; `ANTHROPIC_API_KEY` is not filtered |
| Secret egress proxy | supported | env map and durable/sandbox/PTY refusal confirmed (the env also sets `NODE_USE_ENV_PROXY=1`) |

Answers to the specific questions in the review request, for the reader:

- Exec approval binding: argv, cwd, agentId, sessionKey and a hash of request env overrides only (not the inherited environment); the executable is resolved to a real path without cache, and hashed (SHA-256) only when the file, its directory or a symlink component is writable or owned by the running process, otherwise only the real path is stored. Confirmed.
- A Gateway approval overrides a node's own allow-list; only a local `security=deny` holds. Confirmed (`exec-policy.ts` L60-L117), with the node still refusing approvals that lack a matching plan or whose policy snapshot changed.
- Host exec inherits the Gateway's environment including provider keys. Confirmed.
- Worker per-frame re-check: credential hash, expiry (except an exact durable turn), environment state and owner epoch. Confirmed with the durable-turn exception noted above.

Not checked (8 findings): tool-result caps, heredoc/inline-eval rules, reviewer skip conditions, cron standing grants, sandbox tool policy, browser sandbox, SSH sandbox, node role policy. Not settled by this review: the open items in section 6 (node-side pinning of the rewritten command, reviewer default model, per-channel approver fallback). Overall: the checked mechanisms matched the code closely and the errors found were about overstated scope, not invented behaviour; the unchecked remainder can be trusted at roughly the same level, but treat any categorical "never" or "only" in it as needing a second look. The citation check was re-run after editing.
