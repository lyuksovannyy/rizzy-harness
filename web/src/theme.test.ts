// The status colours (--ok, --warn, --bad) are text on every surface of
// both themes: progress status words, callout titles, tool results; so are
// the syntax colours and the callout tones. WCAG AA asks 4.5:1 for text;
// this reads the tokens out of styles.css, so a theme that forgets to
// redefine one fails here.
import { describe, expect, test } from "vitest";
import css from "./styles.css?raw";

/** The custom properties declared in a `:root { … }` block. */
function tokens(block: string): Record<string, string> {
  return Object.fromEntries([...block.matchAll(/(--[\w-]+):\s*(#[0-9a-f]{6})\s*;/gi)].map((m) => [m[1]!, m[2]!.toLowerCase()]));
}

const dark = tokens(/^:root \{([^}]*)\}/m.exec(css)![1]!);
const light = { ...dark, ...tokens(/@media \(prefers-color-scheme: light\) \{\s*:root \{([^}]*)\}/.exec(css)![1]!) };

function luminance(hex: string): number {
  const n = parseInt(hex.slice(1), 16);
  const [r, g, b] = [n >> 16, (n >> 8) & 255, n & 255].map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
}

/** `a` at `share` over `b` (color-mix in srgb with transparent, drawn on b). */
function mix(a: string, b: string, share: number): string {
  const ch = (hex: string) => [16, 8, 0].map((s) => (parseInt(hex.slice(1), 16) >> s) & 255);
  const [x, y] = [ch(a), ch(b)];
  return `#${x.map((c, i) => Math.round(c * share + y[i]! * (1 - share)).toString(16).padStart(2, "0")).join("")}`;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi! + 0.05) / (lo! + 0.05);
}

describe("theme", () => {
  test.each([
    ["dark", dark],
    ["light", light],
  ] as const)("%s: status colours are readable text on every surface", (_, theme) => {
    for (const tone of ["--ok", "--warn", "--bad"]) {
      for (const surface of ["--bg", "--bg-2", "--bg-3", "--bg-hover"]) {
        const ratio = contrast(theme[tone]!, theme[surface]!);
        expect(ratio, `${tone} ${theme[tone]} on ${surface} ${theme[surface]}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  // A bar or line is a graphical object: WCAG AA asks 3:1 against what it
  // is drawn on, the card.
  test.each([
    ["dark", dark],
    ["light", light],
  ] as const)("%s: chart series colours stand out from the card", (_, theme) => {
    for (let k = 1; k <= 6; k++) {
      const c = theme[`--chart-${k}`]!;
      expect(c, `--chart-${k}`).toBeDefined();
      expect(contrast(c, theme["--bg-2"]!), `--chart-${k} ${c} on --bg-2`).toBeGreaterThanOrEqual(3);
    }
  });

  // Code is text on the code block's --bg-2.
  test.each([
    ["dark", dark],
    ["light", light],
  ] as const)("%s: syntax colours are readable on the code block", (_, theme) => {
    for (const k of ["--syn-kw", "--syn-str", "--syn-com", "--syn-num", "--syn-fn", "--syn-type", "--syn-attr", "--syn-add", "--syn-del"]) {
      expect(contrast(theme[k]!, theme["--bg-2"]!), `${k} ${theme[k]} on --bg-2`).toBeGreaterThanOrEqual(4.5);
    }
  });

  // A callout's label is its tone on a 9% tint of that tone (styles.css,
  // .callout), over the message: Markdown only renders in assistant
  // messages, which have no background of their own.
  test.each([
    ["dark", dark],
    ["light", light],
  ] as const)("%s: each callout label is readable on its own tint", (_, theme) => {
    for (const tone of ["--accent", "--ok", "--tone-important", "--warn", "--bad"]) {
      const tint = mix(theme[tone]!, theme["--bg"]!, 0.09);
      expect(contrast(theme[tone]!, tint), `${tone} ${theme[tone]} on its tint ${tint}`).toBeGreaterThanOrEqual(4.5);
    }
  });

  test("the contrast formula", () => {
    expect(contrast("#000000", "#ffffff")).toBeCloseTo(21);
    expect(contrast("#e3b341", "#f4f4f4")).toBeCloseTo(1.77, 2);
  });
});
