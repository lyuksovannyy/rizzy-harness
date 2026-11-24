// @vitest-environment happy-dom
// The side panel (work beside the conversation): one list with the turn's
// sub-agents on top, a separator only when both kinds are there, then the
// background commands, each a fold of the log (the same after a reload);
// a command's Stop and Stop all, the state after a click (stopping, ended,
// failed with the reason), a node that is away, the model's text never
// rendered as HTML, a time left that a skewed browser clock cannot push
// past the command's timeout or below zero; closed, a button with the
// count; nothing above the composer any more.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { commandHead, leftText, msLeft, runningBackground } from "../lib/background";
import type { Live } from "../live";
import type { Item, NodeInfo, SessionView } from "../types";

const call = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }) as Promise<unknown>);
vi.mock("../api", () => ({
  api: { features: () => Promise.resolve(null), prompt: vi.fn() },
  call: (...a: unknown[]) => call(...a),
  HttpError: Error,
}));
const cancel = vi.fn(() => Promise.resolve({ ok: true }));
vi.mock("../apiPages", () => ({
  pageApi: { cancel: () => cancel(), approve: vi.fn(), turnDiff: () => Promise.resolve({ files: [] }), restoreTurn: vi.fn() },
}));
// The header's git chip reads the server on its own (gitchip.test.tsx); here `call` counts what the panel asks.
vi.mock("./GitChip", () => ({ GitChip: () => null }));
const { default: Conversation } = await import("./Conversation");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SID = "11111111-1111-1111-1111-111111111111";
const NOW = 1_700_000_000_000;

const tool = (call_id: string, command: unknown): Item => ({
  type: "tool",
  call_id,
  author: null,
  tool: "bash",
  input: { command, background: true, timeout_secs: 600 },
  live_output: "",
  result: { ok: true, output: "[running in the background]", exit_code: null },
  approval: null,
});

function view(bg: Record<string, { ended?: boolean; left_s?: number; timeout_s?: number }>, items: Item[] = []): SessionView {
  const background: NonNullable<SessionView["background"]> = {};
  for (const [id, b] of Object.entries(bg)) {
    background[id] = {
      call_id: `c-${id}`,
      ends_by_ms: NOW + (b.left_s ?? 600) * 1000,
      timeout_s: b.timeout_s ?? 600,
      ended: !!b.ended,
    };
  }
  return {
    session_id: SID,
    head_seq: 9,
    meta: { title: "Dev server", node_id: "n1", model: "demo::mock", profile: "agentic", cwd: "/src", backend: "native", approval_mode: "auto" },
    items: [...Object.keys(bg).map((id) => tool(`c-${id}`, `npm run dev -- --port 30${id.slice(3)}`)), ...items],
    running_turn: null,
    pending_approvals: [],
    unknown_events: 0,
    running_agent: null,
    variant_selection: {},
    usage: { input_tokens: 0, output_tokens: 0, context_tokens: 0 },
    context_tokens: 0,
    checkpoints: {},
    background,
  } as unknown as SessionView;
}

const node = (connected: boolean): NodeInfo => ({ node_id: "n1", hostname: "box", version: "1", connected, embedded: false, profile: "agentic" });

let host: HTMLElement;
let root: Root;
let current: SessionView;
const live = { watch: vi.fn(), unwatch: vi.fn(), view: (id: string) => (id === SID ? current : undefined), onTopic: vi.fn(() => () => {}), subscribe: () => () => {}, version: 0 } as unknown as Live;

const until = (ok: () => unknown) =>
  vi.waitFor(
    async () => {
      await act(async () => {});
      if (!ok()) throw new Error("not yet");
    },
    { timeout: 15_000, interval: 10 },
  );

async function show(v: SessionView, nodes: NodeInfo[] = [node(true)]) {
  current = v;
  await act(async () => root.render(<Conversation live={live} sessionId={SID} row={null} nodes={nodes} agents={[]} models={[]} />));
  await act(() => new Promise((r) => setTimeout(r, 0)));
}

const strip = () => host.querySelector<HTMLElement>(".side-panel");
const row = (id: string) => host.querySelector<HTMLElement>(`.side-row[data-bg="${id}"]`);
const button = (label: string) =>
  [...host.querySelectorAll<HTMLButtonElement>(".side-panel button")].find((b) => b.textContent === label || b.getAttribute("aria-label") === label);

