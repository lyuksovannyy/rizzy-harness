# OpenCodex (round 1 survey, 2026-11-10)

Two unrelated projects share the name. Part A surveys `lidge-jun/opencodex` in full. Part B is a shorter section on `RyensX/OpenCodex`.

## Which one is it? (read this first)

| | lidge-jun/opencodex (Part A) | RyensX/OpenCodex (Part B) |
|---|---|---|
| What it is | Local **provider gateway/proxy**: translates Responses, Chat Completions and Anthropic Messages requests from coding clients (Codex, Claude Code, ...) to ~40 model providers; plus a web dashboard, CLI `ocx`, desktop tray app | **Browser front-end / middleware for the Codex (ChatGPT) Desktop app**: use Codex on a target machine from a phone or other computer |
| Runs its own agent loop? | **No.** The client (Codex, Claude Code, ...) runs the loop and tools. Only bounded exceptions: a web-search sidecar loop, a one-shot compaction summariser, a per-image vision call (https://opencodex.me/getting-started/how-it-works/). (corrected in review: no page states outright that the gateway runs no agent loop; the how-it-works and architecture pages describe a translator that passes tool calls through to the client plus a bounded search loop, so 'No' is a reading of those pages, not a quoted claim) Remote Workspace (experimental) is the one place it hosts an agent, but it launches Codex/Claude Code/Pi there, it does not implement a loop | **No (inferred).** README does not state it; it references Codex app-server/CLI binaries and an "official runtime", so it drives Codex's own runtime (https://github.com/RyensX/OpenCodex). Marked inference by the fetch, `unverified` |
| Licence | MIT | AGPL-3.0 |
| Activity | v2.100.1, 2026-11-10, ~18.4k stars | v2.1.0, 2026-09-13, ~730 stars |

Pick Part A if you meant "a router that lets Codex/Claude Code use any model". Pick Part B if you meant "remote web access to Codex". Neither is a harness comparable to Claude Code, OpenCode or Codex itself.

---

# PART A: lidge-jun/opencodex

## 1. Identity
- Name: opencodex (CLI `ocx`, npm `@bitkyc08/opencodex`). Maintainer: `lidge-jun`; community project, states it is not affiliated with OpenAI or Anthropic. Sponsors listed: OrcaRouter, TokenLab (https://github.com/lidge-jun/opencodex).
- Surfaces: CLI, local proxy, web dashboard (`localhost:10100`), desktop app with tray (macOS/Windows/Linux), background service, Docker Compose (README; https://opencodex.me/guides/desktop-app/).
- Open source, MIT, TypeScript on Bun (bundled Bun runtime, Node 18+ for the CLI). Default branch `dev`, 13,543 commits (README page).
- Version: `v2.100.1`, 2026-11-10 (release feed https://github.com/lidge-jun/opencodex/releases.atom); matches `00-targets.md`. The GitHub releases HTML page returned 502 twice, so only the two newest releases could be read.
- Surveyed from public docs only; no source read.

## 2. Features by area

### 1. Agent loop and execution model
Not applicable as a harness: it is a protocol translator. See entries below for the bounded loops it does run.

### Request translation pipeline
- What: Accepts `POST /v1/responses` (SSE, plus opt-in WebSocket upgrade), `/v1/chat/completions`, `/v1/messages`. Each request is validated, lowered to an internal request, routed to a provider, sent via a per-provider adapter and the answer is converted back to the client's event format. For OpenAI-Responses/Azure targets the body is passed through with targeted rewrites.
- Status: GA (v2.x)
- Evidence: https://opencodex.me/getting-started/how-it-works/ (How it works); https://opencodex.me/reference/architecture/ (Architecture)
- Notes: "Zero client patches" is the vendor's framing (https://opencodex.me/). Hard limits documented: 300 s upstream stall deadline for public upstreams, 32 MiB native-compaction body ceiling, WebSocket relay 4 MiB/frame. (verified in review)

### Web-search sidecar (small agentic loop)
- What: For routed (non-OpenAI) models, the hosted `web_search` tool is replaced by a synthetic function tool. The proxy runs searches via a configured backend model (OpenAI, Anthropic, xAI, Gemini, Exa), feeds results back as untrusted-wrapped tool results (deduplicated, capped 8,000 chars) and loops up to `maxSearchesPerTurn` (default 3), then forces an answer. Real client tools (shell, `apply_patch`) end the turn so the client receives them.
- Status: GA
- Evidence: https://opencodex.me/guides/sidecars/ (Web-search sidecar)
- Notes: Output buffered by default; `streamRoutedModelOutput` is opt-in and can partly repeat text. (verified in review)

### 2. Built-in tools
Not applicable: it ships no coding tools. The only synthetic tool is `web_search` (above). The client's tools (shell, `apply_patch`, MCP namespaces, `tool_search`) are relayed; the docs claim they round-trip correctly (https://opencodex.me/getting-started/how-it-works/). Shared tool-argument repair fixes integer-typed fields against the tool schema (https://opencodex.me/reference/adapters/).

### 3. Code editing and file operations
Not applicable (client does it). Freeform `apply_patch` tool passthrough is stated to work (How it works page).

### 4. Terminal and shell integration
- Not applicable for execution. Related: `ocx codex-shim install` wraps the `codex` launcher so the proxy starts on demand (`ocx ensure`); on macOS/Linux the user sources an env file, the tool does not edit shell files. Desktop app can configure its bundled `ocx` for new terminals.
- Status: GA
- Evidence: https://opencodex.me/guides/codex-integration/ (Shim and launcher); https://opencodex.me/guides/desktop-app/

### 5. Permissions, approvals and sandboxing
- Not applicable to tool approvals (client's job). What exists is gateway access control, see sections 17 and 21. Remote Workspace sandboxes commands with bubblewrap on Linux (entry in 17).

### 6. Context management and compaction
- What: For native passthrough, `/v1/responses/compact` is forwarded upstream. For routed models, the proxy acts as a tool-free summariser returning the history shape Codex expects; after routed compaction a checkpoint note is added to system instructions. Per-provider context caps (`providerContextCaps`, default cap value 350,000, only lowers known windows). Upstream 413 becomes a `context_length_exceeded` event.
- Status: GA
- Evidence: https://opencodex.me/getting-started/how-it-works/ (step 7); https://opencodex.me/reference/adapters/ (cross-cutting); https://opencodex.me/guides/model-routing/ (context caps); https://opencodex.me/reference/architecture/ (limits)
- Notes: A context-management relay needs a remote bind with a key; it does not work through default loopback integration (https://opencodex.me/guides/codex-integration/).

### 7. Memory and persistent instructions
- Nothing documented as a user memory feature. Related: "Codex prompt layers" (read/toggle/edit prompt layers and base variants) appear in the management API list and edits need a consent-bound dashboard session; "native reasoning retention" is two opt-in settings, default off (https://opencodex.me/reference/management-api/; https://opencodex.me/guides/web-dashboard/). Details of prompt layers not read: `unverified`.

### 8. Planning, tasks and modes
Not applicable. (Reasoning-effort mapping/clamping per provider exists: https://opencodex.me/reference/adapters/.)

### 9. Sub-agents and multi-agent orchestration
- What: The proxy does not run sub-agents; it controls which collaboration surface Codex uses and which models Codex offers for `spawn_agent`. Mode (`v1` default, `base`, `v2`) is written as `multi_agent_version` into the Codex model catalog (live `/v1/models` and on-disk). Up to five featured models are advertised (Codex lists at most five). Fallback order for a spawned worker: requested model, per-model chain, global chain. Preferred worker model/effort is guidance injected for the main agent, not a router. Known limit: in v2, tasks from a ChatGPT-native parent to a routed child are encrypted and fail with `unreadable_encrypted_agent_task`.
- Status: GA, with an experimental recovery option for the encrypted case (off by default)
- Evidence: https://opencodex.me/guides/sub-agent-surface/ (Sub-agent surface); https://opencodex.me/guides/web-dashboard/ (Subagents page)
- Notes: Cross-provider delegation demos (Claude Desktop dispatching a GPT sub-agent, Grok Build calling a Kimi sub-agent) are README claims (`vendor-claim`). (verified in review)

### 10. Background, long-running and scheduled work
- Background service (launchd / systemd user unit / Windows Task Scheduler, opt-in WinSW) via `ocx service`; a desktop app restarts the proxy it started if it dies; SSE heartbeat comment every 2 s during upstream silence. No scheduled agent work (not applicable).
- Status: GA
- Evidence: https://github.com/lidge-jun/opencodex (CLI commands); https://opencodex.me/guides/desktop-app/; https://opencodex.me/reference/architecture/

### 11. Sessions: persistence, resume, fork, checkpoints, undo
- Not applicable (client owns sessions). Gateway-side: thread-to-account affinity (process-local, not persisted); sync/restore use a backup manifest to restore each thread's original provider metadata; `ocx recover-history --legacy-openai --yes` is a broad history rewrite. Cursor adapter reuses checkpoints and Devin adapter reuses named conversations.
- Status: GA
- Evidence: https://opencodex.me/reference/architecture/; https://opencodex.me/guides/codex-integration/ (Restore and disable); https://opencodex.me/reference/adapters/

### 12. Git and version control
Not applicable.

### 13. Extensibility: MCP, plugins, skills, hooks, SDKs, protocols
- What: No plugin/hook/skill system documented for the gateway. MCP tool namespaces are relayed for Codex. Extension points are adapters (five wire families plus Kiro, Cursor, Devin, Zed, Command Code), custom OpenAI-compatible providers and a management API (HTTP). An `AGENTS_INSTALL.md` and a "for agents" page tell coding agents how to install and verify it.
- Status: GA (adapters, API); experimental for Zed, Cursor
- Evidence: https://opencodex.me/reference/adapters/; https://opencodex.me/reference/management-api/; https://opencodex.me/getting-started/for-agents/
- Notes: Management API page truncated at 100k chars when read; last ~15k chars unread.

### Management API
- What: Admin control plane used by the dashboard: provider CRUD and tests, combos, models, OAuth accounts, keys (masked), logs/usage, config, sync/update jobs, remote workspace pairing, system restart-with-drain/stop. Separate admin credential that must differ from data-plane keys; loopback dashboard gets a 5-minute session tied to origin; mutations need Origin match plus CSRF; preview routes write nothing and confirmations carry a plan fingerprint (stale plan returns 409); 2 MiB body limit.
- Status: GA
- Evidence: https://opencodex.me/reference/management-api/
- Notes: Interesting preview-then-confirm design. (verified in review)

### 14. Models, providers and inference configuration
- What: 40+ built-in providers (OpenAI via ChatGPT login or key, Anthropic, Gemini, xAI, Kimi, Azure OpenAI, Ollama local/cloud, Cursor experimental, DeepSeek, Groq, OpenRouter, Mistral, NVIDIA NIM, etc.) plus any OpenAI-compatible endpoint. Routing is a fixed first-match order: account selector, combo id/alias, explicit `provider/model`, bare OpenAI-family ids, provider `defaultModel`, built-in prefixes (`claude-*` to anthropic, etc.), provider `models[]`, `defaultProvider`. Aliases, `disabledModels`/`selectedModels` catalog filtering, new-model policy (default off, mark NEW), per-provider context caps, Fast rows (`--fast` selectors requesting the priority tier), reasoning-effort mapping.
- Status: GA
- Evidence: https://github.com/lidge-jun/opencodex; https://opencodex.me/guides/model-routing/; https://opencodex.me/reference/configuration/; https://opencodex.me/reference/adapters/
- Notes: Provider count is a README/home claim; full provider matrix not opened. (verified in review)

### Combos (virtual models with failover)
- What: `combo/<id>` routes each request across ordered targets. Strategies: failover (default), weighted round-robin (with `stickyLimit`), random, least-used, reset-window (soonest quota reset), `jev` (external decision backend picks target and effort). Hop errors (401/403/404/408/429/5xx) cool the target and move on; invalid requests and policy refusals stop; context overflow moves on without cooling; cooldown from `Retry-After` then `cooldownMs`; last-resort targets and metered-target consent; tool calls stay on the target that issued them for the turn; streaming fails over only before any output.
- Status: GA (jev is opt-in)
- Evidence: https://opencodex.me/guides/combos/
- Notes: Most mature gateway feature; directly relevant to provider failover design. (verified in review)

### ChatGPT account pool
- What: Multiple ChatGPT/Codex accounts with quota-aware selection (5h, weekly, 30d windows), a per-account selection order (First, Earlier, Normal, Later, Last) and lowest-usage choice for new sessions (corrected in review: 'round-robin or fill-first' not found on the dashboard page), thread affinity (default on), cooldown after 429, reauth flag after 401/403, manual pin, per-account model-picker targeting (opt-in).
- Status: GA; Claude account pool is labelled experimental
- Evidence: https://opencodex.me/guides/web-dashboard/ (Codex Auth and account pools); https://opencodex.me/getting-started/how-it-works/; https://opencodex.me/guides/claude-code/ (Caveats)
- Notes: Docs warn pooling gives no protection from provider enforcement and Anthropic may suspend accounts routed through third-party proxies. Policy risk for anyone copying this. (corrected in review: selection-order wording changed, see What; thread affinity default on confirmed at https://opencodex.me/guides/web-dashboard/)

### Vision sidecar
- What: For models marked text-only, each image is described by a sidecar model (default backend Anthropic if an OAuth account is usable, else OpenAI) and replaced with text. Three at a time, 8 per turn default, `data:` descriptions cached.
- Status: GA
- Evidence: https://opencodex.me/guides/sidecars/ (Vision sidecar)
- Notes: (verified in review)

### Auth to providers
- What: OAuth with auto-refresh (xAI, Anthropic, Kimi), forwarding a `codex login`, pasted keys, `${ENV_VAR}` references resolved at request time. OAuth tokens kept in separate credential stores, not `config.json`; per-account file locks.
- Status: GA
- Evidence: https://github.com/lidge-jun/opencodex; https://opencodex.me/reference/configuration/ (Secrets); https://opencodex.me/reference/architecture/

### 15. UI surfaces
- What: Web dashboard (new and `/classic/` design) with eight sidebar pages: Dashboard, Connect, Codex, Providers, Models, Subagents, Usage & Logs, Remote Link. Quota chip bar (amber at 70%, red at 90%). Provider test (connection, completion with 32-token cap, test-all with confirmation). Desktop app = native tray + dashboard shell, usage popover, macOS WidgetKit widget, update check at startup and every 6 hours with manual install. Machine-readable docs in 8 languages.
- Status: dashboard GA; desktop app labelled beta in README, but the desktop page does not say beta (sources disagree, minor)
- Evidence: https://opencodex.me/guides/web-dashboard/; https://opencodex.me/guides/desktop-app/; https://github.com/lidge-jun/opencodex
- Notes: No TUI, IDE or voice surface for the gateway itself, though it proxies `/v1/live` and `/v1/realtime/calls` voice endpoints for Codex (architecture page).

### 16. Browser automation and computer use
Not applicable. (Video/image bridges exist as guides, `/guides/image-bridge/`, `/guides/video-bridge/`, not opened: `unverified`.)

### 17. Remote, cloud, headless, CI and distributed execution
#### Remote hub
- What: One host holds provider credentials, catalog and usage; clients keep only a per-client data key and send inference directly to the hub's data listener. Separate management listener bound to 127.0.0.1, meant to sit behind Tailscale Serve or another HTTPS frontend. Pairing: `ocx hub invite` makes a single-use rate-limited code; client runs `ocx connect` with code via stdin; keys rotate with a 10-minute overlap; revocation is a dashboard action (disconnect is local only). Model scope `allowedModels`, 403 when out of scope. Data listener is plain HTTP, TLS must come from the operator.
- Status: GA per docs; no official container image published (build from source), contradicting README's Docker Compose mention only loosely (README says Docker Compose build, hub page says no official image)
- Evidence: https://opencodex.me/guides/remote-hub/; https://opencodex.me/guides/web-dashboard/ (Remote Hub)
- Notes: The unauthenticated loopback companion listener lets any local user use the hub's credentials; docs say single-tenant host only. (corrected in review: pairing code is single-use with a five-minute lifetime; the rest, plain-HTTP data listener, loopback management listener, 10-minute key overlap, no official container image, unauthenticated loopback companion listener, was confirmed at https://opencodex.me/guides/remote-hub/)

#### Remote Workspace
- What: Splits work across two machines. The Hub runs the coding agent (Codex, Claude Code or Pi) and its model session; an Executor on a second computer supplies files from one approved root and, on Linux only, runs commands inside bubblewrap. Executor connects outbound over WebSocket (no public port), encrypted RPC v2 (Ed25519-signed ECDH, AES-256-GCM), one-time pairing code, hub stores only a hash of the bearer, ten failed pairing codes within ten minutes return a generic 429 with Retry-After (corrected in review). Read-only default; write needs a command-sandbox probe; disconnected executor never falls back to the hub filesystem.
- Status: experimental (needs `OCX_REMOTE_WORKSPACE_ENABLED=1`)
- Evidence: https://opencodex.me/guides/remote-workspace/
- Notes: v2.100.0 notes add a Rust `ocx-child` relay with offline journal and "zero-preinstall SSH setup" (release feed); whether this is the same feature is `unverified`. This is the closest thing to a master/node design in the project. (verified in review)

#### Headless and health
- What: `ocx health`/`ready`/`status`/`doctor`; `GET /healthz`, `/readyz` (503 with Retry-After while pending); drain mode returns 503 Retry-After 5; non-interactive setup via `ocx provider add`.
- Status: GA
- Evidence: https://github.com/lidge-jun/opencodex; https://opencodex.me/getting-started/for-agents/; https://opencodex.me/reference/architecture/

### 18. Collaboration, teams and enterprise administration
- What: Per-client keys with usage scoped by `apiKeyId`; Tailscale-identity allowlist for dashboard sessions; optional public leaderboard profile (default not confirmed in review). No SSO/teams/RBAC documented beyond that.
- Status: GA (hub); leaderboard `unclear`
- Evidence: https://opencodex.me/guides/remote-hub/; https://opencodex.me/guides/web-dashboard/

### 19. Observability: usage and cost, logs, tracing
- What: Usage & Logs page: per-app token share, estimated cost (API list-price equivalent, not billing), cache hit/write rates, tokens/sec throughput, coverage warnings; request log ring with filters by provider, model, status, time window, speed bucket, conversation ID (accepts `codex://threads/<id>`); classified upstream failure reasons and retry counts; JSONL usage logs. Memory accounting endpoint `GET /api/system/memory` (admin) returning process, heap, stream and active-turn metrics (corrected in review: the 256 MiB default budget was not found on the management-API or architecture pages).
- Status: GA
- Evidence: https://opencodex.me/guides/web-dashboard/ (Usage, Logs); https://opencodex.me/reference/architecture/; https://github.com/lidge-jun/opencodex
- Notes: Public usage dashboard at opencodex.me/dashboard fed by the opt-out telemetry (release v2.100.0).

### 20. Configuration and customisation
- What: `$OPENCODEX_HOME/config.json` (default `~/.opencodex/`). Edit via dashboard, CLI (`ocx provider|models|combo|agent|config`) or by hand (stop the proxy first, live saves can overwrite). Invalid file is renamed `config.json.invalid-<timestamp>` and defaults load; mutating commands refuse an invalid file. Values in file override defaults; `HTTP(S)_PROXY` env beats `proxy` field. Atomic temp-file-plus-rename writes for managed files. Client configs for ~25 tools are exported or managed (JSON/YAML/TOML/JSON5), editing only their own block with lock files where noted.
- Status: GA
- Evidence: https://opencodex.me/reference/configuration/; https://opencodex.me/reference/client-support/
- Notes: Client-support page lists 28 clients (the survey's "~25" is low) and says "runtime verified" is a historical record of a person running a specific client version, while "test-covered" only means a reviewed test exists; Claude Desktop and Grok Build have no runtime-verified record, but a blanket "no client" statement was not found (corrected in review).

### Client integrations
- What: Native: Codex (CLI/app/SDK) by editing `~/.codex/config.toml` (`openai_base_url`, `model_catalog_json`; non-loopback adds a `[model_providers.opencodex]` block), Claude Code via `ocx claude` setting `ANTHROPIC_BASE_URL`, Claude Desktop (gateway mode, or first-party "picker" mode that installs a constrained local CA and routes via `HTTPS_PROXY`/`NODE_EXTRA_CA_CERTS`; not on Linux), Grok Build. Managed/exported: OpenCode, Pi, Oh My Pi, Hermes, OpenClaw, Kimi CLI, DSH, Cline, Kilo Code, T3 Code, Factory Droid and others.
- Status: GA for Codex and Claude Code (test-covered); first-party Claude Desktop interception and account pooling experimental
- Evidence: https://opencodex.me/reference/client-support/; https://opencodex.me/guides/codex-integration/; https://opencodex.me/guides/claude-code/
- Notes: Both guides were truncated at 100k chars when read. Docs warn first-party mode may breach Anthropic's terms and cause suspension. Sync/restore is idempotent but "not byte for byte" on re-enable. A user-owned `openai_base_url` is never overwritten.

### 21. Security and privacy
- What: Loopback default with no token; any non-loopback bind refuses to start without `OPENCODEX_API_AUTH_TOKEN` (header `x-opencodex-api-key`). Admin token separate; dashboard keeps it in memory only; optional "remember device" (off by default; stored record has a fixed 30-day expiry; HttpOnly SameSite=Strict cookie plus device proof, hashed records) that cannot perform consent actions; some actions need an explicit paired session (revealing key values, applying a Codex update). Host/Origin checks on loopback listeners. Docker image: non-root, read-only root fs, dropped caps, digest-pinned. Build verifies a compatibility manifest by SHA-256. Vulnerability reports via GitHub private reporting only.
- Telemetry: default-on anonymous daily summary to telemetry.opencodex.me after a consent popup (daily-changing install hash, version, platform, provider kind, model family, counts; no prompts, outputs, keys, paths). Opt-out: `ocx telemetry disable`, `OCX_TELEMETRY=0`, `DO_NOT_TRACK=1`.
- Agent consent guard: API returns `403 agent_consent_required` so an agent cannot star the repo for the user.
- Status: GA
- Evidence: https://github.com/lidge-jun/opencodex; https://opencodex.me/guides/web-dashboard/; https://opencodex.me/guides/remote-hub/; https://opencodex.me/reference/management-api/; https://opencodex.me/getting-started/for-agents/; release feed v2.100.0 (default-on telemetry)
- Notes: Default-on telemetry arrived 2026-11-10 (v2.100.0), the same day as the survey.

### 22. Licence, pricing and distribution
- MIT, free. npm `@bitkyc08/opencodex`, desktop installers (macOS notarized .dmg, Windows .msi not yet signed, SignPath signing rolling out; Linux AppImage/.deb), Docker Compose, from source. Release cadence: multiple per week at the time of survey (v2.100.0 and v2.100.1 on the same day).
- Evidence: https://github.com/lidge-jun/opencodex; https://opencodex.me/

### 23. Anything else or unusual
See next section. Also: the project's docs include a page addressed to coding agents (`/getting-started/for-agents/`) that sets consent rules for them.

## 3. Distinctive or unusual
- Rewriting Codex's model catalog (`multi_agent_version`, `model_catalog_json`) to steer the client's sub-agent surface without patching it: interesting as "control a closed client by controlling the data it reads".
- Combos with six strategies, quota-window-aware selection, tool-call stickiness within a turn and failover only before output starts.
- Preview-then-confirm management API with plan fingerprints and consent-bound actions that admin tokens cannot perform.
- Hub/data-plane vs. management-plane listener split with separate tokens.
- Remote Workspace: agent on the hub, files and bubblewrap commands on the executor, outbound-only WebSocket.
- Web-search sidecar that gives any model a search tool and wraps results as untrusted data.

## 4. Architecture and UX notes
- Single Bun process hosts proxy, management API and WebSocket bridge; optional OS service. Source layout named in docs: server, responses, router, adapters, bridge, oauth, codex, providers, usage, vision, web-search (https://opencodex.me/reference/architecture/).
- Data plane (`/v1/*`) and management plane (`/api/*`) are separate credentials; default port 10100 (README).
- State on disk: config.json, OAuth store, JSONL usage logs, one-time Codex catalog backup. Thread affinity is in memory only.
- The agent loop and tools run in the client; the gateway sees every prompt and tool call in transit.

## 5. Relevance to a self-hosted master/node harness
- Provider failover (Combos): a model of cooldowns, hop-error classification and "fail over only before output" is directly reusable in the master's provider layer.
- Quota-aware account selection with thread affinity (ChatGPT pool): affinity keeps prompt caches warm; note the ToS risk the docs call out.
- Remote Workspace splits agent (hub) from files and commands (executor) with outbound-only WebSocket and signed pairing: close to master/node but inverted (our node runs the loop). Their experimental status and "three-computer acceptance" gap are lessons.
- Two-listener hub design and per-client keys with rotation overlap and `apiKeyId` usage scoping: useful for node enrollment and revocation.
- Management API with preview, plan fingerprint and consent-bound actions maps to approval-style flows for sensitive dashboard actions.
- Usage page honesty (estimates vs. billing, coverage warnings, tokens/sec) is a UX bar to match.
- Compatibility surface (Responses, Chat Completions, Messages) shows how many clients a gateway must serve; a harness exposing such an endpoint could be reused by other clients.
- Sidecar pattern (search/vision for text-only models) is a cheap way to give weaker models parity.
- Cautionary: default-on telemetry shipped in v2.100.0 and edits to another tool's config files proved a large support surface (restore caveats, stale catalogs).

## 6. Overhyped, stale or contradictory
- "40+ providers", "zero client patches", sub-agent demos: vendor claims; client page says no client has runtime-verified results.
- README calls the desktop app beta; the desktop docs page does not.
- README lists Docker Compose; the remote-hub page says no official image is published.
- Remote Workspace is presented as a feature but is an "experimental foundation".
- 502 errors blocked the GitHub release list; only two release notes seen.

## 7. Unverified
- Full provider list and `/guides/providers/`, `/reference/cli/`, `/reference/proxy-formats/`, `/guides/image-bridge/`, `/guides/video-bridge/`, `/guides/opencode/` and other client guides: not opened.
- Tail of Claude Code, Codex integration and management API pages (unread beyond 100k chars).
- Relationship of Remote Workspace to the "zero-preinstall SSH setup" and `ocx-child` relay in v2.100.0.
- Release history before v2.100.0: needs the GitHub releases page (returned 502).
- Prompt layers behaviour: needs the management API tail or a Codex-integration page section.

---

# PART B: RyensX/OpenCodex (shorter)

## Identity
- "A middleware layer for Codex/ChatGPT Desktop": lets you use Codex on a target machine from a phone, tablet or other computer via a browser. AGPL-3.0, JavaScript, ~732 stars, 198 commits (https://github.com/RyensX/OpenCodex). Latest release `2.1.0`, 13 Sep (https://github.com/RyensX/OpenCodex/releases; page shows dates without year, 2026 inferred from `00-targets.md`).
- Surfaces: web gateway (browser/PWA), desktop launchers for macOS and Windows, CLI start for Linux.
- Requires a local Codex Desktop or ChatGPT Desktop install with Codex; the app need not be running.
- Agent loop: the README does not say. References to Codex app-server/CLI binaries, an "official hidden runtime" with isolated profiles and IPC sockets, and `CODEX_HOME` suggest it drives Codex's own runtime. `unverified` inference; confirming needs the architecture docs or source.

## Documented features
### Browser access to Codex
- What: Use Codex from a browser over localhost, LAN, or Tailscale/ZeroTier/VPN; optional access password (`auth.password`), default token lifetime 12 h, file-preview tokens 5 min; README recommends not exposing publicly and gives no relay service. Claims full Codex functionality including file tree, terminal and review (`vendor-claim`).
- Status: GA (2.x)
- Evidence: https://github.com/RyensX/OpenCodex (README)

### Smart scheduling (Auto)
- What: A plugin that adds an Auto model option picking model and reasoning effort per task, with custom tiers (v2.0.3; dynamic tiers v2.1.0).
- Status: GA
- Evidence: https://github.com/RyensX/OpenCodex; https://github.com/RyensX/OpenCodex/releases

### Plugin system
- What: Plugins with external third-party plugin directories (v2.0.1), Plugin v2 SDK (v2.1.0), a plugin that sorts projects by recent activity.
- Status: GA
- Evidence: https://github.com/RyensX/OpenCodex/releases

### Mobile and usage
- What: Mobile UI, PWA use, real token usage display, real-time sync of Codex Desktop task lists and activity (v2.0.3), remote file download and selecting existing folders as projects remotely (v2.0.2), login brute-force protection (v2.0.2), launcher that stays in the tray, anti-hibernation option, launcher prefers installed PWA, backup/restore of official runtime resources (v2.1.0).
- Status: GA
- Evidence: https://github.com/RyensX/OpenCodex; https://github.com/RyensX/OpenCodex/releases

### Limitations stated
- Late session-state sync if used alongside the official Desktop; first-time history load may be empty or slow; no prebuilt Linux launcher; port conflicts and macOS first-launch prompt.
- Evidence: https://github.com/RyensX/OpenCodex

Not covered for Part B (not read): docs beyond the README, plugin developer docs, source. Areas such as models, sandboxing and context management are not applicable: it fronts Codex.

---

# 8. Sources (all opened 2026-11-10)
- https://github.com/lidge-jun/opencodex (README: install, CLI, clients, security, telemetry)
- https://github.com/lidge-jun/opencodex/releases.atom (v2.100.0, v2.100.1 notes; the `/releases` page returned 502 twice)
- https://opencodex.me/ (home page claims, nav)
- https://opencodex.me/getting-started/how-it-works/ (request flow, loop scope)
- https://opencodex.me/getting-started/for-agents/ (agent consent, install)
- https://opencodex.me/reference/architecture/ (processes, endpoints, limits)
- https://opencodex.me/reference/configuration/ (config file, precedence)
- https://opencodex.me/reference/client-support/ (client matrix)
- https://opencodex.me/reference/adapters/ (adapters)
- https://opencodex.me/reference/management-api/ (control plane; truncated)
- https://opencodex.me/guides/web-dashboard/ (dashboard, auth, usage, logs)
- https://opencodex.me/guides/remote-hub/ (hub topology and security)
- https://opencodex.me/guides/remote-workspace/ (Remote Workspace)
- https://opencodex.me/guides/sub-agent-surface/ (sub-agent surface)
- https://opencodex.me/guides/model-routing/ (routing order)
- https://opencodex.me/guides/combos/ (combos)
- https://opencodex.me/guides/sidecars/ (web and vision sidecars)
- https://opencodex.me/guides/claude-code/ (Claude Code/Desktop; truncated)
- https://opencodex.me/guides/codex-integration/ (Codex wiring; truncated)
- https://opencodex.me/guides/desktop-app/ (desktop app)
- https://github.com/RyensX/OpenCodex (README)
- https://github.com/RyensX/OpenCodex/releases (v1.0.0 to v2.1.0 notes)


# 9. Review

Reviewer: independent agent, 2026-11-10. Public pages only.

Entries checked: 21 of about 33 (the file is short; Part A has about 27 entries, Part B 5). Verdicts: confirmed 15, confirmed-with-correction 6, not-supported 0 whole entries, wrong 0, unreachable 0.

| Entry | Verdict | Reason |
|---|---|---|
| Identity: two unrelated projects | confirmed | lidge-jun/opencodex is a local gateway (MIT, 18.4k stars); RyensX/OpenCodex is a Codex Desktop browser middleware (AGPL-3.0, 732 stars). |
| "Runs no agent loop" (table row and section 4) | confirmed-with-correction | No page states it in those words. how-it-works and architecture describe a translator that passes client tool calls through plus a bounded web-search loop; the claim is a fair reading, now labelled as such. |
| Request translation pipeline, limits | confirmed | 300 s stall, 32 MiB compaction ceiling, 4 MiB frames are on the architecture page. |
| Web-search sidecar | confirmed | Default 3 searches per turn, 8,000-char cap, untrusted wrapping, five backends. |
| Vision sidecar | confirmed | Three at a time, 8 descriptions per turn default. |
| Sub-agent surface | confirmed | v1 default, base, v2, five-model limit, fallback order, `unreadable_encrypted_agent_task`, recovery off by default. |
| Model routing order | confirmed | Eight first-match steps as listed; 350,000 is the dashboard default for `contextCapValue`, inactive until a provider is in `providerContextCaps`. |
| Combos and routing strategies | confirmed | failover (default), round-robin, random, least-used, reset-window, jev; hop codes 401/403/404/408/429/5xx; stickyLimit 1 to 100; failover only before output; tool-call stickiness. Extra detail: jev fails open to the first eligible target. |
| ChatGPT account pool | confirmed-with-correction | "round-robin or fill-first" not on the dashboard page, which describes a selection order and lowest-usage choice; thread affinity default confirmed. |
| Management API | confirmed | Separate admin token, 5-minute sessions bound to origin, Origin plus CSRF, 409 `integration_preview_stale`, 2 MiB limit. |
| Memory accounting (256 MiB) | confirmed-with-correction | Figure not found on either page; removed. |
| Remote hub | confirmed-with-correction | Pairing code lifetime is five minutes (added); other claims confirmed. |
| Remote Workspace | confirmed-with-correction | Hub runs Codex, Claude Code or Pi; Linux-only bubblewrap commands; read-only default; no hub fallback; flag `OCX_REMOTE_WORKSPACE_ENABLED=1`; encryption as stated. Correction: the rate limit is ten failed codes in ten minutes, answered with 429. |
| Web dashboard (security, UI) | confirmed-with-correction | Eight sidebar rows, 70%/90% chips confirmed; "30-day remember device" is a 30-day record expiry and remember is off by default; leaderboard default not found. |
| Client support page | confirmed-with-correction | 28 clients, not about 25; the "no client runtime-verified" statement is not a blanket claim on the page. |
| README facts (MIT, port 10100, desktop beta, Docker Compose, telemetry notice) | confirmed | README lists 40+ providers, desktop app (beta), digest-pinned non-root Compose build, telemetry after notice. Version number is not on the README page. |
| RyensX identity, releases, token TTLs, plugins, smart scheduling | confirmed | AGPL-3.0, 732 stars, v2.1.0 on 13 Sep, 12 h and 5 min TTLs, plugin system from 2.0.1, Smart Scheduling in 2.0.3, brute-force protection in 2.0.2. The README does not say how it drives Codex; the survey's inference is flagged as inference. |

**Borrowed features.** None found. The survey is explicit that Part A is a translator and takes care not to describe the clients' features as its own.

**Notable omissions** (pages the survey names as not opened; I did not read them either): https://opencodex.me/guides/providers/ (provider matrix), https://opencodex.me/reference/cli/, https://opencodex.me/reference/proxy-formats/, https://opencodex.me/guides/image-bridge/, https://opencodex.me/guides/video-bridge/, https://opencodex.me/guides/opencode/. The README's note on the GitHub release feed for the "zero-preinstall SSH setup" was not checked.

**Trust of the unchecked remainder.** Good. Every error found was a small detail (a figure, an option name, a default) and none changed what a feature is. The weakest spots are the v2.100.0 and v2.100.1 release-note claims (default-on telemetry, `ocx-child` relay), which come from an Atom feed I did not re-open, and the Part B agent-loop inference, which the survey already labels `unverified`.
