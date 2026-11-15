// What a turn changed (X.12), beside the conversation on the right, the way
// Claude Desktop opens a turn's diff from its Changes card: the turn's
// "N files changed" row is the opener, the review happens beside the chat.
// Its own lazy chunk, loaded on the first "Show changes".
//
// Everything here is text the agent's files hold: nothing is rendered as
// HTML and nothing is a link. In names and section titles, hidden and
// unusual characters are marked the way the approval card marks them
// (lib/callText); in code lines only what cannot be seen is marked (control,
// format and bidi characters, odd blanks), never indentation, and a file
// whose lines all end in CRLF says so once instead of on every line. An
// added or removed line says so by its "+" or "−" sign and, for screen
// readers, in words, not by its colour alone.
//
// What is drawn is bounded (review D3): a file's lines exist in the page only
// while it is open, the first OPEN_FILES files start open, and each file
// draws LINES_SHOWN lines until "Show more" asks for the rest.
import { PanelRightClose } from "lucide-react";
import { useEffect, useRef, useState, type JSX } from "react";
import { pageApi } from "../apiPages";
import { bytes } from "../lib/bytes";
import { marks, type Seg } from "../lib/callText";
import { codeOf, SHOWN_SRC } from "../md/hiddenChars";
import { showHidden } from "../md/hidden";
import type { DiffFile, DiffHunk, TurnDiff } from "../types";
import "./diffPanel.css";

/** Files whose lines show at first; the rest open on a click. */
export const OPEN_FILES = 30;
/** Lines a file draws at first, and how many more each "Show more" adds. */
export const LINES_SHOWN = 400;
const LINES_MORE = 2000;

const STATUS: Record<DiffFile["status"], { letter: string; word: string }> = {
  added: { letter: "A", word: "added" },
  deleted: { letter: "D", word: "deleted" },
  modified: { letter: "M", word: "modified" },
  renamed: { letter: "R", word: "renamed" },
  type_changed: { letter: "T", word: "type changed" },
};

/** A name or title as it is, hidden characters as codes (never HTML). */
export function Marked({ text }: { text: string }) {
  return <>{marks(text, "text").map((s: Seg, i) => (s.mark ? <span key={i} className="hidden-mark">{s.t}</span> : s.t))}</>;
}

const SHOWN = new RegExp(SHOWN_SRC, "gu");

/** A line of code: only invisible characters become codes; spaces, tabs
 * and indentation stay as they are (review D6). */
