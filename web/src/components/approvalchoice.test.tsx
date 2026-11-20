// @vitest-environment happy-dom
// Row W.6: while a call waits for approval in the open chat, the composer's
// text field is replaced by the choice, with the card above the buttons; the
// field comes back, with its draft, when the call is answered. Keys typed as
// the choice appears answer nothing (a guard of GUARD_MS); no button has
// focus; a second pending call comes one at a time, guarded again.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { Item, SessionRow, SessionView } from "../types";

const approve = vi.fn(async (..._a: unknown[]) => ({}));
const cancel = vi.fn(async (..._a: unknown[]) => ({ ok: true }));
const prompt = vi.fn(async (..._a: unknown[]) => ({}));
vi.mock("../api", () => ({
  api: { features: () => Promise.resolve({ artifacts: { enabled: false }, media: { vision_images: false } }), prompt: (...a: unknown[]) => prompt(...a) },
  call: () => Promise.resolve({ ok: true }),
  HttpError: Error,
}));
vi.mock("../apiPages", () => ({ pageApi: { approve: (...a: unknown[]) => approve(...a), cancel: (...a: unknown[]) => cancel(...a) } }));
vi.mock("../lib/artifacts", async (original) => ({
  ...(await original<typeof import("../lib/artifacts")>()),
  artifactsApi: { forSession: () => Promise.resolve([]), save: vi.fn() },
}));
const { default: Conversation } = await import("./Conversation");
const { ApprovalChoice, GUARD_MS } = await import("./ApprovalChoice");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

type Tool = Extract<Item, { type: "tool" }>;
const RLO = "‮";
const tool = (n: number, over: Partial<NonNullable<Tool["approval"]>> = {}, input: Record<string, unknown> = {}): Tool => ({
  type: "tool",
  call_id: `c${n}`,
  author: null,
  tool: "bash",
  input: { command: `make step${n}`, ...input },
  live_output: "",
  result: null,
  approval: {
    approval_id: `a${n}`,
    reason: "`bash` can change files or run programs",
    turn_allow: true,
    chat_allow: true,
    decision: null,
    by: null,
    ...over,
  },
});

const SID = "33333333-3333-3333-3333-333333333333";
const view = (items: Item[], over: Partial<SessionView> = {}): SessionView =>
  ({
    session_id: SID,
    head_seq: 5,
    meta: { title: "Lighthouse", node_id: "master", model: "m::m", profile: "agentic", incognito: false, approval_mode: "ask", cwd: "/w" },
    items,
    running_turn: null,
    pending_approvals: items.flatMap((i) => (i.type === "tool" && i.approval?.decision === null ? [i.approval.approval_id] : [])),
    unknown_events: 0,
    active_agent: "general",
    running_agent: null,
    variant_selection: {},
    usage: { input_tokens: 0, output_tokens: 0 },
    context_tokens: 0,
    checkpoints: {},
    ...over,
  }) as unknown as SessionView;
const row = {
  session_id: SID,
  node_id: "master",
  title: "Lighthouse",
  cwd: "/w",
  model: "m::m",
  created_ms: 0,
  updated_ms: 0,
  head_seq: 5,
  running: false,
  owner_id: "u1",
  agent_id: "general",
  profile: "agentic",
  incognito: false,
  parent_id: null,
  workspace_id: "w1",
} as SessionRow;
const agents = [{ id: "general", name: "Assistant", avatar: { shape: "circle", eyes: "round", accent: "blue" } }] as never[];

let host: HTMLElement;
let root: Root;
let current: SessionView;
const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => current, onTopic: vi.fn(() => () => {}) } as unknown as Live;
beforeEach(() => {
  localStorage.clear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
  approve.mockClear();
  cancel.mockClear();
  prompt.mockClear();
});

async function show(v: SessionView, over: { blocked?: string | null } = {}) {
  current = v;
  await act(async () =>
    root.render(<Conversation live={live} sessionId={SID} row={{ ...row, blocked: over.blocked ?? null } as SessionRow} nodes={[]} agents={agents} models={[]} />),
  );
}
const box = () => host.querySelector<HTMLTextAreaElement>(".composer textarea")!;
const choice = () => host.querySelector<HTMLElement>(".approval-choice");
const named = (text: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === text)!;
const typeInto = (el: HTMLTextAreaElement | HTMLInputElement, value: string) =>
  act(() => {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
const key = (el: Element, k: string, type: "keydown" | "keyup" = "keydown") => {
  const e = new KeyboardEvent(type, { key: k, bubbles: true, cancelable: true });
  act(() => void el.dispatchEvent(e));
  return e;
};
const wait = (ms: number) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));

