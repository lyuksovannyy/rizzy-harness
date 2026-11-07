// Which bundled list each wall line comes from. With little history the wall
// is all general queries; as titles about a topic pile up, that topic's list
// takes a growing share (up to 60%). The user's own words never appear: the
// titles only steer the choice, and a bundled line that equals one is skipped.
import { apportion, type Deck } from "./place";
import { shuffle } from "./rng";
import type { Topic } from "./topics";

/** The part of the wall for topic lines, from the number of titles that matched a topic. */
export const share = (m: number) => (m < 3 ? 0 : 0.6 * Math.min(1, (m - 2) / 28));

/** Lines that need a topic's file: its whole number of the first `n` lines. */
export function quotas(n: number, s: number, p: ReadonlyMap<Topic, number>): Map<Topic, number> {
  return p.size ? apportion(Math.round(n * s), p) : new Map();
}

/** Lowercase, no accents, no punctuation, single spaces: "How to tie a tie?" is "how to tie a tie". */
export function normalize(s: string): string {
  return s
    .toLowerCase()
    .normalize("NFKD")
    .replace(/\p{M}/gu, "")
    .replace(/[^\p{L}\p{N}\s]/gu, "")
    .replace(/\s+/g, " ")
    .trim();
}

export type Mixer = {
  /** The decks for the first `n` lines, in a shuffled order. */
  plan(n: number): Deck[];
  /** A deck for one replacement line. */
  draw(): Deck;
  /** A text left the wall and may come back later. */
  release(text: string): void;
};

/**
 * `topics` holds the topic lists that were loaded; `p` and `s` come from
 * topicWeights and share. Each list is a seeded shuffled deck, reshuffled
 * when it runs out, and no text is on the wall twice at once.
 */
export function createMixer(o: {
  rng: () => number;
  general: readonly string[];
  topics: ReadonlyMap<Topic, readonly string[]>;
  p: ReadonlyMap<Topic, number>;
  share: number;
  titles: readonly string[];
}): Mixer {
  const banned = new Set(o.titles.map(normalize));
  const onWall = new Set<string>();
  const deck = (list: readonly string[]): Deck => {
    const items = shuffle(
      list.filter((s) => !banned.has(normalize(s))),
      o.rng,
    );
    let i = 0;
    return {
      take(maxLen) {
        // Skipped texts stay in the deck for a tier that has room for them.
        for (let pass = 0; pass < 2; pass++) {
          for (let j = i; j < items.length; j++) {
            const s = items[j]!;
            if (s.length > maxLen || onWall.has(s)) continue;
            items[j] = items[i]!;
            items[i++] = s;
            onWall.add(s);
            return s;
          }
          shuffle(items, o.rng);
          i = 0;
        }
        return null;
      },
      put: (s) => void onWall.delete(s),
    };
  };
  const general = deck(o.general);
  const topic = new Map([...o.topics].map(([t, list]) => [t, deck(list)]));
  return {
    plan(n) {
      const out: Deck[] = [];
      for (const [t, q] of quotas(n, o.share, o.p)) {
        const d = topic.get(t);
        if (d) for (let k = 0; k < q; k++) out.push(d);
      }
      while (out.length < n) out.push(general);
      return shuffle(out, o.rng);
    },
    draw() {
      let r = o.rng();
      for (const [t, d] of topic) {
        r -= o.share * (o.p.get(t) ?? 0);
        if (r < 0) return d;
      }
      return general;
    },
    release: (s) => void onWall.delete(s),
  };
}
