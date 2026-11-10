Every item from section 3 "Distinctive or unusual" of all 17 survey files (179 items), grouped by theme. "Why it matters" is one line for a self-hosted master/node harness, taken from the surveys' own relevance reasoning where they gave one. Evidence is the URL of the matching section 2 entry (section 3 lines mostly carry no URL). Check: `reviewed` (the matching entry was confirmed in review), `corrected` (corrected or weakened in review, the correction is in the "Why" cell), `unreviewed`, `unverified`. Items a review corrected or weakened are marked **[corrected]** or **[weakened]**.

##### A. Approval, permissions and policy

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| Codex | Reviewer agent replaces the human approver, with circuit breaker (3 consecutive or 10-of-50 denials) and a one-shot `/approve` override | Shows cost and failure mode (fail closed) of an automated approver that could run on the node | https://learn.chatgpt.com/docs/sandboxing/auto-review.md | reviewed |
| Codex | Command rules with inline `match`/`not_match` unit tests; tree-sitter splitting of `bash -lc` only when provably linear | Testable allow-lists; opaque scripts judged as one command | https://learn.chatgpt.com/docs/agent-configuration/rules.md | reviewed |
| Codex | Asynchronous safety monitor that can pause a task after the fact | A second control separate from per-action approval | https://learn.chatgpt.com/docs/agent-approvals-security.md | reviewed |
| OpenCodex | Preview-then-confirm management API with plan fingerprints and consent-bound actions admin tokens cannot perform | Pattern for sensitive dashboard actions | https://opencodex.me/reference/management-api/ | reviewed |
| OpenCode | Flat ordered permission list (last match wins) plus admin-only `policies` layer that only tightens; a policy allow can lift an earlier policy deny **[corrected]** | Close to "node decides"; correction: not purely deny | https://opencode.ai/docs/permissions/, https://opencode.ai/docs/policies/ | corrected |
| OpenCode | Built-in multi-shell scanner (Bash, Zsh, sh, PowerShell) checking every extracted subcommand, refusing unparseable input | Allow-list robust against shell tricks | https://opencode.ai/docs/permissions/ | reviewed |
| Claude Desktop | Per-tool decision source in telemetry (`config`, `hook`, `user_permanent`, `user_temporary`, ...) | Good field for event-log approval records | https://claude.com/docs/cowork/monitoring.md | reviewed |
| DeepSeek harness | Approval fail-closed with `approval/asked` and `approval/decided` events; `never` enforced before any answerer | Matches log-as-truth and audit trail | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval | reviewed |
| DeepSeek harness | Timed `ask_user_question` whose answer can arrive as a later turn, even after a host restart | Approval/question survives restarts | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/user-questions | unreviewed |
| Cursor | LLM classifier on the vendor backend approves or blocks commands, tuned by plain-English instructions | Sends command context to a vendor backend: a leak path unless the classifier is local | https://cursor.com/docs/agent/security/run-modes | reviewed |
| Antigravity | Command-permission parser disables prefix matching on shell constructs that hide execution (`$(...)`, `git -c`, `tar --to-command`) | Copy exactly as a requirement for any allow-list | https://antigravity.google/docs/permissions | reviewed |
| Antigravity | Deny > Ask > Allow with allow rules feeding sandbox mounts and network allowlist **[corrected]** | One policy drives prompts and isolation; true in App docs, CLI docs do not spell it out | https://antigravity.google/docs/sandbox | corrected |
| Antigravity | Out-of-sandbox command evaluation ("Auto") instead of a binary approve/deny **[weakened]** | Preview label and business-account unavailability could not be confirmed | https://antigravity.google/docs/permissions | corrected |
| OpenHands | Per-conversation LLM-rated risk inline in the tool schema, pluggable analyzers, separate confirmation policy | Separates risk analysis from policy; analyzers are not a sandbox | https://docs.openhands.dev (SDK `security.md` per section 3) | reviewed |
| OpenClaw | LLM-reviewer exec mode (`mode=auto`) with human escalation after three denials | Middle path between prompting and full trust | https://docs.openclaw.ai/gateway/permission-modes | reviewed |
| OpenClaw | Approvals bound to argv, cwd, env and pinned executable path (content hash if writable), re-checked before launch | Stronger than name-based allow-lists; copy the binding | https://docs.openclaw.ai/tools/exec-approvals | reviewed |
| goose | Adversary mode: LLM reviewer of each shell call, rules in a plain-language file; fails open | Cheap second opinion but fails open | https://goose-docs.ai/docs/guides/security/adversary-mode.md | reviewed |
| goose | Extension allowlist with exact command match, delivered by an admin-hosted URL | Maps onto a node-side policy the master cannot widen | https://goose-docs.ai/docs/guides/allowlist.md | reviewed |
| Hermes Agent | Layered approvals: smart LLM approver, fail-closed timeouts, hardline floor YOLO cannot lift, deny globs before YOLO, headless default deny | Ready checklist for the approval path | /user-guide/security | reviewed |
| Hermes Agent | Container backends skip dangerous-command approval and disable checkpoints **[corrected]** | A design trap: backend-based skipping should be node policy; stated on the security page, not the tools page | /user-guide/security | corrected |
| Zed/ACP | Per-tool regex permission lists with chain splitting and a tiny hardcoded deny set | Policy without claiming unattainable safety | https://zed.dev/docs/ai/tool-permissions.md | reviewed |
| Zed/ACP | Permission "always" semantics left to the agent in ACP | The protocol standardises the prompt, not the policy store | https://agentclientprotocol.com/protocol/v1/prompt-turn.md (nearest entry) | reviewed |
| Warp | Regex denylist overrides "Always allow", but auto-approve bypasses the user denylist by default **[corrected]** | Applies to the explicit run-until-completion shortcut and is switchable; admin denylist is non-bypassable | https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ | corrected |
| Cline | Model-judged `requires_approval` per command instead of a rule list | Weak default; hooks as enforcement layer is the better shape | https://docs.cline.bot/features/auto-approve.md | reviewed |
| Claude Code | Auto mode: second model as approver, prose rule tiers, tool results stripped from classifier input, breaker after 3 consecutive or 20 total blocks | Prompt-injection-aware approver design | https://code.claude.com/docs/en/permission-modes.md | reviewed |
| Claude Code | Hook `defer`: a PreToolUse hook ends the turn with the call pending; SDK `canUseTool` answers out of band and redelivers after reconnect | Approver on another device with a dropped link: our master/node path | https://code.claude.com/docs/en/hooks.md, https://code.claude.com/docs/en/agent-sdk/hooks.md | unreviewed |
| Claude Code | One audit stream: `claude_code.safety_event` for every action a rule, classifier, hook or sandbox stopped | Approval decisions as events the UI folds over | https://code.claude.com/docs/en/changelog.md (nearest entry: OTel additions) | unreviewed |

