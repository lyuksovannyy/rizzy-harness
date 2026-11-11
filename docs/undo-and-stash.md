# Undo and stash: design note

Status: **proposal, nothing built.** Task C.13 of `ADOPTION_PLAN.md`, answering decision O11.
For after Gate 3 of `FIX_PLAN.md` (feature freeze), except the pure fix named in §7. The
direction is the owner's (2026-11-11 and 2026-11-12); every other decision is marked as a
recommendation awaiting the owner's yes. Experiment B.3 has not published its numbers
(`exp/B.3-checkpoint-cost` has no `docs/checkpoint-cost.md`), so costs that need them say "to be
filled from experiment B.3".

**Summary.** Most of the direction is already built under other names: each turn's `before`
snapshot captures the person's edits made between turns, the pre-restore snapshot (A.9) is a
saved copy of the folder taken before every undo, borrowing keeps committed content out of the
shadow repository, and A.16 refuses a restore whose content is gone. Three things are new.
(1) **A commit ends undo, per file:** a file whose committed content changed after a turn is left
to git, and the result names the commit. Undo never moves HEAD, a branch, the index or the
reflog. (2) **A stash:** when the person changed files an undo would overwrite, they are asked;
"stash" keeps exactly those files, outside checkpoint pruning, until the person deletes them.
(3) **Unstash** is a three-way merge per file; a conflict needs a second approval and then writes
the stashed file beside the original as `name-1.py`, never over anything. The agent can list and
read its own chat's stashes and unstash through the approval path; it can never create, change or
delete one.

Terms used below:

- **Shadow:** the harness-owned git directory `<data>/checkpoints/<hash>.git` holding one
  folder's checkpoints (`crates/node/src/git.rs`, `repo_for_with`).
- **`before(T)`, `after(T)`:** snapshots of the folder (minus ignored files and nested
  repositories) at the start and end of agentic turn T.
- **Expected state:** what an undo assumes the folder holds now: `after(T)` for an undo, the
  latest recorded state for a rewind (`crates/node/src/session.rs`, `restore`).
- **`Hb(T)`, `Ha(T)`:** the commit the user's HEAD named at T's before and after.
- **Gap:** an interval in which no turn of this chat runs: `Ha(T)` to `Hb(T+1)`, and `Ha(last)`
  to now.
- **Side file:** a stashed file written under a new name beside the original (§3.4).

Short paths: `git.rs` and `session.rs` are in `crates/node/src/`, `event.rs` and `view.rs` in
`crates/core/src/`. All refer to `main` as of 2026-11-12.

## 1. What exists and what is new

### 1.1 Built on `main`

| Piece | Where | What it does |
|---|---|---|
| Turn snapshots | `session.rs`, `checkpoint`, `checkpoint_after`; `git.rs`, `snapshot_full` | A shadow commit of the folder's tree before and after each agentic turn, `refs/harness/<session>/<turn>/{before,after}`; plain folders too. |
| Borrowing | `git.rs`, `repo_for_with` | The shadow reads the user's objects through `objects/info/alternates`. |
| Dropping duplicates | `git.rs`, `prune` (`gc --prune=now`); comment on `secure_blobs` | gc repacks with `-l`: shadow copies of objects the user's repository has are dropped. |
| Pre-restore snapshot (A.9) | `session.rs`, `restore`; `CheckpointPhase::BeforeRestore` in `crates/core/src/event.rs` | Every undo first saves the folder as `restore-<id>`; restoring that id undoes the undo. Pruned under `checkpoints_keep`. |
| Unforced check and `force` | `git.rs`, `restore_plan`; `MasterToNode::Restore` in `crates/protocol/src/lib.rs`; "Undo anyway" in `web/src/components/Conversation.tsx` | Refuses when a file the undo writes changed since the expected state; `force` skips that. |
| All or nothing (A.16) | `git.rs`, `restore_diff`, `secure_blobs`, `worktree_tree` | Blobs to write are checked and copied first; a missing one refuses; a broken shadow index is rebuilt. |
| Nested repositories (A.15) | `git.rs`, `stage_without_nested`, `restore_plan` | Never snapshotted, never touched. |
| Busy folders | `session.rs`, `claim_folder`, `claim_turn` | No undo while another chat's turn runs in the folder, and the reverse. |

Not built: a chat button to undo an undo (`web/src/fold.ts` ignores `before_restore`), any record
of the user's HEAD, any notion of a committed turn.

