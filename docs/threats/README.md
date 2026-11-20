# Threat model rows

One file per row of the threat model in [`PLAN.md`](../../PLAN.md) section 4: `T<n>.md`.
`PLAN.md` keeps an index of one line per row, with a link here.
A row's text is split one sentence to a line, so that two pull requests that edit different sentences merge without a conflict.
Break a line only after a sentence, never inside a code span, a link or an open bracket; joining the lines of a section with single spaces gives the cell's text.

To add a row: take the next free number (numbers are never reused or renumbered; T15 and T31 were never used), write `T<n>.md` with the same layout as its neighbours (`# T<n>`, `## Threat`, `## Mitigation`), and add one line to the index table in `PLAN.md`.
To change a row, edit its file; leave the index line alone unless the first sentence of the threat changed.
Rows that two files share an id (T49 has two) are named `T49.md` and `T49-2.md`.
