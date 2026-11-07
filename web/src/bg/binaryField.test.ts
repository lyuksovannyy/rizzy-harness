// The binary field as data: grid, mask, flip rate and streak limits.
import { describe, expect, test } from "vitest";
import { CALM, CALM_FADE, capDpr, CELL_H, CELL_W, FADE_X, FADE_Y, Field, fieldMask, FLIP_RATE, gridSize, maxStreaks } from "./binaryField";
import { homeZones } from "./testkit";

const zone = { x: 200, y: 200, w: 400, h: 200 };
const make = (w = 1020, h = 800, clear = [zone], seed = 1) => new Field({ w, h, clear, seed, artMax: 0.12, artPeak: 0.28 });
/** Cell centres in CSS px, with their index. */
const cells = (f: Field) =>
  Array.from({ length: f.cols * f.rows }, (_, i) => ({ i, x: ((i % f.cols) + 0.5) * f.cw, y: (Math.floor(i / f.cols) + 0.5) * f.ch }));

describe("binaryField", () => {
  test("grid size and the DPR cap", () => {
    expect(gridSize(1020, 800)).toEqual({ cols: 73, rows: 50 });
    expect(capDpr(1)).toBe(1);
    expect(capDpr(2)).toBe(1.5);
    expect(capDpr(0)).toBe(1);
  });

  test("the mask: nothing on content, back to full along an ellipse", () => {
    expect(fieldMask(300, 300, [zone])).toBe(0);
    expect(fieldMask(zone.x, 300, [zone])).toBe(0);
    expect(fieldMask(zone.x - FADE_X, 300, [zone])).toBe(1);
    expect(fieldMask(300, zone.y - FADE_Y, [zone])).toBe(1);
    const mid = fieldMask(zone.x - FADE_X / 2, 300, [zone]);
    expect(mid).toBeGreaterThan(0.3);
    expect(mid).toBeLessThan(0.7);
    // Off a corner the ramps meet as an ellipse, not a square.
    expect(fieldMask(zone.x - 0.6 * FADE_X, zone.y - 0.6 * FADE_Y, [zone])).toBeGreaterThan(0.5);
    expect(fieldMask(zone.x - 0.8 * FADE_X, zone.y - 0.8 * FADE_Y, [zone])).toBe(1);
    expect(fieldMask(10, 10, [])).toBe(1);
  });

  test("the column only quiets the field", () => {
    const col = { x: 100, y: 100, w: 600, h: 300 };
    expect(fieldMask(400, 250, [], col)).toBeCloseTo(CALM);
    expect(fieldMask(col.x - CALM_FADE, 250, [], col)).toBe(1);
    const mid = fieldMask(col.x - CALM_FADE / 2, 250, [], col);
    expect(mid).toBeGreaterThan(CALM);
    expect(mid).toBeLessThan(1);
    expect(fieldMask(300, 300, [zone], col)).toBe(0);
  });

  // At 1280 × 800 the column fills all but 130 px on each side of the
  // backdrop: the field must still read as a field there, not a frame.
  test("beside the column and the greeting, at a laptop size, the field shows", () => {
    const z = homeZones(1020, 800);
    const f = new Field({ w: z.w, h: z.h, clear: z.clear, calm: z.column, seed: 1, artMax: 0.12, artPeak: 0.28 });
    const [greeting, composer] = z.clear as [typeof zone, typeof zone];
    const band = cells(f).filter((c) => c.y > composer.y && c.y < composer.y + composer.h);
    const gutters = band.filter((c) => c.x < z.column.x || c.x > z.column.x + z.column.w);
    expect(gutters.length).toBeGreaterThan(50);
    expect(gutters.every((c) => f.base[c.i]! > 0)).toBe(true);
    const far = gutters.filter((c) => c.x < composer.x - FADE_X || c.x > composer.x + composer.w + FADE_X);
    expect(far.every((c) => f.maskAt(c.x, c.y) >= CALM)).toBe(true);
    // Inside the column, beside the greeting's words.
    const beside = cells(f).filter((c) => c.y > greeting.y && c.y < greeting.y + greeting.h && c.x < greeting.x - FADE_X && c.x > z.column.x);
    expect(beside.length).toBeGreaterThan(20);
    expect(beside.every((c) => f.maskAt(c.x, c.y) >= CALM * 0.99)).toBe(true);
  });

  // The atlas rounds a cell to whole device pixels (15 px at 1.1x, not
  // 15.4); the grid must use that pitch to cover the width and to mask
  // each cell where it is drawn.
  test("at a fractional pixel ratio the grid follows the drawn pitch", () => {
    const dpr = 1.1;
    const [cw, ch] = [Math.round(CELL_W * dpr) / dpr, Math.round(CELL_H * dpr) / dpr];
    const f = new Field({ w: 1020, h: 800, clear: [zone], seed: 1, artMax: 0.12, artPeak: 0.28, cw, ch });
    expect(f.cols * cw).toBeGreaterThanOrEqual(1020);
    expect(f.rows * ch).toBeGreaterThanOrEqual(800);
    const inside = cells(f).filter((c) => c.x > zone.x && c.x < zone.x + zone.w && c.y > zone.y && c.y < zone.y + zone.h);
    expect(inside.length).toBeGreaterThan(100);
    expect(inside.every((c) => f.base[c.i] === 0)).toBe(true);
    const outside = cells(f).filter((c) => fieldMask(c.x, c.y, [zone]) === 1);
    expect(outside.every((c) => f.base[c.i]! > 0)).toBe(true);
  });

  test("cells on content stay empty while the field lives, streaks included", () => {
    const f = make();
    const inside = cells(f)
      .filter((c) => c.x > zone.x && c.x < zone.x + zone.w && c.y > zone.y && c.y < zone.y + zone.h)
      .map((c) => c.i);
    expect(inside.length).toBeGreaterThan(100);
    for (let t = 0; t < 600; t++) {
      if (t % 30 === 0) f.spawn();
      f.step(33);
      for (const i of inside) expect(f.alpha[i]).toBe(0);
    }
    // Resting alpha stays within the art budget everywhere.
    expect(Math.max(...f.base)).toBeLessThanOrEqual(0.12);
    expect(Math.max(...f.alpha)).toBeLessThanOrEqual(0.28 + 1e-6);
  });

  test("1.2% of visible cells flip per second (± 10%) over 10 s", () => {
    const f = make();
    let started = 0;
    for (let t = 0; t < 10_000; t += 33.3) started += f.step(33.3);
    const want = f.visible * FLIP_RATE * 10;
    expect(Math.abs(started - want) / want).toBeLessThan(0.1);
    expect(f.activeFlips).toBeLessThanOrEqual(256);
  });

  test("flips come back as the other digit", () => {
    const f = make(140, 64, [], 3);
    const before = Uint8Array.from(f.value);
    f.flipRate = 1;
    f.step(50);
    f.flipRate = 0;
    for (let k = 0; k < 20; k++) f.step(33);
    const changed = f.value.filter((v, i) => v !== before[i]).length;
    expect(changed).toBeGreaterThan(0);
    expect(f.activeFlips).toBe(0);
  });

  test("streaks are limited by the number of columns", () => {
    expect(maxStreaks(20)).toBe(1);
    expect(maxStreaks(45)).toBe(2);
    expect(maxStreaks(73)).toBe(3);
    for (const [w, limit] of [[300, 1], [600, 2], [1020, 3]] as const) {
      const f = make(w, 800, []);
      for (let k = 0; k < 10; k++) f.spawn();
      expect(f.activeStreaks).toBe(limit);
    }
  });

  test("dirty cells are handed out once", () => {
    const f = make();
    f.spawn();
    for (let k = 0; k < 5; k++) f.step(33);
    const first = f.takeDirty().length;
    expect(first).toBeGreaterThan(0);
    expect(f.takeDirty().length).toBe(0);
  });
});
