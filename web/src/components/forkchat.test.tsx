// @vitest-environment happy-dom
// Forking a chat in the page (ADOPTION_PLAN C.2): "Fork from here" on a
// finished turn's end line where the server would take it and nowhere else;
// the divider where a fork's own conversation begins (a link to the parent,
// a plain line once the parent is gone); the note that a fork shares its
// parent's folder; an inherited turn's changes without an Undo.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import forkFixture from "../__fixtures__/fork.json";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { fold } from "../fold";
import type { Live } from "../live";
import type { Agent, Features, NodeInfo, RawEvent, SessionRow, SessionView } from "../types";

const session = vi.fn();
vi.mock("../api", () => {
  class HttpError extends Error {
    constructor(
      public status: number,
      message: string,
    ) {
      super(message);
    }
  }
  return {
    api: {
      features: () => Promise.resolve({ artifacts: { enabled: true, show: true, why: null }, media: { enabled: false, show: false, vision_images: false }, routines: { enabled: true } } as unknown as Features),
      prompt: vi.fn(),
      agents: () => Promise.resolve({ agents: [] }),
      me: () => Promise.resolve({ ok: true, app: "x", user: { username: "alex" }, workspace: null }),
      session: (id: string) => session(id),
    },
    call: vi.fn(() => Promise.resolve({ ok: true })),
    HttpError,
  };
});
const forkChat = vi.fn();
vi.mock("../apiPages", () => ({
  pageApi: { deleteSession: vi.fn(), approve: vi.fn(), turnDiff: vi.fn(() => Promise.resolve({ files: [] })), restoreTurn: vi.fn(), retryTurn: vi.fn(), setAgent: vi.fn(), forkChat: (...a: unknown[]) => forkChat(...a) },
}));
vi.mock("../lib/artifacts", async (original) => ({ ...(await original<typeof import("../lib/artifacts")>()), artifactsApi: { forSession: vi.fn(() => Promise.resolve([])), save: vi.fn() } }));
vi.mock("../lib/media", async (original) => ({ ...(await original<typeof import("../lib/media")>()), mediaApi: { list: vi.fn(() => Promise.resolve([])), get: vi.fn() } }));
const { default: Conversation } = await import("./Conversation");
const { HttpError } = await import("../api");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const fixture = forkFixture as unknown as { events: RawEvent[] };
const SID = fixture.events[0]!.session_id;
const PARENT = (fixture.events[0]!.payload as { forked: { parent: string } }).forked.parent;
const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: SID, seq, ts_ms: 0, kind, payload }) as RawEvent;

const ROW = { session_id: SID, node_id: "n", title: "Parent chat (fork)", cwd: "/w/proj", model: "m", created_ms: 0, updated_ms: 0, head_seq: 40, running: false, owner_id: "u", agent_id: null, profile: "agentic", incognito: false, parent_id: null, workspace_id: "w" } as SessionRow;
const NODE = { node_id: "n", hostname: "h", version: "1", connected: true, embedded: false, profile: "agentic", fork: true } as NodeInfo;
const agent = { id: "general", name: "general", description: "", builtin: true, default_model: null, main_model: null, fallbacks: [], unavailable: [], avatar: { shape: "circle", eyes: "round", accent: "blue" } } as unknown as Agent;

