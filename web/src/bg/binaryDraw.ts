// Drawing the binary field on a canvas. Each digit is copied from a small
// pre-drawn atlas (two digits in the text colour, two in the streak colour)
// with a per-cell alpha, at whole device pixels: thousands of copies cost a
// millisecond or two, and after the first full draw only changed cells are
// redrawn.
import { CELL_H, CELL_W, type Field } from "./binaryField";

export type Ink = { fg: string; accent: string; artMax: number; artPeak: number; mono: string };

/** The theme's colours and art budget, read from the page's tokens. */
export function readInk(el: Element): Ink {
  const cs = getComputedStyle(el);
  const v = (k: string) => cs.getPropertyValue(k).trim();
  return {
    fg: v("--fg") || "#888",
    accent: v("--accent") || "#888",
    artMax: parseFloat(v("--art-max")) || 0.12,
    artPeak: parseFloat(v("--art-peak")) || 0.24,
    mono: v("--mono") || "monospace",
  };
}

export type Atlas = { canvas: HTMLCanvasElement; cw: number; ch: number };

export function makeAtlas(ink: Ink, dpr: number): Atlas {
  const cw = Math.round(CELL_W * dpr);
  const ch = Math.round(CELL_H * dpr);
  const canvas = document.createElement("canvas");
  canvas.width = cw * 4;
  canvas.height = ch;
  const ctx = canvas.getContext("2d");
  if (!ctx) return { canvas, cw, ch };
  ctx.font = `500 ${Math.round(12 * dpr)}px ${ink.mono}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillStyle = ink.fg;
  ["0", "1", "0", "1"].forEach((d, k) => ctx.fillText(d, (k + 0.5) * cw, ch / 2 + dpr * 0.5));
  // The streak colour: 60% text colour, 40% accent, on the last two glyphs.
  ctx.globalCompositeOperation = "source-atop";
  ctx.globalAlpha = 0.4;
  ctx.fillStyle = ink.accent;
  ctx.fillRect(cw * 2, 0, cw * 2, ch);
  return { canvas, cw, ch };
}

/** Sizes a canvas to the field at device resolution; returns its 2D context. */
export function fitCanvas(c: HTMLCanvasElement, f: Field, a: Atlas, dpr: number): CanvasRenderingContext2D | null {
  c.width = f.cols * a.cw;
  c.height = f.rows * a.ch;
  c.style.width = `${c.width / dpr}px`;
  c.style.height = `${c.height / dpr}px`;
  const ctx = c.getContext("2d");
  if (ctx) ctx.imageSmoothingEnabled = false;
  return ctx;
}

/**
 * Draws `cells` (all of them when null) at the field's alpha times `gain`,
 * at most `cap`. The spotlight's copy uses a gain of 2.6 up to --art-peak.
 */
export function drawCells(ctx: CanvasRenderingContext2D, a: Atlas, f: Field, cells: ArrayLike<number> | null, gain = 1, cap = 1): void {
  if (!cells) ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  const n = cells ? cells.length : f.cols * f.rows;
  for (let k = 0; k < n; k++) {
    const i = cells ? cells[k]! : k;
    const x = (i % f.cols) * a.cw;
    const y = ((i / f.cols) | 0) * a.ch;
    if (cells) ctx.clearRect(x, y, a.cw, a.ch);
    const alpha = Math.min(cap, f.alpha[i]! * gain);
    if (alpha < 0.004) continue;
    ctx.globalAlpha = alpha;
    ctx.drawImage(a.canvas, (f.hot[i]! * 2 + f.value[i]!) * a.cw, 0, a.cw, a.ch, x, y, a.cw, a.ch);
  }
  ctx.globalAlpha = 1;
}
