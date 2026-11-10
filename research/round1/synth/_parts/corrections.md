### Agent loop, tools, editing, shell (areas 1-4)

From [round1/synth/01-loop-tools-editing-shell.md](round1/synth/01-loop-tools-editing-shell.md).

What the reviews changed (only items in these areas):

- Claude Code: review 1 confirmed the Bash timeout, background, memory-cgroup, Read, Glob/Grep, WebFetch, WebSearch and Monitor entries and found no correction in Areas 1-4 (the unchecked remainder was sampled and "looked reliable"). Review 3 corrected the self-hosted runner description: the control plane sends each session's tool list and permission rules with the work, and also model, connectors and lifecycle hooks; it is not just "policy".
- Hermes Agent: the agent-loop page does not use the word "synchronous"; the iteration budget (500, `agent.max_turns`) is confirmed. The 50 versus 250 subagent cap disagreement between pages is real.
- OpenClaw: the writer-claim entry was corrected (the commit transaction verifies the claim; the state-directory lock is not on the queue page); the 2-minute stuck threshold applies only with diagnostics enabled and 5 minutes is a floor.
- Cline: roles in the hub are per session participant, not per client; the no-authentication statement for the hub was verified.
- goose: tool-output unit is characters; tool deferral mechanism is described on the config-files page. The review also found the sandbox "nothing documented" claim wrong in another area, so treat every goose "nothing documented" (including edit mechanism) as needing a second look.
- Pi: the survey's claims that failed codemode scripts do not roll back and an output cap exist were not found on the page and were removed from the verified set.
- Oh My Pi: hashline numbers match the blog, which says 15 models in the title and 16 in the body, is dated 2026-02-12 and is written by the omp maintainer.
- OpenCodex: "runs no agent loop" is a reading of the how-it-works and architecture pages, not a quoted claim; Remote Workspace details (pairing rate limit, outbound connection) were corrected.
- DeepSeek harness: the SSH page is on GitHub, not on the docs site; one-process shape is an inference; the last 25,000 characters of the generated tool catalogue were not read, so default-mounted tool sets and defaults are unconfirmed.
- Antigravity: Auto-mode and sandbox "Preview" details could not be confirmed; its `/goal` entry was confirmed.

Contradictions inside vendors' own pages:

- Pi: the CLI page says `codemode` and `tool_search` are off by default; the settings page says on; the MCP page says MCP tools default to `codemode` exposure. Not reconciled without a run.
- Oh My Pi: README says 31 tools and different coreutil counts in different places; `ast_grep` is off in the docs but a headline tool in the README; README lists a `skill://` scheme that the skills page says it does not document; `chillax` keyword in README only. Default approval mode is `yolo`.
- Codex: `Code Mode` is "under development and off by default" in the config reference while releases ship user-facing Code Mode features; the app-server is "experimental and not supported for production" yet carries every client; no tools reference page exists.
- OpenClaw: Tool Search is listed under experimental features but described as on by default.
- Hermes Agent: Tool Search is called opt-in yet the default `auto` behaves as on; the architecture page says seven terminal backends in a diagram and six in prose.
- Cline: docs mention legacy tool names (`write_to_file`, `read_file`) next to the new names.
- Claude Code: the weekly digest stops at Week 37 and lags the changelog; `TaskOutput` was deprecated in 2.1.83 and removed in 2.1.277; task tools were withdrawn on newer models in 2.1.233; `taskOutputMaxChars` lived about two weeks.
- goose: docs still name the server `goosed` in places; the loop was replaced in v1.55.0 and v1.58.0 so the GDK pages may lag.
- Antigravity: the slash-command catalogue calls itself complete but omits commands; no tools reference exists.

Not confirmed by anyone (unverified):

- Claude Code: the exact list `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` removes and the read-only Bash allowlist (the docs say to print them with a CLI command); how the server-side probe of tool results works.
- Cursor: terminal background processes, timeouts and output truncation (not on the terminal page).
- OpenCode: how the `/goal` reviewer works; what `doom_loop` did in V1.
- OpenHands: the stuck detector's halt behaviour; send-message-while-running, pause and resume mechanics (index titles only); `ApplyPatchTool` GPT-5 preset (index only).
- Pi: edit matching rules, read limits and grep/find implementation (no tools reference page located).
- Oh My Pi: the hashline tool schema and fallback; `eval` kernel isolation and the tool-callback bridge; whether bash can run over SSH.
- goose: the Developer extension tool names and edit mechanism; `--max-tool-repetitions` semantics.
- DeepSeek harness: default tool presets and sandbox mode; the exact signatures of the steering interception.
- Codex: the full built-in tool list and each tool's parameters; remote executors and `codex exec-server`.
- Warp: any public tool list; checkpoints or undo for agent edits.

