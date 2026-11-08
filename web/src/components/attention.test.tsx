// @vitest-environment happy-dom
// The home screen's lists under the bar: chats waiting for the user, then
// "Routine results" rows ("<routine> · Finished · 3 min ago"), which open
// the run's chat, the Runs tab or the routine, and step aside for similar
// chats while the user types, like the waiting list does.
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, test, vi } from "vitest";
import type { RoutineAttention } from "../types";

const call = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
vi.mock("../api", () => ({
  call: (...a: unknown[]) => call(...a),
  // Home asks whether the bar takes images.
  api: { features: () => Promise.resolve({ media: { vision_images: false } }) },
  HttpError: class extends Error {},
}));
// The home screen's heavy parts; only the lists under the bar are tested here.
let similarCount = 0;
vi.mock("./Composer", () => ({ Composer: () => null }));
vi.mock("./Notices", () => ({ Notices: () => null }));
vi.mock("./Similar", async () => {
  const { useEffect } = await import("react");
  return {
    Similar: (p: { onCount: (n: number) => void }) => {
      useEffect(() => p.onCount(similarCount));
      return null;
    },
  };
});
vi.mock("../bg/Backdrop", () => ({ default: () => null }));
const { Attention } = await import("./Attention");
const { Home } = await import("./Home");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function mount(node: ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() => root.render(node));
  return { host, render: (n: ReactNode) => act(() => root.render(n)), unmount: () => act(() => root.unmount()) };
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
  notify: "failures",
  ts_ms: Date.now() - 3 * 60_000,
  ...over,
});

const labels = (host: HTMLElement) => [...host.querySelectorAll(".routine-results button")].map((b) => b.textContent);

describe("Attention", () => {
  test("routine rows say what happened and when, below the waiting chats", () => {
    const m = mount(
      <Attention
        items={[{ session_id: "s1", title: "Shall we?", reason: "question", seq: 1, parent_id: null, updated_ms: 0 }]}
        routines={[
          row({ unseen: 3 }),
          row({ routine_id: "r2", name: "Backup", kind: "failed", status: "error", reason: "node offline", session_id: null }),
          row({ routine_id: "r3", name: "Sync", kind: "paused", status: "blocked", reason: "Guests can't run routines in Acme" }),
        ]}
        onOpen={() => {}}
      />,
    );
    const lists = [...m.host.querySelectorAll(".similar-label")].map((l) => l.textContent);
    expect(lists).toEqual(["Waiting for you", "Routine results"]);
    expect(labels(m.host)).toEqual([
      "Morning briefing · Finished · 3 new3m ago",
      "Backup · Failed3m agonode offline",
      "Sync · Paused3m agoGuests can't run routines in Acme",
    ]);
    // Failures read red.
    expect([...m.host.querySelectorAll(".routine-results button")].map((b) => b.className)).toEqual(["", "bad", "bad"]);
    m.unmount();
  });

  test("a row opens the run's chat, the Runs tab (marking a chatless run seen) or the routine", () => {
    const m = mount(
      <Attention
        items={[]}
        routines={[row(), row({ routine_id: "r2", kind: "failed", session_id: null, run_id: "x" }), row({ routine_id: "r3", kind: "paused" })]}
        onOpen={() => {}}
      />,
    );
    const [chat, failed, paused] = [...m.host.querySelectorAll<HTMLButtonElement>(".routine-results button")];
    act(() => chat!.click());
    expect(location.hash).toBe("#/s/11111111-1111-1111-1111-111111111111");
    expect(call).not.toHaveBeenCalled();
    act(() => failed!.click());
    expect(location.hash).toBe("#/routines/r2/runs");
    expect(call).toHaveBeenCalledWith("POST", "/api/routines/r2/runs/x/seen");
    act(() => paused!.click());
    expect(location.hash).toBe("#/routines/r3");
    m.unmount();
  });

  test("nothing at all when nothing waits", () => {
    const m = mount(<Attention items={[]} routines={[]} onOpen={() => {}} />);
    expect(m.host.innerHTML).toBe("");
    m.unmount();
  });
});

describe("Home", () => {
  const home = (routines: RoutineAttention[]) => (
    <Home
      username="alex"
      agents={[]}
      defaultAgent="general"
      models={[]}
      workers={[]}
      prefs={null}
      agenticBlocked={null}
      workspace="w1"
      onSubmit={async () => {}}
      predict={() => null}
      onLearn={() => {}}
      onOpen={() => {}}
      routines={routines}
      seed={1}
      history={{ titles: [], banned: [] }}
    />
  );

  test("shows the routine results under the bar, and hides them for similar chats", () => {
    similarCount = 0;
    const m = mount(home([row()]));
    expect(labels(m.host)).toEqual(["Morning briefing · Finished3m ago"]);
    similarCount = 2;
    m.render(home([row()]));
    expect(m.host.querySelector(".routine-results")).toBeNull();
    m.unmount();
  });
});
