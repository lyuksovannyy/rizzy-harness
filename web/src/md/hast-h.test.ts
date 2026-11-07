// The worker's small `h` builds the same tree as hastscript's (which
// refractor uses outside the worker) for everything refractor gives it, so
// highlighting reads the same in the worker.
import { refractor } from "refractor/core";
import javascript from "refractor/javascript";
import markup from "refractor/markup";
import { describe, expect, test } from "vitest";
import type { HastNode } from "./hast";
import { h } from "./hast-h";

refractor.register(javascript);
refractor.register(markup);

/** Rebuilds an element tree with the worker's `h`, from its tag, classes and attributes. */
function rebuild(n: HastNode): HastNode {
  if (n.type !== "element") return n;
  const { className, ...attrs } = n.properties ?? {};
  return h([n.tagName, ...((className as string[] | undefined) ?? [])].join("."), attrs as Record<string, string>, (n.children ?? []).map(rebuild));
}

describe("the worker's h", () => {
  test("wraps a token as hastscript does", () => {
    const text = { type: "text", value: "x" };
    expect(h("span.token.string.attr-value", {}, text)).toEqual({
      type: "element",
      tagName: "span",
      properties: { className: ["token", "string", "attr-value"] },
      children: [text],
    });
    expect(h("span.token.entity", { title: "&" }, [text, text])).toEqual({
      type: "element",
      tagName: "span",
      properties: { title: "&", className: ["token", "entity"] },
      children: [text, text],
    });
  });

  test("a highlighted block comes out the same as through hastscript", () => {
    for (const [code, lang] of [
      ["const a = `x${1}` // hi\nfunction f(b) { return b > 2 && /re/g.test(b); }", "javascript"],
      ['<a href="x">&amp; <b>t</b></a>', "markup"],
    ] as const) {
      const tree = refractor.highlight(code, lang) as unknown as HastNode;
      expect(tree.children!.some((n) => n.type === "element")).toBe(true);
      for (const n of tree.children ?? []) expect(rebuild(n)).toEqual(n);
    }
  });
});
