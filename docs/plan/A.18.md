# A.18

## Fixes

B.3 (gc was the largest cost of every turn); 0-byte `harness-index-*.lock` files left by a timed-out snapshot

## Change

**Run garbage collection rarely and clean up lock files after a timeout.**
Refs are still dropped every turn; `git gc` runs after 16 dropped checkpoints (or git's auto-gc thresholds), at most once in 5 minutes, after the turn has ended, holding the shadow alone (snapshots, copies and restores wait for a running one).
A timed-out snapshot removes its index lock; stale locks go at start-up.

## Done when

30 turns with two kept run one gc instead of 28; the store stays bounded; every kept checkpoint restores after a gc; a timed-out snapshot leaves no lock; a stale lock goes at start-up and a young one stays.
**S**

## Lands on

`main`

