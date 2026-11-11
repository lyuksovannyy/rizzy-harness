# 09. Remote, cloud, headless, CI and distributed execution (cross-harness synthesis)

Round 1 synthesis, 2026-11-10. Synthesis only: every fact below comes from the survey files in `research/round1/`; nothing was browsed. Short names follow `SYNTH-BRIEF.md`. OpenHands evidence uses the survey's own shorthand `D:` = `https://docs.openhands.dev`; Hermes Agent evidence paths are relative to `https://hermes-agent.nousresearch.com/docs/` (both as stated in those surveys).

## 1. Scope

**Areas covered.** Area 17 (remote, cloud, headless, CI and distributed execution) from every survey, plus section 4 ("Architecture and UX notes") and section 5 ("Relevance to a self-hosted master/node harness") from every survey, and whatever in sections 3, 6, 7 and the review sections belongs to these topics. Where an area-17 topic was filed elsewhere in a survey (for example client/server splits in area 1, protocols in area 13, cloud agents in area 10, remote control in area 15), that entry was read too.

**Files and line ranges read.**

| File | Lines read |
|---|---|
| `codex.md` | 479-494, 604-634, 748-757, 775-853, 986-1082, 1159-1196 |
| `opencodex.md` | 1-30, 139-156, 195-328 |
| `opencode.md` | 30-35, 256-262, 323-397, 458-512, 562-610 |
| `claude-code.md` | 160-186, 868-876, 1463-1470, 1797-1815, 1932-1956, 2113-2122, 2338-2343, 2828-2834, 3549-3842, 4275-4286, 4851-4910, 5202-5340 |
| `claude-code.review-1/2/3.md` | all (grep plus the Area 16-23 and sections 3-7 tables and corrections of review 3 in full) |
| `claude-desktop.md` | 25-31, 196-208, 284-290, 316-322, 357-381, 469-528, 560-607 |
| `deepseek-harness.md` | 313-318, 357-365, 417-442, 533-595, 616-677 |
| `oh-my-pi.md` | 441-464, 549-610, 620-663 |
| `pi.md` | 247-253, 349-354, 391-400, 455-474, 561-618, 662-697 |
| `cursor.md` | 23-28, 201-212, 229-236, 294-310, 359-399, 456-523, 595-642 |
| `antigravity.md` | 268-275, 298-302, 392-416, 494-559, 584-618 |
| `openhands.md` | 1-32, 69-74, 337-392, 459-516, 542-592 |
| `openclaw.md` | 140-145, 468-499, 596-660, 749-788 |
| `goose.md` | 252-262, 360-366, 420-433, 518-578, 646-677 |
| `hermes-agent.md` | 3, 111-131, 169-175, 405-410, 467-478, 569-606, 679-738, 783-820 |
| `zed-acp.md` | 1-12, 173-180, 214-245, 276-310, 346-407, 447-484 |
| `warp.md` | 10, 19-30, 238-258, 282-288, 318-322, 356-362, 391-424, 546-606, 633-670 |
| `cline.md` | 24-29, 232-237, 264-269, 356-386, 476-534, 593-629 |
| `own-rizzy-harness.md` | 1-325 (whole main inventory), 326-404 by grep for link and remote items |

**Not read.** `claude-code.md` outside the ranges above (about 5,000 lines); the Sources sections of every survey (URLs are taken from the entries themselves). Nothing was missing or unreadable. Two survey notes matter for trust: Claude Code's section 3 and section 5 contain the "single-use work order for each session" error that review 3 corrects (used corrected below), and Claude Desktop's section 5 still says the SSH helper "survives app exit", which its own review removed (used corrected below).

## 2. Features

### 2.0 Control side versus execution side: every documented split

The reader's design: a master (accounts, chats, web UI, relays approvals to people) and nodes that run the agent loop and the tools on the machine that owns the code; `node.toml` on the node is the authority and nothing the master sends widens it; the node dials the master over TLS with a pinned certificate.

In the table, "loop" means the agent's model-call/tool-call cycle; "tools" means file and shell execution. "Refuse" asks whether the execution side can turn down what the control side asks for. "n.d." = nothing documented in the survey.

| Design | Loop runs | Tools run | Model credentials | Policy holder; can execution side refuse? | Who dials, transport | Auth and pairing | Data leaving the execution machine | Disconnect / restart | Multi-user | Status | Evidence | Check |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Codex app-server + remote TUI | app-server host | app-server host | n.d. for the remote case | app-server host's layered config and `requirements.toml`; client is a UI | client dials `codex app-server --listen ws://…\|unix://…`; JSON-RPC over WebSocket or UDS; plain ws only on localhost or through an SSH tunnel | `capability-token` or `signed-bearer-token`, enforced before `initialize`; Origin header gets 403; **non-loopback listeners unauthenticated by default "during rollout"** | full thread/turn/item stream to the client | n.d. | n.d. | experimental | https://learn.chatgpt.com/docs/app-server.md | reviewed (auth), unreviewed (remote TUI) |
| Codex remote control, relay, mobile | paired host | paired host | paired host | paired host; admins can disable with `allow_remote_control = false` | via a "secure relay layer" (mechanism not described) | short-lived pairing codes (`codex remote-control pair`), QR on mobile | through the relay (unspecified) | n.d. | single account | GA (relay is vendor-claim) | https://learn.chatgpt.com/docs/developer-commands.md; https://learn.chatgpt.com/docs/remote-connections.md; https://learn.chatgpt.com/docs/mobile.md | reviewed (remote-control), unreviewed (relay) |
| Codex SSH remote projects + Handoff | remote host (app-server started over SSH in the login shell) | remote host | n.d. | remote host | desktop opens SSH using `~/.ssh/config` aliases | the user's SSH setup | chat stream to the desktop | Handoff moves a chat and its Git state between local and a connected host with a saved project for the same repo; a running response is interrupted first; not to Codex Cloud | n.d. | GA | https://learn.chatgpt.com/docs/remote-connections.md | corrected |
| Codex Cloud | OpenAI per-task workspace | same workspace | OpenAI side; network secrets substituted by proxy | OpenAI environment config | user to OpenAI | ChatGPT account | n/a (runs at vendor) | keeps running while the computer sleeps | workspace-shared environments | GA (Legacy runtime: deprecated, planned) | https://learn.chatgpt.com/docs/cloud.md; https://learn.chatgpt.com/docs/environments/cloud-environments.md | reviewed |
| Codex remote executors / `exec-server` | unclear | MCP servers can run "on a remote executor" (`environment_id`) | `codex exec-server --remote` accepts `CODEX_API_KEY` | n.d. | n.d. | n.d. | n.d. | n.d. | n.d. | unclear (no page) | https://learn.chatgpt.com/docs/extend/mcp.md; https://learn.chatgpt.com/docs/config-file/environment-variables.md | unverified |
| OpenCodex Remote Workspace | Hub (runs Codex, Claude Code or Pi and the model session) | Executor: files from one approved root; commands only on Linux inside bubblewrap | Hub | read-only by default; write needs a command-sandbox probe; a disconnected executor never falls back to the hub filesystem (which side sets the root: not stated) | **executor dials out** over WebSocket, no public port | encrypted RPC v2 (Ed25519-signed ECDH, AES-256-GCM); one-time pairing code; hub stores only a hash of the bearer; 10 failed codes in 10 min give 429 | file contents and command output to the hub | no fallback when disconnected; reconnect n.d. | n.d. | experimental (`OCX_REMOTE_WORKSPACE_ENABLED=1`) | https://opencodex.me/guides/remote-workspace/ | corrected |
| OpenCode server and clients | one background server per machine | same server | server host (inferred) | server's permission rules plus an admin-only `policies` layer that only tightens | client attaches with `--server <url>`; HTTP (OpenAPI 3.1), event stream, WebSocket for PTY; SSH port-forward suggested | password (HTTP Basic, user `opencode`); `opencode pair` prints a single-use 5-minute link and QR, sets a 30-day cookie; `pair --remote` goes through "OpenTunnel" end-to-end encrypted, password still required | everything the client views | durable per-session log readable after a sequence number with `follow=true`; the live event stream is volatile and drops slow consumers; network client has no replay or auto-reconnect | n.d. (no fleet; one server per machine) | GA; HTTP API labelled experimental | https://opencode.ai/docs/troubleshooting/; https://opencode.ai/docs/cli/web/; https://opencode.ai/docs/api/ | reviewed |
| Claude Code Remote Control | user's machine | user's machine | user's machine (full-scope claude.ai login; `setup-token` tokens rejected) | local process and local settings; project settings can only force auto-connect off | **local process dials out** (HTTPS only, registers and polls, then a "streaming connection"; wire format not documented) | claude.ai account; several short-lived single-purpose credentials; optional Trusted Devices (beta) | transcript (messages, responses, tool activity) stored on Anthropic servers while connected | server mode exits after about 10 min of outage, interactive retries indefinitely; sessions resumable for about 4 h after Ctrl+C; explicit takeover rules | one account; Team/Enterprise Owner toggle | GA | https://code.claude.com/docs/en/remote-control.md | reviewed |
| Claude Code cloud sessions | Anthropic VM per session | same VM | Anthropic; GitHub credential stays in a server-side proxy | committed `.claude/settings.json` plus server-managed settings | user to Anthropic | claude.ai login | n/a | VM pauses when idle, restores; if reclaimed, fresh VM with history but no running processes | org visibility options | GA | https://code.claude.com/docs/en/claude-code-on-the-web.md; https://code.claude.com/docs/en/security.md | reviewed |
| Claude Code self-hosted runner | child `claude` process on the customer's runner | same runner | inference from the child; session OAuth token delivered and rotated over stdin; Bedrock/Agent Platform possible | **control plane sends each session's tool list and permission rules** (plus model, connectors, hooks); image `managed-settings.json` also applies; runner can refuse sessions with `--confine-repo-settings enforce` or drop repo grants with `--trust-workspace false` | **runner dials out only**: HTTPS poll (lease and heartbeat), SSE session stream, optional WebSocket for the git proxy; Anthropic never connects in | environment secret (365 days) gives runner tokens; in orchestrator mode each spawned runner gets a single-use work-order JWT that registers exactly one runner and expires with the spawn lease (default 120 s); ES256 session JWT | prompts, responses and tool results to `api.anthropic.com`; Anthropic stores the transcript; checkouts and secrets stay | lease lapses after about 60 s; requeue; resume on a fresh runner from a fresh clone loses unpushed work; cut-off commands may rerun, shown as "outcome unknown" | org; one owner per runner; no per-environment ACL | beta (public beta, Team/Enterprise, off by default) | https://code.claude.com/docs/en/self-hosted-environments.md; https://code.claude.com/docs/en/self-hosted-environments-configuration.md; https://code.claude.com/docs/en/self-hosted-environments-deploy.md | corrected |
| Claude Desktop SSH sessions | remote host (helper and Claude Code under `~/.claude/remote/`) | remote host | remote app login, renewed only while Desktop is connected | remote host plus admin keys (`sshHostAllowlist`, `sshHostCommands`, …) | Desktop opens SSH (own OpenSSH or built-in); agent, X11 and tunnel forwarding disabled | `~/.ssh/config` | session to Desktop | login can lapse overnight and stop runs; docs say not to count on overnight tasks; "survives app quit" is NOT supported | admin keys | GA (some beta flags) | https://code.claude.com/docs/en/desktop | corrected |
| Claude Desktop Dispatch | user's desktop (one persistent Cowork thread) | desktop | desktop | desktop; spawned computer-use approvals last 30 min | phone to desktop; transport not described (the survey's architecture notes say cloud, Remote Control and mobile rely on Anthropic-hosted relay infrastructure, `bridge.claudeusercontent.com`) | install both apps, enable Dispatch | via Anthropic | computer must stay awake, app open | single user | beta (limited, not for new users) | https://support.claude.com/en/articles/13947068; https://code.claude.com/docs/en/desktop | corrected |
| DeepSeek harness web server | `dsh web` server process (inferred, not stated) | same process | credentials YAML under `$DSH_HOME` | server process | browser dials server; Typert RPC over `/api` plus multiplexed WebSocket | one launch token plus cookie; Host/Origin fence | everything the browser views | `follow()` plus `page()` fill gaps; reconnect replaces the window from a snapshot | n.d. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-server; https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments | corrected |
| DeepSeek harness SSH backend | local host | remote POSIX host (fs, subprocess, sandbox via `ctx.ssh`) | local; model transport and session storage stay on the host | same sandbox policy applies remotely | harness dials OpenSSH | existing OpenSSH config and host keys; helper checked against a configured SHA-256 hash | file contents and command output to the host | SSH loss is final, no reconnect | n.d. | no maturity label | https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/ssh.md | corrected |
| Pi pi-env (+ pi-durable) | local durable worker | remote Rust daemon (file ops, `listFiles`, PTY, protocol servers such as MCP) | local | local | harness dials SSH (BatchMode, strict host-key checking, no forwarding); framed stdio protocol | daemon named by SHA-256 and verified before start | tool I/O to the worker | pi-durable commits before display, checkpoints each step, resumes with a restart limit (default 5) | n.d. | unclear (pi-durable experimental) | https://github.com/earendil-works/pi/blob/main/packages/env/README.md; https://github.com/earendil-works/pi/blob/main/packages/durable/README.md | reviewed |
| Cursor self-hosted workers | **Cursor cloud** (loop, inference, planning) | customer worker: edits, terminal, computer use, stdio MCP (HTTP/SSE MCP runs at Cursor) | Cursor; worker keeps machine credentials | worker-side `.cursor/hooks.json` plus team/enterprise hooks apply, but Cursor's server issues the tool calls; OS user is the boundary | **worker dials one long-lived outbound HTTPS connection**; no inbound ports | My Machines: user login, personal key or one-hour sub-token; Team Pools: service-account key; per-claim session token (one claim, one worker ID, 7-day backstop); 5-minute claim identity tokens | file contents, terminal output, diffs, screenshots, local MCP results, routing metadata; transcripts may be stored | reconnect after 90 s of silence; `claimed_offline` hibernation and wake; idle release 3600 s | Team Pools (Enterprise); requests reach only the triggering user's machines | unclear (no label) | https://cursor.com/docs/cloud-agent/self-hosted; https://cursor.com/docs/cloud-agent/self-hosted/pool; https://cursor.com/blog/self-hosted-machines | reviewed |
| Cursor cloud agents | Cursor VM (Firecracker microVM) | same VM | Cursor; secrets injected as env | team/environment config | user to Cursor | Cursor account | n/a | VM recycled after idle; snapshots kept 30 days | teams | GA | https://cursor.com/docs/cloud-agent; https://cursor.com/docs/cloud-agent/security | reviewed |
| Antigravity Remote Control | user's host | user's host | host's CLI credentials | host; settings cannot be changed from the web | **host opens a reverse tunnel** to the Google-hosted dashboard; daemon as OS service | same Google account | via Google (relay and encryption undescribed) | n.d. | single account | GA (no label) | https://antigravity.google/docs/remote-control | reviewed |
| Antigravity Remote Agents | Google-managed environment | same | Google; OAuth credentials encrypted at rest, injected on call | Google; Turbo preset by default | user to Google | personal AI Pro/Ultra | n/a | persists when the user's machine is offline | personal accounts only | GA, rolling out | https://antigravity.google/docs/remote-agents | unreviewed |
| OpenHands Canvas + Agent Server | Agent Server (backend host or per-conversation container) | same | backend (LLM config and secrets are per backend) | backend; client-supplied per-conversation container limits are not capped by server env | **browser/Canvas dials the backend URL**; HTTP plus WebSocket | API key in `X-Session-API-Key`; **no auth by default** | everything the client views | n.d. | OSS: single user; Enterprise: per-conversation pods | GA | D:/openhands/usage/agent-canvas/architecture.md; D:/openhands/usage/agent-canvas/backends.md; D:/sdk/arch/agent-server.md | reviewed |
| OpenClaw Gateway + nodes | Gateway | on the Gateway, in a sandbox, or on a node through `node.invoke` (nodes are peripherals) | Gateway | a node command runs only if the node declares it, it is in the node's approved set, and it is in the Gateway's per-platform allowlist; `deny` wins; the node can withhold a command by not declaring it | **node dials the Gateway** WebSocket (default `127.0.0.1:18789`), role `node` | device identity signs `connect.challenge`; device pairing (5 min) then capability approval; SSH-verified auto-approval on by default; loopback auto-approved | command results to the Gateway | n.d. | operator scopes; Gateway plus nodes are one operator trust domain | GA | https://docs.openclaw.ai/concepts/architecture; https://docs.openclaw.ai/gateway/pairing; https://docs.openclaw.ai/nodes/command-policy | corrected |
| OpenClaw cloud workers (Crabbox) | `worker-turn`: on the box, inference proxied by the Gateway; `remote-exec` (Codex): on the Gateway | on the box | **provider keys stay on the Gateway** | Gateway; worker route has a closed method allowlist and cannot call operator RPCs | worker "dials" the Gateway's authenticated public worker route ("outbound-only" and "WebSocket" are not stated) | new worker credential per dispatch, Gateway stores its hash; rotation plus owner-epoch fencing rejects stale workers | n.d. beyond results; GitHub publication goes through a Gateway broker | suggested lease 4 h total / 45 min idle; warm snapshots (Linux) | n.d. | beta (scorecard M3 Beta, quality Alpha 67%) | https://docs.openclaw.ai/gateway/cloud-workers; https://docs.openclaw.ai/gateway/cloud-workers/security-model; https://docs.openclaw.ai/maturity/scorecard | corrected |
| goose `serve` (remote) | `goose serve` on the VM | same | server host (inferred) | server; ACP permission callback "is not a sandbox" | Desktop dials HTTPS `/acp` plus WebSocket | shared secret `X-Secret-Key` (plaintext in plist and Desktop settings, manual rotation); self-signed TLS with printed fingerprint for pinning, else TOFU | everything the client views | `session/load` to resume | n.d. | unclear (no label) | https://goose-docs.ai/docs/guides/remote-goose-server.md | reviewed |
| goose Roaming | the running agent | same | same | all-or-nothing ACP access once keys are accepted | peer-to-peer over iroh (QUIC), no open ports; relays goose-managed unless `GOOSE_ROAM_RELAYS` | long-lived node identity, shareable connection card, both sides accept keys; revoke closes connections in seconds | ACP stream to the peer | spawned OS processes survive revocation; no co-driving | n.d. | experimental, not in released binaries | https://goose-docs.ai/docs/guides/roaming-agents.md | reviewed |
| Hermes Agent terminal backends | host (`AIAgent`) | backend: docker, ssh, modal, daytona, vercel_sandbox, plugins | host; optional iron-proxy keeps keys out of sandboxes | host; container backends skip dangerous-command approval and disable checkpoints | host to backend | per backend | file and command I/O | Modal/Daytona hibernate; Vercel snapshots keep files not processes | n.d. | GA | /user-guide/features/tools; /developer-guide/terminal-environment-plugin | reviewed |
| Hermes Agent gateway + Relay | gateway process | gateway's terminal backend | gateway | gateway | CLI attaches over a local WebSocket with single-use 30 s tickets; Relay: **gateway dials out** one authenticated WebSocket to a connector that holds platform bot tokens | per-gateway secret and delivery key | messages through the connector | connector buffers while the gateway is offline | multiplexed profiles | GA (gateway), experimental (Relay) | /developer-guide/gateway-internals; /user-guide/messaging/relay | reviewed |
| Zed remote development | Zed Agent in the editor; where LLM calls originate is contradictory on the page | headless Zed server on the remote (code, language servers, tasks, terminal) | local, remote, or agent-managed for external agents | n.d. | local `ssh` with ControlMaster; server binary auto-downloaded or uploaded | SSH | n.d. | reconnect keeps unsaved edits | n.d. | GA | https://zed.dev/docs/remote-development.md; https://zed.dev/docs/ai/external-agents.md | unreviewed |
| ACP (protocol) | agent process | agent's own tools, but `fs/*` and `terminal/*` calls are executed **by the client**, which must apply its own policy | agent | agent stores `allow_always`; permission requests are advisory; client must check paths and command, args, env | client spawns agent, stdio JSON-RPC; HTTP/WebSocket remote transport only an RFD | `authMethods` at initialize (`agent` or `terminal` type) | whatever the agent sends | `session/load` replays history, `session/resume` restores without replay; no stream resumption in v1 | multi-user agents "should scope sessions to the owner" | GA (v1); remote transport planned | https://agentclientprotocol.com/protocol/v1/terminals.md; https://agentclientprotocol.com/protocol/v1/file-system.md; https://agentclientprotocol.com/rfds/streamable-http-websocket-transport.md | reviewed |
| Warp self-hosted worker | execution environment (provisioning then agent loop, per the run lifecycle; inferred) | worker backend: Docker, Kubernetes, Direct, Command, one-shot Direct | Warp Agent inference routes through Warp unless team keys or BYO LLM; Claude Code/Codex harnesses call providers directly | **control plane pushes task, resolved config and scoped runtime credentials** | **worker dials out only** | self-hosted worker API key; addressed by worker ID (`--host`) | transcripts and artifacts to Warp-managed storage by default; control-plane metadata | n.d. | Enterprise teams | GA (Enterprise) | https://docs.warp.dev/factories/self-hosting/; https://docs.warp.dev/platform/architecture/ | reviewed |
| Cline hub/spoke | spoke worker owned by the per-machine hub daemon | hub routes capability requests (diff, shell) to the client that advertised them | n.d. | hub | clients connect to the hub over WebSocket (`127.0.0.1:25463`); `remote` backend mode unexplained | **no authentication documented** | n/a (local) | sessions outlive clients | participants with roles creator/participant/observer | GA; `remote` unclear | https://docs.cline.bot/sdk/architecture/hub-spoke.md | corrected |
| Cline SSH remote (Desktop) | remote host (uploaded ~30 MB helper runs a hub on host loopback) | remote host (agent, tools, files, MCP, session history) | provider key sent over the tunnel per session, not written to the host | chat, approvals and provider settings stay local, i.e. **on the control side** | Desktop opens `ssh -L` to a random local port | key-based only, host key must already be trusted, strict checking, one host at a time | session to Desktop | tunnel closes after about 45 s of network loss | n.d. | GA | https://docs.cline.bot/usage/cline-desktop-ssh.md | reviewed |
| **rizzy-harness (ours)** | node | node | **node** (`config.toml` key, calls provider directly) | **node**: `node.toml` is the authority, unknown keys rejected, approval mode `min(requested, node max)`, no HTTP or protocol endpoint writes it | **node dials master**: WebSocket over TLS 1.2/1.3, pinned self-signed master certificate | Ed25519 node key signs a nonce bound to the cert fingerprint; single-use 15-minute join token stored hashed; revoke drops link | every session event (tool output, model text) is mirrored to the master | link resume by `Resume{after}`; node restart closes the open turn as `interrupted` and denies pending approvals | accounts, workspaces, `can_agentic` grant | works (link back-pressure unbounded on `main`, AUDIT L1) | `crates/link/src/lib.rs:43-49`, `crates/master/src/nodes.rs:16-80`, `crates/node/src/policy.rs:9-126`, `crates/node/src/session.rs:154-191` | from own inventory |

