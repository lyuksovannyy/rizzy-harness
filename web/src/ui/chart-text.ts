// The words and numbers of a ui:chart: values and tick labels, the summary
// screen readers get, and the data as CSV rows. Names, labels and units are
// the model's (cleaned in Rust) and only ever become text.
import { rt } from "../md/strings";
import type { ChartSpec } from "../types";
import { num } from "./form";

export const stacked = (spec: ChartSpec) => spec.kind === "bar" && !!spec.stacked;

const plainFmt = new Intl.NumberFormat("en", { maximumSignificantDigits: 15, useGrouping: false });
const groupedFmt = new Intl.NumberFormat("en", { maximumSignificantDigits: 15 });

/** A value for reading: digits in full, grouped from 10,000 (so a year
 * stays 2024). */
export const fmtValue = (v: number) => (Math.abs(v) < 10000 ? plainFmt : groupedFmt).format(v);

/** Tick labels, one style per axis: in full to the step's decimals, or
 * compact (25K, 1.5M) on an axis that reaches 10,000. */
export function tickFormat(ticks: number[]): (v: number) => string {
  const step = ticks.length > 1 ? Math.abs(ticks[1]! - ticks[0]!) : 1;
  const decimals = Math.min(20, Math.max(0, -Math.floor(Math.log10(step))));
  const compact = ticks.some((t) => Math.abs(t) >= 10000);
  const f = compact
    ? new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 2 })
    : new Intl.NumberFormat("en", { minimumFractionDigits: decimals, maximumFractionDigits: decimals, useGrouping: false });
  return (v) => f.format(v);
}

/** The x values as text, in the order given. */
export function xTexts(spec: ChartSpec): string[] {
  return (spec.x.values as (string | number)[]).map((v) => (typeof v === "number" ? fmtValue(v) : v));
}

/** The chart in a few sentences, for screen readers. */
export function summary(spec: ChartSpec): string {
  const kind = rt(spec.kind === "line" ? "chart.line" : stacked(spec) ? "chart.stacked" : "chart.bar");
  const names = spec.series.map((s) => s.name).join(", ");
  const x = xTexts(spec);
  const unit = spec.y?.unit ? ` ${spec.y.unit}` : "";
  const vals = spec.series.flatMap((s) => s.values.filter((v): v is number => v != null));
  const parts = [
    spec.title ? `${kind}: ${spec.title}.` : `${kind}.`,
    rt(spec.series.length === 1 ? "chart.series1" : "chart.series", { n: String(spec.series.length), names }),
    rt(x.length === 1 ? "chart.along1" : "chart.along", {
      n: String(x.length),
      x: spec.x.label ?? "x",
      first: x[0] ?? "",
      last: x.at(-1) ?? "",
    }),
    vals.length
      ? rt("chart.range", { min: fmtValue(Math.min(...vals)) + unit, max: fmtValue(Math.max(...vals)) + unit })
      : rt("chart.empty"),
    rt("chart.keys"),
  ];
  return parts.join(" ");
}

/** The data as CSV rows: x, then one column per series. Numbers are
 * written in full (no exponent, no grouping) so they read back. */
export function csvRows(spec: ChartSpec): string[][] {
  const unit = spec.y?.unit ? ` (${spec.y.unit})` : "";
  const head = [spec.x.label ?? "x", ...spec.series.map((s) => s.name + unit)];
  const xs = (spec.x.values as (string | number)[]).map((v) => (typeof v === "number" ? num(v) : v));
  return [head, ...xs.map((xv, i) => [xv, ...spec.series.map((s) => (s.values[i] == null ? "" : num(s.values[i]!)))])];
}