function Code({ text }: { text: string }) {
  const out: (string | JSX.Element)[] = [];
  let last = 0;
  for (const m of text.matchAll(SHOWN)) {
    if (m.index > last) out.push(text.slice(last, m.index));
    out.push(
      <span key={m.index} className="hidden-mark">
        {codeOf(m[0])}
      </span>,
    );
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return <>{out}</>;
}

/** Every line of the file's hunks ends in CR (both sides use CRLF). */
function crlfFile(f: DiffFile): boolean {
  let n = 0;
  for (const h of f.hunks ?? [])
    for (const l of h.lines) {
      if (l[0] === "\\") continue;
      if (!l.endsWith("\r")) return false;
      n++;
    }
  return n > 0;
}

const sizes = (f: DiffFile) => {
  const a = f.old_size === undefined ? null : bytes(f.old_size);
  const b = f.new_size === undefined ? null : bytes(f.new_size);
  return a && b ? `${a} → ${b}` : (a ?? b ?? "");
};

/** Why a file has no lines here, in words. */
function noLines(f: DiffFile): string | null {
  switch (f.content) {
    case "binary":
      return `Binary file (${sizes(f)}): not shown.`;
    case "too_large":
      return `Too large to compare line by line (${sizes(f)}).`;
    case "omitted":
      return "Not shown: this answer's size was used by the files above it.";
    case "text":
      if (f.hunks?.length) return null;
      if (f.status === "renamed") return "Moved, content unchanged.";
      if (f.old_mode !== f.new_mode) return `Mode changed: ${f.old_mode ?? "none"} → ${f.new_mode ?? "none"}.`;
      return f.status === "added" || f.status === "deleted" ? "Empty file." : "No line changes.";
  }
}

/** One hunk, at most `max` of its lines. */
function Hunk({ h, max, crlf }: { h: DiffHunk; max: number; crlf: boolean }) {
  let oldN = h.old_start;
  let newN = h.new_start;
  return (
    <div className="diff-hunk" role="table" aria-label={`Lines ${h.old_start} to ${h.old_start + Math.max(h.old_lines - 1, 0)} before, ${h.new_start} to ${h.new_start + Math.max(h.new_lines - 1, 0)} after`}>
      <div className="diff-hunk-head" role="row">
        <span role="cell">
          @@ −{h.old_start},{h.old_lines} +{h.new_start},{h.new_lines} @@{h.section ? " " : ""}
          {h.section && <Marked text={h.section} />}
        </span>
      </div>
      {h.lines.slice(0, max).map((line, i) => {
        const mark = line[0];
        let text = line.slice(1);
        if (mark === "\\")
          return (
            <div key={i} className="diff-line diff-note" role="row">
              <span role="cell" className="diff-n" />
              <span role="cell" className="diff-n" />
              <span role="cell" className="diff-sign" aria-hidden />
              <span role="cell" className="diff-text">
                ⟨no newline at end of file⟩
              </span>
            </div>
          );
        if (crlf && text.endsWith("\r")) text = text.slice(0, -1);
        const kind = mark === "+" ? "add" : mark === "-" ? "del" : "ctx";
        const o = kind === "add" ? "" : String(oldN++);
        const n = kind === "del" ? "" : String(newN++);
        return (
          <div key={i} className={`diff-line diff-${kind}`} role="row">
            <span role="cell" className="diff-n" aria-hidden>
              {o}
            </span>
            <span role="cell" className="diff-n" aria-hidden>
              {n}
            </span>
            <span role="cell" className="diff-sign" aria-hidden>
              {kind === "add" ? "+" : kind === "del" ? "−" : " "}
            </span>
            <span role="cell" className="diff-text">
              <span className="sr-only">{kind === "add" ? `added, line ${n}: ` : kind === "del" ? `removed, line ${o}: ` : `line ${n}: `}</span>
              <Code text={text} />
            </span>
          </div>
        );
      })}
    </div>
  );
}

/** A file's lines, at most `limit` of them. */
function FileLines({ f, limit }: { f: DiffFile; limit: number }) {
  const crlf = crlfFile(f);
  let left = limit;
  const parts: JSX.Element[] = [];
  for (const [i, h] of (f.hunks ?? []).entries()) {
    if (left <= 0) break;
    parts.push(<Hunk key={i} h={h} max={left} crlf={crlf} />);
    left -= h.lines.length;
  }
  return (
    <>
      {crlf && <p className="diff-why muted small">CRLF line endings (shown without the CR).</p>}
      {parts}
    </>
  );
}

function FileBlock({ f, index }: { f: DiffFile; index: number }) {
  const s = STATUS[f.status];
  const why = noLines(f);
  const [open, setOpen] = useState(index < OPEN_FILES);
  const [limit, setLimit] = useState(LINES_SHOWN);
  const total = (f.hunks ?? []).reduce((a, h) => a + h.lines.length, 0);
  return (
    <details className="diff-file" id={`diff-f${index}`} open={open} onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary className="diff-file-head">
        <span className={`diff-badge diff-st-${f.status}`} title={s.word}>
          {s.letter}
          <span className="sr-only"> {s.word}</span>
        </span>
        <span className="diff-path mono">
          {f.old_path !== undefined && (
            <>
              <Marked text={f.old_path} /> →{" "}
            </>
          )}
          <Marked text={f.path} />
          {(f.new_mode ?? f.old_mode) === "120000" && <span className="muted"> (link)</span>}
        </span>
        <span className="diff-counts">
          {f.added > 0 && <span className="diff-plus">+{f.added}</span>} {f.removed > 0 && <span className="diff-minus">−{f.removed}</span>}
        </span>
      </summary>
      {/* Mounted only while open (review D3). */}
      {open && (
        <>
          {why && <p className="diff-why muted small">{why}</p>}
          <FileLines f={f} limit={limit} />
          {total > limit && (
            <button type="button" className="link diff-more small" onClick={() => setLimit((n) => n + LINES_MORE)}>
              Show {Math.min(LINES_MORE, total - limit).toLocaleString("en-US")} more lines
            </button>
          )}
          {f.cut && f.content === "text" && (
            <p className="diff-cut small" role="note">
              ⟨Cut: this file has more changes, or longer lines, than are shown here.⟩
            </p>
          )}
        </>
      )}
    </details>
  );
}

export default function DiffPanel(props: {
  sessionId: string;
  turnId: string;
  /** Show this turn and every later one (what a rewind undoes). */
  rewind: boolean;
  /** Later turns changed files too: offer the wider view. */
  laterChanges: boolean;
  /** The turn's changes were undone already, or the viewer may not undo here. */
  canUndo: boolean;
  running: boolean;
  onRewind: (rewind: boolean) => void;
  onClose: () => void;
}) {
  const { sessionId, turnId, rewind } = props;
  const [diff, setDiff] = useState<TurnDiff | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [tries, setTries] = useState(0);
  const [undo, setUndo] = useState<{ busy: boolean; err: string | null; done: boolean }>({ busy: false, err: null, done: false });
  const head = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    const ac = new AbortController();
    setDiff(null);
    setErr(null);
    pageApi
      .turnDiff(sessionId, turnId, rewind, ac.signal)
      .then(setDiff)
      .catch((x: unknown) => {
        if (!ac.signal.aborted) setErr(x instanceof Error ? x.message : String(x));
      });
    return () => ac.abort();
  }, [sessionId, turnId, rewind, tries]);

  // Focus moves into the panel when it opens; Escape inside it closes it.
  useEffect(() => {
    head.current?.focus();
  }, [turnId]);

  const runUndo = async () => {
    setUndo({ busy: true, err: null, done: false });
    try {
      await pageApi.restoreTurn(sessionId, turnId, false, rewind);
      setUndo({ busy: false, err: null, done: true });
    } catch (x) {
      setUndo({ busy: false, err: x instanceof Error ? x.message : String(x), done: false });
    }
  };

  const files = diff?.files ?? [];
  const added = files.reduce((a, f) => a + f.added, 0);
  const removed = files.reduce((a, f) => a + f.removed, 0);
  // Counted as the turn's row counts them (review D5): a rename is two
  // paths there, the old and the new.
  const renamed = files.filter((f) => f.old_path !== undefined).length;
  const count = files.length + renamed + (diff?.more_files ?? 0);

  return (
    <aside
      className="diff-panel"
      aria-label={rewind ? "Changes of this turn and the later ones" : "Changes of this turn"}
      onKeyDown={(e) => {
        if (e.key === "Escape") props.onClose();
      }}
    >
      <div className="diff-head">
        <h2 className="diff-title small" tabIndex={-1} ref={head}>
          {rewind ? "Changes since before this turn" : "Changes in this turn"}
          {diff && (
            <span className="muted">
              {" "}
              · {count} file{count === 1 ? "" : "s"}
              {renamed > 0 ? ` (${renamed} renamed)` : ""} {added > 0 && <span className="diff-plus">+{added}</span>} {removed > 0 && <span className="diff-minus">−{removed}</span>}
            </span>
          )}
        </h2>
        <button type="button" className="icon small" aria-label="Close the changes" title="Close (Esc)" onClick={props.onClose}>
          <PanelRightClose size={16} aria-hidden />
        </button>
      </div>
      <div className="diff-tools small">
        {props.laterChanges && (
          <label>
            <input type="checkbox" checked={rewind} onChange={(e) => props.onRewind(e.target.checked)} /> Include later turns
          </label>
        )}
        {props.canUndo && !undo.done && (
          <button type="button" className="link" disabled={undo.busy || props.running} onClick={() => void runUndo()}>
            {rewind ? "Rewind files to before this turn" : "Undo file changes"}
          </button>
        )}
        {undo.done && <span role="status">{rewind ? "Rewound." : "Undone."}</span>}
        {undo.err && <span className="error-text">{showHidden(undo.err)}</span>}
      </div>
      <div className="diff-body">
        {!diff && !err && (
          <p className="muted small" role="status">
            Reading the changes…
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
        {diff && files.length === 0 && !diff.more_files && <p className="muted small">No file changes.</p>}
        {diff && (diff.cut || !!diff.more_files) && (
          <p className="diff-cut small" role="note">
            ⟨Cut: not every change is shown here
            {diff.more_files ? `; ${diff.more_files} more changed file${diff.more_files === 1 ? " is" : "s are"} not listed` : ""}.⟩
          </p>
        )}
        {!!diff?.nested?.length && (
          <p className="muted small">
            Git repositories inside the folder are not compared: {diff.nested.map((n) => showHidden(n)).join(", ")}.
          </p>
        )}
        {files.length > OPEN_FILES && (
          <p className="muted small">The first {OPEN_FILES} files are open; the others open on a click.</p>
        )}
        {files.length > 1 && (
          <nav aria-label="Changed files">
            <ul className="diff-list">
              {files.map((f, i) => (
                <li key={i}>
                  <button
                    type="button"
                    className="link diff-jump"
                    onClick={() => {
                      const el = document.getElementById(`diff-f${i}`) as HTMLDetailsElement | null;
                      if (el) {
                        el.open = true;
                        el.scrollIntoView({ block: "start" });
                        el.querySelector("summary")?.focus();
                      }
                    }}
                  >
                    <span className={`diff-badge diff-st-${f.status}`} aria-hidden>
                      {STATUS[f.status].letter}
                    </span>{" "}
                    <span className="mono">
                      <Marked text={f.path} />
                    </span>
                    <span className="sr-only"> ({STATUS[f.status].word})</span>
                  </button>
                </li>
              ))}
            </ul>
          </nav>
        )}
        {files.map((f, i) => (
          <FileBlock key={i} f={f} index={i} />
        ))}
      </div>
    </aside>
  );
}
