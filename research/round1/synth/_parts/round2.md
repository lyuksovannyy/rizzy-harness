### Agent loop, tools, editing, shell (areas 1-4)

From [round1/synth/01-loop-tools-editing-shell.md](round1/synth/01-loop-tools-editing-shell.md).

Only source code can settle these. Closed products (Claude Code, Claude Desktop, Cursor, Antigravity) are omitted except where a CLI command would settle it.

- **Codex, parallel read-only tools and `turn/steer`:** which tools count as read-only, how parallelism is scheduled, and at which point of the loop a steered message is injected relative to running tool calls.
- **Codex, `apply_patch`:** the patch grammar, how a failed hunk is reported to the model, and how CRLF preservation is implemented.
- **Codex, unified exec:** process lifetime, process-count limit, output buffering per `write_stdin` poll, and what `tool_output_token_limit` truncates.
- **Codex, Code Mode:** what the real default is, and how nested tool calls interact with approvals.
- **OpenCode, steering and inbox:** where a steered message enters the loop and what priority and cancellation semantics the inbox implements.
- **OpenCode, edit errors:** how "closest lines" are computed, and whether any fuzzy matching exists.
- **OpenCode, `/goal`:** how the reviewer works, and what `doom_loop` was in V1 and whether V2 has any repetition guard.
- **OpenCode, shell scanner:** how commands are extracted and why unparseable input is refused.
- **DeepSeek harness, pipeline and steering:** how pre-step interception and steering claims work at a step boundary; how monotonic guards are enforced; which tools each preset mounts; whether loop and web server share a process.
- **DeepSeek harness, filesystem policy:** what the default is when the policy plugin is absent, and how version freshness is computed.
- **DeepSeek harness, spill:** thresholds for `maxInlineTokens` and retention.
- **Oh My Pi, hashline:** the tag format, how hashes are computed, behaviour on a duplicated line or a deleted line, and what `sloppy` is.
- **Oh My Pi, steering:** what a "safe step boundary" means in code.
- **Oh My Pi, `eval` and minimizer:** kernel isolation, the loopback tool bridge, and the output minimizer rules.
- **Pi, `edit`:** the matching rules; and how parallel execution, `terminate`, and `withFileMutationQueue` are implemented; plus what the real default for `codemode` is.
- **Pi, output truncation:** whether the 50 KB or 2,000 line limit is enforced by the harness or only by convention.
- **OpenHands, stuck detector:** what the agent does after a detection and whether it is configurable.
- **OpenHands, `file_editor` and parallel execution:** failure modes of `str_replace`, and how `tool_concurrency_limit` serialises shared state.
- **OpenClaw, queue and writer claim:** how `steer` is injected into an active run, and how the `activeWriterRunId` check is done in the commit transaction.
- **OpenClaw, loop detection:** the thresholds on the built-in-tools config page that nobody opened.
- **goose, state machine:** how cancellation interacts with in-flight tool calls and what `--max-tool-repetitions` does; the Developer extension's tool list and edit mechanism; how the deferral and Code Mode caps are enforced.
- **Hermes Agent, interruption and parallelism:** how an interrupt interacts with the tool thread pool and why foreground terminal commands move to the background; which of 50 or 250 is the real subagent cap.
- **Hermes Agent, `patch`:** the fuzzy matching algorithm and its failure behaviour; how the `&`, `nohup` and `disown` rewriting works.
- **Hermes Agent, execution backends:** how the environment `execute()` contract is enforced across file tools.
- **Zed, queue and tools:** how Steer is delivered to the native agent at a step; `edit_file` matching and failure behaviour; how the terminal tool applies the hardcoded rules.
- **Warp (open client), Full Terminal Use:** how the agent reads and writes the terminal buffer, and how a user takeover is signalled.
- **Cline, `editor` and hub:** the edit tool's matching rules; how the hub routes capability requests; the `requires_approval` check and the retry and mistake-limit counters.
- **OpenCodex, sidecar and Remote Workspace:** how the bounded search loop hands control back to the client, and how the executor enforces its approved root.
- **Claude Code (CLI, if run):** `claude auto-mode defaults` and the other documented print commands would settle the read-only Bash allowlist and the env-scrub list.

### Permissions, sandboxing, security (areas 5, 21)

From [round1/synth/02-permissions-sandbox-security.md](round1/synth/02-permissions-sandbox-security.md).

