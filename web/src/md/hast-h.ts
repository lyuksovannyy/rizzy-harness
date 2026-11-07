// The highlighter worker's stand-in for hastscript's `h` (vite.config.ts
// swaps it in there). refractor calls `h` only to wrap each token:
// h("span.token.<type>.<alias>", attributes, content), with content one node
// or a list of nodes. hastscript's general version needs property-information,
// which the chat chunk also bundles for react-markdown; this does the one
// thing refractor asks of it. hast-h.test.ts holds it to hastscript.
import type { HastNode } from "./hast";

export function h(selector: string, attributes?: Record<string, string> | null, content?: HastNode | HastNode[] | null): HastNode {
  const [tagName = "div", ...className] = selector.split(".");
  return {
    type: "element",
    tagName,
    properties: { ...attributes, ...(className.length ? { className } : {}) },
    children: content == null ? [] : Array.isArray(content) ? content : [content],
  };
}
