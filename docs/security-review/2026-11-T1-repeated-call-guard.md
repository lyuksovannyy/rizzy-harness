# T.1 review, 2026-11-13: the repeated-call guard

Reviewed: `feat/T.1-repeated-call-guard`. One independent reviewer who did not
write the code read `git diff origin/main...HEAD` after the builder finished
(builder's head `22818fe`), with proofs where it could; the builder then fixed
the findings (head `7f708d4`), and a second, fresh reviewer re-checked the
fixes. Format and severity as in the earlier records in this folder. The texts
below are the reviewers' own, shortened.

## 1. Verdict of the first review

**Fit after fixes.**

| Id | Severity | What | Where |
|---|---|---|---|
| T1-R1 | High | The guard never fires on a bash result the harness itself makes different each time (random tool_output id, stop milliseconds) | `crates/agent-native/src/lib.rs:1617` |
| T1-R2 | Medium | Two unchanging successful commands hide the loop between them: the reset forgets what it needs to judge 'new' | `crates/agent-native/src/repeat.rs:118` |
| T1-R3 | Medium | Only the agent's own successful Mutating call restarts a count: a wait between changing polls, a sub-agent's change and a background command's end do not, and the turn is ended | `crates/agent-native/src/lib.rs:1612` |
| T1-R4 | Low | After the refusal that says 'the turn ends here', the rest of that message's calls still run | `crates/agent-native/src/lib.rs:1595` |
| T1-R5 | Low | The guard's last note of an ended turn is swept into the next callback turn's opening | `crates/agent-native/src/provider.rs:160` |
| T1-R6 | Note | PLAN.md's event table still says `output` is all the model sees of a result | `PLAN.md:308` |

## 2. Findings

### T1-R1 (high): The guard never fires on a bash result the harness itself makes different each time (random tool_output id, stop milliseconds)

The digest is taken over the whole logged output (repeat.rs:109-111). With
shell-output filtering on, which is the default of every agentic session
(core/src/savings.rs:58,79), bash appends `[filtered: N of M lines not shown;
full output: tool_output id=<random>]` whenever the filter drops a line or the
output is cut (tools/src/bash.rs:586-601, outputs.rs:74 random_id). A failing
`cargo build` or `cargo test` always prints `Compiling ...`, which the filter
drops, so every run of the same failing command is a 'new result': no warning,
no refusal, up to max_steps. Worse, a successful filtered command (exit 0)
counts as 'a change with a new result' every time and clears every other count
(repeat.rs:118-120). The same holds for a contained command that times out:
its result ends `its guard confirmed the end (N ms)` with a measured N
(bash.rs:666-669, contain.rs:606,620; by reading). The research the row cites
names this trap (round-2 appendix line 1821: OpenClaw masks volatile tokens).
PLAN T39 and the README claim 'the same failing command' is covered; in the
default configuration the commonest one is not.

Proof: sh crates/cli/tests/support/review_t1/run_rust_proofs.sh (real loop,
real bash, savings = TokenSavings::default().for_turn(false)).
f1_a_filtered_failing_build_is_never_the_same_result: 8 identical calls, 8
runs, no note on any; results differ only by the id: `error[E0308]: mismatched
types / [filtered: 1 of 2 lines not shown; full output: tool_output
id=e7b5872348defe99] / [exit code: 101]`, then `id=247eae90ecb7cbcf`,
`id=eb6e89d014b2b8e5`, ... turn Completed. The control test with filtering off
refuses the 5th. The timeout case is by reading only.

Asked for: Digest the result without the harness's own volatile tokens: let
bash hand the loop the text to compare (the output before the `tool_output
id=` line and without the stop's milliseconds), or strip those two tokens
before hashing. Add a loop test with the real bash and filter_shell_output on
that fails today (the 5th identical failing command with a `Compiling` line is
not run).

### T1-R2 (medium): Two unchanging successful commands hide the loop between them: the reset forgets what it needs to judge 'new'

Progress is 'a successful Mutating call whose result is new for that call',
and on progress `self.calls.clear()` drops every other entry, including the
other Mutating calls' last digests. bash is Mutating. In a cycle A, B, X (two
successful shell commands with constant output, e.g. `git status` and `ls`,
then a failing build) A's entry was just cleared by B and B's by A, so each is
'new' every round and X's count never passes 1. The builder's claim covers
only one command in between (`a_repeated_change_is_not_progress`) and read-
only A/B (`alternating_calls_are_caught`). This is a natural stuck-loop shape,
not an evasion: nothing in the calls changes.

Proof: Same script, f2_two_unchanging_commands_hide_the_loop_between_them: 10
rounds of [bash `git status 2>&1 | head -1; true`, bash `ls`, bash `echo run
>> runs; echo 'error: nope'; exit 1`], filtering off. 30 calls, the failing
command ran 10 of 10 times, no note on any result, turn Completed.

Asked for: On progress, restart the other calls' counts but keep their last
digest (set runs to 0 instead of removing the entry), so 'new for that call'
is judged against that call's last result in the turn. Test: the A, B, X cycle
refuses X the 5th time.

### T1-R3 (medium): Only the agent's own successful Mutating call restarts a count: a wait between changing polls, a sub-agent's change and a background command's end do not, and the turn is ended

(a) Wait, then look: `bash sleep 30` between polls whose answer changes every
time (bash_output of a running command, `read` of a growing log). The poll is
read-only, so the wait is counted on: the 5th sleep is refused and the 6th
ends the turn with an error, although every poll showed the world moving. The
tool descriptions say 'you are told at your next step', so a model waiting for
a background command has to take steps. (b) A test run after a sub-agent's
edit: delegate results never reach the guard (lib.rs:1545-1568), so [test,
delegate] x4 then test (max_subagents defaults to 4, core/src/agent.rs:137; a
writing sub-agent's changes are applied to the folder) refuses the 5th test,
while the same loop with the parent's own `edit` is allowed for ever. (c) A
background end delivered at the step boundary (lib.rs:1673-1682) does not
touch the guard: a check refused before the end, asked again after the harness
itself said the command ended, ends the turn. In all three the note's 'with
nothing changed in between, so it would again' is untrue, and 'N times in a
row' (notes, REFUSED_OUTPUT, README) is not what is counted: calls in between
do not break the row.

Proof: Same script, f3_a_wait_between_changing_polls_ends_the_turn: [bash
`sleep 0.05`, read-only poll answering 'bg-1 (running for 30 s)', '60 s', ...]
x6. Calls 3, 5, 7 carry the warning, call 9 (5th sleep) `Not run`, call 11
`Not run` with 'the turn ends here'; every poll's answer was distinct; turn:
Error 'stopped: the model asked again for a call the repeated-call guard had
refused'. (b) and (c) by reading only.

Asked for: Restart the counts on a delegate that returned ok and on background
ends delivered at the step boundary. For the wait: either treat a result that
differs from the same call's previous result in this turn as a change (any
tool), or keep today's behaviour as a written decision: say it in PLAN T39's
residual and the README, and pin the sequence with a test. Either way correct
the wording ('in a row', 'with nothing changed in between') to what is
counted.

### T1-R4 (low): After the refusal that says 'the turn ends here', the rest of that message's calls still run

`repeat_stop` is only read at the step's end (lib.rs:1683). A message with
[the twice-refused call, another call] gets the note 'This call was not run,
and the turn ends here' on the first and then runs the second (with approval
asked in `ask` mode) before the turn ends. The harness's own line is
contradicted by what it does next, and the person may be asked to approve a
call in a turn already decided to end.

