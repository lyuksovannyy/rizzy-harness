# Gate 3 fix review 4, 2026-11-11: the R1b delta

Fifth and last focused pass on R.1. Reviewed: `fix/R.1-sandbox-findings` at
`6cb4d55`, the delta `git diff f1f9fd3..6cb4d55` (R1b-1 to R1b-4). Read detached,
no product change.

## 1. Verdict

| Finding | Result |
|---|---|
| R1b-1 (a restore holds every blob in memory) | **Closed.** |
| R1b-2 (snapshot read window) | **Closed with a remaining gap** (R1c-1). |
| R1b-3 (`user_repo` reads by path) | **Closed.** |
| R1b-4 (`remove_tree` recursion) | **Closed.** |

**R.1 is fit to merge only if the owner accepts R1c-1**, the author's own stated
Landlock-follows-links gap, which I show is a live arbitrary-file read from a
sub-agent, not only a "widened read-only grant". It is one small fix, the same
pin mechanism already used for the folder. Everything else in the delta holds.

New findings: 1 High (R1c-1), 2 Note.

## 2. New findings

### R1c-1. High. A sub-agent's `.git/objects` grant follows a link, so a left-running parent process gives the child read of any file the user can read

Severity: a prompt-injected agent reads SSH keys, other repositories and other
users' readable files from inside a sub-agent, defeating the sandbox the whole
of R.1 builds; it needs a process left running (the recorded `setsid` survivor)
but no race window.

- Adversary: a prompt-injected agent (1) whose chat delegates to a writing
  sub-agent.
- Cause: a sub-agent's folder is granted read on the user's object store by the
  path `<repo>/.git/objects` (`OwnedFolder::objects`, set once at delegation
  start). The `sandbox-exec` wrapper opens every granted path to apply its
  Landlock rule, and that open follows links (the author names this for the
  node's git; it applies to the child's own grant too). `<repo>/.git` is inside
  the parent session's folder, which the parent's shell may write.
- Sequence: parent `bash`: `mv .git/objects .git/objects.r && ln -s /home/u/.ssh .git/objects`.
  From the next child command on, the child's shell is granted read on
  `/home/u/.ssh`; `cat id_ed25519` returns it. A link into the data directory is
  caught by the `exposes` check (`crates/tools/src/sandbox.rs`, the `checked`
  list), so the node key and `master.db` stay closed; everything else of the OS
  user that is not a protected folder is open.
- Established: **proven by test**,
  `review_r1c_1_the_borrowed_object_store_grant_follows_a_link`
  (`crates/cli/tests/sub_agent_sandbox.rs` in the patch), through the real
  wrapper: the key is denied, then after the swap the child reads
  `SSH-KEY-CANARY`.
- The author discloses the mechanism ("the landlock crate opens grant paths
  following links, so a swapped `.git/objects` could widen the read-only grant")
  but frames it for the node's git, where git reads objects by hash and cannot
  pull a non-object file's bytes into a snapshot. The child's shell has no such
  limit: it is a direct read primitive. Worse than recorded.
- Smallest fix: pin `OwnedFolder::objects` at child creation
  (`FolderPin::at`) and, before each command, refuse when it no longer resolves
  to the pinned directory, exactly as the session folder is already handled; or
  pass the grant to the wrapper as an `O_PATH|O_NOFOLLOW` descriptor opened once
  at child start. The same guard covers the node's git `borrowed` grant.
- Threat row: T4, T10, T32. The data directory stays closed (T10 holds); the
  rest of the user's files do not.

### Notes

- N-1. The node's git `borrowed` grant (`command_std`, `repo.borrowed` pushed to
  `ro`) is the same swappable path, but git reads the alternates store by object
  hash, so a swap to `~/.ssh` yields no object and no exfiltration into a
  snapshot. Low on its own; the fix for R1c-1 should cover it so it cannot
  combine with a future change.
- N-2. `harness_node::testing` is `#[doc(hidden)] pub mod` and so present in a
  release build. It only drives snapshots and sub-agent folders for the cli
  tests and exposes no secret or auth path, but a node binary should not carry a
  public test API; gate it behind a `cfg(feature)` or `cfg(test)` re-export.

## 3. What I checked and found sound

