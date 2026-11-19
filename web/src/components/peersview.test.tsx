// @vitest-environment happy-dom
// A message that called several agents, as the chat draws it (X.37): its
// mentions as chips (the called ones with the agent's mark, a plain one and
// the person's own name quieter), a quiet line where each later agent's turn
// begins, how many are left while they answer, and the note for those not asked.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { fold } from "../fold";
import type { Live } from "../live";
import type { Agent, Features, RawEvent, SessionView } from "../types";

vi.mock("../api", () => ({
  api: {
    features: () => Promise.resolve({ artifacts: { enabled: true, show: true, why: null }, media: { enabled: false, show: false, vision_images: false }, routines: { enabled: true } } as unknown as Features),
    prompt: vi.fn(),
    me: () => Promise.resolve({ ok: true, app: "x", user: { username: "alex" }, workspace: null }),
  },
  call: vi.fn(() => Promise.resolve({ ok: true })),
  HttpError: Error,
}));
vi.mock("../apiPages", () => ({
  pageApi: { deleteSession: vi.fn(), approve: vi.fn(), turnDiff: vi.fn(() => Promise.resolve({ files: [] })), restoreTurn: vi.fn(), retryTurn: vi.fn(), setAgent: vi.fn() },
}));
vi.mock("../lib/artifacts", async (original) => ({ ...(await original<typeof import("../lib/artifacts")>()), artifactsApi: { forSession: vi.fn(() => Promise.resolve([])), save: vi.fn() } }));
vi.mock("../lib/media", async (original) => ({ ...(await original<typeof import("../lib/media")>()), mediaApi: { list: vi.fn(() => Promise.resolve([])), get: vi.fn() } }));
const { default: Conversation } = await import("./Conversation");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SID = "11111111-1111-1111-1111-111111111111";
const ev = (seq: number, kind: string, payload: object): RawEvent => ({ session_id: SID, seq, ts_ms: 0, kind, payload }) as RawEvent;
const ref = (id: string) => ({ agent_id: id, name: id, prompt_hash: "h" });
const agent = (id: string): Agent => ({ id, name: id, description: "", builtin: true, default_model: null, main_model: null, fallbacks: [], unavailable: [], avatar: { shape: "circle", eyes: "round", accent: "blue" } }) as unknown as Agent;
const AGENTS = ["general", "coder", "writer", "editor"].map(agent);
const TEXT = "the @editor is plain here\n@coder @writer @editor @alex";

const head = [
  ev(1, "session_created", { title: "t", cwd: "/w", model: "m", backend: "native", node_id: "n", approval_mode: "ask", profile: "agentic", agent_id: "general" }),
  ev(2, "turn_started", { turn_id: "t1", agent: ref("coder") }),
  ev(3, "user_message", { turn_id: "t1", text: TEXT, triggers: ["coder", "writer", "editor"] }),
  ev(4, "assistant_message", { turn_id: "t1", message_id: "m1", text: "Coder here.", author: "coder" }),
  ev(5, "turn_ended", { turn_id: "t1", reason: "completed" }),
  ev(6, "turn_started", { turn_id: "t2", agent: ref("writer"), cause: { mention: { message_seq: 3, by: "person" } } }),
];

let host: HTMLElement;
let root: Root;
let current: SessionView;
const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => current, onTopic: vi.fn(() => () => {}) } as unknown as Live;
beforeEach(() => {
  vi.spyOn(console, "warn").mockImplementation(() => {});
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

async function render(events: RawEvent[]) {
  current = fold(events);
  await act(async () => root.render(<Conversation live={live} sessionId={SID} row={null} nodes={[]} agents={AGENTS} models={[]} />));
  await act(() => new Promise((r) => setTimeout(r, 10)));
}

describe("a message that called several agents", () => {
  test("draws chips for the mentions: called ones solid, the plain one and the person quiet; the text is the text", async () => {
    await render(head);
    const bubble = host.querySelector(".msg.user .bubble")!;
    expect(bubble.textContent).toBe(TEXT);
    const kinds = [...bubble.querySelectorAll(".mention")].map((m) => `${m.textContent}:${m.classList.contains("called") ? "called" : "plain"}`);
    // The first @editor is in the middle of a sentence: plain. The closing
    // line calls the three agents; the person's name is quiet.
    expect(kinds).toEqual(["@editor:plain", "@coder:called", "@writer:called", "@editor:called", "@alex:plain"]);
    expect(bubble.querySelectorAll(".mention.called svg.agent-avatar").length).toBe(3);
    expect(bubble.querySelector(".mention.plain svg")).toBeNull();
  });

  test("says where each later agent begins and how many are left while it answers", async () => {
    await render(head);
    expect(host.textContent).toContain("@writer, called by you");
    expect(host.textContent).not.toContain("Switched to");
    expect(host.querySelector("[data-testid=turn-status]")?.textContent).toMatch(/writer.*1 more after it/);
    // The last one answers: none are left.
    await render([...head, ev(7, "turn_ended", { turn_id: "t2", reason: "completed" }), ev(8, "turn_started", { turn_id: "t3", agent: ref("editor"), cause: { mention: { message_seq: 3, by: "person" } } })]);
    expect(host.querySelector("[data-testid=turn-status]")?.textContent).not.toMatch(/more after it/);
  });

  test("a mention turn is no callback line, and the agents not asked are one quiet note", async () => {
    await render([...head, ev(7, "turn_ended", { turn_id: "t2", reason: "cancelled" }), ev(8, "peers_skipped", { message_seq: 3, agents: ["editor"], reason: "stopped" })]);
    expect(host.textContent).not.toContain("not by you");
    expect(host.textContent).toContain("Not asked: @editor (you stopped the turn)");
    expect(host.querySelector("[data-testid=turn-status]")).toBeNull();
  });

  test("a mention whose agent is gone from the list is text", async () => {
    await render(head);
    current = fold(head);
    await act(async () => root.render(<Conversation live={live} sessionId={SID} row={null} nodes={[]} agents={AGENTS.filter((a) => a.id !== "writer")} models={[]} />));
    await act(() => new Promise((r) => setTimeout(r, 10)));
    expect([...host.querySelectorAll(".msg.user .mention")].map((m) => m.textContent)).not.toContain("@writer");
  });
});
