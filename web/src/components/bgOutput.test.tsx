// @vitest-environment happy-dom
// A background command's output in the side panel (X.62): closed it asks
// nothing; open it looks at once and again every few seconds while the
// command runs, once after it ended, never in a hidden tab or while the node
// is away; the node's refusal is a sentence and ends the looking; the text
// is only ever text (markup stays characters, hidden and control characters
// are made visible) and a cut says so.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { BgOutput } from "../lib/bgOutput";
import { bytesWords, ranWords } from "../lib/bgOutput";

const { call, FakeHttpError } = vi.hoisted(() => {
  class FakeHttpError extends Error {
    constructor(
      public status: number,
      message: string,
    ) {
      super(message);
    }
  }
  return { FakeHttpError, call: vi.fn((..._a: unknown[]) => Promise.resolve({}) as Promise<unknown>) };
});
vi.mock("../api", () => ({ call: (...a: unknown[]) => call(...a), HttpError: FakeHttpError }));
const { default: CommandOutput } = await import("./CommandOutput");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const SID = "11111111-1111-1111-1111-111111111111";
let root: Root;
let host: HTMLElement;

const out = (o: Partial<BgOutput> = {}): BgOutput => ({ running: true, ran_ms: 5000, printed: 12, text: "listening\n", ...o });
const mount = (node: ReactNode) => act(async () => root.render(node));
const row = (p: { ended?: boolean; away?: boolean } = {}) => (
  <CommandOutput sessionId={SID} bg="bg-1" ended={!!p.ended} away={!!p.away} status={<span>status</span>} stop={<button>Stop</button>} />
);
const toggle = () => host.querySelector<HTMLButtonElement>("button[aria-expanded]")!;
const click = (b: HTMLElement) => act(async () => b.click());
const tick = (ms: number) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));
const reads = () => call.mock.calls.filter((c) => String(c[1]).endsWith("/output")).length;

