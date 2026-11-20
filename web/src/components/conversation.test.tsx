// @vitest-environment happy-dom
// What a chat adds for routines, artifacts and media: the banner of a chat
// holding webhook data, the seen mark for a finished run's chat (in a
// visible page only), the saved-file and failed-save items, the header's
// "Save a file" form (agentic chats the viewer drives while the server
// keeps Artifacts), images under a message (opening the preview, or
// "removed from Media"), the bar's images, and deleting the chat with or
// without the Media files that came from it.
import { act } from "react";
import { apply, emptyView } from "../fold";
import { SHOWN_CHARS } from "./ThinkingRow";
import { linked } from "../lib/links.setup";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { Features, Item, RoutineMark, SessionRow, SessionView } from "../types";

const prompted = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
const call = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
const features = vi.fn<(scope: unknown) => Promise<Features>>();
const agentsOf = vi.fn<(scope: unknown) => Promise<{ agents: unknown[] }>>(() => Promise.resolve({ agents: [] }));
const sessionsOf = vi.fn<(scope: unknown) => Promise<unknown>>();
vi.mock("../api", () => ({
  api: { features: (s: unknown) => features(s), sessions: (s: unknown) => sessionsOf(s), prompt: (...a: unknown[]) => prompted(...a), agents: (s: unknown) => agentsOf(s) },
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
    setAgent: vi.fn(),
    cancel: vi.fn(() => Promise.resolve({ ok: true })),
  },
}));
const artifactsApi = { forSession: vi.fn(() => Promise.resolve([])), save: vi.fn(() => Promise.resolve({ requested: true })) };
vi.mock("../lib/artifacts", async (original) => ({ ...(await original<typeof import("../lib/artifacts")>()), artifactsApi }));
const mediaApi = { list: vi.fn(), get: vi.fn() };
vi.mock("../lib/media", async (original) => ({ ...(await original<typeof import("../lib/media")>()), mediaApi }));
const { pageApi: api } = await import("../apiPages");
const { default: Conversation } = await import("./Conversation");
const { outbox } = await import("../lib/outbox");

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
  // Messages sent in one test are not on their way in the next.
  outbox.reset();
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

describe("shortened context", () => {
  test("a quiet line says what was shortened; the summary is the model's text, shown as text", async () => {
    await show(
      [
        { type: "shortened", turn_id: "t1", cleared: true, tokens_before: 31_000, tokens_after: 9_000 },
        { type: "shortened", turn_id: "t1", cleared: false, steps: 12, text: "Read <b>main.rs</b>", tokens_before: 15_200, tokens_after: 4_100 },
      ],
      sessionRow(),
    );
    const lines = [...host.querySelectorAll(".divider")].map((d) => d.textContent ?? "");
    expect(lines).toContain("Context shortened for the model (older tool results cleared): about 31k to 9.0k tokens. Everything stays shown here.");
    const folded = host.querySelector("details.summary")!;
    expect(folded.querySelector("summary")!.textContent).toBe(
      "Context shortened for the model (12 steps summarised): about 15k to 4.1k tokens. Everything stays shown here.",
    );
    expect(folded.querySelector("p")!.textContent).toBe("Read <b>main.rs</b>");
    expect(folded.querySelector("b")).toBeNull();
  });
});

