# X.59 **sec** (T99)

## Adds

A person looks at a Word or Excel file of a coding chat's folder in the page, the same way as in a chat without a folder (X.58; the owner, 2026-11-23: "do both please", and a coding chat is the basic chat with more abilities, so a person meets the same preview in both).
The made file's chip (`MadeChip`, which carries the file's path in a chat with a folder) and the sent message's chip of a file the person attached, which a coding chat saves into the folder (`user_message.uploads`, whose `path` is where the node saved it), have the same **Preview** control, the same panel, the same wording and the same failure sentences.
The panel has no Download link for a folder file: no existing route serves a folder's bytes to the owner, and none is added here.
In a chat without a folder the control sends the id of the server's copy; in a coding chat it sends the path.
The path goes to the node over one typed request, `session.doc_preview` (`req_id`, `session_id`, `path` as text of at most 1,024 bytes, `sheet` as a place), answered by one `session.doc_preview_result` (the preview as JSON of at most 512 KB, or one word of `PreviewRefusal`: `not_found`, `file_too_large`, `too_large`, `empty`, `not_office`, `protected`, `damaged`, `no_sheet`, `busy`, `not_available`).
The node announces the capability `doc-preview` where it can start its helper; the master asks no other node, and says "this chat's node cannot preview files; update the node" to an older one.
The node resolves the path as its `read` tool does (`ToolCtx::resolve`: the pinned folder, links followed and judged, protected places refused) but never outside the folder, whatever `allow_outside_workspace` says, and never inside `.git`; it opens only a regular file without blocking, reads at most 10 MB, and runs the same confined `doc-edit` helper with the same `Request::Preview`.
A node that denies `read` in its `node.toml` shows nothing.
A sandboxed and a host chat are read alike: the sandbox confines the agent's shell, not the node's own read of a file the person asked to see.
The request appends no event and logs no path.
A person's previews take at most one of the helper's two places on the node (one at a time on the node, one at a time per account on the master), so they cannot starve the agent's own `doc_view`.
`GET /api/sessions/{id}/folder-preview?path=&sheet=` (`crates/master/src/http/folder_preview.rs`, `crates/master/src/folder_preview.rs`) answers with the JSON and the sentences of X.58, which it shares (`doc_preview.rs`).
Claude Desktop and Codex show a folder's text files and diffs in a side panel and none previews an office file from the folder; this does.
Out of scope: editing from the page (the next row builds on this: it sends a list of text changes with the same target and gets a held copy; a sibling message and a sibling `ask` are all it adds), a download of a folder file, a Preview control in the changed-files or diff panel (it lists paths by turn, not files by name, and a control there is not a few lines).

## Safeguards and abuse tests

See [T99](../threats/T99.md).

## Podman / Node

No / Yes (the node reads and parses; the master only relays)

## Depends

X.58, X.57, T63

## Size

**M**
