// What a question and a form card share: the harness chrome around the
// model's content (where the agent had been reading, why the card is
// locked, the fixed footer), sending the answer, and the draft kept while
// the user types. The node checks every answer against its log; nothing
// here decides whether an answer counts.
import { useEffect, useRef, useState, type ReactNode } from "react";
import { api } from "../api";
import type { InputInfo } from "../md/context";
import { rt } from "../md/strings";
import type { FormReply, FormValue } from "../types";

export interface InputProps {
  sessionId: string | null;
  messageId: string | null;
  blockId: string;
  info: InputInfo;
}

/** A card outside a conversation (a preview) can show but never send. */
export const PREVIEW: InputInfo = { state: { kind: "open" }, notice: [] };

const draftKey = (p: InputProps) => `harness.ui.${p.messageId ?? ""}#${p.blockId}`;

/** The answer as it is sent, which is also what its size is checked on. */
export const replyOf = (p: InputProps, values: Record<string, FormValue>): FormReply => ({
  message_id: p.messageId ?? "",
  block_id: p.blockId,
  values,
});

// Session storage can be full, off or blocked; a lost draft is fine.
export function loadDraft(p: InputProps): unknown {
  try {
    const raw = sessionStorage.getItem(draftKey(p));
    return raw === null ? null : JSON.parse(raw);
  } catch {
    return null;
  }
}

export function saveDraft(p: InputProps, draft: unknown, empty: boolean) {
  try {
    if (empty) sessionStorage.removeItem(draftKey(p));
    else sessionStorage.setItem(draftKey(p), JSON.stringify(draft));
  } catch {
    /* not kept */
  }
}

/**
 * Send an answer. `ready` is false while the card cannot send (locked, a
 * preview, already sending). After a successful POST it stays busy until
 * the log closes the card (the answer arrives as the next user message).
 * `frame` is the card's body (focusable, named by its question or title).
 */
export function useSend(p: InputProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const frame = useRef<HTMLDivElement>(null);
  const kind = p.info.state.kind;
  useEffect(() => {
    if (kind !== "open") setBusy(false);
  }, [kind]);
  const ready = kind === "open" && !!p.sessionId && !!p.messageId && !busy;
  const send = async (values: Record<string, FormValue>) => {
    if (!ready) return;
    // The control that sent is disabled now and gone once the card is
    // answered, which would drop keyboard focus to the top of the page; it
    // moves to the card, which stays. Only from inside the card: focus
    // elsewhere is the user's.
    if (frame.current?.contains(document.activeElement)) frame.current.focus();
    setBusy(true);
    setError(null);
    try {
      await api.prompt(p.sessionId!, "", { reply: replyOf(p, values) });
      saveDraft(p, null, true);
    } catch (x) {
      setError(x instanceof Error ? x.message : String(x));
      setBusy(false);
    }
  };
  return { ready, busy, error, send, frame };
}

function hostList(hosts: string[]): string {
  const shown = hosts.slice(0, 3).join(", ");
  return hosts.length > 3 ? rt("ui.noticeMore", { hosts: shown, n: String(hosts.length - 3) }) : shown;
}

/** The harness's lines around a card's content. */
export function InputFrame(props: { info: InputInfo; error: string | null; children: ReactNode }) {
  const { state, notice } = props.info;
  return (
    <>
      {notice.length > 0 && (
        <p className="ui-notice" role="note">
          {rt("ui.notice", { hosts: hostList(notice) })}
        </p>
      )}
      {props.children}
      {state.kind !== "open" && <p className={`ui-state ui-state-${state.kind}`}>{rt(`ui.state.${state.kind}`)}</p>}
      {props.error && (
        <p className="ui-send-error" role="alert">
          {props.error}
        </p>
      )}
      <p className="ui-footer">{rt("ui.footer")}</p>
    </>
  );
}

export function SendButton(props: { disabled: boolean; busy: boolean; onClick: () => void }) {
  return (
    <button type="button" className="primary ui-send" disabled={props.disabled} onClick={props.onClick}>
      {props.busy ? rt("ui.sending") : rt("ui.send")}
    </button>
  );
}
