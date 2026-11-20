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

/** The sentence for how HEAD moved, one of four cases (branch to branch,
 * the same branch with a new commit, and the two with a detached HEAD). */
export function headNote(m: HeadMoved): ReactNode[] {
  const { from, to } = m;
  const undo = " Undo puts the files back as they were before the turn, on whatever is checked out now; it does not switch the branch back.";
  const commits = (a: HeadAt, b: HeadAt) => `(${short(a.commit)} → ${short(b.commit)})`;
  if (from.branch != null && to.branch != null && from.branch !== to.branch) {
    return [
      "This turn switched the branch from ",
      name(from.branch),
      " to ",
      name(to.branch),
      ". The file count includes what the switch changed. Undo puts the files back as they were before the turn, on whichever branch is checked out now; it does not switch the branch back.",
    ];
  }
  if (from.branch != null && to.branch != null) {
    return ["This turn made or moved commits on ", name(from.branch), ` ${commits(from, to)}. Undo puts the files back; the commits stay.`];
  }
  if (from.branch != null) {
    return ["This turn left the branch ", name(from.branch), ` and detached HEAD at ${short(to.commit)}. The file count includes what that changed.${undo}`];
  }
  if (to.branch != null) {
    return [`This turn switched from a detached HEAD (${short(from.commit)}) to the branch `, name(to.branch), `. The file count includes what the switch changed.${undo}`];
  }
  return [`This turn moved a detached HEAD ${commits(from, to)}. Undo puts the files back; the commits stay.`];
}

export function HeadNote(props: { moved: HeadMoved; lead?: string }) {
  return (
    <p className="undo-note" role="note">
      {props.lead}
      {headNote(props.moved)}
    </p>
  );
}
