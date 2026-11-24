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
import { type ReactNode, useEffect, useId, useMemo, useRef, useState } from "react";
import { call } from "../api";
import { type PBlock, type PCell, type PreviewAnswer, type PreviewTarget, type PRow, type PRun, openPreview, previewUrl, targetKey } from "../lib/docPreview";
import { applyChanges, type Change, editRequest, losesFormatting, mentionOf, paragraphNumber, refusals, textOf, withMention } from "../lib/docPreviewEdit";
import { askDiscard, watch } from "../lib/unsavedGuard";
import { showHidden } from "../md/hidden";
import type { UploadedAttachment } from "../types";
import { LinkedA } from "./Linked";
import "./docPreview.css";


/** What the panel's editing hands down: the pending texts, the field open now, and what to do with them. */
interface Edit {
  /** Nothing can be changed (a copy shown after Save in a chat without a folder). */
  off: boolean;
  pending: Record<string, string>;
  open: string | null;
  problems: Record<string, string>;
  /** The place whose field was just closed with a key: its button takes the focus back. */
  back: { current: string | null };
  begin: (at: string) => void;
  keep: (at: string, text: string, original: string) => void;
  drop: () => void;
}

/** The field a place becomes: Enter keeps (Shift+Enter in a paragraph is a line break), Escape drops, leaving keeps. */
function Field({ at, label, area, start, original, edit }: { at: string; label: string; area: boolean; start: string; original: string; edit: Edit }) {
  const [v, setV] = useState(start);
  const ref = useRef<HTMLTextAreaElement & HTMLInputElement>(null);
  // Escape drops: the field going away must not count as leaving it.
  const dropped = useRef(false);
  useEffect(() => {
    ref.current?.focus();
  }, []);
  const props = {
    ref,
    "aria-label": label,
    value: v,
    className: "pv-field",
    onChange: (e: { target: { value: string } }) => setV(e.target.value),
    onBlur: () => {
      if (!dropped.current) edit.keep(at, v, original);
    },
    onKeyDown: (e: React.KeyboardEvent) => {
      if (e.key === "Escape") {
        // Not the panel's own Escape (which closes it).
        e.stopPropagation();
        dropped.current = true;
        edit.back.current = at;
        edit.drop();
      } else if (e.key === "Enter" && !(area && e.shiftKey)) {
        e.preventDefault();
        edit.back.current = at;
        edit.keep(at, v, original);
      }
    },
  };
  return area ? <textarea {...props} rows={Math.min(8, Math.max(2, v.split("\n").length + 1))} /> : <input {...props} type="text" />;
}

const refocus = (edit: Edit, at: string) => (el: HTMLButtonElement | null) => {
  if (el && edit.back.current === at) {
    edit.back.current = null;
    el.focus();
  }
};

/** The reason a place was refused, next to it. */
const Problem = ({ edit, at }: { edit: Edit; at: string }) =>
  edit.problems[at] ? (
    <span className="pv-place-error small error-text" role="note">
      {showHidden(edit.problems[at])}
    </span>
  ) : null;

/** A paragraph, heading or list item that can be changed: a button that opens its field, or the field. */
function Place({ at, runs, edit }: { at: string; runs: PRun[]; edit: Edit }) {
  const id = useId();
  const label = `Paragraph ${paragraphNumber(at)}`;
  const was = textOf(runs);
  const now = edit.pending[at];
  if (edit.open === at)
    return (
      <>
        <Field at={at} label={label} area start={now ?? was} original={was} edit={edit} />
        {losesFormatting(runs) && <span className="pv-place-warn small muted">Changing this paragraph keeps the formatting of its first part and drops the rest.</span>}
      </>
    );
  return (
    <>
      <button type="button" className={`pv-place${now !== undefined ? " pv-changed" : ""}`} aria-label={`Edit ${label}`} aria-describedby={id} ref={refocus(edit, at)} onClick={() => edit.begin(at)}>
        <span id={id}>{now !== undefined ? showHidden(now) : <Runs runs={runs} />}</span>
        {now !== undefined && <span className="sr-only"> (changed)</span>}
      </button>
      <Problem edit={edit} at={at} />
    </>
  );
}

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

