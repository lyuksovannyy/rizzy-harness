# Synthesis 10: configuration, licence and distribution, other; plus distinctive and overhyped across all harnesses

Research date 2026-11-10. Synthesis only: every fact below comes from the survey files and review files in `research/round1/`. Nothing was browsed. "Nothing documented" means the survey found no page, not that the feature does not exist.

## 1. Scope

Areas covered: **20 Configuration and customisation**, **22 Licence, pricing and distribution**, **23 Anything else or unusual**. Plus, from every survey file, **section 3 "Distinctive or unusual"** and **section 6 "Overhyped, stale or contradictory"** in full (they are the two extra sections at the end of this file).

Read (line ranges, current file versions):

| File | Areas 20/22/23 | Section 3 | Section 6 | Other lines read for themes, key bindings, personas, status lines, managed layers |
|---|---|---|---|---|
| `codex.md` | 914-985 | 986-999 | 1050-1064 | 439-442, 404-408, 720-731, 866-881 |
| `opencodex.md` | 168-193 | 195-201 | 220-225 | 1-60 (identity) |
| `opencode.md` | 418-457 | 458-469 | 492-500 | 357-359, 295 |
| `claude-code.md` (900 KB; ranges only) | 4456-4641, 5055-5200 | 5202-5236 | 5285-5324 | 784, 882, 2873, 3342-3357, 4044, 4378 |
| `claude-desktop.md` | 421-467 | 469-479 | 503-511 | - |
| `deepseek-harness.md` | 471-531 | 533-546 | 571-580 | 196 |
| `oh-my-pi.md` | 497-547 | 549-562 | 584-596 | 413 |
| `pi.md` | 507-559 | 561-572 | 595-606 | 313, 438 |
| `cursor.md` | 419-454 | 456-467 | 500-509 | 469-486 (worker config) |
| `antigravity.md` | 431-492 | 494-507 | 531-541 | 368 |
| `openhands.md` | 421-457 | 459-470 | 493-504 | 9-11 (identity) |
| `openclaw.md` | 528-594 | 596-609 | 633-645 | 211 |
| `goose.md` | 474-516 | 518-529 | 552-563 | 442-447 (governance) |
| `hermes-agent.md` | 635-677 | 679-692 | 715-726 | 275, 543, 610 |
| `zed-acp.md` | 191-208 | 355-364 | 385-393 | 336-341 (ACP registry) |
| `warp.md` | 484-544 | 546-558 | 581-589 | 117, 364 |
| `cline.md` | 425-474 | 476-487 | 511-522 | 395-403 |
| `own-rizzy-harness.md` | 276-311 (section 17 and 18), plus greps | - | - | lines 90, 96, 102, 128, 144, 184, 243-246 |

Reviews: `claude-code.review-1.md`, `-2.md`, `-3.md` (corrections and Move-to-Unverified lists applied); the `## 9. Review` tables of the other files (corrections were already applied in place in those survey texts).

Could not read / limits:
- Section 3 lines rarely carry a URL. Evidence URLs in the section 3 table were taken from the matching section 2 entry of the same file (marked "entry"), not from the section 3 line. OpenHands section 3 gives only file names (for example `backends.md`); the base is `https://docs.openhands.dev` per the survey's own shorthand `D:`; the directory was not given in section 3. Hermes evidence paths are relative to `https://hermes-agent.nousresearch.com/docs` as the survey writes them.
- Claude Code was read by range only. Settings precedence, themes, key bindings, output styles and status line entries exist in the file as shown; the 900 KB file was not read whole.
- Antigravity says its precedence tables are in its areas 5 and 7, which are outside this group. OMP themes and key bindings pages were not opened by the survey. Zed and ACP area 20 is one short entry.
- Cells marked `-` in the matrix are weak evidence: they mean nothing was found in the lines read.

## 2. Features

### Area 20: Configuration and customisation