beforeEach(() => {
  vi.spyOn(Date, "now").mockReturnValue(NOW);
  call.mockReset();
  call.mockImplementation(() => Promise.resolve({ ok: true, stopping: [], ended: [] }));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

describe("what the strip derives from the log", () => {
  test("the head is the first line, cut by characters, and only text", () => {
    expect(commandHead("\n  npm run dev\nmore")).toEqual({ head: "npm run dev", cut: true });
    expect(commandHead("x".repeat(81))).toEqual({ head: "x".repeat(80), cut: true });
    expect(commandHead("😀".repeat(81)).head).toBe("😀".repeat(80));
    expect(commandHead({ not: "a string" })).toEqual({ head: "", cut: false });
    expect(commandHead("ls")).toEqual({ head: "ls", cut: false });
  });

  test("running commands in start order, ended ones left out", () => {
    const v = view({ "bg-10": {}, "bg-9": {}, "bg-2": { ended: true } });
    expect(runningBackground(v).map((r) => r.bg_id)).toEqual(["bg-9", "bg-10"]);
    expect(runningBackground(v)[0]!.head).toBe("npm run dev -- --port 309");
    expect(runningBackground(null)).toEqual([]);
  });

  test("a skewed browser clock never shows more than the timeout or less than nothing", () => {
    const r = { ends_by_ms: NOW + 120_000, timeout_s: 120 };
    expect(msLeft(r, NOW)).toBe(120_000);
    // The browser is an hour behind the node: still at most the timeout.
    expect(msLeft(r, NOW - 3_600_000)).toBe(120_000);
    // An hour ahead: nothing left, never negative.
    expect(msLeft(r, NOW + 3_600_000)).toBe(0);
    expect(leftText(0)).toBe("at its timeout");
    expect(leftText(45_000)).toBe("45 s left");
    expect(leftText(600_000)).toBe("10 min left");
  });
});

describe("the side panel's commands", () => {
  test("a command that ended in an earlier turn: no panel", async () => {
    await show(view({ "bg-1": { ended: true } }, [{ type: "turn_end", turn_id: "t1", reason: "completed" }, { type: "user", turn_id: "t2", text: "next" }]));
    await act(() => new Promise((r) => setTimeout(r, 50)));
    expect(strip()).toBeNull();
    expect(host.querySelector(".side-toggle")).toBeNull();
  });

  test("one runs: its id, its head as text, time left, and Stop ends it", async () => {
    const v = view({ "bg-1": { left_s: 300 } });
    // The model wrote the command: markup in it is text.
    (v.items[0] as Extract<Item, { type: "tool" }>).input.command = '<img src=x onerror="alert(1)"> npm start';
    await show(v);
    await until(strip);
    expect(row("bg-1")!.textContent).toContain("bg-1");
    expect(row("bg-1")!.querySelector("code")!.textContent).toBe('<img src=x onerror="alert(1)"> npm start');
    expect(host.querySelector(".side-panel img")).toBeNull();
    // Not above the composer: the panel is beside the conversation.
    expect(host.querySelector(".bottom .side-panel, .bottom .bg-strip")).toBeNull();
    expect(row("bg-1")!.textContent).toContain("5 min left");
    expect(button("Stop all")).toBeUndefined();

    let answer!: (v: unknown) => void;
    call.mockImplementation(() => new Promise((r) => (answer = r)));
    await act(async () => button("Stop bg-1")!.click());
    expect(call).toHaveBeenCalledWith("POST", `/api/sessions/${SID}/background/bg-1/stop`);
    expect(row("bg-1")!.textContent).toContain("stopping…");
    expect(button("Stop bg-1")!.disabled).toBe(true);
    await act(async () => answer({ ok: true, stopping: ["bg-1"], ended: [] }));
    expect(row("bg-1")!.textContent).toContain("stopping…");

    // The end arrives in the log.
    const ended = view({ "bg-1": { ended: true } }, [
      { type: "background_ended", bg_id: "bg-1", reason: "stopped_by_user", exit_code: null, ran_ms: 4000, tail: "" },
    ]);
    await show(ended);
    // It ended in this turn: still listed, as stopped, with nothing to press.
    await until(() => row("bg-1")?.querySelector(".side-state")?.textContent === "stopped");
    expect(button("Stop bg-1")).toBeUndefined();
  });

  test("two run: Stop all asks once for all of them", async () => {
    await show(view({ "bg-1": {}, "bg-2": {} }));
    await until(strip);
    expect(strip()!.textContent).toContain("2 active");
    await act(async () => button("Stop all")!.click());
    expect(call).toHaveBeenCalledTimes(1);
    expect(call).toHaveBeenCalledWith("POST", `/api/sessions/${SID}/background/stop`);
    expect(row("bg-1")!.textContent).toContain("stopping…");
    expect(row("bg-2")!.textContent).toContain("stopping…");
  });

  test("a refused stop says why and can be tried again", async () => {
    // The node's words reach the page: a hidden character in them shows as its code.
    call.mockImplementation(() => Promise.reject(new Error("no background command bg-1 in this chat (it has none)\u202E")));
    await show(view({ "bg-1": {} }));
    await until(strip);
    await act(async () => button("Stop bg-1")!.click());
    expect(row("bg-1")!.querySelector(".error-text")!.textContent).toBe("no background command bg-1 in this chat (it has none)<U+202E>");
    expect(button("Stop bg-1")!.disabled).toBe(false);
  });

  test("a node that is away: said, and nothing to press", async () => {
    await show(view({ "bg-1": {}, "bg-2": {} }), [node(false)]);
    await until(strip);
    expect(strip()!.textContent).toContain("The node box is away");
    expect(row("bg-1")!.textContent).toContain("node away");
    expect(button("Stop bg-1")!.disabled).toBe(true);
    expect(button("Stop all")!.disabled).toBe(true);
  });

  test("after a reload, the log alone: the same rows and no click of before", async () => {
    await show(view({ "bg-1": {}, "bg-2": {} }));
    await until(strip);
    await act(async () => button("Stop bg-1")!.click());
    // A new page (or a second tab) on the same log.
    act(() => root.unmount());
    root = createRoot(host);
    await show(view({ "bg-1": {}, "bg-2": {} }));
    await until(strip);
    expect([...host.querySelectorAll(".side-row")].map((r) => r.getAttribute("data-bg"))).toEqual(["bg-1", "bg-2"]);
    expect(strip()!.textContent).not.toContain("stopping…");
  });

  test("the turn's Stop leaves the strip: its commands keep running", async () => {
    const v = view({ "bg-1": {} });
    v.running_turn = "t2";
    await show(v);
    await until(strip);
    expect(strip()!.textContent).toContain("keep running when the turn stops");
    await act(async () => host.querySelector<HTMLButtonElement>(".composer button.send.stop")!.click());
    expect(cancel).toHaveBeenCalledTimes(1);
    const after = view({ "bg-1": {} }, [{ type: "turn_end", turn_id: "t2", reason: "cancelled" }]);
    await show(after);
    expect(strip()).not.toBeNull();
    expect(row("bg-1")).not.toBeNull();
    expect(call).not.toHaveBeenCalled();
  });
});

describe("the side panel's list", () => {
  const delegate = (call_id: string, agent: string, task: string, result: { ok: boolean; output: string } | null, child?: string): Item => ({
    type: "tool",
    call_id,
    author: null,
    tool: "delegate",
    input: { agent, task },
    live_output: child ? `session: ${child}\n` : "",
    result: result ? { ...result, exit_code: null } : null,
    approval: null,
  });
  const KID = "22222222-2222-2222-2222-222222222222";
  // Two sub-agents never share a session id: a second call naming the same
  // one is a follow-up, listed as that one (sideItems.test.ts).
  const KID2 = "33333333-3333-3333-3333-333333333333";
  const report = (status: string, kid = KID) => `session: ${kid}\nSub-agent @scout finished: ${status}. Its own report follows, every line behind "| "; the harness did not check it.\n| ok`;

  test("sub-agents on top, a separator, then commands; states as the log says", async () => {
    const v = view({ "bg-1": {} }, [
      { type: "user", turn_id: "t2", text: "go" },
      delegate("d1", "scout", "Find the <b>bug</b>\nand more", null, KID),
      delegate("d2", "coder", "Fix it", { ok: false, output: report("failed", KID2) }),
    ]);
    // The command started in this turn too.
    v.items.push(v.items.shift()!);
    v.running_turn = "t2";
    await show(v);
    await until(strip);
    const rows = [...host.querySelectorAll<HTMLElement>(".side-rows > li")].map((r) => r.getAttribute("data-kind") ?? r.getAttribute("role"));
    expect(rows).toEqual(["agent", "agent", "separator", "commands-head", "command"]);
    const agents = [...host.querySelectorAll<HTMLElement>('.side-row[data-kind="agent"]')];
    expect(agents[0]!.querySelector(".side-state")!.textContent).toBe("running");
    expect(agents[0]!.querySelector(".side-what")!.textContent).toBe("Find the <b>bug</b> …");
    expect(agents[0]!.querySelector("b")).toBeNull();
    expect(agents[0]!.querySelector<HTMLAnchorElement>("a")!.getAttribute("href")).toBe(`#/s/${KID}`);
    expect(agents[1]!.querySelector(".side-state")!.textContent).toBe("failed");
    expect(row("bg-1")!.querySelector(".side-state")!.textContent).toBe("running");
  });

  test("only sub-agents: no separator, no commands head", async () => {
    const v = view({}, [{ type: "user", turn_id: "t2", text: "go" }, delegate("d1", "scout", "Look", { ok: true, output: report("done") })]);
    await show(v);
    await until(strip);
    expect(host.querySelector(".side-sep")).toBeNull();
    expect(host.querySelector('[data-kind="commands-head"]')).toBeNull();
    expect(host.querySelector(".side-state")!.textContent).toBe("done");
  });

  test("a sub-agent of an earlier turn is not listed; a delegate left running by an ended turn is stopped", async () => {
    const v = view({}, [
      delegate("d0", "old", "Earlier", { ok: true, output: report("done") }),
      { type: "turn_end", turn_id: "t1", reason: "completed" },
      { type: "user", turn_id: "t2", text: "go" },
      delegate("d1", "scout", "Look", null),
      { type: "turn_end", turn_id: "t2", reason: "interrupted" },
    ]);
    await show(v);
    await until(strip);
    const agents = [...host.querySelectorAll<HTMLElement>('.side-row[data-kind="agent"]')];
    expect(agents.map((a) => a.getAttribute("data-call"))).toEqual(["d1"]);
    expect(agents[0]!.querySelector(".side-state")!.textContent).toBe("stopped");
  });

  test("closed, a button with the count; it opens again", async () => {
    const v = view({ "bg-1": {}, "bg-2": {} });
    await show(v);
    await until(strip);
    await act(async () => button("Close the side panel")!.click());
    expect(strip()).toBeNull();
    const toggle = host.querySelector<HTMLButtonElement>(".side-toggle")!;
    expect(toggle.textContent).toBe("2");
    expect(toggle.getAttribute("aria-label")).toContain("2 active");
    await act(async () => toggle.click());
    expect(strip()).not.toBeNull();
  });
});

describe("one panel owns the right side (review D7)", () => {
  test("while a turn's changes are open the side panel is its count button, and comes back on close", async () => {
    const v = view({ "bg-1": { left_s: 300 } }, [{ type: "turn_end", turn_id: "t1", reason: "completed" }]);
    v.checkpoints = { t1: { before: "a", after: "b", files: ["a.txt"], restored: false } };
    await show(v);
    await until(strip);
    const open = [...host.querySelectorAll<HTMLButtonElement>(".undo button")].find((b) => b.textContent === "Show changes")!;
    await act(async () => open.click());
    await until(() => host.querySelector(".diff-panel"));
    expect(strip()).toBeNull();
    expect(host.querySelector(".side-toggle.yielded")).not.toBeNull();
    await act(async () => host.querySelector<HTMLButtonElement>("button[aria-label='Close the changes']")!.click());
    expect(host.querySelector(".diff-panel")).toBeNull();
    expect(strip()).not.toBeNull();
    // Opening it again, then the side panel's button: the side panel takes the side back.
    await act(async () => open.click());
    await until(() => host.querySelector(".diff-panel"));
    await act(async () => host.querySelector<HTMLButtonElement>(".side-toggle.yielded")!.click());
    expect(host.querySelector(".diff-panel")).toBeNull();
    expect(strip()).not.toBeNull();
  });
});

// X.24 review F6: background sub-agents get what commands have: Stop all
// (two or more, with or without commands), "stopping…" and a refusal on the
// row, a note instead of the button while the node is away, and the line
// that they keep running after the turn's Stop.
describe("background sub-agents in the side panel", () => {
  const KIDS = ["44444444-4444-4444-4444-444444444444", "55555555-5555-5555-5555-555555555555"];
  function kids(ended = false): SessionView {
    const v = view({});
    v.items.push({ type: "user", turn_id: "t2", text: "go" });
    KIDS.forEach((kid, i) => {
      const call_id = `k${i}`;
      v.items.push({
        type: "tool",
        call_id,
        author: null,
        tool: "delegate",
        input: { agent: "scout", task: `look ${i}`, background: true },
        live_output: "",
        result: { ok: true, output: `session: ${kid}\nSub-agent @scout started in the background as bg-${i + 1}.`, exit_code: null },
        approval: null,
      });
      (v.background ??= {})[`bg-${i + 1}`] = { call_id, ends_by_ms: NOW + 600_000, timeout_s: 1800, ended, child: { session: kid, agent_id: "scout" } };
    });
    return v;
  }
  const agentRow = (i: number) => host.querySelectorAll<HTMLElement>('.side-row[data-kind="agent"]')[i]!;

  test("two run, no commands: their head says they keep running, and Stop all stops them once", async () => {
    await show(kids());
    await until(strip);
    expect(host.querySelector('[data-kind="bg-agents-head"]')!.textContent).toContain("keep running when the turn stops");
    expect(host.querySelector('[data-kind="commands-head"]')).toBeNull();
    await act(async () => button("Stop all")!.click());
    expect(call).toHaveBeenCalledTimes(1);
    expect(call).toHaveBeenCalledWith("POST", `/api/sessions/${SID}/background/stop`);
    expect(agentRow(0).textContent).toContain("stopping…");
    expect(agentRow(1).textContent).toContain("stopping…");
  });

  test("a refused stop says why on the row", async () => {
    call.mockImplementation(() => Promise.reject(new Error("no background command bg-1 in this chat")));
    await show(kids());
    await until(strip);
    await act(async () => button("Stop bg-1")!.click());
    expect(call).toHaveBeenCalledWith("POST", `/api/sessions/${SID}/background/bg-1/stop`);
    expect(agentRow(0).querySelector(".error-text")!.textContent).toBe("no background command bg-1 in this chat");
    expect(button("Stop bg-1")!.disabled).toBe(false);
  });

  test("the node away: a note on the row, no Stop to press", async () => {
    await show(kids(), [node(false)]);
    await until(strip);
    expect(agentRow(0).textContent).toContain("The node is away");
    expect(button("Stop bg-1")).toBeUndefined();
    expect(button("Stop all")!.disabled).toBe(true);
  });

  test("ended: no head, no Stop", async () => {
    await show(kids(true));
    await until(strip);
    expect(host.querySelector('[data-kind="bg-agents-head"]')).toBeNull();
    expect(button("Stop bg-1")).toBeUndefined();
  });
});

describe("a narrow screen", () => {
  const narrow = (on: boolean) => {
    window.matchMedia = ((q: string) =>
      ({ matches: on && q.includes("max-width"), media: q, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList) as typeof window.matchMedia;
  };
  const real = window.matchMedia;
  afterEach(() => {
    window.matchMedia = real;
  });
  const granted = () => ({ ...view({}), chat_allows: { g1: { tool: "write", input: { path: "a" } } } }) as SessionView;

  test("does not open the panel over the chat for a grant alone, and the button still opens it", async () => {
    narrow(true);
    await show(granted());
    expect(strip()).toBeNull();
    const open = host.querySelector<HTMLButtonElement>(".side-toggle")!;
    expect(open).not.toBeNull();
    await act(async () => open.click());
    expect(strip()).not.toBeNull();
  });

  test("still opens for running background work", async () => {
    narrow(true);
    await show(view({ "bg-1": {} }));
    expect(strip()).not.toBeNull();
  });

  test("a wide screen opens it for a grant as before", async () => {
    narrow(false);
    await show(granted());
    expect(strip()).not.toBeNull();
  });
});

