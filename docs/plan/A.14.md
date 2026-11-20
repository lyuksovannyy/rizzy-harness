# A.14

## Fixes

A10

## Change

**Fail fast on a long `Retry-After`:** above PR 7's 60 s cap, fall back or end with a message naming the wait, instead of retrying early. `09` §6.5.

## Done when

A 429 with `retry-after: 300` makes no retry and falls back at once.
**S**

## Lands on

PR 7

