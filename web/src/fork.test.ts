import { describe, expect, it } from "vitest";
import { fold } from "./fold";
import { readFileSync } from "node:fs";
import type { RawEvent } from "./types";

// Parsed, not imported (see fold.test.ts).
const fixture = JSON.parse(readFileSync(new URL("./__fixtures__/fork.json", import.meta.url), "utf8"));

describe("a forked chat's fold (C.2)", () => {
  it("matches the Rust reducer on the shared fork fixture", () => {
    expect(fold(fixture.events as RawEvent[])).toStrictEqual(fixture.expected);
  });

  it("costs nothing for the parent's turns, grants nothing, and marks their checkpoints", () => {
    const v = fold(fixture.events as RawEvent[]);
    expect(v.usage.input_tokens).toBe(40);
    expect(Object.keys(v.chat_allows ?? {})).toEqual(["g-own"]);
    expect(v.checkpoints["t1"]?.inherited).toBe(true);
    expect(v.checkpoints["t2"]?.inherited).toBe(true);
    expect(v.checkpoints["u1"]?.inherited).toBeUndefined();
    expect(v.background?.["bg-1"]?.ended).toBe(true);
    const at = v.items.findIndex((i) => i.type === "forked");
    expect(v.items[at - 1]).toMatchObject({ type: "turn_end", turn_id: "t2" });
    expect(v.items[at]).toMatchObject({ type: "forked", turn_id: "t2" });
  });

  it("does not take the first event for a copy: a chat that is no fork inherits nothing", () => {
    const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
    const v = fold([
      ev(1, "turn_started", { turn_id: "t" }),
      ev(2, "turn_ended", { turn_id: "t", reason: "completed", usage: { input_tokens: 5, output_tokens: 1, context_tokens: 6 } }),
    ]);
    expect(v.usage.input_tokens).toBe(5);
  });
});
