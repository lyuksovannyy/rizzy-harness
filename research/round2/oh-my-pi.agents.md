# Oh My Pi (`omp`), Round 2 source investigation: agents cluster

## 1. Repository

- `can1357/oh-my-pi`, pinned SHA `e325ffc73ab86e03bdd20a91b77d8e2b3964c906`, commit date 2026-11-10T21:11:46+01:00, branch `main` (from `research/round2/00-repos.tsv`; `git log -1` agrees).
- Read, not run. Areas read: the Rust edit engine `crates/pi-edit` (hashline, replace, fuzzy, store, sloppy prompt), the snapcompact renderer entry in `crates/pi-natives/src/snapcompact.rs`, and in TypeScript `packages/coding-agent/src/{edit,session,task,config,discovery,capability,export/ttsr.ts,system-prompt.ts,tools/read.ts,tools/plan-mode-guard.ts,prompts}`, `packages/agent/src/compaction/compaction.ts`, `packages/snapcompact/src/snapcompact.ts`, `packages/tui/src/overlays/model-browser.ts`, and `packages/catalog/src/compat/rules/runtime/behavior.kdl`.
- Round 1 file checked: `research/round1/oh-my-pi.md` (areas 3, 6, 7, 8, 9, 13 TTSR, 14, Distinctive, Review).

## 2. Architecture as it bears on this cluster

