// The whole rendering path (prepare, remark plugins, components) as static
// markup. Effects do not run here, so this is what a streaming message shows.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { Markdown } from "../components/Markdown";
import { MessageCtx, NO_CITES, NONE } from "./context";
import { hasHidden } from "./hidden";
import type { Provenance } from "./links";

const md = (text: string, sources: Provenance = NONE) =>
  renderToStaticMarkup(
    <MessageCtx.Provider value={{ provenance: NONE, sources, cites: NO_CITES }}>
      <Markdown text={text} done={false} />
    </MessageCtx.Provider>,
  );

describe("callouts", () => {
  test("a [!WARNING] blockquote becomes a titled aside", () => {
    const out = md("> [!WARNING] Careful now\n> This deletes things.");
    expect(out).toContain('<aside class="callout callout-warning" role="note">');
    expect(out).toContain('<span class="callout-label">Warning</span><span class="callout-title">Careful now</span>');
    expect(out).toContain("<p>This deletes things.</p>");
    expect(out).not.toContain("[!WARNING]");
  });
  test("every tone, case-insensitive, without a title", () => {
    for (const tone of ["NOTE", "tip", "Important", "WARNING", "caution"]) {
      const out = md(`> [!${tone}]\n> Body.`);
      expect(out).toContain(`callout-${tone.toLowerCase()}`);
      expect(out).not.toContain("callout-title");
    }
  });
  test("an unknown marker or a plain quote stays a blockquote", () => {
    expect(md("> [!DANGER] x")).toContain("<blockquote>");
    expect(md("> just a quote")).toContain("<blockquote>");
  });
  test("a title ended by a hard break is still the title", () => {
    for (const src of ["> [!NOTE] Title  \n> body", "> [!NOTE] Title\\\n> body"]) {
      const out = md(src);
      expect(out).toContain('<span class="callout-title">Title</span>');
      expect(out).toContain("<p>body</p>");
      expect(out).not.toContain("<br/>");
    }
  });
  test("a title with formatting is kept as body text", () => {
    const out = md("> [!TIP] Use **bold** here");
    expect(out).toContain("callout-tip");
    expect(out).toContain("<strong>bold</strong>");
    expect(out).not.toContain("callout-title");
  });
});

describe("safety", () => {
  test("raw HTML is escaped", () => {
    const out = md('<script>alert(1)</script> <img src=x onerror="alert(1)">');
    expect(out).not.toMatch(/<script|<img/);
    expect(out).toContain("&lt;script&gt;");
  });
  test("images never become <img>: a chip with the host, or a note for data:", () => {
    const out = md("![chart](https://img.example/p.png?q=s) ![d](data:image/png;base64,AAAA) ![j](javascript:alert(1))");
    expect(out).not.toContain("<img");
    expect(out).toContain('class="img-chip"');
    expect(out).toContain('Image: chart · <bdi dir="ltr" class="img-host">img.example</bdi>');
    expect(out).toContain("embedded image (not shown)");
    expect(out).not.toContain("AAAA");
    expect(out).not.toContain("javascript");
  });
  test("disallowed link schemes render as text", () => {
    const out = md("[click](javascript:alert(1)) [rel](/api/me)");
    expect(out).not.toContain("href");
    expect(out).toContain('<span class="link-off">click</span>');
  });
  test("links open safely and show their real host", () => {
    const out = md("[paypal.com](https://paypal.com.evil.example/login) and <https://docs.example/a>");
    expect(out).toContain('rel="noopener noreferrer nofollow"');
    expect(out).toContain('target="_blank"');
    expect(out).toContain('<bdi dir="ltr" class="link-host">paypal.com.evil.example</bdi>');
    // An autolink already shows its address.
    expect(out.match(/link-host/g)).toHaveLength(1);
  });
  test("a link to a verified source is marked as one", () => {
    const sources: Provenance = { has: (k) => k === "docs.example/a" };
    expect(md("[docs](https://docs.example/a)", sources)).toContain('class="link-source"');
  });
  test("hidden characters are gone", () => {
    expect(md("abc\u{202E}def\u{200B}ghi")).toContain("abcdefghi");
  });
  test("hidden characters written as character references are gone too", () => {
    const refs = "&#x202E;&#8238;&rlm;&zwj;&ZeroWidthSpace;&#xFEFF;&#xE000;&#x2066;&#x061C;&#xE0041;&#xE0100;";
    const cases: [string, string][] = [
      [`abc${refs}def`, "<p>abcdef</p>"],
      [`[${refs}moc.knab](https://evil.example/)`, '>moc.knab</a><bdi dir="ltr" class="link-host">evil.example</bdi>'],
      [`![${refs}elpmaxe.doog](https://evil.example/x.png)`, 'Image: elpmaxe.doog · <bdi dir="ltr" class="img-host">evil.example</bdi>'],
      [`> [!WARNING] ${refs}eltit\n> body`, '<span class="callout-title">eltit</span>'],
      [`| a${refs}b | c |\n|---|---|\n| d${refs}e | f |`, "<td>de</td>"],
      [`Note[^1].\n\n[^1]: ${refs}x`, "x"],
    ];
    for (const [src, want] of cases) {
      const out = md(src);
      expect(hasHidden(out), src).toBe(false);
      expect(out).toContain(want);
    }
    // The table header (and so Copy CSV and the sort title) is clean as well.
    expect(md(`| a${refs}b | c |\n|---|---|\n| d | f |`)).toContain('title="Sort by ab">ab</button>');
  });
  test("a mail address with hidden characters shows percent-encoded", () => {
    const out = md("[support](mailto:%E2%80%AEelpmaxe.live@troppus)");
    expect(hasHidden(out)).toBe(false);
    expect(out).toContain('<bdi dir="ltr" class="link-host">%E2%80%AEelpmaxe.live@troppus</bdi>');
  });
  test("footnotes stay in-page links", () => {
    const out = md("Note[^1].\n\n[^1]: The note.");
    expect(out).toContain('href="#user-content-fn-1"');
    expect(out).not.toMatch(/href="#user-content-fn-1"[^>]*target/);
  });
});

