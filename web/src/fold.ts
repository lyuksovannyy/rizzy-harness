// Port of crates/core/src/view.rs. The shared fixture test keeps them equal.
import type { Item, RawEvent, SessionView, StagedArtifact, TodoItem } from "./types";

export function emptyView(): SessionView {
  return {
    session_id: null,
    head_seq: 0,
    meta: null,
    items: [],
    running_turn: null,
    pending_approvals: [],
    unknown_events: 0,
    active_agent: null,
    running_agent: null,
    variant_selection: {},
    usage: { input_tokens: 0, output_tokens: 0, context_tokens: 0 },
    context_tokens: 0,
    checkpoints: {},
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
  "agent_changed",
  "variant_selected",
  "history_summarized",
  "model_fallback",
  "model_retry",
  "artifact_staged",
  "artifact_stage_failed",
  "background_started",
  "background_ended",
  "callback_refused",
  "queued_delivered",
  "todo_updated",
]);

const CLEARS_RETRY = new Set([
  "turn_started",
  "assistant_delta",
  "assistant_message",
  "tool_call_started",
  "turn_ended",
  "history_summarized",
  "model_fallback",
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

/** The staged file as serde writes it: optional keys only when set. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function staged(p: any): StagedArtifact {
  const s: StagedArtifact = {
    stage_id: p.stage_id,
    by: p.by,
    name: p.name,
    filename: p.filename,
    packed: p.packed ?? null,
    size: p.size,
    sha256: p.sha256,
    source_path: p.source_path,
    meta: p.meta ?? {},
  };
  if (p.call_id != null) s.call_id = p.call_id;
  if (p.kind_hint != null) s.kind_hint = p.kind_hint;
  if (p.notes != null) s.notes = p.notes;
  return s;
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
  // Anything the turn shows next means the wait is over (as the Rust view).
  if (CLEARS_RETRY.has(e.kind)) delete v.retry;
  switch (e.kind) {
    case "session_created": {
      // Match serde: optional fields are omitted when absent.
      const meta: SessionView["meta"] = {
        title: p.title,
        cwd: p.cwd,
        model: p.model,
        backend: p.backend,
        node_id: p.node_id,
        approval_mode: p.approval_mode,
        profile: p.profile,
      };
      if (p.agent_id != null) meta!.agent_id = p.agent_id;
      if (p.incognito) meta!.incognito = true;
      if (p.parent_session != null) meta!.parent_session = p.parent_session;
      if (p.no_callbacks) meta!.no_callbacks = true;
      if (p.exec_mode != null) meta!.exec_mode = p.exec_mode;
      v.meta = meta;
      v.active_agent = p.agent_id ?? null;
      break;
    }
    case "turn_started":
      // A callback turn stands where a person's message would.
      if (p.cause?.callback) v.items.push({ type: "callback", turn_id: p.turn_id, bg_ids: p.cause.callback });
      v.running_turn = p.turn_id;
      v.running_agent = p.agent ?? null;
      break;
    case "callback_refused":
      v.items.push({ type: "callback_refused", bg_ids: p.bg_ids, reason: p.reason });
      break;
    case "user_message": {
      const item: Extract<Item, { type: "user" }> = { type: "user", turn_id: p.turn_id, text: p.text };
      const names = ((p.attachments ?? []) as { name: string }[]).map((a) => a.name);
      if (names.length) item.attachments = names;
      if (p.reply != null) item.reply = p.reply;
      if (p.images?.length) item.images = p.images;
      if (p.queued) item.queued = { seq: e.seq, read: false };
      v.items.push(item);
      break;
    }
    case "queued_delivered": {
      const seqs = (p.seqs ?? []) as number[];
      for (const it of v.items) if (it.type === "user" && it.queued && seqs.includes(it.queued.seq)) it.queued.read = true;
      break;
    }
    case "assistant_delta": {
      const a = lastAssistant(v, p.message_id);
      if (a) a.text += p.text;
      else
        v.items.push({
          type: "assistant",
          message_id: p.message_id,
          text: p.text,
          done: false,
          author: p.author ?? null,
          variant: p.variant ?? null,
        });
      break;
    }
    case "assistant_message": {
      const a = lastAssistant(v, p.message_id);
      if (a) {
        a.text = p.text;
        a.done = true;
        if (p.author != null) a.author = p.author;
        if (p.variant != null) a.variant = p.variant;
        if (p.blocks?.length) a.blocks = p.blocks;
        else delete a.blocks;
      } else if (p.text !== "") {
        const item: Extract<Item, { type: "assistant" }> = {
          type: "assistant",
          message_id: p.message_id,
          text: p.text,
          done: true,
          author: p.author ?? null,
          variant: p.variant ?? null,
        };
        if (p.blocks?.length) item.blocks = p.blocks;
        v.items.push(item);
      }
      break;
    }
    case "tool_call_started":
      v.items.push({
        type: "tool",
        call_id: p.call_id,
        author: p.author ?? null,
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
      if (t) {
        t.result = { ok: p.ok, output: p.output, exit_code: p.exit_code ?? null };
        if (p.sources?.length) t.result.sources = p.sources;
      }
      break;
    }
    case "approval_requested": {
      v.pending_approvals.push(p.approval_id);
      const t = lastTool(v, p.call_id);
      if (t) {
        t.approval = { approval_id: p.approval_id, reason: p.reason, decision: null, by: null };
        // As the Rust view: the key only when true.
        if (p.turn_allow === true) t.approval.turn_allow = true;
      }
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
    case "checkpoint": {
      const c = (v.checkpoints[p.turn_id] ??= { before: null, after: null, files: [], restored: false });
      if (p.phase === "before") c.before = p.git_ref;
      else if (p.phase === "after") {
        c.after = p.git_ref;
        c.files = p.files ?? [];
      } else if (p.phase === "restored") c.restored = true;
      break;
    }
    case "turn_ended":
      if (p.usage) {
        v.usage.input_tokens += p.usage.input_tokens;
        v.usage.output_tokens += p.usage.output_tokens;
        if (p.usage.cache_read_tokens) v.usage.cache_read_tokens = (v.usage.cache_read_tokens ?? 0) + p.usage.cache_read_tokens;
        if (p.usage.cache_write_tokens) v.usage.cache_write_tokens = (v.usage.cache_write_tokens ?? 0) + p.usage.cache_write_tokens;
        v.context_tokens = p.usage.context_tokens;
        if (p.usage.window != null) v.context_window = p.usage.window;
        else delete v.context_window;
        if (p.usage.summary_at != null) v.summary_at = p.usage.summary_at;
        else delete v.summary_at;
      }
      v.running_turn = null;
      v.running_agent = null;
      v.items.push({ type: "turn_end", turn_id: p.turn_id, reason: p.reason });
      break;
    case "error":
      v.items.push({ type: "error", message: p.message });
      break;
    case "agent_changed":
      v.active_agent = p.agent.agent_id;
      v.items.push({ type: "agent_changed", agent: p.agent });
      break;
    case "variant_selected":
      v.variant_selection[p.group] = p.message_id;
      break;
    case "history_summarized":
      v.items.push({ type: "summary", through_seq: p.through_seq, text: p.text });
      break;
    case "model_retry":
      v.retry = { turn_id: p.turn_id, model: p.model, attempt: p.attempt, of: p.of, at_ms: p.at_ms, wait_ms: p.wait_ms, reason: p.reason };
      break;
    case "model_fallback":
      v.items.push({ type: "fallback", turn_id: p.turn_id, model: p.model, from: p.from, reason: p.reason ?? null });
      break;
    case "artifact_staged": {
      // An agent's file joins its `publish` card; the user's stands alone.
      const card = p.by === "agent" && p.call_id != null ? lastTool(v, p.call_id) : undefined;
      if (card) card.staged = staged(p);
      else v.items.push({ type: "artifact", staged: staged(p) });
      break;
    }
    case "background_started":
      (v.background ??= {})[p.bg_id] = { call_id: p.call_id, ends_by_ms: p.ends_by_ms, timeout_s: p.timeout_s, ended: false };
      break;
    case "background_ended": {
      const b = v.background?.[p.bg_id];
      if (b) b.ended = true;
      v.items.push({
        type: "background_ended",
        bg_id: p.bg_id,
        reason: p.reason,
        exit_code: p.exit_code ?? null,
        ran_ms: p.ran_ms,
        tail: p.tail ?? "",
      });
      break;
    }
    case "todo_updated": {
      // The whole list each time; an empty one is no list (serde omits it).
      const items = Array.isArray(p.items) ? p.items : [];
      if (items.length) v.todo = items.map((i: { content: string; status: TodoItem["status"] }) => ({ content: i.content, status: i.status }));
      else delete v.todo;
      break;
    }
    case "artifact_stage_failed":
      v.items.push({ type: "artifact_failed", path: p.path, name: p.name ?? null, error: p.error });
      break;
  }
}

export function fold(events: RawEvent[]): SessionView {
  const v = emptyView();
  for (const e of events) apply(v, e);
  return v;
}
