// Topic inference from chat titles: what counts as a match, how recency and
// the two-title threshold shape the weights.
import { describe, expect, test } from "vitest";
import { grams, matchTopics, topicWeights } from "./topics";

const DAY = 86_400_000;
const NOW = 1_800_000_000_000;

describe("grams", () => {
  test("code tokens keep their marks; accents and plurals fold", () => {
    const g = grams("Résumé tips: C++ vs C#, node.js and CI/CD pipelines.");
    for (const k of ["resume", "c++", "c#", "node.js", "ci/cd", "pipelines", "pipeline", "resume tips"]) expect(g.has(k)).toBe(true);
    expect(g.has("tips:")).toBe(false);
    // Short words keep their s; trailing dots go.
    expect(grams("gas bus").has("ga")).toBe(false);
    expect(grams("use vite.").has("vite")).toBe(true);
  });

  test("two- and three-word phrases", () => {
    const g = grams("Two weeks notice after a merge conflict");
    expect(g.has("two weeks notice")).toBe(true);
    expect(g.has("merge conflict")).toBe(true);
  });
});

describe("matchTopics", () => {
  test.each([
    ["Fix borrow checker error in Rust", ["software-development"]],
    ["PID tuning for my rover", ["robotics"]],
    ["Rewrite my resume for a PM job", ["careers"]],
    ["Cheap flights to Lisbon in May", ["travel"]],
    ["Sourdough starter smells like acetone", ["food"]],
    ["test results", []],
    ["Hello there", []],
  ])("%s", (title, topics) => {
    expect(matchTopics(title)).toEqual(topics);
  });

  test("two weak keywords make a match", () => {
    expect(matchTopics("server error")).toEqual(["software-development"]);
  });
});

describe("topicWeights", () => {
  const rows = (titles: string[], ageDays = 0) => titles.map((title) => ({ title, updated_ms: NOW - ageDays * DAY }));

  test("a topic needs two titles", () => {
    const one = topicWeights(rows(["Fix a bug in my React app", "PID tuning for my rover", "Cheap flights to Lisbon"]), NOW);
    expect(one.m).toBe(3);
    expect(one.p.size).toBe(0);
    const two = topicWeights(rows(["Fix a bug in my React app", "Debug a Python script", "PID tuning for my rover"]), NOW);
    expect([...two.p.keys()]).toEqual(["software-development"]);
    expect(two.p.get("software-development")).toBeCloseTo(1);
  });

  test("recent chats count more, and old ones never less than 0.15", () => {
    const old = rows(["Fix a bug in my React app", "Debug a Python script"], 365);
    const recent = rows(["PID tuning for my rover", "Arduino servo jitter"], 0);
    const w = topicWeights([...old, ...recent], NOW);
    expect(w.p.get("robotics")!).toBeGreaterThan(w.p.get("software-development")!);
    // Scores 2 vs 0.3, flattened by the power of 0.75.
    const ratio = w.p.get("robotics")! / w.p.get("software-development")!;
    expect(ratio).toBeCloseTo((2 / 0.3) ** 0.75, 1);
  });

  test("a title about two topics gives each half", () => {
    const w = topicWeights(rows(["Robot arm in Python", "Robot arm in Python 2", "Kinematics code"]), NOW);
    expect(w.p.has("robotics") && w.p.has("software-development")).toBe(true);
    expect([...w.p.values()].reduce((a, b) => a + b, 0)).toBeCloseTo(1);
  });

  test("no titles, no topics", () => {
    expect(topicWeights([], NOW)).toEqual({ m: 0, p: new Map() });
  });
});
