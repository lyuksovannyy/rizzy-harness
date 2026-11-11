// Projects' pure parts: where a dragged card lands, and that the icon names
// agree with the server's list (the route is in routes.test.ts).
import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";
import { PROJECT_ICONS } from "./projectIconSet";
import { type BoardColumn, dropIndex } from "./projects";

describe("dropIndex", () => {
  const col = (ids: string[]): BoardColumn => ({
    column_id: "c",
    name: "To do",
    done: false,
    cards: ids.map((id, position) => ({
      card_id: id,
      column_id: "c",
      title: id,
      notes: "",
      position,
      chat: null,
      created_by: null,
      updated_ms: 0,
    })),
  });

  test("before a card, or at the end", () => {
    expect(dropIndex(col(["a", "b", "c"]), "x", "b")).toBe(1);
    expect(dropIndex(col(["a", "b", "c"]), "x", null)).toBe(3);
    expect(dropIndex(col([]), "x", null)).toBe(0);
  });

  test("the dragged card does not count in its own column", () => {
    // a moved below c: before nothing → the end, which is index 2 once a is out.
    expect(dropIndex(col(["a", "b", "c"]), "a", null)).toBe(2);
    expect(dropIndex(col(["a", "b", "c"]), "a", "c")).toBe(1);
    expect(dropIndex(col(["a", "b", "c"]), "c", "a")).toBe(0);
    // Dropped on itself: stays where it is.
    expect(dropIndex(col(["a", "b", "c"]), "b", "b")).toBe(1);
  });
});

test("the icons are the server's ICONS, in the same order", () => {
  const rust = readFileSync(new URL("../../../crates/master/src/projects.rs", import.meta.url), "utf8");
  const block = /pub const ICONS: &\[&str\] = &\[([^\]]*)\]/.exec(rust)![1]!;
  const names = [...block.matchAll(/"([a-z0-9-]+)"/g)].map((m) => m[1]);
  expect(names.length).toBeGreaterThan(10);
  expect(Object.keys(PROJECT_ICONS)).toEqual(names);
});
