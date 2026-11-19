# Agents pages: design note

Status: first cut built on branch `claude/agents-pages-d365c2` (see "What this branch builds"),
rebased onto the workspaces work (2026-11-08). Workspace agents (ADOPTION_PLAN X.33, PLAN.md T79)
are built: see "Workspace agents" at the end; the items once marked **[ws]** below say what was
built.

## What the user sees

**Sidebar → Agents** (`#/agents`), a page of its own, no longer a Settings tab. One row per agent
of the current workspace: avatar, name, `@handle`, a "built-in" marker, and the summary
(one or two sentences). People who may edit agents also see a pencil (open the agent page) and a
trash can (delete), and a "New agent" button above the list. Everyone else sees the list, and a
row opens the agent page read-only.

**Agent page** (`#/agents/<id>`, `#/agents/new`), top to bottom:

1. Header: avatar (click to change its look; the existing avatar editor), name, `@handle`, and
   delete (agents made here) or Duplicate (built-in and file agents).
2. Summary, editable, with a "Write from instructions" button.
3. Instructions: the agent's own system prompt (a large text area).
4. Features, as a grid of switch groups (two columns on a wide screen, one on a phone):
   - **Memory**: User facts · Workspace memory · Project memory · Earlier chats and memory search
   - **Web**: Web search
   - **Available as**: Agent · Sub-agent
   - **Tools**: Read files · Change files · Run commands · Read-only commands · Skills · Connectors (MCP servers) · Launch sub-agents · Manage workspace

## Built-in agents

The roster (`crates/agents/builtin/`, since 2026-11; Ultracode removed):

| id | aliases | name | category | runs as | tools |
|---|---|---|---|---|---|
| `general` (default) | `ai` | Assistant | General | agent | web_search, web_fetch, remember, board, board_edit; user facts, project memory |
| `smol` | | Smol | General | agent, sub-agent | none; no memory |
| `researcher` | `research` | Researcher | General | agent, sub-agent | web_search, web_fetch, remember; project memory |
| `coder` | `dev` | Software Developer | IT | agent, sub-agent | bash, tool_output, read, write, edit, grep, glob, outline, graph_query, publish, web, remember, board, board_edit; delegates to scout, researcher, reviewer; project memory |
| `teamlead` | | Senior Teamlead | IT | agent | the developer's tools; user facts; delegates to coder, researcher, writer, scout, reviewer (4 runs, 200k tokens per turn) |
| `writer` | | Writer | General | agent, sub-agent | read, write, edit, grep, glob, web, publish, remember; project memory |
| `scout` | | Scout | IT | sub-agent | read, grep, glob, outline, graph_query, read-only commands (`bash` in the read-only shell); no memory |
| `reviewer` | | Reviewer | IT | agent, sub-agent | read, grep, glob, outline, graph_query, read-only commands (`bash` in the read-only shell), remember; project memory |

`crates/agents/tests/agents.rs` (`builtin_roster_is_exactly_the_table`) checks every
row against every tool. Scout and Reviewer are read-only by that list (the shared rule,
PLAN.md T70: every tool they may call is on `harness_core::agent::READ_ONLY_TOOLS`) and
by the read-only shell the node then runs their `bash` in (docs/read-only-shell.md): they
can run `git log`, `git diff`, `git blame`, pipelines and tests that write only to `/tmp`,
and nothing they run can change the folder. A chat or routine that named `ultracode` falls
back like any removed agent (below).

**Defaults per mode.** `Profile::Chat` chats default to `general`, Agentic ones to `coder`
(`DEFAULT_AGENTIC_AGENT`). Workspace settings and user preferences each have `default_agent`
(chats, the key it always had) and `default_agentic_agent`; the workspace's apply on the
server (`Resolver::default_for`), the user's in the web picker on top, as before. Saving
checks that the agent exists, runs as an agent, and for Agentic has file or command tools;
a stored one that went stale is refused when a chat needs it (`BadDefault`). `GET
/api/agents` returns `defaults: {chat, agentic}` for the picker (null when the setting cannot
work). An agent with file tools may be the chats' default: the chat runtime has no file or
command tools, so its calls to them fail with "unknown tool".

