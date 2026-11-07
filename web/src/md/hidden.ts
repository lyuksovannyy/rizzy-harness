// Hidden characters: bidi controls (U+061C, U+200E-200F, U+202A-202E,
// U+2066-2069), zero-width characters (U+200B-200D, U+2060-2064), the BOM,
// private-use characters, tag characters and the supplementary variation
// selectors. They hide or reorder text, so what the user reads (and what
// Copy gives them) would differ from what is there; tag characters and
// variation selectors are the usual carriers for invisible prompt-injection
// text. clean_text in Rust strips the same set.
import type { HastNode } from "./hast";

const HIDDEN =
  /[\u061C\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u2069\uFEFF\uE000-\uF8FF\u{E0000}-\u{E007F}\u{E0100}-\u{E01EF}]/gu;

export function stripHidden(text: string): string {
  return text.replace(HIDDEN, "");
}

const ANY_HIDDEN = new RegExp(HIDDEN.source, "u");

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
