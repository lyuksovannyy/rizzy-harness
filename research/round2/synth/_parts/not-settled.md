### The agent loop, tool dispatch and durability

From [round2/synth/01-loop-dispatch-durability.md](round2/synth/01-loop-dispatch-durability.md).

- **goose tool batches.** `goose.safety.md` says calls in one batch are "dispatched sequentially, their streams merged"; `goose.agents.md` says calls "run concurrently". Both cite the same lines (`ops_toolcalling.rs` L1083-L1158). I did not open the code (no clone was available). Both can be true if the loop starts each call in order but the returned futures run when merged. A decision that depends on whether goose executes tools in parallel should confirm this.
- **Steering storage in several projects.** The input files do not say whether steered or queued input is stored before it is acknowledged for Codex (`turn/steer`), OpenCode (inbox), Oh My Pi (RPC `steer`) or DeepSeek (inbox, partly process-local). My table entries for Codex and DeepSeek are inferences.
- **What a crash mid-turn does** in Codex (other than a planned daemon restart), Oh My Pi, Cline, Pi's shipped session (a dangling tool call is not repaired; the provider conversion that may drop orphans was not traced) and Hermes' in-process interrupt path were not described. No input file reports an "intent before effect" record for built-in tools in any shipped path except pi-durable (experimental) and DeepSeek's flush-before-tool rule.
- **Hermes loop guard call site** was not traced (author note). Its thresholds are read from the definition.
- **Hermes steering injection.** The admission row has intent `steer`; how a running turn consumes it was not read.
- **OpenClaw web-UI `chat.send` path** was not traced; the claim "stored before ack" is checked for the channel and `agent` paths only.
- **OpenCode** `restart.ts` resumes from the claim, but a workerd comment says boot-time resume replays the log (a discrepancy the reviewer noted). The shipped server does not persist the log.
- **Cline detector default**: the docs say off and the code runs it; the reviewer read it but nobody ran it.
- **Unchecked findings a decision would rest on:** OpenClaw restart-resume text and delivery-queue recovery (read the definition only); DeepSeek schedule double-delivery window (inferred); Hermes cron journal receipt recovery (not read); Pi durable codemode replay (read the definition); pi-durable has not been wired into the shipped CLI, so its replay policy is evidence of a design, not of a behaviour users have; OpenCodex Remote Workspace RPC v2 (traced, not reviewed).
- **Not cloned or not in the repositories:** OpenHands server SDK and Automation Server; Warp's server loop and worker; Zed (topic not covered). No claim in this file about their loops, steering or crash recovery should be taken from here.
- **Zed**: the approval wait is a loop in `thread.rs` L6872-L6985; its interplay with cancel was not read.

### Approvals and permission rules

From [round2/synth/02-approvals-permission-rules.md](round2/synth/02-approvals-permission-rules.md).

##### Remote answers: is the answer bound to the exact action?

No input file traces this, beyond an id, for these projects:

| Project | What is known | Label |
|---|---|---|
| Codex | The answer is a JSON-RPC response. | [inferred] |
| Hermes Agent | Gateway card and ACP: not traced. | — |
| Oh My Pi | ACP, RPC `extension_ui_request` and collab: not traced. | — |
| Zed | ACP option ids are passed through. | — |
| Pi | Bound to the dialog id; seen in the clone (section 1). | — |

OpenClaw's node path is the only fully traced binding.

##### Engines outside the repositories read

- **OpenHands.** How `ConfirmRisky` is enforced is not settled. Neither is whether `LLMSecurityAnalyzer` makes its own model call or reads the acting model's `security_risk`. Both need `OpenHands/software-agent-sdk`. The lesson "the model rates its own risk" rests on an [unchecked] reading.
- **Warp.** How Warp's server sets `is_risky` and `is_read_only`, and whether the server re-checks calls or enforces the organisation denylist.

##### Failure behaviour no one traced

- **OpenCode:** whether a throwing `permission.evaluate` hook fails closed. `plugin/promise.ts` was not read.
- **DeepSeek harness:** the hook bridges' behaviour on crash or timeout.
- **Cline:** whether `--yolo` removes file `PreToolUse` hooks.
- **Codex:** whether a bubblewrap start failure is misread as a sandbox denial, and so offers an unsandboxed retry. `sandboxing/src/denial.rs` matches keywords.
- **Codex:** what a `HumanCommand` approval does when no eligible connection exists.

