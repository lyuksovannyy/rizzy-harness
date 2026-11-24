# X.62 **sec** (T36)

## Adds

See what a background command is printing without asking the agent to read it (the owner's direction, 2026-11-14: usable for coding first; Claude Code and Claude Desktop show a background task's output on demand, Codex shows the terminal, and a dev server's log or a long test run cost a turn here).
Each background command's row in the side panel gets an Output button, closed by default; open, it shows the tail of the output (read-only, monospace, following the end), with the command's state and how long it ran and printed, and looks again every 3 seconds while the command runs and once when it ended, never when closed, hidden or the node is away.
It is not an event: the log carries a command's output only at its end (4 KiB), so a running command's output exists only in the node's ring that `bash_output` reads, and that ring is the only source; no second buffer, no new file, nothing stored on the master.
One typed request over the link, `session.background_output`, answered by `session.background_output_result`, only to a node that announced `bg-output` (additive protocol, no event kind, no schema change); the route is `GET /api/sessions/{id}/background/{bg_id}/output` (`crates/master/src/http/bg_output.rs`), owner only.
The answer is the last 28,000 bytes (the agent tool's own cap less its note) as plain text, valid UTF-8, terminal escapes removed, with `cut` for "earlier output not shown", and `dropped` for output the node gave up.
The text goes through the node's scrubber where it enters the ring, as the events' does; the page shows it as text with hidden and control characters made visible.
A forgotten command (30 minutes after its end), a node that restarted, another chat's id, a sub-agent and a node too old are each a sentence, never a 500.
Not covered: the output of an ended command from an earlier turn (the strip lists a finished command only for the latest turn; the chat's "ended" line keeps its last 4 KiB), a live stream (it looks every 3 s; a push would need a WebSocket message and a per-session subscription), and what a foreground command prints (it is a block in the conversation).

## Safeguards and abuse tests

See [T36](../threats/T36.md).

## Podman / Node

No / Yes (the node reads its own ring; the master only relays)

## Depends

T.8d, X.24

## Size

**S**
