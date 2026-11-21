// Turns a rewind took out of the conversation (X.46), as one block:
// "N turns rewound", closed, opened to read them (muted, with nothing to
// click inside). The block of a rewind that can still be undone offers
// "Undo rewind" and, when the box does not hold the message, "Put the message
// back in the box". While the files a rewind took back are still back, one
// line says so with "Bring the files back". Loads with the conversation chunk.
import { ChevronRight } from "lucide-react";
import { useState, type ReactNode } from "react";
import { pageApi } from "../apiPages";
import { errText } from "../lib/errorText";
import { forceable } from "../lib/rewind";
import { t } from "../lib/i18n";
import "../lib/i18nPages";
import "./rewoundBlock.css";

export function RewoundBlock(props: {
  sessionId: string;
  /** The rewind's seq: what "Undo rewind" names. */
  rewind: number;
  turns: number;
  /** The rewind can be put back, and the viewer may do it now. */
  canUndo: boolean;
  /** The files this rewind took back are still back: the checkpoint that
   * holds what the restore replaced. Absent when there is nothing to bring
   * back (no files part, brought back already, or the viewer may not). */
  filesBack?: string;
  /** A turn runs: the restore would be refused. */
  running?: boolean;
  /** Attachments of the message that the box cannot take back: attached again by hand. */
  lost: string[];
  /** The box may be given the message on request; absent once it was. */
  putBack?: () => void;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const undo = async () => {
    setBusy(true);
    setErr(null);
    try {
      await pageApi.undoRewind(props.sessionId, props.rewind);
    } catch (x) {
      setErr(errText(x));
    } finally {
      setBusy(false);
    }
  };
  // The restore the rewind made, put back: the existing restore route with the
  // id it saved, through the same refusals as any undo.
  const [gone, setGone] = useState(false);
  const [filesErr, setFilesErr] = useState<string | null>(null);
  const bringBack = async (force: boolean) => {
    if (!props.filesBack) return;
    setBusy(true);
    setFilesErr(null);
    try {
      await pageApi.restoreTurn(props.sessionId, props.filesBack, force, false);
    } catch (x) {
      const m = errText(x);
      if (m.includes("no longer kept")) setGone(true);
      setFilesErr(m);
    } finally {
      setBusy(false);
    }
  };
  const id = `rewound-${props.rewind}-${props.turns}`;
  return (
    <div className="rewound-block">
      <div className="rewound-head small muted">
        <button type="button" className="link rewound-toggle" aria-expanded={open} aria-controls={open ? id : undefined} onClick={() => setOpen(!open)}>
          <ChevronRight size={14} aria-hidden className={open ? "open" : ""} />{" "}
          {props.turns === 1 ? t("rewind.turn") : t("rewind.turns", { n: String(props.turns) })}
        </button>
        {props.canUndo && (
          <button type="button" className="rewound-btn" disabled={busy} onClick={() => void undo()}>
            {t("rewind.undo")}
          </button>
        )}
        {props.putBack && (
          <button type="button" className="rewound-btn" onClick={props.putBack}>
            {t("rewind.putBack")}
          </button>
        )}
      </div>
      {!!props.lost.length && <div className="small muted">{t("rewind.attachAgain", { names: props.lost.join(", ") })}</div>}
      {props.filesBack && !gone && (
        <div className="small muted rewound-files">
          {t("rewind.filesBack")}{" "}
          <button type="button" className="rewound-btn" disabled={busy || props.running} onClick={() => void bringBack(false)}>
            {t("rewind.bringBack")}
          </button>
        </div>
      )}
      {filesErr && (
        <div className="error-text small" role="alert">
          {filesErr}{" "}
          {forceable(filesErr) && (
            <button type="button" className="rewound-btn" disabled={busy} onClick={() => void bringBack(true)}>
              {t("rewind.bringBackAnyway")}
            </button>
          )}
        </div>
      )}
      {err && (
        <div className="error-text small" role="alert">
          {err}
        </div>
      )}
      {open && (
        <div id={id} className="rewound-body">
          {props.children}
        </div>
      )}
    </div>
  );
}
