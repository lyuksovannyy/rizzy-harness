import temml from "temml";
import { describe, expect, test } from "vitest";
import { OPTIONS, unsafeAttr } from "./math";

const render = (tex: string) => temml.renderToString(tex, { ...OPTIONS, displayMode: false });

describe("math options", () => {
  test("renders MathML", () => {
    const out = render("a^2+b^2=c^2");
    expect(out).toContain("<math");
    expect(out).toContain("<msup>");
  });

  test("trust is off: no links, classes, ids or styles from TeX", () => {
    for (const tex of ["\\href{https://evil.example}{x}", "\\url{https://evil.example}", "\\class{x}{y}", "\\id{x}{y}", "\\style{color:red}{y}"]) {
      const out = render(tex);
      expect(out).not.toMatch(/href=|class="x"|id="x"|style="color:red"/);
    }
  });

  test("expansion bombs stop with an error instead of hanging", () => {
    const t0 = Date.now();
    const out = render("\\def\\a{\\a\\a}\\a");
    expect(Date.now() - t0).toBeLessThan(2000);
    expect(out).toMatch(/expansion|error/i);
  });

  test("the attribute pass removes links, sources and handlers", () => {
    for (const a of ["href", "HREF", "xlink:href", "src", "onclick", "onload"]) expect(unsafeAttr(a)).toBe(true);
    for (const a of ["class", "style", "mathvariant", "display", "encoding"]) expect(unsafeAttr(a)).toBe(false);
  });
});
