// What the row under an answer reads from the fold: the turn's end, its
// foreground sub-agents, the sub-agent's share of this turn only, and which
// message carries the row.
import { describe, expect, test } from "vitest";
import { fold } from "../fold";
import type { RawEvent } from "../types";
import { answerEnds, childUsage, shortTokens, turnFigures } from "./turnTokens";

const SID = "11111111-1111-1111-1111-111111111111";
const CH = "22222222-2222-2222-2222-222222222222";
const ev = (seq: number, kind: string, payload: object, ts = 1000 * seq): RawEvent => ({ session_id: SID, seq, ts_ms: ts, kind, payload }) as RawEvent;
const created = ev(1, "session_created", { title: "t", cwd: "/w", model: "demo::m", backend: "native", node_id: "n", approval_mode: "ask", profile: "agentic" });
const use = (i: number, o: number, by: Record<string, object> = {}) => ({ input_tokens: i, output_tokens: o, context_tokens: i, by_model: by });

describe("shortTokens", () => {
  test("one decimal from a thousand, a million", () => {
    expect([0, 999, 1000, 1234, 12345, 100000, 1234567].map(shortTokens)).toEqual(["0", "999", "1k", "1.2k", "12.3k", "100k", "1.2M"]);
  });
});

describe("turnFigures", () => {
  const log = [
    created,
    ev(2, "turn_started", { turn_id: "a" }),
    ev(3, "user_message", { turn_id: "a", text: "one" }),
    ev(4, "assistant_message", { turn_id: "a", message_id: "m1", text: "first" }),
    ev(5, "turn_ended", { turn_id: "a", reason: "completed", usage: use(10, 2) }),
    ev(6, "turn_started", { turn_id: "b" }),
    ev(7, "user_message", { turn_id: "b", text: "two" }),
    ev(8, "tool_call_started", { turn_id: "b", call_id: "c1", tool: "delegate", input: { agent: "reviewer", task: "x" } }),
    ev(9, "tool_call_finished", { call_id: "c1", ok: true, output: `session: ${CH}\ndone` }),
    ev(10, "tool_call_started", { turn_id: "b", call_id: "c2", tool: "delegate", input: { agent: "reviewer", task: "again" } }),
    ev(11, "tool_call_finished", { call_id: "c2", ok: true, output: `session: ${CH}\ndone` }),
    ev(12, "tool_call_started", { turn_id: "b", call_id: "c3", tool: "delegate", input: { agent: "x", task: "y" } }),
    ev(13, "tool_call_finished", { call_id: "c3", ok: false, output: `refused: session: ${"3".repeat(36)}` }),
    ev(14, "assistant_message", { turn_id: "b", message_id: "m2", text: "second" }),
    ev(15, "turn_ended", { turn_id: "b", reason: "completed", usage: use(30, 4) }),
  ];
  const v = fold(log);

  test("the end's time, the turn before it, its usage and each sub-agent once", () => {
    const ends = v.items.flatMap((x, i) => (x.type === "turn_end" ? [i] : []));
    const a = turnFigures(v, ends[0]!)!;
    expect(a).toMatchObject({ at: 5000, after: 0, children: [] });
    expect(a.usage?.input_tokens).toBe(10);
    const b = turnFigures(v, ends[1]!)!;
    expect(b).toMatchObject({ at: 15000, after: 5000, children: [{ child: CH, agent: "reviewer" }] });
    expect(b.usage?.output_tokens).toBe(4);
  });

  test("not an item that is no turn end", () => {
    expect(turnFigures(v, 0)).toBeNull();
  });

  test("a background sub-agent is not listed", () => {
    const bg = fold([
      created,
      ev(2, "turn_started", { turn_id: "a" }),
      ev(3, "user_message", { turn_id: "a", text: "go" }),
      ev(4, "tool_call_started", { turn_id: "a", call_id: "c1", tool: "delegate", input: { agent: "r", task: "x", background: true } }),
      ev(5, "background_started", { turn_id: "a", bg_id: "bg1", call_id: "c1", ends_by_ms: 0, timeout_s: 5, child: { session: CH, agent_id: "r" } }),
      ev(6, "tool_call_finished", { call_id: "c1", ok: true, output: `session: ${CH}\nstarted` }),
      ev(7, "turn_ended", { turn_id: "a", reason: "completed", usage: use(1, 1) }),
    ]);
    expect(turnFigures(bg, bg.items.length - 1)!.children).toEqual([]);
  });
});

