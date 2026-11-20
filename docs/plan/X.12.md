# X.12 **sec** (done, T53)

## Adds

A turn's changes as a diff (round 1 ranks it our biggest UI gap: `08` "Diff presentation" and §6 item 2, `05` "Diff views"; Codex Desktop, Claude Desktop, Antigravity, Zed, Warp, Cline and OpenHands all show one).
**Done on branch `feat/turn-diff`:** the turn's "N files changed" row has "Show changes", which opens the diff beside the chat (right side, as Claude Desktop opens a turn's diff from its Changes card; over the chat on a narrow screen): a file list, then per file its hunks with old and new line numbers, `+`/`−` signs and words for screen readers, hidden characters as codes (the approval card's marks), binary and oversized files named with their sizes, cut markers;
"Include later turns" shows what a rewind would undo; Undo from the panel too.
Computed on the node from the turn's two checkpoints by the confined git (`git::turn_diff`), one `session.diff` / `session.diff_result` over the link, nothing logged or cached.
Follow-ups, not built: per-hunk accept or reject, comments on lines sent to the agent (Claude Desktop, Codex, Warp, Antigravity), syntax highlighting, a diff of the whole chat against the folder's start, a sub-agent's folder before merge-back.

## Safeguards and abuse tests

Owner only (404 for admins, T10); confined git with literal pathspecs, ids not header paths; caps per file and per answer, 20 s, two per node; secrets scrubbed.
Tests: node (edit, create, delete, rename, shell change, caps and cut, binary, large, hostile names, link out, nested repos, secrets, lost and pruned checkpoints, rewind), master (owner only, away, too old, refusal, bad id), web (marks beyond colour, hidden characters, cut, binary, refusal, Undo, focus), `web/e2e/turn-diff.cjs`.

## Invariants

No event, no secret in a log; the node decides (it reads only its own checkpoints)

## Podman / Node

Node

## Depends

Checkpoints (A.9, A.17)

## Size

M

