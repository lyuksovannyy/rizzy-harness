// Agentic mode: a quiet field of 0s and 1s with a soft light that follows the
// mouse. The light is a porthole onto a brighter copy of the field, so moving
// it costs three transforms a frame and no repaint. Its own clock is capped
// at 30 ticks a second; when ticks run slow the field calms down, and if
// they stay very slow it goes still. Without a mouse there is no light.
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { capDpr, Field, FLIP_RATE } from "./binaryField";
import { drawCells, fitCanvas, makeAtlas, readInk, type Atlas, type Ink } from "./binaryDraw";
import { clamp } from "./geom";
import { FINE_POINTER, useMedia, useThemeVersion } from "./hooks";
import { loop } from "./loop";
import type { BgProps } from "./types";
import "./binary.css";

const BRIGHT_GAIN = 2.6;
const BOOT_MS = [0, 120, 260];
const PUBLISH_MS = 2000;
/** The tick interval while the light rests: about 15 a second. */
const RESTING_MS = 66;

/** The light's radius: about a sixth of the smaller side. */
const radius = (w: number, h: number) => clamp(140, 0.16 * Math.min(w, h), 200);

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
  const { zones, seed } = p;
  const exiting = useRef(false);
  exiting.current = p.state === "exit";
  const onEntered = useRef(p.onEntered);
  onEntered.current = p.onEntered;
  // Survive a rebuild (zones, theme): the boot streaks run once, and a field
  // that was calmed for slow ticks stays calm.
  const life = useRef({ elapsed: -p.delay, booted: 0, calm: false });

  // Build and draw the whole field: on mount, resize, zone change, theme change.
  useEffect(() => {
    const h = host.current;
    const b = baseEl.current;
    if (!h || !b) return;
    const ink = readInk(h);
    const dpr = capDpr(devicePixelRatio);
    const field = new Field({ w: zones.w, h: zones.h, zones: zones.ex, seed, artMax: ink.artMax, artPeak: ink.artPeak });
    const atlas = makeAtlas(ink, dpr);
    const base = fitCanvas(b, field, atlas, dpr);
    if (!base) return;
    const bright = spot && brightEl.current ? fitCanvas(brightEl.current, field, atlas, dpr) : null;
    if (life.current.calm) calm(field);
    drawCells(base, atlas, field, null);
    if (bright) drawCells(bright, atlas, field, null, BRIGHT_GAIN, ink.artPeak);
    engine.current = { field, atlas, ink, base, bright };
  }, [zones, seed, inkVersion, spot]);

  useEffect(() => {
    const t = setTimeout(() => onEntered.current(), running ? 480 + p.delay : 0);
    return () => clearTimeout(t);
  }, [running, p.delay]);

  // The clock: flips, streaks and the light.
  useEffect(() => {
    if (!running || p.paused) return;
    const h = host.current!;
    let rect = h.getBoundingClientRect();
    const R = radius(zones.w, zones.h);
    const light = { x: 0, y: 0, tx: 0, ty: 0, i: 0, moved: -1e9, away: true, placed: false, shown: -1 };
    const l = life.current;
    let ema = 0;
    let slowFor = 0;
    let verySlowFor = 0;
    let sincePublish = 0;
    let ticks = 0;
    let brightStale = true;

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

    const moveLight = (dt: number) => {
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
      for (const el of [spotEl.current, glowEl.current]) {
        if (!el) continue;
        el.style.transform = at;
        el.style.opacity = light.i.toFixed(3);
      }
      if (brightEl.current) brightEl.current.style.transform = `translate3d(${(R - light.x).toFixed(1)}px, ${(R - light.y).toFixed(1)}px, 0)`;
    };

    const stop = loop((step) => {
      const e = engine.current;
      if (!e) return;
      const t0 = performance.now();
      if (spot) moveLight(step);
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
  }, [running, spot, p.paused, zones]);

  const vars = { "--spot": `${2 * radius(zones.w, zones.h)}px`, "--delay": `${p.delay}ms` } as CSSProperties;
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
