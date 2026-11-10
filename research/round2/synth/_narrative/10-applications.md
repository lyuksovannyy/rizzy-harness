## 10. Potential applications to our project

This is one ranked plan built from section 6 of the ten group files, with the overlap between groups removed. Where two groups made the same recommendation (fencing appears in groups 01, 05 and 06; credential reach in 02, 03, 04 and 05; checkpoint cost in 06 and 08), it appears once, under the group that holds the fullest evidence.

Read it with four caveats:

- Sizes (small, medium, large) are our estimates of the change, not findings.
- `CLAUDE.md` puts a feature freeze in force until Gate 3 of `FIX_PLAN.md`. Part (a) is repairs to what exists. Parts (b) and (c) are mostly new work, and their cost against the plan is not assessed here.
- Security code (auth, the node link, tools, the approval path) gets the full stress test and an abuse-case test written first.
- The synthesis authors did not read the branches of pull requests 4, 6 and 7. Statements about those branches come from the task brief.

The invariants are named in short form: **node decides**, **master runs no tools**, **one writer**, **private chats**, **no secret in event, URL or log**, **loopback default**, **light process**.

### (a) Fix in what is already built

Ranked by the harm a missing fix allows.

**A1. Approvals bound to an action digest, and a narrow "allow again".** `main` and PR 4. Medium. Security code.

- **What.**
  - `ApprovalRequested` gains `action_digest`, computed on the node. It is a SHA-256 over a length-prefixed canonical encoding of: session, node and call id; tool; canonical input; working directory by real path; environment beyond the allow-list; sandbox profile; and a hash of the node policy snapshot.
  - `approval.respond` must echo the digest. The node rejects a mismatch, consumes an approval on first use and asks again if its policy changed between request and answer.
  - The master accepts an answer only from a browser session of the chat's owner, never from the node link.
  - Replace `AllowSession`-by-tool-name with a key on canonical argv, working directory and permission delta. Hold it on the node and write it as an event. It ends with the session, never passes to a child, and is never cached for network access or escalation.
- **Copy from.**
  - OpenClaw's policy-snapshot re-check: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/invoke-system-run.ts#L349-L357
  - OpenClaw's replay guard: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/node-invoke-system-run-approval.ts#L197-L252
  - OpenClaw's executable binding: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/system-run-approval-binding.ts#L366-L458
  - Codex's session key: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/tools/runtimes/unified_exec.rs#L87-L98. This is `unchecked`: re-read it before copying the fields.
- **Invariants.**
  - Node decides: an `Allow` can turn an `ask` into a run, never a `node.toml` deny. OpenClaw's node is the counter-example: a forwarded approval satisfies its own allow-list (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/node-host/exec-policy.ts#L60-L117).
  - Private chats: an empty approver list must mean "the owner only". OpenClaw's empty list means anyone in the chat (https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/approval-auth-helpers.ts#L72-L89).
- **Depends on.** Nothing. The five abuse tests are listed under item 1 of section 6 in `round2/synth/02-approvals-permission-rules.md`.
- **Round 1.** Round 1 already recommended argv binding. Round 2 adds three things: the echoed digest, the owner check and single use. It also shows that OpenClaw's "allow always" is narrower and odder than documented: it hashes the working directory and the arguments after the first, and the executable is hashed only if the user can modify it.

**A2. Fence the node link and every mirrored append.** `main`. Medium. Security code.

- **What.**
  - Add `nodes.link_gen`, bumped when a connection is accepted. The master's reader pump checks it on every inbound frame and drops frames from a superseded connection.
  - Add `sessions.owner_epoch`, drawn from a counter that never goes backwards. It is bumped on archive, delete, node revocation and the close of a fork source, and not on a reconnect of the same node.
  - Check one predicate inside the mirror transaction: owner, epoch, active state and `seq = last_seq + 1`.
  - Handle resends by hash. An equal hash is acknowledged as a duplicate. A different hash is refused as `diverged`, which covers a node restored from an old disk.
  - On a `fenced` reply the node stops the session, kills the running tool and marks the session read-only.
- **Evidence.**
  - Our reconnect cancels the old connection and installs the new one with no generation on its frames (`crates/master/src/nodes.rs:207`).
  - The full design and its tests are in 6.1 of `round2/synth/05-links-workers-protocols.md`.
  - OpenClaw's per-request re-check: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/admission.ts#L188-L218
  - OpenClaw's never-reused epoch: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/store-mutations.ts#L30-L50
  - OpenClaw's terminal `owner-epoch-mismatch`: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-connection-contract.ts#L22-L26
  - OpenClaw's epoch bumps only on attach or from certain states, so we must write our own bump list rather than copy theirs.
- **Invariants.** One writer. This turns the invariant from a convention into a check.
- **Depends on.** PR 4's bounded inbound channel holds up to 8 parsed messages. That queue is exactly where frames from a replaced connection can wait (test D5).
- **Round 1 overturned.** Round 1 named OpenClaw and OpenCodex together as fencing models. OpenCodex's Remote Workspace is "latest connection wins" (`attachConnection`), the same as ours. Only OpenClaw fences.

**A3. Keep every control-plane credential out of the agent's reach, and test it.** PR 4. Small to medium. Security code.

