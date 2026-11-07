// Citation labels in model text. `[S4]` names a page that a web tool
// returned (tool_call_finished.sources); Cite.tsx resolves it against the
// sources recorded before the message, so a label can only ever link to a
// URL our web tools wrote, never to one the model wrote.
//
// Each run of adjacent labels ("[S1][S2]", "[S1] [S2]") becomes one
// `cite-ref` element. A `cite-sources` element lists every label in
// first-cited order for the footer; rehypeSourcesLast moves it below the
// footnotes.
import type { Root } from "mdast";
import type { HastNode } from "./hast";

const LABEL = /\[S([1-9]\d{0,3})\]/g;

// Labels here are code, a link's own text, or not text at all.
const SKIP = new Set(["code", "inlineCode", "link", "linkReference", "definition", "math", "inlineMath", "html"]);

interface MdNode {
  type: string;
  value?: string;
  children?: MdNode[];
  data?: Record<string, unknown>;
}

interface Run {
  start: number;
  end: number;
  labels: string[];
}

/** The label runs in a text, adjacent labels (only spaces between) merged. */
export function labelRuns(text: string): Run[] {
  const runs: Run[] = [];
  for (const m of text.matchAll(LABEL)) {
    const start = m.index;
    const end = start + m[0].length;
    const label = `S${m[1]}`;
    const prev = runs[runs.length - 1];
    if (prev && /^[ \t]*$/.test(text.slice(prev.end, start))) {
      prev.end = end;
      prev.labels.push(label);
    } else {
      runs.push({ start, end, labels: [label] });
    }
  }
  return runs;
}

const element = (type: string, hName: string, labels: string[], children: MdNode[]): MdNode => ({
  type,
  children,
  data: { hName, hProperties: { dataLabels: labels.join(" ") } },
});

export default function remarkCite() {
  return (tree: Root) => {
    const cited: string[] = [];
    walk(tree as unknown as MdNode, cited);
    if (cited.length) (tree as unknown as MdNode).children!.push(element("citeSources", "cite-sources", cited, []));
  };
}

function walk(node: MdNode, cited: string[]) {
  const kids = node.children;
  if (!kids) return;
  for (let i = 0; i < kids.length; i++) {
    const child = kids[i]!;
    if (SKIP.has(child.type)) continue;
    // A table's first row is its header, whose cells are sort buttons: a
    // pill there would be a link inside a button. The label stays text.
    if (node.type === "table" && i === 0) continue;
    if (child.type !== "text") {
      walk(child, cited);
      continue;
    }
    const text = child.value ?? "";
    const runs = labelRuns(text);
    if (!runs.length) continue;
    const parts: MdNode[] = [];
    let at = 0;
    for (const r of runs) {
      if (r.start > at) parts.push({ type: "text", value: text.slice(at, r.start) });
      parts.push(element("cite", "cite-ref", r.labels, [{ type: "text", value: text.slice(r.start, r.end) }]));
      for (const l of r.labels) if (!cited.includes(l)) cited.push(l);
      at = r.end;
    }
    if (at < text.length) parts.push({ type: "text", value: text.slice(at) });
    kids.splice(i, 1, ...parts);
    i += parts.length - 1;
  }
}

/** The footer goes last, after the footnotes section remark-rehype adds. */
export function rehypeSourcesLast() {
  return (tree: HastNode) => {
    const kids = tree.children ?? [];
    const i = kids.findIndex((n) => n.tagName === "cite-sources");
    if (i >= 0) kids.push(...kids.splice(i, 1));
  };
}
