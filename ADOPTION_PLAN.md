# ADOPTION PLAN: rizzy-harness

**Status:** accepted by the owner on 2026-11-11 with one change (O3: a node may serve several
workspaces, see C.12). **Progress, 2026-11-11:** Phase A tasks A.1 to A.14 are written and merged
(A.1 to A.5, A.8 to A.10, A.12, A.13 into `integration/gate3`; A.6, A.7, A.14 into pull request
7; A.11 into pull request 6). A.15 and the new A.16 are open. Experiments B.1, B.2 and B.5 have
run. Phases C, T, N, X and W have not started.

Brings what the two-round harness study found into our own code. Inputs, by authority:
`research/round2/final-review-ours.md` (what is true about our code; it wins every
disagreement), `research/round2/synth/_narrative/10-applications.md` (recommendations, parts a
to e, corrected in place), `01-executive-summary.md` beside it (the ranking followed here) and
`docs/nodes-credentials-environments.md` (the node design; its build order becomes Phase N).
Section 9 maps every recommendation to a task or to "dropped".

State when written: `main` holds fix plan Phases 0, 1 and 2.1 to 2.6. Pull request 4
(`integration/gate3`) holds 2.7 to 3.9, CI green, awaiting Gate 3's outside review. Pull
requests 6 (`feat/agent-roster`) and 7 (`fix/provider-retry-and-compaction`) are stacked on
it. The development machine has no Node runtime and no container runtime.

Task IDs carry a phase letter (`A.1`, `N.3`) so they cannot be confused with the fix plan's
(`3.10`). The study's experiments are cited as "test D3" (no dot). The building phases are
lettered by subject (T, N, X, W) rather than D, E, F, so no task ID reads like a test.
Evidence cites `research/round2/synth/<file>` by number and heading, for example `01` §6.1.

## 1. Rules for the plan

1. **The fix plan comes first.** Phase A fixes what is built and may land before Gate 3.
   Phase B adds tests, not behaviour. Phase C is paper. Phases T, N and X are features and
   start after Gate 3. Phase W holds fix plan 3.10, which is part of Gate 3.
2. **Ideas, not code.** Copying code needs a permissive licence and a `THIRD_PARTY_NOTICES.md`
   entry in the same pull request. Never copy from the AGPL or GPL projects.

   | Project | Licence | Copy code? |
   |---|---|---|
   | Codex, Cline, goose | Apache-2.0 | Yes, with a notice |
   | OpenCode, OpenClaw, Hermes Agent, DeepSeek harness, Pi, Oh My Pi | MIT | Yes, with a notice |
   | OpenHands | MIT (`enterprise/` unverified; cited paths are outside it) | Yes, outside `enterprise/`, with a notice |
   | OpenCodex (`lidge-jun/opencodex`, not the AGPL `RyensX/OpenCodex`) | MIT | Yes, with a notice |
   | Warp | AGPL-3.0 | **No.** Ideas only (the AAD binding in C.5) |
   | Zed | GPL | **No.** Counter-example only |

3. **Every task names its failing test,** written first, failing on the old code. Docs-only
   tasks name the tests they commit later tasks to.
4. **One task, one pull request.** Pull request 4 departed from this by the owner's decision;
   this plan does not.
5. **Security code gets the full stress test:** the six questions of `CLAUDE.md` answered in
   the pull request, plus an abuse-case test. Such tasks are marked **sec**.
6. **Nothing in a task may let the master widen a node.** Every value the master sends (agent
   spec, allow set, grant, container request, rule) can only narrow `node.toml`. Each such
   task has a test where the master asks for more and the node refuses.
7. **A new dependency** follows `CLAUDE.md`: exact pin, API read on docs.rs, at least two weeks
   old, passes `cargo deny`, and the pull request says why this crate.
8. **Docs move with code:** `README.md` for user-visible changes, `PLAN.md` for the threat
   model, the note in `docs/` for design, all in the same pull request.
9. **Say what was not run.** Tests needing Podman or Node are marked; if not run locally, the
   pull request says so and names where they ran.
10. **No compatibility code before the first tag.** Event and schema changes are in place.

Sizes: **S** under half a day, **M** one to two days, **L** three to five days, for one person
working with an agent. Estimates, not promises.

## 2. Decisions already made (recorded, not reopened)

- The fix plan finishes first; features wait for Gate 3. Fixes to what is built are not features.
- Podman is the primary container runtime; Docker only as a self-hosting alternative.
- Provider keys are bring-your-own-key per workspace, held on the master, granted to a
  workspace node per turn, in memory only. A model-call proxy waits for the experimental
  per-user node.
- Every node added to a workspace is trusted and is added by a pair code; no trust flag.
- A node may be attached to several workspaces, each attachment by its own pair code, and the
  design is built around that (owner, 2026-11-11; C.12). The built-in `local` node is an
  ordinary node under this rule.
- Three more decisions of 2026-11-11 (owner; note §4.10.6, §4.11, §4.12): each workspace has its
  own directory on a node and each session its own folder in it, derived on the node; no
  conversation is kept at rest on a node beyond what the master has not yet stored; and `local`
  is shareable like any other node.
- Agentic work happens only inside a git repository; new projects, and sessions without a
  project, get a repository the node initialises (owner, 2026-11-12; note §4.11.5).
- Environments are per workspace. Deleting one classifies its sessions, schedules when work is
  unsaved, and archives them read-only. No migration; continuing elsewhere is a fork.
- Embeddings and voice stay server-wide.
- Model aliases (named ordered groups of models) serve agents and auxiliary slots.
- The built-in roster is the eight agents of pull request 6.
- When the centre requires an update, the node drains: running agents are asked to finish or
  pause, get five minutes to respond (an unanswered turn's running command is then stopped, not
  the turn), then a graceful phase of at most thirty minutes from the start of the drain, then a
  forced restart; the master does not cut off a draining node. A SIGTERM or SIGINT starts the same
  drain: a request, not a kill. A node whose own `node.toml` sets `update_mode = "auto"` may fetch
  a compatible build from its master over a node-authenticated endpoint; the master never pushes
  one (owner, 2026-11-12; C.14, `docs/node-update.md` §2).

## 3. Decisions put to the owner, and the answers

**Answered on 2026-11-11.** O3 was answered differently from the recommendation: see its row.
Every other row was accepted as recommended. The table is kept as the record of what the
other choice would have cost.

