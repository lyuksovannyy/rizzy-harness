# ADOPTION PLAN: rizzy-harness

**Status:** accepted by the owner on 2026-11-11 with one change (O3: a node may serve several
workspaces, see C.12). **Progress, 2026-11-11:** Phase A tasks A.1 to A.14 are written and merged
(A.1 to A.5, A.8 to A.10, A.12, A.13 into `integration/gate3`; A.6, A.7, A.14 into pull request
7; A.11 into pull request 6). A.15 and the new A.16 are open. Experiments B.1, B.2 and B.5 have
run. Phases C, T, N, X and W have not started. **2026-11-12: fix plan Gate 3 is passed (the
owner); the feature freeze is over.** Phases A and R are merged, with experiments B.1, B.2, B.3
and B.5 and notes C.12, C.13 and C.14. What comes first is under "After Gate 3" in §4.

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
10. **A data directory in use is never wiped** (owner, 2026-11-21): a database schema change
    is a new migration, an event change is additive. No other compatibility code before the
    first tag.

Sizes: **S** under half a day, **M** one to two days, **L** three to five days, for one person
working with an agent. Estimates, not promises.

## 2. Decisions already made (recorded, not reopened)

- The fix plan finishes first; features wait for Gate 3. Fixes to what is built are not features.
- Podman is the primary container runtime; Docker only as a self-hosting alternative.
- The agent's shell (owner, 2026-11-12; O12, from B.11): plain sessions run each command in
  bubblewrap, image-based environments in rootless Podman, and Landlock stays for the node's
  own helpers, as a second layer inside bubblewrap, and as the shell's confinement where
  bubblewrap cannot run. This supersedes the direction "replace Landlock with Podman".
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
  one. To guard against a compromised master, the node checks those bytes against the binary's
  hash in the project's GitHub release, which it fetches itself (`update_verify = "release_hash"`,
  the default); `"none"` (no check, the node trusts its master) remains for untagged and
  self-built servers by explicit choice, and is what development nodes set; an offline signing
  key is planned (owner, 2026-11-12; C.14, `docs/node-update.md` §2).

## 3. Decisions put to the owner, and the answers

**Answered on 2026-11-11.** O3 was answered differently from the recommendation: see its row.
Every other row was accepted as recommended. The table is kept as the record of what the
other choice would have cost.

