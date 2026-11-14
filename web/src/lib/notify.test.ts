// Row W.6: a notification for a waiting chat carries fixed words and the
// chat's place in the list, never its title, a command or a path.
import { describe, expect, test } from "vitest";
import { waitingWords } from "./notify";

describe("a waiting chat's notification", () => {
  test("fixed words and the place, nothing of the chat", () => {
    expect(waitingWords("approval", 1, 1)).toBe("A chat needs your approval");
    expect(waitingWords("question", 2, 3)).toBe("A chat asked you a question (2 of 3 waiting)");
  });

  test("the Shell builds it from the reason and the place only", async () => {
    const src = (await import("node:fs")).readFileSync(new URL("../components/Shell.tsx", import.meta.url), "utf8");
    const call = src.slice(src.indexOf("new Notification("), src.indexOf("new Notification(") + 200);
    expect(call).toContain("waitingWords(a.reason");
    expect(call).not.toMatch(/a\.title|title\)/);
  });
});
