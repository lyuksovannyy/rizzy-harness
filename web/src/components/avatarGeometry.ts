// Test helper: the avatar bodies as polygons, so a test can ask whether an
// eye lies inside one. Handles the commands the bodies use (M L H V C S Q T A Z,
// absolute and relative, implicit repeats); curves are sampled.
export type Pt = [number, number];

const ARGS: Record<string, number> = { m: 2, l: 2, h: 1, v: 1, c: 6, s: 4, q: 4, t: 2, a: 7, z: 0 };

function arc(p0: Pt, [rx0, ry0, rot, large, sweep, x, y]: number[], out: Pt[]) {
  // SVG endpoint-to-centre conversion (F.6.5); the bodies use circular arcs.
  let [rx, ry] = [Math.abs(rx0!), Math.abs(ry0!)];
  const phi = (rot! * Math.PI) / 180;
  const [c, s] = [Math.cos(phi), Math.sin(phi)];
  const dx = (p0[0] - x!) / 2;
  const dy = (p0[1] - y!) / 2;
  const x1 = c * dx + s * dy;
  const y1 = -s * dx + c * dy;
  const lam = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
  if (lam > 1) {
    rx *= Math.sqrt(lam);
    ry *= Math.sqrt(lam);
  }
  const num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
  const den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
  const k = (large === sweep ? -1 : 1) * Math.sqrt(Math.max(0, num / den));
  const cx1 = (k * rx * y1) / ry;
  const cy1 = (-k * ry * x1) / rx;
  const cx = c * cx1 - s * cy1 + (p0[0] + x!) / 2;
  const cy = s * cx1 + c * cy1 + (p0[1] + y!) / 2;
  const ang = (ux: number, uy: number, vx: number, vy: number) => Math.atan2(ux * vy - uy * vx, ux * vx + uy * vy);
  const t1 = ang(1, 0, (x1 - cx1) / rx, (y1 - cy1) / ry);
  let dt = ang((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
  if (!sweep && dt > 0) dt -= 2 * Math.PI;
  if (sweep && dt < 0) dt += 2 * Math.PI;
  for (let i = 1; i <= 24; i++) {
    const t = t1 + (dt * i) / 24;
    out.push([c * rx * Math.cos(t) - s * ry * Math.sin(t) + cx, s * rx * Math.cos(t) + c * ry * Math.sin(t) + cy]);
  }
}

/** The closed outlines (one polygon per subpath) of a body path. */
export function polygons(d: string): Pt[][] {
  const tokens = d.match(/[a-zA-Z]|-?\d*\.?\d+(?:e-?\d+)?/g)!;
  const polys: Pt[][] = [];
  let poly: Pt[] = [];
  let cur: Pt = [0, 0];
  let start: Pt = [0, 0];
  let i = 0;
  let cmd = "";
  let prev: Pt = [0, 0]; // the last cubic control point, for S
  const num = () => Number(tokens[i++]);
  const flush = () => {
    if (poly.length > 2) polys.push(poly);
    poly = [];
  };
  while (i < tokens.length) {
    if (/[a-zA-Z]/.test(tokens[i]!)) cmd = tokens[i++]!;
    const rel = cmd === cmd.toLowerCase();
    const lower = cmd.toLowerCase();
    if (!(lower in ARGS)) throw new Error(`avatarGeometry: unsupported path command ${cmd}`);
    if (lower === "z") {
      cur = start;
      flush();
      continue;
    }
    const a = Array.from({ length: ARGS[lower]! }, num);
    const [ox, oy] = rel ? cur : [0, 0];
    if (lower === "m") {
      flush();
      cur = [a[0]! + ox, a[1]! + oy];
      start = cur;
      poly.push(cur);
      cmd = rel ? "l" : "L";
    } else if (lower === "l") {
      cur = [a[0]! + ox, a[1]! + oy];
      poly.push(cur);
    } else if (lower === "h") {
      cur = [a[0]! + ox, cur[1]];
      poly.push(cur);
    } else if (lower === "v") {
      cur = [cur[0], a[0]! + oy];
      poly.push(cur);
    } else if (lower === "q" || lower === "c" || lower === "s" || lower === "t") {
      const pts: Pt[] = [cur];
      if (lower === "s" || lower === "t") pts.push([2 * cur[0] - prev[0], 2 * cur[1] - prev[1]]);
      for (let j = 0; j < a.length; j += 2) pts.push([a[j]! + ox, a[j + 1]! + oy]);
      for (let t = 1; t <= 16; t++) {
        let q = pts;
        while (q.length > 1) q = q.slice(1).map((p, j) => [q[j]![0] + ((p[0] - q[j]![0]) * t) / 16, q[j]![1] + ((p[1] - q[j]![1]) * t) / 16] as Pt);
        poly.push(q[0]!);
      }
      prev = pts[pts.length - 2]!;
      cur = pts[pts.length - 1]!;
    } else if (lower === "a") {
      const end: Pt = [a[5]! + ox, a[6]! + oy];
      arc(cur, [a[0]!, a[1]!, a[2]!, a[3]!, a[4]!, end[0], end[1]], poly);
      cur = end;
    }
  }
  flush();
  return polys;
}

/** Even-odd containment over all subpaths (the mug's handle is a hole). */
export function inside(polys: Pt[][], [x, y]: Pt): boolean {
  let n = 0;
  for (const p of polys) {
    for (let i = 0, j = p.length - 1; i < p.length; j = i++) {
      const [xi, yi] = p[i]!;
      const [xj, yj] = p[j]!;
      if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) n++;
    }
  }
  return n % 2 === 1;
}

/** Distance from a point to the nearest outline segment. */
export function edgeDistance(polys: Pt[][], [x, y]: Pt): number {
  let best = Infinity;
  for (const p of polys) {
    for (let i = 0; i < p.length; i++) {
      const [ax, ay] = p[i]!;
      const [bx, by] = p[(i + 1) % p.length]!;
      const [vx, vy] = [bx - ax, by - ay];
      const t = Math.max(0, Math.min(1, ((x - ax) * vx + (y - ay) * vy) / (vx * vx + vy * vy || 1)));
      best = Math.min(best, Math.hypot(x - (ax + t * vx), y - (ay + t * vy)));
    }
  }
  return best;
}

export function bbox(polys: Pt[][]) {
  const all = polys.flat();
  const xs = all.map((p) => p[0]);
  const ys = all.map((p) => p[1]);
  return { x0: Math.min(...xs), x1: Math.max(...xs), y0: Math.min(...ys), y1: Math.max(...ys) };
}

/** Points on the outline of a pill of half-sizes rx, ry centred on c. */
export function pill([cx, cy]: Pt, rx: number, ry: number): Pt[] {
  const r = Math.min(rx, ry);
  const [hx, hy] = [rx - r, ry - r]; // the straight half-lengths
  const out: Pt[] = [];
  for (let i = 0; i < 32; i++) {
    const t = (i / 32) * 2 * Math.PI;
    out.push([cx + Math.sign(Math.cos(t)) * hx + r * Math.cos(t), cy + Math.sign(Math.sin(t)) * hy + r * Math.sin(t)]);
  }
  return out;
}
