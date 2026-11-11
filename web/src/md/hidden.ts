// The hidden-character sets and their codes live in hiddenChars.ts (small,
// on the first screen); this adds the strip, before and after Markdown
// parsing.
import type { HastNode } from "./hast";
import { HIDDEN_SRC } from "./hiddenChars";

export { showHidden, shownName } from "./hiddenChars";

const HIDDEN = new RegExp(HIDDEN_SRC, "gu");
const ANY_HIDDEN = new RegExp(HIDDEN_SRC, "u");

export function stripHidden(text: string): string {
  return text.replace(HIDDEN, "");
}

export function hasHidden(text: string): boolean {
  return ANY_HIDDEN.test(text);
}

/**
 * The same strip after parsing. Markdown decodes character references
 * (`&#x202E;`, `&zwj;`, …) into the characters themselves, so prepare()'s
 * strip of the raw text never sees them. Every text node and every string
 * property (alt, title, the callout title, …) is cleaned. URLs are left to
 * urlTransform: the browser percent-encodes what is left in them, and the
 * host the page shows is ASCII.
 */
export default function rehypeStripHidden() {
  return (tree: HastNode) => clean(tree);
}

function clean(node: HastNode): void {
  if (typeof node.value === "string") node.value = stripHidden(node.value);
  const props = node.properties;
  if (props) {
    for (const [k, v] of Object.entries(props)) {
      if (k === "href" || k === "src") continue;
      if (typeof v === "string") props[k] = stripHidden(v);
      else if (Array.isArray(v)) props[k] = v.map((x: unknown) => (typeof x === "string" ? stripHidden(x) : x));
    }
  }
  node.children?.forEach(clean);
}
