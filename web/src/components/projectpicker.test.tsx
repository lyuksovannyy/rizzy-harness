// @vitest-environment happy-dom
// The start screen's Project button (ADOPTION_PLAN X.26): in the agentic
// row it shows the choice (a project's name, a folder's last part, or "No
// project") with the execution mode beside it, always (a label without a
// choice); its popup lists the workspace's projects with their folders,
// makes a new project on the node (the API mocked: a name and the node,
// never a path), keeps today's free folder (whose chat sends exactly what
// it sent before), shows "From a repository" as Later, and shows names as
// text.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, test, vi } from "vitest";
import { projectSlug } from "../lib/projectSlug";
import type { NodeInfo } from "../types";
import { Composer, type Submission } from "./Composer";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

beforeAll(async () => {
  await import("./ProjectPicker");
  await import("./ExecChoice");
}, 30_000);

const tick = () => act(async () => new Promise((r) => setTimeout(r, 10)));
async function until<T>(q: () => T | null | undefined, ms = 4000): Promise<T | null> {
  const by = Date.now() + ms;
  while (!q() && Date.now() < by) await tick();
  return q() ?? null;
}

const node = (over: Partial<NodeInfo> = {}): NodeInfo => ({
  node_id: "node-1",
  hostname: "box",
  version: "0",
  connected: true,
  embedded: false,
  profile: "agentic",
  exec_modes: ["sandbox"],
  exec_gaps: [],
  projects: true,
  ...over,
});

const EVIL = '<img src=x onerror="window.pwned=1">';
const LIST = {
  projects: [
    { project_id: "p1", workspace_id: "w", name: "Website", description: "", icon: "code", accent: "blue", chats: 0, updated_ms: 0, node_id: "node-1", cwd: "/home/me/rizzy-projects/website-ab1c" },
    { project_id: "p2", workspace_id: "w", name: EVIL, description: "", icon: "folder", accent: "blue", chats: 0, updated_ms: 0, node_id: "gone", cwd: "/x/evil" },
    { project_id: "p3", workspace_id: "w", name: "Notes", description: "", icon: "folder", accent: "blue", chats: 0, updated_ms: 0, node_id: null, cwd: null },
  ],
  workspace_id: "w",
  can_create: true,
  icons: [],
  accents: [],
};

let calls: { method: string; path: string; body: unknown }[];
let canCreate: boolean;
beforeEach(() => {
  localStorage.clear();
  calls = [];
  canCreate = true;
  vi.stubGlobal("fetch", async (path: string, init: RequestInit) => {
    const body = init.body ? JSON.parse(init.body as string) : undefined;
    calls.push({ method: init.method ?? "GET", path, body });
    const json = (v: unknown) => new Response(JSON.stringify(v), { status: 200, headers: { "content-type": "application/json" } });
    if (path.startsWith("/api/projects?") || path === "/api/projects") {
      if (init.method === "POST")
        return json({
          ...LIST.projects[0],
          project_id: "p9",
          name: body.name,
          settings: { node_id: body.folder_on, cwd: `/home/me/rizzy-projects/${projectSlug(body.name)}-zz12`, workspace_memory: true, members_activate_facts: false },
        });
      return json({ ...LIST, can_create: canCreate });
    }
    if (path === "/api/projects/p3/folder")
      return json({ ...LIST.projects[2], settings: { node_id: body.node_id, cwd: "/home/me/rizzy-projects/notes-qq11" } });
    return new Response("{}", { status: 404 });
  });
});
afterEach(() => {
  vi.unstubAllGlobals();
  document.body.innerHTML = "";
});

function mount(workers: NodeInfo[], over: { hostRight?: { level: "host" | "sandbox" } } = {}) {
  const sent: Submission[] = [];
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() =>
    root.render(
      <Composer
        where="home"
        agents={[]}
        models={[]}
        workers={workers}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        hostRight={over.hostRight}
        workspace="w"
        onSubmit={async (s) => {
          sent.push(s);
        }}
      />,
    ),
  );
  act(() => [...host.querySelectorAll<HTMLButtonElement>(".mode-switch button")].find((b) => b.textContent === "Agentic")!.click());
  const set = (el: HTMLInputElement | HTMLTextAreaElement, value: string) =>
    act(() => {
      Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value")!.set!.call(el, value);
      el.dispatchEvent(new Event("input", { bubbles: true }));
    });
  set(host.querySelector("textarea")!, "fix the build");
  const button = () => until(() => host.querySelector<HTMLButtonElement>(".project-pick"));
  const open = async () => {
    await act(async () => (await button())!.click());
    return (await until(() => document.querySelector<HTMLDialogElement>("dialog.project-picker")))!;
  };
  const send = () => act(async () => host.querySelector<HTMLButtonElement>("button.send")!.click());
  const click = (root: ParentNode, text: string) =>
    act(async () => [...root.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.includes(text))!.click());
  return { host, sent, set, button, open, send, click, unmount: () => act(() => root.unmount()) };
}

