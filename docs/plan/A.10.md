# A.10

## Fixes

PR 7's verification note

## Change

**The third flaky test,** `bash::tests::cancel_during_flood_with_a_stuck_consumer`.
By reading, it sleeps a fixed 300 ms then asserts the channel is full, which a loaded machine need not reach.
Reproduce under load first; then wait for the full channel with a deadline.

## Done when

Failure reproduced under a CPU hog and shown in the PR; 200 runs under the same load pass after.
**S** (first, if it blocks a merge)

## Lands on

gate3

