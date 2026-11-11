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
// The login key (P-1) is forgotten as the real api.ts does it.
vi.mock("./api", () => ({
  api: { me },
  HttpError: class extends Error {},
  login: { ended: () => {} },
  forgetLogin: () => localStorage.removeItem("harness.login"),
}));
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
  permissions: ["agentic", "routines"],
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
    // The login form is a lazy chunk: let it load.
    await act(async () => shell().onLogout());
    expect(signIn).not.toBeNull();
    me.mockResolvedValue(answer(ws("q", { personal: true })));
    await act(async () => signIn!());
    expect(shell().me.workspace!.workspace_id).toBe("q");
    expect(shell().gone).toBeNull();
  });
});

describe("drafts at sign-out (AUDIT L14)", () => {
  const stored = () => Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)!).sort();
  beforeEach(() => {
    localStorage.clear();
    sessionStorage.clear();
  });
  const seed = () => {
    localStorage.setItem("harness.draft.u.s1", "alex's unsent words");
    sessionStorage.setItem("harness.ui.m1#b1", '{"other":"an answer"}');
    localStorage.setItem("harness.sidebarCollapsed", "1");
    localStorage.setItem("harness.login", "k".repeat(64));
  };

  test("logging out clears every draft in this browser", async () => {
    await start(ws("p", { personal: true }));
    seed();
    await act(async () => shell().onLogout());
    expect(stored()).toEqual(["harness.sidebarCollapsed"]);
    expect(sessionStorage.length).toBe(0);
  });

  test("so does a login the server ended (401)", async () => {
    await start(ws("p", { personal: true }));
    seed();
    const { HttpError } = await import("./api");
    me.mockRejectedValue(Object.assign(new HttpError(401, "not logged in"), { status: 401 }));
    await act(() => shell().onMeChanged());
    expect(signIn).not.toBeNull();
    expect(stored()).toEqual(["harness.sidebarCollapsed"]);
  });

  test("so does a 401 from any request (P-1: the login key is gone with it)", async () => {
    await start(ws("p", { personal: true }));
    seed();
    const { login } = await import("./api");
    await act(async () => login.ended());
    expect(signIn).not.toBeNull();
    expect(stored()).toEqual(["harness.sidebarCollapsed"]);
  });

  test("and a fresh load that is not signed in", async () => {
    seed();
    me.mockRejectedValue(new Error("offline"));
    await act(async () => root.render(<App />));
    expect(stored()).toEqual(["harness.sidebarCollapsed"]);
  });
});
