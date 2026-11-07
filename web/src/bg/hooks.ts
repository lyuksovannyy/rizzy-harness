// What the backgrounds need to know about the browser: whether motion is
// welcome, whether the tab is visible, and whether there is a mouse.
import { useEffect, useState, useSyncExternalStore } from "react";
import type { BackdropPref } from "../types";

export type Motion = "animated" | "still";
export type { BackdropPref };

// One subscribe function per query, so a re-render does not resubscribe.
const subscribers = new Map<string, (cb: () => void) => () => void>();
function subscribeMedia(query: string) {
  let s = subscribers.get(query);
  if (!s) {
    s = (cb) => {
      const m = matchMedia(query);
      m.addEventListener("change", cb);
      return () => m.removeEventListener("change", cb);
    };
    subscribers.set(query, s);
  }
  return s;
}

export function useMedia(query: string): boolean {
  return useSyncExternalStore(subscribeMedia(query), () => matchMedia(query).matches, () => false);
}

const onVisibility = (cb: () => void) => {
  document.addEventListener("visibilitychange", cb);
  return () => document.removeEventListener("visibilitychange", cb);
};

/** True while the tab is hidden: every loop and timer pauses then. */
export function useHidden(): boolean {
  return useSyncExternalStore(onVisibility, () => document.visibilityState === "hidden", () => false);
}

/** A mouse or trackpad: parallax and the cursor spotlight only exist then. */
export const FINE_POINTER = "(hover: hover) and (pointer: fine)";

/**
 * How the backgrounds move: not at all when turned off (null); still when
 * the user or their system asks for less motion, or data saving is on.
 */
export function useMotionLevel(pref: BackdropPref): Motion | null {
  const reduce = useMedia("(prefers-reduced-motion: reduce)");
  const saveData = !!(navigator as { connection?: { saveData?: boolean } }).connection?.saveData;
  if (pref === "off") return null;
  return pref === "still" || reduce || saveData ? "still" : "animated";
}

/** Changes whenever the theme or accent does, so canvas art re-reads its colours. */
export function useThemeVersion(): number {
  const [v, setV] = useState(0);
  useEffect(() => {
    const bump = () => setV((x) => x + 1);
    const mo = new MutationObserver(bump);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "data-accent"] });
    const mq = matchMedia("(prefers-color-scheme: light)");
    mq.addEventListener("change", bump);
    return () => {
      mo.disconnect();
      mq.removeEventListener("change", bump);
    };
  }, []);
  return v;
}
