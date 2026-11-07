import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { highlightable, MAX_HIGHLIGHT_CHARS, MAX_HIGHLIGHT_LINE, MAX_HIGHLIGHT_LINES } from "./Code";
import { makeHighlighter, type Job, type Reply, type WorkerLike } from "./highlighter";

describe("highlightable", () => {
  test("ordinary code is highlighted", () => {
    expect(highlightable("fn main() {}\n".repeat(200))).toBe(true);
    expect(highlightable("x".repeat(MAX_HIGHLIGHT_LINE))).toBe(true);
  });
  test("too long, too many lines or one long line stays plain", () => {
    expect(highlightable("x\n".repeat(MAX_HIGHLIGHT_CHARS / 2 + 1))).toBe(false);
    expect(highlightable("\n".repeat(MAX_HIGHLIGHT_LINES))).toBe(false);
    // Minified or hostile: one line that Prism's grammars are slow on.
    expect(highlightable("a'b\"c`d/e*f<g>h{i}j(k)l[m]n\\o".repeat(40))).toBe(false);
    expect(highlightable(`ok\n${"x".repeat(MAX_HIGHLIGHT_LINE + 1)}\nok`)).toBe(false);
  });
});

/** A worker the test drives: it records jobs and replies when told to. */
class FakeWorker implements WorkerLike {
  static all: FakeWorker[] = [];
  onmessage: ((e: MessageEvent<Reply>) => void) | null = null;
  onerror: ((e: unknown) => void) | null = null;
  jobs: Job[] = [];
  terminated = false;
  constructor() {
    FakeWorker.all.push(this);
  }
  postMessage(job: Job) {
    this.jobs.push(job);
  }
  terminate() {
    this.terminated = true;
  }
  reply(r: Reply) {
    this.onmessage?.({ data: r } as MessageEvent<Reply>);
  }
}

const tree = { type: "root", children: [] };
const flush = () => vi.advanceTimersByTimeAsync(0);

describe("makeHighlighter", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    FakeWorker.all = [];
  });
  afterEach(() => vi.useRealTimers());

  test("returns the worker's tree", async () => {
    const hl = makeHighlighter(() => new FakeWorker(), 100, 1000);
    const p = hl("x", "rust");
    await flush();
    const w = FakeWorker.all[0]!;
    expect(w.jobs).toEqual([{ id: 1, code: "x", grammar: "rust" }]);
    w.reply({ id: 1, started: true });
    w.reply({ id: 1, tree });
    expect(await p).toEqual(tree);
  });

  test("a block over budget kills the worker and stays plain; the next block gets a new worker", async () => {
    const hl = makeHighlighter(() => new FakeWorker(), 100, 1000);
    const slow = hl("hostile", "tsx");
    const next = hl("ok", "rust");
    await flush();
    const w1 = FakeWorker.all[0]!;
    w1.reply({ id: 1, started: true });
    await vi.advanceTimersByTimeAsync(101);
    expect(await slow).toBeNull();
    expect(w1.terminated).toBe(true);
    // One block at a time: the second was sent only now, to a fresh worker.
    expect(w1.jobs).toHaveLength(1);
    const w2 = FakeWorker.all[1]!;
    expect(w2.jobs.map((j) => j.code)).toEqual(["ok"]);
    w2.reply({ id: 2, started: true });
    w2.reply({ id: 2, tree });
    expect(await next).toEqual(tree);
  });

  test("the budget starts once the grammar is loaded", async () => {
    const hl = makeHighlighter(() => new FakeWorker(), 100, 1000);
    const p = hl("x", "rust");
    await vi.advanceTimersByTimeAsync(500); // a slow grammar download
    const w = FakeWorker.all[0]!;
    w.reply({ id: 1, started: true });
    await vi.advanceTimersByTimeAsync(50);
    w.reply({ id: 1, tree });
    expect(await p).toEqual(tree);
    expect(w.terminated).toBe(false);
  });

  test("a load that never finishes, or a worker error, gives up", async () => {
    const hl = makeHighlighter(() => new FakeWorker(), 100, 1000);
    const p = hl("x", "rust");
    await vi.advanceTimersByTimeAsync(1001);
    expect(await p).toBeNull();
    const q = hl("y", "rust");
    await flush();
    FakeWorker.all[1]!.onerror?.(new Error("boom"));
    expect(await q).toBeNull();
    expect(FakeWorker.all[1]!.terminated).toBe(true);
  });

  test("a block that went away before its turn is never sent", async () => {
    const hl = makeHighlighter(() => new FakeWorker(), 100, 1000);
    const first = hl("a", "rust");
    const gone = new AbortController();
    const second = hl("b", "rust", gone.signal);
    gone.abort();
    await flush();
    const w = FakeWorker.all[0]!;
    w.reply({ id: 1, started: true });
    w.reply({ id: 1, tree });
    await first;
    expect(await second).toBeNull();
    expect(w.jobs.map((j) => j.code)).toEqual(["a"]);
  });
});
