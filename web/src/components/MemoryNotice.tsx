// What a `remember` call did, in the harness's words, read back from the
// server (the node only reports that it sent the call): saved at once in
// the Personal workspace, with Undo, so "without a click" is never "without
// knowing"; proposed elsewhere, waiting on the Memory page (rich_plan §13: no
// inline Keep/Discard for proposals); refused, with the reason. The fact is
// the model's text and is stored as written, so hidden characters show as
// codes here: the person should see what goes into future chats.
import { BookmarkPlus } from "lucide-react";
import { useEffect, useState } from "react";
import { pageApi } from "../apiPages";
import { MEMORY_HASH } from "../lib/routes";
import { showHidden } from "../md/hidden";
import { rt } from "../md/strings";
import type { MemoryCallState } from "../types";

const quote = (t: string) => <span className="memory-fact">“{showHidden(t.trim())}”</span>;

/** `sessionId`/`callId` absent (a project chat's personal fallback, tests):
 * the line only says what the assistant suggested. */
export function MemoryNotice({
  text,
  sessionId,
  callId,
  forget,
}: {
  text: string;
  sessionId?: string;
  callId?: string;
  /** The call only drops a fact (`forget` alone). */
  forget?: boolean;
}) {
  const [st, setSt] = useState<MemoryCallState | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    if (!sessionId || !callId) return;
    let live = true;
    pageApi
      .memoryCall(sessionId, callId)
      .then((v) => live && setSt(v))
      .catch(() => live && setSt(null));
    return () => {
      live = false;
    };
  }, [sessionId, callId]);

  const undo = async () => {
    if (!sessionId || !callId) return;
    setBusy(true);
    setErr(null);
    try {
      setSt(await pageApi.undoMemoryCall(sessionId, callId));
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
  const undoButton = (
    <button type="button" className="link memory-undo" disabled={busy} onClick={() => void undo()}>
      {rt("memory.undo")}
    </button>
  );
  const review = (
    <a className="memory-review" href={MEMORY_HASH}>
      {rt("memory.review")}
    </a>
  );

  let head: React.ReactNode;
  let foot: React.ReactNode;
  const old = st?.retired?.text;
  switch (st?.state) {
    case "saved":
      head = (
        <>
          {rt(st.place === "workspace" ? "memory.savedHere" : "memory.saved")} {quote(st.fact?.text ?? text)}
          {old && <> {rt("memory.replacing")} {quote(old)}</>}
        </>
      );
      foot = (
        <>
          {undoButton} · {review}
        </>
      );
      break;
    case "forgot":
      head = (
        <>
          {rt("memory.forgot")} {quote(old ?? "")}
        </>
      );
      foot = undoButton;
      break;
    case "refused":
      head = (
        <>
          {rt("memory.refused")} {text && quote(text)}
        </>
      );
      foot = st.reason ?? "";
      break;
    case "undone":
      head = (
        <>
          {rt("memory.undone")} {quote(forget ? (old ?? "") : text)}
        </>
      );
      break;
    case "none":
    case "retired":
      head = (
        <>
          {rt("memory.gone")} {text && quote(text)}
        </>
      );
      foot = review;
      break;
    case "proposed":
    default:
      if (!st && sessionId && callId) {
        head = (
          <>
            {rt("memory.sent")} {forget ? null : quote(text)}
          </>
        );
        break;
      }
      head = (
        <>
          {rt("memory.proposed")} {quote(text)}
        </>
      );
      foot = (
        <>
          {rt("memory.pending")} {review}
        </>
      );
  }
  return (
    <div className="tool tool-memory" role="note">
      <BookmarkPlus size={14} aria-hidden="true" className="memory-icon" />
      <div className="memory-body">
        <div>{head}</div>
        {foot && <div className="muted small">{foot}</div>}
        {err && <div className="error-text small">{err}</div>}
      </div>
    </div>
  );
}
