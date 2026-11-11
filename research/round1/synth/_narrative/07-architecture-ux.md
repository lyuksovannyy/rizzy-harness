## 7. Notable architectural and UX approaches

Everything here is what public sources document. Where a mechanism is inferred rather than stated, the row says so. Terms: the **control side** is whatever holds the UI, accounts and queue (our master); the **execution side** is the machine where commands and file edits happen (our node); the **loop** is the model-call and tool-call cycle.

### Where the loop runs, and where the tools run

The full comparison, with credentials, policy, transport, pairing, data flow, restart behaviour and evidence for 31 documented designs, is the control-side versus execution-side table at the start of section 2 of `round1/synth/09-remote-distributed-architecture.md`. The summary:

| Shape | Designs | Who decides what may run |
|---|---|---|
| **Matches ours**: loop and tools on the machine that owns the code; the other side is a UI or relay | Codex SSH remote and remote TUI, Codex remote control, OpenCode server, Claude Code Remote Control, Claude Code self-hosted runner, Claude Desktop SSH and Dispatch, Antigravity Remote Control, OpenHands Canvas plus Agent Server, OpenClaw cloud workers in `worker-turn` mode, goose `serve` and Roaming, Warp self-hosted worker (loop placement inferred), Cline SSH remote, Zed remote development (where model calls originate is contradictory on its page) | Mostly the control side. See below. |
| **Inverted**: loop on the control side, tools on the worker | Cursor self-hosted workers, OpenCodex Remote Workspace, OpenClaw Gateway plus nodes, DeepSeek harness SSH backend, Pi pi-env, Hermes Agent terminal backends. ACP is a partial inversion (the agent loops; the client executes `fs/*` and `terminal/*`). | The control side, by construction. |
| **Neither**: loop and tools both at the vendor | Codex Cloud, Claude Code cloud sessions, Cursor cloud agents, Antigravity Remote Agents, Cline cloud agents, Warp hosted runs, OpenHands Cloud | The vendor. |

The rows that matter most to us:

