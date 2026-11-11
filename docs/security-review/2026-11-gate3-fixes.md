# Gate 3 fix review, 2026-11-11

Second pass, reviewing the branches that fix the first review
(`docs/security-review/2026-11-gate3.md`). Branches read detached, nothing
committed to them, no product code changed.

- `fix/R.1-sandbox-findings` (S-1, S-2, S-3, S-4, S-6), tip `f21f02e`, fixes at
  `1220e1a` and `1599e50`.
- `fix/R.3-link-findings` (L-1 to L-6, L-8, L-9), fixes `dcbe60c..4b9df87` on
  `f3bf1bc`, then a merge of `main`.

Same severity scale as the first report, and the same rule for proofs: a test
that passes is an attack that still works.

## 1. Verdict per finding

| Finding | First severity | Result |
|---|---|---|
| S-1 (node git runs session-written config) | Critical | **Closed with a remaining gap.** The config/attribute route is gone; a new route, `commondir`, puts a sub-agent in another repository (R1-1), and the pin's check-to-use window writes into an arbitrary folder (R1-2). |
| S-2 (folder swapped for a link) | High | **Closed with a remaining gap** for sub-agent worktrees (R1-2); the file-tool path is closed. |
| S-3 (`core.worktree` moves checkpoints/undo) | High | **Closed.** |
| S-4 (relative data dir) | Low | **Closed.** |
| S-6 (ignored files not undone) | Low | **Closed with a remaining gap**: the chat is told a `.gitignore` changed but not which files were left behind (author states this). |
| L-1 (parent names another chat) | Medium | **Closed.** |
| L-2 (second `session_created` moves owner) | Medium | **Closed.** |
| L-3 (uncapped node strings) | Medium | **Closed.** |
| L-4 (revoke race) | Medium | **Closed** (by reading). |
| L-5 (unbounded hello) | Low | **Closed.** |
| L-6 (hello/append leak another node's session) | Low | **Closed.** |
| L-8 (stuck resume) | Low | **Closed.** |
| L-9 (unbounded ack error) | Low | **Closed.** |

**Fit to merge?**
- **R.3: yes.** Every finding it claims is closed, confirmed for L-1/L-2/L-3 by
  the first review's own proofs now failing, and for the rest by reading. The
  one residual (R3-1) is a defence-in-depth gap, not an exploit.
- **R.1: no, not as it stands.** S-1 and S-2 are closed in the forms the first
  review proved, and S-3/S-4 fully. But R1-1 (High) is a fresh escape of the
  same class the branch set out to close, and R1-2 (Medium) is a reliably
  winnable race that writes outside the folder. Both are new and both have a
  passing proof. R.1 is fit after R1-1 and R1-2.

New findings: 1 High, 1 Medium, 3 Note.

## 2. New findings

### R1-1. High. A `.git/commondir` file puts a sub-agent's worktree, and its sandbox grant, in another repository

- Adversary: a prompt-injected agent whose session is in a git repository and
  can delegate to a writing sub-agent.
- The session's own `.git` is writable from its sandboxed shell (R2). Git reads
  `<session>/.git/commondir`, one line, as the path of the common git directory,
  and `SubRepo` takes the worktree's home from
  `rev-parse --git-common-dir` (`crates/node/src/git.rs:2226`, `:2228`,
  `SubRepo.common`). The top level stays the session folder, so `repo_info`'s
  guard (`:490`) passes. `make_worktree` then runs `git worktree add` in the
  session's repository but creates the worktree under the *other* repository's
  `.git` (`:2226-2243`), and `worktree_git` (`crates/tools/src/sandbox.rs:331`)
  grants the child's shell read on all of that `.git` and read-write on its
  `objects`, judged from the path alone.
- Gain: the sub-agent reads another local repository in full (its config, which
  may hold remote URLs with tokens, and every object = its whole history), and
  writes objects into it. The path in `commondir` is absolute and unbounded by
  `workspace_roots`, so the target is any `.git` on the machine, including
  another user's project.
- Established: **proven by test**,
  `review_r1_1_commondir_puts_the_sub_agent_in_another_repository`
  (`crates/node/src/git_review_fixes.rs` in the patch): the worktree lands
  under `other/.git`, the grant resolves to `other/.git`, and the child's git
  reads `OTHER-REPOSITORY-SECRET`. Git's honouring of `commondir` verified by
  experiment (git 2.56).
- Smallest fix: `find_repo`/`repo_info` already learn the common dir; refuse the
  repository (treat the folder as plain) when the common dir, canonicalised, is
  not `<top>/.git` (or inside it). More generally the worktree admin area
  (`SubRepo.common`) must be inside the session's own checked `.git`.
- Threat row: T19, T32. The branch's premise is that no node git call reaches
  outside the session's repository; this reaches another repository entirely.

### R1-2. Medium. The pin's check-to-use window lets a left-running process redirect a sub-agent's worktree out of the folder

- Adversary: a prompt-injected agent with a process still running after a turn
  (the recorded `setsid` survivor), against its own next delegation.
- `make_worktree` checks the worktree's pin (`crates/node/src/git.rs:2256`,
  `:2258`) and then runs `git checkout-index` with the worktree path as work
  tree. The worktree lives under `<repo>/.git/harness-worktrees/<id>`, inside
  the parent's own folder, which the parent's shell may write. A process that
  renames the worktree away and puts a symlink there between the last check and
  git's open makes the node's own (unsandboxed) git write the base files into
  the link's target.
