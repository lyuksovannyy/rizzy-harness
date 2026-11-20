# A.1 **sec**

## Fixes

Exec 1; missed 2; A6, A7

## Change

**A tool does not run when its event failed to save.** `Session::emit` returns the append error (`session.rs:240-244`) and `TurnHost::emit` becomes fallible (`core/src/backend.rs:19`).
The rule is the simple one: after any failed append the turn ends, with no further tool body and no further model request.
That covers the start event, the approval request and a result that could not be recorded, where carrying on would give the model a history the log does not hold. `01` §6.1.

## Done when

Test D8 without a full disk: a store that fails appends; `bash touch marker`; no marker, and the turn ends saying the log could not be written.
Second case: the append of a tool's result fails; no further model request is made.
**S**

## Lands on

gate3

