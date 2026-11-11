# Zed agent and the Agent Client Protocol (ACP)

Surveyed 2026-11-10. Public surface only (docs, release notes, protocol site). No source read. Two parts: **Part A** Zed's own agent, **Part B** ACP as a protocol. Entries use the brief's format; `Evidence` URLs are all pages actually opened.

## 1. Identity

- **Zed**: code editor by Zed Industries, written in Rust, GUI (GPUI), macOS/Linux/Windows. Repo https://github.com/zed-industries/zed (GPL and Apache files; per 00-targets.md, not re-checked here). Latest stable per release notes: **1.27.0, 2026-11-04** (https://zed.dev/releases/stable). Matches 00-targets.md.
- **ACP**: open protocol "for connecting any editor to any agent". Repo https://github.com/agentclientprotocol/agent-client-protocol, Apache-2.0, no CLA, about 4.6k stars. **Protocol v1 is stable; v2 is a Draft (published 2026-07-20).** Governed jointly by Zed and JetBrains under an interim model, with a stated aim of moving to an independent foundation (https://agentclientprotocol.com/community/governance.md). Lead maintainers: Ben Brandt (Zed) and Sergey Ignatov (JetBrains).
- Correction to 00-targets.md: `https://zed.dev/docs/ai/rules` 404 is explained: Rules were replaced by Skills and Instructions (see A7). The AI docs now also have pages for Skills, Instructions, Terminal Threads, Parallel Agents, Agent Sandboxing and Agent Profiles that 00-targets.md did not list.

---

# PART A: Zed's own agent

## A1. Agent loop and execution model
### Zed Agent (native)
- What: Built-in agent running in the Agent Panel and Threads Sidebar inside the editor process. It searches and reads files, edits, runs terminal commands, calls MCP tools; models, tools, profiles, skills, instructions and MCP come from Zed's own settings.
- Status: GA
- Evidence: https://zed.dev/docs/ai/zed-agent.md (Zed Agent)
- Notes: Docs do not describe the loop internals (that would be source).
### Three agent paths
- What: Docs distinguish Zed Agent, External Agents (ACP process, own auth/config) and Terminal Threads (native CLI/TUI in a terminal). They differ in where config comes from.
- Status: GA
- Evidence: https://zed.dev/docs/ai/agents.md (Agents); https://zed.dev/docs/ai/overview
### Message queue and steer
- What: Messages sent while the agent is generating are queued by default. A queued message can be toggled to "Steer" (interrupt the Zed Agent at its next step), or sent now. Steer is not available for external agents. Queue items are individually editable.
- Status: GA
- Evidence: https://zed.dev/docs/ai/agent-panel (Messages and Queueing)
- Notes: (verified in review)

