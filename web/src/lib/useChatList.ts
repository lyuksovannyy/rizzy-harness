// The workspace's chat list, a page at a time (docs/store-readers.md §4 and
// §5 slice 3): the first page on mount, the next as the sidebar's end
// scrolls into view, and each open project group's own newest chats (a
// closed group costs nothing). A `sessions_changed` nudge refetches from the
// top, as many rows as are loaded (at most 200), one request in flight and
// one queued however many nudges come; the merge is `chatPages.ts`.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type { SessionRow } from "../types";
import { appendPage, applyRefetch, coalesce, combine, type List, type Page, refetchLimit } from "./chatPages";
import { pruneDrafts } from "./drafts";

/** The main list's page (the server's default). */
const FIRST = 50;
/** A project group's page: the group shows five before "show more". */
const GROUP = 10;

/** What the sidebar needs to load more. */
export interface ChatPages {
  /** Loads the main list's next page; null when there is none. */
  more: (() => void) | null;
  busy: boolean;
  /** The last load failed: load again on a click only, never by scrolling
   * (a failing page in view would otherwise be asked for in a loop). */
  stalled: boolean;
  /** An open project group (or the open project's page) wants its own
   * newest chats; the returned function lets go. */
  watch(project: string): () => void;
  group(project: string): List | undefined;
  moreIn(project: string): void;
}

type Got = { sessions: SessionRow[]; children?: SessionRow[]; next?: string | null };
const norm = (r: Got): Page => ({ sessions: r.sessions, children: r.children ?? [], next: r.next ?? null });

export function useChatList(
  workspaceId: string | null,
  onError: (e: unknown) => void,
  /** `keep`: the project whose page is open; `extra`: the open chat's own
   * row from its lookup, shown even when no loaded page holds it. */
  opts: { keep: string | null; extra: SessionRow | null },
) {
  const [main, setMain] = useState<List | null>(null);
  const [groups, setGroups] = useState<Record<string, List>>({});
  const [busy, setBusy] = useState(false);
  const [stalled, setStalled] = useState(false);
  // The latest lists, for callbacks that outlive a render.
  const now = useRef({ main, groups });
  useEffect(() => {
    now.current = { main, groups };
  });
  const watched = useRef(new Map<string, number>());
  const err = useRef(onError);
  useEffect(() => {
    err.current = onError;
  });
  const pruned = useRef(false);
  const loading = useRef(false);

  const get = useCallback(
    (o: { limit?: number; after?: string; project?: string }) => api.sessions(workspaceId, o).then(norm),
    [workspaceId],
  );
  const putGroup = useCallback((id: string, f: (l: List | undefined) => List) => {
    setGroups((g) => (watched.current.has(id) ? { ...g, [id]: f(g[id]) } : g));
  }, []);

  const refresh = useMemo(
    () =>
      coalesce(() => {
        const { main, groups } = now.current;
        const limit = refetchLimit(main?.rows.length ?? 0, FIRST);
        const top = get(limit === FIRST ? {} : { limit }).then((p) => {
          setMain((m) => (m ? applyRefetch(m, p) : appendPage(null, p)));
          // Once: every refresh would ask about each draft beyond page one.
          if (!pruned.current) {
            pruned.current = true;
            pruneDrafts(p.sessions);
          }
        });
        // Open groups whose first page came; the others are still on their way.
        const open = [...watched.current.keys()].filter((id) => groups[id]);
        const each = open.map((id) =>
          get({ project: id, limit: refetchLimit(groups[id]!.rows.length, GROUP) }).then((p) =>
            putGroup(id, (l) => (l ? applyRefetch(l, p) : appendPage(null, p))),
          ),
        );
        return Promise.all([top, ...each]).catch((e: unknown) => err.current(e));
      }),
    [get, putGroup],
  );

  const more = useCallback(() => {
    const next = now.current.main?.next;
    if (!next || loading.current) return;
    loading.current = true;
    setBusy(true);
    get({ after: next })
      .then((p) => {
        setMain((m) => (m ? appendPage(m, p) : m));
        setStalled(false);
      })
      .catch((e: unknown) => {
        setStalled(true);
        err.current(e);
      })
      .finally(() => {
        loading.current = false;
        setBusy(false);
      });
  }, [get]);

  const watch = useCallback(
    (id: string) => {
      const n = watched.current.get(id) ?? 0;
      watched.current.set(id, n + 1);
      if (n === 0)
        get({ project: id, limit: GROUP })
          .then((p) => putGroup(id, () => appendPage(null, p)))
          // A project deleted meanwhile is a 404: the group goes with the next list.
          .catch((e: unknown) => err.current(e));
      return () => {
        const k = (watched.current.get(id) ?? 1) - 1;
        if (k > 0) return void watched.current.set(id, k);
        // Closed: its rows go, so nothing shows stale marks for it.
        watched.current.delete(id);
        setGroups(({ [id]: _, ...rest }) => rest);
      };
    },
    [get, putGroup],
  );

  const moreIn = useCallback(
    (id: string) => {
      const next = now.current.groups[id]?.next;
      if (next)
        get({ project: id, after: next, limit: GROUP })
          .then((p) => putGroup(id, (l) => appendPage(l ?? null, p)))
          .catch((e: unknown) => err.current(e));
    },
    [get, putGroup],
  );

  useEffect(() => (opts.keep ? watch(opts.keep) : undefined), [opts.keep, watch]);

  const extra = opts.extra && (opts.extra.workspace_id ?? null) === workspaceId ? opts.extra : null;
  const sessions = useMemo(() => (main ? combine([main, ...Object.values(groups)], extra) : null), [main, groups, extra]);
  const pages: ChatPages = useMemo(
    () => ({ more: main?.next ? more : null, busy, stalled, watch, group: (id: string) => groups[id], moreIn }),
    [main?.next, more, busy, stalled, watch, groups, moreIn],
  );
  return { sessions, refresh, pages };
}
