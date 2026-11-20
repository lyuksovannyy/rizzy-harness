// The chat header's tokens and cost while a turn waits for its foreground
// sub-agents (X.39 follow-up). The parent's own log carries a child's cost
// only when the child's result returns (its `turn_usage` stops while the
// `delegate` call waits), so a turn that is mostly one long sub-agent would
// show a flat figure and then a jump. The header therefore adds the running
// total of each such child, from the child's own log that this tab already
// follows for the side panel (the same access check as opening that chat).
//
// The log is unchanged and is the truth. A child is counted from the moment
// it is first seen running in this turn until its parent's call has a
// result, ended or not (its latest known figure once it has ended: a
// parallel sibling's result is held until all have finished). The node
// reports the `turn_usage` that takes a child in only AFTER that call's
// `tool_call_finished` (crates/agent-native/src/lib.rs, the delegate site),
// so the parent's own total never includes a child whose row still has no
// result: nothing is counted twice, whatever order the two logs arrive in.
// Between a child's result and the parent's next total the shown figures
// keep the last shown value as a floor, tokens and the cost estimate each
// on their own; it is dropped when the turn ends, when the log's own
// figure is shown exactly as before.
import { useEffect, useRef } from "react";
import { addUsage, cappedUsage, runningUsage } from "../fold";
import { sideItems } from "../components/sideItems";
import type { Live } from "../live";
import type { ModelInfo, SessionView, Usage } from "../types";
import { estimate } from "./cost";

/** The children whose cost the parent has not taken in: foreground
 * `delegate` calls of the turn running now that have no result yet. */
export function runningChildren(view: SessionView): string[] {
  if (view.running_turn === null) return [];
  const ids: string[] = [];
  for (const a of sideItems(view).agents) {
    if (a.current && a.child && !a.bg_id && (a.state === "running" || a.state === "waiting") && !ids.includes(a.child)) ids.push(a.child);
  }
  return ids;
}

/** What the header remembers within one running turn of one chat. */
export interface Memo {
  /** Children seen running in this turn whose row has no result yet, with
   * their latest known usage (null: running, nothing reported yet). */
  seen: Map<string, Usage | null>;
  /** The shown figures so far, once a child was counted (the floor). */
  total?: Pair;
  turn?: Pair;
}
/** Tokens and cost are held on their own: the usage that gives the shown
 * tokens, and the one that gives the shown cost estimate. */
interface Pair {
  tokens: Usage;
  cost: Usage;
}

export interface Figures {
  /** The chat's tokens. */
  usage: Usage;
  /** The turn's own tokens (the header's "this turn" part). */
  turn: Usage | undefined;
  /** What the cost estimate is made of (the same as `usage` unless the
   * floor holds one dimension and not the other). */
  costUsage: Usage;
  costTurn: Usage | undefined;
  /** Children were counted in this turn: the floor applies from now on. */
  engaged: boolean;
  memo: Memo;
}

const tokens = (u: Usage) => u.input_tokens + u.output_tokens;
const zero = (): Usage => ({ input_tokens: 0, output_tokens: 0, context_tokens: 0 });

/** `next`, held at `prev` in each dimension that would go down. */
function floored(prev: Pair | undefined, next: Usage, models: ModelInfo[]): Pair {
  if (!prev) return { tokens: next, cost: next };
  return {
    tokens: tokens(next) >= tokens(prev.tokens) ? next : prev.tokens,
    cost: estimate(next, models).amount >= estimate(prev.cost, models).amount ? next : prev.cost,
  };
}

const logFigures = (view: SessionView, memo: Memo): Figures => ({
  usage: view.usage,
  turn: view.turn_usage,
  costUsage: view.usage,
  costTurn: view.turn_usage,
  engaged: false,
  memo,
});

/** Pure. `childView`: the folded log of a session this tab follows (undefined
 * when it does not). `prev`: what this function returned last in this same
 * turn (its memo is the floor and the children seen); undefined before. */
export function headerFigures(
  view: SessionView,
  childView: (id: string) => SessionView | undefined,
  prev: Figures | undefined,
  models: ModelInfo[],
): Figures {
  if (view.running_turn === null) return logFigures(view, { seen: new Map() });
  const before = prev?.memo;
  const seen = new Map<string, Usage | null>();
  const counted: Usage[] = [];
  for (const id of runningChildren(view)) {
    const c = childView(id);
    const last = before?.seen.get(id);
    let u: Usage | null | undefined;
    if (c && c.running_turn !== null) {
      // Seen running in this turn: from now on it counts.
      u = runningUsage(c) ?? last ?? null;
    } else if (last !== undefined) {
      // Ended (or its view is being replayed) after it was seen: its final
      // figure if the log shows one, never less than what was seen.
      const fin = c?.turn_usage;
      u = fin && (!last || tokens(fin) > tokens(last)) ? fin : last;
    }
    if (u === undefined) continue; // not seen running in this turn yet
    seen.set(id, u);
    if (u) counted.push(u);
  }
  const memo: Memo = { seen, total: before?.total, turn: before?.turn };
  if (counted.length === 0 && !memo.total) return logFigures(view, memo);
  const usage = cappedUsage(view.usage);
  const turn = cappedUsage(runningUsage(view) ?? zero());
  for (const u of counted) {
    addUsage(usage, u);
    addUsage(turn, u);
  }
  memo.total = floored(memo.total, usage, models);
  memo.turn = floored(memo.turn, turn, models);
  return {
    usage: memo.total.tokens,
    turn: memo.turn.tokens,
    costUsage: memo.total.cost,
    costTurn: memo.turn.cost,
    engaged: true,
    memo,
  };
}

/** The header's figures for the open chat: follows the running children
 * (counted watches, shared with the side panel; a sibling's end does not
 * re-subscribe the others) and keeps the memo of the turn. */
export function useHeaderFigures(live: Live, sessionId: string, view: SessionView | undefined, models: ModelInfo[]): Figures | undefined {
  const key = view ? runningChildren(view).join(",") : "";
  const watched = useRef(new Set<string>());
  useEffect(() => {
    const want = new Set(key ? key.split(",") : []);
    for (const id of [...watched.current]) {
      if (want.has(id)) continue;
      live.unwatch(id);
      watched.current.delete(id);
    }
    for (const id of want) {
      if (watched.current.has(id)) continue;
      live.watch(id);
      watched.current.add(id);
    }
  }, [live, key]);
  useEffect(() => {
    const mine = watched.current;
    return () => {
      for (const id of mine) live.unwatch(id);
      mine.clear();
    };
  }, [live]);
  const memo = useRef<{ session: string; turn: string; figures: Figures } | null>(null);
  if (!view) return undefined;
  const f = memo.current;
  const prev = f && f.session === sessionId && f.turn === view.running_turn ? f.figures : undefined;
  const now = headerFigures(view, (id) => live.view(id), prev, models);
  memo.current = view.running_turn !== null ? { session: sessionId, turn: view.running_turn, figures: now } : null;
  return now;
}
