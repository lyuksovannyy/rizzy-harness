// What a `remember` call did, in the harness's words, read back from the
// server (the node only reports that it sent the call). In a Personal chat
// that read nothing from outside it was saved at once: the line says so,
// with Undo, so "without a click" is never "without knowing". Anywhere else
// it waits for the person: the line asks, with Keep and Dismiss, right here
// (review M1: one click, in the chat). A refusal says why. The fact is the
// model's text and is shown as stored: hidden characters as codes.
import { BookmarkPlus } from "lucide-react";
import { useEffect, useState } from "react";
import { pageApi } from "../apiPages";
import { MEMORY_HASH } from "../lib/routes";
import { showHidden } from "../md/hidden";
import { type RichKey, rt } from "../md/strings";
import type { MemoryCallState } from "../types";

const quote = (t: string) => <span className="memory-fact">“{showHidden(t.trim())}”</span>;

/** How often, and how many times, to ask again while the server has not
 * filed the call yet (`waiting`: the node reported the call before the
 * master filed it, or a reload raced it). */
const RETRY_MS = 700;
const RETRIES = 8;

/** `sessionId`/`callId` absent (a project chat's personal fallback): the
 * line only says what the assistant suggested. */
export function MemoryNotice({
  text,
  sessionId,
  callId,
  forget,
  done,
}: {
  text: string;
  sessionId?: string;
  callId?: string;
  /** The call only drops a fact (`forget` alone). */
  forget?: boolean;
  /** The node answered the call: ask the server again (it has filed it). */
  done?: boolean;
}) {
  const [st, setSt] = useState<MemoryCallState | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [tries, setTries] = useState(0);
  useEffect(() => {
    if (!sessionId || !callId) return;
    let live = true;
    const ask = () =>
      void pageApi
        .memoryCall(sessionId, callId)
        .then((v) => {
          if (!live) return;
          setSt(v);
          if (v.state === "waiting" && tries < RETRIES) setTries((n) => n + 1);
        })
        .catch(() => {
          if (live && tries < RETRIES) setTries((n) => n + 1);
        });
    // The first ask at once; a repeat after a pause.
    if (tries === 0) {
      ask();
      return () => {
        live = false;
      };
    }
    const timer = setTimeout(ask, RETRY_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [sessionId, callId, tries, done]);

  const act = async (what: "keep" | "dismiss" | "undo") => {
    if (!sessionId || !callId) return;
    setBusy(true);
    setErr(null);
    try {
      const f = what === "keep" ? pageApi.keepMemoryCall : what === "dismiss" ? pageApi.dismissMemoryCall : pageApi.undoMemoryCall;
      setSt(await f(sessionId, callId));
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
  const button = (what: "keep" | "dismiss" | "undo", primary = false) => (
    <button
      type="button"
      className={`link memory-${what}${primary ? " primary-link" : ""}`}
      disabled={busy}
      onClick={() => void act(what)}
    >
      {rt(`memory.${what}`)}
    </button>
  );
  const review = (
    <a className="memory-review" href={MEMORY_HASH}>
      {rt("memory.review")}
    </a>
  );

  const fact = st?.fact?.text ?? st?.text ?? text;
  // Where it lives: your facts, your Personal workspace (private details),
  // this (team) workspace's shared memory, the project's.
  const where = (user: RichKey, here: RichKey, shared: RichKey, project: RichKey) =>
    rt(
      st?.place === "project"
        ? project
        : st?.place === "workspace"
          ? st.workspace_personal
            ? here
            : shared
          : user,
    );
  const old = st?.target?.text;
  const isForget = st ? !!st.forget : !!forget;
  let head: React.ReactNode;
  let foot: React.ReactNode = null;
  switch (st?.state) {
    case "saved":
      head = old ? (
        <>
          {where("memory.replaced", "memory.replacedHere", "memory.replacedShared", "memory.replacedProject")} {quote(old)} →{" "}
          {quote(fact)}
        </>
      ) : (
        <>
          {where("memory.saved", "memory.savedHere", "memory.savedShared", "memory.savedProject")} {quote(fact)}
        </>
      );
      foot = (
        <>
          {button("undo")} · {review}
        </>
      );
      break;
    case "forgot":
      head = (
        <>
          {where("memory.forgot", "memory.forgot", "memory.forgotShared", "memory.forgotProject")} {quote(old ?? "")}
        </>
      );
      foot = button("undo");
      break;
    case "pending":
      head = isForget ? (
        <>
          {rt("memory.wantsForget")} {quote(old ?? "")}
        </>
      ) : old ? (
        <>
          {rt("memory.wantsReplace")} {quote(old)} → {quote(fact)}
        </>
      ) : (
        <>
          {where("memory.wants", "memory.wantsHere", "memory.wantsShared", "memory.wantsProject")} {quote(fact)}
        </>
      );
      foot =
        st.keep === false ? (
          <>
            {rt(st.decides === "admin" ? "memory.forAdmin" : "memory.forManager")} {button("dismiss")}
          </>
        ) : (
          <>
            {rt("memory.waitingHint")} {button("keep", true)} · {button("dismiss")}
          </>
        );
      break;
    case "kept":
      head = (
        <>
          {rt("memory.kept")} {isForget ? quote(old ?? "") : quote(fact)}
        </>
      );
      foot = button("undo");
      break;
    case "dismissed":
      head = (
        <>
          {rt("memory.dismissed")} {isForget ? quote(old ?? "") : quote(fact)}
        </>
      );
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
      // What Undo did: a save removed (and, for a replacement, the old
      // fact back), or a forgotten fact back.
      head = isForget ? (
        <>
          {rt("memory.undone")} {quote(old ?? "")} {rt("memory.undoneBack")}
        </>
      ) : old ? (
        <>
          {rt("memory.undone")} {quote(old)} {rt("memory.undoneBack")}
        </>
      ) : (
        <>
          {rt("memory.undone")} {quote(fact)} {rt("memory.undoneSave")}
        </>
      );
      break;
    case "gone":
      head = (
        <>
          {rt("memory.gone")} {quote(fact)}
        </>
      );
      foot = review;
      break;
    case "project":
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
      break;
    default:
      // Not answered yet, or nothing filed (memory off for this turn).
      head = (
        <>
          {rt("memory.sent")} {isForget ? null : quote(text)}
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
