// @vitest-environment happy-dom
// The header's figures while a turn waits for its foreground sub-agents:
// the parent's own total plus each such child's latest known total, from the
// moment the child is first seen running in the turn until its call has a
// result (ended or not); the node logs the total that takes a child in only
// after that call's result, so nothing is counted twice in either arrival
// order of the two logs; a floor, tokens and cost each on their own, covers
// the gap between a child's result and the parent's next total.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test } from "vitest";
import { apply, emptyView } from "../fold";
import type { ModelInfo, RawEvent, SessionView, Usage } from "../types";
import { estimate } from "./cost";
import { headerFigures, runningChildren, useHeaderFigures } from "./childcost";

const P = "00000000-0000-7000-8000-00000000000a";
const C1 = "01a19bf2-0000-7000-8000-0000000000c1";
const C2 = "01a19bf2-0000-7000-8000-0000000000c2";
const MODELS = [
  { id: "p::m", raw: "m", provider: "P", provider_id: "p", price: { input: 1000, output: 1000, currency: "USD" } },
] as unknown as ModelInfo[];

const use = (input: number, output: number, model = "p::m"): Usage => ({
  input_tokens: input,
  output_tokens: output,
  context_tokens: input,
  model,
  by_model: { [model]: { input_tokens: input, output_tokens: output } },
});
/** The session's next event, as the log would carry it. */
const push = (v: SessionView, sid: string, kind: string, payload: object) =>
  apply(v, { session_id: sid, seq: v.head_seq + 1, ts_ms: 0, kind, payload } as RawEvent);
const play = (sid: string, ...evs: [string, object][]) => {
  const v = emptyView();
  for (const [k, p] of evs) push(v, sid, k, p);
  return v;
};

/** A parent whose turn t runs and has delegated the calls named (open: no result yet). */
function parent(calls: { id: string; child: string; open: boolean; bg?: boolean }[], own?: Usage, turn = "t"): SessionView {
  const evs: [string, object][] = [
    ["user_message", { turn_id: turn, text: "go" }],
    ["turn_started", { turn_id: turn }],
  ];
  if (own) evs.push(["turn_usage", { turn_id: turn, usage: own }]);
  for (const c of calls) {
    evs.push(["tool_call_started", { turn_id: turn, call_id: c.id, tool: "delegate", input: { agent: "coder", task: "x" } }]);
    evs.push(["tool_output_delta", { call_id: c.id, stream: "stdout", text: `session: ${c.child}\n` }]);
    if (!c.open) evs.push(["tool_call_finished", { call_id: c.id, ok: true, output: `session: ${c.child}\nSub-agent @coder finished: done.` }]);
  }
  return play(P, ...evs);
}
/** A child whose own turn runs (or ended) with the running figure given. */
function child(sid: string, u: Usage | null, ended = false): SessionView {
  const evs: [string, object][] = [["turn_started", { turn_id: "ct" }]];
  if (u) evs.push(["turn_usage", { turn_id: "ct", usage: u }]);
  if (ended) evs.push(["turn_ended", { turn_id: "ct", reason: "completed", ...(u ? { usage: u } : {}) }]);
  return play(sid, ...evs);
}
const views = (m: Record<string, SessionView>) => (id: string) => m[id];
const tokens = (u: Usage) => u.input_tokens + u.output_tokens;

describe("running children of the turn", () => {
  test("only foreground delegates still without a result", () => {
    const v = parent([
      { id: "a", child: C1, open: true },
      { id: "b", child: C2, open: false },
    ]);
    expect(runningChildren(v)).toEqual([C1]);
  });
  test("none when the chat's turn is not running", () => {
    const v = parent([{ id: "a", child: C1, open: true }]);
    v.running_turn = null;
    expect(runningChildren(v)).toEqual([]);
  });
  test("a call left open by an earlier turn is not the running turn's", () => {
    const v = parent([{ id: "a", child: C1, open: true }]);
    push(v, P, "turn_ended", { turn_id: "t", reason: "interrupted" });
    push(v, P, "user_message", { turn_id: "t2", text: "again" });
    push(v, P, "turn_started", { turn_id: "t2" });
    expect(runningChildren(v)).toEqual([]);
  });
  test("a background sub-agent is not part of the turn", () => {
    const v = parent([{ id: "a", child: C1, open: true }]);
    v.background = { "bg-1": { call_id: "a", ends_by_ms: 1, timeout_s: 1800, ended: false, child: { session: C1, agent_id: "coder" } } } as never;
    expect(runningChildren(v)).toEqual([]);
  });
  test("one child twice (follow-ups at once) counts once", () => {
    const v = parent([
      { id: "a", child: C1, open: true },
      { id: "b", child: C1, open: true },
    ]);
    expect(runningChildren(v)).toEqual([C1]);
  });
});

