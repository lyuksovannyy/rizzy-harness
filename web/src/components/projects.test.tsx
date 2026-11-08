// @vitest-environment happy-dom
// Projects in the shell: the bar on a project's page (no Search, no
// Incognito, the project's folder), the chat header's project picker and
// the project page. The sidebar's Projects section is in sidebar.test.tsx.
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test, vi } from "vitest";
import type { ProjectDetail, ProjectSummary } from "../lib/projects";
import type { SessionRow } from "../types";
import { Composer } from "./Composer";
import { canJoinProject, ProjectChip } from "./ProjectChip";

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
