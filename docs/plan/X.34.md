# X.34 **sec**

## Adds

Plugins: a skills store, and which agents may use which skill or connector (the owner, 2026-11-17: "we must have sort of skills storage, where we define skill, its context (global, local) and if local we can assign an skill to agent. skills must be used as well only when required"; "global and local skills/mcp contexts are apply to workspace, not server wide"; "threat more like as Claude's 'Plugins'"; asked for better names than global/local or public/private).
What exists: skills come only from the node's disk and every agent with the `skill` tool gets all of them;
MCP servers come only from `node.toml` and the per-agent MCP switch means every tool of every server; sub-agents never get MCP tools.

## Safeguards and abuse tests

**Done (2026-11-18), in three steps.**
A: each agent's **Skills** switch (its own; on where `skill` was) and the MCP switch shown as **Connectors**; nodes announce their own skills' names and descriptions and their servers' names (`node.plugins`, bounded); per workspace an item is Shared or Assigned to agents of its set (`plugin_access`, `settings` permission); the master sends deny lists only (`deny_skills`, `deny_mcp_servers`, each sub-agent by its own id), the node withholds them (list, tool, `/name`, shell grants; a denied server is not started), an old node gets the whole tool withheld; the sidebar's **Plugins** screen (replaced the "Connections" placeholder) lists both kinds with a filter and a search (PLAN.md T80).
B: a skills store on the master (`stored_skills`: Built-in skills by the server's admins, Workspace skills by its editors; text only, checked on save; 75 per scope), the Plugins screen's **Add** menu (Write a skill, Upload SKILL.md) and Open, Edit, Delete; each turn's specs carry the names and descriptions their agent may use (`AgentSpec::stored_skills`), a body is asked for only when loaded (`skill.request`, answered from the session alone), the node's own skill wins a name and `[skills] from_master` (default on) lets it take none; the Skills switch alone brings `skill` (a node's folders still need file or command access), so the Assistant and the chat runtime use stored skills (PLAN.md T81).
C: a sub-agent follows its own Connectors switch: a foreground one whose own agent may use MCP (not read-only, not in plan mode, its own `deny_mcp_servers` and the node's `denied_tools`) is offered its chat's servers, the same processes, which work in the chat's folder whoever starts them (`mcp::Home`), its calls asked about in its own chat; a background one too, by the owner's decision of 2026-11-18 ("sub-agents allowed to use mcp depending on their own permissions"; its calls can land between the chat's turns, outside every checkpoint: accepted, PLAN.md T47, T72); a person's message or a retry in a sub-agent's chat gets none; a server node.toml denies whole is no longer started (PLAN.md T47, `node/tests/sub_agent_mcp.rs`).
Names: an item is **Shared** (every agent of the workspace) or **Assigned** (only the agents picked); its origin is Built-in, Workspace, Project or Node.
Each agent has two switches, **Skills** and **Connectors**; a sub-agent follows its own (the owner: "depends on their permissions").
A skill in the store is text only (the agentskills.io fields and limits); the master sends the allowed catalogue with the turn and a body only when the agent asks, as today; a skill that bundles programs is installed on the node, never sent.
Node-defined items (a node's skill folders, `node.toml` servers) are announced by name and get the same Shared or Assigned setting per workspace, default Shared, so nothing changes until an admin chooses.
The node starts only the servers a chat's agents may use. `[skills] from_master` in `node.toml` lets a node refuse master-sent skill text (the node decides).
The screen: one Plugins list per workspace with a filter and an Add menu (a skill; a custom connector, X.35); no code bundles, hooks or marketplace.
This changes the written rule "skill fields are set only by the node": the owner's answer above is the yes, for text.
Abuse tests: an Assigned item never reaches an agent it is not assigned to, a sub-agent included; another workspace's item never reaches a chat; hidden characters and over-limit fields are refused on save.
**L.**

