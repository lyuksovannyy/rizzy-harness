// @vitest-environment happy-dom
// Changing text in the side panel and saving a copy (X.60, T100): a click on
// a paragraph or a cell makes it a field, the changes wait until Save, the
// copy goes to the composer in the one way each kind of chat has (a held file
// / an `@` mention), a refusal keeps the changes and says where, and leaving
// with changes asks once.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { PreviewAnswer, PreviewTarget } from "../lib/docPreview";
import type { UploadedAttachment } from "../types";

const call = vi.fn<(...a: unknown[]) => Promise<unknown>>();
vi.mock("../api", () => ({ call: (...a: unknown[]) => call(...a) }));
const { default: DocPreview } = await import("./DocPreview");
const store = await import("../lib/docPreview");
const { mentionOf } = await import("../lib/docPreviewEdit");
const { fileMentions } = await import("../lib/fileMentions");

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
  vi.restoreAllMocks();
});

const plainChat: PreviewTarget = { session: "s1", fileId: "f1", name: "plan.docx" };
const codingChat: PreviewTarget = { session: "s2", path: "docs/plan.docx", name: "plan.docx" };
const flush = () => act(async () => {});

const doc: PreviewAnswer = {
  name: "plan.docx",
  preview: {
    kind: "doc",
    more: false,
    notes: [],
    blocks: [
      { k: "h", l: 1, r: [{ t: "The plan" }], at: "p1#00a1" },
      { k: "p", r: [{ t: "Plain " }, { t: "bold", b: true }], at: "p2#00b2" },
      { k: "p", r: [{ t: "Only bold", b: true }], at: "p3#00c3" },
      { k: "li", r: [{ t: "an item" }], at: "p4#00d4" },
      { k: "t", rows: [["Name", "Qty"]] },
      { k: "p", r: [{ t: "A cut paragraph" }] },
    ],
  },
};

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
      { n: 2, c: [{ a: "A2", v: "Ann" }, { a: "B2", v: "3" }, { a: "C2", f: "=B2*2" }, { a: "D2", v: "cut text", x: true }] },
    ],
  },
};

type Composer = { setFiles: ReturnType<typeof vi.fn>; setText: ReturnType<typeof vi.fn>; focus: ReturnType<typeof vi.fn> };
const composer = (): Composer => ({ setFiles: vi.fn(), setText: vi.fn(), focus: vi.fn() });

let seen: PreviewTarget | null = null;
function Probe() {
  seen = store.usePreview();
  return null;
}

async function show(answer: PreviewAnswer, t: PreviewTarget, c: Composer | null = composer(), onClose = () => {}) {
  seen = null;
  call.mockImplementation((method: unknown) => (method === "GET" ? Promise.resolve(answer) : Promise.reject(new Error("unexpected"))));
  await act(async () =>
    root.render(
      <>
        <Probe />
        <DocPreview target={t} onClose={onClose} composer={{ current: c as never }} />
      </>,
    ),
  );
  await flush();
}

const q = <T extends Element = HTMLElement>(sel: string) => host.querySelector<T>(sel)!;
const field = (name: string) => host.querySelector<HTMLTextAreaElement | HTMLInputElement>(`[aria-label="${name}"]`);
const button = (label: RegExp | string) =>
  [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => (typeof label === "string" ? b.textContent === label || b.getAttribute("aria-label") === label : label.test(b.textContent ?? "") || label.test(b.getAttribute("aria-label") ?? "")));

/** Type into a React-controlled field. */
async function type(el: HTMLTextAreaElement | HTMLInputElement, value: string) {
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(proto, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const press = (el: Element, key: string) => act(async () => void el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true })));
const blur = (el: HTMLElement) =>
  act(async () => {
    el.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
  });

async function change(label: string, to: string) {
  await act(async () => button(label)!.click());
  const f = field(label.replace(/^Edit /, ""))!;
  await type(f, to);
  await press(f, "Enter");
}

