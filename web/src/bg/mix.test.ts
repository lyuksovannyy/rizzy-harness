// Mixing general and topic lines: the share grows with matched titles,
// quotas add up, and the user's own titles never come back.
import { describe, expect, test } from "vitest";
import { createMixer, normalize, quotas, share } from "./mix";
import { mulberry32 } from "./rng";
import type { Topic } from "./topics";
import { queries } from "./testkit";

describe("share", () => {
  test.each([[0, 0], [2, 0], [16, 0.3], [30, 0.6], [100, 0.6]])("%i matched titles → %f", (m, s) => {
    expect(share(m)).toBeCloseTo(s);
  });
});

describe("quotas", () => {
  test("whole lines per topic, by largest remainder", () => {
    const p = new Map<Topic, number>([["robotics", 0.5], ["careers", 0.3], ["food", 0.2]]);
    const q = quotas(24, 0.3, p);
    expect([...q.values()].reduce((a, b) => a + b, 0)).toBe(7);
    expect(q.get("robotics")).toBe(4);
    expect(quotas(24, 0.3, new Map()).size).toBe(0);
  });
});

describe("normalize", () => {
  test("case, accents, punctuation, spacing and a trailing ellipsis", () => {
    expect(normalize("  How to TIE a tie?! ")).toBe("how to tie a tie");
    expect(normalize("Café  crème…")).toBe("cafe creme");
  });
});

describe("createMixer", () => {
  const general = queries(300, 1);
  const robotics = queries(60, 2).map((q) => `robot ${q}`);

  test("no history: every line is general", () => {
    const m = createMixer({ rng: mulberry32(1), general, topics: new Map(), p: new Map(), share: 0, titles: [] });
    const texts = m.plan(30).map((d) => d.take(44)!);
    expect(texts.every((t) => general.includes(t))).toBe(true);
  });

  test("the plan holds the topic quota and N lines in all", () => {
    const p = new Map<Topic, number>([["robotics", 1]]);
    const m = createMixer({ rng: mulberry32(1), general, topics: new Map([["robotics", robotics]]), p, share: 0.3, titles: [] });
    const texts = m.plan(30).map((d) => d.take(44)!);
    expect(texts).toHaveLength(30);
    expect(texts.filter((t) => robotics.includes(t))).toHaveLength(9);
  });

  test("never the user's own title, never a text twice at once", () => {
    const mine = general.slice(0, 50).map((t) => `${t.toUpperCase()}?`);
    const m = createMixer({ rng: mulberry32(3), general, topics: new Map(), p: new Map(), share: 0, titles: mine });
    const banned = new Set(mine.map(normalize));
    const seen = new Set<string>();
    for (let i = 0; i < 250; i++) {
      const t = m.draw().take(44);
      expect(t).not.toBeNull();
      expect(banned.has(normalize(t!))).toBe(false);
      expect(seen.has(t!)).toBe(false);
      seen.add(t!);
    }
    // Everything else is on the wall now: nothing is left to give.
    expect(m.draw().take(44)).toBeNull();
    m.release([...seen][0]!);
    expect(m.draw().take(44)).toBe([...seen][0]);
  });

  test("a deck skips texts that are too long and keeps them for later", () => {
    const m = createMixer({ rng: mulberry32(4), general: ["short query here", "a much longer query that fits only far away"], topics: new Map(), p: new Map(), share: 0, titles: [] });
    expect(m.draw().take(20)).toBe("short query here");
    expect(m.draw().take(20)).toBeNull();
    expect(m.draw().take(44)).toBe("a much longer query that fits only far away");
  });

  test("replacements draw topics at share × p", () => {
    const p = new Map<Topic, number>([["robotics", 1]]);
    const m = createMixer({ rng: mulberry32(5), general, topics: new Map([["robotics", robotics]]), p, share: 0.6, titles: [] });
    let topic = 0;
    for (let i = 0; i < 1000; i++) {
      const d = m.draw();
      const t = d.take(44)!;
      if (robotics.includes(t)) topic++;
      m.release(t);
    }
    expect(topic / 1000).toBeGreaterThan(0.54);
    expect(topic / 1000).toBeLessThan(0.66);
  });
});
