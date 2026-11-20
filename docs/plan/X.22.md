# X.22

## Adds

Follow-ups to a sub-agent it already ran (research gap list: "continue a child: delegate `session` argument (M; read-only children first)"; round 1 "Resume a sub-agent": 11 of 17 harnesses; X.21 is reserved for another builder)

## Safeguards and abuse tests

**Done on branch `feat/continue-child`** (PLAN.md T68): `delegate` takes an optional `session` (the id a finished result began with); the node checks from its own log that the child is the caller's own and was started as the agent named, then the same child session gets a new turn (`turn_started.continued_by`) and continues its own conversation.
A child keeps its conversation, not its files: a writer gets a fresh copy of the folder as it is now at its own path, merged back three ways as before; a reader works in the folder.
Its opening says so in a fixed harness line.
Same caps as a new child (per turn, token budget, depth through the bounded delegates), at most 8 runs, one turn at a time.
As OpenCode (`sessionID`, parent check, `subagent.ts`), goose (`delegate(resume: id)` with an ownership check, `summon.rs` L886-L925), Claude Code (`SendMessage` to the agent id; it keeps its tool set and cache) and Codex (`send_input`, "reuse the agent ... if your task is highly dependent on the context of a previous task"); unlike OpenCode, a follow-up never switches the child's agent; unlike Claude Code's, it is a foreground call that returns the child's result.

## Invariants

`node/tests/continue_child.rs`, the agent-native wording test, the Team Lead prompt test, `web/src/components/sideItems.test.ts`, the shared fold fixture.
**M**

