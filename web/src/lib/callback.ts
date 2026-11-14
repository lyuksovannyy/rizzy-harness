// Callback turns (T.8d step 3): a turn the harness started because
// background commands ended while the chat was idle. It has no person's
// message; the chat shows what started it where the bubble would be.
import type { Item } from "../types";

/** Whether a turn starts at item `i`: a person's message or a callback. A
 * message sent while a turn ran (X.5) belongs to that turn; a turn the node
 * started itself for such messages has no item of its own, so the item
 * right after a turn's end starts one too (review ST-4). */
export function turnStart(items: readonly Item[], i: number): boolean {
  const it = items[i]!;
  return (it.type === "user" && !it.queued) || it.type === "callback" || (i > 0 && items[i - 1]!.type === "turn_end");
}

/** "bg-3 ended (exit 1)", from the ends the chat already holds. */
export function endWords(items: readonly Item[], ids: readonly string[]): string {
  return ids
    .map((id) => {
      const e = items.findLast((x) => x.type === "background_ended" && x.bg_id === id);
      const how =
        e?.type !== "background_ended"
          ? ""
          : e.reason === "timed_out"
            ? " (timed out)"
            : e.exit_code != null
              ? ` (exit ${e.exit_code})`
              : ` (${e.reason.replace(/_/g, " ")})`;
      return `${id} ended${how}`;
    })
    .join(", ");
}
