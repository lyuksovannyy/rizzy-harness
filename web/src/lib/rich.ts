// Pure derivations over the folded timeline for rich message rendering.
import type { Provenance } from "../md/links";
import { urlKeysIn } from "../md/links";
import type { Item } from "../types";

// User items and tool results never change once created, so their tokens
// are worked out once (the view is re-rendered on every streamed delta).
const keysCache = new WeakMap<object, string[]>();

function keysOf(owner: object, text: string): string[] {
  let keys = keysCache.get(owner);
  if (!keys) {
    keys = urlKeysIn(text);
    keysCache.set(owner, keys);
  }
  return keys;
}

/** Changes exactly when provenanceIndex's input does: a new user item or a
 * tool that finished. */
export function provenanceSignature(items: Item[]): number {
  let n = 0;
  for (const it of items) if (it.type === "user" || (it.type === "tool" && it.result)) n++;
  return n;
}

/** For each URL key, the index of the first user item or tool result that
 * contains it. The model's own text never counts. */
export function provenanceIndex(items: Item[]): Map<string, number> {
  const first = new Map<string, number>();
  items.forEach((it, i) => {
    const keys =
      it.type === "user" ? keysOf(it, it.text) : it.type === "tool" && it.result ? keysOf(it.result, it.result.output) : [];
    for (const k of keys) if (!first.has(k)) first.set(k, i);
  });
  return first;
}

/** What the message at `index` may rely on: only what came before it. */
export function provenanceAt(first: Map<string, number>, index: number): Provenance {
  return { has: (key) => (first.get(key) ?? Infinity) < index };
}
