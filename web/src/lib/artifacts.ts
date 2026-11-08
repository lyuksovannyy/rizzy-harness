// Artifacts in the browser (docs/artifacts.md): the wire types, the API
// calls, the download addresses, and the few words the page and the chat
// cards say without asking. Kept out of api.ts and types.ts so this code
// loads with the Artifacts page or a chat, never with the first screen.
// Artifacts are stored files: nothing here renders or runs one.
import { call } from "../api";
import type { ArtifactMeta } from "../types";

export type ArtifactStatus = "pending" | "ready" | "failed";

export interface ArtifactVersion {
  version_id: string;
  seq: number;
  status: ArtifactStatus;
  error: string | null;
  /** Bytes copied so far; only while pending (held in the server's memory). */
  progress: { received: number; total: number } | null;
  size: number;
  sha256: string;
  filename: string;
  packed: "tar.gz" | null;
  kind: string | null;
  format: string | null;
  display: "inline" | "img" | "download" | null;
  /** An image's header size, read by the server (null when unknown; absent from older servers). */
  width?: number | null;
  height?: number | null;
  /** What the node reported: display only. */
  meta: ArtifactMeta;
  notes: string;
  node: { node_id: string; hostname: string };
  /** The title only for the chat's owner. */
  session: { session_id: string; title: string } | { private: true } | null;
  by_user: boolean;
  pinned: boolean;
  has_thumb: boolean;
  created_ms: number;
}

export interface Artifact {
  artifact_id: string;
  name: string;
  kind: string | null;
  visibility: "private" | "workspace";
  mine: boolean;
  owner: { username: string } | null;
  project_id: string | null;
  versions: number;
  /** The newest ready version, else the newest of any status. */
  latest: ArtifactVersion | null;
  /** The lineage's Media item (images, video, audio). */
  media_id: string | null;
  updated_ms: number;
}

/** `GET /api/artifacts/{id}`: the lineage with every version, newest first. */
export type ArtifactWithVersions = Omit<Artifact, "versions"> & { versions: ArtifactVersion[] };

export interface ArtifactPage {
  artifacts: Artifact[];
  next_cursor: string | null;
  usage: { used: number; quota: number };
}

/** A file this chat staged, as its publish card and saved-file items show it. */
export interface SessionArtifact {
  call_id: string | null;
  stage_id: string;
  version_id: string | null;
  artifact_id: string | null;
  status: ArtifactStatus;
  error: string | null;
  progress: { received: number; total: number } | null;
  by_user: boolean;
  path?: string;
}

export type KindFilter = "builds" | "archives" | "documents" | "media" | "other";

export const KIND_FILTERS: [KindFilter | "all", string][] = [
  ["all", "All"],
  ["builds", "Builds"],
  ["archives", "Archives"],
  ["documents", "Documents"],
  ["media", "Media"],
  ["other", "Other"],
];

export interface ArtifactQuery {
  workspace: string | null;
  /** Only what this project's chats published. */
  project?: string | null;
  kind?: KindFilter | null;
  mine?: boolean;
  q?: string;
  cursor?: string | null;
}

function query(q: ArtifactQuery): string {
  const p = new URLSearchParams();
  if (q.workspace) p.set("workspace", q.workspace);
  if (q.project) p.set("project", q.project);
  if (q.kind) p.set("kind", q.kind);
  if (q.mine) p.set("mine", "1");
  if (q.q?.trim()) p.set("q", q.q.trim());
  if (q.cursor) p.set("cursor", q.cursor);
  p.set("limit", "50");
  return p.toString();
}

const one = (id: string) => `/api/artifacts/${encodeURIComponent(id)}`;
const version = (id: string, vid: string) => `${one(id)}/versions/${encodeURIComponent(vid)}`;
const chat = (sid: string) => `/api/sessions/${encodeURIComponent(sid)}/artifacts`;

export const artifactsApi = {
  list: (q: ArtifactQuery) => call<ArtifactPage>("GET", `/api/artifacts?${query(q)}`),
  get: (id: string) => call<ArtifactWithVersions>("GET", one(id)),
  update: (id: string, b: { name?: string; visibility?: Artifact["visibility"] }) => call<unknown>("PATCH", one(id), b),
  remove: (id: string) => call<{ ok: boolean }>("DELETE", one(id)),
  pin: (id: string, vid: string, pinned: boolean) => call<ArtifactVersion>("PATCH", version(id, vid), { pinned }),
  removeVersion: (id: string, vid: string) => call<{ ok: boolean }>("DELETE", version(id, vid)),
  /** What this chat staged: feeds the publish cards and the saved-file items. */
  forSession: (sid: string) => call<SessionArtifact[]>("GET", chat(sid)),
  /** "Save as artifact": the node copies the file, then the chat shows it. */
  save: (sid: string, path: string, name?: string) =>
    call<{ requested: boolean }>("POST", chat(sid), { path, ...(name?.trim() ? { name: name.trim() } : {}) }),
};

export const latestUrl = (id: string) => `${one(id)}/latest/download`;
export const downloadUrl = (id: string, vid: string) => `${version(id, vid)}/download`;
export const previewUrl = (id: string, vid: string) => `${version(id, vid)}/preview`;
export const versionThumbUrl = (id: string, vid: string) => `${version(id, vid)}/thumb`;

const KIND: Record<string, string> = {
  binary: "Program",
  package: "Package",
  archive: "Archive",
  document: "Document",
  image: "Image",
  video: "Video",
  audio: "Audio",
};

/** "Program", "Archive", …; anything else is a "File". */
export const kindLabel = (kind: string | null) => (kind && KIND[kind]) || "File";

/** The chip beside a name: the build target when the node knew it, else the format. */
export function chipOf(v: Pick<ArtifactVersion, "meta" | "format" | "packed">): string | null {
  return v.meta.target ?? (v.packed ? "tar.gz" : v.format ? v.format.toUpperCase() : null);
}

/** Enough of a sha256 to compare by eye; the full one is copied. */
export const shortSha = (sha: string) => sha.slice(0, 12);

/** "45%" of a transfer, or null before the size is known. */
export function percent(p: ArtifactVersion["progress"]): string | null {
  return p && p.total > 0 ? `${Math.min(100, Math.floor((p.received / p.total) * 100))}%` : null;
}

/** "from ‘Build the CLI’" for the owner, "by alex" for a colleague's shared file. */
export function sourceLine(a: Pick<Artifact, "mine" | "owner" | "latest">): string | null {
  const s = a.latest?.session;
  if (a.mine) return s && "title" in s ? `from ‘${s.title}’` : null;
  return a.owner ? `by ${a.owner.username}` : null;
}

/** Whether a version has a preview the browser can play or show. */
export const previewable = (v: Pick<ArtifactVersion, "status" | "display" | "kind">) =>
  v.status === "ready" && (v.display === "inline" || v.display === "img") && ["image", "video", "audio"].includes(v.kind ?? "");

/** "3 symlinks, 1 secret-like name": what a folder pack left out. */
export function skippedLine(skipped: Record<string, number> | undefined): string | null {
  const parts = Object.entries(skipped ?? {})
    .filter(([, n]) => n > 0)
    .map(([what, n]) => `${n} ${what.replace(/_/g, " ")}`);
  return parts.length ? parts.join(", ") : null;
}
