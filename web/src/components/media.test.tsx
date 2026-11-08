// @vitest-environment happy-dom
// The Media page against a mocked API: tiles and what they say to a screen
// reader, filters (the route), origin and name search, empty states, upload
// with progress, selecting several to delete, and the preview: ←/→, Esc,
// rename, delete (with what stays in chats), "Use in chat", and the
// fallbacks to Download (an image too large to decode, a HEIC or MOV the
// browser cannot show).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { MediaItem, MediaPage, MediaQuery, MyStorage } from "../lib/media";
import type { MediaFilter, Route } from "../lib/routes";
import type { Features, Me } from "../types";

class HttpError extends Error {
  constructor(
    public status: number,
    m: string,
  ) {
    super(m);
  }
}
vi.mock("../api", () => ({ HttpError, call: vi.fn() }));
const mediaApi = {
  list: vi.fn<(q: MediaQuery) => Promise<MediaPage>>(),
  get: vi.fn<(id: string) => Promise<MediaItem>>(),
  update: vi.fn(),
  remove: vi.fn<(ids: string[]) => Promise<{ ok: boolean }>>(),
  putThumb: vi.fn(),
};
const storageApi = { mine: vi.fn<() => Promise<MyStorage>>(), admin: vi.fn(), save: vi.fn() };
const upload = vi.fn();
const deleteMedia = vi.fn<(items: MediaItem[], both: boolean) => Promise<string | null>>();
vi.mock("../lib/media", async (original) => ({ ...(await original<typeof import("../lib/media")>()), mediaApi, storageApi, upload, deleteMedia }));
const queueThumb = vi.fn();
vi.mock("../lib/thumbs", async (original) => ({ ...(await original<typeof import("../lib/thumbs")>()), queueThumb }));
const { default: Media } = await import("./Media");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const id = (n: number) => String(n).padStart(32, "a");
const item = (n: number, over: Partial<MediaItem> = {}): MediaItem => ({
  media_id: id(n),
  name: `photo${n}.jpg`,
  kind: "image",
  format: "jpeg",
  mime: "image/jpeg",
  size: 2.3 * 1024 * 1024,
  width: 1200,
  height: 800,
  duration_ms: null,
  display: "inline",
  has_thumb: true,
  no_thumb: false,
  origin: "upload",
  source: null,
  artifact: null,
  chats: 0,
  workspace_id: "w1",
  created_ms: Date.now() - 3 * 86_400_000,
  updated_ms: 0,
  ...over,
});
const page = (items: MediaItem[], next: string | null = null): MediaPage => ({ items, next, usage: { used: 0, quota: 0 }, limits: { max_file: 200 * 1024 * 1024 } });
const me: Me = { user_id: "u1", username: "alex", role: "member", can_agentic: true, must_change_password: false };
const features: Features = {
  voice: false,
  memory: false,
  routines: { enabled: false, show: false, webhooks: false, why: null },
  media: { enabled: true, show: true, max_file: 200 * 1024 * 1024, vision_images: false },
  artifacts: { enabled: false, show: false, why: null },
  search: { kinds: ["chats", "projects", "media"] },
};

