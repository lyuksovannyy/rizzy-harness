// Pure derivations over the folded timeline for rich message rendering.
import type { Provenance } from "../md/links";
import { parseUrl, seenKeys, urlKeysIn } from "../md/links";
import type { Item, Source } from "../types";

// User items and tool results never change once created, so their tokens
// are worked out once (the view is re-rendered on every streamed delta).
const keysCache = new WeakMap<object, string[]>();

function keysOf(owner: object, text: string): string[] {
  let keys = keysCache.get(owner);
  if (!keys) {
    keys = urlKeysIn(text);
    keysCache.set(owner, keys);
  }
  return keys;
}

// Only results of the web tools count. Any other tool output is the model's
// to steer (`echo <url>` in bash, a file it wrote and then read), so it
// could lift the confirm off exactly the URL it wants clicked.
export const WEB_TOOLS = new Set(["web_search", "web_fetch"]);

type ToolItem = Extract<Item, { type: "tool" }>;

const isWeb = (it: Item): it is ToolItem => it.type === "tool" && WEB_TOOLS.has(it.tool);

// Where citation labels resolve: the web tools' sources, and on a delegate
// result the sources its sub-task's web tools recorded (the harness copies
// them there; the sub-agent's answer cites them by the same labels). Only
// the sources, never a delegate's output: that is the sub-agent's writing.
const CITABLE = new Set([...WEB_TOOLS, "delegate"]);

const citable = (it: Item): it is ToolItem => it.type === "tool" && CITABLE.has(it.tool);

const counts = (it: Item) => it.type === "user" || (isWeb(it) && !!it.result);

/** Changes exactly when provenanceIndex's input does: a new user item or a
 * web tool that finished. */
export function provenanceSignature(items: Item[]): number {
  let n = 0;
  for (const it of items) if (counts(it)) n++;
  return n;
}

/** For each URL key, the index of the first user item or web tool result
 * that contains it. The model's own text never counts. */
export function provenanceIndex(items: Item[]): Map<string, number> {
  const first = new Map<string, number>();
  items.forEach((it, i) => {
    if (!counts(it)) return;
    const keys = it.type === "user" ? keysOf(it, it.text) : it.type === "tool" && it.result ? resultKeys(it.result) : [];
    for (const k of keys) if (!first.has(k)) first.set(k, i);
  });
  return first;
}

function resultKeys(r: NonNullable<ToolItem["result"]>): string[] {
  let keys = keysCache.get(r);
  if (!keys) {
    keys = urlKeysIn(r.output);
    for (const s of r.sources ?? []) {
      const u = parseUrl(s.url);
      if (u) keys.push(...seenKeys(u));
    }
    keysCache.set(r, keys);
  }
  return keys;
}

/** What the message at `index` may rely on: only what came before it. */
export function provenanceAt(first: Map<string, number>, index: number): Provenance {
  return { has: (key) => (first.get(key) ?? Infinity) < index };
}

const NO_SOURCES: ReadonlyMap<string, Source> = new Map();

/** The sources a message's citation labels can name. */
export type CiteMaps = (index: number) => ReadonlyMap<string, Source>;

/** Changes exactly when citeMaps' input does: a web or delegate call that
 * finished. */
export function citeSignature(items: Item[]): number {
  let n = 0;
  for (const it of items) if (citable(it) && it.result) n++;
  return n;
}

/**
 * For the item at an index: label to source, from every web or delegate
 * result before it (a later one wins for a label it repeats; labels are
 * session-stable, and a sub-task numbers on from its parent, so that is the
 * same page).
 */
export function citeMaps(items: Item[]): CiteMaps {
  const steps: { from: number; map: ReadonlyMap<string, Source> }[] = [];
  let map = NO_SOURCES;
  items.forEach((it, i) => {
    const sources = citable(it) ? it.result?.sources : undefined;
    if (!sources?.length) return;
    const next = new Map(map);
    for (const s of sources) next.set(s.id, s);
    steps.push({ from: i + 1, map: (map = next) });
  });
  return (index) => {
    for (let k = steps.length - 1; k >= 0; k--) if (steps[k]!.from <= index) return steps[k]!.map;
    return NO_SOURCES;
  };
}

const sourceKeyCache = new WeakMap<ReadonlyMap<string, Source>, Provenance>();

/** The link keys of a cite map's sources: links to them are verified. */
export function sourceKeys(cites: ReadonlyMap<string, Source>): Provenance {
  let p = sourceKeyCache.get(cites);
  if (!p) {
    const keys = new Set<string>();
    for (const s of cites.values()) {
      const u = parseUrl(s.url);
      if (u) for (const k of seenKeys(u)) keys.add(k);
    }
    p = { has: (k) => keys.has(k) };
    sourceKeyCache.set(cites, p);
  }
  return p;
}

export interface WebGroup {
  /** Index of the group's first and last item. */
  start: number;
  end: number;
  /** Indexes of its web tool items. */
  members: number[];
  searches: number;
  pages: number;
}

const blank = (it: Item) => it.type === "assistant" && it.text.trim() === "";

/**
 * Runs of two or more web tool items with no assistant text between them,
 * keyed by their first index. They show as one "Researched" row.
 */
export function groupWeb(items: Item[]): Map<number, WebGroup> {
  const groups = new Map<number, WebGroup>();
  let run: number[] = [];
  const close = () => {
    if (run.length >= 2) {
      const tools = run.map((i) => items[i] as ToolItem);
      groups.set(run[0]!, {
        start: run[0]!,
        end: run[run.length - 1]!,
        members: run,
        searches: tools.filter((t) => t.tool === "web_search").length,
        pages: tools.filter((t) => t.tool === "web_fetch").length,
      });
    }
    run = [];
  };
  items.forEach((it, i) => {
    if (isWeb(it)) run.push(i);
    else if (!blank(it)) close();
  });
  close();
  return groups;
}
