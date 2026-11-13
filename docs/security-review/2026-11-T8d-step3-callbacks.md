# T.8d step 3 review, 2026-11-13: callback turns

Reviewed: `feat/T.8d-step3-callbacks` at `1bf485e` (`git diff 5c3b2ec..HEAD`):
`0b06c86`, `3a6a80c`, `edb3d21`, `1bf485e`. Nothing pushed. One independent
reviewer who did not write the code; the diff read in full, then attacked on
the branch's own build (the binary built after the last commit, pinned by a
hard link under `target/review_t8d_step3/`). No product or builder test
source was changed. The reviewer ran no cargo command (the builder's whole
suite held the worktree); the proofs are Python against the real binary,
under `crates/cli/tests/support/review_t8d/` prefixed `step3_`. Machine: Arch
Linux, kernel 7.2.15, bubblewrap 0.13.1, the repository on a rotational USB
disk. Format and severity as in the step 1 and step 2 records.

## 1. Verdict

**Fit after fixes.** Nothing critical or high. The bounds that make an
unattended turn tolerable are the node's and held under everything tried: the
budget of turns without the person, the step cap, no delegation, the id
checks against the node's own log, and `no_callbacks`. One medium finding
breaks a stated promise (a callback turn files no memory), with a failing
proof on the real binary; the rest are low.

| Id | Severity | What | Proof |
|---|---|---|---|
| S3-1 | **Medium** | A callback turn's `remember` is filed as a proposal once the owner's message is refused while it runs (or the master restarts during it) | `step3_memory.py`, fails |
| S3-2 | Low | The shipped demo provider obeys the first line a background command printed | by reading, and every proof here relies on it |
| S3-3 | Low | Each watcher check reads and parses the chat's whole last turn from the mirror | `step3_scan.py` |
| S3-4 | Low | An end can land between the node's history read and `turn_started`: the opening's bytes differ on rebuild, or the end never calls back | by reading |
| S3-5 | Low | The 60-minute rule and the start scan trust the node's clock | by reading |
| S3-6 | Note | Admission is checked, then the turn is sent: an owner disabled in between gets one callback turn | by reading |
| S3-7 | Note | The README's cost bound is true in steps, not in model calls or commands | by reading |
| Item 9 | Low (merged code) | An owner removed from a team workspace keeps their background commands; step 3's workspace check for it fails open | by reading |

How to run the proofs (temp folders under `target/review_t8d_step3/`, ports
the kernel picks):

```text
RIZZY=$PWD/target/debug/rizzy-harness python3 -I crates/cli/tests/support/review_t8d/step3_memory.py
RIZZY=$PWD/target/debug/rizzy-harness python3 -I crates/cli/tests/support/review_t8d/step3_bounds.py
python3 -I crates/cli/tests/support/review_t8d/step3_scan.py
```

## 2. Findings

### S3-1 (medium): a callback turn's `remember` is filed once the owner's message is refused

`Master::prompt` records the chat's memory allowance (`turn_memory`, keyed by
chat, not by turn) before the node has accepted anything. `file_proposal`
trusts that map. The loop emits `tool_call_started` for any tool name, also
one the turn was not offered, before the tool answers "unknown tool". After a
master restart there is no entry, and no entry means allowed.

So the promise "a callback turn gets no `remember`; program output must not
steer what is remembered" held only until someone called `Master::prompt`
for the chat again:

- While a callback turn runs, the owner types a message. The node refuses it
  ("a turn is already running"), but the master has already overwritten the
  chat's allowance with the owner's. The callback turn's next `remember`
  call is filed as a proposal.
- A master restart during a callback turn does the same.
- The reverse: a callback request the node answers "nothing to resume" has
  already set the chat's memory to none, so the person's running turn loses
  its proposals.

Limits: the result is a *proposed* fact that the owner must confirm, and the
model must call a tool it was not offered. The agentic default agent has
personal memory off, so in a default setup this needs a project chat.