Codex (CLI is open source):
1. Execpolicy: what exactly the tree-sitter splitter treats as "plain linear", and how an opaque invocation is matched (exact string, argv?). How `match`/`not_match` tests are executed at load.
2. Linux sandbox: the bubblewrap argument list and seccomp filter per mode; whether network is actually off by default under `workspace-write`; how the Unix-socket isolation that disqualified Landlock is done.
3. Auto-review: where the "fail closed" paths are and what transcript the reviewer receives; how the circuit-breaker counters persist across turns.
4. `shell_environment_policy`: the real default exclude list and whether `ignore_default_excludes = true` is the shipped default in code.
5. Hook trust by hash: what is hashed (file bytes, resolved command, plugin manifest) and where approvals are stored.

OpenCode:
6. The shell scanner: which parser per shell, how `cd` targets are inferred, and how "always" derives the saved patterns from a command.
7. Whether any OS sandbox or process isolation exists in V2 code despite no docs.
8. What policy applies when the Console is unreachable (cached, fail closed, fail open).

DeepSeek harness:
9. Default sandbox mode and preset at startup; how `partial` enforcement is decided per Landlock ABI.
10. Auto review integration: prompt, model and failure behaviour.
11. Whether the approval UI shows tool arguments anywhere, given requests omit them.

Oh My Pi:
12. `bash.patterns` segment splitting and the exact critical-pattern list; whether yolo really skips them in code.
13. `auth-gateway`: does any provider key ever reach the client process.

Pi:
14. What `enableInstallTelemetry` sends; whether a failing extension `tool_call` handler blocks in every mode.

OpenHands:
15. Whether `LLMSecurityAnalyzer` makes a separate LLM call; the real default confirmation policy when none is set.
16. Per-conversation Docker hardening flags as actually passed, and whether client-supplied limits are capped anywhere.

OpenClaw:
17. Exec approval binding: how the executable content hash and resolved-path re-check are implemented, and the TOCTOU window between check and launch.
18. Auto-review prompt and the three-denial escalation counter; sandbox mode defaults per backend.
19. Node pairing: what the device token is, how SSH-verified auto-approval compares keys.

goose:
20. Does `GOOSE_SANDBOX` exist in current source; Seatbelt profile and egress proxy if so.
21. Smart Approval risk criteria; adversary-mode prompt; malware-check data source.

Hermes Agent:
22. Hardline matcher implementation and what "fails closed on unparseable quoting" means in code; how deny globs "see through" `sh -c`.
23. iron-proxy integration: how tokens map to keys, and what happens to Hermes's own LLM calls.
24. `smart` approver prompt and the 300 s timeout path.

Zed (open source):
25. Terminal chain splitting: which shells are parsed; bubblewrap profile; when Linux falls back to unsandboxed.
26. Where `sandbox_permissions` grants scoped "always" are stored and whether a repo can ship them.

Warp (client open source):
27. The shipped default denylist, and whether any local-agent sandbox code exists; redaction regex list and where it runs.

Cline:
28. Whether `requires_approval` is enforced anywhere beyond the model's flag (e.g. a static override); what `CLINE_SANDBOX` does; whether the hub has any authentication in code.

OpenCodex:
29. Remote Workspace bubblewrap profile and the "command-sandbox probe" that gates write access.

ACP (open protocol, Rust SDK):
30. Whether the SDKs implement any default client-side policy for `fs/*` and `terminal/*`, or leave it entirely to the client.

### Context management and memory (areas 6, 7)

From [round1/synth/03-context-memory.md](round1/synth/03-context-memory.md).

Source-code-settled questions for the open-source harnesses.

