// @vitest-environment happy-dom
// The appearance radiogroups by keyboard: one Tab stop each (the checked
// radio), and the arrow keys move along the group and pick, wrapping around.
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import type { Prefs } from "../types";
import { AppearanceFields } from "./Appearance";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;

function Harness() {
  const [p, setP] = useState<Prefs>({} as Prefs);
  return <AppearanceFields prefs={p} saved={{} as Prefs} onChange={setP} />;
}

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<Harness />));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const radios = (group: string) => [...host.querySelectorAll<HTMLButtonElement>(`[role=radiogroup][aria-label="${group}"] [role=radio]`)];
const checked = (group: string) => radios(group).map((r) => r.getAttribute("aria-checked") === "true");
const press = (key: string) =>
  act(() => void (document.activeElement as HTMLElement).dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true })));

describe("appearance radiogroups", () => {
  test.each(["theme", "accent", "backgrounds"])("%s: one Tab stop, the checked radio", (group) => {
    const tabs = radios(group).map((r) => r.tabIndex);
    expect(tabs.filter((t) => t === 0)).toHaveLength(1);
    expect(tabs[checked(group).indexOf(true)]).toBe(0);
  });

  test("arrows move focus and the choice, and wrap", () => {
    const theme = radios("theme");
    expect(checked("theme")).toEqual([true, false, false]);
    act(() => theme[0]!.focus());
    press("ArrowRight");
    expect(checked("theme")).toEqual([false, true, false]);
    expect(document.activeElement).toBe(radios("theme")[1]);
    expect(radios("theme").map((r) => r.tabIndex)).toEqual([-1, 0, -1]);
    press("ArrowDown");
    press("ArrowRight");
    expect(checked("theme")).toEqual([true, false, false]);
    press("ArrowLeft");
    expect(checked("theme")).toEqual([false, false, true]);
    expect(document.activeElement).toBe(radios("theme")[2]);
  });

  test("other keys are left alone", () => {
    act(() => radios("backgrounds")[0]!.focus());
    press("Enter");
    press("a");
    expect(checked("backgrounds")).toEqual([true, false, false]);
  });
});