**Aliases and categories.** `aliases` (frontmatter, and `aliases` on a custom agent) are
more handles: `@ai`, `"agent_id": "dev"` and a definition's `delegates` resolve to the
id, which is what events, sessions and settings store (settings take ids only). Ids and
aliases are one namespace: creating or changing a custom agent with a handle any agent has,
or that a built-in or file declares, is refused (409) with a message naming the holder
("`ai` is already reserved for the Assistant agent (general)"); an alias listed twice or
equal to the agent's own id is refused (400) saying so. For data from before the roster (a
file `dev.md`, a custom agent `ai`), the id wins and the built-in loses that alias, with a
warning in the log that names both agents (a hard error there would stop an upgraded server
from loading a data directory that was valid the day before); among aliases the first claim wins (built-ins, files, custom
agents). `category` (at most 32 characters, default "General") groups agents in the UI.
Both are optional, so older definitions and saved custom agents still load, and both
are fixed on built-in and file agents.

The user's rule: built-ins cannot be deleted; their name, handle and features are fixed; their
prompt cannot be replaced. What can change: summary, look, and an extra text appended after the
built-in prompt.

**Name: "Additional instructions".** "Custom instructions" is ChatGPT's name for *per-user* text
("what should ChatGPT know about you"), and we may want that later as a personal preference;
reusing the name for an admin's per-agent text would collide. "Additional" also says what it does:
it is added to the built-in prompt, it doesn't replace it.

On the page, a built-in shows its prompt read-only ("Instructions (built in)") and below it an
editable "Additional instructions" area. The prompt the model gets is
`<built-in prompt>\n\n# Additional instructions\n\n<text>`, and the agent's `prompt_hash` changes
with it, so old turns stay explainable (PLAN.md §0 "Agents").

To change a built-in's features or prompt, the user can **Duplicate** it (PLAN.md calls built-ins
"clonable"). That makes a custom agent with the same prompt and features, which is fully editable.

**Agents from files** (`<data>/agents/<id>.md`, written by whoever runs the server) follow the
same rule as built-ins: the file is the source of truth, so the page edits only the summary, the
look and the additional instructions. It shows a "file" marker instead of "built-in". A file can
still override a built-in id; then it counts as a file agent.

So an agent has one of three **origins**: `builtin`, `file`, `custom` (made on this page, stored
in the database).

## Features: what exists and what is new

