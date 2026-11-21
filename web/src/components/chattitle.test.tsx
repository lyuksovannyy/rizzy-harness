// @vitest-environment happy-dom
// Renaming a chat in its header, with no buttons: a click on the title (or
// Enter or F2 while it has keyboard focus) turns it into a field in place,
// text selected. Enter applies; a click outside (blur) applies; Escape
// cancels. An unchanged or blank title sends nothing (blank reverts); a
// server refusal is shown inline and the field stays open with what was
// typed; Enter that confirms an IME composition does not apply. Whoever may
// not rename gets plain text that cannot be focused.
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

const show = (title = "Lunch ideas", canRename = true) => act(() => root.render(<ChatTitle sessionId="s1" title={title} canRename={canRename} />));
const trigger = () => host.querySelector<HTMLButtonElement>('button[title="Rename chat"]')!;
const open = () => act(() => trigger().click());
const field = () => host.querySelector<HTMLInputElement>("input")!;
const type = (v: string) =>
  act(() => {
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    set.call(field(), v);
    field().dispatchEvent(new Event("input", { bubbles: true }));
  });
const key = (k: string, init: KeyboardEventInit = {}, el: Element = field()) =>
  act(async () => void el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...init })));
const clickOut = (el: Element = field()) => act(async () => void el.dispatchEvent(new FocusEvent("focusout", { bubbles: true })));

describe("chat title", () => {
  test("a click opens the field in place, capped, with the title selected; no buttons", () => {
    show();
    expect(host.querySelector("h1")!.textContent).toBe("Lunch ideas");
    expect(host.querySelector("svg")).toBeNull(); // no pencil
    open();
    expect(field().value).toBe("Lunch ideas");
    expect(field().maxLength).toBe(TITLE_MAX);
    expect(field().selectionStart).toBe(0);
    expect(field().selectionEnd).toBe("Lunch ideas".length);
    expect(document.activeElement).toBe(field());
    expect(host.querySelectorAll("button")).toHaveLength(0);
  });

  test("Enter applies; the saved title shows at once and a later rename elsewhere shows when it arrives", async () => {
    show();
    open();
    type("Zeppelin budget");
    await key("Enter");
    expect(rename).toHaveBeenCalledWith("s1", "Zeppelin budget");
    expect(host.querySelector("input")).toBeNull();
    expect(host.querySelector("h1")!.textContent).toBe("Zeppelin budget");
    show("Zeppelin budget");
    expect(host.querySelector("h1")!.textContent).toBe("Zeppelin budget");
    show("Renamed elsewhere");
    expect(host.querySelector("h1")!.textContent).toBe("Renamed elsewhere");
  });

  test("a click outside the field applies, once", async () => {
    show();
    open();
    type("Moved by a click");
    const f = field();
    await act(async () => {
      f.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
      f.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    });
    expect(rename).toHaveBeenCalledTimes(1);
    expect(rename).toHaveBeenCalledWith("s1", "Moved by a click");
    expect(host.querySelector("h1")!.textContent).toBe("Moved by a click");
  });

  test("Escape cancels: nothing is sent, not even by the blur that follows", async () => {
    show();
    open();
    type("Something else");
    const gone = field();
    await key("Escape");
    expect(host.querySelector("input")).toBeNull();
    expect(host.querySelector("h1")!.textContent).toBe("Lunch ideas");
    await clickOut(gone);
    expect(rename).not.toHaveBeenCalled();
    // The next time starts from the title again.
    open();
    expect(field().value).toBe("Lunch ideas");
  });

  test("an unchanged title sends nothing; a blank one reverts and sends nothing", async () => {
    show();
    open();
    await key("Enter");
    expect(rename).not.toHaveBeenCalled();
    expect(host.querySelector("h1")!.textContent).toBe("Lunch ideas");
    open();
    type("   ");
    await clickOut();
    expect(rename).not.toHaveBeenCalled();
    expect(host.querySelector("input")).toBeNull();
    expect(host.querySelector("h1")!.textContent).toBe("Lunch ideas");
    open();
    type("  Lunch ideas  ");
    await key("Enter");
    expect(rename).not.toHaveBeenCalled();
  });

  test("the server's refusal is shown as text, and the field stays open with what was typed", async () => {
    rename.mockImplementationOnce(() => Promise.reject(new Error("title is at most 200 characters <b>")));
    show("a");
    open();
    type("b");
    await key("Enter");
    expect(host.querySelector(".error-text")!.textContent).toBe("title is at most 200 characters <b>");
    expect(host.querySelector(".error-text b")).toBeNull();
    expect(field().value).toBe("b");
    // A second try can succeed.
    await key("Enter");
    expect(rename).toHaveBeenCalledTimes(2);
    expect(host.querySelector("input")).toBeNull();
  });

  test("Enter that confirms an IME composition does not apply", async () => {
    show();
    open();
    type("にほん");
    await key("Enter", { isComposing: true });
    await key("Enter", { keyCode: 229 });
    expect(rename).not.toHaveBeenCalled();
    expect(field()).not.toBeNull();
    await key("Enter");
    expect(rename).toHaveBeenCalledWith("s1", "にほん");
  });

  test("the title is reached by Tab (a button) and Enter or F2 opens the field", async () => {
    show();
    const b = trigger();
    expect(b.tagName).toBe("BUTTON");
    expect(b.tabIndex).toBe(0);
    expect(b.textContent).toBe("Lunch ideas");
    await key("Enter", {}, b);
    expect(field()).not.toBeNull();
    await key("Escape");
    await key("F2", {}, trigger());
    expect(field()).not.toBeNull();
  });

  test("a selected title (to copy it) does not open the field", () => {
    show();
    const sel = vi.spyOn(window, "getSelection").mockReturnValue({ isCollapsed: false, toString: () => "Lunch" } as unknown as Selection);
    open();
    expect(host.querySelector("input")).toBeNull();
    sel.mockRestore();
    open();
    expect(field()).not.toBeNull();
  });

  test("a viewer who may not rename gets plain text, not focusable", () => {
    show("a", false);
    expect(host.querySelector("h1")!.textContent).toBe("a");
    expect(host.querySelector("button, input, [tabindex]")).toBeNull();
  });
});