### Permissions, sandboxing, security (areas 5, 21)

From [round1/synth/02-permissions-sandbox-security.md](round1/synth/02-permissions-sandbox-security.md).

What the reviews changed:
- Codex: "network off by default for workspace-write" not confirmed on either page and moved to Unverified; "WSL1 dropped at 0.115" removed; permission profiles do not require 0.138.0 (only a mixed-version rollout note); project config blocklist also includes `apps_mcp_product_sku`; hooks page gives no GA label.
- Claude Code (review-1): plan mode also runs classifier-approved commands when auto mode is available; SDK `failIfUnavailable` default true moved to Unverified (CLI docs imply off). Review-1 also flags that two entries disagree on when project `defaultMode: bypassPermissions` started being ignored (v2.1.257 vs Week 36), and that the auto-mode classifier engine entry at line 1062 is stale relative to line 705.
- Claude Code (review-2): mods fail open by default; the built-in guard loads only with managed settings, `managed-mcp.json` or a Team/Enterprise login, so API-key users get no guard and a user mod can approve past deny rules; even with the guard, a mod can read `.env` via `$.fs.read`.
- Claude Code (review-3): sandbox credential masking (sentinel, needs experimental `tlsTerminate`, sandboxed Bash only, macOS blocks masked files) and cloud network secrets (no sentinel, key never in the VM) are different mechanisms, not "the same pattern"; work orders register exactly one runner in orchestrator mode, not one per session; Remote Control has no documented lease; the claude.ai login restriction has a carve-out allowing an unmodified `claude` binary signed in by the end user.
- Claude Desktop: Cowork has both local-VM and cloud modes; the getting-started article's cloud-only statement is stale. Chat egress has a fourth option and non-Team defaults.
- Cursor: hook precedence is deny > ask > allow for permission decisions but last-response-wins for other fields (not "most restrictive wins"); Windows sandboxing not documented; a `bubblewrap` status value unexplained; Legacy Privacy Mode statement is on the cloud security page.
- Antigravity: `toolPermission` not mentioned on `/cli/modes`; "Preview" label for business sandbox not found; bubblewrap confirmed only on `/enterprise`; allow-rule-to-mount wiring documented for the App only; `trustedWorkspaces` semantics partly documented.
- goose: the original "no sandbox documented" was not supported; a macOS Desktop sandbox (`GOOSE_SANDBOX`) appears in search snippets but its page 404s, so it is unverified.
- Hermes: container approval bypass is on the security page, not the tools page; iron-proxy binds the Docker bridge gateway (loopback on Docker Desktop), never 0.0.0.0.
- OpenHands: hooks are not part of the documented security model (only a proposed fix); the FAQ's no-auth statement is general, not "open-source build".
- OpenClaw: cloud workers are beta, not GA, and "outbound-only" is not stated; env binding of approvals is conditional; per-node pairing token removal dated (store Jan 2026, RPC 2026.7).
- OpenCode: a policy `allow` can lift an earlier policy deny; "Console policies persist when unreachable" dropped.
- Warp: the denylist bypass applies only to the explicit "Run until completion" shortcut, not to "Always allow".
- Cline: hub roles are per session participant; Kanban CDK root and token-in-user-data are inferences.
- DeepSeek: `$DSH_HOME/.credentials.yaml` is on the providers guide, not the credentials page; SSH subsystem page exists on GitHub.
- OpenCodex: remember-device is a 30-day record expiry and off by default; pairing code single-use with five-minute lifetime; rate limit is ten failed codes per ten minutes.

