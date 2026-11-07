// What the Ask wall may show. The wall is decoration on the home screen,
// so a line that reads as the app taking a position, or as something
// personal, has no place on it: bodily functions, drugs and medication,
// illness, belief, identity, politics, crime and gossip about real people.
// The bundled lists were curated against this; it keeps them that way.
import { describe, expect, test } from "vitest";
import { parseList } from "./wallData";

const lists = import.meta.glob<string>("../../public/wall/**/*.txt", { query: "?raw", import: "default", eager: true });

import policy from "./wall-policy.json";

// The deny patterns live in wall-policy.json so the harvester
// (web/scripts/harvest_queries.py) filters with exactly the same rules.
const DENY: Record<string, RegExp> = Object.fromEntries(
  Object.entries(policy)
    .filter(([k]) => !k.startsWith("$"))
    .map(([k, w]) => [k, new RegExp(`\\b(?:${(w as string[]).join("|")})\\b`, "i")]),
);

const denied = (line: string) => Object.keys(DENY).filter((k) => DENY[k]!.test(line));

describe("wall lists", () => {
  const entries = Object.entries(lists).map(([path, text]) => [path.replace(/^.*\/wall\//, ""), parseList(text)] as const);

  test("the general list and all ten topic lists are bundled", () => {
    expect(entries.map(([name]) => name).sort()).toEqual([
      "queries.txt",
      ...["careers", "food", "health", "home", "language", "money", "robotics", "science", "software-development", "travel"].map(
        (t) => `topics/${t}.txt`,
      ),
    ]);
  });

  test.each(entries)("%s: nothing the wall must not show", (_, lines) => {
    expect(lines.filter((l) => denied(l).length).map((l) => `${l} (${denied(l).join(", ")})`)).toEqual([]);
  });

  // At least 1,000 general queries (the user's requirement), and enough per
  // topic for its share of the widest wall (64 lines, at most 60% topics).
  test.each(entries)("%s: still plenty to draw from", (name, lines) => {
    expect(lines.length).toBeGreaterThanOrEqual(name === "queries.txt" ? 1000 : 60);
  });

  test("the policy catches what it is meant to", () => {
    for (const s of ["why do farts smell", "what are xanax prescribed for", "what are jews", "why is netanyahu called bibi", "why is pancreatic cancer so deadly", "how to vote in the us", "why is diddy in jail"]) {
      expect(denied(s), s).not.toEqual([]);
    }
    for (const s of ["why is the sky blue", "why is gas so high", "diy ant killer", "how to tie a tie", "how to remove hard water stains from glass", "how to make a killing"]) {
      expect(denied(s), s).toEqual([]);
    }
  });
});
