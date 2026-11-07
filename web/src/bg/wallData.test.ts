// The bundled lists: parsed once, fetched once per page load, retried after a failure.
import { afterEach, describe, expect, test, vi } from "vitest";
import { loadList, parseList } from "./wallData";

afterEach(() => vi.unstubAllGlobals());

describe("parseList", () => {
  test("trims, drops short, half and overlong lines, and repeats", () => {
    const text = [
      "how to tie a tie",
      "  how to tie a tie  ",
      "",
      "how to",
      "the best way how to",
      "short one",
      "why is the sky blue\r",
      "x".repeat(45),
      "y".repeat(44),
    ].join("\n");
    expect(parseList(text)).toEqual(["how to tie a tie", "why is the sky blue", "y".repeat(44)]);
  });
});

describe("loadList", () => {
  test("two loads fetch once", async () => {
    const fetch = vi.fn(async () => new Response("how to tie a tie\nwhy is the sky blue\n"));
    vi.stubGlobal("fetch", fetch);
    const [a, b] = await Promise.all([loadList("/wall/test-a.txt"), loadList("/wall/test-a.txt")]);
    expect(a).toEqual(["how to tie a tie", "why is the sky blue"]);
    expect(b).toBe(a);
    await loadList("/wall/test-a.txt");
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  test("a failed fetch is tried again next time", async () => {
    let n = 0;
    vi.stubGlobal("fetch", vi.fn(async () => (n++ ? new Response("how to tie a tie\n") : new Response("", { status: 503 }))));
    await expect(loadList("/wall/test-b.txt")).rejects.toThrow();
    await expect(loadList("/wall/test-b.txt")).resolves.toEqual(["how to tie a tie"]);
  });
});