function Blocks({ blocks, edit }: { blocks: PBlock[]; edit: Edit }) {
  const body = (b: Exclude<PBlock, { k: "t" }>) => (b.at && !edit.off ? <Place at={b.at} runs={b.r} edit={edit} /> : <Runs runs={b.r} />);
  const out: ReactNode[] = [];
  let items: PBlock[] = [];
  const flush = (at: number) => {
    if (!items.length) return;
    out.push(
      <ul key={`l${at}`} className="pv-list">
        {items.map((b, j) => (
          <li key={j}>{b.k === "li" && body(b)}</li>
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
          {body(b)}
        </H>,
      );
    } else if (b.k === "p") {
      out.push(
        <p key={i} className="pv-p">
          {body(b)}
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

/** A cell that can be changed: a button that opens its field, or the field. */
function Cell({ a, c, edit }: { a: string; c: PCell | undefined; edit: Edit }) {
  const label = `Cell ${a}`;
  const was = c?.f ?? c?.v ?? "";
  const now = edit.pending[a];
  if (edit.open === a) return <Field at={a} label={label} area={false} start={now ?? was} original={was} edit={edit} />;
  return (
    <>
      <button type="button" className={`pv-place pv-cell${now !== undefined ? " pv-changed" : ""}`} aria-label={`Edit ${label}`} ref={refocus(edit, a)} onClick={() => edit.begin(a)}>
        {showHidden(now ?? was)}
        {now !== undefined && <span className="sr-only"> (changed)</span>}
      </button>
      <Problem edit={edit} at={a} />
    </>
  );
}

function Grid({ rows, edit }: { rows: PRow[]; edit: Edit }) {
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
                const a = `${colName(k + 1)}${r.n}`;
                // Every cell of the shown grid can be changed (an empty one too),
                // but a cut one or a stand-in.
                const live = !edit.off && !c?.x;
                if (!c) return <td key={k}>{live && <Cell a={a} c={c} edit={edit} />}</td>;
                return c.f !== undefined && edit.pending[c.a] === undefined ? (
                  <td key={k} className="pv-formula" title="A formula, as written; not calculated">
                    <span className="sr-only">Formula </span>
                    {live ? <Cell a={c.a} c={c} edit={edit} /> : showHidden(c.f)}
                  </td>
                ) : (
                  <td key={k}>{live ? <Cell a={c.a} c={c} edit={edit} /> : showHidden(c.v ?? "")}</td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** The box of the message: what the panel may put in it (Conversation hands it over). */
type Box = { current: { setFiles: (f: (c: UploadedAttachment[]) => UploadedAttachment[]) => void; setText: (f: (c: string) => string) => void; focus: () => void } | null };

const plural = (n: number) => `${n} change${n === 1 ? "" : "s"}`;
const noun = (n: string) => `A copy named “${n}” was saved and added to the message.`;

export default function DocPreview(props: { target: PreviewTarget; onClose: () => void; composer?: Box }) {
  const { session, fileId, name } = props.target;
  const key = targetKey(props.target);
  const [data, setData] = useState<PreviewAnswer | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  // The sheet asked for, by its place among the visible ones (a name is the
  // file's text and cannot be sent back), and the tabs the last answer had.
  const [sheet, setSheet] = useState<number | undefined>(undefined);
  const [tabs, setTabs] = useState<string[]>([]);
  const [tries, setTries] = useState(0);
  const head = useRef<HTMLHeadingElement>(null);
  // The changes waiting for Save, by place (a paragraph's tag, a cell's address):
  // for the sheet shown, and kept in a ref too so a click on Save right after a
  // field was left sees the field's text.
  const [pending, setPending] = useState<Record<string, string>>({});
  const mine = useRef<Record<string, string>>({});
  const [open, setOpen] = useState<string | null>(null);
  const [problems, setProblems] = useState<Record<string, string>>({});
  const [fail, setFail] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  // After a save in a chat without a folder: the copy as it reads, and its name
  // (the server's held copy cannot be previewed before the message is sent).
  const [copy, setCopy] = useState<{ name: string; data: PreviewAnswer["preview"] } | null>(null);
  const [saved, setSaved] = useState<string | null>(props.target.note ?? null);

  const setAll = (n: Record<string, string>) => {
    mine.current = n;
    setPending(n);
  };
  const count = Object.keys(pending).length;
  /** Asked once before the changes are thrown away (close, another sheet). */
  const leave = (go: () => void) => {
    const n = Object.keys(mine.current).length;
    const proceed = () => {
      setAll({});
      setOpen(null);
      go();
    };
    if (n === 0) proceed();
    else void askDiscard(n).then((ok) => ok && proceed());
  };

  // Changes waiting: another file, another chat and a closed tab ask first.
  const dirty = count > 0;
  useEffect(() => {
    if (!dirty) return;
    return watch(
      () => Object.keys(mine.current).length,
      () => {
        setAll({});
        setOpen(null);
      },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dirty]);

  useEffect(() => {
    const ac = new AbortController();
    setBusy(true);
    setErr(null);
    // The sheet on screen is never under another sheet's tab or error.
    setData(null);
    call<PreviewAnswer>("GET", previewUrl(props.target, sheet), undefined, ac.signal)
      .then((a) => {
        setData(a);
        if (a.preview.kind === "sheet") setTabs(a.preview.sheets);
        setBusy(false);
      })
      .catch((x: unknown) => {
        if (ac.signal.aborted) return;
        setErr(x instanceof Error ? x.message : String(x));
        setBusy(false);
      });
    return () => ac.abort();
    // The target is the key: a new file remounts the panel (Conversation).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, sheet, tries]);

  // Focus moves into the panel when it opens; Escape inside it closes it.
  useEffect(() => {
    head.current?.focus();
  }, [key]);

  const back = useRef<string | null>(null);
  const edit: Edit = {
    back,
    off: !!copy,
    pending,
    open,
    problems,
    begin: (at) => setOpen(at),
    keep: (at, text, original) => {
      const n = { ...mine.current };
      if (text === original) delete n[at];
      else n[at] = text;
      setAll(n);
      setOpen(null);
    },
    drop: () => setOpen(null),
  };

  const p = copy?.data ?? data?.preview;
  const shownName = copy?.name ?? name;

  const save = async () => {
    const sent = Object.entries(mine.current).map(([at, text]): Change => ({ at, text }));
    if (!sent.length || !data || saving) return;
    setSaving(true);
    setFail(null);
    setProblems({});
    try {
      const { url, body } = editRequest(props.target, data.preview.kind === "sheet" ? data.preview.at : undefined, sent);
      const r = await call<{ file?: UploadedAttachment; saved?: string; name?: string }>("POST", url, body);
      const box = props.composer?.current ?? null;
      if (fileId !== undefined) {
        const f = r.file;
        if (!f || typeof f.attachment_id !== "string" || typeof f.name !== "string") throw new Error("the server's answer was not understood");
        box?.setFiles((c) => (c.some((x) => x.attachment_id === f.attachment_id) ? c : [...c, f]));
        setCopy({ name: f.name, data: applyChanges(data.preview, sent) });
        setSaved(box ? noun(f.name) : `A copy named “${f.name}” was saved.`);
        setAll({});
        // The Save button is gone: keep the focus in the panel.
        head.current?.focus();
      } else {
        const path = r.saved;
        if (typeof path !== "string" || !path) throw new Error("the node's answer was not understood");
        const copyName = typeof r.name === "string" && r.name ? r.name : (path.split("/").pop() ?? path);
        const mention = mentionOf(path);
        if (mention && box) box.setText((c) => withMention(c, mention));
        setAll({});
        // The panel shows the copy the node wrote.
        openPreview({ session, path, name: copyName, note: mention && box ? noun(copyName) : `A copy named “${copyName}” was saved in the folder (${path}); it could not be put in the message as a mention.` });
      }
    } catch (x) {
      const r = refusals(x, sent.map((c) => c.at));
      setProblems(r.places);
      setFail(r.sentence);
    } finally {
      setSaving(false);
    }
  };

  // Something can be changed here (a server that sends no places offers nothing).
  const editable = !copy && (p?.kind === "doc" ? p.blocks.some((b) => b.k !== "t" && b.at) : !!p?.rows.some((r) => r.c.length));
  const hasTable = p?.kind === "doc" && p.blocks.some((b) => b.k === "t" || !b.at);
  const hasCut = p?.kind === "sheet" && p.rows.some((r) => r.c.some((c) => c.x));
  return (
    <aside
      className="doc-preview"
      aria-label={`Preview of ${showHidden(shownName)}`}
      onKeyDown={(e) => {
        if (e.key === "Escape") leave(props.onClose);
      }}
    >
      <div className="pv-head">
        <h2 className="pv-title small" tabIndex={-1} ref={head}>
          {showHidden(shownName)}
        </h2>
        {/* Only the server's own copy has a link; a file of a folder has none here. */}
        {fileId !== undefined && !copy && (
          <LinkedA className="link small" url={`/api/sessions/${encodeURIComponent(session)}/made/${encodeURIComponent(fileId)}`} download={name}>
            Download
          </LinkedA>
        )}
        <button type="button" className="icon small" aria-label="Close the preview" title="Close (Esc)" onClick={() => leave(props.onClose)}>
          <PanelRightClose size={16} aria-hidden />
        </button>
      </div>
      <p className="pv-note small muted">
        Preview: text and structure, not the page layout.{" "}
        {editable ? "Click a paragraph or a cell to change its text; nothing is saved until you press Save a copy, and the original is never changed." : ""}
      </p>
      {saved && (
        <p className="pv-saved small" role="status">
          {showHidden(saved)}
          {copy && " Send the message, then open the copy from the chat to change it again."}
        </p>
      )}
      {count > 0 && (
        <div className="pv-editbar small" role="group" aria-label="Changes">
          <span>{plural(count)}</span>
          <button type="button" className="small" disabled={saving} onClick={() => (setAll({}), setOpen(null), setProblems({}), setFail(null))}>
            Discard
          </button>
          <button type="button" className="small primary" disabled={saving} onClick={() => void save()}>
            Save a copy
          </button>
        </div>
      )}
      {fail && (
        <div className="error-text small pv-fail" role="alert">
          {showHidden(fail)}
        </div>
      )}
      {tabs.length > 1 && (
        <div className="pv-tabs" role="group" aria-label="Sheets">
          {tabs.map((name, i) => (
            <button key={i} type="button" className="small" aria-pressed={i === (sheet ?? 0)} disabled={busy && i !== (sheet ?? 0)} onClick={() => (i === (sheet ?? 0) ? undefined : leave(() => (setCopy(null), setSheet(i))))}>
              {showHidden(name)}
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
        {p?.kind === "doc" && (p.blocks.length ? <Blocks blocks={p.blocks} edit={edit} /> : <p className="muted small">Nothing to show: the document has no text.</p>)}
        {p?.kind === "sheet" && (p.rows.length ? <Grid rows={p.rows} edit={edit} /> : <p className="muted small">Nothing to show: this sheet has no values.</p>)}
        {p?.more && (
          <p className="pv-more small" role="note">
            More is not shown: the preview is cut at a size the page can draw. {fileId !== undefined ? "Download the file to see all of it." : "Open the file in Word or Excel to see all of it."}
          </p>
        )}
        {editable && (hasTable || hasCut) && (
          <p className="muted small" role="note">
            {hasCut ? "Cells that are cut, shared formulas, and cells with characters that cannot be shown here cannot be changed here." : "Tables, paragraphs that are cut, and paragraphs with characters that cannot be shown here cannot be changed here."}
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
