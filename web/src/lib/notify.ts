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
