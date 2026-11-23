// @vitest-environment happy-dom
// A Word or Excel file in the side panel (T99): blocks and runs drawn as
// elements, a workbook as a grid with row and column headers, a formula
// marked, the sheet tabs, the cut marker, the server's refusal as text with
// a way to try again, Escape and the close button, and nothing from the file
// rendered as HTML, an image, a script or a link. The "Preview" control only
// where the server has a copy to read.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { linked } from "../lib/links.setup";
import type { PreviewAnswer, PreviewTarget } from "../lib/docPreview";

const call = vi.fn<(...a: unknown[]) => Promise<unknown>>();
vi.mock("../api", () => ({ call: (...a: unknown[]) => call(...a) }));
const { default: DocPreview } = await import("./DocPreview");
const { PreviewButton } = await import("./PreviewButton");
const { MadeChip } = await import("./ChatArtifacts");
const store = await import("../lib/docPreview");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  call.mockReset();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  store.closePreview();
});

const target = { session: "s1", fileId: "f1", name: "plan.docx" };
const flush = () => act(async () => {});

async function show(answer: PreviewAnswer | Error, t: PreviewTarget = target, onClose = () => {}) {
  call.mockImplementation(() => (answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer)));
  await act(async () => root.render(<DocPreview target={t} onClose={onClose} />));
  await flush();
}

const doc: PreviewAnswer = {
  name: "plan.docx",
  preview: {
    kind: "doc",
    more: false,
    notes: ["Hidden text is not shown."],
    blocks: [
      { k: "h", l: 1, r: [{ t: "The plan" }] },
      { k: "p", r: [{ t: "Plain " }, { t: "bold", b: true }, { t: " and " }, { t: "both", b: true, i: true }, { t: " and " }, { t: "gone", s: true }] },
      { k: "li", r: [{ t: "one" }] },
      { k: "li", r: [{ t: "two" }] },
      { k: "t", rows: [["Name", "Qty"], ["Ann", "3"]] },
    ],
  },
};

test("a document is drawn as headings, paragraphs with their formatting, a list and a table", async () => {
  await show(doc);
  expect(call).toHaveBeenCalledWith("GET", "/api/sessions/s1/files/f1/preview", undefined, expect.anything());
  const body = host.querySelector(".pv-body")!;
  expect(body.querySelector("h3")?.textContent).toBe("The plan");
  expect(body.querySelector("strong")?.textContent).toBe("bold");
  expect(body.querySelector("strong em")?.textContent).toBe("both");
  expect(body.querySelector("s")?.textContent).toBe("gone");
  expect([...body.querySelectorAll("ul li")].map((l) => l.textContent)).toEqual(["one", "two"]);
  expect([...body.querySelectorAll("table td")].map((c) => c.textContent)).toEqual(["Name", "Qty", "Ann", "3"]);
  expect(host.textContent).toContain("Preview: text and structure, not the page layout");
  expect(host.textContent).toContain("Hidden text is not shown.");
  expect(host.textContent).not.toContain("More is not shown");
  // The name, and the download of the same file (a link, as everywhere).
  expect(host.querySelector("h2")?.textContent).toBe("plan.docx");
  const a = host.querySelector<HTMLAnchorElement>("a[href]")!;
  expect(a.getAttribute("href")).toBe(linked("/api/sessions/s1/made/f1"));
  expect(a.textContent).toBe("Download");
  // Focus is in the panel.
  expect(document.activeElement).toBe(host.querySelector("h2"));
});

test("a cut preview says so", async () => {
  await show({ name: "big.docx", preview: { kind: "doc", more: true, notes: [], blocks: [{ k: "p", r: [{ t: "x" }] }] } });
  expect(host.querySelector("[role=note]")?.textContent).toContain("More is not shown");
});

