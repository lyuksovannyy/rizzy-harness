# A.20 **sec**

## Fixes

Nothing stops two processes on one data directory (found by T.8e's re-check, R-1, and by reading the start-up code).
A second `serve` or `node run` started on a live one's directory does this before it fails to bind its port: empties `<data>/sandbox-tmp`, every live session's `TMPDIR` (`start_sandbox`, `crates/cli/src/main.rs`); removes the git lock files of the checkpoint repositories and the sub-agents' sessions folder (`NodeRuntime::start`, `crates/node/src/lib.rs`); and closes the live node's open turns as `Interrupted` (`recover_interrupted`), which is a second writer to those sessions.
Typing `serve` twice is enough.
T.8e already stopped the start-up sweep from touching a live node's commands.

## Change

**Done (branch `fix/A.20-data-dir-lock`): `serve`, `node run` and `node join` hold an exclusive `File::try_lock` on `<data>/lock` (`cli/src/lock.rs`) from right after the data directory is resolved, before anything else in the directory is touched; a second start ends non-zero with one line naming the directory and the holder's role and pid (a hint from the file, never trusted), after up to 200 ms of retries so `doctor`'s moment of shared lock never refuses a start.
One lock, not one per role: the README already said "Run each node from its own data directory" and a server and a node would share `node.db`, `checkpoints/` and `sandbox-tmp/`. `doctor`, `token`, `claim-code` and `graphify` take none; `doctor` names the holder.
No locks on the file system (`ENOLCK`, `EOPNOTSUPP`, `ENOSYS`): a warning and an unprotected start.
Tests in `cli/tests/data_dir_lock.rs` (on `main`: the temp file emptied, the sub-agents' folder removed, the open turn closed as interrupted by the second process, `node run` still running, a node key made) and `lock.rs`;
T.8e's and T.8f's second-start tests now expect the refusal.**
**A lock on the data directory, taken first.** `<data>/lock`, held for the life of the process with the standard library's file lock (`File::try_lock`, Rust 1.89; no dependency), taken before the sweep, `reset_tmp`, the stores and `NodeRuntime::start`.
A second start ends at once with one line naming the directory and, when it can be read, the holder's pid, and changes nothing.
The kernel releases the lock when the holder dies in any way, so a crash leaves no stale lock.
Settle first, from the README and the code, whether `serve` and `node run` are ever meant to share one directory; if they are, the lock is per role and the tests say so. `doctor` takes no lock and says when one is held.

## Done when

Tests first, against the real binary: with a `serve` running, a second `serve` and a `node run` on the same directory exit non-zero with the message, and the first one's session temp folder, open turn and sub-agent folder are as they were (each fails on `main` today); after a `kill -9` of the holder the next start succeeds; `doctor` runs beside a live node.

## Lands on

`main`, its own pull request