```text
== control ==
    their results: ['waited\n[exit code: 0]', 'unknown tool `remember`']
    alex's memory before: 1 entries; new after the callback turn: nothing
== refused ==
    alex's message while it ran: 409 {"error": "a turn is already running"}
    alex's memory before: 1 entries; new after the callback turn: [('projects', 'proposed', 'PLANTED-refused: …')]
RESULT: program output filed a memory proposal: control False, after the owner's refused message True
```

Asked for: `file_proposal` decides from the log (the call's `turn_started`
has a `cause`: file nothing), which survives a restart; and a prompt the node
refuses leaves the chat's allowance as it was.

### S3-2 (low): the shipped demo provider obeys program output

`mock.rs` runs the first quoted output line of a callback opening through the
mock's whole command language (`run:`, `script:`, `read:`, `publish:`,
`delegate:`, …). It is the default provider of `serve`. With the default
`ask` mode a demo user gets an approval card for a command a program's
output chose, presented as the agent's; with `auto` it runs. The first-run
experience demonstrated the attack the opening is written to resist. Asked
for: a switch, off by default, set only by the tests that need a steered
model.

### S3-3 (low): each watcher check reads the whole last turn

`Store::events_since_last_turn` read every event after the floor and filtered
by kind in Rust. A check runs 300 ms after every end that may call back,
every 15 s while the node is away (up to 60 minutes), and at master start for
every chat updated in the last 65 minutes, with or without a background
command. The class of step 2's S2-2 again.

```text
a last turn of 600 steps (64 KiB a result; 36605 events): one check reads 36603 rows, 45.5 MiB: 88 ms; with the kind in the SQL: 1 row, 91 bytes, 12.3 ms
    while its node is away for an hour: 240 checks, 10.7 GiB read under the mirror's lock for this one chat
```

Asked for: the kinds in the SQL; at start, only chats with a
`background_ended` in the window.

### S3-4 (low): "the log it reads is the log the turn joins" is not true

`on_background_end` drops the turn lock before it appends. An end whose "no
turn running" check passed just before `callback` claimed the slot is
appended after the history read, with `delivered: false`. Before
`turn_started`: the live opening lacks it and every rebuild has it, and it
sits below the floor so it never calls back. After it: the master asks, is
told "a turn is already running", and nothing asks again when the turn ends.
The same window exists for `prompt` since step 1. Related: a later compaction
whose cut falls at a callback's `turn_started` puts the notes into the
summary, and the rebuilt opening is the lead line alone. Correctness and
cache only, not a bound.

### S3-5 (low): the 60-minute rule trusts the node's clock

A node whose clock is ahead never ages out: an end is resumed whenever the
node next connects, and a node that is away is retried every 15 s without
end. A node more than 60 minutes behind has every end refused with a
misleading reason, and its chats are skipped at master start. Asked for: age
by the master's own clock.

### S3-6 (note): admission, then send

An owner disabled in the milliseconds between admission and the node's
`turn_started` gets a callback turn that nothing cancels until the node's
next hello (the sweep cancels only what the mirror shows running). The same
shape as the owner's own prompt racing a disable.

### S3-7 (note): the README's cost bound

"Up to two summarising calls a turn" is two summarising passes, each one call
per piece; fallback models add calls per step; one step may carry any number
of tool calls, so 60 steps is not 60 commands.

### Item 9: the builder's "found in what is merged", confirmed, low

After a removal from a team workspace the owner's chats are back in their
Personal workspace, and the revocation sweep asks whether the owner is
blocked *there*, so neither a running turn nor the background commands stop.
§4.7's row "Owner leaves the workspace" was not met by steps 1 and 2. How
bad: little. Nodes and folders are not workspace-scoped, so the removed
member could open the same folder on the same node from Personal anyway.
Step 3's `callback-workspace` check for it fails open three ways (no kv row,
a failed write, a command started after the removal).

### Older, relevant to the opening

`transcript()` gives the summariser the first 2000 characters of each tool
result unquoted, so a result can contain its own `User: …` line, and the
summary lands in the system prompt. The callback opening cannot do this
(every output line is behind `| `). Not introduced by this branch; open.

## 3. Attacks that held

`step3_bounds.py`: `RESULT (True: the bound held): {'a refused message and a
Stop give no callback turns back': True, 'a callback turn does not delegate':
True}`.

