// Port of crates/core/src/view.rs. The shared fixture test keeps them equal.
import type { BackgroundState, Item, ModelTokens, RawEvent, SessionView, StagedArtifact, TodoItem, Usage } from "./types";
import { emptyView, own } from "./emptyView";

export { emptyView };

export class FoldError extends Error {}

/** `Usage::add` in crates/core/src/event.rs: the sums, and the per-model
 * breakdown up to MAX_USAGE_MODELS (a model past it stays unattributed). */
export const MAX_USAGE_MODELS = 16;

// Model ids come from the node's events, so they are never used to index an
// object through its prototype ("__proto__", "constructor"): reads check own
// keys, writes define own properties (review of PR 119, M1). The objects stay
// plain, so the view keeps the JSON shape the Rust fold writes.
// The same holds for every map the view keys by a node's id (checkpoints by
// turn, background entries, chat allows, variant choices): a "__proto__"
// turn id once wrote to Object.prototype.
type ByModel = Record<string, ModelTokens>;
const put = <T>(o: Record<string, T>, k: string, v: T): T => (
  Object.defineProperty(o, k, { value: v, writable: true, enumerable: true, configurable: true }), v
);

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

const isObj = (x: unknown): x is Record<string, unknown> => !!x && typeof x == "object" && !Array.isArray(x);

