import type { NodeInfo } from "../types";
import { shownName } from "../md/hiddenChars";

/** The node's name as its badge shows it. The hostname is the node's own
 * word: hidden characters show (W-3). */
export const nodeLabel = (node: NodeInfo | undefined, nodeId: string) => shownName(node ? (node.embedded ? "local" : node.hostname) : nodeId);

export function NodeBadge({ node, nodeId }: { node?: NodeInfo; nodeId: string }) {
  const online = node?.connected ?? false;
  const label = nodeLabel(node, nodeId);
  return (
    <span
      className={`badge ${online ? "" : "badge-offline"}`}
      title={`node ${nodeId}${node ? ` (${shownName(node.hostname)})` : ""} — ${online ? "online" : "offline"}`}
    >
      {label}
    </span>
  );
}
