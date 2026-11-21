// @vitest-environment happy-dom
// The chat's "Pinned (n)" list, its jump, the mark on a pinned message, and
// pins arriving after the chat is on screen (the view is mutated in place).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { apply, fold } from "../fold";
import type { Live } from "../live";
import type { Features, RawEvent, SessionRow, SessionView } from "../types";

vi.mock("../api", () => ({
  api: {
    features: () => Promise.resolve({ artifacts: { enabled: false, show: false, why: null }, media: { enabled: false, show: false, vision_images: false }, routines: { enabled: true } } as unknown as Features),
    sessions: () => Promise.resolve([]),
    session: () => Promise.resolve({ title: "Parent" }),
    prompt: vi.fn(),
    agents: () => Promise.resolve({ agents: [] }),
  },
  call: () => Promise.resolve({ ok: true }),
  HttpError: Error,
}));
const pinMessage = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
vi.mock("../apiPages", () => ({
  pageApi: { deleteSession: vi.fn(), approve: vi.fn(), turnDiff: vi.fn(), restoreTurn: vi.fn(), retryTurn: vi.fn(), setAgent: vi.fn(), cancel: vi.fn(), forkChat: vi.fn(), pinMessage: (...a: unknown[]) => pinMessage(...a) },
}));
const { default: Conversation } = await import("./Conversation");
const { FLASH_MS } = await import("./PinnedList");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SID = "11111111-1111-1111-1111-111111111111";
const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: SID, seq, ts_ms: 1_700_000_000_000 + seq * 1000, kind, payload }) as RawEvent;
const created = ev(1, "session_created", { title: "Chat", cwd: "/src", model: "demo::mock", backend: "native", node_id: "n1", approval_mode: "ask", profile: "agentic" });
const turn = (at: number, id: string, text: string): RawEvent[] => [
  ev(at, "turn_started", { turn_id: id }),
  ev(at + 1, "user_message", { turn_id: id, text: `question ${id}` }),
  ev(at + 2, "assistant_message", { turn_id: id, message_id: `m-${id}`, text }),
  ev(at + 3, "turn_ended", { turn_id: id, reason: "completed" }),
];
// Answers are events 4 and 8, the person's messages 3 and 7.
const base = [created, ...turn(2, "a", "first   answer\nwith lines"), ...turn(6, "b", "second answer")];
const row = (over: Partial<SessionRow> = {}): SessionRow =>
  ({ session_id: SID, node_id: "n1", title: "Chat", cwd: "/src", model: "demo::mock", created_ms: 0, updated_ms: 0, head_seq: 9, running: false, owner_id: "u1", agent_id: "general", profile: "agentic", incognito: false, parent_id: null, workspace_id: "w1", ...over }) as SessionRow;

let host: HTMLElement;
let root: Root;
let current: SessionView;
const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => current, refused: () => false, version: 0, subscribe: () => () => {}, onTopic: vi.fn(() => () => {}) } as unknown as Live;
const scrolled = vi.fn();
beforeEach(() => {
  localStorage.clear();
  pinMessage.mockReset().mockResolvedValue({ ok: true });
  scrolled.mockReset();
  (Element.prototype as unknown as { scrollIntoView: unknown }).scrollIntoView = function (this: Element) {
    scrolled(this.getAttribute("data-msg-seq"));
  };
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
  vi.restoreAllMocks();
});
const draw = (r: SessionRow | null = row()) => act(async () => root.render(<Conversation live={live} sessionId={SID} row={r} nodes={[]} agents={[]} models={[]} />));
const show = async (events: RawEvent[], r: SessionRow | null = row()) => {
  current = fold(events);
  await draw(r);
  await act(() => new Promise((x) => setTimeout(x, 0)));
};
const pins = (...seqs: number[]) => seqs.map((s, i) => ev(10 + i, "message_pinned", { message_seq: s, pinned: true }));
const button = () => host.querySelector<HTMLButtonElement>(".pinned-btn");
const click = (el: Element | null | undefined) => act(async () => (el as HTMLElement).click());
const entries = () => [...host.querySelectorAll(".pinned-pop li")];
const key = (k: string) => act(async () => void document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));

