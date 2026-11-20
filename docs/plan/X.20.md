# X.20

## Adds

Manual compact with focus text (round 1 "Manual compaction": 14 of 16 harnesses have it, we did not; Claude Code `/compact [instructions]`, Hermes `/compress [focus]`, Codex and Cline `/compact`)

## Safeguards and abuse tests

**Done on branch `feat/parallel-reads-and-compact`** (PLAN.md T64, README "Summarise now"): `/compact [focus]` in a chat's composer is a command, not a prompt: `POST /api/sessions/{id}/compact` (owner only, the checks of a prompt) relays `session.compact` to the node, which runs the existing summariser (prompt, shrink check, breaker of A.7) between turns as an operation of the session's one writer, keeping the last turn verbatim (the automatic summary keeps two; Claude Code replaces the whole history), the task list and standing context; the focus is the person's words quoted under a `Focus:` label (T38), cut at 2,000 characters (Claude Code cuts hook instructions at 10,000).
Refused while a turn runs and for a chat too short (no event); no turn starts while it runs; recorded as `history_summarized` (`manual`, `focus`, `usage`, `tokens_after`: the meter drops) or `compact_failed` (plain reason); two failures since the last turn refuse the next.
After the review: refused while a queued message waits to be read, and one that ends with messages waiting starts the turn that reads them (M1); its usage merged by model in both folds (M2); `/compact` reserved against a skill of that name (M3);
Stop shows while it runs; the focus is the command's own line.
Tests: route owner check and refusals (`master/tests/it/compact_http.rs`), node (`node/tests/compact.rs`: the summary, the focus quoted and cut, the next request, refused while a turn runs, too short, no turn meanwhile, failure and breaker, Stop), `agent-native/src/manual_compact.rs`, fold fixture, composer.
Not done: a partial compaction ("summarise from here", Claude Code's `/rewind`), a summary queued for a running turn's end, a per-project "what to keep" file read by every summary (Claude Code's "Compact Instructions").

## Invariants

One writer per session; chats private to their owner; nothing the model writes is trusted (focus quoted)

## Podman / Node

No / Yes

## Depends

A.7, X.15

## Size

**M**

