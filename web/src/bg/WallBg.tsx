// Ask mode: a wall of commonly asked questions, faint and layered like an
// overlay. Lines ripple in from the composer; then, every few seconds, one
// old line fades and another takes a free spot (now and then typed out).
// Near tiers drift a little against the mouse. The text is drawn by CSS
// from data-text, so it stays out of the accessibility tree, find-in-page
// and copy. The lists are fetched only when this shows (wallKit.ts).
import { useEffect, useReducer, useRef, type CSSProperties } from "react";
import { clamp, hitsAny, inflate } from "./geom";
import { FINE_POINTER, useMedia } from "./hooks";
import type { Mixer } from "./mix";
import { lineCount, placeLine, tiersFor, type Placed, type Tier } from "./place";
import { between, mulberry32 } from "./rng";
import type { TitleRow } from "./topics";
import type { BgProps } from "./types";
import { layWall, measureText, useWallData } from "./wallKit";
import "./wall.css";

type Phase = "in" | "fresh" | "typing" | "typed" | "out" | "gone";
/** `since`: when the line got its phase (placed, or started leaving). */
type Line = Placed & { id: number; since: number; phase: Phase };

/** A line must have shown this long before the cadence may retire it. */
const OLD_MS = 8000;
const OUT_MS = 700;
const TYPE_MS = 32;
const SETTLE_MS = 400;

const live = (l: Line) => l.phase !== "out" && l.phase !== "gone";
/** Lines near the composer leave last: the ripple in reverse. */
const exitStagger = (delay: number) => Math.round(120 * (1 - clamp(0, (delay - 20) / 720, 1)));

