# Rewinding a conversation to an earlier message

Row [X.46](plan/X.46.md), threat row [T90](threats/T90.md), README "Rewind".
The files half of the idea exists since X.42 ("Rewind files to before this turn"); this note is about the conversation.
The design came from the research in `research/` and the comparison of Claude Code, Codex, OpenCode, Pi, Cline, Zed, Warp, goose, OpenHands and Hermes (the table at the end of this note).

## The decision

A rewind is **a new event in the same chat that every fold honours**.
Nothing is deleted, `seq` stays contiguous, one session actor writes it, the UI stays a fold over the log, and a rewind can be undone.
It is not a truncation (the master's mirror would hold content the node no longer has, the next append would collide with it and mark the chat diverged) and not "a rewind is a new chat" (that is a fork, a later row).
No SQLite schema changed: two event kinds only, so existing data directories keep working.

## Events

| Event | Payload | Meaning |
|---|---|---|
| `conversation_rewound` | `message_seq, turn_id, files` | The person took the chat back to their message at `message_seq` (the turn it opened is `turn_id`). Every event with `message_seq <= seq <` this event's own seq is **rewound**. `files`: the files part was asked for and done. |
| `conversation_rewind_undone` | `rewind_seq` | The range of the `conversation_rewound` at `rewind_seq` counts again. |

`crates/core/src/rewind.rs` is the one place that computes it: `rewound(log)` is the union of the ranges of the rewinds not undone, `Ranges::holds(seq)` asks whether an event is in it, `check` and `check_undo` are the rules below.
The Rust fold, the model's rebuild, the seen table, the retry rule, the hand-off note and the node's checks all read it.
`web/src/fold.ts` is a second implementation of the fold, pinned by the shared fold fixture (`crates/core/tests/fold_fixture.rs`, `web/src/__fixtures__/session.json`).

## What "rewound" means, reader by reader

1. **The model** (`agent-native`: `rebuild_spans` and everything built on it, and the turn loop): events in a rewound range are not there, and neither is the `turn_started` of the turn the person went back to.
   That includes their context blocks (a standing section a rewound turn carried is sent again when next due), queued messages, shortenings and withheld images or thinking, and the end of a command a rewound turn started.
   `rewind::visible(log)` is the list the model reads; it keeps `checkpoint` events (state, and the source of the file names below).
   Turns before the cut are rebuilt byte for byte as they were sent, signed thinking prefixes included.
2. **The first message after a rewind that kept the files** opens with one harness line: the conversation was taken back, files the removed turns changed were kept on disk, read a file again before editing it, and the names (at most 20, the rest counted), each quoted on a line of its own as data.
   It is built from the log (`rewind::opening_lines`, `next_opening_line`), so a rebuild writes the bytes the turn was sent with.
3. **What the agent has seen** (the read-first table behind `edit` and `write`): rebuilt from the visible history at every turn's start, so a file read in a rewound turn is not read any more and an edit by tag or by old text needs a fresh read.
4. **The view**: items made from a rewound range carry `rewound` (the seq of the earliest rewind that hides them); `rewinds` lists each rewind with `seq, message_seq, turn_id, turns, files, undone, can_undo`.
   Usage, checkpoints, background commands, chat grants, the approval mode, the step limit, the agent and the task list fold as before: what was spent is spent.
5. **The retry rule and the hand-off note**: rewound turns are absent; a chat rewound since a turn began has nothing of it to retry; the files a rewound turn changed and nobody put back are still in the note.

## The rule

`rewind::check(log, message_seq)` refuses: a message that is not the person's or did not open a turn (a message sent while an answer ran is read into that answer; a retry has no message of its own, its target is the retried turn's); one already rewound (also anything inside a rewound range); one the latest summary holds ("this chat was compacted after that message": the summary would still hold its text); a sub-agent's chat.
The master adds a routine's own first message.
The node adds, under the turn slot: a turn running or being stopped, a `/compact`, a hand-off note being written (it is cancelled), the agents of one message still taking turns, and **any background command or sub-agent of the chat running, or an end not recorded yet** (`Session::restore` counts commands only; a sub-agent writes into the folder as well).
`rewind::check_undo(log, rewind_seq)`: the event is a rewind, not undone, and no message, turn or summary came after it.

## Files

With `files` the node first runs the existing restore with `rewind = true` for the earliest turn of the range that has a checkpoint and is not undone already.
If that is refused (another chat's work in the folder without `force`, a changed file, the folder no longer allowed, a pruned checkpoint) nothing is written and the person gets the reason.
No turn with a checkpoint: nothing to put back, and `files` is recorded false.
Undoing a rewind never touches files; "Put back what this undo replaced" on the restore's block does that.

## Wire and API

Capability `rewind`; `session.rewind {session_id, message_seq, files, force}` and `session.rewind_undo {session_id, rewind_seq}`, every field named by every sender.
`POST /api/sessions/{id}/rewind` with `{message_seq, files, force}` and `POST /api/sessions/{id}/rewind/{rewind_seq}/undo` (`crates/master/src/http/rewind.rs`): the chat's owner only (the API token only for a chat nobody owns), 404 for anyone else, admins included; the master checks its mirror with the same rule, the node decides again.
The model has no tool for this and nothing a model writes can trigger it.

## Not changed, on purpose

Search, recall and the "earlier chats" context do not read the rewound text: `Store::append` rebuilds that one chat's `chat_fts` rows from its visible log (`rewind::rewound`) in the transaction that appends a `conversation_rewound` or `conversation_rewind_undone`, and `earlier_chats` skips rewound events (note, summary, reply) by the same rule; the undo brings the text back. Usage is not reduced; memory facts saved, board changes, published artifacts, commands run, network calls, files outside the folder and sub-agent chats started by rewound turns are not undone; `others.rs` (T57) still reads rewound turns to say who changed a file; the browser still receives every event of its owner's chat.
The web dialog (conversation, files or both), the prompt back in the composer, the collapsed "N turns rewound" block with Undo, forking and editing a message in place come in later changes; until then the chat shows rewound items muted under one line saying so.

## Comparison

| Harness | Rewind storage | Files | UI |
|---|---|---|---|
| this | one event in the same log, undoable until the next message | the existing checkpoint restore, optional, refused as a whole | per message, in a later change |
| Claude Code | tree by `parentUuid`, nothing deleted | its own edit tools only | Esc Esc: code, conversation or both |
| Codex | new rollout file, pointer swapped | none | Esc Esc, prompt back in the composer |
| OpenCode | marker, rows deleted at the next prompt | optional flag | `/undo`, `/redo` |
| Cline | new session seeded with the kept prefix | separate "Reset Code" | "Reset Chat" and "Reset Code" |
| Zed, Warp, goose | truncate in place (Warp saves a backup fork first) | per-edit revert or none | click to edit; Warp confirms |
