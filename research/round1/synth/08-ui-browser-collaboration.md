# Synthesis 08: UI surfaces, browser and computer use, collaboration and administration

Areas: 15 (UI surfaces), 16 (browser automation and computer use), 18 (collaboration, teams and enterprise administration). Research date 2026-11-10. Synthesis only: every fact below comes from the survey and review files in `research/round1/`; nothing was browsed.

## 1. Scope

Read, per file (line ranges in the current files):

| File | Ranges read |
|---|---|
| `codex.md` | 146-160, 555-582 (diff and review entries), 723-914 (areas 15, 16, 17, 18), 986-1250 (sections 3-9) |
| `opencode.md` | 159-166, 213-220, 355-418, 458-610 |
| `claude-code.md` | 1381-1415, 1599-1604, 1737-1745, 1797-1802, 1944-1950, 2621-2628, 3274-3548 (areas 15, 16), 3575-3580, 3640-3680, 3765-3770, 3843-4215 (area 18), 4359-4412, 4851-4856, 5109-5120, 5202-5340; plus `claude-code.review-1/2/3.md` (grepped for these areas) |
| `claude-desktop.md` | 72-90, 148-175, 196-240, 296-407, 469-640 |
| `deepseek-harness.md` | 131-138, 203-209, 383-450, 533-700 |
| `oh-my-pi.md` | 190-198, 302-310, 410-480, 549-640 |
| `pi.md` | 199-204, 435-490, 561-700 |
| `cursor.md` | 55-62, 163-173, 339-411, 443-446, 456-610 |
| `antigravity.md` | 60-72, 196-248, 366-423, 494-600 |
| `openhands.md` | 309-336, 393-412, 459-560 |
| `openclaw.md` | 110-114, 146-150, 410-467, 476-481, 500-513, 596-760 |
| `goose.md` | 404-447, 518-700 |
| `hermes-agent.md` | 287-300, 321-328, 517-634, 679-860 |
| `zed-acp.md` | 38-46, 59-74, 114-118, 164-185, 196-210, 346-420 |
| `warp.md` | 194-205, 260-275, 361-457, 546-700 |
| `cline.md` | 107-111, 174-182, 212-252, 335-424, 476-700 |
| `opencodex.md` | 130-166, 195-240 |
| `own-rizzy-harness.md` | 1-13, 58-76, 172-190, 236-250, 264-275, 290-400 |

Not read in full: `claude-code.md` outside the ranges above (about 900 KB; area 15 and 18 entries were read, but I trimmed long "What" lines to roughly 400-1000 characters, so fine print such as full key lists is not carried). Areas 15 and 18 for `openhands.md` and `claude-desktop.md` have no numeric section headings; I used the "Area 15/16/18" headings. `00-targets.md` was not read. Sections 3, 6, 7, 9 were read for every file and filtered by keyword for my areas, not read line by line in the largest files.

Conventions: Hermes Agent evidence in its survey is written as paths under `https://hermes-agent.nousresearch.com/docs/` and is expanded here by adding that base. OpenHands evidence `D:/...` is expanded with its stated base `https://docs.openhands.dev`. "Check" values: `reviewed` (the entry or a review table row says it was verified), `corrected` (corrected in review; corrected text used), `unreviewed`, `unverified`. Where the survey says "partly verified" I write `reviewed` only for the parts named. "Nothing documented" means the survey found no page, not that the feature is absent. Counts in sections 4-5 treat Claude Code and Claude Desktop as two harnesses and exclude OpenCodex (a gateway, not a harness) and ours: 16 harnesses.

---

## 2. Features

### Area 15. UI surfaces

#### Terminal UI
What it is: an interactive full-screen or scrollback terminal front end to the agent loop, with prompt editing, status line and transcript tools.
Variants: renderer (alternate screen vs native scrollback, and some offer both); whether the TUI is a thin client that attaches to a long-lived daemon (Hermes, Codex remote TUI, Claude Code agent view) or the loop itself; how much of the status line the user can script (Claude Code, Antigravity) or extend.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Fullscreen or scrollback (`/tui`, `/raw`), Kitty/Sixel image preview, configurable keymap with leader key, Vim mode, transcript search, themes, `/statusline`, `/voice`. | GA (voice experimental) | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| OpenCode | OpenTUI-based: tabs, Ctrl+P palette, leader key, `@` attach with line ranges, `!` shell mode, themes. | GA | https://opencode.ai/docs/cli/tui/ | unreviewed |
| Claude Code | Large keyboard model (history search, stash, image chips, mode cycling), Vim mode with `vimInsertModeRemaps` readable only from user/managed settings, scriptable status line with JSON input, prompt suggestions. Fullscreen alt-screen renderer is a research preview with mouse support and an ordered default-renderer decision table; two failed fullscreen starts pin the classic renderer. | GA (fullscreen: research preview) | https://code.claude.com/docs/en/interactive-mode.md, https://code.claude.com/docs/en/fullscreen.md, https://code.claude.com/docs/en/terminal-config.md | unreviewed |
| Antigravity | Go CLI TUI: alt-screen, themes, Vim mode, status line and title scripts fed live metadata, `keybindings.json`. | GA | https://antigravity.google/docs/cli/using | unreviewed |
| Oh My Pi | `pi-tui` differential rendering, tool cards, status line, `@` mentions, agent hub (Alt+A), `/tree` navigator. | GA | https://omp.sh/docs/using, https://omp.sh/docs/settings | unreviewed |
| Pi | Fullscreen by default (`regular` uses native scrollback), external editor, tool-output and thinking toggles, `keybindings.json`. | GA | https://pi.dev/docs/latest/usage, https://pi.dev/docs/latest/keybindings | unreviewed |
| Hermes Agent | Full TUI with status bar, drafts stash and voice key; `hermes chat` attaches to the profile's gateway over a local WebSocket, so closing the terminal does not cancel a running turn. `--tui` gives an Ink UI with mouse. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/cli | unreviewed |
| OpenClaw | `openclaw tui` plus a wide CLI (`approvals`, `devices`, `nodes`, `policy`, `sandbox`). | GA | https://docs.openclaw.ai/llms.txt, https://docs.openclaw.ai/start/getting-started | unreviewed |
| Warp | `warp` agent CLI with transcript, slash menu, Vim mode, statusline, image paste, voice (not in the official Linux build). | GA | https://docs.warp.dev/agents/cli/ | unreviewed |
| Cline | `cline -i`: Tab toggles plan/act, Shift+Tab auto-approve all, status area with model, context, cost, branch, diff stats. | GA | https://docs.cline.bot/usage/tui.md | unreviewed |
| rizzy-harness (ours) | No terminal UI code; the CLI is admin only (`serve`, `token`, `doctor`, `node join`, `node run`). | none | `crates/cli/src/main.rs:31-118` | from own inventory |

Nothing documented as a TUI: Claude Desktop (separate product), DeepSeek harness (CLI profiles listed elsewhere), Zed/ACP, OpenCodex, OpenHands (CLI exists, TUI not described in lines read), goose (CLI named, no TUI detail).

#### IDE integrations
What it is: a panel or plugin inside an editor that hosts the agent and shares selection, open files and diagnostics.
Variants: an extension that bundles the CLI (Claude Code in VS Code) vs a plugin that wraps the terminal CLI (Claude Code in JetBrains); an editor that is itself the harness (Cursor, Antigravity, Zed); and ACP, where an editor hosts someone else's agent.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Extension for VS Code, Cursor, Windsurf: attaches open files and selection, in-place diff review, delegates to cloud; `runCodexInWindowsSubsystemForLinux` keeps the agent in WSL. Xcode and JetBrains integrations are third-party. | GA | https://learn.chatgpt.com/docs/codex/ide.md | unreviewed |
| Claude Code | VS Code extension (and forks) bundles a private CLI copy and shares history and `settings.json` with the CLI; tab dots (blue = permission pending, orange = finished while hidden); plan opened as an editable Markdown document; `vscode://` deep links never auto-submit a prompt. JetBrains plugin does not bundle the CLI, runs `claude` in the IDE terminal, diffs in the IDE viewer. | VS Code GA; JetBrains beta | https://code.claude.com/docs/en/vs-code.md, https://code.claude.com/docs/en/jetbrains.md | unreviewed |
| Cline | VS Code panel with Plan/Act toggle, diffs with Approve/Reject and checkpoints; JetBrains tool window. | GA | https://docs.cline.bot/usage/ide.md | unreviewed |
| Zed/ACP | Zed is the editor: Agent Panel, Threads Sidebar, inline assistant; hosts external agents over ACP (agent runs as a child process, Zed renders the thread). | GA | https://zed.dev/docs/ai/overview, https://zed.dev/docs/ai/external-agents.md | unreviewed |
| Cursor | Cursor is the IDE; Agents Window, CLI, web and iOS view the same backend. | GA | https://cursor.com/docs/agent/agents-window | unreviewed |
| Antigravity | IDE and extensions (JetBrains, Zed, Xcode, Eclipse enterprise sign-in is preview); standalone IDE unsupported for enterprise. | GA (enterprise sign-in in other IDEs: preview) | https://antigravity.google/docs/enterprise, https://antigravity.google/docs/ide/extensions | unreviewed |
| Oh My Pi | Zed through ACP; "IDE wired in" headline is a vendor claim. | GA (ACP) | https://omp.sh/docs/acp | unreviewed |
| goose | Zed integration through ACP (`goose acp`). | GA | https://goose-docs.ai/docs/guides/zed.md | reviewed |
| OpenHands | Legacy GUI embedded VS Code; the legacy GUI is deprecated. | deprecated | https://docs.openhands.dev/openhands/usage/key-features.md | unreviewed |
| rizzy-harness (ours) | None (no LSP, ACP, VS Code or JetBrains plugin); ACP is only a deferred idea in `PLAN.md`. | none | `PLAN.md:143-146` (docs-only mention) | from own inventory |

OpenCode: IDE pages exist in the V1 index and were not opened (`?`).