- **What, as tests written first.**
  - A confined `cat` of `node.key`, `$CREDENTIALS_DIRECTORY/node.key`, the data directory and `node.db` fails.
  - Grant keys never appear in `env`, `/proc/self/environ` or `/proc/$PPID/environ` of a shell.
  - `env_pass = ["*"]` and `env_pass = ["*_KEY", "HARNESS_*"]` still pass no provider key and no link credential.
  - Injection variables (`LD_*`, `NODE_OPTIONS`, `BASH_ENV`, `GIT_*` executables) cannot be passed.
  - The node process is made non-dumpable through a safe `prctl` wrapper, for example `rustix`, which needs the dependency check.
  - On the master, the embedded local node's shell cannot read `api-token`, `secrets.toml`, `keys/secrets.key` or `master.db`.
- **Copy from.**
  - Codex's always-strip list: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/shell_environment.rs#L14-L28
  - Hermes Agent's fail-closed passthrough check, added after advisory GHSA-rhgp-j443-p4rf: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/env_passthrough.py#L46-L79
  - OpenClaw's blocked keys: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/infra/host-env-security.ts#L38-L65
  - Codex has process hardening but does not apply it to its main binary (absence confirmed in review): https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/process-hardening/src/lib.rs#L7-L61
- **Why it ranks this high.** The worst failures in group 04 are a harness's own credentials in the agent's reach. OpenHands puts its session key in the agent's environment (https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985). DeepSeek harness's cookie-signing secret is readable by confined bash (see item 1 of section 6 in `round2/synth/04-credentials-secrets-auth.md`).
- **Invariants.** No secret in event, URL or log. Master runs no tools: the embedded node is the case where the master's own secrets meet a shell.
- **Open.** No code read says whether Landlock alone stops `ptrace` or `/proc/<pid>/mem` of the node process (test D11).

**A4. Replace the admin-equivalent API token with per-user tokens.** `main`. Medium. Security code.

- **What.** Per-user tokens, stored as hashes, scoped and revocable, in place of the one long-lived admin-equivalent token.
- **Evidence.** The same single shared bearer appears in Hermes Agent, goose, OpenHands and OpenCodex (item 4 of section 6 in `round2/synth/04-credentials-secrets-auth.md`).
- **Invariants.** Private chats, "for admins too". Today the token can see every chat, so the invariant is not met on `main`.
- **Depends on.** Nothing. The design note does not cover it.

**A5. Never run tool calls from a response that stopped on the length limit.** `main` (`crates/agent-native`). Small.