##### Bypasses read from code but never run

- **Cline:** a hub connection authorised by `Origin` alone that creates an auto-approved session.
- **OpenCode and OpenHands:** self-approval through a readable credential.
- **Oh My Pi:** `bash -c` past a deny rule.
- **goose:** whether `delegate` runs in `approve` mode.
- **goose:** whether an ACP provider can set `goose.executable` and so pre-approve a call.

##### Unchecked findings a decision here would rest on

- **Codex:** the session approval key fields. They are the model for recommendation 2; re-read `unified_exec.rs#L87-L98` before copying.
- **goose:** the Smart Approve judge.
- **OpenClaw:**
  - heredoc and inline-eval forcing;
  - the reviewer's skip conditions and its 64-candidate cap;
  - cron standing grants.
- **Hermes Agent:** the redacted, deny-only approval card and the ACP bridge.
- **OpenCodex:** the prepare/grant/cancel RPC binding (finding 33).
- **Warp:**
  - the organisation denylist merge;
  - profile inheritance;
  - MCP project trust.
- **Zed:**
  - the file-tool symlink and sensitive-path prompts;
  - the ACP `terminal/create` path.
- **Pi:** the two-pass startup.

##### Not covered by the files read

- **Zed:**
  - how user, project and managed settings layer, and so whether `.zed/settings.json` can loosen user rules;
  - plan mode, hooks and sub-agent rule inheritance.
- **OpenClaw:**
  - whether sub-agents inherit exec mode and approvals;
  - whether MCP, code mode and client-side tools pass the exec approval path;
  - which model the reviewer uses by default;
  - who may call `exec.approval.resolve`.
- **Codex:**
  - how Code Mode (the default tool surface for most catalogued models) meets approvals;
  - the scope of the `/approve` override.

##### Repositories not studied

- **Not cloned at all:** Claude Code, Claude Desktop and Codex Desktop, because they are closed source. Their approval behaviour is known only from Round 1 documentation.
- **Not read:** OpenCodex's Claude Code and Pi remote-workspace runtimes.

### Sandboxing and where commands run

From [round2/synth/03-sandbox-containers-execution.md](round2/synth/03-sandbox-containers-execution.md).

- **Rootless `pasta` reachability.** Whether the host loopback and the node's sockets are
  reachable from a `network = "on"` container. No input covers Podman networking.
- **`podman exec` signal delivery.** Whether killing the client reaches the process inside. No
  input covers it (open in the design note too).
- **Podman's default seccomp profile.** Whether it denies `io_uring` and `AF_VSOCK`. Not in any
  input. OpenClaw only infers that the engine default applies.
- **`sendmsg` to pathname datagram sockets** under Codex's `Restricted` filter. The reviewer
  flagged it and nobody tested it. Whether Landlock ABI 9 covers datagram sends is also not in
  the input.
- **Why Codex rejects Landlock.** The code states only "isolate app-server sockets". The
  `connect()` explanation is inferred. Whether the daemon can spawn unsandboxed processes was
  not traced.
- **Why Zed moved from Landlock to bubblewrap.** No reason is written in the code.
- **Codex:** the Seatbelt base profile, the Windows restricted-token and MXC backends, and
  whether a `bwrap` start failure can be misread as a denial and lead to an unsandboxed retry.
  The policy-selection table and the proxy bridge internals are unchecked.
- **OpenHands:** the real `docker run` invocation, its hardening flags, server-side clamping of
  the client limits, and which names `sanitized_env()` strips. All need
  `OpenHands/software-agent-sdk`, not cloned.
- **Warp:** the self-hosted worker (`warpdotdev/oz-agent-worker`, not cloned). The `sbx` Docker
  sandbox finding is unchecked.
- **OpenClaw:** the browser sandbox and the SSH sandbox backend are unchecked. How `docker exec`
  children are killed is not in the input. The claim that a host-exec agent can edit
  `openclaw.json` is inferred.
- **Oh My Pi:** the kill semantics in `pi-shell` were not checked in review. A decision on the
  re-walk rests on the author's reading alone.
- **goose:** the process-cleanup finding (process group, Job Object, no PDEATHSIG for the shell)
  is unchecked.
- **DeepSeek:** the Windows ACL backend is unchecked, and its token code was not read. The claim
  that confined commands can read `.credentials.yaml` is inferred.
