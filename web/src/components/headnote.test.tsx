// @vitest-environment happy-dom
// X.42: the note on a turn that moved the person's HEAD (on the Undo block
// and in the changes panel), and the action that puts back what an undo
// replaced.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { Features, Item, SessionRow, SessionView } from "../types";

const prompted = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
const call = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
const features = vi.fn<(scope: unknown) => Promise<Features>>();
vi.mock("../api", () => ({
  api: { features: (s: unknown) => features(s), prompt: (...a: unknown[]) => prompted(...a) },
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


const done = (over: Partial<SessionView["checkpoints"][string]> = {}) => ({ before: "a", after: "b", files: ["a.txt", "b.txt"], restored: false, ...over });
const turnEnd = (id: string): Item => ({ type: "turn_end", turn_id: id, reason: "completed" });
const sha = (c: string) => c.repeat(40);
const note = () => host.querySelector(".undo .undo-note")?.textContent ?? null;

describe("a turn that moved HEAD (X.42)", () => {
  const cases: [string, { branch?: string; commit: string }, { branch?: string; commit: string }, string][] = [
    [
      "the branch changed",
      { branch: "main", commit: sha("a") },
      { branch: "feature-x", commit: sha("a") },
      "This turn switched the branch from main to feature-x. The file count includes what the switch changed. Undo puts the files back as they were before the turn, on whichever branch is checked out now; it does not switch the branch back.",
    ],
    [
      "the commit moved on the same branch",
      { branch: "main", commit: "abc1234" + "0".repeat(33) },
      { branch: "main", commit: "def5678" + "0".repeat(33) },
      "This turn made or moved commits on main (abc1234 → def5678). Undo puts the files back; the commits stay.",
    ],
    [
      "the branch was left for a detached HEAD",
      { branch: "main", commit: sha("a") },
      { commit: sha("b") },
      "This turn left the branch main and detached HEAD at bbbbbbb. The file count includes what that changed. Undo puts the files back as they were before the turn, on whatever is checked out now; it does not switch the branch back.",
    ],
    [
      "a detached HEAD was put on a branch",
      { commit: sha("a") },
      { branch: "main", commit: sha("b") },
      "This turn switched from a detached HEAD (aaaaaaa) to the branch main. The file count includes what the switch changed. Undo puts the files back as they were before the turn, on whatever is checked out now; it does not switch the branch back.",
    ],
    ["a detached HEAD moved", { commit: sha("a") }, { commit: sha("b") }, "This turn moved a detached HEAD (aaaaaaa → bbbbbbb). Undo puts the files back; the commits stay."],
  ];
  for (const [name, from, to, text] of cases) {
    test(`the Undo block says so when ${name}`, async () => {
      await show([turnEnd("t1")], sessionRow(), "agentic", { t1: done({ head_moved: { from, to } }) });
      expect(note()).toBe(text);
      // A note, not a gate: the undo is offered as before.
      expect([...host.querySelectorAll(".undo button")].some((b) => b.textContent === "Undo file changes")).toBe(true);
    });
  }

  test("a turn that did not move it has no note", async () => {
    await show([turnEnd("t1")], sessionRow(), "agentic", { t1: done() });
    expect(note()).toBeNull();
  });

  test("names are drawn as text, never as markup, and hidden characters show as codes", async () => {
    const evil = "<img src=x onerror=alert(1)>\u202Eabc";
    await show([turnEnd("t1")], sessionRow(), "agentic", { t1: done({ head_moved: { from: { branch: "main", commit: sha("a") }, to: { branch: evil, commit: sha("a") } } }) });
    expect(host.querySelector(".undo-note img")).toBeNull();
    const code = [...host.querySelectorAll(".undo-note code")].at(-1)!;
    expect(code.textContent).toContain("<img src=x onerror=alert(1)>");
    expect(code.textContent).not.toContain("\u202E");
  });

  test("a rewind also names the later turn that moved it", async () => {
    await show([turnEnd("t1"), turnEnd("t2")], sessionRow(), "agentic", {
      t1: done(),
      t2: done({ head_moved: { from: { branch: "main", commit: sha("a") }, to: { branch: "x", commit: sha("a") } } }),
    });
    const notes = [...host.querySelectorAll(".undo-note")].map((n) => n.textContent);
    // The first turn's block names the later move before its Rewind button;
    // the second turn's block names its own.
    expect(notes.filter((n) => n?.startsWith("Rewinding also covers a later turn. This turn switched the branch from main to x"))).toHaveLength(1);
    expect(notes.filter((n) => n?.startsWith("This turn switched the branch"))).toHaveLength(1);
  });

  test("the changes panel says so too", async () => {
    await show([turnEnd("t1")], sessionRow(), "agentic", {
      t1: done({ head_moved: { from: { branch: "main", commit: sha("a") }, to: { branch: "x", commit: sha("a") } } }),
    });
    const open = [...host.querySelectorAll("button")].find((b) => b.textContent === "Show changes")!;
    await act(async () => open.click());
    await until(() => host.querySelector(".diff-panel"));
    expect(host.querySelector(".diff-panel .undo-note")!.textContent).toContain("This turn switched the branch from main to x.");
  });
});

describe("putting back what an undo replaced (X.42)", () => {
  const undone = { t1: done({ restored: true, saved: "restore-9" }), "restore-9": done() };
  const putBack = () => [...host.querySelectorAll<HTMLButtonElement>(".undo button")].find((b) => b.textContent === "Put back what this undo replaced");
  beforeEach(() => {
    vi.mocked(api.restoreTurn).mockClear();
    vi.mocked(api.restoreTurn).mockResolvedValue({ ok: true });
  });

  test("is offered after an undo and restores the id that undo saved", async () => {
    await show([turnEnd("t1")], sessionRow(), "agentic", undone);
    expect(host.textContent).toContain("undone");
    await act(async () => putBack()!.click());
    expect(api.restoreTurn).toHaveBeenCalledWith(SID, "restore-9", false, false);
  });

  test("is not offered before an undo, or without a saved checkpoint, or to a viewer who may not drive the chat", async () => {
    await show([turnEnd("t1")], sessionRow(), "agentic", { t1: done() });
    expect(putBack()).toBeUndefined();
    await show([turnEnd("t1")], sessionRow(), "agentic", { t1: done({ restored: true }) });
    expect(putBack()).toBeUndefined();
  });

  test("a refusal is shown as for any undo, with Undo anyway repeating the put-back", async () => {
    vi.mocked(api.restoreTurn).mockRejectedValueOnce(new Error("a file changed after the turn: a.txt"));
    await show([turnEnd("t1")], sessionRow(), "agentic", undone);
    await act(async () => putBack()!.click());
    expect(host.querySelector(".undo .error-text")!.textContent).toContain("a file changed after the turn: a.txt");
    const anyway = [...host.querySelectorAll<HTMLButtonElement>(".undo .error-text button")].find((b) => b.textContent === "Undo anyway")!;
    await act(async () => anyway.click());
    expect(api.restoreTurn).toHaveBeenLastCalledWith(SID, "restore-9", true, false);
  });

  test("is gone once the node says the checkpoint is not kept", async () => {
    vi.mocked(api.restoreTurn).mockRejectedValueOnce(new Error("this checkpoint is no longer kept (only the latest 100 changes are)"));
    await show([turnEnd("t1")], sessionRow(), "agentic", undone);
    await act(async () => putBack()!.click());
    expect(host.querySelector(".undo .error-text")!.textContent).toContain("no longer kept");
    expect(putBack()).toBeUndefined();
  });
});
