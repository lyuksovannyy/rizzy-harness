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
