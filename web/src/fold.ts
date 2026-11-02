// Port of crates/core/src/view.rs. The shared fixture test keeps them equal.
import type { Item, RawEvent, SessionView } from "./types";

export function emptyView(): SessionView {
  return {
    session_id: null,
    head_seq: 0,
    meta: null,
    items: [],
    running_turn: null,
    pending_approvals: [],
    unknown_events: 0,
  };
}

export class FoldError extends Error {}

const KNOWN = new Set([
  "session_created",
  "turn_started",
  "user_message",
  "assistant_delta",
  "assistant_message",
  "tool_call_started",
  "tool_output_delta",
  "tool_call_finished",
  "approval_requested",
  "approval_resolved",
  "checkpoint",
  "turn_ended",
  "error",
]);

function lastTool(v: SessionView, callId: string): Extract<Item, { type: "tool" }> | undefined {
  for (let i = v.items.length - 1; i >= 0; i--) {
    const it = v.items[i]!;
    if (it.type === "tool" && it.call_id === callId) return it;
  }
  return undefined;
}

function lastAssistant(
  v: SessionView,
  messageId: string,
): Extract<Item, { type: "assistant" }> | undefined {
  for (let i = v.items.length - 1; i >= 0; i--) {
    const it = v.items[i]!;
    if (it.type === "assistant" && it.message_id === messageId) return it;
  }
  return undefined;
}

/** Apply one event in place. Throws on gaps, duplicates or wrong session. */
export function apply(v: SessionView, e: RawEvent): void {
  if (v.session_id !== null && v.session_id !== e.session_id) {
    throw new FoldError("event for another session");
  }
  if (e.seq !== v.head_seq + 1) {
    throw new FoldError(`expected seq ${v.head_seq + 1}, got ${e.seq}`);
  }
  v.session_id = e.session_id;
  v.head_seq = e.seq;
  if (!KNOWN.has(e.kind)) {
    v.unknown_events += 1;
    return;
  }
  const p = e.payload ?? {};
  switch (e.kind) {
    case "session_created":
      v.meta = {
        title: p.title,
        cwd: p.cwd,
        model: p.model,
        backend: p.backend,
        node_id: p.node_id,
        approval_mode: p.approval_mode ?? "ask",
      };
      break;
    case "turn_started":
      v.running_turn = p.turn_id;
      break;
    case "user_message":
      v.items.push({ type: "user", turn_id: p.turn_id, text: p.text });
      break;
    case "assistant_delta": {
      const a = lastAssistant(v, p.message_id);
      if (a) a.text += p.text;
      else v.items.push({ type: "assistant", message_id: p.message_id, text: p.text, done: false });
      break;
    }
    case "assistant_message": {
      const a = lastAssistant(v, p.message_id);
      if (a) {
        a.text = p.text;
        a.done = true;
      } else if (p.text !== "") {
        v.items.push({ type: "assistant", message_id: p.message_id, text: p.text, done: true });
      }
      break;
    }
    case "tool_call_started":
      v.items.push({
        type: "tool",
        call_id: p.call_id,
        tool: p.tool,
        input: p.input,
        live_output: "",
        result: null,
        approval: null,
      });
      break;
    case "tool_output_delta": {
      const t = lastTool(v, p.call_id);
      if (t) t.live_output += p.text;
      break;
    }
    case "tool_call_finished": {
      const t = lastTool(v, p.call_id);
      if (t) t.result = { ok: p.ok, output: p.output, exit_code: p.exit_code ?? null };
      break;
    }
    case "approval_requested": {
      v.pending_approvals.push(p.approval_id);
      const t = lastTool(v, p.call_id);
      if (t) t.approval = { approval_id: p.approval_id, reason: p.reason, decision: null, by: null };
      break;
    }
    case "approval_resolved": {
      v.pending_approvals = v.pending_approvals.filter((id) => id !== p.approval_id);
      for (let i = v.items.length - 1; i >= 0; i--) {
        const it = v.items[i]!;
        if (it.type === "tool" && it.approval && it.approval.approval_id === p.approval_id) {
          it.approval.decision = p.decision;
          it.approval.by = p.by;
          break;
        }
      }
      break;
    }
    case "checkpoint":
      break;
    case "turn_ended":
      v.running_turn = null;
      v.items.push({ type: "turn_end", turn_id: p.turn_id, reason: p.reason });
      break;
    case "error":
      v.items.push({ type: "error", message: p.message });
      break;
  }
}

export function fold(events: RawEvent[]): SessionView {
  const v = emptyView();
  for (const e of events) apply(v, e);
  return v;
}
