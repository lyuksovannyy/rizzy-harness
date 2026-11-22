// @vitest-environment happy-dom
// The row under a finished turn's final answer: Copy, Fork from here, Pin,
// the time, the tokens and their detail. Events are delivered AFTER mount in
// the tests that matter: the view is mutated in place.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { apply, fold } from "../fold";
import type { Live } from "../live";
import type { Features, ModelInfo, NodeInfo, RawEvent, SessionRow, SessionView } from "../types";

vi.mock("../api", () => ({
  api: {
    features: () => Promise.resolve({ artifacts: { enabled: false, show: false, why: null }, media: { enabled: false, show: false, vision_images: false }, routines: { enabled: true } } as unknown as Features),
    sessions: () => Promise.resolve([]),
    session: () => Promise.resolve({ title: "Parent chat" }),
    prompt: vi.fn(),
    agents: () => Promise.resolve({ agents: [] }),
  },
  call: () => Promise.resolve({ ok: true }),
  HttpError: Error,
}));
const forkChat = vi.fn((..._a: unknown[]) => Promise.resolve({ session_id: "x" }));
const pinMessage = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
vi.mock("../apiPages", () => ({
  pageApi: {
    deleteSession: vi.fn(),
    approve: vi.fn(),
    turnDiff: vi.fn(() => Promise.resolve({ files: [] })),
    restoreTurn: vi.fn(),
    retryTurn: vi.fn(),
    setAgent: vi.fn(),
    cancel: vi.fn(),
    forkChat: (...a: unknown[]) => forkChat(...a),
    pinMessage: (...a: unknown[]) => pinMessage(...a),
  },
}));
const { default: Conversation } = await import("./Conversation");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SID = "11111111-1111-1111-1111-111111111111";
const C1 = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
const C2 = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
const T0 = new Date(2026, 10, 21, 12, 0, 0).getTime();
const ev = (seq: number, kind: string, payload: object, session = SID, ts = T0 + seq * 1000): RawEvent => ({ session_id: session, seq, ts_ms: ts, kind, payload }) as RawEvent;
const created = (session = SID) => ev(1, "session_created", { title: "Chat", cwd: "/src", model: "demo::mock", backend: "native", node_id: "n1", approval_mode: "ask", profile: "agentic" }, session);
const use = (by: Record<string, [number, number, number?, number?]>) => {
  const by_model: Record<string, object> = {};
  let i = 0;
  let o = 0;
  for (const [m, [mi, mo, cr, cw]] of Object.entries(by)) {
    by_model[m] = { input_tokens: mi, output_tokens: mo, ...(cr ? { cache_read_tokens: cr } : {}), ...(cw ? { cache_write_tokens: cw } : {}) };
    i += mi;
    o += mo;
  }
  return { input_tokens: i, output_tokens: o, context_tokens: i, by_model };
};
const turn = (at: number, id: string, text: string, usage?: object, reason = "completed"): RawEvent[] => [
  ev(at, "turn_started", { turn_id: id }),
  ev(at + 1, "user_message", { turn_id: id, text: `q ${id}` }),
  ev(at + 2, "assistant_message", { turn_id: id, message_id: `m-${id}`, text }),
  ev(at + 3, "turn_ended", { turn_id: id, reason, ...(usage ? { usage } : {}) }),
];
const row = (over: Partial<SessionRow> = {}): SessionRow =>
  ({ session_id: SID, node_id: "n1", title: "Chat", cwd: "/src", model: "demo::mock", created_ms: 0, updated_ms: 0, head_seq: 5, running: false, owner_id: "u1", agent_id: "general", profile: "agentic", incognito: false, parent_id: null, workspace_id: "w1", ...over }) as SessionRow;
const NODE = { node_id: "n1", hostname: "h", version: "1", connected: true, embedded: false, profile: "agentic", fork: true } as NodeInfo;

let host: HTMLElement;
let root: Root;
let current: SessionView;
const others = new Map<string, SessionView>();
const refused = new Set<string>();
const watched: string[] = [];
const live = {
  watch: (id: string) => void watched.push(id),
  unwatch: vi.fn(),
  view: (id: string) => (id === SID ? current : others.get(id)),
  refused: (id: string) => refused.has(id),
  version: 0,
  subscribe: () => () => {},
  onTopic: vi.fn(() => () => {}),
} as unknown as Live;
beforeEach(() => {
  localStorage.clear();
  forkChat.mockClear();
  pinMessage.mockReset().mockResolvedValue({ ok: true });
  others.clear();
  refused.clear();
  watched.length = 0;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});
