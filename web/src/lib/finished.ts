// "Finished, unread": the server marks a chat whose last turn completed or
// failed after its owner last looked at it (`SessionRow.unread`; not a
// sub-agent's chat, a callback turn or a question, which have their own
// signals). Here the shell tells the person once per newly finished chat
// they are not looking at, and looking at one clears it (the seen marker).
// The notification carries fixed words and the chat's place in the list:
// never its title, a command or a path (ADOPTION_PLAN W.6), since it shows
// on a lock screen.
import { useEffect, useRef } from "react";
import { api } from "../api";
import type { SessionRow } from "../types";
import { APP_TITLE } from "./brand";
import { t } from "./i18n";
import { notifyEnabled } from "./notify";

/** Unread chats in `now` that this tab saw before and saw not unread then.
 * `before`: every chat seen so far, and whether it was finished and unread
 * when last seen. A chat seen for the first time already unread (a page
 * loaded by scrolling, a project group opened) finished before it came into
 * view: its dot says so, a notification would not be news. Pure, for tests. */
export function newlyFinished(before: ReadonlyMap<string, boolean>, now: SessionRow[]): SessionRow[] {
  return now.filter((s) => done(s) && before.get(s.session_id) === false);
}

const NONE: SessionRow[] = [];

const done = (s: SessionRow) => !!s.unread && !s.running;

const looking = (id: string, viewing: string | null) => viewing === id && document.visibilityState === "visible";

/** `listed`: the chats in the sidebar's order (its numbers are theirs);
 * null until the first list has loaded. */
export function useFinishedSignal(loaded: SessionRow[] | null, viewing: string | null) {
  const listed = loaded ?? NONE;
  // Null until the first list: what was unread before this tab opened is
  // shown by its dot, never notified.
  const before = useRef<Map<string, boolean> | null>(null);
  useEffect(() => {
    if (!loaded) return;
    if (before.current && notifyEnabled()) {
      for (const s of newlyFinished(before.current, listed)) {
        if (looking(s.session_id, viewing)) continue;
        const id = s.session_id;
        const n = new Notification(APP_TITLE, {
          body: t("notify.finished", { n: String(listed.indexOf(s) + 1) }),
          tag: `finished-${id}`,
        });
        n.onclick = () => {
          window.focus();
          location.hash = `#/s/${id}`;
          n.close();
        };
      }
    }
    // Chats that leave the list (a folded group) keep what was last seen of
    // them, so coming back is no news either.
    const seen = before.current ?? new Map<string, boolean>();
    for (const s of listed) seen.set(s.session_id, done(s));
    before.current = seen;
  }, [loaded, listed, viewing]);

  // Looking at a finished chat counts as seeing it, now or when the tab
  // comes back into view.
  const row = listed.find((s) => s.session_id === viewing && s.unread && !s.running);
  const seq = row?.head_seq;
  useEffect(() => {
    if (!viewing || seq === undefined) return;
    const mark = () => {
      if (document.visibilityState === "visible") void api.markSeen(viewing, seq).catch(() => {});
    };
    mark();
    document.addEventListener("visibilitychange", mark);
    return () => document.removeEventListener("visibilitychange", mark);
  }, [viewing, seq]);
}
