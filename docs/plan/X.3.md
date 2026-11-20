# X.3 **sec**

## Adds

MCP client with deferral

## Safeguards and abuse tests

**First cut done (branch `feat/mcp-client`; PLAN.md T47; README "MCP servers"): stdio servers from `[mcp_servers.<name>]` in node.toml only (Codex's field names: `command`, `args`, `env`, `env_vars`, `enabled`, `enabled_tools`, `disabled_tools`, `startup_timeout_sec`, `tool_timeout_sec`), client on the official SDK (`rmcp` 3.8.0, client half, over our own pipes).
One process per agentic chat (never a sub-agent's; since X.34 step C a foreground sub-agent whose own switch is on uses its chat's), started lazily through the chat's own `Confinement::shell` (its execution mode, folder, file view, network rule, guard; a contained server reads its requests from a FIFO the guard hands to bubblewrap); environment: the core allow-list plus `env_vars` (the node's deny-list wins) plus `env` values (scrubbed).
Gone when the chat is deleted, the node stops, after 10 idle minutes, a call timeout or an over-cap message; at most 3 restarts in a row after failures (a successful call resets the count).
Tools `mcp__<server>__<tool>` (hash on collision), offered to agents whose `tools` hold `mcp` (Software Developer, Senior Teamlead, page agents with the "MCP servers" switch, its own permission since 2026-11-14 by the owner's decision, no longer implied by commands; branch `feat/agent-permissions`, `agents/tests/agents.rs` `mcp_servers_are_their_own_switch`) or the exact name (`denied_tools` takes `mcp` and `mcp__<server>` too; a name two servers would share is offered once); every call `Mutating` (asked in `ask`, "for this turn" per exact call as for every tool), annotations never read; caps: 8 MiB per message, 20 list pages, 40 tools and 64,000 bytes of tool text per chat, 8,000-byte schemas, 4,096-character descriptions, results text only (images and binary resources named) and cut like every tool result; what is left out or did not start is shown once in the chat and told to the model. `doctor` starts each server once.
Notes are scrubbed before they reach the model, and every server-supplied name in one is made one clean line.
The `rmcp` pin must move to 3.9.0 or newer before any HTTP transport is turned on (RUSTSEC-2026-0462, ignored in `deny.toml` because that code is not enabled).
Tests: `tools/tests/mcp.rs`, `agent-native/tests/mcp.rs`, `node/tests/mcp.rs`, `cli/tests/mcp_sandbox.rs`.
Left for later, by the owner's 2026-11-14 direction (usability first, no machinery a first cut does not need): deferral and tool search (needs X.1's loadout event; the 40-tool cap stands in), approvals bound to the executable's digest and keyed per server (A.2), Streamable HTTP and OAuth (the token-in-a-URL question of `10` §6.3 item 10), resources, prompts, `list_changed`, sampling and elicitation (refused by the library defaults), Podman (N.4), per-project server lists and a UI for them, the D12 rerun.**
Was: Only `node.toml` adds a stdio server; PR 4's environment allow-list; inside the session's sandbox or container; approval per server and tool, bound to the executable's digest, keyed as in A.2; `readOnlyHint` only tightens; size and time caps; deferral by measured list size, activation as a loadout event;
OAuth tokens per turn. `10` §6.3 items 4, 7, 10; test D12 rerun against every socket.

## Invariants

Node decides; no secret in a URL

## Podman / Node

Partly / Yes

## Depends

A.2, X.1, N.4

## Size

**L**

