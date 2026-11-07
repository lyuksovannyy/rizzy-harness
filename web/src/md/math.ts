// Math rendering (lazy chunk): Temml builds MathML DOM nodes (no innerHTML)
// with its CSS and font served from 'self'. trust:false refuses \href,
// \class, \style and friends; the limits stop expansion bombs; and a last
// pass removes any link, source or handler attribute that slips through.
import temml from "temml";
import "temml/dist/Temml-Local.css";

export const OPTIONS = {
  throwOnError: false,
  trust: false,
  strict: false,
  maxExpand: 300,
  // [em, pt]: Temml's caps for user-specified sizes (\rule, \kern, …).
  maxSize: [20, 200] as [number, number],
  annotate: true,
};

export function renderMath(tex: string, el: HTMLElement, display: boolean): void {
  temml.render(tex, el, { ...OPTIONS, displayMode: display });
  for (const node of [el, ...el.querySelectorAll("*")]) {
    for (const a of [...node.attributes]) if (unsafeAttr(a.name)) node.removeAttribute(a.name);
  }
}

export function unsafeAttr(name: string): boolean {
  const n = name.toLowerCase();
  return n === "href" || n.endsWith(":href") || n === "src" || n.startsWith("on");
}
