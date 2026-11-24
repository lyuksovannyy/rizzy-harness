// "All changes" in a coding chat's header (X.61): opens the changes panel with
// everything the chat changed in its folder, from the start (or the oldest
// checkpoint the node still keeps) to now. Shown once some turn changed files;
// the panel is DiffPanel, the same one a turn's "Show changes" opens.
import { FileDiff } from "lucide-react";
import type { CheckpointState } from "../types";

/** A turn of this chat has changed files (a fork's inherited ones count: the range holds them). */
export function hasChanges(checkpoints: Record<string, CheckpointState> | undefined): boolean {
  return Object.values(checkpoints ?? {}).some((c) => !!c.after && c.files.length > 0);
}

export function AllChangesButton(props: { checkpoints: Record<string, CheckpointState> | undefined; onOpen: (from: HTMLElement) => void }) {
  if (!hasChanges(props.checkpoints)) return null;
  return (
    <button
      type="button"
      className="icon"
      aria-label="All changes"
      title="All changes in this chat"
      onClick={(e) => props.onOpen(e.currentTarget)}
    >
      <FileDiff size={18} aria-hidden />
    </button>
  );
}
