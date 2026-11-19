// @vitest-environment happy-dom
// Projects in the shell: the bar on a project's page (no Search, no
// Incognito, the project's folder), the chat header's project picker and
// the project page. The sidebar's Projects section is in sidebar.test.tsx.
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test, vi } from "vitest";
import { HttpError } from "../api";
import { type PendingProject, type ProjectDetail, type ProjectSummary, pendingProjects } from "../lib/projects";
import type { SessionRow } from "../types";
import { Composer } from "./Composer";
import { canJoinProject, ProjectChip } from "./ProjectChip";

// The page asks whether to show its Artifacts tab and take images on its bar
// (projectartifacts.test.tsx).
vi.mock("../api", async (original) => {
  const m = await original<typeof import("../api")>();
  return { ...m, api: { ...m.api, features: () => Promise.resolve({ artifacts: { show: false }, media: { vision_images: false } }) } };
});
const get = vi.fn<(id: string) => Promise<ProjectDetail>>();
vi.mock("../lib/projects", async (original) => {
  const m = await original<typeof import("../lib/projects")>();
  return { ...m, projectsApi: { ...m.projectsApi, get: (id: string) => get(id) } };
});
const { default: Project, agentMark } = await import("./Project");

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
  test("no Search mode and no Incognito; the project's folder is filled in", async () => {
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
    // The Project button (X.26, a lazy chunk) shows the folder's last part.
    const by = Date.now() + 4000;
    while (!c.host.querySelector(".project-pick") && Date.now() < by)
      await act(async () => new Promise((r) => setTimeout(r, 10)));
    const folder = c.host.querySelector<HTMLButtonElement>(".project-pick")!;
    expect(folder.textContent).toBe("garden");
    expect(folder.title).toBe("/srv/garden");
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
      settings: { workspace_memory: true, members_activate_facts: false, node_id: null, cwd: null },
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
        agentDefaults={{ chat: "", agentic: null }}
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

describe("a pending project's page", () => {
  const page = (n: number) => (
    <Project
      id="p9"
      version={n}
      memoryVersion={0}
      chats={[]}
      agents={[]}
      agentDefaults={{ chat: "", agentic: null }}
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
      onForeign={() => {}}
    />
  );
  const pend = (over: Partial<PendingProject> = {}): PendingProject => ({
    project_id: "p9",
    workspace_id: "w1",
    name: "Harness",
    node_id: "n1",
    state: "cloning",
    source: { host: "github.com", full_name: "ada/harness" },
    started_ms: 1,
    mine: true,
    reason: null,
    ...over,
  });

  test("shows the cloning state, not 'no longer exists', and opens once the clone has finished", async () => {
    get.mockRejectedValue(new HttpError(404, "no such project"));
    pendingProjects.set([pend()]);
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    await act(async () => root.render(page(0)));
    expect(host.textContent).not.toContain("no longer exists");
    expect(host.textContent).toContain("still being cloned from github.com/ada/harness");
    // A failed clone says so, with its reason.
    act(() => pendingProjects.set([pend({ state: "failed", reason: "the clone took longer than this node allows" })]));
    expect(host.textContent).toContain("failed: the clone took longer than this node allows");
    // Gone from the pending list and no project: that is "no longer exists".
    act(() => pendingProjects.set([]));
    await act(async () => root.render(page(1)));
    expect(host.textContent).toContain("This project no longer exists.");
    // The clone finished: projects changed (the version moved), the page looks again.
    get.mockResolvedValue({ ...garden, project_id: "p9", name: "Harness", instructions: "", settings: {} as never, can_manage: true, created_by: "u1", created_ms: 0 });
    await act(async () => root.render(page(2)));
    expect(host.textContent).not.toContain("no longer exists");
    expect(host.querySelector("h1")?.textContent).toBe("Harness");
    act(() => root.unmount());
  });
});

describe("board cards and agents", () => {
  test("a card made or noted by an agent says so", () => {
    expect(agentMark({ agent: "Assistant", notes: "" })).toBe("Added by Assistant (agent)");
    expect(agentMark({ agent: null, notes: "mine\n[agent Assistant] started" })).toBe("Noted by an agent");
    expect(agentMark({ agent: null, notes: "plain [agent x] mid-line" })).toBeNull();
    expect(agentMark({ notes: "" })).toBeNull();
  });
});