### 1.2 The owner's six points

| # | Point | State | Missing |
|---|---|---|---|
| 1 | The shadow keeps only what is not committed; after a commit those files go back with git | **Partly.** Blob storage already works this way (borrowing plus `repack -l`); trees are always local and small. | The meaning: committed files stop being undoable (§2). Nothing has to be deleted to save space. |
| 2 | After each turn the shadow records the present state, folders without a repository included | **Already true.** | Nothing. |
| 3 | The person's edits are tracked | **Partly.** Edits between turns are in `before(T)`, edits after the last turn in the pre-restore snapshot. | Attribution (one tree diff `after(T-1)..before(T)`, no scan). Edits made during a turn count as the turn's (§4.4). |
| 4 | An undo over the person's changes asks whether to stash; the agent can view a stash | **Partly.** Refusal, then "Undo anyway" with a copy in the pre-restore snapshot, pruned after 10 changes and shown nowhere. | The question, a kept and listed stash, unstash, agent tools (§3). |
| 5 | Borrowing stays | **Already true.** | Its remaining cost: §5, first rows. |
| 6 | After a commit undo is unavailable; unstash by the person or the agent; a conflict needs another approval and returns as `filename-1.py` | **New.** | §2, §3.4, §3.5. |

### 1.3 How others do it

From `research/round2/synth/06-sessions-fork-checkpoints.md`, §2 and §3.3:

- **Zed** ("What replaced it"): edits the user commits in Git are auto-accepted, the owner's rule;
  but its log knows only tool edits and is lost on restart.
- **Cline** ("Restore: a transaction with a head guard and compare-and-swap",
  `checkpoint-restore.ts` L50-L154): refuses unless HEAD is the checkpoint's commit, then moves
  HEAD and runs `reset --hard`. We never move HEAD and decide per file.
- **Hermes** ("Restore with a write ledger"): skips files the user edited, judged by a ledger of
  its own writes, so shell edits are missed. We ask, and judge by tree.
- **Pi** (example `git-checkpoint.ts`): `git stash create` in the user's repository. Ours never
  touches the user's refs. **OpenCode** (§3.3 table) overwrites user edits.

No project in the input unstashes by merging or returns a conflict as a side file.

## 2. "Committed" and what it does to undo

### 2.1 Recording HEAD

Each `before`, `after` and `before_restore` event gets a field `head`: a commit id, `"unborn"`
(no commit yet) or `"unknown"` (unreadable); absent outside a repository. **Schema change** to
`Checkpoint` (`crates/core/src/event.rs`) and `CheckpointState` (`crates/core/src/view.rs`), in
place before the first tag, with `PLAN.md`'s event table.

It is read with one `git -C <root> rev-parse -q --verify HEAD^{commit}` under the hardened base
(`git.rs`, `hardened`: cleared environment, no system or global config, `core.fsmonitor=false`,
`core.hooksPath=/dev/null`, `protocol.allow=never`, `GIT_NO_LAZY_FETCH=1`), the call `git.rs`,
`describe` already makes. It reads `.git/config`, `HEAD`, the ref store and one commit, and runs
no hook, filter, fsmonitor or transport. It is the only new call in the user's repository: the
trees of those commits are read in the shadow, through the alternates (in the user's repository,
same base, only when the object formats differ and there are no alternates).

### 2.2 The rule (recommendation implementing the owner's decision)

A path p that an undo would write is **committed since the turn** when p's entry (mode and blob,
or absence) in HEAD changed in any gap after the turn: p in `Ha(T)` against p in `Hb(T+1)`, and so
on, ending with p in `Ha(last)` against p in HEAD now. One `git diff-tree -r --no-renames
--name-only <a> <b> -- <paths>` per gap, in the shadow.

- A committed path is **not undone**; the turn's other paths are. The result names the newest
  commit in `Ha(T)..HEAD` that changed p, or says "HEAD moved from X to Y" when HEAD no longer
  descends from `Ha(T)`.
- **Per file:** of three changed files with one committed, two are undone and the third is named.
- **It follows HEAD as it is now:** a commit undone with `git reset` or `git revert` makes p
  undoable again.
- **Movements inside a turn do not count** (§2.3).
- A gap with an unknown end (a turn without a checkpoint, `head: "unknown"`) merges with its
  neighbours. A wrong guess makes undo unavailable; it never writes over a commit.

