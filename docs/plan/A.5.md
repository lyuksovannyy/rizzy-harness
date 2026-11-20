# A.5

## Fixes

Exec 5; A9; missed 6

## Change

**Finish the loop's limits in `[limits]`:** `max_steps` (default 50, replacing the constant) and `bash_timeout_max_s` (default 3600, today's clamp, so nothing changes until a node lowers it).
Out-of-range values stop the node.
The guard is T.1 (O9). `01` §6.3.

## Done when

`max_steps = 2` with a provider that always calls a tool ends after 2 steps naming the key; `bash_timeout_max_s = 1` cuts a `sleep 5` that asked for 3600; `max_steps = 0` refuses start.
**S**

## Lands on

gate3

