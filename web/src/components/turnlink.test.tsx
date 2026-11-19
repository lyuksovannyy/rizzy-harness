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

const region = () => host.querySelector(".sr-only[role=status]");
const advance = (ms: number) => act(async () => void vi.advanceTimersByTime(ms));
const WAIT = "Assistant is waiting for the model…";
const OFFLINE = "The server is not reachable. Reconnecting…";

describe("the turn line while the link is down", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    live.state = "live";
  });

  test("shows the offline words once the link has been down for 2 s, not before", async () => {
    await show(running(earlier));
    expect(status()?.textContent).toContain(WAIT);
    live.state = "offline";
    await show(running(earlier));
    await advance(1900);
    expect(status()?.textContent).toContain(WAIT);
    expect(region()?.textContent).not.toContain("not reachable");
    await advance(100);
    expect(status()?.textContent).toContain(OFFLINE);
    expect(status()?.textContent).not.toContain("waiting for the model");
  });

  test("offline and connecting count as one stretch: the 2 s run across both", async () => {
    live.state = "offline";
    await show(running(earlier));
    await advance(1500);
    live.state = "connecting";
    await show(running(earlier));
    await advance(500);
    expect(status()?.textContent).toContain(OFFLINE);
  });

  test("goes back at once when the link is live", async () => {
    live.state = "offline";
    await show(running(earlier));
    await advance(2500);
    expect(status()?.textContent).toContain(OFFLINE);
    live.state = "live";
    await show(running(earlier));
    expect(status()?.textContent).toContain(WAIT);
    expect(status()?.textContent).not.toContain("not reachable");
    expect(region()?.textContent).not.toContain("not reachable");
  });

  test("a blip of half a second never shows it", async () => {
    live.state = "offline";
    await show(running(earlier));
    await advance(500);
    live.state = "live";
    await show(running(earlier));
    await advance(5000);
    expect(status()?.textContent).not.toContain("not reachable");
    expect(region()?.textContent).not.toContain("not reachable");
  });

  test("the seconds keep counting for those 2 s, and stop once the offline words show", async () => {
    await show(running(earlier));
    await advance(4000);
    live.state = "offline";
    await show(running(earlier));
    await advance(1500);
    expect(status()?.textContent).toContain(WAIT);
    expect(status()?.textContent).toMatch(/5 s/);
    await advance(500);
    expect(status()?.textContent).toContain(OFFLINE);
    expect(status()?.textContent).not.toMatch(/\d+ ?s/);
  });

  test("a screen reader hears it from the always-present region; the visible line stays hidden from it", async () => {
    await show(running(earlier));
    const before = region();
    expect(before).not.toBeNull();
    live.state = "offline";
    await show(running(earlier));
    await advance(2000);
    expect(region()).toBe(before);
    expect(region()?.textContent).toBe(OFFLINE);
    expect(status()?.getAttribute("aria-hidden")).toBe("true");
    expect(status()?.getAttribute("role")).toBeNull();
    live.state = "live";
    await show(running(earlier));
    expect(region()?.textContent).not.toContain("not reachable");
  });
});
