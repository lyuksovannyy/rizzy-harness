// @vitest-environment happy-dom
// The sidebar's lists (the owner's sketch of 2026-11-16, docs/sidebar-search.md):
// the header's two-part button, Projects as boxed groups holding their chats,
// the chats in no project below, sub-agent chats as a bounded tree under their
// parent, the collapse state kept per browser, and the node badge of agentic
// chats (its colour from the node id, its name on hover and focus).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, test, vi } from "vitest";
import type { ProjectSummary } from "../lib/projects";
import type { Features, Me, NodeInfo, SessionRow } from "../types";
import type { SidebarProps } from "./Sidebar";

vi.mock("./AccountMenu", () => ({ default: () => null }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let Sidebar: (p: SidebarProps) => React.ReactNode;
let nodeHue: (id: string) => string;
beforeAll(async () => {
  ({ Sidebar } = await import("./Sidebar"));
  ({ nodeHue } = await import("./ProjectNav"));
}, 30_000);

const me: Me = { user_id: "u1", username: "alex", role: "member", exec_ceiling: "host", must_change_password: false, workspace: null };
const features: Features = {
  voice: false,
  memory: false,
  routines: { enabled: true, show: false, webhooks: false, why: null },
  media: { enabled: true, show: false, max_file: 0, vision_images: false },
  artifacts: { enabled: true, show: false, why: null },
  search: { kinds: ["chats"] },
};
const project = (n: number, name = `Project ${n}`): ProjectSummary => ({
  project_id: `p${n}`,
  workspace_id: "w1",
  name,
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
const node = (id: string, hostname: string, extra: Partial<NodeInfo> = {}): NodeInfo => ({
  node_id: id,
  hostname,
  version: "1",
  connected: true,
  embedded: false,
  profile: "agentic",
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

function render(over: Partial<SidebarProps> = {}) {
  const p: SidebarProps = {
    route: { kind: "home" },
    onNavigate: vi.fn(),
    me,
    displayName: "Alex",
    sessions: [],
    nodes: [],
    conn: "live",
    workspaces: null,
    features,
    routineUnseen: 0,
    onSwitch: vi.fn().mockResolvedValue(undefined),
    onLogout: vi.fn(),
    onSearch: vi.fn(),
    onSearchIntent: vi.fn(),
    onCollapse: vi.fn(),
    projects: [],
    canCreateProject: true,
    ...over,
  };
  act(() => root.render(<Sidebar {...p} />));
  return p;
}
function remount(over: Partial<SidebarProps> = {}) {
  act(() => root.unmount());
  root = createRoot(host);
  return render(over);
}
const q = <T extends HTMLElement = HTMLElement>(sel: string) => host.querySelector<T>(sel);
const all = (sel: string) => [...host.querySelectorAll<HTMLElement>(sel)];
const hrefs = (sel = ".sidebar-scroll a") => all(sel).map((a) => a.getAttribute("href"));
const byLabel = (label: string) => q<HTMLButtonElement>(`[aria-label="${label}"]`)!;

const scene = (): Partial<SidebarProps> => ({
  projects: [project(1), project(2)],
  sessions: [
    chat("a", { project_id: "p1" }),
    chat("b", { project_id: "p2" }),
    chat("c"),
    chat("e", { parent_id: "c", running: true }),
    chat("d"),
    // Incognito chats stay out of the sidebar, as before.
    chat("x", { incognito: true }),
  ],
});

describe("the sidebar's lists", () => {
  test("in order: the header with its two-part button, the areas, Projects with boxed projects, then the loose chats", () => {
    render(scene());
    const aside = q("aside")!;
    const parts = [".sidebar-head", 'nav[aria-label="Main"]', ".sidebar-scroll"].map((s) => aside.querySelector(s)!);
    expect(parts.every(Boolean)).toBe(true);
    expect(parts[0]!.compareDocumentPosition(parts[1]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(parts[1]!.compareDocumentPosition(parts[2]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // The header: the name, then Search and Collapse as one two-part control.
    expect(q(".sidebar-head .brand")!.textContent).toBe("rizzy harness");
    const pair = q('.sidebar-head [role="group"]')!;
    expect([...pair.querySelectorAll("button")].map((b) => b.getAttribute("aria-label"))).toEqual(["Search", "Collapse sidebar"]);
    // The section header, then each project's box (newest first) with its chats, then the rest.
    expect(q(".sidebar-scroll .section-toggle")!.textContent).toBe("Projects");
    expect(all(".project-box").map((b) => [...b.querySelectorAll("a")].map((a) => a.getAttribute("href")))).toEqual([
      ["#/p/p2", "#/s/b"],
      ["#/p/p1", "#/s/a"],
    ]);
    expect(hrefs()).toEqual(["#/p/p2", "#/s/b", "#/p/p1", "#/s/a", "#/s/c", "#/s/e", "#/s/d"]);
    // Lists are lists.
    expect(q(".project-box")!.tagName).toBe("LI");
    expect(q('a[href="#/s/c"]')!.closest("li")!.parentElement!.tagName).toBe("UL");
  });

  test("the section and one project collapse, and stay so after a remount", () => {
    render(scene());
    const section = q<HTMLButtonElement>(".sidebar-scroll .section-toggle")!;
    const p1 = byLabel("Project 1");
    expect(p1.getAttribute("aria-expanded")).toBe("true");
    act(() => p1.click());
    expect(p1.getAttribute("aria-expanded")).toBe("false");
    expect(hrefs()).toEqual(["#/p/p2", "#/s/b", "#/p/p1", "#/s/c", "#/s/e", "#/s/d"]);
    remount(scene());
    expect(byLabel("Project 1").getAttribute("aria-expanded")).toBe("false");
    expect(byLabel("Project 2").getAttribute("aria-expanded")).toBe("true");
    act(() => q<HTMLButtonElement>(".sidebar-scroll .section-toggle")!.click());
    expect(section.isConnected).toBe(false); // remounted: a new button
    expect(q(".sidebar-scroll .section-toggle")!.getAttribute("aria-expanded")).toBe("false");
    // Closed: the projects go, the chats in no project stay.
    expect(hrefs()).toEqual(["#/s/c", "#/s/e", "#/s/d"]);
    remount(scene());
    expect(q(".sidebar-scroll .section-toggle")!.getAttribute("aria-expanded")).toBe("false");
    expect(q('[aria-label="New project"]')).not.toBeNull();
  });

  test("unreadable stored state is ignored", () => {
    localStorage.setItem("harness.sidebarSections", "{not json");
    render(scene());
    expect(byLabel("Project 1").getAttribute("aria-expanded")).toBe("true");
  });

  test("+ on the section makes a project; + on a project opens its page to start a chat there", () => {
    const p = render(scene());
    act(() => byLabel("New project").click());
    expect(p.onNavigate).toHaveBeenLastCalledWith({ kind: "project", id: "new" });
    act(() => byLabel("New session in Project 1").click());
    expect(p.onNavigate).toHaveBeenLastCalledWith({ kind: "project", id: "p1" });
    // No right to make projects: no section +, the project + stays.
    remount({ ...scene(), canCreateProject: false });
    expect(q('[aria-label="New project"]')).toBeNull();
    expect(q('[aria-label="New session in Project 1"]')).not.toBeNull();
  });

  test("sub-agent chats nest under their parent, the newest three first, the rest on request", () => {
    const kids = [1, 2, 3, 4, 5].map((n) => chat(`k${n}`, { parent_id: "c", running: n === 1 }));
    render({ sessions: [chat("c"), ...kids, chat("d"), chat("o", { parent_id: "gone" })] });
    const parent = q('a[href="#/s/c"]')!.closest("li")!;
    const sub = parent.querySelector("ul.sub")!;
    const shown = () => [...sub.querySelectorAll(":scope > li > a")].map((a) => a.getAttribute("href"));
    expect(shown()).toEqual(["#/s/k1", "#/s/k2", "#/s/k3"]);
    // A running child shows it.
    expect(q('a[href="#/s/k1"] .dot.running')).not.toBeNull();
    const more = [...sub.querySelectorAll("button")].find((b) => b.textContent === "Show 2 more")!;
    act(() => more.click());
    expect(shown()).toHaveLength(5);
    // A child whose parent is not listed is not shown loose.
    expect(q('a[href="#/s/o"]')).toBeNull();
    // The top level is not cut.
    expect(hrefs(".sidebar-scroll > ul > li > a")).toEqual(["#/s/c", "#/s/d"]);
  });

  test("the badge: agentic chats only, the node's name on hover and to assistive tech, a colour per node", () => {
    render({
      nodes: [node("n-alpha", "alpha"), node("n-beta", "beta", { connected: false })],
      sessions: [
        chat("a", { profile: "agentic", node_id: "n-alpha" }),
        chat("b", { profile: "agentic", node_id: "n-alpha" }),
        chat("c", { profile: "agentic", node_id: "n-beta" }),
        chat("d"),
        chat("e", { profile: "agentic", node_id: "n-unknown" }),
      ],
    });
    const mark = (id: string) => q(`a[href="#/s/${id}"] .node-mark`);
    expect(mark("d")).toBeNull();
    expect(mark("a")!.getAttribute("aria-label")).toBe("Runs on alpha");
    expect(mark("a")!.getAttribute("role")).toBe("img");
    expect(mark("a")!.title).toBe("Runs on alpha");
    expect(mark("c")!.getAttribute("aria-label")).toBe("Runs on beta, offline");
    expect(mark("e")!.getAttribute("aria-label")).toBe("Runs on n-unknown, offline");
    // The row's name carries it, so focus reaches it.
    expect(q('a[href="#/s/a"]')!.textContent).toBe("Chat a");
    const colour = (id: string) => mark(id)!.style.getPropertyValue("--node");
    expect(colour("a")).toBe(colour("b"));
    expect(colour("a")).toBe(nodeHue("n-alpha"));
    expect(colour("a")).toMatch(/^var\(--ac-[a-z]+-2\)$/);
  });

  test("the node colour is stable per id and spreads over a small set", () => {
    expect(nodeHue("master")).toBe(nodeHue("master"));
    expect(nodeHue("n-alpha")).not.toBe(nodeHue("n-beta"));
    const ids = ["master", "n-alpha", "n-beta", "laptop", "build-box", "gpu-1"];
    expect(new Set(ids.map(nodeHue)).size).toBeGreaterThanOrEqual(4);
  });

  test("titles and project names with HTML in them are text", () => {
    const evil = '<img src=x onerror="alert(1)">';
    render({ projects: [project(1, evil)], sessions: [chat("a", { title: evil, project_id: "p1" }), chat("b", { title: `<b>${evil}</b>` })] });
    expect(q(".sidebar-scroll img")).toBeNull();
    expect(q(".sidebar-scroll b")).toBeNull();
    expect(q('a[href="#/p/p1"]')!.textContent).toBe(evil);
    expect(q('a[href="#/s/a"]')!.textContent).toBe(evil);
    expect(all("button").some((b) => b.getAttribute("aria-label") === `New session in ${evil}`)).toBe(true);
    expect(all("button").some((b) => b.getAttribute("aria-label") === evil && b.hasAttribute("aria-expanded"))).toBe(true);
  });

  test("empty states: no projects, a project with no chats, no chats at all", () => {
    render();
    expect(q(".sidebar-scroll .section-toggle")!.textContent).toBe("Projects");
    expect(q('[aria-label="New project"]')).not.toBeNull();
    expect(host.textContent).toContain("No projects yet");
    expect(host.textContent).toContain("No chats yet.");
    remount({ projects: [project(1)], sessions: [chat("a")] });
    expect(q(".project-box")!.textContent).toContain("No chats yet.");
    expect(host.textContent).not.toContain("No projects yet");
  });

  test("more than five projects: the newest five, then the rest on request; a project's chats are bounded too", () => {
    const chats = [1, 2, 3, 4, 5, 6, 7].map((n) => chat(`c${n}`, { project_id: "p7" }));
    render({ projects: [1, 2, 3, 4, 5, 6, 7].map((n) => project(n)), sessions: chats });
    const projects = () => all(".project-box > .section-row a").map((a) => a.textContent);
    expect(projects()).toEqual(["Project 7", "Project 6", "Project 5", "Project 4", "Project 3"]);
    const box = q(".project-box")!;
    expect(box.querySelectorAll("a[href^='#/s/']")).toHaveLength(5);
    act(() => [...box.querySelectorAll("button")].find((b) => b.textContent === "Show 2 more")!.click());
    expect(box.querySelectorAll("a[href^='#/s/']")).toHaveLength(7);
    act(() => all("button").find((b) => b.textContent === "Show 2 more")!.click());
    expect(projects()).toHaveLength(7);
  });
});