export default function WallBg(p: BgProps & { titles: readonly TitleRow[] }) {
  const { zones, motion, state, paused, seed } = p;
  const animated = motion === "animated";
  const tiers = tiersFor(zones.w);
  const data = useWallData(p.titles, lineCount(zones));
  // The lines live in a ref and render on demand: the cadence changes one
  // line every few seconds and must read the current set, not a stale render.
  const lines = useRef<Line[]>([]);
  const [, render] = useReducer((x: number) => x + 1, 0);
  const commit = (next: Line[]) => {
    lines.current = next;
    render();
  };
  const wall = useRef<{ mixer: Mixer; rng: () => number; size: string } | null>(null);
  const quick = useRef(false);
  const ids = useRef(0);
  const els = useRef(new Map<number, HTMLElement>());
  const tierEls = useRef<(HTMLDivElement | null)[]>([]);
  const onEntered = useRef(p.onEntered);
  onEntered.current = p.onEntered;

  // The cadence's timers outlive renders: they place against the zones as they are now.
  const latest = useRef({ zones, tiers });
  latest.current = { zones, tiers };
  const fresh = (l: Placed, phase: Phase): Line => ({ ...l, id: ++ids.current, since: performance.now(), phase });
  const ctxFor = (w: { rng: () => number }) => ({ rng: w.rng, ...latest.current, measure: measureText });
  const room = (l: Line, t: readonly Tier[]) => inflate(l, 0.8 * (t[l.tier]?.px ?? 13), 0.5 * (t[l.tier]?.px ?? 13));

  /** Lines a zone pushed out go back on the wall, each in its own tier. */
  const refill = (list: Line[]): Line[] => {
    const w = wall.current;
    if (!w || list.every((l) => l.phase !== "gone")) return list;
    const keep = list.filter((l) => l.phase !== "gone");
    for (const l of list) {
      if (l.phase !== "gone") continue;
      w.mixer.release(l.text);
      const next = l.tier < latest.current.tiers.length ? placeLine(ctxFor(w), l.tier, w.mixer.draw(), keep) : null;
      if (next) keep.push(fresh(next, "fresh"));
    }
    return keep;
  };

  useEffect(() => {
    if (!data) return;
    const size = `${zones.w}x${zones.h}`;
    const lay = () => {
      const { lines: placed, mixer, rng } = layWall(data, seed, zones, tiers, lineCount(zones));
      wall.current = { mixer, rng, size };
      commit(placed.map((l) => fresh(l, "in")));
    };
    if (!wall.current) {
      quick.current = data.late;
      return lay();
    }
    if (wall.current.size !== size) {
      // Resized: a fresh wall once the size settles, faded in without a ripple.
      const t = setTimeout(() => {
        quick.current = true;
        lay();
      }, SETTLE_MS);
      return () => clearTimeout(t);
    }
    // A zone grew (Similar results appeared): lines under it fade out.
    if (!lines.current.some((l) => live(l) && hitsAny(room(l, tiers), zones.ex))) return;
    const next = lines.current.map((l): Line => (live(l) && hitsAny(room(l, tiers), zones.ex) ? { ...l, phase: "gone" } : l));
    commit(animated ? next : refill(next));
  }, [data, zones]);

  // The cadence: every 2.6 ± 0.8 s one old line leaves and another arrives.
  useEffect(() => {
    if (!animated || paused || state === "exit" || !data) return;
    const beat = mulberry32(seed ^ 0x5bd1e995);
    const timers = new Set<ReturnType<typeof setTimeout>>();
    const later = (fn: () => void, ms: number) => {
      const t = setTimeout(() => {
        timers.delete(t);
        fn();
      }, ms);
      timers.add(t);
    };
    const replace = (victim: Line) => {
      const w = wall.current;
      if (!w || !lines.current.some((l) => l.id === victim.id)) return;
      w.mixer.release(victim.text);
      const keep = lines.current.filter((l) => l.id !== victim.id);
      const next = placeLine(ctxFor(w), victim.tier, w.mixer.draw(), keep);
      if (next) {
        // One in three mid-tier replacements is typed out, one at a time.
        const mid = latest.current.tiers.length === 5 && (victim.tier === 2 || victim.tier === 3);
        const typed = mid && !keep.some((l) => l.phase === "typing") && beat() < 1 / 3;
        keep.push(fresh(next, typed ? "typing" : "fresh"));
      }
      commit(keep);
    };
    const tick = () => {
      const now = performance.now();
      // A retiring line whose replacement was cut short by a pause comes back too.
      const stuck = (l: Line) => l.phase === "out" && now - l.since > 2 * OUT_MS;
      let list = lines.current.some(stuck) ? lines.current.map((l): Line => (stuck(l) ? { ...l, phase: "gone" } : l)) : lines.current;
      list = refill(list);
      const old = list.filter((l) => live(l) && l.phase !== "typing" && now - l.since >= OLD_MS);
      if (old.length) {
        const victim = old[Math.floor(beat() * old.length)]!;
        list = list.map((l): Line => (l.id === victim.id ? { ...l, phase: "out", since: now } : l));
        later(() => replace(victim), OUT_MS);
      }
      if (list !== lines.current) commit(list);
      later(tick, between(beat, 1800, 3400));
    };
    later(tick, between(beat, 1800, 3400));
    return () => timers.forEach(clearTimeout);
  }, [animated, paused, state, data]);

  // Typing: straight into data-text, never through React state.
  const typer = useRef<{ id: number; stop(): void } | null>(null);
  useEffect(() => {
    const l = lines.current.find((x) => x.phase === "typing");
    const el = l && els.current.get(l.id);
    if (!l || !el || typer.current?.id === l.id) return;
    let i = 0;
    const t = setInterval(() => {
      el.dataset.text = l.text.slice(0, ++i);
      if (i < l.text.length) return;
      clearInterval(t);
      typer.current = null;
      commit(lines.current.map((x): Line => (x.id === l.id ? { ...x, phase: "typed" } : x)));
    }, TYPE_MS);
    typer.current = { id: l.id, stop: () => clearInterval(t) };
  });
  useEffect(() => () => typer.current?.stop(), []);

  // Parallax: each tier drifts against the mouse by its depth, eased, and
  // the frame loop stops as soon as everything has caught up.
  const fine = useMedia(FINE_POINTER);
  const parallax = animated && fine && tiers.length === 5 && state !== "exit" && !paused;
  const drift = useRef(tiers.map(() => ({ x: 0, y: 0 })));
  useEffect(() => {
    if (!parallax) return;
    let tx = 0;
    let ty = 0;
    let raf = 0;
    let last = 0;
    const step = (now: number) => {
      const k = 1 - Math.exp(-(last ? Math.min(now - last, 100) : 16) / 160);
      last = now;
      let moving = false;
      tiers.forEach((t, i) => {
        const q = (drift.current[i] ??= { x: 0, y: 0 });
        const gx = -tx * t.depth * 8;
        const gy = -ty * t.depth * 8;
        q.x += (gx - q.x) * k;
        q.y += (gy - q.y) * k;
        if (Math.abs(gx - q.x) > 0.1 || Math.abs(gy - q.y) > 0.1) moving = true;
        const el = tierEls.current[i];
        if (el) el.style.transform = `translate3d(${q.x.toFixed(2)}px, ${q.y.toFixed(2)}px, 0)`;
      });
      raf = moving ? requestAnimationFrame(step) : 0;
      if (!moving) last = 0;
    };
    const move = (e: PointerEvent) => {
      if (e.pointerType === "touch") return;
      tx = clamp(-1, (e.clientX / innerWidth) * 2 - 1, 1);
      ty = clamp(-1, (e.clientY / innerHeight) * 2 - 1, 1);
      if (!raf) raf = requestAnimationFrame(step);
    };
    addEventListener("pointermove", move, { passive: true });
    return () => {
      removeEventListener("pointermove", move);
      cancelAnimationFrame(raf);
    };
  }, [parallax, tiers]);

  const laid = lines.current.length > 0;
  useEffect(() => {
    if (!laid) return;
    const last = Math.max(0, ...lines.current.map((l) => l.delay));
    const t = setTimeout(() => onEntered.current(), animated && !quick.current ? last + 700 + p.delay : 0);
    return () => clearTimeout(t);
  }, [laid]);

  return (
    <div className={quick.current ? "wall quick" : "wall"} data-state={state} data-motion={motion} style={{ "--delay": `${p.delay}ms` } as CSSProperties}>
      {tiers.map((t, ti) => (
        <div
          key={ti}
          className="wall-tier"
          ref={(el) => {
            tierEls.current[ti] = el;
          }}
          style={{ "--a": t.alpha, filter: t.blur ? `blur(${t.blur}px)` : undefined } as CSSProperties}
        >
          {lines.current
            .filter((l) => l.tier === ti)
            .map((l) => (
              <span
                key={l.id}
                ref={(el) => {
                  if (el) els.current.set(l.id, el);
                  else els.current.delete(l.id);
                }}
                className={`wall-line${l.pill ? " pill" : ""}${l.phase === "in" ? "" : ` ${l.phase}`}`}
                data-tier={ti}
                data-text={l.phase === "typing" || l.phase === "typed" ? undefined : l.text}
                style={
                  {
                    "--x": `${l.x}px`,
                    "--y": `${l.y}px`,
                    "--d": `${l.delay}ms`,
                    "--xd": `${exitStagger(l.delay)}ms`,
                    fontSize: `${t.px}px`,
                  } as CSSProperties
                }
              />
            ))}
        </div>
      ))}
    </div>
  );
}