test("a paragraph, a heading and a list item are places to click; a table and a cut paragraph are not", async () => {
  await show(doc, plainChat);
  const places = [...host.querySelectorAll<HTMLButtonElement>(".pv-body button.pv-place")].map((b) => b.getAttribute("aria-label"));
  expect(places).toEqual(["Edit Paragraph 1", "Edit Paragraph 2", "Edit Paragraph 3", "Edit Paragraph 4"]);
  // Reachable by Tab: real buttons, none taken out of the tab order.
  expect([...host.querySelectorAll<HTMLButtonElement>(".pv-body button.pv-place")].every((b) => b.tabIndex === 0)).toBe(true);
  expect(q(".pv-table button")).toBeNull();
  expect(host.textContent).toContain("Tables, paragraphs that are cut, and paragraphs with characters that cannot be shown here cannot be changed here");
});

test("a click makes a field with the current text; Enter keeps the change, Escape drops it, leaving the field keeps it", async () => {
  await show(doc, plainChat);
  await act(async () => button("Edit Paragraph 3")!.click());
  const f = field("Paragraph 3")!;
  expect(f.tagName).toBe("TEXTAREA");
  expect(f.value).toBe("Only bold");
  expect(document.activeElement).toBe(f);
  // Escape drops what was typed.
  await type(f, "Dropped");
  await press(f, "Escape");
  expect(field("Paragraph 3")).toBeNull();
  expect(q(".pv-body").textContent).toContain("Only bold");
  expect(host.textContent).not.toContain("1 change");
  // Enter keeps it, and the place is marked; the focus goes back to the place
  // (so a keyboard user is not dropped on the page, and Escape still closes).
  await act(async () => button("Edit Paragraph 3")!.click());
  await type(field("Paragraph 3")!, "Kept");
  await press(field("Paragraph 3")!, "Enter");
  expect(document.activeElement).toBe(button("Edit Paragraph 3"));
  expect(q(".pv-body").textContent).toContain("Kept");
  expect(host.querySelectorAll(".pv-changed")).toHaveLength(1);
  expect(host.textContent).toContain("1 change");
  expect(button("Discard")).toBeTruthy();
  expect(button("Save a copy")).toBeTruthy();
  // Leaving the field keeps it too.
  await act(async () => button("Edit Paragraph 1")!.click());
  await type(field("Paragraph 1")!, "Left by blur");
  await blur(field("Paragraph 1")!);
  expect(host.textContent).toContain("2 changes");
  // Typing the original back is no change.
  await change("Edit Paragraph 1", "The plan");
  expect(host.textContent).toContain("1 change");
  // Nothing was sent.
  expect(call.mock.calls.filter((c) => c[0] !== "GET")).toEqual([]);
});

test("Discard drops every change", async () => {
  await show(doc, plainChat);
  await change("Edit Paragraph 1", "One");
  await change("Edit Paragraph 4", "Four");
  expect(host.textContent).toContain("2 changes");
  await act(async () => button("Discard")!.click());
  expect(host.querySelectorAll(".pv-changed")).toHaveLength(0);
  expect(host.textContent).not.toContain("changes");
  expect(q(".pv-body").textContent).toContain("The plan");
});

test("a paragraph of several formattings says what changing it does before Save; one of a single formatting does not", async () => {
  await show(doc, plainChat);
  await act(async () => button("Edit Paragraph 2")!.click());
  expect(host.textContent).toContain("Changing this paragraph keeps the formatting of its first part and drops the rest");
  await press(field("Paragraph 2")!, "Escape");
  await act(async () => button("Edit Paragraph 3")!.click());
  expect(host.textContent).not.toContain("drops the rest");
});

