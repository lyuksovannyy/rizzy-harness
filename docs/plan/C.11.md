# C.11

## Decision

Process-tree containment (only if B.1 leaks)

## What the note settles

**Closed: decided by B.11 (O12), a PID namespace through bubblewrap** (`bwrap --unshare-pid --die-with-parent`, built as T.8a); no `unsafe`, no `fork` of ours, no cgroup delegation needed.
The options weighed were a per-session cgroup (needs delegation, such as a systemd user scope; 2.5 ms per `systemd-run --user --scope`, B.11), a PID namespace in `sandbox-exec` itself (`nix`'s `fork` is `unsafe`), or a `/proc` re-walk (racy). `01` §6.9; `03` §6.3 item 4.

## Tests it commits to

A `setsid` child is gone 2 s after cancel; none survives `kill -9` of the node (T.8a's tests).

