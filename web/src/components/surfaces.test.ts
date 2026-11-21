// A pane that covers text is opaque: the glass tokens (--bg-2, --glass-*) are
// translucent, so the chat would show through it.
import { expect, test } from "vitest";
import { readFileSync } from "node:fs";

const read = (name: string) => readFileSync(new URL(`./${name}`, import.meta.url), "utf8");
const planReview = read("planReview.css");
const sidePanel = read("sidePanel.css");

const rule = (css: string, selector: string, within = "") => {
  const from = within ? css.indexOf(within) : 0;
  const at = css.indexOf(`${selector} {`, from);
  return at < 0 ? "" : css.slice(at, css.indexOf("}", at));
};

test("the plan-review pane has an opaque surface", () => {
  expect(rule(planReview, ".plan-review")).toMatch(/background:\s*var\(--surface\)/);
});

test("the side panel's drawer on a narrow screen has an opaque surface", () => {
  expect(rule(sidePanel, ".side-panel", "@media (max-width: 900px)")).toMatch(/background:\s*var\(--surface\)/);
});
