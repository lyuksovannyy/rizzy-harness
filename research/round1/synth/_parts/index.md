### Agent loop, tools, editing, shell (areas 1-4)

Evidence tables, one row per harness with its own source: [round1/synth/01-loop-tools-editing-shell.md](round1/synth/01-loop-tools-editing-shell.md)


**Area 1. Agent loop and execution model**

- **Loop structure, turns and stop reasons**: how the harness defines one unit of work (turn, step, item) and how a run reports why it ended.
- **Where the loop runs relative to the tools**: which process owns the loop, which owns tool execution, and what the clients are.
- **Step, turn and budget limits**: hard caps on model calls, turns, spend or wall-clock time inside one run.
- **Stuck-loop and stalled-run detection**: automatic detection of a run that repeats itself or has stopped making progress.
- **Interrupting a running turn**: stopping the current model call and tools without losing the session.
- **Steering and message queueing while a turn runs**: accepting user input during a running turn, either injected into the turn (steer) or held for later (queue).
- **Parallel tool calls**: running several tool calls from one model response at the same time.
- **Goal and objective-continuation loops**: a persisted objective that keeps the agent working across turns until a completion check passes.
- **Time-driven re-submission loops**: re-running a prompt in the same session on a timer or after each turn, independent of an objective check.
- **Retry, backoff and stream-failure handling**: what the loop does when the provider fails or a stream stalls.
- **Multi-model turn strategies (handoff, mixture, forcing)**: using more than one model inside a single run or turn.
- **Event log as the loop's source of truth, one writer per session**: the session is an append-only log the model context and UI are derived from, with a defined writer.
- **Harness-injected context and message provenance**: content the harness adds to the conversation, and whether the model or SDK can tell it from user text.
- **Evaluator outside the main loop (asynchronous monitor, critic)**: a second process judging the run's safety or success without being the approver of each action.
- **Tool-call execution pipeline**: the ordered stages a tool call passes between the model's request and the result.
- **Deliberate turn-ending semantics (forced answer, silent finish, finish tools)**: ways a turn ends by design rather than by the model simply replying.

**Area 2. Built-in tools**

- **Built-in tool inventory (comparison)**: which tools ship with the agent, by category. Only Claude Code, OpenCode, Oh My Pi, Pi, Zed, OpenClaw, Cline and Hermes publish a tool list; Codex, Antigravity, OpenHands, goose, DeepSeek (generated catalogue, not default-mounted) and Warp give partial or no list.
- **Tool-set composition: defaults, presets and per-model gating**: how the set of tools actually offered to the model is chosen.
- **Deferred tool loading (tool search)**: keeping most tool schemas out of the prompt and letting the model look them up.
- **Model-written scripts that call tools (code mode, programmatic tool calling)**: the model writes a short program that calls other tools, so many calls cost one turn and only the script output re-enters the context.
- **Ask-the-user tool**: a tool call that pauses the loop to ask the user a structured question.
- **Todo and task-list tool**: a model-maintained task list shown to the user.
- **Web fetch and web search tools (with per-session caps)**: tools that read a URL or query the web.
- **Code-navigation tools (LSP, structure, AST, outline)**: tools that give the model symbols, definitions or structure instead of raw text search.
- **Tool result size limits and spill-to-file**: capping what a tool result adds to the context and keeping the rest retrievable.
- **Tool risk classification metadata**: tool-level metadata saying whether a call reads, writes or executes, used for approvals or concurrency.
- **Internal URL schemes inside file tools**: virtual paths (`pr://`, `conflict://`, `agent://`, `xd://`) that file-shaped tools read and write instead of dedicated tools.

**Area 3. Code editing and file operations**

- **Edit format: exact search/replace of a string**: the model sends the old text and the new text; the tool replaces an exact match.
- **Edit format: patch (apply_patch)**: the model emits a multi-file patch (add, update, delete) that the tool applies.
- **Edit format: hash-anchored lines (hashline)**: every line shown to the model carries a short content hash; edits cite the hash instead of reproducing old text; a changed file makes hashes mismatch and the edit is rejected.
- **Edit format: structural (AST) edits**: syntax-aware codemods instead of text replacement.
- **Whole-file write and create**: creating a file or replacing it entirely.
- **Model-routed choice of edit tool**: the harness picks which edit tool a session offers based on the model family.
- **Failed-edit handling, staleness and read-before-edit guards**: what the tool does when the old text is missing or ambiguous, when the file changed since the model read it, or when parallel edits race.
- **Encoding and line-ending safety**: avoiding silent corruption when a tool rewrites a file that is not plain UTF-8 or uses CRLF.
- **Post-edit diagnostics and formatters**: feeding type errors or formatter results back after a write.
- **Diff review and in-app file viewing or editing**: showing what the agent changed and letting the user accept, reject or edit.
- **Write confinement and protected paths for file tools**: limits on where the file tools may write, enforced in the tool rather than by an OS sandbox.
- **Read-tool capabilities and limits**: what the read tool returns (text, images, PDFs, notebooks, remote files) and how it bounds it.
- **File mentions and attachments**: attaching files, folders, URLs or images to a prompt from the UI.
- **Notebook support**: cell-level notebook reading or editing.
- **Git conflict resolution through virtual paths**: resolving merge conflicts by writing to session-local URLs rather than editing marker text.
- **Debug mode with log instrumentation**: a guided loop that inserts logging, has the user reproduce, reads logs, fixes, and removes the instrumentation.
- **Inline selection edits and edit prediction (editor-native)**: prompt-driven in-place transforms of a selection, and tab-style completions.
- **Per-session scratch directory**: a temp directory the agent may use without permission prompts.
- **Office-document deliverables**: producing spreadsheets, decks and documents rather than code files.

