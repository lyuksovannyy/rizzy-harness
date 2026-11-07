// A `ui:chart` block (rich_plan §6.6), drawn as our own React SVG in its
// own lazy chunk: only a message with a chart loads it. Every name, label
// and number is the model's and renders as text. Colour is never the only
// key: a legend names the series, each mark says what it is on hover and to
// screen readers, and Show data gives the same numbers as a table.
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent, type RefObject } from "react";
import { useCopy } from "../md/copy";
import { rt } from "../md/strings";
import { toCsv } from "../md/table";
import type { ChartSpec } from "../types";
import { barPath, layout, stepMark, W, type Mark } from "./chart";
import { csvRows, fmtValue, summary, xTexts } from "./chart-text";

/** Past this many points a series shows its dots only where pointed at. */
const DENSE = 30;

const tone = (s: number) => `ui-chart-c${(s % 6) + 1}`;

/** The width an element is shown at, as it changes (W until measured). */
function useWidth(el: RefObject<HTMLElement | null>): number {
  const [width, setWidth] = useState(W);
  useEffect(() => {
    const node = el.current;
    if (!node || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(([e]) => {
      const w = Math.round(e?.contentRect.width ?? 0);
      if (w > 0) setWidth(w);
    });
    ro.observe(node);
    return () => ro.disconnect();
  }, [el]);
  return width;
}

export default function Chart({ spec }: { spec: ChartSpec }) {
  const box = useRef<HTMLDivElement>(null);
  const width = useWidth(box);
  const geo = useMemo(() => layout(spec, width), [spec, width]);
  const described = useMemo(() => summary(spec), [spec]);
  // One mark takes Tab (the arrow keys move from it); the readout names
  // the mark under the pointer or in focus.
  const [active, setActive] = useState(0);
  const [shown, setShown] = useState<number | null>(null);
  const [table, setTable] = useState(false);
  const [copied, copy] = useCopy();
  const svg = useRef<SVGSVGElement>(null);
  const tableId = useId();
  const { plot } = geo;
  const unit = spec.y?.unit ? ` (${spec.y.unit})` : "";
  const yTitle = spec.y?.label ? spec.y.label + unit : spec.y?.unit ?? "";
  const dense = spec.kind === "line" && spec.x.values.length > DENSE;

  const onKey = (e: KeyboardEvent) => {
    const to = stepMark(geo.marks, active, e.key);
    if (to === null) return;
    e.preventDefault();
    setActive(to);
    svg.current?.querySelector<SVGElement>(`[data-mark="${to}"]`)?.focus();
  };
  const mark = (m: Mark, k: number) => ({
    className: `ui-chart-mark ${tone(m.s)} ${spec.kind === "line" ? "ui-chart-point" : "ui-chart-bar"}`,
    role: "img",
    "aria-label": m.label,
    tabIndex: k === Math.min(active, geo.marks.length - 1) ? 0 : -1,
    "data-mark": k,
    onFocus: () => {
      setActive(k);
      setShown(k);
    },
    onBlur: () => setShown(null),
    onPointerEnter: () => setShown(k),
    onPointerLeave: () => setShown(null),
  });
  const current = shown === null ? undefined : geo.marks[shown];

  return (
    <div className="ui-chart-box" ref={box}>
      {spec.title && <p className="ui-title">{spec.title}</p>}
      {(spec.series.length > 1 || !spec.title) && (
        <ul className="ui-chart-legend">
          {spec.series.map((s, k) => (
            <li key={k} className={tone(k)}>
              <span className={`ui-chart-key ui-chart-key-${spec.kind}`} aria-hidden="true" />
              {s.name}
            </li>
          ))}
        </ul>
      )}
      {/* The readout sits beside the y axis title, above the plot, so that
          it never covers a mark. */}
      <div className="ui-chart-head">
        <span className="ui-chart-axis-title">{yTitle}</span>
        {current && (
          <span className="ui-chart-readout" aria-hidden="true">
            <strong>{current.valueText}</strong> {current.name}
          </span>
        )}
      </div>
      <svg
        ref={svg}
        className={`ui-chart${dense ? " ui-chart-dense" : ""}`}
        viewBox={`0 0 ${geo.width} ${geo.height}`}
        role="group"
        aria-label={described}
        onKeyDown={onKey}
      >
        <g className="ui-chart-grid" aria-hidden="true">
          {geo.yTicks.map((t, k) => (
            <line key={k} x1={plot.left} x2={plot.right} y1={t.at} y2={t.at} />
          ))}
          {geo.zero !== null && <line className="ui-chart-zero" x1={plot.left} x2={plot.right} y1={geo.zero} y2={geo.zero} />}
        </g>
        <g className="ui-chart-ticks" aria-hidden="true">
          {geo.yTicks.map((t, k) => (
            <text key={k} x={plot.left - 6} y={t.at} textAnchor="end" dominantBaseline="middle">
              {t.label}
            </text>
          ))}
          {geo.xTicks.map((t, k) => (
            <text key={k} x={t.at} y={plot.bottom + 18} textAnchor="middle">
              {t.label}
            </text>
          ))}
        </g>
        {geo.lines.map((l) => (
          <path key={l.s} className={`ui-chart-line ${tone(l.s)}`} d={l.d} aria-hidden="true" />
        ))}
        {geo.marks.map((m, k) =>
          spec.kind === "line" ? (
            <g key={k} {...mark(m, k)}>
              <title>{m.label}</title>
              <circle className="ui-chart-hit" cx={m.x} cy={m.y} r={10} />
              <circle className="ui-chart-dot" cx={m.x} cy={m.y} r={4} />
            </g>
          ) : (
            <path key={k} d={barPath(m)} {...mark(m, k)}>
              <title>{m.label}</title>
            </path>
          ),
        )}
      </svg>
      {spec.x.label && <div className="ui-chart-axis-title ui-chart-axis-x">{spec.x.label}</div>}
      <div className="ui-chart-tools">
        <button type="button" className="link small" aria-expanded={table} aria-controls={tableId} onClick={() => setTable(!table)}>
          {rt(table ? "chart.hide" : "chart.show")}
        </button>
        <button type="button" className="link small" onClick={() => copy(toCsv(csvRows(spec)))}>
          {copied ? rt("copied") : rt("copyCsv")}
        </button>
      </div>
      <div className="ui-chart-table" id={tableId} hidden={!table}>
        <DataTable spec={spec} unit={unit} />
      </div>
    </div>
  );
}

/** The chart's numbers as a table: x down the side, a column per series. */
function DataTable({ spec, unit }: { spec: ChartSpec; unit: string }) {
  const x = xTexts(spec);
  return (
    <table>
      <thead>
        <tr>
          <th scope="col">{spec.x.label ?? ""}</th>
          {spec.series.map((s, k) => (
            <th key={k} scope="col">
              {s.name}
              {unit}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {x.map((label, i) => (
          <tr key={i}>
            <th scope="row">{label}</th>
            {spec.series.map((s, k) => {
              const v = s.values[i];
              return <td key={k}>{v == null ? "" : fmtValue(v)}</td>;
            })}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