Proof: Same script, f4_calls_after_the_final_refusal_still_run: step 6 is
[refused call, bash `echo ran > after.txt`]; call 6 has the 'turn ends here'
note, call 7 ran with ok=true and after.txt exists; turn: Error.

Asked for: Once a refusal sets stop, do not run the remaining calls of the
step (give each a 'Not run: the turn was ended' result, or leave them to the
log's interrupted-call closing), or change the note to say the turn ends after
this step.

### T1-R5 (low): The guard's last note of an ended turn is swept into the next callback turn's opening

The note is a plain trailing `ChatMsg::Note` after its result (lib.rs:713 and
1662). `open_callback` pops every trailing Note into the callback turn's
opening, which is documented as the ended commands' notes. A turn that the
guard ended (or that ended right after a warned result) while a background
command was still running is exactly the F3 case; when that command ends, the
callback turn opens with '[harness] This call was not run, and the turn ends
here: it was refused before and you asked for it again.' and the refused
result no longer carries it. A model reading its new turn's opening is told
the turn ends here. Live and rebuilt bytes agree, so this is a wrong message,
not a cache problem.

Proof: Same script,
f5_the_guards_last_note_moves_into_a_callback_turns_opening: the events of a
guard-ended turn plus background_ended(bg-1) and turn_started{cause: callback}
through history_to_messages and place_notes. Last result: `Not run: this exact
call gave ...` with no harness line; the opening: '[harness] The person has
NOT written anything. ...' then '[harness] This call was not run, and the turn
ends here: ...' then the bg-1 note.

Asked for: Keep the guard's note apart from background notes (its own ChatMsg
variant handled like Note in place_notes, or carried on ChatMsg::Tool), so
open_callback and the pop at lib.rs:1293 take only background ends. Test: a
callback turn after a guard-ended turn.

