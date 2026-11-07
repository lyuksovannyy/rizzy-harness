// Small helpers over hast (the HTML syntax tree react-markdown and the
// highlighter produce). Typed structurally so nothing depends on @types/hast.
import type { ReactNode } from "react";

export interface HastNode {
  type: string;
  tagName?: string;
  value?: unknown;
  properties?: Record<string, unknown>;
  children?: HastNode[];
}

/** Concatenated text of a node and its descendants. */
export function hastText(node: HastNode | undefined): string {
  if (!node) return "";
  if (node.type === "text") return typeof node.value === "string" ? node.value : "";
  return (node.children ?? []).map(hastText).join("");
}

/** The class names of an element, as a list. */
export function classList(node: HastNode | undefined): string[] {
  const c = node?.properties?.className;
  if (Array.isArray(c)) return c.filter((x): x is string => typeof x === "string");
  return typeof c === "string" ? c.split(/\s+/) : [];
}

/**
 * Render highlighter output as `<span className>` and text only. Every other
 * element (with its content) and every other attribute is dropped, so a
 * grammar bug or a hostile code block can never produce markup.
 */
export function renderHast(node: HastNode, key?: number): ReactNode {
  switch (node.type) {
    case "text":
      return typeof node.value === "string" ? node.value : null;
    case "root":
      return (node.children ?? []).map((c, i) => renderHast(c, i));
    case "element": {
      if (node.tagName !== "span") return null;
      const cls = classList(node).filter((c) => /^[\w-]+$/.test(c)).join(" ");
      return (
        <span key={key} className={cls || undefined}>
          {(node.children ?? []).map((c, i) => renderHast(c, i))}
        </span>
      );
    }
    default:
      return null;
  }
}

/** Text of rendered React children (link labels, inline math source). */
export function reactText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(reactText).join("");
  if (node && typeof node === "object" && "props" in node) {
    return reactText((node as { props: { children?: ReactNode } }).props.children);
  }
  return "";
}
