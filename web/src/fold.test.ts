import { describe, expect, it } from "vitest";
import { apply, emptyView, fold, FoldError } from "./fold";
import fixture from "./__fixtures__/session.json";
import type { RawEvent } from "./types";

describe("fold", () => {
  it("matches the Rust reducer on the shared fixture", () => {
    const events = fixture.events as RawEvent[];
    expect(fold(events)).toEqual(fixture.expected);
  });

  it("rejects duplicates and gaps", () => {
    const events = fixture.events as RawEvent[];
    const v = emptyView();
    apply(v, events[0]!);
    expect(() => apply(v, events[0]!)).toThrow(FoldError);
    expect(() => apply(v, events[2]!)).toThrow(FoldError);
  });
});
