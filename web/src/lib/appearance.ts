// The theme and accent live on <html> (data-theme, data-accent; styles.css
// reads them) and in localStorage mirrors that public/theme.js applies
// before the first paint, so a reload never flashes the other theme or the
// default accent while the preferences load.
import type { BackdropPref } from "../types";

/** A forced theme; null follows the OS. The server also stores "system". */
export type Theme = "light" | "dark";

export const THEME_KEY = "harness.theme";
export const ACCENT_KEY = "harness.accent";
/** The home background choice, so Home never starts one the user turned off before the preferences arrive. */
export const BACKDROP_KEY = "harness.backdrop";

function remember(key: string, value: string | null) {
  try {
    if (value) localStorage.setItem(key, value);
    else localStorage.removeItem(key);
  } catch {
    /* storage unavailable: the next load starts from the OS theme */
  }
}

/**
 * Shows `theme` (anything but light or dark follows the OS) and `accent`
 * (null or blue is the default). A preview passes `mirror: false`, so a
 * reload before Save comes back to what is saved.
 */
export function applyAppearance(theme: string | null | undefined, accent: string | null | undefined, mirror = true): void {
  const root = document.documentElement;
  const forced = theme === "light" || theme === "dark" ? theme : null;
  const tint = accent && accent !== "blue" ? accent : null;
  if (forced) root.dataset.theme = forced;
  else delete root.dataset.theme;
  if (tint) root.dataset.accent = tint;
  else delete root.dataset.accent;
  if (mirror) {
    remember(THEME_KEY, forced);
    remember(ACCENT_KEY, tint);
  }
  // The browser's own bar: index.html gives one colour per OS theme; a
  // forced theme makes both the page colour.
  const bg = forced ? getComputedStyle(root).getPropertyValue("--bg").trim() : "";
  for (const meta of document.querySelectorAll<HTMLMetaElement>('meta[name="theme-color"]')) {
    meta.dataset.os ??= meta.content;
    meta.content = bg || meta.dataset.os;
  }
}

/** Runs `fn` (a theme change) inside a cross-fade where the browser has one and motion is welcome. */
export function withThemeTransition(fn: () => void): void {
  const still = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (still || typeof document.startViewTransition !== "function") fn();
  else document.startViewTransition(fn);
}

/** The saved home background, as the last load left it (animated when unknown). */
export function readBackdropMirror(): BackdropPref {
  try {
    const v = localStorage.getItem(BACKDROP_KEY);
    return v === "still" || v === "off" ? v : "animated";
  } catch {
    return "animated";
  }
}

export function writeBackdropMirror(v: BackdropPref | null | undefined): void {
  remember(BACKDROP_KEY, v === "still" || v === "off" ? v : null);
}