- **What.** Fail every tool call in a message whose finish reason is `length`. Fail the step after three consecutive `length` stops, as OpenCode does.
- **Evidence.** Pi fails every such call, because its streamed arguments are completed by a JSON salvage parser (https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L273-L279, https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/agent/src/agent-loop.ts#L488-L513, checked in review). A truncated `write` or `bash` argument loses data.
- **Invariants.** None touched.
- **Test.** A provider fixture that ends a `write` call mid-JSON.

**A6. "Append, flush, then act" as a tested rule.** `main` and PR 4. Small to check, medium if it fails.

- **What.** The event recording "tool call X with arguments A is about to run" must be durable on the node before the tool body starts. A failed append means the tool never runs. Interrupted calls are closed with wording that says the outcome is unknown.
- **Copy from.**
  - DeepSeek harness's awaited, fail-closed flush: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/session/session-checkpoint-policy/src/index.ts#L63-L83 (checked in review)
  - DeepSeek harness's closer wording: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/repair.ts#L31-L41
  - OpenClaw's lost-worker rule, never auto-continue: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/worker/worker-node-loss.ts#L3-L15
- **Invariants.** One writer.
- **Depends on.** PR 4's bounded link. If the append is queued behind back-pressure rather than committed locally, the rule is false.
- **Tests.** D7 and D8.

**A7. Make the approval path's failure modes fail closed.** `main` and PR 4. Small each.

- **What.**
  - The rules or policy file fails to parse: refuse to start.
  - An unknown setting value: treat it as the strictest value.
  - A shell construct cannot be analysed: ask, and deny when unattended.
  - No person can answer: deny.
  - The approver callback is missing or throws: deny.
  - The audit event cannot be appended: reject the request.
  - The sandbox is unavailable: refuse. PR 4 already does this.
  - In every case, return the real reason to the model.
- **Evidence.** The table under item 3 of section 6 in `round2/synth/02-approvals-permission-rules.md`.
  - Model to copy, DeepSeek harness's `user-approval`: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/user-approval/src/index.ts#L224-L261
  - Counter-example, Codex dropping every user rule on a parse failure: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L729-L744
- **Invariants.** Node decides. One writer: the audit append failing rejects the request.

**A8. Agent roster enforced on the node.** PR 6. Small to medium.

- **What.**
  - The node refuses, at execution, any tool outside the agent's `tools:` list, even when the model names it.
  - Scout and Reviewer have no write tool, and `bash` is absent or under a read-only Landlock profile.
  - A child's authority is the intersection with the parent's effective policy, recomputed at every call rather than copied at creation.
  - Depth is checked at spawn, including for a custom agent that has `delegates`.
  - PR 4's "allow for this turn" does not pass into a child. That this is the safer reading is our inference; test it.
  - A writer child whose worktree cannot be created fails the spawn. OpenHands and Hermes Agent fall back to the shared tree, which puts two writers on one tree.
- **Evidence.**
  - Codex's `explorer.toml` is empty: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/role.rs#L316-L376
  - OpenCode's `explore` allows `shell *`: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/agent.ts#L80-L93
  - OpenCode's creation-time copy: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session.ts#L311-L314
  - Codex's last-step re-apply: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/agent/child_config.rs#L197-L218
- **Invariants.** Node decides. PR 6's server-side check is master-side narrowing, and the node must check again.

**A9. Loop guard, limits and a result ceiling in the dispatcher.** `main`. Small to medium.

- **Loop guard.** Hash the tool and its arguments, and the outcome too. Warn at the third repeat and block at the fifth. Stop hard on routines and only warn in attended chats.
  - The decision is made at admission, before the call runs, and an error in the guard blocks.
  - The guard's state is derived from the log fold, so a restart rebuilds it.
  - Copy Hermes Agent's thresholds: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/tool_guardrails.py#L26-L66
  - Copy OpenClaw's admission point: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/tool-loop-admission.ts#L103-L109
  - Do not repeat goose's guard, which counts on a clone and never accumulates: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/tool_monitor.rs#L99-L134
- **Limits.** Move the hard-coded cap of 50 model calls, a per-turn wall clock and a maximum tool deadline into `node.toml` `[limits]`.
  - At the cap, keep the tool schemas, send `tool_choice: none` and refuse any late call (OpenCode; 6.3 in `round2/synth/01-loop-dispatch-durability.md`).
  - Clamp any model-supplied timeout, and never let 0 mean infinite.
- **Result ceiling.** One cap on every tool result at the dispatcher, proportional to the context window. OpenClaw uses 30% of the window, with 16k, 32k or 64k characters. Cap while reading, not after.
- **Invariants.** Node decides: the limits live in `node.toml`. Light process: the cap applies while reading.

**A10. Pull request 7, retry.** Small.

- **What to adopt.**
  - One function from error to policy, on structured fields.
  - Never retry after visible output was streamed or a tool ran.
  - Cap `Retry-After`; Oh My Pi fails fast above 300 s.
  - A separate patient schedule for no network.
  - Billing errors are not retried.
  - The same helper for title, summary and compaction calls.
  - A per-turn send counter shared by children.
  - Empty responses as their own retry class.
  - Cancel aborts the sleep.
- **Copy from.**
  - Codex's classification: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/protocol/src/error.rs#L445-L511
  - Oh My Pi's replay-safety rule: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/turn-recovery.ts#L1566-L1605
  - OpenCodex's per-request accounting, which is `unchecked`: 6.5 in `round2/synth/09-subagents-models-retry.md`.
- **Round 1 overturned.** Round 1 said copy Pi's retry defaults. Pi's provider layer defaults to 0 retries, so `Retry-After` is read nowhere, and Pi has no jitter despite a comment saying it does (4.9 in `round2/synth/09-subagents-models-retry.md`). PR 7's numbers (3 retries, 2 s base, 60 s cap) sit inside the range the code shows. The model to copy is Codex's classification, not Pi.

**A11. Pull request 7, compaction.** Medium.

- **What to adopt.**
  - A cache-preserving summary request, with leftover headroom in the trigger.
  - Fallback tiers, ending in a model-free summary and then dropping the oldest history.
  - Abort and preserve the session unchanged on terminal failures.
  - A check that compaction actually shrank the context.
  - A breaker after two compactions that did not help.
  - Estimator correction from the provider's real count.
  - One log event, written after the summary exists.
  - Instruction files re-injected after compaction.
  - Redaction applied before a summary is stored.
- **Copy from.**
  - OpenCode's request: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/compaction.ts#L451-L470 (checked in review)
  - Cline's `basic`: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/context/basic-compaction.ts#L452-L512 (corrected in review)
  - Cline's single forced retry: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/agents/src/agent-runtime.ts#L1378-L1423
  - Hermes Agent's abort-and-preserve: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/context_compressor.py#L4080-L4175
  - Hermes Agent's breaker: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/agent/context_compressor.py#L3015-L3022
  - DeepSeek harness's shrink check: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/compaction/compaction-basic/src/region.ts#L431-L468
  - Cline's estimator: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/context/compaction.ts#L211-L230
- **Caution.** PR 7's "reads in pieces" summariser is a hypothesis. No finding describes a map-then-merge summariser as a primary path (absence not confirmed). Keep it as the lowest tier.
- **Round 1 overturned.** Round 1 said copy Pi. Pi's summary request is uncached (https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/compaction/compaction.ts#L618-L638) and Pi has no shrink check.
- **Invariants.** One writer: compaction is an event appended by the session actor. No secret in event: the summary is redacted.

**A12. Seccomp under Landlock, and a protected `.git`.** PR 4 follow-up. Medium. Security code.

- **What.** Install Codex's `Restricted` rule set inside `sandbox-exec`, after `restrict_self` and immediately before `exec`:
  - With network off: `socket` only for `AF_UNIX`; deny `connect`, `bind`, `listen`, `accept`, `sendto`, `sendmmsg` and `recvmmsg`.
  - Always: deny `io_uring` and `AF_VSOCK`; deny `ptrace` and `process_vm_*`.
  - Denials return `EPERM`, never `SIGSYS`.
  - An unknown architecture is refused in `required` mode.
  - Decide `sendmsg` explicitly. Codex leaves `sendmsg` and `recvmsg` open (reviewer correction). Either deny it and test the toolchain, or list it as a named residual in `PLAN.md`.
  - Keep `.git/hooks` and `.git/config` read-only in the session folder, including when `.git` did not exist at start.
  - Show in the node status and the tool result which rules are active.
- **Evidence.**
  - Codex's filter: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/linux-sandbox/src/seccomp.rs#L183-L301
  - The full rule table is in 6.2 and 6.3 of `round2/synth/03-sandbox-containers-execution.md`.
- **Invariants.** Node decides. No `unsafe` in our code, and the seccomp crate needs the dependency check.
- **Why a follow-up PR.** We suggest stacking it on PR 4 rather than adding it to PR 4, so the outside review of PR 4 is not reset. That is our judgement.
- **Round 1 overturned.** Round 1 said copy DeepSeek harness "for honesty" about `full` and `partial` enforcement. Its Landlock launcher reports `full` on ABI 6 and later while confining writes only, and no client reads the value (https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/native/system/packages/entry/src/main.c#L70-L94, checked in review).
- **Round 1 softened.** Round 1 said Codex rejects Landlock because it cannot isolate Unix sockets. The code states only "isolate app-server sockets"; the socket reason is the author's inference.

**A13. A seen-version guard on `edit` and `write`.** `main`. Medium.

- **What.**
  - A per-session table keyed by canonical path, holding a SHA-256, `dev:ino:size:mtime_ns:ctime_ns`, the ranges shown in full and a bounded snapshot of the shown text.
  - The version is checked inside the per-path lock, before matching.
  - `edit` without a prior read is refused, and `write` over an existing file needs a full read.
  - A stale file follows the ladder in 6.1 of `round2/synth/08-edit-tools-mechanics.md`: apply only when the matched span is byte-identical to what the model saw; otherwise refuse and return a bounded diff.
  - Hashes go into the `read` events, so a restart rebuilds the table by fold.
  - Writes are atomic: temp file, `fsync`, rename.
  - Multi-edit batches are matched against the original content.
  - Matching stops at a one-to-one Unicode fold, and the result reports which tier matched.
- **Copy from.**
  - DeepSeek harness's check inside the lock: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/fs/fs-local/src/index.ts#L250-L277
  - Hermes Agent's full-read rule: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/file_tools_write_guards.py#L166-L173
  - Pi's batch: https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/edit-diff.ts#L343-L406
- **Evidence.** Our `edit` is exact-replace with no staleness check (verified).
- **Invariants.** One writer: the table is a fold over the log. Light process: snapshot text goes on disk, hashes stay in memory.
- **Depends on.** A11. Clearing "ranges shown in full" at compaction is `inferred` and unmeasured.

**A14. Checkpoint restore safeguards and a recorded head.** `main`. Medium.

- **What.**
  - Snapshot the present before restoring, so a restore can itself be undone.
  - Send `expected_tree` with the request and refuse when the live tree differs, unless the restore is forced.
  - Refuse during a turn, and when another session shares the worktree.
  - Refuse when the tree holds a nested repository.
  - Keep files no snapshot holds, such as files over the size cap.
  - For "from that turn on", restore only paths the turns being undone touched.
  - Validate every ref name and tree id against the node's own shadow repository.
  - Append a `HeadMoved` event so a rewind survives a restart.
  - Take no snapshot for a turn that only read.
  - Add timing and coverage fields to the `Checkpoint` event.
- **Copy from.**
  - Cline's restore transaction: https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/session/checkpoint-restore.ts#L50-L154
  - Hermes Agent's nested-repository refusal: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1376-L1385
  - Hermes Agent's handling of oversize files: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/checkpoint_manager.py#L1429-L1452
  - OpenCode's attributed paths: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/session/revert.ts#L22-L56
  - Codex's compare-and-set of the active history: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/thread-store/src/local/revert_thread.rs#L138-L156
- **Invariants.** One writer: the node actor writes `HeadMoved`. Node decides: a restore names a tree that exists on the node, never a path.
- **Depends on.** Tests D3, D4 and D13 before any change to cost.

**A15. Keep streaming deltas out of the event log, and resume by cursor.** `main` and PR 4. Medium.

- **What.** Split frames into two kinds.
  - Durable frames carry `(session_id, owner_epoch, seq, hash)`. The node sends them from a cursor over its own log, and the master acknowledges only after its mirror commits.
  - Transient deltas carry no `seq`, are coalesced latest-wins and are dropped first under back-pressure. The durable message supersedes them.
  - The browser subscribes first, then replays to a captured watermark, then follows.
  - Per-socket queues are bounded, and a lagging client is disconnected.
- **Copy from.**
  - OpenHands' frame envelope: https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/src/types/agent-server/session-frames.ts#L1-L79
  - OpenCode's log contract: https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L829-L878 (checked in review)
  - Codex's rule to reject new requests but never drop responses: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/app-server-transport/src/transport/mod.rs#L231-L262
- **Invariants.** One writer. Light process: no unbounded buffer on either side. It fixes AUDIT D5.
- **Depends on.** A2. First settle in code whether deltas are still written to the log on the current branches (test D16).

### (b) Change in the design note before building

Each item names the section of `docs/nodes-credentials-environments.md` it affects.

**§4.1 Data model.**

- Add `nodes.link_gen`, `sessions.owner_epoch` and `session_forks.hash_at_through_seq` (A2).
- State the AES-GCM associated data for `workspace_secrets`: bind `workspace_id` and `name`, so a row copied to another workspace or name fails to open. Warp binds actor, name and type into its sealing: https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/crates/managed_secrets/src/envelope.rs#L33-L80.
- Store only hashes of any bearer the master issues. OpenClaw hashes worker credentials but keeps device tokens in plaintext (6.5 in `round2/synth/05-links-workers-protocols.md`).
- Invariant: no secret in event, URL or log.

**§4.2 Model aliases and slots.**

- Model "strict" as `Option<Vec<_>>`, so a defined empty fallback list fails instead of switching. That is OpenClaw's `fallbacksOverride = []`, `unchecked`.
- Add `timeout` and reasoning effort to each slot, after Hermes Agent's slot shape.
- Keep slot names as strings in a set the master validates, not a protocol enum.
- A slot failure must not fail the turn.
- A slot or child model on a different provider needs its own grant (6.4 in `round2/synth/09-subagents-models-retry.md`).

**§4.3 Granting model access.**

- **TTL.** Replace the 6-hour `expires_ms` with minutes, renewed by the master while the turn is live. Drop the grant on `grant.drop`, at turn end, on link loss after the turn and on a newer grant.
- **Binding.** Bind the grant to `(node_id, session_id, owner_epoch, turn req_id)`. The node refuses it for any other session, and the master refuses to send it to a node of another workspace.
- **Scope.** Check scope against the models resolved after alias expansion, not the names the agent spec carries: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/server/admission-model-scope.ts#L8-L29.
- **Minting.** A new grant cancels in-flight calls that used the old one: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/gateway/worker-environments/credential-broker.ts#L105-L126.
- **Revalidation.** Check the grant on every `session.prompt`. Codex checks bearer credentials once at upgrade and documents that expiry is not enforced after: https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/websocket-auth/src/lib.rs#L286-L330.
- **The proxy.** For the reserved `ModelAccess::Proxy`, write the bounds first: stream the body once, a shared byte budget with 503 and `Retry-After`, a pull stream, a retry budget on the logical request, failover only before output. OpenCodex is the reference, and several of its findings are `unchecked` (item 3 of section 6 in `round2/synth/04-credentials-secrets-auth.md`). Its "queue of one" holds only for the generic streaming bridge.
- **Embedded node.** The local node that runs in the master process must not be able to read the master's secrets (A3).
- Invariants: node decides (a grant is refused when `model_keys` is `local` or `none`); no secret in event, URL or log.

**§4.4 Adding a node with a pair code.**

- Consume the code before validating anything else, so a rejected request still burns it: https://github.com/lidge-jun/opencodex/blob/ddee2c20030c1f0ec852202256be877397e8f396/src/remote-control/workspace-hub.ts#L377-L425.
- Refuse requests carrying an `Origin` header.
- Rate-limit by the kernel-observed peer address, and answer every failure with one opaque reason.
- Send the code in a POST body, never a link.
- Show a 128-bit fingerprint in groups on both sides, not 16 hex characters. goose uses eight groups: https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose-roaming/src/card.rs#L56-L75.
- Invariants: no secret in a URL; loopback default.

**§4.5 `node.toml` additions.**

- Add `[limits]` for steps, a per-turn wall clock and a tool deadline maximum (A9).
- Add a checkpoint time budget and a byte cap (A14).
- Container caps a session may lower and never raise.
- No `extra_args` key, now or later.
- Make "systemd credentials, else a 0600 file" an explicit, logged choice at join, not a fallback on error. OpenCodex fails closed with "no plaintext fallback".
- Invariant: node decides.

**§4.6 The container the node builds.**

- **Argv.** Adopt the argv in 6.1 of `round2/synth/03-sandbox-containers-execution.md`. It adds the following to the current note:
  - an argv-hash label, with the container recreated on mismatch;
  - sized `nosuid,nodev,noexec` tmpfs on `/run`;
  - `--ulimit core=0`;
  - `--network=none` by default;
  - `--pull=never` with the image pinned by digest;
  - a check that the created id is 64 hex digits;
  - `--env NAME` with the value in the `podman` child's environment;
  - one mount whose source is validated against the data directory, `$XDG_RUNTIME_DIR` and `~/.ssh`.
- **Evidence.** OpenClaw's validation: https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/agents/sandbox/validate-sandbox-security.ts#L23-L53. Hermes Agent's appended extras are the hole to avoid: https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L741-L745.
- **Self-test.** Add `podman inspect` assertions: no privilege, empty `CapEff`, one mount, a read-only root, the network mode, the label.
- **Kill.** Keep the three-step kill. `podman stop` is the guarantee. Stop a session's containers as soon as its actor is lost.
- **Same checks inside.** Approval, the safety floor and checkpoints are identical for host and container sessions. Hermes Agent skips its checks inside containers (9.16).
- **Network on.** Do not ship `network = "on"` until test D14 shows what rootless `pasta` exposes on the host's loopback.
- Invariants: node decides; loopback default.

**§4.7 Forge operations.**

- Hand forge tokens only to the git child, scoped to `[forge] hosts` and short-lived.
- Expect the agent to be able to read them whenever it can run git. OpenClaw's worker GitHub token is readable by the agent (corrected finding).
- Strip `GIT_DIR`, `GIT_CONFIG*` and `GIT_WORK_TREE` for every forge git call (item 5 of section 6 in `round2/synth/04-credentials-secrets-auth.md`).

**§4.8 Deleting an environment.**

- Deletion must stop unattended runs, not only interactive ones.
- Deletion bumps `owner_epoch` for every affected session, so late appends from a node are refused (A2).

**§4.9 Fork.**

- **Lineage.** Record lineage inside the child's log. The child's first event is `SessionForked { parent, root, parent_node_id, through_seq, cut, lossy, files, files_ref }`, followed by `history_imported` batches marked `inherited` and a `SeedEnded` marker.
- **Cut point.** `through_seq` is the highest contiguous mirrored seq, never an unmirrored tail. The default cut is the last completed turn.
- **Open turns.** A cut inside an open turn closes it with fork-specific closers.
- **Fresh child state.** The child starts at `ask` with no grants and no "allow for this turn". Usage counters start at zero.
- **Files.** A same-node fork can seed its worktree from the checkpoint tree of the cut turn (`files: "checkpoint"`). A cross-node fork stays on the forge route.
- **Evidence.** DeepSeek harness's fork: https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/session/src/fork.ts#L21-L30. Further detail is in 6.2 of `round2/synth/05-links-workers-protocols.md` and items 1 and 8 of section 6 in `round2/synth/06-sessions-fork-checkpoints.md`.
- **Round 1 overturned.** Round 1 proposed a Pi-style parent pointer over our log. The child runs on a different node and cannot read the parent's file, and Pi's path-only parent link orphans on rename. Copy semantics are the fit.
- Invariant: one writer.

**§6 How this breaks.**

- Replace "a second connection replaces the first" with the fence in A2.
- Keep "revoke after 3 flaps in 10 minutes", and make the revocation bump every affected session's epoch.

### (c) Build next, in this order

Each step is a feature, so it needs the owner's decision to break the freeze. The order follows risk and dependency. A step does not start until the safeguards listed for it are in the same change.

| # | Build | Safeguards from day one | Evidence | Depends on |
|---|---|---|---|---|
| 1 | Unattended authorisation for routines and webhooks | A per-routine allow set created by a person, content-hashed and recorded as an event. Anything outside it is denied with the real reason. The schedule advances before dispatch, and an idempotency record is fsynced before the first model call; an unknown outcome pauses the routine. An agent-created routine starts `pending_approval`. Webhook idempotency keys live in the database. Never auto-approve | Step 1 of 6.1 in `round2/synth/10-extensibility-unattended.md`; 6.7 in `round2/synth/01-loop-dispatch-durability.md` (Hermes Agent `cron/scheduler_tick.py`, `cron/scheduler_authority.py`) | A7, A9 |
| 2 | Workspace nodes, pair codes, per-turn grants (design note) | The §4.1, §4.3 and §4.4 changes above; abuse tests for a grant used by another session and a prompt after node removal | 6.1 and 6.6 in `round2/synth/05-links-workers-protocols.md` | A2, A3, A15 |
| 3 | Environments (rootless Podman, worktree per session) | The §4.6 argv, the `podman inspect` self-test, stopping containers on actor loss, identical checks inside | 6.1 to 6.5 in `round2/synth/03-sandbox-containers-execution.md` | Step 2; tests D14 and D15 |
| 4 | Fork by copy with lineage | The §4.9 items; refuse "continue elsewhere" during a running turn | 6.2 in `round2/synth/05-links-workers-protocols.md` | A2, A14 |
| 5 | Web and memory tools on agentic nodes (today they are registered in the chat runtime only) | Memory approval enforced in the write path and failing closed. A staged write carries the base version of the entry. Shared memory stores live outside the shell's writable set. Threads that touched external content are excluded from automatic extraction. Web tools keep URL provenance and the SSRF resolver | 6.2 in `round2/synth/07-context-compaction-memory.md`; Round 1 section 8, table stakes item 4 | PR 4 network block |
| 6 | Event vocabulary for context changes | A prompt-section delta event and a tool-loadout event, folded by node and UI; sorted tool names; changes appended, never edited into the prompt head | Step 2 of 6.1 in `round2/synth/10-extensibility-unattended.md` | A11 |
| 7 | Skills (text only) | Node-controlled roots; repository skills only after a per-project trust decision bound to content; a capped catalog; the body fetched through the approval path; nothing executes at load | Step 3 of 6.1 in `round2/synth/10-extensibility-unattended.md`; OpenClaw https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/skills/loading/skill-prompt-limits.ts#L10-L40 | Step 6 |
| 8 | MCP client with deferral | Only `node.toml` adds a stdio server. Its environment comes from the PR 4 allow-list. It runs inside the session's sandbox or container. Approval is per server and tool, bound to the executable's digest, and a `readOnlyHint` only tightens. Size and time caps. Deferral is switched on by measured tool-list size, with activation recorded as a loadout event. OAuth tokens are granted per turn like provider keys | Step 4 of 6.1 in `round2/synth/10-extensibility-unattended.md`; Hermes Agent https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/mcp_tool_config.py#L199-L207 | A1, steps 3 and 6 |
| 9 | Hooks | Node only; under the PR 4 sandbox; trust bound to the bytes of every referenced file and stored where only the node owner writes; fail closed on crash, timeout, bad output or a bad matcher; a hook may deny or escalate, never approve; policy re-resolved after a rewrite | Step 5 of 6.1 in `round2/synth/10-extensibility-unattended.md`; Oh My Pi https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/extensibility/extensions/wrapper.ts#L355-L374 | Step 8 |
| 10 | Input during a turn | The message is persisted with an idempotency key before it is acknowledged, delivered at a step boundary, never kills a running tool, and a dropped message gets a receipt | 6.10 in `round2/synth/01-loop-dispatch-durability.md` | A6 |
| 11 | Per-command rules, then a plan mode | A maintained parser (`tree-sitter-bash` or `brush-parser`, after the dependency check). Allow rules must cover every segment; deny rules hit any segment after unwrapping wrappers. Rules are never a click and never lift the sandbox. Plan mode is enforced in the registry, the dispatcher and a read-only Landlock profile, and only the user can leave it | Items 2, 5 and 9 of section 6 in `round2/synth/02-approvals-permission-rules.md` | A1, A12 |

Plugins as bundles and code mode come after step 9, if at all. Code mode is the one feature in the set that can defeat the approval path (goose, 9.30).

### (d) Tests to write against our own code now

Each settles an open risk. Most should fail or surprise on `main`.

| # | Experiment | What it settles | Blocks |
|---|---|---|---|
| D1 | The agent runs `setsid sleep 1000`, then the call is cancelled | Whether a detached child survives our process-group kill. goose and Cline confirm the leak class; for Oh My Pi and Pi it is `inferred` | Choice of cgroup, PID namespace or a re-walk (A12, §4.6) |
| D2 | `kill -9` the node while `bash` runs a long child; restart | Whether children outlive the node. `PR_SET_PDEATHSIG` from `pre_exec` needs `unsafe`, so a supervisor or cgroup may be required | A6, §4.6 kill ladder |
| D3 | Snapshot, rewrite history in the user's repository, run `git gc --prune=now`, restore | Whether alternates let a shadow-repository checkpoint lose objects. OpenCode's version of this risk is `inferred` | A14; whether to copy blobs into our own store |
| D4 | Time a checkpoint over synthetic trees of 1k, 10k, 100k and 500k files, warm and cold cache, with 0, 10 and 1,000 changed files | The real cost of our full-tree scan; nobody in the input measured it | Any checkpoint budget in `node.toml` |
| D5 | Queue two event frames on connection A, connect B for the same node before A's frames are dispatched | Whether frames from a replaced connection are still applied after `nodes.rs:207` installs the new one | A2 |
| D6 | Archive a session while its node is offline; reconnect | Whether a stale node can append to a closed session | A2, §4.8 |
| D7 | `kill -9` the node during a `bash` call; restart | Whether the log holds the call's arguments and a closing event that says "unknown" | A6 |
| D8 | Fill the disk, then start a tool call | Whether a failed append stops the tool | A6 |
| D9 | A provider fixture ending a `write` call mid-JSON with `finish: length` | Whether we run truncated arguments | A5 |
| D10 | A sandboxed shell runs `env`, reads `/proc/self/environ` and `/proc/$PPID/environ` during a turn with a grant | Whether a grant or link key reaches the agent | A3, §4.3 |
| D11 | A sandboxed shell reads `/proc/$PPID/mem` and tries `gdb -p` on the node | Whether Landlock alone stops memory access to the node; no input settles it | A3 (`PR_SET_DUMPABLE`) |
| D12 | A sandboxed command connects to every socket the node and master open (master WebSocket, node link, MCP bridges, the Podman socket) | Whether below ABI 9 a confined process can reach an unauthenticated local socket | A12, step 8 |
| D13 | Rewind to an earlier turn, restart the node, open the session | Whether files and conversation disagree after a restart, which would show that `HeadMoved` is needed | A14 |
| D14 | Start a `network = "on"` rootless container with `pasta`; connect to `127.0.0.1:7777` and the node's sockets | What rootless networking exposes on the host loopback; no input covers it | §4.6 network row |
| D15 | Kill the `podman exec` client during a long command | Whether the signal reaches the process inside the container; this is open in the design note too | §4.6 kill ladder |
| D16 | Stream a long answer and count events in the log | Whether deltas are still stored as events on the current branches (the inventory and the brief disagree) | A15 |
| D17 | The node streams output at the link cap while a cancel and an approval answer arrive | Whether a full queue blocks control messages beyond the grace period | PR 4 back-pressure |
| D18 | A Scout calls `write` by name although it is not in its `tools:` list | Whether the node refuses at execution, not only by hiding the tool | A8 |
| D19 | Compact a session whose tool results contain a provider key | Whether the summary stores the secret | A11 |
| D20 | `git add -A` over a tree containing a nested `.git` directory | What our checkpoint captures and what a whole-tree restore would do | A14 |

### (e) Do not copy

| Do not copy | Project that shows why | Evidence | Invariant at stake |
|---|---|---|---|
| A model-set flag that lowers the gate | Warp's `is_risky == Some(false)` skips the allow-list | https://github.com/warpdotdev/warp/blob/4b790f5fac9a2cc637c865aacbfa4f2cf64f6c39/app/src/ai/blocklist/permissions.rs#L972-L1008 | Node decides |
| An allow rule that lifts the sandbox | Codex `Skip { bypass_sandbox: true }` | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L492-L512 | Node decides |
| Dropping every rule on a parse error | Codex | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/core/src/exec_policy.rs#L729-L744 | Node decides |
| Hooks that fail open | Codex `PreToolUse` | https://github.com/openai/codex/blob/82e6bacb9c6950d3b56cb153847fa80bdd698053/codex-rs/hooks/src/events/pre_tool_use.rs#L211-L300 | Node decides |
| Unattended equals auto-approve | goose scheduled jobs in `Auto`; Cline cron yolo | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/scheduler/full.rs#L881-L907 ; https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/cron/runner/cron-runner.ts#L84-L108 | Node decides |
| Pass-through container arguments | Hermes Agent `docker_extra_args` appended last | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/environments/docker.py#L741-L745 | Node decides |
| Relaxing checks inside a container | Hermes Agent skips its floor and prompts | https://github.com/NousResearch/hermes-agent/blob/95fc3e668fc1e13ad05d22a4f02fccc6a6d6ce02/tools/approval.py#L1161-L1175 | Node decides |
| A control-plane key in the agent's environment | OpenHands session key | https://github.com/OpenHands/OpenHands/blob/d7293dfd671e23dab182fe71c247ff05f48e780b/scripts/dev-with-automation.mjs#L975-L985 | No secret in event, URL or log |
| An empty approver list meaning "anyone in the chat" | OpenClaw | https://github.com/openclaw/openclaw/blob/8bac830dcb8ff99055ba71558dd369a989cc7881/src/plugin-sdk/approval-auth-helpers.ts#L72-L89 | Private chats |
| Repository files that loosen user settings | Oh My Pi project layer merged with no key filtering | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/settings.ts#L2800-L2814 | Node decides |
| Children forced to the most permissive mode | Oh My Pi `yolo` overlay | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L1259-L1278 | Node decides |
| Code-mode inner calls that skip the gate | goose `lease.call` | https://github.com/aaif-goose/goose/blob/14bf51817b79f6e98ca73c6eda61c11d9d5cc892/crates/goose/src/agents/platform_extensions/code_execution.rs#L172-L194 | Node decides |
| Similarity matching at low thresholds | Cline 0.66, Zed 0.8 per line | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/extensions/tools/executors/apply-patch-parser.ts#L347-L430 ; https://github.com/zed-industries/zed/blob/3def8bad0b62dbda02d5b67b00df96fe8d3957bd/crates/agent/src/tools/edit_session/streaming_fuzzy_matcher.rs#L254-L256 | None; a correctness risk |
| NFKC folding that rewrites whole lines | Pi `widenReplacementsToLines` | https://github.com/earendil-works/pi/blob/91d9719b4123a83816249d7a7917c75126bb55af/packages/coding-agent/src/core/tools/edit-diff.ts#L176-L214 | None; a correctness risk |
| Checkpoints as uncapped refs in the user's repository | Cline | https://github.com/cline/cline/blob/500a40e7b76692153906b44dd68732520a966663/sdk/packages/core/src/hooks/checkpoint-hooks.ts#L535-L649 | Light process (disk) |
| A durable log that is off by default | OpenCode `persist` false in the shipped server | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/bus.ts#L186-L192 | One writer |
| Latest connection wins as the only fence | OpenCodex Remote Workspace, and our `main` | 6.1 in `round2/synth/05-links-workers-protocols.md` | One writer |
| Full environment inheritance for MCP children | Oh My Pi, Pi, Cline, Warp | 4.1 in `round2/synth/10-extensibility-unattended.md` | No secret in event, URL or log |
| A guard that ships disabled, or a read-only role that is only a name | DeepSeek harness observation policy; Codex `explorer.toml`; OpenCode `explore` | Section 1, "The general finding" | Node decides |

### Round 1 recommendations that Round 2 changes

| Round 1 said | Round 2 shows | Evidence |
|---|---|---|
| Copy Pi's retry defaults | Pi's `Retry-After` is dormant by default, and it has no jitter; copy Codex's classification | A10 |
| Copy Pi's compaction | Pi's summary is uncached and unchecked for shrinkage; copy OpenCode, DeepSeek harness and Cline | A11 |
| Copy DeepSeek harness's `full`/`partial` honesty | It reports `full` while confining writes only, and nobody reads the value | A12 |
| Codex rejects Landlock because it cannot isolate Unix sockets | The code says only "isolate app-server sockets"; the socket reason is inferred | 9.14 |
| Fence stale nodes like OpenClaw and OpenCodex | OpenCodex is latest-connection-wins; only OpenClaw fences | A2 |
| Fork with a Pi-style parent pointer | A copy with lineage in the child's log fits a child on another node | §4.9 above |
| Copy OpenCode's plan-mode enforcement | OpenCode's plan denies edit tools only; shell, sub-agents and execute stay allowed (corrected) | https://github.com/anomalyco/opencode/blob/05d347d75054bf27778750ad5f2cd30336e81d8f/packages/core/src/plugin/plan.ts#L33-L42 |
| Copy Pi's separate project trust gate | Pi's gate is a path boolean, and project context files load regardless of trust | 4.7 in `round2/synth/10-extensibility-unattended.md`; 4.9 in `round2/synth/07-context-compaction-memory.md` |
| OpenClaw stores steered input before acknowledging it | True for the channel and `agent` paths that were checked; the in-flight steering list is in memory | 4.5 in `round2/synth/01-loop-dispatch-durability.md` |
| Zed removed per-prompt checkpoints because they hung large repositories | The code does not say why; the reason is inferred from the release note | 4.6 in `round2/synth/08-edit-tools-mechanics.md` |
