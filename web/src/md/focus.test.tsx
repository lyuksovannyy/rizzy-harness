// @vitest-environment happy-dom
// Where keyboard focus goes when a popover of ours closes: back to what
// opened it. A focused element that is removed leaves focus at the top of
// the document, and a keyboard user has to find their place again.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, test } from "vitest";
import { Markdown } from "../components/Markdown";
import type { Source } from "../types";
import { MessageCtx, NO_CITES, NONE } from "./context";

let host: HTMLElement | null = null;
let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
});

async function mount(text: string, cites: ReadonlyMap<string, Source> = NO_CITES) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => {
    root!.render(
      <MessageCtx.Provider value={{ provenance: NONE, sources: NONE, cites }}>
        <Markdown text={text} done />
      </MessageCtx.Provider>,
    );
  });
  return host;
}

const key = (el: Element, k: string) =>
  act(() => void el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));

describe("link confirm", () => {
  const open = async () => {
    const el = await mount("[look](https://evil.example/a?q=1)");
    const link = el.querySelector<HTMLAnchorElement>("a[href='https://evil.example/a?q=1']")!;
    link.focus();
    await act(async () => link.click());
    const box = document.querySelector(".link-confirm")!;
    expect(box).not.toBeNull();
    expect(document.activeElement?.textContent).toBe("Open");
    return { link, box };
  };

  test("Escape and Cancel give focus back to the link", async () => {
    let { link } = await open();
    await key(document.activeElement!, "Escape");
    expect(document.querySelector(".link-confirm")).toBeNull();
    expect(document.activeElement).toBe(link);
    act(() => root!.unmount());
    host!.remove();

    let box: Element;
    ({ link, box } = await open());
    const cancel = [...box.querySelectorAll("button")].find((b) => b.textContent === "Cancel")!;
    await act(async () => cancel.click());
    expect(document.querySelector(".link-confirm")).toBeNull();
    expect(document.activeElement).toBe(link);
  });

  test("a click elsewhere closes it and keeps its own focus", async () => {
    const { link } = await open();
    const other = document.createElement("button");
    document.body.append(other);
    other.focus();
    await act(async () => void other.dispatchEvent(new MouseEvent("mousedown", { bubbles: true })));
    expect(document.querySelector(".link-confirm")).toBeNull();
    expect(document.activeElement).not.toBe(link);
    other.remove();
  });
});

describe("citation card", () => {
  test("Escape inside the card gives focus back to the pill", async () => {
    const cites = new Map<string, Source>([["S1", { id: "S1", url: "https://a.example/rust", title: "Rust" }]]);
    const el = await mount("Rust is fast [S1].", cites);
    const pill = el.querySelector<HTMLAnchorElement>("a.cite")!;
    act(() => pill.focus());
    const item = document.querySelector<HTMLAnchorElement>(".cite-card a.cite-card-item")!;
    expect(item).not.toBeNull();
    act(() => item.focus());
    await key(item, "Escape");
    expect(document.querySelector(".cite-card")).toBeNull();
    expect(document.activeElement).toBe(pill);
  });
});
