import { describe, expect, test } from "vitest";
import type { Item } from "../types";
import { provenanceAt, provenanceIndex, provenanceSignature } from "./rich";

const user = (text: string): Item => ({ type: "user", turn_id: "t", text });
const assistant = (text: string): Item => ({
  type: "assistant",
  message_id: "m",
  text,
  done: true,
  author: null,
  variant: null,
});
const tool = (output: string | null): Item => ({
  type: "tool",
  call_id: "c",
  author: null,
  tool: "web_search",
  input: {},
  live_output: "",
  result: output === null ? null : { ok: true, output, exit_code: null },
  approval: null,
});

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

  test("the signature counts user items and finished tools", () => {
    expect(provenanceSignature(items)).toBe(2);
  });
});
