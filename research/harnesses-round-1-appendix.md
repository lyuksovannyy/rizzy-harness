# AI coding harnesses: Round 1 research, appendix

Companion to [`harnesses-round-1.md`](harnesses-round-1.md). Everything below is copied unchanged from the ten synthesis files in [`round1/synth/`](round1/synth/), grouped by the same ten groups of feature areas. Statements about "ours" describe branch `main`; section 8 of the report says what the open pull request and the roster branch change.

## Contents

- A. Features only one harness has
- B. Ours against the field, per group of areas
- C. Corrections, contradictions and unverified claims
- D. Features common across the ecosystem
- E. Every distinctive item, by theme
- F. Every overhyped, stale or contradictory item, by harness
- G. All questions for Round 2

## A. Features only one harness has

### Agent loop, tools, editing, shell (areas 1-4)

From [round1/synth/01-loop-tools-editing-shell.md](round1/synth/01-loop-tools-editing-shell.md).

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

### Permissions, sandboxing, security (areas 5, 21)

From [round1/synth/02-permissions-sandbox-security.md](round1/synth/02-permissions-sandbox-security.md).

- **Approval binding to exact argv, cwd, env and executable content hash, re-checked before launch (OpenClaw, F4).** Interesting: it closes the "approve `git status`, PATH now points to a different `git`" hole that every prefix-rule design leaves open. Directly copyable for our node, which already resolves paths component by component.
- **Command rules with inline unit tests (`match`/`not_match`) checked at load (Codex, F2).** Interesting: rule lists rot; tests in the rule file make a bad rule fail at startup instead of in production.
- **Per-command network grants reviewed with the command (Claude Code auto mode, F13).** Interesting design (capability scoped to one invocation, adds nothing to session state), but it depends on a classifier and a proxy we do not have.
- **Sentinel credential masking with SigV4 re-signing (Claude Code sandbox, F22).** Interesting but heavy: needs TLS termination (labelled experimental) and has documented failure modes (macOS blocks the file, a non-matching pattern leaves the real file readable). Hermes's iron-proxy is the simpler variant of the same idea.
- **Approval-audit append failure rejects the request (DeepSeek, F20).** Interesting and cheap: "no unlogged decision" matches our one-writer event log exactly.
- **Approval requests that deliberately omit tool arguments (DeepSeek, F4/F20).** Not interesting as UX (the reviewer flags it as weak): an approver who cannot see the arguments is approving blind.
- **Hooks trusted by content hash (Codex, F29).** Interesting for any future hooks: a repo or plugin that changes a reviewed hook is re-prompted.
- **Sub-agent report scanning that neutralises harness-tag imitations (Claude Code, F26).** Interesting, cheap, and fits our sub-agent and cross-node reports.
- **Private-address requirement and TLS pinning for the settings gateway (Claude Code apps gateway, F31).** Interesting principle: a server that can push settings that run commands must itself be pinned. Our node pins the master already.
- **Mined allowlist suggestions that never apply themselves and never propose destructive classes (Hermes, F2).** Interesting UX for reducing prompt fatigue without auto-widening.
- **One-owner-per-runner lock (Claude Code self-hosted, F31).** Interesting for multi-user nodes: a runner that served user A never serves user B without a fresh disk.
- **Editable approval scope in the prompt (Antigravity, F4).** Mildly interesting: lets the user widen a file approval to its directory deliberately instead of clicking "always".
- **Backend-reported `full` vs `partial` sandbox enforcement (DeepSeek, F12).** Interesting and honest: tells the user when Landlock ABI or platform leaves gaps. Our branch's ABI 9 socket gap is exactly such a case.
- **Provenance-gated `web_fetch` (ours, F25).** No other harness documents refusing URLs that did not appear in user or tool text.

### Context management and memory (areas 6, 7)

From [round1/synth/03-context-memory.md](round1/synth/03-context-memory.md).

