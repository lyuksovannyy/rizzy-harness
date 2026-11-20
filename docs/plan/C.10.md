# C.10

## Decision

seccomp (O4) and a read-only shell

## What the note settles

`03` §6.2's rules: with network off, `socket` only for `AF_UNIX`, deny `connect`, `bind`, `listen`, `accept`, `sendto`, `sendmmsg`, `recvmmsg`; always deny `io_uring`, `AF_VSOCK`, `ptrace`, `process_vm_*`; `EPERM`, never `SIGSYS`; unknown architecture refused in `required`; `sendmsg` decided.
Status and tool result show the active rules.
The crate's dependency check.
T.7's Landlock profile.

## Tests it commits to

T.6's and T.7's lists.

