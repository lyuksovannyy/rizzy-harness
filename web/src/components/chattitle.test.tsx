// @vitest-environment happy-dom
// Renaming a chat in its header: Rename opens a capped field with the
// title; Enter saves through the API, Escape leaves it unchanged; an
// unchanged or blank title sends nothing; the server's refusal is shown as
// text; without a row (not the owner's list) there is no Rename.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

const rename = vi.fn((_id: string, title: string) => Promise.resolve({ ok: true, title }));
vi.mock("../apiPages", () => ({ pageApi: { renameSession: (id: string, t: string) => rename(id, t) } }));
const { ChatTitle, TITLE_MAX } = await import("./ChatTitle");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  rename.mockClear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const field = () => host.querySelector<HTMLInputElement>("input")!;
const type = (v: string) =>
  act(() => {
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    set.call(field(), v);
    field().dispatchEvent(new Event("input", { bubbles: true }));
  });
const key = (k: string) => act(() => void field().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));
const submit = () => act(async () => void host.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));

describe("chat title", () => {
  test("rename and save", async () => {
    act(() => root.render(<ChatTitle sessionId="s1" title="Lunch ideas" canRename />));
    expect(host.querySelector("h1")!.textContent).toBe("Lunch ideas");
    act(() => host.querySelector<HTMLButtonElement>('button[aria-label="Rename chat"]')!.click());
    expect(field().value).toBe("Lunch ideas");
    expect(field().maxLength).toBe(TITLE_MAX);
    type("Zeppelin budget");
    await submit();
    expect(rename).toHaveBeenCalledWith("s1", "Zeppelin budget");
    expect(host.querySelector("input")).toBeNull();
  });

  test("Escape leaves it; unchanged or blank sends nothing", async () => {
    act(() => root.render(<ChatTitle sessionId="s1" title="Lunch ideas" canRename />));
    act(() => host.querySelector<HTMLButtonElement>("button")!.click());
    type("Something else");
    key("Escape");
    expect(host.querySelector("h1")!.textContent).toBe("Lunch ideas");
    act(() => host.querySelector<HTMLButtonElement>("button")!.click());
    await submit();
    act(() => host.querySelector<HTMLButtonElement>("button")!.click());
    type("   ");
    await submit();
    expect(rename).not.toHaveBeenCalled();
  });

  test("the server's refusal is shown as text and the field stays", async () => {
    rename.mockImplementationOnce(() => Promise.reject(new Error("title is at most 200 characters <b>")));
    act(() => root.render(<ChatTitle sessionId="s1" title="a" canRename />));
    act(() => host.querySelector<HTMLButtonElement>("button")!.click());
    type("b");
    await submit();
    expect(host.querySelector(".error-text")!.textContent).toBe("title is at most 200 characters <b>");
    expect(host.querySelector(".error-text b")).toBeNull();
    expect(field()).not.toBeNull();
  });

  test("no Rename without the owner's row", () => {
    act(() => root.render(<ChatTitle sessionId="s1" title="a" canRename={false} />));
    expect(host.querySelector("button")).toBeNull();
  });
});
