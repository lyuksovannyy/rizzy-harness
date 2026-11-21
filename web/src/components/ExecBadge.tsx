// The chat header's word on how this chat's commands run (N.10a): its
// mode, from its log, and the node's gaps for that mode in plain words
// (review T8B-3; before, only the node's start-up log and `doctor` said them).
import { own } from "../emptyView";
import { MODE_LABEL, modeGaps } from "../lib/execWords";
import type { ExecMode, NodeInfo } from "../types";

/** `inline`: the chat header's suffix, where the gaps are the hover text
 * instead of a line of their own. */
export function ExecBadge({ mode, node, inline }: { mode: ExecMode | undefined; node?: NodeInfo; inline?: boolean }) {
  if (!mode) return null;
  const gaps = modeGaps(mode, node?.exec_gaps);
  const word = own(MODE_LABEL, mode) ?? "unknown mode";
  const hover = `commands run in the ${word}`;
  return (
    <>
      <span
        className={`badge exec-mode ${mode === "host" ? "badge-warn" : ""}`}
        title={inline && gaps.length > 0 ? `${hover} — ${gaps.join("; ")}` : hover}
      >
        {word}
      </span>
      {!inline && gaps.length > 0 && <span className="muted small exec-gaps">{gaps.join("; ")}</span>}
    </>
  );
}
