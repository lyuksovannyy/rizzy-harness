// @vitest-environment happy-dom
// A node's thinking payloads, well-formed or not (X.25, re-checks of PR
// 137): the fold and the chat page never throw on them, the page shows no
// opaque part, and a 256 KiB thinking shows its first 64 KiB until asked.
import { act } from "react";
import "../lib/links.setup";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { Features, Item, SessionRow, SessionView } from "../types";

const call = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
const features = vi.fn<(scope: unknown) => Promise<Features>>();
vi.mock("../api", () => ({
  api: { features: (s: unknown) => features(s), prompt: vi.fn() },
  call: (...a: unknown[]) => call(...a),
  HttpError: Error,
}));
vi.mock("../apiPages", () => ({
  pageApi: {
    deleteSession: vi.fn(() => Promise.resolve({ ok: true })),
    approve: vi.fn(),
    turnDiff: vi.fn(() => Promise.resolve({ files: [] })),
    restoreTurn: vi.fn(() => Promise.resolve({ ok: true })),
    retryTurn: vi.fn(() => Promise.resolve({ ok: true })),
  },
}));
const artifactsApi = { forSession: vi.fn(() => Promise.resolve([])), save: vi.fn(() => Promise.resolve({ requested: true })) };
vi.mock("../lib/artifacts", async (original) => ({ ...(await original<typeof import("../lib/artifacts")>()), artifactsApi }));
const mediaApi = { list: vi.fn(), get: vi.fn() };
vi.mock("../lib/media", async (original) => ({ ...(await original<typeof import("../lib/media")>()), mediaApi }));
const { default: Conversation } = await import("./Conversation");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SID = "11111111-1111-1111-1111-111111111111";
const view = (items: Item[], profile: "chat" | "agentic" = "agentic"): SessionView =>
  ({
    session_id: SID,
    head_seq: 5,
    meta: { title: "Nightly build", node_id: "n1", model: "demo::mock", profile, cwd: "/src", incognito: false },
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
  }) as unknown as SessionView;
const sessionRow = (over: Partial<SessionRow> = {}): SessionRow => ({
  session_id: SID,
  node_id: "n1",
  title: "Nightly build",
  cwd: "/src",
  model: "demo::mock",
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
  ...over,
});
const FEATURES = (enabled: boolean, vision_images = false) =>
  ({ artifacts: { enabled, show: enabled, why: null }, media: { enabled: vision_images, show: vision_images, vision_images }, routines: { enabled: true } }) as unknown as Features;

let host: HTMLElement;
let root: Root;
let current: SessionView;
const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => current, onTopic: vi.fn(() => () => {}) } as unknown as Live;
beforeEach(() => {
  call.mockClear();
  features.mockReset();
  features.mockResolvedValue(FEATURES(true));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

async function show(
  items: Item[],
  row: SessionRow | null,
  profile: "chat" | "agentic" = "agentic",
  checkpoints: SessionView["checkpoints"] = {},
) {
  current = { ...view(items, profile), checkpoints };
  await act(async () => root.render(<Conversation live={live} sessionId={SID} row={row} nodes={[]} agents={[]} models={[]} />));
  await act(() => new Promise((r) => setTimeout(r, 0)));
}
import { apply } from "../fold";

const ev = (seq: number, kind: string, payload: unknown) => ({ session_id: SID, seq, ts_ms: 0, kind, payload }) as never;

describe("malformed thinking and shortening from a node (re-check of PR 137)", () => {
  const thinkingCases: [string, unknown][] = [
    ["number", 5],
    ["string", "x"],
    ["null", null],
    ["blocks not array", { blocks: "x" }],
    ["blocks null", { blocks: null }],
    ["text a number", { blocks: [{ type: "signed", text: 7, signature: "S" }] }],
    ["null block", { blocks: [null] }],
    ["number block", { blocks: [3] }],
    ["type a number", { blocks: [{ type: 3, text: "t" }] }],
    ["array thinking", [1, 2]],
    ["huge text", { blocks: [{ type: "text", text: "x".repeat(1 << 20) }] }],
  ];
  const shortCases: [string, unknown][] = [
    ["thinking a number", 5],
    ["thinking null", null],
    ["steps not array", { turn_id: "t", steps: [1] }],
    ["steps a string", { turn_id: 3, steps: "x" }],
    ["thinking a string", "x"],
  ];
  const run = async (name: string, events: unknown[]) => {
    const v = view([]);
    v.head_seq = 5;
    let foldErr: unknown = null;
    try {
      events.forEach((e) => apply(v, e as never));
    } catch (e) {
      foldErr = e;
    }
    let renderErr: unknown = null;
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      await show(v.items, sessionRow());
    } catch (e) {
      renderErr = e;
    }
    err.mockRestore();
    const hasHi = host.textContent?.includes("hi") ?? false;
    // The server strips the opaque parts (`RawEvent::for_browser`); if a
    // raw event got through anyway, the page still shows none of it.
    const leaks = /SIG-OPAQUE|DATA-OPAQUE/.test(host.innerHTML);
    void name;
    expect(foldErr).toBeNull();
    expect(String(renderErr)).toBe("null");
    expect(hasHi).toBe(true);
    expect(leaks).toBe(false);
  };
  for (const [name, thinking] of thinkingCases) {
    test(`assistant_message.thinking: ${name}`, () =>
      run(name, [ev(6, "assistant_message", { turn_id: "t", message_id: "m1", text: "hi", tool_calls: [], thinking })]));
  }
  test("opaque parts on a raw event never reach the page", () =>
    run("opaque", [
      ev(6, "assistant_message", {
        turn_id: "t",
        message_id: "m1",
        text: "hi",
        tool_calls: [],
        thinking: { model: "a::b", prefix: "P", blocks: [{ type: "signed", text: "why", signature: "SIG-OPAQUE" }, { type: "redacted", data: "DATA-OPAQUE" }] },
      }),
    ]));
  for (const [name, thinking] of shortCases) {
    test(`context_shortened.thinking: ${name}`, () =>
      run(name, [
        ev(6, "assistant_message", { turn_id: "t", message_id: "m1", text: "hi", tool_calls: [] }),
        ev(7, "context_shortened", { turn_id: "t", thinking, tokens_before: 10, tokens_after: 5 }),
      ]));
  }
  test("context_shortened payload null", () =>
    run("payload null", [
      ev(6, "assistant_message", { turn_id: "t", message_id: "m1", text: "hi", tool_calls: [] }),
      ev(7, "context_shortened", null),
    ]));
});

describe("ThinkingRow on 256 KiB", () => {
  test("64 KiB first, then Show all, no freeze", async () => {
    const { ThinkingRow, SHOWN_CHARS } = await import("./ThinkingRow");
    const text = "word ".repeat((256 * 1024) / 5);
    const t0 = performance.now();
    await act(async () => root.render(<ThinkingRow it={{ type: "thinking", message_id: "m", blocks: [{ type: "signed", text }] }} />));
    const t1 = performance.now();
    const p = host.querySelector("p")!;
    void (t1 - t0);
    expect(p.textContent!.length).toBe(SHOWN_CHARS);
    const btn = host.querySelector("button")!;
    const t2 = performance.now();
    await act(async () => btn.click());
    const t3 = performance.now();
    expect(host.querySelector("p")!.textContent!.length).toBe(text.length);
    expect(host.querySelector("button")).toBeNull();
    expect(t3 - t2).toBeLessThan(2000);
  });
});