describe("thinking", () => {
  test("one collapsed row per step, the model's text as text, a redacted part as one line", async () => {
    await show(
      [
        {
          type: "thinking",
          message_id: "m1",
                    blocks: [
            { type: "signed", text: "Check <b>main.rs</b> first." },
            { type: "redacted" },
            { type: "signed", text: " \n" },
            { type: "redacted" },
          ],
        },
        { type: "thinking", message_id: "m", blocks: [{ type: "redacted" }] },
        { type: "thinking", message_id: "m", blocks: [{ type: "signed", text: "" }] },
      ],
      sessionRow(),
    );
    const rows = [...host.querySelectorAll<HTMLDetailsElement>("[data-testid=thinking]")];
    expect(rows).toHaveLength(2);
    expect(rows.every((r) => !r.open)).toBe(true);
    expect(rows[0]!.querySelector("summary")!.textContent).toBe("Thinking");
    const ps = [...rows[0]!.querySelectorAll("p")].map((p) => p.textContent);
    expect(ps).toEqual(["Check <b>main.rs</b> first.", "2 part(s) redacted by the provider: not readable."]);
    expect(rows[0]!.querySelector("b")).toBeNull();
    expect(rows[1]!.querySelectorAll("p")).toHaveLength(1);
  });

  test("a long text shows its first 64 KiB until asked for all of it", async () => {
    const long = "x".repeat(SHOWN_CHARS + 500);
    await show([{ type: "thinking", message_id: "m", blocks: [{ type: "text", text: long }] }], sessionRow());
    const row = host.querySelector<HTMLDetailsElement>("[data-testid=thinking]")!;
    expect(row.querySelector("p")!.textContent).toHaveLength(SHOWN_CHARS);
    await act(async () => row.querySelector("button")!.click());
    expect(row.querySelector("p")!.textContent).toHaveLength(SHOWN_CHARS + 500);
    expect(row.querySelector("button")).toBeNull();
  });

  // Review of PR 137: a node's malformed thinking crashed the chat page.
  const malformed: [string, unknown][] = [
    ["a number", 5],
    ["a string", "x"],
    ["blocks not an array", { blocks: "x" }],
    ["a text that is a number", { blocks: [{ type: "signed", text: 7, signature: "S" }] }],
    ["a null block", { blocks: [null] }],
    ["a number block", { blocks: [3] }],
    ["a block without a type", { blocks: [{ text: "t" }, { type: "text", text: "kept" }] }],
  ];
  for (const [name, thinking] of malformed) {
    test(`malformed thinking from a node (${name}) neither breaks the fold nor the page`, async () => {
      const v = emptyView();
      apply(v, { session_id: SID, seq: 1, ts_ms: 0, kind: "assistant_message", payload: { turn_id: "t", message_id: "m1", text: "hi", tool_calls: [], thinking } } as never);
      await show(v.items, sessionRow());
      expect(host.textContent).toContain("hi");
    });
  }
});

describe("agent switches", () => {
  test("the notice names the agent and its model, from the record; the picker shows the chat's pick", async () => {
    await show(
      [{ type: "agent_changed", agent: { agent_id: "writer", name: "Writer", prompt_hash: "h" }, model: "demo::big" }],
      sessionRow({ agent_id: "researcher" }),
    );
    const lines = [...host.querySelectorAll(".divider")].map((d) => d.textContent ?? "");
    expect(lines).toContain("Switched to Writer · demo::big");
    // The row (the person's pick since) wins over the fold's last switch.
    // (No agent list here: the header names the agent by its id.)
    expect(host.textContent).toContain("researcher");
  });
});

describe("picking the chat's agent", () => {
  const agent = (id: string, name: string) => ({
    id,
    name,
    description: "",
    builtin: true,
    default_model: null,
    main_model: null,
    fallbacks: [],
    unavailable: [],
    avatar: { shape: "circle", eyes: "round", accent: "blue" },
  });
  test("shown at once, held until the server answers, a message still goes; a refusal is said and undone", async () => {
    current = view([]);
    const agents = [agent("general", "General"), agent("writer", "Writer")] as never;
    await act(async () => root.render(<Conversation live={live} sessionId={SID} row={sessionRow()} nodes={[]} agents={agents} models={[]} />));
    const button = () => host.querySelector<HTMLButtonElement>('.agent-pick button[aria-label^="agent:"]')!;
    const pick = async (name: string) => {
      act(() => button().click());
      const option = [...host.querySelectorAll<HTMLLIElement>('.agent-pick [role="option"]')].find((o) => o.textContent === name);
      await act(async () => option?.click());
    };
    let answer!: (v: unknown) => void;
    vi.mocked(api.setAgent).mockImplementationOnce(() => new Promise((r) => (answer = r)));
    await pick("Writer");
    expect(api.setAgent).toHaveBeenCalledWith(SID, "writer");
    expect(button().getAttribute("aria-label")).toBe("agent: Writer");
    expect(button().disabled).toBe(true);
    // Enter still sends: the server orders the message and the pick.
    const textarea = host.querySelector("textarea")!;
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(textarea, "meanwhile");
      textarea.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => textarea.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })));
    expect(prompted).toHaveBeenCalledWith(SID, "meanwhile", {});
    await act(async () => answer({ ok: true }));
    // Taken, but the message is still on its way: held until the chat has it
    // (lib/outbox), then enabled, with no flicker back to the old agent.
    expect(button().disabled).toBe(true);
    expect(button().getAttribute("aria-label")).toBe("agent: Writer");
    current = view([{ type: "user", turn_id: "t1", text: "meanwhile" }]);
    await act(async () => root.render(<Conversation live={live} sessionId={SID} row={sessionRow()} nodes={[]} agents={agents} models={[]} />));
    expect(button().disabled).toBe(false);
    expect(button().getAttribute("aria-label")).toBe("agent: Writer");
    // A refusal is said and the picker goes back to the chat's agent.
    vi.mocked(api.setAgent).mockImplementationOnce(() => Promise.reject(new Error("The agent is working: stop it.")));
    await pick("General");
    expect(host.textContent).toContain("The agent is working: stop it.");
    expect(button().getAttribute("aria-label")).toBe("agent: General");
  });
});

