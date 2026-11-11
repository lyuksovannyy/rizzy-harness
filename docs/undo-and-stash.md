# Undo and stash: design note

Status: **decided by the owner on 2026-11-12, Q1 (the two-choice prompt, §3.3) included.**
Nothing built. Nothing blocks the build
after Gate 3 of `FIX_PLAN.md` (feature freeze), and task A.17 (§2.4), a fix to built behaviour,
may land earlier. Task C.13 of `ADOPTION_PLAN.md`, answering decision O11. Costs come from
experiment B.3, `docs/checkpoint-cost.md` (cited as "B.3"). **B.3 measured on a rotational USB
hard disk under a load average of 10 to 14, warm cache only: they are slow-disk numbers, and no SSD
or quiet machine was measured.**

**Two rules frame the note.** First, an undo after a commit works "like git revert" (the owner's
words). It never removes a commit from history. It applies the inverse change to the working
files, which is what `git revert --no-commit` does to the files, except that we also leave the
index alone. **We do not create the revert commit:** that would write objects and move a branch in
the user's repository, which we never do. The person commits the result when they are satisfied.
Second, **agentic work happens only inside a git repository** (owner, 2026-11-12; designed in
`docs/nodes-credentials-environments.md` on another branch: new projects get an auto-initialised
repository with an initial commit, and there are no plain-folder agentic sessions). Today plain
folders still get shadow checkpoints that store everything; this note does not design for them.
Under that rule HEAD always exists and the shadow always borrows. So the note has no "unborn" or
"no repository" case, the HEAD message never degrades for lack of a repository, and A.17 always
applies.

**Summary.** Most of the direction is already built under other names: each turn's `before`
snapshot captures the person's edits made between turns, the pre-restore snapshot (A.9) is a
saved copy of the folder taken before every undo, borrowing keeps committed content out of the
shadow repository, and A.16 refuses a restore whose content is gone. What is new:

1. **Undo stays available after a commit, by anyone.** It restores the turn's working files and
   never touches history: no HEAD, branch, index or reflog moves, and no `git revert` or
   `git reset` runs. If a restored file is held by a commit made since, the result names the
   commit and says it is still in history.
2. **Undo no longer depends on the user's repository keeping old content (A.17).** At the end of
   each turn, the old versions of the files that turn changed are copied into the shadow's own
   store and kept with the turn's checkpoint.
3. **A stash.** An undo over the person's changes keeps exactly those files in a stash. A stash
   expires at the first local midnight after it is 30 days old, and can be deleted earlier by the
   person or by the agent.
4. **Unstash** is a three-way merge per file. Clean files go back with no further step. For a
   conflicting file, whoever unstashes chooses: skip it, write it beside as `name-1.py`, or have
   the agent merge it. When the agent unstashes, the session's approval mode decides how much it
   may choose by itself.
5. **A soft limit of 100 changed files per turn.** Above it, the end of the turn and its undo
   warn. It never blocks anything.

Terms used below:

- **Shadow:** the harness-owned git directory `<data>/checkpoints/<hash>.git` holding one
  folder's checkpoints (`crates/node/src/git.rs`, `repo_for_with`).
- **`before(T)`, `after(T)`:** snapshots of the folder (minus ignored files and nested
  repositories) at the start and end of agentic turn T.
- **Expected state:** what an undo assumes the folder holds now: `after(T)` for an undo, the
  latest recorded state for a rewind (`crates/node/src/session.rs`, `restore`).
- **`Hb(T)`, `Ha(T)`:** the commit the user's HEAD named at T's before and after.
- **Pin:** a pack of objects in the shadow's own store with a `.keep` file, so gc never drops it
  in favour of the user's copies.
- **Side file:** a stashed file written under a new name beside the original (§3.5).

Short paths: `git.rs` and `session.rs` are in `crates/node/src/`, `event.rs` and `view.rs` in
`crates/core/src/`. All refer to `main` as of 2026-11-12.

## 1. What exists, what is new, what changed

### 1.1 Built on `main`

| Piece | Where | What it does |
|---|---|---|
| Turn snapshots | `session.rs`, `checkpoint`, `checkpoint_after`; `git.rs`, `snapshot_full` | Before and after every agentic turn, a shadow commit of the folder's tree under `refs/harness/<session>/<turn>/{before,after}`. Plain folders too, for now. |
| Borrowing | `git.rs`, `repo_for_with` | The shadow reads the user's objects through `objects/info/alternates`. |
| Dropping duplicates | `git.rs`, `prune` (`gc --prune=now`); comment on `secure_blobs` | gc repacks with `-l`, so shadow copies of objects the user's repository has are dropped. |
| Pre-restore snapshot (A.9) | `session.rs`, `restore`; `CheckpointPhase::BeforeRestore` in `event.rs` | Every undo first saves the folder as `restore-<id>`; restoring that id undoes the undo. Pruned under `checkpoints_keep`. |
| Unforced check and `force` | `git.rs`, `restore_plan`; `MasterToNode::Restore` in `crates/protocol/src/lib.rs`; "Undo anyway" in `web/src/components/Conversation.tsx` | Refuses when a file the undo writes changed since the expected state; `force` skips the check. |
| All or nothing (A.16) | `git.rs`, `restore_diff`, `secure_blobs`, `worktree_tree` | The blobs a restore writes are checked and copied first; a missing one refuses the restore; a broken shadow index is rebuilt. |
| Nested repositories (A.15) | `git.rs`, `stage_without_nested`, `restore_plan` | Never snapshotted, never touched. |
| Busy folders | `session.rs`, `claim_folder`, `claim_turn` | No undo while another chat's turn runs in the folder, and no turn while an undo runs. |
| Periodic node tasks | `crates/node/src/lib.rs`, the loops started at node start (`artifacts::EXPIRE_EVERY`, `purge_incognito`) | Staged artifacts and idle incognito chats expire there. |

