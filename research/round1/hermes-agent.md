# Hermes Agent (Nous Research): Round 1 public-surface survey

Surveyed 2026-11-10. Public surface only (docs site, README, GitHub releases page). No source code read. Docs base: `https://hermes-agent.nousresearch.com/docs/`. Where an entry cites a docs path only, the full URL is that base plus the path (every URL below was opened; they are listed in full in section 8).

## 1. Identity

- Name: Hermes Agent. Vendor: Nous Research. Repository https://github.com/NousResearch/hermes-agent (Python, MIT, about 262k stars, 58.6k forks, 66k commits on `main`, per the repository page).
- Surfaces: terminal CLI and an Ink TUI (`hermes --tui`), native Desktop app (macOS Apple Silicon, Windows, Linux), web dashboard, a gateway-served "Webapp", messaging gateway on about 28 platforms, OpenAI-compatible API server, ACP server for editors, Python embedding, Docker image, Nix, Termux.
- Version surveyed: v0.23.1 (GitHub releases page: Nov 9, 2026, listed Nov 10). Prior: v0.23.0 "The Hearth Release" (Nov 5), v0.22.7, v0.22.6, v0.22.5. Matches `00-targets.md`.
- Corrections to `00-targets.md`: it says "21+ platforms"; the messaging overview lists about 28 (more than that, plus Relay). Its three "not opened" pages (tools, checkpoints, egress) are now opened and used here. It does not mention that the docs call itself a general agent with a coding brief inside the prompt ("coding operating brief", prompt-assembly page).
- Pricing/licence: MIT code. Optional paid Nous Portal subscription (models plus a "Tool Gateway"); everything also works with own keys and local models.

## 2. Features by area

### 1. Agent loop and execution model

### Single synchronous AIAgent core with many entry points
- What: One core class runs the loop; the CLI, gateway, ACP server, batch runner, API server and Python library are thin entry points around it. Supports three wire modes (chat completions, Codex-style responses, Anthropic Messages). The loop builds or reuses the system prompt, runs preflight compression, makes an interruptible API call, executes tool calls and repeats; a plain text reply persists the session and returns.
- Status: GA
- Evidence: /developer-guide/architecture (Architecture); /developer-guide/agent-loop (Agent Loop)
- Notes: Version not stated on page. (corrected in review: the agent-loop page does not use the word "synchronous"; it only describes the per-iteration sequence; the synchronous claim may come from the architecture page, not rechecked)

### Iteration budget and interruption
- What: Each agent has an iteration budget (default 500, `agent.max_turns`); subagents get their own, so totals can exceed the parent cap. The API call runs in a background thread while the main thread watches for an interrupt (new message, `/stop`, signal); the abandoned call adds no partial response.
- Status: GA
- Evidence: /developer-guide/agent-loop
- Notes: Page says subagent cap default is 50; the delegation page says `delegation.max_iterations` default 250. Sources disagree. (verified in review: 500 and `agent.max_turns` on the agent-loop page; the 50 vs 250 contradiction is real)

### Parallel and sequential tool execution
- What: One call runs on the main thread; several calls in a turn run in a thread pool with results re-ordered to match call order. Interactive tools (for example clarify) are always sequential. Per-call flow: handler, pre-tool hook, dangerous-command check, execute, post-tool hook. `todo`, `memory`, `session_search` and `delegate_task` are intercepted before the registry.
- Status: GA
- Evidence: /developer-guide/agent-loop
- Notes: MCP tools are sequential unless a server opts in (see MCP).

### Busy-input modes (interrupt, queue, steer)
- What: A message sent while a turn runs can interrupt and redirect (default; foreground terminal commands move to the background rather than being killed), queue as a later turn, or steer (injected after the next tool call). `/stop` is a hard stop. `/queue` has list, remove, edit, move and clear; the queue lives on the gateway and survives restarts.
- Status: GA
- Evidence: /user-guide/messaging; /user-guide/cli
- Notes: Switch with `/busy queue|steer|interrupt`.

### Persistent goals (`/goal`)
- What: A standing objective in one session. After each turn an auxiliary judge model (sees the goal and the last ~4 KB of the response) returns done, continue, blocked or wait; on continue Hermes feeds a continuation prompt into the same session. Default budget 20 continuation turns, then auto-pause. Optional completion contract and shell-command quality gates that must exit 0 before the judge runs. Judge errors fail open to continue.
- Status: GA
- Evidence: /user-guide/features/goals (Persistent Goals)
- Notes: Docs say it is inspired by Codex CLI's `/goal`. Continuations are plain user messages so the prompt cache is not disturbed. (verified in review: 20 turns, ~4 KB, fail-open; gates default 3 retries, 5 min timeout; judge also sees background processes)

### Recurring loops and session heartbeats
- What: `/loop` re-runs a prompt in the current session on a fixed or self-paced interval (1 min floor backing off to a 15 min ceiling while replies are unchanged), with stop conditions (`LOOP_COMPLETE`, `--times`, `--until` judged by the goal judge, `loops.max_ticks` default 100). `/heartbeat every <interval> <prompt>` attaches one recurring instruction to a session (60 s minimum), idle-only, coalesced, restored after gateway restart. Both only fire while the owning backend process is alive.
- Status: GA
- Evidence: /user-guide/features/loops; /user-guide/features/heartbeat
- Notes: One loop per session.

### Mixture of Agents presets
- What: Named presets appear as models under a virtual `moa` provider. Reference models run first without tool schemas and their text is given to an aggregator model as private guidance; the aggregator writes the response and emits tool calls. `fanout` is `user_turn` (default since July 2026), `per_iteration` or `every_n`; optional privacy filter.
- Status: GA (no maturity label on page)
- Evidence: /user-guide/features/mixture-of-agents
- Notes: Benchmark numbers on the page are the vendor's own (`vendor-claim`).

### 2. Built-in tools

### Tool registry and toolsets
- What: A self-registering central registry of 70+ tools across about 28 toolsets. Toolsets are chosen per run (`--toolsets`) or per platform via `hermes tools`; platform presets (`hermes-cli`, `hermes-telegram`) and dynamic `mcp-<server>` toolsets exist. Delegated children inherit the parent's toolsets.
- Status: GA
- Evidence: /developer-guide/architecture; /user-guide/features/tools; /reference/tools-reference
- Notes: Count from the architecture page (not rechecked).

### Tool inventory (reference page)
- What: terminal and `process_manage`; file tools `read_file`, `write_file`, `patch` (fuzzy matching, returns diff), `search_files` (ripgrep); web `web_search`, `web_extract`; browser `browser_exec`, `browser_cdp`, `browser_dialog`, `browser_vault`; `computer_use`; `execute_code`; `delegate_task`; `memory`, `session_search`; `skills_list`, `skill_view`, `skill_manage`; `cronjob_manage`; `todo_list`; `clarify` (1 to 5 questions, single, multi or open); `vision_analyze`, `image_generate`, `text_to_speech`; opt-in `video_analyze`, `video_generate`, `x_search`; `manage_connections`; a fourteen-tool `kanban_*` set; platform tools for Discord and Feishu; desktop-only UI tools (`desktop_preview`, `drive_preview`, `annotate_preview`, `gui_tour`, `set_theme` and others).
- Status: GA
- Evidence: /reference/tools-reference (Tools Reference)
- Notes: Honcho, Home Assistant, Spotify and Yuanbao tools moved out to plugins. The tools page lists `read_file`, `patch` under terminal and files but omits `write_file`/`search_files`; the reference page is authoritative.

### Tool Search (deferred MCP and plugin tools)
- What: MCP and non-core plugin tools are hidden behind three bridge tools: `tool_search` (BM25 with stemming), `tool_describe`, `tool_call`. Hooks, guardrails and approvals still apply to the real tool name. Three tiers: none deferred; names-only manifest; per-server summary. Default budget 5% of context; core tools stay loaded.
- Status: GA, described as opt-in but `auto` currently behaves as on
- Evidence: /user-guide/features/tool-search
- Notes: Docs call it "opt-in" yet say the default `auto` activates whenever a deferred tool exists; `auto` is reserved for later schema inlining.

### Code execution with tool RPC (`execute_code`)
- What: The model writes a Python script importing a generated `hermes_tools` stub; calls travel over a Unix socket to the parent, so many tool calls cost one turn and only `print()` output returns to the context. Allowed in scripts: web search/extract, file read/write/search/patch, terminal (foreground only). No recursion, no delegation, no MCP. Limits: 300 s, 50 tool calls, 50 KB stdout inline (full output cached on disk). Persistent Python kernel per session on the local backend (idle timeout 1800 s, max 4 kernels). Modes `project` (default) and `strict`.
- Status: GA
- Evidence: /user-guide/features/code-execution
- Notes: Child process env strips names containing KEY, TOKEN, SECRET, PASSWORD, CREDENTIAL, PASSWD, AUTH. (verified in review: 300 s, 50 calls, 50 KB, socket, kernel 1800 s / 4; Windows falls back to loopback TCP)

### 3. Code editing and file operations

