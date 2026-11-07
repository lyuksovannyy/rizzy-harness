// Opt-in ("Also in chats"): a still, fainter copy of the chat's background in
// the side gutters, only where each gutter is at least 180 px wide, so it
// never sits behind the column. Chats get the far tiers of the query wall,
// agentic sessions the binary field, both at half strength and placed by the
// session's id, so a chat always looks the same. Nothing here moves.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { CELL_H, capDpr, Field } from "./binaryField";
import { drawCells, fitCanvas, makeAtlas, readInk } from "./binaryDraw";
import { sameZones, type Rect, type Zones } from "./geom";
import { useThemeVersion } from "./hooks";
import { WIDE, type Tier } from "./place";
import { fnv1a } from "./rng";
import type { TitleRow } from "./topics";
import { layWall, useWallData } from "./wallKit";
import "./wall.css";
import "./gutter.css";

const MIN_GUTTER = 180;
/** Keep clear of the column by this much; the mask fades the same strip. */
const EDGE = 48;
const STRENGTH = 0.5;
const TIERS: Tier[] = WIDE.slice(0, 2).map((t) => ({ ...t, alpha: t.alpha * STRENGTH, bleed: 0 }));

export default function ChatGutter(props: { sessionId: string; agentic: boolean; titles: readonly TitleRow[] }) {
  const host = useRef<HTMLDivElement>(null);
  const [zones, setZones] = useState<Zones | null>(null);
  useLayoutEffect(() => {
    const el = host.current;
    const conv = el?.parentElement;
    if (!el || !conv) return;
    const measure = () => {
      const o = conv.getBoundingClientRect();
      const col = conv.querySelector(".timeline .column")?.getBoundingClientRect();
      if (!col || (o.width - col.width) / 2 < MIN_GUTTER) return setZones(null);
      // The column (and the strip beside it), the header and the composer area.
      const ex: Rect[] = [{ x: col.left - o.left - EDGE, y: 0, w: col.width + 2 * EDGE, h: o.height }];
      for (const sel of [".conv-head", ".bottom"]) {
        const r = conv.querySelector(`:scope > ${sel}`)?.getBoundingClientRect();
        if (r) ex.push({ x: 0, y: r.top - o.top, w: o.width, h: r.height });
      }
      const w = Math.round(o.width);
      const h = Math.round(o.height);
      const next = { w, h, ex, column: ex[0]!, composer: ex[0]! };
      setZones((prev) => (sameZones(prev, next) ? prev : next));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(conv);
    return () => ro.disconnect();
  }, []);
  const seed = fnv1a(props.sessionId);
  return (
    <div ref={host} className="gutter" aria-hidden="true">
      {zones && (props.agentic ? <StillField zones={zones} seed={seed} /> : <StillWall zones={zones} seed={seed} titles={props.titles} />)}
    </div>
  );
}

/** About one line per 30,000 px² of gutter. */
function gutterLines(z: Zones): number {
  const col = z.ex[0]!;
  return Math.max(2, Math.floor(((z.w - col.w) * z.h) / 30000));
}

function StillWall(props: { zones: Zones; seed: number; titles: readonly TitleRow[] }) {
  const n = gutterLines(props.zones);
  const data = useWallData(props.titles, n);
  const lines = useMemo(() => (data ? layWall(data, props.seed, props.zones, TIERS, n).lines : []), [data, props.seed, props.zones, n]);
  return (
    <div className="wall" data-motion="still">
      {TIERS.map((t, ti) => (
        <div key={ti} className="wall-tier" style={{ "--a": t.alpha, filter: `blur(${t.blur}px)` } as CSSProperties}>
          {lines
            .filter((l) => l.tier === ti)
            .map((l, i) => (
              <span
                key={i}
                className="wall-line"
                data-tier={ti}
                data-text={l.text}
                style={{ "--x": `${l.x}px`, "--y": `${l.y}px`, fontSize: `${t.px}px` } as CSSProperties}
              />
            ))}
        </div>
      ))}
    </div>
  );
}

function StillField(props: { zones: Zones; seed: number }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const theme = useThemeVersion();
  const { zones, seed } = props;
  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const ink = readInk(c);
    const dpr = capDpr(devicePixelRatio);
    // The zones already include the strip by the column; the field's own
    // mask would widen it by another 88 px, so it gets the column alone.
    const col = zones.ex[0]!;
    const inner = { ...col, x: col.x + EDGE, w: col.w - 2 * EDGE, y: -CELL_H, h: zones.h + 2 * CELL_H };
    const field = new Field({ w: zones.w, h: zones.h, zones: [inner], seed, artMax: ink.artMax * STRENGTH, artPeak: 0 });
    const atlas = makeAtlas(ink, dpr);
    const ctx = fitCanvas(c, field, atlas, dpr);
    if (ctx) drawCells(ctx, atlas, field, null);
  }, [zones, seed, theme]);
  return <canvas ref={canvas} className="bin-base" style={{ position: "absolute", left: 0, top: 0 }} />;
}
