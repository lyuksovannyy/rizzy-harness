// Profile pictures, made small in the browser before they are sent (as chat
// images are, lib/downscale.ts): the avatar a centred square of at most
// 256 px, the banner a centred 3:1 crop of at most 1500 × 500, encoded again
// (WebP, else what the browser writes), so no EXIF or GPS goes along. The
// server checks the result's bytes and size whatever this does.

export type Slot = "avatar" | "banner";

/** The server's caps, in bytes. */
export const MAX_BYTES: Record<Slot, number> = { avatar: 256 * 1024, banner: 1024 * 1024 };
const BOX: Record<Slot, [number, number]> = { avatar: [256, 256], banner: [1500, 500] };

/** The crop of a `w` × `h` image to `slot`'s shape, and the size it is drawn at. */
export function frame(slot: Slot, w: number, h: number): { sx: number; sy: number; sw: number; sh: number; dw: number; dh: number } {
  const [bw, bh] = BOX[slot];
  const ratio = bw / bh;
  const sw = Math.min(w, Math.round(h * ratio));
  const sh = Math.min(h, Math.round(w / ratio));
  const scale = Math.min(1, bw / sw, bh / sh);
  return {
    sx: Math.floor((w - sw) / 2),
    sy: Math.floor((h - sh) / 2),
    sw,
    sh,
    dw: Math.max(1, Math.round(sw * scale)),
    dh: Math.max(1, Math.round(sh * scale)),
  };
}

/** `file` as `slot`'s picture, small enough for the server. */
export async function shrinkPicture(slot: Slot, file: File): Promise<Blob> {
  if (!/^image\/(png|jpeg|webp)$/.test(file.type)) throw new Error("Pick a PNG, JPEG or WebP picture.");
  const bmp = await createImageBitmap(file).catch(() => {
    throw new Error("This browser can't read this picture.");
  });
  try {
    const f = frame(slot, bmp.width, bmp.height);
    for (const shrink of [1, 0.8, 0.64]) {
      const [w, h] = [Math.max(1, Math.round(f.dw * shrink)), Math.max(1, Math.round(f.dh * shrink))];
      const c = new OffscreenCanvas(w, h);
      c.getContext("2d")!.drawImage(bmp, f.sx, f.sy, f.sw, f.sh, 0, 0, w, h);
      for (const quality of [0.88, 0.72]) {
        const b = await c.convertToBlob({ type: "image/webp", quality });
        if (b.size <= MAX_BYTES[slot]) return b;
      }
    }
  } finally {
    bmp.close();
  }
  throw new Error("Too large, even made smaller.");
}
