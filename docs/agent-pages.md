# Agents pages: design note

Status: first cut built on branch `claude/agents-pages-d365c2` (see "What this branch builds"),
rebased onto the workspaces work (2026-11-08). Agent edits are still server-wide; what workspace
agents would need is marked **[ws]**.

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
   - **Memory**: Own memory · Workspace memory · Project memory
   - **Web**: Web search
   - **Available as**: Agent · Sub-agent
   - **Tools**: Read files · Change files · Run commands · Launch sub-agents · Manage workspace

## Built-in agents

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
| Own memory | Per-user facts (`crates/memory`): recalled into every turn, the `remember` tool proposes new ones | Works. Off: no recall for that agent's turns and no `remember` tool |
| Workspace memory | Doesn't exist (a non-goal of workspaces v1) | Shown, disabled, "planned" |
| Project memory | Doesn't exist (no "project" concept yet) | Shown, disabled, "planned" |
| Web search | `web_search` + `web_fetch` tools (chat runtime, from `[web]`) | Works (tool list) |
| Available as Agent | Every agent is in the picker and `@mentions` | Works in the web (hidden from picker and mentions). The server doesn't refuse it yet |
| Available as Sub-agent | An agent is a sub-agent only if another lists it in `delegates` | Works: off removes it from every agent's `delegates` |
| Read files | `read`, `grep`, `glob`, `outline`, `graph_query` | Works (tool list) |
| Change files | `write`, `edit` | Works (tool list) |
| Run commands | `bash`, `tool_output` | Works (tool list) |
| Launch sub-agents | `delegates` + the `delegate` tool, one level deep | Works: may delegate to every agent available as a sub-agent (except itself), default caps (4 runs, 200k tokens per turn) |
| Manage workspace | Doesn't exist | Shown, disabled, "planned" |

The features live in `harness_agents::Features`. A custom agent stores them, and its tool list
(and `budget`: `standard` when it may change files, run commands or delegate, else `lean`) comes
from them. For built-in and file agents they are **derived** from the definition
(`Features::of`), so the grid shows what the agent really does. The tool list stays the source of
truth for those. Node policy still wins: a switch is a request, not a permission (PLAN.md §0).

Two new frontmatter fields carry what the tool list can't express: `memory: false` (default
true) and `available_as: [agent, subagent]` (default both). Ultracode is `available_as: [agent]`:
an orchestrator can't run as a sub-agent anyway, because delegation is one level deep.

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
  and workspace facts. The member's "Use my memory in this workspace" switch stays separate: it is
  the person's choice, the agent switch is the agent's.
- **Project memory**: there is no "project" yet. Two candidates: (a) a folder on a node (the
  agentic `cwd`), like CLAUDE.md, with facts keyed by node + repository root, which fits coding
  agents; (b) a future **Projects** sidebar item (PLAN.md lists "Projects later"): a group of chats
  with shared files and instructions, as in ChatGPT. → open question 2.

## Storage

- **Built-in agents**: in the binary, unchanged.
- **File agents**: `<data>/agents/*.md`, loaded at startup, unchanged.
- **Edits**: summary and additional instructions for built-in and file agents, plus the custom
  agents. First cut: the master's `kv` table, `agent-edit:server:<id>` and
  `agent-custom:server:<id>` (JSON). No schema change, so no conflict with the workspaces schema
  being written in parallel.
- **With workspaces** **[ws]**: a table `workspace_agents (workspace_id REFERENCES workspaces ON
  DELETE CASCADE, agent_id, kind, doc JSON, updated_ms, PRIMARY KEY (workspace_id, agent_id))`, so
  deleting a workspace deletes its agents. Server-wide edits keep a `server` scope (kv or the same
  table with a NULL workspace). No migration is needed before the first release (PLAN.md
  principle). Files stay for operator-defined server agents: they are easy to version and copy
  between servers, while a team's agents belong to the team's data, not to the server's disk.
- **Layering** **[ws]**: the set a chat sees = built-ins + files (server) → server edits → the
  workspace's edits and custom agents. A built-in's prompt can then get server *and* workspace
  additional instructions, in that order. A workspace's custom agent ids must not collide with
  server agent ids (refused on create; a later file with that id wins and the custom one is skipped
  with a warning).