**Area 4. Terminal and shell integration**

- **Shell execution model (process per command or persistent shell)**: how one shell tool call maps to processes, and what state (cwd, env, aliases) survives between calls.
- **Command timeouts and auto-backgrounding**: how long a command may run and what happens at the limit.
- **Background processes**: starting a long-running command without blocking the loop, then reading, polling or killing it.
- **Agent-driven interactive terminal (PTY)**: the agent opens a real terminal, types into it and reads the screen, so it can drive REPLs, editors and debuggers.
- **User-facing terminal pane in the app**: an interactive terminal the user can use alongside the agent.
- **User shell escape (`!` commands)**: the user runs a command directly from the prompt without a model turn, optionally adding the output to context.
- **Shell output limits and spill**: the cap on command output the model sees and where the rest goes.
- **Command-output filtering and minimisation**: rewriting command output to cut noise before it reaches the model.
- **Environment given to spawned commands**: which environment variables a tool-spawned process inherits, and which markers it receives.
- **Resource limits for commands**: capping memory or CPU of tool-spawned processes.
- **Pluggable execution backends for shell and file tools**: choosing where commands and file operations run (local, container, SSH, remote worker) without changing the agent.
- **PowerShell and Windows shell support**: running the shell tool natively on Windows rather than through WSL.
- **Shell-rc and terminal-emulator integration**: hooking the agent into the user's own terminal or telling the terminal what the agent is doing.
- **Monitor: background command output as conversation events**: a tool that watches a command or socket and turns each output line into an event the model sees mid-conversation.
- **Hosting other CLI agents inside a terminal**: running a third-party agent CLI as a first-class thread or session inside the host app.

### Permissions, sandboxing, security (areas 5, 21)

Evidence tables, one row per harness with its own source: [round1/synth/02-permissions-sandbox-security.md](round1/synth/02-permissions-sandbox-security.md)


**Area 5. Permissions, approvals and sandboxing**

- **F1. Approval modes and the default mode**: a named, session-wide setting that decides which tool calls run without asking, which ask a human, and which are refused. The default mode is what a new user gets without touching anything.
- **F2. Per-tool and per-command permission rules**: a rule language that allows, asks or denies specific tools, commands, paths, domains or MCP tools, with a defined precedence.
- **F3. Shell-command parsing: chains, wrappers and hidden-execution constructs**: how a command rule copes with `a && b`, pipes, subshells, `$(...)`, wrappers like `timeout`, and flags that run other programs (`git -c core.pager=...`).
- **F4. What an approval is bound to**: when a user clicks "allow", what exactly is remembered: the tool, a command prefix, the exact argv, the working directory, the executable's identity, and for how long (once, turn, session, forever).
- **F5. Model-based approvers and risk classifiers**: a second model (or the main model) decides whether an action runs, instead of or before a human.
- **F6. Hardcoded floors no mode can lift**: a fixed set of commands or actions that stay prompted or refused even in the most permissive mode.
- **F7. Protected paths inside writable areas**: paths that stay write-protected even where the agent may otherwise write, because writing them turns into code execution or policy change (git hooks, agent config, shell rc files).
- **F8. Workspace boundary for file tools**: the folder set the agent's built-in file tools may touch without asking, and what happens outside it.
- **F9. Administrator and managed policy layers**: a policy source that outranks the user's own settings, set by an organisation or machine administrator.
- **F10. Repository config cannot widen policy**: settings committed in a cloned repository (or created by the agent inside it) must not grant themselves more access.
- **F11. OS sandbox for agent shell commands**: kernel-enforced confinement (filesystem, network, sometimes process) of the commands the agent runs.
- **F12. Behaviour when the sandbox is unavailable**: what happens on a machine where the sandbox cannot start (old kernel, user namespaces blocked, unsupported OS).
- **F13. Sandbox network egress controls**: limiting where sandboxed commands may connect, usually through a host-side proxy and a domain allowlist.
- **F14. Model-requested widening and sandbox escape hatches**: a sanctioned way for the agent to ask to run one command with more access than the sandbox gives.
- **F15. Container, VM and remote-executor isolation**: running the agent's tools (or the whole agent) in a container, microVM or separate machine so the host is the boundary.
- **F16. Sub-agent permission inheritance**: whether a delegated agent can be more permissive than its parent.
- **F17. Unattended and headless approval behaviour**: what a run with nobody to click does when an action needs approval.
- **F18. Approvals delivered to another device or channel**: answering an approval prompt from a phone, web dashboard or chat app while execution stays on the host.
- **F19. Hooks and extensions in the permission path**: user or admin code that runs before a tool call and can allow, deny, ask or rewrite.
- **F20. Permission and approval decisions as logged events**: every policy change, approval request and decision recorded in an append-only log or audit stream.

**Area 21. Security and privacy**

