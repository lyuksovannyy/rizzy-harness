// Chats waiting for the user (approvals, questions), shown under the bar
// until there are search results to show instead.
import type { AttentionItem } from "../types";

export function Attention(props: { items: AttentionItem[]; onOpen: (id: string) => void }) {
  if (props.items.length === 0) return null;
  return (
    <div className="similar attention" aria-label="chats waiting for you">
      <div className="similar-label">Waiting for you</div>
      <ul>
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
