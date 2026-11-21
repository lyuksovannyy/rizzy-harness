# Forking a chat

ADOPTION_PLAN C.2 and X.48 (part 1: the conversation), PLAN.md T92, README "Fork a chat".
Part 2, a checkout of its own for the fork's files, is a later change; until then both chats share one folder.

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
4. For each `background_started` in `1..=T` with no `background_ended` in it: a `background_ended` with reason `forked` (never calls back; the model's note says the command belongs to the chat this one was forked from).

`harness_core::fork` is the one rule: `check` (a cut: the node and the master run it on their own copies of the log), `build` (the events), `inherited(through, seq)` (`1 < seq <= through`: the first event is the fork's own creation).

## What a copy is not

- It costs nothing: the view folds (Rust and TypeScript) and `store/usage.rs` skip it; the meter still shows the context the model will be sent.
- It grants nothing: grants are bound to the parent's folder, so the node and the view take them from the fork's own events only.
- Its checkpoints cannot be restored in the fork (the refs are the parent's): the view marks them `inherited`, `Session::restore` refuses, the page shows "changed N files (before the fork)".
- Its reads are not the fork's: the seen table starts empty.
- It is not indexed again: `chat_fts` has the parent's text under the parent; the fork's title and its own messages are indexed.
- It is not work another chat did: `others.rs` (the T57 reads of other chats' logs by time) and a deleted chat's write tombstone skip copies.
- It is not a second effect on the master: `Master::apply` mirrors a copy, shows it to the owner's browsers, and does nothing else (`forks.rs`).
- What waited for a callback turn in the parent is not the fork's: the divider clears it (`waiting_ends`, `events_since_last_person_turn`).

## The lineage

`session_forks (session_id, parent_id, through_seq, created_ms)`, migration 3 (recorded `crates/store/schema/3.sql`), written in the transaction that stores the fork's first event, from `session_created.forked`.
No foreign key; deleting the parent deletes no fork.
`through_seq` is what the store and the master read to tell a copy from an event.

## Wire and API

`POST /api/sessions/{id}/fork {turn_id}` (`crates/master/src/http/fork.rs`): owner only (404 for everyone else), then `harness_core::fork::check` on the mirror, then `session.fork {req_id, session_id, parent, turn_id, title}` (capability `fork`) to the chat's node, which checks again and writes the fork (`Store::write_fork`: events and the images its copied events name, one transaction) before publishing the events in order and acking.
The master mints the new id and records the create intent first, and after the ack claims the chat for the owner and links it to the parent's workspace and project, as a create does.
The ack waits four times as long as other commands': the node sends the whole copy first.

## Files

Both chats share the parent's folder.
The page says so; the T57 rules keep one chat's undo from reverting the other's work.
`Session::fork_folder` is the one place that decides the fork's folder.
