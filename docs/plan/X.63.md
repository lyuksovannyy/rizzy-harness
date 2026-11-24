# X.63 **sec** (T99)

## Adds

A person reads a text file of a coding chat's folder as it is now, beside the chat, read-only (the owner's direction, 2026-11-14: usable for coding first; Claude Desktop and Codex open a file in a side panel, and here the diff showed what changed but no way to read `src/main.rs` whole).
An **Open** control sits where the page already holds an exact path: a changed file's row in the turn's and the whole chat's diff (not a deleted file), the head of a finished `read`, `write` or `edit` card (the call's own `path` input, never a path in the model's prose), and a made file's chip in a chat with a folder.
A Word or Excel name goes to its Preview instead (X.59); every other name opens the text.
A diff row's path is the repository's, and the chat's folder may be a subfolder of it: the diff answer carries the folder's place in the repository (`TurnDiff.folder`, additive, not sent when empty), the page strips it for Open and a row outside the folder has no control; the row still shows the repository's path.
A CRLF file shows one note, "Windows line endings (CRLF)", and no code at each line's end.
The panel is the existing side-panel slot: opening it replaces the diff or the Word preview, and a Word preview with unsaved changes asks first (`gate`).
It shows the path, the size, the line count, the time it was read and a Reload control; line numbers; no wrap (a long line scrolls); the chat's own highlighter (a worker, with the code blocks' size bounds) when the name has a grammar, else plain text.
Hidden, control and direction characters are made visible as codes before anything else is done with the text.
One typed request over the link, `session.folder_file` (`req_id`, `session_id`, `path` as text of at most 1,024 bytes), answered by one `session.folder_file_result` (the text, its size, `truncated`, `lossy`, or one word of `FileRefusal`: `not_found`, `binary`, `denied`, `busy`), only to a node that announced `folder-file`; additive, no event kind, no schema change.
The node finds the path exactly as X.59 does (`folder_preview::locate`, shared), opens a regular file without blocking, reads at most 256 KiB plus a 4 KiB margin and one byte (never the whole of a huge file; whether it was cut is told by what was read; a NUL in the first 8,000 bytes is "binary"), decodes UTF-8 (lossy, said) and passes the text through the node's secret scrubber, the margin included, showing only what the scrubbing of the whole window and of the window without its last margin agree on, cut at 256 KiB on a character boundary: the same scrubber an event's text passes, and a secret that crosses the end of the read cannot show in part.
256 KiB is about six thousand lines of code, and even a file of control characters (six bytes each as JSON) stays far under the 8 MiB frame.
A node that denies `read` answers `denied`; one place on the node, kept apart from the document helper's.
`GET /api/sessions/{id}/folder-file?path=` (`crates/master/src/http/folder_file.rs`, waiting in `crates/master/src/folder_preview.rs`, ten seconds): the checks of the folder preview, in its order.
Not covered: editing, a syntax highlight for a file past the chat's code-block bounds (20,000 characters), tracking a file that changes while open (Reload is by hand), and a fixed place on the node per account.

## Safeguards and abuse tests

See [T99](../threats/T99.md).

## Podman / Node

No / Yes (the node reads; the master only relays)

## Depends

X.59, X.61

## Size

**M**
