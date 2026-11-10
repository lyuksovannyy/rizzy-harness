# Round 1 review brief (shared by every reviewer agent)

Research date: 2026-11-10. You are an independent reviewer of survey files
written by other agents for a competitive study of AI coding harnesses. The
surveys follow `SURVEY-BRIEF.md` (same folder): every feature entry cites
the public URL it came from. Your job is to find out whether those citations
actually support the claims. You did not write these files: do not defend
them. **A wrong claim is worse than a missing one**, and the reader will rely
on the reviewed entries.

Round 1 rule still applies: public sources only (sites, docs, READMEs,
changelogs, release notes). Do not read source code.

## What to check, per survey file

Pick at least 20 entries per file, in this order of priority:

1. Every item in the file's "Distinctive or unusual" section and every
   bullet in "Relevance to a self-hosted master/node harness" (find the
   entries they rest on).
2. Every entry about permissions, approvals, sandboxing, remote or
   distributed execution, sub-agents, memory, and anything security-related.
3. Every entry that states a number, a default, a limit, a version or a
   date.
4. Every entry the author marked as resting on a search summary, a
   truncated page or a "summarising fetcher".
5. Any entry that looks too good, too precise or too convenient.
6. Then a spread of ordinary entries from areas not yet touched.

For each one, open the cited URL yourself and look for the specific claim.
When you fetch a page, ask for the specific fact (for example "what does
this page say the default approval mode is, quote the sentence"), not for a
general summary: summaries are how wrong details got in. If the first URL
does not support the claim, look for the fact on the vendor's other primary
pages before judging.

Give each checked entry one verdict:

- `confirmed`: the cited page supports the claim as written.
- `confirmed-with-correction`: the feature exists but a detail is wrong
  (state the right detail and the URL).
- `not-supported`: the cited page does not say this and you could not find a
  primary page that does.
- `wrong`: a primary page contradicts it (give the URL).
- `unreachable`: the page could not be opened (say what you tried).
- `status-wrong`: the feature exists but its status label is wrong (for
  example GA written for something the page calls experimental).

Also look for the two failure modes that citations cannot catch:

- **Borrowed features**: something described the way another vendor's
  feature works rather than this one's.
- **Notable omissions**: skim the vendor's docs navigation and changelog for
  major documented features the survey does not mention at all. List up to
  ten, each with its URL. Do not write full entries for them unless they are
  short.

## What to change

Edit the survey file in place:

- Fix every `confirmed-with-correction`, `wrong` and `status-wrong` entry,
  and end its Notes line with `(corrected in review: <what changed>)`.
- Move every `not-supported` entry to the "Unverified" section with a note
  saying what you looked for.
- Add `(verified in review)` to the Notes line of each `confirmed` entry.
- Append a final section `## 9. Review` with: the date; how many entries you
  checked out of how many in the file; a table of entry name, verdict and
  one-line reason; the borrowed-feature findings; the notable omissions; and
  your overall judgement of how far the unchecked remainder can be trusted
  (say plainly if the file is unreliable).

Do not rewrite entries you did not check, do not reorder the file, and do
not touch any other file.

## Report back

Under 200 words per file: entries checked, counts per verdict, the most
important corrections, the most important omissions, and your overall
trust judgement.
