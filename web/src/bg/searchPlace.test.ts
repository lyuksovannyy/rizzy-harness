// Where the magnifier goes, in each branch, and never under the column.
import { describe, expect, test } from "vitest";
import { hits, type Rect, type Zones } from "./geom";
import { mulberry32 } from "./rng";
import { placeMagnifier, SHADOW_ROOM } from "./searchPlace";

const zones = (w: number, h: number, col: Rect, more: Rect[] = []): Zones => ({
  w,
  h,
  ex: [col, ...more],
  column: col,
  composer: col,
  clear: [col],
});
const box = (s: { x: number; y: number; size: number }) => ({ x: s.x, y: s.y, w: s.size, h: s.size * (1 + SHADOW_ROOM) });

describe("placeMagnifier", () => {
  test("above the greeting when there is room, centred on the column", () => {
    const col = { x: 106, y: 380, w: 808, h: 340 };
    const s = placeMagnifier(zones(1020, 900, col))!;
    expect(s.size).toBe(270);
    expect(s.x + s.size / 2).toBeCloseTo(col.x + col.w / 2, 0);
    expect(s.y + s.size * (1 + SHADOW_ROOM)).toBeLessThanOrEqual(col.y - 16 + 1);
  });

  test("in the right gutter when the top is short and the side is wide", () => {
    const col = { x: 400, y: 120, w: 808, h: 340 };
    const s = placeMagnifier(zones(1660, 700, col))!;
    expect(s.x).toBeGreaterThan(col.x + col.w);
    expect(s.x + s.size).toBeLessThanOrEqual(1660);
  });

  test("smaller above when only that fits", () => {
    const col = { x: 106, y: 200, w: 808, h: 340 };
    const s = placeMagnifier(zones(1020, 800, col))!;
    expect(s.size).toBeLessThan(0.9 * 240);
    expect(s.size).toBeGreaterThanOrEqual(88);
    expect(s.y).toBeGreaterThanOrEqual(0);
  });

  test("nowhere when nothing fits", () => {
    expect(placeMagnifier(zones(390, 400, { x: 0, y: 60, w: 390, h: 340 }))).toBeNull();
  });

  test("never over a zone, the top bar included", () => {
    const rng = mulberry32(9);
    for (let i = 0; i < 500; i++) {
      const w = 320 + rng() * 1800;
      const h = 360 + rng() * 900;
      const cw = Math.min(w, 808);
      const col = { x: (w - cw) / 2, y: rng() * h * 0.6, w: cw, h: 200 + rng() * 300 };
      const bar = { x: 0, y: 0, w: 96, h: 50 };
      const z = zones(w, h, col, [bar]);
      const s = placeMagnifier(z);
      if (!s) continue;
      for (const r of z.ex) expect(hits(box(s), r)).toBe(false);
      expect(s.x).toBeGreaterThanOrEqual(0);
      expect(s.y).toBeGreaterThanOrEqual(0);
    }
  });
});
