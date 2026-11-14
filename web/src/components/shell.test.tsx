// @vitest-environment happy-dom
// Shell's wiring, with the socket, the API and the screens stubbed: a
// re-render with new callbacks from App keeps the one Live connection open
// (a closed Live never reconnects), a workspaces_changed keeps the tab in its
// own workspace, reloading its chats, until the user is out of it; the search
// shortcuts; the drawer on a narrow screen; the unseen routine results here.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { AttentionResponse, Me, WorkspaceList, WorkspaceSummary } from "../types";
import type { SidebarProps } from "./Sidebar";

const lives: { close: () => void; onWorkspacesChanged: () => void }[] = [];
vi.mock("../live", () => ({
  Live: class {
    state = "live";
    version = 0;
    onSessionsChanged = () => {};
    onNodesChanged = () => {};
    onMemoryChanged = () => {};
    onWorkspacesChanged = () => {};
    onTopic = () => () => {};
    close = vi.fn();
    constructor() {
      lives.push(this);
    }
    subscribe() {
      return () => {};
    }
  },
}));
// What the tests look at; every other call stays pending.
const stubs = { sessions: vi.fn(), workspaces: vi.fn(), attention: vi.fn(), agents: vi.fn(), settings: vi.fn() };
vi.mock("../api", () => ({
  api: new Proxy(stubs, {
    get: (o, k: string) => (o as Record<string, unknown>)[k] ?? (() => new Promise(() => {})),
  }),
  HttpError: class extends Error {},
  // lib/projects.ts calls the API through `call`.
  call: () => new Promise(() => {}),
}));
// What Shell hands the home screen (the new chat's agent defaults).
let homeProps: Record<string, unknown> | null = null;
vi.mock("./Home", () => ({
  Home: (p: Record<string, unknown>) => {
    homeProps = p;
    return null;
  },
}));
let sidebar: SidebarProps | null = null;
vi.mock("./Sidebar", () => ({
  Sidebar: (p: SidebarProps) => {
    sidebar = p;
    return (
      <aside className="sidebar">
        <nav>
          <a href="#/">New chat</a>
        </nav>
      </aside>
    );
  },
}));
vi.mock("./SearchPanel", () => ({ default: () => <dialog open className="panel-stub" /> }));
const { Shell, opensSearch } = await import("./Shell");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ws = (id: string, more: Partial<WorkspaceSummary> = {}): WorkspaceSummary => ({
  workspace_id: id,
  name: id,
  personal: false,
  role: "member",
  permissions: ["routines"],
  members: 2,
  exec_level: "host",
  ...more,
});
const personal = ws("p", { personal: true, role: "owner", members: 1 });
const acme = ws("acme", { name: "Acme" });
const me: Me = { user_id: "u", username: "alex", role: "member", exec_ceiling: "host" as const, must_change_password: false, workspace: acme };
const list = (...workspaces: WorkspaceSummary[]): WorkspaceList => ({ workspaces, active: "p", can_create: true });
// New functions on every call, as an App re-render could hand down.
const props = () => ({
  me,
  onLogout: vi.fn(),
  onMeChanged: vi.fn((_keep?: WorkspaceSummary) => Promise.resolve()),
  gone: null,
  onGone: vi.fn(),
});

let host: HTMLElement;
let root: Root;
let mounted = false;
beforeEach(() => {
  lives.length = 0;
  stubs.sessions.mockReset().mockResolvedValue({ sessions: [] });
  stubs.workspaces.mockReset().mockResolvedValue(list(personal, acme));
  stubs.attention.mockReset().mockReturnValue(new Promise(() => {}));
  stubs.agents.mockReset().mockReturnValue(new Promise(() => {}));
  stubs.settings.mockReset().mockReturnValue(new Promise(() => {}));
  homeProps = null;
  sidebar = null;
  localStorage.clear();
  history.replaceState(null, "", "#/");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  mounted = true;
});
afterEach(() => {
  if (mounted) act(() => root.unmount());
  host.remove();
});
const render = (p: ReturnType<typeof props>) => act(async () => root.render(<Shell {...p} />));

