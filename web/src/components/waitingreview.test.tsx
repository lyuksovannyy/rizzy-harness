// @vitest-environment happy-dom
// The review of PR 138 (R1 to R7), taken from the reviewer's proofs: each
// asserts the right behaviour and failed on f667b8aa.
// Between Send and the model's first word (the owner, 2026-11-16: "does it
// immediately send the user message in chat and then display that it waits
// for the model?"). The message shows at once, before the server answers,
// and the log's message replaces it, never beside it; the chat says it waits
// for the model from the same instant, apart from the answer, with a quiet
// count once the wait is long; a refusal gives the text back with why.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { Item, SessionRow, SessionView } from "../types";

const prompt = vi.fn<(...a: unknown[]) => Promise<unknown>>();
vi.mock("../api", () => ({
  api: { features: () => Promise.resolve({ artifacts: { enabled: false }, media: { vision_images: false } }), prompt: (...a: unknown[]) => prompt(...a) },
  call: () => Promise.resolve({ ok: true }),
  HttpError: Error,
}));
vi.mock("../apiPages", () => ({ pageApi: { cancel: vi.fn(() => Promise.resolve({ ok: true })) } }));
vi.mock("../lib/artifacts", async (original) => ({
  ...(await original<typeof import("../lib/artifacts")>()),
  artifactsApi: { forSession: () => Promise.resolve([]), save: vi.fn() },
}));
const { default: Conversation } = await import("./Conversation");
const { outbox } = await import("../lib/outbox");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SID = "22222222-2222-2222-2222-222222222222";
const view = (items: Item[], over: Partial<SessionView> = {}): SessionView =>
  ({
    session_id: SID,
    head_seq: 5,
    meta: { title: "Lighthouse", node_id: "master", model: "slow::slow", profile: "chat", incognito: false },
    items,
    running_turn: null,
    pending_approvals: [],
    unknown_events: 0,
    active_agent: "general",
    running_agent: null,
    variant_selection: {},
    usage: { input_tokens: 0, output_tokens: 0 },
    context_tokens: 0,
    checkpoints: {},
    ...over,
  }) as unknown as SessionView;
const row: SessionRow = {
  session_id: SID,
  node_id: "master",
  title: "Lighthouse",
  cwd: "",
  model: "slow::slow",
  created_ms: 0,
  updated_ms: 0,
  head_seq: 5,
  running: false,
  owner_id: "u1",
  agent_id: "general",
  profile: "chat",
  incognito: false,
  parent_id: null,
  workspace_id: "w1",
};
const agents = [{ id: "general", name: "Assistant", avatar: { shape: "circle", eyes: "round", accent: "blue" } }] as never[];
const earlier: Item[] = [
  { type: "user", turn_id: "t1", text: "what is a lighthouse" },
  { type: "assistant", turn_id: "t1", message_id: "m1", text: "A tower with a light.", done: true, author: null, variant: null } as Item,
  { type: "turn_end", turn_id: "t1", reason: "completed" } as Item,
];
// `last_ms`: the event's time on the server's clock, which the count must
// not trust (R4); the view no longer has such a field.
const running = (items: Item[], last_ms = Date.now()) =>
  view(items, { running_turn: "t2", running_agent: { agent_id: "general", name: "Assistant", prompt_hash: "" }, ...({ last_ms } as object) });

let host: HTMLElement;
let root: Root;
let current: SessionView;
const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => current, onTopic: vi.fn(() => () => {}) } as unknown as Live;
beforeEach(() => {
  prompt.mockReset();
  localStorage.clear();
  (outbox as { reset?: () => void }).reset?.();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

const box = () => host.querySelector<HTMLTextAreaElement>(".bottom textarea, .composer textarea")!;
const type = (text: string) =>
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(box(), text);
    box().dispatchEvent(new Event("input", { bubbles: true }));
  });
const send = () => act(async () => host.querySelector<HTMLButtonElement>(".composer button.send:not(.stop)")!.click());
const status = () => host.querySelector('[data-testid="turn-status"]');
const bubbles = (text: string) => [...host.querySelectorAll(".msg.user")].filter((m) => m.querySelector(".bubble")?.textContent === text);
async function show(v: SessionView) {
  current = v;
  await act(async () => root.render(<Conversation live={live} sessionId={SID} row={row} nodes={[]} agents={agents} models={[]} />));
}
function deferred() {
  let ok!: (v: unknown) => void;
  let no!: (e: unknown) => void;
  const p = new Promise((a, b) => {
    ok = a;
    no = b;
  });
  return { p, ok, no };
}

const { turnState } = await import("./TurnStatus");
const { readDraft } = await import("../lib/drafts");

