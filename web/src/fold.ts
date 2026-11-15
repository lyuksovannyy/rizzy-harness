// Port of crates/core/src/view.rs. The shared fixture test keeps them equal.
import type { Item, ModelTokens, RawEvent, SessionView, StagedArtifact, TodoItem, Usage } from "./types";

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

/** `Usage::add` in crates/core/src/event.rs: the sums, and the per-model
 * breakdown up to MAX_USAGE_MODELS (a model past it stays unattributed). */
export const MAX_USAGE_MODELS = 16;

// Model ids come from the node's events, so they are never used to index an
// object through its prototype ("__proto__", "constructor"): reads check own
// keys, writes define own properties (review of PR 119, M1). The objects stay
// plain, so the view keeps the JSON shape the Rust fold writes.
type ByModel = Record<string, ModelTokens>;
const own = (o: ByModel, k: string): ModelTokens | undefined => (Object.hasOwn(o, k) ? o[k] : undefined);
const put = (o: ByModel, k: string, v: ModelTokens) =>
  Object.defineProperty(o, k, { value: v, writable: true, enumerable: true, configurable: true });

/** Rust's BTreeMap<String> order: UTF-8 bytes, which is code point order
 * (JS objects list integer-like keys first, and `sort()` compares UTF-16). */
function byteOrder(a: string, b: string): number {
  const x = [...a];
  const y = [...b];
  for (let i = 0; i < Math.min(x.length, y.length); i++) {
    const d = x[i]!.codePointAt(0)! - y[i]!.codePointAt(0)!;
    if (d !== 0) return d;
  }
  return x.length - y.length;
}

const sortedKeys = (o: ByModel) => Object.keys(o).sort(byteOrder);

/** A model's tokens as serde writes them (zero cache counts left out). */
function tokensOf(t: ModelTokens): ModelTokens {
  const c: ModelTokens = { input_tokens: t.input_tokens, output_tokens: t.output_tokens };
  if (t.cache_read_tokens) c.cache_read_tokens = t.cache_read_tokens;
  if (t.cache_write_tokens) c.cache_write_tokens = t.cache_write_tokens;
  return c;
}

function addUsage(to: Usage, u: Usage) {
  to.input_tokens += u.input_tokens;
  to.output_tokens += u.output_tokens;
  if (u.cache_read_tokens) to.cache_read_tokens = (to.cache_read_tokens ?? 0) + u.cache_read_tokens;
  if (u.cache_write_tokens) to.cache_write_tokens = (to.cache_write_tokens ?? 0) + u.cache_write_tokens;
  const from = u.by_model ?? {};
  for (const model of sortedKeys(from)) {
    const t = own(from, model)!;
    const by = (to.by_model ??= {});
    const mine = own(by, model);
    if (mine) {
      mine.input_tokens += t.input_tokens;
      mine.output_tokens += t.output_tokens;
      if (t.cache_read_tokens) mine.cache_read_tokens = (mine.cache_read_tokens ?? 0) + t.cache_read_tokens;
      if (t.cache_write_tokens) mine.cache_write_tokens = (mine.cache_write_tokens ?? 0) + t.cache_write_tokens;
    } else if (Object.keys(by).length < MAX_USAGE_MODELS) {
      put(by, model, tokensOf(t));
    }
  }
}

/** `Usage::capped`: an event's usage as the fold keeps it, its known fields
 * only and at most MAX_USAGE_MODELS models, the first in byte order. */
export function cappedUsage(u: Usage): Usage {
  const c: Usage = { input_tokens: u.input_tokens, output_tokens: u.output_tokens, context_tokens: u.context_tokens };
  if (u.cache_read_tokens) c.cache_read_tokens = u.cache_read_tokens;
  if (u.cache_write_tokens) c.cache_write_tokens = u.cache_write_tokens;
  if (u.window != null) c.window = u.window;
  if (u.summary_at != null) c.summary_at = u.summary_at;
  if (u.model != null) c.model = u.model;
  const from = u.by_model ?? {};
  const keys = sortedKeys(from).slice(0, MAX_USAGE_MODELS);
  if (keys.length > 0) {
    const by: ByModel = {};
    for (const k of keys) put(by, k, tokensOf(own(from, k)!));
    c.by_model = by;
  }
  return c;
}

/** `harness_core::event::IMAGE_WITHHELD_ROW`. */
const IMAGE_WITHHELD_ROW =
  "[image not shown to the model: the provider refused a request that carried it]";

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
  "context_shortened",
  "model_fallback",
  "model_retry",
  "retry_given_up",
  "artifact_staged",
  "artifact_stage_failed",
  "background_started",
  "background_ended",
  "callback_refused",
  "queued_delivered",
  "todo_updated",
  "approval_mode_changed",
  "context_sent",
  "chat_allow_added",
  "chat_allow_removed",
  "step_limit_changed",
  "handoff_written",
  "compact_failed",
  "images_withheld",
]);

