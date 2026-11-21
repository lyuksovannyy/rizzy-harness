# X.48 **sec** (T92)

## Adds

Fork a chat at a finished turn, part 1: the conversation (research round: every harness forks by copying into a new chat; ours copies the log whole, at the same `seq`, so nothing is lossy and no second code path reads it).

## Safeguards and abuse tests

A new chat on the same node begins with the parent's events `2..=T` at the same `seq` under a new id, then `session_forked`, then a `background_ended` (reason `forked`) for each command running at the cut; its first event is the parent's `session_created` with the fork's title, the folder the node chose and `forked {parent, through_seq}`.
One rule in `crates/core/src/fork.rs` (`check`, `build`, `inherited`); the view folds, the browser fold and the shared fixture (`fork.json`) honour it.
A copy costs the fork nothing, grants nothing, and its checkpoints cannot be restored in the fork.
`session.fork` (capability `fork`) writes the fork's events and images in one transaction (`Store::write_fork`); `POST /api/sessions/{id}/fork` is owner only (404 for anyone else, admins and the API token included).
`Master::apply` runs one early branch: a copy is mirrored and shown and nothing else happens (`crates/master/src/forks.rs`).
The lineage is `session_forks`, migration 3 with its own recorded `schema/3.sql`.
Tests: `core/src/fork.rs`, `core/tests/fork_fixture.rs`, `store/tests/forks.rs`, `store/tests/migrations.rs`, `node/tests/fork.rs` (a restart right after the fork, the parent still running, a fork of a fork, of a compacted chat, after a rewind, a folder the node no longer allows), `master/tests/it/fork_http.rs` (one test per side effect), `web/src/fork.test.ts`, `web/src/components/forkchat.test.tsx`.
Fix round: grants and images are proven on the node; "earlier chats" and hand-off notes read a fork's own events only (`harness_core::fork::own`); the lineage is read before an event is taken and a failed or early read is never cached; the guard has a test per effect that fails without it; a copied event leaves the fork's row state alone; a late-acked fork (and an ordinary create) still joins its project and the model defaults; a cut inside a message that called several agents is said in the fork (`peers_skipped`, reason `forked`); "Fork from here" shows on hover and focus.
Part 2 (the fork's own folder): `session.fork` carries `checkout: "own" | "shared"` (capability `fork_own`); an own copy is a clone of the parent's repository made from outside it in the node's projects folder (`git/fork_copy.rs`, `fork_checkout.rs`), and the node's git never runs in the parent's repository (the hostile-repository test, `git_fork_tests.rs`, fails with a marker if the clone is replaced by `git worktree add` or a plain clone).
Its files are the cut's checkpoint, its index the branch's commit; ignored files, nested repositories, stashes and the parent's hooks and config are not copied.
The folder is built in the node's staging folder (swept at every start) and moved in with a no-replace rename; a failure leaves no chat and no folder; deleting the chat keeps the folder.
`session_created.forked.own_folder`, the page's choice (`ForkChoice.tsx`) and the fork titles "X (fork)", "X (fork 2)" are part of it, and `history` search leaves the running turn out.
Tests: `node/src/git_fork_tests.rs`, `node/src/fork_checkout.rs`, `node/tests/fork_checkout.rs`, `master/tests/it/fork_http.rs`, `store/tests/forks.rs`, `agent-native/src/history_tests.rs`, `web/src/components/forkchoice.test.tsx`.

## Invariants

One writer; the node decides; chats are private to their owner; nothing the model writes is trusted

## Podman / Node

No / No

## Depends

C.2, X.46

## Size

**L**