- **OpenCodex:** the macOS and Windows helper was not read. That host sockets are invisible in
  its bubblewrap argv is our inference from the mount list.

### Credentials, secrets and authentication

From [round2/synth/04-credentials-secrets-auth.md](round2/synth/04-credentials-secrets-auth.md).

- **OpenHands Agent Server**: whether it closes a WebSocket that never sends the auth frame, which
  names `sanitized_env()` strips, whether `base_state.json` persists the session key carried in
  `LookupSecret` headers, and server-side output masking. Needs `OpenHands/software-agent-sdk`.
- **Warp**: server-side redaction, the self-hosted worker's authentication (`oz-agent-worker`).
- **Codex**: `credential_broker.rs` was not opened by the reviewer (defaults only); the netns proxy
  bridge internals are unchecked; whether Codex guards an unauthenticated loopback listener from a
  sandboxed command with network on was not found; remote-control pairing-code expiry is
  server-side.
- **OpenClaw**: the secret egress proxy's bypass by a process that ignores proxy variables is
  inferred (no namespace found on that path); plugin install trust and the browser sandbox CDP token
  were not read in this round; the POSIX permission check on the GitHub profile directory was not
  seen.
- **Hermes Agent**: the terminal blocklist, the output redaction finding and credential pools are
  unchecked; the relay connector's buffer and use of the delivery key are not in the repository.
- **Oh My Pi**: no gateway token expiry was found, and routing after the gateway's auth check was not
  traced; MCP OAuth and export redaction are unchecked; the CORS attack on `--no-auth` is inferred.
- **OpenCode**: `--remote` encryption lives in `@opentunnel/*`; MCP OAuth was not read; the
  self-approval path with the leaked password is inferred.
- **Cline**: the provider key's lifetime in the remote hub's memory and whether it reaches disk;
  the Origin-shortcut exploit was not run against a hub.
- **OpenCodex**: the Home side of the Remote Link relay was not traced end to end; the value-based
  log guard (finding 11), admin-token rules (16), the unauthenticated loopback listener (15), the
  stream preflight (24), failover ordering (22), the send budget (7) and metered consent (25) are
  unchecked. Recommendations in section 6 item 3 rest on several of these; re-read
  `combo-stream-preflight.ts`, `request-send-budget.ts` and `configured-credentials.ts` before
  copying numbers.
- **Zed**: whether `download_server_binary` refuses a `None` checksum; Zed's provider-key storage
  and collaboration auth were not studied.
- **goose**: the WebSocket Origin policy and the desktop's token-in-URL flow are unchecked.
- **For our own design**: whether Landlock (pull request 4) blocks `ptrace` and `/proc/<pid>/mem`
  of the node process from a confined child, and whether `systemd-creds` works for user services,
  were not in any code read; both need a test on our side before item 1 is claimed done.
- Repositories not cloned that would settle open points: `OpenHands/software-agent-sdk`,
  `warpdotdev/oz-agent-worker`, the `@opentunnel` packages, and the vendor relays and registries
  named above.

### Links, workers and client protocols

From [round2/synth/05-links-workers-protocols.md](round2/synth/05-links-workers-protocols.md).

- **OpenClaw transcript-commit ledger and protocol frames** were not checked in review; the
  fencing and credential findings that 6.1 relies on were.
- **Codex exec-server Noise channel internals** (stream numbers, reordering window) and the
  **remote-control** transport were read from definitions only; the relay, registry and pairing
  expiry are hosted services not in the repository.
- **Codex: what happens when no connection qualifies for a human-only approval** (the remainder
  of `outgoing_message.rs` after L430) was not traced.
- **Hermes relay** (ack after admission, 4401 semantics) and **Kanban** were not checked; the
  connector's buffering is outside the repository.
- **OpenCodex Remote Workspace RPC v2** (prepare/grant) and the handshake's security reading
  were not checked (crypto only spot-checked); the Home side of Remote Link was not traced;
  the macOS and Windows helper (`native/remote-workspace-helper`) was not read.
- **DeepSeek `follow` and reconnect, prompt idempotency and LiveShare** were not checked;
  LiveShare's relay is not in the repository.
- **goose**: whether `setsid` children and MCP server children survive a roaming revocation
  (shell children are killed). The iroh relay's visibility into ACP content was not verified.
