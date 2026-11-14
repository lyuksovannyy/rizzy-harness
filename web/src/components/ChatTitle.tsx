// The chat's title in its header, renamed in place by its owner: Rename
// turns it into a field; Enter or Save keeps it, Escape or Cancel leaves it.
// The server cleans and caps it (control and hidden characters dropped).
import "../lib/i18nPages";
import { Pencil } from "lucide-react";
import { useState } from "react";
import { pageApi } from "../apiPages";
import { t } from "../lib/i18n";
import { shownName } from "../md/hidden";

/** The most the server keeps (`harness_store::MAX_TITLE_CHARS`). */
export const TITLE_MAX = 200;

export function ChatTitle(props: { sessionId: string; title: string; canRename: boolean }) {
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const save = async () => {
    if (draft === null) return;
    const title = draft.trim();
    if (!title || title === props.title) {
      setDraft(null);
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await pageApi.renameSession(props.sessionId, title);
      setDraft(null);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
  if (draft !== null)
    return (
      <form
        className="chat-rename row"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <input
          aria-label={t("conv.renameField")}
          value={draft}
          maxLength={TITLE_MAX}
          autoFocus
          disabled={busy}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              setDraft(null);
              setErr(null);
            }
          }}
        />
        <button type="submit" className="primary small" disabled={busy}>
          {t("conv.renameSave")}
        </button>
        <button type="button" className="small" onClick={() => (setDraft(null), setErr(null))}>
          {t("workspace.cancel")}
        </button>
        {err && <span className="error-text small">{err}</span>}
      </form>
    );
  return (
    <span className="chat-title-row row">
      <h1 className="ellipsis">{shownName(props.title)}</h1>
      {props.canRename && (
        <button
          type="button"
          className="icon small"
          aria-label={t("conv.rename")}
          title={t("conv.rename")}
          onClick={() => setDraft(props.title)}
        >
          <Pencil size={14} />
        </button>
      )}
    </span>
  );
}
