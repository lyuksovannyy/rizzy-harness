// @vitest-environment happy-dom
// Attaching any file (T94): a coding chat's box takes every file and tells
// the person which ones are saved to the chat's folder, with their size,
// before sending; a plain box passes the file on and shows the server's
// reason when it refuses; the picker is not narrowed to text types.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { UploadedAttachment } from "../types";
import type { Submission } from "./Composer";

type To = { session?: string; folder?: boolean; agent?: string };
const upload = vi.fn<(f: File, to: To) => Promise<UploadedAttachment>>();
vi.mock("../api", () => ({ api: { upload: (f: File, to: To) => upload(f, to) }, call: vi.fn(), HttpError: Error }));
const { Composer } = await import("./Composer");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLElement;
let root: Root;
const sent: Submission[] = [];
beforeEach(() => {
  upload.mockReset();
  sent.length = 0;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  sessionStorage.clear();
});

function show(p: { where: "home" | "session"; filesOf?: string; chatId?: string; agentId?: string }) {
  act(() =>
    root.render(
      <Composer
        where={p.where}
        agents={[]}
        models={[]}
        workers={[]}
        agentId={p.agentId ?? ""}
        onAgentChange={() => {}}
        agenticBlocked={null}
        filesOf={p.filesOf}
        chatId={p.chatId}
        onSubmit={async (s) => {
          sent.push(s);
        }}
      />,
    ),
  );
}
const input = () => host.querySelector<HTMLInputElement>('input[type="file"]')!;
const pick = (files: File[]) =>
  act(async () => {
    Object.defineProperty(input(), "files", { value: files, configurable: true });
    input().dispatchEvent(new Event("change", { bubbles: true }));
  });
const chips = () => [...host.querySelectorAll<HTMLLIElement>("ul.chips li")].map((li) => li.textContent?.trim());
function type(value: string) {
  const ta = host.querySelector("textarea")!;
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(ta, value);
    ta.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const send = () => act(async () => host.querySelector<HTMLButtonElement>("button.send")!.click());

const zip = () => new File([new Uint8Array(10)], "q3 report.zip", { type: "application/zip" });

describe("attaching any file", () => {
  test("a coding chat's box offers every file and says the zip is saved to the folder", async () => {
    show({ where: "session", filesOf: "sid-1" });
    expect(input().getAttribute("accept")).toBeNull();
    upload.mockResolvedValue({ attachment_id: "a1", name: "q3 report.zip", mime: "application/zip", bytes: 1_258_291, saved: true });
    await pick([zip()]);
    expect(upload).toHaveBeenCalledWith(expect.objectContaining({ name: "q3 report.zip" }), { session: "sid-1", folder: true });
    expect(chips()).toEqual(["q3 report.zip (1.2 MB, saved to the folder)"]);
    type("unpack it");
    await send();
    expect(sent[0]!.attachments).toEqual(["a1"]);
    expect(sent[0]!.uploads?.[0]?.saved).toBe(true);
  });

  test("a text file beside it is a plain chip, as before", async () => {
    show({ where: "session", filesOf: "sid-1" });
    upload.mockResolvedValueOnce({ attachment_id: "t1", name: "notes.md", mime: "text/markdown", chars: 1234, truncated: false });
    upload.mockResolvedValueOnce({ attachment_id: "a1", name: "x.zip", mime: "application/zip", bytes: 10, saved: true });
    await pick([new File(["# hi"], "notes.md", { type: "text/markdown" })]);
    await pick([zip()]);
    expect(chips()).toEqual(["notes.md", "x.zip (10 B, saved to the folder)"]);
  });

  test("a document is one chip, as a PDF is; one that cannot be read says so (T96)", async () => {
    show({ where: "session", filesOf: "sid-1" });
    const docx = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    upload.mockResolvedValueOnce({ attachment_id: "d1", name: "plan.docx", mime: docx, chars: 1234, truncated: false });
    upload.mockResolvedValueOnce({
      attachment_id: "d2",
      name: "bad.docx",
      mime: "application/octet-stream",
      bytes: 10,
      saved: true,
      note: "this document could not be read: a part of the document is missing",
    });
    await pick([new File(["x"], "plan.docx")]);
    await pick([new File(["y"], "bad.docx")]);
    expect(chips()).toEqual(["plan.docx", "bad.docx (10 B, saved to the folder)", "this document could not be read: a part of the document is missing"]);
    type("read them");
    await send();
    expect(sent[0]!.attachments).toEqual(["d1", "d2"]);
  });

  test("a plain box names no folder, and shows the server's reason", async () => {
    show({ where: "home" });
    upload.mockRejectedValue(new Error("unsupported file: this chat's agent cannot open files; switch to Agentic (next to Ask under the box) to attach any file, or attach text, a PDF, an Office document or an image"));
    await pick([zip()]);
    expect(upload).toHaveBeenCalledWith(expect.anything(), { session: undefined, folder: false });
    expect(host.querySelector(".error-text")?.textContent).toContain("cannot open files");
    expect(chips()).toEqual([]);
  });

  test("a plain chat's box names its chat and agent, and says the kept file is read-only (T95)", async () => {
    show({ where: "session", chatId: "sid-2", agentId: "general" });
    upload.mockResolvedValue({ attachment_id: "k1", name: "q3 report.zip", mime: "application/zip", bytes: 1_258_291, kept: true });
    await pick([zip()]);
    expect(upload).toHaveBeenCalledWith(expect.objectContaining({ name: "q3 report.zip" }), { session: "sid-2", folder: false, agent: "general" });
    expect(chips()).toEqual(["q3 report.zip (1.2 MB, read-only)"]);
    expect(host.querySelector("ul.chips li")?.getAttribute("title")).toContain("cannot change it");
    type("what is in it");
    await send();
    expect(sent[0]!.attachments).toEqual(["k1"]);
  });

  test("the home bar's plain mode names the agent the chat will start with", async () => {
    show({ where: "home", agentId: "general" });
    upload.mockRejectedValue(new Error("Researcher cannot read attached files: turn on \"Read attached files\" for it on the Agents page"));
    await pick([zip()]);
    expect(upload).toHaveBeenCalledWith(expect.anything(), { session: undefined, folder: false, agent: "general" });
    expect(host.querySelector(".error-text")?.textContent).toContain("Read attached files");
  });

  test("a file over the limit shows the number the server gave", async () => {
    show({ where: "session", filesOf: "sid-1" });
    upload.mockRejectedValue(new Error("Files are at most 10 MB."));
    await pick([zip()]);
    expect(host.querySelector(".error-text")?.textContent).toContain("10 MB");
  });

  test("at most 10 files and images go with one message, and the box says so", async () => {
    show({ where: "session", filesOf: "sid-1" });
    let n = 0;
    upload.mockImplementation(async () => ({ attachment_id: `a${n++}`, name: `f${n}.zip`, mime: "application/zip", bytes: 5, saved: true }));
    await pick(Array.from({ length: 12 }, (_, i) => new File([new Uint8Array(3)], `f${i}.zip`)));
    expect(upload).toHaveBeenCalledTimes(10);
    expect(host.querySelector(".error-text")?.textContent).toContain("At most 10 files and images");
    expect(host.querySelector<HTMLButtonElement>('button[aria-label="Attach files"]')?.disabled).toBe(true);
  });
});