| Switch | Today | First cut |
|---|---|---|
| User facts (was "Own memory"; key `memory`) | Per-user facts (`crates/memory`): recalled into every turn, the `remember` tool proposes new ones | Works. Off: no recall for that agent's turns and no `remember` tool |
| Workspace memory | Doesn't exist (a non-goal of workspaces v1) | Shown, disabled, "planned" |
| Project memory | A project's facts (docs/projects.md): recalled into its chats' turns, `remember` with `scope: "project"` proposes new ones | Works (added with Projects). Off: no project facts and no project proposals in that agent's turns; the project's instructions and board still come |
| Earlier chats and memory search | `recall` (PLAN.md T55): the person's own earlier chats in the workspace and the memory the chat reads, by keywords, read-only | Works (tool list); its own switch since 2026-11-14 (the owner: "must have dedicated permission, like web search, file access do"). On for Assistant, Researcher and Reviewer (`RECALL_BUILTINS`), off for the built-ins that can run commands, use MCP servers or publish (Software Developer, Senior Teamlead, Writer), for Smol, Scout and a new agent; built-ins are read-only here, so that list is their permission; a page agent saved before it reads with it off |
| Board | `board` (list) and `board_edit` (add, move, append a note; never delete or rewrite) on the task board of the chat's project (PLAN.md T78, docs/projects.md "Task board") | Works (tool list); its own switch (the owner's rule: each capability its own switch). On for Assistant, Software Developer and Senior Teamlead (their definitions list both tools), off for the others and a new agent. A page agent that changes nothing else and has read-only commands gets `board` only, so it stays read-only; changes need the chat owner's "may manage projects". An agent with the switch counts as one that changes things: as a sub-agent it is not put on the small model, even in a chat where `board_edit` is withheld |
| Web search | `web_search` + `web_fetch` tools (chat runtime, from `[web]`) | Works (tool list) |
| Available as Agent | Every agent is in the picker and `@mentions` | Works: hidden from the picker and mentions, and the server refuses it as a chat's or a turn's agent (`Resolver::top_level`) |
| Available as Sub-agent | An agent is a sub-agent only if another lists it in `delegates` | Works: off removes it from every agent's `delegates` |
| Read files | `read`, `grep`, `glob`, `outline`, `graph_query` | Works (tool list) |
| Change files | `write`, `edit` | Works (tool list) |
| Run commands | `bash`, `tool_output` | Works (tool list) |
| Read-only commands | `bash`, `bash_output`, `bash_stop`, `tool_output`, for an agent that changes nothing else (PLAN.md T70, docs/read-only-shell.md) | Works (tool list); its own switch since 2026-11-15 (the owner: every new agent capability is its own per-agent switch). The node runs that agent's `bash` in the read-only shell, judged from the tools it offers (the shared rule `AgentSpec::changes_things`), never from the switch: it can only add the shell to an agent that writes nothing, and with Change files, Run commands or Connectors on it adds nothing (shown disabled). On for Scout and Reviewer, off for a new agent; a file agent shows it when its list holds `bash` and nothing else that changes things. The shell has no network unless node.toml sets `[sandbox] read_only_network = true`. A built-in or file agent with it shows one line under the switches saying its commands run read-only. Where the node has no read-only shell the agent gets no `bash` and is told why; `denied_tools = ["bash"]` still wins |
| Skills | `skill`: loads the skills the node finds (X.2) that this agent may use in its workspace (Plugins, X.34) | Works (tool list); its own switch since X.34 step A (2026-11-18; key `skills`; before, `skill` came with any file or command switch). Since step B (2026-11-18) the switch alone brings the tool: with Read files, Change files or Run commands the node's skill folders and the server's stored skills; without them the stored skills only (text, PLAN.md T81), and the master withholds the tool in a turn that has none to load. On for a new agent, and for a page agent saved before the switch (absent reads as on). On for Software Developer, Senior Teamlead, Writer, Scout, Reviewer and Assistant (their `tools` hold `skill`; the Assistant's since step B, so a chat on the master can use a workspace's stored skills). Which skills: the workspace's Plugins screen (Shared, or Assigned to some agents); the master sends a deny list per turn and the node withholds those, a sub-agent by its own list; `denied_tools = ["skill"]` on the node still wins |
| Connectors (key `mcp`; was "MCP servers") | `mcp`: the tools of the MCP servers the node runs for the chat (X.3) that this agent may use in its workspace (Plugins, X.34) | Works (tool list); its own switch since 2026-11-14 (the owner: "apart permission like file access, web search"), no longer implied by Run commands; shown as Connectors since X.34 step A, the key unchanged. A page agent saved before it reads with it off. On for Software Developer and Senior Teamlead; a file agent has it when its `tools` are `all`, hold `mcp` or name an `mcp__…` tool. Which servers: the workspace's Plugins screen; a server denied to the agent is not started for its turns. A sub-agent follows its own switch (X.34 step C, the owner: "depends on their permissions"), never its parent's: with it on, a foreground sub-agent is offered its chat's servers (the same processes, working in the chat's folder, not the sub-agent's copy; its calls are asked about in its own chat as the parent's are); a background sub-agent the same way (the owner, 2026-11-18: "sub-agents allowed to use mcp depending on their own permissions"; its calls can land between the chat's turns, outside every checkpoint, PLAN.md T47, T72); a person's message or a retry in a sub-agent's chat gets none. An agent the harness treats as read-only never gets one (its tools decide, PLAN.md T70: with Connectors on it is not read-only). `denied_tools` on the node still wins, and a server it denies whole is not started |
| Launch sub-agents | `delegates` + the `delegate` tool, one level deep | Works: may delegate to every agent available as a sub-agent (except itself), default caps (4 runs, 200k tokens per turn) |
| Manage workspace | Doesn't exist | Shown, disabled, "planned" |

The features live in `harness_agents::Features`. A custom agent stores them, and its tool list
comes from them (the `budget` field once derived here was never read and is gone, X.18). For built-in and file agents they are **derived** from the definition
(`Features::of`), so the grid shows what the agent really does. The tool list stays the source of
truth for those. Node policy still wins: a switch is a request, not a permission (PLAN.md §0).

Two new frontmatter fields carry what the tool list can't express: `memory: false` (default
true) and `available_as: [agent, subagent]` (default both). The Teamlead and the Assistant are
`available_as: [agent]` (an orchestrator can't run as a sub-agent anyway, because delegation is
one level deep), Scout `[subagent]`.

Not switches, but worth knowing: model and fallbacks stay in Settings (per-agent chains), and
effort, `ui` level and delegation caps stay frontmatter-only. The page shows the model chain
read-only.

