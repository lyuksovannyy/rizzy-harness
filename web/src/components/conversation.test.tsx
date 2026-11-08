// @vitest-environment happy-dom
// What a chat adds for routines and artifacts: the banner of a chat holding
// webhook data, the seen mark for a finished run's chat (in a visible page
// only), the saved-file and failed-save items, and the header's "Save a
// file" form, offered only in agentic chats the viewer drives while the
// server keeps Artifacts.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { Features, Item, RoutineMark, SessionRow, SessionView } from "../types";

const call = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
const features = vi.fn<(scope: unknown) => Promise<Features>>();
vi.mock("../api", () => ({
  api: { features: (s: unknown) => features(s), deleteSession: vi.fn(), prompt: vi.fn(), approve: vi.fn() },
  call: (...a: unknown[]) => call(...a),
  HttpError: Error,
}));
const artifactsApi = { forSession: vi.fn(() => Promise.resolve([])), save: vi.fn(() => Promise.resolve({ requested: true })) };
vi.mock("../lib/artifacts", async (original) => ({ ...(await original<typeof import("../lib/artifacts")>()), artifactsApi }));
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
const mark = (over: Partial<RoutineMark> = {}): RoutineMark => ({
  routine_id: "r1",
  name: "Nightly",
  run_id: "run1",
  status: "completed",
  seen: false,
  promoted: false,
  untrusted: false,
  ...over,
});
const FEATURES = (enabled: boolean) =>
  ({ artifacts: { enabled, show: enabled, why: null }, media: { enabled: false, show: false }, routines: { enabled: true } }) as unknown as Features;

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

async function show(items: Item[], row: SessionRow | null, profile: "chat" | "agentic" = "agentic") {
  current = view(items, profile);
  await act(async () => root.render(<Conversation live={live} sessionId={SID} row={row} nodes={[]} agents={[]} models={[]} />));
  await act(() => new Promise((r) => setTimeout(r, 0)));
}

describe("routine chats", () => {
  test("a chat with webhook data says so and links its routine", async () => {
    await show([], sessionRow({ routine: mark({ untrusted: true }) }));
    const banner = host.querySelector(".untrusted-banner")!;
    expect(banner.textContent).toContain("Contains data from an external event, so memory and web tools stay off in this chat.");
    expect(banner.querySelector("a")!.getAttribute("href")).toBe("#/routines/r1");
    await show([], sessionRow({ routine: mark() }));
    expect(host.querySelector(".untrusted-banner")).toBeNull();
  });

  test("a finished run's chat on screen is seen, once the page is visible", async () => {
    const visible = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    await show([], sessionRow({ routine: mark({ run_id: "conv-run" }) }));
    expect(call).not.toHaveBeenCalled();
    visible.mockReturnValue("visible");
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    expect(call).toHaveBeenCalledWith("POST", "/api/routines/r1/runs/conv-run/seen");
  });

  test("a run still going, or already seen, is not marked", async () => {
    await show([], sessionRow({ routine: mark({ run_id: "going", status: "waiting" }) }));
    await show([], sessionRow({ routine: mark({ run_id: "done", seen: true }) }));
    expect(call).not.toHaveBeenCalled();
  });
});

describe("artifacts", () => {
  test("a saved file is an item; a failed save says why", async () => {
    await show(
      [
        {
          type: "artifact",
          staged: { stage_id: "st1", by: "user", name: "report.md", filename: "report.md", packed: null, size: 2048, sha256: "a".repeat(64), source_path: "report.md", meta: {} },
        },
        { type: "artifact_failed", path: "secrets/.env", name: null, error: "outside the chat's folder" },
      ],
      sessionRow(),
    );
    expect(host.querySelector(".artifact-card")!.textContent).toContain("Saved report.md · 2.0 KB");
    expect(artifactsApi.forSession).toHaveBeenCalledWith(SID);
    expect(host.querySelector(".msg.error")!.textContent).toBe("Couldn't save secrets/.env: outside the chat's folder");
  });

  test("the header's Save a file form, in an agentic chat the viewer drives", async () => {
    await show([], sessionRow());
    expect(features).toHaveBeenCalledWith({ workspace: "w1" });
    const toggle = host.querySelector<HTMLButtonElement>('button[aria-label="Save a file as an artifact"]')!;
    act(() => toggle.click());
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    const input = host.querySelector<HTMLInputElement>('.save-file-bar input[aria-label="File or folder to save"]')!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "dist");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => host.querySelector<HTMLFormElement>(".save-file-bar form")!.requestSubmit());
    expect(artifactsApi.save).toHaveBeenCalledWith(SID, "dist", "");
  });

  test("no Save a file in a chat chat, for a blocked viewer, or with Artifacts off", async () => {
    await show([], sessionRow({ profile: "chat" }), "chat");
    await show([], sessionRow({ blocked: "You can't run agentic chats here." }));
    expect(features).not.toHaveBeenCalled();
    features.mockResolvedValue(FEATURES(false));
    await show([], sessionRow());
    expect(host.querySelector('button[aria-label="Save a file as an artifact"]')).toBeNull();
  });
});
