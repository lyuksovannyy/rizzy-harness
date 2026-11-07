import { describe, expect, test } from "vitest";
import type { Item, ProgressSpec, SessionView, StepStatus, TurnEndReason, UiBlock, Variant } from "../types";
import {
  currentStep,
  finishedSteps,
  inputBlock,
  inputState,
  progressSignature,
  progressSlots,
  progressStatus,
  progressValue,
  slotKey,
} from "./blocks";

const spec = (...steps: StepStatus[]): ProgressSpec => ({
  title: "Job",
  steps: steps.map((status, i) => ({ label: `Step ${i + 1}`, status })),
});
const progress = (id: string, ordinal = 0, s = spec("active")): UiBlock => ({
  ordinal,
  hash: "00000000",
  type: "progress",
  id,
  spec: s,
});
const user = (): Item => ({ type: "user", turn_id: "t", text: "go" });
const msg = (message_id: string, blocks: UiBlock[] = [], variant: Variant | null = null): Item => ({
  type: "assistant",
  message_id,
  text: "",
  done: true,
  author: null,
  variant,
  blocks,
});
const end = (reason: TurnEndReason = "completed"): Item => ({ type: "turn_end", turn_id: "t", reason });
const idle: Pick<SessionView, "running_turn" | "variant_selection"> = { running_turn: null, variant_selection: {} };
const running = { ...idle, running_turn: "t" };

const modes = (slots: ReturnType<typeof progressSlots>) =>
  Object.fromEntries([...slots].map(([k, s]) => [k, s.mode]));

describe("progressSlots", () => {
  test("the turn's latest block with an id is full, earlier ones snapshots", () => {
    const items = [user(), msg("a", [progress("p")]), msg("b", [progress("p"), progress("q", 1)]), msg("c", [progress("p")])];
    expect(modes(progressSlots(items, running))).toEqual({
      "a#0": "snapshot",
      "b#0": "snapshot",
      "b#1": "full",
      "c#0": "full",
    });
  });

  test("each turn has its own card for the same id", () => {
    const items = [user(), msg("a", [progress("p")]), end(), user(), msg("b", [progress("p")]), msg("c", [progress("p")])];
    expect(modes(progressSlots(items, running))).toEqual({ "a#0": "full", "b#0": "snapshot", "c#0": "full" });
  });

  test("a running turn has not ended; a turn with its end, or a later turn, has", () => {
    const items = [user(), msg("a", [progress("p")]), user(), msg("b", [progress("p")])];
    let slots = progressSlots(items, running);
    expect(slots.get("a#0")).toEqual({ mode: "full", ended: true });
    expect(slots.get("b#0")).toEqual({ mode: "full", ended: false });
    slots = progressSlots([...items, end("cancelled")], idle);
    expect(slots.get("b#0")).toEqual({ mode: "full", ended: true, reason: "cancelled" });
    // Nothing runs and the end is missing from the log: nothing will move it.
    expect(progressSlots(items, idle).get("b#0")?.ended).toBe(true);
  });

  test("rejected blocks and other types get no slot", () => {
    const rejected: UiBlock = { ordinal: 0, hash: "0", type: "progress", id: "p", error: "steps: missing" };
    const chart: UiBlock = { ordinal: 1, hash: "0", type: "chart", id: "p", spec: { kind: "bar", x: { values: ["a"] }, series: [] } };
    const items = [user(), msg("a", [progress("p")]), msg("b", [rejected, chart])];
    expect(modes(progressSlots(items, running))).toEqual({ "a#0": "full" });
  });

  test("an unchosen parallel answer keeps its own card, outside the group", () => {
    const v = (index: number): Variant => ({ group: "g", index });
    const items = [user(), msg("x", [progress("p")], v(0)), msg("y", [progress("p")], v(1)), msg("z", [progress("p")])];
    // No choice yet: the first answer is the one the turn goes on from.
    expect(modes(progressSlots(items, idle))).toEqual({ "x#0": "snapshot", "y#0": "full", "z#0": "full" });
    const picked = { ...idle, variant_selection: { g: "y" } };
    expect(modes(progressSlots(items, picked))).toEqual({ "x#0": "full", "y#0": "snapshot", "z#0": "full" });
  });

  test("the signature moves when the answer can", () => {
    const items = [user(), msg("a")];
    const sig = progressSignature(items, running);
    (items[1] as Extract<Item, { type: "assistant" }>).blocks = [progress("p")];
    expect(progressSignature(items, running)).not.toBe(sig);
    expect(progressSignature(items, idle)).not.toBe(progressSignature(items, running));
    expect(progressSignature(items, { ...idle, variant_selection: { g: "b" } })).not.toBe(progressSignature(items, idle));
    expect(progressSignature([...items, end()], idle)).not.toBe(progressSignature(items, idle));
  });

  test("slotKey", () => expect(slotKey("m", 2)).toBe("m#2"));
});