| # | Decision | Recommended | If you choose the other |
|---|---|---|---|
| O1 | PR 7's compaction default: 24,000 tokens as before, or the window minus a reserve | **Window minus a reserve.** At 24k a 200k model summarises after a few tool results and loses the task; summaries are lossy. Cost-sensitive operators set `summarize_after_tokens`; the README says what it costs. Since X.15 the same line is also checked before every request inside a turn (a configured line below it stays for between turns only). | 24k: cheaper per turn, many more summaries, and summary quality (A.7) becomes the main way context is lost. Set it before PR 7 merges. |
| O2 | Does a "retrying in N s" notice justify a new event type? | **Yes (2026-11-14):** the freeze that deferred it ended, and Node is here. A `model_retry` event (model, attempt of retries, wake-up time and wait, a reason class; never the provider's body) from the turn's, the parallel answers' and the summary's calls, as OpenCode's `RetryScheduled` with its wake-up time (R9 §2). Kept in the log, not a transient frame: a reload during the wait shows it too. | — |
| O3 | Where the in-process `local` node fits once nodes belong to a workspace (the note's one blocking question) | Recommended was the note's working assumption: `local` alone as a server-admin node usable in the admin's workspaces. **The owner chose more: any node may be attached to several workspaces, and the logic is to be built around that.** `local` stops being a special case. | What it costs: two workspaces now share one machine, one Unix user and one link, so separation between them has to be built and tested instead of assumed (C.12). One node per workspace would have had that separation for free. |
| O4 | seccomp beside Landlock (a new dependency) | **Yes, after Gate 3's outside review, if the dependency check passes** (candidate `seccompiler`, Apache-2.0, `unsafe` kept inside it). Only seccomp closes UDP and DNS with `network = "off"`. T.7's read-only shell needs nothing new and goes first. | No: UDP and DNS stay named residuals; T.6 is dropped. |
| O5 | Per-user API tokens instead of the one administrator-equivalent token | **Yes, after Gate 3** (T.5, after fix plan 4.1). Until then A.13 records the bypass. Scoping the token now breaks the README's script uses. | Keep it: "chats private, for admins too" stays broken by design and the threat model says so permanently. |
| O6 | May routines ever act unattended? | **Yes, only inside a person-created, content-hashed allow set** (C.7, T.4); anything else denied with the reason; never auto-approve. | Never: a routine reaching `ask` is denied at once; T.4 shrinks to that and idempotency. **Replaced by the owner's decision of 2026-11-14, in their words: "about routines default mode - auto, still when creating/editing an route - allows to select the mode" (and "default accept mode is Auto for every chat type"): a routine runs in the mode its owner picks, Auto by default; a run with webhook data asks (`runner::run_mode`, PLAN.md T50).** |
| O7 | Clone and read OpenHands' agent SDK and Warp's worker, which Round 2 could not | **OpenHands: yes,** before C.6 is signed off; its container code is the nearest model for N.4. **Warp: no:** AGPL, ideas only, and OpenClaw already gives the fence. About a day each. | Both: a day more, nothing copyable from Warp. Neither: C.6 rests on the note and Round 2 alone. |
| O8 | A.1, A.2 and A.3 change the approval path and the link that the outside review is about to read | **Stack them on `integration/gate3` and add them to the review's scope before it starts.** | Land them after the review and get a second review of the delta before Gate 3. |
| O9 | The repeated-call guard: the brief put it in Phase A; the final review calls it a feature under the freeze | **Feature:** T.1, first after Gate 3. It adds refusals to the dispatcher PR 7 is rewriting. | Break the freeze for it: S, one more dispatcher change before the review. |
| O11 | **(New, 2026-11-11. Answered the same day: borrowing stays, and undo is to be redesigned around the user's own repository, see C.13. Further answers 2026-11-12, as they now stand: undo stays available after a commit by anyone and restores working files only, never HEAD, a branch, the index or the reflog; unstash of a file without a conflict is seamless; a conflicting file is skipped, written beside with an incrementing suffix (`filename.txt` to `filename-1.txt`) or merged by the agent; a stash can be deleted by the person or by the agent and expires at the first midnight after 30 days; the old versions of each turn's changed files are kept in the shadow (A.17). Later the same day: an undo after a commit applies the inverse change to the working files, like `revert --no-commit` but leaving the index alone, and creates no commit; conflicts while the agent unstashes are decided by the session's approval mode (`Auto`: the agent decides; `Ask`: the agent may skip or write beside, a merge asks; `AskAll`: the person decides); the gap left by an agent that prunes history inside its own turn is accepted with detection; the two-choice undo prompt was not answered and proceeds on the recommendation; a soft limit of 100 changed files per turn warns (T.9). Worked out in `docs/undo-and-stash.md`.)** Should checkpoints keep borrowing objects from the user's own repository? Experiment B.2: a history rewrite plus pruning in the user's repository, or about a month of git's default grace periods, or deleting the repository, makes an older checkpoint unrestorable. | **Decide after B.3 gives the cost.** A.16 makes the failure safe either way (refused, folder untouched). Stopping the borrowing makes every checkpoint self-contained, at the price of storing a copy of the folder's tracked content in the node's data directory and a slower first checkpoint; fix plan 2.6 chose "nothing stored twice" for large repositories. | Keep borrowing: an old checkpoint can become unrestorable after the user rewrites and prunes history; the user is told so instead of losing a file. |
| O10 | Short grants against the note's A3 ("an offline node finishes its running turn") | **A grant whose link drops lives until its turn ends or 30 minutes pass, whichever is first;** while linked it is renewed every few minutes. | Strict TTL: long turns on flaky links fail at expiry. |
| O12 | **(New and answered 2026-11-12: "Bubblewrap + Podman", as recommended.)** What confines the agent's shell, after B.11 (`docs/container-experiment.md`) | **Plain sessions: each command in bubblewrap** (a PID namespace that dies with the command and with the node, a network namespace when the network is off, later a mount view built from the node's grants). **Image-based environments: rootless Podman** as the nodes design intends, with B.11's corrections (N.4, C.6). **Landlock stays** for the node's own helpers (shadow git, PDF parser), as a second layer inside bubblewrap where the kernel has it, and as the shell's confinement where bubblewrap cannot run (nested in the published image, no unprivileged user namespaces). A node with neither refuses the shell under `[sandbox] mode = "required"`, as today. Supersedes the direction "replace Landlock with Podman" (After Gate 3). Built as T.8a to T.8d. | Podman alone: 70 ms a call against 1.5 ms, a `setsid` child survives until the container is stopped, no start under the README unit's `NoNewPrivileges=yes`, no shell in the published image. Bubblewrap alone: no image-based environments. |
| O13 | **(New 2026-11-12, the owner's direction: "we need to rethink the sandboxing [...] 3 permission nodes: podman access, sandbox (bubblewrap) access, shell access; if none are granted the user won't be able to work with workspace nodes". Recorded with four changes, numbered. **Answered the same day:** one ordered level, as recommended ("none -> podman -> sandbox -> host"; the Podman level is named `container`, after `[execution] containers`); "shell access" is a shell with no sandbox and no Podman, as the node's user; choosing `host` shows a warning that names the risks. One point is open, under "After Gate 3".)** Who may run what on a workspace's nodes. Today one workspace permission, `agentic` (`crates/master/src/workspaces.rs`), gives a member whatever the node offers, so a member who joined yesterday gets the same shell as the owner. | **A level per member per workspace, set by that workspace's admins: `none` (every new member), `container`, `sandbox`, `host`.** `container`: sessions only in environments an admin defined (rootless Podman, N.4); the member picks neither image nor mounts. `sandbox`: also host sessions in bubblewrap (T.8a to T.8c); the member chooses per chat. `host`: also a shell as the node's user with no confinement. Each level includes the ones before it. **Changes to the direction:** (1) one ordered level, not three switches: switches allow "shell but not Podman", which means nothing, and with a level every check is `session mode <= member's level <= node's ceiling`; (2) the node's `[execution]` table is the ceiling and the master's level only chooses inside it ("the node decides"): `containers`, `host_shell`, and a new key for the unconfined shell, off by default, refused on a node that serves several workspaces and, for anyone but a server admin, on `local`; a stolen master can therefore ask for any mode up to the ceiling, as it can today, so the levels protect against members, not against the master; (3) nobody grants a level above their own, and `host` is the owner's to grant; (4) the levels hold only as far as the files they share: a `container` session must not be able to write what a `sandbox` or `host` session later executes unread (`repo.git/hooks`, `repo.git/config`), so C.8's read-only binds become required instead of "evaluate". Replaces the workspace permission `agentic`; the account's server grant stays. Built as N.10. | Three independent switches: eight states for four meanings and a check per switch. No levels (today): a workspace cannot admit anyone it does not trust with its node. Podman for everyone: O12's costs (70 ms a call, no start under the README unit, nothing inside the published image) paid by the people who are trusted. |

## 4. Phases

### Phase A: fixes to what is already built (about two weeks; allowed before Gate 3)

Order follows the rewritten executive summary. "Lands on": `gate3` is a branch off
`integration/gate3` with a pull request into it (PR 6 and PR 7 rebase after each merge);
`PR 6` or `PR 7` is stacked on that branch because it rewrites the same code.

Each row's full text is in `docs/plan/`.

| ID | Fixes |
|---|---|
| [A.1 **sec**](docs/plan/A.1.md) | Exec 1; missed 2; A6, A7 |
| [A.2 **sec**](docs/plan/A.2.md) | Exec 2; A1 rewritten |
| [A.3 **sec**](docs/plan/A.3.md) | Exec 3; A2 narrowed; missed 3, 4 |
| [A.4 **sec**](docs/plan/A.4.md) | Exec 4; A8; missed 1 |
| [A.5](docs/plan/A.5.md) | Exec 5; A9; missed 6 |
| [A.6](docs/plan/A.6.md) | Exec 5; A5 rewritten |
| [A.7](docs/plan/A.7.md) | Exec 6; A11 |
| [A.8 **sec**](docs/plan/A.8.md) | Exec 7; missed 10; A13 |
| [A.9 **sec**](docs/plan/A.9.md) | Exec 8; A14; missed 9 |
| [A.10](docs/plan/A.10.md) | PR 7's verification note |
| [A.11](docs/plan/A.11.md) | Missed 7 |
| [A.12 **sec**](docs/plan/A.12.md) | A3's one gap |
| [A.13](docs/plan/A.13.md) | A4 fix half; missed 5 |
| [A.14](docs/plan/A.14.md) | A10 |
| [A.15 **sec**](docs/plan/A.15.md) | A14 |
| [A.16 **sec**](docs/plan/A.16.md) | B.2 (new, 2026-11-11) |
| [A.17 **sec**](docs/plan/A.17.md) | B.2; O11's answer (undo stays available after commits and history changes) |
| [A.18](docs/plan/A.18.md) | B.3 (gc was the largest cost of every turn); 0-byte `harness-index-*.lock` files left by a timed-out snapshot |
| [A.17 **sec**](docs/plan/A.17-2.md) | B.2, B.3; C.13 (new, 2026-11-12) |
| [A.19 **sec**](docs/plan/A.19.md) | C.14 (owner, 2026-11-12); the version check runs after `Accepted` (`crates/master/src/lib.rs:894-897`, `nodes.rs:234-268`), so a refused node reconnects… |
| [A.20 **sec**](docs/plan/A.20.md) | Nothing stops two processes on one data directory (found by T.8e's re-check, R-1, and by reading the start-up code). |
| [A.21 **sec**](docs/plan/A.21.md) | B.7 (2026-11-13, reviews B7-R1 to B7-R7, B7-D1 to B7-D4; the owner put it here): while the node's out queue is full a Stop does not end a streaming command… |
| [A.22 **sec**](docs/plan/A.22.md) | First live test with real models (2026-11-15): two chats in one folder; B edited a file inside A's turn and A's undo put it back silently |
| [A.23 **sec**](docs/plan/A.23.md) | Independent review of the project-MCP change (proof `review_scrubber_breaks_other_users_events`): with `[mcp_servers.*] env` values `approval_mode`… |
| [A.25 **sec**](docs/plan/A.25.md) | Reviews of PRs 130, 134 and 137: each found a node payload of the wrong shape… |

**Gate A:** A.1 to A.14 merged with failing-first tests; CI green; A.1 to A.3 in the outside
review's scope (O8). The freeze is still on.

### Phase B: experiments (three to four days; tests only, allowed now)

Each runs with `cargo test` on Linux with git, bash and Landlock, and has a control that must
fail (the same thing unconfined, say), so a pass means something. A test that shows a failure
is merged `#[ignore]` with the observed result and the decision it feeds.

Each row's full text is in `docs/plan/`.

| ID | Experiment |
|---|---|
| [B.1](docs/plan/B.1.md) | Tests D1 and D2: `setsid sleep 1000`, then cancel; `kill -9` the node during a long child, restart (`bash.rs`, `acceptance_restart.rs`) |
| [B.2](docs/plan/B.2.md) | Test D3: snapshot, rewrite the user's history, `git gc --prune=now`, restore |
| [B.3](docs/plan/B.3.md) | Test D4: checkpoint time over 1k to 500k files, warm and cold, 0 to 1,000 changed (`#[ignore]` benchmark) |
| [B.4](docs/plan/B.4.md) | Test D7: `kill -9` the node during `bash`, restart |
| [B.5](docs/plan/B.5.md) | Test D20: `git add -A` over a nested `.git`, with and without a commit; whole-tree restore |
| [B.6](docs/plan/B.6.md) | Test D13: rewind, restart the node, open the session |
| [B.7](docs/plan/B.7.md) | Test D17: stream at the link cap while a cancel and an approval answer arrive |
| [B.8](docs/plan/B.8.md) | Test D19: compact a session whose results hold a known key and an unknown secret-like string |
| [B.9](docs/plan/B.9.md) | Not settled: where instruction files enter the prompt and whether they survive compaction |
| [B.10](docs/plan/B.10.md) | Missed 8: fsyncs and bytes per turn on node and mirror for a long streamed answer |
| [B.11](docs/plan/B.11.md) | Containers in place of the Landlock sandbox for agent shells (the owner, 2026-11-12: a separate branch with quick Podman tests first). |

Dropped as experiments (reasons in §9): D5, D8, D9 (they are A.3's, A.1's and A.6's failing
tests), D10, D11, D18 (covered on PR 4), D12 (rerun in X.3), D16 (settled by reading), D6,
D14, D15 (moved to N.5 and N.4: no archive exists yet, and Podman is not available here).

**Results so far (2026-11-11).** B.1: confirmed. A process started with `setsid` survives cancel, timeout and a dropped call, with and without the sandbox, and a running command survives a SIGKILL of its parent; C.11 is needed. B.2: worse than predicted. A restore deletes the user's file when a borrowed object is gone, and checkpoints then stop for good; this became task A.16 and decision O11. B.5: one `git init` in a sub-folder disabled checkpoints for the whole folder, and an undo listed nested repositories as restored when nothing was; A.15 was rewritten from it.
B.3 (`docs/checkpoint-cost.md` on `exp/B.3-checkpoint-cost`): the `git gc` that ran on every turn once ten checkpoints existed was the largest cost of a turn at every folder size measured (0.26 s at 1,000 files, 3.3 s at 100,000 in a plain folder, on a slow disk); it became A.18.
B.11 (`docs/container-experiment.md`, merged in pull request 49, 2026-11-12): bubblewrap per command killed every detached process on cancel, on a normal exit and on `kill -9` of the node, at 1.5 ms a call and no idle process; rootless Podman with `podman exec` (the nodes design's §4.6) cost 70 ms a call, let a `setsid` child survive the design's in-container group kill and a node `kill -9` until the container was stopped, fails under the README unit's `NoNewPrivileges=yes`, and does not nest in a rootless container; pasta reaches host services on `0.0.0.0` and the LAN address unless `--map-guest-addr none`. Recommended: bubblewrap for plain sessions, Podman for image-based environments; Landlock was the only confinement that worked inside a default container, so replacing it removes the shell from the published image. **Decided by the owner the same day as recommended (O12, "Bubblewrap + Podman"); built as T.8a to T.8d, with N.4 for environments.**
B.7 (`crates/link/tests/control_under_load.rs`, `crates/node/tests/control_under_load.rs`, 2026-11-13, corrected after review B7-R1 to B7-R7 and its re-check B7-D1 to B7-D4; the fix is A.21). **On the real node a Stop does not end a command that streams output while the out queue (`OUT_QUEUE`, 256 messages) is full.** With the uplink taking nothing and twelve other sessions running `yes`, a stopped command that printed a line every 0.1 s was still writing 6.1 s after the Stop, past the agent loop's 5 s `TOOL_CANCEL_GRACE`, and no write was seen once the uplink was read again (a look of 0.4 s against one write every 0.1 s: stopped within about 0.1 s of it); the ack and the turn's end came 6.09 s after the Stop (with a reading uplink, the first review's run saw no write after the Stop and the ack after 59 µs). The agent loop's `coalesce` awaits the next `tool_output_delta`'s append inside a `select!` arm (`crates/agent-native/src/lib.rs`), the append waits in `publish_wait` for room (`crates/node/src/session.rs`), and while it waits neither the tool nor the cancel is polled. So one workspace's output keeps another workspace's stopped command running, and room for one message does not end it: the appends waiting for room are served first come, so the uplink must take one message for every append waiting ahead (re-check B7-D1, in process: with twelve other streaming sessions 14 messages, 2.5 s at one message per 200 ms and 14.0 s at one a second, the Stop's ack not yet arrived). The link's `IDLE_LIMIT` (60 s) times each frame, so it caps nothing for a peer that takes a frame just inside every 60 s (by reading); it is the bound only for a peer that takes nothing. A call's timeout is held the same way (measured: a 60 s timeout, still writing 75 s after the prompt, ended as timed out when the uplink read again; `a_timeout_ends_a_streaming_command_while_the_uplink_takes_nothing`, `#[ignore]`); the node's own stop takes the same path (by reading; not run). The 5 s grace counts from the Stop and runs out unseen (re-check B7-D2): with the uplink shut 2 s after the Stop the tool ended "[cancelled: process group killed]"; shut 7 s, the loop dropped the tool without its own stop having run, recorded as abandoned, its end not known. A command that writes nothing is stopped at once (no write seen in the first look after the Stop), the control. **Master to node,** the link holds nothing back, but the node's command loop answers a cancel, an approval answer and a `stop_background` inline and waits for room to ack each: whichever comes second is not read until the first one's ack has room, and meanwhile no command of any session is read. The real node, a silent command, an approval answer and then a Stop: the command was still writing 6.0 s after the Stop (6.2 s in another run), its ack 6.04 s after it. In the link test's model (proxy at 1 MiB/s) the second frame reached the loop after 41 ms to 1.21 s depending on when it was sent (1.11 s in the committed test; 1.12 s for a cancel sent second; up to 4.99 s at 256 KiB/s); that wait is the socket's next writable moment and grows with the kernel's send buffer. Once the node's in queue (`IN_QUEUE`, 64) and the master's `TO_NODE_QUEUE` are full, the master's `try_send` fails and every command to that node answers Timeout at once (by reading). **Node to master,** nothing has a lane: at 1 MiB/s with 8 KiB frames the cancel's ack reached the master after 6.6 s and the approval answer's after 7.2 s, behind 783 frames (6.1 MiB); at 256 KiB/s, 26.2 s and 28.2 s (rerun after the review: 26.2 s and 28.3 s), past the master's 15 s `ACK_TIMEOUT`, where Stop answers 504 for a cancel that reached the node (by reading; the route was not driven). These are loopback numbers: 4.1 MiB of the 6.1 MiB was the node socket's send buffer, autotuned to this machine's `tcp_wmem` maximum (4 MiB), which a real path at these rates need not reach; the node's own 256 frames of 8 KiB are about 2 s at 1 MiB/s. But the out queue is bounded in messages, not bytes: with 64 KiB frames the cancel's ack waited 20.2 s behind 18.8 MiB at 1 MiB/s, the real node's `bash` output (`yes`) made events of 8.1 KiB on average (1,547 events, re-check B7-D3) and a full out queue of them held 1.4 MiB, but an event may be 4 MiB and a replay's snapshot batch 2 MiB, so after a reconnect up to 256 x 2 MiB (512 MiB, by reading; also node memory) can be ahead of an ack. **Precondition:** the queue fills only when the node produces faster than its uplink carries. The link test floods from memory; the real node writes each event to node.db with `Durability::Full` first, and on this checkout's slow disk it produced about 2.5 messages a second (one rough count), which no uplink above about 20 KiB/s backs up; on a disk with a 1 to 5 ms fsync it fills. The production rate on a fast disk was not measured. **Fix sketch, three parts (A.21 carries it):** (1) the agent loop keeps polling the tool and the cancel while an emit waits, so a Stop, a timeout and the node's stop end a command whatever the out queue holds; (2) a node command loop that does not wait on the out queue before reading the next command; (3) a control lane in the pump for the acks of the inline commands only (cancel, approval answer, `stop_background`): other acks may not jump the queue, since the master relies on a create's ack coming after its `session_created` (`create_session`, `crates/master/src/lib.rs`; other acks not checked one by one), and an approval request is an event, so it may pass other sessions' events but never its own session's (`seq = head + 1`). Even with all three, the cancelled turn's end is an event of that session and waits behind that session's queued output, so the UI, a fold over the log, shows the turn running after Stop returned unless a cancelled session's queued output is dropped or passed. The failing tests are `#[ignore]` with these numbers, and so are the two that only measure (the link test of the master-to-node direction, the real node's silent control); the link test's command loop is a model and must be replaced with the real node's when the fix lands. **Fixed by A.21 (2026-11-14):** the same tests pass un-ignored on the real node, and over the real link at 256 KiB/s a Stop and an approval answer are acked after 1.0 s (`crates/node/tests/control_over_link.rs`).

**Gate B:** every B test merged, passing or `#[ignore]` with its result, and each outcome
written into the C note or task it feeds.

### Phase C: design decisions on paper (about a week plus owner time; allowed now)

Each task changes `docs/nodes-credentials-environments.md` or adds a short note in `docs/`.
**Done when, for every row: the note is merged with the owner's sign-off and names the tests
it commits the building task to.** Every row is **S** except C.12 and C.13, which are **M**; C.12 comes
before C.3, C.5 and C.6 because it changes what they are written against.

Each row's full text is in `docs/plan/`.

| ID | Decision |
|---|---|
| [C.1](docs/plan/C.1.md) | Fence for the link and for fork |
| [C.2](docs/plan/C.2.md) | Fork by copy with lineage and an explicit head |
| [C.3](docs/plan/C.3.md) | Grant lifetime and binding |
| [C.4](docs/plan/C.4.md) | What "always allow" may mean, where a rule lives |
| [C.5](docs/plan/C.5.md) | Credentials, pairing, aliases (note amendments) |
| [C.6](docs/plan/C.6.md) | Containers, forge, deletion (note amendments) |
| [C.7](docs/plan/C.7.md) | Unattended authorisation (O6) |
| [C.8](docs/plan/C.8.md) | The `.git` residual |
| [C.9](docs/plan/C.9.md) | Staleness guard and match ladder for `edit` |
| [C.10](docs/plan/C.10.md) | seccomp (O4) and a read-only shell |
| [C.11](docs/plan/C.11.md) | Process-tree containment (only if B.1 leaks) |
| [C.12](docs/plan/C.12.md) | A node in several workspaces (O3) |
| [C.13](docs/plan/C.13.md) | Undo and checkpoints, second design (owner's direction, 2026-11-11, answering O11). |
| [C.14](docs/plan/C.14.md) | Node update and drain (owner's direction and answers, 2026-11-12). |
| [C.15](docs/plan/C.15.md) | Background commands (owner's direction, 2026-11-12; task T.8d). |

**Gate C:** C.1 to C.10, C.12, C.13 and C.14 signed off (C.11 if B.1 failed); the design note lists no
blocking question and no passage marked superseded; every T, N and X task can name its abuse tests from a merged note.

### Phase T: loop and tool features (after Gate 3; three to four weeks)

Independent of the node redesign and in other crates, so it runs beside Phase N. One person
should do T.1 to T.4 before N.1: they are small and fix gaps users meet today.

Each row's full text is in `docs/plan/`.

| ID | Adds |
|---|---|
| [T.1](docs/plan/T.1.md) | Repeated-call guard |
| [T.2 **sec**](docs/plan/T.2.md) | `edit` staleness guard (C.9) |
| [T.3 **sec**](docs/plan/T.3.md) | Web and memory tools on agentic nodes |
| [T.4 **sec**](docs/plan/T.4.md) | Unattended authorisation (C.7) |
| [T.5 **sec**](docs/plan/T.5.md) | API keys per user and per workspace (O5; the owner, 2026-11-12) |
| [T.6 **sec**](docs/plan/T.6.md) | seccomp (C.10, if O4). |
| [T.7 **sec**](docs/plan/T.7.md) | Read-only shell for read-only agents (C.10) |
| [T.8a **sec**](docs/plan/T.8a.md) | Done (branch feat/T.8a-shell-containment): every agent shell command runs in bubblewrap, [sandbox] contain (auto, required, off), tests in… |
| [T.8b **sec**](docs/plan/T.8b.md) | Done (branch feat/T.8b-network-off): with [sandbox] network = "off" the contained shell's bubblewrap gets --unshare-net (Contain::args, decided by the node's… |
| [T.8c **sec**](docs/plan/T.8c.md) | Done (branch feat/T.8c-mount-view): a contained command's mount namespace is bubblewrap's fresh root (read-only once built) with only the system read set (the… |
| [T.8d **sec**](docs/plan/T.8d.md) | Background commands (the owner, 2026-11-12). |
| [T.8e **sec**](docs/plan/T.8e.md) | Done (branch feat/T.8e-kill-guard, reviewed in docs/security-review/2026-11-T8e-guard.md and fixed): each contained command runs as node, guard, bwrap, init… |
| [T.8f **sec**](docs/plan/T.8f.md) | Done (branch `feat/T.8f-image-link`): `sandbox-exec` in a contained command is started through a descriptor of the node's image, not a path. |
| [T.9](docs/plan/T.9.md) | Soft limit of 100 changed files per turn: warn at the end of the turn and on its undo; never block |

**Gate T:** each task's abuse tests in CI and the threat model updated with it.

### Phase N: workspace nodes, keys, grants, environments (after Gate 3; months)

The design note's build order (§8), in its sequence, with the research folded in. "Note tests"
are that phase's tests in the note. Each task carries its own UI as lazy chunks, so each needs
W.1.

Each row's full text is in `docs/plan/`.

| ID | Note phase |
|---|---|
| [N.1 **sec**](docs/plan/N.1.md) | 1 workspace nodes, pair codes |
| [N.2 **sec**](docs/plan/N.2.md) | 2 BYOK, aliases, slots |
| [N.3 **sec**](docs/plan/N.3.md) | 3 grants |
| [N.4 **sec**](docs/plan/N.4.md) | 4 environments |
| [N.5 **sec**](docs/plan/N.5.md) | 5 archive, fork |
| [N.6 **sec**](docs/plan/N.6.md) | 6 Connections (the part a person uses is planned ahead of N.1 as X.27, the owner, 2026-11-16) |
| [N.7 **sec**](docs/plan/N.7.md) | 7 classification, deletion, autosave |
| [N.8 **sec**](docs/plan/N.8.md) | 8 proxy, per-user node (experimental) |
| [N.9 **sec**](docs/plan/N.9.md) | `docs/node-update.md` §10 |
| [N.10 **sec**](docs/plan/N.10.md) | none (O13) |

N.3 and N.4 can run in parallel after N.2, and N.6 after N.1, as the note says. C.12 also
binds N.3 (grants keyed by workspace), N.4 (environments, volumes and the per-workspace share
of containers) and N.7 (detaching one workspace is a deletion scoped to it); each carries
C.12's tests for its part.

**Gate N:** per task, the note's tests and the abuse tests above in CI; N.4 and N.7 on a
runner with rootless Podman (confirm one exists before N.4 starts).

### Phase X: extensibility and interaction (after Gate 3 and the T and N tasks named)

Order from `10` §6.1. Plugin code bundles (hooks, programs, a marketplace), code mode and import are not planned (§6); the Plugins screen of X.34 lists skills and connectors only.

Each row's full text is in `docs/plan/`.

| ID | Adds |
|---|---|
| [X.1](docs/plan/X.1.md) | Context-change events ((c) step 6) |
| [X.2 **sec**](docs/plan/X.2.md) | Instruction files and skills, text only |
| [X.3 **sec**](docs/plan/X.3.md) | MCP client with deferral |
| [X.4 **sec**](docs/plan/X.4.md) | Hooks |
| [X.5](docs/plan/X.5.md) | Input during a turn |
| [X.6 **sec**](docs/plan/X.6.md) | Per-command rules |
| [X.7 **sec**](docs/plan/X.7.md) | Enforced plan mode |
| [X.8](docs/plan/X.8.md) | Loop staying power (the owner, 2026-11-14: "all useful things other harnesses ship") |
| [X.9 **sec** (done, T50)](docs/plan/X.9.md) | Per-chat approval modes the person changes (the owner, 2026-11-14: "not that usable"): Ask, Accept edits, Auto, as Claude Desktop names them… |
| [X.10](docs/plan/X.10.md) | Tool-call checks (the owner, 2026-11-14) |
| [X.11 **sec** (done, T51)](docs/plan/X.11.md) | A denial's note and "allow for this chat" (the owner's brief, 2026-11-14) |
| [X.12 **sec** (done, T53)](docs/plan/X.12.md) | A turn's changes as a diff (round 1 ranks it our biggest UI gap: 08 "Diff presentation" and §6 item 2, 05 "Diff views"; Codex Desktop, Claude Desktop… |
| [X.13](docs/plan/X.13.md) | Earlier chats and memory search (the owner, 2026-11-14: "yes, but must have dedicated permission, like web search, file access do") |
| [X.14](docs/plan/X.14.md) | Step limit: 1,000 by default, per chat… |
| [X.15](docs/plan/X.15.md) | Shortening context inside a turn… |
| [X.16](docs/plan/X.16.md) | Hand-off notes and one small model… |
| [X.17 **sec** (done, T63)](docs/plan/X.17.md) | `@` file mentions in the composer (the owner, 2026-11-15: the useful things the other harnesses ship) |
| [X.18](docs/plan/X.18.md) | What a chat costs, estimated from prices the admin sets (the owner, 2026-11-15: the useful things other harnesses ship, "no guessing", usability first) |
| [X.19](docs/plan/X.19.md) | Read-only tool calls of one step run at the same time… |
| [X.20](docs/plan/X.20.md) | Manual compact with focus text… |
| [X.21](docs/plan/X.21.md) | Retry a turn that failed (research gap list: "rewind to a message: files + conversation + prompt back in the composer; Retry on a failed turn"; this row is… |
| [X.22](docs/plan/X.22.md) | Follow-ups to a sub-agent it already ran… |
| [X.23 **sec** (T71)](docs/plan/X.23.md) | Images through `read` (research gap list "images through `read` (M)"; X.22 is the highest id in flight, so this is X.23) |
| [X.24 **sec** (T72)](docs/plan/X.24.md) | Background sub-agents (research round 2, synth/09-subagents-models-retry.md "Delivery timing": Claude Code run_in_background, OpenCode background, Codex… |
| [X.25 **sec** (T73)](docs/plan/X.25.md) | The model's thinking kept and sent back the way each provider needs, so thinking models can use tools… |
| [X.26](docs/plan/X.26.md) | **Built (2026-11-17, branch `feat/project-button`), but for the forge entry (X.27).** |
| [X.27 **sec**](docs/plan/X.27.md) | **Steps 1, 2 and 3 of 4 done (2026-11-18, 2026-11-19); step 4 (push) left, so the row is not Done:** (1) a person's own connection by a pasted token, sealed… |
| [X.28](docs/plan/X.28.md) | Agent switch as state, not history… |
| [X.29 **sec** (T74)](docs/plan/X.29.md) | Account menu, Profile and Personalisation… |
| [X.30 **sec** (T75)](docs/plan/X.30.md) | Streamed text without the secret scrubber's lag… |
| [X.31 **sec** (T76)](docs/plan/X.31.md) | Three owner decisions of 2026-11-17 |
| [X.32 **sec** (T78)](docs/plan/X.32.md) | The task board for agents (the owner, 2026-11-17: "what about the taskboard usability do ai agents able to interact with it?"). |
| [X.33](docs/plan/X.33.md) | Workspace agents (the owner, 2026-11-17: "at the end everything must be workspace isolated (excluding user facts, their settings), so while built-in stuff… |
| [X.34 **sec**](docs/plan/X.34.md) | Plugins: a skills store, and which agents may use which skill or connector… |
| [X.35 **sec**](docs/plan/X.35.md) | Connectors the master answers… |
| [X.36 **sec**](docs/plan/X.36.md) | A repository's own MCP servers… |
| [X.37 **sec**](docs/plan/X.37.md) | Agent peers (the owner, 2026-11-19: "add ability to run multiple agent peers, where in order for one agent to contact other one it must start it's message… |
| [X.38 **sec** (T88)](docs/plan/X.38.md) | Git works unattended in an agent's shell, with the person's identity… |
| [X.39](docs/plan/X.39.md) | Usage during a turn (the readiness trial of 2026-11-19, item 11: a 20-minute turn showed 1505k tokens · ≈ $1.59 throughout, then jumped to 4252k and $3.16… |
| [X.40](docs/plan/X.40.md) | Push from a host chat: the recipe and doctor's check (a trial found "there is no way to push or open a pull request from a chat"). |
| [X.41](docs/plan/X.41.md) | Sub-agents for coding (the pair trial of 2026-11-20, trial/REPORT.md §6: a Pair Coder got every sub-agent with the switch on, only an agent file's delegates… |
| [X.42](docs/plan/X.42.md) | Undo and the branch: a note when HEAD moved; putting back what an undo replaced… |
| [X.43 **sec** (T9, T36, T39)](docs/plan/X.43.md) | Background commands in a `host` chat… |
| [X.44 **sec** (T45)](docs/plan/X.44.md) | Editing by tag and row numbers (the owner, 2026-11-20: "make editing by hash then like in oh my pi") |
| [X.45](docs/plan/X.45.md) | Agent avatars as flat blobs (the owner, 2026-11-20: "rework agent avatars, they must have sort of those styling", with three reference pictures of flat blob… |
| [X.46 **sec** (T90)](docs/plan/X.46.md) | Rewind a conversation to an earlier message (`docs/plan/X.21.md` left it open: editing the message first, cutting the failed attempt out of what the model sees, retrying an older turn…
| [X.47 **sec** (T91)](docs/plan/X.47.md) | The `history` tool: an agent reads its own chat's earlier turns, results, attachments and images in full after a summary… |

When X.2's work on instruction files and prompts starts, the instructions of every agent that can change files gain the 100-file rule of `docs/undo-and-stash.md` §2.6 (try to keep a turn's changes to at most 100 files; split larger work across turns; exceeding it is allowed).

**Gate X:** each task's abuse tests in CI.

### Phase W: the user interface (blocked on Node; W.1 and W.2 now)

Fix plan Gate 3 includes 3.10, so the freeze cannot end until W.2 lands or L14 is accepted in
the threat model. W.1 and W.2 are on the critical path.

Each row's full text is in `docs/plan/`.

| ID | Covers |
|---|---|
| [W.1](docs/plan/W.1.md) | Web checks not runnable here |
| [W.1 (done)](docs/plan/W.1-2.md) | Browser tests runnable here (the owner, 2026-11-12: through `uv`, in the project) |
| [W.2 **sec**](docs/plan/W.2.md) | Fix plan 3.10 (L14) |
| [W.3](docs/plan/W.3.md) | PR 4 follow-ups |
| [W.4](docs/plan/W.4.md) | PR 6 follow-ups |
| [W.5 (done)](docs/plan/W.5.md) | PR 7 follow-up (O2 is yes) |
| [W.5b (done)](docs/plan/W.5b.md) | A person cannot see when a turn ended elsewhere, how full the context is, or what a sub-agent does (2026-11-14) |
| [W.5c (done)](docs/plan/W.5c.md) | Rename and export a chat (2026-11-14) |
| [W.6 **sec** (built, PR open)](docs/plan/W.6.md) | The approval takes the composer's place (the owner, 2026-11-12) |
| [W.7](docs/plan/W.7.md) | The browser scripts pass again |
| [W.7 (done)](docs/plan/W.7-2.md) | Result, 2026-11-12 (`fix/W.7-browser-scripts`) |
| [W.8](docs/plan/W.8.md) | What the browser run turned up |
| [W.9 (done)](docs/plan/W.9.md) | The sidebar to the owner's sketch (2026-11-16) |

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

Each row's full text is in `docs/plan/`.

| ID | Findings |
|---|---|
| [R.1 **sec**](docs/plan/R.1.md) | S-1 (Critical), S-2 (High), S-3 (High), S-4, S-6 |
| [R.2 **sec**](docs/plan/R.2.md) | P-1 (High), P-2, P-3, P-7 |
| [R.3 **sec**](docs/plan/R.3.md) | L-1 to L-4 (Medium), L-5, L-6, L-8, L-9 |
| [R.4](docs/plan/R.4.md) | P-4, P-5, P-6, L-7, S-5, the Notes |
| [R.5](docs/plan/R.5.md) | The whole of Phase R |

**Gate R:** R.1 to R.5 done. Only then can fix plan Gate 3 be called. **Done; Gate 3 was called
passed by the owner on 2026-11-12.**

### After Gate 3: what the review left open, and the owner's directions (2026-11-12)

The owner passed the gate and, for five of the points the review left open, said how each is to
be closed. These come first, in the order below the table.

| Left open | The owner's direction | Task |
|---|---|---|
| Nothing was run in a real browser | Playwright through `uv`, inside the project | **W.1, done 2026-11-12** (see its row). The first full run of `web/e2e/*.cjs` on `main` failed in 11 of 15 scripts: **W.7** |
| The approval card shows what runs but cannot make anyone read it | As Claude Desktop does: the composer's text field is replaced by the choice to allow or deny, and the browser sends a notification | **W.6** |
| The static API token acts as every user | No static token. API keys per user and per workspace; creating a workspace key needs its own permission | **T.5**, extended |
| The sandbox exists only where the kernel has Landlock | First: replace it with Podman, after a separate branch with quick Podman tests. **Superseded the same day by O12** ("Bubblewrap + Podman", from B.11's results): bubblewrap for plain sessions, rootless Podman for image-based environments, Landlock kept for the helpers, inside bubblewrap and where bubblewrap cannot run | **B.11, done and decided**; then T.8a to T.8c; N.4 for environments, in Phase N |
| Processes an agent leaves running outlive their command | Background commands that call the agent back when they end; a foreground command stops at its timeout (60 s when none is given), a background command must state one | **T.8d**, on T.8a's containment |

Not reopened, so they stay as recorded: the workspace `agentic` role is not a boundary until
the nodes design is built (N.1 to N.4); the tree-sitter parsers run in the process that holds
the keys; the review was done by an independent model.

**Order:** W.7 (a browser run that passes is the test bed for W.6), W.6, T.5 after fix plan 4.1,
B.11 (done, decided as O12), then T.8a (containment), T.8e (the guard that confirms every kill;
right after T.8a, because it changes how a command is started and T.8d's background commands
rest on it), T.8b (network off), T.8c (the file view without Landlock) and T.8d (background
commands), each its own small pull request because they change the sandbox; T.8b and T.8c need
only T.8a, T.8d needs T.8e. N.4 stays in Phase N, but O13 makes it the way in for members at `container` (below). T.1, A.19 and A.20
(the lock on the data directory) fit in between; they touch other files. A.21 (B.7's fix: the node's command loop, the agent loop's emit and the pump's acks) comes after N.10a part 1 (on `main` since pull request 78) and before part 2.

**Execution levels (the owner, 2026-11-12; decision O13, task N.10).** The owner's direction:
a member the workspace does not trust yet (a new employee, a trainee) gets Podman only; a
member the workspace's admins trust may also choose the bubblewrap sandbox; a third permission
gives the shell; with none of them a member cannot use the workspace's nodes. Recorded as one
ordered level per member (`none`, `container`, `sandbox`, `host`) under the node's own ceiling;
O13's row has the four changes made to the direction and why.

The order is by what a session starts from, not by a stronger wall. A container starts from an
image and sees only what is mounted, with its own network namespace; a bubblewrap session
starts from the host and sees what the file policy leaves (until T.8c builds the view from the
grants) and, with the network on, the host's loopback services. Both share the node's kernel
and its Unix user: an escape from either is the node user. Neither is a virtual machine, and
the note must not describe `container` as safe against someone who sets out to break it.

What it changes in the order: Podman was "for image-based environments, in Phase N" and is now
the only way in for a member at `container`. Until N.4 is built no node offers that mode and
such a member cannot work at all, which is the direction's own rule. N.10a (the level, the
ceiling and the checks for the two modes that exist) is small and comes after T.8e; T.8c gains
weight, because it is what makes `sandbox` a level worth granting; N.4 no longer waits for a
user who asks for images.

**Answered by the owner, 2026-11-12:** "shell access" is a shell with no sandbox and no Podman,
as the node's current user; the permission is one ordered level, none, Podman, sandbox, host;
choosing `host` shows a warning that says the risks (N.10's row has the text's content).

**Answered by the owner, 2026-11-13:** a node that can run neither bubblewrap nor Podman (inside
the published image; B.11) has only Landlock, and Landlock alone counts as `host`, not as
`sandbox`: it gives no PID namespace and no confirmed kill, which the rest of the sandbox work
relies on. Such a node offers agent chats only with the unconfined shell turned on in its own
`node.toml`. The node setting that would count Landlock as `sandbox` from ABI 9 is not built:
it would give `sandbox` two strengths; it can be added if someone needs it.

**Answered by the owner, 2026-11-14 (part 2):** the account-wide yes or no
(`users.can_agentic`) is replaced by an **account ceiling**, one of the same
levels, set by server admins. A member's effective level in a workspace is
the lower of their account ceiling and their level in that workspace; in a
Personal workspace the level is the account ceiling itself. So a team owner,
self-made or not, can never raise anyone, themselves included, past what a
server admin allowed for that account, and the Personal workspace needs no
rule of its own (the hole the interim rule of part 1 closed with "server
admins only"). A server admin's ceiling is `host`; like anyone else they
have a level in a workspace only as its member.

**Built in two parts (2026-11-13).** N.10a touches auth, the node link and the UI, so it is two
changes with a review each: first the node's side (the `[execution]` ceiling, the modes a node
can enforce announced at hello, the session's mode recorded at creation), then the level per
member that replaces the role-derived `agentic` permission.

## 5. Order and dependencies

```
now:     A.10 ─► A.1 ─► A.2 ─► A.3 ─► A.4 ─► A.5 ─► A.8 ─► A.9 ─► A.12, A.13
         on PR 7: A.6, A.7, A.14     on PR 6: A.11     B.5 ─► A.15
         B.1..B.10 and C.1..C.10 beside A;  B.1 ─► B.11 (O12) ─► T.8a ─► T.8e ─► T.8d;  T.8a ─► T.8b, T.8c
         C.12 ─► C.3, C.5, C.6   (a node serves several workspaces)
         O13 ─► N.10a part 1 ─► A.21 ─► N.10a part 2 (on T.8a, after T.8e) ;  N.10a + N.4 ─► N.10 (the `container` mode)
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
- **No plugin code bundles (hooks, programs, a marketplace), no import, no network allow-list proxy.** The Plugins screen (X.34) is a list of skills and connectors.

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
| A11 compaction | A.7, B.8, B.9, X.15 | Cache-preserving request dropped (O1); tiers inside a turn done in X.15 (clearing old tool results, then summarising the turn's steps). Summary for coding work done (2026-11-14): agentic chats are summarised under Goal, Decisions, Files, Commands and results, Open and Next step (about 1,000 words; a later summary updates the sections; chat-profile chats keep the generic 300 words); the summary still sits in the system prompt, moving it out is later |
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
| (c) 1 unattended | C.7, T.4 | Replaced: the owner's decision of 2026-11-14, in their words: "about routines default mode - auto, still when creating/editing an route - allows to select the mode"; see T50 |
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