test("Save in a chat without a folder holds the copy like an upload and says so; the copy is shown, read-only", async () => {
  const c = composer();
  await show(doc, plainChat, c);
  await change("Edit Paragraph 1", "Annual plan");
  await change("Edit Paragraph 4", "last item");
  const file: UploadedAttachment = { attachment_id: "a9", name: "plan (edited).docx", mime: "application/x", bytes: 10, kept: true };
  call.mockImplementation(() => Promise.resolve({ file, results: [{ ok: true }, { ok: true }] }));
  await act(async () => button("Save a copy")!.click());
  await flush();
  const last = call.mock.calls.at(-1)!;
  expect(last.slice(0, 3)).toEqual([
    "POST",
    "/api/sessions/s1/files/f1/edit",
    { changes: [{ at: "p1#00a1", text: "Annual plan" }, { at: "p4#00d4", text: "last item" }] },
  ]);
  // The composer takes the file as an upload (an updater, so what it holds stays).
  expect(c.setFiles).toHaveBeenCalledTimes(1);
  const upd = c.setFiles.mock.calls[0]![0] as (f: UploadedAttachment[]) => UploadedAttachment[];
  expect(upd([{ attachment_id: "old", name: "x", mime: "y" }])).toEqual([{ attachment_id: "old", name: "x", mime: "y" }, file]);
  expect(upd([file])).toEqual([file]);
  expect(c.setText).not.toHaveBeenCalled();
  expect(host.textContent).toContain("A copy named “plan (edited).docx” was saved and added to the message.");
  // No pending changes; the copy shows the new text and is not editable here.
  expect(host.textContent).not.toContain("1 change");
  expect(q(".pv-body").textContent).toContain("Annual plan");
  expect(host.querySelector(".pv-body button.pv-place")).toBeNull();
  expect(host.textContent).toContain("Send the message, then open the copy from the chat to change it again");
});

test("Save in a chat with a folder puts an @ mention in the message and shows the copy the node wrote", async () => {
  const c = composer();
  await show(doc, codingChat, c);
  await change("Edit Paragraph 1", "Annual plan");
  call.mockImplementation(() => Promise.resolve({ saved: "docs/plan (edited).docx", name: "plan (edited).docx", results: [{ ok: true }] }));
  await act(async () => button("Save a copy")!.click());
  await flush();
  expect(call.mock.calls.at(-1)!.slice(0, 3)).toEqual(["POST", "/api/sessions/s2/folder-edit", { path: "docs/plan.docx", changes: [{ at: "p1#00a1", text: "Annual plan" }] }]);
  // The mention goes in as a person would type it; a path with a space is quoted.
  expect(c.setText).toHaveBeenCalledTimes(1);
  const put = c.setText.mock.calls[0]![0] as (t: string) => string;
  expect(put("")).toBe('@"docs/plan (edited).docx" ');
  expect(put("look at")).toBe('look at @"docs/plan (edited).docx" ');
  expect(put('x @"docs/plan (edited).docx" y')).toBe('x @"docs/plan (edited).docx" y');
  expect(c.setFiles).not.toHaveBeenCalled();
  // The panel's target becomes the new file, with the same sentence.
  const now = seen as PreviewTarget | null;
  expect(now).toMatchObject({ session: "s2", path: "docs/plan (edited).docx", name: "plan (edited).docx" });
  expect(now?.note).toBe("A copy named “plan (edited).docx” was saved and added to the message.");
});

test("the panel opened on a copy shows the note and no changes", async () => {
  await show(doc, { ...codingChat, path: "docs/plan (edited).docx", name: "plan (edited).docx", note: "A copy named “plan (edited).docx” was saved and added to the message." });
  expect(host.textContent).toContain("A copy named “plan (edited).docx” was saved and added to the message.");
  expect(host.textContent).not.toContain("1 change");
});

test("a refused change keeps every change, says which place and why, and adds nothing to the message", async () => {
  const c = composer();
  await show(doc, plainChat, c);
  await change("Edit Paragraph 2", "x");
  await change("Edit Paragraph 4", "y");
  const err = Object.assign(new Error("nothing was saved: some changes could not be applied"), {
    status: 422,
    body: {
      error: "nothing was saved: some changes could not be applied",
      results: [{ ok: false, why: "stale", message: "this paragraph changed since the preview was made; open the preview again" }, { ok: false, why: "skipped", message: "not saved, because another change could not be saved" }],
    },
  });
  call.mockImplementation(() => Promise.reject(err));
  await act(async () => button("Save a copy")!.click());
  await flush();
  expect(c.setFiles).not.toHaveBeenCalled();
  expect(c.setText).not.toHaveBeenCalled();
  expect(host.textContent).toContain("2 changes");
  const notes = [...host.querySelectorAll(".pv-place-error")].map((n) => n.textContent);
  expect(notes[0]).toContain("this paragraph changed since the preview was made; open the preview again");
  expect(notes).toHaveLength(2);
  expect(q(".pv-body").textContent).toContain("x");
  // The message of the HTTP error is shown once, as text.
  expect(q("[role=alert]").textContent).toContain("nothing was saved");
});

