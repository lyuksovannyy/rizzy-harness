import { describe, expect, it } from "vitest";
import { bucket, greeting } from "./greeting";
import { activeMention, filterAgents, insertMention, leadingMention } from "./mentions";
import { agenticBlockReason, cycleMode, enabledModes } from "./modes";
import { buildSearchUrl } from "./searchUrl";
import { titleFrom } from "./title";
import { t } from "./i18n";

const agents = [
  { id: "general", name: "General" },
  { id: "coder", name: "Coder" },
  { id: "writer", name: "Writer" },
  { id: "scout", name: "Scout" },
];

describe("greeting", () => {
  it("buckets hours", () => {
    expect([5, 11, 12, 17, 18, 22, 23, 0, 4].map(bucket)).toEqual([
      "morning", "morning", "afternoon", "afternoon", "evening", "evening", "night", "night", "night",
    ]);
  });
  it("includes the name, is stable within a day, and varies across days", () => {
    const a = greeting(new Date(2026, 10, 3, 9, 0), "alex");
    expect(a).toContain("alex");
    expect(greeting(new Date(2026, 10, 3, 10, 59), "alex")).toBe(a);
    const week = new Set(
      Array.from({ length: 7 }, (_, d) => greeting(new Date(2026, 10, 3 + d, 9), "alex")),
    );
    expect(week.size).toBeGreaterThan(1);
  });
});

describe("mentions", () => {
  it("detects the @token being typed at the caret", () => {
    expect(activeMention("@wr", 3)).toEqual({ start: 0, query: "wr" });
    expect(activeMention("hey @co", 7)).toEqual({ start: 4, query: "co" });
    expect(activeMention("mail@example", 12)).toBeNull();
    expect(activeMention("@writer draft", 13)).toBeNull();
    expect(activeMention("@", 1)).toEqual({ start: 0, query: "" });
  });
  it("filters by id prefix or name", () => {
    expect(filterAgents(agents, "w").map((a) => a.id)).toEqual(["writer"]);
    expect(filterAgents(agents, "").length).toBe(4);
    expect(filterAgents(agents, "SCO").map((a) => a.id)).toEqual(["scout"]);
  });
  it("inserts and places the caret after the mention", () => {
    expect(insertMention("@wr", 0, 3, "writer")).toEqual({ text: "@writer ", caret: 8 });
    expect(insertMention("hi @c please", 3, 5, "coder")).toEqual({ text: "hi @coder please", caret: 10 });
  });
  it("matches the server's leading-mention rule", () => {
    const ids = agents.map((a) => a.id);
    expect(leadingMention("@writer draft", ids)).toBe("writer");
    expect(leadingMention("  @coder, fix", ids)).toBe("coder");
    expect(leadingMention("@nobody hi", ids)).toBeNull();
    expect(leadingMention("email @writer", ids)).toBeNull();
  });
});

describe("modes", () => {
  it("cycles over enabled modes only, both directions", () => {
    const all = enabledModes(null);
    expect(all).toEqual(["search", "ask", "agentic"]);
    expect(cycleMode("ask", all, 1)).toBe("agentic");
    expect(cycleMode("agentic", all, 1)).toBe("search");
    expect(cycleMode("search", all, -1)).toBe("agentic");
    const noAgentic = enabledModes("no grant");
    expect(noAgentic).toEqual(["search", "ask"]);
    expect(cycleMode("ask", noAgentic, 1)).toBe("search");
  });
  it("explains why agentic is unavailable", () => {
    expect(agenticBlockReason(false, 1)).toMatch(/not enabled/);
    expect(agenticBlockReason(true, 0)).toMatch(/worker/);
    expect(agenticBlockReason(true, 2)).toBeNull();
  });
});

describe("search url and titles", () => {
  it("encodes the query", () => {
    expect(buildSearchUrl("https://duckduckgo.com/?q={q}", " rust & tokio ")).toBe(
      "https://duckduckgo.com/?q=rust%20%26%20tokio",
    );
  });
  it("makes short titles without the leading mention", () => {
    expect(titleFrom("@writer draft the changelog\nmore")).toBe("draft the changelog");
    expect(titleFrom("x".repeat(80)).length).toBeLessThanOrEqual(61);
    expect(titleFrom("   ")).toBe(t("session.untitled"));
  });
});