#### Desktop apps
What it is: a native or Electron shell around the agent, usually adding panes (diff, terminal, browser, files), notifications and multi-session management.
Variants: shells that talk to a server over HTTP/ACP and can point at a remote one (goose, Hermes, OpenCode) vs a bundled runtime reached by IPC (DeepSeek harness); platform coverage differs a lot (Hermes macOS Apple Silicon only; DeepSeek macOS arm and Windows x64 only).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | ChatGPT desktop app in Codex mode: projects, parallel chats (Local, Worktree, Cloud), diff pane with Git, terminal, built-in browser, scheduled tasks; `codex app` and `/app` hand over from the CLI. | GA (Linux preview) | https://learn.chatgpt.com/docs/app.md, https://learn.chatgpt.com/docs/linux/linux-app.md | unreviewed |
| Claude Desktop | Draggable pane workspace (chat, diff, browser, terminal, file, plan, tasks, subagent, iOS Simulator), pop-out windows, session rail, Quick Entry hotkey. Chat and Cowork are merging into one message box. | GA (unified Claude: beta, rolling out) | https://code.claude.com/docs/en/desktop, https://claude.com/docs/cowork/changelog.md | reviewed (Linux limits only; rest partly) |
| OpenCode | Electron (third-party report) with sessions, tabs, diffs, browser, persistent terminals, GUI plugins, worktree pickers, read/unread marking. README labels it beta. | beta / unclear | https://opencode.ai/product, https://github.com/anomalyco/opencode | unreviewed |
| DeepSeek harness | Electron app bundling its own `dsh` runtime and profile; no HTTP server, requests go over an IPC bridge. macOS Apple silicon and Windows x64 only. | preview | https://deepseek.com/harness, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-server | reviewed |
| Hermes Agent | Native app sharing config, sessions, skills and memory with the CLI; file browser, terminal, Git review pane, live subagent monitor; connects to a local backend or remote `hermes serve` (OAuth recommended), Hermes Cloud, or SSH gateways, several at once. macOS Apple Silicon only. | GA (no stage given) | https://hermes-agent.nousresearch.com/docs/user-guide/desktop, https://hermes-agent.nousresearch.com/docs/getting-started/platform-support | unreviewed |
| goose | Desktop app starts its own `goose serve` (ACP over HTTP/WebSocket) and can be pointed at a remote one; recipe library, scheduler view. | GA | https://goose-docs.ai/docs/guides/sessions/session-management.md, https://goose-docs.ai/docs/guides/remote-goose-server.md | reviewed (remote serve) |
| OpenHands | Electron desktop (DMG/exe/AppImage/deb); macOS build unsigned. | preview | https://docs.openhands.dev/openhands/usage/agent-canvas/setup.md | unreviewed |
| OpenClaw | macOS menu bar app (can act as a node), iOS/Android nodes, Windows Hub and Tauri app, Linux companion. Its own maturity scorecard rates Windows App/Node as Beta at 19% quality. | GA / unclear per platform | https://docs.openclaw.ai/platforms, https://docs.openclaw.ai/nodes, https://docs.openclaw.ai/maturity/scorecard | unreviewed |
| Cline | Desktop: parallel sessions, Changes button with per-turn and session diffs, files panel, terminal, voice; Tauri plus a Bun sidecar per README. | GA (Windows beta) | https://docs.cline.bot/usage/cline-desktop.md | unreviewed |
| Warp | The product itself is a desktop terminal/agent app (macOS, Windows, Linux). | GA | https://docs.warp.dev/agents/cli/ | unreviewed |
| Antigravity | Desktop App (macOS, Windows, Linux): command centre for agents, side panel with artifacts/files/diffs/terminal/web tabs, built-in browser. | GA | https://antigravity.google/docs/features, https://antigravity.google/product/antigravity-2 | unreviewed |
| OpenCodex | Native tray plus dashboard shell, usage popover, macOS widget, update check every 6 hours; README says beta, desktop page does not. | beta (sources disagree) | https://opencodex.me/guides/desktop-app/ | unreviewed |
| rizzy-harness (ours) | None (web UI only). | none | `web/src/components/Composer.tsx` (web only) | from own inventory |

Nothing documented: Pi, Oh My Pi (no desktop app in the lines read).

#### Web UIs (what the web UI can do)
What it is: a browser client for sessions. The questions that matter for a web-first harness: which UI is open source and self-hostable, whether it has authentication, and which of sessions, diffs, approvals, file browsing, terminal and settings it covers.
Variants: (a) served by the agent process and password protected (OpenCode); (b) served by a gateway with loopback default and an auth gate that engages on non-loopback binds (Hermes); (c) a browser client that federates to any number of independent backends and has no auth in the open tier (OpenHands Canvas); (d) a hosted control-plane UI for cloud and remote sessions (Claude Code, Cursor, Warp, Antigravity); (e) a gateway admin dashboard that is not a session UI (OpenCodex).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | Web UI served by the same server, password protected by default; `opencode pair` prints a single-use 5-minute link and QR, sets a 30-day cookie; `--remote` exposes it through an end-to-end encrypted "OpenTunnel"; localhost by default, port 49374. One shared `opencode` user, no per-user accounts for self-hosted use. | GA | https://opencode.ai/docs/cli/web/, https://opencode.ai/docs/ | reviewed |
| DeepSeek harness | `dsh web` on loopback: workspace chooser, session list with search/queue/cancel, two views of one event stream (chat and trajectory), background jobs, settings, automation tasks page, trace view; right sidebar docks guide, text preview, file tree, browser and subagent-chat tabs. Single launch-token plus signed cookie; no multi-user model; remote access via reverse proxy with `--trusted-host` and TLS. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-client, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/sidebar-right, https://deepseek-harness.github.io/deepseek-harness/en/guide/public-deployments | corrected (right sidebar is documented) |
| OpenHands | Agent Canvas, a React browser UI (MIT): chat with grouped activity stages, files, commits/changes, terminal, conversation list with tags/pins/archive, voice input, context meter. Connects to any number of backends. Phone/tablet access via Tailscale or ngrok with `--public` and an API key. Open tier has no authentication, RBAC or multi-user. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md, https://docs.openhands.dev/openhands/usage/agent-canvas/mobile-access.md, https://docs.openhands.dev/enterprise/enterprise-vs-oss.md | reviewed (matrix); unreviewed (UI list) |
| OpenClaw | Control UI (Vite plus Solid SPA) served by the Gateway on the same port as the WebSocket: chat with child sessions, subagent and background-process panels, Changes (git diff) panel, automation status, settings (MCP, plugins, devices), usage page, schema-generated config form with raw JSON editor. Keeps drafts during disconnect, does not replay actions. | GA | https://docs.openclaw.ai/web/control-ui, https://docs.openclaw.ai/gateway/configuration | unreviewed |
| Hermes Agent | `hermes dashboard` (127.0.0.1:9119): status, embedded TUI chat over WebSocket/PTY, config, API keys, profiles, skills, MCP, sessions, logs, analytics, cron, webhooks, pairing. Non-loopback binds engage an auth gate (username/password, Nous Portal OAuth, or self-hosted OIDC) and refuse to start without a provider. Webapp (v0.23.0) serves the Desktop workspace in a browser. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/web-dashboard, https://hermes-agent.nousresearch.com/docs/user-guide/docker | unreviewed |
| Claude Code | claude.ai/code for cloud sessions: repo and branch pickers, environment selector, permission-mode dropdown, sidebar, Changes card with +/- counts, diff, create PR, archive/delete. Hosted by Anthropic; paid plans only; in every execution mode (including the self-hosted runner) the control plane and claude.ai UI stay Anthropic-hosted. | GA | https://code.claude.com/docs/en/web-quickstart.md, https://code.claude.com/docs/en/claude-code-on-the-web.md | unreviewed |
| Cursor | cursor.com/agents web client for cloud agents; web-only tasks include secrets, environments, MCP management, automations, admin. | GA | https://cursor.com/docs/agent/agents-window, https://cursor.com/docs/cloud-agent/mobile | unreviewed |
| Warp | Oz web app (runs, agents, skills, schedules, environments, secrets, integrations) and Warp Drive on the web (view and edit objects and shared sessions; no command execution). Control plane is Warp-operated. | GA | https://docs.warp.dev/platform/oz-web-app/, https://docs.warp.dev/knowledge-and-collaboration/warp-drive/web/ | unreviewed |
| Antigravity | Web dashboard (antigravity.google.com) controls a session on your machine through a host-initiated reverse tunnel; tool approvals answerable remotely; settings cannot be changed from the web. Same Google account required. | GA (disable-able by policy) | https://antigravity.google/docs/remote-control | reviewed |
| Claude Desktop | Unified Claude (Chat plus Cowork) is rolling out on web, desktop and mobile for Pro and Max. | beta / rolling out | https://support.claude.com/en/articles/12138966-release-notes | reviewed (Sep 16 2026 note only) |
| Oh My Pi | `/collab` guests can join from a browser; this is a live-session viewer, not a general UI. | GA | https://omp.sh/docs/collab | reviewed |
| Cline | Kanban web board (127.0.0.1:3484, no built-in authentication, reachable via `--host 0.0.0.0`); `cline hub dashboard` (port 8787, `--room-secret` required for non-local hosts; what it shows is not documented). | preview (Kanban); unclear (dashboard) | https://docs.cline.bot/kanban/remote-access.md, https://docs.cline.bot/cli/cli-reference.md | corrected (Kanban remote access); unreviewed (dashboard) |
| OpenCodex | Gateway dashboard (eight pages: Dashboard, Connect, Codex, Providers, Models, Subagents, Usage and Logs, Remote Link), quota chip bar, provider tests. Not a session UI. | GA (dashboard) | https://opencodex.me/guides/web-dashboard/ | unreviewed |
| goose | No web UI documented; an experimental roaming transport has a wasm browser peer. | experimental | https://goose-docs.ai/docs/guides/roaming-agents.md | reviewed |
| rizzy-harness (ours) | React 19 web UI embedded in the binary: Search/Ask/Agentic modes, agent/model/node pickers, conversation view with tool cards, per-turn Undo/Rewind, validated `ui:` blocks, sidebar areas (Agents, Routines, Artifacts, Projects, Recent). No file browser, editor or terminal pane; "Connections" is a stub. | works (file/terminal panes: none) | `web/src/components/Composer.tsx`, `web/src/components/Conversation.tsx`, `web/src/components/Sidebar.tsx:129` | from own inventory |

Nothing documented as a first-party web UI: Codex (cloud and mobile exist; the survey found no web UI page), Pi (a `pi-web-ui` components library appeared in a search snippet only), Zed/ACP.

#### Mobile
What it is: a phone client, or phone access to a session that runs elsewhere, so you can start, steer and approve work away from the machine.
Variants: native app against a vendor relay (Codex, Claude, Cursor, OpenCode TestFlight); phone as a node of the gateway (OpenClaw); plain browser over Tailscale/ngrok (OpenHands, Antigravity web); the relay's trust story is mostly undocumented.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Cloud tasks, or a paired Mac/Windows host reached by QR code and relay; the phone can start, steer, approve and review. "Secure relay layer" has no technical description. | GA (rollout-dependent) | https://learn.chatgpt.com/docs/mobile.md, https://learn.chatgpt.com/docs/remote-connections.md | unreviewed |
| Claude Code | No separate app: the Code tab of the Claude iOS/Android app is a client for cloud sessions, Projects, Remote Control and Dispatch. Permission modes from mobile never include Bypass. `/mobile` shows a QR. | GA | https://code.claude.com/docs/en/mobile.md | unreviewed |
| Claude Desktop | Same app; push notifications when Remote Control is active; Android is on the Code docs page but not the download page. | GA | https://code.claude.com/docs/en/mobile, https://claude.com/download | unreviewed |
| Cursor | iOS app (iOS 26+), Live Activities for up to eight agents, push, Siri voice; Android planned (the cloud-agent page also describes an Android PWA; the mobile page says planned). | GA (iOS); planned (Android) | https://cursor.com/docs/cloud-agent/mobile, https://cursor.com/changelog | reviewed (partly) |
| OpenCode | Start, steer and approve from a phone; iPhone via TestFlight, Android "coming soon". | beta/preview | https://opencode.ai/product, https://opencode.ai/changelog | reviewed |
| OpenClaw | iOS and Android are nodes (pairing, chat, voice, camera, screen, location); Apple Watch node. | GA / unclear per platform | https://docs.openclaw.ai/platforms, https://docs.openclaw.ai/nodes | unreviewed |
| Antigravity | Mobile web through Remote Control (no native app documented). | GA | https://antigravity.google/docs/features | unreviewed |
| OpenHands | Phone/tablet access to the Canvas UI via Tailscale (private) or ngrok with `--public` and an API key. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/mobile-access.md | unreviewed |
| Warp | Warp Drive on the web, including mobile; view and edit, no command execution. | GA | https://docs.warp.dev/knowledge-and-collaboration/warp-drive/web/ | unreviewed |
| Oh My Pi | Release notes mention "Tern" with a phone long-press turn menu; no docs page. | unclear | https://github.com/can1357/oh-my-pi/releases | unverified |
| rizzy-harness (ours) | Responsive web (drawer, icon rail, touch bar mode); no native app, no PWA manifest or service worker found. | partial | `web/src/components/Sidebar.tsx:38`, `web/index.html` | from own inventory |

Pi supports Android through Termux (runs the agent on the phone; not a mobile client).

