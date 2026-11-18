// @vitest-environment happy-dom
// FINDING (fold-guard work, 2026-11-18), not fixed here: a `ui:` block's spec
// is validated in Rust where the node writes the message (crates/core/src/ui),
// and the browser renders it as sent. A node that skips that (older, buggy,
// hostile) can send a spec of the wrong shape, and the cards read inside it
// unchecked: `{}` as a choices spec throws in Choices (`spec.options.map`).
// Since the screen boundary (ScreenBoundary), that costs the chat's screen
// (a message and Reload), no longer the whole app. Each case below is
// `test.fails` while it throws: when a card checks its spec, vitest reports
// the case as passing unexpectedly, and it becomes a plain `test`.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test, vi } from "vitest";
import UiBlock from "./UiBlock";
import type { UiBlock as Block } from "../types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function renders(type: Block["type"], spec: unknown) {
  const host = document.createElement("div");
  const root = createRoot(host);
  const block = { ordinal: 0, hash: "h", id: "b", type, spec } as Block;
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  let thrown: unknown = null;
  try {
    await act(async () => root.render(<UiBlock block={block} />));
  } catch (e) {
    thrown = e;
  }
  act(() => root.unmount());
  errors.mockRestore();
  expect(thrown).toBeNull();
}

describe("ui: cards given a spec of the wrong shape", () => {
  test.fails("choices, spec {}", () => renders("choices", {}));
  test.fails("choices, options not a list", () => renders("choices", { question: "q", options: "x" }));
  test.fails("form, spec {}", () => renders("form", {}));
  test.fails("progress, spec {}", () => renders("progress", {}));
});