let host: HTMLElement;
let root: Root;
let current: SessionView;
const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => current, onTopic: vi.fn(() => () => {}) } as unknown as Live;
beforeEach(() => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  session.mockReset();
  forkChat.mockReset();
  location.hash = "";
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

async function render(events: RawEvent[], o: { row?: Partial<SessionRow> | null; node?: Partial<NodeInfo> | null } = {}) {
  current = fold(events);
  const row = o.row === null ? null : { ...ROW, ...o.row };
  const nodes = o.node === null ? [] : [{ ...NODE, ...o.node }];
  await act(async () => root.render(<Conversation live={live} sessionId={SID} row={row} nodes={nodes} agents={[agent]} models={[]} />));
  await act(() => new Promise((r) => setTimeout(r, 10)));
}

const plain = (extra: RawEvent[] = []): RawEvent[] => [
  ev(1, "session_created", { title: "t", cwd: "/w", model: "m", backend: "native", node_id: "n", approval_mode: "ask", profile: "agentic", agent_id: "general" }),
  ev(2, "turn_started", { turn_id: "t1" }),
  ev(3, "user_message", { turn_id: "t1", text: "first" }),
  ev(4, "assistant_message", { turn_id: "t1", message_id: "m1", text: "One." }),
  ev(5, "turn_ended", { turn_id: "t1", reason: "completed" }),
  ...extra,
];
const links = () => [...host.querySelectorAll(".fork-line button")].map((b) => b.textContent);

describe("Fork from here", () => {
  test("is on a finished turn's end line, calls the route and opens the new chat", async () => {
    forkChat.mockResolvedValue({ session_id: "22222222-2222-2222-2222-222222222222" });
    await render(plain());
    expect(links()).toEqual(["Fork from here"]);
    await act(async () => (host.querySelector(".fork-line button") as HTMLButtonElement).click());
    expect(forkChat).toHaveBeenCalledWith(SID, "t1");
    expect(location.hash).toBe("#/s/22222222-2222-2222-2222-222222222222");
  });

  test("is on a failed turn's end line too, next to Retry", async () => {
    await render([...plain().slice(0, 4), ev(5, "turn_ended", { turn_id: "t1", reason: "error" })]);
    expect(links()).toEqual(["Fork from here"]);
    expect(host.textContent).toContain("turn error");
  });

  test("says why when the route refuses, and stays", async () => {
    forkChat.mockRejectedValue(new Error("This chat can't be forked there: that turn is still running"));
    await render(plain());
    await act(async () => (host.querySelector(".fork-line button") as HTMLButtonElement).click());
    expect(host.querySelector(".fork-line .error-text")?.textContent).toContain("still running");
    expect(location.hash).toBe("");
  });

  test.each([
    ["a node that does not announce fork", { node: { fork: false } }],
    ["a node the page does not know", { node: null }],
    ["a chat the viewer cannot drive here", { row: { blocked: "your level is lower" } }],
    ["a sub-agent's chat", { row: { parent_id: "33333333-3333-3333-3333-333333333333" } }],
    ["a routine's own run", { row: { routine: { routine_id: "r", name: "n", run_id: "x", status: "done", seen: true, promoted: false, untrusted: false } } }],
    ["a chat the page has no row of", { row: null }],
  ] as [string, Parameters<typeof render>[1]][])("is not offered for %s", async (_, o) => {
    await render(plain(), o);
    expect(links()).toEqual([]);
  });

  test("is not offered for a turn a rewind took out of the conversation", async () => {
    const log = [
      ...plain(),
      ev(6, "turn_started", { turn_id: "t2" }),
      ev(7, "user_message", { turn_id: "t2", text: "second" }),
      ev(8, "turn_ended", { turn_id: "t2", reason: "completed" }),
      ev(9, "conversation_rewound", { message_seq: 7, turn_id: "t2" }),
    ];
    await render(log);
    expect(links()).toEqual(["Fork from here"]);
    expect(host.querySelectorAll(".fork-line").length).toBe(1);
  });
});

describe("a fork", () => {
  test("shows where its own conversation begins, with a link to the parent", async () => {
    session.mockResolvedValue({ title: "Parent chat" });
    await render(fixture.events);
    const d = host.querySelector("[data-testid=fork-divider]")!;
    expect(d.textContent).toBe("Forked from Parent chat here");
    expect(d.querySelector("a")?.getAttribute("href")).toBe(`#/s/${PARENT}`);
    expect(session).toHaveBeenCalledWith(PARENT);
  });

  test("says so plainly when the parent is gone, and does not break", async () => {
    session.mockRejectedValue(new HttpError(404, "unknown session"));
    await render(fixture.events);
    const d = host.querySelector("[data-testid=fork-divider]")!;
    expect(d.textContent).toBe("Forked from a chat that no longer exists");
    expect(d.querySelector("a")).toBeNull();
    expect(host.querySelector("[data-testid=fork-note]")?.textContent).toContain("the chat it was forked from");
  });

  test("says it shares its parent's folder, in an agentic chat only", async () => {
    session.mockResolvedValue({ title: "Parent chat" });
    await render(fixture.events);
    const note = host.querySelector("[data-testid=fork-note]")!;
    expect(note.textContent).toContain("shares its folder with Parent chat");
    const chat = fixture.events.map((e) => (e.kind === "session_created" ? { ...e, payload: { ...e.payload, profile: "chat" } } : e));
    await render(chat);
    expect(host.querySelector("[data-testid=fork-note]")).toBeNull();
    await render(plain());
    expect(host.querySelector("[data-testid=fork-note]")).toBeNull();
  });

  test("lists an inherited turn's changes without Undo, and its own turn's with it", async () => {
    session.mockResolvedValue({ title: "Parent chat" });
    await render(fixture.events);
    const inherited = [...host.querySelectorAll("[data-testid=inherited-changes]")].map((e) => e.textContent);
    expect(inherited).toEqual(["changed 1 file (before the fork)", "changed 1 file (before the fork)"]);
    const buttons = [...host.querySelectorAll(".undo button")].map((b) => b.textContent);
    // Only the fork's own turn can be undone or shown.
    expect(buttons.filter((b) => b === "Undo file changes").length).toBe(1);
    expect(buttons.filter((b) => b === "Show changes").length).toBe(1);
  });
});
