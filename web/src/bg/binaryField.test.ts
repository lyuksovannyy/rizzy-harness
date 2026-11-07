// The binary field as data: grid, zone mask, flip rate and streak limits.
import { describe, expect, test } from "vitest";
import { capDpr, CELL_H, CELL_W, Field, FLIP_RATE, gridSize, maxStreaks, zoneMask } from "./binaryField";

const zone = { x: 200, y: 200, w: 400, h: 200 };
const make = (w = 1020, h = 800, zones = [zone], seed = 1) => new Field({ w, h, zones, seed, artMax: 0.12, artPeak: 0.28 });

describe("binaryField", () => {
  test("grid size and the DPR cap", () => {
    expect(gridSize(1020, 800)).toEqual({ cols: 73, rows: 50 });
    expect(capDpr(1)).toBe(1);
    expect(capDpr(2)).toBe(1.5);
    expect(capDpr(0)).toBe(1);
  });

  test("the zone mask: nothing inside, a smooth ramp out to 72 px", () => {
    expect(zoneMask(300, 300, [zone])).toBe(0);
    expect(zoneMask(zone.x - 16, 300, [zone])).toBe(0);
    expect(zoneMask(zone.x - 16 - 72, 300, [zone])).toBe(1);
    const mid = zoneMask(zone.x - 16 - 36, 300, [zone]);
    expect(mid).toBeGreaterThan(0.3);
    expect(mid).toBeLessThan(0.7);
    expect(zoneMask(10, 10, [])).toBe(1);
  });

  test("cells in a zone stay empty while the field lives, streaks included", () => {
    const f = make();
    const inside: number[] = [];
    for (let i = 0; i < f.cols * f.rows; i++) {
      const cx = ((i % f.cols) + 0.5) * CELL_W;
      const cy = (Math.floor(i / f.cols) + 0.5) * CELL_H;
      if (cx > zone.x && cx < zone.x + zone.w && cy > zone.y && cy < zone.y + zone.h) inside.push(i);
    }
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