- **Budget.** A self-restarting command got exactly 3 callback turns, then
  `callback_refused: 3 turns in a row without you (node limit)`, with the
  owner sending a refused message in each turn and pressing Stop on the
  second. Only `Session::begin` writes `user_message`, and only three
  callers reach `Master::prompt`; restore, select-variant, compaction and a
  node restart write none.
- **Delegation.** The same `delegate:` line started a sub-agent as the
  owner's message and got ``unknown tool `delegate` `` with zero child chats
  as program output.
- **By reading.** Ids must match an undelivered exit or timeout after the
  floor in the node's own log, so forged, foreign, stale and duplicate ids
  fail. Two callbacks at once are serialised by the session lane. The step
  cap is `min` with the node's `max_steps`, and `TurnExtra` is not on the
  wire. `no_callbacks` comes only from `session_created`. A callback never
  reopens a closed table. Approvals are per turn; nothing auto-approves. The
  opening is not URL provenance, not `own_words`, not indexed; quoting
  replaces control characters and U+2028/2029. Attention rows are filtered
  by owner; the `background` count is only on rows already visible. Audit
  and log lines carry ids and reasons only. Retry is 15 s, one task per
  chat, and ends when the chat is deleted.

## 4. Claims with no test at `1bf485e`

- A callback turn gets no memory and no context; `PromptOpts.callback` with
  text or context is refused.
- No sub-agents in a callback turn (removing `a.delegates.clear()` failed no
  test).
- The routine runner really sends `no_callbacks: true`.
- Admission for a pending password change, a chat with no owner, a
  sub-agent's chat, a lost agentic grant, the local-node rule, and the
  fail-closed store errors.
- Retry when the node is away, slow or stopping; a node without
  `CAP_CALLBACK` is not asked.
- More than 64 ids; a stopping node; the summariser writing the opening
  without `User:`; a node clock ahead.
- Weak: `a_removed_owner…` relied on one `yield_now()` for a spawned write.
- The real-binary tests return early without bubblewrap unless
  `HARNESS_REQUIRE_BWRAP=1`.

## 5. Not checked

- No cargo test, clippy or fmt by the reviewer.
- The web UI beyond reading the diff; `web/e2e/callbacks.cjs` not run by the
  reviewer.
- A hostile node flooding `background_started` into the master's in-memory
  map (by reading: unbounded, but a node is admin-joined).
- A second remote node; a node or master restart in the middle of a callback
  turn on the real binary.

## 6. Left clean

Three proofs in the working tree, scratch under `target/review_t8d_step3/`
(three `run-*` data folders and the hard-linked binary). No processes left;
every run printed "cleanup killed: nothing".

## 7. Re-check of the fixes, `1bf485e..e6a3857`

The same reviewer, on the builder's ten fix commits. Targeted runs only:
`callbacks_real` 4 and `callbacks` 8 passed (sandbox and FUSE required),
`callbacks_http` 9, `hooks_http` 15, `event_log` 25, the loop's callback
tests 4; `step3_memory.py` and `step3_bounds.py` on the binary pinned at
`e6a3857` (both proofs now write `mock_obeys_output = true`, and
`step3_memory.py` gained the default-mock case).

| Finding | Status | Evidence |
|---|---|---|
| S3-1 | Closed | `step3_memory.py`: `control False, after the owner's refused message False`. `file_proposal` returns unless the call's `turn_started` is in the mirror without a `cause` |
| S3-2 | Closed | Without the key: `tool calls 0; its answer: ['Noted: the command ended.']; the file steered exists: False`. Only the operator's `config.toml` can set it; the HTTP provider edit and the environment write `false`; it is not on the node protocol |
| S3-3 | Closed | The kinds are in the SQL; the store test passes. The start filter is right by reading and has no test |
| S3-4 | Closed for callbacks | `an_end_racing_the_callback_lands_before_its_turn`. The same gate in a person's prompt has no test. No deadlock found: the order is always turn slot (claimed, not held), `bg_gate`, `head`. The test hook is behind the `testing` feature, enabled only from `crates/cli` dev-dependencies |
| S3-5 | Partly closed, and it opened R-1 | Clock skew no longer decided, but see below |
| Item 9 | As decided; not unsafe | Resuming a removed member's chat in Personal is what they can do by hand, with Personal's agents and keys. Revisit if nodes or folders ever become workspace-scoped |
| S3-6 | Open, as recorded in §13 | |
| S3-7 | Closed | README and §13 |

