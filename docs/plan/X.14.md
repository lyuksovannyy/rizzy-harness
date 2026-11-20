# X.14

## Adds

Step limit: 1,000 by default, per chat (the owner, 2026-11-15: "Raise max to 1000 by default + add ability for session owners to override max steps (min: 1 - {max setted})")

## Safeguards and abuse tests

**Done on branch `feat/step-limit`:** `DEFAULT_MAX_STEPS` 1,000 (node.toml range 1 to 10,000); a chat's owner sets its own limit from 1 to the node's (`POST /api/sessions/{id}/step-limit`, owner or API token, a raise through the execution-level check, as the approval mode), relayed as `session.set_step_limit` only to nodes announcing `steps-max:<n>`, clamped and recorded by the session as `step_limit_changed`, rebuilt after a restart, read before every request of a person's turn (`TurnHost::step_limit`), so a lowering below the steps used gives the wrap-up next; a sub-agent's cap is at most its parent chat's; callback turns keep `callback_max_steps`; a routine's chat uses the node's limit.
The composer's "N steps" control beside the approval mode.
Threat row T56.

## Invariants

`node/tests/step_limit.rs`, `master/tests/it/step_limit_http.rs`, `agent-native/tests/staying_power.rs`, `web/e2e/step-limit.cjs`.
Not done: a limit at chat creation and in the routine editor (see the pull request).

## Podman / Node

One writer; node decides

## Depends

No / Yes

## Size


## Column 8

**S**

