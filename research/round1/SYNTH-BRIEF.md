# Round 1 synthesis brief (shared by every synthesis agent)

Research date: 2026-11-10. Seventeen harnesses were surveyed from public
sources and the surveys were reviewed. You consolidate ONE GROUP OF FEATURE
AREAS (named in your task) across all of them into a single cross-harness
reference. The reader is building their own self-hosted harness
("rizzy-harness": Rust, a web dashboard, a central "master" server plus
worker "nodes" that run the agent loop and tools) and will use your file to
decide what to copy, skip or do differently.

**This is synthesis only: do not browse the web and do not add facts that are
not in the input files.** Accuracy beats volume.

## Inputs (all in `research/round1/`, repository root
`/run/media/lebron/PROJ/Rust/rizzy-harness`)

Survey files, one per harness, in this canonical order and with these short
names (use exactly these names everywhere):

| Short name | File | Source |
|---|---|---|
| Codex | `codex.md` | open (CLI), closed (cloud, IDE, desktop) |
| OpenCodex | `opencodex.md` | open; a provider gateway, not a harness |
| OpenCode | `opencode.md` | open |
| Claude Code | `claude-code.md` | closed |
| Claude Desktop | `claude-desktop.md` | closed |
| DeepSeek harness | `deepseek-harness.md` | open, developer preview |
| Oh My Pi | `oh-my-pi.md` | open |
| Pi | `pi.md` | open |
| Cursor | `cursor.md` | closed |
| Antigravity | `antigravity.md` | closed |
| OpenHands | `openhands.md` | open core |
| OpenClaw | `openclaw.md` | open |
| goose | `goose.md` | open |
| Hermes Agent | `hermes-agent.md` | open |
| Zed/ACP | `zed-acp.md` | open (Zed), open protocol (ACP) |
| Warp | `warp.md` | open client, closed services |
| Cline | `cline.md` | open |

Also: `own-rizzy-harness.md` (what OUR harness has today, cited to code as
`path:line`), `00-targets.md` (identities), and for Claude Code only the
separate review files `claude-code.review-1.md`, `-2.md`, `-3.md` (their
"Corrections to apply" and "Move to Unverified" lists override the survey
text; the other survey files were corrected in place and end with a
`## 9. Review` section).

Each survey is organised by the same 23 areas (heading styles vary: find
your areas with grep, for example `grep -n -i "^#.*5\. Permissions"`).
`claude-code.md` is about 900 KB: never read it whole, read only your areas
by line range, and expect duplicate entries for one feature (merge them).
For every file also read section 3 "Distinctive", section 6 "Overhyped,
stale or contradictory", section 7 "Unverified" and the review section, and
take from them what belongs to your areas.

## How to consolidate

- **Merge features, never evidence.** If six harnesses have a capability,
  write ONE feature entry and give every one of the six its own row with its
  own mechanism and its own source URL. Do not replace six citations with
  one.
- **Preserve differences.** Similar names hide different mechanisms. When
  two harnesses do "the same thing" differently (who decides, where it
  runs, what the default is, what it fails to), that difference is the
  point: state it in the row and summarise it under "Variants".
- **Carry the trust labels.** For each row give a check value:
  `reviewed` (the entry says verified in review), `corrected` (corrected in
  review: use the corrected text), `unreviewed`, or `unverified` (the survey
  or review could not confirm it). Carry the status label too (GA, beta,
  experimental, preview, deprecated, planned, unclear).
- **Absence is weak evidence.** "Nothing documented" means the survey found
  no page, not that the feature does not exist. Never write "does not have";
  write "nothing documented".
- Copy URLs exactly as they appear. Do not invent or shorten them. Paraphrase;
  no long quotes.
- OpenCodex is a gateway and Zed/ACP is partly a protocol: include them where
  they have something real in your areas, not for symmetry.

## File to write

`research/round1/synth/<nn>-<slug>.md` (name given in your task; create the
directory). Write no other file. Structure:

1. **Scope**: the areas covered, files and line ranges read, anything you
   could not read.
2. **Features**: grouped under your areas, then one entry per feature:

   ```
   #### <feature name>
   What it is: one to three vendor-neutral sentences.
   Variants: the meaningful differences in mechanism across harnesses.

   | Harness | How it does it | Status | Evidence | Check |
   |---|---|---|---|---|
   | <short name> | specific, one or two sentences | GA | <URL> [, <URL>] | reviewed |
   | rizzy-harness (ours) | what we have, or "none" | works/partial/stub/none | `path:line` | from own inventory |
   ```

   Include the "ours" row for every feature, from `own-rizzy-harness.md`
   only (write "not in the inventory" if it says nothing).
3. **Availability matrix**: one table for your areas. Rows are the features,
   columns the seventeen short names plus `Ours`. Cells: `Y` documented and
   released, `B` beta/experimental/preview/partial, `P` planned or
   vendor-claim only, `-` nothing documented, `?` unverified. Put the legend
   above the table.
4. **Common across the ecosystem**: features that at least half of the real
   harnesses document, with a count.
5. **Only one harness has it**: single-harness features, with why each is
   interesting or not.
6. **Ours against the field**: what we have, what we lack and where we are
   ahead, for these areas only, ranked by how much it matters for a
   self-hosted master/node harness. Be blunt; cite the feature entries.
7. **Corrections, contradictions and unverified claims** in these areas:
   what the reviews changed, where a vendor's own pages disagree, and what
   nobody could confirm.
8. **Questions for Round 2**: for the open-source harnesses, the specific
   things in these areas that only source code can settle (how something is
   actually enforced, what a default really is, how a mechanism is built).
   Name the harness and the feature for each.

## Report back

Under 150 words: number of features, the three findings that matter most for
our harness, and any input file that was damaged, inconsistent or missing
your areas.