describe("the Project button", () => {
  test("shows No project, then the latest folder; the mode is always shown", async () => {
    let c = mount([node()]);
    expect((await c.button())!.textContent).toBe("No project");
    // Only the sandbox here: a label, no choice.
    const label = await until(() => c.host.querySelector(".exec-only"));
    expect(label!.textContent).toBe("sandbox");
    expect(c.host.querySelector('select[aria-label="How this chat\'s commands run"]')).toBeNull();
    await c.send();
    expect(c.sent).toEqual([]);
    expect(c.host.textContent).toContain("Choose a project first.");
    c.unmount();
    localStorage.setItem("harness.folders", '["/home/me/proj/", "/srv/old"]');
    c = mount([node({ exec_modes: ["sandbox", "host"] })], { hostRight: { level: "host" } });
    expect(await until(() => (c.host.querySelector(".project-pick")?.textContent === "proj" ? true : null))).toBe(true);
    // Host offered and allowed: the choice.
    expect(await until(() => c.host.querySelector('select[aria-label="How this chat\'s commands run"]'))).not.toBeNull();
    c.unmount();
  });

  test("the popup lists projects with their folders; picking one sends its project, node and folder", async () => {
    const c = mount([node(), node({ node_id: "node-2", hostname: "other" })]);
    const d = await c.open();
    await until(() => d.querySelector(".pp-list li"));
    const rows = [...d.querySelectorAll(".pp-list li")];
    expect(rows.map((r) => r.querySelector(".pp-name")!.textContent)).toEqual(["Website", EVIL, "Notes"]);
    expect(rows[0]!.textContent).toContain("/home/me/rizzy-projects/website-ab1c");
    // Names are text: no element made from one.
    expect(d.querySelector("img")).toBeNull();
    expect((window as { pwned?: number }).pwned).toBeUndefined();
    // A project whose node is not connected cannot be picked.
    expect(rows[1]!.querySelector("button")!.disabled).toBe(true);
    expect(rows[1]!.textContent).toContain("its node gone is not connected");
    // Search.
    c.set(d.querySelector<HTMLInputElement>('input[type="search"]')!, "web");
    expect(d.querySelectorAll(".pp-list li").length).toBe(1);
    await c.click(d, "Website");
    expect(document.querySelector("dialog.project-picker")).toBeNull();
    expect(c.host.querySelector(".project-pick")!.textContent).toBe("Website");
    await c.send();
    expect(c.sent.at(-1)).toMatchObject({ projectId: "p1", nodeId: "node-1", cwd: "/home/me/rizzy-projects/website-ab1c" });
    c.unmount();
  });

  test("New project: the name and the node go to the server, never a path; the chat goes into it", async () => {
    const c = mount([node()]);
    const d = await c.open();
    await until(() => d.querySelector(".pp-list li"));
    const name = d.querySelector<HTMLInputElement>('input[aria-label="Project name"]')!;
    c.set(name, "Project Name Example");
    expect(d.querySelector(".pp-preview")!.textContent).toContain("project-name-example-····");
    await act(async () => d.querySelector<HTMLButtonElement>('section[aria-labelledby="pp-new"] button[type="submit"]')!.click());
    await until(() => (document.querySelector("dialog.project-picker") ? null : true));
    const post = calls.find((x) => x.method === "POST" && x.path === "/api/projects")!;
    expect(post.body).toEqual({ name: "Project Name Example", folder_on: "node-1", workspace_id: "w" });
    expect(c.host.querySelector(".project-pick")!.textContent).toBe("Project Name Example");
    await c.send();
    expect(c.sent.at(-1)).toMatchObject({
      projectId: "p9",
      nodeId: "node-1",
      cwd: "/home/me/rizzy-projects/project-name-example-zz12",
    });
    c.unmount();
  });

  test("a project without a folder gets one on the chosen node", async () => {
    const c = mount([node()]);
    const d = await c.open();
    await until(() => d.querySelector(".pp-list li"));
    await c.click(d, "Create its folder on box");
    await until(() => (document.querySelector("dialog.project-picker") ? null : true));
    expect(calls.find((x) => x.path === "/api/projects/p3/folder")!.body).toEqual({ node_id: "node-1" });
    await c.send();
    expect(c.sent.at(-1)).toMatchObject({ projectId: "p3", cwd: "/home/me/rizzy-projects/notes-qq11" });
    c.unmount();
  });

  test("Existing folder sends what the folder field sent; no project", async () => {
    const c = mount([node()]);
    const d = await c.open();
    c.set(d.querySelector<HTMLInputElement>('input[aria-label="Folder on that node"]')!, "/home/me/checkout");
    await c.click(d, "Use folder");
    expect(c.host.querySelector(".project-pick")!.textContent).toBe("checkout");
    await c.send();
    const s = c.sent.at(-1)!;
    expect(s).toMatchObject({ mode: "agentic", nodeId: "node-1", cwd: "/home/me/checkout", execMode: "sandbox" });
    expect(s.projectId).toBeUndefined();
    // And the chat's create body is today's.
    const { startChat } = await import("../lib/startChat");
    const created: unknown[] = [];
    const api = {
      createSession: async (b: unknown) => {
        created.push(b);
        return { session_id: "s1" };
      },
      prompt: async () => ({}),
    } as unknown as Parameters<typeof startChat>[3];
    await startChat(s, { workspace: "w" }, () => {}, api, () => {});
    expect(created).toEqual([
      { profile: "agentic", title: "fix the build", agent_id: "", node_id: "node-1", cwd: "/home/me/checkout", workspace_id: "w", exec_mode: "sandbox" },
    ]);
    expect(JSON.parse(localStorage.getItem("harness.folders")!)[0]).toBe("/home/me/checkout");
    c.unmount();
  });

  test("the backdrop and Escape close it, and it stays closed; the button's name is its own", async () => {
    const c = mount([node()]);
    let d = await c.open();
    // A click on the dialog itself is the backdrop.
    await act(async () => d.dispatchEvent(new MouseEvent("click", { bubbles: true })));
    await tick();
    expect(document.querySelector("dialog.project-picker")).toBeNull();
    d = await c.open();
    await act(async () => d.dispatchEvent(new Event("cancel", { cancelable: true })));
    await tick();
    expect(document.querySelector("dialog.project-picker")).toBeNull();
    // The dialog is not inside the label that names the button.
    d = await c.open();
    expect(d.closest("label")).toBeNull();
    expect(c.host.querySelector(".project-pick")!.closest("label")!.textContent).toBe("ProjectNo project");
    c.unmount();
  });

  test("From a repository is Later; a role that cannot create projects and a node without project folders say so", async () => {
    let c = mount([node()]);
    let d = await c.open();
    const later = [...d.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.includes("Later"))!;
    expect(later.disabled).toBe(true);
    canCreate = false;
    await c.click(d, "Cancel");
    c.unmount();
    c = mount([node()]);
    d = await c.open();
    await until(() => d.querySelector(".pp-list li"));
    expect(d.textContent).toContain("Your role in this workspace cannot create projects.");
    expect(d.querySelector('input[aria-label="Project name"]')).toBeNull();
    await c.click(d, "Cancel");
    c.unmount();
    canCreate = true;
    c = mount([node({ projects: false })]);
    d = await c.open();
    await until(() => d.querySelector(".pp-list li"));
    expect(d.textContent).toContain("makes no project folders");
    expect(d.textContent).not.toContain("Create its folder");
    c.unmount();
  });
});

describe("the folder name preview", () => {
  test("follows the node's rule", () => {
    for (const [name, want] of [
      ["Project Name Example", "project-name-example"],
      ["../x", "x"],
      ["..", "project"],
      ["/etc/passwd", "etc-passwd"],
      ["a\u0000b\nc", "a-b-c"],
      ["a／b∕c", "a-b-c"],
      ["Café Ünïcode", "caf-n-code"],
      ["", "project"],
    ] as [string, string][]) {
      expect(projectSlug(name)).toBe(want);
    }
    expect(projectSlug("x".repeat(10_000)).length).toBe(48);
  });
});
