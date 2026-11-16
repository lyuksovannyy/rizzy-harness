// Messages sent from this tab that the chat's log does not hold yet: shown
// at once on Send, as the person's bubble marked "Sending…", and gone the
// moment the fold has its `user_message` (`unsent`). A view-only overlay in
// this tab's memory, never a second record: a reload or another tab shows
// what the log holds.
//
// Matched by place, not by text: a message is the chat's `at`-th person
// message (from 0), so one the log holds under other words (a mention the
// node rewrote, whitespace) still replaces its bubble, and the bubble can
// never stay beside it. A message from elsewhere landing first only hides a
// bubble early; its own message then shows from the log. No duplicate row,
// no lost one.

export interface Outgoing {
  text: string;
  /** Its place among the chat's person messages, once the log has it. */
  at: number;
  /** When it was sent (this browser's clock). */
  sent: number;
  /** Why the server refused it; the text went back to the reply box. */
  failed?: string;
}

const out = new Map<string, Outgoing[]>();
const subs = new Set<() => void>();
let version = 0;
const tell = () => {
  version++;
  subs.forEach((f) => f());
};
const live = (chat: string) => (out.get(chat) ?? []).filter((o) => !o.failed);

export const outbox = {
  /** `logged`: the person messages the chat's log holds now. */
  add(chat: string, text: string, logged: number): Outgoing {
    const mine = live(chat);
    const o = { text, at: Math.max(logged, ...mine.map((x) => x.at + 1)), sent: Date.now() };
    out.set(chat, [...mine, o]);
    tell();
    return o;
  },
  /** Refused: taken away, or kept marked with why (`failed`, for a chat this
   * send opened, where no reply box was there to take the text back). The
   * ones sent after it move up a place. */
  drop(chat: string, o: Outgoing, failed?: string) {
    // In place: their senders hold them, to drop them in turn.
    const rest = live(chat).filter((x) => x !== o);
    for (const x of rest) if (x.at > o.at) x.at--;
    out.set(chat, failed ? [...rest, { ...o, failed }] : rest);
    tell();
  },
  /** Changes on every add and drop (for useSyncExternalStore). */
  version: () => version,
  subscribe(f: () => void) {
    subs.add(f);
    return () => void subs.delete(f);
  },
  /** What `chat` still shows beside a log holding `logged` person messages;
   * the rest are forgotten (no update: they were hidden already). */
  unsent(chat: string, logged: number): Outgoing[] {
    const left = (out.get(chat) ?? []).filter((o) => o.failed || o.at >= logged);
    if (left.length) out.set(chat, left);
    else out.delete(chat);
    return left;
  },
};