const draw = (r: SessionRow | null = row(), models: ModelInfo[] = [], node: Partial<NodeInfo> = {}) =>
  act(async () => root.render(<Conversation live={live} sessionId={SID} row={r} nodes={[{ ...NODE, ...node }]} agents={[]} models={models} />));
const show = async (events: RawEvent[], r: SessionRow | null = row(), models: ModelInfo[] = []) => {
  current = fold(events);
  await draw(r, models);
  await act(() => new Promise((x) => setTimeout(x, 0)));
};
const rows = () => [...host.querySelectorAll<HTMLElement>(".answer-actions")];
const words = (el: Element) => [...el.querySelectorAll("button, time")].map((b) => b.textContent?.replace(/\s+/g, " ").trim());
const click = (el: Element | null | undefined) => act(async () => (el as HTMLElement).click());
const popover = () => host.querySelector<HTMLElement>(".tok-pop");
const tokensButton = () => host.querySelector<HTMLButtonElement>(".tok-btn")!;
const key = (k: string) => act(async () => void document.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));
const pointer = (type: string, el: Element, pointerType = "mouse") =>
  act(async () => {
    const e = new Event(type, { bubbles: true });
    Object.defineProperty(e, "pointerType", { value: pointerType });
    el.dispatchEvent(e);
  });

describe("the row's order and visibility", () => {
  const usage = use({ "demo::mock": [1200, 340] });

  test("Copy, Fork from here, Pin, the time, the tokens, in that order, under the final answer only", async () => {
    await show([
      created(),
      ev(2, "turn_started", { turn_id: "a" }),
      ev(3, "user_message", { turn_id: "a", text: "go" }),
      ev(4, "assistant_message", { turn_id: "a", message_id: "m1", text: "a step" }),
      ev(5, "tool_call_started", { turn_id: "a", call_id: "c", tool: "read", input: { path: "a" } }),
      ev(6, "tool_call_finished", { call_id: "c", ok: true, output: "x" }),
      ev(7, "assistant_message", { turn_id: "a", message_id: "m2", text: "the answer" }),
      ev(8, "turn_ended", { turn_id: "a", reason: "completed", usage }),
    ]);
    expect(rows()).toHaveLength(1);
    expect(words(rows()[0]!)).toEqual(["Copy", "Fork from here", "Pin", expect.stringMatching(/ago|just now|yesterday|\d/), "1.2k in · 340 out"]);
    const msgs = [...host.querySelectorAll(".msg.assistant")];
    expect(msgs[0]!.querySelector(".answer-actions")).toBeNull();
    expect(msgs[0]!.querySelector(".msg-actions")).not.toBeNull();
    expect(msgs[1]!.contains(rows()[0]!)).toBe(true);
  });

  test("is the .msg-actions row Copy always was: the same hover, focus and touch rule", async () => {
    await show([created(), ...turn(2, "a", "ok", usage)]);
    expect(rows()[0]!.classList.contains("msg-actions")).toBe(true);
  });

  test("Fork from here is there once, not on the end line too", async () => {
    await show([created(), ...turn(2, "a", "ok", usage), ...turn(6, "b", "ok too", usage)]);
    const forks = [...host.querySelectorAll("button")].filter((b) => b.textContent === "Fork from here");
    expect(forks).toHaveLength(2);
    expect(host.querySelectorAll(".fork-line")).toHaveLength(0);
    await click(forks[1]);
    expect(forkChat).toHaveBeenCalledWith(SID, "b");
  });

  test("where the server would refuse Fork it is absent, Copy and Pin stay", async () => {
    await show([created(), ...turn(2, "a", "ok", usage)], row({ blocked: "not yours" } as Partial<SessionRow>));
    expect(words(rows()[0]!).slice(0, 2)).toEqual(["Copy", expect.stringMatching(/ago|just now|yesterday|\d/)]);
    await draw(row(), [], { fork: false });
    expect(words(rows()[0]!)[1]).toBe("Pin");
  });

  test("a turn still running has no row; the turn that ended has one when it arrives after mount", async () => {
    await show([created(), ev(2, "turn_started", { turn_id: "a" }), ev(3, "user_message", { turn_id: "a", text: "go" }), ev(4, "assistant_message", { turn_id: "a", message_id: "m", text: "so far" })]);
    expect(rows()).toHaveLength(0);
    apply(current, ev(5, "turn_ended", { turn_id: "a", reason: "completed", usage }));
    await draw();
    expect(rows()).toHaveLength(1);
    expect(words(rows()[0]!).at(-1)).toBe("1.2k in · 340 out");
  });

  test("a turn with no usage shows no token figure but keeps its time", async () => {
    await show([created(), ...turn(2, "a", "ok")]);
    expect(host.querySelector(".tok")).toBeNull();
    expect(rows()[0]!.querySelector("time")).not.toBeNull();
  });

  test("a turn that ended without a final answer text has no row, and Fork stays on its end line", async () => {
    await show([created(), ev(2, "turn_started", { turn_id: "a" }), ev(3, "user_message", { turn_id: "a", text: "go" }), ev(4, "turn_ended", { turn_id: "a", reason: "completed" })]);
    expect(rows()).toHaveLength(0);
    expect(host.querySelectorAll(".fork-line button")).toHaveLength(1);
  });
});

