// Forking a chat at a finished turn (ADOPTION_PLAN C.2, README "Fork a chat"):
// when the page offers it. The server decides again (`harness_core::fork::check`
// on the master and on the node); this only keeps the link off where the
// route would refuse, so it is shown on a turn's end line alone.
import type { NodeInfo, SessionRow, SessionView } from "../types";

/** What the page knows about the chat that the log does not say. */
export interface ForkCtx {
  row: SessionRow | null;
  node?: NodeInfo;
  /** Why the viewer cannot drive this chat here (`row.blocked`). */
  blocked: string | null;
  /** A routine's own run. */
  routine: boolean;
  /** A sub-agent's chat. */
  subAgent: boolean;
}

/** Whether the turn that ended at item `i` may be forked from. The chat is
 * the viewer's (a chat is private to its owner), a person's turn of any end
 * reason, a turn a running one does not stop (the parent may run a later
 * turn); not one a rewind took out of the conversation. The chat's node
 * must announce `fork`. */
export function canFork(view: SessionView, i: number, ctx: ForkCtx): boolean {
  const end = view.items[i];
  if (end?.type !== "turn_end" || !ctx.row) return false;
  if (ctx.blocked || ctx.routine || ctx.subAgent || ctx.node?.fork !== true) return false;
  if (end.rewound !== undefined) return false;
  // The turn a rewind went back to is gone too (its start stands outside the range).
  if (view.rewinds?.some((r) => !r.undone && r.turn_id === end.turn_id)) return false;
  return true;
}
