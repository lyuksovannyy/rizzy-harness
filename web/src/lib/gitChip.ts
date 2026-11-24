// The header's git chip (X.64): what `GET /api/sessions/{id}/git` answers and
// the words made of it. The branch name is text from the repository, written
// by whoever controls it: the page cuts it for display and draws it through
// showHidden, as text, never as markup, a link or a URL.
import { showHidden } from "../md/hidden";

/** What the route answers, or `null` for a folder that is no repository the
 * node reads (or a node that is away or too old: nothing is shown). */
export interface GitView {
  /** The branch HEAD names; absent when detached. */
  branch?: string;
  /** The first seven hex digits of HEAD's commit; absent before a first commit. */
  commit?: string;
  /** Files that differ from that commit, untracked ones included, at most
   * 1000; absent when the node could not tell (which is not none). */
  changed?: number;
}

export const gitUrl = (session: string) => `/api/sessions/${encodeURIComponent(session)}/git`;

/** The most characters of a branch name the chip shows. */
export const BRANCH_SHOWN = 80;

/** The route's answer held to its shape (a server that sends more or other
 * is read as nothing to show). */
export function readGit(v: unknown): GitView | null {
  if (typeof v !== "object" || v === null) return null;
  const o = v as Record<string, unknown>;
  const branch = typeof o.branch === "string" && o.branch !== "" ? o.branch : undefined;
  const commit = typeof o.commit === "string" && /^[0-9a-f]{7,64}$/.test(o.commit) ? o.commit.slice(0, 7) : undefined;
  const n = o.changed;
  const changed = typeof n === "number" && Number.isFinite(n) && n >= 0 ? Math.min(Math.floor(n), 1000) : undefined;
  return branch === undefined && commit === undefined ? null : { branch, commit, changed };
}

/** A branch name cut to [BRANCH_SHOWN] characters with an ellipsis, its hidden
 * characters shown as codes. */
export function shownBranch(name: string): string {
  const chars = Array.from(name);
  const cut = chars.length > BRANCH_SHOWN ? `${chars.slice(0, BRANCH_SHOWN).join("")}…` : name;
  return showHidden(cut);
}

/** The chip's text: the branch, or "detached at abc1234". */
export function chipName(g: GitView): string {
  if (g.branch !== undefined) return shownBranch(g.branch);
  return `detached at ${g.commit ?? "an unknown commit"}`;
}

/** "3 changed", "999+ changed"; none for a clean folder or an unknown count. */
export function countText(g: GitView): string | null {
  if (!g.changed) return null;
  return `${g.changed >= 1000 ? "999+" : g.changed} changed`;
}

/** The chip's hover and screen-reader text, in full. */
export function chipTitle(g: GitView): string {
  const where =
    g.branch !== undefined
      ? `Branch ${showHidden(g.branch)}${g.commit === undefined ? ", no commit yet" : ""}`
      : `HEAD is detached at ${g.commit ?? "an unknown commit"}`;
  let what: string;
  if (g.changed === undefined) what = "; the number of changed files is not known";
  else if (g.changed === 0) what = "; no changed files";
  else {
    const n = g.changed >= 1000 ? "999 or more" : String(g.changed);
    what = `; ${n} changed file${g.changed === 1 ? "" : "s"} since the last commit, new files included`;
  }
  return `${where}${what}. Click to refresh.`;
}