describe("the time", () => {
  test("is a <time> with the turn's end as dateTime and the complete date as its title", async () => {
    await show([created(), ...turn(2, "a", "ok")]);
    const t = rows()[0]!.querySelector("time")!;
    const ended = T0 + 5 * 1000;
    expect(t.getAttribute("datetime")).toBe(new Date(ended).toISOString());
    expect(t.getAttribute("title")).toMatch(/2026/);
    expect(t.getAttribute("title")).toMatch(/:00:05/);
  });
});

describe("the tokens and their detail", () => {
  const prices: ModelInfo[] = [
    { id: "p::big", name: "big", provider: "p", price: { currency: "USD", input: 3, output: 15 } },
    { id: "p::small", name: "small", provider: "p", price: { currency: "USD", input: 1, output: 5 } },
  ] as unknown as ModelInfo[];
  // A turn on two models; it delegated to two sub-agents, each on its own model(s). The turn's usage already holds theirs.
  const parent: RawEvent[] = [
    created(),
    ev(2, "turn_started", { turn_id: "t" }),
    ev(3, "user_message", { turn_id: "t", text: "review" }),
    ev(4, "tool_call_started", { turn_id: "t", call_id: "c1", tool: "delegate", input: { agent: "reviewer", task: "x" } }),
    ev(5, "tool_call_finished", { call_id: "c1", ok: true, output: `session: ${C1}\nok` }),
    ev(6, "tool_call_started", { turn_id: "t", call_id: "c2", tool: "delegate", input: { agent: "tester", task: "y" } }),
    ev(7, "tool_call_finished", { call_id: "c2", ok: true, output: `session: ${C2}\nok` }),
    ev(8, "assistant_message", { turn_id: "t", message_id: "m", text: "done" }),
    ev(9, "turn_ended", { turn_id: "t", reason: "completed", usage: use({ "p::big": [10000, 2000, 4000, 500], "p::small": [3000, 700] }) }),
  ];
  const childLog = (id: string, usage: object) => fold([created(id), ev(2, "turn_started", { turn_id: "c" }, id, T0 + 5000), ev(3, "user_message", { turn_id: "c", text: "task" }, id, T0 + 5000), ev(4, "turn_ended", { turn_id: "c", reason: "completed", usage }, id, T0 + 8000)]);

  test("the figure is the turn's final usage; the detail lists models, sub-agents and the total", async () => {
    others.set(C1, childLog(C1, use({ "p::small": [1000, 300] })));
    others.set(C2, childLog(C2, use({ "p::small": [500, 100], "p::big": [800, 50] })));
    await show(parent, row(), prices);
    expect(tokensButton().textContent).toBe("13k in · 2.7k out");
    expect(popover()).toBeNull();
    await click(tokensButton());
    const pop = popover()!;
    expect(pop.getAttribute("role")).toBe("dialog");
    expect(pop.getAttribute("aria-label")).toBeTruthy();
    const models = [...pop.querySelectorAll("tbody tr")].map((r) => [...r.children].map((c) => c.textContent));
    expect(models[0]!.slice(0, 5)).toEqual(["p::big", "10,000", "2,000", "4,000", "500"]);
    expect(models[1]!.slice(0, 3)).toEqual(["p::small", "3,000", "700"]);
    // Cost, from the header's functions: big 6000 in x3 + 4000 cached (no price: unpriced) -> unpriced; small priced.
    expect(models[1]![5]).toMatch(/\$|USD/);
    const subs = [...pop.querySelectorAll(".tok-children li")].map((l) => l.textContent);
    expect(subs).toHaveLength(2);
    expect(subs[0]).toContain("reviewer");
    expect(subs[0]).toContain("p::small");
    expect(subs[0]).toContain("1,000 in · 300 out");
    expect(subs[1]).toContain("tester");
    expect(subs[1]).toContain("p::big, p::small");
    expect(subs[1]).toContain("1,300 in · 150 out");
    expect(pop.querySelector(".tok-total")!.textContent).toContain("13,000 in · 2,700 out");
    // Read from their own chats, and only while the detail is open.
    expect(watched.filter((id) => id !== SID).sort()).toEqual([C1, C2]);
  });

  test("a sub-agent whose chat is not readable, or not loaded yet, says so; the total is still the turn's", async () => {
    refused.add(C1);
    await show(parent);
    await click(tokensButton());
    const subs = [...popover()!.querySelectorAll(".tok-children li")].map((l) => l.textContent);
    expect(subs[0]).toContain("cannot be read");
    expect(subs[1]).toContain("reading its chat");
    expect(popover()!.querySelector(".tok-total")!.textContent).toContain("13,000 in");
  });

  test("a sub-agent chat's earlier turns are not this turn's", async () => {
    const c = childLog(C1, use({ "p::small": [1000, 300] }));
    // An older turn of the same sub-agent chat, before the parent's previous turn ended.
    others.set(C1, fold([created(C1), ev(2, "turn_started", { turn_id: "old" }, C1, T0), ev(3, "turn_ended", { turn_id: "old", reason: "completed", usage: use({ "p::small": [9999, 9999] }) }, C1, T0 + 100), ...(c.items.length ? [] : [])]));
    const log = [created(), ...[ev(2, "turn_started", { turn_id: "t0" }, SID, T0 + 200), ev(3, "turn_ended", { turn_id: "t0", reason: "completed" }, SID, T0 + 300)].map((e, i) => ({ ...e, seq: i + 2 })), ...parent.slice(1).map((e, i) => ({ ...e, seq: i + 4 }))];
    await show(log);
    expect(popover()).toBeNull();
    await click(tokensButton());
    expect(popover()!.querySelector(".tok-children li")!.textContent).toContain("nothing recorded");
  });

  test("opens on hover and on keyboard focus, and on click; Escape and a click outside close it", async () => {
    await show([created(), ...turn(2, "a", "ok", use({ "demo::mock": [100, 20] }))]);
    const span = host.querySelector(".tok")!;
    await pointer("pointerover", span);
    expect(popover()).not.toBeNull();
    await pointer("pointerout", span);
    expect(popover()).toBeNull();
    // A touch pointer does not open it by "hover": its click does.
    await pointer("pointerover", span, "touch");
    expect(popover()).toBeNull();
    await click(tokensButton());
    expect(popover()).not.toBeNull();
    expect(tokensButton().getAttribute("aria-expanded")).toBe("true");
    await key("Escape");
    expect(popover()).toBeNull();
    await act(async () => tokensButton().focus());
    expect(popover()).not.toBeNull();
    await key("Escape");
    expect(popover()).toBeNull();
    await click(tokensButton());
    expect(popover()).not.toBeNull();
    await act(async () => void document.body.dispatchEvent(new Event("pointerdown", { bubbles: true })));
    expect(popover()).toBeNull();
  });

  test("every control has a name", async () => {
    await show([created(), ...turn(2, "a", "ok", use({ "demo::mock": [100, 20] }))]);
    for (const b of rows()[0]!.querySelectorAll("button")) expect((b.getAttribute("aria-label") ?? b.textContent ?? "").trim()).not.toBe("");
    expect(tokensButton().getAttribute("aria-label")).toContain("100 in");
  });
});