test("another failure is one sentence; the changes stay and Save can be tried again", async () => {
  await show(doc, plainChat);
  await change("Edit Paragraph 1", "x <b>bold</b>");
  call.mockImplementation(() => Promise.reject(new Error("the node is away <i>")));
  await act(async () => button("Save a copy")!.click());
  await flush();
  expect(q("[role=alert]").textContent).toContain("the node is away <i>");
  expect(q("[role=alert] i")).toBeNull();
  expect(host.textContent).toContain("1 change");
  expect(button("Save a copy")!.disabled).toBe(false);
});

const dlg = () => document.querySelector<HTMLDialogElement>("dialog.host-warning");
const dlgButton = (label: string) => [...(dlg()?.querySelectorAll("button") ?? [])].find((b) => b.textContent === label)!;
const answer = (label: string) => act(async () => dlgButton(label).click());

test("leaving with changes asks once, in the app's dialog; without changes it does not", async () => {
  const onClose = vi.fn();
  const confirm = vi.fn();
  window.confirm = confirm;
  await show(book, { session: "s1", fileId: "f2", name: "book.xlsx" }, composer(), onClose);
  // No changes: close and sheet tabs go straight.
  await act(async () => q<HTMLButtonElement>(".pv-tabs button:nth-child(2)").click());
  expect(dlg()).toBeNull();
  call.mockImplementation(() => Promise.resolve(book));
  await act(async () => q<HTMLButtonElement>(".pv-tabs button:nth-child(1)").click());
  await flush();
  await change("Edit Cell B2", "9");
  const calls = call.mock.calls.length;
  await act(async () => q<HTMLButtonElement>(".pv-tabs button:nth-child(2)").click());
  expect(dlg()!.textContent).toContain("Discard 1 change that has not been saved?");
  expect(document.activeElement).toBe(dlgButton("Keep editing"));
  await answer("Keep editing");
  expect(dlg()).toBeNull();
  expect(call.mock.calls.length).toBe(calls);
  expect(host.textContent).toContain("1 change");
  await act(async () => button("Close the preview")!.click());
  expect(dlg()).not.toBeNull();
  // Escape is Keep editing.
  await act(async () => void dlg()!.dispatchEvent(new Event("cancel", { cancelable: true })));
  expect(dlg()).toBeNull();
  expect(onClose).not.toHaveBeenCalled();
  await act(async () => button("Close the preview")!.click());
  await answer("Discard changes");
  expect(onClose).toHaveBeenCalledTimes(1);
  expect(confirm).not.toHaveBeenCalled();
});

test("a cell is a field with its text or formula; an empty cell of the grid can be filled; a cut cell cannot", async () => {
  const c = composer();
  await show(book, { session: "s1", fileId: "f2", name: "book.xlsx" }, c);
  expect(button("Edit Cell D2")).toBeUndefined();
  await act(async () => button("Edit Cell C2")!.click());
  expect(field("Cell C2")!.tagName).toBe("INPUT");
  expect(field("Cell C2")!.value).toBe("=B2*2");
  await press(field("Cell C2")!, "Escape");
  await act(async () => button("Edit Cell B1")!.click());
  expect(field("Cell B1")!.value).toBe("");
  await type(field("Cell B1")!, "12");
  await press(field("Cell B1")!, "Enter");
  await change("Edit Cell B2", "");
  expect(host.textContent).toContain("2 changes");
  call.mockImplementation(() => Promise.resolve({ file: { attachment_id: "a1", name: "book (edited).xlsx", mime: "x", kept: true }, results: [{ ok: true }, { ok: true }] }));
  await act(async () => button("Save a copy")!.click());
  await flush();
  expect(call.mock.calls.at(-1)!.slice(0, 3)).toEqual(["POST", "/api/sessions/s1/files/f2/edit", { sheet: 0, changes: [{ at: "B1", text: "12" }, { at: "B2", text: "" }] }]);
  // The copy shown has the typed values.
  const cells = [...host.querySelectorAll("tbody td")].map((x) => x.textContent);
  expect(cells.join("|")).toContain("12");
});

