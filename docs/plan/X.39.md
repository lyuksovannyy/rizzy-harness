# X.39

## Adds

Usage during a turn (the readiness trial of 2026-11-19, item 11: a 20-minute turn showed `1505k tokens · ≈ $1.59` throughout, then jumped to 4252k and $3.16; the log carried usage only on `turn_ended`, and a turn that ended without it, interrupted by a restart, lost what it spent).
Totals for admins are built (see X.18); not done: spending limits; a running sub-agent's own sub-agents are not added to the header while they run (one level only); a routine run recovered after a crash (reconcile) takes its usage from the log's `turn_ended` only, as before.

## Safeguards and abuse tests

**Done on branch `feat/live-usage`** (PLAN.md event table `turn_usage`, README "What a chat costs").
The loop reports the turn's running usage after each model call (and after a summary call, a sub-agent's result and each parallel answer) through the host; the session actor writes `turn_usage { turn_id, usage }`; both folds (`crates/core/src/view.rs`, `web/src/fold.ts`, one shared fixture `web/src/__fixtures__/live_usage.json`) show finished turns plus the running figure, the end's usage replaces it, an end without usage keeps it.
The header keeps today's words and the "≈" mark.
No threat row: the event is numbers only and written by the one writer.
**Follow-ups (branch `feat/usage-follow-ups`):** the header adds each foreground sub-agent's latest known total from first seen running until its call has a result (`web/src/lib/childcost.ts`; the log is unchanged except that a delegate's result is now logged before the `turn_usage` that takes the child in; a floor, tokens and cost each, within the turn); a routine run's record keeps the last `turn_usage` when its turn ends without usage (`runner.rs`, `Bound.last_usage`).

