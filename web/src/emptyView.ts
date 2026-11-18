// A session's view before any event: apart from the fold (fold.ts) so the
// live connection can hold a view in the first load while the fold itself
// loads with the chat (live.ts).
import type { SessionView } from "./types";

export function emptyView(): SessionView {
  return {
    session_id: null,
    head_seq: 0,
    meta: null,
    items: [],
    running_turn: null,
    pending_approvals: [],
    unknown_events: 0,
    running_agent: null,
    variant_selection: {},
    usage: { input_tokens: 0, output_tokens: 0, context_tokens: 0 },
    context_tokens: 0,
    checkpoints: {},
  };
}

/** `o[k]` only when `k` is `o`'s own key. Every lookup of a table or a view
 * map by a string from an event, a model, a node or the server goes through
 * this: a key such as "__proto__", "constructor" or "toString" otherwise
 * reads `Object.prototype` (review of PR 156: a tool named "constructor",
 * which a model can send, and a turn id "toString" took the chat down). */
export const own = <V>(o: { readonly [k: string]: V } | undefined | null, k: unknown): V | undefined =>
  o && typeof k == "string" && Object.hasOwn(o, k) ? o[k] : undefined;
