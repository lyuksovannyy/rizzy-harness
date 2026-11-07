import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { renderHast, type HastNode } from "./hast";
import { highlight, LOADERS } from "./highlight";
import { grammarFor, GRAMMARS } from "./langs";

const html = (n: HastNode) => renderToStaticMarkup(<>{renderHast(n)}</>);

describe("renderHast", () => {
  test("keeps spans with their classes and text", () => {
    const tree: HastNode = {
      type: "root",
      children: [
        { type: "element", tagName: "span", properties: { className: ["token", "keyword"] }, children: [{ type: "text", value: "fn" }] },
        { type: "text", value: " <b>&" },
      ],
    };
    expect(html(tree)).toBe('<span class="token keyword">fn</span> &lt;b&gt;&amp;');
  });

  test("drops other elements with their content, and every attribute but the class", () => {
    const tree: HastNode = {
      type: "root",
      children: [
        { type: "element", tagName: "script", properties: {}, children: [{ type: "text", value: "alert(1)" }] },
        { type: "element", tagName: "img", properties: { src: "https://evil.example/x.png", onError: "alert(1)" }, children: [] },
        {
          type: "element",
          tagName: "span",
          properties: { className: ["token", "bad class\"x"], style: "color:red", onClick: "alert(1)", id: "x", href: "javascript:x" },
          children: [{ type: "element", tagName: "a", properties: { href: "https://evil.example" }, children: [{ type: "text", value: "link" }] }, { type: "text", value: "ok" }],
        },
        { type: "comment", value: "hidden" },
        { type: "doctype" },
      ],
    };
    expect(html(tree)).toBe('<span class="token">ok</span>');
  });
});

describe("highlight", () => {
  test("every grammar has a loader", () => {
    expect(Object.keys(LOADERS).sort()).toEqual([...GRAMMARS].sort());
  });

  test("aliases map to grammars; unknown languages do not", () => {
    expect(grammarFor("RS")).toBe("rust");
    expect(grammarFor("sh")).toBe("bash");
    expect(grammarFor("c++")).toBe("cpp");
    expect(grammarFor("html")).toBe("markup");
    expect(grammarFor("mermaid")).toBeNull();
    expect(grammarFor("")).toBeNull();
  });

  test("rust code becomes token spans", async () => {
    const out = html((await highlight("fn main() { let x = 42; }", "rust")) as HastNode);
    expect(out).toContain('<span class="token keyword">fn</span>');
    expect(out).toContain('<span class="token number">42</span>');
  });

  test("grammars that build on others register them (tsx needs jsx and typescript)", async () => {
    const out = html((await highlight("const a: number = <div>{1}</div>;", "tsx")) as HastNode);
    expect(out).toContain('class="token keyword"');
    expect(out).toContain('class="token tag"');
  });

  test("markup in code stays text", async () => {
    const out = html((await highlight('<img src=x onerror="alert(1)">', "markup")) as HastNode);
    expect(out).not.toContain("<img");
    expect(out).toContain("&lt;");
  });
});
