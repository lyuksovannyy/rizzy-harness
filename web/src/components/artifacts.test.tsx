// @vitest-environment happy-dom
// The Artifacts page against a mocked API: one row per name (v badge, size,
// chip, "from" or "by", Download latest, "Also in Media" only where Media
// shows), the kind filters, Mine/Shared only in a team workspace, the name
// search, empty states, a row opened to its versions (status, progress, the
// failure, sha256 and its hint, pin and delete for the owner), the ⋯ menu,
// and the detail page (notes, facts, files in the pack, the preview rules).
import { act } from "react";
import { linked } from "../lib/links.setup";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Live } from "../live";
import type { Artifact, ArtifactPage, ArtifactQuery, ArtifactVersion, ArtifactWithVersions } from "../lib/artifacts";
import type { MediaItem } from "../lib/media";
import type { ProjectSummary } from "../lib/projects";
import type { Route } from "../lib/routes";
import type { Features, Me, WorkspaceSummary } from "../types";

class HttpError extends Error {
  constructor(
    public status: number,
    m: string,
  ) {
    super(m);
  }
}
vi.mock("../api", () => ({ HttpError, call: vi.fn() }));
const artifactsApi = {
  list: vi.fn<(q: ArtifactQuery) => Promise<ArtifactPage>>(),
  get: vi.fn<(id: string) => Promise<ArtifactWithVersions>>(),
  update: vi.fn(),
  remove: vi.fn(),
  pin: vi.fn(),
  removeVersion: vi.fn(),
  forSession: vi.fn(),
  save: vi.fn(),
};
vi.mock("../lib/artifacts", async (original) => ({ ...(await original<typeof import("../lib/artifacts")>()), artifactsApi }));
const mediaApi = { get: vi.fn<(id: string) => Promise<MediaItem>>() };
vi.mock("../lib/media", async (original) => ({ ...(await original<typeof import("../lib/media")>()), mediaApi }));
const { default: Artifacts } = await import("./Artifacts");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const id = (n: number) => String(n).padStart(32, "a");
const version = (over: Partial<ArtifactVersion> = {}): ArtifactVersion => ({
  version_id: "v7",
  seq: 7,
  status: "ready",
  error: null,
  progress: null,
  size: 12.3 * 1024 * 1024,
  sha256: "3f2a9c81b0d4".padEnd(64, "e"),
  filename: "myapp",
  packed: null,
  kind: "binary",
  format: "elf",
  display: "download",
  meta: { target: "x86_64-unknown-linux-gnu", git: { commit: "abcdef1234567", branch: "main", dirty: true } },
  notes: "",
  node: { node_id: "n1", hostname: "box" },
  session: { session_id: "11111111-1111-1111-1111-111111111111", title: "Build the CLI" },
  by_user: false,
  pinned: false,
  has_thumb: false,
  created_ms: Date.now() - 60_000,
  ...over,
});
const artifact = (n: number, over: Partial<Artifact> = {}): Artifact => ({
  artifact_id: id(n),
  name: `app${n}`,
  kind: "binary",
  visibility: "private",
  mine: true,
  owner: { username: "alex" },
  project_id: null,
  versions: 2,
  latest: version(),
  media_id: null,
  updated_ms: Date.now() - 3_600_000,
  ...over,
});
const listing = (artifacts: Artifact[], next_cursor: string | null = null): ArtifactPage => ({ artifacts, next_cursor, usage: { used: 1024 ** 3, quota: 4 * 1024 ** 3 } });
const detail = (a: Artifact, versions: ArtifactVersion[]): ArtifactWithVersions => ({ ...a, versions });

const PERSONAL: WorkspaceSummary = { workspace_id: "w1", name: "alex", personal: true, role: "owner", permissions: ["settings", "members", "routines"], members: 1, exec_level: "host" as const };
const TEAM: WorkspaceSummary = { workspace_id: "w2", name: "Acme", personal: false, role: "member", permissions: ["routines"], members: 3, exec_level: "host" as const };
const meIn = (workspace: WorkspaceSummary): Me => ({ user_id: "u1", username: "alex", role: "member", exec_ceiling: "host" as const, must_change_password: false, workspace });
const features = (over: Partial<Features> = {}): Features => ({
  voice: false,
  memory: false,
  routines: { enabled: true, show: true, webhooks: true, why: null },
  media: { enabled: false, show: false, max_file: 0, vision_images: false },
  artifacts: { enabled: true, show: true, why: null },
  search: { kinds: ["chats", "projects", "artifacts"] },
  ...over,
});

