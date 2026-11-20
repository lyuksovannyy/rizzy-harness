# X.33

## Adds

Workspace agents (the owner, 2026-11-17: "at the end everything must be workspace isolated (excluding user facts, their settings), so while built-in stuff like agents, mcps and skills are server-wide, workspace admin able to add own custom mcp and skills as well as agents").
What exists: agent edits and custom agents are server-wide and for server admins only; the design is `docs/agent-pages.md` "Workspace agents: what is left" (a `workspace_agents` table, a set composed per workspace: base, server edits, workspace edits).

## Safeguards and abuse tests

**Done** (PR on `feat/workspace-agents`).
A table `workspace_agents` (cascade with the workspace; at most 30 own agents and 64 KiB per agent), the set composed per workspace (`AgentSet::with_layers`: base → server edits → the workspace's) and cached per workspace, every resolver built from the chat's own workspace's set; writes name their scope (`?scope=server` for server admins, `?workspace=` for `settings` holders, Personal included);
Workspace settings → Agents makes and edits them, with each agent's origin.
T27 holds per workspace; a server agent wins over a workspace agent with its id (as a file does), and no workspace agent may take a server agent's name; abuse cases and what stops them: PLAN.md T79.
Accepted by the owner (2026-11-18, "Allow it"): workspace agents may combine chat search with commands, MCP, web or edits.
The composer's picker, its @mention list and the routine editor's agent list mark them "workspace agent" (2026-11-19).
Left: chosen looks for workspace agents.
**L.**

