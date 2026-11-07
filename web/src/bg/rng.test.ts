import { describe, expect, test } from "vitest";
import { fnv1a, mulberry32, shuffle } from "./rng";

describe("rng", () => {
  test("mulberry32 is deterministic and spread over [0, 1)", () => {
    const a = mulberry32(42);
    const b = mulberry32(42);
    const xs = Array.from({ length: 1000 }, () => a());
    expect(Array.from({ length: 1000 }, () => b())).toEqual(xs);
    expect(Math.min(...xs)).toBeGreaterThanOrEqual(0);
    expect(Math.max(...xs)).toBeLessThan(1);
    const mean = xs.reduce((s, x) => s + x, 0) / xs.length;
    expect(Math.abs(mean - 0.5)).toBeLessThan(0.05);
    expect(mulberry32(43)()).not.toBe(mulberry32(42)());
  });

  test("fnv1a gives a stable seed per id", () => {
    expect(fnv1a("abc")).toBe(fnv1a("abc"));
    expect(fnv1a("abc")).not.toBe(fnv1a("abd"));
    expect(fnv1a("")).toBe(0x811c9dc5);
  });

  test("shuffle keeps every item and follows the seed", () => {
    const xs = Array.from({ length: 50 }, (_, i) => i);
    const a = shuffle([...xs], mulberry32(7));
    expect([...a].sort((x, y) => x - y)).toEqual(xs);
    expect(shuffle([...xs], mulberry32(7))).toEqual(a);
    expect(a).not.toEqual(xs);
  });
});
