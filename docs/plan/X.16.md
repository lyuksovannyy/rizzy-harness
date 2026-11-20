# X.16

## Adds

Hand-off notes and one small model (the owner, 2026-11-14: "allow to select whatever response summary is done by other cheap model or by the sub-model it self"; usability first, context across sessions)

## Safeguards and abuse tests

**Done on branch `feat/handoff-note`** (`docs/handoff-notes.md`, PLAN.md T60): a coding chat in a project idle 10 minutes with new file changes or task-list changes gets a hand-off note, an event in its own log (`handoff_written`) written by its node in one bounded request (input at most 16,000 tokens, note at most 1,500 characters, no tools, no retry); the next chat in the same project reads it on its first message under the facts built from the log (first ask, files changed, task list, how the last turn ended), quoted, in place of the last reply.
One small-model setting (`[savings] small_model`), two switches: "Hand-off notes: off / this chat's model / the small model" (default: the small model when set, else the chat's) and "Read-only sub-agents use the small model" (default off; read-only when every allowed tool is on an allow-list of tools that only read).
Fallback: small model, then the chat's own, then the facts alone, named in the event.
Tests: fixed part, due-once, fallbacks, never-for, a message during the write, restart, injection, secret, the next project chat's request, the sub-agent on the small model, fold and UI.
Node bound: `[limits] handoff_notes_per_hour` (4; 0 = off).
Not done: notes for chats outside a project (nothing reads them there yet), a "write the note now" button, notes in `recall`.

## Invariants

One writer per session; node decides; master runs no model

## Podman / Node

No / Yes

## Depends

X.1, X.13, X.15

## Size

**M**

