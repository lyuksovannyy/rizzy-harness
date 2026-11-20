# X.36 **sec**

## Adds

A repository's own MCP servers (the owner, 2026-11-17: "mcps that live in project like in mcp.json and others are not considered in local/global contexts, threat those more as repo/project mcp + maybe its better to have a bit modified version of mcp.json to include as well the install command(s)").
What exists: `.mcp.json` is ignored on purpose (T47; `crates/node/tests/mcp.rs` plants one).

## Safeguards and abuse tests

**Done (2026-11-18, `feat/project-mcp`; PLAN.md T82, README "A repository's own servers (`.mcp.json`)", docs/projects.md).**
Built: node.toml `[mcp] project_servers = "off" | "ask"` (default off: nothing reads the file, `project-mcp` is not announced); `project.mcp` / `project.mcp_result` (List, Approve with the shown entry's digest, Remove), sent only on a person's HTTP call (`GET /api/projects/{id}/mcp`, `POST …/mcp/approve`, `POST …/mcp/remove`) for a member who may run agent chats on the project's node; the node's copies in `<data>/project-mcp.json` (0600), keyed by account, the folder's real path and the name; the chat's owner carried on each turn command (`owner`, set by the master from the chat's row) and kept in the node's memory; each turn's table takes its owner's approved copies (in a `host` chat too since 2026-11-20, as the node's user without the sandbox and without an entry that has `env` values; the owner: "if user selected host project workspace, why do we need to limit it? its his choice"), less names the node's last read did not find; the file is read on a person's request and once per chat at its first turn since the node started, never per turn; only an entry shown whole and unchanged can be approved (512 characters per string, 4 KiB per entry, no line break, control, hidden or direction character, no folded spaces), its `env` values shown too; those values reach only the server's own process (a descriptor the guard hands to bubblewrap, set by `sandbox-exec` after the confinement), join no scrubber, and the server's environment is otherwise the core list only; the project page's Settings tab lists, approves, removes and reloads.
As planned: Read the standard `.mcp.json`; off by default (`node.toml`); a person approves each server, pinned to its exact command, arguments and environment; started inside the chat's sandbox like its shell and refused in a `host` chat at first; no file of our own with install commands in the first version (`npx` and `uvx` install on start).
**Changes to the file (the owner, 2026-11-17: "add as well mcp file watcher which will register file changes and will reload if approved, otherwise uses old variant"):** the node keeps its own copy of each approved entry in its data directory, outside every folder an agent can write; a changed or new entry is shown for approval and, until the yes, the approved copy keeps running (a new entry does not run); a removed entry stops at the next turn without a question, since removing narrows.
How a change is taken up (the owner, 2026-11-17, replacing the watcher: "its might be better to leave a manual reload button instead"): a **Reload** button on the project's list of servers; only that click makes the node read the file again and put changed or new entries up for approval.
Nothing is watched and nothing is checked per turn, so no edit to the file, an agent's included, starts anything by itself.
Opening the list reads the file once to say "the file differs from what is approved", so a forgotten reload is visible.
Why not a file the node obeys: anyone who can write to the repository, the agent included, would run a program on the node.
Left over, to be named in the threat row: the approval pins the command, not the files that command runs, so an approved `./tools/server.py` can be rewritten without a question; it then has the power of the chat's own shell and no more.
Abuse tests: an agent edits the file and every later turn still runs the approved copy until a person reloads and approves; a changed entry never starts before a yes; no protocol message or tool can press Reload; the approved copy cannot be written from the chat's file view.
**M.**

