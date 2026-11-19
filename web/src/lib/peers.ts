// The list of agents one message called (X.37), read from the chat's items.
import type { Item } from "../types";

/** How many agents of the last message still have their turn to come, while the chat works: 0 once the list is over, stopped or the chat idle. */
export function peersLeft(items: Item[], working: boolean): number {
  if (!working) return 0;
  for (let i = items.length - 1; i >= 0; i--) {
    const it = items[i]!;
    if (it.type === "peers_skipped") return 0;
    if (it.type !== "user" || it.queued) continue;
    const n = it.triggers?.length ?? 0;
    if (n < 2) return 0;
    const asked = items.slice(i + 1).filter((x) => x.type === "called").length;
    return Math.max(0, n - 1 - asked);
  }
  return 0;
}
