// Hidden characters: bidi controls (U+061C, U+200E-200F, U+202A-202E,
// U+2066-2069), zero-width characters (U+200B-200D, U+2060-2064), the BOM,
// private-use characters, and the default-ignorable code points no script
// needs: deprecated format characters (U+2065-206F), unassigned specials
// (U+FFF0-FFF8), musical beam controls (U+1D173-1D17A) and the whole tag and
// variation-selector plane (U+E0000-E0FFF). They hide or reorder text, so
// what the user reads (and what Copy gives them) would differ from what is
// there; tag characters and variation selectors are the usual carriers for
// invisible prompt-injection text. The soft hyphen and the BMP variation
// selectors (emoji style) shape real text and stay. clean_text in Rust
// strips the same set.
import type { HastNode } from "./hast";

const HIDDEN =
  /[\u061C\u200B-\u200F\u202A-\u202E\u2060-\u206F\uFEFF\uFFF0-\uFFF8\uE000-\uF8FF\u{1D173}-\u{1D17A}\u{E0000}-\u{E0FFF}]/gu;

export function stripHidden(text: string): string {
  return text.replace(HIDDEN, "");
}

const ANY_HIDDEN = new RegExp(HIDDEN.source, "u");

export function hasHidden(text: string): boolean {
  return ANY_HIDDEN.test(text);
}

// The hidden set plus control characters other than tab and newline.
const SHOWN = new RegExp(`${HIDDEN.source}|[\\x00-\\x08\\x0B-\\x1F\\x7F-\\x9F]`, "gu");

/** Hidden and control characters as visible codes (`<U+202E>`), for text
 * the user has to check as it really is: what a web call sends, what a
 * page said to the model. Stripping them would hide exactly what to see. */
export function showHidden(text: string): string {
  return text.replace(SHOWN, (c) => `<U+${c.codePointAt(0)!.toString(16).toUpperCase().padStart(4, "0")}>`);
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
