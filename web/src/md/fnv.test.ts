import { describe, expect, test } from "vitest";
import fixture from "../__fixtures__/ui.json";
import { fnv1a } from "./fnv";

describe("fnv1a", () => {
  test("matches Rust's hashes (ui.json)", () => {
    expect(fixture.hashes.length).toBeGreaterThan(5);
    for (const { input, hash } of fixture.hashes) expect(fnv1a(input), JSON.stringify(input)).toBe(hash);
  });
  test("reference vectors", () => {
    expect(fnv1a("")).toBe("811c9dc5");
    expect(fnv1a("a")).toBe("e40c292c");
    expect(fnv1a("foobar")).toBe("bf9cf968");
  });
});
