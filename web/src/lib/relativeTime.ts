// A time as the row under an answer shows it: "just now", "5 min ago",
// "2 h ago", "yesterday 14:02", then a short date; the complete local date,
// time and zone as its hover text. One timer for the whole page: a row that
// shows a time subscribes to `useNow`, which changes once per bucket, so
// only these small components render again, never the conversation.
import { useSyncExternalStore } from "react";
import "./i18nPages";
import { t } from "./i18n";

const MIN = 60_000;
const HOUR = 60 * MIN;

const hhmm = (d: Date) => `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
const sameDay = (a: Date, b: Date) => a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();

/** `at` relative to `now` (both ms). A time in the future (a clock that is
 * ahead) reads "just now". Under a minute: "just now"; under an hour: minutes;
 * later the same calendar day: hours; the day before: "yesterday" and the
 * time; older: the date, with the year when it is not this one. */
export function relativeTime(at: number, now: number, locale?: string): string {
  const diff = now - at;
  if (diff < MIN) return t("time.justNow");
  if (diff < HOUR) return t("time.min", { n: String(Math.floor(diff / MIN)) });
  const d = new Date(at);
  const n = new Date(now);
  if (sameDay(d, n)) return t("time.hour", { n: String(Math.floor(diff / HOUR)) });
  const before = new Date(n);
  before.setDate(n.getDate() - 1);
  if (sameDay(d, before)) return t("time.yesterday", { time: hhmm(d) });
  return d.toLocaleDateString(locale, d.getFullYear() === n.getFullYear() ? { month: "short", day: "numeric" } : { year: "numeric", month: "short", day: "numeric" });
}

/** The complete local date and time, with seconds and the time zone. */
export function fullTime(at: number, locale?: string): string {
  return new Date(at).toLocaleString(locale, {
    year: "numeric",
    month: "long",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    timeZoneName: "short",
  });
}

/** How often the page's times are looked at again; also the size of the
 * buckets `useNow` answers in, so its value is steady between ticks. */
export const TICK_MS = 15_000;
const bucket = () => Math.floor(Date.now() / TICK_MS) * TICK_MS;

const subs = new Set<() => void>();
let timer: ReturnType<typeof setInterval> | undefined;
function subscribe(l: () => void): () => void {
  subs.add(l);
  timer ??= setInterval(() => subs.forEach((f) => f()), TICK_MS);
  return () => {
    subs.delete(l);
    if (subs.size === 0 && timer !== undefined) {
      clearInterval(timer);
      timer = undefined;
    }
  };
}

/** The current time in ms, in buckets of `TICK_MS`, from the page's one timer. */
export const useNow = (): number => useSyncExternalStore(subscribe, bucket, bucket);

/** How many components follow the timer now (for tests). */
export const watchers = () => subs.size;