1. **Codex, memories**: what exactly is in an extracted memory file, how it is injected into a turn, and where redaction runs (`memories_v2` store); does `disable_on_external_context` apply at generation or at use?
2. **Codex, AGENTS.md**: is `project_doc_max_bytes` per file or combined, and are empty or override files counted?
3. **Codex, compaction**: what `model_auto_compact_token_limit` scope `body_after_prefix` counts, and what failed-compaction recovery does.
4. **OpenCode, compaction**: how the buffer, `keep.tokens` and summary reuse of the prompt cache are built; how the pre-written background summary is invalidated and what is lost on restart; where keyed session instructions are placed.
5. **OpenCode, instructions**: how nested `AGENTS.md` are tracked as "already in history" and re-injected after compaction.
6. **Oh My Pi, compaction**: how `methodOrder` falls through on failure, how snapcompact renders and re-reads images, and what survives (including TTSR injections).
7. **Oh My Pi, memory**: how the `local` distillation chooses and writes `memory_summary.md`/`MEMORY.md`/`learned.md`, whether `learn` and `manage_skill` writes need approval, and which memory tools exist per backend.
8. **Pi, compaction and `context_edit`**: how cut points, split-turn summaries and `context_edit` are implemented and whether users can create the latter; how idle cache warming decides.
9. **OpenClaw, dreaming and compaction**: the real thresholds (`minScore`, `minRecallCount`, `minUniqueQueries`), how the 25% rewrite guard works, how `safeguard` audits summaries, and the retention and deletion gaps.
10. **OpenClaw, workshop skills**: what the agent-learned skills tier is and who writes it.
11. **Hermes, memory and curator**: how the frozen snapshot, injection scan and `write_approval` staging are enforced; the in-place compaction lineage (`in_place` default); how the Curator lock blocks agent writes including terminal writes.
12. **Hermes, compression**: whether a manual compress command exists; the real behaviour when the summary model's window is smaller than the main model's.
13. **goose, compaction and memory**: how `GOOSE_TOOL_CALL_CUTOFF` and background tool-output summarisation work; what the built-in Memory extension stores; whether "Top of Mind" equals MOIM; precedence of `AGENTS.md` versus `.goosehints`.
14. **OpenHands, condenser and memory**: how `Condensation` events are filtered into the view and how the 6,000-char index trimming and untrusted wrapper are built.
15. **DeepSeek harness, compaction and spill**: numeric thresholds, how the locked sequence recovers from a crash orphan, and the config catalogue defaults.
16. **Zed, instructions and compaction**: which of the nine instruction files the Zed Agent actually loads; how the threshold syntax (percent, tokens, negative) is parsed.
17. **Cline, compaction**: what `basic` versus `agentic` compaction do and whether the Auto Compact and Memory Bank pages describe the current harness.
18. **Cursor (closed, public surface only)**: not source-settleable; the SDK `MemoryBackend` interface and automation `MEMORIES.md` handling are the only open-ish parts.

### Planning, sub-agents, background work (areas 8-10)

From [round1/synth/04-planning-subagents-background.md](round1/synth/04-planning-subagents-background.md).

- **Codex (CLI, open):**
  - What are the prompts and tool sets of the built-in agents `default`, `worker` and `explorer`?
  - Is plan mode enforced by tool removal, by the sandbox, or by prompt?
  - What is the default for `max_concurrent_threads_per_session`?
  - How does goal "automatic continuation" decide to continue, and what stops it besides the token budget?
- **OpenCode:**
  - Does the `plan` agent's rule set actually deny shell, or only edits?
  - How does the `/goal` reviewer judge, and with which model?
  - How do `experimental.schedules` "fire once across multiple server processes" (lock, lease, or DB row)?
  - What model does the hidden `title` agent default to when `agents.title.model` is unset?
- **DeepSeek harness:**
  - What is the default `maxDepth` and `maxGoalRounds`?
  - Does a sub-agent inherit the parent's sandbox mode and approval policy (the docs are silent)?
  - How are reminder deliveries made idempotent on restart?
  - Which model generates titles?
- **Oh My Pi:**
  - What is the exact role auto-resolution algorithm when `modelRoles` is empty?
  - How does `task.isolation` choose a backend and apply patches on conflict?
  - Is plan mode read-only by tool removal?
  - How is the advisor's "blocker interrupt" implemented?
- **Pi:**
  - In the example sub-agent extension and pi-durable, how is a child's tool set and cwd constrained?
  - Does pi-durable's restart limit count per task or per run?
- **OpenHands:**
  - Does the task tool enforce any depth or count limit in code?
  - How are approvals from ACP-hosted agents (Claude Code, Codex) surfaced or answered?
  - Which LLM runs the goal judge by default?
- **OpenClaw:**
  - What are the sub-agent depth defaults and the tool-loss list?
  - How is "one retry for interrupted agent-turn jobs" recorded so it cannot repeat?
  - Is the cloud worker connection outbound-only in code?
- **goose:**
  - Do sub-agents inherit the parent's provider or model when `GOOSE_SUBAGENT_MODEL` is unset?
  - Where does the scheduler store jobs, and what happens to a run missed while `goose serve` is down?
