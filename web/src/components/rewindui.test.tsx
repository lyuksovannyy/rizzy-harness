// @vitest-environment happy-dom
// "Rewind to here" and the rewound block (X.46): the action only where the
// server would accept (the fold's data), the inline choice and the bodies it
// sends, the refusal with "Rewind anyway", the message back in the box (only
// into an empty one; what cannot be sent again is said), the block of
// rewound turns with its Undo, and a rewind or its undo arriving after the
// chat is on screen (the view is mutated in place).
import { act } from "react";
import { apply, fold } from "../fold";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { Features, RawEvent, SessionRow, SessionView } from "../types";

const prompted = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
vi.mock("../api", () => ({
  api: {
    features: () => Promise.resolve({ artifacts: { enabled: false, show: false, why: null }, media: { enabled: false, show: false, vision_images: false }, routines: { enabled: true } } as unknown as Features),
    sessions: () => Promise.resolve([]),
    prompt: (...a: unknown[]) => prompted(...a),
    agents: () => Promise.resolve({ agents: [] }),
  },
  call: () => Promise.resolve({ ok: true }),
  HttpError: Error,
}));
const rewind = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
const undoRewind = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
vi.mock("../apiPages", () => ({
  pageApi: {
    deleteSession: vi.fn(),
    approve: vi.fn(),
    turnDiff: vi.fn(() => Promise.resolve({ files: [] })),
    restoreTurn: vi.fn(() => Promise.resolve({ ok: true })),
    retryTurn: vi.fn(() => Promise.resolve({ ok: true })),
    rewind: (...a: unknown[]) => rewind(...a),
    undoRewind: (...a: unknown[]) => undoRewind(...a),
    setAgent: vi.fn(),
    cancel: vi.fn(() => Promise.resolve({ ok: true })),
  },
}));
// The guard is real but short here.
vi.mock("./ApprovalChoice", async (original) => ({ ...(await original<typeof import("./ApprovalChoice")>()), GUARD_MS: 30 }));
const { default: Conversation } = await import("./Conversation");
const { outbox } = await import("../lib/outbox");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SID = "11111111-1111-1111-1111-111111111111";
const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: SID, seq, ts_ms: 0, kind, payload }) as RawEvent;
const created = ev(1, "session_created", { title: "Nightly", cwd: "/src", model: "demo::mock", backend: "native", node_id: "n1", approval_mode: "ask", profile: "agentic" });
// Seqs: t1 = 2..5 (message 3), t2 = 6..9 (message 7), t3 = 10..13 (message 11).
const turn = (at: number, id: string, text: string, extra: object = {}, end: object = {}): RawEvent[] => [
  ev(at, "turn_started", { turn_id: id }),
  ev(at + 1, "user_message", { turn_id: id, text, ...extra }),
  ev(at + 2, "assistant_message", { turn_id: id, message_id: `m-${id}`, text: `${id} answer` }),
  ev(at + 3, "turn_ended", { turn_id: id, reason: "completed", ...end }),
];
const two = [created, ...turn(2, "t1", "first"), ...turn(6, "t2", "second")];
const row = (over: Partial<SessionRow> = {}): SessionRow =>
  ({ session_id: SID, node_id: "n1", title: "Nightly", cwd: "/src", model: "demo::mock", created_ms: 0, updated_ms: 0, head_seq: 9, running: false, owner_id: "u1", agent_id: "general", profile: "agentic", incognito: false, parent_id: null, workspace_id: "w1", ...over }) as SessionRow;

let host: HTMLElement;
let root: Root;
let current: SessionView;
const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => current, onTopic: vi.fn(() => () => {}) } as unknown as Live;
beforeEach(() => {
  outbox.reset();
  // A message put back in the box is kept as a draft: not for the next test.
  localStorage.clear();
  rewind.mockReset().mockResolvedValue({ ok: true });
  undoRewind.mockReset().mockResolvedValue({ ok: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});
const draw = (r: SessionRow | null = row()) => act(async () => root.render(<Conversation live={live} sessionId={SID} row={r} nodes={[]} agents={[]} models={[]} />));
const show = async (events: RawEvent[], r: SessionRow | null = row()) => {
  current = fold(events);
  await draw(r);
  await act(() => new Promise((x) => setTimeout(x, 0)));
};
const actions = () => [...host.querySelectorAll<HTMLButtonElement>("button.rewind-action")];
const named = (text: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === text);
const wait = (ms = 60) => act(() => new Promise((x) => setTimeout(x, ms)));
const click = (el: Element | undefined) => act(async () => (el as HTMLElement).click());
const textarea = () => host.querySelector("textarea")!;
const type = async (value: string) => {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea(), value);
    textarea().dispatchEvent(new Event("input", { bubbles: true }));
  });
};
/** The event the node writes at `at` after a rewind to `message_seq`. */
const rewound = (at: number, message_seq: number, turn_id: string, files = false) => ev(at, "conversation_rewound", { message_seq, turn_id, files });

