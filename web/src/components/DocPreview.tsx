// A Word or Excel file beside the conversation (PLAN.md T99), the way Claude
// Desktop opens a file in its side panel. The server read the file in its
// confined helper and sent text and structure: headings, paragraphs, list
// items, tables, or one sheet's cells. Nothing here is the page layout.
//
// Everything is text the file holds, drawn as React text nodes: no HTML, no
// image, no link made from the file (a formula that is a link is a formula;
// the only link on the panel is the download). Hidden and direction
// characters are shown as codes. Its own lazy chunk, loaded on the first
// "Preview".
import { PanelRightClose } from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { call } from "../api";
import type { PBlock, PCell, PreviewAnswer, PreviewTarget, PRow, PRun } from "../lib/docPreview";
import { showHidden } from "../md/hidden";
import { LinkedA } from "./Linked";
import "./docPreview.css";


function Runs({ runs }: { runs: PRun[] }) {
  return (
    <>
      {runs.map((r, i) => {
        let n: ReactNode = showHidden(r.t);
        if (r.s) n = <s>{n}</s>;
        if (r.i) n = <em>{n}</em>;
        if (r.b) n = <strong>{n}</strong>;
        return <span key={i}>{n}</span>;
      })}
    </>
  );
}

const level = (l: number) => Math.min(6, Math.max(1, Math.trunc(l) || 1));

