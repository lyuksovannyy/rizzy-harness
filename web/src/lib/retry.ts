// A person's Retry of the chat's failed last turn (ADOPTION_PLAN X.21): when
// the button shows. The node decides again against its log
// (`harness_core::retry::check`); this only keeps the button off where the
// route would refuse, so it is shown on the failed turn's end line alone.
import type { Item, TurnEndReason } from "../types";
import { turnStart } from "./callback";

/** The ends a person may retry: no complete answer, and not their own Stop. */
export const RETRYABLE: readonly TurnEndReason[] = ["error", "interrupted", "stopped"];

/** Whether the turn that ended at item `i` may be retried from the chat:
 * the chat's last turn, a person's (not a callback turn), ended without an
 * answer and not by their Stop, with no message or turn after it. The
 * caller adds what the items do not hold (a turn running, a sub-agent's
 * chat, a routine's own run, an undo of the turn, the viewer's access).
 * Not checked here: an undo of another turn after the failure (a "Rewind
 * files" of an earlier turn), which the items do not order against the
 * end; the node refuses that retry and the line says why. */
export function canRetry(items: readonly Item[], i: number): boolean {
  const end = items[i];
  if (end?.type !== "turn_end" || !RETRYABLE.includes(end.reason)) return false;
  // Nothing that starts or answers a turn after it.
  for (let k = i + 1; k < items.length; k++) {
    const it = items[k]!;
    // A message sent while it ran and never read waits for the turn
    // after the retry: the node accepts the retry (core `retry::check`).
    if (it.type === "user" && it.queued) continue;
    if (["user", "callback", "retry", "turn_end", "assistant", "tool"].includes(it.type)) return false;
  }
  // The item that started it: never a callback turn.
  for (let k = i - 1; k >= 0; k--) {
    if (items[k]!.type === "turn_end") break;
    if (turnStart(items, k)) return items[k]!.type !== "callback";
  }
  return true;
}
