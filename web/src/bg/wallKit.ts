// What the Ask wall and its still copy beside chats (ChatGutter) share: the
// lists to draw from, text measuring, and laying out a fresh wall.
import { useEffect, useRef, useState } from "react";
import type { Zones } from "./geom";
import { createMixer, quotas, share, type Mixer } from "./mix";
import { placeWall, type Placed, type Tier } from "./place";
import { mulberry32 } from "./rng";
import { topicWeights, type TitleRow, type Topic } from "./topics";
import { GENERAL_LIST, loadList, topicList } from "./wallData";

export type WallData = {
  general: string[];
  topics: Map<Topic, string[]>;
  p: Map<Topic, number>;
  share: number;
  titles: string[];
  /** The lists took over 4 s: the wall just fades in, no ripple. */
  late: boolean;
};

/**
 * The general list plus the topic lists the first `n` lines need, fetched
 * once per page load. What the wall starts with is kept: titles that change
 * while it shows do not reshuffle it.
 */
export function useWallData(titles: readonly TitleRow[], n: number): WallData | null {
  const [data, setData] = useState<WallData | null>(null);
  const start = useRef({ titles, n });
  useEffect(() => {
    let alive = true;
    const t0 = performance.now();
    const { titles, n } = start.current;
    const { m, p } = topicWeights(titles, Date.now());
    const s = share(m);
    const need = [...quotas(n, s, p)].filter(([, q]) => q > 0).map(([t]) => t);
    const lists = need.map((t) => loadList(topicList(t)).catch(() => [] as string[]));
    Promise.all([loadList(GENERAL_LIST), ...lists]).then(
      ([general, ...topics]) => {
        if (!alive) return;
        setData({
          general,
          topics: new Map(need.map((t, i) => [t, topics[i]!])),
          p,
          share: s,
          titles: titles.map((r) => r.title),
          late: performance.now() - t0 > 4000,
        });
      },
      () => {},
    );
    return () => {
      alive = false;
    };
  }, []);
  return data;
}

const widths = new Map<string, number>();
let ctx: CanvasRenderingContext2D | null | undefined;
let font = "";

/** A line's width in CSS px, as the wall's font draws it (cached). */
export function measureText(text: string, px: number): number {
  const key = `${px}|${text}`;
  let w = widths.get(key);
  if (w === undefined) {
    if (ctx === undefined) ctx = document.createElement("canvas").getContext("2d");
    font ||= getComputedStyle(document.body).fontFamily;
    if (!ctx) return text.length * px * 0.55;
    ctx.font = `400 ${px}px ${font}`;
    // The wall's letter-spacing is -0.005em.
    w = ctx.measureText(text).width - 0.005 * px * text.length;
    widths.set(key, w);
  }
  return w;
}

/** A fresh wall for `seed`: the same seed, size and zones give the same lines. */
export function layWall(data: WallData, seed: number, zones: Zones, tiers: readonly Tier[], n: number) {
  const rng = mulberry32(seed);
  const mixer: Mixer = createMixer({ rng, ...data });
  const plan = mixer.plan(n);
  let i = 0;
  const lines: Placed[] = placeWall({ rng, zones, tiers, measure: measureText, n, next: () => plan[i++] ?? mixer.draw() });
  return { lines, mixer, rng };
}
