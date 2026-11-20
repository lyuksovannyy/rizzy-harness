# C.7

## Decision

Unattended authorisation (O6)

## What the note settles

A per-routine allow set made by a person, content-hashed, recorded as an event, evaluated on the node (turns `ask` into a run, never a `node.toml` deny); outside it, denial with the reason.
Schedule advances before dispatch; an idempotency record fsynced before the first model call; an unknown outcome pauses.
Agent-made routines start `pending_approval`.
Webhook keys in the database.
Never auto-approve. `10` §6.1 step 1; `01` §6.7.

## Tests it commits to

`10` §6.3 items 1 to 3; an allow set cannot lift a `node.toml` deny.
**Replaced by the owner's decision of 2026-11-14, in their words: "about routines default mode - auto, still when creating/editing an route - allows to select the mode" (and "default accept mode is Auto for every chat type"): a routine runs in the mode its owner picks, Auto by default; a run with webhook data asks (`runner::run_mode`, PLAN.md T50).**

