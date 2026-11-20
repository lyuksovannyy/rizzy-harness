// Reviewing a plan (X.7): the plan as blocks with stable ids, the one
// feedback message, the block diff and where the plans of a chat come from.
import { describe, expect, test } from "vitest";
import { diffBlocks, feedbackText, inPlanOrder, plansOf, splitPlan, type Note } from "./planReview";

const PLAN = [
  "# Plan: add caching",
  "",
  "Introduce a cache for `lookup()` results.",
  "",
  "- Add `cache.rs`",
  "  - Use an LRU of 128 entries",
  "- Wire it in `main.rs`",
  "",
  "1. First",
  "2. Second",
  "",
  "```sh",
  "cargo test",
  "",
  "# not a heading",
  "```",
  "",
  "| a | b |",
  "|---|---|",
  "| 1 | 2 |",
  "",
  "Last paragraph,",
  "two lines.",
].join("\n");

describe("splitPlan", () => {
  const blocks = splitPlan(PLAN);
  test("headings, paragraphs, nested list items, a code fence and a table", () => {
    expect(blocks.map((b) => [b.kind, b.depth, b.text])).toEqual([
      ["heading", 0, "# Plan: add caching"],
      ["paragraph", 0, "Introduce a cache for `lookup()` results."],
      ["list", 0, "- Add `cache.rs`"],
      ["list", 1, "- Use an LRU of 128 entries"],
      ["list", 0, "- Wire it in `main.rs`"],
      ["list", 0, "1. First"],
      ["list", 0, "2. Second"],
      ["code", 0, "```sh\ncargo test\n\n# not a heading\n```"],
      ["table", 0, "| a | b |\n|---|---|\n| 1 | 2 |"],
      ["paragraph", 0, "Last paragraph,\ntwo lines."],
    ]);
  });
  test("ids are stable: the same text keeps its id, whatever changes around it", () => {
    const again = splitPlan(PLAN);
    expect(again.map((b) => b.id)).toEqual(blocks.map((b) => b.id));
    const edited = splitPlan(PLAN.replace("# Plan: add caching", "# Plan: add a cache").replace("Second", "Second!"));
    const ids = (bs: typeof blocks) => new Map(bs.map((b) => [b.text, b.id]));
    expect(ids(edited).get("- Wire it in `main.rs`")).toBe(ids(blocks).get("- Wire it in `main.rs`"));
    expect(ids(edited).get("# Plan: add a cache")).not.toBe(ids(blocks).get("# Plan: add caching"));
  });
  test("identical blocks get different ids", () => {
    const two = splitPlan("Same.\n\nSame.");
    expect(two[0]!.id).not.toBe(two[1]!.id);
  });
  test("an unclosed fence runs to the end; a code block in a list item stays in it", () => {
    expect(splitPlan("```\nopen\n\nstill code").map((b) => b.kind)).toEqual(["code"]);
    const inItem = splitPlan("- step\n  ```sh\n  ls\n\n  ```\n- next");
    expect(inItem.map((b) => b.kind)).toEqual(["list", "list"]);
    expect(inItem[0]!.text).toContain("```sh");
  });
  test("empty and blank text has no blocks", () => {
    expect(splitPlan("")).toEqual([]);
    expect(splitPlan("\n  \n")).toEqual([]);
  });
});

const note = (o: Partial<Note>): Note => ({ id: "n", planId: "p", blockId: "b", quote: "q", selection: false, kind: "comment", text: "t", sent: false, ...o });

