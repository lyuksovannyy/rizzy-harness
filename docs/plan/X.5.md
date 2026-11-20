# X.5

## Adds

Input during a turn

## Safeguards and abuse tests

Persisted with an idempotency key before acknowledgement; delivered at a step boundary; never kills a tool; a dropped message gets a receipt.
Tests: sent twice, stored once; restart before delivery, delivered once; a running `bash` is not interrupted. `01` §6.10.
**Done, without the idempotency key and the receipt** (2026-11-14): a prompt while a turn runs is recorded at once as the turn's `user_message` with `queued: true` and acknowledged (the master answers 200, not 409); the loop reads unread ones before each model request, after the step's tool results, recording `queued_delivered { seqs }` where it read them, and the log rebuilds them there; one that came after the model's last step, or after an error, starts the next turn (a callback turn reads none, the person's turn after it does);
Stop or a restart leave them for the person's next turn (from the log); a summary cutting between a message and its reading carries it; a message naming another agent, to a sub-agent's chat or during parallel answers is refused with the reason (review ST-1 to ST-10); at most 8 wait (the ninth gets the old refusal); answers to questions and parallel answers still wait.
The UI keeps the reply box open and marks the message "sent while working" until it is read.
Tests: `node/tests/queued.rs` (once at the next step, the next turn, Stop, the cap, an approval card, a restart), `agent-native/src/queued_tests.rs` (the request, URL provenance, the rebuild), the fold fixture, `composer.test.tsx`, `conversation.test.tsx`, `web/e2e/steer.cjs`.
Not done: a client retry after a lost acknowledgement stores the message twice (no idempotency key; the UI does not retry), and nothing tells the person a message was dropped, since none is.

## Invariants

One writer

## Podman / Node

No / Yes

## Depends

A.1

## Size

**M**

