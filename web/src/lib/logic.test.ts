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
  it("uses the given time zone", () => {
    // 2026-11-03 07:30 UTC is 09:30 in Riga and 23:30 the day before in Los Angeles.
    const t = new Date(Date.UTC(2026, 10, 3, 7, 30));
    expect(greeting(t, "alex", "Europe/Riga")).toMatch(/morning|Morning|Rise/);
    expect(greeting(t, "alex", "America/Los_Angeles")).toMatch(/late|midnight|Still here/);
    expect(greeting(t, "alex", "Not/AZone")).toContain("alex");
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
    // N.10a part 2: the account's level first, then the level in the
    // workspace (already the lower of the two), then the workers.
    const sandbox = { name: "Acme", personal: false, exec_level: "sandbox" as const };
    const none = { name: "Acme", personal: false, exec_level: "none" as const };
    expect(agenticBlockReason("none", sandbox, 1)).toBe("Your account's level is none: no agent chats. Ask a server admin.");
    expect(agenticBlockReason("host", sandbox, 0)).toMatch(/worker/);
    expect(agenticBlockReason("sandbox", sandbox, 2)).toBeNull();
    expect(agenticBlockReason("host", none, 0)).toBe("Your level in Acme is none: no agent chats. Ask one of its admins.");
    expect(agenticBlockReason("container", none, 1)).toBe(
      "Your account's level is container: no agent chats. Ask a server admin. No node offers containers yet.",
    );
    expect(agenticBlockReason("host", { ...none, exec_level: "container" }, 1)).toContain("No node offers containers yet.");
    // No workspace (the API token): the account and the workers decide.
    expect(agenticBlockReason("host", null, 1)).toBeNull();
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

import { newlyWaiting } from "./notify";
describe("notify", () => {
  it("only reports chats that just started waiting", () => {
    expect(newlyWaiting(["a", "b"], ["b", "c", "d"])).toEqual(["c", "d"]);
    expect(newlyWaiting([], [])).toEqual([]);
  });
});
