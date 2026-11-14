// @vitest-environment happy-dom
// The export dialog: Download builds the file from the view it was given,
// with tool outputs only when the box is ticked, under a name from the title.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { emptyView } from "../fold";
import type { SessionView } from "../types";

const files: { text: string; name: string }[] = [];
vi.mock("../lib/exportChat", async (orig) => ({
  ...(await orig<typeof import("../lib/exportChat")>()),
  download: (text: string, name: string) => void files.push({ text, name }),
}));
const { default: ExportChat } = await import("./ExportChat");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const view: SessionView = {
  ...emptyView(),
  session_id: "s",
  head_seq: 3,
  items: [
    { type: "user", turn_id: "t", text: "List the files" },
    {
      type: "tool",
      call_id: "c",
      author: null,
      tool: "bash",
      input: { command: "ls" },
      live_output: "",
      result: { ok: true, output: "SECRET_OUTPUT_LINE", exit_code: 0 },
      approval: null,
    },
  ],
};

let host: HTMLElement;
let root: Root;
beforeEach(() => {
  files.length = 0;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const show = (onClose = vi.fn()) => {
  act(() => root.render(<ExportChat view={view} title="Files: today" agentName={() => "Assistant"} onClose={onClose} />));
  return onClose;
};
const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label)!;

describe("export dialog", () => {
  test("no outputs unless ticked", () => {
    const closed = show();
    act(() => button("Download .md").click());
    expect(files).toHaveLength(1);
    expect(files[0]!.name).toBe("files-today.md");
    expect(files[0]!.text).toContain("- Tool `bash`: ls (ok, exit 0)");
    expect(files[0]!.text).not.toContain("SECRET_OUTPUT_LINE");
    expect(closed).toHaveBeenCalled();
  });

  test("the box adds the outputs", () => {
    show();
    act(() => host.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
    act(() => button("Download .md").click());
    expect(files[0]!.text).toContain("```text\nSECRET_OUTPUT_LINE\n```");
  });

  test("Cancel downloads nothing", () => {
    const closed = show();
    act(() => button("Cancel").click());
    expect(files).toEqual([]);
    expect(closed).toHaveBeenCalled();
  });
});
