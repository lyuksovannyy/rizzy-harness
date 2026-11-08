// @vitest-environment happy-dom
// The search panel: it opens on the last tab used (All at first) and always
// names it, typing waits 150 ms and aborts the request before, tabs follow
// the viewer's kinds and the arrow keys, rows have two or three lines,
// ↑/↓ move the active row while focus stays in the input, Enter opens it,
// the next page loads at the end of a tab, and Close closes.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { SearchKind, SearchResponse, SearchResult } from "../types";

const search = vi.fn<(q: string, opts: { in?: string; cursor?: string | null; signal?: AbortSignal; workspace?: string | null }) => Promise<SearchResponse>>();
vi.mock("../api", () => ({ api: { search: (q: string, o: never) => search(q, o) } }));
const { default: SearchPanel, resultRoute } = await import("./SearchPanel");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const M = "\u0002";
const E = "\u0003";
const hit = (id: string, more: Partial<SearchResult> = {}): SearchResult => ({
  kind: "chat",
  id,
  title: `Chat ${id}`,
  type: "chat",
  preview: `About ${id}.`,
  snippet: null,
  time_ms: Date.now() - 5 * 60_000,
  project_id: null,
  ...more,
});
const answer = (results: SearchResult[], next: string | null = null, kinds: SearchKind[] = ["chats", "projects"]): SearchResponse => ({ results, next, kinds });

let host: HTMLElement;
let root: Root;
let observed: ((entries: { isIntersecting: boolean }[]) => void) | null;
beforeEach(() => {
  vi.useFakeTimers();
  localStorage.clear();
  search.mockReset().mockResolvedValue(answer([hit("a"), hit("b")]));
  observed = null;
  globalThis.IntersectionObserver = class {
    constructor(cb: (entries: { isIntersecting: boolean }[]) => void) {
      observed = cb;
    }
    observe() {}
    disconnect() {
      observed = null;
    }
  } as unknown as typeof IntersectionObserver;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

async function render(kinds: SearchKind[] = ["chats", "projects"]) {
  const p = { onOpen: vi.fn(), onClose: vi.fn() };
  await act(async () => root.render(<SearchPanel workspace="w1" kinds={kinds} projects={[]} onOpen={p.onOpen} onClose={p.onClose} />));
  return p;
}
const settle = () => act(async () => void vi.advanceTimersByTime(150));
const input = () => host.querySelector<HTMLInputElement>('input[role="combobox"]')!;
const tabs = () => [...host.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
const rows = () => [...host.querySelectorAll<HTMLElement>('[role="option"]')];
const type = (text: string) =>
  act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input(), text);
    input().dispatchEvent(new Event("input", { bubbles: true }));
  });
const key = (k: string, el: Element, more: KeyboardEventInit = {}) =>
  act(async () => void el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...more })));

