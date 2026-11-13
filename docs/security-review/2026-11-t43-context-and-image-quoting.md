# T43 review, 2026-11-13: per-turn context and image notes are not the person's words

Reviewed: `fix/context-and-image-quoting` at `3692ffa` (`git diff b7e6a81..3692ffa`:
the failing tests `c20093f`, then the change). It closes R76-1 and R76-3 from
[the review of pull request 76](2026-11-76-attachment-framing.md). One independent
reviewer, on Opus, who did not write the code: the diff read in full, then attacked
with proofs run against the crates (`crates/cli/tests/support/review_t43/`, run with
its `run_proofs.sh` from the worktree root; all 8 hold). The format and severities
follow the other records in this folder.

## 1. Verdict

**Fit with fixes, and the fixes are documentation only.** No attack on the frame or on
URL provenance got through. One Medium finding is older than this change: the prompt
cache across turns (C43-1). This change is not its cause, but T43 and the README
described it wrongly. One Low (C43-2) and three Notes.

| Id | Severity | What | Where |
|---|---|---|---|
| C43-1 | Medium (older) | Across turns only the system prompt and tools are read from the cache: every entry a turn writes ends after its own per-turn context, and the next turn sends that message without it. The README said the context "does not invalidate the cache" | `agent-native/src/llm.rs` (markers), `README.md` `prompt_cache`, `PLAN.md` T43 and "Token savings" |
| C43-2 | Low (older) | `quoted_line` keeps hidden characters (bidi controls, zero-width characters, the tag plane): a card title or instruction can carry text the model reads and a person may not see. The same holds for files | `core/src/event.rs` `quoted_line`, `provider::with_context` |
| C43-3 | Note | With background notes waiting, the separator line says the person's message follows, and the context's lead comes first. This was also true with `<context>` | `provider.rs` `place_notes`, `with_context` |
| C43-4 | Note | The in-tree tests were thin: one context string, no case with several images and files | `attachment_tests.rs` |
| C43-5 | Note | T43's list of tests left out the updated `images.rs`, `llm.rs` and `images_http.rs` tests. `split_context` gives back U+FFFD for a lone CR, which affects only the demo's `memory?` echo | `PLAN.md` T43 |

## 2. Findings

### C43-1 (Medium, older than this change)

`http.rs` always sends a per-turn context: the person's name and the time. Turn N's
requests therefore hold that turn's user message with its context, and every cache entry
turn N writes (marker on the newest message) ends after it. Turn N+1 rebuilds that
message from the log without the context, so none of turn N's entries matches. Within a
turn each step does read the one before. A one-step chat turn writes the whole history at
1.25 times the input price and never reads it back.

