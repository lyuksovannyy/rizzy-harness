// When something happened (or will), as every list shows it: "now",
// "12m ago", "Yesterday", "Tue", "Mar 4", "Mar 4, 2025". In the user's time
// zone (prefs.timezone), else the browser's. Intl only: no date library in
// the first load.

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** A formatter in `timeZone`, falling back to the browser's zone when it is
 * unset or unknown. */
function inZone(opts: Intl.DateTimeFormatOptions, timeZone?: string | null): Intl.DateTimeFormat {
  try {
    return new Intl.DateTimeFormat("en-US", { ...opts, timeZone: timeZone ?? undefined });
  } catch {
    return new Intl.DateTimeFormat("en-US", opts);
  }
}

/** The calendar day (days since the epoch) and year of `ms` in the zone. */
function calendar(ms: number, timeZone?: string | null): { day: number; year: number } {
  const f = inZone({ year: "numeric", month: "numeric", day: "numeric" }, timeZone);
  const p = Object.fromEntries(f.formatToParts(ms).map((x) => [x.type, x.value]));
  const year = +p.year!;
  return { day: Math.floor(Date.UTC(year, +p.month! - 1, +p.day!) / DAY), year };
}

const relative = new Intl.RelativeTimeFormat("en", { style: "narrow" });

/** `ms` relative to `now` (both epoch milliseconds). Times ahead (a
 * routine's next run) read the same way: "in 5m", "Tomorrow", "Tue". */
export function when(ms: number, now: number, timeZone?: string | null): string {
  const diff = now - ms;
  const ahead = diff < 0;
  const abs = Math.abs(diff);
  if (abs < MINUTE) return "now";
  const at = calendar(ms, timeZone);
  const today = calendar(now, timeZone);
  const days = today.day - at.day;
  // The same day can be 25 hours long (clocks going back).
  if (abs < DAY || days === 0) {
    const [n, unit] = abs < HOUR ? [Math.floor(abs / MINUTE), "minute" as const] : [Math.floor(abs / HOUR), "hour" as const];
    return relative.format(ahead ? n : -n, unit);
  }
  if (days === 1) return "Yesterday";
  if (days === -1) return "Tomorrow";
  if (Math.abs(days) < 7) return inZone({ weekday: "short" }, timeZone).format(ms);
  if (at.year === today.year) return inZone({ month: "short", day: "numeric" }, timeZone).format(ms);
  return inZone({ month: "short", day: "numeric", year: "numeric" }, timeZone).format(ms);
}