let host: HTMLElement;
let root: Root;
const navigate = vi.fn<(r: Route) => void>();
const live = { onTopic: vi.fn(() => () => {}) } as unknown as Live;
beforeEach(() => {
  for (const f of [...Object.values(artifactsApi), mediaApi.get, navigate]) f.mockReset();
  vi.stubGlobal("confirm", () => true);
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
async function show(p: { id?: string | null; me?: Me; features?: Features; projects?: ProjectSummary[] } = {}) {
  await act(async () =>
    root.render(
      <Artifacts me={p.me ?? meIn(PERSONAL)} scope="w1" navigate={navigate} live={live} features={p.features ?? features()} id={p.id ?? null} projects={p.projects ?? []} />,
    ),
  );
  await flush();
}
const PROJECT: ProjectSummary = { project_id: id(7), workspace_id: "w1", name: "CLI", description: "", icon: "box", accent: "blue", chats: 2, updated_ms: 0 };
const rows = () => [...host.querySelectorAll<HTMLLIElement>(".artifact-row")];
const buttons = (within: ParentNode = host) => [...within.querySelectorAll<HTMLButtonElement>("button")];
const button = (text: string, within: ParentNode = host) => buttons(within).find((b) => b.textContent?.trim() === text || b.getAttribute("aria-label") === text);
const radio = (group: string, label: string) =>
  [...host.querySelector(`[role="radiogroup"][aria-label="${group}"]`)!.querySelectorAll<HTMLButtonElement>('[role="radio"]')].find((b) => b.textContent === label)!;
function type(el: HTMLInputElement, value: string) {
  act(() => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("list", () => {
  test("one row per name: badge, size, chip, where it came from, Download latest", async () => {
    artifactsApi.list.mockResolvedValue(
      listing([artifact(1), artifact(2, { mine: false, visibility: "workspace", owner: { username: "bob" }, latest: version({ session: { private: true }, meta: {} }) })]),
    );
    await show({ me: meIn(TEAM) });
    expect(artifactsApi.list).toHaveBeenCalledWith({ workspace: "w1", project: null, kind: null, mine: false, q: "" });
    expect(host.querySelector(".media-usage")!.textContent).toBe("1.0 GB of 4.0 GB used");
    const [mine, theirs] = rows();
    expect(mine!.querySelector(".artifact-line")!.textContent).toBe("app1v712 MBx86_64-unknown-linux-gnu");
    expect(mine!.querySelector(".agent-info .muted")!.textContent).toMatch(/^from ‘Build the CLI’ · /);
    expect(mine!.querySelector<HTMLAnchorElement>("a.button-like")!.getAttribute("href")).toBe(linked(`/api/artifacts/${id(1)}/latest/download`));
    expect(theirs!.querySelector(".artifact-line")!.textContent).toContain("ELFShared");
    expect(theirs!.querySelector(".agent-info .muted")!.textContent).toMatch(/^by bob · /);
    // A colleague's shared artifact: no menu for a member without Settings.
    expect(theirs!.querySelector(".artifact-menu")).toBeNull();
    // The name opens the detail page.
    act(() => mine!.querySelector<HTMLAnchorElement>("a.agent-name")!.click());
    expect(navigate).toHaveBeenCalledWith({ kind: "artifacts", id: id(1) });
  });

  test("Also in Media only where Media shows", async () => {
    artifactsApi.list.mockResolvedValue(listing([artifact(1, { kind: "image", media_id: id(5) })]));
    await show();
    expect(host.textContent).not.toContain("Also in Media");
    await show({ features: features({ media: { enabled: true, show: true, max_file: 0, vision_images: false } }) });
    const link = [...host.querySelectorAll("a")].find((a) => a.textContent === "Also in Media")!;
    expect(link.getAttribute("href")).toBe(`#/media/${id(5)}`);
  });

  test("filters: kind chips, whose (team workspaces only), and the search", async () => {
    artifactsApi.list.mockResolvedValue(listing([artifact(1), artifact(2, { visibility: "workspace" })]));
    await show();
    expect(host.querySelector('[aria-label="Whose"]')).toBeNull();
    await act(async () => radio("Kind", "Archives").click());
    expect(artifactsApi.list).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "archives" }));

    await show({ me: meIn(TEAM) });
    await act(async () => radio("Whose", "Mine").click());
    expect(artifactsApi.list).toHaveBeenLastCalledWith(expect.objectContaining({ mine: true }));
    // Shared has no server filter: the shared rows of the answer.
    await act(async () => radio("Whose", "Shared").click());
    expect(artifactsApi.list).toHaveBeenLastCalledWith(expect.objectContaining({ mine: false }));
    expect(rows().map((r) => r.querySelector("a.agent-name")!.textContent)).toEqual(["app2"]);

    type(host.querySelector<HTMLInputElement>('input[type="search"]')!, "zebra");
    await act(() => new Promise((r) => setTimeout(r, 300)));
    expect(artifactsApi.list).toHaveBeenLastCalledWith(expect.objectContaining({ q: "zebra" }));
  });

  test("empty states: the invitation, the server's reason, nothing matching", async () => {
    artifactsApi.list.mockResolvedValue(listing([]));
    await show();
    expect(host.querySelector(".media-empty")!.textContent).toBe(
      "No artifacts yet. Ask an agent to build something and publish it, or use “Save as artifact” on a file in an agentic chat.",
    );
    await show({ features: features({ artifacts: { enabled: true, show: true, why: "You can't run agentic chats here." } }) });
    expect(host.querySelector(".media-empty")!.textContent).toBe("You can't run agentic chats here.");
    await act(async () => radio("Kind", "Media").click());
    expect(host.querySelector(".media-empty")!.textContent).toBe("Nothing here with these filters.");
  });

  test("the project filter, only where the workspace has projects", async () => {
    artifactsApi.list.mockResolvedValue(listing([artifact(1)]));
    await show();
    expect(host.querySelector('select[aria-label="Project"]')).toBeNull();
    await show({ projects: [PROJECT] });
    const select = host.querySelector<HTMLSelectElement>('select[aria-label="Project"]')!;
    expect([...select.options].map((o) => o.textContent)).toEqual(["Every project", "CLI"]);
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(select, id(7));
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(artifactsApi.list).toHaveBeenLastCalledWith(expect.objectContaining({ project: id(7) }));
    artifactsApi.list.mockResolvedValue(listing([]));
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")!.set!.call(select, "");
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect(artifactsApi.list).toHaveBeenLastCalledWith(expect.objectContaining({ project: null }));
  });

  test("a name with no version ready yet offers no Download", async () => {
    artifactsApi.list.mockResolvedValue(listing([artifact(1, { latest: version({ status: "pending", progress: { received: 1, total: 4 } }) })]));
    await show();
    expect(rows()[0]!.querySelector("a.button-like")).toBeNull();
    expect(rows()[0]!.querySelector(".agent-info .muted")!.textContent).toContain("no version ready yet");
  });

  test("Load more by cursor", async () => {
    artifactsApi.list.mockResolvedValueOnce(listing([artifact(1)], "c1")).mockResolvedValueOnce(listing([artifact(2)]));
    await show();
    await act(async () => button("Load more")!.click());
    expect(artifactsApi.list).toHaveBeenLastCalledWith(expect.objectContaining({ cursor: "c1" }));
    expect(rows()).toHaveLength(2);
    expect(button("Load more")).toBeUndefined();
  });
});

describe("versions", () => {
  test("a row opens to its versions: status, progress, the failure, sha256, pin and delete", async () => {
    const a = artifact(1, { versions: 3 });
    artifactsApi.list.mockResolvedValue(listing([a]));
    artifactsApi.get.mockResolvedValue(
      detail(a, [
        version({ version_id: "v8", seq: 8, status: "pending", progress: { received: 512, total: 1024 } }),
        version(),
        version({ version_id: "v6", seq: 6, status: "failed", error: "the node never sent the file" }),
      ]),
    );
    vi.stubGlobal("navigator", { clipboard: { writeText: vi.fn(() => Promise.resolve()) } });
    await show();
    const toggle = button("3 versions")!;
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    await act(async () => toggle.click());
    await flush();
    expect(artifactsApi.get).toHaveBeenCalledWith(id(1));
    const items = [...host.querySelectorAll<HTMLLIElement>(".artifact-version")];
    expect(items.map((li) => li.querySelector(".badge")!.textContent)).toEqual(["v8", "v7", "v6"]);
    expect(items[0]!.querySelector("progress")!.getAttribute("value")).toBe("512");
    expect([...items[0]!.querySelectorAll("a")].map((a) => a.textContent)).not.toContain("Download");
    expect(items[1]!.textContent).toContain("3f2a9c81b0d4");
    expect(items[1]!.textContent).toContain("abcdef1 (dirty)");
    expect(items[1]!.querySelector<HTMLAnchorElement>('a[href^="#/s/"]')!.textContent).toBe("Build the CLI");
    expect(items[2]!.textContent).toContain("the node never sent the file");
    // Copy puts the whole sha256 on the clipboard and says how to check it.
    await act(async () => button("Copy sha256", items[1]!)!.click());
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(version().sha256);
    expect(items[1]!.querySelector("[role=status]")!.textContent).toBe("Check a download with sha256sum myapp");
    artifactsApi.pin.mockResolvedValue(version({ pinned: true }));
    await act(async () => button("Pin", items[1]!)!.click());
    expect(artifactsApi.pin).toHaveBeenCalledWith(id(1), "v7", true);
    artifactsApi.removeVersion.mockResolvedValue({ ok: true });
    await act(async () => button("Delete", items[2]!)!.click());
    expect(artifactsApi.removeVersion).toHaveBeenCalledWith(id(1), "v6");
  });

  test("someone else's versions: download only", async () => {
    const a = artifact(1, { mine: false, visibility: "workspace", owner: { username: "bob" } });
    artifactsApi.list.mockResolvedValue(listing([a]));
    artifactsApi.get.mockResolvedValue(detail(a, [version({ session: { private: true } })]));
    await show({ me: meIn(TEAM) });
    await act(async () => button("2 versions")!.click());
    await flush();
    const li = host.querySelector(".artifact-version")!;
    expect(buttons(li).map((b) => b.textContent)).toEqual(["Copy sha256"]);
    expect(li.textContent).toContain("from a chat you can't open");
    expect(li.querySelector("a")!.textContent).toBe("Download");
  });
});

describe("menu", () => {
  test("share, rename and delete one's own; a manager may delete a colleague's shared one", async () => {
    artifactsApi.list.mockResolvedValue(listing([artifact(1), artifact(2, { mine: false, visibility: "workspace", owner: { username: "bob" } })]));
    artifactsApi.update.mockResolvedValue({});
    artifactsApi.remove.mockResolvedValue({ ok: true });
    const admin = meIn({ ...TEAM, role: "admin", permissions: ["settings", "members", "routines"] });
    await show({ me: admin });
    const [mine, theirs] = rows();
    expect(buttons(mine!.querySelector(".artifact-menu-items")!).map((b) => b.textContent)).toEqual(["Share with workspace", "Rename", "Delete"]);
    expect(buttons(theirs!.querySelector(".artifact-menu-items")!).map((b) => b.textContent)).toEqual(["Delete"]);
    await act(async () => button("Share with workspace", mine!)!.click());
    expect(artifactsApi.update).toHaveBeenCalledWith(id(1), { visibility: "workspace" });
    act(() => button("Rename", mine!)!.click());
    type(mine!.querySelector<HTMLInputElement>('input[aria-label="New name"]')!, "cli");
    await act(async () => button("Save", mine!)!.click());
    expect(artifactsApi.update).toHaveBeenCalledWith(id(1), { name: "cli" });
    await act(async () => button("Delete", theirs!)!.click());
    expect(artifactsApi.remove).toHaveBeenCalledWith(id(2));
  });

  test("Personal has no sharing; a name clash shows the server's reason", async () => {
    artifactsApi.list.mockResolvedValue(listing([artifact(1)]));
    artifactsApi.update.mockRejectedValue(new Error("An artifact named cli already exists here."));
    await show();
    const row = rows()[0]!;
    expect(buttons(row.querySelector(".artifact-menu-items")!).map((b) => b.textContent)).toEqual(["Rename", "Delete"]);
    act(() => button("Rename", row)!.click());
    type(row.querySelector<HTMLInputElement>('input[aria-label="New name"]')!, "cli");
    await act(async () => button("Save", row)!.click());
    expect(row.querySelector('[role="alert"]')!.textContent).toBe("An artifact named cli already exists here.");
  });
});

describe("detail", () => {
  test("notes, what the node reported, the files in a pack, every version", async () => {
    const latest = version({
      notes: "Release build for Linux.",
      packed: "tar.gz",
      format: "gzip",
      kind: "archive",
      meta: {
        toolchain: "rustc 1.93.0",
        package: { name: "myapp", version: "0.4.1" },
        git: { commit: "abcdef1234567", branch: "main", dirty: false },
        entries: [
          { path: "bin/myapp", size: 4096 },
          { path: "README.md", size: 120 },
        ],
        entries_total: 5,
        skipped: { symlinks: 2 },
        source_path: "dist",
      },
    });
    artifactsApi.get.mockResolvedValue(detail(artifact(1, { kind: "archive", latest }), [latest]));
    await show({ id: id(1) });
    expect(host.querySelector("h1")!.textContent).toBe("app1");
    expect(host.querySelector(".artifact-notes")!.textContent).toBe("Release build for Linux.");
    const facts = Object.fromEntries([...host.querySelectorAll(".artifact-fact")].map((f) => [f.querySelector("dt")!.textContent, f.querySelector("dd")!.textContent]));
    expect(facts).toMatchObject({
      Kind: "Archive, GZIP (folder, packed as tar.gz)",
      Toolchain: "rustc 1.93.0",
      Package: "myapp 0.4.1",
      Git: "abcdef123456 on main",
      From: "dist",
      "Left out": "2 symlinks",
    });
    expect(host.querySelector(".artifact-entries summary")!.textContent).toBe("5 files in the pack");
    expect(host.querySelector(".artifact-entries")!.textContent).toContain("and 3 more");
    expect(host.querySelectorAll(".artifact-version")).toHaveLength(1);
    expect(host.querySelector(".artifact-preview")).toBeNull();
  });

  test("previews: video plays; an image only when its Media item vouches for its size", async () => {
    const clip = version({ kind: "video", format: "mp4", display: "inline", meta: {} });
    artifactsApi.get.mockResolvedValue(detail(artifact(1, { kind: "video", latest: clip }), [clip]));
    await show({ id: id(1) });
    expect(host.querySelector("video")!.getAttribute("src")).toBe(linked(`/api/artifacts/${id(1)}/versions/v7/preview`));

    const png = version({ kind: "image", format: "png", display: "inline", meta: {} });
    const img = artifact(2, { kind: "image", latest: png, media_id: id(5) });
    artifactsApi.get.mockResolvedValue(detail(img, [png]));
    mediaApi.get.mockResolvedValue({ width: 30_000, height: 30_000, kind: "image", display: "inline", format: "png", artifact: { artifact_id: id(2), version_id: "v7", seq: 7, name: "x" } } as MediaItem);
    await show({ id: id(2) });
    expect(mediaApi.get).toHaveBeenCalledWith(id(5));
    expect(host.querySelector(".artifact-preview")).toBeNull();

    mediaApi.get.mockResolvedValue({ width: 800, height: 600, kind: "image", display: "inline", format: "png", artifact: { artifact_id: id(2), version_id: "v7", seq: 7, name: "x" } } as MediaItem);
    act(() => root.unmount());
    root = createRoot(host);
    await show({ id: id(2) });
    expect(host.querySelector("img.artifact-preview")!.getAttribute("src")).toBe(linked(`/api/artifacts/${id(2)}/versions/v7/preview`));
  });

  test("an image version's own header size decides, without asking Media", async () => {
    const png = version({ kind: "image", format: "png", display: "inline", meta: {}, width: 800, height: 600 });
    artifactsApi.get.mockResolvedValue(detail(artifact(2, { kind: "image", latest: png, media_id: id(5) }), [png]));
    await show({ id: id(2) });
    expect(host.querySelector("img.artifact-preview")!.getAttribute("src")).toBe(linked(`/api/artifacts/${id(2)}/versions/v7/preview`));
    // Over 40 MP: never decoded, whatever Media says.
    const bomb = version({ kind: "image", format: "png", display: "inline", meta: {}, width: 20_000, height: 20_000 });
    artifactsApi.get.mockResolvedValue(detail(artifact(3, { kind: "image", latest: bomb, media_id: id(6) }), [bomb]));
    act(() => root.unmount());
    root = createRoot(host);
    await show({ id: id(3) });
    expect(host.querySelector(".artifact-preview")).toBeNull();
    expect(mediaApi.get).not.toHaveBeenCalled();
  });

  test("a gone or private artifact says so", async () => {
    artifactsApi.get.mockRejectedValue(new HttpError(404, "not found"));
    await show({ id: id(9) });
    expect(host.textContent).toContain("This artifact is gone, or it isn't shared with you.");
  });
});
