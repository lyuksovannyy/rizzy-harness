// The choice a waiting call asks for (row W.6): what will run, and the
// buttons that answer it. One component draws it wherever it shows: in the
// composer's place (`place`: the open chat, one call at a time, key-guarded)
// or in the call's card (a chat's other views, a sub-agent's request). The
// answering is the same code either way. Loads with the conversation chunk.
import { createContext, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { pageApi } from "../apiPages";
import { raise } from "../lib/choiceGuard";
import { fields as fieldsOf, fmt, previewField, unseen } from "../lib/callText";
import { t } from "../lib/i18n";
import "../lib/i18nPages";
import { shownName, showHidden } from "../md/hidden";
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
    // A push's card (PLAN.md T101): what will be pushed, as plain text.
    const label = item.tool === "push" ? t("approval.push") : t("approval.rows");
    return preview ? [...f, previewField(preview, label)] : f;
  }, [item.tool, item.input, preview]);
}

/** `place`: this is the composer's choice (answers ignored for GUARD_MS). `lead`: what comes before the fields (a web
 * call's address). `blocked`: the viewer may only deny. */
export function ApprovalChoice({
  sessionId,
  item,
  blocked,
  cwd,
  lead,
  place,
  boxRef,
}: {
  sessionId: string;
  item: Tool;
  blocked?: boolean;
  cwd?: string;
  lead?: React.ReactNode;
  place?: boolean;
  /** The choice's own container, which takes focus when it appears (the composer hands it over). */
  boxRef?: RefObject<HTMLDivElement | null>;
}) {
  const ap = item.approval!;
  // A card whose approval names another tool than its call (a node that
  // labels one call's card as another's): drawn as invalid, Deny only.
  const mismatch = ap.tool !== undefined && ap.tool !== item.tool;
  // A card whose approval id or call was announced twice (review N1).
  const reused = ap.invalid === true;
  // A push card's destination is the server's, never the node's text.
  const push = !mismatch && !reused && (item.tool === "push" || item.tool === "pull_request");
  const [target, setTarget] = useState<{ host: string; full_name: string; login: string } | null | undefined>(
    undefined,
  );
  useEffect(() => {
    if (!push) return;
    let live = true;
    pageApi.pushTarget(sessionId).then(
      (t) => live && setTarget(t),
      () => live && setTarget(null),
    );
    return () => {
      live = false;
    };
  }, [push, sessionId, ap.approval_id]);
  const noAllow = !!blocked || mismatch || reused || (push && !target);
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
  // The page knows a choice is up (global shortcuts that react to plain
  // typing do nothing), from the first render in the composer's place.
  useLayoutEffect(() => (place ? raise() : undefined), [!place]);
  // While the guard runs, a key not aimed at a control of the choice is
  // eaten before any shortcut sees it: a burst of typing that began in the
  // field (now gone) must not open search, send or answer. Chords and keys
  // that move (Tab, Esc, arrows) pass.
  const box = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!place || armed) return;
    const eat = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || !(e.key.length === 1 || e.key === "Enter")) return;
      const el = e.target instanceof Element ? e.target : null;
      const own = box.current;
      if (own && el && el !== own && own.contains(el) && el.closest("button, input, a")) return;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", eat, true);
    window.addEventListener("keyup", eat, true);
    return () => {
      window.removeEventListener("keydown", eat, true);
      window.removeEventListener("keyup", eat, true);
    };
  }, [!place, armed]);
  const setBox = useCallback(
    (el: HTMLDivElement | null) => {
      box.current = el;
      if (boxRef) boxRef.current = el;
    },
    [boxRef],
  );
  // The command scrolls; whether more of it is below the cut is said under it.
  const scroll = useRef<HTMLDivElement>(null);
  const [more, setMore] = useState(false);
  const measure = useCallback(() => {
    const el = scroll.current;
    setMore(!!el && el.scrollHeight - el.clientHeight - el.scrollTop > 1);
  }, []);
  useLayoutEffect(() => {
    if (!place) return;
    measure();
    const el = scroll.current;
    if (!el || typeof ResizeObserver !== "function") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [!place, measure, shown, openFields]);
  // The announcement is put in an empty live region after it is there.
  const [said, setSaid] = useState("");
  const words = place
    ? reused
      ? t("approval.reused")
      : t("approval.waits", { tool: shownName(ap.tool ?? item.tool) })
    : "";
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
      {...(place
        ? {
            ref: setBox,
            tabIndex: -1,
            role: "group",
            "aria-label": t("approval.label"),
            onKeyDownCapture: swallow,
            onKeyUpCapture: swallow,
            // The first Tab from the container (where focus waits) reaches the first button.
            onKeyDown: (e: React.KeyboardEvent<HTMLDivElement>) => {
              if (e.key !== "Tab" || e.shiftKey || e.target !== e.currentTarget) return;
              const first = e.currentTarget.querySelector<HTMLElement>(".approval-actions button");
              if (!first) return;
              e.preventDefault();
              first.focus();
            },
          }
        : {})}
    >
      {place && (
        <span className="sr-only" role="status">
          {said}
        </span>
      )}
      <div
        className="approval-scroll"
        ref={scroll}
        {...(place ? { role: "region", tabIndex: 0, "aria-label": t("approval.scroll"), onScroll: measure } : {})}
      >
        {reused && (
          <div className="error-text approval-invalid" role="note">
            {t("approval.reused")}
          </div>
        )}
        {mismatch && !reused && (
          <div className="error-text approval-invalid" role="note">
            {t("approval.mismatch", { asked: shownName(ap.tool!), call: shownName(item.tool) })}
          </div>
        )}
        {push && target && (
          <div className="approval-push-target" role="note">
            {t(item.tool === "pull_request" ? "approval.prIn" : "approval.pushTo", {
              target: `${target.host}/${target.full_name}`,
              login: target.login,
            })}
          </div>
        )}
        {push && target === null && (
          <div className="error-text" role="note">
            {t("approval.pushNone")}
          </div>
        )}
        {lead}
        <CallInput fields={shown} cwd={cwd} open={openFields} onToggle={toggleField} />
      </div>
      {place && more && (
        <div className="approval-more small" aria-hidden="true">
          {t("approval.more")}
        </div>
      )}
      <div className="approval-reason">{showHidden(ap.reason)}</div>
      {/* Allow stays usable while part of the input is collapsed; this
          line says how much of it the click approves unseen. */}
      {notExpanded > 0 && (
        <div className="call-unseen" id={seen} role="note">
          You are approving {fmt(notExpanded)} characters you have not expanded.
        </div>
      )}
      {ap.chat_allow && !noAllow && (
        <div className="small muted chat-allow-said" id={`chat-${ap.approval_id}`} role="note">
          &ldquo;For this chat&rdquo; lets this exact call run with no card for the rest of the chat, also after a
          restart, and it runs whatever the files it uses say then: allowing <code>make test</code> allows whatever
          the Makefile and the tests say, which the agent may change.
        </div>
      )}
      <div className={`approval-actions row wrap${armed ? "" : " guarded"}`}>
        {!noAllow && (
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