##### B. Sandbox, secrets and credential handling

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| Codex | Placeholder substitution for cloud network secrets (egress proxy swaps the real credential only for allowlisted HTTPS hosts) | Fits "no secret in an event" | https://learn.chatgpt.com/docs/environments/cloud-environments.md | reviewed |
| Codex | Protected subpaths (`.git`, `.agents`, `.codex`, `.aws`, resolved gitdir) inside writable roots | Stops an agent rewriting its own config or Git hooks | https://learn.chatgpt.com/docs/agent-approvals-security.md | reviewed |
| Claude Desktop | Cowork VM with QEMU/KVM on Linux and folder-attach trust where only the user attaches network drives **[corrected]** | Both a local-VM and a cloud Cowork exist; the single-location claim was wrong | https://code.claude.com/docs/en/desktop-linux | corrected |
| DeepSeek harness | Spill files created 0600 with exclusive creation to defeat symlink planting | Small file-handling safeguard | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/spill | unreviewed |
| DeepSeek harness | Honest `full` vs `partial` sandbox enforcement status per backend | Copy the labels and a hard failure instead of silent passthrough | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/sandbox | reviewed |
| Oh My Pi | Auth broker plus gateway: credentials never reach untrusted clients **[corrected]** | Clients also get API keys; loopback default, no TLS | https://omp.sh/docs/secrets | corrected |
| Pi | Isolation patterns where the credential proxy stays outside the sandbox (Docker Sandboxes, OpenShell) | Keep provider keys off worker nodes | https://pi.dev/docs/latest/containerization | unreviewed |
| Cursor | Per-claim OIDC identity tokens: spawn hooks get a nonce-bound JWT naming agent, owner and worker ID | Node-scoped token minted per session | https://cursor.com/docs/cloud-agent/self-hosted/pool | reviewed |
| OpenHands | Per-conversation Docker containers: outer server proxies to an inner agent server with per-container keys and idle stop | Comparison point for a node that spawns sandboxes | `docker-execution.md` (https://docs.openhands.dev, path per survey) | reviewed |
| Hermes Agent | Credential-injecting egress proxy: sandboxes get opaque tokens, real keys stay on the host **[corrected]** | Bind is Docker bridge on Linux, loopback on Docker Desktop; limits verified | /user-guide/egress | corrected |
| Zed/ACP | Sandboxing scoped honestly: Seatbelt, Bubblewrap, Windows via WSL, with known side channels | Honest scope statement | https://zed.dev/docs/ai/sandboxing.md | reviewed |
| Zed/ACP | ACP `terminal` auth method: client runs the agent's login in a terminal, credentials do not pass through the protocol | Authentication without handling secrets | https://agentclientprotocol.com/protocol/v1/authentication.md | unreviewed |
| Warp | Short-lived OIDC identity tokens inside agent runs for AWS and GCP | Avoids storing cloud keys on nodes | https://docs.warp.dev/agents/cli/oz-cli/federate/ | unreviewed |
| Claude Code | Credential masking at the sandbox proxy (sentinel substitution) **[corrected]** | Needs experimental `tlsTerminate`; repo settings cannot set it; sandboxed Bash only; masked files unreadable on macOS; cloud sessions use no sentinel, the key is never in the VM | https://code.claude.com/docs/en/sandboxing.md, https://code.claude.com/docs/en/cloud-environments.md | corrected |
| Claude Code | Worktree hardening: neutralises repo-local filter drivers, `includeIf`, LFS transfer; refuses commands where git cannot be shown to stay in the worktree | Treats `.git/config` as attacker-writable | https://code.claude.com/docs/en/worktrees.md | unreviewed |
| Claude Code | Delegation hygiene: subagent reports are rewritten if they imitate harness control tags; subagents cannot escalate past the parent mode | Cheap prompt-injection layer across sub-agent boundaries | https://code.claude.com/docs/en/agent-sdk/subagents.md | unreviewed |

##### C. Config trust and repo-supplied inputs

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| Codex | Hooks trusted by hash; changed hook skipped until re-reviewed; admins can force managed-only hooks | Right shape for node hooks and plugins | https://learn.chatgpt.com/docs/hooks.md | corrected (page gives no GA label) |
| Codex | Project config cannot redirect credentials (provider, base URLs, `notify`, `otel`, profiles ignored with a warning) | Smallest copyable "repo cannot widen" rule | https://learn.chatgpt.com/docs/config-file/config-advanced.md | corrected |
| Pi | Project trust gate separate from tool permissions, honest about being incomplete | Per-project gating on a node | https://pi.dev/docs/latest/security | reviewed |
| Cursor | Bugbot reads its config from the base branch | A PR cannot loosen its own review | https://cursor.com/docs/bugbot | reviewed |
| Cursor | Hook merging across enterprise, team, project, user, plugin; deny > ask > allow, other fields last response wins **[corrected]** | Not "most restrictive wins" | https://cursor.com/docs/hooks | corrected |
| DeepSeek harness | Webhook-triggered read-only review sessions with PR fields marked untrusted in the prompt | Matches untrusted webhook runs | https://deepseek-harness.github.io/deepseek-harness/en/guide/github-review | reviewed |
| Claude Code | Strict-value-wins settings and fail-closed parsing: lower scope can tighten a managed value, never loosen it | Direct model for `node.toml` authority | https://code.claude.com/docs/en/settings.md | reviewed |
| Claude Code | REVIEW.md trust: review instructions only from the base branch, honoured according to branch protection | A PR author cannot soften their own review | https://code.claude.com/docs/en/code-review-customize.md | unreviewed |

##### D. Protocol, client/server split, remote and node execution

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| Codex | Every client uses one app-server protocol (JSON-RPC thread/turn/item, daemon, remote TUI) | Reference for the master-node link and UI fold; labelled experimental | https://learn.chatgpt.com/docs/app-server.md | reviewed |
| Codex | Chat and Git state move between hosts (Handoff) **[corrected]** | "to a worktree" not stated | https://learn.chatgpt.com/docs/remote-connections.md | corrected |
| OpenCodex | Hub/data-plane vs management-plane listener split with separate tokens | Separate node-facing and admin credentials | https://opencodex.me/guides/remote-hub/ | corrected |
| OpenCodex | Remote Workspace: agent on the hub, files and bubblewrap commands on the executor, outbound-only WebSocket **[corrected]** | Inverted from ours; experimental, read-only default, Linux-only bubblewrap | https://opencodex.me/guides/remote-workspace/ | corrected |
| OpenCode | Pairing: single-use 5-minute link plus optional end-to-end-encrypted tunnel from a phone | Web access without accounts | https://opencode.ai/docs/cli/web/ | reviewed |
| Claude Desktop | Three agent surfaces on one engine with moving boundaries (Chat/Cowork merge, Dispatch spawning Code sessions) | Scope-creep example for one binary | https://code.claude.com/docs/en/platforms | unreviewed |
| Claude Desktop | Dispatch: phone-to-desktop persistent thread that decides whether to spawn a coding session **[corrected]** | 30-minute computer-use window is from the Code docs page; limited beta | https://support.claude.com/en/articles/13947068, https://code.claude.com/docs/en/desktop | corrected |
| Claude Desktop | Admin delivery of the product to a customer's own inference provider (3P), bootstrap-server config delivery **[corrected]** | No GA label; unavailable-feature list from other pages | https://claude.com/docs/third-party/claude-desktop/overview.md | corrected |
| DeepSeek harness | Capability "seams": swapping one provider (e.g. SSH sandbox) changes bash, PTY and LSP together | Pluggable execution location | https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams | unreviewed |
| Oh My Pi | Collab with end-to-end sealed relay and view-only links | E2E conflicts with a master that must read the log: a design choice | https://omp.sh/docs/collab | reviewed |
| Pi | pi-durable plus pi-env: crash-surviving runtime with a Rust daemon on the remote host and local credentials **[corrected]** | Background-owned work survives parent abort | https://github.com/earendil-works/pi/blob/main/packages/env/README.md | corrected |
| Cursor | Self-hosted workers invert the split: loop and inference in vendor cloud, tools on customer machine over one outbound connection | Shows what a loop-at-master design leaks | https://cursor.com/docs/cloud-agent/self-hosted | reviewed |
| Cursor | Claim-then-spawn controller with single-claim session token, warm pool, hibernation | Ready-made scheduler design | https://cursor.com/docs/cloud-agent/self-hosted/pool | reviewed |
| Antigravity | Sidecars plus `agentapi` CLI so local scripts can create or message agent conversations | Local control surface for sessions | https://antigravity.google/docs/sidecars | reviewed |
| OpenHands | Canvas is a pure browser client connecting to any number of independent backends, each with its own settings and secrets | Client-side federation, not a master that owns state | `backends.md`, `setup.md` (https://docs.openhands.dev, path per survey) | reviewed |
| OpenHands | Agent Server exposes an OpenAI-compatible endpoint where the model name is an agent profile | Reuse by other clients | `openai-gateway.md` | reviewed |
| OpenHands | ACP adapter: runs Claude Code, Codex, Gemini, Pi, OpenCode inside the Agent Server with credentials resolved by secret name | Model for hosting other harnesses on nodes | `acp-agents.md` | reviewed |
| OpenHands | Deferred init: dormant warm pods activated by one `POST /api/init` with per-user config | Pre-warmed node sandboxes | `deferred-init.md` | reviewed |
| OpenClaw | Gateway-owned wake words and voice routing broadcast to all nodes | Voice as a Gateway feature | https://docs.openclaw.ai/nodes/voicewake | unreviewed |
| OpenClaw | Two-layer node pairing: device identity then approval of the declared command set **[corrected]** | Per-node token store retired Jan 2026, RPC methods removed 2026.7 | https://docs.openclaw.ai/nodes/command-policy | corrected |
| OpenClaw | Cloud-worker design: closed RPC method list, dials the Gateway, per-dispatch hashed credential **[corrected]** | "Outbound-only/WebSocket" not stated; status is beta, not GA | https://docs.openclaw.ai/gateway/cloud-workers/security-model | corrected |
| goose | Roaming: iroh/QUIC peer-to-peer ACP transport with key-exchange trust and revocation | NAT-free node connection; experimental, not in released binaries | https://goose-docs.ai/docs/guides/roaming-agents.md | reviewed |
| goose | Whole product is an ACP server (desktop, editors, custom UIs) **[corrected]** | GDK ACP page names Desktop, Top Goose, Berd, not Zed | https://goose-docs.ai/docs/gdk/acp/index.md | corrected |
| goose | GDK exposes the loop as a resumable state machine where the application owns state | Resumable loop design | https://goose-docs.ai/docs/gdk/concepts/state-machine.md | unreviewed |
| Hermes Agent | Gateway delivery ledger and durable per-session FIFO with a never-replayed "unknown" crashed-turn state **[corrected]** | Fits one-writer and restart mid-operation; rules are on the messaging page | /user-guide/messaging | corrected |
| Hermes Agent | Hermes Relay: gateway holds no platform credentials and dials out only | Credential-less connector | /user-guide/messaging/relay | reviewed |
| Warp | Local-to-cloud handoff with snapshot of uncommitted and untracked files plus cloud checkpoints **[corrected]** | "Blocked while command runs" not found | https://docs.warp.dev/platform/handoff/ | corrected |
| Cline | Hub/spoke with capability brokering: hub routes a request to the client that advertised it **[corrected]** | Hub does not run the loop; roles are per session participant | https://docs.cline.bot/sdk/architecture/hub-spoke.md | corrected |
| Cline | SSH remote by uploading a self-contained helper and tunneling a loopback hub; credential sent per session | Reference for node bootstrap | https://docs.cline.bot/usage/cline-desktop-ssh.md | reviewed |
| Claude Code | Pull-only execution with leases **[corrected]** | Work order is single-use per runner in orchestrator mode only; Remote Control has no documented lease | https://code.claude.com/docs/en/self-hosted-environments.md | corrected |

##### E. Session log, persistence and state

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| OpenCode | Server-side durable per-session event log with sequence numbers and `follow` tailing, separate from the volatile broadcast stream | Parallels "UI is a fold over the log" | https://opencode.ai/docs/api/ | reviewed |
| OpenCode | Snapshot store as a separate Git object DB with per-step before/after capture and attribution | Checklist of what undo does not cover | https://opencode.ai/docs/snapshots/ | reviewed |
| DeepSeek harness | Strict log discipline: policy and mode changes are log events (`sandbox/mode`, `approval/policy`, `permission/preset`) | Matches our invariant | https://deepseek-harness.github.io/deepseek-harness/en/reference/agent-lifecycle | reviewed |
| Oh My Pi | Session tree inside one JSONL file (id/parentId) with documented on-disk schema | Close to an event-log design | https://omp.sh/docs/session-format | reviewed |
| Pi | Session JSONL tree with `/tree` branching, branch summaries, append-only `context_edit` | Fold-over-log model | https://pi.dev/docs/latest/session-format | reviewed |
| OpenHands | Branch-from-message of conversations and goal loop with a separate judge | Branching UX | conversations.md, convo-goal.md | reviewed |
| OpenClaw | Managed worktrees that snapshot to a git ref before removal and fall back on `restore` | Suits coding sessions on nodes | https://docs.openclaw.ai/concepts/managed-worktrees | reviewed |
| OpenClaw | Durable "active writer claim" checked in the same transaction as every transcript write **[corrected]** | One-writer at the storage layer; state-dir lock not on the queue page | https://docs.openclaw.ai/concepts/agent-loop | corrected |
| Zed/ACP | Checkpoints removed after shipping because Git snapshots stalled on large repos **[corrected]** | Removed in 1.27.0 (2026-11-04); the Agent Panel page is silent | https://zed.dev/releases/stable | corrected |
| Cline | Shadow-git checkpoints with three restore combinations including task-only | Costs on big repos are documented complaints | https://docs.cline.bot/core-workflows/checkpoints.md | reviewed |
| Claude Code | Pluggable `SessionStore` with a conformance suite (mirror of the local JSONL transcript) | Storage adapter semantics | https://code.claude.com/docs/en/agent-sdk/session-storage.md | unreviewed |

##### F. Context, compaction and cache

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| OpenCode | Prompt-cache hygiene: cache warming, keyed session instructions that do not break the cache, summary requests reusing the main prompt cache | Cost wins users notice | https://opencode.ai/docs/compaction/, https://opencode.ai/changelog | reviewed |
| DeepSeek harness | Compaction as a locked log sequence with crash-orphan detection (`start` without `end`) | Crash-safe compaction | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/compaction | unreviewed |
| Oh My Pi | Snapcompact: older context archived as images by a vision model, no summarisation call **[corrected]** | Chain entry corrected (idle compaction off by default; 20k keep-recent is a summarising target) | https://omp.sh/docs/compaction | corrected |
| Oh My Pi | Compaction as a configurable fallback chain (remote, snapcompact, handoff, shake, soft) with idle and mid-turn triggers **[corrected]** | `shake` needs no LLM; same corrections | https://omp.sh/docs/compaction | corrected |
| Claude Code | Idle compaction timed to the prompt-cache TTL **[weakened]** | 200K+ tokens and the one-hour TTL confirmed; "about 54 minutes" not found, other numbers unverified | https://code.claude.com/docs/en/prompt-caching.md | corrected |

##### G. Models, routing and providers

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | Combos: six strategies, quota-window-aware selection, tool-call stickiness, failover only before output starts | Directly reusable provider failover | https://opencodex.me/guides/combos/ | reviewed |
| OpenCodex | Rewriting Codex's model catalog to steer the client's sub-agent surface without patching it | Control a closed client via the data it reads | https://opencodex.me/guides/sub-agent-surface/ | reviewed |
| OpenCode | Console gateway lets other harnesses (Claude Code, Codex, Pi, Hermes) use its models | Commercial model; explains free models and policies | https://opencode.ai/docs/console/harnesses/ | reviewed |
| Oh My Pi | Ten model roles with `@role` aliases, ordered selectors, fallback chains, usage-aware fallback | Clean routing abstraction; README lists nine | https://omp.sh/docs/roles | reviewed |
| Hermes Agent | Per-task auxiliary models (judge, curator, compression, review, approval) | Task-specific model slots | /integrations/providers | unreviewed |
| Warp | Custom model routers by complexity or natural-language rules in YAML | Routing by rule | https://docs.warp.dev/agents/inference/custom-routers/ | unreviewed |

##### H. Tools, code mode and edit formats

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | Web-search sidecar giving any model a search tool, results wrapped as untrusted | Cheap parity for weaker models | https://opencodex.me/guides/sidecars/ | reviewed |
| OpenCode | Code Mode (`execute`): model composes tool calls in JavaScript to cut schema context | Light-harness aid; denying it removes tools reachable only through it | https://opencode.ai/docs/tools/ | reviewed |
| Oh My Pi | Hashline edit anchors with a published 16-model benchmark **[corrected]** | Blog title says 15 LLMs, body 16; author's own benchmark | https://stencil.so/blog/the-harness-problem | corrected |
| Oh My Pi | TTSR: regex over the live stream aborts and retries with an injected rule; "zero standing context cost" **[weakened]** | Cost claim not stated by the TTSR page | https://omp.sh/docs/ttsr | corrected |
| Oh My Pi | Internal URL schemes (`pr://`, `conflict://`, `agent://`, `artifact://`, `security://`), `xd://` on-demand tool catalog | Fewer dedicated tools | https://omp.sh/docs/tools, https://omp.sh/docs/github | reviewed |
| Oh My Pi | Native DAP debugger, LSP with project-local servers and shared daemon, ast-grep codemods | Code-intelligence depth | https://omp.sh/docs/code-intelligence, https://omp.sh/docs/editing | reviewed |
| Pi | Codemode: model writes JS in a QuickJS sandbox that calls tools; default home for MCP tools **[corrected]** | Failed scripts do not roll back tool calls (not found); default contradicts across pages | https://pi.dev/docs/latest/codemode | corrected |
| Pi | MCP exposure modes (`codemode`/`deferred`/`direct`/`hidden`) per server or tool | Systematic answer to MCP context bloat | https://pi.dev/docs/latest/mcp | reviewed |
| Antigravity | Lazy MCP tool schemas by default with per-tool eager override | Light-harness invariant | https://antigravity.google/docs/mcp | unreviewed |
| goose | Code Mode: JS meta-tools in an embedded Boa engine | Same purpose as OpenCode/Pi | https://goose-docs.ai/docs/guides/managing-tools/code-mode.md | reviewed |
| Hermes Agent | `execute_code` tool RPC over a Unix socket with a persistent Python kernel | Many tool calls per turn | /user-guide/features/code-execution | reviewed |
| Hermes Agent | Tool Search bridge (`tool_search`, `tool_describe`, `tool_call`) with tiered manifests | Deferred tools | /user-guide/features/tool-search | unreviewed |
| Cline | Skills with three-level progressive loading and "script output only" context cost | Concrete context targets | https://docs.cline.bot/customization/skills.md | reviewed |
| Cline | Model-based edit tool routing (`apply_patch` vs `editor`) | Edit format per model | https://docs.cline.bot/tools-reference/all-cline-tools.md | reviewed |

##### I. Sub-agents and multi-agent orchestration

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| Claude Desktop | Cross-session tools letting the agent manage other desktop sessions, with admin switch | Agent-over-sessions risk and control | https://code.claude.com/docs/en/desktop | unreviewed |
| DeepSeek harness | Subagents can be external harnesses (Codex, Claude Code, ACP, DSH SDK) | Hosting other harnesses | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/subagent | reviewed |
| DeepSeek harness | Model-written workflow scripts under a hard `maxTotalAgents` the script cannot see | Budget the model cannot raise | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/workflow | reviewed |
| DeepSeek harness | Goal objects with revision compare-and-set and human-authorised resume | Concurrency-safe goals | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/goal | unreviewed |
| Oh My Pi | Advisor models (incl. WATCHDOG.yml roster) reviewing a live session in the background | Background reviewer | https://omp.sh/docs/advisor | reviewed |
| Cursor | Coordinator "Projects" agent that does not write code, only delegates | Delegation-only role | https://cursor.com/docs/agent/projects | unreviewed |
| Antigravity | Subagent frontmatter with per-agent `commandExecutionPolicy` and `model: inherit\|flash\|pro`; depth limit 10 | Per-agent policy | https://antigravity.google/docs/subagents | reviewed |
| Antigravity | `/teamwork` roles with adversarial verification and integrity modes **[corrected]** | Horizon is "multiple days", not "hours to days" | https://antigravity.google/docs/teamwork | corrected |
| Antigravity | Message-board swarm where workers cannot message each other privately | Auditable coordination | https://antigravity.google/docs/swarm | reviewed |
| Hermes Agent | Kanban with worker-per-OS-process, dispatcher reclaiming crashed claims, clean exit without completion is a protocol violation | Crash-safe claims | /user-guide/features/kanban | reviewed |
| Warp | Cross-harness orchestration with a durable inbox-per-agent message bus **[corrected]** | Wake-on-message and one-worktree advice not on cited pages | https://docs.warp.dev/platform/orchestration/ | corrected |
| Cline | Kanban of CLI agents (including competitors) in per-card worktrees **[corrected]** | Remote access has no auth; "root/token" is an inference | https://docs.cline.bot/usage/kanban.md | corrected |
| Claude Code | Workflows with deterministic replay (`Date.now()` and `Math.random()` throw) | Replayable orchestration | https://code.claude.com/docs/en/workflows.md | unreviewed |

##### J. Memory and learning

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| Cursor | Agent SDK memory as a plain JSONL journal read with ordinary file tools | No recall tool | https://cursor.com/docs/agent-sdk/reference/memory | unreviewed |
| Antigravity | `/learn` writes memory as reviewable rule/skill files behind an approval artifact | Visible, reviewable memory | https://antigravity.google/docs/commands/learn | reviewed |
| OpenHands | Persistent memory deliberately treated as untrusted text | Matches "nothing the model writes is trusted" | `persistent-memory.md` | reviewed |
| OpenClaw | Dreaming: scored, gated background promotion with a loss-fraction guard | Default-on; no time-based retention limit | https://docs.openclaw.ai/concepts/dreaming | reviewed |
| OpenClaw | Idle-time `skills` "workshop" tier of agent-learned skills in load precedence | Mechanism not documented | https://docs.openclaw.ai/tools/skills | unreviewed |
| Hermes Agent | Agent-curated procedural memory with deterministic-then-LLM curator, archive, snapshots, hash ledger | Rollback for self-written skills | /user-guide/features/curator | reviewed |
| Hermes Agent | Frozen-snapshot memory with hard character caps (2,200 and 1,375), no auto-compaction | Prompt-prefix stability | /user-guide/features/memory | reviewed |
| Hermes Agent | Session search returns raw FTS5 excerpts with no LLM, plus recovery pointer in compression summaries | Consistent with event-log fold | /user-guide/features/memory, /user-guide/sessions | reviewed |
| Warp | Cross-harness Agent Memory (personal, agent, team stores) shared by Warp Agent, Claude Code, Codex | Research preview | https://docs.warp.dev/agents/agent-memory/ | reviewed |

##### K. Extensibility, plugins, hooks and interoperability

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| Codex | Claude Code compatibility shims (`CLAUDE_PLUGIN_ROOT`, `/import`) **[corrected]** | 50-chat cap is CLI-only | https://learn.chatgpt.com/docs/import.md | corrected |
| OpenCode | In-process SDK host with no HTTP listener and a typed plugin RPC contract | Embedding without a listener | https://opencode.ai/docs/build/sdk/ | unreviewed |
| OpenCode | Plugins with four parts (server, TUI, GUI, RPC); managed policies can withhold plugins but are not a sandbox | No plugin sandbox is the cautionary note | https://opencode.ai/docs/plugins/, https://opencode.ai/docs/policies/ | unreviewed |
| DeepSeek harness | Everything-is-a-plugin runtime with reversible registrations and hot reload | Heavy; plugins run in-process | https://deepseek-harness.github.io/deepseek-harness/en/reference/ | unreviewed |
| Oh My Pi | Reads other harnesses' config natively **[corrected]** | `agents` and `codex` are tied; scope limits | https://omp.sh/docs/context-files | corrected |
| Pi | Extension events at provider level and tool overrides by name | Scope reference | https://pi.dev/docs/latest/extensions | unreviewed |
| Pi | 5.5k+ third-party npm packages fill the omitted features | Unvetted supply chain | https://pi.dev/packages | unreviewed |
| Cursor | Cross-tool compatibility: loads `.claude/` and `.codex/` agents, skills and Claude Code hooks | Cheap adoption | https://cursor.com/docs/skills | unreviewed |
| Antigravity | Hooks that inject steps, rewrite arguments and force the loop to continue; `/goal` on a stop hook | Hooks as a loop control point | https://antigravity.google/docs/hooks | reviewed |
| OpenHands | Canvas Apps: third-party UI pages with host API proxying authenticated backend calls | Unsandboxed per the docs | `canvas-extensions.md` | reviewed |
| OpenClaw | The agent can be hosted by, or host, other harnesses (Codex app-server, Claude CLI, ACP) | Runtime pluggability | https://docs.openclaw.ai/concepts/agent-runtimes | unreviewed |
| goose | ACP providers: drive Claude/Codex/Pi/Amp through ACP mapping its own permission modes | Mode mapping | https://goose-docs.ai/docs/guides/acp-providers.md | reviewed |
| goose | Hooks and plugins follow the "Open Plugins" spec; Gemini manifests accepted; `.claude/` paths read | Interoperability | https://goose-docs.ai/docs/guides/context-engineering/plugins.md | reviewed |
| goose | Configurable instruction-file name list (`CONTEXT_FILE_NAMES`) | Read `CLAUDE.md` or `.cursorrules` | https://goose-docs.ai/docs/guides/context-engineering/using-goosehints.md | unreviewed |
| Zed/ACP | Terminal Threads: embed any CLI agent as a thread without protocol support | Zed profiles and permissions do not apply to them | https://zed.dev/docs/ai/terminal-threads.md | reviewed |
| Zed/ACP | ACP registry with auth requirement and client-driven install | Auth-only catalogue | https://agentclientprotocol.com/get-started/registry.md | reviewed |
| Zed/ACP | Proxy chains and MCP-over-ACP as an extension mechanism | MCP-over-ACP is draft; proxy-chains claim not opened in review | https://agentclientprotocol.com/get-started/related-protocols.md | unverified (proxy chains) |
| Warp | Agent-hosting terminal: hosts third-party CLI agents with review-comment injection and notifications | Harness for harnesses | https://docs.warp.dev/agents/cli-agents/overview/ | unreviewed |
| Warp | Skills read from ten tools' directories | Deliberate compatibility | https://docs.warp.dev/agents/capabilities/ | reviewed |
| Cline | Conditional rules by file-path glob with fail-open behaviour and activation notices | Per-path instructions | https://docs.cline.bot/customization/cline-rules.md | unreviewed |
| Claude Code | Mods: in-process JS/TS middleware over almost every engine decision; static analysis can enumerate the `$` API use | Unsandboxed plugins made auditable; fail-open and guard caveats noted in review 2 | https://code.claude.com/docs/en/plugins/mods/overview.md | unreviewed |

##### L. UI, review and artifacts

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| Claude Desktop | Browser pane with dev-server verification via `launch.json`, clean profile, vendor-run site safety list **[corrected]** | Support article does not cover per-site choices; cookie rules differ by session type | https://code.claude.com/docs/en/desktop | corrected |
| Antigravity | Browser agent drives the user's real Chrome over DevTools MCP | Contradicts "sandboxed" wording elsewhere | https://antigravity.google/docs/commands/browser | reviewed |
| Antigravity | Staged pending comments on artifacts sent with the next message | UI pattern a web dashboard can copy | https://antigravity.google/docs/artifacts | unreviewed |
| Warp | Computer-use screenshots and videos attached to PRs with admin-controlled sharing | Events-plus-files UI model | https://docs.warp.dev/agents/capabilities/computer-use/artifacts-in-prs/ | reviewed |
| Cline | Messaging connectors (Telegram, Slack, Discord, WhatsApp, Linear, Google Chat) as agent front-ends with allow/deny hook | Risky front-ends; Telegram default approves everything | https://docs.cline.bot/cli/connectors.md | reviewed |

##### M. Scheduling, goals and automation

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| Codex | Goals with token and time budgets as persisted thread state with automatic continuation | Budgeted goals | https://learn.chatgpt.com/docs/app-server.md | reviewed |
| Claude Desktop | Scheduled tasks stored as `SKILL.md` on disk with deterministic jitter, one catch-up run, per-task revocation list **[corrected]** | `/schedule` was not removed; settings key claim wrong | https://code.claude.com/docs/en/desktop-scheduled-tasks | corrected |
| Cursor | Rollouts: post-merge monitoring against telemetry at fixed intervals | Beyond merge | https://cursor.com/docs/security-agents | unreviewed |
| OpenHands | Automations synced to Git as YAML with optional encryption and webhook sources with JMESPath filters | Routines as code | `git-sync.md`, `event-automations.md` | unreviewed |
| OpenClaw | Standing intents use a model-free FTS prefilter, with cooldown, fire budget and expiry | Cheap event triggers | https://docs.openclaw.ai/concepts/standing-intents | unreviewed |
| goose | Recipe `retry` with shell success checks and history reset; `response.json_schema` validation | Verified task completion | https://goose-docs.ai/docs/guides/recipes/recipe-reference.md | reviewed |
| Warp | Factories: definitions as code in a customer repo reviewed by PR, foreman agent, never merges itself **[corrected]** | Status is Early Access | https://docs.warp.dev/factories/ | corrected |

##### N. Documentation candour, governance and product posture

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| Codex | Docs are candid about the limits of their own security controls (DNS rebinding "best effort", deny-read glob snapshots, proxy does not cover search/MCP/apps) | Traps to avoid repeating silently | https://learn.chatgpt.com/docs/permissions.md | reviewed |
| Pi | Documented non-features with reasons and a substitute for each **[corrected]** | To-do reason in the 2025 post is "confuse models"; plan reason includes visibility | https://pi.dev/, https://mariozechner.at/posts/2025-11-30-pi-coding-agent/ | corrected |
| Pi | Docs-vs-no-docs behavioural evals for the project's own documentation | Measures doc value | https://github.com/earendil-works/pi/blob/main/packages/evals/README.md | unreviewed |
| Claude Desktop | Three different things called "project" (claude.ai, Cowork, cloud Projects coordinator) | Naming hazard | https://code.claude.com/docs/en/claude-projects | reviewed |
| OpenClaw | Project publishes its own no-action findings list plus a maturity scorecard with percentages | Candid threat model | https://docs.openclaw.ai/gateway/security/trust-model, https://docs.openclaw.ai/maturity/scorecard | reviewed |
| Warp | Agent-driven open-source contribution model (agents code, humans specify and verify) | AGPL means ideas only per our CLAUDE.md | https://www.warp.dev/blog/warp-is-now-open-source | reviewed |
| Cline | Prompt Storage to customer-owned S3/R2 as the enterprise audit approach | Audit without vendor storage | https://docs.cline.bot/enterprise-solutions/monitoring/prompt-storage.md | unreviewed |

Item counts per file in section 3 (items counted in the tables above): Codex 12, OpenCodex 6, OpenCode 10, Claude Desktop 9, DeepSeek harness 12, Oh My Pi 12, Pi 10, Cursor 10, Antigravity 12, OpenHands 10, OpenClaw 12, goose 10, Hermes Agent 12, Zed/ACP 8, Warp 11, Cline 10, Claude Code 13. Each item appears once, under its main theme (goose's extension allowlist is under A). For Claude Code, review 3 also corrected the survey's relevance text: the control plane pushes the tool list, permission rules, model, connectors and lifecycle hooks with each work payload, so the runner design inverts "the node decides".
