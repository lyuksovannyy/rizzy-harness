import { describe, expect, test } from "vitest";
import { hasHidden, stripHidden } from "./hidden";
import { normalizeMath, prepare, urlTransform } from "./pipeline";

describe("stripHidden", () => {
  test("removes bidi, zero-width, BOM, private-use, tag characters and variation selectors 17-256", () => {
    const hidden =
      "\u{202E}\u{2066}\u{2069}\u{200B}\u{200D}\u{2060}\u{FEFF}\u{E000}\u{F8FF}\u{E0041}\u{E007F}\u{061C}\u{E0100}\u{E01EF}";
    expect(stripHidden(`a${hidden}b`)).toBe("ab");
    expect(hasHidden(`a${hidden}b`)).toBe(true);
    expect(hasHidden("plain")).toBe(false);
  });
  test("removes the other default-ignorable code points that draw nothing, as clean_text does", () => {
    // Deprecated format characters, musical beam controls, unassigned
    // specials and the unassigned rest of the tag plane: Chrome draws
    // "Pass\u{1D173}word" exactly as wide as "Password".
    for (const c of ["\u{2065}", "\u{206A}", "\u{206F}", "\u{FFF0}", "\u{FFF8}", "\u{1D173}", "\u{1D17A}", "\u{E0080}", "\u{E01F0}", "\u{E0FFF}"]) {
      expect(stripHidden(`Pass${c}word`), c.codePointAt(0)!.toString(16)).toBe("Password");
    }
    // Soft hyphens and the BMP variation selectors shape real text.
    expect(stripHidden("co\u{AD}op \u{2764}\u{FE0F}")).toBe("co\u{AD}op \u{2764}\u{FE0F}");
  });
  test("keeps ordinary text, emoji and accents", () => {
    expect(stripHidden("café 🚀 日本 ñ ❤\u{FE0F}\n\ttab")).toBe("café 🚀 日本 ñ ❤\u{FE0F}\n\ttab");
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
  test("one-line \\[..\\] is math only when it looks like TeX", () => {
    expect(normalizeMath("\\[1\\]")).toBe("\\[1\\]");
    expect(normalizeMath("\\[NOTE\\]")).toBe("\\[NOTE\\]");
    expect(normalizeMath("\\[E = mc^2\\]")).toBe("$$\nE = mc^2\n$$");
    expect(normalizeMath("\\[\\frac{a}{b}\\]")).toBe("$$\n\\frac{a}{b}\n$$");
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
  test("a code span across lines is untouched", () => {
    expect(normalizeMath("`multi\nline \\(x\\) span` and \\(y\\)")).toBe("`multi\nline \\(x\\) span` and $$y$$");
    // A blank line ends the paragraph, so the backtick cannot pair across it.
    expect(normalizeMath("a ` b\n\n\\(y\\) `")).toBe("a ` b\n\n$$y$$ `");
  });
  test("indented code is untouched", () => {
    expect(normalizeMath("    indented \\(x\\) code")).toBe("    indented \\(x\\) code");
    expect(normalizeMath("text\n\n\tcode \\(x\\)\n\n    more \\(x\\)\n\nback \\(y\\)")).toBe(
      "text\n\n\tcode \\(x\\)\n\n    more \\(x\\)\n\nback $$y$$",
    );
    expect(normalizeMath("# Title\n    code \\(x\\)")).toBe("# Title\n    code \\(x\\)");
    expect(normalizeMath("> a\n>\n>     code \\(x\\)")).toBe("> a\n>\n>     code \\(x\\)");
    // Inside a list, code starts four columns past the item's content.
    expect(normalizeMath("- item\n\n      code \\(x\\)")).toBe("- item\n\n      code \\(x\\)");
  });
  test("indented text that is not code still converts", () => {
    // A paragraph's continuation line, even indented.
    expect(normalizeMath("text\n    more \\(x\\)")).toBe("text\n    more $$x$$");
    // A list item's own paragraph after a blank line.
    expect(normalizeMath("- item\n\n  para \\(x\\)")).toBe("- item\n\n  para $$x$$");
    expect(normalizeMath("1. item\n\n    para \\(x\\)")).toBe("1. item\n\n    para $$x$$");
  });
  test("a $ next to the math does not merge with its delimiters", () => {
    expect(normalizeMath("\\(a\\)$5")).toBe("$$a$$ $5");
    expect(normalizeMath("$\\(a\\)")).toBe("$ $$a$$");
    expect(normalizeMath("\\($x\\)")).toBe("$$ $x $$");
  });
  test("a literal $$ in the paragraph cannot pair with the inserted delimiters", () => {
    expect(normalizeMath("costs $$ and \\(x\\)")).toBe("costs $$ and $$$x$$$");
    expect(normalizeMath("$$a$$ and \\(b\\)")).toBe("$$a$$ and $$$b$$$");
    // Another paragraph does not matter.
    expect(normalizeMath("costs $$\n\nand \\(x\\)")).toBe("costs $$\n\nand $$x$$");
  });
  test("single dollars are never math", () => {
    expect(normalizeMath("$5 and $10")).toBe("$5 and $10");
    expect(prepare("echo $HOME/$PATH costs $5")).toBe("echo $HOME/$PATH costs $5");
  });
});

describe("prepare", () => {
  test("strips first, then normalises", () => {
    expect(prepare("\\(\u{202E}x\\)")).toBe("$$x$$");
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
