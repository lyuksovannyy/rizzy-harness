// @vitest-environment happy-dom
// The real render path: Temml builds DOM nodes, then the clean pass runs.
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import temml from "temml";
import { afterEach, describe, expect, test, vi } from "vitest";
import { DoneCtx } from "./context";
import { hasHidden } from "./hidden";
import { keepAttr, OPTIONS, renderMath } from "./math";
import { MathTeX, MAX_TEX } from "./MathTeX";

// Two gaps in happy-dom that browsers do not have; this runs before the
// imports. It has no compatMode, and Temml refuses to render in quirks
// mode (the app's page has a doctype). Its MathML elements have no style
// object, and Temml sets inline styles; they get the HTML one.
vi.hoisted(() => {
  Object.defineProperty(document, "compatMode", { value: "CSS1Compat", configurable: true });
  if (!("style" in Element.prototype)) {
    Object.defineProperty(Element.prototype, "style", Object.getOwnPropertyDescriptor(HTMLElement.prototype, "style")!);
  }
});

const render = (tex: string, display = false) => {
  const el = document.createElement("span");
  renderMath(tex, el, display);
  return el;
};

describe("renderMath", () => {
  test("renders MathML", () => {
    const el = render("a^2+b^2=c^2");
    expect(el.querySelector("math")).not.toBeNull();
    expect(el.querySelector("msup")).not.toBeNull();
  });

  test("links Temml makes for \\ref and \\eqref lose their href", () => {
    for (const tex of ["\\ref{root}", "\\eqref{x}"]) {
      // Temml does emit one, even with trust off.
      expect(temml.renderToString(tex, OPTIONS)).toContain("href=");
      expect(render(tex, true).querySelectorAll("[href]")).toHaveLength(0);
    }
  });

  test("ids from \\label are removed, so math cannot take over a footnote's target", () => {
    const tex = "\\tag{1}\\label{user-content-fn-1} x";
    expect(temml.renderToString(tex, { ...OPTIONS, displayMode: true })).toContain('id="user-content-fn-1"');
    const el = render(tex, true);
    expect(el.querySelectorAll("[id]")).toHaveLength(0);
    expect(el.textContent).toContain("(1)");
  });

  test("hidden characters that TeX builds are stripped", () => {
    for (const tex of ['\\char"202E x', '\\char"200B y', '\\char"E000 z']) {
      const el = render(tex);
      expect(el.querySelector("math")).not.toBeNull();
      expect(hasHidden(el.textContent ?? "")).toBe(false);
    }
  });

  test("only layout attributes stay", () => {
    for (const a of ["class", "style", "mathvariant", "display", "encoding", "stretchy", "XMLNS"]) expect(keepAttr(a)).toBe(true);
    for (const a of ["href", "xlink:href", "src", "onclick", "id", "name", "data-x"]) expect(keepAttr(a)).toBe(false);
    const el = render("\\frac{a}{b} \\begin{matrix} 1 & 2 \\end{matrix} \\left( x \\right)", true);
    for (const node of el.querySelectorAll("*")) for (const a of node.attributes) expect(keepAttr(a.name), a.name).toBe(true);
  });

  test("trusted commands, bad colours and TeX errors throw (MathTeX shows the source)", () => {
    for (const tex of [
      "\\href{https://evil.example}{x}",
      "\\url{https://evil.example}",
      "\\class{x}{y}",
      "\\id{x}{y}",
      "\\style{color:red}{y}",
      "\\includegraphics{https://evil.example/x.png}",
      "\\color{red;background:url(https://evil.example/x)}{x}",
      "\\frac{",
    ]) {
      expect(() => render(tex), tex).toThrow();
    }
  });

  test("expansion bombs stop with an error instead of hanging", () => {
    const t0 = Date.now();
    expect(() => render("\\def\\a{\\a\\a}\\a")).toThrow(/expansion/i);
    expect(Date.now() - t0).toBeLessThan(2000);
  });
});

describe("MathTeX", () => {
  let host: HTMLElement | null = null;
  afterEach(() => host?.remove());

  const mount = async (tex: string) => {
    host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => {
      root.render(createElement(DoneCtx.Provider, { value: true }, createElement(MathTeX, { tex, display: false })));
    });
    return host;
  };
  const shown = (el: HTMLElement) => !el.querySelector<HTMLElement>(".math-out")!.hidden;

  test("renders once the message is final", async () => {
    const el = await mount("x^2");
    await vi.waitFor(() => expect(shown(el)).toBe(true));
    expect(el.querySelector(".math-out math")).not.toBeNull();
    expect(el.querySelector(".math-src")).toBeNull();
  });

  test("TeX over the limit stays source and is never rendered", async () => {
    const el = await mount("x".repeat(MAX_TEX + 1));
    await new Promise((r) => setTimeout(r, 50));
    expect(shown(el)).toBe(false);
    expect(el.querySelector(".math-src")?.textContent).toHaveLength(MAX_TEX + 1);
    expect(el.querySelector(".math-out")!.childNodes).toHaveLength(0);
  });

  test("a TeX error falls back to the source", async () => {
    const el = await mount("\\frac{");
    await new Promise((r) => setTimeout(r, 50));
    expect(shown(el)).toBe(false);
    expect(el.querySelector(".math-src")?.textContent).toBe("\\frac{");
    expect(el.textContent).not.toContain("ParseError");
  });
});