## A2. Built-in tools
### Tool list
- What: `diagnostics`, `fetch` (URL to Markdown), `find_path` (glob), `grep` (regex), `list_directory`, `read_file`, `search_web` (only for Zed Pro subscribers using the Zed provider; others use an MCP server), `copy_path`, `create_directory`, `delete_path`, `edit_file` (replace specific text), `move_path`, `write_file`, `terminal`, `skill`, `spawn_agent` (subagent with its own context window).
- Status: GA
- Evidence: https://zed.dev/docs/ai/tools.md (Tools)
- Notes: Tool-call support varies by model; a "No tools" label shows for models without it (https://zed.dev/docs/ai/agent-panel). (verified in review: all 16 tool names and the `search_web` Pro condition match tools.md; the "No tools" label was not re-checked)

## A3. Code editing and file operations
### Review of agent edits
- What: Panel shows files/lines changed; "Review Changes" opens a multi-buffer tab where each hunk, or all, can be accepted or rejected. `agent.single_file_review` shows keep/reject inline in the file. "Follow" (crosshair) makes the editor jump to each file the agent touches.
- Status: GA
- Evidence: https://zed.dev/docs/ai/agent-panel (Following and Reviewing Edits)
### Inline Assistant
- What: Prompt-driven in-place transform of a selection (ctrl-enter), multi-cursor, can fan out to several models via `inline_alternatives`. Does not support external agents.
- Status: GA
- Evidence: https://zed.dev/docs/ai/inline-assistant.md
### Edit Prediction (one line)
- What: Tab-style AI completions; default provider Zeta (open model, needs sign-in, included in paid plans); alternatives Copilot, Mercury Coder, Codestral, Ollama/OpenAI-compatible; modes `eager` and `subtle`.
- Status: GA
- Evidence: https://zed.dev/docs/ai/edit-prediction.md

## A4. Terminal and shell integration
### `terminal` tool and Terminal Threads
- What: Agent `terminal` tool runs a command and returns combined output. Separately, **Terminal Threads** host an agent CLI/TUI (Claude Code, Amp, OpenCode, Pi, Codex named) in a terminal listed as a thread in the sidebar; `agent.terminal_init_command` auto-types a command; bell rings produce notifications; closed threads are not kept in history. Zed profiles, permissions, skills and MCP settings do not apply to them. Terminal tabs can be dragged into the Agent Panel (1.24.2).
- Status: GA
- Evidence: https://zed.dev/docs/ai/terminal-threads.md; https://zed.dev/releases/stable (1.24.2)
- Notes: no status label on the page. (verified in review)

## A5. Permissions, approvals, sandboxing
### Tool permissions
- What: Settings `agent.tool_permissions` with global `default` (confirm | allow | deny; default confirm) and per-tool `default`, `always_allow`, `always_deny`, `always_confirm` pattern lists (Rust regex, case-insensitive by default). Matching targets: terminal = command string; file tools = path; move/copy = source and destination; fetch = URL; search_web = query; skill = path of SKILL.md; MCP tools = `mcp:<server>:<tool>`. Precedence: built-in rules, deny, confirm, allow, tool default, global default. Terminal chains (`&&`) are split and each part checked, across many shells. Hardcoded protection only for recursive forced delete of `/`, home, cwd, parent. Approval card offers once / always-for-tool / always-for-pattern (MCP: tool-level only). Introduced as `default` in v0.224.0, replacing boolean `always_allow_tool_actions`. Page does not say how this applies to external agents.
- Status: GA
- Evidence: https://zed.dev/docs/ai/tool-permissions.md (Tool Permissions Model); https://zed.dev/docs/ai/mcp
- Notes: the page says the small hardcoded rules apply only to the terminal tool; the claim that chain splitting works across many shells was not re-checked. (verified in review: keys, default confirm, precedence order, case-insensitive default, `mcp:<server>:<tool>`, v0.224.0 note, silence on external agents)
### Agent sandboxing
- What: Confines only the Zed Agent's `terminal` and `fetch` tools. macOS: Seatbelt (`sandbox-exec`) with an HTTP/HTTPS proxy for approved hosts. Linux: Bubblewrap (non-setuid, may fall back to unsandboxed with a warning). Windows: only inside WSL (Bubblewrap); native commands unconfined. Default: reads allowed, writes only to open project dirs and temp, git metadata never writable, network blocked. Grants in `agent.sandbox_permissions` (`network_hosts`, `allow_all_hosts`, `write_paths`, `allow_fs_write_all`, `allow_unsandboxed`) with scopes once / thread / always. Documented gaps: side channels (rust-analyzer proc macros, modified Makefile run in the normal terminal), proxy-ignoring tools (SSH, raw sockets), Windows NTFS escapes.
- Status: unclear (page states no maturity; only appears as a Linux bug fix in release notes 1.27.0)
- Evidence: https://zed.dev/docs/ai/sandboxing.md (Agent Sandboxing); https://zed.dev/releases/stable
- Notes: Does not cover external agents or Terminal Threads (the page also lists language servers, the built-in Git client and the regular terminal as uncovered). No maturity label on the page. (verified in review: tools covered, mechanisms per OS, default policy, setting keys; release note 1.27.0 sandbox bug fix)
### Trusted worktrees
- What: Project-local skills and instructions load only from trusted worktrees.
- Status: GA
- Evidence: https://zed.dev/docs/ai/skills.md; https://zed.dev/docs/ai/privacy-and-security.md

## A6. Context management and compaction
### Auto compaction
- What: Zed Agent threads compact automatically (`agent.auto_compact`, enabled by default, `threshold` default 90%, accepts percent, token count, or negative tokens-remaining); `/compact` for manual; a "Context Compacted" entry shows the summary; `agent.compaction_model` selects a different model. If a model's window is under 80k tokens a banner suggests "New From Summary". Token usage shown near the profile selector.
- Status: GA
- Evidence: https://zed.dev/docs/ai/agent-panel (Token Usage and Compaction); https://zed.dev/docs/ai/agent-settings.md (Automatic compaction)
- Notes: Session notices and compaction were enabled for external agents via ACP in 1.25.2 (https://zed.dev/releases/stable). (verified in review: the 90% default is on the Agent Settings page; the Agent Panel page gives no default; the 80k banner text matches)
### @-mentions
- What: Mention files, directories, symbols, previous threads, skills, diagnostics, branch diffs, URLs; images for vision models; pasted multi-line code becomes a mention.
- Status: GA
- Evidence: https://zed.dev/docs/ai/agent-panel (Adding Context)

## A7. Memory and persistent instructions
### Instructions (replaces Rules)
- What: Personal `~/.config/zed/AGENTS.md` (Windows `%APPDATA%\Zed\AGENTS.md`). Project file: first match of `.rules`, `.cursorrules`, `.windsurfrules`, `.clinerules`, `.github/copilot-instructions.md`, `AGENT.md`, `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`. Project wins over personal. Rules library was replaced: on-demand Rules became Skills, always-on Rules became personal AGENTS.md. External agents read their own files.
- Status: GA
- Evidence: https://zed.dev/docs/ai/instructions.md (Instructions)
- Notes: Wording on which files the Zed Agent actually loads differs inside the page (a list of nine vs "loads AGENTS.md, CLAUDE.md, copilot-instructions"); see section 6. The Instructions page itself describes a first-match rule over the nine-file list (in the order given) and says project `.rules` files remain supported. (verified in review; corrected in review: first match wins, listed order is `.rules`, `.cursorrules`, `.windsurfrules`, `.clinerules`, `.github/copilot-instructions.md`, `AGENT.md`, `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`)
### Skills
- What: Folder with `SKILL.md` (frontmatter `name`, `description`, optional `disable-model-invocation`) in `~/.agents/skills/` (global) or `<worktree>/.agents/skills/` (project, trusted worktrees only; project overrides global). Model sees a name+description catalog (capped 50KB) and calls the `skill` tool; user skills need permission, built-in ones do not. Manual via `/` or `@skill`. Authoring via `/create-skill`, a skills manager, `zed://skill` share links, create from GitHub URL. Follows agentskills.io spec (not all fields). Zed Agent only. The agent cannot edit SKILL.md without explicit authorisation.
- Status: GA
- Evidence: https://zed.dev/docs/ai/skills.md (Skills)
### Memory
- What: nothing documented (no memory store, only instructions and skills).
- Status: unclear
- Evidence: https://zed.dev/docs/llms.txt (AI section index lists no memory page)

## A8. Planning, tasks, modes
### Profiles
- What: Three built-ins: Write, Ask (read-only), Minimal (no project tools). Custom profiles under `agent.profiles` with per-tool booleans, MCP server toggles, optional `default_model`. Profiles control tool availability only; permission-gated calls still go through Tool Permissions. Zed Agent only.
- Status: GA
- Evidence: https://zed.dev/docs/ai/agent-profiles.md (Agent Profiles)
- Notes: Agent Panel page says profiles are the closest thing to modes. Plan files or a plan-mode: nothing documented for the Zed Agent (ACP does carry plans, see B).

## A9. Sub-agents and multi-agent
### Subagents
- What: `spawn_agent` tool starts a subagent with its own context window. `agent.subagent_model` sets provider, model, effort, `enable_thinking` (default false), `speed`; if unset, subagents inherit the parent's settings.
- Status: GA (inferred; no maturity label)
- Evidence: https://zed.dev/docs/ai/tools.md; https://zed.dev/docs/ai/agent-settings.md (Subagent model)
- Notes: Release notes fix hangs restoring threads with subagents (1.26.2), so subagent threads persist. Tools page says subagents have the same tools as the parent. (verified in review: `agent.subagent_model` keys and `enable_thinking` default false)
### Parallel Agents and Threads Sidebar
- What: Several threads (Zed Agent, external agents, terminal threads) at once, each with own context and history, grouped by project in a sidebar. Multi-root projects let one thread read/write across folders. No numeric limits documented.
- Status: GA
- Evidence: https://zed.dev/docs/ai/parallel-agents.md (Parallel Agents); https://zed.dev/docs/ai/agent-panel

## A10. Background, long-running, scheduled
### Background behaviour
- What: Zed blocks system idle sleep while a thread generates (`agent.prevent_idle_sleep`); desktop notification/sound when done in background. Threads run while you work in other threads. No scheduler, cron or headless agent is documented; agents live inside the desktop app.
- Status: GA
- Evidence: https://zed.dev/docs/ai/agent-panel (Background Behavior and Notifications); https://zed.dev/docs/ai/parallel-agents.md
- Notes: "Scheduled work": nothing documented.

## A11. Sessions, persistence, checkpoints, undo
### Thread history, archive, import
- What: Thread History across projects (fuzzy title search), archive/restore, permanent delete, "New From Summary", message edit-and-resubmit. Can import sessions from configured external agents (duplicates and sessions without a working dir skipped; imported threads arrive archived; Cursor and Gemini CLI import unsupported per Parallel Agents page).
- Status: GA
- Evidence: https://zed.dev/docs/ai/parallel-agents.md; https://zed.dev/docs/ai/external-agents.md (Importing threads)
### Checkpoints (REMOVED)
- What: The Agent Panel "Restore Checkpoint" feature was **removed in 1.27.0** (2026-11-04) because it slowed or hung responses in large Git repositories. Undo is by rejecting edits or using Git. The current Agent Panel page lists no checkpoints.
- Status: deprecated (removed)
- Evidence: https://zed.dev/releases/stable (1.27.0); https://zed.dev/docs/ai/agent-panel (page note)
- Notes: 1.25.2 and 1.23.3 notes still mention Git checkpoints; so checkpoint-per-prompt was a Git-based design that failed at scale. Useful lesson. 1.27.0 release note (Nov 04, 2026) reads: removed agent checkpoints and the "Restore Checkpoint" button because taking a checkpoint before every message made the agent slow or hang in large Git repositories. The Agent Panel page does not mention checkpoints at all (there is no "page note"). (verified in review; corrected in review: Agent Panel page has no removal note, it is simply silent; 1.25.2 is dated Oct 21 2026, 1.23.3 Oct 12 2026)

## A12. Git and version control
### Worktree isolation
- What: Threads can run in separate Git worktrees via a title-bar picker; new worktrees start in detached HEAD; a `create_worktree` task hook with `ZED_WORKTREE_ROOT` and `ZED_MAIN_GIT_WORKTREE` automates setup. Archiving a thread saves worktree Git state and removes it from disk; restoring re-creates it.
- Status: GA
- Evidence: https://zed.dev/docs/ai/parallel-agents.md (Worktree Isolation)
### Commit message generation
- What: AI commit messages from the Git panel (`agent.commit_message_model`, `commit_message_instructions`); uses the three latest commits as style examples (1.27.0). Pull request creation or agent-driven review: nothing documented.
- Status: GA
- Evidence: https://zed.dev/docs/ai/agent-settings.md; https://zed.dev/releases/stable

## A13. Extensibility
### MCP (context servers)
- What: Supports MCP Tools and Prompts; Discovery, Sampling and Elicitation not supported (as documented). Config under `context_servers`: local (`command`, `args`, `env`) or remote (`url`, `headers`); extensions package servers; OAuth prompt when no Authorization header; reloads on `tools/list_changed`. Servers configured in Zed are forwarded to external agents over ACP.
- Status: GA
- Evidence: https://zed.dev/docs/ai/mcp (Model Context Protocol)
- Notes: ACP elicitation exists in release notes (1.24.2, "agent elicitation forms") while MCP elicitation is "not yet supported": different features, same word.
### Extensions and hooks
- What: Extensions (incl. MCP servers). Hooks for agent actions: nothing documented. Custom slash commands as such: skills via `/` only.
- Status: GA / nothing documented
- Evidence: https://zed.dev/docs/ai/mcp; https://zed.dev/docs/ai/skills.md

## A14. Models, providers, inference config
### Providers
- What: Zed-hosted models (billed through Zed); API keys for Anthropic, OpenAI, Google AI, Mistral, DeepSeek, xAI, OpenCode, plus Anthropic/OpenAI-compatible endpoints; gateways OpenRouter, Vercel AI Gateway, Amazon Bedrock; subscriptions (ChatGPT sign-in as a provider; Copilot Chat); local llama.cpp, Ollama, LM Studio, OpenAI-compatible. Keys stored in system keychain. Per-feature models (`inline_assistant_model`, `commit_message_model`, `thread_summary_model`, `compaction_model`, `subagent_model`), favorites, `agent.model_parameters` temperature.
- Status: GA
- Evidence: https://zed.dev/docs/ai/llm-providers.md; https://zed.dev/docs/ai/use-api-access.md; https://zed.dev/docs/ai/use-a-gateway.md; https://zed.dev/docs/ai/use-a-local-model.md; https://zed.dev/docs/ai/use-an-existing-subscription.md; https://zed.dev/docs/ai/agent-settings.md

## A15. UI surfaces
- What: Desktop editor only (Agent Panel, Threads Sidebar, inline assistant). No web, mobile or voice surface documented. ACP lets other clients host agents (Part B).
- Status: GA
- Evidence: https://zed.dev/docs/ai/overview

## A16. Browser automation and computer use
- What: nothing documented (web only via `fetch`, `search_web`, MCP servers; Puppeteer MCP is named as an extension example).
- Evidence: https://zed.dev/docs/ai/tools.md; https://zed.dev/docs/ai/mcp

## A17. Remote, cloud, headless
### Remote development
- What: Local UI plus a headless Zed server on the remote machine (code, language servers, tasks, terminal run there). Uses local `ssh` with a ControlMaster per project; server binary auto-downloaded to `~/.zed_server` or uploaded; reconnect keeps unsaved edits. WSL and dev containers also exist. Agent Panel and Inline Assistant work remotely. The page is contradictory on where LLM calls originate (local machine vs remote proxy settings) and does not mention external agents; the external-agents page says credentials for remote projects may come from local, remote or agent-managed sources. 1.25.2 fixed env vars (incl. secrets) passed to external agents and MCP servers in remote projects being visible in the local process list; 1.27.0 fixed remote projects hanging when loading threads with many external files.
- Status: GA
- Evidence: https://zed.dev/docs/remote-development.md; https://zed.dev/docs/ai/external-agents.md; https://zed.dev/releases/stable
- Notes: Cloud/background agents (server-hosted, headless): nothing documented.

## A18. Collaboration, teams, enterprise
- What: Zed Business: org-wide admin controls, roles, consolidated billing, org spend limit; admins can block members from submitting feedback or training-data collection. Free, Pro (monthly $5 token credit, usage billed at provider list price plus 10%, default $10 extra-spend limit), Student, Business plans.
- Status: GA
- Evidence: https://zed.dev/docs/account/plans-and-pricing.md; https://zed.dev/docs/ai/ai-improvement.md
- Notes: Page has no date; prices not listed in full.

## A19. Observability
- What: Token usage in panel; thread can be opened as Markdown for bug reports; `dev: open acp logs` shows ACP messages between Zed and an external agent; per-response thumbs rating. Cost dashboard at dashboard.zed.dev (Billing page, per plans page). Tracing/export: nothing documented.
- Status: GA
- Evidence: https://zed.dev/docs/ai/agent-panel; https://zed.dev/docs/ai/external-agents.md (Debugging); https://zed.dev/docs/account/plans-and-pricing.md

## A20. Configuration
- What: JSON `settings.json` with `agent.*` keys, Settings Editor AI section, per-project settings; keybindings. Precedence rules between user and project settings for agent keys: nothing documented in pages read.
- Status: GA
- Evidence: https://zed.dev/docs/ai/quick-start.md; https://zed.dev/docs/ai/agent-settings.md

## A21. Security and privacy
- What: Zed does not retain prompts or code by default. Zed-hosted models: provider agreements bar training and require zero retention, **except** Anthropic "Covered Models" retained at least 30 days for safety review (cannot be opted out, also applies with own key). Ratings send the full thread to Zed; Edit Prediction training data only if opted in, project is open source (licence-file detection) and file not excluded; dataset public (`zed-industries/zeta` on Hugging Face). External agents and terminal threads follow their own vendor terms. Master switch `disable_ai: true` (1.27.0 fixed LLM credentials being read when it was true).
- Status: GA
- Evidence: https://zed.dev/docs/ai/privacy-and-security.md; https://zed.dev/docs/ai/ai-improvement.md; https://zed.dev/docs/ai/quick-start.md; https://zed.dev/releases/stable

## A22. Licence, pricing, distribution
- What: Editor open source (GPL/Apache per repo, from 00-targets.md). Free plan allows own API keys and external agents; hosted models and Edit Prediction need Pro. Zed does not charge for external agents.
- Status: GA
- Evidence: https://zed.dev/docs/account/plans-and-pricing.md; https://zed.dev/docs/ai/external-agents.md

## A23. Other
### Rating-based feedback loop
- What: Per-response thumbs sends full thread to Zed to tune prompts and tools (see A21). Evidence: https://zed.dev/docs/ai/agent-panel.

---

# PART B: ACP as a protocol

## B1. What it is
### Purpose and model
- What: Standardises editor-to-agent communication, modelled on LSP (JSON-RPC 2.0, capability negotiation at `initialize`). Client = editor that launches the agent as a subprocess and provides files, terminals and MCP access; Agent = the AI process. One connection can hold several concurrent sessions. Markdown is the default text format. Assumes a user in an editor with a model they trust.
- Status: GA (v1 stable)
- Evidence: https://agentclientprotocol.com/ (Introduction); https://agentclientprotocol.com/get-started/architecture.md; https://agentclientprotocol.com/get-started/related-protocols.md
### Versions
- What: v1 current stable (integer major version negotiated at `initialize`). v2 published in **Draft** 2026-07-20: updates can arrive outside a turn, message IDs required, patch-style message/tool-call updates, structured file changes instead of oldText/newText with optional git patch, permission requests with title/description/subject (command subject), unknown enum variants tolerated. Implementers told to gate v2 behind negotiation and keep v1.
- Status: v1 GA; v2 planned/draft
- Evidence: https://agentclientprotocol.com/announcements/acp-v2-draft.md; https://agentclientprotocol.com/protocol/v1/initialization.md
- Notes: The v2 overview page itself did not mark what is new, so rely on the announcement. (verified in review: Draft status, 2026-07-20, change list, guidance to keep v1)

## B2. Message surface (v1)
### Methods
- What: Agent side (client calls): `initialize`, `authenticate`, `session/new`, `session/prompt` (baseline); optional `session/load`, `session/set_mode`, `logout`, plus separately documented `session/resume`, `session/close`, `session/list`, `session/delete`, `session/set_config_option`; notification `session/cancel`; `$/cancel_request` (generic request cancel, stabilised 2026-06-29). Client side (agent calls): `session/request_permission` (baseline); optional `fs/read_text_file`, `fs/write_text_file`, `terminal/create|output|wait_for_exit|kill|release`, `elicitation/create`. Agent notifications: `session/update` (message chunks incl. thought, tool calls and updates, plans, available commands, mode changes, config option updates, notices, compaction, session info/usage) and `elicitation/complete`.
- Status: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/overview.md; https://agentclientprotocol.com/updates.md; https://agentclientprotocol.com/protocol/v1/session-setup.md
### Capability negotiation
- What: Client capabilities: `auth.terminal`, `fs.readTextFile/writeTextFile`, `terminal`, `elicitation` (form/url), `session.configOptions.boolean`, `session.compaction`, `session.notices`. Agent capabilities: `loadSession`, `promptCapabilities` (text and resource_link baseline; image, audio, embeddedContext optional), `mcpCapabilities` (http, sse), `auth.logout`, `session.delete`, `session.additionalDirectories`, `session.systemPrompt`. Everything optional except baseline.
- Status: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/initialization.md

## B3. Sessions and prompts
### Session lifecycle
- What: `session/new` takes absolute `cwd` and `mcpServers` and returns `sessionId`. `session/load` replays full history as updates; `session/resume` restores without replay; `session/close` frees resources (idempotent); `session/list`; `session/delete` (retention left to agent); `additionalDirectories` extends the root set `[cwd, ...]` (must be re-sent on load/resume). Live metadata via session info update.
- Status: GA (all stabilised Feb-Jun 2026)
- Evidence: https://agentclientprotocol.com/protocol/v1/session-setup.md; https://agentclientprotocol.com/updates.md
### Prompt turn
- What: `session/prompt` with content blocks; agent streams `session/update` (plan, message chunks sharing a `messageId`, tool calls), loops model and tools, then replies with stop reason: `end_turn`, `max_tokens`, `max_turn_requests`, `refusal`, `cancelled`. Cancel via `session/cancel`; client must answer pending permission requests with `cancelled`; agent must respond with `cancelled`, not an error.
- Status: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/prompt-turn.md
### Session modes and config options
- What: Modes (id/name/description, `session/set_mode`, agent-initiated `current_mode_update`, e.g. plan mode exit via permission request) are **to be removed** in a future version in favour of session config options: `select` and `boolean` options with optional categories `mode`, `model`, `model_config`, `thought_level`; set with `session/set_config_option`; every reply carries full state.
- Status: modes: deprecated (planned removal); config options: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/session-modes.md; https://agentclientprotocol.com/protocol/v1/session-config-options.md
### Plans
- What: `plan` update with `entries` (content, priority high/medium/low, status pending/in_progress/completed); each update replaces the whole plan.
- Status: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/agent-plan.md
### Slash commands
- What: Agent advertises commands via `available_commands_update`; the client sends them as plain text in a prompt, so there is no separate API.
- Status: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/slash-commands.md
### Client-provided system prompt
- What: Clients may supply a system prompt (append or override, if agent advertises `session.systemPrompt`). Stabilised 2026-11-06.
- Status: GA
- Evidence: https://agentclientprotocol.com/updates.md; https://agentclientprotocol.com/protocol/v1/initialization.md

## B4. Tool calls and permissions
### Tool call reporting
- What: `tool_call` update with `toolCallId`, `title`, optional `name` (stabilised 2026-09-17), `kind` (read, edit, delete, move, search, execute, think, fetch, switch_mode, other), status (pending, in_progress, completed, failed), content (text/image/resource, **diff** with path/oldText/newText, **terminal** embed by terminalId), `locations` (path and line, display only), raw input/output. Later `tool_call_update` carries only changed fields.
- Status: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/tool-calls.md
### Permission requests
- What: Agent calls `session/request_permission` with options of kind `allow_once`, `allow_always`, `reject_once`, `reject_always`. The kind is a UI hint only; the **agent** stores remembered decisions and decides their scope. Permission requests are advisory: the agent enforces, and the client must still apply its own checks for fs and terminal operations. Agents should re-ask if command or resolved target changes.
- Status: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/tool-calls.md; https://agentclientprotocol.com/protocol/v1/security.md
- Notes: v2 draft replaces the fixed tool-call subject with title/description/extensible subject (https://agentclientprotocol.com/announcements/acp-v2-draft.md). The word "advisory" is on the security page ("Permission requests are advisory"); the tool-calls page says agents may ask, should perform only what the approved request described, and the agent stores `allow_always` decisions. (verified in review: advisory, agent stores always-decisions, `kind` is a UI hint, agents execute tool calls themselves)
### Elicitation
- What: Agent requests structured input: **form** mode (flat JSON Schema, non-secrets only) or **url** mode (out-of-band page, e.g. OAuth; client must show the full URL and get consent). Responses accept/decline/cancel. Stabilised 2026-07-22.
- Status: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/elicitation.md

## B5. File system and terminal access
### File system
- What: `fs/read_text_file` (absolute path, optional 1-based line and limit; returns editor state including unsaved changes) and `fs/write_text_file` (creates file if missing). Client does the I/O, should check paths against the session root set, and should ask before writing files that control execution or approval (hooks, settings, MCP config). Error codes, encoding, line endings not specified.
- Status: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/file-system.md
- Notes: the client performs the I/O and is responsible for checking it; an agent's permission request does not replace the client's checks. (verified in review)
### Terminals
- What: `terminal/create` (command, args, env, absolute cwd, `outputByteLimit`) returns immediately; `output` (with truncated flag, exit status), `wait_for_exit`, `kill` (terminal stays valid), `release` (mandatory). Terminal can be embedded in a tool call. The client runs the command and must apply its own policy to command, args and env, and neutralise control sequences.
- Status: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/terminals.md
- Notes: v2 has an RFD to rework fs and terminal as a client "execution surface" (https://agentclientprotocol.com/rfds/v2/client-filesystem-terminal-capabilities.md, title only; not opened). (verified in review: the client runs the command, applies its own policy to command, args and env, `outputByteLimit` truncates earlier output)

## B6. Transport
### stdio (only standardised transport)
- What: Client spawns agent; newline-delimited JSON-RPC on stdin/stdout, no embedded newlines; stderr free for UTF-8 logs; shutdown by closing stdin then terminating. Custom transports are allowed if JSON-RPC and lifecycle are preserved.
- Status: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/transports.md
- Notes: the page says stdio is currently the one defined transport; Streamable HTTP is in a separate RFD. (verified in review)
### Remote (Streamable HTTP and WebSocket)
- What: RFD for a single `/acp` endpoint: Streamable HTTP (long-lived GET streams, POST for client-to-server, HTTP/2 required, `Acp-Connection-Id` and `Acp-Session-Id` headers, cookies for sticky sessions) or WebSocket upgrade. v1 relies on `session/load` for recovery with no replay; resumption planned for v2. Reference implementation in goose in progress; SDK support and hardening (origin validation, version header) still to do. A Transports Working Group formed 2026-04-22.
- Status: planned
- Evidence: https://agentclientprotocol.com/rfds/streamable-http-websocket-transport.md (status active as of 2026-07-02); https://agentclientprotocol.com/announcements/transports-working-group.md (title only via index)
- Notes: Home page says remote support is "still in progress" (home page not re-checked). The RFD status is Active (moved to Active 2026-07-02; latest revision 2026-10-30 removing batch support); phase 2 (goose reference implementation) in progress; SDK support and hardening remain. goose's own docs already document `goose serve` exposing `/acp` over HTTP and WebSocket (see goose.md), so a vendor implementation ships ahead of the SDKs. (verified in review)

## B7. Authentication
- What: Agent lists `authMethods` at initialize. `agent` type: client calls `authenticate`. `terminal` type: client runs the agent program interactively with extra args/env (client must advertise `auth.terminal`; descriptor cannot supply a command); exit 0 = success. `logout` optional. Registry inclusion requires an auth method.
- Status: GA
- Evidence: https://agentclientprotocol.com/protocol/v1/authentication.md; https://agentclientprotocol.com/get-started/registry.md

## B8. Security model (as documented)
- What: ACP does not sandbox the agent. Permission requests advisory. Treat all agent strings, URIs, paths as untrusted; sanitise Markdown/HTML; make bidi and zero-width characters visible; no automatic remote image fetches; `file:` outside the root set needs user action; no credentials in `_meta`; agents should ask before sending data off-machine after reading untrusted content and before editing hooks/settings/MCP config; proxies can read and alter everything; multi-user agents should scope sessions to the owner. Out of scope: malicious agent binaries, prompt-injection detection, OS sandboxing. Residual risks listed.
- Status: GA (guidance)
- Evidence: https://agentclientprotocol.com/protocol/v1/security.md

## B9. Relationship to MCP and other protocols
- What: Complementary: MCP gives an agent tools; ACP connects client to agent. Client passes its MCP server configs in `session/new` (stdio required; http and sse need agent capabilities; sse deprecated); agent connects directly. Client can expose its own tools through a stdio proxy. ACP reuses MCP JSON types and adds diffs, plans, permissions. Proposals: **MCP-over-ACP** (`"type": "acp"` server, `mcp/message`; draft, targets MCP 2026-07-28), and **ACP proxy chains** (conductor routing messages through proxies; implemented in the Rust SDK, behind `unstable_proxy_chains`). A2A (Linux Foundation) is for agent-to-agent delegation; IBM BeeAI "Agent Communication Protocol" is unrelated and archived Aug 2025 per the page.
- Status: GA (MCP passthrough); planned/draft (MCP-over-ACP, proxy chains)
- Evidence: https://agentclientprotocol.com/get-started/related-protocols.md; https://agentclientprotocol.com/get-started/architecture.md; https://agentclientprotocol.com/protocol/v1/session-setup.md; https://agentclientprotocol.com/rfds/mcp-over-acp.md; https://agentclientprotocol.com/rfds/proxy-chains.md
- Notes: MCP-over-ACP draft, `"type": "acp"`, `mcp/message`, MCP 2026-07-28 only: verified in review. A2A and BeeAI archived Aug 2025: verified. The related-protocols page does not mention proxy chains; the proxy-chains RFD and the `unstable_proxy_chains` Rust SDK claim were not opened in review, so unverified.

## B10. Subagents and other roadmap (RFDs)
- What: Subagent Sessions RFD (draft, revised 2026-11-09): child sessions with own session IDs observable but not promptable, cancel only; behind `unstable_subagents`. Other RFDs listed: session fork, session compaction, session notices, plan operations, end-turn token usage, configurable LLM providers (custom endpoint), v2 background work. Compaction, notices, usage updates already appear as capabilities/updates in v1.
- Status: planned (subagents); mixed for others (titles only seen in index)
- Evidence: https://agentclientprotocol.com/rfds/subagents.md; https://agentclientprotocol.com/llms.txt
- Notes: only the subagents RFD was opened; the others are title-only. (verified in review: subagents RFD revised 2026-11-09, observable not promptable, cancel only, flag `unstable_subagents`; the page states no formal status)

## B11. Agents that implement ACP (official list)
- What: https://agentclientprotocol.com/get-started/agents.md lists **51** agents including: Claude Agent (via Zed's `claude-agent-acp` adapter on the Agent SDK), Codex CLI (via `codex-acp` adapter), Gemini CLI, GitHub Copilot (CLI, public preview since 2026-01-28), Cursor, Cline, goose, Hermes Agent, OpenCode, OpenHands, OpenClaw, Pi (via `pi-acp` adapter), Junie, Kiro CLI, Kimi CLI, Qwen Code, Mistral Vibe, Factory Droid, Augment Code, Poolside, Qoder CLI, Docker cagent, fast-agent, Stakpak, VT Code and others. Several mainstream agents are reached only through adapters, not natively.
- Status: GA
- Evidence: https://agentclientprotocol.com/get-started/agents.md
- Notes: count of 51 confirmed. The page marks Claude Agent ("via Zed's SDK adapter"), Codex CLI ("via ACP's adapter"), Pi ("via pi-acp adapter") and Bub as adapter-based; the `claude-agent-acp` and `codex-acp` binary names come from goose docs, not this page. (verified in review)

## B12. Clients that implement ACP (official list)
- What: https://agentclientprotocol.com/get-started/clients.md lists Zed, JetBrains, VS Code (several third-party extensions), Visual Studio, Neovim (CodeCompanion, agentic.nvim, avante.nvim), Emacs (agent-shell.el), Sublime Text, Qt Creator, Pulsar, Obsidian plugins, Unity plugins, Chrome extensions, Volt, necoder (GPUI Rust editor), Athas, and dozens of CLI, desktop/web and mobile clients (e.g. acpx, Toad, Devin Desktop, Happy, Tidewave). Page gives no support-level ratings. Cursor/Windsurf/Trae get VS Code extension builds.
- Status: GA
- Evidence: https://agentclientprotocol.com/get-started/clients.md
- Notes: the page's six categories total 142 clients (editors and IDEs 21, CLI and TUI 12, desktop and web 72, notebook and data 5, mobile 14, messaging 18), plus separate frameworks (9) and connectors (12) lists; Zed, JetBrains, VS Code, Neovim, Emacs, Sublime Text, Visual Studio all listed. (verified in review; corrected in review: count added, "dozens" understated it)

## B13. Registry
- What: Curated catalogue of ACP agents (only agents supporting authentication). Each described by `agent.json` against a schema in https://github.com/agentclientprotocol/registry (folder named by agent id, optional icon, PR to submit). Clients fetch https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json, which contains distribution info for automatic install. Released and stabilised 2026-03-09. Zed uses it via `zed: acp registry` and Add Agent > Install from Registry. The page does not describe distribution types (npx, binary, uvx) or the review process.
- Status: GA
- Evidence: https://agentclientprotocol.com/get-started/registry.md; https://agentclientprotocol.com/updates.md; https://zed.dev/docs/ai/external-agents.md
- Notes: Install-type details unverified (needs agent.schema.json, not opened as it is a repo file). (verified in review: auth-only requirement, CDN URL, PR submission, 2026-03-09; the registry page itself names no distribution types)

## B14. SDKs
- What: Official: Rust (`agent-client-protocol` crate, has `Agent` and `Client` traits, powers Zed's external agents; also `agent-client-protocol-schema` for bare types), TypeScript (`@agentclientprotocol/sdk`), Python, Kotlin, Java. Rust and TypeScript reached 1.0 on 2026-06-25. Community libraries for Go (several), .NET, Swift, Dart, Ruby, C++, Crystal, Elixir, Emacs, React, Vala, Cangjie. Schema JSON in `schema/v1` and `schema/v2`.
- Status: GA (Rust, TS); others unclear
- Evidence: https://agentclientprotocol.com/libraries/rust.md; https://agentclientprotocol.com/libraries/community.md; https://github.com/agentclientprotocol/agent-client-protocol; https://agentclientprotocol.com/updates.md
- Notes: Rust and TypeScript SDK 1.0 on 2026-06-25 confirmed on updates.md; the SDK list itself was not re-checked.

## B15. How Zed hosts external agents (the client side in practice)
### External Agents in Zed
- What: Agents run as separate processes; Zed renders the thread. Install from ACP registry, or custom entry in `agent_servers` (name, `"type": "custom"`, command, args, env). Extension-provided agents deprecated and migrated. Agent owns auth, model, tools, billing. Zed profiles and Skills do not apply; Zed MCP servers are forwarded over ACP; tool permissions involve both Zed's ACP forwarding rules and the agent's. Named: Claude Agent, Codex, Gemini CLI, OpenCode, Copilot, Cursor, Pi, Poolside. Debug with `dev: open acp logs`. Steer unavailable for external agents.
- Status: GA
- Evidence: https://zed.dev/docs/ai/external-agents.md; https://zed.dev/docs/ai/agent-panel
- Notes: Registry/Zed behaviours moved a lot in the last month (ACP config defaults only for new conversations in 1.26.2; session notices and compaction in 1.25.2).

---

## 3. Distinctive or unusual

- **Per-tool regex permission lists with chain splitting** and a deliberately tiny hardcoded deny set: lets users express policy without the product claiming safety it cannot deliver (A5).
- **Permission "always" semantics left to the agent in ACP**: protocol standardises the prompt but not the policy store (B4).
- **Checkpoints removed after shipping** because Git snapshots stalled on large repos (A11): a documented negative result.
- **Sandboxing scoped honestly**: macOS Seatbelt, Linux Bubblewrap, Windows only via WSL, with a list of known side channels (A5).
- **Terminal Threads**: embed any CLI agent as a thread without protocol support (A4).
- **ACP registry with auth requirement** and client-driven install (B13).
- **ACP `terminal` auth method**: client runs the agent's own login flow in a terminal, without credentials passing through the protocol (B7).
- **Proxy chains and MCP-over-ACP** as an extension mechanism that could subsume hooks and AGENTS.md (B9).

## 4. Architecture and UX notes

- Zed Agent runs inside the desktop editor process; remote projects run the headless Zed server on the remote machine, but docs disagree on where model calls originate (A17).
- ACP: client launches the agent as a child process, JSON-RPC over stdio, one connection with many sessions; the **client** performs file and terminal operations, the **agent** runs the model loop and tools (B1, B5). Remote agents (HTTP/WebSocket) are only an RFD (B6).
- Agent-side tools in ACP agents typically map to ACP kinds, but the protocol does not require the client's tools to match the agent's own (tool calls are reports plus permission asks, not RPC invocations; B4).

## 5. Relevance to a self-hosted master/node harness

- **Node as ACP agent, browser/editor as client**: ACP's launch model is a child process on stdio, but the master/node split needs the unfinished remote transport (B6); until it stabilises, a node would have to wrap stdio over its own link.
- **Master as ACP client hosting other agents** (like Zed's External Agents, B15, B11): the 51-agent list and the registry (B13) mean one client implementation gives access to Claude, Codex, Gemini, Copilot, OpenCode and others, but many are adapters.
- **Invariant "the node decides"** fits ACP's split: the client does fs/terminal and must apply its own policy (B5, B8); a hosting master would have to forward those calls to the node and enforce node.toml there.
- **Permission model**: ACP's four option kinds are UI hints and the agent stores policy (B4); a harness that already has a node-side policy can map `allow_always` onto it. Zed's regex lists with chain splitting (A5) are a concrete design to compare against.
- **Plans, modes, config options, compaction, usage, notices, elicitation** are all first-class `session/update` types (B2, B3, B4): they cover most of what a web UI needs to fold from an event log, which matches the "UI is a fold over events" invariant.
- **Session resume/list/close/delete** (B3) line up with a persisted event log; `session/load` replays history, so a log-backed agent can implement it naturally.
- **Subagents** are not yet in ACP (B10); Zed shows them only as `spawn_agent` inside its native agent (A9).
- **Checkpoint lesson** (A11): Git-snapshot per prompt hurt large repos; relevant to the checkpoint design.
- **Security guidance in ACP** (B8) reads like a checklist for the "nothing the model writes is trusted" invariant (no model HTML, link host display, no automatic remote images).
- **Rust SDK** (B14) exists and powers Zed; licence Apache-2.0 per the repo (crate licence not separately opened).

## 6. Overhyped, stale or contradictory

- Remote development page: says the local machine talks to LLMs, and also that the remote uses its own proxy for LLM calls (A17).
- Instructions page: lists nine recognised project files, then says the Zed Agent loads personal and project `AGENTS.md`, `CLAUDE.md` and `copilot-instructions.md` (A7); unclear whether `.rules` etc. are still loaded.
- Docs say sandboxing without any maturity label; release notes mention it only as a bug fix (A5).
- Agent Panel docs say "no checkpoints", but 1.25.2 and 1.23.3 release notes still refer to them; 1.27.0 removed them (A11).
- Tool-permissions page does not say how it applies to external agents, though the external-agents page says both Zed's ACP forwarding and the agent's own model are involved (A5, B15).
- ACP home page says remote support is in progress; the agents page lists 51 agents but several are third-party, unofficial or adapter-based (e.g. "unofficial agent" note for Muse Spark Code).
- ACP v2 overview page does not mark what is new versus v1.

## 7. Unverified

- Exact licence file layout of the Zed repo and the licence of the Rust SDK crate: not re-opened. Open the repos' LICENSE files.
- Whether the ACP registry supports npx/binary/uvx distribution types: open https://github.com/agentclientprotocol/registry/blob/main/agent.schema.json.
- Text threads (Zed's older editable conversation format) and the rules library UI: not in the current docs index; may have been removed or hidden. Check the release notes history before the last 8 releases.
- Whether Zed Agent loads `.rules` and the other legacy files (see section 6).
- Which Zed version first shipped sandboxing, skills, parallel agents and terminal threads: not in the last 8 releases' notes; read older notes.
- Zed-hosted model list and per-token prices: https://zed.dev/docs/account/zed-hosted-models.md not opened.
- v1 vs v2 timing for stabilising v2: no date given.
- Who implements v2 or the unstable features in the wild: not documented.
- Several RFDs (session fork, session compaction, session notices, plan operations, end-turn usage, custom LLM endpoint, v2 background work): only titles seen in the index.
- ACP Registry review/vetting process: not documented on the page read.

## 8. Sources (all opened 2026-11-10)

Zed:
- https://zed.dev/docs/ai/overview (AI overview): feature map, link index
- https://zed.dev/docs/llms.txt (docs index): list of AI pages; absence of memory/text threads
- https://zed.dev/docs/ai/agent-panel (Agent Panel): threads, queue, review, compaction, privacy of ratings
- https://zed.dev/docs/ai/zed-agent.md (Zed Agent)
- https://zed.dev/docs/ai/agents.md (Agents): three paths
- https://zed.dev/docs/ai/external-agents.md (External Agents)
- https://zed.dev/docs/ai/terminal-threads.md (Terminal Threads)
- https://zed.dev/docs/ai/parallel-agents.md (Parallel Agents)
- https://zed.dev/docs/ai/tools.md (Tools)
- https://zed.dev/docs/ai/tool-permissions.md (Tool Permissions)
- https://zed.dev/docs/ai/sandboxing.md (Agent Sandboxing)
- https://zed.dev/docs/ai/agent-profiles.md (Agent Profiles)
- https://zed.dev/docs/ai/agent-settings.md (Agent Settings)
- https://zed.dev/docs/ai/mcp (Model Context Protocol)
- https://zed.dev/docs/ai/skills.md (Skills)
- https://zed.dev/docs/ai/instructions.md (Instructions)
- https://zed.dev/docs/ai/inline-assistant.md (Inline Assistant)
- https://zed.dev/docs/ai/edit-prediction.md (Edit Prediction)
- https://zed.dev/docs/ai/quick-start.md (AI Quick Start): `disable_ai`
- https://zed.dev/docs/ai/llm-providers.md; https://zed.dev/docs/ai/use-api-access.md; https://zed.dev/docs/ai/use-a-gateway.md; https://zed.dev/docs/ai/use-a-local-model.md; https://zed.dev/docs/ai/use-an-existing-subscription.md: providers
- https://zed.dev/docs/ai/privacy-and-security.md (AI Privacy); https://zed.dev/docs/ai/ai-improvement.md (Feedback and Training Data)
- https://zed.dev/docs/account/plans-and-pricing.md (Plans & Pricing)
- https://zed.dev/docs/remote-development.md (Remote Development)
- https://zed.dev/releases/stable (Stable releases 1.23.3 to 1.27.0): checkpoint removal, ACP fixes

ACP:
- https://agentclientprotocol.com/ (Introduction); https://agentclientprotocol.com/llms.txt (index)
- https://agentclientprotocol.com/get-started/architecture.md; .../related-protocols.md; .../agents.md; .../clients.md; .../registry.md
- https://agentclientprotocol.com/protocol/v1/overview.md; .../initialization.md; .../authentication.md; .../session-setup.md; .../prompt-turn.md; .../tool-calls.md; .../file-system.md; .../terminals.md; .../agent-plan.md; .../session-modes.md; .../session-config-options.md; .../slash-commands.md; .../elicitation.md; .../transports.md; .../security.md
- https://agentclientprotocol.com/protocol/v2/overview.md; https://agentclientprotocol.com/announcements/acp-v2-draft.md
- https://agentclientprotocol.com/updates.md (dated changes)
- https://agentclientprotocol.com/rfds/streamable-http-websocket-transport.md; .../subagents.md; .../mcp-over-acp.md; .../proxy-chains.md
- https://agentclientprotocol.com/libraries/rust.md; .../libraries/community.md
- https://agentclientprotocol.com/community/governance.md
- https://github.com/agentclientprotocol/agent-client-protocol (README, licence, SDK list)

## 9. Review

Date: 2026-11-10. Entries checked: 31 of about 75 (Part A: 11 entries; Part B: 20). All priority items in Section 3 and the Section 5 bullets were traced to their entries.

| Entry | Verdict | Reason |
|---|---|---|
| Tool permissions | confirmed | keys, default confirm, precedence, MCP pattern format, v0.224.0 note all match; hardcoded rules apply to the terminal tool only |
| Agent sandboxing | confirmed | covers `terminal` and `fetch` only; Seatbelt / Bubblewrap / WSL; no maturity label; excludes external agents and Terminal Threads |
| Checkpoints removed | confirmed-with-correction | removed in 1.27.0, Nov 04 2026, for slowness or hangs in large Git repos; Agent Panel page does not carry a removal note, it is silent |
| Terminal Threads | confirmed | closed not archived, own auth/MCP/skills, `terminal_init_command` |
| Queue and steer | confirmed | Zed Agent only |
| Tool list | confirmed | 16 tools, `search_web` Pro only |
| Auto compaction / subagent model | confirmed | 90% default is on Agent Settings, not Agent Panel |
| Instructions | confirmed-with-correction | first-match rule and order of nine files, `.rules` still supported |
| ACP versions (v1 stable, v2 Draft 2026-07-20) | confirmed | announcement page |
| Permission requests | confirmed | "advisory" (security page); agent stores `allow_always`; kind is a UI hint |
| File system and terminals | confirmed | client performs I/O and runs commands, applies own policy |
| Remote transport | confirmed | RFD status Active, stdio is the only standardised transport; goose implementation exists (see goose.md) |
| MCP-over-ACP | confirmed | draft, MCP 2026-07-28 only |
| Proxy chains | not opened | related-protocols page does not mention it; left as is, flagged unverified in its Notes |
| Subagents RFD | confirmed | revised 2026-11-09, `unstable_subagents`, observable but not promptable, cancel only |
| Agents list | confirmed | 51 agents; several adapter-based |
| Clients list | confirmed-with-correction | 142 clients in the page's six categories (21 editors/IDEs) |
| Registry | confirmed | auth-only, CDN URL, 2026-03-09 |
| Dates in updates.md | confirmed | $/cancel_request 06-29, elicitation 07-22, system prompt 11-06, SDK 1.0 06-25, resume/close/delete, tool names 09-17 |
| Governance | confirmed | Zed and JetBrains, interim, Ben Brandt and Sergey Ignatov |
| Tool-call kinds and permission kinds | confirmed | lists match |

Borrowed features: none found. The `claude-agent-acp` / `codex-acp` adapter binary names in B11 come from goose docs, not the ACP agents page, which says "Zed's SDK adapter" and "ACP's adapter".

Notable omissions (from https://zed.dev/docs/llms.txt and the ACP site):
1. Zed-Hosted Models page with prices and tool-call support: https://zed.dev/docs/account/zed-hosted-models.md (survey lists it unverified).
2. Agent Server Extensions: https://zed.dev/docs/extensions/agent-servers.md (survey says extension-provided agents are deprecated; this page was not read).
3. AI by Company: https://zed.dev/docs/ai/by-company.md.
4. ACP RFD index items only seen as titles (session fork, notices, plan operations, custom LLM endpoint): https://agentclientprotocol.com/llms.txt.
5. Zed collaboration docs (channels, private calls): https://zed.dev/docs/collaboration/overview.md, not AI-specific.

Trust judgement: reliable. No wrong or unsupported claim was found in the 31 entries; the corrections are small (a nonexistent "page note", a client count, the first-match rule). Remaining risk is in unchecked entries (providers, privacy and training-data terms, remote development, plans and pricing, ACP session details), which cite pages I did not open; treat those as probably right but unverified. The survey's own caveats (title-only RFDs, installation types) are honest and stay in force.
