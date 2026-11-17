// Messages sent from this tab that the chat's log does not hold yet: shown
// at once on Send, as the person's bubble marked "Sending…" (until the
// server took it), and gone the moment the fold has its `user_message`
// (`unsent`). A view-only overlay in this tab's memory, never a second
// record: a reload or another tab shows what the log holds.
//
// Matched by its words at or after its place among the chat's person
// messages, so a message another tab sent meanwhile does not hide it. Once
// the server took it, it also goes when more person messages landed after
// its place than this tab has waiting: then one of them is it, whatever its
// words became. No duplicate row, no lost one, none that stays.
import type { Submission } from "../components/Composer";

export interface Outgoing {
  id: number;
  text: string;
  /** Its place among the chat's person messages, once the log has it. */
  at: number;
  /** When it was sent (this browser's clock). */
  sent: number;
  /** The server took it (the POST answered yes). */
  ok?: boolean;
  /** Why the server refused it, when no reply box was there to take it back
   * (a new chat's first message): the bubble stays, with Retry and Edit. */
  failed?: string;
  /** What was sent, for Retry and Edit (its attachments and images too). */
  s?: Submission;
}

let out = new Map<string, Outgoing[]>();
const subs = new Set<() => void>();
let version = 0;
let ids = 0;
const tell = () => {
  version++;
  subs.forEach((f) => f());
};
const live = (chat: string) => (out.get(chat) ?? []).filter((o) => !o.failed);

/** The entries of `list` the log's person messages (`users`, their words)
 * do not hold yet. */
function waiting(list: Outgoing[], users: string[]): Outgoing[] {
  const used = new Set<number>();
  const pending = list.filter((o) => !o.failed).length;
  return list.filter((o) => {
    if (o.failed) return true;
    const i = users.findIndex((t, j) => j >= o.at && !used.has(j) && t === o.text);
    if (i >= 0) {
      used.add(i);
      return false;
    }
    return !(o.ok && users.length - o.at > pending);
  });
}

export const outbox = {
  /** `logged`: the person messages the chat's log holds now. A refused
   * message sent again (its words in this one) goes. */
  add(chat: string, text: string, logged: number, s?: Submission): Outgoing {
    const mine = live(chat);
    const kept = (out.get(chat) ?? []).filter((o) => o.failed && !text.includes(o.text));
    const o: Outgoing = { id: ++ids, text, at: Math.max(logged, ...mine.map((x) => x.at + 1)), sent: Date.now(), s };
    out.set(chat, [...kept, ...mine, o]);
    tell();
    return o;
  },
  /** The server took it: no longer "Sending…". */
  accept(o: Outgoing) {
    o.ok = true;
    tell();
  },
  /** Refused: taken away, or kept marked with why (`failed`). The ones sent
   * after it move up a place. */
  drop(chat: string, o: Outgoing, failed?: string) {
    // In place: their senders hold them, to accept or drop them in turn.
    const rest = (out.get(chat) ?? []).filter((x) => x !== o);
    for (const x of rest) if (!x.failed && x.at > o.at) x.at--;
    out.set(chat, failed ? [...rest, { ...o, failed }] : rest);
    tell();
  },
  /** Changes on every add, accept and drop (for useSyncExternalStore). */
  version: () => version,
  subscribe(f: () => void) {
    subs.add(f);
    return () => void subs.delete(f);
  },
  /** What `chat` still shows beside a log whose person messages are
   * `users`. Reads only. */
  unsent: (chat: string, users: string[]): Outgoing[] => waiting(out.get(chat) ?? [], users),
  /** Forget those the log holds (after a render, not during one; nothing
   * shown changes). */
  prune(chat: string, users: string[]) {
    const list = out.get(chat);
    if (list) out.set(chat, waiting(list, users));
  },
  /** A refused one the person took into the box (Edit) or sent again (Retry). */
  forget(chat: string, o: Outgoing) {
    out.set(chat, (out.get(chat) ?? []).filter((x) => x !== o));
    tell();
  },
  /** For tests: forget everything. */
  reset() {
    out = new Map();
  },
};
