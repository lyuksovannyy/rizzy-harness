// "Rewind to here" on a message of the person's (X.46, docs/rewind.md): a
// small action under the bubble and, opened, an inline panel with the
// choice, no dialog. The panel says what happens, offers the conversation
// alone or with the files (only when a turn from this message on changed
// some) and lists what no rewind takes back. The route decides again; its
// refusal is shown here, and a files part refused for another chat's work
// in the same files can be repeated with `force` ("Rewind anyway").
// Loads with the conversation chunk.
import { Undo2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { pageApi } from "../apiPages";
import { errText } from "../lib/errorText";
import { raise } from "../lib/choiceGuard";
import { t } from "../lib/i18n";
import "../lib/i18nPages";
import { forceable } from "../lib/rewind";
import { GUARD_MS } from "./ApprovalChoice";
import "./rewindMessage.css";

export function RewindMessage(props: {
  sessionId: string;
  /** The seq of the message the chat goes back to. */
  seq: number;
  /** A turn from this message on changed files: the second button is offered. */
  files: boolean;
  /** The rewind was accepted: the caller refills the box and moves focus. */
  onDone: () => void;
}) {
  const [open, setOpen] = useState(false);
  // The buttons answer nothing for a moment after the panel opens: the
  // key that opened it (Space acts on key-up) or a double click must not
  // choose (as ApprovalChoice's guard).
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // What the last click asked, so "Rewind anyway" repeats exactly that.
  const asked = useRef(false);
  const root = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const alive = useRef(true);
  useEffect(() => () => void (alive.current = false), []);

  const close = (refocus: boolean) => {
    setOpen(false);
    setErr(null);
    if (refocus) opener.current?.focus();
  };
  useEffect(() => {
    if (!open) return;
    setArmed(false);
    // Focus waits on the panel, not on a button: the first Tab reaches the first one.
    panel.current?.focus();
    const id = setTimeout(() => setArmed(true), GUARD_MS);
    const drop = raise();
    const away = (e: Event) => {
      if (e.target instanceof Node && !root.current?.contains(e.target)) close(false);
    };
    document.addEventListener("pointerdown", away, true);
    return () => {
      clearTimeout(id);
      drop();
      document.removeEventListener("pointerdown", away, true);
    };
  }, [open]);

  const run = async (files: boolean, force = false) => {
    if (!armed || busy) return;
    setBusy(true);
    setErr(null);
    asked.current = files;
    try {
      await pageApi.rewind(props.sessionId, props.seq, files, force);
      if (alive.current) setOpen(false);
      props.onDone();
    } catch (x) {
      if (alive.current) setErr(errText(x));
    } finally {
      if (alive.current) setBusy(false);
    }
  };
  const gate = armed ? undefined : true;
  const id = `rewind-${props.seq}`;
  return (
    <div
      className="rewind-slot"
      ref={root}
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          e.stopPropagation();
          close(true);
        }
      }}
    >
      <button
        type="button"
        ref={opener}
        className="link small rewind-action"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        title={t("rewind.actionHint")}
        onClick={() => (open ? close(true) : setOpen(true))}
      >
        <Undo2 size={14} aria-hidden /> {t("rewind.action")}
      </button>
      {open && (
        <div ref={panel} id={id} className="rewind-panel" role="group" aria-label={t("rewind.label")} tabIndex={-1}>
          <p>{t("rewind.what")}</p>
          <div className={`rewind-buttons row wrap${armed ? "" : " guarded"}`}>
            <button type="button" className="primary" disabled={busy} aria-disabled={gate} onClick={() => void run(false)}>
              {t("rewind.chat")}
            </button>
            {props.files && (
              <button type="button" disabled={busy} aria-disabled={gate} aria-describedby={`${id}-files`} onClick={() => void run(true)}>
                {t("rewind.chatFiles")}
              </button>
            )}
          </div>
          {props.files && (
            <p id={`${id}-files`} className="small muted">
              {t("rewind.filesHelp")}
            </p>
          )}
          <p className="small muted">{t("rewind.notUndone")}</p>
          {err && (
            <>
              <div className="error-text" role="alert">
                {err}
              </div>
              {forceable(err) && (
                <>
                  <p className="small muted">{t("rewind.anywayHelp")}</p>
                  <div className="rewind-buttons row wrap">
                    <button type="button" className="link" disabled={busy} onClick={() => void run(asked.current, true)}>
                      {t("rewind.anyway")}
                    </button>
                  </div>
                </>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
