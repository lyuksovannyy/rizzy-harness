// @vitest-environment happy-dom
// The list hook against a fake API: the open chat's own row is refreshed
// with the list when no page holds it (review F4), and a nudge that comes
// while a project group's first page is on its way is not lost for that
// group (review F7).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { SessionRow } from "../types";

type Got = { sessions: SessionRow[]; children: SessionRow[]; next: string | null };
const sessions = vi.fn<(ws: string | null, o: Record<string, unknown>) => Promise<Got>>();
const session = vi.fn<(id: string) => Promise<SessionRow>>();
vi.mock("../api", () => ({
  api: { sessions: (ws: string | null, o: Record<string, unknown>) => sessions(ws, o), session: (id: string) => session(id) },
  HttpError: class extends Error {
    constructor(public status: number) {
      super(String(status));
    }
  },
}));
vi.mock("./drafts", () => ({ pruneDrafts: () => {} }));
const { useChatList } = await import("./useChatList");
const { HttpError } = await import("../api");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const row = (id: string, more: Partial<SessionRow> = {}): SessionRow => ({
  session_id: id,
  node_id: "master",
  title: `chat ${id}`,
  cwd: "",
  model: "m",
  created_ms: 0,
  updated_ms: 10,
  head_seq: 1,
  running: false,
  owner_id: "u",
  agent_id: null,
  profile: "chat",
  incognito: false,
  parent_id: null,
  workspace_id: "w",
  ...more,
});
const page = (rows: SessionRow[]): Got => ({ sessions: rows, children: [], next: null });

let got: ReturnType<typeof useChatList> | null = null;
function Probe(p: { extra: SessionRow | null; keep: string | null }) {
  got = useChatList("w", () => {}, p);
  return null;
}
let host: HTMLElement;
let root: Root;
beforeEach(() => {
  got = null;
  sessions.mockReset();
  session.mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});
const settle = () => act(async () => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
});

describe("useChatList", () => {
  test("a rename of an old open chat shows after a nudge", async () => {
    sessions.mockResolvedValue(page([row("new", { updated_ms: 100 })]));
    const old = row("old", { title: "Before", updated_ms: 1 });
    await act(async () => root.render(<Probe extra={old} keep={null} />));
    await act(async () => got!.refresh());
    await settle();
    expect(got!.sessions!.find((r) => r.session_id === "old")?.title).toBe("Before");
    session.mockResolvedValue({ ...old, title: "After" });
    await act(async () => got!.refresh());
    await settle();
    expect(session).toHaveBeenCalledWith("old");
    expect(got!.sessions!.find((r) => r.session_id === "old")?.title).toBe("After");
  });

  // Review R2-3: a 404 means the open chat is gone; a network error does not.
  test("a deleted open chat leaves the list; a failed request keeps it", async () => {
    sessions.mockResolvedValue(page([row("new", { updated_ms: 100 })]));
    const old = row("old", { updated_ms: 1 });
    await act(async () => root.render(<Probe extra={old} keep={null} />));
    await act(async () => got!.refresh());
    await settle();
    session.mockRejectedValue(new TypeError("Failed to fetch"));
    await act(async () => got!.refresh());
    await settle();
    expect(got!.sessions!.some((r) => r.session_id === "old")).toBe(true);
    session.mockRejectedValue(new (HttpError as unknown as new (s: number) => Error)(404));
    await act(async () => got!.refresh());
    await settle();
    expect(got!.sessions!.some((r) => r.session_id === "old")).toBe(false);
  });

  test("an open chat a page holds is not looked up again", async () => {
    const open = row("open");
    sessions.mockResolvedValue(page([open]));
    await act(async () => root.render(<Probe extra={open} keep={null} />));
    await act(async () => got!.refresh());
    await settle();
    await act(async () => got!.refresh());
    await settle();
    expect(session).not.toHaveBeenCalled();
  });

  test("a nudge during a group's first fetch refetches that group once it lands", async () => {
    let land: (g: Got) => void = () => {};
    sessions.mockImplementation((_ws, o) =>
      o.project ? new Promise<Got>((r) => (land = r)) : Promise.resolve(page([row("a", { updated_ms: 100 })])),
    );
    await act(async () => root.render(<Probe extra={null} keep="p1" />));
    await act(async () => got!.refresh());
    await settle();
    const groupCalls = () => sessions.mock.calls.filter(([, o]) => o.project === "p1").length;
    expect(groupCalls()).toBe(1);
    // The first page lands after the nudge: one more fetch for the group.
    sessions.mockImplementation((_ws, o) =>
      Promise.resolve(o.project ? page([row("g", { project_id: "p1", title: "fresh" })]) : page([row("a", { updated_ms: 100 })])),
    );
    await act(async () => land(page([row("g", { project_id: "p1", title: "stale" })])));
    await settle();
    expect(groupCalls()).toBe(2);
    expect(got!.sessions!.find((r) => r.session_id === "g")?.title).toBe("fresh");
  });
});
