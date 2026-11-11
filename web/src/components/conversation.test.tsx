// @vitest-environment happy-dom
// What a chat adds for routines, artifacts and media: the banner of a chat
// holding webhook data, the seen mark for a finished run's chat (in a
// visible page only), the saved-file and failed-save items, the header's
// "Save a file" form (agentic chats the viewer drives while the server
// keeps Artifacts), images under a message (opening the preview, or
// "removed from Media"), the bar's images, and deleting the chat with or
// without the Media files that came from it.
import { act } from "react";
import { linked } from "../lib/links.setup";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { Features, Item, RoutineMark, SessionRow, SessionView } from "../types";

const call = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
const features = vi.fn<(scope: unknown) => Promise<Features>>();
vi.mock("../api", () => ({
  api: { features: (s: unknown) => features(s), deleteSession: vi.fn(() => Promise.resolve({ ok: true })), prompt: vi.fn(), approve: vi.fn() },
  call: (...a: unknown[]) => call(...a),
  HttpError: Error,
}));
const artifactsApi = { forSession: vi.fn(() => Promise.resolve([])), save: vi.fn(() => Promise.resolve({ requested: true })) };
vi.mock("../lib/artifacts", async (original) => ({ ...(await original<typeof import("../lib/artifacts")>()), artifactsApi }));
const mediaApi = { list: vi.fn(), get: vi.fn() };
vi.mock("../lib/media", async (original) => ({ ...(await original<typeof import("../lib/media")>()), mediaApi }));
const { api } = await import("../api");
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
const FEATURES = (enabled: boolean, vision_images = false) =>
  ({ artifacts: { enabled, show: enabled, why: null }, media: { enabled: vision_images, show: vision_images, vision_images }, routines: { enabled: true } }) as unknown as Features;
/** Until `ok` holds, a render at a time (the preview is its own chunk). */
const until = (ok: () => unknown) =>
  vi.waitFor(
    async () => {
      await act(async () => {});
      if (!ok()) throw new Error("not yet");
    },
    { timeout: 15_000, interval: 10 },
  );

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
    await show([], sessionRow({ blocked: "You can't run agentic chats here." }));
    expect(features).not.toHaveBeenCalled();
    // A chat chat asks too (images), but saves no files.
    await show([], sessionRow({ profile: "chat" }), "chat");
    expect(features).toHaveBeenCalledWith({ workspace: "w1" });
    expect(host.querySelector('button[aria-label="Save a file as an artifact"]')).toBeNull();
    // Each chat is its own mount (Shell keys it by id), so it asks afresh.
    act(() => root.unmount());
    root = createRoot(host);
    features.mockResolvedValue(FEATURES(false));
    await show([], sessionRow());
    expect(host.querySelector('button[aria-label="Save a file as an artifact"]')).toBeNull();
  });
});