#### Config file formats and locations
What it is: where a harness keeps its settings, in which format, and what else sits next to it (credentials, MCP, keybindings, prompts).
Variants: TOML (Codex, Warp CLI, ours), JSON/JSONC (OpenCode, Claude Code strict JSON, Pi, Antigravity, Zed, OpenCodex), JSON5 with schema validation (OpenClaw), YAML (OMP, goose, Hermes, DSH patches). A few split "settings" from "app state" (Claude Code `~/.claude.json`) or from a client-only file (OpenCode `cli.json`). Antigravity writes only non-default values. DSH composes config from ordered YAML patches instead of one file. OpenHands and Claude Desktop are mainly UI-edited.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | TOML: user `~/.codex/config.toml`, project `.codex/config.toml` (trusted projects), per-profile file, `/etc/codex/config.toml`, and enforced `requirements.toml`. | GA | https://learn.chatgpt.com/docs/config-file/config-basic.md | unreviewed |
| OpenCodex | `$OPENCODEX_HOME/config.json` (default `~/.opencodex/`); atomic temp-file-plus-rename writes; also edits blocks in the config files of 28 client tools (survey's "~25" corrected). | GA | https://opencodex.me/reference/configuration/, https://opencodex.me/reference/client-support/ | corrected |
| OpenCode | JSON/JSONC with `$schema`; `{env:VAR}` and `{file:path}` substitution; global `~/.config/opencode/`, project and `.opencode/` files, system dirs; separate `cli.json` for terminal client options. | GA | https://opencode.ai/docs/config/, https://opencode.ai/docs/migrate-v1/ | reviewed |
| Claude Code | Four strict-JSON settings files (user, shared project, local project, managed) plus `~/.claude.json` app state; `CLAUDE_CONFIG_DIR` relocates home files; about 250 settings keys each tagged with a scope; `.claude/` directory layout with frontmatter per file type. | GA | https://code.claude.com/docs/en/settings.md, https://code.claude.com/docs/en/settings-reference.md, https://code.claude.com/docs/en/claude-directory.md | unreviewed |
| Claude Desktop | Settings UI (Settings > Code), saved local environments, `permissions.defaultMode`; the 3P edition has an in-app configuration window and a documented key reference. | GA | https://code.claude.com/docs/en/desktop, https://claude.com/docs/third-party/claude-desktop/overview.md | unreviewed |
| DeepSeek harness | Profile directory `$DSH_HOME/profiles/<name>` with a `package.json` manifest and `cordis.patch.yml`; storage hub with JSON or SQLite backends; `$DSH_HOME` default `~/.dsh`. | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/settings, https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/storage | unreviewed |
| Oh My Pi | YAML `~/.omp/agent/config.yml`, project `<cwd>/.omp/config.yml` (launch dir only), `PI_CONFIG_FILES`/`--config` overlays; XDG support; Nix flake with home-manager/NixOS modules. | GA | https://omp.sh/docs/settings, https://omp.sh/docs/env | unreviewed |
| Pi | Agent dir `~/.pi/agent` (settings, keybindings, mcp.json, models.json, auth.json, trust.json, extensions, skills, prompts, themes, SYSTEM/APPEND_SYSTEM); project `.pi/` needs trust; published JSON schemas for settings and themes. | GA | https://pi.dev/docs/latest/configuration, https://pi.dev/docs/latest/settings | unreviewed |
| Cursor | Many dotfiles: `.cursor/rules/*.mdc`, `AGENTS.md`, `.cursor/mcp.json`, `hooks.json`, `agents/`, `skills/`, `worktrees.json`, `environment.json`, `permissions.json`, `sandbox.json`, `cli.json`, `.cursorignore`, Bugbot files. CLI project config carries only permissions. | GA | https://cursor.com/docs/rules, https://cursor.com/docs/hooks, https://cursor.com/docs/cli/reference/configuration | unreviewed |
| Antigravity | CLI `~/.gemini/antigravity-cli/settings.json` (only non-default values, 0600, unknown keys preserved, `$schema`), `keybindings.json`; shared `.agents/` and `~/.gemini/config/` trees (rules, skills, agents, plugins, `hooks.json`, `mcp_config.json`); repo `.gemini/config.json` with `personal_customization_dir`. | GA | https://antigravity.google/docs/settings, https://antigravity.google/docs/customization-discovery, https://antigravity.google/docs/cli/using | unreviewed |
| OpenHands | Canvas Settings sections per backend (Agent profiles, LLM, Condenser, Runtime, Verification, Application, Secrets, Agent Context). Agent Server config file formats beyond `OH_*` env vars: nothing documented in pages opened. | GA | D:/openhands/usage/agent-canvas/customize-and-settings.md (D = https://docs.openhands.dev) | unreviewed |
| OpenClaw | `~/.openclaw/openclaw.json` (JSON5), schema-validated (unknown keys stop the Gateway), editable by CLI, Control UI form/raw or Config RPC; read-only modes (`OPENCLAW_CONFIG_READONLY`, Nix). | GA | https://docs.openclaw.ai/gateway/configuration, https://docs.openclaw.ai/web/control-ui | unreviewed |
| goose | `~/.config/goose/config.yaml` plus `permission.yaml`, `secrets.yaml`, `permissions/tool_permissions.json`, `prompts/`, `adversary.md`, `settings.json`; project-level `<project>/.config/goose/settings.json`; `goose configure` is an interactive editor. | GA | https://goose-docs.ai/docs/guides/config-files.md | unreviewed |
| Hermes Agent | `~/.hermes/` (or `HERMES_HOME`): `config.yaml`, `.env`, `auth.json`, `SOUL.md`, `memories/`, `skills/`, `sessions/`, `cron/`, `hooks/`, `plugins/`, `state.db`, `checkpoints/`; config migrations run on update. | GA | /user-guide/docker, /user-guide/profiles, /user-guide/managed-scope | unreviewed |
| Zed/ACP | Zed: JSON `settings.json` with `agent.*` keys, Settings Editor AI section, per-project settings. ACP: protocol, no config file of its own in these lines. | GA | https://zed.dev/docs/ai/quick-start.md, https://zed.dev/docs/ai/agent-settings.md | unreviewed |
| Warp | CLI `settings.toml` (per-OS path); app uses a settings UI; MCP in `~/.warp/.mcp.json`; routers in `~/.warp/custom_model_routers/`. | GA | https://docs.warp.dev/agents/cli/configuration/ | unreviewed |
| Cline | Global and project scopes; CLI `cline config [workflows\|rules\|skills\|agents\|plugins\|hooks\|mcp\|tools]`; `global-settings.json` and `providers.json` under `~/.cline/data/settings/`. | GA | https://docs.cline.bot/getting-started/config.md, https://docs.cline.bot/cli/cli-reference.md | unreviewed |
| rizzy-harness (ours) | Data dir with `config.toml` (providers, models, agent models, chat, web, memory, voice, routines, storage, workspaces, nodes), `node.toml`, `secrets.toml` (0600); mostly editable from Settings. | works | `crates/master/src/config.rs:1-420`, `crates/cli/src/main.rs:130-142`, `README.md:295-510` | from own inventory |

#### Precedence between user, project, managed and command-line settings
What it is: which layer wins when the same key is set in several places.
Variants: the real difference is **who sits on top**. Codex (`requirements.toml`), Claude Code (managed, with strict-value-wins ratchets) and OpenCode (system layer plus `policies`) put an admin layer above everything. Hermes lets a managed scope beat user and env only for pinned keys. OMP, Pi, goose and Claude Code treat env vars differently from file layers (Claude Code decides per key; goose puts env first). Cline's own pages disagree on whether project or global wins. Warp, Zed, OpenHands and OpenClaw: no precedence document found.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Highest to lowest: CLI flags and `-c`; project `.codex/config.toml` files root to cwd (trusted only); profile file; user file; cloud-managed defaults; `/etc/codex/config.toml`; built-ins. `requirements.toml` constrains all of them. | GA | https://learn.chatgpt.com/docs/config-file/config-basic.md | unreviewed |
| OpenCodex | Values in file override defaults; `HTTP(S)_PROXY` env beats the `proxy` field. | GA | https://opencodex.me/reference/configuration/ | unreviewed |
| OpenCode | Low to high: global, project files (farther first), `.opencode/` files, system config dirs, macOS managed preferences. Settings merge; `policies` evaluate in reverse (broader wins). | GA | https://opencode.ai/docs/config/, https://opencode.ai/docs/migrate-v1/ | reviewed |
| Claude Code | Highest first: managed, `--settings` (inline JSON or file, one session), project local, shared project, user. List keys concatenate across files (exceptions listed in the survey). Dedicated flags and env vars are outside the stack and decided per key (`ANTHROPIC_MODEL` beats any `model` key). | GA | https://code.claude.com/docs/en/settings.md, https://code.claude.com/docs/en/env-vars.md | reviewed |
| DeepSeek harness | Patch layers: each bundle's patch, profile patch, `$DSH_HOME/cordis.patch.yml`, then `--patch` overlays. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/settings | unreviewed |
| Oh My Pi | Flags, then `PI_CONFIG_FILES` overlays, project file, global or named profile. Maps deep-merge; scalars and arrays replace. A named profile replaces rather than inherits. | GA | https://omp.sh/docs/settings, https://omp.sh/docs/env | unreviewed |
| Pi | Project settings override user settings; resource lists combine; system-prompt files do not merge; some settings are agent-dir only (`defaultProjectTrust`, `httpProxy`, `selfUpdateCommand`, `cacheWarming`). | GA | https://pi.dev/docs/latest/configuration, https://pi.dev/docs/latest/settings | unreviewed |
| Cursor | Only hooks have a documented merge: enterprise, team, project, user, plugin sources; for permissions deny beats ask beats allow regardless of source, other fields last response wins (corrected: not "most restrictive wins"). | GA | https://cursor.com/docs/hooks | corrected |
| Antigravity | Permission scopes combine Global + Project + Shared; the precedence tables live in its areas 5 and 7 (not read here). | GA | https://antigravity.google/docs/settings, https://antigravity.google/docs/customization-discovery | unreviewed |
| goose | Env vars, then config file, then defaults; project-level `settings.json`. Edits usually need a restart. | GA | https://goose-docs.ai/docs/guides/config-files.md | unreviewed |
| Hermes Agent | Env vars override `.env`; with managed scope, managed beats user beats env for pinned keys. | GA | /user-guide/managed-scope, /user-guide/docker | unreviewed |
| Warp | No precedence document across app, CLI, admin and project levels beyond rules precedence and "admin enforced overrides user". | GA | https://docs.warp.dev/agents/cli/configuration/ | unreviewed |
| Cline | Undefined on the config page; skills say global wins, rules say workspace wins. | GA | https://docs.cline.bot/getting-started/config.md | unreviewed |
| Zed/ACP | User versus project precedence for agent keys: nothing documented in pages read. | GA | https://zed.dev/docs/ai/agent-settings.md | unreviewed |
| Claude Desktop, OpenHands, OpenClaw | Nothing documented in the lines read (OpenHands settings are per backend). | - | - | - |
| rizzy-harness (ours) | `.env` and `HARNESS_*` override `config.toml` (read-only in the UI); `node.toml` is the node's only authority, and approval mode is `min(requested, node max)`. | works | `crates/master/src/env.rs:1-20`, `crates/core/src/policy.rs:19-23` | from own inventory |

#### Managed (admin) policy layer and how it is delivered
What it is: a configuration source an administrator controls that users and repos cannot loosen, and how it reaches the machine.
Variants: pushed from a vendor service (Codex cloud-managed defaults, Claude Code server-managed settings polled hourly, OpenCode Console policies), a customer-hosted config server (Codex `[config_server]`, goose allowlist URL, Claude apps gateway), or OS channels (MDM, registry, files in system dirs). Coverage is uneven: Cline's remote config reaches IDE extensions but not Desktop; Claude Code's own docs say server-managed settings are not a security boundary.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `requirements.toml` outranks all layers; cloud-managed defaults; `[config_server] base_url` makes CLI and desktop fetch defaults and requirements from an organisation-run HTTPS server, optionally behind OAuth; `forced_login_method`. | GA | https://learn.chatgpt.com/docs/config-file/config-basic.md, https://learn.chatgpt.com/docs/enterprise/configuration-server.md | unreviewed |
| OpenCode | System config dirs (`/etc/opencode`, `/Library/Application Support/opencode`, `%ProgramData%\opencode`), macOS managed preferences, `policies` layer that only tightens, Console-delivered policies; the system dir also loads agents, commands, skills and plugins for all users. | GA | https://opencode.ai/docs/config/, https://opencode.ai/docs/policies/ | corrected |
| Claude Code | `managed-settings.json` at an OS path, MDM (about every 30 min), server-managed settings (polled hourly, `~/.claude/remote-settings.json`), `allowManaged*Only` locks, parent settings from embedding hosts; client-side control, not a security boundary. `gateway.yaml` of the Claude apps gateway serves managed settings per IdP group. | GA | https://code.claude.com/docs/en/settings.md, https://code.claude.com/docs/en/claude-apps-gateway.md, https://code.claude.com/docs/en/claude-apps-gateway-config.md | reviewed |
| Claude Desktop | 3P edition: admin console controls, bootstrap-server config delivery; "server-managed vs file" precedence confusion noted in the survey's relevance section. | GA | https://claude.com/docs/third-party/claude-desktop/overview.md | corrected |
| Cline | Remote config: VS Code/JetBrains get rules, workflows, skills, allowed providers, MCP controls, telemetry lock; CLI only rules and workflows; Desktop none. Written locally to `.cline/enterprise/` or `.cline/remote-config/`. | GA | https://docs.cline.bot/enterprise-solutions/configuration/remote-configuration/overview.md | reviewed |
| Hermes Agent | Managed scope for fleet baselines, remote config plane, per-scope admins, git-installable profile distributions (never credentials or memory). | GA | /user-guide/managed-scope, /user-guide/profiles | unreviewed |
| Cursor | Team and enterprise hooks, applied on self-hosted workers on Enterprise. | GA | https://cursor.com/docs/hooks | unreviewed |
| Warp | "Admin enforced overrides user"; admin denylist non-bypassable; Enterprise can enforce secret redaction. | GA | https://docs.warp.dev/knowledge-and-collaboration/admin-panel/ | unreviewed |
| goose | Extension allowlist with exact command match, delivered by a URL the admin hosts. | GA | https://goose-docs.ai/docs/guides/allowlist.md | reviewed |
| rizzy-harness (ours) | `node.toml` on the node is the authority; nothing received over the protocol or HTTP widens it (no separate admin-console channel in the inventory). | works | `crates/core/src/policy.rs:19-23` | from own inventory |

#### Repo-supplied config must not widen policy (workspace trust for config)
What it is: rules that stop a cloned repository from changing credentials, providers, hooks or permissions through its own config files.
Variants: Codex blocks named keys in project config; Claude Code gates keys behind a trust dialog and ignores some env vars from repo files; Pi has a trust gate for all project resources; Cursor's Bugbot reads config from the base branch; Antigravity has trusted workspaces. OMP loads repo content automatically and treats it as trusted code.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Project `.codex/config.toml` cannot set `openai_base_url`, `chatgpt_base_url`, `model_provider(s)`, `notify`, `profile(s)`, `otel`, the realtime base URL, or `apps_mcp_product_sku`; Codex warns when it ignores them. Protected subpaths (`.git`, `.agents`, `.codex`, `.aws`) inside writable roots. | GA | https://learn.chatgpt.com/docs/config-file/config-advanced.md | corrected |
| Claude Code | From a committed project file, `permissions.allow`, `additionalDirectories`, marketplaces, project `apiKeyHelper` and most `env` values wait for folder trust (`deny` and `ask` apply at once); user/managed-scoped keys never apply from a repo; config-dir, OTel exporter and session-export env vars ignored from repo files; one-way ratchets for security keys. `-p` and SDK sessions treat any folder as trusted. | GA | https://code.claude.com/docs/en/settings.md, https://code.claude.com/docs/en/settings-reference.md | unreviewed |
| Pi | Trust decision gates `.pi` settings, MCP config, extensions, skills, prompts, themes, system-prompt files; order: CLI override, extension, saved decision, default `ask`; headless skips protected resources; "honest about being incomplete". | GA | https://pi.dev/docs/latest/security | reviewed |
| Cursor | Bugbot reads its config from the base branch so a PR cannot loosen its own review; hooks from project need trust. | GA | https://cursor.com/docs/bugbot | reviewed |
| Antigravity | Workspace trust (`trustedWorkspaces` semantics on the settings page). | GA | https://antigravity.google/docs/settings | corrected |
| Oh My Pi | Repo content auto-loaded; MCP, plugins, hooks, skills and custom tools all treated as trusted code. | GA | https://omp.sh/docs/plugins, https://omp.sh/docs/mcp | unreviewed |
| Hermes Agent | One project context file loads; flagged project files are blocked; a flagged `SOUL.md` in home loads with a warning. | GA | /user-guide/features/context-files | unreviewed |
| rizzy-harness (ours) | No repo-supplied config is read at all (no `AGENTS.md`/`CLAUDE.md` reader); per-project instructions are text typed into the Projects UI. | none | `crates/master/src/projects.rs:1-12` | from own inventory |

#### Profiles, presets and recipes
What it is: named bundles of settings that can be switched or shared.
Variants: file overlays (Codex), isolated homes (OMP profile replaces config; Hermes profile is a separate home), manifest-plus-patch composition (DSH), per-backend agent profiles that also scope secrets (OpenHands), permission profiles (Warp), task recipes with typed parameters (goose).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `--profile name` overlays `~/.codex/name.config.toml`; since 0.134.0 `[profiles.x]` tables and the `profile =` selector are no longer read. | GA | https://learn.chatgpt.com/docs/config-file/config-advanced.md | unreviewed |
| DeepSeek harness | Profiles with ordered `bundles`; shipped `web`, `headless`, `sdk`, `sdk-minimal`, `acp`; `--from-default-profile`; `dsh rescue`. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/settings | unreviewed |
| Oh My Pi | Named profiles isolate settings, auth, caches and sessions; a profile replaces rather than inherits. | GA | https://omp.sh/docs/settings | unreviewed |
| OpenHands | Agent profiles per backend; new profiles copy from `default`; profiles scope secrets (all, none, selected; enforced by the server). | GA | D:/openhands/usage/agent-canvas/agent-profiles.md | unreviewed |
| goose | Recipes: YAML/JSON with typed `parameters`, Jinja templating, `extensions`, `settings` (provider, model, temperature, max_turns), `response.json_schema`, `retry`, `sub_recipes` (experimental); trust prompt on first desktop run; shareable deeplinks. | GA | https://goose-docs.ai/docs/guides/recipes/recipe-reference.md | reviewed |
| Hermes Agent | A profile is a separate Hermes home (config, `.env`, SOUL, memories, sessions, skills, cron, gateway); does not sandbox the agent; git-installable profile distributions. | GA | /user-guide/profiles | unreviewed |
| Warp | Agent profiles with regex allow and deny lists; admin-enforced denylist. | GA | https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/ | corrected |
| rizzy-harness (ours) | No profiles. Closest: custom agents (markdown files in `<data>/agents/`), provider presets, and memory-embedding presets fast/balanced/best. | partial | `crates/master/src/agent_edits.rs:1-12`, `crates/master/src/presets.rs`, `crates/memory/src/embed.rs:12-40` | from own inventory |

#### Environment variables
What it is: process-level settings that override or supply what files hold.
Variants: Claude Code has about 400 variables with a per-key precedence rule and unusual boolean parsing (some accept any non-empty value as true); goose puts env above the file; Hermes puts env above `.env`; most others use env for homes, proxies, telemetry and credentials only.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | About 400 variables in families (auth, provider selection, model mapping, network, tools and limits, context, about 60 `CLAUDE_CODE_DISABLE_*`, telemetry, updates); booleans accept 1/true/yes/on; `env` block in settings copies verbatim into subprocesses; some variables ignored from every file and read only from the launch environment. | GA | https://code.claude.com/docs/en/env-vars.md, https://code.claude.com/docs/en/settings-reference.md | unreviewed |
| Codex | Environment variables page exists; `CODEX_API_KEY`, `CODEX_ACCESS_TOKEN`, OIDC/SPIFFE variables named elsewhere; not read in full. | GA | https://learn.chatgpt.com/docs/config-file/environment-variables.md | unreviewed |
| OpenCodex | `OPENCODEX_HOME`, `HTTP(S)_PROXY`, `OCX_TELEMETRY`, `DO_NOT_TRACK`, `OPENCODEX_API_AUTH_TOKEN`. | GA | https://opencodex.me/reference/configuration/ | unreviewed |
| OpenCode | `HTTP_PROXY/HTTPS_PROXY/NO_PROXY` (must include localhost), `NODE_EXTRA_CA_CERTS`, `OPENCODE_PASSWORD`, `OPENCODE_DISABLE_FREE_MODELS`; `opencode service set env` persists variables for the background service. | GA | https://opencode.ai/docs/network/ | unreviewed |
| DeepSeek harness | `DSH_HOME`, `DSH_PERMISSION_MODE`, `DSH_WEB_URL`, proxy variables, `DSH_TELEMETRY_*`, `DSH_GITHUB_*`, `DSH_BUNDLED_SKILL_DIR`; credentials from env or `.env`; list not exhaustive. | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/reference/README.md | unreviewed |
| Oh My Pi | `PI_CONFIG_FILES`, `PI_SUBPROCESS_CMD` and others on the env page. | GA | https://omp.sh/docs/env | unreviewed |
| Pi | `PI_TELEMETRY`, `PI_OFFLINE`, `PI_SKIP_VERSION_CHECK`, `PI_SELF_UPDATE_COMMAND`, `PI_CODING_AGENT_DIR`, `PI_CODING_AGENT_SESSION_DIR`, `PI_PACKAGE_DIR`, proxies, provider keys. | GA | https://pi.dev/docs/latest/environment-variables | unreviewed |
| OpenHands | `OH_*` variables for the Agent Server; `OH_SECRET_KEY`. | GA | D:/openhands/usage/agent-canvas/customize-and-settings.md | unreviewed |
| OpenClaw | State dir overridable via env vars; `OPENCLAW_CONFIG_READONLY`, `OPENCLAW_NO_AUTO_UPDATE`, `OPENCLAW_TELEMETRY_ENDPOINT`, `DO_NOT_TRACK`. | GA | https://docs.openclaw.ai/gateway/configuration, https://docs.openclaw.ai/gateway/telemetry | unreviewed |
| goose | `GOOSE_*` keys at root of config; env beats file; `GOOSE_TELEMETRY_ENABLED` default false; `GOOSE_RECIPE_PATH`, `GOOSE_DOCS_ROOT`. | GA | https://goose-docs.ai/docs/guides/environment-variables.md, https://goose-docs.ai/docs/guides/config-files.md | unreviewed |
| Hermes Agent | `HERMES_HOME`; env overrides `.env`. | GA | /user-guide/docker | unreviewed |
| Warp | `WARP_API_KEY`, `WARP_TUI_DISABLE_AUTOUPDATE`, `WARP_SKILL_DIRS`. | GA | https://docs.warp.dev/agents/cli/configuration/ | unreviewed |
| rizzy-harness (ours) | `.env` file and `HARNESS_*` variables override `config.toml`; `.env` names are hidden from tools; env-var names are the way to reference provider keys. | works | `crates/master/src/env.rs:1-20`, `docker/env.reference`, `crates/master/src/secrets.rs:1-15` | from own inventory |

#### Themes
What it is: user-changeable colour schemes.
Variants: JSON files with hot reload (Claude Code, Pi), YAML and image-based (Warp), presets only or page-not-read (Codex, OpenCode, OMP, Antigravity, Hermes skins).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/theme` presets (auto light/dark, daltonized, ANSI) plus custom JSON in `~/.claude/themes/<slug>.json` (`name`, `base`, `overrides` of named tokens, including eight subagent colours); directory watched and reloaded live; plugins can contribute themes. | GA | https://code.claude.com/docs/en/terminal-config.md | unreviewed |
| Pi | JSON theme files (`name`, `colors`, `vars`, `appearance`, `export`); hex, OKLCH, OKHSL, 256-index or variable references; built-ins `system`, `dark`, `light`; light/dark pairs; hot reload of the active user theme. | GA | https://pi.dev/docs/latest/themes | unreviewed |
| Codex | Themes listed among TUI options (`tui.*`). | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| OpenCode | Themes page exists (not read); GUI plugin part can contribute themes. | GA | https://opencode.ai/docs/themes/ | unreviewed |
| Oh My Pi | Themes (default dark `titanium`); theme page not opened. | unclear | https://omp.sh/docs/using, https://omp.sh/sitemap.xml | unreviewed |
| Antigravity | TUI themes. | GA | https://antigravity.google/docs/features | unreviewed |
| Hermes Agent | CLI skins, desktop themes, animated "pet" mascot. | GA | /docs/llms.txt, /user-guide/features/personality | unreviewed |
| Warp | Themes in YAML, image-based themes, prompt chips (terminal appearance, "not agent related"). | GA | https://docs.warp.dev/terminal/appearance/ | unreviewed |
| rizzy-harness (ours) | Light/dark/system, accents, localisation catalogs, home backgrounds (web UI). | works | `web/src/lib/appearance.ts`, `web/src/lib/i18n.ts`, `web/src/bg/` | from own inventory |

#### Key bindings
What it is: user-remappable keyboard shortcuts.
Variants: Claude Code has a full contexts-and-actions file with chords and validation; Codex and OpenCode have a leader key (Ctrl+X); Pi and Antigravity have `keybindings.json`; Warp documents Vim keybindings only.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `~/.claude/keybindings.json` (hot-reloaded): about 30 contexts, `namespace:action` names, `command:<name>` binds any slash command; chords need each key within 3 s; reserved keys (Ctrl+C, Ctrl+D, ...); validation warnings go to the debug log. | GA | https://code.claude.com/docs/en/keybindings.md | unreviewed |
| Codex | Configurable keymap with a leader key (Ctrl+X) and Vim mode. | GA | https://learn.chatgpt.com/docs/developer-commands.md, https://learn.chatgpt.com/docs/config-file/config-reference.md | unreviewed |
| OpenCode | Leader key Ctrl+X, Ctrl+P palette; keybinds page exists. | GA | https://opencode.ai/docs/cli/tui/, https://opencode.ai/docs/cli/keybinds/ | unreviewed |
| Pi | Customised in `keybindings.json`; `/hotkeys`. | GA | https://pi.dev/docs/latest/keybindings | unreviewed |
| Antigravity | `keybindings.json`, Vim mode. | GA | https://antigravity.google/docs/cli/using | unreviewed |
| Oh My Pi | `/hotkeys`; keybindings page listed in docs index, not opened. | unclear | https://omp.sh/sitemap.xml | unverified |
| Warp | Vim keybindings in the editor and CLI; no remap file documented. | GA | https://docs.warp.dev/code/code-editor/ (via `code.txt`) | unreviewed |
| Zed/ACP | Keybindings mentioned in the configuration entry; no detail. | GA | https://zed.dev/docs/ai/agent-settings.md | unreviewed |
| rizzy-harness (ours) | Not in the inventory. | none | not in the inventory | from own inventory |

#### Output styles, personas and system-prompt overrides
What it is: persistent changes to how the agent speaks or behaves, separate from per-project instructions.
Variants: Claude Code output styles (switchable, mid-session, plugin-shippable); a global identity file (Hermes `SOUL.md` with 14 presets, OpenClaw `SOUL.md`/`IDENTITY.md`); prompt-section composition (DSH persona slots); Codex instruction overrides (replace or add, admin can add up to 10,000 tokens); Pi `SYSTEM`/`APPEND_SYSTEM` files.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Output styles: built-ins Default, Proactive, Concise, Explanatory, Learning; custom markdown in `~/.claude/output-styles`, `.claude/output-styles`, managed dir or plugin; frontmatter `keep-coding-instructions` (default false drops the built-in software-engineering section); applies to main conversation and forks only; mid-session switch keeps the cache. | GA | https://code.claude.com/docs/en/output-styles.md | unreviewed |
| Codex | `model_instructions_file` replaces built-in instructions; `developer_instructions` adds; managed `additional_developer_instructions` up to 10,000 estimated tokens; custom prompts (`~/.codex/prompts/`) deprecated in favour of skills. | GA | https://learn.chatgpt.com/docs/config-file/config-reference.md, https://learn.chatgpt.com/docs/custom-prompts.md | unreviewed |
| DeepSeek harness | Ordered named prompt sections with persona prefix and suffix slots and scoped overrides; one `complete` section can replace the whole prompt. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/system-prompt | unreviewed |
| Pi | `SYSTEM` and `APPEND_SYSTEM` files in the agent dir or project; system-prompt files do not merge. | GA | https://pi.dev/docs/latest/configuration | unreviewed |
| OpenHands | System prompt options: default, default plus extra instructions, custom persona (memory, security and tool guidance, skills still included). | GA | D:/openhands/usage/agent-canvas/customize-and-settings.md | unreviewed |
| OpenClaw | `AGENTS.md` (rules), `SOUL.md` (persona), `IDENTITY.md`, `USER.md`, `BOOT.md`, `BOOTSTRAP.md` injected into the system prompt; sub-agents get only `AGENTS.md`. | GA | https://docs.openclaw.ai/concepts/agent-workspace, https://docs.openclaw.ai/concepts/system-prompt | unreviewed |
| Hermes Agent | `SOUL.md` in `HERMES_HOME` is the global identity (slot 1 of the prompt); 14 built-in personality presets overlay it. | GA | /user-guide/features/personality, /user-guide/features/context-files | unreviewed |
| Cline | `--system` flag overrides the prompt for a CLI run. | GA | https://docs.cline.bot/cli/cli-reference.md | unreviewed |
| Antigravity | Python SDK supports personas. | GA | https://antigravity.google/llms.txt (sdk personas page listed, not opened) | unverified |
| rizzy-harness (ours) | Custom agents with per-agent extra instructions and avatars; project instructions up to 4000 characters. No global persona file. | partial | `crates/master/src/agent_edits.rs:1-12`, `crates/master/src/projects.rs:1-12` | from own inventory |

#### Status lines and footers
What it is: a configurable line of live session data in the UI.
Variants: Claude Code runs a user command with session JSON on stdin (and a separate subagent status line); Antigravity feeds live metadata to status-line and title scripts; Codex has `/statusline` and `/title`; Warp's CLI has a configurable statusline; OMP and Pi have fixed footers.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `statusLine: {type: "command", command, padding, refreshInterval, hideVimModeIndicator}`; script gets JSON (model, context, cost, rate limits, cache fields); debounced 300 ms; `/statusline <description>` generates the script; `subagentStatusLine` rewrites per-row text; project-set status line follows the workspace-trust rule; managed hooks-only mode narrows it. | GA | https://code.claude.com/docs/en/statusline.md | unreviewed |
| Codex | `/statusline` and `/title` customisation in the TUI. | GA | https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| Antigravity | TUI status line and title scripts fed live metadata. | GA | https://antigravity.google/docs/features, https://antigravity.google/docs/cli/using | unreviewed |
| Warp | CLI has a configurable statusline (model and usage). | GA | https://docs.warp.dev/agents/cli/ | unreviewed |
| Oh My Pi | Status line shows model, thinking, mode, path, git, context, cost, session; configurability not stated. | GA | https://omp.sh/docs/using | unreviewed |
| Pi | Footer shows folder, session, model, context usage and cost. | GA | https://pi.dev/docs/latest/usage | unreviewed |
| rizzy-harness (ours) | Not in the inventory (web UI; usage is tokens per turn). | none | not in the inventory | from own inventory |

#### Hot reload and invalid-file handling
What it is: what happens when a config file changes at runtime or is malformed.
Variants: stop on unknown keys (OpenClaw), rename the bad file and load defaults (OpenCodex, OMP stops after setting it aside), fall back per value (Warp), skip bad entries with a dialog (Claude Code), restart required (goose).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Files watched; most edits apply live; startup-only keys listed (`model`, OTel exporter, `forceRemoteSettingsRefresh`, ...); invalid file gives a "Settings Error" dialog (fix, exit or continue), bad entries skipped; `-p` skips broken files silently; corrupt `~/.claude.json` backed up. | GA | https://code.claude.com/docs/en/settings.md | unreviewed |
| Oh My Pi | Invalid YAML moved aside as `*.broken-*`, startup stops. | GA | https://omp.sh/docs/settings | unreviewed |
| OpenCodex | Invalid file renamed `config.json.invalid-<timestamp>` and defaults load; mutating commands refuse an invalid file; live saves can overwrite hand edits. | GA | https://opencodex.me/reference/configuration/ | unreviewed |
| OpenClaw | Schema-validated; unknown keys stop the Gateway; hot reload; guard against accidental data-loss writes; no automatic last-known-good restore except `doctor --fix`; secrets resolution falls back to last-known-good. | GA | https://docs.openclaw.ai/gateway/configuration, https://docs.openclaw.ai/gateway/secrets | unreviewed |
| Warp | Most values hot-reload; invalid values fall back to defaults. | GA | https://docs.warp.dev/agents/cli/configuration/ | unreviewed |
| Pi | `/reload` re-reads config. | GA | https://pi.dev/docs/latest/configuration | unreviewed |
| Antigravity | Customisation files re-read every turn; unknown settings keys preserved. | GA | https://antigravity.google/docs/settings | unreviewed |
| goose | Edits usually need a restart. | GA | https://goose-docs.ai/docs/guides/config-files.md | unreviewed |
| rizzy-harness (ours) | Provider config hot-swappable without restart; invalid-file behaviour not in the inventory (`doctor` checks that config and `node.toml` parse). | partial | `crates/agent-native/src/llm.rs:326-380`, `crates/cli/src/main.rs:557-632` | from own inventory |

#### Programmatic config editing
What it is: changing settings without hand-editing the file.
Variants: atomic batch writes over an API (Codex), optimistic revisions (DSH), RPC plus UI (OpenClaw), settable from chat or the prompt (Claude Code `/config key=value`, Warp "ask the agent").

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `config/read`, `config/value/write`, `config/batchWrite` (atomic) over the app-server. | GA | https://learn.chatgpt.com/docs/app-server.md | unreviewed |
| Claude Code | `/config key=value` (also in `-p` and over Remote Control, 2.1.181); SDK `applyFlagSettings()` and `updateSettings()`; `--safe-mode`. | GA | https://code.claude.com/docs/en/whats-new/2026-w23.md, https://code.claude.com/docs/en/agent-sdk/typescript.md | unreviewed |
| DeepSeek harness | Settings UI edits go through the same patch files with optimistic revisions; secret fields redacted on remote reads. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/settings | unreviewed |
| OpenClaw | CLI, Control UI form/raw, Config RPC. | GA | https://docs.openclaw.ai/gateway/configuration | unreviewed |
| OpenCodex | Dashboard, CLI (`ocx provider\|models\|combo\|agent\|config`), management API with preview and plan fingerprints. | GA | https://opencodex.me/reference/configuration/, https://opencodex.me/reference/management-api/ | reviewed |
| Oh My Pi | `omp config list\|get\|set\|reset` (global only). | GA | https://omp.sh/docs/settings | unreviewed |
| Warp | Change by file, slash command, or by asking the agent. | GA | https://docs.warp.dev/agents/cli/configuration/ | unreviewed |
| Cline | `cline config` views and edits scopes. | GA | https://docs.cline.bot/cli/cli-reference.md | unreviewed |
| rizzy-harness (ours) | Settings UI writes `config.toml` ("mostly editable from Settings"); `.env`-sourced values read-only in the UI. | works | `crates/master/src/config.rs:1-420`, `crates/master/src/env.rs:1-20` | from own inventory |

#### Reading other harnesses' configuration and instruction files
What it is: loading `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, skills directories or MCP configs written for another tool, or importing their settings.
Variants: one-shot import commands (Claude Code, Codex), live multi-format reading (OMP, Warp, Cursor, goose, Hermes, Zed), management of other tools' config blocks (OpenCodex). OpenCode V2 reads only `AGENTS.md`.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `/import [codex\|gemini\|cursor] [--dry-run] [--yes]` and `claude import` bring instruction files, MCP servers, commands, subagents, skills; unavailable on third-party providers or with flag fetching off. | GA | https://code.claude.com/docs/en/commands.md, https://code.claude.com/docs/en/cli-reference.md | unreviewed |
| Codex | `/import` reads Claude Code and Cursor setups and chats (50-chat cap is CLI-only); sets `CLAUDE_PLUGIN_ROOT` for plugin hooks. | GA | https://learn.chatgpt.com/docs/import.md | corrected |
| Oh My Pi | Reads AGENTS/CLAUDE/GEMINI/Copilot/OpenCode instruction files, skills dirs, MCP configs, `.claude-plugin` marketplaces, Cursor/Cline rules; imports Claude/Codex history. | GA | https://omp.sh/docs/context-files | corrected |
| Cline | Reads `.cursorrules`, `.windsurfrules`, `AGENTS.md`, `.claude/skills/`, `.agents/skills/`; Desktop imports Claude Code and Codex history. | GA | https://docs.cline.bot/customization/cline-rules.md, https://docs.cline.bot/usage/cline-desktop.md | unreviewed |
| Warp | Skills read from ten tools' directories; migration guides from Claude Code, Cursor and others; `AGENTS.md` default rules file. | GA | https://docs.warp.dev/agents/capabilities/, https://docs.warp.dev/getting-started/migrate-to-warp/ | reviewed |
| Cursor | Loads `.claude/` and `.codex/` agents, skills and Claude Code hooks. | GA | https://cursor.com/docs/skills, https://cursor.com/docs/hooks | unreviewed |
| goose | `CONTEXT_FILE_NAMES` is configurable (so `CLAUDE.md` or `.cursorrules` can be read); skills and agents read `.claude/` paths; accepts Gemini extension manifests. | GA | https://goose-docs.ai/docs/guides/context-engineering/using-goosehints.md, https://goose-docs.ai/docs/guides/context-engineering/plugins.md | unreviewed |
| Hermes Agent | First match of `.hermes.md`/`HERMES.md`, `AGENTS.override.md`, `AGENTS.md`, `CLAUDE.md`, `.cursorrules`. | GA | /user-guide/features/context-files | unreviewed |
| Zed/ACP | Personal `AGENTS.md` plus first match of nine project files (`.rules`, `.cursorrules`, `.windsurfrules`, `.clinerules`, `.github/copilot-instructions.md`, `AGENT.md`, `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`). | GA | https://zed.dev/docs/ai/instructions.md | corrected |
| OpenCodex | Exports or manages config for 28 client tools, editing only its own block, idempotent but "not byte for byte" on re-enable; never overwrites a user-owned `openai_base_url`. | GA | https://opencodex.me/reference/client-support/ | corrected |
| Antigravity | Backward compatibility for `GEMINI.md`, `~/.gemini/skills/` and MCP config from Gemini CLI (inline MCP must move to `mcp_config.json`). | GA | https://antigravity.google/docs/customization-discovery (migration page not given) | unreviewed |
| OpenCode | V2 supports only `AGENTS.md`; the `CLAUDE.md` fallback is not supported. | GA | https://opencode.ai/docs/migrate-v1/ | reviewed |
| rizzy-harness (ours) | Truly absent: the agent does not read `AGENTS.md`/`CLAUDE.md` or any repo instruction file; no import. | none | `crates/master/src/projects.rs:1-12` | from own inventory |

(The Antigravity row cites the discovery page, not the migration page, because the survey names no URL for the migration doc beyond its path `D/cli/gcli-migration`.)

#### Diagnosis and recovery commands
What it is: commands that check, repair or isolate a broken setup.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `claude doctor`; `claude --safe-mode` starts with all customisations off (CLAUDE.md, skills, plugins, hooks, MCP, output styles, themes, keybindings, status line) while managed policy still applies. | GA | https://code.claude.com/docs/en/model-config.md, https://code.claude.com/docs/en/monitoring-usage.md | unreviewed |
| OpenClaw | `openclaw doctor` (`--fix`, lint, post-upgrade, SQLite maintenance, legacy migration), `openclaw triage` (read-only sanitised diagnosis), update rollback, `openclaw backup`. | GA | https://docs.openclaw.ai/start/getting-started, https://docs.openclaw.ai/llms.txt | unreviewed |
| DeepSeek harness | `dsh rescue`; `--from-default-profile` resets a profile. | preview | https://github.com/deepseek-ai/deepseek-harness/blob/master/apps/cli/README.md | unreviewed |
| rizzy-harness (ours) | `rizzy-harness doctor` checks bash/git present, config and `node.toml` parse, token file mode. | works | `crates/cli/src/main.rs:557-632` | from own inventory |

#### Feature flags and experimental toggles
What it is: switches that gate unfinished behaviour, with maturity labels.
Variants: user-visible maturity ladder (Codex), experimental config block (OpenCode), vendor-fetched flags (Claude Code, which fall back to compiled-in defaults when flag fetching is off), a project-published maturity scorecard (OpenClaw).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | `[features]` flags with labels (under development, experimental, beta, stable, deprecated); `/experimental` toggles; `codex features`. | GA | https://learn.chatgpt.com/docs/feature-maturity.md, https://learn.chatgpt.com/docs/config-file/config-basic.md | unreviewed |
| OpenCode | `experimental` keys: `todo`, `subagent_depth`, `background_compaction`, `image_generate.model`, `location_idle_timeout` (60 min default); changelog adds `schedules` and `artifacts`, absent from the config page. | experimental | https://opencode.ai/docs/config/, https://opencode.ai/changelog | reviewed |
| Claude Code | Behaviours gated by flags fetched from Anthropic; `CLAUDE_CODE_DISABLE_FEATURE_FLAGS` (2.1.319, changelog only) stops fetching; some features absent in telemetry-off sessions. | GA | https://code.claude.com/docs/en/whats-new/2026-w36.md, https://code.claude.com/docs/en/llm-gateway-rollout.md | unverified |
| OpenClaw | "Experimental features" page and a maturity scorecard with percentages. | GA | https://docs.openclaw.ai/maturity/scorecard | reviewed |
| rizzy-harness (ours) | None in the inventory. | none | not in the inventory | from own inventory |

### Area 22: Licence, pricing and distribution

#### Licence, open/closed split, install, pricing and governance (one table)
What it is: the legal and commercial shape of each harness. Governance and ownership moves use the reviewed versions (Cursor acquisition confirmed; goose governance move corrected; Pi move confirmed; OpenCode V2/V1 confusion corrected; Warp licence split confirmed; Warp Factories relabelled Early Access).
Variants: eight harnesses are MIT (OpenCodex, OpenCode, DeepSeek harness, OMP, Pi core, OpenHands, OpenClaw, Hermes), three Apache-2.0 (Codex, goose, Cline), one has an AGPL client (Warp), Zed is GPL/Apache per repo; four are closed products (Claude Code, Claude Desktop, Cursor, Antigravity) and two are open-core or open-client with closed services (OpenHands, Warp). Money comes from three models: a subscription that includes the harness (Codex, Claude, Cursor, Antigravity), a vendor-run model gateway or credits next to a free harness (OpenCode Console, Cline, Hermes Portal), or an enterprise tier that holds multi-user features (OpenHands, Warp self-hosting).

| Harness | Licence | What is open / what is closed | Install methods | Pricing model | Governance / ownership changes | Evidence | Check |
|---|---|---|---|---|---|---|---|
| Codex | Apache-2.0 (`openai/codex`) | Open: CLI, SDK, App Server, Codex Security CLI and SDK, skills and plugins repos, universal cloud image. Closed: IDE extension and Codex Cloud ("not open source"); desktop app not listed as open. No external pull requests accepted (issues only). | `install.sh`/`install.ps1` from `chatgpt.com/codex` (falls back to GitHub Releases), npm `@openai/codex`, Homebrew cask, static musl Linux binaries; `codex update`. | Included in ChatGPT plans (Free, Go, Plus, Pro, Business, Edu, Enterprise), shares usage and credits with ChatGPT Work; API-key use billed at API rates. | None recorded. Docs index is titled "Codex" but mixes ChatGPT/Work pages. | https://learn.chatgpt.com/docs/open-source.md, https://raw.githubusercontent.com/openai/codex/main/README.md, https://learn.chatgpt.com/docs/pricing.md | unreviewed |
| OpenCodex (lidge-jun) | MIT. (An unrelated `RyensX/OpenCodex` is AGPL-3.0.) | Open source; a community provider gateway, not affiliated with OpenAI or Anthropic. | npm `@bitkyc08/opencodex`, desktop installers (macOS notarized .dmg; Windows .msi not yet signed, SignPath rolling out; Linux AppImage/.deb), Docker Compose (README) though the remote-hub page says no official image is published, source. | Free; sponsors listed (OrcaRouter, TokenLab). | Default-on anonymous telemetry arrived in v2.100.0 (2026-11-10), after a consent popup; releases several per week. | https://github.com/lidge-jun/opencodex, https://opencodex.me/ | reviewed |
| OpenCode | MIT | Open source (CLI, desktop, plugins). Optional commercial "OpenCode Console" gateway is a separate service. | curl installer, Homebrew, npm, Bun, pnpm, Yarn, AUR, mise, Nix, standalone binaries incl. musl, desktop installers, Docker images; the V2 curl installer replaces the V1 binary. | Console: pay-as-you-go credits (4.6% + $0.31 purchase fee), Go $10/month, Go Plus $40/month, Go Teams $20/member; some free models (time-limited; some train on data), hidden with `OPENCODE_DISABLE_FREE_MODELS=1`. Prices are vendor pages that change often. | Maintained under `anomalyco` (formerly `sst`); footer "(c) 2026 Anomaly"; relation to `sst` not verified. V2 current per docs, but GitHub "latest" flagged v1.19.5; README lags V2; session sharing discontinued in v1.19.1 (corrected from "undocumented"). | https://opencode.ai/docs/, https://github.com/anomalyco/opencode, https://opencode.ai/docs/migrate-v1/, https://opencode.ai/docs/console/, https://opencode.ai/docs/console/go/, https://opencode.ai/changelog | corrected |
| Claude Code | Closed. `anthropics/claude-code` is public but "All rights reserved" (changelog, plugins, issues). Agent SDK under Commercial Terms; SDK package licences (Python MIT, TypeScript all rights reserved per `00-targets.md`) not reopened. | Closed binary. | Native installer (`curl ... install.sh \| bash`, PowerShell, CMD), Homebrew casks `claude-code` and `claude-code@latest`, WinGet, signed apt/dnf/apk repos, npm `@anthropic-ai/claude-code` (native binary via optional deps; npm install deprecation notice 2.1.15). | Requires Pro, Max, Ultra, Team, Enterprise, a Console account, a third-party cloud provider or a self-hosted apps gateway (no separate gateway fee). Ultra is the renamed Max 20x. Code Review is billed by tokens via usage credits. | SDK renamed `claude-code-sdk`/`@anthropic-ai/claude-code` to `claude-agent-sdk`; branding rule ("Claude Agent", not "Claude Code"). | https://code.claude.com/docs/en/legal-and-compliance.md, https://code.claude.com/docs/en/setup.md, https://code.claude.com/docs/en/agent-sdk/migration-guide.md, https://code.claude.com/docs/en/feature-availability.md | corrected |
| Claude Desktop | Closed, proprietary. | Closed; Code tab is a shell over the Claude Code binary. | DMG, MSIX, PKG, Microsoft Store, signed apt repository (Linux beta; no self-update, apt delivers updates). | Pro $17/month annual or $20 monthly, Max $100, Ultra $200, Team $20/seat (2-150 seats), Enterprise custom (Cowork product page; savings are vendor-claim). | Cowork "folded into Claude"; versioning looks non-monotonic. The "Windows build 10 19041" claim was moved to Unverified. | https://code.claude.com/docs/en/desktop-linux, https://claude.com/download, https://claude.com/product/cowork | corrected |
| DeepSeek harness | MIT (first npm release `0.0.1-rc.1` listed BSD-3-Clause; change not traced). | Open source, developer preview; only prerelease GitHub releases (newest `dsh-v0.2.2-rc.1`, 2026-11-04). | npm `@deepseek-ai/dsh` (`npx @deepseek-ai/dsh web`), PyPI `deepseek-harness-sdk`, desktop installers (macOS arm64 dmg, Windows x64 exe), source. | Harness free; model use needs a DeepSeek or other provider key; nothing else documented. | Created 2026-08-13; about 278k stars (says nothing about maturity). | https://github.com/deepseek-ai/deepseek-harness, https://deepseek.com/harness, https://registry.npmjs.org/@deepseek-ai/dsh | reviewed |
| Oh My Pi | MIT | Open source (TypeScript/Bun plus native Rust crates). | curl script (prebuilt binary with SHA-256 check, or bun), Homebrew tap, bun global (bun >=1.4.4 in README vs 1.3.14+ in SDK page), Nix flake, mise, PowerShell; `omp update` canary/stable. | None; bring your own model or subscription. | Maintainer can1357 / Stencil Labs, Inc.; PRs temporarily open to everyone. | https://github.com/can1357/oh-my-pi (README), https://omp.sh/docs/quickstart, https://omp.sh/docs/cli | unreviewed |
| Pi | MIT core ("MIT, forever" per author). Fair Source and proprietary tiers announced, not built. | Open core; name and logo trademarked by Earendil. | Installer, npm, Nix, standalone binaries built from release source; `pi update`. | Free; model costs are the user's; Radius gateway (early alpha) pricing not opened. | Moved from `badlogic/pi-mono` to `earendil-works/pi`; Earendil owns Pi (author post 2026-04-08); new-contributor issues and PRs auto-closed by default. pi-chat shows two licences on one page. | https://pi.dev/, https://github.com/earendil-works/pi, https://mariozechner.at/posts/2026-04-08-ive-sold-out/ | reviewed |
| Cursor | Closed source; no licence found on docs. | Closed. | Not itemised in area 22 (editor, CLI `agent`). | Free $0; Plus $8; Premium $30 (adds Cloud Agents, Automations, Bugbot, third-party models); Super $100; Ultra $200; Teams $30/$100/$200 per seat; Enterprise custom; one weekly pool for Cursor models; legacy plans persist. | Anysphere acquired by SpaceX (completion announced 2026-08-14; blog says "Cursor"). | https://cursor.com/docs/models-and-pricing, https://cursor.com/blog/joining-spacex | reviewed |
| Antigravity | No licence stated in docs. | Closed core. `antigravity-cli` repo has README, changelog, examples only (no licence file seen); `antigravity-sdk-python` is Apache-2.0 but needs a compiled runtime binary in the PyPI wheel. | `curl ... antigravity.google/cli/install.sh \| bash`; app auto-updates (switchable). | Free (weekly quota), Google AI Plus/Pro (five-hour quota plus weekly limit), Ultra, AI credits for overage, Enterprise via Google Cloud; no prices on the plans page; under 18 not allowed; work/school Google accounts cannot use personal sign-in. | Gemini CLI transition: post dated 2026-05-19; consumer service stopped 2026-06-18; the blog does not say what happens to the open-source repo. | https://antigravity.google/docs/plans, https://antigravity.google/docs/faq, https://github.com/google-antigravity/antigravity-cli, https://antigravity.google/releases | unreviewed |
| OpenHands | MIT on the public repos (Canvas, SDK, Agent Server; CLI MIT but "no longer actively maintained"); docs say each repo has its own licence. | Open core. Cloud is commercial SaaS; Enterprise is commercial self-hosted. Auth, multi-user, RBAC, isolation by default and budgets sit behind the paid tier. Enterprise server code licence/visibility: not found. | npm `@openhands/agent-canvas` (Node 24+, `uv`), Docker image `ghcr.io/openhands/agent-canvas`, Helm chart, Electron installers, SDK on PyPI (not confirmed on an opened page), Cloud at app.all-hands.dev. | Cloud pricing not on pages opened; Enterprise by contacting sales with trial; Design Partner program (vendor-claim). | Formerly All-Hands-AI; front end now "Agent Canvas"; legacy local GUI and CLI deprecated/unmaintained. | https://github.com/OpenHands/OpenHands, D:/openhands/usage/agent-canvas/setup.md, D:/enterprise/index.md (D = https://docs.openhands.dev) | reviewed |
| OpenClaw | MIT | Open source; no paid tier, hosted service or separate enterprise edition per README and Why-OpenClaw. | Install script, npm, Docker (`ghcr.io/openclaw/openclaw`, image tags `latest`, `main`, `extended-stable`), Nix, Podman, Kubernetes; daemon as LaunchAgent, systemd unit or Scheduled Task. Signed releases. | Free; funding statements are vendor-claim. | Stewarded by the OpenClaw Foundation (501(c)(3) that employs the core team and signs releases); donors named include Amazon, OpenAI, Red Hat. | https://github.com/openclaw/openclaw, https://docs.openclaw.ai/start/why-openclaw, https://docs.openclaw.ai/install/docker | reviewed |
| goose | Apache-2.0 | Open source (Rust core). | Desktop installers; CLI scripts `download_cli.sh`/`download_cli.ps1` from GitHub releases ("stable" tag); `goose update [--canary]`; offline docs page. | Free; provider costs only (not documented on pages opened). | Originally Block; now part of the Agentic AI Foundation (AAIF) at the Linux Foundation (announced 2025-12-09; goose is a Series of LF Projects, LLC; repo now `aaif-goose/goose`); GOVERNANCE.md: 3-7 core maintainers, creator as deadlock tie-breaker; Block is a platinum member (corrected). | https://github.com/aaif-goose/goose, https://aaif.io/press/linux-foundation-announces-the-formation-of-the-agentic-ai-foundation-aaif-anchored-by-new-project-contributions-including-model-context-protocol-mcp-goose-and-agents-md/, https://github.com/aaif-goose/goose/blob/main/GOVERNANCE.md | corrected |
| Hermes Agent | MIT | Open source (Python). Optional paid Nous Portal (models plus "Tool Gateway") is a separate service; everything also works with own keys. | `install.sh` (Linux/macOS/WSL2), `install.ps1` or MSIX, Desktop bundles, Docker, Nix (Tier 2), Termux signed APT (Tier 2, prerelease). PyPI, Homebrew, AUR and Intel-Mac bundles unsupported. | Free code; optional Nous Portal subscription. | Very high release cadence (hundreds of merged PRs per patch release). | https://github.com/NousResearch/hermes-agent, /getting-started/platform-support | reviewed |
| Zed/ACP | Zed editor: GPL/Apache per repo (from `00-targets.md`). ACP: Apache-2.0, no CLA. | Zed editor open source. ACP is an open protocol; Rust SDK Apache-2.0 per repo. | Not in the lines read. | Zed Free plan allows own API keys and external agents; hosted models and Edit Prediction need Pro; Zed does not charge for external agents. | ACP governed jointly by Zed and JetBrains under an interim model, aiming at an independent foundation; v2 is a Draft (2026-07-20). | https://zed.dev/docs/account/plans-and-pricing.md, https://zed.dev/docs/ai/external-agents.md, https://agentclientprotocol.com/community/governance.md | unreviewed (governance reviewed) |
| Warp | Client AGPL v3 (UI crates `warpui_core`/`warpui` MIT); open-sourced 2026-04-28. | Open: the client. Not stated open: server, control plane, Oz/cloud orchestration, model routing, Factories backend; `oz-agent-worker` and SDK licences unchecked. Agent-driven contribution model (OpenAI founding sponsor). | Homebrew cask, winget, deb, rpm, Arch, openSUSE, AppImage; `oz` via Homebrew cask and apt/yum/pacman; Preview channel. | Free, Build, Max, Business, Enterprise; $3 per agent hour on dollar-billed tiers; hosted compute metered per minute; self-hosting needs Enterprise. | Oz agent renamed "Warp Agent" (2026.08.13) while `oz-*` names stay; Factories is Early Access (relabelled from preview in review); `oz` CLI "legacy" on one page, "current" on another. | https://www.warp.dev/blog/warp-is-now-open-source, https://github.com/warpdotdev/warp, https://docs.warp.dev/support-and-community/plans-and-billing/, https://docs.warp.dev/platform/transitioning-from-oz/ | reviewed |
| Cline | Apache-2.0 (`cline/cline`; Cline Bot Inc.); SDK docs page itself states no licence. | Open; the JetBrains plugin is not open source. | VS Code Marketplace, npm `cline`, `@cline/sdk`, Desktop installers. | Cline credits pay-as-you-go, ClinePass $9.99/month ("2-5x usage" is vendor-claim, no absolute limits), BYOK free, enterprise by contact. | None recorded. | https://github.com/cline/cline, https://docs.cline.bot/cline-overview.md, https://docs.cline.bot/getting-started/clinepass.md | unreviewed (ClinePass reviewed) |
| rizzy-harness (ours) | Project licence not in the inventory; CI runs `cargo deny` with permissive-only dependency licences. | Single Rust binary; web UI embedded. | Docker (Linux amd64 images), `cargo run`; Unix only. No release tag yet (no migrations before first tag). | Not applicable / not in the inventory. | Not in the inventory. | `.github/workflows/ci.yml`, `Cargo.toml:14`, `crates/tools/src/bash.rs:6-7`, `README.md:7-13` | from own inventory |

#### Third-party use of vendor subscriptions and credentials
What it is: terms that decide whether another harness may use a vendor's login, quota or gateway.
Variants: Claude Code forbids third-party login and routing through subscription credentials but allows embedding an unmodified binary where each end user signs in themselves (corrected); Antigravity's ToS forbids third-party tools on the consumer service; OpenCodex warns its first-party Claude Desktop mode may breach Anthropic's terms; OpenCode Console deliberately serves other harnesses.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Anthropic does not permit third-party developers to offer claude.ai login or route requests through Free/Pro/Max/Ultra credentials on behalf of users ("unless previously approved" on the SDK page). Corrected scope: embedding is allowed under Commercial Terms if the binary is unmodified, no auth method is removed or restricted, and each end user authenticates with their own API key, subscription or third-party credential; the restriction does not stop a user signing in to the unmodified binary with their own subscription, including where a platform hosts it. | GA (legal terms) | https://code.claude.com/docs/en/legal-and-compliance.md, https://code.claude.com/docs/en/agent-sdk/overview.md | corrected |
| Antigravity | Using third-party tools to access the consumer service violates ToS and can lead to suspension; strongly limits driving `agy` from another harness. | GA (policy) | https://antigravity.google/docs/faq | unreviewed |
| OpenCodex | Docs warn that first-party Claude Desktop interception and account pooling may breach Anthropic's terms and cause suspension. | experimental | https://opencodex.me/guides/claude-code/, https://opencodex.me/reference/client-support/ | unreviewed |
| OpenCode | Console models can be used from Claude Code, Codex, Hermes, Pi or Raycast through `/inference` base URLs; free chat models work only inside OpenCode. | GA (commercial) | https://opencode.ai/docs/console/harnesses/ | reviewed |
| rizzy-harness (ours) | Not in the inventory. | none | not in the inventory | from own inventory |

#### Self-update and release channels
What it is: how the binary updates itself and whether stable and canary tracks exist.
Variants: background auto-update with a one-week-behind stable channel (Claude Code); explicit `update` commands (Codex, Pi, OMP, goose); update run history with rollback (OpenClaw); apt-only on Linux for Claude Desktop; config migrations on update (Hermes).

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | Native installs auto-update in the background; `autoUpdatesChannel` `latest` or `stable` (about a week old, skips releases with major regressions), enforceable by managed settings; `minimumVersion`; `DISABLE_AUTOUPDATER` vs `DISABLE_UPDATES`; Homebrew/WinGet/apt do not auto-update; `/restart`. | GA | https://code.claude.com/docs/en/setup.md, https://code.claude.com/docs/en/settings-reference.md | unreviewed |
| Codex | `codex update` self-updates. | GA | https://learn.chatgpt.com/docs/developer-commands.md | unreviewed |
| Pi | `pi update` updates Pi, packages (`--extensions`), model catalogs (`--models`) or `--self`; `selfUpdateCommand`; `PI_SKIP_VERSION_CHECK`. | GA | https://pi.dev/docs/latest/cli, https://pi.dev/docs/latest/quickstart | unreviewed |
| Oh My Pi | `omp update` with canary and stable. | GA | https://omp.sh/docs/cli | unreviewed |
| goose | `goose update [--canary]`. | GA | https://goose-docs.ai/docs/guides/goose-cli-commands.md | unreviewed |
| OpenClaw | Update run history and rollback; `update.checkOnStart: false` or `OPENCLAW_NO_AUTO_UPDATE=1` stop the daily check (which `telemetry off` does not). | GA | https://docs.openclaw.ai/gateway/telemetry, https://docs.openclaw.ai/llms.txt | reviewed |
| Hermes Agent | Config migrations run on update; `stable` and `canary` channels mentioned for Termux; `stable` Docker tag. | GA | https://github.com/NousResearch/hermes-agent/releases, /user-guide/docker | unreviewed |
| Antigravity | App auto-updates (switchable). | GA | https://antigravity.google/docs/faq | unreviewed |
| Warp | Preview build channel; `WARP_TUI_DISABLE_AUTOUPDATE`. | GA | https://docs.warp.dev/agents/cli/configuration/ | unreviewed |
| Claude Desktop | Linux: no self-update, apt delivers updates. | beta (Linux) | https://code.claude.com/docs/en/desktop-linux | corrected |
| rizzy-harness (ours) | None in the inventory; no migrations before the first tag, dev data dirs are wiped. | none | `CLAUDE.md`, `README.md:7-13` | from own inventory |

### Area 23: Anything else or unusual

#### Adjacent products around the harness
What it is: separate products a vendor ships beside the harness, relevant mostly as scope warnings.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Codex | Codex Security (vulnerability scanning family with CLI, SDK, cloud, plugin, desktop workbench; not read beyond the index); cyber access program (`--cyber-access-program`; cyber models force Auto-review behaviour and ignore `allow` prefix rules). | research preview / beta; cyber GA opt-in | https://learn.chatgpt.com/docs/llms.txt, https://learn.chatgpt.com/docs/changelog (0.161.0, 0.163.0), https://learn.chatgpt.com/docs/sandboxing/auto-review.md | reviewed (cyber access) |
| OpenCode | Browse (real-browser agent), Hex (voice dictation), OpenTunnel (public HTTPS for local apps), OpenTUI; not surveyed. | unclear | https://opencode.ai/product | unreviewed |
| Claude Desktop | Claude Science workbench (beta); chat-side Research, memory, Personal Finance (US, beta, 2026-11-05), monthly recap. Research "mobile web only" was wrong: it works on web, desktop and mobile. | mix GA/beta | https://claude.com/download, https://support.claude.com/en/articles/12138966-release-notes, https://support.claude.com/en/articles/11088861-use-research-on-claude | corrected |
| Claude Code | Artifact templates Slides (`/slides`), Design (`/design`), Docs (claude.ai connector), exporting to PowerPoint/PDF/Word; prompt library; `/claude-api` skill subcommands. | beta | https://code.claude.com/docs/en/artifacts.md, https://code.claude.com/docs/en/prompt-library.md | unreviewed |
| DeepSeek harness | DeepSeek account/platform plugin and LLM API extensions (session-log upload); `present` tool declares up to four final deliverables; landing page shows Word/Excel/HTML/PDF/Markdown previews (vendor-claim). | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/capability-seams, https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog, https://deepseek.com/harness | unreviewed |
| Cursor | Realtime voice chat (2026-11-06), cloud agents that build internal apps behind Cursor sign-in (2026-11-05), Design Mode in mobile. | unclear | https://cursor.com/changelog, https://cursor.com/docs/cloud-agent/mobile | unreviewed |
| OpenHands | Design Partner program, Sandbox Server, "software factory" and daily-workflow use-case pages (pages not opened). | vendor-claim | D:/overview/faqs.md, D:/llms.txt | unreviewed |
| Warp | Oz web app remains for standalone cloud-agent workflows; Factories is the newer web experience; packages renamed `warp-platform-sdk`. | GA / early access mix | https://docs.warp.dev/platform/transitioning-from-oz/ | reviewed |
| Cline | Cline-as-a-service OpenAI-compatible chat-completions API at app.cline.bot. | GA | https://docs.cline.bot/getting-started/clinepass.md, https://docs.cline.bot/llms.txt | unreviewed |
| OpenClaw | Personal agent benchmark pack, "Mantis" Slack desktop runbook, QA/e2e suite (titles only). | unclear | https://docs.openclaw.ai/llms.txt | unreviewed |
| Oh My Pi | CLI utilities: `omp compress`, `omp tiny-models`, `omp render`, `omp gallery`, `omp token`, `omp dry-balance`, `omp images`; native Rust crates (`pi-shell`, `pi-natives`, `pi-walker`, ...). | GA | https://omp.sh/docs/cli, https://github.com/can1357/oh-my-pi | unreviewed |
| Hermes Agent | Optional Codex app-server runtime; docs page for verifying official domains (titles only). | unclear | /docs/llms.txt | unreviewed |
| goose | `goose-doc-guide` docs root and an `ask-ai-bot` service directory. | unclear | https://goose-docs.ai/docs/guides/config-files.md | unreviewed |
| rizzy-harness (ours) | Chat assistant with web search and memory, routines, kanban projects, voice, media; none are "adjacent products" but they widen scope (feature freeze applies). | works | `own-rizzy-harness.md` lines 10, 243-246 (summary only) | from own inventory |

#### Artifacts and deliverables as a reviewed object
What it is: agent output treated as a first-class thing to review, comment on and approve rather than a transcript.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Antigravity | "Artifacts": implementation plans, walkthroughs, screenshots, recordings, `learning_proposal.md`, interactive HTML widgets (App only); CLI `ctrl+r` picker approves or rejects per file; `/diff` line comments sent as the next prompt; pending comments held and sent with the next message. Storage location and retention not documented. | GA | https://antigravity.google/docs/artifacts, https://antigravity.google/docs/cli/artifacts, https://antigravity.google/docs/review-changes | unreviewed |
| Claude Code | Artifacts from claude.ai templates (see above). | beta | https://code.claude.com/docs/en/artifacts.md | unreviewed |
| DeepSeek harness | `present` tool declares final deliverables. | preview | https://deepseek-harness.github.io/deepseek-harness/en/reference/tool-catalog | unreviewed |
| OpenCode | `experimental.artifacts` appears in the changelog but not on the config page. | experimental | https://opencode.ai/changelog | reviewed |
| rizzy-harness (ours) | `[artifacts]` section in `config.toml`; files, artifacts and media are covered in the inventory's section 13 (not read for this group). | partial | `crates/master/src/config.rs:1-420` | from own inventory |

#### Deep links
What it is: a URL scheme that opens the app with a working directory or a prompt.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Claude Code | `claude-cli://open` with `cwd`, `repo=owner/name`, `q` prompt (max 5,000 chars); prompt never sent automatically, warning shown; handler registers at user level only; `disableDeepLinkRegistration` enforceable by managed settings. VS Code has `vscode://anthropic.claude-code/open`; Desktop uses `claude://`. | GA | https://code.claude.com/docs/en/deep-links.md | unreviewed |
| goose | `goose recipe deeplink`; desktop asks to trust a recipe on first run. | GA | https://goose-docs.ai/docs/guides/goose-cli-commands.md | unreviewed |
| rizzy-harness (ours) | None in the inventory. | none | not in the inventory | from own inventory |

#### Agent-addressed docs and consent rules
What it is: documentation or API behaviour written for an AI agent that reads the project, including limits on what the agent may do on the user's behalf.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | `/getting-started/for-agents/` sets consent rules for agents; API returns `403 agent_consent_required` so an agent cannot star the repo for the user. | GA | https://opencodex.me/getting-started/for-agents/ | unreviewed |
| Hermes Agent | A docs page for verifying genuine resources and spotting impersonation (title only). | unclear | /docs/llms.txt | unreviewed |
| rizzy-harness (ours) | None in the inventory. | none | not in the inventory | from own inventory |

#### Vendor-side feedback and one-off oddities
What it is: small features that do not fit elsewhere.

| Harness | How it does it | Status | Evidence | Check |
|---|---|---|---|---|
| Zed/ACP | Per-response thumbs sends the full thread to Zed to tune prompts and tools. | GA | https://zed.dev/docs/ai/agent-panel | unreviewed |
| Antigravity | Prevents the computer from sleeping while an agent runs. | GA | https://antigravity.google/docs/faq | unreviewed |
| Claude Code | Claude-drafted feedback (`/feedback` queue), "You should know" side-agent mod (2.1.287), `login_notice` on the gateway sign-in screen. | GA | https://code.claude.com/docs/en/changelog.md, https://code.claude.com/docs/en/claude-apps-gateway-config.md | unreviewed |
| OpenClaw | `progress_card` tool, streaming and typing indicators for chat channels. | GA | https://docs.openclaw.ai/tools, https://docs.openclaw.ai/concepts/queue | unreviewed |
| Pi | Docs-aware behavioural evals (with and without the project's docs). | mixed | https://github.com/earendil-works/pi/blob/main/packages/evals/README.md | reviewed |

## 3. Availability matrix (areas 20, 22, 23)

Legend: `Y` documented and released; `B` beta, experimental, preview (all DeepSeek harness cells are preview) or partial; `P` planned or vendor-claim only; `-` nothing documented in the lines read; `?` unverified. Columns: Cx Codex, OCx OpenCodex, OC OpenCode, CC Claude Code, CD Claude Desktop, DS DeepSeek harness, OMP Oh My Pi, Pi Pi, Cu Cursor, AG Antigravity, OH OpenHands, Cl OpenClaw, Go goose, He Hermes Agent, Z Zed/ACP, W Warp, Cn Cline, Ours.

| Feature | Cx | OCx | OC | CC | CD | DS | OMP | Pi | Cu | AG | OH | Cl | Go | He | Z | W | Cn | Ours |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Config file formats and locations | Y | Y | Y | Y | B | B | Y | Y | Y | Y | B | Y | Y | Y | Y | Y | Y | Y |
| Precedence user/project/managed/CLI | Y | B | Y | Y | - | B | Y | Y | B | B | - | - | Y | Y | - | B | B | Y |
| Managed (admin) layer and delivery | Y | - | Y | Y | Y | - | - | - | Y | - | - | - | Y | Y | - | Y | Y | Y |
| Repo-supplied config trust gating | Y | - | - | Y | B | - | B | Y | Y | Y | - | - | - | B | - | - | - | - |
| Profiles, presets, recipes | Y | - | - | - | - | B | Y | - | - | - | Y | - | Y | Y | - | Y | - | B |
| Environment variables | Y | Y | Y | Y | - | B | Y | Y | - | - | Y | Y | Y | Y | - | Y | - | Y |
| Themes | Y | - | Y | Y | - | - | Y | Y | - | Y | - | - | - | Y | - | Y | - | Y |
| Key bindings | Y | - | Y | Y | - | - | ? | Y | - | Y | - | - | - | - | Y | B | - | - |
| Output styles, personas, prompt overrides | Y | - | - | Y | - | B | - | Y | - | B | Y | Y | - | Y | - | - | B | B |
| Status lines and footers | Y | - | - | Y | - | - | Y | B | - | Y | - | - | - | - | - | Y | - | - |
| Hot reload and invalid-file handling | - | Y | - | Y | - | - | Y | Y | - | Y | - | Y | B | - | - | Y | - | B |
| Programmatic config editing | Y | Y | - | Y | - | B | Y | - | - | - | - | Y | - | - | - | Y | Y | Y |
| Reading other harnesses' config | Y | Y | B | Y | - | - | Y | - | Y | B | - | - | Y | Y | Y | Y | Y | - |
| Diagnosis and recovery commands | - | - | - | Y | - | B | - | - | - | - | - | Y | - | - | - | - | - | Y |
| Feature flags, experimental toggles | Y | - | B | Y | - | - | - | - | - | - | - | Y | - | - | - | - | - | - |
| Licence, open/closed, install, pricing table | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | Y | - |
| Third-party use terms for credentials | - | B | B | Y | - | - | - | - | - | Y | - | - | - | - | - | - | - | - |
| Self-update and release channels | Y | - | - | Y | B | - | Y | Y | - | Y | - | Y | Y | B | - | B | - | - |
| Adjacent products | Y | - | Y | Y | Y | B | Y | - | Y | - | B | B | B | B | - | Y | Y | - |
| Artifacts and deliverables as reviewed object | - | - | B | B | - | B | - | - | - | Y | - | - | - | - | - | - | - | B |
| Deep links | - | - | - | Y | - | - | - | - | - | - | - | - | Y | - | - | - | - | - |
| Agent-addressed docs and consent rules | - | Y | - | - | - | - | - | - | - | - | - | - | - | ? | - | - | - | - |

Notes on cells: Antigravity "Y" for themes, key bindings and status lines is from one TUI feature line; DSH is `B` throughout because the whole project is a developer preview; Pi footer is `B` because configurability is not stated. Claude Desktop "Y" for the managed layer is the 3P admin console; Claude Code's themes are `Y` but the survey also says plugin themes live under `experimental.*` manifest keys (see section 7).

## 4. Common across the ecosystem

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

## 5. Only one harness has it

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

## 6. Ours against the field (areas 20, 22, 23 only)

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

## 7. Corrections, contradictions and unverified claims (areas 20, 22, 23)

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

## 8. Questions for Round 2 (open-source harnesses only; source code settles these)

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

## 9. Distinctive features across all harnesses

Every item from section 3 "Distinctive or unusual" of all 17 survey files (179 items), grouped by theme. "Why it matters" is one line for a self-hosted master/node harness, taken from the surveys' own relevance reasoning where they gave one. Evidence is the URL of the matching section 2 entry (section 3 lines mostly carry no URL). Check: `reviewed` (the matching entry was confirmed in review), `corrected` (corrected or weakened in review, the correction is in the "Why" cell), `unreviewed`, `unverified`. Items a review corrected or weakened are marked **[corrected]** or **[weakened]**.

### A. Approval, permissions and policy

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

### B. Sandbox, secrets and credential handling

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

### C. Config trust and repo-supplied inputs

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

### D. Protocol, client/server split, remote and node execution

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

### E. Session log, persistence and state

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

### F. Context, compaction and cache

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| OpenCode | Prompt-cache hygiene: cache warming, keyed session instructions that do not break the cache, summary requests reusing the main prompt cache | Cost wins users notice | https://opencode.ai/docs/compaction/, https://opencode.ai/changelog | reviewed |
| DeepSeek harness | Compaction as a locked log sequence with crash-orphan detection (`start` without `end`) | Crash-safe compaction | https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/compaction | unreviewed |
| Oh My Pi | Snapcompact: older context archived as images by a vision model, no summarisation call **[corrected]** | Chain entry corrected (idle compaction off by default; 20k keep-recent is a summarising target) | https://omp.sh/docs/compaction | corrected |
| Oh My Pi | Compaction as a configurable fallback chain (remote, snapcompact, handoff, shake, soft) with idle and mid-turn triggers **[corrected]** | `shake` needs no LLM; same corrections | https://omp.sh/docs/compaction | corrected |
| Claude Code | Idle compaction timed to the prompt-cache TTL **[weakened]** | 200K+ tokens and the one-hour TTL confirmed; "about 54 minutes" not found, other numbers unverified | https://code.claude.com/docs/en/prompt-caching.md | corrected |

### G. Models, routing and providers

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| OpenCodex | Combos: six strategies, quota-window-aware selection, tool-call stickiness, failover only before output starts | Directly reusable provider failover | https://opencodex.me/guides/combos/ | reviewed |
| OpenCodex | Rewriting Codex's model catalog to steer the client's sub-agent surface without patching it | Control a closed client via the data it reads | https://opencodex.me/guides/sub-agent-surface/ | reviewed |
| OpenCode | Console gateway lets other harnesses (Claude Code, Codex, Pi, Hermes) use its models | Commercial model; explains free models and policies | https://opencode.ai/docs/console/harnesses/ | reviewed |
| Oh My Pi | Ten model roles with `@role` aliases, ordered selectors, fallback chains, usage-aware fallback | Clean routing abstraction; README lists nine | https://omp.sh/docs/roles | reviewed |
| Hermes Agent | Per-task auxiliary models (judge, curator, compression, review, approval) | Task-specific model slots | /integrations/providers | unreviewed |
| Warp | Custom model routers by complexity or natural-language rules in YAML | Routing by rule | https://docs.warp.dev/agents/inference/custom-routers/ | unreviewed |

### H. Tools, code mode and edit formats

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

### I. Sub-agents and multi-agent orchestration

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

### J. Memory and learning

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

### K. Extensibility, plugins, hooks and interoperability

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

### L. UI, review and artifacts

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| Claude Desktop | Browser pane with dev-server verification via `launch.json`, clean profile, vendor-run site safety list **[corrected]** | Support article does not cover per-site choices; cookie rules differ by session type | https://code.claude.com/docs/en/desktop | corrected |
| Antigravity | Browser agent drives the user's real Chrome over DevTools MCP | Contradicts "sandboxed" wording elsewhere | https://antigravity.google/docs/commands/browser | reviewed |
| Antigravity | Staged pending comments on artifacts sent with the next message | UI pattern a web dashboard can copy | https://antigravity.google/docs/artifacts | unreviewed |
| Warp | Computer-use screenshots and videos attached to PRs with admin-controlled sharing | Events-plus-files UI model | https://docs.warp.dev/agents/capabilities/computer-use/artifacts-in-prs/ | reviewed |
| Cline | Messaging connectors (Telegram, Slack, Discord, WhatsApp, Linear, Google Chat) as agent front-ends with allow/deny hook | Risky front-ends; Telegram default approves everything | https://docs.cline.bot/cli/connectors.md | reviewed |

### M. Scheduling, goals and automation

| Harness | Item | Why it matters | Evidence | Check |
|---|---|---|---|---|
| Codex | Goals with token and time budgets as persisted thread state with automatic continuation | Budgeted goals | https://learn.chatgpt.com/docs/app-server.md | reviewed |
| Claude Desktop | Scheduled tasks stored as `SKILL.md` on disk with deterministic jitter, one catch-up run, per-task revocation list **[corrected]** | `/schedule` was not removed; settings key claim wrong | https://code.claude.com/docs/en/desktop-scheduled-tasks | corrected |
| Cursor | Rollouts: post-merge monitoring against telemetry at fixed intervals | Beyond merge | https://cursor.com/docs/security-agents | unreviewed |
| OpenHands | Automations synced to Git as YAML with optional encryption and webhook sources with JMESPath filters | Routines as code | `git-sync.md`, `event-automations.md` | unreviewed |
| OpenClaw | Standing intents use a model-free FTS prefilter, with cooldown, fire budget and expiry | Cheap event triggers | https://docs.openclaw.ai/concepts/standing-intents | unreviewed |
| goose | Recipe `retry` with shell success checks and history reset; `response.json_schema` validation | Verified task completion | https://goose-docs.ai/docs/guides/recipes/recipe-reference.md | reviewed |
| Warp | Factories: definitions as code in a customer repo reviewed by PR, foreman agent, never merges itself **[corrected]** | Status is Early Access | https://docs.warp.dev/factories/ | corrected |

### N. Documentation candour, governance and product posture

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

## 10. Overhyped, stale or contradictory

Every item from section 6 of all 17 survey files, grouped by harness, followed by the pattern the reviews found. Items a review changed are marked **[review]**.

### Codex
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

### OpenCodex
- "40+ providers", "zero client patches", sub-agent demos are vendor claims; the client page says no client has runtime-verified results **[review]**: the page does not make a blanket statement.
- README calls the desktop app beta; the desktop docs page does not.
- README lists Docker Compose; the remote-hub page says no official image is published.
- Remote Workspace is presented as a feature but is an "experimental foundation".
- 502 errors blocked the GitHub release list; only two release notes seen.

### OpenCode
- Version confusion: docs say V2 current; GitHub "latest" flags v1.19.5; a Korean post (search snippet only) says "2.0" referred to Desktop v2. https://opencode.ai/changelog
- README (https://github.com/anomalyco/opencode) lists npm `@opencode/cli`, describes plan as asking before bash, calls desktop beta and has no client/server note: likely lags V2.
- The API page calls the whole surface experimental while SDK pages present it as the supported contract.
- Config page omits `experimental.schedules`/`experimental.artifacts` that the changelog introduces; `/goal` is in the changelog without a docs page.
- V2 docs omit sharing and GitHub integration; V1 pages stay live; sharing was discontinued in v1.19.1 **[review]**, https://opencode.ai/en/docs/share seen in a search result only.
- Product page: "One agent, every surface", "100% free to use", a "Degraded performance" banner: marketing. https://opencode.ai/product
- Console page does not map the three prices to plan names; the Go page does.

### Claude Code
- Docs lag the changelog: weekly digest stops at Week 37 (2026-09-11), Week 38 returns 404; many 5.5 models, Mods, idle compaction, AGENTS.md support have no digest; `fast-mode.md` still says unavailable on Google Cloud a day after 2.1.322 added it on Vertex; status line JSON sample shows version 2.1.90; mods reference says "as of" 2.1.290/2.1.299 while citing 2.1.321; settings JSON schema lags; anchors in `server-managed-settings.md` renamed.
- Default-model advice contradicts the defaults: `how-claude-code-works.md` and `costs.md` say Sonnet handles most work, every plan defaults to Opus 5.5; `costs.md` thinking budgets that `model-config.md` says current models ignore. https://code.claude.com/docs/en/model-config.md
- Churn: subagent nesting went 5 levels, off, 3 within six weeks (digest says 5); `keybindingFlavor` lived 23 days, `taskOutputMaxChars` about 2 weeks; output styles deprecated then un-deprecated after 5 days; `/simplify` renamed `/code-review` and now names a different command; `/fork` meant three things.
- Status labels disagree: fullscreen "research preview" yet default since 2026-05-06; `claude agents` research preview on one page; plugin evals, monitors and themes presented as available but under `experimental.*` manifest keys; auto mode "available to all users on every provider" vs model- and provider-gated. **[review]** Review 2 confirmed agent view is research preview (the survey's "GA" entries were wrong).
- Pages disagree on mechanism: spend limits ("not a shared pool" vs `pooled_budget`); cross-session messaging across machines vs same machine; MCP startup timeout 30 s vs 5 s; MCP "Clear authentication" revoking upstream tokens; `self-hosted-environments-deploy` recommends `--capacity 1` but recipes use 4.
- Vendor claims without mechanism or numbers: fast mode "up to 2.5x faster"; Ultrareview "every finding independently reproduced" at $5-25; enterprise cost averages; GitLab "sandboxed execution" with no sandbox in the examples; computer use "flags prompt injection"; the communications kit says the CLI talks directly to Anthropic's API, false for gateway and cloud-provider deployments.
- The vendor admits limits: Bash rules "not a security boundary", sandbox "not a complete isolation boundary", auto mode "does not guarantee safety", policy mods bypassable by `--safe-mode` and worker crashes, server-managed settings not a security boundary.

### Claude Desktop
- Where Cowork runs: getting-started article says cloud; Linux and 3P pages say a local VM. **[review]** Both modes exist; the getting-started article is stale or incomplete.
- Cowork scheduled tasks "run in the cloud" vs Desktop local tasks needing the app open. **[review]** Different products, not a contradiction; `/schedule` now fills in a scheduling request.
- Dispatch: Pro/Max/Ultra feature in Code docs, "limited beta, not for new users" in the support article.
- Cowork "generally available" Apr 9 2026 vs Sep 16 2026 gradual rollout; product page still says "being folded into Claude".
- Linux page lists Cowork in the beta; download page does not list Android; docs index says no dedicated artifacts, memory or research pages.
- Productivity claims on the Cowork page come from testimonials (vendor-claim).
- Changelog version numbers look non-monotonic (v4.29.0 then v4.4212.0).

### DeepSeek harness
- `00-targets.md` said no GitHub releases; the API shows prereleases up to `dsh-v0.2.2-rc.1` **[review]** confirmed.
- Landing page marks agent teams, auto review, scheduled tasks and voice experimental; the docs site documents schedules without that label.
- Landing statements ("out of the box", "explore the upper limit of intelligence", Pomodoro plugin in 5 min 24 s) are marketing.
- The subsystems index links `ssh`, `mcp`, `agent-team`, `webhook` GitHub pages that the docs-site mirror lacks (404; content exists on GitHub) **[review]**.
- The tool catalogue lists alternate or opt-in tools without saying which are mounted by default.
- 278k stars for a repo created 2026-08-13 says nothing about maturity.
- npm first release listed BSD-3-Clause, later MIT; not traced.
- SDK page says the SDK profile pins `danger-full-access`; the web quickstart asks for approval (different profiles, easy to confuse).

### Oh My Pi
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

### Pi
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

### Cursor
- Worker limits: 200 per user and 1,000 per team (pool page) vs 10,000 each (self-hosted page).
- Cloud data retention: privacy page says encrypted copies only while running; cloud security page says VM snapshots kept 30 days and not deletable on demand.
- Memories: silent in agent overview and rules pages; removal documented only on forum posts that disagree (UI vs feature removed).
- Mobile: cloud-agent page suggests Android via PWA; mobile page says Android planned.
- GitHub integration page omits `@cursor` mentions that the cloud agent page lists.
- "Cursor never connects into your network" and "meets the security requirements of most teams" are vendor claims.
- Auto-review is "not a security boundary" but marketed as a default.
- BYOK and Max Mode pages are brief; many status labels absent.

### Antigravity
- "Sandboxed" browser: docs say the subagent uses the real Chrome profile with live sign-ins.
- Hooks: plugins page says shell commands or prompt evaluations; hooks page documents only `command`.
- Effort: `max` in the CLI reference vs low/medium/high elsewhere.
- Plans page says no BYOK/custom endpoints, yet CLI settings list `modelProvider`/`customModelsConfig`/`GEMINI_API_KEY` and the SDK supports Gemini keys, Vertex and local servers.
- Slash command catalog calls itself complete but omits commands; `/context` page does not mention `/compact`; artifact review page mentions "Fast mode" though `/fast` was removed in v1.1.0.
- Overview says four surfaces, llms.txt five; migration page links to a missing settings page.
- "Co-trained", "highly optimized", "securely" are vendor claims.
- Auto mode "Preview" in v2.29.0 and earlier; sandbox "Preview" for business accounts **[review]**: the label could not be found.
- Enterprise "never used for training" is a vendor claim; the enterprise page defers to linked Cloud docs.

### OpenHands
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

### OpenClaw
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

### goose
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

### Hermes Agent
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

### Zed/ACP
- Remote development page says the local machine talks to LLMs and also that the remote uses its own proxy.
- Instructions page lists nine project files, then says the Zed Agent loads `AGENTS.md`, `CLAUDE.md`, `copilot-instructions.md` **[review]**: first match wins in the listed order and `.rules` is still supported.
- Docs say sandboxing without any maturity label; release notes mention it only as a bug fix.
- Agent Panel docs say "no checkpoints" but 1.25.2 and 1.23.3 release notes still refer to them; 1.27.0 removed them.
- Tool-permissions page does not say how it applies to external agents.
- ACP home page says remote support is in progress; the agents page lists 51 agents, several third-party or adapter-based.
- ACP v2 overview page does not mark what is new versus v1.

### Warp
- Naming churn: "Oz" agent renamed "Warp Agent" (2026.08.13) while Oz CLI, Oz web app and `oz-*` repos keep the name; CLI docs call `oz` "legacy" yet it is the documented CI route and the transitioning page says "current". **[review]** confirmed.
- SOC 2, ZDR and "no code stored" are vendor claims.
- Codebase Context page says no code is stored on Warp servers but does not say where embeddings are computed.
- "Open" covers the client only; the cloud platform is not described as open anywhere.
- Orchestration page does not label beta while Factories is Early Access **[review]** (status-wrong: relabelled from preview); parts of the CLI doc truncated.
- Admin Panel page says only the team creator is admin; enterprise pages describe Owner, Admin and Member roles.

### Cline
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

### The pattern across files and reviews

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
