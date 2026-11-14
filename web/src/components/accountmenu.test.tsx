// @vitest-environment happy-dom
// The account menu, opened from the sidebar's footer button: the APG menu
// button keys and focus return, the header row opening Workspaces (Personal
// first and checked, teams A–Z), a failed switch saying why, a new
// workspace named in a dialog, drilling in place on a narrow screen, and no
// separate identity line.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Me, WorkspaceList, WorkspaceSummary } from "../types";
import type { SidebarProps } from "./Sidebar";

const createWorkspace = vi.fn();
vi.mock("../apiPages", () => ({ pageApi: { createWorkspace } }));
const { Sidebar } = await import("./Sidebar");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ws = (id: string, more: Partial<WorkspaceSummary>): WorkspaceSummary => ({
  workspace_id: id,
  name: id,
  personal: false,
  role: "member",
  permissions: [],
  members: 2,
  exec_level: "host",
  ...more,
});
const personal = ws("w-personal", { name: "stored", personal: true, role: "owner" });
const zeta = ws("w-zeta", { name: "Zeta", role: "member" });
const acme = ws("w-acme", { name: "acme", role: "admin" });
const list = (can_create = true): WorkspaceList => ({ workspaces: [personal, zeta, acme], active: personal.workspace_id, can_create });
const me = (workspace: WorkspaceSummary = personal): Me => ({
  user_id: "u1",
  username: "alex",
  role: "member",
  exec_ceiling: "none" as const,
  must_change_password: false,
  workspace,
});

/** `narrow`: what `(max-width: 760px)` answers (the menu then drills in place). */
function media(narrow: boolean) {
  window.matchMedia = ((query: string) => ({ matches: narrow && query.includes("max-width"), media: query }) as MediaQueryList) as typeof window.matchMedia;
}

let host: HTMLElement;
let root: Root;
const realMatchMedia = window.matchMedia;
beforeEach(() => {
  media(false);
  createWorkspace.mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  window.matchMedia = realMatchMedia;
});

async function render(over: Partial<SidebarProps> = {}) {
  const p: SidebarProps = {
    route: { kind: "home" },
    onNavigate: vi.fn(),
    me: me(),
    displayName: "Alex",
    sessions: [],
    nodes: [],
    conn: "live",
    workspaces: list(),
    features: {
      voice: false,
      memory: false,
      routines: { enabled: false, show: false, webhooks: false, why: null },
      media: { enabled: false, show: false, max_file: 0, vision_images: false },
      artifacts: { enabled: false, show: false, why: null },
      search: { kinds: ["chats", "projects"] },
    },
    routineUnseen: 0,
    onSwitch: vi.fn().mockResolvedValue(undefined),
    onLogout: vi.fn(),
    onSearch: vi.fn(),
    onSearchIntent: vi.fn(),
    onCollapse: vi.fn(),
    projects: [],
    canCreateProject: false,
    ...over,
  };
  act(() => root.render(<Sidebar {...p} />));
  return p;
}
const button = () => host.querySelector<HTMLButtonElement>("button[aria-haspopup='menu']")!;
const menus = () => [...document.querySelectorAll<HTMLElement>('[role="menu"]')];
const menu = (label: string) => menus().find((m) => m.getAttribute("aria-label") === label);
const items = (m: HTMLElement | undefined) => [...(m?.querySelectorAll<HTMLElement>('[role^="menuitem"]') ?? [])];
const texts = (m: HTMLElement | undefined) => items(m).map((i) => i.textContent?.trim());
const key = async (k: string, el: Element = document.activeElement!) =>
  act(async () => void el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true })));
async function open(how: "click" | "ArrowDown" = "click") {
  if (how === "click") await act(async () => button().click());
  else await key("ArrowDown", button());
  await act(async () => vi.dynamicImportSettled());
}

