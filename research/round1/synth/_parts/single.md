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
