// A message whose code fence is not closed (a cancelled answer, pasted
// text) must not swallow every later section of the export (review 101-1).
import { describe, expect, test } from "vitest";
import { chatMarkdown } from "./exportChat";
import type { SessionView } from "../types";

describe("review: export fences", () => {
  test("an unclosed fence in an answer does not swallow the next message", () => {
    const view = {
      items: [
        { type: "user", text: "Write the script", message_id: "u1" },
        { type: "assistant", text: "Here:\n\n```bash\nrm -rf build", message_id: "a1", turn_id: "t1" },
        { type: "turn_end", reason: "cancelled", turn_id: "t1" },
        { type: "user", text: "Next question", message_id: "u2" },
      ],
    } as unknown as SessionView;
    const md = chatMarkdown(view, "T", { outputs: false, now: new Date(0) });
    const before = md.slice(0, md.lastIndexOf("## You"));
    const fences = (before.match(/^```/gm) ?? []).length;
    // An odd count means the second "## You" is inside a code block.
    expect(fences % 2, md).toBe(0);
  });
});
