// What a finished turn used, for the row under its final answer.
//
// The number is the turn's final usage as the log recorded it
// (`turn_ended.usage`, kept on the turn's end item by both folds): input
// tokens (cache reads and writes included, as providers count them) and
// output tokens, its foreground sub-agents INCLUDED, because the node adds a
// delegate call's result usage to the parent's turn total (crates/store/src/
// usage.rs, the same rule as the chat header). The sub-agents listed in the
// hover are read from their own chats and are shown as a share of that
// number, never added to it. A background sub-agent's cost reaches the chat
// at `background_ended`, not in a turn, so it is not listed here.
import type { Item, SessionView, Usage } from "../types";

export interface TurnChild {
  child: string;
  /** The agent id the delegate call named. */
  agent: string;
}

export interface TurnFigures {
  /** When the turn ended (ms). */
  at: number;
  /** When the turn before it ended (ms; 0 for the first): a sub-agent turn that ended after this and by `at` is this turn's. */
  after: number;
  usage: Usage | undefined;
  /** The foreground sub-agents the turn called, once each. */
  children: TurnChild[];
}

/** The first line of a delegate result: the child's session (the node's own line, never a uuid the model wrote). */
const CHILD = /^session: ([0-9a-f-]{36})(?:\n|$)/;

/** The turn that ends at `items[end]` (a `turn_end`). */
export function turnFigures(view: SessionView, end: number): TurnFigures | null {
  const e = view.items[end];
  if (e?.type !== "turn_end") return null;
  let from = 0;
  let after = 0;
  for (let k = end - 1; k >= 0; k--) {
    const x = view.items[k]!;
    if (x.type === "turn_end") {
      after = x.at ?? 0;
      from = k + 1;
      break;
    }
  }
  const background = new Set(Object.values(view.background ?? {}).map((b) => b.call_id));
  const children: TurnChild[] = [];
  for (let k = from; k < end; k++) {
    const x = view.items[k]!;
    if (x.type !== "tool" || x.tool !== "delegate" || background.has(x.call_id)) continue;
    const id = CHILD.exec(x.result?.output ?? "")?.[1];
    if (!id || children.some((c) => c.child === id)) continue;
    children.push({ child: id, agent: typeof x.input?.agent === "string" ? x.input.agent : "?" });
  }
  return { at: e.at ?? 0, after, usage: e.usage, children };
}

/** What a sub-agent's chat used in the turns that ended in `(after, at]`: its figures for this one parent turn, however many turns the chat has had. Null when none ended there. */
export function childUsage(child: SessionView, after: number, at: number): Usage | null {
  let sum: Usage | null = null;
  for (const it of child.items) {
    if (it.type !== "turn_end" || !it.usage || (it.at ?? 0) <= after || (it.at ?? 0) > at) continue;
    sum ??= { input_tokens: 0, output_tokens: 0, context_tokens: 0 };
    sum.input_tokens += it.usage.input_tokens;
    sum.output_tokens += it.usage.output_tokens;
    if (it.usage.cache_read_tokens) sum.cache_read_tokens = (sum.cache_read_tokens ?? 0) + it.usage.cache_read_tokens;
    if (it.usage.cache_write_tokens) sum.cache_write_tokens = (sum.cache_write_tokens ?? 0) + it.usage.cache_write_tokens;
    for (const [m, t] of Object.entries(it.usage.by_model ?? {})) {
      const by = (sum.by_model ??= {});
      const mine = (by[m] ??= { input_tokens: 0, output_tokens: 0 });
      mine.input_tokens += t.input_tokens;
      mine.output_tokens += t.output_tokens;
      if (t.cache_read_tokens) mine.cache_read_tokens = (mine.cache_read_tokens ?? 0) + t.cache_read_tokens;
      if (t.cache_write_tokens) mine.cache_write_tokens = (mine.cache_write_tokens ?? 0) + t.cache_write_tokens;
    }
  }
  return sum;
}

/** "12.3k": one decimal from a thousand, "1.2M" from a million. */
export function shortTokens(n: number): string {
  const f = (v: number, s: string) => `${v.toFixed(1).replace(/\.0$/, "")}${s}`;
  if (n >= 1_000_000) return f(n / 1_000_000, "M");
  if (n >= 1000) return f(n / 1000, "k");
  return String(n);
}

/** Which assistant items are the final message of a finished turn, with the index of that turn's end: a row of actions goes under each. One pass; read the items as they are now (they are mutated in place). A parallel-answer group (variants) has no such row. */
export function answerEnds(items: readonly Item[]): Map<number, number> {
  const out = new Map<number, number>();
  let end = -1;
  let seen = false;
  for (let i = items.length - 1; i >= 0; i--) {
    const x = items[i]!;
    if (x.type === "turn_end") {
      end = i;
      seen = false;
    } else if (x.type === "user" && !x.queued) {
      end = -1;
    } else if (x.type === "assistant" && !x.variant) {
      if (end >= 0 && !seen && x.text !== "") out.set(i, end);
      seen = true;
    }
  }
  return out;
}
