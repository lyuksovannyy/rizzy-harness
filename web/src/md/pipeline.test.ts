import { describe, expect, test } from "vitest";
import { normalizeMath, prepare, stripHidden, urlTransform } from "./pipeline";

describe("stripHidden", () => {
  test("removes bidi, zero-width, BOM, private-use and tag characters", () => {
    const hidden = "\u202E\u2066\u2069\u200B\u200D\u2060\uFEFF\uE000\uF8FF\u{E0041}\u{E007F}";
    expect(stripHidden(`a${hidden}b`)).toBe("ab");
  });
  test("keeps ordinary text, emoji and accents", () => {
    expect(stripHidden("café 🚀 日本 ñ\n\ttab")).toBe("café 🚀 日本 ñ\n\ttab");
  });
});

describe("normalizeMath", () => {
  test("inline \\( \\) becomes $$ $$", () => {
    expect(normalizeMath("so \\(a^2+b^2=c^2\\) holds")).toBe("so $$a^2+b^2=c^2$$ holds");
  });
  test("adjacent inline math stays two pieces", () => {
    expect(normalizeMath("\\(a\\)\\(b\\)")).toBe("$$a$$ $$b$$");
  });
  test("an escaped \\\\( is not math", () => {
    expect(normalizeMath("a \\\\(b\\) c")).toBe("a \\\\(b\\) c");
  });
  test("\\[ \\] on their own lines becomes a $$ block", () => {
    expect(normalizeMath("before\n\\[\nx^2\n\\]\nafter")).toBe("before\n$$\nx^2\n$$\nafter");
    expect(normalizeMath("  \\[ x^2 \\]")).toBe("  $$\n  x^2\n  $$");
    expect(normalizeMath("> \\[\n> x\n> \\]")).toBe("> $$\n> x\n> $$");
  });
  test("an unclosed \\[ is left alone (it would swallow the rest)", () => {
    expect(normalizeMath("\\[\nx^2\nmore text")).toBe("\\[\nx^2\nmore text");
  });
  test("code spans and fenced code are untouched", () => {
    expect(normalizeMath("`\\(x\\)` and \\(y\\)")).toBe("`\\(x\\)` and $$y$$");
    expect(normalizeMath("``a ` \\(x\\)`` \\(y\\)")).toBe("``a ` \\(x\\)`` $$y$$");
    const fenced = "```tex\n\\(x\\)\n\\[\ny\n\\]\n```\n\\(z\\)";
    expect(normalizeMath(fenced)).toBe("```tex\n\\(x\\)\n\\[\ny\n\\]\n```\n$$z$$");
    expect(normalizeMath("~~~~\n\\(x\\)\n~~~\n\\(x\\)\n~~~~")).toBe("~~~~\n\\(x\\)\n~~~\n\\(x\\)\n~~~~");
    // A one-line ```span``` is not a fence opener.
    expect(normalizeMath("```a``` then\n\\(y\\)")).toBe("```a``` then\n$$y$$");
  });
  test("single dollars are never math", () => {
    expect(normalizeMath("$5 and $10")).toBe("$5 and $10");
    expect(prepare("echo $HOME/$PATH costs $5")).toBe("echo $HOME/$PATH costs $5");
  });
});

describe("prepare", () => {
  test("strips first, then normalises", () => {
    expect(prepare("\\(\u202Ex\\)")).toBe("$$x$$");
  });
});

describe("urlTransform", () => {
  test("keeps http, https, mailto and fragments", () => {
    for (const u of ["https://a.example/x?q=1", "http://a.example", "MAILTO:a@b.example", "#user-content-fn-1"]) {
      expect(urlTransform(u, "href")).toBe(u);
    }
  });
  test("drops every other scheme and relative URLs", () => {
    for (const u of ["javascript:alert(1)", "JaVaScRiPt:x", "data:text/html,x", "vbscript:x", "file:///etc/passwd", "/api/me", "//evil.example", "x.html", "ftp://a"]) {
      expect(urlTransform(u, "href")).toBe("");
    }
  });
  test("a data: image keeps only its scheme", () => {
    expect(urlTransform("data:image/png;base64,AAAA", "src")).toBe("data:");
    expect(urlTransform("data:image/png;base64,AAAA", "href")).toBe("");
  });
});
