# OpenHands (Agent Canvas) - Round 1 public-surface survey

Surveyed 2026-11-10. Public docs and READMEs only; no source code read. All URLs below were opened in this pass. Docs fetched through the `.md` variants of docs.openhands.dev pages (summaries produced by a fetch tool, so details are paraphrased from those renderings).

Base for evidence shorthand: `D:` = `https://docs.openhands.dev`.

## 1. Identity

- Name: OpenHands (formerly All-Hands-AI). The current front end is branded **Agent Canvas**. The product is now a family: Agent Canvas (browser client), Agent Server (execution), Automation Server (schedules and events), Software Agent SDK (the agent framework and the Agent Server, Python), Sandbox Server (community-supported standalone sandbox control plane), OpenHands Cloud (commercial SaaS), OpenHands Enterprise (commercial, self-hosted). Source: D:/overview/introduction.md.
- Repositories (from GitHub pages): `OpenHands/OpenHands` (Agent Canvas UI and launcher; TypeScript; MIT; 93.1k stars), `OpenHands/software-agent-sdk` (SDK, tools, workspace, agent-server, TypeScript client; MIT per README), `OpenHands/automation` (Automation Server), `OpenHands/typescript-client`, `OpenHands/OpenHands-CLI` (MIT, "no longer actively maintained"), `OpenHands/extensions` (public skills/plugins registry).
- Open or closed: open core. Canvas, SDK, Agent Server, CLI are MIT per READMEs. Cloud is commercial SaaS; Enterprise is commercial self-hosted (D:/enterprise/enterprise-vs-oss.md). The docs say each repo has its own licence. The enterprise-vs-oss page does not name the open-source licence; the MIT claim comes from the GitHub READMEs. Whether the Enterprise server code is public: not found (see Unverified).
- Version: `00-targets.md` says v1.38.0 (2026-11-10); GitHub Releases page confirms v1.38.0 dated 2026-11-10. Docs release notes stop at 1.35.0 (2026-11-03), so docs lag by three releases (confirmed). Docker quickstart in README pins `agent-canvas:1.38.0`. Bundled agent-server was 1.59.0 in Canvas 1.35.0. Enterprise release notes are versioned separately (0.103.0 is the newest listed in the index).
- Correction to `00-targets.md`: none of substance. Note that the target file's "(now Agent Canvas)" is right for the UI, but the legacy "Local GUI" (`openhands serve`) and the CLI are deprecated/unmaintained, not the same product.

## 2. Features by area

### Area 1: Agent loop and execution model

### Stateless step loop over an event log
- What: The `Agent` is a stateless executor; each `step()` is atomic. A step: run pending approved actions; optionally condense; query the LLM; parse tool calls into `ActionEvent`s or a text reply into a `MessageEvent` (ends the step); check confirmation; execute and emit `ObservationEvent`s. Agent reads history and appends events.
- Status: GA
- Evidence: D:/sdk/arch/agent.md (Agent architecture); D:/sdk/arch/conversation.md
- Notes: Design principles state components are stateless, immutable, Pydantic-typed (D:/sdk/arch/overview.md).

### Conversation as the unit; local vs remote with one API
- What: `Conversation` returns a `LocalConversation` (in-process, local workspace) or a `RemoteConversation` (workspace is remote; execution via agent-server over HTTP/WebSocket). Same API; swapping the workspace argument switches modes. State is a Pydantic model; updates run under a FIFO lock.
- Status: GA
- Evidence: D:/sdk/arch/conversation.md; D:/sdk/guides/agent-server/overview.md
- Notes: —

### Event-sourced, append-only event log
- What: `EventLog` is described as an immutable append-only store with indexes; auxiliary services read it but never mutate state directly.
- Status: GA
- Evidence: D:/sdk/arch/conversation.md
- Notes: Compare the reader's "one writer per session" invariant; OpenHands says the log is event-sourced but docs do not state a single-writer rule.

### Goal completion loop (`/goal`)
- What: Runs the agent until a separate judge LLM audits the transcript and says the objective is done, or a round cap is hit (default 10 in the SDK). Judge returns score, complete flag and "missing" text that is fed back as a follow-up. Invalid judge JSON defaults to "not complete". In Canvas, `/goal [--max N] <objective>`; a normal message interrupts it.
- Status: GA (SDK guide); Canvas feature described as needing backend goal-route support
- Evidence: D:/sdk/guides/convo-goal.md; D:/openhands/usage/agent-canvas/conversations.md (Goals)
- Notes: Extra model calls per round. (verified in review) (SDK part verified: default 10 rounds, judge fallback; the Canvas `/goal` syntax rests on conversations.md, not re-checked)

### Critic and iterative refinement
- What: An extra evaluation model scores the likelihood that the agent's work succeeded (on message or finish, not each tool call by default). With iterative refinement on, a score below threshold (default 0.6) sends the agent back, up to 3 retries. Shows category probabilities (incomplete changes, missing validation, etc.).
- Status: experimental
- Evidence: D:/openhands/usage/agent-canvas/critic.md; D:/sdk/guides/critic.md (index listing)
- Notes: Default hosted critic is free "currently"; third-party ACP agents may not expose it.

### Stuck detector
- What: On by default. Flags repeating action/observation (4+), repeating action/error (3+), monologue (3+ messages), ping-pong (6+ cycles) and repeated context-window errors, over events after the last user message.
- Status: GA
- Evidence: D:/sdk/guides/agent-stuck-detector.md
- Notes: Exact default halt behaviour not specified by the page. (verified in review)

### Send message while running; pause and resume; ask agent
- What: Guides exist for sending a message during a run, pausing/resuming, async conversations and "Ask Agent" side questions (`/ask-agent` endpoint in the API list). Mechanics not read beyond index titles.
- Status: GA (existence), details unclear
- Evidence: D:/llms.txt index entries (/sdk/guides/convo-send-message-while-running.md, convo-pause-and-resume.md, convo-ask-agent.md; API reference "Ask Agent", "Pause Conversation")
- Notes: Pages not opened; do not rely on mechanism.

### Area 2: Built-in tools

### Tool set named in architecture docs
- What: Tools named: `finish`, `think`, `execute_bash` (terminal), `file_editor`, `glob`, `grep`, `task_tracker`; plus a browser tool set and the task (sub-agent) tool. Tools follow Action/Observation/Executor with MCP-style annotations (read-only, destructive, idempotent, open-world). `ToolRegistry` resolves specs to instances.
- Status: GA
- Evidence: D:/sdk/arch/tool-system.md; D:/sdk/guides/agent-browser-use.md; D:/sdk/guides/task-tool-set.md
- Notes: Docs do not state which tools an agent gets by default. A GPT-5 preset using an `ApplyPatchTool` exists (index only: /sdk/guides/llm-gpt5-preset.md). Agent profiles can pick "standard" or hand-chosen tools; removing a tool removes the capability (agent-profiles page). Agent can call `request_secret` (secrets page) and `launch_child_conversation` (Canvas conversations page) and an optional `SwitchLLMTool` (agent-server >= 1.31.0).

### Custom tools, including against a remote agent server
- What: SDK custom tools via `definition.py`/`impl.py` pattern; a separate guide covers custom tools with a remote Agent Server.
- Status: GA
- Evidence: D:/sdk/arch/tool-system.md; index entries /sdk/guides/custom-tools.md, /sdk/guides/agent-server/custom-tools.md
- Notes: Remote custom-tool page not opened.

### Area 3: Code editing and file operations

### File editor, glob, grep; Canvas Files and Changes views
- What: Editing via `file_editor` (view/create/str_replace/insert/undo_edit commands are referenced in the isolation cookbook). Canvas has a Files drawer with tabs and tree, per-workspace discovery limits (excluded patterns, max file count), download, and a combined Commits/uncommitted-changes drawer.
- Status: GA
- Evidence: D:/cookbook/workspace-isolation.md; D:/openhands/usage/agent-canvas/conversations.md (Files, Overview panel and Commits drawer)
- Notes: Cloud downloads only UTF-8 text files; binaries have no Download button.

