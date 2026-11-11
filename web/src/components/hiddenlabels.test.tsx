// W-3 (Gate 3 fix review 2): strings a node or the model supplies, shown
// outside the Markdown pipeline (titles, snippets, node names, tool names,
// error text), show hidden characters as codes instead of applying them.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import type { Item } from "../types";
import { Highlight } from "./Highlight";
import { NodeBadge } from "./NodeBadge";
import { ToolCard } from "./ToolCard";

const RLO = "‮";

describe("W-3: hidden characters in labels are shown", () => {
  test("a search snippet or title, with its highlight kept", () => {
    const html = renderToStaticMarkup(<Highlight text={`Budget ${RLO}fdp.exe \u0002tax\u0003 \u{E0041}`} />);
    expect(html).not.toContain(RLO);
    expect(html).toBe("<span>Budget &lt;U+202E&gt;fdp.exe </span><mark>tax</mark><span> &lt;U+E0041&gt;</span>");
  });

  test("a node's hostname", () => {
    const node = { hostname: `build${RLO}01`, connected: true, embedded: false } as never;
    const html = renderToStaticMarkup(<NodeBadge node={node} nodeId="n1" />);
    expect(html).not.toContain(RLO);
    expect(html).toContain(">build&lt;U+202E&gt;01</span>");
    expect(html).toContain("(build&lt;U+202E&gt;01)");
  });

  test("a tool's name, its output and the node's reason on an approval card", () => {
    const item: Extract<Item, { type: "tool" }> = {
      type: "tool",
      call_id: "c",
      author: null,
      tool: `ba${RLO}sh`,
      input: {},
      live_output: `out${RLO}put`,
      result: null,
      approval: { approval_id: "a", reason: `asks ${RLO}x`, turn_allow: true, decision: null, by: null },
    };
    const html = renderToStaticMarkup(<ToolCard sessionId="s" item={item} />);
    expect(html).not.toContain(RLO);
    expect(html).toContain('<span class="tool-name">ba&lt;U+202E&gt;sh</span>');
    expect(html).toContain("asks &lt;U+202E&gt;x");
    expect(html).toContain("out&lt;U+202E&gt;put");
  });
});
