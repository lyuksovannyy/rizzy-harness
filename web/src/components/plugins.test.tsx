// @vitest-environment happy-dom
// The Plugins page (X.34 step A) against a mocked API: one list of skills
// and connectors with the nodes that have each, the kind filter and the
// search, node text shown as text only, Shared / Assigned with the agent
// picker for an editor (the PUT it sends), read-only for a member, and the
// notes for a node that does not enforce the settings.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { Features, Me } from "../types";
import type { PluginList } from "./Plugins";

const call = vi.fn();
vi.mock("../api", () => ({ call }));
const { default: Plugins } = await import("./Plugins");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const listing = (can_edit: boolean): PluginList => ({
  can_edit,
  items: [
    { kind: "mcp", name: "github", nodes: [{ node_id: "n1", hostname: "box", connected: true, description: null }], access: "assigned", agents: ["coder"] },
    {
      kind: "skill",
      name: "deploy",
      nodes: [
        { node_id: "n1", hostname: "box", connected: true, description: '<img src="https://x.example/a.png"> Ship it' },
        { node_id: "n2", hostname: "far", connected: false, description: "Other text" },
      ],
      access: "shared",
      agents: [],
    },
  ],
  agents: [
    { id: "coder", name: "Software Developer", skills: true, connectors: true },
    { id: "general", name: "Assistant", skills: false, connectors: false },
  ],
  unenforced: [{ node_id: "n3", hostname: "old" }],
  left_out: [{ node_id: "n1", hostname: "box", count: 3 }],
});

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  call.mockReset();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const live = { onTopic: () => () => {} } as unknown as Live;
async function show(list: PluginList) {
  call.mockResolvedValue(list);
  await act(async () => {
    root.render(<Plugins me={{} as Me} scope="w1" navigate={() => {}} live={live} features={{} as Features} />);
  });
}
const rows = () => [...host.querySelectorAll(".plugin-row")];
const button = (text: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === text);

describe("plugins", () => {
  test("lists both kinds with their nodes, text only, and filters", async () => {
    await show(listing(false));
    expect(call).toHaveBeenCalledWith("GET", "/api/workspaces/w1/plugins");
    expect(rows().map((r) => r.querySelector("strong")!.textContent)).toEqual(["github", "deploy"]);
    const deploy = rows()[1]!;
    expect(deploy.textContent).toContain('<img src="https://x.example/a.png"> Ship it');
    expect(host.querySelector("img")).toBeNull();
    expect(deploy.textContent).toContain("Node: box, far (offline)");
    expect(deploy.textContent).toContain("Nodes describe it differently");
    expect(deploy.textContent).toContain("Shared: every agent");
    expect(rows()[0]!.textContent).toContain("Assigned: Software Developer");
    expect(host.textContent).toContain("Node old does not enforce these settings");
    expect(host.textContent).toContain("has 3 more that are not listed");
    // A member reads; no change control.
    expect(button("Change")).toBeUndefined();
    act(() => button("Skills")!.click());
    expect(rows().map((r) => r.querySelector("strong")!.textContent)).toEqual(["deploy"]);
    act(() => button("Connectors")!.click());
    expect(rows().map((r) => r.querySelector("strong")!.textContent)).toEqual(["github"]);
  });

  test("an editor assigns an item to the agents picked", async () => {
    await show(listing(true));
    const change = [...host.querySelectorAll<HTMLButtonElement>("button")].filter((b) => b.textContent === "Change");
    expect(change).toHaveLength(2);
    act(() => change[1]!.click());
    act(() => button("Assigned")!.click());
    const boxes = [...host.querySelectorAll<HTMLInputElement>(".plugin-editor input[type=checkbox]")];
    expect(boxes).toHaveLength(2);
    expect(host.querySelector(".plugin-editor")!.textContent).toContain("(its Skills switch is off)");
    act(() => boxes[0]!.click());
    call.mockResolvedValueOnce({ ok: true }).mockResolvedValue(listing(true));
    await act(async () => button("Save")!.click());
    expect(call).toHaveBeenCalledWith("PUT", "/api/workspaces/w1/plugins/skill/deploy", { access: "assigned", agents: ["coder"] });
  });

  test("a stored skill shows its origin and opens as text; the nodes' own of its name is said", async () => {
    const l = listing(false);
    l.items.push({
      kind: "skill",
      name: "notes",
      stored: { origin: "builtin", description: "Take notes", can_edit: false },
      nodes: [{ node_id: "n1", hostname: "box", connected: true, description: "Box notes" }],
      access: "shared",
      agents: [],
    });
    await show(l);
    const notes = rows()[2]!;
    expect(notes.textContent).toContain("Built-in");
    expect(notes.textContent).toContain("box has its own skill of this name and uses that one there");
    call.mockResolvedValueOnce({ name: "notes", description: "Take notes", body: "<script>x()</script> **Do** it", can_edit: false });
    await act(async () => button("Open")!.click());
    expect(call).toHaveBeenCalledWith("GET", "/api/workspaces/w1/skills/server/notes");
    expect(host.querySelector("pre.skill-body")!.textContent).toBe("<script>x()</script> **Do** it");
    expect(host.querySelector("script")).toBeNull();
    expect(button("Edit")).toBeUndefined();
    expect(button("Add")).toBeUndefined();
  });

  test("an editor writes a skill from the Add menu and sees the server's notes", async () => {
    await show({ ...listing(true), can_edit_server: true });
    act(() => button("Add")!.click());
    act(() => button("Write a skill")!.click());
    const inputs = [...host.querySelectorAll<HTMLInputElement>(".plugin-editor input:not([type=file])")];
    const area = host.querySelector<HTMLTextAreaElement>(".plugin-editor textarea")!;
    const type = (el: HTMLInputElement | HTMLTextAreaElement, v: string) => {
      const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, v);
      el.dispatchEvent(new Event("input", { bubbles: true }));
    };
    act(() => {
      type(inputs[0]!, "release-notes");
      type(inputs[1]!, "Write release notes");
      type(area, "Group by area.");
    });
    act(() => button("Built-in (every workspace)")!.click());
    call.mockResolvedValueOnce({ ok: true, notes: ["not used: license"] }).mockResolvedValue(listing(true));
    await act(async () => button("Save")!.click());
    expect(call).toHaveBeenCalledWith("POST", "/api/workspaces/w1/skills", {
      scope: "server",
      name: "release-notes",
      description: "Write release notes",
      body: "Group by area.",
    });
    expect(host.textContent).toContain("not used: license");
  });

  test("an editor is told of a workspace skill a built-in one hides, and can delete it", async () => {
    await show({ ...listing(true), hidden_skills: [{ name: "notes", description: "Old notes" }] });
    expect(host.textContent).toContain("This workspace's skill notes is hidden: a built-in skill has its name");
    window.confirm = () => true;
    call.mockResolvedValueOnce({ ok: true }).mockResolvedValue(listing(true));
    await act(async () => button("Delete")!.click());
    expect(call).toHaveBeenCalledWith("DELETE", "/api/workspaces/w1/skills/workspace/notes");
  });
});
