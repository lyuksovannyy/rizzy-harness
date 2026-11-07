// The web status rows: harness wording in the card head, as static markup.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import type { WebGroup } from "../lib/rich";
import type { Item, ToolResult } from "../types";
import { ToolCard, WebGroupCard } from "./ToolCard";

type Tool = Extract<Item, { type: "tool" }>;

const web = (tool: string, input: object, result: ToolResult | null = null): Tool => ({
  type: "tool",
  call_id: `c-${tool}-${JSON.stringify(input)}`,
  author: null,
  tool,
  input,
  live_output: "",
  result,
  approval: null,
});

const head = (item: Tool) => {
  const html = renderToStaticMarkup(<ToolCard sessionId="s" item={item} />);
  return /<span class="web-status[^"]*">([^<]*)<\/span>/.exec(html)?.[1] ?? html;
};

const found = {
  ok: true,
  output: "[S1] A\n    https://a.example/\n[S2] B\n    https://b.example/",
  exit_code: null,
  sources: [
    { id: "S1", url: "https://a.example/", title: "A" },
    { id: "S2", url: "https://b.example/", title: "B" },
  ],
};

describe("web status rows", () => {
  test("search: running, done with a count, failed", () => {
    expect(head(web("web_search", { query: "rust" }))).toBe("Searching the web for “rust”");
    expect(head(web("web_search", { query: "rust" }, found))).toBe("Searched “rust” · 2 results");
    expect(head(web("web_search", { query: "x" }, { ok: true, output: "No results.", exit_code: null }))).toBe(
      "Searched “x” · 0 results",
    );
    expect(head(web("web_search", { query: "x" }, { ok: false, output: "search failed", exit_code: null }))).toBe(
      "Could not search the web for “x”",
    );
  });

  test("only the running row shimmers", () => {
    expect(renderToStaticMarkup(<ToolCard sessionId="s" item={web("web_search", { query: "q" })} />)).toContain("shimmer");
    expect(renderToStaticMarkup(<ToolCard sessionId="s" item={web("web_search", { query: "q" }, found)} />)).not.toContain(
      "shimmer",
    );
  });

  test("the model's query is one clean line", () => {
    expect(head(web("web_search", { query: "a‮b\n\n c" }))).toBe("Searching the web for “ab c”");
    expect(head(web("web_search", { query: "x".repeat(300) }))).toHaveLength("Searching the web for “”".length + 100);
  });

  test("fetch: the host asked for, then the page it ended on", () => {
    expect(head(web("web_fetch", { url: "https://www.docs.example/a?b=1" }))).toBe("Reading docs.example…");
    const read = { ok: true, output: "[S3] Page", exit_code: null, sources: [{ id: "S3", url: "https://final.example/p", title: "P" }] };
    expect(head(web("web_fetch", { url: "https://docs.example/a" }, read))).toBe("Read final.example");
    expect(head(web("web_fetch", { url: "javascript:alert(1)" }))).toBe("Reading a page…");
    expect(head(web("web_fetch", { url: "https://x.example/" }, { ok: false, output: "nope", exit_code: null }))).toBe(
      "Could not read x.example",
    );
  });

  test("a finished search opens to host · title rows", () => {
    // Collapsed by default; the rows are what opening it shows.
    const html = renderToStaticMarkup(<ToolCard sessionId="s" item={web("web_search", { query: "q" }, found)} />);
    expect(html).not.toContain("web-results");
    expect(html).toContain('aria-expanded="false"');
  });

  test("a run of web calls is one Researched row", () => {
    const group: WebGroup = { start: 0, end: 2, members: [0, 1, 2], searches: 1, pages: 2 };
    const done = [
      web("web_search", { query: "q" }, found),
      web("web_fetch", { url: "https://a.example/" }, found),
      web("web_fetch", { url: "https://b.example/" }, found),
    ];
    const html = renderToStaticMarkup(<WebGroupCard sessionId="s" group={group} tools={done} />);
    expect(html).toContain("Researched · 1 search, 2 pages");
    expect(html).not.toContain("tool-group-body");
    const busy = renderToStaticMarkup(<WebGroupCard sessionId="s" group={group} tools={[...done.slice(0, 2), web("web_fetch", { url: "https://b.example/" })]} />);
    expect(busy).toContain("Researching · 1 search, 2 pages");
    expect(busy).toContain("tool-group-body");
  });
});
