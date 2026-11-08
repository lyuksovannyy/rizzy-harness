import { describe, expect, it } from "vitest";
import { when } from "./when";

const RIGA = "Europe/Riga";
// Sunday 2026-11-08, 14:00 in Riga (UTC+2).
const NOW = Date.UTC(2026, 10, 8, 12, 0);
const MIN = 60_000;
const HOUR = 60 * MIN;
// The wording of relative times is Intl's; the buckets and units are ours.
const rel = (n: number, unit: "minute" | "hour") => new Intl.RelativeTimeFormat("en", { style: "narrow" }).format(n, unit);

describe("when", () => {
  it("says now, then minutes and hours for the last day", () => {
    expect(when(NOW - 30_000, NOW, RIGA)).toBe("now");
    expect(when(NOW, NOW, RIGA)).toBe("now");
    expect(when(NOW - 12 * MIN - 59_000, NOW, RIGA)).toBe(rel(-12, "minute"));
    expect(when(NOW - 3 * HOUR - 10 * MIN, NOW, RIGA)).toBe(rel(-3, "hour"));
    expect(when(NOW - 23 * HOUR, NOW, RIGA)).toBe(rel(-23, "hour"));
  });

  it("names yesterday, then the weekday, then the date", () => {
    expect(when(Date.UTC(2026, 10, 7, 8, 0), NOW, RIGA)).toBe("Yesterday");
    expect(when(Date.UTC(2026, 10, 3, 9, 0), NOW, RIGA)).toBe("Tue");
    expect(when(Date.UTC(2026, 10, 1, 9, 0), NOW, RIGA)).toBe("Nov 1");
    expect(when(Date.UTC(2026, 2, 4, 9, 0), NOW, RIGA)).toBe("Mar 4");
    expect(when(Date.UTC(2025, 2, 4, 9, 0), NOW, RIGA)).toBe("Mar 4, 2025");
  });

  it("reads times ahead the same way", () => {
    expect(when(NOW + 5 * MIN, NOW, RIGA)).toBe(rel(5, "minute"));
    expect(when(NOW + 30 * HOUR, NOW, RIGA)).toBe("Tomorrow");
    expect(when(Date.UTC(2026, 10, 12, 9, 0), NOW, RIGA)).toBe("Thu");
  });

  it("counts calendar days in the given time zone", () => {
    const now = Date.UTC(2026, 10, 9, 10, 0); // Monday, 12:00 in Riga
    const late = Date.UTC(2026, 10, 7, 22, 30); // 00:30 on Sunday in Riga, Saturday in UTC
    expect(when(late, now, RIGA)).toBe("Yesterday");
    expect(when(late, now, "UTC")).toBe("Sat");
    // Around midnight the year turns in one zone and not the other.
    const newYear = Date.UTC(2026, 11, 31, 23, 30); // Jan 1 2027, 01:30 in Riga
    const later = Date.UTC(2027, 1, 1, 12, 0);
    expect(when(newYear, later, RIGA)).toBe("Jan 1");
    expect(when(newYear, later, "UTC")).toBe("Dec 31, 2026");
  });

  it("falls back to the browser's zone for an unknown one", () => {
    const ms = Date.UTC(2026, 2, 4, 12, 0);
    expect(when(ms, NOW, "Not/AZone")).toBe(when(ms, NOW, null));
    expect(when(ms, NOW, undefined)).toBe(when(ms, NOW, null));
  });

  it("keeps a long day (clocks going back) in hours", () => {
    // Riga falls back on 2026-10-25: that day has 25 hours.
    const start = Date.UTC(2026, 9, 24, 21, 30); // 00:30 local (UTC+3)
    const end = start + 24 * HOUR + 15 * MIN; // 23:45 local (UTC+2), the same day
    expect(when(start, end, RIGA)).toBe(rel(-24, "hour"));
  });
});
