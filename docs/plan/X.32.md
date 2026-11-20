# X.32 **sec** (T78)

## Adds

The task board for agents (the owner, 2026-11-17: "what about the taskboard usability do ai agents able to interact with it?").
What existed: people have the whole board; an agent gets one line per turn with at most 8 open card titles of 80 characters, no notes, no ids and no tool (`crates/master/src/projects.rs` `board_line`). `docs/projects.md` planned a `board` tool and it was never built.

## Safeguards and abuse tests

**Done on branch `feat/board-tool`** (PLAN.md T78, README "Agents on the board", docs/projects.md "Task board").
Two tools, not one with an action: `board` (list; read-only) and `board_edit` (`add`, `move`, `note`; changes), because the approval path classes a call by its tool (plan mode refuses by name, Ask asks by risk, the read-only list is of names).
The node files `board.request` (`CAP_BOARD`) for its session as `recall` does; the master (`crates/master/src/board.rs`) takes the project, the owner and the permission (`projects::may_manage`, per request) from the session alone, refuses a node that does not run it, and answers with the board or the change.
No delete, no overwrite: notes are appended as `[agent <name>] …`, refused past 4,000.
Caps: notes 500 per card and 12,000 per list, 100 cards, 10 changes and 20 lists per turn, 4 calls per node.
A repeated request id changes nothing again.
Each change is an audit line `board.agent`; a card an agent made records the agent and links the chat (T10 for others).
Per-agent switch **Board**: on for Assistant, Software Developer, Senior Teamlead; off for the others and new agents; a read-only page agent gets the list only.
Not offered outside a project, in temporary chats, untrusted or callback turns.
The per-turn board line is unchanged.
Changes only in a turn the owner sent signed in or a run of the owner's own routine (the owner, 2026-11-18: "Routines may change"); an API-token turn only lists.
After review of PR 145: every line of a note carries the real agent's mark, names in the list are JSON strings, the rendered list stays below the tool output cap.
Skipped from Hermes (14 `kanban_*` tools, a dispatcher) and Cline (cards that run agents): claims, dependencies, cards that start work.
**S to M.**

