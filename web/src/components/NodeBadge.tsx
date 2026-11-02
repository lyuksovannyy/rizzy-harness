import type { NodeInfo } from "../types";

export function NodeBadge({ node, nodeId }: { node?: NodeInfo; nodeId: string }) {
  const online = node?.connected ?? false;
  const label = node ? (node.embedded ? "local" : node.hostname) : nodeId;
  return (
    <span
      className={`badge ${online ? "" : "badge-offline"}`}
      title={`node ${nodeId}${node ? ` (${node.hostname})` : ""} — ${online ? "online" : "offline"}`}
    >
      {label}
    </span>
  );
}
