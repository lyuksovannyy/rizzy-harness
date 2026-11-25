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
import type { ChatPages } from "../lib/useChatList";

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
    const p1 = byLabel("Collapse Project 1");
    expect(p1.getAttribute("aria-expanded")).toBe("true");
    act(() => p1.click());
    expect(p1.getAttribute("aria-expanded")).toBe("false");
    // The chevron says what it will do, not only the project's name (the link beside it).
    expect(p1.getAttribute("aria-label")).toBe("Expand Project 1");
    expect(hrefs()).toEqual(["#/p/p2", "#/s/b", "#/p/p1", "#/s/c", "#/s/e", "#/s/d"]);
    remount(scene());
    expect(byLabel("Expand Project 1").getAttribute("aria-expanded")).toBe("false");
    expect(byLabel("Collapse Project 2").getAttribute("aria-expanded")).toBe("true");
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
    expect(byLabel("Collapse Project 1").getAttribute("aria-expanded")).toBe("true");
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

  // React escapes text and attributes, so markup was text on main as well:
  // this proves the escaping (it passes on main too). What changed is the
  // hidden characters: a right-to-left override in a project name, a title or
  // a hostname shows as its code in the text, the labels and the titles.
  test("names with markup and a right-to-left override are text, the override shown as its code", () => {
    const evil = '<img src=x onerror="alert(1)">\u202Eexe.gpj';
    const shown = '<img src=x onerror="alert(1)"><U+202E>exe.gpj';
    render({
      projects: [project(1, evil)],
      nodes: [node("n1", evil)],
      sessions: [chat("a", { title: evil, project_id: "p1" }), chat("b", { title: `<b>${evil}</b>`, profile: "agentic", node_id: "n1" })],
    });
    expect(q(".sidebar-scroll img")).toBeNull();
    expect(q(".sidebar-scroll b")).toBeNull();
    expect(host.innerHTML).not.toContain("\u202E");
    expect(q('a[href="#/p/p1"]')!.textContent).toBe(shown);
    expect(q('a[href="#/s/a"]')!.textContent).toBe(shown);
    const labels = all(".sidebar-scroll [aria-label], .sidebar-scroll [title]").flatMap((e) => [e.getAttribute("aria-label"), e.getAttribute("title")]);
    expect(labels).toContain(`New session in ${shown}`);
    expect(labels).toContain(`Collapse ${shown}`);
    expect(labels).toContain(`Runs on ${shown}`);
    expect(q(".node-mark")!.getAttribute("style")).toMatch(/^--node: var\(--ac-[a-z]+-2\);?$/);
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

// The review of PR 132 (r-sidebar, P1 to P13): odd data, scale, and the open
// chat never lost behind a cap or a closed group.
const current = () => q('.sidebar-scroll a[aria-current="page"]');
const open = (id: string) => ({ kind: "session" as const, id });

describe("odd data and scale", () => {
  test("incognito: a hidden parent hides its child; an incognito child or project chat is not counted in Show N more", () => {
    render({
      projects: [project(1)],
      sessions: [
        chat("ip", { incognito: true, title: "SECRET parent" }),
        chat("ic", { parent_id: "ip", title: "SECRET child of incognito" }),
        chat("n", { title: "normal" }),
        chat("k1", { parent_id: "n" }),
        chat("k2", { parent_id: "n" }),
        chat("k3", { parent_id: "n" }),
        chat("ki", { parent_id: "n", incognito: true, title: "SECRET incognito kid" }),
        ...[1, 2, 3, 4, 5].map((i) => chat(`pc${i}`, { project_id: "p1" })),
        chat("pi", { project_id: "p1", incognito: true, title: "SECRET in project" }),
      ],
    });
    expect(host.innerHTML).not.toContain("SECRET");
    expect(host.textContent).not.toMatch(/Show \d+ more/);
  });

  test("a cycle and a self-parent do not hang and are not shown, even when one is open", () => {
    const t0 = performance.now();
    render({ sessions: [chat("A", { parent_id: "B" }), chat("B", { parent_id: "A" }), chat("S", { parent_id: "S" }), chat("top")], route: open("A") });
    expect(performance.now() - t0).toBeLessThan(2000);
    expect(hrefs()).toEqual(["#/s/top"]);
  });

  test("depth over one nests and stays capped at three per level", () => {
    render({ sessions: [chat("r"), chat("c1", { parent_id: "r" }), ...[1, 2, 3, 4].map((i) => chat(`g${i}`, { parent_id: "c1" }))] });
    expect(all("ul.chats.sub ul.chats.sub > li > a")).toHaveLength(3);
    expect(host.textContent).toContain("Show 1 more");
  });

  test("a child claiming another project shows once, under its parent", () => {
    render({ projects: [project(1), project(2)], sessions: [chat("par", { project_id: "p1" }), chat("kid", { parent_id: "par", project_id: "p2" })] });
    expect(hrefs().filter((h) => h === "#/s/kid")).toHaveLength(1);
    expect(q("li.project-box:has(a[href='#/s/par']) a[href='#/s/kid']")).not.toBeNull();
  });

  test("500 chats, 50 projects, 200 children of one parent render and re-render quickly", () => {
    const projects = Array.from({ length: 50 }, (_, i) => project(i + 1));
    const sessions = [
      chat("big"),
      ...Array.from({ length: 200 }, (_, i) => chat(`k${i}`, { parent_id: "big" })),
      ...Array.from({ length: 300 }, (_, i) => chat(`s${i}`, { project_id: i % 2 ? `p${(i % 50) + 1}` : null })),
    ];
    render({ projects, sessions });
    act(() => all("button").find((b) => b.textContent === "Show 45 more")!.click());
    const t1 = performance.now();
    for (let i = 0; i < 10; i++) render({ projects, sessions: [...sessions] });
    expect(performance.now() - t1).toBeLessThan(5000);
  });

  test("an old stored recent:false is ignored; a stored array or a __proto__ project id do not break it", () => {
    localStorage.setItem("harness.sidebarSections", JSON.stringify({ projects: true, recent: false }));
    render({ sessions: [chat("a")] });
    expect(hrefs()).toEqual(["#/s/a"]);
    localStorage.setItem("harness.sidebarSections", "[false]");
    const odd = { projects: [{ ...project(1), project_id: "__proto__" }], sessions: [chat("b", { project_id: "__proto__" })] };
    remount(odd);
    act(() => q<HTMLButtonElement>(".project-box .section-toggle")!.click());
    remount(odd);
    expect(q(".project-box .section-toggle")!.getAttribute("aria-expanded")).toBe("false");
  });
});

describe("the open chat stays findable", () => {
  test("behind a project's Show N more: shown in its place, the count stays true", () => {
    const sessions = Array.from({ length: 7 }, (_, i) => chat(`c${i}`, { project_id: "p1" }));
    render({ projects: [project(1)], sessions, route: open("c6") });
    expect(current()!.getAttribute("href")).toBe("#/s/c6");
    expect(hrefs(".project-box a[href^='#/s/']")).toEqual(["#/s/c0", "#/s/c1", "#/s/c2", "#/s/c3", "#/s/c4", "#/s/c6"]);
    expect(host.textContent).toContain("Show 1 more");
    act(() => all("button").find((b) => b.textContent === "Show 1 more")!.click());
    expect(hrefs(".project-box a[href^='#/s/']")).toHaveLength(7);
  });

  test("in a sixth project: that project is shown in its place", () => {
    const projects = Array.from({ length: 7 }, (_, i) => project(i + 1));
    render({ projects, sessions: [chat("old", { project_id: "p1" })], route: open("old") });
    expect(current()!.getAttribute("href")).toBe("#/s/old");
    expect(all(".project-box > .section-row a").map((a) => a.textContent)).toEqual(["Project 7", "Project 6", "Project 5", "Project 4", "Project 3", "Project 1"]);
    expect(host.textContent).toContain("Show 1 more");
  });

  test("a fourth sub-agent, and a grandchild behind two caps, are shown with their parents", () => {
    render({ sessions: [chat("r"), ...[1, 2, 3, 4].map((i) => chat(`k${i}`, { parent_id: "r" }))], route: open("k4") });
    expect(current()!.getAttribute("href")).toBe("#/s/k4");
    expect(host.textContent).not.toMatch(/Show \d+ more/);
    remount({
      sessions: [chat("r"), ...[1, 2, 3, 4].map((i) => chat(`k${i}`, { parent_id: "r" })), ...[1, 2, 3, 4, 5].map((i) => chat(`g${i}`, { parent_id: "k4" }))],
      route: open("g5"),
    });
    expect(current()!.getAttribute("href")).toBe("#/s/g5");
    expect(hrefs("ul.chats.sub ul.chats.sub a")).toEqual(["#/s/g1", "#/s/g2", "#/s/g3", "#/s/g5"]);
  });

  test("a closed project stays closed; its header carries the open chat's mark and the dots of what is inside", () => {
    localStorage.setItem("harness.sidebarSections", JSON.stringify({ p1: false, p2: false }));
    render({
      projects: [project(1), project(2)],
      sessions: [chat("a", { project_id: "p1" }), chat("u", { project_id: "p2", unread: true }), chat("r", { project_id: "p2" }), chat("rk", { parent_id: "r", running: true })],
      route: open("a"),
    });
    const box = (n: number) => q(`li.project-box:has(a[href='#/p/p${n}'])`)!;
    expect(box(1).querySelector(".section-toggle")!.getAttribute("aria-expanded")).toBe("false");
    expect(current()).toBeNull();
    expect(box(1).querySelector("a[href='#/p/p1']")!.className).toContain("active");
    expect(box(1).querySelector(".dot.here")!.getAttribute("aria-label")).toBe("The open chat is in here");
    expect(box(2).querySelector("a[href='#/p/p2']")!.className).not.toContain("active");
    // A running sub-agent wins over the unread one, as on a row; both say so.
    expect(box(2).querySelector(".dot.running")!.getAttribute("aria-label")).toBe("A chat in here is running");
    expect(box(2).querySelector(".dot.unseen")).toBeNull();
    remount({ projects: [project(2)], sessions: [chat("u", { project_id: "p2", unread: true })] });
    expect(box(2).querySelector(".dot.unseen")!.getAttribute("aria-label")).toBe("Finished; not seen yet");
    // Open, the rows carry the marks; the header does not repeat them.
    act(() => box(2).querySelector<HTMLButtonElement>(".section-toggle")!.click());
    expect(box(2).querySelector(".section-row .dot")).toBeNull();
  });

  test("Projects closed stays closed; its header carries the marks of every project's chats", () => {
    localStorage.setItem("harness.sidebarSections", JSON.stringify({ projects: false }));
    render({ projects: [project(1)], sessions: [chat("a", { project_id: "p1" }), chat("b", { project_id: "p1", unread: true }), chat("loose")], route: open("a") });
    const head = q(".sidebar-scroll .side-section > .section-row")!;
    expect(head.querySelector(".section-toggle")!.getAttribute("aria-expanded")).toBe("false");
    expect(q(".project-box")).toBeNull();
    expect(head.className).toContain("active");
    expect(head.querySelector(".dot.here")!.getAttribute("aria-label")).toBe("The open chat is in here");
    expect(head.querySelector(".dot.unseen")!.getAttribute("aria-label")).toBe("Finished; not seen yet");
    expect(hrefs()).toEqual(["#/s/loose"]);
  });

  test("+ on the project whose page is open focuses its composer", async () => {
    const page = document.createElement("section");
    page.className = "project-page";
    page.innerHTML = '<div class="composer"><textarea></textarea></div>';
    document.body.append(page);
    try {
      const p = render({ projects: [project(1)], route: { kind: "project", id: "p1" } });
      act(() => byLabel("New session in Project 1").click());
      await act(() => new Promise((r) => setTimeout(r, 50)));
      expect(p.onNavigate).toHaveBeenLastCalledWith({ kind: "project", id: "p1" });
      expect(document.activeElement).toBe(page.querySelector("textarea"));
    } finally {
      page.remove();
    }
  });
});

describe("the paged list (FIX_PLAN 4.5)", () => {
  const pages = (over: Partial<ChatPages> = {}): ChatPages => ({
    more: vi.fn(),
    busy: false,
    stalled: false,
    watch: vi.fn(() => vi.fn()),
    group: () => undefined,
    moreIn: vi.fn(),
    ...over,
  });

  test("an open project group asks for its own chats; a closed one costs nothing", () => {
    const p = pages();
    render({ ...scene(), pages: p });
    expect(p.watch).toHaveBeenCalledWith("p1");
    expect(p.watch).toHaveBeenCalledWith("p2");
    const w = p.watch as ReturnType<typeof vi.fn>;
    const release = (id: string) => w.mock.results[w.mock.calls.findIndex((c) => c[0] === id)]!.value as ReturnType<typeof vi.fn>;
    act(() => byLabel("Collapse Project 2").click());
    expect(release("p1")).not.toHaveBeenCalled();
    expect(release("p2")).toHaveBeenCalledTimes(1);
    // Closed in this browser: no request for it next time.
    const again = pages();
    remount({ ...scene(), pages: again });
    expect(again.watch).toHaveBeenCalledWith("p1");
    expect(again.watch).not.toHaveBeenCalledWith("p2");
  });

  test("a group with more on the server offers its next page once every loaded row shows", () => {
    const rows = Array.from({ length: 7 }, (_, i) => chat(`g${i}`, { project_id: "p1", updated_ms: 100 - i }));
    const own = { rows, children: [], next: "cursor" };
    const p = pages({ group: (id) => (id === "p1" ? own : undefined) });
    render({ projects: [project(1)], sessions: rows, pages: p });
    const box = () => q(".project-box")!;
    const button = (label: string) => [...box().querySelectorAll("button")].find((b) => b.textContent === label);
    expect(button("Show 2 more")).toBeTruthy();
    act(() => button("Show 2 more")!.click());
    act(() => button("Show more")!.click());
    expect(p.moreIn).toHaveBeenCalledWith("p1");
  });

  test("the end loads the next page when it scrolls into view, but not after a failed load", () => {
    const seen: Element[] = [];
    let report: ((es: { isIntersecting: boolean }[]) => void) | null = null;
    class IO {
      constructor(cb: (es: { isIntersecting: boolean }[]) => void) {
        report = cb;
      }
      observe(el: Element) {
        seen.push(el);
      }
      disconnect() {}
    }
    vi.stubGlobal("IntersectionObserver", IO);
    try {
      const p = pages();
      render({ ...scene(), pages: p });
      expect(seen).toEqual([q(".load-more")]);
      act(() => report!([{ isIntersecting: true }]));
      expect(p.more).toHaveBeenCalledTimes(1);
      seen.length = 0;
      remount({ ...scene(), pages: pages({ stalled: true }) });
      expect(seen).toEqual([]);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  test("the end of the list is a button that loads older chats, disabled while it loads", () => {
    const p = pages();
    render({ ...scene(), pages: p });
    const more = q<HTMLButtonElement>(".load-more")!;
    expect(more.textContent).toBe("Load older chats");
    act(() => more.click());
    expect(p.more).toHaveBeenCalled();
    render({ ...scene(), pages: pages({ busy: true }) });
    expect(q<HTMLButtonElement>(".load-more")!.disabled).toBe(true);
    render({ ...scene(), pages: pages({ more: null }) });
    expect(q(".load-more")).toBeNull();
  });
});