- **OpenHands**: whether the Agent Server closes a socket that never sends the auth frame,
  how it orders and skips events, its fork and persistence: all in `software-agent-sdk`, not
  cloned.
- **Warp**: the server-side loop, inbox durability and ordering, and the self-hosted worker
  (`warpdotdev/oz-agent-worker`) were not available.
- **Zed**: registry download verification when `sha256` is absent (`download_server_binary` not
  read); a reader of `tool_permissions` in a crate outside the three searched is not ruled out;
  the ACP schema is in `agentclientprotocol/agent-client-protocol`, not cloned.
- **Cline**: the Origin-only exploit (create an auto-approved session in any directory) is code
  reading, not exercised; where a provider key is dropped from the remote hub's memory after a
  session was not found.
- **Oh My Pi**: the real collab relay and the approval wiring of guest prompts.
- **Pi**: how `apps/p2` authenticates its workers to its server (`owner-link.ts`) was not read.
- **Our own code**: whether `nodes.rs:208-212` drains or drops frames already queued from a
  replaced connection, and whether deltas are still written to the log on the current branches,
  must be checked in our tree before 6.1 and 6.4 are written; this synthesis cites them only
  from the design note and the Round 1 inventory.

### Session storage, resume, fork and checkpoints

From [round2/synth/06-sessions-fork-checkpoints.md](round2/synth/06-sessions-fork-checkpoints.md).

