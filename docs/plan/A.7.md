# A.7

## Fixes

Exec 6; A11

## Change

**Shrink check and breaker for compaction.**
If a summary does not make the history smaller, discard it (no `HistorySummarized`) and count a failure; after two, stop compacting for the turn. `07` §6.1.
Since X.15 they apply to a summary of a turn's own steps too (same rule, same count).

## Done when

A summariser that returns more than it got leaves history and log unchanged; after two, no third summary call in that turn.
**S**

## Lands on

PR 7