Where a vendor's own pages disagree:
- Codex: the retired `untrusted` approval policy "can stop clients starting", yet managed `allowed_approval_policies` "must include `untrusted`".
- OpenHands: default confirmation policy ConfirmRisky (architecture page) vs "not stated" (guide); LLM analyzer inline-only vs also a separate LLM.
- Cursor: cloud repository copies deleted after the run (privacy page) vs VM snapshots with code kept 30 days and not deletable (cloud security page).
- Cline: "every action requires your explicit approval" (overview) vs CLI auto-approve default true, schedules default yolo, SDK default-allow; YOLO enterprise control "all interfaces" vs "no surface"; `--auto-approve` vs `--yolo` naming.
- Antigravity: browser called "sandboxed" in the slash-command catalogue while it drives the user's real Chrome profile.
- Claude Code: auto mode "available to all users on every provider" vs model- and provider-gated.
- DeepSeek: session telemetry default `FEEDBACK_ONLY` (CLI README) vs no default stated (subsystem page); maturity labels for Auto review inconsistent across pages.
- Hermes: plugin page says 27 lifecycle hooks but lists 38.
- OpenClaw: `telemetry off` and `DO_NOT_TRACK` leave the daily check on (documented but easy to misread); prompt-injection page cites an unsourced attack-success test.
- Zed: sandboxing has no maturity label and appears in release notes only as a bug fix.

What nobody could confirm:
- Claude Code: how the server-side tool-result probe works; the exact variable list `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` removes; the full built-in classifier rules and read-only Bash list (printable only by running the CLI); cloud VM isolation technology; mods' isolation level ("worker thread").
- Codex: relay architecture and encryption for remote control; per-task isolation model of Codex Cloud.
- Antigravity: Auto evaluator mechanism; `sandboxProxy` details; Remote Control relay and encryption.
- DeepSeek: Auto review reviewer model, rules and prompt; default sandbox mode.
- goose: Smart Approval criteria; malware check data source; macOS sandbox existence.
- Cursor: worker wire protocol and encryption ("not documented" per the docs).
- OpenCode: telemetry of the local app; any OS sandbox.
- Pi: what install telemetry sends; whether `auth.json` and sessions are encrypted at rest.
- Oh My Pi: `eval` kernel isolation and tool-callback bridge security.
- Cline: hub authentication; meaning of `CLINE_SANDBOX`.

### Context management and memory (areas 6, 7)

From [round1/synth/03-context-memory.md](round1/synth/03-context-memory.md).

Corrections applied (from reviews):
- Claude Code idle compaction: the survey had three conflicting entries; review kept "200,000 tokens", "one-hour cache TTL" and `idleCompaction: false (v2.1.290+)`, and moved "about 54 minutes", "Terminal 2.1.299, Oct 14, 2026", "/config row 2.1.302", "about an hour idle", "machine awake" and the 100,000 floor to Unverified.
- Claude Code `promptCacheTtl`/`subagentPromptCacheTtl` version is v2.1.242 (docs page), not 2.1.243.
- Claude Code `.claude/AGENTS.md` being read was not found in memory.md (Unverified).
- Oh My Pi: idle compaction is off by default, not "optionally on"; 20k keep-recent is a summarising-compaction target; `agents` and `codex` instruction providers are tied; scope limits added; per-backend memory tool availability and mnemopi retain interval (every 4 turns) added.
- Zed/ACP instructions: first match wins, in the listed order of nine files; project `.rules` still supported.
- goose tool deferral: mechanism is on the config-files page (names up front, schemas via `enable_tools`), not undocumented.
- Cursor: Memories launched in 1.0 beta (changelog); the forum reply was not shown as staff and two other threads were not opened.
- OpenCodex: the 256 MiB memory accounting figure was removed (not found).
- Pi's "under 1,000 tokens" prompt claim is from a 2025 post, not current docs.