- **F21. Child-process environment scrubbing**: removing secrets from the environment that the agent's commands inherit.
- **F22. Credential placeholders and injection at an egress proxy**: the agent's processes never hold the real secret; a proxy outside the agent's boundary adds it to outbound requests for allowed hosts.
- **F23. Secret redaction in output, logs and telemetry**: masking known secret values or secret-shaped strings before they reach the model, the transcript, logs or the vendor.
- **F24. Storage of the harness's own credentials**: where provider keys and login tokens live on disk.
- **F25. Prompt-injection defences for untrusted content**: measures that stop text from web pages, tool output, files or other users from steering the agent into harmful actions.
- **F26. Sub-agent and inter-agent output hygiene**: treating a sub-agent's or peer agent's report as data, not as instructions with user authority.
- **F27. SSRF and private-address guards on fetches**: refusing fetches to loopback, private ranges and cloud metadata endpoints, including after redirects and DNS rebinding.
- **F28. Project and workspace trust prompts**: before loading configuration, hooks, MCP servers or skills from a folder, the user confirms they trust it.
- **F29. Trust for extensions, plugins, hooks and marketplaces**: how the harness decides whether to run third-party code it did not write (plugins, MCP servers, hooks, skills).
- **F30. Authentication of the harness's own server and web UI**: how the harness's local or self-hosted HTTP/WebSocket server decides who may use it.
- **F31. Authentication of remote links (nodes, workers, remote control)**: how a worker, executor or remote-control client proves its identity to the central server, and the reverse.
- **F32. Telemetry defaults and opt-outs**: what the harness sends to its vendor by default, and how to stop it.
- **F33. Data retention and local data at rest**: what session data the harness keeps, where, for how long, encrypted or not, and how to delete it.
- **F34. Documented trust model and stated non-boundaries**: the vendor's own written statement of what its controls do not protect against.

### Context management and memory (areas 6, 7)

Evidence tables, one row per harness with its own source: [round1/synth/03-context-memory.md](round1/synth/03-context-memory.md)


**Area 6: context management and compaction**

- **Automatic compaction (trigger, who compacts, what is kept)**: when the conversation nears the model window, older history is replaced by a model-written summary and the run continues.
- **Manual compaction and partial compaction**: a user command that compacts now, sometimes with a focus instruction or a partial range.
- **Staged, fallback and overflow-recovery compaction**: more than one compaction method, or a defined recovery when summarising fails or the provider rejects the request as too long.
- **Idle, background and incremental compaction**: compacting before the user needs it, to save cache-miss cost or latency.
- **Pruning or masking of old tool output**: shrinking or removing old tool results from the prompt without a summary call.
- **Large-output handling (caps, spill to file, retrieval)**: bounding what one tool result can put in the prompt and keeping the full text retrievable.
- **Prompt caching (layout, TTL, warming, diagnostics)**: keeping the request prefix byte-stable so the provider's prefix cache hits, and managing its lifetime.
- **Context usage display**: showing the user how full the window is, or what fills it.
- **Repository maps and code indexing**: giving the agent a ranked summary or searchable index of the repository so it need not discover files by search alone.
- **Context cost of tool and skill catalogs (lazy schemas, budgets)**: limiting what tool definitions, skill lists and rule files cost before the first prompt.
- **Hooks, plugins and templates that change compaction**: extension points around compaction (replace the summary, change the prompt, observe it, swap the whole context engine).
- **Side questions, recaps and fresh-context hand-offs**: ways to get an answer or a clean context without compacting the main thread.

**Area 7: memory and persistent instructions**

- **Instruction files: which file names are read**: plain-text project or user files loaded into the prompt every session.
- **Instruction scopes, precedence, imports and conditional loading**: how several instruction files combine, and when they load.
- **Instruction size limits and trust of loaded files**: caps on how much instruction text is injected and whether a repo's own files are trusted.
- **Generating and auditing instruction files (`/init`, prompt audit)**: commands that write or lint the instruction file.
- **Agent-written memory: storage format and what is injected**: notes the agent (or a background job) writes for later sessions.
- **Who approves an agent memory write**: whether the human or a policy gate sits between the agent proposing a memory and it entering the prompt.
- **Memory scopes (user, workspace, project, repository, team, agent)**: how far a memory reaches and who can see it.
- **Recall, memory back ends and embeddings**: how stored memory is found again.
- **Background consolidation and self-review of memory**: a non-interactive pass that extracts, merges or prunes memory outside the user turn.
- **Agent-written skills as memory (procedural memory)**: the agent turns lessons or procedures into reusable skill files instead of facts.
- **Memory content safety (untrusted text, secrets, injection)**: treating stored memory as potentially attacker-influenced input and keeping secrets out.
- **Memory and instruction opt-outs**: switches that turn memory or instruction loading off, per user, chat, workspace or organisation.
- **Memory import, export and portability**: moving memory or instruction setups between tools or accounts.
- **Searching past sessions as memory**: recall by searching stored conversation history instead of curated notes.

### Planning, sub-agents, background work (areas 8-10)

Evidence tables, one row per harness with its own source: [round1/synth/04-planning-subagents-background.md](round1/synth/04-planning-subagents-background.md)


**Area 8. Planning, tasks and modes**

- **Plan mode (restricted planning phase)**: a session state in which the agent investigates and writes a plan but may not change the project until a human approves the plan.
- **Plan review and hand-off**: how the user inspects, edits, comments on and accepts a plan, and what context the implementation then starts with.
- **Model-maintained task / to-do list**: a tool the model writes to keep a visible checklist of steps and their status.
- **Goal / run-until-done loop**: a standing objective that keeps the agent taking turns until a check says the objective is met, a budget runs out, or a human stops it.
- **Read-only, ask and custom modes**: named modes other than plan that change what the agent may do or how it behaves (ask-only, chat-only, debug, profiles, presets).
- **Interview-first planning commands**: a command that makes the agent question the user (and the code) before planning.
- **Side questions (`/btw`, `/side`)**: ask something without polluting the main conversation.
- **Orchestration modes and trigger keywords**: a mode or a typed keyword that makes the main agent decompose the task and delegate, instead of the model deciding case by case.

**Area 9. Sub-agents and multi-agent orchestration**

