// The join question (owner's decision 2026-11-14): in a team workspace the
// person has not answered for, may its agents use their own facts? Until
// they answer, the answer is no (the server recalls nothing and saves
// nothing there). Asked in the workspace itself, on every page, until
// answered; the answer can be changed later in Settings → Workspace.
// Lazy: Shell loads it only while a question is open.
import { BookmarkPlus } from "lucide-react";
import { useState } from "react";
import { pageApi } from "../apiPages";
import type { WorkspaceSummary } from "../types";

export default function MemoryShareAsk(props: { workspace: WorkspaceSummary; onAnswered: () => void }) {
  const { workspace: ws } = props;
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const answer = async (on: boolean) => {
    setBusy(true);
    setErr(null);
    try {
      await pageApi.setWorkspaceMemory(ws.workspace_id, on);
      props.onAnswered();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
      setBusy(false);
    }
  };
  return (
    <section className="memory-ask" role="region" aria-label={`Your memory in ${ws.name}`}>
      <BookmarkPlus size={16} aria-hidden="true" className="memory-icon" />
      <p>
        <strong>Use your memory in {ws.name}?</strong>{" "}
        <span className="muted">
          Yes lets the agents in your chats here read your own facts and suggest new ones. Private details stay in
          your Personal workspace either way, and nobody else here ever sees your memory. Until you answer, it stays
          out.
        </span>
      </p>
      <button type="button" disabled={busy} onClick={() => void answer(false)}>
        Keep it out
      </button>
      <button type="button" disabled={busy} onClick={() => void answer(true)}>
        Use my facts here
      </button>
      {err && <div className="error-text small">{err}</div>}
    </section>
  );
}
