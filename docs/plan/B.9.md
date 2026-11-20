# B.9

## Experiment

Not settled: where instruction files enter the prompt and whether they survive compaction

## Outcome that changes a decision

Lost after compaction: re-injection joins X.2. Repository instruction files: done there (per-turn context, rebuilt each turn).
A summary inside a turn (X.15) keeps the turn's opening, with its context block, verbatim.
The node operator's file (2026-11-15, T62) is a standing section like them and survives compaction the same way; a file mentioned with `@path` (X.17) lives in the turn's `user_message`, so an in-turn summary keeps it with the opening and a history summary summarises it like any message.
**Nested instruction files: done** (2026-11-15, PLAN.md T65): the `AGENTS.md` (else `CLAUDE.md`) of each folder between a file a successful `read`, `edit` or `write` touched (or a file the message mentions) and the chat's folder, loaded the first time, as Claude Code, OpenCode and DeepSeek do (research/round1/claude-code.md 1266-1296, research/round2/opencode.agents.md 96-104, research/round2/deepseek-harness.agents.md 178-190); sent after that step's results as a block of its own (`context_sent` frame `step`), appended, so the cache holds; one standing section per file, read again every turn, replaced or withdrawn in the turn's opening (a replacement rewrites the request from the old step block on, once per change, as a root file's does); kept by clearing and in-turn summaries; dropped by a history summary and loaded again on the next touch (what all three do after compaction).
Caps 16 KiB a file, 32 KiB and 16 files together, a tenth of a small window; over a cap named, not loaded.
A file that changed during the turn (by the agent's `write`/`edit`, or with a change or modification time after the turn began, 50 ms of slack) waits for a later turn, which may be a callback turn; a sub-agent takes nothing changed since its parent's turn began.

## Size

**S**

