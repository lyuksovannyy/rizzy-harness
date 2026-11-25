// The chat list one page at a time (docs/store-readers.md §4 "Live
// updates"): the pure part of `useChatList`. A list is the pages loaded so
// far, newest first, the sub-agent chats below them, and where the next page
// starts. A refetch from the top replaces the rows it covers; rows loaded
// past it stay until scrolled again.
import type { SessionRow } from "../types";

/** One answer of `GET /api/sessions`. */
export interface Page {
  sessions: SessionRow[];
  children: SessionRow[];
  next: string | null;
}

export interface List {
  rows: SessionRow[];
  children: SessionRow[];
  next: string | null;
}

/** The server's largest page. */
export const MAX_PAGE = 200;

/** `a` comes after `b` in the server's order (`updated_ms`, then the id,
 * both descending; ids are lower-case hex, so text order is the server's). */
export function older(a: SessionRow, b: SessionRow): boolean {
  return a.updated_ms < b.updated_ms || (a.updated_ms === b.updated_ms && a.session_id < b.session_id);
}

const ids = (rows: SessionRow[]) => new Set(rows.map((r) => r.session_id));

/** How many rows a refetch from the top asks for: as many as are loaded,
 * at least a first page, at most the server's largest page. */
export function refetchLimit(loaded: number, first: number): number {
  return Math.min(Math.max(loaded, first), MAX_PAGE);
}

/** The first page, or the next one after scrolling. A row the new page
 * lists again (its time went down meanwhile) leaves its old place. */
export function appendPage(l: List | null, p: Page): List {
  if (!l) return { rows: p.sessions, children: p.children, next: p.next };
  const rows = ids(p.sessions);
  const kids = ids(p.children);
  return {
    rows: [...l.rows.filter((r) => !rows.has(r.session_id)), ...p.sessions],
    children: [...l.children.filter((r) => !kids.has(r.session_id)), ...p.children],
    next: p.next,
  };
}

/** The top-level chat a sub-agent chat hangs from, through `children`. */
function rootOf(r: SessionRow, byId: Map<string, SessionRow>): string | null {
  const seen = new Set<string>();
  let at = r;
  while (at.parent_id && !seen.has(at.session_id)) {
    seen.add(at.session_id);
    const up = byId.get(at.parent_id);
    if (!up) return at.parent_id;
    at = up;
  }
  return null;
}

/** A refetch from the top (`p`) over what was loaded (`l`). The rows it
 * covers are replaced: a chat that moved up shows in its new place, one
 * deleted or moved out is gone. Rows loaded past its last row stay, with
 * their sub-agents and the old `next`, until scrolled again. */
export function applyRefetch(l: List | null, p: Page): List {
  if (!l || p.next === null) return { rows: p.sessions, children: p.children, next: p.next };
  const last = p.sessions[p.sessions.length - 1];
  const fresh = ids(p.sessions);
  const tail = last ? l.rows.filter((r) => !fresh.has(r.session_id) && older(r, last)) : [];
  if (!tail.length) return { rows: p.sessions, children: p.children, next: p.next };
  const kept = ids(tail);
  const freshKids = ids(p.children);
  const byId = new Map(l.children.map((r) => [r.session_id, r]));
  const oldKids = l.children.filter((r) => !freshKids.has(r.session_id) && kept.has(rootOf(r, byId) ?? ""));
  return { rows: [...p.sessions, ...tail], children: [...p.children, ...oldKids], next: l.next };
}

/** A project group's rows, from every list that holds them, newest first.
 * While the group's own pages have more, rows older than its last loaded
 * one wait for its next page, so the group never shows a gap that a page
 * later fills in the middle. */
export function groupRows(rows: SessionRow[], own: List | undefined): SessionRow[] {
  const last = own?.next ? own.rows[own.rows.length - 1] : undefined;
  return last ? rows.filter((r) => !older(r, last)) : rows;
}

/** Every listed row once (the freshest copy: the highest `head_seq`), and
 * `extra` (the open chat's own row, when no page holds it), newest first. */
export function combine(lists: List[], extra: SessionRow | null): SessionRow[] {
  const by = new Map<string, SessionRow>();
  const put = (r: SessionRow) => {
    const had = by.get(r.session_id);
    if (!had || r.head_seq > had.head_seq) by.set(r.session_id, r);
  };
  for (const l of lists) {
    l.rows.forEach(put);
    l.children.forEach(put);
  }
  if (extra && !by.has(extra.session_id)) by.set(extra.session_id, extra);
  return [...by.values()].sort((a, b) => (older(a, b) ? 1 : older(b, a) ? -1 : 0));
}

/** `run`, at most one at a time: calls while one runs make exactly one
 * more run after it (a burst of `sessions_changed` is one refetch, then one
 * for whatever changed during it). */
export function coalesce(run: () => Promise<unknown>): () => void {
  let busy = false;
  let again = false;
  const go = () => {
    if (busy) {
      again = true;
      return;
    }
    busy = true;
    void run()
      .catch(() => {})
      .finally(() => {
        busy = false;
        if (again) {
          again = false;
          go();
        }
      });
  };
  return go;
}
