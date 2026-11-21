# X.47 **sec** (T91)

## Adds

The `history` tool: an agent reads its own chat's earlier turns in full (the owner, 2026-11-21: "give agents ability to view history, like it conversation was compressed, agent able to view history via that tool, previous media files that were attached and so on").
After a summary the model has a paragraph in place of every earlier event, old tool results are stubs and earlier images and attachments are gone from its context.
Claude Code points the model at the raw transcript file (noisy JSONL read with file tools), Hermes has a session search tool, and Codex, OpenCode, Cline and Zed give it nothing.

## Safeguards and abuse tests

One read-only tool on the node against the session's own log: no master round trip, no approval, its own per-agent switch (`Features.history`, on when absent; the built-in agents have it except Smol, which has no tools at all).
What it reads is the model's own rebuild with compaction and shortening off (`Rebuild::unshortened`), so a rewound range is absent and withheld images stay withheld.
Calls: `history {}` the index (100 turns a page: number, time, who spoke, the person's first 120 characters, tool calls, attachments, images, and whether the turn is now sent only as a summary); `{turn: N}` a turn in full; `{turn, call | attachment | message, part}` one result, attachment or message in parts; `{search}` turns holding the words (3 characters or more, at most 20, newest first); `{turn, image}` an earlier image the way `read` returns one.
Old text is quoted behind `| ` line by line (never a harness line), every answer is inside the tool-result budget, and a `history` call's own earlier results are not copied again.
The compaction summary's harness line gains one sentence when the tool is on.
Tests: `agent-native/src/history_tests.rs`, `agent-native/tests/history_tool.rs`, `node/tests/history.rs`, `agents/tests/agents.rs`, `web/src/components/agents.test.tsx`.
It does not show: rewound turns, images a provider refused, the agent's thinking, a queued message nobody read (one that was read counts under the turn that read it, with its attachments, files and images), the environment line and the context block of old requests, the text of files the person mentioned (their path and state only), or any other chat.
A call id repeated inside a turn is read by its result's number (`"call": "#k"`), and an id or a tool name in a label is cut to a short plain token.
Left out: reading another chat (that is `recall`'s, and it names no chat), search ranking, a way to restore a rewound turn.

## Invariants

The node decides; chats are private to their owner; nothing the model writes is trusted; the harness process stays light

## Podman / Node

No / Yes

## Depends

X.46

## Size

**M**
