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
const running = (items: Item[], head_seq = 5) =>
  view(items, { running_turn: "t2", running_agent: { agent_id: "general", name: "Assistant", prompt_hash: "" }, head_seq });

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

describe("the agent picker while a message is on its way (PR 135)", () => {
  const picker = () => host.querySelector<HTMLButtonElement>('.composer button.select-button[aria-label^="agent:"]')!;
  const hover = () => picker().closest(".agent-pick")?.getAttribute("title");

  test("held from the click on Send until the logged row, with the running turn's hover text", async () => {
    const d = deferred();
    prompt.mockReturnValue(d.p);
    await show(view(earlier));
    expect(picker().disabled).toBe(false);
    type("to this agent");
    await send();
    expect(picker().disabled).toBe(true);
    expect(hover()).toBe("Stop the agent to pick another");
    await act(async () => d.ok({ ok: true }));
    // Taken, not logged yet: still held.
    expect(picker().disabled).toBe(true);
    await show(view([...earlier, { type: "user", turn_id: "t2", text: "to this agent" }, { type: "turn_end", turn_id: "t2", reason: "completed" } as Item]));
    expect(picker().disabled).toBe(false);
  });

  test("a refused send frees it again", async () => {
    prompt.mockRejectedValue(new Error("the node is not connected"));
    await show(view(earlier));
    type("refused");
    await send();
    expect(picker().disabled).toBe(false);
  });
});

describe("after the server took it", () => {
  test("no longer marked Sending… while the log's message is on its way (a slow socket); still one bubble", async () => {
    prompt.mockResolvedValue({ ok: true });
    await show(view(earlier));
    type("taken");
    await send();
    const mine = bubbles("taken");
    expect(mine).toHaveLength(1);
    expect(mine[0]!.classList.contains("sending")).toBe(false);
    expect(mine[0]!.textContent).not.toContain("Sending…");
    await show(running([...earlier, { type: "user", turn_id: "t2", text: "taken" }], 6));
    expect(bubbles("taken")).toHaveLength(1);
  });
});

describe("a new chat's first message, refused after the chat opened", () => {
  const files = [{ attachment_id: "a1", name: "notes.txt", mime: "text/plain", chars: 5, truncated: false }];
  const pictures = [{ key: "k1", name: "pier.png", size: 10, media_id: "m1" }];
  const sub = { text: "hello from home", mode: "ask", agentId: "", model: "slow::slow", effort: null, nodeId: "", cwd: "", incognito: false, attachments: ["a1"], images: ["m1"], compareWith: null, uploads: files, pictures } as never;
  async function refused() {
    const o = outbox.add(SID, "hello from home", 0, sub);
    await show(view([]));
    await act(async () => outbox.drop(SID, o, "the node is not connected"));
    await show(view([]));
  }
  const failed = () => host.querySelector(".msg.user.failed");
  const button = (label: string) => [...failed()!.querySelectorAll("button")].find((b) => b.textContent === label)!;

  test("the bubble says why and claims nothing else; the box has its text, attachment and image", async () => {
    await refused();
    expect(failed()?.textContent).toContain("Not sent: the node is not connected.");
    expect(failed()?.textContent).not.toContain("reply box");
    expect([...failed()!.querySelectorAll(".chip")].map((c) => c.textContent?.trim())).toEqual(["notes.txt", "pier.png"]);
    expect(box().value).toBe("hello from home");
    expect([...host.querySelectorAll(".composer .chip")].map((c) => c.textContent?.trim())).toContain("notes.txt");
  });

  test("Retry sends it again with its attachment and image, and takes it out of the box", async () => {
    await refused();
    prompt.mockResolvedValue({ ok: true });
    await act(async () => button("Retry").click());
    expect(prompt).toHaveBeenCalledWith(SID, "hello from home", expect.objectContaining({ attachments: ["a1"], images: ["m1"] }));
    expect(failed()).toBeNull();
    expect(box().value).toBe("");
    expect(bubbles("hello from home")).toHaveLength(1);
  });

  test("Retry refused again: the bubble comes back with the new reason", async () => {
    await refused();
    prompt.mockRejectedValue(new Error("still down"));
    await act(async () => button("Retry").click());
    expect(failed()?.textContent).toContain("Not sent: still down.");
  });

  test("Edit puts it into the box ahead of what was typed since, and the bubble goes", async () => {
    await refused();
    type("and one more thing");
    await act(async () => button("Edit").click());
    expect(failed()).toBeNull();
    expect(box().value).toBe("hello from home\nand one more thing");
  });
});