function Blocks({ blocks }: { blocks: PBlock[] }) {
  const out: ReactNode[] = [];
  let items: PBlock[] = [];
  const flush = (at: number) => {
    if (!items.length) return;
    out.push(
      <ul key={`l${at}`} className="pv-list">
        {items.map((b, j) => (
          <li key={j}>{b.k === "li" && <Runs runs={b.r} />}</li>
        ))}
      </ul>,
    );
    items = [];
  };
  blocks.forEach((b, i) => {
    if (b.k === "li") {
      items.push(b);
      return;
    }
    flush(i);
    if (b.k === "h") {
      // Below the panel's own title (h2): level 1 is h3, and so on to h6.
      const H = `h${Math.min(6, level(b.l) + 2)}` as "h3" | "h4" | "h5" | "h6";
      out.push(
        <H key={i} className={`pv-h pv-h${level(b.l)}`}>
          <Runs runs={b.r} />
        </H>,
      );
    } else if (b.k === "p") {
      out.push(
        <p key={i} className="pv-p">
          <Runs runs={b.r} />
        </p>,
      );
    } else if (b.k === "t") {
      out.push(
        <div key={i} className="pv-scroll">
          <table className="pv-table">
            <tbody>
              {b.rows.map((row, r) => (
                <tr key={r}>
                  {row.map((c, k) => (
                    <td key={k}>{showHidden(c)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
    }
  });
  flush(blocks.length);
  return <>{out}</>;
}

/** `B` → 2, `AA` → 27; the address of a cell is `B7`. */
function colOf(a: string): number {
  const m = /^([A-Z]{1,3})\d+$/.exec(a);
  if (!m) return 0;
  return [...(m[1] ?? "")].reduce((n, c) => n * 26 + c.charCodeAt(0) - 64, 0);
}
function colName(n: number): string {
  let s = "";
  for (let x = n; x > 0; x = Math.floor((x - 1) / 26)) s = String.fromCharCode(65 + ((x - 1) % 26)) + s;
  return s;
}

function Grid({ rows }: { rows: PRow[] }) {
  const { cols, cells } = useMemo(() => {
    const cells = rows.map((r) => {
      const m = new Map<number, PCell>();
      for (const c of r.c) {
        const k = colOf(c.a);
        if (k > 0) m.set(k, c);
      }
      return m;
    });
    const cols = Math.max(1, ...cells.flatMap((m) => [...m.keys()]));
    return { cols, cells };
  }, [rows]);
  return (
    <div className="pv-scroll">
      <table className="pv-grid">
        <thead>
          <tr>
            <th scope="col" className="pv-corner">
              <span className="sr-only">Row</span>
            </th>
            {Array.from({ length: cols }, (_, i) => (
              <th key={i} scope="col">
                {colName(i + 1)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={r.n}>
              <th scope="row">{r.n}</th>
              {Array.from({ length: cols }, (_, k) => {
                const c = cells[i]?.get(k + 1);
                if (!c) return <td key={k} />;
                return c.f !== undefined ? (
                  <td key={k} className="pv-formula" title="A formula, as written; not calculated">
                    <span className="sr-only">Formula </span>
                    {showHidden(c.f)}
                  </td>
                ) : (
                  <td key={k}>{showHidden(c.v ?? "")}</td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export default function DocPreview(props: { target: PreviewTarget; onClose: () => void }) {
  const { session, fileId, name } = props.target;
  const [data, setData] = useState<PreviewAnswer | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [sheet, setSheet] = useState<string | undefined>(undefined);
  const [tries, setTries] = useState(0);
  const head = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    const ac = new AbortController();
    setBusy(true);
    setErr(null);
    const q = sheet === undefined ? "" : `?sheet=${encodeURIComponent(sheet)}`;
    call<PreviewAnswer>("GET", `/api/sessions/${encodeURIComponent(session)}/files/${encodeURIComponent(fileId)}/preview${q}`, undefined, ac.signal)
      .then((a) => {
        setData(a);
        setBusy(false);
      })
      .catch((x: unknown) => {
        if (ac.signal.aborted) return;
        setErr(x instanceof Error ? x.message : String(x));
        setBusy(false);
      });
    return () => ac.abort();
  }, [session, fileId, sheet, tries]);

  // Focus moves into the panel when it opens; Escape inside it closes it.
  useEffect(() => {
    head.current?.focus();
  }, [fileId]);

  const p = data?.preview;
  return (
    <aside
      className="doc-preview"
      aria-label={`Preview of ${showHidden(name)}`}
      onKeyDown={(e) => {
        if (e.key === "Escape") props.onClose();
      }}
    >
      <div className="pv-head">
        <h2 className="pv-title small" tabIndex={-1} ref={head}>
          {showHidden(name)}
        </h2>
        <LinkedA className="link small" url={`/api/sessions/${encodeURIComponent(session)}/made/${encodeURIComponent(fileId)}`} download={name}>
          Download
        </LinkedA>
        <button type="button" className="icon small" aria-label="Close the preview" title="Close (Esc)" onClick={props.onClose}>
          <PanelRightClose size={16} aria-hidden />
        </button>
      </div>
      <p className="pv-note small muted">Preview: text and structure, not the page layout</p>
      {p?.kind === "sheet" && p.sheets.length > 1 && (
        <div className="pv-tabs" role="group" aria-label="Sheets">
          {p.sheets.map((s) => (
            <button key={s} type="button" className="small" aria-pressed={s === p.sheet} disabled={busy && s !== p.sheet} onClick={() => setSheet(s)}>
              {showHidden(s)}
            </button>
          ))}
        </div>
      )}
      <div className="pv-body">
        {busy && (
          <p className="muted small" role="status">
            Reading the file…
          </p>
        )}
        {err && (
          <div className="error-text small" role="alert">
            {showHidden(err)}{" "}
            <button type="button" className="link" onClick={() => setTries((n) => n + 1)}>
              Try again
            </button>
          </div>
        )}
        {p?.kind === "doc" && (p.blocks.length ? <Blocks blocks={p.blocks} /> : <p className="muted small">Nothing to show: the document has no text.</p>)}
        {p?.kind === "sheet" && (p.rows.length ? <Grid rows={p.rows} /> : <p className="muted small">Nothing to show: this sheet has no values.</p>)}
        {p?.more && (
          <p className="pv-more small" role="note">
            More is not shown: the preview is cut at a size the page can draw. Download the file to see all of it.
          </p>
        )}
        {p?.notes.map((n, i) => (
          <p key={i} className="muted small" role="note">
            {showHidden(n)}
          </p>
        ))}
      </div>
    </aside>
  );
}