| Feature | Harness | Why interesting or not |
|---|---|---|
| Snapcompact: older context archived as images by a vision model, no summarisation call | Oh My Pi | Avoids lossy summaries but needs a vision model and an image font; unproven benefit. |
| Configurable compaction method chain (remote, snapcompact, handoff, shake, soft) | Oh My Pi | Best fallback design in the field; `shake` needs no LLM. |
| Failure ladder 60/300/900 s with a deterministic fallback summary; "summary model smaller than main model silently drops middle turns" documented as a failure mode | Hermes Agent | A concrete failure list worth testing. |
| Summary audit (`safeguard`: identifiers, pending asks, degraded marker) | OpenClaw | Cheap quality control on summaries. |
| Compaction as a locked log sequence with crash-orphan detection | DeepSeek harness | Fits an append-only event log; relevant to restart mid-compaction. |
| Idle compaction timed to prompt-cache expiry | Claude Code | Only worthwhile if cache TTL economics apply (the exact idle time is unverified). |
| Pre-written background summary swapped in | OpenCode | Experimental; held summaries lost on restart. |
| Keyed session instructions that do not touch the cached prompt | OpenCode | Directly reusable idea for per-turn memory injection. |
| `context_edit` entries that rewrite future context without mutating history | Pi | Maps to a fold over the event log. |
| Dreaming with scoring gates and a 25% rewrite-loss guard | OpenClaw | Defensible consolidation design; also the clearest example of no human approval and no retention (project's own admission). |
| Curator for agent-written skills (stale at 14 d, archive at 30 d, lock, pin, hash ledger, rollback) | Hermes Agent | Treats self-written skills as managed assets. |
| Write-approval staging queues for memory and skills with `/journey` | Hermes Agent | Closest field example of a human gate on agent-written memory (off by default). |
| `/learn` writing a reviewable proposal artifact before any rule or skill file changes | Antigravity | Approval-gated, file-based, workspace or global scope. |
| Cross-harness shared memory (personal, agent, team stores with read-only/read-write attachment) | Warp | Preview; mechanism undocumented. |
| Frozen memory snapshot with hard character caps (2,200 and 1,375) and no auto-compaction | Hermes Agent | Forces curation and preserves the cache prefix. |
| Per-turn re-injected instructions, 64 KB cap (MOIM) | goose | Counters forgetting; extension link unverified. |
| Standing intents (keyword-triggered owner instructions, no model call) | OpenClaw | Interesting for routines; owner-only on authenticated channels. |
| Rules fail open on bad frontmatter (Cline) versus silently discarded (Antigravity) | Cline / Antigravity | Opposite failure modes of the same design; pick deliberately. |
| `REVIEW.md` read only from the base branch, honoured by branch-protection level | Claude Code | Defence against a PR editing its own review instructions. |
| Embeddings code index with ignore files from five tools | Warp | The only documented embeddings index; compute location unstated. |

### Planning, sub-agents, background work (areas 8-10)

From [round1/synth/04-planning-subagents-background.md](round1/synth/04-planning-subagents-background.md).

| Feature | Harness | Why it is interesting (or not) |
|---|---|---|
| Sub-agent output scanning before the parent reads it | Claude Code | A cheap prompt-injection layer at the delegation boundary. **Directly relevant to us**, because sub-agent and cross-node reports go to a parent. |
| Workflows with deterministic replay (`Date.now()`/`Math.random()` throw) | Claude Code | Lets a crashed fan-out resume from cached results by prompt order. A strong idea for durable orchestration; DeepSeek's workflow API copies the shape without the replay claim. |
| Cross-session inbound trust model (`crossSessionInbound`, peer messages cannot answer approvals) | Claude Code | The only documented answer to "what may a message from another agent authorise". Needed before any agent-to-agent bus. |
| Auto-backgrounding long MCP calls and MCP tasks | Claude Code | Useful only once there is MCP; we have none. |
| Auto-continue after a usage limit | Claude Code | Subscription-specific; `/config` can disable it but not enable it, because enabling grants unattended execution. That one-way rule is worth copying. |
| Timed user questions answered after a restart | DeepSeek harness | A question survives the tool call and a host restart. Good for a web UI where the user answers hours later. |
| Goal objects with compare-and-set revision and human-authorised resume | DeepSeek harness | Matches an event-log design: the goal is a fold over events. |
| Hidden `maxTotalAgents` that the script cannot see | DeepSeek harness | A cap the model cannot plan around. Simple and correct. |
| Ten model roles with `@role` aliases, per-role fallback chains and auto-resolution | Oh My Pi | The most complete slot design. Its auto-resolution makes "which model ran this" non-obvious, so it is a weakness for auditability. |
| Prewalk (strong model starts, one switch to a cheaper model) | Oh My Pi | A cheap cost lever; one switch, logged. |
| Copy-on-write isolation backends with patch merge | Oh My Pi | Faster than worktrees on reflink filesystems; the docs say it is not a sandbox. |
| Vision sidecar for text-only models | OpenCodex (gateway) | Exactly the "vision bridge" slot the reader wants. Hermes's auxiliary vision model is the in-harness equivalent. |
| Message-board-only swarm (no private messages) | Antigravity | Forces visibility of all coordination. Interesting for auditability; preview. |
| Sidecars plus the `agentapi` CLI | Antigravity | Lets local scripts create or message conversations: an event source without a webhook. |
| Integrity modes (`benchmark` forbids mock passes) | Antigravity | A rare explicit guard against an agent faking success. |
| Standing intents (keyword-triggered, model-free prefilter) | OpenClaw | Reminders that fire on content rather than time, at zero model cost. |
| Restart resume input ("check files, processes, external actions") | OpenClaw | An honest handling of side effects across a crash. |
| Kanban with an OS process per worker, a reclaiming dispatcher and `protocol_violation` | Hermes Agent | A real crash model for multi-agent boards; its threat model is single-host only. |
| Separate fallback chains per auxiliary task | Hermes Agent | The cleanest answer to "a summariser outage should not take down titles". |
| Script-only cron with `wakeAgent: false` gating | Hermes Agent | A model-free pre-check before paying for an agent run. Directly applicable to routines. |
| Durable server-backed inbox per agent across harnesses | Warp | Cross-harness orchestration; the control plane sees all traffic. |
| One-catch-up rule for missed scheduled runs (within 7 days) | Claude Desktop | A clear and testable missed-run policy. |

---

### Sessions and git (areas 11, 12)

From [round1/synth/05-sessions-git.md](round1/synth/05-sessions-git.md).

| Feature | Harness | Why interesting (or not) |
|---|---|---|
| In-file session tree shared by two harnesses only | Oh My Pi, Pi (same lineage) | Pi-style `id`/`parentId` log maps cleanly onto "fold over an append-only log": branches are just different leaf pointers. Interesting for Ours: forking without copying events. Cost: branches change file state invisibly, neither reverts files on navigation. |
| Pluggable `SessionStore` adapters | Claude Code | Documents an explicit append/load contract (ordered, deep-equal, dedupe by entry uuid), conformance suite and dual-write caveats. Relevant to a master that mirrors node logs; the best-effort mirror that drops a batch after 3 failed attempts is a counter-example to copy from. |
| SDK `rewindFiles(uuid)` with dry-run | Claude Code | Programmatic restore point per user message; the survey notes a past bug class (writing through symlinks before v2.1.216). |
| Agent-facing `checkpoint`/`rewind` tools | Oh My Pi | The model itself marks and returns to conversational checkpoints (off by default, no file revert). Interesting as context management; not a safety feature. |
| Ultrareview (cloud fleet, independent reproduction, billing, exit codes) | Claude Code | Interesting as a verified-findings design; not applicable to a self-hosted harness unless nodes can run a reviewer fleet. Research preview. |
| `omp commit` with dependency-ordered splits and validation | Oh My Pi | The most developed commit-message feature in the survey; deterministic validation plus agentic split with a mechanical fallback. |
| Pre-removal snapshot refs for worktrees and 30-day restore | OpenClaw (Hermes has a similar archive-ref design) | Prevents the "deleted a dirty worktree" loss that Claude Desktop's 30-day archive removal can cause. Cheap to copy. |
| Bugbot reading its config from the base branch | Cursor | A PR cannot weaken its own review; transferable to any review feature that reads repo-supplied rules. |
| Cloud git credential proxy (credentials never in the VM, GraphQL rejected, no tag pushes or branch deletion) | Claude Code | Directly relevant to keeping git secrets off worker nodes; the "GraphQL rejected" side effect breaks `gh pr`. |
| Session-to-PR linking and PR-URL lookup in the picker | Claude Code | Cheap way to find a session from its PR; needs a PR record in the session log. |
| Client-side encrypted share with key in the URL fragment | Oh My Pi | Share without the server reading the content; conflicts with a master that must read the log. |
| Documented, removed checkpoint design | Zed | A real negative result (per-prompt Git snapshots stalled on large repos), not an absent feature. |
| Webhook-spawned read-only review sessions (no outbound GitHub access) | DeepSeek harness | Closest published analogue of "webhook triggers a read-only session on a node"; documents its own gaps (no dedupe, no retry). |
| Computer-use artifacts attached to PRs | Warp | Visual evidence in PRs; privacy toggle (link only vs embed). Not core. |

### Extensibility (area 13)

From [round1/synth/06-extensibility.md](round1/synth/06-extensibility.md).

| Feature | Harness | Why it is interesting (or not) |
|---|---|---|
| Hooks trusted by hash | Codex | A changed hook or plugin script is skipped until re-reviewed. Closes "a repo ships a hook" without a trust dialog per folder. Directly applicable to node-side hooks. |
| PreToolUse `defer` | Claude Code | A hook ends a headless turn with the tool call pending; the host resumes after a human decides, no timeout. Exactly our "approver on another device, link dropped" case. |
| Mods with static analysis of a host API | Claude Code | Unsandboxed in-process middleware made auditable by forcing every side effect through an enumerable `$` API; but fail-open by default and bypassable by `--safe-mode` and worker crashes (review 2). Interesting design, risky defaults. |
| Prompt-cache-aware plugin reload | Claude Code | `/reload-plugins` refuses changes that would invalidate the prompt cache unless forced. Cheap, users feel it in cost. |
| MCP tasks extension client | Claude Code (beta) | Long server-side jobs polled and re-followed after resume. Relevant if nodes run long MCP jobs. |
| CLI hints stripped from tool output | Claude Code | Third-party CLIs can suggest a plugin via a stderr tag that is stripped before the model sees it, so the channel cannot inject text. Niche. |
| Plugin relevance signals | Claude Code | Marketplace entries suggest themselves from local signals (cwd, commands, hosts) with no network. Niche discovery UX. |
| Agent-skill curator with ledger and rollback | Hermes Agent | Treats agent-written skills as a managed asset: archive, consolidate, lock, sha256 ledger, rollback. The only answer seen to "the agent's self-written skills rot". |
| A2A both ways | Hermes Agent | Only harness with agent-to-agent federation; per-peer tokens and audit. Low priority for us. |
| MCP spend limit that YOLO cannot skip | Hermes Agent | Per-tool cost declarations and a per-chat spend cap that still asks on trusted servers. Good model for paid MCP tools. |
| TTSR stream rules | Oh My Pi | Regex over the live token stream aborts and regenerates with an injected rule; rules cost context only when they fire. Cheap to implement node-side; "zero standing cost" is the reviewer-noted inference. |
| Four-part plugins (server, TUI, GUI, RPC) | OpenCode | A plugin can add a typed RPC contract clients call. Over-engineered for us. |
| Command shell blocks outside the permission flow | OpenCode | Documented approval-path hole; a counter-example. |
| Peer-to-peer ACP transport (Roaming) | goose (experimental) | iroh/QUIC with key-card trust and revoke; NAT-free node reach. Note "spawned processes survive revoke". |
| Canvas Apps (third-party UI pages) | OpenHands (beta) | UI extensions that call the backend without holding the session key; no sandbox for app code. |
| Agent-written plugin market with confirm-source dialog | DeepSeek harness | Shows the risk of open in-process plugins on a machine that runs shell commands; `http:` indexes tamperable. |
| Plugin worker-thread isolation with opt-in sandbox subprocess | Cline | The only documented sandbox mode for plugin code, even if opt-in. |
| ACP registry with auth requirement | Zed/ACP (protocol) | Curated `registry.json` CDN for one-click agent install; only agents with an auth method qualify. |

### Models and observability (areas 14, 19)

From [round1/synth/07-models-observability.md](round1/synth/07-models-observability.md).

| Capability | Harness | Why it is interesting or not |
|---|---|---|
| Parallel answers from up to 4 models with a pick | rizzy-harness (ours) | No equivalent found in the surveyed entries for this area; cheap model comparison, but multiplies cost with no budget to bound it. |
| Content-based safety fallback (flagged cyber or biology requests re-run on another model, session stays there) | Claude Code | Vendor-specific safety routing; not portable, but shows fallback can be triggered by content, not only by errors. |
| `modelPricing` multiplier and per-model overrides for chargeback, applied to `/usage`, `--max-budget-usd` and OTel cost | Claude Code | Directly useful for a multi-user master that bills teams; managed-only, so users cannot edit their own rates. |
| Gateway-side spend caps by user, group and org, with pooled budgets, enforced on every request | Claude Code (apps gateway) | The closest design to a master enforcing per-user budgets; it is a gateway service, not a client setting. |
| Managed OTLP destination lock (developer endpoints stripped when admin sets one; a user endpoint once redirected away from the collector, closed in v2.1.217) | Claude Code | A security detail: telemetry destination is policy, not preference. |
| Cache-miss attribution in `/usage` and idle compaction timed to the cache TTL | Claude Code | Cost optimisation users notice; ties observability to prompt-cache behaviour. |
| `claude plugin details` projected token cost of a plugin | Claude Code | Cost shown before install; out of our scope until we have plugins. |
| Six routing strategies, tool-call stickiness within a turn, quota-window-aware selection | OpenCodex | The most developed provider-failover design in the set; fits a master's provider layer. |
| Estimated cost labelled as list-price equivalent with coverage warnings | OpenCodex | Honest-estimate UX bar. |
| Ten model roles with `@role` aliases and per-role fallback chains | Oh My Pi | Cleaner than per-feature settings; one resolver for all auxiliary work. |
| `omp bench` and `omp dry-balance` (simulate account selection) | Oh My Pi | Testing the routing logic without sending traffic. |
| Per-task auxiliary models, each with its own fallback chain | Hermes Agent | Shows that auxiliary calls need their own failure handling. |
| Local subscription proxy exposing a user's own subscription to other apps (no auth of its own) | Hermes Agent | Convenient and risky; legal status not discussed in the entry. |
| Custom model routers by complexity or natural-language rules in YAML | Warp | User-authored routing; targets restricted to Warp-supported models. |
| Raw LLM request logs, last ten kept | goose | Useful for debugging; the page's own implication is that they hold raw prompts. |
| Redaction waterfall on a full-ledger OTel mirror, with the seam shipping no rules | DeepSeek harness | Shows the danger of an export with no default redaction. |
| Provider-reported quota and spend rather than estimates; subscription sessions hide dollars | OpenClaw | Avoids showing a wrong number; needs provider cooperation. |
| Goal token and time budget as persisted thread state | Codex | Budget is part of the log, not a runtime flag. |
| Config options (`mode`, `model`, `model_config`, `thought_level`) as protocol-level session state | ACP | Model and effort selection that a web UI can fold from events. |

---

### UI, browser, collaboration (areas 15, 16, 18)

From [round1/synth/08-ui-browser-collaboration.md](round1/synth/08-ui-browser-collaboration.md).

- **Claude Code agent view with state grouping ("Needs input", "Ready for review") and a peek-and-reply panel.** Interesting: it is the only documented "waiting for you" dashboard that is a list grouped by state, and the supervisor daemon behind it is the closest thing to a node hosting sessions.
- **Claude Code mods that draw UI (render sites and element trees validated by the host), with the permission prompt excluded from the render sites.** Interesting for "nothing the model writes is trusted"; the explicit carve-out is the useful part.
- **Claude apps gateway: a self-hosted, binary-built-in admin plane (OIDC, per-group policies, server-side model enforcement, spend caps, audit log) with no user directory.** Interesting because it is a free self-hosted multi-user admin plane next to a closed product; its fail-open default is a warning.
- **Claude Code Trusted Devices for Remote Control** (device credential plus a sign-in no older than 18 hours): a step-up control for remote steering that nobody else documents.
- **Oh My Pi collab: client-side sealed live sessions through a relay, bearer-secret links, no accounts.** Interesting, but end-to-end sealing conflicts with a master that must read the log.
- **Hermes: credential vault for browser logins (the model never sees values, origin-bound fills), and Hermes Relay (gateway holds no platform tokens).** Interesting as a pattern for keeping secrets off the agent host.
- **Hermes: gateway delivery ledger and unknown-crashed-turn handling for chat channels.** Relevant to one writer per session and restart mid-operation.
- **OpenClaw: approvals answerable inside chat channels with native approver lists; two-layer node pairing; operator scopes with a ceiling.** Relevant to remote approval and to the master/node link.
- **Warp: Warp Drive as a synced, role-based, folder-inherited workspace of prompts, workflows, notebooks, rules and MCP servers; editable plans with version history.** Interesting for shared team resources; the closed control plane is the catch.
- **Cursor: Live Activities for up to eight agents on iOS; Cloud Agent Canvases.** Cursor-specific, mobile polish.
- **Antigravity: browser subagent on the user's real Chrome via DevTools MCP with no extension.** Different from the extension route, and its own pages disagree on whether it is "sandboxed".
- **OpenHands: browser UI that federates to any number of backends with `--frontend-only`/`--backend-only`.** A client-side federation, not a master that owns state.
- **OpenCode: `opencode pair` single-use link plus optional end-to-end encrypted tunnel.** A concrete pattern for web access without accounts.
- **goose roaming: iroh/QUIC peer-to-peer ACP with a wasm browser peer.** Experimental, not in released binaries.
- **Codex: self-hosted `[config_server]` for requirements; omitted permission profiles denied including future built-ins.** The strongest "admin policy" design for the node-decides idea.
- **Cline: remote config coverage differs per surface and the docs say so.** Interesting as a failure mode: the enterprise policy does not reach CLI or Desktop.

### Remote execution and architecture (area 17)

From [round1/synth/09-remote-distributed-architecture.md](round1/synth/09-remote-distributed-architecture.md).

- **Claude Code, PreToolUse `defer` for headless runs** (2.1 "Approval behaviour"): ends the turn with the call pending and resumes later with no timeout. Interesting: it is exactly "the approver is on another device and may answer tomorrow", which our routines currently solve by blocking a turn for up to 24 h.
- **Claude Code, one-owner-per-runner lock** (2.4 "Self-hosted workers"): the first session claimed locks a runner to that owner until it exits with a fresh disk. Interesting for a node shared by several master users: it bounds cross-user leakage on a shared checkout without per-session VMs.
- **Claude Code, session identity JWT with a public JWKS and an `act` chain** (2.4 "Per-dispatch credentials"): internal services can verify which user, runner and pool a session belongs to. Interesting if nodes ever call other self-hosted services on a user's behalf; overkill today.
- **Claude Code, `--confine-repo-settings enforce`** (2.4 "Execution-side authority"): the runner refuses sessions whose committed repo settings would reach outside the workspace. Interesting: a node-side floor against repo-shipped policy, which is our invariant applied to repo config.
- **Claude Code, `--cloud` follow-ups refuse to send if org policy has not loaded** (2.5 "Cloud agent control API"): fail closed on unknown policy. Cheap to copy wherever the master caches a node's policy.
- **OpenHands, deferred init** (2.5 "Warm pools"): dormant pods activated by one call that brings per-user config, so the image holds no user data. Interesting for nodes that spawn per-session sandboxes later; not needed for persistent nodes.
- **OpenCodex, executor that never falls back to the hub filesystem** (2.0, Remote Workspace): when the executor is gone, file tools fail rather than silently run on the control side. A rule worth stating for us: a session bound to a disconnected node must not run anywhere else.
- **OpenClaw, owner-epoch fencing of stale workers** (2.3 "Reconnect"): a worker that comes back after its credential was rotated is rejected. Directly relevant to "one writer per session" after a node reconnects with stale state.
- **OpenClaw, two-layer node pairing** (2.3 "Pairing"): device identity first, then approval of the declared command set. Interesting as an admin UX: the master shows what a node claims to offer before it is used.
- **goose, peer-to-peer ACP over iroh (QUIC)** (2.0, Roaming): no open ports, key cards, revoke closes connections. Interesting as NAT traversal without a relay, but experimental and not in released binaries; its stated limit (spawned processes survive revocation) applies to any revoke design.
- **goose, ACP over HTTP and WebSocket shipping before the RFD** (2.3 "ACP"): the only remote ACP transport in use.
- **Hermes Agent, Relay that holds platform bot tokens** (2.4 "Central credential broker"): the gateway dials out and holds no platform credentials. A pattern for chat front-ends if the master ever bridges messaging platforms.
- **DeepSeek harness, Host fence that rejects `0.0.0.0` and TLS that never falls back** (2.3 "Exposure"): concrete, cheap checks for a loopback-by-default server.
- **Codex, bounded queues answering `-32001` "Server overloaded"** (2.3 "Programmatic control protocol"): the only documented overload behaviour on a control protocol. Directly relevant to our unbounded link channel (AUDIT L1).
- **Warp, `Command` worker backend** (2.4 "Self-hosted workers"): the worker hands each task to an external job API or queue. Cheap to offer to operators who already have a scheduler.
- **Cline, capability brokering across attached clients** (2.0, Cline hub): the hub routes a diff to the IDE and a shell request to the CLI. Interesting for multiple UIs on one session; not a node concern.

### Configuration, licence, other (areas 20, 22, 23)

From [round1/synth/10-config-licence-distinctive.md](round1/synth/10-config-licence-distinctive.md).

| Feature | Harness | Why it is interesting or not |
|---|---|---|
| Self-hosted config server fetched by every client (`[config_server]`, optional OAuth) | Codex | Interesting: one org-run HTTPS endpoint feeds defaults and requirements to CLI and desktop. https://learn.chatgpt.com/docs/enterprise/configuration-server.md |
| Config assembled by a patch pipeline (bundles, profile, home, `--patch`) with optimistic-revision UI edits | DeepSeek harness | Interesting for hot-reloadable settings, but preview and plugin-heavy. https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/settings |
| Strict-JSON settings with about 250 keys each tagged with a scope, plus about 400 env vars and one-way ratchets | Claude Code | Shows how large the surface gets; the scope tag per key is the useful idea. https://code.claude.com/docs/en/settings-reference.md |
| Per-viewing-surface remote config table (what each client gets) | Cline | Useful as a checklist of gaps (CLI/Desktop not covered). https://docs.cline.bot/enterprise-solutions/configuration/remote-configuration/overview.md |
| Output styles that drop the built-in software-engineering prompt section (`keep-coding-instructions`) | Claude Code | Interesting: a persona that can turn the coding prompt off. https://code.claude.com/docs/en/output-styles.md |
| `SOUL.md` global identity with 14 personality presets | Hermes Agent | Cheap persona layer; relevant to the assistant side. /user-guide/features/personality |
| Recipes with typed parameters, JSON-schema response validation and retry with shell success checks | goose | A reusable task preset format, close to routines. https://goose-docs.ai/docs/guides/recipes/recipe-reference.md |
| Config key blocklist for repo-local config with a warning | Codex | Smallest copyable form of "repo cannot redirect credentials". https://learn.chatgpt.com/docs/config-file/config-advanced.md |
| Maturity scorecard with percentages for each area | OpenClaw | Unusual candour; the survey notes Windows is labelled Beta at 19% quality against the page's own Experimental band. https://docs.openclaw.ai/maturity/scorecard |
| Fair Source and proprietary tiers announced after an ownership change | Pi | Governance signal for dependency risk. https://mariozechner.at/posts/2026-04-08-ive-sold-out/ |
| Foundation governance (Linux Foundation AAIF) | goose | Neutral steward; contrast with company-owned projects. https://github.com/aaif-goose/goose/blob/main/GOVERNANCE.md |
| 501(c)(3) foundation employing the core team | OpenClaw | Another governance model. https://github.com/openclaw/openclaw |
| Client AGPL with agent-driven contribution model | Warp | Licence constraint: the repo's CLAUDE.md allows ideas only, not code, from AGPL sources. https://www.warp.dev/blog/warp-is-now-open-source |
| ToS that forbids third-party harnesses on the consumer service | Antigravity | Hard constraint if rizzy wanted to drive `agy`. https://antigravity.google/docs/faq |
| API that refuses agent self-service (`403 agent_consent_required`) | OpenCodex | A small anti-abuse idea. https://opencodex.me/getting-started/for-agents/ |
| Machine kept awake while an agent runs | Antigravity | Relevant to nodes on laptops. https://antigravity.google/docs/faq |
| Deep links with warning-not-send prompt handling | Claude Code | Safe pattern for external prompts. https://code.claude.com/docs/en/deep-links.md |

## B. Ours against the field, per group of areas

### Agent loop, tools, editing, shell (areas 1-4)

From [round1/synth/01-loop-tools-editing-shell.md](round1/synth/01-loop-tools-editing-shell.md).

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

### Permissions, sandboxing, security (areas 5, 21)

From [round1/synth/02-permissions-sandbox-security.md](round1/synth/02-permissions-sandbox-security.md).

Ranked by how much it matters for a self-hosted master/node harness that runs shell commands on other people's machines.

1. **No shell sandbox on `main` (F11, F12, F13). This is the single largest gap, and it is worse than it looks.** Seven of sixteen harnesses document an OS sandbox for the shell (two more unverified); Codex, Cursor, Antigravity and Zed confine shell commands by default or in the recommended mode. Ours runs `bash` as the node's OS user, able to read the data directory, with no network control (AUDIT S1). The Landlock sandbox exists only on the unmerged `integration/gate3` branch; until it merges, every compensating control we list (admin-only embedded node, `denied_tools`, file-tool confinement) is a permission-layer control, which every vendor in F34 says is not a boundary. Two blunt facts about the branch design: Codex rejects Landlock for filesystem-restricted policies because it cannot isolate Unix sockets, and our own branch notes the same gap (pathname sockets such as `docker.sock` and `ssh-agent` reachable below ABI 9) plus no UDP/DNS coverage. Its fail-closed `mode = required` default is the right call and is stricter than Claude Code's silent fallback (F12). Merging gate3 is the top priority in these areas; reporting `full`/`partial` enforcement per DeepSeek would make the socket gap visible to the operator.

2. **Every node holds provider keys (F22).** The field is moving to "workers never hold the model key": OpenClaw cloud workers, Oh My Pi auth-gateway, OpenCodex hub, Warp and Claude Code cloud all proxy inference or attach credentials outside the agent's boundary. In our design a compromised node, or an unconfined `bash` on a node, can read the provider key from the node's `config.toml`. The master already exists as a natural inference proxy; routing node inference through the master (or a node-local credential proxy outside the sandbox) would remove secrets from the most exposed machine. This interacts with the "harness stays light" invariant only lightly (it is a forwarder), and with "master never runs tools" not at all.

3. **Approval binding is per tool name per turn (F4).** One click on "allow for this session" allows every later `bash` command of that turn (AUDIT L10), and the label overstates it. The field ranges from prefix rules (Codex, OpenCode) to OpenClaw's argv, cwd, env and executable-hash binding. Gate3 fixes the label and the first-approval semantics; it does not add argument-level binding. Given that we have no sandbox, the binding is currently the only thing between one approval and arbitrary commands.

4. **No per-command rules and no shell parsing (F2, F3).** Fifteen of sixteen have rules; ours has `denied_tools`. Every vendor says Bash rules are UX, not security, so this matters less than items 1-3, but `ask` prompting for every `ls` is the prompt-fatigue failure that drives users to `auto`. If we add rules, the converged design is clear: split plain chains, treat anything with substitution or variables as opaque requiring exact match (Codex, Antigravity), refuse unparseable input (OpenCode), deny beats ask beats allow regardless of specificity (Claude Code). Hermes's "suggest, never apply" mining is a good fit for the event log.

5. **Where we are ahead: node authority and repo config (F9, F10).** "The node decides" is stronger than anything comparable: Claude Code's self-hosted runner and Cursor's workers take policy from the vendor control plane, and Claude Code says its own server-managed settings are a client-side control. OpenClaw's two-layer node pairing (device identity, then declared capability approval) is the closest peer and is worth comparing against our pairing. We also read no repository config at all, so the whole F10/F28 attack class (repo hooks running under `-p`, repo `.mcp.json`, repo settings widening allow rules) does not exist for us yet. The day we read `AGENTS.md` or any repo config, we need a trust prompt and a repo-ignored-keys list from the start.

6. **Where we are ahead: unattended runs (F17) and the approval event trail (F20).** Agentic routines cannot run in `auto` and park for a human for 24 hours; Cline schedules default to yolo, Cursor cloud agents never ask, DeepSeek's SDK pins full access. Every approval decision is a session event under one writer, which matches DeepSeek's best-in-class design; adopting its rule that a failed append rejects the request costs little.

7. **Where we are ahead: prompt-injection containment on the web path (F25, F27).** Provenance-gated `web_fetch`, an SSRF-safe resolver re-checked per redirect, untrusted webhook turns losing memory and web tools, and no model HTML: no other surveyed harness documents URL provenance. Two weaknesses on `main` (provenance set too wide, AUDIT L11; IPv6 transition-address gaps, L12) are fixed only on the branch. Note the asymmetry: these protections cover the chat runtime; agentic sessions reach the network only through unconfined `bash`, which bypasses all of them.

8. **Environment allow-list (F21).** Ours starts children from an empty environment plus an allow-list; Codex passes `*TOKEN*` variables by default and Claude Code's sandbox inherits secrets unless an opt-in scrub is set. This is a real lead, but it only covers the environment: files on disk (provider key in `config.toml`, data dir) are still readable by `bash` on `main` (item 1).

9. **No model-based approver (F5).** Twelve harnesses ship one; ours is docs-only. Do not rush it: every vendor caveats it (Cursor "not a security boundary", Claude Code "does not guarantee safety", goose fails open), and it is only safe behind a sandbox. If built, copy the converged safeguards: strip tool results from the reviewer's input, fail closed on reviewer error, circuit-break back to a human after a few consecutive denials, and drop broad allow rules while it is active.

10. **Secret redaction is a courtesy (F23).** Known-value scrubbing misses split or encoded keys on `main`; gate3's tail-holding stream scrubber helps. The field's stronger answer is item 2 (do not put the secret where the agent can print it).

11. **Things we lack that matter less here:** protected paths for `bash` (only file tools refuse `.git/**`; Codex and Claude Code protect `.git/hooks`, `.git/config` and agent config inside writable roots at the OS layer, which our gate3 sandbox could add cheaply), sub-agent report scanning (F26, cheap to copy), hardcoded dangerous-command floors (F6), container isolation for nodes (deliberately excluded by design), extension trust (no extensions exist), telemetry statement (we send none; say so in the README, since several vendors are judged on it).

### Context management and memory (areas 6, 7)

From [round1/synth/03-context-memory.md](round1/synth/03-context-memory.md).

Ranked by how much each matters for a self-hosted master/node harness.

1. **No overflow handling or `/compact` (largest gap).** Our trigger is a char/4 estimate against a fixed 24000 and a context-length error simply errors the turn (see Automatic compaction; Staged and fallback). Fifteen of 16 harnesses key compaction to the model's actual window or a reserve, and several add reactive compact-and-retry (OpenCode, Pi, OpenClaw, OpenHands, DeepSeek harness). On nodes with different models this is a correctness bug, not a polish item. The cheapest field-proven fix is overflow-then-compact-once plus a reserve (Pi 16384, OpenCode 15%).
2. **We do not read repo instruction files.** 14 of 16 harnesses read `AGENTS.md` or equivalents (see Instruction files). For a node that "owns the code" this means the agent cannot see the repo's own rules; our per-project instruction text lives on the master, not with the code. Note the trust questions the field already hit: Pi loads context files regardless of project trust, Claude Code gates out-of-cwd imports behind an approval, OpenHands and Hermes treat repo-shipped memory as untrusted text, Hermes blocks flagged files. The node (not the master) should read these, consistent with "the node decides".
3. **Memory approval: we are ahead of most.** Owner confirmation before activation is rarer than it looks: only Antigravity (`/learn`) and Hermes (opt-in, default off) have a gate in the surveys; Claude Code, Claude chat, OpenHands, OpenClaw, Codex write without approval. Our open S12 (project members can activate project facts) is the one real hole against this lead (see Who approves). Also `remember` is not registered on agentic nodes, so the feature that matters most for a coding harness does not write there.
4. **Memory scopes: user and project only.** The field splits on scope in ways we lack: per-repository (Claude Code auto memory keyed by git repo, shared across worktrees; Hermes Honcho per-repo), workspace, team (Warp), agent (Claude subagent memory, Cursor Bots). We have no per-repo or workspace-shared scope (see Memory scopes).
5. **Observation masking, output filter and caching are strong.** Our masking (batched to protect the cache), shell filter with retrievable full output, and per-session random output ids match or beat the field (see Pruning; Large output; Prompt caching). Only OpenClaw, OMP, Hermes and goose document anything similar for pruning; Claude Code documents none. No other harness documents cache-keeping batch masking.
6. **No context-usage display against the model window.** We show a total-token tooltip only; nine harnesses show fullness or a per-category breakdown (see Context usage display). It also depends on knowing the real window, which ties to item 1.
7. **No staged compaction, idle or background compaction.** OMP's chain, Hermes' ladder and OpenClaw's summary audit show what failures look like. Lower priority than item 1, but the summary audit and "full history stays in the log" are natural fits for our event log (our `history_summarized` event already keeps the log).
8. **Repo map: present but off by default.** We are one of two with a code map or index (Warp has embeddings). Worth turning on only when its cost is measured; the field offers little evidence either way.
9. **No agent-written skills, no consolidation, no memory import/export, no session-history search.** All are niche in the field (see Sections 4 and 5). Skills as memory should wait; if added, Antigravity's proposal-artifact and Hermes' staging queue are the approval shapes.

##### Evidence bearing on the four scopes and on human approval

The reader's question is how to design four scopes (workspace, project, user facts, per-repository) and whether agent-written memory needs a human approval step. What the surveys say:

- **Scope examples in the field.** Claude Code auto memory is per git repository, machine-local and shared across worktrees; Claude chat is global topics plus per-project with no cross-project reads; OpenHands has user and project tiers; Warp has personal, agent and team stores with read-only or read-write attachment; OMP has global, per-project, per-project-tagged; Claude Tag has channel and workspace notes (workspace notes written only from public channels, readable everywhere). Nobody documents all four of our scopes. A per-repository scope is only well defined where the harness runs next to the code (the node); a master-side store cannot key it by git repo without the node reporting an identity.
- **Isolation rules to copy.** Claude chat: project chats see project memory plus non-project memory, not other projects' (a one-way containment rule). Warp: attachment is per store and read-only or read-write. Hermes: profile distributions carry config and skills but never credentials or memory.
- **Approval: mixed evidence.** Gate: Antigravity `/learn` and ours write only after approval; Hermes offers a staging queue defaulting to off. No gate: Claude Code, OpenHands, Codex, Claude chat, OpenClaw (scored promotion only). Failure evidence from the field for no-gate designs: Cursor's docs warn that untrusted input can poison automation memory and that the SDK journal is shared across all sessions and users; OpenClaw admits promoted memories have no retention limit and deletion has gaps; OpenHands and Hermes treat injected memory as untrusted text and scan it. Nothing in the surveys measures how often users accept or reject a proposal, or whether an approval step is abandoned in practice; that is unverified. A design implication that follows from the sources: the shared scopes (project, workspace, team) carry the poisoning risk across users, so an approval step is better justified there than for a single user's private facts. This is an inference, not a finding from a survey.
- **Size discipline.** Hard caps force curation: Hermes (2,200 / 1,375 chars), OpenHands (about 6,000 chars, oldest lines dropped), Claude Code (200 lines or 25 KB, error on overflow forcing a rewrite), OpenClaw `USER.md` 4,000 chars. Ours: 500 chars per fact, top 6 recalled.
- **Injection placement.** Injected after history to keep the prefix stable (ours); frozen at session start with mid-session writes visible only next session (Hermes, Claude Code CLAUDE.md edits); wrapped as untrusted content (OpenHands).
- **Exclusions.** Incognito, per-chat off switch, untrusted turns and external-context chats (Codex) are the documented exclusion points; ours already has similar ones.

### Planning, sub-agents, background work (areas 8-10)

From [round1/synth/04-planning-subagents-background.md](round1/synth/04-planning-subagents-background.md).

Ranked by how much each matters for a self-hosted master/node harness.

1. **Unattended routines are the strongest part and also the most hobbled.**
   - **Ahead:** our webhook entry point (hashed per-routine secret, idempotency key, token buckets, sticky untrusted flag that strips memory and web tools; see *Event triggers*) is ahead of DeepSeek (no dedup), Cline (Telegram approves everything) and OpenHands (exposing the backend exposes the agent server). It is on par with Claude Code's fire-payload wrapping and OpenClaw's webhooks.
   - **Hobbled:** agentic routines cannot run without a human click (*Permission posture of unattended runs*). Codex runs scheduled tasks under its default sandbox and Claude under an auto-mode classifier. Once the `integration/gate3` Landlock sandbox lands, "auto inside a required sandbox, node-side" is the defensible middle that no competitor documents with node authority.
   - **Missing:** a model-free pre-check (Hermes `wakeAgent`) and a stated catch-up rule (Claude Desktop: one run within 7 days) beyond our "missed-run rule".

2. **We have no plan mode, and the field shows exactly how not to build one.**
   - DeepSeek's plan mode is prompt guidance that the sandbox ignores. Claude Code does not enforce plan blocks when bypass is available. Cline's plan mode lets `python -c` write. (*Plan mode*)
   - For us, plan mode must be a **node-enforced tool and policy state**, as OpenCode and the Antigravity CLI do it, recorded as an event (as DeepSeek logs `plan/mode`), with approval through the existing approval path.
   - Review UX worth taking: OMP's "execute in a fresh session with only the plan" and Antigravity's comment-on-artifact. (*Plan review*)

3. **Delegation is solid but shallow and synchronous.**
   - **Ahead:** worktree-per-writing-child with patch-back and a filter-driver refusal matches Claude Code and OMP (*Isolation*). Token plus count caps per turn are rarer than depth caps (only Claude `maxBudgetUsd` and OMP request budgets compare).
   - **Lacking:**
     - background children (9 harnesses document them) and resumable children (11);
     - structured output schemas (Claude, OMP, Hermes, DeepSeek);
     - a delegation-boundary output scan (only Claude);
     - a sub-agent "cannot do" list. Hermes and goose block memory, scheduling and messaging from children; we do not, though `remember` is chat-only anyway.
   - **Our approval model:** the child runs with the parent's approval mode, approvals inside the child. That is the Codex/Antigravity model, and it is the right one; Cline and OMP, where approving the spawn approves everything, are what not to copy.

4. **Model slots: we have per-agent model and fallback, and nothing else.** (*Auxiliary model slots*)
   - The reader's plan (small model for titles, vision bridge, summariser) is validated by the field. Hermes, OMP, Zed and OpenCode all have it. Design lessons:
     - **One slot per task, not one shared "small" model.** Claude's single `ANTHROPIC_DEFAULT_HAIKU_MODEL` also moves the goal judge and the `haiku` alias: a documented coupling trap.
     - **Default each slot to the main model** (Hermes, Zed) rather than auto-resolving from whatever is authenticated (OMP). With "the node decides", which model a node may call must be explicit.
     - **Give each slot its own fallback chain** (Hermes, OMP); our per-agent chain machinery can be reused.
     - **The vision bridge is a sidecar pattern** (OpenCodex: describe, cache, cap per turn). Its output is model-written text and must be treated as untrusted.
     - **Which model runs our history summarisation is not stated in the inventory.** Settle that before adding a summariser slot.

5. **Roster (`feat/agent-roster`).**
   - The field converges on 3 to 6 built-ins with a read-only explorer, a planner and a general worker (*Built-in agent roster*). Our branch's eight is on the high side, and three are worth attacking:
     - **"Smol"** (no tools, no memory) is a model slot dressed as an agent. OpenCode's title/summary agents show that shape and hide them. If its job is cheap side tasks, make it a slot. If it is a user-facing cheap chat agent, say so.
     - **"Senior Teamlead"** duplicates the delegation role that "Software Developer" already has (it delegates to scout/researcher/reviewer).
     - **"Researcher"** has web tools that agentic nodes do not register (inventory "Still true"), so it is chat-only.
   - **No harness ships a built-in "Writer".** Not wrong, but a reason to check it earns its place.
   - **Missing compared with the field:** a **Plan** agent paired with a plan mode (Claude, OpenCode).

6. **No goal loop and no task tool.**
   - Common now (*Goal / run-until-done*, *Task / to-do*). The judge design matters more than the loop. Hermes fails **open** on judge errors and Antigravity trusts self-declaration. OpenHands (invalid judge output counts as not done) and goose (shell checks that must exit 0) are the safer patterns.
   - Our hard 50-step cap would bound a goal loop only per turn, not across turns. A goal needs its own budget (Codex tokens and time, OMP tokens).

7. **Cross-harness sub-agents.**
   - `AgentBackend` exists with one implementation (*Other harnesses as sub-agents*). OpenHands, OpenClaw, goose and Zed prove ACP-over-stdio works. Every one of them leaves **approval handling for the hosted agent undocumented**: the gap we would have to close to keep "the node decides".

8. **Teams, swarms and script workflows: skip for now.**
   - Nearly all are experimental or preview, and their own docs admit costs (Cline says a single agent is often more efficient; Claude says the lead sometimes does the work itself).
   - Under the feature freeze these are the easiest to defer. The one idea worth keeping on file is Claude's deterministic replay of workflow results, because it maps onto our event log.

9. **Durability.** Closing an interrupted turn and denying pending approvals is on par with Hermes's `unknown` turn and safer than replaying. What we lack is OpenClaw's restart "resume input" and any durable background work: we have no background work, so nothing to make durable yet.

---

### Sessions and git (areas 11, 12)

From [round1/synth/05-sessions-git.md](round1/synth/05-sessions-git.md).

Our design in these areas: an append-only SQLite event log per session (one writer, the owning node; the UI a fold over the log) and per-turn checkpoints in a shadow git repo that never touches the user's `.git`. Ranked by how much each matters for a self-hosted master/node harness:

1. **Checkpoints: ahead, with named risks.** Shadow-repo undo per turn that works on non-git folders, with git called under a cleared env, no config and no hooks (see "File checkpoints and undo: the mechanism"). Only OpenCode, Hermes and Cline document comparable shadow-git designs; Claude Code and Cursor use non-git snapshots; Codex, DeepSeek, Oh My Pi, Pi, goose, Warp and OpenClaw document none. The surveys document no comparable hardening of the git invocation for OpenCode or Cline (nothing documented, not proof of absence). Risks the field has already hit: Zed removed per-prompt Git checkpoints because they slowed or hung large repositories (see "Checkpoint coverage, retention, costs and removals"); Cline documents large-repo slowness; Hermes caps size (500 MB total, 10 MB per file, 50,000-file skip); Claude Code is explicit about what is not covered (Bash-only changes, sub-agent edits, symlinks). Our inventory shows only a count cap (latest 10) and no size cap, no file-count skip and no list of what is not restored. Expect the Zed failure at scale unless that is measured. Also note the OpenCode warning that snapshot objects contain file contents (data-dir sensitivity) and that Hermes turns checkpoints off on container backends; the second is a trap for a node-decides model.
2. **Event-log session storage with one writer: ahead on invariants, behind on lifecycle.** Single-writer enforcement matches DeepSeek (preview) and OpenClaw (a claim checked inside the write transaction); Claude Code's equivalent is advisory and lets two terminals interleave. Behind: the log keeps every streaming delta forever (AUDIT D5, no log compaction), while Codex, Claude Code (30-day sweep, `claude purge`), OpenClaw (`pruneAfter`/`maxEntries`) and Hermes (prune, optimize) all ship retention. DeepSeek's zstd frames with torn-tail truncation is the one design that directly addresses growth and crash-torn writes in an append-only log.
3. **Fork, branch and edit-and-resubmit: absent.** 12 of 16 harnesses fork, 11 offer some conversation rewind, and Oh My Pi and Pi do it inside the log without copying. Because our log is already a single append-only sequence, the lowest-cost shape is the Pi-style parent pointer. Beware the OpenHands and Antigravity pitfall: a session fork does not fork file state; our shadow-repo checkpoints are the missing half and would let a fork carry a consistent file baseline, which no other harness documents.
4. **Export, share and import: absent.** 10 of 16 can export; Hermes (`--redact`) and OpenCode (`--sanitize`) show the minimum bar for a secrets-aware export. Sharing: OpenCode removed it, and Pi's share docs carry only a minimal warning; our "chats are private to their owner" invariant argues against building link sharing. Export is the cheap win.
5. **Moving a session between nodes: not in the inventory.** Codex (chat plus Git state between hosts), Warp (snapshot of tracked and untracked changes) and Claude Desktop/Cursor show the demand; Warp's snapshot-of-uncommitted-state is the mechanism closest to what a node migration would need. Our single-owner rule and per-node `node.toml` authority make this a design decision, not a copy.
6. **Worktrees: behind as a user feature, with a hostile-config stance that is ahead.** 14 of 16 offer user-facing worktrees; ours exist only for sub-agents. We refuse repos whose config defines filter commands; Claude Code (filter drivers, `includeIf`, LFS custom transfers) and Hermes (hooks, fsmonitor, filters off) are the only others documented doing the same. Known residual: git-lfs filters still run in our sub-agent worktrees (PLAN T19). Missing: ignored-file copy or setup scripts (7 harnesses), cleanup and dirty-work protection (OpenClaw snapshot refs and Hermes archive refs are the models), and any lifecycle doc.
7. **Diff view: behind.** We show old/new strings of one `edit` call and a per-turn file list. Warp, Antigravity, Claude Desktop and Cline offer whole-change review with inline comment or hunk revert. The shadow repo already holds before/after states per turn, so a per-turn diff is derivable without new capture.
8. **Commit, PR, review and attribution: absent, deliberately thin.** We have `git`/`gh` through `bash`, and the env allow-list keeps tokens off. 7 harnesses generate commit messages, 5 create PRs, 8 run hosted review bots, and only Claude Code and Cursor mark commits (the Claude Code cloud credential proxy is the one design aimed at keeping tokens off the worker). Under the feature freeze this is the lowest priority; the only item that follows from our own invariants is attribution, because agent commits made via `bash` currently carry no marker tied to a session id.

### Extensibility (area 13)

From [round1/synth/06-extensibility.md](round1/synth/06-extensibility.md).

Where we stand: rizzy-harness has none of MCP, hooks, skills, slash commands, plugins, custom tools, code mode, an SDK, ACP or config import (`own-rizzy-harness.md` §9 and (c)). What exists that bears on extensibility: agent definition files with strict frontmatter (`crates/agents/src/lib.rs:1-33`), validated `ui:` blocks, a REST/WS API with an admin-equivalent bearer token, routine webhooks, and the `AgentBackend` trait with one backend. There is a feature freeze until Gate 3 (`CLAUDE.md`, `FIX_PLAN.md:8-12`); every item below is post-freeze work and costs against the fix plan.

Table stakes now (by the counts in section 4): MCP client (16/16), skills (16/16), custom commands (14/16, and mostly implemented as skills), plugins (15/16, but as packaging), hooks (10/16). ACP is common (9/16) but optional for a web-first product. Code mode, harness-as-MCP-server and A2A are not table stakes.

Ranked by how much it matters for a self-hosted master/node harness, with the order to add them:

1. **Skills, read on the node (add first).** Cheapest gap with the widest adoption, no code execution needed to load one, and it gives slash commands almost for free (Codex, Antigravity, Claude Code, Hermes, Cline, Zed and Warp all make skills the command system: "Custom slash commands" entry). Read `.agents/skills/` (the only cross-vendor path) plus a user dir; show name and description, load body on demand (progressive loading is universal). What the surveys say goes wrong: listing overflow (Codex and Claude Code both had to add budgets and eviction rules), skills that run shell at load time (OpenCode's runs outside permissions; Claude Code had to add permission checks and a kill switch), project skills auto-trusted (Hermes, Zed and Pi require explicit trust), third-party skills as untrusted code (OpenClaw, Hermes scan; Pi warns), and skill frontmatter that silently grants tools (Claude Code `allowed-tools` clears next message). For us: skill files in the repo are data the model reads, so the "nothing the model writes is trusted" stance extends to "nothing a repo ships is trusted": no `!cmd`, no `allowed-tools`, no frontmatter hooks in v1. `AGENTS.md` reading (area 7) belongs in the same change.

2. **MCP client on the node, with deferral from day one (add second).** Every competitor has it, and it is how users bring their own tools. It must run on the node (the master never runs tools) and be authorised by `node.toml` (the node decides): no HTTP or protocol path may add a stdio server, because a stdio server is arbitrary code. Ship tool deferral at the same time: our loop already sends every schema every step (`crates/cli/src/main.rs:447-451`), and Pi's reversal on MCP (argued against it for context bloat, then shipped it behind codemode and deferral) shows the failure mode. What goes wrong per the surveys: project-supplied servers launching processes in headless runs without consent (Claude Code `-p`, SDK); servers outside the sandbox and outside permission rules (Claude Code plugins, DSH enables none by default for this reason); egress controls that do not cover MCP (Codex proxy); OAuth complexity and headless runs that cannot sign in (Claude Code); secrets in world-readable config (Claude Code `managed-mcp.json` warning); tool schemas the provider rejects (Claude Code normalises root combinators, drops invalid tools, retries parameterless); unbounded calls (Warp only recently added timeouts); tool descriptions and server instructions as an injection channel (Claude Code caps lengths; Hermes strips invisible Unicode). Our `web_fetch` provenance and SSRF code and `Secret<…>` handling are reusable here.

3. **Hooks, node-side and hash-trusted (add third).** Ten harnesses have them and they are where guardrails belong (Claude Code's own docs: instructions are requests, hooks are guarantees). For a node-decides design the right shape is Codex's: hooks configured only on the node, trusted by hash, plus a managed-only mode. Default PreToolUse to fail closed. What goes wrong: fail-open defaults on crash or timeout (Claude Code command hooks, Cursor, goose, OpenHands LLM hooks, Cline SDK, Claude Code mods); exit-1-means-allow traps (Claude Code warns policy hooks must use exit 2); parallel hooks whose side effects run even when a sibling denies (Claude Code); precedence where a lower source overrides a higher one (Cursor last-response-wins); repo-shipped hooks running under headless trust (Claude Code `-p`); a hook-provided env file sourced before every command (Claude Code `CLAUDE_ENV_FILE`, flagged by the survey as a trust surface). Claude Code's `defer` is the one hook feature worth copying beyond the basics, since our approvals already cross a link.

4. **Custom commands beyond skills: skip.** Once skills exist, separate templates add little; Codex and Antigravity deprecated theirs.

5. **ACP agent role on the node (later, optional).** Nine harnesses speak it, so a node that is also an ACP agent would let editors attach. But stdio is the only standardised transport and remote HTTP/WebSocket is still an RFD, so a node would have to tunnel stdio over our own link; and ACP permission requests are advisory with the agent storing "always" decisions, which fits our node-side policy only if we map ACP's options onto `node.toml`. The ACP client role (master hosting Claude Code, Codex and others through `AgentBackend`) is the bigger product lever, but OpenHands documents nothing about how approvals cross that bridge and goose maps its modes loosely; that is the part to design before building.

6. **Plugins and marketplaces (last or never).** Packaging matters only once there is something to package, and it is where the supply-chain stories are: no plugin sandbox anywhere except Cline's opt-in subprocess, auto-update letting a publisher ship code without approval (OMP `auto`), command and npm sources needing hardened installs (Claude Code), tamperable `http:` indexes (DSH). If built, bundle only declarative parts (skills, MCP config, hooks) and keep code plugins out.

7. **SDK and control protocol.** We have a REST/WS API but its token is admin-equivalent and there is no spec. Competitors with a clean protocol (Codex app-server's generated schemas and `experimentalApi` gate; Pi's JSONL RPC with `agent_settled`; OpenClaw's idempotency keys) are the reference. A scoped token and an OpenAPI document are higher value than an SDK.

Where we are ahead, honestly stated: not on features. On structure: (a) a single authority on the machine that runs tools (`node.toml`, `crates/node/src/policy.rs:9-105`) means any extension system can be clamped there, whereas most competitors gate repo-supplied MCP servers, hooks and plugins with per-folder trust dialogs that their own headless modes skip; (b) `ui:` blocks validated in Rust (`crates/core/src/ui/mod.rs:56-83`) are a safer rich-output channel than MCP Apps rendering server-supplied HTML (OpenHands Canvas Apps and Claude Code mods both run unsandboxed UI code); (c) untrusted-webhook turns that lose memory and web tools (`crates/master/src/lib.rs:257-297`) are the kind of provenance rule MCP results and skill content will need. Weakness to fix before any of this: the admin-equivalent API token and the per-turn, per-tool-name "allow" (`crates/agent-native/src/lib.rs:625`) would make an MCP tool approval far too broad.

### Models and observability (areas 14, 19)

From [round1/synth/07-models-observability.md](round1/synth/07-models-observability.md).

Ranked by how much it matters for a self-hosted master/node harness that runs unattended work and shares a master across users.

1. **No provider retry or backoff (largest functional gap).** A single 429 or 5xx either triggers a fallback model or ends the turn (`crates/agent-native/src/provider.rs:104-116`). The others document: Claude Code retries transient failures up to 10 times with exponential backoff, has an unattended mode that waits out 429/529 for hours, aborts quiet streams with four watchdogs, treats gateway `retry-after` above 60 s as "stop", and never retries a spend-limit 429. Pi's defaults are 3 retries, 2000 ms base, 60000 ms max, with provider-level retries 0 so layers do not multiply. Codex honours `Retry-After` and stops when the model is not found. OpenClaw backs off 30 s, 1 min, 5 min and disables a billing-failed credential for 10 minutes. OpenCodex cools a target from `Retry-After` then a configured delay. Hermes fires fallback only after retries (429/5xx) but immediately for 401/403/404. A node running a long routine overnight is exactly the unattended case Claude Code's watchdog exists for. See the retry feature entry.
2. **No cost tracking.** We record four token counts per turn and nothing in currency. The field computes cost locally from a price table and labels it an estimate (Claude Code `/usage`, `costBasis` in SDK results; OpenCodex "list-price equivalent, not billing" with coverage warnings; Pi billed vs estimated; Oh My Pi and goose carry per-model prices in config). OpenClaw shows the opposite choice: provider-reported spend, no dollars for subscription sessions. We already read per-model data from `/models`; a price field and a per-turn cost event would be the minimum. Claude Code's `modelPricing` multiplier (discount or markup, managed-only) is the design to study for team chargeback. See token and cost display.
3. **No budgets, and a dead `budget` field.** The agent frontmatter `budget: lean/standard` is parsed and never read, which is worse than absent because it promises something. Meanwhile the per-user limit is none and the main agent has only a 50-call step cap. The field's enforcement points are instructive: OpenCode Console enforces at request start (429); Claude Code's SDK cap checks after each response and can overshoot; Claude Code's gateway caps per user, group and org with daily/weekly/monthly windows and a pooled option, returning a non-retryable 429. For a shared master, per-user budgets belong in the master and should be checked before a turn starts. See budgets and limits.
4. **No export (OpenTelemetry or Prometheus).** Our event log already contains the facts (tool calls, approvals, usage). Field practice to copy: opt-in, off by default; project or repo config must not be able to set the destination (Codex, Claude Code); redact prompts by default (Codex) or make content capture a separate switch (goose); tag every datapoint with a prompt id so logs, metrics and traces join. DeepSeek harness is the warning: a full mirror with no default redaction rules exports message text and tool results. Our invariant "no secret in an event, a URL or a log line" makes the redaction default non-negotiable. See OpenTelemetry and other export.
5. **No raw provider request log.** Only goose documents one by default and flags that it holds raw prompts; Claude Code makes bodies opt-in; Pi and Oh My Pi expose hooks instead. Debugging provider quirks (the whole reason Claude Code's gateway guide and Oh My Pi's compat-flag matrix exist) is hard without it. If added, it must follow the secrets invariant and be off by default.
6. **Provider breadth and reasoning.** Two wire formats cover a lot through OpenAI-compatible endpoints, but there is no native Gemini, Bedrock or Vertex adapter and no Responses API (Codex accepts only Responses, so we cannot be a drop-in for a Codex-style provider). Anthropic models are forced to `effort=false` and reasoning text is not surfaced, so Claude users get no thinking control while Claude Code, Oh My Pi and Pi expose it. See provider support, cloud-platform, effort.
7. **No credential pools, rotation or per-person credentials.** OpenClaw ties credentials to a gateway profile per person; Hermes and OpenClaw rotate keys inside a provider before falling back across providers; OpenCodex pools accounts with thread affinity. For a self-hosted master with several users this matters more than for a single-user CLI, but pooling subscription accounts carries the legal risk Anthropic and OpenCodex both state. Key pooling for API keys does not. See routing and credential pools.
8. **No model governance.** Claude Code has `availableModels`, `deniedModels`, `maxEffortLevel` and `allowedProviders`; OpenCodex scopes `allowedModels` per client key. We have per-workspace default overrides only. An admin on a shared master will ask for this before asking for dashboards.
9. **Model roles for auxiliary work.** Hermes and Oh My Pi route compaction, vision, titles and approval review to chosen models; our history summarisation exists but the inventory names no separate model for it. Low cost to add given per-agent models exist.
10. **No fast/speed tier.** Not in the inventory; only relevant for providers that sell one.

Where we are level or ahead:
- **Fallback with a visible record.** Per-agent ordered fallbacks, a `model_fallback` event in the chat, and no switching mid-stream match OpenCodex's "only before output" rule. Claude Code's own docs admit its fallback is not visible in `/status` (a discoverability gap); ours is in the log.
- **Capability discovery.** Reading context, vision, tools and effort from `/models` with a daily refresh and keeping vanished models as `unavailable` is on par with Claude Code's gateway discovery and ahead of harnesses that rely on model-ID patterns.
- **Selection order and parallel answers.** A five-step selection order and up to 4 parallel answers with a pick; no equivalent found.
- **Key safety.** Same-origin-only redirects for stored keys echoes Claude Code's treatment of a discovery redirect as failure so credentials cannot leak.
- **No subscription logins.** Our deliberate stance avoids the legal limits Anthropic and Antigravity state, though it costs convenience that OpenCode, Hermes, goose and Pi offer.
- **Event log as trace.** The session log already carries what other harnesses add through OTel traces.
- **Vision with enforced limits** and **voice input** (OpenAI-style transcription, not stored) are comparable to the field; we lack TTS and realtime voice.

---

### UI, browser, collaboration (areas 15, 16, 18)

From [round1/synth/08-ui-browser-collaboration.md](round1/synth/08-ui-browser-collaboration.md).

Ranked by importance for a self-hosted, web-first, multi-user master/node harness. Blunt.

1. **Multi-user with private chats is our strongest card, and the field's open tiers are thin there.** We have accounts, workspaces, roles and private-by-default chats in the open tier (entry "Accounts, roles"). OpenHands keeps all of it behind Cloud/Enterprise; DeepSeek, Pi, goose, Oh My Pi document nothing; OpenClaw has multi-user in open code but says visibility is not a security boundary; Hermes has chat-platform admins only. The only free self-hosted multi-user admin plane in the survey is Claude's gateway, inside a closed binary. Keep this and say so in the README; do not let the freeze erode it.
2. **The web UI is a thin session viewer compared with the competition (entries "Web UIs", "Diff presentation").** We have no file browser, editor or terminal pane, no whole-turn diff, no inline comments back to the agent, and no plan review. Codex, Claude, Warp, Antigravity, Zed and Cline all have whole-turn or per-hunk review with comments that feed the next turn. For a web-first harness the review loop is the product; this is our biggest functional gap. The checkpoint already stores the file list, so the data is there.
3. **No plan review (entry "Plan review").** Six harnesses ship it (Y) and five more have a plan mode. A read-only plan phase with approval and comment-chips (Antigravity) is a UI pattern a web dashboard reproduces cheaply, and it matches `ask_all` plus a read-only agent we already have on an unmerged branch.
4. **Remote approval and "waiting for you" are half done (entries "Approval presentation", "Notifications").** Ours: browser Notifications in a secure context plus an Attention list. The field: push to a phone with presence suppression (Claude), iOS Live Activities (Cursor), approvals inside chat (OpenClaw), reverse-tunnel remote approval (Antigravity). We have no PWA manifest or service worker, so mobile is a responsive page that cannot push when closed. Also fix the "Allow for this session" label, which the field would call a bug (OpenCode's `always` writes real rules; ours covers one turn).
5. **No managed-config distribution; `node.toml` is local only (entry "Managed configuration").** The "node decides" invariant is stronger than Cursor's or Cline's (where policy does not reach every surface), but the field lets an admin distribute a policy. Codex's `requirements.toml` with a self-hosted config server and Claude's ranking, fail-open/fail-closed and "approval dialog for risky server-delivered settings" show what the master side of this needs. Design question: how can the master propose a node policy that the node may only tighten (OpenCode's layer "only tightens" plus the caveat that a policy allow can lift a policy deny).
6. **No SSO/OIDC (entry "Single sign-on").** Cheap relative to its adoption value; Hermes already offers OIDC on its dashboard in open code, and the Claude gateway shows the no-directory model (rely on IdP claims, no SCIM). SCIM is not worth building first.
7. **No budgets or cost in currency (entry "Budgets").** Only tokens per turn. The field enforces at the request path with 429 (OpenCode Console, Claude gateway) and decides fail-open vs fail-closed explicitly. Our master sits on the request path for chat; budgets are tractable.
8. **Audit table without a UI (entry "Audit logs").** The event log is a strong substrate (DeepSeek's `approval/asked`/`approval/decided` events show the idea); what is missing is browsing and export. Claude's one `safety_event` per stop, prompt or bounce is the model to match.
9. **No browser tool and no computer use (entries 13-16).** Twelve harnesses have an own browser. For a node-runs-tools design the browser would run on the node, which keeps "the master never runs tools", but the isolation statements (per-site approval, origin lists, credential vault) must come first. Do not copy "sandboxed" wording for a live-profile browser (Antigravity).
10. **No messaging channels (entry "Messaging-app channels").** Eight harnesses. Hermes and OpenClaw run the agent behind the channel; Codex, Cursor, OpenHands run cloud tasks. Pursue only after the freeze; Hermes's relay (gateway holds no platform tokens) is the shape that fits "no secret on the worker".
11. **No TUI, no IDE integration, no headless CLI (entries 1, 2).** Twelve harnesses have a TUI and eight an IDE path. For a web-first product this ranks low; ACP would cover both an editor client and other clients and only needs the master to expose a session over a remote transport (the remote transport is still a draft in ACP).
12. **Mobile is responsive web only.** Nine harnesses have more. A PWA with web push is the minimum.
13. **Where we are ahead or at par.** Rich validated `ui:` blocks with answers checked against the log, per-turn Undo/Rewind visible in the conversation (shadow-repo checkpoints), parallel answers from up to four models, the rendering-safety invariant (no model HTML, no remote images, links show host), voice dictation with per-account limits. Compare Cursor Canvas and OpenClaw's `show_widget` render-only panel: ours is validated in Rust; theirs is not described that way.

### Remote execution and architecture (area 17)

From [round1/synth/09-remote-distributed-architecture.md](round1/synth/09-remote-distributed-architecture.md).

Ranked by how much it matters for a self-hosted master/node harness.

1. **Ahead: the execution side is the authority.** No other surveyed design documents a worker-side file that the control side cannot widen (2.4 "Execution-side authority"). The closest public analogue, Claude Code's self-hosted runner, has the control plane send tool lists and permission rules per session; the runner adds only a floor. Cursor's server issues the tool calls and relies on the OS user; Warp pushes resolved config; OpenClaw's Gateway allowlist gates node commands. This is our strongest differentiator and should be stated as such in the docs and kept under test (`crates/master/tests/hostile_node.rs` covers the hostile-node direction; we also need the hostile-master direction).
2. **Behind: every node holds a long-lived provider key, and the node shell is unsandboxed on `main`.** The field is moving keys to the control side or the boundary: OpenClaw `worker-turn` proxies inference through the Gateway, Cline sends the key per session and never writes it, Oh My Pi and OpenCodex centralise credentials, Codex and Claude Code inject at an egress proxy (2.4 "Central credential broker", 2.5 "Credential injection"). With AUDIT S1 open, a prompt-injected `bash` can read the node's `config.toml`. The fix order matters: the Landlock sandbox on `integration/gate3` closes the read path; after that, decide between (a) node-held keys behind the sandbox (current design, simplest) and (b) master-held keys with an inference relay, which keeps keys off nodes but makes the master see every prompt (it already mirrors every event, so the privacy cost is small) and adds a single point of failure. This is a design decision, not a fix; it should go to the owner before Gate 3 work on the link.
3. **Behind: no per-session or per-dispatch credential on the link.** We have a good enrolment (single-use hashed join token, Ed25519 signature bound to the pinned cert), but after that one long-lived key does everything (2.4 "Per-dispatch credentials"). Claude Code (work orders, session JWT), Cursor (per-claim session token) and OpenClaw (per-dispatch credential stored as a hash) all scope what a stolen token can do. For us the risk is lower because the node dials and holds its own key, but a per-session token would let the master revoke one session without dropping the node.
4. **Behind on `main`: unbounded link back-pressure (AUDIT L1).** Codex is the only one documenting overload behaviour (`-32001`). Fixed on `integration/gate3` with bounded channels; until merged, a registered node can make the master buffer without limit.
5. **Behind: no headless or CI path for agentic work.** 13 of 17 have a one-shot headless run, 11 a structured stream, several an official CI action (2.1, 2.2). We have an HTTP API and routines, but unattended agentic routines can only run in `ask` modes, so a scheduled agent cannot change anything without a human click. This is a feature and the freeze applies; the cheapest honest step is documenting the API-plus-routine path. If built later, copy Claude Code's `defer` (end the turn, resume on decision) rather than blocking a turn for 24 h.
6. **Equal or ahead: who dials and how pairing works.** Outbound-only from the execution side is the norm and we match it (2.0). Our pairing is stronger than the UI-to-server designs (OpenHands no auth by default, Codex non-loopback unauthenticated during rollout, goose shared plaintext secret) and comparable to OpenClaw's device challenge. Loopback by default for the UI matches the best (DeepSeek harness, OpenClaw).
7. **Equal: reconnect and restart.** `Resume{after}` replay plus "restart closes the turn as interrupted and denies pending approvals" matches OpenCode's durable log and Hermes Agent's "unknown, never replayed" rule (2.3 "Reconnect"). Missing: a documented rule that a session bound to an offline node never runs elsewhere (OpenCodex's no-fallback) and fencing of a node that returns with stale state (OpenClaw).
8. **Ahead on ownership of data that leaves the node.** Our node mirrors every event to the master, which the owner runs. Claude Code's runner and Remote Control store transcripts at Anthropic; Cursor's worker sends file contents and terminal output; Warp stores transcripts by default (2.0, column "Data leaving"). This is the point of self-hosting and should be the headline comparison.
9. **Behind, lower priority: no ACP, no hand-off between nodes, no health or metrics endpoint.** ACP (11 of 17) would let editors talk to a node session or let a node host other agents, but ACP's remote transport is still an RFD and its stdio model fits a node-local adapter, not the master link. Hand-off (Codex, Claude Code, Warp) maps to moving a session between nodes with its Git state; our shadow-repo checkpoints are a good base. Health is `doctor` only; Claude Code's runner and OpenCodex show the minimum (liveness, readiness, drain).
10. **Not applicable by design: vendor cloud agents, cloud environments, warm pools, idle pause.** These solve "the vendor owns the machine". Our nodes are persistent machines the user owns. Skip, except the per-session sandbox idea (OpenHands per-conversation Docker, Claude Code capacity 1 ephemeral runners) once Landlock lands.

### Configuration, licence, other (areas 20, 22, 23)

From [round1/synth/10-config-licence-distinctive.md](round1/synth/10-config-licence-distinctive.md).

Ranked by how much it matters for a self-hosted master/node harness. The surveys and the own inventory are the only evidence.

1. **Node authority over config: we are ahead.** `node.toml` is the node's only authority, approval mode is `min(requested, node max)` (`crates/core/src/policy.rs:19-23`). The nearest peers are Codex `requirements.toml` and the config server, Claude Code managed settings (whose own docs call server-managed settings "not a security boundary") and OpenCode's `policies` layer. Cline's remote config does not reach CLI or Desktop at all. See "Managed (admin) policy layer" and "Precedence".
2. **Reading repo instruction files and trusting repo config: we have nothing, and the rest of the ecosystem has both.** Ten of seventeen read other harnesses' instruction files (see "Reading other harnesses' configuration"); we read none (`crates/master/src/projects.rs:1-12`). When this is added, the field already shows the safe shape: a key blocklist for repo-local config (Codex), trust-gated keys with `deny` and `ask` applying at once (Claude Code), a trust gate for all project resources (Pi), review settings read from the base branch (Cursor Bugbot). Claude Code's `-p` mode treating any folder as trusted is the documented failure to avoid. This is a gap, but it sits behind the feature freeze.
3. **Precedence documentation: partial.** Our order (`.env`/`HARNESS_*` over `config.toml`, `node.toml` authoritative) works but only the env override is documented in the inventory. The field's failure modes are Cline (project vs global contradict across pages), Warp and Zed (nothing documented) and Claude Code's per-key env rules. A one-table precedence statement is cheap to write.
4. **Programmatic config editing and hot reload: partial.** Settings UI edits `config.toml` and providers hot-swap, but invalid-file handling, atomic batch writes (Codex) and optimistic revisions (DSH) are not in the inventory. Models to compare: OpenCodex (rename bad file, load defaults), OMP (set aside, stop), OpenClaw (unknown keys stop the gateway; no automatic last-known-good).
5. **Licence and third-party terms: undefined in the inventory.** Our project licence is not stated there. What the field shows: MIT and Apache-2.0 dominate; Warp's AGPL client is ideas-only for us; Claude subscriptions cannot be used by a third-party harness for login or quota (corrected: an unmodified `claude` binary signed in by the user is allowed); Antigravity forbids third-party tools on the consumer service. See the licence table and "Third-party use of vendor subscriptions".
6. **Profiles and presets: weaker.** We have custom agents and provider presets; Codex profile files, OpenHands agent profiles (which scope secrets server-side) and DSH profiles go further. Relevant if a master serves several nodes or teams.
7. **Personas and output styles: roughly even.** Custom agents with extra instructions and project instructions cover most of it; no global persona file (Hermes `SOUL.md`) and no switchable style (Claude Code output styles). Low priority.
8. **Themes: even; key bindings and status lines: not applicable** to a web UI at the depth TUI harnesses document. We have light/dark/system, accents and localisation.
9. **Diagnosis: ahead of most.** `doctor` checks config and `node.toml` parse and token file mode; only Claude Code (`doctor`, `--safe-mode`) and OpenClaw (`doctor`, `triage`, `backup`) go further.
10. **Distribution and updates: behind, by design.** No tagged release, no installer, no self-update, Unix and Linux amd64 only. The field offers a native installer plus package managers (Claude Code, Codex, OpenCode, Hermes) and explicit channels (Claude Code `stable`/`latest`, OMP canary/stable). The "No compatibility code before the first tag" rule means migrations come only after tagging.

## C. Corrections, contradictions and unverified claims

### Agent loop, tools, editing, shell (areas 1-4)

From [round1/synth/01-loop-tools-editing-shell.md](round1/synth/01-loop-tools-editing-shell.md).

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

### Permissions, sandboxing, security (areas 5, 21)

From [round1/synth/02-permissions-sandbox-security.md](round1/synth/02-permissions-sandbox-security.md).

What the reviews changed:
- Codex: "network off by default for workspace-write" not confirmed on either page and moved to Unverified; "WSL1 dropped at 0.115" removed; permission profiles do not require 0.138.0 (only a mixed-version rollout note); project config blocklist also includes `apps_mcp_product_sku`; hooks page gives no GA label.
- Claude Code (review-1): plan mode also runs classifier-approved commands when auto mode is available; SDK `failIfUnavailable` default true moved to Unverified (CLI docs imply off). Review-1 also flags that two entries disagree on when project `defaultMode: bypassPermissions` started being ignored (v2.1.257 vs Week 36), and that the auto-mode classifier engine entry at line 1062 is stale relative to line 705.
- Claude Code (review-2): mods fail open by default; the built-in guard loads only with managed settings, `managed-mcp.json` or a Team/Enterprise login, so API-key users get no guard and a user mod can approve past deny rules; even with the guard, a mod can read `.env` via `$.fs.read`.
- Claude Code (review-3): sandbox credential masking (sentinel, needs experimental `tlsTerminate`, sandboxed Bash only, macOS blocks masked files) and cloud network secrets (no sentinel, key never in the VM) are different mechanisms, not "the same pattern"; work orders register exactly one runner in orchestrator mode, not one per session; Remote Control has no documented lease; the claude.ai login restriction has a carve-out allowing an unmodified `claude` binary signed in by the end user.
- Claude Desktop: Cowork has both local-VM and cloud modes; the getting-started article's cloud-only statement is stale. Chat egress has a fourth option and non-Team defaults.
- Cursor: hook precedence is deny > ask > allow for permission decisions but last-response-wins for other fields (not "most restrictive wins"); Windows sandboxing not documented; a `bubblewrap` status value unexplained; Legacy Privacy Mode statement is on the cloud security page.
- Antigravity: `toolPermission` not mentioned on `/cli/modes`; "Preview" label for business sandbox not found; bubblewrap confirmed only on `/enterprise`; allow-rule-to-mount wiring documented for the App only; `trustedWorkspaces` semantics partly documented.
- goose: the original "no sandbox documented" was not supported; a macOS Desktop sandbox (`GOOSE_SANDBOX`) appears in search snippets but its page 404s, so it is unverified.
- Hermes: container approval bypass is on the security page, not the tools page; iron-proxy binds the Docker bridge gateway (loopback on Docker Desktop), never 0.0.0.0.
- OpenHands: hooks are not part of the documented security model (only a proposed fix); the FAQ's no-auth statement is general, not "open-source build".
- OpenClaw: cloud workers are beta, not GA, and "outbound-only" is not stated; env binding of approvals is conditional; per-node pairing token removal dated (store Jan 2026, RPC 2026.7).
- OpenCode: a policy `allow` can lift an earlier policy deny; "Console policies persist when unreachable" dropped.
- Warp: the denylist bypass applies only to the explicit "Run until completion" shortcut, not to "Always allow".
- Cline: hub roles are per session participant; Kanban CDK root and token-in-user-data are inferences.
- DeepSeek: `$DSH_HOME/.credentials.yaml` is on the providers guide, not the credentials page; SSH subsystem page exists on GitHub.
- OpenCodex: remember-device is a 30-day record expiry and off by default; pairing code single-use with five-minute lifetime; rate limit is ten failed codes per ten minutes.

Where a vendor's own pages disagree:
- Codex: the retired `untrusted` approval policy "can stop clients starting", yet managed `allowed_approval_policies` "must include `untrusted`".
- OpenHands: default confirmation policy ConfirmRisky (architecture page) vs "not stated" (guide); LLM analyzer inline-only vs also a separate LLM.
- Cursor: cloud repository copies deleted after the run (privacy page) vs VM snapshots with code kept 30 days and not deletable (cloud security page).
- Cline: "every action requires your explicit approval" (overview) vs CLI auto-approve default true, schedules default yolo, SDK default-allow; YOLO enterprise control "all interfaces" vs "no surface"; `--auto-approve` vs `--yolo` naming.
- Antigravity: browser called "sandboxed" in the slash-command catalogue while it drives the user's real Chrome profile.
- Claude Code: auto mode "available to all users on every provider" vs model- and provider-gated.
- DeepSeek: session telemetry default `FEEDBACK_ONLY` (CLI README) vs no default stated (subsystem page); maturity labels for Auto review inconsistent across pages.
- Hermes: plugin page says 27 lifecycle hooks but lists 38.
- OpenClaw: `telemetry off` and `DO_NOT_TRACK` leave the daily check on (documented but easy to misread); prompt-injection page cites an unsourced attack-success test.
- Zed: sandboxing has no maturity label and appears in release notes only as a bug fix.

What nobody could confirm:
- Claude Code: how the server-side tool-result probe works; the exact variable list `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` removes; the full built-in classifier rules and read-only Bash list (printable only by running the CLI); cloud VM isolation technology; mods' isolation level ("worker thread").
- Codex: relay architecture and encryption for remote control; per-task isolation model of Codex Cloud.
- Antigravity: Auto evaluator mechanism; `sandboxProxy` details; Remote Control relay and encryption.
- DeepSeek: Auto review reviewer model, rules and prompt; default sandbox mode.
- goose: Smart Approval criteria; malware check data source; macOS sandbox existence.
- Cursor: worker wire protocol and encryption ("not documented" per the docs).
- OpenCode: telemetry of the local app; any OS sandbox.
- Pi: what install telemetry sends; whether `auth.json` and sessions are encrypted at rest.
- Oh My Pi: `eval` kernel isolation and tool-callback bridge security.
- Cline: hub authentication; meaning of `CLINE_SANDBOX`.

### Context management and memory (areas 6, 7)

From [round1/synth/03-context-memory.md](round1/synth/03-context-memory.md).

Corrections applied (from reviews):
- Claude Code idle compaction: the survey had three conflicting entries; review kept "200,000 tokens", "one-hour cache TTL" and `idleCompaction: false (v2.1.290+)`, and moved "about 54 minutes", "Terminal 2.1.299, Oct 14, 2026", "/config row 2.1.302", "about an hour idle", "machine awake" and the 100,000 floor to Unverified.
- Claude Code `promptCacheTtl`/`subagentPromptCacheTtl` version is v2.1.242 (docs page), not 2.1.243.
- Claude Code `.claude/AGENTS.md` being read was not found in memory.md (Unverified).
- Oh My Pi: idle compaction is off by default, not "optionally on"; 20k keep-recent is a summarising-compaction target; `agents` and `codex` instruction providers are tied; scope limits added; per-backend memory tool availability and mnemopi retain interval (every 4 turns) added.
- Zed/ACP instructions: first match wins, in the listed order of nine files; project `.rules` still supported.
- goose tool deferral: mechanism is on the config-files page (names up front, schemas via `enable_tools`), not undocumented.
- Cursor: Memories launched in 1.0 beta (changelog); the forum reply was not shown as staff and two other threads were not opened.
- OpenCodex: the 256 MiB memory accounting figure was removed (not found).
- Pi's "under 1,000 tokens" prompt claim is from a 2025 post, not current docs.

Contradictions between a vendor's own pages:
- Codex `project_doc_max_bytes`: combined (AGENTS.md page) vs per file (config-advanced page). Codex memories "off by default" vs 0.165.0 "memory v2 default for fresh profiles" (the latter is the store version).
- Hermes: sessions page says compression creates a child session, compression page says in-place is the default (`in_place: true`); README says FTS5 session search uses LLM summarisation, memory and sessions pages say it does not; prompt-assembly page puts the skills index in two tiers.
- Oh My Pi: docs/memory says no named memory tools, docs/tools and README list `retain`/`recall`/`reflect`/`memory_edit`/`learn`; README says a `skill://` scheme exists, skills doc documents none; whether TTSR injections survive compaction (README claim) is unverified.
- Antigravity: `/context` page does not mention `/compact` while the changelog does.
- Zed: the Instructions page lists nine files and also says the Zed Agent loads `AGENTS.md`, `CLAUDE.md` and copilot-instructions; unclear whether `.rules` and the legacy names are still loaded by the Zed Agent.
- OpenClaw: dreaming is default-on yet promoted memories have no time-based retention (project's own admission); its page lists `TOOLS.md` and `HEARTBEAT.md` that the file map does not.
- OpenHands: `max_size` vs `max_context_length` for the same value; the guide gives no product defaults while the architecture page gives 120/4.
- Cline: Auto Compact and Memory Bank pages appear legacy (not in the llms.txt index); Auto Compact page still mentions todo lists after Focus Chain was deprecated.

Not confirmed by anyone:
- Claude Code: which model summarises; the 100K-1M window range and `CLAUDE_CODE_MAX_CONTEXT_TOKENS` detail (page truncated); whether old tool results are cleared.
- Cursor: whether Memories still produce agent-visible memories; codebase indexing, embeddings and `.cursorindexingignore`.
- Antigravity: whether an earlier Knowledge feature existed; algorithm and thresholds of compaction; `/codesearch`.
- goose: the context window report and "context relevance operation" (v1.58.0 notes); the Memory extension mechanism; whether the "Top of Mind" extension is the MOIM persistent-instructions feature.
- Warp: where embeddings are computed; Agent Memory retrieval mechanism.
- Claude Desktop: memory import/export article (not opened); Hermes Honcho hosting details; OpenClaw `workshop` skills tier mechanism; OpenCodex prompt layers; DeepSeek harness compaction thresholds (generated config catalogue not opened).
- Pi: mechanism of idle cache warming; how `context_edit` is created.
- OpenClaw dreaming numeric thresholds are not on the page (review).

### Planning, sub-agents, background work (areas 8-10)

From [round1/synth/04-planning-subagents-background.md](round1/synth/04-planning-subagents-background.md).

**Changed by review (these areas):**
- **Claude Code.** Background sessions, the supervisor CLI, agent view, `--bg`, the daemon and attach/logs/stop are **research preview**, not GA (review-2 corrections 1 and 2). The agent-view introduction version (2.1.139) is not on agent-view.md (correction 3). Cloud scheduled tasks are the product formerly called routines, with a **1-hour minimum interval**, and the 30/hour per-task cap is one shared count (correction 5). "Skipped up to 72 hours then turns off" moved to Unverified (review-3). Plan mode also runs classifier-approved commands when auto mode is available (review-1 correction 1).
- **Claude Desktop.** `/schedule` was not removed (typing it requests a schedule); Cowork versus local scheduled tasks are two products, not a contradiction; Dispatch's 30-minute approval window comes from the desktop page, not the support article.
- **OpenHands.** DelegateTool is "retired", not "replaced".
- **DeepSeek harness.** The agent-team page exists on GitHub, not on the docs site; `write_scopes` are advisory; reminders "do not guarantee exactly-once" (the page does not say "at-least-once").
- **Antigravity.** `/teamwork` horizon is "multiple days".
- **Warp.** Factories are "Early Access". Orchestration "wake on message" and "one worktree per agent" are not on the cited pages; orchestration has no status label.
- **OpenClaw.** Cloud workers are beta, and "outbound-only" is an inference. The writer claim is verified in the commit transaction, not written in it.
- **Pi.** Background-owned work survives the parent's abort in pi-durable; the to-do non-feature reason is "confuse models".
- **Cline.** Hub roles are per session participant.
- **Codex.** The presets 8,000-byte limit moved to Unverified.

**Vendor pages that disagree:**
- **Claude Code** sub-agent nesting went 5 → 0 → 3 within six weeks (the digest still says 5). `/fork` has meant three things, and today depends on whether agent view is on. Cross-session messaging works on "any of your machines" (changelog) or the same machine (digest).
- **Hermes** delegation `max_iterations` is 250 on the delegation page and 50 on the agent-loop page.
- **Cline** says plan mode "cannot execute commands" on one page and only blocks mutating commands in sub-agents and schedules on others. Kanban auto-PR is described on one page and not on another.
- **OpenCode** README (plan asks before bash) disagrees with the V2 permissions page. `/goal` and schedules exist only in the changelog.
- **OMP** README "schema-validated typed results" disagrees with the docs, where schemas come only through `output`. The README lists 9 roles; the docs list 10.
- **goose** says sub-agents are disabled outside autonomous mode, which conflicts with using approvals and delegation together. v1.55.0 removed "background subagents and orchestrator".
- **Antigravity**'s App page still says "Fast mode" after the CLI removed `/fast`.
- **DeepSeek** schedules are labelled experimental on the landing page and unlabelled in the guide.
- **OpenClaw**'s home page still advertises "heartbeat scheduling" after the heartbeat was retired.

**Nobody could confirm:**
- what Codex's built-ins `default`/`worker`/`explorer` actually do, and the default of `max_concurrent_threads_per_session`;
- how OpenCode's `/goal` reviewer works;
- whether Antigravity timers survive closing the app;
- OpenClaw's sub-agent depth defaults (subpage unopened);
- Hermes webhook details (page unopened);
- Warp scheduled-agent timezone and overlap behaviour;
- Cline hub behaviour after the hub stops, and zen mode;
- the model behind Claude Code's compaction (review-1 omission 3) and WebFetch extraction;
- the model behind DeepSeek's title generation;
- OpenHands' routing and fallback guides (index only);
- the Claude Code changelog-only variables `CLAUDE_CODE_SUBAGENT_EFFORT` and `CLAUDE_CODE_WORKFLOW_SUBAGENT_MODEL`.

---

### Sessions and git (areas 11, 12)

From [round1/synth/05-sessions-git.md](round1/synth/05-sessions-git.md).

Corrections applied (from the reviews):
- OpenCode session sharing: was "nothing documented"; it was discontinued in v1.19.1 (15 Oct 2026). The V2 config `share` field statement comes from a search snippet only and the V2 config API page returned 404 (corrected in review).
- Zed checkpoint removal: removed in 1.27.0 (2026-11-04); the Agent Panel page is silent (there is no removal note on it); 1.25.2 is dated Oct 21 2026 and 1.23.3 Oct 12 2026 (corrected in review).
- Codex Handoff: moves a chat and its Git state between the local computer and a connected remote host; the phrase "to a worktree" is not on the remote-connections page. (The Local/Worktree handoff comes from the git-worktrees page.)
- Warp Handoff: `/handoff` confirmed; the detail "blocked while a command runs or child agents are active" was not on the pages opened and is unverified; the 2026.10.15 and 2026.09.23 periodic-checkpoint changelog claims were not checked. Warp multi-agent "one worktree per agent" advice was not found on the cited pages.
- Claude Code agent-view related status: "GA" was wrong; background sessions, agent view, `--bg` and its background-worktree isolation and PR-status rows are research preview (review 2). Background worktree isolation is labelled research preview here.
- Claude Code checkpoint limits (100 checkpoints, 30-day sweep, six menu actions, Bash and subagent exclusions, symlinks) were first unchecked in review 1 and then confirmed by review 2.
- OpenClaw per-session writer claim: the commit transaction verifies the claim; a state-dir lock is not on the queue page (corrected in review).
- Cursor cloud retention: indefinite by default, 30-day VM snapshot inactivity deletion, snapshots not deletable on demand (reviewed).

Contradictions:
- Cursor: the privacy page says Cloud Agents store encrypted repository copies only while running, then delete; the cloud security page says VM snapshots with cloned code are kept 30 days after inactivity and cannot be deleted on demand. The wording contradiction is real (reviewed).
- Cline: the checkpoints page and the IDE page name the three restore options differently ("Files only / Task only / Files and Task" versus "Task and Workspace / Task Only / Workspace Only"). The prompt-storage page names `~/.cline/data/tasks/` while the CLI layout names `data/sessions/`.
- Codex: GitLab MR review is "beta" on the GitLab page and "preview" on the code-review page.
- Cline Kanban: the overview mentions auto-commit/auto-PR settings, core-workflow does not describe auto-PR.
- Zed: `session/resume` and `session/load` stabilised Feb-Jun 2026, but ACP session fork exists only as an RFD title in the index.
- DeepSeek harness: the persistence page does not give the storage root; the Python SDK page gives `<dsh_home>/sessions` for the minimal profile only.

Unverified or unconfirmed:
- Antigravity `/rewind` and `/undo`: whether files are restored; checkpoint storage location and retention.
- Oh My Pi `/dump` secrets warning (not on the sessions page); `/annotate` is README-only.
- Pi `git-checkpoint.ts` and `auto-commit-on-exit.ts`: names only, contents not opened.
- Cursor "Shared Transcripts" help page not opened; Cursor Origin pages not opened; Cursor commit signing is a vendor statement.
- Hermes PR reviewer and webhook guides not opened; Kanban PR completion contracts claim comes from the llms index.
- OpenClaw: no built-in commit/PR/review flows found; CHANGELOG not opened; the managed-worktrees page text was truncated (30k chars unread), the survey's numbers were confirmed in review.
- goose: `goose review` flag semantics; `--no-orchestrate` implies an unconfirmed orchestration default.
- Codex: `/export` and `/rename` details and the exact CLI flags (ConfigTable components do not render in the `.md` twins).
- Claude Code: changelog-derived version and week claims (for example the `/fork` rename history) were not independently verified; "Resume across runners" is beta and was not checked by the reviewers.
- Claude Desktop: the session sidebar, worktree defaults, PR monitoring and diff review were not checked in review (the author's text only).
- ACP: `session/fork`, `session/notices` and others are RFD titles only.
- Hermes `/developer-guide/gateway-internals` citation for crash behaviour: copied as the survey writes it (the evidence-path convention in the Hermes file is relative).

### Extensibility (area 13)

From [round1/synth/06-extensibility.md](round1/synth/06-extensibility.md).

What the reviews changed:
- Claude Code hooks: 33 events, not 31 (review 2, correction 4; full list in the hooks entry).
- Claude Code mods: confirmed, but the review adds that mods fail open by default (a throwing or timed-out hook is skipped unless the author adds `.catch`), that the `cc-plugin-sec-default` guard loads only with managed settings or a Team/Enterprise login (API-key users get no guard), that a mod can read `.env` via `$.fs.read` and run processes regardless of Read deny rules, and that managed PreToolUse hooks run before mods while other PreToolUse hooks run after them (review 2, "Mods" section).
- Codex hooks: the page states no maturity label; "GA" was the survey's inference. Trust-by-hash, handler types, the 8-hook async cap and 2,500-token default confirmed.
- Codex import: the 50-chat, 30-day cap is CLI-only; hooks, slash commands and subagents are also imported; project memories only from Claude Code.
- OMP hooks: `session_stop` gives one continuation only. OMP instruction discovery: agents and codex providers are tied, with scope limits.
- Pi codemode: "failed scripts do not roll back tool calls" and the output-cap and store-limit details were not found and are unverified.
- Cursor hooks precedence: not "most restrictive wins"; deny > ask > allow for permission fields, last response wins for other fields.
- OpenHands: hooks are not part of the documented security model (only a proposed fix).
- goose: tool deferral mechanism is stated in one line on the config page; the GDK ACP page does not mention Zed (Zed use is on `zed.md`); the adapter binary is `claude-agent-acp`.
- DeepSeek harness MCP: the `mcp` page lives on GitHub, not the docs site; it documents no OAuth, no approval flow and not the exact tool-name format.
- Warp Hosted Factory MCP: server from 2026.08.18; automatic access for Claude Code and Codex from 2026.09.09.
- Zed/ACP: ACP clients number 142, not "dozens"; Zed instructions use a first-match rule over nine files.

Where a vendor's own pages disagree:
- Claude Code: MCP startup timeout 30 s (quickstart) vs a 5 s connect cap (`mcp.md`); two MCP statements disagree on whether "Clear authentication" revokes upstream tokens; plugin evals, monitors and themes are presented as available but sit under `experimental.*` manifest keys.
- Codex: the app-server is "experimental and isn't supported for production workloads" yet it is the only replacement for the removed MCP server and the stable Python SDK is built on it; Code Mode is "under development and off by default" while releases ship Code Mode features.
- OpenCode: the HTTP API is labelled experimental while the SDK pages present it as the contract.
- Pi: the CLI page says codemode and tool search are off by default; the settings page says on; the MCP page says MCP tools default to `codemode` exposure. The homepage lists MCP among deliberate non-features while noting it is built in.
- Antigravity: the plugins page says hooks can be "prompt evaluations"; the hooks page documents only `command`.
- OpenClaw: Tool Search is under "Experimental features" yet on by default; plugin APIs are all called experimental while plugins are GA.
- Hermes Agent: Tool Search called "opt-in" while default `auto` behaves as on; the plugins page says 27 hooks and lists 38.
- goose: SubagentStart/Stop listed among hook events but "not emitted"; the ACP providers page mentions a `copilot` binary not in its supported list.
- OpenHands: the MCP architecture page says the SDK spawns servers over stdio; the overview says SSE and SHTTP too.
- Cline: the docs hooks page is a stub; file-hook event names beyond `PreToolUse` are undocumented.
- Zed: "elicitation" in ACP release notes vs "MCP elicitation not supported": same word, different features.

What nobody could confirm:
- Claude Code: the 1% context budget for the skill listing (only the 1,536-char cap was seen); the 5 s `alwaysLoad` connect cap and the 2,048/4,096 description caps; the full SDK control protocol; mods' JavaScript engine and isolation beyond "worker thread"; the mod `decider: "person"` (changelog only).
- Codex: the presets 8,000-byte limit; the full tool list; remote executors and `exec-server` auth.
- DeepSeek harness: whether shell-command hooks exist; the "Claude Code mods via an experimental bridge".
- OMP: `eval` kernel isolation and the tool-callback bridge (README only).
- OpenClaw: the agent-learned "workshop" skill mechanism; ClawHub scan methodology; embedding and Gateway-client pages.
- Hermes Agent: the shell hook schema (hooks page tail unread); the Codex app-server runtime (title only); the MCP config reference.
- Warp: ACP support (changelog wording only).
- Claude Desktop: MCP tunnels and MCP Apps detail pages (titles only).
- Zed/ACP: ACP proxy chains (RFD not opened); registry distribution types and vetting.
- OpenHands: how approvals work for ACP-hosted agents.

### Models and observability (areas 14, 19)

From [round1/synth/07-models-observability.md](round1/synth/07-models-observability.md).

Changed by review (the corrected text is used above):
- **Codex.** The four model descriptions are confirmed but the page names no default model; the claim that GPT-6.1 Sol became the bundled default in 0.161.0 was not confirmed. "Responses API only, `wire_api = "chat"` unsupported" is on `models.md`, not the config page. The 300 s figure is an example `refresh_interval_ms`; 0 means refresh only after an auth retry. The project-config blocklist also includes `apps_mcp_product_sku`. The Speed page still cites GPT-5.6 beside GPT-6 (stale).
- **OpenCodex.** The account-pool selection wording ("round-robin or fill-first") was not on the dashboard page; the page describes a selection order and lowest-usage choice for new sessions. The "256 MiB memory budget" was removed (not found). Client support lists 28 clients, not about 25. Pairing code lifetime is five minutes. "Remember device" is a 30-day record expiry, off by default. The default-on telemetry claim (v2.100.0) comes from an Atom release feed the reviewer did not reopen.
- **Claude Desktop.** The 3P overview carries no GA label ("GA" is an inference); the list of unavailable features (computer use, cloud sessions, Remote Control) comes from other pages, not the overview.
- **DeepSeek harness.** The credential file location and "keys are write-only" are on the providers guide, not the credentials subsystem page, which gives no file location, permissions or encryption. The two telemetry sources differ in tone (subsystem page: no end-user opt-out described; CLI reference: `FEEDBACK_ONLY` default).
- **Cursor.** The Legacy Privacy Mode statement is on the cloud security page; the privacy page confirms Anthropic harm-review retention for Fable models.
- **Claude Code (review-2 and review-3).** Model flags, default model and effort, fallback chains, content-based fallback, advisor, fast mode, model allowlists and `/usage` cost semantics were confirmed. Routine naming and several non-model statuses were corrected (outside this group). Review-1 notes that no entry says which model summarises during compaction, though fallback chains apply to compaction.
- **Oh My Pi.** README lists nine roles; docs list ten (adds `designer`). README claims round-robin credentials; docs/providers does not describe it. "Default thinking level `high`" is stated on docs/settings, not docs/roles.
- **Pi.** Agent retry defaults confirmed (3, 2000 ms, 60000 ms). "15+ providers" on the homepage confirmed, versus more than thirty on the docs page.
- **goose.** ACP providers and the deprecated CLI providers confirmed; the troubleshooting binary is `claude-agent-acp`, not `claude-acp`.

Contradictions between a vendor's own pages:
- **Claude Code.** `fast-mode.md` still says fast mode is unavailable on Google Cloud, a day after 2.1.322 added it on Vertex. Default-model advice (Sonnet for most work) contradicts every plan defaulting to Opus 5.5; `costs.md` describes thinking budgets that `model-config.md` says current models ignore. A group spend cap is "not a shared pool" on one page while the config reference documents `pooled_budget`. Auto mode is "available on every provider" on one page and model-gated on another. Enterprise cost averages ($13 per developer-day and similar) are vendor claims without method.
- **Antigravity.** Plans page says no BYOK or custom endpoints, yet CLI settings list `modelProvider`, `customModelsConfig` and `GEMINI_API_KEY`, and the SDK supports Gemini keys, Vertex and local servers. Effort: `max` in the CLI reference versus low/medium/high elsewhere.
- **goose.** In-session page says the CLI cannot change model via a setting; the CLI reference lists `/model`. README says 15+ providers; docs list well over 40. The multi-model page promises dynamic switching strategies but documents only `/model` and per-sub-agent models.
- **Hermes Agent.** Sub-agent iteration cap is 50 on one page and `delegation.max_iterations` 250 on another.
- **Zed.** Remote development page is contradictory on where model calls originate.
- **Pi and goose** home pages understate provider counts.

Unverified or only vendor claims:
- **Oh My Pi** `omp stats` dashboard contents (no docs page; `/docs/stats` returns 404). Provider terms of service for subscription logins (docs silent).
- **OpenCode** telemetry behaviour of the local app; product page "100% free" and "data stored only for free models" are vendor claims.
- **OpenHands** LLM routing, fallback and subscriptions pages (index only); telemetry contents.
- **Hermes Agent** "spend limits that YOLO cannot skip" appears in a distinctive-features bullet but no entry describes it; credential-pool page not opened.
- **DeepSeek harness** token cost display and usage UI; trace view is a landing-page claim.
- **Cline** ClinePass "2-5x" is a vendor claim; provider list taken from nav titles; OpenTelemetry events and env pages not opened.
- **Cursor** Analytics and OTel details beyond index titles; BYOK page is brief.
- **Warp** BYOK, pricing and analytics API entries were not checked in review; model-choice pages at one path returned 404 (real path `/agents/inference/model-choice/`). Whether local-run model calls always go via Warp's servers is not spelled out.
- **Zed** hosted-model list and per-token prices page not opened; several ACP RFDs (usage, compaction, custom LLM endpoint) are title-only.
- **Claude Code** most provider and gateway entries (Bedrock, Vertex, Foundry, gateway upstreams, spend limits, OTel) were outside review-2's 47 checked entries; they are unreviewed, not wrong. Changelog-derived version and date claims were only partly reachable by reviewers.
- **OpenCodex** image and video bridge guides and the full provider matrix were not opened.

---

### UI, browser, collaboration (areas 15, 16, 18)

From [round1/synth/08-ui-browser-collaboration.md](round1/synth/08-ui-browser-collaboration.md).

What the reviews changed:
- DeepSeek harness web UI: the right sidebar is documented (docks guide, text preview, file tree, browser and subagent-chat tabs; layout in memory only). The survey's "contents not documented" was wrong.
- Claude Desktop built-in browser: a Deny choice was not found, only Allow once and Always allow; cookie behaviour differs by session type (Cowork persists until cleared; Code clears at quit unless Keep cookies is Shared or Per session).
- Claude Desktop Cowork: the sources conflict on where it runs (cloud vs local VM); the reviewer says both modes exist and the getting-started article is stale.
- Claude Desktop Dispatch: the 30-minute computer-use approval figure is from code.claude.com, not the support article; the article says limited beta and not available to new users while code.claude.com does not.
- Claude Code agent view: "Introduced v2.1.139" is not on `agent-view.md`; research preview confirmed.
- Claude Code "Claude in Chrome became GA in 2.1.198 (Jul 1, 2026)" and `allowClaudeInChromeWithManagedMcp` (2.1.282) moved to Unverified; only the removal of the shortcut tools in 2.1.319 was confirmed.
- Claude Code Chrome, computer use, Remote Control, HIPAA, managed-settings delivery and Claude Tag session model: confirmed.
- Codex: remote-control and cloud entries confirmed; Handoff text corrected ("to a worktree" not stated). No review entry exists for the desktop, IDE, browser or computer-use rows, which is why they are `unreviewed`.
- OpenCode: managed policies corrected (a policy allow can lift an earlier policy deny; "persist if Console unreachable" not found); web app, pairing, mobile and Console budgets confirmed; sharing was discontinued (v1.19.1) while a V1 share page may still be indexed.
- Cline: Kanban remote access corrected (the CDK example's root service and token in user-data are inferred, not stated by the page); remote-config per-surface table, SSH, YOLO and MCP controls confirmed.
- Cursor: mobile partly verified (the cloud-agent page also describes an Android PWA); hook merge is "deny beats ask beats allow, other fields last-response-wins", not "most restrictive wins".
- goose: the survey's "nothing documented" on sandboxing was not supported; a macOS Desktop sandbox page appears in search results but returned 404, so it stays unverified. The reviewer asks that every goose "nothing documented" claim get a second look, which affects the goose `-` cells here.
- OpenCodex: Remote Workspace pairing code is single-use with a five-minute lifetime.
- Hermes: ledger, breaker, watchdog and loop-guard details are on the messaging page, not gateway-internals.

Contradictions between vendor pages and in the surveys:
- Cline: the YOLO enterprise page says the policy applies "in all interfaces" while the remote-config table lists YOLO as unsupported on every surface.
- Codex: `codex agents --no-daemon` (CLI reference says unsupported; 0.167.0 notes say to use it); GitLab review is "preview" on one page and "beta" on another; the "secure relay layer" for mobile and remote hosts has no technical description.
- Antigravity: the slash-command catalog and overview say "sandboxed Chrome/Chromium" while the browser pages describe the user's live profile with sign-ins; remote-agents and Remote Control encryption and relay hosts are not described.
- Cursor: Android is "planned" on the mobile page but a PWA on the cloud-agent page; self-hosted worker limits are 200/1,000 on one page and 10,000 on another.
- OpenHands: the key-features page still describes the legacy GUI; the lifecycle page contradicts itself on PAUSED vs MISSING; the enterprise-vs-oss page says "open source" without naming a licence; FAQ says single local user while Canvas pages describe team VMs.
- OpenClaw: the maturity scorecard rates the Windows App/Node as Beta at 19% quality, which its own bands call Experimental.
- Warp: Admin Panel says only the team creator is admin while enterprise pages list Owner, Admin, Member roles (probably different tiers); "Oz" renamed "Warp Agent" while Oz CLI and web app keep the name.
- Claude Code: the spend-limit pages disagree on whether a group cap is a shared pool (config reference documents `pooled_budget`); the communications kit says the CLI talks directly to Anthropic's API, false for gateway and cloud-provider deployments.
- Oh My Pi: the "IDE wired in" headline is a vendor claim.
- OpenCodex: README says beta for the desktop app; the desktop page does not.

Unverified or unconfirmed in these areas:
- Claude Desktop: Claude Duet mechanism (only a one-line release note); last 25,000 characters of the desktop page were not read.
- Codex: relay architecture and encryption; computer-history, notifications and Record and Replay pages not read; managed-configuration page seen as headings only; service-account and access-token pages only skimmed.
- Oh My Pi: "Tern" phone UI (release notes only); Pi: whether `pi-web-ui` still ships.
- Cursor: SSO, SCIM, OTel export, analytics API pages not opened; audit logs and spend limits are index titles; Voice Chat and Design Mode pages not opened.
- Cline: SSO setup and member pages, enterprise API reference not opened; no browser tool page found; what the hub dashboard shows is undocumented.
- Warp: SCIM named only; audit logs not documented as a feature; the last parts of several `llms-txt` dumps were not read.
- OpenCode: SCIM and workspace pages not opened; V1 IDE pages and V1 Enterprise page not opened.
- OpenHands: how Canvas presents approval; SAML group-to-role mapping; whether the Enterprise server code is public and under what licence.
- OpenClaw: policy CLI details; Control UI feature depth (scorecard truncated).
- Hermes: remote-config, multi-gateway, voice, pets and webhooks pages not opened; whether `hermes serve` is separate from the gateway.
- Antigravity: relay host and encryption for Remote Control and Remote Agents.
- DeepSeek harness: sandboxing of browser tools; desktop signing and update mechanism.

### Remote execution and architecture (area 17)

From [round1/synth/09-remote-distributed-architecture.md](round1/synth/09-remote-distributed-architecture.md).

**What the reviews changed (used corrected above).**

- **Claude Code work orders.** Not one per session: in orchestrator (on-demand) mode each spawned runner gets a single-use work order that registers exactly one runner and expires with the spawn lease (default 120 s, `--expected-spawn-seconds`); sessions run as child processes of their runner. The survey's section 3 and section 5 bullets say "single-use work orders" per session; review 3 calls that sentence wrong (claude-code.review-3.md, corrections 4, 5, 9).
- **Claude Code Remote Control is not lease-based.** It polls outbound, but no lease is documented for it; only self-hosted runners refresh a lease by polling. "Outbound HTTPS only" is also short: runners and sessions use SSE and, with the git proxy, a WebSocket, all outbound (review 3, correction 4).
- **Claude Code control-plane policy delivery.** Confirmed and wider than the survey said: tool list and permission rules come "with the work payload"; model, connectors (as MCP config), lifecycle hooks and API endpoint also come from the control plane; the runner image's `managed-settings.json` applies too; on Bedrock/Agent Platform runners server-managed settings do not reach sessions, so the image file is the only enforcement. stdin carries token rotation and end signals, fd 3 carries activity signals (review 3, corrections 5, 6).
- **Claude Code cloud network secrets** use no sentinel: the key is never in the VM and the proxy attaches it. Only local sandbox credential masking uses sentinel substitution, and that needs experimental TLS termination (review 3, correction 3).
- **Claude Code cloud scheduled tasks** are renamed from "routines"; minimum interval one hour; the "skipped for up to 72 hours then turns off" clause could not be found (review 2 correction 5; review 3 correction 7).
- **Claude Code legal constraint** (section 5 bullet): the restriction on offering claude.ai login has a carve-out: an end user may sign in to the unmodified `claude` binary with their own subscription, including where a platform hosts Claude Code. So a node could drive an unmodified `claude` binary the user signs in, but rizzy-harness may not offer claude.ai login itself (review 3, corrections 1, 2).
- **Claude Desktop SSH sessions.** The claim that tasks survive sleep, network loss and the app quitting was removed; the remote login is renewed only while Desktop is connected and the docs say not to count on overnight runs. The survey's section 5 bullet ("the SSH helper that survives app exit") was not updated and is stale. Trusted Devices is beta (claude-desktop.md, section 9).
- **Claude Desktop Dispatch.** Limited beta, not available to new users; the 30-minute approval window is from the code.claude.com desktop page, not the support article.
- **OpenClaw cloud workers.** Status is beta (scorecard M3 Beta, quality Alpha 67%), not GA; the page says the worker "dials" the Gateway's public worker route and never says "outbound-only" or "WebSocket"; 4 h / 45 min is the suggested `cloud` profile, not a universal default. The survey's section 5 still says "outbound-only connection" (stale). Node pairing: the standalone node pairing store left the connect path in January 2026, `node.pair.request`/`node.pair.verify` were removed in 2026.7 (openclaw.md, section 9).
- **OpenCodex.** Hub pairing code lifetime is five minutes; the Remote Workspace rate limit is ten failed codes in ten minutes answered with 429; "runs no agent loop" is a reading of the pages, not a quoted claim (opencodex.md, section 9).
- **DeepSeek harness.** No page states that the agent loop and web server share a process (inference). The SSH page is not missing: it is in the repository at `docs/subsystems/ssh.md` and says the harness stays local and SSH loss is final.
- **OpenHands CLI** is "no longer actively maintained", not formally deprecated; the `openhands serve` GUI is deprecated.
- **Codex.** GitHub Action also offers `read-only` and `unsafe` safety strategies; Handoff does not say "to a worktree".
- **Cline Kanban.** The page does not flag the CDK example's root service or token in user-data; those are the survey's inferences. Hub roles are per session participant, not per client.
- **Warp.** `oz` is "a supported legacy interface" on the oz-cli page but "current" on the transitioning page; handoff "blocked while a command runs or child agents are active" was not found; key expiry options not confirmed on the cited page.
- **Hermes Agent.** Gateway ledger, breaker, watchdog and loop guard are documented on `/user-guide/messaging`, not on the gateway-internals page cited; iron-proxy binds the Docker bridge on Linux and loopback on Docker Desktop.
- **Oh My Pi.** The auth broker hands out API keys as well as access tokens; loopback default, no TLS.
- **goose.** The "no sandbox documented" claim is not supported (a macOS Desktop sandbox page appears in search results but 404s); reviewer advises a second look at every "nothing documented" in that survey.

**Where a vendor's own pages disagree (in these areas).**

- Codex: the app-server is "experimental and isn't supported for production workloads" while it is the only replacement for the removed MCP server and the stable Python SDK is built on it.
- Cursor: worker limits are 10,000 per user and team (self-hosted page) versus 200 per user and 1,000 per team (pool page); cloud retention "deleted after run" (privacy page) versus 30-day VM snapshots (security page).
- Claude Code: `self-hosted-environments-deploy` recommends `--capacity 1` while its own recipes use 4.
- Zed/ACP: the remote development page says the local machine talks to LLMs and also that the remote uses its own proxy for LLM calls; the ACP home page says remote support is in progress while goose already ships ACP over HTTP and WebSocket.
- Warp: `oz` legacy versus current.
- OpenCodex: README lists Docker Compose while the hub page says no official image is published.
- OpenHands: the Enterprise lifecycle page says idle timeout leads to PAUSED and to MISSING.
- goose: docs say `goosed` in logs and deeplinks while the remote guide and CLI reference say `goose serve`.

**What nobody could confirm.**

- Wire protocols: Claude Code Remote Control (SSE or WebSocket, schema), Cursor's worker protocol and encryption (docs say it is not documented), Codex's "secure relay layer", Antigravity's relay and encryption, Claude Code runner-to-child message format.
- Codex remote executors and `codex exec-server`: what they are and how they authenticate.
- Cloud VM isolation technology for Claude Code (microVM, container or VM) and Codex Cloud.
- Whether Warp's local (non-cloud) model calls always pass through Warp's servers.
- Whether Hermes Agent's `hermes serve` is separate from the gateway process; Hermes Agent and OpenClaw idle memory footprint (relevant to our 50 MB invariant only by comparison).
- OpenCodex's `ocx-child` relay and "zero-preinstall SSH setup" relation to Remote Workspace.
- Pi: whether the coding-agent CLI integrates with pi-durable and pi-env at all.
- Oh My Pi: whether `bash` runs remotely over SSH.
- Claude Code: "self-hosted runner public beta from 2.1.224 (Aug 7, 2026)" (the public-beta status itself is confirmed).

### Configuration, licence, other (areas 20, 22, 23)

From [round1/synth/10-config-licence-distinctive.md](round1/synth/10-config-licence-distinctive.md).

Corrected in review (applied in the text above):
- Codex project-config blocklist also includes `apps_mcp_product_sku` (https://learn.chatgpt.com/docs/config-file/config-advanced.md). Codex `/import`: the 50-chat cap is CLI-only.
- OpenCodex client-support page lists 28 clients, not about 25; "runtime verified" is a historical record, no blanket "no client" statement. The "runs no agent loop" statement is a reading of the architecture pages, not a quoted claim.
- OpenCode managed policies: a policy allow can lift an earlier policy deny (not purely deny); session sharing was discontinued in v1.19.1 (not merely undocumented); V2 "current" is from the V1 docs banner, not the changelog.
- Cursor hooks merge: permission deny beats ask beats allow regardless of source; other fields last response wins; not "most restrictive wins".
- Claude Code (review 3): the legal-terms entry omitted the carve-out that allows an unmodified binary signed in by each end user; the supporting sentence is on `agent-sdk/overview.md`, not `session-storage.md`. Review 1: idle-compaction detail ("about 54 minutes") not supported.
- Claude Desktop: the "Windows build 10 19041" distribution claim moved to Unverified; Research is not "mobile web only"; the 30-minute Dispatch window comes from the Code docs, not the support article.
- goose: Block's platinum AAIF membership is on the primary announcement (survey had called it third-party); goose's sandbox "nothing documented" claim was wrong in part (a macOS desktop `GOOSE_SANDBOX` page appears in search results but did not load).
- Warp: Factories status is Early Access; `oz` is "legacy" on one page and "current" on another.
- OpenHands: the CLI is "no longer actively maintained" (not formally deprecated); the legacy `openhands serve` GUI is deprecated.
- OpenCodex `Remote Workspace` and `Remote hub` details corrected (read-only default, Linux-only bubblewrap, five-minute pairing).

Contradictions inside a vendor's own pages that touch these areas:
- Cline: skills say global wins, rules say workspace wins; YOLO "applies in all interfaces" vs the remote-config table listing YOLO as unsupported on every surface.
- Warp: the Admin Panel page says only the team creator is the admin; enterprise pages name Owner, Admin and Member roles.
- Cursor worker limits: 200 per user and 1,000 per team on the pool page vs 10,000 each on the self-hosted page.
- OpenCodex: README lists Docker Compose, the remote-hub page says no official image is published; the README calls the desktop app beta, the desktop page does not.
- Pi: pages disagree on whether codemode is on by default (needs a run to settle).
- OpenClaw: `telemetry off` and `DO_NOT_TRACK=1` do not stop the daily version check; the Docker page does not state the default bind.

Unverified (could not be confirmed, or pages not opened):
- OMP themes and key bindings pages (not opened); Antigravity pricing page and precedence tables (areas 5, 7); Cursor full pricing page; Warp worker and SDK licences; OpenHands Enterprise server licence and Cloud pricing; Claude Code SDK package licences; Claude Code `CLAUDE_CODE_DISABLE_FEATURE_FLAGS` (changelog only, no docs page).
- DSH npm licence switch from BSD-3-Clause to MIT: not traced.
- Zed/ACP: user-versus-project precedence for agent keys; install methods.

## D. Features common across the ecosystem

### Agent loop, tools, editing, shell (areas 1-4)

From [round1/synth/01-loop-tools-editing-shell.md](round1/synth/01-loop-tools-editing-shell.md).

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

### Permissions, sandboxing, security (areas 5, 21)

From [round1/synth/02-permissions-sandbox-security.md](round1/synth/02-permissions-sandbox-security.md).

"Real harnesses" here are the sixteen excluding OpenCodex (a gateway). Half is eight. Counts use `Y` or `B` in the matrix.

| Feature | Count (of 16) | Comment |
|---|---|---|
| F1 Approval modes | 15 | Everyone but Pi, which documents not having one. Defaults split three ways (F1 Variants); ours is prompt-by-default, the conservative end. |
| F2 Per-tool/command rules | 15 | Universal (several partial). Ours has only whole-tool `denied_tools`. |
| F32 Telemetry stated | 13 | Defaults vary widely: on by default without content (Claude Code, Cline, Pi install telemetry, OpenClaw version check), session-log upload on by default (DeepSeek), opt-in (goose). |
| F34 Stated non-boundaries | 12 | The norm is to say in writing that rule lists and approvals are not a boundary. |
| F9 Admin/managed policy | 12 | Mostly org-level push from a vendor console, MDM or system file. |
| F5 Model-based approver | 12 | The fastest-moving area. Codex, Claude Code, Cursor and OpenClaw ship a separate reviewer; Codex, Claude Code and OpenClaw add a circuit breaker back to a human. |
| F24 Own credential storage | 10 | Keyring with plaintext fallback is the norm. |
| F29 Extension/plugin trust | 10 | Everyone who has plugins says they are unsandboxed. |
| F31 Remote link auth | 10 (Cursor `?`, Zed/ACP `P` not counted) | Dominant pattern: outbound-only connection from worker to control plane with short-lived credentials. |
| F3 Shell parsing | 9 | Every harness that parses converges on the same rule: split plain chains, refuse or exact-match anything with substitution. |
| F8 Workspace boundary | 9 | Usually enforced only for built-in file tools, not the shell. |
| F15 Container/VM isolation | 9 (goose `?` not counted) | Mostly vendor cloud VMs or documented recipes. |
| F22 Credential placeholder/proxy | 9 | The strongest-growing secret pattern; ours has none. |
| F30 Own server/UI auth | 9 (goose `?` not counted) | Ranges from strong (ours, DeepSeek launch token, OpenCode password) to none (OpenHands default, Cline hub and Kanban). |
| F33 Retention stated | 9 | |
| F17 Unattended behaviour | 8 | Deny-and-continue is converging as the safe default; Cline and Cursor cloud are the exceptions. |
| F25 Prompt-injection defences | 8 (goose `?` not counted) | Mostly content marking and pattern scans; nobody claims to solve it. |

Just under half (seven each), listed because they matter for the comparison: F11 OS sandbox for the shell (Codex, Claude Code, Claude Desktop, DeepSeek, Cursor, Antigravity, Zed; goose and Cline unverified), F16 sub-agent inheritance, F18 remote approval delivery, F19 hooks in the permission path. Seatbelt on macOS is used by every harness with a macOS sandbox; Linux splits between bubblewrap (Codex, Claude Code, Zed, Antigravity) and Landlock plus seccomp (Cursor; DeepSeek offers both).

### Context management and memory (areas 6, 7)

From [round1/synth/03-context-memory.md](round1/synth/03-context-memory.md).

Denominator: 16 real harnesses (OpenCodex is a gateway and excluded); Zed/ACP counted as one. Counts are `Y` or `B` cells above.

| Capability | Count | Who |
|---|---|---|
| Automatic summarising compaction | 16 of 16 | all except OpenCodex (which proxies it) |
| Manual compaction command | 14 of 16 | not Hermes (nothing documented), not ours |
| Instruction files with `AGENTS.md` or an equivalent project file | 14 of 16 | not DeepSeek harness (unclear), not Claude Desktop (own survey), not ours |
| Scopes, precedence or conditional loading of instruction files | 14 of 16 | same set (ours is partial) |
| Reads other vendors' instruction names | at least 9 of 16 | OMP, Pi (`CLAUDE.md`), Hermes, Zed, goose, Cline, Warp (via `/init`), OpenHands, Cursor (`.claude`/`.codex` agents), Claude Code (`AGENTS.md`) |
| Compaction extension points (hooks, plugin, template, model choice) | 9 of 16 | Codex, OpenCode, Claude Code, Pi, Cursor (observe only), OpenClaw, goose, Hermes, Zed (model choice only) |
| Agent-written memory of some kind documented | 11 of 16 | Codex, Claude Code, Claude Desktop, OMP, Cursor, OpenHands, OpenClaw, Hermes, Warp, Cline (convention), goose (unclear) |
| Context-usage display | 9 of 16 (counting ours as partial: 10) | Claude Code, Claude Desktop, OMP, Antigravity, OpenHands, goose, Zed, Cline, DeepSeek harness (API only) |
| Memory off by default or easily disabled | majority of the 11 above | Codex, OMP, OpenHands off by default; Claude Code, Claude Desktop, Cursor, Warp have toggles |

Skills directories (`SKILL.md`) are read in many of these harnesses but belong to another area; here they matter only as agent-written memory.

### Planning, sub-agents, background work (areas 8-10)

From [round1/synth/04-planning-subagents-background.md](round1/synth/04-planning-subagents-background.md).

There are 16 real harnesses (all except OpenCodex, the gateway). A feature counts as common when at least 8 of them document it (`Y` or `B`).

| Feature | Count (of 16) | Notes |
|---|---|---|
| Sub-agent delegation tool | 15 | all except Pi (example extension only) |
| Custom agent definition files | 14 (+1 unverified) | goose unverified; Cline nothing documented |
| Read-only / custom modes | 14 | nothing documented: OpenHands, Hermes |
| Sub-agent model slot | 13 | not documented: Claude Desktop (inherits the engine), Pi, Cline |
| Standalone scheduled runs | 12 (+1 unverified) | nothing documented: DeepSeek (reminders only), OMP, Zed; goose unverified |
| Resume a sub-agent | 11 | |
| Event triggers / webhooks | 11 | |
| Plan mode | 10 | nothing documented: OpenHands, OpenClaw, goose, Hermes, Zed Agent; Pi refuses it |
| Built-in named roster | 10 | the shapes differ sharply; see the roster table |
| Depth limit documented | 10 (+1 unverified) | from 1 to 10 levels, or none documented |
| Plan review / hand-off | 9 | |
| Task / to-do tool | 9 (+3 unverified) | |
| Background sub-agents | 9 | |
| Cross-session messaging / bus | 9 | |
| Background sessions / supervisor | 9 | |
| Cloud background agents | 9 | |
| Interview-first planning | 8 | |
| Concurrency limit documented | 8 | |
| Worktree / overlay isolation for sub-agents | 8 | |
| Background shell jobs | 8 | |
| Agent dashboard | 8 | |
| In-session schedules / loops | 8 | |
| Unattended-run permission policy documented | 8 | |
| Durable / crash-surviving runs | 8 | |
| Goal / run-until-done | 7 (+3 unverified) | just under the bar; counting the unverified rows would make it 10 |

Counts come from the availability matrix (`Y` or `B`, OpenCodex excluded).

What this says: by late 2026, **delegation with a per-agent model, custom Markdown agent files, a plan mode, cron-style scheduled runs and webhook triggers are table stakes**. Teams, swarms, script workflows, advisors and auxiliary slots are not, yet.

---

### Sessions and git (areas 11, 12)

From [round1/synth/05-sessions-git.md](round1/synth/05-sessions-git.md).

"Documented" here means `Y` or `B` in the matrix. Denominator is the 16 real harnesses (OpenCodex excluded; Zed/ACP counted as one). Half = 8.

| Feature | Count of 16 | Notes |
|---|---|---|
| Resume / continue | 15 | Only OpenCode lacks it in the survey ("nothing documented", not absence). |
| Archive / delete / retention / ephemeral | 15 | Mostly basic list/delete; the interesting differences are retention defaults. |
| Session storage documented | 13 | Strong families: JSONL (Codex, Claude Code, Oh My Pi, Pi, DeepSeek), SQLite (OpenCode, OpenClaw, goose, Hermes, Cline), file-per-event (OpenHands). |
| Worktrees for parallel agents | 14 | Everyone except Pi and goose (and Ours as user feature). Differences are in who gets one and how cleanup works. |
| Fork to a new session | 12 | Several (Antigravity, OpenHands) explicitly do not fork file state. |
| Diff views | 12 | Mix of full review panes (Warp, Antigravity, Claude Desktop, Cline) and API-only (OpenCode, OpenHands). |
| Conversation rewind / edit-and-resubmit | 11 | Counting Oh My Pi and OpenClaw as `B`. |
| Git host integrations / credential handling | 10 | |
| Export a session | 10 | Format and redaction differ widely. |
| Move session between machines/surfaces | 9 | Codex, Claude Code, Claude Desktop, Cursor, Warp are the full ones. |
| Search across sessions / picker | 8 | Exactly half. |
| Hosted PR review bot / CI action | 8 | Exactly half; 5 are GA-style (Codex, Claude Code, Cursor, OpenHands, Warp). |
| Worktree lifecycle / cleanup | 8 | Exactly half, GA in most. |

Below half but notable: file checkpoints (6 of 16 including OpenHands `undo_edit` as `B`; GA in OpenCode, Claude Code, Cursor, Hermes (off by default), Cline), commit-message generation (7), worktree setup/ignored-file copy (7), share by link (5), PR creation (5-6), local review command (4).

The notable negative pattern: 10 of 16 harnesses document no file-level undo at all (Codex, DeepSeek, Oh My Pi, Pi, Claude Desktop, Antigravity (unverified), OpenClaw, goose, Warp, Zed after removal), and the ones that do split between shadow git (OpenCode, Hermes, Cline, Ours) and non-git file snapshots (Claude Code, Cursor).

### Extensibility (area 13)

From [round1/synth/06-extensibility.md](round1/synth/06-extensibility.md).

Real harnesses counted: 16 (OpenCodex excluded as a gateway; Zed counted). Threshold: 8. Counts include `Y` and `B`.

| Feature | Count | Who lacks it (nothing documented) |
|---|---|---|
| MCP client | 16/16 | none |
| Skills in SKILL.md format | 16/16 | none |
| Plugin bundles | 15/16 | Warp (own agent) |
| Plugin distribution / marketplaces | 15/16 | Warp |
| MCP remote auth (OAuth) | 14/16 | Claude Desktop (not described), DeepSeek harness |
| MCP approval, trust, allow/deny | 14/16 | DeepSeek harness, OpenClaw |
| Custom slash commands / templates | 14/16 | Claude Desktop, DeepSeek harness (Cursor partial) |
| Programmatic control protocol | 13/16 (+Antigravity unverified) | Claude Desktop, Zed |
| Reads other harnesses' files in place | 13/16 | Claude Desktop, Antigravity, Hermes Agent |
| Embeddable SDK | 13/16 (OpenClaw unverified) | Claude Desktop, Zed |
| Plugin trust and supply-chain controls | 14/16 (6 of them only partial: OpenCode, DSH, OMP, Pi, Antigravity, OpenHands) | Zed, Warp |
| MCP beyond tools | 12/16 | Antigravity, OpenHands, Warp, Cline |
| Skill loading budget and invocation control | 14/16 | Claude Desktop, Warp |
| Hook trust, precedence, failure documented | 12/16 | Claude Desktop, DeepSeek harness, Zed, Warp |
| Custom tools in code | 11/16 | Codex, Claude Desktop, goose, Zed, Warp |
| Hooks (config-declared) | 10/16 (+2 unverified) | OpenCode and Pi (use in-process code instead), Zed, Warp |
| Skill registries and account sync | 10/16 | DeepSeek harness, OMP, Cursor, Antigravity, goose, Warp |
| Tool deferral / tool search | 9/16 | Claude Desktop, DeepSeek harness, Cursor, OpenHands, Zed, Warp, Cline |
| ACP agent (server) | 9/16 (+Warp unverified) | Codex, Claude Code, Pi (all adapter-only), Claude Desktop, OpenHands, Zed (client) |
| In-process extension API | 9/16 | Codex, Cursor, Antigravity, goose, Zed, Warp |
| Managed admin controls over extensions | 9/16 | DeepSeek harness, OMP, Pi, Antigravity, Hermes, Zed, Warp |
| Hook handler types beyond command | 8/16 (+Antigravity unverified) | OpenCode, Claude Desktop, DSH, Pi, goose, Zed, Warp |
| MCP limits and lifecycle documented | 8/16 | Claude Desktop, OMP, Pi, Cursor, OpenHands, OpenClaw, Zed, Cline |

Two observations the counts hide:
- The skill format is the one true standard (agentskills.io plus `.agents/skills/`), but frontmatter extensions, precedence and trust gates differ per harness, so a skill is portable as text, not as behaviour.
- "Plugins" in 15 harnesses means two different things: declarative bundles of skills, hooks and MCP config (Claude Code shape, copied by Codex, Cursor, OpenHands, Antigravity, OMP fallback) and code packages that run in-process (Pi, OMP, OpenCode, OpenClaw, Hermes, DSH, Cline). Only the first is safely portable.

### Models and observability (areas 14, 19)

From [round1/synth/07-models-observability.md](round1/synth/07-models-observability.md).

Counted over the 16 real harnesses (the 17 minus OpenCodex, a gateway; Zed/ACP counted as a harness), `Y` or `B` cells only, threshold 8 of 16.

| Feature | Count (of 16) | Note |
|---|---|---|
| Provider support and BYOK | 16 | Universal. Breadth and credential handling vary. |
| Model selection per session, agent, task | 16 | Universal; the scope ladder differs. |
| Cloud-platform providers (Bedrock, Vertex, Azure) | 15 | Absent only from DeepSeek harness. Ours is the gap. |
| Custom endpoints and wire formats | 15 | Cursor is the exception. Only Codex and Claude Code constrain the wire format hard. |
| Token and cost display | 15 | DeepSeek harness unverified. Cost in currency is a minority within this. |
| Logs, transcripts, raw request logs | 15 | Raw provider request logs are rare (goose documents them). |
| Voice | 13 | Mostly input dictation; realtime and TTS in OpenClaw, Hermes, Oh My Pi, Cursor, Antigravity. |
| Debugging and diagnostics tools | 13 | `doctor` is the common shape. |
| Vendor telemetry and upload defaults | 13 | Defaults differ more than the existence of telemetry. |
| OpenTelemetry and other export | 12 | Claude Code, Codex, OpenHands, goose, OpenClaw, Oh My Pi lead; four more are partial or enterprise-only. |
| Local models | 11 | |
| Hosted model offerings (commercial) | 11 | Vendor services, not harness features. |
| Budgets and limits | 11 | Mostly vendor plan quotas; user-set USD budgets are Claude Code, OpenCode Console, OpenHands Enterprise, Zed. |
| Usage dashboards and quota views | 11 | |
| Reasoning effort controls | 10 | |
| Subscription login or harness as provider | 10 | Legal limits documented only by Anthropic, Antigravity and OpenCodex. |
| Model roles for auxiliary work | 9 | |
| Vision and other modalities | 8 | |
| Routing across providers, credential pools | 8 | |
| Model catalogue and discovery | 8 | |

Fewer than half: provider retry (7), model fallback chains (6), governance (5), speed tiers (4), prompt-cache controls (4).

---

### UI, browser, collaboration (areas 15, 16, 18)

From [round1/synth/08-ui-browser-collaboration.md](round1/synth/08-ui-browser-collaboration.md).

Counts are of Y and B together out of the 16 harnesses (OpenCodex and ours excluded); "Y only" in brackets. Half is 8.

- Approval presentation in a UI: 15 of 16 (Y 14). Everyone except Pi, which has no permission system by design.
- Several agents or sessions at once: 14 of 16 (Y 9). Only Pi and goose have nothing documented.
- Diff presentation and review: 14 of 16 (Y 11). Missing: Pi and goose.
- Desktop app: 13 of 16 (Y 10).
- Terminal UI: 12 of 16 (Y 12); voice: 12 of 16 (Y 10); the agent's own browser: 12 of 16 (Y 10).
- Web UI: 12 of 16 (Y 7). Five of those are hosted and closed (Claude Code, Claude Desktop, Cursor, Antigravity, Warp); the open, self-served ones are OpenCode, OpenHands, OpenClaw, Hermes, DeepSeek (preview), Cline (Kanban preview) and Oh My Pi (collab viewer only).
- Accounts, roles or organisations: 12 of 16 (Y 11), but only OpenClaw and Hermes have them in open code; the rest keep them in a paid or hosted tier.
- Managed configuration or policy distribution: 12 of 16 (Y 7); shared team resources: 12 of 16 (Y 5).
- Plan review: 11 of 16 (Y 6); SSO and provisioning: 11 of 16 (Y 8); browser isolation or site policy: 11 of 16 (Y 7); computer use: 10 of 16 (Y 6).
- Mobile: 9 of 16 (Y 5); audit logs: 9 of 16 (Y 3).
- Exactly half (8): IDE integrations (Y 8), messaging-app channels (Y 7, B 1), budgets and usage controls (Y 6, B 2), session sharing or live collaboration (Y 7, B 1).
- Below half: notifications and waiting signals (6 of 16, Y 4); the user's real browser (7 of 16, all Y).

### Remote execution and architecture (area 17)

From [round1/synth/09-remote-distributed-architecture.md](round1/synth/09-remote-distributed-architecture.md).

Counted over the 17 surveyed harnesses (Y or B); "half" means 9 or more.

- **Programmatic control protocol: 15 of 17.** Every harness except OpenCodex (a gateway) and Claude Desktop documents one. The split is stdio-to-child (Claude Code SDK, Pi, DeepSeek harness `sdk`, ACP) versus long-lived server with HTTP/WebSocket (Codex, OpenCode, OpenHands, OpenClaw, goose, Hermes Agent, Cline hub). See 2.3.
- **One-shot headless run: 13 of 17.** Missing only from OpenCodex, Claude Desktop (by statement), OpenClaw (not in the survey) and Zed/ACP; OpenHands' is unmaintained. Ours has none. See 2.1.
- **Relay-based remote control from web or phone: 13 of 17.** The vendor relay is the commercial norm (Claude Code, Codex, Antigravity, Cursor, OpenCode OpenTunnel); none of them is self-hostable except as a pattern. See 2.3.
- **Pairing and authentication of remote clients and hosts: 13 of 17.** Single-use short-lived codes are the shared idiom (OpenCode, OpenCodex, OpenClaw, Codex). See 2.3.
- **Reconnect and resume semantics: 12 of 17**, with very different strength (from Claude Code's takeover rules to DeepSeek harness's "SSH loss is final"). See 2.3.
- **ACP support: 11 of 17** (12 if Warp's changelog mention is real). Nearly all are stdio only; goose alone ships ACP over HTTP/WebSocket. See 2.3.
- **Structured event stream: 11 of 17.** See 2.1.
- **SSH remote execution: 11 of 17** (12 if OpenCodex's release-note SSH setup is real). Split evenly between "whole agent on the remote" and "tools only on the remote". See 2.4.
- **Approval behaviour with no human present: 10 of 17**, with opposite defaults (fail closed versus approve everything). See 2.1.
- **Exposure beyond loopback documented: 10 of 17.** Loopback by default is now the norm among servers; the exceptions are Codex's unauthenticated non-loopback rollout default and container images that bind wide (OpenClaw, OpenHands Docker mode). See 2.3.
- **Event-triggered remote runs: 9 of 17.** See 2.2.

Just under half: vendor-hosted cloud agents (8), remote attach of a client to an agent server (8).

### Configuration, licence, other (areas 20, 22, 23)

From [round1/synth/10-config-licence-distinctive.md](round1/synth/10-config-licence-distinctive.md).

Count is out of the 17 listed names (half is nine). `Y` only; `B` counted in brackets.

| Feature | Harnesses documenting it | Count |
|---|---|---|
| Licence, open/closed, install and pricing information | all 17 | 17 |
| Config file formats and locations | Codex, OpenCodex, OpenCode, Claude Code, OMP, Pi, Cursor, Antigravity, OpenClaw, goose, Hermes, Zed, Warp, Cline (+ Claude Desktop, DSH, OpenHands as `B`) | 14 (+3) |
| Environment variables as an override/input layer | Codex, OpenCodex, OpenCode, Claude Code, OMP, Pi, OpenHands, OpenClaw, goose, Hermes, Warp (+ DSH) | 11 (+1) |
| Reading other harnesses' config or instruction files | Claude Code, Codex, OpenCodex, OMP, Cline, Warp, Cursor, goose, Hermes, Zed (+ OpenCode `AGENTS.md` only, Antigravity compat) | 10 (+2) |
| Managed (admin) config layer | Codex, OpenCode, Claude Code, Claude Desktop, Cursor, goose, Hermes, Warp, Cline | 9 |

Close to common: precedence documented as an ordered list (Codex, OpenCode, Claude Code, OMP, Pi, goose, Hermes: 7, plus 6 partial), themes (8), hot reload or invalid-file handling (7 plus 2 partial), programmatic config editing (7 plus 1).

The ecosystem has converged on: a user file plus a project file, with a project layer that is distrusted for anything touching credentials or execution; `AGENTS.md` as the lowest common denominator instruction file (OpenCode V2 reads only that; Warp, Zed, Hermes, goose, OMP, Cline, Cursor all read it).

## E. Every distinctive item, by theme

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

## F. Every overhyped, stale or contradictory item, by harness

Every item from section 6 of all 17 survey files, grouped by harness, followed by the pattern the reviews found. Items a review changed are marked **[review]**.

##### Codex
- `project_doc_max_bytes` is described two ways: combined size (AGENTS.md page) vs per file (advanced config page). https://learn.chatgpt.com/docs/agent-configuration/agents-md.md vs https://learn.chatgpt.com/docs/config-file/config-advanced.md
- `codex agents --no-daemon`: CLI reference says unsupported, 0.167.0 notes (2026-11-06) tell users to use it; the reference may lag. https://learn.chatgpt.com/docs/developer-commands.md vs https://learn.chatgpt.com/docs/changelog
- App-server is "experimental and isn't supported for production workloads" yet is the only replacement for the removed MCP server and under the stable Python SDK. https://learn.chatgpt.com/docs/mcp-server.md, https://learn.chatgpt.com/docs/codex-sdk.md
- GitLab review is "preview" on one page and "beta" on another. https://learn.chatgpt.com/docs/code-review.md, https://learn.chatgpt.com/docs/third-party/gitlab.md
- Code Mode is "under development and off by default" in the config reference while releases keep shipping user-facing Code Mode features. https://learn.chatgpt.com/docs/config-file/config-reference.md vs https://learn.chatgpt.com/docs/changelog
- The `untrusted` migration text contradicts itself (retired, yet managed `allowed_approval_policies` "must include `untrusted`"). https://learn.chatgpt.com/docs/agent-approvals-security.md
- Broken in-page anchors to `cloud#...` sections that do not exist in the Markdown (may render only in HTML; not checked). https://learn.chatgpt.com/docs/cloud.md
- Speed page still cites GPT-5.6 beside GPT-6 models (looks stale). https://learn.chatgpt.com/docs/agent-configuration/speed.md
- Docs index titled "Codex" but most pages are ChatGPT, Work, dots or Space. https://learn.chatgpt.com/docs/llms.txt
- README links to `developers.openai.com/codex`, now redirecting to the unified docs. https://raw.githubusercontent.com/openai/codex/main/README.md
- Marketing-level claim: the mobile/remote "secure relay layer" has no technical description. https://learn.chatgpt.com/docs/remote-connections.md
- Memories: "off by default" next to "Enabled memory v2 by default for fresh profiles" (refers to store version, not enablement).

##### OpenCodex
- "40+ providers", "zero client patches", sub-agent demos are vendor claims; the client page says no client has runtime-verified results **[review]**: the page does not make a blanket statement.
- README calls the desktop app beta; the desktop docs page does not.
- README lists Docker Compose; the remote-hub page says no official image is published.
- Remote Workspace is presented as a feature but is an "experimental foundation".
- 502 errors blocked the GitHub release list; only two release notes seen.

##### OpenCode
- Version confusion: docs say V2 current; GitHub "latest" flags v1.19.5; a Korean post (search snippet only) says "2.0" referred to Desktop v2. https://opencode.ai/changelog
- README (https://github.com/anomalyco/opencode) lists npm `@opencode/cli`, describes plan as asking before bash, calls desktop beta and has no client/server note: likely lags V2.
- The API page calls the whole surface experimental while SDK pages present it as the supported contract.
- Config page omits `experimental.schedules`/`experimental.artifacts` that the changelog introduces; `/goal` is in the changelog without a docs page.
- V2 docs omit sharing and GitHub integration; V1 pages stay live; sharing was discontinued in v1.19.1 **[review]**, https://opencode.ai/en/docs/share seen in a search result only.
- Product page: "One agent, every surface", "100% free to use", a "Degraded performance" banner: marketing. https://opencode.ai/product
- Console page does not map the three prices to plan names; the Go page does.

##### Claude Code
- Docs lag the changelog: weekly digest stops at Week 37 (2026-09-11), Week 38 returns 404; many 5.5 models, Mods, idle compaction, AGENTS.md support have no digest; `fast-mode.md` still says unavailable on Google Cloud a day after 2.1.322 added it on Vertex; status line JSON sample shows version 2.1.90; mods reference says "as of" 2.1.290/2.1.299 while citing 2.1.321; settings JSON schema lags; anchors in `server-managed-settings.md` renamed.
- Default-model advice contradicts the defaults: `how-claude-code-works.md` and `costs.md` say Sonnet handles most work, every plan defaults to Opus 5.5; `costs.md` thinking budgets that `model-config.md` says current models ignore. https://code.claude.com/docs/en/model-config.md
- Churn: subagent nesting went 5 levels, off, 3 within six weeks (digest says 5); `keybindingFlavor` lived 23 days, `taskOutputMaxChars` about 2 weeks; output styles deprecated then un-deprecated after 5 days; `/simplify` renamed `/code-review` and now names a different command; `/fork` meant three things.
- Status labels disagree: fullscreen "research preview" yet default since 2026-05-06; `claude agents` research preview on one page; plugin evals, monitors and themes presented as available but under `experimental.*` manifest keys; auto mode "available to all users on every provider" vs model- and provider-gated. **[review]** Review 2 confirmed agent view is research preview (the survey's "GA" entries were wrong).
- Pages disagree on mechanism: spend limits ("not a shared pool" vs `pooled_budget`); cross-session messaging across machines vs same machine; MCP startup timeout 30 s vs 5 s; MCP "Clear authentication" revoking upstream tokens; `self-hosted-environments-deploy` recommends `--capacity 1` but recipes use 4.
- Vendor claims without mechanism or numbers: fast mode "up to 2.5x faster"; Ultrareview "every finding independently reproduced" at $5-25; enterprise cost averages; GitLab "sandboxed execution" with no sandbox in the examples; computer use "flags prompt injection"; the communications kit says the CLI talks directly to Anthropic's API, false for gateway and cloud-provider deployments.
- The vendor admits limits: Bash rules "not a security boundary", sandbox "not a complete isolation boundary", auto mode "does not guarantee safety", policy mods bypassable by `--safe-mode` and worker crashes, server-managed settings not a security boundary.

##### Claude Desktop
- Where Cowork runs: getting-started article says cloud; Linux and 3P pages say a local VM. **[review]** Both modes exist; the getting-started article is stale or incomplete.
- Cowork scheduled tasks "run in the cloud" vs Desktop local tasks needing the app open. **[review]** Different products, not a contradiction; `/schedule` now fills in a scheduling request.
- Dispatch: Pro/Max/Ultra feature in Code docs, "limited beta, not for new users" in the support article.
- Cowork "generally available" Apr 9 2026 vs Sep 16 2026 gradual rollout; product page still says "being folded into Claude".
- Linux page lists Cowork in the beta; download page does not list Android; docs index says no dedicated artifacts, memory or research pages.
- Productivity claims on the Cowork page come from testimonials (vendor-claim).
- Changelog version numbers look non-monotonic (v4.29.0 then v4.4212.0).

##### DeepSeek harness
- `00-targets.md` said no GitHub releases; the API shows prereleases up to `dsh-v0.2.2-rc.1` **[review]** confirmed.
- Landing page marks agent teams, auto review, scheduled tasks and voice experimental; the docs site documents schedules without that label.
- Landing statements ("out of the box", "explore the upper limit of intelligence", Pomodoro plugin in 5 min 24 s) are marketing.
- The subsystems index links `ssh`, `mcp`, `agent-team`, `webhook` GitHub pages that the docs-site mirror lacks (404; content exists on GitHub) **[review]**.
- The tool catalogue lists alternate or opt-in tools without saying which are mounted by default.
- 278k stars for a repo created 2026-08-13 says nothing about maturity.
- npm first release listed BSD-3-Clause, later MIT; not traced.
- SDK page says the SDK profile pins `danger-full-access`; the web quickstart asks for approval (different profiles, easy to confuse).

##### Oh My Pi
- README tool count (31) and coreutil counts (46/58/67) disagree internally; README nine roles vs docs ten (`designer`).
- README says `skill://` exists; skills doc documents none. README lists `chillax`; magic-keywords page does not.
- docs/memory says no named memory tools; docs/tools and README list five.
- README "schema-validated typed results" for task vs subagents doc "no formal results schema".
- README claims round-robin credentials with session affinity; the providers doc does not describe it.
- Hashline benchmark numbers are the author's own; docs/editing does not mention hashline.
- "Time-traveling" is marketing: abort-and-retry with partial output discarded by default.
- Default approval mode is `yolo`; "IDE wired in" and 60+ providers are vendor claims.
- SDK requires Bun 1.3.14+ vs README install Bun >=1.4.4.
- docs/tools says `ast_grep` is off but README lists it as a headline tool.
- No docs page for stats, SSH, eval; themes and keybindings not read; sitemap has no changelog.

##### Pi
- Codemode default: CLI page says off, settings page says `on` and lists it loaded by default, MCP page says MCP tools default to `codemode`; needs a run to settle. https://pi.dev/docs/latest/cli, https://pi.dev/docs/latest/settings, https://pi.dev/docs/latest/mcp, https://pi.dev/docs/latest/codemode
- Homepage lists MCP among deliberate non-features while noting it is built in. https://pi.dev/
- npm scope: April 2026 post says `@earendil/pi`; README and docs use `@earendil-works/pi-coding-agent`.
- Homepage "15+ providers" vs a longer providers page.
- Non-feature rationale is from a 2025 post when Pi was a personal project; MCP was argued against there and later added.
- how-pi-works and quickstart pages do not mention the omitted features.
- pi-chat shows two licences on one page. https://github.com/earendil-works/pi-chat
- README package table omits `pi-env`, `codemode`, `mcp`, `schema`, `evals` that exist as directories.
- The 2025 "under 1,000 tokens" prompt claim is not repeated and may no longer hold (unverified).
- Terminal-Bench 2.0 benchmark claims are the author's own runs.

##### Cursor
- Worker limits: 200 per user and 1,000 per team (pool page) vs 10,000 each (self-hosted page).
- Cloud data retention: privacy page says encrypted copies only while running; cloud security page says VM snapshots kept 30 days and not deletable on demand.
- Memories: silent in agent overview and rules pages; removal documented only on forum posts that disagree (UI vs feature removed).
- Mobile: cloud-agent page suggests Android via PWA; mobile page says Android planned.
- GitHub integration page omits `@cursor` mentions that the cloud agent page lists.
- "Cursor never connects into your network" and "meets the security requirements of most teams" are vendor claims.
- Auto-review is "not a security boundary" but marketed as a default.
- BYOK and Max Mode pages are brief; many status labels absent.

##### Antigravity
- "Sandboxed" browser: docs say the subagent uses the real Chrome profile with live sign-ins.
- Hooks: plugins page says shell commands or prompt evaluations; hooks page documents only `command`.
- Effort: `max` in the CLI reference vs low/medium/high elsewhere.
- Plans page says no BYOK/custom endpoints, yet CLI settings list `modelProvider`/`customModelsConfig`/`GEMINI_API_KEY` and the SDK supports Gemini keys, Vertex and local servers.
- Slash command catalog calls itself complete but omits commands; `/context` page does not mention `/compact`; artifact review page mentions "Fast mode" though `/fast` was removed in v1.1.0.
- Overview says four surfaces, llms.txt five; migration page links to a missing settings page.
- "Co-trained", "highly optimized", "securely" are vendor claims.
- Auto mode "Preview" in v2.29.0 and earlier; sandbox "Preview" for business accounts **[review]**: the label could not be found.
- Enterprise "never used for training" is a vendor claim; the enterprise page defers to linked Cloud docs.

##### OpenHands
- Docs release notes end at 1.35.0 while GitHub is at 1.38.0.
- Key-features page describes the legacy GUI; the introduction page says GUI and CLI are deprecated/unmaintained.
- Enterprise lifecycle page contradicts itself on idle timeout leading to PAUSED vs MISSING.
- SDK security docs disagree on whether the LLM analyzer uses a separate LLM, and on the default confirmation policy.
- Condenser guide says no defaults documented; architecture page gives 120/4.
- Workspace architecture page lists five workspace types; guide index lists Apptainer and Cloud too.
- The enterprise-vs-oss page says "open source" without naming the licence; the FAQ says single local user while Canvas pages describe team VMs.
- V1 API pause/resume paths look truncated.
- Automation pages describe Cloud behaviour (fresh sandbox per run) that does not apply to a local backend.
- Slide-style claims ("composable", use-case pages) are vendor claims.

##### OpenClaw
- Home page and features list still say "heartbeat scheduling" while the heartbeat page says the monitor is retired.
- Workspace page lists `TOOLS.md`/`HEARTBEAT.md` in a summary but not in its file map.
- Tool Search is under "Experimental features" yet on by default and a built-in tool.
- Maturity scorecard shows Windows App/Node as M3 Beta with 19% quality, which the page's bands call Experimental.
- Why-OpenClaw is a self-written comparison with Hermes Agent (`vendor-claim`); its "single-process design relying on the OS" for Hermes is unverified.
- `telemetry off` and `DO_NOT_TRACK` still leave the daily check on.
- Docker page does not state the default bind; the security page says containers default to an exposed bind.
- The prompt-injection page's attack-success test by model (0.5% to 8.5%) has no primary source.
- Dreaming is default-on yet "promoted memories have no time-based retention limit".
- Docs move fast (anchors redirected, schema bumps; heartbeat migration needs schema 30).

##### goose
- README says 15+ providers; the providers page lists many more.
- Docs name the server `goosed` in places while the remote-server guide and CLI use `goose serve`; v1.53.0 notes a lean ACP-only binary.
- Multi-model page promises "dynamic, context-aware switching" but documents only `/model` and subagent models.
- In-session actions page says the CLI does not support model switching as a setting; the CLI reference lists `/model`.
- ACP providers page mentions a `copilot` binary not in its supported list.
- Prompt injection page does not state the default; config pages say off.
- Subagents are disabled outside autonomous mode, so delegation conflicts with approvals.
- Hook page: `SubagentStart/Stop` listed but "not emitted".
- Context relevance operation and context report (v1.58.0) have no doc page found.
- Release notes mention newer models (Opus 5.5, GPT-6) read only as release-note text.
- **[review]** The survey's "nothing documented" for sandboxing was wrong in part (a macOS desktop `GOOSE_SANDBOX` page appears in search results; the page 404s on fetch).

##### Hermes Agent
- README says session search uses LLM summarisation; memory and sessions pages say raw excerpts, no LLM.
- Architecture diagram says seven terminal backends, prose six, tools page eight including plugins.
- Delegation `max_iterations`: 250 (delegation page) vs 50 (agent-loop page).
- Compression lineage: in-place (compression page) vs "creates a child session" (sessions page).
- Tool Search called "opt-in" yet default `auto` behaves as on.
- Prompt-assembly page places the skills index in both the volatile and stable tiers.
- Docker network recipe mixes `dashboard` and `hermes-dashboard` names.
- "Skills improve during use" and MoA benchmark numbers are vendor claims.
- Release notes show hundreds of merged PRs per patch; the docs describe a very wide surface with many "listed" pages; some features may be thin.
- Termux and several packaging paths flagged prerelease or unsupported.

##### Zed/ACP
- Remote development page says the local machine talks to LLMs and also that the remote uses its own proxy.
- Instructions page lists nine project files, then says the Zed Agent loads `AGENTS.md`, `CLAUDE.md`, `copilot-instructions.md` **[review]**: first match wins in the listed order and `.rules` is still supported.
- Docs say sandboxing without any maturity label; release notes mention it only as a bug fix.
- Agent Panel docs say "no checkpoints" but 1.25.2 and 1.23.3 release notes still refer to them; 1.27.0 removed them.
- Tool-permissions page does not say how it applies to external agents.
- ACP home page says remote support is in progress; the agents page lists 51 agents, several third-party or adapter-based.
- ACP v2 overview page does not mark what is new versus v1.

##### Warp
- Naming churn: "Oz" agent renamed "Warp Agent" (2026.08.13) while Oz CLI, Oz web app and `oz-*` repos keep the name; CLI docs call `oz` "legacy" yet it is the documented CI route and the transitioning page says "current". **[review]** confirmed.
- SOC 2, ZDR and "no code stored" are vendor claims.
- Codebase Context page says no code is stored on Warp servers but does not say where embeddings are computed.
- "Open" covers the client only; the cloud platform is not described as open anywhere.
- Orchestration page does not label beta while Factories is Early Access **[review]** (status-wrong: relabelled from preview); parts of the CLI doc truncated.
- Admin Panel page says only the team creator is admin; enterprise pages describe Owner, Admin and Member roles.

##### Cline
- Overview says "Every action requires your explicit approval", but CLI defaults to auto-approve true, schedules default to yolo, SDK default-allows.
- YOLO enterprise control: page says all interfaces; remote-config matrix says no surface supports it.
- Plan mode "cannot run commands" vs subagent/scheduling pages saying it only blocks mutating commands and not `python -c` writes.
- Flag naming: `--auto-approve` vs `--yolo`; Plan/Act pages still mention todo lists though Focus Chain is deprecated.
- Checkpoint restore labels differ between the checkpoints page and the IDE page.
- Kanban overview mentions auto-commit/auto-PR; core-workflow says auto-PR not described; Kanban remote access has no auth.
- `.clineignore` "200k+ to under 50k tokens" is a vendor claim; not a security boundary and being deprecated.
- ClinePass "2-5x usage" is a vendor claim; no absolute limits.
- Hooks docs page is a stub; docs reference old tool names; Auto Compact and Memory Bank pages look legacy.
- Release page shows no year; Cloud sessions "rolling out gradually".

##### The pattern across files and reviews

Taken from the surveys' section 6 lists and the review tables:
1. **Docs lag releases and READMEs lag docs.** Seen for Codex (reference vs release notes), Claude Code (digest stops at Week 37, fast-mode page a day stale), OpenCode (README lists V1 package and behaviour), OpenHands (docs notes 1.35.0 vs 1.38.0), OpenCodex (README vs pages), OMP (README vs docs counts), Hermes (README says session search uses an LLM), Pi (homepage "15+ providers"), goose (README "15+ providers").
2. **Maturity and status labels are the most common error in the surveys themselves and in vendor pages.** Review corrections were mostly status labels: Claude Code agent view GA→research preview, OpenClaw cloud workers GA→beta, Warp Factories preview→Early Access, OpenHands CLI "deprecated"→"no longer actively maintained". Vendors disagree with themselves: Codex GitLab preview/beta and App-server "experimental" under a stable SDK, OpenClaw Tool Search experimental but on by default, Claude Code fullscreen research preview but default, Antigravity auto mode Preview.
3. **Defaults contradict across pages.** Pi codemode, Cline auto-approve versus "every action requires approval", OMP default `yolo`, goose prompt-injection default, Hermes delegation `max_iterations` 250 vs 50, Claude Code default-model advice, OpenHands default confirmation policy.
4. **Absence claims ("nothing documented") were occasionally wrong.** goose sandboxing; OpenCode "sharing undocumented" was actually discontinued; OpenClaw's hardened-baselines page 404 once. This is why the matrix treats `-` as weak.
5. **Review corrections to the surveys' own synthesis sections were more frequent than to feature entries.** In section 3 and relevance text: Claude Code's "each session gets a single-use work order" and grouping Remote Control under leases; the credential-masking summary missing its experimental dependency; OMP TTSR "zero standing context cost" not on the page; OpenClaw cloud workers "outbound-only"; goose ACP users; Cursor hooks "most restrictive wins"; Zed checkpoint "page note" that did not exist. Reviewers said the same of feature entries: errors were small (a default, a version, a cap), not invented features.
6. **Numeric and date claims from changelogs are the weakest category** (Claude Code idle compaction "about 54 minutes", 2.1.x dates; Codex 0.115/0.161.0; OpenCodex release-feed claims; Zed dates and 1.27.0).
7. **Vendor claims without mechanism cluster in marketing text:** speed-ups, "securely", "co-trained", SOC 2/ZDR, `.clineignore` token savings, ClinePass multipliers, benchmarks run by the author (Hashline, Terminal-Bench), star counts as maturity, "zero client patches", "40+ providers".
8. **Cross-page conflicts that reviewers resolved as different products or modes, not errors:** Cowork cloud vs local VM, Cowork cloud tasks vs Desktop local tasks, DSH SDK profile `danger-full-access` vs web profile asking for approval.
9. **Naming and renaming churn:** Warp Oz, Claude Code subagent depth and `/fork`, goose `goosed`, OpenClaw heartbeat, OpenCode V1/V2 and Zen/Console, Pi npm scope.

## G. All questions for Round 2

### Agent loop, tools, editing, shell (areas 1-4)

From [round1/synth/01-loop-tools-editing-shell.md](round1/synth/01-loop-tools-editing-shell.md).

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

### Permissions, sandboxing, security (areas 5, 21)

From [round1/synth/02-permissions-sandbox-security.md](round1/synth/02-permissions-sandbox-security.md).

Codex (CLI is open source):
1. Execpolicy: what exactly the tree-sitter splitter treats as "plain linear", and how an opaque invocation is matched (exact string, argv?). How `match`/`not_match` tests are executed at load.
2. Linux sandbox: the bubblewrap argument list and seccomp filter per mode; whether network is actually off by default under `workspace-write`; how the Unix-socket isolation that disqualified Landlock is done.
3. Auto-review: where the "fail closed" paths are and what transcript the reviewer receives; how the circuit-breaker counters persist across turns.
4. `shell_environment_policy`: the real default exclude list and whether `ignore_default_excludes = true` is the shipped default in code.
5. Hook trust by hash: what is hashed (file bytes, resolved command, plugin manifest) and where approvals are stored.

OpenCode:
6. The shell scanner: which parser per shell, how `cd` targets are inferred, and how "always" derives the saved patterns from a command.
7. Whether any OS sandbox or process isolation exists in V2 code despite no docs.
8. What policy applies when the Console is unreachable (cached, fail closed, fail open).

DeepSeek harness:
9. Default sandbox mode and preset at startup; how `partial` enforcement is decided per Landlock ABI.
10. Auto review integration: prompt, model and failure behaviour.
11. Whether the approval UI shows tool arguments anywhere, given requests omit them.

Oh My Pi:
12. `bash.patterns` segment splitting and the exact critical-pattern list; whether yolo really skips them in code.
13. `auth-gateway`: does any provider key ever reach the client process.

Pi:
14. What `enableInstallTelemetry` sends; whether a failing extension `tool_call` handler blocks in every mode.

OpenHands:
15. Whether `LLMSecurityAnalyzer` makes a separate LLM call; the real default confirmation policy when none is set.
16. Per-conversation Docker hardening flags as actually passed, and whether client-supplied limits are capped anywhere.

OpenClaw:
17. Exec approval binding: how the executable content hash and resolved-path re-check are implemented, and the TOCTOU window between check and launch.
18. Auto-review prompt and the three-denial escalation counter; sandbox mode defaults per backend.
19. Node pairing: what the device token is, how SSH-verified auto-approval compares keys.

goose:
20. Does `GOOSE_SANDBOX` exist in current source; Seatbelt profile and egress proxy if so.
21. Smart Approval risk criteria; adversary-mode prompt; malware-check data source.

Hermes Agent:
22. Hardline matcher implementation and what "fails closed on unparseable quoting" means in code; how deny globs "see through" `sh -c`.
23. iron-proxy integration: how tokens map to keys, and what happens to Hermes's own LLM calls.
24. `smart` approver prompt and the 300 s timeout path.

Zed (open source):
25. Terminal chain splitting: which shells are parsed; bubblewrap profile; when Linux falls back to unsandboxed.
26. Where `sandbox_permissions` grants scoped "always" are stored and whether a repo can ship them.

Warp (client open source):
27. The shipped default denylist, and whether any local-agent sandbox code exists; redaction regex list and where it runs.

Cline:
28. Whether `requires_approval` is enforced anywhere beyond the model's flag (e.g. a static override); what `CLINE_SANDBOX` does; whether the hub has any authentication in code.

OpenCodex:
29. Remote Workspace bubblewrap profile and the "command-sandbox probe" that gates write access.

ACP (open protocol, Rust SDK):
30. Whether the SDKs implement any default client-side policy for `fs/*` and `terminal/*`, or leave it entirely to the client.

### Context management and memory (areas 6, 7)

From [round1/synth/03-context-memory.md](round1/synth/03-context-memory.md).

Source-code-settled questions for the open-source harnesses.

1. **Codex, memories**: what exactly is in an extracted memory file, how it is injected into a turn, and where redaction runs (`memories_v2` store); does `disable_on_external_context` apply at generation or at use?
2. **Codex, AGENTS.md**: is `project_doc_max_bytes` per file or combined, and are empty or override files counted?
3. **Codex, compaction**: what `model_auto_compact_token_limit` scope `body_after_prefix` counts, and what failed-compaction recovery does.
4. **OpenCode, compaction**: how the buffer, `keep.tokens` and summary reuse of the prompt cache are built; how the pre-written background summary is invalidated and what is lost on restart; where keyed session instructions are placed.
5. **OpenCode, instructions**: how nested `AGENTS.md` are tracked as "already in history" and re-injected after compaction.
6. **Oh My Pi, compaction**: how `methodOrder` falls through on failure, how snapcompact renders and re-reads images, and what survives (including TTSR injections).
7. **Oh My Pi, memory**: how the `local` distillation chooses and writes `memory_summary.md`/`MEMORY.md`/`learned.md`, whether `learn` and `manage_skill` writes need approval, and which memory tools exist per backend.
8. **Pi, compaction and `context_edit`**: how cut points, split-turn summaries and `context_edit` are implemented and whether users can create the latter; how idle cache warming decides.
9. **OpenClaw, dreaming and compaction**: the real thresholds (`minScore`, `minRecallCount`, `minUniqueQueries`), how the 25% rewrite guard works, how `safeguard` audits summaries, and the retention and deletion gaps.
10. **OpenClaw, workshop skills**: what the agent-learned skills tier is and who writes it.
11. **Hermes, memory and curator**: how the frozen snapshot, injection scan and `write_approval` staging are enforced; the in-place compaction lineage (`in_place` default); how the Curator lock blocks agent writes including terminal writes.
12. **Hermes, compression**: whether a manual compress command exists; the real behaviour when the summary model's window is smaller than the main model's.
13. **goose, compaction and memory**: how `GOOSE_TOOL_CALL_CUTOFF` and background tool-output summarisation work; what the built-in Memory extension stores; whether "Top of Mind" equals MOIM; precedence of `AGENTS.md` versus `.goosehints`.
14. **OpenHands, condenser and memory**: how `Condensation` events are filtered into the view and how the 6,000-char index trimming and untrusted wrapper are built.
15. **DeepSeek harness, compaction and spill**: numeric thresholds, how the locked sequence recovers from a crash orphan, and the config catalogue defaults.
16. **Zed, instructions and compaction**: which of the nine instruction files the Zed Agent actually loads; how the threshold syntax (percent, tokens, negative) is parsed.
17. **Cline, compaction**: what `basic` versus `agentic` compaction do and whether the Auto Compact and Memory Bank pages describe the current harness.
18. **Cursor (closed, public surface only)**: not source-settleable; the SDK `MemoryBackend` interface and automation `MEMORIES.md` handling are the only open-ish parts.

### Planning, sub-agents, background work (areas 8-10)

From [round1/synth/04-planning-subagents-background.md](round1/synth/04-planning-subagents-background.md).

- **Codex (CLI, open):**
  - What are the prompts and tool sets of the built-in agents `default`, `worker` and `explorer`?
  - Is plan mode enforced by tool removal, by the sandbox, or by prompt?
  - What is the default for `max_concurrent_threads_per_session`?
  - How does goal "automatic continuation" decide to continue, and what stops it besides the token budget?
- **OpenCode:**
  - Does the `plan` agent's rule set actually deny shell, or only edits?
  - How does the `/goal` reviewer judge, and with which model?
  - How do `experimental.schedules` "fire once across multiple server processes" (lock, lease, or DB row)?
  - What model does the hidden `title` agent default to when `agents.title.model` is unset?
- **DeepSeek harness:**
  - What is the default `maxDepth` and `maxGoalRounds`?
  - Does a sub-agent inherit the parent's sandbox mode and approval policy (the docs are silent)?
  - How are reminder deliveries made idempotent on restart?
  - Which model generates titles?
- **Oh My Pi:**
  - What is the exact role auto-resolution algorithm when `modelRoles` is empty?
  - How does `task.isolation` choose a backend and apply patches on conflict?
  - Is plan mode read-only by tool removal?
  - How is the advisor's "blocker interrupt" implemented?
- **Pi:**
  - In the example sub-agent extension and pi-durable, how is a child's tool set and cwd constrained?
  - Does pi-durable's restart limit count per task or per run?
- **OpenHands:**
  - Does the task tool enforce any depth or count limit in code?
  - How are approvals from ACP-hosted agents (Claude Code, Codex) surfaced or answered?
  - Which LLM runs the goal judge by default?
- **OpenClaw:**
  - What are the sub-agent depth defaults and the tool-loss list?
  - How is "one retry for interrupted agent-turn jobs" recorded so it cannot repeat?
  - Is the cloud worker connection outbound-only in code?
- **goose:**
  - Do sub-agents inherit the parent's provider or model when `GOOSE_SUBAGENT_MODEL` is unset?
  - Where does the scheduler store jobs, and what happens to a run missed while `goose serve` is down?
- **Hermes Agent:**
  - Which model does each auxiliary task use when unset, and do the auxiliary fallback chains share cooldown state with the primary chain?
  - Does the goal judge's fail-open path have any cap?
  - How does the kanban dispatcher reclaim a crashed claim without double-running a worker?
- **Zed:**
  - Does `spawn_agent` enforce any depth or concurrency limit?
  - Do `thread_summary_model` and `compaction_model` default to the main model?
  - What tools does a sub-agent get when the parent's profile restricts them?
- **Warp (open client):**
  - Is orchestration (child launch, inbox) implemented client-side or only on the closed control plane?
  - Where is the planning-model switch applied?
- **Cline:**
  - Does `spawn_agent` really have no depth, count or timeout cap?
  - Is plan mode's shell restriction a blocklist in code?
  - Does a schedule created while the hub is stopped fire later, and with what approval mode?
- **Zed/ACP protocol:** In the sub-agents RFD's reference implementation (if one exists), can a child session's permission requests reach the client?

### Sessions and git (areas 11, 12)

From [round1/synth/05-sessions-git.md](round1/synth/05-sessions-git.md).

Source code only can settle these. Open-source harnesses only.

Codex (open CLI):
- Rollout files plus SQLite index: what exactly is the write path and recovery after a crash mid-write; are rollout files append-only and how does the index get rebuilt? Is the `thread/rollback` removal documented anywhere in code, and what replaced it?
- Resume: how are saved permissions and network approvals applied against managed policy (precedence) when the session resumes?

OpenCode:
- Snapshot store: how does it attribute changed paths to assistant steps, what is the exact tracked/untracked/ignored rule, and what does the 2 MiB cap do to larger files? How does `/undo` handle a path changed by both the user and the assistant?
- Session database engine and schema, and the event-log contract behind `GET /api/experimental/session/{id}/log` (exclusive sequence, ordering, retention).
- Worktree "strategies" plugin contract and what it may do with repo git config.

Oh My Pi and Pi:
- Branch navigation (`/tree`): what happens to file state, what is stored in `branch_summary`, and how is the active leaf persisted? Is the v3 format append-only or are entries rewritten?
- `omp commit`: how the dependency-ordered split and cycle rejection are actually implemented and what the mechanical fallback commits.
- `github` tool: how "push only to branches it checked out" is enforced.
- Pi `git-checkpoint.ts` example: what it does and whether it is a real checkpoint design.

DeepSeek harness:
- Persistence: zstd frame and checksum format, torn-tail truncation algorithm, per-batch fsync cost, and how `session/end-seed` is used on replay. Whether the `create_worktree` tool hardens git config. The `POST /github` webhook: dedupe and failure behaviour in code.
- Whether undo/checkpoints exist in code even though no page documents them.

OpenHands:
- Event-per-file persistence: crash consistency of `base_state.json` (rewritten whole) versus `events/`; what `fork()` copies and shares; encryption of persisted secrets (`OH_SECRET_KEY`).

OpenClaw:
- Writer claim: the transaction that checks it, behaviour on a superseded writer, and the worktree snapshot ref logic (`refs/openclaw/snapshots/<id>`), including how dirty-idle (>7 days) deletion is guarded.

goose:
- `goose review` flag semantics and orchestration default; session `sessions.db` schema; whether any checkpoint/undo exists in code.

Hermes Agent:
- Shadow-git checkpoints: the destructive-command detection list, the one-per-directory-per-turn rule, how nested repos become gitlinks, the pre-rollback snapshot, and exactly how user edits since the snapshot are detected and skipped. `hermes -w`: how hooks, fsmonitor and filters are disabled at checkout.
- Gateway crash handling: how a turn is marked `unknown` and what discarding does.

Zed/ACP:
- What the removed checkpoint code did (which git operations, how often, on what size repos) and whether any replacement is planned; how the worktree archive restore re-creates git state; commit-message generation prompt and the three-commit style examples.

Warp (open client):
- Handoff snapshot contents (declaration scripts or static JSONL), how untracked files are captured, and the "blocked while a command runs" rule that was unverified.

Cline:
- Shadow git: location on disk, exclusion rules, size handling, commit-per-tool-use cost, what happens to files created after the checkpoint, and the SDK's "one git process" diff mechanism.
- Hub: how sessions are persisted and re-attached across clients.

Not answerable from code (closed): Cursor checkpoint mechanism and retention, Claude Code transcript and checkpoint internals (public docs only), Antigravity `/rewind` behaviour.

### Extensibility (area 13)

From [round1/synth/06-extensibility.md](round1/synth/06-extensibility.md).

| Harness | Feature | Question the source must settle |
|---|---|---|
| Codex (CLI) | Hook trust | What exactly is hashed for a hook (command string, script file contents, plugin directory), where the trust record lives, and whether a plugin update re-triggers review. |
| Codex (CLI) | Hook handlers | Whether `prompt` and `agent` handlers are parsed and silently skipped or logged; what the 2,500-token default caps. |
| Codex (CLI) | App-server | Whether non-loopback WebSocket listeners really accept unauthenticated connections by default, and where `capability-token` is enforced relative to `initialize`. |
| Codex (CLI) | Code Mode | How the V8 runtime is isolated, what tool calls it can reach, and how the `direct`/`deferred`/`code_mode` surfaces are chosen per MCP server. |
| Codex (CLI) | MCP `environment_id` | How a server is placed on a remote executor and how that executor authenticates. |
| OpenCode | `execute` | Which JS engine runs Code Mode, how `fetch` is restricted, and how nested tool permissions are enforced inside a script. |
| OpenCode | Commands `!cmd` | The exact code path that runs command shell blocks outside the permission flow, and whether any policy layer sees them. |
| OpenCode | Plugins | Whether server plugins run in the same process with full rights; how `permission.evaluate` is ordered against configured deny and admin `policies`. |
| DeepSeek harness | MCP | Whether MCP tool calls pass through the approval service; whether OAuth exists; the exact tool-name format. |
| DeepSeek harness | Hooks | Whether user-facing shell hooks exist and what the "hook-bridge" to Claude Code mods does. |
| DeepSeek harness | Plugin market | What install verifies beyond format (signatures, hashes) and whether plugins run in-process. |
| Oh My Pi | `eval` | Whether Python/JS kernels can call agent tools, over what bridge, and with what approval class. |
| Oh My Pi | MCP auto-import | Whether servers imported from other apps' configs start automatically and under which approval tier. |
| Oh My Pi | Hooks | How `tool_call` fail-closed is implemented and whether other hook errors can still affect state. |
| Pi | Codemode default | Whether `builtin:codemode` and `builtin:tool-search` load by default in the CLI, and therefore whether MCP tools are callable by default. |
| Pi | Project trust | Which resources (`.pi/mcp.json`, extensions, prompts) are gated by project trust and what headless mode does. |
| OpenHands | MCP | Whether MCP server processes are sandboxed in Docker workspaces and which transports the SDK actually uses. |
| OpenHands | ACP hosting | How permission requests from an ACP-hosted agent reach the Canvas user, and what is auto-approved. |
| OpenHands | Hooks | Where the hook config file lives and how plugin hooks are merged with others. |
| OpenClaw | Plugins | Whether native plugins have any isolation; what `security.installPolicy` checks. |
| OpenClaw | `mcp serve` and ACP bridge | What authentication `openclaw mcp serve` requires and which session data it exposes. |
| goose | Code Mode | Boa context limits (time, memory, call count) and how `tools.call()` enforces per-tool permissions. |
| goose | Hooks | The fail-allow default path and whether `on_failure: block` covers timeouts. |
| goose | ACP providers | How goose's `auto`/`approve` modes are translated into each adapter's permission settings. |
| goose | Malware check | Data source and offline behaviour of the extension malware check. |
| Hermes Agent | Hooks | The shell hook schema and which events fail closed besides `pre_tool_call`. |
| Hermes Agent | Plugins | What the optional host-process isolation does and whether gateway hooks can be disabled. |
| Hermes Agent | Skills Hub | What the install scanner checks and how `dangerous` verdicts are computed. |
| Hermes Agent | `execute_code` | Whether the Python child is sandboxed beyond env stripping. |
| Zed | MCP forwarding | How Zed passes its MCP servers to external ACP agents and how its ACP permission forwarding rules combine with the agent's own. |
| Zed/ACP SDK | Rust crate | Whether `agent-client-protocol` supports any transport besides stdio today, and its licence. |
| Cline | Hooks | Which file-hook events exist beyond `PreToolUse`, and the VS Code `.clinerules/hooks/` loading path. |
| Cline | Plugins | What `CLINE_PLUGIN_MODE=sandbox` restricts in the subprocess. |
| Cline | Hub | Whether the local hub authenticates clients at all. |
| Warp (client) | MCP and ACP | Where MCP credentials are stored locally, and whether the client implements ACP. |

### Models and observability (areas 14, 19)

From [round1/synth/07-models-observability.md](round1/synth/07-models-observability.md).

Open-source harnesses only; Claude Code, Cursor, Antigravity and Claude Desktop are closed.

- **Codex, retry and overload.** How are retries counted and delayed (the entry only says `Retry-After` is honoured), what exactly makes it stop on "model not found", and does the WebSocket-to-HTTP fallback preserve partial output?
- **Codex, goals.** How is `tokenBudget` enforced: checked before each continuation or per model call, and what happens on overshoot?
- **Codex, OTel.** Where is the 2,048-byte tool-result cap applied and is prompt redaction a pattern match or a drop?
- **OpenCodex, combos.** How are hop errors classified, how is cooldown stored (memory only, as thread affinity is), and how does "metered-target consent" gate a hop? How does the estimated-cost table get its prices and how are coverage warnings computed?
- **OpenCodex, account pool.** What does "lowest-usage" compare across 5h/weekly/30d windows when windows disagree?
- **OpenCode, cost and stats.** How `opencode stats --cost` computes cost (price source, cache tokens) and how Go allowance windows are enforced in the client versus the Console.
- **OpenCode, retries.** Nothing documented; settle by reading the provider layer. Also what cache warming does and when `settings.cache.ttl` applies.
- **DeepSeek harness, telemetry.** What the redaction waterfall does by default when no rules are registered, and what the `token-meter` subsystem exposes for cost.
- **DeepSeek harness, compat flags.** How `supportsDeveloperRole`, `maxTokensField` and `thinkingFormat` change request bodies.
- **Oh My Pi, fallback and pools.** How `usageAwareFallback` reads coding-plan quota and applies the reserve; whether round-robin credential selection with session affinity exists as the README claims (`omp dry-balance`); the backoff constants in the `retry.*` namespace; where `omp stats` stores data.
- **Oh My Pi, roles.** How roles auto-resolve from authenticated models when `modelRoles` is empty, and why `advisor` will not inherit default.
- **Pi, retry.** How agent-level retry (3, 2000, 60000) and provider-level retry (0) interact on a streamed response; how billed vs estimated cost is determined; what `pi-telemetry` spans contain and who calls them.
- **OpenHands, LiteLLM layer.** What retry and fallback LiteLLM is configured with; how per-conversation budget meters and Enterprise budgets are enforced; which spans carry token counts.
- **OpenClaw, failover.** The exact profile-rotation and cooldown algorithm (30 s, 1 min, 5 min cap; 10 minutes for billing failures) and how "prefer the working fallback for 30 minutes" is stored; what `/usage cost` reads from providers.
- **goose, rate limits and logs.** Whether any generic retry exists beyond Bedrock and Databricks env vars; how `llm_request.N.jsonl` rotation works and whether it redacts headers; what the Langfuse integration sends.
- **Hermes Agent, fallback and pools.** The retry policy before fallback fires, the credential-pool rotation rules, how auxiliary fallback chains differ from the main chain, and what "spend limits" in the approval layer are.
- **Cline, cost.** How `totalCost` is computed (catalogue prices, cache pricing) and what the OpenTelemetry events contain.
- **Warp client (open), routing.** Where custom model routers are evaluated (client or Warp's control plane) and how fallback routing between comparable models is chosen.
- **Zed and ACP, config options and usage.** How `session/set_config_option` categories (`model`, `thought_level`) are rendered, and what the v1 usage update carries versus the end-turn usage RFD.

### UI, browser, collaboration (areas 15, 16, 18)

From [round1/synth/08-ui-browser-collaboration.md](round1/synth/08-ui-browser-collaboration.md).

Only source code can settle these, for the open-source harnesses (and the open client of Warp):

- **OpenCode, web app and pairing:** how the single-use pairing link, the 30-day cookie and the Basic-auth path are checked in the server; whether per-session data is scoped to the cookie or visible to anyone with the shared password; how the OpenTunnel end-to-end encryption is keyed.
- **OpenCode, policies layer:** how "only tightens" is evaluated when a policy allow and a policy deny both match (the review found a policy allow can lift an earlier policy deny); where the Console policy cache lives and what happens when the Console is unreachable.
- **OpenHands Canvas:** what authentication exists between the Canvas client and the Agent Server beyond `X-Session-API-Key`; how the confirm/reject UI is wired to the confirmation policy; what the open tier does when exposed with `--public`.
- **OpenClaw, Control UI and operator scopes:** how a scope ceiling is enforced (the survey says it can only filter); how session visibility (`none/view/suggest/write`) is enforced on the WebSocket; what `openclaw policy` attestation checks.
- **OpenClaw, approval delivery:** how `/approve <id>` from a chat channel is bound to the approver list and to the exact command (argv, cwd, env, pinned executable) so a replayed approval cannot widen a different call.
- **Hermes dashboard and gateway:** the non-loopback auth gate (what it checks before the dashboard starts, how sessions are issued); whether `hermes serve` is a separate process; how the delivery ledger records the "unknown" crashed-turn state.
- **Hermes browser vault and relay:** how `browser_vault` verifies the origin in-page before a fill; how the relay's per-gateway secret and delivery key are scoped.
- **DeepSeek harness web server:** how the launch token, signed cookie and Host/Origin fence work; whether the agent loop and HTTP server really share a process in all web profiles; how `follow()` plus `page()` fill gaps after reconnect.
- **Cline hub and Kanban:** how the hub binds and authenticates (loopback WebSocket, `--room-secret` for non-local hosts); what the Kanban server exposes with no authentication; which enterprise remote-config keys are honoured by CLI and Desktop code paths.
- **Oh My Pi collab:** the relay protocol and what metadata the relay sees; how guest prompts are checked against host policy; how `omp browser-relay` authenticates the extension and what the optional token protects.
- **goose roaming and remote serve:** how peer trust, revocation and "force close" are implemented; why spawned OS processes survive revocation; the `X-Secret-Key` check and TLS fingerprint pinning path.
- **Zed/ACP:** how the Agent Panel's hunk accept/reject maps to buffers and whether the review state survives a restart; how Zed forwards approval requests from external agents and applies its own `tool_permissions` to them (the page is silent).
- **OpenCodex:** the remote hub pairing, key rotation overlap and the unauthenticated loopback companion listener; whether Remote Workspace (experimental) is the same feature as the v2.100.0 `ocx-child` relay.
- **Warp client (open) :** where the code review comment injection into third-party CLI agents lives and what it sends; whether the admin-enforced denylist is checked client-side only.
- **Codex CLI:** how `requirements.toml` layering and the config server decide precedence at runtime; how the app-server WebSocket authentication is enforced before `initialize`; how `remote-control` pairing codes expire.
- **Pi:** whether any first-party web UI package still ships in the monorepo.

### Remote execution and architecture (area 17)

From [round1/synth/09-remote-distributed-architecture.md](round1/synth/09-remote-distributed-architecture.md).

For the open-source harnesses (and open parts), what only reading the code can settle:

- **Codex (CLI, open):** how app-server WebSocket auth is enforced before `initialize`, and what exactly the non-loopback "unauthenticated during rollout" default is in code; queue sizes behind `-32001`; the daemon's Unix socket permissions; what `codex exec-server --remote` and remote executors are and how they authenticate; how Handoff packages Git state.
- **OpenCode:** where the server password and pairing tokens are stored and compared; what OpenTunnel's end-to-end encryption actually is; the durable session log's ordering and retention guarantees versus the volatile event stream; what the `process-daemon` release tags are.
- **OpenCodex (lidge-jun):** which side configures the Remote Workspace approved root and read-only mode; the RPC v2 handshake (key confirmation, replay protection); how the bubblewrap probe gates writes; what `ocx-child` is.
- **DeepSeek harness:** whether the web profile runs the loop in the HTTP server process; the SSH backend's helper hash check and what "SSH loss is final" does to an in-flight tool call; how `follow()`/`page()` reconcile gaps.
- **Oh My Pi:** whether `ssh://` support extends to `bash`; how the auth broker scopes and expires tokens it hands out.
- **Pi:** pi-env's framed stdio protocol (framing, auth, error model); how a CLI session would attach to pi-durable; how the restart limit and commit-before-display are implemented.
- **OpenHands:** whether `X-Session-API-Key` is checked on WebSocket upgrades as well as `/api/*`; how client-supplied per-conversation container limits reach Docker without server caps; how Canvas reconnects a running conversation; deferred-init key handling.
- **OpenClaw:** where node command policy is enforced (Gateway before `node.invoke`, node on receipt, or both); the cloud worker route's method allowlist and owner-epoch fencing; the actual transport and direction of the worker connection; the durable writer claim transaction.
- **goose:** whether the `serve` secret comparison is constant-time and where it is read from; Roaming's trust store, revocation path and why spawned processes survive revoke; the `token` query parameter's exposure in logs.
- **Hermes Agent:** the environment `execute()` contract for remote backends (env stripping, timeouts); why container backends skip dangerous-command approval in code; the gateway's 30 s ticket mechanism; the Relay wire contract.
- **Zed/ACP:** where LLM calls originate in remote development; how the ACP Rust SDK frames stdio and whether it already has a custom-transport seam a node could use; how Zed applies its own policy to `terminal/create` from external agents.
- **Warp (client, AGPL):** where the local agent loop runs and whether local model calls are proxied; the `oz-agent-worker` licence and its connection and credential handling (if the worker source is public).
- **Cline:** whether the hub on `127.0.0.1:25463` authenticates connecting clients at all; what the `remote` backend mode does; how the SSH helper receives and holds the provider key in memory; how the 45 s tunnel timeout is implemented.

### Configuration, licence, other (areas 20, 22, 23)

From [round1/synth/10-config-licence-distinctive.md](round1/synth/10-config-licence-distinctive.md).

- **Codex, project config blocklist:** is the blocklist (`openai_base_url`, `model_provider(s)`, `notify`, `otel`, ...) enforced at config-load time for every client (CLI, app-server, SDK) or only in the CLI? Where is `requirements.toml` merged relative to the config-server response?
- **Codex, hooks and plugin trust-by-hash:** what exactly is hashed (definition, script bytes, path) and where is the trust decision stored?
- **OpenCode, `policies` evaluation:** how "reverse, broader wins" is implemented, and how a policy allow can lift an earlier policy deny.
- **OpenCode, system dir loading:** how agents, commands, skills and plugins from `/etc/opencode` interact with the plugin "not a sandbox" statement.
- **OMP, profile replace semantics:** exactly which settings a named profile replaces versus the global file; how invalid YAML is set aside before startup stops.
- **Pi, codemode default and project trust:** run the CLI to settle the codemode/`tool_search` default contradiction; read the trust resolution order and the headless behaviour.
- **DeepSeek harness, config patches:** the layer merge order in code, default storage paths and per-user scoping, and how optimistic revisions reject conflicting settings edits.
- **goose, config precedence:** env versus file versus project `settings.json` in code; the `GOOSE_SANDBOX` macOS sandbox; `init-config.yaml` and the custom providers dir.
- **Hermes Agent, managed scope:** which keys are "pinned", how managed beats user beats env is implemented, and how profile distributions exclude credentials.
- **OpenClaw, config guard:** how the "accidental data-loss write" guard decides, and how last-known-good fallback works for secrets.
- **Cline, project vs global precedence:** which wins for each of rules, skills, hooks, MCP; whether remote config can be overridden on CLI.
- **OpenCodex, client-config editing:** how restore works on disable ("not byte for byte"), and where lock files apply.
- **OpenHands, Agent Server config files:** formats beyond `OH_*` env vars; whether Enterprise server code is public and its licence.
- **Warp (AGPL client), settings:** the `settings.toml` hot-reload and invalid-value fallback code; precedence between app, CLI, admin and project.
- **Zed, agent settings:** user versus project precedence for `agent.*` keys.
- **Claude Code (closed):** nothing in source; these areas rely on docs only. The settings precedence and ratchets could only be settled by running the binary.
