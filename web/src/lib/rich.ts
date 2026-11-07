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

// Only results of the web tools count. Any other tool output is the model's
// to steer (`echo <url>` in bash, a file it wrote and then read), so it
// could lift the confirm off exactly the URL it wants clicked.
const WEB_TOOLS = new Set(["web_search", "web_fetch"]);

const counts = (it: Item) => it.type === "user" || (it.type === "tool" && !!it.result && WEB_TOOLS.has(it.tool));

/** Changes exactly when provenanceIndex's input does: a new user item or a
 * web tool that finished. */
export function provenanceSignature(items: Item[]): number {
  let n = 0;
  for (const it of items) if (counts(it)) n++;
  return n;
}

/** For each URL key, the index of the first user item or web tool result
 * that contains it. The model's own text never counts. */
export function provenanceIndex(items: Item[]): Map<string, number> {
  const first = new Map<string, number>();
  items.forEach((it, i) => {
    if (!counts(it)) return;
    const keys = it.type === "user" ? keysOf(it, it.text) : it.type === "tool" && it.result ? keysOf(it.result, it.result.output) : [];
    for (const k of keys) if (!first.has(k)) first.set(k, i);
  });
  return first;
}

/** What the message at `index` may rely on: only what came before it. */
export function provenanceAt(first: Map<string, number>, index: number): Provenance {
  return { has: (key) => (first.get(key) ?? Infinity) < index };
}
