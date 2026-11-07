// @vitest-environment happy-dom
// The profile's time zone field: a reset button shows only while the value
// is a real zone other than this device's, and the input leaves room for
// every button on it.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Prefs } from "../types";
import { ProfileFields } from "./forms";

vi.mock("../lib/greeting", () => ({ browserTimeZone: () => "Europe/Riga" }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
let latest: Prefs;

function render(timezone: string | null) {
  act(() =>
    root.render(
      <ProfileFields prefs={{ timezone } as Prefs} onChange={(p) => (latest = p)} username="alex" />,
    ),
  );
  const input = host.querySelector<HTMLInputElement>('input[role="combobox"]')!;
  return {
    input,
    reset: host.querySelector<HTMLButtonElement>('button[aria-label^="Reset to this device"]'),
    buttons: input.parentElement!.querySelectorAll("button.icon").length,
    room: input.parentElement!.style.getPropertyValue("--combo-actions"),
  };
}

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe("time zone reset", () => {
  test("shows for another valid zone and puts the device's back", () => {
    const f = render("America/New_York");
    expect(f.reset?.getAttribute("aria-label")).toBe("Reset to this device's time zone (Europe/Riga)");
    expect(f.reset?.title).toBe(f.reset?.getAttribute("aria-label"));
    // Reset and clear: the input's padding counts both.
    expect(f.buttons).toBe(2);
    expect(f.room).toBe("2");
    act(() => f.reset!.click());
    expect(latest.timezone).toBe("Europe/Riga");
  });

  test("hidden for the device's zone, an empty field and a zone that does not exist", () => {
    const same = render("Europe/Riga");
    expect(same.reset).toBeNull();
    expect(same.buttons).toBe(1);
    expect(same.room).toBe("1");

    const empty = render(null);
    expect(empty.reset).toBeNull();
    expect(empty.buttons).toBe(0);
    expect(host.textContent).toContain("Use this device's (Europe/Riga)");

    expect(render("Mars/Base").reset).toBeNull();
  });

  test("each caption is a label for its field", () => {
    render(null);
    for (const label of host.querySelectorAll<HTMLLabelElement>("label.field-label")) {
      expect(document.getElementById(label.htmlFor)?.tagName).toBe("INPUT");
    }
    expect(host.querySelectorAll("label.field-label").length).toBe(4);
  });
});
