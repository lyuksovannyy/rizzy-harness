// What an agent's face shows, read from its session: thinking while text
// streams, working while a tool runs, unsure while it waits on the user (an
// approval, a question), error after a failed turn, and done for a moment
// once a turn ends well, then idle again.
import { useEffect, useRef, useState } from "react";
import { turnStart } from "../lib/callback";
import type { AvatarState, Item, SessionView } from "../types";

/** How long a finished turn shows the happy face. */
export const DONE_MS = 2400;

/** A question or form the user has not answered yet. */
function asking(items: Item[]): boolean {
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i]!;
    if (turnStart(it)) return false;
    if (it.type === "assistant") return !!it.blocks?.some((b) => (b.type === "choices" || b.type === "form") && b.spec);
  }
  return false;
}

/** The face for a session as it stands; "done" needs time, see useAvatarState. */
export function sessionFace(view: SessionView | undefined | null): Exclude<AvatarState, "done"> {
  if (!view) return "idle";
  const items = view.items;
  if (view.running_turn) {
    if (view.pending_approvals.length > 0) return "unsure";
    for (let i = items.length - 1; i >= 0; i--) {
      const it = items[i]!;
      if (it.type === "tool") return it.result ? "thinking" : "working";
      if (it.type === "assistant" || turnStart(it)) return "thinking";
    }
    return "thinking";
  }
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i]!;
    if (it.type === "error" || (it.type === "turn_end" && it.reason === "error")) return "error";
    if (it.type === "turn_end" || turnStart(it) || it.type === "assistant" || it.type === "tool") break;
  }
  return asking(items) ? "unsure" : "idle";
}

/** The face, with "done" for DONE_MS after a turn ends without trouble. */
export function useAvatarState(view: SessionView | undefined | null): AvatarState {
  const face = sessionFace(view);
  const running = !!view?.running_turn;
  const was = useRef(running);
  const [done, setDone] = useState(false);
  useEffect(() => {
    const finished = was.current && !running;
    was.current = running;
    if (!finished) {
      if (running) setDone(false);
      return;
    }
    setDone(true);
    const t = setTimeout(() => setDone(false), DONE_MS);
    return () => clearTimeout(t);
  }, [running]);
  return done && face === "idle" ? "done" : face;
}
