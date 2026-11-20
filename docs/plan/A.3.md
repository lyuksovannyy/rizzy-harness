# A.3 **sec**

## Fixes

Exec 3; A2 narrowed; missed 3, 4

## Change

**Refuse a repeated `seq` whose content differs; drop a revoked node's queued frames.**
On `seq <= head`, equal content is acknowledged, different is refused as `diverged` and logged; `apply` refuses frames from a node revoked after they were queued.
Epoch waits for N.5. `05` §6.1.

## Done when

Test D5 and its divergent variant in `crates/master/tests/it/link_hardening.rs`: two links send `seq` 7 with different content, the second is refused, the mirror unchanged; revoke with 8 frames queued, none applied.
**S**

## Lands on

gate3