test("changes are per sheet: Save sends the shown sheet's place", async () => {
  await show({ ...book, preview: { ...(book.preview as { kind: "sheet" }), at: 1 } as PreviewAnswer["preview"] }, { session: "s1", path: "b.xlsx", name: "b.xlsx" });
  await change("Edit Cell A1", "Hello");
  call.mockImplementation(() => Promise.resolve({ saved: "b (edited).xlsx", name: "b (edited).xlsx", results: [{ ok: true }] }));
  await act(async () => button("Save a copy")!.click());
  await flush();
  expect(call.mock.calls.at(-1)!.slice(0, 3)).toEqual(["POST", "/api/sessions/s1/folder-edit", { path: "b.xlsx", sheet: 1, changes: [{ at: "A1", text: "Hello" }] }]);
});

test("the saved path is text, and a path that cannot be a mention is shown and not inserted", async () => {
  const c = composer();
  await show(doc, codingChat, c);
  await change("Edit Paragraph 1", "x");
  call.mockImplementation(() => Promise.resolve({ saved: 'a "q" <img src=x onerror=alert(1)>.docx', name: 'a "q" <img src=x onerror=alert(1)>.docx', results: [{ ok: true }] }));
  await act(async () => button("Save a copy")!.click());
  await flush();
  expect(c.setText).not.toHaveBeenCalled();
  // The panel opens on the copy with a sentence that holds the path as text.
  const next = seen as PreviewTarget | null;
  expect(next?.note).toContain("<img src=x onerror=alert(1)>");
  expect(next?.note).toContain("could not be put in the message as a mention");
  await show(doc, next!);
  expect(host.querySelector("img")).toBeNull();
  expect(host.textContent).toContain("<img src=x onerror=alert(1)>");
});

test("a mention is the path as typed, quoted when it holds a space, and none when it cannot be read back", () => {
  expect(mentionOf("docs/a.docx")).toBe("@docs/a.docx");
  expect(mentionOf("a (edited).docx")).toBe('@"a (edited).docx"');
  expect(mentionOf('a"b.docx')).toBeNull();
  expect(mentionOf("a\nb.docx")).toBeNull();
  expect(mentionOf("a@b.docx")).toBeNull();
  expect(mentionOf("")).toBeNull();
  // Invisible and direction characters (Cf), private use, line separators: shown, not inserted.
  for (const c of ["\u200b", "\u200c", "\u200f", "\u202a", "\u202e", "\u2060", "\u2066", "\u2069", "\ufeff", "\u00ad", "\ue000", "\u2028"]) {
    expect(mentionOf(`a${c}b.docx`), JSON.stringify(c)).toBeNull();
  }
  expect(mentionOf("日本語 (edited).docx")).toBe('@"日本語 (edited).docx"');
  // And the composer reads both forms back.
  expect(fileMentions('see @docs/a.docx and @"docs/a (edited).docx" now')).toEqual(["docs/a.docx", "docs/a (edited).docx"]);
  expect(fileMentions('@"nofile" @x')).toEqual([]);
});

test("nothing from the file or the answer becomes HTML while editing", async () => {
  await show({ ...doc, preview: { ...(doc.preview as { kind: "doc" }), blocks: [{ k: "p", r: [{ t: "<img src=x onerror=alert(1)>" }], at: "p1#0001" }] } as PreviewAnswer["preview"] }, plainChat);
  expect(host.querySelector("img")).toBeNull();
  await act(async () => button("Edit Paragraph 1")!.click());
  expect(field("Paragraph 1")!.value).toBe("<img src=x onerror=alert(1)>");
  expect(host.querySelector("img")).toBeNull();
});

