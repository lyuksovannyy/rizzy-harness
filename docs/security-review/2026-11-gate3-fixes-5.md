# Gate 3 fix review 5, 2026-11-11: the grant-identity fix, and the whole review

Sixth and final read on R.1. Reviewed: `fix/R.1-sandbox-findings` at `536492e`
(the fix `git diff 6cb4d55..536492e`, restricted to the sandbox, wrapper, git and
test files). Read detached, no product change.

## 1. Verdict

**R1c-1 is closed. R.1 is fit to merge.** No new finding of Medium or above; one
Low and two Notes, all narrow.

## 2. R1c-1: closed

My proof (`review_r1c_1_the_borrowed_object_store_grant_follows_a_link`,
`docs/security-review/proofs/2026-11-gate3-fixes-4.patch`) **fails on `536492e`**:
after the parent swaps `.git/objects` for a link to `~/.ssh`, the child's shell
no longer reads the key. The author's inverted tests pass and cover more than
mine: `a_replaced_object_store_is_never_granted_to_a_child` (`objects`→ssh,
`objects`→data, `.git`→ssh, `.git`→data), `the_nodes_git_never_follows_a_
replaced_object_store`, and `the_wrapper_refuses_a_grant_that_was_replaced_and_
names_it` (the session folder swapped after its pin check).

## 3. What I checked and found sound

- **The checked descriptor is the one the rule is built from.** `open_grant`
  (`crates/tools/src/sandbox.rs`) opens each path once with `O_PATH` (plus
  `O_NOFOLLOW` for an identity grant), `fstat`s that descriptor, compares
  `st_dev`/`st_ino` to the expected `Identity`, and returns the descriptor;
  `imp::restrict_self` builds `PathBeneath::new(fd, access)` from that same
  descriptor. The old `PathFd::new(path)` second open is gone. So the inode the
  rule binds is the inode that was verified: no path is resolved twice.
- **Intermediate swaps.** `O_NOFOLLOW` guards only the last component, but a
  swapped intermediate makes the final descriptor resolve to a different
  directory (or `ENOENT`), so its dev/inode differ and the command is refused.
  The author's `.git`→data case (an intermediate `.git` relinked) exercises this
  and leaks neither canary.
- **Reachability of a swap.** Landlock is inherited across fork, so a process
  the agent leaves running has the same grants as the agent. A path is swappable
  only when its parent is writable under those grants. The identity-checked
  grants (session folder, child folder, its `.git`, the borrowed store) are
  exactly the parents a session can write, and each now carries an identity; the
  wrapper refuses a mismatch. System grants' parents (`/usr`, `/dev`, node.toml
  extras outside the project) are not agent-writable, so they cannot be swapped
  from within the sandbox.
- **Grant construction, every site.** `Confinement::command`, `Sandbox::helper`
  (the node's git, `git.rs:322-347`), `Sandbox::wrapper` (the agent shell),
  graphify and the self-test: the session folder gets its pin's identity, the
  child's folder and `.git` the identity taken when the node made them, the
  borrowed store the identity `user_repo` accepted (and the node re-checks it in
  `Confinement` before adding the grant, so a store replaced earlier is left out
  and one replaced later is refused by the wrapper), the shadow/temp/graphify-out
  folders `Grant::made` (their identity now); only `SYSTEM_READ`, the devices and
  node.toml's `[sandbox] read`/`write` are `system`. None of the `system` paths
  comes from a session, an agent spec or the master.
- **No argument smuggling.** `push_args` passes `<flag> <spec> <path>` as three
  distinct argv entries; clap reads the flag's two values positionally, so a
  path starting with `-` is a value, not an option. `spec` is `system` or
  `<dev>:<ino>:<label>` with `label` a fixed English string from the node, no
  session input; `parse_pairs` splits on `:` with `splitn(3)`, so a `:` in a
  label is harmless, and `dev`/`ino` must parse as `u64`.
- **The agent starting the wrapper itself** gains nothing: Landlock only
  narrows, and `system` on a path outside its domain adds no access.
- **The `testing` module** is `#[cfg(feature = "testing")]` and the feature is
  enabled only by `crates/cli`'s `[dev-dependencies]` edge. The workspace uses
  `resolver = "3"`, which does not unify a dev-dependency's features into the
  normal build, so `cargo build --release -p harness` compiles the module out.
  There is no normal-dependency edge that turns it on.
- **The flaky `cancel_kills_process_tree`** is unrelated to the wrapper; the fix
  is in the test's `pid_alive`, which read a process in state `X` (dead, being
  reaped) as alive. Sound.

