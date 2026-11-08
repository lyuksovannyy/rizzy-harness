// @vitest-environment happy-dom
// Shell's live wiring, with the socket, the API and the screens stubbed: a
// re-render with new callbacks from App keeps the one Live connection open
// (a closed Live never reconnects), and a workspaces_changed keeps the tab in
// its own workspace, reloading its chats, until the user is out of it.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Me, WorkspaceList, WorkspaceSummary } from "../types";

const lives: { close: () => void; onWorkspacesChanged: () => void }[] = [];
vi.mock("../live", () => ({
  Live: class {
    state = "live";
    version = 0;
    onSessionsChanged = () => {};
    onNodesChanged = () => {};
    onMemoryChanged = () => {};
    onWorkspacesChanged = () => {};
    close = vi.fn();
    constructor() {
      lives.push(this);
    }
    subscribe() {
      return () => {};
    }
  },
}));
// What the tests look at; every other call stays pending.
const stubs = { sessions: vi.fn(), workspaces: vi.fn() };
vi.mock("../api", () => ({
  api: new Proxy(stubs, {
    get: (o, k: string) => (o as Record<string, unknown>)[k] ?? (() => new Promise(() => {})),
  }),
  HttpError: class extends Error {},
}));
vi.mock("./Home", () => ({ Home: () => null }));
vi.mock("./Sidebar", () => ({ Sidebar: () => null }));
const { Shell } = await import("./Shell");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ws = (id: string, more: Partial<WorkspaceSummary> = {}): WorkspaceSummary => ({
  workspace_id: id,
  name: id,
  personal: false,
  role: "member",
  permissions: ["agentic"],
  members: 2,
  ...more,
});
const personal = ws("p", { personal: true, role: "owner", members: 1 });
const acme = ws("acme", { name: "Acme" });
const me: Me = { user_id: "u", username: "alex", role: "member", can_agentic: true, must_change_password: false, workspace: acme };
const list = (...workspaces: WorkspaceSummary[]): WorkspaceList => ({ workspaces, active: "p", can_create: true });
// New functions on every call, as an App re-render could hand down.
const props = () => ({
  me,
  onLogout: vi.fn(),
  onMeChanged: vi.fn((_keep?: WorkspaceSummary) => Promise.resolve()),
  gone: null,
  onGone: vi.fn(),
});

let host: HTMLElement;
let root: Root;
let mounted = false;
beforeEach(() => {
  lives.length = 0;
  stubs.sessions.mockReset().mockResolvedValue({ sessions: [] });
  stubs.workspaces.mockReset().mockResolvedValue(list(personal, acme));
  history.replaceState(null, "", "#/");
  host = document.createElement("div");
  root = createRoot(host);
  mounted = true;
});
afterEach(() => {
  if (mounted) act(() => root.unmount());
});
const render = (p: ReturnType<typeof props>) => act(async () => root.render(<Shell {...p} />));

describe("Shell", () => {
  test("new callbacks from App keep the live connection; only unmounting closes it", async () => {
    await render(props());
    await render(props());
    expect(lives).toHaveLength(1);
    expect(lives[0]!.close).not.toHaveBeenCalled();
    act(() => root.unmount());
    mounted = false;
    expect(lives[0]!.close).toHaveBeenCalledTimes(1);
  });

  test("workspaces_changed keeps the tab's workspace, refreshed, and reloads its chats", async () => {
    const p = props();
    await render(p);
    const before = stubs.sessions.mock.calls.length;
    const promoted = ws("acme", { name: "Acme", role: "admin", permissions: ["agentic", "settings", "members"], members: 3 });
    stubs.workspaces.mockResolvedValue(list(personal, promoted));
    await act(async () => lives[0]!.onWorkspacesChanged());
    expect(stubs.sessions.mock.calls.length).toBe(before + 1);
    expect(stubs.sessions).toHaveBeenLastCalledWith("acme");
    expect(p.onMeChanged).toHaveBeenLastCalledWith(promoted);
    expect(p.onGone).not.toHaveBeenCalled();
  });

  test("taken out of it, the tab falls back to the server's and then says so", async () => {
    const p = props();
    await render(p);
    stubs.workspaces.mockResolvedValue(list(personal));
    await act(async () => lives[0]!.onWorkspacesChanged());
    expect(p.onMeChanged).toHaveBeenLastCalledWith();
    expect(p.onGone).toHaveBeenCalledWith("Acme");
    expect(p.onMeChanged.mock.invocationCallOrder.at(-1)!).toBeLessThan(p.onGone.mock.invocationCallOrder.at(-1)!);
  });
});