- Established: **proven by test**,
  `review_r1_2_a_swap_during_the_worktree_fill_writes_into_another_folder`
  (in the patch): a thread doing only file-system calls wins the window on
  7 to 17 of 120 delegations here, writing the parent's `authorized_keys` into a
  sibling folder. An attacker simply retries.
- The author discloses this as a residual ("a parent's shell can still swap a
  child's worktree within that window"). It is filed because it is reliably
  winnable and writes attacker-chosen content outside the folder, which is the
  S-2 impact the branch set out to remove.
- Smallest fix: make the worktree in a directory the agent cannot write (under
  the data directory, as turn checkpoints already are), or open the work-tree
  directory once (`O_PATH`/`O_NOFOLLOW`) and have git use that fd, so the path
  is not resolved again after the check.
- Threat row: T33 (the pin residual), but worse than recorded here because the
  window spans several git calls, not one open.

### R1-3. Note. The narrowed grant leaves `<repo>/.git/objects` writable to the sub-agent's shell

With R1-1 fixed this is the sub-agent's own repository, but a child can still
write arbitrary loose objects there. Snapshots hash file content, so a planted
object is not picked up unless a file hashes to it; the risk is store bloat and
corruption of the user's object store, not content forgery. Noted.

### R3-1. Note. A hostile node that acks a create before sending `session_created` leaves a cross-owner parent in the row

`Store::append` keeps a `parent_session` whose parent is on the same node
(`crates/store/src/lib.rs:772-781`) without checking the owner, which is not
known yet. `claim_session` drops a cross-owner parent afterwards
(`:1647-1666`), but only if the row already names it. A node that sends its
`ack` for the create before the `session_created` event makes `claim_session`
run first (nothing to drop); the event then sets `parent_id` and copies the
victim's `session_workspaces` row. No leak follows today: every consumer
(`project_of`, `turn_memory`) goes through `projects::parent_of`
(`crates/master/src/projects.rs:282-293`), which re-checks node and owner. The
gap is defence-in-depth: any future code that reads `parent_id` or the copied
workspace mapping without that re-check would leak. Fix: have `append` keep a
parent only when it already has an owner equal to the new row's, or drop the
copied workspace rows whenever `parent_of` later refuses.

### R3-2. Note. A node can create frozen placeholder rows

`Store::freeze` (`crates/store/src/lib.rs:1055`) inserts an empty frozen row for
a session whose first event is refused. A node controls session ids, so it can
make many tiny frozen rows (one refused first event each). Bounded by one small
row per id and by the frame queue; no worse than a node writing its own real
sessions. Noted.

## 3. What I checked and found sound

**R.1**
- The first review's three proofs now fail on the branch: the old S-1 node-half
  (`review_s1_a_sub_agent_makes_the_nodes_own_git_run_a_command`, the filter
  never runs), the old S-1/S-2 sandbox proofs (`git config` into `.git` is
  denied; the worktree folder cannot be renamed away), and the old S-3 proof
  (the checkpoint root no longer moves to an ancestor). The branch's own
  inverted tests (`a_filter_a_sub_agent_defines_never_runs_in_the_nodes_git`,
  `hostile_config_in_the_main_repository_never_runs_in_the_nodes_git`,
  `a_session_whose_folder_was_replaced_refuses_to_work`) test the attack, not a
  weaker thing.
- `user_git` (`:324`) runs only `rev-parse` and `worktree add
  --no-checkout`/`--orphan` in the user's repository, with the hardened
  overrides and `GIT_CEILING_DIRECTORIES` set to the parent; `--no-checkout`
  and `--orphan` convert no file, so filters, hooks and fsmonitor do not run
  (confirmed by `hostile_config_...`). Base, worktree fill, result and patch run
  in the shadow `<hash>-sub.git` with `GIT_ATTR_SOURCE` = empty tree and
  `core.attributesFile=/dev/null`.
- `merge_back` writes the result with the restore machinery (`restore_paths` +
  `check_inside`), not `git apply`; `check_inside` (`:1900`) refuses any path
  not relative-with-plain-components, outside the folder, or in a protected
  folder, re-checking the folder's canonical identity. `restore_report`'s
  symlink-escape soundness from the first review still holds.
- `FolderPin` (`crates/core/src/folder.rs`) pins canonical path, device and
  inode; `check` re-canonicalises and compares. `checked_folder`
  (`crates/node/src/session.rs:368`) gates every turn, restore, delegation,
  checkpoint-after, restore-write and publish, and also re-tests
  `workspace_roots` and the protected folders. The data directory is made
  absolute in the CLI (`crates/cli/src/main.rs:169`) and the sandbox refuses a
  relative data dir or temp root in every mode (S-4).
- `find_repo` (`:442`) walks the file system, stops at `bounds.ceiling`
  (the innermost workspace root), and a repository folder that holds the data
  directory is checkpointed as a plain folder at the session folder.

**R.3**
- L-1/L-2/L-3 proofs from the first review now fail: a second `session_created`
  is `Refused` (seq != 1), a cross-node parent is dropped to `None`, and the
  title is cut to 200 characters.
- `owner_id` is never changed by an event once set (`:757` update is
  `WHERE owner_id IS NULL`). `refusal` (`:506`) caps kind, total size,
  `cwd`, `model`, `agent_id` and the profile enum. Every `parent_id` follower on
  the master goes through `parent_of` with a node-and-owner re-check; the one
  raw use (`http/projects.rs:394`) only refuses joining.
- L-4: `revoked` set and the node map are written under one lock in
  `revoke_node` (`nodes.rs:64-72`) and read under it in `add_link`
  (`lib.rs:880`), with a second `node_is_revoked` store read in `node_connection`
  after the stop token is registered; `send_command` and `send_raw` refuse a
  revoked node.
- L-5: the hello is read in batches of 256 with one store call each and stops
  when a newer link exists (`is_generation`). L-6: `hello_states` returns
  `Foreign` for another node's session (live or deleted), and `append` refuses
  another node's deleted session as `WrongOwner` rather than saying it was
  deleted; `redeleting` is keyed by `(node, session)`. L-8: `ResumeGuard` clears
  `resuming` on every exit except a batch that landed with more due. L-9:
  `ack_error` cleans a remote node's error to one 300-char line.
- The node handles the new `Refused` outcome (`session.rs:265`) by ending the
  turn on a log failure; its own log refuses the event too, so the master never
  sees it.

## 4. What I did not check

- The web UI rendering of node- and model-controlled strings (the first
  review's open area): not reached again. R.3 does touch `web/src` (Markdown,
  titles, Users), but I did not read or build it this pass.
- I did not reproduce L-4, L-5 or L-8 with a running master; they are judged by
  reading.
- I did not run `cargo clippy`, `cargo deny` or the full workspace test suite;
  only the proof tests and the packages they build (`harness-node`,
  `harness-store`, `harness`).
- R.1's merge of `main` (`f21f02e`) and R.3's merge (`043aea9`) were not read
  for conflicts beyond confirming the fix commits are intact.
- Two node processes on one data directory, and `make_worktree` under a full
  disk, were not exercised.

## 5. Proofs

`docs/security-review/proofs/2026-11-gate3-fixes.patch` adds the two new-finding
tests (R1-1, R1-2). Each **passes on `fix/R.1-sandbox-findings`, and passing is
the defect**.

```bash
git checkout --detach fix/R.1-sandbox-findings
git apply docs/security-review/proofs/2026-11-gate3-fixes.patch
cargo test -p harness-node --lib review_fixes -- --nocapture   # R1-1, R1-2
git apply -R docs/security-review/proofs/2026-11-gate3-fixes.patch
```

To confirm the originals are closed, apply the first review's patch
(`docs/security-review/proofs/2026-11-gate3.patch`, on `main`) to each branch and
see the relevant tests **fail**:

```bash
# On fix/R.1: S-1 node half, S-1/S-2 sandbox half, S-3 (all now fail)
git apply --include='crates/cli/*' --include='crates/node/src/git_review_gate3.rs' <main>/docs/security-review/proofs/2026-11-gate3.patch
#   (the runtime.rs hunk needs a 3-way apply; the S-1 test body is unchanged)
cargo test -p harness --test review_gate3
cargo test -p harness-node --lib review_s3
# On fix/R.3: L-1, L-2, L-3 (all now fail)
git apply --include='crates/store/tests/*' <main>/docs/security-review/proofs/2026-11-gate3.patch
cargo test -p harness-store --test review_gate3
```

All of the above was run here on Linux 7.2, git 2.56 (2026-11-11): R1-1 and R1-2
pass on R.1; the four S-proofs fail on R.1; the three L-proofs fail on R.3.

## 6. The three things I am least sure of

1. R1-2's framing as Medium rather than High: the write content is the parent's
   own folder (attacker-controlled), the target is any folder, and it is
   reliably winnable, but it needs a process left running across the pin check.
2. Whether R1-1's `commondir` is the only redirect left: I checked `commondir`
   and `core.worktree`; `GIT_DIR`/`GIT_WORK_TREE` are cleared by `hardened`, but
   I did not exhaustively test every `.git` gitfile/`extensions.*` form against
   `user_git`.
3. R.3's L-4/L-5/L-8 are read-only conclusions; I did not drive a master to
   reproduce the races, so a lock-ordering mistake could remain that reading
   missed.
