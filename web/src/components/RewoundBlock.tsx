// Turns a rewind took out of the conversation (X.46), as one block:
// "N turns rewound", closed, opened to read them (muted, with nothing to
// click inside). The block of a rewind that can still be undone offers
// "Undo rewind" and, when the box did not take the message back by itself,
// "Put the message back in the box". Loads with the conversation chunk.
import { ChevronRight } from "lucide-react";
import { useState, type ReactNode } from "react";
import { pageApi } from "../apiPages";
import { errText } from "../lib/errorText";
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
  /** The files went back too. */
  filesToo: boolean;
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
  const id = `rewound-${props.rewind}-${props.turns}`;
  return (
    <div className="rewound-block">
      <div className="rewound-head small muted">
        <button type="button" className="link rewound-toggle" aria-expanded={open} aria-controls={open ? id : undefined} onClick={() => setOpen(!open)}>
          <ChevronRight size={14} aria-hidden className={open ? "open" : ""} />{" "}
          {props.turns === 1 ? t("rewind.turn") : t("rewind.turns", { n: String(props.turns) })}
        </button>
        {props.canUndo && (
          <button type="button" className="link" disabled={busy} onClick={() => void undo()}>
            {t("rewind.undo")}
          </button>
        )}
        {props.putBack && (
          <button type="button" className="link" onClick={props.putBack}>
            {t("rewind.putBack")}
          </button>
        )}
      </div>
      {!!props.lost.length && <div className="small muted">{t("rewind.attachAgain", { names: props.lost.join(", ") })}</div>}
      {props.filesToo && <div className="small muted">{t("rewind.filesToo")}</div>}
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
