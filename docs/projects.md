# Projects: design note

Status: phase 1 built on branch `claude/projects-f61daa`, on top of the workspaces and the
Agents pages on `main` (`crate::workspaces`: Personal and team workspaces, fixed roles). What
still waits on a later workspaces step (workspace-shared memory) is marked **[ws]**. The Agents
page's "Project memory" switch is wired here (see "Memory").

## What a project is

A **project** is a named place for one piece of work inside a **workspace**: a group of chats
plus what the model should know across them. It has:

- a name, a one-line description, an **icon** and an **accent** colour;
- **instructions** (the project brief): text written by people, given to the model in every
  chat of the project;
- its own **memory**: confirmed facts about the project, recalled into its chats;
- a **task board** (columns and cards), made from a template;
- defaults for agentic work: a worker **node** and a **folder** (`cwd`).

Rules:

- **A project belongs to exactly one workspace**, Personal included, and never moves between
  workspaces (moving chats between workspaces is a non-goal of workspaces v1; projects
  follow). Deleting the workspace deletes its projects, boards and saved templates (foreign
  key cascade) and the master forgets their facts.
- **A chat is in at most one project.** A turn needs one unambiguous answer to "which brief,
  which project facts, where does a project `remember` go"; several projects per chat would
  make every turn merge briefs. Grouping across projects (tags) can come later without a
  schema break.
- **A chat moves in and out freely**, by its owner: "New chat" on the project page starts it
  inside; an existing chat is added from the project page or the chat's header; removing it
  only drops the link. The chat and its history are never touched. Deleting a project
  returns its chats to "no project"; it never deletes chats.
- **Chats stay private to their owner (T10), projects do not.** In a team workspace every
  member sees the project, its instructions, its confirmed facts and its board; each member
  sees only *their own* chats in it. Nothing from one member's chat reaches another member
  unless that member explicitly confirms it into the project (see "Memory").
- **The chat's workspace must be the project's workspace.** A chat's *effective* project is
  its link row only while the project is in the chat's effective workspace; otherwise it has
  none. Like the workspaces contract's `session_workspaces`, this self-heals: when a member
  leaves a team workspace and their chats fall back to Personal, the old links go inert
  without a sweep.
- **Incognito chats cannot join a project** (they keep no memory; a project is memory). A
  chat without a workspace (an API-token chat made without `workspace_id`) cannot either.
- **Sub-agent sessions** count as part of their parent's project (looked up through
  `parent_id`), so a sub-agent's `remember` lands in the right place without copying links.

### Agentic work

A project may name a default node and folder. Starting an agentic chat from the project page
prefills them in the bar; creating one through the API with a project and no `cwd`/`node_id`
uses them too. They are only defaults: the node's `node.toml` still decides what is allowed
(T5), and agentic access is still the member's execution level there (the lower of the
account's ceiling and their level in the workspace: the same gate as any agentic chat).

## Memory

Three kinds of facts, all in the existing `MemoryStore` (`crates/memory`, built-in SQLite or
mem0), told apart by the owner key. No new memory table or backend work:

| kind | owner key | who sees and edits | recalled into |
|---|---|---|---|
| personal (exists) | `<user_id>` | that user only | that user's chats |
| project (new) | `project:<project_id>` | every member of the project's workspace sees confirmed facts; adding, rewording or deleting them needs "may manage projects"; a proposal is its source chat's owner's | chats in the project |
| workspace **[ws]** | `workspace:<workspace_id>` | workspace members; writes need `settings` (proposal) | chats in the workspace, and in its projects when the project shares it |

User ids are UUIDs, so the prefixed keys can never collide with a user, and the existing
`/api/memory` routes (which match `owner_id == caller`) can never reach project facts.

**Recall in a project chat** (`Master::prompt`, per-turn context, after the history):

1. personal facts of the chat's owner (top 6, as today), unless the owner keeps their
   personal memory out of this team workspace (`workspaces::uses_memory`) or the agent's
   "Own memory" is off;
2. project facts (top 6, recalled by the message), unless the agent's "Project memory" is
   off;
3. workspace facts (top 4) when the project's **Use workspace memory** switch is on and
   workspace memory exists **[ws]**.

