# B.6

## Experiment

Test D13: rewind, restart the node, open the session

## Outcome that changes a decision

A disagreement puts an explicit head into C.2 and N.5; otherwise it is dropped.
**Outcome (2026-11-21, with the conversation rewind of X.46):** no disagreement, so no head.
A rewind is one event in the chat's own log (`conversation_rewound`), and the view, the model's history and the node's state are folds over the log, so a restart changes none of them.
`crates/node/tests/rewind.rs` `a_rewind_survives_a_node_restart` rewinds, restarts the node on the same database, and checks that the log is the same, that the fold lists the same rewind and marks the same items, that the model's history is the same bytes, and that the next turn is sent the chat without the rewound turn.
What a restart does not keep is memory-only: the agents a message called and had not yet asked (`peers_skipped`, written at the node's start) and the unread queued messages, which are rebuilt from the visible log.

## Size

**S**

