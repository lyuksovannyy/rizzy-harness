// Where the Ask wall's lines go. Pure and seeded: the same seed, size and
// zones give the same wall. Five depth tiers (fewer on phones) make the
// overlay feel; large near lines are placed first so they find room, and
// each line takes the best of 24 random spots (Mitchell's best candidate),
// which spreads lines evenly without a grid look.
import { areaIn, centre, clamp, hitsAny, inflate, type Rect, type Zones } from "./geom";

export type Tier = {
  px: number;
  /** Part of the lines in this tier. */
  share: number;
  /** Times --art-max. */
  alpha: number;
  blur: number;
  maxChars: number;
  /** Parallax travel, times 8 px. */
  depth: number;
  /** How far a line may run past the right edge, as a part of its width. */
  bleed: number;
};

const tier = (px: number, share: number, alpha: number, blur: number, maxChars: number, depth: number, bleed: number): Tier => ({
  px, share, alpha, blur, maxChars, depth, bleed,
});

/** Far (0) to near (4), at 600 px wide and up. */
export const WIDE: Tier[] = [
  tier(13, 0.3, 0.42, 0.6, 44, 0.15, 0.3),
  tier(15, 0.25, 0.55, 0.3, 40, 0.3, 0.3),
  tier(18, 0.2, 0.68, 0, 34, 0.5, 0),
  tier(22, 0.15, 0.84, 0, 28, 0.75, 0),
  tier(28, 0.1, 1, 0, 22, 1, 0),
];
/** Phones: three small tiers, no blur, no parallax. */
export const NARROW: Tier[] = [tier(12, 0.4, 0.55, 0, 34, 0, 0), tier(13, 0.35, 0.75, 0, 30, 0, 0), tier(15, 0.25, 1, 0, 24, 0, 0)];

export const tiersFor = (w: number) => (w >= 600 ? WIDE : NARROW);

/** About one line per 24,000 px² of free space (20,000 on phones). */
export function lineCount(z: Pick<Zones, "w" | "h" | "ex">): number {
  const free = z.w * z.h - z.ex.reduce((a, r) => a + areaIn(r, z.w, z.h), 0);
  return z.w >= 600 ? clamp(8, Math.floor(free / 24000), 64) : clamp(6, Math.floor(free / 20000), 14);
}

/** Splits `total` into whole parts in proportion to `weights` (largest remainder). */
export function apportion<K>(total: number, weights: ReadonlyMap<K, number>): Map<K, number> {
  const sum = [...weights.values()].reduce((a, b) => a + b, 0);
  const raw = [...weights].map(([k, w]) => [k, sum > 0 ? (total * w) / sum : 0] as const);
  const out = new Map(raw.map(([k, v]) => [k, Math.floor(v)]));
  let left = total - [...out.values()].reduce((a, b) => a + b, 0);
  for (const [k] of [...raw].sort((a, b) => (b[1] % 1) - (a[1] % 1))) {
    if (left-- <= 0) break;
    out.set(k, out.get(k)! + 1);
  }
  return out;
}

/** Where texts come from: one source (general queries or a topic). */
export type Deck = {
  /** A text of at most `maxLen` characters that is not on the wall yet. */
  take(maxLen: number): string | null;
  /** Gives a text back: it left the wall or did not fit. */
  put(text: string): void;
};

export type Placed = Rect & { text: string; tier: number; pill: boolean; delay: number };

type Ctx = {
  rng: () => number;
  zones: Zones;
  tiers: readonly Tier[];
  measure: (text: string, px: number) => number;
};

/**
 * One line of tier `ti` from `deck`, clear of the zones and of `taken`
 * (with a margin of 0.8 em across and 0.5 em down). A text that finds no
 * spot is swapped for a shorter one from the same deck, three times at most.
 */
export function placeLine(c: Ctx, ti: number, deck: Deck, taken: readonly Rect[]): Placed | null {
  const t = c.tiers[ti]!;
  let maxLen = t.maxChars;
  for (let attempt = 0; attempt < 4 && maxLen >= 10; attempt++) {
    const text = deck.take(maxLen);
    if (!text) return null;
    // About a quarter of the near lines sit in a faint search-field outline.
    const pill = ti >= 3 && c.rng() < 0.25;
    const w = Math.ceil(c.measure(text, t.px) + (pill ? 1.6 * t.px + 2 : 0));
    const h = Math.ceil(1.3 * t.px + (pill ? 0.5 * t.px + 2 : 0));
    const spot = bestSpot(c, w, h, t, taken);
    if (spot) return { ...spot, w, h, text, tier: ti, pill, delay: 0 };
    deck.put(text);
    maxLen = Math.floor(text.length * 0.75);
  }
  return null;
}

function bestSpot(c: Ctx, w: number, h: number, t: Tier, taken: readonly Rect[]) {
  // Far lines may run off the right edge, the window's, to suggest a wall
  // that goes on. Never off the left: while the sidebar shows, that edge is
  // a panel's, and a line cut against it reads as cut short.
  const x0 = 12;
  const x1 = c.zones.w - w - 12 + t.bleed * w;
  const y1 = c.zones.h - h - 12;
  if (x1 < x0 || y1 < 12) return null;
  let best: { x: number; y: number } | null = null;
  let far = -1;
  for (let i = 0; i < 24; i++) {
    const x = Math.round(x0 + c.rng() * (x1 - x0));
    const y = Math.round(12 + c.rng() * (y1 - 12));
    const room = inflate({ x, y, w, h }, 0.8 * t.px, 0.5 * t.px);
    if (hitsAny(room, c.zones.ex) || hitsAny(room, taken)) continue;
    let near = Infinity;
    for (const r of taken) near = Math.min(near, Math.hypot(r.x + r.w / 2 - x - w / 2, r.y + r.h / 2 - y - h / 2));
    if (near > far) {
      far = near;
      best = { x, y };
    }
  }
  return best;
}

/**
 * A whole wall of `n` lines. `next()` hands out the deck for each slot in
 * turn (mix.ts plans which). Each line's `delay` ripples outwards from the
 * composer, 60-700 ms with a little jitter.
 */
export function placeWall(c: Ctx & { n: number; next: () => Deck }): Placed[] {
  const counts = apportion(c.n, new Map(c.tiers.map((t, i) => [i, t.share])));
  const out: Placed[] = [];
  for (let ti = c.tiers.length - 1; ti >= 0; ti--) {
    for (let k = 0; k < counts.get(ti)!; k++) {
      const line = placeLine(c, ti, c.next(), out);
      if (line) out.push(line);
    }
  }
  const from = centre(c.zones.composer);
  const reach = Math.max(...[0, c.zones.w].flatMap((x) => [0, c.zones.h].map((y) => Math.hypot(x - from.x, y - from.y))));
  for (const l of out) {
    const d = Math.hypot(l.x + l.w / 2 - from.x, l.y + l.h / 2 - from.y);
    l.delay = Math.round(60 + (640 * d) / reach + (c.rng() * 80 - 40));
  }
  return out;
}
