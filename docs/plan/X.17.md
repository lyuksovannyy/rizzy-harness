# X.17 **sec** (done, T63)

## Adds

`@` file mentions in the composer (the owner, 2026-11-15: the useful things the other harnesses ship)

## Safeguards and abuse tests

**Done** (2026-11-15): `@path` in an agentic chat puts that file of the chat's folder into the message as a quoted, bounded excerpt with its path and line count, without a tool call, as Claude Code, OpenCode and goose do (`research/round1/claude-code.md` 546-550: inlined up to the Read limit, text over 256 KB left out, a folder as a listing, Read deny rules hold; `research/round1/synth/01-loop-tools-editing-shell.md` 665-680; `research/round1/goose.md` 181).
The node reads each path under the `read` tool's rules (T63) and records it in `user_message.files`; 20 files and 64 KiB shared per message, 256 KiB per file, binaries named, folders listed (200), counts as read for `edit` when shown; completion from the chat's folder through `session.files` (bounded walk on the node, nothing on the master); `@agent` stays an agent, a token with `/` or `.` a path.
README "Mentioning a file".
Later: line ranges (`@file#5-10`, Claude Code's VS Code form), the home bar's new-chat composer (no chat folder yet to list), nested `AGENTS.md` of a mentioned file's folders (Claude Code adds them; planned with nested instruction files).

## Invariants

Node decides; nothing the model writes is trusted

## Podman / Node

No / Yes

## Depends

X.2

## Size

**M**