- **Codex:** the trigger of the first `persist()`; legacy-mode (copy) fork; the `rollout_lineage` fallback when a parent is missing. Only the writer lock, revert and the absence of checkpoints were reviewed; rollout format, resume, fork pointers and retention were not.
- **OpenCode:** behaviour of the log endpoint in a non-persisting server (inferred; a test with `persist:false` would settle it); whether `git gc` in the user's repository can invalidate snapshot objects reached through alternates (inferred; needs a gc-then-restore test); the implementation of the "inherited prefix" for forked aggregates. The cost guards, export, fork and revert findings were not reviewed.
- **OpenClaw:** what the worktree snapshot scans and its cost guards; whether the removal path always snapshots first; fork, schema, pending inputs and restart-resume were not reviewed.
- **Hermes:** whether hooks or filter drivers can fire in the checkpoint store (inferred not; a hostile `.gitattributes` plus a global filter would settle it); whether a command the regex misses loses its snapshot in practice (inferred); resume, branch, rewind, export and the schema were not reviewed.
- **Oh My Pi:** whether `/tree` navigation without a later write persists (inferred non-durable; a branch, exit and resume test would settle it); the Rust crates `pi-edit` and `pi-vcs` were not read, so a file-level undo there is not excluded; most storage plumbing (title slot, id generation, lease, 500,000-character cap, pluggable backends, resume, export, foreign import) was not reviewed.
- **Pi:** the restart consequence of a lost leaf is inferred (reviewer agreed); fork, resume, export, durable forks and the branch-summary path were not reviewed; durable's storage and task-commit code were not read.
- **DeepSeek:** log core append mechanics, projection cache, reconnect and prompt admission were not reviewed; `flock` on a shared network directory was not examined.
- **goose:** fork, truncate, session types and the run registry were not reviewed.
- **Cline:** a size or count cap for checkpoints was searched for in `checkpoint-hooks.ts` and not found; nothing was run, so behaviour on a large repository is unmeasured; restore-forking, the compaction sidecar and the checkpoint diff were not reviewed.
- **OpenHands and Warp:** the engine, storage format, event persistence and fork internals are in repositories that were not cloned (`OpenHands/software-agent-sdk`; Warp's server and `warp-proto-apis`). Warp's local SQLite conversation storage and resume were not read.
- **Zed:** the shallow clone has no history, so why per-prompt checkpoints were removed and what they did cannot be read; the cost reasoning is inference. The thread database was not described.
- **OpenCodex:** the hub session persistence (`workspace-sessions.ts` `persist()`) was not read.
- **Gaps a decision rests on:** no project bounds a full-coverage checkpoint in a very large repository without losing coverage or accepting O(tree), and nobody measured it. The input shows no project handling container-made writes other than excluding the container backend (Hermes). Nothing in the input confirms git's built-in filesystem monitor or an inotify flag as a safe "tree unchanged" signal for item 4; the bounded newer-file walk (item 4) and the bundle transfer for cross-node fork files (item 8) are proposals, not input findings.

### Context: prompts, compaction, instruction files, memory

From [round2/synth/07-context-compaction-memory.md](round2/synth/07-context-compaction-memory.md).

Cross-cutting:
- No input file measures compaction quality or cost; trigger fractions (0.5 to 0.9) and keep sizes (10K to 25K tokens) are choices, not tested values.
- Cache-preserving summary requests: OpenCode states the intent in a design comment, DeepSeek and Zed implement it; no finding measures the hit rate. Hermes quotes a vendor cost saving for its review fork (about 26%) that was not measured.
- A map-then-merge chunked summariser: not found in the findings (absence not confirmed).
- Whether any project verifies a summary against the original (facts preserved): only OpenClaw's audit of headings and identifiers is described.
- Findings that no reviewer checked and a decision might rest on are marked `unchecked` in section 2; the main ones are OpenCode's background precompute and compaction prompt text, DeepSeek's summariser and what-is-kept details, goose's compaction internals and spill, Oh My Pi's method chain, snapcompact and overflow handling (the reviewer's own "not checked" list), Hermes' provider-overflow ladder and prompt caching, OpenClaw's overflow recovery, summary generation, pruning and memory flush, and Codex's world-state diffing, local and remote compaction and memory read path. Re-read the code before copying a number from any of these.

Per project:
**Codex**
- How the context size is measured for the trigger (provider usage or estimate): not described in the findings.
- Whether a mid-session edit of a project `AGENTS.md` is ever re-read: concluded from the cache key; a run would settle it.
- `redact_secrets` rules: not read.
- The per-model prompt wording beyond the first catalog entry, and the server-side catalog refresh.
- Local and remote compaction details, the memory read path and the ad-hoc note tool were not checked in review.

**OpenCode**
- What happens when `agents.compaction.model` names a model with a smaller window than the session model; the input files do not describe a window check for the summariser.
- Whether any check exists that compaction output is smaller than its input (none described).
- Size limits on AGENTS.md and instruction URLs: none seen in `config/plugin/instruction.ts` or `instruction-discovery.ts`; a provider-side limit may exist. API-attached `InstructionEntry` has `MaxValueBytes` (`instruction-entry.ts` L18).
- Trust prompts for project plugins and skills outside `packages/core` (desktop or TUI).
- Memory: not searched beyond the three cluster files and their headings; no reviewer searched for absence.
- Unchecked findings a decision would rest on: instruction sources and nested AGENTS.md (descriptive, "same author care" per the review), background precompute, native compaction, the compaction prompt text. Re-read the code before copying a number.

**OpenClaw**
- The 40,000-token summariser input cap and `maxSummaryChars` 16,000 (value of `MAX_COMPACTION_SUMMARY_CHARS`, `sessions/compaction/request-budget.ts` not read).
- What a failed or interrupted compaction leaves in the transcript, and whether a compaction always opens a `session_windows` row with reason `compaction` (the sessions cluster did not read compaction).
- Context-engine plugin slot and its `legacy` default: how `ownsCompaction` changes the compaction path was not read.
- The workshop skill review prompt and writer; the consolidation (dreaming) prompt text.
- No check that compaction shrank the context and no summariser-window check were found (not searched for absence by a reviewer).
- Unchecked findings a decision would rest on: prompt assembly and cache prefix, summary generation, overflow ladder, memory flush, pruning, memory search, who writes memory. The review says to trust constants and structure and re-check anything that says a lane, queue or claim is global, durable or unsettled.

**Hermes Agent**
- Gateway "hygiene" compaction at 85%: file found (`gateway/run_turn_hygiene.py`), threshold not traced.
- Micro-compaction and `compression.prepare_ahead`: named, not traced; they may add a between-compaction pruning or precompute path.
- `curator.backup.keep` default (Round 1 says 2): `get_keep` not read.
- Prompt caching, the provider-overflow ladder, the lock parser and the instruction-file loader were not in the reviewer's checked set.
- Whether the summary request can share the main conversation's cache prefix: not found; the call is a single user message on an aux slot.
- What "provider-confirmed ineffective" means exactly in the breaker (no reviewer check).
- Reviewer caveat: the lowered trigger equals the whole summariser window, leaving no headroom for template and output.
- Whether `/skills approve` in `hermes_cli` adds a hash check beyond `skill_manage` (investigator found none at replay).

**Oh My Pi**
- The compaction algorithm itself (`prepareCompaction`, the summary prompt, what is kept verbatim beyond the 20,000-token keep-recent budget) and the meaning of `handoff`, `shake` and `soft`: not read in depth.
- Whether a summary request reuses the conversation's cache prefix; whether the trigger count is provider-reported or estimated beyond the stored-conversation floor.
- Whether a summariser with a window smaller than the history is detected anywhere other than by candidate ordering.
- Reviewer did not check: snapcompact, compaction triggers, context-length-error recovery, pruning, system-prompt assembly, instruction-file shadowing, compaction replay, sharpshooter. Decisions resting on those carry only the investigator's trace.
- Whether `ttsr-injection` messages are pinned through compaction: searched, but `prepareCompaction` and `tool-protection.ts` were not read fully.
- Local distillation memory, `learn`/`manage_skill` approval, and any session search: not traced.
- Instruction-file size budget: none reported; absence not confirmed by a reviewer.

**Pi**
- What happens when the summarisation input itself exceeds the window: no guard was found; the reading that it fails as a non-retryable overflow was not traced to a test (a test in `packages/coding-agent/test` would settle it; not read).
- Provider-specific `maxTokens` clamping for summary requests: the `_reserveTokens` parameter is unused, so any clamp is in `packages/ai`, not read.
- Findings 1.3, 1.4, 2.2, 2.3, 4.5, 4.6, 5.2 and 10.1 were not checked by a reviewer; only the numbers 2,000 lines and 50 KB were.
- The roughly 2,000-character default prompt size was not re-measured.
- Whether `todo.ts` state survives compaction was not tested.

**DeepSeek harness**
- What stops a hostile repository AGENTS.md: no trust gate in the code read; one could exist in the sessions or safety paths.
- Compaction findings on the summariser model, what-is-kept, recovery and model-switch details were not checked by a reviewer.
- The consequence of a committed summary with a failed `compaction/end` (lock open until `end-seed`) was not traced.
- What happens when the summarising model's window is smaller than the span: not covered in the input; the model-switch rule compacts against the new window and refuses the switch on failure.
- Whether the token meter prefers provider-reported usage over its own count: not stated in the input.
- Memory: a negative search cannot prove absence outside `packages/`.

**goose**
- Whether the deferral enabled-set counts messages hidden by compaction (a test in `compaction_lifecycle.rs` or `advertised_tools.rs` would settle it; not read).
- How the session model's `context_limit` is resolved for custom providers (`context_limit.rs`): not traced.
- The input describes no check that compaction shrank the context and no deterministic fallback; no reviewer searched for either.
- The summariser format, MOIM, context report and compaction-storage findings were not in the reviewed set; the 800-character and 48,000-byte numbers and the cutoff examples were not recomputed.
- Memory: no size limit, no background review and no session search are described; no reviewer searched for them specifically.
- The reactive-compaction cap is read from a comparison, not exercised; `compaction_lifecycle.rs` was not read.

**Cline**
- Per-model system prompts: only the two templates exist here; the Desktop and Kanban repositories were not read.
- Whether the summary request reuses the conversation's cache prefix: not described.
- Whether any size cap applies to rule files: none found, no reviewer confirmed absence.
- Conditional-rule evaluator on the SDK path: found no caller; a runtime test would settle it.
- Behaviour when the summariser model's window is smaller than the prefix: not described beyond "input budget exhausted".

**OpenHands**
- The condenser's trigger unit, summary prompt, summariser model, overflow behaviour and what a failed condense leaves: all in `OpenHands/software-agent-sdk`, not cloned.
- The base system prompt text and any caching: not in this repository.
- Memory: not checked by any reviewer; SDK.
- Skills catalog and allow-list rule were not checked in review.

**Zed**
- Whether a `PromptTooLarge` path elsewhere triggers compaction: inferred from `is_transient`, not run.
- Instruction files, system prompt text and memory in Zed: outside the study's scope.
- Output limits and retry findings were not checked in review.

**Warp**
- The prompt, tool descriptions, compaction, model routing and fallback: server-side, schema repo not cloned.
- Whether the server also compacts on `ContextWindowExceeded`: not visible.
- What the `summary` column of `agent_conversations` stores and who writes it.
- Rules files and memory: not covered by the study.

**OpenCodex**
- What the client sends as history and when it compacts: outside this code.
- Whether the gateway estimates context size before routing (to pick a model with a large enough window): not described; `jev` combo decision backend and `routing/cost.ts` not read.
- Finding 28 (compaction) and the failover finding were not checked in review.

### Edit tools and the mechanics behind the scenes

From [round2/synth/08-edit-tools-mechanics.md](round2/synth/08-edit-tools-mechanics.md).

- **Edit-tool findings no reviewer checked** that a decision here rests on: Hermes' nine-strategy chain, its mtime warning and its lint delta; OpenClaw `edit` and `apply_patch` (including post-write verification and the success-on-throw behaviour); Codex's freeform grammar and line-ending handling; Oh My Pi's edit grammar and `sloppy`; Pi's argument repair and output-cap numbers; goose's process cleanup and unbounded collection; Cline's background detach, VS Code terminal and cancel; Zed terminal output cap; Warp's PTY mechanics; OpenCode's tool-input repair, large-repository snapshot guards and undo; OpenCodex's RPC and tool set. Re-read the cited lines before building on any number.
- **Oh My Pi TTSR** matching, abort, judge and compaction findings were not checked beyond default settings; the 50 ms delay, the `contextMode: discard` default and the absence of a retry counter all come from the research author's read. The claim that a catastrophic regex stalls the session is `inferred` (no timeout was found).
- **Hashline collision consequence** (a live 16-bit collision bypassing recovery) is `inferred` from the code path; a test with two texts sharing a tag would settle it. The Round 1 benchmark claims (hashline beating patch for 14 of 16 models) were not run.
- **Post-edit diagnostics** in Oh My Pi (`packages/coding-agent/src/edit/index.ts`) and Hermes (LSP layer), and Round 1's claim that Claude Code runs diagnostics after each edit, are not settled by Round 2. Claude Code was not cloned.
- **Hermes' staleness path**: whether `patch` uses the byte-snapshot helper or only the mtime was not traced.
- **Whether git hooks or filter drivers can fire in Hermes' checkpoint store**: untested.
- **Why Zed's per-prompt checkpoints hung** is `inferred`; the history is not in the clone.
- **Process lifetime in Codex's unified exec** beyond the limits read; shell kill and background mechanics for OpenClaw `exec`, Zed's terminal, Warp's blocks and DeepSeek's `bash` tool were not in the findings.
- **OpenCode formatter default**: Round 1 says off unless enabled; the Round 2 files show a formatter running in `commit` but do not show the config gate.
- **Tool-result and read/search tool internals** (OpenClaw `read`, Codex tool caps, Oh My Pi `read` and `grep` caps, OpenCode's `grep`/`glob` limits): not in the findings; only Pi, Cline, OpenCode and our own numbers are known.
- **OpenHands** `file_editor` and its stuck detector (SDK repository), **Warp's** server-side edit logic and **Claude Code** were not cloned.
- **Cross-file atomicity**: no implementation read provides rollback; whether crash-safe multi-file commit matters for our users is a product call, not answerable from the code.
- **Whether clearing the "seen" table at compaction (6.1 item 6) helps or only adds re-reads** has no evidence in the files; measure it after pull request 7 lands.

### Sub-agents, orchestration, model roles, fallback and retry

From [round2/synth/09-subagents-models-retry.md](round2/synth/09-subagents-models-retry.md).

- **OpenClaw per-attempt retry** (`src/llm/utils/retry.ts`) and the per-run retry budget (`run/retry-budget.ts`) were not read by the author: no retry count, delay or jitter for OpenClaw. Its fallback loop body (lines 440 to 745) was skimmed, and its spawn path, child model, result announcement and fallback chain were not reviewed.
- **Zed** has no sub-agent section in the input; whether its native agent has a sub-agent tool is unknown here. Its retry numbers were not given (`retry_strategy_for`; "output limits and retry" was not reviewed).
- **Warp**: retry delays and `Retry-After`, model selection and fallback, the child's context and result are server-side; the message bus's ordering and durability are invisible.
- **OpenHands**: SDK sub-agent depth, concurrency, model and permission inheritance; retry and fallback; ACP permission handling. Only a captured snapshot (not line-citable) gives `num_retries 5`.
- **Hermes**: the child run loop, auxiliary slots, retry, fallback chain and credential pools were not reviewed (only the 250-versus-50 and child-permission findings were). The auto-recovery ladder and OAuth refresh were not read.
- **OMP**: alias resolution, chains, triggers and `yield` were not reviewed. The `IRC peer roster` in the child prompt is named but not described. `usageAwareFallback` behaviour is not described.
- **OpenCodex**: findings 7, 18 to 22, 24 and 28 were not reviewed; the Home side of the link, `jev`, and the thread-affinity store were not read.
- **Codex**: result return and spawn configuration details were not reviewed; the count of 9 of 12 catalogued models with `multi_agent_version: v2` was not recounted. Codex has no cooldown or credential pool in the findings.
- **DeepSeek**: Claude Code result collection was not read in full; the Auto-review reaction to a child at the `auto` preset was not traced; the `workflow` and pool interaction is `inferred`.
- **goose**: named-agent loading, parallel dispatch, `/goal` and the empty-response retry were not reviewed; the "two third-party classifiers" are referred to in a finding I did not read.
- **Pi**: the example router `jev-router.ts` was not read; model selection (6.1, 6.2) was not reviewed.
- **Our own code:** I did not read the PR 4, 6 or 7 branches. Whether PR 7 already has jitter, a `Retry-After` cap, a no-network schedule or per-reason classes is unknown to me. The alias and slot tables are from the design note only (the code on `main` has no title generator or vision bridge, per the design note).
- **Two points where input files disagreed**, resolved by the reviewers' corrections and used as corrected: OpenCode child permissions (the agents file after review says agent rules plus session rules; the original safety text said otherwise); OpenClaw's lane width (per spawning session, not gateway-wide). Pi's `Retry-After` (inert by default) was also corrected in review.
- **No repository was left uncloned**; the limits above are limits of reading, not of access.

### Extensibility and unattended operation

From [round2/synth/10-extensibility-unattended.md](round2/synth/10-extensibility-unattended.md).

- **Whether a stdio MCP child inherits the harness environment** in Codex and OpenCode: not stated in the findings. Whether an OpenClaw stdio server does: not read.
- **OAuth detail** for OpenCode (`mcp/oauth.ts`, 667 lines, not read), OpenClaw, Warp and Cline beyond token storage; Codex's PKCE is delegated to `rmcp` and not traced.
- **Codex Code Mode** runtime isolation and approval interaction were not traced; the catalog-dependent `code_mode_only` count was not recounted by the reviewer.
- **Findings no reviewer checked that a decision here rests on**: the whole of Codex MCP client, OAuth, deferral, plugins and `list_changed` (section 3.7 of its sessions file); OpenCode skills, schedules and instruction-source diffs; OpenClaw MCP, standing grants, heartbeat, webhooks, delivery queue and skill tiers; Hermes tool search, the cron admission journal and plugin trust; Oh My Pi tool cache, OAuth and foreign import; Pi skills discovery, the two-pass start-up, tool-search activation as a recorded loadout and deterministic replay; DeepSeek schedule and headless; goose deferral and MCP roots; Cline marketplace, skills and import; OpenHands skills, MCP and automation; Zed ACP registry install and spawn; Warp MCP tool lists and permissions.
- **Hook failure handling** in DeepSeek's `hooks-claude-code` and `hooks-codex` bridges was not read (`runPoint`).
- **Whether workflow children (DeepSeek) and code-mode inner calls (Codex)** go through approvals.
- **Whether a project-plugin or hook trust prompt exists in a UI layer** for OpenCode, goose and Cline: the findings are negative searches in the engine, and goose says a prompt could live in the UI.
- **Whether Cline file hooks run under `--yolo`** (code does not remove the file-hook extension; docs say hooks are disabled); **whether a Cline schedule created while the hub is stopped fires later**.
- **Whether Zed's registry download rejects a missing checksum**; Zed's `context_server` MCP crates and `agent_skills` were not read.
- **Repositories not available**: the OpenHands automation service and Software Agent SDK (hook semantics, MCP client, skill triggers), Warp's server-side loop and automations, OpenClaw's plugin install trust and ClawHub policy (not read), the ACP schema repository.
- **`AUDIT.md` and `FIX_PLAN.md`** were not read, so the freeze cost of each step is not assessed.
