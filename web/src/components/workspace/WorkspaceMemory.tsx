// Settings → Workspace → Memory: the workspace's memory, one per workspace
// (the owner, 2026-11-14: "Workspace memory available to all users in it").
// Every member reads it, and every member's chats here are given it. Its
// admins (or every member, behind the switch) add, edit, retire, restore and
// delete facts, and keep or dismiss what agents propose; a member sees their
// own chats' proposals waiting. In a Personal workspace it is the person's
// private details, theirs alone.
import { useCallback, useEffect, useState } from "react";
import { pageApi } from "../../apiPages";
import type { MemoryFact, WorkspaceFacts } from "../../types";
import { message, type WorkspacePanelProps } from "./panel";

export function WorkspaceMemory(props: WorkspacePanelProps & { version: number }) {
  const ws = props.workspace;
  const id = ws.workspace_id;
  const [data, setData] = useState<WorkspaceFacts | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const load = useCallback(() => {
    pageApi
      .workspaceFacts(id)
      .then(setData)
      .catch((x) => setErr(message(x)));
  }, [id]);
  useEffect(load, [load, props.version]);
  const act = (p: Promise<unknown>) => {
    setErr(null);
    p.then(load).catch((x) => setErr(message(x)));
  };
  if (!data) return err ? <p className="error-text">{err}</p> : <p className="muted">Loading…</p>;
  const edit = data.can_edit;
  const byText = (fid: string | null | undefined) => data.memories.find((f) => f.id === fid)?.text;
  const waiting = data.memories.filter((f) => f.status === "proposed");
  const active = data.memories.filter((f) => f.status === "active");
  const retired = data.memories.filter((f) => f.status === "retired");
  const day = (ms?: number | null) => (ms ? new Date(ms).toLocaleDateString() : "");
  // Who did what to a shared fact (review F4): every member sees it.
  const credit = (f: MemoryFact) => {
    const parts = [
      f.added_by && `added by ${f.added_by}${f.added_ms ? `, ${day(f.added_ms)}` : ""}`,
      f.kept_by && `kept by ${f.kept_by}`,
      f.retired_by && `retired by ${f.retired_by}${f.retired_ms ? `, ${day(f.retired_ms)}` : ""}`,
    ].filter(Boolean);
    return parts.length ? <span className="muted small fact-credit">{parts.join(" · ")}</span> : null;
  };
  const what = (f: MemoryFact) => {
    const old = byText(f.proposal?.replaces);
    if (f.proposal?.drop) return `Forget: “${f.text}”`;
    if (old) return `Replace “${old}” with “${f.text}”`;
    return f.text;
  };
  return (
    <div className="stack">
      <p className="field-hint">
        {ws.personal
          ? "Private details about you, kept in your Personal workspace: given to your chats here and nowhere else."
          : "What this workspace has established: given to every member's chats here, and visible to every member. Facts about how the workspace works, never anything private about a person (that stays in each person's own memory)."}
      </p>
      {waiting.length > 0 && (
        <div className="card stack">
          <h3>Waiting ({waiting.length})</h3>
          <ul className="facts">
            {waiting.map((f) => (
              <li key={f.id} className="fact proposed">
                <div className="fact-text">{what(f)}</div>
                <div className="row fact-meta small">
                  {f.mine && (
                    <span className="muted">
                      {f.proposal?.submitted ? "From your chat, sent on" : "From your chat, only you see it: Keep it in the chat to send it on"}
                    </span>
                  )}
                  {!f.mine && f.proposed_by && <span className="muted">Proposed by {f.proposed_by}</span>}
                  {edit && !f.mine && (
                    <button className="link" onClick={() => act(pageApi.updateWorkspaceFact(id, f.id, { status: "active" }))}>
                      Keep
                    </button>
                  )}
                  <button className="link" onClick={() => act(pageApi.deleteWorkspaceFact(id, f.id))}>
                    Dismiss
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="card stack">
        <h3>Remembered</h3>
        {active.length === 0 && <p className="muted">Nothing yet.</p>}
        <ul className="facts">
          {active.map((f) =>
            editing?.id === f.id ? (
              <li key={f.id} className="fact">
                <form
                  className="row"
                  onSubmit={(e) => {
                    e.preventDefault();
                    act(pageApi.updateWorkspaceFact(id, f.id, { text: editing.text }).then(() => setEditing(null)));
                  }}
                >
                  <input className="grow" autoFocus value={editing.text} aria-label="Fact" onChange={(e) => setEditing({ id: f.id, text: e.target.value })} />
                  <button className="primary">Save</button>
                  <button type="button" onClick={() => setEditing(null)}>
                    Cancel
                  </button>
                </form>
              </li>
            ) : (
              <li key={f.id} className="fact">
                <div className="fact-text">{f.text}</div>
                {credit(f)}
                {edit && (
                  <div className="row fact-meta small">
                    <button className="link" onClick={() => setEditing({ id: f.id, text: f.text })}>
                      Edit
                    </button>
                    <button className="link" onClick={() => act(pageApi.updateWorkspaceFact(id, f.id, { status: "retired" }))}>
                      Retire
                    </button>
                  </div>
                )}
              </li>
            ),
          )}
        </ul>
        {edit && (
          <form
            className="row"
            onSubmit={(e) => {
              e.preventDefault();
              act(pageApi.addWorkspaceFact(id, text).then(() => setText("")));
            }}
          >
            <input className="grow" value={text} placeholder="Add something this workspace should remember" aria-label="New fact" onChange={(e) => setText(e.target.value)} />
            <button disabled={!text.trim()}>Add</button>
          </form>
        )}
      </div>
      {retired.length > 0 && (
        <div className="card stack">
          <h3>Replaced or forgotten ({retired.length})</h3>
          <ul className="facts">
            {retired.map((f) => (
              <li key={f.id} className="fact retired">
                <div className="fact-text">{f.text}</div>
                {credit(f)}
                {edit && (
                  <div className="row fact-meta small">
                    <button className="link" onClick={() => act(pageApi.updateWorkspaceFact(id, f.id, { status: "active" }))}>
                      Restore
                    </button>
                    {/* Only admins and owners delete, and only what is retired (review F5). */}
                    {data.can_set && (
                      <button className="link" onClick={() => act(pageApi.deleteWorkspaceFact(id, f.id))}>
                        Delete
                      </button>
                    )}
                  </div>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
      {!ws.personal && data.can_set && (
        <div className="card stack">
          <label className="check">
            <input
              type="checkbox"
              checked={data.members_edit}
              onChange={(e) => act(pageApi.setWorkspaceFactsSettings(id, e.target.checked))}
            />
            Every member may change this memory
          </label>
          <span className="field-hint">
            Off: admins and owners change it and keep what members' agents propose. On: every member can, and their
            agents' saves here apply at once (each shown in their chat with Undo).
          </span>
        </div>
      )}
      {err && <p className="error-text">{err}</p>}
    </div>
  );
}
