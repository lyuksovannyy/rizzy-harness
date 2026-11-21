// Workspace settings → Agents (X.33, docs/agent-pages.md): the agents chats
// in this workspace use, read in its scope (so each one's model line is the
// workspace's), each marked built-in, server or workspace. Whoever holds the
// workspace's `settings` permission (its owners and admins; everyone in
// their own Personal workspace; the server answers `may_edit`) makes the
// workspace's own agents here, edits them whole, and adds this workspace's
// summary and additional instructions to the server's agents, after the
// server's own. The page opens in place (AgentPage, in this workspace's
// scope). A lazy chunk.
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type Scope } from "../../api";
import { mayEditAgents, NEW_AGENT } from "../../lib/agents";
import type { Agent, NewAgent } from "../../types";
import { AgentPage, confirmDelete } from "../AgentPage";
import { AgentList } from "../Agents";
import { message, type WorkspacePanelProps } from "./panel";

export default function WorkspaceAgents(props: WorkspacePanelProps) {
  const ws = props.workspace.workspace_id;
  const scope = useMemo<Scope>(() => ({ workspace: ws }), [ws]);
  const [agents, setAgents] = useState<Agent[] | null>(null);
  const [mayEdit, setMayEdit] = useState(false);
  // Its own agents a server agent with the same handle hides here.
  const [hidden, setHidden] = useState<{ id: string; name: string }[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [seed, setSeed] = useState<NewAgent | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(
    () =>
      api.agents(scope).then(
        (r) => {
          setAgents(r.agents);
          setMayEdit(!!r.may_edit);
          setHidden(r.hidden ?? []);
        },
        (x) => setErr(message(x)),
      ),
    [scope],
  );
  useEffect(() => void load(), [load]);
  useEffect(() => setOpen(null), [ws]);
  const changed = () => {
    void load();
    props.onChanged();
  };
  if (open !== null && agents) {
    return (
      <AgentPage
        key={open}
        id={open}
        agents={agents}
        editable={mayEdit}
        seed={open === NEW_AGENT ? seed : null}
        onOpen={setOpen}
        onDuplicate={(d) => {
          setSeed(d);
          setOpen(NEW_AGENT);
        }}
        onChanged={changed}
        scope={scope}
        personal={props.workspace.personal}
        serverAdmin={mayEditAgents(props.me)}
      />
    );
  }
  const remove = async (a: Agent) => {
    setErr(null);
    try {
      if (await confirmDelete(a, scope)) changed();
    } catch (x) {
      setErr(message(x));
    }
  };
  return (
    <div className="stack">
      <div className="row">
        <p className="muted small grow">
          The agents chats in this workspace can use, with this workspace's models. Built-in and server agents belong to
          the whole server; {mayEdit ? "you can add this workspace's own instructions to them, and make agents only this workspace has." : "agents marked workspace are this workspace's own, made by its admins."}
        </p>
        {mayEdit && (
          <button
            type="button"
            className="primary"
            onClick={() => {
              setSeed(null);
              setOpen(NEW_AGENT);
            }}
          >
            New agent
          </button>
        )}
      </div>
      {err && <div className="error-text">{err}</div>}
      {hidden.map((h) => (
        <div key={h.id} className="row hidden-agent">
          <span className="small grow">
            {h.name} <span className="mono muted">@{h.id}</span>: a server agent has the same handle, so it is hidden here and @{h.id} means
            the server's. Delete it, or make it again under another handle.
          </span>
          <button
            type="button"
            onClick={() =>
              void remove({ id: h.id, name: h.name } as Agent)
            }
          >
            Delete
          </button>
        </div>
      ))}
      {agents === null ? (
        <div className="muted">Loading…</div>
      ) : (
        <AgentList
          agents={agents}
          editable={() => mayEdit}
          deletable={(a) => a.scope === "workspace"}
          onOpen={setOpen}
          onDelete={(a) => void remove(a)}
        />
      )}
    </div>
  );
}
