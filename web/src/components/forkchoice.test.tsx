// @vitest-environment happy-dom
// "Fork from here" where the node can give the fork a folder of its own
// (C.2 part 2): the inline choice (its default, its guard, Escape, a click
// outside, what is shown while the node copies, the node's refusal), the
// plain fork where it cannot, the note in the fork for each choice, and the
// delete dialog that says the folder stays.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import forkFixture from "../__fixtures__/fork.json";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { fold } from "../fold";
import type { Live } from "../live";
import type { Features, NodeInfo, RawEvent, SessionRow, SessionView } from "../types";

const session = vi.fn();
vi.mock("../api", () => ({
  api: {
    features: () => Promise.resolve({ artifacts: { enabled: false, show: false, why: null }, media: { enabled: false, show: false, vision_images: false }, routines: { enabled: true } } as unknown as Features),
    prompt: vi.fn(),
    agents: () => Promise.resolve({ agents: [] }),
    session: (id: string) => session(id),
  },
  call: vi.fn(() => Promise.resolve({ ok: true })),
  HttpError: class extends Error {},
}));
const forkChat = vi.fn();
const deleteSession = vi.fn();
vi.mock("../apiPages", () => ({
  pageApi: { deleteSession: (...a: unknown[]) => deleteSession(...a), approve: vi.fn(), turnDiff: vi.fn(() => Promise.resolve({ files: [] })), restoreTurn: vi.fn(), retryTurn: vi.fn(), setAgent: vi.fn(), forkChat: (...a: unknown[]) => forkChat(...a) },
}));
vi.mock("../lib/media", async (original) => ({ ...(await original<typeof import("../lib/media")>()), mediaApi: { list: vi.fn(() => Promise.resolve({ items: [], next: null })), get: vi.fn() } }));
// The guard is real but short here.
vi.mock("./ApprovalChoice", async (original) => ({ ...(await original<typeof import("./ApprovalChoice")>()), GUARD_MS: 30 }));
const { default: Conversation } = await import("./Conversation");
const { DeleteChat } = await import("./DeleteChat");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const fixture = forkFixture as unknown as { events: RawEvent[] };
const SID = fixture.events[0]!.session_id;
const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: SID, seq, ts_ms: 0, kind, payload }) as RawEvent;
const ROW = { session_id: SID, node_id: "n", title: "Chat", cwd: "/w/proj", model: "m", created_ms: 0, updated_ms: 0, head_seq: 40, running: false, owner_id: "u", agent_id: null, profile: "agentic", incognito: false, parent_id: null, workspace_id: "w" } as SessionRow;
const NODE = { node_id: "n", hostname: "h", version: "1", connected: true, embedded: false, profile: "agentic", fork: true, fork_own: true } as NodeInfo;

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
  deleteSession.mockReset();
  location.hash = "";
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

async function render(events: RawEvent[], o: { node?: Partial<NodeInfo>; row?: Partial<SessionRow> } = {}) {
  current = fold(events);
  await act(async () => root.render(<Conversation live={live} sessionId={SID} row={{ ...ROW, ...o.row }} nodes={[{ ...NODE, ...o.node }]} agents={[]} models={[]} />));
  await act(() => new Promise((r) => setTimeout(r, 10)));
}
const plain = (profile = "agentic"): RawEvent[] => [
  ev(1, "session_created", { title: "t", cwd: "/w", model: "m", backend: "native", node_id: "n", approval_mode: "ask", profile }),
  ev(2, "turn_started", { turn_id: "t1" }),
  ev(3, "user_message", { turn_id: "t1", text: "first" }),
  ev(4, "assistant_message", { turn_id: "t1", message_id: "m1", text: "One." }),
  ev(5, "turn_ended", { turn_id: "t1", reason: "completed" }),
];
const opener = () => host.querySelector<HTMLButtonElement>(".fork-line > button")!;
const panel = () => host.querySelector<HTMLElement>(".fork-panel");
const choice = (text: string) => [...host.querySelectorAll<HTMLButtonElement>(".fork-panel button")].find((b) => b.textContent === text)!;
const wait = (ms = 60) => act(() => new Promise((x) => setTimeout(x, ms)));
const click = (el: Element) => act(async () => (el as HTMLElement).click());
const OWN = "In its own copy of the repository";
const SAME = "In the same folder";

