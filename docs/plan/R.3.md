# R.3 **sec**

## Findings

L-1 to L-4 (Medium), L-5, L-6, L-8, L-9

## The change

**A node's word does not move or widen anything on the master.** `session_created` takes effect only at `seq` 1 and never changes an owner; a parent is followed only under the same node and owner; node-supplied strings and event sizes are capped at append; revocation is re-read after the stop token is registered and `send_command` refuses a revoked node; `node.hello` is bounded.

## Done when

The review's proofs for L-1, L-2 and L-3 fail; a node revoked during its handshake is cut.

