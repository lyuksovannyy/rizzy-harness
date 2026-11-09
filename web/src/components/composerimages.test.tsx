// @vitest-environment happy-dom
// Images in the bar (docs/media.md §6.6): the paperclip takes them only
// where the server does, never in search or a temporary chat; picked, pasted or dropped
// images go to lib/downscale (mocked here; its own tests shrink and upload)
// with the bar's room and workspace, and their chips wait for the Media id;
// a model that cannot see them is named, and the message carries the ids.
// The home bar asks downscale for what "Use in chat" left; documents still
// attach as before.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Bar, Picture } from "../lib/downscale";
import type { ModelInfo } from "../types";
import type { Submission } from "./Composer";

const upload = vi.fn();
vi.mock("../api", () => ({ api: { upload: (f: File) => upload(f) }, call: vi.fn(), HttpError: Error }));
const addImages = vi.fn<(list: File[], bar: Bar) => Promise<void>>();
const pickUp = vi.fn<(set: Bar["set"]) => Promise<void>>();
vi.mock("../lib/downscale", () => ({ addImages: (l: File[], b: Bar) => addImages(l, b), pickUp: (s: Bar["set"]) => pickUp(s) }));
const { Composer } = await import("./Composer");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const model = (id: string, vision: boolean): ModelInfo => ({
  id,
  raw: id.split("::")[1]!,
  provider: "demo",
  provider_id: "demo",
  name: null,
  context: null,
  max_output: null,
  effort: false,
  vision,
  tools: true,
  default: id === "demo::eyes",
  unavailable: false,
});
const MODELS = [model("demo::eyes", true), model("demo::blind", false)];
const picture = (n: number, uploading = false): Picture => ({ key: `k${n}`, name: `p${n}.webp`, size: 1000, ...(uploading ? {} : { media_id: String(n).padStart(32, "a") }) });
const png = (name = "shot.png") => new File(["x"], name, { type: "image/png" });
/** What the last call handed over, to drive the chips as downscale would. */
const lastBar = () => addImages.mock.calls.at(-1)![1];