- **Languages and split.** The session, the agent loop, prompts, compaction, TTSR, roles and sub-agents are TypeScript on Bun (`packages/coding-agent`, `packages/agent`). The edit engines are Rust in `crates/pi-edit`, a port of an earlier TS package (the module header says so: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/mod.rs#L1-L3). Snapcompact rasterises in Rust through N-API (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-natives/src/snapcompact.rs#L1067-L1073).
- **Edit engine dispatch.** One `ModeEngine` per mode, stateless, policy per call: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/mod.rs#L11-L30. Session state (snapshots, clipboard registers, no-op counters) lives in `EditStore`: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/store.rs#L190-L194.
- **Compaction.** The policy loop is `SessionMaintenance.runAutoCompaction` in `packages/coding-agent/src/session/session-maintenance.ts` (6,677 lines); the engine primitives (`shouldCompact`, `prepareCompaction`, `compact`) are in `packages/agent/src/compaction/compaction.ts`.
- **TTSR.** `TtsrCoordinator` (session side: abort, inject, resume) over `TtsrManager` (rule registry and incremental stream matcher): https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/ttsr-coordinator.ts#L74-L101.
- **Sub-agents.** In-process `AgentSession`s (main thread, or worker threads above eight concurrent), not child processes: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/settings.ts#L438-L458.

## 3. Findings

### Edit tools and formats

#### Hashline is a per-FILE 4-hex snapshot tag plus bare line numbers, not a per-line hash
- What the code does: the tag is `xxh32(seed 0)` over the whole file with trailing spaces, tabs and CRs stripped from every line, masked to the low 16 bits and printed as four uppercase hex digits. Lines carry no hash at all; an anchor is just a 1-indexed line number. The file header is `[path#TAG]` and a numbered row is `N:text`.
- Trace: hash function https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/store.rs#L70-L91 ; tag length and regex https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/format.rs#L43-L48 ; header and row encoding https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/format.rs#L96-L110 ; `Anchor { line }` only https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/types.rs#L7-L11 .
- Edge cases: the tag ignores trailing whitespace, so an edit that only changes trailing blanks does not change the tag. 16 bits means a 1-in-65,536 chance that a changed file keeps its tag (see the collision finding).
- Round 1: `refuted`. Round 1 (from README and blog) said every line is tagged with a short content hash (`2:f1|...`). The code tags the file once; lines are bare numbers.
- For rizzy-harness: a file-level snapshot tag is cheap to compute and display and gives the "changed since read" check; copy the idea but with a longer tag (32 bits or more) since our nodes may serve long sessions.
- Confidence: `traced end to end`. (checked in review)

#### Tags are shown on read and bound to a retained snapshot with "seen lines"
- What the code does: `read` records the normalised file text in the session `EditStore` and prepends `[path#TAG]` to hashline-formatted output; then it records which rows were displayed in full (`recordFullySeenRows`, skipping column-cut lines). The store keeps up to 4 versions per path, 256 paths, 64 MiB, and never snapshots files over 4 MiB.
- Trace: read records the tag https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/read.ts#L2862-L2890 ; seen rows recorded https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/read.ts#L3049-L3051 ; limits https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/store.rs#L13-L24 ; `record` dedupes by hash and text, keeps newest first https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/store.rs#L218-L254 .
- Edge cases: a first line longer than the read byte cap gets no editable numbered preview in hashline mode (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/read.ts#L2951-L2954). The store is in memory: after a restart the "seen" provenance is gone (handled below).
- Round 1: `new (not documented)`.
- For rizzy-harness: binding "what the model saw" to the edit is the real safety property, more than the hash; worth copying in the node's edit tool.
- Confidence: `traced end to end`. (checked in review)

#### The edit grammar: `PUT`/`CUT`/`REM`/`MV` hunks under a tagged header
- What the code does: ops are `PUT N.=M:` (replace inclusive range with `+` body rows), `PUT <N:`/`PUT >N:` (insert), `PUT N*:` (replace the syntactic block opened on line N), `CUT` with optional named clipboard registers, `REM` and `MV`. Line numbers must be those of the read, never hunk-shifted. A section without a tag never applies.
- Trace: tool prompt https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/prompts/hashline.md#L1-L17 ; keywords https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/format.rs#L7-L38 ; header parsing strips `*** Update File:` noise copied from apply_patch https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/mod.rs#L97-L127 ; untagged section gets the mismatch reply https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/patcher.rs#L845-L849 .
- Edge cases: two sections resolving to the same canonical file are rejected (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/patcher.rs#L853-L860); all sections stage atomically before anything is written (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/patcher.rs#L797-L808).
- Round 1: `corrected` (format unknown in Round 1).
- For rizzy-harness: block-anchored ops (`N*`) need a parser per language; the line-range ops alone are enough for a first version.
- Confidence: `read the definition`.

#### When the tag is live: the seen-line guard rejects anchors the model never saw
- What the code does: if the tag equals the live file's hash, the edit applies directly, but first every anchored line (and every line of a replaced or cut block) must be in the snapshot's seen set. Unseen lines get a rejection that reveals up to a cap of those rows, so the retry can proceed; lines over a column cap force a full read. With no retained snapshot (restart, eviction), only lines longer than `max_columns` are refused, because only those could have been cut on display.
- Trace: `apply_with_recovery` live branch https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/patcher.rs#L601-L657 ; `assert_seen_lines` https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/patcher.rs#L424-L527 ; setting `edit.enforceSeenLines` default true https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/edit/settings.ts#L149-L159 .
- Edge cases: the reveal itself is recorded as seen, so the second attempt passes; a probe (dry-run) works on a forked store so a consumed reveal cannot leak (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/store.rs#L208-L215).
- Round 1: `new (not documented)`.
- For rizzy-harness: copy. "You may only edit lines you were shown" catches edits built from `grep -n` output or hallucinated line numbers.
- Confidence: `traced end to end`. (checked in review)

#### When the tag is stale: diff-based anchor remapping with four refusal checks
- What the code does: a stale tag that matches a retained version is rebased: a line diff (`pi_diff::line_runs_str`) maps old numbers to new for unchanged lines only; every anchor must map, all anchors must shift by the SAME offset, each anchor needs a neighbouring line that moved by that offset (strictly both neighbours for duplicated lines), the enclosing syntax nodes must match by opener text, and a shifted replacement whose body resembles the lines at the ORIGINAL numbers (50% floor, 0.3 margin) is refused as "numbers were already current". A remap that shifts anchors (offset not 0) and breaks a file that parsed is also refused; when every anchor maps with offset 0 the edit applies to the new text with an external-change warning and that parse check is skipped (`recovery.rs` L540-L549). Success applies with a warning; failure returns the mismatch reply.
- Trace: line map https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/recovery.rs#L69-L87 ; neighbour and duplicate check https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/recovery.rs#L128-L192 ; uniform offset https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/recovery.rs#L301-L305 ; syntax context https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/recovery.rs#L334-L368 ; authored-lines heuristic https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/recovery.rs#L438-L495 ; `try_recover` https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/recovery.rs#L503-L565 ; caller https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/patcher.rs#L674-L705 .
- Edge cases: a DELETED anchor line has no map entry, so `map_line` returns `None` and recovery fails to the mismatch reply. A DUPLICATED line needs both neighbours to agree, else refusal. Edits with no anchors (pure BOF/EOF inserts) apply on a stale tag with a drift warning (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/patcher.rs#L663-L673). A bare `MV`/`REM` on a stale tag is always refused (L658-L662).
- Round 1: `corrected`. Round 1 said a changed file is "rejected before corruption". The code rejects only when it cannot prove a safe uniform rebase; otherwise it applies on the new text.
- For rizzy-harness: the rebase is clever but large (565 lines plus a tree-sitter dependency). A first version should reject on any mismatch and return the current tag and rows; add rebase only with evidence of wasted retries.
- Confidence: `traced end to end`. (checked in review) (corrected in review: the broke-a-parsed-file refusal applies only to shifted remaps, offset != 0; offset-0 recovery has no such check)

#### Collisions and unknown tags: what the 16-bit tag does and does not protect
- What the code does: `by_hash` returns the most recent retained version whose tag matches; if a stale tag happens to equal the live file's hash, `live_matches` is true and the edit applies to live content with no recovery checks, protected only by the seen-line guard. A tag that is not in the store is "not from this session"; a tag issued for a different path in the session is named in the reply so a worktree lane does not write the wrong tree.
- Trace: `by_hash` https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/store.rs#L325-L336 ; `live_matches` https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/patcher.rs#L610-L612 ; rejection classes `StaleTag`/`MissingTag`/`UnknownTag`/`OtherFileTag` https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/mismatch.rs#L142-L153 ; reply text https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/mismatch.rs#L75-L122 ; path recovery from a unique tag plus file name https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/patcher.rs#L733-L761 .
- Edge cases: no explicit collision handling exists. A same-path, two-version collision inside the 4-version window resolves to the newest; a live collision bypasses recovery entirely (inferred from the code path, no test found for it).
- Round 1: `new (not documented)`.
- For rizzy-harness: use at least 32 bits; a short tag is a token-cost optimisation that becomes a correctness bet in long sessions.
- Confidence: `traced end to end` (the collision consequence is `inferred`). (checked in review)

#### Identical no-op edits escalate after three repeats
- What the code does: a hashline patch that changes nothing returns a "no change" diagnostic; the same payload (xxh64 of the raw input) on the same path three times in a row becomes a hard `NoChange` rejection.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/patcher.rs#L927-L936 ; counter https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/store.rs#L428-L439 .
- Round 1: `new (not documented)`.
- For rizzy-harness: a cheap stuck-loop detector at the tool level; copy.
- Confidence: `read the definition`. (checked in review)

#### Five edit modes coexist; hashline is the default but auto-downgrades for some model families
- What the code does: `edit.mode` is one of `apply_patch`, `hashline`, `patch`, `replace`, `sloppy` or `none` (no edit tool, bash forced on); default `hashline`. `edit.modelVariants` maps model-name substrings to modes. When the mode came from settings (not `PI_EDIT_VARIANT`) and is hashline, models classified as kimi, mimo, minimax, deepseek, stepfun, codex-spark, glm flash 5.3 or qwen 3.8 get `replace` instead.
- Trace: modes and default https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/edit/settings.ts#L18-L56 ; resolution https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/utils/edit-mode.ts#L24-L49 .
- Edge cases: the mode is resolved per active model, so a fallback model mid-session can switch the edit format the model is told to use.
- Round 1: `corrected`. Round 1 treated hashline as the edit format; the code ships five and keeps a per-model blacklist, which says hashline is not universally better.
- For rizzy-harness: if we adopt hashline, keep search-and-replace as the fallback and make the choice per model.
- Confidence: `read the definition`. (checked in review)

#### `sloppy` is a Find/Replace/Insert format with a tolerant matcher
- What the code does: `*** Edit File:` sections with `*** Find`, then `*** Replace`, `*** Insert Before` or `*** Insert After`; `…` captures omitted text; each failing edit is rejected alone while the rest apply.
- Trace: prompt https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/prompts/sloppy.md#L1-L9 ; engine header https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/sloppy/mod.rs#L1-L3 .
- Round 1: `new (not documented)`; answers the Round 2 question "what `sloppy` is".
- For rizzy-harness: partial application ("the rest apply") is the opposite of hashline's atomic staging; pick one rule and state it.
- Confidence: `read the definition`.

#### `replace` mode: an exact-then-fuzzy ladder, and "read before edit" is prompt only
- What the code does: exact match first; then trailing-trim (0.99), full trim (0.98), comment-prefix (0.975), Unicode normalisation (0.97) without fuzzy; with fuzzy on (default, threshold 0.95) also prefix and substring passes; several fuzzy hits above threshold are accepted only if the best is at least 0.97 and 0.08 above the second. The replace engine ignores the `EditStore`, so the prompt's "Tool errors on edit before read" is not enforced there.
- Trace: `find_match` https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/fuzzy.rs#L712-L750 ; ladder https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/fuzzy.rs#L836-L891 ; fuzzy defaults https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/edit/settings.ts#L75-L111 ; `_store` unused in `stage` https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/replace.rs#L177-L195 ; prompt claim https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/prompts/replace.md#L16-L18 .
- Edge cases: I did not find a read-before-edit check elsewhere in the TS edit wrapper (searched `packages/coding-agent/src/edit` and `tools`), so the claim stands as prompt-only unless it lives in a path I did not read.
- Round 1: `new (not documented)`.
- For rizzy-harness: our edit tool should enforce read-before-edit in code, not in the tool description.
- Confidence: `read the definition`. (checked in review)

### TTSR (stream rules)

#### Rule matching: JS regex over per-stream buffers, incremental by line, thinking off by default
- What the code does: rules carry `condition` (JS `RegExp`, inline flag prefix translated), `astCondition` (ast-grep, edit/write only), optional `question`, `scope` and `interruptMode`. Each stream (text, thinking, each tool call) has a `StreamBuffer` that scans only from the start of the last unterminated line for line-local patterns and rescans the whole buffer geometrically for cross-line ones; a monotonic match is "settled". Buffers reset at every assistant message start. Default scope allows text and any tool but NOT thinking.
- Trace: rule fields https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/capability/rule.ts#L61-L76 ; compile https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/capability/rule.ts#L364-L371 ; default scope https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/ttsr.ts#L154-L159 ; buffer https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/ttsr.ts#L415-L486 ; per-delta dispatch https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/ttsr-coordinator.ts#L140-L183 ; match loop https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/ttsr.ts#L1050-L1084 .
- Edge cases: an invalid regex is logged and skipped (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/ttsr.ts#L600-L609). Rules come from project files and run as backtracking JS regex on the event loop against model output, so a catastrophic pattern in a cloned repo can stall the session (inferred; no timeout found). For hashline tool calls the matcher sees only the added `+` rows per file (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/mod.rs#L32-L55).
- Round 1: `confirmed` for fields; `new` for the default excluding thinking and the per-file added-lines digest.
- For rizzy-harness: run rule regexes with Rust's `regex` crate (linear time) on the node; that removes the ReDoS class outright.
- Confidence: `traced end to end`.

#### The abort, the retry, and why it terminates
- What the code does: on a match whose interrupt mode applies (`always` default; `prose-only`, `tool-only`, `never` per rule), the coordinator aborts the agent (tool-scoped when the match is inside a tool call), and 50 ms later, if nothing superseded it, drops the aborted assistant message when `contextMode` is `discard` (default), appends a hidden `ttsr-injection` custom message wrapping the rule in `<system-interrupt reason="rule_violation">`, persists it, marks the rule injected, and schedules a continue. There is no retry counter: termination comes from `repeatMode: once` (default), under which an injected rule never fires again in the session; `after-gap` allows a repeat after `repeatGap` (10) turns.
- Trace: interrupt decision https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/ttsr-coordinator.ts#L495-L507 ; abort and retry https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/ttsr-coordinator.ts#L678-L756 ; defaults https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/ttsr.ts#L137-L146 ; repeat gate https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/ttsr.ts#L586-L598 ; template https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/prompts/system/ttsr-interrupt.md#L1-L11 .
- Edge cases: a stale retry (prompt generation changed, retry token bumped, or the target message gone) clears pending injections and resolves the resume gate (L707-L719). The matched excerpt is JSON-quoted, harness tags escaped, and labelled as untrusted data in the template. Non-interrupting matches inside a tool call become a reminder attached after that tool result (`afterToolCall`, https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/ttsr-coordinator.ts#L289-L293). AST rules run on finalised arguments in `beforeToolCall` and block the tool (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/ttsr-coordinator.ts#L187-L200).
- Round 1: `confirmed` (abort-and-regenerate, fire-once, `repeatMode`); `new` for interrupt modes, tool-scoped abort and the absence of a retry cap.
- For rizzy-harness: implementable as a node-side stream filter; the loop bound must be explicit in our version (fire-once per session is the bound here).
- Confidence: `traced end to end`.

#### Judged rules: an LLM judge on completed output, warning only
- What the code does: a rule with `question` never matches mid-stream; after a message completes (not aborted or errored), one judge request per output asks all eligible questions (content capped at 32,000 tokens); a yes delivers a non-interrupting warning. The run waits at most 5 s for pending judgments before yielding.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/ttsr-coordinator.ts#L564-L613 ; settle timeout https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/ttsr-coordinator.ts#L42-L43 ; cap https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/ttsr.ts#L64-L64 .
- Edge cases: judge failures are logged and dropped; verdicts from an older session generation are discarded.
- Round 1: `new (not documented)`.
- For rizzy-harness: a costed feature; skip until rules exist.
- Confidence: `read the definition`.

#### What survives compaction and restart: the fire-once set, not the rule text
- What the code does: injected rule names are appended as `ttsr_injection` session entries and rebuilt by scanning the whole branch path, compaction entries included, so fire-once state survives compaction and resume. On restore, records get `lastInjectedAt: 0`. The injected rule TEXT is an ordinary hidden custom message and goes wherever compaction sends old messages.
- Trace: persistence https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/ttsr-coordinator.ts#L455-L462 ; rebuild over the path https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L368-L374 ; restore https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/ttsr.ts#L1128-L1136 .
- Edge cases: after compaction, a `once` rule is suppressed for the rest of the session even though the model may no longer have its text in context (inferred: I found no compaction step that re-injects or pins `ttsr-injection` messages).
- Round 1: `corrected`. The README claim "injections survive compaction" is true for the suppression state; the rule body is not pinned.
- For rizzy-harness: if we copy TTSR, either pin injected rules through compaction or reset fire-once state on compaction.
- Confidence: `traced end to end` for state; `inferred` for the text.

### Compaction

#### The method chain, its default order, and what makes a method unusable
- What the code does: `compaction.methodOrder` default is `remote, snapcompact, handoff, shake, soft`. A method is skipped when unusable for the reason: `remote` needs a configured endpoint or a native route (OpenAI Responses compact, Anthropic compaction beta); `snapcompact` needs a model that sends image input and is excluded for byte or media 413s; `handoff` is never used for `overflow`. A remote method that just failed speculatively is skipped if a later method can run.
- Trace: choices and default https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/compaction-methods.ts#L11-L51 ; usability https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L203-L217 ; selection loop https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L5237-L5293 .
- Edge cases: an empty or all-unusable order returns "none", or blocks automatic continuation on a threshold pass after a fallback (L5289-L5293).
- Round 1: `confirmed` (order) and `new` (usability rules).
- For rizzy-harness: a configured, ordered method list with explicit per-reason usability is a clean design; copy the shape.
- Confidence: `traced end to end`. (checked in review)

#### How `methodOrder` falls through: recursion with `methodIndex + 1`
- What the code does: every failure path calls `runAutoCompaction` again with `methodIndex + 1`: shake that reclaims too little; handoff that returns no document; any snapcompact blocker (unrenderable glyphs, no frame budget, over the per-request image byte budget, no reduction, still over the window, or no headroom under the trigger while a later summarising method exists); a summary that would not reduce context; and any thrown error before commit. Abort ends the chain.
- Trace: shake https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L5309-L5326 ; handoff https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L5636-L5655 ; snapcompact blockers https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L5677-L5732 and fallthrough https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L5844-L5861 ; no reduction https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L6116-L6169 ; thrown error https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L6201-L6242 .
- Edge cases: if snapcompact leaves no headroom but no later summariser is usable, it keeps the archive with a warning (L5829-L5839). A speculative background summary, when armed and still valid, is spliced in instead of a blocking call, except for snapcompact (L5301-L5302).
- Round 1: `new (not documented)`; answers Round 2 question 6 ("how methodOrder falls through").
- For rizzy-harness: "does it actually reduce context?" as a gate before committing is the detail most harnesses miss; copy it.
- Confidence: `traced end to end`.

#### Triggers and thresholds
- What the code does: compact when context tokens exceed the threshold; the threshold is `thresholdTokens` if set, else a percentage, else the window minus the reserve; the reserve is max(15% of window, `reserveTokens` default 16,384), with a proportional fallback for small windows. The trigger token count is floored by a local estimate of the stored conversation so on-wire compression cannot hide growth. Summaries are capped at 16,384 tokens. A 0.8 hysteresis band defines "pressure resolved". Mid-turn checks and async speculation are on; idle compaction is off (200,000 tokens, 300 s when on).
- Trace: constants https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/compaction/compaction.ts#L193-L235 ; reserve https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/compaction/compaction.ts#L324-L353 ; trigger and floor https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/compaction/compaction.ts#L355-L381 ; threshold resolution https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/compaction/compaction.ts#L394-L407 ; settings https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/context-settings.ts#L130-L156 and https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/context-settings.ts#L282-L313 .
- Round 1: `confirmed` (reserve, 20k keep-recent, idle off, mid-turn on).
- For rizzy-harness: the "floor by stored estimate" rule matters for us if the node ever compresses requests; copy.
- Confidence: `traced end to end`.

#### Snapcompact: older history rendered to PNG frames of pixel-font text, no LLM call
- What the code does: the discarded messages are serialised (tool results capped at 2,000 chars, argument values at 500, a call at 2,000, 60% head and 40% tail), normalised, folded together with any previous archive's source text, laid out into at most 80 frames, and rendered natively to PNG with a per-provider shape (font, cell, ink). The summary message is a template explaining how to read the frames, plus a FILES list; frames live in the compaction entry's `preserveData` and are reattached on each context rebuild.
- Trace: serialisation caps https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/snapcompact/src/snapcompact.ts#L807-L819 ; `compact` https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/snapcompact/src/snapcompact.ts#L2494-L2568 ; frames and summary https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/snapcompact/src/snapcompact.ts#L2570-L2655 ; reading instructions https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/snapcompact/src/prompts/snapcompact-summary.md#L1-L20 ; native renderer https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-natives/src/snapcompact.rs#L1067-L1100 .
- Edge cases: re-compaction re-renders the old archive's text rather than stacking images; archives without stored text lose their frames and count as truncated; data URLs are elided before layout; a reader that drops reasoning gets a re-rendered "without thinking" view, falling back to text without frames when that render fails (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L5126-L5155).
- Round 1: `confirmed` with detail (Round 1 said "archived as dense images by a vision model"; the vision model only reads; rendering is local).
- For rizzy-harness: it is lossy (tool output truncated to 2k before rendering) and provider-specific; interesting, but it violates our "harness stays light" spirit only mildly (pure Rust raster). Skip for now; revisit with measurements.
- Confidence: `traced end to end`.

#### Which model runs a summarising compaction, and when it falls back
- What the code does: candidates in order are the model's configured `compactionModel`, the current model, each chat role's resolved model, then the single largest-context available model. Candidates without an API key are skipped; an auth failure moves on; a provider-native compaction error moves on only within the same provider, otherwise it is rethrown; any other error is thrown (and then the method chain falls through).
- Trace: candidates https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L3708-L3752 ; loop https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L3767-L3852 ; `compactionModel` resolution https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/role-models.ts#L64-L67 .
- Round 1: `new (not documented)`.
- For rizzy-harness: matches our planned "summariser slot defaults to the main model"; their order (configured, current, roles, largest) is a reasonable default.
- Confidence: `traced end to end`.

#### Recovery from a context-length error: promotion first, then compaction without handoff
- What the code does: on overflow evidence from the same model, the failed assistant turn is removed from active context; if `contextPromotion.enabled` (default false) and the model has a `contextPromotionTarget`, the session switches model and retries without compacting; otherwise it runs recovery compaction with reason `overflow` (handoff excluded) and, for byte or media 413s, snapcompact excluded.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L3223-L3246 ; promotion default https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/context-settings.ts#L76-L87 ; overflow is not a retry case https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/turn-recovery.ts#L1584-L1587 .
- Round 1: `confirmed` (`contextPromotionTarget`).
- For rizzy-harness: keep overflow handling separate from transport retry, as here.
- Confidence: `traced end to end`.

#### Pruning of old tool output every turn
- What the code does: before any summarising method, `supersedeReads` (default on) prunes older read results of a file read again, and `dropUseless` (default on) prunes results flagged useless once consumed; both are cache-aware (they leave the warm provider prefix alone until the cache is cold) and protect plan and skill reads; the pruned branch is persisted.
- Trace: settings https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/context-settings.ts#L359-L382 ; prune pass https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-maintenance.ts#L817-L852 ; plan protection https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/plan-mode/plan-protection.ts#L16-L32 .
- Round 1: `confirmed` ("pre-step prunes superseded reads").
- For rizzy-harness: "supersede an older read of the same file" fits our event-log fold; copy, and keep it prompt-cache aware.
- Confidence: `read the definition`.

### Model roles, aliases, fallback chains, retry

#### There are 16 built-in roles, not ten or nine
- What the code does: `MODEL_ROLE_IDS` lists 16: chat roles `default, smol, slow, vision, plan, commit, tiny, memory, task, advisor, widget` (11) and model-kind roles `image, web, speech, dictation, judge` (5). `designer` is not built in; any configured role name is accepted as a custom role.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/tui/src/overlays/model-browser.ts#L61-L109 ; role metadata https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/model-roles.ts#L56-L91 ; custom roles https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/model-roles.ts#L136-L155 .
- Round 1: `refuted`. Round 1: ten roles (docs) including `designer`, nine per README. Code: 16 built in, no `designer`.
- For rizzy-harness: we need three slots, not sixteen; the lesson is "roles grow", so key them by string with a known set rather than an enum baked into the protocol.
- Confidence: `read the definition`. (checked in review)

#### `@role` aliases and how an empty `modelRoles` auto-resolves
- What the code does: `@role`, legacy `pi/role` and `*` (default) are aliases; a pattern list may be comma-separated and each entry may carry `:level`. Resolution: a configured role expands its patterns recursively (cycles fall back to the role's built-in defaults); an unconfigured role uses a configured fallback role (`advisor`→`slow` only if `slow` is configured; `memory`→`tiny`; `tiny`, `widget`→`smol`); `smol` and `slow` inherit a configured `default` before their priority list; otherwise the catalog's built-in priority chain (`role-defaults` in `behavior.kdl`, e.g. a list of haiku, gemini flash, spark ids for `smol`) is matched against authenticated models.
- Trace: alias parsing https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/model-resolver.ts#L1113-L1133 ; priority aliases and the advisor rule https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/model-resolver.ts#L1168-L1201 ; recursive resolution https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/model-resolver.ts#L1260-L1307 ; built-in chains https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/catalog/src/compat/rules/runtime/behavior.kdl#L421-L462 ; `default` role falls back to the current model https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/role-models.ts#L70-L88 .
- Edge cases: the comment in the source states why advisor will not inherit default: "so it stays a distinct strong model out of the box" (L1174-L1176).
- Round 1: `confirmed` (aliases, "advisor will not inherit default"); answers the Round 2 question on the auto-resolution algorithm.
- For rizzy-harness: a hard-coded catalogue of model names per role ages weekly (their list already names models by version); prefer explicit configuration with "default to the main model".
- Confidence: `traced end to end`.

#### `retry.*` settings and the backoff constants
- What the code does: `retry.maxRetries` 10, `baseDelayMs` 500, `maxDelayMs` 300,000 (a provider wait above it fails fast unless `waitForUsageReset`), `modelFallback` true, `fallbackOn` `any`, `rateLimitWalksChain` false, `usageAwareFallback` false, `usageReservePolicy` `confirm`, `fallbackRevertPolicy` `cooldown-expiry`. The backoff is `base × 2^(attempt-1)` capped at 8,000 ms with up to 25% downward jitter.
- Trace: settings https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/settings.ts#L721-L919 ; backoff https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/retry-fallback-chains.ts#L89-L97 .
- Round 1: `confirmed` (usageAwareFallback false, cooldown-expiry) and answers "the backoff constants in the `retry.*` namespace".
- For rizzy-harness: copy the fail-fast cap on provider-requested waits; a 3-hour sleep in a node is a stuck session.
- Confidence: `read the definition`. (checked in review)

#### Fallback chains: key resolution by specificity
- What the code does: `retry.fallbackChains` keys are roles, exact `provider/model[:level]`, `provider/*` or `provider/prefix/*`. For the failing selector the key is chosen as: exact model key (exact effort, then normalised effort, then suffixless), then the longest matching wildcard, then the hinted role, then a role whose assigned model matches (`default` preferred), then `default`. A configured `default` chain is copied to every named role without its own chain. Wildcard entries keep the failing model id and swap the provider.
- Trace: key resolution https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/retry-fallback-chains.ts#L429-L537 ; default expansion https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/retry-fallback-chains.ts#L159-L178 ; entry parsing https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/retry-fallback-chains.ts#L539-L551 .
- Round 1: `confirmed` with the precise precedence added.
- For rizzy-harness: a per-slot ordered list is enough for us; skip wildcards.
- Confidence: `read the definition`.

#### What triggers a fallback, the cooldown, and the walk bound
- What the code does: only retryable errors enter the saga (`stopReason: error`, not context overflow, and no replay-unsafe committed output unless every tool call provably never ran). A fallback hop needs `modelFallback`, `fallbackOn` admitting the error class, and not a stale usage limit, thinking loop or first-attempt socket drop. The failing selector is suppressed for the provider's retry-after, or a reason-derived backoff, or 5 minutes when unknown. A switched model gets a fresh retry budget; classifier refusals visit each model once per turn so A→B→A chains terminate.
- Trace: retryable https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/turn-recovery.ts#L1566-L1605 ; admission https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/retry-fallback-chains.ts#L356-L366 ; hop decision https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/turn-recovery.ts#L3005-L3069 ; cooldown https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/turn-recovery.ts#L1901-L1909 ; budget reset https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/turn-recovery.ts#L3090-L3107 .
- Edge cases: by default a rate-limited turn whose thinking is signed by the failing model keeps retrying that model until the budget is spent before walking the chain (`rateLimitWalksChain` false).
- Round 1: `new (not documented)` beyond the setting names.
- For rizzy-harness: "never replay a turn that already executed a tool" is the invariant to copy; it is what makes fallback safe.
- Confidence: `traced end to end` for the trigger; `read the definition` for the walk.

### Sub-agents and orchestration

#### Bundled agents are six: scout, reviewer, security-reviewer, seance, task, sonic
- What the code does: bundled definitions are embedded Markdown with YAML frontmatter. `scout` (read, find, grep, glob, ast_grep, web_search; `@smol`), `reviewer` (adds bash and lsp, spawns scout; `@slow`), `security-reviewer` (read-only tools, no model), `seance` (read a historical session; read, grep, glob), `task` (`@task`, spawns `*`, auto thinking) and `sonic` (`@smol`, medium thinking, same body as task). Agents may declare a JSON-Typedef-like `output` schema.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/agents.ts#L45-L74 ; scout https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/prompts/agents/scout.md#L1-L7 ; reviewer https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/prompts/agents/reviewer.md#L1-L6 ; security-reviewer https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/prompts/agents/security-reviewer.md#L1-L4 ; seance https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/prompts/agents/seance.md#L1-L5 .
- Round 1: `corrected`. Round 1 listed `designer` and `librarian`; neither is bundled at this commit; `seance` is missing from Round 1.
- For rizzy-harness: the reviewer's structured verdict (`overall_correctness`, `confidence`) is a good pattern for our review routine.
- Confidence: `read the definition`. (checked in review)

#### The context a child gets: role prompt plus a parent-written `context`, never the parent conversation, never AGENTS.md
- What the code does: the task tool takes `{context, tasks[]}` (batch) or one task, each with `agent`, `task`, `solutionSpace`, optional `outputSchema`, `tools`, `isolated`. The child's system prompt is the normal default prompt blocks followed by a sub-agent block: the agent body under "Role", the shared `context`, an approved plan if any, worktree notice, IRC peer roster, and the yield protocol. The child is created with `excludeAgentsMd: true`, so files named `AGENTS.md` are dropped (other context files such as `CLAUDE.md` still pass through).
- Trace: schema https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/types.ts#L77-L145 ; prompt composition https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L4139-L4176 ; template https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/prompts/system/subagent-system-prompt.md#L1-L25 ; AGENTS.md exclusion https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L4993-L4996 and filter by basename https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/system-prompt.ts#L357-L360 .
- Edge cases: per-spawn text is placed after the static `<project-context>` block so sibling spawns share a cached prefix (L4171-L4173).
- Round 1: `confirmed` ("workers get no parent conversation"); AGENTS.md exclusion is `new`.
- For rizzy-harness: excluding by file NAME is inconsistent (CLAUDE.md passes); if we exclude instruction files for children, do it by role, not by basename.
- Confidence: `traced end to end`. (checked in review)

#### Tools, depth, approvals: the child's capability boundary
- What the code does: the agent's `tools` frontmatter is the list; `task` is auto-added when `spawns` is set and removed at max depth; `exec` expands to bash plus eval; `wait` is added when the child can start background work. Depth: child depth = parent + 1, and at `childDepth >= maxRecursionDepth` (default 2) the child cannot spawn. Approval mode is forced to `yolo` for every child (parent `task` approval is the boundary), user `tools.approval` deny/prompt rules still apply; advisors are off unless the agent opts in.
- Trace: tool list and depth https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L4543-L4586 ; yolo override https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L1259-L1265 ; depth helper https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/types.ts#L319-L325 ; default depth https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/settings.ts#L473-L491 .
- Edge cases: an agent without `tools` gets the full default set (the `if (agent.tools)` branch is skipped), so the bundled `task` agent has everything.
- Round 1: `confirmed` (yolo, depth 2).
- For rizzy-harness: a child must not get wider authority than its parent's current approval state; their yolo-for-children is exactly what not to copy.
- Confidence: `traced end to end`. (checked in review)

#### Isolation, parallelism and budgets
- What the code does: isolation is opt-in (`task.isolation.enabled` false; per-task `isolated: true`), backend `auto` picks ovnfs if found, else a platform clone backend (APFS, btrfs, zfs, reflink, overlayfs, ProjFS, block clone, rcopy). The child's delta is captured as a git patch against a baseline and applied to the parent through a temporary index: plain apply, then `--3way`, then replaying the parent's WIP first; failure throws. Concurrency 32; up to 8 children on the main thread, then worker threads (default up to 4); soft request budget 200 with forced stop at 1.5×; idle children parked after 420,000 ms; no wall-clock cap by default.
- Trace: isolation settings https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/settings.ts#L27-L79 ; delta capture https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/worktree.ts#L411-L444 ; apply ladder https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/worktree.ts#L781-L829 ; concurrency and threads https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/settings.ts#L404-L458 ; runtime, idle and budget https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/settings.ts#L493-L548 .
- Edge cases: isolated runs get `workspace.additionalDirectories: []` so no root outside the worktree is exposed (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L4534-L4535). All of this is filesystem isolation in one process, not a security boundary.
- Round 1: `confirmed` with correction (the setting is `task.isolation.enabled`, not `task.isolation.mode`).
- For rizzy-harness: patch-back through a temporary index without touching the checkout is the safe way to merge child work; copy the mechanism if we add isolated children.
- Confidence: `read the definition`. (checked in review)

#### How results return: the `yield` tool
- What the code does: a structured child must call `yield` to finish; incremental `type: [..]` sections accumulate, a terminal call ends the run, and without `data` (unstructured) the last assistant turn becomes the result. Workpool mode yields once per numbered item. A yield invalidated by a later background result must be redone.
- Trace: protocol https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/prompts/system/subagent-system-prompt.md#L54-L75 ; monitor contract https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L1340-L1372 .
- Round 1: `corrected`. Round 1 noted the README's "schema-validated typed results" versus docs; the code validates against the agent or caller `outputSchema` through `yield`.
- For rizzy-harness: an explicit "finish" tool beats parsing the last message; copy.
- Confidence: `read the definition`.

### System prompt assembly and instruction files

#### The system prompt is an ordered array of blocks from Handlebars templates
- What the code does: block 0 is the bundled `system-prompt.md` (or a discovered template, or `custom-system-prompt.md` when a custom prompt is set) rendered with tools, skills, rules, environment, personality and feature flags; then eval-prelude guidance; then one trailing `<project-context>` block (context files, workspace tree, active repo, append text) so the static prefix caches across directories. Each preparation step has a deadline and falls back to a minimal default on timeout. A literal `SYSTEM.md` beats a template; project beats user.
- Trace: render and blocks https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/system-prompt.ts#L1049-L1090 ; data fed to the template https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/system-prompt.ts#L998-L1048 ; timeouts https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/system-prompt.ts#L886-L912 ; override discovery https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/system-prompt.ts#L261-L286 .
- Edge cases: an invalid discovered template is ignored with a warning, an explicit one is fatal (L1055-L1065).
- Round 1: `confirmed` (`SYSTEM.md`, `APPEND_SYSTEM.md`).
- For rizzy-harness: "static first, cwd-specific last" is the cache rule to copy.
- Confidence: `traced end to end`.

#### Reading other harnesses' instruction files: one file per depth, priority shadowing, paragraph dedupe
- What the code does: context-file providers register with priorities (native 100, Claude `.claude/` 80, `.agent(s)` 70, Codex 70, Gemini 60, OpenCode 55, GitHub 30, standalone `AGENTS.md` 10, standalone `CLAUDE.md` 10). The capability key is "user" or `project:<depth>`, so per depth one file wins; configured extras are additive. Files are sorted farthest first and a file is dropped if a closer one contains all its paragraphs. `@path` imports are expanded. Walk-up stops at the repo root or home.
- Trace: key https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/capability/context-file.ts#L32-L46 ; priorities https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L42-L45 , https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/agents-md.ts#L24-L29 , https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude-md.ts#L24-L30 ; walk-up and boundary https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/helpers.ts#L723-L763 ; dedupe and imports https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/system-prompt.ts#L336-L401 .
- Edge cases: `AGENTS.md` and standalone `CLAUDE.md` tie at 10, so in a repo that has both at the root (like ours) only one is loaded; empty files do not claim the slot (helpers L750-L753). The user-level `~/.claude` tree is opt-in (https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L47-L69).
- Round 1: `corrected`. Priority order confirmed; Round 1's "Claude project file is `.claude/CLAUDE.md` in the launch dir only" misses the standalone root `CLAUDE.md` walk-up provider.
- For rizzy-harness: shadowing silently drops a file; if we read foreign instruction files, show which file won per directory.
- Confidence: `traced end to end`.

#### Foreign MCP and agent configs
- What the code does: the Claude provider reads `mcpServers` from the user `~/.claude.json` and `~/.claude/mcp.json` and the project `.claude/.mcp.json` or `.claude/mcp.json`, expanding `${VAR}` environment references in every string (commands, args, env, URL, headers); project entries load first so a project `enabled: false` wins. `mcp.enableProjectConfig` defaults to true. `.claude/agents` are deliberately not read as task agents.
- Trace: https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude.ts#L86-L153 ; env expansion https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/helpers.ts#L573-L588 ; default https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/mcp/settings.ts#L10-L12 ; agents skipped https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/discovery.ts#L13-L16 .
- Edge cases: a cloned repo's project MCP file can name a URL and put `${SOME_API_KEY}` in a header; expansion happens at discovery (inferred risk; whether the server is then connected without a prompt is in the sessions cluster).
- Round 1: `confirmed` (`.claude/agents` not scanned); env expansion is `new`.
- For rizzy-harness: never expand environment variables into a repo-supplied config; that is our "no secret leaves via config" invariant.
- Confidence: `read the definition`. (checked in review)

### Plan mode

#### Plan mode guards write and edit in code; bash is prompt-only
- What the code does: `enforcePlanModeWrite` rejects renames, deletes and any write outside the session `local://` sandbox, resolving symlinks and hashline headers so the guard and the write agree. The plan-mode prompt tells the model not to run state-changing commands; I found no plan-mode check in the bash tool.
- Trace: guard https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/tools/plan-mode-guard.ts#L89-L116 ; prompt https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/prompts/system/plan-mode-active.md#L1-L10 .
- Edge cases: searched `tools/bash*.ts` and `tools/index.ts` for plan state: only the type declaration matches.
- Round 1: `new` (answers "is plan mode read-only by tool removal?": no, by a write guard plus prompt).
- For rizzy-harness: a plan mode that leaves bash open is advisory; ours should drop exec or require approval.
- Confidence: `read the definition` (bash absence `inferred` from search). (checked in review)

## 4. Round 1 claims checked

| Claim (Round 1) | Verdict | Citation |
|---|---|---|
| Every line tagged with a short content hash (`2:f1|...`) | refuted (file tag + bare numbers) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/store.rs#L70-L91 |
| Changed file → edit rejected before corruption | corrected (rebased when provably safe) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-edit/src/modes/hashline/recovery.rs#L503-L565 |
| `sloppy` alternate edit tool exists | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/edit/settings.ts#L18-L24 |
| Ten roles incl. `designer` (README nine) | refuted (16, no designer) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/tui/src/overlays/model-browser.ts#L78-L109 |
| Advisor will not inherit default | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/config/model-resolver.ts#L1172-L1201 |
| `fallbackRevertPolicy` cooldown-expiry; `usageAwareFallback` false | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/settings.ts#L823-L834 |
| Compaction default order remote, snapcompact, handoff, shake, soft | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/compaction-methods.ts#L44-L51 |
| Reserve max(16,384, 15%); keepRecent 20,000; idle off | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/agent/src/compaction/compaction.ts#L328-L330 |
| Snapcompact "by a vision model" | corrected (rendered locally; the vision model only reads) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/crates/pi-natives/src/snapcompact.rs#L1067-L1073 |
| TTSR abort, inject, regenerate; fire-once default | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/export/ttsr.ts#L137-L146 |
| TTSR injections survive compaction | corrected (suppression state survives; rule text not pinned) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/session/session-context.ts#L368-L374 |
| Bundled agents include `designer`, `librarian` | refuted (six: scout, reviewer, security-reviewer, seance, task, sonic) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/agents.ts#L45-L74 |
| Limits 32 concurrency, depth 2, budget 200 (1.5× hard), idle 7 min, runtime 0 | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/settings.ts#L404-L548 |
| Subagents run yolo but keep `tools.approval` | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/executor.ts#L1259-L1262 |
| Subagent process launch overridable via `PI_SUBPROCESS_CMD` | not in code (no match in `packages/coding-agent/src`; children are in-process) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/settings.ts#L434-L458 |
| `task.isolation.mode` default none | corrected (`task.isolation.enabled` false, `isolation.backend` auto) | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/settings.ts#L27-L46 |
| `.claude/agents` not scanned | confirmed | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/task/discovery.ts#L13-L16 |
| Instruction priority native > claude > agents = codex > gemini > opencode > github > agents-md | confirmed; standalone `CLAUDE.md` provider missing from Round 1 | https://github.com/can1357/oh-my-pi/blob/e325ffc73ab86e03bdd20a91b77d8e2b3964c906/packages/coding-agent/src/discovery/claude-md.ts#L24-L30 |

## 5. In the code but not in the documentation

- The seen-line guard on hashline edits and its reveal-on-reject reply (patcher.rs L424-L527).
- Stale-tag recovery with uniform-offset, neighbour, syntax-context and "authored lines" checks (recovery.rs).
- Per-model hashline downgrade list (edit-mode.ts L33-L46).
- Three-strike no-op edit loop guard (store.rs L428-L439).
- `replace` read-before-edit is prompt-only in the Rust engine.
- TTSR judged rules (LLM judge, warning only), interrupt modes, thinking excluded by default, tool-scoped aborts.
- Compaction "would it reduce context?" gate and the shake/snapcompact/handoff fallthrough conditions.
- Trigger floor by stored-conversation estimate (compaction.ts L364-L381).
- Compaction model candidate order (configured, current, roles, largest window).
- Six roles beyond the docs (`memory`, `widget`, `image`, `web`, `speech`, `dictation`, `judge`).
- Sub-agents never get `AGENTS.md`, but do get other context files.
- `seance` bundled agent; worker-thread hosting of sub-agents above eight.
- `${VAR}` expansion in foreign MCP configs.

## 6. Could not find or could not settle

- **Hashline benchmark claims**: not tested; needs running `packages/typescript-edit-benchmark` (not allowed here).
- **Live-tag collision behaviour**: inferred from `apply_with_recovery`; a test with two texts sharing a 16-bit xxh32 tag would settle it.
- **Whether `ttsr-injection` messages are pinned through compaction**: I searched `packages/agent/src/compaction` and found only the entry type and branch-summary handling; reading `prepareCompaction` and `tool-protection.ts` fully would settle it.
- **Memory (`local` distillation, `learn`/`manage_skill` approval)** and **advisor blocker interrupt**: not traced for lack of time; `packages/coding-agent/src/memories`, `autolearn` and `advisor` are the entry points.
- **Post-edit LSP diagnostics and format-on-write**: not traced; start at `packages/coding-agent/src/edit/index.ts`.
- **Plan mode and bash**: absence of a bash guard is from a name search; reading `tools/bash.ts` in full would settle it.
- **Usage-aware fallback internals** (quota reading, reserve) and round-robin credentials: settings read, mechanism not traced (`turn-recovery.ts` usage-limit paths).

## 7. Citation check

`research/round2/oh-my-pi.agents.md: 175 citations valid (0 without a line range), 0 invalid, 0 GitHub links not pinned to a commit`

## 8. Review

Independent review against the clone at the pinned SHA. Checked 17 of 35 findings, covering all the Round 1 `refuted`/`corrected` items the brief named (hashline, rebase, roles, sub-agent yolo) plus the negative claims (no designer role, no read-before-edit in `replace`, no plan-mode check on bash).

| Finding | Verdict | Reason |
|---|---|---|
| Hashline is a per-FILE tag, not per line | supported | `store.rs` L70-L91 `file_hash` = `Xxh32(seed 0)` over whole text, trailing blanks stripped, `& 0xffff`, `{:04X}`; `Anchor { line }` only; row format `N:text` |
| Tags bound to a retained snapshot | supported | limits 256 paths, 4 versions, 64 MiB, 4 MiB at `store.rs` L13-L24 (read.rs call sites not re-read) |
| Seen-line guard on live tag | supported | `patcher.rs` L626-L640; `edit.enforceSeenLines` default true |
| Stale tag rebase | partly | rebase exists (`try_recover`, `patcher.rs` L674-L705) but the parse-break refusal applies only when offset != 0; fixed |
| Collisions and unknown tags | supported | `live_matches` bypasses recovery (`patcher.rs` L610-L626); `by_hash` returns first (newest) match |
| No-op escalation after three | supported | `NOOP_HARD_LIMIT` 3; counted only when the patch has a single section (small omission) |
| Five edit modes, per-model downgrade | supported | `edit-mode.ts` L24-L49 matches the class list; `PI_EDIT_VARIANT` and `PI_STRICT_EDIT_MODE` also bypass it |
| `replace` mode read-before-edit prompt-only | absence confirmed | `_store` unused in `replace.rs` `stage`; no read tracker or "not read" error in `edit/`, `pi-edit/src`, `tools/write.ts` |
| Compaction method order | supported | `compaction-methods.ts` default order matches |
| `retry.*` settings and backoff | supported | defaults 10, 500, 5 min, 8 000 ms cap, 25% jitter |
| 16 built-in roles, no `designer` | supported | `model-browser.ts` L61-L109 lists exactly 16; `git grep -i designer` in coding-agent, tui, catalog src finds nothing |
| Bundled agents are six | supported | `task/agents.ts` L45-L74 |
| Child context, AGENTS.md excluded | supported | `executor.ts` L4993-L4996, `system-prompt.ts` L357-L360 (filters by basename, so `CLAUDE.md` passes) |
| Child tools, depth, yolo | supported | `executor.ts` L1259-L1278, L4543-L4586; per-spawn `overrides` spread after the yolo key can change it |
| Isolation, parallelism, budgets | supported | defaults 32, 4 workers (8 on main thread per the setting text), 2, 420 000, 200, `task.isolation.enabled` false; 1.5x hard stop not re-read |
| Foreign MCP and agent configs | supported | `claude.ts` L86-L153 reads `.claude/.mcp.json`/`.claude/mcp.json`, `expandEnvVarsDeep` expands every string; `.claude/agents` skipped per `task/discovery.ts` L10-L17 |
| Plan mode, bash prompt-only | absence confirmed | entering plan mode keeps previous tools (`interactive-mode.ts` L6124 keeps all but `write`); `getPlanModeState` is read only by the write guard, ask, and similar, not bash |

Not checked: TTSR matching, abort and judge findings (only the default scope, settings and repeat defaults were glanced at), snapcompact, compaction triggers, fallback chains, recovery from context-length errors, pruning, system-prompt assembly, instruction-file shadowing, `yield`, sloppy mode, the edit grammar and the "Round 1 claims" table rows not named above.

Small inconsistency found: section 5 says "Six roles beyond the docs" but lists seven names.

Overall: the central claim holds. "Hashline" hashes the whole file into a single 4-hex tag (low 16 bits of xxh32), anchors are bare line numbers, and the seen-line guard is the real per-line protection; a stale tag is rebased only under uniform-offset and context checks. The role count (16, no designer) and the sub-agent yolo claim are correct. The unchecked half is mostly TTSR and compaction mechanics, each cited to specific lines and worded with the same care as the checked part; the checked half showed one wording slip, so moderate-to-high trust, with the unchecked numbers worth a spot check before being quoted.
