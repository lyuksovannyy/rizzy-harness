## 3. Harness overview

Identities, licences and versions are from `round1/00-targets.md`, corrected where a survey or review found it wrong (noted in the table). "Entries" is the mechanical heading count from `stats.json` (it includes area headings in some files); "Checked" is the number of entries or claim groups a reviewer re-opened, from each survey's review section.

| Harness | Vendor or maintainer | What it is | Surfaces | Open or closed | Licence | Version surveyed (date) | Survey file | Entries | Checked |
|---|---|---|---|---|---|---|---|---|---|
| Codex | OpenAI | coding agent family | CLI (`codex exec`), IDE extension, ChatGPT desktop app (Codex mode), Codex Cloud, mobile, SDK, App Server, GitHub Action | mixed: CLI, SDK, App Server, Codex Security CLI/SDK open; IDE extension, cloud and desktop closed | Apache-2.0 (open parts) | `rust-v0.167.0` (2026-11-06) | `codex.md` | 161 | 47 |
| OpenCodex | `lidge-jun` (identity assumed, owner to confirm) | local provider gateway and proxy; **not a harness** | CLI `ocx`, local proxy, web dashboard, remote hub, experimental Remote Workspace | open | MIT | `v2.100.1` (2026-11-10) | `opencodex.md` | 36 | 21 |
| OpenCode | `anomalyco` organisation (formerly `sst`) | open coding agent with a background server and thin clients | TUI/CLI, desktop (beta), web mode, mobile, HTTP API, SDK, ACP | open | MIT | `v2.2.7` (2026-11-10); V1 line `v1.19.5` (2026-10-28) | `opencode.md` | 110 | 31 |
| Claude Code | Anthropic | agentic coding tool, one engine across surfaces | CLI, Desktop Code tab, VS Code, JetBrains, web cloud sessions, mobile, Remote Control, self-hosted runners (beta), Agent SDK | closed: public repo is "All rights reserved"; Agent SDK Python is MIT, TypeScript "All rights reserved" | proprietary | `v2.1.322` (2026-11-10) | `claude-code.md` | 806 | 161 items in three reviews |
| Claude Desktop | Anthropic | desktop app holding Chat, Cowork and the Code tab | macOS, Windows, Linux (beta); Dispatch from a phone | closed | proprietary | `v4.23118.1` (2026-11-09; the scout had it as unverified) | `claude-desktop.md` | 94 | 30 |
| DeepSeek harness | DeepSeek AI | "everything is a plugin" harness, developer preview | web UI, desktop (macOS, Windows), headless, ACP, SDK over stdio, Python SDK | open | MIT (first npm version listed BSD-3-Clause; not traced) | prerelease `dsh-v0.2.2-rc.1` (2026-11-04); no stable release (the scout's "no GitHub releases" was wrong) | `deepseek-harness.md` | 100 | 30 |
| Oh My Pi | can1357 / Stencil Labs | hard fork of Pi with a Rust core | TUI/CLI, RPC, SDK, ACP, collab | open | MIT | `v18.18.6` (2026-11-09) | `oh-my-pi.md` | 112 | about 30 |
| Pi | Earendil Inc. (created by Mario Zechner) | minimal, extensible harness with documented non-features | TUI, print and JSON modes, RPC, TypeScript SDK | open | MIT | `v1.5.2` (2026-11-10) | `pi.md` | 104 | 30 |
| Cursor | Cursor (acquired by SpaceX, 2026-08-14) | AI editor plus cloud and self-hosted agents | IDE, Agents Window, CLI `agent`, Cloud Agents, Self-Hosted Machines, iOS, Bugbot, SDKs, ACP | closed | proprietary | changelog 2026-11-06; CLI `v2026.10.22` | `cursor.md` | 95 | 28 |
| Antigravity | Google | agentic coding platform on one harness | App, CLI `agy`, IDE, extensions, Python SDK, Remote Agents, Remote Control | closed core; Python SDK open | proprietary; SDK Apache-2.0 | App `v2.31.0` (2026-11-09); CLI `1.5.5` (2026-11-10) | `antigravity.md` | 93 | 27 |
| OpenHands | OpenHands (formerly All-Hands-AI) | self-hosted web control centre (Agent Canvas) over pluggable backends | Canvas web UI, Agent Server, SDK, CLI ("no longer actively maintained") | open core: Canvas, SDK, Agent Server, CLI open; Cloud and Enterprise commercial | MIT per READMEs | `v1.38.0` (2026-11-10) | `openhands.md` | 89 | 23 |
| OpenClaw | OpenClaw (a 501(c)(3) foundation employs the core team) | personal agent: one Gateway with paired nodes | Gateway daemon, CLI, Control UI, macOS app, nodes, chat channels | open | MIT | `v2026.11.2` (2026-11-08) | `openclaw.md` | 112 | 28 |
| goose | Agentic AI Foundation / Linux Foundation (formerly Block) | Rust harness where everything is ACP | desktop, CLI, `goose serve`, ACP, GDK Rust crates | open | Apache-2.0 | `v1.59.0` (2026-11-10; the scout listed `v1.58.1`) | `goose.md` | 99 | 29 |
| Hermes Agent | Nous Research | general agent with memory, agent-written skills and a messaging gateway | CLI/TUI, desktop, gateway on about 28 platforms, API server, Docker and remote terminal backends | open | MIT | `v0.23.1` (2026-11-10) | `hermes-agent.md` | 126 | 27 |
| Zed/ACP | Zed Industries | editor agent, plus the Agent Client Protocol | Zed agent panel, external agents, remote development; ACP spec, registry and SDKs | open | GPL and Apache licence files at the root (copyleft: ideas only) | Zed `v1.27.0` (2026-11-04) | `zed-acp.md` | 47 | 31 |
| Warp | Warp (Denver Technologies) | terminal with an agent and a cloud agent platform | terminal, Warp Agent, `oz` CLI, cloud agents, Factories, self-hosted workers | client open since 2026-04-28; control plane and cloud services closed | AGPL-3.0 with MIT UI crates (ideas only) | changelog 2026.10.22 (`v0.2026.10.21.08.25`) | `warp.md` | 114 | 28 |
| Cline | Cline | IDE agent that grew a CLI, desktop, Kanban and SDK | VS Code and JetBrains extension, CLI/TUI, desktop with SSH remote, cloud agents, Kanban, SDK, ACP | open | Apache-2.0 | `v4.1.26` (2026-10-26) | `cline.md` | 95 | 25 |

### What each one is best understood as

**Codex.** A family, not a product. The open Rust CLI and its App Server protocol are the part worth reading; one JSON-RPC protocol carries the TUI, IDE, desktop and SDK, but OpenAI labels that App Server experimental and "not supported for production" (https://learn.chatgpt.com/docs/app-server.md). Its docs are unusually candid about the limits of its own sandbox (https://learn.chatgpt.com/docs/permissions.md).

**OpenCodex.** A provider gateway that lets Codex, Claude Code and other clients run on any model provider. It runs no agent loop of its own (a reading of its how-it-works and architecture pages, corrected in review: not a quoted claim). It matters for its routing, sidecars and remote hub, not as a harness. The owner must confirm that `lidge-jun/opencodex` is the project meant; `RyensX/OpenCodex` is the other candidate.

**OpenCode.** One background server per machine that owns sessions and permissions, with TUI, web and desktop as clients (https://opencode.ai/docs/troubleshooting/). Its durable sequence-numbered session log, separate from a volatile broadcast stream, is the closest public analogue to our fold-over-log UI (https://opencode.ai/docs/api/). It discontinued session sharing in v1.19.1 (https://opencode.ai/changelog/v1.19.1, corrected in review).

**Claude Code.** The most documented harness in the set and the one that ships most mechanisms first: auto-mode classifier, hook `defer`, credential masking, self-hosted runners, workflows. It is closed, its docs lag its changelog, and several headline features (agent view, background sessions) are research preview, not GA (corrected in review). Its self-hosted runner inverts "the node decides": the control plane sends each session's tool list and permission rules (https://code.claude.com/docs/en/self-hosted-environments-configuration.md, corrected in review).

**Claude Desktop.** Claude Code with a desktop shell, plus Cowork for non-coding work. What it adds is UX: diff comments, a browser pane, scheduled tasks, SSH sessions, Dispatch from a phone (https://code.claude.com/docs/en/desktop). Cowork runs either in a local VM or in the cloud; the getting-started article that says cloud only is stale (corrected in review).

**DeepSeek harness.** An open, preview-stage harness whose design notes are the best match to our event-log discipline: approval decisions and mode changes are log events, a failed audit append rejects the request (https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval). Its own `SAFETY.md` says it has had no security audit (https://github.com/deepseek-ai/deepseek-harness/blob/master/SAFETY.md).

**Oh My Pi.** Pi's maximalist fork: ten model roles, hash-anchored edits, compaction method chains, internal URL schemes, a Rust core. Its default approval mode is `yolo` (https://omp.sh/docs/approvals). Its README and docs disagree with each other often, so treat README claims as unverified.

**Pi.** The minimalist reference: no built-in permissions, no plan mode, no sub-agents, each a documented non-feature with a substitute (https://pi.dev/). It reversed its position on MCP and now ships it behind code mode and deferral (https://pi.dev/docs/latest/mcp).

**Cursor.** An editor company whose self-hosted workers are the clearest inversion of our design: the loop and inference stay in Cursor's cloud and the customer's machine only executes tools over one outbound connection (https://cursor.com/docs/cloud-agent/self-hosted). Closed; docs read through a summarising fetcher.

**Antigravity.** Google's closed platform, with a strong command-permission parser and plan and artifact review UX (https://antigravity.google/docs/permissions, https://antigravity.google/docs/plan). Its own pages call its browser "sandboxed" while describing a browser that drives the user's real Chrome profile (https://antigravity.google/docs/commands/browser).

**OpenHands.** The closest product shape to ours: a browser client over independent agent backends, each with its own secrets, plus per-conversation containers. Its open tier has no authentication by default and its FAQ calls it a single-user local tool (https://docs.openhands.dev/overview/faqs.md, corrected in review).

**OpenClaw.** The closest topology to ours: one Gateway, nodes that dial in and declare commands, two-layer pairing, exec approvals bound to argv, cwd, env and executable (https://docs.openclaw.ai/tools/exec-approvals). Authority sits on the Gateway, not the node. Its trust-model page is the most candid in the set (https://docs.openclaw.ai/gateway/security/trust-model).

**goose.** A Rust harness that made ACP its only protocol, including the only shipped ACP-over-HTTP/WebSocket transport (https://goose-docs.ai/docs/gdk/acp/index.md). Its survey's "nothing documented" claims are the least reliable in the set: the reviewer found the sandbox one wrong in part and asked for a second look at all of them.

**Hermes Agent.** A general agent with the most developed memory and skill management (write-approval staging, a curator with a hash ledger and rollback) and a credential-injecting egress proxy (https://hermes-agent.nousresearch.com/docs/user-guide/features/memory, https://hermes-agent.nousresearch.com/docs/user-guide/egress/iron-proxy). Its docs describe a very wide surface; some features may be thin.

**Zed/ACP.** Two things: Zed's own agent (per-tool regex permissions, a scoped sandbox, a removed checkpoint feature) and ACP, the editor-to-agent protocol that puts file and terminal execution on the client and leaves policy storage to the agent (https://agentclientprotocol.com/protocol/v1/security.md). Copyleft: ideas only.

**Warp.** A terminal with an open client and a closed cloud platform that pushes resolved config to self-hosted workers (https://docs.warp.dev/platform/architecture/). Its code review panel and handoff snapshot are worth studying; its "open" covers the client only. AGPL: ideas only.

**Cline.** The open IDE agent with the reference approval and checkpoint UX, now with a CLI, desktop SSH remote and a Kanban. Its documented defaults contradict its own overview page (auto-approve on in the CLI, schedules default to yolo); see Cline in `round1/synth/_parts/overhyped.md`.
