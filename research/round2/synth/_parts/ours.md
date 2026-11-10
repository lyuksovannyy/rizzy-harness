### The agent loop, tool dispatch and durability

From [round2/synth/01-loop-dispatch-durability.md](round2/synth/01-loop-dispatch-durability.md).

Our state, from `research/round1/own-rizzy-harness.md` (as of `main`): one native loop in `crates/agent-native/src/lib.rs`; a fixed cap of 50 model calls per turn, not configurable; tool calls run one after another (only `delegate` runs in parallel); a prompt sent while a turn runs is rejected ("a turn is already running", `crates/node/src/session.rs:297-299`); a restart mid-turn closes the turn as `interrupted`, denies pending approvals and closes interrupted tool calls synthetically (`session.rs:154-191`); 600 s per tool call; no loop detector; no provider retry (pull request 7 adds it); cancel does not abandon a tool future that ignores the token (fixed only on `integration/gate3`); routines run agentic only in `ask` or `ask_all`. Feature freeze applies until Gate 3 of `FIX_PLAN.md`: items marked "fix" below are repairs of the loop's safety; items marked "feature" cost against the plan and need the owner's decision to break the freeze.

Ranked by importance. Each point is stated as a verdict.

##### 6.1 Do: make the order "append, flush, then act" a tested rule (fix)

- **Copy from DeepSeek harness** (`packages/session/session-checkpoint-policy/src/index.ts`, `packages/core/agent-loop/src/index.ts` `interruptedTurnClosers` in `packages/core/session/src/repair.ts`): an awaited, fail-closed flush before each model request and before each top-level tool body; a failed flush means the tool never runs.
- **Check in our code:** is the event that records "tool call X with arguments A is about to run" committed (not just sent down the node link) before the tool body starts? Our log is written by the node's session actor; the master persists via the link. Name the exact point (the `approval_requested` or the tool-start event in `session.rs` around `:955-1012` and the loop call site `agent-native/src/lib.rs:744-870`). If the append is queued behind the back-pressure channel (pull request 4's bounded link) the flush rule is false.
- **Test that fails first:** kill -9 the node while a `bash` call is running; assert the log holds the call's arguments and, after restart, a closing event.
- **Closing wording:** copy DeepSeek's synthetic result ("outcome is unknown ... retry only if read-only or idempotent ... do not retry blindly") and OpenClaw's restart text (check files, processes and external actions before repeating). Our synthetic close already exists; check that it says "unknown", not "cancelled".
- **Avoid:** Cline's whole-file transcript rewrite with no atomic rename, and Pi's "persist only on `message_end`" (a crash loses the partial assistant message and leaves a dangling call). Our append-only events are ahead of both.

##### 6.2 Do: a default-on repeated-call guard, checked before dispatch (fix)

Evidence that the gap is real: Codex and Pi have none and say so; goose's is dead code; OpenCode has none outside `/goal`.
- **Copy from Hermes** (`agent/tool_guardrails.py`): tool plus arguments hash, a result hash (so a changing timestamp does not hide a loop, as OpenClaw masks volatile tokens in `tool-loop-detection.ts`), warn at the 3rd repeat, block at 5, and `hard_stop` on for routines and off for attended chats. Hash arguments so the guard cannot leak them.
- **Copy from OpenClaw** (`tool-loop-admission.ts`): decide at admission, before the call runs; if the guard itself errors, block. Cline runs its detector from `tool-started`, after the call was issued: do not do that.
- **Copy from Cline** (`mistake-tracker.ts`): a turn where every tool call failed is a "mistake"; any success resets it.
- **Where the state lives:** derive it from the event log fold, not an object local to the loop (goose's `RepetitionInspector` counts on a clone and never accumulates). The node can then rebuild it after a restart.
- **Do not copy** DeepSeek's notice-only reminder as the only guard: it never stops a loop. Add it as the warning stage, with the block after it.
- **Test:** a scripted provider issuing the same failing `read` call; assert the 5th call is refused with a visible reason and the 3rd carried a warning.

##### 6.3 Do: step, time and tool limits in `node.toml`, enforced in the dispatcher (fix)

- The cap of 50 is hard-coded; the node is the authority, so move the cap and a per-turn wall-clock into `node.toml` `[limits]` (today only `tool_stream_bytes`) with defaults, as Hermes does for routines (a finite default per routine) and goose does with `GOOSE_MAX_TURNS` plus a half-budget `<turn-budget>` hint.
- **Copy from OpenCode** (`session/runner/llm.ts`, `step.ts`): at the cap, keep tool schemas in the request, set `tool_choice: none`, append an instruction, and refuse any late tool call at dispatch. It keeps the provider cache and is enforced, not requested.
- **Enforce each tool's deadline in the dispatcher, once** (Cline's `timeoutMs` metadata is never read; goose lets the model set `timeout_secs` and 0 disables). Clamp any model-supplied timeout to a `node.toml` maximum; never let 0 mean infinite. Our bash cap of 3600 s is the model-visible ceiling: check it is a node setting.
- Prefer idle limits to total limits for the provider stream (OpenClaw 120 s, Codex 5 min, OpenCodex 300 s), which pull request 7 should set.

##### 6.4 Do: never run tool calls from a response that stopped on the length limit (fix)

Pi fails every tool call in a `length`-stopped message ("was not executed: the response hit the output token limit, so its arguments may be truncated") because streamed arguments are completed by a JSON salvage parser (`packages/agent/src/agent-loop.ts` L273-L279, L488-L513, checked in review). OpenCode separately fails the step after three consecutive `length` stops. A truncated `write` or `bash` argument is a data-loss and safety hazard.
- **Check in our code:** how does `agent-native` assemble arguments from stream chunks, and what does it do with a `length` finish? Test: a provider fixture ending a `write` mid-JSON.

##### 6.5 Do: provider retry rules (pull request 7), from Codex and others (fix, already in flight)

- Copy Codex's single error-to-policy function (`protocol/src/error.rs` classification; `core/src/responses_retry.rs`): never retry context overflow, auth, model-not-found, quota; server advice before local backoff; `Retry-After` cannot extend the configured count.
- **Add the rule Cline and Oh My Pi have and Codex only implies:** never retry a turn after it already streamed visible output, or after a tool ran ("no replay-unsafe committed output unless every tool call provably never ran", `turn-recovery.ts`). Check that PR 7's retry cannot re-run a step whose tool calls were already issued.
- Cap provider-requested waits (Oh My Pi `maxDelayMs` 300 s): a three-hour sleep in a node is a stuck session.
- OpenCodex's rule fits master-side proxies: charge retries to the logical request, not each hop.

##### 6.6 Do: fence every settle with a claim generation (fix, ties to the nodes design note)

