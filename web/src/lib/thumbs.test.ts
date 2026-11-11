// @vitest-environment happy-dom
// Browser-made thumbnails with the canvas, the decoder and the media
// elements faked: an image is drawn at 320 px as WebP (JPEG where WebP
// cannot be written), never decoded past 40 MP or 16,384 px a side or
// without known dimensions, SVG and HEIC go through an <img>, a video's
// frame at min(1 s, a tenth) with its hints, audio gives only its length,
// and any failure is recorded as no_thumb. Two at a time, once per item.
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { MediaItem } from "./media";
import { linked } from "./links.setup";
import { MAX_THUMB, makeThumb, queueThumb } from "./thumbs";

const item = (over: Partial<MediaItem> = {}): MediaItem => ({
  media_id: "m1",
  name: "a.jpg",
  kind: "image",
  format: "jpeg",
  mime: "image/jpeg",
  size: 1000,
  width: 2000,
  height: 1000,
  duration_ms: null,
  display: "inline",
  has_thumb: false,
  no_thumb: false,
  origin: "upload",
  source: null,
  artifact: null,
  chats: 0,
  workspace_id: null,
  created_ms: 0,
  updated_ms: 0,
  ...over,
});

const api = { update: vi.fn((..._a: unknown[]) => Promise.resolve({} as MediaItem)), putThumb: vi.fn((..._a: unknown[]) => Promise.resolve()) };
/** The canvases made, and what each encode hands back for a type. */
let canvases: [number, number][] = [];
let encoder: (type: string) => Blob | null = (type) => new Blob(["thumb"], { type });
const drawn: unknown[] = [];
class FakeCanvas {
  constructor(
    public width: number,
    public height: number,
  ) {
    canvases.push([width, height]);
  }
  getContext() {
    return { drawImage: (src: unknown) => drawn.push(src) };
  }
  convertToBlob(o: { type: string }) {
    return Promise.resolve(encoder(o.type));
  }
}
const bitmap = { width: 2000, height: 1000, close: vi.fn() };
const decode = vi.fn((_b: Blob) => Promise.resolve(bitmap));
const fetchFn = vi.fn((_url: string, _o?: unknown) => Promise.resolve({ ok: true, blob: () => Promise.resolve(new Blob(["img"])) }));

