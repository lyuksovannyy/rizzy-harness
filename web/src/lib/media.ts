// Media in the browser (docs/media.md): the wire types, the API calls, the
// streamed upload with progress, and what the grid and the preview decide
// without asking (labels, sizes, whether an image is safe to decode). Kept
// out of api.ts and types.ts so the page's code stays in its lazy chunk; the
// Storage settings tab shares the size and storage calls.
import { call, HttpError, keyed, seen } from "../api";
import { artifactsApi } from "./artifacts";
import type { MediaFilter } from "./routes";

export type MediaKind = "image" | "video" | "audio" | "other";
export type Display = "inline" | "img" | "download";

export interface MediaItem {
  media_id: string;
  name: string;
  kind: MediaKind;
  format: string;
  mime: string;
  size: number;
  width: number | null;
  height: number | null;
  duration_ms: number | null;
  display: Display;
  has_thumb: boolean;
  no_thumb: boolean;
  origin: "upload" | "chat" | "agent";
  /** Only the owner's own chat; "deleted" once it is gone. */
  source: { session_id: string; title: string } | "deleted" | null;
  artifact: { artifact_id: string; version_id: string; seq: number; name: string } | null;
  /** Chats still carrying it (images in chats). */
  chats: number;
  workspace_id: string | null;
  created_ms: number;
  updated_ms: number;
  /** On an upload's answer: these bytes were already here. */
  duplicate?: boolean;
}

export interface MediaPage {
  items: MediaItem[];
  next: string | null;
  usage: { used: number; quota: number };
  limits: { max_file: number };
}

export interface MyStorage {
  used: number;
  /** 0: unlimited. */
  quota: number;
  media: { bytes: number; items: number };
  artifacts: { bytes: number; versions: number };
}

export interface StorageLimits {
  max_file_mb: number;
  account_quota_mb: number;
  server_quota_mb: number;
  min_free_mb: number;
}

export interface AdminStorage {
  settings: StorageLimits;
  usage: { user_id: string; username: string; used: number; media_items: number; artifact_versions: number }[];
  total: number;
  /** null when the server cannot measure its data disk. */
  disk: { free: number | null; total: number | null };
}

export interface MediaQuery {
  workspace: string | null;
  kind?: MediaKind | null;
  origin?: MediaItem["origin"] | null;
  q?: string;
  /** Only the items that came from this chat (the chat-delete dialog). */
  session?: string | null;
  before?: string | null;
}

/** The hints the owner's browser may set; the server clamps them. */
export type MediaPatch = Partial<Pick<MediaItem, "name" | "width" | "height" | "duration_ms" | "no_thumb">>;

const item = (id: string) => `/api/media/${encodeURIComponent(id)}`;

function query(q: MediaQuery): string {
  const p = new URLSearchParams();
  if (q.workspace) p.set("workspace", q.workspace);
  if (q.kind) p.set("kind", q.kind);
  if (q.origin) p.set("origin", q.origin);
  if (q.q?.trim()) p.set("q", q.q.trim());
  if (q.session) p.set("session", q.session);
  if (q.before) p.set("before", q.before);
  p.set("limit", "60");
  return p.toString();
}

export const mediaApi = {
  list: (q: MediaQuery) => call<MediaPage>("GET", `/api/media?${query(q)}`),
  get: (id: string) => call<MediaItem>("GET", item(id)),
  update: (id: string, b: MediaPatch) => call<MediaItem>("PATCH", item(id), b),
  remove: (ids: string[]) =>
    ids.length === 1 ? call<{ ok: boolean }>("DELETE", item(ids[0]!)) : call<{ ok: boolean }>("POST", "/api/media/delete", { ids }),
  putThumb: async (id: string, thumb: Blob) => {
    const res = await fetch(`${item(id)}/thumb`, {
      method: "PUT",
      credentials: "same-origin",
      headers: keyed({ "content-type": thumb.type }),
      body: thumb,
    });
    seen(res.status, null);
    if (!res.ok) throw new HttpError(res.status, `${res.status} ${res.statusText}`);
  },
};

/** "myapp v3": the artifact version behind an agent's item. */
export const artifactName = (a: NonNullable<MediaItem["artifact"]>) => `${a.name} v${a.seq}`;

/** Deletes items (100 per call, the server's limit), then, for "Delete from
 * both", the artifact versions behind the agent's ones. The items go first;
 * versions that could not go are named in the answer, else it is null. */
export async function deleteMedia(items: MediaItem[], both: boolean): Promise<string | null> {
  const ids = items.map((m) => m.media_id);
  for (let i = 0; i < ids.length; i += 100) await mediaApi.remove(ids.slice(i, i + 100));
  if (!both) return null;
  const stayed: string[] = [];
  for (const m of items) {
    if (!m.artifact) continue;
    await artifactsApi.removeVersion(m.artifact.artifact_id, m.artifact.version_id).catch(() => stayed.push(artifactName(m.artifact!)));
  }
  return stayed.length ? `Deleted from Media, but these stay in Artifacts: ${stayed.join(", ")}.` : null;
}

