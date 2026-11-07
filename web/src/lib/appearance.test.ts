// @vitest-environment happy-dom
// Theme and accent on <html> and in the mirrors public/theme.js reads
// before the first paint.
import { beforeEach, describe, expect, test } from "vitest";
import { ACCENT_KEY, applyAppearance, BACKDROP_KEY, readBackdropMirror, THEME_KEY, writeBackdropMirror } from "./appearance";

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
    applyAppearance("light", "violet", false);
    expect(root().dataset.theme).toBe("light");
    expect(root().dataset.accent).toBe("violet");
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
