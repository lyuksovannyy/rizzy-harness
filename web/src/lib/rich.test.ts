import { describe, expect, test } from "vitest";
import type { Item, Source } from "../types";
import { citeMaps, citeSignature, groupWeb, provenanceAt, provenanceIndex, provenanceSignature, sourceKeys } from "./rich";

const user = (text: string): Item => ({ type: "user", turn_id: "t", text });
const assistant = (text: string): Item => ({
  type: "assistant",
  message_id: "m",
  text,
  done: true,
  author: null,
  variant: null,
});
const tool = (output: string | null, name = "web_search", sources?: Source[]): Item => ({
  type: "tool",
  call_id: "c",
  author: null,
  tool: name,
  input: {},
  live_output: "",
  result: output === null ? null : { ok: true, output, exit_code: null, ...(sources ? { sources } : {}) },
  approval: null,
});
const src = (id: string, url: string): Source => ({ id, url, title: id });

describe("provenance", () => {
  const items = [
    user("check https://a.example/x?q=1"),
    assistant("see https://model.example/made-up?id=7"),
    tool("1. Result\n   https://b.example/page?ref=2"),
    assistant("done"),
    tool(null),
  ];

  test("only user text and tool output count, never the model's text", () => {
    const first = provenanceIndex(items);
    expect(first.get("a.example/x?q=1")).toBe(0);
    expect(first.get("b.example/page?ref=2")).toBe(2);
    expect(first.has("model.example/made-up?id=7")).toBe(false);
  });

  test("a message relies only on what came before it", () => {
    const first = provenanceIndex(items);
    expect(provenanceAt(first, 1).has("a.example/x?q=1")).toBe(true);
    expect(provenanceAt(first, 1).has("b.example/page?ref=2")).toBe(false);
    expect(provenanceAt(first, 3).has("b.example/page?ref=2")).toBe(true);
  });

  test("the signature counts user items and finished web tools", () => {
    expect(provenanceSignature(items)).toBe(2);
    expect(provenanceSignature([...items, tool("x", "bash")])).toBe(2);
  });

  test("only web tools count: the model steers other tool output", () => {
    const steered = [
      user("go"),
      tool("https://evil.example/c?d=SECRET", "bash"),
      tool("see https://evil.example/r?d=1", "read"),
      tool("https://docs.example/p?x=1", "web_fetch"),
      assistant("[x](https://evil.example/c?d=SECRET)"),
    ];
    const at = provenanceAt(provenanceIndex(steered), 4);
    expect(at.has("evil.example/c?d=SECRET")).toBe(false);
    expect(at.has("evil.example/r?d=1")).toBe(false);
    expect(at.has("docs.example/p?x=1")).toBe(true);
  });
});

describe("citeMaps", () => {
  const items = [
    user("q"),
    tool("[S1] A", "web_search", [src("S1", "https://a.example/"), src("S2", "https://b.example/")]),
    assistant("A [S1] [S3]"),
    tool("[S3] C", "web_fetch", [src("S3", "https://c.example/p")]),
    tool("x", "bash", [src("S4", "https://evil.example/")]),
    tool("[S2] B again", "web_search", [{ id: "S2", url: "https://b.example/", title: "B, newer title" }]),
    assistant("B [S2] [S3] [S4]"),
  ];
  const at = citeMaps(items);

  test("a message sees only the sources recorded before it", () => {
    expect([...at(0).keys()]).toEqual([]);
    expect([...at(2).keys()]).toEqual(["S1", "S2"]);
    expect(at(2).get("S3")).toBeUndefined();
    expect(at(6).get("S3")?.url).toBe("https://c.example/p");
    // Indexes past the end (a message still to come) see everything.
    expect(at(99).size).toBe(3);
  });

  test("the latest source for a label wins", () => {
    expect(at(2).get("S2")?.title).toBe("S2");
    expect(at(6).get("S2")?.title).toBe("B, newer title");
  });

  test("only web tools give sources", () => {
    expect(at(6).get("S4")).toBeUndefined();
  });

  test("source URLs are verified link keys", () => {
    const keys = sourceKeys(at(6));
    expect(keys.has("c.example/p")).toBe(true);
    expect(keys.has("a.example")).toBe(true);
    expect(keys.has("evil.example")).toBe(false);
    expect(sourceKeys(at(6))).toBe(keys);
  });

  test("a source with a fragment vouches for its page, and for that fragment only", () => {
    const keys = sourceKeys(citeMaps([tool("x", "web_search", [src("S1", "https://e.example/w#History")])])(1));
    expect(keys.has("e.example/w")).toBe(true);
    expect(keys.has("e.example/w#History")).toBe(true);
    expect(keys.has("e.example/w#c2VjcmV0")).toBe(false);
  });

  test("a delegate result's sources resolve, but its text is no provenance", () => {
    const items = [
      user("q"),
      tool("[S1] A", "web_search", [src("S1", "https://a.example/")]),
      tool("session: x\n@scout ended (completed):\nSee [S2] https://evil.example/c?d=1", "delegate", [
        src("S2", "https://scout.example/p"),
      ]),
      assistant("A [S1], scout found [S2]"),
    ];
    expect(citeMaps(items)(3).get("S2")?.url).toBe("https://scout.example/p");
    expect(citeMaps(items)(3).get("S1")?.url).toBe("https://a.example/");
    const first = provenanceIndex(items);
    expect(first.has("evil.example/c?d=1")).toBe(false);
    // The signature the citations are memoised on moves when it finishes.
    expect(citeSignature(items)).toBe(2);
    expect(citeSignature([...items, tool(null, "delegate")])).toBe(2);
    expect(citeSignature([...items, tool("x", "bash", [src("S9", "https://evil.example/")])])).toBe(2);
  });

  test("source URLs count as provenance", () => {
    const first = provenanceIndex([user("q"), tool("no urls in the text", "web_fetch", [src("S1", "https://d.example/x?y=1")])]);
    expect(first.get("d.example/x?y=1")).toBe(1);
  });
});

describe("groupWeb", () => {
  test("runs of two or more web calls with no text between them group", () => {
    const items = [
      user("q"),
      tool("a"),
      assistant(" "),
      tool("b", "web_fetch"),
      tool("c", "web_fetch"),
      assistant("text"),
      tool("d"),
      assistant("more"),
      tool("e"),
      tool("f", "bash"),
      tool("g"),
      tool("h"),
    ];
    const g = groupWeb(items);
    expect([...g.keys()]).toEqual([1, 10]);
    expect(g.get(1)).toEqual({ start: 1, end: 4, members: [1, 3, 4], searches: 1, pages: 2 });
    expect(g.get(10)).toEqual({ start: 10, end: 11, members: [10, 11], searches: 2, pages: 0 });
  });
});
