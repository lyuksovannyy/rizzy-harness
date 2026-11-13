// The chat header's word on how this chat's commands run (N.10a): its
// mode, from its log, and the node's gaps for that mode in plain words
// (review T8B-3; before, only the node's start-up log and `doctor` said them).
import { MODE_LABEL, modeGaps } from "../lib/execWords";
import type { ExecMode, NodeInfo } from "../types";

export function ExecBadge({ mode, node }: { mode: ExecMode | undefined; node?: NodeInfo }) {
  if (!mode) return null;
  const gaps = modeGaps(mode, node?.exec_gaps);
  return (
    <>
      <span className={`badge exec-mode ${mode === "host" ? "badge-warn" : ""}`} title={`commands run in the ${MODE_LABEL[mode]}`}>
        {MODE_LABEL[mode]}
      </span>
      {gaps.length > 0 && <span className="muted small exec-gaps">{gaps.join("; ")}</span>}
    </>
  );
}
