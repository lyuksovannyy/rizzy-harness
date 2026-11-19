// The trigger rule's mirror (lib/mentionScan.ts) against the cases the Rust
// rule (crates/core/src/mention.rs) is tested with: one fixture, written by
// hand, read by both.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { MAX_CALLED, resolver, scanMentions } from "./mentionScan";

interface Case {
  name: string;
  text: string;
  called: string[];
  found: { text: string; who: string; trigger: boolean }[];
}
const fixture = JSON.parse(readFileSync(new URL("../__fixtures__/mentions.json", import.meta.url), "utf8")) as {
  agents: Record<string, string>;
  tags: Record<string, string>;
  person: string;
  cases: Case[];
};

const resolve = (n: string) =>
  n in fixture.agents ? `agent:${fixture.agents[n]}` : n in fixture.tags ? `tag:${fixture.tags[n]}` : n === fixture.person ? "person" : null;

describe("scanMentions", () => {
  it("has the cases", () => expect(fixture.cases.length).toBeGreaterThanOrEqual(50));
  for (const c of fixture.cases) {
    it(c.name, () => {
      const m = scanMentions(c.text, resolve);
      expect(m.called).toEqual(c.called);
      expect(m.found.map((f) => ({ text: c.text.slice(f.start, f.end), who: f.who, trigger: f.trigger }))).toEqual(c.found);
    });
  }

  it("is one pass: a hundred thousand mentions, and the list stays bounded", () => {
    const t0 = Date.now();
    expect(scanMentions("@coder ".repeat(100_000), resolve).called).toEqual(["coder"]);
    scanMentions(`${"word @a ".repeat(100_000)}\n${"@a ".repeat(50_000)}`, resolve);
    scanMentions("` ".repeat(100_000), resolve);
    expect(Date.now() - t0).toBeLessThan(5000);
    const five = scanMentions("@a @b @c @d @e @f @g", (n) => `agent:${n}`);
    expect(five.called.length).toBe(MAX_CALLED + 1);
  });

  it("resolves from the agent list: an agent that runs only as a sub-agent is a tag, one that is neither is text", () => {
    const r = resolver(
      [
        { id: "coder", features: { as_agent: true, as_subagent: true } },
        { id: "advisor", features: { as_agent: false, as_subagent: true } },
        { id: "off", features: { as_agent: false, as_subagent: false } },
      ],
      "alex",
    );
    const m = scanMentions("@coder @advisor @off hi", r);
    expect(m.found.map((f) => [f.who, f.trigger])).toEqual([["agent:coder", true], ["tag:advisor", false]]);
    expect(m.called).toEqual(["coder"]);
  });

  it("resolves an agent's alias to its id and the person to the person", () => {
    const r = resolver([{ id: "coder", aliases: ["dev"] }, { id: "writer" }], "alex");
    expect(scanMentions("@dev @alex hi", r).found.map((f) => f.who)).toEqual(["agent:coder", "person"]);
    expect(scanMentions("@bob hi", r).found).toEqual([]);
  });
});
