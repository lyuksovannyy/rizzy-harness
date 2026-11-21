# Forking a chat

ADOPTION_PLAN C.2 and X.48, PLAN.md T92, README "Fork a chat".
Part 1 is the conversation; part 2 (below, "Files") gives a fork a folder of its own where the person chooses it.

## What a fork is

A new chat on the same node whose log begins with a copy of its parent's events up to the end of a finished turn, at the same `seq`, and then goes its own way.
The parent is only read.

Why a copy at the same `seq`, not a seed: every other harness forks by copying into a new chat (research round, `.claude/handoff-2026-11-15/research-rewind-fork/findings.md`), and a lossy import (`history_imported` batches, the note's `lossy` flag) would need its own path through the model rebuild, the folds and every `seq` a payload names (`history_summarized.through_seq`, a rewind's `message_seq`, `queued_delivered.seqs`, `mention.message_seq`).
With the same `seq`s nothing is renumbered and nothing has a second reader.

## The log of a fork

1. Event 1: the parent's `session_created` with the fork's title (the master sends the parent's current title and " (fork)"), the folder the node chose (`Session::fork_folder`, the parent's, checked as at a create), and `forked {parent, through_seq: T}`.
   `parent_session` is not set: it means "sub-agent of" and drives owner inheritance, search exclusion, usage skipping, the delete cascade and the sub-task banner.
2. Events `2..=T`: the parent's, as they were (same `ts_ms`), with the session id replaced.
   `T` is the `turn_ended` of the chosen turn: never inside a turn.
3. Event `T+1`: `session_forked {parent, through_seq, turn_id}`, the divider.
4. When the cut is inside a message that called several agents (`user_message.triggers`), a `peers_skipped` with reason `forked` names the agents that had no turn at the cut (`mention::unasked` on the copy).
   The node's start-up check reads the same rule, so without this note it would have appended "the node restarted" to the fork.
   A refusal of such a cut was rejected: it would lock out cuts that are legitimate, and the page would need a twin of the rule to hide the button.
