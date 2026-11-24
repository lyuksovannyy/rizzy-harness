// A text file of a coding chat's folder in the side panel (X.63): what the
// panel is asked for and what the server answers. The panel is a lazy chunk
// (components/FolderFile.tsx); it shares the one side-panel slot with the
// Word and Excel preview (lib/docPreview.ts), so opening one replaces the other
// and the unsaved-changes question is asked once, in one place.

/** What `GET /api/sessions/{id}/folder-file` answers. */
export interface FileAnswer {
  /** The file's name, as the page asked for it. */
  name: string;
  /** At most 256 KiB, scrubbed of the node's secrets. */
  text: string;
  /** The file's size in bytes when it was read. */
  size: number;
  /** The file is longer than `text`. */
  truncated?: boolean;
  /** Bytes that are not UTF-8 are shown as U+FFFD. */
  lossy?: boolean;
}

/** The route for `path` (any text: it is encoded). */
export const fileUrl = (session: string, path: string) => `/api/sessions/${encodeURIComponent(session)}/folder-file?path=${encodeURIComponent(path)}`;

/** The extension (or the whole name of a Dockerfile) that picks a grammar, lower case. */
export function langOf(name: string): string {
  const base = name.toLowerCase();
  return base === "dockerfile" ? "docker" : (base.split(".").pop() ?? "");
}
