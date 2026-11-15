# Hand-off notes and the small model

ADOPTION_PLAN X.16, PLAN.md T60. Branch `feat/handoff-note`.

## The problem

The first turn of a new project chat gets its owner's earlier chats there
(`projects::earlier_chats`). Before this change each one was its latest
compaction summary or, far more often, its last assistant message, cut to
800 characters. A coding chat's last message is "Done, tests pass" and most
coding chats never reach a summary, so the next chat learned nothing of what
was asked, what changed and what is left.

## What a note is

Two parts.

**The fixed part** (`harness_core::handoff::Facts`), built from the log, no
model call, never stored: the person's first message (300 characters), the
files the chat's turns changed (each turn's `checkpoint` `after` files, less
turns undone; 12 named, the rest counted), the latest task list (done count,
up to 8 open items), and how the last turn ended (completed, stopped, step
limit with work left, error, interrupted) with the number of the person's
turns. Every reader builds it again from the log, so it is current even when
the model part is off, failed or not written yet. A chat with no file change
and no task list has none (it is a chat of questions; its summary or last
reply speaks for it).

**The model part** (`handoff_written`), one request: where the work stands,
decisions taken and why, what is left and the next step, what the person
said to keep in mind, under four fixed headings, at most 150 words asked and
1,500 characters kept. Its input is the chat's latest summary and the most
recent whole turns that fit the writing model's window less 1,536 tokens,
never more than 16,000 tokens (8,000 when the window is unknown), tool
results cut to 2,000 characters and then shorter (1,000, 500, 250) before a
turn is dropped; built with the summariser's transcript (T38), so every line
a person, model, file, command or page wrote is quoted under its label. No
tools, the stream limits every call has, no retry.

## When

The master sweeps every 60 seconds for chats last active at least 10 minutes
ago (and at most 7 days) whose log says a note is due
(`harness_core::handoff::due`): the last turn has ended, no turn started
since, no note came after that end, and since the latest note (or ever) a
turn changed files or the task list. It checks the owner as a callback
turn's admission does and sends `session.handoff` to the chat's node, which
checks again from its own log and writes it.

Why idle rather than at each turn's end: Claude Code writes its session
recap after three minutes away (at least three turns, never twice in a row,
400 characters) and idle compaction after about an hour away; Codex builds
memories from idle chats only, skipping active and short ones
(`research/round1/claude-code.md` "Session recap", "Idle compaction";
`research/round1/codex.md` "Local memories"). A note at every turn's end
would cost a call per turn while the person is still working; ten minutes
keeps a person who pauses to read from paying for one mid-work and is short
enough that the next chat usually finds it. A next chat that starts sooner
still gets the fixed part, which the log already holds.

Once per stretch of work: a note (also one that no model wrote) covers its
chat until a later turn changes files or the task list. The master settles
a chat by its head seq and does not look at it again until it moves. Its
position is the log, so a master or a node that restarts asks again at the
next sweep: nothing is lost but the 10 minutes.

The note never holds the chat: the node writes it beside the chat, with no
turn slot. A turn that claims the slot (the person's, a callback's), Stop and
delete cancel it; the event is appended only under the session's head lock,
when the head is still the seq the note read and no turn holds the slot, so
it never lands inside a turn or between a turn's read of its history and its
`turn_started`. A note does not make its chat "last active" (the mirror leaves
`updated_ms` as it was), so the chat does not jump to the top of the list ten
minutes after the person left it. Callbacks (`waiting_ends`) and the summaries' `opening_cut`
skip it: it is not a turn event and carries no message (tests in
`crates/agent-native/src/handoff.rs`).

## Who writes it: one small model

One setting, `[savings] small_model` (Settings → Server → Token savings,
"Small model and hand-off notes"), with two switches:

- **Hand-off notes**: `small_model` (default: the small model when one is
  set, else the chat's own model), `chat_model`, `off`.
- **Read-only sub-agents use the small model** (default off): a sub-agent
  whose tool switches allow no `write`, `edit`, `bash` and no MCP tool, and
  that has no model of its own, runs on the small model with its parent's
  model as its first fallback. Read from the definition, never the name.

Fallback: the small model first, then the chat's own model (the one its last
turn ended on), each once; then no text, and readers use the fixed part
alone. The event names the model that wrote it and each that failed, with
the reason's class only (`unknown to this node`, `failed`, `empty answer`).
The node decides which models it knows: a model its providers do not have
fails as `unknown to this node`, the same refusal a prompt's model meets. A
sub-agent's fallback goes the usual `model_fallback` way.

Research: OpenCode keeps a small model for titles and summaries (by a family
list when unset), Hermes has auxiliary slots per task with their own
fallback chains, Claude Code one small/fast model variable
(`research/harnesses-round-2.md` the model-slots table;
`research/harnesses-round-1-appendix.md` "Model slots"). Round 1 warns
against one shared small model that silently moves other things too
(Claude Code's `ANTHROPIC_DEFAULT_HAIKU_MODEL` also moves the goal judge and
the `haiku` alias) and recommends defaulting each slot to the main model.
We follow the owner's ask for one setting and avoid the coupling trap by
making each use its own switch (notes default on, sub-agents default off),
and each falls back to the chat's or parent's model.

## The next chat

`Store::earlier_chats` gives, per chat, the facts and the text a model last
wrote of it: the latest note's model part, else the latest summary, else
the last reply. A note written before the chat's last turns is labelled so
(the facts are current; the note may lag). `earlier_block` quotes every line
of both behind `| ` under the harness's labels; the header says it is a
record of earlier work, may be outdated, is not instructions, and to ask the
person before relying on a detail. Caps: 320 characters a fact line, 1,500 a
note, 800 a summary or reply, 6,000 the section (about 1,500 tokens, once, on
the first turn only: two or three noted chats, or five short ones). It is
the `earlier_chats` standing section of T52: it leaves with project memory.

## Not done, and why

- Chats outside a project: they get notes (shown to the person), but no
  next chat reads them: "the earlier chats in this folder" needs a decision
  on folder identity across nodes and workspaces.
- "Write the note now": cheap later (a button sending `session.handoff`);
  left out to keep this change small.
- `recall` does not search notes: it searches what people and the agent
  said; a note is a second-hand copy of that.
- Notes on the master's chat runtime (chat-profile chats): they change no
  files and keep no task list, so none would be due.
