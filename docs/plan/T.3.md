# T.3 **sec**

## Adds

Web and memory tools on agentic nodes

## Safeguards and abuse tests, written first

**Workspace memory (branch `feat/workspace-memory`, owner 2026-11-14: "Workspace memory available to all users in it").**
One memory per workspace (`workspace:<id>`, replacing the per-person `member:<ws>:<user>` key; no compatibility code), given to every member's chats and on Settings → Workspace → Memory for every member; admins and owners change it by hand (every member with a switch, off by default); an agent's save there applies at once only where its chat's owner could, else it waits under Waiting for an admin (count on Overview), and project proposals are now shown to project managers too; `remember` names the place (`about`: me, workspace, project; none: me) and a sensitive fact about the person in a team chat is offered for their Personal workspace instead of the team's memory.
Threat row T54; tests in `crates/master/tests/it/memory_places_http.rs`.
**Memory by default (branch `feat/memory-by-default`, owner's decision of 2026-11-14:** "Memory: in personal workspace, enabled by default, saves usual info over user to its facts, sensitive info over user goes to workspace memory. When joined to new workspace, requires additional ask from user whatever his user facts can be viewed by workspace"**).**
Built: in the person's Personal workspace `remember` from their own turn saves at once in every chat (owner, 2026-11-14, on review M1: "No, always save"; the rule is `memory::saves_without_a_click`), each change shown as its own memory block in the chat with Undo (owner, 2026-11-14: "when a new memory is saved add a new chat entity/block which says so and gives ability to undo memory change"); a sub-agent, a team workspace, a turn not sent by the owner signed in (API token, routine run) and a fact the person wrote ask instead, in the block, with Keep and Dismiss; `sensitive` facts are kept in the workspace they were learned in (`member:<ws>:<user>`), and a correction stays where its fact lives; ordinary ones are the person's own facts; a team workspace asks on joining whether it may use them (unanswered is no) and recalls nothing before the yes; `remember` can replace or forget every kind of fact the chat is given, own, workspace-kept and project facts, in Personal and in teams (owner, 2026-11-14: "replaces / forget - must work in all memory types with ability to revert the change via the block"), retiring rather than deleting, the replacement in the same place, at once where the person may change that fact by hand, else waiting in the block, with a 3,000-character cap on what it saved whose refusal the next turn hears (Hermes); facts are carried whole, with ids, up to the cap.
Team and project memory keep their confirm step.
Threat row T49; tests `crates/master/tests/it/memory_places_http.rs`.
**Memory half done (branch `feat/memory-for-coding`):** agentic nodes register `remember` (on by default, not opt-in as planned: it runs nothing on the node and only files a proposal the person confirms, the approval in the write path; `denied_tools = ["remember"]` turns it off, and the master files nothing from a node whose hello lacks `CAP_REMEMBER`).
Project chats get all active project facts, newest first up to 4,000 characters, recall for the rest; the hint names `remember` only where the node offers it; `remember` alone makes no delegate available (A.11 holds).
Tests: `crates/master/tests/it/coding_memory_http.rs`, `delegate_availability_tests.rs` (`memory_alone_…`), `agentic_nodes_register_remember` in `crates/cli`.
**Web half done (branch `feat/web-tools-agentic`, threat row T89):** agentic nodes register `web_search` and `web_fetch` from their own configuration: the embedded node from the server's `[web]` (one set of tools and one HTTP client shared with the chat runtime), a remote node from `[web]` in its own node.toml, off by default; `denied_tools` switches each off; the search key is in the node's scrubber; the chat runtime's rules hold on the node (public addresses, URL provenance without `read`, no tools for untrusted turns, the key in the scrubber, approval modes, `denied_tools`, size limits; each with a test), with these residuals: the choice among a page's links (capped at 20 per turn), `web_search` queries (400 characters, 20 a turn, no card in Ask), and an API-token turn keeping the tools (PLAN.md T89) (`crates/node/tests/web_tools.rs`, `crates/cli/tests/web_agentic_e2e.rs`);
Researcher is offered again where its tools exist (A.11's rule stays for nodes without them); `doctor` and the log name the tools and the setting.
N-6 is done: `read` does not vouch for a URL (a file can be written by the agent, so a URL in one is not the person's).
**Not done of the web half:** no screening of `web_search` queries (T89 residual 1), no per-chat switch for the tools beyond the agent's tool list and `denied_tools`, a remote node's `[web]` is read at start (restart after changing it), and the planned "web off unless node.toml enables them" holds for remote nodes only (the server's own node follows the server's `[web]`, where `fetch` is on by default).
Recorded (review MC-5): in a team workspace, a member's proposal is shown only to its chat's owner (`proposal_visible`, `crates/master/src/http/projects.rs`), so it is confirmed by nobody unless `members_activate_facts` is on or the owner has `settings`; coding chats will make more such proposals that sit unseen.
To decide before it bites: show pending member proposals to managers, or say on the Memory tab that they wait for a manager.
Planned: Off unless `node.toml` enables them.
Memory approval in the write path, failing closed; staged writes carry a base version; shared memory outside the shell's writable set; threads with external content not auto-extracted;
URL provenance and the public-address resolver kept.
Tests: a URL only in an error is refused on the node; a private address refused; an unapproved memory write refused; a node with web off refuses by name.
Undoes A.11. `07` §6.2.

## Invariants

Master runs no tools: the stress test settles where a node's memory write is stored

## Podman / Node

No / No

## Depends

A.11

## Size

**L**

