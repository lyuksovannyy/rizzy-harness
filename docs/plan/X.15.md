# X.15

## Adds

Shortening context inside a turn (first live test with real models, 2026-11-15: the summary came too late and freed too little on a small-window model; T56's 1,000 steps outgrow any window in one turn)

## Safeguards and abuse tests

**Done on branch `feat/turn-compaction`** (`docs/turn-compaction.md`, T59): every request is checked before it is sent against the window less its reserve; past it the oldest tool results become a one-line stub (tool, target, size, how to get it back; no model call) down to half the line, never in the newest steps (Hermes' tail, `07` Hermes; OpenClaw's recorded hard clear, `07` OpenClaw); when that leaves less than a quarter of the line, earlier turns are summarised, then the turn's steps before the tail (Pi's split turn, `07` Pi), with the opening, the person's messages during the turn and the task list kept verbatim.
Recorded as `context_shortened`; the guards (T40, T45) checked; sub-agents included.
With a known window it replaces the count-based mask (`keep_tool_outputs`), which made 36 rewrites and no cut in a 300-step turn on 128k; with the defaults the same turn now makes 7. After the independent review: the summary is a message of its own after the opening, a summary of earlier turns never takes what the running turn's opening reads, and the provider's count is capped at twice the estimate.
Also fixes two kept turns that are alone over the line (summarised instead of kept whole) and a turn larger than the summariser's window (read with shorter results).

## Invariants

Not done: provider-side clearing (Anthropic `context_management`, OpenClaw's direct-API path), spilling a cleared result to a retrievable file (Cline, DeepSeek), re-attaching recently read files after a summary (Claude Code).