describe("the choice in the composer's place", () => {
  test("replaces the text field; the card (command, reason, hidden marks) is above the buttons, which are not in the transcript", async () => {
    await show(view([tool(1, { reason: `in the host shell ${RLO}x` }, { command: `rm ${RLO}-rf build` })]));
    expect(choice()).not.toBeNull();
    expect(box().hidden).toBe(true);
    const c = choice()!;
    expect(c.textContent).toContain("rm ");
    expect(c.textContent).toContain("<U+202E>");
    expect(c.textContent).toContain("in the host shell");
    expect(c.textContent).not.toContain(RLO);
    // The card scrolls, the buttons stay: the buttons are outside the scroll region.
    const card = c.querySelector(".approval-card")!;
    const actions = c.querySelector(".approval-actions")!;
    expect(card).not.toBeNull();
    expect(card.contains(actions)).toBe(false);
    expect(card.compareDocumentPosition(actions) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // One set of buttons: the transcript's card does not repeat them.
    expect(host.querySelectorAll("button.primary").length).toBe(1);
    expect(host.querySelectorAll("input.deny-note").length).toBe(1);
  });

  test("shows the edit preview rows", async () => {
    const t = tool(1);
    t.approval!.preview = "- old row\n+ new row";
    await show(view([t]));
    expect(choice()!.textContent).toContain("old row");
  });

  test("announces itself in a live region without taking focus", async () => {
    await show(view([tool(1)]));
    const live = choice()!.querySelector('[role="status"]')!;
    expect(live.textContent).toMatch(/waits for your answer/);
    expect(choice()!.contains(document.activeElement)).toBe(false);
    expect(document.activeElement?.tagName).not.toBe("BUTTON");
  });

  test("the field comes back with the draft and the caret where it was, once answered", async () => {
    await show(view([]));
    typeInto(box(), "a line being typed");
    act(() => {
      box().focus();
      box().setSelectionRange(3, 3);
    });
    await show(view([tool(1)]));
    expect(box().hidden).toBe(true);
    // Answered (here, by another tab: the event arrives).
    await show(view([tool(1, { decision: "allow", by: "user" })]));
    expect(choice()).toBeNull();
    expect(box().hidden).toBe(false);
    expect(box().value).toBe("a line being typed");
    expect(document.activeElement).toBe(box());
    expect(box().selectionStart).toBe(3);
  });

  test("a person who was not typing in the box does not have focus pulled to it", async () => {
    await show(view([tool(1)]));
    const other = document.createElement("button");
    document.body.append(other);
    other.focus();
    await show(view([tool(1, { decision: "deny", by: "user" })]));
    expect(document.activeElement).toBe(other);
    other.remove();
  });

  test("a chat without a pending call keeps its field", async () => {
    await show(view([tool(1, { decision: "allow", by: "user" })]));
    expect(choice()).toBeNull();
    expect(box().hidden).toBe(false);
  });
});

describe("the key guard", () => {
  test("Enter and Space typed as the choice appears approve nothing and send nothing; no button has focus", async () => {
    vi.useFakeTimers();
    await show(view([]));
    typeInto(box(), "half a sentence");
    act(() => box().focus());
    await show(view([tool(1)]));
    for (const el of [document.activeElement ?? document.body, document.body, choice()!, named("Allow once")]) {
      key(el, "Enter");
      key(el, " ");
      key(el, " ", "keyup");
      key(el, "Enter", "keyup");
    }
    expect(approve).not.toHaveBeenCalled();
    expect(prompt).not.toHaveBeenCalled();
    expect(choice()!.contains(document.activeElement)).toBe(false);
  });

  test("a key on a button is swallowed before GUARD_MS and works from it", async () => {
    vi.useFakeTimers();
    await show(view([tool(1)]));
    const allow = named("Allow once");
    expect(key(allow, "Enter").defaultPrevented).toBe(true);
    expect(key(allow, " ", "keyup").defaultPrevented).toBe(true);
    await wait(GUARD_MS - 1);
    expect(key(allow, "Enter").defaultPrevented).toBe(true);
    await act(async () => allow.click());
    expect(approve).not.toHaveBeenCalled();
    await wait(1);
    expect(key(allow, "Enter").defaultPrevented).toBe(false);
    await act(async () => allow.click());
    expect(approve).toHaveBeenCalledWith(SID, "a1", "allow");
  });

  test("a click at 0 ms does nothing; the buttons say they are not ready", async () => {
    vi.useFakeTimers();
    await show(view([tool(1)]));
    const allow = named("Allow once");
    expect(allow.getAttribute("aria-disabled")).toBe("true");
    await act(async () => allow.click());
    await act(async () => named("Deny").click());
    expect(approve).not.toHaveBeenCalled();
    await wait(GUARD_MS);
    expect(allow.getAttribute("aria-disabled")).toBeNull();
  });

  test("the duration is a short moment: over 300 ms, under a second", () => {
    expect(GUARD_MS).toBeGreaterThanOrEqual(300);
    expect(GUARD_MS).toBeLessThanOrEqual(1000);
  });

  test("a call in the transcript card (no composer) is not guarded: the field has the keys there", async () => {
    const host2 = document.createElement("div");
    document.body.append(host2);
    const r = createRoot(host2);
    await act(async () => r.render(<ApprovalChoice sessionId="s" item={tool(1)} />));
    await act(async () => [...host2.querySelectorAll("button")].find((b) => b.textContent === "Allow once")!.click());
    expect(approve).toHaveBeenCalledWith("s", "a1", "allow");
    act(() => r.unmount());
    host2.remove();
  });
});

describe("every answer from the composer's place is the card's call", () => {
  const ready = async (t: Tool = tool(1)) => {
    vi.useFakeTimers();
    await show(view([t]));
    await wait(GUARD_MS);
  };
  test("allow once", async () => {
    await ready();
    await act(async () => named("Allow once").click());
    expect(approve).toHaveBeenCalledWith(SID, "a1", "allow");
  });
  test("allow for this turn and for this chat, where offered", async () => {
    await ready();
    await act(async () => named("Allow this exact call for this turn").click());
    expect(approve).toHaveBeenLastCalledWith(SID, "a1", "allow_session");
    await act(async () => named("Allow this exact call for this chat").click());
    expect(approve).toHaveBeenLastCalledWith(SID, "a1", "allow_chat");
  });
  test("not offered where the node does not (a routine's, a sub-agent's)", async () => {
    await ready(tool(1, { turn_allow: undefined, chat_allow: undefined }));
    expect([...host.querySelectorAll("button")].some((b) => /for this (turn|chat)/.test(b.textContent ?? ""))).toBe(false);
  });
  test("deny, with and without a note; the note is not the composer's draft", async () => {
    await show(view([]));
    typeInto(box(), "my draft");
    vi.useFakeTimers();
    await show(view([tool(1)]));
    await wait(GUARD_MS);
    const note = host.querySelector<HTMLInputElement>("input.deny-note")!;
    expect(document.activeElement).not.toBe(note);
    typeInto(note, "keep the build folder");
    expect(box().value).toBe("my draft");
    expect(key(note, "Enter").defaultPrevented).toBe(true);
    expect(approve).not.toHaveBeenCalled();
    expect(prompt).not.toHaveBeenCalled();
    await act(async () => named("Deny").click());
    expect(approve).toHaveBeenCalledWith(SID, "a1", "deny", "keep the build folder");
    expect(box().value).toBe("my draft");
    approve.mockClear();
    await act(async () => named("Deny").click());
    expect(approve).toHaveBeenCalledWith(SID, "a1", "deny", "keep the build folder");
  });
  test("a viewer who may not drive the chat is offered Deny only", async () => {
    vi.useFakeTimers();
    await show(view([tool(1)]), { blocked: "viewer" });
    await wait(GUARD_MS);
    expect(host.querySelector("button.primary")).toBeNull();
    await act(async () => named("Deny").click());
    expect(approve).toHaveBeenCalledWith(SID, "a1", "deny");
  });
});

describe("several pending calls", () => {
  test("one at a time, in the order the node asked, with a count; guarded again when the next appears", async () => {
    vi.useFakeTimers();
    // The node asked for a2 first: the fold's list says so, whatever the items' order.
    const items = [tool(1), tool(2)];
    await show(view(items, { pending_approvals: ["a2", "a1"] }));
    expect(choice()!.textContent).toContain("make step2");
    expect(choice()!.textContent).not.toContain("make step1");
    expect(choice()!.textContent).toContain("1 of 2");
    // The second call's card in the transcript has no buttons: one choice at a time.
    expect(host.querySelectorAll("button.primary").length).toBe(1);
    expect(host.textContent).toContain("Asked after the one below.");
    await wait(GUARD_MS);
    await act(async () => named("Allow once").click());
    expect(approve).toHaveBeenCalledWith(SID, "a2", "allow");
    // a2 answered: a1 is next, alone, guarded afresh.
    await show(view([tool(1), tool(2, { decision: "allow", by: "user" })], { pending_approvals: ["a1"] }));
    expect(choice()!.textContent).toContain("make step1");
    expect(choice()!.textContent).not.toContain("1 of");
    approve.mockClear();
    await act(async () => named("Allow once").click());
    expect(approve).not.toHaveBeenCalled();
    key(named("Allow once"), "Enter");
    await wait(GUARD_MS);
    await act(async () => named("Allow once").click());
    expect(approve).toHaveBeenCalledWith(SID, "a1", "allow");
  });
});

describe("what stays reachable", () => {
  test("Stop works while the choice is shown", async () => {
    await show(view([tool(1)], { running_turn: "t1", running_agent: { agent_id: "general", name: "Assistant", prompt_hash: "" } as never }));
    const stop = host.querySelector<HTMLButtonElement>("button.send.stop")!;
    expect(stop).not.toBeNull();
    expect(stop.closest(".composer")!.contains(box())).toBe(true);
    await act(async () => stop.click());
    expect(cancel).toHaveBeenCalledWith(SID);
  });
});

describe("a chat that is not the open one", () => {
  test("its approval does not touch the open chat's field", async () => {
    // The open chat has nothing pending; the other waits (the attention list, not this view).
    await show(view([]));
    expect(choice()).toBeNull();
    expect(box().hidden).toBe(false);
  });
});
