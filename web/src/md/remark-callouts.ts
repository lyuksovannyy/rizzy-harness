// GitHub-style callouts: a blockquote whose first line is [!NOTE], [!TIP],
// [!IMPORTANT], [!WARNING] or [!CAUTION] becomes an `aside` with a tone. The
// rest of that line, if it is plain text, is the title.
import type { Blockquote, Root } from "mdast";

export const TONES = ["note", "tip", "important", "warning", "caution"] as const;
export type Tone = (typeof TONES)[number];

const MARKER = /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\][ \t]*/i;

interface Parent {
  type: string;
  children?: Parent[];
}

export default function remarkCallouts() {
  return (tree: Root) => walk(tree as Parent);
}

function walk(node: Parent) {
  for (const child of node.children ?? []) {
    if (child.type === "blockquote") callout(child as unknown as Blockquote);
    walk(child);
  }
}

function callout(q: Blockquote) {
  const p = q.children[0];
  if (p?.type !== "paragraph") return;
  const first = p.children[0];
  if (first?.type !== "text") return;
  const m = MARKER.exec(first.value);
  if (!m) return;
  const tone = m[1]!.toLowerCase() as Tone;
  const rest = first.value.slice(m[0].length);
  const nl = rest.indexOf("\n");
  let title = "";
  if (nl >= 0) {
    title = rest.slice(0, nl).trim();
    first.value = rest.slice(nl + 1);
  } else if (p.children.length === 1 || p.children[1]?.type === "break") {
    // The whole first line, ended by the paragraph or a hard break
    // (`Title  ` or `Title\`); the break goes below.
    title = rest.trim();
    first.value = "";
  } else {
    // The first line goes on with formatting: keep all of it as body text.
    first.value = rest;
  }
  if (first.value === "") p.children.shift();
  if (p.children[0]?.type === "break") p.children.shift();
  if (p.children.length === 0) q.children.shift();
  (q as { data?: Record<string, unknown> }).data = {
    hName: "aside",
    hProperties: { className: ["callout", `callout-${tone}`], role: "note", dataTone: tone, dataTitle: title || undefined },
  };
}
