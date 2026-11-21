# X.52 **sec** (T39)

## Adds

Switch an existing agentic chat between the sandbox and the host shell (the owner, 2026-11-21: "im not able to change sandboxing state during session creation nor during active session"; asked whether to build the switch for a running chat, "Yes, build it"; then "when chat is active/running sandbox change is unavailable but if chat is stopped then no problem").
The reference our research found is the DeepSeek harness, whose sandbox mode changes are log events with the last one winning (`research/harnesses-round-1-appendix.md`, "Strict log discipline"; round 2: "Mode precedence: approved mode, last `sandbox/mode`, deployment default"): the same shape as `exec_mode_changed`; we found no harness of the others that documents a switch, and the alternative here was a new chat, which loses the conversation.
The chat's owner may switch it while nothing of the chat runs, in the strip under the message box ("Commands"), with the existing host warning (and a sentence about Auto); a divider marks the change; the new-chat screen says how to turn the host shell on when the node does not offer it.

## Safeguards and abuse tests

One additive event, `exec_mode_changed {mode, from}`, and one rule in `crates/core/src/exec_mode.rs` (the latest wins, else the creation's; an unknown mode is unreadable and runs nothing); no schema change (the `sessions.exec_mode` column the mirror already had follows the event, as written).
`session.set_exec_mode` (capability `exec-switch`); `POST /api/sessions/{id}/exec-mode` is owner only (404 for anyone else: admins, workspace admins, the API token), with the checks of a create for the target mode, and audited when it asks for `host`.
The node decides again under the turn slot (offered, nothing running, no approval waiting), stops the chat's MCP servers and waits for them to be gone, removes the chat grants, then records the event.
Tests: `core/src/exec_mode.rs`, `core/src/view.rs`, `store/tests/event_log.rs`, `node/tests/exec_switch.rs`, `master/tests/it/exec_switch_http.rs`, `web/src/components/execswitch.test.tsx`, `web/e2e/exec-switch.cjs`.
See `docs/execution-modes.md`.

## Invariants

The node decides; one writer per session; chats are private to their owner; nothing a model writes switches the mode; the master runs no command.

## Podman / Node

No / Yes

## Depends

N.10

## Size

**M**
