# X.8

## Adds

Loop staying power (the owner, 2026-11-14: "all useful things other harnesses ship")

## Safeguards and abuse tests

**Done on branch `feat/loop-staying-power`, with the review's fixes:** (1) a person's turn, after its `max_steps` tool steps, gets one more request: tools still listed (prompt cache), `tool_choice: none`, the harness's note asking what is done and what is left, any call that arrives anyway answered "not run", queued messages left for the next turn; it ends `turn_ended { reason: limit }`, not as an error; a note at half the budget; both notes ride on the step's last `tool_call_finished.note` so later requests rebuild them (OpenCode's max-steps step, goose's turn budget; `01` OpenCode and goose "Limits"); callback turns and sub-agents keep their own ends; (2) `[limits] model_first_byte_timeout_s` (600 s) and `model_idle_timeout_s` (300 s, Codex): genai's header timeout plus the loop's own wait per stream event; a silent provider is `ProviderError::Silent`, retried at most once and never after visible text, then a fallback; (3) a call with no recorded result reads "may have run fully, partly or not at all; check before repeating" (DeepSeek, OpenClaw; `01` §4.6); (4) the node's environment line each agentic turn (clock with zone, OS, shell, git branch from `HEAD` without a process; Codex, OpenCode, OpenClaw, goose; `07` §2, §4.1), recorded on `turn_started.environment` and rebuilt byte for byte (PLAN.md T48).

## Invariants

`agent-native/tests/staying_power.rs` (incl. the next turn's request keeping this turn's bytes), `agent-native/tests/loop_limits.rs`, `node/src/environment.rs`, `node/tests/queued.rs`, `node/tests/repo_instructions.rs`, the callback test in `agent-native/src/callback_tests.rs`.
Not done: the default of 50 (the owner's), a wrap-up for callback turns; the per-turn context, instruction files, memory and skills catalogue are logged since X.1 (`context_sent`).

## Podman / Node

One writer; node decides

## Depends

No / No

## Size


## Column 8

**S**