#### Voice
What it is: dictation into the prompt, or a conversational voice loop.
Variants: local vs vendor-hosted transcription (Claude Code audio streams to Anthropic; Codex, goose and OpenHands offer provider choices); dictation only vs full "talk" loop with wake word (OpenClaw, Hermes).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/voice` hold or tap mode; audio streams to Anthropic, needs claude.ai sign-in and a local microphone (not cloud or SSH); 20 languages; admin policy can disable. | GA | https://code.claude.com/docs/en/voice-dictation.md | unreviewed |
| Codex | `/voice` in the TUI; Codex Micro hardware keypad with dial and voice. | experimental (voice); GA (Micro, limited run) | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/features/codex-micro.md | unreviewed |
| Antigravity | Gemini Live voice mode and live dictation (Ctrl+M); CLI `/voice live`, `agy mic-serve` over SSH; dictation unavailable for business/enterprise. | GA (rolling out) | https://antigravity.google/docs/features | unreviewed |
| OpenClaw | Talk mode (continuous loop, ElevenLabs/MLX/system TTS), realtime variants, voice wake with a Gateway-owned global word list. | GA / unclear per surface | https://docs.openclaw.ai/nodes/talk, https://docs.openclaw.ai/nodes/voicewake | unreviewed |
| Hermes Agent | Voice mode in CLI, Telegram, Discord voice channels; wake word; ten TTS options. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/overview | unreviewed |
| goose | Desktop dictation with a local provider or ElevenLabs/Groq/OpenAI. | GA | https://goose-docs.ai/docs/guides/sessions/in-session-actions.md | unreviewed |
| DeepSeek harness | Experimental speech-to-text providers listed on the landing page; no mechanism read. | experimental | https://deepseek.com/harness | unreviewed |
| Cursor | Voice Chat (2026-11-06 changelog), Siri on iOS. | GA (no label) | https://cursor.com/changelog | unreviewed |
| OpenHands | Browser speech recognition or an OpenAI-compatible transcription endpoint. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md | unreviewed |
| Warp | Voice input in the app, CLI and rich input editor. | GA | https://docs.warp.dev/agents/cli/input-and-shell-commands/ | unreviewed |
| Cline | Desktop voice input via a provider such as ElevenLabs. | GA | https://docs.cline.bot/usage/cline-desktop.md | unreviewed |
| rizzy-harness (ours) | Voice dictation (mic in the composer) with per-account daily request and size limits on `integration/gate3` only. | works | `crates/master/src/voice.rs` | from own inventory |

#### Messaging-app channels
What it is: using a chat platform (Slack, Telegram, Discord, Teams and so on) either as the agent's primary UI or as a trigger that starts a cloud task.
Variants: channel-as-UI, where a long-running gateway holds bot tokens and maps chats or threads to sessions (Hermes, OpenClaw); integration-as-trigger, where an `@mention` starts a task in the vendor's cloud and the result is posted back (Codex, Cursor, OpenHands, Claude Code, Warp). They differ on who holds the platform credentials and where the agent runs.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Hermes Agent | One gateway process connects Telegram, Discord, Slack, WhatsApp, Signal, Matrix, Teams, email, SMS and more; per-chat sessions, delivery ledger (replies stored before send, redelivered up to 3 times in 24 h), circuit breaker, token locks, bot-to-bot loop guard (4 hops). Hermes Relay lets the gateway dial out only and hold no platform tokens. | GA (Relay: experimental) | https://hermes-agent.nousresearch.com/docs/user-guide/messaging, https://hermes-agent.nousresearch.com/docs/user-guide/messaging/relay | corrected (details are on the messaging page, not gateway-internals); Relay reviewed |
| OpenClaw | Channels as UI: Telegram, WebChat in core; plugins for Discord, Slack, WhatsApp, Signal, iMessage, Matrix, Teams and more; threads map to sessions; ambient room events let the agent read unmentioned messages and speak only via the `message` tool; approval prompts can be forwarded to chat and answered with `/approve`. | GA (per channel varies) | https://docs.openclaw.ai/concepts/features, https://docs.openclaw.ai/channels/ambient-room-events | unreviewed |
| Claude Code | Claude Tag: Anthropic-hosted Slack agent for Team and Enterprise, one org pairing, spend limit, no per-user seat, cross-channel posting needs "Approve and post" from a 1:1 DM. Legacy per-user Slack app retired in Tag-connected workspaces from 2026-10-05. Channels let MCP servers push messages into a session. | public beta (Tag); unclear (Channels, research preview in admin page) | https://code.claude.com/docs/en/claude-tag.md, https://code.claude.com/docs/en/slack.md, https://code.claude.com/docs/en/channels.md | reviewed (Tag session model); unreviewed (rest) |
| Codex | `@ChatGPT` in Slack delegates repo work to a Codex Cloud task (needs a published, workspace-shared environment); Linear assignment or `@Codex` creates a cloud chat. | GA | https://learn.chatgpt.com/docs/third-party/slack.md, https://learn.chatgpt.com/docs/third-party/linear.md | unreviewed |
| Cursor | `@Cursor` in Slack starts or follows up cloud agents with inline options (`repo`, `env`, `branch`, `model`, `pool`), posts a short plan first for non-trivial tasks; Linear delegation. | GA (no label) | https://cursor.com/docs/integrations/slack, https://cursor.com/docs/integrations/linear | unreviewed |
| OpenHands | Cloud: GitHub, GitLab, Bitbucket, Slack, Jira triggers; HMAC-verified webhook, resolve org/user, run in a sandbox, callback posts result. | GA | https://docs.openhands.dev/openhands/usage/cloud/slack-installation.md | unreviewed |
| Warp | Slack, Linear and GitHub identities map to Warp users; cloud runs billed to team or individual by trigger. | GA | https://docs.warp.dev/platform/team-access-billing-and-identity/ | unreviewed |
| Claude Desktop | Claude Tag and Slack integrations exist as team entry points; only seen in the docs index and platforms page. | GA | https://claude.com/product/cowork | unreviewed |
| rizzy-harness (ours) | None. Routines have webhooks as inbound triggers, not chat channels. | none | not in the inventory (routines: `crates/master/src/routines/`) | from own inventory |

#### Approval presentation (how a permission request reaches a human)
What it is: the prompt that asks a human to allow a tool call: what it shows, what answers exist, and where it can be answered from.
Variants: (a) answers offered: once only (DeepSeek) vs once/session/pattern or always (OpenCode, Zed, Codex); (b) who answers: the local UI, a remote device (Codex, Claude, Antigravity, OpenCode, OpenClaw), a chat channel (OpenClaw), or a reviewer model (Codex Auto-review, Claude Auto, OpenClaw LLM reviewer); (c) whether the request carries the arguments (DeepSeek deliberately omits them); (d) whether the prompt UI can be altered by plugins (Claude Code says no).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | Replies `once`, `always` (saves the tool's proposed patterns as project-scoped allow rules, never overriding a deny) or `reject` (rejects every pending request in the session, with optional feedback); mobile can approve. | GA | https://opencode.ai/docs/permissions/ | reviewed |
| Claude Code | Prompt with options; `Tab` on an option opens a comment field; VS Code tab dot shows a pending permission; permission prompt is not a render site, so no mod can change it; Remote Control lets the phone answer. Cloud sessions offer Auto, Accept edits, Plan only (no Manual, no Bypass). | GA | https://code.claude.com/docs/en/interactive-mode.md, https://code.claude.com/docs/en/plugins/mods/interface.md, https://code.claude.com/docs/en/web-quickstart.md | unreviewed (modes and remote: reviewed) |
| Claude Desktop | Modes Manual, Accept edits, Plan, Auto (classifier), Bypass; Cowork has Manual, Auto (reviewer), Skip; mode remembered per folder for local sessions. | GA | https://code.claude.com/docs/en/desktop, https://support.claude.com/en/articles/13345190-get-started-with-claude-cowork | reviewed (mode list); partly |
| Codex | Patch details can be expanded in the transcript while an approval prompt is open (0.167.0); app-server sends server-initiated approval requests with available decisions; Auto-review can replace the human with a reviewer agent; mobile can approve. | GA | https://learn.chatgpt.com/docs/changelog, https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/mobile.md | reviewed (Auto-review); unreviewed (rest) |
| OpenClaw | Gateway broadcasts `exec.approval.requested` to operator clients; can forward prompts to chat targets answered with `/approve <id> allow-once|allow-always|deny`; native approval UIs on Discord, Slack, Telegram and others check approver lists; resolving needs scope `operator.approvals`; cron approvals reach only connected clients. | GA | https://docs.openclaw.ai/tools/exec-approvals, https://docs.openclaw.ai/tools/exec-approvals-advanced | reviewed |
| Antigravity | In CLI default mode every edit shows an inline diff: accept, reject, or Tab to type an alternative instruction; a pending edit cannot be dismissed with Esc; Remote Control answers approvals from either end. | GA | https://antigravity.google/docs/cli/modes, https://antigravity.google/docs/remote-control | reviewed (Remote Control); unreviewed (CLI) |
| DeepSeek harness | Approval service writes `approval/asked` and `approval/decided` events, waterfall of answerers, anything but allowed-once denies, args omitted from the request, failed audit append rejects. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval | reviewed |
| Zed/ACP | Approval card offers once, always for tool, always for pattern (MCP: tool-level only); ACP standardises four option kinds as UI hints and leaves policy storage to the agent. | GA | https://zed.dev/docs/ai/tool-permissions.md | reviewed |
| Hermes Agent | Smart default mode, 300 s timeout fails closed, headless runs deny. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/security | reviewed |
| Cline | VS Code diffs with Approve/Reject; TUI Shift+Tab auto-approves all; enterprise `yoloModeAllowed:false` disables the toggle org-wide. | GA | https://docs.cline.bot/usage/ide.md, https://docs.cline.bot/enterprise-solutions/configuration/infrastructure-configuration/control-other-cline-features/yolo-mode.md | reviewed (YOLO control) |
| Oh My Pi | Tier-based Read/Write/Exec classes plus ordered per-tool patterns; default mode is `yolo`; `tools.approval.computer: prompt` recommended. | GA | https://omp.sh/docs/approvals, https://omp.sh/docs/computer | reviewed (default yolo) |
| OpenHands | How confirm/reject is presented in Canvas is not documented (Settings says Verification covers approval; conversations page says the confirmation UI is not covered). | unclear | https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md | unverified |
| rizzy-harness (ours) | Mutating call emits `approval_requested`, the turn blocks until the UI answers, cancel or restart denies, every decision is an event; "Allow for this session" actually covers one turn and keys on tool name only (fixed only on `integration/gate3`); no per-command rules or persistent allow. | works (label partial) | `crates/node/src/session.rs:955-1012`, `crates/agent-native/src/lib.rs:1104-1122`, `web/src/components/ToolCard.tsx:234-236` | from own inventory |

Pi: the survey records a deliberate no-permission-system stance (section 5 of `pi.md`); not an approval UI.

#### Diff presentation and review (per-file/hunk accept and reject, comments on diffs)
What it is: showing what the agent changed and letting the human keep, revert or comment on it.
Variants: whole-turn diff vs per-tool-call old/new; whether review actions work per hunk or per file (Zed, Codex desktop, Warp); whether comments go back to the agent (Claude, Codex, Warp, Antigravity, Cline Kanban); what the base is (uncommitted, branch, per turn, agent edits only); whether diffs are computed from the repo (Claude cloud uses raw blobs, ignoring diff drivers).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `/diff` shows the Git diff including untracked files; Desktop diff pane stages or reverts hunks and files, commits, pushes, opens PRs, takes inline comments the agent addresses; separate PR-review desktop plugin. | GA | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/environments/local-environment.md, https://learn.chatgpt.com/docs/code-review.md | unreviewed |
| Claude Desktop | Click line counts on the Changes card; hover a line and `+` to comment; comments are sent with the next message; `/code-review` card with "Walk through in diff" and "Apply fixes" (local, SSH, WSL). | GA | https://code.claude.com/docs/en/desktop | unreviewed (diff review: probably reliable per reviewer) |
| Claude Code | On claude.ai/code: diff vs base or "Compare against" any branch, queued line comments bundled with the next message; before a push only Claude's own edits are listed (changes by commands such as formatters are invisible); computed from raw blobs so repo diff drivers do not apply. Terminal fullscreen has a live `/diff` panel (110 columns minimum). | GA | https://code.claude.com/docs/en/claude-code-on-the-web.md, https://code.claude.com/docs/en/web-quickstart.md | unreviewed |
| Antigravity | App review pane with Uncommitted, Branch, Agent Edits views, per-turn filter, split/stacked, line comments go to the agent, stage/discard/commit/amend/push; discard of unstaged changes is immediate and deletes untracked files. CLI shows inline diffs per edit. | GA | https://antigravity.google/docs/review-changes, https://antigravity.google/docs/cli/modes | unreviewed |
| Zed/ACP | "Review Changes" multi-buffer tab where each hunk or all can be accepted or rejected; `single_file_review` shows keep/reject inline; "Follow" jumps to each touched file. | GA | https://zed.dev/docs/ai/agent-panel | unreviewed |
| Warp | Code Review panel (uncommitted, vs main, vs any branch): attach diffs as context, revert individual hunks, discard all; inline comments sent to running agents including third-party CLI agents; remote repos support commit, push, PR. | GA | https://docs.warp.dev/code/code-review/ | unreviewed |
| Cline | VS Code diffs with Approve/Reject plus checkpoints; Desktop Changes button with per-turn and session diffs; Kanban review shows checkpoint-scoped diffs with inline comments fed back to the agent. | GA (Kanban: preview) | https://docs.cline.bot/usage/ide.md, https://docs.cline.bot/usage/cline-desktop.md, https://docs.cline.bot/usage/kanban.md | unreviewed |
| Cursor | Agents Window diffs view; dedicated Agent Review (Quick and Deep) after agent tasks, reads `BUGBOT.md`. | GA | https://cursor.com/docs/agent/agents-window, https://cursor.com/docs/agent/agent-review | unreviewed |
| OpenHands | Canvas has Files and Changes views; the legacy GUI had a Changes tab. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md | unreviewed |
| OpenClaw | Control UI "Changes (git diff)" panel. | GA | https://docs.openclaw.ai/web/control-ui | unreviewed |
| Hermes Agent | Desktop Git review pane with worktrees. | GA (no stage given) | https://hermes-agent.nousresearch.com/docs/user-guide/desktop | unreviewed |
| Oh My Pi | `/review` with P0-P3 findings and up to 16 parallel reviewer slices; `/annotate` pins notes to diff lines (README only, not on the review page). | GA | https://omp.sh/docs/review | unreviewed |
| OpenCode | Desktop diffs plus quote and annotation comments; desktop labelled beta. | beta / unclear | https://opencode.ai/product, https://opencode.ai/changelog | unreviewed |
| DeepSeek harness | "Changed-file cards" appear as a Coding Tools preference; no review flow described. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-client | unreviewed |
| rizzy-harness (ours) | Shows only the old/new strings of an `edit` call; per-turn "N files changed / Undo / Rewind"; the checkpoint stores the file list but there is no whole-turn diff or file-tree view; no inline comments. | partial | `web/src/components/ToolCard.tsx:125-131`, `crates/core/src/event.rs` (`Checkpoint.files`) | from own inventory |

#### Plan review (artifacts and comments on plans)
What it is: a plan is produced before editing and the human approves, edits or comments on it first.
Variants: plan as a persistent artifact with versions and share link (Warp) vs a one-shot approval dialog (Claude Code, Oh My Pi); inline comments batched into the next message (Antigravity, Claude VS Code); plan as pure display via the protocol (ACP); plan mode as soft guidance only (DeepSeek) vs enforced tool removal.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Approval options (auto, manual edits, keep planning); `Ctrl+G` edits the plan in `$EDITOR`; in VS Code the plan opens as a Markdown document where the user adds inline comments before work starts. | GA | https://code.claude.com/docs/en/permission-modes.md, https://code.claude.com/docs/en/vs-code.md | reviewed (plan gating); unreviewed (VS Code) |
| Claude Desktop | Dedicated plan, tasks and subagent panes and a plan box at the end of turns. | GA | https://code.claude.com/docs/en/desktop, https://claude.com/docs/cowork/changelog.md | unreviewed |
| Antigravity | `/plan` drafts an Implementation Plan artifact; comments attach to the next message as a chip ("2 Comments"); `/plan` always stops for approval; `artifactReviewPolicy` (asks-for-review, agent-decides, always-proceed) applies elsewhere. | GA | https://antigravity.google/docs/plan, https://antigravity.google/docs/implementation-plan, https://antigravity.google/docs/artifact-review | unreviewed |
| Oh My Pi | Full-screen Plan Review with annotate/delete/edit-in-`$EDITOR`; approve options include execute in a fresh session with only the plan, compact then implement, keep context. | GA | https://omp.sh/docs/plan | unreviewed |
| Cursor | Plan mode asks questions, researches, writes an editable plan, user clicks to build; "Build in Parallel". | GA | https://cursor.com/docs/agent/plan-mode | unreviewed |
| Warp | Editable plan in a rich-text editor with restorable version history, run all or a section, sync to Warp Drive, share by link. | GA | https://docs.warp.dev/agents/capabilities/planning/ | unreviewed |
| DeepSeek harness | `exit_plan_mode` requires a Markdown plan presented for review; approved, changes requested and dismissed are distinct outcomes; plan mode is soft guidance, sandbox and approval do not read it. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/plan | unreviewed |
| Zed/ACP | ACP `plan` update carries entries (content, priority, status); each update replaces the whole plan. Display only. | GA | https://agentclientprotocol.com/protocol/v1/agent-plan.md | unreviewed |
| Codex | `/plan` switches to plan mode; no review UI described. | GA | https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| OpenCode | `plan` agent edits only under `~/.opencode/plan`; no review artifact described. | GA | https://opencode.ai/docs/agents/ | unreviewed |
| rizzy-harness (ours) | None. `ui:progress` is a display card, not a plan with approval; closest is `ask_all` and read-only Scout/Reviewer agents (the latter on an unmerged branch). | none | `crates/core/src/ui/progress.rs` | from own inventory |

Deliberate omission: Pi (plans belong in files). Nothing documented: Hermes, goose, OpenHands (a `task_tracker` tool only), OpenClaw.

#### Notifications and "waiting for you" signals
What it is: telling the human that a run finished, errored or is blocked on an approval or question, especially when the tab or terminal is not focused.
Variants: terminal escape sequences and bell (Claude Code); push to a phone with presence suppression (Claude); Live Activities (Cursor); a list view that groups sessions by state (Claude agent view "Needs input"); toast/mailbox/tab badges (Warp).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Desktop notification in Ghostty/Kitty/iTerm2, bell elsewhere; OSC 7501 program-status protocol reports working/waiting/done; hooks can return an allowlisted `terminalSequence`; Remote Control pushes to the phone when a task ends or a decision is needed, with two `/config` toggles, suppressed while the terminal is focused, extendable with `CLAUDE_CLIENT_PRESENCE_FILE`. | GA | https://code.claude.com/docs/en/terminal-config.md, https://code.claude.com/docs/en/hooks.md, https://code.claude.com/docs/en/remote-control.md | unreviewed |
| Claude Code (agent view) | `claude agents` groups background sessions as Pinned, Ready for review, Needs input, Working, Completed; peek panel shows the pending question and lets you reply. | research preview | https://code.claude.com/docs/en/agent-view.md | corrected ("Introduced v2.1.139" is not on the page) |
| Cursor | iOS push and Live Activities for up to eight agents. | GA (no label) | https://cursor.com/docs/cloud-agent/mobile | reviewed (partly) |
| Warp | Agent notifications for complete, request, error: toasts, mailbox, tab badges; for Claude Code, Codex and OpenCode via plugins. | GA | https://docs.warp.dev/agents/capabilities/agent-notifications/ | unreviewed |
| Claude Desktop | Push notifications when Remote Control is active; Dispatch-spawned sessions push. | GA | https://code.claude.com/docs/en/mobile | unreviewed |
| OpenCode | Desktop read/unread marking; mobile composer. | beta | https://opencode.ai/changelog | unreviewed |
| Codex | A notifications page exists but was only skimmed; nothing recorded. | unclear | https://learn.chatgpt.com/docs/notifications.md | unverified |
| rizzy-harness (ours) | Browser Notifications when a run finishes (secure context only); an "Attention" list for pending approvals. | works | `web/src/lib/notify.ts:1-30`, `web/src/components/Attention.tsx` | from own inventory |

#### Managing several agents at once
What it is: one place to launch, watch and switch between multiple concurrent agent sessions.
Variants: a command-centre UI over many sessions (Antigravity, Cursor, Warp, Claude agent view); a board of cards each running an agent in its own worktree (Cline Kanban, Hermes kanban); a per-user supervisor daemon hosting sessions (Claude); client-side federation to many backends (OpenHands, Hermes Desktop).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `claude agents` full-terminal dashboard of background sessions across projects; per-user supervisor daemon hosts each as its own process; idle processes reaped after about an hour and resumed from disk. | research preview | https://code.claude.com/docs/en/agent-view.md | corrected |
| Claude Desktop | Session sidebar and rail, parallel worktree sessions, cross-session tools letting the agent list/read/message/archive other desktop sessions (admin key `crossSessionInbound`), cloud Projects coordinator. | GA (Projects: beta) | https://code.claude.com/docs/en/desktop, https://code.claude.com/docs/en/claude-projects | reviewed (Projects) |
| Antigravity | App is a "command centre" to launch and monitor agents across projects; Overview panel lists changes, artifacts, subagents; one git worktree per agent session. | GA | https://antigravity.google/docs/overview, https://antigravity.google/docs/projects | unreviewed |
| Cursor | Agents Window: multi-workspace, diffs, cloud handoff, worktrees; web and iOS view the same agents. | GA | https://cursor.com/docs/agent/agents-window | unreviewed |
| Codex | Desktop projects with parallel chats (Local, Worktree, Cloud); 0.167.0 notes mention `codex agents --no-daemon` while the CLI reference says unsupported. | GA | https://learn.chatgpt.com/docs/app.md, https://learn.chatgpt.com/docs/changelog | unreviewed |
| Warp | Agent Management Panel with notifications; third-party CLI agents appear in vertical tabs. | GA | https://docs.warp.dev/platform/managing-cloud-agents/ | unreviewed |
| Zed/ACP | Threads Sidebar: several threads (Zed Agent, external agents, terminal threads) grouped by project; no numeric limits documented. | GA | https://zed.dev/docs/ai/parallel-agents.md | unreviewed |
| Hermes Agent | Kanban board (SQLite, separate OS process per worker, dispatcher tick about 60 s) plus Desktop live subagent monitor and multi-gateway connections. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/kanban | reviewed |
| Cline | Kanban preview: cards each run a CLI agent (including Claude Code, Codex, OpenCode) in its own worktree; Desktop parallel sessions. | preview | https://docs.cline.bot/usage/kanban.md | unreviewed |
| OpenHands | Conversation list with tags, pins, archive; child conversations in Canvas. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md | unreviewed |
| OpenClaw | Chat with nested child sessions, Subagents and background-process panels, agent selector and team mode. | GA | https://docs.openclaw.ai/web/control-ui | unreviewed |
| Oh My Pi | Agent hub (Alt+A) in the TUI. | GA | https://omp.sh/docs/using | unreviewed |
| OpenCode | Desktop tabs and sessions. | beta | https://opencode.ai/product | unreviewed |
| rizzy-harness (ours) | Sidebar with Recent, Agents, Projects and an attention list for approvals; many chats per user; no overview of running agent sessions as a board. | partial | `web/src/components/Sidebar.tsx`, `web/src/components/Attention.tsx` | from own inventory |

---

### Area 16. Browser automation and computer use

#### The agent's own browser (embedded or managed)
What it is: a browser the harness starts and controls (headless or visible), separate from the user's day-to-day browser.
Variants: an embedded pane in the desktop app (Codex, Claude Desktop, OpenCode, Antigravity, Cursor); a managed Chromium profile (Oh My Pi, OpenClaw, Hermes); a library-driven tool set (OpenHands browser-use, DeepSeek Playwright/Stagehand providers); where the browser runs relative to the loop (same host vs a cloud VM).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Built-in browser on the desktop app only (not CLI or IDE); page content treated as untrusted. | GA | https://learn.chatgpt.com/docs/browser.md | unreviewed |
| Claude Desktop | Tabbed browser pane with a clean profile separate from the user's browser; Claude starts dev servers, takes screenshots, inspects DOM ("Auto-verify"); `.claude/launch.json` lists dev servers. | GA | https://code.claude.com/docs/en/desktop, https://claude.com/docs/third-party/claude-desktop/browser.md | corrected |
| OpenCode | `browser` namespace in the desktop app: navigation, snapshots, forms, screenshots, console/network, traces, Lighthouse; no per-operation prompts; deny `browser` `*` removes it. | GA (desktop) | https://opencode.ai/docs/tools/, https://opencode.ai/product | unreviewed |
| Cursor | Web view pane controlled by an MCP server shipped as an extension; navigate, click, type, screenshot, console, network; a built-in Browser subagent filters noisy DOM output. | GA | https://cursor.com/docs/agent/tools/browser, https://cursor.com/docs/subagents | unreviewed |
| Antigravity | App side-panel browser with separate cookies per Antigravity account; Comment and Quote send page context to the agent. | GA (App v2.28.0) | https://antigravity.google/docs/features, https://antigravity.google/docs/ide/separate-chrome-profile | unreviewed |
| Oh My Pi | `browser` tool (on by default): headless Chromium downloaded on first use, omp profile, or visible; attach via `browser.cdpUrl`. | GA | https://omp.sh/docs/web, https://omp.sh/docs/tools | unreviewed |
| OpenClaw | Dedicated Chrome/Brave/Edge/Chromium `openclaw` profile controlled by a loopback-only service in the Gateway; tabs, snapshots, PDFs, downloads; remote and hosted CDP; node browser proxy needs admin approval. | GA | https://docs.openclaw.ai/tools/browser | unreviewed |
| Hermes Agent | `browser_exec`: model writes Python against helpers (`goto_url`, `js`, `click_at_xy`, `cdp`); backends local headless Chromium (fresh profile, 120 s idle close), cloud providers, Lightpanda. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/browser | unreviewed |
| OpenHands | `BrowserToolSet` on browser-use: navigate, click, fill forms, extract; optional VNC through the Docker workspace; "Docker browser" setting. | GA | https://docs.openhands.dev/sdk/guides/agent-browser-use.md | unreviewed |
| DeepSeek harness | Experimental browser seam with Playwright MCP, Chrome DevTools MCP and a native Stagehand provider. | experimental | https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams | unreviewed |
| Warp | Every cloud computer-use environment bundles Chromium and the Playwright CLI; screenshot or scripted modes; recordings capped at 10 minutes and 1 GiB. | GA | https://docs.warp.dev/agents/capabilities/computer-use/browser-use/ | unreviewed |
| goose | Computer Controller extension (web scraping, automation); browser automation by installing the upstream browser-use skill; no dedicated page. | unclear | https://goose-docs.ai/docs/getting-started/using-extensions.md | unreviewed |
| Cline | No current docs page for a browser tool; auto-approve lists a "Use the browser" category; legacy page now redirects. | unclear | https://docs.cline.bot/features/auto-approve.md, https://docs.cline.bot/exploring-clines-tools/remote-browser-support | unverified |
| rizzy-harness (ours) | None; agentic nodes have shell and file tools only. | none | not in the inventory (obvious-absences table: "Browser/computer use ... Truly absent") | from own inventory |

Nothing documented: Pi (a third-party `pi-web-access` package searches and fetches), Zed/ACP, Claude Code core (CLI uses the Chrome extension; no built-in browser).

#### The user's real browser (extension, DevTools attach)
What it is: driving the browser the user already uses, with its logged-in sessions, through an extension or the DevTools protocol.
Variants: an extension plus native-messaging host and a vendor bridge (Claude in Chrome, Codex); an extension with a local relay (Oh My Pi, 127.0.0.1:9224); DevTools MCP attach to the user's own Chrome after a Chrome toggle (Antigravity, OpenClaw `user` profile, Hermes `/browser connect`); none of these hold a separate profile, so cookies and sign-ins are shared with the agent.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Claude in Chrome extension (v1.0.36+) drives real Chrome/Edge/Brave and others through a native-messaging manifest and `bridge.claudeusercontent.com`; tabs go in a session tab group; pauses on login/CAPTCHA; uploads capped at 10 MB and refuse credential-named paths; same claude.ai account and org required; not in WSL. | GA | https://code.claude.com/docs/en/chrome.md | reviewed |
| Claude Desktop | Same extension connected to the app; shortcuts and scheduled tasks saved in the extension are lost on logout. | GA (extension 1.0.36+) | https://code.claude.com/docs/en/chrome | reviewed |
| Codex | Operates existing Chrome tabs through the ChatGPT browser extension; full CDP access ("Developer mode"); each can be switched off by requirement. | GA (as admin-controllable features) | https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| Antigravity | Hidden `browser` subagent drives the user's running Chrome (v144+) via Chrome DevTools MCP started by `npx`; user enables `chrome://inspect/#remote-debugging`; no extension, no separate profile (existing cookies and sign-ins). | GA | https://antigravity.google/docs/commands/browser, https://antigravity.google/docs/ide/browser | reviewed |
| Oh My Pi | Browser Relay Chrome extension (`omp browser-relay install`) on 127.0.0.1:9224 with optional token; cannot attach to chrome:// or DevTools tabs; no context isolation. | GA | https://omp.sh/docs/web | unreviewed |
| OpenClaw | Built-in `user` profile attaches to the signed-in Chrome via DevTools MCP. | GA | https://docs.openclaw.ai/tools/browser | unreviewed |
| Hermes Agent | `/browser connect` to your own Chrome over CDP (CLI only); opt-in snapshot of the real profile. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/browser | unreviewed |
| OpenCode | A separate product "Browse" (agents using your real browser) is listed on the product page only. | planned / vendor-claim | https://opencode.ai/product | unverified |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