5. For each `background_started` in `1..=T` with no `background_ended` in it: a `background_ended` with reason `forked` (never calls back; the model's note says the command belongs to the chat this one was forked from).

`harness_core::fork` is the one rule: `check` (a cut: the node and the master run it on their own copies of the log), `build` (the events), `inherited(through, seq)` (`1 < seq <= through`: the first event is the fork's own creation).

## What a copy is not

- It costs nothing: the view folds (Rust and TypeScript) and `store/usage.rs` skip it; the meter still shows the context the model will be sent.
- It grants nothing: grants are bound to the parent's folder, so the node and the view take them from the fork's own events only (`Session::load`; `a_chat_grant_is_not_inherited_by_the_fork` runs the same command in both chats).
- The approval mode is the parent's at the cut (the copied `session_created` and `approval_mode_changed` events fold to it), clamped by `node.toml` as at any create.
- Its checkpoints cannot be restored in the fork (the refs are the parent's): the view marks them `inherited`, `Session::restore` refuses, the page shows "changed N files (before the fork)".
- Its reads are not the fork's: the seen table starts empty.
- Its images are the fork's own: `Store::write_fork` copies the `session_images` rows the copied messages name, under the fork's id, in the same transaction, so deleting the parent keeps them and the model is sent them on the fork's first turn.
- It is not indexed again: `chat_fts` has the parent's text under the parent; the fork's title and its own messages are indexed.
- It is not work another chat did: `others.rs` (the T57 reads of other chats' logs by time) and a deleted chat's write tombstone skip copies.
- It is not a second effect on the master: `Master::apply` mirrors a copy, shows it to the owner's browsers, and does nothing else (`forks.rs`).
- What waited for a callback turn in the parent is not the fork's: the divider clears it (`waiting_ends`, `events_since_last_person_turn`), and the guard keeps a copied `background_ended` from starting the watcher at all.
- It is not the fork's row state: while the copy arrives `Store::append` moves only `head_seq` and the event row (not `running`, pending approvals, `finished_seq`, asks, attention or the last-active time); `session_forked` sets the row once, idle, with the divider's time.
- It is not the fork's note: the "earlier chats" list and hand-off notes read a fork's OWN events only (`harness_core::fork::own`, the one rule; the store's hand-off log and `earlier_chats`, and the node's `handoff`, use it), so a fork with no turn of its own is not listed and is due no note, and the parent and its fork never show the same reply.

## Incognito

A fork of an incognito chat is incognito (the copied `session_created` keeps the flag): not indexed in `chat_fts`, so not found by search or recall, and not in "earlier chats" (`an_incognito_fork_is_not_indexed_nor_listed`).

## Rewind in a fork

A rewind of the fork to a message it copied works for the conversation (the rewind event is the fork's own and its range hides copied events at their seqs).
"Rewind files" to such a message is refused whole, with nothing written: the folder would go back to before a copied turn, whose snapshot belongs to the parent's chat (`restore_checked` refuses a copied checkpoint; `a_fork_rewinds_to_a_copied_message_but_not_with_files`).

## The lineage

`session_forks (session_id, parent_id, through_seq, created_ms)`, migration 3 (recorded `crates/store/schema/3.sql`), written in the transaction that stores the fork's first event, from `session_created.forked`.
No foreign key; deleting the parent deletes no fork.
`through_seq` is what the store and the master read to tell a copy from an event.

## Wire and API

`POST /api/sessions/{id}/fork {turn_id}` (`crates/master/src/http/fork.rs`): owner only (404 for everyone else), then `harness_core::fork::check` on the mirror, then `session.fork {req_id, session_id, parent, turn_id, title}` (capability `fork`) to the chat's node, which checks again and writes the fork (`Store::write_fork`: events and the images its copied events name, one transaction) before publishing the events in order and acking.
The master mints the new id and records the create intent first, and after the ack claims the chat for the owner and links it to the parent's workspace and project, as a create does.
The ack waits four times as long as other commands': the node sends the whole copy first.
If the ack is late anyway the fork still appears, and the late claim (`claim_late`) gives it what the normal path gives: owner, workspace, the parent's project and the follow-the-defaults mark, all from the create intent the master recorded before it sent the command (an ordinary create carries the same two fields).
The caller still gets the timeout error and finds the fork in the list.
`Master::apply` asks for the fork's lineage BEFORE it takes an event, so a failed read stops the event (the node sends it again) and is never cached; "not a fork" is cached only for a chat the mirror holds.

## The page

"Fork from here" is as quiet as the copy action under an answer: hidden until the turn's end line (or the answer above it) is hovered or the link has keyboard focus, always shown on touch and while it has an error to show (`.fork-line`, `.turn-end` in `styles.css`).

## Files

`session.fork` carries `checkout`: `shared` (part 1: the fork works in the parent's folder; the page says so, and the T57 rules keep one chat's undo from reverting the other's work) or `own`.
`Session::fork_folder` decides the shared folder; `Session::own_copy` and `crates/node/src/fork_checkout.rs` decide the own one.

### An own copy

A clone of the parent's repository, made from outside it, in the node's projects folder (`[projects] dir`, where forge clones go), named `<repository name>-fork-<short id>`, with a branch `fork/<short id>`.
The fork's folder is the same sub-folder of the copy that the parent worked in.

**Why not `git worktree add`, and why not a plain clone of the parent.**
`git worktree add` runs inside the parent's repository: it reads that repository's config (shared by all worktrees) and runs its `post-checkout` hook.
A `git clone --no-local` of the parent was the first design; its `upload-pack` runs in the parent's `.git` as the node's user and follows every link an agent planted in `refs/` and `objects/` (and in `config`), so a link to another repository on the disk put that repository's history into the fork.
An agent in a sandboxed chat can write all of that, and the node is not sandboxed.
So git never opens the parent.
Stage 1 is the harness's own walk (`git/fork_sanitise.rs`), with file descriptors and `O_NOFOLLOW`, into the node's staging folder.
It copies `HEAD`, the loose refs, `packed-refs` and `shallow`, each parsed and written again from the lines that parse (a commit id, `ref: <name>`, git's `# pack-refs` header, `<id> <refname>` and `^<id>` lines; a ref name must pass `git check-ref-format`, checked in code); a loose ref or a line that does not parse refuses the copy, by name or by line number and never by content.
Of `objects/` it copies only what stage 2's clone needs: the loose objects, and the `.pack` and `.idx` of each pack, as new files (never a link, never a hard link).
It leaves out `objects/info/*` (the alternates files are only checked, and refuse the copy when they hold anything), `commit-graph`, `multi-pack-index`, bitmaps, reverse indexes, keep files and temporary files: git rebuilds or ignores them, and fewer hostile parsers run in stage 2.
A `.promisor` pack (a partial clone) refuses the copy.
It reads `config` and `info/exclude` as capped data.
Anything that is not a plain file or folder in what it walks (a link, a FIFO, a socket) refuses the whole copy with a sentence naming the path; so do alternates, sha256 objects and reftable refs.
It copies no config, hooks, worktrees, modules, index, logs or grafts.
Stage 2 is `git clone --no-local --no-checkout --template= -- <sanitised copy> <dest>` by the hardened runner, in the node's sandbox jail when it has one (only this copy's folders visible read-write; without a jail it runs hardened and unjailed), and the sanitised copy is deleted.
Bounds: `clone_max_mb` bytes, 1,000,000 files and folders, 32 levels, 1 MiB each for the config and `info/exclude`, 512 bytes for a ref, 16 MiB each for `packed-refs` and `shallow`, `clone_timeout_s` per stage.
The room check before the copy is a first pass of the same walk (descriptors, `O_NOFOLLOW`, the same bounds, cancellable), not a path walk.
The reasoning, the quoted documentation, the minimum git version and the proof are in `docs/threats/T92.md`.

**What the copy is.**
The clone's own config: remote `origin` is the parent's path (where the fork came from), and its `pushurl` is a path that does not exist (`/nonexistent/no-push--this-fork-does-not-push-into-its-parent`), so `git push` from the fork fails with git's own message naming it.
Bringing a fork's work back into the parent is not built yet.
Anything the person does with git by hand in either folder runs git on a repository an agent could write to; the git(1) SECURITY section says "it is not safe to run Git commands in a .git directory (or the working tree that surrounds it) when that .git directory itself comes from an untrusted source".
In particular `git fetch <fork folder>` makes git read the fork's `.git` as the person, and an agent that worked in the fork could have linked other files into it, so another repository's history could land in the parent.
The push block (`pushurl`) guards the person's habit only: the fork's agent can edit its own config.
Plus remote `upstream` when the parent had a usable `origin` URL (read as data, validated; the user name and password of an https URL and the password of any other URL are dropped, and a URL with a query or a fragment, or an `http://` one, gives no `upstream`).
The branch is at the commit the parent's HEAD was on at the cut: the log's `head_moved` of the last turn at or before the cut, else the HEAD the first later turn that moved it found, else the parent's HEAD now (and the chat says so).
The working files are the cut's: the `after` checkpoint of the last turn at or before the cut that has one, written from the shadow by the checkpoint writers; with none, the branch's commit as it is.
The index is the branch's commit, so what the parent had uncommitted at the cut is uncommitted in the fork.
Not copied: files git ignores, nested repositories and submodule contents, stashes, the parent's hooks and local config.
The parent's `info/exclude` is copied (patterns), so the same files stay ignored.

**Refused, with a sentence that says which and that "the same folder" is possible:** a chat with no folder, a folder that is no git repository (or a `.git` the node does not copy: a file, a link anywhere in what is copied, a `commondir`, borrowed objects, sha256 objects, reftable refs), no projects folder, no checkpoint store, a pruned checkpoint, a git below the minimum, no room on the disk, a copy over `clone_max_mb` or `clone_timeout_s`, more folders than `projects_per_hour`.
Nothing is written before the checks; a failure or a cancelled copy removes the build folder and the sanitised copy at once, and a kill (or a cancelled request) between the move and the chat leaves a marker, and the next start removes the unused folder only when no chat in the node's store works in it and nothing in it is newer than the copy (otherwise it keeps it and logs its name); the chat is written only after the folder exists, and a copy whose chat could not be written is removed.

**Running.**
The copy runs in the new chat's lane (nothing else uses it yet), off every other chat's, behind a small limit on copies at once (two; the rest wait).
The master waits for the ack as long as a node may take to copy; the page shows "Making the copy…" and does not close the choice meanwhile, and opens the chat when the node answers.
A refusal shows in the choice, which stays open with "In the same folder" as the likely next click.

### The choice in the page

"Fork from here" opens a small inline choice (modelled on the rewind choice) where the chat is agentic and its node announced `fork_own`: "In its own copy of the repository" (the default; where it goes, and that ignored files are not copied) or "In the same folder".
Where the node cannot, the link forks at once into the same folder, as in part 1.
The page cannot tell beforehand whether the folder is a git repository, so the default is the own copy, and a refusal flips the default.
The fork's header shows its folder as any chat's; the note under it says "shares its folder" only for a shared fork and, for an own copy, where it is and what was not copied; `session_created.forked.own_folder` says which.
Deleting a fork's chat never deletes its folder (the files are the person's); the delete dialog says where it is.

### Titles

A fork's title is the parent's with a trailing " (fork)" or " (fork N)" taken off, then " (fork)" for the first fork of the original chat and " (fork 2)", " (fork 3)" ... after it, counted over the whole family (`session_forks`), so a fork of a fork is the next number of the same original.
