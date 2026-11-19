// @vitest-environment happy-dom
// The live link to the server is down (the trial, 2026-11-19: a stopped
// server left the chat saying "waiting for the model… 11 s"): the turn line
// says the server is not reachable and the page is reconnecting, counts no
// seconds, and goes back by itself when the link returns.
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
const running = (items: Item[], head_seq = 5) =>
  view(items, { running_turn: "t2", running_agent: { agent_id: "general", name: "Assistant", prompt_hash: "" }, head_seq });

let host: HTMLElement;
let root: Root;
let current: SessionView;
const live = { state: "live", watch: vi.fn(), unwatch: vi.fn(), view: () => current, onTopic: vi.fn(() => () => {}) } as unknown as Live & { state: string };
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

const status = () => host.querySelector('[data-testid="turn-status"]');
async function show(v: SessionView) {
  current = v;
  await act(async () => root.render(<Conversation live={live} sessionId={SID} row={row} nodes={[]} agents={agents} models={[]} />));
}

describe("the turn line while the link is down", () => {
  test("says the server is not reachable instead of waiting for the model, and recovers", async () => {
    live.state = "live";
    await show(running(earlier));
    expect(status()?.textContent).toContain("Assistant is waiting for the model…");
    live.state = "offline";
    await show(running(earlier));
    expect(status()?.textContent).toContain("The server is not reachable. Reconnecting…");
    expect(status()?.textContent).not.toContain("waiting for the model");
    // Reconnect attempts in flight say the same.
    live.state = "connecting";
    await show(running(earlier));
    expect(status()?.textContent).toContain("not reachable");
    live.state = "live";
    await show(running(earlier));
    expect(status()?.textContent).toContain("Assistant is waiting for the model…");
    expect(status()?.textContent).not.toContain("not reachable");
  });

  test("counts no seconds while down", async () => {
    vi.useFakeTimers();
    live.state = "offline";
    await show(running(earlier));
    await act(async () => void vi.advanceTimersByTime(12_000));
    expect(status()?.textContent).not.toMatch(/\d+ ?s/);
  });

  test("is read out by a screen reader", async () => {
    live.state = "offline";
    await show(running(earlier));
    expect(status()?.getAttribute("aria-hidden")).not.toBe("true");
    expect(status()?.getAttribute("role")).toBe("status");
  });
});