- **Looks**: avatars stay in `config.toml` `[agent_avatars]` for now (one mechanism, server-wide).
  A workspace agent **[ws]** should carry its avatar in its own doc instead.

## Permissions

One helper on each side decides "may edit agents here":

- web: `mayEditAgents(me)` in `web/src/lib/agents.ts`;
- master: `require_agent_editor` in `crates/master/src/http/agent_edits.rs`.

Today both mean *server admin*, because every edit is server-wide. **[ws]**: a workspace's own
agents and edits would take the `settings` permission in that workspace: on the master
`crate::workspaces::access(m, who, workspace_id)` then `Access::can(Perm::Settings)` (or
`require(Perm::Settings)`), on the web `can(me.workspace, "settings")` from `lib/workspaces.ts`.
Personal workspaces have the owner holding every permission, so each user could make personal
agents. Server-scope edits (additional instructions on a built-in for the whole server) stay with
harness admins, on this page when the server scope is chosen.

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

- **Deleting an agent that chats use**: the chat's next turn falls back to the default agent
  instead of failing with "unknown agent" (this helps for agent files removed from disk too). Old
  turns keep their agent snapshot (name and prompt hash). Deleting also drops its look and model
  chain from `config.toml`, so a new agent with the same id doesn't inherit them.
- **Ids are permanent**: the handle is the id (`@coder`); it is chosen at creation and can't change
  later (chats, mentions and model chains refer to it). `new` is reserved for the route.
- **Both "available as" switches off**: refused (the agent would be unusable).

## Open questions for the user

1. **"Own memory"**: your personal memory (my reading, and what exists), or the agent's own
   notebook shared across users?
2. **"Project memory"**: a folder/repository on a node, or a future Projects feature (a group of
   chats with shared files)?
3. **Can members read agent instructions?** I said yes. The alternative is that only editors see
   them, as with the GPT store.
4. **A utility model** for summaries, titles and memory extraction, or keep using the default
   model?
5. **Launch sub-agents**: is "every agent available as a sub-agent" enough, or should the page pick
   which ones, as Ultracode's fixed list does?
6. **Should "Available as Agent: off" be enforced by the server** (refuse it as a chat's agent), or
   is hiding it from the pickers enough?
7. **Additional instructions per workspace** **[ws]**: should a team admin be able to add them to
   built-ins for their team (my proposal: yes, after the server's own), or only server admins?
8. **Who can make agents in a team workspace** **[ws]**: only `settings` holders (admins), or
   members too?

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
  agent was deleted goes on with that workspace's default agent, and an agent with memory off gets
  neither recall nor `remember`, in the same condition as incognito and the workspace's "use my
  memory" rule.
- Switching workspace while on an agents page goes to the agents list.
- Stubs: Workspace memory, Project memory and Manage workspace are shown disabled, marked
  "planned". The server doesn't enforce "Available as Agent: off" yet (only the pickers hide it).
- Bundle: the page's lazy chunk is about 5 KB; after the rebase the total is 309.9 of 310 KB and
  the initial load 96.5 of 110 KB (main: 304.9 and 96.1).

## Workspace agents: what is left **[ws]**

- **Agent set per workspace**: `Master::resolver` takes `self.agents()`, one server-wide set
  (`agent_edits::Agents`: base → server edits). Workspace agents would compose a set per workspace
  (base → server edits → that workspace's edits and custom agents) in `Master::resolver` /
  `resolver_for`, from the workspace's stored edits, so every list, picker and turn agrees.
- **Storage**: kv keys `agent-edit:server:*` / `agent-custom:server:*` → a `workspace_agents` table
  with the workspace id and an FK cascade, so deleting a workspace deletes its agents.
- **Permission**: `require_agent_editor` and `mayEditAgents` → `Access::can(Perm::Settings)` /
  `can(me.workspace, "settings")` for the workspace's own agents; the server scope stays with
  harness admins.
- **API**: the agent endpoints already read `?workspace=` for the model line; a write would save
  into that workspace instead of the server's scope.
- **Looks**: avatars stay in `config.toml` `[agent_avatars]`, server-wide; a workspace agent should
  carry its avatar in its own doc.