- **Sub-agent delegation tool**: a model-callable tool that starts a child agent on a task and gives the parent its result.
- **Built-in agent roster**: the named agents a harness ships, what each is for, and where it may run.
- **Custom agent definition files**: user- or project-authored agent definitions (prompt, tools, model, limits).
- **Sub-agent context: fresh, forked or inherited**: what the child sees when it starts.
- **Sub-agent tools and permissions**: which tools a child gets and whose approval rules apply to it.
- **Sub-agent model and effort resolution**: how the child's model and effort are chosen.
- **Result return and output hygiene**: what comes back to the parent and how it is checked.
- **Background sub-agents and resumption**: children that run while the parent continues, and children that can be continued later.
- **Depth, concurrency and spend limits**: caps on nesting depth, parallel children and cost.
- **Isolation for sub-agents (worktrees, overlays, VMs)**: giving a writing child its own copy of the code so parallel edits do not collide.
- **Agent teams, swarms and shared boards**: several long-lived agents with names that coordinate through a shared task list, mailbox or board, rather than one parent with throwaway children.
- **Cross-session messaging and message buses**: agents or sessions sending messages to each other outside a parent/child call.
- **Script-orchestrated workflows**: the model writes a program (or a declarative recipe) that fans work out to many agents, so intermediate results stay out of the model's context.
- **Running other vendors' harnesses as sub-agents or runtimes**: driving Claude Code, Codex, Gemini CLI, OpenCode and others as children or as the agent runtime.
- **Coordinator agents that delegate and do not code**: a long-running top-level agent that routes work into separate worker sessions or threads and watches them.
- **Built-in parallel fan-out commands**: a shipped command that runs one task as many parallel agents (best-of-n, review fleets, split-change).
- **Advisor and watchdog models**: a second model that watches or is consulted during the main agent's work and injects guidance.
- **Agent-to-agent peering protocols**: calling an independently deployed agent as a peer over a protocol, not as a child.

**Model roles and slots for auxiliary work (cross-cutting; mostly Area 14)**

- **Auxiliary model slots**: named model assignments for work other than the main turn (titles, summaries and compaction, vision, goal judging, commit messages, planning, sub-agents, approval classifiers).
- **Fallback chains for the main model**: an ordered list of models (or accounts) to try when the primary fails.
- **Phase-based model switching**: changing model by phase of the task (plan with a strong model, execute with a cheaper one).

**Area 10. Background, long-running and scheduled work**

- **Background shell jobs**: long-running commands (dev servers, watchers) that run while the agent continues, with output the agent can poll.
- **Background agent sessions and supervisor daemons**: an agent session that runs without an attached UI, hosted by a long-lived local process so sessions outlive the client.
- **Agent dashboards and command centres**: one view of many running agents with status (working, needs input, done) and quick reply.
- **Auto-backgrounding of long tool calls and server-side tasks**: a long tool call stops blocking the turn and reports back later.
- **In-session schedules, loops and reminders**: timers that wake the **same** session later or repeatedly.
- **Standalone scheduled runs (cron automations)**: saved prompts that start a **fresh** agent run on a schedule, without a user present.
- **Event triggers and inbound webhooks**: external events (HTTP calls, GitHub, chat, tickets) that start an agent run.
- **Permission posture of unattended runs**: what an unattended scheduled or triggered run may do without a human.
- **Cloud background agents**: vendor-hosted runs that continue with the user's computer off (summary only; remote execution is covered by another group).
- **Durable and crash-surviving runs**: what survives a process crash or restart in the middle of a run.
- **Auto-continue after a usage limit**
- **Keep the machine awake**
- **Completion notifications**

### Sessions and git (areas 11, 12)

Evidence tables, one row per harness with its own source: [round1/synth/05-sessions-git.md](round1/synth/05-sessions-git.md)


**Area 11: Sessions**

- **Session storage: format, location and shape**: where a conversation lives on disk (or server) and whether it is a structured event log, a transcript, a database, or a set of files.
- **Resume and continue**: reopening an earlier session and continuing it with its history, and what state comes back with it.
- **Fork: copy a session into a new session**: creating a second, independent session from a prefix (or all) of an existing one.
- **In-file session trees (branching without copying)**: the session log is a tree; rewinding and continuing creates a sibling branch in the same file instead of a new session.
- **Conversation rewind and edit-and-resubmit**: cutting the conversation back to an earlier message and continuing, possibly with an edited prompt, without necessarily touching files.
- **Export of a session**: writing a session out as a file for reading, archiving or analysis.
- **Session sharing by link**: giving someone else a view of a session via a URL.
- **Search across sessions and the session picker**: finding an earlier session by text, title, branch or PR.
- **Moving a session between machines or surfaces (handoff)**: continuing the same task on another host or surface (local, cloud, SSH host, messaging channel), with or without the git state.
- **Importing sessions from other harnesses**: reading another harness's saved sessions.
- **Archive, delete, retention and ephemeral sessions**: lifecycle controls for stored sessions.
- **Single writer, concurrency guard and crash recovery**: preventing two processes from appending to one session and repairing a session after a crash.
- **Pluggable external session storage**: letting the host replace local transcript files with its own store.
- **File checkpoints and undo: the mechanism**: automatic capture of file state around agent edits so a turn can be undone or a session rewound, separate from the user's own VCS.
- **Checkpoint coverage, retention, costs and removals**: what a checkpoint system does not restore, how long it keeps snapshots, and what it costs.
- **SDK file checkpointing and rewind**: a programmatic restore point per user message, usable from a host application.

**Area 12: Git and version control**

