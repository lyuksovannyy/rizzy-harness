// @vitest-environment happy-dom
// The sidebar's workspace switcher: Personal first and named from the
// catalog, a role hint on team workspaces, a pick switches, a new workspace
// is created and then switched to, and the settings entry opens them.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { WorkspaceList, WorkspaceSummary } from "../types";

const createWorkspace = vi.fn();
vi.mock("../api", () => ({ api: { createWorkspace } }));
const { WorkspaceSwitcher } = await import("./WorkspaceSwitcher");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const personal: WorkspaceSummary = {
  workspace_id: "w-personal",
  name: "stored name",
  personal: true,
  role: "owner",
  permissions: ["agentic", "settings", "members"],
  members: 1,
};
const acme: WorkspaceSummary = {
  workspace_id: "w-acme",
  name: "Acme",
  personal: false,
  role: "guest",
  permissions: [],
  members: 4,
};
const list = (can_create: boolean): WorkspaceList => ({ workspaces: [personal, acme], active: personal.workspace_id, can_create });

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  createWorkspace.mockReset();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

function render(l: WorkspaceList | null, onSwitch = vi.fn().mockResolvedValue(undefined), onSettings = vi.fn()) {
  act(() =>
    root.render(<WorkspaceSwitcher active={personal} list={l} onSwitch={onSwitch} onSettings={onSettings} />),
  );
  return { onSwitch, onSettings };
}
const button = () => host.querySelector<HTMLButtonElement>(".select-button")!;
const options = () => [...host.querySelectorAll<HTMLElement>('[role="option"]')];
const open = () => act(() => button().click());
const choose = (label: string) => {
  open();
  act(() => options().find((o) => o.querySelector(".option-label")!.textContent!.endsWith(label))!.click());
};

describe("WorkspaceSwitcher", () => {
  test("lists Personal first, by its catalog name, with a badge and the role elsewhere", () => {
    render(list(true));
    expect(button().getAttribute("aria-label")).toBe("Workspace: Personal");
    expect(button().querySelector(".workspace-badge")!.textContent).toBe("P");
    open();
    expect(options().map((o) => o.textContent)).toEqual(["PPersonal", "AAcmeGuest", "New workspace…", "Workspace settings"]);
  });

  test("offers no new workspace when the server does not allow it, and only the active one before the list loads", () => {
    render(list(false));
    open();
    expect(options().map((o) => o.querySelector(".option-label")!.textContent)).toEqual(["PPersonal", "AAcme", "Workspace settings"]);
    render(null);
    expect(options().map((o) => o.querySelector(".option-label")!.textContent)).toEqual(["PPersonal", "Workspace settings"]);
  });

  test("a pick switches; the active one does not", () => {
    const { onSwitch } = render(list(true));
    choose("Personal");
    expect(onSwitch).not.toHaveBeenCalled();
    choose("Acme");
    expect(onSwitch).toHaveBeenCalledWith("w-acme");
  });

  test("a failed switch says why", async () => {
    render(list(true), vi.fn().mockRejectedValue(new Error("unknown workspace")));
    choose("Acme");
    await act(async () => {});
    expect(host.querySelector(".error-text")!.textContent).toBe("unknown workspace");
  });

  test("Workspace settings opens them", () => {
    const { onSettings, onSwitch } = render(list(true));
    choose("Workspace settings");
    expect(onSettings).toHaveBeenCalled();
    expect(onSwitch).not.toHaveBeenCalled();
  });

  test("a new workspace is created by name, then switched to", async () => {
    createWorkspace.mockResolvedValue({ ...acme, workspace_id: "w-new", name: "Lab", role: "owner" });
    const { onSwitch } = render(list(true));
    choose("New workspace…");
    const input = host.querySelector<HTMLInputElement>('input[aria-label="Workspace name"]')!;
    const create = [...host.querySelectorAll("button")].find((b) => b.textContent === "Create")!;
    expect(create.disabled).toBe(true);
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "  Lab ");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => create.click());
    expect(createWorkspace).toHaveBeenCalledWith("Lab");
    expect(onSwitch).toHaveBeenCalledWith("w-new");
    expect(host.querySelector(".workspace-new")).toBeNull();
  });
});
