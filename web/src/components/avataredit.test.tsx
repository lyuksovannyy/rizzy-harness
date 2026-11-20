// @vitest-environment happy-dom
// The avatar picker: a grid of every shape drawn in the current colour and
// eyes, a row of round colour swatches, the eyes, a live preview that plays
// the faces. Each group is a radio group the arrow keys walk; every choice
// has a plain-word name.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Agent } from "../types";

const { save } = vi.hoisted(() => ({ save: vi.fn() }));
vi.mock("../apiPages", () => ({ pageApi: { saveAgentAvatar: save } }));
import { AvatarEditor } from "./AvatarEditor";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
const agent = {
  id: "coder",
  name: "Coder",
  avatar: { shape: "cloud", eyes: "long", accent: "teal" },
} as unknown as Agent;
const onSaved = vi.fn();
const onClose = vi.fn();

beforeEach(() => {
  vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }));
  save.mockReset().mockResolvedValue({});
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<AvatarEditor agent={agent} onSaved={onSaved} onClose={onClose} />));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

const group = (name: string) => host.querySelector<HTMLElement>(`[role="radiogroup"][aria-label="${name}"]`)!;
const radios = (name: string) => [...group(name).querySelectorAll<HTMLButtonElement>('[role="radio"]')];
const checked = (name: string) => radios(name).find((r) => r.getAttribute("aria-checked") === "true")!;

describe("AvatarEditor", () => {
  test("a grid of all twenty-one shapes in plain words, a colour row and the eyes", () => {
    expect(radios("Shape").map((r) => r.getAttribute("aria-label"))).toEqual([
      "round blob", "rounded triangle", "soft square", "hexagon", "diamond", "drop", "cloud", "bean", "heart", "star", "flower",
      "ghost", "cat", "bunny", "mushroom", "leaf", "mug", "shield", "speech bubble", "wizard hat", "rocket",
    ]);
    expect(radios("Colour").map((r) => r.getAttribute("aria-label"))).toEqual([
      "white", "brown", "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink", "grey", "black",
    ]);
    expect(radios("Eyes").map((r) => r.getAttribute("aria-label"))).toEqual(["tall", "round", "short", "wide", "sleepy"]);
    expect(checked("Shape").getAttribute("aria-label")).toBe("cloud");
    expect(checked("Colour").getAttribute("aria-label")).toBe("teal");
    expect(checked("Eyes").getAttribute("aria-label")).toBe("tall");
  });

  test("each shape is drawn in the current colour and eyes; each swatch is a round dot of its colour", () => {
    for (const r of radios("Shape")) {
      const av = r.querySelector<SVGSVGElement>("svg.agent-avatar")!;
      expect(av.dataset.accent).toBe("teal");
      expect(av.dataset.eyes).toBe("long");
      expect(av.getAttribute("aria-hidden")).toBe("true");
    }
    const dot = radios("Colour")[0]!.querySelector<HTMLElement>(".avatar-swatch")!;
    expect(dot.style.getPropertyValue("--sw")).toBe("var(--av-c-white)");
    expect(radios("Colour")[2]!.querySelector<HTMLElement>(".avatar-swatch")!.style.getPropertyValue("--sw")).toBe("var(--ac-red-2)");
  });

  test("picking redraws the grid, the eyes and the preview; Save sends the three names", async () => {
    act(() => radios("Shape").find((r) => r.title === "ghost")!.click());
    act(() => radios("Colour").find((r) => r.title === "grey")!.click());
    act(() => radios("Eyes").find((r) => r.title === "sleepy")!.click());
    const first = radios("Shape")[0]!.querySelector<SVGSVGElement>("svg")!;
    expect([first.dataset.accent, first.dataset.eyes]).toEqual(["graphite", "sleepy"]);
    const preview = host.querySelector<SVGSVGElement>(".avatar-preview svg.agent-avatar")!;
    expect([preview.dataset.shape, preview.dataset.accent, preview.dataset.eyes]).toEqual(["ghost", "graphite", "sleepy"]);
    await act(async () => [...host.querySelectorAll("button")].find((b) => b.textContent === "Save")!.click());
    expect(save).toHaveBeenCalledWith("coder", { shape: "ghost", eyes: "sleepy", accent: "graphite" });
    expect(onSaved).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  test("the preview plays the six faces", () => {
    const preview = () => host.querySelector<SVGSVGElement>(".avatar-preview svg.agent-avatar")!;
    for (const s of ["idle", "thinking", "working", "unsure", "done", "error"]) {
      act(() => [...group("preview face").querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === s)!.click());
      expect(preview().dataset.state).toBe(s);
    }
  });

  test("each group is one Tab stop and the arrow keys walk it, wrapping round", () => {
    for (const name of ["Shape", "Colour", "Eyes"]) {
      expect(radios(name).filter((r) => r.tabIndex === 0)).toEqual([checked(name)]);
    }
    const key = (el: HTMLElement, k: string) => act(() => void el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));
    key(checked("Shape"), "ArrowRight");
    expect(checked("Shape").title).toBe("bean");
    key(checked("Shape"), "ArrowLeft");
    key(checked("Shape"), "ArrowLeft");
    expect(checked("Shape").title).toBe("drop");
    // The swatches wrap from the last (black) to the first (white).
    for (let i = 0; i < 12; i++) key(checked("Colour"), "ArrowRight");
    expect(checked("Colour").title).toBe("teal");
    key(radios("Colour")[11]!, "ArrowRight");
    expect(checked("Colour").title).toBe("white");
    expect(document.activeElement).toBe(checked("Colour"));
  });

  test("focus starts on the shape picked", () => {
    expect(document.activeElement).toBe(checked("Shape"));
  });
});