The invariant is "events are appended only by the owning node's session actor". Nothing in the files read shows a design that survives a stale node better than Hermes: `claim_session_input` bumps a per-session `runtime_generation`; `settle_session_input` accepts only a matching epoch and generation (`hermes_state_runtime.py` L206-L274); at start a global epoch is incremented and every `started` row of an earlier epoch becomes `unknown` and is never requeued (L453-L464). OpenClaw's `activeWriterRunId` compared inside the write transaction (`session-accessor.sqlite-scope.ts` L79-L97) does the same at the storage level, but is skipped when a write carries no expected writer: make the unfenced path impossible. OpenCode has no fence and says its sweep "assumes every orphaned claim's owner is dead"; Codex and DeepSeek use file locks that only protect one host.
- **For us:** when a workspace node reconnects or a session is moved to another node (the design note's worktree-per-session and node-per-workspace), give each claim a generation; the master rejects an append that names an old one. Test: two nodes append to one session; the older one is refused and the log stays contiguous.
- Also copy the OpenClaw lost-worker message: "tool effects may be unknown"; never auto-continue a turn after a lost node.

##### 6.7 Do: unattended runs close themselves and record why (fix, routines)

- **Copy from Hermes** (`gateway/session_unattended_unknown.py`, `cron/scheduler_tick.py`, `cron/scheduler_authority.py`): advance the routine's next-run time before dispatch (at-most-once), write an idempotency key for each fire before it runs, pause the routine when the outcome is unknown, and settle a crashed routine turn as `interrupted` with a reason so the next run is not blocked; a person's own crashed turn waits for a decision. OpenClaw's cron retry has no attempt counter (a run that crashes again before delivery qualifies again): store an attempt count on the receipt.
- **Default for unattended:** keep our "routines run agentic only in `ask` or `ask_all`" rule; Hermes' `cron_mode: deny` is the same idea. Do not follow Oh My Pi, Cline or goose (default `yolo` or `Auto` for scheduled runs).
- **Return the real refusal to the model** on unattended runs (Warp tells the model "denylisted" for every denial, which misleads it).
- Skip-not-burst on catch-up (OpenCode, DeepSeek's latest-only) and an explicit overlap rule (goose lets a job overlap itself).

##### 6.8 Do: a dispatcher-level ceiling on every tool result (fix, small)

Pi enforces truncation per built-in tool; an extension tool returning 10 MB goes straight into context. Our shell filter and the 2 MB `tool_output` store exist; pull request 4 adds capped file tools. **Check:** `web_fetch` (2 MiB body cap, 20,000 characters), MCP results and any future tool pass through one cap in the dispatcher, ideally proportional to the context window (OpenClaw: 30% of the window and 16k, 32k or 64k characters), with a recoverable handle (Cline's `cline://cache` URI; our `tool_output` id is the equivalent). Cap while reading, not after (goose holds a gigabyte in memory first; our idle RSS budget makes this mandatory).

##### 6.9 Do: cancel and orphan processes (fix; mostly gate3)

- Cancel semantics to copy: Goose's "every open tool call gets a terminal result" on cancel (`machine.rs` L250-L285); OpenCode's cooperative cancel, grace, then interrupt, and its "decline is a defect, feedback is a failure" split so a tool cannot swallow a refusal (`model-request.ts`, `permission.ts`).
- Kill: Oh My Pi re-walks the process tree between SIGTERM and SIGKILL (`proctree.rs`); a cgroup or pid namespace is stronger (goose and Cline children that call `setsid` escape a group kill).
- **Node crash leaves children running** (goose: shells get no `PR_SET_PDEATHSIG`). Check what our node does with a `bash` child when the node is killed; with the Landlock launcher in pull request 4, test that a restarted node does not leave an earlier shell running against the same worktree.

##### 6.10 Decide, feature (freeze): input during a turn

Today: reject. That is safe; no accepted input is lost. If the owner breaks the freeze, the evidence says:
- Persist the message as an event or row with an idempotency key before acknowledging (Hermes `session_admissions`, OpenClaw `session_pending_inputs`, DeepSeek's `requestId` in the logged message). Pi's and OpenClaw's in-memory queues do not survive a crash.
- Deliver at a step boundary and never kill a running tool (Codex, Pi, Oh My Pi). In parallel batches this waits for the batch; if we stay sequential it is fine. Make interrupt a separate explicit action.
- When a queued message is dropped, write an explicit receipt (OpenClaw).
- Avoid the OpenHands shape: a message written to a socket the server has not subscribed, or held in browser memory, can be lost; the node should append the user message to the log before any "run" decision.
- Cost against the plan: a new event type, a node protocol message and UI state (not in `Shell.tsx`). I would not do it before Gate 3.

##### 6.11 Avoid

- Per-tool authorisation (OpenCode `permission.assert`, Cline per-tool validation after approval): one forgotten check is an unchecked tool. Our dispatcher should authorise once, with tools declaring resources.
- Validation after policy (DeepSeek): validate arguments before policy so a reviewer sees a normalised shape, and re-validate after any hook mutates them (Pi does not).
- Tool results as user-role text (OpenHands client tools): results are events written by the owning node.
- Persistence as an option (OpenCode's durable log is off in the shipped server): our log is the truth.
- Parallelism by name ("read-only"): Codex's lesson is that a shell command can write. If we add parallel execution, key it on declared effects, serialise authorisation and parallelise execution (Pi, Cline), and keep shell exclusive.
- Resume cost growing with age: DeepSeek reads the whole log (`read(0)`). Codex's rule (latest full-history compaction is a base, replay only the suffix) makes resume independent of session age; our fold needs the same checkpoint record. This ties to pull request 7's compaction.

##### 6.12 Relation to open pull requests and the design note

| Point | main | PR 4 (sandbox, bounded link) | PR 6 (agent roster) | PR 7 (retry, compaction) | Design note |
|---|---|---|---|---|---|
| 6.1 append, flush, act | needs the check above | bounded channels could delay appends; verify | none | none | node-per-workspace: the writer is still the node |
| 6.2 loop guard | missing | none | per-agent limits could carry thresholds | retry must not mask a loop | none |
| 6.3 limits in `node.toml` | cap hard-coded | `[limits] tool_stream_bytes` exists | agents already carry `max_subagents` | idle timeouts belong here | per-turn grants: key lifetime equals turn |
| 6.4 length stop | unknown | none | none | natural home | none |
| 6.5 retry | none | none | none | in flight | none |
| 6.6 fencing | one node per session | none | server-side enforcement of where an agent may run | none | needs a claim generation when sessions move |
| 6.7 routines | ask-only | none | none | none | deletion flow must stop unattended runs |
| 6.8 truncation | filter plus store | capped file tools | none | none | none |
| 6.9 cancel | partial (gate3) | Landlock child; check orphans | none | none | none |

### Approvals and permission rules

From [round2/synth/02-approvals-permission-rules.md](round2/synth/02-approvals-permission-rules.md).

##### Where we stand

- **Main branch.** Three modes: `ask_all`, `ask` (default; asks for mutating tools) and `auto`. The node clamps a request with `min(requested, node max)` (`crates/core/src/policy.rs:19-23`). There are no per-command rules and no persistent "always".
- **How a request flows.** A request is an `approval_requested` event carrying `{approval_id, call_id, tool, input, reason}` (`crates/core/src/event.rs:446-452`). The node makes `approval_id` with `new_id()` (`crates/node/src/session.rs:958`, `:979`). The answer is `approval.respond {req_id, session_id, approval_id, decision}`, where `decision` is `Allow | Deny | AllowSession` (`crates/protocol/src/lib.rs:217-222`, `crates/core/src/policy.rs:42-47`). Restart or cancel denies anything pending.
- **"Allow for this session".** It is keyed on the tool name and lives for one `run_turn` (AUDIT L10, `crates/agent-native/src/lib.rs:625`). One click therefore allows every later `bash` command in that turn. PR 4 (commit `0d5cd95`) narrows this: the allowance is honoured only after a first approval of that tool in the turn, and it is recorded node-side. It still keys on the tool name.
- **Delegation.** It uses the same approval mode, with depth one.
- **Not built yet.** We have no hooks, no plan mode and no model approver (PLAN only). PR 6 adds server-side enforcement of where an agent may run. PR 4 adds a fail-closed Landlock sandbox for the shell.

By the evidence above, our answer sits at level 2 of 4.3: an opaque id. Our "always" sits at level 4: a tool name. Both are below Codex and OpenClaw.

##### Ranked recommendations

**1. Bind every approval to an action digest, and make the answer name it.**

This is the single most important change. It is a protocol and event change, and it is security code, so it gets the full stress test and an abuse test.

What to do:

- **Compute a digest on the node when the request is made.** `ApprovalRequested` gains `action_digest`: a SHA-256 over a length-prefixed canonical encoding of:
  - session id, node id, call id;
  - tool name;
  - the canonical input JSON (for `bash`: the exact command string, plus the parsed argv per segment once item 5 exists);
  - cwd, by realpath;
  - the names and hashes of any environment beyond the allow-list;
  - the sandbox profile and any requested permission delta;
  - a hash of the node policy snapshot.
- **Make the answer echo it.** `approval.respond` carries `approval_id` and `action_digest`. The node rejects an answer whose digest differs from the pending request.
- **Use each approval once.** The node consumes it on first use and rejects a replay.
- **Re-ask if policy moved.** If the policy snapshot changed between request and answer, the node asks again. Copy OpenClaw's "policy snapshot changed, approve again" check: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L349-L357
- **Check who answers on the master.** The master accepts an answer only from the chat's owner (chats are private). Copy OpenClaw's Gateway replay guard (`sanitizeSystemRunParamsForForwarding`, `consumeAllowOnce`): https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-invoke-system-run-approval.ts#L197-L252
- **Bind executables the way OpenClaw does.** Bind the executable's real path, add a content hash when the file is writable by the user, and bind cwd by inode: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L366-L458
  OpenClaw leaves a window between hashing and exec. Close it by hashing from an open file descriptor and executing that descriptor. Codex does this for its bundled bubblewrap: it checks the digest, then execs through the fd (codex.safety.md review row "Which bwrap"). Whether this works without `unsafe` in our crate set is my own open question, not a finding.
- **Show what is being approved.** The UI renders the full input. The card stays deny-only until the whole command has been shown: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_gateway_wait.py#L182-L188
  That card is [unchecked]; build it from the idea, not the code. Redact secrets in the display only, never in the digest input.

What to avoid:

- OpenHands `{accept}`, Cline "any client by `approvalId`", DeepSeek harness `{clientId, eventId, outcome}` and OpenCode "any password holder". All four answer an id, not an action.
- OpenClaw's empty approver list meaning "anyone in the chat" (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/approval-auth-helpers.ts#L72-L89). Ours must mean "the owner only".

The node keeps the last word. An approval can only turn an `ask` into a run. It can never turn a `node.toml` deny into a run. That is the opposite of OpenClaw's `approvedByAsk`, where a forwarded approval satisfies the node's own allow-list: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117

Tests, written first:

- an answer with a wrong digest is refused;
- a second answer to the same id is refused;
- an answer from a non-owner is refused at the master, and the node never sees it;
- a policy change between request and answer forces a new request;
- a `node.toml`-denied tool stays denied after an `Allow` answer.

**2. Redefine "always", and keep it narrow.**

Replace `AllowSession`-by-tool-name.

- **"Allow this again in this session."** This is the only remembered decision the UI offers by default.
  - Keyed on Codex's session key: canonical argv, cwd and permission delta. Read `UnifiedExecApprovalKey` and the cache check: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/runtimes/unified_exec.rs#L87-L98 and https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/sandboxing.rs#L72-L117
  - Held on the node and written as an event, so a fold over the log rebuilds it after a restart.
  - Ends with the session, and never crosses into a child session.
  - Never cached for network access, permission escalation, or future MCP tools. Codex leaves these three out too.
- **Persistent "always" is a rule, never a click.** It may exist only as an owner-written rule in a node-side rules file next to `node.toml`. The UI can propose the exact rule text (Codex's "smart approvals" proposal has a ban list and must cover the whole command: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L1065-L1127). The owner must see and confirm the exact rule.
- **Limits on any rule:**
  - It is scoped to one project root on one node.
  - It is never a first-word prefix for wrappers or interpreters: `sudo`, `env`, `bash`, `sh`, `zsh`, `python*`, `node`, `perl`, `ruby`, `xargs`, `find`, `git -c`. Compare OpenCode's `sudo *` and `bash *`: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/shell/parse.ts#L603-L609
  - It never lifts the sandbox. Codex's `Skip { bypass_sandbox: true }` is the counter-example (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L492-L512). Leaving the Landlock profile of PR 4 is a separate `node.toml` grant.
  - It is never written by the agent. Our file tools already refuse the data directory. Add the rules file to the always-prompt class, as Zed does for `.zed`: https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/thread.rs#L6218-L6235
  - No repository file can add or loosen a rule. Oh My Pi (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L2800-L2814) and OpenCode are the counter-examples.
- **Avoid permanent global grants.** That rules out Zed's prefix regex written to user settings (https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/pattern_extraction.rs#L53-L95), goose's bare tool name, and Hermes Agent's class key in agent-reachable config.

Per-command rules are a feature, so they wait for Gate 3. The session-key replacement for AUDIT L10 is a fix and does not wait.

Tests:

- approving `git status` does not allow `git push`;
- approving in turn 1 does allow the identical argv in turn 2 of the same session;
- a child session does not inherit the parent's grant;
- a restart rebuilds the grants from the log.

**3. Make these failure modes fail closed, each with a test.**

| Failure | Required outcome | Counter-example / model |
|---|---|---|
| Policy or rules file fails to parse | The node refuses to start, or refuses agentic sessions. Never drop the other rules silently. | Codex `load_exec_policy_with_warning` drops every user and project rule: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L729-L744 |
| Unknown mode or setting value | Treat it as the strictest value. We already reject unknown keys; also reject unknown values. | Oh My Pi falls back to `yolo`; Cline cron turns unknown modes into yolo. |
| Shell parse failure or unanalysable construct | No auto-allow: ask. Deny if unattended. | Zed `from_input`, OpenCode `parse.ts#L162-L171` (model); Cline plan guard (counter). |
| No human reachable (routine, link down, no UI) | Deny. Never "nobody is here, so approve". | Hermes Agent `-z` (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1300-L1308), goose scheduled `Auto`, Cline cron (counters); OpenClaw node ask→deny (model). |
| Approver callback missing or throws | Deny. | Cline `prepareToolExecution` (model): https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/agents/src/agent-runtime.ts#L2539-L2571 |
| Audit event for ask or decide cannot be appended | Reject the request. Our one-writer rule applied to approvals. | DeepSeek harness `user-approval` (model): https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/user-approval/src/index.ts#L224-L261 |
| Restart or cancel with an approval pending | Deny, and the old approval can never run later. We already do this. | goose lease expiry (model): https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L999-L1039 |
| Model approver errors, times out or returns invalid output | Escalate to the human. Deny when unattended. | OpenClaw, Hermes Agent (models); goose adversary and classifier (counters). |
| A future permission hook crashes, times out or returns bad output | Block the call (not the whole run). | Oh My Pi runner (model): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/runner.ts#L2338-L2364 ; Codex, Cline, goose, OpenHands (counters). |
| Any review layer errors | Record which layer decided, and never drop the layer. | goose skips a failing inspector: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_inspection.rs#L84-L118 |
| Sandbox unavailable | Refuse. PR 4 does this with `[sandbox] mode = required`. Keep it. | Zed macOS (model); Zed Linux "run unsandboxed for this thread" prompt (counter). |

Return the real reason to the model and to the UI. Warp reports every refusal as "denylisted", and goose reports every denial as "the user declined".

**4. Put one gate at the dispatch boundary, and route every path through it.**

Our loop checks before dispatch. Keep it that way, and write a test per path. The paths that must reach the gate:

- every tool;
- `delegate`;
- any future code-mode or scripting tool, call by call (goose Code Mode is the counter-example; OpenCode `codemode/tool.ts#L126-L145` and Pi's re-entrant `runToolCall` are the models);
- any future MCP tool;
- any user-typed command (Pi `!` is the counter-example);
- any template (OpenCode `` !`cmd` ``).

No execution endpoint may exist outside a session. OpenHands' server-level bash is the counter-example: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/bash-service/bash-service.api.ts#L24-L44

Validate the arguments before authorising them (Pi). Re-authorise after anything rewrites them (Oh My Pi `ExtensionToolWrapper`): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L355-L374

Authorise calls one at a time even when they run in parallel. Read Pi's `executeToolCallsParallel`: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L596-L670

**5. If per-command rules come (after Gate 3), parse with a maintained parser.**

- **Allow rule:** it applies only when every simple command matches and the parse contains no unanalysable node: substitution, process substitution, `eval`, heredoc, a function definition, or a redirect to a path outside the session folder.
- **Deny rule:** it matches any segment, after unwrapping `sudo`, `env`, `bash -c`, `sh -c`, `xargs`, `nohup`, `timeout`, `command`, `exec` and absolute paths. Copy Hermes Agent's per-segment executable projection: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval_detection.py#L1623-L1664
- **Splitting:** read Codex's node-kind allow-list (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/shell-command/src/bash.rs#L29-L98) and Zed's `extract_commands` (https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/shell_command_parser/src/shell_command_parser.rs#L559-L650). Zed treats redirects as pseudo-commands.
- **Parser crate:** `tree-sitter-bash` or `brush-parser`. Both are crates, so they need the dependency check: licence, age and `cargo deny`.
- **What we only run is bash.** Never parse PowerShell as POSIX (Zed).
- **Test:** one test per refused construct.

Rules are a convenience layer. The Landlock sandbox from PR 4 stays the containment. Do not repeat DeepSeek harness's "sandbox instead of rules" while reads and network stay open, and do not trust rules to contain anything.

**6. The model never lowers its own gate.**

- **Never let a model flag skip a check.** No `is_risky`, `security_risk`, `requires_approval`, `readOnlyHint` or `approvedBuilds` style field may skip an ask. Warp and OpenHands are the counter-examples.
- **Model-requested escalation must:**
  - carry a non-empty reason;
  - pass the normal check first;
  - be approved by a human once, bound by item 1;
  - be reported in the tool result when it ran with wider rights.
  Read Zed's `terminal_tool.rs` (https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/terminal_tool.rs#L678-L705) and DeepSeek harness's `approveEscalation` (https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sandbox/sandbox/src/escalation.ts#L172-L206).
- **An escalated retry is a new approval.** Copy Codex's rule that a reviewer approval never pre-approves an escalated retry: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/orchestrator.rs#L491-L501

**7. Keep every credential that can answer an approval away from the agent.**

- **What we already do.** Our environment allow-list (main, extended in PR 4) keeps the master session cookie, node keys and provider keys out of `bash`.
- **Add a test.** No variable in the child environment can authenticate to the master or answer `approval.respond`.
- **The node cannot answer.** The master accepts an answer only from a browser session of the chat owner, never from the node link. A prompt-injected node session therefore cannot approve itself.
- **Counter-examples:** OpenHands (https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985) and the OpenCode server password.
- **Design note.** It grants provider keys to nodes per turn. Keep those grants from ever carrying master authority.

**8. Sub-agents inherit a ceiling, not a copy, and never ask for themselves.**

- **How children work today.** Our children use the same approval mode, with depth one.
- **Ceiling.** Make the child's ceiling the parent's effective policy at every call, not a creation-time copy (OpenCode's copy is the counter-example: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L311-L314).
- **Roles narrow only.** An agent definition is a patch type that can only narrow. Copy Codex `role.rs` and `child_config.rs`: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/child_config.rs#L197-L218
- **Prompts reach the parent's owner.** A child's approval prompts go to the parent chat's owner, as goose does (`summon.rs`, `subagent_handler.rs`), carrying the item-1 digest.
- **Unattended children deny.**
- **Grants do not travel.** "This turn" and "this session" grants do not pass between parent and child in either direction.
- **Counter-examples:** Oh My Pi forced `yolo`, Cline's policy-less children, and Warp and DeepSeek harness foreign children with safety off.
- **PR 6.** It enforces where an agent may run on the server. Check that it is also enforced on the node, since the node decides.

**9. Plan or read-only mode, when built, is a capability, not a prompt.**

This is a feature, so it waits for Gate 3.

- **Enforce it in three places:**
  - the tool registry: remove the mutating tools from the request, not only refuse them;
  - the dispatcher: refuse at execution;
  - the sandbox: a read-only Landlock profile for `bash`.
- **Leaving it.** Leaving plan mode is a user-only event. A model-callable tool cannot do it; Cline's `switch_to_act_mode` is the counter-example.
- **Models.** OpenHands' separate planner conversation with three tools (https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/api/agent-server-adapter.ts#L1352-L1363) and OpenClaw's `read-only` session mode.
- **Counter-example.** Codex's prompt-only `/plan` and its empty `explorer.toml`.
- **Check our own agents.** The read-only Scout and Reviewer agents: are they enforced by tool narrowing and node policy, or only by description? Our inventory says the `tools:` list narrows what is offered, and that node policy decides execution. Add a test that a Scout cannot execute a tool outside its list, even when the model names that tool.

**10. A judge model, if it is ever built (PLAN §0), follows these rules.**

- **What it sees.** The digest-bound action and the user's own messages, never tool output. It runs in a read-only, rule-less context.
- **Output.** Strict closed-key JSON (OpenClaw `exec-auto-reviewer.ts`: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/exec-auto-reviewer.ts#L152-L191).
- **Pre-filter.** A regex that denies text aimed at the reviewer before the model sees it.
- **Any failure goes to the human.**
- **Scope of its approval.** It covers only what it reviewed, and cannot approve an escalation.
- **Denial counter.** It lives in the session log, not in memory per turn (Codex per turn, OpenClaw LRU).
- **Repository instructions are not authorisation.**
- **Its decision is an event naming the judge model,** so the UI can show "approved by judge".

**11. Hooks, if ever added, fail closed and can only restrict.**

- A hook may deny, or escalate to the human. It may never approve (Hermes Agent).
- Policy is re-resolved on any rewritten input (Oh My Pi).
- Hooks shipped in a repository need a node-side trust record bound to a content hash of the hook and every script it references (Codex hashes only the declaration).
- Crash and timeout block.

##### What not to copy at all

- Open defaults: OpenCode, Oh My Pi, goose, Cline CLI, OpenClaw host exec.
- Independent security and ask axes, where a combination silently never prompts (OpenClaw `exec-approvals-policy.ts`).
- A "run to completion" switch that drops deny rules (Warp `auto_approve_bypasses_command_denylist`, default true).
- A command-line flag that loosens policy (Warp `--sandboxed`, Codex `--ignore-rules`).
- Hosting foreign agents with their safety flags off.

### Sandboxing and where commands run

From [round2/synth/03-sandbox-containers-execution.md](round2/synth/03-sandbox-containers-execution.md).

These points are ranked. Each says what to do, whom to copy from, and how it relates to `main`,
pull request 4 (Landlock, on `integration/gate3`) and the design note
(`docs/nodes-credentials-environments.md` §4.6).

##### 6.1 The recommended rootless Podman invocation

The node builds this argv and nothing else contributes to it. The design note's §4.6 is close;
the changes are marked.

```
podman run --detach
  --name rh-<env>-<session>
  --label rizzy.env=<env> --label rizzy.session=<session>
  --label rizzy.argv=<sha256 of the canonical argv below>            # ADD
  --userns=keep-id --user <node uid>:<gid>
  --cap-drop=all --security-opt=no-new-privileges
  --read-only
  --tmpfs /tmp:size=1g,mode=1777,nosuid,nodev
  --tmpfs /run:size=64m,nosuid,nodev,noexec                          # ADD sizes and flags
  --network=none | --network=pasta                                   # see the network row
  --memory=<m> --memory-swap=<m> --cpus=<n> --pids-limit=<p>
  --ulimit core=0                                                    # ADD
  --mount type=bind,src=<root>/<env>,dst=<root>/<env>,relabel=private
  --workdir <root>/<env>/worktrees/<session>
  --env HOME=<root>/<env>/home --env NAME …                          # names only; values in podman's env
  --init --pull=never --log-driver=none
  <image@sha256:…> sleep infinity
```

Each `bash` call runs as `podman exec --user <uid>:<gid> --workdir … --env NAME … <ctr> <wrapper> bash -c <cmd>`.

Why each flag is fixed, and the project it comes from:

| Flag | Reason | Evidence |
|---|---|---|
| Node-built argv; no `extra_args` key in `node.toml`, now or later | Hermes appends `docker_extra_args` last, so a `-v /:/host` overrides everything, and its host-access check never reads extras. OpenClaw's three `dangerously*` booleans are the same hole, behind a name. | Hermes `docker.py#L741-L745`, `approval.py#L1161-L1175`; OpenClaw `validate-sandbox-security.ts#L23-L53` |
| `--userns=keep-id --user` | Hermes runs as root with `DAC_OVERRIDE` over host binds; do not. With keep-id, files written in the volume are owned by the node user, which the node's file tools and checkpoint git need. | Hermes `docker.py#L321-L343`; design note §4.6 |
| `--cap-drop=all`, nothing added back | Hermes re-adds `DAC_OVERRIDE`, `CHOWN`, `FOWNER`, `SETUID`, `SETGID`. OpenClaw keeps `ALL` dropped by default. | Hermes `docker.py#L321-L343`; OpenClaw `container-allocation.ts#L71-L167` |
| `--security-opt=no-new-privileges` | The one flag OpenClaw's configuration cannot remove. Hermes drops it in `docker_snap_compat`. | OpenClaw `container-allocation.ts#L71-L167` |
| No `--security-opt seccomp=unconfined` or `apparmor=unconfined`, ever | OpenClaw rejects both. Keep Podman's default seccomp profile. | OpenClaw `validate-sandbox-security.ts#L274-L356` |
| `--read-only` plus sized tmpfs | OpenClaw sizes every tmpfs. The design note's `/run` has no size, so a full `/run` is RAM pressure on the node. | OpenClaw `config.ts#L91-L117`; Hermes tmpfs `noexec` on `/run` |
| `--network=none` by default | OpenClaw's default is `none`; Hermes's default of on is the mistake. When `network = "on"`, Hermes shows that an env-var proxy is not egress control: raw sockets bypass it. So "on" means really on. Any allow-list needs a namespace with only a proxy route, which Codex's bridge and Zed's proxy show. | OpenClaw `config.ts#L91-L117`; Hermes `docker_egress.py#L29-L76`; Codex `proxy_routing.rs#L131-L206` |
| Never `--network=host` or `container:*` without `allow_host_network` | OpenClaw rejects both outright. | OpenClaw `validate-sandbox-security.ts#L93-L132` |
| No published ports; nothing on the host listens for the container | OpenHands binds `0.0.0.0` so containers can reach the host, putting the whole API on the LAN. | OpenHands `dev-with-automation.mjs#L528-L543` |
| Limits from `node.toml` only; a session may lower them, never raise them | OpenHands forwards client-supplied memory and CPU with no bound. | OpenHands `agent-server-adapter.ts#L804-L835` |
| `--ulimit core=0` | OpenClaw. A core dump of a process holding a secret lands on the volume. | OpenClaw `config.ts#L91-L117` |
| One `--mount`, source validated | OpenClaw rejects socket paths, credential directories and system directories, and checks the real path of the nearest existing ancestor. Our single source must pass `check_protected` and must not be, or contain, the data directory, `$XDG_RUNTIME_DIR` (the rootless Podman socket lives there) or `~/.ssh`. | OpenClaw `validate-sandbox-security.ts#L23-L53` |
| `--env NAME` with the value in the `podman` child's environment | Hermes keeps values out of `/proc/*/cmdline` this way; goose puts `-e K=V` on argv, and the host process list shows it. | Hermes `docker.py#L539-L542`; goose `stdio.rs#L40-L53` |
| `--pull=never`, image by digest from the allow-list | OpenClaw `--pull=never`. The image is never chosen by the master, the project or the repository. | OpenClaw `container-allocation.ts#L71-L167` |
| `--label rizzy.argv=<hash>`; recreate on mismatch | OpenClaw bumps `createArgsEpoch` so an old container never runs with stale flags after an upgrade. | OpenClaw `constants.ts#L62-L64` |
| Check that the created id is 64 hex digits | OpenClaw. It rejects a confused CLI output before the id is used in later commands. | OpenClaw `container-allocation.ts#L214-L273` |
| `--init` | OpenClaw and Hermes. It reaps zombies and gives the container a real PID 1, so stopping the container kills every descendant (see 6.4). | as cited |
| No node or master credential in the container | OpenHands puts the session key, which can approve the agent's own actions, into every container. The link key and any grant must never be in the volume or the environment. | OpenHands `dev-with-automation.mjs#L975-L985` |

Network caveat (`not settled`): no input file says what rootless `pasta` exposes to the container.
In particular, it is not known whether the host loopback (the master on `127.0.0.1:7777`, the
node's own sockets) is reachable. Codex's reviewer flagged the same shape: with network on, an
unauthenticated loopback listener is reachable from the sandbox (`inferred`). Write the test
before shipping `network = "on"`.

##### 6.2 What to add to the Landlock path (PR 4): a seccomp layer, rule by rule

Our Landlock path already confines reads, which DeepSeek's does not. Its residuals are UDP, DNS
and pathname sockets below ABI 9. Codex's `Restricted` table fills exactly that gap.

Install the filter inside `sandbox-exec`: after `restrict_self` and `no_new_privs`, immediately
before `exec`. The order follows Codex's inner stage, which also checks that no capabilities
remain first.

| # | Rule | Mode | Closes | Caveat |
|---|---|---|---|---|
| 1 | `socket`: allow only `AF_UNIX`; `EPERM` otherwise | network off | UDP and DNS, raw sockets, `AF_VSOCK`, and anything Landlock ABI 4 does not cover | Codex accepts this in `Restricted`. Test that the tools we expect (git, cargo, compilers) still run. |
| 2 | `socketpair`: `AF_UNIX` only | network off | nothing new; matches Codex | none |
| 3 | `connect`, `bind`, `listen`, `accept`, `accept4`: deny | network off | pathname Unix stream sockets (Docker, D-Bus, ssh-agent) on kernels below ABI 9 | Breaks any legitimate local socket use under network off. Codex accepts that. |
| 4 | `sendto`, `sendmmsg`, `recvmmsg`: deny | network off | addressed datagrams | `recvfrom` stays open: Codex keeps it for socketpair users (cargo clippy). |
| 5 | `sendmsg`: decide explicitly | network off | an addressed `sendmsg` to a pathname datagram socket (for example a syslog or notify socket) | Codex leaves `sendmsg`/`recvmsg` open (the reviewer's correction). Seccomp cannot read `msg_name` behind the pointer, so denying `sendmsg` also breaks socketpair IPC that uses it. Whether Landlock ABI 9 covers datagram sends is not in the input. Either deny it and test the toolchain, or keep it open and list it as a named residual in `PLAN.md` R2. Do not leave it unmentioned. |
| 6 | `getsockopt`, `setsockopt`, `getpeername`, `getsockname`, `shutdown`: deny | network off | nothing on its own; it is part of Codex's set | Codex denies these and its tools work. Copy the set, and drop a rule only if a test shows breakage. |
| 7 | `io_uring_setup`, `io_uring_enter`, `io_uring_register`: deny | always | io_uring ops do not pass through the seccomp checks on `connect` and the rest; Codex blocks io_uring in every mode | none known |
| 8 | `socket(AF_VSOCK)`: deny | always, including network on | host services over vsock; on WSL2, Windows process launch | Codex's stated reason |
| 9 | `ptrace`, `process_vm_readv`, `process_vm_writev`: deny | always | reads of another sandboxed process's memory | Landlock already denies ptrace-class access to processes outside the domain; this is defence in depth inside the domain |
| 10 | Default action allow; denied calls return `EPERM`, not `SIGSYS` | | | A denylist, as in Codex. Use `EPERM`, never a kill: Codex treats 128 plus SIGSYS as a "sandbox denial" that can lead to an unsandboxed retry. |
| 11 | Unknown architecture: `Refused` in `required` mode | | | Codex supports only x86_64 and aarch64 and panics elsewhere. Our equivalent must be a refusal, not a silent skip. The filter must also check the syscall architecture field (the 32-bit compat syscall path); that requirement is ours and is not in the input. |

How to report it: extend `Enforced` to include "seccomp filter installed". The status line
should say which of rules 1 to 9 are active. DeepSeek computes `full`/`partial` and shows it to
no one; ours must appear in the node status and in the tool result (as Zed tells the model when
it ran unsandboxed). With rules 1 to 4, the PR 4 residuals "UDP, DNS" close under network off,
and "pathname sockets below ABI 9" closes for stream sockets. The datagram case stays open unless
rule 5 denies `sendmsg`.

Dependency: a seccomp crate must go through the `CLAUDE.md` dependency check (exact pin, docs.rs,
licence, two weeks old, `cargo deny`). No input file evaluates one. Our own code must stay free
of `unsafe`.

##### 6.3 Also on the Landlock path

1. **Protected subpaths inside writable roots.**
   - Codex keeps `.git`, `.agents`, `.codex` and `.aws` read-only.
   - DeepSeek has none, so `.git/hooks` is writable and a later unconfined `git` runs the hook.
   - Zed's Linux protection skips a `.git` that does not exist yet.
   - Check whether PR 4 protects `.git/hooks` and `.git/config` in the session folder. If not,
     add Landlock rules that leave them read-only. Test the "`.git` did not exist at start"
     case.
2. **The node's own sockets.** Codex had to mask its daemon directory because a same-uid process
   could drive an unauthenticated control socket.
   - With `network = "on"`, below ABI 9, a Landlock-confined command can connect to any pathname
     socket the node user owns.
   - Inventory every socket the node and the master open: master WebSocket, node link, MCP
     bridges, the Podman socket. Each must be authenticated or outside `SYSTEM_READ`.
   - Test: a sandboxed command cannot connect to any of them.
3. **One root derivation.** DeepSeek's file fence and its bwrap backend disagree about `/tmp`.
   Our file tools and the sandbox should derive their roots from one function. The design note's
   "same absolute mount path" requirement for containers is the same rule.
4. **Kill on Landlock.** A process-group kill leaks `setsid` children (Cline, goose; inferred for
   Oh My Pi and Pi). Oh My Pi's re-walk narrows the leak; a cgroup closes it. On the host path,
   consider a per-session cgroup or systemd scope. At least test that a `setsid sleep 1000`
   started by the agent is gone after cancel.

##### 6.4 Killing inside the container

The design note assumes that killing the `podman exec` client does not kill the process inside.
No input settles that.

Keep the design's three steps, all three:

1. A wrapper prints its process-group id.
2. `podman exec <ctr> kill -TERM -<pgid>`, a grace period, then `-KILL`.
3. If that fails, `podman stop`. That step is the guarantee: with `--init`, stopping the
   container kills its whole cgroup, the same reason Codex and Zed use `--die-with-parent` and a
   PID namespace.

Add Pi-env's liveness rule: when the node loses a session's actor, it stops that session's
containers. Do not leave them for a later sweep: Warp's `sbx` cleanup is a TODO and stopped
containers pile up.

##### 6.5 Do not relax checks inside a container

- Hermes skips the hardline floor, the sudo guard and the dangerous-pattern prompts on container
  backends.
- OpenClaw's default lets sandboxed exec run without approval.
- Warp's `--sandboxed` empties the denylist.
- OpenHands skips hooks for container workspaces.

A container with network can still push to a remote or exfiltrate. Keep the approval path, the
floor and checkpoints identical for host and container sessions. Add a test that a command
denied on the host is also denied in a container session.

##### 6.6 Executor policy and escalation

Our node is the executor and the decider. Three models to set against it:

- Do not copy Codex's exec-server, which applies whatever sandbox intent arrives and runs
  unsandboxed when none does.
- Do not copy OpenClaw's rule that a Gateway-forwarded approval satisfies the node's allow-list.
- Copy OpenCodex's executor rules:
  - validate every argument on the executor;
  - refuse `exec` when no sandbox exists;
  - check file identity after open;
  - refuse hard links;
  - re-check the root's identity on each call.

For escalation, copy Zed and DeepSeek, not Codex:

- The model asks for a specific widening with a required reason. The grant is one-shot and bound
  to that call. A sub-agent cannot escalate (DeepSeek pins it to `never`).
- The result tells the model the truth about its isolation.
- Never let an `allow` rule remove the sandbox (Codex). Never let a "for this thread" answer
  silently unsandbox later commands (Zed).
- Never decide "denied by sandbox" from stderr keywords (Codex). Use our exit code 125 and the
  runner's own signature (DeepSeek).

##### 6.7 Unavailability

- Keep PR 4's fail-closed `required` mode.
- Copy DeepSeek's functional probe: really restrict and run `true`, never a version check. Run it
  once at node start, and announce the result.
- For containers, the design note's self-test (run, exec, kill, `keep-id` ownership) is the
  matching probe. Add `podman inspect` assertions to it: no privilege, `CapEff` empty, one mount,
  `ReadonlyRootfs`, the expected network mode, the argv label.
- Zed's "Run unsandboxed" fallback prompt is a design we should not offer. A node either has a
  confinement kind or refuses the session.

##### 6.8 Pass-through flags for foreign agents

If rizzy ever runs Codex or Claude Code as a sub-agent, Warp shows the failure:
`--dangerously-bypass-approvals-and-sandbox` plus pre-written trust. The foreign CLI must run
inside our confinement (Landlock or the container), never with its own sandbox turned off on the
bare node.

### Credentials, secrets and authentication

From [round2/synth/04-credentials-secrets-auth.md](round2/synth/04-credentials-secrets-auth.md).

Our position, for reference: on `main` the node starts children from an empty environment plus an
allow-list and `env_pass` globs, always stripping provider key vars, `.env` names and `HARNESS_*`
(`crates/tools/src/env.rs`); known secret values are scrubbed from tool output, model text and
events (a courtesy, `crates/node/src/session.rs:194-215`); the master has accounts, cookie
sessions stored as SHA-256 with `HttpOnly; SameSite=Strict`, a Host allow-list and Origin check,
and an admin-equivalent API bearer; the node dials out over TLS to a pinned certificate and signs
a nonce; each node holds its own provider key in `config.toml` (`research/round1/own-rizzy-harness.md`).
Pull request 4 adds the Landlock shell sandbox, the empty allow-listed child environment and
redaction across chunk boundaries. The design note moves keys to sealed per-workspace storage on
the master, hands them to workspace nodes per turn (`session.grant`), reserves `ModelAccess::Proxy`
for per-user nodes, and replaces join tokens with a pair code.

Ranked by importance.

**1. The agent must not be able to reach any control-plane credential. Test it.** The worst
failures in this group are not provider keys; they are the harness's own credentials in the
agent's reach: OpenHands puts the master session key in the agent's environment and prompt;
OpenCode leaves the server password in the environment outside stdio mode; DeepSeek's
cookie-signing secret is readable by confined bash, so the agent can forge a login. On a node
that means three things, each with an abuse test written first:

- The node's link private key (`node.key` or `$CREDENTIALS_DIRECTORY/node.key`), its data dir and
  `node.db` are unreadable under the Landlock profile from pull request 4. Test: a confined `cat`
  of each path fails. DeepSeek is the counter-example (finding 21).
- Grant keys live only in the node process's memory and never in a child's environment, argv or
  a file. Test: start a turn with a grant, run `env` and `cat /proc/self/environ` and
  `cat /proc/$PPID/environ` in the shell, assert the key bytes are absent.
- The node process is non-dumpable, so a same-uid child cannot read its memory through
  `/proc/<pid>/mem` or ptrace. Codex has `pre_main_hardening` but does not apply it to its main
  binary (absence confirmed in review):
  https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/process-hardening/src/lib.rs#L7-L61.
  We forbid `unsafe`, so use a safe `prctl(PR_SET_DUMPABLE, 0)` wrapper (for example `rustix`,
  which needs the dependency check in CLAUDE.md). Test: the shell's `cat /proc/$PPID/mem` or a
  `gdb -p` attach fails. Note this is not enough without the sandbox; whether Landlock alone stops
  ptrace was not in the code read (section 7).

On the master side, the same rule applies to the local node and the master's chat runtime, which
run in the master process (design note §4.3): the master's `api-token`, `secrets.toml`,
`keys/secrets.key` and `master.db` must be unreadable from the embedded node's shell. That embedded
node is the case OpenHands' host mode shows going wrong.

**2. Shape the per-turn grant like OpenClaw's worker credential, not like a long-lived key
drop.** Read `src/gateway/worker-environments/credential.ts`, `admission.ts`,
`inference-model.ts` and `store-transitions.ts` in OpenClaw. Concretely:

- *Lifetime.* The design note's `expires_ms` default of 6 hours renewed while a turn runs is
  longer than it needs to be. OpenClaw's credential lives 10 minutes and is renewed while the
  owner is current, with a timer at expiry that re-runs validation; Hermes' tickets live 30 s and
  are single-use. A grant TTL does not stop the node from copying a provider key (the design note
  says so, and only rotation fixes that), but it bounds what an attacker who later steals the
  node's link key receives, and what a crashed turn leaves in memory. Recommendation: a short TTL
  (minutes), renewed by the master on the link while the turn is live, dropped on `grant.drop`,
  on turn end, on link loss after the running turn ends (A3), and when a newer grant for the same
  session is sent. Minting a new grant should cancel any in-flight model call that used the old
  one, as OpenClaw's mint does
  (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential-broker.ts#L105-L126).
- *Scoping.* Bind the grant to `(node_id, session_id, turn or epoch)`. The node must refuse to use a
  grant for any other session, and the master must refuse to send one to a node whose
  `workspace_id` differs from the session's effective workspace. Scope it to the providers the
  resolver computed, and check that list against the *resolved* models after alias expansion, not
  the names the agent spec carries (OpenCodex finding 14:
  https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/admission-model-scope.ts#L8-L29).
  Never let the node name a key or profile (OpenClaw refuses `@profile`).
- *What to store.* On the node: nothing. On the master: `node_key_grants` with name and times, as
  designed, plus the grant id and its hash only if the grant itself becomes a bearer (it will for
  the proxy, item 3). Sealed `workspace_secrets` should use AES-GCM associated data that binds
  `workspace_id` and `name`, so a row copied from one workspace or name to another fails to open.
  Warp binds `{actor_uid, secret_name, secret_type}` into HPKE sealing
  (https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/managed_secrets/src/envelope.rs#L33-L80),
  and Oh My Pi binds the broker URL into its cache's AAD. The design note does not say what the AAD
  is; it should.
- *Register every granted value with the node's `SecretSet` at receipt* (the design note does this),
  the way OpenClaw registers each minted credential and GitHub token for redaction.
- *Revalidate per frame.* Codex checks bearer credentials once at upgrade and documents that expiry
  is not enforced after
  (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L286-L330);
  OpenClaw re-checks hash, expiry, state and epoch before every worker request. Our link should
  refuse a `session.prompt` whose `grant_id` is expired or revoked on the node, and the master
  should drop the link of a node removed mid-turn (the design note's `grant.drop` then link drop
  does this; test that a prompt sent after removal is refused).
- *Forge tokens* will be readable by the agent if they are in git's environment inside the
  sandbox (OpenClaw corrected finding). Hand them only to the git child, scoped to `[forge] hosts`,
  short-lived, and expect the agent can read them when it can run git.

**3. The proxy path (`ModelAccess::Proxy`) must be bounded before it is built.** OpenCodex is the
reference; read `src/server/inbound-body-admission.ts`, `src/lib/translator-budget.ts`,
`src/bridge/sse.ts`, `src/server/responses/request-send-budget.ts`,
`src/server/responses/combo-stream-preflight.ts` and `src/combos/failover.ts`. Oh My Pi's
`auth-gateway/clients.ts` is the reference for scoped tokens. Our process must stay under 50 MB
idle, and OpenCodex says its byte budgets "do not cap RSS" because bodies are materialised several
times. So:

- Stream the request body through without materialising it more than once; cap it well below
  OpenCodex's 256 MiB (model requests with images are the only large ones); admit against a shared
  byte budget with a bounded wait queue and shed with 503 plus `Retry-After`.
- Make the response a pull stream with a buffer of one event and cancel the upstream on client
  drop, as `bridgeToResponsesSSE` does. Cap per-event and per-turn retained bytes.
- Put the retry budget on the logical request and on the turn (OpenCodex charges a workflow root so
  fan-out cannot multiply sends). Pull request 7's provider retry with backoff must count against
  this budget when the call goes through the proxy.
- Fail over only before output, with a byte-bounded replay buffer that commits on any event it does
  not recognise; separate "may try another target", "cool this target" and "cooldown scope", and
  persist cooldowns as absolute times.
- Check scope after routing; refuse metered or paid targets unless the workspace flagged them
  (OpenCodex finding 25).
- The proxy token is a bearer. Store only its hash with a domain tag (OpenClaw), bind it to one
  session and one node, give it the grant's TTL, and test that it does not work for another session
  (design note phase 8 already lists that test). Oh My Pi's gateway tokens have no expiry; do not
  copy that.
- The relay side on a per-user node should strip any credential header the agent's request carries
  and add only its own, as `ocx-child` does
  (https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/native/ocx-child/src/relay.rs#L225-L246).
  For our link the node already authenticates to a pinned master, so the mutual HMAC challenge is
  not needed.

**4. Refuse these authentication shortcuts outright.** Each has a project in this group that ships
it, and each would break an invariant or our multi-user model:

- **`Origin` as authentication** (Cline hub). Origin is a CSRF check on top of a credential, never
  instead of one. Our master already requires the cookie; keep any future local socket the same.
- **Unauthenticated loopback** (Codex app-server and exec-server, OpenCodex data plane and
  companion listener, Oh My Pi `--no-auth`, goose `--dangerously-unauthenticated`, OpenHands MARS
  without a key). The agent's shell is a local process; a sandboxed command with network on reaches
  loopback (Codex finding, inferred). No route on the master or node may be open on loopback,
  including a future proxy or health route that does more than return `ok`.
- **Any credential in a URL**: `?auth_token=` (OpenCode), `?token=` (goose desktop, DeepSeek launch
  URL, Oh My Pi relay), `?roomSecret=` (Cline dashboard), the VS Code editor token (OpenHands),
  `#token=` (OpenClaw). Our invariant already forbids it; the claim code and the pair code must
  travel in a POST body, never as a link. OpenClaw's endpoint schema (no userinfo, no query) is a
  test to copy for `node join` URLs.
- **Credentials in served HTML or browser storage** (OpenHands injects its key into `index.html`
  and `localStorage`; OpenClaw keeps a device seed in `localStorage`). Our UI must hold nothing but
  the `HttpOnly` session cookie; if we ever add browser device keys, use a non-extractable WebCrypto
  key.
- **Non-constant-time comparison of any secret** (OpenCode `===`, Cline dashboard `===`, Oh My Pi
  `Set.has` and `!==`, goose MCP-app proxy `!=`). Grep our code for `==` on secret types; compare
  hashes in constant time as the design note's pair-code step 4 says.
- **Tokens derived from one shared secret with no per-token revocation** (OpenCode pairing tokens
  are an HMAC of the password; DeepSeek cookies are HMACs of one persistent secret). Keep
  server-side session rows, as we do.
- **One shared admin-equivalent bearer** (Hermes API server key, goose secret, OpenHands session
  key, OpenCodex env token, which carries no scope). Our `api-token` is admin-equivalent and "only
  the API token sees all" chats (`own-rizzy-harness.md`), which is the same weakness and sits
  against "chats are private to their owner, for admins too". This is outside the design note;
  it should get its own finding: per-user API tokens, stored as hashes, scoped, revocable.
- **Bearers stored in plaintext** (OpenClaw device tokens, OpenCodex data keys in `config.json`).
  Store hashes of anything we issue; OpenClaw does it for worker credentials and not for device
  tokens, which shows how easy it is to be inconsistent.
- **Approvals answerable by any connection** (Cline `approval.respond`). Bind an approval to the
  session, the owner and the tool-call identity on the master; the node already decides policy.
- **A silent fallback to a weaker store** (goose keyring to plaintext; OpenHands in-memory key).
  OpenCodex shows the alternative: fail closed with a clear error ("no plaintext fallback"; 503 if a
  token file cannot be hardened). The design note's "systemd credentials, else 0600 file" must be an
  explicit, logged choice at join, not a fallback on error.
- **Trust on first use with a blank fingerprint** (goose desktop) and SSH-verified auto-approval by
  network position (OpenClaw). Our pairing binds to a pinned certificate and a typed code; keep it.
- **Secrets on the command line** (Pi `--api-key`, goose's container path `-e KEY=VALUE`). Codex
  reads its remote token only from a named environment variable and only over `wss` or loopback;
  Hermes passes Docker values by name only.

On pairing specifically, OpenCodex's rules to copy into design note §4.4: store only the code's
hash; delete the pending request before validating anything else (one shot even on rejected
metadata); refuse requests carrying an `Origin` header; rate-limit by the kernel-observed peer
address; give every failure one generic message. Our design inverts the direction (the node shows
the code, the admin types it), so an attacker who reaches the node port can open pending requests
and print codes on its own terminal. The defence is that the admin compares the key prefix. The
design note shows 16 hex characters (64 bits); goose uses a 128-bit fingerprint in eight groups for
out-of-band comparison
(https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/card.rs#L56-L75).
Show 128 bits, grouped, on both sides.

**5. Check our environment allow-list and redaction against these lists.** Tests to add in
`crates/tools/src/env.rs` and the redaction code (pull request 4):

- *Always strip, even if `env_pass` names it.* Codex keeps a non-inheritable list that `set` cannot
  re-add (https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/shell_environment.rs#L14-L28);
  Hermes had a published advisory (GHSA-rhgp-j443-p4rf) because a passthrough name could register a
  managed credential, and now re-checks at use time and fails closed
  (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/env_passthrough.py#L46-L79).
  Test: `env_pass = ["*"]` and `env_pass = ["*_KEY", "HARNESS_*"]` still pass no provider key, no
  `HARNESS_*`, no link credential.
- *Secret-name substrings for any future blocklist layer:* Hermes `execute_code` drops names
  containing `KEY`, `TOKEN`, `SECRET`, `PASSWORD`, `CREDENTIAL`, `PASSWD`, `AUTH`, `DSN`,
  `WEBHOOK`, `CREDS`, `BEARER`, `APIKEY`, `COOKIE`
  (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/code_execution_env.py#L57-L92).
  DeepSeek's shorter pattern lets `DATABASE_URL`, `GITHUB_PAT` and proxy URLs with passwords
  through; use those three as test cases for `env_pass` warnings.
- *Injection variables:* OpenClaw's ~110 blocked-everywhere keys and prefixes (`NODE_OPTIONS`,
  `PYTHONPATH`, `BASH_ENV`, `GIT_*` executables, `LD_`, `DYLD_`, `BASH_FUNC_`)
  (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/host-env-security.ts#L38-L65)
  and goose's 31-key disallow list
  (https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/extension.rs#L91-L122).
  Test that `env_pass` cannot pass any of them and that `PATH` cannot be overridden from a request.
- *Git location variables:* Codex and Oh My Pi strip `GIT_DIR`, `GIT_CONFIG*`, `GIT_WORK_TREE`;
  goose adds `-c safe.bareRepository=explicit -c core.fsmonitor=false`. Our checkpoint git already
  clears the environment; the forge git path (design note §4.7) must too.
- *Project `.env`:* Oh My Pi strips values that came from the project's `.env`; DeepSeek accepts a
  provider key from the cwd `.env`. Test: a repository `.env` neither enters a child environment
  nor supplies a key.
- *Our own spawn secrets:* capture them into memory and delete them from the process environment
  before any child is spawned (Cline core; OpenCode stdio mode). Test: `HARNESS_*` and the node key
  path variable are absent from `/proc/<child>/environ`.
- *MCP and future plugin children:* start them from the same allow-list (Hermes:
  `PATH, HOME, USER, LANG, LC_ALL, TERM, SHELL, TMPDIR, XDG_*` plus declared env), not inherited
  (Cline, Pi, Warp, Zed and Oh My Pi inherit everything).
- *Redaction, by value first.* OpenCodex's log guard checks text against every value it holds,
  rebuilds the set on a content digest, and fails closed when it cannot enumerate it
  (https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/configured-credentials.ts#L1-L30).
  Our `SecretSet` should be the same: grant keys, forge tokens and the link key added at receipt,
  and the scrubber should refuse to emit (not emit unredacted) when the set is known to be
  incomplete.
- *Redaction patterns as a second layer:* Hermes always masks vendor-prefixed keys, auth headers,
  JWTs and private keys and runs a `KEY=value` pass on env-dump commands; for file reads it writes a
  non-reusable sentinel so the agent cannot copy a masked value back into config
  (https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/redact.py#L1491-L1530).
  Warp's default pattern list (Google, OpenAI, Anthropic, `sk-`, AWS, Slack, GitHub, Stripe, JWT and
  others) is a test corpus
  (https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/secret_redaction/src/lib.rs#L296-L360).
- *Ordering and coverage:* redact before any hook or plugin sees output (Hermes redacts after an
  output hook); redact before compaction summaries are stored (Hermes redacts summariser inputs and
  output; pull request 7's compaction should be tested with a key in a tool result); and never use
  reversible placeholders that turn back into real values in tool arguments (Oh My Pi).

**6. Trust gates, for what nodes load from a repository.** The node will eventually load project
files that execute or steer (skills, MCP servers, agent definitions, setup scripts). Copy Codex's
rule that trust state lives only in a layer the repository cannot write
(`codex-rs/hooks/src/config_rules.rs`), Pi's rule that the default cannot be relaxed from the
project (`settings-manager.ts` L1020-L1023), Pi's two-pass startup (decide with trusted code only),
and Warp's "project MCP servers never auto-start". Add what none of them has: bind the decision to a
hash of every file that will execute, including scripts a hook points at (Codex's gap), and store it
in `node.toml`'s directory, not on the master. Oh My Pi's `isProjectTrusted() => true` is the
anti-pattern; OpenCode and goose show what a cloned repository can do without a gate.

**7. Smaller items.**

- Refuse to start on a loose file mode for every secret file (DeepSeek's `.credentials.yaml`
  check; our AUDIT L7 notes the `api-token` mode is not enforced on read on `main`).
- Describe parse errors of secret files without quoting lines (DeepSeek).
- Keep idempotency and webhook tokens in headers only; OpenClaw rejects `?token=` with 400 and does
  not exempt loopback from failure throttling. Ours already uses a bearer header and stores hashes.
- Credential pools (Hermes, OpenClaw auth profiles, OpenCodex) belong on the master per workspace
  once keys move there; key the backoff on the failure reason, separate "disabled until a human
  acts" (billing, permanent auth) from "cooling", and never record pool state in the event log.

### Links, workers and client protocols

From [round2/synth/05-links-workers-protocols.md](round2/synth/05-links-workers-protocols.md).

Our position, for reference. On `main`: the node dials the master over TLS with a pinned
self-signed master certificate; the node's Ed25519 key signs a nonce bound to the certificate
fingerprint; 8 MiB frames; 10 s handshake steps; 32 concurrent pre-auth handshakes
(`research/round1/own-rizzy-harness.md`, "Link"). The owning node is the only writer; the
master mirrors its events and rejects events for sessions another node owns; a link reconnect
replays with `Resume{after}`. Streaming deltas are stored as events forever (AUDIT D5). On
`main` the master reads node frames into an unbounded channel (AUDIT L1); pull request 4
bounds it (8 inbound parsed messages, 64 outbound commands). The design note
(`docs/nodes-credentials-environments.md`) adds pair codes (§4.4), per-turn `session.grant`
(§4.3), and fork by `session.seed` batches written as `history_imported` (§4.9), and records
that "a second connection replaces the first (`nodes.rs:208-212`)" (§6).

The surveyed code says one thing clearly: **only OpenClaw fences a remote executor's writes**,
and every system that lets the controlling side send policy ends up letting it widen the
executing side. Our invariants put us on the right side of both; what we lack is the fence
itself and a resume protocol that keeps deltas out of the log.

Feature freeze note: items 1 to 4 below harden the node link and the approval path (security
code under `CLAUDE.md`); items 8 and later are features and wait for Gate 3. Check each against
`FIX_PLAN.md` before scheduling; this file does not know the plan's task numbers.

##### 6.1 (Rank 1) Fence the node link and every session write

**What we have is "latest connection wins"**, the same as OpenCodex Remote Workspace
(`workspace-hub.ts` `attachConnection`, finding 31) and weaker than OpenClaw. It replaces a
dead socket, but it does not refuse frames that were already read from the old socket and
are still queued (pull request 4's 8-message inbound channel is exactly such a queue), and it
says nothing about a session that was archived, deleted or forked while its node was offline.
Codex's "one attachment plus retention" leaves the same gap (review: "no epoch or fencing term
found anywhere in exec-server").

**Design. Two counters, both in `master.db`, both checked inside the transaction that writes.**

1. `nodes.link_gen` (u64). Incremented in the transaction that accepts a node connection,
   after the signature check. Sent to the node in `Accepted { node_id, link_gen }`. The master's
   in-memory registry holds the current value per node.
   - *Checked where:* in the master's reader pump, on **every** inbound frame, before
     dispatch: the frame's connection must still be the registry's current `link_gen` for that
     node; otherwise drop the frame and close that socket with a `superseded` code. This is
     OpenClaw's per-request revalidation (`admission.ts` L188-L218, `worker-connection.ts`
     L326-L351) reduced to one integer compare.
   - *Bumped when:* each accepted connection; node revocation; node re-pairing.
2. `sessions.owner_epoch` (u64), drawn from one global counter that never goes backwards and
   never reuses a value (OpenClaw: `max(...) + 1`, `store-mutations.ts` L30-L50; a single
   `meta.epoch_counter` row does the same with one `UPDATE ... RETURNING`).
   - *Set when:* the session is created on its node.
   - *Bumped when:* the session is archived, deleted, its node is revoked or removed from the
     workspace, or the source of a "continue elsewhere" fork is closed (6.2). **Not** bumped by a
     reconnect of the same node: the node is still the owner and must be able to resume.
   - *Carried:* in `session.create`/`session.open` to the node, and in every node-to-master
     event frame as `(session_id, owner_epoch, seq, hash)`.
   - *Checked where (master):* in the same SQLite transaction as the mirror insert, as one
     predicate: `owner_node = conn.node_id AND owner_epoch = frame.epoch AND state = 'active'
     AND seq = last_seq + 1`. For `seq <= last_seq`, compare the stored event hash: equal means
     a duplicate from a resend, acknowledge it; different means divergence (a node restored from
     an old disk image, a cloned VM with the same key): refuse with `diverged` and mark the
     session for attention. This is OpenCode's `replay` rule ("Replay diverged", "Sequence
     mismatch", owner refused; `bus.ts` L329-L378) applied to a real link.
   - *Checked where (node):* the session actor stores the epoch it was given and refuses any
     master command for that session that carries a different epoch. On a `fenced` reply to an
     append, the actor stops, kills the running tool's process group, marks the session
     read-only locally, and does **not** retry (OpenClaw's worker treats `owner-epoch-mismatch`
     as terminal: `worker-connection-contract.ts` L22-L26).
3. Grants (`session.grant`, design note §4.3) are bound to `(session_id, owner_epoch,
   turn req_id)`; the node drops a grant on `grant.drop`, at expiry, on a `fenced` reply, or
   when the epoch it holds changes. OpenClaw binds its credential hash to the durable turn
   claim for the same reason (`credential.ts` L9-L60).

**How this breaks, and what the counters do about it.** A stolen node key: both copies get
`link_gen` bumps, each kills the other's socket, frames from the loser are dropped at dispatch;
the design note's "revoke after 3 flaps in 10 minutes" (§6) then bumps every session's epoch. A
node restored from a backup: its replayed events diverge by hash and are refused. A session
archived while its node was offline: the node's resume replays events stamped with the old
epoch and is told `fenced`. A master restored from an older backup: it may hold a smaller
`last_seq` than the node; the node's resend fills the gap (hashes match), which is the desired
outcome.

**Tests that fail on `main` and pass after.** (a) Queue two event frames on connection A,
connect B for the same node before A's frames are dispatched: A's frames are refused and not
mirrored. (b) Archive a session while its node is disconnected; reconnect; the node's resume is
refused with `fenced` and the node marks the session read-only. (c) Resend an already-mirrored
event with the same hash: acknowledged, not duplicated. (d) Resend `seq` N with a different
payload: refused with `diverged`. (e) A grant is not usable after the session's epoch changes.

##### 6.2 (Rank 1, same change) Session fork without live migration

The design note forbids live migration and forks by re-seeding history (§4.9). The fence above
makes that safe; the protocol:

1. The master, in one transaction: chooses `through_seq` as the highest **contiguous mirrored**
   seq of the source session S (never an unmirrored tail); records `session_forks(S', S,
   through_seq, hash_at_through_seq)`; creates S' with a fresh `owner_epoch` and its target node.
   For "continue elsewhere" (source closed), the same transaction sets `S.state = archived` and
   bumps `S.owner_epoch`, so any later append from S's node is refused.
2. If S's node is online and S has a running turn, refuse "continue elsewhere" until the turn
   ends, or close it: pending approvals in S become synthetic closers (goose answers a pending
   approval whose lease is gone with "Tool approval expired", `ops_toolcalling.rs` L1010-L1031;
   DeepSeek's closers say the outcome is unknown, `repair.ts` L53-L98). Events S's node wrote
   after `through_seq` and never delivered are refused by the fence; the UI must say how many
   (the master knows the node's reported head from its last resume).
3. Seeding: `session.seed { session_id: S', batch_no, events, last }`. The target node writes
   each batch as `history_imported` events with its own seq, keyed by `batch_no` so a resend is a
   no-op, and acknowledges only after the batch is durable (Hermes acknowledges a relay frame only
   after durable admission, `ws_transport.py` L971-L1004). S' refuses prompts until the last batch
   is acknowledged. The first event of S' records `forked_from { parent: S, through_seq,
   parent_hash }`.
4. No files cross. The design note already says files come back from the forge. Codex offers
   nothing to copy here except the "owner id inside `.git`" marker (`worktree/src/metadata.rs`
   L1-L90), which is worth copying so two sessions never share one worktree.

##### 6.3 (Rank 2) Nothing the master sends may widen the node

Every design surveyed that lets the controlling side send policy lets it widen the executing
side. Do **not** copy these properties:

| Project | Property | Why it widens the executing side |
|---|---|---|
| Codex exec-server | sandbox intent sent per request; no `sandbox` means unsandboxed; no executor ceiling | the caller's policy is the executor's policy |
| OpenClaw node | `approved === true` from the Gateway satisfies the node's ask and allow-list miss; only `security=deny` holds | a master-side approval overrides the node's own list |
| OpenClaw worker | a GitHub token in the launch descriptor, written where the agent's shell reads it | the controller hands the agent a credential |
| OpenClaw provisioning | the Gateway SSHes into the worker box | the controller dials the executor, so the executor must listen |
| Cline hub | `session.create` takes `toolPolicies`, `autoApprove`, `cwd`, `systemPrompt` from the client; approvals answerable by anyone | the client sets the executing side's policy |
| Cline hub | loopback `Origin` accepted as a credential | any local page or process becomes a controller |
| goose roaming and serve | a peer gets the full ACP surface, including `config/extensions/add` (a stored stdio command) and an unforced working-dir update | the controller adds code to the host and moves its root |
| OpenHands | start request carries `tool_module_qualnames` (imported by the server) and container limits (forwarded uncapped); a server-level bash endpoint | the controller chooses code and resources |
| OpenHands, OpenCode | the stack's own key reachable from the agent's shell | the executed agent can act as the controller and approve itself |
| DeepSeek external sub-agents | the child's permission mode is fixed per provider, independent of the parent | delegation escapes the parent's limits |
| Warp | unattended CLI children launched with their own permission systems off; ACP policy allows every non-denylisted request and prefers `AllowAlways` | the controller turns off the executor's guards |
| Zed | its `tool_permissions` are not applied to external agents; `terminal/create` runs unconfined | the client's own policy does not bind the tools it serves |
| Hermes SSH | sync-back writes remote files into the host's skills tree | the executor writes into the controller's instructions |
| pi-env | the daemon trusts every request on stdin | no executor-side policy at all |

The node must treat every `MasterToNode` field as a request that `node.toml` can only narrow:
`approval.respond` (an allow from the master must still pass `node.toml`'s `denied_tools` and
`max_approval_mode`), `session.create` (cwd inside `workspace_roots`, agent spec within what the
node allows), grants (refused when `model_keys` is `local` or `none`, as the design note says),
container and resource requests (caps from `[containers]`, as OpenHands should have done), and
anything that names code to load (refuse outright). Pull request 6's server-side enforcement of
where an agent may run is master-side narrowing; good, but the node must check again.

Copy the opposite cases: OpenCodex's executor intersects requested capabilities with what it
allows at the per-session handshake and refuses a presence that claims more than was enrolled
(`crypto.ts` L214-L286, `workspace-hub.ts` L427-L463); OpenClaw's node refuses a delayed approval
when its own policy snapshot changed and refuses PATH overrides (`invoke-system-run.ts` L349-L357,
L265-L290). Note that OpenCodex's reviewer suggested the hub should decide the capability set at
enrolment; for us that is backwards. Our node declares its ceiling in `node.toml`, the master
narrows.

**Tests.** A master `approval.respond` that allows a tool `node.toml` denies is refused and
logged by the node. A `session.create` whose cwd is outside `workspace_roots` is refused. A
grant sent to a node with `model_keys = "none"` gets an ack error. A container request above the
node's cap is clamped or refused, never applied.

##### 6.4 (Rank 3) Resume: durable events by cursor, deltas transient

**Durable stream, node to master.**

- Every event frame carries `(session_id, owner_epoch, seq, hash)`. The node's sender is a
  **cursor over the node's own log**, not an in-memory queue: it keeps `next_to_send` and
  `acked` per session and reads from the log. With at most W unacknowledged frames per link
  (a count and a byte cap), a slow master makes the sender wait, while the agent loop keeps
  appending locally because the node's log is the truth. This removes any need for an unbounded
  buffer on either side (AUDIT L1) and makes pull request 4's bounded channel safe to fill.
- The master acknowledges `(session_id, seq)` only after its mirror transaction commits,
  including the fence predicate (OpenClaw updates its ACK cursor only after `assertCurrent`,
  `worker-turn-rpc.ts` L279-L320; Hermes acks after durable admission).
- On reconnect the master sends `resume { session_id, after_seq }` with its mirrored head
  (we have `Resume{after}`); the node rewinds the cursor; duplicates are absorbed by the hash
  rule in 6.1.

**Transient stream.** `delta { session_id, owner_epoch, item_id, rev, text }` and tool
progress: no `seq`, not stored, coalesced latest-wins per `item_id`, dropped first under
back-pressure, never replayed. The durable `assistant_message` event carries the same
`item_id` and supersedes the deltas. This is OpenHands' envelope (`session-frames.ts` L1-L79:
`durable` with `seq`, `transient` and `delta` without) and DeepSeek's dense `revision` on
assistant frames. It also fixes AUDIT D5 (every delta stored forever): stop writing deltas
into the event log. Our own inventory says deltas are stored today
(`crates/core/src/event.rs:59-80`); the brief says the design separates them. Settle that
contradiction in code before building resume on top of it.

**Master to browser.** `subscribe { session_id, after_seq }`; the master registers the listener
first, reads the mirror up to a captured watermark, sends `sync { through_seq }`, then live
events (OpenCode's log contract, `bus.ts` L829-L878; DeepSeek's "listener armed before the
snapshot"). Each socket gets a bounded queue; on overflow close it with a "lagging" code and let
the client reconnect with its cursor (OpenCode fails the subscriber at 4,096; DeepSeek's
unbounded `Deque` is the bug to avoid). Deltas reach the browser only live; on reconnect the
client discards open streaming slots and waits for the durable event (OpenHands
`conversation-websocket-context.tsx` L1305-L1312). Because our seq is contiguous per session
with one writer, the cursor is "highest contiguous seq"; we never need OpenHands' 1,024-hole
heuristic.

**Master to node commands.** Every `session.prompt` carries a client-generated `req_id`
stored in the node's `user_message` event; the node acknowledges a repeat as already accepted
(DeepSeek `commands.ts` L389-L413). Every `approval.respond` names the `approval_id` and the
session epoch; the node accepts it only if an `approval_asked` event exists without a decision,
and rejects a second, conflicting decision (goose `tool_confirmation.rs` L148-L186). Pending
approvals are derived from the log, so a reconnecting UI rebuilds them (DeepSeek keeps them in a
process `Map` and loses them on restart).

**Node restart mid-turn.** Append closers with fixed wording (DeepSeek: "outcome is unknown ...
do not retry blindly", `repair.ts` L31-L41); never re-run a non-idempotent tool on resume (Pi
durable commits `replay: "unsafe"` before the effect, `tool.ts` L195-L227); never auto-continue
a turn whose node was lost (OpenClaw `canContinue = false`, `worker-node-loss.ts` L3-L15);
settle unattended routine turns as interrupted at boot so a routine queue does not stall
(Hermes `session_unattended_unknown.py` L41-L66), while a person's turn waits for the person.

**Tests.** Kill the link mid-turn, reconnect: the mirror has every durable event exactly once
and no deltas. Stall the master's mirror writes: the node keeps appending, memory stays bounded,
and the sender catches up after. A browser that stops reading is disconnected, reconnects with
its cursor, and sees no gap and no duplicates. A prompt resent with the same `req_id` after a
reconnect produces one user message.

##### 6.5 (Rank 4) Credentials on the node and the link key

- The node's link key and any grant must be unreachable from the agent's process tree.
  OpenHands puts its stack key in the agent's environment on purpose
  (`dev-with-automation.mjs` L975-L985); OpenCode leaves its password in shells outside stdio mode
  (`server-process.ts` L85-L97); in both, a prompt-injected agent can approve itself. Cline
  captures its spawn secret into memory and deletes it from the environment before spawning
  anything (`host-bridge-auth.ts` L4-L47); pull request 4's allow-listed empty child environment
  is the stronger form. Test: the key file and `$CREDENTIALS_DIRECTORY` are not readable from a
  sandboxed shell, and no grant value appears in a child's environment.
- Any node-local socket (a future control socket, an MCP bridge) must authenticate its peer or
  be hidden from the sandbox. Codex hides its unauthenticated daemon directory with bubblewrap and
  refuses legacy Landlock for that reason (`linux_run_main.rs` L386-L393); that Landlock cannot
  stop `connect()` to an existing Unix socket is the author's inference, and it matches pull
  request 4's recorded residual (pathname Unix sockets before Linux 7.1). Test: a sandboxed
  command cannot connect to any socket the node opens.
- Grants: OpenClaw keeps keys on the controller and brokers inference by model reference,
  refusing an unapproved model or a profile suffix (`inference-runtime.ts` L122-L153), and it
  also ships a worker-local mode. The design note's `ModelAccess::Proxy` slot is the same idea;
  keep it reserved, and bind each `Direct` grant to session, epoch and turn as in 6.1.
- Store only hashes of any bearer the master issues. OpenClaw hashes worker credentials but keeps
  device tokens in plaintext (`pairing-token.ts` L4-L18); OpenCodex hashes device bearers
  (`workspace-hub.ts` L377-L425). Our node identity is a key pair, so this applies to pair codes
  (the design note already compares hashes) and to any future per-client API tokens.

##### 6.6 (Rank 5) Pairing

The design note's pair code (§4.4) is in line with the best surveyed (OpenCodex: single use,
10 minutes, hashed, rate-limited per source; OpenCode: atomic consume). Copy three details:
**consume the code before validating the rest of the request** (OpenCodex deletes the grant
first, so a rejected request still burns it; `workspace-hub.ts` L377-L425); refuse any request
carrying an `Origin` header on the pairing path (OpenCodex `serve-options.ts` L484-L513); and
answer every failure with one opaque reason while charging only failures to the limiter
(OpenClaw `worker-connection.ts` L218-L262). Never let a pairing flow print a secret on a remote
stdout and read it back (Cline's SSH remote does, `remote-environments.ts` L1436-L1490), and never
put a secret in a URL or a flag value (Codex accepts a token only from a named environment
variable and only over `wss://` or loopback, `cli/src/main.rs` L2649-L2676).

##### 6.7 (Rank 6) Back-pressure without deadlock

Pull request 4 bounds the link (8 inbound, 64 outbound). Two rules from Codex to check against it:
reject *new requests* with a retryable error when a queue is full, but never drop *responses or
events* (Codex `transport/mod.rs` L231-L262); and make sure a full queue of tool output cannot
block an approval reply or a cancel (our per-session lanes answer approvals inline; add a test
that a node streaming at the cap still processes a cancel within the grace period). With the
cursor sender of 6.4, the node-to-master direction never needs to drop anything.

##### 6.8 (Rank 7) Listener authentication on the master

Our master already re-checks login on long-lived sockets (60 s recheck, close on logout); Codex
does not re-check expiry after the upgrade, so we are ahead there. Copy DeepSeek's fence order on
every `/api` request and upgrade: Host (loopback, bind address or `--allowed-host`), then
`Sec-Fetch-Site: cross-site` refused, then `Origin` must equal the Host authority, then the
credential (`api-request-trust.ts` L121-L151). Consider DeepSeek's refusal of wildcard binds
(it names one interface instead), which is stricter than our "explicit, logged" rule. The node
opens no listener; keep it that way (OpenClaw's provisioning SSH and Cline's remote hub both
require the executor to listen).

##### 6.9 (Rank 8) Version gate at admission

OpenClaw refuses a worker whose bundle hash or version differs from the expected build
(`admission.ts` L79-L186); OpenHands refuses an Agent Server older than `minimumAgentServer`
(`agent-server-compatibility.ts` L253-L318). Our `PROTOCOL_VERSION = 1` should become a minimum
plus a feature list in `node.hello`, so the master refuses an incompatible node before it
receives sessions and the UI can show which optional features a node supports.

##### 6.10 (After Gate 3) Protocol and client surfaces

- Generate the web UI's TypeScript types from the Rust protocol types and fail CI on drift
  (Codex `export.rs` L125-L150; OpenCode generates clients from one API definition).
- Put the ordering key of each master API method in the method table, not the handler (Codex
  `common.rs` L129-L139).
- If we ever offer ACP, copy Hermes' shape: a thin adapter that attaches to the node-owned
  session and never runs tools (`acp_adapter/entry.py` L205-L215). If we ever host another
  vendor's agent as an ACP client, start from DeepSeek's: empty client capabilities, no MCP
  servers, decline every approval by default, scrubbed environment, and derive the child's mode
  from the node policy rather than fixing it per provider.
- A "human approval" class that headless clients structurally cannot answer (Codex
  `outgoing_message.rs` L414-L430) is a cheap primitive for routines.

### Session storage, resume, fork and checkpoints

From [round2/synth/06-sessions-fork-checkpoints.md](round2/synth/06-sessions-fork-checkpoints.md).

State today (`research/round1/own-rizzy-harness.md`, `docs/nodes-credentials-environments.md`): an append-only per-session event log written only by the owning node's actor; resume rebuilds history from the log and closes interrupted tool calls synthetically; no fork; a shadow repository (cleared environment, reading the user's objects through alternates, 120 s git timeout) snapshots the folder before and after each agentic turn (`git add -A` into a private index), keeps the latest 10 (`checkpoints_keep`), and restores exact bytes for a turn or "from that turn on" with a forced restore when files changed since. The design note adds a worktree per session, the shadow repository keyed by work-tree path, a master-side `session_forks(session_id, parent_id, through_seq)` table, and a fork that writes imported history as `history_imported` events on the new node and copies no files. PRs 4, 6 and 7 do not touch the log or checkpoints; PR 4's bounded link matters only for item 9.

Ranked by importance.

**1. Fork: copy the prefix into the child's own log, and record lineage inside that log.** (design note)

Our log is linear with one writer per node, so fork does not need in-file branching. Record, written by the child's owning node as its first events:
1. `SessionForked { parent_session_id, root_session_id, parent_node_id, through_seq, through_turn_id, cut: "completed_turn" | "closed_open_turn", lossy: bool, files: "none" | "checkpoint" | "forge", files_ref }`. `through_seq` mirrors the master's `session_forks.through_seq`; the log copy survives the master row being lost or the parent being deleted.
2. `history_imported` batches as in the design note, each event tagged `inherited: true`.
3. `SeedEnded { inherited_count }` (DeepSeek's `session/end-seed`).

Why: DeepSeek's `isSeeded`, `inheritedEventCount` and the end-seed marker are what let duplicate-prompt detection and the fold distinguish inherited from new history (`packages/api/session-controller/src/commands.ts` L404-L407; `packages/core/session/src/fork.ts`). OpenCode zeroes the child's cost and tokens in the fork projector (`projector.ts` L121-L160), and goose carries non-unique message ids; usage totals and idempotency keys need the same care in ours. Copy semantics keep deletion simple (Codex pins the parent only during prepare; Pi's path-only parent link orphans on rename). Pointer-over-file (Codex) is not worth it for us: the child runs on a different node than the parent in the design note and cannot read the parent's file.

Cut rules, each from a project that got it right or wrong:
- Default cut is the last completed turn (DeepSeek's web default). A cut inside an open turn closes it with fork-specific synthetic results and `cause: forked` (`repair.ts` L31-L98, `fork.ts`), so the fold does not render an "interrupted" turn.
- Refuse while the parent turn runs unless the cut is at or before the last completed turn (Hermes requires an idle parent; goose's truncate checks nothing).
- The design note's import is lossy (tool calls cut to one line, 2 MiB per batch). `lossy: true` lets the UI say so and keeps a later export from presenting the child as the full transcript.
- The child starts at approval `ask` with no live connections (Hermes `hermes_state_mutation_branch.py` L34-L49). Check that our fork does not carry the parent's PR 4 "allow for this turn" state.

**2. Store the head explicitly; do not infer it from the last row.** (our log; applies to the existing "rewind from that turn on" and to any later edit-and-resubmit)

Evidence: Pi derives the leaf from the last entry, so a navigation with no following write is lost (inferred; reviewer agreed); Oh My Pi patched the same flaw with `activeLeafId` on off-branch writes; only Codex records the active history explicitly and swaps it by compare-and-set. Recommendation:
- Turn events carry `turn_id` and `parent_turn_id`. The active path is the chain from `head_turn_id` to the root; ordinary appends extend it.
- Add `HeadMoved { to_turn_id, reason: rewind | edit | fork_cut, expected_head }`, appended by the node actor, so the move is a compare-and-swap inside the node (Hermes `RewindPlan.expected_active_ids`, `hermes_state_rewind.py` L64-L77; Codex `replace_rollout_path_if_current`, `revert_thread.rs` L138-L156). The master keeps the head as a projection, never as the authority.
- Events that must not move the head (late usage, a background result for a finished turn, title updates) name their turn or carry `off_head: true` (Oh My Pi `activeLeafId`, `session-manager.ts` L3595-L3628; its `model_usage` re-journaling for discarded turns, L4089-L4108, is the model for billing).
- Rewind never deletes events (OpenCode's commit deletes rows and leaves the log as the only recovery, and only when persistence is on): the fold skips turns off the active path; abandoned turns stay for audit and export.
- Check first in our code: today a rewind restores files; does the fold still show the rewound turns as active after a restart? If so, files and conversation disagree after a restart, and a `HeadMoved` event fixes it even if we never build in-log branching.

**3. Restore: add the safeguards our forced restore lacks.** (destructive write to the user's tree: full stress test per `CLAUDE.md`, plus an abuse-case test)

1. Snapshot the present first and make it a checkpoint the user can restore to (Cline `checkpoint-restore.ts` L50-L154; Hermes pre-rollback, `checkpoint_manager.py` L1387-L1414; OpenCode `stage` and `clear`). If the snapshot or its rollback fails, refuse to continue (Cline throws `AggregateError`; Hermes' curator continues unprotected, `curator.py` L951-L958, the choice to avoid). Record `Restored { from_tree, to_tree, pre_tree, paths_changed, paths_skipped, forced }` so restore is itself undoable. Check whether our restore already takes a pre-restore snapshot; the inventory does not say.
2. Compare-and-swap on the tree we believe is there. The request carries `expected_tree` (the last post-turn snapshot). The node takes a fresh capture; if the tree id differs, it returns the changed paths (user edits, or work outside a turn) and requires an explicit choice per the existing forced flag. This is Cline's `update-ref HEAD new old` idea applied to our tree id. It is better than Hermes' write ledger, which only knew file-tool writes and therefore skipped exactly the shell-made files (`checkpoint_manager.py` L986-L1050, with the three exceptions in section 2).
3. Refuse while a turn runs on the session, and while another session shares the worktree. OpenCode returns a busy error (`handlers/session.ts` L445-L458); Hermes needs an idle parent. Add a test for restore during a turn.
4. Restore only attributed paths when "from that turn on" spans turns: the union of `Checkpoint.files` of the turns being undone, taking each path's earliest start tree (OpenCode `revert.ts` L22-L56). This bounds restore cost; unattributed paths are left alone and reported.
5. Never half-restore. Refuse a whole-tree restore when the tree holds a nested repository at a captured path (Hermes `checkpoint_manager.py` L1376-L1385; OpenClaw `snapshot-inventory.ts` L406-L434). Check what our `git add -A` does with a nested `.git` directory.
6. Keep what no snapshot holds. Files over our size cap are not in a tree; restore must not delete them (Hermes `checkpoint_manager.py` L1429-L1452). Record skipped oversize files in the checkpoint event so the UI can warn before a restore that would delete one.
7. Validate every ref name and path that reaches git from a stored event or the protocol (OpenClaw's `^refs/openclaw/worker-results/[A-Za-z0-9-]+$` at `session-repository-checkpoint-refs.ts` L9-L23). The node decides: a restore request must name a tree id that exists in the node's own shadow repository, never a path.
8. Say in the UI what a restore does not cover, as Cline does: files outside the work tree, installed packages, databases, ignored files (the shadow repository honours ignore rules, so `node_modules` and `.env` are not captured), and anything a container wrote outside the bind-mounted worktree.

**4. Measure and bound checkpoint cost without giving up shell and container coverage.** (our `crates/node/src/git.rs`, `session.rs`; the design note's per-session worktree)

Where we are: a full `git add -A` scan twice per agentic turn. What the input shows: OpenCode's mitigations lower constants but each capture stays O(working tree); Zed removed its per-prompt version and its surviving recipe has the same `git add --all` shape (the reason is inferred); Hermes bounds cost by skipping above 50,000 files, which loses coverage silently; Cline has no cap; DeepSeek bounds only its display recorder. No project in the input bounds a full-coverage checkpoint without losing coverage or accepting O(tree), and nobody measured it.

Step 1, measure before changing anything. Add to the `Checkpoint` event: `{phase, scan_ms, git_calls, files_stat, files_changed, bytes_hashed, objects_written, index_reused, timed_out, coverage}`. Add a node benchmark over synthetic trees (for example 1k, 10k, 100k, 500k files; warm and cold page cache; 0, 10 and 1,000 changed files) and run it in CI as a regression guard. Field data comes from the event, aggregated per node. Set budgets after seeing the numbers; this report does not propose a number it cannot cite.

Step 2, apply the cheap wins the input shows (all `unchecked`; measure each):
- Keep the private index between turns, seeded from the user's index; set `core.untrackedCache=true`, `feature.manyFiles=true`, index v4, `core.fsmonitor=false` (OpenCode `git.ts` L28-L42, L503-L520; Cline persists its scratch index so git's stat cache skips unchanged files, `checkpoint-hooks.ts` L249-L338). Check whether our private index persists across turns today.
- Write the index to a temp file and rename it; rebuild from the user's index if unreadable (OpenCode `git.ts` L704-L714).
- Coalesce concurrent captures and return the previous tree when nothing changed (OpenCode `git.ts` L760-L762, L845-L871).
- Take the pre-turn snapshot lazily, just before the first write-class or shell tool of the turn; record "no checkpoint: read-only turn" otherwise. Hermes and Cline tie the snapshot to the first write-class call or first model call; our turn-start snapshot pays the scan for read-only turns. The post-turn snapshot is needed only if a write-class or shell tool ran.
- Reuse the previous post-turn tree as the next pre-turn tree when the work tree cannot have changed between turns. That needs a cheap "unchanged" signal. Candidates not in the input and therefore unverified: git's built-in filesystem monitor (as distinct from a hook program, which stays off) or an inotify flag kept by the node. Test before relying on either.
- Batch git plumbing (Cline `checkpoint-diff.ts` L67-L105 uses two `git cat-file --batch` processes).

Step 3, bound it with a degradation ladder, not a skip. Every limit produces a recorded `coverage` value on the `Checkpoint` event; none may silently drop the snapshot (Hermes' 50,000-file skip is the pattern to avoid):
1. `full`: the scan finishes within the budget. Covers tool-made, shell-made and container-made changes to the bind-mounted worktree, because the node runs git on the host against the worktree and container writes appear as ordinary files (the design note's `keep-id` mount). This is the coverage Hermes gives up by excluding container backends.
2. `partial`: the scan exceeds the budget. Stop, then fall back to paths we know changed: paths written by our file tools (Warp derives its handoff set from the last 500 tool calls; Zed's action log keeps a baseline at first touch) plus a bounded walk for files newer than a marker. The walk is our proposal, not an input finding; test its cost. Record `coverage: partial` and show in the UI that shell-made changes may be missing for this turn.
3. `files_only`: the tool-reported fallback only. This is Zed's level: no shell coverage, no repository scan.
Expose the budget in `node.toml` (the node decides) with a conservative default and a hard per-command timeout. Our 120 s per git call is too long to sit inside a turn; DeepSeek uses 30 s.

Step 4, bound storage as well as time. `checkpoints_keep` counts turns; add a byte cap for the shadow repository and a per-file size cap; a file over the cap is excluded from the tree and listed in the event (so restore keeps it, item 3.6). Garbage-collect while keeping objects reachable from retained refs: OpenCode's store has no refs, so `git gc` would delete everything and it packs by explicit object list; Hermes turns `gc.auto` off and trims by snapshot count and bytes. Test pruning against a repository with refs and with alternates.

Step 5, test the alternates risk. OpenCode's inferred risk: if the user's repository runs `git gc --prune` after history is rewritten, objects that a snapshot tree references only through the alternate can disappear and a restore fails. Our shadow repository also uses alternates (own inventory, Checkpoints row). Write the test: snapshot, rewrite history and gc the user repository, restore. If it fails, copy referenced blobs into our own object directory at snapshot time (cost: disk) or stop using alternates for tracked content.

Step 6, keep per-turn pairs. OpenCode snapshots at the start and end of every model step; Cline once per user run; Hermes once per directory per turn. Do not go per-step unless measurement shows the scan is cheap.

**5. Resume: copy three things, keep one thing of ours.** (our log, resume path)
- Flush before effect, fail closed. DeepSeek awaits a durable flush before the model request and before every top-level tool body and treats a failed flush as an error (`session-checkpoint-policy/src/index.ts` L29-L83; `checked in review`). Check that our node's append is durable before tool dispatch and before the provider request; test a full disk.
- Close the open turn with a log event and fixed wording. We already close interrupted calls synthetically. Compare our wording with DeepSeek's ("outcome is unknown ... retry only if read-only or idempotent ... do not retry blindly", `repair.ts` L31-L41) and OpenClaw's prompt telling the model to check files, processes and external actions first (`session-restart-resume.ts` L78-L85). The closer must be an event so the fold is the same for live and recovered sessions.
- A bounded attempt counter on restart. OpenCode caps resume at 10 attempts and then ends the turn `aborted`; pi-durable counts crashes per task and resets on progress. A node that crashes on the same tool must not loop. Do not re-run a tool with a side effect (pi-durable defaults to `unsafe`).
- Keep our rule that the owning node is the only writer. Codex's and DeepSeek's locks are single-host mechanisms; OpenCode, goose and Pi have nothing across processes; OpenClaw's owner epoch (global, never reused, checked on every worker request) is the model if a session ever moves between nodes. We do not need it now; we will if restore on a different node or node replacement is built, so put a generation in the node-link binding before then.
- Fold cost: Codex replays only from the newest full compaction; DeepSeek loads the whole log; Pi rebuilds from all entries per projection. Start our fold at the newest compaction event. If folds are cached (DeepSeek's projection cache), bind the cache to the log's identity (`createdAt`, `cwd`, inherited count, format version).

**6. Retention, export and import.** (AUDIT D5 growth; `CLAUDE.md`: no secret in an event)
- Retention: OpenClaw is the only project with a disk-budget trigger (30 days, 5,000 entries, 10 GiB, oldest first, `enforce` by default). Codex compresses after 7 days and never deletes. Oh My Pi and Hermes keep forever unless the user opts in. For us: compress cold sessions and cap total bytes per node; deletion stays explicit (the design note's deletion flow).
- Export: refuse an export if redaction fails rather than falling back to raw (Hermes `sessions_cmd.py` L552-L554). Pi's HTML export embeds every branch plus the system prompt and tool schemas with no redaction (unchecked); DeepSeek's ZIP has none (`absence confirmed`); OpenCode's `--sanitize` redacts metadata, not message text.
- Import of other harnesses' transcripts: goose converts Claude Code, Codex rollout and Pi JSONL; Oh My Pi converts Claude Code and Codex; Cline converts Claude Code, Codex and OpenCode. If we copy it, write one converter per format into `history_imported` events with `inherited: true`, run every imported string through the same redactor as live output, and never resolve code or tool names from imported content. OpenHands' stored `tool_module_qualnames` is a load-time code-execution surface; our resume path must never resolve code from stored session data.

**7. Avoid, with the evidence.**
- Do not make the log optional. OpenCode's shipped server leaves it off and its revert commit deletes projection rows.
- Do not decide when to snapshot from command text (Hermes: misses `bash -c`, `find -delete`, `python -c`, `tee`; `2>&1` triggers needless snapshots).
- Do not build a restore filter from a ledger fed only by file tools (Hermes).
- Do not store checkpoints as refs in the user's repository without a cap (Cline: no size, count or total cap; refs live until the session is deleted).
- Do not rewrite a log in place on migration without a temp file and rename (Pi). We have no tag yet, so per `CLAUDE.md` the schema changes in place; decide before the first tag whether unknown events are refused unless marked ignorable (DeepSeek) or skipped (Codex).
- Do not describe a checkpoint the code does not take. Oh My Pi's `checkpoint` tool text says "git-based" while it only rewinds the conversation. Ours should say, per rewind, which of conversation and files were restored.
- Do not count on fork to carry files; no project does it well (item 8).

**8. Fork with files: use our own checkpoints.** (the design note says "a fork copies no files; files come back from the forge")
No input fork restores files from its own checkpoints; Cline's restore-with-messages is the closest. We hold a tree id per turn in the shadow repository. For a same-node fork, seed the child's new worktree from the tree recorded at the cut turn (`git read-tree` and checkout, as Zed's archive restore uses `read-tree`), and record `files: "checkpoint"` with `files_ref: <tree id>` on `SessionForked`. For a cross-node fork the objects are not on the child's node: keep the forge route as the default; a bundle transfer of the tree would be new work no input project demonstrates (`inferred`, untested). Decide `files` explicitly at fork time and show it; the failure to avoid is a forked chat whose history says files exist that the child's worktree does not have.

**9. Reconnect cursor.** (small; uses PR 4's bounded link) OpenCode's first-party clients refetch everything on reconnect because the cursor API goes unused; DeepSeek's `follow` has no cursor in the request (docs and code disagree) and an unbounded listener queue; OpenHands and Cline resume by sequence number. Our `Resume{after}` exists on the node link; give the browser stream the same contiguous-`seq` cursor and OpenCode's order: subscribe first, capture a watermark, replay to it, emit a sync marker, then follow (`bus.ts` L765-L878, `checked in review`). Add a per-client bounded queue with a lag disconnect; it composes with PR 4's back-pressure.

### Context: prompts, compaction, instruction files, memory

From [round2/synth/07-context-compaction-memory.md](round2/synth/07-context-compaction-memory.md).

State assumed: `main` has no compaction (inventory not re-read here); pull request 7 (open, not merged) has compaction keyed to the model's context window, one overflow recovery, provider retry with backoff, and a summariser that reads in pieces; pull request 4 adds the Landlock shell sandbox, an allow-listed child environment and secret redaction; pull request 6 the agent roster; the design note proposes per-turn provider key grants. Claims about our code come from the task description, not from reading it.

##### 6.1 What pull request 7 should still adopt, ranked

**1. A cache-preserving summary request, with the "pieces" reader kept only as a lower tier.** Copy OpenCode (`packages/core/src/session/compaction.ts`: the request builder around L451-L470 and L853-L870 sends the primary agent's own system parts, tools and history plus one final user message with the summary prompt) and DeepSeek harness (`packages/compaction/compaction-basic/src/summarizer.ts` L120-L181: same prefix, instruction last, full tool schemas still sent, tool calls in the reply dropped, empty, image-only or `max-tokens`-truncated output an error). Zed (`crates/agent/src/thread.rs` L4764-L4835) keeps the tools with `tool_choice: none` for the same reason. Three conditions follow from the code: leave headroom for prompt plus output in the trigger (OpenCode's buffer, DeepSeek's 65,536 that also equals the summary `maxTokens`); only expect a cache hit with the same model and an unexpired cache (OpenCode ties its precompute to the cache lifetime minus 30 s); and give the fallback tiers below, because the full-prefix request is the largest request you make. OpenCode's `deliver` ladder is the model for tiers (unchanged request, history minus the kept tail, then a flattened transcript with progressive clipping: tool output to 2,000 chars, then tool output dropped, then user text only, then oldest exchanges omitted). A reader that summarises in pieces fits that last tier. None of the findings describe a map-then-merge chunked summariser as a primary path: Hermes sends exactly one call with an evenly sampled input and elision markers, Pi splits only a split turn into two summaries, Cline refuses its own agentic summariser during overflow. Treat the pieces reader as a hypothesis to test, not an established pattern (absence not confirmed). Pi (`cacheRetention: "none"`, history re-sent and re-billed uncached) and goose (cache writes off) are the examples not to follow. Related to our design: a summariser override model needs its own per-turn key grant (inferred from the design note).

**2. A deterministic fallback after an overflow, so a context-length error never ends in a dead turn.** Copy Cline: `sdk/packages/core/src/extensions/context/basic-compaction.ts` L452-L512 (`basic`: no model call, keeps every typed user prompt, collapses dropped tool work into a notice, keeps the last three assistant texts, strips stale attachments) and `sdk/packages/agents/src/agent-runtime.ts` L1378-L1423 (one forced compaction and one retry; during overflow `agentic` is never used because its own request could overflow). Add Hermes' rule from `agent/context_compressor.py` L4080-L4175: terminal failures (network, truncation, empty content, auth) abort and leave the session unchanged instead of inserting a placeholder, other failures insert a deterministic summary of user asks, actions, files and errors (L3502-L3510). Codex's oldest-first drop inside compaction (`core/src/compact.rs` L347-L359) is the last tier. Do not copy Codex's sampling path (the turn ends; `core/src/session/turn.rs` L2028-L2031), Pi (a second failure stops) or goose (the fourth overflow gives up). Order for us: model summary (cache-preserving), shrink request, deterministic `basic`, drop oldest, typed error. Test: a summary request that itself returns "too long" must end in the deterministic tier with the history shrunk, never in an error loop.

**3. A check that compaction made the context smaller, at two levels.** DeepSeek `region.ts` L417-L468: a summary is rejected unless its framed estimate is strictly smaller than the span it replaces, and automatic compaction also demands the surface be unchanged during summarisation. Cline `agent-runtime.ts` L2254-L2306: the retry is refused unless the compacted request shrank. Oh My Pi falls through to the next method when a summary would not reduce context (`packages/coding-agent/src/session/session-maintenance.ts`; the fall-through ranges are listed in its account in section 2). Add Hermes' breaker (`context_compressor.py` L3015-L3022): two compactions that did not bring usage under the threshold stop the automatic attempts for 300 s, which prevents a compaction loop. Codex, OpenCode, Pi, goose and Zed do not check (OpenCode checks only the summariser's input). Test: a summary longer than the kept span, and a history where the kept tail alone is above the threshold (Pi keeps a single recent tool result over 20,000 tokens in full, so its tail can still exceed the trigger).

**4. A self-correcting estimator.** Copy Cline `sdk/packages/core/src/extensions/context/compaction.ts` L211-L230: scale the budget by the ratio of the provider's real count for the previous request to the estimate when the real count is higher (cap 4x). Add Pi's rule (`packages/coding-agent/src/core/compaction/compaction.ts` L298-L349): ignore provider usage recorded before the last compaction. Add Oh My Pi's floor (`packages/agent/src/compaction/compaction.ts` L355-L381): never let the count fall below a local estimate of the stored conversation. Count images explicitly (OpenCode: 1,500 per image and 2,000 per PDF; Pi: 4,800 characters per image) and score a request-too-large (413) in bytes, not tokens, as Hermes does (`agent/turn_overflow.py` L219-L275). Adopt a provider-reported limit from an error message, never a guessed one (L315-L340). Decide what an unknown window does: OpenCode's infinite ceiling means auto compaction never fires and is the worst default; Cline's 128,000 fallback for manual and overflow is safer.

**5. Recover from overflow by compacting and restarting the same step, once, and hide the failed attempt.** OpenCode (`session/runner/step.ts` L186-L197, `llm.ts` L316-L322: once per step, only if no output started) and Pi (`agent-session.ts` L3132-L3202: `context_edit` entries hide the failed assistant message from the model while the raw log keeps it; a second failure stops with a clear message). In an append-only log this is the natural form: the failed attempt stays recorded, the projection hides it.

**6. Check the summariser's window, but not the way Hermes does it.** Hermes (`agent/conversation_compression_feasibility.py` L64-L127, `conversation_compression.py` L2028-L2117) probes the summariser's window, lowers the trigger to it and refuses below a minimum; the reviewer noted the lowered trigger equals the whole summariser window, leaving no headroom for prompt and output. Oh My Pi's candidate order (configured model, current, role models, largest context) avoids the check by selection. We should do both: pick a summariser whose window is at least the history plus the prompt plus output, else go to the fitted-request tiers in item 1.

**7. Record compaction so a one-writer log can replay it, and test the crash case.** Our log is append-only with contiguous `seq`, written by the owning node. Pi's entry (`firstKeptEntryId`, `tokensBefore`, system-prompt snapshot; written only after the summary exists; the model context is a pure function of the entries) and Codex's `replacement_history` base (resume replays only the suffix; `core/src/session/rollout_reconstruction.rs` L205-L260) fit that. DeepSeek's start and end events show the cost of a two-event protocol: a start with no end blocks further compaction until a later `session/end-seed`, and a summary committed with a failed close leaves the surface replaced and the lock open. Write one event after the summary exists, or make the closing event idempotent. Re-base instruction state at compaction (OpenCode `projector.ts` L1119-L1122 and `instruction-state.ts` L126-L140; Codex renders everything in a fresh window) and test that AGENTS.md, the environment block and nested instruction files are present after compaction; DeepSeek's compaction path does not re-inject them and goose's subdirectory hints drop out.

**8. Keep the prompt prefix stable with keyed sources and logged diffs.** Prompt and tool changes as transcript entries is the shape that fits our event log. Copy OpenCode (`session/instruction-state.ts` L49-L89, `message-updater.ts` L154-L166: a hash per canonical-JSON value, a durable event holding the delta and the already-rendered text) and Codex (`core/src/session/world_state.rs`, `core/src/session/mod.rs` L4180-L4207: stable section ids, snapshot, render the diff; fresh windows render everything). Avoid Codex's pile of legacy-fragment matchers and, from OpenCode, the model-id substring match without a provider check.

**9. Instruction files: read on the node, gated by the node.** The node's `node.toml` decides. Copy Codex's `core/src/agents_md.rs`: an untrusted project returns nothing (L65-L67), root markers are read only from non-project layers (L207-L212), one combined budget with a warning on truncation (L69-L76, L148-L182). Add a per-file cap (Hermes: 20,000 chars or 6% of window; OpenClaw: 20,000 and 60,000 with a digest of must/never lines so a cut file still keeps its rules). Avoid Pi's rule (no cap, walk to the filesystem root, loaded regardless of trust, so a hostile repository's AGENTS.md steers the model and the model can then write `.pi/extensions/x.ts`) and Codex's empty-override shadowing. Hermes' regex scan blocks a whole file on one false positive; use a scan as a warning, not as the gate. Whatever the file says, nothing in it may widen node policy (invariant).

**10. Tool output.** Keep what we have (capped file tools, bounded node link); if long sessions need it, Oh My Pi's cache-aware prune (`context-settings.ts`, `session-maintenance.ts` L817-L852) and OpenClaw's frozen projection recorded in the transcript are the designs that do not break the prefix cache. DeepSeek's spill fails open to inline on a full disk (`spill-policy/src/index.ts` L95-L131); fail closed with a truncation notice instead.

##### 6.2 What the memory designs here imply for four scopes and approval

We do not know our four scopes beyond "human approval for shared memory". The input implies:

1. **Approval must be enforced in code, not in the prompt.** goose's "always confirm with the user before saving" is prompt-only and unenforced (`crates/goose-mcp/src/memory/mod.rs` L118-L172; checked in review). Hermes enforces it in the write path (`tools/write_approval.py` L201-L248, `tools/memory_tool.py` L64-L90) but the gate fails open if its module cannot be imported and a disk failure while staging loses the write. Shared memory must fail closed: no staged record, no write.
2. **Approve what the person saw.** Hermes pins the matched entry at staging time for `replace` and `remove`, so approval applies to the text the person reviewed. Its skills replay has no base hash, version or mtime check (corrected in review: a staged targeted `patch` still needs its `old_string` to match, so a stale patch normally fails, but full rewrites are unguarded). Our staged write should carry the base version of the entry (event seq or content hash) and be refused if it moved.
3. **Unattended writers must not remove.** Hermes' background review can stage `patch` and `remove` but not apply them; with approval on, every background write is staged. Codex's consolidation and OpenClaw's dreaming write without a person; OpenClaw bounds it with a rule that a plan removing more than 25% of prior entries is rejected. For shared scopes copy the staged-only rule; for personal scopes the 25% bound is a cheap second guard.
4. **A background writer's sandbox is not enough, and the shell can bypass the store.** Codex's consolidation agent has approvals `Never`, no network and memory-root-only writes, but inherits an unsandboxed profile if the parent is unsandboxed (`memories/write/src/phase2.rs` L339-L359). Hermes withholds `terminal` from the curator because a shell `mv` would leave no ledger entry, and states that `terminal` writes bypass its ledger and lock. On a node, the agent's shell can write wherever pull request 4's Landlock policy allows: shared and personal memory stores must live outside the shell's writable set and be written only through the approval path (inferred from our sandbox and the two findings).
5. **Chat-time writes are an injection path into persistent memory.** Codex's ad-hoc note tool and goose's memory tool both let model output become later system-prompt text. Hermes strict-scans writes and blocks matching entries from the prompt snapshot (the live list keeps raw text so the user can delete it); Codex marks threads that touched MCP or web content "polluted" and excludes them from extraction (`core/src/stream_events_utils.rs` L176-L192). Both ideas apply: scan on write, and exclude threads with external content from automatic extraction.
6. **Rollback needs a ledger that is a gate, not telemetry.** Hermes' ledger (`tools/skill_ledger.py` L298-L323, L557-L633) stores before and after manifests with sha256, content-addressed blobs and a `pre-rollback` entry, restores or deletes accordingly, and does nothing if the safety capture fails. But a failed ledger append is logged and the mutation proceeds, so the audit trail can have gaps, and sha256 is content addressing, not a hash chain. With an event log we can make the memory write and its inverse the same event (inferred): rollback is a new compensating event and cannot be skipped.
7. **Scope filtering belongs in the prompt builder.** OpenClaw drops `MEMORY.md` for sub-agent, cron, group and channel sessions (`src/agents/workspace.ts` L896-L917). Our four scopes need the same filter per chat type, tested for the "chats are private to their owner" invariant.
8. **Keep memory small and frozen per session.** Hermes caps notes at 2,200 and 1,375 chars, refuses an `add` over the limit and returns the live entries so the model consolidates; the prompt gets a per-session snapshot refreshed at compaction. goose loads global memories once at construction. Codex injects only a 2,500-token summary and lets the model read detail on demand. All three keep the prefix stable.
9. **Session search is not a substitute.** Hermes' FTS5 `session_search` finds rows archived by compaction without an LLM (`tools/session_search_tool.py`), which means archived history stays reachable after a summary; chats being private per owner means any such search must be scoped to the caller.

##### 6.3 What to check or test in our own code

- Compaction with the summary larger than the span; with an unknown window; with images dominating the size; with a tool result bigger than the kept tail.
- The summariser itself overflowing; the overflow retry landing again on overflow (stops, deterministic tier runs once).
- Crash between summary and log write; restart with a half-recorded compaction.
- After compaction: instruction files, environment and memory snapshot present; no stale diff notices.
- Whether an edited AGENTS.md is re-read mid-session, and whether that is what we want (Codex: not until restart, safe against self-modification but surprising).
- Estimator error with provider-reported usage absent (first request, aborted response).

### Edit tools and the mechanics behind the scenes

From [round2/synth/08-edit-tools-mechanics.md](round2/synth/08-edit-tools-mechanics.md).

**Our state.** On `main`: `edit` is an exact replace that must match once unless `replace_all`, with a diff card; no read-before-edit, no staleness check, no patch tool, no diagnostics; `write` and `edit` open without following links and refuse `.git/**`; per-turn shadow-repo checkpoints scan the whole tree; bash has its own process group, SIGTERM then SIGKILL after 2 s, 600 s default, 3600 s cap on the model's `timeout_secs`, allow-listed env, 30,000 characters head and tail, filtered full text retrievable with `tool_output`. Pull request 4 adds file-tool caps and a Landlock shell; pull request 6 adds the new agent roster (relevant to which agents get edit tools); pull request 7 adds compaction keyed to the window. The design note's worktree-per-session reduces outside edits but does not remove them (shell commands and the user still write).

Ranked by importance. Items 1 and 2 are the design the task asked for.

##### 6.1 A seen-version guard in code, on by default (highest value)

**Evidence.** Seven of the eleven read implementations that have an edit tool have no staleness check (Codex, OpenCode, OpenClaw, Pi, goose, Cline, DeepSeek in the shipped config). The two soft ones only warn (Hermes `patch`, Zed). The hard ones are OpenCodex (`expectedSha256` on `write_file`), Hermes `write_file`, Oh My Pi hashline, and DeepSeek's plugin, which ships disabled while its prompt still demands a read. A guard that is only in the prompt is not a guard (Oh My Pi `replace`, Zed, DeepSeek).

**Design (ours, assembled from the sources; not from one project).**

1. The node keeps a per-session table, keyed by canonical path (after the same symlink-safe resolution `edit` already does): `{ sha256 of normalised content, dev:ino:size:mtime_ns:ctime_ns, ranges shown in full, a bounded copy of the shown text }`. It is written by `read` (and by `grep` or `glob` only as "partial: lines N..M"), and rewritten after every successful agent write with the new hash. Version token shape from DeepSeek `fsio.ts` (`dev:ino:size:mtimeNs:ctimeNs`); hash from OpenCodex and Oh My Pi. Use at least 128 bits of hash, not Oh My Pi's 16 (Oh My Pi's own review: a live collision bypasses recovery).
2. `edit` and `write`-over-existing take the per-path lock first, then check the version **inside the lock, before matching** (DeepSeek `fs-local/src/index.ts#L250-L277`: stale read reports `FS_STALE_VERSION`, not a misleading not-found). Creating a new file needs no prior read (DeepSeek, Hermes).
3. `edit` with no table entry: refuse with "read this file first" and return nothing else (enforced in code; Hermes `write_file` and Oh My Pi hashline do this; Oh My Pi `replace` does not). `write` over an existing file needs a **full** read, not a partial one (Hermes `file_tools_write_guards.py`).
4. Table-entry present, content hash differs from live (file changed since the model last saw it). **What to do on a stale file, in order:**
   a. Re-run the match against the live text. If the matched span is not unique, or does not match exactly, refuse and return the failure with the current nearby lines (OpenCode `missHint`, gated by read permission).
   b. If it matches uniquely and the matched span is byte-identical to the same span in the retained snapshot (the model saw exactly these lines), apply, and say in the result that the file had changed outside the session and which line ranges differ. This is the string-replace analogue of Oh My Pi's "provably safe" rebase, without the line-diff machinery: a content match has no line numbers to remap.
   c. If the span is not in the snapshot (the model never saw it, or the snapshot was evicted), refuse and return a bounded unified diff between the snapshot and live content. That saves the re-read, which Hermes and Pi make the model do.
   d. With no snapshot (hash only), refuse on any mismatch.
   Why not warn-only: Hermes and Zed show the warning being the only effect; a same-mtime change passes Hermes' check. Why not Oh My Pi's full rebase: it is 565 lines plus a tree-sitter dependency, and its own research file recommends rejecting on mismatch until retries are shown to be wasted.
5. Persist what the table needs in the event log: each `read` tool-result event carries `path`, `sha256`, `ranges`. On node restart the table is rebuilt by folding the log (our invariant: the UI and state are a fold over the log). Oh My Pi's and DeepSeek's tables are memory-only, and after a restart their provenance is gone. The snapshot *text* is not logged; after a restart the guard degrades to branch (d), which is safe.
6. **On compaction, clear "ranges shown in full"** (keep hashes). After compaction the model no longer has the text the table says it saw. This is `inferred` from Oh My Pi's TTSR note (fire-once state survives compaction while rule text does not); none of the files test it for read tables. Relates to pull request 7: clear in the same change that lands compaction.
7. Bound the table: Oh My Pi uses 4 versions per path, 256 paths, 64 MiB, files over 4 MiB not snapshotted. Our idle budget is 50 MB RSS, so keep the snapshot text on disk in the session scratch directory and hold only hashes in memory, or cap memory copies at a few MiB.
8. Track our own writes so the agent is not refused for its own edits: after a successful write the entry is replaced with the new hash and the shown range is the touched lines (Hermes' ledger records hashes after its own writes for the same reason). A formatter or shell `sed -i` that changes the file will make the next edit refuse and return the diff. That is the intended behaviour.

**Line-anchored edits.** For a string-match `edit` the match itself proves the model knows the text, so Oh My Pi's seen-line guard adds little. It becomes necessary only if we add line-number anchors, where it is the real per-line protection ("catches edits built from `grep -n` output or hallucinated line numbers"). Do not ship hashline in this round: Oh My Pi disables it for eight model families, its benchmark is the maintainer's own, and its stale-tag recovery is the largest code in the stack.

**Test that fails before and passes after.** (a) read file, change it from outside, `edit` an unrelated span: succeeds with a changed-outside note (4b); (b) same, `edit` a span that changed: refused with a diff (4c); (c) `edit` without any read: refused; (d) restart the node mid-session and `edit`: guard degrades safely (4d); (e) `write` over an existing file after only a partial `read`: refused.

##### 6.2 The match ladder to adopt, and its order

Tier-first, as Codex does (the corrected review text: tiers are tried in order across the whole file). Order, each tier must give **exactly one** match or the next tier is tried; more than one match at the winning tier is an error that lists line numbers (Zed) and never a first-wins (Codex):

1. **Exact** bytes, on an LF view with offsets mapped back (OpenCode `matchView`; DeepSeek CRLF collapse).
2. **Trailing-whitespace-insensitive, line-wise** (Codex tier 2; OpenCode tier 3).
3. **Unicode punctuation fold, one character to one character** (curly quotes to straight, en and em dashes and minus to hyphen, NBSP and exotic spaces to space) so offsets stay valid (OpenCode `normalizeForMatch`; Codex tier 4). Avoid NFKC (Pi, OpenClaw): it changes length.

Stop there for `edit`. Not in the first version: a trim-both-sides rung (it ignores indentation; OpenCode's `edit` deliberately omits it and its own finding says that is where other harnesses had corruption bugs), and any similarity rung. If similarity is ever added, require: a unique candidate, a margin over the second (Oh My Pi 0.08), never with `replace_all` (Hermes), and a flag in the result.

Rules that go with the ladder:
- **Report the tier that matched** in the result text and in the event ("matched after Unicode normalisation"). Only Cline reports fuzz; Codex, Pi and OpenCode give no sign, and Pi's fuzzy mode rewrites whole lines silently.
- **Fuzzy finds, it does not rewrite**: splice the replacement into the original bytes (OpenClaw). Do not copy Pi's `widenReplacementsToLines`.
- **Line endings and BOM**: preserve per line (OpenCode `withRegionEndings`; Codex first-seen-ending for inserted lines). Drop Codex's habit of adding a trailing newline.
- **Multi-edit batch, one call**: match every `oldText` against the **original** content, require disjoint spans, apply in reverse order, write once (Pi `edit-diff.ts#L343-L406`). All-or-nothing in memory is cheap and removes partial states inside one file. Add `edits[]` next to the single-edit form (Pi `prepareEditArguments` accepts both).
- **Failure hints**: closest lines gated by "read is already permitted" (OpenCode); "newString already present, edit may already be applied" (OpenCode); strip a pasted `N|` gutter (Hermes); escalate the hint at the third miss on the same path (Hermes); refuse the third identical no-op (Oh My Pi `NOOP_HARD_LIMIT`). Hash the outcome, not just the arguments (OpenClaw).
- **Argument repair before validation**: a `prepare_arguments` step that accepts stringified arrays and a single object where an array is expected (Pi, OpenCode), and tells the model what ran (OpenCode appends a note).

**Patch tool**: not now. Route by model family only if GPT-family models are in scope (OpenCode and Cline both do), and keep a JSON-schema fallback because Codex's freeform grammar needs custom-tool support. When added: copy Codex's verify-all-then-apply and fix its two gaps (6.3).

##### 6.3 Atomic writes and multi-file apply

1. Per-file: write to a temp file in the same directory, `fsync`, rename over the target, preserving mode (DeepSeek, OpenCodex). Our `fs.rs` opens without following links, which must be kept: open the temp with `O_EXCL`, rename without following (OpenCodex adds `fstat` identity and refuses files with more than one hard link).
2. Per-path async mutex for edit and write on the same canonical path (Pi, OpenClaw, DeepSeek, OpenCode), and make the **version check run inside it**.
3. If a patch tool is added: bind approval to content. At verification time record `sha256` of each target; at write time recheck under the lock and refuse on mismatch (Codex re-parses and re-matches at apply time with no hash binding). Write all temps first, then rename in order; on a failed rename restore the already-renamed targets from the originals kept in memory. State plainly that this is not crash-atomic across files, and log the applied delta as an event (Codex's `AppliedPatchDelta` is the shape).
4. Verify after write like OpenClaw (re-read and compare), at least in debug builds; it catches a silent short write.
5. Scheduling: edits are exclusive, reads shared (Codex `RwLock` scheme, flagged per tool, fail-closed on a throwing classifier as in DeepSeek and Oh My Pi). Set the flag from effects ("writes the workspace"), not from the name "read-only"; shell is exclusive unless proven read-only.

##### 6.4 An edit ledger for review, rewind and cheaper checkpoints

**Problem (evidence).** Our checkpoint is `git add -A` into a private index every agentic turn. Zed deleted that design (per-prompt checkpoints hung large repositories; reason `inferred`) and replaced it with an in-memory baseline per touched file. Hermes bounds it (skip over 50,000 files, drop files over 10 MB, 20 snapshots, 500 MB) and is off by default. OpenCode keeps O(tree) cost per step and no retention. Cline has no caps.

**Do.**
- Reuse the seen-version table (6.1) as an edit ledger: per path, the baseline at first agent touch (hash plus a blob), then each own write's hash. It feeds three things: the staleness guard, a per-turn "files changed" view with a real diff (our UI shows only an `edit` call's old and new strings today), and Hermes-style safe restore ("restore only if the file still has the hash we wrote").
- Fix Hermes' hole: its safe-restore ledger sees only file-tool writes, so shell-changed files are silently skipped or silently restored depending on ledger state. Our node sees every `bash` call, so for any shell call that is not provably read-only, diff the tree (or the paths in the Landlock writable set from pull request 4) before and after and record those paths in the ledger too. Do not decide by a command-text regex (Hermes' regex misses `find -delete`, `tee`, `python -c`).
- Use the full-tree snapshot only when a turn contained a write-class or non-read-only shell call. A turn of reads and a pure `edit` can use the ledger alone.
- From OpenCode: seed the shadow index from the real index so tracked files are not rehashed; write the index to a temp file and rename; coalesce concurrent captures; set `fsmonitor=false`, `untrackedCache=true`, `feature.manyFiles=true`. Add what OpenCode lacks: a retention policy. From OpenCode also: skip untracked files over 2 MiB. From Hermes: set `core.hooksPath` and `core.fsmonitor` explicitly rather than rely on the store being bare (Hermes' own reviewer left that inference untested).
- From Cline's restore transaction: snapshot the present before destroying it, refuse if `HEAD` moved, and refuse (do not half-restore) when a nested repository is present (Hermes refuses; Cline's untracked-files clean runs only when untracked files were captured).
- Decide explicitly whether "rewind to message N" reverts only edits after N. Zed reverts everything unreviewed (corrected in review).

**Check in our code.** Run a checkpoint on a repository with a very large tree and time it (Round 1's own inventory notes cost on a large repository was not measured); confirm a turn that only read files takes no snapshot.

##### 6.5 Shell mechanics: gaps relative to what the sources do

1. **Orphans that leave the group.** Our group kill misses a child that calls `setsid` (goose, Cline confirmed this class; Oh My Pi `inferred`). Pull request 4's Landlock does not contain it. Options: a PID namespace or cgroup per command on Linux; or Oh My Pi's walk-then-re-walk before SIGKILL as a cheaper step. Check whether we should kill leftovers after the command's own process exits, unless the call was explicitly background (goose leaves them alive on purpose; Cline's detach does too; both are choices we should make deliberately, not inherit).
2. **Model-supplied timeout.** Ours is already capped at 3,600 s and cannot be 0. Keep it. goose and Oh My Pi let the model pass 0 for "no deadline", and Pi has no default.
3. **Cap while reading.** goose reads output into an unbounded `Vec` first. Our stream budget on `main` does bound it; keep the ring-buffer-plus-spill shape (Codex 1 MiB head and tail per process; Oh My Pi 20 KiB head plus 20 KiB tail with a 16 MiB artifact cap; Pi tail accumulator). Tail-biased previews suit build logs (goose, Pi); head and tail suit both (Cline 24k/24k, Oh My Pi).
4. **Background jobs, if added.** Promote an over-limit foreground call to a tracked background job instead of refusing it (Hermes avoids refusal loops). Pair each job's PID with a process-start token against PID reuse (Cline). Put them in a per-session table with a cap and LRU eviction that protects the most recent (Codex: 64, 8 protected). A backgrounded job gets no further approval check in Oh My Pi; ours should keep the node's policy and cancel path attached. Decide what survives a node restart; none of the sources says it is "nothing", but our event log means a restart must be visible as an event.
5. **Node crash.** goose's shell has no `PR_SET_PDEATHSIG`; its MCP servers do. `prctl` from a `pre_exec` closure needs `unsafe`, which our workspace forbids. Check for a crate that does this without our own `unsafe`, or use a supervisor process as Hermes does ("owner-death supervisor"); until then children survive a SIGKILL of the node, as with Pi.
6. **Do not decide safety from command text.** Hermes' destructive-command regex (snapshot decisions) and `skill_lock.py` (write lock), Cline's plan-mode blacklist (its header admits `python -c` escapes) are advisory. Use effects: Landlock writable set, tool class, ledger.

##### 6.6 Post-edit step: parse delta, not formatter

- Run a parse check on the node and report **only errors the edit introduced** (Hermes "pre-existing errors filtered out"; its line-set diff misfires when a linter renumbers lines, so compare error count and error node kinds, not line text). We already use tree-sitter for `outline`; no new dependency. Skip files whose language has no grammar.
- Do not run a formatter after the agent's edit by default. OpenCode does and then returns a diff against the formatted file; the model's picture of the file no longer matches what it wrote, so the version table must take the post-format hash or the next edit refuses.
- LSP diagnostics: no verified implementation to copy (Hermes' LSP layer and Oh My Pi's were not read). Leave it.

##### 6.7 Tool-result caps and loop guards

- Scale per-result caps with the window (OpenClaw: min of 30% of the window and 16k/32k/64k characters) instead of our fixed 30,000; this follows pull request 7's window awareness.
- Exempt `read` from spill so reading a spilled output is not re-spilled (Hermes pins `read_file` to infinity; DeepSeek exempts `read`); give the model a handle it can page (`tool_output` already does; Cline's `cline://cache/...` and OpenCode's path are alternatives).
- Do not fail open when spilling fails (DeepSeek keeps the full text inline on any error). Truncate inline instead.
- Add a generic ceiling in the dispatcher in addition to per-tool limits (Pi's extension tools have none).
- Refuse every tool call in a message that ended with `length` (Pi `failToolCallsFromTruncatedMessage`). Check our `agent-native` loop for this today: it is the cheapest safety fix in this list. Also fail after three consecutive `length` stops (OpenCode).
- Loop guards at tool level: copy the three-identical-no-op rule, an always-on identical-error breaker at 3 (OpenClaw), and Hermes' dual default (warn when attended, halt when unattended), which fits our routines. Hash outcomes. Do not copy goose's inert inspector or Hermes' regex tripwires.

##### 6.8 Stream rules (TTSR): later, and only in this shape

The feature freeze applies. If built after Gate 3: run rules on the node with the Rust `regex` crate (linear time) so an untrusted repository's rule file cannot stall a session; make the retry bound explicit (fire once per session, as Oh My Pi; there is no counter); because our log is append-only and one-writer, record the abort as an event that supersedes the partial assistant message rather than deleting it (Oh My Pi's `contextMode: discard` drops the message); reset the fire-once state on compaction or pin the rule text, because Oh My Pi keeps suppression without the text (`inferred`). Rules come from project files, so they are untrusted input: label the injected excerpt as data as Oh My Pi's template does. Skip judged rules (an extra model call per output).

##### 6.9 What not to copy

- **Similarity matching at low thresholds** (Cline 0.66, Zed 0.8 per line, Hermes 0.5 for block anchors): wrong-place edits on repetitive code; all four tools apply the best unique candidate silently except Cline.
- **First-match-wins with no uniqueness** (Codex).
- **A staleness check that only warns** (Hermes `patch`, Zed) and **a guard that ships disabled** (DeepSeek). Check what we ship, not what we document.
- **Prompt-only read-before-edit** (Oh My Pi `replace`, Zed, DeepSeek, goose).
- **A 16-bit tag as the only guard** (Oh My Pi).
- **NFKC folding with whole-line rewrite** (Pi).
- **Absolute paths accepted by edit tools** (Cline `editor` and `apply_patch` for absolute paths, goose `resolve_path`, Pi): our node-decides invariant and `workspace_roots` already forbid this; keep the identity-after-open check from OpenCodex.
- **`edit` that saves on tool completion even when a later edit failed** (Zed).
- **Rewinding a conversation without files, or files without the conversation, without saying which** (Oh My Pi conversation-only rewind, Zed whole-log reject).
- **Letting the model detach a command silently** (Oh My Pi 60 s auto-background gives the job no further check).

##### 6.10 Quick checks in our own tree (each is a test)

1. `edit` after an outside change to the same file (the case in 6.1).
2. Two parallel `edit` calls on one path: serialised, second sees the first's hash.
3. CRLF, BOM and a file with no trailing newline round-trip unchanged for an `edit` that touches the middle.
4. A response that stops with `length` and contains a half-written `write` call: nothing runs.
5. A shell call that changes a file the agent had read, then an `edit` on it: refused with a diff.
6. A command that calls `setsid` and sleeps: still running after cancel? (document the answer).
7. A turn with only reads: no snapshot taken.
8. Compaction followed by `edit` on a file read before the compaction.

### Sub-agents, orchestration, model roles, fallback and retry

From [round2/synth/09-subagents-models-retry.md](round2/synth/09-subagents-models-retry.md).

Ranked by importance. Where a point relies on an unchecked finding I say so. I have not seen the code of pull requests 4, 6 or 7; statements about them come from the task brief and the design note. `CLAUDE.md` says there is a feature freeze until Gate 3: aliases and slots are new features, so before building them, state the cost against `FIX_PLAN.md` and ask the owner whether to break the freeze. The permission and caps points below are fixes to existing behaviour, not new features.

##### 6.1 How a child's permissions should relate to its parent's

1. **Make the child's authority an intersection, computed on the node, and never a parameter the model or a role can raise.** Read Codex `core/src/agent/role.rs` (`AgentRoleOverrides`) and `child_config.rs` (`apply_spawn_agent_runtime_overrides`): the role is a patch type with narrowing fields only, and the parent's live approval policy and permission profile are re-applied last. Copy both halves: the type that cannot express widening, and the last-step re-apply as a second guard. Add Hermes's rule from `delegate_tool_toolsets.py`: explicit toolsets are intersected with the parent's. `main` already says a child gets the same approval mode and `tools:` narrows what is offered, with node policy deciding execution (`own-rizzy-harness.md`, section 7); that is the right shape. Relation to our state: the node's `node.toml` stays the ceiling for every child.
2. **Do not copy the three failure modes in the input.** OMP forces `yolo` for every child (`task/executor.ts`); Cline builds children with no policy and no approval callback; Warp types `--dangerously-skip-permissions` into a hidden pane and pre-writes trust into the vendor's config. In each, approving the delegation approves everything. The same logic applies to PR 4: a child must run under the same Landlock profile or tighter, an allow-listed environment no wider than the parent's, and capped file tools.
3. **Decide approval routing for children explicitly, with two cases.** A foreground child whose parent has a person watching: forward the child's approval request to the parent's chat owner (goose, `subagent_handler.rs`, remaps ids so the parent's user answers). A background or unattended child: reject, and write the child's effective policy into the child's own log at creation (DeepSeek `child-agent.ts`, `captureDelegatedPolicyOverrides`; the reviewer confirmed it). Either way, "allow for this turn" from PR 4 should not carry into a child by default: DeepSeek copies only the explicit session override and not one-shot grants, and that is the safer reading. That last step is my inference; test it.
4. **"Read-only" must be a tool filter plus a sandbox profile, not a description.** Two projects ship a read-only role that is not read-only in code: Codex `explorer.toml` is a zero-byte file (reviewer confirmed); OpenCode `explore` allows `shell *` and relies on a prompt. PR 6's Scout and Reviewer are read-only by roster. Test that each has no write tool and that its shell is absent or under a read-only Landlock profile, and that a call to a tool outside the agent's `tools:` list is refused at execution, not only hidden from the model (the inventory says node policy "still decides execution"; confirm it covers the agent's list). A table-driven test per roster entry (the inventory says one exists for the tools) should also assert the sandbox profile.
5. **Project-supplied agent files are prompt and permission injection.** goose reads `.claude/agents` and `.goose/agents`; OpenCode reads `.opencode`, `.claude`, `.agents` with no trust prompt (a repository file can set `permissions: [* allow]`); Cline reads workspace `.cline/agents` as tools; Pi's example gates project agents on an interactive confirm. Our agents live in the data directory and the master database, which is safer. Do not add repository-read agent files without a trust step, and never let such a file name a model, tools or `delegates`.
6. **External vendors' agents: not now.** The node's Landlock sandbox does not cover a vendor's process, and the input shows the permission request is advisory (Zed does not apply its deny list to ACP agents; Warp auto-allows). If it is ever added, copy DeepSeek's unattended table (decline every approval, empty user input, decline elicitation, empty client capabilities, `mcpServers: []`, environment scrubbed) and derive the vendor agent's mode from the node policy so a delegation cannot widen it (the opposite of DeepSeek's fixed per-provider config); test the "chat only" gap that goose has for Amp and Pi.

##### 6.2 Caps and defaults

Most harnesses converge on a short list; the first three rows are the minimum.

| Cap | Where to enforce | What the input suggests as a default | Source |
|---|---|---|---|
| Depth | In the spawn handler, from the persisted depth in the child's record (a resumed parent must not reset it) | 1 (Codex, OpenCode, goose, DeepSeek, Hermes) | DeepSeek `child-agent.ts` L49-L58 takes the max of persisted and runtime depth |
| Active children per parent, and concurrent children per node | Node, at admission; error names which limit | 4 per parent is already our default; per node 8 (DeepSeek) is the middle of the range 5 to 10 | OpenClaw `child-admission.ts` has three separate limits |
| Wall-clock and idle limit per child | Node supervisor | No project in the input has a good default: goose and Cline have none; Hermes marks stale at 450 s idle or 1,200 s in one tool; OMP parks idle children after 420 s | Hermes `delegate_tool.py` L107-L121 |
| Step cap per child | In the loop, keeping tool schemas and sending `tool_choice: none` | OpenCode `steps` (unset by default); goose 1,000; Hermes 250 | OpenCode `runner/llm.ts` L259-L281 |
| Token cap per child | Already present (200,000 combined, checked between steps so a step can overshoot) | Keep; add a per-step bound | `own-rizzy-harness.md` section 7 |
| Total sends including retries per turn | Node, a shared counter charged by every child | OpenCodex: 3 attempts per logical request, children inherit the parent's spend | OpenCodex finding 7 (unchecked) |

Also copy: the cap message tells the model what to do next ("Solve the task yourself", Codex; "wait or do the work directly", DeepSeek). Check our `depth is one (children have no delegates)` is a code check at spawn and not only a property of the built-in definitions: a custom agent with `delegates` used as a child is the case to test (OpenCode needs both a depth check and `subagent: deny` on its two built-in children).

##### 6.3 How to return results

1. **Give the child a finish tool with a default one-field schema, an optional caller schema, and a failure state.** goose `subagent_handler.rs` and `summon.rs` (`final_output`, default `{summary}`, failure with a reason and the last text); OMP `yield`; OpenClaw `structured_output`. This makes the parent's context cost predictable and answers "did it finish". The current `main` returns the child's final answer text.
2. **The result is a log event, and the parent folds it.** The child's session is already a session record on the node. Append one result event to the parent's log with the child's session id, a typed status and exit reason (Hermes `exit_reason`, `truncated`), and a capped summary: Hermes caps at 24,000 characters (head 75%, tail 25%), redacts, and spills the rest to a file. This keeps the single-writer invariant: the same node's session actor writes both.
3. **Delivery is queue-only at the next step boundary, with an idempotency key.** Codex queues a note with `trigger_turn = false`; DeepSeek wakes an idle parent or steers a busy one; OpenClaw uses an idempotency key and never replays an ambiguous delivery. Because our node may restart mid-operation, key the result event by the child session id so a retry cannot deliver twice; goose re-derives pending children from persisted "delivered" notes, and OpenCode fails a running `subagent` tool call after a crash so the model can continue the child by id. Copy one of the two (goose's is the closer fit for an event log).
4. **Mark the result as untrusted data and as an unverified self-report.** OpenClaw wraps it as data; Hermes tells the parent summaries are "SELF-REPORTS, not verified facts" and asks for a verifiable handle. Our inventory records that sub-agent task text enters the web-fetch "observed" set (AUDIT L11, `unchecked` here); do not let a child's result widen that set.
5. **Strip or validate any in-band marker that triggers orchestration** (goose strips `subagent_session_id` from external MCP output and verifies ownership). If a result event can start something, only the node's own session actor may write it.

##### 6.4 The alias and slot data model, and from whom

The design note already has a per-workspace `model_aliases` table (ordered list of `<provider>::<raw>`), `::` to tell a model from an alias, built-ins `small`, `vision`, `long-context`, an alias expanding to its models in order, a chain capped by `MAX_FALLBACKS`, and slots `title`, `summary`, `vision_bridge` with a capability check at save time. Measured against the input, this is close to the right shape. What to copy or check:

1. **Hermes `auxiliary_task_config.py` and `config_defaults.py` for the slot shape.** One block per side task, all the same shape: `provider`, `model`, credentials, `timeout`, `reasoning_effort`, `fallback_chain`. Default every slot to "the turn's own model"; walk a global discovery chain only when the user configured no policy. Add `timeout` and reasoning effort to our slot record: a title call on a reasoning model at full effort is the cost OpenCode avoids with a minimal-reasoning variant.
2. **Keep the route as one unit** (Hermes: provider, model, base URL and key travel together, so one vendor's key is never sent to another's endpoint). Our `provider::raw` form does this for the model; check that a slot or child model on a different provider than the turn's gets its own key grant on the node, and that the node never receives a key the turn did not need (this follows the design note's "granted per turn"; I did not verify the code).
3. **OpenClaw's option type for "strict".** A defined empty fallback list (`fallbacksOverride = []`) is how a user's explicit model choice fails instead of silently switching, and the primary is not appended. Model this as `Option<Vec<_>>`, not a default value; it distinguishes unset from none. Unchecked in review; the call sites were not traced.
4. **Resolution order for a child's model.** Codex: tool argument, config, parent, with a role pin overriding all; goose: recipe, env, parameter, config, parent, provider default (and a quirk where a mismatched provider silently falls through). Recommended for us: the agent definition's model or alias, else the parent's current model. Whether to let the model pass a model argument is a cost question: Codex allows it (validated against the model list, error lists up to 5 models); DeepSeek restricts it to the parent's route unless policy allows more. I would not give the model a model argument at first (my judgement, not an input finding).
5. **Do not hard-code model names or families.** OMP's role priority lists, OpenCode's title families and OpenHands's embedded benchmark table age quickly. Our design (empty built-in aliases proposed from reported capabilities) already avoids it; keep the proposal a suggestion the admin confirms.
6. **Slot names as strings in a known set, validated by the master.** OMP went from nine to 16 roles and accepts custom names; Hermes has 19 slots and plugins can add more. We need three slots now plus a child-model role; do not bake the set into the protocol as an enum.
7. **Summary slot window rule.** The design skips a summary model whose window is smaller than the main model's. Hermes instead lowers the compaction trigger to the summary model's window (reviewer confirmed it leaves no headroom for template and output); skipping is simpler and keeps the trigger.
8. **A slot failure must not fail the turn.** OpenCode retries a failed title on the primary model and keeps the fallback title on failure; Hermes falls back to the main model with a warning when a configured aux route cannot be resolved; Hermes's summary failure ladder (one retry on the main model, cooldowns, then abort and preserve the session unchanged) is worth copying for `summary`.
9. **Groups with strategy belong to a later design.** OpenCodex combos (failover, weighted round-robin, least-used, reset-window; metered consent) are for a pool of accounts; one ordered alias is enough until we hold several keys per provider.

##### 6.5 What pull request 7's retry should still adopt

I have not seen PR 7. Compare it with this list; each item has at least one working example in the input.

1. **One function from error to policy, on structured fields, not message text.** Codex `protocol/src/error.rs` `retry_delay` (never retry auth, invalid request, model-not-found, context overflow, usage and quota limits; server-advice-only; backoff). Hermes `error_classifier.py` puts `retryable`, `should_compress`, `should_rotate_credential`, `should_fallback` on the error type. Pi's roughly 100-pattern text regex is the example to avoid. Add a test table: status and code in, policy out.
2. **Honour `Retry-After` with a cap, never let it extend the count, and fail the step or fall back when it exceeds the cap.** Codex sets a wake instant and keeps the count; OpenCode caps at 15 minutes; DeepSeek gives up when it exceeds `maxDelayMs`; OMP fails fast above 300 s ("a 3-hour sleep in a node is a stuck session"). Make sure the header is read where the response arrives; Pi shows how it can be dormant (`provider.maxRetries` 0 means it is read nowhere).
3. **Jitter.** Pi and Cline have none (Pi's comment says there is). The range in the input is plus or minus 10% (Codex, DeepSeek) to 25% (OMP, down only).
4. **Numbers.** Count: 3 to 5 for the main schedule (goose 3, Hermes 3, Cline 3, Pi 3, Codex 5, DeepSeek 5), 10 where the cap is low (OpenCode and OMP, about 84 s total at 2 s to 10 s). Delays: base 200 ms to 2 s; cap 8 s to 60 s.
5. **Retry only when it is safe.** Do not retry once output or a tool call has been committed unless the stream was interrupted (Cline, OMP, OpenCode). In our event log record the failed attempt as an event with the wake-up time and delay (DeepSeek `llm/retry` events, OpenCode `RetryScheduled`) and decide whether partial assistant text is kept or hidden (Pi hides it with `context_edit` entries; DeepSeek keeps it flagged interrupted on abort). `main` already falls back only before output; keep that rule.
6. **A separate patient schedule for no network.** OpenCode: `ENOTFOUND`, `EAI_AGAIN`, `ENETUNREACH` and similar use a one-hour window with its own allowance; Codex retries pure connection loss forever (5 s to 60 s) with a visible state. A node on a laptop needs this.
7. **Per-reason cooldown and fallback integration.** Trigger fallback eagerly on rate limit and billing and after N retries on transport (Hermes `turn_recovery.py`); restore the primary at the next turn (Hermes) or probe it half-open once per interval (OpenClaw `model-fallback-cooldown.ts`: at most one probe per 30 s). Key the cooldown on the reason (OpenClaw table in 3.2) and on the (credential, model) pair (Hermes). Keep cooldown state on the node, not in the event log; persist it as absolute expiry (OpenCodex `cooldown-store.ts`) so a restart does not re-offer a cooled target. Never replay a turn that already executed a tool when falling back (OMP).
8. **Billing is its own class.** Do not retry; disable the credential or model with a long window (OpenClaw: 10 minutes doubling to 24 hours), and show the user a clear message (Hermes: "retrying only burns paid requests on a depleted balance").
9. **Retry accounting across children.** A parent with 4 children each retrying 5 times is 20 sends. OpenCodex charges retries to the logical request and to a workflow root; do the same with a per-turn counter shared by children.
10. **Use the same helper for every model call**, not only the turn: title, summary and compaction calls (Pi `retryAssistantCall`; Hermes `transient_retries`). Add an idle timeout on the stream (OpenClaw: 120 s model-stream idle, 48 hours per run).
11. **Empty responses** are a distinct retryable class (goose: 3 retries; DeepSeek `EMPTY_RESPONSE`).
12. **Cancellation aborts the sleep and emits an end event** (Pi, DeepSeek, Cline).

##### 6.6 Things to test in our own code

- A child cannot call a tool its definition does not list, even if the model names it (Codex explorer, OpenCode explore).
- A child under a read-only parent policy cannot write; its shell sandbox is no wider than the parent's (PR 4 and PR 6 together).
- A child created from a custom agent with `delegates` cannot spawn at depth 1.
- A worktree creation failure for a writing child does not silently run it in the shared tree: OpenHands and Hermes fall back to shared; the input shows that is the wrong default for a node (two writers on one tree). Fail the spawn or run read-only. Copy OMP's apply ladder (plain apply, `--3way`, replay the parent's work-in-progress) and OpenClaw's rule that every uncertain garbage-collection branch means "keep".
- A restart between a child finishing and the parent recording the result delivers exactly once.
- A `Retry-After` of one hour does not block a worker for an hour.
- A 402 does not retry; a 401 does not rotate to an unconfigured provider (Hermes: "never silently hop to an unconfigured provider on auth error").

### Extensibility and unattended operation

From [round2/synth/10-extensibility-unattended.md](round2/synth/10-extensibility-unattended.md).

State used for these points: `main` has routines and webhooks that cannot act unattended without a human click, and no MCP, skills, hooks or plugins. Pull request 4 (Landlock sandbox that fails closed, an allow-listed empty child environment, secret redaction across output chunks, "allow for this turn" honoured only after a first approval) gives the base every item below plugs into. Pull request 6 enforces where an agent may run. Pull request 7 adds compaction keyed to the context window. The design note gives per-turn credential grants to nodes and rootless Podman environments with a worktree per session. `CLAUDE.md` puts a feature freeze in force until Gate 3 of `FIX_PLAN.md`; I did not read `AUDIT.md` or `FIX_PLAN.md`, so each step below needs the owner's decision to break the freeze, and step 1 is the one that fixes something that exists today.

##### 6.1 The order, with the safeguards each step needs from day one

| Step | Add | Safeguards that ship in the same change | Whose code shows why |
|---|---|---|---|
| 1 | **Unattended authorisation for routines and webhooks** | (a) an unattended run has an explicit per-routine allow set, created by a person, content-hashed and recorded as an event; anything not in it is **denied**, and the real reason is returned to the model; the node's `node.toml` is still the ceiling. (b) an agent-created routine starts `pending_approval`. (c) the agent cannot choose model or provider for unattended spend, and a routine agent cannot create routines. (d) advance the schedule before dispatch, write a fsynced dispatch record with an idempotency key before the first model call, pause (not guess) when the outcome is unknown. (e) missed runs: skip or latest-only, chosen and recorded; overlap: refuse or queue, chosen explicitly. (f) webhook idempotency keys in the database, not memory; webhook runs start from a read-only policy recorded as an event; the webhook listener is separate from the UI API. (g) never default to auto-approve; a prompt scan is a tripwire, not the gate. | OpenCode `core/src/permission.ts` (`Unattended` in `evaluateInput`); Hermes `cron/scheduler_tick.py`, `cron/scheduler_authority.py`, `tools/cronjob_tools.py`, `hermes_cli/config_defaults.py` (`cron_mode`); OpenClaw `bash-tools.exec-host-gateway.ts` (standing grant) and `delivery-queue-recovery.ts`; DeepSeek `schedule/src/runtime.ts` (flush then receipt), webhook `session.ts` (preset as log event) and its isolated listener; Cline `tasks/store/sqlite-task-store.ts` (agent-created tasks pending). Negative: Cline `cron-runner.ts`, goose `scheduler/full.rs`, Warp `execute.rs` |
| 2 | **Event vocabulary for context changes** (prerequisite for 3 and 4) | a prompt-section delta event and a tool-loadout event in the session log; the UI and the node both fold them; sorted tool names; changes appended, never edited into the head of the prompt | Pi `core/system-prompt.ts` (`diffSystemPromptSections`) and `agent-session.ts` (`_restoreToolsFromTranscript`); OpenCode `instructions/index.ts` and `session/instruction-state.ts`; Codex `core/session/world_state.rs` |
| 3 | **Skills** (text only) | roots are node-controlled paths; repository skills load only under the same per-project trust decision as repository config (Codex does not do this); catalog capped by count and characters with degradation to short descriptions; body fetched by a tool call that goes through the approval path; nothing executes at load; remote sources, if any: same origin, safe relative paths, atomic replace, content hash recorded; skill-declared environment names cannot widen the child environment; agent-written skills are `propose`-only and an approval is bound to the base content hash | OpenClaw `skills/loading/skill-prompt-limits.ts`; OpenCode `skill/discovery.ts`, `tool/plugin/skill.ts`; Hermes `tools/env_passthrough.py`, `skill_manager_tool.py` (staging) and `background_review.py` (dispatch-time restriction); negatives: Codex `ext/skills`, OpenClaw `extensions/skill-workshop` |
| 4 | **MCP client, with deferral** | (a) **only the node's own policy file may add a stdio server**; no repository file, HTTP call, model tool or master setting can. (b) the child's environment is built by the PR 4 allow-listed builder, never inherited; secrets reach it only as named per-server entries granted from the node's config. (c) stdio servers start inside the session's environment (Podman) and under the same sandbox as the shell. (d) approval is per `server + tool`, bound to the server's executable identity (path plus digest); a server's `readOnlyHint` may only tighten; unknown tools default to "needs approval". (e) tool names `mcp__<server>__<tool>` with a hash suffix on collision; a server's tools register all or none; per-line, per-result and per-call size and time caps. (f) deferral: a local BM25 search tool, switched on by **measured tool-list size**, with a hard cap on any name listing; activation recorded as a loadout event (step 2) so fork, resume and a node restart reproduce it; approval decided on the underlying tool, not on the search or bridge call. (g) `list_changed` bumps a per-server revision and specs are re-resolved before the next step. (h) Streamable HTTP only; OAuth callback hosted where a browser can reach it (the master, not the node), the token granted to the node per turn like a provider key, keyed by user and server URL, never in an event, log or URL; remote responses capped; auth stripped on cross-origin redirect. (i) refuse sampling by default | Hermes `tools/mcp_tool_config.py` (`_build_safe_env`), `mcp_tool_registration.py` (filter, collisions), `tool_search.py`, `tool_dispatch_helpers.py` (bridge peeling); DeepSeek `mcp-client/src/tools.ts` (`publicToolName`, generation swap) and `transport.ts` (`scrubbedParentEnv`); Pi `extensions/mcp/index.ts` and `tool-search/tool.ts` (recorded activation); Codex `rmcp-client/src/tool_list_changes.rs`, `bounded_stdio_transport.rs`; omp `mcp/tool-cache.ts` (cache by config hash), `mcp/client.ts` (roots only); Warp `file_based_manager.rs` (project servers never auto-start); OpenCode `mcp/pool.ts` (key includes credential). Negatives: goose `permission_inspector.rs` (`readOnlyHint`), omp `stdio.ts` and Pi `stdio.ts` (full env), Cline `McpHub.ts` (name allow-list) |
| 5 | **Hooks** | run on the node only; command hooks run under the PR 4 sandbox with the empty environment; trust is a hash of event, matcher, handler **and the bytes of every file the command references in the repository**, stored in a place only the node owner writes, re-hashed at every execution; untrusted or modified hooks do not run, and a project cannot write its own trust; **fail closed**: crash, timeout (process group killed), bad output, a rule that fails to load, or an invalid matcher all block, never silently drop; a hook may add a denial or escalate to the human, never approve; policy is re-resolved after any rewrite of the call; no hooks imported from other harnesses' directories | Codex `hooks/src/engine/discovery.rs`, `plugin_fingerprint.rs`, `plugin_trust.rs`, `config_rules.rs`; Hermes `hermes_cli/plugins.py` (`approve` escalation); omp `extensibility/extensions/wrapper.ts` (re-resolve), `runner.ts` (fail closed, 30 s); goose `hooks/mod.rs` (`classify_output` taxonomy: policy deny versus failure). Negatives: Codex `pre_tool_use.rs` (fail open), goose `hooks/mod.rs` (invalid matcher drops rule), Cline `hook-file-hooks.ts`, OpenHands `hooks-service.ts` |
| 6 | **Plugins as bundles, never in-process** | a bundle is skills, MCP definitions and hooks only; installed by an explicit human action in the UI, never a tool argument; pinned by commit hash or content hash; package-manager install with scripts disabled; `GIT_*` variables scrubbed for any Git call on an untrusted checkout; updates visible and pinned, never silent; no code runs in the master or in the node process | Codex `core-plugins/src/git_policy.rs`, `marketplace.rs` (sha pin); Hermes `plugin_guard.py` (scanner as evidence only); DeepSeek `plugin-manager/src/tools.ts` (negative: `approvedBuilds` as a model flag); Pi `package-manager.ts` (negative: no `--ignore-scripts`); omp `marketplace-auto-update.ts` (negative: silent `auto`) |
| 7 | **Code mode**, if the numbers justify it | only after steps 4 and 5: every inner call is a normal tool call through the one approval path, with the parent call id on the event and hook visibility; runs on the node under the shell's sandbox, not in the master; host-set wall-clock, memory and call-count caps the script cannot raise; no `fetch`, or an SSRF-guarded one with per-hop re-check; denying the tool must not expose the inner tools as direct tools; a failed script's completed inner calls are in the log | Pi `agent-loop.ts` (`runToolCall`), `agent-session.ts` (`_executeNestedToolCall`), `codemode/runtime/vm.ts`; OpenCode `codemode/tool.ts` and `fetch.ts`. Negatives: goose `code_execution.rs` (`lease.call` bypass), Pi `execute.ts` (script-set deadline), OpenCode `tool.ts` (denial exposes tools) |
| 8 | **Import (data only)** | per-source converters from Claude Code, Codex and OpenCode transcripts into a **new** session with a fresh id and a dedupe key; read skills and MCP definitions from other harnesses' directories as data that still passes steps 3 and 4; never read executable hooks, tools or plugins from them; never expand environment variables into imported config | goose `session/import_formats/mod.rs`; Cline `services/session-import/index.ts`; omp `session/foreign-session-import.ts`. Negative: omp `discovery/claude.ts` (`.claude/hooks` imported as code), `discovery/helpers.ts` (`${VAR}` expansion) |

The order rests on risk and on what exists. Step 1 is already needed (routines are blocked today) and adds no execution surface. Step 2 is cheap and is what makes 3, 4 and 7 safe against the prompt cache and against fork and restart. Skills add no process. MCP is the first step that starts a program the user did not write; deferral ships with it because the context cost arrives with the first few servers (Cline, DeepSeek and Warp pay it on every step). Hooks come after MCP because the node policy already gates tool use; hooks only add project-specific restrictions. Plugins are packaging over steps 3 to 5 and are last among the safe items. Code mode is last of the engine features because it is the only one that can defeat the approval path (goose) and its benefit is covered, for most users, by deferral.

##### 6.2 What to avoid, and why

- **In-process plugins on the master or in the node process** (OpenCode, Pi, Oh My Pi, Cline worker mode, Hermes default). They hold every credential the process holds.
- **Reading executable hooks, tools or plugins from other harnesses' directories** (Oh My Pi). Cloning a repository becomes code execution before any approval.
- **Auto-enabling anything discovered in a repository** (goose plugins). Trust must be a recorded decision.
- **A path-keyed trust boolean** (Pi) as the only gate for executable project content: a later pull changes what it covers. Bind to content.
- **Trusting server or model self-description for approval** (goose `readOnlyHint`, Warp `is_risky` and `is_read_only`).
- **Full-environment inheritance for MCP children** (Oh My Pi, Pi, Cline, Warp). It contradicts our PR 4 environment rule.
- **A scanner that only logs** (Hermes MCP description scan) and **a phrase deny-list used as a gate** (Hermes prompt scan). Use them as evidence or tripwires; a hit should demote a tool to needs-approval.
- **Prefix or name-based allow-lists for servers** (goose, Cline).
- **Fail-open hooks in the approval path** (Codex, Hermes, goose default, Cline, OpenHands).
- **Unattended mode equal to auto-approve** (Cline, goose, Oh My Pi default, Warp ACP), and hiding the real denial reason from the model (Warp).
- **Idempotency state in memory** (OpenClaw webhooks) or no check of a delivery id (DeepSeek).
- **Interpolating user or model text into anything scanned for executable markup** (OpenCode command templates).
- **Agent-written skills with no human step** (OpenClaw Workshop default) and **an approval replayed without a base hash** (Hermes).
- **Word "sandbox" for crash isolation** (Cline plugin mode). Do not name a feature after a property it lacks.

##### 6.3 What to check or test in our own code

Each is a test that fails on a naive implementation.
1. A routine run that reaches an `ask` returns a denial with the real reason, never waits, and writes the denial to the log.
2. A webhook delivered twice, with a restart between, creates one run.
3. Kill the node between "dispatch recorded" and "first model call": the routine pauses or resumes once, it does not fire twice.
4. A stdio MCP test server that prints its own environment sees none of the provider keys or `GIT_*` variables, only the entries granted in `node.toml`.
5. A repository containing `.mcp.json`, `.claude/hooks/*`, `.agents/plugins/*` and `.agents/skills/*` starts nothing and loads nothing without a node-side decision; a hostile repository skill is not in the first request when untrusted.
6. Tool activation by search, then node restart, fork and compaction (PR 7): the loaded set is identical, and the provider request prefix is byte-identical when nothing changed.
7. Approval for a deferred or bridged tool is decided on the underlying name (the class of bug Hermes had to fix).
8. A hook that crashes, times out, prints junk, or has a malformed matcher blocks the call. Editing a script a hook references invalidates trust.
9. If code mode is built: an inner call to `shell` in a script triggers the approval prompt; a script cannot extend its own deadline; denying the code tool does not make inner tools callable.
10. The OAuth authorisation code travels in a redirect URL, which our "no secret in a URL" invariant forbids as written: decide how the callback is handled (single-use, short-lived, never logged) before step 4(h). This is my inference, not a finding.

##### 6.4 Relation to the current state

- **main**: routines and webhooks (step 1) are the live gap; nothing else here exists.
- **PR 4**: the empty child environment and the Landlock shell are the substrate for MCP stdio, hooks and any code mode; reuse the same builder and the same sandbox entry point, or the new features become the unsandboxed path (this is what Pi, omp and Zed external agents are). "Allow for this turn" after a first approval is the right shape for unattended standing grants (OpenClaw binds them to job, revision and operation).
- **PR 6**: server-side enforcement of where an agent may run should also say which agents may run unattended and which may use MCP, as Hermes denies the `cronjob` toolset for cron agents.
- **PR 7**: compaction must keep the loadout and the skill bodies the model still needs; Hermes prunes old skill bodies and forces reload pointers into the summary; Pi's compaction entry carries a snapshot of system messages; OpenCode re-bases its instruction baseline at compaction. Goose's claim that a conversation fold needs no side table under compaction should be tested against our compaction, not assumed.
- **Design note**: per-turn credential grants to nodes extend naturally to MCP OAuth tokens; rootless Podman environments are the place stdio servers should run (OpenCode runs them through the location's environment, Codex through an executor `environment_id`, goose through `docker exec`). Our git-worktree-per-session helps import: imported transcripts become new sessions.