describe("the waiting line", () => {
  test("tells the wait from the answer, and counts the seconds of a long wait without reading them out", async () => {
    vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
    await show(running([...earlier, { type: "user", turn_id: "t2", text: "and the pier?" }]));
    expect(status()?.textContent).toBe("Assistant is waiting for the model…");
    // A long wait, counted on this tab's clock from when it saw the event.
    await act(async () => vi.advanceTimersByTime(12_000));
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
      ], 6),
    );
    expect(status()?.textContent).toBe("Assistant is answering…");
    expect(status()?.querySelector(".waited")).toBeNull();
    // Not said again within the turn: the region keeps what it said.
    expect(host.querySelector('[role="status"].sr-only')?.textContent).toBe("Assistant is waiting for the model…");
  });

  test("a new event starts the count again; an approval is said once, the steps between are not", async () => {
    vi.useFakeTimers({ toFake: ["Date", "setInterval", "clearInterval"] });
    const said = () => host.querySelector('[role="status"].sr-only')?.textContent;
    const tool = (result: unknown) =>
      ({ type: "tool", turn_id: "t2", call_id: "c1", tool: "shell", input: {}, author: null, live_output: "", approval: null, result }) as unknown as Item;
    const asked = [...earlier, { type: "user", turn_id: "t2", text: "go" } as Item];
    await show(running(asked, 6));
    await act(async () => vi.advanceTimersByTime(9_000));
    expect(status()?.querySelector(".waited")?.textContent).toBe("9 s");
    await show(running([...asked, tool(null)], 7));
    expect(status()?.textContent).toBe("Assistant is working…");
    await show(running([...asked, tool({ ok: true, output: "", exit_code: 0 })], 8));
    expect(status()?.querySelector(".waited")).toBeNull();
    expect(said()).toBe("Assistant is waiting for the model…");
    await show({ ...running([...asked, tool(null)], 9), pending_approvals: ["a1"] });
    expect(said()).toBe("Assistant is waiting for you");
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
    // Thrown on: the home box must not learn from it or clear its images.
    await expect(startChat(s, { workspace: "w1" }, (id) => opened.push(id), fake, (c, t) => kept.push([c, t]))).rejects.toThrow("the node is not connected");
    expect(opened).toEqual(["33333333-3333-3333-3333-333333333333"]);
    expect(kept).toEqual([["33333333-3333-3333-3333-333333333333", "hello?"]]);
    const left = outbox.unsent("33333333-3333-3333-3333-333333333333", []);
    expect(left).toHaveLength(1);
    expect(left[0]!.failed).toBe("the node is not connected");
  });
});

describe("outbox", () => {
  test("matched by words from its place: the same words twice show twice until the log holds both", () => {
    const log = ["a", "b", "c"];
    outbox.add("c", "again", 3);
    outbox.add("c", "again", 3);
    expect(outbox.unsent("c", log)).toHaveLength(2);
    expect(outbox.unsent("c", [...log, "again"])).toHaveLength(1);
    expect(outbox.unsent("c", [...log, "again", "again"])).toHaveLength(0);
    // An earlier message with the same words is not it.
    expect(outbox.unsent("c", ["again", "b", "c"])).toHaveLength(2);
  });

  test("taken by the server and its words changed in the log: gone once more messages landed than are waiting", () => {
    const o = outbox.add("e", "@writer hi", 1);
    expect(outbox.unsent("e", ["x", "from another tab", "hi"])).toHaveLength(1);
    outbox.accept(o);
    // One landed: another tab's or this one, so it stays; two: one is it.
    expect(outbox.unsent("e", ["x", "from another tab"])).toHaveLength(1);
    expect(outbox.unsent("e", ["x", "from another tab", "hi"])).toHaveLength(0);
  });

  test("a refused message frees its place for the next one", () => {
    const a = outbox.add("d", "first", 0);
    const b = outbox.add("d", "second", 0);
    expect(b.at).toBe(1);
    outbox.drop("d", a);
    expect(b.at).toBe(0);
    expect(outbox.unsent("d", []).map((o) => o.text)).toEqual(["second"]);
    expect(outbox.unsent("d", ["second"])).toHaveLength(0);
  });
});