describe("hand-off notes", () => {
  test("only the latest note shows: a quiet line with the model's text as text, or why there is none", async () => {
    await show(
      [
        { type: "handoff", through_seq: 4, text: "old", model: "p::small" },
        { type: "handoff", through_seq: 9, text: "Status: <b>half</b>", model: "p::small" },
      ],
      sessionRow(),
    );
    const notes = host.querySelectorAll("[data-testid=handoff]");
    expect(notes).toHaveLength(1);
    const note = notes[0] as HTMLDetailsElement;
    expect(note.querySelector("summary")!.textContent).toBe("Hand-off note for the next chat, written by p::small");
    expect(note.querySelector("p")!.textContent).toBe("Status: <b>half</b>");
    expect(note.querySelector("b")).toBeNull();
    await show([{ type: "handoff", through_seq: 9, failed: [{ model: "x::gone", reason: "unknown to this node" }] }], sessionRow());
    expect(host.querySelector("[data-testid=handoff]")!.textContent).toBe(
      "Hand-off note: the next chat gets the facts from this chat's log only (x::gone: unknown to this node)",
    );
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

  test("the dialog says sub-agent sessions go with the chat, without a number it cannot know", async () => {
    mediaApi.list.mockRejectedValue(new Error("not found"));
    await show([], sessionRow());
    await act(async () => deleteButton().click());
    await act(() => new Promise((r) => setTimeout(r, 0)));
    const text = host.querySelector(".delete-chat")!.textContent!;
    expect(text).toContain("Delete this chat for good?");
    expect(text).toContain("Any sessions of sub-agents it started are deleted with it.");
    expect(text).not.toMatch(/\d/);
    // The sidebar's list can leave some out (a routine's runs, another
    // workspace): the dialog does not read it.
    expect(sessionsOf).not.toHaveBeenCalled();
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

describe("callback turns (T.8d step 3)", () => {
  test("a turn nobody wrote says what started it, and a refusal says why", async () => {
    await show(
      [
        { type: "user", turn_id: "t1", text: "build it" },
        { type: "turn_end", turn_id: "t1", reason: "completed" },
        { type: "background_ended", bg_id: "bg-3", reason: "exited", exit_code: 1, ran_ms: 9000, tail: "boom" },
        { type: "callback", turn_id: "t2", bg_ids: ["bg-3"] },
        { type: "assistant", message_id: "m2", text: "The build failed.", done: true, author: null, variant: null },
        { type: "turn_end", turn_id: "t2", reason: "completed" },
        { type: "background_ended", bg_id: "bg-4", reason: "timed_out", exit_code: null, ran_ms: 60000, tail: "" },
        { type: "callback_refused", bg_ids: ["bg-4"], reason: "3 turns in a row without you (node limit)" },
      ],
      sessionRow(),
    );
    const started = host.querySelector(".divider.callback")!;
    expect(started.textContent).toBe("Started by bg-3 ended (exit 1), not by you");
    // Nothing of it reads as the person's message.
    expect([...host.querySelectorAll(".msg.user")].map((m) => m.textContent)).toEqual(["build it"]);
    expect(host.querySelector(".msg.error")!.textContent).toBe(
      "bg-4 ended (timed out); the agent was not resumed: 3 turns in a row without you (node limit)",
    );
  });
});


describe("Retry on a failed turn (X.21)", () => {
  const failed: Item[] = [
    { type: "user", turn_id: "t1", text: "migrate it" },
    { type: "error", message: "The model is not available." },
    { type: "turn_end", turn_id: "t1", reason: "error" },
  ];
  const buttons = () => [...host.querySelectorAll(".divider button")].filter((b) => b.textContent === "Retry");
  test("on the failed last turn's end line; a click retries that turn", async () => {
    await show(failed, sessionRow());
    expect(buttons()).toHaveLength(1);
    await act(async () => (buttons()[0] as HTMLButtonElement).click());
    expect(api.retryTurn).toHaveBeenCalledWith(SID, "t1");
  });
  test("not after a turn that followed, a Stop, a completed turn, in a sub-agent's chat or a routine's run", async () => {
    await show([...failed, { type: "user", turn_id: "t2", text: "never mind" }, { type: "turn_end", turn_id: "t2", reason: "completed" }], sessionRow());
    expect(buttons()).toHaveLength(0);
    await show([{ type: "user", turn_id: "t1", text: "x" }, { type: "turn_end", turn_id: "t1", reason: "cancelled" }], sessionRow());
    expect(buttons()).toHaveLength(0);
    await show(failed, sessionRow({ parent_id: "p1" }));
    expect(buttons()).toHaveLength(0);
    await show(failed, sessionRow({ routine: mark({ status: "failed" }) }));
    expect(buttons()).toHaveLength(0);
    await show(failed, sessionRow({ blocked: "your level" }));
    expect(buttons()).toHaveLength(0);
  });
  test("the retry stands after the failed attempt, which stays, and the message shows once", async () => {
    await show(
      [
        ...failed,
        { type: "retry", turn_id: "t2", of: "t1", reason: "error", failed_model: "demo::small", model: "openai::gpt-4o" },
        { type: "assistant", message_id: "m2", text: "Migrated.", done: true, author: null, variant: null },
        { type: "turn_end", turn_id: "t2", reason: "completed" },
      ],
      sessionRow(),
    );
    expect([...host.querySelectorAll(".msg.user")].map((m) => m.textContent)).toEqual(["migrate it"]);
    expect(host.textContent).toContain("turn error");
    expect(host.textContent).toContain("Retried with openai::gpt-4o (the attempt above used demo::small)");
    expect(buttons()).toHaveLength(0);
  });
});

describe("messages sent while the agent works (X.5)", () => {
  test("shown where they were sent, marked until the agent reads them; the streaming reply before one is not cut", async () => {
    const items: Item[] = [
      { type: "user", turn_id: "t1", text: "fix the bug" },
      { type: "assistant", message_id: "m1", text: "Looking", done: false, author: null, variant: null },
      { type: "user", turn_id: "t1", text: "also the docs", queued: { seq: 4, read: false } },
    ];
    current = { ...view(items), running_turn: "t1" };
    await act(async () => root.render(<Conversation live={live} sessionId={SID} row={sessionRow()} nodes={[]} agents={[]} models={[]} />));
    const users = [...host.querySelectorAll(".msg.user")];
    expect(users.map((u) => u.querySelector(".bubble")!.textContent)).toEqual(["fix the bug", "also the docs"]);
    expect(users[0]!.querySelector(".queued-mark")).toBeNull();
    expect(users[1]!.querySelector(".queued-mark")!.textContent).toBe("sent while working");
    // Still the running turn's reply, streaming, not a cut-off one (a
    // final message gets its copy action).
    expect(host.querySelector(".msg.assistant")!.className).toContain("streaming");
    expect(host.querySelector(".msg.assistant .msg-actions")).toBeNull();
    // The composer stays open for more.
    expect(host.querySelector("textarea")!.disabled).toBe(false);
    // Read: the mark goes.
    await show(
      [items[0]!, { ...items[1]!, done: true } as Item, { type: "user", turn_id: "t1", text: "also the docs", queued: { seq: 4, read: true } }],
      sessionRow(),
    );
    expect(host.querySelector(".queued-mark")).toBeNull();
  });
});

describe("an unread message when no turn runs (review ST-3)", () => {
  test("says it goes with the next message", async () => {
    await show(
      [
        { type: "user", turn_id: "t1", text: "fix it" },
        { type: "user", turn_id: "t1", text: "also the docs", queued: { seq: 4, read: false } },
        { type: "turn_end", turn_id: "t1", reason: "cancelled" },
      ],
      sessionRow(),
    );
    expect(host.querySelector(".queued-mark")!.textContent).toBe("not read yet: goes with your next message");
  });
});

describe("a turn's changes (X.12)", () => {
  test("the files-changed row opens the changes beside the chat, and closing gives focus back", async () => {
    const items: Item[] = [{ type: "turn_end", turn_id: "t1", reason: "completed" }];
    await show(items, sessionRow(), "agentic", { t1: { before: "a", after: "b", files: ["a.txt"], restored: false } });
    expect(host.textContent).toContain("1 file changed");
    const open = [...host.querySelectorAll("button")].find((b) => b.textContent === "Show changes")!;
    open.focus();
    await act(async () => open.click());
    await until(() => host.querySelector(".diff-panel"));
    expect(api.turnDiff).toHaveBeenCalledWith(SID, "t1", false, expect.anything());
    // Undo stays on the row.
    expect([...host.querySelectorAll(".undo button")].some((b) => b.textContent === "Undo file changes")).toBe(true);
    const close = host.querySelector<HTMLButtonElement>("button[aria-label='Close the changes']")!;
    await act(async () => close.click());
    expect(host.querySelector(".diff-panel")).toBeNull();
    expect(document.activeElement).toBe(open);
  });
});

describe("a notice among the chat's notes", () => {
  test("is drawn as a notice, a failure as an error, both through showHidden", async () => {
    await show([{ type: "error", message: "This undo put back 150 files." , notice: true }, { type: "error", message: "It broke" }], sessionRow());
    const notice = host.querySelector(".msg.notice");
    expect(notice?.textContent).toBe("This undo put back 150 files.");
    expect(notice?.classList.contains("error")).toBe(false);
    const errors = host.querySelectorAll(".msg.error");
    expect(errors).toHaveLength(1);
    expect(errors[0]!.textContent).toBe("It broke");
  });
});

describe("events the fold could not show", () => {
  test("a chat says how many, and nothing when there are none", async () => {
    await show([{ type: "error", message: "x" }], sessionRow());
    expect(host.querySelector("[data-testid=unshown]")).toBeNull();
    current = { ...view([{ type: "error", message: "x" }]), unknown_events: 2 };
    await act(async () => root.render(<Conversation live={live} sessionId={SID} row={sessionRow()} nodes={[]} agents={[]} models={[]} />));
    expect(host.querySelector("[data-testid=unshown]")!.textContent).toBe(
      "2 of this chat's events could not be shown: this page does not know them, or they came malformed.",
    );
  });
});

// The fold keeps a tool's `input` as the node sent it (fold.ts, the guard:
// type `j`); the cards that read inside it must not throw on any shape.
describe("rows given what the fold keeps as sent", () => {
  const inputs: unknown[] = [null, 5, "x", [1, "a"], {}, { command: 5, path: {}, query: [], url: 7, agent: null, task: {}, pattern: 1, old_string: 2, new_string: [] }, { a: { b: { c: [{ d: 1 }] } } }];
  const tools = ["bash", "read", "write", "edit", "grep", "glob", "web_search", "web_fetch", "delegate", "publish", "todo", "unknown_tool"];
  for (const tool of tools) {
    test(`a ${tool} card with any input renders`, async () => {
      for (const input of inputs) {
        const item = { type: "tool", call_id: "c", author: null, tool, input, live_output: "", result: { ok: true, output: "out", exit_code: null }, approval: null } as Item;
        await show([item], sessionRow());
        expect(host.textContent).not.toBe("");
      }
    });
  }

  // A `ui:` block's spec of the wrong shape: see src/ui/uishapes.test.tsx.
});

// X.39: the header's tokens and cost estimate follow a turn while it runs.
describe("usage in the header during a turn", () => {
  const PRICED = [
    {
      id: "p::m", raw: "m", provider: "P", provider_id: "p", name: null, context: null, max_output: null,
      effort: false, vision: false, tools: false, default: false, unavailable: false,
      price: { input: 1000, output: 1000, currency: "USD" },
    },
  ] as unknown as import("../types").ModelInfo[];
  const ev = (seq: number, kind: string, payload: object) => ({ session_id: SID, seq, ts_ms: 0, kind, payload }) as import("../types").RawEvent;
  const use = (input: number, output: number) => ({ input_tokens: input, output_tokens: output, context_tokens: input, model: "p::m", by_model: { "p::m": { input_tokens: input, output_tokens: output } } });
  const header = () => host.querySelector(".conv-meta .meter")?.textContent ?? "";

  test("moves with each turn_usage, and keeps the figure when the turn ends", async () => {
    current = emptyView();
    apply(current, ev(1, "turn_started", { turn_id: "t" }));
    apply(current, ev(2, "turn_usage", { turn_id: "t", usage: use(1000, 200) }));
    const draw = async () => {
      await act(async () => root.render(<Conversation live={live} sessionId={SID} row={sessionRow()} nodes={[]} agents={[]} models={PRICED} />));
      await act(() => new Promise((r) => setTimeout(r, 0)));
    };
    await draw();
    expect(header()).toContain("1.2k tokens");
    expect(header()).toContain("≈ $1.20");
    apply(current, ev(3, "turn_usage", { turn_id: "t", usage: use(3000, 400) }));
    await draw();
    expect(header()).toContain("3.4k tokens");
    expect(header()).toContain("≈ $3.40");
    // An interrupted turn ends without usage: what it spent stays.
    apply(current, ev(4, "turn_ended", { turn_id: "t", reason: "interrupted" }));
    await draw();
    expect(header()).toContain("3.4k tokens");
    expect(header()).toContain("≈ $3.40");
  });
  test("follows a running sub-agent's turn_usage while the parent's log is silent, and never dips when it ends", async () => {
    const CHILD = "01a19bf2-0000-7000-8000-0000000000c1";
    const push = (v: import("../types").SessionView, sid: string, kind: string, payload: object) => apply(v, { ...ev(v.head_seq + 1, kind, payload), session_id: sid });
    const parent = emptyView();
    push(parent, SID, "user_message", { turn_id: "t", text: "go" });
    push(parent, SID, "turn_started", { turn_id: "t" });
    push(parent, SID, "turn_usage", { turn_id: "t", usage: use(1000, 200) });
    push(parent, SID, "tool_call_started", { turn_id: "t", call_id: "d1", tool: "delegate", input: { agent: "coder", task: "x" } });
    push(parent, SID, "tool_output_delta", { call_id: "d1", stream: "stdout", text: `session: ${CHILD}\n` });
    const kid = emptyView();
    push(kid, CHILD, "turn_started", { turn_id: "ct" });
    push(kid, CHILD, "turn_usage", { turn_id: "ct", usage: use(500, 100) });
    const watched = vi.fn();
    const both = { watch: watched, unwatch: vi.fn(), subscribe: () => () => {}, version: 0, view: (id: string) => (id === CHILD ? kid : parent), onTopic: vi.fn(() => () => {}) } as unknown as Live;
    const draw = async () => {
      await act(async () => root.render(<Conversation live={both} sessionId={SID} row={sessionRow()} nodes={[]} agents={[]} models={PRICED} />));
      await act(() => new Promise((r) => setTimeout(r, 0)));
    };
    await draw();
    // The parent's own 1.2k plus the child's running 0.6k.
    expect(header()).toContain("1.8k tokens");
    expect(header()).toContain("≈ $1.80");
    expect(watched).toHaveBeenCalledWith(CHILD);
    // The child goes on; the parent's log says nothing.
    push(kid, CHILD, "turn_usage", { turn_id: "ct", usage: use(2000, 300) });
    await draw();
    expect(header()).toContain("3.5k tokens");
    expect(header()).toContain("≈ $3.50");
    // The child's end shows before the parent's result: no dip.
    push(kid, CHILD, "turn_ended", { turn_id: "ct", reason: "completed", usage: use(2000, 300) });
    await draw();
    expect(header()).toContain("3.5k tokens");
    // The parent catches up with the child's cost in its own figure.
    push(parent, SID, "tool_call_finished", { call_id: "d1", ok: true, output: `session: ${CHILD}\nSub-agent @coder finished: done.` });
    push(parent, SID, "turn_usage", { turn_id: "t", usage: use(3200, 500) });
    await draw();
    expect(header()).toContain("3.7k tokens");
    push(parent, SID, "turn_ended", { turn_id: "t", reason: "completed" });
    await draw();
    expect(header()).toContain("3.7k tokens");
  });
});

describe("a sub-agent's chat (X.41)", () => {
  const agent = (id: string, as_agent: boolean) =>
    ({ id, name: id === "advisor" ? "Advisor" : "General", features: { as_agent, as_subagent: true }, avatar: { shape: "circle", eyes: "round", accent: "blue" }, unavailable: [], fallbacks: [] }) as unknown as import("../types").Agent;
  // The chat's own workspace's agents decide, not the page's list (which may be another workspace's).
  const render = async (row: SessionRow, listed: import("../types").Agent[], running = false, page: import("../types").Agent[] = listed) => {
    agentsOf.mockResolvedValue({ agents: listed });
    current = { ...view([]), running_turn: running ? "t1" : null };
    await act(async () => root.render(<Conversation live={live} sessionId={SID} row={row} nodes={[]} agents={page} models={[]} />));
    await act(() => new Promise((r) => setTimeout(r, 0)));
  };
  const stop = () => host.querySelector<HTMLButtonElement>("button.stop");

  test("an agent that works only as a sub-agent has no reply box, only a line", async () => {
    await render(sessionRow({ agent_id: "advisor", parent_id: "parent-1" }), [agent("advisor", false), agent("general", true)]);
    expect(host.querySelector("textarea")).toBeNull();
    const line = host.querySelector(".tag-only")!;
    expect(line.textContent).toContain("Advisor only takes tasks from other agents");
    // The banner at the top is the one link back.
    expect(line.querySelector("a")).toBeNull();
    expect(host.querySelectorAll('a[href="#/s/parent-1"]')).toHaveLength(1);
    expect(stop()).toBeNull();
  });

  test("while its turn runs the line has the composer's Stop, which cancels it", async () => {
    await render(sessionRow({ agent_id: "advisor", parent_id: "parent-1" }), [agent("advisor", false)], true);
    expect(host.querySelector("textarea")).toBeNull();
    expect(stop()!.getAttribute("aria-label")).toBe("Stop");
    await act(async () => stop()!.click());
    expect(api.cancel).toHaveBeenCalledWith(SID);
  });

  test("a sub-agent chat whose agent can also answer a person keeps its reply box", async () => {
    await render(sessionRow({ agent_id: "general", parent_id: "parent-1" }), [agent("advisor", false), agent("general", true)]);
    expect(host.querySelector("textarea")).not.toBeNull();
    expect(host.querySelector(".tag-only")).toBeNull();
  });

  test("an ordinary chat, an agent not known in the chat's workspace, and a failed lookup keep the box", async () => {
    await render(sessionRow({ agent_id: "advisor", parent_id: null }), [agent("advisor", false)]);
    expect(host.querySelector("textarea")).not.toBeNull();
    await render(sessionRow({ agent_id: "advisor", parent_id: "parent-1" }), []);
    expect(host.querySelector("textarea")).not.toBeNull();
    agentsOf.mockRejectedValue(new Error("no"));
    await act(async () => root.render(<Conversation live={live} sessionId={SID} row={sessionRow({ agent_id: "advisor", parent_id: "p" })} nodes={[]} agents={[]} models={[]} />));
    await act(() => new Promise((r) => setTimeout(r, 0)));
    expect(host.querySelector("textarea")).not.toBeNull();
  });

  test("the page's own list (another workspace's) does not lock a chat out", async () => {
    // The page holds a list where @advisor is sub-agent-only; the chat's workspace says it can answer.
    await render(sessionRow({ agent_id: "advisor", parent_id: "parent-1", workspace_id: "w2" }), [agent("advisor", true)], false, [agent("advisor", false)]);
    expect(host.querySelector("textarea")).not.toBeNull();
    expect(agentsOf).toHaveBeenCalledWith({ workspace: "w2" });
  });
});