const book: PreviewAnswer = {
  name: "book.xlsx",
  preview: {
    kind: "sheet",
    sheets: ["Data", "Totals"],
    at: 0,
    more: false,
    notes: [],
    rows: [
      { n: 1, c: [{ a: "A1", v: "Name" }, { a: "C1", v: "Total" }] },
      { n: 4, c: [{ a: "A4", v: "Ann" }, { a: "C4", f: "=SUM(B1:B3)" }] },
    ],
  },
};

test("a workbook is a grid with column and row headers, a marked formula, and sheet tabs", async () => {
  await show(book, { session: "s1", fileId: "f2", name: "book.xlsx" });
  const heads = [...host.querySelectorAll("thead th")].map((h) => h.textContent);
  expect(heads).toEqual(["Row", "A", "B", "C"]);
  expect([...host.querySelectorAll("tbody th")].map((h) => h.textContent)).toEqual(["1", "4"]);
  const rows = [...host.querySelectorAll("tbody tr")].map((r) => [...r.querySelectorAll("td")].map((c) => c.textContent));
  expect(rows[0]).toEqual(["Name", "", "Total"]);
  const f = host.querySelector("td.pv-formula")!;
  expect(f.textContent).toContain("=SUM(B1:B3)");
  expect(f.textContent).toContain("Formula");
  expect(f.getAttribute("title")).toContain("not calculated");
  // The tabs: the shown sheet is pressed; another asks the server for it.
  const tabs = [...host.querySelectorAll<HTMLButtonElement>(".pv-tabs button")];
  expect(tabs.map((t) => [t.textContent, t.getAttribute("aria-pressed")])).toEqual([["Data", "true"], ["Totals", "false"]]);
  await act(async () => tabs[1]!.click());
  expect(call).toHaveBeenLastCalledWith("GET", "/api/sessions/s1/files/f2/preview?sheet=1", undefined, expect.anything());
});

test("nothing from the file becomes HTML, an image, a script or a link", async () => {
  const evil = "<img src=x onerror=alert(1)><script>alert(1)</script> javascript:alert(1) ‮evil";
  await show(
    {
    name: "<b>x</b>.docx",
    preview: {
      kind: "doc",
      more: false,
      notes: [],
      blocks: [
        { k: "h", l: 99, r: [{ t: evil }] },
        { k: "p", r: [{ t: evil, b: true }] },
        { k: "li", r: [{ t: "[x](javascript:alert(1)) https://evil.example/" }] },
        { k: "t", rows: [[evil, "=HYPERLINK(\"http://evil.example\",\"x\")"]] },
      ],
    },
  },
    { ...target, name: "<b>x</b>.docx" },
  );
  const body = host.querySelector(".pv-body")!;
  expect(body.querySelector("a, img, script, iframe, [onerror], [href], [src]")).toBeNull();
  expect(body.textContent).toContain("<img src=x onerror=alert(1)><script>alert(1)</script> javascript:alert(1)");
  // The direction character is shown as a code, not obeyed.
  expect(body.textContent).not.toContain("‮");
  // The one link on the panel is the download, and the name is text.
  expect(host.querySelectorAll("a[href]")).toHaveLength(1);
  expect(host.querySelector("h2")?.textContent).toBe("<b>x</b>.docx");
  expect(host.querySelector("h2 b")).toBeNull();
  // A level past six stays a heading.
  expect(body.querySelector("h6")).not.toBeNull();

  // The same in a workbook: a formula that is a link is text.
  await show({
    name: "w.xlsx",
    preview: {
      kind: "sheet",
      sheets: ["S"],
      at: 0,
      more: false,
      notes: [],
      rows: [{ n: 1, c: [{ a: "A1", v: "=cmd|' /C calc'!A0" }, { a: "B1", f: '=HYPERLINK("http://evil.example/","x")' }, { a: "javascript:1", v: "bad address" }] }],
    },
  }, { ...target, fileId: "f3", name: "w.xlsx" });
  const b2 = host.querySelector(".pv-body")!;
  expect(b2.querySelector("a, img, script, [href], [src]")).toBeNull();
  expect(b2.textContent).toContain("=cmd|' /C calc'!A0");
  expect(b2.textContent).toContain("=HYPERLINK(");
  expect(b2.textContent).not.toContain("bad address");
});

