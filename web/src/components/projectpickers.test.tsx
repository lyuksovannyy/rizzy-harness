// @vitest-environment happy-dom
// The project page's two chat pickers see more than the sidebar loaded
// (review F5 of the paged chat list, FIX_PLAN 4.5 slice 4): "Add a chat"
// pages the caller's chats in no project from the server, and a board
// card's editor always offers the chat the card links, from the board.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Board, ProjectDetail } from "../lib/projects";
import type { Features, SessionRow } from "../types";

vi.mock("../api", () => ({
  api: { features: () => Promise.resolve({ artifacts: { enabled: false, show: false, why: null }, media: { vision_images: false } } as unknown as Features) },
  call: vi.fn(),
  HttpError: Error,
}));
const get = vi.fn<(id: string) => Promise<ProjectDetail>>();
const looseChats = vi.fn<(workspace: string, after?: string | null, limit?: number) => Promise<{ sessions: SessionRow[]; next: string | null }>>();
const board = vi.fn<(id: string) => Promise<Board>>();
vi.mock("../lib/projects", async (original) => {
  const m = await original<typeof import("../lib/projects")>();
  return {
    ...m,
    projectsApi: {
      ...m.projectsApi,
      get: (id: string) => get(id),
      looseChats: (w: string, after?: string | null, limit?: number) => looseChats(w, after, limit),
      board: (id: string) => board(id),
      templates: () => Promise.resolve({ templates: [] }),
    },
  };
});
const { default: Project } = await import("./Project");
const { nudgeSessions } = await import("../lib/sessionsNudge");

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
const row = (n: number, project_id: string | null = null) =>
  ({ session_id: `s-${n}`, title: `chat ${n}`, updated_ms: n, project_id }) as unknown as SessionRow;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  get.mockResolvedValue(project);
  looseChats.mockReset();
  board.mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function show(tab: "chats" | "board", chats: SessionRow[]) {
  await act(async () =>
    root.render(
      <Project
        id={PID}
        tab={tab}
        version={0}
        memoryVersion={0}
        chats={chats}
        agents={[]}
        agentDefaults={{ chat: "", agentic: null }}
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

const options = (select: HTMLSelectElement) => [...select.options].map((o) => o.textContent);

describe("Add a chat", () => {
  test("offers the server's pages of chats in no project, not the sidebar's loaded rows", async () => {
    looseChats.mockImplementation((_, after) =>
      Promise.resolve(after ? { sessions: [row(1)], next: null } : { sessions: [row(3), row(2)], next: "c1" }),
    );
    // The sidebar loaded one loose chat that the server does not list
    // first: it is not what the picker shows.
    await show("chats", [row(9)]);
    expect(looseChats).toHaveBeenCalledWith("w1", null, 50);
    const select = host.querySelector<HTMLSelectElement>(".field-inline select")!;
    expect(options(select)).toEqual(["Choose one of your chats…", "chat 3", "chat 2"]);
    const older = [...host.querySelectorAll("button")].find((b) => b.textContent === "Older chats")!;
    await act(async () => older.click());
    expect(looseChats).toHaveBeenLastCalledWith("w1", "c1", 50);
    expect(options(select)).toEqual(["Choose one of your chats…", "chat 3", "chat 2", "chat 1"]);
    expect([...host.querySelectorAll("button")].some((b) => b.textContent === "Older chats")).toBe(false);
  });
});

describe("Add a chat stays current", () => {
  test("a sessions_changed nudge refetches it from the top", async () => {
    looseChats.mockResolvedValue({ sessions: [row(2), row(1)], next: null });
    await show("chats", []);
    const select = host.querySelector<HTMLSelectElement>(".field-inline select")!;
    expect(options(select)).toEqual(["Choose one of your chats…", "chat 2", "chat 1"]);
    looseChats.mockResolvedValue({ sessions: [row(7), row(2), row(1)], next: null });
    await act(async () => nudgeSessions());
    expect(looseChats).toHaveBeenLastCalledWith("w1", null, 50);
    expect(options(select)).toEqual(["Choose one of your chats…", "chat 7", "chat 2", "chat 1"]);
  });

  test("a refetch asked during a next page waits for it, and nothing lands twice or is lost", async () => {
    const pending: ((p: { sessions: SessionRow[]; next: string | null }) => void)[] = [];
    looseChats.mockImplementation(() => new Promise((r) => pending.push(r)));
    await show("chats", []);
    await act(async () => pending.shift()!({ sessions: [row(3), row(2)], next: "c1" }));
    const older = () => [...host.querySelectorAll("button")].find((b) => b.textContent === "Older chats");
    await act(async () => older()!.click());
    expect(looseChats).toHaveBeenCalledTimes(2);
    // While the next page is on its way: two nudges, and no second "Older".
    await act(async () => {
      nudgeSessions();
      nudgeSessions();
    });
    expect(older()).toBeUndefined();
    expect(looseChats).toHaveBeenCalledTimes(2);
    await act(async () => pending.shift()!({ sessions: [row(1)], next: null }));
    // Then exactly one refetch from the top.
    expect(looseChats).toHaveBeenCalledTimes(3);
    expect(looseChats).toHaveBeenLastCalledWith("w1", null, 50);
    await act(async () => pending.shift()!({ sessions: [row(9), row(3), row(2), row(1)], next: null }));
    const select = host.querySelector<HTMLSelectElement>(".field-inline select")!;
    expect(options(select)).toEqual(["Choose one of your chats…", "chat 9", "chat 3", "chat 2", "chat 1"]);
    expect(pending).toHaveLength(0);
  });
});

describe("Add a chat after the page is gone", () => {
  test("a refetch queued before unmount never starts", async () => {
    const pending: ((p: { sessions: SessionRow[]; next: string | null }) => void)[] = [];
    looseChats.mockImplementation(() => new Promise((r) => pending.push(r)));
    await show("chats", []);
    expect(looseChats).toHaveBeenCalledTimes(1);
    // A nudge while the first page is on its way queues one refetch.
    await act(async () => nudgeSessions());
    act(() => root.unmount());
    await act(async () => pending.shift()!({ sessions: [row(1)], next: null }));
    await act(async () => nudgeSessions());
    expect(looseChats).toHaveBeenCalledTimes(1);
    root = createRoot(host);
  });
});

describe("a board card's chat", () => {
  test("is offered even when the sidebar has not loaded it", async () => {
    looseChats.mockResolvedValue({ sessions: [], next: null });
    board.mockResolvedValue({
      columns: [
        {
          column_id: "c1",
          name: "Todo",
          done: false,
          cards: [{ card_id: "k1", column_id: "c1", title: "Fix it", notes: "", position: 0, chat: { session_id: "s-old", title: "an old chat" }, created_by: null, agent: null, updated_ms: 0 }],
        },
      ],
    } as unknown as Board);
    await show("board", [row(5, PID)]);
    await act(async () => host.querySelector<HTMLButtonElement>(".card-open")!.click());
    const select = [...host.querySelectorAll<HTMLSelectElement>(".board-card.editing select")].at(-1)!;
    expect(options(select)).toEqual(["None", "chat 5", "an old chat"]);
    expect(select.value).toBe("s-old");
  });
});
