import { tPick } from "./i18n";

export type Bucket = "morning" | "afternoon" | "evening" | "night";

export function bucket(hour: number): Bucket {
  if (hour >= 5 && hour < 12) return "morning";
  if (hour >= 12 && hour < 18) return "afternoon";
  if (hour >= 18 && hour < 23) return "evening";
  return "night";
}

/** Hour and calendar day of `now` in `timeZone` (browser zone if unset/invalid). */
export function localParts(now: Date, timeZone?: string | null): { hour: number; day: number } {
  const fmt = (tz?: string) =>
    new Intl.DateTimeFormat("en-CA", {
      timeZone: tz,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      hourCycle: "h23",
    });
  let f: Intl.DateTimeFormat;
  try {
    f = fmt(timeZone ?? undefined);
  } catch {
    f = fmt();
  }
  const parts = Object.fromEntries(f.formatToParts(now).map((p) => [p.type, p.value]));
  const day = Math.floor(Date.UTC(+parts.year!, +parts.month! - 1, +parts.day!) / 86_400_000);
  return { hour: +parts.hour! % 24, day };
}

/** A greeting for `now` in the user's zone. Stable within a day, varies by day. */
export function greeting(now: Date, name: string, timeZone?: string | null): string {
  const { hour, day } = localParts(now, timeZone);
  return tPick(`greeting.${bucket(hour)}`, day, { name });
}

/** The browser's IANA zone, e.g. `Europe/Riga`. */
export function browserTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
}
