// Math rendering (lazy chunk): Temml builds MathML DOM nodes (no innerHTML)
// with its CSS and font served from 'self'. trust:false refuses \href,
// \class, \style and friends, and the limits stop expansion bombs. Even so,
// Temml emits a link for \ref, an id for \label (which could take over a
// footnote's scroll target) and any character \char names, so a last pass
// keeps only MathML layout attributes and strips hidden characters.
import temml from "temml";
import "temml/dist/Temml-Local.css";
import { stripHidden } from "./hidden";

export const OPTIONS = {
  // A TeX error throws, and MathTeX shows the source instead of Temml's
  // error text.
  throwOnError: true,
  trust: false,
  strict: false,
  maxExpand: 300,
  // [em, pt]: Temml's caps for user-specified sizes (\rule, \kern, …).
  maxSize: [20, 200] as [number, number],
  annotate: true,
};

// The attributes Temml sets for layout (its setAttribute calls in 0.14).
// Anything else, href and id included, is removed.
const KEEP = new Set([
  "accent", "class", "columnalign", "depth", "display", "displaystyle", "encoding", "fence", "form",
  "height", "largeop", "linebreak", "linethickness", "lspace", "mathbackground", "mathcolor", "mathsize",
  "mathvariant", "maxsize", "minsize", "movablelimits", "notation", "rowspacing", "rspace", "scriptlevel",
  "separator", "stretchy", "style", "symmetric", "voffset", "width", "xmlns",
]);

export function keepAttr(name: string): boolean {
  return KEEP.has(name.toLowerCase());
}

/** Render `tex` into `el`. Throws on a TeX error. */
export function renderMath(tex: string, el: HTMLElement, display: boolean): void {
  temml.render(tex, el, { ...OPTIONS, displayMode: display });
  // `el` itself is ours (React's class and hidden); only Temml's output is cleaned.
  for (const child of [...el.childNodes]) clean(child);
}

function clean(node: Node): void {
  if (node.nodeType === Node.TEXT_NODE) {
    const text = node.nodeValue ?? "";
    const visible = stripHidden(text);
    if (visible !== text) node.nodeValue = visible;
    return;
  }
  if (node.nodeType !== Node.ELEMENT_NODE) {
    node.parentNode?.removeChild(node);
    return;
  }
  const el = node as Element;
  for (const a of [...el.attributes]) if (!keepAttr(a.name)) el.removeAttribute(a.name);
  for (const child of [...el.childNodes]) clean(child);
}
