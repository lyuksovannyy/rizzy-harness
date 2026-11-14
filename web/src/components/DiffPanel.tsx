// What a turn changed (X.12), beside the conversation on the right, the way
// Claude Desktop opens a turn's diff from its Changes card: the turn's
// "N files changed" row is the opener, the review happens beside the chat.
// Its own lazy chunk, loaded on the first "Show changes".
//
// Everything here is text the agent's files hold: nothing is rendered as
// HTML, nothing is a link, and hidden, control and unusual blank characters
// are marked the way the approval card marks them (lib/callText). An added
// or removed line says so by its "+" or "−" sign and, for screen readers,
// in words, not by its colour alone.
import { PanelRightClose } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { pageApi } from "../apiPages";
import { bytes } from "../lib/bytes";
import { marks, type Seg } from "../lib/callText";
import { showHidden } from "../md/hidden";
import type { DiffFile, DiffHunk, TurnDiff } from "../types";
import "./diffPanel.css";

/** Files whose lines show at first; the rest open on a click. */
const OPEN_FILES = 30;

const STATUS: Record<DiffFile["status"], { letter: string; word: string }> = {
  added: { letter: "A", word: "added" },
  deleted: { letter: "D", word: "deleted" },
  modified: { letter: "M", word: "modified" },
  renamed: { letter: "R", word: "renamed" },
  type_changed: { letter: "T", word: "type changed" },
};

/** Text as it is, hidden characters as codes (never HTML: React escapes). */
export function Marked({ text }: { text: string }) {
  return <>{marks(text, "text").map((s: Seg, i) => (s.mark ? <span key={i} className="hidden-mark">{s.t}</span> : s.t))}</>;
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

function Hunk({ h }: { h: DiffHunk }) {
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
      {h.lines.map((line, i) => {
        const mark = line[0];
        const text = line.slice(1);
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
              <Marked text={text} />
            </span>
          </div>
        );
      })}
    </div>
  );
}

function FileBlock({ f, index }: { f: DiffFile; index: number }) {
  const s = STATUS[f.status];
  const why = noLines(f);
  return (
    <details className="diff-file" id={`diff-f${index}`} open={index < OPEN_FILES}>
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
      {why && <p className="diff-why muted small">{why}</p>}
      {f.hunks?.map((h, i) => <Hunk key={i} h={h} />)}
      {f.cut && f.content === "text" && (
        <p className="diff-cut small" role="note">
          ⟨Cut: this file has more changes, or longer lines, than are shown here.⟩
        </p>
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
  const count = files.length + (diff?.more_files ?? 0);

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
              · {count} file{count === 1 ? "" : "s"} {added > 0 && <span className="diff-plus">+{added}</span>} {removed > 0 && <span className="diff-minus">−{removed}</span>}
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
