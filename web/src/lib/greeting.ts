import { tPick } from "./i18n";

export type Bucket = "morning" | "afternoon" | "evening" | "night";

export function bucket(hour: number): Bucket {
  if (hour >= 5 && hour < 12) return "morning";
  if (hour >= 12 && hour < 18) return "afternoon";
  if (hour >= 18 && hour < 23) return "evening";
  return "night";
}

/** A greeting for `now` (local time). Stable within a day, varies by day. */
export function greeting(now: Date, name: string): string {
  const day = Math.floor(
    Date.UTC(now.getFullYear(), now.getMonth(), now.getDate()) / 86_400_000,
  );
  return tPick(`greeting.${bucket(now.getHours())}`, day, { name });
}