### File tools and guards
- What: `patch` does targeted find-and-replace with fuzzy matching and returns a diff; `read_file` paginates with line numbers, transcodes UTF-16 and refuses files over 10 MB and binaries; exit codes such as -9/137 are explained. `write_file` and `patch` have no approval prompt but hard-block writes to `~/.ssh` (except config, approval-gated), `~/.aws`, `~/.kube`, sudoers, `~/.netrc`, Hermes secret stores; those are also read-denied. Optional `HERMES_WRITE_SAFE_ROOT` restricts writes to listed prefixes (auto-set in the official Docker image).
- Status: GA
- Evidence: /user-guide/security; /user-guide/features/tools; /reference/tools-reference
- Notes: Docs state the terminal tool can bypass these guards; they are "defense in depth, not a sandbox".

### LSP diagnostics after writes
- What: After each `write_file`/`patch`, an in-process syntax check runs, then LSP diagnostics if clean. Only new errors versus pre-edit content are reported in a separate `lsp_diagnostics` field; verdicts within about 80 ms appear inline, slower ones ride on the next tool result. About 30 language servers listed; some auto-install. Dormant outside a git repo. Workspaces are untrusted by default: only some servers run until the directory is in `lsp.trusted_workspaces`.
- Status: GA (`enabled: true` default)
- Evidence: /user-guide/features/lsp
- Notes: None.

### Context references (`@` syntax)
- What: `@` attaches files, folders, git diffs and URLs to a message.
- Status: GA
- Evidence: /user-guide/features/overview (one-line listing only)
- Notes: Dedicated page `/user-guide/features/context-references` exists but was not opened; detail unverified.

### Document extraction
- What: Converts PDFs, Office files and notebooks to text.
- Status: unclear
- Evidence: /docs/llms.txt (title and one-line description only)
- Notes: Dedicated page not opened.

### 4. Terminal and shell integration

### Terminal tool, background processes and PTY
- What: `terminal` runs non-interactive commands (no TTY unless `pty=true`); `background=true` returns a session id and pid, managed with `process_manage` (list, poll, wait, log, kill, write). A trailing `&` is auto-backgrounded; `nohup`, `disown` and command substitution forms are refused. After two unchanged polls Hermes enables notify-on-complete. Newest 64 completed results kept up to 7 days with a 200,000-char redacted tail. Sudo: masked prompt cached for the session in interactive sessions or `SUDO_PASSWORD` in `.env`.
- Status: GA
- Evidence: /user-guide/features/tools (Background Processes, Sudo)
- Notes: Docs recommend an early non-interactive guard in `.bashrc`/`.zshrc`.

### Terminal backends and how the agent is separated from execution
- What: `terminal.backend` selects where commands run: `local` (default), `docker`, `ssh`, `modal`, `daytona`, `vercel_sandbox`, plus plugin backends (`singularity`, `sprites`/Fly.io). The agent process stays on the host; terminal, `read_file`, `write_file`, `patch`, `search_files` and `execute_code` all go through the environment's `execute()` so files and commands see the same sandbox. Only the local backend uses the host filesystem directly. Docker runs one long-lived container (`sleep infinity`, `docker exec`), removed on shutdown, `/workspace` and `/root` persist if `container_persistent`. SSH is recommended by the docs because the agent "can't modify its own code". Default container resources 1 CPU, 5120 MB, 51200 MB disk.
- Status: GA
- Evidence: /user-guide/features/tools (Terminal Backends, Separating the Agent...); /user-guide/docker (Docker as the Terminal Backend); /developer-guide/terminal-environment-plugin
- Notes: Architecture page says seven backends in a diagram but six in prose (page inconsistent). README says six built-in, Singularity as plugin; tools page lists Singularity and Sprites as plugins. (verified in review: six built-in, SSH wording, resources 1 CPU/5120 MB/51200 MB, `container_persistent` default true; the architecture inconsistency was not rechecked)

### Terminal environment plugin interface
- What: A backend is a provider class (`is_available`, `create_environment`, flags `is_remote`, `is_container`, optional `probe`, `strip_env_keys`) plus an environment object (`execute(command, timeout)` returning output and exit code, `cleanup`). Plugins cannot shadow built-in names. Background processes are polled through the creating environment and marked `lost` after cleanup. Refused under `plugins.isolation: host`.
- Status: GA
- Evidence: /developer-guide/terminal-environment-plugin
- Notes: None.

### Skills and credential files into sandboxes
- What: Skills are bind-mounted read-only into Docker; SSH and Modal sync them by rsync or Modal mount. Skills can declare required credential files, mounted read-only (Docker) or synced before each command (Modal).
- Status: GA
- Evidence: /user-guide/docker; /user-guide/security
- Notes: None.

### Shell passthrough `!<command>`
- What: `!cmd` in the CLI runs a shell command without a model turn.
- Status: GA
- Evidence: /user-guide/cli
- Notes: None.

### 5. Permissions, approvals and sandboxing

### Command approval modes (smart, manual, off)
- What: `approvals.mode` is `smart` (default: an auxiliary LLM auto-approves low-risk, auto-denies dangerous, escalates uncertain), `manual`, or `off` (equals `--yolo`). Prompt timeout 300 s and expiry denies (fail-closed). CLI choices once, session, always, deny; gateway accepts yes/no replies or buttons. Headless defaults deny: `cron_mode`, `single_query_mode`, `unattended_mode` (webhook, msgraph, API server). `approvers` controls who may answer in shared chats. Many dangerous-pattern triggers are listed (recursive delete, `chmod 777`, `mkfs`, `dd`, SQL DROP, `curl | sh`, writes to `/etc`, etc.).
- Status: GA
- Evidence: /user-guide/security (Dangerous Command Approval); /guides/secure-hermes-on-a-work-machine
- Notes: Docs say deny rules and write guards protect against honest mistakes, not a hostile agent. (verified in review: smart default, off disables all checks, 300 s fail-closed, headless modes default deny)

### Hardline blocklist and user deny rules
- What: A floor refused even under YOLO or mode off: `rm -rf /` variants, fork bomb, `mkfs` on mounted root, `dd` to physical disks, piping untrusted URLs into `sh` at top level; fails closed on unparseable quoting. `approvals.deny` fnmatch globs apply before YOLO and to all backends, see through `sudo`, `env`, `nohup` and `sh -c` payloads. Supervised-gateway guard stops the agent from stopping or restarting its own gateway.
- Status: GA
- Evidence: /user-guide/security
- Notes: Docs say the matcher is not a full shell interpreter. (verified in review: hardline refused even under YOLO/off, no override flag; approvals.deny applies before YOLO and off)

### Permanent allowlist and mined suggestions
- What: `command_allowlist` in config; `hermes approvals suggest` mines approval history into proposals, never applies them and never proposes destructive classes.
- Status: GA
- Evidence: /user-guide/security
- Notes: None.

### Container isolation and container bypass of approvals
- What: Docker backend uses cap-drop ALL (adds back DAC_OVERRIDE, CHOWN, FOWNER), no-new-privileges, pids limit 256, tmpfs `/tmp` (512 MB nosuid) and `/var/tmp` (noexec). Dangerous-command checks are skipped on docker, modal, daytona, vercel_sandbox and plugin backends, since the container is treated as the boundary. `docker_forward_env` defaults empty.
- Status: GA
- Evidence: /user-guide/security (Container Isolation); /user-guide/features/tools
- Notes: Checkpoints are also disabled on container backends (see checkpoints). (verified in review on the security page: dangerous-command checks are skipped on docker, modal, daytona, vercel_sandbox and container plugins, cap-drop ALL with three added back, pids-limit 256; the tools page does NOT state the skip; the tmpfs 512 MB/nosuid detail was not confirmed. Checkpoint disablement confirmed on the checkpoints page, which adds that local and SSH are unaffected)

### Egress proxy with credential injection (iron-proxy)
- What: Optional host-side TLS-intercepting proxy (ironsh/iron-proxy, SHA-256-verified pinned binary) for remote/Docker sandboxes. The sandbox holds opaque proxy tokens in the normal provider env vars; the proxy swaps them for real keys (from `~/.hermes/.env` or Bitwarden Secrets Manager) on allowlisted upstreams. Default-deny upstream allowlist (OpenRouter, OpenAI, Anthropic, Google, xAI, Mistral, Groq, Together, DeepSeek, Nous), SSRF CIDR denies, binds the Docker bridge gateway (never 0.0.0.0). Managed with `hermes egress install|setup|start|reload|restart|status`.
- Status: GA (called "optional"; no maturity label)
- Evidence: /user-guide/egress; /user-guide/egress/iron-proxy; /developer-guide/architecture is silent
- Notes: Limits stated: a stolen CA key, redirected egress or raw sockets defeat it; Bedrock SigV4 and Vertex service-account auth keep real credentials in the sandbox; Hermes' own LLM calls are not rewritten. (corrected in review: the proxy never binds 0.0.0.0 but binds the Docker bridge gateway on Linux and loopback on Docker Desktop; subcommands also include stop, disable, config; the overview page says only "optional", details are on /user-guide/egress/iron-proxy; all stated limits verified)

