// @vitest-environment happy-dom
// Projects in the shell: the sidebar section above the chats, the bar on a
// project's page (no Search, no Incognito, the project's folder), and the
// chat header's project picker.
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test, vi } from "vitest";
import type { ProjectDetail, ProjectSummary } from "../lib/projects";
import type { Me, SessionRow, WorkspaceSummary } from "../types";
import { Composer } from "./Composer";
import { canJoinProject, ProjectChip } from "./ProjectChip";
import { Sidebar } from "./Sidebar";

const get = vi.fn<(id: string) => Promise<ProjectDetail>>();
vi.mock("../lib/projects", async (original) => {
  const m = await original<typeof import("../lib/projects")>();
  return { ...m, projectsApi: { ...m.projectsApi, get: (id: string) => get(id) } };
});
const { default: Project } = await import("./Project");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function mount(node: ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() => root.render(node));
  return { host, unmount: () => act(() => root.unmount()) };
}

const personal: WorkspaceSummary = {
  workspace_id: "w1",
  name: "Personal",
  personal: true,
  role: "owner",
  permissions: ["agentic", "settings", "members", "routines"],
  members: 1,
};
const me: Me = { user_id: "u1", username: "alex", role: "member", can_agentic: false, must_change_password: false, workspace: personal };

const garden: ProjectSummary = {
  project_id: "p1",
  workspace_id: "w1",
  name: "Garden",
  description: "Our allotment",
  icon: "house",
  accent: "green",
  chats: 1,
  updated_ms: 0,
};

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

function sidebar(p: { projects: ProjectSummary[]; canCreate: boolean; sessions?: SessionRow[]; onProject?: (id: string) => void }) {
  return mount(
    <Sidebar
      me={me}
      sessions={p.sessions ?? []}
      nodes={[]}
      selected={null}
      conn="live"
      workspaces={{ workspaces: [personal], active: "w1", can_create: true }}
      onSwitch={async () => {}}
      onWorkspaceSettings={() => {}}
      onSelect={() => {}}
      onNew={() => {}}
      onAgents={() => {}}
      agentsOpen={false}
      onSettings={() => {}}
      onLogout={() => {}}
      onCollapse={() => {}}
      projects={p.projects}
      canCreateProject={p.canCreate}
      selectedProject="p1"
      onProject={p.onProject}
    />,
  );
}

describe("sidebar", () => {
  test("projects sit below the switcher and Agents, above the chats; + opens the create form", () => {
    const opened: string[] = [];
    const s = sidebar({
      projects: [garden],
      canCreate: true,
      sessions: [chat("a", { project_id: "p1" }), chat("b")],
      onProject: (id) => opened.push(id),
    });
    const labels = [...s.host.querySelectorAll(".section-label")].map((l) => l.textContent);
    expect(labels).toEqual(["Projects", "Recents"]);
    const order = [".workspace-switch", ".nav-item:not(.new-chat):not(.user)", ".project-list", ".session-list"].map(
      (sel) => s.host.querySelector(sel)!,
    );
    expect(order[1]!.textContent?.trim()).toBe("Agents");
    for (let i = 1; i < order.length; i++) {
      expect(order[i - 1]!.compareDocumentPosition(order[i]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }
    const item = s.host.querySelector<HTMLButtonElement>(".project-list .session-item")!;
    expect(item.textContent).toBe("Garden");
    expect(item.className).toContain("active");
    act(() => item.click());
    act(() => s.host.querySelector<HTMLButtonElement>('[aria-label="New project"]')!.click());
    expect(opened).toEqual(["p1", "new"]);
    // A chat in a project carries its mark; one outside does not.
    const marks = [...s.host.querySelectorAll(".session-list .session-item")].map((b) => !!b.querySelector(".chat-project"));
    expect(marks).toEqual([true, false]);
    s.unmount();
  });

  test("no projects and no right to create: no section at all", () => {
    const s = sidebar({ projects: [], canCreate: false });
    expect([...s.host.querySelectorAll(".section-label")].map((l) => l.textContent)).toEqual(["Recents"]);
    s.unmount();
  });
});

describe("the bar on a project's page", () => {
  test("no Search mode and no Incognito; the project's folder is filled in", () => {
    const c = mount(
      <Composer
        where="home"
        agents={[]}
        models={[]}
        workers={[{ node_id: "n1", hostname: "box", version: "1", connected: true, embedded: false, profile: "agentic" }]}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        onSubmit={async () => {}}
        project={{ nodeId: "n1", cwd: "/srv/garden" }}
      />,
    );
    const modes = [...c.host.querySelectorAll(".mode-switch button")].map((b) => b.textContent);
    expect(modes).toEqual(["Ask", "Agentic"]);
    expect(c.host.querySelector(".pill.incognito")).toBeNull();
    act(() => [...c.host.querySelectorAll<HTMLButtonElement>(".mode-switch button")].find((b) => b.textContent === "Agentic")!.click());
    const folder = [...c.host.querySelectorAll<HTMLInputElement>("input")].find((i) => i.value === "/srv/garden");
    expect(folder).toBeTruthy();
    c.unmount();
  });
});

describe("project picker", () => {
  test("temporary chats and sub-tasks cannot join", () => {
    expect(canJoinProject(chat("a"))).toBe(true);
    expect(canJoinProject(chat("a", { incognito: true }))).toBe(false);
    expect(canJoinProject(chat("a", { parent_id: "x" }))).toBe(false);
    expect(canJoinProject(null)).toBe(false);
  });

  test("shows the chat's project, or nothing when there are no projects", () => {
    const inside = mount(<ProjectChip row={chat("a", { project_id: "p1" })} projects={[garden]} />);
    expect(inside.host.querySelector(".project-chip")!.textContent).toContain("Garden");
    expect(inside.host.querySelector<HTMLSelectElement>("select")!.value).toBe("p1");
    inside.unmount();
    const none = mount(<ProjectChip row={chat("a")} projects={[]} />);
    expect(none.host.querySelector(".project-chip")).toBeNull();
    none.unmount();
  });
});

describe("project page", () => {
  test("a link to a project of another of your workspaces switches there, once", async () => {
    get.mockResolvedValue({
      ...garden,
      workspace_id: "w2",
      instructions: "",
      settings: { workspace_memory: true, node_id: null, cwd: null },
      can_manage: true,
      created_by: null,
      created_ms: 0,
    });
    const switched: string[] = [];
    const page = (n: number) => (
      <Project
        id="p1"
        version={n}
        memoryVersion={0}
        chats={[]}
        agents={[]}
        defaultAgent=""
        models={[]}
        workers={[]}
        nodes={[]}
        prefs={null}
        agenticBlocked={null}
        onSubmit={async () => {}}
        onOpenChat={() => {}}
        onOpenProject={() => {}}
        onDeleted={() => {}}
        workspaceId="w1"
        // A new callback on every render, as Shell passes it.
        onForeign={(w) => switched.push(w)}
      />
    );
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(page(0)));
    await act(async () => root.render(page(0)));
    expect(host.textContent).toContain("Garden");
    expect(switched).toEqual(["w2"]);
    act(() => root.unmount());
  });
});
