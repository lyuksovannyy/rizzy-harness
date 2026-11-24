// @vitest-environment happy-dom
// A text file of the chat's folder in the side panel (X.63): the text with
// line numbers as React text only (a script, markup, an escape sequence and a
// direction override are inert text, the hidden ones shown as codes), the
// size and "as of" line, the cut and the lossy notes, the server's sentence
// with a way to try again, Reload asking again, Escape and the close button;
// and the "Open" control: only for a path the page can ask for, a Word or
// Excel file handed to its Preview, one panel at a time with the unsaved
// question asked.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { PreviewTarget } from "../lib/docPreview";
import type { FileAnswer } from "../lib/folderFile";

const call = vi.fn<(...a: unknown[]) => Promise<unknown>>();
vi.mock("../api", () => ({ call: (...a: unknown[]) => call(...a) }));
const highlight = vi.fn<(code: string, grammar: string) => Promise<unknown>>(() => Promise.resolve(null));
vi.mock("../md/highlighter", () => ({ highlightOffThread: (code: string, grammar: string) => highlight(code, grammar) }));
const { default: FolderFile } = await import("./FolderFile");
const { FileButton } = await import("./FileButton");
const store = await import("../lib/docPreview");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  call.mockReset();
  highlight.mockReset();
  highlight.mockImplementation(() => Promise.resolve(null));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  store.closePreview();
  store.gate.ask = null;
});

const flush = () => act(async () => {});
const target: PreviewTarget = { session: "s 1", path: "src/main.rs", name: "main.rs", text: true };

async function show(answer: FileAnswer | Error, t: PreviewTarget = target, onClose = () => {}) {
  call.mockImplementation(() => (answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer)));
  await act(async () => root.render(<FolderFile target={t} onClose={onClose} />));
  await flush();
}

const file = (text: string, more: Partial<FileAnswer> = {}): FileAnswer => ({ name: "main.rs", text, size: new TextEncoder().encode(text).length, ...more });

test("the file's text has its line numbers, its size and the time it was read", async () => {
  await show(file("fn main() {\n    run();\n}\n"));
  expect(call).toHaveBeenCalledWith("GET", "/api/sessions/s%201/folder-file?path=src%2Fmain.rs", undefined, expect.anything());
  expect(host.querySelector(".fv-text")!.textContent).toBe("fn main() {\n    run();\n}\n");
  // Three lines, the last newline is no fourth.
  expect(host.querySelector(".fv-gutter")!.textContent).toBe("1\n2\n3");
  expect(host.querySelector(".fv-gutter")!.getAttribute("aria-hidden")).toBe("true");
  expect(host.querySelector("h2")!.textContent).toBe("main.rs");
  expect(host.textContent).toContain("src/main.rs");
  expect(host.textContent).toMatch(/3 lines shown · as of /);
  expect(host.textContent).toContain("Read-only");
  expect(host.textContent).not.toContain("not shown");
});

test("markup, escapes and direction overrides are inert text, the hidden ones as codes", async () => {
  await show(file("<script>alert(1)</script>\n<img src=x onerror=alert(2)>\n[a](https://evil.example/)\n\u001b[31mred‮evil​\n"));
  expect(host.querySelector("script")).toBeNull();
  expect(host.querySelector("img")).toBeNull();
  expect(host.querySelector("a")).toBeNull();
  const t = host.querySelector(".fv-text")!.textContent!;
  expect(t).toContain("<script>alert(1)</script>");
  expect(t).toContain("[a](https://evil.example/)");
  expect(t).toContain("<U+001B>[31mred<U+202E>evil<U+200B>");
  expect(t).not.toContain("‮");
  expect(t).not.toContain("\u001b");
});

test("a cut file says how much is shown, and invalid bytes are said", async () => {
  await show(file("a\nb\n", { size: 3 * 1024 * 1024, truncated: true, lossy: true }));
  const t = host.textContent!;
  expect(t).toContain("3.0 MB file is shown; the rest is not shown");
  expect(t).toContain("not valid text");
});

test("a known language is highlighted from the text as it is shown, and drawn as spans and text only", async () => {
  highlight.mockImplementation((code) =>
    Promise.resolve({
      type: "root",
      children: [
        { type: "element", tagName: "span", properties: { className: ["token", "keyword"] }, children: [{ type: "text", value: "fn" }] },
        { type: "element", tagName: "img", properties: { src: "x" }, children: [] },
        { type: "text", value: code.slice(2) },
      ],
    }),
  );
  await show(file("fn main() {}\u202e\n"));
  expect(highlight).toHaveBeenCalledWith("fn main() {}<U+202E>\n", "rust");
  expect(host.querySelector(".fv-text .keyword")!.textContent).toBe("fn");
  expect(host.querySelector("img")).toBeNull();
  expect(host.querySelector(".fv-text")!.textContent).toBe("fn main() {}<U+202E>\n");
});

