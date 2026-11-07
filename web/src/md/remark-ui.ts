// `ui:` fences (rich_plan §6.1). A code node becomes a `ui-block` element
// only where Rust's scanner (crates/core/src/ui/scan.rs) makes a block: at
// the root, at column 1, with an opener line that is the fence run and
// exactly `ui:<type>`. The line itself is checked, not `lang`, because
// micromark has decoded escapes and character references in `lang` and
// Rust has not. Each marked node carries its type, its ordinal among the
// marked nodes and the FNV hash of its body; UiHost shows the stored block
// only when all three match, and plain code otherwise.
import type { Root, RootContent } from "mdast";
import type { UiKind } from "../types";
import { fnv1a } from "./fnv";

const OPENER = /^(?:`{3,}|~{3,})[ \t]*ui:(choices|form|progress|chart)[ \t]*$/;

/** The block type an opener line opens, if it is a ui opener. */
export function uiOpener(line: string): UiKind | null {
  return (OPENER.exec(line)?.[1] as UiKind | undefined) ?? null;
}

export default function remarkUi() {
  return (tree: Root, file: { value?: unknown }) => {
    const lines = String(file.value ?? "").split("\n");
    let ordinal = 0;
    tree.children.forEach((node, i) => {
      if (node.type !== "code" || node.meta || node.position?.start.column !== 1) return;
      const type = uiOpener(lines[node.position.start.line - 1] ?? "");
      if (!type || node.lang !== `ui:${type}`) return;
      const block = {
        type: "uiBlock",
        value: node.value,
        data: {
          hName: "ui-block",
          hProperties: { dataType: type, dataOrdinal: ordinal++, dataHash: fnv1a(node.value) },
          hChildren: [{ type: "text", value: node.value }],
        },
      };
      tree.children[i] = block as unknown as RootContent;
    });
  };
}