- **Claude Code self-hosted runner** is the closest in shape and the furthest in authority. The runner dials out only, polls for work under a lease (about 60 seconds), and runs each session as a child process. But the control plane sends each session's tool list and permission rules with the work payload, plus model, connectors and hooks; the runner can only add a floor (image `managed-settings.json`, `--confine-repo-settings enforce`) (https://code.claude.com/docs/en/self-hosted-environments-configuration.md, corrected in review). Prompts, responses and tool results go to Anthropic, which stores the transcript.
- **Cursor self-hosted workers** invert our design: loop, planning and inference stay in Cursor's cloud; the worker executes edits, terminal commands and local MCP over one outbound HTTPS connection, and sends back file contents, terminal output, diffs and screenshots. The OS user is the boundary (https://cursor.com/docs/cloud-agent/self-hosted).
- **OpenClaw Gateway plus nodes** is the closest topology. Nodes dial the Gateway, sign a challenge with a device key, and declare commands; a command runs only if the node declared it, the operator approved it, and the Gateway's per-platform allowlist includes it. The node can withhold a command by not declaring it, but the gate is on the Gateway (https://docs.openclaw.ai/nodes/command-policy).
- **Warp self-hosted worker**: the control plane pushes the task, resolved config and scoped runtime credentials; the worker dials out (https://docs.warp.dev/platform/architecture/).
- **Ours**: the node runs loop and tools, holds the provider key, and its `node.toml` is the only authority; approval mode is `min(requested, node max)`; nothing over HTTP or the link writes it. No other row documents an execution-side file that outranks what the control side sends.

Outbound-only connections from the execution side are the norm (Claude Code runner and Remote Control, Cursor, Warp, OpenCodex executor, OpenClaw nodes, Antigravity, Hermes Relay), and ours matches. The designs where a client dials an agent server (Codex remote TUI, OpenCode, goose, OpenHands, DeepSeek harness) need the execution host to listen, and that is where the weakest authentication defaults sit: OpenHands has no authentication by default (https://docs.openhands.dev/overview/faqs.md), and Codex's non-loopback App Server listeners are unauthenticated by default "during rollout" (https://learn.chatgpt.com/docs/app-server.md).

### One protocol for every client

Several harnesses put every client on one protocol so that the UI is just another client:

- **Codex App Server**: JSON-RPC over stdio, WebSocket or a Unix socket, one `initialize` with capability negotiation and an `experimentalApi` gate, server-initiated approval requests, version-matched generated schemas, bounded queues that answer `-32001`. It carries the TUI, IDE, desktop and SDKs, and is labelled experimental (https://learn.chatgpt.com/docs/app-server.md).
- **OpenCode**: one background server per machine with an OpenAPI 3.1 HTTP API; TUI, web and desktop attach to it (https://opencode.ai/docs/api/).
- **goose**: everything is ACP, over stdio and over HTTP plus WebSocket (https://goose-docs.ai/docs/gdk/acp/index.md).
- **Warp**: all clients use the same control-plane APIs, with a published OpenAPI spec and SDKs (https://docs.warp.dev/platform/architecture/).
- **Cline hub**: clients register capabilities with a per-machine hub, which routes a diff to the IDE and a shell request to the CLI; no authentication is documented (https://docs.cline.bot/sdk/architecture/hub-spoke.md, corrected in review).

Ours already works this way (master REST and `/api/ws` for the UI, a versioned master-node protocol for nodes), but it has no published spec and the API token is admin-equivalent. See "Programmatic control protocol" in `round1/synth/09-remote-distributed-architecture.md`.

### Client protocols: ACP and who speaks it in which role

The Agent Client Protocol (ACP) is a JSON-RPC protocol between an editor (client) and an agent. Its design choices matter more than its adoption:

- The agent runs the loop, but `fs/*` and `terminal/*` calls are executed **by the client**, which must apply its own policy (https://agentclientprotocol.com/protocol/v1/terminals.md).
- Permission requests are advisory; the agent stores "always" decisions, and the protocol standardises only the prompt's option kinds (https://agentclientprotocol.com/protocol/v1/security.md).
- The standard transport is stdio. A remote transport (Streamable HTTP or WebSocket on one endpoint) is still an Active RFD (https://agentclientprotocol.com/rfds/streamable-http-websocket-transport.md).

| Role | Who, as documented |
|---|---|
| Agent (served to editors) | OpenCode, DeepSeek harness, Oh My Pi, Cursor, Antigravity, OpenHands CLI, goose, OpenClaw, Hermes Agent, Cline. Codex, Claude Code and Pi are adapter-only. Warp's support appears only in changelog wording (unverified). |
| Client (hosting other agents) | Zed (the reference client), OpenHands Agent Server (runs Claude Code, Codex, Gemini, Pi, OpenCode as backends, credentials resolved by secret name), goose ACP providers, OpenClaw `/acp spawn` |
| Remote transport shipped | goose only, ahead of the RFD |

Nobody in the client role documents how a hosted agent's approval requests reach a human (see "Other harnesses as sub-agents" in `round1/synth/04-planning-subagents-background.md`). That is the gap any ACP integration of ours would have to close, because ACP's advisory permission model and agent-side "always" store sit badly with "the node decides".

### Event logs versus transcripts

| Model | Who | What is documented |
|---|---|---|
| Event log as source of truth with a stated writer rule | OpenClaw, DeepSeek harness, ours | OpenClaw records an active-writer claim and verifies it in the commit transaction (https://docs.openclaw.ai/concepts/agent-loop, corrected). DeepSeek derives model context from an append-only typed log and requires anything the model sees to be logged (https://deepseek-harness.github.io/deepseek-harness/en/reference/agent-lifecycle). |
| Durable log separate from a volatile stream | OpenCode | A sequence-numbered per-session log readable with `follow=true`, beside a broadcast stream that drops slow consumers (https://opencode.ai/docs/api/). |
| Event-sourced, no writer rule stated | OpenHands | An append-only `EventLog` with a FIFO lock (https://docs.openhands.dev/sdk/arch/conversation.md). |
| Transcript file | Claude Code, Pi, Oh My Pi, Codex | Claude Code keeps an append-only JSONL transcript (https://code.claude.com/docs/en/sessions.md); its single-writer protection is advisory and two terminals can interleave (see "Session storage" in `round1/synth/05-sessions-git.md`). Pi and Oh My Pi keep a branching JSONL tree (https://pi.dev/docs/latest/session-format). |

Ours is in the first group and is the only one that combines a single writer on the execution side with a mirror to a self-hosted control side. What we lack is lifecycle: retention, compaction of streaming deltas, and crash-torn write handling (see "Where we are ahead" in section 8).

### Keeping credentials away from the execution side

Four patterns are documented (see "Credential placeholders and injection at an egress proxy" in `round1/synth/02-permissions-sandbox-security.md` and "Central credential broker or inference proxy" in `round1/synth/09-remote-distributed-architecture.md`):

1. **The key never enters the sandbox; a vendor proxy attaches it after the request leaves.** Codex Cloud network secrets (https://learn.chatgpt.com/docs/environments/cloud-environments.md), Claude Code cloud sessions (https://code.claude.com/docs/en/cloud-environments.md), Cursor runtime secrets (vendor statement, https://cursor.com/docs/cloud-agent/setup).
2. **A placeholder inside, substituted by a local proxy outside the sandbox.** Hermes iron-proxy (https://hermes-agent.nousresearch.com/docs/user-guide/egress/iron-proxy, corrected), Pi's documented Docker Sandboxes pattern (https://pi.dev/docs/latest/containerization, unreviewed), Claude Code's sandbox `mask` mode, which needs experimental TLS termination, covers sandboxed Bash only, and is a different mechanism from its cloud secrets (https://code.claude.com/docs/en/sandboxing.md, corrected in review).
3. **Inference proxied by the control side, so workers never hold provider keys.** OpenClaw cloud workers (beta, https://docs.openclaw.ai/gateway/cloud-workers), Oh My Pi `auth-gateway` (https://omp.sh/docs/secrets), OpenCodex hub (https://opencodex.me/guides/remote-hub/), Warp Agent traffic (https://docs.warp.dev/platform/architecture/). Oh My Pi's `auth-broker` is weaker than its name suggests: it hands clients API keys as well as access tokens, and is loopback-only plain HTTP without TLS (corrected in review). Oh My Pi states that the proxy operator can read prompts and responses.
4. **The key sent per session, never written to the worker.** Cline SSH remote (https://docs.cline.bot/usage/cline-desktop-ssh.md).

Ours does none of these: each node holds its provider key in `config.toml` and calls the provider directly; tool children have provider key variables stripped from their environment.

### How policy is layered, and who can override whom

| Pattern | Who | Note |
|---|---|---|
| Admin file outranks everything on one machine | Codex `requirements.toml` and MDM, with an optional self-hosted `[config_server]`; omitted permission profiles are denied, including future built-ins (https://learn.chatgpt.com/docs/enterprise/managed-configuration.md) | The strongest admin design; single host, not a split. |
| Vendor console pushes managed settings | Claude Code (hourly; MDM every 30 minutes), with strict-value-wins keys a lower scope may tighten but not loosen (https://code.claude.com/docs/en/settings.md). Its own docs say server-managed settings are a client-side control, not a security boundary (https://code.claude.com/docs/en/server-managed-settings.md). | |
| A policy layer that "only tightens" | OpenCode `policies`: but a policy `allow` can lift an earlier policy deny, so it is not deny-only (https://opencode.ai/docs/policies/, corrected in review) | The caveat is the lesson. |
| Merge by decision kind | Cursor hooks: deny beats ask beats allow for permission fields regardless of source; for other fields the last response wins (https://cursor.com/docs/hooks, corrected: not "most restrictive wins") | |
| Policy that does not reach every surface | Cline remote config covers the IDE extensions, gives the CLI only rules and workflows, and Desktop nothing (https://docs.cline.bot/enterprise-solutions/configuration/remote-configuration/overview.md) | |
| Filesystem permissions only | Hermes `/etc/hermes` overrides user files per key; no signing, no MDM (https://hermes-agent.nousresearch.com/docs/user-guide/managed-scope) | |
| Repo config may not widen | Codex ignores provider, base URL, `notify`, `otel` and profile keys in project config with a warning (https://learn.chatgpt.com/docs/config-file/config-advanced.md); Claude Code lists keys project files cannot set (https://code.claude.com/docs/en/settings.md) | Oh My Pi loads everything from the repo with no trust prompt. |
| **Execution side outranks control side** | **Ours only**: `node.toml` on the node | The inverse direction from everyone above. |

### UX patterns

**Reviewing plans.** Antigravity drafts an Implementation Plan artifact; comments attach to the next message as a chip, and `/plan` always stops for approval (https://antigravity.google/docs/plan). Oh My Pi offers "execute in a fresh session with only the plan" (https://omp.sh/docs/plan). Warp keeps an editable plan with version history and share links (https://docs.warp.dev/agents/capabilities/planning/). How plan mode is enforced varies more than how it looks: runtime tool rules in OpenCode (https://opencode.ai/docs/agents/) and the Antigravity CLI, prompt guidance only in DeepSeek harness (https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/plan), and in Claude Code not enforced at all in interactive terminals where bypass is available (https://code.claude.com/docs/en/permission-modes.md).

**Reviewing diffs.** The field's standard is a whole-change review pane with per-hunk revert and inline comments fed to the agent's next turn: Codex desktop (https://learn.chatgpt.com/docs/environments/local-environment.md, unreviewed), Claude Desktop (https://code.claude.com/docs/en/desktop), Antigravity with Uncommitted, Branch and Agent Edits views (https://antigravity.google/docs/review-changes), Warp, including comments sent to third-party CLI agents (https://docs.warp.dev/code/code-review/), Zed per hunk (https://zed.dev/docs/ai/agent-panel). Ours shows the old and new strings of one `edit` call.

**Approvals.** Answer options range from once only (DeepSeek harness) to once, session, pattern and always (OpenCode, Zed, Codex). Approvals can be answered from a phone (Codex, Claude Code Remote Control, Antigravity, OpenCode) or inside a chat channel with approver lists (OpenClaw `/approve <id>`, https://docs.openclaw.ai/tools/exec-approvals). DeepSeek deliberately omits tool arguments from approval requests, which its own reviewer flags as approving blind. Claude Code excludes the permission prompt from what mods can render (https://code.claude.com/docs/en/plugins/mods/interface.md).

**Running several agents.** Claude Code's agent view groups background sessions by state ("Needs input", "Ready for review") with a peek-and-reply panel, backed by a per-user supervisor daemon; research preview (https://code.claude.com/docs/en/agent-view.md, corrected). Cline and Hermes run a Kanban where each card is an agent in its own worktree (https://docs.cline.bot/usage/kanban.md, https://hermes-agent.nousresearch.com/docs/user-guide/features/kanban). Antigravity, Cursor and Warp have command-centre windows.

**Notifications.** Claude Code pushes to the phone when a task ends or a decision is needed, suppressed while the terminal is focused (https://code.claude.com/docs/en/remote-control.md); Cursor uses iOS Live Activities for up to eight agents (https://cursor.com/docs/cloud-agent/mobile); Warp uses toasts, a mailbox and tab badges (https://docs.warp.dev/agents/capabilities/agent-notifications/). Ours has browser notifications in a secure context and an attention list, with no PWA or service worker, so nothing reaches a closed tab.

**Hand-off between devices and machines.** Codex Handoff moves a chat and its Git state between the local computer and a connected remote host (https://learn.chatgpt.com/docs/remote-connections.md, corrected: "to a worktree" is not stated). Warp's handoff snapshots tracked and untracked uncommitted changes for a local-to-cloud move (https://docs.warp.dev/platform/handoff/, corrected). Claude Code Remote Control has explicit takeover rules ("another connection took over") and a resume window of about four hours (https://code.claude.com/docs/en/remote-control.md). Claude Desktop's SSH sessions are not documented to survive the app quitting: the remote login is renewed only while Desktop is connected, and the docs advise against counting on overnight runs (https://code.claude.com/docs/en/desktop, corrected in review).

### The central tension: authority on the node means credentials on the node

Our design puts authority on the execution side. A node must therefore be able to act on its own policy without asking the master, and that includes calling the model: the node holds the provider key. The field has gone the other way. Where it splits control and execution, it centralises policy and credentials together:

- Claude Code's runner receives its tool list and permission rules from the control plane; Warp's worker receives resolved config and scoped credentials; OpenClaw's Gateway holds the command allowlist and, for cloud workers, the provider keys. Cline's SSH remote keeps approvals and provider settings on the control side and sends the key per session.
- In those designs a compromised worker leaks little: no long-lived model key, a short-lived or per-dispatch credential (Claude Code work orders, Cursor per-claim tokens, OpenClaw hashed per-dispatch credentials). In ours, a prompt-injected `bash` on `main` can read the node's `config.toml`, and one long-lived node key does everything after enrolment.
- But the centralised designs move the trust problem rather than solve it, and their vendors say so. Claude Code calls its server-managed settings "not a security boundary" (https://code.claude.com/docs/en/server-managed-settings.md). OpenClaw says one Gateway is one trust boundary and that operator scopes are not tenant isolation (https://docs.openclaw.ai/gateway/security/trust-model). A compromised or hostile control side can widen what every worker runs. In ours it cannot: that is what "the node decides" buys. `crates/master/tests/hostile_node.rs` covers the hostile-node direction; the hostile-master direction still has no test (see "Ours against the field" in `round1/synth/09-remote-distributed-architecture.md`).

What the surveys show for each side:

| Question | Evidence for node-held credentials (ours) | Evidence for control-side credentials (the field) |
|---|---|---|
| Who can widen policy | Only the node's own file; no surveyed design matches this. | The control side, in every documented split. |
| Blast radius of a compromised execution machine | Large on `main`: the key is readable by unconfined `bash`. Gate3's Landlock sandbox closes the read path for the shell (unmerged). | Small: workers hold short-lived or no credentials. |
| Blast radius of a compromised control side | Small: a hostile master cannot widen `node.toml`. | Large, and stated by the vendors themselves. |
| What the control side sees | Every event is mirrored to the master already, so an inference relay would add little exposure. | The control side sees all traffic (Oh My Pi says so for its gateway). |
| Availability | The node calls the provider directly. | Inference depends on the control side being up. |

There is a middle pattern that keeps the key on the node but out of the agent's reach: a node-local credential proxy outside the sandbox, as Hermes iron-proxy does for its container backends and as Pi documents for Docker Sandboxes. It is compatible with "the node decides". It is also heavier than it looks: Hermes lists a stolen CA key, raw sockets and cloud SigV4 as limits, and Claude Code's local equivalent needs experimental TLS termination and leaves the real file readable when its pattern does not match.

The question is not settled by this evidence. No surveyed harness combines execution-side authority with control-side credentials, so there is no field data on how that combination fails. The decision belongs to the owner, before Gate 3 work on the link, and should be made after the gate3 sandbox is merged and its residual gaps (pathname Unix sockets before Linux 7.1, UDP and DNS) are measured on real nodes.
