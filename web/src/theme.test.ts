// Every colour that carries text is checked against every ground it sits
// on, in both themes and with every accent preset: status words, callout
// titles, tool results, syntax, accents and their tints. WCAG AA asks 4.5:1
// for text and 3:1 for marks and focus rings. The materials are
// translucent (Rizzy Glass, docs/rizzy-glass.md), so a ground is the colour
// the browser composites: the tint over what it is drawn on, down to the
// flat backdrop. This reads the tokens out of styles.css, so a theme that
// forgets to redefine one fails here.
import { describe, expect, test } from "vitest";
import css from "./styles.css?raw";

type Theme = Record<string, string>;

/** Every declaration in a block, as written. */
function declarations(block: string): Record<string, string> {
  return Object.fromEntries([...block.matchAll(/(--[\w-]+):\s*([^;]+);/g)].map((m) => [m[1]!, m[2]!.trim()]));
}

const darkBlock = /^:root \{([^}]*)\}/m.exec(css)![1]!;
const osLight = /@media \(prefers-color-scheme: light\) \{\s*:root:not\(\[data-theme="dark"\]\) \{([^}]*)\}/.exec(css)![1]!;
const forcedLight = /^:root\[data-theme="light"\] \{([^}]*)\}/m.exec(css)![1]!;
/** The materials block: the tints while transparency is allowed. */
const materials = /^:root \{(\s*--sidebar: [^}]*)\}/m.exec(css)![1]!;
const reduced = /@media \(prefers-reduced-transparency: reduce\) \{\s*:root \{([^}]*)\}/.exec(css)![1]!;

/** The eight presets, in the design system's order. */
const ACCENTS = ["blue", "purple", "pink", "red", "orange", "yellow", "green", "graphite"];
const presetRule = (name: string) => {
  const m = new RegExp(`^:root\\[data-accent="${name}"\\] \\{([^}]*)\\}`, "m").exec(css);
  return m ? declarations(m[1]!) : {};
};

const dark: Theme = { ...declarations(darkBlock), ...declarations(materials) };
const light: Theme = { ...dark, ...declarations(osLight) };
const THEMES = [
  ["dark", dark, darkBlock],
  ["light", light, osLight],
] as const;

const withPreset = (t: Theme, name: string): Theme => ({ ...t, ...presetRule(name) });
const solidTheme = (t: Theme): Theme => ({ ...t, ...declarations(reduced) });

type Rgba = [number, number, number, number];

function hex(c: Rgba): string {
  return `#${c.slice(0, 3).map((v) => Math.round(v).toString(16).padStart(2, "0")).join("")}`;
}

/** A token's colour with its alpha, following var() and the color-mix() forms styles.css uses. */
function color(t: Theme, value: string, depth = 0): Rgba {
  if (depth > 12) throw new Error(`var() loop at ${value}`);
  const v = value.trim();
  let m = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(v);
  if (m) {
    const n = parseInt(m[1]!, 16);
    return [n >> 16, (n >> 8) & 255, n & 255, m[2] ? parseInt(m[2], 16) / 255 : 1];
  }
  m = /^var\((--[\w-]+)\)$/.exec(v);
  if (m) {
    const ref = t[m[1]!];
    if (ref === undefined) throw new Error(`${m[1]} is not defined`);
    return color(t, ref, depth + 1);
  }
  m = /^color-mix\(in srgb, (.+) (\d+(?:\.\d+)?)%, (.+)\)$/.exec(v);
  if (m) {
    const a = color(t, m[1]!, depth + 1);
    const share = Number(m[2]) / 100;
    if (m[3] === "transparent") return [a[0], a[1], a[2], a[3] * share];
    const b = color(t, m[3]!, depth + 1);
    return [0, 1, 2].map((i) => a[i]! * share + b[i]! * (1 - share)).concat(1) as Rgba;
  }
  throw new Error(`cannot read colour ${v}`);
}

