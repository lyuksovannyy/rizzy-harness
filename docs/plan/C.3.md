# C.3

## Decision

Grant lifetime and binding

## What the note settles

TTL in minutes, renewed while the turn lives; O10's link-loss rule.
Bound to `(node_id, session_id, owner_epoch, turn req_id)`, checked on every `session.prompt`, scope checked after alias expansion; a newer grant cancels the old one's in-flight calls.
Proxy bounds written now for N.8. `04` §6 item 2; `05` §6.5.

## Tests it commits to

Grant for X refused for Y; expired grant refused at prompt; old in-flight call cancelled; grant absent from `env` and `/proc/*/environ` (test D10, grant case).

