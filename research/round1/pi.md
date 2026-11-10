# Pi (pi coding agent) - Round 1 public-surface survey

Surveyed 2026-11-10. Public docs only (pi.dev docs, GitHub READMEs, changelog, author blog posts). No source code read. Entries marked "page text read via fetch tool" mean the page was opened and summarised by a fetch tool; where a page was truncated this is stated.

## 1. Identity

- Name: Pi ("pi coding agent"), described as a minimal, extensible agent harness. Vendor/owner: Earendil (the post says "the company"; the "Inc." suffix was not found in review). Creator and lead: Mario Zechner.
- Repository: https://github.com/earendil-works/pi (confirmed by opening it; description "An AI agent toolkit with a unified LLM API, agent loop, TUI, and coding agent CLI"). Shows about 121k stars, 15.6k forks, 7,767 commits on `main`. The old home `badlogic/pi-mono` is gone: Zechner's own post (2026-04-08) states the move to `earendil-works/pi` and that Earendil owns Pi, the name and logo are trademarked by Earendil. 00-targets.md is correct on the move and owner.
- Surfaces: interactive TUI, print mode, JSON-lines event mode, RPC mode (stdin/stdout JSONL), TypeScript SDK. Separate extension repo `earendil-works/pi-chat` (Discord/Telegram bridge).
- Open source: MIT core ("MIT, forever" per the author's post). The post announces future tiers: Fair Source (delayed open source) value-add features and proprietary enterprise/cloud features; the post says tiers 2 and 3 are not yet built.
- Latest version seen: changelog lists 1.5.2 on 2026-11-10 (matches 00-targets.md). Version 1.0.0 was 2026-10-01 per the changelog.
- Install: shell installer (macOS/Linux, PowerShell for Windows), npm (Node.js 22.19+), Nix flake. Package scope on npm is `@earendil-works/...` per README and docs. The April blog post said the scope would be `@earendil/pi`; the docs now say otherwise (see section 6).
- Contribution policy: README says new issues and PRs from new contributors are auto-closed by default; maintainers review closures daily.

## 2. Features by area

### 1. Agent loop and execution model

### Turn loop
- What: A submitted message is appended to the active branch; Pi builds a request from system prompt, branch history, tool definitions and model settings, streams the reply, records it, runs every tool call and records results (one turn). Further turns run while tool results or queued messages need another request.
- Status: GA
- Evidence: https://pi.dev/docs/latest/how-pi-works (Agent loop)
- Notes: v1.5.2 docs.

### Steering and follow-up queues
- What: While the agent runs, Enter sends a steering message (delivered after the current assistant turn, interrupting remaining tools per the homepage); Alt+Enter queues a follow-up delivered after pending work finishes; Alt+Up pulls queued messages back to the editor; Escape aborts and returns queued messages to the editor. Settings `steeringMode` and `followUpMode` default `one-at-a-time` (alternative `all` in agent-core).
- Status: GA
- Evidence: https://pi.dev/docs/latest/usage (Queuing and steering); https://pi.dev/docs/latest/how-pi-works; https://pi.dev/docs/latest/settings; https://github.com/earendil-works/pi/blob/main/packages/agent/README.md
- Notes: Windows/WSL default follow-up key is Ctrl+Q (https://pi.dev/docs/latest/keybindings).

### Run-settled signalling
- What: `agent_end` does not mean Pi is done (retries, compaction or queued work may follow); `agent_settled` signals nothing more will run automatically. Documented for RPC, JSON and SDK clients.
- Status: GA
- Evidence: https://pi.dev/docs/latest/rpc (Caveats); https://pi.dev/docs/latest/json; https://pi.dev/docs/latest/sdk
- Notes: Matters for any wrapper that drives Pi.

### Parallel tool execution
- What: Tool calls from one assistant message run in parallel by default; a tool can opt out with `executionMode: "sequential"` (one sequential tool forces its batch sequential). A tool result with `terminate: true` skips the follow-up model call only if every result in the batch sets it.
- Status: GA
- Evidence: https://pi.dev/docs/latest/extensions (Tool authoring); https://github.com/earendil-works/pi/blob/main/packages/agent/README.md
- Notes: Extensions doc was read in two parts (page is ~158k chars).

### Agent retry
- What: Agent-level retry is enabled by default: 3 retries, 2000 ms base delay, 60000 ms max; provider-level retries default 0; http idle timeout 300000 ms.
- Status: GA
- Evidence: https://pi.dev/docs/latest/settings (Retries and network)
- Notes: Defaults as documented on 2026-11-10. (verified in review: 3, 2000 ms, 60000 ms)

### 2. Built-in tools

### Built-in tool set
- What: Built-in tools are `read`, `bash`, `powershell` (Windows only), `edit`, `write`, `grep`, `find`, `ls`. Default active set is `read`, `bash`, `edit`, `write` (setting `defaultTools`; entries can replace, or add/remove with `+name`/`-name`). The other four are off by default (consistent with the 2025 blog post).
- Status: GA
- Evidence: https://pi.dev/docs/latest/cli (Tool flags); https://pi.dev/docs/latest/settings (Tools); https://mariozechner.at/posts/2025-11-30-pi-coding-agent/
- Notes: The pages opened give no per-tool parameter schemas or behaviour (for example how `edit` matches text); `nothing documented` in the pages read. Per-tool docs may exist elsewhere (see Unverified).

### Tool selection flags
- What: `-t/--tools` allowlist with `*` patterns and `mcp__` entries; `-xt/--exclude-tools`; `-nbt/--no-builtin-tools` (keep extension tools); `-nt/--no-tools` (disable everything including MCP).
- Status: GA
- Evidence: https://pi.dev/docs/latest/cli (Flags)
- Notes: `*` patterns added in 1.0.4 (https://pi.dev/changelog). (verified in review: flags and 1.0.4; the CLI page says codemode and tool_search are off by default, enabled via `defaultTools: ["+codemode"]` or `--tools +codemode`, while the settings page lists `codemode.mode` default `on` and both builtins loaded by default: the contradiction in section 6 is real)

### Tool output truncation
- What: Tools are expected to truncate output; the documented built-in limit is 50KB or 2000 lines, whichever comes first, with `truncateHead` (reads/search) and `truncateTail` (logs/command output) helpers for extension authors.
- Status: GA
- Evidence: https://pi.dev/docs/latest/extensions (Output limits)
- Notes: Applies to extension-authoring guidance; the page frames it as a requirement on tools.

### Tool override and pluggable operations
- What: Extensions can replace built-in tools by reusing their names. Built-in tools accept pluggable operations (for SSH, containers and similar) and bash/powershell accept a spawn hook that can change command, cwd or environment. Bash exposes session variables (session ID, model etc.) unless disabled.
- Status: GA
- Evidence: https://pi.dev/docs/latest/extensions (Overrides and remote execution); https://pi.dev/docs/latest/environment-variables
- Notes: `PI_SESSION_ID`, `PI_SESSION_FILE`, `PI_PROVIDER`, `PI_MODEL`, `PI_REASONING_LEVEL` set for shell tool commands; `AI_AGENT=pi` and `PI_CODING_AGENT=true` set for child processes.

### Codemode tool
- What: A `codemode` tool where the model writes raw JavaScript that calls other tools (`tools.<name>(args)`) and non-LLM models (classifiers, image models); only the script's output returns to the model. Runs in QuickJS with no Node APIs, file system, network or timers; 256 MB per VM; at most two workers per Pi process; output cap and store limits documented; failed scripts do not roll back tool calls.
- Status: GA (no status stated; shipped in 0.99.0 and refined through 1.x)
- Evidence: https://pi.dev/docs/latest/codemode; https://pi.dev/changelog
- Notes: Docs disagree on default state; see section 6. (verified in review: QuickJS, 256 MB, two workers, no Node APIs/fs/network/timers, BM25 limit 8, added in 0.99.0 on 2026-09-29; the codemode page does not state a default. The clause "failed scripts do not roll back tool calls" and the "output cap and store limits" detail were not found on the page and should be treated as unverified. corrected in review: removed claim from verified set)

### Tool search
- What: A `tool_search` extension finds and declares deferred tools on demand. MCP tools use an exposure mode (`codemode` default, `deferred`, `direct`, `hidden`).
- Status: GA
- Evidence: https://pi.dev/docs/latest/cli (Tool flags); https://pi.dev/docs/latest/mcp (Tool exposure)
- Notes: The codemode page itself does not mention a `tool_search` tool, only `searchTools()` (BM25, default limit 8).

### 3. Code editing and file operations

### Edit and write tools
- What: `edit` and `write` are default tools; the model edits through them. Extensions writing files are told to use `withFileMutationQueue()` to avoid lost updates from parallel tool calls.
- Status: GA
- Evidence: https://pi.dev/docs/latest/settings (Tools); https://pi.dev/docs/latest/extensions
- Notes: Mechanism of `edit` not described in pages opened. Image inputs: auto-resize defaults 2000x2000 px, 4.5 MiB encoded, JPEG quality 80 (https://pi.dev/docs/latest/models, https://pi.dev/docs/latest/settings).

### File mentions and attachments
- What: `@` in the editor searches and attaches files; Tab completes paths; images can be pasted or dragged (terminal-dependent); `pi @file prompt` includes a text file or image in the first prompt; piped stdin is prepended to the first prompt.
- Status: GA
- Evidence: https://pi.dev/docs/latest/usage; https://pi.dev/docs/latest/cli (Input, files, and stdin)
- Notes: RPC mode rejects `@file` arguments.

### 4. Terminal and shell integration

### Shell escape (! and !!)
- What: In the editor, `!cmd` runs a shell command and includes its output in the conversation; `!!cmd` runs it without sending output to the model.
- Status: GA
- Evidence: https://pi.dev/docs/latest/usage (Shell commands)
- Notes: `user_bash` is an extension event.

### Shell selection and Windows
- What: Default shell is Bash; on native Windows Pi looks for Git Bash (setting `shellPath`, Git for Windows locations, PATH). An optional `powershell` tool exists on native Windows (prefers pwsh.exe, no profile, non-interactive, execution policy bypassed unless admin-enforced). Settings `shellPath`, `shellCommandPrefix`.
- Status: GA
- Evidence: https://pi.dev/docs/latest/windows; https://pi.dev/docs/latest/settings
- Notes: `!` commands still use Bash on Windows.

### Terminal requirements and multiplexers
- What: Needs extended-key support to distinguish Shift+Enter/Alt+Enter; docs give tmux config (`extended-keys on`, csi-u for tmux 3.5+) and list terminals that work (Kitty, Ghostty, WezTerm, iTerm2, Windows Terminal) and ones that do not (xfce4-terminal, Terminator, IntelliJ terminal). Termux (Android) supported without image paste.
- Status: GA
- Evidence: https://pi.dev/docs/latest/terminal-setup; https://pi.dev/docs/latest/tmux; https://pi.dev/docs/latest/termux
- Notes: The tmux page gives no guidance on background processes (consistent with the author's "use tmux" stance in the blog).

### 5. Permissions, approvals and sandboxing

### No built-in permission system (documented non-feature)
- What: Pi runs with the OS permissions of the launching account and does not ask approval before each tool call. Extensions and child processes inherit the same permissions. The working folder does not restrict access. The docs state that watching the transcript, project trust and reviewing changes are not security boundaries.
- Status: GA (deliberate design)
- Evidence: https://pi.dev/docs/latest/security; https://pi.dev/docs/latest/quickstart; https://github.com/earendil-works/pi (README)
- Notes: Reason given by the author (2025 blog): permission prompts and safety checks are largely security theatre once an agent can read data, run code and reach the network; most users run permissive modes anyway. Alternative proposed: run in a container. The 2025 post is older than the current docs; the security page itself gives no rationale beyond "out of scope". (verified in review: README, security page, 2025 post; the homepage gives container or an extension-built confirmation flow as substitutes and no reason)

### Isolation patterns the docs propose instead
- What: (1) Plain Docker, whole Pi process in a container (`docker run` with workspace mount; warns that passed env vars are visible and a mounted `~/.pi/agent` exposes credentials). (2) Docker Sandboxes (`sbx`), where a proxy keeps the real provider key on the host and the sandbox sees a placeholder; do not `/login` inside it. (3) OpenShell, a sandbox with filesystem, process, network, credential and inference policies; inference routing can keep model credentials outside. (4) Gondolin extension: Pi stays on the host, built-in tools and `!` commands are routed into a local Linux micro-VM (QEMU, Node 23.6+); VM inherits host env vars so it is not a credential boundary.
- Status: GA (docs); Gondolin is an example extension
- Evidence: https://pi.dev/docs/latest/containerization; https://pi.dev/docs/latest/security
- Notes: Key distinction in the docs: Docker/OpenShell contain the whole process including extensions; Gondolin contains only built-in tools.

### Project trust
- What: A trust decision gates project-local resources before they load: `.pi` settings, MCP config, extensions, skills, prompts, themes, system-prompt files, project `.agents/skills`. Resolution order: CLI override, extensions handling `project_trust`, saved decisions in `~/.pi/agent/trust.json`, then `defaultProjectTrust` (default `ask`, settable only in agent-dir settings). In print/JSON/RPC modes `always` loads, `ask`/`never` skip; `--approve`/`--no-approve` override.
- Status: GA
- Evidence: https://pi.dev/docs/latest/security (Project trust); https://pi.dev/docs/latest/cli
- Notes: Docs say trust is not a full boundary: `sessionDir` is read before trust, context files (`AGENTS.md`, `CLAUDE.md`) load regardless of trust, and trust does not limit tool calls. (verified in review: resolution order, `ask` default agent-dir only, trust.json, headless behaviour, `--approve`/`--no-approve`)

### Permission gates via extensions
- What: Extensions can block or mutate tool calls (`tool_call` event), patch results, and expose MCP-style annotations (read-only/destructive hints). Example extensions `permission-gate.ts`, `protected-paths.ts` are listed. Community packages such as `@gotgenes/pi-permission-system` and `cc-safety-net` exist in the gallery.
- Status: GA (mechanism); permission systems are third-party
- Evidence: https://pi.dev/docs/latest/extensions (Examples index); https://pi.dev/packages
- Notes: A failing `tool_call` handler blocks that tool call.

### Vulnerability scope
- What: The security page says expected local-agent behaviour, prompt injection, lack of a sandbox and user-installed extension behaviour are generally out of scope for vulnerability reports unless a privilege-boundary bypass is shown.
- Status: GA
- Evidence: https://pi.dev/docs/latest/security
- Notes: Reports go through SECURITY.md.

### 6. Context management and compaction

### Auto and manual compaction
- What: Compaction triggers when context exceeds window minus `reserveTokens` (default 16384); checked between tool batches and turns, before prompts, and after overflow errors (compact and retry once). It walks back from the newest message to keep about `keepRecentTokens` (default 20000), summarises the older part, and writes a compaction entry; later requests use summary plus kept messages. Cuts never land on tool results; an oversize single turn is cut internally and gets two merged summaries. Summary sections: Goal, Constraints and Preferences, Progress, Key Decisions, Next Steps, Critical Context, plus read/modified file lists. Thinking blocks omitted and tool results truncated to 2000 chars when summarising. `/compact [instructions]` triggers it manually; `modelOverrides` set per-model thresholds.
- Status: GA
- Evidence: https://pi.dev/docs/latest/compaction; https://pi.dev/docs/latest/settings
- Notes: Original entries stay in the session tree. Extensions can cancel or replace summaries (`session_before_compact`). (verified in review: reserveTokens 16384 and keepRecentTokens 20000 defaults on the settings page; summary format not re-checked)

### Branch summarization
- What: When navigating with `/tree`, Pi can summarise the branch being left and attach the summary to the branch entered; reserve 16384 tokens by default.
- Status: GA
- Evidence: https://pi.dev/docs/latest/compaction; https://pi.dev/docs/latest/sessions
- Notes: Extension hook `session_before_tree`.

### Context edits and context hooks
- What: Session format has an append-only `context_edit` entry type that changes future model context for an earlier entry without modifying it. Extensions get `context`, `context_with_system`, `before_provider_request` hooks to transform what the model sees.
- Status: GA
- Evidence: https://pi.dev/docs/latest/session-format; https://pi.dev/docs/latest/extensions
- Notes: How `context_edit` is created by users is not described in pages read.

### Prompt cache controls
- What: Settings `cacheWarming` (default `idle`, global-only) and `showCacheMissNotices` (default false); models can configure `short`/`long` cache lifetimes; changelog 1.5.1 added idle cache warming; MCP summary design is described as keeping earlier messages cached when server summary changes.
- Status: GA
- Evidence: https://pi.dev/docs/latest/settings; https://pi.dev/docs/latest/models; https://pi.dev/changelog; https://pi.dev/docs/latest/mcp
- Notes: Mechanism of idle warming not described in the pages read.

### 7. Memory and persistent instructions

### Context files (AGENTS.md / CLAUDE.md)
- What: `AGENTS.md`, `CLAUDE.md` and variants load from the agent dir, the working dir and its parent dirs; a file applies to its own directory and below; `AGENTS.override.md` replaces `AGENTS.md`/`CLAUDE.md` only in the same directory. Loaded without project trust. `-nc/--no-context-files` disables.
- Status: GA
- Evidence: https://pi.dev/docs/latest/configuration (Context files); https://pi.dev/docs/latest/security
- Notes: Security note: loaded regardless of trust.

### SYSTEM.md and APPEND_SYSTEM.md
- What: `SYSTEM.md` replaces the default system prompt, `APPEND_SYSTEM.md` appends; both in agent dir and `.pi`; trusted project file wins and files are never merged. CLI: `--system-prompt`, `--append-system-prompt` (repeatable).
- Status: GA
- Evidence: https://pi.dev/docs/latest/configuration; https://pi.dev/docs/latest/cli
- Notes: Author's 2025 post says default prompt plus tool definitions are under 1,000 tokens (post, not current docs; may have grown).

### Automatic memories
- What: nothing documented in the core. The `pi-chat` extension adds account-wide and channel-specific memory files for chat use.
- Status: unclear
- Evidence: https://pi.dev/docs/latest/configuration; https://github.com/earendil-works/pi-chat
- Notes: Checked configuration, how-pi-works, quickstart; no memory feature found in core.

### 8. Planning, tasks and modes

### No plan mode, no built-in todos (documented non-features)
- What: Homepage lists plan mode and built-in to-dos as deliberately omitted. Author's reasons (2025 post): plans belong in files like PLAN.md that can be read, edited, versioned and shared across sessions; where planning needs restrictions, limit the tool set. To-dos: model-tracked state is something else to get wrong; use a TODO.md with checkboxes.
- Status: GA (deliberate omission)
- Evidence: https://pi.dev/ (non-features list); https://mariozechner.at/posts/2025-11-30-pi-coding-agent/
- Notes: (corrected in review: the 2025 post's to-do reason is that to-do lists generally confuse models more than they help, not "state to get wrong"; the plan-mode reason also includes wanting to see what the agent reads and avoiding approval of many commands; the homepage itself gives no reasons, only substitutes) Docs propose extensions/packages. Example `plan-mode/` extension and `todo.ts` ship in the examples index (https://pi.dev/docs/latest/extensions); gallery lists `@plannotator/pi-extension` (plan review) and `@juicesharp/rpiv-todo` (https://pi.dev/packages).

### Presets and tool loadouts
- What: Example extensions `preset.ts` and `tools.ts` exist; extensions can activate/deactivate tools (`setActiveTools`) and `prepareLoadout` adjusts what the model sees.
- Status: GA (examples)
- Evidence: https://pi.dev/docs/latest/extensions
- Notes: Only names and one-line group labels seen; contents not opened.

### 9. Sub-agents and multi-agent orchestration

### No built-in sub-agents (documented non-feature)
- What: Listed as deliberately omitted. Author's reasons (2025 post): sub-agents are a black box with poor context transfer and hard to debug; prefer gathering context in a separate session saved as an artifact; where useful (code review) Pi spawns itself through bash so output stays visible.
- Status: GA (deliberate omission)
- Evidence: https://pi.dev/; https://mariozechner.at/posts/2025-11-30-pi-coding-agent/
- Notes: Docs propose extensions or packages.

### Example subagent extension
- What: Ships as an example: each subagent is a separate `pi` process with its own context; modes single, parallel (max 8 tasks, 4 concurrent) and chain (`{previous}` placeholder); agents defined as markdown with frontmatter in `~/.pi/agent/agents/` (user) or `.pi/agents/` (project, opt-in scope); inherits parent model if none set.
- Status: GA (example extension)
- Evidence: https://github.com/earendil-works/pi/tree/main/packages/coding-agent/examples/extensions/subagent
- Notes: Gallery lists several sub-agent packages (`pi-subagents`, `@tintinweb/pi-subagents`, etc.) (https://pi.dev/packages).

### Subagents in pi-durable
- What: The experimental durable runtime lists subagents and child tasks owned by parent tasks with abort propagation.
- Status: experimental
- Evidence: https://github.com/earendil-works/pi/blob/main/packages/durable/README.md
- Notes: README warns the API changes without notice. (corrected in review: aborting the parent aborts the child, but background-owned work survives the parent's abort)

### 10. Background, long-running and scheduled work

### No background bash (documented non-feature)
- What: Listed as omitted. Author's reasons (2025 post): background process management adds tracking, buffering and cleanup complexity, and another harness lost track of processes after compaction. Alternative: tmux for long-running and interactive processes.
- Status: GA (deliberate omission)
- Evidence: https://pi.dev/; https://mariozechner.at/posts/2025-11-30-pi-coding-agent/
- Notes: https://pi.dev/docs/latest/tmux is only about key configuration.

### Scheduled tasks
- What: nothing documented in core pages read.
- Status: unclear
- Evidence: https://pi.dev/docs/latest; https://pi.dev/docs/latest/how-pi-works
- Notes: pi-durable mentions sleeping tasks that wake on a timer or host alarm (experimental): https://github.com/earendil-works/pi/blob/main/packages/durable/README.md

### Durable, crash-surviving runs (pi-durable)
- What: An experimental harness whose transcripts, model turns, tool calls and state are committed to storage (memory, IndexedDB, SQLite, JSONL) before being shown; tasks checkpoint each step and resume after crash with a restart limit; also forks, handoffs, background/threshold compaction, task-graph view.
- Status: experimental
- Evidence: https://github.com/earendil-works/pi/blob/main/packages/durable/README.md
- Notes: Built on `pi-ai` and `chord`. Relationship to the coding-agent CLI not stated in pages read. (verified in review: experimental warning, four backends, commit-before-display, checkpoint per step, restart limit default 5, sleeping tasks, handoffs, forks)

### 11. Sessions

### Session storage as a tree
- What: Sessions auto-save as JSONL under `~/.pi/agent/sessions/` grouped by directory (override with `--session-dir`, env var or `sessionDir` setting); each entry has `id` and `parentId`; the current leaf is the active branch. Entry types: message, model_change, thinking_level_change, compaction, context_edit, branch_summary, custom, custom_message, label, session_info, usage. Format versions v1 linear, v2 tree, v3; old files auto-migrate. `--no-session` is in-memory.
- Status: GA
- Evidence: https://pi.dev/docs/latest/sessions; https://pi.dev/docs/latest/session-format; https://pi.dev/docs/latest/how-pi-works
- Notes: The format is documented for post-processing (an author goal).

### Resume, continue, fork, clone, tree
- What: `-c` resumes latest in cwd; `-r`//resume picker; `--session <path|id>`, `--session-id`, `--fork`. `/tree` navigates inside one file keeping all branches (selecting a user message loads it into the editor); `/fork` creates a new file from an earlier user message; `/clone` copies the active branch. A session resumes in its original cwd; if gone, options are continue here, fork, or cancel. Double Escape action defaults to `tree`.
- Status: GA
- Evidence: https://pi.dev/docs/latest/sessions; https://pi.dev/docs/latest/cli; https://pi.dev/docs/latest/settings
- Notes: 1.5.2 "fork sessions whose folder is gone" (https://pi.dev/changelog).

### Export and share
- What: `/export` writes HTML or JSONL; `pi --export <input> [output]` exports to HTML; `/share` uploads and returns a viewer link, via a Radius artifact when configured or a private GitHub gist; `sessionShareDestination` forces one. A separate tool `pi-share-hf` publishes sessions to Hugging Face (README mention only).
- Status: GA
- Evidence: https://pi.dev/docs/latest/sessions; https://pi.dev/docs/latest/cli; https://github.com/earendil-works/pi
- Notes: Docs warn exports can contain prompts, file contents and tool arguments. (verified in review: /share destination, sessionShareDestination, /export, session dir, tree/fork/clone, missing-cwd options)

### Checkpoints and undo
- What: No built-in file checkpoint/undo documented. An example extension `git-checkpoint.ts` and `auto-commit-on-exit.ts` appear in the examples index; docs advise version control or backups.
- Status: unclear
- Evidence: https://pi.dev/docs/latest/extensions (Examples index); https://pi.dev/docs/latest/quickstart
- Notes: Example names only; contents not opened.

### 12. Git and version control

### Git integration
- What: nothing documented as a built-in feature beyond the footer showing the folder/session (Windows page mentions "Git branch polling" under WSL caveats). Examples `git-checkpoint.ts` and `auto-commit-on-exit.ts` exist. No worktree, PR or review feature documented.
- Status: unclear
- Evidence: https://pi.dev/docs/latest/usage; https://pi.dev/docs/latest/windows; https://pi.dev/docs/latest/extensions
- Notes: Absence is a finding; the docs recommend version control as the safety net.

### 13. Extensibility

### Extensions
- What: TypeScript modules loaded in-process (via jiti, no compile step) from `~/.pi/agent/extensions/`, `.pi/extensions/` (trusted projects), settings paths or packages; a default-exported factory gets `ExtensionAPI`. Can register tools (TypeBox/schema), slash commands, shortcuts, CLI flags, message/entry renderers, model providers (with OAuth and dynamic discovery), virtual models, MCP servers, custom TUI (dialogs, widgets, footers, headers, editors, overlays). About 70 examples in the examples index.
- Status: GA
- Evidence: https://pi.dev/docs/latest/extensions (read in two parts)
- Notes: Extensions run with the full permissions of the Pi process; the docs say to load only trusted ones.

### Extension event surface
- What: Documented events cover input, run lifecycle (`agent_start`, `agent_before_settle`, `agent_settled`, turns), model requests (`context`, `before_provider_request`, `after_provider_response`, `provider_stream_event`), tools (`tool_call`, `tool_result`, execution events), sessions (start, shutdown, switch, fork, tree, compaction), startup (`project_trust`, `resources_discover`), and `model_select`, `thinking_level_select`, `user_bash`, `mcp_servers_change`, `cache_warming_decision`. Handlers run in load order; some gate, some chain.
- Status: GA
- Evidence: https://pi.dev/docs/latest/extensions (Events)
- Notes: Extension errors are logged and the agent continues, except a failing `tool_call` handler blocks that call.

### Skills
- What: A skill is a folder with `SKILL.md` (frontmatter `name`, `description`, optional `license`, `compatibility`, `metadata`, `allowed-tools` (experimental), `disable-model-invocation`). Names and descriptions are listed in the system prompt; full text loads on demand or via `/skill:name`. Scanned from user and project skill dirs plus `~/.agents/skills/` and `.agents/skills/` (project dirs searched up to the repo root). Name max 64, description max 1024; duplicates keep the first with a warning.
- Status: GA
- Evidence: https://pi.dev/docs/latest/skills
- Notes: The `.agents/skills` location and `SKILL.md` format follow a shared convention with other harnesses; I did not verify a named standard on this page.

### Prompt templates
- What: Markdown files in `prompts/` become `/name` commands; optional frontmatter `description` and `argument-hint`; arguments `$1`, `$@`, `$ARGUMENTS`, `${1:-default}`, `${@:N}`, `${@:N:L}`; shell-like quoting.
- Status: GA
- Evidence: https://pi.dev/docs/latest/prompt-templates
- Notes: Project templates require trust.

### Themes
- What: JSON theme files (`name`, `colors`, optional `vars`, `appearance`, `export`); colours as hex, OKLCH, OKHSL, 256-index or variable references; built-ins `system` (default, derives from terminal palette), `dark`, `light`; light/dark pairs; hot reload of active user theme.
- Status: GA
- Evidence: https://pi.dev/docs/latest/themes
- Notes: 1.2.0 moved the theme schema URL (breaking, https://pi.dev/changelog).

### Pi packages and distribution
- What: A package bundles extensions, skills, prompts and themes as an npm package, git repo or local dir (`pi install`, `pi remove`, `pi list`, `pi update`, `pi config`, `pi -e`). Install scope user (`~/.pi/agent/settings.json`) or project (`-l`, needs trust). Versioned npm specs and git tags/commits pin. A `pi` key in `package.json` or conventional dirs declares contents; `pi-package` keyword lists it in the gallery. Gallery at pi.dev/packages shows 5,592 packages (at read time) with type filters and download counts.
- Status: GA
- Evidence: https://pi.dev/docs/latest/packages; https://pi.dev/packages
- Notes: Docs warn packages execute code and skills can instruct the model to run programs; no vetting or signing described.

### MCP (built-in client)
- What: Built-in as a default extension (`builtin:mcp`). stdio and streamable HTTP (SSE not supported), OAuth (including dynamic registration and client-ID metadata documents), `pi mcp add|remove|list|login|logout`. Config: `~/.pi/agent/mcp.json`, project `.pi/mcp.json` (trust required). Per-server/tool exposure: `codemode` (default), `deferred` (via tool_search), `direct`, `hidden`. Resource tools `list_mcp_resources`, `list_mcp_resource_templates`, `read_mcp_resource`; MCP Apps (`ui://`) not rendered. MCP calls pass through the normal tool pipeline so extension gates apply. Can be disabled or replaced (`--no-mcp`, `pi-mcp-adapter`).
- Status: GA
- Evidence: https://pi.dev/docs/latest/mcp; https://pi.dev/changelog (0.99.0 added MCP)
- Notes: (verified in review: transports, SSE rejected, exposure modes and `codemode` default, config paths, `--no-mcp`, SDK sessions, MCP Apps; the MCP page gives no version, the changelog says MCP and codemode arrived in 0.99.0 on 2026-09-29) This reverses the 2025 position (author's post argued against MCP because tool descriptions bloat context); the codemode/deferred defaults are the answer to that. The MCP docs page does not state its own rationale. SDK sessions do not load it by default.

### Hooks and custom commands
- What: No separate hooks system; the extension event surface plays this role, and prompt templates and skills provide custom commands.
- Status: GA
- Evidence: https://pi.dev/docs/latest/extensions; https://pi.dev/docs/latest/prompt-templates
- Notes: No shell-command-hook config file documented in pages read.

### Customization chooser
- What: The quickstart gives a ladder: AGENTS.md for persistent folder instructions, prompt template for reusable prompts, skill for task instructions plus files, extension for executable tools/commands/handlers, custom provider for unsupported model services, package to distribute.
- Status: GA
- Evidence: https://pi.dev/docs/latest/quickstart
- Notes: Quickstart does not mention sub-agents, plan mode, permissions or todos.

### TypeScript SDK
- What: `@earendil-works/pi-coding-agent` embeds Pi in Node.js or Bun: `createAgentSession()`, `session.prompt()`, `steer()`, `followUp()`, `subscribe()`, `abort()`, `dispose()`; options for cwd, model, thinking, settings manager, session manager (persistent or in-memory), resource loader, tools. `AgentSessionRuntime` handles new/switch/fork/import. Codemode, tool search and MCP are not loaded in SDK sessions unless added.
- Status: GA
- Evidence: https://pi.dev/docs/latest/sdk
- Notes: Docs say to use the CLI integration for non-TypeScript or isolated processes.

### RPC mode
- What: `pi --mode rpc` is a long-lived process speaking JSONL on stdin/stdout (LF framing, U+2028/2029 warning). Commands include prompt, steer, follow_up, abort, session commands (new_session, switch_session, fork, clone, get_tree, get_entries, export_html...), model/thinking commands, compaction, `bash`, and `get_commands`. Extension UI dialogs travel via `extension_ui_request`/`extension_ui_response`. Orderly shutdown on stdin close/SIGTERM.
- Status: GA
- Evidence: https://pi.dev/docs/latest/rpc
- Notes: Commands carry optional ids; responses can arrive out of order.

### JSON event mode and print mode
- What: `pi -p` prints the final text; `pi --mode json` emits JSONL: a `session` header then events (agent/turn/message/tool/queue/compaction/retry events). Redirected stdin/stdout without a mode flag selects print mode.
- Status: GA
- Evidence: https://pi.dev/docs/latest/json; https://pi.dev/docs/latest/cli
- Notes: Doc gives a `jq` example for extracting `message_end` events.

### Packages in the monorepo (public surface)
- What: Twelve directories under `packages/`: agent, ai, chord, codemode, coding-agent, durable, env, evals, mcp, schema, telemetry, tui. The README table names seven (chord, pi-telemetry, pi-ai, pi-durable, pi-agent-core, pi-coding-agent, pi-tui). `pi-web-ui` (reusable chat web components on mini-lit and Tailwind v4, with artifacts and a JS REPL, per its npm listing) is not in the current README table or packages list; third-party search results show an npm listing at version 0.75.3 (third-party).
- Status: unclear for web-ui; GA for the others as shipped
- Evidence: https://github.com/earendil-works/pi; https://github.com/earendil-works/pi/tree/main/packages; https://github.com/earendil-works/pi/blob/main/packages/ai/README.md; https://github.com/earendil-works/pi/blob/main/packages/agent/README.md; https://github.com/earendil-works/pi/blob/main/packages/tui/README.md; https://socket.dev/npm/package/@earendil-works/pi-web-ui (third-party, via search result only; page not opened)
- Notes: The web-ui package appears to have been dropped from the monorepo; unverified (see section 7). READMEs for `mcp`, `codemode`, `schema` not opened.

### pi-ai (unified LLM API)
- What: Provider-collection `Models` object; streaming typed events; JSON-Schema tools with partial argument streaming; unified reasoning interface; cross-provider handoff and serialisable context; abort and continue; token and cost tracking (`totalFromProvider` flags billed vs estimated); OAuth flows; faux provider for tests; image-generation and classifier APIs; browser support. Chat catalog contains only models that support tool calling.
- Status: GA
- Evidence: https://github.com/earendil-works/pi/blob/main/packages/ai/README.md
- Notes: Page truncated by the fetch tool; browser, validation and OAuth detail not read.

### pi-agent-core
- What: Stateful `Agent` class over pi-ai with `prompt`, `continue`, `abort`, `waitForIdle`, `subscribe`; events (agent/turn/message/tool); tools with parallel or sequential execution; `beforeToolCall`/`afterToolCall`, `transformContext`, `convertToLlm`, `finishTurn`; steering and follow-up queues; `streamProxy` for browser apps behind a backend; app-defined message types via declaration merging.
- Status: GA
- Evidence: https://github.com/earendil-works/pi/blob/main/packages/agent/README.md
- Notes: README notes `shouldStopAfterTurn` was removed in favour of `finishTurn`.

### pi-tui
- What: TUI library with differential rendering (main-screen mode keeps scrollback; alt-screen mode for fixed viewport), components (Editor, Markdown, Input, SelectList, Image, ScrollView etc.), Kitty keyboard protocol, Kitty/iTerm2 inline images, IME cursor handling, overlays, synchronized output (CSI 2026), bracketed paste, mouse in alt-screen.
- Status: GA
- Evidence: https://github.com/earendil-works/pi/blob/main/packages/tui/README.md
- Notes: Default `tuiMode` is `fullscreen` since 1.0.0 (https://pi.dev/changelog, https://pi.dev/docs/latest/settings).

### chord (object runtime)
- What: TypeScript framework for apps made of objects running across processes (server, workers, browser tabs) with JSON-Schema contracts, live references, principals, documents persisted in a store, links (in-memory, IPC, WebSocket), HTTP/worker/web/terminal surfaces, and hot-reloadable plugins rolled out per host.
- Status: unclear (sync module marked experimental; no overall stage stated)
- Evidence: https://github.com/earendil-works/pi/blob/main/packages/chord/README.md
- Notes: Underlies pi-durable.

### pi-env (remote execution environment)
- What: Remote execution for pi-durable: a small Rust daemon on the remote machine speaks a framed stdio protocol; `RemoteExecutionEnv` reaches it over SSH (host-key scan, daemon deploy checked by SHA-256, restricted SSH options, no forwarding), providing file ops, `listFiles`, PTY `openTerminal`, and `openProcess` for protocol servers such as MCP. Durable worker, storage and credentials stay local.
- Status: unclear (no stage stated; tied to experimental pi-durable)
- Evidence: https://github.com/earendil-works/pi/blob/main/packages/env/README.md
- Notes: The core coding agent has an `ssh.ts` example extension (https://pi.dev/docs/latest/extensions); how pi-env connects to the CLI is not stated. (verified in review: Rust daemon, framed stdio protocol, SHA-256-named daemon verified before start, BatchMode strict host-key checking, no forwarding, openTerminal/openProcess, credentials local, no stage label; "host-key scan" wording not found)

### pi-chat
- What: Extension bridging Discord server channels and Telegram DMs/groups to a Pi session; each connection runs in its own Gondolin micro-VM (Alpine) with read/write/edit/bash tools and persistent workspace; memory files, agent-created skills, encrypted runtime-secret exchange, chat commands stop/status/compact/new session, tmux-based multi-channel worker orchestration.
- Status: unclear (no releases published; 16 commits at read time)
- Evidence: https://github.com/earendil-works/pi-chat
- Notes: Page lists both Apache-2.0 and MIT licences (conflict).

### 14. Models, providers and inference configuration

### Providers
- What: `/login` for subscription or API key, env vars for CI. Documented providers: Anthropic, Ant Ling, OpenAI, DeepSeek, NVIDIA NIM, Google Gemini, GitHub Copilot, Mistral, Groq, Cerebras, xAI, OpenRouter, Vercel AI Gateway, ZAI Coding Plan (global/China), OpenCode Zen and Go, Radius, TypeSafe, Hugging Face, Fireworks, Together AI, Baseten, Kimi For Coding, Meta, MiniMax (+China), Moonshot, Qwen Token Plan (+China), Xiaomi MiMo (several regions), Azure OpenAI, Amazon Bedrock, Cloudflare AI Gateway and Workers AI, Google Vertex AI. Credential lookup order: `--api-key`, `auth.json`, `models.json` apiKey, env/ambient cloud credentials.
- Status: GA
- Evidence: https://pi.dev/docs/latest/providers; https://pi.dev/docs/latest/models
- Notes: The page does not say which providers support subscription OAuth, except pi-ai README says Codex and GitHub Copilot require OAuth. Homepage says "15+ providers", the docs list well over thirty entries.

### Thinking levels and sampling
- What: Levels `off`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`, limited to what the model supports; default `medium`; Shift+Tab cycles; per-model overrides; `samplingParamsByThinkingLevel` (OpenAI-compatible APIs only, since 1.0.2).
- Status: GA
- Evidence: https://pi.dev/docs/latest/models; https://pi.dev/docs/latest/settings; https://pi.dev/changelog
- Notes: Thinking and model changes are recorded as session entries and restored on resume.

### Model selection and scoping
- What: `/model` (searches only authenticated providers), Ctrl+P cycles, `/scoped-models` and `--models` set the cycle list, `enabledModels` limits startup selection; `--model` accepts fuzzy patterns with `provider/id:thinking`; `--list-models`.
- Status: GA
- Evidence: https://pi.dev/docs/latest/models; https://pi.dev/docs/latest/cli
- Notes: Mid-session model switching is claimed on the homepage.

### Custom models.json and local models
- What: `models.json` defines providers with `baseUrl`, `api` type, `apiKey`/headers (literal, env interpolation, or `!command` run per request) and model lists, plus `modelOverrides`. A built-in llama.cpp router (`/llama`) loads GGUF files on demand; Ollama, LM Studio, vLLM, SGLang connect through `models.json`.
- Status: GA
- Evidence: https://pi.dev/docs/latest/models
- Notes: Compat flags should be set only after verifying endpoint behaviour.

### Classifier and image models
- What: Non-chat model types (classifiers answering typed questions about JSON state; image generators) hidden from `/model`, reachable through `codemode` or `ctx.modelRegistry.classify()/generateImages()`. Generated images are not saved to disk by the registry call.
- Status: GA
- Evidence: https://pi.dev/docs/latest/models; https://pi.dev/docs/latest/codemode
- Notes: Unusual; see section 3.

### 15. UI surfaces

### Interactive TUI
- What: Fullscreen by default (`regular` mode uses native scrollback); editor with multi-line, external editor (Ctrl+G), `@` mentions, image paste; tool output toggle (Ctrl+O), thinking toggle (Ctrl+T); footer shows folder, session, model, context usage and cost; startup header lists loaded resources; `/hotkeys`; `/debug` writes `pi-debug.log` (may contain sensitive data). Keybindings customised in `keybindings.json`.
- Status: GA
- Evidence: https://pi.dev/docs/latest/usage; https://pi.dev/docs/latest/keybindings; https://pi.dev/docs/latest/settings
- Notes: Mermaid rendering setting `markdown.mermaid` default `streaming`.

### Web, IDE, desktop, mobile, voice
- What: No first-party web app, IDE extension, desktop app or voice mode documented in pages read. A `pi-web-ui` components library is (or was) published; third-party browser front ends exist in the gallery. Android supported through Termux.
- Status: unclear
- Evidence: https://pi.dev/docs/latest; https://pi.dev/docs/latest/termux; https://pi.dev/packages
- Notes: Gallery results were found via search (third-party): pi.dev/packages/pi-web-ui and others; not opened.

### 16. Browser automation and computer use
- What: nothing documented in core. Gallery shows `pi-web-access` (web search and URL fetch) as a third-party package.
- Status: unclear
- Evidence: https://pi.dev/packages; https://pi.dev/docs/latest
- Notes: Checked docs index and quickstart; no browser or computer-use feature.

### 17. Remote, cloud, headless, CI and distributed execution

### Headless modes
- What: Print, JSON, RPC and SDK are the automation interfaces; `PI_OFFLINE=1`/`--offline` disables automatic network activity; project trust behaviour in headless modes is documented.
- Status: GA
- Evidence: https://pi.dev/docs/latest; https://pi.dev/docs/latest/cli; https://pi.dev/docs/latest/security
- Notes: In headless modes `ask` trust skips project resources.

### Remote tool execution
- What: Built-in tools take pluggable operations (example `ssh.ts`); sandbox example `gondolin/`; `pi-env` provides SSH-based remote execution for pi-durable. No master/worker topology for the CLI documented.
- Status: GA (extension hooks); experimental for pi-durable/pi-env
- Evidence: https://pi.dev/docs/latest/extensions; https://github.com/earendil-works/pi/blob/main/packages/env/README.md
- Notes: Example names only for `ssh.ts`.

### Chat-platform remote control
- What: pi-chat lets Discord/Telegram users drive a sandboxed Pi session.
- Status: unclear
- Evidence: https://github.com/earendil-works/pi-chat
- Notes: See pi-chat entry.

### 18. Collaboration, teams and enterprise administration
- What: nothing documented (no accounts, teams, admin controls). Sharing is session export/gist/Radius artifact. Future "Fair Source" and "proprietary enterprise" tiers are announced but not built.
- Status: unclear
- Evidence: https://pi.dev/docs/latest; https://mariozechner.at/posts/2026-04-08-ive-sold-out/
- Notes: Roadmap statement from the author's post only.

### 19. Observability

### Usage and cost
- What: Footer shows context usage and cost; `/session` shows file, ID, message count, token usage and cost; session files include `usage` entries; pi-ai reports billed vs estimated cost.
- Status: GA
- Evidence: https://pi.dev/docs/latest/usage; https://pi.dev/docs/latest/sessions; https://pi.dev/docs/latest/session-format; https://github.com/earendil-works/pi/blob/main/packages/ai/README.md
- Notes: -

### Telemetry contracts (pi-telemetry)
- What: Vendor-neutral spans/metrics contract with no-op default, in-memory and SQLite adapters, conformance suite, trace renderer; no exporter included. Extension hook `session_compact_failed` is described as useful for telemetry.
- Status: unclear (no stage stated)
- Evidence: https://github.com/earendil-works/pi/blob/main/packages/telemetry/README.md; https://pi.dev/docs/latest/compaction
- Notes: Page does not show Pi sending telemetry anywhere.

### Debug log and provider payload hooks
- What: `/debug` writes `pi-debug.log`; extensions get `before_provider_request`/`after_provider_response` hooks (example `provider-payload.ts`).
- Status: GA
- Evidence: https://pi.dev/docs/latest/usage; https://pi.dev/docs/latest/extensions
- Notes: Debug log may contain sensitive data per docs.

### Behaviour evals
- What: `packages/evals` runs behavioural evals with `vitest-evals`, including paired with-docs vs without-docs container runs to measure whether documentation helps the agent.
- Status: GA (internal tooling in repo)
- Evidence: https://github.com/earendil-works/pi/blob/main/packages/evals/README.md
- Notes: Interesting for measuring doc quality.

### 20. Configuration and customisation

### Config files and precedence
- What: User config in agent dir `~/.pi/agent` (override `PI_CODING_AGENT_DIR`): settings, keybindings, mcp.json, models.json, auth.json, trust.json, extensions/skills/prompts/themes, SYSTEM/APPEND_SYSTEM. Project config in `.pi/` (trust required). Project settings override user settings and resource lists are combined; system-prompt files do not merge. `/reload` re-reads. Published JSON schemas (`$schema`) for settings and themes. Resource filters `!pattern`, `+path`, `-path`.
- Status: GA
- Evidence: https://pi.dev/docs/latest/configuration; https://pi.dev/docs/latest/settings; https://pi.dev/changelog
- Notes: Some settings are agent-dir only (`defaultProjectTrust`, `httpProxy`, `selfUpdateCommand`, `cacheWarming`).

### Environment variables
- What: `PI_TELEMETRY`, `PI_OFFLINE`, `PI_SKIP_VERSION_CHECK`, `PI_SELF_UPDATE_COMMAND`, `PI_CODING_AGENT_DIR`, `PI_CODING_AGENT_SESSION_DIR`, `PI_PACKAGE_DIR`, `HTTP_PROXY`/`HTTPS_PROXY`, plus provider keys.
- Status: GA
- Evidence: https://pi.dev/docs/latest/environment-variables
- Notes: -

### 21. Security and privacy

### Install telemetry and update checks
- What: `enableInstallTelemetry` defaults to true (install/update reporting, not update checks); `enableAnalytics` defaults false; `PI_TELEMETRY` overrides install telemetry and provider attribution headers; `PI_SKIP_VERSION_CHECK` disables the latest-version request to pi.dev; `warnings.anthropicExtraUsage` default true.
- Status: GA
- Evidence: https://pi.dev/docs/latest/settings (Updates and telemetry); https://pi.dev/docs/latest/environment-variables
- Notes: What exactly is reported by install telemetry is not described in pages read. Default-on is worth noting. (verified in review: `enableInstallTelemetry` true, `enableAnalytics` false)

### Supply-chain hardening
- What: README states exact version pinning for direct dependencies, a lockfile guard, lifecycle-script restrictions and `--ignore-scripts` installs; the installer script pins all dependencies while npm install does not pin transitive ones.
- Status: GA
- Evidence: https://github.com/earendil-works/pi; https://pi.dev/docs/latest/quickstart
- Notes: Changelog 1.0.1 removed `npm-shrinkwrap.json` from the published package.

### Credential handling
- What: Credentials stored in `auth.json` in the agent dir; `pi auth login/logout/check/print-api-key/print-bearer-token` manage them; `models.json` values can use `!command`; MCP OAuth tokens stored locally and refreshed. Docs warn session exports and debug logs can hold sensitive data.
- Status: GA
- Evidence: https://pi.dev/docs/latest/cli; https://pi.dev/docs/latest/models; https://pi.dev/docs/latest/mcp
- Notes: Encryption at rest of `auth.json` not stated in pages read.

### 22. Licence, pricing, distribution
- What: MIT; free to use (no pricing on homepage; model costs are the user's). Distribution via installer, npm, Nix, standalone binaries built from release source. Trademarked name. Future Fair Source/proprietary tiers announced.
- Status: GA
- Evidence: https://pi.dev/; https://github.com/earendil-works/pi; https://mariozechner.at/posts/2026-04-08-ive-sold-out/
- Notes: Radius (linked from the homepage) is an Earendil-adjacent gateway listed as early alpha in the providers page; its pricing was not opened.

### 23. Anything else

### Docs-aware evaluation, cache warming, durable runtime
- What: see evals, cache controls, pi-durable entries above.
- Status: mixed
- Evidence: see those entries
- Notes: -

### Self-update
- What: `pi update` updates Pi, packages (`--extensions`), model catalogs (`--models`), or `--self`; custom command via `selfUpdateCommand`/`PI_SELF_UPDATE_COMMAND`; Nix installs use `nix profile upgrade`.
- Status: GA
- Evidence: https://pi.dev/docs/latest/cli; https://pi.dev/docs/latest/quickstart
- Notes: -

## 3. Distinctive or unusual

- Documented non-features with reasons (no permission prompts, no plan mode, no sub-agents, no todos, no background bash) and a concrete substitute for each (container, PLAN.md, separate sessions or self-spawn, TODO.md, tmux): clearest design stance of any harness; the reasons come from the 2025 author post.
- Codemode: model writes JS in a QuickJS sandbox that calls tools; only the output returns; used as the default home for MCP tools so they cost no context.
- MCP exposure modes (`codemode`/`deferred`/`direct`/`hidden`) per server or tool: a systematic answer to MCP context bloat.
- Session JSONL tree with `/tree` in-file branching, branch summaries on navigation, and an append-only `context_edit` entry.
- Extension events at provider level (before request, after response, stream events, cache warming decision) and tool overrides by name.
- Project trust gate separate from tool permissions, honest about being incomplete.
- Documented isolation patterns where the credential proxy stays outside the sandbox (Docker Sandboxes, OpenShell).
- pi-durable plus pi-env: crash-surviving agent runtime with a Rust daemon on the remote host and local credentials.
- A package ecosystem of 5.5k+ npm packages filled by third parties for the omitted features.
- Docs-vs-no-docs behavioural evals for the project's own documentation.

## 4. Architecture and UX notes

- One in-process agent per Pi process; extensions run in the same process with the same OS permissions (https://pi.dev/docs/latest/how-pi-works, https://pi.dev/docs/latest/extensions).
- Layering: pi-ai (providers) -> pi-agent-core (loop) -> pi-coding-agent (tools, sessions, extensions, modes) -> pi-tui (rendering) (https://github.com/earendil-works/pi). All interfaces (TUI, print, JSON, RPC, SDK) share the same agent and session mechanisms.
- RPC is the process-isolation boundary for non-TypeScript hosts; the SDK is in-process only (https://pi.dev/docs/latest/sdk).
- Sessions are local JSONL files; sharing is by gist or Radius upload.
- Experimental pi-durable splits worker, storage and remote execution (via pi-env), a different model from the CLI (https://github.com/earendil-works/pi/blob/main/packages/durable/README.md).

## 5. Relevance to a self-hosted master/node harness

- The "no permission system, isolate instead" stance (section 2, area 5) is the opposite of a node-side policy approach; the docs' own caveat that trust and transcript review are not boundaries is a useful warning about relying on UI approvals.
- Docker Sandboxes' credential proxy and OpenShell inference routing keep provider keys off the worker: a pattern for nodes that hold code but should not hold keys.
- RPC mode (JSONL, `agent_settled`, orderly shutdown, out-of-order responses by id) is a ready reference for a master-to-node protocol; the `agent_end` versus `agent_settled` distinction avoids a common wrapper bug.
- Session tree format (id/parentId JSONL, compaction and branch-summary entries, `context_edit`, versioned) is close to an event-log design; the "one active leaf, branches never deleted" model maps to a fold over the log.
- Compaction defaults (reserve 16384, keep 20000, cut-point rules, previous summary chained, thinking dropped, tool results truncated to 2000 chars) are concrete numbers to compare against.
- MCP exposure modes plus codemode show a way to keep tool lists small; codemode's QuickJS limits (256 MB, no network) are a possible safe-scripting design.
- pi-durable's commit-before-display, restart-limit crash loops and pi-env's restricted SSH and hash-checked daemon deploy are directly about remote worker execution.
- Project trust order (CLI, extension event, saved decision, default `ask`) and headless behaviour (skip protected resources) is a model for per-project config gating on a node.
- Packages are npm/git with no vetting: an example of the supply-chain risk to avoid or gate on a self-hosted node.
- Session sharing by uploading to gist or Radius shows the privacy hazard of share features; the docs' warning is minimal.

## 6. Overhyped, stale or contradictory

- Codemode default: the CLI page says codemode and tool_search are off by default (enable with `--tools +codemode`), while the settings page says `codemode.mode` defaults to `on` and lists `builtin:codemode` and `builtin:tool-search` as loaded by default, and the MCP page says MCP tools default to `codemode` exposure (callable only from codemode scripts). The pages as read do not reconcile whether MCP tools are usable by default; this needs a run to settle. Sources: https://pi.dev/docs/latest/cli, https://pi.dev/docs/latest/settings, https://pi.dev/docs/latest/mcp, https://pi.dev/docs/latest/codemode.
- Homepage lists MCP among deliberate non-features while noting it is now built in; the page text is internally inconsistent (https://pi.dev/).
- npm scope: the April 2026 post says `@earendil/pi`; README and docs use `@earendil-works/pi-coding-agent` (https://mariozechner.at/posts/2026-04-08-ive-sold-out/ vs https://pi.dev/docs/latest/sdk).
- Homepage says "15+ providers"; providers page lists many more (https://pi.dev/ vs https://pi.dev/docs/latest/providers).
- The design rationale for non-features is from a 2025 post when Pi was a personal project; MCP was argued against there and later added, so the post is not a reliable statement of today's policy (https://mariozechner.at/posts/2025-11-30-pi-coding-agent/).
- The how-pi-works and quickstart pages do not mention the omitted features at all; the non-feature list lives on the homepage and README.
- pi-chat shows two different licences on one page (https://github.com/earendil-works/pi-chat).
- README package table lists `pi-telemetry`, `pi-durable`, `chord` but not `pi-env`, `codemode`, `mcp`, `schema`, `evals` that exist as directories.
- The 2025 post's "under 1,000 tokens" prompt claim is not repeated in current docs; with MCP, skills, codemode and tool search added it may no longer hold (unverified).
- Benchmark claims in the 2025 post (Terminal-Bench 2.0) are the author's own runs.

## 7. Unverified

- Per-tool behaviour (edit matching rules, read limits, grep/find implementation): not in pages opened; a tools reference page may exist under https://pi.dev/docs/latest/ but was not located. Settle by finding the page or reading packages/coding-agent docs.
- Whether `pi-web-ui` still ships from the monorepo (not in the README or packages list; npm listing seen only via a search snippet at 0.75.3). Settle by opening npm or the repo history.
- READMEs for `packages/mcp`, `packages/codemode`, `packages/schema`; Pi Durable blog post (linked from the changelog banner); Radius pricing and product details; `pi-share-hf`; RFCs at rfc.earendil.com/keyword/pi; CONTRIBUTING, AGENTS.md, SECURITY.md in the repo.
- Exact content of install telemetry and what data is sent.
- Whether the CLI integrates with pi-durable/pi-env or they are separate libraries.
- Content of example extensions (`plan-mode/`, `git-checkpoint.ts`, `ssh.ts`) beyond the subagent README; only names were read.
- The extensions page was read in two fetch segments; the pi-ai README was truncated.
- Whether sessions or credentials are encrypted at rest.

## 8. Sources

All opened with the fetch tool on 2026-11-10 unless marked.

- https://github.com/earendil-works/pi - repo README: identity, packages, non-features, install, supply chain
- https://pi.dev/ - homepage: positioning, non-features list, modes, licence
- https://pi.dev/docs/latest - docs index
- https://pi.dev/docs/latest/quickstart - install, first run, customization chooser
- https://pi.dev/docs/latest/how-pi-works - loop, sessions, interfaces
- https://pi.dev/docs/latest/usage - interactive features
- https://pi.dev/docs/latest/configuration - files, context files, SYSTEM.md
- https://pi.dev/docs/latest/settings - defaults
- https://pi.dev/docs/latest/models - models, thinking, models.json, classifier/image models
- https://pi.dev/docs/latest/providers - provider list
- https://pi.dev/docs/latest/sessions - session commands, share/export
- https://pi.dev/docs/latest/session-format - entry types, versions
- https://pi.dev/docs/latest/compaction - compaction and branch summaries
- https://pi.dev/docs/latest/extensions - extension API (two fetch segments)
- https://pi.dev/docs/latest/skills - skills
- https://pi.dev/docs/latest/prompt-templates - prompt templates
- https://pi.dev/docs/latest/themes - themes
- https://pi.dev/docs/latest/packages - packages and distribution
- https://pi.dev/docs/latest/mcp - MCP
- https://pi.dev/docs/latest/codemode - codemode
- https://pi.dev/docs/latest/security - security and trust
- https://pi.dev/docs/latest/containerization - isolation patterns
- https://pi.dev/docs/latest/cli - CLI
- https://pi.dev/docs/latest/json - JSON mode
- https://pi.dev/docs/latest/rpc - RPC mode
- https://pi.dev/docs/latest/sdk - SDK
- https://pi.dev/docs/latest/environment-variables - env vars
- https://pi.dev/docs/latest/keybindings - keybindings
- https://pi.dev/docs/latest/windows, https://pi.dev/docs/latest/tmux, https://pi.dev/docs/latest/termux, https://pi.dev/docs/latest/terminal-setup - platform guides
- https://pi.dev/changelog - releases 1.0.0 to 1.5.2
- https://pi.dev/packages - package gallery
- https://mariozechner.at/posts/2025-11-30-pi-coding-agent/ - original design rationale (author blog)
- https://mariozechner.at/posts/2026-04-08-ive-sold-out/ - ownership move and licensing commitments
- https://github.com/earendil-works/pi/tree/main/packages - package directory list
- https://github.com/earendil-works/pi/blob/main/packages/ai/README.md, .../agent/README.md, .../tui/README.md, .../durable/README.md, .../chord/README.md, .../telemetry/README.md, .../env/README.md, .../evals/README.md - package READMEs
- https://github.com/earendil-works/pi/tree/main/packages/coding-agent/examples/extensions/subagent - example subagent extension README
- https://github.com/earendil-works/pi-chat - chat bridge extension
- Web search results (third-party, snippets only, pages not opened): https://socket.dev/npm/package/@earendil-works/pi-web-ui and pi.dev/packages/pi-web-ui listings

## 9. Review

Reviewed 2026-11-10 by an independent reviewer (public pages only, fetched through a summarising fetch tool with specific-fact prompts). Checked 30 entries or claim-groups out of about 85 entries. Entries I verified carry `(verified in review)` or a specific list of verified details in their Notes; entries not listed below were not checked.

| Entry | Verdict | Reason |
|---|---|---|
| Identity: repo move, ownership, trademark, MIT, Fair Source tiers not built | confirmed | 2026-04-08 post; "Inc." suffix not found, softened |
| Identity: npm scope contradiction | confirmed | post says `@earendil/pi`, README uses `@earendil-works/...` |
| Identity: versions 1.0.0 (2026-10-01), 1.5.2 (2026-11-10) | confirmed | changelog |
| README facts (121.2k stars, 15.6k forks, 7,767 commits, package table of seven, new-contributor auto-close, MIT) | confirmed | repo page |
| No permission system | confirmed | README, security page, 2025 post |
| Project trust | confirmed | security page |
| Vulnerability scope | confirmed | security page |
| No plan mode / no todos | confirmed-with-correction | To-do reason in 2025 post is "confuse models", plan reason includes visibility |
| No sub-agents, no background bash | confirmed | 2025 post reasons and substitutes match |
| No MCP then MCP added (0.99.0, 2026-09-29) | confirmed | changelog; homepage heading "No MCP Now with MCP+Codemode" |
| Homepage "15+ providers" | confirmed | heading text |
| Codemode | confirmed-with-correction | "failed scripts do not roll back tool calls" and output-cap detail not found |
| Codemode default contradiction | confirmed | CLI page says off, settings page says on |
| MCP built-in details | confirmed | mcp page |
| Tool selection flags, default tools | confirmed | cli and settings pages |
| Agent retry defaults | confirmed | settings page |
| Compaction defaults | confirmed | settings page |
| Cache warming default and 1.5.1 | confirmed | settings, changelog |
| pi-env | confirmed | README; no stage label, "host-key scan" not found |
| pi-durable | confirmed-with-correction | background-owned work survives parent abort |
| Subagents in pi-durable | confirmed-with-correction | same as above |
| Install telemetry and analytics defaults | confirmed | settings page |
| Session share, export, tree, fork, clone | confirmed | sessions page |
| Changelog items (1.0.1, 1.0.2, 1.0.4, 1.2.0, 1.5.2) | confirmed | changelog |

Borrowed features: none found; the "documented non-feature plus substitute" framing matches Pi's own homepage and 2025 post. One caution: the homepage gives substitutes but few reasons, so the reasons in the survey come from the 2025 post only, as the survey says.

Notable omissions: the docs index fetched lists no pages for extensions, MCP, codemode, skills, packages or sessions in its navigation text, though all those pages exist and are cited, so the navigation could not be audited. Items the survey could not open and worth a follow-up: Radius (the Earendil-adjacent gateway), RFCs at rfc.earendil.com, the MCP "Why we changed our minds" post linked from the homepage, `pi-share-hf`, SECURITY.md, packages `mcp`, `codemode`, `schema` READMEs.

Trust judgement: the checked entries were accurate on defaults, versions and mechanisms, with small reason and nuance errors. The survey is also candid about its contradictions (codemode default, npm scope), and the codemode contradiction reproduced. The unchecked remainder (provider lists, TUI, themes, pi-ai, pi-tui, chord, pi-chat) is probably reliable but has less scrutiny; treat as mostly reliable.
