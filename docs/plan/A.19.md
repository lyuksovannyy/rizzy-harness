# A.19 **sec**

## Fixes

C.14 (owner, 2026-11-12); the version check runs after `Accepted` (`crates/master/src/lib.rs:894-897`, `nodes.rs:234-268`), so a refused node reconnects forever without the reason;
SIGTERM kills `serve` and `node run` (`crates/cli/src/main.rs:432`, `:670`) The README unit needs `KillMode=mixed` for the drain: with the default `control-group` systemd sends the stop signal to every process of the unit, every command's guard (T.8e) among them, and a guard kills its command at once on SIGTERM, SIGINT or SIGHUP (review T8E-2 N-2; `docs/node-update.md` §7.1).

## Change

**Done (branch `fix/A.19-stop-and-refusal`, reviewed in `docs/security-review/2026-11-A19-stop-and-refusal.md` and fixed, re-checked (§7) and fixed again): the `auth` and `join` hellos carry the node's protocol version (required) and the master compares it after the key is proved (for a join, with a valid token, before it is spent) and before `accepted`, answering `rejected` with both versions and the side to upgrade; a stranger gets the old refusal whatever it claims.
Every `rejected` ends the node's reconnect loop (each needs a person) and the process exits 77, which the README unit does not restart; the node prints the reason as printable ASCII on one line, at most 300 characters, and accepts only a node id in the master's own shape.
SIGTERM and SIGINT start the same stop in `serve` and `node run` (`cli/src/stop.rs`): new sessions, turns, restores (checked again under the registry's lock), stages and sub-agents are refused with `harness_protocol::NODE_STOPPING` (503 on the master, a refused message keeps its uploads); approvals and cancels still taken, the cancel winning the approval's wait; running turns are cancelled through the normal path and end `turn_ended{reason: stopped}` (a new `TurnEndReason`; a turn cancelled before keeps `cancelled`, one completed keeps `completed`); the wait is the tool grace plus the checkpoint's limit plus 15 s (80 s), exit 0 when all closed, else 1 naming the sessions (an unwritten end included); a second signal exits 1 at once; the data-directory lock is held in `main` until the process is gone and the runtime gets 2 s, also after a panic; a holder stuck in an uninterruptible read is named as such by the lock's refusal and `doctor`. `serve` keeps HTTP up during the wait, starts no routine run and answers Run now and webhooks 503; a run a stop cut ends `interrupted` with the reason, is not counted and is not run again by itself (the put-back of the first fix was withdrawn: it lost a webhook's event and could loop, R-1 to R-4); a crash mid-turn still counts.
README unit: `KillMode=mixed`, `TimeoutStopSec=120`, `RestartPreventExitStatus=77`, no `KillSignal`.
Tests that failed first: `master/tests/it/version_refusal.rs`, `routines_http.rs` and `hooks_http.rs` (the stop's runs), `cli/tests/stop_signal.rs` (both signals to `serve` and `node run`, the refusals during the wait, a stuck thread and the lock, the bound, the refused node's status and text, an upload sent again), `node/tests/runtime.rs` (the restore race, an unwritten end, a late allow, a cancel and a completion before the stop).
Not run: systemd itself.
The full drain is N.9 (`docs/node-update.md` §7.0).**
**Drain on a stop signal, and tell a node why it was refused.**
The protocol check moves before `Accepted` and answers `Rejected` with the reason;
SIGTERM is handled like SIGINT; a stop refuses new work with a reason, cancels running turns through the normal cancel path, closes them in the log naming the stop, and exits; a second signal skips the wait.
The README unit sets `TimeoutStopSec` and drops `KillSignal=SIGINT`.
The agent-asked drain and the update itself stay in N.9.

## Done when

Tests: a protocol mismatch is refused before `Accepted` with the reason, and the node exits instead of reconnecting; a SIGTERM closes a running turn with its process tree gone and the stop named in the log; a second signal forces.
**M**

## Lands on

`main`

