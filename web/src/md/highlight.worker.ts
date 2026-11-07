// The highlighter's worker. highlighter.ts sends one block at a time and
// kills this worker when a block runs past its budget; "started" tells it
// the grammar is loaded, so a slow network does not count against the block.
import { refractor } from "refractor/core";
import { loadGrammar } from "./highlight";
import type { Job, Reply } from "./highlighter";

const scope = self as unknown as {
  onmessage: ((e: MessageEvent<Job>) => void) | null;
  postMessage(m: Reply): void;
};

scope.onmessage = async ({ data: { id, code, grammar } }) => {
  try {
    await loadGrammar(grammar);
    scope.postMessage({ id, started: true });
    scope.postMessage({ id, tree: refractor.highlight(code, grammar) });
  } catch {
    scope.postMessage({ id, tree: null });
  }
};