**Which match ours, which invert it, which are neither.**

- **Match (loop and tools both on the machine that owns the code; the other side is a UI or relay):** Codex SSH remote projects and remote TUI, Codex remote control, OpenCode server, Claude Code Remote Control, Claude Code self-hosted runner, Claude Desktop SSH and Dispatch, DeepSeek harness web server (single machine), Antigravity Remote Control, OpenHands Canvas + Agent Server, OpenClaw cloud workers in `worker-turn` mode, goose `serve` and Roaming, Warp self-hosted worker (loop placement inferred), Cline SSH remote, Zed remote development (partly: model-call origin unclear).
- **Inverted (loop on the control side, tools on the worker):** Cursor self-hosted workers, OpenCodex Remote Workspace, OpenClaw Gateway + nodes and `remote-exec`, DeepSeek harness SSH backend, Pi pi-env, Hermes Agent terminal backends. ACP is a partial inversion (agent loops, client executes `fs`/`terminal`), and Cline hub/spoke routes some tool capabilities to clients.
- **Neither (loop and tools both at the vendor):** Codex Cloud, Claude Code cloud sessions, Cursor cloud agents, Antigravity Remote Agents, Cline Cloud Agents, Warp hosted runs, OpenHands Cloud.

**What separates the "match" designs from ours.**

1. **Who decides.** No other row documents an execution side whose local file outranks what the control side sends. The Claude Code runner is the closest in shape and the furthest in authority: the control plane supplies tool list and permission rules per session, and the runner can only add a host-side floor (image managed settings, `--confine-repo-settings enforce`). Warp's control plane also pushes resolved config. OpenClaw lets the node withhold commands by not declaring them, but the Gateway's allowlist is the gate. Cline SSH keeps approvals on the control side by design.
2. **Where model credentials live.** OpenClaw `worker-turn`, Cline SSH and Cursor keep provider keys on the control side (inference proxied, or the key sent per session). We put the key on the node. That is simpler and keeps inference off the master, but every node holds a long-lived provider key (see 2.4 "Central credential broker").
3. **What leaves the execution machine.** Claude Code's runner and Remote Control send prompts, responses and tool results to the vendor and store the transcript there; Cursor's worker sends file contents, terminal output, diffs and screenshots; Warp stores transcripts and artifacts by default. Our node mirrors every event to the master, which is the owner's own server; the difference is ownership of the control side, not the amount of data.
4. **Who dials.** Outbound-only from the execution side is the norm (Claude Code runner and Remote Control, Cursor, Warp, OpenCodex executor, OpenClaw nodes, Antigravity, Hermes Relay); ours matches. The UI-to-server designs (Codex remote TUI, OpenCode, goose, OpenHands, DeepSeek) instead need the execution host to listen, which is where their weakest auth defaults sit.

### 2.1 Headless and non-interactive modes

#### One-shot headless run
What it is: run one prompt (or a recipe) without an interactive UI, print the answer and exit, for scripts and CI.
Variants: most are a flag on the main CLI; some are profiles of one launcher (DeepSeek harness), some are a separate CLI (Warp `oz`), one is a deprecated CLI (OpenHands). Whether the run is persisted as a resumable session differs (Claude Code persists by default; goose and Codex have opt-outs). Whether edits are applied differs: Cursor only proposes edits unless `--force`.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `codex exec`: read-only sandbox by default; progress on stderr, final message on stdout; stdin as context; `--ephemeral`, `--ignore-user-config`, `--ignore-rules`; a failing `required` MCP server makes it exit | GA | https://learn.chatgpt.com/docs/non-interactive-mode.md | reviewed |
| OpenCode | `opencode run "<prompt>"` with `--format json`, `--file`, `--agent`, `--session`, `--continue`, `--auto`, `--server` (run against a chosen server) | GA | https://opencode.ai/docs/cli/commands/; https://opencode.ai/docs/cli/ | unreviewed |
| Claude Code | `claude -p` reads stdin (10 MB cap), exit 0/non-zero; runs are saved as resumable sessions unless `--no-session-persistence`; `--max-turns`, `--max-budget-usd` (client-side estimate); after the last turn waits for background subagents, workflows and Monitor watches, other idle waiting capped at 10 min; SIGTERM exits 143 | GA | https://code.claude.com/docs/en/headless.md; https://code.claude.com/docs/en/cli-reference.md; https://code.claude.com/docs/en/glossary.md | reviewed |
| Claude Desktop | none: scripting and the Agent SDK are CLI-only | GA (statement) | https://code.claude.com/docs/en/platforms | unreviewed |
| DeepSeek harness | `dsh headless "job"` runs one persisted session and exits; one of several launcher profiles | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md | unreviewed |
| Oh My Pi | `-p` one-shot, plus json, rpc, rpc-ui and acp modes | GA | https://omp.sh/docs/modes | unreviewed |
| Pi | print, JSON, RPC and SDK interfaces; `PI_OFFLINE=1`/`--offline` disables automatic network activity | GA | https://pi.dev/docs/latest/cli | unreviewed |
| Cursor | `agent -p`; edits only proposed unless `--force`/`--yolo`; `CURSOR_API_KEY` | GA | https://cursor.com/docs/cli/headless | unreviewed |
| Antigravity | `agy -p`: stdout response, stderr diagnostics, `--print-timeout`, `--sandbox`, `--dangerously-skip-permissions`; uses cached credentials, unauthenticated CI exits with an error | GA | https://antigravity.google/docs/cli/headless | unreviewed |
| OpenHands | OpenHands-CLI `openhands --headless -t "task"` (`-f`, `--json`); the CLI is no longer actively maintained and headless use is meant to go through the Agent Server API | deprecated (unmaintained) | https://github.com/OpenHands/OpenHands-CLI | corrected |
| goose | `goose run -t/-i/--recipe`, `--no-session`, `-q`, `--provider`, `--model`; context overflow summarises automatically; `GOOSE_DISABLE_SESSION_NAMING` suggested for CI | GA | https://goose-docs.ai/docs/guides/running-tasks.md | unreviewed |
| Hermes Agent | `hermes chat -q`, `--query-file`, `-z` one-shot with worktrees; `hermes send` pipes script output to platforms | GA | /user-guide/cli | unreviewed |
| Zed/ACP | nothing documented (no server-hosted or headless agent) | - | https://zed.dev/docs/remote-development.md | unreviewed |
| Warp | `oz agent run` (local) and `oz agent run-cloud`; `oz` is called "a supported legacy interface" on one page and "current" on another; the `warp` CLI has no documented print mode | GA (labelled legacy) | https://docs.warp.dev/agents/cli/oz-cli/ | corrected |
| Cline | headless auto-engages with `--json`, piped stdin or redirected stdout; `-t` timeout (no default), `--retries`; exit codes not documented | GA | https://docs.cline.bot/usage/cli-overview.md; https://docs.cline.bot/cli/cli-reference.md | unreviewed |
| rizzy-harness (ours) | none: no one-shot CLI; closest is the HTTP API with a bearer token, or a routine | none (CLI), partial (API) | `crates/cli/src/main.rs:31-118` | from own inventory |

