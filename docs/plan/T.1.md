# T.1

## Adds

Repeated-call guard

## Safeguards and abuse tests, written first

**Done (branch `feat/T.1-repeated-call-guard`): `crates/agent-native/src/repeat.rs`, tests in `repeat_tests.rs`.
The same call (`call_key`: tool, folder, canonical arguments) with the same result (digest of `ok` and the logged output) warns from the 2nd time, is not run the 5th (before approval and dispatch), and asking for it again ends the turn.
A change starts every count again, keeping each call's last result: a new successful result of a call made before (a failure in new words restarts only its own count, T1-D1), a successful change with a new result, a completed sub-agent, a background end (review T1-R2, R3).
Asking again ends the turn only after the model was sent the refusal: the same call twice in the refusing message is refused twice (T1-D2).
The harness's own volatile tokens (`tool_output id=`, a stop's time) are not part of the result (T1-R1); calls after the turn-ending refusal are not run (T1-R4); the note is `ChatMsg::CallNote`, never taken into a callback opening (T1-R5).
The window is one turn.
What the model is told is `tool_call_finished.note`, sent after the escaped result, so the log rebuilds it byte for byte.
Differs from the row (owner's decisions, 2026-11-13): one behaviour for chats and routines (a fifth identical call with an identical result is as useless in a chat as in a routine, and the person sees the refusal as the tool result); no `[limits]` keys and no switch to turn it off (constants until a false positive is reported); the warning is for the model only (whether the UI shows it is a later web task); "a restart keeps the count" is moot because a restart ends the turn, and the notes rebuild from the log; the guard cannot fail (no `Result`), so there is no "guard error" path; `delegate` keeps its own caps.
Second rule (live test 2026-11-15, branch `fix/live-loop`): one tool's identical failures in a row, whatever the input, warn from the 3rd and refuse the tool's next call after the 7th (Hermes's same-tool failure counts, with an identical result required); the tool's next call in a later message runs as a trial, and ends the turn only if it fails the same way; successes and the person's denials never count (review of #111).**
Was: Hash of tool, arguments and outcome; warn at the second repeat, block at the fifth (Hermes Agent's thresholds, as ideas); hard stop in routines, warning in chats; decided before dispatch; a guard error blocks; state from the fold.
Tests: the fifth identical failing call blocked; a restart keeps the count; a guard error blocks; different arguments never trip it. `01` §6.2.

## Invariants

Node decides (`[limits]`)

## Podman / Node

No / No

## Depends

A.5

## Size

**M**

