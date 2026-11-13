# The folder pin and the sub-agent's folder did not reach the tools

Date: 2026-11-13. Found by the builder of task T.8d while reading the agent loop; confirmed in
the code on `main` at `8f07d78`; fixed in `d11b679`. This record corrects what earlier reviews
of R.1 state.

## What was wrong

The agent loop wraps its host in `Logged` (`crates/agent-native/src/lib.rs`, added in
`d16b9a7`) so that a failed append stops the turn. `TurnHost` later gained two methods with a
default body that answers `None`: `folder_pin` (`1220e1a`, finding S-2 of R.1) and
`owned_folder` (`f83a87a`, findings R1-1 to R1-3). `Logged` forwarded neither. The loop builds
every tool call's context from the wrapper, so in production every tool ran with
`pin: None, owned: None`.

## What was untrue, from `1220e1a` and `f83a87a` until `d11b679`

- **The per-call folder-pin check (S-2; `PLAN.md` T4) held only between turns.** The session
  checks its folder when a turn starts (`Session::checked_folder`), and that check did run. A
  folder replaced during a turn was read, written and run in by the tool calls that followed.
- **A sub-agent that changes files could not use its own folder (`PLAN.md` T32).** Its `write`
  was refused as "inside the harness's own data directory" and its shell as "would expose the
  harness's data directory". Its delegation came back with "[no file changes]". This failed
  closed: nothing was written where it should not be.
- **`2026-11-gate3-fixes-5.md`** says the test
  `the_wrapper_refuses_a_grant_that_was_replaced_and_names_it` covers a session folder swapped
  after the pin check. That was true of the sandbox wrapper only: in production no pin reached
  it, and the grant was built from the folder as it was at the call.

## Why the tests did not see it

The R.1 tests for both properties built a tool's context by hand, or replaced the folder
before the turn started. None ran a tool through `run_turn` with a host that has a pin.

## The fix and its tests

`Logged` forwards both methods. Four tests, each failing before the fix:

- `agent-native/tests/host_forwarding.rs`: a host's pin and owned folder arrive in a tool's
  context through `run_turn`; a folder replaced during a turn (by a link, by a new folder)
  refuses `read`, `write` and `bash`. Before: "ran in the replaced folder: 1 SSH-KEY-CANARY".
- `node/tests/runtime.rs` `a_folder_replaced_during_a_turn_refuses_the_next_tool_calls`: the
  same through a real session.
- `node/tests/runtime.rs` `a_writing_sub_agent_works_in_its_own_folder_and_nowhere_else` and
  `cli/tests/sub_agent_sandbox.rs` `a_delegated_sub_agent_gets_its_folder_through_the_node`
  (inside bubblewrap): the child's `write` and shell work in its own folder, its changes are
  merged back, and its parent's folder, a sibling's folder, the node's database and key, and
  `~/.ssh` are refused or absent.

## What is left

- The two methods keep their `None` default. `Logged` is the only wrapper of a `TurnHost` in
  the workspace; a test now fails when a wrapper inside `run_turn` drops them. A second wrapper
  outside the loop would not be caught: making both methods required is the stronger rule and
  is the change to make the day such a wrapper appears.
- A sub-agent's file-changing path ran in production for the first time with this fix. Its
  tests pass; it has had no review of its own under real use.