### T1-R6 (note): PLAN.md's event table still says `output` is all the model sees of a result

The schema row for tool_call_finished lists `call_id, ok, output, exit_code?,
truncated` and '`output` is what the model sees'. With `note` the model sees
more than `output`, and the person sees less than the model (the UI fold and
fold.ts ignore `note`, so warnings are invisible to the person). CLAUDE.md
asks for the schema note to move with the code.

Proof: by reading only

Asked for: Add `note?` to the row with one clause: written only by the node's
loop, sent to the model after the escaped output, not shown in the UI.

## 3. Attacked and found sound

- Admission is before approval and dispatch (lib.rs:1592-1609): a refused call
  is not offered to the person and nothing starts; the control proof with the
  real bash runs 4 of 5 and the builder's a_person_is_not_asked_a_fifth_time
  holds.
- The harness line cannot be forged by tool output: place_notes appends the
  note after escape_leads has escaped the result's own lines, the note texts
  are constants with no argument or output text, and the count is fed only
  what was logged.
- A compromised master cannot plant `note`: events travel node to master only
  (protocol NodeToMaster::Event/Snapshot); the only writers of
  ToolCallFinished are in agent-native, all with note: None except the
  guard's.
- The same call: the existing SHA-256 call_key (tool, folder, canonical JSON
  with keys sorted recursively, write/edit by real path) is reused, no new
  parser. serde_json is built with preserve_order here (cargo tree), so
  key_order_does_not_make_a_new_call is a real test.
- The window is one turn: the guard is a local of run_turn, not seeded from
  history; callback turns and sub-agents get their own. A restart ends the
  turn, so 'state from the fold' is moot as the builder says.
- bash_output on a running command is not refused: its text carries 'running
  for N s' at one-second steps and a background command cannot outlive
  bash_timeout_max (at most 3600 s), so the coarser minute wording is never
  reached (by reading).
- No new dependency; the std SipHash digest can at worst cause a refusal,
  never a run.
- Constants instead of [limits] keys are acceptable once R1-R3 are fixed: no
  protocol or model input reaches them.
- The builder's red/green claims check out: target/tmp/t1-red.log shows the 7
  named tests failing at the named lines and the 5 others passing; each of the
  7 fails on the old code for the reason given.
- harness-node tests pass with the guard: 3 runtime tests timed out in the
  full run under load (one of them has no tool call at all) and all 3 passed
  rerun alone.

## 4. Claims with no test at the first review

- 'It costs nothing on the prompt-cache prefix' is tested only for one call
  per step in one plain turn (the_warning_and_the_refusal_rebuild_from_the_log
  compares tool-result texts only). Several calls in a step, a callback turn
  after it (R5) and mid-turn compaction are not tested.
- 'The summariser transcript also ignores it' is wrong by reading:
  transcript() goes through history_to_messages, so the note reaches the
  summariser as an unquoted harness line (lib.rs:353, 380). Harmless, but not
  what was reported.
- 'A poll whose answer changes is never refused' is tested with a stand-in
  tool only; no test uses the real bash, bash_output, read or edit, which is
  why R1 was missed.
- 'delegate keeps its own caps' is true, but the effect on the guard (R3 b)
  has no test.
- No test for a refusal in a step with several calls (R4), for approval in ask
  mode combined with a warning, or for a routine/unattended turn.
- How a real model reacts to the warning and refusal lines was not measured
  (the builder says so).
- The contained-timeout part of R1 (milliseconds in the result) was
  established by reading, not by a run.

## 5. What the builder did, and the re-check