Not built: a chat button to undo an undo (`web/src/fold.ts` ignores `before_restore`), and any
record of the user's HEAD.

### 1.2 The owner's six points

| # | Point | State | Missing |
|---|---|---|---|
| 1 | The shadow holds only fresh edits; committed content is git's | **Already true as a storage rule** (§2.3). | Nothing, apart from A.17's pins, which deliberately keep old versions of changed files. |
| 2 | After each turn the shadow records the present state, including folders without a repository | **Already true.** | Nothing. Plain folders stop being a case once the repository rule is built. |
| 3 | The person's edits are tracked | **Partly.** Edits between turns are in `before(T)`; edits after the last turn are in the pre-restore snapshot. | Attribution: one tree diff `after(T-1)..before(T)`, no scan. Edits made during a turn count as the turn's (§4.4). |
| 4 | An undo over the person's changes asks about a stash; the agent can view one | **Partly.** Today: a refusal, then "Undo anyway" with a copy that is pruned after 10 changes and shown nowhere. | The question, a listed stash with a 30-day life, unstash, agent tools (§3). |
| 5 | Borrowing stays | **Already true.** | A.17 removes the main thing borrowing still cost undo (§2.4). |
| 6 | Undo stays available after a commit; unstash; conflict choices; 30-day expiry | **New.** | §2, §3. |

### 1.3 What changed since the first version of this note

- **The owner reversed "after a commit, undo is unavailable"** (2026-11-12). The per-file
  "committed since" rule, its gaps, its unavailability and its tests are gone. This is also the
  safer rule. Under the old rule, any agent could switch off undo for its own turn with
  `git commit`; the first version already said that undoing a committed file is safe, because it
  only leaves an uncommitted change and the commit stays in history.
- Former Q1 (agent commits) no longer arises. Q2 (retention) is answered with 30 days, ending at
  midnight. Q4 is answered: merge, with a choice per conflicting file. Q5 is decided yes, as
  A.17.
- Stashes can be deleted by the agent too, through the approval path.
- No "undo without a stash" choice (§3.3): the owner decided Q1 on 2026-11-12 ("do stash and
  undo").
- **Conflicts during the agent's unstash follow the approval mode** (owner, the second Q2;
  §3.4). This replaces "always ask a person".
