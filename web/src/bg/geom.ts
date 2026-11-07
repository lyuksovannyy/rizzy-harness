// Rectangles and the exclusion zones every background keeps clear: nothing is
// ever drawn under the greeting, the composer or the top bar.

export type Rect = { x: number; y: number; w: number; h: number };

/**
 * The backdrop's size and where its art must not go, all relative to the
 * backdrop. `column` is the home column inflated by 24 px (also `ex[0]`);
 * `ex` adds the top bar when it shows; `composer` is the bar itself, the
 * centre the wall ripples out from. `clear` is the content alone (the
 * greeting's words, the composer, any list under it, and the top bar):
 * the binary field fades out around that and only quiets the rest of the
 * column.
 */
export type Zones = { w: number; h: number; ex: Rect[]; column: Rect; composer: Rect; clear: Rect[] };

export const inflate = (r: Rect, dx: number, dy = dx): Rect => ({
  x: r.x - dx,
  y: r.y - dy,
  w: r.w + 2 * dx,
  h: r.h + 2 * dy,
});

export const hits = (a: Rect, b: Rect) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

export const hitsAny = (r: Rect, zones: readonly Rect[]) => zones.some((z) => hits(r, z));

/** The area of `r` inside a w × h box. */
export function areaIn(r: Rect, w: number, h: number): number {
  const x0 = Math.max(0, r.x);
  const y0 = Math.max(0, r.y);
  const x1 = Math.min(w, r.x + r.w);
  const y1 = Math.min(h, r.y + r.h);
  return Math.max(0, x1 - x0) * Math.max(0, y1 - y0);
}

export const centre = (r: Rect) => ({ x: r.x + r.w / 2, y: r.y + r.h / 2 });

export const clamp = (lo: number, v: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Same zones to the pixel: a resize that changes nothing re-renders nothing. */
export function sameZones(a: Zones | null, b: Zones): boolean {
  if (!a || a.ex.length !== b.ex.length || a.w !== b.w || a.h !== b.h) return false;
  const eq = (p: Rect, q: Rect) => p.x === q.x && p.y === q.y && p.w === q.w && p.h === q.h;
  return (
    eq(a.composer, b.composer) &&
    a.ex.every((r, i) => eq(r, b.ex[i]!)) &&
    a.clear.length === b.clear.length &&
    a.clear.every((r, i) => eq(r, b.clear[i]!))
  );
}