let host: HTMLElement;
let root: Root;
const sent: Submission[] = [];
beforeEach(() => {
  addImages.mockReset().mockResolvedValue();
  upload.mockReset();
  pickUp.mockReset().mockResolvedValue();
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

/** Until `ok` holds, a render at a time (the downscale chunk is imported on first use). */
const until = (ok: () => unknown) =>
  vi.waitFor(
    async () => {
      await act(async () => {});
      if (!ok()) throw new Error("not yet");
    },
    { timeout: 15_000, interval: 10 },
  );

function show(p: { images?: { workspace: string | null; pending?: boolean; left?: number } | null; models?: ModelInfo[]; where?: "home" | "session" } = {}) {
  act(() =>
    root.render(
      <Composer
        where={p.where ?? "home"}
        agents={[]}
        models={p.models ?? MODELS}
        workers={[]}
        agentId=""
        onAgentChange={() => {}}
        agenticBlocked={null}
        sessionModel="demo::eyes"
        onSubmit={async (s) => {
          sent.push(s);
        }}
        images={p.images === undefined ? { workspace: "w1" } : p.images}
      />,
    ),
  );
}
const attach = () => host.querySelector<HTMLButtonElement>('button[aria-label="Attach files"]');
const input = () => host.querySelector<HTMLInputElement>('input[type="file"]')!;
const chips = () => [...host.querySelectorAll<HTMLLIElement>(".image-chip")];
const pick = (files: File[]) =>
  act(async () => {
    Object.defineProperty(input(), "files", { value: files, configurable: true });
    input().dispatchEvent(new Event("change", { bubbles: true }));
  });
function type(value: string) {
  const ta = host.querySelector("textarea")!;
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(ta, value);
    ta.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const send = () => act(async () => host.querySelector<HTMLButtonElement>("button.send")!.click());
const error = () => host.querySelector(".error-text")?.textContent;

describe("where images are offered", () => {
  test("only where the server takes them; never in search or a temporary chat", () => {
    const hint = () => attach()?.title;
    show({ images: null });
    expect(hint()).toBe("Attach documents (text, code, Markdown, CSV, JSON, PDF; up to 5)");
    expect(input().accept).not.toContain("image/");
    show();
    expect(hint()).toBe("Attach documents or images (images are kept in Media)");
    expect(input().accept).toContain(",image/*,.heic,.heif");
    act(() => host.querySelector<HTMLButtonElement>("button.incognito")!.click());
    expect(hint()).not.toContain("images");
    act(() => host.querySelector<HTMLButtonElement>("button.incognito")!.click());
    act(() => [...host.querySelectorAll<HTMLButtonElement>(".mode-switch button")].find((b) => b.textContent === "Search")!.click());
    expect(attach()).toBeNull();
  });
});

describe("attaching", () => {
  test("picked images go to downscale with the room and workspace; chips wait for the Media id; the message carries the ids", async () => {
    show();
    type("what is this?");
    const file = png();
    await pick([file]);
    await until(() => addImages.mock.calls.length === 1);
    expect(addImages.mock.calls[0]![0]).toEqual([file]);
    expect(lastBar()).toMatchObject({ workspace: "w1", room: 4, pictures: [] });
    act(() => lastBar().set([picture(1, true)]));
    expect(chips()[0]!.querySelector(".spin")).not.toBeNull();
    // Nothing goes while an image is still on its way.
    expect(host.querySelector<HTMLButtonElement>("button.send")!.disabled).toBe(true);
    act(() => lastBar().set([picture(1)]));
    expect(chips()[0]!.textContent).toBe("p1.webp");
    expect(chips()[0]!.querySelector("img")!.getAttribute("src")).toBe(`/api/media/${picture(1).media_id}/content`);
    act(() => lastBar().fail("p2.png: This browser can't read this image."));
    expect(error()).toBe("p2.png: This browser can't read this image.");
    await send();
    expect(sent[0]!.images).toEqual([picture(1).media_id]);
    expect(chips()).toHaveLength(0);
  });

  test("the room left: 4 a message, less where the chat has fewer left; a chip can be removed; documents still attach", async () => {
    show({ images: { workspace: "w1", left: 2 } });
    await pick([png()]);
    await until(() => addImages.mock.calls.length === 1);
    expect(lastBar().room).toBe(2);
    act(() => lastBar().set([picture(1), picture(2)]));
    await pick([png("third.png")]);
    await until(() => addImages.mock.calls.length === 2);
    expect(lastBar().room).toBe(0);
    act(() => chips()[0]!.querySelector<HTMLButtonElement>("button")!.click());
    expect(chips().map((c) => c.textContent)).toEqual(["p2.webp"]);
    upload.mockResolvedValue({ attachment_id: "a1", name: "notes.md", chars: 10, truncated: false });
    await pick([new File(["# hi"], "notes.md", { type: "text/markdown" })]);
    expect(upload).toHaveBeenCalledWith(expect.objectContaining({ name: "notes.md" }));
    expect(addImages).toHaveBeenCalledTimes(2);
  });

  test("pasted and dropped images go with the message; pasted text stays text", async () => {
    show();
    const ta = host.querySelector("textarea")!;
    const paste = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(paste, "clipboardData", { value: { files: [png("pasted.png")] } });
    act(() => ta.dispatchEvent(paste));
    expect(paste.defaultPrevented).toBe(true);
    // One at a time: two imports of a mocked module at once can miss the mock.
    await until(() => addImages.mock.calls.length === 1);
    const text = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(text, "clipboardData", { value: { files: [] } });
    act(() => ta.dispatchEvent(text));
    expect(text.defaultPrevented).toBe(false);
    const drop = new Event("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", { value: { files: [png("dropped.png")], types: ["Files"] } });
    act(() => host.querySelector(".composer")!.dispatchEvent(drop));
    expect(drop.defaultPrevented).toBe(true);
    await until(() => addImages.mock.calls.length === 2);
    expect(addImages.mock.calls.map((c) => c[0][0]!.name)).toEqual(["pasted.png", "dropped.png"]);
  });

  test("a model that cannot see them stops the message before any chat is made", async () => {
    show();
    await pick([png()]);
    await until(() => addImages.mock.calls.length === 1);
    act(() => lastBar().set([picture(1)]));
    act(() => {
      const select = host.querySelector<HTMLSelectElement>('select[aria-label="model"]')!;
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(select, "demo::blind");
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    type("look");
    await send();
    expect(sent).toHaveLength(0);
    expect(error()).toBe("blind can't read images; remove them to send.");
  });
});

describe("Use in chat", () => {
  test("the home bar asks downscale for what Use in chat left", async () => {
    pickUp.mockImplementation(async (set) => set([picture(3)]));
    show({ images: { workspace: "w1", pending: true } });
    await until(() => chips().length === 1);
    expect(chips()[0]!.textContent).toBe("p3.webp");
  });

  test("a bar that does not pick up (a chat's) never asks", async () => {
    show({ images: { workspace: "w1" } });
    await act(async () => {});
    expect(pickUp).not.toHaveBeenCalled();
  });
});