### Area 4: Terminal and shell integration

### Shared terminal, bash events API
- What: Agent Server persists `bash_events/` and has endpoints to start/execute bash commands and search bash events. Legacy UI documents a Terminal tab shared by user and agent. Workspace `execute_command` returns stdout, stderr, exit code, timeout flag, duration.
- Status: GA
- Evidence: D:/sdk/arch/agent-server.md; D:/sdk/arch/workspace.md; D:/openhands/usage/key-features.md; API list in D:/llms.txt
- Notes: Whether the current Canvas keeps the same Terminal tab: the Canvas overview says Canvas shows "files and terminal output" from the server.

### Area 5: Permissions, approvals and sandboxing

### Confirmation policies
- What: `AlwaysConfirm`, `NeverConfirm`, `ConfirmRisky` (confirm when risk >= threshold; threshold default HIGH, `confirm_unknown` default True). Pending actions put the conversation in `WAITING_FOR_CONFIRMATION`; reject with a reason. Policy settable per conversation via API.
- Status: GA
- Evidence: D:/sdk/arch/security.md; D:/sdk/guides/security.md; API list "Set Conversation Confirmation Policy"
- Notes: Docs disagree on defaults: the architecture page lists ConfirmRisky as default; the guide page says the default policy when none is set is "not stated". (verified in review) (architecture page gives ConfirmRisky default, HIGH, confirm_unknown True; guide page says default not stated; both confirmed as written)

### Risk analyzers (LLM, pattern, policy-rail, ensemble)
- What: `LLMSecurityAnalyzer` adds a required `security_risk` argument to non-read-only tool schemas so the model rates each action inline (no extra call per the architecture page; the guide page says it also uses a separate LLM). Defence-in-depth analyzers: `PatternSecurityAnalyzer` (regex on shell and injection corpora), `PolicyRailSecurityAnalyzer` (composed threats like fetch-piped-to-exec), `EnsembleSecurityAnalyzer` (max risk). 30k-char scan cap per corpus. Analyzers return only a risk level; no hard deny. Docs list gaps: no homoglyph detection, thinking blocks not scanned, "not a sandbox replacement". `conversation.execute_tool()` bypasses analysis.
- Status: GA, with documented limitations
- Evidence: D:/sdk/guides/security.md; D:/sdk/arch/security.md
- Notes: The two pages disagree on whether the LLM analyzer is a single inline annotation or also a separate LLM. "Hook-based enforcement" is named as a future fix (planned). (verified in review)

