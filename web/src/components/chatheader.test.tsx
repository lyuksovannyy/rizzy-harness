// @vitest-environment happy-dom
// The chat's header is one line: the title (click to rename, ChatTitle), then
// a muted suffix, "project · node · how commands run", and at the right
// Export and Delete (and a slot for one more small button). The agent, the
// model, the folder, the project chip and the pencil are gone from it (the
// agent and model are in the composer's strip). The project in the suffix is
// the control the chip was: the same select, the same call.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { ProjectSummary } from "../lib/projects";
import type { NodeInfo, SessionRow } from "../types";

const setChatProject = vi.fn((_s: string, _p: string | null) => Promise.resolve({} as never));
vi.mock("../lib/projects", async (original) => {
  const m = await original<typeof import("../lib/projects")>();
  return { ...m, projectsApi: { ...m.projectsApi, setChatProject: (s: string, p: string | null) => setChatProject(s, p) } };
});
vi.mock("../apiPages", () => ({ pageApi: { renameSession: (_i: string, title: string) => Promise.resolve({ ok: true, title }) } }));
const { ChatHeader } = await import("./ChatHeader");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  setChatProject.mockClear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const garden: ProjectSummary = { project_id: "p1", workspace_id: "w1", name: "test-project", description: "", icon: "house", accent: "green", chats: 1, updated_ms: 0 };
const other: ProjectSummary = { ...garden, project_id: "p2", name: "Other" };
const row = (over: Partial<SessionRow> = {}): SessionRow =>
  ({ session_id: "s1", node_id: "n1", title: "yoyo", cwd: "/src/app", model: "m", created_ms: 0, updated_ms: 0, head_seq: 1, running: false, owner_id: "u1", agent_id: "general", profile: "agentic", incognito: false, parent_id: null, project_id: "p1", ...over }) as SessionRow;
const local: NodeInfo = { node_id: "n1", hostname: "box", version: "1", connected: true, embedded: true, profile: "agentic" };

function draw(over: Partial<React.ComponentProps<typeof ChatHeader>> = {}) {
  const calls = { export: 0, del: 0 };
  act(() =>
    root.render(
      <ChatHeader
        sessionId="s1"
        title="yoyo"
        canRename
        row={row()}
        projects={[garden, other]}
        agentic
        node={local}
        nodeId="n1"
        execMode="sandbox"
        cwd="/src/app"
        exportDisabled={false}
        onExport={() => void calls.export++}
        onDelete={() => void calls.del++}
        {...over}
      />,
    ),
  );
  return calls;
}
const suffix = () => host.querySelector<HTMLElement>(".chat-suffix");
// A part's words (the project control also holds its options' text).
const visible = () => [...suffix()!.querySelectorAll(".chat-part")].map((c) => (c.querySelector(".ellipsis") ?? c).textContent);

