// The /compact command (X.20): what the composer reads as the command, and
// when the chat's log shows its outcome.
import { describe, expect, test } from "vitest";
import { emptyView } from "../fold";
import type { SessionView } from "../types";
import { compactEnded } from "./compact";
import { parseCompact } from "./compactCommand";

describe("parseCompact", () => {
  test("the command, with or without focus", () => {
    expect(parseCompact("/compact")).toBe("");
    expect(parseCompact("  /compact  ")).toBe("");
    expect(parseCompact("/compact keep the database migration details")).toBe("keep the database migration details");
    expect(parseCompact("/compact\nline one\nline two")).toBe("line one\nline two");
  });
  test("anything else is a message", () => {
    expect(parseCompact("/compactness")).toBeNull();
    expect(parseCompact("please /compact")).toBeNull();
    expect(parseCompact("/skill compact")).toBeNull();
    expect(parseCompact("")).toBeNull();
  });
});

describe("compactEnded", () => {
  const view = (items: SessionView["items"]): SessionView => ({ ...emptyView(), items });
  test("a manual summary or an error after the request ends the wait", () => {
    const before = [{ type: "summary" as const, through_seq: 3, text: "old" }];
    expect(compactEnded(view(before), 1)).toBe(false);
    expect(compactEnded(view([...before, { type: "summary", through_seq: 9, text: "new", manual: true }]), 1)).toBe(true);
    expect(compactEnded(view([...before, { type: "error", message: "The chat was not summarised: x. Nothing was lost." }]), 1)).toBe(true);
    // An automatic summary is not the person's.
    expect(compactEnded(view([...before, { type: "summary", through_seq: 9, text: "auto" }]), 1)).toBe(false);
  });
});
