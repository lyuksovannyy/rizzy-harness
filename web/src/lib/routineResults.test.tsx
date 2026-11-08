// @vitest-environment happy-dom
// Routine results in the shell: a new row of attention's `routines`
// notifies as its routine's `notify` says (never on the first load, never
// while its chat is on screen), and a run's chat on screen in a visible page
// is marked seen. Rows open their chat, the Runs tab or the routine.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { RoutineAttention } from "../types";
import type { Route } from "./routes";

const call = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
vi.mock("../api", () => ({ call: (...a: unknown[]) => call(...a) }));
let enabled = true;
vi.mock("./notify", () => ({ notifyEnabled: () => enabled }));
const { markRunSeen, notifies, openResult, resultHash, useRoutineResults } = await import("./routineResults");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const notes: { title: string; body?: string; onclick?: () => void }[] = [];
class FakeNotification {
  onclick: (() => void) | null = null;
  constructor(title: string, o?: { body?: string }) {
    const n = { title, body: o?.body, onclick: () => this.onclick?.() };
    notes.push(n);
  }
  close() {}
}

const row = (over: Partial<RoutineAttention> = {}): RoutineAttention => ({
  kind: "result",
  routine_id: "r1",
  name: "Morning briefing",
  workspace_id: "w1",
  run_id: "run1",
  session_id: "11111111-1111-1111-1111-111111111111",
  status: "completed",
  reason: null,
  unseen: 1,
  notify: "always",
  ts_ms: 0,
  ...over,
});

function Probe(p: { route: Route; routines: RoutineAttention[] }) {
  useRoutineResults({ route: p.route, routines: p.routines, workspaceId: "w1" });
  return null;
}

let root: Root;
const home: Route = { kind: "home" };
// Shell's first list, replaced by each answer.
const INITIAL: RoutineAttention[] = [];
const render = (routines: RoutineAttention[], route: Route = home) => act(() => root.render(<Probe route={route} routines={routines} />));

beforeEach(() => {
  notes.length = 0;
  call.mockClear();
  enabled = true;
  vi.stubGlobal("Notification", FakeNotification);
  history.replaceState(null, "", "#/");
  root = createRoot(document.createElement("div"));
});
afterEach(() => {
  act(() => root.unmount());
  vi.unstubAllGlobals();
});

describe("notifications", () => {
  test("nothing for what the first load holds; a new row after it notifies", () => {
    render(INITIAL);
    render([row()]);
    expect(notes).toEqual([]);
    render([row(), row({ routine_id: "r2", name: "Digest", run_id: "run2", session_id: null })]);
    expect(notes.map((n) => [n.title, n.body])).toEqual([["Digest", "Finished"]]);
    // A newer run of the same routine is news again.
    render([row({ run_id: "run3" })]);
    expect(notes.at(-1)!.title).toBe("Morning briefing");
  });

  test("notify: always, failures or never", () => {
    expect(notifies({ kind: "result", notify: "always" })).toBe(true);
    expect(notifies({ kind: "result", notify: "failures" })).toBe(false);
    expect(notifies({ kind: "failed", notify: "failures" })).toBe(true);
    expect(notifies({ kind: "paused", notify: "failures" })).toBe(true);
    expect(notifies({ kind: "failed", notify: "never" })).toBe(false);

    render(INITIAL);
    render([]);
    render([
      row({ notify: "failures" }),
      row({ routine_id: "r2", name: "Backup", kind: "failed", notify: "failures", reason: "node offline", run_id: "b" }),
      row({ routine_id: "r3", name: "Quiet", kind: "failed", notify: "never", run_id: "q" }),
      row({ routine_id: "r4", name: "Sync", kind: "paused", notify: "failures", reason: "blocked", run_id: null }),
    ]);
    expect(notes.map((n) => [n.title, n.body])).toEqual([
      ["Backup", "Failed: node offline"],
      ["Sync", "Paused: blocked"],
    ]);
  });

  test("none while turned off, or while the run's chat is on screen", () => {
    render(INITIAL);
    render([]);
    enabled = false;
    render([row()]);
    expect(notes).toEqual([]);
    enabled = true;
    location.hash = "#/s/22222222-2222-2222-2222-222222222222";
    render([row(), row({ routine_id: "r2", run_id: "x", session_id: "22222222-2222-2222-2222-222222222222" })]);
    expect(notes).toEqual([]);
  });

  test("a click opens the run's chat", () => {
    render(INITIAL);
    render([]);
    render([row()]);
    act(() => notes[0]!.onclick!());
    expect(location.hash).toBe("#/s/11111111-1111-1111-1111-111111111111");
  });
});

describe("seen", () => {
  test("the run's chat on screen, in a visible page, marks the run seen", () => {
    const chat: Route = { kind: "session", id: "11111111-1111-1111-1111-111111111111" };
    const hidden = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    render([row()], chat);
    expect(call).not.toHaveBeenCalled();
    hidden.mockReturnValue("visible");
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    expect(call).toHaveBeenCalledWith("POST", "/api/routines/r1/runs/run1/seen");
    hidden.mockRestore();
  });

  test("a run is marked once per page, wherever it is seen from", async () => {
    await markRunSeen("r9", "once");
    await markRunSeen("r9", "once");
    expect(call.mock.calls.filter((c) => c[1] === "/api/routines/r9/runs/once/seen")).toHaveLength(1);
  });

  test("another chat, or a pause, marks nothing", () => {
    render([row()], { kind: "session", id: "33333333-3333-3333-3333-333333333333" });
    render([row({ kind: "paused" })], { kind: "session", id: "11111111-1111-1111-1111-111111111111" });
    expect(call).not.toHaveBeenCalled();
  });
});

describe("where a row leads", () => {
  test("a result opens its chat, or the Runs tab without one; a pause opens the routine", () => {
    expect(resultHash(row())).toBe("#/s/11111111-1111-1111-1111-111111111111");
    expect(resultHash(row({ kind: "failed", session_id: null }))).toBe("#/routines/r1/runs");
    expect(resultHash(row({ kind: "paused" }))).toBe("#/routines/r1");
  });

  test("opening a run without a chat marks it seen; one with a chat waits for the chat", () => {
    openResult(row({ kind: "failed", session_id: null, run_id: "nochat" }));
    expect(call).toHaveBeenCalledWith("POST", "/api/routines/r1/runs/nochat/seen");
    expect(location.hash).toBe("#/routines/r1/runs");
    call.mockClear();
    openResult(row());
    expect(call).not.toHaveBeenCalled();
  });
});