beforeEach(() => {
  vi.useFakeTimers();
  call.mockReset();
  call.mockImplementation(() => Promise.resolve(out()));
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("a command's output row", () => {
  test("closed by default: nothing is asked, and the row keeps its status and Stop", async () => {
    await mount(row());
    await tick(20_000);
    expect(call).not.toHaveBeenCalled();
    expect(host.textContent).toContain("status");
    expect(host.textContent).toContain("Stop");
    expect(toggle().getAttribute("aria-expanded")).toBe("false");
  });

  test("opening looks at once, at the command's own path, and shows the tail with its state", async () => {
    await mount(row());
    await click(toggle());
    expect(call).toHaveBeenCalledTimes(1);
    expect(call.mock.calls[0]!.slice(0, 2)).toEqual(["GET", `/api/sessions/${SID}/background/bg-1/output`]);
    const pre = host.querySelector("pre")!;
    expect(pre.textContent).toBe("listening\n");
    expect(host.textContent).toContain("running for 5 s");
    expect(toggle().getAttribute("aria-expanded")).toBe("true");
  });

  test("while it runs it looks again every few seconds; closing stops it", async () => {
    await mount(row());
    await click(toggle());
    expect(reads()).toBe(1);
    await tick(3000);
    expect(reads()).toBe(2);
    await tick(6000);
    expect(reads()).toBe(4);
    await click(toggle());
    await tick(30_000);
    expect(reads()).toBe(4);
  });

  test("a command that ended is looked at once, not again", async () => {
    call.mockImplementation(() => Promise.resolve(out({ running: false, ran_ms: 62_000 })));
    await mount(row({ ended: true }));
    await click(toggle());
    await tick(30_000);
    expect(reads()).toBe(1);
    expect(host.textContent).toContain("ran 1 min 2 s");
  });

  test("once more when the log shows the end, then no more", async () => {
    await mount(row());
    await click(toggle());
    call.mockImplementation(() => Promise.resolve(out({ running: false, text: "done\n" })));
    await mount(row({ ended: true }));
    await tick(30_000);
    expect(host.querySelector("pre")!.textContent).toBe("done\n");
    const after = reads();
    await tick(30_000);
    expect(reads()).toBe(after);
  });

  test("a hidden tab does not ask", async () => {
    await mount(row());
    await click(toggle());
    const vis = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    const before = reads();
    await tick(30_000);
    expect(reads()).toBe(before);
    vis.mockReturnValue("visible");
    await act(async () => void document.dispatchEvent(new Event("visibilitychange")));
    expect(reads()).toBe(before + 1);
  });

  test("flips of tab visibility while a look is in flight never start a second loop", async () => {
    call.mockImplementation(() => new Promise((r) => setTimeout(() => r(out()), 2000)));
    await mount(row());
    await click(toggle());
    const vis = vi.spyOn(document, "visibilityState", "get");
    for (let i = 0; i < 4; i++) {
      vis.mockReturnValue("hidden");
      await act(async () => void document.dispatchEvent(new Event("visibilitychange")));
      vis.mockReturnValue("visible");
      await act(async () => void document.dispatchEvent(new Event("visibilitychange")));
      await tick(500);
    }
    await tick(30_000);
    // One loop: a look of 2 s and a wait of 3 s is about 7 in 32 s.
    expect(reads()).toBeLessThanOrEqual(9);
  });

  test("while the node is away nothing is asked and it says so", async () => {
    await mount(row({ away: true }));
    await click(toggle());
    await tick(10_000);
    expect(call).not.toHaveBeenCalled();
    expect(host.textContent).toContain("node is away");
  });

  test("the node's refusal is a sentence and ends the looking; a busy server is tried again", async () => {
    call.mockImplementation(() => Promise.reject(new FakeHttpError(409, "no background command bg-1 in this chat")));
    await mount(row());
    await click(toggle());
    expect(host.textContent).toContain("no background command bg-1 in this chat");
    await tick(30_000);
    expect(reads()).toBe(1);
    call.mockReset();
    call.mockImplementation(() => Promise.reject(new FakeHttpError(503, "the node is busy with other outputs")));
    await click(toggle());
    await click(toggle());
    await tick(9000);
    expect(reads()).toBeGreaterThanOrEqual(3);
    expect(host.textContent).toContain("busy");
  });

  test("an empty answer is a sentence, not a crash", async () => {
    call.mockImplementation(() => Promise.resolve(null));
    await mount(row());
    await click(toggle());
    await tick(3000);
    expect(host.textContent).toContain("The server sent no output.");
  });

  test("hostile text is only text: markup stays characters, hidden and control characters are shown", async () => {
    const evil = '<script>globalThis.pwned=1</script><img src=x onerror=globalThis.pwned=1>‮rtl \u001b[31mred\u0007 <a href="https://evil.example">x</a>\n';
    call.mockImplementation(() => Promise.resolve(out({ text: evil })));
    await mount(row());
    await click(toggle());
    const pre = host.querySelector("pre")!;
    expect(pre.querySelector("script,img,a")).toBeNull();
    expect(host.querySelector("a,img,script")).toBeNull();
    expect(pre.textContent).toContain("<script>globalThis.pwned=1</script>");
    expect(pre.textContent).toContain("<U+202E>");
    expect(pre.textContent).toContain("<U+001B>");
    expect(pre.textContent).toContain("<U+0007>");
    expect((globalThis as { pwned?: number }).pwned).toBeUndefined();
  });

  test("a cut says earlier output is not shown; dropped output says so; empty says nothing yet", async () => {
    call.mockImplementation(() => Promise.resolve(out({ cut: true, printed: 5_000_000 })));
    await mount(row());
    await click(toggle());
    expect(host.textContent).toContain("earlier output not shown");
    expect(host.textContent).toContain("4.8 MB printed");
    call.mockImplementation(() => Promise.resolve(out({ dropped: true, cut: true, text: "" })));
    await click(toggle());
    await click(toggle());
    expect(host.querySelector("pre")).toBeNull();
    expect(host.textContent).toContain("no longer keeps this output");
    call.mockImplementation(() => Promise.resolve(out({ text: "", printed: 0 })));
    await click(toggle());
    await click(toggle());
    expect(host.querySelector("pre")!.textContent).toBe("(nothing printed yet)");
  });
});

describe("the words", () => {
  test("durations and sizes", () => {
    expect(ranWords(4_999)).toBe("4 s");
    expect(ranWords(61_000)).toBe("1 min 1 s");
    expect(ranWords(3_900_000)).toBe("1 h 5 min");
    expect(ranWords(-5)).toBe("0 s");
    expect(bytesWords(12)).toBe("12 bytes");
    expect(bytesWords(2048)).toBe("2.0 KB");
  });
});
