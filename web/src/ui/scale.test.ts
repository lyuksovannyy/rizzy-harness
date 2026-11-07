import { describe, expect, test } from "vitest";
import { band, linear, niceTicks } from "./scale";

describe("niceTicks", () => {
  test("round steps of 1, 2 or 5 that cover the range", () => {
    expect(niceTicks(0, 40)).toEqual([0, 10, 20, 30, 40]);
    expect(niceTicks(0, 30)).toEqual([0, 5, 10, 15, 20, 25, 30]);
    expect(niceTicks(0, 37)).toEqual([0, 10, 20, 30, 40]);
    expect(niceTicks(-3, 12.5)).toEqual([-4, -2, 0, 2, 4, 6, 8, 10, 12, 14]);
    expect(niceTicks(3, 97)).toEqual([0, 20, 40, 60, 80, 100]);
    expect(niceTicks(1200, 98000)).toEqual([0, 20000, 40000, 60000, 80000, 100000]);
  });

  test("fractions come out as the doubles they are written as", () => {
    expect(niceTicks(0, 1)).toEqual([0, 0.2, 0.4, 0.6, 0.8, 1]);
    expect(niceTicks(0.1, 0.3, 4)).toEqual([0.1, 0.15, 0.2, 0.25, 0.3]);
    expect(niceTicks(0, 0.0003)).toEqual([0, 0.00005, 0.0001, 0.00015, 0.0002, 0.00025, 0.0003]);
  });

  test("the range always lies within the ticks", () => {
    for (const [lo, hi] of [
      [-7.3, 2.1],
      [0.001, 0.0049],
      [123456, 123789],
      [-1e9, 3e9],
      [5, 6],
    ] as const) {
      const t = niceTicks(lo, hi);
      expect(t[0]).toBeLessThanOrEqual(lo);
      expect(t.at(-1)).toBeGreaterThanOrEqual(hi);
      expect(t.length).toBeGreaterThanOrEqual(2);
      expect(t.length).toBeLessThanOrEqual(13);
    }
  });

  test("a single value gets room; zero alone runs to 1; no -0", () => {
    expect(niceTicks(0, 0)).toEqual([0, 0.2, 0.4, 0.6, 0.8, 1]);
    const five = niceTicks(5, 5);
    expect(five[0]).toBeLessThan(5);
    expect(five.at(-1)).toBeGreaterThan(5);
    expect(niceTicks(-1, 0).every((t) => !Object.is(t, -0))).toBe(true);
  });

  test("whole steps for whole numbers such as years", () => {
    expect(niceTicks(2019.5, 2021.5, 6, true)).toEqual([2019, 2020, 2021, 2022]);
    expect(niceTicks(2020, 2020, 6, true)).toEqual([2019, 2020, 2021]);
    expect(niceTicks(0, 1, 5, true)).toEqual([0, 1]);
  });

  test("swapped bounds, and values at the edge of the double range", () => {
    expect(niceTicks(40, 0)).toEqual([0, 10, 20, 30, 40]);
    expect(niceTicks(Number.NaN, 1)).toEqual([]);
    expect(niceTicks(0, Infinity)).toEqual([]);
    for (const [lo, hi] of [
      [-1.7e308, 1.7e308],
      [1e18, 1e18 + 256],
      [0, 5e-324],
    ] as const) {
      const t = niceTicks(lo, hi);
      expect(t.every(Number.isFinite)).toBe(true);
      expect(t.length).toBeGreaterThanOrEqual(2);
    }
  });
});

describe("linear", () => {
  test("maps the domain onto the range, also reversed (y grows down)", () => {
    const y = linear(0, 40, 300, 0);
    expect(y(0)).toBe(300);
    expect(y(40)).toBe(0);
    expect(y(10)).toBe(225);
    expect(linear(-10, 10, 0, 100)(0)).toBe(50);
  });
  test("an empty domain maps to the middle; huge domains stay finite", () => {
    expect(linear(5, 5, 0, 100)(5)).toBe(50);
    const big = linear(-1.7e308, 1.7e308, 0, 100);
    expect(big(1.7e308)).toBe(100);
    expect(big(0)).toBe(50);
  });
});

describe("band", () => {
  test("equal steps, a padded width, centres in the middle of each step", () => {
    const b = band(4, 0, 400);
    expect(b.step).toBe(100);
    expect(b.width).toBe(80);
    expect(b.start(0)).toBe(10);
    expect(b.center(0)).toBe(50);
    expect(b.center(3)).toBe(350);
  });
  test("no categories is one empty step, not a division by zero", () => {
    expect(band(0, 0, 100).step).toBe(100);
  });
});
