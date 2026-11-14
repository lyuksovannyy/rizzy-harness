// Export as Markdown: from the fold only; tool calls as one line each, their
// output only when asked, in a fence the output cannot close; hidden
// characters in a call's line marked; a safe file name.
import { describe, expect, test } from "vitest";
import { emptyView } from "../fold";
import type { Item, SessionView } from "../types";
import { chatMarkdown, fence, fileName } from "./exportChat";

const tool = (call_id: string, name: string, input: object, output: string, ok = true): Item => ({
  type: "tool",
  call_id,
  author: null,
  tool: name,
  input,
  live_output: "",
  result: { ok, output, exit_code: name === "bash" ? (ok ? 0 : 1) : null },
  approval: null,
});

const view = (items: Item[]): SessionView => ({
  ...emptyView(),
  session_id: "s",
  head_seq: 9,
  meta: { title: "t", cwd: "/w", model: "demo::mock", backend: "native", node_id: "n", approval_mode: "auto", profile: "agentic" },
  items,
});

const NOW = new Date("2026-11-14T10:20:00Z");

describe("export as Markdown", () => {
  const v = view([
    { type: "user", turn_id: "t1", text: "Fix the build" },
    tool("c1", "bash", { command: "cargo test" }, "test result: FAILED\n```\nsk-not-shown?", false),
    tool("c2", "read", { path: "src/lib.rs" }, "fn main() {}"),
    { type: "assistant", message_id: "m1", text: "Fixed: <b>not html</b>", done: true, author: "coder", variant: null },
    { type: "turn_end", turn_id: "t1", reason: "completed" },
  ]);

  test("messages in full, tool calls as one line each, no outputs by default", () => {
    const md = chatMarkdown(v, "Build fix", { outputs: false, agentName: (id) => (id === "coder" ? "Coder" : "?"), now: NOW });
    expect(md).toBe(
      [
        "# Build fix",
        "",
        "Exported 2026-11-14 10:20 UTC · model demo::mock",
        "",
        "## You",
        "",
        "Fix the build",
        "",
        "- Tool `bash`: cargo test (failed, exit 1)",
        "- Tool `read`: src/lib.rs (ok)",
        "",
        "## Coder",
        "",
        "Fixed: <b>not html</b>",
        "",
      ].join("\n"),
    );
    expect(md).not.toContain("test result");
  });

  test("outputs when asked, in a fence the output cannot end", () => {
    const md = chatMarkdown(v, "Build fix", { outputs: true, now: NOW });
    expect(md).toContain("````text\ntest result: FAILED\n```\nsk-not-shown?\n````");
    expect(md).toContain("```text\nfn main() {}\n```");
  });

  test("a hidden character in a call's line is marked, and the line stays one line", () => {
    const md = chatMarkdown(view([tool("c", "bash", { command: "rm -rf /tmp/x‮\nls" }, "")]), "x", { outputs: false, now: NOW });
    const line = md.split("\n").find((l) => l.startsWith("- Tool"))!;
    expect(line).not.toContain("‮");
    expect(line).toContain("ls");
  });

  test("fences and file names", () => {
    expect(fence("a")).toBe("```text\na\n```");
    expect(fence("``` x ````")).toBe("`````text\n``` x ````\n`````");
    expect(fileName("Build fix: ../../etc/passwd")).toBe("build-fix-etc-passwd.md");
    expect(fileName("///")).toBe("chat.md");
  });
});
