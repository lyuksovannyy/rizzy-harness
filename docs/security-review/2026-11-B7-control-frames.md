# B.7 review, 2026-11-13: control frames on a full node uplink

Reviewed: `exp/B.7-control-frames-under-load`. One independent reviewer who
did not write the code read `git diff origin/main...HEAD` after the builder
finished (builder's head `b06c03e`), with proofs where it could; the builder
then fixed the findings (head `93d1599`), and a second, fresh reviewer re-
checked the fixes. Format and severity as in the earlier records in this
folder. The texts below are the reviewers' own, shortened.

## 1. Verdict of the first review

**Fit after fixes.**

| Id | Severity | What | Where |
|---|---|---|---|
| B7-R1 | High | The written result says a cancel acts on the node at once; on the real node a Stop does not end a streaming command while the out queue is full | `ADOPTION_PLAN.md:187` |
| B7-R2 | Medium | 'The cancel reaches the node in 0.2 ms' holds only for the first command; a cancel sent after any other answered-inline command waits on the node | `docs/nodes-credentials-environments.md:806` |
| B7-R3 | Medium | The delay is reported as about 6 MiB ahead; the out queue is bounded in messages, not bytes, and the same 256 messages were 18.8 MiB and 20 s | `ADOPTION_PLAN.md:187` |
| B7-R4 | Medium | The committed result leaves out its two preconditions: the node must produce faster than the uplink carries, and two thirds of the 6 MiB is this machine's loopback send buffer | `crates/link/tests/control_under_load.rs:22` |
| B7-R5 | Low | The fix sketch lets every ack jump the queue; the master relies on a create's ack coming after its session_created, and an ack alone does not make Stop feel answered | `ADOPTION_PLAN.md:187` |
| B7-R6 | Low | The passing test's approval assertion tests the test's own model and the kernel, and the ignored test cannot go green from a fix in the node | `crates/link/tests/control_under_load.rs:385` |
| B7-R7 | Note | The B.7 row moves a 'fix before Gate 3' to after the gate on the builder's say, and no task row carries the lane | `ADOPTION_PLAN.md:174` |

## 2. Findings

### B7-R1 (high): The written result says a cancel acts on the node at once; on the real node a Stop does not end a streaming command while the out queue is full

The builder's test never runs the node: it timestamps when a model's command
loop receives the frame (control_under_load.rs:265-272), and the text turns
that into 'the node acted on it at once' (ADOPTION_PLAN.md:187), 'a cancel
acts on the node at once' (docs/nodes-credentials-environments.md:806) and
'The node had stopped the turn 0.2 ms after the cancel was sent'
(control_under_load.rs:394-395). On the real NodeRuntime that is false for any
session that is streaming output. The agent loop's `coalesce_led` awaits
`host.emit(...)` inside a select arm (crates/agent-native/src/lib.rs:870-872,
884, 890); emit is `Session::append`, which waits in `publish_wait` for room
in the out queue (crates/node/src/session.rs:628-630). While it waits, the
tool future is not polled, so `bash`'s own `ctx.cancel.cancelled()` arm
(crates/tools/src/bash.rs:374, 380) and the 5 s abandon token are never looked
at. Proof with the real node, real agent loop, real bash, default limits:
session B runs a quiet loop (one line and one file write every 0.1 s), 12
other sessions run `yes`, the reader of `from_node` takes nothing. Stop on B:
its command was still writing its file 3 s and 7 s after the Stop (past
TOOL_CANCEL_GRACE), and ended only when the uplink was read again; ack and
turn_ended arrived 6.81 s after the Stop. Control, same sessions with a
reading uplink: command gone at the first look (under 0.4 s), ack after 59
microseconds. On a link that still moves the wait is the time to the socket's
next writable moment (the builder's own 1.1 s at 1 MiB/s and 5 s at 256
KiB/s); on a link that takes nothing it is IDLE_LIMIT, 60 s
(crates/link/src/lib.rs:61), by reading. So one workspace's output can keep
another workspace's stopped command running, and the builder's reason for
leaving PLAN.md alone ('the cancel itself is never delayed') does not hold;
PLAN.md T9 line 589 still says cancel and timeout are awaited alongside every
send and that a cancelled tool has 5 s.

Proof: `sh crates/cli/tests/support/review_b7/run_proofs.sh node` (log
target/tmp/review-b7-node-2.log): 'B.7 review, control: command stopped
Some(10.86µs) after the Stop, ack after 58.57µs' and 'B.7 review, full uplink:
command still running 3 s after the Stop: true; 7 s after: true; uplink read
again after 6.807755447s; command stopped Some(1.493µs) after that; ack
6.812258022s and turn_ended 6.817881241s after the Stop'; 2 passed in 24.13 s.
node.db was opened Durability::Normal for this run (see B7-R4 for why).

Asked for: No product change here. Correct the three sentences
(ADOPTION_PLAN.md:187, nodes note :806, control_under_load.rs:38-39 and
:394-395) to say what was measured (the frame reached a model's loop) and what
the real node does; add the real-node measurement as an `#[ignore]` test in
crates/node/tests with these numbers; add the residual to PLAN.md T9; add a
third part to the fix sketch: the agent loop must keep polling the tool (or
the cancel) while an emit waits.

### B7-R2 (medium): 'The cancel reaches the node in 0.2 ms' holds only for the first command; a cancel sent after any other answered-inline command waits on the node

The test always sends the cancel first (control_under_load.rs:327-342), so the
cancel gets the idle command loop and the approval answer gets the wait. The
loop treats Cancel, ApprovalRespond and StopBackground the same way
(crates/node/src/lib.rs:565-571, 974-981): whichever comes second waits for
the first one's ack to get room. With the order swapped the cancel waits. A
double Stop, a Stop in two sessions, or an approval answer just before a Stop
is enough. The nodes note reads as if the cancel were exempt ('a cancel acts
on the node at once, but ... an approval answer sent after it waits 1.1 s'),
and the passing test's name and its `cancel_at_node < 1 s` assertion (line
384) pin only the favourable order. While the loop waits it also reads no
other command for any session or workspace; once the node's 64 and the
bridge's 64 are queued the master's `try_send` fails and every command to that
node answers Timeout at once (crates/master/src/lib.rs:1443-1452, by reading).

Proof: `sh crates/cli/tests/support/review_b7/run_proofs.sh link` (log
target/tmp/review-b7-link-1.log): 'B.7 review, approval answer then cancel at
1024 KiB/s: approval answer at node 205.999µs, CANCEL at node 1.123875016s'.

Asked for: Say in the result and the note that the second answered-inline
command waits, a cancel included, and that it holds up every command of the
node; either add the swapped order to the test or rename the passing test so
it does not claim more than 'the first frame'.

### B7-R3 (medium): The delay is reported as about 6 MiB ahead; the out queue is bounded in messages, not bytes, and the same 256 messages were 18.8 MiB and 20 s

'6.1 MiB ahead of them (the out queue's 2 MiB and the kernel's socket
buffers)', 'about 6 MiB in B.7' (nodes note :806) and 'below about 410 KiB/s
the wait passes the master's 15 s ACK_TIMEOUT' (control_under_load.rs:396) all
rest on 8 KiB frames. OUT_QUEUE is 256 messages (crates/node/src/lib.rs:64).
An event may be up to 4 MiB (crates/store/src/lib.rs:480) and a replay's
snapshot batch up to 2 MiB (crates/node/src/lib.rs:60, 1247-1253, sent with
publish_wait), so after a reconnect with sessions to replay the queue ahead of
a Stop's ack can be hundreds of MiB (256 x 2 MiB = 512 MiB, by reading), which
is also node memory. With 64 KiB frames at the builder's own 1 MiB/s the
cancel's ack was past ACK_TIMEOUT, where the result says 6.6 s. The builder's
weak spot 'the 2 MiB out queue that is ours is fixed' is wrong for the same
reason.

Proof: Same run as B7-R2: 'B.7 review, 64 KiB frames at 1024 KiB/s: cancel's
ack at master Some(20.176438072s), 301 frames (18.8 MiB) ahead of it'.
Builder's numbers at 8 KiB reproduced: `cargo test -p harness-link --test
control_under_load -- --nocapture --include-ignored --test-threads=1` gave
6.67 s and 6.61 s, 783 frames (target/tmp/review-b7-builder-1.log).

Asked for: State the bound as 256 messages of whatever size, give the 8 KiB
figure as one case and the replay case as the worst, and drop or qualify the
410 KiB/s threshold.

### B7-R4 (medium): The committed result leaves out its two preconditions: the node must produce faster than the uplink carries, and two thirds of the 6 MiB is this machine's loopback send buffer

The flood in the test is a tight loop of in-memory sends (lines 249-256). The
real node writes every event to node.db with Durability::Full before it
publishes it (crates/cli/src/main.rs:632, crates/node/src/session.rs:603-608).
On this checkout's disk the real node with 12 flooding sessions had 147
messages in its out queue after 60 s (about 2.5 a second, some 20 KiB/s), so
on this machine the queue cannot fill on any uplink faster than that, and the
headline case (1 MiB/s kept full) cannot happen here at all; on a disk with 1
to 5 ms fsync it can. Separately, 783 - 256 frames = 4.1 MiB of the 6.1 MiB is
the node socket's send buffer autotuned to this machine's tcp_wmem maximum
(4096 16384 4194304) behind a loopback proxy; a real WAN path at these rates
will not grow it that far, so 6.6 s and the 410 KiB/s threshold are loopback
numbers, and the part that is ours (2 MiB at 8 KiB frames) gives about 2 s at
1 MiB/s, inside the grace. The builder named both in the report; neither is in
the test's doc comment, the plan entry or the note, which are what later
readers will size the fix from.

Proof: `REVIEW_B7_FULL=1 sh crates/cli/tests/support/review_b7/run_proofs.sh
node` (log target/tmp/review-b7-node-3-full.log): 'the out queue never filled:
147' after 60 s, and the control timed out waiting 30 s for 12 flooders to
print their first chunk; an earlier run with 4 flooders gave 157
(review-b7-node-1.log). Rough: it includes session creation. The buffer split
is arithmetic from the builder's own frame count and
/proc/sys/net/ipv4/tcp_wmem.

Asked for: Add both caveats to the test's doc comment and the plan entry: the
production rate of a real node was not measured (give the fsync dependence),
and how much of the bytes ahead is the kernel's on loopback against the 256
messages that are the node's.

### B7-R5 (low): The fix sketch lets every ack jump the queue; the master relies on a create's ack coming after its session_created, and an ack alone does not make Stop feel answered

The sketch: 'a control lane in the pump that sends an `ack` ahead of queued
output', with the seq constraint named only for approval requests.
`create_session` says 'The node's session_created is mirrored before its ack
(same ordered link), so the row exists' (crates/master/src/lib.rs:1569-1570)
and stamps owner and workspace on that row; with the ack ahead, every create
on a busy link takes the fallback meant for 'a hostile or odd node'
(1582-1586). Other acks may carry the same assumption (not checked one by
one). And the builder's own weak spot is right: the turn's end is an event of
the cancelled session and still waits behind that session's queued output, so
the UI, a fold over the log, shows the turn running for the same 6.6 s or 26 s
after Stop returned 200.

Proof: by reading only

Asked for: In the sketch, limit the lane to acks of the answered-inline
commands (cancel, approval answer, stop_background) or name ack-after-own-
events as a constraint, and say that the turn's end still waits unless a
session's queued output is dropped or passed on cancel.

### B7-R6 (low): The passing test's approval assertion tests the test's own model and the kernel, and the ignored test cannot go green from a fix in the node

`assert!(m.approval_at_node < GRACE)`: the wait it bounds is created by the
model loop's own `out.send(ack).await` (line 273) and ended by the kernel's
next writable moment. No change in crates/node can make it fail or pass, since
the loop is a copy, as are OUT_QUEUE and IN_QUEUE (lines 67-69). The value is
a sample of one moment: the test sends 0.5 s after the queue first fills (line
321), which is why two runs agree to 30 ms; sent at other moments the same
wait was 41 ms to 1.21 s at 1 MiB/s and 41 ms to 4.19 s at 256 KiB/s. It
scales with tcp_wmem's maximum, so a machine tuned to 16 MiB would fail the 5
s limit at the default rate. The `#[ignore]` test (lines 400-408) has the same
model inside it, so 'passes once the lane exists' needs the test rewritten,
and the pump's single `outgoing` channel changed. `behind >= OUT_QUEUE - 2`
(lines 383, 405) is a real control. The file adds 9 s of wall clock to every
`cargo test` of the crate (8.99 s in the builder's log).

Proof: `sh crates/cli/tests/support/review_b7/run_proofs.sh link`
(target/tmp/review-b7-link-1.log), the ten 'sent ... after the queue filled'
lines; e.g. 1024 KiB/s: 6 ms, 1.212 s, 806 ms, 406 ms, 41 ms; 256 KiB/s: 41
ms, 4.19 s, 2.89 s, 1.59 s, 289 ms. The review expected a wait past 5 s at 256
KiB/s when sent 150 ms after the fill and did not get one (41 ms,
review-b7-link-2.log); the longest seen is the builder's 4.99 s.

Asked for: Drop the approval assertion from the passing test (keep the print)
or state in its doc that it bounds the model; write 'up to' with the numbers
in the docs; say in the ignored test's doc that the command loop is modelled
in the test and must be replaced with the real node when the fix lands.

### B7-R7 (note): The B.7 row moves a 'fix before Gate 3' to after the gate on the builder's say, and no task row carries the lane

The row's outcome was 'a control lane in the pump, as a fix before Gate 3'.
The edit writes 'the lane is now a fix after the gate' into the plan as
settled. B.2, B.3 and B.5 each ended in a task (A.16, A.18, A.15); B.7 ends in
a paragraph and a table cell, and Gate B asks for 'each outcome written into
the C note or task it feeds'. Where the fix goes, and whether B7-R1 makes it
security work under the freeze, is the owner's decision.

Proof: by reading only

Asked for: Word the row as a result awaiting the owner's slot, not as a
rescheduling, and ask the owner for the task id.

## 3. Attacked and found sound

- No product code changed: the diff is one test file, a dev-dependency on a
  workspace crate (crates/link/Cargo.toml, Cargo.lock), ADOPTION_PLAN.md and
  the nodes note.
- The link half of the experiment is real: TLS, WebSocket, `server::bridge`
  and `client::run_with` behind a rate-limited proxy, with a control that the
  uplink was full (`behind >= OUT_QUEUE - 2`).
- The builder's numbers reproduce at 1 MiB/s: cancel's ack at the master 6.67
  s and 6.61 s, approval answer's 7.18 s and 7.12 s, 783 frames ahead,
  approval answer at the node 1.11 s; the passing test passes and the ignored
  one fails as recorded.
- The model of the node's command loop matches the code for what it models:
  `handle` acts, then waits in `publish_wait` for the ack, inline for Cancel,
  ApprovalRespond and StopBackground (crates/node/src/lib.rs:565-571,
  974-981).
- The 8 KiB frame size is right for steady `bash` output (one 8192-byte pipe
  read per event once the buffer passes FLUSH_BYTES); larger events appear
  only when a cancel drains the 32-chunk channel into one.
- The master never blocks on a full link: it sends with `try_send`
  (crates/master/src/lib.rs:991, 1123, 1443), so the two ends cannot deadlock
  on each other's queues.
- `CommandError::Timeout` maps to 504 (crates/master/src/http.rs:252) and
  ACK_TIMEOUT is 15 s (crates/master/src/lib.rs:55).
- The decision the experiment feeds stands and is stronger than written:
  control frames from the node wait behind everything queued, and a lane is
  needed.
- The test keeps its temp files under CARGO_TARGET_TMPDIR; `cargo fmt --all
  --check` passes.

## 4. Claims with no test at the first review

- The 256 KiB/s node-to-master numbers (26.2 s and 28.2 s): not rerun; read
  from the builder's log target/tmp/b7-run3.log only.
- That Stop's HTTP route answers 504 after 15 s in this case: the mapping and
  the timeout were read, the route was not driven.
- The 60 s bound (IDLE_LIMIT) on a Stop that waits for a link taking nothing:
  by reading crates/link/src/pump.rs and `publish_wait`; my real-node proof
  stalls an in-process link for 7 s and has no pump.
- That a tool's timeout and the node's own stop (SIGTERM drain) are held the
  same way as a cancel while an emit waits: follows from the same code path,
  not run.
- The 512 MiB replay case: arithmetic from SNAPSHOT_BYTES and OUT_QUEUE;
  measured only up to 64 KiB frames (18.8 MiB).
- The real node's production rate with Durability::Full (about 2.5 messages a
  second here): one rough count on this disk, including session creation; not
  measured on a fast disk.
- Whether acks other than create's are relied on to follow their events on the
  master.
- `cargo clippy --workspace --all-targets -- -D warnings` exit 0: the
  builder's claim, not rerun.

## 5. What the builder did, and the re-check

| Id | Builder | Re-check | Evidence of the re-check |
|---|---|---|---|
| B7-R1 | fixed `2e7dde1` | closed | Reproduced myself: `cargo test -p harness-node --test control_under_load -- --nocapture --include-ignored --test-threads=1` (target/tmp/recheck-b7-builder-node-1.log): the streaming command was not stopped during a 6.08 s shut uplink (stalled: None), ack 6.085 |
| B7-R2 | fixed `2e7dde1` | closed | Same run: `a_stop_after_an_approval_answer_ends_a_silent_command_while_the_uplink_takes_nothing` fails as stated (silent command not stopped in 6.04 s, ack 6.043 s), while the same silent command with the Stop sent first passes, so the approval answer is the o |
| B7-R3 | fixed `859cff8` | partly closed | The bound is now stated as 256 messages of any size, the 64 KiB and replay cases are given (SNAPSHOT_BYTES and publish_wait at crates/node/src/lib.rs:60, 1249 checked), and the 410 KiB/s threshold and 'fixed 2 MiB' are gone. But the fix added 'real `yes` event |
| B7-R4 | fixed `859cff8` | closed | Both preconditions are in the link test's doc comment (lines 52-66 region) and in ADOPTION_PLAN.md:187, with the fast-disk production rate stated as not measured. My own datum agrees with the 8 KiB case: the real node's full out queue held 1,482,542 and 1,474, |
| B7-R5 | fixed `93d1599` | closed | By reading ADOPTION_PLAN.md:187: the lane is limited to acks of cancel, approval answer and stop_background, names the create-ack-after-session_created reliance (crates/master/src/lib.rs:1569-1586 confirmed), says other acks were not checked one by one, and sa |
| B7-R6 | fixed `859cff8` | closed | `assert!(m.approval_at_node < GRACE)` is removed (git diff b06c03e..HEAD); the value is printed. `cargo test -p harness-link --test control_under_load`: 1 passed, 1 ignored, 9.28 s (target/tmp/recheck-b7-link-1.log). The docs give the range and say the ignored |
| B7-R7 | fixed `93d1599` | closed | ADOPTION_PLAN.md:174 now records a result and says before or after Gate 3, the task and whether it is security work under the freeze are the owner's decision, not made yet. No task id was invented. |

New in the re-check:

### B7-D1 (medium): The new T9 residual caps the hold at the link's 60 s idle limit; the hold is the number of publishers waiting ahead times the time per frame, and the idle limit is per frame

PLAN.md T9 now says a stopped or timed-out streaming command runs on 'until
the uplink has room again (... up to the link's 60 s idle limit by reading)';
docs/nodes-credentials-environments.md:806 and ADOPTION_PLAN.md:187 say the
same. Room for one message is not enough: the blocked emit waits its turn
among the other blocked publishers (tokio's mpsc is first come). With the
builder's 12 flooders the uplink had to take 14 messages before the stopped
command ended: 2.5 s at one message per 200 ms, 14.0 s at one per second, and
the Stop's ack had still not arrived. The link's idle limit times each frame
separately (`tokio::time::timeout(timing.idle_limit, sink.send(frame))`,
crates/link/src/pump.rs:82), so a peer that takes one frame just under every
60 s is never dropped: by reading, the hold is then about 14 x 60 s with 12
other streaming sessions, and grows with their number. The same holds for the
timeout, which I ran: with no Stop and the in-process uplink shut, a command
with the default 60 s timeout was still writing at 66 s and 75 s and ended
only when the uplink read again, recorded then as '[timed out after 60s ...]'.
So a master that reads slowly (compromised, T6, or just wedged) keeps a node's
streaming commands running past node.toml's timeout for minutes, while T9's
bold headline still says a command's processes 'never outlive its timeout' and
'Time and steps are the node's'. Not introduced by this branch; the branch
writes a bound into the threat model that the code does not have. Limits of
the proof: the trickle and the shut uplink are the test's in-process reader,
not the TLS link; the per-frame timer is by reading.

Asked for: In PLAN.md T9, the nodes note and the plan entry: replace 'until
the uplink has room again, up to 60 s' with what holds: until the uplink has
taken one message for every publisher waiting ahead (14 with 12 other
streaming sessions), each frame allowed up to IDLE_LIMIT, so no fixed cap; 60
s only for a peer that takes nothing. Mark the timeout as measured (60 s
timeout, still running at 75 s) and qualify the bold 'never outlive its
timeout' with a pointer to the residual. Add 'a command's timeout' to the
README paragraph.

### B7-D2 (low): 'The 5 s grace does not start' is wrong: the grace runs out during the wait, and a Stop held longer than 5 s ends the tool as abandoned, never as cancelled

The grace is a task of its own (`cancel.cancelled().await;
sleep(TOOL_CANCEL_GRACE); abandon.cancel()`, crates/agent-
native/src/lib.rs:2083-2088), so it counts from the Stop whatever `coalesce`
is doing. When the emit finally gets room after more than 5 s, the biased
select sees `abandon` first and drops the tool future without ever polling it
after the Stop: the tool's own stop sequence (SIGTERM, the guard's confirmed
end for a contained command) does not run, and the result recorded for the
model and the person is 'whether what it started has ended is not known'.
Under 5 s the tool is polled and returns '[cancelled: process group killed]'.
PLAN.md T9 states the opposite, and the fix sketch's reader will size part (1)
from it.

Asked for: Reword the residual: the grace counts from the Stop and is spent
unseen; a Stop held past it drops the tool without its own stop having run,
recorded as abandoned (end not known).

### B7-D3 (low): 'Real `yes` events averaged 23 KiB' is a misread sample; they are 8 KiB, and the real node's full out queue held 1.4 MiB

The figure (also ADOPTION_PLAN.md:187) comes from one line of the first
review's log: 197 events, 4,661,313 bytes, twelve flooders cancelled early. A
cancelled call's last event carries everything still in the stream channel
(the `while let Some(d) = rx.try_next()` drain, crates/agent-
native/src/lib.rs:895-900; that this explains the 23 KiB is my inference, not
measured). Run to their 1 MiB cut, each flooder made 128 or 129 events of
8,134 bytes on average, with the uplink read throughout and with it shut for
the first 8 s. A reader sizing the queue from 23 KiB gets about 5.8 MiB for
256 messages; measured it was 1.41 MiB. The same paragraph's own '2.5 messages
a second ... about 20 KiB/s' already assumes 8 KiB.

Asked for: Replace the 23 KiB sentence in both places with the measured 8.1
KiB an event and 1.4 MiB for a full queue of real `bash` output; if wanted,
add that a cancelled call's last event can be much larger.

### B7-D4 (low): The committed microsecond figures are the test's loop clock, not when the command stopped, and one number is attached to the wrong thing

`stopped_after` takes `at = from.elapsed()` before a 0.4 s look and returns
that `at` if the look saw no write, so the first look always reports about a
microsecond. From it the docs say the command 'ended within 1 µs of the uplink
being read again' and 'is stopped at once (14 µs)' (ADOPTION_PLAN.md:187; test
docs lines 29, 32-33, 408). My runs printed 741 ns and 782 ns 'after the
uplink read again', less than one system call. What the look shows is no write
in the next 0.4 s, with a write every 0.1 s: stopped within about 0.1 s.
ADOPTION_PLAN.md:187 also writes 'the ack and the turn's end came 6.03 s after
the Stop (with a reading uplink, 11 µs)': the 11 µs was the first review's
command-stopped stamp (same artifact); that run's ack was 59 µs and no turn's
end was reported. And 'the turn's end 13.0 s after the Stop' (test doc line
39) came from the builder's run of three node instances at once
(b7r-node-1.log, no --test-threads=1); alone it was 6.05 s here. This is the
same kind of error B7-R1 was about, in the text that corrects it.