#### Browser isolation, site policy and credential handling (what the docs say)
What it is: statements about what a browser tool can reach and what limits apply: separate storage, per-site approval, origin lists, SSRF rules, credential filling.
Variants: per-site allow/ask/deny prompts (Claude Desktop, Antigravity, Cursor); org-level domain lists (Claude, Antigravity, Cursor, Codex); credential vaults that fill without the model seeing secrets (Hermes); no isolation claimed (Oh My Pi relay, Hermes real profile); "sandboxed" used loosely for a live-profile browser (Antigravity).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Desktop | Clean profile separate from the user's browser; per-site Allow once / Always allow for external sites (a Deny choice was not found); Keep cookies and Clear browsing data; cookies persist in Cowork until cleared but Code sessions clear them at app quit unless set to Shared or Per session; Anthropic site-safety check; admin domain allow/block lists shared with the Claude in Chrome policy. | GA | https://claude.com/docs/third-party/claude-desktop/browser.md, https://code.claude.com/docs/en/desktop | corrected |
| Antigravity | Allow/Ask/Deny URL list (domain and subdomains, not paths; deny beats allow; Google data sites need their own rules); JavaScript policy Request Review (default), Always Proceed or Disabled; separate Network Access Rules for page reads; admin can set allowed domains or disable browser access. The slash-command catalog calls the subagent "sandboxed Chrome" while its own pages describe the user's live profile. | GA | https://antigravity.google/docs/ide/allowlist-denylist, https://antigravity.google/docs/commands/browser, https://antigravity.google/docs/settings | reviewed |
| Cursor | Approval modes manual/allowlist/auto-run; per-session auth token and per-tab ids; storage isolated per workspace; Enterprise origin allowlist (must be enabled by the account team), described as best-effort. | GA | https://cursor.com/docs/agent/tools/browser | unreviewed |
| Codex | Admins can restrict origins, uploads, downloads and developer tools; page content treated as untrusted. | GA | https://learn.chatgpt.com/docs/browser.md | unreviewed |
| Claude Code | Extension site permissions inherited; project settings cannot enable it; off by default in HIPAA orgs; managed `deniedMcpServers` or an exclusive `managed-mcp.json` suppresses the install prompt; browser tool definitions raise context use when on by default. | GA | https://code.claude.com/docs/en/chrome.md | reviewed |
| Hermes Agent | `browser_vault` fills logins, cards and 2FA without the model seeing them, refuses on origin mismatch (re-checked in-page), never submits, card fills always need approval; docs say `browser_exec` runs model-written Python on your machine and real-profile browsing is not an isolation boundary. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/credential-vault, https://hermes-agent.nousresearch.com/docs/user-guide/features/browser | unreviewed |
| OpenClaw | Loopback-only control service; isolated `openclaw` profile; strict-by-default SSRF policy; node browser proxy needs admin approval. | GA | https://docs.openclaw.ai/tools/browser, https://docs.openclaw.ai/gateway/security | unreviewed |
| OpenCode | No per-operation prompts; policy can deny the `browser` namespace. | GA | https://opencode.ai/docs/tools/ | unreviewed |
| Oh My Pi | Relay: optional token, cannot attach to chrome:// tabs, no context isolation. | GA | https://omp.sh/docs/web | unreviewed |
| OpenHands | Docker workspace with optional VNC; per-conversation Docker browser setting. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/backend-setup/docker-execution.md | unreviewed |
| DeepSeek harness | Sandboxing of browser tools not documented. | experimental | https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams | unverified |
| rizzy-harness (ours) | No browser tool. Related rendering safety: no remote images, links show real host. | none (browser) | `crates/core/src/ui/mod.rs:56-83` | from own inventory |

