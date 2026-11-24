// @vitest-environment happy-dom
// A turn's changes beside the chat (X.12): line numbers and signs that mark
// an added or removed line beyond its colour, words for screen readers,
// hidden characters shown as codes the way the approval card shows them,
// nothing from the files rendered as HTML or as a link, the cut marker, a
// binary file named with its sizes, the node's refusal as text with a way
// to try again, and Undo from the panel.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { TurnDiff } from "../types";

const turnDiff = vi.fn((..._a: unknown[]) => Promise.resolve(sample()) as Promise<TurnDiff>);
const chatDiff = vi.fn((..._a: unknown[]) => Promise.resolve(sample()) as Promise<TurnDiff>);
const restoreTurn = vi.fn((..._a: unknown[]) => Promise.resolve({ ok: true }));
vi.mock("../apiPages", () => ({
  pageApi: {
    turnDiff: (...a: unknown[]) => turnDiff(...a),
    chatDiff: (...a: unknown[]) => chatDiff(...a),
    restoreTurn: (...a: unknown[]) => restoreTurn(...a),
  },
}));
const { default: DiffPanel, LINES_SHOWN, OPEN_FILES } = await import("./DiffPanel");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function sample(): TurnDiff {
  return {
    files: [
      {
        path: "src/a‮txt.rs",
        status: "modified",
        content: "text",
        old_mode: "100644",
        new_mode: "100644",
        old_size: 30,
        new_size: 40,
        added: 1,
        removed: 1,
        hunks: [
          {
            old_start: 7,
            old_lines: 3,
            new_start: 7,
            new_lines: 3,
            section: "fn main()",
            lines: [" keep", "-<img src=x onerror=alert(1)>", "+see https://evil.example/​ now", "\\ No newline at end of file"],
          },
        ],
        cut: true,
      },
      { path: "logo.png", status: "added", content: "binary", new_mode: "100644", new_size: 2048, added: 0, removed: 0 },
      { path: "old.txt", old_path: "older.txt", status: "renamed", content: "text", added: 0, removed: 0 },
    ],
    more_files: 3,
    cut: true,
  };
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  turnDiff.mockClear();
  chatDiff.mockClear();
  restoreTurn.mockClear();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function render(over: Partial<Parameters<typeof DiffPanel>[0]> = {}) {
  const props = {
    sessionId: "s1",
    turnId: "t1",
    rewind: false,
    laterChanges: false,
    canUndo: true,
    running: false,
    onRewind: vi.fn(),
    onClose: vi.fn(),
    ...over,
  };
  await act(async () => {
    root.render(<DiffPanel {...props} />);
  });
  await act(async () => {});
  return props;
}

test("lines carry numbers and a sign, and words for screen readers", async () => {
  await render();
  expect(turnDiff).toHaveBeenCalledWith("s1", "t1", false, expect.anything());
  const rows = [...host.querySelectorAll(".diff-line")];
  const del = rows.find((r) => r.classList.contains("diff-del"))!;
  const add = rows.find((r) => r.classList.contains("diff-add"))!;
  // Old and new line numbers: context 7/7, the removed line is old 8, the
  // added one new 8.
  const nums = (r: Element) => [...r.querySelectorAll(".diff-n")].map((n) => n.textContent);
  expect(nums(rows[0]!)).toEqual(["7", "7"]);
  expect(nums(del)).toEqual(["8", ""]);
  expect(nums(add)).toEqual(["", "8"]);
  expect(del.querySelector(".diff-sign")!.textContent).toBe("−");
  expect(add.querySelector(".diff-sign")!.textContent).toBe("+");
  expect(del.querySelector(".sr-only")!.textContent).toContain("removed");
  expect(add.querySelector(".sr-only")!.textContent).toContain("added");
  expect(host.textContent).toContain("no newline at end of file");
  expect(host.textContent).toContain("@@ −7,3 +7,3 @@ fn main()");
});

test("file text is text: no HTML, no live link, hidden characters as codes", async () => {
  await render();
  expect(host.querySelector("img")).toBeNull();
  expect(host.querySelector(".diff-body a")).toBeNull();
  expect(host.textContent).toContain("<img src=x onerror=alert(1)>");
  const marks = [...host.querySelectorAll(".hidden-mark")].map((m) => m.textContent);
  expect(marks).toContain("<U+200B>");
  // The path's right-to-left override, in the file's head and the list.
  expect(marks.filter((m) => m === "<U+202E>").length).toBeGreaterThanOrEqual(2);
});

test("cut, binary and rename rows say what they are", async () => {
  await render();
  const text = host.textContent!;
  expect(text).toContain("Cut: this file has more changes");
  expect(text).toContain("3 more changed files are not listed");
  expect(text).toContain("Binary file (2.0 KB): not shown.");
  expect(text).toContain("older.txt → old.txt");
  expect(text).toContain("Moved, content unchanged.");
  // Status by letter and by word, not by colour alone.
  const badge = host.querySelector(".diff-file .diff-badge")!;
  expect(badge.textContent).toContain("M");
  expect(badge.textContent).toContain("modified");
  // Counted as the row counts: a rename is its two paths (review D5).
  expect(text).toContain("7 files (1 renamed)");
});

test("a refusal is text with a way to try again", async () => {
  turnDiff.mockImplementationOnce(() => Promise.reject(new Error("the node local that holds this chat's folder is away")));
  await render();
  expect(host.querySelector("[role=alert]")!.textContent).toContain("is away");
  const again = [...host.querySelectorAll("button")].find((b) => b.textContent === "Try again")!;
  await act(async () => again.click());
  await act(async () => {});
  expect(turnDiff).toHaveBeenCalledTimes(2);
  expect(host.querySelector("[role=alert]")).toBeNull();
});

test("Undo from the panel, the wider view, Escape closes", async () => {
  const p = await render({ laterChanges: true });
  const undo = [...host.querySelectorAll("button")].find((b) => b.textContent === "Undo file changes")!;
  await act(async () => undo.click());
  expect(restoreTurn).toHaveBeenCalledWith("s1", "t1", false, false);
  expect(host.textContent).toContain("Undone.");
  const box = host.querySelector<HTMLInputElement>("input[type=checkbox]")!;
  await act(async () => box.click());
  expect(p.onRewind).toHaveBeenCalledWith(true);
  const aside = host.querySelector("aside")!;
  await act(async () => aside.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
  expect(p.onClose).toHaveBeenCalled();
});

const fileOf = (path: string, lines: string[]): TurnDiff["files"][number] => ({
  path,
  status: "modified",
  content: "text",
  added: lines.filter((l) => l[0] === "+").length,
  removed: lines.filter((l) => l[0] === "-").length,
  hunks: [{ old_start: 1, old_lines: lines.length, new_start: 1, new_lines: lines.length, lines }],
});

// The page this draws is the largest the panel ever draws at once
// (OPEN_FILES x LINES_SHOWN rows), which takes jsdom several seconds on a
// loaded CI runner: 5.8 s against the default 5 s limit once. The answer is
// no larger than it must be to prove the bound (files past the open ones,
// more lines than one "Show more" step), and the test has its own limit.
test("a huge answer draws a bounded page; closed files have no lines; Show more adds them (review D3)", async () => {
  const many = Array.from({ length: LINES_SHOWN + 2200 }, (_, i) => `+l${i}`);
  turnDiff.mockImplementationOnce(() =>
    Promise.resolve({ files: Array.from({ length: OPEN_FILES + 2 }, (_, i) => fileOf(`f${i}`, many)) }),
  );
  await render();
  const rows = host.querySelectorAll(".diff-line").length;
  expect(rows).toBe(OPEN_FILES * LINES_SHOWN);
  expect(host.textContent).toContain(`The first ${OPEN_FILES} files are open`);
  // A closed file has no lines in the page.
  const closed = host.querySelector<HTMLDetailsElement>(`#diff-f${OPEN_FILES}`)!;
  expect(closed.open).toBe(false);
  expect(closed.querySelectorAll(".diff-line").length).toBe(0);
  const first = host.querySelector("#diff-f0")!;
  const more = [...first.querySelectorAll("button")].find((b) => b.textContent?.startsWith("Show "))!;
  expect(more.textContent).toBe("Show 2,000 more lines");
  await act(async () => more.click());
  expect(first.querySelectorAll(".diff-line").length).toBe(LINES_SHOWN + 2000);
}, 30_000);

test("code is marked only where it hides something: indentation stays, CRLF said once (review D6)", async () => {
  turnDiff.mockImplementationOnce(() =>
    Promise.resolve({
      files: [
        fileOf("crlf.txt", [" a\r", "-b\r", "+B\r"]),
        fileOf("mixed.txt", [" a", "+lone cr\r"]),
        fileOf("indent.py", ["+" + " ".repeat(48) + "return x", "+\tx\u202E"]),
      ],
    }),
  );
  await render();
  const crlf = host.querySelector("#diff-f0")!;
  expect(crlf.textContent).toContain("CRLF line endings");
  expect(crlf.querySelectorAll(".hidden-mark").length).toBe(0);
  const mixed = host.querySelector("#diff-f1")!;
  expect(mixed.textContent).not.toContain("CRLF line endings");
  expect([...mixed.querySelectorAll(".hidden-mark")].map((m) => m.textContent)).toEqual(["<U+000D>"]);
  const indent = host.querySelector("#diff-f2")!;
  expect(indent.textContent).toContain(" ".repeat(48) + "return x");
  expect(indent.textContent).not.toContain("spaces⟩");
  expect([...indent.querySelectorAll(".hidden-mark")].map((m) => m.textContent)).toEqual(["<U+202E>"]);
});

test("another chat's files are named before Undo, hidden characters as codes (T57)", async () => {
  turnDiff.mockImplementationOnce(() =>
    Promise.resolve({ ...sample(), others_wrote: ["shop/format.py"], others_may: ["x\u202Ey"] }),
  );
  await render();
  const text = host.textContent!;
  // Certain and possible apart (review F6).
  expect(text).toContain("Another chat working in this folder changed shop/format.py during");
  expect(text).toContain("so it asks first");
  expect(text).toContain("so x<U+202E>y may hold its changes");
  expect(text).not.toContain("changed x<U+202E>y");
});

test("no note when no other chat worked here", async () => {
  await render();
  expect(host.textContent).not.toContain("Another chat");
});

// All of a chat's changes (X.61): the same panel, asked for the whole chat.
test("the whole chat's changes: asked once for the chat, titled by range, no turn actions", async () => {
  await render({ all: true, turnId: "", canUndo: false });
  expect(chatDiff).toHaveBeenCalledWith("s1", expect.anything());
  expect(turnDiff).not.toHaveBeenCalled();
  expect(host.querySelector(".diff-title")!.textContent).toContain("All changes in this chat");
  expect(host.querySelector("aside")!.getAttribute("aria-label")).toBe("All changes in this chat");
  // Nothing that acts on one turn: no undo, no rewind, no later-turns switch.
  expect(host.textContent).not.toContain("Undo file changes");
  expect(host.textContent).not.toContain("Rewind files");
  expect(host.querySelector("input[type=checkbox]")).toBeNull();
  // The files are the same rows as a turn's.
  expect(host.textContent).toContain("7 files (1 renamed)");
  expect(host.querySelectorAll(".diff-file").length).toBeGreaterThan(0);
});

test("the whole chat's range says what it holds, and where it starts when older checkpoints were pruned", async () => {
  await render({ all: true, turnId: "", canUndo: false });
  expect(host.querySelector("[role=note]")!.textContent).toContain(
    "Everything that changed in this chat's folder since it started, including changes made by hand or by other chats that share the folder.",
  );
  chatDiff.mockImplementationOnce(() => Promise.resolve({ ...sample(), since_pruned: true }));
  await render({ all: true, turnId: "", canUndo: false, sessionId: "s2" });
  expect(host.querySelector(".diff-title")!.textContent).toContain("Changes since the oldest turn still kept");
  expect(host.querySelector("[role=note]")!.textContent).toContain("since the oldest turn still kept, including changes made by hand");
});

test("the whole chat is read again when the running turn ends", async () => {
  await render({ all: true, turnId: "", canUndo: false, running: true });
  expect(chatDiff).toHaveBeenCalledTimes(1);
  const props = { sessionId: "s1", turnId: "", all: true, rewind: false, laterChanges: false, canUndo: false, onRewind: vi.fn(), onClose: vi.fn() };
  await act(async () => root.render(<DiffPanel {...props} running={true} />));
  expect(chatDiff).toHaveBeenCalledTimes(1);
  await act(async () => root.render(<DiffPanel {...props} running={false} />));
  await act(async () => {});
  expect(chatDiff).toHaveBeenCalledTimes(2);
  // A turn's own panel is not read again by a run ending.
  turnDiff.mockClear();
  await act(async () => root.render(<DiffPanel {...props} all={false} turnId="t1" running={true} />));
  await act(async () => root.render(<DiffPanel {...props} all={false} turnId="t1" running={false} />));
  expect(turnDiff).toHaveBeenCalledTimes(1);
});

test("a whole chat with no file change says so; a running turn is named", async () => {
  chatDiff.mockImplementationOnce(() => Promise.resolve({ files: [] }));
  await render({ all: true, turnId: "", canUndo: false, running: true });
  expect(host.textContent).toContain("No file changes in this chat.");
  expect(host.textContent).toContain("A turn is running: its changes show here once it ends.");
});

test("the whole chat's refusal is text with a way to try again", async () => {
  chatDiff.mockImplementationOnce(() => Promise.reject(new Error("this chat's node is older than this server; update the node")));
  await render({ all: true, turnId: "", canUndo: false });
  expect(host.querySelector("[role=alert]")!.textContent).toContain("older than this server");
  const again = [...host.querySelectorAll("button")].find((b) => b.textContent === "Try again")!;
  await act(async () => again.click());
  expect(chatDiff).toHaveBeenCalledTimes(2);
});

test("each changed file that still exists has an Open control; a deleted one has none", async () => {
  turnDiff.mockResolvedValueOnce({
    files: [
      { path: "src/a.rs", status: "modified", content: "text", added: 1, removed: 0 },
      { path: "gone.rs", status: "deleted", content: "text", added: 0, removed: 1 },
      { path: "new.rs", old_path: "old.rs", status: "renamed", content: "text", added: 0, removed: 0 },
    ],
  } as TurnDiff);
  await render();
  const labels = [...host.querySelectorAll("summary button")].map((b) => b.getAttribute("aria-label"));
  expect(labels).toEqual(["Open a.rs", "Open new.rs"]);
});

test("when the chat's folder is a subfolder of the repository, Open asks for the folder's own path and a file outside it has none", async () => {
  const open = vi.fn();
  const store = await import("../lib/docPreview");
  const files = ["pkg/src/a.rs", "pkg/b.rs", "pkgx/c.rs", "top.txt"].map((path) => ({ path, status: "modified", content: "text", added: 1, removed: 0 }));
  turnDiff.mockResolvedValueOnce({ files, folder: "pkg" } as TurnDiff);
  await render();
  const labels = [...host.querySelectorAll("summary button")].map((b) => b.getAttribute("aria-label"));
  // The row still shows the repository's path; Open takes the folder's.
  expect(labels).toEqual(["Open a.rs", "Open b.rs"]);
  expect(host.textContent).toContain("pkg/src/a.rs");
  void open;
  host.querySelector<HTMLButtonElement>('summary button[aria-label="Open a.rs"]')!.click();
  let seen: unknown = null;
  function P() {
    seen = store.usePreview();
    return null;
  }
  act(() => root.render(<P />));
  expect(seen).toMatchObject({ session: "s1", path: "src/a.rs", text: true });
  store.closePreview();
});