Asked for: Write 'no write seen after the Stop / after the uplink read again
(one write every 0.1 s)' in place of the µs values, give the reading-uplink
ack as 59 µs, and drop or re-run the 13.0 s.

Tests the re-check found weak:

- crates/node/tests/control_under_load.rs
  `a_stop_ends_a_streaming_command_while_the_uplink_takes_nothing` and
  `a_stop_after_an_approval_answer_...` (both #[ignore]): they fail today and
  can go green, but `assert!(m.stalled.is_some())` accepts any stop within
  `bound()` = grace + 1 s = 6 s. A fix that only lets the wait see the abandon
  token would end the command at 5 s, recorded as abandoned (see B7-D2), and
  both would pass; the passing control asks for under 1 s. They also shut the
  uplink completely, so they cannot see the trickle case of B7-D1.
- crates/node/tests/control_under_load.rs
  `a_stop_ends_a_silent_command_while_the_uplink_takes_nothing` (runs in every
  suite, 4.9 s alone): can fail (a silent command not stopped gives None). Its
  `d < 1 s` compares the start time of a 0.4 s look, so it means 'stopped
  within about 1.4 s', not the 14 µs the docs quote. Its fill step is not
  fragile: the flooders can make 1,632 messages against the 256 needed (N4).
- crates/link/tests/control_under_load.rs
  `the_first_control_frame_reaches_the_node_while_its_uplink_is_full`: after
  the fix its only assertions are that the queue was full and that the test's
  own model loop received the first frame on an idle direction within 1 s. It
  can fail only if the pump stopped reading while its writes are blocked,
  which crates/link/src/pump.rs rules out by construction; it costs 9.3 s in
  every run of the crate. No change in crates/node can turn it red or green,
  as its own doc now says.
- No new test covers the timeout on a full queue (B7-D1); the claim stays 'by
  reading; not run' in the plan although it reproduces in 75 s (N3 in my
  proofs).

