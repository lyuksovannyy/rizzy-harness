import type { NodeInfo } from "../types";
import { shownName } from "../md/hiddenChars";

export function NodeBadge({ node, nodeId }: { node?: NodeInfo; nodeId: string }) {
  const online = node?.connected ?? false;
  // The hostname is the node's own word: hidden characters show (W-3).
  const label = shownName(node ? (node.embedded ? "local" : node.hostname) : nodeId);
  return (
    <span
      className={`badge ${online ? "" : "badge-offline"}`}
      title={`node ${nodeId}${node ? ` (${shownName(node.hostname)})` : ""} — ${online ? "online" : "offline"}`}
    >
      {label}
    </span>
  );
}