describe("progress status", () => {
  test("derived from the steps when the model gave none", () => {
    expect(progressStatus(spec("done", "active"), false)).toBe("running");
    expect(progressStatus(spec("done", "skipped"), false)).toBe("done");
    expect(progressStatus(spec("done", "failed", "pending"), false)).toBe("failed");
    expect(progressStatus({ ...spec("done", "active"), status: "failed" }, false)).toBe("failed");
  });

  test("a job still running when its turn ended has stopped", () => {
    expect(progressStatus(spec("done", "active"), true)).toBe("stopped");
    expect(progressStatus({ ...spec("done"), status: "running" }, true)).toBe("stopped");
    expect(progressStatus(spec("done", "done"), true)).toBe("done");
    expect(progressStatus(spec("failed"), true)).toBe("failed");
  });

  test("value: the model's, else the share of finished steps", () => {
    expect(progressValue(spec("done", "skipped", "active", "pending"))).toBe(0.5);
    expect(progressValue({ ...spec("pending"), value: 0.4 })).toBe(0.4);
    expect(finishedSteps(spec("done", "skipped", "failed"))).toBe(2);
  });

  test("a snapshot names the first active step, else the last done one", () => {
    expect(currentStep(spec("done", "active", "active"))).toBe("Step 2");
    expect(currentStep(spec("done", "done", "pending"))).toBe("Step 2");
    expect(currentStep(spec("pending", "skipped"))).toBeNull();
  });
});

describe("inputState", () => {
  const question: UiBlock = {
    ordinal: 0,
    hash: "00000000",
    type: "choices",
    id: "colour",
    spec: { question: "Which?", options: [{ id: "blue", label: "Blue" }, { id: "red", label: "Red" }], multi: false, other: false },
  };
  const asked = (id = "q", variant: Variant | null = null) => msg(id, [question], variant);
  const answer = (message_id = "q", block_id = "colour"): Item => ({
    type: "user",
    turn_id: "t2",
    text: "Choice answer · colour: blue",
    reply: { message_id, block_id, values: { selected: ["blue"] } },
  });
  const view = (over: Partial<Pick<SessionView, "running_turn" | "variant_selection" | "meta">> = {}) => ({
    ...idle,
    meta: null,
    ...over,
  });
  const kind = (items: Item[], idx: number, v = view()) => inputState(items, idx, v).kind;

  test("open until something closes or locks it", () => {
    expect(inputState([user(), asked(), end()], 1, view())).toEqual({ kind: "open" });
  });

  test("the next user message closes it: answered only by its own reply", () => {
    expect(inputState([user(), asked(), end(), answer()], 1, view())).toEqual({
      kind: "answered",
      values: { selected: ["blue"] },
    });
    expect(kind([user(), asked(), end(), user()], 1)).toBe("not_answered");
    expect(kind([user(), asked(), end(), answer("other")], 1)).toBe("not_answered");
    expect(kind([user(), asked(), end(), answer("q", "shade")], 1)).toBe("not_answered");
    // Only the first later message counts.
    expect(kind([user(), asked(), end(), user(), answer()], 1)).toBe("not_answered");
  });

  test("a closed card stays closed whatever runs; an open one waits", () => {
    expect(kind([user(), asked(), end(), user()], 1, view({ running_turn: "t2" }))).toBe("not_answered");
    expect(kind([user(), asked()], 1, view({ running_turn: "t" }))).toBe("waiting");
  });

  test("only the parallel answer continued from is open", () => {
    const items = [user(), asked("a", { group: "g", index: 0 }), asked("b", { group: "g", index: 1 }), end()];
    expect([kind(items, 1), kind(items, 2)]).toEqual(["open", "choose_variant"]);
    const chosen = view({ variant_selection: { g: "b" } });
    expect([kind(items, 1, chosen), kind(items, 2, chosen)]).toEqual(["choose_variant", "open"]);
    // Waiting comes first.
    expect(kind(items, 2, view({ running_turn: "t" }))).toBe("waiting");
  });

  test("a sub-task's questions are not for the user", () => {
    const meta = {
      title: "t",
      cwd: "/w",
      model: "m",
      backend: "native",
      node_id: "local",
      approval_mode: "ask" as const,
      profile: "agentic" as const,
      parent_session: "01a19bf2-0000-7000-8000-00000000f1f1",
    };
    expect(kind([user(), asked(), end()], 1, view({ meta }))).toBe("sub_task");
  });

  test("the shown input block of a message", () => {
    expect(inputBlock(msg("m", [progress("p"), question]) as Extract<Item, { type: "assistant" }>)?.id).toBe("colour");
    const rejected = { ...question, spec: undefined, error: "x" } as UiBlock;
    expect(inputBlock(msg("m", [rejected]) as Extract<Item, { type: "assistant" }>)).toBeUndefined();
  });
});
