// @vitest-environment happy-dom
// Temporary passwords (AUDIT L14): the server makes them; the admin sees
// each once in a dialog, can copy it, and closing the dialog forgets it.
// No browser prompt() is ever involved.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Me, User } from "../types";

const PW = "k3m9q-x7vt2-ab4cd-0ze8h";
const bob: User = {
  user_id: "b",
  username: "bob",
  role: "member",
  disabled: false,
  exec_ceiling: "none" as const,
  exec_ceiling_stored: "none" as const,
  must_change_password: false,
  created_ms: 0,
};
const api = vi.hoisted(() => ({
  users: vi.fn(),
  createUser: vi.fn(),
  resetPassword: vi.fn(),
  updateUser: vi.fn(),
  deleteUser: vi.fn(),
  setCeiling: vi.fn(),
}));
vi.mock("../api", () => ({ api }));
vi.mock("../apiPages", () => ({ pageApi: api }));
const { Users } = await import("./Users");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const me: Me = { user_id: "a", username: "alex", role: "admin", exec_ceiling: "host" as const, must_change_password: false };
const writeText = vi.fn().mockResolvedValue(undefined);

beforeEach(() => {
  document.body.innerHTML = "";
  api.users.mockResolvedValue({ users: [{ ...bob, user_id: "a", username: "alex", role: "admin" }, bob] });
  api.createUser.mockResolvedValue({ ...bob, user_id: "c", username: "carol", must_change_password: true, password: PW });
  api.resetPassword.mockResolvedValue({ ...bob, must_change_password: true, password: PW });
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  vi.stubGlobal("prompt", vi.fn(() => "typed by the admin"));
  vi.stubGlobal("confirm", vi.fn(() => true));
  writeText.mockClear();
});
afterEach(() => vi.unstubAllGlobals());

async function mount() {
  const host = document.createElement("div");
  document.body.append(host);
  await act(async () => createRoot(host).render(<Users me={me} />));
  return host;
}

const button = (host: HTMLElement, text: string) =>
  [...host.querySelectorAll("button")].find((b) => b.textContent === text)!;

const typeInto = (el: HTMLInputElement, value: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });

/** The password is in the DOM, in no storage, and in no URL. */
function where(host: HTMLElement) {
  const stored = [localStorage, sessionStorage].some((s) =>
    Object.keys(s).some((k) => (s.getItem(k) ?? "").includes(PW) || k.includes(PW)),
  );
  return { shown: host.innerHTML.includes(PW), stored, url: location.href.includes(PW) };
}

async function shownThenForgotten(host: HTMLElement) {
  const dialog = host.querySelector("dialog")!;
  expect(dialog).not.toBeNull();
  expect(dialog.textContent).toContain(PW);
  expect(dialog.textContent).toMatch(/will not be shown again/i);
  expect(where(host)).toEqual({ shown: true, stored: false, url: false });

  await act(async () => button(host, "Copy").click());
  expect(writeText).toHaveBeenCalledWith(PW);
  expect(host.textContent).toContain("Copied");

  await act(async () => button(host, "Done").click());
  expect(host.querySelector("dialog")).toBeNull();
  expect(where(host)).toEqual({ shown: false, stored: false, url: false });
}

describe("temporary passwords", () => {
  test("creating a user sends no password and shows the server's once", async () => {
    const host = await mount();
    expect(host.querySelector('input[placeholder^="temporary password"]')).toBeNull();
    typeInto(host.querySelector<HTMLInputElement>('input[placeholder="username"]')!, "carol");
    await act(async () => button(host, "Create").click());
    // No level: every new account starts at none, set in the list.
    expect(api.createUser).toHaveBeenCalledWith({ username: "carol", role: "member" });
    expect(host.querySelector("dialog")!.textContent).toContain("carol");
    await shownThenForgotten(host);
    expect(prompt).not.toHaveBeenCalled();
  });

  test("a reset asks the server, never prompt()", async () => {
    const host = await mount();
    await act(async () => button(host, "Reset password").click());
    expect(prompt).not.toHaveBeenCalled();
    expect(api.resetPassword).toHaveBeenCalledWith("b");
    expect(api.updateUser).not.toHaveBeenCalled();
    await shownThenForgotten(host);
  });

  test("a reset the admin cancels asks nothing of the server", async () => {
    vi.stubGlobal("confirm", vi.fn(() => false));
    const host = await mount();
    await act(async () => button(host, "Reset password").click());
    expect(api.resetPassword).not.toHaveBeenCalled();
    expect(host.querySelector("dialog")).toBeNull();
  });
});

describe("the account's level (N.10a part 2)", () => {
  const level = (host: HTMLElement, who: string) =>
    host.querySelector<HTMLSelectElement>(`select[aria-label="Level of ${who}"]`);
  const choose = (el: HTMLSelectElement, value: string) =>
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(el, value);
      el.dispatchEvent(new Event("change", { bubbles: true }));
    });
  const warning = async () => {
    for (let i = 0; i < 50 && !document.querySelector("dialog.host-warning"); i++) {
      await act(async () => new Promise((r) => setTimeout(r, 10)));
    }
    return document.querySelector<HTMLDialogElement>("dialog.host-warning");
  };

  test("granting host shows the risks before anything is saved; cancelling saves nothing", async () => {
    api.setCeiling.mockClear();
    api.setCeiling.mockResolvedValue({ ...bob, exec_ceiling: "host", exec_ceiling_stored: "host" });
    const host = await mount();
    const sel = level(host, "bob")!;
    expect(sel.value).toBe("none");
    choose(sel, "host");
    const d = (await warning())!;
    expect(d).not.toBeNull();
    expect(d.textContent).toContain("Let bob run commands without the sandbox?");
    for (const risk of [
      "with nothing between them and the machine",
      "its keys and other projects included",
      "can keep running after the chat ends",
      "one web page or file the agent reads can turn all of that against you",
      "The approval card is then the only check",
      "what a sandbox chat wrote in a folder",
    ]) {
      expect(d.textContent).toContain(risk);
    }
    expect(api.setCeiling).not.toHaveBeenCalled();
    await act(async () => button(d as unknown as HTMLElement, "Cancel").click());
    expect(document.querySelector("dialog.host-warning")).toBeNull();
    expect(api.setCeiling).not.toHaveBeenCalled();
    expect(level(host, "bob")!.value).toBe("none");
    // Confirmed, it is saved.
    choose(level(host, "bob")!, "host");
    await act(async () => button((await warning()) as unknown as HTMLElement, "Grant host").click());
    expect(api.setCeiling).toHaveBeenCalledWith("b", "host");
  });

  test("a lower level is saved at once; a server admin's is host and not set here", async () => {
    api.setCeiling.mockResolvedValue({ ...bob, exec_ceiling: "sandbox", exec_ceiling_stored: "sandbox" });
    const host = await mount();
    choose(level(host, "bob")!, "sandbox");
    await act(async () => {});
    expect(document.querySelector("dialog.host-warning")).toBeNull();
    expect(api.setCeiling).toHaveBeenCalledWith("b", "sandbox");
    expect(level(host, "alex")).toBeNull();
  });
});
