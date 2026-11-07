// Syntax highlighting off the page's thread. Blocks go one at a time to a
// worker; one that takes longer than its budget gets the worker killed and
// stays plain text, so a hostile code block (Prism's grammars are
// super-linear on some input) cannot freeze the tab. The next block starts
// a fresh worker.
import type { HastNode } from "./hast";
import type { Grammar } from "./langs";

export interface Job {
  id: number;
  code: string;
  grammar: Grammar;
}

export type Reply = { id: number; started: true } | { id: number; tree: unknown };

/** The part of Worker this uses (tests pass a fake). */
export interface WorkerLike {
  onmessage: ((e: MessageEvent<Reply>) => void) | null;
  onerror: ((e: unknown) => void) | null;
  postMessage(job: Job): void;
  terminate(): void;
}

/** Time one block may take once its grammar is loaded; ordinary code takes
 * a few milliseconds, even on a slow phone well under this. */
export const BUDGET_MS = 1_500;
/** Time to load the worker and the grammar before giving up on a block. */
export const LOAD_MS = 20_000;

export type Highlight = (code: string, grammar: Grammar, signal?: AbortSignal) => Promise<HastNode | null>;

export function makeHighlighter(spawn: () => WorkerLike, budget = BUDGET_MS, load = LOAD_MS): Highlight {
  let worker: WorkerLike | null = null;
  let seq = 0;
  let queue: Promise<unknown> = Promise.resolve();

  const run = (code: string, grammar: Grammar) =>
    new Promise<HastNode | null>((resolve) => {
      const w = (worker ??= spawn());
      const id = ++seq;
      const finish = (tree: HastNode | null) => {
        clearTimeout(timer);
        w.onmessage = null;
        w.onerror = null;
        resolve(tree);
      };
      const kill = () => {
        w.terminate();
        if (worker === w) worker = null;
        finish(null);
      };
      let timer = setTimeout(kill, load);
      w.onmessage = ({ data }) => {
        if (data.id !== id) return;
        if ("started" in data) {
          clearTimeout(timer);
          timer = setTimeout(kill, budget);
        } else {
          finish((data.tree as HastNode | null) ?? null);
        }
      };
      w.onerror = kill;
      w.postMessage({ id, code, grammar });
    });

  return (code, grammar, signal) => {
    // A block that went away (another session opened) is skipped, not run.
    const job = queue.then(() => (signal?.aborted ? null : run(code, grammar)));
    queue = job;
    return job;
  };
}

let shared: Highlight | null = null;

export const highlightOffThread: Highlight = (code, grammar, signal) => {
  shared ??= makeHighlighter(
    () => new Worker(new URL("./highlight.worker.ts", import.meta.url), { type: "module" }) as unknown as WorkerLike,
  );
  return shared(code, grammar, signal);
};
