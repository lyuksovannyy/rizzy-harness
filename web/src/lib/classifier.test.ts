import { describe, expect, it } from "vitest";
import { decide, emptyState, modelTrusted, parseState, predict, record, serialize, update } from "./classifier";
import type { Mode } from "./modes";
import { ruleMode } from "./rules";

const TABLE: [string, Mode][] = [
  ["youtube", "search"],
  ["github.com", "search"],
  ["https://docs.rs/tokio", "search"],
  ["weather riga", "search"],
  ["rust book", "search"],
  ["iphone 17", "search"],
  ["netflix", "search"],
  ["bbc.co.uk", "search"],
  ["amazon", "search"],
  ["tokio docs", "search"],
  ["how do I configure the tokio runtime?", "ask"],
  ["what is a monad", "ask"],
  ["why is the sky blue", "ask"],
  ["explain ownership in rust", "ask"],
  ["write an email to my landlord about the heating", "ask"],
  ["can you summarize this article for me", "ask"],
  ["compare postgres and sqlite for a small app", "ask"],
  ["translate this into german please", "ask"],
  ["is it safe to eat raw eggs?", "ask"],
  ["give me three names for a cat", "ask"],
  ["@writer draft the release notes", "ask"],
  ["the meaning of life according to douglas adams", "ask"],
  ["best way to learn piano as an adult", "ask"],
  ["ideas for a birthday party", "ask"],
  ["help me plan a trip to japan", "ask"],
  ["hello there", "ask"],
  ["hi", "ask"],
  ["thanks!", "ask"],
  ["ok continue", "ask"],
  ["fix the failing test in src/lib.rs", "agentic"],
  ["refactor the auth module to use argon2", "agentic"],
  ["run cargo test and fix any errors", "agentic"],
  ["add a dark mode toggle to web/src/App.tsx", "agentic"],
  ["implement pagination for the sessions endpoint", "agentic"],
  ["build the docker image and push it", "agentic"],
  ["update dependencies in Cargo.toml", "agentic"],
  ["debug why the websocket disconnects", "agentic"],
  ["deploy the staging branch", "agentic"],
  ["rename UserRow to Account across the codebase", "agentic"],
  ["install playwright and write an e2e test", "agentic"],
  ["migrate the database schema to add an index", "agentic"],
  ["look at ./scripts/size.mjs and lower the budget", "agentic"],
  ["remove the unused imports in crates/master", "agentic"],
  ["lint and format the web folder", "agentic"],
];

describe("rules", () => {
  it("classify the labelled table", () => {
    const wrong = TABLE.filter(([t, m]) => ruleMode(t, true) !== m);
    expect(wrong).toEqual([]);
  });
  it("never predict agentic when it is not allowed", () => {
    for (const [t] of TABLE) expect(ruleMode(t, false)).not.toBe("agentic");
  });
});

describe("classifier", () => {
  it("learns a user-specific split the rules get wrong", () => {
    // This user types bare product names and always wants Ask for them.
    const st = emptyState();
    const train: [string, Mode][] = [];
    const items = ["kettle", "toaster", "blender", "fridge", "oven", "mixer", "kettle black", "smart oven"];
    for (let round = 0; round < 15; round++) {
      for (const it of items) train.push([it, "ask"]);
      for (const [t, m] of TABLE) train.push([t, m]);
    }
    for (const [t, m] of train) record(st, t, m, true);
    const correct = TABLE.filter(([t, m]) => predict(st, t).mode === m).length + items.filter((i) => predict(st, i).mode === "ask").length;
    expect(correct / (TABLE.length + items.length)).toBeGreaterThan(0.9);
    expect(ruleMode("kettle", true)).toBe("search");
    expect(modelTrusted(st)).toBe(true);
    expect(decide(st, "kettle", true)).toEqual({ mode: "ask", source: "model" });
  });

  it("keeps the rules until the model is better on the same window", () => {
    const st = emptyState();
    for (let i = 0; i < 40; i++) for (const [t, m] of TABLE.slice(0, 3)) record(st, t, m, true);
    // Rules are perfect on these; the model is never strictly better.
    expect(modelTrusted(st)).toBe(false);
    expect(decide(st, "youtube", true).source).toBe("rules");
  });

  it("skips agentic when blocked, even from the model", () => {
    const st = emptyState();
    for (let i = 0; i < 200; i++) update(st, "fix the tests", "agentic");
    st.labels = 100;
    st.model_window = new Array(100).fill(1);
    st.rules_window = new Array(100).fill(0);
    expect(decide(st, "fix the tests", false).mode).not.toBe("agentic");
  });

  it("serializes compactly and parses only the right shape", () => {
    const st = emptyState();
    record(st, "how does this work?", "ask", true);
    const back = parseState(JSON.parse(JSON.stringify(serialize(st))));
    expect(back.labels).toBe(1);
    expect(predict(back, "how does this work?").mode).toBe(predict(st, "how does this work?").mode);
    expect(parseState({ version: 1, weights: [1], bias: [0, 0, 0] }).labels).toBe(0);
    expect(parseState(null).weights.length).toBe(4096 * 3);
  });
});
