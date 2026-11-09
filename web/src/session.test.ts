// @vitest-environment happy-dom
// The session keeper: single-flight refresh and one retry after a 401.
import { afterEach, describe, expect, it, vi } from "vitest";

describe("session keeper", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetModules();
    delete (window as { __sessionKeeper?: boolean }).__sessionKeeper;
  });

  it("refreshes once for parallel 401s, retries, and leaves login alone", async () => {
    let refreshed = 0;
    let valid = false;
    const base = vi.fn(async (input: RequestInfo | URL) => {
      const path = new URL(String(input), location.href).pathname;
      if (path === "/api/auth/refresh") {
        refreshed++;
        await new Promise((r) => setTimeout(r, 5));
        valid = true;
        return new Response("{}", { status: 200 });
      }
      if (path === "/api/login") return new Response("{}", { status: 401 });
      return new Response("{}", { status: valid ? 200 : 401 });
    });
    vi.stubGlobal("fetch", base);
    const { installSessionKeeper } = await import("./session");
    installSessionKeeper();
    const [a, b] = await Promise.all([fetch("/api/me"), fetch("/api/sessions")]);
    expect([a.status, b.status]).toEqual([200, 200]);
    expect(refreshed).toBe(1);
    // A wrong password is a 401 too, but not a reason to refresh.
    const login = await fetch("/api/login", { method: "POST" });
    expect(login.status).toBe(401);
    expect(refreshed).toBe(1);
    // Other origins are never touched.
    valid = false;
    await fetch("https://example.com/api/x");
    expect(refreshed).toBe(1);
  });
});
