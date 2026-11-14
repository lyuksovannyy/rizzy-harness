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

/** Unread chats in `now` that were not unread in `before`. Pure, for tests. */
export function newlyFinished(before: ReadonlySet<string>, now: SessionRow[]): SessionRow[] {
  return now.filter((s) => s.unread && !s.running && !before.has(s.session_id));
}

const NONE: SessionRow[] = [];

const unreadIds = (rows: SessionRow[]) => new Set(rows.filter((s) => s.unread && !s.running).map((s) => s.session_id));

const looking = (id: string, viewing: string | null) => viewing === id && document.visibilityState === "visible";

/** `listed`: the chats in the sidebar's order (its numbers are theirs);
 * null until the first list has loaded. */
export function useFinishedSignal(loaded: SessionRow[] | null, viewing: string | null) {
  const listed = loaded ?? NONE;
  // Null until the first list: what was unread before this tab opened is
  // shown by its dot, never notified.
  const before = useRef<Set<string> | null>(null);
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
    before.current = unreadIds(listed);
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
