## 14. Recommended focus for Round 2

Round 2 reads the source code of the open-source harnesses. The ten synthesis files list well over a hundred questions that only code can settle (collected in `round1/synth/_parts/round2.md`). These twenty are the ones whose answers change a decision we face now. Each names the repository, the feature, what exactly to find out, and why it matters. The number in brackets is the overall rank.

Licence note: Zed (GPL and Apache files) and Warp (AGPL client) may be read for ideas only. Copying code from any repository needs a permissive licence and an update to `THIRD_PARTY_NOTICES.md`.

### Sandbox design

- **[1] Codex, Linux sandbox** (https://github.com/openai/codex). Find the bubblewrap argument list and seccomp filter per sandbox mode; whether network is actually off by default under `workspace-write` (not confirmed in review); and how the Unix-socket isolation that disqualified Landlock is done. Why: our gate3 Landlock sandbox cannot block pathname Unix sockets (Docker socket, D-Bus) before Linux 7.1 and does not cover UDP or DNS. This decides whether we add a seccomp or namespace layer for those gaps or accept them with a per-node warning.
- **[8] DeepSeek harness, sandbox enforcement reporting** (https://github.com/deepseek-ai/deepseek-harness). Find how `partial` enforcement is decided per Landlock ABI, and the default sandbox mode and preset at startup. Why: it is the model for reporting `full` or `partial` per node, which would make our socket gap visible to the operator.

### Approval binding

- **[2] OpenClaw, exec approval binding** (https://github.com/openclaw/openclaw). Find how the executable content hash and the resolved-path re-check are implemented, the time-of-check to time-of-use window between check and launch, and how `/approve <id>` from a chat channel is bound to the approver list and the exact command so a replayed approval cannot cover a different call. Why: this is the binding we intend to copy to replace "per tool name per turn".
- **[13] OpenCode, shell scanner and "always"** (https://github.com/anomalyco/opencode). Find which parser runs per shell, how `cd` targets are inferred, why unparseable input is refused, and how "always" derives the saved patterns from a command. Why: if we add per-command rules, this is the converged design to match.
- **[19] Codex, execpolicy** (https://github.com/openai/codex). Find what the tree-sitter splitter treats as "plain linear", how an opaque invocation is matched (exact string or argv), and how `match`/`not_match` tests run at load. Why: rules with inline tests are on the "worth copying" list.

### Credentials on workers

- **[3] Hermes Agent, iron-proxy integration** (https://github.com/NousResearch/hermes-agent). Find how opaque tokens map to real keys, what happens to Hermes' own model calls, and why container backends skip dangerous-command approval in code. Why: a node-local credential proxy is the one option that keeps keys off the agent's reach without moving authority to the master.
- **[7] OpenClaw, cloud workers** (https://github.com/openclaw/openclaw). Find the worker route's method allowlist, how inference is proxied by model reference, how owner-epoch fencing rejects a stale worker, and the actual transport and direction of the worker connection (the docs do not say outbound-only). Why: it is the reference for the other option, credentials on the control side, and for fencing a node that returns with stale state.

### The control and execution link

- **[4] OpenClaw, where node command policy is enforced** (https://github.com/openclaw/openclaw). Find whether the check runs on the Gateway before `node.invoke`, on the node on receipt, or both. Why: OpenClaw is the closest topology to ours; if its node also enforces, it is the only peer with any execution-side authority, and its code shows how that is tested.
- **[11] Codex, App Server** (https://github.com/openai/codex). Find how WebSocket authentication is enforced before `initialize`, what the non-loopback "unauthenticated during rollout" default is in code, and the queue sizes behind `-32001`. Why: we need an explicit overload reply on the bounded gate3 link and a clean auth order for any future remote client.
- **[20] OpenCode, durable session log** (https://github.com/anomalyco/opencode). Find the contract behind `GET /api/experimental/session/{id}/log`: exclusive sequence, ordering, retention, and how it relates to the volatile event stream. Why: the closest public analogue to our fold-over-log UI, and we have no log lifecycle yet.

### Checkpoints at scale

- **[6] Zed, the removed checkpoint code** (https://github.com/zed-industries/zed). Find which git operations it ran, how often, on what repository sizes, and what made it stall. Why: our shadow-repo checkpoints have a count cap only; this predicts the failure we will hit.
- **[12] Hermes Agent and Cline, shadow git** (https://github.com/NousResearch/hermes-agent, https://github.com/cline/cline). For Hermes: the destructive-command detection list, the one-per-directory-per-turn rule, how nested repos become gitlinks, how user edits since the snapshot are detected and skipped, and how the size caps are enforced. For Cline: location, exclusion rules, size handling and the cost of a commit per tool use. Why: the two other shadow-git designs; their limits are the ones we lack.

### Compaction

- **[5] Pi, compaction and `context_edit`** (https://github.com/earendil-works/pi). Find how cut points and split-turn summaries are chosen, how compact-and-retry on overflow is implemented, and how `context_edit` entries are created and folded. Why: Pi is the model for our overflow-aware compaction, and `context_edit` maps onto a fold over our event log.
- **[16] Hermes Agent, compression** (https://github.com/NousResearch/hermes-agent). Find the real behaviour when the summary model's window is smaller than the main model's, whether compression is in place or creates a child session (the docs disagree), and whether a manual compress command exists. Why: the failure mode a summariser slot must guard against.

### Sub-agent context and isolation

- **[10] OpenHands, approvals for ACP-hosted agents** (https://github.com/OpenHands/OpenHands). Find how permission requests from Claude Code or Codex running as ACP backends reach the Canvas user, and what is auto-approved. Why: no host documents this, and it is the part we would have to design before hosting other harnesses through `AgentBackend` without breaking "the node decides".
- **[17] DeepSeek harness, sub-agent inheritance** (https://github.com/deepseek-ai/deepseek-harness). Find whether a sub-agent inherits the parent's sandbox mode and approval policy (the docs are silent), and how the hidden `maxTotalAgents` cap is enforced. Why: our children inherit the approval mode; we need to know how others stop a child from widening it.

### Model slots

- **[15] Hermes Agent, auxiliary models** (https://github.com/NousResearch/hermes-agent). Find which model each auxiliary task uses when unset, and whether auxiliary fallback chains share cooldown state with the primary chain. Compare Zed's `thread_summary_model` and `compaction_model` defaults (https://github.com/zed-industries/zed). Why: settles "default to the main model" and "one chain per slot" for our three planned slots.

### Memory

- **[14] Hermes Agent, memory write approval** (https://github.com/NousResearch/hermes-agent). Find how `write_approval` staging, the frozen snapshot and the injection scan are enforced, and how the curator lock blocks agent writes, including writes from the terminal tool. Why: the only open implementation of a human gate on agent-written memory; it tells us whether the gate can be bypassed through the shell.

### MCP and tool deferral

- **[18] Pi, code mode and tool search defaults** (https://github.com/earendil-works/pi). Find whether `builtin:codemode` and `builtin:tool-search` load by default in the CLI (three docs pages disagree), and which resources project trust gates in headless mode. Why: Pi's exposure modes are the systematic answer to MCP context cost, and its trust gate is the model for repo-shipped servers.

### Hooks

- **[9] Codex, hook trust by hash** (https://github.com/openai/codex). Find what exactly is hashed (command string, script bytes, plugin directory), where the trust record lives, and whether a plugin update re-triggers review. Why: node-side hooks trusted by hash are the shape we would adopt; the details decide whether a repository can swap a script under a reviewed hook.

### Closed: documentation only

These cannot be covered by reading source code. Round 2 can only re-read their docs or, where a CLI prints its own defaults, run it.

| Harness or part | Status |
|---|---|
| Claude Code | Closed; the public repository holds the changelog and plugins only. Some defaults (the read-only Bash allowlist, the environment scrub list, auto-mode rules) can be printed by running the CLI, for example `claude auto-mode defaults`. |
| Claude Agent SDK, TypeScript | "All rights reserved". The Python SDK is MIT but drives the closed binary. |
| Claude Desktop | Closed; no repository. |
| Cursor | Closed; the public repository has no licence and no code. |
| Antigravity | Closed core; the CLI repository is distribution only; the Python SDK is Apache-2.0 but depends on a compiled runtime binary. |
| Codex IDE extension, Codex Cloud, ChatGPT desktop app | Closed; the CLI, SDK and App Server are open. |
| Warp control plane, cloud agents, Factories backend | Not described as open anywhere; the client is AGPL (ideas only); worker and SDK licences were not checked. |
| OpenHands Cloud and Enterprise | Commercial; whether the Enterprise server code is public was not found. |
