import { describe, expect, it } from "vitest";
import { apply, emptyView, fold, FoldError } from "./fold";
import fixture from "./__fixtures__/session.json";
import type { RawEvent } from "./types";

describe("fold", () => {
  it("matches the Rust reducer on the shared fixture", () => {
    const events = fixture.events as RawEvent[];
    // Strict: a key set to undefined is not the same as a key serde omits
    // (meta.parent_session, item.reply, result.sources, …).
    expect(fold(events)).toStrictEqual(fixture.expected);
  });

  it("keeps the latest task list only, and drops a cleared one", () => {
    const sid = "11111111-1111-1111-1111-111111111111";
    const ev = (seq: number, items: unknown[]): RawEvent => ({
      session_id: sid,
      seq,
      ts_ms: 0,
      kind: "todo_updated",
      payload: { turn_id: "t", call_id: `c${seq}`, items },
    });
    const v = fold([
      ev(1, [{ content: "Read", status: "in_progress" }]),
      ev(2, [
        { content: "Read", status: "done" },
        { content: "Fix", status: "in_progress" },
      ]),
    ]);
    expect(v.todo).toStrictEqual([
      { content: "Read", status: "done" },
      { content: "Fix", status: "in_progress" },
    ]);
    apply(v, ev(3, []));
    expect("todo" in v).toBe(false);
  });

  it("rejects duplicates and gaps", () => {
    const events = fixture.events as RawEvent[];
    const v = emptyView();
    apply(v, events[0]!);
    expect(() => apply(v, events[0]!)).toThrow(FoldError);
    expect(() => apply(v, events[2]!)).toThrow(FoldError);
  });
});