## Memory scopes: what they could mean

- **Own memory**: I read it as *your* memory: the facts you keep in Settings → Memory, which is
  what exists today. The other reading, *the agent's own* notebook (facts it keeps across everyone's
  chats), would need facts keyed by agent and a privacy story, because facts would cross users.
  → open question 1.
- **Workspace memory** **[ws]**: facts shared by a team workspace's members. It needs facts keyed by
  workspace (`owner` = workspace id, or a `scope` column in `crates/memory`), a review flow
  (who approves a proposed team fact: the `settings` permission?), and recall that merges personal
  and workspace facts. The member's "Share my facts with this workspace" switch (off by default
  outside Personal) stays separate: it is
  the person's choice, the agent switch is the agent's.
- **Project memory**: answered by Projects (docs/projects.md): a group of chats in the sidebar with
  instructions, a board and its own facts. The switch is `Features.project_memory` (frontmatter
  `project_memory`, default true). `remember` comes with either memory switch; the master offers
  it per turn only for what the turn may write and checks each call's scope when it files it.
- **Personalisation** (its own group): whether the agent gets each person's Personalisation
  (Settings → Personalisation, PLAN.md T74) in their own chats: the fixed sentences of their reply
  choices and their personal instructions, one standing section of the context block
  (`personal`). `Features.personalisation`, frontmatter `personalisation`, **on by default** for
  every agent (off changes the agent's prompt hash, on leaves earlier hashes as they were). Off means neither the sentences nor the instructions, because an agent with a strict
  output format is disturbed as much by "use emoji" as by free text; the settings line (name,
  place, time zone, About you) is not this switch's. It is the agent's setting, like the others:
  only those who may edit agents change it (today the server's admins). Each built-in
  agent's own definition says (owner, 2026-11-17: "decided by agent, scout dont, others maybe,
  default one - yes"; ADOPTION_PLAN X.31): **Scout off, Reviewer off** (both report to a lead in a
  strict form, the agents a format-changing instruction disturbs most); **Assistant (`general`,
  the default agent), Software Developer (`coder`), Researcher, Senior Teamlead (`teamlead`),
  Writer and Smol on**. The page shows each built-in's true value, read-only like its other
  switches (Duplicate makes an editable copy). Scout is a sub-agent only, so its value matters
  when a lead delegates to it. It travels in the
  agent's spec (`AgentSpec::no_personalisation`, absent = on): the master leaves the section out
  of an off agent's own turns, and the node drops it from what a sub-agent inherits of its parent
  turn when the child's switch is off, for a new child and a continued one (T68). Workspace settings →
  Agents lists the same agents and opens the same pages, so the switch is reachable from there
  too.

## Storage

- **Built-in agents**: in the binary, unchanged.
- **File agents**: `<data>/agents/*.md`, loaded at startup, unchanged.
- **Edits**: summary and additional instructions for built-in and file agents, plus the custom
  agents. First cut: the master's `kv` table, `agent-edit:server:<id>` and
  `agent-custom:server:<id>` (JSON). No schema change, so no conflict with the workspaces schema
  being written in parallel.
- **Workspace agents** (built, X.33): the table `workspace_agents (workspace_id REFERENCES
  workspaces ON DELETE CASCADE, agent_id, kind 'custom'|'overlay', doc JSON at most 64 KiB,
  updated_ms, PRIMARY KEY (workspace_id, agent_id))` (`crates/store/src/workspace_agents.rs`), so
  deleting a workspace deletes its agents. The server's edits stay in `kv` as they were (nothing
  that exists moved). Files stay for operator-defined server agents: they are easy to version and
  copy between servers, while a team's agents belong to the team's data, not to the server's disk.
- **Layering** (built): the set a chat sees = built-ins + files (server) → server edits → the
  workspace's edits and custom agents (`AgentSet::with_layers`). A built-in's prompt gets the
  server's additional instructions under `# Additional instructions`, then the workspace's under
  `# Additional instructions for this workspace`; both change its prompt hash. A workspace's
  overlay goes on any agent it did not make (built-in, file, server custom). A workspace's custom
  agent ids, aliases and names must not collide with any server agent's id, alias or name (names
  compared letters and digits only, case-folded; 409). The server's agents win, one rule for files
  and server agents alike: a server agent made later with a workspace agent's id is accepted, the
  workspace's is skipped there with a warning and listed to its editors as hidden, and the id then
  means the server's agent in that workspace. Ids beat aliases; server agents claim aliases first.
  Deleting a server agent deletes every workspace's overlay on it. Delegates come from the agent's own layer or a wider one: a built-in, file or server
  agent never gets a workspace's agent as a sub-agent, even one with a name its `delegates` lists;
  a workspace agent that delegates gets every sub-agent of the server and of its workspace.
