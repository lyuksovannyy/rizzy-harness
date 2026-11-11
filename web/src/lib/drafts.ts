// Unsent drafts (AUDIT L14): the reply box's text in localStorage under
// `harness.draft.<user id>.<chat id>`, and the answers typed into a
// question or form card in sessionStorage (`harness.ui.…`, ui/Input.tsx).
// Both hold chat content, so: sign-out clears every one of them in this
// browser (App), whoever's; a chat's draft goes with the chat, deleted
// here (DeleteChat) or elsewhere (pruneDrafts); an incognito chat never
// has one. Storage can be off, full or blocked: nothing here throws.
import { api, HttpError } from "../api";

const DRAFT = "harness.draft.";
let owner = "-";

/** Whose drafts are read and written: the signed-in user (the API token: "-"). */
export function setDraftOwner(id: string | null) {
  owner = id ?? "-";
}

const key = (chat: string) => `${DRAFT}${owner}.${chat}`;

export function readDraft(chat: string): string {
  try {
    return localStorage.getItem(key(chat)) ?? "";
  } catch {
    return "";
  }
}

/** Empty text removes the draft. */
export function writeDraft(chat: string, text: string) {
  try {
    if (text) localStorage.setItem(key(chat), text);
    else localStorage.removeItem(key(chat));
  } catch {
    /* not kept */
  }
}

export const dropDraft = (chat: string) => writeDraft(chat, "");

/** The keys of `store` that start with `prefix` (none if it cannot be read). */
function keysOf(store: Storage, prefix: string): string[] {
  const found: string[] = [];
  try {
    for (let i = 0; i < store.length; i++) {
      const k = store.key(i);
      if (k?.startsWith(prefix)) found.push(k);
    }
  } catch {
    /* unreadable */
  }
  return found;
}

/** Sign-out: every draft and card answer in this browser, any user's, and
 * the other things one user leaves that are theirs rather than a display
 * preference: recent folders (Composer), routine test events
 * (RoutineEditor), Media picked for the next chat (lib/media). Theme,
 * accent, backdrop, notifications and sidebar layout stay. */
export function clearDrafts() {
  const gone: [() => Storage, string][] = [
    [() => localStorage, DRAFT],
    [() => localStorage, "harness.folders"],
    [() => localStorage, "routine-sample:"],
    [() => sessionStorage, "harness.ui."],
    [() => sessionStorage, "harness.pendingImages"],
  ];
  for (const [store, prefix] of gone)
    try {
      const s = store();
      for (const k of keysOf(s, prefix)) s.removeItem(k);
    } catch {
      /* storage gone */
    }
}

/** After the chat list loads: drop the drafts of incognito chats, and of
 * chats the server no longer has (deleted in another tab or on another
 * device: 404). A chat not listed may be in another workspace, so each is
 * asked about; an error other than 404 keeps the draft. */
export function pruneDrafts(listed: { session_id: string; incognito?: boolean }[]) {
  const here = new Set<string>();
  for (const s of listed) {
    if (s.incognito) dropDraft(s.session_id);
    else here.add(s.session_id);
  }
  const mine = key("");
  for (const k of keysOf(localStorage, mine)) {
    const chat = k.slice(mine.length);
    if (here.has(chat)) continue;
    api.session(chat).then(
      (s) => s.incognito && dropDraft(chat),
      (e) => e instanceof HttpError && e.status === 404 && dropDraft(chat),
    );
  }
}
