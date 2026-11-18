// @vitest-environment happy-dom
// "MCP servers from the repository" (X.36) against a mocked API: each
// entry's command and arguments in full as text, environment names only,
// Approve sends the name and the digest it was shown, Remove the name,
// Reload asks again; the node's refusal and the reasons are shown.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { McpList } from "./ProjectMcp";

const call = vi.fn();
vi.mock("../api", () => ({ call }));
const { default: ProjectMcp, commandLine } = await import("./ProjectMcp");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const list: McpList = {
  node_id: "n1",
  entries: [
    { name: "fs", state: "new", command: "npx", args: ["-y", "@x/server", "a b"], env_keys: ["TOKEN"], digest: "d".repeat(64) },
    {
      name: "db",
      state: "changed",
      command: "uvx",
      args: ["db-new"],
      env_keys: [],
      digest: "e".repeat(64),
      approved_command: "uvx",
      approved_args: ["db-old"],
      approved_env_keys: [],
    },
    { name: "web", state: "unsupported", command: "", args: [], env_keys: [], reason: "not supported: remote" },
    { name: "x", state: "approved", command: '<img src="https://x.example/a.png">', args: [], env_keys: [] },
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

test("lists every entry as text, and Approve, Remove and Reload ask the node", async () => {
  call.mockResolvedValue(list);
  await show();
  expect(call).toHaveBeenCalledWith("GET", "/api/projects/p1/mcp");
  const text = host.textContent ?? "";
  expect(text).toContain("npx -y @x/server 'a b'");
  expect(text).toContain("Environment: TOKEN (values not shown)");
  expect(text).toContain("uvx db-old");
  expect(text).toContain("not supported: remote");
  expect(host.querySelector("img")).toBeNull();
  expect(text).toContain('<img src="https://x.example/a.png">');
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

test("a command line quotes what would be split", () => {
  expect(commandLine("python3", ["x.py", "--name", "it's here", ""])).toBe("python3 x.py --name 'it'\\''s here' ''");
});
