// @vitest-environment happy-dom
// Theme and accent on <html> and in the mirrors public/theme.js reads
// before the first paint.
import { beforeEach, describe, expect, test } from "vitest";
import script from "../../public/theme.js?raw";
import { ACCENT_KEY, ACCENTS, applyAppearance, BACKDROP_KEY, normalizeAccent, readBackdropMirror, THEME_KEY, writeBackdropMirror } from "./appearance";

const root = () => document.documentElement;

beforeEach(() => {
  localStorage.clear();
  delete root().dataset.theme;
  delete root().dataset.accent;
  document.head.innerHTML =
    '<meta name="theme-color" content="#111113" media="(prefers-color-scheme: dark)">' +
    '<meta name="theme-color" content="#fcfcfc" media="(prefers-color-scheme: light)">';
});

const metas = () => [...document.querySelectorAll<HTMLMetaElement>('meta[name="theme-color"]')].map((m) => m.content);

describe("applyAppearance", () => {
  test("a forced theme and accent go on <html> and into the mirrors", () => {
    applyAppearance("dark", "green");
    expect(root().dataset.theme).toBe("dark");
    expect(root().dataset.accent).toBe("green");
    expect(localStorage.getItem(THEME_KEY)).toBe("dark");
    expect(localStorage.getItem(ACCENT_KEY)).toBe("green");
  });

  test("system and blue are the defaults: no attribute, no mirror", () => {
    applyAppearance("light", "pink");
    for (const theme of [null, "system"]) {
      applyAppearance(theme, "blue");
      expect(root().dataset.theme).toBeUndefined();
      expect(root().dataset.accent).toBeUndefined();
      expect(localStorage.getItem(THEME_KEY)).toBeNull();
      expect(localStorage.getItem(ACCENT_KEY)).toBeNull();
    }
  });

  test("a preview changes the page but not what the next load starts with", () => {
    applyAppearance("dark", "orange");
    applyAppearance("light", "purple", false);
    expect(root().dataset.theme).toBe("light");
    expect(root().dataset.accent).toBe("purple");
    expect(localStorage.getItem(THEME_KEY)).toBe("dark");
    expect(localStorage.getItem(ACCENT_KEY)).toBe("orange");
  });

  test("the browser bar follows a forced theme and returns to the OS colours", () => {
    root().style.setProperty("--bg", "#123456");
    applyAppearance("dark", null);
    expect(metas()).toEqual(["#123456", "#123456"]);
    applyAppearance(null, null);
    expect(metas()).toEqual(["#111113", "#fcfcfc"]);
    root().style.removeProperty("--bg");
  });
});

describe("accent presets", () => {
  test("the eight Rizzy Glass presets, in the design system's order", () => {
    expect(ACCENTS).toEqual(["blue", "purple", "pink", "red", "orange", "yellow", "green", "graphite"]);
  });

  test("the retired names map to their successors; anything unknown is the default", () => {
    expect(normalizeAccent("violet")).toBe("purple");
    expect(normalizeAccent("gray")).toBe("graphite");
    for (const a of ACCENTS) expect(normalizeAccent(a)).toBe(a);
    for (const a of [null, undefined, "", "teal", "Blue", "blue;x"]) expect(normalizeAccent(a)).toBeNull();
  });

  test("a saved old name is shown, and mirrored, as the new one", () => {
    applyAppearance(null, "violet");
    expect(root().dataset.accent).toBe("purple");
    expect(localStorage.getItem(ACCENT_KEY)).toBe("purple");
    applyAppearance(null, "teal");
    expect(root().dataset.accent).toBeUndefined();
    expect(localStorage.getItem(ACCENT_KEY)).toBeNull();
  });

  // public/theme.js runs before the bundle: the same mapping, by hand.
  test("theme.js applies the same mapping before the first paint", () => {
        const run = () => new Function(script)();
    for (const [saved, shown] of [["violet", "purple"], ["gray", "graphite"], ["yellow", "yellow"], ["graphite", "graphite"], ["blue", undefined], ["teal", undefined], ['x" onload="', undefined]] as const) {
      delete root().dataset.accent;
      localStorage.setItem(ACCENT_KEY, saved);
      run();
      expect(root().dataset.accent, saved).toBe(shown);
    }
  });
});

describe("the home background mirror", () => {
  test("still and off are remembered; animated is the default and leaves nothing", () => {
    expect(readBackdropMirror()).toBe("animated");
    writeBackdropMirror("off");
    expect(localStorage.getItem(BACKDROP_KEY)).toBe("off");
    expect(readBackdropMirror()).toBe("off");
    writeBackdropMirror("still");
    expect(readBackdropMirror()).toBe("still");
    writeBackdropMirror(null);
    expect(localStorage.getItem(BACKDROP_KEY)).toBeNull();
    localStorage.setItem(BACKDROP_KEY, "loud");
    expect(readBackdropMirror()).toBe("animated");
  });
});
