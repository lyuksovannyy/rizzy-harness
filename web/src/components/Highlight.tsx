import { shownName } from "../md/hiddenChars";
/** Render a search snippet: terms between \u0002 and \u0003 are marked.
 * Hidden characters in titles and snippets show as codes (W-3). */
export function Highlight({ text }: { text: string }) {
  const parts = text.split(/(\u0002[^\u0003]*\u0003)/g);
  return (
    <>
      {parts.map((p, i) =>
        p.startsWith("\u0002") ? <mark key={i}>{shownName(p.slice(1, -1))}</mark> : <span key={i}>{shownName(p)}</span>,
      )}
    </>
  );
}