- **R1b-1.** `blob_contents` is gone (my old proof no longer compiles, which is
  the confirmation). `blob_sizes` reads every size with
  `cat-file --batch-check` and refuses per-file and total above `WriteLimits`
  (UNDO 4/16 GiB, SUB_AGENT 200 MiB/2 GiB) with `saturating_add`, before any
  delete or write. `stream_writes` then streams each blob from one
  `cat-file --batch` through a 64 KiB `BufReader` into `at::Top::write_from`,
  re-checking the streamed header's size against the pre-checked size and
  refusing a mismatch (`n != want`); `write_from` copies exactly `size` bytes
  with `io::copy` of a `take(size)`, holding none, and a link target is bounded
  by `LINK_MAX` (4096). `restore_paths` and `SubRepo::apply` pass UNDO and
  SUB_AGENT respectively.
- **R1b-2.** `command_std` runs every git of a shadow repository through
  `Sandbox::helper`: read-only on the work tree (read-write only when the node
  fills a sub-agent's new node-owned folder, which no session can reach),
  read-only on the borrowed object store, read-write on the shadow and on a
  private index's parent when outside the shadow. The read-write grants are all
  on node-owned directories; no session can write where the confined git has
  write. `inside_only` drops a newly listed file whose real folder lies outside
  the session folder or in a protected one; links are stored as links.
  `best-effort`/`off` run unconfined, logged; an agent cannot flip the sandbox
  to `None` at runtime (it is set at node start), and deleting the wrapper binary
  fails commands closed. I searched `git.rs`, `git/upkeep.rs`, `git/at.rs` and
  `session.rs`: no git call reads a session's work tree outside the jail;
  `init`, `verify-pack` and the global-excludes lookup touch only the shadow or
  the user's global config, which a session cannot write. `user_repo` reads the
  session's `.git` with its own bounded reader, not git.
- **R1b-3.** `user_repo::read_bounded` opens with `O_NOFOLLOW | O_NONBLOCK`,
  `fstat`s the handle for a regular file, and reads `take(cap + 1)` (512 B for
  `HEAD` and refs, 1 MiB for `packed-refs`); `find` now runs in
  `spawn_blocking`.
- **R1b-4.** `remove_tree` is iterative with a stack of one open directory per
  level, listed through a duplicated descriptor (no path resolution), capped at
  `REMOVE_DEPTH` (128); deeper content is left and the write fails with the
  reason.
- The flaky `bash::tests::cancel_kills_process_tree` is unrelated to the git
  confinement: it exercises bash's own process-group kill, which this delta does
  not touch; it reads as load-dependent timing.

## 4. What I did not check

- I did not re-run the author's race test
  (`a_folder_swapped_for_a_link_during_an_undo_…` and the snapshot/sub-agent
  variants) confined and unconfined; I rely on the author's rates (4/300 and
  2/100 unconfined, 0 confined) plus the reading above and my R1c-1 proof, which
  exercises the same follow-the-link mechanism on the child's grant.
- The memory tests the author added (`git_restore_memory_tests.rs`,
  `tests/restore_memory.rs`) were read, not run.
- `git/upkeep.rs` and `crates/node/src/artifacts.rs` beyond the jail-plumbing
  lines; `crates/cli/tests/sub_agent_sandbox.rs` beyond its fixture and my own
  added case.
- No full suite, `clippy` or `cargo deny`.

## 5. Proof

`docs/security-review/proofs/2026-11-gate3-fixes-4.patch` adds one test; it
passes on `6cb4d55`, and passing is the gap.

```bash
git checkout --detach fix/R.1-sandbox-findings
git apply docs/security-review/proofs/2026-11-gate3-fixes-4.patch
cargo test -p harness --test sub_agent_sandbox review_r1c_1   # needs Landlock
```

Run here on 2026-11-11 (Linux 7.2): it passes.

## 6. The three things I am least sure of

1. R1c-1's severity against the owner's intent: it is an acknowledged residual,
   and the owner may accept "arbitrary read minus the data directory" as the
   price of borrowing the object store; I rate it High because it defeats the
   agent sandbox with no race.
2. Whether the `exposes` check truly closes every link into a protected folder
   at command time (it canonicalizes the stored `objects` path, which follows
   the swap); I tested the `~/.ssh` target, not a data-directory target.
3. The race rates are the author's, not mine; I did not reproduce them.
