// "Open" on a path the page knows exactly (a file card's own input, a changed
// file of a diff, a made file's path): opens the file's text beside the chat
// (components/FolderFile.tsx); a Word or Excel file goes to its Preview
// instead. Never made from text the model wrote in prose: only from fields
// that are paths.
import { PanelRightOpen } from "lucide-react";
import { openPreview, previewable } from "../lib/docPreview";

/** The path as the node takes it: no control character, at most 1,024 bytes (the server refuses the rest). */
const askable = (p: string) => p !== "" && new TextEncoder().encode(p).length <= 1024 && !/\p{Cc}/u.test(p);

export function FileButton({ sessionId, path, cwd }: { sessionId: string; path: string; /** The chat's folder: a path outside it is not offered. */ cwd?: string }) {
  if (!askable(path)) return null;
  if (cwd && path.startsWith("/") && path !== cwd && !path.startsWith(cwd.replace(/\/$/, "") + "/")) return null;
  const name = path.split("/").pop() || path;
  const open = (e: React.MouseEvent<HTMLButtonElement>) => openPreview(previewable(name) ? { session: sessionId, path, name } : { session: sessionId, path, name, text: true }, e.currentTarget);
  return (
    <button type="button" className="link small" aria-label={`Open ${name}`} onClick={open}>
      <PanelRightOpen size={14} aria-hidden="true" /> Open
    </button>
  );
}