- **Hermes Agent:**
  - Which model does each auxiliary task use when unset, and do the auxiliary fallback chains share cooldown state with the primary chain?
  - Does the goal judge's fail-open path have any cap?
  - How does the kanban dispatcher reclaim a crashed claim without double-running a worker?
- **Zed:**
  - Does `spawn_agent` enforce any depth or concurrency limit?
  - Do `thread_summary_model` and `compaction_model` default to the main model?
  - What tools does a sub-agent get when the parent's profile restricts them?
- **Warp (open client):**
  - Is orchestration (child launch, inbox) implemented client-side or only on the closed control plane?
  - Where is the planning-model switch applied?
- **Cline:**
  - Does `spawn_agent` really have no depth, count or timeout cap?
  - Is plan mode's shell restriction a blocklist in code?
  - Does a schedule created while the hub is stopped fire later, and with what approval mode?
- **Zed/ACP protocol:** In the sub-agents RFD's reference implementation (if one exists), can a child session's permission requests reach the client?

### Sessions and git (areas 11, 12)

From [round1/synth/05-sessions-git.md](round1/synth/05-sessions-git.md).

Source code only can settle these. Open-source harnesses only.

Codex (open CLI):
- Rollout files plus SQLite index: what exactly is the write path and recovery after a crash mid-write; are rollout files append-only and how does the index get rebuilt? Is the `thread/rollback` removal documented anywhere in code, and what replaced it?
- Resume: how are saved permissions and network approvals applied against managed policy (precedence) when the session resumes?

OpenCode:
- Snapshot store: how does it attribute changed paths to assistant steps, what is the exact tracked/untracked/ignored rule, and what does the 2 MiB cap do to larger files? How does `/undo` handle a path changed by both the user and the assistant?
- Session database engine and schema, and the event-log contract behind `GET /api/experimental/session/{id}/log` (exclusive sequence, ordering, retention).
- Worktree "strategies" plugin contract and what it may do with repo git config.

Oh My Pi and Pi:
- Branch navigation (`/tree`): what happens to file state, what is stored in `branch_summary`, and how is the active leaf persisted? Is the v3 format append-only or are entries rewritten?
- `omp commit`: how the dependency-ordered split and cycle rejection are actually implemented and what the mechanical fallback commits.
- `github` tool: how "push only to branches it checked out" is enforced.
- Pi `git-checkpoint.ts` example: what it does and whether it is a real checkpoint design.

DeepSeek harness:
- Persistence: zstd frame and checksum format, torn-tail truncation algorithm, per-batch fsync cost, and how `session/end-seed` is used on replay. Whether the `create_worktree` tool hardens git config. The `POST /github` webhook: dedupe and failure behaviour in code.
- Whether undo/checkpoints exist in code even though no page documents them.

OpenHands:
- Event-per-file persistence: crash consistency of `base_state.json` (rewritten whole) versus `events/`; what `fork()` copies and shares; encryption of persisted secrets (`OH_SECRET_KEY`).

OpenClaw:
- Writer claim: the transaction that checks it, behaviour on a superseded writer, and the worktree snapshot ref logic (`refs/openclaw/snapshots/<id>`), including how dirty-idle (>7 days) deletion is guarded.

goose:
- `goose review` flag semantics and orchestration default; session `sessions.db` schema; whether any checkpoint/undo exists in code.

Hermes Agent:
- Shadow-git checkpoints: the destructive-command detection list, the one-per-directory-per-turn rule, how nested repos become gitlinks, the pre-rollback snapshot, and exactly how user edits since the snapshot are detected and skipped. `hermes -w`: how hooks, fsmonitor and filters are disabled at checkout.
- Gateway crash handling: how a turn is marked `unknown` and what discarding does.

Zed/ACP:
- What the removed checkpoint code did (which git operations, how often, on what size repos) and whether any replacement is planned; how the worktree archive restore re-creates git state; commit-message generation prompt and the three-commit style examples.

Warp (open client):
- Handoff snapshot contents (declaration scripts or static JSONL), how untracked files are captured, and the "blocked while a command runs" rule that was unverified.

Cline:
- Shadow git: location on disk, exclusion rules, size handling, commit-per-tool-use cost, what happens to files created after the checkpoint, and the SDK's "one git process" diff mechanism.
- Hub: how sessions are persisted and re-attached across clients.

