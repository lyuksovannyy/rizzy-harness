// @vitest-environment happy-dom
// Media's own helpers: sizes and lengths as people read them, which images
// are safe to decode (header dimensions, 40 MP, 16,384 px), the origin line,
// and the upload: a raw body with the name in a header (never in the URL),
// progress, a retry after the server's Retry-After, and its error text.
import { afterEach, describe, expect, test, vi } from "vitest";
import { bytes, clockTime, type MediaItem, originLine, safeToDecode, upload } from "./media";

describe("helpers", () => {
  test("sizes and lengths", () => {
    expect(bytes(500)).toBe("500 B");
    expect(bytes(1536)).toBe("1.5 KB");
    expect(bytes(2.3 * 1024 * 1024)).toBe("2.3 MB");
    expect(bytes(200 * 1024 * 1024)).toBe("200 MB");
    expect(bytes(4 * 1024 ** 3)).toBe("4.0 GB");
    expect(clockTime(83_000)).toBe("1:23");
    expect(clockTime(3_723_000)).toBe("1:02:03");
  });

  const img = (over: Partial<MediaItem>) => ({ kind: "image" as const, display: "inline" as const, format: "png", width: 100, height: 100, ...over });
  test("an image is decoded only within 40 MP and 16,384 px, with known dimensions", () => {
    expect(safeToDecode(img({}))).toBe(true);
    expect(safeToDecode(img({ width: 6324, height: 6324 }))).toBe(true);
    expect(safeToDecode(img({ width: 8000, height: 6000 }))).toBe(false);
    expect(safeToDecode(img({ width: 16_385, height: 10 }))).toBe(false);
    expect(safeToDecode(img({ width: null, height: null }))).toBe(false);
    expect(safeToDecode(img({ format: "svg", width: null, height: null, display: "img" }))).toBe(true);
    expect(safeToDecode(img({ display: "download", format: "tiff" }))).toBe(false);
    expect(safeToDecode({ ...img({}), kind: "video" })).toBe(false);
  });

  test("where an item came from", () => {
    expect(originLine({ origin: "upload", source: null })).toBe("Uploaded");
    expect(originLine({ origin: "chat", source: { session_id: "s", title: "Trip" } })).toBe("From chat ‘Trip’");
    expect(originLine({ origin: "chat", source: "deleted" })).toBe("From a deleted chat");
    expect(originLine({ origin: "agent", source: null })).toBe("Published by an agent");
  });
});

/** Each request made, answered by the test. */
const sent: FakeXhr[] = [];
class FakeXhr {
  method = "";
  url = "";
  headers: Record<string, string> = {};
  body: unknown;
  status = 0;
  statusText = "";
  responseText = "";
  withCredentials = false;
  upload: { onprogress: ((e: { lengthComputable: boolean; loaded: number; total: number }) => void) | null } = { onprogress: null };
  onload: (() => void) | null = null;
  onerror: (() => void) | null = null;
  private reply: Record<string, string> = {};
  open(m: string, u: string) {
    this.method = m;
    this.url = u;
  }
  setRequestHeader(k: string, v: string) {
    this.headers[k] = v;
  }
  getResponseHeader(k: string) {
    return this.reply[k] ?? null;
  }
  send(b: unknown) {
    this.body = b;
    sent.push(this);
  }
  answer(status: number, body: unknown, headers: Record<string, string> = {}) {
    this.status = status;
    this.responseText = JSON.stringify(body);
    this.reply = headers;
    this.onload?.();
  }
}

describe("upload", () => {
  afterEach(() => {
    sent.length = 0;
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  test("the name goes in a header, never in the URL; progress; the item back", async () => {
    vi.stubGlobal("XMLHttpRequest", FakeXhr);
    const file = new File(["x"], "Ünïcode beach.png", { type: "image/png" });
    const progress: number[] = [];
    const done = upload(file, "w1", (s) => progress.push(s));
    const x = sent[0]!;
    expect([x.method, x.url]).toEqual(["POST", "/api/media?origin=upload&workspace=w1"]);
    expect(x.url).not.toContain("beach");
    expect(x.headers["x-file-name"]).toBe(encodeURIComponent("Ünïcode beach.png"));
    expect(x.headers["content-type"]).toBe("image/png");
    expect(x.body).toBe(file);
    x.upload.onprogress!({ lengthComputable: true, loaded: 1, total: 4 });
    expect(progress).toEqual([0.25]);
    x.answer(200, { media_id: "m1", duplicate: true });
    expect(await done).toEqual({ item: { media_id: "m1", duplicate: true }, duplicate: true });
  });

  test("a busy server is asked again after its Retry-After; a refusal gives its reason", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("XMLHttpRequest", FakeXhr);
    const done = upload(new File(["x"], "a.png"), null, () => {});
    sent[0]!.answer(429, { error: "busy" }, { "retry-after": "2" });
    expect(sent).toHaveLength(1);
    vi.advanceTimersByTime(2000);
    expect(sent).toHaveLength(2);
    expect(sent[1]!.url).toBe("/api/media?origin=upload");
    sent[1]!.answer(507, { error: "Your storage is full (4.0 of 4 GB)" });
    await expect(done).rejects.toMatchObject({ status: 507, message: "Your storage is full (4.0 of 4 GB)" });
  });
});
