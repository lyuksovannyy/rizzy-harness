# T.8a **sec**

## Adds

**Done (branch `feat/T.8a-shell-containment`): every agent shell command runs in bubblewrap, `[sandbox] contain` (`auto`, `required`, `off`), tests in `cli/tests/contain.rs`.
The full claim holds from Landlock ABI 9 (Linux 7.1); below it a command can still start a process outside through a service on a Unix socket (the session bus), so `doctor` warns and `contain = "required"` refuses there, until T.8c (review T8A-1).**
Containment (O12; closes B.1 and C.11).
The existing `sandbox-exec` invocation runs inside `bwrap --unshare-pid --die-with-parent` with a fresh `/proc` and the host's filesystem bound as it is; the file policy is unchanged (Landlock inside).
The command sees only its own processes.
Where bubblewrap cannot run, today's path, chosen by a node setting and shown in `doctor` and one log line at start; `mode = "required"` with neither refuses the shell.

## Safeguards and abuse tests, written first

Tests first, from `experiments/b11/contain.sh`: a `setsid` child, a `nohup … &` child, a double-forked child, a child that ignores SIGTERM and a plain child are all gone 2 s after each ending: cancel, timeout, normal exit, `kill -9` of the node (control: today's path, where the `setsid` child survives).
A cancel's SIGTERM reaches the command (bwrap does not forward signals: the node signals inside, then kills bwrap). `ls /proc` shows only the command's processes.
Exit codes 3, 137, 143 and stdout apart from stderr, as today (`signals.sh`).
A command started from a thread that then ends is not killed (`PR_SET_PDEATHSIG` follows the parent thread).
The fallback is named in `doctor`.

## Invariants

Light process; node decides

## Podman / Node

No / No

## Depends

B.11

## Size

**M**

