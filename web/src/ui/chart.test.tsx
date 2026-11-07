// @vitest-environment happy-dom
// The chart's geometry (pure) and the drawn card: marks, legend, the words
// for screen readers, the data table and keyboard focus.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { toCsv } from "../md/table";
import type { ChartSpec } from "../types";
import Chart from "./Chart";
import { barPath, layout, stepMark, yExtent } from "./chart";
import { csvRows, summary, tickFormat } from "./chart-text";

// The mock's `ui: chart`.
const builds: ChartSpec = {
  kind: "bar",
  title: "Build time by crate",
  x: { label: "crate", values: ["core", "web", "master"] },
  y: { label: "time", unit: "s" },
  series: [
    { name: "debug", values: [12.5, 8, 30] },
    { name: "release", values: [40, 22, null] },
  ],
  stacked: false,
};

describe("geometry", () => {
  test("grouped bars: one per value, a gap for a missing one, side by side at most 24 wide", () => {
    const g = layout(builds);
    expect(g.marks.map((m) => m.label)).toEqual([
      "debug, core: 12.5 s",
      "release, core: 40 s",
      "debug, web: 8 s",
      "release, web: 22 s",
      "debug, master: 30 s",
    ]);
    const [a, b] = g.marks;
    expect(a!.w).toBe(24);
    expect(b!.x - (a!.x + a!.w)).toBeCloseTo(2);
    // Bars stand on zero; the tallest reaches the top tick.
    expect(g.zero).toBe(g.plot.bottom);
    expect(b!.y + b!.h).toBeCloseTo(g.plot.bottom);
    expect(b!.y).toBeCloseTo(g.plot.top);
    expect(g.yTicks.map((t) => t.label)).toEqual(["0", "10", "20", "30", "40"]);
    expect(g.xTicks.map((t) => t.label)).toEqual(["core", "web", "master"]);
  });

  test("stacks sum above and below zero apart; only the outer end is rounded", () => {
    const spec: ChartSpec = { ...builds, stacked: true, series: [{ name: "a", values: [10, -5, 0] }, { name: "b", values: [30, -5, 4] }] };
    expect(yExtent(spec)).toEqual([-10, 40]);
    const g = layout(spec);
    const at = (s: number, i: number) => g.marks.find((m) => m.s === s && m.i === i)!;
    expect(at(0, 0).end).toBeNull();
    expect(at(1, 0).end).toBe("top");
    expect(at(1, 1).end).toBe("bottom");
    // The surface gap between the two parts of the first stack.
    expect(at(0, 0).y - (at(1, 0).y + at(1, 0).h)).toBeCloseTo(2);
    // A zero in a stack is still there to focus.
    expect(at(0, 2).h).toBe(1);
    expect(g.marks.every((m) => m.w <= 24)).toBe(true);
  });

  test("a stack that sums past the largest double still draws finite bars", () => {
    const spec: ChartSpec = { kind: "bar", stacked: true, x: { values: ["a"] }, series: [1, 2, 3].map((k) => ({ name: `s${k}`, values: [1e308] })) };
    const g = layout(spec);
    expect(g.marks.every((m) => [m.x, m.y, m.w, m.h].every(Number.isFinite))).toBe(true);
    expect(g.yTicks.every((t) => Number.isFinite(t.at))).toBe(true);
  });

  test("a line joins its points in x order and breaks at a gap", () => {
    const spec: ChartSpec = {
      kind: "line",
      x: { values: [3, 1, 2, 4] },
      series: [{ name: "n", values: [30, 10, null, 40] }],
    };
    const g = layout(spec);
    expect(g.lines).toHaveLength(1);
    const d = g.lines[0]!.d;
    // x 1 starts a run, x 2 is missing, x 3 starts a new one up to x 4.
    expect(d.match(/M/g)).toHaveLength(2);
    expect(d.match(/L/g)).toHaveLength(1);
    expect(g.marks).toHaveLength(3);
    // Lines do not have to start at zero.
    expect(g.zero).toBeNull();
    expect(g.xTicks.map((t) => t.label)).toEqual(["1", "2", "3", "4"]);
  });

  test("numbers on x sit at their value: uneven gaps stay uneven", () => {
    const spec: ChartSpec = { kind: "bar", x: { values: [2019, 2020, 2024] }, series: [{ name: "n", values: [1, 2, 3] }] };
    const [a, b, c] = layout(spec).marks.map((m) => m.x + m.w / 2);
    expect((c! - b!) / (b! - a!)).toBeCloseTo(4);
    expect(layout(spec).xTicks.every((t) => /^\d{4}$/.test(t.label))).toBe(true);
  });

  test("many labels are thinned out and long ones cut", () => {
    const values = Array.from({ length: 100 }, (_, i) => `category number ${i}`);
    const g = layout({ kind: "bar", x: { values }, series: [{ name: "n", values: values.map((_, i) => i) }] });
    expect(g.xTicks.length).toBeLessThan(15);
    expect(g.xTicks[0]!.label).toBe("category nu…");
    expect(g.marks).toHaveLength(100);
  });

  test("tick labels: the step's decimals, compact from 10,000, no grouping below", () => {
    expect([0, 0.5, 1].map(tickFormat([0, 0.5, 1]))).toEqual(["0.0", "0.5", "1.0"]);
    expect([0, 25000].map(tickFormat([0, 25000]))).toEqual(["0", "25K"]);
    expect([2019].map(tickFormat([2019, 2020]))).toEqual(["2019"]);
  });

  test("a bar's data end is rounded and its baseline end square", () => {
    const up = barPath({ s: 0, i: 0, value: 1, x: 0, y: 0, w: 20, h: 50, end: "top", name: "", valueText: "", label: "" });
    expect(up).toBe("M0 50V4A4 4 0 0 1 4 0H16A4 4 0 0 1 20 4V50Z");
    const flat = barPath({ s: 0, i: 0, value: 1, x: 0, y: 0, w: 20, h: 50, end: null, name: "", valueText: "", label: "" });
    expect(flat).toBe("M0 0H20V50H0Z");
  });

  test("the summary names the kind, the series, the x values and the range", () => {
    expect(summary(builds)).toBe(
      "Bar chart: Build time by crate. 2 series: debug, release. crate: 3 values, from core to master. Values from 8 s to 40 s. Arrow keys move between values.",
    );
  });

  test("CSV: numbers in full, model text defused for spreadsheets", () => {
    const spec: ChartSpec = { ...builds, x: { values: ["=HYPERLINK(\"x\")", "web", "master"] } };
    spec.series = [{ name: "@cmd", values: [1e21, -1e-7, null] }];
    expect(toCsv(csvRows(spec))).toBe(
      "x,'@cmd (s)\r\n\"'=HYPERLINK(\"\"x\"\")\",1000000000000000000000\r\nweb,-0.0000001\r\nmaster,",
    );
  });
});

