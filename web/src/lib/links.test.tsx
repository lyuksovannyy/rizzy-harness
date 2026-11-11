// @vitest-environment happy-dom
// Links for what the browser loads by itself (Gate 3 P-1): minted by one
// POST per tick for every URL asked for in it, reused while fresh, never
// across logins, and renewed for a download button that stays on screen.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";

vi.unmock("./links");
let key = "k1";
const call = vi.fn(async (_m: string, _p: string, b: { urls: string[] }) => ({
  links: Object.fromEntries(b.urls.filter((u) => !u.includes("refused")).map((u) => [u, `${u}?link=${key}`])),
}));
vi.mock("../api", () => ({ call: (...a: [string, string, { urls: string[] }]) => call(...a), loginKey: () => key }));

const { link, useLink } = await import("./links");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

beforeEach(() => {
  call.mockClear();
  key = `k${Math.random()}`;
});
afterEach(() => vi.useRealTimers());

describe("link", () => {
  test("one call for all the URLs of a tick, each answered with its own", async () => {
    const [a, b, c] = await Promise.all([link("/api/media/a/thumb"), link("/api/media/b/thumb"), link("/api/media/a/thumb")]);
    expect(call).toHaveBeenCalledTimes(1);
    expect(call.mock.calls[0]!.slice(0, 2)).toEqual(["POST", "/api/links"]);
    expect(call.mock.calls[0]![2]).toEqual({ urls: ["/api/media/a/thumb", "/api/media/b/thumb"] });
    expect([a, b, c]).toEqual([`/api/media/a/thumb?link=${key}`, `/api/media/b/thumb?link=${key}`, `/api/media/a/thumb?link=${key}`]);
  });

  test("reused while fresh, minted again for another login or once stale", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    await link("/api/media/a/content");
    await link("/api/media/a/content");
    expect(call).toHaveBeenCalledTimes(1);
    key = "another login";
    expect(await link("/api/media/a/content")).toBe("/api/media/a/content?link=another login");
    expect(call).toHaveBeenCalledTimes(2);
    vi.setSystemTime(Date.now() + 26 * 60_000);
    await link("/api/media/a/content");
    expect(call).toHaveBeenCalledTimes(3);
  });

  test("a URL the server makes no link for fails, and is asked again next time", async () => {
    await expect(link("/api/refused")).rejects.toThrow("no link");
    await expect(link("/api/refused")).rejects.toThrow("no link");
    expect(call).toHaveBeenCalledTimes(2);
  });
});

describe("useLink", () => {
  test("nothing until minted; a download button's link is renewed while shown", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    const seen: (string | undefined)[] = [];
    function Probe() {
      seen.push(useLink("/api/artifacts/a/versions/v/download", true));
      return null;
    }
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => root.render(<Probe />));
    expect(seen[0]).toBeUndefined();
    expect(seen.at(-1)).toBe(`/api/artifacts/a/versions/v/download?link=${key}`);
    const first = key;
    key = "later";
    await act(async () => {
      vi.advanceTimersByTime(25 * 60_000);
    });
    expect(call).toHaveBeenCalledTimes(2);
    expect(seen.at(-1)).toBe("/api/artifacts/a/versions/v/download?link=later");
    expect(first).not.toBe("later");
    act(() => root.unmount());
  });
});
