// A turn that moved the person's HEAD says so where its changes and its Undo
// are shown (X.42, docs/undo-and-stash.md §2.2): the file count of such a turn
// holds what a branch switch changed, and Undo puts files back without moving
// the branch. Names are text from the repository: drawn as text, with the
// characters that hide or reorder text shown as codes, never as markup.
import type { ReactNode } from "react";
import { showHidden } from "../md/hidden";
import type { HeadAt, HeadMoved } from "../types";

const short = (c: string) => c.slice(0, 7);
const name = (n: string) => <code key={`b${n}`}>{showHidden(n)}</code>;

/** The sentence for how the folder's HEAD moved while a turn ran. The
 * readings are of the folder: another chat in it, a person or a command may
 * have moved it, so the words say what changed while the turn ran, not who
 * did it. `later`: the turn is a later one that a rewind to the shown turn
 * covers ("while it ran"). */
export function headNote(m: HeadMoved, later = false): ReactNode[] {
  const { from, to } = m;
  const whileIt = later ? "while it ran," : "While this turn ran,";
  const here = " Undo puts the files back as they were before the turn, on whichever branch is checked out now; it does not switch back.";
  const commits = (a: HeadAt, b: HeadAt) => `from ${short(a.commit)} to ${short(b.commit)}`;
  const same = from.commit === to.commit;
  if (from.branch != null && to.branch != null && from.branch !== to.branch) {
    return [
      `${whileIt} the checked-out branch changed from `,
      name(from.branch),
      " to ",
      name(to.branch),
      same ? " at the same commit. Undo does not switch back." : `. The file count includes what that switch changed.${here}`,
    ];
  }
  if (from.branch != null && to.branch != null) {
    return [`${whileIt} `, name(from.branch), ` moved ${commits(from, to)} (a commit, a reset or the like). Undo puts the files back as they were before the turn; it does not move the branch.`];
  }
  if (from.branch != null) {
    return [`${whileIt} `, name(from.branch), ` was left for the commit ${short(to.commit)}.${same ? "" : " The file count includes what that changed."} Undo puts the files back as they were before the turn, on whatever is checked out now; it does not switch back.`];
  }
  if (to.branch != null) {
    return [`${whileIt} the commit ${short(from.commit)} was left for `, name(to.branch), `.${same ? "" : " The file count includes what that changed."} Undo puts the files back as they were before the turn, on whatever is checked out now; it does not switch back.`];
  }
  return [`${whileIt} the checked-out commit changed ${commits(from, to)}. Undo puts the files back as they were before the turn; it does not move HEAD.`];
}

export function HeadNote(props: { moved: HeadMoved; later?: boolean }) {
  return (
    <p className="undo-note" role="note">
      {props.later && "A later turn is covered by rewinding to here: "}
      {headNote(props.moved, props.later)}
    </p>
  );
}

/** One sentence per later move, the most recent last; past three, the first
 * two and the count. */
export function LaterNotes(props: { moves: HeadMoved[] }) {
  const shown = props.moves.length > 3 ? props.moves.slice(0, 2) : props.moves;
  return (
    <>
      {shown.map((m, i) => (
        <HeadNote key={i} moved={m} later />
      ))}
      {props.moves.length > 3 && (
        <p className="undo-note" role="note">
          and {props.moves.length - 2} more.
        </p>
      )}
    </>
  );
}
