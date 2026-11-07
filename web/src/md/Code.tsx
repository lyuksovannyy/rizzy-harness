// Code: fenced blocks get a header (language, Copy), long ones collapse, and
// once the message is final known languages are highlighted in a lazy chunk.
// Math arrives as code too (remark-math) and is routed to MathTeX here.
import { useContext, useEffect, useState, type ComponentProps } from "react";
import type { ExtraProps } from "react-markdown";
import { DoneCtx } from "./context";
import { useCopy } from "./copy";
import { classList, hastText, reactText, renderHast, type HastNode } from "./hast";
import { grammarFor, type Grammar } from "./langs";
import { MathTeX } from "./MathTeX";
import { rt } from "./strings";

const COLLAPSE_LINES = 60;
const MAX_HIGHLIGHT_CHARS = 50_000;
const MAX_HIGHLIGHT_LINES = 1_500;

type PreProps = ComponentProps<"pre"> & ExtraProps;

/** `pre` override: reads the code straight from hast and ignores the
 * rendered children, so block code never goes through the inline path. */
export function Pre({ node, children }: PreProps) {
  const code = (node as HastNode | undefined)?.children?.find((c) => c.type === "element" && c.tagName === "code");
  if (!code) return <pre className="code">{children}</pre>;
  const cls = classList(code);
  // hast code text ends with the newline that closed the fence.
  const text = hastText(code).replace(/\n$/, "");
  if (cls.includes("math-display")) return <MathTeX tex={text} display />;
  const lang = cls.find((c) => c.startsWith("language-"))?.slice("language-".length) ?? "";
  return <CodeBlock code={text} lang={lang} />;
}

type CodeProps = ComponentProps<"code"> & ExtraProps;

/** Inline code; inline math (`code.math-inline`) goes to MathTeX. */
export function InlineCode({ className, children }: CodeProps) {
  if (className?.split(" ").includes("math-inline")) return <MathTeX tex={reactText(children)} display={false} />;
  return <code className={className}>{children}</code>;
}

export function CodeBlock({ code, lang }: { code: string; lang: string }) {
  const done = useContext(DoneCtx);
  const [expanded, setExpanded] = useState(false);
  const [copied, copy] = useCopy();
  const lines = code.split("\n").length;
  const mermaid = lang.toLowerCase() === "mermaid";
  const grammar = mermaid ? null : grammarFor(lang);
  const wanted = done && grammar !== null && code.length <= MAX_HIGHLIGHT_CHARS && lines <= MAX_HIGHLIGHT_LINES;
  const tree = useHighlight(code, wanted ? grammar : null);
  const long = lines > COLLAPSE_LINES;
  return (
    <div className={`codeblock${long && !expanded ? " collapsed" : ""}`}>
      <div className="code-head">
        <span className="lang">{mermaid ? rt("diagram") : lang.slice(0, 32)}</span>
        <button type="button" className="copy" onClick={() => copy(code)}>
          {copied ? rt("copied") : rt("copy")}
        </button>
      </div>
      <pre className="code">
        <code className={grammar ? `language-${grammar}` : undefined}>{tree ? renderHast(tree) : code}</code>
      </pre>
      {long && (
        <button type="button" className="link small code-more" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
          {expanded ? rt("showLess") : rt("showAll", { n: String(lines) })}
        </button>
      )}
    </div>
  );
}

/** Highlighted hast for `code`, or null until (and unless) it is ready. */
function useHighlight(code: string, grammar: Grammar | null): HastNode | null {
  const [result, setResult] = useState<{ code: string; grammar: Grammar; tree: HastNode } | null>(null);
  useEffect(() => {
    if (!grammar) return;
    let live = true;
    import("./highlight")
      .then((m) => m.highlight(code, grammar))
      .then((tree) => {
        if (live) setResult({ code, grammar, tree: tree as HastNode });
      })
      .catch(() => {}); // plain text is a fine fallback
    return () => {
      live = false;
    };
  }, [code, grammar]);
  return result && result.code === code && result.grammar === grammar ? result.tree : null;
}
