// @vitest-environment happy-dom
// A Select's list stays inside the window wherever its button is (the trial,
// 2026-11-19: the approval-mode menu on the project page opened upward from
// a bar near the top and lost "Plan" and "Ask every call" above the window).
// It takes the same placement as the other menus (md/anchored.ts): under the
// button, above it only when there is no room below.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
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
  // As a browser: showPopover() on an element without a popover attribute throws.
  Object.defineProperty(proto, "showPopover", {
    configurable: true,
    value(this: HTMLElement) {
      if (!this.hasAttribute("popover")) throw new DOMException("not a popover", "NotSupportedError");
    },
  });
  Object.defineProperty(window, "innerHeight", { configurable: true, value: 900 });
  Object.defineProperty(window, "innerWidth", { configurable: true, value: 1440 });
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete proto.showPopover;
  for (const [k, d] of [["offsetHeight", saved.height], ["offsetWidth", saved.width]] as const) {
    if (d) Object.defineProperty(proto, k, d);
    else delete proto[k];
  }
});

const picked = vi.fn();
const render = () =>
  act(async () =>
    root.render(
      <Select
        label="Approval mode"
        value="auto"
        stacked
        options={["plan", "ask", "auto"].map((v) => ({ value: v, label: v }))}
        onChange={picked}
      />,
    ),
  );
const button = () => host.querySelector<HTMLButtonElement>(".select-button")!;
const list = () => host.querySelector(".select-list");
const key = (k: string) => act(async () => void button().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true })));

/** Open the list with the button's box at `top`. */
async function openAt(top: number) {
  picked.mockReset();
  await render();
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

describe("the list's own behaviour", () => {
  beforeEach(() => picked.mockReset());

  test("the list is a real popover (a browser throws on showPopover otherwise)", async () => {
    const l = await openAt(240);
    expect(l.getAttribute("popover")).toBe("manual");
  });

  test("Esc closes it and focus stays on the button", async () => {
    await render();
    button().focus();
    await key("ArrowDown");
    expect(list()).not.toBeNull();
    await key("Escape");
    expect(list()).toBeNull();
    expect(document.activeElement).toBe(button());
  });

  test("down opens, up and down move, Enter picks and closes", async () => {
    await render();
    button().focus();
    await key("ArrowDown");
    expect(list()).not.toBeNull();
    // Opens on the chosen one (auto); up twice: plan.
    await key("ArrowUp");
    await key("ArrowUp");
    expect(host.querySelector(".select-list li.on")?.textContent).toContain("plan");
    await key("ArrowDown");
    expect(host.querySelector(".select-list li.on")?.textContent).toContain("ask");
    await key("Enter");
    expect(picked).toHaveBeenCalledWith("ask");
    expect(list()).toBeNull();
  });

  test("a click outside closes it", async () => {
    await render();
    await key("ArrowDown");
    expect(list()).not.toBeNull();
    await act(async () => void document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true })));
    expect(list()).toBeNull();
    expect(picked).not.toHaveBeenCalled();
  });
});
