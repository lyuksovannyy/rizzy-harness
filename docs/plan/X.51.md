# X.51 **sec** (T93)

## Adds

The row under a finished answer (Copy, Fork from here, Pin, when the turn ended, the tokens it used with the models and sub-agents on hover) and pinned messages as a bookmark (Claude Desktop's side menu and ChatGPT's pinned chats were the references; their "pin" changes what a model reads, ours does not).

## Safeguards and abuse tests

One event, `message_pinned {message_seq, pinned}`, additive; no schema change; one rule in `crates/core/src/pins.rs` (latest wins, 50 per chat, rewound messages refused and unlisted).
`session.pin` (capability `pin`) is accepted while a turn runs because the model reads nothing of it; `POST /api/sessions/{id}/pins` is owner only (404 for anyone else, admins and the API token included).
Both folds list the pins and carry a turn's end time and usage; the row reads them.
Tests: `core/src/pins.rs`, `core/tests/fold_fixture.rs`, `node/tests/pins.rs`, `master/tests/it/pins_http.rs`, `protocol/tests/pin_wire.rs`, `web/src/components/messageactions.test.tsx`, `web/src/components/pinned.test.tsx`, `web/src/lib/relativeTime.test.ts`, `web/src/lib/turnTokens.test.ts`.
See `docs/pins.md`.

## Invariants

One writer; the node decides; chats are private to their owner; a pin is never sent to the model

## Podman / Node

No / No

## Depends

X.46, C.2

## Size

**M**