describe("media", () => {
  const ref = (n: number) => ({ media_id: String(n).padStart(32, "c"), sha256: "s", mime: "image/webp", name: `p${n}.webp`, width: 800, height: 600 });
  const user = (images: ReturnType<typeof ref>[]): Item => ({ type: "user", turn_id: "t1", text: "look", images });
  beforeEach(() => {
    vi.mocked(api.deleteSession).mockClear();
    mediaApi.list.mockReset();
    mediaApi.get.mockReset();
  });

  test("a message's images open the preview; one deleted from Media says so", async () => {
    mediaApi.get.mockImplementation((id: string) =>
      id === ref(1).media_id
        ? Promise.resolve({ ...ref(1), kind: "image", format: "webp", display: "inline", size: 1, chats: 1, origin: "chat", source: null, artifact: null, has_thumb: false, no_thumb: false, duration_ms: null, workspace_id: "w1", created_ms: 0, updated_ms: 0 })
        : Promise.reject(Object.assign(new Error("gone"), { status: 404 })),
    );
    await show([user([ref(1), ref(2)])], sessionRow());
    const pictures = [...host.querySelectorAll<HTMLButtonElement>(".bubble-image")];
    expect(pictures.map((b) => b.querySelector("img")!.getAttribute("src"))).toEqual([linked(`/api/media/${ref(1).media_id}/content`), linked(`/api/media/${ref(2).media_id}/content`)]);
    await act(async () => pictures[0]!.click());
    await until(() => host.querySelector("dialog.media-preview"));
    expect(host.querySelector("dialog.media-preview")!.getAttribute("aria-label")).toBe("p1.webp");
    await act(async () => pictures[1]!.click());
    await until(() => host.querySelector(".bubble-images .chip"));
    expect(host.querySelector(".bubble-images .chip")!.textContent).toBe("p2.webp: removed from Media");
  });

  test("the bar takes images where the server does", async () => {
    features.mockResolvedValue(FEATURES(true, true));
    await show([], sessionRow());
    const attach = () => host.querySelector<HTMLButtonElement>('button[aria-label="Attach files"]')!;
    expect(attach().title).toBe("Attach documents or images (images are kept in Media)");
    features.mockResolvedValue(FEATURES(true, false));
    act(() => root.unmount());
    root = createRoot(host);
    await show([], sessionRow());
    expect(attach().title).not.toContain("images");
  });

  const deleteButton = () => host.querySelector<HTMLButtonElement>('button[aria-label="Delete chat"]')!;
  const inDialog = (label: string) =>
    [...host.querySelectorAll<HTMLButtonElement>(".delete-chat button")].find((b) => b.textContent === label)!;

  test("deleting asks in the page, then offers the chat's Media files (unticked)", async () => {
    const ask = vi.fn(() => true);
    vi.stubGlobal("confirm", ask);
    mediaApi.list.mockResolvedValue({ items: [{}, {}, {}], next: null });
    await show([], sessionRow());
    await act(async () => deleteButton().click());
    expect(mediaApi.list).toHaveBeenCalledWith({ workspace: "w1", session: SID });
    const dialog = host.querySelector(".delete-chat")!;
    expect(dialog.textContent).toContain("Delete this chat for good?");
    expect(dialog.textContent).toContain("Also delete the 3 Media files that came from this chat");
    const box = dialog.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    expect(box.checked).toBe(false);
    await act(async () => box.click());
    await act(async () => inDialog("Delete").click());
    expect(api.deleteSession).toHaveBeenCalledWith(SID, { media: true });
    expect(ask).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });

  test("with no Media files from the chat (or Media off) it is just the chat; Cancel keeps it", async () => {
    mediaApi.list.mockRejectedValue(new Error("not found"));
    await show([], sessionRow());
    await act(async () => deleteButton().click());
    expect(host.querySelector(".delete-chat input")).toBeNull();
    await act(async () => inDialog("Cancel").click());
    expect(host.querySelector(".delete-chat")).toBeNull();
    expect(api.deleteSession).not.toHaveBeenCalled();
    await act(async () => deleteButton().click());
    await act(async () => inDialog("Delete").click());
    expect(api.deleteSession).toHaveBeenCalledWith(SID, { media: false });
  });

  test("one Media file reads as one", async () => {
    mediaApi.list.mockResolvedValue({ items: [{}], next: null });
    await show([], sessionRow());
    await act(async () => deleteButton().click());
    expect(host.querySelector(".delete-chat")!.textContent).toContain("Also delete the Media file that came from this chat");
  });
});

describe("drafts (AUDIT L14)", () => {
  const KEY = `harness.draft.u1.${SID}`;
  const box = () => host.querySelector<HTMLTextAreaElement>(".bottom textarea")!;
  const type = (text: string) =>
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(box(), text);
      box().dispatchEvent(new Event("input", { bubbles: true }));
    });
  async function showChat(incognito: boolean, row: SessionRow | null) {
    current = view([]);
    current.meta!.incognito = incognito;
    await act(async () => root.render(<Conversation live={live} sessionId={SID} row={row} nodes={[]} agents={[]} models={[]} />));
  }
  beforeEach(async () => {
    localStorage.clear();
    (await import("../lib/drafts")).setDraftOwner("u1");
    vi.mocked(api.deleteSession).mockClear();
    mediaApi.list.mockResolvedValue({ items: [], next: null });
  });

  test("a chat keeps what was typed under its user, and shows it again", async () => {
    await showChat(false, sessionRow());
    type("half a thought");
    expect(localStorage.getItem(KEY)).toBe("half a thought");
    act(() => root.unmount());
    root = createRoot(host);
    await showChat(false, sessionRow());
    expect(box().value).toBe("half a thought");
  });

  test("an incognito chat stores nothing, and loses a draft it had", async () => {
    localStorage.setItem(KEY, "from before");
    await showChat(true, sessionRow({ incognito: true }));
    expect(localStorage.getItem(KEY)).toBeNull();
    expect(box().value).toBe("");
    type("not to be kept");
    expect(Object.keys(localStorage).filter((k) => k.startsWith("harness.draft."))).toEqual([]);
  });

  test("deleting the chat deletes its draft", async () => {
    await showChat(false, sessionRow());
    type("never sent");
    expect(localStorage.getItem(KEY)).toBe("never sent");
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Delete chat"]')!.click());
    const del = [...host.querySelectorAll<HTMLButtonElement>(".delete-chat button")].find((b) => b.textContent === "Delete")!;
    await act(async () => del.click());
    expect(api.deleteSession).toHaveBeenCalled();
    expect(localStorage.getItem(KEY)).toBeNull();
  });

  test("storage that throws (a private window, blocked or full) never breaks the chat or typing", async () => {
    const boom = () => {
      throw new DOMException("blocked", "SecurityError");
    };
    vi.stubGlobal("localStorage", { getItem: boom, setItem: boom, removeItem: boom, key: boom });
    try {
      await showChat(false, sessionRow());
      type("still typing");
      expect(box().value).toBe("still typing");
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
