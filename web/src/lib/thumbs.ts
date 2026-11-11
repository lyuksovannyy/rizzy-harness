// Thumbnails made by the owner's browser (docs/media.md §6.5): the server
// never decodes media, so the grid's small pictures are drawn here and sent
// back. Images through createImageBitmap, SVG and HEIC through an <img>
// (only where the browser renders them), a video's frame at min(1 s, a
// tenth) through a hidden <video>; audio and other files get an icon. Never
// a blob: URL, so the app's CSP stays as it is. Whatever fails is recorded
// (`no_thumb`), so it is never tried again.
import { keyed } from "../api";
import { link } from "./links";
import { contentUrl, type MediaItem, type MediaPatch, mediaApi, safeToDecode } from "./media";

/** Longest side of a thumbnail, and the most the server takes. */
export const THUMB_PX = 320;
export const MAX_THUMB = 64 * 1024;
/** A media element that says nothing for this long has failed. */
const WAIT_MS = 15_000;
/** How many are made at once, so scrolling a long grid stays smooth. */
const AT_ONCE = 2;

type Api = Pick<typeof mediaApi, "update" | "putThumb">;

function canvas(w: number, h: number): OffscreenCanvas | HTMLCanvasElement {
  if (typeof OffscreenCanvas !== "undefined") return new OffscreenCanvas(w, h);
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  return c;
}

function toBlob(c: OffscreenCanvas | HTMLCanvasElement, type: string, quality: number): Promise<Blob | null> {
  if ("convertToBlob" in c) return c.convertToBlob({ type, quality }).catch(() => null);
  return new Promise((resolve) => c.toBlob(resolve, type, quality));
}

/** `src` drawn at most THUMB_PX on its longest side, as WebP, else JPEG
 * (a browser that cannot write WebP hands back PNG), small enough to send. */
export async function encode(src: CanvasImageSource, w: number, h: number): Promise<Blob> {
  const scale = Math.min(1, THUMB_PX / Math.max(w, h, 1));
  const cw = Math.max(1, Math.round(w * scale));
  const ch = Math.max(1, Math.round(h * scale));
  const c = canvas(cw, ch);
  const ctx = c.getContext("2d") as CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null;
  if (!ctx) throw new Error("no 2D canvas");
  ctx.drawImage(src, 0, 0, cw, ch);
  for (const [type, q] of [
    ["image/webp", 0.8],
    ["image/jpeg", 0.8],
    ["image/jpeg", 0.6],
  ] as const) {
    const b = await toBlob(c, type, q);
    if (b && b.type === type && b.size <= MAX_THUMB) return b;
  }
  throw new Error("the thumbnail came out too large");
}

/** Resolves on the first of `ok`, rejects on `bad` or after WAIT_MS. */
function once(el: EventTarget, ok: string, bad = "error"): Promise<void> {
  return new Promise((resolve, reject) => {
    const t = setTimeout(() => done(new Error(`no ${ok}`)), WAIT_MS);
    const done = (err?: Error) => {
      clearTimeout(t);
      el.removeEventListener(ok, onOk);
      el.removeEventListener(bad, onBad);
      if (err) reject(err);
      else resolve();
    };
    const onOk = () => done();
    const onBad = () => done(new Error(bad));
    el.addEventListener(ok, onOk);
    el.addEventListener(bad, onBad);
  });
}

/** A raster image the browser decodes from its bytes. */
async function fromBitmap(m: MediaItem): Promise<Blob> {
  const res = await fetch(contentUrl(m.media_id), { credentials: "same-origin", headers: keyed() });
  if (!res.ok) throw new Error(`${res.status}`);
  const bmp = await createImageBitmap(await res.blob());
  try {
    return await encode(bmp, bmp.width, bmp.height);
  } finally {
    bmp.close();
  }
}

/** SVG and HEIC: shown only as an image (`Sec-Fetch-Dest: image`), so an
 * <img> loads it; the same origin keeps the canvas readable. */
async function fromImg(m: MediaItem): Promise<Blob> {
  // A link: an element cannot send the login key (P-1, lib/links).
  const src = await link(contentUrl(m.media_id));
  const img = new Image();
  img.decoding = "async";
  const loaded = once(img, "load");
  img.src = src;
  await loaded;
  return encode(img, img.naturalWidth || THUMB_PX, img.naturalHeight || THUMB_PX);
}

/** A media element's metadata (and, for video, a frame drawn). */
async function fromMedia(m: MediaItem, tag: "video" | "audio"): Promise<{ thumb: Blob | null; hints: MediaPatch }> {
  const src = await link(contentUrl(m.media_id));
  const el = document.createElement(tag);
  el.muted = true;
  el.preload = "metadata";
  if (el instanceof HTMLVideoElement) el.playsInline = true;
  const meta = once(el, "loadedmetadata");
  el.src = src;
  try {
    await meta;
    const secs = Number.isFinite(el.duration) ? el.duration : 0;
    const hints: MediaPatch = secs ? { duration_ms: Math.round(secs * 1000) } : {};
    if (!(el instanceof HTMLVideoElement)) return { thumb: null, hints };
    const seeked = once(el, "seeked");
    el.currentTime = Math.min(1, secs / 10);
    await seeked;
    const { videoWidth: w, videoHeight: h } = el;
    return { thumb: await encode(el, w, h), hints: { ...hints, ...(w && h ? { width: w, height: h } : {}) } };
  } finally {
    el.removeAttribute("src");
    el.load();
  }
}

/** One item's thumbnail and hints, sent to the server. "none": it gets an
 * icon (audio, a file, an image too big to decode); "failed": the browser
 * could not; both are marked so nobody tries again. */
export async function makeThumb(m: MediaItem, api: Api = mediaApi): Promise<"made" | "none" | "failed"> {
  try {
    if (m.kind === "video" || m.kind === "audio") {
      if (m.display === "download") {
        await api.update(m.media_id, { no_thumb: true });
        return "none";
      }
      const { thumb, hints } = await fromMedia(m, m.kind);
      if (thumb) await api.putThumb(m.media_id, thumb);
      await api.update(m.media_id, thumb ? hints : { ...hints, no_thumb: true });
      return thumb ? "made" : "none";
    }
    if (!safeToDecode(m)) {
      await api.update(m.media_id, { no_thumb: true });
      return "none";
    }
    await api.putThumb(m.media_id, m.display === "img" ? await fromImg(m) : await fromBitmap(m));
    return "made";
  } catch {
    await api.update(m.media_id, { no_thumb: true }).catch(() => {});
    return "failed";
  }
}

/** Whether the grid should still ask for one. */
export const wantsThumb = (m: MediaItem) => !m.has_thumb && !m.no_thumb;

const queue: { m: MediaItem; done: (r: Awaited<ReturnType<typeof makeThumb>>) => void }[] = [];
const queued = new Set<string>();
let busy = 0;

function pump() {
  while (busy < AT_ONCE && queue.length) {
    const job = queue.shift()!;
    busy++;
    void makeThumb(job.m).then((r) => {
      busy--;
      queued.delete(job.m.media_id);
      job.done(r);
      pump();
    });
  }
}

/** Makes `m`'s thumbnail when its turn comes (two at a time); once per item. */
export function queueThumb(m: MediaItem, done: (r: "made" | "none" | "failed") => void): void {
  if (!wantsThumb(m) || queued.has(m.media_id)) return;
  queued.add(m.media_id);
  queue.push({ m, done });
  pump();
}