describe("a rewound turn and a copied one", () => {
  const usage = use({ "demo::mock": [100, 20] });
  test("a rewound turn's row has no Copy, Fork or Pin: its figures only, in an inert block", async () => {
    await show([created(), ...turn(2, "a", "one", usage), ...turn(6, "b", "two", usage), ev(10, "conversation_rewound", { message_seq: 7, turn_id: "b" })]);
    await click(host.querySelector(".rewound-toggle"));
    expect(rows()).toHaveLength(2);
    const inside = rows().filter((r) => r.closest("[inert]"));
    expect(inside).toHaveLength(1);
    expect(words(inside[0]!).filter((w) => ["Copy", "Fork from here", "Pin"].includes(w ?? ""))).toEqual([]);
    expect(words(inside[0]!).at(-1)).toBe("100 in · 20 out");
  });

  test("a copied turn shows its time and tokens, marked before the fork, and the fork's totals stay zero", async () => {
    const log = [
      ev(1, "session_created", { title: "Chat (fork)", cwd: "/src", model: "demo::mock", backend: "native", node_id: "n1", approval_mode: "ask", profile: "agentic", forked: { parent: "33333333-3333-3333-3333-333333333333", through_seq: 5 } }),
      ...turn(2, "a", "copied", usage),
      ev(6, "session_forked", { parent: "33333333-3333-3333-3333-333333333333", through_seq: 5, turn_id: "a" }),
      ...turn(7, "b", "own", usage).map((e, i) => ({ ...e, seq: 7 + i })),
    ];
    await show(log);
    expect(rows()).toHaveLength(2);
    expect(words(rows()[0]!).at(-1)).toBe("100 in · 20 out");
    await click(rows()[0]!.querySelector(".tok-btn"));
    expect(popover()!.textContent).toContain("before the fork");
    await key("Escape");
    await click(rows()[1]!.querySelector(".tok-btn"));
    expect(popover()!.textContent).not.toContain("before the fork");
    expect(current.usage.input_tokens).toBe(100);
  });
});