describe("account menu", () => {
  test("↓ or a click opens it on the header row; Esc or Tab closes it back to the button", async () => {
    await render();
    for (const close of ["Escape", "Tab"]) {
      button().focus();
      await open("ArrowDown");
      expect(button().getAttribute("aria-expanded")).toBe("true");
      const m = menu("Account")!;
      expect(button().getAttribute("aria-controls")).toBe(m.id);
      expect(items(m).map((i) => i.textContent)).toEqual(["AAlexPersonal · User", "Profile", "Settings", "Log out"]);
      expect(document.activeElement).toBe(items(m)[0]);
      await key(close);
      expect(menus()).toHaveLength(0);
      expect(document.activeElement).toBe(button());
    }
    await open("click");
    expect(document.activeElement).toBe(items(menu("Account"))[0]);
  });

  test("↑/↓ wrap; the header row names the account with its @username; no identity line", async () => {
    await render();
    await open();
    const m = menu("Account")!;
    const [head, , , logout] = items(m);
    await key("ArrowUp");
    expect(document.activeElement).toBe(logout);
    await key("ArrowDown");
    expect(document.activeElement).toBe(head);
    await key("End");
    expect(document.activeElement).toBe(logout);
    expect(head!.getAttribute("aria-haspopup")).toBe("menu");
    expect(head!.getAttribute("aria-label")).toBe("Alex (@alex) — Personal · User — Workspaces");
    expect(m.textContent).not.toContain("@alex");
  });

  test("→ opens Workspaces on the checked one: Personal first, then A–Z; ← goes back", async () => {
    await render({ me: me(acme) });
    await open();
    await key("ArrowRight");
    const sub = menu("Workspaces")!;
    const radios = [...sub.querySelectorAll<HTMLElement>('[role="menuitemradio"]')];
    expect(radios.map((r) => r.textContent)).toEqual(["AAlexPersonal", "AacmeAdmin", "ZZetaMember"]);
    expect(radios.map((r) => r.getAttribute("aria-checked"))).toEqual(["false", "true", "false"]);
    expect(document.activeElement).toBe(radios[1]);
    expect(texts(sub).slice(3)).toEqual(["New workspace…", "Workspace settings"]);
    await key("ArrowLeft");
    expect(menu("Workspaces")).toBeUndefined();
    expect(document.activeElement).toBe(items(menu("Account"))[0]);
    // Esc inside the submenu also only goes back.
    await key("ArrowRight");
    await key("Escape");
    expect(menu("Workspaces")).toBeUndefined();
    expect(menu("Account")).toBeDefined();
  });

  test("picking a workspace switches; a failed switch says why and stays open", async () => {
    const p = await render({ onSwitch: vi.fn().mockRejectedValue(new Error("you are no longer a member")) });
    await open();
    await key("ArrowRight");
    const zetaItem = [...menu("Workspaces")!.querySelectorAll<HTMLElement>('[role="menuitemradio"]')].find((r) => r.textContent?.includes("Zeta"))!;
    await act(async () => zetaItem.click());
    expect(p.onSwitch).toHaveBeenCalledWith("w-zeta");
    expect(document.querySelector('[role="alert"]')!.textContent).toBe("you are no longer a member");
    expect(menu("Workspaces")).toBeDefined();
    // The active one only closes the menu.
    const personalItem = menu("Workspaces")!.querySelector<HTMLElement>('[aria-checked="true"]')!;
    await act(async () => personalItem.click());
    expect(p.onSwitch).toHaveBeenCalledTimes(1);
    expect(menus()).toHaveLength(0);
  });

  test("a new workspace is named in a dialog, created, then switched to", async () => {
    createWorkspace.mockResolvedValue({ workspace_id: "w-new" });
    const p = await render();
    await open();
    await key("ArrowRight");
    await act(async () => items(menu("Workspaces")).find((i) => i.textContent?.trim() === "New workspace…")!.click());
    await act(async () => vi.dynamicImportSettled());
    expect(menus()).toHaveLength(0);
    const dialog = document.querySelector<HTMLDialogElement>("dialog.workspace-dialog")!;
    expect(dialog.open).toBe(true);
    const input = dialog.querySelector("input")!;
    await act(async () => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      set.call(input, "Garden club");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => dialog.querySelector<HTMLButtonElement>("button[type=submit]")!.click());
    expect(createWorkspace).toHaveBeenCalledWith("Garden club");
    expect(p.onSwitch).toHaveBeenCalledWith("w-new");
    expect(document.querySelector("dialog.workspace-dialog")).toBeNull();
  });

  test("without the right to create, there is no New workspace", async () => {
    await render({ workspaces: list(false) });
    await open();
    await key("ArrowRight");
    expect(texts(menu("Workspaces"))).not.toContain("New workspace…");
  });

  test("on a narrow screen Workspaces drills in place, Back first", async () => {
    media(true);
    await render();
    await open();
    await key("ArrowRight");
    expect(menus()).toHaveLength(1);
    const m = menu("Workspaces")!;
    expect(texts(m)[0]).toBe("Back");
    expect(document.activeElement).toBe(m.querySelector('[aria-checked="true"]'));
    await act(async () => items(m)[0]!.click());
    expect(menu("Account")).toBeDefined();
    expect(document.activeElement).toBe(items(menu("Account"))[0]);
  });

  test("Profile and Settings open the account's settings; Log out logs out", async () => {
    const p = await render();
    await open();
    await act(async () => items(menu("Account"))[1]!.click());
    expect(p.onNavigate).toHaveBeenLastCalledWith({ kind: "settings", section: "account", tab: "profile" });
    expect(menus()).toHaveLength(0);
    await open();
    expect(items(menu("Account"))[2]!.getAttribute("href")).toBe("#/settings/account/preferences");
    await act(async () => items(menu("Account"))[3]!.click());
    expect(p.onLogout).toHaveBeenCalled();
  });
});
