# Attached files in a plain chat

Row [X.54](plan/X.54.md), threat row [T95](threats/T95.md), README "Files in a chat without a folder".

## The decision

**Every chat works with attached files; a plain chat gets a bounded reader, not a folder.**
Claude.ai and ChatGPT open an uploaded file in a code sandbox; Claude Code has no upload; X.53 (T94) saves a file into a coding chat's folder.
A plain chat has no folder and no sandbox, and a file the model cannot read as text is the common case (a zip of documents, a log archive, a binary).
The owner chose: readable, never changeable, through a tool the agent calls on demand, with its own switch.

## Shape

- **Upload.** `POST /api/attachments` with `session=<chat>` (or no chat and `agent=<id>` on the home bar) keeps a non-text file as the owner's blob (the same streaming, quota and unsent bounds as X.53) when the agent that will get the message has "Read attached files"; the response says `kept: true`.
- **Send.** `Master::prompt` checks the switch again for the turn's agent, holds a `session_files` row per file (not yet `logged`), and sends the prompt with `attached` references: no byte and no `session.upload` chunk.
  A refusal the node certainly makes releases the rows and gives the uploads back.
- **Record.** The node checks the references on its own limits and records `user_message.attached`; the master marks the rows `logged` when it applies that event, and a fork's copy of the event copies the rows (same node only).
- **Tool.** `attached_file` is node code; a call is one `attached.request`; the master answers from `crates/master/src/attached.rs`, which finds the file by the session and the id, reads the copy once and parses with `crates/attached` in `spawn_blocking`.
- **Parser.** `harness-attached` lists and reads zip (hand-written, stored and deflate), tar, tar.gz and gz and sniffs types with `infer`; every number an archive states is checked, inflation stops at a ceiling and a deadline, nothing is written.
- **Offer.** The tool is offered only to a turn of a plain chat that has a file, on a node that announces `attached-file`, for an agent with the switch; otherwise the master withholds it (`AgentRef::withheld`).

## Why not

- *Unpack into the chat's folder:* a plain chat has none, and a folder is writable by the agent.
- *Extract text for every archive entry up front:* unbounded, and most entries are never read.
- *The `zip` crate:* several new crates and readers that are not bounded by our ceilings; the format's central directory is small enough to read by hand, and every refusal is tested.

## Limits

Per file 10 MiB, 10 files and images a message, 20 unsent; a read shows at most 30,000 characters of text, 4 KiB of hex or runs; a listing shows 500 entries and reads 20,000; 60 calls a turn; two parses at once and 8 seconds each.
