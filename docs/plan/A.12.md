# A.12 **sec**

## Fixes

A3's one gap

## Change

**`node.key` unreadable from a `node run` shell.**
Test only; expected to pass through 2.1 and 2.7.

## Done when

`cat <data>/node.key` from bash under `node run` is permission denied (`crates/cli/tests/sandbox.rs`).
**S**

## Lands on

gate3

