// @vitest-environment happy-dom
// The time under an answer: its words at the boundaries, its full hover
// text, and the one timer the whole page shares.
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { fullTime, relativeTime, TICK_MS, watchers } from "./relativeTime";
import { RelTime } from "../components/MessageActions";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = new Date(2026, 10, 21, 15, 0, 0).getTime();
const at = (ms: number) => NOW - ms;
const S = 1000;
const M = 60 * S;
const H = 60 * M;

describe("relativeTime", () => {
  test("under a minute is just now, then minutes up to the hour", () => {
    expect(relativeTime(at(0), NOW)).toBe("just now");
    expect(relativeTime(at(59 * S), NOW)).toBe("just now");
    expect(relativeTime(at(60 * S), NOW)).toBe("1 min ago");
    expect(relativeTime(at(5 * M), NOW)).toBe("5 min ago");
    expect(relativeTime(at(59 * M + 59 * S), NOW)).toBe("59 min ago");
  });

  test("a clock that is ahead reads just now", () => {
    expect(relativeTime(NOW + 5 * M, NOW)).toBe("just now");
  });

  test("later the same day is hours", () => {
    expect(relativeTime(at(H), NOW)).toBe("1 h ago");
    expect(relativeTime(at(2 * H + 59 * M), NOW)).toBe("2 h ago");
    expect(relativeTime(new Date(2026, 10, 21, 0, 0, 1).getTime(), NOW)).toBe("14 h ago");
  });

  test("the day before is yesterday with the time, however few hours ago", () => {
    expect(relativeTime(new Date(2026, 10, 20, 14, 2).getTime(), NOW)).toBe("yesterday 14:02");
    expect(relativeTime(new Date(2026, 10, 20, 0, 5).getTime(), NOW)).toBe("yesterday 00:05");
    // 40 minutes ago across midnight is still minutes.
    const early = new Date(2026, 10, 21, 0, 30).getTime();
    expect(relativeTime(new Date(2026, 10, 20, 23, 50).getTime(), early)).toBe("40 min ago");
    // 3 hours ago across midnight is yesterday.
    expect(relativeTime(new Date(2026, 10, 20, 22, 0).getTime(), new Date(2026, 10, 21, 1, 0).getTime())).toBe("yesterday 22:00");
  });

  test("older is a short date, with the year only when it is not this one", () => {
    const old = relativeTime(new Date(2026, 10, 12, 9, 0).getTime(), NOW, "en-US");
    expect(old).toBe("Nov 12");
    expect(relativeTime(new Date(2025, 10, 12, 9, 0).getTime(), NOW, "en-US")).toBe("Nov 12, 2025");
  });

  test("the full time has the date, the seconds and the zone", () => {
    const s = fullTime(new Date(2026, 10, 21, 15, 4, 9).getTime(), "en-US");
    expect(s).toMatch(/November 21, 2026/);
    expect(s).toMatch(/:04:09/);
    expect(s).toMatch(/(GMT|UTC|[A-Z]{2,5})/);
  });
});

describe("one timer for the page", () => {
  let host: HTMLElement;
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] });
    vi.setSystemTime(NOW);
    host = document.createElement("div");
    document.body.append(host);
  });
  afterEach(() => {
    vi.useRealTimers();
    host.remove();
  });

  test("many times, one interval; they move on together and the timer ends with the last", async () => {
    const spy = vi.spyOn(globalThis, "setInterval");
    const root = createRoot(host);
    await act(async () =>
      root.render(
        createElement("div", null, createElement(RelTime, { at: at(30 * S) }), createElement(RelTime, { at: at(50 * S) }), createElement(RelTime, { at: at(5 * M) })),
      ),
    );
    expect(watchers()).toBe(3);
    expect(spy).toHaveBeenCalledTimes(1);
    const words = () => [...host.querySelectorAll("time")].map((t) => t.textContent);
    expect(words()).toEqual(["just now", "just now", "5 min ago"]);
    expect(host.querySelector("time")!.getAttribute("title")).toBe(fullTime(at(30 * S)));
    expect(host.querySelector("time")!.getAttribute("datetime")).toBe(new Date(at(30 * S)).toISOString());
    await act(async () => void vi.advanceTimersByTime(TICK_MS * 4));
    expect(words()).toEqual(["1 min ago", "1 min ago", "6 min ago"]);
    await act(async () => root.unmount());
    expect(watchers()).toBe(0);
  });
});
