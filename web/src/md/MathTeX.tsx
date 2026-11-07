// TeX from remark-math. While the message streams (or when the TeX is too
// long, or rendering throws) it shows as monospace source; once final, the
// lazy math chunk renders MathML into a node React does not manage.
import { useContext, useEffect, useRef, useState } from "react";
import { DoneCtx } from "./context";

const MAX_TEX = 4_000;

export function MathTeX({ tex, display }: { tex: string; display: boolean }) {
  const done = useContext(DoneCtx);
  const out = useRef<HTMLSpanElement>(null);
  const [state, setState] = useState<"source" | "rendered" | "failed">("source");
  const wanted = done && tex.length <= MAX_TEX;
  useEffect(() => {
    if (!wanted) return;
    let live = true;
    import("./math")
      .then((m) => {
        if (!live || !out.current) return;
        m.renderMath(tex, out.current, display);
        setState("rendered");
      })
      .catch(() => {
        if (live) setState("failed");
      });
    return () => {
      live = false;
    };
  }, [tex, display, wanted]);
  const shown = wanted && state === "rendered";
  const source = display ? (
    <pre className="code math-src">
      <code>{tex}</code>
    </pre>
  ) : (
    <code className="math-src">{tex}</code>
  );
  const Tag = display ? "div" : "span";
  return (
    <Tag className={display ? "math math-display" : "math math-inline"}>
      {!shown && source}
      {/* Temml fills this; it has no React children, so React never touches it. */}
      <span ref={out} className="math-out" hidden={!shown} />
    </Tag>
  );
}
