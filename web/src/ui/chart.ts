// Where everything of a ui:chart goes (rich_plan §6.6): ticks, bars,
// lines and points, and where keyboard focus moves among them. Pure;
// Chart.tsx draws the result. The viewBox is the width the chart is shown
// at (640 until measured), so tick text stays 12px on a phone instead of
// shrinking with the picture.
import type { ChartSpec } from "../types";
import { fmtValue, stacked, tickFormat, xTexts } from "./chart-text";
import { band, linear, niceTicks } from "./scale";

export const W = 640;
const H = 320;
/** The chart is laid out for widths in this range and scaled beyond it. */
const MIN_W = 280;
const MAX_W = 960;
/** A bar is never thicker than this, however few there are. */
const BAR_MAX = 24;
/** Surface left between touching bars and between stacked segments. */
const GAP = 2;
/** A generous width of one character of tick text (12px system sans). */
const CHAR = 7;
/** x labels are cut to this many characters; the full one is in the data. */
const X_CHARS = 12;

export interface Tick {
  at: number;
  label: string;
}

/** One value as drawn: a bar (x, y, w, h) or a point (x, y). */
export interface Mark {
  /** Series and x index. */
  s: number;
  i: number;
  value: number;
  x: number;
  y: number;
  w: number;
  h: number;
  /** Bars: which end is the data end (rounded): the top, or the bottom
   * for a value below zero; none for a segment inside a stack. */
  end: "top" | "bottom" | null;
  /** Which value it is (`release, web`) and the value (`22 s`). */
  name: string;
  valueText: string;
  /** Both, for its tooltip and its aria-label. */
  label: string;
}

export interface Layout {
  width: number;
  height: number;
  plot: { left: number; top: number; right: number; bottom: number };
  xTicks: Tick[];
  yTicks: Tick[];
  /** Where zero is, when it is on the y axis. */
  zero: number | null;
  marks: Mark[];
  /** Line paths per series, broken where a value is missing. */
  lines: { s: number; d: string }[];
}

const cut = (s: string, n: number) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);

/** A stack of huge values can sum past the largest double. */
const finite = (v: number) => Math.max(-Number.MAX_VALUE, Math.min(Number.MAX_VALUE, v));

const numericX = (spec: ChartSpec): number[] | null =>
  spec.x.values.length && typeof spec.x.values[0] === "number" ? (spec.x.values as number[]) : null;

/** The y range the marks need: bars and stacks start at zero; a stack
 * sums its values above and below zero separately. */
export function yExtent(spec: ChartSpec): [number, number] {
  const n = spec.x.values.length;
  const vals: number[] = [];
  if (stacked(spec)) {
    for (let i = 0; i < n; i++) {
      let up = 0;
      let down = 0;
      for (const s of spec.series) {
        const v = s.values[i];
        if (v == null) continue;
        if (v >= 0) up += v;
        else down += v;
      }
      vals.push(finite(up), finite(down));
    }
  } else {
    for (const s of spec.series) for (const v of s.values) if (v != null) vals.push(v);
  }
  if (spec.kind === "bar") vals.push(0);
  if (!vals.length) return [0, 1];
  return [Math.min(...vals), Math.max(...vals)];
}

/** What a mark says: `release, web: 22 s`. */
function markText(spec: ChartSpec, s: number, i: number, value: number, x: string[]) {
  const name = `${spec.series[s]!.name}, ${x[i]}`;
  const valueText = fmtValue(value) + (spec.y?.unit ? ` ${spec.y.unit}` : "");
  return { name, valueText, label: `${name}: ${valueText}` };
}

/** A label centred at `at`, moved in so that it stays inside the chart. */
const fit = (at: number, label: string, width: number) => {
  const half = (label.length * CHAR) / 2;
  return Math.min(width - half, Math.max(half, at));
};