describe("review PR 138", () => {
  test("R1 queued message while the model streams: line says waiting for the model, not answering", () => {
    const v = running([
      ...earlier,
      { type: "user", turn_id: "t2", text: "go" },
      { type: "assistant", turn_id: "t2", message_id: "m2", text: "Streaming", done: false, author: null, variant: null } as Item,
      { type: "user", turn_id: "t2", text: "also this", queued: { seq: 9, read: false } } as Item,
    ]);
    expect(turnState(v, false)).toBe("answering");
  });

  test("R1b queued message while a tool runs: line says working", () => {
    const tool = { type: "tool", turn_id: "t2", call_id: "c1", tool: "shell", input: {}, author: null, live_output: "", approval: null, result: null } as unknown as Item;
    const v = running([...earlier, { type: "user", turn_id: "t2", text: "go" }, tool, { type: "user", turn_id: "t2", text: "also", queued: { seq: 9, read: false } } as Item]);
    expect(turnState(v, false)).toBe("working");
  });

  test("R2 send A, type B, leave the chat, A refused: A must not be lost", async () => {
    const d = deferred();
    prompt.mockReturnValue(d.p);
    await show(view(earlier));
    type("A: the message");
    await send();
    type("B: typed meanwhile");
    await act(async () => root.render(<div />));
    await act(async () => d.no(new Error("409 the agent is working")));
    const kept = localStorage.getItem(`harness.draft.-.${SID}`) ?? "";
    console.log("R2 draft after refusal:", JSON.stringify(kept));
    expect(kept).toContain("A: the message");
  });

  test("R3 home: chat opened, its log arrived, then the first message is refused: box must hold the text as the bubble says", async () => {
    // startChat: add the entry, open the chat, then the prompt fails.
    const o = outbox.add(SID, "hello from home", 0);
    await show(view([]));
    await act(async () => {
      outbox.drop(SID, o, "the node is not connected");
      localStorage.setItem(`harness.draft.-.${SID}`, "hello from home"); // keep(): writeDraft
    });
    await show(view([]));
    const note = host.querySelector(".msg.user.failed")?.textContent;
    console.log("R3 failed bubble:", note, "| box:", JSON.stringify(box().value), "| draft:", JSON.stringify(readDraft(SID)));
    expect(box().value).toBe("hello from home");
  });

  test("R3b then typing something else in the box erases the kept draft", async () => {
    const o = outbox.add(SID, "hello again", 0);
    await show(view([]));
    await act(async () => {
      outbox.drop(SID, o, "x");
      localStorage.setItem(`harness.draft.-.${SID}`, "hello again");
    });
    type("hmm");
    console.log("R3b draft after typing:", JSON.stringify(readDraft(SID)));
    expect(readDraft(SID)).toContain("hello again");
  });

  test("R4 browser clock 5 min ahead of the server: count shows 300 s at once", async () => {
    await show(running([...earlier, { type: "user", turn_id: "t2", text: "q" }], Date.now() - 300_000));
    const c = status()?.querySelector(".waited")?.textContent ?? null;
    console.log("R4 count with browser 5 min ahead, event just now:", c);
    expect(c).toBeNull();
  });

  // Adapted (builder): the fix the review asked for counts from when this
  // tab got the event, on its own clock, so a tab cannot know the event was
  // 60 s old; what must hold is that the count does not wait 5 minutes.
  test("R4b browser clock 5 min behind: the count still runs, from when this tab got the event", async () => {
    vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
    await show(running([...earlier, { type: "user", turn_id: "t2", text: "q" }], Date.now() + 300_000 - 60_000));
    await act(async () => vi.advanceTimersByTime(60_000));
    const c = status()?.querySelector(".waited")?.textContent ?? null;
    console.log("R4b count with browser 5 min behind, 60 s after this tab got the event:", c);
    expect(c).toBe("60 s");
  });

  test("R5 two tabs: B's message lands while A's is pending: A's bubble vanishes before A is logged", async () => {
    const d = deferred();
    prompt.mockReturnValue(d.p);
    await show(view(earlier));
    type("from tab A");
    await send();
    await show(view([...earlier, { type: "user", turn_id: "t2", text: "from tab B" }]));
    const a = bubbles("from tab A").length;
    console.log("R5 A bubbles while A unlogged:", a, "| sending rows:", host.querySelectorAll(".msg.user.sending").length);
    expect(a).toBe(1);
    await act(async () => d.ok({ ok: true }));
  });

  test("R6 incognito chat: home refusal bubble says the text is back in the box", async () => {
    const o = outbox.add(SID, "secret q", 0);
    current = view([], { meta: { title: "t", node_id: "master", model: "slow::slow", profile: "chat", incognito: true } as never });
    await act(async () => root.render(<Conversation live={live} sessionId={SID} row={{ ...row, incognito: true }} nodes={[]} agents={agents} models={[]} />));
    await act(async () => outbox.drop(SID, o, "refused"));
    console.log("R6:", host.querySelector(".msg.user.failed")?.textContent, "| box:", JSON.stringify(box().value));
    expect(box().value).toBe("secret q");
  });
  test("R7 while the POST is in flight the draft no longer holds the message (a reload then loses it; main kept it)", async () => {
    const d = deferred();
    prompt.mockReturnValue(d.p);
    await show(view(earlier));
    type("in flight");
    expect(localStorage.getItem(`harness.draft.-.${SID}`)).toBe("in flight");
    await send();
    console.log("R7 draft while the POST is pending:", JSON.stringify(localStorage.getItem(`harness.draft.-.${SID}`)));
    expect(localStorage.getItem(`harness.draft.-.${SID}`)).toBe("in flight");
    await act(async () => d.ok({ ok: true }));
  });
});