Not answerable from code (closed): Cursor checkpoint mechanism and retention, Claude Code transcript and checkpoint internals (public docs only), Antigravity `/rewind` behaviour.

### Extensibility (area 13)

From [round1/synth/06-extensibility.md](round1/synth/06-extensibility.md).

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

### Models and observability (areas 14, 19)

From [round1/synth/07-models-observability.md](round1/synth/07-models-observability.md).

Open-source harnesses only; Claude Code, Cursor, Antigravity and Claude Desktop are closed.

- **Codex, retry and overload.** How are retries counted and delayed (the entry only says `Retry-After` is honoured), what exactly makes it stop on "model not found", and does the WebSocket-to-HTTP fallback preserve partial output?
- **Codex, goals.** How is `tokenBudget` enforced: checked before each continuation or per model call, and what happens on overshoot?
- **Codex, OTel.** Where is the 2,048-byte tool-result cap applied and is prompt redaction a pattern match or a drop?
- **OpenCodex, combos.** How are hop errors classified, how is cooldown stored (memory only, as thread affinity is), and how does "metered-target consent" gate a hop? How does the estimated-cost table get its prices and how are coverage warnings computed?
- **OpenCodex, account pool.** What does "lowest-usage" compare across 5h/weekly/30d windows when windows disagree?
- **OpenCode, cost and stats.** How `opencode stats --cost` computes cost (price source, cache tokens) and how Go allowance windows are enforced in the client versus the Console.
- **OpenCode, retries.** Nothing documented; settle by reading the provider layer. Also what cache warming does and when `settings.cache.ttl` applies.
- **DeepSeek harness, telemetry.** What the redaction waterfall does by default when no rules are registered, and what the `token-meter` subsystem exposes for cost.
- **DeepSeek harness, compat flags.** How `supportsDeveloperRole`, `maxTokensField` and `thinkingFormat` change request bodies.
- **Oh My Pi, fallback and pools.** How `usageAwareFallback` reads coding-plan quota and applies the reserve; whether round-robin credential selection with session affinity exists as the README claims (`omp dry-balance`); the backoff constants in the `retry.*` namespace; where `omp stats` stores data.
- **Oh My Pi, roles.** How roles auto-resolve from authenticated models when `modelRoles` is empty, and why `advisor` will not inherit default.
- **Pi, retry.** How agent-level retry (3, 2000, 60000) and provider-level retry (0) interact on a streamed response; how billed vs estimated cost is determined; what `pi-telemetry` spans contain and who calls them.
- **OpenHands, LiteLLM layer.** What retry and fallback LiteLLM is configured with; how per-conversation budget meters and Enterprise budgets are enforced; which spans carry token counts.
- **OpenClaw, failover.** The exact profile-rotation and cooldown algorithm (30 s, 1 min, 5 min cap; 10 minutes for billing failures) and how "prefer the working fallback for 30 minutes" is stored; what `/usage cost` reads from providers.
- **goose, rate limits and logs.** Whether any generic retry exists beyond Bedrock and Databricks env vars; how `llm_request.N.jsonl` rotation works and whether it redacts headers; what the Langfuse integration sends.
- **Hermes Agent, fallback and pools.** The retry policy before fallback fires, the credential-pool rotation rules, how auxiliary fallback chains differ from the main chain, and what "spend limits" in the approval layer are.
- **Cline, cost.** How `totalCost` is computed (catalogue prices, cache pricing) and what the OpenTelemetry events contain.
- **Warp client (open), routing.** Where custom model routers are evaluated (client or Warp's control plane) and how fallback routing between comparable models is chosen.
- **Zed and ACP, config options and usage.** How `session/set_config_option` categories (`model`, `thought_level`) are rendered, and what the v1 usage update carries versus the end-turn usage RFD.

### UI, browser, collaboration (areas 15, 16, 18)

From [round1/synth/08-ui-browser-collaboration.md](round1/synth/08-ui-browser-collaboration.md).

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

### Remote execution and architecture (area 17)

From [round1/synth/09-remote-distributed-architecture.md](round1/synth/09-remote-distributed-architecture.md).

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

### Configuration, licence, other (areas 20, 22, 23)

From [round1/synth/10-config-licence-distinctive.md](round1/synth/10-config-licence-distinctive.md).

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