export function layout(spec: ChartSpec, shownWidth = W): Layout {
  const width = Math.round(Math.min(MAX_W, Math.max(MIN_W, shownWidth)));
  // Half as tall as wide, within 220 to 320.
  const height = Math.round(Math.min(H, Math.max(220, width / 2)));
  const x = xTexts(spec);
  const [lo, hi] = yExtent(spec);
  const yt = niceTicks(lo, hi, 5);
  const yLabels = yt.map(tickFormat(yt));
  const left = Math.max(28, Math.max(...yLabels.map((l) => l.length)) * CHAR + 10);
  const plot = { left, top: 10, right: width - 14, bottom: height - 26 };
  const y = linear(yt[0]!, yt.at(-1)!, plot.bottom, plot.top);
  const zero = yt[0]! <= 0 && yt.at(-1)! >= 0 ? y(0) : null;
  const axis = xAxis(spec, x, plot, width);
  const frame: Frame = {
    spec,
    y,
    base: zero ?? plot.bottom,
    ...axis,
    mark: (s, i, value, box) => ({ s, i, value, ...box, ...markText(spec, s, i, value, x) }),
  };
  const placed =
    spec.kind === "line" ? placeLines(frame) : { marks: stacked(spec) ? placeStacks(frame) : placeGroups(frame), lines: [] };
  return {
    width,
    height,
    plot,
    xTicks: axis.xTicks,
    yTicks: yt.map((t, k) => ({ at: y(t), label: yLabels[k]! })),
    zero,
    ...placed,
  };
}

type Box = Pick<Mark, "x" | "y" | "w" | "h" | "end">;

/** What the mark placers share: the scales and how to make a mark. */
interface Frame {
  spec: ChartSpec;
  y: (v: number) => number;
  /** Where bars stand: zero, or the bottom when zero is off the axis. */
  base: number;
  /** The middle of x value i, and the width a group of bars may use. */
  centerOf: (i: number) => number;
  slot: number;
  mark: (s: number, i: number, value: number, box: Box) => Mark;
}

/** Where each x value sits, and the x tick labels. Numbers sit at their
 * value, and bars get the narrowest gap between two; labels sit in equal
 * bands and are thinned out so that they do not run into each other. */
function xAxis(spec: ChartSpec, x: string[], plot: Layout["plot"], width: number) {
  const nums = numericX(spec);
  if (nums) {
    const sorted = [...new Set(nums)].sort((a, b) => a - b);
    const gaps = sorted.slice(1).map((v, k) => v - sorted[k]!);
    const gap = gaps.length ? Math.min(...gaps) : 1;
    const pad = spec.kind === "bar" ? gap / 2 : 0;
    const whole = nums.every(Number.isInteger);
    const xt = niceTicks(sorted[0]! - pad, sorted.at(-1)! + pad, Math.max(2, Math.round(width / 100)), whole);
    const sx = linear(xt[0]!, xt.at(-1)!, plot.left, plot.right);
    const label = tickFormat(xt);
    return {
      centerOf: (i: number) => sx(nums[i]!),
      slot: Math.abs(sx(sorted[0]! + gap) - sx(sorted[0]!)) * 0.8,
      xTicks: xt.map((t) => ({ at: fit(sx(t), label(t), width), label: label(t) })),
    };
  }
  const b = band(x.length, plot.left, plot.right);
  const widest = Math.max(1, ...x.map((l) => Math.min(l.length, X_CHARS))) * CHAR + 8;
  const every = Math.max(1, Math.ceil(widest / b.step));
  return {
    centerOf: b.center,
    slot: b.width,
    xTicks: x.flatMap((l, i) => {
      const label = cut(l, X_CHARS);
      return i % every ? [] : [{ at: fit(b.center(i), label, width), label }];
    }),
  };
}

/** Points, joined in x order (numbers may come in any order) and broken
 * where a value is missing. */
function placeLines(f: Frame): Pick<Layout, "marks" | "lines"> {
  const n = f.spec.x.values.length;
  const order = [...Array(n).keys()].sort((a, b) => f.centerOf(a) - f.centerOf(b));
  const marks: Mark[] = [];
  const lines: Layout["lines"] = [];
  f.spec.series.forEach((ser, s) => {
    let d = "";
    let joined = false;
    for (const i of order) {
      const v = ser.values[i];
      if (v == null) {
        joined = false;
        continue;
      }
      const m = f.mark(s, i, v, { x: f.centerOf(i), y: f.y(v), w: 0, h: 0, end: null });
      marks.push(m);
      d += `${joined ? "L" : "M"}${r(m.x)} ${r(m.y)}`;
      joined = true;
    }
    if (d) lines.push({ s, d });
  });
  return { marks, lines };
}

/** One bar per x value: the series stacked up from zero, values below
 * zero stacked down; only the outer end of each stack is rounded. */
