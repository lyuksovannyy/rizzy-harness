// @vitest-environment happy-dom
// "MCP servers from the repository" (X.36) against a mocked API: each
// entry's command, then each argument on its own line, then each
// NAME=value, exactly as sent (nothing joined, quoted or cut), as text;
// Approve sends the name and the digest it was shown, Remove the name,
// Reload asks again; the node's refusal and the reasons are shown.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { McpList } from "./ProjectMcp";

const call = vi.fn();
vi.mock("../api", () => ({ call }));
const { default: ProjectMcp } = await import("./ProjectMcp");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const list: McpList = {
  node_id: "n1",
  entries: [
    {
      name: "fs",
      state: "new",
      command: "npx",
      args: ["-y", "@x/server", "a b", "it's \"quoted\" $HOME"],
      env: [
        { name: "TOKEN", value: "t-123" },
        { name: "NODE_OPTIONS", value: "--require ./x.js" },
      ],
      digest: "d".repeat(64),
    },
    {
      name: "db",
      state: "changed",
      command: "uvx",
      args: ["db-new"],
      env: [],
      digest: "e".repeat(64),
      approved_command: "uvx",
      approved_args: ["db-old"],
      approved_env: [{ name: "OLD", value: "1" }],
    },
    { name: "web", state: "unsupported", command: "", args: [], env: [], reason: "not supported: remote" },
    { name: "x", state: "approved", command: '<img src="https://x.example/a.png">', args: [], env: [] },
  ],
};

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

async function show(hasFolder = true) {
  await act(async () => {
    root.render(<ProjectMcp projectId="p1" hasFolder={hasFolder} />);
  });
}

const button = (label: string, n = 0) =>
  [...host.querySelectorAll("button")].filter((b) => b.textContent === label)[n] as HTMLButtonElement;
const lines = (cls: string, entry: number) =>
  [...(host.querySelectorAll(".mcp-entry")[entry]?.querySelectorAll(`.${cls}`) ?? [])].map((n) => n.textContent);

test("each argument and NAME=value on its own line, unmodified, as text", async () => {
  call.mockResolvedValue(list);
  await show();
  expect(call).toHaveBeenCalledWith("GET", "/api/projects/p1/mcp");
  expect(lines("mcp-arg", 0)).toEqual(["-y", "@x/server", "a b", "it's \"quoted\" $HOME"]);
  expect(lines("mcp-env", 0)).toEqual(["TOKEN=t-123", "NODE_OPTIONS=--require ./x.js"]);
  // The changed entry: the file's, then the approved copy that runs now.
  expect(lines("mcp-arg", 1)).toEqual(["db-new", "db-old"]);
  expect(lines("mcp-env", 1)).toEqual(["OLD=1"]);
  expect(host.textContent).toContain("not supported: remote");
  expect(host.querySelector("img")).toBeNull();
  expect(host.textContent).toContain('<img src="https://x.example/a.png">');
});

test("Approve, Remove and Reload ask the node", async () => {
  call.mockResolvedValue(list);
  await show();
  await act(async () => button("Approve").click());
  expect(call).toHaveBeenLastCalledWith("POST", "/api/projects/p1/mcp/approve", { name: "fs", digest: "d".repeat(64) });
  await act(async () => button("Approve the change").click());
  expect(call).toHaveBeenLastCalledWith("POST", "/api/projects/p1/mcp/approve", { name: "db", digest: "e".repeat(64) });
  // Remove for the changed and the approved entries; none for the others.
  expect([...host.querySelectorAll("button")].filter((b) => b.textContent === "Remove")).toHaveLength(2);
  await act(async () => button("Remove", 1).click());
  expect(call).toHaveBeenLastCalledWith("POST", "/api/projects/p1/mcp/remove", { name: "x" });
  await act(async () => button("Reload").click());
  expect(call).toHaveBeenLastCalledWith("GET", "/api/projects/p1/mcp");
});

test("the node's refusal and a project without a folder are said", async () => {
  call.mockRejectedValue(new Error('node n1 does not run a repository\'s MCP servers: its node.toml has no [mcp] project_servers = "ask"'));
  await show();
  expect(host.textContent).toContain("project_servers");
  call.mockReset();
  await show(false);
  expect(call).not.toHaveBeenCalled();
  expect(host.textContent).toContain("no folder on a node");
});

const text = () => host.textContent ?? "";

test("the page says where an approved server runs, by the modes the node offers", async () => {
  call.mockResolvedValue(list);
  const shown = async (modes?: ("sandbox" | "host")[]) => {
    await act(async () => root.render(<ProjectMcp projectId="p1" hasFolder modes={modes} />));
    return text();
  };
  // Sandbox only: inside the sandbox, no word of the host.
  expect(await shown(["sandbox"])).toContain("inside the sandbox");
  expect(text()).not.toContain("without the sandbox");
  expect(text()).not.toContain("host chat");
  // Not known (the node list has not loaded, the node is not in it, or the
  // master blanked the modes): said so, and never the sandbox sentence next
  // to an Approve button.
  for (const unknown of [undefined, []] as ("sandbox" | "host")[][] | undefined[]) {
    const t = await shown(unknown);
    expect(t).toContain("Where an approved server runs is not known yet: this node's modes have not loaded");
    expect(t).not.toContain("inside the sandbox");
    expect(t).not.toContain("without the sandbox");
  }
  // Both: each chat kind's answer, and the entries with values say theirs.
  expect(await shown(["sandbox", "host"])).toContain("as the node's own user without the sandbox in a host chat");
  expect(text()).not.toContain("never in a host chat");
  // `fs` has values, `db`'s approved copy has one: each says it.
  expect(text().split("not started while it has environment values").length - 1).toBe(2);
  // Host only: every chat there is a host chat.
  expect(await shown(["host"])).toContain("every chat on this node is a host chat");
  expect(text()).toContain("as the node's own user, without the sandbox");
});
