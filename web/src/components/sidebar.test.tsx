// @vitest-environment happy-dom
// The sidebar: the areas in order and only when the viewer has them, the
// open page marked, rows that are real links, the unseen dot, Connections
// as a disabled placeholder, the Projects and Recent sections, the footer's
// account button with its tier line, the collapsed rail, and the account
// menu's chunk loaded on intent.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { ProjectSummary } from "../lib/projects";
import type { Route } from "../lib/routes";
import type { Features, Me, SessionRow, WorkspaceSummary } from "../types";
import type { SidebarProps } from "./Sidebar";

function MenuStub() {
  return <div role="menu" className="menu-stub" />;
}
vi.mock("./AccountMenu", () => ({ default: MenuStub }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const personal: WorkspaceSummary = {
  workspace_id: "w1",
  name: "stored",
  personal: true,
  role: "owner",
  permissions: ["settings", "members", "routines"],
  members: 1,
  exec_level: "host",
};
const acme: WorkspaceSummary = { workspace_id: "w2", name: "Acme", personal: false, role: "admin", permissions: ["settings", "members", "routines"], members: 3, exec_level: "host" as const };
const me = (workspace: WorkspaceSummary | null = personal, role: Me["role"] = "member"): Me => ({
  user_id: "u1",
  username: "alex",
  role,
  exec_ceiling: "host" as const,
  must_change_password: false,
  workspace,
});
const features = (show: boolean): Features => ({
  voice: false,
  memory: false,
  routines: { enabled: true, show, webhooks: false, why: null },
  media: { enabled: true, show, max_file: 0, vision_images: false },
  artifacts: { enabled: true, show, why: null },
  search: { kinds: ["chats", "projects"] },
});
const project = (n: number): ProjectSummary => ({
  project_id: `p${n}`,
  workspace_id: "w1",
  name: `Project ${n}`,
  description: "",
  icon: "folder",
  accent: "green",
  chats: 0,
  updated_ms: n,
});
const chat = (id: string, extra: Partial<SessionRow> = {}): SessionRow => ({
  session_id: id,
  node_id: "master",
  title: `Chat ${id}`,
  cwd: "",
  model: "m",
  created_ms: 0,
  updated_ms: 0,
  head_seq: 1,
  running: false,
  owner_id: "u1",
  agent_id: null,
  profile: "chat",
  incognito: false,
  parent_id: null,
  ...extra,
});

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  localStorage.clear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function render(over: Partial<SidebarProps> = {}, Sidebar?: (p: SidebarProps) => React.ReactNode) {
  const S = Sidebar ?? (await import("./Sidebar")).Sidebar;
  const p: SidebarProps = {
    route: { kind: "home" },
    onNavigate: vi.fn(),
    me: me(),
    displayName: "Alex",
    sessions: [],
    nodes: [],
    conn: "live",
    workspaces: null,
    features: features(false),
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
  act(() => root.render(<S {...p} />));
  return p;
}
const q = <T extends Element = HTMLElement>(sel: string) => host.querySelector<T & HTMLElement>(sel);
const all = (sel: string) => [...host.querySelectorAll<HTMLElement>(sel)];
const navLinks = () => all('nav[aria-label="Main"] a');
const byText = (sel: string, text: string) => all(sel).find((e) => e.textContent?.trim() === text)!;

describe("sidebar", () => {
  test("the areas in order, each only when the viewer has it; Connections is a disabled placeholder", async () => {
    await render();
    expect(q("aside")!.getAttribute("aria-label")).toBe("Sidebar");
    expect(navLinks().map((a) => a.textContent)).toEqual(["New chat", "Agents"]);
    await render({ features: features(true) });
    expect(navLinks().map((a) => [a.textContent, a.getAttribute("href")])).toEqual([
      ["New chat", "#/"],
      ["Agents", "#/agents"],
      ["Routines", "#/routines"],
      ["Media", "#/media"],
      ["Artifacts", "#/artifacts"],
    ]);
    // The end-to-end scripts start fresh chats by this class.
    expect(all(".new-chat").map((a) => a.textContent)).toEqual(["New chat"]);
    const later = q('nav[aria-label="Main"] button')!;
    expect(later.getAttribute("aria-disabled")).toBe("true");
    expect(later.textContent).toBe("ConnectionsLater");
    expect(later.title).toBe("Coming later");
    expect(later.getAttribute("href")).toBeNull();
  });

  test("the open page is aria-current; a plain click navigates in place, a modified one is the browser's", async () => {
    await render({
      route: { kind: "session", id: "b" },
      sessions: [chat("a"), chat("b")],
      projects: [project(1)],
    });
    expect(all('[aria-current="page"]').map((e) => e.textContent)).toEqual(["Chat b"]);
    await render({ route: { kind: "agents", id: "x" } });
    expect(all('[aria-current="page"]').map((e) => e.textContent)).toEqual(["Agents"]);
    const p = await render({ route: { kind: "project", id: "p1" }, projects: [project(1)] });
    expect(all('[aria-current="page"]').map((e) => e.textContent)).toEqual(["Project 1"]);

    const agents = byText('nav[aria-label="Main"] a', "Agents");
    const plain = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0 });
    act(() => void agents.dispatchEvent(plain));
    expect(plain.defaultPrevented).toBe(true);
    expect(p.onNavigate).toHaveBeenLastCalledWith({ kind: "agents", id: null } satisfies Route);
    const calls = vi.mocked(p.onNavigate).mock.calls.length;
    for (const mod of [{ ctrlKey: true }, { metaKey: true }, { shiftKey: true }, { button: 1 }]) {
      const e = new MouseEvent("click", { bubbles: true, cancelable: true, button: 0, ...mod });
      e.preventDefault = vi.fn(); // keep happy-dom from following the link either way
      act(() => void agents.dispatchEvent(e));
      expect(e.preventDefault).not.toHaveBeenCalled();
    }
    expect(vi.mocked(p.onNavigate).mock.calls.length).toBe(calls);
  });

  test("routine results not seen yet put a dot on Routines and say how many", async () => {
    await render({ features: features(true), routineUnseen: 2 });
    let routines = byText('nav[aria-label="Main"] a', "Routines");
    expect(routines.getAttribute("aria-label")).toBe("Routines, 2 new results");
    expect(routines.querySelector(".nav-dot")).not.toBeNull();
    await render({ features: features(true), routineUnseen: 1 });
    routines = byText('nav[aria-label="Main"] a', "Routines");
    expect(routines.getAttribute("aria-label")).toBe("Routines, 1 new result");
    await render({ features: features(true), routineUnseen: 0 });
    routines = byText('nav[aria-label="Main"] a', "Routines");
    expect(routines.getAttribute("aria-label")).toBeNull();
    expect(routines.querySelector(".nav-dot")).toBeNull();
  });

  test("Search is one button with its shortcut; pointing at it loads the panel", async () => {
    const p = await render();
    const search = q('button[aria-label="Search"]')!;
    expect(search.title).toBe("Search (Ctrl K)");
    expect(search.getAttribute("aria-keyshortcuts")).toBe("Control+K Meta+K");
    act(() => void search.dispatchEvent(new MouseEvent("pointerover", { bubbles: true })));
    expect(p.onSearchIntent).toHaveBeenCalled();
    act(() => search.click());
    expect(p.onSearch).toHaveBeenCalledTimes(1);
    expect(q("input")).toBeNull();
  });

  test("Projects and Recent: empty texts, the newest five projects first, then the rest on request", async () => {
    await render();
    expect(all(".section-toggle").map((b) => b.textContent)).toEqual(["Projects", "Recent"]);
    expect(host.textContent).toContain("No projects yet");
    expect(host.textContent).toContain("No chats yet.");
    expect(q('[aria-label="New project"]')).toBeNull();

    const p = await render({ projects: [1, 2, 3, 4, 5, 6, 7].map(project), canCreateProject: true, sessions: [chat("a", { project_id: "p1" }), chat("b")] });
    const rows = () => all(".section-body a.session-item[href^='#/p/']").map((a) => a.textContent);
    expect(rows()).toEqual(["Project 7", "Project 6", "Project 5", "Project 4", "Project 3"]);
    act(() => byText("button", "Show 2 more").click());
    expect(rows()).toHaveLength(7);
    act(() => byText("button", "Show fewer").click());
    expect(rows()).toHaveLength(5);
    act(() => q<HTMLButtonElement>('[aria-label="New project"]')!.click());
    expect(p.onNavigate).toHaveBeenLastCalledWith({ kind: "project", id: "new" });
    // A chat in a project carries its mark.
    const marks = all("a.session-item[href^='#/s/']").map((a) => !!a.querySelector(".chat-project"));
    expect(marks).toEqual([true, false]);
  });

  test("a chat with a background command running says so; a running turn's dot wins", async () => {
    await render({ sessions: [chat("a", { background: 2 }), chat("b", { background: 1, running: true }), chat("c")] });
    const dots = all("a.session-item[href^='#/s/']").map((a) => [...a.querySelectorAll(".dot")].map((d) => d.className));
    expect(dots).toEqual([["dot background"], ["dot running"], []]);
    expect(q(".dot.background")!.getAttribute("title")).toBe("2 background command(s) running");
  });

  test("a section closes and stays closed", async () => {
    await render({ sessions: [chat("a")] });
    const recent = byText(".section-toggle", "Recent");
    expect(recent.getAttribute("aria-expanded")).toBe("true");
    act(() => recent.click());
    expect(recent.getAttribute("aria-expanded")).toBe("false");
    expect(q("a[href^='#/s/']")).toBeNull();
    expect(JSON.parse(localStorage.getItem("harness.sidebarSections")!)).toEqual({ projects: true, recent: false });
    act(() => root.unmount());
    root = createRoot(host);
    await render({ sessions: [chat("a")] });
    expect(byText(".section-toggle", "Recent").getAttribute("aria-expanded")).toBe("false");
  });

  test("the account button: the name, the tier line and a menu, no gear or Log out link", async () => {
    await render({ me: me(acme), displayName: "Alex B" });
    const b = q("button[aria-haspopup='menu']")!;
    expect(b.textContent).toBe("AAlex BAcme · Admin");
    expect(b.getAttribute("aria-label")).toBe("Alex B (@alex) — Acme · Admin");
    expect(b.title).toBe(b.getAttribute("aria-label"));
    expect(b.getAttribute("aria-expanded")).toBe("false");
    expect(host.textContent).not.toContain("Log out");
    expect(q('[title="Settings"]')).toBeNull();
    await render({ me: me(personal, "admin") });
    expect(q("button[aria-haspopup='menu']")!.textContent).toBe("AAlexPersonal · Server admin");
  });

  test("collapsed: a rail of labelled icons, the avatar saying who and where", async () => {
    const p = await render({ rail: true, features: features(true), routineUnseen: 3, me: me(acme), sessions: [chat("a")], projects: [project(1)] });
    expect(q("aside")!.className).toContain("rail");
    const labelled = all("aside button, aside a").map((e) => [e.getAttribute("aria-label"), e.title]);
    expect(labelled).toEqual([
      ["Expand sidebar", "Expand sidebar"],
      ["New chat", "New chat"],
      ["Search", "Search (Ctrl K)"],
      ["Agents", "Agents"],
      ["Routines, 3 new results", "Routines"],
      ["Media", "Media"],
      ["Artifacts", "Artifacts"],
      ["Connections", "Coming later"],
      ["Alex — Acme · Admin", "Alex — Acme · Admin"],
    ]);
    // No Projects or Recent in the rail.
    expect(q(".section-toggle")).toBeNull();
    expect(q("a[href^='#/s/']")).toBeNull();
    expect(q(".rail-avatar .workspace-badge")!.textContent).toBe("A");
    act(() => q<HTMLButtonElement>('[aria-label="Expand sidebar"]')!.click());
    expect(p.onCollapse).toHaveBeenCalled();
    // Personal: no badge, the server role in the label.
    await render({ rail: true, me: me(personal) });
    expect(q(".rail-avatar")!.getAttribute("aria-label")).toBe("Alex — Personal · User");
    expect(q(".rail-avatar .workspace-badge")).toBeNull();
  });
});

describe("the account menu's chunk", () => {
  for (const [what, fire] of [
    ["pointerenter", (b: HTMLElement) => b.dispatchEvent(new MouseEvent("pointerover", { bubbles: true }))],
    ["focus", (b: HTMLElement) => b.focus()],
    ["touchstart", (b: HTMLElement) => b.dispatchEvent(new Event("touchstart", { bubbles: true }))],
  ] as const) {
    test(`loads on ${what} of the button, before any click`, async () => {
      // A fresh module graph, so the chunk's import is counted from here.
      vi.resetModules();
      let menuLoads = 0;
      vi.doMock("./AccountMenu", () => {
        menuLoads++;
        return { default: MenuStub };
      });
      const { Sidebar } = await import("./Sidebar");
      await render({}, Sidebar);
      expect(menuLoads).toBe(0);
      const b = q("button[aria-haspopup='menu']")!;
      await act(async () => void fire(b));
      await vi.dynamicImportSettled();
      expect(menuLoads).toBe(1);
      expect(q('[role="menu"]')).toBeNull();
      await act(async () => b.click());
      expect(b.getAttribute("aria-expanded")).toBe("true");
      expect(b.getAttribute("aria-controls")).toBeTruthy();
      expect(q('[role="menu"]')).not.toBeNull();
    });
  }
});