#### Structured event stream and schema-checked output
What it is: machine-readable output (JSON, NDJSON event stream) and an optional JSON Schema the final answer must satisfy.
Variants: event vocabularies differ (thread/turn/item, init/step_update/result, agent_event/run_result). Only a few validate the final answer against a schema. Claude Code adds CI-gating fields (`mcp_server_errors`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `--json` JSONL (`thread.started`, `turn.*`, `item.*`, `error`, with usage); `--output-schema` plus `-o` file | GA | https://learn.chatgpt.com/docs/non-interactive-mode.md | reviewed |
| OpenCode | `--format json` | GA | https://opencode.ai/docs/cli/commands/ | unreviewed |
| Claude Code | `--output-format text\|json\|stream-json`, `--input-format stream-json`; `json` carries `result`, `session_id`, `total_cost_usd`; `--json-schema` gives `structured_output`; stream events `system/init` (tools, MCP status, plugin errors, `capabilities`), `system/api_retry`, `permission_denied`; subagent messages carry `parent_tool_use_id`; CI should gate on `mcp_server_errors` | GA | https://code.claude.com/docs/en/headless.md | reviewed |
| Oh My Pi | `--mode json` event stream | GA | https://omp.sh/docs/modes | unreviewed |
| Pi | JSON mode | GA | https://pi.dev/docs/latest | unreviewed |
| Cursor | `text`, `json`, `stream-json` (`--stream-partial-output`) | GA | https://cursor.com/docs/cli/headless | unreviewed |
| Antigravity | `--output-format text\|json\|stream-json`, NDJSON input, `--json-schema`; envelope with `conversation_id`, status, usage, duration, `num_turns`; events `init`, `step_update`, `result` | GA | https://antigravity.google/docs/cli/headless | unreviewed |
| OpenHands | CLI `--json` | deprecated | https://github.com/OpenHands/OpenHands-CLI | corrected |
| goose | `--output-format text\|json\|stream-json`; recipes support `response.json_schema` | GA | https://goose-docs.ai/docs/guides/running-tasks.md | unreviewed |
| Cline | `--json` NDJSON with `type` and `ts` (`agent_event`, `hook_event`, `team_event`, `run_result`, `run_aborted`) | GA | https://docs.cline.bot/usage/cli-overview.md | unreviewed |
| Warp | `oz run message list\|read\|watch\|send` for run messages | GA (legacy) | https://docs.warp.dev/agents/cli/oz-cli/ | corrected |
| rizzy-harness (ours) | none as CLI output; the event log over `/api/ws` is the structured stream | none (CLI) | `crates/master/src/http.rs:26-175` | from own inventory |

#### Approval behaviour when nobody can answer
What it is: what a headless or unattended run does when a tool call needs approval.
Variants: fail closed (OMP, Hermes Agent, Codex auto-review fails closed), soft-deny and continue with exit 0 (Antigravity), approve everything by default (Cline CLI), defer the call and end the turn so a human can decide later (Claude Code), end the run as blocked (Warp), or wait for a human click (ours). Trust of the project folder in headless mode is a separate axis: Claude Code `-p` runs repo hooks and `.mcp.json` with no trust dialog; Pi skips protected project resources.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `--approve-for-me` routes approvals to Auto-review (reviewer agent, fails closed); `--full-auto` removed | GA | https://learn.chatgpt.com/docs/non-interactive-mode.md | reviewed |
| Claude Code | PreToolUse hook `permissionDecision: "defer"` (one tool call per turn) ends the turn with `stop_reason: "tool_deferred"`; the caller decides in its own UI and resumes with `claude -p --resume <id>`; no timeout; `-p` without `--bare` runs project hooks and `.mcp.json` even in untrusted folders; `--restricted` removes command tools and refuses bypass | GA | https://code.claude.com/docs/en/hooks.md; https://code.claude.com/docs/en/headless.md; https://code.claude.com/docs/en/changelog.md | unreviewed (defer), reviewed (headless trust) |
| Oh My Pi | approvals fail closed without a UI | GA | https://omp.sh/docs/approvals | unreviewed |
| Pi | in headless modes `ask` trust skips project resources | GA | https://pi.dev/docs/latest/security | unreviewed |
| Cursor | edits are proposals unless `--force`/`--yolo` | GA | https://cursor.com/docs/cli/headless | unreviewed |
| Antigravity | un-obtainable approvals are soft-denied (run continues, exit 0, notice on stderr); `--mode=auto` falls back to default | GA | https://antigravity.google/docs/cli/headless | unreviewed |
| Hermes Agent | cron, webhooks and the API server run headless with deny-by-default approvals; approval timeout 300 s fails closed | GA | /user-guide/security | reviewed |
| Cline | CLI `--auto-approve` defaults to true (false in ACP); schedules default to yolo | GA | https://docs.cline.bot/cli/cli-reference.md | reviewed |
| Warp | unattended CLI agent runs end as blocked when the agent stops to ask | GA | https://docs.warp.dev/platform/architecture/ | unreviewed |
| DeepSeek harness | GitHub review sessions run with the `read-only` permission preset | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/github-review | reviewed |
| rizzy-harness (ours) | agentic routines run only in `ask`/`ask_all`: a command waits for a human click, cancelled after 24 h; `auto` is rejected, so unattended agentic runs can only do read-only work | works | `crates/master/src/routines/spec.rs:131-141`, `crates/master/src/routines/runner.rs:95` | from own inventory |

#### Config isolation for reproducible CI runs
What it is: a way to run headless without picking up the user's or the repo's ambient configuration, so CI results do not depend on the machine.
Variants: skip discovery entirely (Claude Code `--bare`), ignore user config or rules (Codex), point at a separate config and data dir (Cline), pre-seed read-only plugin directories at image build (Claude Code).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `--ephemeral`, `--ignore-user-config`, `--ignore-rules` | GA | https://learn.chatgpt.com/docs/non-interactive-mode.md | reviewed |
| Claude Code | `--bare` skips hooks, skills, plugins, MCP, memory, CLAUDE.md and OAuth (needs `ANTHROPIC_API_KEY` or `apiKeyHelper`), planned to become the `-p` default; `CLAUDE_CODE_PLUGIN_SEED_DIR` read-only plugin seeds built into images | GA (bare default: planned) | https://code.claude.com/docs/en/headless.md; https://code.claude.com/docs/en/plugins/org.md | reviewed (bare), unreviewed (seeds) |
| Cline | `--config <dir>`/`CLINE_DIR` and `--data-dir`/`CLINE_DATA_DIR`; parallel runs are separate processes | GA | https://docs.cline.bot/cli/cli-reference.md | unreviewed |
| goose | `--no-session` | GA | https://goose-docs.ai/docs/guides/goose-cli-commands.md | unreviewed |
| rizzy-harness (ours) | not applicable (no headless CLI); data dir is set with `--data-dir`/`HARNESS_DATA_DIR` | none | `crates/cli/src/main.rs:130-142` | from own inventory |

### 2.2 CI integrations

#### Official CI action or pipeline integration
What it is: a packaged way to run the agent inside a CI job (GitHub Actions, GitLab CI) triggered by events or mentions.
Variants: who can trigger it (write-access checks, bot allowlists), how the model key is protected from job steps (Codex's Responses API proxy, Claude Code's OIDC to cloud providers), and whether a read-only analysis job is split from a write job (Codex, Cline).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `openai/codex-action@v1` installs the CLI, starts a Responses API proxy so shell steps do not get the key, runs `codex exec`; `safety-strategy` `drop-sudo` (default), `unprivileged-user`, `read-only`, `unsafe` (required on Windows); `allow-users`/`allow-bots`; by default only users with write access can trigger; auto-fix pattern splits a read-only job from the PR-writing job | GA | https://learn.chatgpt.com/docs/github-action.md; https://learn.chatgpt.com/docs/non-interactive-mode.md | corrected |
| OpenCode | V1: `opencode github install` sets up an app and workflow, `/opencode` mentions, OIDC token exchange or `use_github_token`; V2: the Action is "no longer published" | V1 GA; V2 removed | https://opencode.ai/v1/docs/github/; https://opencode.ai/changelog | reviewed |
| Claude Code | `claude-code-action@v1` (interactive mode on `@claude`, automation mode on any event); write-access and human-actor checks before Claude starts (known gap: on public repos with OIDC the credential steps run before the write check); `use_bedrock`/`use_vertex`/`use_foundry` with GitHub OIDC so no static cloud key; GitLab CI via a user-written job (page maintained by GitLab) | GA (Action); beta (GitLab) | https://code.claude.com/docs/en/github-actions.md; https://code.claude.com/docs/en/github-actions-cloud-providers.md; https://code.claude.com/docs/en/gitlab-ci-cd.md | reviewed (trigger checks), unreviewed (cloud providers, GitLab) |
| Cursor | a GitHub Actions page exists in the docs index (not opened) | unclear | https://cursor.com/docs/cli/headless | unverified |
| Antigravity | no GitHub Action found; no API-key auth for CI except the `gemini` provider | - | https://antigravity.google/docs/cli/headless | unreviewed |
| OpenHands | SDK GitHub workflow guides listed in the index (not opened) | unclear | D:/llms.txt | unreviewed |
| Warp | `oz-agent-action` for PR review, issue fixes, CI fixes | GA | https://docs.warp.dev/platform/integrations/ | unreviewed |
| Cline | GitHub Actions samples: issue RCA on `@cline` (three jobs: authorise with read-only token, analyse with `--auto-approve true --json`, post with write-only issues token) and PR review | GA (sample) | https://docs.cline.bot/cli/samples/github-integration.md | unreviewed |
| rizzy-harness (ours) | none (the repo's own CI workflow is for building the harness, not for running it) | none | `.github/workflows/ci.yml` | from own inventory |

#### Machine credentials for CI and automation
What it is: how a non-human run authenticates to the model and to the vendor's control plane.
Variants: long-lived subscription tokens (Claude Code `setup-token`, one year, inference only), API keys with chosen expiry (Warp), short-lived federated tokens (Claude Code via GitHub OIDC, Cursor OIDC sub-tokens, Warp `oz federate`), or no machine identity at all (Claude Code self-hosted dispatch and gateway need a human login).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `CODEX_API_KEY` for `exec-server --remote`; Action keeps the key behind a proxy | GA / unclear | https://learn.chatgpt.com/docs/config-file/environment-variables.md; https://learn.chatgpt.com/docs/github-action.md | unverified / corrected |
| Claude Code | `claude setup-token` prints a one-year OAuth token for `CLAUDE_CODE_OAUTH_TOKEN` (model requests only, no Remote Control); bare mode needs an API key; dispatch to self-hosted environments needs a human claude.ai OAuth login (scope capped at 30 days); gateway has no service-token flow | GA (token); beta (dispatch) | https://code.claude.com/docs/en/authentication.md; https://code.claude.com/docs/en/self-hosted-environments-testing.md; https://code.claude.com/docs/en/claude-apps-gateway.md | reviewed (token), unreviewed (rest) |
| Cursor | `CURSOR_API_KEY`; one-hour `POST /v1/sub-tokens`; 5-minute RS256 OIDC tokens inside cloud runs via a local socket | GA | https://cursor.com/docs/cli/headless; https://cursor.com/docs/cloud-agent/identity | reviewed |
| Antigravity | cached credentials only; unauthenticated CI exits | GA | https://antigravity.google/docs/cli/headless | unreviewed |
| Warp | `WARP_API_KEY`: personal, agent or self-hosted worker keys (expiry options not confirmed on the cited page); `oz federate issue-token` gives 5 min to 3 h OIDC tokens inside runs | GA | https://docs.warp.dev/agents/cli/oz-cli/; https://docs.warp.dev/agents/cli/oz-cli/federate/ | corrected / unreviewed |
| rizzy-harness (ours) | one admin-equivalent API token (`Authorization: Bearer`) for scripts; per-routine webhook bearer secrets (shown once, rotatable, stored as SHA-256) | works | `crates/master/src/auth.rs:528-580`, `crates/master/src/http/hooks.rs:1-60` | from own inventory |

#### Event-triggered remote runs (chat, tracker, git host, webhook)
What it is: starting an agent run from an external event (Slack mention, Linear assignment, PR mention, signed webhook) rather than from the harness UI.
Variants: most vendor products start a vendor-cloud run; DeepSeek harness and ours start a local run from a signed webhook; trust of the triggering content differs (DeepSeek harness labels PR fields untrusted and forces read-only; ours strips memory and web tools).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Slack `@ChatGPT` delegates to a Codex Cloud task (needs a published shared environment); Linear assignment or `@Codex` creates a cloud chat | GA | https://learn.chatgpt.com/docs/third-party/slack.md; https://learn.chatgpt.com/docs/third-party/linear.md | unreviewed |
| Claude Code | cloud scheduled tasks with schedule, per-task `/fire` API token and GitHub triggers; Slack app (legacy) and Claude Tag (public beta) bind threads to cloud sessions | research preview / legacy / public beta | https://code.claude.com/docs/en/routines.md; https://code.claude.com/docs/en/slack.md; https://claude.com/docs/claude-tag/concepts/how-it-works.md | corrected / unreviewed |
| Cursor | Slack, GitHub app, Linear; Automations on cron, webhook, GitHub, GitLab, Bitbucket, Slack, Linear, PagerDuty, Sentry | GA | https://cursor.com/docs/integrations/slack; https://cursor.com/docs/cloud-agent/automations | unreviewed |
| DeepSeek harness | opt-in signed GitHub webhook on a second loopback listener (`127.0.0.1:3081`) starts a read-only review session; deliveries not stored, so duplicates create more sessions | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/github-review | reviewed |
| OpenHands | Cloud: GitHub label or `@openhands`, GitLab webhooks, Slack; HMAC-verified integration manager | GA | D:/openhands/usage/cloud/github-installation.md; D:/openhands/usage/cloud/slack-installation.md | unreviewed |
| OpenClaw | inbound webhooks, disabled by default: `POST /hooks/wake`, `/hooks/agent` (isolated turn, `Idempotency-Key`), mapped `/hooks/<name>`; dedicated token, query-string tokens rejected, 429 on repeated auth failures; content wrapped as untrusted but tool access not removed | GA | https://docs.openclaw.ai/automation/cron-jobs/webhooks | unreviewed |
| Hermes Agent | webhook-triggered runs and PR reviewer guides (guides not opened) | GA (guides) | /docs/llms.txt | unreviewed |
| Warp | Slack, Linear, Jira, GitHub, Teams, GitLab, Azure DevOps, custom webhooks as Factory work sources | GA (some early access) | https://docs.warp.dev/platform/integrations/slack/; https://docs.warp.dev/factories/ | unreviewed |
| Cline | `cline connect` for Telegram, Slack, Discord, Google Chat, WhatsApp, Linear: each message creates or continues a session; an allow/deny hook script gates actors; Telegram without flags approves everything; needs the hub; CLI only | GA (CLI) | https://docs.cline.bot/cli/connectors.md | reviewed |
| rizzy-harness (ours) | `POST /api/hooks/<trigger id>` with per-routine bearer, idempotency key, 64 KiB body, token buckets; triggers a routine; webhook chats get no memory, no `web_*`, no `remember`, a banner | works | `crates/master/src/http/hooks.rs:1-60`, `crates/master/src/lib.rs:257-297` | from own inventory |

### 2.3 Client protocols for remote control

#### Programmatic control protocol (RPC, HTTP API, SDK transport)
What it is: a documented protocol a separate process (UI, SDK, wrapper, remote client) uses to drive sessions: start, prompt, stream events, answer approvals, cancel.
Variants: stdio JSONL/JSON-RPC to a child process (Claude Code SDK, Pi RPC, DeepSeek harness `sdk`, ACP), long-lived server with HTTP and WebSocket (Codex app-server, OpenCode, OpenHands Agent Server, goose, OpenClaw, Hermes Agent API server), or a vendor REST API to cloud runs (Cursor, Warp). Approvals as server-initiated requests (Codex) versus control envelopes (Claude Code) versus UI request/response messages (Pi). Overload behaviour is documented only by Codex (bounded queues answer `-32001`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | app-server: JSON-RPC 2.0 over stdio JSONL (default), WebSocket or UDS; one `initialize` with capability negotiation and an `experimentalApi` gate; server-initiated approval and elicitation requests; version-matched generated schemas; bounded queues return `-32001`; one protocol for TUI, IDE, desktop and SDKs | experimental | https://learn.chatgpt.com/docs/app-server.md | reviewed |
| OpenCode | HTTP API with OpenAPI 3.1 (170 operations); `location` param scopes to a project; volatile `GET /api/event` plus durable per-session log; `@opencode/sdk` embeds the server in-process, `@opencode/client` is the network client | experimental (API), GA (SDK) | https://opencode.ai/docs/api/; https://opencode.ai/docs/build/sdk/ | reviewed / unreviewed |
| Claude Code | SDK spawns the CLI per session, NDJSON over stdin/stdout, `control_request`/`control_response` envelopes for permissions; `spawnClaudeCodeProcess` lets the host carry the stdio over any pipe (ssh, container exec); the subprocess never listens on the network | GA | https://code.claude.com/docs/en/agent-sdk/hosting.md; https://code.claude.com/docs/en/agent-sdk/typescript.md | unreviewed |
| DeepSeek harness | `dsh sdk`/`sdk-minimal` JSON-RPC over stdio; web profile uses Typert RPC plus a multiplexed WebSocket (`/api/remote.mux`) | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md; https://deepseek-harness.github.io/deepseek-harness/en/reference/api-gateway | unreviewed |
| Oh My Pi | rpc and rpc-ui modes; collab host executes everything, guests are remote views over a sealed relay | GA | https://omp.sh/docs/modes; https://omp.sh/docs/collab | unreviewed / reviewed (collab) |
| Pi | `pi --mode rpc`: JSONL on stdin/stdout; prompt, steer, follow_up, abort, session commands; `extension_ui_request/response`; ids allow out-of-order responses; orderly shutdown on stdin close | GA | https://pi.dev/docs/latest/rpc | unreviewed |
| Cursor | SDK with `local` or `cloud` runtime; SDK Bridge is a loopback Connect/protobuf server with a per-process bearer; Cloud Agents API v1 (SSE with `Last-Event-ID`, follow-up or `delivery: "steer"`) | unclear (SDK); beta (API) | https://cursor.com/docs/sdk/bridge; https://cursor.com/docs/cloud-agent/api/endpoints | unreviewed / reviewed |
| Antigravity | SDK launches a compiled binary through a Connection abstraction; `agy acp` | GA | https://antigravity.google/docs/cli/reference | unreviewed |
| OpenHands | Agent Server REST and WebSocket (FastAPI); same API for local and remote; `POST /command`, `POST /file/upload`; OpenAI-compatible endpoint where a model name is an agent profile | GA | D:/sdk/arch/agent-server.md; D:/sdk/arch/workspace.md | reviewed |
| OpenClaw | JSON over one WebSocket port (also Control UI and HTTP hooks); roles `operator` and `node`; scopes; TypeBox schemas | GA | https://docs.openclaw.ai/concepts/architecture | unreviewed |
| goose | everything is ACP: `goose acp` (stdio) and `goose serve` (HTTP `/acp` plus WebSocket, port 3284); W3C trace context passed to MCP calls | GA (no label) | https://goose-docs.ai/docs/gdk/acp/index.md | reviewed |
| Hermes Agent | OpenAI-compatible API server (`/v1/chat/completions`, `/v1/responses`, `/v1/runs` with SSE, approval, clarify), bearer `API_SERVER_KEY` required, `127.0.0.1:8642`, 10 concurrent runs; CLI attaches to the gateway with single-use 30 s tickets | GA | /user-guide/features/api-server | unreviewed |
| Zed/ACP | ACP v1: JSON-RPC 2.0 modelled on LSP, capability negotiation; v2 Draft adds out-of-turn updates and required message ids | GA (v1), draft (v2) | https://agentclientprotocol.com/get-started/architecture.md; https://agentclientprotocol.com/announcements/acp-v2-draft.md | reviewed |
| Warp | all clients use the same control-plane APIs; REST Platform API with published OpenAPI; Python and TypeScript SDKs | GA | https://docs.warp.dev/platform/architecture/ | reviewed |
| Cline | hub WebSocket: clients register capabilities, events fanned out to all attached clients | GA | https://docs.cline.bot/sdk/architecture/hub-spoke.md | corrected |
| rizzy-harness (ours) | master-node protocol `session.create/prompt/select_variant/set_agent/cancel/restore/delete`, `approval.respond`, `resume`, `artifact.*`, `PROTOCOL_VERSION=1`; master REST plus `/api/ws`; no OpenAPI spec or SDK | works | `crates/protocol/src/lib.rs:74-260`, `crates/master/src/http.rs:26-175` | from own inventory |

#### ACP (Agent Client Protocol) support
What it is: speaking the open editor-to-agent protocol, as an agent (served to editors) or as a client (hosting other agents).
Variants: almost all agent-side support is stdio only; goose is the only one shipping ACP over HTTP and WebSocket (ahead of the RFD); OpenHands, goose and Zed also host other agents as ACP clients. ACP itself puts file and terminal execution on the client.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | `opencode acp`: NDJSON JSON-RPC over stdio, starts a private server, one prompt per session at a time, MCP over ACP unsupported | GA | https://opencode.ai/docs/cli/acp/ | unreviewed |
| DeepSeek harness | `dsh acp` profile over stdio | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md | unreviewed |
| Oh My Pi | acp mode | GA | https://omp.sh/docs/modes | unreviewed |
| Cursor | `agent acp`: JSON-RPC 2.0 over stdio, auth method `cursor_login`, modes agent/plan/ask; used by JetBrains, Neovim, Zed | unclear (no label) | https://cursor.com/docs/cli/acp | unreviewed |
| Antigravity | `agy acp` server mode | GA | https://antigravity.google/docs/cli/reference | unreviewed |
| OpenHands | CLI `openhands acp`; Agent Server runs Claude Code, Codex, Gemini, Pi, OpenCode as ACP "backend agent kinds", credentials resolved server-side by secret name | GA | D:/sdk/guides/agent-server/overview.md; https://github.com/OpenHands/OpenHands-CLI | reviewed |
| goose | ACP server over stdio and HTTP/WebSocket; ACP providers drive Claude/Codex/Pi/Amp as backends | GA | https://goose-docs.ai/docs/gdk/acp/index.md; https://goose-docs.ai/docs/guides/zed.md | reviewed |
| OpenClaw | `openclaw acp` bridges IDEs (Zed documented) to a Gateway session over stdio; `/acp spawn` hosts Codex, Claude Code, Gemini CLI, OpenCode, Cursor, Droid through acpx; bridge lacks per-session MCP and client fs/terminal methods | GA / partial | https://docs.openclaw.ai/cli/acp; https://docs.openclaw.ai/concepts/agent-runtimes | unreviewed |
| Hermes Agent | `hermes acp` over stdio for VS Code, Zed, JetBrains, Buzz; curated toolset; 300 s approval timeout denies | GA | /user-guide/features/acp | unreviewed |
| Zed/ACP | Zed is the reference client; External Agents run as separate processes; remote transport (Streamable HTTP or WebSocket on one `/acp` endpoint) is an Active RFD, v1 recovery is `session/load` with no replay | GA (v1, stdio); planned (remote) | https://zed.dev/docs/ai/external-agents.md; https://agentclientprotocol.com/protocol/v1/transports.md; https://agentclientprotocol.com/rfds/streamable-http-websocket-transport.md | reviewed |
| Warp | changelog mentions "ACP agents" and "ACP-driven parent runs"; no ACP page read | unclear | https://docs.warp.dev/changelog/2026/ | unverified |
| Cline | an ACP mode exists (CLI auto-approve is false in ACP) | GA | https://docs.cline.bot/cli/cli-reference.md | reviewed |
| rizzy-harness (ours) | none; ACP is only a deferred idea in PLAN | docs-only mention | `PLAN.md:143-146` | from own inventory |

#### Remote attach of a client to an agent server on another machine
What it is: running the agent server on one machine (often a VM) and pointing a TUI, desktop or browser client at it.
Variants: the client dials the server in every case here, so the server must listen; auth ranges from none by default (OpenHands, Codex non-loopback during rollout) through a shared secret (goose) and password plus pairing (OpenCode) to a launch token (DeepSeek harness).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `codex --remote ws(s)://…\|unix://… --remote-auth-token-env VAR` against `codex app-server --listen`; a host-local background daemon on a Unix socket | experimental (remote), GA (daemon) | https://learn.chatgpt.com/docs/app-server.md; https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| OpenCode | `opencode serve --hostname --port --cors`; any client `--server <url>`; shared service discovered via `~/.local/state/opencode/service.json` | GA | https://opencode.ai/docs/cli/commands/; https://opencode.ai/docs/troubleshooting/ | reviewed |
| DeepSeek harness | browser to `dsh --profile web` behind a TLS reverse proxy | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments | reviewed |
| OpenHands | Canvas "Manage Backends" stores name, URL and key per backend; `--frontend-only`/`--backend-only`; VM, Docker, Helm, Modal, DigitalOcean backends | GA | D:/openhands/usage/agent-canvas/backends.md; D:/openhands/usage/agent-canvas/setup.md | reviewed |
| goose | `goose serve --host 0.0.0.0 --port N --tls` on a VM; Desktop Settings point at it | unclear | https://goose-docs.ai/docs/guides/remote-goose-server.md | reviewed |
| Hermes Agent | Desktop, dashboard and Webapp are clients of the per-profile gateway; Desktop can connect to many gateways | GA | /developer-guide/gateway-internals; /user-guide/desktop | unreviewed |
| Cline | CLI/Desktop can attach to a task already running in another Cline app; `remote` backend mode listed but unexplained | GA / unclear | https://docs.cline.bot/cli/cli-reference.md; https://github.com/cline/cline/releases | unreviewed |
| OpenClaw | operator clients and the Control UI connect to the Gateway WebSocket | GA | https://docs.openclaw.ai/concepts/architecture | unreviewed |
| rizzy-harness (ours) | the web UI and API on the master are the remote client; nodes are not reachable directly | works | `crates/master/src/http.rs:26-175` | from own inventory |

#### Relay-based remote control of a session from web or phone
What it is: a session keeps running on the user's machine while a browser or phone views and steers it through a hosted relay, without opening inbound ports.
Variants: vendor relay that stores the transcript while connected (Claude Code), reverse tunnel from the host (Antigravity), end-to-end encrypted tunnel (OpenCode OpenTunnel, Oh My Pi collab), chat platforms as the remote UI (Pi pi-chat, Hermes Agent gateway). Ours is a self-hosted relay by construction: the master is the relay.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `codex remote-control` daemon plus short-lived pairing codes; mobile drives Cloud or a paired host through a relay | GA (relay vendor-claim) | https://learn.chatgpt.com/docs/developer-commands.md; https://learn.chatgpt.com/docs/mobile.md | reviewed |
| OpenCode | `opencode pair --remote` through OpenTunnel, end-to-end encrypted; mobile app (iPhone TestFlight) starts, steers, approves | GA (pair), beta (mobile) | https://opencode.ai/docs/cli/web/; https://opencode.ai/product | reviewed |
| Claude Code | Remote Control: server mode up to 32 sessions (`--capacity`), `--spawn worktree`, QR; devices mirror the terminal; mobile push on "decision needed"; `remoteControlAtStartup` can be forced on only from user or managed settings; cross-machine messaging rides the Remote Control connection | GA | https://code.claude.com/docs/en/remote-control.md; https://code.claude.com/docs/en/cross-session-messaging.md | reviewed |
| Claude Desktop | "Connect new sessions" setting for Remote Control; Dispatch from phone to desktop; mobile app as thin client | GA / beta (Dispatch) | https://code.claude.com/docs/en/remote-control; https://support.claude.com/en/articles/13947068; https://code.claude.com/docs/en/mobile | corrected |
| Oh My Pi | collab: end-to-end sealed relay (AES-256-GCM), view-only links; no self-hosted relay | GA | https://omp.sh/docs/collab | reviewed |
| Pi | pi-chat bridges Discord and Telegram to a Pi session in a Gondolin micro-VM | unclear | https://github.com/earendil-works/pi-chat | unreviewed |
| Cursor | `/remote-control` lets a phone drive the computer (Cursor 3.9.8+, admin enable on Team/Enterprise) | GA (no label) | https://cursor.com/docs/agent/agents-window; https://cursor.com/docs/cloud-agent/mobile | unreviewed |
| Antigravity | host-initiated reverse tunnel to a Google web dashboard; `agy remote-control start` registers an OS service; approvals answerable from either end | GA (no label) | https://antigravity.google/docs/remote-control | reviewed |
| OpenClaw | "OpenClaw ID" relay with six-digit approval | experimental (Labs) | https://docs.openclaw.ai/gateway/remote | reviewed |
| Hermes Agent | messaging gateway (Telegram, Discord, Slack, Signal and many more) with delivery ledger, circuit breaker, watchdog; Relay keeps bot tokens off the gateway | GA / experimental (Relay) | /user-guide/messaging; /user-guide/messaging/relay | reviewed |
| Cline | Kanban web server reachable remotely only through Tailscale, SSH, ngrok or Cloudflare; no built-in auth | preview | https://docs.cline.bot/kanban/remote-access.md | corrected |
| OpenCodex | (Part B, RyensX) browser front-end for Codex Desktop over localhost, LAN or VPN; optional password, 12 h token | GA | https://github.com/RyensX/OpenCodex | reviewed |
| rizzy-harness (ours) | the master is the self-hosted relay; browser notifications and a "Waiting for you" list for approvals; responsive web, no native app or PWA | works / partial (mobile) | `web/src/lib/notify.ts:1-30`, `web/src/components/Attention.tsx` | from own inventory |

#### Pairing and authentication of remote clients and execution hosts
What it is: how a new device, client or worker proves who it is and gets a credential.
Variants: single-use short-lived codes or links (OpenCode 5 min, OpenCodex 5 min, ours 15 min, OpenClaw bootstrap 10 min); challenge signing with a device key (OpenClaw, ours); key exchange both ways (goose Roaming); shared static secret (goose `serve`, OpenHands, DeepSeek harness launch token); optional step-up device credentials (Claude Code Trusted Devices).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | short-lived pairing codes; app-server capability token (file or SHA-256 verifier) or signed bearer (issuer, audience, clock skew) | GA / experimental | https://learn.chatgpt.com/docs/developer-commands.md; https://learn.chatgpt.com/docs/app-server.md | reviewed |
| OpenCodex | hub: `ocx hub invite` single-use, rate-limited code with five-minute lifetime, entered on stdin; keys rotate with 10-minute overlap; Remote Workspace: one-time code, bearer hash stored | GA / experimental | https://opencodex.me/guides/remote-hub/; https://opencodex.me/guides/remote-workspace/ | corrected |
| OpenCode | single-use 5-minute link and QR, 30-day cookie, Basic auth user `opencode` | GA | https://opencode.ai/docs/cli/web/ | reviewed |
| Claude Code | Trusted Devices: enrolled device credential plus sign-in no older than 18 h, biometric or passkey step-up, revocable, off by default | beta | https://code.claude.com/docs/en/remote-control.md | reviewed (via Claude Desktop review) |
| Claude Desktop | Dispatch pairing: install both apps, open Cowork, select Dispatch, enable file access and keep-awake | beta | https://support.claude.com/en/articles/13947068 | corrected |
| DeepSeek harness | one process launch token plus cookie; Host/Origin fence; `http://` public root exposes the token in plaintext | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments | reviewed |
| OpenHands | `X-Session-API-Key`, rotation via `OH_SESSION_API_KEYS_0`, `_1`; no auth by default; anyone with the key can request execution | GA | D:/sdk/arch/agent-server.md; D:/openhands/usage/agent-canvas/backend-setup/remote.md | reviewed |
| OpenClaw | device identity signs `connect.challenge`; new devices need approval; device pairing requests expire in 5 min, capability approvals do not; SSH-verified auto-approval (default on), trusted CIDRs (off unless set); `oc-pair://` link with 10-minute bootstrap token; `openclaw devices rotate` | GA | https://docs.openclaw.ai/gateway/pairing | corrected |
| Cursor | worker auth by user login, personal key or one-hour sub-token (My Machines) or service-account key (pools), then a per-claim session token | unclear | https://cursor.com/docs/cloud-agent/self-hosted/pool | reviewed |
| goose | `serve`: shared secret, plaintext at rest, manual rotation, TLS fingerprint pin or TOFU; Roaming: mutual key acceptance, revoke closes connections | unclear / experimental | https://goose-docs.ai/docs/guides/remote-goose-server.md; https://goose-docs.ai/docs/guides/roaming-agents.md | reviewed |
| Hermes Agent | CLI to gateway: single-use 30 s tickets via a control socket; Relay: per-gateway secret and delivery key | GA / experimental | /developer-guide/gateway-internals; /user-guide/messaging/relay | reviewed |
| Warp | self-hosted worker API key (raw key shown once) | GA (Enterprise) | https://docs.warp.dev/factories/self-hosting/ | reviewed |
| Cline | hub: nothing documented; `cline hub dashboard` needs `--room-secret` for non-local hosts; SSH: key-only, pre-trusted host key | unclear / GA | https://docs.cline.bot/cli/cli-reference.md; https://docs.cline.bot/usage/cline-desktop-ssh.md | unreviewed / reviewed |
| rizzy-harness (ours) | admin creates a single-use 15-minute join token (stored hashed); `node join <wss://...> --fingerprint ...`; Ed25519 node key signs a nonce bound to the master cert fingerprint; revoke drops the link; UI uses server-side cookie sessions | works | `crates/master/src/nodes.rs:16`, `:38-80`, `crates/cli/src/main.rs:102-118`, `crates/link/src/lib.rs:43-49` | from own inventory |

#### Exposure beyond loopback (bind, TLS, tunnels)
What it is: how a server or gateway is made reachable from other machines, and what protects it when it is.
Variants: refuse wildcard binds and require a reverse proxy (DeepSeek harness), require auth for non-loopback and `wss://` for public (OpenClaw), recommend tunnels and provide none (Cline Kanban), or accept unauthenticated non-loopback connections (Codex during rollout). TLS fingerprint pinning appears in OpenClaw, goose and ours.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | plain ws only on localhost or via SSH tunnel; non-loopback listeners unauthenticated by default during rollout | experimental | https://learn.chatgpt.com/docs/app-server.md | reviewed |
| OpenCodex | hub data listener plain HTTP (operator must add TLS); management listener on 127.0.0.1 meant for Tailscale Serve; unauthenticated loopback companion listener lets any local user use the hub's credentials | GA | https://opencodex.me/guides/remote-hub/ | corrected |
| OpenCode | localhost by default (port 49374, 28894 on Windows); SSH port-forward or OpenTunnel | GA | https://opencode.ai/docs/cli/web/ | reviewed |
| DeepSeek harness | loopback by default; `--host` must be one concrete IP (`0.0.0.0` rejected), warns on non-loopback plain HTTP; TLS never falls back to HTTP; `--trusted-host`; reverse proxy must pass Host and WebSocket upgrades | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments | reviewed |
| Oh My Pi | auth broker on 127.0.0.1:8765, plain HTTP, bearer; docs advise private network or TLS proxy | GA | https://omp.sh/docs/secrets | corrected |
| OpenHands | loopback by default; `--host 0.0.0.0`/`OH_BIND_HOST`; key required with `--public`; Docker runtime mode binds all interfaces | GA | D:/openhands/usage/agent-canvas/setup.md | reviewed |
| OpenClaw | Tailscale Serve, LAN/tailnet WebSocket, SSH tunnel, Cloudflare Tunnel plus Access; non-loopback binds require auth; public must be `wss://`; TLS fingerprint pinning; a headerless SSH forward looks like local access; container images default to an exposed bind | GA | https://docs.openclaw.ai/gateway/remote; https://docs.openclaw.ai/gateway/security | reviewed |
| goose | `serve` default `127.0.0.1:3284`; `--tls` self-signed with fingerprint; `--dangerously-unauthenticated` for local only | unclear | https://goose-docs.ai/docs/guides/goose-cli-commands.md; https://goose-docs.ai/docs/guides/remote-goose-server.md | reviewed |
| Hermes Agent | non-loopback dashboard fails closed without auth; Docker image refuses a root gateway | GA | /user-guide/docker | unreviewed |
| Cline | Kanban on `127.0.0.1:3484`, `--host 0.0.0.0` exposes it with no auth; docs warn it gives full git and terminal access | preview | https://docs.cline.bot/kanban/remote-access.md | corrected |
| rizzy-harness (ours) | default `127.0.0.1:7777`; non-loopback needs `--insecure-bind` or a WireGuard address; `--tls` self-signed UI cert; `--allowed-host` for proxies; node listener off unless `--node-listen` | works | `crates/cli/src/main.rs:33-70`, `crates/cli/src/net.rs` | from own inventory |

#### Reconnect, resume and takeover after link loss
What it is: what happens to a running session when the link between control and execution side drops, or one side restarts.
Variants: vendor-side transcript lets devices resync (Claude Code Remote Control); leases and requeue onto another worker (Claude Code runner, Cursor); durable sequence-numbered log for catch-up (OpenCode, ours); fencing of stale workers (OpenClaw); no reconnect at all (DeepSeek harness SSH, Cline SSH after 45 s).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | Remote Workspace: a disconnected executor never falls back to the hub filesystem; reconnect behaviour not described | experimental | https://opencodex.me/guides/remote-workspace/ | corrected |
| OpenCode | durable per-session log after a sequence number with `follow=true`; volatile broadcast overflows slow consumers; client library has no replay or auto-reconnect | GA (log route experimental) | https://opencode.ai/docs/api/; https://opencode.ai/docs/build/client/ | reviewed |
| Claude Code | Remote Control: about 4 h resume window, `--continue`/`--session-id`, "another connection took over" as a distinct reason, server mode gives up after about 10 min outage, crashed sessions re-served on next message; runner: lease lapses after about 60 s, poll timeout 10 s, backoff to 20 s, resume on a fresh runner loses unpushed work (`--push-outcome-on-release` best-effort push) | GA / beta | https://code.claude.com/docs/en/remote-control.md; https://code.claude.com/docs/en/self-hosted-environments.md; https://code.claude.com/docs/en/self-hosted-environments-deploy.md | reviewed |
| Claude Desktop | SSH: remote login renewed only while Desktop is connected | GA | https://code.claude.com/docs/en/desktop | corrected |
| DeepSeek harness | web: `follow()` plus `page()` fill gaps, reconnect replaces the window from a snapshot; SSH backend: loss is final | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/ssh.md | corrected |
| Pi | pi-durable commits before display and resumes after crash with a restart limit | experimental | https://github.com/earendil-works/pi/blob/main/packages/durable/README.md | reviewed |
| Cursor | worker reconnects after 90 s of silence; `claimed_offline` hibernation with a wake timeout; after claim all activity for a chat goes to the same worker | unclear | https://cursor.com/docs/cloud-agent/self-hosted/pool | reviewed |
| OpenClaw | cloud workers: credential rotation plus owner-epoch fencing rejects stale workers; durable "active writer claim" checked in the transcript-write transaction | beta / GA | https://docs.openclaw.ai/gateway/cloud-workers/security-model | corrected / reviewed |
| goose | `session/load` resumes; Roaming: multiple clients cannot co-drive a live session | GA / experimental | https://goose-docs.ai/docs/gdk/acp/index.md | reviewed |
| Hermes Agent | gateway delivery ledger (replies stored before send, 3 attempts in 24 h); crashed turns marked "unknown" and never replayed; Relay connector buffers while offline | GA / experimental | /user-guide/messaging; /user-guide/messaging/relay | reviewed |
| Zed/ACP | Zed remote dev reconnect keeps unsaved edits; ACP v1 recovers with `session/load` only, stream resumption planned for v2 | GA / planned | https://zed.dev/docs/remote-development.md; https://agentclientprotocol.com/rfds/streamable-http-websocket-transport.md | unreviewed / reviewed |
| Cline | SSH tunnel closes after about 45 s of network loss; hub sessions outlive clients | GA | https://docs.cline.bot/usage/cline-desktop-ssh.md | reviewed |
| rizzy-harness (ours) | link reconnect replays events by `Resume{after}`; restart mid-turn closes the turn as `interrupted` and denies pending approvals; master rejects events for sessions another node owns | works | `crates/node/src/lib.rs:323-330`, `:590-624`, `crates/node/src/session.rs:154-191` | from own inventory |

### 2.4 Distributed execution

#### SSH remote execution
What it is: using SSH to put work on another machine, either the whole agent (helper installed on the host) or only the tools.
Variants: whole agent on the host with the UI local (Codex app-server over SSH, Claude Desktop helper, Cline helper hub, Zed headless server); tools only on the host with the loop local (DeepSeek harness, Pi pi-env, Hermes Agent, Oh My Pi `ssh://` paths). Hardening differs: Pi pi-env and DeepSeek harness verify the uploaded helper by SHA-256; Cline and Pi require pre-trusted host keys; Claude Desktop disables forwarding. Who holds the model key also differs: Cline sends it per session; Claude Desktop's remote login lapses when Desktop disconnects.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Desktop reads `~/.ssh/config` host aliases and starts the remote app-server over SSH in the login shell; chats run on the remote filesystem and shell | GA | https://learn.chatgpt.com/docs/remote-connections.md | unreviewed |
| OpenCodex | v2.100.0 release notes mention "zero-preinstall SSH setup" and an `ocx-child` relay; relation to Remote Workspace unknown | unclear | https://github.com/lidge-jun/opencodex/releases.atom | unverified |
| OpenCode | SSH port-forward recommended to reach a localhost-only server | GA | https://opencode.ai/docs/cli/web/ | unreviewed |
| Claude Desktop | helper and Claude Code installed under `~/.claude/remote/` (about 1 GB, not on noexec); forwarding disabled; admin keys `sshConfigs`, `sshHostCommands`, `sshHostAllowlist`, `disableSshSavedPasswords`; Windows hosts from v2.31226.0 | GA (some beta flags) | https://code.claude.com/docs/en/desktop | corrected |
| DeepSeek harness | `ctx.ssh` plus `fs-ssh`, `subprocess-ssh`, `sandbox-ssh`; harness, model transport and session storage stay on the host; helper hash-checked | no maturity label | https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/ssh.md | corrected |
| Oh My Pi | `read`/write/search on `ssh://` paths of saved POSIX hosts (remote read cap 1 MiB); whether `bash` runs remotely is not documented | GA | https://omp.sh/docs/files; https://omp.sh/docs/cli | unreviewed |
| Pi | pi-env reaches a Rust daemon over SSH (BatchMode, strict host-key checking, no forwarding, SHA-256-named daemon); core CLI has an `ssh.ts` example extension | unclear | https://github.com/earendil-works/pi/blob/main/packages/env/README.md; https://pi.dev/docs/latest/extensions | reviewed |
| Antigravity | CLI "runs in ... SSH sessions"; sign-in prints a URL; `agy mic-serve` forwards the microphone | unclear | https://antigravity.google/docs/overview | unreviewed |
| OpenClaw | headless node host can run through an SSH tunnel; SSH-verified node auto-approval | GA | https://docs.openclaw.ai/nodes; https://docs.openclaw.ai/gateway/pairing | unreviewed / corrected |
| Hermes Agent | `ssh` terminal backend; docs recommend it because the agent "can't modify its own code" | GA | /user-guide/features/tools | reviewed |
| Zed/ACP | Zed remote development: local `ssh` with ControlMaster per project, `~/.zed_server` binary; 1.25.2 fixed secrets in env vars of external agents being visible in the local process list | GA | https://zed.dev/docs/remote-development.md; https://zed.dev/releases/stable | unreviewed |
| Cline | Desktop uploads a ~30 MB helper to `~/.cline/remote/`, starts a loopback hub, reaches it via `ssh -L`; key-only, one host, strict host keys; ProxyJump in SDK v0.2.0 | GA | https://docs.cline.bot/usage/cline-desktop-ssh.md; https://github.com/cline/cline/releases | reviewed |
| rizzy-harness (ours) | none: SSH node setup was cut (`1fa8f27`); nodes are installed by hand and join over the TLS link | none (by decision) | `crates/cli/src/main.rs:84-100` | from own inventory |

#### Pluggable execution backend (tools run elsewhere than the loop)
What it is: an interface inside the agent through which every file and shell operation goes, so the same loop can execute locally, in a container, over SSH or in a cloud sandbox.
Variants: one environment object for all tools (Hermes Agent, DeepSeek harness capability seams), per-tool pluggable operations (Pi), workspace classes (OpenHands), replacing the process transport of the whole agent (Claude Code SDK), or the client doing the execution (ACP).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | MCP servers can run "on a remote executor" via `environment_id`; `env_vars` with `source = "remote"` read the executor's environment; no design page | unclear | https://learn.chatgpt.com/docs/extend/mcp.md | unverified |
| Claude Code | `spawnClaudeCodeProcess(SpawnOptions)` lets the host run the CLI in a VM, container or remote machine over any byte pipe | GA | https://code.claude.com/docs/en/agent-sdk/typescript.md | unreviewed |
| DeepSeek harness | capability seams: swapping one provider (e.g. SSH sandbox) changes bash, PTY and LSP together | unclear | https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams | corrected |
| Pi | built-in tools take pluggable operations (`ssh.ts` example), `gondolin/` sandbox example | GA (hooks) | https://pi.dev/docs/latest/extensions | unreviewed |
| OpenHands | `LocalWorkspace`, `RemoteWorkspace` (HTTP), `DockerWorkspace`, `RemoteAPIWorkspace`; same `Conversation` API for local and remote; Runtime API "designed primarily for benchmark evaluation" | GA | D:/sdk/arch/workspace.md; D:/sdk/arch/conversation.md | unreviewed |
| OpenClaw | exec can be routed to a node; tools run in-process on the Gateway, in a sandbox, or on a node | GA | https://docs.openclaw.ai/nodes | unreviewed |
| Hermes Agent | `terminal.backend` (`local`, `docker`, `ssh`, `modal`, `daytona`, `vercel_sandbox`, plugins); every file and shell tool calls the environment's `execute()`; plugin interface with `is_remote`, `is_container`, `strip_env_keys` | GA | /user-guide/features/tools; /developer-guide/terminal-environment-plugin | reviewed / unreviewed |
| Zed/ACP | `fs/*` and `terminal/*` executed by the client, which must apply its own policy; v2 RFD reworks them as a client "execution surface" | GA | https://agentclientprotocol.com/protocol/v1/terminals.md | reviewed |
| rizzy-harness (ours) | none as an interface: the node is the execution environment; sub-agent worktrees on the same node | none | `crates/node/src/session.rs:765-856` | from own inventory |

#### Self-hosted workers for a hosted control plane
What it is: the vendor (or a self-hosted gateway) keeps the control plane and UI; a customer-run worker process executes sessions on customer machines.
Variants: loop on the worker (Claude Code, Warp, OpenClaw `worker-turn`) versus loop at the vendor (Cursor). Runner backends: spawn child processes (Claude Code), Docker/Kubernetes/Direct/Command (Warp), one agent per worker (Cursor pools). On-demand spawning via an orchestrator hook (Claude Code) or controller (Cursor).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `claude self-hosted-runner` (environment = pool `ccpool_...`, runner, session as child process); orchestrator mode runs a `spawn-runner` hook per queued session (exit 0 submitted, 1 retryable, ≥2 not); `--capacity` default advice 1 but recipes use 4; surfaces: web, mobile, desktop, scheduled tasks, `--cloud`, `--environment` dispatch, Claude Tag | beta (public beta, Team/Enterprise) | https://code.claude.com/docs/en/self-hosted-environments.md; https://code.claude.com/docs/en/self-hosted-environments-configuration.md; https://code.claude.com/docs/en/self-hosted-environments-deploy.md | reviewed |
| Claude Desktop | Team/Enterprise can route cloud sessions to self-hosted runners | GA (as stated; runner pages not opened by that survey) | https://code.claude.com/docs/en/platforms | unreviewed |
| Cursor | `agent worker start`: My Machines (personal) and Team Pools (Enterprise, service account, controller autoscaling); one agent per worker in pools; up to 20 `--worker-dir`; worker limits contradict across pages (10,000 vs 200/1,000) | unclear (no label) | https://cursor.com/docs/cloud-agent/self-hosted; https://cursor.com/docs/cloud-agent/self-hosted/pool; https://cursor.com/docs/cloud-agent/self-hosted/my-machines | reviewed |
| OpenClaw | Crabbox cloud workers: disposable machines, placement from repo URL, managed worktree or empty workspace; requires Crabbox 0.82.0+ | beta | https://docs.openclaw.ai/gateway/cloud-workers | corrected |
| Warp | `oz-agent-worker`: Docker (default), Kubernetes, Direct, Command (hand off to an external job API or queue), one-shot Direct; unmanaged mode is just `oz agent run` in CI | GA (Enterprise) | https://docs.warp.dev/factories/self-hosting/ | reviewed |
| rizzy-harness (ours) | the node is a self-hosted worker for a self-hosted master; no vendor control plane | works | `crates/cli/src/main.rs:213-300`, `:500-540` | from own inventory |

#### Work claim, lease and heartbeat
What it is: how queued sessions get assigned to workers, and how the control side notices a dead worker.
Variants: pull with a lease refreshed by each poll (Claude Code), claim-then-spawn with a single-claim session token (Cursor), fencing by owner epoch (OpenClaw). Ours has no queue: the user picks a node per chat.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | runner with free capacity claims a session and holds a lease; each poll refreshes the lease and acts as heartbeat; lease lapses after about 60 s; malformed poll responses (captive portals) rejected rather than read as empty; orchestrator spawn requests carry a reason (`session_created`, `runner_lost`, `requeued`, `prewarm`, …) and an idempotency order ID | beta | https://code.claude.com/docs/en/self-hosted-environments.md; https://code.claude.com/docs/en/self-hosted-environments-configuration.md | reviewed |
| Cursor | requests wait in a named pool; a controller claims each (API with SSE watch), spawns a machine, the worker connects with a single-claim token; after claim all chat activity goes to that worker; a wrong-repo request is rejected | unclear | https://cursor.com/docs/cloud-agent/self-hosted/pool | reviewed |
| OpenClaw | owner-epoch fencing rejects stale workers; suggested lease 4 h total / 45 min idle (Crabbox max 4 h, policy up to 24 h) | beta | https://docs.openclaw.ai/gateway/cloud-workers; https://docs.openclaw.ai/gateway/cloud-workers/security-model | corrected |
| Warp | worker addressed by worker ID (`--host`); runs have a maximum runtime (`infrastructure_timeout`) | GA (Enterprise) | https://docs.warp.dev/factories/self-hosting/; https://docs.warp.dev/platform/architecture/ | reviewed / unreviewed |
| rizzy-harness (ours) | none: node chosen per chat or per project default; ping 20 s / idle 60 s on the link | works (link keepalive only) | `crates/master/src/lib.rs:557-600`, `crates/link/src/lib.rs:43-49` | from own inventory |

#### Execution-side enrolment and per-dispatch credentials
What it is: the long-lived credential a worker enrols with, and any shorter credential minted per session or dispatch so a leaked token is worth little.
Variants: one long-lived worker key only (ours, Warp worker key, goose shared secret); a long-lived secret kept on an orchestrator plus single-use work orders (Claude Code); per-claim session tokens (Cursor); per-dispatch credential stored only as a hash (OpenClaw); session identity JWTs that internal services can verify (Claude Code).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | hub client keys rotate with a 10-minute overlap; revocation is a dashboard action (disconnect is local only); model scope `allowedModels` | GA | https://opencodex.me/guides/remote-hub/ | corrected |
| Claude Code | environment secret shown once, 365 days, registers runners; orchestrator keeps it, each spawned runner gets a single-use work-order JWT registering exactly one runner, expiring with the spawn lease (default 120 s); every session gets `CLAUDE_CODE_SESSION_ACCESS_TOKEN` (ES256 JWT, 4 h default, public JWKS, RFC 8693 `act` chain); all token kinds signed by one JWKS, distinguished by `ccr:role`; no revocation feed | beta | https://code.claude.com/docs/en/self-hosted-environments-configuration.md; https://code.claude.com/docs/en/self-hosted-environments-identity.md; https://code.claude.com/docs/en/self-hosted-environments-quickstart.md | corrected (work order), unreviewed (JWT) |
| Cursor | per-claim session tokens (one claim, one worker ID, 7-day expiry backstop) keep the service-account key on the controller; nonce-bound 5-minute identity JWTs for spawn hooks, separate issuer | unclear | https://cursor.com/docs/cloud-agent/self-hosted/pool | reviewed |
| OpenClaw | nodes: device token after approval, rotation; cloud workers: new credential per dispatch, Gateway stores only its hash | GA / beta | https://docs.openclaw.ai/gateway/pairing; https://docs.openclaw.ai/gateway/cloud-workers/security-model | corrected |
| Warp | self-hosted worker API key; control plane sends scoped runtime credentials per task | GA (Enterprise) | https://docs.warp.dev/factories/self-hosting/; https://docs.warp.dev/platform/architecture/ | reviewed |
| rizzy-harness (ours) | long-lived Ed25519 node key after a single-use join token; no per-session or per-dispatch credential | works | `crates/master/src/nodes.rs:16-80`, `crates/link/src/lib.rs:43-49` | from own inventory |

#### Execution-side authority over policy
What it is: whether the machine that runs tools has a local policy that the control side cannot widen.
Variants: control side sends policy and the worker adds a floor (Claude Code runner); control side issues tool calls and the worker relies on OS user and hooks (Cursor); worker must declare what it can do and the control side approves (OpenClaw); executor-side read-only default (OpenCodex); local file is the ceiling (ours, and in a single-machine sense Codex's `requirements.toml`). Client-supplied limits not capped (OpenHands) is the anti-pattern.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `requirements.toml` outranks all config; project config cannot set provider, credentials, `notify`, `otel`; subagents cannot widen permissions (single host, not a split) | GA | https://learn.chatgpt.com/docs/config-file/config-basic.md | reviewed |
| OpenCodex | Remote Workspace executor: one approved root, read-only default, write only after a sandbox probe, no fallback to the hub filesystem | experimental | https://opencodex.me/guides/remote-workspace/ | corrected |
| Claude Code | control plane sends each session's tool list and permission rules with the work payload (plus model, connectors, hooks); runner image `managed-settings.json` also applies and on Bedrock/Agent Platform runners is the only enforcement; `--confine-repo-settings warn\|enforce\|off` refuses sessions whose repo grants reach outside the workspace; `--trust-workspace false`; docs say the product "can't verify or enforce" egress restriction | beta | https://code.claude.com/docs/en/self-hosted-environments-configuration.md; https://code.claude.com/docs/en/self-hosted-environments-deploy.md | corrected |
| Cursor | worker applies `.cursor/hooks.json` and team/enterprise hooks; dashboard secrets only with `--sync-dashboard-secrets` plus admin enablement; but the server drives tool calls, limited by the OS user | unclear | https://cursor.com/docs/cloud-agent/self-hosted/my-machines | reviewed |
| OpenHands | backend scopes settings, secrets and MCP; client-supplied per-conversation container limits are not capped by server env | GA | `docker-execution.md` under D:/openhands/usage/agent-canvas (path abbreviated in the survey) | reviewed |
| OpenClaw | node command runs only if declared by the node, in its approved set, and in the Gateway's per-platform allowlist; sensitive commands need persistent opt-in in `gateway.nodes.commands.allow`; `deny` overrides; authority sits on the Gateway | GA | https://docs.openclaw.ai/nodes/command-policy | reviewed |
| Warp | control plane pushes resolved config; admin denylist is non-bypassable | GA (Enterprise) | https://docs.warp.dev/platform/architecture/ | reviewed |
| Cline | SSH remote: approvals stay on the local Desktop (control side) | GA | https://docs.cline.bot/usage/cline-desktop-ssh.md | reviewed |
| rizzy-harness (ours) | `node.toml` is the only authority, unknown keys rejected, no HTTP or protocol endpoint writes it; approval mode `min(requested, node max)`; `denied_tools`, `workspace_roots`, `env_pass`; hostile-node tests | works | `crates/node/src/policy.rs:9-126`, `crates/core/src/policy.rs:19-23`, `crates/master/tests/hostile_node.rs` | from own inventory |

#### Central credential broker or inference proxy
What it is: one host holds provider credentials so execution machines or less-trusted clients can call models without holding long-lived keys.
Variants: hand out short-lived access tokens (Oh My Pi broker), proxy requests so clients never see keys (Oh My Pi gateway, OpenCodex hub, OpenClaw Gateway inference proxy, Claude Code apps gateway), or send the key per session over the tunnel (Cline SSH). The operator of a proxy can read prompts and responses (stated by Oh My Pi).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | Remote hub: one host holds provider credentials, catalog and usage; clients keep a per-client data key; `allowedModels` scope, 403 outside it; single-tenant host only | GA | https://opencodex.me/guides/remote-hub/ | corrected |
| Claude Code | `claude gateway --config gateway.yaml`: stateless server on PostgreSQL, device-flow sign-in, OIDC upstream, managed settings per IdP group, failover across Bedrock/Vertex/Foundry; disables Remote Control | GA | https://code.claude.com/docs/en/claude-apps-gateway.md; https://code.claude.com/docs/en/claude-apps-gateway-deploy.md | reviewed |
| Oh My Pi | `omp auth-broker` (127.0.0.1:8765, bearer, plain HTTP; clients get API keys and access tokens, refresh tokens stay on the broker); `omp auth-gateway` forward proxy (127.0.0.1:4000) so clients never receive credentials; operator can see content | GA | https://omp.sh/docs/secrets | corrected |
| OpenClaw | cloud workers: provider keys stay on the Gateway, inference proxied by model reference by default | beta | https://docs.openclaw.ai/gateway/cloud-workers | corrected |
| Hermes Agent | Relay keeps messaging-platform bot tokens off the gateway (not model keys) | experimental | /user-guide/messaging/relay | reviewed |
| Cline | SSH: provider key sent to the host over the tunnel for the session, not written to host settings | GA | https://docs.cline.bot/usage/cline-desktop-ssh.md | reviewed |
| Warp | Warp Agent model traffic routes through Warp unless team-managed keys or BYO LLM | GA | https://docs.warp.dev/platform/architecture/ | reviewed |
| rizzy-harness (ours) | none: each node holds its own provider key in `config.toml` and calls the provider directly; child processes have provider key vars stripped | works (by design) | `crates/cli/src/main.rs:213-300`, `crates/tools/src/env.rs:1-135` | from own inventory |

#### Multi-node fleet under one control side
What it is: one control side driving several execution machines, with per-machine selection.
Variants: client-side federation of independent backends (OpenHands Canvas), one gateway with many peripheral nodes (OpenClaw), multiplexed profiles and multi-gateway clients (Hermes Agent), pools of workers (Claude Code, Cursor, Warp). Most single-machine servers document no fleet (OpenCode: one server per machine; Cline: one hub per machine).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | nothing documented: one server per machine, clients attach | - | https://opencode.ai/docs/build/ | unreviewed |
| Claude Code | many runners per environment; minimum fleet equals concurrent active owners because of the one-owner lock | beta | https://code.claude.com/docs/en/self-hosted-environments-deploy.md | reviewed |
| Cursor | named pools; repo-backed or any-repo pools (`--clone-git-repos`) | unclear | https://cursor.com/docs/cloud-agent/self-hosted/pool | reviewed |
| OpenHands | Canvas stores any number of backends; settings, LLM config, MCP, secrets and automations scoped per backend; "a client-side federation, not a master that owns state" | GA | D:/openhands/usage/agent-canvas/backends.md | reviewed |
| OpenClaw | one Gateway per host; many nodes declare capabilities; macOS app is a single node | GA | https://docs.openclaw.ai/concepts/architecture; https://docs.openclaw.ai/nodes | unreviewed |
| Hermes Agent | `gateway.multiplex_profiles: true` serves all profiles from one process, each with its own state DB and secret scope; Desktop connects to many gateways; a kanban board can span gateways | GA | /developer-guide/gateway-internals; /user-guide/desktop | unreviewed |
| Warp | workers addressed by ID; per-plan agent limits | GA (Enterprise) | https://docs.warp.dev/factories/self-hosting/ | reviewed |
| Cline | one hub per machine shared by CLI sessions, connectors and schedules | GA | https://docs.cline.bot/cli/cli-reference.md | unreviewed |
| rizzy-harness (ours) | multiple nodes; node picker per chat, default node per project, per-node `node.toml` | works | `crates/master/src/lib.rs:557-600`, `crates/master/src/projects.rs` | from own inventory |

#### Execution-side health, drain and metrics
What it is: endpoints and flags an operator uses to know a worker is alive, take it out of rotation, and retire it cleanly.
Variants: liveness and readiness endpoints (OpenCodex, Codex, Cursor), Prometheus metrics with alert rules (Claude Code), OpenTelemetry metrics (Warp), drain, retire and hold flags (Claude Code), the agent itself as diagnostician (Claude Code `doctor`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | app-server `/readyz` and `/healthz` | experimental | https://learn.chatgpt.com/docs/app-server.md | reviewed |
| OpenCodex | `ocx health`/`ready`/`status`/`doctor`; `GET /healthz`, `/readyz` (503 with Retry-After while pending); drain mode returns 503 Retry-After 5 | GA | https://opencodex.me/reference/architecture/ | unreviewed |
| Claude Code | runner `/healthz` (liveness only, `last_poll_age_ms`) and `/metrics` on `--health-port`; orchestrator health with queue counts; sample alert rules; `--drain-wait-sec`, `--retire-at`, `--defer-shutdown-max-min`, `--hold-idle-file`; `self-hosted-runner doctor` starts a Claude session with the runner's logs | beta | https://code.claude.com/docs/en/self-hosted-environments-reference.md; https://code.claude.com/docs/en/self-hosted-environments-deploy.md | unreviewed / reviewed |
| Cursor | worker health endpoint `/readyz` and a management address | unclear | https://cursor.com/docs/cloud-agent/self-hosted | reviewed |
| OpenHands | deferred-init pods answer `/health` while `/api/*` returns 503 | GA | D:/sdk/guides/agent-server/deferred-init.md | reviewed |
| Warp | workers can export OpenTelemetry metrics | GA (Enterprise) | https://docs.warp.dev/factories/self-hosting/ | reviewed |
| rizzy-harness (ours) | `rizzy-harness doctor` (bash/git present, config and `node.toml` parse, token file mode); no metrics or health endpoint; no drain | partial | `crates/cli/src/main.rs:557-632` | from own inventory |

#### Containerised deployment of the execution side
What it is: shipping the worker or server as a container image or Kubernetes recipe.
Variants: official images (OpenCode, OpenClaw, Hermes Agent), recipes but no image (Claude Code runner), Helm single-replica (OpenHands), Docker as a tool backend rather than the deployment (Hermes Agent, Warp). Ours deliberately ships only a master image.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | Docker images `ghcr.io/anomalyco/opencode:<ver>` | GA | https://opencode.ai/docs/ | unreviewed |
| Claude Code | no pre-built runner image; Dockerfile example, Kubernetes Deployment (3 replicas, capacity 4, 4 GiB) and Compose (evaluation only); session children share the runner's UID | beta | https://code.claude.com/docs/en/self-hosted-environments-deploy.md | reviewed |
| OpenHands | Docker, per-conversation Docker, Helm (single-replica StatefulSet, "unauthenticated, single-tenant"), Modal, DigitalOcean | GA / unlabelled | D:/openhands/usage/agent-canvas/backends.md | reviewed |
| OpenClaw | install script, npm, Docker, Nix, Podman, Kubernetes; container images default to an exposed bind | GA | https://docs.openclaw.ai/install/docker; https://docs.openclaw.ai/gateway/security | unreviewed |
| Hermes Agent | official image (amd64, arm64), s6-overlay, non-root UID 10000, read-only install tree; never two gateway containers on one data dir | GA | /user-guide/docker | unreviewed |
| Warp | worker backends Docker (default) and Kubernetes | GA (Enterprise) | https://docs.warp.dev/factories/self-hosting/ | reviewed |
| rizzy-harness (ours) | master image only; nodes cannot be containerised by design; compose with host networking | works | `Dockerfile:1-25`, `compose.yaml` | from own inventory |

### 2.5 Cloud agents and their environments

#### Vendor-hosted cloud agents
What it is: agent sessions that run on vendor infrastructure in an isolated VM or container with a clone of the repo, and keep running when the user's machine is off.
Variants: per-task workspace (Codex), per-session VM (Claude Code, Cursor Firecracker), Google-managed environment (Antigravity), per-session sandbox that pauses when idle (Cline). Limits and plan gating differ widely.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Codex Cloud: own workspace per task, runs while the computer sleeps; Legacy two-phase runtime (setup with network and secrets, agent phase offline) still used for Code Review and integrations | GA; Legacy deprecated (planned) | https://learn.chatgpt.com/docs/cloud.md; https://learn.chatgpt.com/docs/agent-approvals-security.md | reviewed / unreviewed |
| Claude Code | cloud sessions on Anthropic VMs (Ubuntu 24.04 x86_64, about 4 vCPU, 16 GB, 30 GB); clone, setup script, network level, work, push a branch; session stays open after push; no shell access into the VM | GA | https://code.claude.com/docs/en/web-quickstart.md; https://code.claude.com/docs/en/cloud-environments.md | reviewed |
| Claude Desktop | same cloud sessions started from the Desktop environment selector; not with ZDR, HIPAA or CMEK | GA | https://code.claude.com/docs/en/claude-code-on-the-web | unreviewed |
| Cursor | Cloud Agents in isolated VMs, many in parallel, push a branch and PR; started from desktop, iOS, web, Slack, Linear, GitHub/Bitbucket mentions, API; Free and Plus excluded | GA | https://cursor.com/docs/cloud-agent | reviewed |
| Antigravity | Remote Agents in a Google-managed environment; persist offline; Turbo preset (always proceed), no plan/auto modes; personal AI Pro/Ultra only; no print mode, local MCP, plugins or skills | GA, rolling out | https://antigravity.google/docs/remote-agents | unreviewed |
| OpenHands | OpenHands Cloud: a control plane creates the sandbox that hosts the Agent Server; browser talks directly to the sandbox (enterprise architecture page) | GA | D:/openhands/usage/agent-canvas/backends.md (the enterprise architecture page is cited by name only) | unreviewed |
| Cline | Cloud Agents at app.cline.bot: per-session sandbox, `cline/` branch, PR back; pauses after 15 min idle; 10 concurrent, 50 new per day; rules, skills, MCP and connectors not loaded yet | preview (rolling out) | https://docs.cline.bot/usage/cloud-agents.md | reviewed |
| Warp | cloud runs: trigger, config resolution, provisioning, agent loop, outputs, teardown with optional snapshot; `--harness oz\|claude\|codex` | GA | https://docs.warp.dev/platform/architecture/; https://docs.warp.dev/agents/cli/oz-cli/ | unreviewed |
| rizzy-harness (ours) | none (by design: no vendor cloud; the node is the user's machine) | none | `README.md` "Remote worker nodes" | from own inventory |

#### Cloud environment definition and setup caching
What it is: a saved description of the machine a cloud agent starts in (image, repos, setup script, env vars, network) and a cache of the prepared filesystem.
Variants: agent-led setup that records an install script and start skill (Codex), setup script snapshotted for about 7 days (Claude Code), Build captures disk not processes (Cursor), Docker image plus setup that reruns fresh each time (Warp).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | reusable environments: Codex inspects repos, installs and tests, records an install script and a start skill; publishing captures the prepared filesystem; background refresh with caches | GA | https://learn.chatgpt.com/docs/environments/cloud-environments.md | reviewed |
| Claude Code | environment = network level, env vars, network secrets, setup script (root, Ubuntu 24.04, up to 64 KB, about 5 min); filesystem snapshot reused, rebuilt when script or hosts change or after about 7 days; personal and org-shared environments; replacing the base image "not supported yet" | GA | https://code.claude.com/docs/en/cloud-environments.md | reviewed |
| Cursor | resolution personal, team, then `.cursor/environment.json`; agent-led setup, snapshot or Dockerfile; `install` at Build time; Build captures disk only; up to 300 repos; x86_64 only | GA; Environments API beta | https://cursor.com/docs/cloud-agent/setup | unreviewed |
| Antigravity | GitHub repos cloned in, Drive files on demand, uploads (8 MiB per file, 50 files/100 MiB per project) staged at `/home/antigravity/upload` | GA, rolling out | https://antigravity.google/docs/remote-agents | unreviewed |
| Warp | Docker image (glibc only) plus repos plus setup commands, fresh container each run, setup must be idempotent | GA | https://docs.warp.dev/agents/cli/oz-cli/integration-setup/ | unreviewed |
| rizzy-harness (ours) | not in the inventory | none | n/a | from own inventory |

#### Cloud isolation and network egress
What it is: the isolation unit for a cloud session and how its outbound network is limited.
Variants: microVM in a separate cloud account (Cursor), Anthropic VM with an audit proxy (Claude Code), access levels or allowlists (Codex, Claude Code, Cursor), private networks via Tailscale (Codex, Cursor) or PrivateLink (Cursor).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | public internet all, package-manager preset, or extra domains; Tailscale VPN for private networks; published egress IP feed | GA | https://learn.chatgpt.com/docs/cloud.md; https://learn.chatgpt.com/docs/environments/modes.md | reviewed |
| Claude Code | isolated VM per session; network limited by default (disabled, trusted, allowlist); all outbound traffic through a security proxy; GitHub proxy rejects branch deletions and tag pushes; operations audit-logged; isolation technology not documented | GA | https://code.claude.com/docs/en/security.md; https://code.claude.com/docs/en/sandbox-environments.md | reviewed |
| Cursor | Firecracker microVM per agent in a separate AWS account; egress allow-all, defaults plus allowlist, allowlist only; Enterprise lock; git egress proxy for IP allowlisting; PrivateLink or Cloudflare Tunnel | GA (vendor-claim isolation) | https://cursor.com/docs/cloud-agent/security; https://cursor.com/docs/cloud-agent/security-network | corrected |
| Warp | fresh isolated container per run | GA | https://docs.warp.dev/agents/cli/oz-cli/integration-setup/ | unreviewed |
| rizzy-harness (ours) | not applicable; node shell has no sandbox on `main` (Landlock with optional network off only on `integration/gate3`) | none (on main) | `README.md` "The shell is not confined yet" | from own inventory |

#### Secrets in cloud environments
What it is: how API keys and tokens reach a cloud run without leaking into the transcript, the repo or the model.
Variants: plain env vars (most), env vars that "never reach the model" by vendor claim (Cursor), removal before the agent phase (Codex Legacy), just-in-time run-scoped fetch (Warp), short-lived OIDC tokens instead of stored keys (Cursor, Warp, Claude Code GitHub Action). Injection outside the sandbox is the next feature.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | env vars, or network secrets; personal vault; environments shareable in a workspace; Legacy runtime removes secrets before the agent phase | GA / deprecated | https://learn.chatgpt.com/docs/cloud.md; https://learn.chatgpt.com/docs/cloud/internet-access.md | reviewed |
| Claude Code | environment env vars (`.env` format) reach the session only at restore or rebuild; network secrets (next feature); not available in self-hosted environments | GA | https://code.claude.com/docs/en/cloud-environments.md | reviewed |
| Cursor | types `runtime_secret`, `environment_variable`, `build_secret`; scopes environment, team, personal; 1,000 per scope; never returned; KMS; runtime secrets kept out of transcript and "never reach the model" (vendor statement); TOTP secrets; 5-minute OIDC tokens and AWS role assumption; environment-scoped and build secrets never reach self-hosted workers | GA | https://cursor.com/docs/cloud-agent/setup; https://cursor.com/docs/cloud-agent/identity | reviewed |
| Antigravity | OAuth credentials encrypted at rest, tokens injected on call | GA, rolling out | https://antigravity.google/docs/remote-agents | unreviewed |
| OpenHands | env-var injection, masking in output, per-profile scope enforced server-side, `request_secret` card; any authorised runtime receives the value in usable form | GA | `secrets-settings.md`, `agent-profiles.md` (cited by name only in the survey) | unreviewed |
| Warp | `oz secret` (legacy) and factory secrets: encrypted, fetched just-in-time with run-scoped tokens, audit-logged; locally injected secrets never transit Warp; `oz federate` OIDC | GA | https://docs.warp.dev/platform/secrets/; https://docs.warp.dev/agents/cli/oz-cli/federate/ | unreviewed |
| rizzy-harness (ours) | not applicable to cloud; on the node, provider key vars, `.env` names and `HARNESS_*` are stripped from child processes; known secret values redacted from tool output and events (courtesy only) | works / partial | `crates/tools/src/env.rs:1-135`, `crates/node/src/session.rs:194-215` | from own inventory |

#### Credential injection outside the sandbox (egress proxy)
What it is: the agent's processes never hold the real secret; a proxy on the boundary adds it to requests for allowed hosts.
Variants: placeholder in the process replaced by the proxy (Codex network secrets, Hermes Agent iron-proxy opaque tokens, Claude Code sandbox credential masking which is local and needs TLS termination); key never in the VM at all, attached by the proxy (Claude Code cloud network secrets, GitHub proxy); keys held at the control side with inference proxied (OpenClaw, Codex GitHub Action). Stated limits: raw sockets, redirected egress or a stolen CA defeat it (Hermes Agent); SigV4 and service-account auth keep real credentials in the sandbox (Hermes Agent).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | network secrets: programs see a placeholder; the proxy substitutes the real value only for allowed HTTPS:443 destinations; the GitHub Action's Responses API proxy keeps the key from shell steps | GA | https://learn.chatgpt.com/docs/cloud.md; https://learn.chatgpt.com/docs/non-interactive-mode.md | reviewed |
| Claude Code | cloud network secrets: Anthropic's agent proxy attaches the key after requests leave the VM (no sentinel; key never in VM); never attached to GitHub, `api.anthropic.com`, public registries, setup-script requests or telemetry; GitHub credentials encrypted server-side, VM holds a short-lived scoped credential | GA | https://code.claude.com/docs/en/cloud-environments.md; https://code.claude.com/docs/en/security.md | reviewed (corrected wording in review 3) |
| Cursor | git egress proxy (for IP allowlisting, not secret injection) | GA | https://cursor.com/docs/cloud-agent/security-network | corrected |
| OpenClaw | provider keys and GitHub publication stay on the Gateway; secrets with egress-time injection of sentinels | beta | https://docs.openclaw.ai/gateway/cloud-workers/security-model | corrected |
| Hermes Agent | iron-proxy: host-side TLS-intercepting proxy, SHA-256-pinned binary; sandbox holds opaque tokens in the normal env vars; default-deny upstream allowlist; SSRF CIDR denies; binds Docker bridge (Linux) or loopback (Docker Desktop), never `0.0.0.0`; Hermes Agent's own LLM calls are not rewritten | GA ("optional") | /user-guide/egress; /user-guide/egress/iron-proxy | corrected |
| Pi | documented isolation patterns where the credential proxy stays outside the sandbox (Docker Sandboxes, OpenShell) | GA (docs) | https://pi.dev/docs/latest/security | unreviewed |
| rizzy-harness (ours) | none: the node process holds the provider key; tool processes get it stripped from their env, but nothing stops a shell from reading the node's config without a sandbox | none | `crates/tools/src/env.rs:1-135`, AUDIT S1 | from own inventory |

#### Local and cloud hand-off
What it is: moving a running conversation, with its working state, between the user's machine and a remote or cloud session.
Variants: chat plus Git state between local and a connected host, not cloud (Codex); cloud to terminal by pulling the pushed branch, and local to cloud by bundling the repo (Claude Code); fork with a snapshot of uncommitted and untracked files, both directions (Warp); move to cloud and back (Cursor).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Handoff moves a chat and its Git state between local and a connected remote host with a saved project for the same repo; interrupts a running response; not to Codex Cloud | GA | https://learn.chatgpt.com/docs/remote-connections.md | corrected |
| Claude Code | `--teleport`/`/teleport`: checks clean git state, same repo, branch pushed, same account; fetches the branch and loads the conversation (compacted messages not copied); one-way cloud to terminal from the CLI; `--cloud` clones the GitHub branch, or bundles the local repo (under 100 MB, credential-like files left out on macOS/Linux/WSL); Desktop can push local to cloud | GA | https://code.claude.com/docs/en/claude-code-on-the-web.md | unreviewed |
| Claude Desktop | Desktop can push a local session to the cloud (per the Claude Code survey's teleport entry) | GA | https://code.claude.com/docs/en/claude-code-on-the-web.md | unreviewed |
| Cursor | Handoff to Cloud Agents and back; `/in-cloud`, `/autopilot` | GA (no label) | https://cursor.com/docs/agent/agents-window | unreviewed |
| Warp | `/handoff` forks a local conversation to a cloud run with a snapshot of tracked and untracked changes; cloud to cloud and cloud to local (`/continue-locally`); BYOK keys not copied; needs "Store AI conversations in the cloud" | GA | https://docs.warp.dev/platform/handoff/local-to-cloud/; https://docs.warp.dev/platform/handoff/ | corrected |
| Cline | Local/Cloud toggle and sandbox progress card (v4.1.26) | preview | https://github.com/cline/cline/releases | reviewed |
| rizzy-harness (ours) | none: a chat is bound to one node; no move between nodes | none | `crates/master/src/lib.rs:557-600` | from own inventory |

#### Cloud agent control API and follow-ups
What it is: starting, listing, steering and following up on remote runs from a CLI or REST API rather than the UI.
Variants: queue-one-message CLI calls (Claude Code), REST with SSE and steer delivery (Cursor), list and exec from the CLI (Codex), run message bus (Warp).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `codex cloud` picker, `codex cloud exec`, `codex cloud list [--json]` (id, status, environment, `is_review`, `attempt_total`) | GA | https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| Claude Code | `claude -p "msg" --cloud <session-id\|url>` queues one message and exits, sends no local state, refuses if org policy has not loaded (fail closed); `--environment ccpool_...` dispatch returns JSON; environments creatable by API with an Owner's OAuth token | GA / beta | https://code.claude.com/docs/en/claude-code-on-the-web.md; https://code.claude.com/docs/en/self-hosted-environments-testing.md | unreviewed |
| Cursor | Cloud Agents API v1: create, runs, SSE with `Last-Event-ID`, follow-up or `delivery: "steer"`, cancel, artifacts by 15-minute presigned URL, environments, secrets, worker pools; legacy v0 webhook HMAC-signed | beta (v1 public beta) | https://cursor.com/docs/cloud-agent/api/endpoints; https://cursor.com/docs/cloud-agent/api/webhooks | reviewed (partly) |
| OpenHands | Sandbox Server V1 REST under `/api/v1` (app-conversations, sandboxes start/pause/resume) | GA, community-supported | D:/openhands/usage/api/v1.md | unreviewed |
| Warp | `oz run message list\|read\|watch\|send`, named agents, artifacts; Platform API | GA (legacy CLI) | https://docs.warp.dev/agents/cli/oz-cli/ | corrected |
| rizzy-harness (ours) | HTTP API with bearer token drives chats on any node; routines run-now | works | `crates/master/src/http.rs:26-175`, `crates/master/src/http/routines.rs:35-45` | from own inventory |

#### Idle pause and wake on demand
What it is: releasing an idle remote sandbox or worker while keeping the session resumable, and bringing it back when a message arrives.
Variants: VM pause with files kept, then fresh VM with history if reclaimed (Claude Code); release slot and resume on a fresh runner, losing unpushed work (Claude Code runner); hibernate and wake with a timeout (Cursor); serverless hibernation (Hermes Agent Modal, Daytona); sandbox released minutes after a turn and rebuilt on the next reply (Claude Tag).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | cloud VM pauses after a few minutes idle, next message restores it; a reclaimed VM is replaced with history restored but no shell processes or subagents; runner `--release-idle-session-min`, `--kill-session-after-min`, `--exit-if-unused-min`; Claude Tag releases the sandbox a few minutes after a turn and resumes in a fresh one | GA / beta / public beta | https://code.claude.com/docs/en/claude-code-on-the-web.md; https://code.claude.com/docs/en/self-hosted-environments-deploy.md; https://claude.com/docs/claude-tag/concepts/how-it-works.md | unreviewed / reviewed |
| Cursor | live VM recycled after idle; self-hosted `claimed_offline` hibernation with wake timeout; idle release default 3600 s | GA / unclear | https://cursor.com/docs/cloud-agent/self-hosted/pool | reviewed |
| OpenHands | DigitalOcean Managed Agents sandboxes pause when idle; per-conversation Docker idle stop | unlabelled | D:/openhands/usage/agent-canvas/backends.md; `docker-execution.md` (path abbreviated in the survey) | reviewed |
| Hermes Agent | Modal and Daytona backends hibernate when idle; Vercel snapshots keep files, not processes | GA | /user-guide/features/tools | unreviewed |
| Cline | cloud sandbox pauses after 15 min idle and resumes | preview | https://docs.cline.bot/usage/cloud-agents.md | reviewed |
| rizzy-harness (ours) | none; a node is either connected or not; incognito chats purged after idle TTL is unrelated | none | n/a | from own inventory |

#### Warm pools and pre-warming
What it is: keeping started-but-unassigned workers, processes or checkouts ready so a new session starts fast, without putting one user's data in the shared image.
Variants: spare processes claimed later (Claude Code SDK `prewarm()`), standby runners (`--min-idle`), pre-warmed checkouts (Claude Code runner), dormant pods activated with one call carrying per-user config (OpenHands deferred init), warm pools in the controller (Cursor), warm snapshots (OpenClaw), pre-warmed sandboxes (Warp).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | SDK `startup()` completes the handshake before a prompt; alpha `prewarm()` starts an unbound spare (about 230-260 MB) claimed later with limited options; orchestrator `--min-idle` standby runners and `prewarm` spawn reason; runner keeps a canonical clone per repo (capacity 1 hard-resets without `git clean`, so untracked files from earlier sessions of the same owner persist) | GA / alpha / beta | https://code.claude.com/docs/en/agent-sdk/typescript.md; https://code.claude.com/docs/en/self-hosted-environments-configuration.md; https://code.claude.com/docs/en/self-hosted-environments-deploy.md | unreviewed / reviewed |
| Cursor | controller warm pool | unclear | https://cursor.com/docs/cloud-agent/self-hosted/pool | reviewed |
| OpenHands | `OH_DEFERRED_INIT=true`: pod boots dormant (`/api/*` 503), `POST /api/init` with `X-Init-API-Key` activates it with one user's env, persistence paths and session keys; second init returns 400 | GA | D:/sdk/guides/agent-server/deferred-init.md | reviewed |
| OpenClaw | warm snapshots (Linux images only) | beta | https://docs.openclaw.ai/gateway/cloud-workers | corrected |
| Warp | pre-warmed sandboxes (changelog 2026.10.22) | GA | https://docs.warp.dev/agents/cli/oz-cli/troubleshooting/ | unreviewed |
| rizzy-harness (ours) | not in the inventory | none | n/a | from own inventory |

## 3. Availability matrix

Legend: `Y` documented and released; `B` beta, experimental, preview, partial, or released but deprecated; `P` planned or vendor-claim only; `-` nothing documented in the survey (weak evidence); `?` unverified. Column `Ours` uses the own inventory (`Y` works, `B` partial, `-` none). Short column names: CX Codex, OCX OpenCodex, OC OpenCode, CC Claude Code, CD Claude Desktop, DS DeepSeek harness, OMP Oh My Pi, Pi Pi, Cur Cursor, AG Antigravity, OH OpenHands, OCl OpenClaw, gs goose, HA Hermes Agent, Zed Zed/ACP, Wp Warp, Cl Cline.

| Feature | CX | OCX | OC | CC | CD | DS | OMP | Pi | Cur | AG | OH | OCl | gs | HA | Zed | Wp | Cl | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| One-shot headless run | Y | - | Y | Y | - | B | Y | Y | Y | Y | B | - | Y | Y | - | Y | Y | - |
| Structured event stream / schema output | Y | - | Y | Y | - | - | Y | Y | Y | Y | B | - | Y | - | - | Y | Y | - |
| Approval behaviour with no human present | Y | - | - | Y | - | B | Y | Y | Y | Y | - | - | - | Y | - | Y | Y | Y |
| Config isolation for CI runs | Y | - | - | Y | - | - | - | - | - | - | - | - | Y | - | - | - | Y | - |
| Official CI action / pipeline | Y | - | B | Y | - | - | - | - | ? | - | ? | - | - | - | - | Y | Y | - |
| Machine credentials for CI | Y | - | - | Y | - | - | - | - | Y | B | - | - | - | - | - | Y | - | Y |
| Event-triggered remote runs | Y | - | - | B | - | B | - | - | Y | - | Y | Y | - | Y | - | Y | Y | Y |
| Programmatic control protocol | B | - | Y | Y | - | B | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | Y |
| ACP support | - | - | Y | - | - | B | Y | - | B | Y | Y | Y | Y | Y | Y | ? | Y | - |
| Remote attach of client to agent server | B | - | Y | - | - | B | - | - | - | - | Y | Y | B | Y | - | - | B | Y |
| Relay-based remote control (web/phone) | Y | Y | Y | Y | Y | - | Y | B | Y | Y | - | B | B | Y | - | - | B | Y |
| Pairing / auth of remote clients and hosts | Y | Y | Y | B | B | B | - | - | Y | - | Y | Y | B | Y | - | Y | B | Y |
| Exposure beyond loopback | B | Y | Y | - | - | B | Y | - | - | - | Y | Y | B | Y | - | - | B | Y |
| Reconnect / resume after link loss | - | B | Y | Y | Y | B | - | B | B | - | - | B | Y | Y | Y | - | Y | Y |
| SSH remote execution | Y | ? | Y | - | Y | B | Y | B | - | B | - | Y | - | Y | Y | - | Y | - |
| Pluggable execution backend | ? | - | - | Y | - | B | - | Y | - | - | Y | Y | - | Y | Y | - | - | - |
| Self-hosted workers for hosted control plane | - | - | - | B | B | - | - | - | B | - | - | B | - | - | - | Y | - | Y |
| Work claim, lease, heartbeat | - | - | - | B | - | - | - | - | B | - | - | B | - | - | - | B | - | - |
| Per-session / per-dispatch worker credentials | - | - | - | B | - | - | - | - | B | - | - | B | - | - | - | Y | - | - |
| Execution-side authority over policy | Y | B | - | B | - | - | - | - | B | - | - | B | - | - | - | - | - | Y |
| Central credential broker / inference proxy | - | Y | - | Y | - | - | Y | - | - | - | - | B | - | B | - | Y | Y | - |
| Multi-node fleet under one control side | - | - | - | B | - | - | - | - | B | - | Y | Y | - | Y | - | Y | - | Y |
| Execution-side health, drain, metrics | B | Y | - | B | - | - | - | - | B | - | Y | - | - | - | - | Y | - | B |
| Containerised execution side | - | - | Y | B | - | - | - | - | - | - | Y | Y | - | Y | - | Y | - | - |
| Vendor-hosted cloud agents | Y | - | - | Y | Y | - | - | - | Y | Y | Y | - | - | - | - | Y | B | - |
| Cloud environment definition and caching | Y | - | - | Y | - | - | - | - | Y | Y | - | - | - | - | - | Y | - | - |
| Cloud isolation and egress control | Y | - | - | Y | - | - | - | - | Y | - | - | - | - | - | - | Y | - | - |
| Secrets in cloud environments | Y | - | - | Y | - | - | - | - | Y | Y | Y | - | - | - | - | Y | - | - |
| Credential injection outside the sandbox | Y | - | - | Y | - | - | - | Y | - | - | - | B | - | Y | - | - | - | - |
| Local and cloud hand-off | Y | - | - | Y | Y | - | - | - | Y | - | - | - | - | - | - | Y | B | - |
| Cloud agent control API / follow-ups | Y | - | - | Y | - | - | - | - | B | - | Y | - | - | - | - | Y | - | Y |
| Idle pause and wake on demand | - | - | - | Y | - | - | - | - | Y | - | B | - | - | Y | - | - | B | - |
| Warm pools and pre-warming | - | - | - | B | - | - | - | - | B | - | Y | B | - | - | - | Y | - | - |

Notes on the matrix. "Execution-side authority over policy": `Y`/`B` means the execution side can restrict what the control side asks for; OpenHands, Warp and Cline have entries in 2.4 but are `-` because their documented model puts the decision on the control side (or lets the client widen limits). "Self-hosted workers": ours is `Y` in the sense that the node is a self-hosted worker, but the control side (master) is self-hosted too, unlike every other row.

## 4. Common across the ecosystem

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

## 5. Only one harness has it

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

## 6. Ours against the field

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

## 7. Corrections, contradictions and unverified claims

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

## 8. Questions for Round 2 (source code only)

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