- **Looks**: avatars stay in `config.toml` `[agent_avatars]` (one mechanism, server-wide, changed
  at the server's scope only). A workspace's own agent has the look its id gives
  (`AgentAvatar::default_for`); carrying a chosen look in its doc is left for later.

## Permissions

One helper on each side decides "may edit agents here":

- web: `mayEditAgents(me)` in `web/src/lib/agents.ts`;
- master: `require_agent_editor` in `crates/master/src/http/agent_edits.rs`.

A write names its scope (`?scope=server` or `?workspace=<id>`; neither is 400). The server's
scope is for server admins (`mayEditAgents` on the web, the sidebar Agents page). A workspace's
own agents and edits take the `settings` permission there: `workspaces::access(m, who, id)` then
`Access::require(Perm::Settings)` (a permission, never a role name); a non-member, a server admin
included, gets 404 as for every workspace read; the API token acts as an owner of any workspace,
as it does everywhere. Personal workspaces have the owner holding every permission, so each user
makes agents for themselves there. The listing says whether the caller may edit in its scope
(`may_edit`), and the web follows it (Workspace settings → Agents).

Reading: anyone who can use an agent can open its page read-only, including its instructions. It
is a self-hosted team tool, and knowing what an agent is told helps you trust it. The list endpoint
still leaves prompts out (size). → open question 3.

## Summary generation

"Write from instructions" calls `POST /api/agent-summary` with the name and the instructions as
they are in the editor (for a built-in: its prompt plus the additional instructions). The text
fills the field; nothing is saved until the user presses Save.

- **Model**: the server default (General's main model, the same one a new chat gets), no
  reasoning effort, no tools, one non-streamed request. A small "utility model" setting (for
  titles, summaries, memory extraction) would be better later. → open question 4.
- **Prompt**: "In one or two sentences (at most 200 characters), say what this assistant is for,
  starting with a noun phrase such as 'Software engineer that…'. Plain text." The reply is trimmed
  to one paragraph and 300 characters.
- **Cost**: the instructions (usually 100–2,000 tokens) in, about 60 tokens out, so a fraction of a
  cent on a typical model, and only when someone presses the button. Only editors can call it.

## Edge cases

- **Deleting an agent that chats use** (or removing its file, or a built-in such as `ultracode`
  going away): the chat still lists and opens, but its next prompt is refused with
  `AgentGone` (409, "this chat's agent "x" no longer exists: choose another agent for this
  chat"), and works again after `set_agent` to an existing agent. No fallback: a stand-in could
  be more capable than the agent named (owner's decision, 2026-11). A default agent setting that
  names a missing or sub-agent-only agent is kept as stored and refused when a chat needs it
  (`BadDefault`, 409, naming the setting). A routine naming it is blocked. Old
  turns keep their agent snapshot (name and prompt hash). Deleting also drops its look and model
  chain from `config.toml`, so a new agent with the same id doesn't inherit them.
- **Ids are permanent**: the handle is the id (`@coder`); it is chosen at creation and can't change
  later (chats, mentions and model chains refer to it). `new` is reserved for the route.
- **Both "available as" switches off**: refused (the agent would be unusable).

## Open questions for the user

1. **"Own memory"**: your personal memory (my reading, and what exists), or the agent's own
   notebook shared across users?
2. **"Project memory"**: settled by Projects (a group of chats with its own facts).
3. **Can members read agent instructions?** I said yes. The alternative is that only editors see
   them, as with the GPT store.
4. **A utility model** for summaries, titles and memory extraction, or keep using the default
   model?
5. **Launch sub-agents**: is "every agent available as a sub-agent" enough, or should the page pick
   which ones, as the Teamlead's fixed list does?
6. ~~**Should "Available as Agent: off" be enforced by the server?**~~ Settled (2026-11): yes. The
   master refuses such an agent for a new chat, `set_agent`, a prompt's `agent_id`, a leading
   `@mention` (the prompt is refused with the reason, not answered by another agent; with an
   explicit `agent_id` the mention is only text), workspace and user defaults (refused on save,
   refused when used) and routines (blocked). A chat whose agent became sub-agent-only or was
   removed refuses its next prompt until another agent is chosen.
7. ~~**Additional instructions per workspace**~~ Settled (X.33): yes, after the server's own.
8. ~~**Who can make agents in a team workspace**~~ Settled (X.33): `settings` holders (owners,
   admins); members use them.

## What this branch builds

- Sidebar entry "Agents" under the workspace switcher; routes `#/agents`, `#/agents/new`,
  `#/agents/<id>` in `web/src/lib/routes.ts` (the old `#/settings/server/agents` opens the list).
  The Server → Agents tab is gone: its avatar editor moved to `AvatarEditor.tsx`, and its model
  line to the agent page.
- The list page and the agent page as above, plus Duplicate for built-in and file agents.
- Backend: `GET/POST /api/agents`, `GET/PATCH/DELETE /api/agents/{id}`, `POST /api/agent-summary`;
  `origin` and `features` on every listed agent; edits stored in `kv` and applied to the set the
  master uses (summary → description, additional instructions → prompt, custom agents built from
  their features). Frontmatter gains `memory` and `available_as`.
- `GET /api/agents/{id}`, `POST /api/agents` and `PATCH /api/agents/{id}` take `?workspace=<id>` or
  `?scope=server` like `/api/agents`, so the model line on the page follows the same workspace's
  chains as the list (through the scope's `Resolver`). The edits themselves stay server-wide.
- In `Master::prompt`, the agent is checked against the chat's workspace `Resolver`: a chat whose
  agent was deleted refuses the prompt until another agent is chosen, and an agent with memory off gets
  neither recall nor `remember`, in the same condition as incognito and the workspace's "use my
  memory" rule.
- Switching workspace while on an agents page goes to the agents list.
- Stubs: Workspace memory and Manage workspace are shown disabled, marked "planned" (Project
  memory works since Projects). "Available as Agent: off" is enforced by the server since 2026-11.
- Bundle: the page's lazy chunk is about 5 KB; after the rebase the total is 309.9 of 310 KB and
  the initial load 96.5 of 110 KB (main: 304.9 and 96.1).

## Workspace agents (built, X.33)

- **Agent set per workspace**: `Master::agents_in(workspace)` composes base → server edits → that
  workspace's edits and custom agents (`AgentSet::with_layers`), cached per workspace (at most 32
  kept) and dropped by a write to that workspace, a write at the server's scope, reloading the
  definitions, or the workspace's deletion. `Master::resolver_for` / `resolver_in` /
  `Access::resolver` build every resolver from it, so the listing, the picker, new chats, prompts
  (`agent_id`, `@mention`), the agent switch, workspace defaults and chains, routines, recall and
  the board all resolve through the chat's own workspace set (PLAN.md T79). The server's set
  (`Master::agents`) stays for the server's settings, user preferences and the API token without a
  workspace.
- **Storage**: `workspace_agents` (above). A workspace has at most 30 agents of its own
  (`MAX_WORKSPACE_AGENTS`, checked in the store's transaction too); each document is at most
  64 KiB (`MAX_DOC_BYTES`, also a CHECK on the column). Refused past either, never cut.
- **Permission and API**: above. `GET /api/agents/{id}?workspace=` adds `own`,
  `server_instructions` and `may_edit`; deleting a workspace's agent also drops its model chain
  from the workspace's settings (a default naming it stays, refused when used: `BadDefault`).
- **Web**: Workspace settings → Agents lists the workspace's agents with their origin (built-in,
  server, server file, workspace), a New agent button and the agent page in place, in the
  workspace's scope; the sidebar Agents page edits the server's agents (server admins) and opens a
  workspace's agents read-only.
- **Accepted by the owner (2026-11-18, "Allow it")**: a workspace's admin may give its agents
  "Earlier chats and memory search" with commands, MCP, the web or file edits and make one the
  default (PLAN.md T79, T55).
- **Built (2026-11-19)**: the composer's agent picker, its @mention list and the routine
  editor's agent list say "workspace agent" for a workspace's own agent (`marked` in `web/src/lib/agents.ts`).
- **Left**: chosen looks for workspace agents; per-workspace caps on overlays (bounded by the
  number of server agents).
