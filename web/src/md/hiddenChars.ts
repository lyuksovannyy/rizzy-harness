// Hidden characters: bidi controls (U+061C, U+200E-200F, U+202A-202E,
// U+2066-2069), zero-width characters (U+200B-200D, U+2060-2064), the BOM,
// private-use characters, and the default-ignorable code points no script
// needs: deprecated format characters (U+2065-206F), unassigned specials
// (U+FFF0-FFF8), musical beam controls (U+1D173-1D17A) and the whole tag and
// variation-selector plane (U+E0000-E0FFF). They hide or reorder text, so
// what the user reads (and what Copy gives them) would differ from what is
// there; tag characters and variation selectors are the usual carriers for
// invisible prompt-injection text. The soft hyphen and the BMP variation
// selectors (emoji style) shape real text and are not stripped. clean_text
// in Rust strips the same set.
//
// This module is the small part the first screen needs (chat titles and
// node names in the sidebar); md/hidden.ts adds the strip and the Markdown
// pass on top of it, in the lazy chunks.

export const HIDDEN_SRC =
  "[\\u061C\\u200B-\\u200F\\u202A-\\u202E\\u2060-\\u206F\\uFEFF\\uFFF0-\\uFFF8\\uE000-\\uF8FF\\u{1D173}-\\u{1D17A}\\u{E0000}-\\u{E0FFF}]";

/** What showHidden marks, as a regex source: the hidden set; control
 * characters other than tab and newline (so a carriage return too); spaces
 * that are not the plain space and blanks that render as nothing (no-break
 * and typographic spaces, the line and paragraph separators, the soft
 * hyphen, the combining grapheme joiner, the Hangul fillers, the Khmer and
 * Mongolian invisible vowels); and the BMP variation selectors except
 * right after a pictograph, where they pick the emoji's style. */
export const SHOWN_SRC = `${HIDDEN_SRC}|[\\x00-\\x08\\x0B-\\x1F\\x7F-\\x9F\\u00A0\\u00AD\\u034F\\u115F\\u1160\\u1680\\u17B4\\u17B5\\u180E\\u2000-\\u200A\\u2028\\u2029\\u202F\\u205F\\u3000\\u3164\\uFFA0]|(?<!\\p{Extended_Pictographic})[\\uFE00-\\uFE0F]`;
const SHOWN = new RegExp(SHOWN_SRC, "gu");

/** `<U+202E>` for one character. */
export const codeOf = (c: string) => `<U+${c.codePointAt(0)!.toString(16).toUpperCase().padStart(4, "0")}>`;

/** Hidden, control and unusual blank characters as visible codes
 * (`<U+202E>`), for text the user has to check as it really is: what a web
 * call sends, what a page said to the model, a tool call's input, a node's
 * error. Stripping them would hide exactly what to see. */
export const showHidden = (text: string) => text.replace(SHOWN, codeOf);

/** showHidden for a name or a title: the same, except that the zero-width
 * joiner stays. It joins emoji (👩‍💻), which titles carry, and it reorders
 * nothing; every bidi control, tag and other invisible character is still
 * shown. */
export const shownName = (text: string) => text.replace(SHOWN, (c) => (c === "‍" ? c : codeOf(c)));
