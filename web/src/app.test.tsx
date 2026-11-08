// @vitest-environment happy-dom
// App, against a mocked /api/me and a stand-in Shell: `me` carries the
// active workspace the server sends beside the user, a reload can keep the
// workspace the tab shows, and the "no longer in" line goes with the account
// that read it.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Me, WorkspaceSummary } from "./types";

type ShellProps = {
  me: Me;
  onLogout: () => void;
  onMeChanged: (keep?: WorkspaceSummary) => Promise<void>;
  gone: string | null;
  onGone: (name: string | null) => void;
};

const me = vi.fn();
const shells: ShellProps[] = [];
let signIn: (() => void) | null = null;
vi.mock("./api", () => ({ api: { me }, HttpError: class extends Error {} }));
vi.mock("./components/Shell", () => ({
  Shell: (p: ShellProps) => {
    shells.push(p);
    return null;
  },
}));
vi.mock("./components/Login", () => ({
  Login: (p: { onDone: () => void }) => {
    signIn = p.onDone;
    return null;
  },
}));
const { App } = await import("./App");

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
const user = { user_id: "u", username: "alex", role: "member" as const, can_agentic: true, must_change_password: false };
// What GET /api/me answers: the active workspace sits beside the user, not in it.
const answer = (workspace: WorkspaceSummary | null) => ({ ok: true, app: "rizzy-harness", user, workspace });
const shell = () => shells.at(-1)!;

let host: HTMLElement;
let root: Root;
beforeEach(async () => {
  me.mockReset();
  shells.length = 0;
  signIn = null;
  host = document.createElement("div");
  root = createRoot(host);
});
afterEach(() => act(() => root.unmount()));

const start = async (workspace: WorkspaceSummary | null) => {
  me.mockResolvedValue(answer(workspace));
  await act(async () => root.render(<App />));
};

describe("App", () => {
  test("me carries the workspace /api/me sends beside the user", async () => {
    await start(ws("acme"));
    expect(shell().me).toEqual({ ...user, workspace: ws("acme") });
  });

  test("a reload keeps the tab's workspace when told to, else takes the server's", async () => {
    await start(ws("acme"));
    // Another tab made Beta active meanwhile.
    me.mockResolvedValue(answer(ws("beta")));
    const fresh = ws("acme", { role: "admin", members: 3 });
    await act(() => shell().onMeChanged(fresh));
    expect(shell().me.workspace).toEqual(fresh);
    await act(() => shell().onMeChanged());
    expect(shell().me.workspace).toEqual(ws("beta"));
  });

  test("the no-longer-in line goes on logout, so the next account never reads it", async () => {
    await start(ws("p", { personal: true }));
    act(() => shell().onGone("Acme"));
    expect(shell().gone).toBe("Acme");
    act(() => shell().onLogout());
    expect(signIn).not.toBeNull();
    me.mockResolvedValue(answer(ws("q", { personal: true })));
    await act(async () => signIn!());
    expect(shell().me.workspace!.workspace_id).toBe("q");
    expect(shell().gone).toBeNull();
  });
});
