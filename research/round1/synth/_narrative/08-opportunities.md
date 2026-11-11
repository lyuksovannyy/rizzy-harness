## 8. Potential opportunities for our project

Two warnings before the list. First, `CLAUDE.md` puts a feature freeze in place until Gate 3 of the fix plan. Most items below are features and cost against that plan; a few are correctness fixes and are marked as such. Second, every "ours" statement in the synthesis describes branch `main`. Where pull request 4 (`integration/gate3`, open, CI green, awaiting an outside security review) or the local branch `feat/agent-roster` changes our position, it is stated per branch.

### Where we are ahead

Ahead on structure, not on feature count. Several of these leads exist only once gate3 merges.

| Lead | Evidence from the field | Branch |
|---|---|---|
| **The execution side is the authority.** `node.toml` is the only policy source; approval mode is `min(requested, node max)`; nothing over HTTP or the link writes it. | No surveyed design documents an execution-side file that outranks the control side. Claude Code's runner receives tool lists and permission rules from the control plane (https://code.claude.com/docs/en/self-hosted-environments-configuration.md, corrected); Warp pushes resolved config (https://docs.warp.dev/platform/architecture/); OpenClaw's Gateway allowlist is the gate (https://docs.openclaw.ai/nodes/command-policy). | `main` |
| **The owner owns the control side.** Every event mirrors to the master the owner runs. | Claude Code's runner and Remote Control store transcripts at Anthropic; Cursor's worker sends file contents and terminal output; Warp stores transcripts by default. See the control-side table in `round1/synth/09-remote-distributed-architecture.md`. | `main` |
| **Multi-user with private chats in the open tier.** Accounts, workspaces, roles, chats private to their owner. | OpenHands keeps RBAC and SAML in Cloud and Enterprise (https://docs.openhands.dev/openhands/usage/agent-canvas/agent-profiles.md); OpenClaw has multi-user in open code but says scopes are not tenant isolation (https://docs.openclaw.ai/gateway/operator-scopes). See "Accounts, roles" in `round1/synth/08-ui-browser-collaboration.md`. | `main` |
| **Children start from an empty environment** plus an allow-list; provider keys always stripped. | Codex passes `*KEY*`, `*SECRET*`, `*TOKEN*` by default (https://learn.chatgpt.com/docs/config-file/config-advanced.md); Claude Code's sandbox inherits secrets unless an opt-in scrub is set (https://code.claude.com/docs/en/env-vars.md). No other harness documents an empty-environment start. | `main` |
| **Prompt-injection containment on the web path.** `web_fetch` only fetches URLs that appeared in the user's words or tool output; SSRF-safe resolver re-checked per redirect; untrusted webhook turns lose memory and web tools; no model HTML. | No other surveyed harness documents URL provenance. Gate3 tightens the provenance set, makes sub-agents inherit it, and closes IPv6 transition-address gaps (AUDIT L11, L12). Caveat: this covers the chat runtime only; agentic sessions reach the network through `bash`. | `main`, tighter on gate3 |
| **Agent-written memory needs owner approval** before it becomes active. | Only Antigravity `/learn` (https://antigravity.google/docs/commands/learn) and Hermes, opt-in and off by default (https://hermes-agent.nousresearch.com/docs/user-guide/features/memory), gate writes. Claude Code, Codex, OpenHands and OpenClaw do not. On `main` project members can activate project facts (AUDIT S12); gate3 restricts activation to a manager, which closes S12. | lead complete on gate3 |
| **Shadow-repo checkpoints with hardened git** (cleared env, no system or global config, no hooks), working on non-git folders. | Only OpenCode, Hermes and Cline document shadow git; none documents comparable git hardening (nothing documented, not proof of absence). See "File checkpoints and undo" in `round1/synth/05-sessions-git.md`. Unmeasured at scale: see "Worth studying". | `main` |
| **Unattended runs fail safe.** Agentic routines cannot run in `auto`; the webhook entry has a hashed per-routine secret, idempotency keys and token buckets. | Cline schedules default to yolo; Cursor cloud agents never ask; DeepSeek's SDK pins full access (see F17 in `round1/synth/02-permissions-sandbox-security.md`). | `main` |
| **Command output and cache discipline.** Output filter with a retrievable full copy under per-session random ids; observation masking batched to protect the prompt cache. | Claude Code and DeepSeek spill to a file; no other harness documents cache-keeping batch masking. See "Pruning" and "Large-output handling" in `round1/synth/03-context-memory.md`. | `main` |
| **Fallback is visible.** Per-agent fallback chains with a `model_fallback` event; no switching mid-stream. | Claude Code's fallback is not shown in `/status` (https://code.claude.com/docs/en/model-config.md); OpenCodex fails over only before output (https://opencodex.me/guides/combos/). | `main` |
| **Fail-closed sandbox default.** `[sandbox] mode = required` refuses agentic sessions where Landlock is not enforced. | Claude Code silently runs unsandboxed by default (https://code.claude.com/docs/en/sandboxing.md); Zed on Linux falls back with a warning (https://zed.dev/docs/ai/sandboxing.md). | gate3 only |

### Table stakes we lack

Ranked by how much each matters for a self-hosted master/node harness that runs shell commands on other people's machines.

#### 1. A shell sandbox, on `main`

- **What.** Kernel confinement of agent shell commands. On `main`, `bash` runs as the node's OS user, can read the data directory and the provider key, and has no network control (AUDIT S1).
- **Who and how.** Seven of sixteen harnesses document one. Copy **Codex** for posture: fail closed when the policy cannot be enforced, protected subpaths (`.git`, agent config, resolved gitdir) read-only inside writable roots, and candid limits in the docs (https://learn.chatgpt.com/docs/sandboxing.md, https://learn.chatgpt.com/docs/agent-approvals-security.md). Copy **DeepSeek harness** for honesty: report `full` or `partial` enforcement per backend (https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/sandbox). **Cursor** is the Landlock-plus-seccomp reference on Linux 6.2+ (https://cursor.com/docs/agent/security/run-modes).
- **What goes wrong.** Claude Code's default silently runs unconfined when the sandbox cannot start. Codex rejects Landlock for filesystem-restricted policies because it cannot isolate Unix sockets: that is exactly our residual.
- **Invariants.** Fits "the node decides": the mode lives in `node.toml`.
- **Dependency.** This is pull request 4. It adds Landlock confinement (Linux 6.2 or newer, optional TCP block), fails closed, and has known residuals: pathname Unix sockets such as the Docker socket or D-Bus are blocked only from Linux 7.1, and UDP and DNS are not covered. Merging it is Gate 3 work, not a feature.
- **Size.** Small to finish (the code exists and is under review). Small to add per-node `full`/`partial` reporting and OS-level protection for `.git/hooks` and `.git/config`.

#### 2. Context handling keyed to the real model window, with overflow recovery

- **What.** Our trigger is a characters-divided-by-four estimate against a fixed 24,000; a context-length error ends the turn; there is no `/compact`. On nodes with different models this is a correctness bug.
- **Who and how.** Fifteen of sixteen key compaction to the window or a reserve. Copy **Pi**: compact at window minus a 16,384-token reserve, keep about 20,000 tokens verbatim, and on an overflow error compact and retry once (https://pi.dev/docs/latest/compaction). **OpenCode** does the same with a 15% buffer (https://opencode.ai/docs/compaction/). Add **OpenClaw**'s summary audit (https://docs.openclaw.ai/concepts/compaction).
- **What goes wrong.** Hermes documents that a summary model with a smaller window than the main model silently drops middle turns (https://hermes-agent.nousresearch.com/docs/developer-guide/context-compression-and-caching). Claude Code stops after repeated compactions that refill the context, and notes that a gateway rewriting the too-long error breaks reactive compaction.
- **Invariants.** Compaction must be an event sequence written by the session actor (DeepSeek logs it as a locked start/end sequence and treats a start without an end as a crash orphan).
- **Dependency.** None. Arguably a fix, not a feature.
- **Size.** Medium.

#### 3. Provider retry and backoff

- **What.** A single 429 or 5xx either triggers a fallback model or ends the turn.
- **Who and how.** Copy **Pi**'s small, exact defaults: 3 retries, 2,000 ms base, 60,000 ms max, provider-level retries 0 so layers do not multiply (https://pi.dev/docs/latest/settings). For unattended routines, take **Claude Code**'s rules: never retry a spend-limit 429, stop when a gateway's `retry-after` exceeds 60 s, abort quiet streams with a watchdog (https://code.claude.com/docs/en/errors.md, unreviewed). Take **Hermes**' ordering: fall back after retries for 429 and 5xx, immediately for 401, 403 and 404.
- **What goes wrong.** Retry layers that multiply; retrying billing failures.
- **Invariants.** Node-side; none touched.
- **Dependency.** None. A fix.
- **Size.** Small.

#### 4. Web tools and memory proposals on agentic nodes

- **What.** `web_search`, `web_fetch` and `remember` are registered only in the chat runtime. An agentic Software Developer has no web search; a Researcher delegated on a node has no web tools; agentic sessions can read memory but never propose it. The README and the built-in agent definitions promise otherwise.
- **Who and how.** Eleven of sixteen harnesses ship web fetch and search. Our own chat-runtime implementation is the model to copy, because no other harness documents URL provenance.
- **What goes wrong.** Today agentic sessions reach the network only through `bash`, which bypasses provenance, the SSRF resolver and the untrusted-turn rules. Adding web tools without the sandbox's network block leaves both paths open.
- **Invariants.** "The master never runs tools": the web tools must run on the node, not be relayed to the master's chat runtime. That puts any search-provider key on the node as well, which feeds the credentials question below. The untrusted-webhook rules must carry over.
- **Dependency.** `feat/agent-roster` (the roster assumes these tools) and gate3 (tighter provenance, sub-agents inheriting the parent's allowed set, network block for `bash`).
- **Size.** Medium.

#### 5. Steering and queueing while a turn runs

- **What.** A prompt during a running turn is rejected with "a turn is already running". Twelve of sixteen accept it. For a web UI driving a remote node, cancel is the only recourse today.
- **Who and how.** Copy **OpenClaw**: accepted input is stored before the Gateway acknowledges it, with `steer` as the default mode, a debounce and a cap of 20 per session (https://docs.openclaw.ai/concepts/queue). Add **Claude Code**'s protocol details: per-message priority (`next`, `later`, `now`) and an interrupt receipt listing still-queued message ids, which a web client that loses its connection needs (https://code.claude.com/docs/en/agent-sdk/typescript.md, unreviewed).
- **What goes wrong.** Queue-by-default versus steer-by-default differs across harnesses; pick one and show it.
- **Invariants.** One writer per session: the master relays, the node's session actor appends the queued message as an event and decides the step boundary.
- **Dependency.** Gate3's per-session command lanes, so cancel and approvals are never stuck behind a slow command.
- **Size.** Medium.

#### 6. Repository instruction files and skills, read on the node

- **What.** Fourteen of sixteen read `AGENTS.md` or an equivalent; sixteen of sixteen read `SKILL.md` skills. We read neither; our project instructions live on the master, away from the code.
- **Who and how.** Read `AGENTS.md` and `.agents/skills/` (the only cross-vendor skill path) with progressive loading: name and description up front, body on demand. Copy **Codex**'s project-config key blocklist the moment any repo config is read (https://learn.chatgpt.com/docs/config-file/config-advanced.md) and **Pi**'s separate project trust gate (https://pi.dev/docs/latest/security).
- **What goes wrong.** Skill listings overflow the context (Codex and Claude Code both added budgets); skills that run shell at load time (OpenCode's run outside permissions); project skills trusted automatically; frontmatter that grants tools (Claude Code `allowed-tools`); headless modes that treat any folder as trusted (Claude Code `-p`). See "Skills" in `round1/synth/06-extensibility.md`.
- **Invariants.** The node reads them, consistent with "the node decides". Nothing a repository ships is trusted: no `!cmd`, no `allowed-tools`, no frontmatter hooks in the first version.
- **Dependency.** None, but it breaks the freeze.
- **Size.** Medium.

#### 7. A plan mode enforced by the node

- **What.** No read-only plan-then-approve phase. Ten of sixteen ship one.
- **Who and how.** Copy the enforcement of **OpenCode** (the `plan` agent's rules deny edits outside a plan directory, https://opencode.ai/docs/agents/) and the **Antigravity CLI** (`--mode=plan` allows only read and search tools, https://antigravity.google/docs/cli/modes). Record the mode as an event, as DeepSeek logs `plan/mode`. Copy the review UX of **Antigravity** (comments attach to the next message, https://antigravity.google/docs/plan) and **Oh My Pi**'s "execute in a fresh session with only the plan" (https://omp.sh/docs/plan).
- **What goes wrong.** DeepSeek's plan mode is prompt guidance that the sandbox ignores; Claude Code does not enforce it in interactive terminals where bypass is available (https://code.claude.com/docs/en/permission-modes.md); Cline's lets `python -c` write (https://docs.cline.bot/features/subagents.md).
- **Invariants.** A node-enforced tool and policy state, approved through the existing approval path. A plan mode that keeps an unconfined `bash` is not read-only.
- **Dependency.** Gate3 sandbox (read-only `bash` needs it); the roster (a Plan agent).
- **Size.** Medium.

#### 8. Whole-turn diff review with comments back to the agent

- **What.** We show the old and new strings of one `edit` call and a per-turn file list. The field's standard is a review pane with per-hunk revert and inline comments sent with the next message.
- **Who and how.** Copy **Claude Desktop** (comment on a line, sent with the next message, https://code.claude.com/docs/en/desktop) and **Antigravity** (Uncommitted, Branch and Agent Edits views, https://antigravity.google/docs/review-changes). The data already exists: the shadow repo holds the before and after of every turn.
- **What goes wrong.** Claude Code's web review lists only Claude's own edits, so formatter changes are invisible, and ignores repo diff drivers (https://code.claude.com/docs/en/claude-code-on-the-web.md); Antigravity's discard deletes untracked files immediately.
- **Invariants.** Diff content is file content leaving the node, like every other event; render as text, never HTML.
- **Dependency.** None.
- **Size.** Medium.

#### 9. Background processes and timeout-to-background

- **What.** An agent cannot run a dev server or watcher without blocking until the timeout kills it. Eleven of sixteen document background processes.
- **Who and how.** Copy **Claude Code**: a foreground command at its timeout moves to the background with a task id and output path instead of being killed, and `Monitor` turns background output into conversation events with a mandatory deadline (https://code.claude.com/docs/en/tools-reference.md). **Hermes** auto-backgrounds a trailing `&` and refuses `nohup` and `disown` (https://hermes-agent.nousresearch.com/docs/user-guide/features/tools, unreviewed).
- **What goes wrong.** Pi's answer is "use tmux", which only works where a user is present. goose documents that spawned processes survive revocation, and the same risk applies to any process that outlives its session.
- **Invariants.** Background children must stay inside the sandbox and die with the session; the harness process stays light.
- **Dependency.** Gate3 sandbox and cancel handling.
- **Size.** Medium to large.

#### 10. An MCP client on the node, with tool deferral from day one

- **What.** No MCP at all; sixteen of sixteen have it.
- **Who and how.** **Pi**'s per-server exposure modes (`codemode`, `deferred`, `direct`, `hidden`) are the systematic answer to context bloat (https://pi.dev/docs/latest/mcp); Pi argued against MCP for that reason, then shipped it. **Claude Code** tool search is the deferral reference (https://code.claude.com/docs/en/mcp.md).
- **What goes wrong.** Project-supplied servers launching processes in headless runs, servers running outside the sandbox and permission rules, egress controls that miss MCP (Codex's proxy), schemas providers reject, tool descriptions used as an injection channel. See "MCP" in `round1/synth/06-extensibility.md`.
- **Invariants.** Only `node.toml` may add a server: a stdio server is arbitrary code, so no HTTP or protocol path may add one. The approval binding must be per call, not per tool name per turn, before MCP tools exist.
- **Dependency.** Gate3; approval binding (below).
- **Size.** Large.

### Worth copying

Ranked. Each is small enough to adopt without a design debate.

1. **Bind approvals to argv, cwd, env and executable (OpenClaw).** Our "allow for this turn" keys on the tool name, so one click allows every later `bash` command of the turn (AUDIT L10). Gate3 honours it only after a tool's first approval, but does not add argument binding. OpenClaw binds to exact argv and cwd, the env when present, a pinned executable path and a content hash for writable executables, and re-checks before launch (https://docs.openclaw.ai/tools/exec-approvals). Fits node-side policy; record the binding in the approval event. Depends on gate3. Size: small to medium.
2. **A failed audit append rejects the approval (DeepSeek harness),** plus a decision-source field on each approval event (`config`, `hook`, `user_permanent`, `user_temporary`, as Claude Desktop's telemetry records, https://claude.com/docs/cowork/monitoring.md). Matches one writer per session exactly (https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval). Size: small.
3. **Park unattended approvals instead of blocking (Claude Code `defer`).** A PreToolUse hook ends the headless turn with the call pending; the host resumes after a human decides, with no timeout (https://code.claude.com/docs/en/hooks.md, unreviewed; one deferred call per turn). Our routines block a turn for up to 24 hours. The pending call is a log event, so it survives a restart. Pair it with "auto inside a required sandbox" for agentic routines once gate3 lands: no competitor documents that combination with node authority. Size: small to medium.
4. **Scan sub-agent reports before the parent reads them (Claude Code).** Neutralise imitations of harness tags, mark reports as carrying no user authority (https://code.claude.com/docs/en/agent-sdk/subagents.md, unreviewed). Applies to sub-agent and future cross-node reports. Size: small.
5. **Fence stale nodes and forbid fallback (OpenClaw, OpenCodex).** A node that reconnects with a rotated credential or stale state is rejected (owner-epoch fencing, https://docs.openclaw.ai/gateway/cloud-workers/security-model); a session bound to an offline node never runs elsewhere (https://opencodex.me/guides/remote-workspace/). Both protect one writer per session. Size: small.
6. **Routine pre-checks and a catch-up rule.** Hermes' `wakeAgent: false` lets a script decide whether to pay for an agent run (https://hermes-agent.nousresearch.com/docs/user-guide/features/cron); Claude Desktop runs one catch-up for missed scheduled tasks within 7 days (https://code.claude.com/docs/en/desktop-scheduled-tasks, corrected for other details). Size: small.
7. **An explicit overload reply on the link (Codex).** Gate3 bounds the link (8 inbound messages per node, 64 outbound commands, 8 MiB frames); Codex's App Server answers `-32001` "Server overloaded" rather than stalling (https://learn.chatgpt.com/docs/app-server.md). Size: small, after gate3.
8. **Cost as a labelled estimate, budgets checked before a turn starts.** OpenCodex labels cost an API list-price equivalent, not billing, with coverage warnings (https://opencodex.me/guides/web-dashboard/, corrected; see "Token and cost display" in `round1/synth/07-models-observability.md`); OpenCode Console enforces at request start with a 429 (see "Budgets and limits" in the same file). Our `budget` frontmatter field is parsed and never read, which is worse than absent. Budgets only tighten, so the master enforcing them for chat does not conflict with "the node decides"; agentic budgets belong on the node. Size: medium.
9. **Secrets-aware session export (Hermes `--redact`, OpenCode `--sanitize`).** Ten of sixteen can export; we cannot. Owner-only export keeps chats private. Size: small.
10. **Log lifecycle.** Our log keeps every streaming delta forever (AUDIT D5). Claude Code sweeps after 30 days, OpenClaw has `pruneAfter` and `maxEntries`, and DeepSeek's checksummed zstd frames with torn-tail truncation handle growth and torn writes (https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/persistence). Before the first tagged release this can change the schema in place. Size: medium.

### Worth studying before deciding

1. **Where model credentials live.** Every node holds a long-lived provider key. Options: (a) keep node-held keys behind the gate3 sandbox (simplest, keeps "the node decides" whole); (b) a node-local credential proxy outside the sandbox (Hermes iron-proxy, https://hermes-agent.nousresearch.com/docs/user-guide/egress/iron-proxy; Pi's Docker Sandboxes pattern, https://pi.dev/docs/latest/containerization), which keeps the key on the node but out of the agent's reach, at the cost of TLS interception; (c) inference relayed through the master (OpenClaw cloud workers, https://docs.openclaw.ai/gateway/cloud-workers), which removes keys from nodes but makes nodes depend on the master and adds a single point of failure. See the central tension in section 7. Decide before Gate 3 work on the link. Size: medium for (b), large for (c).
2. **Per-session credentials on the link.** After enrolment one long-lived Ed25519 node key does everything. Cursor mints per-claim session tokens (https://cursor.com/docs/cloud-agent/self-hosted/pool); OpenClaw stores per-dispatch credentials only as a hash; Claude Code's work orders register exactly one runner and expire with a 120-second spawn lease, and every session gets a short-lived JWT (https://code.claude.com/docs/en/self-hosted-environments-configuration.md, corrected). For us the risk is lower because the node dials and holds its own key; the gain is revoking one session without dropping the node. Size: medium.
3. **A model-based approver.** Twelve harnesses ship one and every vendor caveats it: Cursor "not a security boundary" (https://cursor.com/docs/agent/security/run-modes), Claude Code "does not guarantee safety", goose fails open. Only safe behind a sandbox. If built, copy the converged safeguards: strip tool results from the reviewer's input, fail closed on reviewer error, circuit-break to a human after a few denials (Codex: 3 consecutive or 10 of 50, https://learn.chatgpt.com/docs/sandboxing/auto-review.md), drop broad allow rules while active. Run it on the node, with the node's model, so no command context leaves for a third party (Cursor's runs on its backend). Size: medium.
4. **A master-proposed policy that a node may only tighten.** Admins of a fleet will want to distribute policy. Codex's `requirements.toml` with a self-hosted config server denies omitted profiles including future ones (https://learn.chatgpt.com/docs/enterprise/configuration-server.md); OpenCode's "only tightens" layer can still lift an earlier deny (https://opencode.ai/docs/policies/, corrected), which is the bug to avoid. The node must remain the authority: a proposal is applied only as `min(proposal, node.toml)`. Size: medium.
5. **ACP, in which role.** Hosting Claude Code or Codex through `AgentBackend` (client role) is the bigger product lever; serving ACP from a node (agent role) lets editors attach. ACP's remote transport is still an RFD, its permission requests are advisory and the agent stores "always" decisions (https://agentclientprotocol.com/protocol/v1/security.md), and no host documents how hosted agents' approvals reach a human. Design the approval mapping onto `node.toml` before building. Size: large.
6. **Checkpoints at scale.** Our shadow-repo checkpoints are a lead today, but Zed removed per-prompt Git checkpoints in 1.27.0 because they slowed or hung large repositories (https://zed.dev/releases/stable, corrected); Cline documents large-repo slowness; Hermes caps checkpoints at 500 MB total, 10 MB per file and skips above 50,000 files; Claude Code lists what it does not restore (Bash-only changes, sub-agent edits, symlinks). We have only a count cap (latest 10). Measure on a large repository before claiming the lead. Size: small to measure.
7. **Fork with a file baseline.** Twelve of sixteen fork sessions; OpenHands and Antigravity explicitly do not fork file state. A Pi-style parent pointer over our append-only log (https://pi.dev/docs/latest/session-format) plus a shadow-repo baseline would be a fork that carries consistent files, which no harness documents. Size: medium.
8. **Moving a session between nodes.** Codex Handoff moves a chat with its Git state (https://learn.chatgpt.com/docs/remote-connections.md, corrected); Warp snapshots tracked and untracked uncommitted changes (https://docs.warp.dev/platform/handoff/). Our single-owner rule and per-node `node.toml` make this a design decision, not a copy. Size: large.

### What we should not copy

- **Controls their own vendors say are not a security boundary, used as one.** Claude Code's server-managed settings and Bash rules (https://code.claude.com/docs/en/server-managed-settings.md, https://code.claude.com/docs/en/permissions.md); Cursor's Auto-review (https://cursor.com/docs/agent/security); Oh My Pi's approvals and sub-agent isolation (https://omp.sh/docs/approvals, https://omp.sh/docs/subagents); Cline's plan mode and `.clineignore` (https://docs.cline.bot/customization/clineignore.md); OpenClaw's operator scopes (https://docs.openclaw.ai/gateway/operator-scopes); OpenHands' security analyzers (https://docs.openhands.dev/sdk/guides/security.md); OpenCode's policies for plugin code (https://opencode.ai/docs/policies/); ACP's permission requests. These are UX. Containment comes from the sandbox.
- **Plan mode as prompt guidance** (DeepSeek harness, and Claude Code in interactive terminals with bypass available).
- **The acting model judging its own risk.** Cline's model-set `requires_approval` (https://docs.cline.bot/features/auto-approve.md), OpenHands' inline `security_risk` argument, Warp's "Agent decides".
- **Fail-open judges and hooks.** goose's adversary reviewer (https://goose-docs.ai/docs/guides/security/adversary-mode.md), Hermes' goal judge on error (https://hermes-agent.nousresearch.com/docs/user-guide/features/goals), Claude Code mods by default (corrected in review: the built-in guard loads only with managed settings or a Team or Enterprise login).
- **Permissive defaults.** Oh My Pi's `yolo` default; Cline schedules defaulting to yolo and its Telegram connector approving everything (https://docs.cline.bot/cli/connectors.md); Codex's token pass-through; Claude Code's silent sandbox fallback; Codex's unauthenticated non-loopback App Server during rollout.
- **Skipping approvals by backend.** Hermes container backends skip dangerous-command approval and turn checkpoints off (https://hermes-agent.nousresearch.com/docs/user-guide/security, corrected). Any such rule belongs in `node.toml`, explicit.
- **Approval requests without arguments** (DeepSeek harness): the approver is approving blind.
- **Shell that bypasses the permission flow,** such as OpenCode's command `!cmd` blocks (see "Custom slash commands" in `round1/synth/06-extensibility.md`).
- **Link sharing of chats.** OpenCode discontinued it in v1.19.1 (https://opencode.ai/changelog/v1.19.1, corrected), and "chats are private to their owner" argues against it. End-to-end sealed relays (Oh My Pi collab, https://omp.sh/docs/collab) conflict with a master that must read the log.
- **One shared "small model" key.** Claude Code's `ANTHROPIC_DEFAULT_HAIKU_MODEL` also moves the goal evaluator and the `haiku` alias (https://code.claude.com/docs/en/model-config.md).
- **The loop on the control side** (Cursor self-hosted workers). It would break "the master never runs tools" in spirit and send every file the agent reads to the control side.
- **In-process code plugins and unsandboxed marketplaces.** Oh My Pi `auto` updates let a publisher ship code without approval; DeepSeek's `http:` plugin indexes are tamperable. If we ever package, package declarative parts only.
- **Subscription logins.** Anthropic's terms do not let a third-party harness offer claude.ai login; the carve-out allows an end user to sign in to the unmodified `claude` binary (https://code.claude.com/docs/en/agent-sdk/overview.md, corrected in review). Antigravity forbids third-party tools on its consumer service (https://antigravity.google/docs/faq).
- **"Sandboxed" wording for a browser that drives a live profile** (Antigravity, https://antigravity.google/docs/commands/browser).

### The two things the owner is designing now

#### The built-in agent roster (`feat/agent-roster`)

How the field builds rosters (the full table is the "Built-in agent roster" entry in `round1/synth/04-planning-subagents-background.md`):

- **Small rosters, three to six agents, split by capability:** a read-only explorer, a planner and a general worker. Codex ships `default`, `worker`, `explorer` (https://learn.chatgpt.com/docs/agent-configuration/subagents.md); OpenCode `build`, `plan`, `general`, `explore` plus hidden `title`, `summary`, `compaction` agents that are really model slots (https://opencode.ai/docs/agents/); Claude Code Explore, Plan, general-purpose and a few helpers (https://code.claude.com/docs/en/sub-agents.md); Cursor Explore and Browser.
- **Built-ins are mostly sub-agent only.** Claude Code's and OpenCode's explore and general agents cannot be the main agent.
- **Seven harnesses ship no named roster** (DeepSeek harness, OpenHands, OpenClaw, goose, Hermes, Cline, Pi) and rely on custom definitions.
- **Read-only is enforced by tool rules, not names.** Claude Code's Explore has Write and Edit denied; OpenCode's `plan` denies edits outside its plan directory; Codex agent files cannot override sandbox mode, approval policy, provider or MCP servers; Claude Code plugin agents ignore `hooks`, `mcpServers` and `permissionMode`; Antigravity agents carry a `commandExecutionPolicy` defaulting to `sandbox` (https://antigravity.google/docs/subagents). The warning case is Cline: a plan mode that removes edit tools but keeps a shell lets `python -c` write.

What our roster gets right:

- **Server-side enforcement of where an agent may run** (`available_as`, refused by `Resolver::top_level`), rather than only hiding it in a picker. The field documents where built-ins run; none documents refusing a sub-agent-only agent at the server.
- **A stale or unknown agent id is an error** (`AgentGone`), not a silent switch to another agent.
- **Unknown frontmatter fields are rejected.** Claude Code silently ignores them and skips files with bad YAML.
- **A default agent per mode** (chat: Assistant; Agentic: Software Developer), refusing a tool-less agent as the agentic default.
- **Children inherit the parent's approval mode, with approvals inside the child.** That is the Codex and Antigravity model; Cline and Oh My Pi, where approving the spawn approves everything, are the ones not to copy.
- **A table-driven test guards each agent's tool list.**

What it gets wrong, bluntly:

- **Eight is at the high end** of a field that converges on three to six.
- **Smol is a model slot dressed as an agent.** No tools, no memory: OpenCode ships exactly that shape and hides it. If its job is titles and other cheap side tasks, make it a slot. If it is a cheap chat agent for users, say so in its description.
- **Senior Teamlead duplicates Software Developer.** Software Developer already delegates to Scout, Researcher and Reviewer; the Teamlead is the old `ultracode` orchestrator under a new name.
- **Researcher and Software Developer are broken on agentic nodes.** The web tools they rely on are not registered there (table stakes item 4). Until that is fixed, Researcher is chat-only and a Researcher delegated by Software Developer has no web tools.
- **No Plan agent,** and no plan mode for it to pair with. Claude Code and OpenCode pair the two.
- **"Read-only" must mean no unconfined shell.** The inventory says Scout and Reviewer are read-only; if either can run `bash` on `main`, it is not read-only in any sense the Cline evidence would accept. Make the guarantee "read-only tools, or `bash` inside a read-only sandbox".
- **Only Scout is documented as sub-agent only.** Decide for each of the eight; the field's built-ins are mostly sub-agent only.
- **No other harness ships a built-in Writer.** Not wrong, but it should earn its place.

#### Model slots and memory scopes

**What the field does with slots** (the slot tables are under "Auxiliary model slots" in `round1/synth/04-planning-subagents-background.md`):

| Design | Who | What it shows |
|---|---|---|
| One small or fast model for every background task | Claude Code: titles, `/goal` evaluator, agent-view summaries, set by one variable that also changes the `haiku` alias (https://code.claude.com/docs/en/model-config.md) | The documented coupling trap. |
| A slot per task, each with its own fallback chain, defaulting to the main model | Hermes: compression, vision, titles, goal judge, curator, approval (https://hermes-agent.nousresearch.com/docs/user-guide/features/fallback-providers, unreviewed) | The cleanest design: a summariser outage does not take titles down. |
| Per-feature model keys | Zed: `thread_summary_model`, `compaction_model`, `commit_message_model`, `subagent_model` (https://zed.dev/docs/ai/agent-settings.md) | Simple, no fallback documented. |
| A role table with aliases and auto-resolution | Oh My Pi: ten roles, `@role` aliases, per-role fallback chains; unassigned roles resolve from whatever is authenticated (https://omp.sh/docs/roles) | Complete, but "which model ran this" becomes non-obvious. |
| Hidden agents as slots | OpenCode `title`, `summary`, `compaction` (https://opencode.ai/docs/agents/) | Same shape as our Smol. |
| Sidecar for a missing capability | OpenCodex vision sidecar, capped per turn and cached (https://opencodex.me/guides/sidecars/) | The vision bridge, as a pattern. |

What that implies for the owner's three slots (title, vision bridge, summariser), each pointing at an agent's model chain:

- **One slot per task, not one shared default.** A shared "small" model is fine as the default value of several slots; it must not be one key that moves them all. The evidence for this is Claude Code's trap and Hermes' separate chains.
- **Default each slot to the main or agent model** (Hermes, Zed), not to whatever happens to be authenticated (Oh My Pi). Under "the node decides", which models a node may call must be explicit, and the slot must resolve to a model the node holds a key for.
- **Place slots where the work runs.** Titles for chat can run on the master's chat runtime. The summariser for agentic compaction and the vision bridge for an agentic turn must run on the node, because the loop runs there. Pointing a slot at an agent's chain reuses our existing per-agent fallback machinery.
- **The summariser's window must be at least the main model's.** Hermes requires this and documents the silent failure when it is not; Claude Code refuses to fall back to a smaller-context model during compaction. Validate it when the slot is configured.
- **Treat slot output as untrusted model text,** especially the vision bridge's descriptions.
- **First settle which model runs history summarisation today.** The inventory does not say.

**What the field does with memory scopes.** Nobody documents all four of our planned scopes (workspace, project, user facts, per-repository). Claude Code's auto memory is per git repository, machine-local and shared across worktrees (https://code.claude.com/docs/en/memory.md); Claude chat has global plus per-project memory, where project chats see project and non-project memory but never another project's, a one-way containment rule (https://support.claude.com/en/articles/11817273-use-claude-s-chat-search-and-memory-to-build-on-previous-context); Warp attaches personal, agent and team stores read-only or read-write (https://docs.warp.dev/agents/agent-memory/, preview); Hermes profile distributions carry config and skills but never memory or credentials. Hard size caps force curation: Hermes 2,200 and 1,375 characters, Claude Code 200 lines or 25 KB, ours 500 characters per fact and the top 6 recalled.

Implications for our design:

- **A per-repository scope is only well defined on the node,** which can key it by git identity. A master-side store cannot, unless the node reports an identity. Today `remember` is not registered on agentic nodes, so nothing could write that scope.
- **Per-repository memory crosses users.** On a node shared by several master users, memory written from one user's chat would be read in another's. "Chats are private to their owner" means a per-repository fact derived from a private chat needs the same sharing step as a workspace fact. This is an inference from our invariants, not a field finding.
- **Copy Claude chat's one-way containment** between project scopes and **Warp's read-only or read-write attachment** per scope.

**Does agent-written memory need a human approval step?** The field mostly says no: Claude Code, Codex, OpenHands, Claude chat and OpenClaw write without one. The failure evidence comes from those designs: Cursor warns that untrusted input can poison automation memory and that its SDK journal is shared across all sessions and users (https://cursor.com/docs/agent-sdk/reference/memory, unreviewed); OpenClaw admits promoted memories have no time-based retention and deletion has gaps (https://docs.openclaw.ai/concepts/dreaming); OpenHands and Hermes treat injected memory as untrusted text. Nothing in the surveys measures whether users abandon an approval step in practice. The inference that follows: an approval step is clearly justified for the shared scopes (project, workspace, per-repository), where one poisoned fact reaches other people; for a user's own private facts it could be optional, as Hermes makes it. Our current design (owner approval for everything, manager approval for project facts on gate3, sharing off by default outside the Personal workspace on `feat/agent-roster`) is stricter than the field and on the right side of that line. Keep the gate on shared scopes; relaxing it for private facts is a product choice, not a safety one.
