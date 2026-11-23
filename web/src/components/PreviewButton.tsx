// "Preview" on a Word or Excel file: opens the side panel
// (components/DocPreview.tsx). Offered only for a name the server reads and
// something to ask for: the id of the server's copy (a chat without a folder)
// or the file's path in the chat's folder (a coding chat).
import { Eye } from "lucide-react";
import { openPreview, type PreviewTarget, previewable } from "../lib/docPreview";

export function PreviewButton({ sessionId, fileId, path, name }: { sessionId: string; fileId?: string; path?: string; name: string }) {
  if (!previewable(name)) return null;
  const target: PreviewTarget | null = fileId ? { session: sessionId, fileId, name } : path ? { session: sessionId, path, name } : null;
  if (!target) return null;
  return (
    <button
      type="button"
      className="link small"
      aria-label={`Preview ${name}`}
      onClick={(e) => openPreview(target, e.currentTarget)}
    >
      <Eye size={14} aria-hidden="true" /> Preview
    </button>
  );
}