#### Computer use (screen control)
What it is: the agent sees screenshots and sends clicks and keystrokes to a desktop or app.
Variants: local desktop with per-app tiers and a session lock (Claude); local with per-app rules and signed publisher matching (Codex); background control without moving the cursor via accessibility APIs (Hermes); cloud-VM-only (Warp, Cursor cloud); an MCP server with a lock, hide-other-apps and Esc abort. Platform coverage differs: Claude Code CLI is macOS only, Claude Desktop adds Windows, nobody documents Linux except OpenClaw/Oh My Pi/Hermes (varying completeness).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Built-in MCP server `computer-use`, off by default, per project via `/mcp`; macOS Accessibility and Screen Recording; one session at a time holds a lock; other apps hidden while working; terminal excluded from screenshots; Esc aborts and is consumed; screenshots downscaled; Pro/Max/Ultra only, interactive only. "Flags potential prompt injection" has no mechanism described. | research preview | https://code.claude.com/docs/en/computer-use.md | reviewed |
| Claude Desktop | Same engine plus Windows; app tiers View only (browsers, trading), Click only (terminals, IDEs), Full control; approvals last the session (30 min for Dispatch-spawned); denied-apps list in Desktop only; not Linux, not Team/Enterprise, not 3P. | research preview | https://code.claude.com/docs/en/desktop, https://code.claude.com/docs/en/computer-use | reviewed |
| Codex | Operates GUIs on macOS and Windows from the desktop app with per-app rules; Windows rules match signed publisher and product; app-level prompts go to the user, individual tool calls can go to Auto-review. | GA (region-dependent) | https://learn.chatgpt.com/docs/computer-use.md, https://learn.chatgpt.com/docs/sandboxing/auto-review.md | unreviewed |
| Cursor | Cloud agent VMs have a desktop, user can take over via remote desktop; self-hosted workers use `--computer-use` (macOS helper app or Linux X11 with xdotool/ffmpeg) and `--share-desktop` (Linux; isolated desktop, input filter, clipboard blocked); admins can disable it. | GA (no label) | https://cursor.com/docs/cloud-agent/capabilities, https://cursor.com/docs/cloud-agent/self-hosted/computer-use | unreviewed |
| Oh My Pi | `computer` tool off by default (`/computer on`): accessibility tree, screenshots, mouse/keyboard; macOS, Linux X11, Windows x64 full, Wayland limited; `tools.approval.computer: prompt` recommended; on-screen text cannot grant authorization. | GA | https://omp.sh/docs/computer, https://omp.sh/docs/approvals | unreviewed |
| OpenClaw | One `computer` tool for the gateway desktop or a paired node; actions carry a `frameId` and are blocked on stale frames; macOS Peekaboo; Windows/Linux experimental; no per-action confirmation; screenshots go only to the model. | GA (macOS), experimental (Windows/Linux) | https://docs.openclaw.ai/nodes/computer-use | reviewed |
| Hermes Agent | Background desktop control without cursor movement or focus steal on macOS (AX), Windows (UIAutomation), Linux (AT-SPI) via cua-driver over MCP stdio; modes standard approvals, bounded (reviewed capability manifest), unrestricted; hard-blocked key combos; relies on private SkyLight APIs on macOS. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/computer-use | unreviewed |
| Warp | Cloud sandbox environments only (not local sessions); on by default for the Warp harness, off for third-party harnesses; app-started runs follow an off-by-default setting; Anthropic Claude models. | GA | https://docs.warp.dev/agents/capabilities/computer-use/ | reviewed |
| DeepSeek harness | Experimental computer-use seam with `cua-driver` providers. | experimental | https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams | unreviewed |
| goose | `computercontroller__computer_control` named in adversary-mode docs for macOS UI automation; no dedicated page. | unclear | https://goose-docs.ai/docs/guides/security/adversary-mode.md | unreviewed |
| rizzy-harness (ours) | None. | none | not in the inventory | from own inventory |