describe("keyboard", () => {
  const g = layout(builds);
  test("left and right walk a series, up and down change series at the same x", () => {
    // Marks: 0 debug/core, 1 release/core, 2 debug/web, 3 release/web, 4 debug/master.
    expect(stepMark(g.marks, 0, "ArrowRight")).toBe(2);
    expect(stepMark(g.marks, 2, "ArrowRight")).toBe(4);
    expect(stepMark(g.marks, 4, "ArrowRight")).toBe(4);
    expect(stepMark(g.marks, 4, "Home")).toBe(0);
    expect(stepMark(g.marks, 1, "End")).toBe(3);
    expect(stepMark(g.marks, 0, "ArrowUp")).toBe(1);
    expect(stepMark(g.marks, 1, "ArrowDown")).toBe(0);
    // release has no master value: nowhere to go.
    expect(stepMark(g.marks, 4, "ArrowUp")).toBe(4);
    expect(stepMark(g.marks, 0, "Enter")).toBeNull();
  });
});

describe("Chart", () => {
  test("an SVG of marks with labels and tooltips, a legend, and the data behind a button", () => {
    const html = renderToStaticMarkup(<Chart spec={builds} />);
    expect(html).toContain('<svg class="ui-chart" viewBox="0 0 640 320" role="group" aria-label="Bar chart: Build time by crate.');
    expect(html.match(/class="ui-chart-mark /g)).toHaveLength(5);
    expect(html).toContain('role="img" aria-label="release, web: 22 s"');
    expect(html).toContain("<title>release, web: 22 s</title>");
    // One mark takes Tab; the arrow keys reach the rest.
    expect(html.match(/tabindex="0"/g)).toHaveLength(1);
    expect(html).toContain('<li class="ui-chart-c1"><span class="ui-chart-key ui-chart-key-bar" aria-hidden="true"></span>debug</li>');
    expect(html).toContain('<span class="ui-chart-axis-title">time (s)</span>');
    expect(html).toMatch(/<div class="ui-chart-table" id="[^"]+" hidden="">/);
    expect(html).toContain("<th scope=\"col\">release (s)</th>");
  });

  test("model text stays text", () => {
    const html = renderToStaticMarkup(
      <Chart spec={{ ...builds, title: "<b>t</b>", series: [{ name: "<img src=x>", values: [1, 2, 3] }] }} />,
    );
    expect(html).not.toContain("<b>");
    expect(html).not.toContain("<img");
  });

  test("Show data opens the table; arrow keys move focus and the readout follows", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(<Chart spec={builds} />));
    const table = host.querySelector<HTMLElement>(".ui-chart-table")!;
    expect(table.hidden).toBe(true);
    const show = [...host.querySelectorAll("button")].find((b) => b.textContent === "Show data")!;
    await act(async () => show.click());
    expect(table.hidden).toBe(false);
    expect(show.textContent).toBe("Hide data");
    expect(show.getAttribute("aria-expanded")).toBe("true");
    expect(table.querySelectorAll("tbody tr")).toHaveLength(3);
    expect([...table.querySelectorAll("tbody tr")].map((r) => r.textContent)).toEqual(["core12.540", "web822", "master30"]);

    const first = host.querySelector<SVGElement>('[data-mark="0"]')!;
    await act(async () => first.focus());
    expect(host.querySelector(".ui-chart-readout")?.textContent).toBe("12.5 s debug, core");
    await act(async () => {
      first.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    });
    const now = host.querySelector<SVGElement>('[data-mark="2"]')!;
    expect(document.activeElement).toBe(now);
    expect(now.getAttribute("tabindex")).toBe("0");
    expect(first.getAttribute("tabindex")).toBe("-1");
    expect(host.querySelector(".ui-chart-readout")?.textContent).toBe("8 s debug, web");
    await act(async () => root.unmount());
    host.remove();
  });
});
