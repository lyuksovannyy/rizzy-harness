// @vitest-environment happy-dom
// The background host: one layer per mode, the old one leaving while the
// new one enters, at most two at a time, nothing when turned off, and a
// pause while the tab is hidden. The artwork itself is stubbed.
import { act, useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Mode } from "../lib/modes";
import type { BackdropPref } from "../types";
import Backdrop from "./Backdrop";
import type { BgProps } from "./types";

// Each stub finishes its entrance at once unless the test holds it back.
let quick = true;
function stub(mode: Mode) {
  return {
    default: function Art(p: BgProps) {
      useEffect(() => {
        if (quick) p.onEntered();
      }, []);
      return <i data-art={mode} data-delay={p.delay} />;
    },
  };
}
vi.mock("./SearchBg", () => stub("search"));
vi.mock("./WallBg", () => stub("ask"));
vi.mock("./BinaryBg", () => stub("agentic"));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
let reduce = false;

beforeEach(() => {
  vi.useFakeTimers();
  reduce = false;
  quick = true;
  vi.stubGlobal("matchMedia", (q: string) => ({
    matches: q.includes("reduce") && reduce,
    addEventListener() {},
    removeEventListener() {},
  }));
  host = document.createElement("section");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

async function show(mode: Mode, seed = 1, pref: BackdropPref = "animated") {
  await act(async () => {
    root.render(
      <>
        <Backdrop mode={mode} seed={seed} pref={pref} typing={false} titles={[]} />
        <div className="column" />
      </>,
    );
  });
  await act(async () => {});
}
const layers = () => [...host.querySelectorAll<HTMLElement>(".backdrop-layer")].map((l) => `${l.dataset.mode}:${l.dataset.state}`);
const wait = (ms: number) => act(async () => void vi.advanceTimersByTime(ms));

describe("Backdrop", () => {
  test("hidden from assistive tech, one layer for the mode", async () => {
    await show("ask");
    const b = host.querySelector<HTMLElement>(".backdrop")!;
    expect(b.getAttribute("aria-hidden")).toBe("true");
    expect(b.dataset.mode).toBe("ask");
    expect(b.dataset.motion).toBe("animated");
    expect(layers()).toEqual(["ask:idle"]);
    expect(host.querySelector("[data-art=ask]")).not.toBeNull();
  });

  test("a mode change: the old layer leaves, the new one enters 140 ms later", async () => {
    await show("ask");
    await show("search");
    expect(layers()).toEqual(["ask:exit", "search:idle"]);
    expect(host.querySelector<HTMLElement>("[data-art=search]")!.dataset.delay).toBe("140");
    await wait(399);
    expect(layers()).toHaveLength(2);
    await wait(2);
    expect(layers()).toEqual(["search:idle"]);
  });

  test("a layer cut short mid-entrance just fades", async () => {
    quick = false;
    await show("ask");
    await show("search");
    expect(host.querySelector(".backdrop-layer.cut")).not.toBeNull();
    await wait(161);
    expect(layers()).toEqual(["search:enter"]);
  });

  test("never more than two layers", async () => {
    await show("ask");
    await show("search");
    await show("agentic");
    expect(layers()).toEqual(["search:exit", "agentic:idle"]);
  });

  test("a new seed re-rolls the same mode", async () => {
    await show("ask", 1);
    await show("ask", 2);
    expect(layers()).toEqual(["ask:exit", "ask:idle"]);
  });

  test("still: changes cross-fade at once", async () => {
    reduce = true;
    await show("ask");
    expect(host.querySelector<HTMLElement>(".backdrop")!.dataset.motion).toBe("still");
    await show("agentic");
    expect(host.querySelector<HTMLElement>("[data-art=agentic]")!.dataset.delay).toBe("0");
    await wait(161);
    expect(layers()).toEqual(["agentic:idle"]);
  });

  test("off: nothing at all", async () => {
    await show("ask", 1, "off");
    expect(host.querySelector(".backdrop")).toBeNull();
  });

  test("paused while the tab is hidden", async () => {
    await show("ask");
    let state = "hidden";
    vi.spyOn(document, "visibilityState", "get").mockImplementation(() => state as DocumentVisibilityState);
    await act(async () => void document.dispatchEvent(new Event("visibilitychange")));
    expect(host.querySelector(".backdrop")!.hasAttribute("data-paused")).toBe(true);
    state = "visible";
    await act(async () => void document.dispatchEvent(new Event("visibilitychange")));
    expect(host.querySelector(".backdrop")!.hasAttribute("data-paused")).toBe(false);
  });
});
