// @vitest-environment happy-dom
// The chat's background strip (T.8d step 2): shown while a background
// command of the open chat runs, a fold of the log (the same after a
// reload), its Stop and Stop all, the state after a click (stopping, ended,
// failed with the reason), a node that is away, the command's text never
// rendered as HTML, and a time left that a skewed browser clock cannot
// push past the command's timeout or below zero.
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
vi.mock("../apiPages", () => ({ pageApi: { cancel: () => cancel(), approve: vi.fn() } }));
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
    active_agent: "general",
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
const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => current, onTopic: vi.fn(() => () => {}) } as unknown as Live;

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

const strip = () => host.querySelector<HTMLElement>(".bg-strip");
const row = (id: string) => host.querySelector<HTMLElement>(`.bg-row[data-bg="${id}"]`);
const button = (label: string) =>
  [...host.querySelectorAll<HTMLButtonElement>(".bg-strip button")].find((b) => b.textContent === label || b.getAttribute("aria-label") === label);

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

describe("the strip", () => {
  test("nothing runs: no strip", async () => {
    await show(view({ "bg-1": { ended: true } }));
    expect(strip()).toBeNull();
  });

  test("one runs: its id, its head as text, time left, and Stop ends it", async () => {
    const v = view({ "bg-1": { left_s: 300 } });
    // The model wrote the command: markup in it is text.
    (v.items[0] as Extract<Item, { type: "tool" }>).input.command = '<img src=x onerror="alert(1)"> npm start';
    await show(v);
    await until(strip);
    expect(row("bg-1")!.textContent).toContain("bg-1");
    expect(row("bg-1")!.querySelector("code")!.textContent).toBe('<img src=x onerror="alert(1)"> npm start');
    expect(host.querySelector(".bg-strip img")).toBeNull();
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
    await until(() => row("bg-1")?.textContent?.includes("ended: stopped"));
    expect(button("Stop bg-1")).toBeUndefined();
  });

  test("two run: Stop all asks once for all of them", async () => {
    await show(view({ "bg-1": {}, "bg-2": {} }));
    await until(strip);
    expect(strip()!.textContent).toContain("2 background commands");
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
    expect([...host.querySelectorAll(".bg-row")].map((r) => r.getAttribute("data-bg"))).toEqual(["bg-1", "bg-2"]);
    expect(strip()!.textContent).not.toContain("stopping…");
  });

  test("the turn's Stop leaves the strip: its commands keep running", async () => {
    const v = view({ "bg-1": {} });
    v.running_turn = "t2";
    await show(v);
    await until(strip);
    expect(strip()!.textContent).toContain("keeps running when the turn stops");
    await act(async () => host.querySelector<HTMLButtonElement>(".composer button.send.stop")!.click());
    expect(cancel).toHaveBeenCalledTimes(1);
    const after = view({ "bg-1": {} }, [{ type: "turn_end", turn_id: "t2", reason: "cancelled" }]);
    await show(after);
    expect(strip()).not.toBeNull();
    expect(row("bg-1")).not.toBeNull();
    expect(call).not.toHaveBeenCalled();
  });
});
