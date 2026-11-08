// Sidebar → Agents (docs/agent-pages.md): every agent with its face, name,
// handle and summary. People who may edit agents get a pencil and, for the
// agents made here, a trash can; built-in and file agents cannot be deleted.
// A row opens the agent's own page (AgentPage).
import { Pencil, Trash2 } from "lucide-react";
import { useState } from "react";
import type { Scope } from "../api";
import { mayEditAgents, NEW_AGENT } from "../lib/agents";
import type { Agent, Me, NewAgent } from "../types";
import { AgentMark, agentAlert } from "./AgentAvatar";
import { AgentPage, agentMarkers, confirmDelete } from "./AgentPage";

export function AgentList(props: {
  agents: Agent[];
  editable: boolean;
  onOpen: (id: string) => void;
  onDelete: (a: Agent) => void;
}) {
  if (props.agents.length === 0) return <div className="muted">No agents.</div>;
  return (
    <ul className="card agent-list agents-index">
      {props.agents.map((a) => (
        <li key={a.id} className={`agent-item ${agentAlert(a) ? "bad" : ""}`}>
          <button type="button" className="agent-open" onClick={() => props.onOpen(a.id)}>
            <AgentMark agent={a} size={36} />
            <span className="agent-info">
              <span className="agent-name">
                {a.name} <span className="mono muted small">@{a.id}</span>
                {agentMarkers(a).map((m) => (
                  <span key={m} className="muted small">
                    {" "}
                    · {m}
                  </span>
                ))}
              </span>
              <span className={`small agent-summary ${a.description ? "muted" : "muted faint"}`}>
                {a.description || "No summary yet."}
              </span>
            </span>
          </button>
          {props.editable && (
            <span className="agent-actions">
              <button type="button" className="icon" aria-label={`Edit ${a.name}`} title="Edit" onClick={() => props.onOpen(a.id)}>
                <Pencil size={16} />
              </button>
              {a.origin === "custom" ? (
                <button type="button" className="icon" aria-label={`Delete ${a.name}`} title="Delete" onClick={() => props.onDelete(a)}>
                  <Trash2 size={16} />
                </button>
              ) : (
                // Built-in and file agents stay; the slot keeps the pencils in line.
                <span className="icon-slot" aria-hidden="true" />
              )}
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}

export default function Agents(props: {
  me: Me;
  agents: Agent[];
  /** null: the list; NEW_AGENT: a new agent; else that agent's page. */
  id: string | null;
  onOpen: (id: string | null) => void;
  /** An agent was made, changed or deleted: reload the agents. */
  onChanged: () => void;
  /** The workspace `agents` were listed for (the agent page reads in it too). */
  scope?: Scope | null;
}) {
  const editable = mayEditAgents(props.me);
  // A duplicate's starting point, kept while the new-agent page shows.
  const [seed, setSeed] = useState<NewAgent | null>(null);
  const [err, setErr] = useState<string | null>(null);
  if (props.id !== null) {
    return (
      <AgentPage
        key={props.id}
        id={props.id}
        agents={props.agents}
        editable={editable}
        seed={props.id === NEW_AGENT ? seed : null}
        onOpen={props.onOpen}
        onDuplicate={(d) => {
          setSeed(d);
          props.onOpen(NEW_AGENT);
        }}
        onChanged={props.onChanged}
        scope={props.scope}
      />
    );
  }
  const remove = async (a: Agent) => {
    setErr(null);
    try {
      if (await confirmDelete(a)) props.onChanged();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    }
  };
  return (
    <section className="agents-page">
      <div className="column wide">
        <div className="agents-head">
          <h1>Agents</h1>
          {editable && (
            <button
              type="button"
              className="primary"
              onClick={() => {
                setSeed(null);
                props.onOpen(NEW_AGENT);
              }}
            >
              New agent
            </button>
          )}
        </div>
        <p className="muted agents-intro">
          Who answers in a chat. Pick one in the bar or type @ and its handle.
          {!editable && " Only admins can add or change agents."}
        </p>
        {err && <div className="error-text">{err}</div>}
        <AgentList agents={props.agents} editable={editable} onOpen={props.onOpen} onDelete={(a) => void remove(a)} />
      </div>
    </section>
  );
}
