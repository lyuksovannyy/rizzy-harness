# C.15

## Decision

Background commands (owner's direction, 2026-11-12; task T.8d).
**Note: `docs/background-commands.md`** (2026-11-12, proposal, nothing decided): §1 what exists, §2 the stress test and verdict, §3 the recommendations, §4 the tool (`background`, `bash_output`, `bash_stop`), timeouts, the per-session table, the end and the callback, `[limits]` keys, unattended turns, lifecycle, events and protocol, sub-agents and routines, the UI's cost, §5 security and the draft threat row T36, §6 the other harnesses, §7 abuse tests, §8 build order, §10 six questions for the owner **Answered by the owner on 2026-11-13 (the note's §0): Stop leaves background commands running and each can be stopped on its own; callback turns run in `auto` chats within their bounds; a shared range and clamp for both kinds (5 s to `bash_timeout_max_s`), 60 s for a foreground command that states none, and a background command refused without `timeout_secs` (Q5, decided later the same day); on by default with 8 per session and 64 per node.**

## What the note settles

`bash` gains `background: true`; the command stays in its own contained sandbox under its guard, held by the session instead of the call, and must state its timeout (foreground calls default to 60 s), never above `bash_timeout_max_s`.
Its end is recorded once by the session actor; a running turn sees it at its next step; an idle session is called back by the master after a routine's admission checks, at most `callback_turns` in a row without a person's message and never in an untrusted chat.
Stopped at the session's deletion, the owner's loss of access and the node's end, each with an event; refused for sub-agents and on nodes without containment.

## Tests it commits to

The note's §7 and §8: the T.8a escapers gone 2 s after a timeout, a stop, deletion, revocation and `kill -9` of the node; the ceiling and the counts hold and nothing is evicted; a callback once, in the owning session, never past the budget, never into a deleted, revoked or untrusted chat; a restart records `node_restarted` and starts no turn; no secret in `background_ended`.

