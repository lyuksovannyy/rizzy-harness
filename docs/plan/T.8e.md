# T.8e **sec**

## Adds

**Done (branch `feat/T.8e-kill-guard`, reviewed in `docs/security-review/2026-11-T8e-guard.md` and fixed): each contained command runs as node, guard, bwrap, init, `sandbox-exec`, bash; the guard (`sandbox-guard`, `tools/src/guard.rs`, started from a descriptor of the node's own running image, not the binary's path) is a child subreaper whose stdin is a Unix socket pair end (not `std::io::pipe`: it must answer while the node runs, and a second descriptor needs `pre_exec`); the channel closing, or the guard's parent no longer being the node (checked at start and every 500 ms: a child of the node waiting before `exec` holds a copy of the channel, T8E-1), is the kill order; it kills and repeats until `waitpid` gives ECHILD, and reports "ended" or "still alive after 2 s", which the tool records instead of "cancelled"; a guard that dies of a signal or does not answer makes the tool sweep that session before it returns (T8E-2), within the loop's grace (T8E-3); the spawner thread is removed; at start the node ends marked namespaces a dead node left and tells the guards of a dead node (their parent is not their own `--node`) to stop, never a live node's (re-check R-1), and logs STILL ALIVE, never "ended", what it cannot see end; nothing is kept between starts (T8E-4, re-check R-2: the file of pids is gone; sessions in the log are escaped); `doctor` warns and `contain = "required"` refuses where a session can replace the binary `sandbox-exec` is started from (T8E-5, closed by T.8c).
Tests in `cli/tests/guard.rs`: the kill window 0 of 200 (170 of 200 on `main`), a node killed while its child waits before `exec` (the command ran on before; now gone in 0.5 s), a guard killed in its first millisecond 0 of 60 once the tool returned (61 of 260 ran on before), a guard that does not answer reported inside the grace, a FUSE read reported still alive and the stuck leftover logged STILL ALIVE, a second start on a live node's data directory, a planted pid file and a session with a line break (nothing killed), the replaced binary not run as the guard, the sweep through `serve`, the command neither reaching nor holding its guard or channel, the cost (1.4 ms per command, debug build, unchanged by the 500 ms look; the bound: the guard adds less than bubblewrap's own start or 15 ms, whichever is larger, because a fixed 15 ms failed on a CI runner where bubblewrap alone took 57 ms).
Not proven: other uninterruptible waits than FUSE (NFS, disks), a kernel without `/proc/<pid>/task/<pid>/children` (fallback untested), systemd's stop (the unit needs `KillMode=mixed`, A.19), a core dump's delay, macOS; no guard without bubblewrap (`contain = "off"`, the Landlock-only fallback), where nothing is confirmed.
A guard and its node both killed in the guard's first millisecond leave the namespace to the next start's sweep (PLAN.md T9).**
A guard that watches every command until it is dead (the owner, 2026-11-12: "a closure or watchdog that watches the process until it actually dies, so a kill request is not ignored").
Closes T.8a's residual: a node killed within about 0.5 ms of a spawn leaves that namespace running, because bubblewrap's init has not yet set its death signal and no node is left to kill it; and today a kill is sent and never confirmed.
**The guard** is a subcommand of our own binary, one short-lived process per command, between the node and bubblewrap: node, guard, `bwrap`, init, `sandbox-exec`, `bash`.
Its stdin is the read end of a pipe (`std::io::pipe`, no `unsafe`) whose write end only the node holds; it starts bubblewrap itself, so it exists before anything can be left behind.
It waits on two things: its child, and the pipe.
**The pipe closing is the kill order that cannot be lost:** the kernel closes it when the node dies in any way, and the node closes it to cancel.
The guard then runs the kill of T.8a (`kill_namespace`), and, being bubblewrap's parent, **waits until the namespace is reaped**, repeating the kill; it exits only when nothing is left, and reports to the node (when one is alive) "ended" with the time it took, or "still alive after N s" for a process the kernel will not kill yet (stuck in uninterruptible I/O), which the node records in the chat instead of reporting the call as cancelled.
The graceful stop (SIGTERM, 2 s) is a byte on the pipe before the close. bubblewrap's `--die-with-parent` now names the guard, a single-threaded process, so the spawner thread of T.8a is no longer what the guarantee rests on (keep or remove it by what the tests show).
**Second line, for a guard that was itself killed:** at start the node finds namespaces left by a dead node (a marker in bubblewrap's arguments naming the node's data directory and the session, looked for among the user's own processes in `/proc`) and kills them before it serves, each named in the log.

## Safeguards and abuse tests, written first

Tests first: the reviewer's `kill_window.py` (a `kill -9` of the node 0 to 1 ms after a spawn) leaves 0 of 200 namespaces running, where `main` leaves 20 of 20; a node killed mid-command leaves nothing after 2 s, with the node's thread exits and a blocked spawner included; cancel returns only after the guard confirmed the end; a child that cannot be killed at once (stopped in a FUSE read the test controls) is reported as still alive, not as cancelled, and is gone once released; a `kill -9` of the guard alone still ends the namespace (bubblewrap's death signal), and a restarted node sweeps what a doubly unlucky kill left; the sweep never touches a process without the marker or of another data directory; a command cannot reach, signal or outlive its guard (it is outside the PID namespace); exit codes, output and timing per call (the guard adds one `exec` of our binary: measure it against T.8a's 1.5 ms) unchanged within a stated bound.

## Invariants

Light process (no idle process: the guard lives as long as its command); node decides; no `unsafe`

## Podman / Node

No / No

## Depends

T.8a

## Size

**M**

