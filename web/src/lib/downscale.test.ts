// @vitest-environment happy-dom
// Chat images made small in the browser, with the canvas and the decoder
// faked: at most 2048 px on the long edge, then smaller (a fifth each time)
// and at a lower quality until 1.25 MB fits; photos stay JPEG, the rest go
// WebP (PNG where WebP cannot be written), named for what they became; a
// HEIC only where it decodes; other kinds refused. The bar's side: each
// image uploads to Media as a chat image, within the room and 5 MB, the
// same picture once; and what "Use in chat" left is taken once.
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Picture } from "./downscale";
import type { MediaItem } from "./media";

let uploaded = 0;
const upload = vi.fn((f: File, ..._a: unknown[]) =>
  Promise.resolve({ item: { media_id: f.name.startsWith("same") ? "same" : `m${++uploaded}`, name: f.name, size: f.size } as MediaItem, duplicate: false }),
);
const get = vi.fn((id: string) => (id === "gone" ? Promise.reject(new Error("404")) : Promise.resolve({ media_id: id, name: `${id}.jpg`, size: id.startsWith("big") ? 4.5 * 1024 * 1024 : 1000 } as MediaItem)));
vi.mock("./media", async (original) => ({ ...(await original<typeof import("./media")>()), upload, mediaApi: { get } }));
const { addImages, HEIC_TEXT, MAX_BYTES, pickUp, renamed, shrink, sizes } = await import("./downscale");

/** Each canvas's size, and the sizes each encode hands back in turn. */
let canvases: [number, number][] = [];
let results: number[] = [];
let writes: (type: string) => string = (type) => type;
const asked: [string, number][] = [];
class FakeCanvas {
  constructor(
    public width: number,
    public height: number,
  ) {
    canvases.push([width, height]);
  }
  getContext() {
    return { drawImage: () => {} };
  }
  convertToBlob(o: { type: string; quality: number }) {
    asked.push([o.type, o.quality]);
    return Promise.resolve(new Blob([new Uint8Array(results.shift() ?? 1000)], { type: writes(o.type) }));
  }
}
const bitmap = { width: 4000, height: 3000, close: vi.fn() };
const decode = vi.fn((_f: Blob) => Promise.resolve(bitmap));

beforeEach(() => {
  canvases = [];
  results = [];
  asked.length = 0;
  writes = (type) => type;
  bitmap.close.mockClear();
  decode.mockImplementation(() => Promise.resolve(bitmap));
  vi.stubGlobal("OffscreenCanvas", FakeCanvas);
  vi.stubGlobal("createImageBitmap", decode);
});
afterEach(() => vi.unstubAllGlobals());

describe("helpers", () => {
  test("names follow the encoding", () => {
    expect(renamed("IMG_1.heic", "image/jpeg")).toBe("IMG_1.jpg");
    expect(renamed("shot.png", "image/webp")).toBe("shot.webp");
    expect(renamed("noext", "image/png")).toBe("noext.png");
    expect(renamed(".png", "image/webp")).toBe("image.webp");
  });

  test("sizes: the long edge capped at 2048, then a fifth smaller each time", () => {
    expect(sizes(4000, 3000).slice(0, 2)).toEqual([
      [2048, 1536],
      [1638, 1229],
    ]);
    expect(sizes(100, 50)[0]).toEqual([100, 50]);
    expect(sizes(10, 10)).toHaveLength(8);
  });
});

