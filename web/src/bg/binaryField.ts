// Agentic mode's field of 0s and 1s, as data: which digit each cell shows and
// how strongly. BinaryBg draws it. Soft clouds of density come from value
// noise. The field fades out around the content itself (the greeting's
// words, the composer) and is only quieter across the rest of the column,
// so it reads as a background the content sits on, not a frame around it.
// Life is slow: 1.2% of cells flip each second, and a few streaks run down
// like rain.
import { between, mulberry32 } from "./rng";
import type { Rect } from "./geom";

/** The cell in CSS px. The drawn cell is rounded to whole device pixels, so
 *  BinaryBg passes the field that pitch: cells are masked where they land. */
export const CELL_W = 14;
export const CELL_H = 16;
/** Sharper than 1.5x costs more than it shows at this alpha. */
export const capDpr = (dpr: number) => Math.min(dpr || 1, 1.5);
export const gridSize = (w: number, h: number, cw = CELL_W, ch = CELL_H) => ({ cols: Math.ceil(w / cw), rows: Math.ceil(h / ch) });
export const maxStreaks = (cols: number) => (cols < 30 ? 1 : cols < 60 ? 2 : 3);

export const FLIP_RATE = 0.012;
const FLIP_OUT = 180;
const FLIP_IN = 220;
const MAX_FLIPS = 256;

const smooth = (t: number) => t * t * (3 - 2 * t);

/** Seeded 2-octave value noise in [0, 1], in cell units. */
export function valueNoise(seed: number): (x: number, y: number) => number {
  const at = (ix: number, iy: number) => {
    let h = (Math.imul(ix, 374761393) + Math.imul(iy, 668265263) + seed) | 0;
    h = Math.imul(h ^ (h >>> 13), 1274126177);
    return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
  };
  const octave = (x: number, y: number) => {
    const ix = Math.floor(x);
    const iy = Math.floor(y);
    const fx = smooth(x - ix);
    const fy = smooth(y - iy);
    const top = at(ix, iy) + (at(ix + 1, iy) - at(ix, iy)) * fx;
    const bottom = at(ix, iy + 1) + (at(ix + 1, iy + 1) - at(ix, iy + 1)) * fx;
    return top + (bottom - top) * fy;
  };
  return (x, y) => (octave(x / 12, y / 12) + 0.35 * octave(x / 5 + 31, y / 5 + 17)) / 1.35;
}

/** How far the field takes to come back beside content, and above or below it. */
export const FADE_X = 72;
export const FADE_Y = 40;
/** The column's share of full strength, reached again CALM_FADE px outside it. */
export const CALM = 0.55;
export const CALM_FADE = 120;

/**
 * How much of the field shows at a point. Nothing on a `clear` rect, rising
 * to full strength along an ellipse FADE_X px to the side and FADE_Y px
 * above and below, so a wide composer does not clear the whole row. Inside
 * `calm` (the column) everything is at CALM of that, rising to full over
 * CALM_FADE px outside it.
 */
export function fieldMask(px: number, py: number, clear: readonly Rect[], calm?: Rect | null): number {
  let m = 1;
  for (const r of clear) {
    const ex = Math.max(r.x - px, 0, px - (r.x + r.w)) / FADE_X;
    const ey = Math.max(r.y - py, 0, py - (r.y + r.h)) / FADE_Y;
    const e = Math.hypot(ex, ey);
    if (e < 1) m = Math.min(m, smooth(e));
  }
  if (calm && m > 0) {
    const d = Math.hypot(Math.max(calm.x - px, 0, px - (calm.x + calm.w)), Math.max(calm.y - py, 0, py - (calm.y + calm.h)));
    if (d < CALM_FADE) m *= CALM + (1 - CALM) * smooth(d / CALM_FADE);
  }
  return m;
}

type Streak = { col: number; head: number; speed: number; tail: number; row: number };

export class Field {
  readonly cols: number;
  readonly rows: number;
  /** The digit in each cell. */
  readonly value: Uint8Array;
  /** Resting alpha: --art-max shaped by the noise and the zone mask. */
  readonly base: Float32Array;
  /** What to draw now. */
  readonly alpha: Float32Array;
  /** 1 where a streak's head shows, drawn in the streak colour. */
  readonly hot: Uint8Array;
  /** The mask, so a streak's peak fades near the content too. */
  private mask: Float32Array;
  /** The pitch in CSS px. */
  readonly cw: number;
  readonly ch: number;
  private peak: number;
  /** Cells that changed since the last `takeDirty()`. */
  private dirty: Int32Array;
  private dirtyN = 0;
  private marked: Uint8Array;
  private open: Int32Array;
  private flips = new Map<number, number>();
  private streaks: Streak[] = [];
  private rng: () => number;
  private acc = 0;
  private untilStreak: number;
  /** Lowered by BinaryBg when ticks run slow. */
  flipRate = FLIP_RATE;
  streakLimit: number;

