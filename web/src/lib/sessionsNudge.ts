// The chat list changed somewhere (the server's content-free
// `sessions_changed`, a reconnect, a chat started here), for screens that
// keep a list of chats of their own: the project page's "Add a chat". The
// Shell calls `nudgeSessions` whenever it refetches its own list.
const listeners = new Set<() => void>();

export function nudgeSessions(): void {
  for (const f of listeners) f();
}

/** Call `f` on every nudge; returns the unsubscribe. */
export function onSessionsNudge(f: () => void): () => void {
  listeners.add(f);
  return () => {
    listeners.delete(f);
  };
}