| # | Decision | Recommended | If you choose the other |
|---|---|---|---|
| O1 | PR 7's compaction default: 24,000 tokens as before, or the window minus a reserve | **Window minus a reserve.** At 24k a 200k model summarises after a few tool results and loses the task; summaries are lossy. Cost-sensitive operators set `summarize_after_tokens`; the README says what it costs. | 24k: cheaper per turn, many more summaries, and summary quality (A.7) becomes the main way context is lost. Set it before PR 7 merges. |
| O2 | Does a "retrying in N s" notice justify a new event type? | **No, not now:** a schema change under the freeze, with UI that needs Node. If fix plan 4.8's ADR brings transient frames, the notice is one. | A `provider_retry` event now: a `PLAN.md` schema change, a fold change and W.5. |
| O3 | Where the in-process `local` node fits once nodes belong to a workspace (the note's one blocking question) | Recommended was the note's working assumption: `local` alone as a server-admin node usable in the admin's workspaces. **The owner chose more: any node may be attached to several workspaces, and the logic is to be built around that.** `local` stops being a special case. | What it costs: two workspaces now share one machine, one Unix user and one link, so separation between them has to be built and tested instead of assumed (C.12). One node per workspace would have had that separation for free. |
| O4 | seccomp beside Landlock (a new dependency) | **Yes, after Gate 3's outside review, if the dependency check passes** (candidate `seccompiler`, Apache-2.0, `unsafe` kept inside it). Only seccomp closes UDP and DNS with `network = "off"`. T.7's read-only shell needs nothing new and goes first. | No: UDP and DNS stay named residuals; T.6 is dropped. |
| O5 | Per-user API tokens instead of the one administrator-equivalent token | **Yes, after Gate 3** (T.5, after fix plan 4.1). Until then A.13 records the bypass. Scoping the token now breaks the README's script uses. | Keep it: "chats private, for admins too" stays broken by design and the threat model says so permanently. |
| O6 | May routines ever act unattended? | **Yes, only inside a person-created, content-hashed allow set** (C.7, T.4); anything else denied with the reason; never auto-approve. | Never: a routine reaching `ask` is denied at once; T.4 shrinks to that and idempotency. |
| O7 | Clone and read OpenHands' agent SDK and Warp's worker, which Round 2 could not | **OpenHands: yes,** before C.6 is signed off; its container code is the nearest model for N.4. **Warp: no:** AGPL, ideas only, and OpenClaw already gives the fence. About a day each. | Both: a day more, nothing copyable from Warp. Neither: C.6 rests on the note and Round 2 alone. |
| O8 | A.1, A.2 and A.3 change the approval path and the link that the outside review is about to read | **Stack them on `integration/gate3` and add them to the review's scope before it starts.** | Land them after the review and get a second review of the delta before Gate 3. |
| O9 | The repeated-call guard: the brief put it in Phase A; the final review calls it a feature under the freeze | **Feature:** T.1, first after Gate 3. It adds refusals to the dispatcher PR 7 is rewriting. | Break the freeze for it: S, one more dispatcher change before the review. |
| O11 | **(New, 2026-11-11. Answered the same day: borrowing stays, and undo is to be redesigned around the user's own repository, see C.13. Further answers 2026-11-12, as they now stand: undo stays available after a commit by anyone and restores working files only, never HEAD, a branch, the index or the reflog; unstash of a file without a conflict is seamless; a conflicting file is skipped, written beside with an incrementing suffix (`filename.txt` to `filename-1.txt`) or merged by the agent; a stash can be deleted by the person or by the agent and expires at the first midnight after 30 days; the old versions of each turn's changed files are kept in the shadow (A.17). Later the same day: an undo after a commit applies the inverse change to the working files, like `revert --no-commit` but leaving the index alone, and creates no commit; conflicts while the agent unstashes are decided by the session's approval mode (`Auto`: the agent decides; `Ask`: the agent may skip or write beside, a merge asks; `AskAll`: the person decides); the gap left by an agent that prunes history inside its own turn is accepted with detection; the two-choice undo prompt was not answered and proceeds on the recommendation; a soft limit of 100 changed files per turn warns (T.9). Worked out in `docs/undo-and-stash.md`.)** Should checkpoints keep borrowing objects from the user's own repository? Experiment B.2: a history rewrite plus pruning in the user's repository, or about a month of git's default grace periods, or deleting the repository, makes an older checkpoint unrestorable. | **Decide after B.3 gives the cost.** A.16 makes the failure safe either way (refused, folder untouched). Stopping the borrowing makes every checkpoint self-contained, at the price of storing a copy of the folder's tracked content in the node's data directory and a slower first checkpoint; fix plan 2.6 chose "nothing stored twice" for large repositories. | Keep borrowing: an old checkpoint can become unrestorable after the user rewrites and prunes history; the user is told so instead of losing a file. |
| O10 | Short grants against the note's A3 ("an offline node finishes its running turn") | **A grant whose link drops lives until its turn ends or 30 minutes pass, whichever is first;** while linked it is renewed every few minutes. | Strict TTL: long turns on flaky links fail at expiry. |

## 4. Phases

### Phase A: fixes to what is already built (about two weeks; allowed before Gate 3)

Order follows the rewritten executive summary. "Lands on": `gate3` is a branch off
`integration/gate3` with a pull request into it (PR 6 and PR 7 rebase after each merge);
`PR 6` or `PR 7` is stacked on that branch because it rewrites the same code.

| ID | Fixes | Change | Done when | Lands on |
|---|---|---|---|---|
| A.1 **sec** | Exec 1; missed 2; A6, A7 | **A tool does not run when its event failed to save.** `Session::emit` returns the append error (`session.rs:240-244`) and `TurnHost::emit` becomes fallible (`core/src/backend.rs:19`). The rule is the simple one: after any failed append the turn ends, with no further tool body and no further model request. That covers the start event, the approval request and a result that could not be recorded, where carrying on would give the model a history the log does not hold. `01` §6.1. | Test D8 without a full disk: a store that fails appends; `bash touch marker`; no marker, and the turn ends saying the log could not be written. Second case: the append of a tool's result fails; no further model request is made. **S** | gate3 |
| A.2 **sec** | Exec 2; A1 rewritten | **Key "allow for this turn" on what was approved:** `(tool, canonical argument)`, meaning the exact command string for `bash` (no parser yet, see X.6), the resolved path for `write` and `edit`, canonical JSON otherwise. No digest (§6). `02` §6 item 2. | Approving `ls` then `rm -rf x` asks again; the same command twice in a turn asks once; the next turn asks again; a child does not inherit it. **S** | gate3 |
| A.3 **sec** | Exec 3; A2 narrowed; missed 3, 4 | **Refuse a repeated `seq` whose content differs; drop a revoked node's queued frames.** On `seq <= head`, equal content is acknowledged, different is refused as `diverged` and logged; `apply` refuses frames from a node revoked after they were queued. Epoch waits for N.5. `05` §6.1. | Test D5 and its divergent variant in `crates/master/tests/link_hardening.rs`: two links send `seq` 7 with different content, the second is refused, the mirror unchanged; revoke with 8 frames queued, none applied. **S** | gate3 |
| A.4 **sec** | Exec 4; A8; missed 1 | **A writer sub-agent gets its worktree or is refused.** When the worktree cannot be made or the folder is not a repository (`session.rs:676-686`, `:789-798`), a child with `write`, `edit` or `bash` fails the spawn with a reason the parent sees. Scout and Reviewer still run. `09` §6.1. | Non-git folder, Teamlead delegates to Software Developer: refused; forced worktree failure: refused; Scout in a non-git folder runs. **S** | gate3 |
| A.5 | Exec 5; A9; missed 6 | **Finish the loop's limits in `[limits]`:** `max_steps` (default 50, replacing the constant) and `bash_timeout_max_s` (default 3600, today's clamp, so nothing changes until a node lowers it). Out-of-range values stop the node. The guard is T.1 (O9). `01` §6.3. | `max_steps = 2` with a provider that always calls a tool ends after 2 steps naming the key; `bash_timeout_max_s = 1` cuts a `sleep 5` that asked for 3600; `max_steps = 0` refuses start. **S** | gate3 |
| A.6 | Exec 5; A5 rewritten | **Tell the model a call was cut off for length.** `Completion` carries the finish reason (read on docs.rs what genai 0.7.2 exposes per adapter). On a length stop every call in the message is refused with "output limit reached; split the change", without the fragment; three in a row end the turn. `01` §6.4. | Test D9: a `write` ends mid-JSON with `finish: length`; the error names the limit and holds no fragment; three in a row end the turn; one fixture per adapter (Anthropic, OpenAI, OpenAI Responses, Gemini) shows whether any returns a cut call as a valid object, which must also be refused. **S** | PR 7 |
| A.7 | Exec 6; A11 | **Shrink check and breaker for compaction.** If a summary does not make the history smaller, discard it (no `HistorySummarized`) and count a failure; after two, stop compacting for the turn. `07` §6.1. | A summariser that returns more than it got leaves history and log unchanged; after two, no third summary call in that turn. **S** | PR 7 |
| A.8 **sec** | Exec 7; missed 10; A13 | **Atomic `edit` and `write`:** temp file in the same directory (`O_EXCL`, `O_NOFOLLOW` kept), `fsync`, rename, `fsync` the directory, mode kept. Hard links are split; README says so. `08` §6.3. | A write failing halfway (test hook) leaves the original byte-identical and no temp file; mode `0640` kept; a symlink target still refused. **S** | gate3 |
| A.9 **sec** | Exec 8; A14; missed 9 | **Snapshot before a forced restore,** so the restore can be undone. The new checkpoint phase is an in-place schema change, recorded in `PLAN.md`. `06` §6 item 3. | Edit a file, force a restore, undo the restore: the edit is back. **S** | gate3 |
| A.10 | PR 7's verification note | **The third flaky test,** `bash::tests::cancel_during_flood_with_a_stuck_consumer`. By reading, it sleeps a fixed 300 ms then asserts the channel is full, which a loaded machine need not reach. Reproduce under load first; then wait for the full channel with a deadline. | Failure reproduced under a CPU hog and shown in the PR; 200 runs under the same load pass after. **S** (first, if it blocks a merge) | gate3 |
| A.11 | Missed 7 | **Do not offer Researcher where it has no tools.** On an agentic node, a delegate whose tool list resolves empty is left out and a call to it is refused with the reason. T.3 removes the need. | Teamlead on an agentic node does not list Researcher; in the chat runtime it does. **S** | PR 6 |
| A.12 **sec** | A3's one gap | **`node.key` unreadable from a `node run` shell.** Test only; expected to pass through 2.1 and 2.7. | `cat <data>/node.key` from bash under `node run` is permission denied (`crates/cli/tests/sandbox.rs`). **S** | gate3 |
| A.13 | A4 fix half; missed 5 | **Record the API token's reach:** a `PLAN.md` row saying it bypasses chat privacy and the `Origin` rule until T.5 (O5). | Docs only; commits T.5 to "A's token cannot read B's chat". **S** | gate3 |
| A.14 | A10 | **Fail fast on a long `Retry-After`:** above PR 7's 60 s cap, fall back or end with a message naming the wait, instead of retrying early. `09` §6.5. | A 429 with `retry-after: 300` makes no retry and falls back at once. **S** | PR 7 |
| A.15 **sec** | A14 | **(After B.5; changed by its result.)** B.5 showed that refusing is the wrong fix for nested repositories: the restore is not what loses the work, and one `git init` in a sub-folder silently disabled checkpoints for the whole folder. So: **nested repositories are excluded from checkpoints and never touched by a restore;** they can no longer make the snapshot of the rest fail; an undo never lists one as restored and says which ones it did not cover. Unchanged: **a restore is refused while another session has a turn running in the same folder.** | B.5's twelve tests assert the fixed behaviour; a nested repository with no commit no longer stops the checkpoint; hostile config in a nested repository never runs; restore while a second session in the folder runs a turn is refused. **M** | gate3 |
| A.16 **sec** | B.2 (new, 2026-11-11) | **A restore is all or nothing, and a damaged checkpoint store repairs itself.** B.2 showed that when an object a checkpoint borrowed from the user's repository is gone (history rewritten and pruned), a restore deletes the user's file and leaves the folder half-restored, and from then on every turn runs with no checkpoint. So: before a restore writes anything it checks that every object it needs can be read, and refuses with the reason if not; a shadow index that refers to a missing object is rebuilt so later turns are checkpointed again. Whether checkpoints keep borrowing at all is O11. | B.2's tests assert the fixed behaviour: with a missing blob the restore is refused and the folder is byte-identical; the next turn has a checkpoint. **M** | gate3, after A.15 (same files) |
| A.17 **sec** | B.2; O11's answer (undo stays available after commits and history changes) | **Copy the old versions of the files a turn changed into the shadow's own store at the end of the turn,** with the trees a diff of the turn reads, as a pack with a `.keep` file that names the turn and is released when its checkpoint is pruned. Content already gone when the turn ends (the agent pruned it inside its turn) is named in the chat. No folder scan, no gc. | Undo works after the user rewrites history and prunes, and after the shadow's own gc; the in-turn prune is named in the chat and the turn ends normally; only changed files are copied and the store stays bounded over 60 turns; nothing is written into the user's repository and none of its config runs. **M** | `main`, after A.18 |
| A.18 | B.3 (gc was the largest cost of every turn); 0-byte `harness-index-*.lock` files left by a timed-out snapshot | **Run garbage collection rarely and clean up lock files after a timeout.** Refs are still dropped every turn; `git gc` runs after 16 dropped checkpoints (or git's auto-gc thresholds), at most once in 5 minutes, after the turn has ended, holding the shadow alone (snapshots, copies and restores wait for a running one). A timed-out snapshot removes its index lock; stale locks go at start-up. | 30 turns with two kept run one gc instead of 28; the store stays bounded; every kept checkpoint restores after a gc; a timed-out snapshot leaves no lock; a stale lock goes at start-up and a young one stays. **S** | `main` |
| A.17 **sec** | B.2, B.3; C.13 (new, 2026-11-12) | **Undo does not depend on the user's repository keeping old content.** At the end of each turn, copy the old versions of the files the turn changed (and the trees on their paths) into the shadow repository's own store as a kept pack, and keep them with the turn's checkpoint; `prune` deletes the pack with the checkpoint; no gc runs for it. Objects already gone (an agent that pruned inside its own turn) are named in the chat as files that turn can no longer undo. Same for a pre-restore checkpoint. `docs/undo-and-stash.md` §2.4. | After a turn, history is rewritten and pruned in the user's repository and the undo of that turn still restores every file; the copy holds only the files the turn changed; nothing is written into the user's repository. **S to M** | `main` |
| A.19 **sec** | C.14 (owner, 2026-11-12); the version check runs after `Accepted` (`crates/master/src/lib.rs:894-897`, `nodes.rs:234-268`), so a refused node reconnects forever without the reason; SIGTERM kills `serve` and `node run` (`crates/cli/src/main.rs:432`, `:670`) | **Drain on a stop signal, and tell a node why it was refused.** The protocol check moves before `Accepted` and answers `Rejected` with the reason; SIGTERM is handled like SIGINT; a stop refuses new work with a reason, cancels running turns through the normal cancel path, closes them in the log naming the stop, and exits; a second signal skips the wait. The README unit sets `TimeoutStopSec` and drops `KillSignal=SIGINT`. The agent-asked drain and the update itself stay in N.9. | Tests: a protocol mismatch is refused before `Accepted` with the reason, and the node exits instead of reconnecting; a SIGTERM closes a running turn with its process tree gone and the stop named in the log; a second signal forces. **M** | `main` |

**Gate A:** A.1 to A.14 merged with failing-first tests; CI green; A.1 to A.3 in the outside
review's scope (O8). The freeze is still on.

### Phase B: experiments (three to four days; tests only, allowed now)

Each runs with `cargo test` on Linux with git, bash and Landlock, and has a control that must
fail (the same thing unconfined, say), so a pass means something. A test that shows a failure
is merged `#[ignore]` with the observed result and the decision it feeds.

| ID | Experiment | Outcome that changes a decision | Size |
|---|---|---|---|
| B.1 | Tests D1 and D2: `setsid sleep 1000`, then cancel; `kill -9` the node during a long child, restart (`bash.rs`, `acceptance_restart.rs`) | Predicted: both survive our `killpg`. Then C.11 picks a containment and T.8 builds it; otherwise C.11 closes with the evidence. | **S** |
| B.2 | Test D3: snapshot, rewrite the user's history, `git gc --prune=now`, restore | Lost objects: snapshots stop relying on alternates (copy blobs) and fix plan 2.6's residual is rewritten. | **S** |
| B.3 | Test D4: checkpoint time over 1k to 500k files, warm and cold, 0 to 1,000 changed (`#[ignore]` benchmark) | Sets or rules out a checkpoint budget key (C.6) and decides whether a read-only turn skips its snapshot. | **S** |
| B.4 | Test D7: `kill -9` the node during `bash`, restart | Arguments in the log (expected); a closer only at history rebuild (expected). If fork or UI needs it as an event, add an "outcome unknown" closer as an A follow-up. | **S** |
| B.5 | Test D20: `git add -A` over a nested `.git`, with and without a commit; whole-tree restore | Sets A.15's refusal and whether checkpoints exclude nested repositories. | **S** |
| B.6 | Test D13: rewind, restart the node, open the session | A disagreement puts an explicit head into C.2 and N.5; otherwise it is dropped. | **S** |
| B.7 | Test D17: stream at the link cap while a cancel and an approval answer arrive | Either waiting past grace means a control lane in the pump, as a fix before Gate 3. | **S** |
| B.8 | Test D19: compact a session whose results hold a known key and an unknown secret-like string | Known key in the summary: a bug in A.7's path. The unknown one: a residual in the threat model. | **S** |
| B.9 | Not settled: where instruction files enter the prompt and whether they survive compaction | Lost after compaction: re-injection joins X.2. | **S** |
| B.10 | Missed 8: fsyncs and bytes per turn on node and mirror for a long streamed answer | Numbers for fix plan 4.8's ADR. This plan changes nothing about deltas. | **S** |

Dropped as experiments (reasons in §9): D5, D8, D9 (they are A.3's, A.1's and A.6's failing
tests), D10, D11, D18 (covered on PR 4), D12 (rerun in X.3), D16 (settled by reading), D6,
D14, D15 (moved to N.5 and N.4: no archive exists yet, and Podman is not available here).

**Results so far (2026-11-11).** B.1: confirmed. A process started with `setsid` survives cancel, timeout and a dropped call, with and without the sandbox, and a running command survives a SIGKILL of its parent; C.11 is needed. B.2: worse than predicted. A restore deletes the user's file when a borrowed object is gone, and checkpoints then stop for good; this became task A.16 and decision O11. B.5: one `git init` in a sub-folder disabled checkpoints for the whole folder, and an undo listed nested repositories as restored when nothing was; A.15 was rewritten from it.
B.3 (`docs/checkpoint-cost.md` on `exp/B.3-checkpoint-cost`): the `git gc` that ran on every turn once ten checkpoints existed was the largest cost of a turn at every folder size measured (0.26 s at 1,000 files, 3.3 s at 100,000 in a plain folder, on a slow disk); it became A.18.

**Gate B:** every B test merged, passing or `#[ignore]` with its result, and each outcome
written into the C note or task it feeds.

### Phase C: design decisions on paper (about a week plus owner time; allowed now)

Each task changes `docs/nodes-credentials-environments.md` or adds a short note in `docs/`.
**Done when, for every row: the note is merged with the owner's sign-off and names the tests
it commits the building task to.** Every row is **S** except C.12 and C.13, which are **M**; C.12 comes
before C.3, C.5 and C.6 because it changes what they are written against.

| ID | Decision | What the note settles | Tests it commits to |
|---|---|---|---|
| C.1 | Fence for the link and for fork | A **generation per connection** (`nodes.link_gen`, bumped on accept), checked by the reader pump per frame. An **epoch per session** (`sessions.owner_epoch`, a counter that never goes back), bumped on archive, delete, revocation, environment deletion and fork-source close, not on a reconnect; checked in one predicate in the mirror transaction with owner, active state and `seq = head + 1`. On `fenced` the node stops the session and kills the tool. Three flaps revoke and bump. Replaces §6's "second connection replaces the first". Our own bump list, not OpenClaw's. `05` §6.1. | Superseded connection's frame dropped; append after archive refused; node kills the tool on `fenced`; reconnect does not bump. |
| C.2 | Fork by copy with lineage and an explicit head | First event `SessionForked { parent, root, parent_node_id, through_seq, cut, lossy }`, then inherited `history_imported` batches, then a seed-ended marker. `through_seq` is the highest contiguous mirrored `seq`; default cut the last completed turn; an open turn is closed. The child starts at `ask`, no grants, no allowance, zero counters. `session_forks` gains the hash at `through_seq`. Same-node forks may seed files from the cut's checkpoint. Explicit head only if B.6 says so. `05` §6.2; `06` §6 items 1, 2, 8. | Lineage first; no unmirrored tail; non-owner gets 404; the child asks for what the parent had allowed; no fork during a running turn. |
| C.3 | Grant lifetime and binding | TTL in minutes, renewed while the turn lives; O10's link-loss rule. Bound to `(node_id, session_id, owner_epoch, turn req_id)`, checked on every `session.prompt`, scope checked after alias expansion; a newer grant cancels the old one's in-flight calls. Proxy bounds written now for N.8. `04` §6 item 2; `05` §6.5. | Grant for X refused for Y; expired grant refused at prompt; old in-flight call cancelled; grant absent from `env` and `/proc/*/environ` (test D10, grant case). |
| C.4 | What "always allow" may mean, where a rule lives | Three levels: once; this exact call for this turn (A.2); a persistent rule, never a click, in a node-side file beside `node.toml` written by the node's owner (the UI may propose its text). One project root on one node; never a prefix for wrappers or interpreters (`sudo`, `env`, `bash`, `python*`, `xargs`, `find`, `git -c`); never lifts the sandbox; never covers network, escalation or MCP; no repository file adds or loosens one; the file is always-ask for the agent. `02` §6 items 2, 5. | `git status` never allows `git push`; a repository rules file changes nothing; the agent cannot write the rules file; no rule runs outside Landlock. |
| C.5 | Credentials, pairing, aliases (note amendments) | §4.1: AES-GCM associated data binds `workspace_id` and `name` (Warp's idea, not its code). §4.2: strict fallback as `Option<Vec<_>>`; per-slot timeout and effort; slot names as validated strings; a slot failure never fails the turn; a slot on another provider needs its own grant route. §4.4: burn the code before any check; the master's code endpoint refuses foreign `Origin`, gives one failure reason, limits by peer. §9: "fallback" becomes the explicit `--systemd-creds` flag. Pairing is written per attachment (C.12). `05` §6.6; `09` §6.4. | Sealed row copied across workspaces fails to open; empty strict list fails with a reason; rejected attempt burns the code; failed title slot leaves the turn running. |
| C.6 | Containers, forge, deletion (note amendments) | §4.5: A.5's keys; a checkpoint budget if B.3 asks; caps a session may lower, never raise; never `extra_args`. §4.6: `--network=none` default until test D14 shows what rootless `pasta` exposes; `--ulimit core=0`; sized `nosuid,nodev,noexec` tmpfs on `/run`; argv-hash label, recreate on mismatch; 64-hex id check; mount source validated against the data directory, `$XDG_RUNTIME_DIR` and `~/.ssh`; `podman inspect` assertions; stop on actor loss; identical checks inside. Tests D14, D15 are N.4's first step. §4.7: a forge token is readable in container sessions. §4.8: deletion stops routines and bumps epochs. OpenHands read first if O7. `03` §6.1, §6.4, §6.5. | Off-list image, raised cap, unknown flag refused by the node; inspect shows label, `core=0`, no network; cancel kills inside within 2 s; a routine on a deleted environment stops. |
| C.7 | Unattended authorisation (O6) | A per-routine allow set made by a person, content-hashed, recorded as an event, evaluated on the node (turns `ask` into a run, never a `node.toml` deny); outside it, denial with the reason. Schedule advances before dispatch; an idempotency record fsynced before the first model call; an unknown outcome pauses. Agent-made routines start `pending_approval`. Webhook keys in the database. Never auto-approve. `10` §6.1 step 1; `01` §6.7. | `10` §6.3 items 1 to 3; an allow set cannot lift a `node.toml` deny. |
| C.8 | The `.git` residual | Landlock only adds rights and a right covers the tree, so `.git` cannot be read-only in a read-write folder. Host sessions: a named residual in `PLAN.md` with its real impact (our git ignores hooks and config; a planted hook fires when the user runs git). Containers: evaluate read-only binds over `repo.git/hooks` and `repo.git/config` in N.4. `03` §6.3 item 1. | If chosen, a container session cannot write `repo.git/hooks/post-checkout`. |
| C.9 | Staleness guard and match ladder for `edit` | Per-session table by canonical path: SHA-256, `dev:ino:size:mtime_ns:ctime_ns`, ranges shown, a bounded snapshot on disk; checked in the per-path lock before matching; `edit` needs a read, `write` over a file needs a full read; a stale file is edited only when the matched span is byte-identical, else refused with a bounded diff; hashes in `read` events for the fold; batches match the original; the ladder stops at a one-to-one Unicode fold and reports the tier; what compaction does to "ranges shown". `08` §6.1, §6.2. | T.2's list. |
| C.10 | seccomp (O4) and a read-only shell | `03` §6.2's rules: with network off, `socket` only for `AF_UNIX`, deny `connect`, `bind`, `listen`, `accept`, `sendto`, `sendmmsg`, `recvmmsg`; always deny `io_uring`, `AF_VSOCK`, `ptrace`, `process_vm_*`; `EPERM`, never `SIGSYS`; unknown architecture refused in `required`; `sendmsg` decided. Status and tool result show the active rules. The crate's dependency check. T.7's Landlock profile. | T.6's and T.7's lists. |
| C.11 | Process-tree containment (only if B.1 leaks) | A per-session cgroup (needs delegation, such as a systemd user scope), a PID namespace in `sandbox-exec` (must stay free of `unsafe`; `nix`'s `fork` is not), or a `/proc` re-walk (racy). `01` §6.9; `03` §6.3 item 4. | A `setsid` child is gone 2 s after cancel; none survives `kill -9` of the node. |
| C.12 | A node in several workspaces (O3) | Signed off by the owner on 2026-11-12. Settled in the note: attachment, the node's attached list and pairing per attachment (§4.1, §4.4, §4.10.1); the two trees on the node, derived session folders, projects as worktrees, linked folders and safe removal (§4.11); no conversation at rest, the spool and hydration (§4.12); what one session reaches of another and the shared-node rule (§4.10.2, §4.10.3); grants keyed by workspace (§4.3); the share (§4.10.4); detach and revoke (§4.10.5); `local` shareable (§4.10.6); the draft threat row and the `PLAN.md` sentences that change (§5, §4.12.5); costs (§8). Decisions recorded in the note's §9. | The test table in the note's §8 (N.1, N.1b, N.3, N.4, N.7). |
| C.13 | Undo and checkpoints, second design (owner's direction, 2026-11-11, answering O11). **Note: `docs/undo-and-stash.md`** (2026-11-12, proposal, revised the same day after the owner's answers: undo stays available after a commit by anyone and restores working files only; non-conflicting unstash is seamless; a conflict is skipped, written beside with a suffix, or merged by the agent; a stash can be deleted by the person or the agent and expires at the first midnight after 30 days): §1 what exists, what is new and what changed, §2 undo and commits and A.17, §3 the stash, its expiry, unstash and the agent's tools, §4 code changes, cost (B.3) and residuals, §5 abuse cases, §6 tests, §7 build order, §8 questions and answers (later the same day: undo after a commit applies the inverse change to the working files and creates no commit; Q2 decided by approval mode; Q3 accepted with detection; Q1 not answered; a soft limit of 100 changed files per turn, §2.6, task T.9) | The owner wants undo rethought around the user's own repository instead of a full shadow history. In the owner's words, restated: (1) the shadow repository saves only what is **not committed**; when a new commit appears in the user's repository the shadow drops what that commit now holds, and undo for those files goes "by the actual committed files"; (2) after each agent turn the shadow records the present state as a shadow commit, uncommitted files and folders without a repository included; (3) the user's own edits are tracked too; (4) undoing an agent turn when the user has since changed files **asks the user** whether to stash those changes, and the agent can view a stash. Borrowing objects from the user's repository stays. The note settles, with the owner: what undo does when the turn's changes were committed (recommended: never move HEAD or the index; say which commit holds them and leave that to git); what a stash is (recommended: the pre-restore checkpoint A.9 already takes, given a name, a list, and a read-only tool for the agent); whether stashed edits are put back automatically when they do not overlap the undone changes; how long a stash is kept (it holds the user's work, so not under `checkpoints_keep`); what happens when the base commit is gone (B.2); and how much of A.9, A.15 and A.16 stays as it is. Experiment B.3's numbers go into it. | The note names them. At least: undo after the user committed the turn's changes moves no ref and loses nothing; a stash survives pruning; the agent can read a stash and cannot write one; declining the stash prompt changes nothing. |
| C.14 | Node update and drain (owner's direction and answers, 2026-11-12). **Note: `docs/node-update.md`** (revised 2026-11-12 after the answers, recorded in its §2): §1 what exists and why the removed SSH upload is not back, §3 `update_mode` (`off`, `notify` by default, `restart`, `auto`) and `update_trust`, §4 build identity, the master's own build and the cache for other targets, §5 the endpoint on the node link, §6 verification, install, rollback and the trust given up, §7 the drain, §8 protocol, events, `local`, supervisors and the draft threat row T35, §9 tests, §10 build order, §12 two questions | The node pulls a compatible build (the master's own) from its master over the authenticated link, only when its own `node.toml` says `auto` with an explicit `update_trust`; the master never pushes. With `update_trust = "master"` a compromised master runs code on those nodes, stated as the price; `"signature"` with a pinned key is the end state once releases are signed. Staged and preflighted while serving, renamed in after the drain, rolled back on a failed handshake. The drain as decided, started also by a stop signal. Before the first tag: same target and admin-placed builds only. | The note's §9: a node not in `auto` never requests a build; the master sends nothing unasked; the endpoint refuses without node authentication, a revoked node and another target; wrong hash, target, identity or an older build refused with nothing installed; unsigned builds refused under `signature`; `auto` without `update_trust` refuses to start; a failed download causes no drain; a failed first handshake rolls back; a SIGTERM drains and a second forces; plus the drain tests (pause and continue, the five-minute stop, the forced cancel, restores, events, routines, abuse). |

**Gate C:** C.1 to C.10, C.12, C.13 and C.14 signed off (C.11 if B.1 failed); the design note lists no
blocking question and no passage marked superseded; every T, N and X task can name its abuse tests from a merged note.

### Phase T: loop and tool features (after Gate 3; three to four weeks)

Independent of the node redesign and in other crates, so it runs beside Phase N. One person
should do T.1 to T.4 before N.1: they are small and fix gaps users meet today.

| ID | Adds | Safeguards and abuse tests, written first | Invariants | Podman / Node | Depends | Size |
|---|---|---|---|---|---|---|
| T.1 | Repeated-call guard | Hash of tool, arguments and outcome; warn at the second repeat, block at the fifth (Hermes Agent's thresholds, as ideas); hard stop in routines, warning in chats; decided before dispatch; a guard error blocks; state from the fold. Tests: the fifth identical failing call blocked; a restart keeps the count; a guard error blocks; different arguments never trip it. `01` §6.2. | Node decides (`[limits]`) | No / No | A.5 | **M** |
| T.2 **sec** | `edit` staleness guard (C.9) | `edit` without a read refused; stale with a changed span refused with a diff; identical span applied; `write` over an unread file refused; restart rebuilds; batch against the original; tier reported. | One writer; light process | No / No | A.8, C.9, PR 7 | **M** |
| T.3 **sec** | Web and memory tools on agentic nodes | Off unless `node.toml` enables them. Memory approval in the write path, failing closed; staged writes carry a base version; shared memory outside the shell's writable set; threads with external content not auto-extracted; URL provenance and the public-address resolver kept. Tests: a URL only in an error is refused on the node; a private address refused; an unapproved memory write refused; a node with web off refuses by name. Undoes A.11. `07` §6.2. | Master runs no tools: the stress test settles where a node's memory write is stored | No / No | A.11 | **L** |
| T.4 **sec** | Unattended authorisation (C.7) | C.7's tests. | Node decides | No / Yes | C.7, A.1, A.5 | **L** |
| T.5 **sec** | Per-user API tokens (O5) | Hashed, scoped, revocable; the single token removed. A's token cannot read B's chat; a revoked token is 401 at once; no token in a log line. | Private chats, restored | No / Yes | fix plan 4.1 | **M** |
| T.6 **sec** | seccomp (C.10, if O4) | With network off, UDP `sendto` and a DNS lookup fail `EPERM`; `io_uring_setup` fails; a cargo build in the cwd works; unknown architecture refused under `required`. | No `unsafe` in our crates | No / No | C.10, outside review | **M** |
| T.7 **sec** | Read-only shell for read-only agents (C.10) | A Landlock profile with only a per-call temp directory writable, chosen by the node for agents without a write tool; gives Scout and Reviewer `bash` for `grep` and `git log`. `touch x` fails; `git log` works; a spec asking a writable shell for Scout is refused by the node. | Node decides | No / No | C.10 | **M** |
| T.8 **sec** | Process containment (C.11, if B.1 leaked) | C.11's tests in CI. | Light process | No / No | B.1, C.11 | **M** |
| T.9 | Soft limit of 100 changed files per turn: warn at the end of the turn and on its undo; never block | One named constant (100); the count is `changed(before, after)` (nested repositories and ignored files not counted; a sub-agent's changes count toward the turn that applied them); the warning is the existing chat note and the undo plan's count, no new event. Tests: 100 files warn nobody; 101 warn at the end of the turn and on its undo, and both complete normally; nested repositories are not counted. `docs/undo-and-stash.md` §2.6. | One writer | No / No | C.13 | **S** |

**Gate T:** each task's abuse tests in CI and the threat model updated with it.

### Phase N: workspace nodes, keys, grants, environments (after Gate 3; months)

The design note's build order (§8), in its sequence, with the research folded in. "Note tests"
are that phase's tests in the note. Each task carries its own UI as lazy chunks, so each needs
W.1.

| ID | Note phase | Adds beyond the note | Abuse tests beyond the note's | Invariants | Podman / Node | Depends | Size |
|---|---|---|---|---|---|---|---|
| N.1 **sec** | 1 workspace nodes, pair codes | Link generation (C.1); pairing per C.5; attachment to several workspaces, the node's own attached list and the per-workspace data root (C.12) | Superseded connection's frame dropped; rejected attempt burns the code; a session for an unattached workspace refused by the node; A cannot read B's folder | One writer; adding a node only narrows; the node decides which workspaces it serves | No / Yes | C.1, C.5, C.12 | **L** |
| N.2 **sec** | 2 BYOK, aliases, slots | AAD; strict lists; slot rules (C.5) | Row copied across workspaces fails to open; empty strict list fails | No secret in event, URL or log | No / Yes | N.1 | **L** |
| N.3 **sec** | 3 grants | C.3 in full | C.3's tests, test D10's grant case | Node decides (`model_keys`) | No / small | N.2, C.3 | **M** |
| N.4 **sec** | 4 environments | Tests D14, D15 first; C.6's argv and checks; C.8's binds if chosen | C.6's tests; identical behaviour inside and out | Node decides; loopback; RSS with 8 containers | **Yes** / Yes | N.2, C.6, C.8 | **L** |
| N.5 **sec** | 5 archive, fork | Session epoch (C.1); fork by copy (C.2) | Test D6 rewritten: archive while offline, reconnect, appends refused; C.2's tests | One writer, now checked | No / Yes | N.1, C.1, C.2, B.6 | **L** |
| N.6 **sec** | 6 Connections | Device flow; the OAuth callback rule (`10` §6.3 item 10) | Note tests | No secret in a URL | No / Yes | N.1 | **L** |
| N.7 **sec** | 7 classification, deletion, autosave | Routines stopped, epochs bumped (C.6, C.1) | A routine on a deleted environment does not run; a late append is refused | Private chats: T10 change needs the owner's explicit yes | **Yes** / Yes | N.4, N.5, N.6, T.4 | **L** |
| N.8 **sec** | 8 proxy, per-user node (experimental) | C.3's proxy bounds first | Note tests | Node decides | No / Yes | N.3 | **L** |
| N.9 **sec** | `docs/node-update.md` §10 | Steps 2 to 8 after Gate 3, on A.19: build identity; the full drain (note, `drain_answer`, events, Continue, routines); the requirement and `off`, `notify`, `restart`; the build endpoint on the link and the master's own build; the node side of `auto` (fetch, verify, preflight, install after the drain, rollback, `update_trust`); the cache (`node-build add`; the release fetch after the first tag); `signature` trust after signed releases | The note's §9; with N.1 the spool flush and a notice per attached workspace, with N.4 containers stopped and reconciled | Node decides (`update_mode`, `update_trust`; the trust given up in `auto` is the owner's yes of 2026-11-12, written into `PLAN.md` T5); the node pulls, the master never pushes; one writer | No / Yes | A.19, C.14 | **L** (four to five weeks in all) |

N.3 and N.4 can run in parallel after N.2, and N.6 after N.1, as the note says. C.12 also
binds N.3 (grants keyed by workspace), N.4 (environments, volumes and the per-workspace share
of containers) and N.7 (detaching one workspace is a deletion scoped to it); each carries
C.12's tests for its part.

**Gate N:** per task, the note's tests and the abuse tests above in CI; N.4 and N.7 on a
runner with rootless Podman (confirm one exists before N.4 starts).

### Phase X: extensibility and interaction (after Gate 3 and the T and N tasks named)

Order from `10` §6.1. Plugins, code mode and import are not planned (§6).

| ID | Adds | Safeguards and abuse tests | Invariants | Podman / Node | Depends | Size |
|---|---|---|---|---|---|---|
| X.1 | Context-change events ((c) step 6) | A prompt-section delta event and a tool-loadout event, folded by node and UI; sorted names; appended, never edited into the prompt head. `10` §6.3 item 6: after restart, fork and compaction the loaded set and request prefix are identical when nothing changed. | One writer | No / small | A.7, PR 7 | **M** |
| X.2 **sec** | Instruction files and skills, text only | Node-controlled roots; repository skills only after a per-project trust decision bound to content; capped catalog; bodies through the approval path; nothing runs at load; instructions survive compaction (B.9). `10` §6.3 item 5. | Node decides | No / Yes | X.1, B.9 | **M** |
| X.3 **sec** | MCP client with deferral | Only `node.toml` adds a stdio server; PR 4's environment allow-list; inside the session's sandbox or container; approval per server and tool, bound to the executable's digest, keyed as in A.2; `readOnlyHint` only tightens; size and time caps; deferral by measured list size, activation as a loadout event; OAuth tokens per turn. `10` §6.3 items 4, 7, 10; test D12 rerun against every socket. | Node decides; no secret in a URL | Partly / Yes | A.2, X.1, N.4 | **L** |
| X.4 **sec** | Hooks | Node only; sandboxed, empty environment; trust bound to every referenced file's bytes, stored where only the node owner writes; fail closed on crash, timeout, bad output or matcher; may deny, never approve. `10` §6.3 item 8. | Node decides | No / No | X.3 | **M** |
| X.5 | Input during a turn | Persisted with an idempotency key before acknowledgement; delivered at a step boundary; never kills a tool; a dropped message gets a receipt. Tests: sent twice, stored once; restart before delivery, delivered once; a running `bash` is not interrupted. `01` §6.10. | One writer | No / Yes | A.1 | **M** |
| X.6 **sec** | Per-command rules | A maintained parser (`tree-sitter-bash` or `brush-parser`, after the check); allow rules cover every segment; deny rules hit any segment after unwrapping; C.4's limits. C.4's tests; `ls; rm -rf x` under an `ls` rule asks; unparseable asks, and is denied unattended. `02` §6 item 5. | Node decides | No / Yes | C.4, A.2 | **L** |
| X.7 **sec** | Enforced plan mode | Registry, dispatcher and T.7's shell; only the user leaves. A named `write` is refused, `bash` cannot write, delegating to a writer is refused. `02` §6 item 9; OpenCode leaves the shell open. | Node decides | No / Yes | T.7 | **M** |

When X.2's work on instruction files and prompts starts, the instructions of every agent that can change files gain the 100-file rule of `docs/undo-and-stash.md` §2.6 (try to keep a turn's changes to at most 100 files; split larger work across turns; exceeding it is allowed).

**Gate X:** each task's abuse tests in CI.

### Phase W: the user interface (blocked on Node; W.1 and W.2 now)

Fix plan Gate 3 includes 3.10, so the freeze cannot end until W.2 lands or L14 is accepted in
the threat model. W.1 and W.2 are on the critical path.

| ID | Covers | Change | Done when, and size |
|---|---|---|---|
| W.1 | Web checks not runnable here | Node 22 (CI's version) and pnpm 12.17.0 through `corepack`, plus Playwright's browsers; steps in the README's development section. Owner action. Fallback: a manually triggered CI job running one named e2e script against the built binary. | The four `pnpm` checks pass locally on `integration/gate3`, and `node web/e2e/onboard.cjs target/debug/rizzy-harness` runs. **S** |
| W.2 **sec** | Fix plan 3.10 (L14) | Drafts cleared on logout and chat delete, never stored for incognito; `#` links only to footnote ids; temporary passwords generated by the server and shown once (Rust endpoint and test first). | 3.10's tests; `pnpm size` passes. **S** |
| W.3 | PR 4 follow-ups | The "for this turn" button, labelled as A.2 makes it ("this exact command, for this turn"), offered after the first approval; no reconnect after close code 4401; request id on errors; messages for the new 403, 422, 429 and 503. | Component tests each; the approval e2e flow passes. **M** |
| W.4 | PR 6 follow-ups | Picker grouped by category; aliases in mentions; the "User facts" label; a default picker per mode; the new error texts. | Component tests each. **M** |
| W.5 | PR 7 follow-up (only if O2 is yes) | "Retrying in N s" from the new event. | A fixture-event test. **S** |

The design note's UI (Nodes page and pair dialog, per-workspace providers and aliases,
onboarding, key list on removal, environments, archived view and fork, Connections, the
deletion dialog) travels with its N task.

**Gate W:** W.1 and W.2 merged; fix plan Gate 3 can be called.

### Phase R: findings of the Gate 3 security review (now; before the gate can be called)

The review Gate 3 asks for was done on 2026-11-11 by an independent Fable reviewer
(`docs/security-review/2026-11-gate3.md`, with proof tests in `docs/security-review/proofs/`).
Its verdict: **not fit to pass**, with 1 Critical, 3 High, 4 Medium, 14 Low and 6 Note. These
are fixes to what is built, so they are allowed now. Each task inverts the review's proof
tests, and the fixes are read again by an independent reviewer before the gate is called.

| ID | Findings | The change | Done when |
|---|---|---|---|
| R.1 **sec** | S-1 (Critical), S-2 (High), S-3 (High), S-4, S-6 | **Nothing the agent's shell writes may steer the node's own git or move a session's folder.** Sub-agent snapshots and the patch are taken in a harness-owned git directory with the hardened settings, never under the user's repository config; the child's grant on the main `.git` is narrowed; a session's folder is pinned by canonical path, device and inode and refused when it changed; the checkpoint root is found by walking up for `.git`, never taken from git configuration, and goes through the same checks as a session folder; a restore refuses protected folders. | The review's proofs for S-1, S-2 and S-3 fail (the attack no longer works), on the real sandbox where they need it. |
| R.2 **sec** | P-1 (High), P-2, P-3, P-7 | **A login is bound to its origin, not only to its host.** A random value kept per origin by the page is required on every cookie-authenticated request and as the first WebSocket message, so another port on the same host cannot use the cookie. `delegate` goes through the approval path and `denied_tools`; the revocation sweep runs again on reconnect; a deny or cancel is never refused. | A request with the cookie but without the origin value is refused; the UI works; `denied_tools = ["delegate"]` is honoured. |
| R.3 **sec** | L-1 to L-4 (Medium), L-5, L-6, L-8, L-9 | **A node's word does not move or widen anything on the master.** `session_created` takes effect only at `seq` 1 and never changes an owner; a parent is followed only under the same node and owner; node-supplied strings and event sizes are capped at append; revocation is re-read after the stop token is registered and `send_command` refuses a revoked node; `node.hello` is bounded. | The review's proofs for L-1, L-2 and L-3 fail; a node revoked during its handshake is cut. |
| R.4 | P-4, P-5, P-6, L-7, S-5, the Notes | The remaining Low and Note findings: fixed where small, otherwise recorded in the threat table with the reason. | Each has a test or a threat-table row. **Done (branch `fix/R.4-remaining-findings`):** fixed, each with a test that failed first: R1d-1, R2-1, R2-2, R2-3, L-7, S-5, P-6, N-3, N-4, N-5 (and R3-1, closed by the P-6 claim path), plus the routines retry race behind a flaky CI run; recorded in `PLAN.md` with the reason: P-4 (T11), P-5 (T20), N-1 tree-sitter (T23), N-2 (T4, README), N-6 (T22, a condition on T.3), R3-2 (T10), W-4 and the card notes (T4), the notes of fix reviews 2 to 5 (T2, T4, T5, T32), the residuals of pull requests 35, 36, 38 and 40 (T2, T4, T7, T32); S-6's gap was already in T33. |
| R.5 | The whole of Phase R | **The fixes are read again** by an independent reviewer, as the review asks, together with the part it did not check: how the web UI renders node-controlled and model-controlled strings. | A second report with no finding above Low. The reviewer's closing verdict on all three areas is in `docs/security-review/2026-11-gate3-fixes-5.md` (section 6). |

**Gate R:** R.1 to R.5 done. Only then can fix plan Gate 3 be called.

## 5. Order and dependencies

```
now:     A.10 ─► A.1 ─► A.2 ─► A.3 ─► A.4 ─► A.5 ─► A.8 ─► A.9 ─► A.12, A.13
         on PR 7: A.6, A.7, A.14     on PR 6: A.11     B.5 ─► A.15
         B.1..B.10 and C.1..C.10 beside A;  B.1 ─► C.11 (if it leaks)
         C.12 ─► C.3, C.5, C.6   (a node serves several workspaces)
         W.1 ─► W.2   (Gate 3 needs 3.10)
Gate 3 ─┬─► T.1 (A.5), T.2 (A.8, C.9), T.3 (A.11), T.4 (C.7)
        ├─► fix plan 4.1 ─► T.5
        ├─► outside review + C.10 ─► T.6, T.7 ─► X.7
        └─► N.1 ─► N.2 ─┬─► N.3 ─► N.8
                        └─► N.4 ─────────┐
            N.1 ─► N.5 ──────────────────┤
            N.1 ─► N.6 ──────────────────┴─► N.7 (also T.4)
        X.1 ─► X.2 ;  X.1 + A.2 + N.4 ─► X.3 ─► X.4 ;  A.1 ─► X.5 ;  C.4 + A.2 ─► X.6
W.1 is needed by every N task and by T.4, T.5, X.2, X.5, X.6.
```

Where this differs from the brief's sketch:

- The repeated-call guard moved to T.1: the final review calls it a feature under the freeze (O9).
- W.1 and W.2 run now: fix plan Gate 3 cannot pass without 3.10.
- Phase T runs before or beside N: small, independent work should not wait months behind N.
- T.3 and T.4 come before workspace nodes: routines are blocked today and Researcher has no tools on nodes.
- The `edit` guard (T.2) moved up: it is independent and `edit` is in every session.

## 6. What this plan deliberately does not do

From part (e), each with the project that shows why:

| Not done | Shown by |
|---|---|
| A model-set flag that lowers the gate | Warp, `is_risky == Some(false)` |
| An allow rule that lifts the sandbox | Codex, `Skip { bypass_sandbox: true }` |
| Dropping every rule on a parse error | Codex, `exec_policy.rs` |
| Hooks that fail open | Codex, `PreToolUse` |
| Unattended equals auto-approve | goose scheduled jobs in `Auto`; Cline cron "yolo" |
| Pass-through container arguments | Hermes Agent, `docker_extra_args` |
| Relaxed checks inside a container | Hermes Agent skips its floor and prompts |
| A control-plane key in the agent's environment | OpenHands' session key |
| An empty approver list meaning "anyone in the chat" | OpenClaw |
| Repository files that loosen user settings | Oh My Pi's unfiltered project layer |
| Children forced to the most permissive mode | Oh My Pi's `yolo` overlay |
| Code mode | goose's `lease.call` skips the gate |
| Low-threshold similarity matching; NFKC rewriting whole lines | Cline 0.66, Zed 0.8; Pi |
| Checkpoints as uncapped refs in the user's repository | Cline |
| A durable log off by default | OpenCode, `persist` false |
| Latest connection wins as the only fence | OpenCodex Remote Workspace |
| Full environment inheritance for MCP children | Oh My Pi, Pi, Cline, Warp |
| A guard shipped disabled; a read-only role that is only a name | DeepSeek harness; Codex `explorer.toml`; OpenCode `explore` |

Also not done, by choice:

- **No action digest on approvals** (A1): the id is already node-minted, single-use and owner-only; a digest guards only against a lying master, which can approve anything anyway (fix plan §5, `PLAN.md` R1).
- **No split of deltas out of the log** (A15): fix plan 4.8 measures and decides by ADR; B.10 feeds it.
- **No proportional result cap** (A9): a 30,000-character cap already covers every result.
- **No `PR_SET_DUMPABLE`, `rustix` or `LD_*` blocking** (A3): Landlock already denies ptrace-class access (tested on PR 4), and the agent can set those variables in its own command.
- **No retry tuning beyond A.14** (A10: no-network schedule, shared per-turn counter, empty-response class) until real providers give evidence.
- **No cache-preserving summary request or model-free fallback tiers** (A11): a cost choice tied to O1.
- **No 128-bit pairing fingerprint now** (§4.4): matching 64 bits needs about 2^64 key generations in a 10-minute window.
- **No plugins, no import, no network allow-list proxy.**

## 7. How this plan can fail

| Risk | Why it is real | What to do |
|---|---|---|
| Phase A resets the outside review | A.1 to A.3 change code the review covers | O8: add them to its scope first. |
| The stacks conflict | PR 6, PR 7, A.1, A.2, A.5 and A.6 all touch `agent-native/src/lib.rs`; PR 7 rewrote 445 lines of it | Merge PR 7 into `integration/gate3` first if the owner agrees; otherwise rebase after each merge and keep A tasks small. |
| The freeze never ends | 3.10 is in Gate 3 and nothing under `web/` runs here | W.1 this week, or its CI fallback. |
| Experiments prove nothing | Predictions come from reading; a test can be written to pass | Each B test has a control that must fail. |
| Phase C stalls | Eleven notes awaiting sign-off | Each is S and can be drafted on the recommendation, as the fix plan does with its own decisions. The building task that depends on a note does not start without the sign-off. |
| The outside review never happens | Gate 3 asks for one outside review of `crates/link`, the approval path and the sandbox; no reviewer and no date exist | The owner names a reviewer or a date now. Until then every T, N and X task is blocked, however fast Phase A goes. |
| A shared node leaks between workspaces | O3 puts two workspaces on one machine, one Unix user and one link; host sessions are separated only by what the sandbox denies | C.12's separation tests are N.1's and N.4's abuse tests, on the host and in a container; a node may refuse host sessions once it serves more than one workspace. |
| Phase N eats everything | Five L tasks before environments work | T runs beside it; N.6 to N.8 wait if time is short. |
| No machine for Podman tests | No container runtime here | Find a CI runner or a second machine before N.4; tests D14, D15 first. |
| Short grants break long turns | Minute TTLs against A3 | O10's ceiling. |
| The guard blocks legitimate polling | Waiting on a build repeats a call | Warn in chats; block at five only with unchanged outcome, hard stop only in routines. |
| The `edit` guard annoys | A formatter between read and edit makes files stale | The ladder applies an edit whose span is byte-identical. |
| The plan drifts from the code | The report's first draft got a third of its statements about us partly wrong | Each task re-reads its cited lines on its base branch first. |

## 8. Minimum set, if time is short

A.10, A.1, A.2, A.3, A.4, A.8 and A.9 (about four days); W.1 and W.2 (two days, for Gate 3);
B.1 and B.2 (a day); C.12, C.1, C.3 and C.5 (three days, before N.1). After Gate 3: T.4, then N.1 to
N.3. That closes the fail-open log, the broad turn allowance, silent divergence, the shared
writer folder and both ways a file is lost, and it ends the freeze. No environments, fork or
extensibility.

## 9. Traceability

| Item | Task | Note |
|---|---|---|
| A1 approvals | A.2, C.4, W.3 | Digest dropped; single use and owner check exist |
| A2 fence | A.3; C.1, N.1, N.5 | Epoch with the features that move sessions |
| A3 credentials | A.12 | Mostly done on PR 4; prctl, `rustix`, `LD_*` dropped |
| A4 tokens | A.13, T.5 | O5 |
| A5 length stop | A.6 | Premise wrong; now a message |
| A6 append first | A.1 | Raised to first |
| A7 fail closed | A.1 | Other cases done |
| A8 roster | A.4, A.11, T.7 | Rest done |
| A9 guard, limits, ceiling | A.5, T.1 | Ceiling exists; proportional cap dropped |
| A10 retry | A.14 | Mostly done; other tuning dropped |
| A11 compaction | A.7, B.8, B.9 | Cache-preserving request and tiers dropped (O1) |
| A12 seccomp, `.git` | C.10, T.6; C.8 | `.git` read-only impossible with Landlock: residual |
| A13 edit guard | A.8, C.9, T.2 | |
| A14 restore | A.9, A.15, B.2, B.3, B.6 | Rest done; timing fields only if B.3 asks |
| A15 deltas | B.10 | Dropped; fix plan 4.8 decides; browser half done |
| (b) §4.1, §6 | C.1, C.2, C.5 | Bearer hashing already our pattern |
| (b) §4.2 | C.5, N.2 | |
| (b) §4.3 | C.3, N.3 | Session binding already in the note |
| (b) §4.4 | C.5, N.1 | Fingerprint length not scheduled |
| (b) §4.5 | A.5, C.6 | `[limits]` exists |
| (b) §4.6 | C.6, N.4 | Most of the argv already in the note |
| (b) §4.7 | C.6, N.6 | `GIT_*` already stripped |
| (b) §4.8 | C.6, N.7 | |
| (b) §4.9 | C.2, N.5 | |
| (c) 1 unattended | C.7, T.4 | Never-auto-approve already true |
| (c) 2 workspace nodes | N.1 to N.3 | Not dependent on A15 |
| (c) 3 environments | N.4 | |
| (c) 4 fork | N.5 | |
| (c) 5 web, memory | A.11, T.3 | |
| (c) 6 to 10 | X.1, X.2, X.3, X.4, X.5 | Instruction files added to X.2 |
| (c) 11 rules, plan mode | C.4, X.6, T.7, X.7 | |
| (c) plugins, code mode | Dropped | §6 |
| D1, D2 / D3 / D4 / D7 | B.1 / B.2 / B.3 / B.4 | |
| D13 / D17 / D19 / D20 | B.6 / B.7 / B.8 / B.5 | |
| D5 / D8 / D9 | A.3 / A.1 / A.6 | Their failing tests |
| D6 | N.5 | No archive yet |
| D10 | N.3 | Covered on PR 4; grant case later |
| D11, D18 | Dropped | Covered on PR 4 |
| D12 | X.3 | Partly covered; residual on PR 4 |
| D14, D15 | N.4 | Need Podman |
| D16 | Dropped | Settled by reading |
| Missed 1, 2 / 3, 4 | A.4, A.1 / A.3 | |
| Missed 5 / 6 / 7 | A.13, T.5 / A.5 / A.11, T.3 | |
| Missed 8 / 9 / 10 | B.10 / A.9 / A.8 | 8 feeds fix plan 4.8 |
| Not settled: cut calls per adapter; instructions after compaction | A.6; B.9 | |
| Exec 1 / 2 / 3 / 4 | A.1 / A.2 / A.3, C.1 / A.4 | |
| Exec 5 / 6 / 7 | A.5, A.6, T.1 / A.7 / A.8, T.2 | |
| Exec 8 / 9 | B.2, B.3, A.9 / C.3, C.8 | |
| PR 4, 6, 7 UI follow-ups | W.3, W.4, W.5 | |
| PR 7 owner questions | O1, O2 | |
| Design note's blocking question | O3, C.12, N.1 | Answered: a node may serve several workspaces |
