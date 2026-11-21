// The chat's title in its header, renamed in place by its owner with no
// buttons: a click (or Enter or F2 on the focused title) turns it into a
// field, text selected. Enter applies, a click outside applies, Escape
// cancels. A blank or unchanged title sends nothing. The server cleans and
// caps it (control and hidden characters dropped); its refusal is shown
// beside the field, which stays open with what was typed.
import "../lib/i18nPages";
import { errText } from "../lib/errorText";
import { useEffect, useRef, useState } from "react";
import { pageApi } from "../apiPages";
import { t } from "../lib/i18n";
import { shownName } from "../md/hidden";

/** The most the server keeps (`harness_store::MAX_TITLE_CHARS`). */
export const TITLE_MAX = 200;

export function ChatTitle(props: { sessionId: string; title: string; canRename: boolean }) {
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // The title the server returned, shown until the list's refetch brings it
  // (no flash of the old one after it applied).
  const [saved, setSaved] = useState<string | null>(null);
  if (saved !== null && saved === props.title) setSaved(null);
  const title = saved ?? props.title;
  const input = useRef<HTMLInputElement>(null);
  // Set when Escape or an apply closed the field: the blur that follows it
  // (an unmounting field may fire one) must not apply anything.
  const closed = useRef(false);
  const inflight = useRef(false);
  const editing = draft !== null;
  useEffect(() => {
    if (!editing) return;
    input.current?.focus();
    input.current?.select();
  }, [editing]);
  const open = () => {
    // Selecting the title to copy it is not a click to rename.
    const sel = window.getSelection?.();
    if (sel && !sel.isCollapsed && sel.toString().trim()) return;
    closed.current = false;
    setErr(null);
    setDraft(title);
  };
  const cancel = () => {
    closed.current = true;
    setDraft(null);
    setErr(null);
  };
  const apply = async () => {
    if (draft === null || inflight.current || closed.current) return;
    const next = draft.trim();
    if (!next || next === title) return cancel();
    inflight.current = true;
    setBusy(true);
    setErr(null);
    try {
      const r = await pageApi.renameSession(props.sessionId, next);
      setSaved(r.title);
      closed.current = true;
      setDraft(null);
    } catch (x) {
      setErr(errText(x));
    } finally {
      inflight.current = false;
      setBusy(false);
    }
  };
  if (draft !== null)
    return (
      <span className="chat-title-row row">
        <h1 className="chat-title-edit">
          <input
            ref={input}
            aria-label={t("conv.renameField")}
            value={draft}
            maxLength={TITLE_MAX}
            readOnly={busy}
            style={{ width: `${Math.max(draft.length, 6) + 1}ch` }}
            aria-invalid={err ? true : undefined}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => void apply()}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                cancel();
              } else if (e.key === "Enter") {
                e.preventDefault();
                // The Enter that confirms an IME composition is not ours.
                if (e.nativeEvent.isComposing || e.keyCode === 229) return;
                void apply();
              }
            }}
          />
        </h1>
        {err && (
          <span className="error-text small" role="alert">
            {err}
          </span>
        )}
      </span>
    );
  return (
    <span className="chat-title-row row">
      <h1 className="ellipsis">
        {props.canRename ? (
          <button
            type="button"
            className="chat-title-btn"
            title={t("conv.rename")}
            aria-keyshortcuts="Enter F2"
            onClick={open}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === "F2") {
                e.preventDefault();
                open();
              }
            }}
          >
            {shownName(title)}
          </button>
        ) : (
          shownName(title)
        )}
      </h1>
    </span>
  );
}
