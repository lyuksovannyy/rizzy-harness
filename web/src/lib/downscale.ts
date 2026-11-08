// Images for a chat message (docs/media.md §6.6). The browser draws each one
// at most 2048 px on its long edge and encodes it again until it is at most
// 1.25 MB, so the file sent carries no EXIF or GPS. A HEIC photo goes only
// where this browser decodes it, as JPEG. The result is uploaded to Media as
// a chat image (the Media page makes its thumbnail, as for any upload). Also
// takes what "Use in chat" left for the home bar. Its own lazy chunk: the
// bar imports it on the first image, so the first load stays small.
import type { Dispatch, SetStateAction } from "react";
import { CHAT_BYTES, mediaApi, PENDING_IMAGES, upload } from "./media";

export const MAX_BYTES = 1.25 * 1024 * 1024;
/** Images per message. */
const MAX_IMAGES = 4;
export const HEIC_TEXT = "This browser can't read HEIC photos. Convert it to JPEG first.";

/** An image going with a message: uploading until it has its Media id. */
export interface Picture {
  key: string;
  name: string;
  size: number;
  media_id?: string;
}

/** The bar's side: its images now, how to change them and say what went
 * wrong, the workspace they go to, and how many more fit. */
export interface Bar {
  pictures: Picture[];
  set: Dispatch<SetStateAction<Picture[]>>;
  fail: (why: string | null) => void;
  workspace: string | null;
  room: number;
}

/** "IMG_1.heic" → "IMG_1.jpg" when the encoding changed. */
export const renamed = (name: string, type: string) => `${name.replace(/\.[^./]*$/, "") || "image"}.${type === "image/jpeg" ? "jpg" : type.slice(6)}`;

/** The sizes to try, largest first: the long edge capped at 2048, then a
 * fifth smaller each time. */
export function sizes(w: number, h: number): [number, number][] {
  const first = Math.min(1, 2048 / Math.max(w, h));
  return [0, 1, 2, 3, 4, 5, 6, 7].map((i) => [w, h].map((n) => Math.max(1, Math.round(n * first * 0.8 ** i))) as [number, number]);
}

/** The image re-encoded small enough to send. Photos (and HEIC) become
 * JPEG; the others WebP, which keeps transparency (a browser that cannot
 * write WebP hands back PNG). A GIF keeps its first frame. */
export async function shrink(file: File): Promise<File> {
  const heic = /^image\/hei[cf]$/.test(file.type) || /\.hei[cf]$/i.test(file.name);
  if (!heic && !/^image\/(png|jpeg|webp|gif)$/.test(file.type)) throw new Error("Only PNG, JPEG, WebP and GIF images can go in a chat.");
  const bmp = await createImageBitmap(file).catch(() => {
    throw new Error(heic ? HEIC_TEXT : "This browser can't read this image.");
  });
  try {
    const type = heic || file.type === "image/jpeg" ? "image/jpeg" : "image/webp";
    for (const [w, h] of sizes(bmp.width, bmp.height)) {
      const c = new OffscreenCanvas(w, h);
      c.getContext("2d")!.drawImage(bmp, 0, 0, w, h);
      for (const quality of [0.9, 0.75]) {
        const b = await c.convertToBlob({ type, quality });
        if (b.size <= MAX_BYTES) return new File([b], renamed(file.name, b.type), { type: b.type });
      }
    }
  } finally {
    bmp.close();
  }
  throw new Error("Too large to send, even made smaller.");
}

/** Adds images to the bar one at a time: each is shrunk, then uploaded to
 * the workspace's Media and its chip becomes the item (the same picture
 * twice is one image). Past the bar's room, or past 5 MB together, is
 * refused with a reason. */
export async function addImages(list: File[], bar: Bar): Promise<void> {
  bar.fail(list.length > bar.room ? `At most ${MAX_IMAGES} images per message.` : null);
  let bytes = bar.pictures.reduce((n, x) => n + x.size, 0);
  for (const f of list.slice(0, Math.max(0, bar.room))) {
    const key = `${f.name}:${Math.random()}`;
    bar.set((p) => [...p, { key, name: f.name, size: 0 }]);
    try {
      const small = await shrink(f);
      if (bytes + small.size > CHAT_BYTES) throw new Error("At most 5 MB of images per message.");
      const { media_id, name, size } = (await upload(small, bar.workspace, () => {}, "chat")).item;
      bytes += size;
      bar.set((p) => (p.some((x) => x.media_id === media_id) ? p.filter((x) => x.key !== key) : p.map((x) => (x.key === key ? { key, name, size, media_id } : x))));
    } catch (x) {
      bar.set((p) => p.filter((y) => y.key !== key));
      bar.fail(`${f.name}: ${x instanceof Error ? x.message : x}`);
    }
  }
}

/** The home bar takes what "Use in chat" left (this tab only), once; items
 * gone since, or past one message's 5 MB, are left out. */
export async function pickUp(set: Bar["set"]): Promise<void> {
  let ids: unknown = [];
  try {
    ids = JSON.parse(sessionStorage.getItem(PENDING_IMAGES) ?? "[]");
    sessionStorage.removeItem(PENDING_IMAGES);
  } catch {
    /* storage unavailable or not ours: nothing waits */
  }
  let bytes = 0;
  for (const id of Array.isArray(ids) ? ids.slice(0, MAX_IMAGES) : []) {
    const m = await mediaApi.get(String(id)).catch(() => null);
    if (!m || bytes + m.size > CHAT_BYTES) continue;
    bytes += m.size;
    set((p) => (p.some((x) => x.media_id === m.media_id) ? p : [...p, { key: m.media_id, name: m.name, size: m.size, media_id: m.media_id }]));
  }
}
