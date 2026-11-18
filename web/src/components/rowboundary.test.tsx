// @vitest-environment happy-dom
// One row that throws costs that row, not the chat (review of PR 156,
// MEDIUM 4): the rows around it and the composer stay; the error is logged,
// never shown; a tool call waiting for an answer keeps a Deny (never an
// Allow: the call could not be shown).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { Features, Item, SessionView } from "../types";

vi.mock("../api", () => ({
  api: { features: () => Promise.resolve({ artifacts: { enabled: false, show: false, why: null }, media: { enabled: false, show: false, vision_images: false }, routines: { enabled: true } } as unknown as Features), prompt: vi.fn() },
  call: vi.fn(() => Promise.resolve({ ok: true })),
  HttpError: Error,
}));
const approve = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
vi.mock("../apiPages", () => ({
  pageApi: { deleteSession: vi.fn(), approve: (...a: unknown[]) => approve(...a), turnDiff: vi.fn(() => Promise.resolve({ files: [] })), restoreTurn: vi.fn(), retryTurn: vi.fn(), setAgent: vi.fn() },
}));
// The thinking row and the tool card throw while rendering, as a bug in a row would.
vi.mock("./ThinkingRow", () => ({
  SHOWN_CHARS: 1,
  ThinkingRow: () => {
    throw new Error("thinking row broke: SECRET_DETAIL");
  },
}));
vi.mock("./ToolCard", async (original) => ({
  ...(await original<typeof import("./ToolCard")>()),
  ToolCard: () => {
    throw new Error("tool card broke: SECRET_DETAIL");
  },
}));
const { default: Conversation } = await import("./Conversation");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SID = "11111111-1111-1111-1111-111111111111";
let current: SessionView;
const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => current, onTopic: vi.fn(() => () => {}) } as unknown as Live;
const view = (items: Item[]): SessionView =>
  ({
    session_id: SID,
    head_seq: 5,
    meta: { title: "t", node_id: "n1", model: "demo::mock", profile: "agentic", cwd: "/src" },
    items,
    running_turn: null,
    pending_approvals: [],
    unknown_events: 0,
    running_agent: null,
    variant_selection: {},
    usage: { input_tokens: 0, output_tokens: 0 },
    context_tokens: 0,
    checkpoints: {},
  }) as unknown as SessionView;

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  approve.mockClear();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});
async function show(items: Item[]) {
  current = view(items);
  await act(async () => root.render(<Conversation live={live} sessionId={SID} row={null} nodes={[]} agents={[]} models={[]} />));
  await act(() => new Promise((r) => setTimeout(r, 0)));
}

describe("a row that throws", () => {
  test("between two good ones: both good ones render, one line in its place, the composer stays", async () => {
    await show([
      { type: "user", turn_id: "t", text: "the first good row" },
      { type: "thinking", message_id: "m", blocks: [{ type: "text", text: "x" }] },
      { type: "error", message: "the second good row" },
    ]);
    expect(host.textContent).toContain("the first good row");
    expect(host.textContent).toContain("the second good row");
    expect(host.querySelectorAll("[data-testid=row-failed]")).toHaveLength(1);
    expect(host.querySelector("[data-testid=row-failed]")!.textContent).toBe("This item could not be shown.");
    expect(host.querySelector("[role=alert]")).toBeNull();
    expect(host.querySelector(".bottom textarea, .bottom [contenteditable]")).not.toBeNull();
    expect(host.textContent).not.toContain("SECRET_DETAIL");
    expect(JSON.stringify((console.error as unknown as { mock: { calls: unknown[][] } }).mock.calls.map((c) => c.map(String)))).toContain("SECRET_DETAIL");
  });

  test("a tool call waiting for an answer keeps a Deny, never an Allow", async () => {
    await show([
      { type: "user", turn_id: "t", text: "go" },
      { type: "tool", call_id: "c", author: null, tool: "bash", input: { command: "rm -rf /" }, live_output: "", result: null, approval: { approval_id: "a1", reason: "runs", decision: null, by: null } },
    ]);
    const line = host.querySelector("[data-testid=row-failed]")!;
    expect(line.textContent).toContain("can only be denied");
    const buttons = [...line.querySelectorAll("button")].map((b) => b.textContent);
    expect(buttons).toEqual(["Deny"]);
    await act(async () => line.querySelector("button")!.click());
    expect(approve).toHaveBeenCalledWith(SID, "a1", "deny");
  });

  test("a read-only chat shows the line without the Deny", async () => {
    current = view([{ type: "tool", call_id: "c", author: null, tool: "bash", input: {}, live_output: "", result: null, approval: { approval_id: "a1", reason: "runs", decision: null, by: null } }]);
    await act(async () =>
      root.render(<Conversation live={live} sessionId={SID} row={{ blocked: "read-only" } as never} nodes={[]} agents={[]} models={[]} />),
    );
    await act(() => new Promise((r) => setTimeout(r, 0)));
    expect(host.querySelector("[data-testid=row-failed] button")).toBeNull();
  });
});
