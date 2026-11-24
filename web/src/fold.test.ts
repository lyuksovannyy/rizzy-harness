import { describe, expect, it } from "vitest";
import { apply, emptyView, fold, FoldError } from "./fold";
import { readFileSync } from "node:fs";

// Parsed, not imported: a JSON module is compiled to an object literal, where
// a "__proto__" key (the fixture has one: model ids are the node's) sets the
// prototype instead of making a key.
const fixture = JSON.parse(readFileSync(new URL("./__fixtures__/session.json", import.meta.url), "utf8"));
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

  it("keeps an approval's own tool only when it is not its call's (review F2)", () => {
    const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
    const v = fold([
      ev(1, "turn_started", { turn_id: "t" }),
      ev(2, "tool_call_started", { turn_id: "t", call_id: "c", tool: "bash", input: { command: "ls" } }),
      ev(3, "approval_requested", { approval_id: "a", call_id: "c", tool: "push", input: {}, reason: "r" }),
      ev(4, "tool_call_started", { turn_id: "t", call_id: "d", tool: "push", input: {} }),
      ev(5, "approval_requested", { approval_id: "b", call_id: "d", tool: "push", input: {}, reason: "r" }),
    ]);
    const tools = v.items.filter((i) => i.type === "tool");
    expect(tools[0]!.approval!.tool).toBe("push");
    expect(tools[1]!.approval).not.toHaveProperty("tool");
  });

  it("marks every card of a reused approval id or call invalid, in either order (review N1)", () => {
    const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
    const start = (c: string, t: string) => ({ turn_id: "t", call_id: c, tool: t, input: {} });
    const ask = (a: string, c: string, t: string) => ({ approval_id: a, call_id: c, tool: t, input: {}, reason: "r" });
    const marks = (evs: [string, object][]) => {
      const v = fold([ev(1, "turn_started", { turn_id: "t" }), ...evs.map(([k, p], i) => ev(i + 2, k, p))]);
      return {
        v,
        invalid: v.items.flatMap((i) => (i.type === "tool" && i.approval ? [i.approval.invalid === true] : [])),
      };
    };
    // The reviewer's probe: the composer's first card is the bash one.
    const probe = marks([
      ["tool_call_started", start("cb", "bash")],
      ["approval_requested", ask("r1", "cb", "bash")],
      ["tool_call_started", start("cp", "push")],
      ["approval_requested", ask("r1", "cp", "push")],
    ]);
    expect(probe.v.pending_approvals).toEqual(["r1", "r1"]);
    expect(probe.invalid).toEqual([true, true]);
    expect(marks([["tool_call_started", start("c", "bash")], ["approval_requested", ask("r", "c", "push")], ["tool_call_started", start("c", "push")]]).invalid).toEqual([true]);
    expect(marks([["tool_call_started", start("c", "push")], ["tool_call_started", start("c", "bash")], ["approval_requested", ask("r", "c", "push")]]).invalid).toEqual([true]);
    const ok = marks([["tool_call_started", start("c", "push")], ["approval_requested", ask("r", "c", "push")]]);
    expect(ok.invalid).toEqual([false]);
    expect(ok.v.items[0]).toMatchObject({ approval: { approval_id: "r" } });
    expect((ok.v.items[0] as { approval: object }).approval).not.toHaveProperty("invalid");
  });

  it("shows the file a doc_create call made on its result (T97)", () => {
    const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
    const made = { name: "plan.docx", size: 1234, sha256: "ab".repeat(32), file_id: "f1" };
    const v = fold([
      ev(1, "turn_started", { turn_id: "t" }),
      ev(2, "user_message", { turn_id: "t", text: "make it" }),
      ev(3, "tool_call_started", { turn_id: "t", call_id: "c", tool: "doc_create", input: {} }),
      ev(4, "tool_call_finished", { call_id: "c", ok: true, output: "Created", truncated: false, made }),
    ]);
    expect(v.items[1]).toMatchObject({ type: "tool", result: { ok: true, made } });
    expect(v.unknown_events).toBe(0);
  });

  it("shows the files saved into the chat's folder on the message that brought them (T94)", () => {
    const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
    const up = { name: "x.zip", path: "attachments/x (2).zip", bytes: 1234, sha256: "ab".repeat(32), mime: "application/zip" };
    const v = fold([
      ev(1, "turn_started", { turn_id: "t" }),
      ev(2, "user_message", { turn_id: "t", text: "unpack it", uploads: [up] }),
      ev(3, "turn_ended", { turn_id: "t", reason: "completed" }),
      ev(4, "turn_started", { turn_id: "u" }),
      ev(5, "user_message", { turn_id: "u", text: "again" }),
    ]);
    expect(v.items[0]).toMatchObject({ type: "user", uploads: [{ name: "x.zip", path: "attachments/x (2).zip", bytes: 1234 }] });
    expect(v.items.find((i) => i.type === "user" && i.turn_id === "u")).not.toHaveProperty("uploads");
    expect(v.unknown_events).toBe(0);
  });

  it("shows the files kept for the agent to read on the message that brought them (T95)", () => {
    const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
    const f = { file_id: "f1", name: "x.zip", bytes: 1234, sha256: "ab".repeat(32), mime: "application/zip" };
    const v = fold([
      ev(1, "turn_started", { turn_id: "t" }),
      ev(2, "user_message", { turn_id: "t", text: "look", attached: [f] }),
      ev(3, "turn_ended", { turn_id: "t", reason: "completed" }),
      ev(4, "turn_started", { turn_id: "u" }),
      ev(5, "user_message", { turn_id: "u", text: "again" }),
    ]);
    expect(v.items[0]).toMatchObject({ type: "user", attached: [{ file_id: "f1", name: "x.zip", bytes: 1234 }] });
    expect(v.items.find((i) => i.type === "user" && i.turn_id === "u")).not.toHaveProperty("attached");
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

  it("ignores a withheld-images record whose calls are not a list", () => {
    const ev = (seq: number, kind: string, payload: object): RawEvent =>
      ({ session_id: "s", seq, ts_ms: 0, kind, payload }) as RawEvent;
    const base = () =>
      fold([
        ev(1, "turn_started", { turn_id: "t" }),
        ev(2, "user_message", { turn_id: "t", text: "go" }),
        ev(3, "tool_call_started", { turn_id: "t", call_id: "c", tool: "read", input: {} }),
        ev(4, "tool_call_finished", { call_id: "c", ok: true, output: "x", truncated: false }),
      ]);
    const output = (v: ReturnType<typeof base>) => JSON.stringify(v.items);
    const plain = output(base());
    // A node's malformed event must not break the chat's page.
    for (const calls of ["c", 7, { a: 1 }, null]) {
      const v = base();
      apply(v, ev(5, "images_withheld", { turn_id: "t", calls, status: 400 }));
      expect(output(v)).toBe(plain);
    }
    const v = base();
    apply(v, ev(5, "images_withheld", { turn_id: "t", calls: ["nope", "c"], status: 400 }));
    expect(output(v)).not.toBe(plain);
  });

  it("rejects duplicates and gaps", () => {
    const events = fixture.events as RawEvent[];
    const v = emptyView();
    apply(v, events[0]!);
    expect(() => apply(v, events[0]!)).toThrow(FoldError);
    expect(() => apply(v, events[2]!)).toThrow(FoldError);
  });
});