describe("Shell", () => {
  // TD-1: a new Agentic chat gets the Agentic default, not the chat one.
  const agentsReply = { agents: [], default: "general", defaults: { chat: "general", agentic: "coder" } };
  const prefsReply = (more: Record<string, unknown> = {}) => ({ search_url: "", default_agent: null, default_model: null, onboarded: true, ...more });

  test("the home screen gets the server's Agentic default beside the chat default", async () => {
    stubs.agents.mockResolvedValue(agentsReply);
    stubs.settings.mockResolvedValue(prefsReply());
    await render(props());
    expect(homeProps?.agentDefaults).toEqual({ chat: "general", agentic: "coder" });
  });

  test("the person's own defaults win, per mode", async () => {
    stubs.agents.mockResolvedValue(agentsReply);
    stubs.settings.mockResolvedValue(prefsReply({ default_agent: "writer", default_agentic_agent: "teamlead" }));
    await render(props());
    expect(homeProps?.agentDefaults).toEqual({ chat: "writer", agentic: "teamlead" });
  });

  test("new callbacks from App keep the live connection; only unmounting closes it", async () => {
    await render(props());
    await render(props());
    expect(lives).toHaveLength(1);
    expect(lives[0]!.close).not.toHaveBeenCalled();
    act(() => root.unmount());
    mounted = false;
    expect(lives[0]!.close).toHaveBeenCalledTimes(1);
  });

  test("workspaces_changed keeps the tab's workspace, refreshed, and reloads its chats", async () => {
    const p = props();
    await render(p);
    const before = stubs.sessions.mock.calls.length;
    const promoted = ws("acme", { name: "Acme", role: "admin", permissions: ["settings", "members", "routines"], members: 3, exec_level: "host" as const });
    stubs.workspaces.mockResolvedValue(list(personal, promoted));
    await act(async () => lives[0]!.onWorkspacesChanged());
    expect(stubs.sessions.mock.calls.length).toBe(before + 1);
    expect(stubs.sessions).toHaveBeenLastCalledWith("acme");
    expect(p.onMeChanged).toHaveBeenLastCalledWith(promoted);
    expect(p.onGone).not.toHaveBeenCalled();
  });

  test("taken out of it, the tab falls back to the server's and then says so", async () => {
    const p = props();
    await render(p);
    stubs.workspaces.mockResolvedValue(list(personal));
    await act(async () => lives[0]!.onWorkspacesChanged());
    expect(p.onMeChanged).toHaveBeenLastCalledWith();
    expect(p.onGone).toHaveBeenCalledWith("Acme");
    expect(p.onMeChanged.mock.invocationCallOrder.at(-1)!).toBeLessThan(p.onGone.mock.invocationCallOrder.at(-1)!);
  });

  test("Ctrl/Cmd+K opens search anywhere, the composer too; \"/\" only outside fields", async () => {
    await render(props());
    const panel = () => host.querySelector(".panel-stub");
    const press = async (init: KeyboardEventInit, target: EventTarget = document.body) =>
      act(async () => {
        target.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init }));
        await vi.dynamicImportSettled();
      });
    const field = document.createElement("textarea");
    document.body.append(field);
    await press({ key: "/" }, field);
    expect(panel()).toBeNull();
    await press({ key: "k", ctrlKey: true }, field);
    expect(panel()).not.toBeNull();
    field.remove();
  });

  test("which keys open search", () => {
    const ev = (init: KeyboardEventInit, target?: HTMLElement) => {
      const e = new KeyboardEvent("keydown", init);
      if (target) Object.defineProperty(e, "target", { value: target });
      return e;
    };
    const editable = document.createElement("div");
    editable.setAttribute("contenteditable", "true");
    document.body.append(editable);
    expect(opensSearch(ev({ key: "k", metaKey: true }))).toBe(true);
    expect(opensSearch(ev({ key: "K", ctrlKey: true }))).toBe(true);
    expect(opensSearch(ev({ key: "k" }))).toBe(false);
    expect(opensSearch(ev({ key: "k", ctrlKey: true, altKey: true }))).toBe(false);
    expect(opensSearch(ev({ key: "/" }, document.body))).toBe(true);
    for (const tag of ["input", "textarea", "select"]) expect(opensSearch(ev({ key: "/" }, document.createElement(tag)))).toBe(false);
    expect(opensSearch(ev({ key: "/" }, editable))).toBe(false);
    expect(opensSearch(ev({ key: "/", ctrlKey: true }, document.body))).toBe(false);
    editable.remove();
  });

  test("narrow: the drawer holds the whole sidebar, takes focus, makes the page inert and closes on Esc", async () => {
    const real = window.matchMedia;
    window.matchMedia = ((query: string) => ({ matches: true, media: query, addEventListener() {}, removeEventListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia;
    localStorage.setItem("harness.sidebarCollapsed", "1");
    try {
      await render(props());
      expect(sidebar!.rail).toBe(false);
      const shell = host.querySelector(".shell")!;
      expect(shell.className).not.toContain("collapsed");
      const menu = host.querySelector<HTMLButtonElement>('.topbar button[aria-label="Open sidebar"]')!;
      await act(async () => menu.click());
      expect(shell.className).toContain("drawer-open");
      expect(host.querySelector("main")!.hasAttribute("inert")).toBe(true);
      expect(document.activeElement).toBe(host.querySelector(".sidebar nav a"));
      await act(async () => void window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
      expect(shell.className).not.toContain("drawer-open");
      expect(host.querySelector("main")!.hasAttribute("inert")).toBe(false);
      expect(document.activeElement).toBe(menu);
      // Search from the drawer closes it first.
      await act(async () => menu.click());
      await act(async () => sidebar!.onSearch());
      expect(shell.className).not.toContain("drawer-open");
    } finally {
      window.matchMedia = real;
    }
  });

  test("wide and collapsed: the rail; the dot counts this workspace's unseen routine results", async () => {
    localStorage.setItem("harness.sidebarCollapsed", "1");
    const row = (workspace_id: string, kind: "result" | "failed" | "paused", unseen: number) => ({
      kind,
      routine_id: "r",
      name: "Daily",
      workspace_id,
      run_id: null,
      session_id: null,
      status: "completed",
      reason: null,
      unseen,
      notify: "always" as const,
      ts_ms: 0,
    });
    stubs.attention.mockResolvedValue({
      items: [],
      routines: [row("acme", "result", 2), row("acme", "failed", 1), row("acme", "paused", 0), row("p", "result", 5)],
    } satisfies AttentionResponse);
    await render(props());
    expect(sidebar!.rail).toBe(true);
    expect(host.querySelector(".shell")!.className).toContain("collapsed");
    expect(sidebar!.routineUnseen).toBe(3);
  });
});