describe("the action: only where the server would accept", () => {
  test("every message of the person's that opened a turn, in an idle chat the viewer owns", async () => {
    await show(two);
    expect(actions().map((a) => a.textContent?.trim())).toEqual(["Rewind to here", "Rewind to here"]);
    expect(actions()[0]!.getAttribute("aria-expanded")).toBe("false");
  });

  test("not for a viewer who may not drive the chat, nor without the chat's row", async () => {
    await show(two, row({ blocked: "not yours here" } as Partial<SessionRow>));
    expect(actions()).toHaveLength(0);
    await draw(null);
    expect(actions()).toHaveLength(0);
  });

  test("not in a sub-agent's chat", async () => {
    await show(two, row({ parent_id: "22222222-2222-2222-2222-222222222222" }));
    expect(actions()).toHaveLength(0);
  });

  test("not on a routine's own first message, but on a later one", async () => {
    await show(two, row({ routine: { routine_id: "r1", name: "Nightly", run_id: "run1", status: "completed", seen: true, promoted: false, untrusted: false } }));
    expect(actions()).toHaveLength(1);
  });

  test("not on a message sent while an answer ran", async () => {
    await show([...two, ev(10, "turn_started", { turn_id: "t3" }), ev(11, "user_message", { turn_id: "t3", text: "third" }), ev(12, "user_message", { turn_id: "t3", text: "meanwhile", queued: true }), ev(13, "turn_ended", { turn_id: "t3", reason: "completed" })]);
    expect(host.querySelectorAll(".msg.user").length).toBe(4);
    expect(actions()).toHaveLength(3);
  });

  test("not on a message already rewound (inside the opened block)", async () => {
    await show([...two, rewound(10, 7, "t2")]);
    expect(actions()).toHaveLength(1);
    await click(host.querySelector(".rewound-toggle")!);
    expect(host.querySelector(".rewound-body")!.textContent).toContain("second");
    expect(actions()).toHaveLength(1);
  });

  test("not on a message a summary holds, but on the ones after it", async () => {
    await show([...two, ev(10, "history_summarized", { turn_id: "t2", through_seq: 5, text: "s" })]);
    expect(actions()).toHaveLength(1);
  });

  test("not while a turn runs", async () => {
    await show([...two, ev(10, "turn_started", { turn_id: "t3" })]);
    expect(actions()).toHaveLength(0);
  });

  test("not while a background command or a background sub-agent runs", async () => {
    await show([...two, ev(10, "background_started", { bg_id: "bg-1", call_id: "c", turn_id: "t2", timeout_s: 60, ends_by_ms: 0 })]);
    expect(actions()).toHaveLength(0);
    await show([...two, ev(10, "background_started", { bg_id: "bg-1", call_id: "c", turn_id: "t2", timeout_s: 60, ends_by_ms: 0, child: { session: "x", agent_id: "a" } })]);
    expect(actions()).toHaveLength(0);
  });
});

