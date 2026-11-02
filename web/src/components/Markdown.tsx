// Deliberately tiny: fenced code blocks, inline code, and paragraphs. Text is
// rendered through React (escaped); no HTML from the model ever reaches the DOM.
import { Fragment } from "react";

function inline(text: string, key: string) {
  const parts = text.split(/(`[^`\n]+`)/g);
  return parts.map((p, i) =>
    p.startsWith("`") && p.endsWith("`") && p.length > 1 ? (
      <code key={`${key}-${i}`}>{p.slice(1, -1)}</code>
    ) : (
      <Fragment key={`${key}-${i}`}>{p}</Fragment>
    ),
  );
}

export function Markdown({ text }: { text: string }) {
  const blocks: React.ReactNode[] = [];
  const re = /```([^\n`]*)\n([\s\S]*?)(```|$)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let i = 0;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) blocks.push(<p key={i++}>{inline(text.slice(last, m.index), `t${i}`)}</p>);
    blocks.push(
      <pre key={i++} className="code" data-lang={m[1] || undefined}>
        <code>{m[2]}</code>
      </pre>,
    );
    last = re.lastIndex;
    if (m[0].length === 0) break;
  }
  if (last < text.length) blocks.push(<p key={i++}>{inline(text.slice(last), `t${i}`)}</p>);
  return <div className="md">{blocks}</div>;
}
