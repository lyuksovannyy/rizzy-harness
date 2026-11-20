// The "Delete chat" confirm: one in-page dialog (no browser confirm boxes)
// that also offers the viewer's Media files that came from the chat.
import "../lib/i18nPages";
import { useEffect, useRef, useState } from "react";
import { pageApi } from "../apiPages";
import { dropDraft } from "../lib/drafts";
import { errText } from "../lib/errorText";
import { t } from "../lib/i18n";
import { mediaApi } from "../lib/media";

/** The Media files from this chat: how many on the first page, and whether more follow. */
interface FromChat {
  n: number;
  more: boolean;
}

export function DeleteChat(props: {
  sessionId: string;
  workspaceId: string | null;
  /** An incognito chat has no Media of its own: nothing to offer. */
  incognito: boolean;
  onClose: () => void;
}) {
  const { sessionId, workspaceId, incognito } = props;
  const dialog = useRef<HTMLDialogElement>(null);
  const [files, setFiles] = useState<FromChat | null>(null);
  const [withMedia, setWithMedia] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    const d = dialog.current;
    if (d && typeof d.showModal === "function" && !d.open) d.showModal();
  }, []);

  // Media off, or no such files: the dialog is just about the chat.
  useEffect(() => {
    if (incognito) return;
    let live = true;
    mediaApi
      .list({ workspace: workspaceId, session: sessionId })
      .then((r) => live && setFiles({ n: r.items.length, more: !!r.next }))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [incognito, workspaceId, sessionId]);

  const close = () => {
    dialog.current?.close?.();
    props.onClose();
  };
  const confirm = async () => {
    setBusy(true);
    setErr(null);
    // Its unsent draft goes with it (other tabs: pruneDrafts). A delete the
    // server refused stays here with its reason: going home would look done.
    try {
      await pageApi.deleteSession(sessionId, { media: withMedia && !!files?.n });
    } catch (x) {
      setErr(errText(x));
      setBusy(false);
      return;
    }
    dropDraft(sessionId);
    location.hash = "#/";
  };

  return (
    <dialog
      ref={dialog}
      className="delete-chat"
      aria-label={t("conv.delete")}
      onClose={props.onClose}
      // A click on the backdrop lands on the dialog itself.
      onClick={(e) => e.target === dialog.current && close()}
    >
      <div className="media-confirm stack">
        <p>{t("conv.deleteConfirm")}</p>
        {/* No number: the sidebar's list leaves some out (a routine's runs, another workspace). */}
        <p>{t("conv.deleteSubs")}</p>
        {!!files?.n && (
          <label className="row">
            <input type="checkbox" checked={withMedia} onChange={(e) => setWithMedia(e.target.checked)} />
            <span>{mediaLine(files)}</span>
          </label>
        )}
        {err && (
          <p className="error-text" role="alert">
            {err}
          </p>
        )}
        <div className="row end">
          <button type="button" autoFocus onClick={close}>
            Cancel
          </button>
          <button type="button" className="danger" disabled={busy} onClick={() => void confirm()}>
            Delete
          </button>
        </div>
      </div>
    </dialog>
  );
}

/** "Also delete the 3 Media files…", left unticked: they stay in Media unless asked. */
function mediaLine(f: FromChat): string {
  const count = `${f.n}${f.more ? "+" : ""}`;
  return f.n === 1 && !f.more
    ? "Also delete the Media file that came from this chat"
    : `Also delete the ${count} Media files that came from this chat`;
}
