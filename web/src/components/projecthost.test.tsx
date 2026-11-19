// @vitest-environment happy-dom
// The project page's new-chat row offers the host shell by the start
// screen's rule (the trial, 2026-11-19: the page showed a fixed "sandbox"
// where the same project started from the start screen offered the choice):
// `hostRight` reaches the page's bar, and the bar decides by `mayPickHost`.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { ProjectDetail } from "../lib/projects";
import type { ExecLevel, NodeInfo } from "../types";

vi.mock("../api", async (original) => {
  const m = await original<typeof import("../api")>();
  return { ...m, api: { ...m.api, features: () => Promise.resolve({ artifacts: { show: false }, media: { vision_images: false } }) } };
});
const get = vi.fn<(id: string) => Promise<ProjectDetail>>();
vi.mock("../lib/projects", async (original) => {
  const m = await original<typeof import("../lib/projects")>();
  return { ...m, projectsApi: { ...m.projectsApi, get: (id: string) => get(id) } };
});
const { default: Project } = await import("./Project");
await import("./ExecChoice");
await import("./ProjectPicker");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const node = (modes: NodeInfo["exec_modes"]): NodeInfo => ({
  node_id: "n1",
  hostname: "box",
  version: "0",
  connected: true,
  embedded: false,
  profile: "agentic",
  exec_modes: modes,
  exec_gaps: [],
});

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  get.mockResolvedValue({
    project_id: "p1",
    workspace_id: "w1",
    name: "Garden",
    description: "",
    icon: "house",
    accent: "green",
    chats: 0,
    updated_ms: 0,
    instructions: "",
    settings: { workspace_memory: true, members_activate_facts: false, node_id: "n1", cwd: "/srv/garden" },
    can_manage: true,
    created_by: null,
    created_ms: 0,
  } as ProjectDetail);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function open(modes: NodeInfo["exec_modes"], hostRight?: { level: ExecLevel }) {
  const n = node(modes);
  await act(async () =>
    root.render(
      <Project
        id="p1"
        version={0}
        memoryVersion={0}
        chats={[]}
        agents={[]}
        agentDefaults={{ chat: "", agentic: null }}
        models={[]}
        workers={[n]}
        nodes={[n]}
        prefs={null}
        agenticBlocked={null}
        hostRight={hostRight}
        onSubmit={async () => {}}
        onOpenChat={() => {}}
        onOpenProject={() => {}}
        onDeleted={() => {}}
        workspaceId="w1"
        onForeign={() => {}}
      />,
    ),
  );
  await act(async () => {});
  act(() => [...host.querySelectorAll<HTMLButtonElement>(".mode-switch button")].find((b) => b.textContent === "Agentic")!.click());
  const by = Date.now() + 4000;
  while (!host.querySelector('[aria-label="How this chat\'s commands run"]') && Date.now() < by)
    await act(async () => new Promise((r) => setTimeout(r, 10)));
  return host.querySelector<HTMLElement>('[aria-label="How this chat\'s commands run"]');
}

describe("the host shell on the project page's bar", () => {
  test("a person at level host on a node that offers it gets the choice, as on the start screen", async () => {
    const el = await open(["sandbox", "host"], { level: "host" });
    expect(el?.tagName).toBe("SELECT");
    expect([...el!.querySelectorAll("option")].map((o) => o.value)).toEqual(["sandbox", "host"]);
  });

  test("below level host, or on a node without it, the plain label stays", async () => {
    expect((await open(["sandbox", "host"], { level: "sandbox" }))?.tagName).toBe("SPAN");
    act(() => root.render(null));
    expect((await open(["sandbox"], { level: "host" }))?.tagName).toBe("SPAN");
  });
});