describe("the choice", () => {
  test("opens inline with the own copy as the default and says what each is", async () => {
    await render(plain());
    expect(panel()).toBeNull();
    await click(opener());
    expect(forkChat).not.toHaveBeenCalled();
    expect(panel()).not.toBeNull();
    expect(opener().getAttribute("aria-expanded")).toBe("true");
    expect(choice(OWN).className).toContain("primary");
    expect(choice(SAME).className).not.toContain("primary");
    const text = panel()!.textContent!;
    expect(text).toContain("projects folder");
    expect(text).toContain("Files git ignores");
    expect(text).toContain(".env");
    expect(text).toContain("not copied");
    expect(text).toContain("share the files");
  });

  test("the buttons answer nothing for a moment (a double click or the opening key does not choose)", async () => {
    forkChat.mockResolvedValue({ session_id: "22222222-2222-2222-2222-222222222222" });
    await render(plain());
    await click(opener());
    await click(choice(OWN));
    expect(forkChat).not.toHaveBeenCalled();
    await wait();
    await click(choice(OWN));
    expect(forkChat).toHaveBeenCalledTimes(1);
  });

  test("the own copy asks for checkout own, shows that it is working, and opens the new chat", async () => {
    let done!: (v: { session_id: string }) => void;
    forkChat.mockReturnValue(new Promise((r) => (done = r)));
    await render(plain());
    await click(opener());
    await wait();
    await click(choice(OWN));
    expect(forkChat).toHaveBeenCalledWith(SID, "t1", "own");
    expect(panel()!.textContent).toContain("Making the copy");
    expect(choice(OWN).disabled).toBe(true);
    expect(choice(SAME).disabled).toBe(true);
    // While it works the panel cannot be closed: the chat opens when it ends.
    await act(async () => panel()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(panel()).not.toBeNull();
    await act(async () => document.body.dispatchEvent(new Event("pointerdown", { bubbles: true })));
    expect(panel()).not.toBeNull();
    await act(async () => done({ session_id: "22222222-2222-2222-2222-222222222222" }));
    expect(location.hash).toBe("#/s/22222222-2222-2222-2222-222222222222");
  });

  test("the same folder asks for checkout shared", async () => {
    forkChat.mockResolvedValue({ session_id: "33333333-3333-3333-3333-333333333333" });
    await render(plain());
    await click(opener());
    await wait();
    await click(choice(SAME));
    expect(forkChat).toHaveBeenCalledWith(SID, "t1", "shared");
    expect(location.hash).toBe("#/s/33333333-3333-3333-3333-333333333333");
  });

  test("Escape closes it also when the focus has left the panel", async () => {
    await render(plain());
    await click(opener());
    expect(panel()).not.toBeNull();
    await act(async () => document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(panel()).toBeNull();
  });

  test("Escape and a click outside close it, Escape puts the focus back on the link", async () => {
    await render(plain());
    await click(opener());
    await act(async () => panel()!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(panel()).toBeNull();
    expect(document.activeElement).toBe(opener());
    await click(opener());
    expect(panel()).not.toBeNull();
    await act(async () => document.body.dispatchEvent(new Event("pointerdown", { bubbles: true })));
    expect(panel()).toBeNull();
    expect(forkChat).not.toHaveBeenCalled();
  });

  test("a refusal of the own copy stays in the panel with the node's reason and makes the other choice the default", async () => {
    forkChat.mockRejectedValue(new Error("This chat can't be forked there: this chat's folder is not a git repository; the fork can share this chat's folder instead"));
    await render(plain());
    await click(opener());
    await wait();
    await click(choice(OWN));
    expect(panel()!.querySelector("[role=alert]")?.textContent).toContain("not a git repository");
    expect(location.hash).toBe("");
    expect(choice(SAME).className).toContain("primary");
    expect(choice(OWN).className).not.toContain("primary");
    expect(choice(OWN).disabled).toBe(false);
    forkChat.mockResolvedValue({ session_id: "44444444-4444-4444-4444-444444444444" });
    await click(choice(SAME));
    expect(forkChat).toHaveBeenLastCalledWith(SID, "t1", "shared");
  });

  test("no choice where the node cannot give a folder, or the chat has none: it forks at once as before", async () => {
    forkChat.mockResolvedValue({ session_id: "55555555-5555-5555-5555-555555555555" });
    await render(plain(), { node: { fork_own: false } });
    await click(opener());
    expect(panel()).toBeNull();
    expect(forkChat).toHaveBeenCalledWith(SID, "t1");
    forkChat.mockClear();
    await render(plain("chat"), { row: { profile: "chat" } as Partial<SessionRow> });
    await click(opener());
    expect(panel()).toBeNull();
    expect(forkChat).toHaveBeenCalledWith(SID, "t1");
  });
});

describe("in the fork", () => {
  const forkLog = (own: boolean): RawEvent[] => {
    const events = JSON.parse(JSON.stringify(fixture.events)) as RawEvent[];
    (events[0]!.payload as { forked: { own_folder?: boolean }; cwd: string }).forked.own_folder = own;
    (events[0]!.payload as { cwd: string }).cwd = own ? "/projects/proj-fork-ab12cd34" : "/w/proj";
    return events;
  };

  test("the shared-folder note shows only for a fork in the same folder", async () => {
    session.mockResolvedValue({ title: "Parent chat" });
    await render(forkLog(false));
    const note = host.querySelector("[data-testid=fork-note]")!;
    expect(note.textContent).toContain("shares its folder");
    expect(note.textContent).not.toContain("own copy");
  });

  test("a fork with a folder of its own says so, shows the folder and what was not copied", async () => {
    session.mockResolvedValue({ title: "Parent chat" });
    await render(forkLog(true));
    const note = host.querySelector("[data-testid=fork-note]")!;
    expect(note.textContent).toContain("its own copy of the repository");
    expect(note.textContent).toContain("/projects/proj-fork-ab12cd34");
    expect(note.textContent).toContain("ignores");
    expect(note.textContent).toContain("keeps the folder");
    expect(note.textContent).not.toContain("shares its folder");
    // The header shows the folder as it shows any chat's.
    expect(host.querySelector(".conv-meta")?.textContent).toContain("proj-fork-ab12cd34");
  });

  test("the note still shows when the parent is gone", async () => {
    session.mockRejectedValue(new Error("gone"));
    await render(forkLog(true));
    expect(host.querySelector("[data-testid=fork-note]")?.textContent).toContain("its own copy");
  });
});

describe("the delete dialog", () => {
  const open = async (ownFolder?: string) => {
    await act(async () => root.render(<DeleteChat sessionId="abc" workspaceId={null} incognito ownFolder={ownFolder} onClose={vi.fn()} />));
  };

  test("says where a fork's own folder is and that it stays", async () => {
    await open("/projects/proj-fork-ab12cd34");
    const line = host.querySelector("[data-testid=delete-keeps-folder]")!;
    expect(line.textContent).toContain("/projects/proj-fork-ab12cd34");
    expect(line.textContent).toContain("keeps that folder on disk");
  });

  test("says nothing of a folder for a chat that has none of its own", async () => {
    await open(undefined);
    expect(host.querySelector("[data-testid=delete-keeps-folder]")).toBeNull();
  });
});