function placeStacks(f: Frame): Mark[] {
  const w = Math.max(1, Math.min(BAR_MAX, f.slot));
  const marks: Mark[] = [];
  for (let i = 0; i < f.spec.x.values.length; i++) {
    const left = f.centerOf(i) - w / 2;
    for (const sign of [1, -1]) {
      const parts = f.spec.series.flatMap((ser, s) => {
        const v = ser.values[i];
        return v != null && (sign === 1 ? v > 0 : v < 0) ? [{ s, v }] : [];
      });
      let acc = 0;
      parts.forEach(({ s, v }, k) => {
        const [a, b] = [f.y(acc), f.y(finite(acc + v))];
        acc = finite(acc + v);
        // The surface gap: half of it off each end that meets another part.
        const inner = k > 0 ? GAP / 2 : 0;
        const outer = k < parts.length - 1 ? GAP / 2 : 0;
        const top = sign === 1 ? b + outer : a + inner;
        const bottom = sign === 1 ? a - inner : b - outer;
        const end = k < parts.length - 1 ? null : sign === 1 ? "top" : "bottom";
        marks.push(f.mark(s, i, v, { x: left, y: top, w, h: Math.max(0.5, bottom - top), end }));
      });
    }
    // A zero has no height in a stack: a stub on the baseline to focus.
    f.spec.series.forEach((ser, s) => {
      if (ser.values[i] === 0) marks.push(f.mark(s, i, 0, { x: left, y: f.base - 1, w, h: 1, end: null }));
    });
  }
  return marks;
}

/** The series side by side at each x value, at most 24 wide each. */
function placeGroups(f: Frame): Mark[] {
  const k = f.spec.series.length;
  const gap = Math.min(GAP, f.slot / k / 4);
  const w = Math.max(0.5, Math.min(BAR_MAX, (f.slot - gap * (k - 1)) / k));
  const group = k * w + gap * (k - 1);
  const marks: Mark[] = [];
  for (let i = 0; i < f.spec.x.values.length; i++) {
    f.spec.series.forEach((ser, s) => {
      const v = ser.values[i];
      if (v == null) return;
      const x = f.centerOf(i) - group / 2 + s * (w + gap);
      // A zero is a 1px stub on the baseline: there, but nothing.
      const box: Box =
        v === 0
          ? { x, y: f.base - 1, w, h: 1, end: null }
          : { x, y: Math.min(f.base, f.y(v)), w, h: Math.max(1, Math.abs(f.y(v) - f.base)), end: v > 0 ? "top" : "bottom" };
      marks.push(f.mark(s, i, v, box));
    });
  }
  return marks;
}

/** Coordinates to a tenth of a unit: enough for the viewBox, short paths. */
const r = (v: number) => Math.round(v * 10) / 10;

/** A bar with its data end rounded (4px, less for a thin or short bar) and
 * its baseline end square. */
export function barPath(m: Mark): string {
  const rad = m.end ? Math.min(4, m.w / 2, m.h) : 0;
  const [x0, x1, y0, y1] = [r(m.x), r(m.x + m.w), r(m.y), r(m.y + m.h)];
  if (!rad) return `M${x0} ${y0}H${x1}V${y1}H${x0}Z`;
  const q = r(rad);
  return m.end === "top"
    ? `M${x0} ${y1}V${r(y0 + q)}A${q} ${q} 0 0 1 ${r(x0 + q)} ${y0}H${r(x1 - q)}A${q} ${q} 0 0 1 ${x1} ${r(y0 + q)}V${y1}Z`
    : `M${x0} ${y0}H${x1}V${r(y1 - q)}A${q} ${q} 0 0 1 ${r(x1 - q)} ${y1}H${r(x0 + q)}A${q} ${q} 0 0 1 ${x0} ${r(y1 - q)}Z`;
}

const ALONG = new Set(["ArrowLeft", "ArrowRight", "Home", "End"]);

/**
 * Where keyboard focus goes from mark `k`: Left and Right to the previous
 * and next value of its series, Home and End to the series' first and last,
 * Up and Down to the next and previous series with a value at the same x.
 * `k` itself when there is nowhere further to go; null for any other key.
 */
export function stepMark(marks: readonly Mark[], k: number, key: string): number | null {
  const m = marks[k];
  if (!m) return null;
  const along = ALONG.has(key);
  if (!along && key !== "ArrowUp" && key !== "ArrowDown") return null;
  const line = marks
    .flatMap((o, j) => ((along ? o.s === m.s : o.i === m.i) ? [j] : []))
    .sort((a, b) => (along ? marks[a]!.x - marks[b]!.x : marks[a]!.s - marks[b]!.s));
  const here = line.indexOf(k);
  const to =
    key === "Home" ? 0 : key === "End" ? line.length - 1 : here + (key === "ArrowRight" || key === "ArrowUp" ? 1 : -1);
  return line[Math.min(line.length - 1, Math.max(0, to))] ?? k;
}
