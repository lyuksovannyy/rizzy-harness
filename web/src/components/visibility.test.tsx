// @vitest-environment happy-dom
// What the agent is doing: the retry countdown (clamped to the wait, so a
// skewed browser clock cannot show more), the context meter against the
// model's window (and a visible "window unknown"), and a sub-agent card
// following its child's session through the page's live connection.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { emptyView } from "../fold";
import { LiveCtx } from "../lib/liveCtx";
import type { Live } from "../live";
import type { Item, ModelRetry, SessionView } from "../types";
import { ContextMeter, meter } from "./ContextMeter";
import { RetryNote, secondsLeft } from "./RetryNote";
import { activityOf, activityWords, SubagentActivity } from "./SubagentActivity";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = 1_700_000_000_000;
let host: HTMLElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("retry", () => {
  const retry = (extra: Partial<ModelRetry> = {}): ModelRetry => ({
    turn_id: "t",
    model: "openai::gpt-4o",
    attempt: 2,
    of: 3,
    at_ms: NOW + 12_000,
    wait_ms: 12_000,
    reason: "rate_limit",
    ...extra,
  });

  test("seconds left: rounded up, never below zero, never more than the wait", () => {
    expect(secondsLeft(NOW + 11_200, 12_000, NOW)).toBe(12);
    expect(secondsLeft(NOW - 5_000, 12_000, NOW)).toBe(0);
    // The browser's clock is an hour behind the node's.
    expect(secondsLeft(NOW + 12_000, 12_000, NOW - 3_600_000)).toBe(12);
  });

  test("counts down, then says it is trying now", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
    act(() => root.render(<RetryNote retry={retry()} />));
    expect(host.textContent).toBe("openai::gpt-4o: rate limit. Trying again in 12 s (attempt 2 of 3)");
    await act(async () => vi.advanceTimersByTime(5_000));
    expect(host.textContent).toContain("in 7 s");
    await act(async () => vi.advanceTimersByTime(8_000));
    expect(host.textContent).toBe("openai::gpt-4o: rate limit. Trying again now (attempt 2 of 3)");
  });

  test("a class this build does not know is shown as text", () => {
    vi.spyOn(Date, "now").mockReturnValue(NOW);
    act(() => root.render(<RetryNote retry={retry({ reason: "<b>new</b>" as ModelRetry["reason"] })} />));
    expect(host.querySelector("b")).toBeNull();
    expect(host.textContent).toContain("<b>new</b>");
  });
});

describe("context meter", () => {
  test("the share used and where the summary is written", () => {
    expect(meter(79_360, 128_000, 102_400, "m").text).toBe("62% of 128k · summary at 80%");
    expect(meter(1_000, 128_000, 0, "m").text).toBe("1% of 128k · no summary");
    // A configured trigger above the window is named in tokens.
    expect(meter(1_000, 32_000, 50_000, "m").text).toBe("3% of 32k · summary at 50k");
    expect(meter(79_360, 128_000, 102_400, "m").unknown).toBe(false);
  });

  test("an unknown window says so, and how to set it", () => {
    const m = meter(12_000, undefined, 24_000, "deepseek::my-finetune");
    expect(m.unknown).toBe(true);
    expect(m.text).toBe("12k context");
    expect(m.hint).toContain("deepseek::my-finetune");
    expect(m.hint).toContain("24,000");
    expect(m.hint).toContain("context = <tokens>");
    act(() => root.render(<ContextMeter used={12_000} summaryAt={24_000} model="m" />));
    expect(host.querySelector(".window-unknown")!.textContent).toBe("window unknown");
    expect(host.querySelector(".context-meter")!.getAttribute("title")).toContain("context = <tokens>");
  });

  test("a backend that reports no limits shows the bare size", () => {
    expect(meter(12_000, undefined, undefined, "m")).toEqual({ text: "12k context", hint: null, unknown: false });
  });
});

describe("sub-agent card", () => {
  const CHILD = "22222222-2222-2222-2222-222222222222";
  const tool = (call_id: string, toolName: string, input: object, done: boolean): Item => ({
    type: "tool",
    call_id,
    author: null,
    tool: toolName,
    input,
    live_output: "",
    result: done ? { ok: true, output: "", exit_code: 0 } : null,
    approval: null,
  });
  const child = (items: Item[], extra: Partial<SessionView> = {}): SessionView => ({
    ...emptyView(),
    session_id: CHILD,
    head_seq: 5,
    items,
    running_turn: "t",
    ...extra,
  });

  test("its latest call, how many, and its tokens once it ended", () => {
    const working = child([tool("a", "read", { path: "src/lib.rs" }, true), tool("b", "bash", { command: "cargo  test\n-q" }, false)]);
    expect(activityWords(activityOf(working))).toBe("2 tool calls · now: bash cargo  test -q");
    const ended = child([tool("a", "read", { path: "src/lib.rs" }, true)], {
      running_turn: null,
      usage: { input_tokens: 12_000, output_tokens: 345, context_tokens: 0 },
    });
    expect(activityWords(activityOf(ended))).toBe("1 tool call · last: read src/lib.rs · 12k tokens");
    expect(activityWords(activityOf(child([], { pending_approvals: ["x"] })))).toBe("waiting for an approval");
    expect(activityWords(activityOf(child([])))).toBe("starting…");
  });

  test("follows the child through the page's connection, and lets go when it goes", () => {
    let current: SessionView | undefined;
    const live = {
      watch: vi.fn(() => {
        current = child([tool("a", "grep", { pattern: "TODO" }, false)]);
      }),
      unwatch: vi.fn(),
      view: () => current,
      subscribe: () => () => {},
      version: 1,
    } as unknown as Live;
    act(() =>
      root.render(
        <LiveCtx.Provider value={live}>
          <SubagentActivity sessionId={CHILD} running />
        </LiveCtx.Provider>,
      ),
    );
    expect(live.watch).toHaveBeenCalledWith(CHILD);
    act(() =>
      root.render(
        <LiveCtx.Provider value={live}>
          <SubagentActivity sessionId={CHILD} running />
        </LiveCtx.Provider>,
      ),
    );
    expect(host.textContent).toBe("1 tool call · now: grep TODO");
    act(() => root.render(<LiveCtx.Provider value={live}>{null}</LiveCtx.Provider>));
    expect(live.unwatch).toHaveBeenCalledWith(CHILD);
  });

  test("a session the viewer may not see (no log arrives) shows nothing", () => {
    const live = { watch: vi.fn(), unwatch: vi.fn(), view: () => emptyView(), subscribe: () => () => {}, version: 1 } as unknown as Live;
    act(() =>
      root.render(
        <LiveCtx.Provider value={live}>
          <SubagentActivity sessionId={CHILD} running />
        </LiveCtx.Provider>,
      ),
    );
    expect(host.textContent).toBe("");
  });
});
