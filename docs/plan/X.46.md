# X.46 **sec** (T90)

## Adds

Rewind a conversation to an earlier message (`docs/plan/X.21.md` left it open: "editing the message first, cutting the failed attempt out of what the model sees, retrying an older turn, putting the prompt back in the composer"; Claude Code's Esc Esc, Codex's backtrack, OpenCode's `/undo`, Cline's Reset Chat all have it).

## Safeguards and abuse tests

**Server half, built on branch `feat/conversation-rewind`** (docs/rewind.md, PLAN.md T90, README "Rewind"): two event kinds, `conversation_rewound {message_seq, turn_id, files}` and `conversation_rewind_undone {rewind_seq}`, that the folds honour; nothing is deleted, `seq` stays contiguous, no schema change.
One rule in `crates/core/src/rewind.rs` (ranges, `check`, `check_undo`, the events the model reads); the model is sent the chat without the rewound turns (turns before the cut byte for byte), the first message after a rewind that kept the files opens with one harness line naming them, what the agent read in a rewound turn is not read; the view marks rewound items and lists the rewinds; retry and the hand-off note treat rewound turns as absent.
The node runs it under the turn slot (`session.rewind`, `session.rewind_undo`, capability `rewind`); the optional files part is the existing restore and a refusal of it writes nothing; refused while a turn, `/compact`, background commands or sub-agents run.
`POST /api/sessions/{id}/rewind` and `.../rewind/{seq}/undo`, owner only (404 for anyone else, admins included).
The web fold marks rewound items and the chat draws them muted under one line.
Tests: `core/src/rewind.rs`, `core/tests/fold_fixture.rs`, `agent-native/tests/rewind.rs`, `node/tests/rewind.rs` (with the restart of B.6), `master/tests/it/rewind_http.rs`, `protocol/tests/rewind_wire.rs`, `cli/tests/background.rs` and `background_agents.rs` (the refusals while background work runs), `web/src/rewind.test.ts`.
Left for the web pull request: per-message "Rewind to here", the dialog (conversation, files or both, and what is not undone), the prompt back in the composer through `put()`, the collapsed "N turns rewound" block with Undo.
Search, recall and the earlier-chats context skip rewound text (the chat's `chat_fts` rows are rebuilt from its visible log when a rewind or an undo is appended); the undo brings it back.
Left for later: fork; editing a message in place.

## Invariants

One writer; the node decides; chats are private to their owner; nothing the model writes is trusted

## Podman / Node

No / No

## Depends

X.21, X.42

## Size

**M**
