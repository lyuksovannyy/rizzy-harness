# Round 1 synthesis 06: extensibility (MCP, plugins, skills, hooks, commands, SDKs, protocols)

Synthesis date: 2026-11-10. Synthesis only: every fact below comes from the Round 1 survey files; nothing was browsed.

## 1. Scope

Area covered: survey area 13 (extensibility), split into: MCP as a client, the harness as an MCP server, skills, plugins and extensions, in-process middleware, hooks, custom commands and prompt templates, custom tools, code mode, SDKs and embedding, client protocols (ACP, vendor app-server and RPC protocols, A2A), and reading other harnesses' configuration.

Files and line ranges read:

| File | Lines read for this group |
|---|---|
| `codex.md` | 65-68 (Code Mode), 133-136 (tool search), 370-374, 433-442 (skills budget, presets, custom prompts), 585-687 (area 13), 986-1080 (sections 3-7), 1159-1196 (review) |
| `opencodex.md` | 89-100 (area 13), 195-328 (sections 3-7, Part B, review) |
| `opencode.md` | 93-97 (execute / Code Mode), 200-204 (AGENTS.md), 289-339 (area 13), 458-512, 562-610 (sections 3-7, review) |
| `claude-code.md` | 2153-2980 (area 13 in full); plus extensibility entries filed in other areas: 192-196, 369-373, 894-898, 911-915, 1052-1056, 1189-1222, 1290-1294, 1565-1569, 1720-1735, 3885-3934, 4458-4462, 4512-4521, 4776-4809, 5126-5130; sections 3-7 at 5202-5339 |
| `claude-code.review-1.md`, `-2.md`, `-3.md` | whole files (their corrections and "Move to Unverified" lists are applied below) |
| `claude-desktop.md` | 240-300 (area 13), 469-607 (sections 3-7, review) |
| `deepseek-harness.md` | 319-362 (area 13), 533-677 (sections 3-7, review) |
| `oh-my-pi.md` | 114-118 (eval), 174-178 (instruction files), 317-373 (area 13), 549-663 (sections 3-7, review) |
| `pi.md` | 75-85 (codemode, tool search), 181-185 (context files), 287-402 (area 13), 561-618, 662-697 (sections 3-7, review) |
| `cursor.md` | 265-319 (area 13), 456-523, 595-642 (sections 3-7, review) |
| `antigravity.md` | 164-212 (/learn, rules, skills), 303-358 (area 13), 494-618 (sections 3-7, review) |
| `openhands.md` | 69-73, 133-137, 257-310 (area 13), 459-592 (sections 3-7, review) |
| `openclaw.md` | 24-28, 62-80, 268-272, 300-304, 352-389 (area 13), 596-788 (sections 3-7, review) |
| `goose.md` | 116-126, 166-184, 226-245, 298-365 (area 13), 374-379, 428-433, 482-487, 518-578, 646-677 |
| `hermes-agent.md` | 73-83 (tool search, execute_code), 333-337 (A2A), 411-484 (area 13), 667-671, 679-820 |
| `zed-acp.md` | 1-40, 92-95 (skills), 147-157 (A13), 206-407 (Part B ACP and sections 3-7), 447-484 (review) |
| `warp.md` | 176-205 (rules, skills, slash commands), 288-322 (area 13), 546-670 (sections 3-7, review) |
| `cline.md` | 146-197 (rules, skills, workflows), 212-216 (Kanban), 270-310 (area 13), 476-629 (sections 3-7, review) |
| `own-rizzy-harness.md` | whole file (sections 2, 9, 10, 11 and (a)-(c) are the ones used) |

Not read: `00-targets.md` (not needed for this group). Nothing was unreadable. Evidence shorthands in the surveys were expanded mechanically: OpenHands `D:` is `https://docs.openhands.dev`; Hermes Agent paths are relative to `https://hermes-agent.nousresearch.com/docs/`. Claude Code line numbers in the Check column refer to `claude-code.md` headings as cited by its three review files.

Check labels: `reviewed` (the survey or review says verified/confirmed), `corrected` (corrected in review; corrected text used), `unreviewed`, `unverified` (the survey or review could not confirm it).

## 2. Features

### 2.1 MCP as a client

