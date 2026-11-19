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
});

describe("usage during a turn", () => {
  it("follows the rule after every event, as the Rust fold does", () => {
    expect(fixture.expected).toHaveLength(events.length);
    const v = emptyView();
    events.forEach((e, i) => {
      apply(v, e);
      const { seq, ...want } = fixture.expected[i];
      expect(seq).toBe(e.seq);
      expect(figures(v), `after seq ${e.seq}`).toStrictEqual(want);
    });
    expect(v.unknown_events).toBe(0);
  });

  it("folds the same events twice to the same view", () => {
    expect(fold(events)).toStrictEqual(fold(events));
  });

  it("shows the running figure in a turn that has not ended", () => {
    const v = fold(events.slice(0, 5));
    expect(v.running_turn).toBe("t1");
    expect(v.usage.input_tokens + v.usage.output_tokens).toBe(280);
    expect(v.context_tokens).toBe(150);
  });
});