Why not "a commit after the turn contains the path": every commit contains every tracked path.
Comparing entries across gaps needs no history walk and stays right for a branch switch, reset,
amend or revert; walking commits by date would not (dates are what the committer wrote).

### 2.3 A commit made by the agent inside its turn (Q1)

If it counted, any agent could switch off undo for its own turn with `git commit -am x`, the same
class of problem as the planted `.git` fixed this week (A.15). **Recommendation:** movements
between `Hb(T)` and `Ha(T)` are not acceptance. Undo restores the files, leaves HEAD and the index,
and says "The agent committed during this turn (HEAD moved from X to Y); the files are restored,
the commit is still in history: `git log X..Y`." The same applies to a later turn's agent
committing an earlier turn's file, which is why the rule counts gaps, not everything after
`Ha(T)`. This departs from the owner's words "after commit, revert is unavailable", so it is Q1.
If agent commits are to count, the rule starts at `Hb(T)` and the abuse becomes a residual.

### 2.4 Undo never moves a ref

**Rule:** undo, rewind and unstash never move HEAD, a branch, a tag, the index or the reflog, and
never write anything in the user's repository. They write working files only. True on `main`
already (`git.rs`, `restore_paths` writes through a private index in the shadow); the note makes
it a stated rule with a test (T1).

### 2.5 Rewind

The same rule per path, over the gaps after the first rewound turn. Committed paths stay; the
rest go back to `before(T)`; each rewound turn gets `restored`.

### 2.6 What "the shadow drops it" means

- **Storage:** nothing new; gc's `repack -l` already drops shadow blobs a commit now holds.
  Whether loose duplicates go too is not established (§9).
- **Older checkpoints** are unusable for committed paths, by design. A.16 treated a missing
  object as damage; the two fit by order: the committed test runs before `secure_blobs`, so a
  committed path is reported as "committed in 1a2b3c4 after this turn; undo leaves it to git",
  never as missing content. A.16's refusal remains for uncommitted paths whose borrowed blobs
  were pruned.
- **The cost of the owner's decision, plainly:** undoing a committed file would be safe (an
  uncommitted revert; the commit stays). Forbidding it is a clarity choice, not a safety need. It
  is followed as decided.

### 2.7 Folders without a repository

No HEAD, nothing is ever committed: undo works as today.

### 2.8 A base commit that is gone (B.2)

