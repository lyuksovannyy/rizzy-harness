// The home screen's background layer: it sits behind the column (styles.css,
// "Backdrops") and shows the chosen mode's artwork. The host and each mode's
// artwork are lazy chunks, so the first load pays nothing for them. A mode
// change lets the old layer leave while the new one enters 140 ms later; a
// new seed (a new chat) re-rolls the wall and the field the same way. Under
// "Still" or reduced motion every change is a plain cross-fade.
import { lazy, memo, Suspense, useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import type { Mode } from "../lib/modes";
import { inflate, sameZones, type Rect, type Zones } from "./geom";
import { useHidden, useMotionLevel, type BackdropPref } from "./hooks";
import type { BgProps, LayerState } from "./types";
import type { History } from "./topics";

const load = {
  search: () => import("./SearchBg"),
  ask: () => import("./WallBg"),
  agentic: () => import("./BinaryBg"),
};
const SearchBg = lazy(load.search);
const WallBg = lazy(load.ask);
const BinaryBg = lazy(load.agentic);

/** How long each mode takes to leave (its --dur-* token; the wall's lines
 *  leave over --dur-3 plus a stagger of up to 120 ms); its layer is removed
 *  after that. A plain fade is --dur-2. */
const EXIT_MS: Record<Mode, number> = { search: 360, ask: 360, agentic: 240 };
const FADE_MS = 160;
const OVERLAP_MS = 140;

type Layer = { key: number; mode: Mode; seed: number; state: LayerState; delay: number; cut?: boolean };

function Backdrop(props: {
  mode: Mode;
  seed: number;
  pref: BackdropPref;
  /** The composer has text: the magnifier's lens warms up. */
  typing: boolean;
  history: History;
}) {
  const motion = useMotionLevel(props.pref);
  const paused = useHidden();
  const host = useRef<HTMLDivElement>(null);
  const zones = useZones(host, motion !== null);
  const [layers, setLayers] = useState<Layer[]>([]);
  const keys = useRef(0);

  useEffect(() => {
    if (!motion) return setLayers([]);
    setLayers((ls) => {
      const live = ls.find((l) => l.state !== "exit");
      if (live && live.mode === props.mode && live.seed === props.seed) return ls;
      // At most one layer leaves at a time; one cut short mid-entrance just fades.
      const leaving = ls.slice(-1).map((l): Layer => (l.state === "exit" ? l : { ...l, state: "exit", cut: l.state === "enter" }));
      const delay = leaving.length && motion === "animated" ? OVERLAP_MS : 0;
      return [...leaving, { key: ++keys.current, mode: props.mode, seed: props.seed, state: "enter", delay }];
    });
  }, [props.mode, props.seed, motion]);

  // Exits run on a timer, not animationend, which never fires without motion.
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>());
  useEffect(() => {
    for (const l of layers) {
      if (l.state !== "exit" || timers.current.has(l.key)) continue;
      const ms = motion === "animated" && !l.cut ? EXIT_MS[l.mode] : FADE_MS;
      const id = setTimeout(() => {
        timers.current.delete(l.key);
        setLayers((ls) => ls.filter((x) => x.key !== l.key));
      }, ms);
      timers.current.set(l.key, id);
    }
  }, [layers, motion]);
  useEffect(() => {
    const t = timers.current;
    return () => t.forEach(clearTimeout);
  }, []);

  const entered = useCallback((key: number) => {
    setLayers((ls) => ls.map((l) => (l.key === key && l.state === "enter" ? { ...l, state: "idle" } : l)));
  }, []);

  // The other two modes' code (never their data) loads while the page is idle.
  const on = motion !== null;
  useEffect(() => {
    if (!on) return;
    const prefetch = () => Object.values(load).forEach((f) => void f().catch(() => {}));
    if (typeof requestIdleCallback === "function") {
      const id = requestIdleCallback(prefetch, { timeout: 2000 });
      return () => cancelIdleCallback(id);
    }
    const id = setTimeout(prefetch, 2000);
    return () => clearTimeout(id);
  }, [on]);

  if (!motion) return null;
  return (
    <div ref={host} className="backdrop" aria-hidden="true" data-mode={props.mode} data-motion={motion} data-paused={paused ? "" : undefined}>
      {zones &&
        layers.map((l) => {
          const p: BgProps = { state: l.state, motion, paused, zones, seed: l.seed, delay: l.delay, onEntered: () => entered(l.key) };
          return (
            <div key={l.key} className={l.cut ? "backdrop-layer cut" : "backdrop-layer"} data-mode={l.mode} data-state={l.state}>
              <Suspense fallback={null}>
                {l.mode === "search" ? (
                  <SearchBg {...p} typing={props.typing} />
                ) : l.mode === "ask" ? (
                  <WallBg {...p} history={props.history} />
                ) : (
                  <BinaryBg {...p} />
                )}
              </Suspense>
            </div>
          );
        })}
    </div>
  );
}

// Home re-renders on every keystroke; the backdrop only needs to when its
// own props change.
export default memo(Backdrop);

/**
 * Where the art must not go, kept current as the column grows (Similar
 * results), the window resizes or the sidebar collapses; measured at most
 * once a frame. The column is found in the DOM, not through a ref: it comes
 * after the backdrop, and a later sibling's ref is not attached yet when
 * this layout effect runs.
 */
function useZones(host: RefObject<HTMLElement | null>, on: boolean): Zones | null {
  const [zones, setZones] = useState<Zones | null>(null);
  useLayoutEffect(() => {
    const b = host.current;
    const col = b?.parentElement?.querySelector<HTMLElement>(":scope > .column");
    if (!on || !b || !col) return;
    let raf = 0;
    const measure = () => {
      raf = 0;
      const next = readZones(b, col);
      setZones((prev) => (sameZones(prev, next) ? prev : next));
    };
    const queue = () => {
      if (!raf) raf = requestAnimationFrame(measure);
    };
    measure();
    const ro = new ResizeObserver(queue);
    ro.observe(b);
    ro.observe(col);
    window.addEventListener("resize", queue);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", queue);
      cancelAnimationFrame(raf);
    };
  }, [host, on]);
  return zones;
}

function readZones(b: HTMLElement, col: HTMLElement): Zones {
  const o = b.getBoundingClientRect();
  const rel = (r: DOMRect): Rect => ({
    x: Math.round(r.left - o.left),
    y: Math.round(r.top - o.top),
    w: Math.round(r.width),
    h: Math.round(r.height),
  });
  const column = inflate(rel(col.getBoundingClientRect()), 24);
  const ex = [column];
  // What each child of the column shows: a range's box is the greeting's
  // words, not its full-width line.
  const range = document.createRange();
  const clear: Rect[] = [];
  for (const child of col.children) {
    range.selectNodeContents(child);
    const r = range.getBoundingClientRect();
    if (r.width && r.height) clear.push(rel(r));
  }
  const bar = document.querySelector<HTMLElement>(".topbar");
  if (bar && getComputedStyle(bar).display !== "none") {
    ex.push(inflate(rel(bar.getBoundingClientRect()), 8));
    clear.push(ex[1]!);
  }
  const composer = col.querySelector(".composer");
  return { w: Math.round(o.width), h: Math.round(o.height), ex, column, composer: composer ? rel(composer.getBoundingClientRect()) : column, clear };
}