- **Worktrees as the isolation unit for parallel agents**: running a session or sub-agent in its own git worktree so parallel agents do not edit the same files.
- **Worktree setup and copying ignored files**: making a fresh worktree usable (env files, dependencies) without checking secrets in.
- **Worktree lifecycle: cleanup, snapshots before removal, caps**: what happens to worktrees when the session ends, is archived or goes stale; whether dirty work can be lost.
- **Git repository requirement and non-git folders**: whether the harness works (and what it loses) outside a git repository.
- **Commits and commit-message generation**: the harness creating commits, or generating a message for the user's commit.
- **Pull-request creation and PR checkout**: opening a PR from the harness, or checking one out as a session.
- **PR status tracking, auto-fix and auto-merge**: following a PR after it is opened: CI status, review comments, fixing and merging.
- **Local code review command**: a read-only reviewer run on the working tree, a branch diff or a commit from inside the harness.
- **Hosted PR review bots and CI actions**: an automated reviewer running on a hosting-platform event (PR opened, push, comment) and posting to the PR.
- **Cloud multi-agent deep review (Ultrareview)**: a fleet of reviewer agents in a cloud sandbox, each finding independently reproduced.
- **Diff views**: viewing what the agent changed, per file, per turn or per branch, with the ability to comment, revert or apply.
- **Attribution, signing and provenance in commits and PRs**: marking agent-made commits and PRs, and proving who made them.
- **Git host integrations and credential handling**: how the harness talks to GitHub/GitLab/others, and where git credentials live when work runs remotely.

### Extensibility (area 13)

Evidence tables, one row per harness with its own source: [round1/synth/06-extensibility.md](round1/synth/06-extensibility.md)


**2.1 MCP as a client**