describe("fold: a notice", () => {
  const ev = (seq: number, payload: object): RawEvent => ({ session_id: "s", seq, ts_ms: 0, kind: "error", payload }) as RawEvent;
  it("keeps the flag, and a failure has no key for it", () => {
    const v = fold([ev(1, { message: "n", notice: true }), ev(2, { message: "f" })]);
    expect(v.items[0]).toStrictEqual({ type: "error", message: "n", notice: true });
    expect(v.items[1]).toStrictEqual({ type: "error", message: "f" });
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
    // A summary call that gave up ends the countdown too.
    apply(v, ev(4, "model_retry", { turn_id: "t", model: "p::m", attempt: 1, of: 3, at_ms: 5, wait_ms: 2000, reason: "network" }));
    apply(v, ev(5, "retry_given_up", { turn_id: "t", model: "p::m" }));
    expect("retry" in v).toBe(false);
    apply(v, ev(6, "turn_ended", { turn_id: "t", reason: "completed", usage: { input_tokens: 5, output_tokens: 1, context_tokens: 4, window: 8000, summary_at: 0, model: "p::fb" } }));
    expect([v.context_window, v.summary_at, v.context_model]).toEqual([8000, 0, "p::fb"]);
    expect("window" in v.usage).toBe(false);
    // A turn whose backend says nothing of its limits drops them.
    apply(v, ev(7, "turn_ended", { turn_id: "u", reason: "completed", usage: { input_tokens: 1, output_tokens: 1, context_tokens: 2 } }));
    expect("context_window" in v || "summary_at" in v || "context_model" in v).toBe(false);
  });
});
