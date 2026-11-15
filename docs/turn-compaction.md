# Shortening context inside a turn (ADOPTION_PLAN X.15, PLAN.md T59)

A coding turn may take 1,000 steps (T56). Before this change the loop shortened
the conversation only between turns, kept the two newest turns whole however
large, and inside a turn waited for the provider to refuse a request as too long;
then it summarised older turns, hid old tool output and, if that did not help,
ended the turn with a wrap-up. On a small-window model the first live test
(2026-11-15) saw the summary come too late and free too little. Now the request is
shortened before it is sent, in two tiers, and only what the model is sent
changes: the log keeps every event and the chat shows every result.

## When

With the answering model's window `W` known (`usage.window`):

| | value | why |
|---|---|---|
| line `at` | `W` less its reserve (a fifth, at least 16k, at most half), or a configured `summarize_after_tokens` above that | the same line the chat meter shows as "summary at"; inside a turn the question is what fits |
| target | `at / 2` | clearing frees down to it: the next cut is `at / 2` tokens of new work away |
| `min_cut` | `at / 4` | clearing does not act for less; steps are summarised only when clearing leaves less than this below `at`; after any check the next waits until the request grew by it |
| tail | `W / 40` within 10k..25k, at most `W / 5` and `at / 4` | Hermes' lean tail; the newest step is always in it, whatever its size |

The request's size is our estimate (4 characters a token) corrected by the
provider's count of the previous request (Pi's measure), the correction bounded to
between three quarters and twice the estimate: real tokenizers stay within about 0.6
to 1.6 times four characters a token, and a router that reports nonsense must not make
every step look full (review M2: at ten times, 12 summaries in 60 steps before the cap,
one after). With a known window the count-based mask (`keep_tool_outputs`, "Hide old
tool outputs", on by default) does not act: it hid outputs by count every 8 steps,
rewrote the cache that often, and kept clearing from ever acting on a large window
(review M1: 36 rewrites and no cut in a 300-step turn on 128k). With an unknown window
nothing changes inside a turn: the summary between turns at 24k, the count-based mask
and the reactive path on a provider's refusal, as before.

## Tier 1: clearing (no model call)

The oldest tool results before the tail become a one-line stub, oldest first, until
the request is at the target:

```
[harness] This result was cleared to make room in your context (the log keeps it, and the person still sees it): `read` "src/parse.rs" returned 412 lines, 18204 characters. The file still counts as read for `edit` and `write`; read it again (a line range is enough) if you need its text.
```

- Built from the call only (its tool's name when it is a plain name, one argument
  as a quoted JSON string on one line, cut to 120 characters) and the result's size.
  No text of the result survives, so nothing a page, a file or a command printed can
  pose as the harness's line. It is sent as the call's note after an empty result,
  so `escape_leads` does not touch it and the result's own `[harness]` lines go with
  the result.
- The call and its arguments stay (Hermes never rewrites them; a summary of a cleared
  pair would lose what was asked).
- Never cleared: results under 1,000 characters, the `todo` tool's, and every load
  of a skill's instructions (Hermes keeps a just-loaded skill body; Oh My Pi protects
  plan and skill reads). Keeping only the newest load would clear an earlier one when
  the skill is loaded again, a change no event records.
- Advice by tool: read-only tools are called again; `read` also says the file
  still counts as read (and to read a few files per step); any other tool (`bash`,
  `edit`, `write`, `delegate`, MCP) "ran once: do not run it again only to see this
  output".
- One step alone over the line (a model that reads 30 files in one message, seen
  live): its older results are cleared too, all but its newest.
- Where clearing can happen (a known window), the system prompt says so once, and
  asks the model to write down the facts it will need and to read a cleared result
  again before relying on it (same bytes every turn, so cached). Added after the live
  test below.

## Tier 2: summarising steps

When clearing leaves less than `min_cut` below the line, earlier turns go into the
summary first (as between turns, all of them), then the running turn's steps before
the tail are summarised with the existing summariser (`CODING_SUMMARY_PROMPT` or
`SUMMARY_PROMPT`), in one request whose tool results are cut to fit the window (2,000
characters each, then 1,000, 500, 250). The summary is sent as a user-role message
of its own right after the message before the turn's first step (the opening), under
the harness's line, each of its lines quoted, with the task list as it stood then;
model-written text never goes inside the person's message. The opening itself (the
person's words, attachments, the context block with its standing sections) stays as
it was, so do the person's messages sent during the summarised steps, placed after
the summary, and the steps after it. A summary is asked for only when dropping those
steps would make room: a system prompt and tools that fill the window alone are left
to the last resort.

