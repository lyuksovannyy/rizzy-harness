// @vitest-environment happy-dom
// A project's Artifacts tab: shown while the server keeps Artifacts for the
// viewer here, it lists what the project's chats published (`?project=`),
// each linking to its page, with Download once a version is ready. The
// project's bar takes images where the server does.
import { act } from "react";
import { linked } from "../lib/links.setup";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Artifact, ArtifactPage, ArtifactQuery } from "../lib/artifacts";
import type { ProjectDetail } from "../lib/projects";
import type { Features } from "../types";

const features = vi.fn<() => Promise<Features>>();
vi.mock("../api", () => ({ api: { features: () => features() }, call: vi.fn(), HttpError: Error }));
const get = vi.fn<(id: string) => Promise<ProjectDetail>>();
vi.mock("../lib/projects", async (original) => {
  const m = await original<typeof import("../lib/projects")>();
  return { ...m, projectsApi: { ...m.projectsApi, get: (id: string) => get(id) } };
});
const list = vi.fn<(q: ArtifactQuery) => Promise<ArtifactPage>>();
vi.mock("../lib/artifacts", async (original) => ({ ...(await original<typeof import("../lib/artifacts")>()), artifactsApi: { list: (q: ArtifactQuery) => list(q) } }));
const { default: Project } = await import("./Project");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const PID = "f".repeat(32);
const project = {
  project_id: PID,
  workspace_id: "w1",
  name: "CLI",
  description: "",
  icon: "box",
  accent: "blue",
  chats: 0,
  updated_ms: 0,
  instructions: "",
  settings: { node_id: null, cwd: null },
  can_manage: true,
  created_by: "u1",
  created_ms: 0,
} as unknown as ProjectDetail;
const shown = (on: boolean, vision_images = false) => ({ artifacts: { enabled: on, show: on, why: null }, media: { vision_images } }) as unknown as Features;
const artifact = (n: number, status: "ready" | "pending"): Artifact =>
  ({
    artifact_id: String(n).padStart(32, "a"),
    name: `app${n}`,
    kind: "binary",
    latest: { seq: 3, size: 2048, status },
    updated_ms: 0,
  }) as unknown as Artifact;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  get.mockResolvedValue(project);
  list.mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function show() {
  await act(async () =>
    root.render(
      <Project
        id={PID}
        version={0}
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
        onForeign={() => {}}
      />,
    ),
  );
  await act(async () => {});
}
const tab = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find((b) => b.textContent === label);

describe("the project's Artifacts tab", () => {
  test("lists what its chats published, each to its page; Download once ready", async () => {
    features.mockResolvedValue(shown(true));
    list.mockResolvedValue({ artifacts: [artifact(1, "ready"), artifact(2, "pending")], next_cursor: null, usage: { used: 0, quota: 0 } });
    await show();
    await act(async () => tab("Artifacts")!.click());
    expect(list).toHaveBeenCalledWith({ workspace: "w1", project: PID });
    const rows = [...host.querySelectorAll(".project-chats li")];
    expect(rows.map((r) => r.querySelector(".session-item")!.textContent)).toEqual(["app1v3", "app2v3"]);
    expect(rows[0]!.querySelector(".session-item")!.getAttribute("href")).toBe(`#/artifacts/${artifact(1, "ready").artifact_id}`);
    expect(rows[0]!.querySelector("a[download]")!.getAttribute("href")).toBe(linked(`/api/artifacts/${artifact(1, "ready").artifact_id}/latest/download`));
    expect(rows[1]!.querySelector("a[download]")).toBeNull();
  });

  test("empty, and hidden where the viewer has no Artifacts", async () => {
    features.mockResolvedValue(shown(true));
    list.mockResolvedValue({ artifacts: [], next_cursor: null, usage: { used: 0, quota: 0 } });
    await show();
    await act(async () => tab("Artifacts")!.click());
    expect(host.textContent).toContain("No artifacts yet. Files agents publish from this project's agentic chats show here.");
    act(() => root.unmount());
    root = createRoot(host);
    features.mockResolvedValue(shown(false));
    await show();
    expect(tab("Artifacts")).toBeUndefined();
    expect(tab("Memory")).toBeDefined();
  });
});

describe("the project's bar", () => {
  test("takes images where the server does", async () => {
    const hint = () => host.querySelector<HTMLButtonElement>('button[aria-label="Attach files"]')!.title;
    features.mockResolvedValue(shown(true, true));
    await show();
    expect(hint()).toBe("Attach documents or images (images are kept in Media)");
    act(() => root.unmount());
    root = createRoot(host);
    features.mockResolvedValue(shown(true, false));
    await show();
    expect(hint()).not.toContain("images");
  });
});
