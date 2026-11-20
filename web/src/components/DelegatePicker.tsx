// Who an agent may hand tasks to (X.41): under its sub-agent switch, a
// multi-select of the agents offered as sub-agents. No pick means all of
// them; any pick narrows to the picked ones, never beyond what the server
// offers. A pick whose agent is gone stays visible, marked, so it can be
// unticked. Built-in and file agents show their list, read-only.
import { marked } from "../lib/agents";
import type { Agent } from "../types";

/** The agents `self` may be given as sub-agents: offered as one, and not itself. */
export function delegateOptions(agents: Agent[], self: string, serverScope = false): Agent[] {
  // A server agent never gets a workspace's agent as a sub-agent, and the
  // server refuses the save: the viewer's list holds both, offer what it takes.
  return agents.filter((a) => a.id !== self && a.features?.as_subagent !== false && !(serverScope && a.scope === "workspace"));
}

export function DelegatePicker(props: {
  agents: Agent[];
  self: string;
  picks: string[];
  /** The agent is the server's: only the server's own agents are offered. */
  serverScope?: boolean;
  /** The switch above: off keeps the list but it does nothing. */
  active: boolean;
  /** Not editable here: shows `shown` (the agent's resolved delegates) instead. */
  readOnly: boolean;
  shown?: string[];
  onChange: (picks: string[]) => void;
}) {
  const options = delegateOptions(props.agents, props.self, props.serverScope);
  const known = new Set(options.map((a) => a.id));
  if (props.readOnly) {
    const by = new Map(props.agents.map((a) => [a.id, a]));
    const ids = props.shown ?? [];
    return (
      <div className="field delegate-picker">
        <span className="field-label">May delegate to</span>
        {ids.length === 0 ? (
          <p className="field-hint">{props.active ? "No sub-agents." : "It launches no sub-agents."}</p>
        ) : (
          <ul className="delegate-list">
            {ids.map((id) => (
              <li key={id}>
                {by.get(id)?.name ?? id} <span className="mono muted">@{id}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    );
  }
  const gone = props.picks.filter((p) => !known.has(p));
  const toggle = (id: string, on: boolean) =>
    props.onChange(on ? [...props.picks, id] : props.picks.filter((p) => p !== id));
  return (
    <fieldset className="field delegate-picker" disabled={!props.active} aria-label="May delegate to">
      <span className="field-label">May delegate to</span>
      <p className="field-hint">
        {props.picks.length === 0 ? "All sub-agents. Tick agents to narrow it." : `Only the ${props.picks.length} ticked.`}
        {!props.active && " Switch on Launch sub-agents for this to apply."}{" "}
        {props.picks.length > 0 && (
          <button type="button" className="link small" onClick={() => props.onChange([])}>
            All sub-agents
          </button>
        )}
      </p>
      <ul className="delegate-list">
        {options.map((a) => (
          <li key={a.id}>
            <label className="delegate-option">
              <input type="checkbox" checked={props.picks.includes(a.id)} onChange={(e) => toggle(a.id, e.target.checked)} />
              <span>
                {a.name} <span className="mono muted">@{a.id}</span>
                {marked(a) && <span className="small muted"> · {marked(a)}</span>}
              </span>
            </label>
          </li>
        ))}
        {gone.map((id) => (
          <li key={id}>
            <label className="delegate-option">
              <input type="checkbox" checked onChange={() => toggle(id, false)} />
              <span>
                <span className="mono">@{id}</span> <span className="small error-text">no longer available</span>
              </span>
            </label>
          </li>
        ))}
      </ul>
    </fieldset>
  );
}
