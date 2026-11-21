import { expect, test } from "vitest";
import { folderWords } from "./subFolder";

test("the two folders have words, anything else none", () => {
  expect(folderWords("shared")).toBe("in this chat's folder");
  expect(folderWords("own")).toBe("in a copy of the folder of its own");
  expect(folderWords(undefined)).toBeNull();
  expect(folderWords("other")).toBeNull();
});
