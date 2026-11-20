// The header's figures while a turn waits for its foreground sub-agents:
// the parent's own total plus each such child's running total, a child
// counted only while it runs (once it has ended its cost arrives through the
// parent's own events), and never less than what the turn already showed.
import { describe, expect, test } from "vitest";
import { apply, emptyView } from "../fold";
import type { ModelInfo, RawEvent, SessionView, Usage } from "../types";
import { estimate } from "./cost";
import { headerFigures, runningChildren } from "./childcost";

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
  const f = (p: SessionView, m: Record<string, SessionView>, prev?: ReturnType<typeof headerFigures>) =>
    headerFigures(p, views(m), prev, MODELS);

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
    expect(estimate(r.usage, MODELS).amount).toBeCloseTo(1.55);
    expect(r.engaged).toBe(true);
    // The chat's own figure is not changed by it.
    expect(tokens(p.usage)).toBe(1100);
  });

  test("it follows the child's next turn_usage while the parent's log is silent", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const c = child(C1, use(400, 50));
    const r1 = f(p, { [C1]: c });
    push(c, C1, "turn_usage", { turn_id: "ct", usage: use(900, 120) });
    const r2 = f(p, { [C1]: c }, r1);
    expect(tokens(r2.usage)).toBe(1100 + 1020);
  });

  test("a child that ended before the parent caught up does not make the total dip", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const r1 = f(p, { [C1]: child(C1, use(900, 120)) });
    // The child's log shows its end; the parent has no result yet.
    const r2 = f(p, { [C1]: child(C1, use(900, 120), true) }, r1);
    expect(tokens(r2.usage)).toBe(tokens(r1.usage));
    expect(estimate(r2.usage, MODELS).amount).toBeCloseTo(estimate(r1.usage, MODELS).amount);
  });

  test("a child that ended and the parent caught up: the parent's own total, no double count", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const r1 = f(p, { [C1]: child(C1, use(900, 120)) });
    // The parent's next turn_usage carries the child's cost, its result is in.
    push(p, P, "tool_call_finished", { call_id: "a", ok: true, output: `session: ${C1}\nSub-agent @coder finished: done.` });
    push(p, P, "turn_usage", { turn_id: "t", usage: use(1000 + 900, 100 + 120 + 30) });
    const r2 = f(p, { [C1]: child(C1, use(900, 120), true) }, r1);
    expect(tokens(r2.usage)).toBe(tokens(p.usage));
    expect(r2.usage.by_model!["p::m"]!.input_tokens).toBe(p.usage.by_model!["p::m"]!.input_tokens);
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
    const r = f(p, { [C1]: child(C1, use(400, 50, "x::unpriced")) });
    const e = estimate(r.usage, MODELS);
    expect(e.unpricedTokens).toBe(450);
    expect(e.unpricedModels).toEqual(["x::unpriced"]);
    expect(e.amount).toBeCloseTo(1.1);
  });

  test("a child not yet loaded, or silent, adds nothing", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    expect(tokens(f(p, {}).usage)).toBe(1100);
    expect(tokens(f(p, { [C1]: child(C1, null) }).usage)).toBe(1100);
  });

  test("a child of ANOTHER turn or chat is ignored", () => {
    // A follow-up's earlier turn figure in the child's view is not its running one.
    const old = child(C1, use(5000, 500), true);
    push(old, C1, "turn_started", { turn_id: "ct2" });
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    expect(tokens(f(p, { [C1]: old }).usage)).toBe(1100);
    // Another chat's child is not among this chat's calls at all.
    expect(tokens(f(p, { [C2]: child(C2, use(900, 90)) }).usage)).toBe(1100);
    // A call of an earlier turn.
    const q = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    push(q, P, "turn_ended", { turn_id: "t", reason: "interrupted" });
    push(q, P, "turn_started", { turn_id: "t2" });
    expect(tokens(f(q, { [C1]: child(C1, use(900, 90)) }).usage)).toBe(tokens(q.usage));
  });

  test("the floor is dropped when the turn ends: the log's figure alone", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const r1 = f(p, { [C1]: child(C1, use(900, 120)) });
    push(p, P, "turn_ended", { turn_id: "t", reason: "interrupted" });
    const r2 = f(p, { [C1]: child(C1, use(900, 120), true) }, r1);
    expect(r2.usage).toBe(p.usage);
    expect(r2.engaged).toBe(false);
  });

  test("the turn's own figure carries the children too", () => {
    const p = parent([{ id: "a", child: C1, open: true }], use(1000, 100));
    const r = f(p, { [C1]: child(C1, use(400, 50)) });
    expect(tokens(r.turn!)).toBe(1550);
  });
});