  constructor(o: {
    w: number;
    h: number;
    /** Where nothing shows: see fieldMask. */
    clear: readonly Rect[];
    calm?: Rect | null;
    seed: number;
    artMax: number;
    artPeak: number;
    /** The pitch in CSS px; CELL_W × CELL_H unless the atlas rounded it. */
    cw?: number;
    ch?: number;
  }) {
    this.cw = o.cw ?? CELL_W;
    this.ch = o.ch ?? CELL_H;
    const { cols, rows } = gridSize(o.w, o.h, this.cw, this.ch);
    this.cols = cols;
    this.rows = rows;
    const n = cols * rows;
    this.rng = mulberry32(o.seed);
    const noise = valueNoise(o.seed);
    this.value = new Uint8Array(n);
    this.base = new Float32Array(n);
    this.alpha = new Float32Array(n);
    this.hot = new Uint8Array(n);
    this.mask = new Float32Array(n);
    this.dirty = new Int32Array(n);
    this.marked = new Uint8Array(n);
    const open: number[] = [];
    for (let i = 0; i < n; i++) {
      const c = i % cols;
      const r = (i / cols) | 0;
      this.value[i] = this.rng() < 0.5 ? 0 : 1;
      const mask = fieldMask((c + 0.5) * this.cw, (r + 0.5) * this.ch, o.clear, o.calm);
      this.mask[i] = mask;
      // Value noise bunches around 0.5; stretching it makes the clouds read.
      const cloud = smooth(Math.min(1, Math.max(0, (noise(c, r) - 0.2) / 0.6)));
      this.base[i] = o.artMax * (0.45 + 0.55 * cloud) * mask;
      this.alpha[i] = this.base[i]!;
      if (mask > 0) open.push(i);
    }
    this.open = Int32Array.from(open);
    this.peak = o.artPeak;
    this.streakLimit = maxStreaks(cols);
    this.untilStreak = between(this.rng, 1400, 3200);
  }

  /** The mask at a point in CSS px (1 off the field). */
  maskAt(x: number, y: number): number {
    const c = Math.floor(x / this.cw);
    const r = Math.floor(y / this.ch);
    return c < 0 || r < 0 || c >= this.cols || r >= this.rows ? 1 : this.mask[r * this.cols + c]!;
  }

  /** Cells that may show anything at all. */
  get visible() {
    return this.open.length;
  }
  get activeFlips() {
    return this.flips.size;
  }
  get activeStreaks() {
    return this.streaks.length;
  }

  private mark(i: number) {
    if (this.marked[i]) return;
    this.marked[i] = 1;
    this.dirty[this.dirtyN++] = i;
  }

  /** The changed cells, once: the caller redraws exactly these. */
  takeDirty(): Int32Array {
    const out = this.dirty.subarray(0, this.dirtyN);
    for (const i of out) this.marked[i] = 0;
    this.dirtyN = 0;
    return out;
  }

  /** Advances the field by `dt` ms; returns how many flips started. */
  step(dt: number): number {
    let started = 0;
    this.acc += this.open.length * this.flipRate * (dt / 1000);
    while (this.acc >= 1) {
      this.acc -= 1;
      if (this.flips.size >= MAX_FLIPS || !this.open.length) continue;
      const i = this.open[(this.rng() * this.open.length) | 0]!;
      if (this.flips.has(i)) continue;
      this.flips.set(i, 0);
      started++;
    }
    for (const [i, t0] of this.flips) {
      const t = t0 + dt;
      if (t0 < FLIP_OUT && t >= FLIP_OUT) this.value[i] = 1 - this.value[i]!;
      if (t >= FLIP_OUT + FLIP_IN) this.flips.delete(i);
      else this.flips.set(i, t);
      this.alpha[i] = this.base[i]! * (t < FLIP_OUT ? 1 - t / FLIP_OUT : Math.min(1, (t - FLIP_OUT) / FLIP_IN));
      this.mark(i);
    }
    this.untilStreak -= dt;
    if (this.untilStreak <= 0) {
      this.untilStreak = between(this.rng, 1400, 3200);
      this.spawn();
    }
    this.streaks = this.streaks.filter((s) => this.advance(s, dt));
    return started;
  }

  /** A new streak in an open column (the best of four), if the limit allows. */
  spawn(): void {
    if (this.streaks.length >= this.streakLimit || !this.open.length) return;
    let col = 0;
    let best = -1;
    for (let k = 0; k < 4; k++) {
      const c = (this.rng() * this.cols) | 0;
      let room = 0;
      for (let r = 0; r < this.rows; r++) room += this.base[r * this.cols + c]!;
      if (room > best) [best, col] = [room, c];
    }
    const start = Math.floor(between(this.rng, -4, this.rows * 0.4));
    this.streaks.push({ col, head: start, row: start - 1, speed: between(this.rng, 14, 22), tail: Math.floor(between(this.rng, 8, 17)) });
  }

  /** Moves a streak down; the cells its head passes get new digits. False once it has left. */
  private advance(s: Streak, dt: number): boolean {
    s.head += (s.speed * dt) / 1000;
    const row = Math.floor(s.head);
    if (row === s.row) return true;
    for (let r = s.row + 1; r <= row; r++) if (r >= 0 && r < this.rows) this.value[r * this.cols + s.col] = this.rng() < 0.5 ? 0 : 1;
    // Repaint from just behind the old tail to the new head.
    for (let r = s.row - s.tail - 1; r <= row; r++) {
      if (r < 0 || r >= this.rows) continue;
      const i = r * this.cols + s.col;
      const k = row - r;
      if (this.flips.has(i)) continue;
      const b = this.base[i]!;
      const lit = k <= s.tail && b > 0;
      this.alpha[i] = lit ? b + (this.peak * this.mask[i]! - b) * 0.82 ** k : b;
      this.hot[i] = lit && k === 0 ? 1 : 0;
      this.mark(i);
    }
    s.row = row;
    return row - s.tail <= this.rows;
  }
}