describe("code", () => {
  test("a fenced block has a header with the language and Copy", () => {
    const out = md("```rust\nfn main() {}\n```");
    expect(out).toContain('<span class="lang">rust</span>');
    expect(out).toContain('<code class="language-rust">fn main() {}</code>');
    // Not highlighted while streaming.
    expect(out).not.toContain("token");
  });
  test("more than 60 lines collapse", () => {
    const long = Array.from({ length: 61 }, (_, i) => `line ${i}`).join("\n");
    const out = md("```\n" + long + "\n```");
    expect(out).toContain('class="codeblock collapsed"');
    expect(out).toContain("Show all 61 lines");
    expect(md("```\n" + long.split("\n").slice(0, 60).join("\n") + "\n```")).not.toContain("collapsed");
  });
  test("mermaid is labelled source, not a diagram", () => {
    expect(md("```mermaid\ngraph TD; A-->B\n```")).toContain('<span class="lang">diagram source</span>');
  });
});

describe("math", () => {
  test("\\( \\) and $$ blocks reach the math component as source while streaming", () => {
    const out = md("Pythagoras: \\(a^2+b^2=c^2\\)\n\n$$\n\\int_0^1 x\\,dx\n$$");
    expect(out).toContain('<span class="math math-inline"><code class="math-src">a^2+b^2=c^2</code>');
    expect(out).toContain('<div class="math math-display"><pre class="code math-src"><code>\\int_0^1 x\\,dx</code></pre>');
  });
  test("converted math survives a $ beside it and a literal $$ in the paragraph", () => {
    let out = md("\\(a\\)$5");
    expect(out).toContain('<code class="math-src">a</code>');
    expect(out).toContain("</span> $5</p>");
    out = md("costs $$ and \\(x\\)");
    expect(out).toContain("<p>costs $$ and <span");
    expect(out).toContain('<code class="math-src">x</code>');
    // A converted span alone at the start of a line stays inline.
    expect(md("\\(x\\) is small")).toContain('<p><span class="math math-inline">');
  });
  test("indented code and code spans across lines keep \\( \\) as written", () => {
    expect(md("    indented \\(x\\) code")).toContain("<code>indented \\(x\\) code</code>");
    expect(md("`multi\nline \\(x\\) span`")).toContain("<code>multi line \\(x\\) span</code>");
    expect(md("\\[1\\]")).toBe('<div class="md"><p>[1]</p></div>');
  });
  test("single dollars stay text", () => {
    const out = md("Prices: $5 and $10, and $HOME/$PATH.");
    expect(out).toContain("Prices: $5 and $10, and $HOME/$PATH.");
    expect(out).not.toContain("math");
  });
});

describe("tables", () => {
  test("wrapped, with sortable headers and Copy CSV", () => {
    const out = md("| crate | time |\n|---|---:|\n| core | 12.5 |\n| web | 8 |");
    expect(out).toContain('<div class="table-wrap"><div class="table-scroll"><table>');
    expect(out).toContain('<th aria-sort="none"><button type="button" class="th-sort" title="Sort by crate">crate</button></th>');
    expect(out).toContain("Copy CSV");
  });
  test("a header cell with a link keeps the link outside its sort button", () => {
    const out = md("| [crate](https://crates.io/) | ![logo](https://img.example/x.png) | time |\n|---|---|---|\n| core | x | 1 |");
    const head = out.slice(out.indexOf("<thead>"), out.indexOf("</thead>"));
    // No link or image chip nests inside a button.
    expect(head).not.toMatch(/<button[^>]*>(?:(?!<\/button>).)*<a /);
    expect(head).toContain('<span class="th-label"><span class="link-wrap"><a href="https://crates.io/"');
    expect(head).toContain('<button type="button" class="th-sort-icon" title="Sort by crate" aria-label="Sort by crate">');
    // A cell with no text is named by its place.
    expect(head).toContain('class="img-chip"');
    expect(head).toContain('aria-label="Sort by column 2"');
    expect(head).toContain('<button type="button" class="th-sort" title="Sort by time">time</button>');
  });
});