describe("the header's content", () => {
  test("title, then project, node and execution state in that order; export and delete at the right", () => {
    const calls = draw();
    expect(host.querySelector("h1")!.textContent).toBe("yoyo");
    expect(visible()).toEqual(["test-project", "local", "sandbox"]);
    const head = host.querySelector(".conv-head")!;
    expect(head.querySelector("h1")!.compareDocumentPosition(suffix()!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const actions = [...head.querySelectorAll(".conv-actions button")].map((b) => b.getAttribute("aria-label"));
    expect(actions).toEqual(["Export as Markdown", "Delete chat"]);
    act(() => head.querySelector<HTMLButtonElement>('button[aria-label="Export as Markdown"]')!.click());
    act(() => head.querySelector<HTMLButtonElement>('button[aria-label="Delete chat"]')!.click());
    expect(calls).toEqual({ export: 1, del: 1 });
  });

  test("nothing of the removed items: no pencil, no second line, no agent or model or folder text, no chip, no cube", () => {
    draw();
    const head = host.querySelector(".conv-head")!;
    expect(head.querySelector(".conv-meta")).toBeNull();
    expect(head.querySelector(".conv-agent, .meta-model, .agent-avatar")).toBeNull();
    expect(head.querySelector(".chat-suffix .project-chip")).not.toBeNull(); // the suffix's own control
    expect(head.querySelector(".conv-head > .project-chip, .conv-actions .project-chip")).toBeNull();
    expect(head.querySelector('[aria-label="Save a file as an artifact"]')).toBeNull();
    expect(head.querySelector('[aria-label="Rename chat"], .icon.small')).toBeNull();
    expect(visible().join(" ")).not.toContain("/src/app");
    // Only the export and delete buttons are buttons, besides the title itself.
    expect([...head.querySelectorAll("button")].map((b) => b.className)).toEqual(["chat-title-btn", "icon", "icon"]);
  });

  test("the suffix's hover text is the folder; a hidden sentence says it all in words", () => {
    draw();
    expect(suffix()!.title).toBe("/src/app");
    expect(suffix()!.querySelector(".sr-only")!.textContent).toBe("In project test-project, on node local, sandboxed, folder /src/app");
  });

  test("the execution state keeps ExecBadge's words and hover text; a host chat says so", () => {
    draw({ execMode: "host" });
    const exec = suffix()!.querySelector<HTMLElement>(".exec-mode")!;
    expect(exec.textContent).toBe("host shell");
    expect(exec.title).toContain("commands run in the host shell");
    expect(suffix()!.querySelector(".sr-only")!.textContent).toContain("in the host shell, not contained");
  });

  test("no project: what the chip showed (its add control, muted); no projects at all: nothing there", () => {
    draw({ row: row({ project_id: null }) });
    expect(visible()).toEqual(["No project", "local", "sandbox"]);
    // Quiet: words only, no folder icon, the same control underneath.
    const chip = suffix()!.querySelector(".project-chip")!;
    expect(chip.classList.contains("on")).toBe(false);
    expect(chip.querySelector("svg")).toBeNull();
    expect(chip.querySelector("select")).not.toBeNull();
    expect(suffix()!.querySelector(".sr-only")!.textContent).toBe("In no project, on node local, sandboxed, folder /src/app");
    draw({ row: row({ project_id: null }), projects: [] });
    expect(visible()).toEqual(["local", "sandbox"]);
    expect(suffix()!.querySelector(".sr-only")!.textContent).toBe("On node local, sandboxed, folder /src/app");
  });

  test("a plain chat (no node shell, no project) has no suffix and no empty separators", () => {
    draw({ agentic: false, execMode: undefined, cwd: "", row: row({ profile: "chat", project_id: null }), projects: [] });
    expect(suffix()).toBeNull();
    expect(host.querySelector(".chat-sep")).toBeNull();
  });

  test("a plain chat in a project shows only the project; incognito and sub-task chats have no project control", () => {
    draw({ agentic: false, execMode: undefined, cwd: "", row: row({ profile: "chat" }) });
    expect(visible()).toEqual(["test-project"]);
    expect(host.querySelectorAll(".chat-sep")).toHaveLength(0);
    draw({ agentic: false, execMode: undefined, cwd: "", row: row({ profile: "chat", incognito: true }) });
    expect(suffix()).toBeNull();
    draw({ row: row({ parent_id: "x" }) });
    expect(visible()).toEqual(["local", "sandbox"]);
    expect(host.querySelectorAll(".chat-sep")).toHaveLength(1);
  });

  test("an offline node shows the way the badge did", () => {
    draw({ node: { ...local, connected: false, embedded: false, hostname: "build-box" } });
    const node = suffix()!.querySelector<HTMLElement>(".badge")!;
    expect(node.textContent).toBe("build-box");
    expect(node.className).toContain("badge-offline");
    expect(node.title).toBe("node n1 (build-box) — offline");
  });

  test("the export button waits for something to export; the slot holds one more button", () => {
    draw({ exportDisabled: true, extra: <button type="button" className="icon" aria-label="Pinned" /> });
    expect(host.querySelector<HTMLButtonElement>('button[aria-label="Export as Markdown"]')!.disabled).toBe(true);
    expect([...host.querySelectorAll(".conv-actions button")].map((b) => b.getAttribute("aria-label"))).toEqual(["Pinned", "Export as Markdown", "Delete chat"]);
  });
});

describe("the project control is what the chip was", () => {
  test("a pick moves the chat into another project, or out; its name is Project", () => {
    draw();
    const sel = host.querySelector<HTMLSelectElement>('select[aria-label="Project"]')!;
    expect(sel.value).toBe("p1");
    expect([...sel.options].map((o) => o.textContent)).toEqual(["No project", "test-project", "Other"]);
    const pick = (v: string) =>
      act(() => {
        sel.value = v;
        sel.dispatchEvent(new Event("change", { bubbles: true }));
      });
    pick("p2");
    pick("");
    expect(setChatProject.mock.calls).toEqual([["s1", "p2"], ["s1", null]]);
  });

  test("a refusal shows in its hover text", async () => {
    setChatProject.mockImplementationOnce(() => Promise.reject(new Error("not allowed")));
    draw();
    const sel = host.querySelector<HTMLSelectElement>('select[aria-label="Project"]')!;
    await act(async () => {
      sel.value = "p2";
      sel.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(host.querySelector<HTMLElement>(".project-chip")!.title).toBe("not allowed");
  });
});

describe("renaming from the header", () => {
  test("the title is the only rename control; a viewer who may not rename gets plain text", () => {
    draw();
    expect(host.querySelector('button[title="Rename chat"]')!.textContent).toBe("yoyo");
    draw({ canRename: false });
    expect(host.querySelector('button[title="Rename chat"]')).toBeNull();
    expect(host.querySelector("h1")!.textContent).toBe("yoyo");
  });
});