### Docker network isolation recipe
- What: A compose recipe with an `internal: true` network (no route out) for agent and dashboard, and an `egress` network only the dual-homed gateway joins, optionally with a Squid allowlist proxy. Documented limits: DNS can still resolve externally; not a sandbox with the local terminal backend.
- Status: GA (documentation recipe)
- Evidence: /user-guide/egress/network-isolation
- Notes: The page's service names are inconsistent (`dashboard` vs `hermes-dashboard`).

### SSRF, website blocklist and content checks
- What: SSRF protection on by default (RFC 1918, loopback, link-local incl. metadata, CGNAT, DNS failures blocked, redirects re-validated per hop); `security.allow_private_urls` default false and cloud metadata always blocked; `security.website_blocklist`. Commands are flagged for curl/wget carrying secret variables and for invisible or bidi Unicode. Context files, memory entries and cron prompts are scanned for injection phrases, hidden HTML comments and exfiltration patterns.
- Status: GA
- Evidence: /user-guide/security; /user-guide/features/context-files
- Notes: A bundled scanner "tirith" is documented as removed with nothing replacing it.

### Supply-chain advisory scanner and lazy installs
- What: A stdlib scanner checks the active venv against known-compromised package versions (CLI banner, `hermes doctor`, gateway logs). Lazy dependency installs are on by default (`security.allow_lazy_installs`); verified tool archives use SHA-256.
- Status: GA
- Evidence: /user-guide/security
- Notes: None.

### Gateway user authorization and DM pairing
- What: Default deny. Order: per-platform allow-all, DM pairing list, platform allowlist, `GATEWAY_ALLOWED_USERS`, `GATEWAY_ALLOW_ALL_USERS`, deny. Pairing: 8-char code from a 32-char alphabet via `secrets.choice`, expires in 1 h, 1 request per 10 min per user, max 3 pending per platform, 5 failures give a 1 h lockout, files chmod 0600. Admin vs regular users per platform and scope; slash-command gating is opt-in (`allow_admin_from`).
- Status: GA
- Evidence: /user-guide/security (User Authorization); /user-guide/messaging; /developer-guide/gateway-internals
- Notes: None.

### Managed scope (admin-pinned config)
- What: Root-owned `config.yaml`/`.env` in `/etc/hermes` override user files and shell env per key; users cannot change pinned keys. Plugin isolation settings can be pinned.
- Status: GA, v1
- Evidence: /user-guide/managed-scope
- Notes: Linux/POSIX first; no signed files or MDM; enforcement is filesystem permissions only; managed `.env` is world-readable; subprocesses can still set different env values.