Each kind is its own labelled block ("What you remember about the user…", "What this project
has established…"), so the model knows whose fact is whose. No member's personal facts leak:
personal recall is keyed by the chat's owner, and only the owner can read their chat.

**The member's memory switch gates personal memory only.** A project's facts, instructions and
board belong to the project, not to the member, so a project chat keeps them when its owner
keeps their personal memory out of the workspace. `remember` then stays available in that
chat for project facts only: the project block says "use no other scope", and a personal
proposal from it is dropped (`Master::file_proposal`). Outside projects the switch works as
on `main` (no personal recall, no `remember`).

**"Share the current workspace's memory"** = the project's chats also recall the workspace's
shared facts (read direction). Workspace-shared facts are a non-goal of workspaces v1, so the
switch is stored and shown now (default on, disabled with a note) and takes effect when
workspace memory exists. The other reading, "publish project facts to the workspace", is an
open question below.

**Writing project facts.**
- Anyone who may manage projects adds, edits and deletes them on the project's Memory tab.
- The model proposes them: `remember` gains `scope: "project"` (default `"user"`). The
  per-turn project block tells the model when to use it. A project proposal is filed whenever
  the chat is in a project (its owner is a member: the chat's effective workspace is the
  project's); outside a project it becomes a personal proposal (the user still reviews it).
- **A proposal is visible only to the owner of the chat it came from** (the source session's
  owner, looked up in the mirror). It came from a private chat, so nobody else sees its text
  until that person keeps it (any member, guests included: the project's memory grows from
  its chats). Confirming a proposal publishes it to every member's agent turns, so it needs
  "may manage projects", or the project's **Members may confirm project facts** setting
  (`members_activate_facts`, default off, set by managers): then a member confirms their own
  proposals only. Rewording a proposal stays the owner's. Once active, a fact is edited,
  demoted or forgotten only by managers, with the setting on or off. The agent never confirms:
  `remember` always files a proposal, whoever's chat it is (`Master::file_proposal`); confirming
  is a human `PATCH`. Guests are members here.
  Orphaned proposals (source chat deleted) are shown to nobody and go with the project.
- Deleting a project, or its workspace, deletes its facts.

**For agents** (the "Project memory" switch on the Agents page, `Features.project_memory`,
frontmatter `project_memory`, default on): on = in chats that belong to a project, the agent's
turns get the project's facts and its `remember` may use `scope: "project"`; off = neither (a
project-scoped `remember` from it is filed as a personal proposal when its personal memory is
on, as outside projects, else dropped). It does nothing in chats outside projects. The
project's *instructions* and board are not memory and always come (they are the brief).

How a turn's memory is decided: `Master::prompt` works out what the turn may write,
`TurnMemory { personal, project }` (`crates/master/src/projects.rs`), from the agent's two
switches, the member's workspace switch and whether the chat is in a project. `remember` is
offered only when one of them is on (`offer_remember`, which also checks each sub-agent's own
switches), and every `remember` call is checked against it when the master files it, by scope.
A custom agent gets the `remember` tool when either switch is on.

## Better context

What every turn of a project chat gets, in the per-turn context after the history (PLAN.md
"Token savings": never in the system prompt, which must stay byte-identical for the cache).
The node sends it in front of the person's message under one line of its own,
`[harness] Context for this turn, not part of the person's message: …`, with every line of
it behind `| ` and an empty line after it (`provider::with_context`, PLAN.md T43): the
instructions and card titles are written by other members and cannot close the frame or
pose as the person's message. Unquoted, it reads:

```
Project "<name>" — <description>
This chat is part of the project; its instructions, memory and board are shared with the
people in its workspace.

Project instructions:
<instructions, up to 4,000 characters>

What this project has established (confirmed; may be outdated):
- <up to 6 facts recalled by this message>

Board: In progress: <title>; <title>. To do: <title>; <title> (+3 more).

To keep a lasting decision or fact about this project, call `remember` with scope "project".
```

Budget: the block is rebuilt and sent on every turn, uncached, so it is capped: instructions
≤ 4,000 characters (the editor shows the count and says it goes into every message), ≤ 6
facts, ≤ 8 open cards (titles cut at 80 characters, done columns left out). Worst case ≈ 1,500
tokens; a typical project a few hundred.

Not in phase 1, proposed:
- **Project files**: documents uploaded once (extracted text, like attachments), with FTS5
  passage recall so each turn gets the 2–3 relevant passages, not whole files.
- **"Update the brief from my chats"**: a button; the model reads *the presser's own* chats
  in the project (titles and rolling summaries) and proposes an edit to the instructions,
  which the person accepts or not. Never automatic: an automatic digest would put one
  member's private chats in front of the others, and spend tokens nobody asked for.

## Task board

One board per project, made when the project is.

- **Columns**: name, position, `done` flag (cards there count as finished: left out of the
  model's context, shown faded). 1–12 columns.
- **Cards**: title (≤ 200), notes (≤ 4,000), column, position, an optional link to a chat,
  created by / when. ≤ 500 cards per project. A link to a chat is shown with its title only
  to the chat's owner; other members see "a private chat" (T10). Only your own chats in the
  project can be linked.
- **Templates** give the starting columns:
  - built-in (in code): **Basic** (To do · In progress · Done), the default; **Software**
    (Backlog · To do · In progress · Review · Done); **Writing** (Ideas · Drafting · Editing ·
    Published);
  - user-made, per workspace: "Save columns as a template" on a board; listed next to the
    built-ins when creating a project or resetting an empty board; deletable by anyone who
    may manage projects.
  A template only seeds columns. Applying one to a board with cards is refused (409), so
  nothing is lost silently.
- **Agents**: phase 1 gives them the board *read-only* through the per-turn block. Later, a
  `board` tool (list, add, move cards) run by the master for chat and agentic sessions alike,
  writes allowed only when the chat's owner may manage projects, every change an audit line
  and a card note "by <agent> in <chat>".

## Artifacts — deferred

Worth storing: build outputs from agentic nodes (binaries, `.jar`, `.wasm`, `.apk`, archives),
generated media (images, audio, video), exported documents. Proposal for a later phase:

- metadata in master.db (`artifacts`: id, project, source chat, node, name, kind, size,
  SHA-256, created, pinned); bytes **on the master**, content-addressed under
  `<data>/artifacts/`, for anything up to a size limit (default 50 MB, configurable);
- larger outputs **stay on the node** as a reference (node, path, hash) fetched on demand,
  which needs a streamed `artifact.fetch` in the node protocol (a protocol change, so it waits
  for a real need);
- a `publish` tool for agents, approval-gated like a mutating tool (it moves a file off the
  node), inside the node's folder policy;
- retention: the last 5 versions per (project, name) unless pinned, a per-workspace quota
  (default 2 GB), deleted with the project.

Why deferred: it touches the node protocol and the master's disk budget, and the first two
open questions below decide its shape.

## Icon and accent

- **Accent**: one of the existing `ACCENTS` (`crates/master/src/prefs.rs`; web tokens
  `--accent-<name>` in `styles.css`, both themes). The project's accent tints its icon in the
  sidebar and its page; it never changes the app accent.
- **Icon**: one of 20 names from `lucide-react` (already a dependency), checked by the server
  (`ICONS` in `crates/master/src/projects.rs`) and mapped in `web/src/lib/projectIcons.tsx`.
  A fixed list keeps the sidebar's initial bundle small and the stored value safe.

## Permissions

"May manage projects" is the workspace permission **`settings`**: owners and admins of a team
workspace, and everyone in their own Personal workspace. Code checks the permission, never a
role name. The workspaces contract has no permission of its own for projects; one held by
members too is open question 3.

| action | needs |
|---|---|
| see projects, instructions, confirmed facts, board | membership of the workspace (guests too) |
| create, edit, delete a project; board and template changes; add/edit/delete confirmed project facts | `settings` ("may manage projects") |
| put your chat into a project / take it out; link your chat to a card | membership, and the chat is yours and in the project's workspace |
| word or discard a project proposal | the source chat's owner (any member) |
| confirm a project proposal (make it active) | `settings`, or the source chat's owner when the project's `members_activate_facts` is on |
| set the default node/folder | `settings`; *using* them still needs agentic access |

Non-members get 404 for everything (as for workspaces), managers' actions 403 for the others.
The API token acts as an owner of every existing workspace (contract), so it may do all of the
above; its chats join projects only when it made them in a workspace.

The helpers in `crates/master/src/projects.rs`, over `crate::workspaces`:
- `current_workspace(m, who, requested)` → `workspaces::scope`: the workspace a list, a
  template list or a new project applies to (`?workspace=`, else the active one; `None` only
  for the API token without one, which then lists every workspace's projects).
- `member(m, who, workspace)` → `workspaces::access` (non-members: `NotFound`, said as 404).
- `may_manage(&Access)` → `Access::can(Perm::Settings)`.
- `chat_workspace(row)` → `SessionRow.workspace_id` (the effective one).

## API (JSON; errors `{ "error": "…" }`)

```ts
type ProjectIcon = string;            // one of GET /api/projects → icons
interface ProjectSettings { workspace_memory: boolean; node_id: string | null; cwd: string | null }
interface ProjectSummary {
  project_id: string; workspace_id: string; name: string; description: string;
  icon: ProjectIcon; accent: string; chats: number /* the caller's own */; updated_ms: number;
}
interface ProjectDetail extends ProjectSummary {
  instructions: string; settings: ProjectSettings; can_manage: boolean;
  created_by: string | null; created_ms: number;
}
interface BoardCard { card_id: string; column_id: string; title: string; notes: string;
  position: number; chat: { session_id: string; title: string } | "private" | null;
  created_by: string | null; updated_ms: number }
interface BoardColumn { column_id: string; name: string; done: boolean; cards: BoardCard[] }
interface BoardTemplate { template_id: string; name: string; builtin: boolean;
  columns: { name: string; done: boolean }[] }
```

| method & path | body | answer |
|---|---|---|
| GET `/api/projects?workspace=` | – | `{ projects, workspace_id, can_create, icons, accents }` |
| POST `/api/projects` | `{ name, description?, icon?, accent?, template_id?, instructions?, workspace_id? }` | `ProjectDetail` (users default to the active workspace; the API token must name one) |
| GET `/api/projects/{id}` | – | `ProjectDetail` (404 when not a member) |
| PATCH `/api/projects/{id}` | any of `name, description, icon, accent, instructions, settings` | `ProjectDetail` |
| DELETE `/api/projects/{id}` | – | `{ ok }` (chats return to no project; facts and board deleted) |
| PUT `/api/sessions/{id}/project` | `{ project_id: string \| null }` | `{ ok }` |
| POST `/api/sessions` | gains `project_id?` | as before; the project must be in the chat's workspace (`workspace_id`, else the active one), and the master links the chat when it stamps owner and workspace |
| GET `/api/sessions`, `/api/sessions/{id}` | – | rows gain `project_id` (effective) |
| GET `/api/projects/{id}/board` | – | `{ columns: BoardColumn[] }` |
| POST `/api/projects/{id}/board/columns` | `{ name, done? }` | board |
| PATCH `/api/projects/{id}/board/columns/{col}` | `{ name?, done?, position? }` | board |
| DELETE `/api/projects/{id}/board/columns/{col}` | – | board (409 while it has cards or is the last) |
| POST `/api/projects/{id}/board/cards` | `{ column_id, title, notes?, session_id? }` | board |
| PATCH `/api/projects/{id}/board/cards/{card}` | `{ title?, notes?, column_id?, position?, session_id? }` | board |
| DELETE `/api/projects/{id}/board/cards/{card}` | – | board |
| POST `/api/projects/{id}/board/template` | `{ template_id }` | board (409 when it has cards) |
| GET `/api/board-templates?workspace=` | – | `{ templates: BoardTemplate[] }` (built-ins first) |
| POST `/api/board-templates` | `{ name, project_id }` (its columns) | `BoardTemplate` |
| DELETE `/api/board-templates/{id}` | – | `{ ok }` (built-ins 400) |
| GET `/api/projects/{id}/memory` | – | `{ memories: Fact[] }` (confirmed + your own proposals) |
| POST `/api/projects/{id}/memory` | `{ text }` | `Fact` (confirmed at once) |
| PATCH `/api/projects/{id}/memory/{fact}` | `{ text?, status? }` | `Fact` |
| DELETE `/api/projects/{id}/memory/{fact}` | – | `{ ok }` |

Every board answer is the whole board (boards are small; the client never merges). Every
change sends `{"type":"projects_changed"}` on the WebSocket (content-free, like the others);
project fact changes send `memory_changed` too; a role change (`workspaces_changed`) makes the
web reload its project list, since a role decides who may change projects.

## Storage (master.db; new tables only, no migrations)

In `crates/store/src/projects.rs`, created with the shared schema (so node.db and chat.db get
empty copies, as with the workspace tables). No column is added to `sessions`.

```sql
CREATE TABLE IF NOT EXISTS projects (
    project_id   TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    icon         TEXT NOT NULL,
    accent       TEXT NOT NULL,
    instructions TEXT NOT NULL DEFAULT '',
    settings     TEXT NOT NULL DEFAULT '{}',   -- JSON ProjectSettings
    created_by   TEXT,
    created_ms   INTEGER NOT NULL,
    updated_ms   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS session_projects (       -- which project a chat is in
    session_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(project_id) ON DELETE CASCADE,
    added_ms   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS board_columns (...project_id → projects ON DELETE CASCADE...);
CREATE TABLE IF NOT EXISTS board_cards   (...column_id → board_columns ON DELETE CASCADE...);
CREATE TABLE IF NOT EXISTS board_templates (template_id, workspace_id → workspaces ON DELETE CASCADE, name, columns JSON, ...);
```

Deleting a workspace (or an account, which takes its Personal workspace and the team ones it
was alone in) cascades to its projects, their links, columns and cards, and its templates. The
facts live in the memory store, outside master.db, so the master forgets them after the
deletion (`projects::ids` before, `projects::forget_gone` after). `delete_session` drops the
chat's link and its card links in its transaction. A new chat is linked by
`Store::link_new_session` right after `claim_session`, only when its effective workspace is
the project's.

## Threat model addition (proposed T15)

Project instructions, facts and board titles are written by workspace members and go into
*every* member's turns, agentic ones included: a member could plant instructions aimed at
another member's agent. Mitigations: only managers write instructions, boards and confirmed
facts directly; other members reach project memory only through a proposal from their own
chat that they keep themselves, which managers can edit or forget; the text is cleaned like
memory facts (controls and hidden characters dropped); the block is labelled as project
context written by people; mutating tools still need approval (T4) and node policy still
rules (T5).

## Open questions for the user

1. **Artifacts**: which matter first (build outputs from nodes, or generated media)? Is the
   master's disk the right home for them (with a size limit), or should big files stay on
   the node?
2. **Artifacts, retention**: last 5 versions per name and a 2 GB quota per workspace, or
   something else?
3. **Who manages projects in a team workspace.** Now: owners and admins (`settings`); members
   see projects and boards, put their chats in, and propose facts. Should members also edit
   projects and move cards? That needs a new workspace permission `projects` (owner, admin,
   member; not guest), an addition to the workspaces contract and its UI.
4. **Deleting a project**: anyone who may manage projects (phase 1), or only its creator and
   workspace admins?
5. **"Share the workspace memory"**: read direction (project chats also recall workspace
   facts; my reading), or also publish confirmed project facts to the workspace?
6. **Recents**: project chats stay in Recents with a small project mark (phase 1), or move out
   of Recents into their project, as ChatGPT does?
7. **Personal memory in project chats**: always (subject to the member's workspace switch,
   which leaves project memory on; see "Memory"), or should a project be able to turn
   personal recall off (e.g. client work)?
8. **Board for agents**: should agents move and create cards (phase 3 `board` tool), and
   should their changes need the user's approval?
9. **One board per project** enough, or several (e.g. "Bugs" and "Roadmap")?

## Phases

**Phase 1 (this branch, on top of workspaces)**
- Store: the tables above; projects, links, board, templates; tests.
- Master: `projects` module (validation, icons, built-in templates, the helpers, the per-turn
  block), `http/projects.rs`, `project_id` on session rows and create, `remember`'s
  `scope: "project"`, `Live::ProjectsChanged`; HTTP tests, a team workspace's among them.
- Web: sidebar Projects section between the workspace switcher and Recents, lazy project page
  (New chat, Chats, Board, Memory, Settings), project mark in Recents and the chat header's
  project picker.

What phase 1 adds to the web, beyond the API above: routes `#/p/new`, `#/p/<id>` and
`#/p/<id>/<tab>` in `lib/routes.ts` (`chats`, `board`, `memory`, `settings`; a project
proposal's notice in the chat links to `…/memory`); the board's drag and drop has a keyboard
and touch fallback (the card editor's Column select). Bundle (`pnpm size`), on top of
workspaces: initial JS 96.1 → 99.7 KB gzip (budget 110), total 304.9 → 315.3 KB, so
`TOTAL_KB` went from 310 to 316 (the smallest whole number that fits). The page is its own
5.5 KB chunk; the 20 icons the sidebar draws are 2.2 KB of the first load. On top of the Agents
pages: initial 96.5 → 100.1 KB, total 309.9 → 320.2 KB, so `TOTAL_KB` is 321.

**Phase 2** — a `projects` workspace permission if wanted (open question 3), workspace memory
and the project switch **[ws]**; project files; "Update the brief from my chats"; card
assignees; an agent `board` tool.

**Phase 3** — agents on the board (`board` tool), artifacts.

## Integration points

Where projects meet the **workspaces** code on `main`:

1. `crates/master/src/projects.rs`: the four helpers above (`current_workspace`, `member`,
   `may_manage`, `chat_workspace`) are the only places that ask `crate::workspaces`. A
   `projects` permission (open question 3) is a one-line change in `may_manage`.
2. `Master::create_session`: `CreateSpec::project_id`, checked against the chat's workspace by
   `http/projects.rs::for_new_chat` before the create, and linked in the same mirror call
   that runs `claim_session` (`Store::link_new_session`), so a removal racing the create
   leaves the chat in Personal and outside the team's project. `ProjectsChanged` follows
   `SessionsChanged` when it was linked.
3. `Master::prompt` and `Master::file_proposal`: `workspaces::uses_memory` gates personal
   memory only; project chats keep their project block and project proposals (see "Memory").
   `deny_remember` (agent and delegates) applies when neither kind is allowed.
4. Deletions: the foreign keys on `projects.workspace_id` and `board_templates.workspace_id`
   do the store side. `http/workspaces.rs::delete_workspace` (owner and server-admin delete)
   and `http.rs::delete_user` call `projects::ids` before and `projects::forget_gone` after,
   which forgets the facts of every project that went and sends `ProjectsChanged`.
5. `remove_member` / leave: nothing needed. The member's chats fall back to Personal and their
   project links go inert (`projects::effective` compares workspaces); they come back to life
   only if the chat returns to that workspace.
6. `StoreError::Refused`: `main`'s; `ApiError` maps it to 409 there.
7. Web: `lib/routes.ts` has the `project` route and `projectHash`. `Shell.tsx` lists projects
   with its own `workspaceId` (`projectsApi.list(workspaceId)`), reloads them on
   `projects_changed` and `workspaces_changed`, passes `workspace_id` with `project_id` on a
   new chat, and hands the project page `workspaceId` and `onForeign`: a link to a project of
   another of the user's workspaces switches there and stays on it, as a chat link does. A
   switch from the sidebar while on a project page goes home (`keepsRoute` keeps chats only).
   `shell.test.tsx`'s `../api` mock gained `call`, which `lib/projects.ts` uses. The sidebar's
   Projects section is one component, `ProjectNav` (with `ChatProjectMark` for the Recents
   mark), so a sidebar redesign can move it whole; `Sidebar.tsx` only places it.
8. Bundle: `TOTAL_KB` 321 (see "Phases").
9. `web/src/api.ts`: `call` is exported (used by `lib/projects.ts`); `createSession` takes
   `project_id`; `SessionRow.project_id` is optional in `types.ts`.

With the **Agents pages** work:

10. "Project memory" is `harness_agents::Features::project_memory` (stored switches from
    before it read as on; the prompt hash of an agent with it on is unchanged), a real switch
    on the agent page, and `projects::TurnMemory` in `Master::prompt` / `file_proposal` (see
    "Memory"). A "Workspace memory" switch would add a third field the same way **[ws]**.
11. `ToolCard`'s `remember` notice: `ProjectMemoryNotice` (in `ProjectChip.tsx`) handles
    `scope: "project"`; `MemoryNotice.tsx` is untouched.

Files touched outside the new ones: `crates/store/src/lib.rs` (module, schema init,
`delete_session` cleanup), `crates/agents/src/lib.rs` (`project_memory`),
`crates/master/src/lib.rs` (module, `Live::ProjectsChanged`, `CreateSpec::project_id`, the turn's
memory and the project block in `prompt`, proposal routing),
`crates/master/src/http.rs` (module + merge, `project_id` on session rows and create, project
cleanup in `delete_user`), `crates/master/src/http/workspaces.rs` (project cleanup in
`delete_workspace`), `crates/master/src/ws.rs`, `crates/memory/src/tool.rs` (`scope`),
`crates/agent-native/src/mock.rs` (`remember project:`), and in `web/`: `api.ts`, `types.ts`,
`live.ts`, `lib/i18n.ts` (two keys), `lib/routes.ts` (+ test), `Shell.tsx` (+ its test's
mock), `Sidebar.tsx`, `AgentPage.tsx` (+ test; the Project memory switch), `Composer.tsx`
(`project` prop), `Conversation.tsx` (the header chip),
`ToolCard.tsx`, `styles.css` (one appended section), `scripts/size.mjs`.
