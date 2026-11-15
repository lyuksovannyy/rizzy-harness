// When Retry shows (X.21): only on the failed last turn of a person's.
import { describe, expect, test } from "vitest";
import type { Item } from "../types";
import { canRetry } from "./retry";

const user = (t: string): Item => ({ type: "user", turn_id: t, text: "do it" });
const end = (t: string, reason: Extract<Item, { type: "turn_end" }>["reason"]): Item => ({ type: "turn_end", turn_id: t, reason });

describe("canRetry", () => {
  test("the failed last turn, for each end that gave no answer", () => {
    for (const r of ["error", "interrupted", "stopped"] as const) expect(canRetry([user("t1"), end("t1", r)], 1)).toBe(true);
    for (const r of ["completed", "cancelled", "limit"] as const) expect(canRetry([user("t1"), end("t1", r)], 1)).toBe(false);
  });
  test("not when anything started or answered after it; a note after it is fine", () => {
    const items = [user("t1"), end("t1", "error"), user("t2"), end("t2", "error")];
    expect(canRetry(items, 1)).toBe(false);
    expect(canRetry(items, 3)).toBe(true);
    const bg: Item = { type: "background_ended", bg_id: "bg-1", reason: "exited", exit_code: 0, ran_ms: 1, tail: "" };
    expect(canRetry([user("t1"), end("t1", "error"), bg], 1)).toBe(true);
    // A message sent while it ran, not read: it waits for the next turn.
    const queued: Item = { type: "user", turn_id: "t1", text: "also this", queued: { seq: 4, read: false } };
    expect(canRetry([user("t1"), end("t1", "error"), queued], 1)).toBe(true);
  });
  test("a callback turn no; a failed retry yes", () => {
    expect(canRetry([{ type: "callback", turn_id: "t1", bg_ids: ["bg-1"] }, end("t1", "error")], 1)).toBe(false);
    const retried: Item[] = [user("t1"), end("t1", "error"), { type: "retry", turn_id: "t2", of: "t1", reason: "error" }, end("t2", "error")];
    expect(canRetry(retried, 3)).toBe(true);
    expect(canRetry(retried, 1)).toBe(false);
    expect(canRetry([user("t1")], 0)).toBe(false);
  });
});