describe("Pin and Unpin", () => {
  const usage = use({ "demo::mock": [100, 20] });
  const pinButton = () => [...host.querySelectorAll<HTMLButtonElement>(".answer-actions button")].find((b) => /^(Pin|Unpin)$/.test(b.textContent?.trim() ?? ""));

  test("Pin sends the message's seq; the log, delivered after mount, turns it into Unpin", async () => {
    await show([created(), ...turn(2, "a", "ok", usage)]);
    expect(pinButton()!.textContent!.trim()).toBe("Pin");
    await click(pinButton());
    // The assistant message of turn a is event 4.
    expect(pinMessage).toHaveBeenCalledWith(SID, 4, true);
    apply(current, ev(current.head_seq + 1, "message_pinned", { message_seq: 4, pinned: true }));
    await draw();
    expect(pinButton()!.textContent!.trim()).toBe("Unpin");
    expect(pinButton()!.getAttribute("aria-pressed")).toBe("true");
    await click(pinButton());
    expect(pinMessage).toHaveBeenLastCalledWith(SID, 4, false);
    apply(current, ev(current.head_seq + 1, "message_pinned", { message_seq: 4, pinned: false }));
    await draw();
    expect(pinButton()!.textContent!.trim()).toBe("Pin");
  });

  test("a refusal is shown in the row, in its own words", async () => {
    pinMessage.mockRejectedValue(new Error("this chat has 50 pinned messages; unpin one first"));
    await show([created(), ...turn(2, "a", "ok", usage)]);
    await click(pinButton());
    expect(rows()[0]!.querySelector('[role="alert"]')!.textContent).toContain("50 pinned messages");
  });

  test("not offered to a viewer who does not own the chat, nor without its row", async () => {
    await show([created(), ...turn(2, "a", "ok", usage)], row({ blocked: "not yours" } as Partial<SessionRow>));
    expect(pinButton()).toBeUndefined();
    await draw(null);
    expect(pinButton()).toBeUndefined();
  });

  test("offered in a sub-agent's chat and a routine's chat the viewer owns", async () => {
    await show([created(), ...turn(2, "a", "ok", usage)], row({ parent_id: "44444444-4444-4444-4444-444444444444" }));
    expect(pinButton()).toBeDefined();
  });
});
