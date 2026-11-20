# X.9 **sec** (done, T50)

## Adds

Per-chat approval modes the person changes (the owner, 2026-11-14: "not that usable"): Ask, Accept edits, Auto, as Claude Desktop names them (`08` "Approval presentation");
Codex lets an unmatched command run inside the sandbox (`02` §2, Codex)

## Safeguards and abuse tests

A command to the owning node, owner only through `send_command`; the node clamps by `max_approval_mode` alone (default `auto`; new chats of every kind start in Auto, host chats included: the owner, 2026-11-14), records `approval_mode_changed`, re-clamps at load; accept edits = `write`/`edit` resolved inside the folder by the node; a sub-agent's ceiling is its chat's mode at every call (`02` §6 item 8); routines and sub-agents' chats refuse a change.
Tests: `crates/node/tests/approval_modes.rs`, `crates/master/tests/it/approval_mode_http.rs`, `web/e2e/approval-mode.cjs`

## Invariants

Node decides; one writer

## Podman / Node

No

## Depends

A.2

## Size

M