test("the server's refusal is shown as text, with a way to try again", async () => {
  await show(new Error("this file is too large to preview <b>"));
  const alert = host.querySelector("[role=alert]")!;
  expect(alert.textContent).toContain("this file is too large to preview <b>");
  expect(alert.querySelector("b")).toBeNull();
  expect(host.querySelector("[role=status]")).toBeNull();
  call.mockResolvedValue(doc);
  await act(async () => alert.querySelector<HTMLButtonElement>("button")!.click());
  await flush();
  expect(host.querySelector("[role=alert]")).toBeNull();
  expect(host.querySelector(".pv-body h3")?.textContent).toBe("The plan");
});

test("while it reads it says so; Escape and the close button close it", async () => {
  call.mockReturnValue(new Promise(() => {}));
  const onClose = vi.fn();
  await act(async () => root.render(<DocPreview target={target} onClose={onClose} />));
  expect(host.querySelector("[role=status]")?.textContent).toContain("Reading");
  await act(async () => {
    host.querySelector("aside")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
  expect(onClose).toHaveBeenCalledTimes(1);
  await act(async () => host.querySelector<HTMLButtonElement>("button[aria-label='Close the preview']")!.click());
  expect(onClose).toHaveBeenCalledTimes(2);
});

test("Preview is offered for a Word or Excel name that has a server copy, and opens the panel", async () => {
  await act(async () => root.render(<PreviewButton sessionId="s9" fileId="abc" name="Plan.XLSX" />));
  const btn = host.querySelector<HTMLButtonElement>("button")!;
  expect(btn.textContent).toContain("Preview");
  expect(btn.getAttribute("aria-label")).toBe("Preview Plan.XLSX");
  let seen: unknown = null;
  await act(async () => btn.click());
  function Probe() {
    seen = store.usePreview();
    return null;
  }
  await act(async () => root.render(<><PreviewButton sessionId="s9" fileId="abc" name="Plan.XLSX" /><Probe /></>));
  expect(seen).toEqual({ session: "s9", fileId: "abc", name: "Plan.XLSX" });
  // Not for another kind of file, nor without an id.
  await act(async () => root.render(<><PreviewButton sessionId="s9" fileId="abc" name="paper.pdf" /><PreviewButton sessionId="s9" fileId={undefined} name="a.docx" /></>));
  expect(host.querySelector("button")).toBeNull();
});

test("a made file's chip has Preview only in a chat that keeps a copy", async () => {
  const made = { name: "plan.docx", size: 1200, sha256: "ab" };
  await act(async () => root.render(<MadeChip sessionId="s1" made={{ ...made, file_id: "a".repeat(32) }} />));
  expect(host.textContent).toContain("Preview");
  expect(host.textContent).toContain("Download");
  // A chat with a folder: the file is there; the node reads it by its path.
  await act(async () => root.render(<MadeChip sessionId="s1" made={{ ...made, path: "out/plan.docx" }} />));
  expect(host.textContent).toContain("Preview");
  expect(host.textContent).not.toContain("Download");
  let seen: unknown = null;
  function Probe() {
    seen = store.usePreview();
    return null;
  }
  await act(async () => root.render(<><MadeChip sessionId="s1" made={{ ...made, path: "out/plan.docx" }} /><Probe /></>));
  await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
  expect(seen).toEqual({ session: "s1", path: "out/plan.docx", name: "plan.docx" });
  // Neither a copy nor a path: nothing to ask for.
  await act(async () => root.render(<MadeChip sessionId="s1" made={made} />));
  expect(host.textContent).not.toContain("Preview");
});

test("a file of the folder is asked for by its path, the path carried as data, with no download of it", async () => {
  const t = { session: "s 1", path: "docs/Q1 report & plan.docx", name: "Q1 report & plan.docx" };
  await show(doc, t);
  expect(call).toHaveBeenCalledWith(
    "GET",
    "/api/sessions/s%201/folder-preview?path=docs%2FQ1%20report%20%26%20plan.docx",
    undefined,
    expect.anything(),
  );
  expect(host.querySelector("a[href]")).toBeNull();
  expect(host.textContent).toContain("Preview: text and structure, not the page layout");
  // The same failure sentences, with the same way to try again.
  await act(async () => root.render(null));
  await show(new Error("unknown file"), t);
  expect(host.querySelector("[role=alert]")?.textContent).toContain("unknown file");
  expect(host.querySelector("[role=alert] button")?.textContent).toBe("Try again");
});

test("a sheet of a folder file is asked for by its place beside the path", async () => {
  const two: PreviewAnswer = {
    name: "b.xlsx",
    preview: { kind: "sheet", sheets: ["One", "Two"], at: 0, more: false, notes: [], rows: [{ n: 1, c: [{ a: "A1", v: "x" }] }] },
  };
  await show(two, { session: "s1", path: "b.xlsx", name: "b.xlsx" });
  await act(async () => host.querySelectorAll<HTMLButtonElement>(".pv-tabs button")[1]!.click());
  await flush();
  expect(call).toHaveBeenLastCalledWith("GET", "/api/sessions/s1/folder-preview?path=b.xlsx&sheet=1", undefined, expect.anything());
});

test("tabs go by place: names that look alike are two tabs, and a failed switch shows the error under the chosen tab, not the old sheet", async () => {
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  const two: PreviewAnswer = {
    name: "b.xlsx",
    preview: { kind: "sheet", sheets: ["A B", "A B", "Q1 2025"], at: 0, more: false, notes: [], rows: [{ n: 1, c: [{ a: "A1", v: "first sheet" }] }] },
  };
  await show(two, { ...target, fileId: "f9", name: "b.xlsx" });
  const tabs = () => [...host.querySelectorAll<HTMLButtonElement>(".pv-tabs button")];
  expect(tabs().map((t) => t.textContent)).toEqual(["A B", "A B", "Q1 2025"]);
  expect(errors).not.toHaveBeenCalled();
  // The second of the two: asked for by its place.
  call.mockImplementation(() => Promise.reject(new Error("the server is busy")));
  await act(async () => tabs()[1]!.click());
  await flush();
  expect(call).toHaveBeenLastCalledWith("GET", "/api/sessions/s1/files/f9/preview?sheet=1", undefined, expect.anything());
  expect(host.querySelector("[role=alert]")?.textContent).toContain("the server is busy");
  expect(host.querySelector("table")).toBeNull();
  expect(host.textContent).not.toContain("first sheet");
  expect(tabs().map((t) => t.getAttribute("aria-pressed"))).toEqual(["false", "true", "false"]);
  // Try again asks for the same place.
  call.mockImplementation(() => Promise.resolve({ ...two, preview: { ...two.preview, at: 1, rows: [{ n: 1, c: [{ a: "A1", v: "second sheet" }] }] } }));
  await act(async () => host.querySelector<HTMLButtonElement>("[role=alert] button")!.click());
  await flush();
  expect(call).toHaveBeenLastCalledWith("GET", "/api/sessions/s1/files/f9/preview?sheet=1", undefined, expect.anything());
  expect(host.textContent).toContain("second sheet");
  errors.mockRestore();
});

test("the cut note sends a folder file to Word or Excel and a kept file to its download", async () => {
  const cut: PreviewAnswer = { name: "a.docx", preview: { kind: "doc", more: true, notes: [], blocks: [{ k: "p", r: [{ t: "x" }] }] } };
  await show(cut, { session: "s1", fileId: "f1", name: "a.docx" });
  expect(host.querySelector(".pv-more")?.textContent).toContain("Download the file to see all of it.");
  await act(async () => root.render(null));
  await show(cut, { session: "s1", path: "a.docx", name: "a.docx" });
  const note = host.querySelector(".pv-more")?.textContent ?? "";
  expect(note).toContain("Open the file in Word or Excel to see all of it.");
  expect(note).not.toContain("Download");
});
