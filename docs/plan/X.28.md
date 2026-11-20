# X.28

## Adds

Agent switch as state, not history (the owner, 2026-11-16: "add model switched notice into the chat … only when agent is changed and it followed up with next message, agent cannot be changed when agent is working")

## Safeguards and abuse tests

**Done on branch `fix/agent-switch-notice`** (README "Switching a chat's agent", PLAN.md "Agents"): the pick goes into the master's session row (`Store::set_session_agent`), never the log; the node refuses `session.set_agent` while its turn slot is held or a `/compact` runs (`AGENT_WORKING`, 409); the person's next turn under the chat's own agent (a message or a Retry; not an @mention, an answer, a callback turn, a sub-agent's) writes `agent_changed { agent, model }` before its `turn_started` when agent or model differ from the last recorded; the picker is disabled while a turn runs or is stopping.
Tests: `node/tests/agent_switch.rs`, `master/tests/it/agent_switch_http.rs`, `composer.test.tsx`, `conversation.test.tsx`.

## Invariants

One writer per session; chats private

## Podman / Node

No / No

## Depends


## Size

**S**