test("a file the chat's code blocks would not highlight, and a name with no grammar, stay plain", async () => {
  await show(file("x".repeat(30_000)));
  expect(highlight).not.toHaveBeenCalled();
  await show(file("hello\n"), { session: "s1", path: "notes.txt", name: "notes.txt", text: true });
  expect(highlight).not.toHaveBeenCalled();
});

test("an empty file says so", async () => {
  await show(file(""));
  expect(host.textContent).toContain("The file is empty.");
  expect(host.querySelector(".fv-text")).toBeNull();
});

test("the server's sentence is text, nothing old stays under it, and Try again asks again", async () => {
  await show(new Error("unknown file"));
  expect(host.textContent).toContain("unknown file");
  expect(host.querySelector(".fv-text")).toBeNull();
  call.mockImplementation(() => Promise.resolve(file("ok\n")));
  const again = [...host.querySelectorAll("button")].find((b) => b.textContent === "Try again")!;
  await act(async () => again.click());
  await flush();
  expect(host.querySelector(".fv-text")!.textContent).toBe("ok\n");
  expect(host.textContent).not.toContain("unknown file");
});

test("Reload asks for the file again, and a file that is gone is not shown from before", async () => {
  await show(file("one\n"));
  expect(call).toHaveBeenCalledTimes(1);
  call.mockImplementation(() => Promise.reject(new Error("unknown file")));
  const reload = host.querySelector<HTMLButtonElement>('button[aria-label="Reload the file"]')!;
  await act(async () => reload.click());
  await flush();
  expect(call).toHaveBeenCalledTimes(2);
  expect(host.textContent).toContain("unknown file");
  expect(host.querySelector(".fv-text")).toBeNull();
});

test("Escape and the close button close the panel", async () => {
  const onClose = vi.fn();
  await show(file("x\n"), target, onClose);
  await act(async () => {
    host.querySelector("aside")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
  expect(onClose).toHaveBeenCalledTimes(1);
  await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Close the file"]')!.click());
  expect(onClose).toHaveBeenCalledTimes(2);
});

test("a very long line is one line that scrolls: nothing wraps or is cut by the page", async () => {
  await show(file("x".repeat(256 * 1024)));
  expect(host.querySelector(".fv-text")!.textContent!.length).toBe(256 * 1024);
  expect(host.querySelector(".fv-gutter")!.textContent).toBe("1");
});

function openFrom(path: string, cwd?: string) {
  act(() => root.render(<FileButton sessionId="s1" path={path} cwd={cwd} />));
  return host.querySelector("button");
}

test("Open offers a text target; a Word or Excel file goes to its Preview", () => {
  openFrom("src/main.rs")!.click();
  expect(store.usePreview).toBeDefined();
  let seen: PreviewTarget | null = null;
  const probe = () => {
    function P() {
      seen = store.usePreview();
      return null;
    }
    return <P />;
  };
  act(() => root.render(probe()));
  expect(seen).toMatchObject({ session: "s1", path: "src/main.rs", name: "main.rs", text: true });
  store.closePreview();
  act(() => root.render(<FileButton sessionId="s1" path="docs/plan.docx" />));
  host.querySelector("button")!.click();
  act(() => root.render(probe()));
  expect(seen).toMatchObject({ session: "s1", path: "docs/plan.docx", name: "plan.docx" });
  expect((seen as PreviewTarget | null)?.text).toBeUndefined();
  expect(store.targetKey({ session: "s", path: "a", name: "a" })).not.toBe(store.targetKey({ session: "s", path: "a", name: "a", text: true }));
});

test("Open is not offered for a path the server would refuse or that leaves the folder", () => {
  expect(openFrom("a\nb.rs")).toBeNull();
  expect(openFrom("a‮b.rs")).not.toBeNull(); // a direction override is asked as written; the node decides
  expect(openFrom("a/" + "é".repeat(600))).toBeNull(); // over 1,024 bytes
  expect(openFrom("")).toBeNull();
  expect(openFrom("/w/src/a.rs", "/w")).not.toBeNull();
  expect(openFrom("/etc/passwd", "/w")).toBeNull();
  expect(openFrom("/w2/a.rs", "/w")).toBeNull();
});

test("opening a file while changes wait in the Word panel asks first", async () => {
  const ask = vi.fn(() => Promise.resolve(false));
  store.gate.ask = ask;
  store.openPreview({ session: "s1", path: "a.docx", name: "a.docx" });
  openFrom("src/main.rs")!.click();
  await flush();
  expect(ask).toHaveBeenCalledTimes(1);
  let seen: PreviewTarget | null = null;
  function P() {
    seen = store.usePreview();
    return null;
  }
  act(() => root.render(<P />));
  expect(seen).toMatchObject({ path: "a.docx" });
});