beforeEach(() => {
  canvases = [];
  drawn.length = 0;
  encoder = (type) => new Blob(["thumb"], { type });
  api.update.mockClear();
  api.putThumb.mockClear();
  decode.mockClear();
  bitmap.close.mockClear();
  fetchFn.mockClear();
  vi.stubGlobal("OffscreenCanvas", FakeCanvas);
  vi.stubGlobal("createImageBitmap", decode);
  vi.stubGlobal("fetch", fetchFn);
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const sentThumb = () => api.putThumb.mock.calls[0]?.[1] as Blob | undefined;

describe("images", () => {
  test("decoded from their bytes and drawn at 320 px on the long side, as WebP", async () => {
    expect(await makeThumb(item(), api)).toBe("made");
    expect(fetchFn.mock.calls[0]![0]).toBe("/api/media/m1/content");
    expect(canvases).toEqual([[320, 160]]);
    expect(drawn).toEqual([bitmap]);
    expect(api.putThumb.mock.calls[0]![0]).toBe("m1");
    expect(sentThumb()!.type).toBe("image/webp");
    expect(bitmap.close).toHaveBeenCalled();
    expect(api.update).not.toHaveBeenCalled();
  });

  test("JPEG where the browser cannot write WebP (it hands back PNG)", async () => {
    encoder = (type) => new Blob(["thumb"], { type: type === "image/webp" ? "image/png" : type });
    expect(await makeThumb(item(), api)).toBe("made");
    expect(sentThumb()!.type).toBe("image/jpeg");
  });

  test("a thumbnail too large at every quality is a failure, recorded", async () => {
    encoder = (type) => new Blob([new Uint8Array(MAX_THUMB + 1)], { type });
    expect(await makeThumb(item(), api)).toBe("failed");
    expect(api.putThumb).not.toHaveBeenCalled();
    expect(api.update).toHaveBeenCalledWith("m1", { no_thumb: true });
  });

  test.each([
    ["over 40 MP", { width: 8000, height: 6000 }],
    ["over 16,384 px on a side", { width: 20_000, height: 100 }],
    ["dimensions unknown", { width: null, height: null }],
    ["shown only as a download", { display: "download" as const, format: "tiff" }],
  ])("never decoded: %s", async (_, over) => {
    expect(await makeThumb(item(over), api)).toBe("none");
    expect(fetchFn).not.toHaveBeenCalled();
    expect(decode).not.toHaveBeenCalled();
    expect(api.update).toHaveBeenCalledWith("m1", { no_thumb: true });
  });

  test("a decode error is recorded, so it is never tried again", async () => {
    decode.mockRejectedValueOnce(new Error("bad image"));
    expect(await makeThumb(item(), api)).toBe("failed");
    expect(api.update).toHaveBeenCalledWith("m1", { no_thumb: true });
  });
});

/** An <img> that loads (or fails) once given a src. */
function fakeImage(ok: boolean, w = 3000, h = 4000) {
  vi.stubGlobal(
    "Image",
    class extends EventTarget {
      decoding = "";
      naturalWidth = w;
      naturalHeight = h;
      src = "";
      constructor() {
        super();
        let src = "";
        Object.defineProperty(this, "src", {
          get: () => src,
          set: (v: string) => {
            src = v;
            queueMicrotask(() => this.dispatchEvent(new Event(ok ? "load" : "error")));
          },
        });
      }
    },
  );
}

describe("SVG and HEIC", () => {
  test("go through an <img>, which sends Sec-Fetch-Dest: image", async () => {
    fakeImage(true);
    expect(await makeThumb(item({ format: "heic", display: "img" }), api)).toBe("made");
    expect(fetchFn).not.toHaveBeenCalled();
    expect(canvases).toEqual([[240, 320]]);
    expect((drawn[0] as { src: string }).src).toBe(linked("/api/media/m1/content"));
    // SVG has no header size, and needs none.
    fakeImage(true, 0, 0);
    expect(await makeThumb(item({ format: "svg", display: "img", width: null, height: null }), api)).toBe("made");
  });

  test("a HEIC this browser cannot read gets an icon from now on", async () => {
    fakeImage(false);
    expect(await makeThumb(item({ format: "heic", display: "img" }), api)).toBe("failed");
    expect(api.update).toHaveBeenCalledWith("m1", { no_thumb: true });
  });
});

/** The next <video> or <audio> made, with what the browser would report. */
function nextMedia(tag: "video" | "audio", meta: { duration: number; w?: number; h?: number }) {
  const made = document.createElement.bind(document);
  const el: { current: HTMLMediaElement | null; seekedTo: number | null } = { current: null, seekedTo: null };
  vi.spyOn(document, "createElement").mockImplementation(((name: string) => {
    const e = made(name) as HTMLMediaElement;
    if (name !== tag) return e;
    Object.defineProperty(e, "duration", { get: () => meta.duration });
    Object.defineProperty(e, "videoWidth", { get: () => meta.w ?? 0 });
    Object.defineProperty(e, "videoHeight", { get: () => meta.h ?? 0 });
    Object.defineProperty(e, "currentTime", {
      get: () => el.seekedTo ?? 0,
      set: (v: number) => {
        el.seekedTo = v;
        queueMicrotask(() => e.dispatchEvent(new Event("seeked")));
      },
    });
    el.current = e;
    return e;
  }) as typeof document.createElement);
  return el;
}
const tick = () => new Promise((r) => setTimeout(r, 0));

describe("video and audio", () => {
  test("a video's frame at a tenth of it (at most 1 s), with its size and length", async () => {
    const v = nextMedia("video", { duration: 5, w: 1280, h: 720 });
    const done = makeThumb(item({ kind: "video", format: "mp4" }), api);
    await tick();
    expect(v.current!.getAttribute("src")).toBe(linked("/api/media/m1/content"));
    expect(v.current!.preload).toBe("metadata");
    v.current!.dispatchEvent(new Event("loadedmetadata"));
    expect(await done).toBe("made");
    expect(v.seekedTo).toBe(0.5);
    expect(canvases).toEqual([[320, 180]]);
    expect(api.update).toHaveBeenCalledWith("m1", { duration_ms: 5000, width: 1280, height: 720 });
  });

  test("never later than 1 s", async () => {
    const v = nextMedia("video", { duration: 600, w: 640, h: 480 });
    const done = makeThumb(item({ kind: "video", format: "webm" }), api);
    await tick();
    v.current!.dispatchEvent(new Event("loadedmetadata"));
    await done;
    expect(v.seekedTo).toBe(1);
  });

  test("a MOV this browser cannot play gets an icon from now on", async () => {
    const v = nextMedia("video", { duration: 0 });
    const done = makeThumb(item({ kind: "video", format: "mov" }), api);
    await tick();
    v.current!.dispatchEvent(new Event("error"));
    expect(await done).toBe("failed");
    expect(api.update).toHaveBeenCalledWith("m1", { no_thumb: true });
  });

  test("audio sends only its length, and asks for no thumbnail again", async () => {
    const a = nextMedia("audio", { duration: 200.4 });
    const done = makeThumb(item({ kind: "audio", format: "mp3" }), api);
    await tick();
    a.current!.dispatchEvent(new Event("loadedmetadata"));
    expect(await done).toBe("none");
    expect(api.putThumb).not.toHaveBeenCalled();
    expect(api.update).toHaveBeenCalledWith("m1", { duration_ms: 200_400, no_thumb: true });
  });
});

describe("queue", () => {
  test("two at a time, once per item, and never for one that has a thumbnail", async () => {
    fetchFn.mockImplementation(() => new Promise(() => {}));
    const done = vi.fn();
    queueThumb(item({ media_id: "a" }), done);
    queueThumb(item({ media_id: "b" }), done);
    queueThumb(item({ media_id: "c" }), done);
    queueThumb(item({ media_id: "a" }), done);
    queueThumb(item({ media_id: "d", has_thumb: true }), done);
    await tick();
    expect(fetchFn.mock.calls.map((c) => c[0])).toEqual(["/api/media/a/content", "/api/media/b/content"]);
  });
});
