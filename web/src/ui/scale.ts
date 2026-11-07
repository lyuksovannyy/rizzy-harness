// Scales for ui:chart (rich_plan §6.6): round tick values, a linear scale
// for numbers and a band scale for labels. Pure, so they are tested on
// their own; chart.ts places the marks with them.

/**
 * Round tick values from at or below `lo` to at or above `hi`, about
 * `count` steps apart, each step 1, 2 or 5 times a power of ten (d3's
 * rule). `integer` keeps the step whole, for x values such as years. A
 * single value gets room around it; values too large or too small to step
 * through come back as the bare range.
 */
export function niceTicks(lo: number, hi: number, count = 5, integer = false): number[] {
  if (!Number.isFinite(lo) || !Number.isFinite(hi)) return [];
  if (lo > hi) [lo, hi] = [hi, lo];
  if (lo === hi) {
    if (lo === 0) hi = 1;
    else {
      const pad = integer ? 1 : Math.abs(lo) / 10;
      lo -= pad;
      hi += pad;
    }
  }
  // Halves first: hi - lo can overflow when both are near the double range.
  const raw = ((hi / 2 - lo / 2) / Math.max(1, count)) * 2;
  let power = Math.floor(Math.log10(raw));
  if (!Number.isFinite(power) || power < -300 || power > 300) return [lo, hi];
  const error = raw / 10 ** power;
  let factor = error >= Math.sqrt(50) ? 10 : error >= Math.sqrt(10) ? 5 : error >= Math.sqrt(2) ? 2 : 1;
  if (integer && power < 0) [power, factor] = [0, 1];
  // Tick k is k steps from zero. Below 1, divide by a whole number rather
  // than multiply by a fraction: 3 / 10 is the double nearest 0.3, while
  // 3 * 0.1 is 0.30000000000000004.
  const inv = 10 ** -power / factor;
  const step = 10 ** power * factor;
  const at = power < 0 ? (k: number) => k / inv : (k: number) => k * step;
  const k0 = Math.floor(power < 0 ? lo * inv : lo / step);
  const k1 = Math.ceil(power < 0 ? hi * inv : hi / step);
  // From 2^53 up, k + 1 is k again: the loop would not move.
  if (!(Math.abs(k0) < 2 ** 52 && Math.abs(k1) < 2 ** 52 && k1 - k0 <= 1000)) return [lo, hi];
  const ticks: number[] = [];
  for (let k = k0; k <= k1; k++) ticks.push(at(k) || 0); // || 0: no -0
  const finite = ticks.filter(Number.isFinite);
  return finite.length >= 2 ? finite : [lo, hi];
}

/** Maps [d0, d1] onto [r0, r1]; an empty domain maps to the middle. */
export function linear(d0: number, d1: number, r0: number, r1: number): (v: number) => number {
  const span = d1 / 2 - d0 / 2;
  return (v) => r0 + (span === 0 ? 0.5 : (v / 2 - d0 / 2) / span) * (r1 - r0);
}

export interface Band {
  /** Room per category. */
  step: number;
  /** The part of it a group of marks may use; the rest is air. */
  width: number;
  start(i: number): number;
  center(i: number): number;
}

/** `n` categories side by side across [r0, r1], `padding` of each left empty. */
export function band(n: number, r0: number, r1: number, padding = 0.2): Band {
  const step = (r1 - r0) / Math.max(1, n);
  const width = step * (1 - padding);
  return {
    step,
    width,
    start: (i) => r0 + i * step + (step - width) / 2,
    center: (i) => r0 + (i + 0.5) * step,
  };
}