## 4. New findings

- **R1d-1. Low.** `[sandbox] read`/`write` extras are granted `system` (opened
  following links) and are checked at start only against the data directory
  (`Sandbox::new`, `exposes`), not against the workspace roots. An operator who
  lists an extra that lies inside a session's workspace (unusual, but nothing
  refuses it) makes that folder's parent agent-writable and `system`, so a
  left-running process could relink it and the next command's wrapper would
  follow the link. This is the concrete form of the author's stated "a grant
  wrongly marked `system`" gap. Fix: refuse a `[sandbox]` extra inside any
  workspace root, or give extras an identity too.
- **N-1. Note.** The identity is device plus inode. On ext4 an inode number can
  be reused after deletion. A session cannot delete the node-owned grants, and
  for its own folder a reused inode would hold the session's own content, so
  nothing is gained; recorded only because the check rests on inode stability.
- **N-2. Note.** `exposes` still canonicalizes the configured extra path at
  start, which follows a link present then; it is a first line only, and the
  identity/`system` split above is what actually bounds runtime swaps.

## 5. What I did not check

- I did not re-run the author's new sandbox tests (`snapshot_sandbox.rs`, the new
  cases in `sub_agent_sandbox.rs`); I read them and re-ran my own R1c-1 proof
  (now failing). I did not reproduce the inode-reuse note.
- `crates/node/src/git.rs` beyond the `helper` grant construction; `git/
  user_repo.rs`'s 14 new lines (the `Identity` it records) were read, not
  exercised.
- No full suite, `clippy`, `cargo deny` or a release `strings` check (I reasoned
  the feature out from the resolver instead).

## 6. Closing verdict for Gate 3

Across the five reads, the three areas stand as follows.

**A. The node link and the mirror.** The link findings L-1 to L-9 are closed
(fix review 1): a node can no longer move a chat to another user, name another
node's chat as a parent to pull its context, bloat the mirror with unbounded
strings, or stay linked through a revoke. The residuals that remain are the ones
`PLAN.md` already records (a copied key can freeze a session; no certificate
rotation). **Fit.**

**B. The approval path.** P-1 (the port-wide cookie) is closed with a login key
and per-URL links; P-2, P-3, P-7 are closed. W-1, the approval card that did not
show what would run, is closed by a card that shows the whole call, marks hidden
characters and bounds nothing silently. The residuals are Low: a cookie alone
still takes a socket place and is a liveness oracle (R2-1), and the card lets a
person approve an unexpanded middle if they choose to (W-4). **Fit after those
Low items, which do not block.**

**C. The sandbox and the tools.** The three proven escapes of the first review
(S-1, S-2, S-3) and the two I found in the first rework (R1-1, R1-2) are closed,
and the fixes introduced no lasting hole: R1b-1 to R1b-4 are closed, and R1c-1,
the grant that followed a link, is closed here by binding every grant to a
checked inode. What remains are Low and Note items (R1d-1, the memory and
blob-read bounds, the operator-config edges) and the residuals `PLAN.md` records
(a secret split or encoded past redaction; UDP and pathname sockets before
Landlock ABI 9; a crafted object git must parse). **Fit once R1d-1 and the
remaining Low items are handled or accepted.**

**Overall:** once this branch merges and the handful of Low and Note items above
are dealt with or explicitly accepted by the owner, I would call all three areas
fit to pass Gate 3. The design that made that possible is the one the project set
out to build: the node decides, the master never runs a tool, the data directory
is closed to sessions, and nothing the model writes is trusted by the code that
runs git. Each of those I could confirm from the code at the end of this review.

## 7. Proof

No new passing proof: the finding is closed. The regression is my existing
`review_r1c_1_…` (fix review 4's patch), which passed on `6cb4d55` and **fails on
`536492e`**:

```bash
git checkout --detach fix/R.1-sandbox-findings   # 536492e
git apply docs/security-review/proofs/2026-11-gate3-fixes-4.patch
cargo test -p harness --test sub_agent_sandbox review_r1c_1   # now fails (needs Landlock)
```

## 8. The three things I am least sure of

1. R1d-1's reachability: it needs an operator to list a `[sandbox]` extra inside
   a session's workspace, which is unusual; I did not build it.
2. Inode-number reuse (N-1): I argued it yields nothing, but did not test a
   forced-reuse scenario.
3. The release `strings` claim for the `testing` feature: I reasoned it out from
   `resolver = "3"` rather than building the release binary and checking.