**R-1 (low), opened by the S3-5 fix:** an end of any age was resumed when
the master first saw it. An end recorded while the link or the master was
down is replayed at the next hello and was stamped "now", so a laptop node
offline for five hours started an unattended turn for a command that ended
five hours earlier, against §4.4, §4.7 and the README. The builder's own
test asserted it as intended. Asked for: age by the older of the two times.

Smaller points of the re-check:

- "Off is silent" had two holes at the master: the age refusal and admission
  ran before the capability check, so a blocked owner or an old end still
  got a "not resumed" row on a node that would never be asked.
- The `turn_memory` restore was tested by nothing (the S3-1 test passed on
  the log check alone), and two early returns between the insert and the
  send skipped it. The restart half of that test had no positive control.
- Stale: `PLAN.md` T36 ("uses the node's clock", "not reviewed") and one
  sentence of §13.
- `too_many_ids_and_a_stopping_node_are_refused` reaches the stop refusal in
  `starts_work`, not the branch inside `Session::callback`.

## 8. After the re-check (the co-owner's closing)

Done on the branch, tests first and seen failing on the code before:

- **R-1.** An end is aged from the older of its `ts_ms` and the moment the
  master first saw it. A node clock that is ahead keeps nothing fresh; an
  end replayed late starts nothing and the owner is told why; a node whose
  clock is more than 60 minutes behind gets no callback turns (fails closed,
  written in §13 and `PLAN.md`). Test
  `an_end_is_aged_by_the_older_of_the_two_clocks`, which failed with
  `reason: "resumed"` before.
- **Off is silent at the master.** A connected node without `CAP_CALLBACK`
  is not asked, and nothing is refused on its behalf: the check comes before
  the master's own refusals. The stale-end case in
  `a_busy_or_away_node_is_asked_again_and_one_without_the_capability_never`
  failed once R-1 was in and the order was not.
- **The `turn_memory` restore** now has its own test,
  `a_refused_prompt_leaves_the_running_turns_memory_as_it_was`: an untrusted
  turn runs, the owner's message is refused, the untrusted turn's `remember`
  is not filed. With the restore disabled it failed with `filed:
  ["steered-by-the-event"]`. That is the S3-1 class in code older than step
  3 (an untrusted turn, not a callback turn), closed by the same fix. The
  entry is now recorded only after the last early return, right before the
  send. The restart half of the S3-1 test has its positive control.
- Docs: `PLAN.md` T36, §13, README.

Decided, not changed:

- **S3-6** stays: one callback turn of at most `callback_max_steps` for an
  owner disabled in the instant between the master's admission and the
  node's `turn_started`, the same as their own prompt racing the disable.
  Cancelling every idle chat of a disabled owner to close it costs more
  than it buys. In §13 and `PLAN.md`.
- **Item 9:** the design note said a member removed from a team workspace
  has their background commands stopped; the code never did. The note is
  corrected rather than the stop added, and step 3's workspace check is
  deleted: nodes and folders are not workspace-scoped, so the stop would
  revoke nothing the member cannot redo from Personal. The owner may
  overrule this.

Open after this branch:

- The summariser reads the first 2000 characters of each tool result
  unquoted (older than this branch; a result can carry its own `User:`
  line into a summary). Its own task.
- Two person prompts racing with different agents can leave the loser's
  earlier memory entry for the winner's turn (harmless for callback turns,
  which the log decides).
- Untested: the start filter of S3-3, the gate in a person's prompt (S3-4),
  the retry cap, the `is_stopping` branch inside `Session::callback`.
- Not attacked: a hostile node flooding `background_started`, a second
  remote node, a restart in the middle of a callback turn on the real
  binary. R-1 was not shown on the real binary (it needs a remote node with
  its link cut); the master test with the fake node stands for it.
