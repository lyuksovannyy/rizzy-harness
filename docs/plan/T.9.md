# T.9

## Adds

Soft limit of 100 changed files per turn: warn at the end of the turn and on its undo; never block

## Safeguards and abuse tests, written first

One named constant (100); the count is `changed(before, after)` (nested repositories and ignored files not counted; a sub-agent's changes count toward the turn that applied them); the warning is the existing chat note and the undo plan's count, no new event.
Tests: 100 files warn nobody; 101 warn at the end of the turn and on its undo, and both complete normally; nested repositories are not counted. `docs/undo-and-stash.md` §2.6.

## Invariants

One writer

## Podman / Node

No / No

## Depends

C.13

## Size

**S**

