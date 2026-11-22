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

const upload = vi.fn<(f: File, to: { session?: string; folder?: boolean }) => Promise<UploadedAttachment>>();
vi.mock("../api", () => ({ api: { upload: (f: File, to: { session?: string; folder?: boolean }) => upload(f, to) }, call: vi.fn(), HttpError: Error }));
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

function show(p: { where: "home" | "session"; filesOf?: string }) {
  act(() =>
    root.render(
      <Composer
        where={p.where}
        agents={[]}
        models={[]}
        workers={[]}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        filesOf={p.filesOf}
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

  test("a plain box names no folder, and shows the server's reason", async () => {
    show({ where: "home" });
    upload.mockRejectedValue(new Error("unsupported file: this chat's agent cannot open files; start a coding chat, or attach text, a PDF or an image"));
    await pick([zip()]);
    expect(upload).toHaveBeenCalledWith(expect.anything(), { session: undefined, folder: false });
    expect(host.querySelector(".error-text")?.textContent).toContain("cannot open files");
    expect(chips()).toEqual([]);
  });

  test("a file over the limit shows the number the server gave", async () => {
    show({ where: "session", filesOf: "sid-1" });
    upload.mockRejectedValue(new Error("Files are at most 50 MB."));
    await pick([zip()]);
    expect(host.querySelector(".error-text")?.textContent).toContain("50 MB");
  });
});