describe("shrink", () => {
  test("a photo stays JPEG, made smaller until it fits; no EXIF survives a re-encode", async () => {
    results = [2 * MAX_BYTES, 2 * MAX_BYTES, MAX_BYTES];
    const file = await shrink(new File(["x"], "beach.jpg", { type: "image/jpeg" }));
    expect(asked).toEqual([
      ["image/jpeg", 0.9],
      ["image/jpeg", 0.75],
      ["image/jpeg", 0.9],
    ]);
    expect(canvases).toEqual([
      [2048, 1536],
      [1638, 1229],
    ]);
    expect([file.name, file.type, file.size]).toEqual(["beach.jpg", "image/jpeg", MAX_BYTES]);
    expect(bitmap.close).toHaveBeenCalled();
  });

  test("a PNG goes WebP, or PNG where the browser cannot write WebP", async () => {
    expect((await shrink(new File(["x"], "chart.png", { type: "image/png" }))).name).toBe("chart.webp");
    writes = () => "image/png";
    expect((await shrink(new File(["x"], "chart.png", { type: "image/png" }))).type).toBe("image/png");
  });

  test("a HEIC becomes JPEG where it decodes, and says so where it does not", async () => {
    const heic = new File(["x"], "IMG_7.HEIC", { type: "" });
    expect((await shrink(heic)).name).toBe("IMG_7.jpg");
    decode.mockImplementation(() => Promise.reject(new Error("unsupported")));
    await expect(shrink(heic)).rejects.toThrow(HEIC_TEXT);
  });

  test("other kinds are refused; one that never fits says so", async () => {
    await expect(shrink(new File(["x"], "scan.bmp", { type: "image/bmp" }))).rejects.toThrow("Only PNG, JPEG, WebP and GIF images can go in a chat.");
    results = Array(16).fill(2 * MAX_BYTES);
    await expect(shrink(new File(["x"], "huge.png", { type: "image/png" }))).rejects.toThrow("Too large to send, even made smaller.");
    expect(bitmap.close).toHaveBeenCalled();
  });
});

/** A bar whose state lives here, as the composer's would. */
function bar(room = 4, start: Picture[] = []) {
  let pictures = start;
  const fails: (string | null)[] = [];
  const b = {
    pictures,
    set: (f: Picture[] | ((p: Picture[]) => Picture[])) => (pictures = typeof f === "function" ? f(pictures) : f),
    fail: (w: string | null) => fails.push(w),
    workspace: "w1",
    room,
  };
  return { b, fails, now: () => pictures };
}
const jpeg = (name: string) => new File(["x"], name, { type: "image/jpeg" });

describe("the bar", () => {
  beforeEach(() => {
    uploaded = 0;
    upload.mockClear();
    get.mockClear();
  });

  test("each image is shrunk, then uploaded to Media as a chat image; its chip becomes the item", async () => {
    const { b, fails, now } = bar();
    await addImages([jpeg("a.jpg"), jpeg("b.jpg")], b);
    const [file, ws, , origin] = upload.mock.calls[0]!;
    expect([(file as File).name, ws, origin]).toEqual(["a.jpg", "w1", "chat"]);
    expect(now().map((p) => [p.name, p.media_id])).toEqual([
      ["a.jpg", "m1"],
      ["b.jpg", "m2"],
    ]);
    expect(fails).toEqual([null]);
  });

  test("past the room, 5 MB together, or the same picture again: refused or merged", async () => {
    const { b, fails, now } = bar(1);
    await addImages([jpeg("a.jpg"), jpeg("b.jpg")], b);
    expect(fails[0]).toBe("At most 4 images per message.");
    expect(now()).toHaveLength(1);

    const full = bar(4, [{ key: "k", name: "big.jpg", size: 4.5 * 1024 * 1024, media_id: "big" }]);
    results = [MAX_BYTES];
    await addImages([jpeg("c.jpg")], full.b);
    expect(full.fails.at(-1)).toBe("c.jpg: At most 5 MB of images per message.");
    expect(full.now()).toHaveLength(1);

    const twice = bar();
    await addImages([jpeg("same1.jpg"), jpeg("same2.jpg")], twice.b);
    expect(twice.now().map((p) => p.media_id)).toEqual(["same"]);
  });

  test("one that cannot go leaves no chip and says why", async () => {
    decode.mockImplementation(() => Promise.reject(new Error("bad")));
    const { b, fails, now } = bar();
    await addImages([jpeg("x.jpg")], b);
    expect(now()).toEqual([]);
    expect(fails.at(-1)).toBe("x.jpg: This browser can't read this image.");
  });

  test("Use in chat: taken once; gone items and what passes 5 MB are left out", async () => {
    sessionStorage.setItem("harness.pendingImages", JSON.stringify(["big", "gone", "m7", "big2"]));
    const { b, now } = bar();
    await pickUp(b.set);
    expect(sessionStorage.getItem("harness.pendingImages")).toBeNull();
    // big2 (another 4.5 MB) would take the message past 5 MB.
    expect(now().map((p) => p.media_id)).toEqual(["big", "m7"]);
    expect(get.mock.calls.map((c) => c[0])).toEqual(["big", "gone", "m7", "big2"]);
  });
});
