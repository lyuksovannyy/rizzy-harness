// The chat header's tokens and cost while a turn waits for its foreground
// sub-agents (X.39 follow-up). The parent's own log carries a child's cost
// only when the child's result returns (its `turn_usage` stops while the
// `delegate` call waits), so a turn that is mostly one long sub-agent would
// show a flat figure and then a jump. The header therefore adds the running
// total of each such child, from the child's own log that this tab already
// follows for the side panel (the same access check as opening that chat).
//
// The log is unchanged and is the truth: a child is added only while its own
// turn runs AND its parent's call has no result yet; once the result is in,
// the parent's `turn_usage`/`turn_ended` carry that cost and the child adds
// nothing, so it is never counted twice. The two logs reach the browser
// independently, so for an instant a child can look ended while the parent
// has not caught up: within one running turn the shown figures keep the
// last shown value as a floor (dropped when the turn ends, when the log's
// own figure is shown exactly as before).
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

export interface Figures {
  /** The chat's tokens and cost basis. */
  usage: Usage;
  /** The turn's own figure (the header's "this turn" part). */
  turn: Usage | undefined;
  /** Children were counted in this turn: the floor applies from now on. */
  engaged: boolean;
}

const tokens = (u: Usage) => u.input_tokens + u.output_tokens;
const zero = (): Usage => ({ input_tokens: 0, output_tokens: 0, context_tokens: 0 });

/** `next`, unless it is less than `prev` in tokens or in cost. */
function atLeast(prev: Usage | undefined, next: Usage, models: ModelInfo[]): Usage {
  if (!prev) return next;
  return tokens(next) < tokens(prev) || estimate(next, models).amount < estimate(prev, models).amount ? prev : next;
}

/** Pure. `childView`: the folded log of a session this tab follows (undefined
 * when it does not). `prev`: what the header showed last in this same turn
 * once children were counted (the floor); undefined before that. */
export function headerFigures(
  view: SessionView,
  childView: (id: string) => SessionView | undefined,
  prev: Figures | undefined,
  models: ModelInfo[],
): Figures {
  if (view.running_turn === null) return { usage: view.usage, turn: view.turn_usage, engaged: false };
  const running: Usage[] = [];
  for (const id of runningChildren(view)) {
    const c = childView(id);
    // Its own running turn's latest figure; an ended child adds nothing.
    const u = c && c.running_turn !== null ? runningUsage(c) : undefined;
    if (u) running.push(u);
  }
  if (running.length === 0 && !prev) return { usage: view.usage, turn: view.turn_usage, engaged: false };
  const usage = cappedUsage(view.usage);
  const turn = cappedUsage(runningUsage(view) ?? zero());
  for (const u of running) {
    addUsage(usage, u);
    addUsage(turn, u);
  }
  return { usage: atLeast(prev?.usage, usage, models), turn: atLeast(prev?.turn, turn, models), engaged: true };
}

/** The header's figures for the open chat: follows the running children
 * (counted watches, shared with the side panel) and keeps the floor. */
export function useHeaderFigures(live: Live, sessionId: string, view: SessionView | undefined, models: ModelInfo[]): Figures | undefined {
  const key = view ? runningChildren(view).join(",") : "";
  useEffect(() => {
    const ids = key ? key.split(",") : [];
    for (const id of ids) live.watch(id);
    return () => {
      for (const id of ids) live.unwatch(id);
    };
  }, [live, key]);
  const floor = useRef<{ session: string; turn: string; figures: Figures } | null>(null);
  if (!view) return undefined;
  const f = floor.current;
  const prev = f && f.session === sessionId && f.turn === view.running_turn ? f.figures : undefined;
  const now = headerFigures(view, (id) => live.view(id), prev, models);
  floor.current = now.engaged && view.running_turn !== null ? { session: sessionId, turn: view.running_turn, figures: now } : null;
  return now;
}
