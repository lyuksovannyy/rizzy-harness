// Agentic mode: a quiet field of 0s and 1s with a soft light that follows the
// mouse. The light is a porthole onto a brighter copy of the field, so moving
// it costs three transforms a frame and no repaint; over the content, where
// the field is clear, its glow dims so it never shows as an empty smudge.
// Its own clock is capped at 30 ticks a second; when ticks run slow the
// field calms down, and if they stay very slow it goes still. Without a
// mouse there is no light.
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { capDpr, Field, FLIP_RATE } from "./binaryField";
import { drawCells, fitCanvas, makeAtlas, pitch, readInk, type Atlas, type Ink } from "./binaryDraw";
import { clamp, type Zones } from "./geom";
import { FINE_POINTER, useDevicePixelRatio, useMedia, useThemeVersion } from "./hooks";
import { loop } from "./loop";
import type { BgProps } from "./types";
import "./binary.css";

const BRIGHT_GAIN = 2.6;
/** --dur-5, the bin-in animation's length. */
const ENTER_MS = 520;
const BOOT_MS = [0, 120, 260];
const PUBLISH_MS = 2000;
/** The tick interval while the light rests: about 15 a second. */
const RESTING_MS = 66;
/** A resize rebuilds the field once the size has held this long. */
const SETTLE_MS = 300;

/** The light's radius: about a sixth of the smaller side. */
const radius = (w: number, h: number) => clamp(140, 0.16 * Math.min(w, h), 200);

/** How much of the field the light can show here: the mask at its centre and halfway out. */
function openness(f: Field, x: number, y: number, r: number): number {
  const d = 0.5 * r;
  return (f.maskAt(x, y) + f.maskAt(x - d, y) + f.maskAt(x + d, y) + f.maskAt(x, y - d) + f.maskAt(x, y + d)) / 5;
}

/** Half the flips and a single streak, for a machine that struggles. */
function calm(f: Field) {
  f.flipRate = FLIP_RATE / 2;
  f.streakLimit = 1;
}

type Engine = {
  field: Field;
  atlas: Atlas;
  ink: Ink;
  base: CanvasRenderingContext2D;
  bright: CanvasRenderingContext2D | null;
};