- **MCP client: transports and configuration scopes**: the harness connects to Model Context Protocol servers, local (stdio child process) or remote (HTTP), and offers their tools to the model. Configuration lives in one or more files whose scope (user, project, managed) decides who can add a server.
- **MCP remote authentication (OAuth and headers)**: signing the harness in to a remote MCP server, usually OAuth 2.0 with discovery, plus static or helper-generated headers.
- **Tool deferral and tool search**: instead of sending every tool schema each turn, the harness sends names (or nothing) and lets the model search for and load schemas on demand. The main answer to MCP context bloat.
- **MCP protocol features beyond tools**: the rest of MCP: prompts, resources, elicitation (server asks the user for input), roots, sampling (server asks the harness's model), `list_changed`, MCP Apps (`ui://` interactive UI), and the tasks extension for long jobs.
- **MCP limits, timeouts and connection lifecycle**: wall-clock and idle timeouts per call, startup waits, output caps, reconnect and retry rules.
- **MCP approval, trust and admin allow/deny lists**: how MCP tool calls are approved, how a project-supplied server is trusted before it starts, and how admins restrict servers.
- **Connector directories and MCP registries**: a browsable catalog of MCP servers or connectors that users can enable from inside the product.
- **MCP servers that push events into a session**: an MCP server that can inject messages (CI results, chat messages, alerts) into a running session rather than only answering calls.

**2.2 The harness as an MCP server**

- **Exposing the harness over MCP**: the harness runs as an MCP server so another client can use its tools or conversations.

**2.3 Skills**

- **Skill format and discovery**: a folder with `SKILL.md` (YAML frontmatter plus Markdown instructions) and optional scripts and references, discovered from user, project and bundled locations. The Agent Skills format (agentskills.io) is the shared base; `.agents/skills/` is the shared project path.
- **Skill loading, listing budget and invocation control**: how much of each skill sits in context (usually name plus description), how the body is loaded, and who may invoke it (user, model, both).
- **Skills that run shell commands while loading**: placeholders inside a skill or command that run a shell command before the prompt reaches the model, splicing the output in.
- **Agent-written skills and extensions**: the agent creates or edits its own skills, rules or extensions, turning experience into reusable procedures.
- **Skill registries and account sync**: a remote source of skills, either a public registry or skills synced from a user's account.

**2.4 Plugins and extensions (packaging and distribution)**

- **Plugin bundles: what a plugin can contain**: one installable unit that packages skills, commands, agents, hooks, MCP servers and sometimes code, themes or UI.
- **Plugin distribution and marketplaces**: where plugins come from (registry, git, npm, local folder), how a catalog is described and how updates arrive.
- **Plugin trust, isolation and supply-chain controls**: what stops an installed plugin, skill or extension from doing harm, and what the docs admit.
- **Managed (admin) controls over extensions**: settings that only an administrator source can set, restricting which hooks, plugins, skills and MCP servers may load.

**2.5 In-process middleware**

- **In-process extension API (code hooks into the loop)**: extension code loaded into the harness process that can observe, rewrite or answer engine events (prompts, tool calls, model requests, compaction) and register tools, commands and UI.

**2.6 Hooks**

- **Hook events and what a hook can decide**: user-configured handlers fired at lifecycle events (session start, prompt submit, before and after a tool, stop, compaction) that can block, rewrite, add context or force continuation.
- **Hook handler types**: what a hook actually runs: a command, an HTTP call, an MCP tool, a single LLM prompt, or a sub-agent.
- **Hook trust, precedence and failure behaviour**: whether a hook from a repo or plugin runs without review, how hooks from several sources combine, and what happens when a hook crashes or times out.

**2.7 Custom commands and prompt templates**

- **Custom slash commands and prompt templates**: user-defined `/name` commands that expand into a prompt, usually Markdown files with argument placeholders.

**2.8 Custom tools**

- **Custom tools defined in code**: tools defined by users or integrators as code (not via a separate MCP process) and offered to the model next to built-ins.

**2.9 Code mode**

- **Code mode (the model writes code that calls tools)**: instead of one tool call per turn, the model writes a script that calls tools, and only the script's output returns to context. Cuts schema cost and round trips.

**2.10 SDKs and embedding**

- **Embeddable SDK (agent as a library)**: a library that runs the harness's agent loop inside another program.
- **Programmatic control protocol (headless JSON streams, RPC, app-servers)**: a wire protocol a separate process uses to drive the agent: start turns, stream events, answer approvals.
- **OpenAI-compatible endpoint where the "model" is an agent**: an HTTP endpoint shaped like OpenAI's API that runs a full agent per request, so any OpenAI client can call the harness.
- **Structured output for programmatic callers**: the caller supplies a JSON Schema and the harness guarantees (or retries towards) a final answer that validates.

**2.11 Client protocols**

- **ACP: the harness as an ACP agent**: the Agent Client Protocol (JSON-RPC 2.0, modelled on LSP) lets an editor launch an agent as a subprocess over stdio. The agent runs the loop; the client does file and terminal I/O and shows permission prompts. v1 is stable; v2 is a draft (2026-07-20); stdio is the only standardised transport; remote Streamable HTTP/WebSocket is an RFD.
- **ACP: the harness as an ACP client hosting other agents**: the harness launches another vendor's agent over ACP and renders its thread, so one UI can drive Claude, Codex, Gemini and others.
- **Hosting other harnesses through their own interfaces (not ACP)**: running another vendor's agent as a backend through its own app-server, CLI or SDK, or wrapping it in a terminal.
- **Agent-to-Agent (A2A) protocol**: Linux Foundation protocol for agent-to-agent delegation (agent cards, JSON-RPC, push notifications); distinct from ACP.

**2.12 Reading other harnesses' configuration**

- **Reading other harnesses' files in place**: the harness reads instruction files, skills, MCP configs, hooks or plugin layouts that belong to another harness, without converting them.
- **One-shot import from another harness**: a command that copies another harness's setup (and sometimes chats) into this one once.

### Models and observability (areas 14, 19)

Evidence tables, one row per harness with its own source: [round1/synth/07-models-observability.md](round1/synth/07-models-observability.md)


**Area 14: models, providers and inference configuration**

- **Provider support and bring-your-own-key**: which model providers a harness can talk to with a key the user supplies, and how that key is stored and resolved.
- **Cloud-platform providers and enterprise bring-your-own-cloud**: using models served from the customer's own AWS, Google Cloud or Azure account instead of the vendor's API.
- **Custom endpoints, wire formats and gateways as a provider**: pointing the harness at any HTTP endpoint that speaks a known wire protocol (Chat Completions, Responses, Messages), and how it copes with quirks.
- **Local models**: running or connecting to a model on the user's own hardware.
- **Using another harness or a subscription as a provider**: three different things share this name: (a) signing in with a consumer subscription (ChatGPT, Claude, Copilot, SuperGrok) so the harness bills against plan limits; (b) wrapping another harness or CLI as the model backend; (c) exposing your own subscription to other tools as a local proxy.
- **Model selection per session, per agent and per task**: choosing which model answers, at which scope (whole install, workspace, session, agent, sub-agent, single turn), and what happens on resume.
- **Reasoning effort and thinking controls**: a user-facing control for how much a model reasons before answering.
- **Speed tiers (fast mode)**: a paid, faster serving configuration of the same model.
- **Model fallback chains**: automatically retrying a request on a different model when the chosen one fails or is unavailable.
- **Provider retry, backoff and timeouts**: re-sending the same request to the same provider after a transient error, and how long to wait or give up.
- **Routing across providers and credential pools**: spreading requests over several providers, accounts or keys by rules, quota or cost.
- **Model roles for auxiliary work**: using a different, usually cheaper or stronger, model for side tasks (titles, compaction, summaries, classification, approval review, advice).
- **Vision and other modalities**: image input, image generation, and handling for models that lack vision.
- **Voice (dictation, text-to-speech, realtime)**: speech in or out of the agent. Three distinct mechanisms: dictation into the prompt, speech output, and realtime voice conversation.
- **Hosted model offerings (commercial services)**: model access sold by the harness vendor itself. **Everything in this section is a commercial service, not a harness feature.** Prices are as stated on vendor pages on 2026-11-10 and change often.
- **Model catalogue, capability discovery and pinning**: how the harness learns what each model can do (context, vision, tools, effort, price) and how users freeze or relabel models.
- **Model and provider governance (allowlists, locks)**: admin controls over which models, providers or effort levels users may use.
- **Prompt-cache controls and cache-aware behaviour**: settings and design choices that keep provider prompt caches warm (a cost lever).

**Area 19: observability (usage and cost, logs, tracing, debugging)**

- **Token and cost display**: showing the user how many tokens and how much money a turn, session or run used.
- **Budgets and limits**: caps that stop or warn on spend, tokens or time, set by the user or an admin, as distinct from plan quotas set by a vendor.
- **Usage dashboards and quota views**: an aggregated view across sessions, users or accounts, in the product or via API.
- **OpenTelemetry and other export**: sending traces, metrics or events to an external collector or analytics service.
- **Logs, transcripts and raw request logging**: durable records a developer can read after the fact: application logs, per-session transcripts, and raw provider requests.
- **Debugging and diagnostics tools**: commands that check installation, configuration and connectivity, or profile a running process.
- **Vendor telemetry and data-upload defaults**: what the harness itself sends to its vendor by default, and how to turn it off. Distinct from the user-configured export above. Included because a self-hosted harness's stance here is a visible product choice.

### UI, browser, collaboration (areas 15, 16, 18)

Evidence tables, one row per harness with its own source: [round1/synth/08-ui-browser-collaboration.md](round1/synth/08-ui-browser-collaboration.md)


**Area 15. UI surfaces**

- **Terminal UI**: an interactive full-screen or scrollback terminal front end to the agent loop, with prompt editing, status line and transcript tools.
- **IDE integrations**: a panel or plugin inside an editor that hosts the agent and shares selection, open files and diagnostics.
- **Desktop apps**: a native or Electron shell around the agent, usually adding panes (diff, terminal, browser, files), notifications and multi-session management.
- **Web UIs (what the web UI can do)**: a browser client for sessions. The questions that matter for a web-first harness: which UI is open source and self-hostable, whether it has authentication, and which of sessions, diffs, approvals, file browsing, terminal and settings it covers.
- **Mobile**: a phone client, or phone access to a session that runs elsewhere, so you can start, steer and approve work away from the machine.
- **Voice**: dictation into the prompt, or a conversational voice loop.
- **Messaging-app channels**: using a chat platform (Slack, Telegram, Discord, Teams and so on) either as the agent's primary UI or as a trigger that starts a cloud task.
- **Approval presentation (how a permission request reaches a human)**: the prompt that asks a human to allow a tool call: what it shows, what answers exist, and where it can be answered from.
- **Diff presentation and review (per-file/hunk accept and reject, comments on diffs)**: showing what the agent changed and letting the human keep, revert or comment on it.
- **Plan review (artifacts and comments on plans)**: a plan is produced before editing and the human approves, edits or comments on it first.
- **Notifications and "waiting for you" signals**: telling the human that a run finished, errored or is blocked on an approval or question, especially when the tab or terminal is not focused.
- **Managing several agents at once**: one place to launch, watch and switch between multiple concurrent agent sessions.

**Area 16. Browser automation and computer use**

- **The agent's own browser (embedded or managed)**: a browser the harness starts and controls (headless or visible), separate from the user's day-to-day browser.
- **The user's real browser (extension, DevTools attach)**: driving the browser the user already uses, with its logged-in sessions, through an extension or the DevTools protocol.
- **Browser isolation, site policy and credential handling (what the docs say)**: statements about what a browser tool can reach and what limits apply: separate storage, per-site approval, origin lists, SSRF rules, credential filling.
- **Computer use (screen control)**: the agent sees screenshots and sends clicks and keystrokes to a desktop or app.

**Area 18. Collaboration, teams and enterprise administration**

- **Accounts, roles, organisations and workspaces**: more than one human using the same harness with separate identities, roles and scopes.
- **Single sign-on and provisioning**: login through the company identity provider, plus automatic account creation and removal (SCIM).
- **Managed configuration and policy distribution**: an admin sets rules that users and repositories cannot override, delivered by file, MDM, a server, or a self-hosted config service.
- **Shared team resources (rules, prompts, servers, plugins)**: things a team defines once and every member gets: instruction files, prompt libraries, MCP servers, skills, plugin marketplaces, environments.
- **Budgets and usage controls per team**: limits on spend or usage per user, group or organisation, and how they are enforced.
- **Audit logs**: a record of who did what that an admin can export, as opposed to the operator's own debug log.
- **Session sharing and live collaboration**: letting another person see, steer or continue a session.
- **Tier summary: which of the web UI and multi-user features are in the open or free tier**

### Remote execution and architecture (area 17)

Evidence tables, one row per harness with its own source: [round1/synth/09-remote-distributed-architecture.md](round1/synth/09-remote-distributed-architecture.md)


**2.1 Headless and non-interactive modes**

- **One-shot headless run**: run one prompt (or a recipe) without an interactive UI, print the answer and exit, for scripts and CI.
- **Structured event stream and schema-checked output**: machine-readable output (JSON, NDJSON event stream) and an optional JSON Schema the final answer must satisfy.
- **Approval behaviour when nobody can answer**: what a headless or unattended run does when a tool call needs approval.
- **Config isolation for reproducible CI runs**: a way to run headless without picking up the user's or the repo's ambient configuration, so CI results do not depend on the machine.

**2.2 CI integrations**

- **Official CI action or pipeline integration**: a packaged way to run the agent inside a CI job (GitHub Actions, GitLab CI) triggered by events or mentions.
- **Machine credentials for CI and automation**: how a non-human run authenticates to the model and to the vendor's control plane.
- **Event-triggered remote runs (chat, tracker, git host, webhook)**: starting an agent run from an external event (Slack mention, Linear assignment, PR mention, signed webhook) rather than from the harness UI.

**2.3 Client protocols for remote control**

- **Programmatic control protocol (RPC, HTTP API, SDK transport)**: a documented protocol a separate process (UI, SDK, wrapper, remote client) uses to drive sessions: start, prompt, stream events, answer approvals, cancel.
- **ACP (Agent Client Protocol) support**: speaking the open editor-to-agent protocol, as an agent (served to editors) or as a client (hosting other agents).
- **Remote attach of a client to an agent server on another machine**: running the agent server on one machine (often a VM) and pointing a TUI, desktop or browser client at it.
- **Relay-based remote control of a session from web or phone**: a session keeps running on the user's machine while a browser or phone views and steers it through a hosted relay, without opening inbound ports.
- **Pairing and authentication of remote clients and execution hosts**: how a new device, client or worker proves who it is and gets a credential.
- **Exposure beyond loopback (bind, TLS, tunnels)**: how a server or gateway is made reachable from other machines, and what protects it when it is.
- **Reconnect, resume and takeover after link loss**: what happens to a running session when the link between control and execution side drops, or one side restarts.

**2.4 Distributed execution**

- **SSH remote execution**: using SSH to put work on another machine, either the whole agent (helper installed on the host) or only the tools.
- **Pluggable execution backend (tools run elsewhere than the loop)**: an interface inside the agent through which every file and shell operation goes, so the same loop can execute locally, in a container, over SSH or in a cloud sandbox.
- **Self-hosted workers for a hosted control plane**: the vendor (or a self-hosted gateway) keeps the control plane and UI; a customer-run worker process executes sessions on customer machines.
- **Work claim, lease and heartbeat**: how queued sessions get assigned to workers, and how the control side notices a dead worker.
- **Execution-side enrolment and per-dispatch credentials**: the long-lived credential a worker enrols with, and any shorter credential minted per session or dispatch so a leaked token is worth little.
- **Execution-side authority over policy**: whether the machine that runs tools has a local policy that the control side cannot widen.
- **Central credential broker or inference proxy**: one host holds provider credentials so execution machines or less-trusted clients can call models without holding long-lived keys.
- **Multi-node fleet under one control side**: one control side driving several execution machines, with per-machine selection.
- **Execution-side health, drain and metrics**: endpoints and flags an operator uses to know a worker is alive, take it out of rotation, and retire it cleanly.
- **Containerised deployment of the execution side**: shipping the worker or server as a container image or Kubernetes recipe.

**2.5 Cloud agents and their environments**

- **Vendor-hosted cloud agents**: agent sessions that run on vendor infrastructure in an isolated VM or container with a clone of the repo, and keep running when the user's machine is off.
- **Cloud environment definition and setup caching**: a saved description of the machine a cloud agent starts in (image, repos, setup script, env vars, network) and a cache of the prepared filesystem.
- **Cloud isolation and network egress**: the isolation unit for a cloud session and how its outbound network is limited.
- **Secrets in cloud environments**: how API keys and tokens reach a cloud run without leaking into the transcript, the repo or the model.
- **Credential injection outside the sandbox (egress proxy)**: the agent's processes never hold the real secret; a proxy on the boundary adds it to requests for allowed hosts.
- **Local and cloud hand-off**: moving a running conversation, with its working state, between the user's machine and a remote or cloud session.
- **Cloud agent control API and follow-ups**: starting, listing, steering and following up on remote runs from a CLI or REST API rather than the UI.
- **Idle pause and wake on demand**: releasing an idle remote sandbox or worker while keeping the session resumable, and bringing it back when a message arrives.
- **Warm pools and pre-warming**: keeping started-but-unassigned workers, processes or checkouts ready so a new session starts fast, without putting one user's data in the shared image.

### Configuration, licence, other (areas 20, 22, 23)

Evidence tables, one row per harness with its own source: [round1/synth/10-config-licence-distinctive.md](round1/synth/10-config-licence-distinctive.md)


**Area 20: Configuration and customisation**

- **Config file formats and locations**: where a harness keeps its settings, in which format, and what else sits next to it (credentials, MCP, keybindings, prompts).
- **Precedence between user, project, managed and command-line settings**: which layer wins when the same key is set in several places.
- **Managed (admin) policy layer and how it is delivered**: a configuration source an administrator controls that users and repos cannot loosen, and how it reaches the machine.
- **Repo-supplied config must not widen policy (workspace trust for config)**: rules that stop a cloned repository from changing credentials, providers, hooks or permissions through its own config files.
- **Profiles, presets and recipes**: named bundles of settings that can be switched or shared.
- **Environment variables**: process-level settings that override or supply what files hold.
- **Themes**: user-changeable colour schemes.
- **Key bindings**: user-remappable keyboard shortcuts.
- **Output styles, personas and system-prompt overrides**: persistent changes to how the agent speaks or behaves, separate from per-project instructions.
- **Status lines and footers**: a configurable line of live session data in the UI.
- **Hot reload and invalid-file handling**: what happens when a config file changes at runtime or is malformed.
- **Programmatic config editing**: changing settings without hand-editing the file.
- **Reading other harnesses' configuration and instruction files**: loading `AGENTS.md`, `CLAUDE.md`, `.cursorrules`, skills directories or MCP configs written for another tool, or importing their settings.
- **Diagnosis and recovery commands**: commands that check, repair or isolate a broken setup.
- **Feature flags and experimental toggles**: switches that gate unfinished behaviour, with maturity labels.

**Area 22: Licence, pricing and distribution**

- **Licence, open/closed split, install, pricing and governance (one table)**: the legal and commercial shape of each harness. Governance and ownership moves use the reviewed versions (Cursor acquisition confirmed; goose governance move corrected; Pi move confirmed; OpenCode V2/V1 confusion corrected; Warp licence split confirmed; Warp Factories relabelled Early Access).
- **Third-party use of vendor subscriptions and credentials**: terms that decide whether another harness may use a vendor's login, quota or gateway.
- **Self-update and release channels**: how the binary updates itself and whether stable and canary tracks exist.

**Area 23: Anything else or unusual**

- **Adjacent products around the harness**: separate products a vendor ships beside the harness, relevant mostly as scope warnings.
- **Artifacts and deliverables as a reviewed object**: agent output treated as a first-class thing to review, comment on and approve rather than a transcript.
- **Deep links**: a URL scheme that opens the app with a working directory or a prompt.
- **Agent-addressed docs and consent rules**: documentation or API behaviour written for an AI agent that reads the project, including limits on what the agent may do on the user's behalf.
- **Vendor-side feedback and one-off oddities**: small features that do not fit elsewhere.