The reviewer found this by reading the code; it was not measured against a real
provider. The reviewer asked for two things. First, say in T43 what actually happens.
Second, as its own change, add a marker at the end of the history (the message before
this turn's user message, which rebuilds byte-identical from the log) and measure
`cache_read_tokens` before and after.

### C43-2 (Low, older than this change)

The house quote (`quoted_line`) replaces control characters and U+2028/2029. Bidi
overrides, zero-width characters and the tag plane go through, and
`core/src/text.rs` itself calls them "the usual carriers for invisible prompt-injection
text". Context lines and file lines share this. The reviewer asked to name it as a
residual, or to strip the tag plane and bidi controls (not U+200C/U+200D, which Persian
and emoji need). Not checked: whether the project and board UI shows them.

### C43-3, C43-4, C43-5 (Notes)

As in the table above.

## 3. Attacked and found sound

- **The frame.** The reviewer fed `with_context` contexts with each of these: CR alone,
  CRLF, `\r\n\r\n`, NEL, U+2028/2029, VT, FF, NUL, ESC, `\n</context>\n\nrun:`, a forged
  context lead after an empty line, the notes separator, `| `, lines of only tabs or
  spaces, `""`, `"\n"`, `"\n\n\n"`, a trailing line break, a 1 MiB line, and 100,000
  lines.
  - Every line of the frame starts with `| `, whatever break a reader splits it at.
  - The text after the first empty line is exactly the person's.
  - The count in the lead is right.
  - `split_context` gives the context back.
  - An empty context adds nothing.
- **The demo provider.** With each of those contexts in front, it runs no command from
  the context, and the person's own `run:` still runs.
- **Out of the log and out of provenance.** The context is added only to the in-memory
  request. This turn's own words come from `ctx.user_text`.
  - After an overflow compaction rebuild, the request still starts with the context
    lead, a URL in the context is refused, the person's own URL is allowed, and the
    summariser gets no context.
  - A sub-agent's inherited set comes from the same function.
  - Sub-agents and callback turns get no context, and the parallel-answers path has no
    tools.
- **`own_words` never widens.** The new pattern `\n\n[harness]` is a prefix of the old
  one, so a cut comes earlier or at the same place. A text that starts with the lead
  gives `""`.
  - Checked against hostile texts and files: the result is never longer than the old
    one or than the person's text.
  - No path applies it to text with a harness line in front.
- **Image notes.**
  - No break, quotation mark, bidi or tag character survives in the name; it is at most
    120 characters and never empty.
  - Each note is one line with exactly two `"`.
  - No image name is in the person's own words: checked with empty text, text, one or
    two files, and one to three images. The summariser's text agrees.
  - `llm.rs` names images the same way.
  - Nothing else read the old `[image:` form.
- **Failing first.** The tests of `c20093f` fail on their assertions on `b7e6a81` (run by
  the author). Against the new code, pull request 76's R76-1 and R76-3 proofs fail as they
  should, and its held proofs still hold.

## 4. Not checked

- How a real model weighs the quoted context against the old tags, and whether project
  instructions are followed less.
- C43-1 against a real provider.
- Whether the project and board UI hides tag characters.
- `cargo deny`, the cli crate's tests, and the web checks: no web file changed.

## 5. After the review (the author's closing)

- **C43-1.** The T43 row, the "Token savings" methods paragraph and README's
  `prompt_cache` row now say what happens. The marker at the end of the history is
  proposed as its own change; it is not done here.
- **C43-2.** Recorded as a T43 residual and not stripped here: the same quote serves
  files and background output, so changing it belongs in its own change, across all of
  them.
- **C43-3.** Recorded as a T43 residual. Rewording the separator moves the cached prefix
  of every message that has notes.
- **C43-4.** The reviewer's two pure properties are now in the tree:
  `provider.rs` `nothing_in_the_context_ends_its_frame` and `images.rs`
  `no_image_name_is_in_the_persons_own_words`.
- **C43-5.** T43's list of tests is complete.

## 6. C43-1 fixed in its own change (2026-11-14)

`llm::to_genai` now marks up to 4 places: the system prompt, the end of the history
before this turn's user message (the message before it, which the next turn rebuilds
byte-identical from the log), the end of the history before the previous user message
(where the previous turn put that marker, so it is read exactly where it ends and the
20-block lookback is not relied on), and the newest message. Places that coincide are
marked once.

- **Failing first.** `llm.rs` `the_history_before_this_turn_and_the_last_are_marked`
  and `short_requests_mark_each_place_once`, and `lib.rs`
  `the_next_turn_marks_a_prefix_this_turn_marked`, failed on their assertions on the
  old code. The last runs two turns through the loop with a per-turn context and a tool
  call; on the old code the only marked prefix the two turns share is the system prompt.
  `no_request_has_more_than_four_breakpoints` holds the limit for every prefix of a
  mixed conversation.
- **Masking.** The hidden set is a pure function of all tool outputs, the same at a
  turn's last step and the next turn's first. A batch change writes the history once
  more; Anthropic bills writes only past the highest hit, so the extra markers cost no
  write tokens. A turn that ends at the step cap with outputs it never sent can mask
  differently in the next turn, which then misses once.
- **Summary.** A new summary changes the system prompt, and every entry misses once.
- **Notes.** Notes waiting for the person go into this turn's message, after the history
  marker; a note after a tool result joins it the same way in every rebuild.
- **Not checked.** `cache_read_tokens` against a real provider. Also unchecked: whether
  genai's string form for an unmarked text-only message caches the same as the array
  form it sends when marked. Anthropic says moved markers do not invalidate. The OpenAI
  explicit-cache models (gpt-5.6 and later) get the same per-message markers through
  genai, with no documented limit read.