let host: HTMLElement;
let root: Root;
const navigate = vi.fn<(r: Route) => void>();
const live = { onTopic: vi.fn(() => () => {}) } as unknown as Live;
beforeEach(() => {
  for (const f of [...Object.values(mediaApi), ...Object.values(storageApi), upload, queueThumb, navigate, deleteMedia]) f.mockReset();
  deleteMedia.mockResolvedValue(null);
  storageApi.mine.mockResolvedValue({ used: 1.5 * 1024 ** 3, quota: 4 * 1024 ** 3, media: { bytes: 0, items: 0 }, artifacts: { bytes: 0, versions: 0 } });
  vi.stubGlobal("confirm", () => true);
  history.replaceState(null, "", "#/media");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

const flush = () => act(() => new Promise((r) => setTimeout(r, 0)));
/** Until `ok` holds, a render at a time: a lazy chunk's import takes real
 * time, long on a machine running every test file at once. */
const until = (ok: () => unknown) =>
  vi.waitFor(
    async () => {
      await act(async () => {});
      if (!ok()) throw new Error("not yet");
    },
    { timeout: 15_000, interval: 10 },
  );
async function show(p: { filter?: MediaFilter | null; id?: string | null; features?: Features } = {}) {
  await act(async () =>
    root.render(
      <Media me={me} scope="w1" navigate={navigate} live={live} features={p.features ?? features} filter={p.filter ?? null} id={p.id ?? null} />,
    ),
  );
  // The preview is its own chunk.
  if (p.id) await until(() => host.querySelector("dialog"));
  await flush();
}
const tiles = () => [...host.querySelectorAll<HTMLButtonElement>(".media-tile")];
const button = (text: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === text || b.getAttribute("aria-label") === text);
function type(el: HTMLInputElement, value: string) {
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const key = (el: Element, k: string) => act(() => el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true })));

