// The one animation clock the canvas backgrounds share: at most 30 ticks a
// second, none while the tab is hidden, and after a pause the clock starts
// from zero so nothing jumps ahead. It asks the browser for a frame only
// when a tick is due: every requested frame costs the page a full frame,
// even one the tick would skip.

const MAX_DT = 100;
/** Wake this early and let the next display frame run the tick. */
const EARLY_MS = 12;

/**
 * Calls `tick(dt)` (ms since the last tick) until the returned stop is
 * called. `tick` may return how long it wants until the next one; never
 * less than 1000 / `fps`.
 */
export function loop(tick: (dt: number) => number | void, fps = 30): () => void {
  const min = 1000 / fps;
  let raf = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let last = -1;
  let stopped = false;
  const frame = (now: number) => {
    raf = 0;
    const dt = last < 0 ? 0 : Math.min(now - last, MAX_DT);
    last = now;
    const wait = Math.max(min, tick(dt) || 0);
    timer = setTimeout(() => {
      timer = undefined;
      raf = requestAnimationFrame(frame);
    }, wait - EARLY_MS);
  };
  const halt = () => {
    cancelAnimationFrame(raf);
    clearTimeout(timer);
    raf = 0;
    timer = undefined;
  };
  const sync = () => {
    if (document.visibilityState === "hidden") halt();
    else if (!raf && timer === undefined && !stopped) {
      last = -1;
      raf = requestAnimationFrame(frame);
    }
  };
  document.addEventListener("visibilitychange", sync);
  sync();
  return () => {
    stopped = true;
    halt();
    document.removeEventListener("visibilitychange", sync);
  };
}
