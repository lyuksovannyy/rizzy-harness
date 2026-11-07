// Every colour that carries text is checked against every surface it sits
// on, in both themes: status words, callout titles, tool results, syntax,
// accents and their tints. WCAG AA asks 4.5:1 for text and 3:1 for marks
// and focus rings. This reads the tokens out of styles.css, so a theme that
// forgets to redefine one fails here.
import { describe, expect, test } from "vitest";
import css from "./styles.css?raw";

type Theme = Record<string, string>;

/** The hex colours declared in a block. */
function tokens(block: string): Theme {
  return Object.fromEntries([...block.matchAll(/(--[\w-]+):\s*(#[0-9a-f]{6})\s*;/gi)].map((m) => [m[1]!, m[2]!.toLowerCase()]));
}

/** Every declaration in a block, as written. */
function declarations(block: string): Record<string, string> {
  return Object.fromEntries([...block.matchAll(/(--[\w-]+):\s*([^;]+);/g)].map((m) => [m[1]!, m[2]!.trim()]));
}

const darkBlock = /^:root \{([^}]*)\}/m.exec(css)![1]!;
const osLight = /@media \(prefers-color-scheme: light\) \{\s*:root:not\(\[data-theme="dark"\]\) \{([^}]*)\}/.exec(css)![1]!;
const forcedLight = /^:root\[data-theme="light"\] \{([^}]*)\}/m.exec(css)![1]!;

const ACCENTS = ["blue", "violet", "green", "orange", "pink", "gray"];
/** `--accent` is a pointer into the ramp; the default points at blue. */
const withAccent = (t: Theme): Theme => ({ ...t, "--accent": t["--accent-blue"]! });
const dark = withAccent(tokens(darkBlock));
const light = withAccent({ ...tokens(darkBlock), ...tokens(osLight) });
/** A number token (the art alphas), from the light block when it has one. */
const num = (name: string, block: string) => Number(declarations(block)[name] ?? declarations(darkBlock)[name]);
const THEMES = [
  ["dark", dark, darkBlock],
  ["light", light, osLight],
] as const;

const SURFACES = ["--bg", "--bg-2", "--bg-3", "--bg-3-hover", "--bg-hover", "--surface", "--surface-pop", "--bubble", "--bg-side"];

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
  // The OS-light block and the forced-light block are one theme written
  // twice (CSS cannot share a declaration block between the two selectors).
  test("the forced light theme is the OS light theme", () => {
    expect(declarations(forcedLight)).toEqual(declarations(osLight));
    expect(Object.keys(declarations(osLight)).length).toBeGreaterThan(40);
  });

  test.each(THEMES)("%s: secondary text and status colours are readable on every surface", (_, theme) => {
    for (const tone of ["--fg", "--fg-2", "--ok", "--warn", "--bad"]) {
      for (const surface of SURFACES) {
        const ratio = contrast(theme[tone]!, theme[surface]!);
        expect(ratio, `${tone} ${theme[tone]} on ${surface} ${theme[surface]}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  // Each accent is text (links, labels, the author line), the fill under
  // --accent-fg, a tint under its own text, and the focus ring.
  test.each(THEMES)("%s: every accent works as text, fill and focus ring", (_, theme) => {
    for (const name of ACCENTS) {
      const a = theme[`--accent-${name}`]!;
      expect(a, `--accent-${name}`).toBeDefined();
      for (const surface of ["--bg", "--bg-2", "--bg-3"]) {
        expect(contrast(a, theme[surface]!), `${name} ${a} on ${surface}`).toBeGreaterThanOrEqual(4.5);
      }
      for (const share of [0.09, 0.16]) {
        const tint = mix(a, theme["--bg"]!, share);
        expect(contrast(a, tint), `${name} on its ${share * 100}% tint ${tint}`).toBeGreaterThanOrEqual(4.5);
      }
      expect(contrast(theme["--accent-fg"]!, a), `--accent-fg on ${name}`).toBeGreaterThanOrEqual(4.5);
      expect(contrast(a, theme["--bg"]!), `${name} focus ring on --bg`).toBeGreaterThanOrEqual(3);
    }
  });

  // Links in messages are the accent, and on hover --accent-hover (86%
  // accent, 14% text colour): both are text on the page. The composer's
  // focus border is the accent between the page and the composer itself.
  test.each(THEMES)("%s: message links and the composer's focus border", (_, theme) => {
    expect(declarations(darkBlock)["--accent-hover"]).toBe("color-mix(in srgb, var(--accent) 86%, var(--fg))");
    expect(css).toMatch(/\.md a \{\s*color: var\(--accent\);/);
    expect(css).toMatch(/\.md a:hover \{\s*color: var\(--accent-hover\);/);
    expect(css).toMatch(/\.composer:focus-within \{\s*border-color: var\(--accent\);/);
    for (const name of ACCENTS) {
      const a = theme[`--accent-${name}`]!;
      const hover = mix(a, theme["--fg"]!, 0.86);
      expect(contrast(a, theme["--bg"]!), `${name} link on --bg`).toBeGreaterThanOrEqual(4.5);
      expect(contrast(hover, theme["--bg"]!), `${name} hovered link ${hover} on --bg`).toBeGreaterThanOrEqual(4.5);
      for (const surface of ["--bg", "--surface"]) {
        expect(contrast(a, theme[surface]!), `${name} composer focus on ${surface}`).toBeGreaterThanOrEqual(3);
      }
    }
  });

  // A bar or line is a graphical object: WCAG AA asks 3:1 against what it
  // is drawn on, the card.
  test.each(THEMES)("%s: chart series colours stand out from the card", (_, theme) => {
    for (let k = 1; k <= 6; k++) {
      const c = theme[`--chart-${k}`]!;
      expect(c, `--chart-${k}`).toBeDefined();
      expect(contrast(c, theme["--bg-2"]!), `--chart-${k} ${c} on --bg-2`).toBeGreaterThanOrEqual(3);
    }
  });

  // Code is text on the code block's --bg-2.
  test.each(THEMES)("%s: syntax colours are readable on the code block", (_, theme) => {
    for (const k of ["--syn-kw", "--syn-str", "--syn-com", "--syn-num", "--syn-fn", "--syn-type", "--syn-attr", "--syn-add", "--syn-del"]) {
      expect(contrast(theme[k]!, theme["--bg-2"]!), `${k} ${theme[k]} on --bg-2`).toBeGreaterThanOrEqual(4.5);
    }
  });

  // A callout's label is its tone on a 9% tint of that tone (styles.css,
  // .callout), over the message: Markdown only renders in assistant
  // messages, which have no background of their own.
  test.each(THEMES)("%s: each callout label is readable on its own tint", (_, theme) => {
    for (const tone of ["--accent", "--ok", "--tone-important", "--warn", "--bad"]) {
      const tint = mix(theme[tone]!, theme["--bg"]!, 0.09);
      expect(contrast(theme[tone]!, tint), `${tone} ${theme[tone]} on its tint ${tint}`).toBeGreaterThanOrEqual(4.5);
    }
  });

  // Background art is --fg at up to --art-max over the page: it must read
  // as texture, and text drawn over it by mistake must stay readable.
  test.each(THEMES)("%s: background art stays within its contrast budget", (_, theme, block) => {
    const max = num("--art-max", block);
    expect(max).toBeGreaterThan(0);
    expect(num("--art-peak", block)).toBeGreaterThan(max);
    const ink = mix(theme["--fg"]!, theme["--bg"]!, max);
    expect(contrast(ink, theme["--bg"]!), `ink ${ink} on --bg`).toBeLessThanOrEqual(1.35);
    expect(contrast(theme["--fg-2"]!, ink), `--fg-2 over ink ${ink}`).toBeGreaterThanOrEqual(4.5);
  });

  test("the contrast formula", () => {
    expect(contrast("#000000", "#ffffff")).toBeCloseTo(21);
    expect(contrast("#e3b341", "#f4f4f4")).toBeCloseTo(1.77, 2);
  });
});
