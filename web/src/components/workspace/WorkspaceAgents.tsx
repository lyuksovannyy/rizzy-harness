// Workspace settings → Agents: the agents of this workspace as the Agents
// page lists them (AgentList, the same rows and the same actions), read in
// this workspace's scope so each one's model line is the workspace's. Every
// agent is the server's today (built-in, a file, or made on the Agents page
// by a server admin; docs/agent-pages.md): there are no private or
// workspace-only agents to hide, and only server admins see the edit and
// delete actions, as on the Agents page. A row opens the agent's page,
// where its switches (Personalisation among them) are. A lazy chunk.
import { useCallback, useEffect, useState } from "react";
import { api } from "../../api";
import { mayEditAgents } from "../../lib/agents";
import { routeHash } from "../../lib/routes";
import type { Agent } from "../../types";
import { confirmDelete } from "../AgentPage";
import { AgentList } from "../Agents";
import { message, type WorkspacePanelProps } from "./panel";

export default function WorkspaceAgents(props: WorkspacePanelProps) {
  const ws = props.workspace.workspace_id;
  const [agents, setAgents] = useState<Agent[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(
    () =>
      api.agents({ workspace: ws }).then(
        (r) => setAgents(r.agents),
        (x) => setErr(message(x)),
      ),
    [ws],
  );
  useEffect(() => void load(), [load]);
  const editable = mayEditAgents(props.me);
  const open = (id: string) => {
    location.hash = routeHash({ kind: "agents", id });
  };
  const remove = async (a: Agent) => {
    setErr(null);
    try {
      if (await confirmDelete(a)) await load();
    } catch (x) {
      setErr(message(x));
    }
  };
  return (
    <div className="stack">
      <p className="muted small">
        The agents chats in this workspace can use, with this workspace's models. Agents belong to the whole server: {editable ? "you can change them here or on the Agents page." : "only server admins can add or change them."}
      </p>
      {err && <div className="error-text">{err}</div>}
      {agents === null ? <div className="muted">Loading…</div> : <AgentList agents={agents} editable={editable} onOpen={open} onDelete={(a) => void remove(a)} />}
    </div>
  );
}