If a commit a gap needs cannot be read (history rewritten and pruned), a path that HEAD tracks now
is treated as committed ("history was rewritten since this turn, so the harness cannot tell
whether you committed it; use git"); a path HEAD does not track stays undoable.

## 3. The stash

### 3.1 The object (recommendation)

**Contents:** for each path the undo writes **and** whose content differs from the expected
state, the whole file just before the undo (the person's version) and the same path in the
expected state (the merge base). A deletion by the person is an absent file. Not the whole
folder: the undo does not touch the other paths, the pre-restore snapshot already holds the whole
folder for undoing the undo, and a small stash is one a person can read and keep.

**Place:** a commit in the shadow at `refs/harness-stash/<session>/<stash-id>`, outside
`refs/harness/`, so `prune` (`for-each-ref refs/harness/`) never sees it and gc keeps what it
reaches. Its tree holds the stashed files; its parent's tree holds the base versions; trailers
name the turn, the rewound turns and HEAD.

**Self-contained:** its blobs go into a pack of their own with a `.keep` file, so gc's
`repack -l` does not drop them in favour of the user's copies, and a later history rewrite cannot
take them. Deleting the stash removes ref, pack and `.keep`. That gc honours `.keep` here is
proven by T6.

**Shown as:** "Stash of 12 Nov 14:03, before undoing *Fix the parser*, 3 files, 18 KB"; the id
`stash-<id>` is used by the API and the tools.

**Event:** a new kind `stash { stash_id, phase: created | applied | deleted, turn_id, files,
bytes, by }`, paths and sizes only, folded into `SessionView.stashes`. **Schema change**; the UI is
a fold over the log, so a stash must be in it.

### 3.2 Lifetime (Q2)

**Recommendation:** never pruned automatically; it holds the person's own work. Deleted by the
person from a list showing each size and the total, or with its chat (the delete confirmation
counts them); an incognito chat's stashes go when it expires (`crates/node/src/lib.rs`,
`purge_incognito`). **Alternative:** a generous cap (90 days, or 1 GiB per folder, oldest first).
Risk: it deletes someone's only copy of their edits, months later, without their action.

### 3.3 The question at undo

The node plans first. If nothing is committed and nothing the undo writes changed, the undo runs
at once, as today. Otherwise nothing is written and the node returns the plan: paths it will
undo, committed paths with their commits, changed paths, and files that would be deleted unsaved
(`RestorePlan.unsaved`). The choices:

| Choice | Honest label | Effect |
|---|---|---|
| Stash | "Keep my changes in a stash, then undo" (default) | Builds and records the stash, then undoes. |
| Drop | "Undo without a stash (my changes stay with the checkpoints for the next 10 changes)" | Undoes; the pre-restore snapshot still holds them, pruned as today. |
| Cancel | "Cancel" | Nothing written; the snapshot ref is deleted, as on any refusal today. |

**Recommendation:** keep the pre-restore snapshot for every restore, "Drop" included. The check
needs that scan anyway (`session.rs`, `restore`), so it costs nothing extra, and it is what lets
an undo be undone. "Drop" therefore does not discard at once, and the label must say so (Q3).

**Mechanism (recommendation):** the existing refuse-then-retry round trip, not the approval path.
`ApprovalRequested` belongs to a tool call in a running turn and is denied when the turn ends
(`session.rs`, `end_turn`); an undo runs with no turn. `session.restore` replaces `force: bool`
with `changes: "ask" | "stash" | "drop"` (absent is `ask`) and `seen`, a hash of the plan's inputs
(paths, blob ids, HEAD) returned by the ask. The answer re-plans from a fresh snapshot and asks
again if the hash differs. **Protocol change** (`crates/protocol/src/lib.rs`) and HTTP body change
(`crates/master/src/http.rs`, `restore_turn`).

**No person present** (an API call, a script): a call that does not choose gets the plan and
changes nothing. Routines have no undo tool. Never a silent choice.

**Order with "stash":** snapshot, plan, `secure_blobs`, stash commit and kept pack, stash ref,
`before_restore` event, `stash{created}` event, writes, `after`, `restored`. If an append fails
before the writes, the stash ref and pack are deleted and nothing is written.

### 3.4 Unstash by the person

A stashed file is the agent's change with the person's edits on top; writing it back whole would
bring the undone change back. So unstash is a **three-way merge per file**: base B (the file as
the turn left it, from the stash's parent), current C (the file now), stashed S. It runs
`git merge-file -p` on three temporary files in a private directory under the data directory,
with `--git-dir <shadow>` and the hardened base. `merge-file` calls git's merge code directly:
no merge driver, no attributes, and only `merge.conflictStyle` from config, which here is the
harness's. T12 proves it with a hostile `merge.<x>.driver` and `.gitattributes`. The result is
written to the shadow with `hash-object -w`.

Unstash claims the folder (`claim_folder`) and first takes the same pre-write snapshot as a
restore, so it can be undone. It writes through a private index and `checkout-index`, as `git.rs`,
`restore_paths` does: no symlink in a leading path is followed, no filter applies.

| Case | Result |
|---|---|
| S equals B, or C equals S | Nothing to do for this file. |
| C equals B | S written in place. |
| Text, clean merge | Merge written in place: the person's edit kept, the agent's change still undone. |
| Text with conflicts; binary (merge-file refuses); symlink differing | **Conflict.** |
| C absent (deleted since, or the undo removed a file the turn created) | Conflict: S carries the turn's content. On approval written under its own name, as nothing is there. |
| B absent (the turn deleted it, the person recreated it), C present and not S | Conflict. |
| S absent (the person had deleted it) | Reported, never applied: unstash never deletes a file. |
| p committed since the turn | No difference: unstash works on files and never reads HEAD. |

**Conflict (owner's rule):** nothing of that unstash is written until the person approves a
second time, seeing the conflicting files and their side-file names; the approval carries `seen`.
Then clean merges go in place and each conflicting file's stashed version is written as a side
file. Nothing is written over an existing entry and no conflict markers go into the person's file.
Note: the side file is the person's whole stashed file, so it contains the agent's change too;
that is what they had, and they take lines from it.

**Side-file name.** On the last path component N, as bytes:

1. The extension starts at the last `.` that is neither N's first nor its last byte; the stem is
   what precedes it. With no such dot the stem is N and the extension is empty.
2. Candidate k is `stem-k` followed by the extension, for k = 1 to 999.
3. The first candidate with nothing at that name (`lstat`: a file, directory, symlink or dangling
   symlink all count; a case-insensitive file system matches any case) and not taken by another
   side file of the same unstash wins. `checkout-index` runs without `-f`, so a name that appeared
   meanwhile fails the write and the unstash is planned again.
4. Over 255 bytes, the stem is cut at a character boundary (a byte boundary if not UTF-8). An
   empty stem, a path over 4,096 bytes, or k past 999 refuses that file with the reason.
5. The stashed mode is kept (executable bit; a symlink stays a symlink).

| Name | Side file |
|---|---|
| `name.py` | `name-1.py`, then `name-2.py` |
| `archive.tar.gz` | `archive.tar-1.gz` |
| `Makefile` | `Makefile-1` |
| `.env` | `.env-1` |
| `.eslintrc.json` | `.eslintrc-1.json` |
| `name.` | `name.-1` |
| `name-1.py` conflicting | `name-1-1.py` |

The last dot keeps the final extension, so editors still know the type; the cost is
`archive.tar-1.gz`.

**Partial unstash:** the person picks files. `stash{applied}` names the files applied and side
files written. The stash stays until deleted, marked "applied" once every file was; applying it
again finds C equal to S and writes nothing.

### 3.5 The agent and stashes

| Tool | Risk | Does |
|---|---|---|
| `stash_list` | read-only | This chat's stashes: id, date, turn, files, applied marks, size. |
| `stash_show` | read-only | One stashed file, or its diff against the base or the file now; `read`'s size limits. |
| `stash_apply` | mutating | Unstash files of one stash, exactly as §3.4. |

- Only stashes of the agent's own chat, in its folder.
- **No tool creates, changes or deletes a stash.** Only an undo the person chose makes one; only
  the person deletes one.
- `stash_apply` goes through the approval path like `write` (`TurnHost::approve`,
  `crates/core/src/backend.rs`). A conflict asks again, always a person whatever the approval
  mode, with `turn_allow` false, and is denied when no person can answer (O6). Whether the fold
  and UI take two approvals under one `call_id` is not established; if not, the second gets a
  derived id.
- The shadow is in the data directory: the file tools refuse paths there
  (`crates/tools/src/lib.rs`, `refuse_protected`) and the enforced sandbox gives the shell no
  access (`crates/tools/src/sandbox.rs`, module comment, `exposes`). With `[sandbox] mode = "off"`
  or best-effort without Landlock it is reachable, like every checkpoint today (`PLAN.md` R2).
- A stash holds text anyone may have written; `stash_show` carries the risk of `read`, no more.
- Served through `TurnHost`, implemented by `Session`, as `delegate` is.

## 4. What changes in the code

### 4.1 Existing pieces

| Piece | Verdict | How |
|---|---|---|
| `before`/`after` snapshots | Changes | Add `head`. Optionally `before.files` = the person's edits since `after(T-1)`. |
| `before_restore` (A.9) | Stays | For every restore and every unstash; under `checkpoints_keep`. |
| `restored` | Stays | Lists only what was undone; committed paths go into the note (`session.rs`, `restore_notice`). |
| Unforced check (`restore_plan`) | Changes | Becomes the plan of `changes: "ask"`, with committed paths and `seen`. |
| `force` | Goes | Replaced by `changes`. |
| `secure_blobs` | Stays, reused | Also for stash blobs, into a kept pack. |
| Index rebuild, nested-repository exclusion, interrupted-restore path, `claim_folder` | Stay | Unstash also claims the folder. |
| `checkpoints_keep` pruning (`git::prune`) | Stays | Stash refs are outside `refs/harness/`. |
| Borrowing | Stays | Stashes do not rely on it. |

### 4.2 New pieces

| Piece | Where |
|---|---|
| HEAD per checkpoint | `git.rs`, `session.rs`, `event.rs`, `view.rs`, `web/src/fold.ts` |
| Committed-since test over gaps | `git.rs`; called in `session.rs`, `restore`, before `restore_diff` |
| Stash commit, kept pack, list, delete; `stash` event and fold | `git.rs`, `session.rs`, `event.rs`, `view.rs`, `web/src/fold.ts` |
| `changes`/`seen` on `session.restore`; `session.stash.apply`, `.delete`, `.show` | `crates/protocol/src/lib.rs`, `crates/node/src/lib.rs`; HTTP in a new `crates/master/src/http/stashes.rs` |
| Merge and side-file naming (naming as a pure function) | `git.rs` |
| `stash_list`, `stash_show`, `stash_apply` | `crates/tools`, `TurnHost` |
| Secure at `after` (Q5): a kept pack per turn, deleted when `prune` drops that turn | `session.rs`, `checkpoint_after`; `git.rs`, `prune` |

**The UI needs** (no screens here): the plan with its three choices; a per-chat stash list; a
stash file view (diff against the base); unstash with file selection and a conflict confirmation
naming side files; delete; the stash total per folder; stashes counted in the chat delete
confirmation.

### 4.3 Cost per turn

Recording HEAD: one `rev-parse` per checkpoint that reads no file of the folder; milliseconds,
no scan. The committed test runs only at undo: one `diff-tree` per gap over the turn's paths, at
most one per kept checkpoint. Unstash adds one folder snapshot: to be filled from experiment B.3.
Q5 adds one `cat-file --batch-check` and one `pack-objects` over the old versions of the turn's
changed files, so it scales with the change, not the folder; its disk use is to be filled from
experiment B.3.

### 4.4 What it does not fix

- **A restore is not atomic across files** (T33 residual); unstash shares this. Both name the
  pre-write checkpoint on failure.
- **Uncommitted work inside a nested repository** is in no checkpoint and no stash.
- **Two chats in one folder:** chat B's agent edits look like the person's to chat A's undo (and
  are stashed under A, unseen by B's agent); B's agent commits look like the person's in A's
  gaps. Turns are still not serialised.
- **The person's edits made during a turn** are part of `after(T)`, so undo overwrites them
  without asking; they survive only in the pre-restore snapshot. Fixing that needs per-write
  attribution, which the shell does not give.
- **An agent that prunes its own turn's base** (§5), unless Q5.
- **A process that outlives its turn** (B.1) can commit in a gap and switch off undo for those
  files. C.11 fixes the process; the rule cannot tell.

## 5. How it breaks

| Case | What happens |
|---|---|
| Injected agent runs `git commit` | In-turn movement, ignored (if Q1). Files restored, HEAD left, the commit named. |
| Agent amends or rebases the person's commits | Same. HEAD is not moved back; the note gives the old HEAD id from the harness's log, which outlives `reflog expire` (the commit object may not). |
| Agent runs `git stash` or `git checkout <branch>` | The file changes are the turn's and are undone. The agent's stash entry and the branch stay; the note says HEAD moved and from where. |
| Agent deletes stashes or checkpoints through the shell | Unreachable with the enforced sandbox and the file tools (§3.5); reachable with the sandbox off, an existing residual. |
| Agent rewrites history and runs `gc --prune=now` in its turn | The `before` snapshot's borrowed blobs vanish and A.16 refuses the undo: the agent made its turn un-undoable. The user's `.git` lies in the writable folder and Landlock cannot carve it out; borrowing (O11) keeps this open. **Q5:** at `after`, copy the old versions of the changed files into a kept pack (then no later rewrite takes them) and say in the chat which ones are already gone. |
| Person commits, then changes their mind | `git reset` or `git revert` makes the file undoable again; otherwise git is the way back. |
| History rewritten after a stash was taken | Unaffected: kept pack. |
| Restart between question and answer | No state is kept between them; the answer re-plans and asks again if `seen` differs. |
| Very large stash | Costs its files' compressed size once, shown in the list and total; no cap (Q2). Building it is bounded by the 60 s snapshot limit (`git.rs`, `SNAPSHOT_TIMEOUT`). |
| Stash holding a secret (an edited `.env`) | File content on the node's disk, the same exposure as the checkpoint store. Events carry paths and sizes only. Content leaves the node only when viewed (to the master, like a file view) or read by the agent (to the provider, like `read`). |
| Routine, no person | No undo tool; `stash_apply` and every conflict denied without a person. |
| Hostile repository config | The only new call in the user's repository is `rev-parse` (§2.1), which runs nothing; the merge runs in the shadow. |

## 6. Tests the build is committed to

1. `undo_after_the_person_committed_moves_no_ref_and_names_the_commit`: HEAD, branches, the index
   file and the reflog byte-identical, the file unchanged, the commit named.
2. `a_commit_made_by_the_agent_in_its_turn_does_not_switch_off_undo` (if Q1): files back to
   `before`, HEAD still at the agent's commit, which the note names.
3. `one_of_three_files_committed_the_other_two_are_undone`.
4. `a_later_turns_agent_commit_is_not_acceptance`: turn 2's agent commits turn 1's file; undo of
   turn 1 still restores it.
5. `reset_after_a_commit_makes_undo_available_again`.
6. `a_stash_survives_pruning_and_a_history_rewrite`: after 20 turns, a rewrite and
   `gc --prune=now`, every stashed file reads back byte-identical.
7. `declining_the_question_changes_nothing`: folder, shadow refs and log as before; no stash.
8. `a_folder_changed_between_question_and_answer_is_asked_again`: nothing written.
9. `clean_unstash_keeps_the_persons_edit_and_not_the_agents_change`.
10. `a_conflict_writes_nothing_until_approved_then_a_side_file`: before approval every file
    byte-identical; after it `name-1.py` equals the stash and `name.py` is byte-identical.
11. `side_file_names_follow_the_rule`: §3.4's table, taken names, 255-byte and non-UTF-8 names, a
    dangling symlink in the way.
12. `no_git_call_in_undo_or_unstash_runs_a_hook_or_reads_user_config`: hooks, fsmonitor, filters,
    a merge driver named in `.gitattributes` and a global config set as markers; none fires.
13. `the_agent_reads_its_stashes_and_cannot_create_change_or_delete_one`: list and show work for
    its chat and not another's; no tool or sandboxed shell reaches the refs or packs.
14. `the_agents_unstash_asks_and_a_conflict_asks_again`: the second approval is denied in a
    routine.
15. `unstash_never_deletes_a_file`: a stashed deletion is reported, the file stays.
16. `a_turn_that_pruned_its_own_base_says_so` (if Q5).

## 7. Build order and size

S: a day or less, M: a few days, L: a week or more, for one person with an agent.

| # | Task | Size | When |
|---|---|---|---|
| 1 | Record HEAD per checkpoint, fold, `PLAN.md` | S | After Gate 3 |
| 2 | Secure at `after`, report a pruned base (Q5) | S | **A pure fix: may go before Gate 3** (narrows B.2's residual) |
| 3 | Committed-since test, per-file undo and rewind, notes | M | After 1 |
| 4 | Stash object, kept pack, `stash` event, `changes`/`seen`, the question | M | After 3 |
| 5 | Unstash by the person: merge, naming, conflict approval | M | After 4 |
| 6 | Agent tools | S | After 5 |
| 7 | Web UI | M | With 4 to 6 |
| 8 | `PLAN.md` T33 and event table, `README.md` undo section | S | With each task |

Together about **L** (two to three weeks). It changes built behaviour and adds surface, so it
waits for Gate 3, except task 2.

## 8. Questions for the owner

| # | Question | Recommendation | If the other answer |
|---|---|---|---|
| Q1 | Does a commit made inside a turn make that turn's files un-undoable? | **No:** only commits in gaps count; undo restores the files and names the agent's commit. | Any agent switches off undo for its own turn with one command; documented as a residual. |
| Q2 | How long is a stash kept? | **Until the person deletes it or its chat;** sizes always shown. | A cap deletes someone's only copy of their edits without their action. |
| Q3 | Does "undo without a stash" still keep the pre-restore copy? | **Yes,** labelled as kept for the next 10 changes. Or offer only "stash, then undo" and "cancel". | A true discard saves no work (the scan runs anyway) and makes a misclick final. |
| Q4 | Should unstash merge, or always write beside? | **Merge,** side file only on conflict. | Every unstash brings the agent's change back in a side file the person cleans up by hand. |
| Q5 | At each `after`, copy the old versions of the changed files into a kept pack, and say when they are already gone? | **Yes.** It closes B.2 for kept turns and exposes an agent that pruned its own base; disk cost to be filled from experiment B.3. | Recent undos stay exposed to any history rewrite plus gc, by the person or an injected agent. |

## 9. Not settled

- B.3's numbers: the unstash snapshot's time, Q5's disk use.
- Whether gc drops loose shadow objects the alternate also has (§2.6), and that `.keep` keeps a
  stash's pack through `repack -l` (test 6).
- Whether two approvals under one `call_id` fold and render (§3.5).
- The exit status of `checkout-index` without `-f` on an existing file (tests 10 and 11).
