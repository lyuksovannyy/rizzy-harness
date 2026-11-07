// Where Search mode's magnifier goes: above the greeting when there is room,
// else in the right-hand gutter, else smaller above, else nowhere (a phone in
// landscape with the keyboard open). Never under the column or the top bar.
import { clamp, hitsAny, type Rect, type Zones } from "./geom";

/** The floor shadow sits below the glyph and needs this much of its size. */
export const SHADOW_ROOM = 0.22;

export type MagSpot = { x: number; y: number; size: number };

export function placeMagnifier(z: Zones): MagSpot | null {
  const col = z.column;
  const S = clamp(120, Math.min(0.3 * z.h, 0.32 * z.w), 300);
  const above = col.y - 24 - SHADOW_ROOM * S;
  const gutter = z.w - (col.x + col.w) - 32;
  // Above: the glyph and its shadow end 16 px over the column's zone.
  const top = (size: number): MagSpot => ({
    x: Math.round(col.x + col.w / 2 - size / 2),
    y: Math.round(col.y - 16 - (1 + SHADOW_ROOM) * size),
    size: Math.round(size),
  });
  const side = (size: number): MagSpot => ({
    x: Math.round(col.x + col.w + 16 + (gutter - size) / 2),
    y: Math.round(col.y + col.h * 0.4 - size / 2),
    size: Math.round(size),
  });
  const tries: MagSpot[] = [];
  if (above >= 0.9 * S) tries.push(top(Math.min(S, above)));
  if (gutter >= 160) tries.push(side(Math.min(S, gutter)));
  if (above >= 88) tries.push(top(above));
  return tries.find((s) => fits(s, z)) ?? null;
}

/** The glyph and its shadow stay inside the backdrop and clear of every zone. */
function fits(s: MagSpot, z: Zones): boolean {
  const r: Rect = { x: s.x, y: s.y, w: s.size, h: s.size * (1 + SHADOW_ROOM) };
  return r.x >= 0 && r.y >= 0 && r.x + r.w <= z.w && r.y + r.h <= z.h && !hitsAny(r, z.ex);
}
