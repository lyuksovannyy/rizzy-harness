import { describe, expect, test } from "vitest";
import { parseNum, sortOrder, toCsv } from "./table";

describe("parseNum", () => {
  test("numbers, currency, thousands and percent", () => {
    expect(parseNum("12.5")).toBe(12.5);
    expect(parseNum(" -3 ")).toBe(-3);
    expect(parseNum("−4")).toBe(-4);
    expect(parseNum("$1,250.50")).toBe(1250.5);
    expect(parseNum("40%")).toBe(40);
    expect(parseNum(".5")).toBe(0.5);
  });
  test("anything else is not a number", () => {
    for (const s of ["", "abc", "1.2.3", "12px", "1,2", "v2", "-", "$", "1e9999", "Infinity"]) expect(parseNum(s)).toBeNull();
  });
});

describe("sortOrder", () => {
  const rows = [
    ["core", "12.5"],
    ["web", "8"],
    ["master", "100"],
    ["cli", ""],
  ];
  test("numeric when every non-empty cell is a number", () => {
    expect(sortOrder(rows, 1, 1)).toEqual([1, 0, 2, 3]);
    expect(sortOrder(rows, 1, -1)).toEqual([2, 0, 1, 3]);
  });
  test("text otherwise, by locale; empty cells last", () => {
    expect(sortOrder(rows, 0, 1)).toEqual([3, 0, 2, 1]);
    const mixed = [["10"], ["9"], ["n/a"], [""]];
    expect(sortOrder(mixed, 0, 1)).toEqual([1, 0, 2, 3]);
  });
  test("ties keep their order", () => {
    expect(sortOrder([["a"], ["a"], ["a"]], 0, -1)).toEqual([0, 1, 2]);
  });
});

describe("toCsv", () => {
  test("quotes what needs quoting", () => {
    expect(toCsv([["a", "b,c"], ['say "hi"', "x\ny"]])).toBe('a,"b,c"\r\n"say ""hi""","x\ny"');
  });
  test("defuses spreadsheet formulas but keeps negative numbers", () => {
    expect(toCsv([["=HYPERLINK(\"x\")", "-5", "+1", "@SUM(A1)", "-cmd"]])).toBe(
      "\"'=HYPERLINK(\"\"x\"\")\",-5,+1,'@SUM(A1),'-cmd",
    );
  });
});