## 6. Not checked

- The whole workspace test suite and `cargo deny check` (not allowed, not
  available).
- CI behaviour of the new 9 s timing test on a GitHub runner (buffer sizes,
  two cores).
- A real WAN path or a kernel with other tcp_wmem settings; everything ran on
  loopback on this machine.
- The real NodeRuntime behind the real TLS link and proxy together: the node
  proof uses the in-process link, the link proof uses the builder's model.
- The web UI's behaviour when Stop answers 504 or the turn's end arrives late.
- Web checks (nothing under web/ changed).

## 7. After the re-check (the co-owner's closing)

B.7 is an experiment: tests and documents only, no product code. Its value is
the result, so the review was about whether the written result is true.

Done on the branch at `cba69dd`:

- **B7-D1 to B7-D4** corrected in `PLAN.md` T9, the nodes note and the plan's
  result: the hold lasts until the uplink has taken one message for every
  append waiting ahead (14 messages: 2.5 s at one every 200 ms, 14.0 s at one
  a second), and nothing caps it, because the link's idle limit times each
  frame on its own; the grace counts from the Stop and runs out unseen, and a
  Stop held past it ends the tool as abandoned, never as cancelled; real
  `bash` output is about 8.1 KiB an event and a full out queue about 1.4 MiB;
  the figures in the node test say what was observed.
