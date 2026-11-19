// X.39: the chat's figures during a turn. The same events and the same
// expected figures as crates/core/tests/live_usage_fixture.rs; one rule: the
// finished turns plus the running turn's latest `turn_usage`, which
// `turn_ended.usage` replaces (never adds to) and a `turn_ended` without usage
// keeps; a duplicate, lower, foreign or late `turn_usage` changes nothing.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { apply, emptyView, fold } from "./fold";
import type { RawEvent, SessionView } from "./types";

const fixture = JSON.parse(readFileSync(new URL("./__fixtures__/live_usage.json", import.meta.url), "utf8"));
const events = fixture.events as RawEvent[];

const figures = (v: SessionView) => ({
  input: v.usage.input_tokens,
  output: v.usage.output_tokens,
  context: v.context_tokens,
  by_model_input: v.usage.by_model?.["p::m"]?.input_tokens ?? 0,
  turn: v.turn_usage ? [v.turn_usage.input_tokens, v.turn_usage.output_tokens] : null,
  window: v.context_window ?? null,
  summary_at: v.summary_at ?? null,
  context_model: v.context_model ?? null,
  unknown_events: v.unknown_events,
});

describe("usage during a turn", () => {
  it("follows the rule after every event, as the Rust fold does", () => {
    expect(fixture.expected).toHaveLength(events.length);
    const v = emptyView();
    events.forEach((e, i) => {
      apply(v, e);
      const { seq, ...want } = fixture.expected[i];
      expect(seq).toBe(e.seq);
      // Only the figures an entry names are checked.
      const got = Object.fromEntries(Object.entries(figures(v)).filter(([k]) => k in want));
      expect(got, `after seq ${e.seq}`).toStrictEqual(want);
    });
    expect(v.unknown_events).toBe(5);
  });

  it("folds the same events twice to the same view", () => {
    expect(fold(events)).toStrictEqual(fold(events));
  });

  it("adds up to the safe-integer ceiling and no further", () => {
    const big = (n: number) => ({ input_tokens: n, output_tokens: 0, context_tokens: 0 });
    const v = emptyView();
    const ev = (seq: number, kind: string, payload: object) => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
    apply(v, ev(1, "turn_started", { turn_id: "t" }));
    apply(v, ev(2, "turn_ended", { turn_id: "t", reason: "completed", usage: big(Number.MAX_SAFE_INTEGER) }));
    apply(v, ev(3, "turn_started", { turn_id: "u" }));
    apply(v, ev(4, "turn_ended", { turn_id: "u", reason: "completed", usage: big(Number.MAX_SAFE_INTEGER) }));
    expect(v.usage.input_tokens).toBe(Number.MAX_SAFE_INTEGER);
    // Past the ceiling is not an integer a browser can hold: the event is dropped.
    apply(v, ev(5, "turn_ended", { turn_id: "u", reason: "completed", usage: big(2 ** 53) }));
    expect(v.unknown_events).toBe(1);
  });

  it("shows the running figure in a turn that has not ended", () => {
    const v = fold(events.slice(0, 5));
    expect(v.running_turn).toBe("t1");
    expect(v.usage.input_tokens + v.usage.output_tokens).toBe(280);
    expect(v.context_tokens).toBe(150);
  });
});
