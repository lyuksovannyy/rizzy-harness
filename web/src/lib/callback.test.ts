// Callback turns in the chat (T.8d step 3): what started a turn nobody
// wrote, in words, and where turns start for everything that counts them.
import { describe, expect, test } from "vitest";
import type { Item } from "../types";
import { endWords, turnStart } from "./callback";

const end = (bg_id: string, reason: string, exit_code: number | null): Item => ({ type: "background_ended", bg_id, reason, exit_code, ran_ms: 1000, tail: "" });

describe("callback turns", () => {
  test("the ends that started one, as the chat says them", () => {
    const items = [end("bg-3", "exited", 1), end("bg-4", "timed_out", null), end("bg-5", "exited", null)];
    expect(endWords(items, ["bg-3"])).toBe("bg-3 ended (exit 1)");
    expect(endWords(items, ["bg-3", "bg-4", "bg-5"])).toBe("bg-3 ended (exit 1), bg-4 ended (timed out), bg-5 ended (exited)");
    // An end the chat does not hold (cut by a summary) is still named.
    expect(endWords([], ["bg-9"])).toBe("bg-9 ended");
    // Program output never reaches the words.
    expect(endWords([{ ...end("bg-3", "exited", 1), tail: "<b>x</b>" } as Item], ["bg-3"])).not.toContain("<b>");
  });

  test("a callback starts a turn like a person's message does", () => {
    expect(turnStart({ type: "callback", turn_id: "t", bg_ids: ["bg-1"] })).toBe(true);
    expect(turnStart({ type: "user", turn_id: "t", text: "hi" })).toBe(true);
    expect(turnStart({ type: "callback_refused", bg_ids: ["bg-1"], reason: "r" })).toBe(false);
  });
});