describe("childUsage", () => {
  const child = fold([
    { ...created, session_id: CH } as RawEvent,
    ...[
      ["x", 2000, 100, 10, "m1"],
      ["y", 8000, 200, 20, "m1"],
      ["z", 12000, 300, 30, "m2"],
    ].flatMap(([id, at, i, o, m], k) => [
      ev(2 + 2 * k, "turn_started", { turn_id: id }, 1),
      ev(3 + 2 * k, "turn_ended", { turn_id: id, reason: "completed", usage: use(i as number, o as number, { [m as string]: { input_tokens: i, output_tokens: o } }) }, at as number),
    ]).map((e) => ({ ...e, session_id: CH }) as RawEvent),
  ]);

  test("only the child turns that ended in the parent turn's window", () => {
    const u = childUsage(child, 5000, 15000)!;
    expect([u.input_tokens, u.output_tokens]).toEqual([500, 50]);
    expect(Object.keys(u.by_model!).sort()).toEqual(["m1", "m2"]);
    expect(childUsage(child, 0, 5000)!.input_tokens).toBe(100);
    expect(childUsage(child, 15000, 20000)).toBeNull();
  });
});

describe("answerEnds", () => {
  test("the last answer of each finished turn; not a running turn, not an earlier step", () => {
    const v = fold([
      created,
      ev(2, "turn_started", { turn_id: "a" }),
      ev(3, "user_message", { turn_id: "a", text: "one" }),
      ev(4, "assistant_message", { turn_id: "a", message_id: "m1", text: "step" }),
      ev(5, "tool_call_started", { turn_id: "a", call_id: "c", tool: "read", input: {} }),
      ev(6, "tool_call_finished", { call_id: "c", ok: true, output: "x" }),
      ev(7, "assistant_message", { turn_id: "a", message_id: "m2", text: "final" }),
      ev(8, "turn_ended", { turn_id: "a", reason: "completed" }),
      ev(9, "turn_started", { turn_id: "b" }),
      ev(10, "user_message", { turn_id: "b", text: "two" }),
      ev(11, "assistant_message", { turn_id: "b", message_id: "m3", text: "running" }),
    ]);
    const m = answerEnds(v.items);
    const texts = [...m.entries()].map(([i, e]) => [(v.items[i] as { text: string }).text, v.items[e]!.type]);
    expect(texts).toEqual([["final", "turn_end"]]);
  });

  test("a message sent while the turn ran does not split it; a turn that never ended has no row", () => {
    const v = fold([
      created,
      ev(2, "turn_started", { turn_id: "a" }),
      ev(3, "user_message", { turn_id: "a", text: "one" }),
      ev(4, "user_message", { turn_id: "a", text: "also", queued: true }),
      ev(5, "assistant_message", { turn_id: "a", message_id: "m1", text: "final" }),
      ev(6, "turn_ended", { turn_id: "a", reason: "completed" }),
      ev(7, "turn_started", { turn_id: "b" }),
      ev(8, "user_message", { turn_id: "b", text: "two" }),
      ev(9, "assistant_message", { turn_id: "b", message_id: "m2", text: "cut off" }),
      ev(10, "turn_started", { turn_id: "c" }),
      ev(11, "user_message", { turn_id: "c", text: "three" }),
      ev(12, "assistant_message", { turn_id: "c", message_id: "m3", text: "done" }),
      ev(13, "turn_ended", { turn_id: "c", reason: "completed" }),
    ]);
    const texts = [...answerEnds(v.items).keys()].map((i) => (v.items[i] as { text: string }).text);
    expect(texts).toEqual(["done", "final"]);
  });
});
