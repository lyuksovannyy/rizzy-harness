// The Ask wall's bundled query lists (public/wall). Fetched only when the
// wall shows, once per page load; a failed fetch leaves the wall empty and
// is tried again on the next visit.
import type { Topic } from "./topics";

export const GENERAL_LIST = "/wall/queries.txt";
export const topicList = (t: Topic) => `/wall/topics/${t}.txt`;

/** Longer lines would not fit the far tier (44 characters) without cutting. */
const MAX_CHARS = 44;

/** One query per line; short lines, half-queries ("… how to") and repeats are dropped. */
export function parseList(text: string): string[] {
  const out = new Set<string>();
  for (const raw of text.split(/\r?\n/)) {
    const s = raw.trim();
    if (s.length >= 10 && s.length <= MAX_CHARS && !/\bhow to$/i.test(s)) out.add(s);
  }
  return [...out];
}

const cache = new Map<string, Promise<string[]>>();

export function loadList(path: string): Promise<string[]> {
  let p = cache.get(path);
  if (!p) {
    p = fetch(path)
      .then((r) => {
        if (!r.ok) throw new Error(`${path}: ${r.status}`);
        return r.text();
      })
      .then(parseList);
    p.catch(() => cache.delete(path));
    cache.set(path, p);
  }
  return p;
}
