// Citation labels through the whole rendering path, as static markup.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { Markdown } from "../components/Markdown";
import { sourceKeys } from "../lib/rich";
import type { Source } from "../types";
import { MessageCtx, NONE } from "./context";
import { labelRuns } from "./remark-cite";

const src = (id: string, url: string, title = `Title ${id}`): Source => ({ id, url, title });
const CITES = new Map<string, Source>([
  ["S1", src("S1", "https://www.a.example/rust")],
  ["S2", src("S2", "https://b.example/book", "The ‮book")],
  ["S3", src("S3", "javascript:alert(1)")],
]);

const md = (text: string, done = true) =>
  renderToStaticMarkup(
    <MessageCtx.Provider value={{ provenance: NONE, sources: NONE, cites: CITES }}>
      <Markdown text={text} done={done} />
    </MessageCtx.Provider>,
  );

const pills = (html: string) => [...html.matchAll(/<a class="cite"[^>]*>(.*?)<\/a>/g)].map((m) => m[1]!.replace(/<[^>]+>/g, ""));

describe("label runs", () => {
  test("adjacent labels, or labels with only spaces between, are one run", () => {
    expect(labelRuns("a [S1][S2] b [S3] [S4], [S5]").map((r) => r.labels)).toEqual([["S1", "S2"], ["S3", "S4"], ["S5"]]);
  });
  test("only S1 to S9999", () => {
    expect(labelRuns("[S0] [S01] [S10000] [s1] [S12]").map((r) => r.labels)).toEqual([["S12"]]);
  });
});

describe("citation pills", () => {
  test("a label becomes a pill with the source's host, linking to the source", () => {
    const out = md("Rust is fast [S1].");
    expect(pills(out)).toEqual(["a.example"]);
    expect(out).toContain('href="https://www.a.example/rust"');
    expect(out).toContain('rel="noopener noreferrer nofollow"');
    expect(out).not.toContain("[S1]");
  });

  test("adjacent labels merge into one pill: host +k", () => {
    expect(pills(md("Both [S1][S2]."))).toEqual(["a.example +1"]);
    expect(pills(md("Both [S1] [S2] and [S1][S1]."))).toEqual(["a.example +1", "a.example"]);
  });

  test("an unknown label, or a source without a web URL, stays muted text", () => {
    const out = md("Unknown [S9] and [S3], mixed [S1][S9].");
    expect(out).toContain('<span class="cite-missing" title="No such source">[S9]</span>');
    expect(out).toContain('<span class="cite-missing" title="No such source">[S3]</span>');
    expect(out).not.toContain("javascript:");
    expect(pills(out)).toEqual(["a.example"]);
  });

  test("no pills inside code, inline code or a link's text", () => {
    const out = md("`[S1]` and [see [S1]](https://c.example/) and\n\n```\n[S1]\n```\n");
    expect(pills(out)).toEqual([]);
    expect(out).toContain("<code>[S1]</code>");
    expect(out).not.toContain("<details");
  });

  test("labels inside emphasis, lists and tables still resolve", () => {
    const out = md("*fast [S1]*\n\n- item [S2]\n\n| a |\n|---|\n| [S1] |\n");
    expect(pills(out)).toEqual(["a.example", "b.example", "a.example"]);
  });

  test("a header cell's label is a pill beside the cell's own sort button, not inside it", () => {
    const out = md("| a [S1] | b |\n|---|---|\n| x [S2] | y |\n");
    const head = out.slice(out.indexOf("<thead>"), out.indexOf("</thead>"));
    expect(pills(head)).toEqual(["a.example"]);
    expect(head).toMatch(/<span class="th-label">a <span class="cite-wrap">.*<\/span><\/span><button type="button" class="th-sort-icon" title="Sort by a \[S1\]" aria-label="Sort by a \[S1\]">/);
    expect(head).not.toMatch(/<button[^>]*>(?:(?!<\/button>).)*<a /);
    expect(head).toContain('<button type="button" class="th-sort" title="Sort by b">b</button>');
    expect(pills(out)).toEqual(["a.example", "b.example"]);
  });

  test("a callout title's label is a pill in the title", () => {
    const out = md("> [!NOTE] Title [S1]\n> body [S2]");
    const head = out.slice(out.indexOf('<div class="callout-head">'), out.indexOf("</div>"));
    expect(head).toContain('<span class="callout-title">Title <span class="cite-wrap">');
    expect(pills(head)).toEqual(["a.example"]);
    expect(pills(out)).toEqual(["a.example", "b.example"]);
    expect(out).not.toContain("[S1]");
  });
});

describe("links to sources", () => {
  const linked = (text: string) =>
    renderToStaticMarkup(
      <MessageCtx.Provider value={{ provenance: NONE, sources: sourceKeys(CITES), cites: CITES }}>
        <Markdown text={text} done />
      </MessageCtx.Provider>,
    );
  test("a link to a source page is marked verified", () => {
    expect(linked("[Official docs](https://a.example/rust)")).toContain('class="link-source"');
  });
  test("not with a fragment the source did not have: the page's scripts can read it", () => {
    const out = linked("[Official docs](https://a.example/rust#c2VjcmV0LXRva2VuLTEyMzQ1Njc4OTA)");
    expect(out).not.toContain("link-source");
  });
});

describe("sources footer", () => {
  test("lists the cited sources once each, in first-cited order, after the footnotes", () => {
    const out = md("B [S2], A [S1][S2], none [S9].[^1]\n\n[^1]: A note.");
    const footer = out.slice(out.indexOf('<details class="sources">'));
    expect(footer).toContain("<summary>Sources (2)</summary>");
    const items = [...footer.matchAll(/<li><a href="([^"]+)"/g)].map((m) => m[1]);
    expect(items).toEqual(["https://b.example/book", "https://www.a.example/rust"]);
    // Hidden characters in titles are dropped.
    expect(footer).toContain(">The book</a>");
    expect(out.indexOf("footnotes")).toBeLessThan(out.indexOf('<details class="sources">'));
  });

  test("not while streaming, and not when nothing resolves", () => {
    expect(md("A [S1].", false)).not.toContain("<details");
    expect(md("A [S9].")).not.toContain("<details");
  });
});