| Id | Builder | Re-check | Evidence of the re-check |
|---|---|---|---|
| T1-R1 | fixed `d0b2601` | closed | The first review's proof f1 (written to pass while the weakness exists) now fails: with the real bash and TokenSavings::default().for_turn(false) the failing 'Compiling' command ran 4 of 8 times and the turn ended in Error (sh crates/cli/tests/support/review_t |
| T1-R2 | fixed `aed7d42` | closed | The first review's proof f2 now fails: in 10 rounds of [git status / head -1, ls, failing X] X ran 5 times and the turn ended in Error (5, not 4, because ls gained the 'runs' file once, a real change). repeat.rs:123-127 restart() zeroes counts and keeps each d |
| T1-R3 | fixed `fcc3f6e` | closed | (a) The first review's proof f3 now fails: 8 rounds of [bash sleep 0.05, changing poll] gave 0 refusals and a Completed turn. My d3 shows one leftover: the 2nd wait still carries the warning ('nothing changing in between ... wait inside one command') because t |
| T1-R4 | fixed `de7aa39` | closed | The first review's proof f4 now fails: after.txt is not created. In my d2 the call after the turn-ending refusal is closed with 'Not run: the turn was ended by the harness's repeated-call guard at an earlier call of this message' and tool_call_started/finished |
| T1-R5 | fixed `5e7075e` | closed | The first review's proof f5 now fails: the callback opening holds the fixed first line and the bg-1 note only, and the last result reads 'Not run: ...\n\n[harness] This call and the calls after it in this message were not run, and the turn ends here ...'. By r |
| T1-R6 | fixed `1252fc2` | closed | By reading: PLAN.md:308 lists `note?` with who writes it, where the model gets it and that the UI does not show it; the T39 row, README 'Repeated calls' and the T.1 row of ADOPTION_PLAN.md describe the guard as the code now behaves (what counts as a change, th |

New in the re-check:

### T1-D1 (medium): Opened by the R3 fix: a failed call whose text changes restarts every count, so a failing test runner un-guards the whole loop, the denied approval included

`Some(_) => (1, true)` makes any new result of an earlier call a change,
whatever the tool and whether it failed. On 22818fe only a successful Mutating
call with a new result cleared counts (`if changes && ok`), so a failing
command never did. A failing test runner prints its run time (cargo test's
'Finished ... in 0.03s' survives the filter, pytest 'in 0.03s', jest 'Time:'),
so each rerun is 'a change' and zeroes every other call's count. In the loop
[failing test, the same read, the same change the person denies] nothing is
ever warned or refused: the person is asked for the identical denied call
every round up to max_steps. PLAN T39 names exactly that ('the same denied
approval asked of the person again and again') as mitigated; its residual says
generally that such a call restarts the others, but neither it nor the README
says the person is asked again, and no test pins it. Before the fix the read
was refused the 5th time and the 5th denied call was not offered (by reading
22818fe's record()).

Asked for: repeat.rs:147: `Some(_) => (1, ok)`, so a new result is a change
only when the call succeeded (a poll that answers still restarts the wait; a
failing runner's changing text restarts nothing). Test: d1's loop asks the
person 4 times and refuses the 5th read. Cost to state: a check that fails
while printing progress between sleeps is then warned from the 2nd wait and
refused the 5th. If the owner keeps today's rule instead, PLAN T39's residual
and the README must say that a denied call is offered again every round while
any other call's result changes, with a test that pins it.

### T1-D2 (low): The same call twice in one message ends the turn on a refusal the model was never sent (not from the fixes: in 22818fe too, missed by the first review)

`stop = s.runs > REFUSE_AT` counts asks, not refusals the model has seen. A
model that sends its calls in pairs ([X, X] per message) gets X run 4 times
over two messages, then in the third message the first X is refused and the
second is 'refused before and you asked for it again': the turn ends in Error
and, since the R4 fix, every later call of that message is closed unrun. The
model's last request held warnings only; it never had the chance the first
refusal's note promises ('Try a different approach ... Asking for it again
ends the turn'). The note and STOP_ERROR state something that did not happen.

Asked for: Stop only on a refusal the model was sent: a second ask in the step
that made the first refusal is refused again with stop=false (for example a
per-step flag the loop clears at the step boundary, or a `told` bit on the
streak set there). Test: [a,a] three times leaves the turn running with two
plain refusals; the next message's ask ends it.

Tests the re-check found weak:

- repeat::tests::the_harness_own_volatile_tokens_are_not_the_result
  (repeat.rs:261): the three strings are hand-written, and two are not what
  bash.rs writes (the real ones read 'timed out after 5s (state
  `timeout_secs`, ...)' and '...; the kernel cannot end it yet (...'). Only
  the leads matter, and nothing ties VOLATILE_AFTER/VOLATILE_BEFORE to the
  format strings at tools/src/bash.rs:592, 646, 668: rewording any of them
  leaves this test green and the guard blind again. No test runs a contained
  timeout through the loop.
- repeat_tests::several_calls_a_step_rebuild_from_the_log
  (repeat_tests.rs:853): by the builder's own account it also passes on
  22818fe, so it does not test the R5 change. It compares live with rebuilt,
  both through the same place_notes, and asserts only that some result has a
  harness line, not which result carries which note.
- repeat_tests::a_test_run_after_each_delegation_runs (repeat_tests.rs:711):
  the stand-in delegate always completes. Removing the `if outcome.ok` at
  lib.rs:1551 (restart on a failed or errored sub-agent too) leaves every test
  green.
- repeat_tests::a_callback_turn_after_a_guard_ended_turn_opens_with_the_ends_o
  nly (repeat_tests.rs:814): goes through history_to_messages only. The other
  place the fix relies on, the post-compaction pop in a callback turn
  (lib.rs:1295), has no test with a CallNote before the trailing notes.
- repeat_tests::a_wait_between_changing_polls_is_never_refused
  (repeat_tests.rs:689): asserts no 'Not run' and a Completed turn, but not
  that the waits go unwarned; the 2nd wait is warned (my d3). Nothing pins the
  scope of the new rule either way: no test has a failed call with a changing
  result, so T1-D1's behaviour is neither asserted nor forbidden.

## 6. Not checked

- crates/cli and crates/master test suites (background.rs, callbacks.rs,
  callbacks_real.rs and the rest): not run. By search, the only scripted turn
  with 5 or more identical calls is cli/tests/background.rs:1426 (12 identical
  background starts), whose results differ by bg id, so it should not trip the
  guard.
- cargo fmt, cargo clippy and the harness-agent-native / harness-core suites
  were not rerun by me; the builder's logs in target/tmp/t1-*.log were read
  only for the red run.
- cargo deny (not available here; no dependency was added).
- web/ (nothing changed; fold.ts ignores the new field by reading, not by
  test).
- A real provider and the real binary.
- A run of the contained-timeout and delegate scenarios (R1 timeout part, R3 b
  and c).

## 7. After the re-check (the co-owner's closing)

Done on the branch (head `443f924` before the merge of `main`), tests first:

- **T1-D1.** A new result restarts the other calls' counts only when the
  call succeeded. `a_failing_run_whose_text_changes_restarts_nothing` failed
  with the person asked 8 times instead of 4. The opposite error is pinned by
  `a_failing_build_that_moves_after_each_edit_runs`: seven failing builds
  whose text moves, with a look and an edit between each, all run unwarned.
  Cost, in the T40 residual: a wait between runs of a failing check whose
  text changes is warned from its second run and refused at its fifth.
- **T1-D2.** The turn ends only on a refusal the model has been sent: each
  streak records the message of its first refusal, and a repeat in that same
  message is refused again without ending the turn.
  `a_second_ask_in_the_refusing_message_does_not_end_the_turn` failed with
  the turn ended after 3 requests.
- **Weak tests.** The harness's own volatile tokens are now constants that
  `bash.rs` formats with and the guard masks with, so they cannot drift
  apart; a failed delegation is tested as no change (checked by mutation);
  the wait-between-polls test asserts that all eight waits ran and which is
  warned; a call note is read through the summariser (checked by mutation).

Decided by the co-owner, recorded in the README, T40 and the plan row:

- One behaviour for chats and routines. The plan row said a warning only in
  chats and a hard stop only in routines; a fifth identical call with an
  identical result is as useless in a chat, and the person sees the refusal
  in the tool result. The owner may overrule.
- The thresholds (2 and 5) are constants; there is no key and no switch to
  turn the guard off.
- The warning is for the model only; whether the UI shows it is a later web
  task.

Open:

- A successful call whose result is new every time (a background start that
  gets a fresh id) still restarts the other calls' counts.
- No test compacts in the middle of a callback turn with a call note before
  its trailing notes; no contained timeout runs through the loop in the
  agent-native tests (they have no bubblewrap).
- No real provider was used. Whether a real model heeds the warning is not
  measured; the refusal does not depend on it.
