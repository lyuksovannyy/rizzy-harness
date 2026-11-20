# X.19

## Adds

Read-only tool calls of one step run at the same time (round 1 gap "A1 Parallel tool calls": Claude Code, Codex, Pi, Oh My Pi, OpenCode, OpenClaw, DeepSeek, Hermes and Cline run them, research round 2 §3.5; goose and we ran every call in order)

## Safeguards and abuse tests

**Done on branch `feat/parallel-reads-and-compact`** (PLAN.md §2 "Event order of a parallel step", T40): a run of neighbouring calls whose tool declares itself read-only **and** parallel safe (`Tool::parallel_safe`, default false: `read`, `grep`, `glob`, `outline`; never `bash`, web tools, `skill`, `todo`, `remember`/`recall`, `delegate`, MCP tools), with valid arguments and no card on this node (`runs_unasked`), is one batch; any other call runs alone in its place.
Starts, guard admissions, early refusals and approvals in call order (Pi's and Cline's serial preparation), then at most 8 at once (Hermes 8, DeepSeek 10), no live output, results logged and sent in call order.
The repeated-call guard sees each call once in order and keeps a batch inside its margin (one call key per batch; `RepeatGuard::batchable`).
Stop: every call still gets its result.
A panicking tool fails its own call.
Tests: `agent-native/src/parallel_tests.rs` (four reads in one delay, call 3 finishing first, a change between reads, the cap, Stop, a panic, a failure, a denied tool, `ask_all`, the guard's two rules, a sub-agent and a callback turn) and a parallel case of the byte-for-byte rebuild property test (`tests/turn_compaction_review.rs`).
Not done: MCP tools declared read-only by their server (`readOnlyHint`, Claude Code's rule; the server's word is untrusted here), `bash` commands judged read-only (Codex's shell is parallel; its lesson is that a command can write), web tools (their citation labels are numbered in call order), starting calls while the answer still streams (Codex, OpenCode).

## Invariants

One writer per session (event order fixed by call order, not finish order); node decides (`runs_unasked`)

## Podman / Node

No / Yes

## Depends

T40, X.15

## Size

**S**

