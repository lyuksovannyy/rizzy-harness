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

/** Characters that render as nothing or as a blank, or only change how their
 * neighbours are laid out, but are letters, symbols or marks by category and
 * so a rule by category would pass them: the braille blank, the Hangul and
 * Khmer fillers, the combining grapheme joiner, a musical null notehead and
 * the object replacement character. Most are also default-ignorable; they
 * are named so that an engine with older Unicode tables still marks them. */
export const NAMED_BLANKS = [
  "͏", // combining grapheme joiner
  "ᅟ", // Hangul choseong filler
  "ᅠ", // Hangul jungseong filler
  "឴", // Khmer inherent vowel Aq
  "឵", // Khmer inherent vowel Aa
  "⠀", // braille pattern blank
  "ㅤ", // Hangul filler
  "ﾠ", // halfwidth Hangul filler
  "￼", // object replacement character
  "\u{1D159}", // musical symbol null notehead
];

/** What showHidden marks, as a regex source. A rule, not a list: every
 * character that is invisible or blank when drawn, by Unicode category (an
 * engine that has the property escapes, ES2018, which every browser the
 * build targets does): control (Cc; a tab and a line feed stay, they are how
 * text is laid out, and the card counts their runs), format (Cf: bidi
 * controls, zero-width characters, the soft hyphen, the interlinear
 * annotation marks, shorthand and Egyptian format controls), private use
 * (Co, every plane), unassigned and noncharacter (Cn), lone surrogates (Cs),
 * every space but the plain one (Zs), line and paragraph separators (Zl,
 * Zp), and the default-ignorable code points (Hangul fillers, variation
 * selectors, tags, the combining grapheme joiner), plus NAMED_BLANKS. This
 * contains the strip's set above. A variation selector right after a
 * pictograph picks the emoji's style and is not marked. */
export const SHOWN_SRC =
  "(?![ \\t\\n\\uFE00-\\uFE0F])[\\p{Cc}\\p{Cf}\\p{Co}\\p{Cn}\\p{Cs}\\p{Zs}\\p{Zl}\\p{Zp}\\p{Default_Ignorable_Code_Point}" +
  NAMED_BLANKS.join("") +
  "]|(?<!\\p{Extended_Pictographic})[\\uFE00-\\uFE0F]";
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