describe("grid", () => {
  test("tiles: thumbnail, video length, audio name, a file's type; each labelled", async () => {
    mediaApi.list.mockResolvedValue(
      page([
        item(1),
        item(2, { kind: "video", format: "mp4", name: "clip.mp4", duration_ms: 83_000, size: 12 * 1024 * 1024 }),
        item(3, { kind: "audio", format: "mp3", name: "song.mp3", duration_ms: 200_000, has_thumb: false, size: 4000 }),
        item(4, { kind: "other", format: "mkv", name: "film.mkv", display: "download", has_thumb: false }),
      ]),
    );
    await show();
    expect(tiles().map((t) => t.getAttribute("aria-label"))).toEqual([
      expect.stringMatching(/^photo1\.jpg, Image, 2\.3 MB, added \w+$/),
      expect.stringMatching(/^clip\.mp4, Video, 12 MB, added /),
      expect.stringMatching(/^song\.mp3, Audio, 3\.9 KB, added /),
      expect.stringMatching(/^film\.mkv, File, 2\.3 MB, added /),
    ]);
    expect(tiles()[0]!.querySelector("img")!.getAttribute("src")).toBe(`/api/media/${id(1)}/thumb`);
    expect(tiles()[1]!.querySelector(".media-badge")!.textContent).toBe(" 1:23");
    expect(tiles()[2]!.textContent).toBe("song.mp33:20");
    expect(tiles()[3]!.textContent).toBe("film.mkvMKV");
    expect(host.querySelector(".media-usage")!.textContent).toBe("1.5 GB of 4.0 GB used");
    expect(mediaApi.list).toHaveBeenCalledWith({ workspace: "w1", kind: null, origin: null, q: "" });
  });

  test("thumbnails are asked for as tiles come into view, only where none exists or was refused", async () => {
    // Every observed tile is in view at once.
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(private cb: (e: { isIntersecting: boolean; target: Element }[]) => void) {}
        observe(target: Element) {
          this.cb([{ isIntersecting: true, target }]);
        }
        unobserve() {}
        disconnect() {}
      },
    );
    mediaApi.list.mockResolvedValue(page([item(1), item(2, { has_thumb: false }), item(3, { has_thumb: false, no_thumb: true })]));
    await show();
    expect(queueThumb.mock.calls.map((c) => (c[0] as MediaItem).media_id)).toEqual([id(2)]);
  });

  test("filters: the kind is the route; origin and name search narrow the list", async () => {
    mediaApi.list.mockResolvedValue(page([item(1)]));
    await show({ filter: "videos" });
    expect(mediaApi.list).toHaveBeenLastCalledWith({ workspace: "w1", kind: "video", origin: null, q: "" });
    const seg = host.querySelector('[role="radiogroup"][aria-label="Show"]')!;
    act(() => [...seg.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Images")!.click());
    expect(navigate).toHaveBeenLastCalledWith({ kind: "media", filter: "images", id: null });
    act(() => [...seg.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "All")!.click());
    expect(navigate).toHaveBeenLastCalledWith({ kind: "media", filter: null, id: null });
    const origin = host.querySelector<HTMLSelectElement>('select[aria-label="Origin"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(origin, "chat");
      origin.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(mediaApi.list).toHaveBeenLastCalledWith({ workspace: "w1", kind: "video", origin: "chat", q: "" });
    type(host.querySelector<HTMLInputElement>('input[type="search"]')!, "beach");
    await act(() => new Promise((r) => setTimeout(r, 300)));
    expect(mediaApi.list).toHaveBeenLastCalledWith({ workspace: "w1", kind: "video", origin: "chat", q: "beach" });
  });

  test("empty states", async () => {
    mediaApi.list.mockResolvedValue(page([]));
    await show();
    expect(host.querySelector(".media-empty")!.textContent).toBe("Nothing here yet. Upload images, video or music.");
    await show({ filter: "images" });
    expect(host.querySelector(".media-empty")!.textContent).toBe("No images here yet.");
    type(host.querySelector<HTMLInputElement>('input[type="search"]')!, "zebra");
    await act(() => new Promise((r) => setTimeout(r, 300)));
    expect(host.querySelector(".media-empty")!.textContent).toBe("Nothing matches “zebra”.");
  });

  test("once Artifacts shows, the empty grid also names agents", async () => {
    mediaApi.list.mockResolvedValue(page([]));
    await show({ features: { ...features, artifacts: { enabled: true, show: true, why: null } } });
    expect(host.querySelector(".media-empty")!.textContent).toBe(
      "Nothing here yet. Upload images, video or music, or ask an agent to publish a file it made.",
    );
  });

  test("an agent's item wears the agent mark and says so", async () => {
    const art = { artifact_id: id(9), version_id: id(8), seq: 3, name: "chart" };
    mediaApi.list.mockResolvedValue(page([item(1, { origin: "agent", artifact: art, name: "chart.png" }), item(2)]));
    await show();
    expect(tiles()[0]!.getAttribute("aria-label")).toMatch(/^chart\.png, Image, 2\.3 MB, added .*, published by an agent$/);
    expect(tiles()[0]!.querySelector(".media-agent")).not.toBeNull();
    expect(tiles()[1]!.querySelector(".media-agent")).toBeNull();
  });

  test("a server with Media turned off answers 404", async () => {
    mediaApi.list.mockRejectedValue(new HttpError(404, "not found"));
    await show();
    expect(host.textContent).toContain("Media is not turned on on this server.");
  });

  test("Load more by cursor", async () => {
    mediaApi.list.mockResolvedValueOnce(page([item(1)], "c1")).mockResolvedValueOnce(page([item(2)]));
    await show();
    await act(async () => button("Load more")!.click());
    expect(mediaApi.list).toHaveBeenLastCalledWith({ workspace: "w1", kind: null, origin: null, q: "", before: "c1" });
    expect(tiles()).toHaveLength(2);
    expect(button("Load more")).toBeUndefined();
  });
});

describe("upload", () => {
  const pick = (files: File[]) =>
    act(async () => {
      const input = host.querySelector<HTMLInputElement>('input[type="file"]')!;
      Object.defineProperty(input, "files", { value: files, configurable: true });
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });

  test("each file streams up with progress; a new one joins the grid and gets a thumbnail", async () => {
    mediaApi.list.mockResolvedValue(page([item(1)]));
    await show();
    let progress: (s: number) => void = () => {};
    let finish: (v: unknown) => void = () => {};
    upload.mockImplementationOnce((_f: File, _ws: string, onProgress: (s: number) => void) => {
      progress = onProgress;
      return new Promise((r) => (finish = r));
    });
    const file = new File(["x"], "new.png", { type: "image/png" });
    await pick([file]);
    expect(upload).toHaveBeenCalledWith(file, "w1", expect.any(Function));
    act(() => progress(0.5));
    expect(host.querySelector<HTMLProgressElement>(".media-uploads progress")!.value).toBe(0.5);
    const fresh = item(9, { name: "new.png", has_thumb: false });
    await act(async () => finish({ item: fresh, duplicate: false }));
    expect(host.querySelector(".media-uploads")).toBeNull();
    expect(queueThumb).toHaveBeenCalledWith(fresh, expect.any(Function));
  });

  test("too large is refused before sending; a duplicate says so; errors stay listed", async () => {
    mediaApi.list.mockResolvedValue(page([item(1)]));
    await show();
    upload.mockResolvedValueOnce({ item: item(1), duplicate: true }).mockRejectedValueOnce(new HttpError(415, "not a media file"));
    const big = new File(["x"], "huge.mov");
    Object.defineProperty(big, "size", { value: 300 * 1024 * 1024 });
    await pick([big, new File(["y"], "again.jpg"), new File(["z"], "doc.pdf")]);
    await flush();
    const rows = [...host.querySelectorAll(".media-uploads li")].map((l) => l.textContent);
    expect(rows).toEqual(["huge.movToo large: the limit is 200 MB.Dismiss", "again.jpgAlready in Media.Dismiss", "doc.pdfnot a media fileDismiss"]);
    expect(upload).toHaveBeenCalledTimes(2);
    expect(queueThumb).not.toHaveBeenCalled();
  });
});

describe("selection", () => {
  test("Select picks tiles, Delete removes them after a confirm", async () => {
    mediaApi.list.mockResolvedValue(page([item(1), item(2), item(3)]));
    mediaApi.remove.mockResolvedValue({ ok: true });
    await show();
    act(() => button("Select")!.click());
    act(() => tiles()[0]!.click());
    act(() => tiles()[2]!.click());
    expect(tiles().map((t) => t.getAttribute("aria-pressed"))).toEqual(["true", "false", "true"]);
    expect(host.querySelector(".media-selection")!.textContent).toContain("2 selected");
    expect(navigate).not.toHaveBeenCalled();
    act(() => button("Delete")!.click());
    // Asked in place; none came from an agent, so there is no "Delete from both".
    expect(host.querySelector(".media-selection")!.textContent).toContain("Delete these 2 items from Media?");
    expect(button("Delete from both")).toBeUndefined();
    expect(deleteMedia).not.toHaveBeenCalled();
    await act(async () => button("Delete")!.click());
    expect(deleteMedia).toHaveBeenCalledWith([item(1), item(3)].map((m) => expect.objectContaining({ media_id: m.media_id })), false);
    expect(host.querySelector(".media-selection")).toBeNull();
  });

  test("agent items in the selection stay in Artifacts unless deleted from both", async () => {
    const art = { artifact_id: id(9), version_id: id(8), seq: 3, name: "chart" };
    mediaApi.list.mockResolvedValue(page([item(1, { origin: "agent", artifact: art }), item(2)]));
    deleteMedia.mockResolvedValue("Deleted from Media, but these stay in Artifacts: chart v3.");
    await show();
    act(() => button("Select")!.click());
    act(() => tiles()[0]!.click());
    act(() => tiles()[1]!.click());
    act(() => button("Delete")!.click());
    expect(host.querySelector(".media-selection")!.textContent).toContain("One came from an agent and stays in Artifacts");
    act(() => button("Cancel")!.click());
    expect(host.querySelector(".media-selection")!.textContent).toContain("2 selected");
    act(() => button("Delete")!.click());
    await act(async () => button("Delete from both")!.click());
    expect(deleteMedia).toHaveBeenCalledWith([expect.objectContaining({ media_id: id(1) }), expect.objectContaining({ media_id: id(2) })], true);
    expect(host.querySelector(".error-text")!.textContent).toBe("Deleted from Media, but these stay in Artifacts: chart v3.");
  });
});

describe("preview", () => {
  test("a tile opens it; ←/→ step through the grid; Esc goes back", async () => {
    mediaApi.list.mockResolvedValue(page([item(1), item(2), item(3)]));
    await show();
    act(() => tiles()[1]!.click());
    expect(navigate).toHaveBeenLastCalledWith({ kind: "media", filter: null, id: id(2) });
    await show({ id: id(2) });
    const dialog = host.querySelector<HTMLDialogElement>("dialog.media-preview")!;
    expect(dialog.getAttribute("aria-label")).toBe("photo2.jpg");
    expect(dialog.querySelector("img")!.getAttribute("src")).toBe(`/api/media/${id(2)}/content`);
    key(dialog, "ArrowRight");
    expect(location.hash).toBe(`#/media/${id(3)}`);
    key(dialog, "ArrowLeft");
    expect(location.hash).toBe(`#/media/${id(1)}`);
    // Typing a name does not move.
    key(dialog.querySelector('input[aria-label="Name"]')!, "ArrowLeft");
    expect(location.hash).toBe(`#/media/${id(1)}`);
    // Opened from a tile, so closing is Back.
    const back = vi.spyOn(history, "back").mockImplementation(() => {});
    act(() => dialog.dispatchEvent(new Event("cancel", { cancelable: true })));
    expect(back).toHaveBeenCalled();
  });

  test("a link opens it over the grid, even for an item not loaded; closing lands on the grid", async () => {
    mediaApi.list.mockResolvedValue(page([item(1)]));
    mediaApi.get.mockResolvedValue(item(7, { name: "old.jpg" }));
    await show({ filter: "images", id: id(7) });
    const dialog = host.querySelector<HTMLDialogElement>("dialog.media-preview")!;
    expect(dialog.getAttribute("aria-label")).toBe("old.jpg");
    act(() => button("Close")!.click());
    expect(navigate).toHaveBeenLastCalledWith({ kind: "media", filter: "images", id: null });
  });

  test("rename and delete", async () => {
    mediaApi.list.mockResolvedValue(page([item(1)]));
    mediaApi.update.mockResolvedValue(item(1, { name: "beach.jpg" }));
    mediaApi.remove.mockResolvedValue({ ok: true });
    await show({ id: id(1) });
    type(host.querySelector<HTMLInputElement>('dialog input[aria-label="Name"]')!, "beach.jpg");
    await act(async () => button("Rename")!.click());
    expect(mediaApi.update).toHaveBeenCalledWith(id(1), { name: "beach.jpg" });
    expect(host.querySelector("dialog h2")!.textContent).toBe("beach.jpg");
    act(() => host.querySelector<HTMLButtonElement>("dialog button.danger")!.click());
    const ask = host.querySelector(".media-confirm")!;
    expect(ask.textContent).toContain("Delete beach.jpg from Media?");
    expect([...ask.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Cancel", "Delete"]);
    await act(async () => [...ask.querySelectorAll("button")].find((b) => b.textContent === "Delete")!.click());
    expect(deleteMedia).toHaveBeenCalledWith([expect.objectContaining({ media_id: id(1) })], false);
  });

  test("an agent's item: Also in Artifacts, and Delete from both", async () => {
    const art = { artifact_id: id(9), version_id: id(8), seq: 3, name: "chart" };
    mediaApi.list.mockResolvedValue(page([item(1, { origin: "agent", artifact: art, source: { session_id: "s1", title: "Plots" } })]));
    await show({ id: id(1) });
    const also = [...host.querySelectorAll<HTMLAnchorElement>("dialog a")].find((a) => a.textContent?.startsWith("Also in Artifacts"))!;
    expect(also.textContent).toBe("Also in Artifacts: chart v3");
    expect(also.getAttribute("href")).toBe(`#/artifacts/${id(9)}`);
    act(() => host.querySelector<HTMLButtonElement>("dialog button.danger")!.click());
    const ask = host.querySelector(".media-confirm")!;
    expect(ask.textContent).toContain("The file stays in Artifacts (chart v3); delete it there to free space.");
    await act(async () => [...ask.querySelectorAll("button")].find((b) => b.textContent === "Delete from both")!.click());
    expect(deleteMedia).toHaveBeenCalledWith([expect.objectContaining({ media_id: id(1) })], true);
  });

  test("an image chats still carry says so before it goes, one at a time or several", async () => {
    mediaApi.list.mockResolvedValue(page([item(1, { origin: "chat", chats: 2 }), item(2, { chats: 1 }), item(3)]));
    await show({ id: id(1) });
    act(() => host.querySelector<HTMLButtonElement>("dialog button.danger")!.click());
    expect(host.querySelector(".media-confirm")!.textContent).toContain(
      "Still in 2 chats. Delete those chats to remove it there. Copies already sent to the model provider can't be recalled.",
    );
    await show();
    act(() => button("Select")!.click());
    tiles().forEach((t) => act(() => t.click()));
    act(() => button("Delete")!.click());
    expect(host.querySelector(".media-selection")!.textContent).toContain(
      "2 are still in chats; delete those chats to remove them there. Copies already sent to the model provider can't be recalled.",
    );
  });

  test("Use in chat: an image a message can carry waits for the home bar", async () => {
    const vision = { ...features, media: { ...features.media, vision_images: true } };
    mediaApi.list.mockResolvedValue(
      page([item(1), item(2, { kind: "video", format: "mp4" }), item(3, { size: 6 * 1024 * 1024 }), item(4, { width: 9000, height: 9000 })]),
    );
    sessionStorage.setItem("harness.pendingImages", JSON.stringify([id(9)]));
    await show({ id: id(1), features: vision });
    act(() => button("Use in chat")!.click());
    expect(JSON.parse(sessionStorage.getItem("harness.pendingImages")!)).toEqual([id(9), id(1)]);
    expect(navigate).toHaveBeenLastCalledWith({ kind: "home" });
    // Not a video, nor over 5 MB, nor too large to decode; nor while the server takes no chat images.
    for (const n of [2, 3, 4]) {
      await show({ id: id(n), features: vision });
      expect(button("Use in chat")).toBeUndefined();
    }
    await show({ id: id(1) });
    expect(button("Use in chat")).toBeUndefined();
    sessionStorage.clear();
  });

  test("falls back to Download: too large to decode, a HEIC or MOV the browser cannot show", async () => {
    const huge = item(1, { width: 8000, height: 6000 });
    const heic = item(2, { format: "heic", display: "img", name: "phone.heic" });
    const mov = item(3, { kind: "video", format: "mov", name: "phone.mov" });
    mediaApi.list.mockResolvedValue(page([huge, heic, mov]));
    await show({ id: id(1) });
    const stage = () => host.querySelector(".media-stage")!;
    expect(stage().querySelector("img")).toBeNull();
    expect(stage().textContent).toContain("This image is too large to show here safely.");
    expect(stage().querySelector("a")!.getAttribute("href")).toBe(`/api/media/${id(1)}/content?download=1`);

    await show({ id: id(2) });
    act(() => stage().querySelector("img")!.dispatchEvent(new Event("error")));
    expect(stage().textContent).toContain("This browser can't show HEIC images.");
    expect(stage().querySelector("a")!.textContent).toBe(" Download");

    await show({ id: id(3) });
    const video = stage().querySelector("video")!;
    expect(video.getAttribute("preload")).toBe("metadata");
    act(() => video.dispatchEvent(new Event("error")));
    expect(stage().textContent).toContain("This browser can't play this video.");
  });
});