export const storageApi = {
  mine: () => call<MyStorage>("GET", "/api/me/storage"),
  admin: () => call<AdminStorage>("GET", "/api/admin/storage"),
  /** The limits in, the limits as saved back (§5.6: `settings` → `settings`). */
  save: (settings: StorageLimits) => call<StorageLimits>("PUT", "/api/admin/storage", settings),
};

/** Where an item's bytes are; `download` asks for an attachment. */
export const contentUrl = (id: string, download = false) => `${item(id)}/content${download ? "?download=1" : ""}`;
export const thumbUrl = (id: string) => `${item(id)}/thumb`;

/** The kind a grid filter shows. */
export const FILTER_KIND: Record<MediaFilter, MediaKind> = { images: "image", videos: "video", audio: "audio", other: "other" };

export const KIND_LABEL: Record<MediaKind, string> = { image: "Image", video: "Video", audio: "Audio", other: "File" };

/** 83_000 → "1:23"; an hour or more → "1:02:03". */
export function clockTime(ms: number): string {
  const s = Math.round(ms / 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  const h = Math.floor(s / 3600);
  return h ? `${h}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}` : `${Math.floor(s / 60)}:${pad(s % 60)}`;
}

/** Images the browser must never decode: header dimensions over 40 MP or
 * 16,384 px on a side (a decompression bomb would take the tab down). */
export const MAX_PIXELS = 40_000_000;
export const MAX_SIDE = 16_384;

/** Whether an image may be decoded here (a thumbnail, the preview's <img>).
 * The master reads dimensions from the file's header; a raster image whose
 * header it could not read is not decoded either, since nothing bounds it.
 * SVG is drawn at the size asked for, so it needs none. */
export function safeToDecode(m: Pick<MediaItem, "kind" | "display" | "format" | "width" | "height">): boolean {
  if (m.kind !== "image" || m.display === "download") return false;
  if (m.format === "svg") return true;
  const { width: w, height: h } = m;
  return !!w && !!h && w * h <= MAX_PIXELS && Math.max(w, h) <= MAX_SIDE;
}

/** The most one message may carry (docs/media.md §6.6, checked again by the server). */
export const CHAT_BYTES = 5 * 1024 * 1024;
/** "Use in chat" leaves item ids under this sessionStorage key for the home bar (lib/downscale). */
export const PENDING_IMAGES = "harness.pendingImages";

/** Whether "Use in chat" can offer an item: an image the browser shows as
 * it is, small enough for one message. */
export const chatReady = (m: MediaItem) => m.display === "inline" && safeToDecode(m) && m.size <= CHAT_BYTES;

/** Deleting from Media cannot reach the chats an image went into, nor the provider. */
export const stillIn = (chats: number) =>
  `Still in ${chats === 1 ? "1 chat" : `${chats} chats`}. Delete those chats to remove it there. Copies already sent to the model provider can't be recalled.`;

/** "From chat ‘Trip notes’", "Uploaded", "Published by an agent". */
export function originLine(m: Pick<MediaItem, "origin" | "source">): string {
  if (m.origin === "agent") return "Published by an agent";
  if (m.origin === "chat") return m.source && m.source !== "deleted" ? `From chat ‘${m.source.title}’` : "From a deleted chat";
  return "Uploaded";
}

export interface Uploaded {
  item: MediaItem;
  duplicate: boolean;
}

/** One file, streamed as the raw body with its name in a header (never in
 * the URL); `onProgress` gets the share sent, 0 to 1. XHR because fetch has
 * no upload progress. A busy server (429) is retried after its Retry-After.
 * `chat`: an image for a message, which the composer made small first. */
export function upload(
  file: File,
  workspace: string | null,
  onProgress: (share: number) => void,
  origin: "upload" | "chat" = "upload",
  tries = 3,
): Promise<Uploaded> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    const q = new URLSearchParams({ origin });
    if (workspace) q.set("workspace", workspace);
    xhr.open("POST", `/api/media?${q}`);
    xhr.withCredentials = true;
    xhr.setRequestHeader("content-type", file.type || "application/octet-stream");
    xhr.setRequestHeader("x-file-name", encodeURIComponent(file.name));
    for (const [k, v] of Object.entries(keyed())) xhr.setRequestHeader(k, v);
    xhr.upload.onprogress = (e) => e.lengthComputable && onProgress(e.loaded / e.total);
    xhr.onerror = () => reject(new HttpError(0, "The upload was cut off."));
    xhr.onload = () => {
      let data: (MediaItem & { error?: string }) | null = null;
      try {
        data = JSON.parse(xhr.responseText);
      } catch {
        /* not JSON */
      }
      seen(xhr.status, data);
      if (xhr.status === 429 && tries > 1) {
        const wait = Number(xhr.getResponseHeader("retry-after")) || 5;
        setTimeout(() => upload(file, workspace, onProgress, origin, tries - 1).then(resolve, reject), wait * 1000);
        return;
      }
      if (xhr.status < 200 || xhr.status >= 300 || !data) {
        reject(new HttpError(xhr.status, data?.error ?? `${xhr.status} ${xhr.statusText}`));
        return;
      }
      resolve({ item: data, duplicate: !!data.duplicate });
    };
    xhr.send(file);
  });
}
