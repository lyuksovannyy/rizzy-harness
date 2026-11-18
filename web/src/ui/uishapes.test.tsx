// @vitest-environment happy-dom
// A `ui:` block's spec of the wrong shape (review of PR 156, MEDIUM 4): the
// spec is checked in Rust where the node writes it, and the browser renders
// it as sent, so a node that skips that check could take the chat down with
// `{}` as a choices spec. Each card's spec is checked first (shape.ts); a
// wrong shape shows one line instead of the card. Every spec of the shared
// ui fixture still renders its card, and every leaf of each, replaced by a
// bad value, renders without a throw.
import { act, Suspense } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test, vi } from "vitest";
import UiBlock from "./UiBlock";
import { specFits } from "./shape";
import ui from "../__fixtures__/ui.json";
import type { UiBlock as Block } from "../types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** The block rendered (and the chart's chunk awaited); its text, or the throw. */
async function draw(block: Block): Promise<{ text: string; thrown: unknown }> {
  const host = document.createElement("div");
  const root = createRoot(host);
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  let thrown: unknown = null;
  try {
    await act(async () =>
      root.render(
        <Suspense fallback={null}>
          <UiBlock block={block} />
        </Suspense>,
      ),
    );
    await act(async () => new Promise((r) => setTimeout(r, 0)));
  } catch (e) {
    thrown = e;
  }
  const text = host.textContent ?? "";
  act(() => root.unmount());
  errors.mockRestore();
  return { text, thrown };
}
const block = (type: Block["type"], spec: unknown) => ({ ordinal: 0, hash: "h", id: "b", type, spec }) as Block;

const FIXTURE = ui.texts.flatMap((t) => t.blocks as unknown as Block[]).filter((b) => b.spec);

describe("ui: cards given a spec of the wrong shape", () => {
  for (const [name, type, spec] of [
    ["choices, spec {}", "choices", {}],
    ["choices, options not a list", "choices", { question: "q", options: "x" }],
    ["form, spec {}", "form", {}],
    ["progress, spec {}", "progress", {}],
    ["chart, spec {}", "chart", {}],
    ["choices, a spec that is a string", "choices", "x"],
  ] as [string, Block["type"], unknown][]) {
    test(`${name}: one line in its place`, async () => {
      const { text, thrown } = await draw(block(type, spec));
      expect(thrown).toBeNull();
      expect(text).toBe("This item could not be shown.");
    });
  }

  // Rust's id rule allows "constructor": a form's draft and answers are read by own key.
  test("a form whose field id is a prototype name Rust allows renders", async () => {
    const spec = { title: "T", fields: [{ id: "constructor", label: "Name", type: "text" }, { id: "valueof", label: "N", type: "number", required: true }] };
    const { text, thrown } = await draw(block("form", spec));
    expect(thrown).toBeNull();
    expect(text).toContain("Name");
  });

  test("every spec of the shared fixture fits, and renders its card", async () => {
    expect(FIXTURE.length).toBeGreaterThan(10);
    for (const b of FIXTURE) {
      expect(specFits(b), JSON.stringify(b.spec)).toBe(true);
      const { text, thrown } = await draw(b);
      expect(thrown).toBeNull();
      expect(text).not.toContain("could not be shown");
    }
  });

  test("every leaf of every fixture spec, replaced by a bad value, renders without a throw", async () => {
    const BAD: unknown[] = [null, 7, "x", "__proto__", [], {}, true, -1, 1e308];
    const leaves = (x: unknown, at: (string | number)[] = []): (string | number)[][] =>
      Array.isArray(x)
        ? [at, ...x.slice(0, 2).flatMap((y, i) => leaves(y, [...at, i]))]
        : x && typeof x == "object"
          ? [at, ...Object.entries(x).flatMap(([k, y]) => leaves(y, [...at, k]))]
          : [at];
    const seen = new Set<string>();
    let n = 0;
    for (const b of FIXTURE) {
      for (const path of leaves(b.spec)) {
        if (path.length === 0) continue;
        for (const bad of BAD) {
          const spec = structuredClone(b.spec) as unknown as Record<string | number, unknown>;
          let o = spec;
          for (const k of path.slice(0, -1)) o = o[k] as Record<string | number, unknown>;
          o[path.at(-1)!] = bad;
          const key = `${b.type} ${JSON.stringify(spec)}`;
          if (seen.has(key)) continue;
          seen.add(key);
          const { thrown } = await draw(block(b.type, spec));
          expect(thrown, key).toBeNull();
          n++;
        }
      }
    }
    expect(n).toBeGreaterThan(200);
  }, 120_000);
});