- **The in-turn pruning gap is accepted, with detection** (the second Q3, decided by the
  coordinator on the owner's delegation; §2.4).
- **Undo after a commit is "like git revert"**, without creating the revert commit (see the top
  of this note).
- **Agentic work only inside a git repository.** Plain-folder cases are no longer designed for.
- **New: a soft limit of 100 changed files per turn** (§2.6).

### 1.4 How others do it

From `research/round2/synth/06-sessions-fork-checkpoints.md`, §2 and §3.3:

- **Cline** ("Restore: a transaction with a head guard and compare-and-swap",
  `checkpoint-restore.ts` L50-L154): refuses unless HEAD is the checkpoint's commit, then moves
  HEAD and runs `reset --hard`. We never move HEAD, and a moved HEAD does not stop an undo.
- **Zed** ("What replaced it"): edits the user commits in Git are auto-accepted. That was the
  owner's first rule; the owner has dropped it.
- **Hermes** ("Restore with a write ledger"): skips files the user edited, judged by a ledger of
  its own writes, so shell edits are missed. We ask, and judge by tree.
- **Pi** (example `git-checkpoint.ts`): `git stash create` in the user's repository. Our stash
  never touches the user's refs. **OpenCode** (§3.3 table) overwrites user edits.

No project in the input unstashes by merging, or offers skip, write-beside and merge for a
conflict.

## 2. Undo and commits

### 2.1 The rule (direction from the owner and coordinator, 2026-11-12)

An undo of turn T always puts the files the turn changed back to their `before(T)` content. It
does so whether or not those changes were committed since, by the agent or by the person.
**Undo, rewind and unstash never move HEAD, a branch, a tag, the index or the reflog, never run
`git revert` or `git reset`, and never write anything in the user's repository.** They write
working files only. This is already true on `main`: `git.rs`, `restore_paths` writes through a
private index in the shadow.

As the top of this note says, this is "like git revert" without the commit: the working files
get the inverse of the turn's change, and the commit stays in history. Git then shows the undo as
uncommitted changes, which the person commits, or not, when satisfied.

**What the person is told.** For each restored path p whose entry in HEAD now differs from p in
`Hb(T)`, the path is held by a commit made since. The note after the undo names the newest commit
in `Hb(T)..HEAD` that changed p, found with `rev-list -1 --full-history`, and says whether it was
made during the turn (reachable from `Ha(T)`) or later. Example: "Undo restored 3 files.
`src/a.py` is also in commit 1a2b3c4 ("Fix parser"), which the agent made during this turn; the
commit was left in history, so git now shows the undo as an uncommitted change." When the commit
cannot be found (HEAD no longer descends from `Hb(T)`, or history was pruned), the note says
"HEAD moved from X to Y since this turn". This costs one `diff-tree` limited to the restored paths,
plus one `rev-list` per path that is held by a commit. It runs in the shadow, at undo time only.

**Rewind** (this turn and every later one) follows the same rule, with `Hb` of the first rewound
turn.

### 2.2 Recording HEAD

The message needs HEAD, so each `before`, `after` and `before_restore` event gets a field `head`.
It holds a commit id, or `"unknown"` when HEAD is unreadable. Under the repository rule there is
always a commit. Until that rule is built, the field is absent in plain folders. This is a
**schema change** to `Checkpoint` (`event.rs`) and
`CheckpointState` (`view.rs`). It is made in place, before the first tag, together with
`PLAN.md`'s event table.

HEAD is read with one `git -C <root> rev-parse -q --verify HEAD^{commit}` under the hardened
base (`git.rs`, `hardened`: cleared environment, no system or global config,
`core.fsmonitor=false`, `core.hooksPath=/dev/null`, `protocol.allow=never`,
`GIT_NO_LAZY_FETCH=1`). It is the call `git.rs`, `describe` already makes. It reads `.git/config`,
`HEAD`, the ref store and one commit, and it runs no hook, filter, fsmonitor or transport. It is
the only new call in the user's repository: the trees of those commits are read in the shadow,
through the alternates.

### 2.3 The storage rule: the shadow holds only fresh edits

The shadow stores only content the user's repository does not hold (borrowing). After a commit,
the shadow's copies of the newly committed content are dropped at its next gc (`repack -l`). The
shadow's own store therefore shrinks to what is uncommitted. Whether loose duplicates are dropped
as well is not established (§9).

At 100,000 files, a committed repository's shadow is 9.7 MiB after the first snapshot with
borrowing and 478 MiB without. The first snapshot takes 1.9 s with borrowing and 5.7 s without
(B.3, "Disk" and "Snapshots"; slow disk, loaded machine). A.17's pins are the deliberate exception
to the rule: old versions of changed files only.

### 2.4 A.17: pin the old versions at the end of each turn (decided)

Undo must keep working after commits and after history is rewritten. So the content an undo
writes must not depend on the user's repository keeping it. Today it does: B.2 showed that a
rewrite plus pruning makes an older checkpoint unrestorable. A.16 makes that safe (refused, folder
untouched) but does not keep the undo working.

**What is pinned.** In `checkpoint_after`, after the `after` snapshot:

1. `git diff-tree -r -t --no-renames before(T) after(T)` lists the paths the turn changed. With
   `-t` it also lists the trees on those paths, giving the old-side and new-side ids.
2. The pin set is: those old-side blobs, the trees on the changed paths on both sides, and both
   root trees. The commits are always the shadow's own, so they are already in its store.
   - Old-side blobs are what the undo writes.
   - The trees are what `restore_diff` and the unforced check read. A tree equal to one the user
     committed is borrowed too.
3. `cat-file --batch-check` checks that every object in the set is present.
4. One `pack-objects` writes the present objects into `objects/pack/` with a `.keep` file. The
   pack is recorded against the turn's checkpoint group.
5. When `prune` drops that group, it deletes the pack and its `.keep`.
6. Skipped in today's plain folders, whose shadow already stores everything.

**What this fixes.** Once a turn is pinned, a later rewrite plus `gc --prune=now` in the user's
repository can no longer defeat that turn's undo. This holds whether the person or an agent does
it in a later turn. It also covers the rewind, because each turn pins its own changes. The
`before_restore` snapshot gets the same pin, so an undo of an undo survives as well. In the
restore, `secure_blobs` (A.16) mostly turns into a check, because the blobs are already local.

**What stays exposed** (said plainly, because the direction claimed more):

- **An agent that prunes inside its own turn.** The pin runs at the end of the turn. An agent
  that rewrites the user's history, expires the reflog and runs `gc --prune=now` during the turn
  destroys the borrowed old versions before they are pinned. Pinning cannot fix this; only not
  borrowing tracked content would. **Decided: borrowing stays, and the gap is accepted with
  detection.** Objects missing at step 3 produce a chat note saying which files of this turn can
  no longer be undone. The reasons:
  - The cost of not borrowing: about 10 MiB against 478 MiB of store, and 1.9 s against 5.7 s for
    a first snapshot, at 100,000 files (B.3).
  - The attack needs the agent to run history-rewriting and pruning commands through its shell.
    Those commands are in the log, and they need approval in the default mode.
  - The turn is flagged at its end with the affected files.
  
  A possible follow-up, not part of this design: our own `edit` and `write` tools know which file
  they are about to change, so they could copy its old version before writing, with no scan.
  Changes made through the shell would still be covered only at the end of the turn.
- **Turns older than `checkpoints_keep`.** Their checkpoints and pins are gone, as today.
- **Files the turn did not change.** They are not pinned. Their undo is not needed, because undo
  writes only changed paths.

**Cost.** Pinning adds one `diff-tree` (B.3 puts `changed`, a tree diff, at 3 to 4 ms), one
`cat-file --batch-check` and one `pack-objects`, and only on turns that changed something.
- **Disk:** the compressed old versions of the changed files, which is tiny next to 478 MiB.
- **Time:** in the restore, B.3 measured the same `pack-objects` step through the alternates at
  170 to 400 ms, even for one file ("Restore"). A restore of 10 files costs 0.5 to 1.3 s in a
  repository folder. Pinning moves most of that cost from the undo to every changing turn.
- **Scheduling:** to keep it off the turn's visible end, the pin runs after `turn_ended` is
  written. The session's next turn and any restore wait for it.

A.17 is a fix to built behaviour, security code, and its own task that may land before Gate 3
(§7, `ADOPTION_PLAN.md` A.17).

### 2.5 Folders without a repository

Until the repository rule is built, plain folders keep today's undo: there is no HEAD, no
borrowing and no pin. After it, they are no longer a place for agentic sessions.

### 2.6 A soft limit of 100 changed files per turn (owner's rule)

**What is counted.** The files a turn changed (added, modified or deleted), as the `after`
checkpoint already lists them: `changed(before, after)` into `Checkpoint.files`
(`session.rs`, `checkpoint_after`).
- Nested repositories and ignored files are not in that list, so they are not counted.
- A sub-agent's changes count toward the turn that applied them, because they are in the folder
  when that turn's `after` is taken.

**Above 100: a warning, never an error, never a block.** The turn and its undo complete normally.
The warning appears in two places:
- **At the end of the turn:** "This turn changed 143 files, more than the recommended 100; undoing
  it will affect 143 files."
- **On the undo of such a turn:** "This undo affects 143 files." It is shown on the undo card if
  there is one, and otherwise in the note after the undo.

**Channel.** No new event type. The `after` checkpoint already carries the file list, so the UI
derives the count from it and shows the warning under the turn's undo row. The node also writes
the end-of-turn sentence as the existing chat note (an `error` event with the turn id, the channel
used today for "This turn has no checkpoint", `session.rs`, `no_checkpoint_notice`). A log read
without the UI then says so too. The undo plan returned by `changes: "ask"` carries the count.
The UI needs the count and the warning text beside the undo row and on the undo card; no screens
are designed here.

**The number.** It is the owner's number. It lives in one named constant,
`RECOMMENDED_MAX_FILES_PER_TURN = 100`, in `crates/core/src/policy.rs`, and the node, the UI's
text and the agents' rule all read that constant. **Recommendation: no `node.toml` setting now**;
one can come when someone needs it.

**What it is for.** A large turn is hard to review, and its undo is heavy. B.3 measured an undo of
10 files at 0.5 to 1.3 s in a repository folder and of 1,000 files at 1.2 to 2.1 s (B.3,
"Restore"; slow disk). So the time an undo takes grows slowly. The real cost of a big turn is the
review, and the chance that the undo meets the person's changes in many files.

**The models are told (a commitment for later work).** When the work on model instructions and
orchestration starts (`ADOPTION_PLAN.md` X.2), the instructions of every agent that can change
files gain this sentence. That is every agent whose effective tools include `write`, `edit` or
`bash`, the set `Session::isolate` already computes (T32). The sentence:

> "Try to keep the files you change in one turn to at most 100; split larger work across turns.
> Going over is allowed when the task needs it; say so when you do."

No prompt is edited now.

## 3. The stash

### 3.1 The object (recommendation)

**Contents.** The stash holds the paths the undo writes **and** whose content differs from the
expected state. For each, it holds the whole file just before the undo (the person's version) and
the same path in the expected state (the merge base). A deletion by the person is an absent file.

It does not hold the whole folder: the undo does not touch the other paths. The direction said
the pre-restore copy "becomes" the stash; that is true only of the person's changed files. The
A.9 snapshot of the whole folder is still taken, still pruned under `checkpoints_keep`, and still
what undoes an undo. The person's own edits, which today last only ten further changes, now last
30 days in the stash.

**Place.** A commit in the shadow at `refs/harness-stash/<session>/<stash-id>`.
- It sits outside `refs/harness/`, so `prune` (`for-each-ref refs/harness/`) never sees it, and gc
  keeps what it reaches.
- Its tree holds the stashed files; its parent's tree holds the base versions.
- Its blobs are pinned in a pack of their own, so the stash is self-contained and survives a
  history rewrite.
- Trailers name the turn, the rewound turns, HEAD and the expiry instant.

**Shown as.** "Stash of 12 Nov 14:03, before undoing *Fix the parser*, 3 files, 18 KB, expires
13 Dec 00:00". The id `stash-<id>` is used by the API and the tools.

**Event.** A new kind `stash { stash_id, phase: created | applied | deleted, turn_id, files, bytes,
expires_at, by, reason }`. It carries paths and sizes only, never content. `reason` is
`person | agent | expired | empty | chat_deleted`. It is folded into `SessionView.stashes`. This is
a **schema change**; the UI is a fold over the log, so a stash must be in it.

### 3.2 Lifetime: 30 days, ending at midnight (owner's decision)

**Rule.** A stash expires at the first local midnight (00:00) that falls at or after its creation
plus 30 × 24 hours. It always lives at least 30 days and always disappears at 00:00, never in the
middle of a day. In a time zone that skips 00:00 at a daylight-saving change, "midnight" is the
first instant of that local day.

**Clock.** The node holds the stash, so the node computes the instant once, at creation, from its
own clock and time zone. It is stored as an absolute instant in the event and the commit trailer,
so a later time-zone change on the node does not move it.
- The calculation uses `jiff`, already a workspace dependency (`crates/master/Cargo.toml`,
  `crates/memory/Cargo.toml`), with the system time-zone database. Without one it falls back to
  UTC, and the list says so.
- A system clock set wrong makes the stash expire early or late.

**Removal.** A check runs at node start and then every five minutes, as a loop beside the
incognito purge in `crates/node/src/lib.rs`.
- It removes every expired stash: ref, pin pack and `.keep`. It records `stash{deleted,
  reason: expired}` through the owning session's actor (one writer per session), plus a note in
  the chat.
- A node that was off at that midnight removes the stash at its next start.
- **An expired stash is never usable, even if it is still on disk:** every list, show and apply
  checks `expires_at` first and refuses.

**What the person sees.** Every stash shows its expiry date; a stash removed by expiry leaves a
note in the chat.

**Said plainly:** this deletes someone's saved edits without their action after 30 days. It is
the owner's decision. Earlier deletion:
- The person deletes from the list, with one confirmation.
- The agent deletes through a tool (§3.6).
- Deleting a chat deletes its stashes; the confirmation counts them.
- An incognito chat's stashes go with it (`purge_incognito`).

### 3.3 The question at undo (Q1, decided by the owner on 2026-11-12)

The node plans first. If nothing the undo writes changed since the expected state, the undo runs
at once, as today. Otherwise nothing is written, and the node returns the plan:
- the paths it will undo;
- the changed paths that will be stashed;
- the paths held by a commit made since (§2.1);
- files that would be deleted and cannot be kept (`RestorePlan.unsaved`: ignored files in a
  folder the undo replaces by a file).

The choices:

| Choice | Label | Effect |
|---|---|---|
| Stash | "Undo and keep my changes in a stash for 30 days" | Builds and records the stash, then undoes. Any unsaved files are listed on the card as deleted. |
| Cancel | "Cancel" | Nothing written; the snapshot ref is deleted, as on any refusal today. |

**Why no "undo without a stash".** With a stash that expires anyway, dropping saves nothing: the
check needs the scan in any case (`session.rs`, `restore`). Dropping would only make a misclick
final, and the person can delete the stash a second later.

**Mechanism.** The existing refuse-then-retry round trip, not the approval path.
`ApprovalRequested` belongs to a tool call in a running turn and is denied when the turn ends
(`session.rs`, `end_turn`); an undo runs with no turn.
- `session.restore` replaces `force: bool` with `changes: "ask" | "stash"` (absent means `ask`)
  and `seen`, a hash of the plan's inputs (paths, blob ids, HEAD) that the ask call returned.
- The answer re-plans from a fresh snapshot and asks again if the hash differs. A restart between
  question and answer loses nothing, because the node keeps no state between the two calls.
- This is a **protocol change** (`crates/protocol/src/lib.rs`) and an HTTP body change
  (`crates/master/src/http.rs`, `restore_turn`).
- With no person present (an API call, a script), a call that does not choose gets the plan and
  changes nothing. Routines have no undo tool. Never a silent choice.

**Order.** Snapshot, plan, `secure_blobs`, stash commit and pin, stash ref, `before_restore`
event, `stash{created}` event, writes, `after`, `restored`. If an append fails before the writes,
the stash ref and pin are deleted and nothing is written.

### 3.4 Unstash

A stashed file is the agent's change with the person's edits on top. Writing it back whole would
bring the undone change back. So unstash stays a **three-way merge per file**:
- base B: the file as the turn left it, from the stash's parent;
- current C: the file now;
- stashed S: the file in the stash.

**The merge.** It runs `git merge-file -p` on three temporary files in a private directory under
the data directory, with `--git-dir <shadow>` and the hardened base.
- `merge-file` calls git's merge code directly. It uses no merge driver and no attributes, and it
  reads only `merge.conflictStyle` from config, which here is the harness's.
- Test 12 proves this with a hostile `merge.<x>.driver` and `.gitattributes`.
- The result goes into the shadow with `hash-object -w`.

**Writing.** Unstash claims the folder (`claim_folder`) and first takes the same pre-write
snapshot as a restore, so it can itself be undone. It writes through a private index and
`checkout-index`, as `git.rs`, `restore_paths` does: no symlink in a leading path is followed, and
no filter applies.

| Case | Result |
|---|---|
| S equals B, or C equals S | Nothing to write; the file leaves the stash. |
| C equals B | S written in place. |
| Text, clean merge | Merge written in place: the person's edit kept, the agent's change still undone. |
| Text with conflicts; binary (merge-file refuses); a symlink that differs | **Conflict.** |
| C absent (deleted since, or the undo removed a file the turn created) | Conflict: S carries the turn's content. |
| B absent (the turn deleted the file, the person recreated it), C present and not equal to S | Conflict. |
| S absent (the person had deleted the file) | Reported, never applied: unstash never deletes a file. The entry leaves the stash. |

**No conflict is seamless (owner's decision).** A clean file is written with no step beyond the
unstash itself. For the person, pressing unstash is the approval. For the agent, the tool call's
normal approval covers the clean files.

**A conflict is a choice per file**, made by whoever is unstashing:

1. **Skip.** Nothing is written; the file stays in the stash.
2. **Beside.** The stashed version is written next to the file under the name rule in §3.5
   (`filename.txt` becomes `filename-1.txt`). Nothing is written over an existing entry, and no
   conflict markers go into the person's file. If C is absent, nothing is there to protect, so S
   is written under its own name.
3. **Merge by the agent.** This is not a mechanism of its own.
   - The agent reads B, C and S through `stash_show` and writes a merged file with its ordinary
     `edit` or `write` tool.
   - That write goes through the ordinary approval and is checkpointed like any change, so the
     person can undo it.
   - The file stays in the stash until it is deleted or expires; the harness cannot tell when the
     agent's merge is "done".

The side file is the person's whole stashed file, so it contains the agent's change too. That is
what the person had, and they take lines from it.

**When the agent unstashes and hits a conflict, the session's approval mode decides** (owner,
2026-11-12). The owner's names for the modes differ from the code's (`ApprovalMode` in
`crates/core/src/policy.rs`):

| Owner's name | Code | Who decides a conflict |
|---|---|---|
| "Always allow" | `Auto` (nothing needs approval) | The agent, by itself: skip, beside or merge. |
| "Auto, if safe" | `Ask`, the default (mutating tools need approval) | The agent may choose the **safe** outcomes by itself: **skip** and **beside**, since neither changes or overwrites an existing file. **Merging** changes the person's file, so it asks, exactly as `edit` does in this mode. |
| "Manual" | `AskAll` (every tool call needs approval) | The person decides every conflict on one card listing the conflicting files, each with the three choices and its side-file name. |

The node's own policy caps the mode, as it does for every tool (`ApprovalMode`: the stricter of
the requested mode and the node's). A card carries `seen`, so it applies only to the plan it
showed.

The first version of this note rejected letting "the person already said which they want" in
chat decide a conflict, because the node cannot verify text that the model relays. The mode rule
does not need that, and chat text still decides nothing.

**With no person present** (a routine): under `Ask` and `AskAll`, conflicts are skipped and
reported, and nothing is written beside or merged. Under `Auto`, the agent decides.

**Afterwards.** Files written in place or beside leave the stash, and so do files with nothing to
write. Skipped files stay. The stash ref moves to a commit without the files that left, and the
pin is repacked to match. A stash with nothing left is removed (`reason: empty`).

### 3.5 Side-file name

The rule works on the last path component N, as bytes:

1. The extension starts at the last `.` that is neither N's first byte nor its last. The stem is
   what precedes it. With no such dot, the stem is N and the extension is empty.
2. Candidate k is `stem-k` followed by the extension, for k = 1 to 999.
3. The first candidate wins if nothing exists at that name and no other side file of the same
   unstash takes it.
   - The check uses `lstat`, so a file, a directory, a symlink and a dangling symlink all count. On
     a case-insensitive file system it matches any case.
   - `checkout-index` runs without `-f`, so a name that appears meanwhile fails the write, and the
     unstash is planned again.
4. A name over 255 bytes is shortened by cutting the stem at a character boundary (a byte
   boundary if the name is not UTF-8). That file is refused, with the reason, if the stem would be
   empty, the path would exceed 4,096 bytes, or k passes 999.
5. The stashed mode is kept: the executable bit, and a symlink stays a symlink.

| Name | Side file |
|---|---|
| `filename.txt` | `filename-1.txt`, then `filename-2.txt` |
| `archive.tar.gz` | `archive.tar-1.gz` |
| `Makefile` | `Makefile-1` |
| `.env` | `.env-1` |
| `.eslintrc.json` | `.eslintrc-1.json` |
| `name.` | `name.-1` |
| `name-1.py` (itself conflicting) | `name-1-1.py` |

The last dot keeps the final extension, so editors still know the file type. The cost is
`archive.tar-1.gz` for a double extension.

### 3.6 The agent's tools

| Tool | Risk | What it does |
|---|---|---|
| `stash_list` | read-only | This chat's unexpired stashes: id, date, turn, files, size, expiry. |
| `stash_show` | read-only | One file of a stash as base, stashed or current version, or a diff between two of them. Same size limits as `read`. |
| `stash_apply` | mutating, approval | Unstash files of one stash, exactly as §3.4; conflicts as the approval mode decides. |
| `stash_delete` | mutating, **always asks** | Delete one stash. The card says what is lost: how many files, from when, the turn. It cannot be undone. Unlike other mutating tools it asks the person in every approval mode, `Auto` included, and is refused where no person can answer: a stash is the person's own saved work, and under `Auto` an ordinary mutating tool would delete it unasked. If the person does not answer within **60 seconds** the request is denied automatically and the stash is kept; the agent is told it timed out and may ask again. (Confirmed by the owner on 2026-11-12, who added the 60-second timeout.) |

- The tools reach only the stashes of the agent's own chat, in its folder.
- **No tool creates a stash or changes its contents.** Only an undo the person chose creates one.
- Both mutating tools go through the approval path like `write` (`TurnHost::approve`,
  `crates/core/src/backend.rs`).
  - A conflict card, when the mode asks for one (§3.4), has `turn_allow` false.
  - Whether the fold and the UI accept two approvals under one `call_id` is not established. If
    they do not, the second approval gets a derived id.
- The shadow is in the data directory, and the agent cannot reach it there by other means.
  - The file tools refuse paths in it (`crates/tools/src/lib.rs`, `refuse_protected`).
  - The enforced sandbox gives the shell no access to it (`crates/tools/src/sandbox.rs`, module
    comment and `exposes`).
  - With `[sandbox] mode = "off"`, or best-effort without Landlock, the shell can reach it, as it
    can every checkpoint today (`PLAN.md` R2).
- A stash holds text anyone may have written. `stash_show` carries the same risk as `read`, and
  no more.
- The tools are served through `TurnHost` and implemented by `Session`, as `delegate` is.

## 4. What changes in the code

### 4.1 Existing pieces

| Piece | Verdict | How |
|---|---|---|
| `before`/`after` snapshots | Change | Add `head`; `after` adds the A.17 pin. Optionally `before.files` lists the person's edits since `after(T-1)`. |
| `before_restore` (A.9) | Stays | Taken for every restore and every unstash, pinned, and pruned under `checkpoints_keep`. |
| `restored` and the note after an undo | Stay, extended | The note names commits that hold restored files and the stash that was made (`session.rs`, `restore_notice`). |
| Unforced check (`restore_plan`) | Changes | Becomes the plan returned for `changes: "ask"`, with `seen`. |
| `force` | Goes | Replaced by `changes`. |
| `secure_blobs` (A.16) | Stays | Mostly a check once turns are pinned; the copy remains for unpinned (older) checkpoints. |
| Index rebuild, nested-repository exclusion, interrupted-restore path, `claim_folder` | Stay | Unstash also claims the folder. |
| `checkpoints_keep` pruning (`git::prune`) | Changes | Also deletes the pins of dropped groups. Stash refs are outside `refs/harness/`. |
| Borrowing | Stays | Neither undo of a pinned turn nor a stash relies on it. |

### 4.2 New pieces

| Piece | Where |
|---|---|
| A.17 pin per turn and per restore; pin deletion in `prune` | `session.rs`, `checkpoint_after`, `restore`; `git.rs` |
| HEAD per checkpoint; "held by a commit made since" in the note | `git.rs`, `session.rs`, `event.rs`, `view.rs`, `web/src/fold.ts` |
| Stash commit and pin, list, delete, shrink on apply; `stash` event and fold | `git.rs`, `session.rs`, `event.rs`, `view.rs`, `web/src/fold.ts` |
| Expiry: instant at creation, check at start and every 5 minutes | `crates/node/src/lib.rs`, beside the `purge_incognito` loop |
| `changes`/`seen` on `session.restore`; `session.stash.apply`, `.delete`, `.show` | `crates/protocol/src/lib.rs`, `crates/node/src/lib.rs`; HTTP in a new `crates/master/src/http/stashes.rs` |
| Merge, conflict choices, side-file naming (as a pure function) | `git.rs`, `session.rs` |
| `stash_list`, `stash_show`, `stash_apply`, `stash_delete` | `crates/tools`, `TurnHost` |
| Soft limit: `RECOMMENDED_MAX_FILES_PER_TURN`, end-of-turn note, count in the undo plan | `crates/core/src/policy.rs`, `session.rs`, `web/` |

Gone from the first version: the "committed since" test and its gaps.

**What the UI needs** (no screens designed here):
- the undo card with its two choices;
- a per-chat stash list with expiry dates;
- a stash file view (a diff against the base);
- unstash with file selection, and a conflict card offering the three choices per file;
- delete with confirmation;
- the stash total per folder;
- stashes counted in the chat delete confirmation.

### 4.3 Cost per turn

From B.3 ("A whole turn"; slow disk, loaded machine, warm cache only): a turn's checkpoints
without gc cost about 30 ms at 1,000 files, 70 ms at 10,000 and 600 ms at 100,000.
- `prune` runs `git gc --prune=now` on every turn once ten checkpoints exist. It is the largest
  cost of a turn: 0.26 s at 1,000 files and 3.3 s at 100,000 in a plain folder. That is being
  fixed as its own task.
- **The additions proposed here must not add to that cost and must not run gc.**
  - Recording HEAD is one `rev-parse` that reads no file of the folder: milliseconds.
  - Pinning (§2.4) adds `diff-tree`, `cat-file` and `pack-objects` on changing turns only, about
    170 to 400 ms by B.3's `secure_blobs` figure. It runs after `turn_ended`, and the restore
    saves most of it in return.
  - Stash creation adds a `pack-objects` of the stashed files only at undo time.
- Unstash adds one folder snapshot: B.3's unchanged repeat is 11 ms at 1,000 files, 25 ms at
  10,000 and 166 to 201 ms at 100,000.
- "Held by a commit since" runs only at undo.

### 4.4 What this does not fix

- **A restore is not atomic across files** (a T33 residual), and unstash shares this. Both name
  the pre-write checkpoint on failure.
- **Uncommitted work inside a nested repository** is in no checkpoint and in no stash.
- **Two chats in one folder.** Chat B's agent edits look like the person's edits to chat A's undo.
  They are stashed under A, unseen by B's agent. Turns are still not serialised.
- **The person's edits made during a turn** are part of `after(T)`, so undo overwrites them
  without asking. They survive only in the pre-restore snapshot, under `checkpoints_keep`.
  Fixing that needs per-write attribution, which the shell does not give.
- **An agent that prunes inside its own turn** (§2.4) is detected and reported, but not
  prevented. This is accepted.
- **Plain folders** are no longer a supported place for agentic sessions once the repository rule
  is built. Until then they keep today's undo, with no pin.

## 5. How it breaks

| Case | What happens |
|---|---|
| An injected agent commits its change | Undo restores the files; HEAD stays; the note names the commit as made during the turn. The attack fails. |
| The agent amends or rebases the person's commits | The files are restored; HEAD is not moved back. The note gives the old HEAD id from the harness's log, which outlives `reflog expire` (the commit object may not). The undo works; restoring the history is git's job, and the agent's rewrite of it remains. |
| The agent runs `git stash` or `git checkout <branch>` | The file changes are the turn's and are undone. The agent's stash entry and the branch switch stay; the note says HEAD moved and from where. |
| The agent rewrites history and prunes **in a later turn**, or the person does | Fails against kept turns: pinned (A.17). |
| The agent rewrites history and prunes **inside its own turn** | **Remains, accepted:** the borrowed old versions are gone before the pin. The commands are in the log and need approval in the default mode, and A.17 reports which files cannot be undone. |
| A background process that outlives its turn commits | Fails: commits no longer matter. If it prunes after the turn, the pin holds. If it edits files, its edits look like the person's and are stashed. C.11 is still needed for other reasons. |
| The agent deletes a stash through its tool | Goes through the approval path like any mutating tool. It asks under `Ask` and `AskAll`, and the card says what is lost. Under `Auto` it runs without asking, as `bash` does. |
| The agent deletes stashes or checkpoints through the shell | Unreachable with the enforced sandbox and the file tools (§3.6). Reachable with the sandbox off, an existing residual. |
| The person commits, then changes their mind | Undo works as always; git handles the commit. |
| History rewritten after a stash was taken | No effect: the stash is pinned. |
| A restart between question and answer | Nothing is lost; the answer re-plans (§3.3). |
| The node is off at a stash's expiry midnight | Removed at the next start. Unusable from the expiry instant on. |
| A very large stash | Costs its files' compressed size for up to 30 days, shown in the list and the total. Building it is bounded by the 60 s snapshot limit (`git.rs`, `SNAPSHOT_TIMEOUT`). |
| A stash holding a secret (an edited `.env`) | File content on the node's disk, with the same exposure as the checkpoint store, for up to 30 days. Events carry paths and sizes only. |
| A routine, with no person | No undo tool. `stash_apply` follows the routine's approval rules (O6); `stash_delete` is refused, because it always needs a person. Conflicts are skipped under `Ask` and `AskAll`; under `Auto` the agent decides. |
| A turn that changes 5,000 files | Completes. It warns at its end and on its undo (§2.6), and its undo completes too. |
| Hostile repository config | The only new call in the user's repository is `rev-parse`, which runs nothing; the merge runs in the shadow. |

## 6. Tests the build is committed to

1. `undo_after_the_person_committed_restores_the_files_moves_no_ref_and_names_the_commit`: HEAD,
   branches, the index file and the reflog are byte-identical; the files are back to `before`; the
   note names the commit as made later.
2. `undo_after_the_agent_committed_in_its_turn_does_the_same`, with the commit named as made
   during the turn.
3. `undo_of_a_kept_turn_works_after_history_was_rewritten_and_pruned` (A.17): every file is
   restored; the pin holds only the turn's changed files; nothing is written in the user's
   repository.
4. `a_turn_that_pruned_its_own_base_says_which_files_cannot_be_undone`.
5. `a_stash_is_gone_at_the_first_midnight_after_30_days_and_not_before`, with a fixed clock and
   time zone, including a daylight-saving change.
6. `an_expired_stash_cannot_be_listed_shown_or_applied_even_if_still_on_disk`.
7. `a_stash_survives_pruning_and_a_history_rewrite_until_it_expires`.
8. `declining_the_question_changes_nothing`: folder, shadow refs and log as before; no stash.
9. `a_folder_changed_between_question_and_answer_is_asked_again`.
10. `a_clean_unstash_asks_nothing_further_and_does_not_bring_the_agents_change_back`.
11. `a_conflict_offers_skip_beside_and_merge`: skip writes nothing; beside writes `name-1.py` equal
    to the stash and leaves `name.py` byte-identical; merge leaves the file in the stash and
    changes nothing until the agent's own `write`.
12. `no_git_call_in_undo_or_unstash_runs_a_hook_or_reads_user_config`: hooks, fsmonitor, filters,
    a merge driver named in `.gitattributes` and a global config set as markers; none fires.
13. `side_file_names_follow_the_rule`: §3.5's table, taken names, 255-byte and non-UTF-8 names, a
    dangling symlink in the way.
14. `the_agent_reads_and_deletes_only_its_own_chats_stashes_and_cannot_create_or_change_one`.
15. `the_agents_delete_asks_in_every_approval_mode_and_names_what_is_lost` (`Auto` included).
    `an_unanswered_delete_request_is_denied_after_60_seconds_and_the_stash_is_kept`.
16. `the_agents_unstash_follows_the_approval_mode_on_conflicts`:
    - `Auto` decides without a card.
    - `Ask` allows skip and beside, and asks for a merge.
    - `AskAll` asks for every conflict.
    - A routine under `Ask` skips the conflicts.
17. `a_stash_with_nothing_left_is_removed`.
18. `a_turn_that_changes_100_files_warns_nobody`.
19. `a_turn_that_changes_101_files_warns_at_its_end_and_on_its_undo`: both the turn and the undo
    complete normally.
20. `the_file_count_ignores_nested_repositories`.

## 7. Build order and size

S means a day or less, M a few days, L a week or more, for one person with an agent.

| # | Task | Size | When |
|---|---|---|---|
| 1 | A.17: pin old versions at the end of each turn and restore; report pruned bases; `prune` deletes pins | S to M | **A fix to built behaviour: may land before Gate 3** |
| 2 | Record HEAD; "held by a commit made since" in the note | S | After Gate 3 |
| 3 | Stash object, pin, `stash` event, expiry loop, `changes`/`seen`, the question | M | After 2 |
| 4 | Unstash by the person: merge, conflict choices, side files | M | After 3 |
| 5 | Agent tools (four) | S | After 4 |
| 6 | Web UI | M | With 3 to 5 |
| 7 | Soft limit of 100 files (`ADOPTION_PLAN.md` T.9) | S | After Gate 3 |
| 8 | `PLAN.md` T33 and event table, `README.md` undo section | S | With each task |

Together about **L** (two to three weeks). It changes built behaviour and adds surface, so it
waits for Gate 3, except task 1.

## 8. Questions for the owner, and the answers

| # | Question | Recommendation | Answer (2026-11-12) |
|---|---|---|---|
| Q1 | When an undo meets the person's changes, offer only "undo and keep my changes in a stash for 30 days" and "cancel"? | **Yes.** Dropping saves nothing and makes a misclick final; the stash can be deleted a second later. | **Yes** (decided by the owner on 2026-11-12: "do stash and undo"). Two choices: "undo and keep my changes in a stash for 30 days" and "cancel". |
| Q2 | When the agent unstashes and hits a conflict, who decides? | A person's card, always. | **By the session's approval mode** (§3.4). |
| Q3 | An agent that prunes history inside its own turn defeats that turn's undo: accept, with detection? | Accept, with A.17's report. | **Accepted** (decided on the owner's delegation; §2.4). |

## 9. Not settled

- Whether gc drops loose shadow objects that the alternate also has (§2.3), and that `.keep`
  holds a pin through `repack -l` (tests 3 and 7).
- Whether two approvals under one `call_id` fold and render (§3.6).
- The exit status of `checkout-index` without `-f` on an existing file (tests 11 and 13).
- Pin times on an SSD or an idle machine: B.3 measured neither.
- If the owner wants the revert commit created automatically after an undo, that is a different
  rule. It needs their explicit yes, because it writes into the user's repository.
