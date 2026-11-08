// Artifacts' own helpers: the list query (kind, mine, search, cursor), the
// addresses, what a row says (the chip, "from" or "by", progress, what a
// pack left out), which versions have a preview, and "Save as artifact"'s
// body (a name only when one was typed).
import { beforeEach, describe, expect, test, vi } from "vitest";
import type { ArtifactVersion } from "./artifacts";

const call = vi.fn((..._a: unknown[]) => Promise.resolve({}));
vi.mock("../api", () => ({ call: (...a: unknown[]) => call(...a) }));
const { artifactsApi, chipOf, downloadUrl, kindLabel, latestUrl, percent, previewable, skippedLine, sourceLine } = await import("./artifacts");

const v = (over: Partial<ArtifactVersion> = {}): ArtifactVersion => ({
  version_id: "v1",
  seq: 7,
  status: "ready",
  error: null,
  progress: null,
  size: 12_900_000,
  sha256: "3f2a".padEnd(64, "0"),
  filename: "myapp",
  packed: null,
  kind: "binary",
  format: "elf",
  display: "download",
  meta: {},
  notes: "",
  node: { node_id: "n1", hostname: "box" },
  session: { session_id: "s1", title: "Build the CLI" },
  by_user: false,
  pinned: false,
  has_thumb: false,
  created_ms: 0,
  ...over,
});

beforeEach(() => call.mockClear());

describe("api", () => {
  test("the list sends only the filters that are set", async () => {
    await artifactsApi.list({ workspace: "w1" });
    expect(call).toHaveBeenLastCalledWith("GET", "/api/artifacts?workspace=w1&limit=50");
    await artifactsApi.list({ workspace: "w1", kind: "builds", mine: true, q: "  my app ", cursor: "c2" });
    expect(call).toHaveBeenLastCalledWith("GET", "/api/artifacts?workspace=w1&kind=builds&mine=1&q=my+app&cursor=c2&limit=50");
  });

  test("Save as artifact sends a name only when one was typed", async () => {
    await artifactsApi.save("s1", "target/release/app", "  ");
    expect(call).toHaveBeenLastCalledWith("POST", "/api/sessions/s1/artifacts", { path: "target/release/app" });
    await artifactsApi.save("s1", "dist", " site ");
    expect(call).toHaveBeenLastCalledWith("POST", "/api/sessions/s1/artifacts", { path: "dist", name: "site" });
  });

  test("addresses", () => {
    expect(latestUrl("a1")).toBe("/api/artifacts/a1/latest/download");
    expect(downloadUrl("a1", "v1")).toBe("/api/artifacts/a1/versions/v1/download");
  });
});

describe("what a row says", () => {
  test("the chip is the build target, else the pack, else the format", () => {
    expect(chipOf(v({ meta: { target: "x86_64-unknown-linux-gnu" } }))).toBe("x86_64-unknown-linux-gnu");
    expect(chipOf(v({ packed: "tar.gz", format: "gzip" }))).toBe("tar.gz");
    expect(chipOf(v())).toBe("ELF");
    expect(chipOf(v({ format: null }))).toBeNull();
  });

  test("the owner reads the chat it came from; a colleague reads who shared it", () => {
    expect(sourceLine({ mine: true, owner: { username: "alex" }, latest: v() })).toBe("from ‘Build the CLI’");
    expect(sourceLine({ mine: true, owner: null, latest: v({ session: null }) })).toBeNull();
    expect(sourceLine({ mine: false, owner: { username: "bob" }, latest: v({ session: { private: true } }) })).toBe("by bob");
  });

  test("progress, kinds and what a pack left out", () => {
    expect(percent({ received: 512, total: 1024 })).toBe("50%");
    expect(percent({ received: 0, total: 0 })).toBeNull();
    expect(percent(null)).toBeNull();
    expect(kindLabel("binary")).toBe("Program");
    expect(kindLabel(null)).toBe("File");
    expect(skippedLine({ symlinks: 3, secret_names: 1, git: 0 })).toBe("3 symlinks, 1 secret names");
    expect(skippedLine(undefined)).toBeNull();
  });

  test("a preview only for ready images, video and audio the browser may show", () => {
    expect(previewable(v({ kind: "image", display: "inline" }))).toBe(true);
    expect(previewable(v({ kind: "image", display: "img" }))).toBe(true);
    expect(previewable(v({ kind: "image", display: "download" }))).toBe(false);
    expect(previewable(v({ kind: "video", display: "inline", status: "pending" }))).toBe(false);
    expect(previewable(v({ kind: "document", display: "inline" }))).toBe(false);
  });
});
