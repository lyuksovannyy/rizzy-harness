# X.61 **sec** (T53)

## Adds

All of a coding chat's changes in one diff (the owner's direction, 2026-11-14: usable for coding first; Claude Desktop and Codex show a session's total change, and a change made over several turns could only be reviewed turn by turn).
A header button "All changes" opens the same changes panel as a turn's "Show changes", with the change from the start of the chat to now; its title says "All changes in this chat", or "Changes since the oldest turn still kept" when the oldest checkpoints were pruned.
Undo, rewind and the later-turns switch are not shown: they act on a turn.
The request is the existing `session.diff` with an additive `all` flag, behind the capability `diff-all` that the master checks (a node without it is told to update, with a 409); the answer is the same `session.diff_result` and carries `since_pruned` when the range starts later than the chat.
The route is `GET /api/sessions/{id}/diff` (`crates/master/src/http/chat_diff.rs`), owner only, 404 for a plain chat (no folder); a temporary coding chat takes checkpoints and is served like any other.
No event kind, no schema change, nothing stored.
The range rule (node, `session_diff.rs`): from the oldest snapshot taken before work (a turn's `before`, a restore's `before_restore`) that is still kept, found by bisection since pruning drops the oldest first, to the latest recorded state that is not one (a turn's `after`, a restore's `after`).
Undone and rewound work drops out by itself, because a restore's `after` is the latest state.
A turn still running has a `before` and no `after`: it is left out until it ends (the panel says so), not refused as busy, so the review can start while the agent works.
A chat with no finished turn, or whose turns changed no file, answers an empty diff ("No file changes in this chat."); a chat whose node keeps no checkpoints (`checkpoints_keep = 0`, a sub-agent's own copy) answers a sentence.
A fork's log holds its parent's checkpoint events and shares its folder, so its range holds the inherited turns too; a sub-agent's chat that shares its parent's folder is read like any other.
Known, not fixed, and said in the panel under its title ("Everything that changed in this chat's folder since it started, including changes made by hand or by other chats that share the folder", with "the oldest turn still kept" when pruned): the range is two snapshots of a folder that other chats can share (a fork's parent after the cut, sibling chats, shared-folder sub-agents) and that the person edits, so all of that is inside it; the others-wrote notes of a turn's undo are not computed for the range.
A start is kept only when it comes before the end in the log, so a range is never inverted.
The panel reads the range again when a running turn ends.
The header button shows "All changes" with its icon, the icon alone under 600 px.
Not covered: the diff against the repository's `HEAD` or a branch point (this is the chat's range, not a pull request's), and the others-wrote notes of a turn's undo (nothing here is undone).

## Safeguards and abuse tests

See [T53](../threats/T53.md).

## Podman / Node

No / Yes (the node reads its checkpoints; the master only relays)

## Depends

X.12, X.59

## Size

**M**
