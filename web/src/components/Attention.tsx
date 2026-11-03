// Chats waiting for the user (approvals, questions), shown under the bar
// until there are search results to show instead.
import type { AttentionItem, NodeAttention } from "../types";

export function Attention(props: { items: AttentionItem[]; nodes?: NodeAttention[]; onOpen: (id: string) => void }) {
  const nodes = props.nodes ?? [];
  if (props.items.length === 0 && nodes.length === 0) return null;
  return (
    <div className="similar attention" aria-label="chats waiting for you">
      <div className="similar-label">Waiting for you</div>
      <ul>
        {nodes.map((n) => (
          <li key={`node-${n.setup_id}`}>
            <button type="button" onClick={() => (location.hash = "#/settings/nodes")}>
              <span className="similar-title ellipsis">Server {n.host}</span>
              <span className="similar-snippet ellipsis">{n.problems.map((p) => p.detail).join("; ")}</span>
            </button>
          </li>
        ))}
        {props.items.map((a) => (
          <li key={a.session_id}>
            <button type="button" onClick={() => props.onOpen(a.session_id)}>
              <span className="similar-title ellipsis">{a.title}</span>
              <span className="similar-snippet ellipsis">
                {a.reason === "approval" ? "Needs your approval" : "Asked you a question"}
                {a.parent_id ? " (sub-task)" : ""}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