- **A timeout does not end a streaming command while the uplink takes
  nothing.** The re-check's 75 s reproduction is now the ignored test
  `a_timeout_ends_a_streaming_command_while_the_uplink_takes_nothing` (still
  writing 66 s after the call started, with a 60 s timeout). T9's "never
  outlive its timeout" now points at this residual.
- **Weak tests.** The two failing real-node tests required a stop within 6 s,
  which a fix that only abandons at the grace would pass; they now require
  under 1 s, like the control. Two tests that could not fail are ignored
  measurements. Left: nothing covers a trickling uplink; the tests shut it
  completely.

Decided:

- **The fix is task A.21** in `ADOPTION_PLAN.md`, a security task, placed
  after N.10a part 1 and before part 2: (a) the node's command loop never
  waits for room in the out queue before it reads the next command; (b) acks
  of cancel, approval answer and `stop_background`, and only those, go ahead
  of queued output (a create's ack must still follow its `session_created`);
  (c) the out queue is bounded in bytes as well as messages; (d) the agent
  loop keeps polling the tool, the cancel and the timeout while an emit
  waits. Part (d) was added by the finishing builder and is accepted: without
  it the streaming-command test cannot pass, because that hold is in the
  loop's emit, not in the command loop or the pump. Done when the ignored
  tests of this branch pass un-ignored on the real node and a person's Stop
  answers within the 15 s `ACK_TIMEOUT` at 256 KiB/s.
- The row said a failure is "a fix before Gate 3". Gate 3 was called on
  2026-11-12 without this experiment having run. The record now says so; the
  order of A.21 is the owner's to change.

Open: the acceptable delay for a Stop and an approval on a slow uplink is
the owner's number and decides how far A.21's part (c) goes. Everything was
measured on loopback on one machine; a real WAN path was not.
