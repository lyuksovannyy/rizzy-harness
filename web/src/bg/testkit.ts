// Shared fixtures for the background tests: a home-like layout and decks of
// made-up queries, all seeded.
import type { Zones } from "./geom";
import { createMixer } from "./mix";
import type { Deck } from "./place";
import { mulberry32 } from "./rng";

/** The home column (greeting to composer) centred in a w × h backdrop, as Backdrop measures it. */
export function homeZones(w: number, h: number, colH = 300): Zones {
  const colW = Math.min(760, w);
  const col = { x: (w - colW) / 2 - 24, y: Math.round(h * 0.44 - colH / 2) - 24, w: colW + 48, h: colH + 48 };
  const composer = { x: col.x + 64, y: col.y + 160, w: colW - 80, h: 98 };
  const greeting = { x: w / 2 - 110, y: col.y + 40, w: 220, h: 70 };
  return { w, h, ex: [col], column: col, composer, clear: [greeting, composer] };
}

const WORDS = "how to why is what are the best way fix make cook learn rust bread tie a tie travel cheap flight robot arm".split(" ");

/** Distinct query-like lines of 10 to 44 characters. */
export function queries(n: number, seed = 1): string[] {
  const rng = mulberry32(seed);
  const out = new Set<string>();
  while (out.size < n) {
    let s = WORDS[Math.floor(rng() * 3)]!;
    while (s.length < 10 + rng() * 30) s += ` ${WORDS[Math.floor(rng() * WORDS.length)]}`;
    out.add(`${s.slice(0, 40)} ${out.size}`);
  }
  return [...out];
}

/** About 0.5 em per character, like a proportional sans. */
export const fakeMeasure = (text: string, px: number) => text.length * px * 0.5;

/** A `next` for placeWall that always hands out the general deck. */
export function generalDecks(seed: number, list = queries(400)): () => Deck {
  const mixer = createMixer({ rng: mulberry32(seed), general: list, topics: new Map(), p: new Map(), share: 0, banned: [] });
  return () => mixer.draw();
}
