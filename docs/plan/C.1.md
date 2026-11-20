# C.1

## Decision

Fence for the link and for fork

## What the note settles

A **generation per connection** (`nodes.link_gen`, bumped on accept), checked by the reader pump per frame.
An **epoch per session** (`sessions.owner_epoch`, a counter that never goes back), bumped on archive, delete, revocation, environment deletion and fork-source close, not on a reconnect; checked in one predicate in the mirror transaction with owner, active state and `seq = head + 1`. On `fenced` the node stops the session and kills the tool.
Three flaps revoke and bump.
Replaces §6's "second connection replaces the first".
Our own bump list, not OpenClaw's. `05` §6.1.

## Tests it commits to

Superseded connection's frame dropped; append after archive refused; node kills the tool on `fenced`; reconnect does not bump.

