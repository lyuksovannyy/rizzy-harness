// @vitest-environment happy-dom
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
const { Composer } = await import("./Composer");
const { outbox } = await import("../lib/outbox");
const { startChat } = await import("../lib/startChat");

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
const running = (items: Item[], last_ms = Date.now()) =>
  view(items, { running_turn: "t2", running_agent: { agent_id: "general", name: "Assistant", prompt_hash: "" }, last_ms });

let host: HTMLElement;
let root: Root;
let current: SessionView;
const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => current, onTopic: vi.fn(() => () => {}) } as unknown as Live;
beforeEach(() => {
  prompt.mockReset();
  localStorage.clear();
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

describe("a message in an open chat", () => {
  test("shows at once, marked Sending…, with the waiting line, before the server answers", async () => {
    const d = deferred();
    prompt.mockReturnValue(d.p);
    await show(view(earlier));
    type("and its keeper?");
    await send();
    // The server has not answered: the message is in the chat, not in the box.
    expect(box().value).toBe("");
    const mine = bubbles("and its keeper?");
    expect(mine).toHaveLength(1);
    expect(mine[0]!.classList.contains("sending")).toBe(true);
    expect(mine[0]!.textContent).toContain("Sending…");
    expect(status()?.textContent).toContain("Assistant is waiting for the model…");
    // The log has it: the same one message, no longer marked.
    await act(async () => d.ok({ ok: true }));
    await show(running([...earlier, { type: "user", turn_id: "t2", text: "and its keeper?" }]));
    expect(bubbles("and its keeper?")).toHaveLength(1);
    expect(host.querySelector(".msg.user.sending")).toBeNull();
    expect(status()?.textContent).toContain("Assistant is waiting for the model…");
  });

  test("the log's message replaces the row even when it arrives before the answer to the POST", async () => {
    const d = deferred();
    prompt.mockReturnValue(d.p);
    await show(view(earlier));
    type("same words");
    await send();
    await show(running([...earlier, { type: "user", turn_id: "t2", text: "same words" }]));
    expect(bubbles("same words")).toHaveLength(1);
    await act(async () => d.ok({ ok: true }));
    expect(bubbles("same words")).toHaveLength(1);
  });

  test("a refused message leaves no row; the text goes back to the box with why", async () => {
    prompt.mockRejectedValue(new Error("the node is not connected"));
    await show(view(earlier));
    type("lost?");
    await send();
    expect(bubbles("lost?")).toHaveLength(0);
    expect(box().value).toBe("lost?");
    expect(host.querySelector(".bottom .error-text")?.textContent).toBe("the node is not connected");
    expect(status()).toBeNull();
  });

  test("refused after the person left the chat: the text is the chat's draft, not lost", async () => {
    const d = deferred();
    prompt.mockReturnValue(d.p);
    await show(view(earlier));
    type("sent, then away");
    await send();
    await act(async () => root.render(<div />));
    await act(async () => d.no(new Error("the node is not connected")));
    expect(localStorage.getItem(`harness.draft.-.${SID}`)).toBe("sent, then away");
  });
});

describe("the waiting line", () => {
  test("tells the wait from the answer, and counts the seconds of a long wait without reading them out", async () => {
    await show(running([...earlier, { type: "user", turn_id: "t2", text: "and the pier?" }], Date.now() - 2_000));
    expect(status()?.textContent).toBe("Assistant is waiting for the model…");
    // A long wait: the count shows, outside the announced text.
    await show(running([...earlier, { type: "user", turn_id: "t2", text: "and the pier?" }], Date.now() - 12_000));
    const count = status()?.querySelector(".waited");
    expect(count?.textContent).toBe("12 s");
    // Screen readers hear the words, from a region that was there before
    // them, never the count; the visible line is not read twice.
    expect(host.querySelector('[role="status"].sr-only')?.textContent).toBe("Assistant is waiting for the model…");
    expect(status()?.getAttribute("aria-hidden")).toBe("true");
    // The first word is there: the answer, not the wait.
    await show(
      running([
        ...earlier,
        { type: "user", turn_id: "t2", text: "and the pier?" },
        { type: "assistant", turn_id: "t2", message_id: "m2", text: "The pier", done: false, author: null, variant: null } as Item,
      ]),
    );
    expect(status()?.textContent).toBe("Assistant is answering…");
    expect(status()?.querySelector(".waited")).toBeNull();
    expect(host.querySelector('[role="status"].sr-only')?.textContent).toBe("");
  });

  test("a turn whose message the log does not hold yet is a wait, not the last turn's answer", async () => {
    // `turn_started` comes before `user_message`: for a moment the newest
    // item is the last turn's end.
    await show(running(earlier));
    expect(status()?.textContent).toBe("Assistant is waiting for the model…");
  });

  test("a tool's result means the next model call is a wait again", async () => {
    const tool = { type: "tool", turn_id: "t2", call_id: "c1", tool: "web_fetch", input: {}, author: null, live_output: "", approval: null, result: { ok: true, output: "page", exit_code: null } } as unknown as Item;
    await show(running([...earlier, { type: "user", turn_id: "t2", text: "look it up" }, tool]));
    expect(status()?.textContent).toBe("Assistant is waiting for the model…");
  });
});

describe("the first message from home", () => {
  test("the box shows the message above it at once while the chat is being made", async () => {
    const d = deferred();
    await act(async () =>
      root.render(
        <Composer
          where="home"
          agents={[]}
          models={[]}
          workers={[]}
          agentId=""
          onAgentChange={() => {}}
          agenticBlocked={null}
          onSubmit={() => d.p as Promise<void>}
          predict={() => "ask"}
        />,
      ),
    );
    type("tell me about the lighthouse");
    await send();
    expect(box().value).toBe("");
    expect(host.querySelector(".msg.user.sending .bubble")?.textContent).toBe("tell me about the lighthouse");
    // Refused (the session could not be made): back in the box, with why.
    await act(async () => d.no(new Error("no node")));
    expect(host.querySelector(".msg.user.sending")).toBeNull();
    expect(box().value).toBe("tell me about the lighthouse");
    expect(host.querySelector(".error-text")?.textContent).toBe("no node");
  });

  test("a first message the server refuses stays in the new chat, marked not sent with why, and its text is the chat's draft", async () => {
    const kept: [string, string][] = [];
    const opened: string[] = [];
    const fake = {
      createSession: () => Promise.resolve({ session_id: "33333333-3333-3333-3333-333333333333" }),
      prompt: () => Promise.reject(new Error("the node is not connected")),
    } as never;
    const s = { text: "hello?", mode: "ask", agentId: "", model: "", effort: null, nodeId: "", cwd: "", incognito: false, attachments: [], compareWith: null } as never;
    await startChat(s, { workspace: "w1" }, (id) => opened.push(id), fake, (c, t) => kept.push([c, t]));
    expect(opened).toEqual(["33333333-3333-3333-3333-333333333333"]);
    expect(kept).toEqual([["33333333-3333-3333-3333-333333333333", "hello?"]]);
    const left = outbox.unsent("33333333-3333-3333-3333-333333333333", 0);
    expect(left).toHaveLength(1);
    expect(left[0]!.failed).toBe("the node is not connected");
  });
});

describe("outbox", () => {
  test("matched by place: two messages with the same words show twice until the log holds both, then never", () => {
    const a = outbox.add("c", "again", 3);
    outbox.add("c", "again", 3);
    expect(outbox.unsent("c", 3)).toHaveLength(2);
    expect(outbox.unsent("c", 4)).toHaveLength(1);
    expect(outbox.unsent("c", 5)).toHaveLength(0);
    // Gone for good: an older log does not bring them back.
    expect(outbox.unsent("c", 3)).toHaveLength(0);
    void a;
  });

  test("a refused message frees its place for the next one", () => {
    const a = outbox.add("d", "first", 0);
    const b = outbox.add("d", "second", 0);
    expect(b.at).toBe(1);
    outbox.drop("d", a);
    expect(b.at).toBe(0);
    expect(outbox.unsent("d", 0).map((o) => o.text)).toEqual(["second"]);
    expect(outbox.unsent("d", 1)).toHaveLength(0);
  });
});
