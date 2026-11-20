# X.13

## Adds

Earlier chats and memory search (the owner, 2026-11-14: "yes, but must have dedicated permission, like web search, file access do")

## Safeguards and abuse tests

**Done on branch `feat/recall`:** a read-only `recall { query, in: chats \| memory \| both, limit? }` tool behind its own per-agent switch ("Earlier chats and memory search"; on for Assistant, Researcher and Reviewer, off for the built-ins with commands, MCP servers or publishing, for Smol, Scout and new agents: one list, `RECALL_BUILTINS`).
The node files a `recall.request` for its session; the master answers from the existing FTS5 index (`Store::recall_search`, 64-token excerpts) and the memory store, scope decided from the session alone (owner, effective workspace, the memory the turn reads; never another person's, another workspace's, a temporary or a sub-agent chat, never the chat itself), no model call (Hermes `session_search`: FTS5, no LLM, capped excerpts; research round 2 `07` §2 Hermes; Claude Desktop's chat search shows calls as tool calls with links, round 1 `03` "Searching past sessions").
Whether the asking turn may search is read from the log at request time (`Store::open_turn`, `AgentRef::withheld`); each word is a prefix after common English suffixes are trimmed, chats with more words first; excerpts quoted line by line as data and scrubbed with the master's secrets; 8 searches per turn, 40 per chat per hour, 4 at once per node, 10 hits, 6,000 characters.
Not in temporary chats, untrusted or callback turns; sub-agents only with their own switch.
PLAN.md T55.

## Invariants

`master/tests/it/recall_http.rs`, `node/tests/recall.rs`, `memory/src/recall.rs`, `agents/tests/agents.rs`.
Not done: semantic search, reading a whole earlier chat (Hermes's scroll and read shapes), compaction-archived rows (our index keeps every message, so nothing is lost to compaction), links from the tool card to the cited chat.

## Podman / Node

Chats private to their owner

## Depends

No / No

## Size


## Column 8

**M**

