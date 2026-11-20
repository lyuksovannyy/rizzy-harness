# X.11 **sec** (done, T51)

## Adds

A denial's note and "allow for this chat" (the owner's brief, 2026-11-14)

## Safeguards and abuse tests

OpenCode's `reject` carries the person's message and the model goes on with it (`02` §2, OpenCode);
Codex keeps an exact-call session map (`02` §3.2) and research 02 §7 item 2 asks for it held on the node, written as an event, never crossing to a child.
The note: capped, dropped with an allow, quoted after the fixed sentence.
The grant: the exact `call_key`, this chat only, rebuilt from the log, removed from the side panel, not in `ask_all`, not past `denied_tools`.
No prefix or pattern rules (X.6).
Tests: `crates/node/tests/approval_modes.rs`, `crates/master/tests/it/approval_answers_http.rs`, `web/e2e/approval-answers.cjs`

## Invariants

Node decides; one writer

## Podman / Node

No

## Depends

X.9

## Size

S