Contradictions between a vendor's own pages:
- Codex `project_doc_max_bytes`: combined (AGENTS.md page) vs per file (config-advanced page). Codex memories "off by default" vs 0.165.0 "memory v2 default for fresh profiles" (the latter is the store version).
- Hermes: sessions page says compression creates a child session, compression page says in-place is the default (`in_place: true`); README says FTS5 session search uses LLM summarisation, memory and sessions pages say it does not; prompt-assembly page puts the skills index in two tiers.
- Oh My Pi: docs/memory says no named memory tools, docs/tools and README list `retain`/`recall`/`reflect`/`memory_edit`/`learn`; README says a `skill://` scheme exists, skills doc documents none; whether TTSR injections survive compaction (README claim) is unverified.
- Antigravity: `/context` page does not mention `/compact` while the changelog does.
- Zed: the Instructions page lists nine files and also says the Zed Agent loads `AGENTS.md`, `CLAUDE.md` and copilot-instructions; unclear whether `.rules` and the legacy names are still loaded by the Zed Agent.
- OpenClaw: dreaming is default-on yet promoted memories have no time-based retention (project's own admission); its page lists `TOOLS.md` and `HEARTBEAT.md` that the file map does not.
- OpenHands: `max_size` vs `max_context_length` for the same value; the guide gives no product defaults while the architecture page gives 120/4.
- Cline: Auto Compact and Memory Bank pages appear legacy (not in the llms.txt index); Auto Compact page still mentions todo lists after Focus Chain was deprecated.

Not confirmed by anyone:
- Claude Code: which model summarises; the 100K-1M window range and `CLAUDE_CODE_MAX_CONTEXT_TOKENS` detail (page truncated); whether old tool results are cleared.
- Cursor: whether Memories still produce agent-visible memories; codebase indexing, embeddings and `.cursorindexingignore`.
- Antigravity: whether an earlier Knowledge feature existed; algorithm and thresholds of compaction; `/codesearch`.
- goose: the context window report and "context relevance operation" (v1.58.0 notes); the Memory extension mechanism; whether the "Top of Mind" extension is the MOIM persistent-instructions feature.
- Warp: where embeddings are computed; Agent Memory retrieval mechanism.
- Claude Desktop: memory import/export article (not opened); Hermes Honcho hosting details; OpenClaw `workshop` skills tier mechanism; OpenCodex prompt layers; DeepSeek harness compaction thresholds (generated config catalogue not opened).
- Pi: mechanism of idle cache warming; how `context_edit` is created.
- OpenClaw dreaming numeric thresholds are not on the page (review).

### Planning, sub-agents, background work (areas 8-10)

From [round1/synth/04-planning-subagents-background.md](round1/synth/04-planning-subagents-background.md).

**Changed by review (these areas):**
- **Claude Code.** Background sessions, the supervisor CLI, agent view, `--bg`, the daemon and attach/logs/stop are **research preview**, not GA (review-2 corrections 1 and 2). The agent-view introduction version (2.1.139) is not on agent-view.md (correction 3). Cloud scheduled tasks are the product formerly called routines, with a **1-hour minimum interval**, and the 30/hour per-task cap is one shared count (correction 5). "Skipped up to 72 hours then turns off" moved to Unverified (review-3). Plan mode also runs classifier-approved commands when auto mode is available (review-1 correction 1).
- **Claude Desktop.** `/schedule` was not removed (typing it requests a schedule); Cowork versus local scheduled tasks are two products, not a contradiction; Dispatch's 30-minute approval window comes from the desktop page, not the support article.
- **OpenHands.** DelegateTool is "retired", not "replaced".
- **DeepSeek harness.** The agent-team page exists on GitHub, not on the docs site; `write_scopes` are advisory; reminders "do not guarantee exactly-once" (the page does not say "at-least-once").
- **Antigravity.** `/teamwork` horizon is "multiple days".
- **Warp.** Factories are "Early Access". Orchestration "wake on message" and "one worktree per agent" are not on the cited pages; orchestration has no status label.
- **OpenClaw.** Cloud workers are beta, and "outbound-only" is an inference. The writer claim is verified in the commit transaction, not written in it.
- **Pi.** Background-owned work survives the parent's abort in pi-durable; the to-do non-feature reason is "confuse models".
- **Cline.** Hub roles are per session participant.
- **Codex.** The presets 8,000-byte limit moved to Unverified.

**Vendor pages that disagree:**
- **Claude Code** sub-agent nesting went 5 → 0 → 3 within six weeks (the digest still says 5). `/fork` has meant three things, and today depends on whether agent view is on. Cross-session messaging works on "any of your machines" (changelog) or the same machine (digest).
- **Hermes** delegation `max_iterations` is 250 on the delegation page and 50 on the agent-loop page.
- **Cline** says plan mode "cannot execute commands" on one page and only blocks mutating commands in sub-agents and schedules on others. Kanban auto-PR is described on one page and not on another.
- **OpenCode** README (plan asks before bash) disagrees with the V2 permissions page. `/goal` and schedules exist only in the changelog.
- **OMP** README "schema-validated typed results" disagrees with the docs, where schemas come only through `output`. The README lists 9 roles; the docs list 10.
- **goose** says sub-agents are disabled outside autonomous mode, which conflicts with using approvals and delegation together. v1.55.0 removed "background subagents and orchestrator".
- **Antigravity**'s App page still says "Fast mode" after the CLI removed `/fast`.
- **DeepSeek** schedules are labelled experimental on the landing page and unlabelled in the guide.
- **OpenClaw**'s home page still advertises "heartbeat scheduling" after the heartbeat was retired.

**Nobody could confirm:**
- what Codex's built-ins `default`/`worker`/`explorer` actually do, and the default of `max_concurrent_threads_per_session`;
- how OpenCode's `/goal` reviewer works;
- whether Antigravity timers survive closing the app;
- OpenClaw's sub-agent depth defaults (subpage unopened);
- Hermes webhook details (page unopened);
- Warp scheduled-agent timezone and overlap behaviour;
- Cline hub behaviour after the hub stops, and zen mode;
- the model behind Claude Code's compaction (review-1 omission 3) and WebFetch extraction;
- the model behind DeepSeek's title generation;
- OpenHands' routing and fallback guides (index only);
- the Claude Code changelog-only variables `CLAUDE_CODE_SUBAGENT_EFFORT` and `CLAUDE_CODE_WORKFLOW_SUBAGENT_MODEL`.

---

### Sessions and git (areas 11, 12)

From [round1/synth/05-sessions-git.md](round1/synth/05-sessions-git.md).

Corrections applied (from the reviews):
- OpenCode session sharing: was "nothing documented"; it was discontinued in v1.19.1 (15 Oct 2026). The V2 config `share` field statement comes from a search snippet only and the V2 config API page returned 404 (corrected in review).
- Zed checkpoint removal: removed in 1.27.0 (2026-11-04); the Agent Panel page is silent (there is no removal note on it); 1.25.2 is dated Oct 21 2026 and 1.23.3 Oct 12 2026 (corrected in review).
- Codex Handoff: moves a chat and its Git state between the local computer and a connected remote host; the phrase "to a worktree" is not on the remote-connections page. (The Local/Worktree handoff comes from the git-worktrees page.)
- Warp Handoff: `/handoff` confirmed; the detail "blocked while a command runs or child agents are active" was not on the pages opened and is unverified; the 2026.10.15 and 2026.09.23 periodic-checkpoint changelog claims were not checked. Warp multi-agent "one worktree per agent" advice was not found on the cited pages.
- Claude Code agent-view related status: "GA" was wrong; background sessions, agent view, `--bg` and its background-worktree isolation and PR-status rows are research preview (review 2). Background worktree isolation is labelled research preview here.
- Claude Code checkpoint limits (100 checkpoints, 30-day sweep, six menu actions, Bash and subagent exclusions, symlinks) were first unchecked in review 1 and then confirmed by review 2.
- OpenClaw per-session writer claim: the commit transaction verifies the claim; a state-dir lock is not on the queue page (corrected in review).
- Cursor cloud retention: indefinite by default, 30-day VM snapshot inactivity deletion, snapshots not deletable on demand (reviewed).

Contradictions:
- Cursor: the privacy page says Cloud Agents store encrypted repository copies only while running, then delete; the cloud security page says VM snapshots with cloned code are kept 30 days after inactivity and cannot be deleted on demand. The wording contradiction is real (reviewed).
- Cline: the checkpoints page and the IDE page name the three restore options differently ("Files only / Task only / Files and Task" versus "Task and Workspace / Task Only / Workspace Only"). The prompt-storage page names `~/.cline/data/tasks/` while the CLI layout names `data/sessions/`.
- Codex: GitLab MR review is "beta" on the GitLab page and "preview" on the code-review page.
- Cline Kanban: the overview mentions auto-commit/auto-PR settings, core-workflow does not describe auto-PR.
- Zed: `session/resume` and `session/load` stabilised Feb-Jun 2026, but ACP session fork exists only as an RFD title in the index.
- DeepSeek harness: the persistence page does not give the storage root; the Python SDK page gives `<dsh_home>/sessions` for the minimal profile only.

Unverified or unconfirmed:
- Antigravity `/rewind` and `/undo`: whether files are restored; checkpoint storage location and retention.
- Oh My Pi `/dump` secrets warning (not on the sessions page); `/annotate` is README-only.
- Pi `git-checkpoint.ts` and `auto-commit-on-exit.ts`: names only, contents not opened.
- Cursor "Shared Transcripts" help page not opened; Cursor Origin pages not opened; Cursor commit signing is a vendor statement.
- Hermes PR reviewer and webhook guides not opened; Kanban PR completion contracts claim comes from the llms index.
- OpenClaw: no built-in commit/PR/review flows found; CHANGELOG not opened; the managed-worktrees page text was truncated (30k chars unread), the survey's numbers were confirmed in review.
- goose: `goose review` flag semantics; `--no-orchestrate` implies an unconfirmed orchestration default.
- Codex: `/export` and `/rename` details and the exact CLI flags (ConfigTable components do not render in the `.md` twins).
- Claude Code: changelog-derived version and week claims (for example the `/fork` rename history) were not independently verified; "Resume across runners" is beta and was not checked by the reviewers.
- Claude Desktop: the session sidebar, worktree defaults, PR monitoring and diff review were not checked in review (the author's text only).
- ACP: `session/fork`, `session/notices` and others are RFD titles only.
- Hermes `/developer-guide/gateway-internals` citation for crash behaviour: copied as the survey writes it (the evidence-path convention in the Hermes file is relative).

### Extensibility (area 13)

From [round1/synth/06-extensibility.md](round1/synth/06-extensibility.md).

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

### Models and observability (areas 14, 19)

From [round1/synth/07-models-observability.md](round1/synth/07-models-observability.md).

Changed by review (the corrected text is used above):
- **Codex.** The four model descriptions are confirmed but the page names no default model; the claim that GPT-6.1 Sol became the bundled default in 0.161.0 was not confirmed. "Responses API only, `wire_api = "chat"` unsupported" is on `models.md`, not the config page. The 300 s figure is an example `refresh_interval_ms`; 0 means refresh only after an auth retry. The project-config blocklist also includes `apps_mcp_product_sku`. The Speed page still cites GPT-5.6 beside GPT-6 (stale).
- **OpenCodex.** The account-pool selection wording ("round-robin or fill-first") was not on the dashboard page; the page describes a selection order and lowest-usage choice for new sessions. The "256 MiB memory budget" was removed (not found). Client support lists 28 clients, not about 25. Pairing code lifetime is five minutes. "Remember device" is a 30-day record expiry, off by default. The default-on telemetry claim (v2.100.0) comes from an Atom release feed the reviewer did not reopen.
- **Claude Desktop.** The 3P overview carries no GA label ("GA" is an inference); the list of unavailable features (computer use, cloud sessions, Remote Control) comes from other pages, not the overview.
- **DeepSeek harness.** The credential file location and "keys are write-only" are on the providers guide, not the credentials subsystem page, which gives no file location, permissions or encryption. The two telemetry sources differ in tone (subsystem page: no end-user opt-out described; CLI reference: `FEEDBACK_ONLY` default).
- **Cursor.** The Legacy Privacy Mode statement is on the cloud security page; the privacy page confirms Anthropic harm-review retention for Fable models.
- **Claude Code (review-2 and review-3).** Model flags, default model and effort, fallback chains, content-based fallback, advisor, fast mode, model allowlists and `/usage` cost semantics were confirmed. Routine naming and several non-model statuses were corrected (outside this group). Review-1 notes that no entry says which model summarises during compaction, though fallback chains apply to compaction.
- **Oh My Pi.** README lists nine roles; docs list ten (adds `designer`). README claims round-robin credentials; docs/providers does not describe it. "Default thinking level `high`" is stated on docs/settings, not docs/roles.
- **Pi.** Agent retry defaults confirmed (3, 2000 ms, 60000 ms). "15+ providers" on the homepage confirmed, versus more than thirty on the docs page.
- **goose.** ACP providers and the deprecated CLI providers confirmed; the troubleshooting binary is `claude-agent-acp`, not `claude-acp`.

Contradictions between a vendor's own pages:
- **Claude Code.** `fast-mode.md` still says fast mode is unavailable on Google Cloud, a day after 2.1.322 added it on Vertex. Default-model advice (Sonnet for most work) contradicts every plan defaulting to Opus 5.5; `costs.md` describes thinking budgets that `model-config.md` says current models ignore. A group spend cap is "not a shared pool" on one page while the config reference documents `pooled_budget`. Auto mode is "available on every provider" on one page and model-gated on another. Enterprise cost averages ($13 per developer-day and similar) are vendor claims without method.
- **Antigravity.** Plans page says no BYOK or custom endpoints, yet CLI settings list `modelProvider`, `customModelsConfig` and `GEMINI_API_KEY`, and the SDK supports Gemini keys, Vertex and local servers. Effort: `max` in the CLI reference versus low/medium/high elsewhere.
- **goose.** In-session page says the CLI cannot change model via a setting; the CLI reference lists `/model`. README says 15+ providers; docs list well over 40. The multi-model page promises dynamic switching strategies but documents only `/model` and per-sub-agent models.
- **Hermes Agent.** Sub-agent iteration cap is 50 on one page and `delegation.max_iterations` 250 on another.
- **Zed.** Remote development page is contradictory on where model calls originate.
- **Pi and goose** home pages understate provider counts.

Unverified or only vendor claims:
- **Oh My Pi** `omp stats` dashboard contents (no docs page; `/docs/stats` returns 404). Provider terms of service for subscription logins (docs silent).
- **OpenCode** telemetry behaviour of the local app; product page "100% free" and "data stored only for free models" are vendor claims.
- **OpenHands** LLM routing, fallback and subscriptions pages (index only); telemetry contents.
- **Hermes Agent** "spend limits that YOLO cannot skip" appears in a distinctive-features bullet but no entry describes it; credential-pool page not opened.
- **DeepSeek harness** token cost display and usage UI; trace view is a landing-page claim.
- **Cline** ClinePass "2-5x" is a vendor claim; provider list taken from nav titles; OpenTelemetry events and env pages not opened.
- **Cursor** Analytics and OTel details beyond index titles; BYOK page is brief.
- **Warp** BYOK, pricing and analytics API entries were not checked in review; model-choice pages at one path returned 404 (real path `/agents/inference/model-choice/`). Whether local-run model calls always go via Warp's servers is not spelled out.
- **Zed** hosted-model list and per-token prices page not opened; several ACP RFDs (usage, compaction, custom LLM endpoint) are title-only.
- **Claude Code** most provider and gateway entries (Bedrock, Vertex, Foundry, gateway upstreams, spend limits, OTel) were outside review-2's 47 checked entries; they are unreviewed, not wrong. Changelog-derived version and date claims were only partly reachable by reviewers.
- **OpenCodex** image and video bridge guides and the full provider matrix were not opened.

---

### UI, browser, collaboration (areas 15, 16, 18)

From [round1/synth/08-ui-browser-collaboration.md](round1/synth/08-ui-browser-collaboration.md).

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

### Remote execution and architecture (area 17)

From [round1/synth/09-remote-distributed-architecture.md](round1/synth/09-remote-distributed-architecture.md).

**What the reviews changed (used corrected above).**

- **Claude Code work orders.** Not one per session: in orchestrator (on-demand) mode each spawned runner gets a single-use work order that registers exactly one runner and expires with the spawn lease (default 120 s, `--expected-spawn-seconds`); sessions run as child processes of their runner. The survey's section 3 and section 5 bullets say "single-use work orders" per session; review 3 calls that sentence wrong (claude-code.review-3.md, corrections 4, 5, 9).
- **Claude Code Remote Control is not lease-based.** It polls outbound, but no lease is documented for it; only self-hosted runners refresh a lease by polling. "Outbound HTTPS only" is also short: runners and sessions use SSE and, with the git proxy, a WebSocket, all outbound (review 3, correction 4).
- **Claude Code control-plane policy delivery.** Confirmed and wider than the survey said: tool list and permission rules come "with the work payload"; model, connectors (as MCP config), lifecycle hooks and API endpoint also come from the control plane; the runner image's `managed-settings.json` applies too; on Bedrock/Agent Platform runners server-managed settings do not reach sessions, so the image file is the only enforcement. stdin carries token rotation and end signals, fd 3 carries activity signals (review 3, corrections 5, 6).
- **Claude Code cloud network secrets** use no sentinel: the key is never in the VM and the proxy attaches it. Only local sandbox credential masking uses sentinel substitution, and that needs experimental TLS termination (review 3, correction 3).
- **Claude Code cloud scheduled tasks** are renamed from "routines"; minimum interval one hour; the "skipped for up to 72 hours then turns off" clause could not be found (review 2 correction 5; review 3 correction 7).
- **Claude Code legal constraint** (section 5 bullet): the restriction on offering claude.ai login has a carve-out: an end user may sign in to the unmodified `claude` binary with their own subscription, including where a platform hosts Claude Code. So a node could drive an unmodified `claude` binary the user signs in, but rizzy-harness may not offer claude.ai login itself (review 3, corrections 1, 2).
- **Claude Desktop SSH sessions.** The claim that tasks survive sleep, network loss and the app quitting was removed; the remote login is renewed only while Desktop is connected and the docs say not to count on overnight runs. The survey's section 5 bullet ("the SSH helper that survives app exit") was not updated and is stale. Trusted Devices is beta (claude-desktop.md, section 9).
- **Claude Desktop Dispatch.** Limited beta, not available to new users; the 30-minute approval window is from the code.claude.com desktop page, not the support article.
- **OpenClaw cloud workers.** Status is beta (scorecard M3 Beta, quality Alpha 67%), not GA; the page says the worker "dials" the Gateway's public worker route and never says "outbound-only" or "WebSocket"; 4 h / 45 min is the suggested `cloud` profile, not a universal default. The survey's section 5 still says "outbound-only connection" (stale). Node pairing: the standalone node pairing store left the connect path in January 2026, `node.pair.request`/`node.pair.verify` were removed in 2026.7 (openclaw.md, section 9).
- **OpenCodex.** Hub pairing code lifetime is five minutes; the Remote Workspace rate limit is ten failed codes in ten minutes answered with 429; "runs no agent loop" is a reading of the pages, not a quoted claim (opencodex.md, section 9).
- **DeepSeek harness.** No page states that the agent loop and web server share a process (inference). The SSH page is not missing: it is in the repository at `docs/subsystems/ssh.md` and says the harness stays local and SSH loss is final.
- **OpenHands CLI** is "no longer actively maintained", not formally deprecated; the `openhands serve` GUI is deprecated.
- **Codex.** GitHub Action also offers `read-only` and `unsafe` safety strategies; Handoff does not say "to a worktree".
- **Cline Kanban.** The page does not flag the CDK example's root service or token in user-data; those are the survey's inferences. Hub roles are per session participant, not per client.
- **Warp.** `oz` is "a supported legacy interface" on the oz-cli page but "current" on the transitioning page; handoff "blocked while a command runs or child agents are active" was not found; key expiry options not confirmed on the cited page.
- **Hermes Agent.** Gateway ledger, breaker, watchdog and loop guard are documented on `/user-guide/messaging`, not on the gateway-internals page cited; iron-proxy binds the Docker bridge on Linux and loopback on Docker Desktop.
- **Oh My Pi.** The auth broker hands out API keys as well as access tokens; loopback default, no TLS.
- **goose.** The "no sandbox documented" claim is not supported (a macOS Desktop sandbox page appears in search results but 404s); reviewer advises a second look at every "nothing documented" in that survey.

**Where a vendor's own pages disagree (in these areas).**

- Codex: the app-server is "experimental and isn't supported for production workloads" while it is the only replacement for the removed MCP server and the stable Python SDK is built on it.
- Cursor: worker limits are 10,000 per user and team (self-hosted page) versus 200 per user and 1,000 per team (pool page); cloud retention "deleted after run" (privacy page) versus 30-day VM snapshots (security page).
- Claude Code: `self-hosted-environments-deploy` recommends `--capacity 1` while its own recipes use 4.
- Zed/ACP: the remote development page says the local machine talks to LLMs and also that the remote uses its own proxy for LLM calls; the ACP home page says remote support is in progress while goose already ships ACP over HTTP and WebSocket.
- Warp: `oz` legacy versus current.
- OpenCodex: README lists Docker Compose while the hub page says no official image is published.
- OpenHands: the Enterprise lifecycle page says idle timeout leads to PAUSED and to MISSING.
- goose: docs say `goosed` in logs and deeplinks while the remote guide and CLI reference say `goose serve`.

**What nobody could confirm.**

- Wire protocols: Claude Code Remote Control (SSE or WebSocket, schema), Cursor's worker protocol and encryption (docs say it is not documented), Codex's "secure relay layer", Antigravity's relay and encryption, Claude Code runner-to-child message format.
- Codex remote executors and `codex exec-server`: what they are and how they authenticate.
- Cloud VM isolation technology for Claude Code (microVM, container or VM) and Codex Cloud.
- Whether Warp's local (non-cloud) model calls always pass through Warp's servers.
- Whether Hermes Agent's `hermes serve` is separate from the gateway process; Hermes Agent and OpenClaw idle memory footprint (relevant to our 50 MB invariant only by comparison).
- OpenCodex's `ocx-child` relay and "zero-preinstall SSH setup" relation to Remote Workspace.
- Pi: whether the coding-agent CLI integrates with pi-durable and pi-env at all.
- Oh My Pi: whether `bash` runs remotely over SSH.
- Claude Code: "self-hosted runner public beta from 2.1.224 (Aug 7, 2026)" (the public-beta status itself is confirmed).

### Configuration, licence, other (areas 20, 22, 23)

From [round1/synth/10-config-licence-distinctive.md](round1/synth/10-config-licence-distinctive.md).

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
