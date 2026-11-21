# Pinned messages, and the row under an answer

Row [X.51](plan/X.51.md), threat row [T93](threats/T93.md), README "The row under an answer" and "Pinned messages".

## The decision

**A pin is a bookmark for the person and nothing else.**
Claude Desktop and ChatGPT use "pin" for things that change what a model reads (a pinned file, a pinned instruction); here the owner chose the other meaning: mark a message in the chat, list the marks under "Pinned", jump back to one.
So a pin is one new event in the same chat, `message_pinned`; no reader of the conversation looks at it, and no SQLite schema changed.

## Event

| Event | Payload | Meaning |
|---|---|---|
| `message_pinned` | `message_seq, pinned` | The owner bookmarked (`pinned`) or un-bookmarked the `user_message` or `assistant_message` written at `message_seq`. The latest event for a seq wins. |

`crates/core/src/pins.rs` is the one place that computes it: `pins(log)` lists the pins in log order (seq, who wrote it, its first 120 characters on one line, its turn, when it was written); `check(log, message_seq, pinned)` is the rule the master and the node both apply.
A pin that names no earlier message of its log counts for nothing, in the fold as in `check`.
`check` refuses a seq that is not a message of the person's or the assistant's in this log (an assistant step with no text is not one), a message a rewind hides, pinning what is pinned, unpinning what is not, and a pin beyond **50 per chat** ("this chat has 50 pinned messages; unpin one first").
The count includes pins of rewound messages: an undo brings them back, and an undo is still possible after a pin.
The Rust fold (`view.rs`) and `web/src/fold.ts` both list the pins (`view.pins`, without those a rewind hides) and give an assistant message the seq of its final event (`seq`), which is what the page sends; the shared fold fixture pins them to each other.

## What reads it, and what does not

The model is sent nothing: the rebuild in `agent-native`, the `history` tool, the hand-off note, search and recall ignore the kind (`crates/node/tests/pins.rs::the_model_is_sent_nothing_about_a_pin`).
The scrubber's table lists it with no free text.
A fork copies its parent's events up to the cut whole, so a fork **inherits the pins** made before the cut, at the same seqs; a copy has no side effect to skip.

## Who may pin

Only the chat's owner, on their click: `POST /api/sessions/{id}/pins` with `{message_seq, pinned}` (`crates/master/src/http/pins.rs`) answers anyone else, admins and the API token on an owned chat included, with the 404 of a chat that does not exist, the same bytes.
A sub-agent's chat and a routine's chat the caller owns can be pinned in.
The master checks its mirror with `pins::check`, then relays `session.pin` (capability `pin`; a node without it is told to update); the node decides again against its own log.
The node appends through the session's one writer (`Session::append`, under the head lock), so `seq` stays contiguous.
Unlike a rewind it does not claim the turn slot: it is accepted while a turn runs, because it changes nothing the model reads.
Two pins at once cannot pass the limit: the check and the write hold the session's `bg_gate`.

## The page

The row under a finished turn's final answer reads Copy, Fork from here, Pin, when the turn ended and the tokens it used.
The time is relative ("just now", "5 min ago", "2 h ago", "yesterday 14:02", then a date) with the complete date, time and zone as its hover text; one timer for the page moves every shown time on.
The tokens are the turn's final usage, **input (cache reads and writes included, as providers count it) and output**, as recorded on the turn's end; its foreground sub-agents are already in that number, because the node adds a delegate result's usage to the parent's turn total (the rule of `crates/store/src/usage.rs`).
The detail (hover, click or keyboard focus; Escape or a click outside closes it) lists the models with their input, output and cache tokens and the cost where prices are set, then each foreground sub-agent of the turn read from its own chat (the same access check as opening it) as a share of the total, never added to it, then the total.
A background sub-agent's cost comes in at its own end, not in a turn, so it is not listed.
A turn copied into a fork shows what the parent spent, marked "before the fork", and counts as zero in the fork's totals as before.
"Pinned (n)" in the chat's header opens the list: each pin's first words, who wrote it and when; choosing one scrolls to the message and marks it for a moment; the owner can unpin from the list.
