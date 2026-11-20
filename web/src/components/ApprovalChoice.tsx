// The choice a waiting call asks for (row W.6): what will run, and the
// buttons that answer it. One component draws it wherever it shows: in the
// composer's place (`place`: the open chat, one call at a time, key-guarded)
// or in the call's card (a chat's other views, a sub-agent's request). The
// answering is the same code either way. Loads with the conversation chunk.
import { createContext, useEffect, useMemo, useState } from "react";
import { pageApi } from "../apiPages";
import { fields as fieldsOf, fmt, previewField, unseen } from "../lib/callText";
import { t } from "../lib/i18n";
import "../lib/i18nPages";
import { showHidden } from "../md/hidden";
import type { Decision, Item } from "../types";
import { CallInput, useOpenFields } from "./CallInput";
import "./approvalChoice.css";

type Tool = Extract<Item, { type: "tool" }>;

/** How long the composer's choice ignores every answer after it appears.
 * Long enough for a typing burst (keystrokes come 100 to 250 ms apart, and a
 * person needs about 300 ms to see that the field has changed) to end, short
 * enough that the buttons do not feel stuck: below a second, as the browsers'
 * own install prompts are (they wait a full second). */
export const GUARD_MS = 700;

/** The approval id the composer shows now: that call's card in the transcript
 * leaves its buttons to the composer's place. */
export const Held = createContext<string | null>(null);

/** The longest note with a denial (`policy::MAX_DENY_NOTE`). */
const MAX_DENY_NOTE = 2000;

/** The call's fields as the card shows them, with the rows a line edit replaces. */
export function useShown(item: Tool) {
  const preview = item.approval?.preview;
  return useMemo(() => {
    const f = fieldsOf(item.tool, item.input);
    return preview ? [...f, previewField(preview, t("approval.rows"))] : f;
  }, [item.tool, item.input, preview]);
}

/** `place`: this is the composer's choice ("n of of" pending in this chat,
 * answers ignored for GUARD_MS). `lead`: what comes before the fields (a web
 * call's address). `blocked`: the viewer may only deny. */
export function ApprovalChoice({
  sessionId,
  item,
  blocked,
  cwd,
  lead,
  place,
}: {
  sessionId: string;
  item: Tool;
  blocked?: boolean;
  cwd?: string;
  lead?: React.ReactNode;
  place?: { n: number; of: number };
}) {
  const ap = item.approval!;
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // The person's note for a denial (T51): kept while they type, sent only
  // with Deny; never focused by itself, so nothing typed elsewhere lands
  // here, and Enter in it answers nothing. It is not the composer's draft.
  const [note, setNote] = useState("");
  const [openFields, toggleField] = useOpenFields();
  const shown = useShown(item);
  const notExpanded = unseen(shown, openFields);
  const [armed, setArmed] = useState(!place);
  useEffect(() => {
    if (!place) return;
    const id = setTimeout(() => setArmed(true), GUARD_MS);
    return () => clearTimeout(id);
  }, [!place]);
  // The announcement is put in an empty live region after it is there.
  const [said, setSaid] = useState("");
  const words = place ? `${t("approval.waits")}${place.of > 1 ? ` (${t("approval.count", { n: String(place.n), of: String(place.of) })})` : ""}` : "";
  useEffect(() => setSaid(words), [words]);

  const decide = async (d: Decision) => {
    if (!armed) return;
    setBusy(true);
    setErr(null);
    try {
      const n = d === "deny" ? note.trim() : "";
      if (n) await pageApi.approve(sessionId, ap.approval_id, d, n);
      else await pageApi.approve(sessionId, ap.approval_id, d);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
  // Until the guard ends a key answers nothing (Space acts on key-up).
  const swallow = (e: React.KeyboardEvent) => {
    if (!armed && (e.key === "Enter" || e.key === " ") && !(e.target instanceof HTMLInputElement)) {
      e.preventDefault();
      e.stopPropagation();
    }
  };
  const gate = armed ? undefined : true;
  const seen = notExpanded > 0 ? `unseen-${ap.approval_id}` : undefined;
  return (
    <div
      className={`approval${place ? " approval-choice" : ""}`}
      {...(place ? { role: "group", "aria-label": t("approval.label"), onKeyDownCapture: swallow, onKeyUpCapture: swallow } : {})}
    >
      {place && (
        <span className="sr-only" role="status">
          {said}
        </span>
      )}
      {place && place.of > 1 && (
        <div className="approval-count small muted" aria-hidden="true">
          {t("approval.count", { n: String(place.n), of: String(place.of) })}
        </div>
      )}
      <div className="approval-card">
        {lead}
        <CallInput fields={shown} cwd={cwd} open={openFields} onToggle={toggleField} />
        <div className="approval-reason">{showHidden(ap.reason)}</div>
        {/* Allow stays usable while part of the input is collapsed; this
            line says how much of it the click approves unseen. */}
        {notExpanded > 0 && (
          <div className="call-unseen" id={seen} role="note">
            You are approving {fmt(notExpanded)} characters you have not expanded.
          </div>
        )}
        {ap.chat_allow && !blocked && (
          <div className="small muted chat-allow-said" id={`chat-${ap.approval_id}`} role="note">
            &ldquo;For this chat&rdquo; lets this exact call run with no card for the rest of the chat, also after a
            restart, and it runs whatever the files it uses say then: allowing <code>make test</code> allows whatever
            the Makefile and the tests say, which the agent may change.
          </div>
        )}
      </div>
      <div className={`approval-actions row wrap${armed ? "" : " guarded"}`}>
        {!blocked && (
          <>
            <button className="primary" disabled={busy} aria-disabled={gate} aria-describedby={seen} onClick={() => decide("allow")}>
              Allow once
            </button>
            {ap.turn_allow && (
              <button disabled={busy} aria-disabled={gate} aria-describedby={seen} onClick={() => decide("allow_session")}>
                Allow this exact call for this turn
              </button>
            )}
            {ap.chat_allow && (
              <button
                disabled={busy}
                aria-disabled={gate}
                aria-describedby={`chat-${ap.approval_id}${seen ? ` ${seen}` : ""}`}
                title="The same call runs without asking for the rest of this chat; remove it from the side panel"
                onClick={() => decide("allow_chat")}
              >
                Allow this exact call for this chat
              </button>
            )}
          </>
        )}
        <input
          className="deny-note"
          aria-label="Note for the agent with Deny (optional)"
          placeholder="Why not? (optional, sent with Deny)"
          maxLength={MAX_DENY_NOTE}
          value={note}
          disabled={busy}
          onChange={(e) => setNote(e.target.value)}
          // Enter answers nothing: only the Deny button sends the note.
          onKeyDown={(e) => {
            if (e.key === "Enter") e.preventDefault();
            e.stopPropagation();
          }}
        />
        <button className="danger" disabled={busy} aria-disabled={gate} onClick={() => decide("deny")}>
          Deny
        </button>
      </div>
      {err && <div className="error-text">{showHidden(err)}</div>}
    </div>
  );
}
