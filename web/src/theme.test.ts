// The status colours (--ok, --warn, --bad) are text on every surface of
// both themes: progress status words, callout titles, tool results. WCAG
// AA asks 4.5:1 for text; this reads the tokens out of styles.css, so a
// theme that forgets to redefine one fails here.
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

  test("the contrast formula", () => {
    expect(contrast("#000000", "#ffffff")).toBeCloseTo(21);
    expect(contrast("#e3b341", "#f4f4f4")).toBeCloseTo(1.77, 2);
  });
});