### Plugin trust model
- What: Plugins are opt-in via `plugins.enabled`. Capabilities (MCP access, gateway message injection, platform actions) are declared in `plugin.yaml` and consent-recorded, off by default. Install runs a static scan with safe/caution/dangerous verdicts (dangerous not overridable). `--ref` pins a 40-char SHA. Default runs plugins in-process; `plugins.isolation: host` runs them in a separate process per profile.
- Status: GA
- Evidence: /user-guide/features/plugins
- Notes: Docs state plainly that a malicious in-process plugin can ignore the gates and that catalog admission is "not a code audit". (verified in review; the page says 27 lifecycle hooks but lists 38 names, a page inconsistency; the survey's 27 follows the page text)

### Secret management sources
- What: Secrets can come from `.env`, 1Password, Bitwarden Secrets Manager or a command helper; browser logins use a separate vault (see browser). `security.redact_secrets` masks key-like strings in output and logs. Docker and SSH forward no env by default.
- Status: GA
- Evidence: /user-guide/security; /guides/secure-hermes-on-a-work-machine; /docs/llms.txt (Secrets pages listed, not opened)
- Notes: Secrets sub-pages not opened.

### 6. Context management and compaction

### Context compression (two layers)
- What: Primary compressor inside the tool loop fires at 50% of context (floored at 0.75 for windows under 512K); a gateway "hygiene" pass at 85% is a safety net. Algorithm: prune old tool results over 200 chars without an LLM call; protect the first 3 messages and a tail; summarize the middle once via an auxiliary model; re-compressions update the previous summary. Summary template: Goal, Constraints, Progress, Key Decisions, Relevant Files, Next Steps, Critical Context. Default `lean` tail mode keeps 2.5% of the window plus an identifier-preserving log, anchor index, verbatim user messages and a `session_search` pointer. Per-model thresholds. Failure ladder 60/300/900 s then a deterministic fallback summary.
- Status: GA
- Evidence: /developer-guide/context-compression-and-caching
- Notes: If the summary model's window is smaller than the main model's, middle turns are dropped without a summary.

### In-place compaction lineage
- What: Default `in_place: true` keeps one session id and soft-archives pre-compaction turns (still searchable). `in_place: false` creates a child session linked by `parent_session_id`.
- Status: GA
- Evidence: /developer-guide/context-compression-and-caching; /user-guide/sessions
- Notes: The sessions page describes compression as creating a child session; the compression page says in-place is the default. Sources disagree. (verified in review: both quotes confirmed; compression thresholds 50%/0.75 floor/85%, lean 2.5% with 10K floor and 25K cap, 60/300/900 s ladder, smaller summary model drops middle turns silently)

### Micro-compaction
- What: Optional mode that, after each turn, summarizes the oldest unsummarized exchange into a rolling summary marker (user messages never summarized; defrag pass re-summarizes the summary).
- Status: opt-in, default off
- Evidence: /developer-guide/micro-compaction
- Notes: Page warns each pass invalidates the cached prefix; no maturity label.

### Prompt caching strategy
- What: Stable, context and volatile prompt tiers; the prompt is not changed mid-conversation except by explicit actions. For Claude (native or OpenRouter) a `system_and_3` strategy uses up to 4 cache breakpoints with `5m`, `1h` or `auto` TTL. Memory writes mid-session reach the prompt only next session; skill content and skills-index changes travel as user messages to keep the prefix stable.
- Status: GA
- Evidence: /developer-guide/context-compression-and-caching; /developer-guide/prompt-assembly
- Notes: Prompt-assembly page is internally inconsistent about the tier of the skills index.

### Pluggable context engine
- What: The compressor can be replaced by a plugin (one active, `context.engine`).
- Status: GA
- Evidence: /user-guide/features/plugins; /developer-guide/architecture
- Notes: Detail page not opened.

### 7. Memory and persistent instructions

### Built-in file memory (MEMORY.md and USER.md)
- What: Two small curated files in `~/.hermes/memories/`: MEMORY.md (agent notes, 2,200 chars) and USER.md (user profile, 1,375 chars), per profile. Loaded at session start as a frozen snapshot in the system prompt; mid-session writes go to disk but show only next session. `memory` tool actions: add, patch, remove (no read, no replace); over-limit writes error and force the agent to consolidate. No auto-compaction. Entries are scanned for injection and invisible Unicode; exact duplicates rejected.
- Status: GA
- Evidence: /user-guide/features/memory (Memory)
- Notes: Fixed cost about 1,300 tokens. (verified in review: 2,200 / 1,375 chars, no read/replace action, frozen snapshot, write_approval false)

### Background self-review (memory and skill writes)
- What: After turns, a background review can write memory or update skills, on its own configurable cheaper model with an input-token cap. `memory.write_approval` and `skills.write_approval` stage writes for `/memory pending|approve|reject` and `/skills pending|diff|approve|reject`. `/journey` shows a timeline of what was learned with list, edit and delete.
- Status: GA
- Evidence: /user-guide/features/memory; /user-guide/features/skills
- Notes: Write approval defaults to false.

### Session search (FTS5)
- What: All CLI and messaging sessions stored in SQLite (`state.db`) with FTS5. `session_search` returns actual stored excerpts with no LLM summarization; four call shapes (discovery query, scroll around a message, read, browse); FTS5 syntax supported.
- Status: GA
- Evidence: /user-guide/features/memory; /user-guide/sessions
- Notes: The README says "FTS5 session search with LLM summarization"; the memory and sessions pages say no LLM summarization. Sources disagree (README likely stale). (verified in review: contradiction is real; sessions page: no LLM calls, views of actual messages)

### External memory providers and Honcho user modeling
- What: One external provider can run alongside built-in memory (bundled RetainDB, ByteRover; via catalog Honcho, Hindsight, Supermemory, Mem0, OpenViking, Holographic). Honcho (Plastic Labs): after turns it derives user insights through a 1 to 3 pass "dialectic"; injects a base context (summary, user representation, peer card) plus a dialectic supplement; tools `honcho_profile`, `honcho_search`, `honcho_context`, `honcho_reasoning`, `honcho_conclude`; session strategy per-session, per-directory, per-repo or global.
- Status: GA (plugins)
- Evidence: /user-guide/features/memory; /user-guide/features/honcho
- Notes: Honcho can be a hosted service or self-hosted; hosting details not verified.

### Context files (AGENTS.md family) and SOUL.md
- What: One project context file loads, first match of `.hermes.md`/`HERMES.md`, `AGENTS.override.md`, `AGENTS.md`, `CLAUDE.md`, `.cursorrules`. In git repos AGENTS.md files from git root down to the cwd load, deeper taking precedence; subdirectory files are discovered lazily when tool calls touch paths (up to five parents, 32,000-char cap). Per-file cap scales with the model window, head 70% / tail 20% truncation. `SOUL.md` in `HERMES_HOME` is the global identity (slot 1 of the prompt); 14 built-in personality presets overlay it. Flagged project files are blocked; a flagged SOUL.md in home loads with a warning.
- Status: GA
- Evidence: /user-guide/features/context-files; /user-guide/features/personality; /developer-guide/prompt-assembly
- Notes: None.

### Import from other agents
- What: `agent-importers` plugin migrates from OpenClaw, Claude Code, Codex CLI, Cursor, Grok Bot and OpenBot: instructions become memory entries, MCP servers and skills carried over, Claude `Bash(...)` rules map to allow/deny lists, routines become paused cron jobs. Secrets excluded unless `--migrate-secrets` (OpenClaw). Per-item plan shown first; `--sync` re-imports changed sources.
- Status: GA
- Evidence: /user-guide/import-from-other-agents
- Notes: None.

### 8. Planning, tasks and modes

### Todo tool
- What: `todo_list` keeps a session task list with nested subtasks.
- Status: GA
- Evidence: /reference/tools-reference
- Notes: None.

### Plan mode
- What: nothing documented. The CLI page explicitly does not describe a plan mode; `/goal`, `/loop`, `/heartbeat` are the nearest session-attached features.
- Status: unclear
- Evidence: /user-guide/cli
- Notes: Absence on one page only; other pages not exhaustively searched.

### Clarify tool
- What: The agent asks 1 to 5 questions (single, multi-select or open) with messaging-platform replies as numbers.
- Status: GA
- Evidence: /reference/tools-reference; /user-guide/messaging
- Notes: None.

### 9. Sub-agents and multi-agent orchestration

### Delegation (`delegate_task`)
- What: Spawns isolated child agents (fresh conversation, only `goal` plus `context` plus workspace context files), batch via `tasks` with optional `output_schema` and images. Defaults: 10 concurrent children, depth 1 (flat; raise to 2 for `orchestrator` role), 250 iterations. Children inherit toolsets and cannot use `clarify`, `memory`, `send_message`, `cronjob`; leaves cannot delegate. Control actions list, steer, stop. Per-child model/provider overrides. Only the final summary returns. Stall monitoring with budget warning and force-finalize.
- Status: GA
- Evidence: /user-guide/features/delegation
- Notes: Cost grows multiplicatively with depth, per the page. (verified in review)

### Delegation worktree isolation
- What: `delegation.worktree_isolation` (default false) gives each child a git worktree and branch `hermes-subagent/subagent-<id>`; empty clean trees are pruned. Git repos on the local backend only; otherwise silently shared workspace.
- Status: GA, opt-in
- Evidence: /user-guide/features/delegation; /user-guide/git-worktrees
- Notes: None.

### Kanban multi-agent board
- What: A durable SQLite task board (`kanban.db`) shared across profiles. Each worker is a separate OS process spawned by a dispatcher (in the gateway, about 60 s tick) as a named profile; states triage, todo, ready, running, blocked, review, done, archived; parent/child dependencies, heartbeats, review handoffs, scratch/dir/worktree workspaces, boards as hard boundary, PR completion contracts, goal-mode cards. Agents use `kanban_*` tools, humans use CLI, slash command or dashboard tab.
- Status: GA
- Evidence: /user-guide/features/kanban (first 100,000 characters read)
- Notes: Threat model stated as single-host, trusted local user; REST routes unauthenticated by design behind localhost. Remaining 37k characters of the page not read. (verified in review: 14 kanban tools, 8 states, dispatcher tick default 60 s, crash reclaim, auto-block after 2 spawn failures by default, protocol_violation; no maturity label)

### Profiles, Bot Mode and group chats
- What: A profile is a separate Hermes home (config, `.env`, SOUL, memories, sessions, skills, cron, state, gateway). Profiles do not sandbox the agent. Bot Mode (Desktop, on by default) presents profiles as bots that can join rooms of 2 to 6 bots, reply in turn (up to 3 rounds), with hop limit 4, per-round caps, scoped expiring access grants and approvals that cannot be disabled in rooms.
- Status: GA; bot-to-bot live interruption listed as future work
- Evidence: /user-guide/profiles; /user-guide/bot-mode
- Notes: Two agents on one home corrupt memory (docs warn).

### Agent-to-Agent (A2A) protocol
- What: Plugin supporting A2A v1.0 both ways: agent card at `/.well-known/agent-card.json`, JSON-RPC on port 9900, SSE streaming, HMAC-signed push notifications; outbound `a2a_*` toolset off by default. Localhost-only without a token; per-peer tokens, rate limit 60/min, audit log.
- Status: GA, with a stated migration gap for old forwarded sessions
- Evidence: /user-guide/messaging/a2a
- Notes: Interop verified against the official Python SDK, per the page.

### 10. Background, long-running and scheduled work

### Cron jobs (agent tasks)
- What: Jobs are agent runs (fresh agent, no history, attached skills injected), not shell cron. Schedules: one-shot `in 30m`, intervals, day/time phrases, cron expressions, ISO timestamps, per-job timezone. Stored in `~/.hermes/cron/jobs.json` with history in `executions.db`; scheduler ticks every 60 s inside the gateway. Delivery to origin, local files or any platform home channel or explicit chat/thread; `[SILENT]` suppresses. `context_from` chains outputs; `continuity` feeds a job its prior output. Prompts are scanned at create time; cron runs cannot create jobs by default; dangerous commands denied in cron by default.
- Status: GA
- Evidence: /user-guide/features/cron
- Notes: Last 1,090 chars of the page not read. (verified in review: 60 s tick, jobs.json, executions.db, no job creation from cron by default; the claim "dangerous commands denied in cron by default" is not on the cron page but follows from the security page's headless default-deny)

### Script-only cron and wake gating
- What: `--no-agent --script <file>` runs a script on schedule and delivers stdout verbatim with no LLM; scripts must live in `$HERMES_HOME/scripts/`. A last-line `{"wakeAgent": false}` skips a tick; also works as a pre-check for agent jobs.
- Status: GA
- Evidence: /user-guide/features/cron
- Notes: None.

### Slash `/bg` and `/btw`
- What: `/bg` runs a task in a separate session with its own history; results post back to the originating chat.
- Status: GA
- Evidence: /user-guide/messaging
- Notes: `/btw` listed, not described.

### Webhook-triggered runs
- What: Events from GitHub, GitLab and others trigger agent runs through the gateway; MCP event subscriptions use dynamic signed webhook routes (experimental).
- Status: GA (webhooks); experimental (MCP events)
- Evidence: /docs/llms.txt (Webhooks listed); /user-guide/features/mcp
- Notes: Webhook page itself not opened.

### 11. Sessions: persistence, resume, fork, checkpoints, undo

### Session storage and resume
- What: Every conversation in SQLite `state.db` (WAL; FTS5) with full messages, tool calls, system prompt snapshot, tokens. `-c` resumes the latest session per terminal; `-r` by id, title or `latest`; `--in <dir>`; resume restores cwd and re-reads current config but keeps the model. Titles auto-generated. `/handoff <platform>` moves a live CLI session to a messaging home channel. Gateway crash mid-turn marks the turn `unknown`, not replayed, and the session blocks until discarded.
- Status: GA
- Evidence: /user-guide/sessions; /developer-guide/gateway-internals
- Notes: Docker page: WAL is unsafe on virtiofs/9p mounts.

### Rewind, fork and branch
- What: Esc-Esc or `/rewind` opens a transcript browser: edit a prior prompt to cut the session back, or press `f` to fork a new session before it. `/branch [title]` in gateway CLI mode. `/undo` rewinds the conversation only.
- Status: GA
- Evidence: /user-guide/cli; /user-guide/sessions; /user-guide/checkpoints-and-rollback
- Notes: Sessions page has no dedicated branching section.

### Session export, trace export and pruning
- What: Export as `jsonl`, `md`/`qmd`, `html`, or `trace` (Claude Code JSONL for Hugging Face), with `--redact` and per-prompt-only options. Prune/archive/pin commands, optional auto-prune (off by default), `optimize`, journal mode switching.
- Status: GA
- Evidence: /user-guide/sessions
- Notes: See also trajectories.

### Shadow-git checkpoints and rollback
- What: Opt-in (off by default since v2). One shared bare git repo in `~/.hermes/checkpoints/store/` with a ref per project; snapshots taken before `write_file`/`patch` and before destructive terminal commands (`rm`, `mv`, `sed -i`, redirects, `git reset`...), at most one per directory per turn. `/rollback` lists, restores (skips files the user edited since unless `--all`), diffs, restores one file; a pre-rollback snapshot makes restore undoable. Caps: 20 snapshots per project, 500 MB total, 10 MB per file, 7-day retention. Never snapshots `/` or `$HOME`; disabled on container backends.
- Status: GA, off by default
- Evidence: /user-guide/checkpoints-and-rollback
- Notes: Nested git repos stored as gitlinks and not fully recoverable. (verified in review: caps 20/500 MB/10 MB/7 d, container backends excluded, local and SSH unaffected, /rollback behaviours; page also skips dirs over 50,000 files)

### Session heartbeats and goal state persistence
- What: Goal and heartbeat state stored in `SessionDB.state_meta`, restored by `/resume`.
- Status: GA
- Evidence: /user-guide/features/goals; /user-guide/features/heartbeat
- Notes: None.

### 12. Git and version control

### Worktree mode (`hermes -w`)
- What: Creates a disposable worktree under `<repo>/.worktrees/` on branch `hermes/hermes-<hash>`; checkout runs with the repo's own git hooks, fsmonitor and filters off. On exit with dirty or unpushed work, offers keep, archive-and-remove, or discard. Stale sweep every 6 h (24 h idle for auto trees, 72 h named); archives stored as commits under `refs/hermes/archive/<tree>`. `hermes worktree list|prune|restore`.
- Status: GA
- Evidence: /user-guide/git-worktrees; /user-guide/cli
- Notes: Docs say LFS files appear as pointers.

### GitHub PR review and webhook flows
- What: Guides for a PR reviewer agent and webhook-triggered reviews; Kanban PR completion contracts check required GitHub checks.
- Status: GA (guides)
- Evidence: /docs/llms.txt (guides listed); /user-guide/features/kanban
- Notes: Guide pages not opened. No dedicated commit/PR tool documented in the tools reference.

### 13. Extensibility: MCP, plugins, skills, hooks, SDKs, protocols

### MCP client
- What: Stdio, streamable HTTP and legacy SSE transports; config under `mcp_servers` in `config.yaml`; tools registered as `mcp_<server>_<tool>` with per-server toolsets and include/exclude globs; mTLS client certs; OAuth with PKCE, DCR or client-ID metadata documents, refresh, headless options; sampling (on by default, rate limited) and elicitation (on by default, always asks even under YOLO); `list_changed` refreshes; lazy servers; per-tool cost declarations and a per-chat `spend_limit` that asks even on trusted servers; event subscriptions (experimental, signed webhook routes). Stdio servers get only a safe env baseline; invisible Unicode tag characters stripped from results; error redaction.
- Status: GA; event subscriptions experimental
- Evidence: /user-guide/features/mcp; /reference/mcp-config-reference (listed, not opened)
- Notes: Spend totals are in memory and reset on restart. Prompt and resource change notifications received but not acted on. (verified in review: elicitation default-on and not answered by YOLO, spend_limit asks even on trust:full, sampling default-on 10 rpm)

### Hermes as an MCP server
- What: `mcp-serve` plugin exposes messaging conversations and optionally web, browser, vision, image, skills and TTS tools to MCP clients.
- Status: GA (plugin)
- Evidence: /user-guide/features/mcp
- Notes: Replaces the old `hermes mcp serve`.

### Skills (agentskills.io standard)
- What: SKILL.md with frontmatter (name, description, version, platforms, `metadata.hermes`) and optional `references/`, `templates/`, `scripts/`, `assets/`. Progressive disclosure: `skills_list` (about 3k tokens), `skill_view`, `skill_view` with path. Conditional activation via `requires_*` / `fallback_for_*`. Every skill is a slash command; up to five can be stacked; `~/.hermes/skill-bundles/*.yaml` groups skills; `/learn` makes a skill from a directory, URL or document. About 90 bundled and 60 optional skills.
- Status: GA
- Evidence: /user-guide/features/skills; /reference/skills-catalog (title only, count from llms.txt)
- Notes: Skill precedence: project, local, `create_dir`, `external_dirs`. Project-local skills need an explicit `hermes skills trust`.

### Agent-created skills (`skill_manage`)
- What: The agent creates, patches (preferred, token-cheap), rewrites, and deletes its own skills and supporting files, as "procedural memory". An advisory linter warns on incident-style logs, over 60 reference files or bodies over about 24k chars. Writes can be staged for human approval.
- Status: GA
- Evidence: /user-guide/features/skills
- Notes: `vendor-claim` that skills "improve during use" appears in the README; the mechanism documented is patching by the agent plus the curator. (verified in review: action list create/patch/delete/write_file/remove_file, patch preferred, linter warns only, write_approval default false)

### Skills Hub, trust levels and scanning
- What: Sources: official, skills-sh (Vercel), well-known URL index, direct URL, GitHub taps, clawhub, lobehub, browse-sh. Trust levels builtin, official, trusted (named orgs), community. Every install scanned for exfiltration, prompt injection, destructive commands; `--force` cannot override a `dangerous` verdict; optional NVIDIA SkillEvaluator advisory.
- Status: GA
- Evidence: /user-guide/features/skills
- Notes: None.

### Curator (maintenance of agent-created skills)
- What: Idle-triggered background fork (default every 168 h after 2 h idle). Phase 1 deterministic with no LLM: unused skills become stale at 14 days, archived at 30 to `~/.hermes/skills/.archive/`. Phase 2 (opt-in) LLM consolidation merges near-duplicates into umbrella skills as whole packages. Only skills marked agent-created are managed. Safeguards: no auto-delete, pin, lock (blocks all agent writes including terminal writes into the skill directory), tar.gz snapshots before consolidation, an audit ledger with before/after hashes and per-entry rollback, run reports.
- Status: GA
- Evidence: /user-guide/features/curator
- Notes: Skills referenced by any cron job (including paused) are skipped. (verified in review; refinements: pin blocks archival and deletion but the agent may still patch a pinned skill, lock blocks the agent entirely incl. restricted terminal writes; backups under `.curator_backups/` keep 2 by default and only consolidation takes a snapshot; ledger is append-only JSONL with per-file sha256 before/after; `hermes curator rollback <entry-id>`; phase 2 off by default; bundled skills archivable only with `prune_builtins`)

### Plugins (tools, hooks, platforms, backends)
- What: General plugins register tools, slash commands, CLI commands and 27 lifecycle hooks, platform adapters, image/video/delegation/terminal/computer-use providers. Special single-slot types: memory provider, context engine; model providers load many. Discovery: bundled, `~/.hermes/plugins/`, project `.hermes/plugins/` (not loaded by the gateway), pip entry points, Nix. Curated SHA-pinned catalog of 1,050+ entries (release notes).
- Status: GA
- Evidence: /user-guide/features/plugins; github.com/NousResearch/hermes-agent/releases (v0.23.0)
- Notes: Catalog size from release notes.

### Hooks (four systems)
- What: Gateway hooks (HOOK.yaml plus handler.py, passive); plugin hooks (observers, directive hooks that can block, approve, modify arguments or veto a turn, transforms); shell hooks in `config.yaml` (can block a tool call; timeouts fail closed for `pre_tool_call`); outbound signed webhooks. Gateway hooks load in-process with no sandbox or consent prompt.
- Status: GA
- Evidence: /user-guide/features/hooks
- Notes: Last 54k chars of the page not read; shell hook schema unseen.

### Python library and programmatic integration
- What: `from run_agent import AIAgent`, `chat()` and `run_conversation()`; one agent per thread. No supported wheel or sdist is published. Programmatic options: ACP, TUI JSON-RPC, HTTP API.
- Status: GA, source checkout only
- Evidence: /guides/python-library; /developer-guide/programmatic-integration (listed, not opened)
- Notes: Platform-support page lists PyPI and Homebrew installs as unsupported.

### ACP server
- What: `hermes acp` serves ACP over stdio for VS Code (ACP Client extension), Zed, JetBrains and Buzz Desktop. Shows tool activity, diffs, terminal commands and approval prompts (once, session, always, deny; 300 s timeout denies). Curated `hermes-acp` toolset without messaging or cron. Sessions persist, can be listed, resumed and forked.
- Status: GA
- Evidence: /user-guide/features/acp
- Notes: Docs warn Buzz Desktop auto-approves allow-once so `approvals.mode: manual` does not stop shell commands there. (not rechecked)

### OpenAI-compatible API server
- What: `POST /v1/chat/completions` (stateless), `/v1/responses` (server-side state), `/v1/runs` (start, poll, SSE events, stop, approval, clarify), `/api/jobs`, `/api/sessions` (with fork and chat streaming), plus health and capability endpoints. Bearer `API_SERVER_KEY` required for every deployment; default bind 127.0.0.1:8642; max 10 concurrent runs (HTTP 429 beyond); CORS off by default.
- Status: GA
- Evidence: /user-guide/features/api-server
- Notes: Exposes full toolset including terminal.

### Subscription proxy
- What: Local server (127.0.0.1:8645) that lets other OpenAI-compatible apps use a Nous Portal or xAI subscription via Hermes-managed refresh. Raw inference only; no agent tools. Has no auth of its own.
- Status: GA
- Evidence: /user-guide/features/subscription-proxy
- Notes: None.

### 14. Models, providers and inference configuration

### Provider support and auth
- What: Nous Portal, subscription OAuth logins (OpenAI Codex, Anthropic via Claude Max, xAI, Qwen, MiniMax), routers (OpenRouter and others), direct providers (Anthropic, OpenAI, Gemini, DeepSeek, z.ai, Kimi, xAI, and others), Bedrock, Vertex AI, Azure Foundry, and any OpenAI-compatible local server (Ollama, vLLM, SGLang, llama.cpp, LM Studio, LiteLLM). Keys in `~/.hermes/.env`, OAuth in `auth.json`, `key_cmd` helpers, ADC for Vertex, AWS chain for Bedrock. Needs at least 64K context for agent work. Can adopt Codex CLI and Claude Code OAuth logins (`auth.adopt_external_logins` default true).
- Status: GA
- Evidence: /integrations/providers (first 100,000 chars); /user-guide/security
- Notes: Claude Pro cannot use the OAuth path; Max consumes extra-usage credits only, per page.

### Fallback providers, credential pools and routing
- What: Turn-scoped primary fallback chain (`fallback_providers`) on 429/5xx (after retries) and 401/403/404 (immediately), reset-aware cooldown, once per turn. Credential pools rotate keys for one provider before cross-provider fallback. `provider_routing` for OpenRouter; separate fallback chains for auxiliary tasks (compression, vision, titles, approval).
- Status: GA
- Evidence: /user-guide/features/fallback-providers; /integrations/providers
- Notes: Switching invalidates prompt cache. Pool details page not opened.

### Auxiliary models per task
- What: Side tasks (compression, vision, summaries, goal judge, curator, background review, smart approval) can each be routed to a different provider and model, defaulting to the main model.
- Status: GA
- Evidence: /integrations/providers; /user-guide/features/goals; /user-guide/features/curator; /user-guide/features/memory
- Notes: None.

### Nous Tool Gateway
- What: Paid Nous Portal feature routing web search/extract, image generation, TTS, cloud browser and hosted connectors through Nous-run infrastructure with one bill.
- Status: GA
- Evidence: /user-guide/features/tool-gateway
- Notes: Page says little on privacy and data retention for gateway traffic (gap). Tools stop working if the subscription lapses.

### Web search backends
- What: Separate backends for `web_search` and `web_extract`: Firecrawl (default), SearXNG, Brave, DDGS, Exa, Parallel, Tavily, Perplexity, Keenable, xAI, OpenAI native; keyless fallback ring; page cache (20 min default); truncation with full text saved to disk.
- Status: GA
- Evidence: /user-guide/features/web-search
- Notes: None.

### 15. UI surfaces

### CLI and TUI
- What: Full terminal UI with multiline editing, slash autocomplete, status bar (model, context, cost), inline images, live work monitor, drafts stash, voice key. `hermes chat` attaches to the profile's gateway over a local WebSocket so closing the terminal does not cancel a running turn. `hermes --tui` gives an Ink UI with overlays and mouse support.
- Status: GA
- Evidence: /user-guide/cli
- Notes: None.

### Desktop app
- What: Native app sharing config, sessions, skills and memory with the CLI. Chat with live tool activity, file browser, integrated terminal, Git review pane with worktrees, live subagent monitor, Memory Graph, skills browser, Quick Entry hotkey, voice, HUD mode, multi-window. Connects to a local backend or a remote `hermes serve` gateway (OAuth recommended beyond your own network), Hermes Cloud, or SSH gateways, several at once.
- Status: GA (no stage given); macOS Apple Silicon only, Linux packaging disabled in release workflow
- Evidence: /user-guide/desktop; /getting-started/platform-support
- Notes: Page truncated at 100,000 of 127,231 chars.

### Web dashboard and Webapp
- What: `hermes dashboard` (127.0.0.1:9119): status, embedded TUI chat over WebSocket and PTY, config, API keys, profiles, skills, MCP, sessions, logs, analytics, cron, webhooks, pairing, channels, system. Non-loopback binds engage an auth gate and refuse to start without a provider (username/password, Nous Portal OAuth, or self-hosted OIDC). The Webapp (v0.23.0) serves the Desktop workspace in a browser from the gateway.
- Status: GA
- Evidence: /user-guide/features/web-dashboard; /user-guide/docker; github.com/NousResearch/hermes-agent/releases
- Notes: Dashboard page described as "no data leaves localhost" (loopback default).

### Voice, wake word, vision, TTS
- What: Voice mode in CLI, Telegram and Discord (voice channels); wake word "Hey Hermes"; clipboard image paste; TTS with ten provider options; voice-memo transcription on messaging platforms.
- Status: GA
- Evidence: /user-guide/features/overview; /user-guide/messaging
- Notes: Dedicated voice pages not opened.

### Skins, pets, themes
- What: CLI skins, desktop themes, an animated "pet" mascot reacting to activity.
- Status: GA
- Evidence: /docs/llms.txt; /user-guide/features/overview; /user-guide/features/personality
- Notes: Pets page not opened.

### 16. Browser automation and computer use

### Browser (`browser_exec`)
- What: The model writes Python against pre-imported helpers (`goto_url`, `js`, `fill_input`, `click_at_xy`, `capture_screenshot`, `cdp`); state persists across calls, variables do not. Backends: local headless Chromium (default, fresh profile, 120 s idle close), cloud (Nous, Browserbase, Browser Use, Firecrawl, plugins), Lightpanda (text-first), `/browser connect` to your own Chrome over CDP (CLI only), opt-in snapshot of your real profile. Private/LAN URLs route to a local sidecar when a cloud provider is configured.
- Status: GA
- Evidence: /user-guide/features/browser
- Notes: Docs say `browser_exec` runs model-written Python on your machine, and real-profile browsing is not an isolation boundary.

### Credential vault for logins
- What: `browser_vault` fills logins, cards, addresses and 2FA codes into pages without the model seeing them; fills refuse unless the origin matches, re-checked in-page; filling never submits. Backends: local encrypted vault, 1Password and Bitwarden CLIs, real-browser profile. Card fills always need approval and are refused headless.
- Status: GA
- Evidence: /user-guide/features/credential-vault
- Notes: Once typed, the site has the password; origin binding prevents wrong-site fill only.

### Computer use
- What: Background desktop control (no cursor movement or focus steal) on macOS (AX), Windows (UIAutomation) and Linux (AT-SPI) through the open-source cua-driver over MCP stdio. Permission modes: standard approvals, bounded (reviewed capability manifest), unrestricted. Hard-blocked key combos and dangerous type patterns; refused where nobody can answer. Image eviction and compressor stripping limit screenshot cost.
- Status: GA
- Evidence: /user-guide/features/computer-use
- Notes: Docs note macOS relies on private SkyLight APIs. Not hostable on Modal, Daytona or Vercel sandboxes yet. A 30K vs 600K token cost figure is the page's own estimate.

### 17. Remote, cloud, headless, CI and distributed execution

### Remote execution via terminal backends
- What: Modal and Daytona (serverless, hibernate when idle), Vercel Sandbox (snapshots keep filesystem not processes), SSH, Sprites. Backend choice affects checkpoints, approvals and worktree isolation as noted above.
- Status: GA
- Evidence: /user-guide/features/tools; /user-guide/security; /user-guide/checkpoints-and-rollback; README at github.com/NousResearch/hermes-agent
- Notes: Persistent-sandbox behaviour per backend only partly documented.

### Messaging gateway (platforms, reliability)
- What: One long-running process connects all configured platforms, routes per-chat sessions, runs cron and the kanban dispatcher. Platforms listed in the overview: Telegram, Discord, Slack, Google Chat, WhatsApp (Baileys and Cloud API), Signal, SMS, Email, Home Assistant, Mattermost, Matrix, DingTalk, Feishu/Lark, WeCom (two variants), Weixin, BlueBubbles and Photon iMessage, QQ, Yuanbao, Teams, LINE, ntfy, SimpleX, Raft, IRC, Buzz, A2A, plus experimental Relay. Per-channel model/provider/prompt overrides; delivery ledger (replies stored before send, redelivered up to 3 times in 24 h); circuit breaker pauses a failing adapter; watchdogs restart a frozen gateway; token locks prevent two profiles using one bot token; bot-to-bot loop guard (4 hops). Sessions persist until `/new`; no inactivity reset by default.
- Status: GA
- Evidence: /user-guide/messaging; /developer-guide/gateway-internals; /docs/llms.txt
- Notes: Session key format `agent:{namespace}:{platform}:{chat_type}:{chat_id}`. (verified in review against /user-guide/messaging: ledger "3 attempts, 24-hour freshness, then abandoned"; a send that never started is resent, a mid-send one gets a "may be a duplicate" prefix; circuit breaker stays open until `/platform resume`; watchdog default 3 strikes then exit code 75; loop guard max_hops 4; no inactivity reset. The gateway-internals page cited does NOT contain the ledger, breaker, watchdog or loop-guard details, they are on the messaging page; it does confirm unknown crashed-turn handling and token locks)

### Gateway multiplexing and multi-gateway use
- What: `gateway.multiplex_profiles: true` serves the default and all profiles from one process, each with its own state DB and secret scope (secret reads fail closed when no scope is bound). Desktop can connect to many gateways; a kanban board can span gateways.
- Status: GA
- Evidence: /developer-guide/gateway-internals; /user-guide/desktop; /docs/llms.txt
- Notes: Multi-gateway and multi-connection pages not opened.

### Hermes Relay (credential-less gateway connector)
- What: A connector service holds platform bot tokens; the gateway dials out over one authenticated WebSocket with only a per-gateway secret and delivery key, opens no inbound port, and the connector buffers messages while the gateway is offline.
- Status: experimental (wire contract may change without deprecation)
- Evidence: /user-guide/messaging/relay
- Notes: None.

### Headless and one-shot use
- What: `hermes chat -q` and `--query-file`; `-z` one-shot with worktrees; cron, webhooks and the API server run headless with deny-by-default approvals; `hermes send` pipes script output to platforms.
- Status: GA
- Evidence: /user-guide/cli; /user-guide/security; /docs/llms.txt (pipe-script-output guide listed)
- Notes: None.

### Docker deployment
- What: Official image (amd64 and arm64; tags `latest`/`stable`, `main`, `X.Y.Z`) with s6-overlay supervision, non-root `hermes` user (UID 10000), root-owned read-only install tree, data in `/opt/data` mapped to `~/.hermes`. Refuses root gateway unless overridden; non-loopback dashboard fails closed without auth. Never run two gateway containers on one data dir.
- Status: GA
- Evidence: /user-guide/docker
- Notes: Docker does not support `hermes update`.

### 18. Collaboration, teams and enterprise administration

### Admin and user roles in chat
- What: Per-platform and per-scope admins versus regular users with a command allowlist; managed scope for fleet baselines; remote config plane for profile config; profile distributions (git-installable agents carrying SOUL, config, skills, cron, MCP, never credentials or memory).
- Status: GA
- Evidence: /user-guide/messaging; /user-guide/managed-scope; /user-guide/profiles; /docs/llms.txt (Remote Config, Profile Distributions listed)
- Notes: Remote config page not opened. No SSO or audit-log product documented beyond OIDC on the dashboard and the A2A audit log.

### 19. Observability: usage and cost, logs, tracing, debugging

### Usage, insights and monitoring
- What: Status bar cost and context; `/usage`, `/insights`; dashboard Analytics page; per-run curator reports; A2A audit log; `tool_progress: log` audit routing; gateway health export and diagnostics; `hermes doctor` and per-component `doctor` commands; `agent.log`.
- Status: GA
- Evidence: /user-guide/cli; /user-guide/messaging; /user-guide/features/web-dashboard; /user-guide/features/curator; /user-guide/messaging/a2a
- Notes: Observer hooks and shared relay metrics pages listed, not opened. No tracing standard such as OpenTelemetry documented in pages read.

### Trajectory generation and batch runner
- What: `batch_runner.py` runs the agent over a JSONL of prompts with N worker processes, per-prompt isolated environments (optional per-prompt docker image and cwd), toolset distribution sampling per prompt, resumable checkpoints matched by prompt text, and outputs ShareGPT-style `trajectories.jsonl` with tool stats. Filters drop samples with no reasoning and entries with hallucinated tool names.
- Status: GA
- Evidence: /user-guide/features/batch-processing; /developer-guide/trajectory-format
- Notes: README also advertises "trajectory compression for training"; the trajectory page says compression and export commands are not covered. Defaults on the page: model `claude-sonnet-4.6` via OpenRouter, 4 workers, 10 max turns.

### Trajectory format
- What: JSONL `conversations` with roles system/human/gpt/tool, reasoning wrapped in `<think>`, tool calls as `<tool_call>` JSON blocks, results as `<tool_response>`; a Hermes function-calling system message with a `<tools>` block is generated at save time. Interactive saving via `--save_trajectories` on `run_agent.py` or `save_trajectories=True`; the `hermes` CLI has no config key for it. Completed and failed runs go to separate files.
- Status: GA
- Evidence: /developer-guide/trajectory-format
- Notes: Session `trace` export (Claude Code JSONL) is a second route to training-like data.

### 20. Configuration and customisation

### Config layout and profiles
- What: `~/.hermes/` (or `HERMES_HOME`) holds `config.yaml`, `.env`, `auth.json`, SOUL.md, `memories/`, `skills/`, `sessions/`, `cron/`, `hooks/`, `plugins/`, `logs/`, `state.db`, `checkpoints/`. Env vars override `.env`. Config migrations run on update. Precedence with managed scope: managed beats user beats env for pinned keys.
- Status: GA
- Evidence: /user-guide/docker (Volumes); /user-guide/profiles; /user-guide/managed-scope
- Notes: `/user-guide/configuration` not opened.

### 21. Security and privacy

### No telemetry statement and TLS
- What: Docs state API calls go only to the configured provider and data stays in `~/.hermes/`; TLS via `truststore`; `ssl_verify: false` marked unsafe. cua-driver telemetry disabled by default.
- Status: GA
- Evidence: /guides/secure-hermes-on-a-work-machine; /user-guide/security; /user-guide/features/computer-use
- Notes: Managed Tool Gateway and Portal logins naturally send data to Nous; privacy terms not on the pages read.

### Official domains and phishing safety
- What: A docs page for verifying genuine resources and spotting impersonation.
- Status: unclear
- Evidence: /docs/llms.txt (title only)
- Notes: Page not opened.

### 22. Licence, pricing and distribution

### Distribution
- What: MIT. Install via `install.sh` (Linux/macOS/WSL2), `install.ps1` or MSIX (Windows), Desktop bundles, Docker, Nix (Tier 2), Termux signed APT (Tier 2, prerelease). PyPI, Homebrew, AUR and Intel Mac desktop bundles are unsupported. Stable and canary: the README mentions `stable` and `canary` channels for Termux; release notes mention only a `stable` Docker tag.
- Status: GA
- Evidence: github.com/NousResearch/hermes-agent (README); /getting-started/platform-support; github.com/NousResearch/hermes-agent/releases
- Notes: Release cadence is very high: about 618 merged PRs in v0.23.1 and 776 in v0.22.6 per the notes.

### 23. Anything else or unusual

### Codex app-server runtime (optional)
- What: A documented optional runtime using Codex's app-server.
- Status: unclear
- Evidence: /docs/llms.txt (title only)
- Notes: Page not opened.

### Persona and loop-guard safety for bot rooms
- What: See Bot Mode entry.
- Status: GA
- Evidence: /user-guide/bot-mode
- Notes: None.

## 3. Distinctive or unusual

- Agent-curated procedural memory: `skill_manage` plus a deterministic-then-LLM curator with archive, lock, snapshots and a hash ledger. Interesting because it treats self-written skills as a managed asset with rollback, not an append-only pile.
- Frozen-snapshot memory with hard character caps (2,200 and 1,375) and no auto-compaction: forces the agent to curate, and preserves the prompt prefix.
- Session search returns raw FTS5 excerpts with no LLM, plus a `session_search` recovery pointer written into compression summaries.
- Credential-injecting egress proxy: sandboxes get opaque tokens, real keys stay on the host (with stated limits).
- `execute_code` tool RPC over a Unix socket with a persistent per-session Python kernel: many tool calls per model turn.
- Approvals as a layered model: smart LLM approver, fail-closed timeouts, hardline floor that YOLO cannot lift, deny globs applied before YOLO, headless defaults deny, and spend limits and elicitation that YOLO cannot skip.
- Container backends skip dangerous-command approval and disable checkpoints, treating the container as the boundary.
- Kanban with worker-per-OS-process and a dispatcher that reclaims crashed claims, auto-blocks after spawn failures and treats a clean exit without completion as a protocol violation.
- Gateway delivery ledger and durable per-session FIFO with an "unknown" crashed-turn state that is never replayed.
- Hermes Relay: gateway holds no platform credentials and dials out only.
- Tool Search bridge (`tool_search`, `tool_describe`, `tool_call`) with tiered manifests.
- Per-task auxiliary models (judge, curator, compression, review, approval).

## 4. Architecture and UX notes

- One Python core class (`AIAgent`) behind CLI, gateway, ACP, API server, batch runner and library; platform differences live in entry points.
- The CLI attaches to a per-profile gateway over a local WebSocket (single-use 30 s tickets via a control socket); the gateway owns sessions, queue, cron ticking and the kanban dispatcher. Desktop, dashboard and Webapp are further clients of the same gateway.
- State: SQLite `state.db` (WAL, FTS5) per profile; cron in `jobs.json` plus `executions.db`; kanban in `kanban.db`; shadow git for checkpoints. One writer per Hermes home is a stated rule.
- Agent and execution are separated by the terminal environment interface: the agent process stays on the host, every file and shell operation goes through the environment object. The agent's own code is outside the sandbox on remote backends.
- Prompt is three tiers (stable, context, volatile) plus ephemeral API-time content, designed to stay byte-stable for caching.

## 5. Relevance to a self-hosted master/node harness

- Environment interface (terminal backends, terminal-environment plugin): the clean "execute through one object" seam, including `uses_host_filesystem` and `is_remote` flags, maps to a node's tool layer; note it keeps the agent on the host, unlike rizzy's node which runs the loop.
- Approval layering (smart mode, hardline floor, deny before YOLO, headless defaults deny, 300 s fail-closed): a ready checklist for the approval path; note smart mode makes a model decision security-relevant.
- Container bypass of approvals plus checkpoints off on containers: a design trap to avoid; the node decides, so skipping checks by backend should be a node-side policy, not a default.
- Egress proxy with credential injection and the Docker `internal` network recipe: directly relevant to "no secret in an event" and to hostile-agent containment on nodes.
- Gateway pairing, default-deny, admin scopes and delivery ledger with exactly-once FIFO and `unknown` crashed-turn handling: relevant to one-writer-per-session and restart mid-operation.
- Session store as FTS5 over raw messages plus archived-on-compaction rows: consistent with an event log fold; in-place compaction keeping the id is a useful variant.
- Compression design (prune tool outputs first, protected head, iterative summary update, aux model must be at least as large, cooldown ladder): concrete failure modes to test.
- Memory with write-approval staging and journey/edit/delete: user control over agent-written persistence.
- Plugin trust model that admits "capabilities are consent, not isolation" with optional host-process isolation: honest framing; contrast with rizzy's "nothing the model writes is trusted".
- Idle footprint: not documented. Python and Node dependencies suggest well above the 50 MB invariant; no figure is published.

## 6. Overhyped, stale or contradictory

- README says session search uses LLM summarization; the memory and sessions pages say it returns raw excerpts with no LLM.
- Architecture page diagram says seven terminal backends, prose six; tools page lists eight including plugins.
- Delegation max_iterations: 250 (delegation page) versus 50 (agent-loop page).
- Compression lineage: default in-place (compression page) versus "creates a child session" (sessions page).
- Tool Search called "opt-in" yet default `auto` behaves as on.
- Prompt-assembly page places the skills index in both the volatile and stable tiers.
- Docker network recipe mixes `dashboard` and `hermes-dashboard` names.
- "Skills improve during use" and MoA benchmark numbers are vendor claims.
- Release notes show very high merge volume (hundreds of PRs per patch release); the docs describe a very wide surface with many "listed" pages; some features may be thin.
- Termux and several packaging paths flagged prerelease or unsupported.

## 7. Unverified

- Idle memory footprint and per-session cost: not published in pages read. Settles with a measurement.
- Whether the Tool Gateway or Portal retains prompts: privacy page not located.
- Full list of platform support quality per messaging platform (several pages not opened).
- Contents of: configuration, context-references, document-extraction, voice-mode, webhooks, credential-pools, multi-gateway, remote-config, secrets pages, observer hooks, programmatic-integration, MCP config reference, pm-audit-status, stable-releases, Codex app-server runtime, official-domains. Each is listed in `llms.txt` but not opened.
- Kanban page tail (37k chars) and hooks page tail (54k chars) and cron tail (1k chars) were not read.
- Whether `hermes serve` is separate from the gateway process: desktop page mentions it; not described on pages read.
- Skills "improve during use" mechanism beyond agent patching and curator: only README claim.
- GitHub releases page was read to 100,000 of 136,614 chars; older releases not read.

## 8. Sources (all opened)

Docs base `https://hermes-agent.nousresearch.com/docs`:
- /llms.txt (docs index): full page list.
- /developer-guide/architecture: process model, subsystems.
- /developer-guide/agent-loop: loop, budget, fallback.
- /developer-guide/prompt-assembly: prompt tiers.
- /developer-guide/context-compression-and-caching: compression, caching.
- /developer-guide/micro-compaction: micro-compaction.
- /developer-guide/gateway-internals: gateway model.
- /developer-guide/terminal-environment-plugin: backend interface.
- /developer-guide/trajectory-format: trajectory fields.
- /user-guide/security: approvals and security layers.
- /user-guide/features/memory: memory.
- /user-guide/features/skills: skills.
- /user-guide/features/curator: curator.
- /user-guide/features/delegation: delegation.
- /user-guide/features/tools: tools and backends.
- /user-guide/features/tool-search: tool search.
- /user-guide/features/cron: cron.
- /user-guide/features/goals, /loops, /heartbeat: goals, loops, heartbeat.
- /user-guide/features/code-execution: execute_code.
- /user-guide/features/hooks: hooks.
- /user-guide/features/batch-processing: batch runner.
- /user-guide/features/browser, /computer-use, /credential-vault: browser and vault, computer use.
- /user-guide/features/mcp: MCP.
- /user-guide/features/plugins, /context-files, /personality: plugins, context files, SOUL.
- /user-guide/features/web-dashboard, /api-server, /acp, /honcho, /fallback-providers, /mixture-of-agents, /lsp, /subscription-proxy, /tool-gateway, /web-search, /overview: respective features.
- /user-guide/kanban at /user-guide/features/kanban: kanban.
- /user-guide/checkpoints-and-rollback: checkpoints.
- /user-guide/egress, /egress/iron-proxy, /egress/network-isolation: egress.
- /user-guide/docker: Docker deployment and backend.
- /user-guide/sessions: sessions.
- /user-guide/messaging, /messaging/a2a, /messaging/relay: gateway.
- /user-guide/profiles, /bot-mode, /managed-scope, /desktop, /git-worktrees, /import-from-other-agents, /cli: respective topics.
- /reference/tools-reference: tool inventory.
- /integrations/providers: providers.
- /guides/python-library, /guides/secure-hermes-on-a-work-machine: library, posture.
- /getting-started/platform-support: platforms and distribution.

Other:
- https://github.com/NousResearch/hermes-agent: README (features, backends, licence, stars).
- https://github.com/NousResearch/hermes-agent/releases: v0.23.1 and earlier release notes (first 100,000 chars).

## 9. Review

Reviewed 2026-11-10 by an independent reviewer, public docs only. Checked 27 entries of about 95 (every cited page re-fetched for the specific fact; the fetch tool summarises pages).

| Entry | Verdict | Reason |
|---|---|---|
| Curator | confirmed | 168 h/2 h idle, 14/30 days, `.archive/`, consolidation off by default, lock, tar.gz backups, sha256 ledger, rollback; refinements added (pin vs lock, backups only for consolidation) |
| Agent-created skills / Skills hub | confirmed | actions, linter thresholds, write_approval false, `--force` cannot override dangerous |
| Memory size limits | confirmed | 2,200 and 1,375 chars, frozen snapshot, no read/replace |
| Session search | confirmed | no LLM on memory and sessions pages; README says LLM summarization; contradiction real |
| Compaction lineage | confirmed | sessions page says new continuation session, compression page says in-place default |
| Context compression | confirmed | 50% (0.75 floor under 512K), 85% hygiene, lean tail, ladder |
| Approval modes | confirmed | smart default, 300 s fail-closed, headless deny |
| Hardline and deny rules | confirmed | not liftable by YOLO/off; deny before YOLO |
| What YOLO cannot lift (MCP elicitation, spend_limit) | confirmed | quoted on MCP page |
| Container bypass of approvals | confirmed-with-correction | stated on the security page, not on the tools page cited; tmpfs 512 MB/nosuid not confirmed |
| Egress proxy (iron-proxy) | confirmed-with-correction | limits all verified; bind is Docker bridge on Linux, loopback on Docker Desktop; more subcommands |
| Terminal backends | confirmed | six built-in plus two plugins; resources; SSH wording |
| Gateway delivery/replay rules | confirmed-with-correction | ledger, breaker, watchdog, loop guard are on /user-guide/messaging, not gateway-internals |
| Hermes Relay | confirmed | outbound-only single WebSocket, no inbound port, durable buffering, experimental |
| Kanban | confirmed | 14 tools, 8 states, 60 s tick, reclaim, protocol_violation; single-host threat model |
| Checkpoints | confirmed | opt-in, caps, container exclusion |
| Delegation | confirmed | 10/depth 1/250, blocked tools |
| Goals | confirmed | |
| Agent loop | confirmed-with-correction | 500 and 50 confirmed; "synchronous" not on that page |
| execute_code | confirmed | |
| Cron | confirmed | cron-default-deny claim lives on security page |
| Plugin trust model | confirmed | page says 27 hooks but lists 38 |
| MCP client | confirmed | |
| Sessions / resume | confirmed | crash-unknown, `-c`/`-r` |
| README identity | confirmed | MIT, 262.4k stars, six built-in backends, "self-improve" vendor claim |
| Tool inventory, Tool Search, LSP, Desktop, A2A, API server | not checked | |

Borrowed-feature findings: none detected. The survey records Hermes-specific mechanisms, and where it names inspirations (Codex `/goal`) the docs do too.

Notable omissions: not assessed against the full docs nav; the llms.txt index was not re-skimmed. The survey itself lists unopened pages (context-references, webhooks, credential-pools, remote-config, secrets, Codex app-server runtime, official-domains) that remain open gaps.

Trust judgement: the unchecked remainder is likely reliable. Every substantive claim checked held; errors were mis-attributed page citations and small details. The survey is also unusually candid about page contradictions, and those were real. Least trustworthy: counts from llms.txt or release notes (catalog size, skill counts) and entries marked "listed, not opened".