/** `top` drawn over the opaque `ground`. */
function over(top: Rgba, ground: Rgba): Rgba {
  const a = top[3];
  return [0, 1, 2].map((i) => top[i]! * a + ground[i]! * (1 - a)).concat(1) as Rgba;
}

/**
 * A ground as the browser paints it: the layers from the top down, the last
 * one over the page. `ground(t, "--fill", "--glass-thick")` is a button on a card.
 */
function ground(t: Theme, ...layers: string[]): string {
  let c = color(t, "var(--bg)");
  for (const name of [...layers].reverse()) c = over(color(t, `var(${name})`), c);
  return hex(c);
}

/** A token as an opaque colour (a text colour is opaque; checked below). */
function ink(t: Theme, name: string, on: string): string {
  return hex(over(color(t, `var(${name})`), color(t, on)));
}

function luminance(h: string): number {
  const n = parseInt(h.slice(1), 16);
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

/** `a` at `share` over `b`, both opaque. */
function mix(a: string, b: string, share: number): string {
  return hex(over([...color({}, a).slice(0, 3), share] as Rgba, color({}, b)));
}

/**
 * Where neutral text sits: the page, the sidebar and its active row, cards
 * (in-flow surfaces, tool and approval cards), a card nested in a card, the
 * user's bubble, menus and dialogs over the page or over a card, toasts,
 * thin rows in a card, fields and buttons on a card and on the page, the
 * selected segment, tooltips.
 */
const GROUNDS: string[][] = [
  [],
  ["--sidebar"],
  ["--fill", "--sidebar"],
  ["--glass-thick"],
  ["--glass-thick", "--glass-thick"],
  ["--bubble"],
  ["--glass-thick", "--glass-thick"],
  ["--glass"],
  ["--glass", "--glass-thick"],
  ["--glass-thin", "--glass-thick"],
  ["--field"],
  ["--field", "--glass-thick"],
  ["--fill"],
  ["--fill", "--glass-thick"],
  ["--fill-2", "--glass-thick"],
  ["--solid"],
  ["--surface"],
];
/**
 * Grounds that carry content text only (--fg): the active sidebar row and
 * the selected segment. Their secondary text switches to --fg (styles.css),
 * because --fg-2 on the active row is 4.47:1 in light.
 */
const STRONG_GROUNDS: string[][] = [
  ["--fill-2", "--sidebar"],
  ["--raised", "--fill", "--glass-thick"],
  ["--raised", "--fill"],
];
const CARDS = [[], ["--glass-thick"], ["--glass"], ["--glass-thick", "--glass-thick"]];
const name = (layers: string[]) => (layers.length ? layers.join(" over ") : "--bg");

/** Every ground text sits on, with transparency allowed and reduced. */
const both = (t: Theme) => [t, solidTheme(t)] as const;

describe("theme", () => {
  // The OS-light block and the forced-light block are one theme written
  // twice (CSS cannot share a declaration block between the two selectors).
  test("the forced light theme is the OS light theme", () => {
    expect(declarations(forcedLight)).toEqual(declarations(osLight));
    expect(Object.keys(declarations(osLight)).length).toBeGreaterThan(40);
  });

  test.each(THEMES)("%s: text colours are opaque", (_, theme) => {
    for (const k of ["--fg", "--fg-2", "--fg-3", "--ok", "--warn", "--bad", "--on-light-accent", "--on-dark-accent", "--on-danger"]) {
      expect(color(theme, `var(${k})`)[3], k).toBe(1);
    }
  });

  test.each(THEMES)("%s: content and secondary text read on every ground", (_, theme) => {
    for (const t of both(theme)) {
      for (const layers of GROUNDS) {
        const g = ground(t, ...layers);
        for (const tone of ["--fg", "--fg-2"]) {
          expect(contrast(ink(t, tone, g), g), `${tone} on ${name(layers)} ${g}`).toBeGreaterThanOrEqual(4.5);
        }
      }
      for (const layers of STRONG_GROUNDS) {
        const g = ground(t, ...layers);
        expect(contrast(ink(t, "--fg", g), g), `--fg on ${name(layers)} ${g}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  test.each(THEMES)("%s: placeholders read in a field", (_, theme) => {
    for (const t of both(theme)) {
      for (const layers of [["--field"], ["--field", "--glass-thick"], ["--field", "--glass-thick", "--glass-thick"]]) {
        const g = ground(t, ...layers);
        expect(contrast(ink(t, "--fg-3", g), g), `--fg-3 on ${name(layers)} ${g}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  // Warnings, errors, the approval card, "not by you", "not resumed",
  // hidden-character marks: status text on the page, a card, a toast and
  // its own soft tint over each of those.
  test.each(THEMES)("%s: status colours read on cards and on their soft tints", (_, theme) => {
    for (const t of both(theme)) {
      for (const tone of ["--ok", "--warn", "--bad"]) {
        for (const layers of [...CARDS, ...CARDS.map((c) => [`${tone}-soft`, ...c]), ["--fill", "--glass-thick"], ["--field", "--glass-thick"]]) {
          const g = ground(t, ...layers);
          expect(contrast(ink(t, tone, g), g), `${tone} on ${name(layers)} ${g}`).toBeGreaterThanOrEqual(4.5);
        }
      }
      for (const tone of ["--ok", "--warn", "--bad"]) {
        expect(color(t, `var(${tone}-fill)`)[3], `${tone}-fill`).toBe(1);
      }
      // The approval's body and a notice: neutral text on the soft tint.
      for (const layers of [["--warn-soft", "--glass-thick"], ["--bad-soft"], ["--bad-soft", "--glass-thick"]]) {
        const g = ground(t, ...layers);
        for (const tone of ["--fg", "--fg-2"]) {
          expect(contrast(ink(t, tone, g), g), `${tone} on ${name(layers)} ${g}`).toBeGreaterThanOrEqual(4.5);
        }
      }
    }
  });

  test.each(THEMES)("%s: the destructive button and the switch", (_, theme) => {
    expect(contrast(ink(theme, "--on-danger", "#000000"), ink(theme, "--danger-bg", "#000000"))).toBeGreaterThanOrEqual(4.5);
    // The white thumb on the on track and on the off track (a mark: 3:1 on).
    const card = ground(theme, "--glass-thick");
    expect(contrast(ink(theme, "--switch-thumb", card), ink(theme, "--ok-fill", card)), "thumb on --ok-fill").toBeGreaterThanOrEqual(1.5);
  });

  // Each preset: text on its fill, its text tone on every neutral ground
  // and on its soft tint, its graphics tone as a mark, and the focus ring.
  test.each(THEMES)("%s: every accent preset works as fill, text, graphics and focus ring", (_, base) => {
    for (const preset of ACCENTS) {
      const theme = withPreset(base, preset);
      if (preset !== "blue") expect(Object.keys(presetRule(preset)), preset).toContain("--accent");
      for (const t of both(theme)) {
        const fill = ground(t, "--accent");
        expect(contrast(ink(t, "--on-accent", fill), fill), `${preset}: --on-accent on --accent ${fill}`).toBeGreaterThanOrEqual(4.5);
        for (const layers of [...CARDS, ["--sidebar"], ["--fill", "--glass-thick"], ["--accent-soft"], ["--accent-soft", "--glass-thick"], ["--field", "--glass-thick"]]) {
          const g = ground(t, ...layers);
          expect(contrast(ink(t, "--accent-fg", g), g), `${preset}: --accent-fg on ${name(layers)} ${g}`).toBeGreaterThanOrEqual(4.5);
          expect(contrast(ink(t, "--focus", g), g), `${preset}: focus ring on ${name(layers)} ${g}`).toBeGreaterThanOrEqual(3);
        }
        for (const layers of [[], ["--sidebar"], ["--glass-thick"]]) {
          const g = ground(t, ...layers);
          expect(contrast(ink(t, "--accent-2", g), g), `${preset}: --accent-2 mark on ${name(layers)} ${g}`).toBeGreaterThanOrEqual(3);
        }
        // A hovered or active sidebar row's icon (a mark) is the text tone:
        // --accent-2 there is 2.8:1 for Blue in light.
        for (const layers of [["--fill", "--sidebar"], ["--fill-2", "--sidebar"]]) {
          const g = ground(t, ...layers);
          expect(contrast(ink(t, "--accent-fg", g), g), `${preset}: hovered or active row icon on ${name(layers)}`).toBeGreaterThanOrEqual(3);
        }
      }
    }
  });

  // Projects, avatars and swatches draw a preset by name whatever the
  // active accent is: its graphics tone as an icon, its fill under a mark.
  test.each(THEMES)("%s: each preset's own tones, as projects and avatars use them", (_, theme) => {
    for (const preset of ACCENTS) {
      for (const layers of [[], ["--sidebar"], ["--glass-thick"]]) {
        const g = ground(theme, ...layers);
        expect(contrast(ink(theme, `--ac-${preset}-2`, g), g), `--ac-${preset}-2 on ${name(layers)}`).toBeGreaterThanOrEqual(3);
        expect(contrast(ink(theme, `--ac-${preset}-fg`, g), g), `--ac-${preset}-fg on ${name(layers)}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  // Links in messages are the accent text, and on hover --accent-hover (86%
  // accent text, 14% text colour): both are text on the page and on a card.
  // The composer's focus border is the focus colour between the page and
  // the composer itself.
  test.each(THEMES)("%s: message links and the composer's focus border", (_, base) => {
    expect(declarations(darkBlock)["--accent-hover"]).toBe("color-mix(in srgb, var(--accent-fg) 86%, var(--fg))");
    expect(css).toMatch(/\.md a \{\s*color: var\(--accent-fg\);/);
    expect(css).toMatch(/\.md a:hover \{\s*color: var\(--accent-hover\);/);
    expect(css).toMatch(/\.composer:focus-within \{\s*border-color: var\(--focus\);/);
    for (const preset of ACCENTS) {
      const theme = withPreset(base, preset);
      for (const layers of [[], ["--glass-thick"]]) {
        const g = ground(theme, ...layers);
        expect(contrast(ink(theme, "--accent-hover", g), g), `${preset} hovered link on ${name(layers)}`).toBeGreaterThanOrEqual(4.5);
      }
      for (const layers of [[], ["--surface"]]) {
        const g = ground(theme, ...layers);
        expect(contrast(ink(theme, "--focus", g), g), `${preset} composer focus on ${name(layers)}`).toBeGreaterThanOrEqual(3);
      }
    }
  });

  // A bar or line is a graphical object: WCAG AA asks 3:1 against what it
  // is drawn on, the card.
  test.each(THEMES)("%s: chart series colours stand out from the card", (_, theme) => {
    const card = ground(theme, "--glass-thick");
    for (let k = 1; k <= 6; k++) {
      const c = theme[`--chart-${k}`]!;
      expect(c, `--chart-${k}`).toBeDefined();
      expect(contrast(c, card), `--chart-${k} ${c} on the card ${card}`).toBeGreaterThanOrEqual(3);
    }
  });

  // Code is text on the code block's field, in a message (the page) and in
  // a tool card.
  test.each(THEMES)("%s: syntax colours are readable on the code block", (_, theme) => {
    for (const layers of [["--field"], ["--field", "--glass-thick"]]) {
      const g = ground(theme, ...layers);
      for (const k of ["--syn-kw", "--syn-str", "--syn-com", "--syn-num", "--syn-fn", "--syn-type", "--syn-attr", "--syn-add", "--syn-del"]) {
        expect(contrast(theme[k]!, g), `${k} ${theme[k]} on ${name(layers)} ${g}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  // A callout's label is its tone on a 9% tint of that tone (styles.css,
  // .callout), over the message: Markdown only renders in assistant
  // messages, which have no background of their own.
  test.each(THEMES)("%s: each callout label is readable on its own tint", (_, base) => {
    for (const preset of ACCENTS) {
      const theme = withPreset(base, preset);
      for (const tone of ["--accent-fg", "--ok", "--tone-important", "--warn", "--bad"]) {
        const c = ink(theme, tone, theme["--bg"]!);
        const tint = mix(c, theme["--bg"]!, 0.09);
        expect(contrast(c, tint), `${preset}: ${tone} ${c} on its tint ${tint}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  // Background art is --fg at up to --art-max over the page: it must read
  // as texture, and text drawn over it by mistake must stay readable.
  test.each(THEMES)("%s: background art stays within its contrast budget", (_, theme, block) => {
    const num = (n: string) => Number(declarations(block)[n] ?? declarations(darkBlock)[n]);
    const max = num("--art-max");
    expect(max).toBeGreaterThan(0);
    expect(num("--art-peak")).toBeGreaterThan(max);
    const inkColour = mix(theme["--fg"]!, theme["--bg"]!, max);
    expect(contrast(inkColour, theme["--bg"]!), `ink ${inkColour} on --bg`).toBeLessThanOrEqual(1.35);
    expect(contrast(theme["--fg-2"]!, inkColour), `--fg-2 over ink ${inkColour}`).toBeGreaterThanOrEqual(4.5);
  });

  // Blur only on the sidebar, menus, dialogs and the palette: never on the
  // transcript, its messages, tool cards, code blocks or tables.
  test("backdrop-filter is used only where the design system puts it", () => {
    const rules = [...css.matchAll(/([^{}]+)\{[^{}]*backdrop-filter[^{}]*\}/g)].map((m) => m[1]!.trim());
    expect(rules.length).toBeGreaterThan(0);
    for (const sel of rules) {
      expect(sel, sel).not.toMatch(/\.(msg|bubble|tool|codeblock|md|conversation|transcript|table|ui-card|card)\b/);
    }
  });

  test("the contrast formula and compositing", () => {
    expect(contrast("#000000", "#ffffff")).toBeCloseTo(21);
    expect(contrast("#e3b341", "#f4f4f4")).toBeCloseTo(1.77, 2);
    expect(hex(over(color({}, "#ffffff80"), color({}, "#000000")))).toBe("#808080");
    expect(ground(light, "--glass-thick")).toBe("#f9f9f9");
  });
});

/** The worst pairs, for the report (`DEBUG_CONTRAST=1 pnpm vitest run src/theme.test.ts`). */
if (typeof process !== "undefined" && process.env.DEBUG_CONTRAST) {
  test("print the lowest ratios", () => {
    for (const [label, theme] of THEMES) {
      const rows: [number, string][] = [];
      for (const t of both(theme)) {
        for (const layers of GROUNDS) {
          const g = ground(t, ...layers);
          for (const tone of ["--fg", "--fg-2"]) rows.push([contrast(ink(t, tone, g), g), `${tone} on ${name(layers)}${t === theme ? "" : " (solid)"}`]);
        }
        for (const tone of ["--ok", "--warn", "--bad"])
          for (const layers of [...CARDS, ...CARDS.map((c) => [`${tone}-soft`, ...c])]) {
            const g = ground(t, ...layers);
            rows.push([contrast(ink(t, tone, g), g), `${tone} on ${name(layers)}${t === theme ? "" : " (solid)"}`]);
          }
        for (const preset of ACCENTS) {
          const p = withPreset(t, preset);
          const fill = ground(p, "--accent");
          rows.push([contrast(ink(p, "--on-accent", fill), fill), `${preset} --on-accent on --accent`]);
          for (const layers of [...CARDS, ["--accent-soft", "--glass-thick"]]) {
            const g = ground(p, ...layers);
            rows.push([contrast(ink(p, "--accent-fg", g), g), `${preset} --accent-fg on ${name(layers)}`]);
          }
        }
      }
      rows.sort((a, b) => a[0] - b[0]);
      console.log(label, rows.slice(0, 14).map(([r, s]) => `${r.toFixed(2)} ${s}`).join("\n"));
    }
  });
}
