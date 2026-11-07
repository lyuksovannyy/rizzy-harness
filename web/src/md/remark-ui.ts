// `ui:` fences (rich_plan §6.1). A code node becomes a `ui-block` element
// only where Rust's scanner (crates/core/src/ui/scan.rs) makes a block: at
// the root, at column 1, with an opener line that is the fence run and
// exactly `ui:<type>`. The line itself is checked, not `lang`, because
// micromark has decoded escapes and character references in `lang` and
// Rust has not. Each marked node carries its type, its ordinal among the
// marked nodes and the FNV hash of its body; UiHost shows the stored block
// only when all three match, and plain code otherwise. A fence the text
// never closes (the reply ran out of tokens mid-block) is marked open:
// Rust made no block of it, and its body is half a JSON object.
import type { Root, RootContent } from "mdast";
import type { UiKind } from "../types";
import { fnv1a } from "./fnv";

const OPENER = /^(`{3,}|~{3,})[ \t]*ui:(choices|form|progress|chart)[ \t]*$/;
const CLOSER = /^ {0,3}(`{3,}|~{3,})[ \t]*$/;

/** The block type an opener line opens, if it is a ui opener. */
export function uiOpener(line: string): UiKind | null {
  return (OPENER.exec(line)?.[2] as UiKind | undefined) ?? null;
}

/** Whether the fence opened on line `start` (1-based) is closed on line
 * `end`, where micromark ends its node. An unclosed fence runs to the end
 * of the text, so its last line is body, not a closer (CommonMark: the
 * same character, a run at least as long, then only spaces). */
function closed(lines: readonly string[], start: number, end: number): boolean {
  const run = OPENER.exec(lines[start - 1] ?? "")?.[1] ?? "";
  const closer = CLOSER.exec(lines[end - 1] ?? "")?.[1];
  return end > start && !!closer && closer[0] === run[0] && closer.length >= run.length;
}

export default function remarkUi() {
  return (tree: Root, file: { value?: unknown }) => {
    const lines = String(file.value ?? "").split("\n");
    let ordinal = 0;
    tree.children.forEach((node, i) => {
      if (node.type !== "code" || node.meta || node.position?.start.column !== 1) return;
      const { start, end } = node.position;
      const type = uiOpener(lines[start.line - 1] ?? "");
      if (!type || node.lang !== `ui:${type}`) return;
      const open = !closed(lines, start.line, end.line);
      const block = {
        type: "uiBlock",
        value: node.value,
        data: {
          hName: "ui-block",
          hProperties: {
            dataType: type,
            dataOrdinal: ordinal++,
            dataHash: fnv1a(node.value),
            ...(open && { dataOpen: "" }),
          },
          hChildren: [{ type: "text", value: node.value }],
        },
      };
      tree.children[i] = block as unknown as RootContent;
    });
  };
}
