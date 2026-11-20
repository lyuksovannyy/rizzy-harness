# X.24 **sec** (T72)

## Adds

Background sub-agents (research round 2, `synth/09-subagents-models-retry.md` "Delivery timing": Claude Code `run_in_background`, OpenCode `background`, Codex mailbox, DeepSeek, Hermes)

## Safeguards and abuse tests

**Done on branch `feat/background-sub-agents`** (README "Background sub-agents", `docs/background-commands.md` §14, PLAN.md T72): `delegate { …, background: true }` returns at once with the child's id and `bg-<n>`; the child is the same child session as a foreground one, held as an entry of the chat's background table, so `bash_output`/`bash_stop`, the side panel's Stop and Stop all, revocation, deletion, the node's stop and the undo refusal cover it with no new path.
Its end is recorded once by the session (`background_ended { reason: finished, child: {result, ok, usage} }`) at a step boundary or between turns, where a writer's folder is merged back three ways (the foreground's merge and report), and calls back by PR 116's rules.
Bounds: the agent's own switch (`background_subagents`, Agents page "Background sub-agents", on only for the built-in Senior Teamlead), `[limits] background_agents_per_chat` (4; 0 off), `background_agent_timeout_s` (30 min), the node's step cap, a token cap that is its share of the turn's sub-agent budget, reserved at its start (the caps a turn grants never exceed the budget), one of the turn's `max_subagents`, depth one, no callback turn, routine chat or `host` chat.
A restart records `node_restarted` once (no callback) and the child can be given a follow-up.
Unlike Claude Code, the lead does not read the child's live output (its own chat has it); unlike Codex, no `wait_agent`; unlike OpenCode, a running foreground child cannot be moved into the background.
Tests: `cli/tests/background_agents.rs` (bubblewrap: returns at once and calls back once, rebuilt byte for byte; a busy lead reads it at a step boundary once; two at once into one callback; the node's cap and the spec that cannot raise it; switch off and key 0: not offered, a forged one refused; Stop, `bash_stop` and delete leave no process or folder; merge at delivery and a conflict; plan mode; a follow-up while it runs; the real binary: the master's callback, `kill -9` mid-child then a follow-up), `tools/src/background.rs`, `node/src/policy.rs`, `core/src/event.rs`, `agents/tests/agents.rs`, the fold fixture, `web/src/components/sideItems.test.ts`.

## Invariants

The node decides; one writer per session; chats private to their owner; nothing the model writes is trusted (the report quoted)

## Podman / Node

No / Yes

## Depends

C.15, X.22

## Size

**M**