// The paths that used to drop the changes without a word (another file's
// Preview, another chat or page, closing the tab) ask the same question.
const other: PreviewTarget = { session: "s1", fileId: "f9", name: "other.docx" };
async function withChange() {
  store.openPreview(plainChat);
  await show(doc, plainChat);
  await change("Edit Paragraph 2", "Edited");
}

test("Preview on another file with changes asks; Keep editing and Escape keep the panel's file, Discard opens the other", async () => {
  await withChange();
  const opener = document.createElement("button");
  document.body.append(opener);
  opener.focus();
  act(() => store.openPreview(other));
  expect(dlg()!.textContent).toContain("Discard 1 change that has not been saved?");
  expect(seen).toEqual(plainChat);
  await answer("Keep editing");
  expect(seen).toEqual(plainChat);
  expect(host.textContent).toContain("1 change");
  expect(document.activeElement).toBe(opener);
  act(() => store.openPreview(other));
  await act(async () => void dlg()!.dispatchEvent(new Event("cancel", { cancelable: true })));
  expect(seen).toEqual(plainChat);
  act(() => store.openPreview(other));
  await answer("Discard changes");
  expect(seen).toEqual(other);
  opener.remove();
});

test("Preview on another file asks nothing without changes, after Save, or for the file already open", async () => {
  store.openPreview(plainChat);
  await show(doc, plainChat);
  act(() => store.openPreview(other));
  expect(dlg()).toBeNull();
  expect(seen).toEqual(other);
  // The same file, with changes: not a question.
  store.openPreview(plainChat);
  await withChange();
  act(() => store.openPreview({ ...plainChat }));
  expect(dlg()).toBeNull();
  // After a successful Save there is nothing to lose.
  call.mockImplementation(() => Promise.resolve({ file: { attachment_id: "a1", name: "plan (edited).docx", mime: "x", kept: true }, results: [{ ok: true }] }));
  await act(async () => button("Save a copy")!.click());
  await flush();
  act(() => store.openPreview(other));
  expect(dlg()).toBeNull();
  expect(seen).toEqual(other);
});

test("moving to another chat or page holds the move until answered; Keep editing restores the address, Discard replays it", async () => {
  const seenByApp = vi.fn();
  window.addEventListener("hashchange", seenByApp);
  await withChange();
  const move = (to: string) => {
    const old = location.href;
    location.hash = to;
    act(() => void window.dispatchEvent(new HashChangeEvent("hashchange", { oldURL: old, newURL: location.href })));
  };
  const start = location.hash;
  move("#/s/other");
  expect(dlg()).not.toBeNull();
  expect(seenByApp).not.toHaveBeenCalled();
  await answer("Keep editing");
  expect(seenByApp).not.toHaveBeenCalled();
  expect(location.hash).toBe(start);
  expect(host.textContent).toContain("1 change");
  move("#/s/other");
  await answer("Discard changes");
  expect(seenByApp).toHaveBeenCalled();
  expect(location.hash).toBe("#/s/other");
  expect(host.textContent).not.toContain("1 change");
  // The changes are gone with the answer: the same question is not asked twice.
  move("#/s/third");
  expect(dlg()).toBeNull();
  window.removeEventListener("hashchange", seenByApp);
});

test("closing the tab is stopped by the browser only while there are changes", async () => {
  const fire = () => {
    const e = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(e);
    return e.defaultPrevented;
  };
  store.openPreview(plainChat);
  await show(doc, plainChat);
  expect(fire()).toBe(false);
  await change("Edit Paragraph 2", "Edited");
  expect(fire()).toBe(true);
  await act(async () => button("Discard")!.click());
  expect(fire()).toBe(false);
  await change("Edit Paragraph 2", "Edited again");
  expect(fire()).toBe(true);
  act(() => root.render(<></>));
  expect(fire()).toBe(false);
});