/** Sums stop at the largest integer a browser holds (Rust: `saturating_add`). */
const sat = (a: number, b: number) => Math.min(a + b, Number.MAX_SAFE_INTEGER);
export function addUsage(to: Usage, u: Usage) {
  to.input_tokens = sat(to.input_tokens, u.input_tokens);
  to.output_tokens = sat(to.output_tokens, u.output_tokens);
  if (u.cache_read_tokens) to.cache_read_tokens = sat(to.cache_read_tokens ?? 0, u.cache_read_tokens);
  if (u.cache_write_tokens) to.cache_write_tokens = sat(to.cache_write_tokens ?? 0, u.cache_write_tokens);
  const from = u.by_model ?? {};
  for (const model of sortedKeys(from)) {
    const t = own(from, model)!;
    const by = (to.by_model ??= {});
    const mine = own(by, model);
    if (mine) {
      mine.input_tokens = sat(mine.input_tokens, t.input_tokens);
      mine.output_tokens = sat(mine.output_tokens, t.output_tokens);
      if (t.cache_read_tokens) mine.cache_read_tokens = sat(mine.cache_read_tokens ?? 0, t.cache_read_tokens);
      if (t.cache_write_tokens) mine.cache_write_tokens = sat(mine.cache_write_tokens ?? 0, t.cache_write_tokens);
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

/** The latest `before_restore`'s id, as `SessionView::saving` in
 * crates/core/src/view.rs: the restore whose `restored` events follow it.
 * Beside the view for the same reason as `spent`. */
const saving = new WeakMap<SessionView, string>();
/** As `SessionView::undone_before`: for the last MAX_SAVED saved ids, which
 * turns were undone (and by which saved id; "" for none) just before that
 * restore. Restoring a saved id returns exactly these flags. */
const MAX_SAVED = 100;
const undoneBefore = new WeakMap<SessionView, [string, Map<string, string>][]>();

/** X.39, as `SessionView::settled` and `live` in crates/core/src/view.rs: what
 * the finished turns (and every other cost the chat carries) used, apart from
 * the running turn's latest `turn_usage`; `v.usage` is the two together. Kept
 * beside the view, not in it, so the view keeps the JSON shape the Rust fold
 * writes. */
interface Spent {
  settled: Usage;
  turn?: string;
  live?: Usage;
}
const spent = new WeakMap<SessionView, Spent>();
function spentOf(v: SessionView): Spent {
  let s = spent.get(v);
  if (!s) spent.set(v, (s = { settled: v.usage }));
  return s;
}
/** What the view's running turn has used so far (its latest `turn_usage`);
 * undefined when none is running or it has reported none yet. Not
 * `v.turn_usage`, which also holds the last finished turn's figure. */
export function runningUsage(v: SessionView): Usage | undefined {
  const s = spent.get(v);
  return v.running_turn !== null && s?.turn === v.running_turn ? s.live : undefined;
}
const sumTokens = (u: Usage) => sat(u.input_tokens, u.output_tokens);
/** `v.usage` = the settled sums plus the running turn's. */
function showUsage(v: SessionView, s: Spent) {
  if (!s.live) return void (v.usage = s.settled);
  const u = cappedUsage(s.settled);
  addUsage(u, s.live);
  v.usage = u;
}
/** A cost the chat carries (a hand-off, a summary, a background agent, a
 * finished turn): settled, whatever turn runs. */
function settle(v: SessionView, u: Usage) {
  const s = spentOf(v);
  addUsage(s.settled, u);
  showUsage(v, s);
}

/** `harness_core::event::IMAGE_WITHHELD_ROW`. */
const IMAGE_WITHHELD_ROW =
  "[image not shown to the model: the provider refused a request that carried it]";

// ── The guard: where events enter the fold (nothing a node sends can crash it) ──
// A node's payload reaches the browser as the node wrote it: the master
// relays it without reading it, and a node may be older, newer, buggy or
// hostile (reviews of PRs 130, 134 and 137 each found one that crashed a
// fold or a row). So every payload is checked here, once, against the
// schema of its kind, and the fold below reads only the checked copy:
// - a payload that is not an object, or a field serde requires that is
//   missing, of the wrong type or (an enum) not one of its values, drops
//   the event: it is counted in `unknown_events` and the state is otherwise
//   the state before it. The Rust fold does the same with what serde cannot
//   read. An enum value outside its list drops the event wherever it stands
//   (optional fields and array elements too): a newer node's new value is
//   therefore not shown by an older page, and counted, as the Rust fold
//   counts it;
// - an optional field of the wrong type is left out, as if absent, and so is
//   an array element of the wrong type. Here the browser deliberately
//   differs from the Rust fold, which drops the whole event: a bad `thinking`
//   or background `child` does not cost the message or the end (review of
//   PR 137). Only a buggy or hostile node causes it, and the chat then shows
//   an event the server's own views count as unknown;
// - fields the guard does not list are left out (forward compatible: a
//   newer node's extra fields fold as before; serde drops them too).
// Numbers are checked as numbers, not as serde's integer ranges. `j` values
// are kept as sent (a tool's `input`, a `ui:` block's `spec`, a form's
// values), and so is an artifact's `meta` (`o`): the rows that read inside
// them check what they read.
//
// The table is compared with the Rust schema (web/src/schema.test.ts and
// crates/core/tests/browser_schema.rs): every field serde requires is
// listed, and every optional one is listed or named there as not read.
//
// A field is `name` (a string) or `name:T`, `?name` when optional. T is one
// of s n b (string, number, boolean), o (an object), j (any JSON value),
// =e (one of ENUMS[e]), [T (an array of T), {T (an object of T by key) or
// @x (the struct x of STRUCTS).
const ENUMS: Record<string, string> = {
  approvalMode: "plan ask_all ask accept_edits auto",
  profile: "chat agentic",
  execMode: "sandbox host",
  turnEnd: "completed cancelled error interrupted stopped limit",
  mention: "file cut folder missing refused binary too_large",
  uiKind: "choices form progress chart",
  thought: "signed redacted text",
  stream: "stdout stderr",
  decision: "allow deny allow_session allow_chat",
  resolvedBy: "user policy cancelled interrupted",
  phase: "before after restored before_restore",
  bgReason: "exited timed_out stopped_by_agent stopped_by_user access_revoked session_deleted node_stopping node_restarted finished",
  confirmed: "ended still_alive unconfirmed unknown",
  retryReason: "rate_limit overloaded server_error network no_answer",
  stagedBy: "agent user",
  todoStatus: "pending in_progress done",
  frame: "person task callback step",
  calledBy: "person",
  peersReason: "stopped restarted unavailable",
};

const STRUCTS: Record<string, string> = {
  tokens: "input_tokens:u output_tokens:u ?cache_read_tokens:u ?cache_write_tokens:u",
  usage:
    "input_tokens:u output_tokens:u context_tokens:u ?cache_read_tokens:u ?cache_write_tokens:u ?window:u ?summary_at:u ?model ?by_model:{@tokens",
  agent: "agent_id name prompt_hash ?withheld:[s ?effort",
  variant: "group index:n ?model",
  cause: "?callback:[s ?mention:@called",
  called: "message_seq:n by:=calledBy ?tags:[s",
  retry: "turn_id reason:=turnEnd ?failed_model ?model",
  attachment: "name mime text",
  mention: "path state:=mention",
  reply: "message_id block_id values:{j",
  image: "media_id sha256 mime name width:n height:n",
  ui: "ordinal:n hash type:=uiKind id ?spec:j ?error",
  thinking: "model blocks:[@thought",
  thought: "type:=thought ?text",
  source: "id url title",
  cleared: "turn_id results:n",
  unthought: "turn_id steps:n",
  folded: "steps:n text",
  child: "session agent_id",
  ended: "session agent_id ?result ?usage:@usage",
  failure: "model reason",
  todo: "content status:=todoStatus",
  headAt: "?branch commit",
  headMoved: "from:@headAt to:@headAt",
};

// Every kind the fold handles.
const KINDS: Record<string, string> = {
  session_created:
    "title cwd model backend node_id approval_mode:=approvalMode profile:=profile ?agent_id ?incognito:b ?parent_session ?no_callbacks:b ?exec_mode:=execMode",
  turn_started: "turn_id ?agent:@agent ?cause:@cause ?continued_by ?retry:@retry",
  user_message: "turn_id text ?attachments:[@attachment ?files:[@mention ?reply:@reply ?images:[@image ?queued:b ?triggers:[s ?tags:[s",
  assistant_delta: "turn_id message_id text ?author ?variant:@variant",
  assistant_message: "turn_id message_id text ?author ?variant:@variant ?blocks:[@ui ?thinking:@thinking",
  tool_call_started: "turn_id call_id tool input:j ?author",
  tool_output_delta: "call_id stream:=stream text",
  tool_call_finished: "call_id ok:b output ?exit_code:n ?sources:[@source",
  approval_requested: "approval_id call_id tool input:j reason ?turn_allow:b ?chat_allow:b ?preview",
  approval_resolved: "approval_id decision:=decision by:=resolvedBy ?note",
  checkpoint: "turn_id phase:=phase git_ref ?files:[s ?head_moved:@headMoved",
  turn_ended: "turn_id reason:=turnEnd ?usage:@usage",
  turn_usage: "turn_id usage:@usage",
  error: "message",
  agent_changed: "agent:@agent ?model",
  variant_selected: "group message_id",
  history_summarized: "turn_id through_seq:n text ?manual:b ?focus ?usage:@usage ?tokens_after:n",
  context_shortened: "turn_id ?cleared:@cleared ?folded:@folded ?thinking:@unthought tokens_before:n tokens_after:n",
  model_fallback: "turn_id model from ?reason",
  model_retry: "turn_id model attempt:n of:n at_ms:n wait_ms:n reason:=retryReason",
  retry_given_up: "turn_id model",
  artifact_staged:
    "stage_id ?call_id by:=stagedBy name filename ?packed size:n sha256 ?kind_hint source_path ?notes ?meta:o",
  artifact_stage_failed: "path ?name error",
  background_started: "bg_id call_id turn_id timeout_s:n ends_by_ms:n ?child:@child",
  background_ended:
    "bg_id reason:=bgReason ?exit_code:n confirmed:=confirmed ran_ms:n output_bytes:n ?tail ?child:@ended",
  callback_refused: "bg_ids:[s reason",
  queued_delivered: "turn_id seqs:[n",
  todo_updated: "turn_id call_id ?items:[@todo",
  approval_mode_changed: "mode:=approvalMode ?requested:=approvalMode",
  context_sent: "turn_id frame:=frame",
  chat_allow_added: "grant_id approval_id tool input:j",
  chat_allow_removed: "grant_id",
  step_limit_changed: "max_steps:n ?requested:n",
  handoff_written: "through_seq:n ?text ?model ?failed:[@failure ?usage:@usage",
  compact_failed: "reason ?usage:@usage",
  images_withheld: "turn_id calls:[s status:n",
  thinking_withheld: "turn_id model status:n",
  peers_skipped: "message_seq:n agents:[s reason:=peersReason",
};

/** The guard's tables, for the test that compares them with Rust's schema. */
export const GUARD = { ENUMS, STRUCTS, KINDS };

const BAD = Symbol("bad");
// A string outside its enum: never left out as a bad optional field or array
// element would be, it drops the whole event (a newer node's value, which
// serde refuses too; rows key tables by these strings).
const UNKNOWN = Symbol("unknown value");
type Field = [name: string, optional: boolean, type: string];
const parsed = new Map<string, Field[]>();
const fields = (spec: string): Field[] => {
  let f = parsed.get(spec);
  if (!f) {
    f = spec
      .split(" ")
      .filter(Boolean)
      .map((w) => {
        const opt = w[0] === "?";
        const [name, type = "s"] = (opt ? w.slice(1) : w).split(":");
        return [name!, opt, type];
      });
    parsed.set(spec, f);
  }
  return f;
};

/** `x` as type `t`, a checked copy, BAD, or UNKNOWN (which wins over everything). */
function checked(t: string, x: unknown): unknown {
  const rest = t.slice(1);
  switch (t[0]) {
    case "s":
      return typeof x == "string" ? x : BAD;
    case "n":
      return typeof x == "number" ? x : BAD;
    // A token count: a non-negative integer a browser holds exactly. Any
    // other value is one serde's u64 refuses, and it refuses the whole event.
    case "u":
      return x === undefined ? BAD : typeof x == "number" && Number.isSafeInteger(x) && x >= 0 ? x : UNKNOWN;
    case "b":
      return typeof x == "boolean" ? x : BAD;
    case "o":
      return isObj(x) ? x : BAD;
    case "j":
      return x === undefined ? BAD : x;
    case "=":
      return typeof x != "string" ? BAD : ENUMS[rest]!.split(" ").includes(x) ? x : UNKNOWN;
    case "[": {
      if (!Array.isArray(x)) return BAD;
      const a = x.map((y) => checked(rest, y));
      return a.includes(UNKNOWN) ? UNKNOWN : a.filter((y) => y !== BAD);
    }
    case "{": {
      if (!isObj(x)) return BAD;
      const m: Record<string, unknown> = {};
      for (const k of Object.keys(x)) {
        const y = checked(rest, x[k]);
        if (y === UNKNOWN) return UNKNOWN;
        if (y !== BAD) put(m, k, y);
      }
      return m;
    }
    default:
      return struct(STRUCTS[rest]!, x);
  }
}

function struct(spec: string, x: unknown): unknown {
  if (!isObj(x)) return BAD;
  const out: Record<string, unknown> = {};
  for (const [name, opt, type] of fields(spec)) {
    const v = Object.hasOwn(x, name) ? x[name] : undefined;
    const y = opt && v == null ? BAD : checked(type, v);
    if (y === UNKNOWN) return UNKNOWN;
    if (y !== BAD) out[name] = y;
    else if (!opt) return BAD;
  }
  return out;
}

/** Counted, and one console line: the kind (cut) and seq, never the content. */
function notShown(v: SessionView, e: RawEvent) {
  v.unknown_events += 1;
  console.warn("[fold] event not shown:", String(e.kind).slice(0, 64), e.seq);
}

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
  const p = Object.hasOwn(KINDS, e.kind) ? struct(KINDS[e.kind]!, e.payload) : BAD;
  if (p === BAD || p === UNKNOWN) return notShown(v, e);
  // A cause is one variant (`callback` or `mention`), as serde reads it.
  if (e.kind === "turn_started" && (p as { cause?: object }).cause) {
    const c = (p as { cause: { callback?: unknown; mention?: unknown } }).cause;
    if ((c.callback === undefined) === (c.mention === undefined)) return notShown(v, e);
  }
  try {
    fold1(v, e, p);
  } catch {
    // No checked payload reaches this (the fuzz tests). If a fold bug does,
    // the chat stays up, but the step is not atomic: what it changed before
    // the throw (a `delete v.retry`, a pushed item) stays, and the event is
    // also counted. Accepted: making it atomic needs a copy of the view per
    // event, for a case no input reaches.
    notShown(v, e);
  }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function fold1(v: SessionView, e: RawEvent, p: any): void {
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
      break;
    }
    case "turn_started":
      // A callback turn stands where a person's message would.
      if (p.cause?.callback) v.items.push({ type: "callback", turn_id: p.turn_id, bg_ids: p.cause.callback });
      // So does the turn of an agent a message called after the first (X.37).
      if (p.cause?.mention)
        v.items.push({ type: "called", turn_id: p.turn_id, agent: p.agent ?? null, message_seq: p.cause.mention.message_seq });
      // A sub-agent's follow-up from the chat that started it (T68).
      if (p.continued_by) (v.follow_ups ??= []).push(p.turn_id);
      // A person's Retry (X.21): the failed attempt stays above it.
      if (p.retry) {
        const r: Extract<Item, { type: "retry" }> = { type: "retry", turn_id: p.turn_id, of: p.retry.turn_id, reason: p.retry.reason };
        if (p.retry.failed_model != null) r.failed_model = p.retry.failed_model;
        if (p.retry.model != null) r.model = p.retry.model;
        v.items.push(r);
      }
      {
        // A turn that never ended keeps what it had used.
        const s = spentOf(v);
        if (s.live) {
          addUsage(s.settled, s.live);
          delete s.live;
          delete s.turn;
          showUsage(v, s);
        }
      }
      v.running_turn = p.turn_id;
      v.running_agent = p.agent ?? null;
      break;
    case "callback_refused":
      v.items.push({ type: "callback_refused", bg_ids: p.bg_ids, reason: p.reason });
      break;
    case "peers_skipped":
      v.items.push({ type: "peers_skipped", agents: p.agents, reason: p.reason });
      break;
    case "handoff_written": {
      // Its cost is the chat's, like a turn's (the sums only).
      if (p.usage) settle(v, p.usage);
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
      if (p.triggers?.length) item.triggers = p.triggers;
      if (p.tags?.length) item.tags = p.tags;
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
      // Its thinking, a row before what the step said (X.25), as the Rust view.
      // Each block's kind and text (`for_browser` sent no more; the guard keeps no more).
      if (p.thinking) v.items.splice(a ? v.items.indexOf(a) : v.items.length, 0, { type: "thinking", message_id: p.message_id, blocks: p.thinking.blocks });
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
      for (const id of p.calls) {
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
        if (typeof p.preview === "string") t.approval.preview = p.preview;
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
      const c = own(v.checkpoints, p.turn_id) ?? put(v.checkpoints, p.turn_id, { before: null, after: null, files: [], restored: false });
      if (p.phase === "before") c.before = p.git_ref;
      else if (p.phase === "before_restore") {
        // A restore's own checkpoint is undone like a turn's.
        c.before = p.git_ref;
        const now = new Map<string, string>();
        for (const [k, o] of Object.entries(v.checkpoints)) if (o.restored) now.set(k, o.saved ?? "");
        const list = (undoneBefore.get(v) ?? []).filter(([k]) => k !== p.turn_id);
        list.push([p.turn_id, now]);
        if (list.length > MAX_SAVED) list.shift();
        undoneBefore.set(v, list);
        saving.set(v, p.turn_id);
      } else if (p.phase === "after") {
        c.after = p.git_ref;
        c.files = p.files ?? [];
        if (p.head_moved) c.head_moved = p.head_moved;
        else delete c.head_moved;
      } else if (p.phase === "restored") {
        const back = undoneBefore.get(v)?.find(([k]) => k === p.turn_id)?.[1];
        if (back) {
          // A saved id restored: the folder is as it was just before the
          // restore that saved it.
          for (const [k, o] of Object.entries(v.checkpoints)) {
            const by = back.get(k);
            if (by !== undefined) {
              o.restored = true;
              if (by) o.saved = by;
              else delete o.saved;
            } else {
              o.restored = false;
              delete o.saved;
            }
          }
        } else {
          c.restored = true;
          const s = saving.get(v);
          if (s != null) c.saved = s;
          else delete c.saved;
        }
      }
      break;
    }
    case "turn_ended": {
      // X.39: the end's usage REPLACES the running figure; one without keeps
      // the last running one as the turn's own.
      const s = spentOf(v);
      const running = s.turn === p.turn_id ? s.live : undefined;
      if (running) {
        delete s.live;
        delete s.turn;
      }
      if (p.usage) {
        addUsage(s.settled, p.usage);
        v.context_tokens = p.usage.context_tokens;
        if (p.usage.window != null) v.context_window = p.usage.window;
        else delete v.context_window;
        if (p.usage.summary_at != null) v.summary_at = p.usage.summary_at;
        else delete v.summary_at;
        if (p.usage.model != null) v.context_model = p.usage.model;
        else delete v.context_model;
      } else if (running) addUsage(s.settled, running);
      showUsage(v, s);
      // The turn that ended, also when it reported none (an interrupted
      // one): never an older turn's as the last.
      const last = p.usage ?? running;
      if (last) v.turn_usage = cappedUsage(last);
      else delete v.turn_usage;
      v.running_turn = null;
      v.running_agent = null;
      v.items.push({ type: "turn_end", turn_id: p.turn_id, reason: p.reason });
      break;
    }
    case "turn_usage": {
      // X.39: what the running turn has used so far, a running total. Only
      // the running turn's counts, a total below the last is stale, and a
      // late one (its turn ended) changes nothing: the fold trusts no order.
      if (v.running_turn !== p.turn_id) break;
      const s = spentOf(v);
      if (s.turn === p.turn_id && s.live && sumTokens(p.usage) < sumTokens(s.live)) break;
      s.turn = p.turn_id;
      s.live = cappedUsage(p.usage);
      // A summary call's usage has no context size of its own (0): the
      // meter keeps the last one. The turn's limits are applied as
      // `turn_ended` does.
      if (p.usage.context_tokens > 0) v.context_tokens = p.usage.context_tokens;
      if (p.usage.window != null) v.context_window = p.usage.window;
      else delete v.context_window;
      if (p.usage.summary_at != null) v.summary_at = p.usage.summary_at;
      else delete v.summary_at;
      if (p.usage.model != null) v.context_model = p.usage.model;
      else delete v.context_model;
      v.turn_usage = cappedUsage(p.usage);
      showUsage(v, s);
      break;
    }
    case "error":
      v.items.push({ type: "error", message: p.message });
      break;
    case "agent_changed": {
      const it: Extract<Item, { type: "agent_changed" }> = { type: "agent_changed", agent: p.agent };
      if (p.model != null) it.model = p.model;
      v.items.push(it);
      break;
    }
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
      put(v.variant_selection, p.group, p.message_id);
      break;
    case "history_summarized": {
      // A person's /compact (X.20): its cost is the chat's, and the meter
      // shows the shorter context until the next turn reports its own.
      if (p.usage) settle(v, p.usage);
      if (p.tokens_after != null) v.context_tokens = p.tokens_after;
      const it: Extract<Item, { type: "summary" }> = { type: "summary", through_seq: p.through_seq, text: p.text };
      if (p.manual === true) it.manual = true;
      if (p.focus != null) it.focus = p.focus;
      v.items.push(it);
      break;
    }
    case "compact_failed":
      if (p.usage) settle(v, p.usage);
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
      if (p.thinking != null) it.thinking = true;
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
    case "background_started": {
      const b: BackgroundState = { call_id: p.call_id, ends_by_ms: p.ends_by_ms, timeout_s: p.timeout_s, ended: false };
      // A background sub-agent (X.24), not a command.
      if (p.child) b.child = p.child;
      put((v.background ??= {}), p.bg_id, b);
      break;
    }
    case "background_ended": {
      const b = v.background && own(v.background, p.bg_id);
      if (b) b.ended = true;
      const c = p.child;
      // Its cost is the chat's, as a foreground sub-agent's is its turn's.
      if (c?.usage) settle(v, c.usage);
      const it: Extract<Item, { type: "background_ended" }> = {
        type: "background_ended",
        bg_id: p.bg_id,
        reason: p.reason,
        exit_code: p.exit_code ?? null,
        ran_ms: p.ran_ms,
        tail: (c ? c.result : p.tail) ?? "",
      };
      if (c) it.agent = c.agent_id;
      v.items.push(it);
      break;
    }
    case "chat_allow_added":
      put((v.chat_allows ??= {}), p.grant_id, { tool: p.tool, input: p.input });
      break;
    case "chat_allow_removed":
      if (v.chat_allows) {
        delete v.chat_allows[p.grant_id];
        if (Object.keys(v.chat_allows).length === 0) delete v.chat_allows;
      }
      break;
    case "todo_updated": {
      // The whole list each time; an empty one is no list (serde omits it).
      const items: TodoItem[] = p.items ?? [];
      if (items.length) v.todo = items;
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
