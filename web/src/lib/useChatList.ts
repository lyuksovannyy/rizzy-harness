// The workspace's chat list, a page at a time (docs/store-readers.md §4 and
// §5 slice 3): the first page on mount, the next as the sidebar's end
// scrolls into view, and each open project group's own newest chats (a
// closed group costs nothing). A `sessions_changed` nudge refetches from the
// top, as many rows as are loaded (at most 200), one request in flight and
// one queued however many nudges come; the merge is `chatPages.ts`.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, HttpError } from "../api";
import type { SessionRow } from "../types";
import { appendPage, applyRefetch, coalesce, combine, type List, refetchLimit } from "./chatPages";
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
  /** How many older chats the last page added (`at` tells two equal ones
   * apart), for the sidebar's polite status line. */
  loaded: { n: number; at: number } | null;
  /** Pages asked for by `more` that have landed, or failed: a click's wait
   * ends when this moves, whatever else changes the rows meanwhile. */
  landed: number;
  /** The same for one group's `moreIn`. */
  landedIn(project: string): number;
  /** An open project group (or the open project's page) wants its own
   * newest chats; the returned function lets go. */
  watch(project: string): () => void;
  group(project: string): List | undefined;
  moreIn(project: string): void;
}


/** The open chat's row to show: its refreshed copy; none once it was
 * deleted or moved to another workspace. */
function shownRow(extra: SessionRow | null, fresh: SessionRow | { gone: string } | null, ws: string | null) {
  if (!extra || !fresh) return extra;
  if ("gone" in fresh) return fresh.gone === extra.session_id ? null : extra;
  if (fresh.session_id !== extra.session_id) return extra;
  return (fresh.workspace_id ?? null) === ws ? fresh : null;
}

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
  const [loaded, setLoaded] = useState<{ n: number; at: number } | null>(null);
  const groupBusy = useRef(new Set<string>());
  const [landed, setLanded] = useState(0);
  const [groupLanded, setGroupLanded] = useState<Record<string, number>>({});
  // Groups whose first page is on its way when a nudge comes: refetched once
  // it lands, so the nudge is not lost for them.
  const nudged = useRef(new Set<string>());
  // The open chat's row as refreshed here, when no loaded page holds it;
  // `gone`: the server said 404 for it (deleted elsewhere).
  const [fresh, setFresh] = useState<SessionRow | { gone: string } | null>(null);
  const extra = opts.extra && (opts.extra.workspace_id ?? null) === workspaceId ? opts.extra : null;
  // The latest lists, for callbacks that outlive a render.
  const now = useRef({ main, groups, extra });
  useEffect(() => {
    now.current = { main, groups, extra };
  });
  const watched = useRef(new Map<string, number>());
  const err = useRef(onError);
  useEffect(() => {
    err.current = onError;
  });
  const pruned = useRef(false);
  const loading = useRef(false);

  const get = useCallback(
    (o: { limit?: number; after?: string; project?: string }) => api.sessions(workspaceId, o),
    [workspaceId],
  );
  const putGroup = useCallback((id: string, f: (l: List | undefined) => List) => {
    setGroups((g) => (watched.current.has(id) ? { ...g, [id]: f(g[id]) } : g));
  }, []);

  const refetchGroup = useCallback(
    (id: string, loaded: number) =>
      get({ project: id, limit: refetchLimit(loaded, GROUP) }).then((p) =>
        putGroup(id, (l) => (l ? applyRefetch(l, p) : appendPage(null, p))),
      ),
    [get, putGroup],
  );

  const refresh = useMemo(
    () =>
      coalesce(() => {
        const { main, groups, extra } = now.current;
        const limit = refetchLimit(main?.rows.length ?? 0, FIRST);
        const top = get(limit === FIRST ? {} : { limit }).then((p) => {
          setMain((m) => (m ? applyRefetch(m, p) : appendPage(null, p)));
          // Once: every refresh would ask about each draft beyond page one.
          if (!pruned.current) {
            pruned.current = true;
            pruneDrafts(p.sessions);
          }
        });
        // Open groups whose first page came; the others are still on their
        // way and refetch once it lands.
        const open = [...watched.current.keys()];
        for (const id of open) if (!groups[id]) nudged.current.add(id);
        const each = open.filter((id) => groups[id]).map((id) => refetchGroup(id, groups[id]!.rows.length));
        // The open chat outside every loaded page: its row again (a rename or
        // a move does not bring it to the top). A 404, and only that, means it
        // is gone; a network error keeps what is shown.
        const held = (id: string) => [main, ...Object.values(groups)].some((l) => l?.rows.some((r) => r.session_id === id) || l?.children.some((r) => r.session_id === id));
        const own =
          extra && main && !held(extra.session_id)
            ? api.session(extra.session_id).then(
                (r) => setFresh(r),
                (e: unknown) => e instanceof HttpError && e.status === 404 && setFresh({ gone: extra.session_id }),
              )
            : null;
        return Promise.all([top, ...each, own]).catch((e: unknown) => err.current(e));
      }),
    [get, refetchGroup],
  );

  const more = useCallback(() => {
    const next = now.current.main?.next;
    if (!next || loading.current) return;
    loading.current = true;
    setBusy(true);
    get({ after: next })
      .then((p) => {
        const had = new Set(now.current.main?.rows.map((r) => r.session_id));
        setLoaded({ n: p.sessions.filter((r) => !had.has(r.session_id)).length, at: Date.now() });
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
        setLanded((n) => n + 1);
      });
  }, [get]);

  const watch = useCallback(
    (id: string) => {
      const n = watched.current.get(id) ?? 0;
      watched.current.set(id, n + 1);
      if (n === 0)
        get({ project: id, limit: GROUP })
          .then((p) => {
            putGroup(id, () => appendPage(null, p));
            if (nudged.current.delete(id) && watched.current.has(id)) return refetchGroup(id, p.sessions.length);
          })
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
    [get, putGroup, refetchGroup],
  );

  const moreIn = useCallback(
    (id: string) => {
      const next = now.current.groups[id]?.next;
      // A click while that group's page loads is ignored.
      if (!next || groupBusy.current.has(id)) return;
      groupBusy.current.add(id);
      get({ project: id, after: next, limit: GROUP })
        .then((p) => putGroup(id, (l) => appendPage(l ?? null, p)))
        .catch((e: unknown) => err.current(e))
        .finally(() => {
          groupBusy.current.delete(id);
          setGroupLanded((g) => ({ ...g, [id]: (g[id] ?? 0) + 1 }));
        });
    },
    [get, putGroup],
  );

  useEffect(() => (opts.keep ? watch(opts.keep) : undefined), [opts.keep, watch]);

  const shown = shownRow(extra, fresh, workspaceId);
  const sessions = useMemo(() => (main ? combine([main, ...Object.values(groups)], shown) : null), [main, groups, shown]);
  const pages: ChatPages = useMemo(
    () => ({
      more: main?.next ? more : null,
      busy,
      stalled,
      loaded,
      landed,
      landedIn: (id: string) => groupLanded[id] ?? 0,
      watch,
      group: (id: string) => groups[id],
      moreIn,
    }),
    [main?.next, more, busy, stalled, loaded, landed, groupLanded, watch, groups, moreIn],
  );
  return { sessions, refresh, pages };
}