const CLEARS_RETRY = new Set([
  "turn_started",
  "assistant_delta",
  "assistant_message",
  "tool_call_started",
  "turn_ended",
  "history_summarized",
  "context_shortened",
  "model_fallback",
  "retry_given_up",
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
      // A sub-agent's follow-up from the chat that started it (T68).
      if (p.continued_by) (v.follow_ups ??= []).push(p.turn_id);
      // A person's Retry (X.21): the failed attempt stays above it.
      if (p.retry) {
        const r: Extract<Item, { type: "retry" }> = { type: "retry", turn_id: p.turn_id, of: p.retry.turn_id, reason: p.retry.reason };
        if (p.retry.failed_model != null) r.failed_model = p.retry.failed_model;
        if (p.retry.model != null) r.model = p.retry.model;
        v.items.push(r);
      }
      v.running_turn = p.turn_id;
      v.running_agent = p.agent ?? null;
      break;
    case "callback_refused":
      v.items.push({ type: "callback_refused", bg_ids: p.bg_ids, reason: p.reason });
      break;
    case "handoff_written": {
      // Its cost is the chat's, like a turn's (the sums only).
      if (p.usage) addUsage(v.usage, p.usage);
      const it: Extract<Item, { type: "handoff" }> = { type: "handoff", through_seq: p.through_seq };
      if (p.text != null) it.text = p.text;
      if (p.model != null) it.model = p.model;
      if (p.failed != null && p.failed.length > 0) it.failed = p.failed;
      v.items.push(it);
      break;
    }
    case "user_message": {
      const item: Extract<Item, { type: "user" }> = { type: "user", turn_id: p.turn_id, text: p.text };
      const names = ((p.attachments ?? []) as { name: string }[]).map((a) => a.name);
      if (names.length) item.attachments = names;
      const files = ((p.files ?? []) as { path: string }[]).map((f) => f.path);
      if (files.length) item.files = files;
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
    case "images_withheld": {
      // As the Rust view: the row says its image was not shown (X.23).
      for (const id of p.calls ?? []) {
        const t = lastTool(v, id);
        if (t?.result) t.result.output += "\n\n" + IMAGE_WITHHELD_ROW;
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
        if (p.chat_allow === true) t.approval.chat_allow = true;
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
          if (p.note != null) it.approval.note = p.note;
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
        addUsage(v.usage, p.usage);
        v.context_tokens = p.usage.context_tokens;
        if (p.usage.window != null) v.context_window = p.usage.window;
        else delete v.context_window;
        if (p.usage.summary_at != null) v.summary_at = p.usage.summary_at;
        else delete v.summary_at;
        if (p.usage.model != null) v.context_model = p.usage.model;
        else delete v.context_model;
      }
      // The turn that ended, also when it reported none (an interrupted
      // one): never an older turn's as the last.
      if (p.usage) v.turn_usage = cappedUsage(p.usage);
      else delete v.turn_usage;
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
    case "approval_mode_changed": {
      if (v.meta) v.meta.approval_mode = p.mode;
      const it: Extract<Item, { type: "approval_mode_changed" }> = { type: "approval_mode_changed", mode: p.mode };
      if (p.requested != null) it.requested = p.requested;
      v.items.push(it);
      break;
    }
    case "step_limit_changed": {
      v.max_steps = p.max_steps;
      const it: Extract<Item, { type: "step_limit_changed" }> = { type: "step_limit_changed", max_steps: p.max_steps };
      if (p.requested != null) it.requested = p.requested;
      v.items.push(it);
      break;
    }
    case "variant_selected":
      v.variant_selection[p.group] = p.message_id;
      break;
    case "history_summarized": {
      // A person's /compact (X.20): its cost is the chat's, and the meter
      // shows the shorter context until the next turn reports its own.
      if (p.usage) addUsage(v.usage, p.usage);
      if (p.tokens_after != null) v.context_tokens = p.tokens_after;
      const it: Extract<Item, { type: "summary" }> = { type: "summary", through_seq: p.through_seq, text: p.text };
      if (p.manual === true) it.manual = true;
      if (p.focus != null) it.focus = p.focus;
      v.items.push(it);
      break;
    }
    case "compact_failed":
      if (p.usage) addUsage(v.usage, p.usage);
      v.items.push({ type: "error", message: p.reason });
      break;
    case "context_shortened": {
      const it: Extract<Item, { type: "shortened" }> = {
        type: "shortened",
        turn_id: p.turn_id,
        cleared: p.cleared != null,
        tokens_before: p.tokens_before,
        tokens_after: p.tokens_after,
      };
      if (p.folded != null) {
        it.steps = p.folded.steps;
        it.text = p.folded.text;
      }
      v.items.push(it);
      break;
    }
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
    case "chat_allow_added":
      (v.chat_allows ??= {})[p.grant_id] = { tool: p.tool, input: p.input };
      break;
    case "chat_allow_removed":
      if (v.chat_allows) {
        delete v.chat_allows[p.grant_id];
        if (Object.keys(v.chat_allows).length === 0) delete v.chat_allows;
      }
      break;
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
    // What the model was sent with the turn's message (X.1): never shown.
    case "context_sent":
      break;
  }
}

export function fold(events: RawEvent[]): SessionView {
  const v = emptyView();
  for (const e of events) apply(v, e);
  return v;
}
