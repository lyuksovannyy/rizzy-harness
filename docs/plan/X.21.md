# X.21

## Adds

Retry a turn that failed (research gap list: "rewind to a message: files + conversation + prompt back in the composer; Retry on a failed turn"; this row is Retry only)

## Safeguards and abuse tests

**Done on branch `feat/retry-turn`** (PLAN.md T69, README "Retry"): a Retry link on the failed last turn's end line (`error`, `interrupted`, `stopped`; not after the person's Stop, not after an answer) runs the same message again: `POST /api/sessions/{id}/turns/{turn}/retry` (owner only, a prompt's checks; refused for a sub-agent's chat and a routine's own run) relays `session.retry`; the node checks its log under the turn slot (`harness_core::retry::check`: the last turn, a person's, nothing after it, its changes not undone) and starts a new turn whose `turn_started.retry` names the failed one, with no second `user_message`, the chat's current agent, model and settings and the master's context as for a prompt.
The model gets the message once, the failed attempt's steps (its calls ran: they stay, so nothing is repeated blindly), then one harness opening saying so with the attempt's error quoted (after OpenClaw's restart resume prompt and DeepSeek's "outcome unknown" closers; research round 2), rebuilt byte for byte from the log; the retry has its own checkpoint (Undo of it undoes it only).
Never automatic (the loop's `model_retry` is).
Tests: `node/tests/retry.rs`, `master/tests/it/retry_http.rs`, `core/src/retry.rs`, `agent-native/tests/turn_compaction_review.rs` (`a_retry_after_a_failed_turn_rebuilds_at_every_step`), `store/tests/search.rs`, the fold fixture, `web/src/lib/retry.test.ts`, `web/src/components/conversation.test.tsx`.
Left for rewinding the conversation (a larger change, now row [X.46](X.46.md)): editing the message first, cutting the failed attempt out of what the model sees, retrying an older turn, putting the prompt back in the composer.

## Invariants

One writer; the node decides; nothing the model writes is trusted

## Podman / Node

No / No

## Depends

X.5, X.15, X.20

## Size

**M**

