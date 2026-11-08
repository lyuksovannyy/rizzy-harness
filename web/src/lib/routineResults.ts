// Routine results in the first load (docs/routines.md): what a row of
// attention's `routines` says, where it leads, the browser notification for a
// new one, and the seen mark while a run's chat is on screen. Shell calls the
// hook on every render; Attention draws the rows. Kept apart from
// lib/routines.ts, which is the Routines page's own (lazy) code.
import { useEffect, useRef } from "react";
import { call } from "../api";
import type { RoutineAttention } from "../types";
import { notifyEnabled } from "./notify";
import { type Route, routeHash, routineHash } from "./routes";

/** Finished runs this page already marked: the attention row and the open
 * chat both mark the same run, and a run stays seen. */
const marked = new Set<string>();

/** The finished run is seen by its owner (idempotent on the server). */
export async function markRunSeen(routineId: string, runId: string): Promise<void> {
  if (marked.has(runId)) return;
  await call<{ ok: boolean }>("POST", `/api/routines/${routineId}/runs/${runId}/seen`);
  marked.add(runId);
}

/** "Finished", "Failed" or "Paused", as the row and the notification say it. */
export function resultLabel(r: Pick<RoutineAttention, "kind">): string {
  return r.kind === "paused" ? "Paused" : r.kind === "failed" ? "Failed" : "Finished";
}

/** Where a row leads: a result opens its run's chat, or the Runs tab when the
 * run has none (a run blocked before it started); a pause opens the routine. */
export function resultHash(r: Pick<RoutineAttention, "kind" | "routine_id" | "session_id">): string {
  if (r.kind === "paused") return routineHash(r.routine_id);
  return r.session_id ? routeHash({ kind: "session", id: r.session_id }) : routineHash(r.routine_id, "runs");
}

/** Opens a row; a run without a chat has nothing else that marks it seen. */
export function openResult(r: RoutineAttention): void {
  if (r.kind !== "paused" && !r.session_id && r.run_id) void markRunSeen(r.routine_id, r.run_id).catch(() => {});
  location.hash = resultHash(r);
}

/** Whether a new row notifies, as the routine's `notify` says. */
export function notifies(r: Pick<RoutineAttention, "kind" | "notify">): boolean {
  return r.notify === "always" || (r.notify === "failures" && r.kind !== "result");
}

/** A row's identity: a newer run of the same routine is a new result. */
const rowKey = (r: RoutineAttention) => `${r.kind}:${r.routine_id}:${r.run_id ?? ""}`;

const viewing = (sessionId: string | null) =>
  !!sessionId && location.hash === `#/s/${sessionId}` && document.visibilityState === "visible";

function notify(r: RoutineAttention) {
  const body = r.kind === "result" ? "Finished" : `${resultLabel(r)}${r.reason ? `: ${r.reason}` : ""}`;
  const n = new Notification(r.name, { body, tag: `routine:${r.routine_id}` });
  n.onclick = () => {
    window.focus();
    openResult(r);
    n.close();
  };
}

export function useRoutineResults(opts: { route: Route; routines: RoutineAttention[]; workspaceId: string | null }): void {
  const { route, routines } = opts;
  // Shell starts with one empty list and replaces it with each answer, so
  // the first list that is not that one is the first load: what it holds is
  // not news. Rows in every workspace notify, like waiting chats do.
  const initial = useRef(routines);
  const known = useRef<Set<string> | null>(null);
  useEffect(() => {
    if (routines === initial.current && known.current === null) return;
    const before = known.current;
    known.current = new Set(routines.map(rowKey));
    if (!before || !notifyEnabled()) return;
    for (const r of routines) {
      if (!before.has(rowKey(r)) && notifies(r) && !viewing(r.session_id)) notify(r);
    }
  }, [routines]);

  // A finished run's chat on screen, in a visible page, is seen.
  const open = route.kind === "session" ? route.id : null;
  const row = open ? routines.find((r) => r.kind !== "paused" && r.session_id === open && r.run_id) : undefined;
  const routineId = row?.routine_id;
  const runId = row?.run_id;
  useEffect(() => {
    if (!routineId || !runId) return;
    const mark = () => {
      if (document.visibilityState === "visible") void markRunSeen(routineId, runId).catch(() => {});
    };
    mark();
    document.addEventListener("visibilitychange", mark);
    return () => document.removeEventListener("visibilitychange", mark);
  }, [routineId, runId]);
}