describe("search panel", () => {
  test("opens as a modal on All, sending `in` and the workspace; nothing typed lists the newest", async () => {
    await render();
    expect(host.querySelector<HTMLDialogElement>("dialog")!.open).toBe(true);
    expect(document.activeElement).toBe(input());
    expect(search).not.toHaveBeenCalled();
    await settle();
    expect(search).toHaveBeenCalledTimes(1);
    expect(search.mock.calls[0]![0]).toBe("");
    expect(search.mock.calls[0]![1]).toMatchObject({ in: "all", workspace: "w1" });
    expect(rows()).toHaveLength(2);
    expect(host.querySelector('[aria-live="polite"]')!.textContent).toBe("2 results");
  });

  test("opens on the last tab used, unless the viewer no longer has it", async () => {
    localStorage.setItem("harness.searchTab", "projects");
    await render();
    await settle();
    expect(search.mock.calls[0]![1].in).toBe("projects");
    expect(tabs().find((t) => t.getAttribute("aria-selected") === "true")!.textContent).toBe("Projects");
    act(() => root.unmount());
    root = createRoot(host);
    search.mockClear();
    localStorage.setItem("harness.searchTab", "media");
    await render();
    await settle();
    expect(search.mock.calls[0]![1].in).toBe("all");
  });

  test("typing waits for a pause, and a newer request aborts the one before", async () => {
    await render();
    await settle();
    search.mockClear();
    let first: AbortSignal | undefined;
    search.mockImplementationOnce((_q, o) => {
      first = o.signal;
      return new Promise(() => {});
    });
    await type("t");
    await type("to");
    await type("tok");
    expect(search).not.toHaveBeenCalled();
    await settle();
    expect(search).toHaveBeenCalledTimes(1);
    expect(search.mock.calls[0]![0]).toBe("tok");
    await type("toki");
    expect(first!.aborted).toBe(true);
    await settle();
    expect(search.mock.calls[1]![0]).toBe("toki");
  });

  test("tabs show only the viewer's kinds, in order; a tab or ←/→ asks for that kind and is kept", async () => {
    search.mockResolvedValue(answer([hit("a")], null, ["chats", "projects", "media"]));
    await render();
    expect(tabs().map((t) => t.textContent)).toEqual(["All", "Chats", "Projects"]);
    await settle();
    expect(tabs().map((t) => t.textContent)).toEqual(["All", "Chats", "Media", "Projects"]);
    expect(tabs().map((t) => t.tabIndex)).toEqual([0, -1, -1, -1]);
    await act(async () => tabs()[3]!.click());
    await settle();
    expect(search.mock.lastCall![1].in).toBe("projects");
    expect(localStorage.getItem("harness.searchTab")).toBe("projects");
    await key("ArrowRight", tabs()[3]!);
    expect(document.activeElement!.textContent).toBe("All");
    await settle();
    expect(search.mock.lastCall![1].in).toBe("all");
    await key("ArrowLeft", tabs()[0]!);
    await settle();
    expect(search.mock.lastCall![1].in).toBe("projects");
  });

  test("rows: title · type and when, the preview, and the excerpt only when there is one", async () => {
    search.mockResolvedValue(
      answer([
        hit("a", { title: `Configure ${M}tokio${E}`, type: "agentic", snippet: null }),
        hit("b", { title: "", preview: "", snippet: `…the ${M}tokio${E} runtime…`, type: "routine" }),
        hit("c", { kind: "project", type: "project", title: "Garden" }),
      ]),
    );
    await render();
    await settle();
    const [a, b, c] = rows();
    expect(a!.querySelector(".search-line")!.textContent).toBe("Configure tokio · Agentic chat5m ago");
    expect(a!.querySelector("mark")!.textContent).toBe("tokio");
    expect(a!.querySelector(".search-preview")!.textContent).toBe("About a.");
    expect(a!.querySelector(".search-snippet")).toBeNull();
    expect(b!.querySelector(".search-line")!.textContent).toContain("New chat · Routine run");
    expect(b!.querySelector(".search-preview")).toBeNull();
    expect(b!.querySelector(".search-snippet mark")!.textContent).toBe("tokio");
    expect(c!.textContent).toContain("Garden · Project");
  });

  test("↑/↓ move the active row while focus stays in the input; Enter opens it", async () => {
    const p = await render();
    await settle();
    const ids = rows().map((r) => r.id);
    expect(input().getAttribute("aria-activedescendant")).toBe(ids[0]);
    await key("ArrowDown", input());
    expect(input().getAttribute("aria-activedescendant")).toBe(ids[1]);
    expect(rows()[1]!.getAttribute("aria-selected")).toBe("true");
    await key("ArrowDown", input());
    expect(input().getAttribute("aria-activedescendant")).toBe(ids[1]);
    await key("Home", input());
    expect(input().getAttribute("aria-activedescendant")).toBe(ids[0]);
    await key("End", input());
    expect(document.activeElement).toBe(input());
    await key("Enter", input());
    expect(p.onOpen).toHaveBeenCalledWith({ kind: "session", id: "b" });
    await act(async () => rows()[0]!.click());
    expect(p.onOpen).toHaveBeenLastCalledWith({ kind: "session", id: "a" });
  });

  test("Ctrl+K inside selects what is typed; Close closes", async () => {
    const p = await render();
    await type("tokio");
    input().setSelectionRange(5, 5);
    await key("k", input(), { ctrlKey: true });
    expect([input().selectionStart, input().selectionEnd]).toEqual([0, 5]);
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Close search"]')!.click());
    expect(p.onClose).toHaveBeenCalled();
  });

  test("a tab loads its next page at the end of the list; All never pages", async () => {
    localStorage.setItem("harness.searchTab", "chats");
    search.mockResolvedValueOnce(answer([hit("a"), hit("b")], "2"));
    await render();
    await settle();
    expect(observed).not.toBeNull();
    search.mockResolvedValueOnce(answer([hit("c")], null));
    await act(async () => observed!([{ isIntersecting: true }]));
    expect(search.mock.lastCall![1]).toMatchObject({ in: "chats", cursor: "2" });
    expect(rows().map((r) => r.id.length > 0 && r.textContent!.slice(0, 6))).toEqual(["Chat a", "Chat b", "Chat c"]);
    expect(observed).toBeNull();
    act(() => root.unmount());
    root = createRoot(host);
    localStorage.setItem("harness.searchTab", "all");
    search.mockResolvedValueOnce(answer([hit("a")], "1"));
    await render();
    await settle();
    expect(observed).toBeNull();
  });

  test("each kind opens its page", () => {
    expect(resultRoute(hit("s"))).toEqual({ kind: "session", id: "s" });
    expect(resultRoute(hit("p", { kind: "project" }))).toEqual({ kind: "project", id: "p" });
    expect(resultRoute(hit("m", { kind: "media" }))).toEqual({ kind: "media", filter: null, id: "m" });
    expect(resultRoute(hit("x", { kind: "artifact" }))).toEqual({ kind: "artifacts", id: "x" });
  });
});