describe("the header's figures with running children", () => {
  type R = ReturnType<typeof headerFigures>;
  const f = (p: SessionView, m: Record<string, SessionView>, prev?: R) => headerFigures(p, views(m), prev, MODELS);
  const cost = (u: Usage) => estimate(u, MODELS).amount;
  const result = (p: SessionView, id: string, child: string, ok = true) =>
    push(p, P, "tool_call_finished", { call_id: id, ok, output: `session: ${child}\nSub-agent @coder finished: done.` });
  const total = (p: SessionView, u: Usage) => push(p, P, "turn_usage", { turn_id: "t", usage: u });

  test("no children: the chat's own figures, and no floor", () => {
    const p = parent([], use(1000, 100));
    const r = f(p, {});
    expect(r.usage).toBe(p.usage);
    expect(r.engaged).toBe(false);
  });

  test("one running child adds its running total, tokens and cost", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const r = f(p, { [C1]: child(C1, use(400, 50)) });
    expect(tokens(r.usage)).toBe(1550);
    expect(cost(r.costUsage)).toBeCloseTo(1.55);
    expect(r.engaged).toBe(true);
    expect(tokens(p.usage)).toBe(1100);
  });

  test("it follows the child's next turn_usage while the parent's log is silent", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const c = child(C1, use(400, 50));
    const r1 = f(p, { [C1]: c });
    push(c, C1, "turn_usage", { turn_id: "ct", usage: use(900, 120) });
    expect(tokens(f(p, { [C1]: c }, r1).usage)).toBe(1100 + 1020);
  });

  // The real log order: the child's result, THEN the parent's total that
  // takes it in. Both arrival orders of the two logs.
  test("real order, the child's log ahead: its end shows before the parent's result", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const r1 = f(p, { [C1]: child(C1, use(900, 120)) });
    // The child ended; the parent has no result yet: still counted.
    const r2 = f(p, { [C1]: child(C1, use(900, 120), true) }, r1);
    expect(tokens(r2.usage)).toBe(1100 + 1020);
    result(p, "a", C1);
    // Result in, parent's total not yet: held by the floor.
    const r3 = f(p, { [C1]: child(C1, use(900, 120), true) }, r2);
    expect(tokens(r3.usage)).toBe(1100 + 1020);
    total(p, use(1900, 250));
    const r4 = f(p, { [C1]: child(C1, use(900, 120), true) }, r3);
    expect(tokens(r4.usage)).toBe(tokens(p.usage));
    expect(tokens(r4.usage)).toBe(2150);
  });

  test("real order, the parent's log ahead: result and total arrive before the child's end", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const r1 = f(p, { [C1]: child(C1, use(900, 120)) });
    result(p, "a", C1);
    total(p, use(1900, 250));
    // The child's view still says running: the row has its result, no double count.
    const r2 = f(p, { [C1]: child(C1, use(900, 120)) }, r1);
    expect(tokens(r2.usage)).toBe(2150);
    expect(tokens(r2.usage)).toBe(tokens(p.usage));
    expect(cost(r2.costUsage)).toBeCloseTo(2.15);
  });

  test("parallel children: the first ends and waits for the join, the second's growth still shows", () => {
    const p = parent(
      [
        { id: "a", child: C1, open: true },
        { id: "b", child: C2, open: true },
      ],
      use(1000, 100),
    );
    const r1 = f(p, { [C1]: child(C1, use(5000, 0)), [C2]: child(C2, use(100, 0)) });
    expect(tokens(r1.usage)).toBe(1100 + 5000 + 100);
    const r2 = f(p, { [C1]: child(C1, use(5000, 0), true), [C2]: child(C2, use(1000, 0)) }, r1);
    expect(tokens(r2.usage)).toBe(1100 + 5000 + 1000);
    const r3 = f(p, { [C1]: child(C1, use(5000, 0), true), [C2]: child(C2, use(3000, 0)) }, r2);
    expect(tokens(r3.usage)).toBe(1100 + 5000 + 3000);
    // The join: results and totals in call order, each total after its result.
    result(p, "a", C1);
    total(p, use(6000, 100));
    const r4 = f(p, { [C1]: child(C1, use(5000, 0), true), [C2]: child(C2, use(3000, 0), true) }, r3);
    expect(tokens(r4.usage)).toBe(6100 + 3000);
    result(p, "b", C2);
    total(p, use(9000, 100));
    const r5 = f(p, { [C1]: child(C1, use(5000, 0), true), [C2]: child(C2, use(3000, 0), true) }, r4);
    expect(tokens(r5.usage)).toBe(9100);
    expect(tokens(r5.usage)).toBe(tokens(p.usage));
  });

  test("a child that ended with a final figure above the last one seen counts that one", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const r1 = f(p, { [C1]: child(C1, use(400, 50)) });
    const r2 = f(p, { [C1]: child(C1, use(900, 120), true) }, r1);
    expect(tokens(r2.usage)).toBe(1100 + 1020);
  });

  test("a follow-up child is not counted before its new turn is seen running, and is after", () => {
    // Its view still shows an earlier finished turn.
    const old = child(C1, use(5000, 500), true);
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const r1 = f(p, { [C1]: old });
    expect(tokens(r1.usage)).toBe(1100);
    expect(r1.engaged).toBe(false);
    push(old, C1, "turn_started", { turn_id: "ct2" });
    // Running, nothing reported yet: seen, adds nothing.
    const r2 = f(p, { [C1]: old }, r1);
    expect(tokens(r2.usage)).toBe(1100);
    push(old, C1, "turn_usage", { turn_id: "ct2", usage: use(300, 20) });
    const r3 = f(p, { [C1]: old }, r2);
    expect(tokens(r3.usage)).toBe(1100 + 320);
  });

  test("two running children both count", () => {
    const p = parent(
      [
        { id: "a", child: C1, open: true },
        { id: "b", child: C2, open: true },
      ],
      use(1000, 100),
    );
    const r = f(p, { [C1]: child(C1, use(400, 50)), [C2]: child(C2, use(200, 10)) });
    expect(tokens(r.usage)).toBe(1100 + 450 + 210);
  });

  test("a child on a model with no price makes the cost partial", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const e = estimate(f(p, { [C1]: child(C1, use(400, 50, "x::unpriced")) }).costUsage, MODELS);
    expect(e.unpricedTokens).toBe(450);
    expect(e.unpricedModels).toEqual(["x::unpriced"]);
    expect(e.amount).toBeCloseTo(1.1);
  });

  test("a child not yet loaded, or silent, adds nothing", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    expect(tokens(f(p, {}).usage)).toBe(1100);
    expect(tokens(f(p, { [C1]: child(C1, null) }).usage)).toBe(1100);
  });

  test("another chat's child, or a call of an earlier turn, is ignored", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    expect(tokens(f(p, { [C2]: child(C2, use(900, 90)) }).usage)).toBe(1100);
    const q = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    push(q, P, "turn_ended", { turn_id: "t", reason: "interrupted" });
    push(q, P, "turn_started", { turn_id: "t2" });
    expect(tokens(f(q, { [C1]: child(C1, use(900, 90)) }).usage)).toBe(tokens(q.usage));
  });

  test("a failed child's cost is shown while it runs and leaves with the turn", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const r1 = f(p, { [C1]: child(C1, use(9000, 0)) });
    result(p, "a", C1, false);
    const r2 = f(p, { [C1]: child(C1, use(9000, 0), true) }, r1);
    expect(tokens(r2.usage)).toBe(10100);
    push(p, P, "turn_ended", { turn_id: "t", reason: "completed", usage: use(1000, 100) });
    const r3 = f(p, {}, r2);
    expect(r3.usage).toBe(p.usage);
    expect(r3.engaged).toBe(false);
  });

  test("the turn's own figure carries the children too", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    expect(tokens(f(p, { [C1]: child(C1, use(400, 50)) }).turn!)).toBe(1550);
  });

  test("the floor holds tokens and cost on their own", () => {
    // Prices: only p::m is priced; x::free tokens cost nothing counted (unpriced).
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const base = f(p, { [C1]: child(C1, use(1000, 0)) });
    expect(tokens(base.usage)).toBe(2100);
    // Tokens up, estimate down: the child is now on an unpriced model (its figure was replaced).
    const kid = child(C1, use(3000, 0, "x::unpriced"));
    const mixed = f(p, { [C1]: kid }, base);
    expect(tokens(mixed.usage)).toBeGreaterThan(tokens(base.usage));
    expect(cost(mixed.costUsage)).toBeCloseTo(cost(base.costUsage));
    // The reverse: tokens down, estimate up (a smaller figure on the priced, dearer side).
    const base2 = f(p, { [C1]: child(C1, use(3000, 0, "x::unpriced")) });
    const rev = f(p, { [C1]: child(C1, use(500, 500)) }, base2);
    expect(tokens(rev.usage)).toBe(tokens(base2.usage));
    expect(cost(rev.costUsage)).toBeGreaterThan(cost(base2.costUsage));
  });
});

describe("watching the children", () => {
  test("a child that stays is not re-subscribed when a sibling ends", async () => {
    const calls: string[] = [];
    const live = { watch: (id: string) => calls.push(`+${id}`), unwatch: (id: string) => calls.push(`-${id}`), view: () => undefined } as never;
    let p = parent(
      [
        { id: "a", child: C1, open: true },
        { id: "b", child: C2, open: true },
      ],
      use(1000, 100),
    );
    function Probe() {
      useHeaderFigures(live, P, p, MODELS);
      return null;
    }
    const root = createRoot(document.createElement("div"));
    await act(async () => root.render(<Probe />));
    expect(calls.sort()).toEqual([`+${C1}`, `+${C2}`]);
    calls.length = 0;
    p = parent(
      [
        { id: "a", child: C1, open: false },
        { id: "b", child: C2, open: true },
      ],
      use(1000, 100),
    );
    await act(async () => root.render(<Probe />));
    expect(calls).toEqual([`-${C1}`]);
    await act(async () => root.unmount());
    expect(calls).toEqual([`-${C1}`, `-${C2}`]);
  });
});