describe("the Pinned entry in the header", () => {
  test("is absent without pins; with them it shows the count", async () => {
    await show(base);
    expect(button()).toBeNull();
    await show([...base, ...pins(4, 8)]);
    expect(button()!.textContent).toContain("Pinned (2)");
    expect(button()!.getAttribute("aria-label")).toBe("Pinned messages: 2");
    expect(button()!.getAttribute("aria-expanded")).toBe("false");
  });

  test("a pin that arrives after mount shows it, and taking it off removes it again", async () => {
    await show(base);
    apply(current, ev(10, "message_pinned", { message_seq: 4, pinned: true }));
    await draw();
    expect(button()!.textContent).toContain("Pinned (1)");
    apply(current, ev(11, "message_pinned", { message_seq: 4, pinned: false }));
    await draw();
    expect(button()).toBeNull();
  });

  test("lists each pin's first words on one line, who wrote it and when", async () => {
    await show([...base, ...pins(4, 3)]);
    await click(button());
    const pop = host.querySelector('[role="dialog"].pinned-pop')!;
    expect(pop.getAttribute("aria-label")).toBe("Pinned messages");
    // In log order: the person's message (3) before the answer (4).
    expect(entries().map((e) => e.querySelector(".pinned-text")!.textContent)).toEqual(["question a", "first answer with lines"]);
    expect(entries()[0]!.querySelector(".pinned-meta")!.textContent).toContain("You");
    expect(entries()[1]!.querySelector(".pinned-meta")!.textContent).toContain("Assistant");
    expect(entries()[1]!.querySelector("time")).not.toBeNull();
  });

  test("choosing a pin scrolls to its message, marks it for a moment and closes the list", async () => {
    await show([...base, ...pins(4, 8)]);
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    await click(button());
    await click(entries()[0]!.querySelector(".pinned-go"));
    expect(scrolled).toHaveBeenCalledWith("4");
    const msg = host.querySelector('[data-msg-seq="4"]')!;
    expect(msg.classList.contains("pin-flash")).toBe(true);
    expect(host.querySelector(".pinned-pop")).toBeNull();
    await act(async () => void vi.advanceTimersByTime(FLASH_MS + 10));
    expect(msg.classList.contains("pin-flash")).toBe(false);
  });

  test("Escape and a click outside close it; Escape returns focus to the button", async () => {
    await show([...base, ...pins(4)]);
    await click(button());
    expect(host.querySelector(".pinned-pop")).not.toBeNull();
    await key("Escape");
    expect(host.querySelector(".pinned-pop")).toBeNull();
    expect(document.activeElement).toBe(button());
    await click(button());
    await act(async () => void document.body.dispatchEvent(new Event("pointerdown", { bubbles: true })));
    expect(host.querySelector(".pinned-pop")).toBeNull();
  });
});

describe("the mark on a pinned message", () => {
  test("is on the pinned answer only", async () => {
    await show([...base, ...pins(4)]);
    const marks = [...host.querySelectorAll(".msg.assistant")].map((m) => !!m.querySelector(".pin-mark"));
    expect(marks).toEqual([true, false]);
    expect(host.querySelector(".pin-mark")!.getAttribute("aria-label")).toBe("Pinned");
  });
});

describe("who can unpin", () => {
  test("the owner unpins from the list; the call names the message", async () => {
    await show([...base, ...pins(4)]);
    await click(button());
    const un = entries()[0]!.querySelector<HTMLButtonElement>("button.link")!;
    expect(un.textContent).toBe("Unpin");
    await click(un);
    expect(pinMessage).toHaveBeenCalledWith(SID, 4, false);
  });

  test("a refusal is said in the list", async () => {
    pinMessage.mockRejectedValue(new Error("This message can't be pinned: that message is rewound"));
    await show([...base, ...pins(4)]);
    await click(button());
    await click(entries()[0]!.querySelector("button.link"));
    expect(host.querySelector(".pinned-pop [role=alert]")!.textContent).toContain("rewound");
  });

  test("someone who cannot drive the chat reads the list without Unpin (and the row without Pin)", async () => {
    await show([...base, ...pins(4)], row({ blocked: "not yours" } as Partial<SessionRow>));
    await click(button());
    expect(entries()).toHaveLength(1);
    expect(host.querySelector(".pinned-pop button.link")).toBeNull();
    expect([...host.querySelectorAll(".answer-actions button")].some((b) => /Pin/.test(b.textContent ?? ""))).toBe(false);
  });
});

describe("rewound and inherited pins", () => {
  test("a pin of a rewound message is not listed until the rewind is undone, delivered after mount", async () => {
    await show([...base, ...pins(4, 8)]);
    expect(button()!.textContent).toContain("Pinned (2)");
    apply(current, ev(current.head_seq + 1, "conversation_rewound", { message_seq: 7, turn_id: "b" }));
    await draw();
    expect(button()!.textContent).toContain("Pinned (1)");
    const at = current.rewinds![0]!.seq;
    apply(current, ev(current.head_seq + 1, "conversation_rewind_undone", { rewind_seq: at }));
    await draw();
    expect(button()!.textContent).toContain("Pinned (2)");
  });

  test("a fork lists the pins of the events it copied", async () => {
    const parent = "33333333-3333-3333-3333-333333333333";
    const log = [
      ev(1, "session_created", { title: "Chat (fork)", cwd: "/src", model: "demo::mock", backend: "native", node_id: "n1", approval_mode: "ask", profile: "agentic", forked: { parent, through_seq: 5 } }),
      ...turn(2, "a", "copied answer").slice(0, 3),
      ev(5, "message_pinned", { message_seq: 4, pinned: true }),
      ev(6, "turn_ended", { turn_id: "a", reason: "completed" }),
    ];
    await show(log);
    expect(button()!.textContent).toContain("Pinned (1)");
  });
});