#### MCP client: transports and configuration scopes
What it is: the harness connects to Model Context Protocol servers, local (stdio child process) or remote (HTTP), and offers their tools to the model. Configuration lives in one or more files whose scope (user, project, managed) decides who can add a server.
Variants: transports differ (SSE is dropped by Pi, goose and Antigravity's app/CLI, kept as legacy by Claude Code, OMP, Cline, Hermes, Cursor, Warp; Claude Code alone adds WebSocket and in-process `sdk` servers). Scope models differ: Claude Code resolves local > project > user > plugin > connector with whole-entry wins; OMP takes the first definition found and reads other apps' configs; goose treats every extension as an MCP server; Zed forwards its configured servers to external agents over ACP; Codex can place a server on a remote executor (`environment_id`); Cursor cloud agents run stdio servers inside the VM but HTTP servers on Cursor's backend.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | STDIO and Streamable HTTP; `config.toml` with `enabled_tools`/`disabled_tools`, timeouts, `required`, `startup_readiness`, per-tool approval mode and output limit; server `instructions` read (first 512 chars should stand alone); one config shared by CLI, IDE, Desktop; `environment_id` runs a server on a remote executor. | GA | https://learn.chatgpt.com/docs/extend/mcp.md | unreviewed |
| OpenCodex | Gateway only: relays MCP tool namespaces for Codex; it is not an MCP client. | GA (adapters) | https://opencodex.me/reference/adapters/ | unreviewed |
| OpenCode | Local (stdio) and remote (Streamable HTTP) under `mcp.servers`; protocol `legacy` (default), `auto` or `2026-07-28`; `pool: true` shares a connection across directories (2026-07-28 only); server instructions injected as `<mcp_instructions>`; `opencode mcp add|list|auth|logout`. | GA | https://opencode.ai/docs/mcp-servers/ ; https://opencode.ai/changelog | reviewed |
| Claude Code | `http` (recommended), `sse` (deprecated, auto-fallback), `stdio`, `ws` (JSON-only, no OAuth), in-process `sdk` (SDK hosts only). Scopes local > project > user > plugin > claude.ai connector, whole entry from the winner, no field merging; managed servers outrank all. Two client runtimes (MCP TS SDK 1.x and 2.0 with revision 2026-07-28). `roots/list` answered with launch dir plus added dirs. | GA (SSE deprecated) | https://code.claude.com/docs/en/mcp.md ; https://code.claude.com/docs/en/mcp-quickstart.md | reviewed (scopes, transports) |
| Claude Desktop | Connectors are remote MCP servers added in the UI; local servers ship as Desktop Extensions (`.mcpb` ZIP with `manifest.json`, Node runtime bundled, sensitive `user_config` to the OS keychain); no manual JSON path documented for local servers. | GA | https://claude.com/marketplace/connectors-plugins ; https://www.anthropic.com/engineering/desktop-extensions ; https://support.claude.com/en/articles/10949351-getting-started-with-local-mcp-servers-on-claude-desktop | reviewed |
| DeepSeek harness | Client plugin `@deepseek-ai/dsh-mcp-client`, stdio and streamable HTTP, YAML entries via a patch; crashed stdio children reconnect with backoff; no server enabled by default because server commands are trusted code outside the sandbox. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/mcp-memory ; https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/mcp.md | corrected |
| Oh My Pi | stdio, Streamable HTTP, legacy SSE; `.omp/mcp.json`, user, profile and project-root files (lowest); first definition of a duplicate wins; auto-imports definitions from Claude Code, Codex, Gemini CLI, OpenCode, Cursor, Windsurf, VS Code; in ACP mode the client supplies servers. | GA | https://omp.sh/docs/mcp ; https://omp.sh/docs/mcp-authoring | reviewed |
| Pi | Built-in default extension `builtin:mcp`; stdio and streamable HTTP (SSE not supported); `~/.pi/agent/mcp.json` and `.pi/mcp.json` (trust required); `--no-mcp`; not loaded in SDK sessions by default. Added in 0.99.0 after the author had argued against MCP. | GA | https://pi.dev/docs/latest/mcp ; https://pi.dev/changelog | reviewed |
| Cursor | stdio, SSE, Streamable HTTP; `.cursor/mcp.json` and `~/.cursor/mcp.json` with `${env:NAME}` interpolation. Cloud agents: HTTP and stdio only; HTTP keeps credentials out of the VM, stdio runs inside it. ACP sessions load project and user MCP but not team servers. | GA | https://cursor.com/docs/mcp ; https://cursor.com/docs/cloud-agent/capabilities ; https://cursor.com/docs/cli/acp | unreviewed |
| Antigravity | `mcp_config.json` (global, workspace `.agents/`, plugins); stdio or Streamable HTTP; SSE and `httpUrl` not supported in app/CLI (SDK supports SSE); per-server `enabledTools`/`disabledTools`, `timeoutSeconds`. | GA | https://antigravity.google/docs/mcp | unreviewed |
| OpenHands | SSE, streamable HTTP and stdio (pages disagree on which the SDK uses); FastMCP config format; MCP annotations mapped to tool annotations; per-backend config in Canvas; agent profiles can scope to `mcp_server_refs`; Enterprise org servers. | GA | https://docs.openhands.dev/overview/model-context-protocol.md ; https://docs.openhands.dev/sdk/arch/tool-system.md ; https://docs.openhands.dev/openhands/usage/agent-canvas/agent-profiles.md | unreviewed |
| OpenClaw | `openclaw mcp add/set/login/probe/doctor` manages saved outbound servers (transports, OAuth, MCP Apps), Control UI editor at `/settings/mcp`; servers can be scoped to the Codex app-server runtime; the ACP bridge rejects per-session MCP injection. | GA | https://docs.openclaw.ai/cli/mcp ; https://docs.openclaw.ai/cli/acp | unreviewed |
| goose | Every extension is MCP: `builtin`, `platform`, `stdio`, `streamable_http` (SSE dropped); `extensions:` in `config.yaml` with `available_tools`; Extension Manager lets the agent find and enable an extension mid-task (CLI asks, session only); `--container` runs extensions in Docker. | GA | https://goose-docs.ai/docs/getting-started/using-extensions.md ; https://goose-docs.ai/docs/guides/config-files.md | unreviewed |
| Hermes Agent | stdio, streamable HTTP, legacy SSE under `mcp_servers`; tools `mcp_<server>_<tool>` with include/exclude globs; mTLS client certs; lazy servers; stdio servers get a safe env baseline; invisible Unicode tag characters stripped from results. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp | reviewed |
| Zed/ACP | `context_servers`: local (`command`, `args`, `env`) or remote (`url`, `headers`); extensions package servers; reloads on `tools/list_changed`; servers configured in Zed are forwarded to external agents over ACP. | GA | https://zed.dev/docs/ai/mcp | unreviewed |
| Warp | CLI (stdio) and Streamable HTTP/SSE servers, UI or JSON, stored in Warp Drive or `~/.warp/.mcp.json`; sharing a server scrubs env values; in cloud, URL servers go through Warp's proxy, command servers run in the execution environment. | GA | https://docs.warp.dev/agents/capabilities/mcp/ ; https://docs.warp.dev/platform/architecture/ | unreviewed |
| Cline | `cline_mcp_settings.json`; stdio, streamable HTTP (recommended), SSE (legacy, fallback when `type` omitted); `autoApprove` and `disabled` per entry; subagents get no MCP tools. | GA | https://docs.cline.bot/mcp/mcp-overview.md ; https://docs.cline.bot/features/subagents.md | unreviewed |
| rizzy-harness (ours) | none. No MCP code or config key; sidebar "Connections" placeholder says "no route, no code". | none | `web/src/components/Sidebar.tsx:129-143` | from own inventory |

#### MCP remote authentication (OAuth and headers)
What it is: signing the harness in to a remote MCP server, usually OAuth 2.0 with discovery, plus static or helper-generated headers.
Variants: most implement the same discovery set (Dynamic Client Registration, Client ID Metadata Documents, pre-registered clients). Differences are where tokens live (Claude Code keychain, Antigravity a JSON file, OMP per profile plus URL), whether headless runs can sign in (Claude Code `-p`/SDK cannot; Hermes lists headless options), and extras: Codex adds RFC 8707 resource and ChatGPT session auth, Hermes adds mTLS, Antigravity Google ADC, OMP `!command` header values, Claude Code a `headersHelper`.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Bearer token, static or env headers, header-helper command, OAuth (CIMD, DCR, pre-registered, RFC 8707 resource), or the ChatGPT session for first-party servers; `codex mcp login [--no-browser]`. | GA | https://learn.chatgpt.com/docs/extend/mcp.md | unreviewed |
| OpenCode | OAuth on by default for remote servers with pre-registered client, CIMD or dynamic registration; `opencode mcp auth|logout`. | GA | https://opencode.ai/docs/mcp-servers/ | reviewed |
| Claude Code | 401/403 flags "needs authentication"; RFC 9728 then RFC 8414 discovery, DCR and CIMD, pre-registered `--client-id`/`--client-secret`, fixed `--callback-port`, `oauth.scopes` pinning; refresh-and-retry once; secret in keychain or credentials file; a configured `Authorization` header disables OAuth; `-p`/SDK cannot run OAuth. Removing a server does not revoke tokens upstream; `claude mcp logout` does where supported. | GA | https://code.claude.com/docs/en/mcp.md | unreviewed |
| Oh My Pi | OAuth via `/mcp reauth`, stored per profile plus URL; `!command` header values. | GA | https://omp.sh/docs/mcp | reviewed |
| Pi | OAuth including dynamic registration and client-ID metadata documents; `pi mcp login|logout`. | GA | https://pi.dev/docs/latest/mcp | reviewed |
| Cursor | OAuth code flow with PKCE, dynamic registration or static client id; cloud agents use OAuth per user. | GA | https://cursor.com/docs/mcp | unreviewed |
| Antigravity | Google ADC (`google_credentials`) or OAuth 2.0 with PKCE (manual client, CIMD, DCR); fixed redirect `https://antigravity.google/oauth-callback`; tokens in `~/.gemini/antigravity/mcp_oauth_tokens.json`. | GA | https://antigravity.google/docs/mcp | unreviewed |
| OpenHands | OAuth-only library entries install through the server's own OAuth (1.31.0). | GA | https://docs.openhands.dev/overview/model-context-protocol.md | unreviewed |
| OpenClaw | `openclaw mcp login` with transports and OAuth. | GA | https://docs.openclaw.ai/cli/mcp | unreviewed |
| goose | CIMD or DCR auto-registration; pre-registered `client_id`/`client_secret_key` and `scopes`; callback on 127.0.0.1 ephemeral port (`GOOSE_OAUTH_CALLBACK_PORT`). | GA | https://goose-docs.ai/docs/getting-started/using-extensions.md | unreviewed |
| Hermes Agent | OAuth with PKCE, DCR or CIMD, refresh, headless options; mTLS client certificates. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp | reviewed |
| Zed/ACP | OAuth prompt when no `Authorization` header is set. | GA | https://zed.dev/docs/ai/mcp | unreviewed |
| Warp | Credentials stored locally; in cloud, URL-backed servers including OAuth go through Warp's proxy. | GA | https://docs.warp.dev/agents/capabilities/mcp/ | unreviewed |
| Cline | CLI `cline mcp` wizard with OAuth authorize; token storage not described. | GA | https://docs.cline.bot/mcp/mcp-overview.md | unreviewed |
| DeepSeek harness | Nothing documented (review: the `mcp` subsystem page does not document OAuth). | n/a | https://github.com/deepseek-ai/deepseek-harness/blob/master/docs/subsystems/mcp.md | corrected |
| rizzy-harness (ours) | none. Nearest is per-service keys sent only to their saved base URL. | none | `crates/master/src/secrets.rs:1-15` | from own inventory |

#### Tool deferral and tool search
What it is: instead of sending every tool schema each turn, the harness sends names (or nothing) and lets the model search for and load schemas on demand. The main answer to MCP context bloat.
Variants: who triggers loading (a `ToolSearch` tool in Claude Code, `call_mcp_tool` in Antigravity, bridge tools `tool_search`/`tool_describe`/`tool_call` in Hermes and OpenClaw, `enable_tools` in goose, `xd://` reads in OMP); the default (on by default in Claude Code, Antigravity, Hermes `auto`, OpenClaw; opt-in in goose); threshold rules (Claude Code `auto` switches at 10% of context; Hermes default budget 5% of context); per-server or per-tool overrides (Claude Code `alwaysLoad` and `_meta["anthropic/alwaysLoad"]`, Antigravity `eager: true`, Pi and Codex per-server surfaces `direct`/`deferred`/`code_mode`/`hidden`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | MCP tools can be kept out of the initial list and found by tool search; per-server surfaces `direct`, `deferred`, `code_mode`; 0.167.0 turned "deferred tool world state" on by default. Mechanism only partly documented. | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md ; https://learn.chatgpt.com/docs/changelog | unreviewed |
| OpenCode | MCP tools are reached through Code Mode by default, grouped under server namespaces; `codemode:false` lists them directly. | GA | https://opencode.ai/docs/mcp-servers/ | reviewed |
| Claude Code | On by default: names and server instructions load at start, definitions via `ToolSearch` (up to 5 matches per search, kept until compacted); `ENABLE_TOOL_SEARCH` `true`/`auto`/`auto:N`/`false`; model-gated; gateways must forward `defer_loading`/`tool_reference`; catalog limit 10,000 tools; descriptions cut at 4,096 chars upfront and 16,384 when loaded by search. Docs: selection accuracy degrades beyond 30-50 loaded tools. | GA | https://code.claude.com/docs/en/mcp.md ; https://code.claude.com/docs/en/agent-sdk/tool-search.md | unreviewed (description caps and the 5 s `alwaysLoad` cap: unverified per review 2) |
| Oh My Pi | Extra built-in tools sit behind `xd://` devices (`read xd://` lists, `write xd://<tool>` runs); on by default; affects presentation, not permissions. MCP deferral not documented. | GA | https://omp.sh/docs/tools | unreviewed |
| Pi | `tool_search` extension declares deferred tools on demand; MCP exposure per server or tool: `codemode` (default), `deferred`, `direct`, `hidden`. Not loaded in SDK sessions. | GA | https://pi.dev/docs/latest/cli ; https://pi.dev/docs/latest/mcp | unreviewed |
| Antigravity | Lazy MCP tool loading by default: schemas cached on disk, called through `call_mcp_tool`; `eager: true` per tool registers the schema upfront. | GA | https://antigravity.google/docs/mcp | unreviewed |
| OpenClaw | `tool_search`, `tool_describe`, `dispatch_action`; on by default when unset, yet listed under "Experimental features". | GA | https://docs.openclaw.ai/tools ; https://docs.openclaw.ai/concepts/experimental-features | reviewed |
| goose | `GOOSE_TOOL_DEFERRAL=lookup` (off unless set) publishes MCP tool names upfront, schemas on demand via `enable_tools`. | unclear | https://goose-docs.ai/docs/guides/config-files.md | corrected |
| Hermes Agent | MCP and non-core plugin tools behind `tool_search` (BM25), `tool_describe`, `tool_call`; hooks and approvals still apply to the real tool name; three tiers; default budget 5% of context. Called "opt-in" but default `auto` behaves as on. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/tool-search | unreviewed |
| rizzy-harness (ours) | none. Every allowed tool's schema is sent every step; PLAN's "schemas loaded on demand" is docs-only. | docs-only | `crates/cli/src/main.rs:447-451` (PLAN §0) | from own inventory |

#### MCP protocol features beyond tools
What it is: the rest of MCP: prompts, resources, elicitation (server asks the user for input), roots, sampling (server asks the harness's model), `list_changed`, MCP Apps (`ui://` interactive UI), and the tasks extension for long jobs.
Variants: coverage varies widely. Prompts-as-slash-commands is common (OpenCode, Claude Code, OMP). Elicitation is default-on in goose and Hermes (Hermes asks even under YOLO); Zed does not support it in MCP (it has ACP elicitation, a different feature). Sampling is on by default only in Hermes (rate limited); goose removed sampling code. MCP Apps are rendered by Claude Desktop, Cursor, goose (preview), OpenClaw; Pi does not render them. Only Claude Code documents the MCP tasks extension.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Server `instructions` read; elicitations and approvals arrive as server-initiated app-server requests. | GA | https://learn.chatgpt.com/docs/extend/mcp.md ; https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| OpenCode | Prompts become `/<server>:<prompt>`; instructions injected as `<mcp_instructions>`; HTTP API can add/remove MCP servers at runtime. | GA | https://opencode.ai/docs/mcp-servers/ ; https://opencode.ai/docs/api/ | reviewed |
| Claude Code | Prompts as `/server:prompt (MCP)`; resources via `@` mentions; elicitation form and URL modes (URL capped near 8,000 chars); `list_changed` refetch; MCP Apps resources excluded from listings; MCP tasks extension (poll `tasks/get`, answer `input_required`, re-follow on resume); long calls backgrounded after 2 minutes. | GA; MCP tasks beta (v2.1.304+) | https://code.claude.com/docs/en/mcp.md | reviewed (tasks, auto-backgrounding); rest unreviewed |
| Claude Desktop | MCP Apps: connectors can ship `ui://` resources rendered as side-panel or full-screen apps; mobile renders interactive apps. | GA | https://claude.com/docs/llms.txt ; https://support.claude.com/en/articles/12138966-release-notes | unreviewed (detail pages not opened) |
| DeepSeek harness | Separate MCP resource tools; a failed refresh keeps the previous tool generation. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | corrected |
| Oh My Pi | Prompts become `/server:prompt`; resources and optional notifications; `/mcp resources|prompts|notifications`. | GA | https://omp.sh/docs/mcp | reviewed |
| Pi | `list_mcp_resources`, `list_mcp_resource_templates`, `read_mcp_resource`; MCP Apps not rendered. | GA | https://pi.dev/docs/latest/mcp | reviewed |
| Cursor | Elicitation; MCP Apps (interactive UI). | GA | https://cursor.com/docs/mcp | unreviewed |
| OpenClaw | MCP Apps supported in the managed server config. | GA | https://docs.openclaw.ai/cli/mcp | unreviewed |
| goose | Elicitation (form) on by default, 5-minute timeout; one root per session (the working directory); MCP Apps early support for the draft spec; sampling code removed (v1.53.0). | elicitation and roots GA; MCP Apps preview | https://goose-docs.ai/docs/guides/mcp-elicitation.md ; https://goose-docs.ai/docs/guides/mcp-roots.md | unreviewed |
| Hermes Agent | Sampling on by default (rate limited, 10 rpm per review); elicitation on by default and always asks even under YOLO; `list_changed` refreshes; prompt and resource change notifications received but not acted on. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp | reviewed |
| Zed/ACP | Tools and Prompts supported; discovery, sampling and elicitation not supported. | GA | https://zed.dev/docs/ai/mcp | unreviewed |
| rizzy-harness (ours) | none. Nearest: model-emitted `ui:` blocks (fixed set, validated in Rust), not user-extensible. | none | `crates/core/src/ui/mod.rs` | from own inventory |

#### MCP limits, timeouts and connection lifecycle
What it is: wall-clock and idle timeouts per call, startup waits, output caps, reconnect and retry rules.
Variants: Claude Code documents the most (per-call hard limit, idle timeout 5 min remote / 30 min stdio, 25,000-token output cap with spill to file, 16 MiB response cap, reconnect backoff). Others give one or two numbers: OpenCode 30 s startup / 30 s catalog / 12 h execution; Antigravity 300 s connect / 180 s per call; Codex per-tool output limit. Warp only recently stopped waiting forever.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Timeouts, `required` servers and `startup_readiness`, per-tool output limit in `config.toml`. | GA | https://learn.chatgpt.com/docs/extend/mcp.md | unreviewed |
| OpenCode | Startup 30 s, catalog 30 s, execution 12 h. | GA | https://opencode.ai/docs/mcp-servers/ | reviewed |
| Claude Code | Per-server `timeout` hard wall clock; `MCP_TOOL_TIMEOUT` about 28 h; idle timeout 5 min remote, 30 min stdio; startup `MCP_TIMEOUT` 30 s; background connect; up to 5 reconnects with backoff; output over 25,000 tokens or 50,000 chars saved to a file and replaced by its path; 16 MiB HTTP body cap; stdio shutdown SIGINT, SIGTERM after 100 ms, SIGKILL after 400 ms more. | GA | https://code.claude.com/docs/en/mcp.md | unreviewed |
| DeepSeek harness | Crashed stdio children reconnect with backoff, then tools unregister until reload. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/mcp-memory | corrected |
| Antigravity | Default timeouts 300 s connect/list, 180 s per call; per-tool `background` setting. | GA | https://antigravity.google/docs/mcp | unreviewed |
| goose | Per-extension timeout. | GA | https://goose-docs.ai/docs/getting-started/using-extensions.md | unreviewed |
| Hermes Agent | Lazy servers; error redaction; per-tool cost declarations with in-memory spend totals that reset on restart. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp | reviewed |
| Warp | MCP tool calls now time out instead of waiting forever (changelog 2026.10.07). | GA | https://docs.warp.dev/agents/capabilities/mcp/ | unreviewed |
| rizzy-harness (ours) | none (no MCP). Our own tool caps exist: 600 s default per tool call, 30,000 chars to the model. | none | `crates/agent-native/src/lib.rs:68`, `crates/tools/src/lib.rs:37` | from own inventory |

#### MCP approval, trust and admin allow/deny lists
What it is: how MCP tool calls are approved, how a project-supplied server is trusted before it starts, and how admins restrict servers.
Variants: project servers: Claude Code asks once per project server but loads them without asking in `-p`, SDK and some cloud modes; Pi requires project trust for `.pi/mcp.json`; DSH enables none by default. Per-tool approval: Codex per-tool approval mode, Cline `autoApprove` lists, Antigravity unconfigured tools are Ask with `mcp(server/tool)` rules, OpenHands sends tools with `readOnlyHint=False` to an LLM risk check, Hermes adds a per-chat spend limit that asks even on trusted servers. Admin: Claude Code `managed-mcp.json` (exclusive) and URL/command/name allow and deny lists; Cursor enterprise allowlists by command, URL and tool; goose an admin-hosted allowlist with exact command match; Cline `allowedMCPServers`.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Per-tool approval mode in config; subagents cannot override `mcp_servers`; the network proxy does not cover MCP. | GA | https://learn.chatgpt.com/docs/extend/mcp.md | unreviewed |
| OpenCode | No per-agent MCP enablement; use permission rules. | GA | https://opencode.ai/docs/mcp-servers/ | reviewed |
| Claude Code | Project `.mcp.json` servers need an approval prompt the first time; repo-committed approvals ignored until workspace trust; but `-p`, Agent SDK, some cloud sessions and bypass with `skipDangerousModePermissionPrompt` load them without asking. Managed `managed-mcp.json` (exclusive, world-readable so no secrets), `managedMcpServers` (https-only), `allowedMcpServers`/`deniedMcpServers` by URL, command or name (name "not a security control"); deny wins. Anthropic does not security-audit MCP servers. | GA | https://code.claude.com/docs/en/mcp.md ; https://code.claude.com/docs/en/managed-mcp.md ; https://code.claude.com/docs/en/security.md | unreviewed (headless untrusted loading: reviewed, review 3 line 3622) |
| Claude Desktop | Owners allow or deny directory extensions and upload private ones; system policy overrides in-app lists. | GA | https://claude.com/docs/plugins/platform-support.md ; https://www.anthropic.com/engineering/desktop-extensions | unreviewed |
| Oh My Pi | No MCP-specific approval beyond the write tier. | GA | https://omp.sh/docs/approvals | reviewed |
| Pi | MCP calls go through the normal tool pipeline, so extension gates apply; project config requires trust. | GA | https://pi.dev/docs/latest/mcp | reviewed |
| Cursor | Enterprise allowlists by command, URL and tool. | GA | https://cursor.com/docs/mcp | unreviewed |
| Antigravity | Unconfigured MCP tools are Ask; policy rules `mcp(server/tool)`. | GA | https://antigravity.google/docs/mcp ; https://antigravity.google/docs/permissions | unreviewed |
| OpenHands | Tools with `readOnlyHint=False` get LLM risk prediction; MCP server processes not sandboxed (nothing documented). | GA | https://docs.openhands.dev/sdk/arch/tool-system.md | unreviewed |
| goose | Admin YAML allowlist at a URL (`GOOSE_ALLOWLIST`), install rejected unless the command matches exactly (gates install, not runtime); per-extension `available_tools`; malware check on external extensions. | GA | https://goose-docs.ai/docs/guides/allowlist.md ; https://goose-docs.ai/docs/getting-started/using-extensions.md | reviewed |
| Hermes Agent | Per-server include/exclude globs; per-chat `spend_limit` asks even on `trust: full` servers. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp | reviewed |
| Zed/ACP | Tool permission lists have an MCP pattern format. | GA | https://zed.dev/docs/ai/mcp | reviewed (via tool-permissions entry) |
| Warp | MCP allowlist and denylist (denylist wins) plus "Agent decides". | GA | https://docs.warp.dev/agents/capabilities/mcp/ | reviewed |
| Cline | Per-server `autoApprove` tool lists; enterprise `mcpMarketplaceEnabled` and `allowedMCPServers`. | GA | https://docs.cline.bot/mcp/mcp-overview.md ; https://docs.cline.bot/enterprise-solutions/configuration/infrastructure-configuration/control-other-cline-features/mcp-server-controls.md | unreviewed |
| rizzy-harness (ours) | none for MCP. The analogous lever exists: `node.toml` `denied_tools` and approval caps that protocol and HTTP cannot widen. | none (MCP); works (node policy) | `crates/node/src/policy.rs:9-105` | from own inventory |

#### Connector directories and MCP registries
What it is: a browsable catalog of MCP servers or connectors that users can enable from inside the product.
Variants: vendor-hosted directories (Claude Desktop 1,104 connectors with Verified/Community/Custom labels; Codex "universal" directory shared with ChatGPT for apps), in-product search that the agent itself can use (goose Extension Manager), marketplaces mixed with skills and plugins (Cline Desktop), library entries (OpenHands). Claude Code documents no built-in MCP registry and points admins to a managed plugin marketplace as the catalog.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Apps (connectors) from plugins, browsed with `/apps`, inserted as `$app-slug`; their traffic does not pass the command proxy. | GA | https://learn.chatgpt.com/docs/developer-commands.md ; https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| Claude Code | claude.ai connectors appear in `/mcp` with a claude.ai login (not with API keys or third-party providers); no built-in MCP registry. | GA | https://code.claude.com/docs/en/mcp.md ; https://code.claude.com/docs/en/managed-mcp.md | unreviewed |
| Claude Desktop | Marketplace lists 1,104 connectors and 530 plugins with Verified, Community, Custom labels; directory extensions auto-update, private ones do not. | GA | https://claude.com/marketplace/connectors-plugins | reviewed |
| OpenHands | OAuth-only library entries; Enterprise org MCP servers. | GA | https://docs.openhands.dev/overview/model-context-protocol.md | unreviewed |
| goose | Extension Manager lets the agent search for and enable extensions mid-task (CLI asks; session only). | GA | https://goose-docs.ai/docs/getting-started/using-extensions.md | unreviewed |
| Zed/ACP | Extensions package MCP servers. | GA | https://zed.dev/docs/ai/mcp | unreviewed |
| Cline | Desktop Marketplace for skills, MCP servers and plugins; enterprise switch hides it. | GA | https://docs.cline.bot/usage/cline-desktop.md | unreviewed |
| rizzy-harness (ours) | none ("Connections" placeholder only). | stub | `web/src/components/Sidebar.tsx:129-143` | from own inventory |

#### MCP servers that push events into a session
What it is: an MCP server that can inject messages (CI results, chat messages, alerts) into a running session rather than only answering calls.
Variants: Claude Code channels are opt-in per session, allowlisted during preview, and break on protocol revision 2026-07-28; Hermes uses event subscriptions with signed webhook routes.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Local stdio server declaring `claude/channel` emits `notifications/claude/channel`; Claude sees `<channel source=...>`; opt in with `--channels`; Anthropic allowlist or org `allowedChannelPlugins`; official Telegram, Discord, iMessage plugins; not registered if the server negotiates 2026-07-28. | research preview | https://code.claude.com/docs/en/channels-reference.md ; https://code.claude.com/docs/en/channels.md | unreviewed |
| Hermes Agent | MCP event subscriptions over signed webhook routes. | experimental | https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp | reviewed |
| rizzy-harness (ours) | none. Inbound routine webhooks exist, but they start a routine, not a session event. | none | `crates/master/src/http/hooks.rs:1-60` | from own inventory |

### 2.2 The harness as an MCP server

#### Exposing the harness over MCP
What it is: the harness runs as an MCP server so another client can use its tools or conversations.
Variants: what is exposed differs: Claude Code exposes its own tools (Read, Edit, Bash) but not the agent loop; OpenClaw exposes channel conversations; Hermes exposes messaging conversations and optionally tool families; goose exposes each built-in extension as a standalone server; Warp exposes a hosted Factory endpoint. Codex removed its MCP server and pushes integrators to its non-MCP app-server.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `codex mcp-server` and the `codex-mcp-server` binary removed; integrations must move to the app-server, which is not MCP. | deprecated (removed) | https://learn.chatgpt.com/docs/mcp-server.md | reviewed |
| Claude Code | `claude mcp serve` exposes its own tools over stdio; the client must confirm calls; no end-of-output marker for background commands. Separately, the IDE extensions host a loopback `ws://` MCP server named `ide` (per-activation token in a 0600 lock file) that the CLI connects to. | GA | https://code.claude.com/docs/en/mcp.md ; https://code.claude.com/docs/en/vs-code.md | unreviewed |
| OpenClaw | `openclaw mcp serve` exposes channel conversations to external MCP clients. | GA | https://docs.openclaw.ai/cli/mcp | unreviewed |
| goose | Built-in extensions are standalone MCP servers usable by other agents (`goose mcp <name>`). | GA | https://goose-docs.ai/docs/getting-started/using-extensions.md | unreviewed |
| Hermes Agent | `mcp-serve` plugin exposes messaging conversations and optionally web, browser, vision, image, skills and TTS tools; replaces `hermes mcp serve`. | GA (plugin) | https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp | unreviewed |
| Warp | Hosted Factory MCP endpoint; built-in server from 2026.08.18, automatic access for Claude Code and Codex runs from 2026.09.09. | beta (Factories early access) | https://docs.warp.dev/platform/architecture/ ; https://docs.warp.dev/changelog/2026/ | corrected |
| rizzy-harness (ours) | none. | none | `own-rizzy-harness.md` §9 (grep finds no "mcp") | from own inventory |

### 2.3 Skills

#### Skill format and discovery
What it is: a folder with `SKILL.md` (YAML frontmatter plus Markdown instructions) and optional scripts and references, discovered from user, project and bundled locations. The Agent Skills format (agentskills.io) is the shared base; `.agents/skills/` is the shared project path.
Variants: the frontmatter keys diverge (Claude Code adds `context: fork`, `hooks`, `paths`, `allowed-tools`, `model`; OpenHands adds `triggers` and `paths`; OpenClaw gates by `metadata.openclaw.requires`; Hermes `requires_*`/`fallback_for_*`; Codex puts UI and invocation flags in `agents/openai.yaml`). Precedence diverges: Claude Code enterprise > personal > project; OpenHands, OpenClaw, Hermes project first; Cline global wins over project. Project skills need trust in Hermes, Zed and Pi templates.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `.agents/skills` from cwd up to repo root, `~/.agents/skills`, `/etc/codex/skills`, bundled `skill-creator` and `skill-installer`; `agents/openai.yaml` sets UI metadata, `allow_implicit_invocation`, tool dependencies; symlinks followed. | GA | https://learn.chatgpt.com/docs/build-skills.md ; https://learn.chatgpt.com/docs/skills-and-plugins.md | reviewed |
| OpenCode | `.opencode/skills`, `~/.config/opencode/skills`, plus `.claude/skills` and `.agents/skills`; HTTP catalogs via `index.json`; `license`/`compatibility` accepted but ignored. | GA | https://opencode.ai/docs/skills/ | reviewed |
| Claude Code | Agent Skills standard plus extensions; locations enterprise, personal, project (cwd and parents), nested subdirs (loaded on first touch), `--add-dir`, plugins, claude.ai account; precedence enterprise > personal > project; names with bidi, zero-width, control chars or `<`/`>` refused; legacy `.claude/commands/*.md` merged in. Only six frontmatter fields accepted for claude.ai uploads and the Skills API. | GA | https://code.claude.com/docs/en/skills.md | reviewed |
| Claude Desktop | `~/.claude/skills/` for the Code tab plus account-synced skills; Cowork reads only the account-synced set. Local scheduled tasks are stored as `SKILL.md` files. | GA | https://code.claude.com/docs/en/desktop ; https://claude.com/docs/cowork/overview.md | unreviewed |
| DeepSeek harness | `<name>/SKILL.md` or flat `<name>.md`, kebab-case; roots `.dsh/skills`, `.agents/skills`, custom, home, bundled; project root is nearest `.git`; file watcher; no recursive discovery. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/skills | unreviewed |
| Oh My Pi | One level deep; `.omp/skills`, user, managed, plus `.agent(s)`, `.claude`, `.codex`, `.opencode`, `.github` skills dirs; allow/ignore lists; per-provider switches. | GA | https://omp.sh/docs/skills | unreviewed |
| Pi | User and project skill dirs plus `~/.agents/skills/` and `.agents/skills/` (up to repo root); name max 64, description max 1024; duplicates keep the first. | GA | https://pi.dev/docs/latest/skills | unreviewed |
| Cursor | agentskills.io standard in `.agents/skills/`, `.cursor/skills/`, user equivalents, plus `.claude/skills/` and `.codex/skills/`; `paths` scoping; cloud agents only get synced `~/.cursor/skills/`. | GA | https://cursor.com/docs/skills | unreviewed |
| Antigravity | Required `SKILL.md` (`description` required) plus `scripts/`, `examples/`, `resources/`; `.agents/skills/` and parents, `~/.gemini/config/skills/`, plugin skills `<plugin>:<skill>`; discovery manifests (`skills.json`) and a personal folder inside a shared repo that outranks `.agents/`. | GA | https://antigravity.google/docs/skills ; https://antigravity.google/docs/customization-discovery | unreviewed |
| OpenHands | Agent Skills compatible with OpenHands `triggers` (keyword injected) and `paths` (path-triggered rules injected once); `.agents/skills/`, `~/.agents/skills/`, public registry; legacy `.openhands/skills` and microagents. | GA | https://docs.openhands.dev/overview/skills.md | unreviewed |
| OpenClaw | Precedence workspace, project `.agents/skills`, personal, managed, workshop (agent-learned), bundled, extras; gating by `requires.bins/env/config` and `os`; per-skill env injected on host, not into the sandbox; third-party skills to be treated as untrusted code. | GA | https://docs.openclaw.ai/tools/skills | unreviewed |
| goose | `~/.agents/skills/`, `.agents/skills/`, plugin dirs, plus `.goose/skills/` and `.claude/skills/`; built-in `web-search` skill. | GA | https://goose-docs.ai/docs/guides/context-engineering/using-skills.md | unreviewed |
| Hermes Agent | agentskills.io with `version`, `platforms`, `metadata.hermes`; conditional activation; precedence project, local, `create_dir`, `external_dirs`; project-local skills need `hermes skills trust`; about 90 bundled, 60 optional. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/skills | unreviewed |
| Zed/ACP | `~/.agents/skills/` or `<worktree>/.agents/skills/` (trusted worktrees only; project overrides global); not all agentskills.io fields; Zed Agent only (not external agents). | GA | https://zed.dev/docs/ai/skills.md | unreviewed |
| Warp | Walks cwd to repo root across `.agents/skills/` and nine other tools' dirs (`.warp`, `.claude`, `.codex`, `.cursor`, `.gemini`, `.copilot`, `.factory`, `.github`, `.opencode`) and home equivalents; `WARP_SKILL_DIRS` (cloud); file-based, not synced. | GA | https://docs.warp.dev/agents/capabilities/skills/ | reviewed |
| Cline | `name` must equal the dir name, description up to 1024; `.cline/skills/`, `.clinerules/skills/`, `.claude/skills/`, global `~/.cline/skills/`; global wins over project. | GA | https://docs.cline.bot/customization/skills.md | reviewed |
| rizzy-harness (ours) | none. PLAN says agents get "attached skills"; no mechanism. Closest: agent definition files (`<data>/agents/<id>.md`, frontmatter, unknown fields rejected). | docs-only | `PLAN.md:83-90`, `crates/agents/src/lib.rs:1-33` | from own inventory |

#### Skill loading, listing budget and invocation control
What it is: how much of each skill sits in context (usually name plus description), how the body is loaded, and who may invoke it (user, model, both).
Variants: budgets: Codex 2% of context or 8,000 chars (cap 10,000 tokens), Claude Code 1,536 chars per skill (the 1% total budget is unverified), Zed 50 KB catalog, Hermes about 3k tokens for `skills_list`, Cline about 100 tokens per skill. Overflow: Codex shortens descriptions then omits skills with a warning; Claude Code drops descriptions of the least-invoked skills first; OpenClaw adds `skills_search`/`skills_read` for omitted ones. Invocation: `disable-model-invocation` (Claude Code, DSH, OMP, Pi, Cursor, Zed), `user-invocable` (Claude Code, DSH, OpenClaw), `autoinvoke: false` (OpenCode). Stacking: Claude Code up to six per message, Hermes up to five.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Initial list at most 2% of context or 8,000 chars; descriptions shortened, then skills omitted with a warning; `skills.max_context_tokens` capped at 10,000; explicit `$skill` or implicit by description. | GA | https://learn.chatgpt.com/docs/build-skills.md | reviewed |
| OpenCode | Each step lists permitted skills (ID, name, description); model loads body with the `skill` tool; users load with `@skill-id`; permission action `skill`; `autoinvoke: false`. | GA | https://opencode.ai/docs/skills/ | reviewed |
| Claude Code | Name and description always in context, body on invocation; `disable-model-invocation`, `user-invocable: false`; 1,536-char listing cap; least-invoked descriptions dropped first; `skillOverrides` on/name-only/user-invocable-only/off; `/skill-doctor` reports context cost and unused skills; up to six stacked skills; `allowed-tools` pre-approves only for the invoking turn; invoked content stays across turns. | GA | https://code.claude.com/docs/en/skills.md | reviewed (1% total budget: unverified per review 2) |
| DeepSeek harness | Names and descriptions injected as a system reminder in the first step; `skill({name})` loads the body; `disable-model-invocation`, `user-invocable`. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/skills | unreviewed |
| Oh My Pi | Natural matching or `/skill:<name>`; hide and disable-model-invocation flags. | GA | https://omp.sh/docs/skills | unreviewed |
| Pi | Names and descriptions in the system prompt, body on demand or `/skill:name`; `disable-model-invocation`; `allowed-tools` experimental. | GA | https://pi.dev/docs/latest/skills | unreviewed |
| Cursor | Auto-applied by description, `/name` manual, `disable-model-invocation`, `paths` scoping. | GA | https://cursor.com/docs/skills | unreviewed |
| Antigravity | Names and descriptions every turn, full file on activation; CLI makes every skill a slash command; `/skills reload`. | GA | https://antigravity.google/docs/skills | unreviewed |
| OpenHands | Only name/description preloaded; `triggers` inject on keywords; `paths` rules inject once when a matching file is touched. | GA | https://docs.openhands.dev/overview/skills.md | unreviewed |
| OpenClaw | Eligible skills snapshotted per session into a compact list; `skills_search`/`skills_read` for omitted ones; per-agent allowlists limit visibility only; watcher plus 30-min rescan. | GA | https://docs.openclaw.ai/tools/skills | unreviewed |
| goose | Names and descriptions added at session start; full text when a request matches or by name. | GA | https://goose-docs.ai/docs/guides/context-engineering/using-skills.md | unreviewed |
| Hermes Agent | `skills_list` (about 3k tokens), `skill_view`, `skill_view` with path; up to five stacked; `skill-bundles` group skills. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/skills | unreviewed |
| Zed/ACP | Catalog capped at 50 KB; model calls the `skill` tool; user skills need permission, built-in ones do not. | GA | https://zed.dev/docs/ai/skills.md | unreviewed |
| Cline | Metadata always (about 100 tokens each), body on trigger (under 5k tokens), resources on demand; only script output enters context. | GA | https://docs.cline.bot/customization/skills.md | reviewed |
| rizzy-harness (ours) | none. | none | n/a | from own inventory |

#### Skills that run shell commands while loading
What it is: placeholders inside a skill or command that run a shell command before the prompt reaches the model, splicing the output in.
Variants: the decisive difference is the approval path. Claude Code checks each injected command against permission rules and aborts the invocation on anything but allow; OpenCode runs command `` !`cmd` `` blocks outside the permission flow, which its docs warn about.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | Custom commands' `` !`cmd` `` injects shell output before submission and runs OUTSIDE the permission flow. | GA | https://opencode.ai/docs/commands/ | reviewed |
| Claude Code | `` !`cmd` `` and ```` ```! ```` blocks run via the Bash tool in one non-recursive pass; non-zero exit aborts; commands never prompt: anything other than an allow rule aborts the invocation; `disableSkillShellExecution` blanks them for user/project/plugin skills; synced claude.ai skills never run them locally. | GA | https://code.claude.com/docs/en/skills.md | reviewed |
| rizzy-harness (ours) | none. | none | n/a | from own inventory |

#### Agent-written skills and extensions
What it is: the agent creates or edits its own skills, rules or extensions, turning experience into reusable procedures.
Variants: approval gates differ: Antigravity `/learn` writes only after explicit approval of a proposal artifact; Hermes `skill_manage` writes directly by default (staging for approval is optional) and a curator archives, consolidates and can roll back; Claude Code's `/verify` writes a project skill itself and mods Claude writes need per-file approval; Zed forbids the agent editing `SKILL.md` without explicit authorisation.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/verify` writes `.claude/skills/verify/SKILL.md` when it had to discover the recipe; `/run-skill-generator` records a launch recipe as a project skill; built-in `plugin-authoring` skill writes mods into a protected dev folder (each file needs approval in default/acceptEdits; mods Claude writes turn on without asking in auto and bypass modes). | GA | https://code.claude.com/docs/en/skills.md ; https://code.claude.com/docs/en/plugins/mods/create.md ; https://code.claude.com/docs/en/changelog.md | unreviewed |
| DeepSeek harness | A "creation mode" chat can write a plugin; `plugin_manager` tool (install, enable, remove) needs danger-full-access or approval. | preview | https://deepseek.com/harness ; https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | reviewed |
| Oh My Pi | Memory toolset (off by default) includes `manage_skill`; `/omfg` turns a complaint into a TTSR rule. | GA (tool off by default) | https://omp.sh/docs/tools ; https://omp.sh/docs/slash | unreviewed |
| Antigravity | `/learn` classifies lessons as rule or skill, drafts `learning_proposal.md`, writes only after explicit approval. | GA | https://antigravity.google/docs/commands/learn | reviewed |
| OpenClaw | A "workshop" tier of agent-learned skills sits in load precedence; mechanism not documented on the page read. | GA (mechanism undocumented) | https://docs.openclaw.ai/tools/skills | unverified |
| Hermes Agent | `skill_manage` create/patch (preferred)/rewrite/delete; advisory linter; `write_approval` default false; curator (deterministic stale/archive at 14/30 days, opt-in LLM consolidation, pin and lock, tar.gz snapshots, sha256 ledger, per-entry rollback). | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/skills ; https://hermes-agent.nousresearch.com/docs/user-guide/features/curator | reviewed |
| Zed/ACP | The agent cannot edit `SKILL.md` without explicit authorisation; users author with `/create-skill`. | GA | https://zed.dev/docs/ai/skills.md | unreviewed |
| Cline | `/newrule` creates a rule file. | GA | https://docs.cline.bot/core-workflows/using-commands.md | unreviewed |
| rizzy-harness (ours) | none. The analogous design is memory: the model only proposes facts, a human confirms. | none (skills); works (memory proposal) | `crates/memory/src/lib.rs:34-48` | from own inventory |

#### Skill registries and account sync
What it is: a remote source of skills, either a public registry or skills synced from a user's account.
Variants: scanning is the difference: Hermes scans every install for exfiltration, injection and destructive commands, and `--force` cannot override a `dangerous` verdict; ClawHub runs automated scans with held and blocked states; Claude Code's synced skills never run `!` commands locally; OpenCode just reads an `index.json` catalog.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Bundled `skill-installer` system skill; plugins share one "universal" directory with ChatGPT. | GA | https://learn.chatgpt.com/docs/build-skills.md ; https://learn.chatgpt.com/docs/plugins.md | reviewed |
| OpenCode | HTTP skill catalogs via `index.json`. | GA | https://opencode.ai/docs/skills/ | reviewed |
| Claude Code | claude.ai account skills download to `~/.claude/skills/synced/` (about every 10 min active), download-only, `!` commands never run locally, `syncClaudeAiSkills: false` opts out; `anthropics/skills` topic marketplace. | GA | https://code.claude.com/docs/en/skills.md ; https://code.claude.com/docs/en/plugins/anthropic-marketplaces.md | reviewed |
| Claude Desktop | Skills and plugins enabled on the account sync into Cowork and Claude Code. | GA | https://claude.com/docs/cowork/overview.md | unreviewed |
| Pi | Packages gallery (5,592 packages at read time) bundles skills with other resources. | GA | https://pi.dev/packages | unreviewed |
| OpenHands | Public registry `OpenHands/extensions`. | GA | https://docs.openhands.dev/overview/skills.md | unreviewed |
| OpenClaw | ClawHub (clawhub.ai) for skills and plugins; versioned bundles; publishing needs a GitHub account meeting an age requirement; automated scans with held/blocked states; telemetry opt-out. | GA | https://docs.openclaw.ai/clawhub ; https://docs.openclaw.ai/clawhub/security | unreviewed |
| Hermes Agent | Skills Hub sources: official, skills-sh, well-known URL index, direct URL, GitHub taps, clawhub, lobehub, browse-sh; trust levels builtin/official/trusted/community; scan on every install. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/skills | reviewed |
| Zed/ACP | `zed://skill` share links; create from a GitHub URL. | GA | https://zed.dev/docs/ai/skills.md | unreviewed |
| Cline | `cline skill` wraps `npx skills`; Desktop Marketplace. | GA | https://docs.cline.bot/cli/cli-reference.md ; https://docs.cline.bot/usage/cline-desktop.md | reviewed |
| rizzy-harness (ours) | none. | none | n/a | from own inventory |

### 2.4 Plugins and extensions (packaging and distribution)

#### Plugin bundles: what a plugin can contain
What it is: one installable unit that packages skills, commands, agents, hooks, MCP servers and sometimes code, themes or UI.
Variants: Claude-Code-shaped bundles dominate (Codex `.codex-plugin`, OpenHands `.plugin`, Cursor `.cursor-plugin`, Antigravity `plugin.json`), and several read each other's layouts. Code-bearing packages are a second family: Pi packages and OMP plugins carry TypeScript extensions; OpenCode npm packages carry server, TUI, GUI and RPC parts; Hermes and OpenClaw plugins add providers, platforms and single-slot memory or context engines. Claude Desktop shows the cost of one format across runtimes: component support differs per surface.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Bundle of skills, MCP servers and hooks (`.codex-plugin/plugin.json`); IDE extension does not support plugins; plugin hooks get `PLUGIN_ROOT` and `CLAUDE_PLUGIN_ROOT` for compatibility; app-server install API "under development". | GA | https://learn.chatgpt.com/docs/plugins.md ; https://learn.chatgpt.com/docs/hooks.md | unreviewed |
| OpenCodex | (Part B, RyensX/OpenCodex, a different project) plugin system with external plugin directories and a Plugin v2 SDK. | GA | https://github.com/RyensX/OpenCodex/releases | reviewed |
| OpenCode | One npm package with four parts: server, TUI (OpenTUI 0.8+), GUI (Electron, experimental), RPC (typed contract). V1 plugins do not run in V2. | server/TUI GA; GUI experimental | https://opencode.ai/docs/plugins/ ; https://opencode.ai/docs/build/plugins/ ; https://opencode.ai/docs/build/plugins/migrate-v1/ | unreviewed |
| Claude Code | Directory of skills, commands, agents, hooks, a JS hooks module (makes it a mod), MCP and LSP servers, monitors (experimental), output styles, themes, workflows, `bin/` (appended after the user's PATH), default settings, channels; optional `.claude-plugin/plugin.json`; enabled plugins cost context every turn (names and descriptions); `.mcpb` bundles allowed; plugin-root `CLAUDE.md` not loaded. | GA (themes, monitors, evals experimental) | https://code.claude.com/docs/en/plugins/overview.md ; https://code.claude.com/docs/en/plugins/components.md ; https://code.claude.com/docs/en/plugins/manifest-reference.md | unreviewed |
| Claude Desktop | Same format across Chat, Cowork, Code with different components: agents and hooks ignored in Chat, local MCP only when Cowork runs locally, `bin/` not in Chat/Cowork, LSP/output styles/themes/settings Code-only; zip upload 200 MB, 5,000 files. | GA | https://claude.com/docs/plugins/platform-support.md | reviewed |
| DeepSeek harness | Everything (model adapter, tool registry, session log, loop) is a Cordis plugin with reversible registrations; plugins add tools, skills, UI slots; profiles compose them via YAML patches. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/ | reviewed |
| Oh My Pi | Bundle of extension code, skills, commands, hooks, tools, agents, rules, prompts, MCP/LSP/DAP config; optional features; typed settings. | GA | https://omp.sh/docs/plugins | reviewed |
| Pi | Package bundles extensions, skills, prompts and themes as npm, git or local; `pi` key in `package.json`. | GA | https://pi.dev/docs/latest/packages | unreviewed |
| Cursor | Rules, skills, agents, commands, MCP servers and hooks (`.cursor-plugin/plugin.json`; also the open Agent Plugins `plugin.json`). | GA (no label) | https://cursor.com/docs/plugins | unreviewed |
| Antigravity | `plugin.json` (or `.antigravity-plugin/`, `.gemini-plugin/`) with skills, MCP servers (namespaced), agents, rules, hooks; hooks "run automatically" and are called the main execution risk; plugins needing config install disabled. | GA | https://antigravity.google/docs/plugins | unreviewed |
| OpenHands | Skills, `hooks/hooks.json`, `.mcp.json`, `agents/*.md`, `commands/*.md` under `.plugin/plugin.json`; described as compatible with the Claude Code plugin structure. | GA | https://docs.openhands.dev/sdk/guides/plugins.md | unreviewed |
| OpenClaw | Native plugins (`openclaw.plugin.json`, in-process) add channels, providers, tools, skills, speech; bundle compatibility maps Codex, Claude, Cursor and Agent Plugin layouts; exclusive slots for `memory` and `contextEngine`; all plugin APIs called experimental. | GA | https://docs.openclaw.ai/tools/plugin ; https://docs.openclaw.ai/concepts/experimental-features | unreviewed |
| goose | Plugin directory (Open Plugins or Gemini extension manifest) with skills and/or `hooks/hooks.json`. | unclear (no label) | https://goose-docs.ai/docs/guides/context-engineering/plugins.md | unreviewed |
| Hermes Agent | Register tools, slash commands, CLI commands, lifecycle hooks, platform adapters, media/delegation/terminal/computer-use providers; single-slot memory provider and context engine. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/plugins | reviewed |
| Zed/ACP | Extensions, including MCP servers; extension-provided ACP agents are deprecated. | GA | https://zed.dev/docs/ai/mcp ; https://zed.dev/docs/ai/external-agents.md | unreviewed |
| Warp | Ships plugins only for other agents (Claude Code, Codex, OpenCode) to send notifications to Warp; nothing documented for Warp's own agent. | GA | https://docs.warp.dev/agents/cli-agents/claude-code/ | unreviewed |
| Cline | SDK `AgentPlugin` (name, manifest capabilities, `setup`, hooks) adding tools and hooks. | GA | https://docs.cline.bot/sdk/plugins.md | reviewed |
| rizzy-harness (ours) | none. PLAN §0 cuts "Plugins". | none | `PLAN.md:101-110` | from own inventory |

#### Plugin distribution and marketplaces
What it is: where plugins come from (registry, git, npm, local folder), how a catalog is described and how updates arrive.
Variants: catalog file formats converge on `marketplace.json` (Claude Code; OMP falls back to `.claude-plugin/marketplace.json`). Sources: npm (Claude Code never runs install scripts; Pi, OMP, OpenCode, DSH, Cline use npm), git with pinning, archives with sha256 (Claude Code), shell `command` sources shown for review (Claude Code). Update policy differs: Claude Code auto-updates official marketplaces but not third-party; OMP defaults to `notify` (debug log only), with `auto` letting a publisher ship code without approval; goose `--auto-update` opt-in. Review: Cursor marketplace listings are manually reviewed and must be open source; Anthropic does not review third-party marketplaces.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `/plugins`, `codex plugin marketplace`; Git, local or remote marketplaces; admins can sync one from GitHub. | GA | https://learn.chatgpt.com/docs/plugins.md | unreviewed |
| OpenCode | `opencode plugin add|list|check|update|remove`; npm or Git specs (tags, commits), local dirs, auto-discovery in `.opencode/plugins/`; `-id` disables. | GA | https://opencode.ai/docs/plugins/ | unreviewed |
| Claude Code | Sources: relative path, `github` (ref/sha), git `url`, `git-subdir`, `npm` (scripts never run), `archive` (HTTPS zip, optional sha256), `command` (shown for review, re-run per session). Official, community and topic marketplaces; semver dependencies resolved against git tags; scopes user/project/local; auto-update with random delay; reload refuses changes that would invalidate the prompt cache without `--force`. | GA (npm marketplace source not implemented) | https://code.claude.com/docs/en/plugins/marketplace-reference.md ; https://code.claude.com/docs/en/plugins/loading.md ; https://code.claude.com/docs/en/plugins/dependencies.md ; https://code.claude.com/docs/en/plugins/cli-reference.md | reviewed (sources); rest unreviewed |
| Claude Desktop | Customize > Plugins attaches to the account and syncs into Claude Code; up to 25 self-added marketplaces; admins mark Not available / Available / Installed by default / Required; org sync from GitHub or GitLab. | GA | https://claude.com/docs/plugins/platform-support.md | reviewed |
| DeepSeek harness | Third-party market indexes; install shows a confirmation with the source (npm, git repo/commit/directory); markets untrusted, format validated not claims; `http:` indexes tamperable; compatibility and registry checks. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/plugin-market | reviewed |
| Oh My Pi | npm, git shorthand (github/gitlab/bitbucket/codeberg/sourcehut), URL, local dir (symlink), marketplace; `.omp-plugin/marketplace.json` with fallback to `.claude-plugin/marketplace.json`; `marketplace.autoUpdate` off / notify (default) / auto. | GA | https://omp.sh/docs/plugins ; https://omp.sh/docs/marketplace | reviewed |
| Pi | `pi install|remove|list|update|config`, `pi -e`; npm specs and git tags/commits pin; user or project scope (project needs trust); `pi-package` keyword lists it in the gallery. | GA | https://pi.dev/docs/latest/packages | unreviewed |
| Cursor | Marketplace listings manually reviewed, must be open source, updates reviewed; team marketplaces (Teams one plus Default; Enterprise unlimited) with Default Off/On/Required per plugin. | GA | https://cursor.com/docs/plugins | unreviewed |
| Antigravity | Official `antigravity-plugins-official`, GitHub URL, local dir; `agy plugin validate --strict`; partner plugins added v2.28.2; vetting process undocumented. | GA | https://antigravity.google/docs/plugins ; https://antigravity.google/docs/changelog | unreviewed |
| OpenHands | Local path, `github:owner/repo`, Git URL with ref, marketplaces (`plugin@marketplace`), install under `~/.openhands/plugins/installed/`; Enterprise plugin marketplace. | GA | https://docs.openhands.dev/sdk/guides/plugins.md ; https://docs.openhands.dev/enterprise/architecture.md | unreviewed |
| OpenClaw | ClawHub (default), npm, git, local, marketplace. | GA | https://docs.openclaw.ai/tools/plugin | unreviewed |
| goose | `goose plugin install <git-url> [--auto-update]`, `goose plugin update`; user and project scope; `disabledPlugins`. | unclear | https://goose-docs.ai/docs/guides/context-engineering/plugins.md | unreviewed |
| Hermes Agent | Bundled, `~/.hermes/plugins/`, project `.hermes/plugins/` (not loaded by the gateway), pip entry points, Nix; curated SHA-pinned catalog of 1,050+ entries (release notes). | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/plugins ; https://github.com/NousResearch/hermes-agent/releases | reviewed |
| Zed/ACP | Extensions (including MCP servers); ACP agents install from the ACP registry (`registry.json` on a CDN, only agents with an auth method, submissions by PR). | GA | https://zed.dev/docs/ai/mcp ; https://agentclientprotocol.com/get-started/registry.md | reviewed (registry) |
| Cline | `cline plugin install` (npm, git, local, file URL), `config.extensions`, `pluginPaths`. | GA | https://docs.cline.bot/sdk/plugins.md | reviewed |
| rizzy-harness (ours) | none. | none | n/a | from own inventory |

#### Plugin trust, isolation and supply-chain controls
What it is: what stops an installed plugin, skill or extension from doing harm, and what the docs admit.
Variants: almost nobody sandboxes plugin code. Stated "not a sandbox": Claude Code (plugins run arbitrary code with user privileges; mods not sandboxed), OMP, Pi, OpenCode (no plugin sandbox section), OpenClaw (about 150 public entrypoints), Hermes ("capabilities are consent, not isolation"), OpenHands Canvas Apps. Partial isolation: Cline runs plugins in a per-session worker thread (crash isolation only) with an opt-in sandbox subprocess; Hermes offers optional host-process isolation. Supply-chain controls instead: Claude Code hardened npm dependency install (exact pins, `--ignore-scripts`, frozen, separate folder) and repo plugins gated by folder trust; Codex hashes hooks including plugin scripts; ClawHub and Hermes Hub scans; goose malware check; Cursor human review.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Non-managed hooks, including a changed plugin script with the same definition, are skipped until re-reviewed (hash trust). | GA | https://learn.chatgpt.com/docs/hooks.md | corrected (status label) |
| OpenCode | No plugin sandbox or security section; managed policies can withhold plugins but are not a sandbox. | GA | https://opencode.ai/docs/plugins/ | unreviewed |
| Claude Code | Plugins run arbitrary code with user privileges; hooks, monitors, MCP/LSP servers and mod-started processes run outside the sandbox and outside permission rules; repository plugins load only after that folder's trust dialog, and their MCP servers need per-server approval; Node deps installed hardened (registry only, exact pins, `--ignore-scripts`, no `.npmrc`/`.env`); `command` sources need re-acceptance when changed; `-y` ignored when Claude itself runs the install. | GA | https://code.claude.com/docs/en/plugins/security.md ; https://code.claude.com/docs/en/plugins/loading.md ; https://code.claude.com/docs/en/plugins/cli-reference.md | unreviewed |
| Claude Desktop | Org governance: per-plugin availability, allow/deny directory extensions, private extensions and marketplaces; sensitive extension config in the OS keychain. | GA | https://claude.com/docs/plugins/platform-support.md ; https://www.anthropic.com/engineering/desktop-extensions | unreviewed |
| DeepSeek harness | Confirmation dialog shows the source; exact-version exemption for incompatible plugins; signing and in-process execution not documented (survey inference). | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/plugin-market | reviewed |
| Oh My Pi | "Trusted code, not a sandbox"; no signing, sandboxing or central review. | GA | https://omp.sh/docs/plugins | reviewed |
| Pi | Extensions run with full Pi process permissions; packages execute code and skills can instruct the model to run programs; no vetting or signing. | GA | https://pi.dev/docs/latest/extensions ; https://pi.dev/docs/latest/packages | unreviewed |
| Cursor | Marketplace listings manually reviewed, open source required, updates reviewed. | GA | https://cursor.com/docs/plugins | unreviewed |
| Antigravity | Hooks and sidecars are local executables run with user privileges; vetting and how permissions apply to plugin components undocumented. | GA | https://antigravity.google/docs/plugins | unreviewed |
| OpenHands | Canvas Apps run in the Canvas browser context with no sandbox; optional iframe backend view is sandboxed with server-issued tokens. | beta | https://docs.openhands.dev/openhands/usage/agent-canvas/canvas-extensions.md | reviewed |
| OpenClaw | `plugins.deny` beats allow; workspace-origin plugins disabled by default; `security.installPolicy` allow/warn/block; plugins not sandboxed; `openclaw security audit --deep` scans plugin and skill code. | GA | https://docs.openclaw.ai/tools/plugin | unreviewed |
| goose | Malware check before activating external extensions (mechanism undocumented); admin allowlist. | unclear | https://goose-docs.ai/docs/getting-started/using-extensions.md ; https://goose-docs.ai/docs/guides/allowlist.md | reviewed |
| Hermes Agent | Skills Hub scans every install; `--force` cannot override `dangerous`; gateway hooks load in-process with no sandbox or consent prompt; plugin trust model "capabilities are consent, not isolation". | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/skills ; https://hermes-agent.nousresearch.com/docs/user-guide/features/hooks | reviewed |
| Cline | Plugins run in a per-session worker thread (crash isolation, "not a security boundary"); opt-in `CLINE_PLUGIN_MODE=sandbox` subprocess; SDK v0.0.94 removed `in_process` mode. | GA | https://docs.cline.bot/sdk/plugins.md | reviewed (release-note detail not rechecked) |
| rizzy-harness (ours) | none (no plugins). Compiled-in tool set; the only external helper, `graphify`, needs `graphify = true` in `node.toml`. | none | `crates/tools/src/lib.rs:385-396`, `crates/tools/src/graphify.rs` | from own inventory |

#### Managed (admin) controls over extensions
What it is: settings that only an administrator source can set, restricting which hooks, plugins, skills and MCP servers may load.
Variants: Claude Code has the widest set (managed-only hooks, `strictPluginOnlyCustomization`, marketplace allow and block lists checked before download, `disableSideloadFlags`, MCP allow/deny); Codex `requirements.toml` with `allow_managed_hooks_only`; Cursor enterprise and team hooks via MDM and dashboard; goose an admin-hosted URL allowlist; Cline per-surface enterprise toggles. The node-side analogue (the machine that runs tools decides) is rare: most of these are pushed from a vendor console.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Managed hooks in `requirements.toml`; `allow_managed_hooks_only`; project config cannot set provider, base URLs, `notify`, `otel`. | GA | https://learn.chatgpt.com/docs/hooks.md | corrected (status label) |
| OpenCode | Admin-only `policies` layer (system/MDM/Console) can withhold plugins. | GA | https://opencode.ai/docs/plugins/ | unreviewed |
| Claude Code | `allowManagedHooksOnly`, `disableAllHooks`, `strictPluginOnlyCustomization` (skills/agents/hooks/mcp), `strictKnownMarketplaces`, `blockedMarketplaces`, `disableSideloadFlags`, `disableCommandPluginSources`, `allowManagedModsOnly`, MCP allow/deny and `managed-mcp.json`; lists apply before download and again at session start. Mods guard loads only with managed settings or Team/Enterprise login. | GA | https://code.claude.com/docs/en/settings-reference.md ; https://code.claude.com/docs/en/managed-settings.md ; https://code.claude.com/docs/en/plugins/org.md ; https://code.claude.com/docs/en/plugins/mods/admin.md | unreviewed (mods guard: reviewed, review 2) |
| Claude Desktop | Owners set per-plugin availability, sync plugin repos, allow/deny directory extensions; system policy overrides in-app lists. | GA | https://claude.com/docs/plugins/platform-support.md | unreviewed |
| Cursor | Hook sources by priority: enterprise MDM file, team dashboard (synced about every 30 min), project, user, plugins; team marketplaces with Required plugins; enterprise MCP allowlists. | GA | https://cursor.com/docs/hooks ; https://cursor.com/docs/plugins | corrected |
| OpenHands | Enterprise org MCP servers and plugin marketplace. | GA | https://docs.openhands.dev/enterprise/architecture.md | unreviewed |
| OpenClaw | `plugins.deny`, `security.installPolicy`. | GA | https://docs.openclaw.ai/tools/plugin | unreviewed |
| goose | `GOOSE_ALLOWLIST` URL with exact command match (HTTPS-only with redirect validation since v1.57.0). | GA | https://goose-docs.ai/docs/guides/allowlist.md | reviewed |
| Cline | `mcpMarketplaceEnabled`, `allowedMCPServers`; remote config does not cover skills on CLI/Desktop. | GA | https://docs.cline.bot/enterprise-solutions/configuration/infrastructure-configuration/control-other-cline-features/mcp-server-controls.md | unreviewed |
| rizzy-harness (ours) | No extension controls (no extensions). The authority model exists: `node.toml` is the only authority, unknown keys rejected, nothing over HTTP or protocol writes it. | works (node policy) | `crates/node/src/policy.rs:9-105`, `:112-126` | from own inventory |

### 2.5 In-process middleware

#### In-process extension API (code hooks into the loop)
What it is: extension code loaded into the harness process that can observe, rewrite or answer engine events (prompts, tool calls, model requests, compaction) and register tools, commands and UI.
Variants: scope ranges from tool-call gates (Cline SDK hooks) to nearly every decision (Claude Code mods: tool checks, prompt sections, model requests, stored transcript rows, settings merge, other mods' API calls). Isolation: Claude Code mods have no Node APIs of their own and must use a `$` API that static analysis can enumerate, but they are not sandboxed and fail open by default; Pi, OMP, OpenCode, OpenClaw and Hermes run with full process rights; DSH makes the whole runtime plugins with reversible registrations. Precedence against policy: OpenCode `permission.evaluate` cannot change a configured deny; OMP hooks cannot bypass approval policy; a Claude Code mod can approve past deny rules unless the guard is loaded.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | `Plugin.define({id, setup(ctx)})`; hook points for session prompt, context, compaction, generate, title, model request, HTTP request/response, WS handshake, retry, `permission.evaluate`, `shell.create.before`, `tool.execute.before/after`; tools via `ctx.tool.transform`; `evaluate` can change allow/ask but not configured deny. | GA (some hooks experimental) | https://opencode.ai/docs/build/plugins/server/ | unreviewed |
| Claude Code | Mods: plugin hooks module exporting `register(on)`; middleware `($, e, next)` that observes, rewrites or answers; events for tools, prompts, commands, config, turns (`turn.step` can swap model or answer), sessions, subagents, UI, telemetry, every settings hook as `classic.<Event>`, every `$` API call. Limits: 10 s per hook (50 ms `prompt.edit`), `$.process.run` 30 s default. Not sandboxed; fail-open by default unless a `.catch` handler is added; three worker crashes unload all non-built-in mods; `--safe-mode` drops them. Built-in features (AGENTS.md, `/diff`, the security guard, telemetry) are mods. A mod's own `$.tool.call`/`$.mcp.call` skip permission prompts (since 2.1.321). | GA (on by default from v2.1.287) | https://code.claude.com/docs/en/plugins/mods/overview.md ; https://code.claude.com/docs/en/plugins/mods/reference.md ; https://code.claude.com/docs/en/plugins/mods/events.md ; https://code.claude.com/docs/en/plugins/mods/admin.md | reviewed (fail-open, guard and admin caveats added from review 2) |
| Claude Desktop | Mods run in the Desktop Code tab per Claude Code's pages; Desktop-specific extension beyond that: nothing documented. | unclear | https://claude.com/docs/plugins/platform-support.md | unreviewed |
| DeepSeek harness | Cordis: everything is a plugin, registrations are reversible effects unwound on unload; plugin pipeline waterfalls `tools/pre-execute`, `tools/post-execute`; optional HMR; core APIs still changing. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/ ; https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/tools | reviewed |
| Oh My Pi | Extensions API (`registerTool`, `registerCommand`, shortcuts, flags, renderers, providers, UI); hooks are trusted TS/JS modules. | GA | https://omp.sh/docs/extension-authoring ; https://omp.sh/docs/hooks | unreviewed |
| Pi | TypeScript extensions loaded in-process via jiti; events at input, run lifecycle, provider level (`before_provider_request`, `after_provider_response`, `provider_stream_event`, `cache_warming_decision`), tools, sessions, `project_trust`; register tools, commands, providers, MCP servers, TUI; about 70 examples. | GA | https://pi.dev/docs/latest/extensions | unreviewed |
| OpenHands | Canvas Apps: custom sidebar pages, one bundled ESM entry `activate(host)`; host API makes authenticated backend calls without the session key; local backends only. | beta | https://docs.openhands.dev/openhands/usage/agent-canvas/canvas-extensions.md | reviewed |
| OpenClaw | Plugin hooks (`api.on`) can modify prompts, intercept `before_tool_call` and `message_sending`; internal Gateway hooks in-process with no timeout, retry or durable queue; pluggable context engine quarantined on failure. | GA | https://docs.openclaw.ai/automation/hooks ; https://docs.openclaw.ai/concepts/agent-loop | unreviewed |
| Hermes Agent | Plugins with lifecycle hooks (page says 27, lists 38): observers, directive hooks that block, approve, modify arguments or veto a turn, transforms. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/plugins ; https://hermes-agent.nousresearch.com/docs/user-guide/features/hooks | reviewed |
| Cline | SDK hooks `beforeRun/afterRun/beforeModel/afterModel/beforeTool/afterTool/onEvent`; `beforeTool` can skip, rewrite input, force approval, append context, stop; `afterTool` can replace results. | GA | https://docs.cline.bot/sdk/plugins.md | reviewed |
| rizzy-harness (ours) | none. Closest seam: the `AgentBackend` trait, with only `NativeBackend`. | partial (trait only) | `crates/agent-native/src/lib.rs:537-541` | from own inventory |

### 2.6 Hooks

#### Hook events and what a hook can decide
What it is: user-configured handlers fired at lifecycle events (session start, prompt submit, before and after a tool, stop, compaction) that can block, rewrite, add context or force continuation.
Variants: event counts range from 6 (Antigravity, OpenHands) to 33 (Claude Code). The shared core is PreToolUse/PostToolUse/UserPromptSubmit/Stop/SessionStart. Decision powers differ: rewrite tool input (Codex, Claude Code, Antigravity, Cline, OMP); replace tool output (Claude Code, Cline SDK); force continuation from Stop (Claude Code capped at 8 consecutive, goose cap env var, OMP one continuation only); `defer` a tool call until a later headless resume (Claude Code only). Cursor and goose add file and shell specific events (`beforeReadFile`, `afterFileEdit`, `beforeShellExecution`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | 14 events: SessionStart, SessionEnd, UserPromptSubmit, PreToolUse, PermissionRequest, PostToolUse, PreCompact, PostCompact, PreModelRequest, PostModelRequest, SubagentStart, SubagentStop, Stop, Interrupt. PreToolUse can deny (exit 2 or JSON), add context or rewrite input; 8-hook async cap; 2,500-token default (per review). | on by default; no maturity label | https://learn.chatgpt.com/docs/hooks.md | corrected |
| Claude Code | 33 events (corrected from 31): SessionStart, Setup, UserPromptSubmit, UserPromptExpansion, PreToolUse, PermissionRequest, PermissionDenied, PostToolUse, PostToolUseFailure, PostToolBatch, Notification, MessageDisplay, SubagentStart, SubagentStop, TaskCreated, TaskCompleted, Stop, StopFailure, TeammateIdle, InstructionsLoaded, ConfigChange, CwdChanged, DirectoryAdded, FileChanged, WorktreeCreate, WorktreeRemove, PreCompact, PostCompact, PreModelSwitch, PostModelSwitch, Elicitation, ElicitationResult, SessionEnd. Decisions: PreToolUse allow/deny/ask/defer plus `updatedInput`; PostToolUse `updatedToolOutput`; Stop block with an 8-continuation cap; PermissionRequest can write permanent rules; WorktreeCreate replaces git; `CLAUDE_ENV_FILE` sourced before every Bash command. `additionalContext` capped at 10,000 chars and escaped. | GA (agent hooks experimental) | https://code.claude.com/docs/en/hooks.md ; https://code.claude.com/docs/en/hooks-guide.md | corrected (event count); exit codes, Stop, defer, PermissionDenied reviewed |
| Oh My Pi | input, user_bash, tool_call (block or rewrite), tool_result, context, before_provider_request, before_agent_start, session_before_switch/branch/compact/tree, session_stop (one continuation only), plus notification events; hooks cannot bypass approval policy. | GA | https://omp.sh/docs/hooks | corrected |
| Cursor | sessionStart/End, preToolUse, postToolUse, postToolUseFailure, subagentStart/Stop, before/afterShellExecution, before/afterMCPExecution, beforeReadFile, afterFileEdit, beforeSubmitPrompt, preCompact, stop, afterAgentResponse, afterAgentThought, Tab hooks, workspaceOpen; regex matchers; default `loop_limit` 5; loads Claude Code hooks. | GA | https://cursor.com/docs/hooks | corrected |
| Antigravity | SessionStart, PreToolUse, PostToolUse, PreInvocation, PostInvocation, Stop; can inject steps (system/user/ephemeral/error), overwrite tool args, force-continue or force-stop; `/goal` is built on a Stop hook; exit codes undocumented. | GA | https://antigravity.google/docs/hooks | reviewed |
| OpenHands | PreToolUse, PostToolUse, UserPromptSubmit, Stop, SessionStart, SessionEnd; exit 2 blocks PreToolUse, UserPromptSubmit, Stop; API lists hooks; Canvas shows hook results. Docs do not present hooks as part of the security model. | GA | https://docs.openhands.dev/sdk/guides/hooks.md | reviewed; security framing corrected |
| OpenClaw | Internal Gateway hooks for `/new`, `/reset`, `/stop`, `gateway:startup`, `message:received`; bundled `boot-md`, `command-logger`, `session-memory` and others. | GA | https://docs.openclaw.ai/automation/hooks | unreviewed |
| goose | SessionStart/End, Stop, UserPromptSubmit, PreToolUse, PreToolUseResult (observe), PostToolUse, PostToolUseFailure, BeforeReadFile, AfterFileEdit, BeforeShellExecution, AfterShellExecution; only PreToolUse and Stop block; stop-block cap `GOOSE_STOP_HOOK_BLOCK_CAP`; SubagentStart/Stop listed but not emitted. | unclear (no label) | https://goose-docs.ai/docs/guides/context-engineering/hooks.md | reviewed |
| Hermes Agent | Four systems: gateway hooks (`HOOK.yaml` + `handler.py`, passive), plugin hooks (directive), shell hooks in `config.yaml` (can block a tool call), outbound signed webhooks. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/hooks | unreviewed (page tail not read) |
| Cline | Docs hooks page is a stub pointing to SDK plugins; CLI file hooks (`PreToolUse` script, `--hooks-dir`, `CLINE_HOOKS_DIR`); VS Code needs "Enable Hooks" and `.clinerules/hooks/`; other file-hook events undocumented. | GA | https://docs.cline.bot/customization/hooks.md ; https://docs.cline.bot/cli/cli-reference.md | reviewed |
| DeepSeek harness | No user-facing hooks documented; "Claude Code mods via an experimental bridge" and "hook-bridge records" mentioned in passing. | unclear | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/ | unverified |
| Zed/ACP | Hooks for agent actions: nothing documented. | n/a | https://zed.dev/docs/ai/mcp | unreviewed |
| Warp | Hooks for the Warp Agent: nothing documented. | n/a | https://docs.warp.dev/agents/cli-agents/claude-code/ | unreviewed |
| rizzy-harness (ours) | none. Only git hooks being disabled for checkpoints and inbound routine webhooks. Closest policy lever: `denied_tools` and approval modes. | none | `crates/node/src/git.rs:59` | from own inventory |

#### Hook handler types
What it is: what a hook actually runs: a command, an HTTP call, an MCP tool, a single LLM prompt, or a sub-agent.
Variants: command-only (Antigravity, goose, Cursor in cloud); command plus MCP tool (Codex, which parses `prompt` and `agent` but skips them); all five (Claude Code: command with exec or shell form, http with env-var allowlists, mcp_tool, prompt, agent); code modules (OMP, OpenClaw, Hermes plugins). LLM hooks exist in Claude Code, OpenHands (both fail open) and Cursor (not in cloud).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `command` and `mcp_tool`; `prompt` and `agent` parsed but skipped; matching command hooks run concurrently. | on by default | https://learn.chatgpt.com/docs/hooks.md | corrected |
| Claude Code | `command` (exec form without a shell when `args` given, else `sh -c`), `http` (headers with `$VAR` only from `allowedEnvVars`; admin `allowedHttpHookUrls`), `mcp_tool`, `prompt` (returns `{ok, reason, impossible}`), `agent` (up to 50 turns, experimental). Not every event takes every type. `${user_config.*}` rejected in shell-form commands as an injection fix. | GA (agent experimental) | https://code.claude.com/docs/en/hooks.md | unreviewed |
| Oh My Pi | Trusted TS/JS modules in `.omp/hooks/pre|post/` or `--hook`, not JSON config. | GA | https://omp.sh/docs/hooks | corrected |
| Cursor | Spawned processes, JSON on stdin/stdout; prompt-based hooks (LLM `{ok, reason}`) not in cloud agents; plugins run command hooks only. | GA | https://cursor.com/docs/hooks | corrected |
| Antigravity | `command` only per the hooks page; the plugins page also says "prompt evaluations" (contradiction). | GA | https://antigravity.google/docs/hooks ; https://antigravity.google/docs/plugins | reviewed |
| OpenHands | Command hooks plus `prompt` and `agent` hook types returning allow/deny JSON. | GA | https://docs.openhands.dev/sdk/guides/hooks.md | reviewed |
| OpenClaw | JS/TS handlers in the Gateway process. | GA | https://docs.openclaw.ai/automation/hooks | unreviewed |
| goose | Shell commands with JSON on stdin; follows the Open Plugins hooks spec. | unclear | https://goose-docs.ai/docs/guides/context-engineering/hooks.md | reviewed |
| Hermes Agent | Python handlers (gateway), plugin code, shell commands, outbound webhooks. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/hooks | unreviewed |
| Cline | Scripts reading the payload from stdin (`cline hook`); SDK hook functions. | GA | https://docs.cline.bot/cli/cli-reference.md ; https://docs.cline.bot/sdk/plugins.md | reviewed |
| rizzy-harness (ours) | none. | none | n/a | from own inventory |

#### Hook trust, precedence and failure behaviour
What it is: whether a hook from a repo or plugin runs without review, how hooks from several sources combine, and what happens when a hook crashes or times out.
Variants: trust: Codex is the only one that trusts non-managed hooks by hash (a changed hook needs review again); Claude Code relies on workspace trust (and `-p` treats every folder as trusted); Hermes gateway hooks load with no consent prompt. Precedence: Claude Code merges all sources, runs matches in parallel, deny beats ask beats allow, winner among `updatedInput` rewrites unspecified; Cursor uses deny > ask > allow for permission fields but last response wins for other fields, so a lower source can override a higher one. Failure: fail-open is the common default (Claude Code command hooks, Cursor, goose, OpenHands prompt/agent hooks, Cline SDK `hookErrorMode: ignore`, Claude Code mods); fail-closed defaults exist in OMP `tool_call`, Hermes `pre_tool_call` timeouts, Pi `tool_call` handlers, Claude Code SDK callbacks on PreToolUse timeout, and Claude Code MCP-tool hooks whose answer is unreadable.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Non-managed hooks must be reviewed and are trusted by hash; `--dangerously-bypass-hook-trust` skips; every layer's hooks run (no replacement); managed-only mode. | on by default | https://learn.chatgpt.com/docs/hooks.md | corrected |
| OpenCode | Plugin `permission.evaluate` cannot override configured deny. | GA | https://opencode.ai/docs/build/plugins/server/ | unreviewed |
| Claude Code | Sources merge (identical handler runs once); matches run in parallel and all complete before merging, so a sibling's deny does not stop another hook's side effects. Exit 2 blocks; any other non-zero exit is non-blocking (docs warn policy hooks must use exit 2); timeout discards output so PreToolUse fails open, except PreModelSwitch; `onFailure: block` (v2.1.295+) flips command/http hooks. Repo hooks gated by workspace trust; `-p` runs project hooks untrusted. | GA | https://code.claude.com/docs/en/hooks.md ; https://code.claude.com/docs/en/hooks-guide.md | reviewed (exit codes; `-p` untrusted: review 3) |
| Oh My Pi | `tool_call` fails closed on error or timeout (default 30 s); other hook errors do not block. | GA | https://omp.sh/docs/hooks | corrected |
| Pi | Extension errors are logged and the agent continues, except a failing `tool_call` handler blocks that call. | GA | https://pi.dev/docs/latest/extensions | unreviewed |
| Cursor | Exit 0 uses output, exit 2 blocks, other codes fail open unless `failClosed`; team dashboard hooks cannot set `failClosed` or `matcher`; permission deny > ask > allow regardless of source, other fields last response wins. | GA | https://cursor.com/docs/hooks | corrected |
| Antigravity | Default 30 s timeout; control via stdout JSON; exit codes undocumented. | GA | https://antigravity.google/docs/hooks | reviewed |
| OpenHands | Non-zero non-2 exits are non-blocking; `prompt`/`agent` hooks fail open. | GA | https://docs.openhands.dev/sdk/guides/hooks.md | reviewed |
| OpenClaw | Internal hooks have no timeout, retry or durable queue; trusted code with full Gateway access. | GA | https://docs.openclaw.ai/automation/hooks | unreviewed |
| goose | Hook failure allows by default; `on_failure: block` flips PreToolUse; 30 s default timeout. | unclear | https://goose-docs.ai/docs/guides/context-engineering/hooks.md | reviewed |
| Hermes Agent | Shell hook timeouts fail closed for `pre_tool_call`; gateway hooks load in-process with no sandbox or consent prompt. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/hooks | unreviewed |
| Cline | 3 s per-call timeout; `hookErrorMode` ignore (default) or throw; plugin degraded then failed after 5 consecutive failures. | GA | https://docs.cline.bot/sdk/plugins.md | reviewed |
| rizzy-harness (ours) | none. Our analogous fail-closed path: approvals denied on cancel or restart. | none | `crates/node/src/session.rs:154-191` | from own inventory |

### 2.7 Custom commands and prompt templates

#### Custom slash commands and prompt templates
What it is: user-defined `/name` commands that expand into a prompt, usually Markdown files with argument placeholders.
Variants: the field is converging on "a skill is a command": Codex deprecated custom prompts for skills, Antigravity deprecated workflows for skills, Claude Code merged `.claude/commands` into skills, Hermes, Cline, Zed and Warp expose skills as `/name`. Separate template systems survive in Pi, OMP and OpenCode (with `agent`/`model` frontmatter). goose maps a command to a recipe. Code-registered commands exist in Claude Code mods (answered with no model turn), Pi and OMP extensions, and Hermes plugins. ACP standardises commands as advertised plain text, with no separate API.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `~/.codex/prompts/*.md` as `/prompts:<name>` with `$1`-`$9`, `$ARGUMENTS`, named placeholders: deprecated in favour of skills. Ten numbered presets hold reusable prompts or commands. | deprecated (custom prompts); GA (presets) | https://learn.chatgpt.com/docs/custom-prompts.md ; https://learn.chatgpt.com/docs/developer-commands.md | unreviewed (preset 8,000-byte limit: unverified per review) |
| OpenCode | Markdown or JSON; `$ARGUMENTS`, `$1..`; `agent`, `model`, `subagent` frontmatter; nested folders namespace; can replace built-ins; model precedence command > agent > session. | GA | https://opencode.ai/docs/commands/ | reviewed |
| Claude Code | User commands are skills; legacy `.claude/commands/*.md` still work; arguments `$ARGUMENTS`, `$N`, named; MCP prompts as commands; mods register commands answered without a model turn (`immediate: true` while Claude works). | GA | https://code.claude.com/docs/en/commands.md ; https://code.claude.com/docs/en/skills.md ; https://code.claude.com/docs/en/plugins/mods/api.md | reviewed (skills) |
| Oh My Pi | Prompt templates in `.omp/prompts/` with `$1`, `$@`, Handlebars helpers; MCP prompts; file commands; extensions `registerCommand`. | GA | https://omp.sh/docs/prompt-templates ; https://omp.sh/docs/slash | unreviewed |
| Pi | `prompts/*.md` as `/name`, `description`, `argument-hint`, `$1`, `$@`, `${1:-default}`, `${@:N:L}`; project templates need trust. | GA | https://pi.dev/docs/latest/prompt-templates | unreviewed |
| Cursor | Skills as `/name`; a `.cursor/commands` format was not documented on pages opened. | unclear | https://cursor.com/docs/plugins ; https://cursor.com/docs/skills | unreviewed |
| Antigravity | No separate feature: skills become slash commands; Gemini CLI custom commands convert to skills on import; workflows deprecated (`/migrate-workflows`). | GA (via skills) | https://antigravity.google/docs/slash-commands ; https://antigravity.google/docs/ide/workflows | unreviewed |
| OpenHands | Plugins carry `commands/*.md`. | GA | https://docs.openhands.dev/sdk/guides/plugins.md | unreviewed |
| OpenClaw | Skills support `user-invocable` and `command-dispatch`; slash commands authorised via `commands.allowFrom`. | GA | https://docs.openclaw.ai/tools/skills | unreviewed |
| goose | A command maps to a recipe (one parameter, unique, cannot clash with built-ins). | GA | https://goose-docs.ai/docs/guides/context-engineering/slash-commands.md | unreviewed |
| Hermes Agent | Every skill is a slash command; plugins register slash commands. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/skills | unreviewed |
| Zed/ACP | Skills via `/`; ACP agents advertise commands with `available_commands_update` and the client sends them as prompt text. | GA | https://zed.dev/docs/ai/skills.md ; https://agentclientprotocol.com/protocol/v1/slash-commands.md | unreviewed |
| Warp | Built-in menu plus saved Warp Drive prompts and skills; asterisked commands use metered inference. | GA | https://docs.warp.dev/agents/capabilities/slash-commands/ | unreviewed |
| Cline | Built-ins `/newtask`, `/smol`, `/newrule`, `/deep-planning`, `/reportbug`; enabled skills as `/skill-name`; workflows dirs exist but authoring undocumented. | GA | https://docs.cline.bot/core-workflows/using-commands.md | unreviewed |
| rizzy-harness (ours) | none. Composer has `@agent` mentions only; `/` opens search. Closest reusable prompt: a saved routine or a custom agent. | none | `web/src/lib/mentions.ts` | from own inventory |

### 2.8 Custom tools

#### Custom tools defined in code
What it is: tools defined by users or integrators as code (not via a separate MCP process) and offered to the model next to built-ins.
Variants: two shapes. In-process MCP: Claude Code's SDK `tool()`/`createSdkMcpServer` (names `mcp__<server>__<tool>`; annotations are hints, not enforcement). Native registration: OpenCode `ctx.tool.transform`, OMP `.omp/tools/<name>/index.ts` with a declared approval class (default exec), Pi `registerTool`, Hermes and OpenClaw plugins, Cline `AgentPlugin`, SDKs of Cursor, Antigravity and OpenHands (which also documents custom tools against a remote agent server). Claude Code alone documents aliasing a built-in to a host implementation (`toolAliases: { Bash: ... }`).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | V2 custom tools via plugins (`ctx.tool.transform`: JSON Schema input, progress callback, abort signal); each model request captures a stable tool snapshot. | GA | https://opencode.ai/docs/build/plugins/server/ | unreviewed |
| Claude Code | SDK `tool(name, description, zodShape, handler)` / Python `@tool` grouped into an in-process MCP server; uncaught exceptions become error results; `permission_policy` `always_ask`/`always_deny`; `toolAliases` routes a built-in to a host tool; mods `$.tool.register` (named `mcp__<plugin>__<name>`). | GA | https://code.claude.com/docs/en/agent-sdk/custom-tools.md ; https://code.claude.com/docs/en/agent-sdk/typescript.md ; https://code.claude.com/docs/en/plugins/mods/api.md | unreviewed |
| DeepSeek harness | Plugins add tools; cookbook covers a new tool. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/ | reviewed |
| Oh My Pi | TS modules at `.omp/tools/<name>/index.ts` (also scans `.claude/tools`, `.codex/tools`); zod/arktype/typebox; declared approval class, default exec. | GA | https://omp.sh/docs/custom-tools | unreviewed |
| Pi | Extensions register tools (TypeBox); tool overrides by name. | GA | https://pi.dev/docs/latest/extensions | unreviewed |
| Cursor | SDK custom tools. | unclear (no label) | https://cursor.com/docs/sdk/typescript | unreviewed |
| Antigravity | Python SDK custom tools. | unclear | https://antigravity.google/docs/sdk/overview | unreviewed |
| OpenHands | SDK `definition.py`/`impl.py` pattern; a guide for custom tools with a remote Agent Server. | GA | https://docs.openhands.dev/sdk/arch/tool-system.md | unreviewed |
| OpenClaw | Plugin-provided tools (diffs renderer, `show_widget`, LLM Task, Lobster, Tokenjuice, Canvas). | GA/unclear per plugin | https://docs.openclaw.ai/tools | unreviewed |
| Hermes Agent | General plugins register tools. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/plugins | reviewed |
| Cline | SDK plugins add tools; SDK lists custom tools. | GA | https://docs.cline.bot/sdk/plugins.md ; https://docs.cline.bot/sdk/overview.md | reviewed |
| goose | Tools only via MCP extensions; GDK for building a whole agent. | GA | https://goose-docs.ai/docs/getting-started/using-extensions.md | unreviewed |
| rizzy-harness (ours) | none. Tool set compiled in. | none | `crates/tools/src/lib.rs:385-396` | from own inventory |

### 2.9 Code mode

#### Code mode (the model writes code that calls tools)
What it is: instead of one tool call per turn, the model writes a script that calls tools, and only the script's output returns to context. Cuts schema cost and round trips.
Variants: runtime and isolation: Codex embedded V8 with an optional remote Code Mode host over gRPC; Pi QuickJS (256 MB, two workers, no fs/network/timers); goose Boa (no fs/shell/network/env); OpenCode JavaScript with only `fetch` (no fs, imports, timers); Hermes a persistent Python kernel over a Unix socket (300 s, 50 calls, 50 KB stdout, no MCP, no delegation). Role for MCP: OpenCode and Pi route MCP tools through code mode by default; Codex per server; Hermes excludes MCP.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Model drives tools through scripts in an embedded V8 runtime; tools routed per MCP server to `direct`, `deferred` or `code_mode`; app-server can share a remote Code Mode host over gRPC. | unclear ("under development and off by default" vs shipping features) | https://learn.chatgpt.com/docs/config-file/config-reference.md ; https://learn.chatgpt.com/docs/app-server.md ; https://learn.chatgpt.com/docs/changelog | reviewed |
| OpenCode | `execute`: JavaScript composing catalog and MCP tools (including in parallel); no fs, imports or timers, only `fetch`; MCP tools reached through it by default; denying `execute` removes tools only reachable through it; nested tools keep their own permissions. | GA | https://opencode.ai/docs/tools/ ; https://opencode.ai/docs/mcp-servers/ | reviewed |
| Oh My Pi | `eval` Python/JS persistent kernels; README says cells can call back into agent tools over a loopback bridge; sandboxing undocumented. | GA (tool); tool bridge is a README claim | https://omp.sh/docs/tools | unverified (tool bridge) |
| Pi | `codemode`: JavaScript calls `tools.<name>(args)` and non-LLM models; QuickJS, 256 MB per VM, two workers, no Node APIs/fs/network/timers; `searchTools()` BM25 limit 8; default MCP exposure. Docs disagree whether it is on by default. | GA (no status stated) | https://pi.dev/docs/latest/codemode ; https://pi.dev/changelog | corrected (rollback and output-cap details unverified) |
| goose | Code Mode platform extension: `list_functions`, `get_function_details`, `execute_javascript` in a fresh Boa context with `tools.call()`; text-only results; recommended for 5+ extensions; replaced an earlier "PCTX" mode. | unclear ("if included in your build") | https://goose-docs.ai/docs/guides/managing-tools/code-mode.md | reviewed |
| Hermes Agent | `execute_code`: Python script imports a generated `hermes_tools` stub; calls go over a Unix socket; only `print()` returns; web, file and foreground terminal tools allowed; no recursion, delegation or MCP; 300 s, 50 calls, 50 KB inline; persistent kernel (1800 s idle, max 4); child env strips KEY/TOKEN/SECRET names. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/code-execution | reviewed |
| rizzy-harness (ours) | none. | none | n/a | from own inventory |

### 2.10 SDKs and embedding

#### Embeddable SDK (agent as a library)
What it is: a library that runs the harness's agent loop inside another program.
Variants: process model is the main difference. Claude Code's SDK spawns the bundled CLI as a child (NDJSON over stdio; hosts can swap the transport); Codex's Python SDK drives the local app-server over JSON-RPC; OpenCode's `@opencode/sdk` embeds the full server in-process with no HTTP listener; Pi's SDK is in-process; DSH's Python SDK lazily starts `dsh --profile sdk-minimal` and pins `danger-full-access`; Cursor's SDK runs the loop locally with inference at Cursor or fully in the cloud; Hermes has an in-process Python class but no published wheel. Languages: TypeScript and Python dominate; OMP is Bun-only; goose's GDK is Rust crates.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `@openai/codex-sdk` (Node 18+): `startThread()`, `run()`, `resumeThread()`; Python `openai-codex` (3.10+) drives the app-server, `ExternalMessage` passes third-party content at tool-output priority. | GA | https://learn.chatgpt.com/docs/codex-sdk.md | reviewed |
| OpenCode | `@opencode/sdk` embeds the server in-process; `@opencode/client` network client (no replay or auto-reconnect); `@opencode/plugin`; Effect variants; Cloudflare guide. | GA | https://opencode.ai/docs/build/sdk/ ; https://opencode.ai/docs/build/client/ | unreviewed |
| Claude Code | Agent SDK (TS, Python) bundles the native binary and spawns it per session; hooks as in-process callbacks (fail-open by default, PreToolUse timeout blocks); in-process MCP tools; skills filesystem-only; local plugins only; renamed from Claude Code SDK (v0.1.0 no longer uses the Claude Code system prompt by default). | GA | https://code.claude.com/docs/en/agent-sdk/overview.md ; https://code.claude.com/docs/en/agent-sdk/hooks.md ; https://code.claude.com/docs/en/agent-sdk/migration-guide.md | unreviewed |
| Claude Desktop | Scripting and the Agent SDK are CLI-only; Desktop has no headless mode. | GA (statement) | https://code.claude.com/docs/en/platforms | unreviewed |
| DeepSeek harness | `deepseek-harness-sdk` (Python 3.10+) bundles the runtime; `harness.run(prompt, session_id=...)`; minimal profile has a persistent shell, no file/web/subagent tools, pins `danger-full-access`; session-log upload on by default unless disabled. | preview | https://deepseek-harness.github.io/deepseek-harness/en/guide/python-sdk | reviewed |
| Oh My Pi | Bun 1.3.14+ only (`createAgentSession`, events, custom tools); not Node. | GA | https://omp.sh/docs/sdk | unreviewed |
| Pi | `@earendil-works/pi-coding-agent` in Node or Bun: `createAgentSession()`, `prompt()`, `steer()`, `followUp()`, `subscribe()`; lower layers `pi-ai` and `pi-agent-core` (`beforeToolCall`/`afterToolCall`); codemode, tool search and MCP not loaded unless added. | GA | https://pi.dev/docs/latest/sdk ; https://github.com/earendil-works/pi/blob/main/packages/agent/README.md | unreviewed |
| Cursor | `@cursor/sdk` (Node 22.13+) and Python `cursor-sdk`; `local` runtime (loop in your process, inference at Cursor) or `cloud`; custom tools, MCP, hooks, local sandbox. Separate Agent SDK for agents-as-code folders (announced 2026-10-20). | unclear (no label) | https://cursor.com/docs/sdk/typescript ; https://cursor.com/docs/agent-sdk | unreviewed |
| Antigravity | Python `google.antigravity`: `Agent`, custom tools, MCP, hooks (`pre_turn`, `on_tool_error`), triggers, policies, subagents, structured output; Connection abstraction suggests a pluggable transport; needs the compiled binary. | unclear | https://antigravity.google/docs/sdk/overview ; https://github.com/google-antigravity/antigravity-sdk-python | unreviewed |
| OpenHands | Python Software Agent SDK (`openhands.sdk`, `.tools`, `.workspace`, `.agent_server`); CLI uses it directly. | GA | https://docs.openhands.dev/sdk/arch/overview.md | unreviewed |
| OpenClaw | "Embedding OpenClaw" and "Building a Gateway client" pages exist but were not opened. | unclear | https://docs.openclaw.ai/gateway/protocol | unverified |
| goose | GDK Rust crates (`goose-agent`, `goose-sdk`, providers, context management) with Python and Kotlin bindings for provider calls; app owns state. | unclear | https://goose-docs.ai/docs/gdk/index.md | unreviewed |
| Hermes Agent | `from run_agent import AIAgent`; one agent per thread; no supported wheel or sdist. | GA, source checkout only | https://hermes-agent.nousresearch.com/docs/guides/python-library | unreviewed |
| Warp | Python and TypeScript Platform SDKs over the REST Platform API (cloud runs). | GA | https://docs.warp.dev/platform/transitioning-from-oz/ | unreviewed |
| Cline | `@cline/sdk` (TypeScript, Node 22+): `Agent`/`ClineCore`, events, cost in results, custom tools, cron, subagents, MCP, checkpoints. | GA (0.x) | https://docs.cline.bot/sdk/overview.md | unreviewed |
| rizzy-harness (ours) | none. No SDK and no OpenAPI spec. | none | `crates/master/src/http.rs:26-175` | from own inventory |

#### Programmatic control protocol (headless JSON streams, RPC, app-servers)
What it is: a wire protocol a separate process uses to drive the agent: start turns, stream events, answer approvals.
Variants: stdio JSONL (Pi RPC and `--mode json`, OMP RPC v1/v2, DSH `sdk` profile, Claude Code `-p --output-format stream-json`, Codex app-server default), WebSocket (Codex app-server experimental; OpenClaw Gateway with idempotency keys and TypeBox schemas; Cline hub), HTTP with OpenAPI (OpenCode 170 operations, OpenHands Agent Server, Warp Platform API), loopback Connect/protobuf (Cursor SDK Bridge). Maturity contradictions: Codex calls the app-server experimental and unsupported for production while its stable Python SDK depends on it; OpenCode calls its HTTP API experimental while its SDK presents it as the contract. Auth gaps: Codex non-loopback WebSocket listeners accept unauthenticated connections by default "during rollout"; Cline hub auth undocumented; OpenHands Agent Server has no auth by default beyond an API key.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | App-server: JSON-RPC 2.0 without the `jsonrpc` field over stdio JSONL, WebSocket (experimental) or Unix socket; one `initialize` with capability negotiation (`experimentalApi`); server-initiated approvals and elicitations; generated TS and JSON schemas; bounded queues answer `-32001`. WebSocket auth `capability-token` or `signed-bearer-token`, Origin requests get 403, non-loopback unauthenticated by default during rollout. | experimental | https://learn.chatgpt.com/docs/app-server.md ; https://learn.chatgpt.com/docs/mcp-server.md | reviewed |
| OpenCode | HTTP API with OpenAPI 3.1 at `/docs/openapi.json`, 170 operations; volatile SSE event stream plus a durable per-session log; `opencode api` CLI. | experimental | https://opencode.ai/docs/api/ ; https://opencode.ai/docs/cli/commands/ | reviewed |
| Claude Code | `-p` with `json`/`stream-json` output; SDK speaks NDJSON with `control_request`/`control_response` envelopes (full control protocol not specified publicly); `--bare` to become the default for `-p`. | GA | https://code.claude.com/docs/en/agent-sdk/hosting.md ; https://code.claude.com/docs/en/agent-sdk/overview.md | reviewed (headless basics, review 3); control protocol unverified |
| DeepSeek harness | `dsh headless`, `dsh sdk`/`sdk-minimal` JSON-RPC over stdio, `dsh acp`, all profiles of one launcher. | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md | unreviewed |
| Oh My Pi | `omp --mode rpc` NDJSON with ready frame; protocol v1 (1 MiB lines) and v2 (chunked up to 64 MiB); prompt/steer/follow_up/abort, sessions, branch, handoff, compact, host tools, extension UI requests. | GA | https://omp.sh/docs/rpc | unreviewed |
| Pi | `pi --mode rpc` long-lived JSONL; commands with optional ids, out-of-order responses; extension UI via `extension_ui_request`; `pi --mode json` event stream; orderly shutdown on stdin close. | GA | https://pi.dev/docs/latest/rpc ; https://pi.dev/docs/latest/json | unreviewed |
| Cursor | SDK Bridge: loopback Connect/protobuf (`sdk.v1`) with per-process bearer token for other languages; Cloud Agents REST v1 with SSE and `Last-Event-ID`. | unclear; Cloud API public beta | https://cursor.com/docs/sdk/bridge ; https://cursor.com/docs/cloud-agent/api/endpoints | unreviewed (API partly reviewed) |
| OpenHands | Agent Server REST and WebSocket with OpenAPI at `/docs`; TypeScript client. | GA | https://docs.openhands.dev/sdk/arch/agent-server.md | reviewed |
| OpenClaw | Gateway: JSON over WebSocket, first frame `connect`, request/response/event frames, idempotency keys for side effects, events not replayed, TypeBox schemas with generated JSON Schema and Swift models, N-1 node version window. | GA | https://docs.openclaw.ai/gateway/protocol ; https://docs.openclaw.ai/concepts/architecture | unreviewed |
| goose | ACP is goose's own client protocol: Desktop talks to its backend over ACP (`goose serve`, HTTP `/acp` plus WebSocket, port 3284); goose-specific methods in a generated reference. | GA | https://goose-docs.ai/docs/gdk/acp/index.md | reviewed |
| Hermes Agent | ACP, TUI JSON-RPC and an HTTP API server (`/v1/runs` with SSE, stop, approval, clarify; bearer key required; 127.0.0.1:8642; max 10 runs). | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/api-server | unreviewed |
| Warp | REST Platform API with OpenAPI spec; create, monitor, follow up on, cancel runs. | GA | https://docs.warp.dev/platform/transitioning-from-oz/ | unreviewed |
| Cline | Local hub daemon (WebSocket, loopback by default) owns spokes; UIs attach; hub auth undocumented. | GA | https://docs.cline.bot/sdk/overview.md | reviewed (hub entry) |
| rizzy-harness (ours) | REST plus `/api/ws`, JSON; bearer API token is admin-equivalent; no OpenAPI or SDK; no headless one-shot CLI. The master-node protocol is versioned (`PROTOCOL_VERSION=1`) but internal. | partial | `crates/master/src/http.rs:26-175`, `crates/master/src/auth.rs:528-580`, `crates/protocol/src/lib.rs:74-260` | from own inventory |

#### OpenAI-compatible endpoint where the "model" is an agent
What it is: an HTTP endpoint shaped like OpenAI's API that runs a full agent per request, so any OpenAI client can call the harness.
Variants: OpenHands maps profiles to model names and returns final text (no `previous_response_id`, `store`, `stream` on Responses); OpenClaw maps agents to `openclaw/<agentId>` and documents it as full operator access; Hermes exposes stateless chat completions, stateful Responses and a runs API with the full toolset including terminal.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenHands | `/v1/chat/completions`, `/v1/responses`, `/v1/models`; profiles as `openhands_<profile>`. | GA | https://docs.openhands.dev/sdk/guides/agent-server/openai-gateway.md | reviewed |
| OpenClaw | `/v1/chat/completions`, `/v1/models`, `/v1/embeddings` (off by default), `/v1/responses`; `model` selects an agent; session via `user` or `x-openclaw-session-key`. | GA | https://docs.openclaw.ai/gateway/openai-http-api | unreviewed |
| Hermes Agent | `/v1/chat/completions` (stateless), `/v1/responses` (server state), `/v1/runs`; bearer key always required; CORS off. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/api-server | unreviewed |
| rizzy-harness (ours) | none. | none | n/a | from own inventory |

#### Structured output for programmatic callers
What it is: the caller supplies a JSON Schema and the harness guarantees (or retries towards) a final answer that validates.
Variants: Claude Code adds a synthetic `StructuredOutput` tool and retries on mismatch (draft-07 only); goose recipes validate the final answer with `response.json_schema`; Antigravity's SDK lists structured output.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `outputFormat: {type: "json_schema", schema}`; mismatches return an error tool result and the agent retries; failure `error_max_structured_output_retries`; draft-07 schemas only. | GA | https://code.claude.com/docs/en/agent-sdk/structured-outputs.md | unreviewed |
| Antigravity | SDK structured output. | unclear | https://antigravity.google/docs/sdk/overview | unreviewed |
| goose | Recipe `response.json_schema` validates the final output; recipe `retry` with shell success checks. | GA | https://goose-docs.ai/docs/guides/recipes/recipe-reference.md | reviewed |
| rizzy-harness (ours) | none for API callers. `ui:` blocks are model-to-UI structure, validated in Rust. | none | `crates/core/src/ui/mod.rs:56-83` | from own inventory |

### 2.11 Client protocols

#### ACP: the harness as an ACP agent
What it is: the Agent Client Protocol (JSON-RPC 2.0, modelled on LSP) lets an editor launch an agent as a subprocess over stdio. The agent runs the loop; the client does file and terminal I/O and shows permission prompts. v1 is stable; v2 is a draft (2026-07-20); stdio is the only standardised transport; remote Streamable HTTP/WebSocket is an RFD.
Variants: what crosses the bridge differs: OMP and Cline let the editor own files, terminals and permissions; OMP, Hermes and Zed-forwarded setups take MCP servers from the client, while OpenCode does not support MCP over ACP and the OpenClaw bridge rejects per-session MCP; Cline defaults every edit to the client's permission UI but `--auto-approve true` pre-approves; goose also serves ACP over HTTP and WebSocket (ahead of the RFD). Claude Code, Codex and Pi are reached only through third-party adapters per the ACP agents page.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCode | `opencode acp`: newline-delimited JSON-RPC over stdio (ACP v1), private server, one prompt per session at a time, MCP over ACP unsupported. | GA | https://opencode.ai/docs/cli/acp/ | unreviewed |
| DeepSeek harness | `dsh acp` serves ACP clients over stdio. | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md | unreviewed |
| Oh My Pi | `omp acp`: editor-owned files, terminals and permissions; session list/load/fork; Plan mode via client; client supplies MCP. | GA | https://omp.sh/docs/acp | unreviewed |
| Cursor | `agent acp`: JSON-RPC 2.0 over stdio, auth method `cursor_login`, modes agent/plan/ask; used by JetBrains, Neovim, Zed. | unclear (no label) | https://cursor.com/docs/cli/acp | unreviewed |
| Antigravity | `agy acp` over stdio for Zed, JetBrains, Android Studio, Xcode; uses CLI auth, tools and settings. | GA | https://antigravity.google/docs/cli/acp | unreviewed |
| OpenClaw | `openclaw acp` bridges IDEs (Zed documented) to a Gateway session; lacks per-session MCP servers and client fs/terminal methods; usage approximate. | GA/partial | https://docs.openclaw.ai/cli/acp | unreviewed |
| goose | `goose acp` (stdio) and `goose serve` (HTTP `/acp` plus WebSocket, port 3284; token query param for browsers); initialize, session with absolute cwd, streamed updates, permission requests, `session/load`, `session/cancel`; permission callback "is not a sandbox"; listed in Zed's registry. | GA | https://goose-docs.ai/docs/gdk/acp/index.md ; https://goose-docs.ai/docs/guides/zed.md | corrected |
| Hermes Agent | `hermes acp` for VS Code, Zed, JetBrains, Buzz Desktop; approvals once/session/always/deny with 300 s timeout denying; curated toolset; sessions listed, resumed, forked. Docs warn Buzz Desktop auto-approves allow-once. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/features/acp | unreviewed |
| Zed/ACP | Protocol: client-called `initialize`, `session/new` (absolute cwd plus `mcpServers`), `session/prompt`, optional load/resume/list/close/delete; agent-called `session/request_permission`, `fs/*`, `terminal/*`, `elicitation/create`. Permission option kinds are UI hints; the agent stores "always" decisions; requests are advisory and the client must apply its own checks. 51 agents and 142 clients listed; registry requires an auth method. | v1 GA; v2 draft; remote transport planned | https://agentclientprotocol.com/protocol/v1/overview.md ; https://agentclientprotocol.com/protocol/v1/tool-calls.md ; https://agentclientprotocol.com/protocol/v1/security.md ; https://agentclientprotocol.com/get-started/agents.md | reviewed (clients count corrected) |
| Cline | `cline --acp` over stdio for Zed, JetBrains, Neovim, Emacs; sign-in, Plan/Act, model picking, session resume; default every edit and command through the client permission UI, `--auto-approve true` pre-approves. | GA | https://docs.cline.bot/usage/acp.md | reviewed (auto-approve false in ACP) |
| Warp | Changelog mentions "ACP agents" and "ACP-driven parent runs" (2026.10.22); no ACP page read. | unclear | https://docs.warp.dev/changelog/2026/ | unverified |
| rizzy-harness (ours) | none. ACP appears only as a deferred idea in PLAN. | docs-only | `PLAN.md:143-146` | from own inventory |

#### ACP: the harness as an ACP client hosting other agents
What it is: the harness launches another vendor's agent over ACP and renders its thread, so one UI can drive Claude, Codex, Gemini and others.
Variants: Zed renders external agents but its profiles and skills do not apply to them; OpenHands runs the external CLI inside its Agent Server and resolves credentials by secret name at spawn (approval handling undocumented); goose uses ACP agents as "providers" and maps its permission modes onto theirs (resume and fork unsupported); OpenClaw spawns harnesses via acpx; DSH lets subagents be ACP agents.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| DeepSeek harness | Subagents can be external harnesses (Codex, Claude Code, ACP, DSH SDK); follow-ups rejected. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/ | reviewed |
| OpenHands | Canvas can run Claude Code, Codex, Gemini CLI, Pi, OpenCode or any stdio ACP server as the agent; Pi and OpenCode local only; credentials reuse host login or env-named secrets resolved at spawn; approvals and lost features undocumented. | GA | https://docs.openhands.dev/openhands/usage/agent-canvas/acp-agents.md | reviewed |
| OpenClaw | `/acp spawn` launches Codex, Claude Code, Gemini CLI, OpenCode, Cursor, Droid via acpx; once a harness starts, failures are not retried on another runtime. | GA | https://docs.openclaw.ai/cli/acp ; https://docs.openclaw.ai/concepts/agent-runtimes | unreviewed |
| goose | ACP providers `amp-acp`, `claude-acp`, `codex-acp`, `pi-acp` reuse a subscription; goose `auto` skips Claude permission checks, `approve` maps to read-only for Codex; extensions passed as MCP servers; resume/fork unsupported. Experimental Roaming carries ACP peer-to-peer over iroh/QUIC with key-card trust (not in released binaries). | GA (providers); experimental (roaming) | https://goose-docs.ai/docs/guides/acp-providers.md ; https://goose-docs.ai/docs/guides/roaming-agents.md | reviewed |
| Zed/ACP | External Agents run as separate processes; install from the ACP registry or `agent_servers`; agent owns auth, model, tools, billing; Zed MCP servers forwarded; tool permissions involve both sides; steer unavailable for external agents. | GA | https://zed.dev/docs/ai/external-agents.md | unreviewed |
| rizzy-harness (ours) | none. The `AgentBackend` trait is meant to drive external harnesses; only the native backend exists. | partial (trait only) | `crates/agent-native/src/lib.rs:537-541` | from own inventory |

#### Hosting other harnesses through their own interfaces (not ACP)
What it is: running another vendor's agent as a backend through its own app-server, CLI or SDK, or wrapping it in a terminal.
Variants: OpenClaw runs OpenAI models through Codex's app-server (Codex owns the loop, OpenClaw mirrors the transcript and bridges tools) and routes model refs through local CLIs; Warp hosts third-party CLI agents in its terminal and ships notification plugins for them; Cline's Kanban runs Claude Code, Codex and OpenCode per card in worktrees; Zed Terminal Threads embed any CLI agent with no protocol; goose configures Codex as an MCP-server subagent; OpenCodex is a gateway that lets closed clients use other providers.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | Gateway between 28 clients (Codex, Claude Code and others) and many providers; rewrites Codex's model catalog to steer its sub-agent surface. | GA | https://opencodex.me/reference/adapters/ | reviewed |
| OpenClaw | Runtimes `openclaw`, bundled `codex` (via Codex app-server), opt-in `copilot` and `muse`, CLI backends such as `claude-cli`. | GA | https://docs.openclaw.ai/concepts/agent-runtimes | unreviewed |
| goose | External subagents, e.g. Codex run as an MCP server. | unclear | https://goose-docs.ai/docs/guides/context-engineering/subagents.md | reviewed |
| Hermes Agent | Optional Codex app-server runtime (page title only). | unclear | https://hermes-agent.nousresearch.com/docs/llms.txt | unverified |
| Zed/ACP | Terminal Threads embed any CLI/TUI agent as a thread without protocol support. | GA | https://zed.dev/docs/ai/agents.md | reviewed |
| Warp | Hosts Claude Code, Codex, OpenCode, Amp, Gemini, Kiro in the terminal with input editor and review-comment injection; plugins make them notify Warp. | GA | https://docs.warp.dev/agents/cli-agents/claude-code/ | unreviewed |
| Cline | Kanban cards run Cline CLI, Claude Code, Codex or OpenCode in their own worktree and terminal. | research preview | https://docs.cline.bot/usage/kanban.md | unreviewed |
| rizzy-harness (ours) | none (trait only, as above). | partial | `crates/agent-native/src/lib.rs:537-541` | from own inventory |

#### Agent-to-Agent (A2A) protocol
What it is: Linux Foundation protocol for agent-to-agent delegation (agent cards, JSON-RPC, push notifications); distinct from ACP.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Hermes Agent | Plugin supporting A2A v1.0 both ways: agent card at `/.well-known/agent-card.json`, JSON-RPC on port 9900, SSE, HMAC-signed push; outbound `a2a_*` toolset off by default; localhost-only without a token; per-peer tokens, 60/min, audit log. | GA | https://hermes-agent.nousresearch.com/docs/user-guide/messaging/a2a | unreviewed |
| Zed/ACP | ACP docs describe A2A as complementary (agent-to-agent) and IBM BeeAI's "Agent Communication Protocol" as unrelated and archived. | n/a | https://agentclientprotocol.com/get-started/related-protocols.md | reviewed |
| rizzy-harness (ours) | none. | none | n/a | from own inventory |

### 2.12 Reading other harnesses' configuration

#### Reading other harnesses' files in place
What it is: the harness reads instruction files, skills, MCP configs, hooks or plugin layouts that belong to another harness, without converting them.
Variants: scope ranges from one shared path (`.agents/skills`) to deep compatibility: OMP reads eight instruction formats, other apps' MCP configs, `.claude/tools` and Claude marketplaces; Cursor loads Claude Code hooks and `.claude/` and `.codex/` agents and skills; Warp scans ten tools' skills dirs; OpenClaw maps Codex, Claude, Cursor and Agent Plugin bundle layouts; goose accepts Gemini extension manifests; Codex sets `CLAUDE_PLUGIN_ROOT` for plugin hooks. The cost is trust: content written for one harness's approval model runs under another's.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Plugin hooks get `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA` for compatibility. | GA | https://learn.chatgpt.com/docs/hooks.md | reviewed (borrowed-feature check) |
| OpenCode | Skills from `.claude/skills` and `.agents/skills`; V2 reads only `AGENTS.md` for instructions. | GA | https://opencode.ai/docs/skills/ ; https://opencode.ai/docs/instructions/ | reviewed |
| Claude Code | AGENTS.md via the built-in `cc-plugin-agents-md` mod (default: only where no CLAUDE.md exists); `/init` reads Cursor and Copilot rules; `claude mcp add-from-claude-desktop`. | GA | https://code.claude.com/docs/en/memory.md ; https://code.claude.com/docs/en/mcp.md | reviewed (AGENTS.md) |
| DeepSeek harness | Reads `.agents/skills` and `<agentsHome>/skills`. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/skills | unreviewed |
| Oh My Pi | Instruction providers for Claude, `.agent(s)`, Codex, Gemini, OpenCode, Copilot, Cursor/Cline rules; skills from five other tools' dirs; MCP auto-import from eight apps; custom tools from `.claude/tools`, `.codex/tools`; LSP config from Claude, Codex, Gemini dirs; Claude marketplace fallback. | GA | https://omp.sh/docs/context-files ; https://omp.sh/docs/mcp ; https://omp.sh/docs/marketplace | corrected (instruction priority) |
| Pi | `AGENTS.md`, `CLAUDE.md` and variants from agent, working and parent dirs (loaded without project trust); `.agents/skills`. | GA | https://pi.dev/docs/latest/configuration ; https://pi.dev/docs/latest/skills | unreviewed |
| Cursor | Skills from `.claude/skills/` and `.codex/skills/`; loads Claude Code hooks (`CLAUDE_PROJECT_DIR` aliases `CURSOR_PROJECT_DIR`); loads `.claude/` and `.codex/` agents. | GA | https://cursor.com/docs/skills ; https://cursor.com/docs/hooks | corrected (hooks entry) |
| OpenHands | `CLAUDE.md` and `GEMINI.md` recognised as model-specific repo context; Claude-Code-shaped plugins; `.agents/skills`. | GA | https://docs.openhands.dev/overview/skills.md ; https://docs.openhands.dev/sdk/guides/plugins.md | unreviewed |
| OpenClaw | Bundle compatibility maps Codex, Claude, Cursor and Agent Plugin layouts; `.agents/skills`. | GA | https://docs.openclaw.ai/tools/plugin | unreviewed |
| goose | Skills from `.claude/skills/`; agents from `.claude/agents/`; Gemini extension manifests for plugins; `CONTEXT_FILE_NAMES` can read `CLAUDE.md` or `.cursorrules`. | GA | https://goose-docs.ai/docs/guides/context-engineering/using-skills.md ; https://goose-docs.ai/docs/guides/context-engineering/using-goosehints.md | unreviewed |
| Zed/ACP | First match among nine files (`.rules`, `.cursorrules`, `.windsurfrules`, `.clinerules`, `.github/copilot-instructions.md`, `AGENT.md`, `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`); `.agents/skills`. | GA | https://zed.dev/docs/ai/skills.md | corrected (first-match rule) |
| Warp | Skills from ten tools' dirs; `AGENTS.md` default rules file; `/init` can link `CLAUDE.md`, `.cursorrules`, `GEMINI.md` and others. | GA | https://docs.warp.dev/agents/capabilities/skills/ ; https://docs.warp.dev/agents/capabilities/rules/ | reviewed (skills) |
| Cline | Skills from `.claude/skills/`; auto-detects `.cursorrules`, `.windsurfrules`, `AGENTS.md`. | GA | https://docs.cline.bot/customization/skills.md ; https://docs.cline.bot/customization/cline-rules.md | reviewed (skills) |
| rizzy-harness (ours) | none. The agent reads no `AGENTS.md`/`CLAUDE.md` or repo instruction file; per-project instructions only as UI text. | none | `crates/master/src/projects.rs:1-12` (grep finds no reader) | from own inventory |

#### One-shot import from another harness
What it is: a command that copies another harness's setup (and sometimes chats) into this one once.
Variants: Codex imports config, skills, AGENTS.md, plugins, MCP, projects, hooks, commands, subagents and up to 50 recent chats (CLI cap); Claude Code imports from Codex, Gemini CLI and Cursor; OMP imports Claude and Codex history; Antigravity imports Claude Code plugins and Gemini CLI extensions.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `/import` (CLI) or Settings > Import (Desktop) from Claude Code or Cursor (Desktop also Claude Cowork); config, skills, AGENTS.md, plugins, MCP, projects, hooks, slash commands, subagents, up to 50 chats from 30 days (CLI only); project memories only from Claude Code; app-server `externalAgentConfig/detect|import`. | GA | https://learn.chatgpt.com/docs/import.md ; https://learn.chatgpt.com/docs/app-server.md | corrected |
| Claude Code | `/import [codex|gemini|cursor] [--dry-run] [--yes]` and `claude import`: instruction files, MCP servers, commands, subagents, skills; unavailable on third-party providers. | GA | https://code.claude.com/docs/en/commands.md ; https://code.claude.com/docs/en/cli-reference.md | unreviewed |
| Oh My Pi | Imports Claude and Codex session history (`--from-claude`). | GA | https://omp.sh/docs/sessions | reviewed |
| Antigravity | `agy plugin import` from `~/.claude/plugins/` and Gemini CLI extensions; App v2.30.0 imports MCP and skills from other coding agents. | GA | https://antigravity.google/docs/plugins ; https://antigravity.google/docs/changelog | unreviewed |
| rizzy-harness (ours) | none (no import or export of any kind). | none | `crates/master/src/http.rs:26-175` | from own inventory |

## 3. Availability matrix

Legend: `Y` documented and released; `B` beta, experimental, preview or partial; `P` planned, vendor claim only, or (for Ours) described in PLAN but not built; `-` nothing documented (or, for Codex's MCP server, removed); `?` unverified.

Columns: Cdx Codex, OCx OpenCodex, OC OpenCode, CC Claude Code, CD Claude Desktop, DSH DeepSeek harness, OMP Oh My Pi, Pi, Cur Cursor, AG Antigravity, OH OpenHands, OCl OpenClaw, gse goose, Her Hermes Agent, Zed Zed/ACP, Wrp Warp, Cln Cline.

| Feature | Cdx | OCx | OC | CC | CD | DSH | OMP | Pi | Cur | AG | OH | OCl | gse | Her | Zed | Wrp | Cln | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| MCP client | Y | - | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | - |
| MCP remote auth (OAuth) | Y | - | Y | Y | - | - | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | - |
| Tool deferral / tool search | Y | - | Y | Y | - | - | Y | Y | - | Y | - | Y | B | Y | - | - | - | P |
| MCP beyond tools (prompts, resources, elicitation, Apps, tasks) | Y | - | Y | Y | Y | B | Y | Y | Y | - | - | Y | Y | Y | Y | - | - | - |
| MCP limits and lifecycle documented | Y | - | Y | Y | - | B | - | - | - | Y | - | - | Y | Y | - | Y | - | - |
| MCP approval, trust, allow/deny | Y | - | Y | Y | Y | - | Y | Y | Y | Y | Y | - | Y | Y | Y | Y | Y | - |
| Connector directory / MCP registry | Y | - | - | Y | Y | - | - | - | - | - | Y | - | Y | - | Y | - | Y | - |
| MCP push channels | - | - | - | B | - | - | - | - | - | - | - | - | - | B | - | - | - | - |
| Harness as MCP server | - | - | - | Y | - | - | - | - | - | - | - | Y | Y | Y | - | B | - | - |
| Skills (SKILL.md format and discovery) | Y | - | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | P |
| Skill loading budget and invocation control | Y | - | Y | Y | - | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | - | Y | - |
| Shell injection in skills or commands | - | - | Y | Y | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| Agent-written skills or extensions | - | - | - | Y | - | B | Y | - | - | Y | - | ? | - | Y | - | - | Y | - |
| Skill registries and account sync | Y | - | Y | Y | Y | - | - | Y | - | - | Y | Y | - | Y | Y | - | Y | - |
| Plugin bundles | Y | Y | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | - | Y | - |
| Plugin distribution / marketplaces | Y | - | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | Y | - | Y | - |
| Plugin trust and supply-chain controls | Y | - | B | Y | Y | B | B | B | Y | B | B | Y | Y | Y | - | - | Y | - |
| Managed admin controls over extensions | Y | - | Y | Y | Y | - | - | - | Y | - | Y | Y | Y | - | - | - | Y | - |
| In-process extension API (middleware) | - | - | Y | Y | ? | B | Y | Y | - | - | B | Y | - | Y | - | - | Y | B |
| Hooks: events and decisions | Y | - | - | Y | ? | ? | Y | - | Y | Y | Y | Y | Y | Y | - | - | Y | - |
| Hook handler types beyond command | Y | - | - | Y | - | - | Y | - | Y | ? | Y | Y | - | Y | - | - | Y | - |
| Hook trust, precedence, failure documented | Y | - | Y | Y | - | - | Y | Y | Y | B | Y | Y | Y | Y | - | - | Y | - |
| Custom slash commands / templates | Y | - | Y | Y | - | - | Y | Y | B | Y | Y | Y | Y | Y | Y | Y | Y | - |
| Custom tools in code | - | - | Y | Y | - | B | Y | Y | Y | Y | Y | Y | - | Y | - | - | Y | - |
| Code mode | B | - | Y | - | - | - | P | Y | - | - | - | - | Y | Y | - | - | - | - |
| Embeddable SDK | Y | - | Y | Y | - | B | Y | Y | Y | Y | Y | ? | Y | B | - | Y | Y | - |
| Programmatic control protocol | B | - | B | Y | - | B | Y | Y | Y | ? | Y | Y | Y | Y | - | Y | B | B |
| OpenAI-compatible agent endpoint | - | - | - | - | - | - | - | - | - | - | Y | Y | - | Y | - | - | - | - |
| Structured output for callers | - | - | - | Y | - | - | - | - | - | Y | - | - | Y | - | - | - | - | - |
| ACP agent (server) | - | - | Y | - | - | B | Y | - | Y | Y | - | Y | Y | Y | - | ? | Y | P |
| ACP client (hosting other agents) | - | - | - | - | - | B | - | - | - | - | Y | Y | Y | - | Y | - | - | B |
| Hosting other harnesses, non-ACP | - | Y | - | - | - | B | - | - | - | - | - | Y | Y | ? | Y | Y | B | B |
| A2A | - | - | - | - | - | - | - | - | - | - | - | - | - | Y | - | - | - | - |
| Reads other harnesses' files in place | B | - | Y | Y | - | B | Y | Y | Y | - | Y | Y | Y | - | Y | Y | Y | - |
| One-shot import from another harness | Y | - | - | Y | - | - | Y | - | - | Y | - | - | - | - | - | - | - | - |

Notes on cells: Codex "Harness as MCP server" is `-` because it was removed. Zed/ACP "ACP agent" is `-` because Zed is the client side; the protocol itself is described in that entry. Antigravity "Programmatic control protocol" is `?` because its headless NDJSON stream is mentioned only in the survey's section 5 (area 17), outside the lines read. OMP code mode is `P` because the tool bridge is a README claim. Ours `B` for middleware, ACP client and non-ACP hosting means only the `AgentBackend` trait exists; `B` for the control protocol means a REST/WS API with an admin-equivalent token and no spec.

## 4. Common across the ecosystem

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

## 5. Only one harness has it

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

## 6. Ours against the field

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

## 7. Corrections, contradictions and unverified claims

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

## 8. Questions for Round 2 (source code only)

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
