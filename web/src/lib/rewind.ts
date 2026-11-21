// Rewinding a chat to one of the person's messages (X.46, docs/rewind.md):
// when "Rewind to here" shows, what a message gives back to the box, and how
// the rewound items group into blocks. The server decides again
// (`harness_core::rewind::check`, the node's own checks under the turn
// slot); this only keeps the action off where the route would refuse, from
// what the fold holds. Its refusal text is shown where the client's guess
// was wrong.
import { own } from "../emptyView";
import { seqAt } from "../fold";
import type { Item, SessionView } from "../types";
import { turnStart } from "./callback";
import type { Picture } from "./downscale";

/** Why the action is not offered; null when it is. */
export type NoRewind =
  | "unknown" // the fold did not make this item (nothing to name)
  | "viewer" // the viewer is not the one who drives this chat here
  | "sub-agent" // a sub-agent's chat is rewound by the agent that delegated it
  | "routine" // a routine's own first message
  | "not-a-turn" // sent while an answer ran, or not the person's message
  | "rewound" // already out of the conversation
  | "compacted" // a summary holds its text
  | "busy"; // a turn, a /compact, a background command or sub-agent runs

export interface RewindWhere {
  /** The chat's row is known and the viewer may drive it (not `row.blocked`). */
  mine: boolean;
  /** A sub-agent's chat (`row.parent_id` or `meta.parent_session`). */
  subagent: boolean;
  /** A routine's run: its first message is the routine's prompt. */
  routine: boolean;
  /** A /compact asked for from this tab has no outcome yet. */
  compacting: boolean;
}

/** Whether `view.items[i]` is a message "Rewind to here" may name. Reads the
 * view as it is now: never memoise it on the items array (mutated in place). */
export function noRewind(view: SessionView, i: number, w: RewindWhere): NoRewind | null {
  const it = view.items[i];
  if (!it || it.type !== "user") return "not-a-turn";
  if (!w.mine) return "viewer";
  if (w.subagent) return "sub-agent";
  if (it.rewound !== undefined) return "rewound";
  if (it.queued) return "not-a-turn";
  const seq = seqAt(view, i);
  if (seq === undefined) return "unknown";
  if (w.routine && view.items.findIndex((x) => x.type === "user" && !x.queued) === i) return "routine";
  // The latest summary that still counts: its text would still hold the message.
  const through = Math.max(0, ...view.items.flatMap((x) => (x.type === "summary" && x.rewound === undefined ? [x.through_seq] : [])));
  if (seq <= through) return "compacted";
  if (view.running_turn !== null || w.compacting) return "busy";
  if (Object.values(view.background ?? {}).some((b) => !b.ended)) return "busy";
  return null;
}

/** Whether a turn from item `i` on changed files nobody put back: the
 * second button ("and files") is offered only then. */
export function changesFiles(view: SessionView, i: number): boolean {
  for (let k = i; k < view.items.length; k++) {
    const x = view.items[k]!;
    if (x.type !== "turn_end" || x.rewound !== undefined) continue;
    const c = own(view.checkpoints, x.turn_id);
    if (c && c.after && c.files.length > 0 && !c.restored) return true;
  }
  return false;
}

/** What a logged message can honestly be sent again with. */
export interface Resend {
  text: string;
  /** Images are references into Media; a chat sends again what it already carries. */
  pictures: Picture[];
  /** Attachments are consumed when the message is sent (the master deletes the upload): they must be attached again. */
  lost: string[];
}

/** The message at `item` for the box. A form's answer is not one: it has no text to edit. */
export function resend(item: Extract<Item, { type: "user" }>): Resend | null {
  if (item.reply) return null;
  return {
    text: item.text,
    pictures: (item.images ?? []).map((m) => ({ key: m.media_id, name: m.name, size: 0, media_id: m.media_id })),
    lost: item.attachments ?? [],
  };
}

/** The restore's refusals that a forced call goes past (as "Undo anyway"). */
export const forceable = (err: string): boolean => err.includes("changed after the turn") || err.includes('"Undo anyway"');

/** One block of consecutive rewound items under the same rewind. */
export interface Run {
  /** The rewind (its seq) that hides them. */
  rewind: number;
  start: number;
  end: number;
  /** The turns it holds (at least one). */
  turns: number;
}

/** The rewound runs of `items`, in order. */
export function runs(items: readonly Item[]): Run[] {
  const out: Run[] = [];
  items.forEach((it, i) => {
    if (it.rewound === undefined) return;
    const last = out[out.length - 1];
    if (last && last.end === i - 1 && last.rewind === it.rewound) {
      last.end = i;
      if (turnStart(items, i)) last.turns++;
    } else {
      out.push({ rewind: it.rewound, start: i, end: i, turns: 1 });
    }
  });
  return out;
}
