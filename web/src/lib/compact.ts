// A person's /compact (ADOPTION_PLAN X.20): the working state from the
// request until the chat's log shows the outcome (a summary made at the
// person's request, or the reason none was). The command itself is
// parsed in compactCommand.ts, which the first-load composer imports.
import { useEffect, useState } from "react";
import { pageApi } from "../apiPages";
import type { SessionView } from "../types";

/** How long the working state waits for the outcome before it gives up showing. */
const WAIT_MS = 10 * 60 * 1000;

/** Whether `items` from `from` on hold a /compact's outcome. */
export function compactEnded(view: SessionView | null | undefined, from: number): boolean {
  return !!view?.items.slice(from).some((it) => (it.type === "summary" && it.manual) || it.type === "error");
}

export function useCompact(sessionId: string, view: SessionView | null | undefined) {
  // The number of items when the request went out: the outcome comes after.
  const [from, setFrom] = useState<number | null>(null);
  useEffect(() => setFrom(null), [sessionId]);
  useEffect(() => {
    if (from != null && compactEnded(view, from)) setFrom(null);
  }, [view, from]);
  useEffect(() => {
    if (from == null) return;
    const id = setTimeout(() => setFrom(null), WAIT_MS);
    return () => clearTimeout(id);
  }, [from]);
  const start = async (focus: string) => {
    const n = view?.items.length ?? 0;
    await pageApi.compact(sessionId, focus);
    setFrom(n);
  };
  return { compacting: from != null, start };
}
