# X.1

## Adds

Context-change events ((c) step 6)

## Safeguards and abuse tests

**Prompt sections: done** (branch `feat/context-log`, 2026-11-14; PLAN.md T52, README "What goes with each message"): the harness's context block of a turn (the repository's instruction files, the list of skills, the master's memory and project parts as standing sections keyed and hashed; the master's settings line, a `/skill` line and MCP notes as this turn's) is recorded as sent on `context_sent` by the loop through the session's one writer and put back by the rebuild, so turn N+1's request holds turn N's last request byte for byte (measured with a recording provider: all 6,733 bytes of turn 1's last request against 1,501 before, the system prompt only; a 4 KiB `AGENTS.md` no longer re-sent: 4,403 bytes a message).
A standing section is sent only when its digest differs from the last one since the summary, then in full with a line saying it replaces the earlier one; a gone one gets a withdrawal line once; after a summary (also one mid-turn) everything is sent again.
Codex's world-state diff (replacement line, removal notice, everything in a fresh window) and OpenCode's frozen update text (`07` §2).
The master's context is split on the wire (`MasterContext`: this message's line, and standing parts keyed `memory`, `workspace`, `remember`, `project`, `project_memory`, `board`; recall past a cap, the saved-memory counter, notes on earlier calls and first-turn earlier chats go with one message only: review R4).
The history holds only the current version of each standing section (review R1, R2).
Callback turns, sub-agents (each its own log), parallel answers and a turn that only reads queued messages are covered; the summariser's transcript leaves the blocks out.
Was: a prompt-section delta event and a tool-loadout event, folded by node and UI; sorted names; appended, never edited into the prompt head. `10` §6.3 item 6: after restart, fork and compaction the loaded set and request prefix are identical when nothing changed.
**Left:** the tool-loadout event (needed with MCP deferral, X.3); fork (none exists yet); the master's settings line changes every minute and goes every turn (moving its clock to the node's environment line would let it dedupe).

## Invariants

One writer

## Podman / Node

No / small

## Depends

A.7, PR 7

## Size

**M**

