// "Preview" on a Word or Excel file of a chat without a folder: opens the
// side panel (components/DocPreview.tsx). Offered only for a name the server
// reads and an id it has.
import { Eye } from "lucide-react";
import { openPreview, previewable } from "../lib/docPreview";

export function PreviewButton({ sessionId, fileId, name }: { sessionId: string; fileId: string | undefined; name: string }) {
  if (!fileId || !previewable(name)) return null;
  return (
    <button
      type="button"
      className="link small"
      aria-label={`Preview ${name}`}
      onClick={(e) => openPreview({ session: sessionId, fileId, name }, e.currentTarget)}
    >
      <Eye size={14} aria-hidden="true" /> Preview
    </button>
  );
}
