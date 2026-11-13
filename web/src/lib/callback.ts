// Callback turns (T.8d step 3): a turn the harness started because
// background commands ended while the chat was idle. It has no person's
// message; the chat shows what started it where the bubble would be.
import type { Item } from "../types";

/** Where a turn starts in the items: a person's message or a callback. */
export const turnStart = (it: Item): boolean => it.type === "user" || it.type === "callback";

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