### Hooks as the hard-deny layer
- What: See Area 13 (hooks). Cookbook uses PreToolUse hooks for command blacklists and workspace isolation (both heuristic, fail open on hook errors).
- Status: GA (hooks); cookbook policies are examples
- Evidence: D:/cookbook/command-blacklist.md; D:/cookbook/workspace-isolation.md
- Notes: Cookbook itself says the isolation hook is "heuristic", cannot resolve symlinks, and fails open. (corrected in review: the SDK security docs do not present hooks as part of the security model; they name hook-based enforcement only as a proposed future fix, so calling hooks 'the hard-deny layer' is the survey's own framing; cookbook caveats verified)

### Sandbox backends and their trust boundaries
- What: Host process (no isolation); Docker whole-stack (container, mounted dirs only); per-conversation Docker containers (hardened: host UID/GID, all caps dropped, `no-new-privileges`, API on 127.0.0.1 only, memory/CPU/PID limits, own secret key and session key per container); Kubernetes (Helm) runs everything in one shared pod; Cloud/Enterprise per-conversation pods under a hardened runtime (Sysbox) with Docker-in-sandbox.
- Status: GA (per-conversation Docker introduced in Canvas 1.34.0, 2026-10-27); Helm chart present
- Evidence: D:/openhands/usage/agent-canvas/architecture.md; D:/openhands/usage/agent-canvas/backend-setup/docker-execution.md; D:/openhands/usage/agent-canvas/backend-setup/kubernetes.md; D:/enterprise/docker-in-sandbox.md; GitHub releases page v1.34.0
- Notes: The docs warn that client-supplied container memory/CPU override env limits and are not capped (cannot be treated as an upper bound on a shared server). (verified in review) (per-conversation hardening, outer-server proxying, 20-minute default idle stop, uncapped client-supplied memory/CPU, Helm single shared pod, Enterprise one pod per conversation, v1.34.0 date all checked)

### No-isolation default and trust statements
- What: Docs state the product is meant for "a single user on their local workstation", not multi-tenant, with no built-in authentication, isolation or scalability (the FAQ states this generally; see Notes). The Helm page calls Canvas "an unauthenticated, single-tenant application" (only an API key gates the agent server). Whole-Docker mode needs `--privileged` for the agent to run containers; docs warn this weakens isolation.
- Status: GA statement
- Evidence: D:/overview/faqs.md; D:/openhands/usage/agent-canvas/backend-setup/kubernetes.md; D:/openhands/usage/agent-canvas/backend-setup/docker.md
- Notes: Important for the reader's "node decides" invariant: here, anyone with the key can request agent execution in that backend's workspace. (corrected in review: the FAQ states the single-user/no-auth/no-isolation/no-scalability limits generally and does not itself say 'open-source build'; that the open-source tier lacks authentication comes from enterprise-vs-oss.md. Helm 'unauthenticated, single-tenant' and the `--privileged` warning are confirmed.)

### Area 6: Context management and compaction

### LLM summarising condenser
- What: `LLMSummarizingCondenser` triggers when history exceeds `max_size` (default 120 events), keeps first `keep_first` events (default 4), summarises the middle, aims for `max_size//2`, and emits a `Condensation` event with forgotten event ids and a summary at an offset; the event log stays intact and the view applies the filter. Manual trigger by `CondensationRequest` event (agent raises it on context-window error). `PipelineCondenser` chains condensers; `NoOpCondenser`. Canvas has a context meter and "Compact context" button; API has "Condense Conversation".
- Status: GA
- Evidence: D:/sdk/arch/condenser.md; D:/sdk/guides/context-condenser.md; D:/openhands/usage/agent-canvas/conversations.md; API list
- Notes: The guide page says no product defaults are documented; the architecture page gives the 120/4 numbers. Calls the same value `max_context_length` in prose and `max_size` in code (docs inconsistency). (verified in review)

### Area 7: Memory and persistent instructions

### AGENTS.md and skill-based instructions
- What: `AGENTS.md` loads whole into the system prompt as repo context. `CLAUDE.md` and `GEMINI.md` are recognised as model-specific repo context. `.openhands/microagents/` and `.openhands/skills/` are legacy but supported.
- Status: GA
- Evidence: D:/overview/skills.md
- Notes: See Area 13 for skills.

### Opt-in persistent memory (MEMORY.md plus daily logs)
- What: Off by default (`AgentContext(load_memory=True)`; a Settings > Agent Context toggle in Canvas). Two tiers: `~/.openhands/memory/` (user) and `<workspace>/.openhands/memory/` (project), each with `MEMORY.md` (curated index; the only part injected) and `YYYY-MM-DD.md` daily logs (read on demand). Combined index cap about 6,000 chars; oldest whole lines dropped first. Injected inside an untrusted-content wrapper; agent told not to store secrets and to treat contents as unverified because cloned repos can ship their own.
- Status: GA (SDK guide); Canvas toggle documented
- Evidence: D:/sdk/guides/persistent-memory.md; D:/openhands/usage/agent-canvas/customize-and-settings.md
- Notes: Agent writes the memory itself, typically at end of task. (verified in review)

### Area 8: Planning, tasks and modes

### Task tracker tool and goal loop
- What: `task_tracker` tool exists (named in tool annotations). No separate plan mode was found documented.
- Status: unclear for plan mode
- Evidence: D:/sdk/arch/tool-system.md
- Notes: "Plan mode": nothing documented in pages opened.

### Area 9: Sub-agents and multi-agent orchestration

### Task tool set (sub-agents)
- What: Parent calls a `task` tool with a prompt and optional `subagent_type`, description, and `resume` (prior task id). A TaskManager runs a fresh sub-agent conversation to completion, returns status and text. Sub-agent conversations are saved and can be resumed. Calls are synchronous; parallelism comes from the parent's `tool_concurrency_limit`. The page refers to the older DelegateTool as retired.
- Status: GA
- Evidence: D:/sdk/guides/task-tool-set.md
- Notes: Docs state no nesting depth, count, timeout limits, and no filesystem sandboxing between parent and child. (corrected in review: the page calls DelegateTool 'retired', it does not say TaskToolSet 'replaces' it; other details verified)

### File-based agent definitions
- What: Agents are `.md` files with YAML frontmatter in `.agents/agents/`, `.openhands/agents/` (project then user). Fields: name, description, tools, model (profile or `inherit`), skills, max_iteration_per_run, color, mcp_servers (with `${VAR}`), hooks, permission_mode (always/never/confirm_risky; omitted means parent's), profile_store_dir. Plugin `agents/` dirs also supply agents.
- Status: GA
- Evidence: D:/sdk/guides/agent-file-based.md
- Notes: Close in shape to Claude Code sub-agent files.

### Child conversations in Canvas
- What: An agent can call `launch_child_conversation` to start a child on a local or Cloud target. Local children run in an isolated worktree or share the parent's workspace; result returns to the parent; launch inputs validated.
- Status: GA
- Evidence: D:/openhands/usage/agent-canvas/conversations.md (Child conversations)
- Notes: —

### Parallel tool execution
- What: `tool_concurrency_limit` on `Agent`, default 1 (sequential); no built-in locking; suggested 2-8.
- Status: experimental
- Evidence: D:/sdk/guides/parallel-tool-execution.md
- Notes: Page itself warns of race conditions.

### Other multi-agent guides (index only)
- What: Dynamic Agent Workflows, Iterative Refinement, Ask Oracle, Theory of Mind agent, Model Routing, Task-Based Model Routing exist as SDK guides.
- Status: unclear
- Evidence: D:/llms.txt index
- Notes: Pages not opened.

### Area 10: Background, long-running and scheduled work

### Automations (prompt-based, cron and event triggers)
- What: An Automation Server stores schedules and event triggers, tracks runs and dispatches conversations to the Agent Server. Types: prompt, prompt plus plugins, script bundle (tarball; no conversation, view logs). Schedules are cron with timezone. Run statuses: PENDING, RUNNING, COMPLETED, FAILED, CANCELLED, SKIPPED (skipped when backend concurrency limit is reached). Each run gets a fresh sandbox (Cloud). "Run now", keep-alive (cloud only), export/import as `<slug>.automation.json`, tags, per-run cost in USD, Mermaid diagram per automation (1.37.0), service-account runs (1.36.0), org admins can edit others' automations (1.38.0).
- Status: GA
- Evidence: D:/openhands/usage/automations/overview.md; D:/openhands/usage/agent-canvas/managing-automations.md; GitHub releases (v1.36.0-v1.38.0)
- Notes: Automations overview page is Cloud-oriented; Canvas run semantics (fresh sandbox vs the local backend) differ by backend. Docs state a standalone Agent Server does not serve `/api/automation/*`.

### Event triggers and custom webhooks
- What: Built-in sources: GitHub (Enterprise), Azure DevOps, Jira Data Center, Bitbucket Data Center. Custom webhooks registered via `POST /api/automation/v1/webhooks`; HMAC-SHA256 of the raw body verified (default header `X-Signature-256`). Filters are JMESPath with extensions (`icontains`, `glob`, `!`, `&&`, `||`). Canvas must be publicly reachable (ngrok with `--public`); docs say exposing the backend exposes the whole agent server. Fallback is polling via cron.
- Status: GA
- Evidence: D:/openhands/usage/automations/event-automations.md; D:/enterprise/enterprise-vs-oss.md
- Notes: Enterprise-vs-OSS matrix says event-driven automations on a VM need the VM reachable; local Canvas has none. (verified in review)

### Sync automations with Git
- What: Bidirectional sync of `automation.yaml` (and expanded tarball) per automation through a Git repo on an interval or "Sync now"; local pending changes win in a cycle; optional encryption of committed files.
- Status: GA (documented)
- Evidence: D:/openhands/usage/agent-canvas/git-sync.md
- Notes: —

### Pre-built automations
- What: Issue to PR, GitHub PR review assistant, GitHub repo monitor, Slack channel monitor templates; use-case pages: vulnerability remediation, code review, QA, incident triage, COBOL modernisation, dependency upgrades, "software factory", Spark migrations.
- Status: GA (existence); not opened individually
- Evidence: D:/llms.txt index
- Notes: vendor-claim for use-case pages.

### Enterprise sandbox idle/deletion lifecycle
- What: Sandbox states STARTING, RUNNING, PAUSED, ERROR, MISSING. Idle time pauses; deletion time permanently deletes; 12-hour continuous-run cap force-pauses (not configurable); limits deployment-wide.
- Status: GA (Enterprise)
- Evidence: D:/enterprise/conversations-and-sandboxes.md
- Notes: The page is internally inconsistent about whether idle leads to PAUSED or MISSING.

### Area 11: Sessions: persistence, resume, fork, checkpoints, undo

### On-disk persistence (JSON files)
- What: Default `workspace/conversations/<id>/` holds `base_state.json` (rewritten whole on every state change) and `events/` (one JSON file per event, sequential index plus event id). Resume by recreating a `Conversation` with the same id and persistence dir. Agent Server keeps `bash_events/`, `conversations/`, `project/` under `workspace/`; Canvas data lives in `~/.openhands` (or Docker volume); Helm chart uses a PVC; Enterprise uses PostgreSQL plus object storage for transcripts. Canvas's own API settings and automation DB are SQLite (Helm page names "automation SQLite DB").
- Status: GA
- Evidence: D:/sdk/guides/convo-persistence.md; D:/sdk/arch/agent-server.md; D:/openhands/usage/agent-canvas/setup.md; D:/openhands/usage/agent-canvas/backend-setup/kubernetes.md; D:/enterprise/architecture.md
- Notes: SDK page does not say whether persisted secrets are encrypted; agent-server page says `OH_SECRET_KEY` encrypts sensitive values including LLM keys.

### Fork and "Branch from here"
- What: SDK `fork()` deep-copies events, agent and skill state into a new idle conversation sharing the same workspace; metrics reset by default; `POST /api/conversations/{id}/fork` for remote (no agent override remotely). Canvas "Branch from here" on any message; from a user message it starts at that message's parent and loads text for editing. Needs a backend that supports forks.
- Status: GA
- Evidence: D:/sdk/guides/convo-fork.md; D:/openhands/usage/agent-canvas/conversations.md (Branching)
- Notes: Workspace is shared, so a fork does not fork file state.

### Checkpoints and undo
- What: Nothing documented for file checkpoints/rewind in pages opened. (The `file_editor` has `undo_edit`.)
- Status: unclear
- Evidence: D:/cookbook/workspace-isolation.md (undo_edit named)
- Notes: Nothing documented for workspace snapshot or rewind.

### Export, archive, share
- What: Export transcript as Markdown or self-contained HTML (generated client-side from loaded events). Archive hides without deleting (stored as server-side tags). Cloud sharing: public link for personal workspace, org-only for team org.
- Status: GA
- Evidence: D:/openhands/usage/agent-canvas/conversations.md
- Notes: "Download trajectory" API endpoint exists (index).

### Area 12: Git and version control

### Worktrees and git panel
- What: Per-conversation runtime choice can be Local, Docker, or Worktree (when a folder is open); Settings > Runtime sets "create worktrees from" and a branch-name prefix. Commits drawer sends commit/pull/push/PR prompts to the agent. Git changes/diff API endpoints exist.
- Status: GA
- Evidence: D:/openhands/usage/agent-canvas/backend-setup/docker-execution.md; D:/openhands/usage/agent-canvas/conversations.md; API list (Git Changes, Git Diff)
- Notes: —

### Issue-to-PR and PR review flows
- What: See Area 17/18 (GitHub, GitLab resolver-style integrations); SDK has GitHub workflow guides (assign reviews, PR review, TODO management).
- Status: GA
- Evidence: D:/openhands/usage/cloud/github-installation.md; D:/llms.txt (SDK GitHub Workflows)
- Notes: —

### Area 13: Extensibility

### MCP
- What: SSE, streamable HTTP ("SHTTP") and stdio servers. Agent `mcp_config` uses FastMCP config format; tools registered at `initialize()`, schemas generated from `inputSchema`, MCP annotations map to tool annotations; tools with `readOnlyHint=False` get LLM risk prediction. Canvas configures MCP per backend under Customize; agent profiles may scope to `mcp_server_refs`. OAuth-only library entries install through the server's own OAuth (1.31.0). Enterprise supports org MCP servers.
- Status: GA
- Evidence: D:/overview/model-context-protocol.md; D:/sdk/arch/tool-system.md; D:/openhands/usage/agent-canvas/agent-profiles.md; GitHub releases v1.31.0
- Notes: Architecture page says SDK spawns servers over stdio; overview says SSE/SHTTP too. No sandboxing of MCP server processes documented.

### Skills (Agent Skills compatible, with triggers and paths)
- What: `SKILL.md` with frontmatter; only name/description pre-loaded, body loads on invocation. OpenHands extensions: `triggers` (keyword-injected) and `paths` (path-triggered rules injected once when a matching file is touched). Locations `<project>/.agents/skills/`, `~/.agents/skills/`, public registry `OpenHands/extensions`; legacy `.openhands/skills` and `microagents`. Precedence project over user over public.
- Status: GA
- Evidence: D:/overview/skills.md
- Notes: OpenHands microagents were renamed skills; legacy paths still read.

### Plugins (Claude-Code-shaped)
- What: A plugin bundles skills, hooks (`hooks/hooks.json`), MCP config (`.mcp.json`), agents (`agents/*.md`) and slash commands (`commands/*.md`) under a `.plugin/plugin.json` manifest. Sources: local path, `github:owner/repo`, Git URL with ref, registered marketplaces (`plugin@marketplace`), persistent install under `~/.openhands/plugins/installed/`. Described as compatible with the Claude Code plugin structure. Enterprise adds a plugin marketplace and plugin directory.
- Status: GA
- Evidence: D:/sdk/guides/plugins.md; D:/enterprise/architecture.md
- Notes: Differences from Claude plugins not stated.

### Hooks
- What: Events PreToolUse, PostToolUse, UserPromptSubmit, Stop, SessionStart, SessionEnd. Command hooks: exit 2 blocks (PreToolUse, UserPromptSubmit, Stop); other non-zero exits are non-blocking errors. Also `prompt` and `agent` hook types returning allow/deny JSON; these fail open. API endpoint lists hooks. Canvas shows hook results grouped in the chat.
- Status: GA
- Evidence: D:/sdk/guides/hooks.md; D:/openhands/usage/agent-canvas/conversations.md
- Notes: Mirrors Claude Code hook semantics; hook config file path not given on this page. (verified in review)

### Canvas Apps (custom UI pages)
- What: Beta: Apps add custom pages to the Canvas sidebar. An app is a directory with `canvas-extension.json` and one bundled ESM entry exporting `activate(host)`. Installed from Git or absolute path on the backend machine; start disabled; per-backend. Runs in the same browser context as Canvas with no sandbox; an optional iframe backend view is sandboxed with server-issued tokens. Host API lets pages make authenticated requests to the owning backend without handling the session key.
- Status: beta
- Evidence: D:/openhands/usage/agent-canvas/canvas-extensions.md
- Notes: Local backends only. Docs state "no sandbox for app code". (verified in review)

### Agent Server as a protocol surface
- What: Documented REST and WebSocket API with OpenAPI at `/docs`, a TypeScript client, and an OpenAI-compatible adapter (`/v1/chat/completions`, `/v1/responses`, `/v1/models`) where each request runs a full agent and returns final text; profiles are exposed as models `openhands_<profile>`.
- Status: GA (adapter feature, observability headers from agent-server 1.30.0)
- Evidence: D:/sdk/guides/agent-server/openai-gateway.md; D:/sdk/arch/agent-server.md
- Notes: Responses protocol unsupported options: `previous_response_id`, `store:true`, `stream:true` return 400. (verified in review)

### ACP (agents driven by Canvas)
- What: Canvas can run Claude Code, Codex, Gemini CLI, Pi, OpenCode or any stdio ACP server as the agent. The Agent Server spawns the provider CLI as a subprocess (JSON-RPC over stdio) and relays turns; the external agent runs its own loop, tools and LLM calls. Pi and OpenCode are local-backend only; Cloud offers three. Credentials: reuse host subscription login on the same machine, else secrets stored under env-var-named global secrets and resolved by the server at spawn. Also SDK `ACP Agent` guide.
- Status: GA (documented); `acp-agents` page gives no per-version note
- Evidence: D:/openhands/usage/agent-canvas/acp-agents.md; D:/llms.txt (sdk/guides/agent-acp.md)
- Notes: Docs say nothing on how approvals work for ACP agents or what OpenHands features are lost. (verified in review) (agent list, Pi/OpenCode local-only, Cloud three, subprocess over stdio, credentials resolved at spawn; the page indeed says nothing on approvals)

### Area 14: Models, providers and inference configuration

### LLM profiles, provider connections, routing
- What: Named LLM profiles (provider, model, base URL, key or a Provider Connection) per backend; `/model` switches mid-conversation preserving history; routing by model string prefix (`openai/`, `litellm_proxy/`, `openhands/`). SDK also documents LLM registry, subscriptions, model routing, fallback, streaming, image input, reasoning, profile store. Provider pages: OpenHands, Bedrock, Azure, DigitalOcean, Gemini/Vertex, Groq, local LLMs, LiteLLM proxy, Moonshot, OpenAI, OpenRouter. Backend (not the browser) must reach the base URL.
- Status: GA
- Evidence: D:/openhands/usage/agent-canvas/llm-profiles.md; D:/llms.txt (LLM Providers; SDK LLM guides)
- Notes: LLM calls go through LiteLLM (observability doc: "LLM calls via LiteLLM").

### Enterprise LLM gateway and budgets
- What: Managed LiteLLM gateway; monthly org budget, default user budget, per-user overrides; reset on the 1st or 15th at 00:00 UTC; alerts at 80/90/100% for org budget only.
- Status: GA (Enterprise)
- Evidence: D:/openhands/usage/cloud/organizations/budgets.md; D:/enterprise/architecture.md
- Notes: Canvas 1.38.0 added per-conversation budget meters.

### Area 15: UI surfaces

### Browser Canvas UI plus desktop app, mobile access
- What: React browser UI: chat panel with grouped activity stages, files, commits/changes, terminal, conversations list with tags, pins, archive, filters, resizable sidebar, voice input (browser speech recognition or an OpenAI-compatible transcription endpoint), context meter, themes (a "Paper" theme in 1.38.0). Desktop app (Electron, preview): DMG/exe/AppImage/deb. Phone/tablet access via Tailscale (private) or ngrok with `--public` and an API key.
- Status: GA (web); preview (desktop)
- Evidence: D:/openhands/usage/agent-canvas/conversations.md; D:/openhands/usage/agent-canvas/setup.md; D:/openhands/usage/agent-canvas/mobile-access.md; GitHub releases
- Notes: macOS desktop build unsigned/not notarised per docs.

### Legacy UI features (VS Code, App tab, Browser tab)
- What: The legacy GUI had a Chat panel, Changes tab, embedded VS Code, Terminal tab, App tab for a running web server, and a read-only Browser tab.
- Status: deprecated (Local GUI)
- Evidence: D:/openhands/usage/key-features.md; D:/overview/introduction.md
- Notes: The key-features page still describes the old UI; Canvas docs do not mention an App tab. Agent-server has VS Code and desktop URL endpoints (index).

### Area 16: Browser automation and computer use

### Browser tool set (browser-use)
- What: `BrowserToolSet` built on browser-use: navigate, click, scroll, fill forms, extract content; optional VNC visualisation via Docker workspace; browser session recording guide. Per-conversation Docker runtime has a "Docker browser" setting.
- Status: GA
- Evidence: D:/sdk/guides/agent-browser-use.md; D:/sdk/guides/agent-server/docker-sandbox.md; D:/openhands/usage/agent-canvas/backend-setup/docker-execution.md
- Notes: Computer use (desktop control): nothing documented.

### Area 17: Remote, cloud, headless, CI and distributed execution

### Backend model: any Canvas frontend talks to any Canvas backend
- What: A backend is an Agent Server (plus Automation Server when enabled) at a URL with an API key. Canvas stores name, host URL and key per backend ("Manage Backends"). Request path is browser to Canvas to selected backend; the same Agent Server API is used for local and remote; Canvas has no direct filesystem access. Settings, LLM config, MCP servers, secrets and automations are scoped to the active backend. Launcher modes: all-in-one, `--frontend-only`, `--backend-only`; port 8000 default; ingress serves frontend and routes API.
- Status: GA
- Evidence: D:/openhands/usage/agent-canvas/architecture.md; D:/openhands/usage/agent-canvas/backends.md; D:/openhands/usage/agent-canvas/backend-setup/remote.md; D:/openhands/usage/agent-canvas/setup.md
- Notes: Transport named elsewhere: HTTP plus WebSocket (agent-server overview); long-lived WebSockets noted in the Helm page. Architecture page does not state auth or transport details. (verified in review)

### Backend kinds
- What: (1) default local process (loopback, auto-created); (2) self-hosted remote on a VM (nginx/TLS or ngrok, tmux; Ubuntu 24.04 2 vCPU/4 GB for one user); (3) Docker (whole stack, `--privileged` optional); (4) per-conversation Docker; (5) Kubernetes Helm (single-replica StatefulSet, PVC, all-in-one image); (6) Modal (agent server in one container, 2 vCPU/4 GB, `*.modal.run` public URL, API key via `OH_SESSION_API_KEYS_0`, single-user, no Docker-in-Docker, about $0.14/hour); (7) DigitalOcean Managed Agents (isolated sandboxes that pause when idle; each session shows as its own backend; PAT auth; desktop app or `MARS_WEB=1`); (8) OpenHands Cloud (login) and Enterprise (host URL).
- Status: GA (local, VM, Docker, Cloud); other kinds documented without a stated status (Modal, DigitalOcean).
- Evidence: D:/openhands/usage/agent-canvas/backends.md; .../backend-setup/vm.md; .../docker.md; .../docker-execution.md; .../kubernetes.md; .../modal.md; .../cloud.md; .../local.md
- Notes: "Remote" means how Canvas connects, not where it runs. (verified in review) (backends.md itself lists only four 'recommended setups'; the eight kinds are assembled from the backend-setup pages; Modal figures verified in modal.md)

### Agent Server authentication and persistence settings
- What: No auth by default. API key in `X-Session-API-Key`; rotation with `OH_SESSION_API_KEYS_0`, `_1`...; legacy `SESSION_API_KEY`; 401 without it on `/api/*`; CORS via `OH_ALLOW_CORS_ORIGINS_0`. `OH_SECRET_KEY` encrypts sensitive stored values and must be stable. In Canvas, `LOCAL_BACKEND_API_KEY` is auto-generated and persisted when local; required with `--public`. Loopback bind by default; `--host 0.0.0.0`/`OH_BIND_HOST` to widen (Docker runtime mode binds all interfaces).
- Status: GA
- Evidence: D:/sdk/arch/agent-server.md; D:/openhands/usage/agent-canvas/setup.md; .../backend-setup/remote.md; .../docker-execution.md
- Notes: Remote page warns anyone with the key can request execution in that workspace. Browser embeds key in the page only with explicit Docker opt-in (`AGENT_CANVAS_ALLOW_LAN_SESSION_KEY`). (verified in review) (the `AGENT_CANVAS_ALLOW_LAN_SESSION_KEY` variable is on setup.md)

### Workspace implementations in the SDK
- What: `LocalWorkspace` (subprocess), `RemoteWorkspace` (HTTP), `DockerWorkspace` (spawns container with agent-server), `RemoteAPIWorkspace`/`APIRemoteWorkspace` (existing server or Runtime API), plus Apptainer and OpenHands Cloud workspace guides. Remote commands via `POST /command`, uploads `POST /file/upload`. Runtime API "designed primarily for benchmark evaluation at scale", not production.
- Status: GA; Runtime API sandbox described as eval-oriented
- Evidence: D:/sdk/arch/workspace.md; D:/sdk/guides/agent-server/api-sandbox.md; D:/sdk/guides/agent-server/docker-sandbox.md; D:/llms.txt (Apptainer, Cloud workspace)
- Notes: The architecture page lists five workspace types and omits cloud/Apptainer; the guide index has them.

### Deferred init (warm pool support)
- What: With `OH_DEFERRED_INIT=true` a pod boots dormant (`/health` OK, `/api/*` returns 503); `POST /api/init` with a bootstrap secret in `X-Init-API-Key` activates it with one user's env, persistence paths and session keys. Lets an orchestrator keep warm pods and keep per-user data out of the image.
- Status: GA (documented)
- Evidence: D:/sdk/guides/agent-server/deferred-init.md
- Notes: Directly relevant to a node-side pre-warm design. (verified in review) (default `deferred_init` false makes /api/init return 404; a second init call returns 400)

### Sandbox Server and V1 REST API
- What: Standalone service that creates and manages sandboxes hosting Agent Server; V1 REST API under `/api/v1` with app-conversations, sandboxes (search, start, pause, resume), and sandbox-specs. Community-supported. Can use Canvas as its browser client.
- Status: GA, community-supported
- Evidence: D:/openhands/usage/api/v1.md; D:/overview/introduction.md
- Notes: Docs show truncated pause/resume paths (`//pause`).

### CLI and headless
- What: `OpenHands-CLI` offers TUI, `openhands --headless -t "task"` (with `-f` and `--json`), `openhands acp`, `openhands web`, `openhands serve`, `--always-approve` (`--yolo`) and `--llm-approve`, resume flags, `openhands mcp` management, cloud login. README says it is no longer actively maintained and recommends Agent Canvas. Docs introduction says the CLI is unmaintained and `openhands serve` GUI is deprecated; headless is described as through Agent Server API.
- Status: deprecated (CLI: unmaintained; `openhands serve` GUI: deprecated)
- Evidence: https://github.com/OpenHands/OpenHands-CLI (README); D:/overview/introduction.md
- Notes: The main docs site has no CLI section in its index; the repository README is the only description found. (corrected in review: the CLI README says only 'no longer actively maintained' and recommends Agent Canvas; the introduction page calls the CLI unmaintained and `openhands serve` GUI deprecated. Status relabelled accordingly.)

### GitHub, GitLab, Bitbucket, Slack, Jira integrations
- What (Cloud): GitHub: add the `openhands` label to an issue or comment starting `@openhands`; comments acknowledge, then opens a PR and summarises; PR-comment mentions need source and target repos installed; short-lived 8-hour tokens with read/write on Actions, Contents, Issues, PRs, Workflows etc. GitLab: same triggers via webhook (installed automatically after owner/maintainer sign-in; group webhooks need Premium/Ultimate); `GITLAB_TOKEN` secret overrides the agent's token. Slack: `@openhands` in a channel or thread; up to 10 repos named in the message; each user links their own Cloud account; only the starter can send follow-ups. Bitbucket, Jira Cloud integrations exist (index). Enterprise adds GitHub Enterprise, GitLab self-hosted, Azure DevOps, Bitbucket Data Center, Jira Data Center.
- Status: GA
- Evidence: D:/openhands/usage/cloud/github-installation.md; .../gitlab-installation.md; .../slack-installation.md; D:/enterprise/index.md
- Notes: Integration manager pattern: HMAC-verified webhook, resolve org/user, apply filters, run in a sandbox, callback posts result (enterprise architecture page).

### GitHub Actions workflows (SDK)
- What: SDK docs include GitHub workflow guides for assigning reviews, PR review and TODO management.
- Status: unclear (index only)
- Evidence: D:/llms.txt
- Notes: Pages not opened.

### Area 18: Collaboration, teams and enterprise administration

### Organizations, roles, sharing (Cloud)
- What: Roles Member, Admin, Owner. Members start conversations on shared org credits and manage private settings; admins invite users, promote to admin, add credits, set org-wide LLM; owners promote owners, claim Git organisations for resolver routing, delete org. At least one Owner required. Share links: public or org-only.
- Status: GA
- Evidence: D:/openhands/usage/cloud/organizations/roles-permissions.md; D:/openhands/usage/agent-canvas/conversations.md
- Notes: Chats are not documented as private to the owner; org members can view automation-run conversations read-only on Cloud.

### Enterprise: SSO, RBAC, custom images, observability
- What: SAML SSO (Okta, Entra ID, Google Workspace, ADFS; assertions must be signed and carry an email attribute), Keycloak brokering OAuth to Git providers, RBAC, PostgreSQL 16.4+, Redis/Valkey, LiteLLM proxy, Runtime API with warm pools, per-conversation pods under Sysbox, S3/GCS conversation bucket, plugin marketplace, custom sandbox images with warm pools, Laminar observability. Install on Kubernetes (EKS/AKS/GKE with Helm), VM (Replicated-style admin console), or Docker Compose. Unlimited users, "no per-seat licensing", 30-day trial.
- Status: GA (commercial)
- Evidence: D:/enterprise/enterprise-vs-oss.md; D:/enterprise/architecture.md; D:/enterprise/integrations/saml-sso.md; D:/enterprise/index.md
- Notes: SAML group-to-role mapping not documented. Browsers connect directly to authenticated sandbox endpoints (architecture page). (verified in review) (architecture page parts: per-conversation pods, warm pool, PostgreSQL 16.4+, plugin marketplace, Laminar; SAML specifics not re-checked)

### Open source vs commercial matrix
- What: Open-source local/VM Canvas has scheduled and polling automations, secrets/MCP/skills, LLM profiles, Slack/GitHub/GitLab usage; but no authentication/authorisation, RBAC, multi-user orgs, SAML, custom runtime images, budgets, plugin marketplace; isolated sandboxes are "on roadmap" for VM backends; Cloud/Enterprise add the rest.
- Status: GA
- Evidence: D:/enterprise/enterprise-vs-oss.md
- Notes: Pricing: none listed on docs pages. (verified in review)

### Area 19: Observability: usage and cost, logs, tracing, debugging

### OpenTelemetry tracing and Laminar
- What: Env-only OTEL tracing: `conversation` > `conversation.run` > `agent.step` > `llm.completion`/`tool.execute`; Laminar default instrumentation layer; any OTLP backend (MLflow, Honeycomb, Jaeger, Datadog...); named spans since v1.30.0. SDK metrics tracking guide; per-conversation budget meters and token usage in Canvas; automation run cost in USD; Enterprise Conversation Metrics cookbook and analytics page.
- Status: GA
- Evidence: D:/sdk/guides/observability.md; D:/openhands/usage/agent-canvas/managing-automations.md; GitHub releases v1.38.0
- Notes: Canvas telemetry can be disabled with `AGENT_CANVAS_DISABLE_TELEMETRY` (docker page); what is collected: nothing documented in pages opened.

### Area 20: Configuration and customisation

### Settings per backend, precedence notes
- What: Canvas Settings sections: Agent (profiles), LLM, Condenser, Runtime, Verification (approval, critic), Application, Secrets, Agent Context. Skills, MCP, plugins, apps are under Customize. All per backend. New profiles copy from `default`. System prompt options: default, default plus extra instructions, custom persona (memory, security/tool guidance, skills still included).
- Status: GA
- Evidence: D:/openhands/usage/agent-canvas/customize-and-settings.md; D:/openhands/usage/agent-canvas/agent-profiles.md
- Notes: Configuration file formats for the Agent Server (beyond env vars like `OH_*`) not documented in pages opened.

### Area 21: Security and privacy

### Secrets handling
- What: Settings > Secrets: name becomes env var; description passed to the agent; values masked by default; an agent `request_secret` tool shows a card in chat and saves the secret attached to the running conversation (local backend only). The SDK secret registry detects references in bash commands, injects them as env vars and masks values in output. Agent Profiles can scope secrets: all, none, selected (enforced by server). Enterprise encrypts values in the database, supports personal and org-shared secrets, shows names only after creation, and syncs from external managers via API (AWS cookbook) rather than live vault references. Per-conversation secrets update endpoint exists.
- Status: GA
- Evidence: D:/openhands/usage/settings/secrets-settings.md; D:/sdk/guides/secrets.md; D:/enterprise/secrets-management.md; D:/openhands/usage/agent-canvas/agent-profiles.md
- Notes: Docs say secret values stay out of chat, agent events and tool arguments. Docs do not describe encryption at rest for the open-source stack except `OH_SECRET_KEY`.

### Untrusted content rules
- What: Memory is wrapped as untrusted; Canvas release 1.34.0 listed "a markdown security fix for untrusted links"; Apps are explicitly unsandboxed. Security analyzer scans for prompt-injection phrases.
- Status: GA
- Evidence: D:/sdk/guides/persistent-memory.md; GitHub releases v1.34.0; D:/sdk/guides/security.md
- Notes: Nothing found on model-generated HTML handling or remote image blocking.

### Area 22: Licence, pricing and distribution

### Distribution
- What: npm package `@openhands/agent-canvas` (Node 24+, `uv` needed), Docker image `ghcr.io/openhands/agent-canvas`, Helm chart in repo, Electron desktop installers; SDK on PyPI (not confirmed by an opened page); Cloud SaaS at app.all-hands.dev; Enterprise by contacting sales with trial.
- Status: GA
- Evidence: D:/openhands/usage/agent-canvas/setup.md; https://github.com/OpenHands/OpenHands (README); D:/enterprise/index.md
- Notes: Licence: MIT on the public repos named above. Pricing for Cloud: not on pages opened.

### Area 23: Anything else or unusual

### Design Partner program, Sandbox Server, "software factory"
- What: Organisations can join a Design Partner program for early commercial features. "Software Factory" use case and an "Agent-Driven Daily Workflow" page exist.
- Status: vendor-claim
- Evidence: D:/overview/faqs.md; D:/llms.txt
- Notes: Use-case pages not opened.

## 3. Distinctive or unusual

- Canvas is a pure browser client that can connect to any number of independent backends, each with its own settings, secrets, MCP and automations, and `--frontend-only`/`--backend-only` launcher modes (backends.md, setup.md). It is a client-side federation, not a master that owns state.
- Agent Server exposes an OpenAI-compatible endpoint where a model name is an agent profile (openai-gateway.md).
- ACP is used as an adapter: Canvas runs Claude Code, Codex, Gemini, Pi, OpenCode inside the Agent Server, with credentials resolved server-side by secret name (acp-agents.md).
- Per-conversation Docker containers with an outer server proxying to an inner agent server, per-container generated keys and idle stop (docker-execution.md).
- Deferred init: dormant warm pods activated by one `POST /api/init` with per-user config (deferred-init.md).
- Per-conversation LLM-rated risk inline in tool schema, plus pluggable analyzers and a separate confirmation policy (security.md).
- Automations synced to Git as YAML with optional encryption and webhook sources with JMESPath filters (git-sync.md, event-automations.md).
- Canvas Apps: third-party UI pages with host API proxying authenticated backend calls (canvas-extensions.md).
- Persistent memory deliberately treated as untrusted text (persistent-memory.md).
- Branch-from-message of conversations and goal loop with separate judge (conversations.md, convo-goal.md).

## 4. Architecture and UX notes

- Processes: Canvas client (React, "not an agent runtime or sandbox"), Agent Server (runs conversations, agents, tools, workspace ops; streams events), Automation Server (stores schedules/triggers/run history, dispatches conversations to Agent Server), workspace/sandbox, optional Sandbox Server, `agent-canvas` launcher with ingress (D:/openhands/usage/agent-canvas/architecture.md).
- The agent loop runs in the Agent Server, i.e. on the backend/workspace host (or inside the per-conversation container), never in the browser or UI server. Cloud and Enterprise: a control plane creates the sandbox that hosts the Agent Server; the user's browser talks directly to the sandbox's authenticated endpoints (enterprise architecture.md).
- Where state lives: conversation history, profiles, secrets and MCP config on the Agent Server; automation data on the Automation Server; backend connection info in the Canvas client (architecture.md). Storage technology not named there; elsewhere SQLite and JSON files (persistence page, Helm page).
- Layers stated: clients (Canvas, CLI, custom), interface (Agent Server REST and WebSocket), core SDK, external services (D:/sdk/arch/overview.md). The CLI uses the SDK directly; Canvas and custom clients go through the server.
- The packages: `openhands.sdk`, `openhands.tools`, `openhands.workspace`, `openhands.agent_server` (FastAPI) (same page).

## 5. Relevance to a self-hosted master/node harness

- Backend = URL + API key, with every setting scoped to the backend (backends.md). The reader's node.toml authority is stronger: OpenHands lets anyone with the key start agent execution in the workspace (remote.md warning), and per-conversation container limits supplied by the client are not capped by server env (docker-execution.md). Worth copying the model of uniform API across local and remote; do not copy the trust model.
- Per-conversation hardened container with an outer proxy and per-container keys (docker-execution.md) is a concrete design to compare with a node that spawns sandboxes.
- Deferred init for warm pools (deferred-init.md) maps to pre-warmed node sandboxes without per-user data in the image.
- Event-sourced log with one file per event and a base_state.json rewritten on each change (convo-persistence.md) is the opposite trade to the reader's SQLite append log; the whole-file rewrite is a cost the docs only imply.
- Confirmation policy is separated from risk analysis (security.md); hooks can deterministically block (exit code 2, hooks.md) but the security docs do not present them as part of the security model and list hook-based enforcement only as a future fix; their own docs say analyzers return only a risk level and are not a sandbox replacement (corrected in review).
- Automations with per-run fresh sandboxes, service-account runs, run status SKIPPED on concurrency (managing-automations.md) are a design reference for the reader's routines.
- ACP agents as a "backend agent kind" per profile (acp-agents.md), including credential flow by secret name, is a model for hosting other harnesses on nodes.
- Secrets: env-var injection, masking in output, scope per profile enforced server-side, agent `request_secret` card (secrets-settings.md, agent-profiles.md). Their Enterprise page itself notes any authorised runtime receives the value in usable form.
- The open-source versus commercial boundary (enterprise-vs-oss.md): auth, multi-user, RBAC, isolation by default, budgets all sit behind the paid tier. This is a competitive gap: an open self-hosted multi-user master with private chats is exactly what the OSS tier lacks.
- The Helm and VM pages show what users must hand-build for TLS, WebSocket timeouts and key distribution (kubernetes.md, vm.md); a master that ships that would be a visible improvement.

## 6. Overhyped, stale or contradictory

- Docs release notes end at 1.35.0 while GitHub is at 1.38.0 (index vs releases).
- Key-features page still describes the legacy GUI (VS Code, App, Browser tabs); introduction page says the GUI and CLI are deprecated/unmaintained.
- Enterprise lifecycle page contradicts itself on idle timeout leading to PAUSED vs MISSING.
- SDK security docs disagree on whether the LLM analyzer uses a separate LLM; default confirmation policy "not stated" vs ConfirmRisky as default.
- Condenser guide says no defaults documented; architecture page gives 120/4.
- Workspace architecture page lists five workspace types; guide index lists Apptainer and Cloud workspaces too.
- The enterprise-vs-oss page says "open source" without naming the licence; FAQ says use is meant for a single local user, while Canvas pages describe team-shared VMs.
- V1 API pause/resume paths appear truncated.
- Feature pages for automations describe Cloud behaviour (fresh sandbox per run) that does not apply to a local backend.
- Slide-style claims ("composable", use-case pages) are vendor-claim.

## 7. Unverified

- Whether the Enterprise server code is public and under which licence: not found on pages opened; check the OpenHands org repositories and any `enterprise/` directory licence file.
- Exact transport and auth for Canvas to Agent Server beyond `X-Session-API-Key` and "HTTP/WebSocket": architecture page does not say; agent-server API reference pages (not opened) would.
- Approval/Verification UI in Canvas (how confirm/reject is presented): Settings page only says Verification covers approval; conversations page says confirmation UI not covered.
- ACP agents' approval handling and feature loss: not documented.
- Checkpoints/undo, plan mode, computer use: nothing found.
- Cloud pricing, data retention and telemetry contents: pages not found.
- Pages not opened (index only): ask-agent, send-message-while-running, pause/resume, LLM routing/fallback/subscriptions, Apptainer/Cloud workspace guides, Jira/Bitbucket integration pages, Sysbox/K8s installs, use-case pages, pre-built automation pages, SDK getting-started and PyPI install.
- Whether the SDK is published on PyPI: README points to getting-started; not opened.

## 8. Sources

All opened 2026-11-10 (docs fetched in `.md` form):
- https://docs.openhands.dev/llms.txt - docs index (full structure)
- https://github.com/OpenHands/OpenHands - Canvas README, licence, directories
- https://github.com/OpenHands/OpenHands/releases - versions and dates 1.31.0-1.38.0
- https://github.com/OpenHands/software-agent-sdk - SDK README, packages, repo boundaries
- https://github.com/OpenHands/OpenHands-CLI - CLI README, modes, deprecation
- https://docs.openhands.dev/overview/introduction.md - product family, CLI status
- https://docs.openhands.dev/overview/faqs.md - single-user statement, safety, storage
- https://docs.openhands.dev/openhands/usage/key-features.md - legacy UI tabs
- https://docs.openhands.dev/openhands/usage/agent-canvas/overview.md, architecture.md, backends.md, setup.md, conversations.md, agent-profiles.md, llm-profiles.md, critic.md, canvas-extensions.md, mobile-access.md, customize-and-settings.md, acp-agents.md, managing-automations.md, git-sync.md, release-notes/v1.35.0.md
- https://docs.openhands.dev/openhands/usage/agent-canvas/backend-setup/local.md, remote.md, vm.md, docker.md, docker-execution.md, kubernetes.md, modal.md, cloud.md
- https://docs.openhands.dev/openhands/usage/automations/overview.md, event-automations.md
- https://docs.openhands.dev/openhands/usage/settings/secrets-settings.md
- https://docs.openhands.dev/openhands/usage/api/v1.md
- https://docs.openhands.dev/openhands/usage/cloud/openhands-cloud.md, github-installation.md, gitlab-installation.md, slack-installation.md, organizations/roles-permissions.md, organizations/budgets.md
- https://docs.openhands.dev/overview/skills.md, model-context-protocol.md
- https://docs.openhands.dev/sdk/arch/overview.md, agent.md, agent-server.md, workspace.md, security.md, tool-system.md, condenser.md, conversation.md
- https://docs.openhands.dev/sdk/guides/security.md, secrets.md, convo-persistence.md, hooks.md, task-tool-set.md, persistent-memory.md, agent-file-based.md, plugins.md, agent-browser-use.md, parallel-tool-execution.md, convo-fork.md, convo-goal.md, context-condenser.md, observability.md, agent-stuck-detector.md
- https://docs.openhands.dev/sdk/guides/agent-server/overview.md, docker-sandbox.md, api-sandbox.md, openai-gateway.md, deferred-init.md
- https://docs.openhands.dev/enterprise/index.md, enterprise-vs-oss.md, architecture.md, conversations-and-sandboxes.md, secrets-management.md, docker-in-sandbox.md, integrations/saml-sso.md
- https://docs.openhands.dev/cookbook/workspace-isolation.md, command-blacklist.md


## 9. Review

Reviewed 2026-11-10 by an independent reviewer (public docs only, each cited page re-fetched with a question about the specific claim). Checked 23 entries plus the identity block (release date) out of about 66 feature entries (89 `###` headings, of which about 23 are area headings).

| Entry | Verdict | Reason |
|---|---|---|
| Goal completion loop | confirmed | Default 10 rounds and judge fallback to not-complete verified; Canvas `/goal` syntax not re-checked |
| Stuck detector | confirmed | Thresholds 4/3/3/6 and default-on verified |
| Confirmation policies | confirmed | Architecture page: ConfirmRisky default, HIGH, confirm_unknown True; guide page leaves default unstated |
| Risk analyzers | confirmed | 30k cap, homoglyph, thinking blocks, execute_tool bypass all on security.md; inline-vs-separate-LLM disagreement is real |
| Sandbox backends and trust boundaries | confirmed | Hardening list, outer-server proxy, per-container keys, uncapped client limits, single-pod Helm, v1.34.0 date |
| No-isolation default and trust statements | confirmed-with-correction | FAQ states the limits generally; it does not say "open-source build" |
| LLM summarising condenser | confirmed | 120 / 4 / 60 verified on the architecture page |
| Opt-in persistent memory | confirmed | Off by default, ~6,000 chars, untrusted wrapper |
| Task tool set | confirmed-with-correction | Page says DelegateTool is "retired", not "replaced" |
| Hooks | confirmed | Event names, exit 2 blocks, prompt/agent hooks fail open |
| Hooks as the hard-deny layer | confirmed-with-correction | Docs do not frame hooks as part of the security model; only a proposed fix |
| Canvas Apps | confirmed | Beta, no isolation for app code, local backends only |
| Agent Server protocol surface (OpenAI-compatible) | confirmed | `/v1/chat/completions`, `/v1/responses`, `openhands_<profile>`, 400s as listed |
| ACP | confirmed | Agents, local-only Pi/OpenCode, stdio subprocess, credentials at spawn; approvals undocumented |
| Deferred init | confirmed | 503 on `/api/*`, `X-Init-API-Key`, `OH_SECRET_KEY`, warm pods |
| Backend model | confirmed | "Browser, Agent Canvas, selected backend"; "not an agent runtime or sandbox" |
| Backend kinds | confirmed | Modal figures and Helm verified; backends.md lists only four recommended setups |
| Agent Server auth and persistence | confirmed | No auth by default, header, key rotation, `OH_SECRET_KEY` |
| CLI and headless | status-wrong | CLI is "no longer actively maintained" (not formally deprecated); `openhands serve` GUI is deprecated |
| Open source vs commercial matrix | confirmed | Auth, RBAC, SAML, budgets, marketplace, custom images Enterprise/Cloud; VM isolated sandboxes "on roadmap"; no licence named |
| Event triggers and webhooks | confirmed | HMAC-SHA256, `X-Signature-256`, JMESPath, public-exposure warning |
| Enterprise SSO/RBAC/observability | confirmed | Per-conversation pods, warm pool, PostgreSQL 16.4+, marketplace, Laminar |
| Identity: version and date | confirmed | v1.38.0 is dated 2026-11-10; v1.35.0 2026-11-03; v1.34.0 2026-10-27 |

Specific points requested by the caller:
- Canvas and backends: confirmed. Docs call it a browser client and control center, not an agent runtime; any backend speaks the same Agent Server API.
- "No real authentication" in the open-source build: the FAQ says no built-in authentication, isolation or scalability and "single user on their local workstation", the Helm page says "unauthenticated, single-tenant", and enterprise-vs-oss lists authentication/authorisation as Cloud and Enterprise only. The survey's nuance (only an API key gates the agent server) is correct.
- "Three-part security model": the survey does not use this phrase. The docs describe two mechanisms (risk analyzer plus confirmation policy), with three defence-in-depth analyzers (pattern, policy-rail, ensemble); hooks are only a proposed fix. If the reader's brief says "three parts", it is not the docs' framing.
- Deprecation: the CLI is unmaintained; the `openhands serve` Local GUI is deprecated (introduction.md).

Borrowed features: none found. The Claude-Code-shaped plugin/hook/agent-file entries are described by the survey as compatible in shape, and the docs say the same for plugins.

Notable omissions (not in the survey, all listed in https://docs.openhands.dev/llms.txt):
- Cloud API and sandbox endpoints: https://docs.openhands.dev/openhands/usage/cloud/cloud-api.md
- Plugin Launcher (Cloud): https://docs.openhands.dev/openhands/usage/cloud/plugin-launcher.md
- Plugins in Agent Canvas: https://docs.openhands.dev/openhands/usage/agent-canvas/plugins.md
- Event sources for custom webhooks: https://docs.openhands.dev/openhands/usage/agent-canvas/automation-events.md
- Enterprise custom sandbox images: https://docs.openhands.dev/enterprise/custom-sandbox-images/index.md
- Enterprise external LLM gateways and observability platforms: https://docs.openhands.dev/enterprise/integrations/external-llm-gateways.md
- Enterprise skills and plugins, plugin marketplace: https://docs.openhands.dev/enterprise/skills-and-plugins.md
- Troubleshooting: https://docs.openhands.dev/openhands/usage/agent-canvas/troubleshooting.md
- DigitalOcean Managed Agents backend page (survey covers it only through backends.md): https://docs.openhands.dev/openhands/usage/agent-canvas/backend-setup/digitalocean.md
- Model configuration page: https://docs.openhands.dev/openhands/usage/agent-canvas/model-configuration.md

Overall judgement: reliable. All 23 checked entries matched their pages apart from three wording or status fixes, all corrected above, and the survey is careful to flag its own gaps and internal doc contradictions. The unchecked remainder (index-only entries are already labelled as such; Cloud integrations, release-note and UI details) can be trusted for existence but not mechanism where the survey says "pages not opened".
