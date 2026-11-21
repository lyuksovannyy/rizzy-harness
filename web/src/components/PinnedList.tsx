// "Pinned (n)" in the chat's header and the list it opens (docs/pins.md): each
// pin's first words, who wrote it and when; choosing one scrolls to the
// message and marks it for a moment; the owner can unpin from the list. The
// list is readable wherever the chat is. Escape and a click outside close it.
// Loads with the conversation chunk.
import { Pin } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { pageApi } from "../apiPages";
import { errText } from "../lib/errorText";
import { t } from "../lib/i18n";
import "../lib/i18nPages";
import type { PinInfo } from "../types";
import { RelTime } from "./MessageActions";
import "./pinned.css";

/** How long the message a pin led to stays marked. */
export const FLASH_MS = 1600;

/** Scroll to the message written at `seq` and mark it for a moment; false when it is not on the page. */
export function jumpTo(seq: number, root: ParentNode = document): boolean {
  const el = root.querySelector<HTMLElement>(`[data-msg-seq="${seq}"]`);
  if (!el) return false;
  el.scrollIntoView?.({ block: "center", behavior: "smooth" });
  el.classList.add("pin-flash");
  window.setTimeout(() => el.classList.remove("pin-flash"), FLASH_MS);
  return true;
}

/** The small mark on a pinned message. */
export function PinMark() {
  return (
    <span className="pin-mark" role="img" aria-label={t("msg.pinned")} title={t("msg.pinned")}>
      <Pin size={12} aria-hidden />
    </span>
  );
}

export function PinnedList(props: { sessionId: string; pins: PinInfo[]; canUnpin: boolean }) {
  const [open, setOpen] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!open) return;
    const down = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      setOpen(false);
      opener.current?.focus();
    };
    document.addEventListener("pointerdown", down);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", down);
      document.removeEventListener("keydown", key);
    };
  }, [open]);
  const n = props.pins.length;
  // The last pin went (unpinned here): the list closes with it.
  useEffect(() => {
    if (n === 0) setOpen(false);
  }, [n]);
  if (n === 0) return null;
  const unpin = async (seq: number) => {
    setErr(null);
    try {
      await pageApi.pinMessage(props.sessionId, seq, false);
    } catch (x) {
      setErr(errText(x));
    }
  };
  return (
    <div ref={root} className="pinned-wrap">
      <button
        ref={opener}
        type="button"
        className="link small pinned-btn"
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label={t("pins.buttonLabel", { n: String(n) })}
        onClick={() => setOpen(!open)}
      >
        <Pin size={14} aria-hidden /> {t("pins.button", { n: String(n) })}
      </button>
      {open && (
        <div role="dialog" aria-label={t("pins.title")} className="pinned-pop small">
          <ul>
            {props.pins.map((p) => (
              <li key={p.seq}>
                <button
                  type="button"
                  className="pinned-go"
                  title={t("pins.jump")}
                  onClick={() => {
                    if (jumpTo(p.seq)) setOpen(false);
                  }}
                >
                  <span className="pinned-text">{p.text || "…"}</span>
                  <span className="pinned-meta muted">
                    {p.who === "user" ? t("pins.you") : t("pins.assistant")} {"·"} <RelTime at={p.at} />
                  </span>
                </button>
                {props.canUnpin && (
                  <button type="button" className="link small" onClick={() => void unpin(p.seq)}>
                    {t("pins.unpin")}
                  </button>
                )}
              </li>
            ))}
          </ul>
          {err && (
            <div className="error-text" role="alert">
              {err}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
