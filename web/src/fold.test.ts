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

  it("shows a message sent while a turn ran where it was sent, read once the agent took it", () => {
    const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
    const v = fold([
      ev(1, "turn_started", { turn_id: "t" }),
      ev(2, "user_message", { turn_id: "t", text: "fix it" }),
      ev(3, "tool_call_started", { turn_id: "t", call_id: "c", tool: "read", input: {} }),
      ev(4, "user_message", { turn_id: "t", text: "also the docs", queued: true }),
    ]);
    expect(v.items.map((i) => i.type)).toEqual(["user", "tool", "user"]);
    expect(v.items[0]).not.toHaveProperty("queued");
    expect(v.items[2]).toMatchObject({ text: "also the docs", queued: { seq: 4, read: false } });
    apply(v, ev(5, "tool_call_finished", { call_id: "c", ok: true, output: "x", truncated: false }));
    apply(v, ev(6, "queued_delivered", { turn_id: "t", seqs: [4] }));
    expect(v.items[2]).toMatchObject({ queued: { seq: 4, read: true } });
    expect(v.running_turn).toBe("t");
    expect(v.unknown_events).toBe(0);
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

describe("fold: retries and the turn's limits", () => {
  const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
  it("shows a retry until a tool call starts, and keeps the turn's window and summary point", () => {
    const v = fold([
      ev(1, "turn_started", { turn_id: "t" }),
      ev(2, "model_retry", { turn_id: "t", model: "p::m", attempt: 1, of: 3, at_ms: 5, wait_ms: 2000, reason: "network" }),
    ]);
    expect(v.retry).toStrictEqual({ turn_id: "t", model: "p::m", attempt: 1, of: 3, at_ms: 5, wait_ms: 2000, reason: "network" });
    apply(v, ev(3, "tool_call_started", { turn_id: "t", call_id: "c", tool: "read", input: {} }));
    expect("retry" in v).toBe(false);
    apply(v, ev(4, "turn_ended", { turn_id: "t", reason: "completed", usage: { input_tokens: 5, output_tokens: 1, context_tokens: 4, window: 8000, summary_at: 0 } }));
    expect([v.context_window, v.summary_at]).toEqual([8000, 0]);
    expect("window" in v.usage).toBe(false);
    // A turn whose backend says nothing of its limits drops them.
    apply(v, ev(5, "turn_ended", { turn_id: "u", reason: "completed", usage: { input_tokens: 1, output_tokens: 1, context_tokens: 2 } }));
    expect("context_window" in v || "summary_at" in v).toBe(false);
  });
});