describe("feedbackText", () => {
  test("one note of each kind, exactly", () => {
    const out = feedbackText([
      note({
        kind: "comment",
        quote: "Introduce a cache for `lookup()` results.",
        text: "Why not *HashMap*? Use:\n```rs\nlet m = 1;\n```",
      }),
      note({ kind: "replace", quote: "LRU of 128 entries", selection: true, text: "a 256-entry LRU\n(keeps `a_b` and # marks)" }),
      note({ kind: "remove", quote: "```sh\ncargo test\n\n# not a heading\n```", text: "" }),
    ]);
    expect(out).toBe(
      [
        "Feedback on your plan. Revise it to address each note below, then end your turn with the full revised plan. Do not carry anything out yet.",
        "",
        "**Note 1 of 3 (comment)**",
        "",
        "> Introduce a cache for `lookup()` results.",
        "",
        "Comment:",
        "````",
        "Why not *HashMap*? Use:",
        "```rs",
        "let m = 1;",
        "```",
        "````",
        "",
        "**Note 2 of 3 (replace)**",
        "",
        "> LRU of 128 entries",
        "",
        "Replace the quoted passage with:",
        "```",
        "a 256-entry LRU",
        "(keeps `a_b` and # marks)",
        "```",
        "",
        "**Note 3 of 3 (remove)**",
        "",
        "> ```sh",
        "> cargo test",
        ">",
        "> # not a heading",
        "> ```",
        "",
        "Remove the quoted passage.",
      ].join("\n"),
    );
  });
  test("a one-line comment stays on its line", () => {
    expect(feedbackText([note({ text: "Too vague." })])).toContain("Comment: Too vague.");
  });
  test("notes come out in the plan's order", () => {
    const blocks = splitPlan("A\n\nB\n\nC");
    const ns = [note({ id: "c", blockId: blocks[2]!.id }), note({ id: "a", blockId: blocks[0]!.id }), note({ id: "b2", blockId: blocks[1]!.id })];
    expect(inPlanOrder(ns, blocks).map((n) => n.id)).toEqual(["a", "b2", "c"]);
  });
});

describe("diffBlocks", () => {
  test("added, removed, changed and unchanged blocks", () => {
    const older = splitPlan("# T\n\nKeep.\n\nGo away.\n\nOld wording here.\n\nEnd.");
    const newer = splitPlan("# T\n\nKeep.\n\nBrand new.\n\nNew wording here.\n\nEnd.");
    const rows = diffBlocks(older, newer).map((r) => [r.status, r.block.text, r.before?.text]);
    expect(rows).toEqual([
      ["same", "# T", undefined],
      ["same", "Keep.", undefined],
      ["changed", "Brand new.", "Go away."],
      ["changed", "New wording here.", "Old wording here."],
      ["same", "End.", undefined],
    ]);
    const more = diffBlocks(splitPlan("A\n\nB"), splitPlan("A\n\n# H\n\nB\n\nC"));
    expect(more.map((r) => r.status)).toEqual(["same", "added", "same", "added"]);
    const gone = diffBlocks(splitPlan("A\n\n# H\n\nB"), splitPlan("A\n\nB"));
    expect(gone.map((r) => [r.status, r.block.text])).toEqual([["same", "A"], ["removed", "# H"], ["same", "B"]]);
  });
});

describe("plansOf", () => {
  const a = (id: string, text: string, done = true) => ({ type: "assistant", message_id: id, text, done });
  test("the last finished answer of each turn that ended", () => {
    const items = [
      { type: "user" },
      a("m1", "Let me look."),
      { type: "tool" },
      a("m2", "Plan one."),
      { type: "turn_end" },
      { type: "user" },
      a("m3", "Plan two."),
      { type: "turn_end" },
      { type: "user" },
      a("m4", "Midway of a running turn."),
    ];
    expect(plansOf(items)).toEqual([
      { id: "m2", text: "Plan one." },
      { id: "m3", text: "Plan two." },
    ]);
  });
  test("only an answer that ended a turn in plan mode is a plan", () => {
    const mode = (m: string) => ({ type: "approval_mode_changed", mode: m });
    // An ordinary chat that switches to plan mode: the earlier answer is not
    // a plan, so the first plan is not shown as a revision of it.
    const items = [
      { type: "user" },
      a("m1", "Done: I fixed the typo."),
      { type: "turn_end" },
      mode("plan"),
      { type: "user" },
      a("m2", "Plan one."),
      { type: "turn_end" },
      mode("auto"),
      { type: "user" },
      a("m3", "Carried out."),
      { type: "turn_end" },
      mode("plan"),
      { type: "user" },
      a("m4", "Plan two."),
      { type: "turn_end" },
    ];
    expect(plansOf(items, "plan").map((p) => p.id)).toEqual(["m2", "m4"]);
    // No change in the log: the mode now is the mode it always had.
    const plain = [{ type: "user" }, a("m1", "An answer."), { type: "turn_end" }];
    expect(plansOf(plain, "plan").map((p) => p.id)).toEqual(["m1"]);
    expect(plansOf(plain, "auto")).toEqual([]);
  });
  test("a turn that said nothing proposed nothing", () => {
    expect(plansOf([a("m1", "  "), { type: "turn_end" }, a("m2", "streaming", false), { type: "turn_end" }])).toEqual([]);
  });
});
