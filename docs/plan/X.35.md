# X.35 **sec**

## Adds

Connectors the master answers (the owner, 2026-11-17: "i'd guess more of hybrid … one is forge support (installed aside the node, received secrets from master and holds them in memory as example) and sort of mail connector (server-side only so no aside node install, agent may call it and master will return the result with latest X mails payload as example)").
What exists: step 1 (below); X.3's MCP client stays stdio and node-side.

## Safeguards and abuse tests

**Step 1 done (2026-11-18, T84):** the owner, 2026-11-18: "for connectors make each user can connect own account (if connector supports auth)".
A workspace's admins add a custom connector by address (name, `https` URL, sign-in `none` or a token in `Authorization: Bearer` or a named header; OAuth not built and said so); each member connects their own account (token checked by speaking MCP, stored sealed, bound to the connector's URL; an edited URL or sign-in disconnects everyone first) and sees what it offers (server, instructions, tools, as text, capped, in memory only).
The master's client is rmcp's streamable HTTP transport on our own HTTP client (`sse-stream` 0.2.6 added): shared rules with forges (`remote.rs`), 512 KiB, 8,192 JSON values and 10 s per check, no redirect and nothing compressed, error bodies unread, our headers only, no standalone stream, no capability advertised; checks run on two long-lived workers that log nothing (a third at once, or a second of one person, is refused at once).
After review (PR 155): the token's header is an allow rule (`Authorization`, `*-Key`/`-Token`/`-Auth`, `X-*` but forwarding and tracing ones) and changing it disconnects everyone as a new URL does; connect and check carry what the person saw and are refused when the connector changed.
A custom connector is a Plugins item by name (Shared / Assigned).
README "Custom connectors".
**Step 2 done (2026-11-19, T85):** agents call a connector's tools through the master: the node's MCP tool `mcp__<connector>__<tool>` files `connector.request`, the master answers with the chat OWNER's own account, only in a chat in which nobody but the owner has ever acted (a persisted mark on the chat and every chat above it, set durably before any other caller's action is forwarded; such a chat's memory and facts are proposals only, its turns change no board, its agent adds nothing to a file shared with the workspace, and it is left out of "Earlier chats", checked at each call so a mark during a running turn stops that turn's writes too; text written by people with rights for every chat, such as agent instructions, project briefs and board cards, is not covered and only the approval card stops a call it steers (the owner, 2026-11-19, asked whether a connector call should always ask the first time even in Auto: "No, keep Auto"); after five reviews of PR 157) and only in a turn the master itself started for the owner, under the Connectors switch and Shared / Assigned (a sub-agent by its own), asked like any MCP call, result text only; tools with schemas cached per person from checks (background reads, never on a turn's path), checks and calls on one connector thread (4 call slots), 2 calls per person and per connector, 30 per granted turn and 200 per chat per hour, 256 KiB and 120 s per call; approval keys carry the tool's origin (connector or node server); node.toml `[mcp] master_connectors` (default on).
**Left (later items, not this row's steps):** browser (OAuth) sign-in, MCP resources and prompts, server-initiated messages, partial results while a call runs. (2) **No mail connector:** the owner, 2026-11-17: "it was as an example, dont needs implementation".
Should one be asked for later, the reason for care stands: mail is written by strangers, and with a shell or web access beside it a letter can ask the agent to send the inbox out.
Forge accounts stay X.27 and stay on the node.
**M to L.**