describe("the choice, inline", () => {
  test("opens a labelled panel under the message: what happens, one button, what is not undone; no files button when no turn changed files", async () => {
    await show(two);
    await click(actions()[1]);
    const panel = host.querySelector<HTMLElement>("[role=group][aria-label='Rewind this chat']")!;
    expect(panel).not.toBeNull();
    expect(actions()[1]!.getAttribute("aria-expanded")).toBe("true");
    expect(actions()[1]!.getAttribute("aria-controls")).toBe(panel.id);
    expect(panel.textContent).toContain("The chat goes back to before this message. Nothing is deleted: you can undo it until you send the next message.");
    expect([...panel.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Rewind conversation"]);
    expect(panel.textContent).toContain("Not undone: commands that ran, network calls, memory saved, board changes, published artifacts, sub-agent chats, tokens spent.");
    expect(document.activeElement).toBe(panel);
    expect(host.querySelector("[role=dialog]")).toBeNull();
  });

  test("Rewind conversation sends {message_seq, files: false, force: false}", async () => {
    await show(two);
    await click(actions()[1]);
    await wait();
    await click(named("Rewind conversation"));
    expect(rewind).toHaveBeenCalledTimes(1);
    expect(rewind).toHaveBeenCalledWith(SID, 7, false, false);
    expect(host.querySelector("[role=group][aria-label='Rewind this chat']")).toBeNull();
  });

  test("the files button shows only when a turn from that message on changed files, and sends files: true", async () => {
    const changed = [created, ...turn(2, "t1", "first"), ev(6, "checkpoint", { turn_id: "t1", phase: "after", git_ref: "r", files: ["a.rs"] }), ...turn(7, "t2", "second")];
    await show(changed);
    await click(actions()[1]);
    expect(named("Rewind conversation and files")).toBeUndefined();
    await click(actions()[1]);
    await click(actions()[0]);
    const both = named("Rewind conversation and files")!;
    expect(both).toBeDefined();
    expect(host.textContent).toContain("Files go back to before this message, like “Rewind files to before this turn”.");
    expect(both.getAttribute("aria-describedby")).toBeTruthy();
    await wait();
    await click(both);
    expect(rewind).toHaveBeenCalledWith(SID, 3, true, false);
  });

  test("a click or key straight after opening chooses nothing; a double click sends once", async () => {
    await show(two);
    await click(actions()[1]);
    await click(named("Rewind conversation"));
    expect(rewind).not.toHaveBeenCalled();
    await wait();
    let release!: () => void;
    rewind.mockImplementationOnce(() => new Promise((r) => (release = () => r({ ok: true }))));
    await click(named("Rewind conversation"));
    await click(named("Rewind conversation"));
    expect(rewind).toHaveBeenCalledTimes(1);
    await act(async () => release());
  });

  test("Escape closes it and focus goes back to the action; a click outside closes it", async () => {
    await show(two);
    await click(actions()[1]);
    await act(async () => void document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(host.querySelector(".rewind-panel")).toBeNull();
    expect(document.activeElement).toBe(actions()[1]);
    await click(actions()[1]);
    expect(host.querySelector(".rewind-panel")).not.toBeNull();
    await act(async () => void document.body.dispatchEvent(new Event("pointerdown", { bubbles: true })));
    expect(host.querySelector(".rewind-panel")).toBeNull();
  });

  test("a refusal is shown inline, with no repeat button for a refusal force does not go past", async () => {
    rewind.mockRejectedValueOnce(new Error("This chat can't be rewound there: a turn is running"));
    await show(two);
    await click(actions()[1]);
    await wait();
    await click(named("Rewind conversation"));
    expect(host.querySelector(".rewind-panel [role=alert]")!.textContent).toContain("This chat can't be rewound there: a turn is running");
    expect(named("Rewind anyway")).toBeUndefined();
  });

  test("a files part refused for another chat's work offers Rewind anyway, which repeats the call with force", async () => {
    const changed = [created, ...turn(2, "t1", "first"), ev(6, "checkpoint", { turn_id: "t1", phase: "after", git_ref: "r", files: ["a.rs"] }), ...turn(7, "t2", "second")];
    rewind.mockRejectedValueOnce(new Error("these files changed after the turn; undo would overwrite that: `a.rs`"));
    await show(changed);
    await click(actions()[0]);
    await wait();
    await click(named("Rewind conversation and files"));
    expect(rewind).toHaveBeenLastCalledWith(SID, 3, true, false);
    await click(named("Rewind anyway"));
    expect(rewind).toHaveBeenCalledTimes(2);
    expect(rewind).toHaveBeenLastCalledWith(SID, 3, true, true);
  });
});

describe("the message back in the box", () => {
  const withMedia = [
    created,
    ...turn(2, "t1", "look at this", {
      attachments: [{ name: "notes.txt", mime: "text/plain", text: "abcd" }],
      images: [{ media_id: "m1", sha256: "x", mime: "image/png", name: "shot.png", width: 1, height: 1 }],
    }),
  ];
  const accept = async (message_seq: number, turn_id: string) => {
    // The route answers once the node wrote the event: the socket delivers it too.
    rewind.mockImplementationOnce(async () => {
      apply(current, rewound(current.head_seq + 1, message_seq, turn_id));
      return { ok: true };
    });
  };

  test("an empty box gets the text and the images; the attachments are named as to attach again; focus goes to the box", async () => {
    await show(withMedia);
    await accept(3, "t1");
    await click(actions()[0]);
    await wait();
    await click(named("Rewind conversation"));
    await draw();
    expect(textarea().value).toBe("look at this");
    expect(host.querySelector(".composer .image-chip")!.textContent).toContain("shot.png");
    expect(host.querySelector(".rewound-block")!.textContent).toContain("Attach again: notes.txt");
    expect(named("Put the message back in the box")).toBeUndefined();
    expect(document.activeElement).toBe(textarea());
  });

  test("a box with something typed is left alone; the link on the block puts the message in on request", async () => {
    await show(two);
    await type("my draft");
    await accept(7, "t2");
    await click(actions()[1]);
    await wait();
    await click(named("Rewind conversation"));
    await draw();
    expect(textarea().value).toBe("my draft");
    expect(document.activeElement).toBe(textarea());
    await click(named("Put the message back in the box"));
    expect(textarea().value).toBe("second\nmy draft");
    expect(named("Put the message back in the box")).toBeUndefined();
  });
});

describe("the rewound block", () => {
  const three = [...two, ...turn(10, "t3", "third")];

  test("rewound turns are one closed block: N turns rewound, singular handled, opened to read them", async () => {
    await show([...three, rewound(14, 7, "t2")]);
    expect(host.querySelectorAll(".rewound-block")).toHaveLength(1);
    expect(host.querySelector(".rewound-toggle")!.textContent).toContain("2 turns rewound");
    expect(host.querySelector(".rewound-body")).toBeNull();
    expect(host.textContent).not.toContain("t2 answer");
    await click(host.querySelector(".rewound-toggle")!);
    expect(host.querySelector(".rewound-toggle")!.getAttribute("aria-expanded")).toBe("true");
    expect(host.querySelector(".rewound-body")!.textContent).toContain("second");
    expect(host.querySelector(".rewound-body")!.textContent).toContain("t3 answer");
    await show([...three, rewound(14, 11, "t3")]);
    expect(host.querySelector(".rewound-toggle")!.textContent).toContain("1 turn rewound");
  });

  test("nothing inside can be acted on: no Retry, no copy, no Rewind to here, no file buttons; every item is inert", async () => {
    const failed = [...two, ev(10, "turn_started", { turn_id: "t3" }), ev(11, "user_message", { turn_id: "t3", text: "third" }), ev(12, "turn_ended", { turn_id: "t3", reason: "error" }), ev(13, "checkpoint", { turn_id: "t3", phase: "after", git_ref: "r", files: ["a.rs"] })];
    await show([...failed, rewound(14, 7, "t2")]);
    await click(host.querySelector(".rewound-toggle")!);
    const body = host.querySelector<HTMLElement>(".rewound-body")!;
    expect(body.textContent).toContain("turn error");
    expect(body.textContent).not.toContain("Retry");
    expect(body.textContent).not.toContain("Undo file changes");
    expect(body.textContent).not.toContain("Rewind files to before this turn");
    expect(body.querySelector(".msg-actions, .rewind-slot, .undo")).toBeNull();
    // Whatever is shown of a turn is inert, but its end line, which holds the one way back for files.
    const holding = (text: string) => [...body.children].find((c) => c.textContent?.includes(text))!;
    for (const text of ["second", "t2 answer", "third"]) expect(holding(text).hasAttribute("inert")).toBe(true);
    expect(body.querySelectorAll("button")).toHaveLength(0);
  });

  test("Undo rewind shows while the fold says it can be undone and calls the route with the rewind's seq", async () => {
    await show([...two, rewound(10, 7, "t2")]);
    const undo = named("Undo rewind")!;
    expect(undo).toBeDefined();
    await click(undo);
    expect(undoRewind).toHaveBeenCalledWith(SID, 10);
  });

  test("a message after the rewind ends the Undo; the turns stay readable", async () => {
    await show([...two, rewound(10, 7, "t2"), ...turn(11, "t3", "third")]);
    expect(named("Undo rewind")).toBeUndefined();
    expect(host.querySelector(".rewound-toggle")!.textContent).toContain("1 turn rewound");
    expect(actions()).toHaveLength(2);
  });

  test("an undone rewind is not a block any more", async () => {
    await show([...two, rewound(10, 7, "t2"), ev(11, "conversation_rewind_undone", { rewind_seq: 10 })]);
    expect(host.querySelector(".rewound-block")).toBeNull();
    expect(actions()).toHaveLength(2);
  });

  test("a refused undo says why", async () => {
    undoRewind.mockRejectedValueOnce(new Error("This chat can't be rewound there: a message was sent after it"));
    await show([...two, rewound(10, 7, "t2")]);
    await click(named("Undo rewind"));
    expect(host.querySelector(".rewound-block [role=alert]")!.textContent).toContain("a message was sent after it");
  });

  test("no Undo for a viewer who may not drive the chat", async () => {
    await show([...two, rewound(10, 7, "t2")], row({ blocked: "not yours here" } as Partial<SessionRow>));
    expect(named("Undo rewind")).toBeUndefined();
    expect(host.querySelector(".rewound-toggle")).not.toBeNull();
  });

  test("a rewind that took the files back says where they come back; the way back is inside the turn", async () => {
    const files = [created, ...turn(2, "t1", "first"), ev(6, "checkpoint", { turn_id: "t1", phase: "after", git_ref: "r1", files: ["a.rs"] }), ...turn(7, "t2", "second")];
    const log = [
      ...files,
      ev(11, "checkpoint", { turn_id: "restore-1", phase: "before_restore", git_ref: "saved1" }),
      ev(12, "checkpoint", { turn_id: "t1", phase: "restored", git_ref: "r1" }),
      ev(13, "conversation_rewound", { message_seq: 3, turn_id: "t1", files: true }),
    ];
    await show(log);
    expect(host.querySelector(".rewound-block")!.textContent).toContain(
      "Files were rewound too. “Undo rewind” leaves them as they are; “Put back what this undo replaced”, in the turns below, brings them back.",
    );
    await click(host.querySelector(".rewound-toggle")!);
    const body = host.querySelector(".rewound-body")!;
    expect(body.textContent).toContain("Put back what this undo replaced");
    expect(body.textContent).not.toContain("Undo file changes");
  });
});

describe("a rewind arriving after the chat is on screen", () => {
  test("its events change the screen in place: the block appears, the actions go, the undo brings it back; the header and the task list do not double count", async () => {
    const usage = { input_tokens: 1000, output_tokens: 500, context_tokens: 900 };
    const log = [
      created,
      ...turn(2, "t1", "first", {}, { usage }),
      ev(6, "turn_started", { turn_id: "t2" }),
      ev(7, "user_message", { turn_id: "t2", text: "second" }),
      ev(8, "todo_updated", { turn_id: "t2", call_id: "c", items: [{ content: "write the thing", status: "pending" }] }),
      ev(9, "assistant_message", { turn_id: "t2", message_id: "m-t2", text: "t2 answer" }),
      ev(10, "turn_ended", { turn_id: "t2", reason: "completed", usage }),
    ];
    await show(log);
    const header = () => host.querySelector(".conv-meta .meter")?.textContent;
    const before = header();
    expect(before).toContain("3.0k");
    expect(actions()).toHaveLength(2);
    await wait(20);
    expect(host.querySelector(".todo-panel")!.textContent).toContain("0 of 1 done");
    // The same view object, grown in place, as the socket does.
    const sameView = current;
    apply(current, ev(11, "conversation_rewound", { message_seq: 7, turn_id: "t2", files: false }));
    expect(current).toBe(sameView);
    await draw();
    expect(host.querySelector(".rewound-toggle")!.textContent).toContain("1 turn rewound");
    expect(actions()).toHaveLength(1);
    expect(host.querySelector(".todo-panel")).toBeNull();
    expect(header()).toBe(before);
    apply(current, ev(12, "conversation_rewind_undone", { rewind_seq: 11 }));
    await draw();
    expect(host.querySelector(".rewound-block")).toBeNull();
    expect(actions()).toHaveLength(2);
    expect(host.querySelector(".todo-panel")!.textContent).toContain("0 of 1 done");
    expect(header()).toBe(before);
  });
});
