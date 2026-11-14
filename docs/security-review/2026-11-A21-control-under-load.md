# A.21 review, 2026-11-14: Stop and approvals on a full node uplink

Reviewed: `feat/A.21-control-under-load`. First review at `8c28173`
(`git diff origin/main...HEAD`: tests `bd4805c`, fix `8c28173`); re-check of
the fixes at `c9be431`; the last round's fixes at `fee3606`. One independent
reviewer who did not write the code, with proofs against the real node in
`crates/cli/tests/support/review_a21/` (`run_proofs.sh` copies
`review_a21_proofs.rs` into `crates/node/tests/` for one run and removes it).
Machine: Arch Linux, kernel 7.2.15, loopback only.

## 1. What A.21 fixes

Experiment B.7 (`docs/security-review/2026-11-B7-control-frames.md`) showed
that while a node's out queue is full, because the node produces faster than
its uplink carries, a Stop does not end a streaming command, a call's
timeout is held, a Stop after an approval answer is not read, and the acks
of Stop and approval answers reach the master after its 15 s `ACK_TIMEOUT`.

Built:

- The acks of cancel, approval answer and `stop_background`, and refusals
  past a lane's depth or the lane cap, go on an urgent lane ahead of queued
  output (`publish_urgent`, never waits). Every other ack, a create's
  included, stays in order (`Master::create_session` relies on a create's
  ack coming after its `session_created`).
- `crates/node/src/outbox.rs`: the ordered lane is bounded at 256 messages
  and 1 MiB (a larger message goes alone), the urgent lane at 64 unread
  acks, at most 8 urgent acks in a row while output waits; one message at a
  time to the link. `SNAPSHOT_BYTES` 512 KiB.
- `coalesce_led` keeps polling the tool and the cancel while an append
  waits, and drains what piled up in 32 KiB batches.
- `TCP_NOTSENT_LOWAT` = 128 KiB on the node's socket (`link::UNSENT_LIMIT`,
  Linux and Android): the kernel's own send buffer had held up to 4 MiB
  ahead of every ack.
- `crates/node/src/audit.rs`: the command loop acts, acks, then hands the
  audit line to one writer (1,024 lines queued, batches in one
  transaction); a line it cannot write is counted and the count recorded.
- An inline command for a session the node does not have is refused at
  once; at most 256 sessions have a lane.

The builder rejected removing back-pressure altogether (appends never
waiting, the link reading the log): back-pressure also bounds how far an
agent runs ahead of what the person sees, so a Stop stops something the
person saw. The reviewer agreed.

Measured on the real node, 12 sessions flooding output:

| | Before | After |
|---|---|---|
| Stop of a streaming command, uplink taking nothing | still writing 6.04 s after | no write after the Stop |
| A 15 s timeout, queue full | still writing at 21.1 s | ended |
| Stop after an approval answer | still writing 6.03 s after | no write after the Stop |
| Stop's ack over the real link at 256 KiB/s | 20.1 s | 0.5 to 1.0 s |
| The same with the urgent lane but without the socket cap | | 8.9 s |

## 2. First review: fit after fixes

| Id | Severity | What | Done |
|---|---|---|---|
| A21-1 | **High**, a regression from this change | Once 64 audit lines waited on a slow or locked store, a Stop waited for the store before it acted, and the command loop read nothing else (B.7's defect again, through the disk) | The command loop acts and acks first, then hands the line to one writer; it never awaits the store |
| A21-2 | Medium, older | A Cancel for an unknown session cost one task each: 20,000 cancels, 20,000 tasks, +199 MB | Refused at once; at most 256 lanes |
| A21-3 | Low | Model output that piled up behind a waiting append went out as one 77 KB event | Drained in batches |
| A21-4 | Low, test gap | Nothing tested the socket cap | Read back from the socket; the link test asserts under 3 s |
| A21-5, A21-6 | Low, note | Ordered acks still pass the 15 s limit below about 75 KiB/s; a frame the uplink cannot carry in 60 s drops the link | In `PLAN.md` T9 |
| A21-7 | Note | The tests' "queue full" detector could be fooled by a slow disk | Read from `NodeRuntime::backlog()` |
| A21-8 | Note | An urgent ack can overtake `Hello` if a master sends a command before reading it | Harmless; not changed |
| A21-9 | Note, older | `Store::call` panicked on a task cancelled at shutdown | `StoreError::Stopped` |

Found sound: one session's events stay in `seq` order across both lanes; no
master path relies on events arriving before a Cancel, ApprovalRespond or
StopBackground ack; a create's ack is never urgent by any path; a started
append is never dropped or run twice; the socket option is set on the raw
socket before TLS; `socket2 =0.6.5` is already in the tree through tokio
(MIT or Apache-2.0); B.7's assertions were not loosened.

## 3. Re-check of the fixes: one small fix left

A21-1, -3, -4, -7 and -9 closed, A21-2 closed for the task pile-up; every
claim rerun by the reviewer. New:

| Id | Severity | What | Done |
|---|---|---|---|
| D-1 | Low, proved | A refusal past the lane cap went into the ordered queue and was dropped while it was full: 0 of 44 reached the master | Refusals go on the urgent lane |
| D-2 | Low/medium, by reading | A clean stop did not wait for the audit writer, so queued lines and the count of lost ones vanished | `flush` waits for the writer within its bound and logs at error level what it could not write |
| D-3 | Note | A dead audit writer was silent | Logged once at error level |
| D-4 | Note | Audit rows no longer follow command order | `PLAN.md` T5 says so; `ts_ms` keeps arrival order |
| D-5 | Note | The drain's 2 KiB batches meant about 40 appends after a Stop | 32 KiB batches in the drain only |

## 4. The co-owner's closing

Accepted, and written in `PLAN.md` T5 and T9:

- **Refusals share the urgent lane's 64 unread slots** (a consequence of
  D-1): a burst of more than 64 refusals keeps 64 and the rest time out at
  the master. Only a master sending commands for more than 256 sessions at
  once produces such a burst; the test asserts the task cap and at least 64
  refusals, a weaker assertion than before D-1.
- **A crash loses at most the queued audit lines** (up to 1,024, usually
  milliseconds' worth) and one unwritten count. The cross-check is the
  session's log, on the node and in the master's mirror.
- **Ordered acks** (create, prompt, delete, restore) still pass the 15 s
  limit below about 75 KiB/s, so a person may get a 504 for a prompt that
  runs.
- **The back-pressure is node-wide, not per session.** One session's flood
  still slows another session's normal events and creates, never its Stop.
  A per-session round-robin ordered lane would keep the back-pressure and
  give fairness: recorded in the A.21 row as a follow-up, not built.

Not measured: a real WAN or high-latency path (whether the 128 KiB socket
cap costs throughput there; what would settle it is bulk replay over netem
at 100 to 300 ms and 10 to 1,000 Mbit/s, with and without it), macOS (no
cap there), a master whose mirror is slower than the uplink.
