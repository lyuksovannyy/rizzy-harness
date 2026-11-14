// "Trying again in N s": a model call failed for a passing reason and the
// node waits before sending it again (`model_retry`). The fold drops it once
// output resumes; this only counts down to the time the node gave.
import { useEffect, useState } from "react";
import "../lib/i18nPages";
import { t, type Key } from "../lib/i18n";
import { shownName } from "../md/hidden";
import type { ModelRetry, RetryReason } from "../types";

const WHY: Record<RetryReason, Key> = {
  rate_limit: "retry.rate_limit",
  overloaded: "retry.overloaded",
  server_error: "retry.server_error",
  network: "retry.network",
};

/** Whole seconds until `at_ms` (rounded up): never below 0, and never more
 * than the wait itself, so a browser clock that is off cannot show more. */
export function secondsLeft(atMs: number, waitMs: number, now: number): number {
  return Math.min(Math.ceil(waitMs / 1000), Math.max(0, Math.ceil((atMs - now) / 1000)));
}

export function RetryNote({ retry }: { retry: ModelRetry }) {
  // Read the clock on every render; the state only makes it render each second.
  const [, tick] = useState(0);
  const left = secondsLeft(retry.at_ms, retry.wait_ms, Date.now());
  const waiting = left > 0;
  useEffect(() => {
    if (!waiting) return;
    const id = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(id);
  }, [retry.at_ms, waiting]);
  const vars = {
    model: shownName(retry.model),
    // A class this build does not know (a newer node) is shown as sent.
    why: WHY[retry.reason] ? t(WHY[retry.reason]) : shownName(String(retry.reason)),
    s: String(left),
    n: String(retry.attempt),
    of: String(retry.of),
  };
  return (
    <div className="retry-note small" role="status">
      {t(left > 0 ? "conv.retrying" : "conv.retryingNow", vars)}
    </div>
  );
}