A summary of earlier turns made inside a turn never takes what the running turn's
opening reads from the log: it stops before the turn in which a queued message the
running turn read at its start was sent, and before a background end logged after the
last turn (what a callback turn's opening takes). Otherwise the next request rebuilt
from the log would lose the person's message (review H1). A later
summary of the same turn reads the earlier one and covers the turn from its start.
The shrink check (at most nine tenths of what it replaces) and the breaker (two
failures end summarising for the turn) are A.7's. At most 12 per turn.

## Recorded

Each cut is one `context_shortened` event, appended by the loop through the session's
one writer before the request it shortens: `cleared {turn_id, results}` (every result
of the turns before `turn_id` and the first `results` of it; the latest is in force),
`folded {steps, text, todo?}` (the event's turn's first `steps` steps; the latest per
turn is in force) and the sizes before and after. Anchors are turns and positions in
them, not `seq`s, which the loop does not learn; a turn a later summary covers simply
is not there any more. `rebuild` applies the latest state to what it rebuilds, so the
next request, the next turn, a retry or a restart send the same bytes (test
`the_log_rebuilds_the_same_request`). `history_summarized` was not extended: it means
"everything up to seq N" to every reader (the handoff summary in `store`, the meter,
the UI), and an in-turn cut is neither.

## The guards

- **Read before write (T45).** The file table lives outside the history and is
  not touched. A file whose read was cleared still counts as read: `edit` is a string
  match, so it still proves the model has the text, and a changed file is refused by
  its hash as before; the stub says so. Clearing the table at compaction is an idea
  in the research with no evidence (round 2 appendix §6.1 item 6); not done.
- **Repeated calls (T40).** A call whose result left the request may be read again
  without counting as a repeat (`RepeatGuard::forgive`), once per call per turn, so a
  model stuck reading one large file is still refused after at most four more runs
  (test `clearing_does_not_defeat_the_repeat_guard`).

## Cost to the provider's cache

Each cut changes bytes from the first newly cleared result on. Measured with 300
steps of 6,000-character results (about 1,550 tokens a step) and the default token-
saving settings (`TokenSavings::default()`, the count mask on with keep 8, which with
a known window does not act): 7 rewrites on a 128k window (each frees about 51k
tokens, one every 36 steps), 52 on a 32k window (about 8k each, one every 5 to 6
steps, a summary about every 100 steps). Never on two steps in a row. Both bounds are
tests (`shortening_is_rare_and_never_on_consecutive_steps`,
`mask_and_clearing_at_128k`).

## Live check (2026-11-15, the owner's router, synthetic repository)

Thirty synthetic files of 6,000 characters, a 32k window set on the model, the task
"read every file, then say which defines `checksum`, its constant, and how many
functions two files have":

- `mistral-small`, all 30 reads in one message: 52,980 tokens cut to 6,851 at once
  (in-step clearing); it re-read the three files it needed (two by line range) and
  answered all four questions right.
- `mistral-small`, one read per message: five cuts, every five or six steps, about
  8k tokens each, no overflow, no loop. Without the system prompt's sentence it
  re-read nothing and answered three of four wrong from results it no longer had;
  with it, it re-read the file whose count it needed and answered three of four
  right (the constant, from a cleared result, was still made up).
- `mistral-medium`, one read per message: it saw the stubs, re-read the files in a
  second pass, writing what it found as it went ("No `checksum` in src/m03.rs"), one
  summary of its first 49 steps came on the way, and it answered all four right
  after re-reading the two files it was unsure of: 62 reads for 30 files.

So clearing does what it should for the window, and a weaker model can still answer
from what it no longer sees; the stub and the system prompt tell it not to, which
helps but does not guarantee it.

## By hand: `/compact` (ADOPTION_PLAN X.20, PLAN.md T64)

Between turns the chat's owner can summarise now: `/compact [focus]` in the
composer. It travels as its own route and message (`POST
/api/sessions/{id}/compact`, `session.compact`), never as a prompt, and the
node runs the summariser of a turn's start (`compact_with`, the same prompt,
shrink check and breaker) as an operation of the session's one writer:

- **Kept:** the last turn verbatim (`MANUAL_KEEP_TURNS` = 1; a turn's start
  keeps `KEEP_TURNS` = 2), what the next turn's opening reads
  (`opening_cut`), and, as after any summary, the task list and the standing
  context. Less than 1,000 tokens before the last turn is "too short".
- **Focus:** the person's words, scrubbed and cut to 2,000 characters, as
  the transcript's last part under a `Focus:` label, quoted like every part
  (T38); both summary prompts say what that part is and that it shifts
  emphasis only.
- **Recorded:** `history_summarized` with `manual`, `focus`, `usage` and
  `tokens_after`, appended only if the log's head is still what was read;
  otherwise, and on a model error, a summary that is not smaller, or Stop,
  `compact_failed` with the reason. Two failures since the last turn refuse
  the next.
- **Order:** refused while a turn runs; while it runs no turn starts (a
  prompt is refused, a callback asked again later, a hand-off note dropped).

## What the others do (research)

- Hermes: prunes old tool results over 200 characters with no model call before
  summarising; tail 2.5% of the window (10k..25k, at most 20%); keeps the last
  user message, the pending tool round and a just-loaded skill; never rewrites call
  arguments (`research/round2/synth/07-context-compaction-memory.md`, Hermes).
- OpenClaw: hard-clears whole results to a placeholder at 50% of the window, never in
  the last three assistant turns; frozen projections recorded so a restart rebuilds
  the same prefix (same file, OpenClaw "Pruning of old tool output"). It is off by
  default, except that the Anthropic provider turns it on (`cache-ttl`, 1 h) when
  Anthropic auth is found and the mode was left unset.
- Pi: compacts after each tool batch at window less 16,384; keeps about 20,000 tokens;
  cuts never on a tool result; an oversized turn is split and its early part summarised
  separately (same file, Pi "Compaction" items 3-4, 6).
- Codex: mid-turn compaction at 90% of the window; old outputs vanish only at
  compaction (same file, Codex).
- Claude Code: compacts at the limit and stops with a "thrashing" error after
  repeated refills; re-attaches recent files and skills after a compaction
  (`research/round1/claude-code.md`); clearing old tool results as a context step is
  not documented (`research/round1/synth/03-context-memory.md`, "Pruning").