export default function BinaryBg(p: BgProps) {
  const host = useRef<HTMLDivElement>(null);
  const baseEl = useRef<HTMLCanvasElement>(null);
  const brightEl = useRef<HTMLCanvasElement>(null);
  const spotEl = useRef<HTMLDivElement>(null);
  const glowEl = useRef<HTMLDivElement>(null);
  const engine = useRef<Engine | null>(null);
  const fine = useMedia(FINE_POINTER);
  const [tooSlow, setTooSlow] = useState(false);
  const running = p.motion === "animated" && !tooSlow;
  const spot = running && fine;
  const inkVersion = useThemeVersion();
  const dpr = capDpr(useDevicePixelRatio());
  const { zones, seed } = p;
  // The zones the field was last built for: the light is sized and placed by them.
  const [built, setBuilt] = useState<Zones | null>(null);
  const exiting = useRef(false);
  exiting.current = p.state === "exit";
  const onEntered = useRef(p.onEntered);
  onEntered.current = p.onEntered;
  // Survive a rebuild (zones, theme): the boot streaks run once, and a field
  // that was calmed for slow ticks stays calm.
  const life = useRef({ elapsed: -p.delay, booted: 0, calm: false });

  // Build and draw the whole field: on mount and for new content zones,
  // theme, seed or screen at once; for a new size only once it has held,
  // since a dragged window changes it every frame. Meanwhile the old canvas
  // stays, clipped or a little short. The bright copy is only sized here:
  // the clock draws it whole when the light comes on.
  useEffect(() => {
    const h = host.current;
    const b = baseEl.current;
    if (!h || !b) return;
    const build = () => {
      const ink = readInk(h);
      const atlas = makeAtlas(ink, dpr);
      const field = new Field({ w: zones.w, h: zones.h, clear: zones.clear, calm: zones.column, seed, artMax: ink.artMax, artPeak: ink.artPeak, ...pitch(atlas, dpr) });
      const base = fitCanvas(b, field, atlas, dpr);
      if (!base) return;
      const bright = spot && brightEl.current ? fitCanvas(brightEl.current, field, atlas, dpr) : null;
      if (life.current.calm) calm(field);
      drawCells(base, atlas, field, null);
      engine.current = { field, atlas, ink, base, bright };
      setBuilt(zones);
    };
    if (!built || (built.w === zones.w && built.h === zones.h)) return build();
    const t = setTimeout(build, SETTLE_MS);
    return () => clearTimeout(t);
  }, [zones, seed, inkVersion, spot, dpr]);

  useEffect(() => {
    const t = setTimeout(() => onEntered.current(), running ? ENTER_MS + p.delay : 0);
    return () => clearTimeout(t);
  }, [running, p.delay]);

  // The clock: flips, streaks and the light. It restarts with each build,
  // which also re-measures where the field is on the page.
  useEffect(() => {
    if (!running || p.paused || !built) return;
    const h = host.current!;
    let rect = h.getBoundingClientRect();
    const R = radius(built.w, built.h);
    const light = { x: 0, y: 0, tx: 0, ty: 0, i: 0, moved: -1e9, away: true, placed: false, shown: -1 };
    const l = life.current;
    let ema = 0;
    let slowFor = 0;
    let verySlowFor = 0;
    let sincePublish = 0;
    let ticks = 0;
    let brightStale = true;
    let drawn: Engine | null = null;

    const onMove = (e: PointerEvent) => {
      if (e.pointerType === "touch") return;
      light.tx = e.clientX - rect.left;
      light.ty = e.clientY - rect.top;
      if (!light.placed) [light.x, light.y, light.placed] = [light.tx, light.ty, true];
      light.moved = performance.now();
      light.away = false;
    };
    const onAway = (e?: MouseEvent) => {
      if (!e || !e.relatedTarget) light.away = true;
    };
    const onBlur = () => onAway();
    const onResize = () => (rect = h.getBoundingClientRect());
    if (spot) {
      addEventListener("pointermove", onMove, { passive: true });
      document.addEventListener("mouseout", onAway, { passive: true });
      addEventListener("blur", onBlur);
      addEventListener("resize", onResize);
    }

    const moveLight = (f: Field, dt: number) => {
      const k = 1 - Math.exp(-dt / 110);
      light.x += (light.tx - light.x) * k;
      light.y += (light.ty - light.y) * k;
      const gone = light.away || exiting.current;
      const want = !gone && performance.now() - light.moved < 1200 ? 1 : 0;
      const tau = gone ? 200 : want > light.i ? 120 : 450;
      light.i += (want - light.i) * (1 - Math.exp(-dt / tau));
      if (want === 0 && light.i < 0.03) light.i = 0;
      if (light.i === 0 && light.shown === 0) return;
      light.shown = light.i;
      const at = `translate3d(${(light.x - R).toFixed(1)}px, ${(light.y - R).toFixed(1)}px, 0)`;
      const glow = light.i * openness(f, light.x, light.y, R);
      for (const [el, o] of [[spotEl.current, light.i], [glowEl.current, glow]] as const) {
        if (!el) continue;
        el.style.transform = at;
        el.style.opacity = o.toFixed(3);
      }
      if (brightEl.current) brightEl.current.style.transform = `translate3d(${(R - light.x).toFixed(1)}px, ${(R - light.y).toFixed(1)}px, 0)`;
    };

    const stop = loop((step) => {
      const e = engine.current;
      if (!e) return;
      const t0 = performance.now();
      // A rebuilt field starts with a blank bright copy.
      if (e !== drawn) [drawn, brightStale] = [e, true];
      if (spot) moveLight(e.field, step);
      l.elapsed += step;
      while (l.booted < BOOT_MS.length && l.elapsed >= BOOT_MS[l.booted]!) {
        e.field.spawn();
        l.booted++;
      }
      e.field.step(step);
      const dirty = e.field.takeDirty();
      drawCells(e.base, e.atlas, e.field, dirty);
      // The bright copy only shows through the light: while it is off it is
      // left alone, and redrawn whole as the light comes on.
      if (e.bright && light.i > 0) {
        drawCells(e.bright, e.atlas, e.field, brightStale ? null : dirty, BRIGHT_GAIN, e.ink.artPeak);
        brightStale = false;
      } else brightStale = true;
      const cost = performance.now() - t0;
      ema = ticks++ ? ema * 0.9 + cost * 0.1 : cost;
      // Slow for two seconds: fewer flips and one streak; very slow: still.
      slowFor = ema > 4 ? slowFor + step : 0;
      verySlowFor = ema > 8 ? verySlowFor + step : 0;
      if (slowFor >= 2000 && !l.calm) {
        l.calm = true;
        calm(e.field);
      }
      if (verySlowFor >= 2000) setTooSlow(true);
      sincePublish += step;
      if (ticks === 10 || sincePublish >= PUBLISH_MS) {
        sincePublish = 0;
        h.dataset.tickMs = ema.toFixed(2);
      }
      // The field changes slowly: 15 ticks a second are plenty while the
      // light rests (or there is none), and each costs the browser a frame.
      return spot && light.i > 0 ? 0 : RESTING_MS;
    });
    return () => {
      stop();
      removeEventListener("pointermove", onMove);
      document.removeEventListener("mouseout", onAway);
      removeEventListener("blur", onBlur);
      removeEventListener("resize", onResize);
    };
  }, [running, spot, p.paused, built]);

  const sized = built ?? zones;
  const vars = { "--spot": `${2 * radius(sized.w, sized.h)}px`, "--delay": `${p.delay}ms` } as CSSProperties;
  return (
    <div
      ref={host}
      className="bin-host"
      style={vars}
      data-state={p.state}
      data-motion={p.motion}
      data-running={running ? "true" : "false"}
    >
      {spot && <div ref={glowEl} className="bin-glow" />}
      <canvas ref={baseEl} className="bin-base" />
      {spot && (
        <div ref={spotEl} className="bin-spot">
          <canvas ref={brightEl} className="bin-bright" />
        </div>
      )}
    </div>
  );
}
