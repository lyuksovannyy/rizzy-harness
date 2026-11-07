// @vitest-environment happy-dom
// The server answers "try again" for a burst of new icons, which an <img>
// sees as an error: the icon retries a few times before showing the globe.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, test, vi } from "vitest";
import { EngineIcon } from "./EngineIcon";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => vi.useRealTimers());

function mount(origin: string) {
  const host = document.createElement("div");
  document.body.append(host);
  act(() => createRoot(host).render(<EngineIcon origin={origin} />));
  return host;
}

const fail = (host: HTMLElement) => act(() => host.querySelector("img")!.dispatchEvent(new Event("error")));

describe("EngineIcon", () => {
  test("retries with backoff, then settles on the globe", () => {
    vi.useFakeTimers();
    const host = mount("https://example.com");
    expect(host.querySelector("img")!.getAttribute("src")).toBe("/api/favicon?url=https%3A%2F%2Fexample.com");
    for (const [n, wait] of [[1, 800], [2, 2000], [3, 5000]] as const) {
      fail(host);
      expect(host.querySelector("img")).toBeNull();
      act(() => vi.advanceTimersByTime(wait));
      expect(host.querySelector("img")!.getAttribute("src")).toContain(`&try=${n}`);
    }
    fail(host);
    expect(host.querySelector("img")).toBeNull();
    expect(host.querySelector("svg")).not.toBeNull();
  });

  test("no origin is the globe at once", () => {
    const host = document.createElement("div");
    act(() => createRoot(host).render(<EngineIcon origin={null} />));
    expect(host.querySelector("img")).toBeNull();
    expect(host.querySelector("svg")).not.toBeNull();
  });
});
