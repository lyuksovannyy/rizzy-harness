# Plan rows

One file per task row of [`ADOPTION_PLAN.md`](../../ADOPTION_PLAN.md) section 4: `<id>.md` (`A.17.md`, `X.44.md`, `T.8d.md`).
`ADOPTION_PLAN.md` keeps an index of one line per row under each phase, with a link here.
A row's text is split one sentence to a line, so that two pull requests that edit different sentences merge without a conflict.
Break a line only after a sentence, never inside a code span, a link or an open bracket; joining the lines of a section with single spaces gives the cell's text.

To add a row: take the next free id of its phase, write `<id>.md` with the layout of its neighbours (`# <id>`, then one `## <column>` section per column of that phase's table), and add one line to the phase's index table.
To change a row, edit its file; leave the index line alone unless the first sentence of the first column changed.
Ids that appear twice (A.17, W.1, W.7) are kept as `A.17.md` and `A.17-2.md`; renumber one of them in a later change.
