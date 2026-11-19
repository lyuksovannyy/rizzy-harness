// @vitest-environment happy-dom
// A Select's list stays inside the window wherever its button is (the trial,
// 2026-11-19: the approval-mode menu on the project page opened upward from
// a bar near the top and lost "Plan" and "Ask every call" above the window).
// It takes the same placement as the other menus (md/anchored.ts): under the
// button, above it only when there is no room below.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import { Select } from "./Select";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
const proto = HTMLElement.prototype as unknown as Record<string, unknown>;
const saved = { height: Object.getOwnPropertyDescriptor(proto, "offsetHeight"), width: Object.getOwnPropertyDescriptor(proto, "offsetWidth") };
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  Object.defineProperty(proto, "offsetHeight", { configurable: true, get: () => 300 });
  Object.defineProperty(proto, "offsetWidth", { configurable: true, get: () => 300 });
  Object.defineProperty(window, "innerHeight", { configurable: true, value: 900 });
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440 });
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  for (const [k, d] of [["offsetHeight", saved.height], ["offsetWidth", saved.width]] as const) {
    if (d) Object.defineProperty(proto, k, d);
    else delete proto[k];
  }
});

/** Open the list with the button's box at `top`. */
async function openAt(top: number) {
  await act(async () =>
    root.render(
      <Select
        label="Approval mode"
        value="auto"
        stacked
        options={["plan", "ask", "auto"].map((v) => ({ value: v, label: v }))}
        onChange={() => {}}
      />,
    ),
  );
  const rect = { top, bottom: top + 34, left: 900, right: 1000, width: 100, height: 34, x: 900, y: top, toJSON: () => ({}) };
  host.querySelector<HTMLElement>(".select")!.getBoundingClientRect = () => rect as DOMRect;
  await act(async () => host.querySelector<HTMLButtonElement>(".select-button")!.click());
  return host.querySelector<HTMLElement>(".select-list")!;
}

describe("where a Select's list opens", () => {
  test("under a button near the top of the window, whole in view", async () => {
    const list = await openAt(240);
    const top = parseFloat(list.style.top);
    expect(top).toBe(240 + 34 + 4);
    expect(top).toBeGreaterThanOrEqual(0);
    expect(top + 300).toBeLessThanOrEqual(900);
  });

  test("above a button at the bottom (the chat's composer), as before", async () => {
    const list = await openAt(840);
    expect(parseFloat(list.style.top)).toBe(840 - 4 - 300);
  });
});
