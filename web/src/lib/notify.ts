// Browser notifications for chats that need the user. They only work while
// a tab is open (no push server), and only on secure pages (localhost or
// HTTPS), which is why the server has --tls.
const KEY = "harness.notify";

export function notifySupported(): boolean {
  return typeof window !== "undefined" && "Notification" in window && window.isSecureContext;
}

export function notifyEnabled(): boolean {
  try {
    return notifySupported() && Notification.permission === "granted" && localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

/** Ask permission (must run from a click) and remember the choice. */
export async function enableNotify(on: boolean): Promise<boolean> {
  try {
    if (!on) {
      localStorage.setItem(KEY, "0");
      return false;
    }
    if (!notifySupported()) return false;
    const p = Notification.permission === "default" ? await Notification.requestPermission() : Notification.permission;
    localStorage.setItem(KEY, p === "granted" ? "1" : "0");
    return p === "granted";
  } catch {
    return false;
  }
}

/** Ids that are new in `now` compared to `before`. Pure, for tests. */
export function newlyWaiting(before: string[], now: string[]): string[] {
  const b = new Set(before);
  return now.filter((id) => !b.has(id));
}

/** A notification's words for a chat that waits (row W.6): fixed words and
 * the chat's place in the list of waiting chats (1-based), never its title,
 * a command or a path: the lock screen and the notification log keep them. */
export function waitingWords(reason: string, place: number, of: number): string {
  const what = reason === "approval" ? "A chat needs your approval" : "A chat asked you a question";
  return of > 1 ? `${what} (${place} of ${of} waiting)` : what;
}