Nothing documented: OpenCode (explicitly: "computer-use: nothing documented"), OpenHands, Cline, Pi, Zed/ACP, Antigravity.

---

### Area 18. Collaboration, teams and enterprise administration

#### Accounts, roles, organisations and workspaces
What it is: more than one human using the same harness with separate identities, roles and scopes.
Variants: org/role model in a hosted control plane (Claude, Cursor, OpenHands Cloud, Warp, Cline Enterprise); a self-hostable multi-user mode (OpenClaw, ours); per-client keys on a gateway (OpenCodex); chat-platform admins (Hermes). Key differences: whether chats are private to their owner (ours states it as an invariant; OpenHands notes chats are not documented as private; OpenClaw says visibility is a usability feature, not a security boundary).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenClaw | Multi-user mode: session creator (immutable), owner (assignable), participants; presence; mentions inbox; per-person model credentials; named roles under `gateway.roles.definitions` (visibility of others' sessions none/view/suggest/write, allowed agents, scope ceiling, sandbox inherit/required, model policy); docs say scopes are not tenant isolation. | GA | https://docs.openclaw.ai/concepts/multi-user, https://docs.openclaw.ai/gateway/operator-scopes | reviewed (scopes and roles); unreviewed (multi-user) |
| OpenHands | Cloud roles Member, Admin, Owner (shared org credits, org-wide LLM, claim Git orgs); share links public or org-only; chats not documented as private to their owner. Open tier: no authentication, RBAC or multi-user orgs. | GA (Cloud); none in open tier | https://docs.openhands.dev/openhands/usage/cloud/organizations/roles-permissions.md, https://docs.openhands.dev/enterprise/enterprise-vs-oss.md | reviewed (matrix) |
| OpenCode | OpenCode Console workspaces with members (commercial, optional). Self-hosted use: single `opencode` user and one password; no per-user accounts documented. | GA (commercial service) | https://opencode.ai/docs/console/ | unreviewed |
| Claude Code | Team and Enterprise orgs with Owner/Admin and custom roles (cloud sessions need a permission); Enterprise RBAC can toggle features per role. Claude apps gateway authorises each request from IdP token claims and keeps no user directory. | GA | https://code.claude.com/docs/en/feature-availability.md, https://code.claude.com/docs/en/claude-apps-gateway.md | unreviewed |
| Claude Desktop | Org admin toggles for Desktop, cloud sessions and Remote Control; owners govern plugins and extensions. | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| Cursor | Org groups, pooled usage, service accounts, billing groups (page titles from the index; details only where cited). | GA | https://cursor.com/llms.txt | unreviewed |
| Warp | Roles Team Owner, Team Admin, Member; Admin Panel page says only the team creator is the designated admin (pages may describe different tiers). | GA (Enterprise) | https://docs.warp.dev/knowledge-and-collaboration/admin-panel/, https://docs.warp.dev/enterprise/security-and-compliance/security-overview/ | unreviewed |
| Cline | Enterprise role hierarchy Member/Admin/Owner, per-team model and tool access. | GA | https://docs.cline.bot/enterprise-solutions/overview.md | unreviewed |
| Zed/ACP | Zed Business: org-wide admin controls, roles, consolidated billing. | GA | https://zed.dev/docs/account/plans-and-pricing.md | unreviewed |
| Antigravity | Enterprise via Google Cloud project billing or Gemini Enterprise seats with IAM roles `agentspaceAdmin` and `agentspaceUser`. | GA | https://antigravity.google/docs/enterprise | unreviewed |
| Hermes Agent | Per-platform and per-scope admins versus regular users with a command allowlist in chat; profiles. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/messaging, https://hermes-agent.nousresearch.com/docs/user-guide/profiles | unreviewed |
| OpenCodex | Per-client keys with usage scoped by `apiKeyId`, a Tailscale-identity allowlist for dashboard sessions; no SSO, teams or RBAC. | GA (hub) | https://opencodex.me/guides/remote-hub/, https://opencodex.me/guides/web-dashboard/ | corrected (pairing code single-use, five-minute lifetime) |
| Codex | `forced_login_method` and `forced_chatgpt_workspace_id` restrict sign-in; cloud environments shareable within a workspace; no role model read. | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md, https://learn.chatgpt.com/docs/auth.md | unreviewed |
| DeepSeek harness | Nothing documented for multi-user: one launch token plus cookie for the one process; gateway "Peers" are in-process principals. | unclear | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/web-server | reviewed |
| rizzy-harness (ours) | First admin by one-time claim code; admin-created accounts (Argon2id, no self-signup); server-side sessions; personal plus team workspaces with owner/admin/member/guest roles mapped to `agentic`, `settings`, `members`, `routines`; per-user `can_agentic`; chats private to their owner in every workspace, admins included. | works | `crates/master/src/auth.rs:182-196`, `crates/master/src/workspaces.rs:18-110`, `crates/master/src/auth.rs:138-145` | from own inventory |

Nothing documented: Oh My Pi, Pi (a Fair Source and proprietary enterprise tier is announced, not built), goose.

#### Single sign-on and provisioning
What it is: login through the company identity provider, plus automatic account creation and removal (SCIM).
Variants: SAML/OIDC on the account plane (Claude, Cursor, OpenCode, Warp, OpenHands Enterprise, Cline); IdP tokens on a self-hosted gateway with no user directory and therefore no SCIM (Claude apps gateway); workforce identity (Antigravity); OIDC only on the local dashboard (Hermes).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | SSO on Team/Enterprise, SCIM and Compliance API Enterprise only (plan table). Claude apps gateway (built into the `claude` binary, self-hosted): corporate OIDC IdP, device-grant sign-in, short-lived bearer tokens, no user directory and no SCIM endpoint; group changes apply at the next token refresh. | GA | https://code.claude.com/docs/en/feature-availability.md, https://code.claude.com/docs/en/claude-apps-gateway.md, https://code.claude.com/docs/en/claude-apps-gateway-config.md | unreviewed |
| Claude Desktop | SSO on Team/Enterprise; a separate Government admin guide mentions SCIM (titles only). | GA; unclear (Government) | https://code.claude.com/docs/en/desktop, https://claude.com/docs/llms.txt | unreviewed |
| OpenCode | SSO via OIDC or SAML 2.0 with domain verification and enforcement (included in every workspace); SCIM directory sync (page not opened). | GA (commercial service) | https://opencode.ai/docs/console/sso/ | unreviewed |
| Cursor | SSO and SCIM listed (index titles; SCIM and SSO pages not opened); Enterprise plan carries SCIM. | GA | https://cursor.com/llms.txt, https://cursor.com/docs/models-and-pricing | unreviewed |
| OpenHands | Enterprise SAML SSO (Okta, Entra ID, Google Workspace, ADFS; signed assertions with an email attribute), Keycloak brokering; group-to-role mapping not documented. | GA (commercial) | https://docs.openhands.dev/enterprise/integrations/saml-sso.md | reviewed (architecture parts); SAML specifics not re-checked |
| Warp | SSO (Okta, Entra ID, Google Workspace, OneLogin; SAML 2.0 and OIDC) with MFA through the IdP; SCIM only named. | GA (Enterprise) | https://docs.warp.dev/enterprise/security-and-compliance/security-overview/ | unreviewed |
| Cline | SSO listed for Enterprise; SSO setup and member pages not opened. | GA | https://docs.cline.bot/enterprise-solutions/overview.md | unreviewed |
| Antigravity | SSO/BYOID via Workforce Identity Federation. | GA | https://antigravity.google/docs/enterprise | unreviewed |
| Hermes Agent | Dashboard auth gate offers username/password, Nous Portal OAuth or self-hosted OIDC; no SSO or audit product beyond that. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/web-dashboard | unreviewed |
| OpenClaw | Gateway auth via token/password, Tailscale Serve identity, trusted-proxy headers; roles assignable by GitHub login mapping. | GA | https://docs.openclaw.ai/gateway/pairing, https://docs.openclaw.ai/gateway/operator-scopes | reviewed (roles) |
| Codex | Enterprise access tokens, service accounts, OIDC or SPIFFE workload identity variables; pages only skimmed. | GA | https://learn.chatgpt.com/docs/auth.md | unreviewed |
| rizzy-harness (ours) | None (SSO/OIDC/SCIM). | none | not in the inventory ("SSO / OIDC / SCIM: None") | from own inventory |

#### Managed configuration and policy distribution
What it is: an admin sets rules that users and repositories cannot override, delivered by file, MDM, a server, or a self-hosted config service.
Variants: ranking and merge rules among sources (Claude first-wins by default, merge opt-in; Codex MDM over Agent Security over file); fail-open vs fail-closed on fetch failure (Claude default fail-open with opt-in fail-closed); gaps per surface (Cline's remote config does not reach CLI or Desktop; Claude's server-managed settings do not reach Cowork or API-key fleets); policy computed by an executable (Claude `policyHelper`); who decides at the node (Codex `requirements.toml`, OpenCode policies, ours `node.toml`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `requirements.toml`, MDM and Agent Security enforce allowed approval policies, sandbox modes, reviewers, permission profiles (omitted ones are denied, including future built-ins), deny-read paths, command rules, pinned features, managed hooks; self-hosted `[config_server]` fetches defaults and requirements from an HTTPS server the organisation runs, optionally behind company OAuth. Plan tier not stated in the survey. | GA | https://learn.chatgpt.com/docs/enterprise/managed-configuration.md, https://learn.chatgpt.com/docs/enterprise/configuration-server.md | unreviewed (headings only for the managed page) |
| Claude Code | Four sources ranked server-managed > MDM > file > HKCU; default first-wins (silently ignores lower sources, `/status` shows skipped), `managedSourcesBehavior: "merge"` composes them; drop-in directory merge rules; server-managed fetched at startup and polled hourly, fail-open by default (`forceRemoteSettingsRefresh` for fail-closed); approval dialog for server-delivered risky settings; version floor and ceiling; computed `policyHelper`; "client-side control, not a security boundary". | GA | https://code.claude.com/docs/en/server-managed-settings.md, https://code.claude.com/docs/en/managed-settings.md | reviewed (delivery, ranking, server-managed) |
| Claude Code (gateway) | Self-hosted gateway delivers managed settings per IdP group: `managed.policies` first-match merged onto a catch-all base; `availableModels` also enforced at `/v1/messages` (400), per-upstream groups. | GA | https://code.claude.com/docs/en/claude-apps-gateway-config.md | unreviewed |
| Claude Desktop | Admin console toggles; managed keys delivered via macOS `com.anthropic.claudefordesktop`, Windows `SOFTWARE\Policies\Claude`, Linux `/etc/claude-desktop/managed-settings.json`; Cowork sessions do not fetch console settings; a broken managed file blocks local Code sessions (v3.1623.0). | GA | https://code.claude.com/docs/en/desktop | unreviewed |
| OpenCode | Admin-only `policies` layer above user and project files that only tightens (a policy allow can lift an earlier policy deny), delivered by system/MDM/Console; "always" approvals cannot override it. | GA | https://opencode.ai/docs/policies/ | corrected |
| Cursor | MDM-delivered policy (hooks, workspace trust, allowed team IDs); hook merging across enterprise/team/project/user/plugin sources (permission deny beats ask beats allow, other fields last-response-wins); admin controls over run modes, sandbox network, MCP allowlists, browser allowlist. | GA (page titles from index) | https://cursor.com/docs/hooks, https://cursor.com/docs/agent/security/run-modes | corrected (hook merge description) |
| Warp | Admin Panel enforces AI in remote sessions, apply diffs, plans, command execution, file reads, directory allowlist, regex command allow/deny, secret redaction, codebase context, spend limits; Free mostly fixed, Business mostly toggleable, Enterprise full control; admin denylist is non-bypassable. | GA | https://docs.warp.dev/knowledge-and-collaboration/admin-panel/ | unreviewed |
| Cline | Remote configuration from an admin console pushes rules, workflows, skills, allowed providers, MCP controls, telemetry lock; VS Code/JetBrains get all, CLI only rules and workflows, Desktop none. Enterprise policy is therefore not enforced on CLI/Desktop. | GA | https://docs.cline.bot/enterprise-solutions/configuration/remote-configuration/overview.md | reviewed |
| Antigravity | Org allowed browser domains, browser disable, admin opt-in for Claude, request/response logging into the customer's Cloud project; hook-driven overrides. | GA | https://antigravity.google/docs/enterprise | unreviewed |
| OpenHands | Enterprise org-wide LLM, custom sandbox images, plugin marketplace; open tier has none of that. | GA (commercial) | https://docs.openhands.dev/enterprise/enterprise-vs-oss.md | reviewed |
| goose | Extension allowlist (exact command match) delivered from a URL the admin hosts; custom distributions; env-var defaults. | unclear | https://goose-docs.ai/docs/guides/allowlist.md, https://goose-docs.ai/docs/guides/custom-distributions.md | reviewed (allowlist) |
| Hermes Agent | Managed scope for fleet baselines, remote config plane, profile distributions (git-installable agents carrying config, skills, cron, MCP, never credentials or memory); remote config page not opened. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/managed-scope, https://hermes-agent.nousresearch.com/docs/user-guide/profiles | unreviewed |
| OpenClaw | `openclaw policy` authors and checks policy files with scoped overlays and attestation; details not read. | unclear | https://docs.openclaw.ai/gateway/security | unverified |
| rizzy-harness (ours) | `node.toml` on each node is the authority (clamps approval mode, workspace roots, denied tools); no HTTP or protocol endpoint writes it; no mechanism to distribute policy from the master to nodes. | works (local authority only) | `crates/node/src/policy.rs:9-105`, `crates/master/src/config.rs:149-165` | from own inventory |

Nothing documented: Oh My Pi (closest: `--config` overlay files, `PI_CONFIG_FILES`), Pi, DeepSeek harness, Zed/ACP, OpenCodex.

#### Shared team resources (rules, prompts, servers, plugins)
What it is: things a team defines once and every member gets: instruction files, prompt libraries, MCP servers, skills, plugin marketplaces, environments.
Variants: a synced workspace of objects with sharing roles (Warp Drive); config pushed from an admin console (Cline, Claude); a git repo as the sharing mechanism (Antigravity `.agents/`, Claude repo-required plugins); agents distributed as git-installable profiles (Hermes); shared cloud environments with network secrets (Claude, Codex, Cursor).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Warp | Warp Drive: personal and team workspaces for Workflows, Notebooks, Prompts, environment variables (dynamic via 1Password/LastPass), Rules, MCP servers; share with team, individuals or link; roles can view/can edit/full access, inherited from folders; moving to a team workspace is one-way; secrets cannot be saved in plain text. | GA | https://docs.warp.dev/knowledge-and-collaboration/warp-drive/, https://docs.warp.dev/support-and-community/privacy-and-security/secret-redaction/ | unreviewed |
| Claude Code | Managed MCP (exclusive `managed-mcp.json`, `managedMcpServers` for every user, allow/deny lists), org plugin policy and private marketplaces, org-wide CLAUDE.md, plugins synced from claude.ai accounts, repo-required plugins, org-shared cloud environments with network secrets, per-tool `ask`/`blocked` controls on connectors. | GA | https://code.claude.com/docs/en/managed-mcp.md, https://code.claude.com/docs/en/plugins/org.md, https://code.claude.com/docs/en/cloud-environments.md | unreviewed |
| Claude Desktop | Owners set per-plugin availability, sync plugin repos, allow/deny directory extensions, private marketplaces; system policy overrides in-app lists. | GA | https://claude.com/docs/plugins/platform-support.md | reviewed (plugin matrix) |
| Cline | Remote config pushes rules, workflows, skills and MCP controls (`allowedMCPServers`, `remoteMCPServers`, `blockPersonalRemoteMCPServers`). | GA | https://docs.cline.bot/enterprise-solutions/configuration/infrastructure-configuration/control-other-cline-features/mcp-server-controls.md | reviewed |
| Hermes Agent | Profile distributions as git-installable agents; managed scope for fleet baselines. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/profiles | unreviewed |
| Cursor | Team and enterprise hooks, MCP allowlists, shared cloud environments (page titles); Team Pools are Enterprise-only. | GA | https://cursor.com/docs/mcp, https://cursor.com/docs/cloud-agent/self-hosted | reviewed (Team Pools plan) |
| Antigravity | No shared-session feature; `.agents/` in git is the sharing mechanism. | GA (as described) | https://antigravity.google/docs/enterprise | unreviewed |
| Codex | Cloud environments can be shared within a workspace; Slack delegation needs a workspace-shared environment. | GA | https://learn.chatgpt.com/docs/environments/cloud-environments.md | reviewed |
| OpenCode | Console team-wide policies pushed to every connected client; plugins can be withheld by managed policy. | GA (commercial) | https://opencode.ai/docs/policies/ | corrected |
| OpenHands | Cloud org shared credits and org-wide LLM; open tier keeps secrets, MCP and skills per backend. | GA | https://docs.openhands.dev/enterprise/enterprise-vs-oss.md | reviewed |
| OpenClaw | Personal instructions per person; one shared agent. | GA | https://docs.openclaw.ai/concepts/multi-user | unreviewed |
| rizzy-harness (ours) | Artifacts can be shared with a workspace; projects (instructions, project memory, kanban) are shared inside a workspace; chats and memories are not shareable. | partial | `README.md` "Artifacts", "Projects"; `crates/master/src/projects.rs:1-12` | from own inventory |

#### Budgets and usage controls per team
What it is: limits on spend or usage per user, group or organisation, and how they are enforced.
Variants: enforcement point (gateway request path with HTTP 429 vs client-side display only); fail-open vs fail-closed when the usage store is down (Claude gateway, two-second Postgres timeout, default open); metering (cost from list price with an unknown-model tier vs raw tokens); alert thresholds (OpenHands 80/90/100% for the org budget only).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Gateway enforces caps `{scope, amount, period}` per user, IdP group or org on every `/v1/messages`; pooled group budgets; pre-check queries Postgres with a two-second timeout and fails open unless `enforcement.fail_closed_on_error`; client warns at 75% and 95%; unknown models priced at a $5/$25 per M tokens tier, never free. Claude Tag has a spend limit; `/usage-credits` lets members request credits. | GA | https://code.claude.com/docs/en/claude-apps-gateway-spend-limits.md, https://code.claude.com/docs/en/claude-tag.md | unreviewed (a doc inconsistency on "pooled" is noted by the survey) |
| OpenCode | Per-member, tagged and workspace budgets in USD resetting monthly 00:00 UTC, enforced as HTTP 429 at request start; CSV usage export. | GA (commercial service) | https://opencode.ai/docs/console/budgets/ | reviewed |
| OpenHands | Enterprise managed LiteLLM gateway: monthly org budget, default user budget, per-user overrides; alerts at 80/90/100% for the org budget only; Canvas 1.38.0 adds per-conversation budget meters. | GA (Enterprise) | https://docs.openhands.dev/openhands/usage/cloud/organizations/budgets.md | unreviewed |
| Warp | Admin Panel spend limits and auto-reload. | GA | https://docs.warp.dev/knowledge-and-collaboration/admin-panel/ | unreviewed |
| Zed/ACP | Org spend limit in Zed Business; Pro default $10 extra-spend limit. | GA | https://zed.dev/docs/account/plans-and-pricing.md | unreviewed |
| Cursor | Spend limits, pooled usage, billing groups (index titles). | GA | https://cursor.com/llms.txt | unreviewed |
| Claude Desktop | Enterprise project spend tags (beta, Nov 6 2026), Analytics API, OpenTelemetry. | GA (spend tags beta) | https://support.claude.com/en/articles/12138966-release-notes | reviewed (release-note dates) |
| Cline | Enterprise cost breakdown by team and model; no budgets documented. | GA | https://docs.cline.bot/enterprise-solutions/overview.md | unreviewed |
| rizzy-harness (ours) | None per user; tokens per turn and per routine run; routine run caps and storage quotas only; no currency. | none | not in the inventory ("Per-user cost/usage limits: None") | from own inventory |

#### Audit logs
What it is: a record of who did what that an admin can export, as opposed to the operator's own debug log.
Variants: events for approval decisions inside the session log (DeepSeek); OpenTelemetry events positioned as the audit source (Claude Code); a dedicated audit stream on the gateway; logs shipped into the customer's own cloud project (Antigravity) or bucket (Cline prompt storage); an audit table with no UI (ours).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | OTel events documented as the audit source with identity attributes; one `safety_event` per action a rule, classifier, hook or sandbox stopped, prompted or bounced; example SIEM config. Gateway emits single-line JSON audit events (`config.load` with config hash, `session.mint`, `inference`, `spend.blocked`, `admin.limit.upsert`, `guardrail.trace`). Artifact audit events (`claude_artifact_*`) and a Compliance API; server-managed settings changes in the compliance API or audit log export. | GA | https://code.claude.com/docs/en/monitoring-usage.md, https://code.claude.com/docs/en/claude-apps-gateway-deploy.md, https://code.claude.com/docs/en/artifacts.md | unreviewed |
| Cursor | Audit logs to SIEM, webhook or S3, OpenTelemetry export (index titles; detail pages not opened). | GA | https://cursor.com/llms.txt | unreviewed |
| Cline | Selective audit logging of admin operations, cost breakdown, OTel export; Prompt Storage syncs each task's `api_conversation_history.json` to a customer S3/R2 bucket with no built-in erasure. Audit log contents and retention not specified. | GA | https://docs.cline.bot/enterprise-solutions/overview.md, https://docs.cline.bot/enterprise-solutions/monitoring/prompt-storage.md | unreviewed |
| Codex | App-server clients must set `clientInfo.name` for the OpenAI Compliance Logs Platform; OTel export off by default, project config cannot set it. | GA | https://learn.chatgpt.com/docs/app-server.md, https://learn.chatgpt.com/docs/agent-approvals-security.md | reviewed (OTel) |
| DeepSeek harness | `approval/asked` and `approval/decided` events form an audit trail in the session log. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/approval | reviewed |
| Antigravity | Request/response logging into the customer's Cloud project (enterprise). | GA | https://antigravity.google/docs/enterprise | unreviewed |
| Hermes Agent | A2A audit log and `tool_progress: log` audit routing; no OpenTelemetry documented. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/messaging/a2a | unreviewed |
| OpenHands | Enterprise Laminar observability; no audit log feature named. | GA | https://docs.openhands.dev/enterprise/architecture.md | reviewed (parts) |
| Warp | Not documented as a feature; closest is agent actions logged when cloud conversation storage is enabled. | unclear | https://docs.warp.dev/enterprise/security-and-compliance/security-overview/ | unverified |
| rizzy-harness (ours) | SQLite `audit` table: node commands, join/revoke, artifact refusals, routine fires, webhook rejects (no content); no UI to browse it. The session event log carries every tool call, approval and checkpoint. | partial | `crates/store/src/lib.rs:293-297`, `crates/store/src/lib.rs:1268-1275`, `crates/core/src/event.rs:59-80` | from own inventory |

#### Session sharing and live collaboration
What it is: letting another person see, steer or continue a session.
Variants: snapshot sharing with visibility levels (Claude cloud sessions: "shared views are snapshots, not live"); live sharing through a relay with bearer-secret links and client-side sealing (Oh My Pi); multi-participant sessions with a creator/owner model (OpenClaw); share links public or org-only (OpenHands); VM or terminal sharing (Cursor); upload to a gist (Pi). Two harnesses removed sharing: OpenCode (discontinued in v1.19.1) and goose (Nostr session sharing removed in v1.54.0).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Oh My Pi | `/collab` publishes a live session via relay (default `wss://my.omp.sh`); full-control and view-only links as bearer secrets, no accounts; the host still runs the model and tools; guest prompts run under host policies; content sealed with AES-256-GCM client-side, key in the URL fragment, relay sees metadata; no production self-host relay shipped. | GA | https://omp.sh/docs/collab | reviewed |
| Claude Code | Cloud sessions: Team Private or Team visibility with repo-access verification, Pro/Max Private or Public; snapshots, not live. Artifacts: private by default, share to org members, folders or public link (off by default on Team/Enterprise), viewer vs editor roles, comments with optional auto-reply by Claude, stale-copy publishes refused. | GA | https://code.claude.com/docs/en/claude-code-on-the-web.md, https://code.claude.com/docs/en/artifacts.md | unreviewed |
| Claude Desktop | Cloud sessions shareable (as above); Cowork sessions cannot be shared. | GA | https://code.claude.com/docs/en/claude-code-on-the-web | unreviewed |
| Cursor | Cloud run URLs shared read-only; admins can enable team follow-ups; "Allow VM sharing" gives teammates terminal and file access to self-hosted workers when live workspace is on; Canvas read-only snapshots on paid plans. | GA | https://cursor.com/docs/cloud-agent, https://cursor.com/docs/cloud-agent/self-hosted, https://cursor.com/docs/agent/tools/canvas | unreviewed |
| Warp | Agent session sharing and cloud agent session sharing; `/remote-control` publishes a third-party CLI session to a shareable link (view or edit); not covered by secret redaction (treat like screen sharing). | GA | https://docs.warp.dev/knowledge-and-collaboration/session-sharing/, https://docs.warp.dev/agents/cli-agents/overview/ | unreviewed |
| OpenHands | Share links public or org-only; org members can view automation-run conversations read-only on Cloud. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/conversations.md | unreviewed |
| OpenClaw | Several trusted people operate one agent with presence and a mentions inbox; docs say ownership and visibility are usability features, not security boundaries. | GA | https://docs.openclaw.ai/concepts/multi-user | unreviewed |
| Pi | Session export, upload to a gist or Radius; docs' warning about privacy is minimal. | GA | https://pi.dev/docs/latest/sessions | reviewed |
| rizzy-harness (ours) | Artifacts shared with a workspace; projects shared inside a workspace; chats and memories deliberately not shareable; no export. | partial | `README.md` "Artifacts", "Projects" | from own inventory |

#### Tier summary: which of the web UI and multi-user features are in the open or free tier
This table answers "who keeps it for a paid tier". Evidence is the rows above; cells say what the survey recorded, not what a vendor currently sells.

| Harness | Web UI (open or free?) | Accounts, roles | SSO / SCIM | Managed config | Budgets | Audit |
|---|---|---|---|---|---|---|
| OpenCode | Open source, password protected, single shared user | Console workspaces: commercial service | SSO included in every Console workspace; SCIM page not opened | Console policies: commercial; local policies layer exists | Console: commercial | Not documented |
| OpenHands | Canvas UI is MIT and free, but no authentication, RBAC or multi-user | Cloud/Enterprise only | Enterprise only (SAML) | Enterprise only | Enterprise only | Laminar in Enterprise |
| OpenClaw | Control UI open source | Multi-user mode and roles in the open code | Not documented (Tailscale identity, GitHub mapping) | Policy CLI, unclear | Not documented | Not documented |
| Hermes Agent | Dashboard and Webapp open (MIT); auth gate with OIDC in the open code | Chat-platform roles in the open code | OIDC on dashboard only; no SSO product | Managed scope documented; page not opened | Not documented | A2A audit log only |
| DeepSeek harness | Web UI open (MIT, preview) | None | None | None | None | Approval events in session log |
| Oh My Pi | Browser viewer for collab only | None | None | None | None | None |
| Pi | None first-party; Fair Source and enterprise tiers announced, not built | None | None | None | None | None |
| goose | None | None | None | Allowlist and custom distributions (open) | None | None |
| Cline | Kanban preview (open, no auth); dashboard unclear | Enterprise (commercial; tiers not given) | Enterprise | Enterprise | Cost breakdown only | Enterprise |
| Warp | Client is open (AGPL-3.0, MIT for UI crates); web apps and services are Warp-operated; the survey found nothing saying the server side is open | Team plans; Admin Panel tiered Free/Business/Enterprise | Enterprise | Admin Panel: Free mostly fixed, Business mostly toggleable, Enterprise full | Admin Panel | Not documented |
| Zed/ACP | Editor only | Zed Business (paid) | Not documented | Not documented | Zed Business spend limit | Not documented |
| Claude Code | Closed, hosted; paid plans | Team/Enterprise | SSO Team/Enterprise; SCIM Enterprise; self-hosted gateway has OIDC and no SCIM | Server-managed settings Team/Enterprise; file and MDM delivery documented separately; gateway built into the binary at no separate licence fee | Gateway caps (no per-seat fee) | OTel events; gateway audit log; Compliance API Enterprise |
| Claude Desktop | Closed | Team/Enterprise admin console | Team/Enterprise | Admin console and MDM | Spend tags (beta) | OTel |
| Cursor | Closed; web client | Teams and Enterprise (Enterprise: SCIM, Team Pools) | Enterprise (page titles) | Enterprise (MDM, admin controls) | Spend limits (titles) | Enterprise (titles) |
| Codex | CLI open; desktop, IDE and cloud closed; no web UI page found | Not documented | Access tokens, service accounts | Managed requirements (tier not stated in the survey) | Not documented | Compliance Logs Platform, OTel |
| Antigravity | Closed | Enterprise via Google Cloud or Gemini Enterprise | Workforce Identity Federation | Enterprise | Not documented | Cloud project logging |
| OpenCodex | Gateway dashboard open | Per-client keys only | None | None | None | Usage logs only |
| rizzy-harness (ours) | Web UI is the main surface, open, multi-user with workspaces | Yes, self-hosted, no paid tier | None | `node.toml` local only | None | Audit table, no UI |

---

## 3. Availability matrix

Legend: `Y` documented and released; `B` beta, experimental, preview or partial (including "exists but the specific review/approval flow is not documented"); `P` planned or vendor-claim only; `-` nothing documented; `?` unverified (page not opened or skimmed only). Row 4 counts a hosted web UI as `Y` even when it is closed and paid; use the tier summary to see which are free. Rows 14 and 15 mean "documented browser statements"; a `-` means the survey found no page.

| Feature | Codex | OpenCodex | OpenCode | Claude Code | Claude Desktop | DeepSeek harness | Oh My Pi | Pi | Cursor | Antigravity | OpenHands | OpenClaw | goose | Hermes Agent | Zed/ACP | Warp | Cline | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 Terminal UI | Y | - | Y | Y | - | - | Y | Y | Y | Y | - | Y | Y | Y | - | Y | Y | - |
| 2 IDE integrations | Y | - | ? | Y | - | - | Y | - | Y | Y | - | - | Y | - | Y | - | Y | - |
| 3 Desktop apps | Y | B | B | - | Y | B | - | - | Y | Y | B | Y | Y | Y | Y | Y | Y | - |
| 4 Web UIs | - | B | Y | Y | B | B | B | - | Y | B | Y | Y | - | Y | - | Y | B | Y |
| 5 Mobile | Y | - | B | Y | Y | - | ? | - | Y | B | B | Y | - | - | - | B | - | B |
| 6 Voice | B | - | - | Y | Y | B | - | - | Y | Y | Y | Y | Y | Y | - | Y | Y | Y |
| 7 Messaging-app channels | Y | - | - | Y | B | - | - | - | Y | - | Y | Y | - | Y | - | Y | - | - |
| 8 Approval presentation | Y | - | Y | Y | Y | Y | Y | - | Y | Y | B | Y | Y | Y | Y | Y | Y | Y |
| 9 Diff review | Y | - | B | Y | Y | B | B | - | Y | Y | Y | Y | - | Y | Y | Y | Y | B |
| 10 Plan review | B | - | B | Y | Y | B | Y | - | Y | Y | - | - | - | - | B | Y | B | - |
| 11 Notifications / waiting signals | ? | - | B | Y | Y | - | - | - | Y | - | - | B | - | - | - | Y | - | Y |
| 12 Several agents at once | Y | - | B | B | Y | B | B | - | Y | Y | Y | Y | - | Y | Y | Y | B | B |
| 13 Agent's own browser | Y | - | Y | - | Y | B | Y | - | Y | Y | Y | Y | B | Y | - | Y | ? | - |
| 14 User's real browser | Y | - | P | Y | Y | - | Y | - | - | Y | - | Y | - | Y | - | - | - | - |
| 15 Browser isolation and site policy | Y | - | B | Y | Y | - | B | - | Y | Y | B | Y | - | Y | - | B | - | - |
| 16 Computer use | Y | - | - | B | B | B | Y | - | Y | - | - | Y | B | Y | - | Y | - | - |
| 17 Accounts, roles, orgs, workspaces | B | B | Y | Y | Y | - | - | - | Y | Y | Y | Y | - | Y | Y | Y | Y | Y |
| 18 SSO and provisioning | B | - | Y | Y | Y | - | - | - | Y | Y | Y | B | - | B | - | Y | Y | - |
| 19 Managed config / policy distribution | Y | - | Y | Y | Y | - | - | - | Y | B | B | B | B | B | - | Y | Y | B |
| 20 Shared team resources | B | - | B | Y | Y | - | - | - | B | B | B | B | B | Y | - | Y | Y | B |
| 21 Budgets and usage controls | - | - | Y | Y | B | - | - | - | Y | - | Y | - | - | - | Y | Y | B | - |
| 22 Audit logs | B | - | - | Y | B | B | - | - | Y | B | B | - | - | B | - | - | Y | B |
| 23 Session sharing / live collaboration | - | - | - | Y | Y | - | Y | B | Y | - | Y | Y | - | - | - | Y | - | B |

Cell notes: Claude Code row 12 is `B` because agent view is a research preview. Claude Code row 13 is `-` because Claude Code itself has no built-in browser (the pane belongs to Claude Desktop). Row 18 Zed/ACP is `-` (Zed Business has roles but SSO is not documented in the lines read). Row 1 goose is `Y` on the strength of a named CLI; the TUI itself is not described. OpenCode row 2 is `?` because its IDE pages were not opened.

---

## 4. Common across the ecosystem

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

## 5. Only one harness has it

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

## 6. Ours against the field

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

## 7. Corrections, contradictions and unverified claims

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

## 8. Questions for Round 2

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
