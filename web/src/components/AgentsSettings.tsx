// Settings, Agents: each agent with its face and models. An admin can
// change how an agent looks (shape, eyes, accent) with a live preview that
// also plays the faces it makes while it works.
import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { radioKeys, radioTab } from "../lib/radios";
import type { Agent, AgentAvatar as Avatar, AvatarState } from "../types";
import { rawModel } from "../lib/models";
import { AgentAvatar, AgentMark, agentAlert, AVATAR_ACCENTS, EYES, SHAPES, STATES } from "./AgentAvatar";

/** One radio group of avatar choices, each drawn as the avatar it gives. */
function Picks<K extends keyof Avatar>(props: {
  label: string;
  part: K;
  values: Avatar[K][];
  avatar: Avatar;
  onPick: (a: Avatar) => void;
}) {
  return (
    <div className="field">
      {props.label}
      <div className="row wrap avatar-picks" role="radiogroup" aria-label={props.label} onKeyDown={radioKeys}>
        {props.values.map((v) => {
          const on = props.avatar[props.part] === v;
          const next = { ...props.avatar, [props.part]: v };
          return (
            <button
              key={v}
              type="button"
              role="radio"
              aria-checked={on}
              tabIndex={radioTab(on)}
              aria-label={v}
              title={v}
              className={on ? "on" : ""}
              onClick={() => props.onPick(next)}
            >
              <AgentAvatar avatar={next} size={28} />
            </button>
          );
        })}
      </div>
    </div>
  );
}

function AvatarEditor(props: { agent: Agent; onSaved: () => void; onClose: () => void }) {
  const [avatar, setAvatar] = useState<Avatar>(props.agent.avatar);
  const [state, setState] = useState<AvatarState>("idle");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // Focus moves into the editor that opens, on the face picked.
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => box.current?.querySelector<HTMLElement>('[role="radio"][tabindex="0"]')?.focus(), []);
  const save = async () => {
    setBusy(true);
    setErr(null);
    try {
      await api.saveAgentAvatar(props.agent.id, avatar);
      props.onSaved();
      props.onClose();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="avatar-editor stack" ref={box}>
      <div className="avatar-preview">
        <AgentAvatar avatar={avatar} state={state} size={64} title={`${props.agent.name}, ${state}`} />
        <div className="seg" role="radiogroup" aria-label="preview face" onKeyDown={radioKeys}>
          {STATES.map((s) => (
            <button
              key={s}
              type="button"
              role="radio"
              aria-checked={state === s}
              tabIndex={radioTab(state === s)}
              className={state === s ? "on" : ""}
              onClick={() => setState(s)}
            >
              {s}
            </button>
          ))}
        </div>
      </div>
      <Picks label="Shape" part="shape" values={SHAPES} avatar={avatar} onPick={setAvatar} />
      <Picks label="Eyes" part="eyes" values={EYES} avatar={avatar} onPick={setAvatar} />
      <Picks label="Colour" part="accent" values={AVATAR_ACCENTS} avatar={avatar} onPick={setAvatar} />
      {err && <div className="error-text">{err}</div>}
      <div className="row end">
        <button type="button" onClick={props.onClose}>
          Cancel
        </button>
        <button type="button" className="primary" disabled={busy} onClick={() => void save()}>
          Save
        </button>
      </div>
    </div>
  );
}

export function AgentsSettings(props: { agents: Agent[]; admin: boolean; onChanged: () => void }) {
  const [editing, setEditing] = useState<string | null>(null);
  // The editor closed: focus goes back to the button that opened it.
  const edits = useRef(new Map<string, HTMLButtonElement>());
  const back = useRef<string | null>(null);
  useEffect(() => {
    if (editing !== null || !back.current) return;
    edits.current.get(back.current)?.focus();
    back.current = null;
  }, [editing]);
  if (props.agents.length === 0) return <div className="muted">No agents.</div>;
  return (
    <ul className="card agent-list">
      {props.agents.map((a) => {
        const alert = agentAlert(a);
        const gone = a.unavailable;
        // With no chain of its own it runs on the server default, and its
        // fallbacks are General's: show them after the model they back up.
        const main = a.main_model;
        const fallbacks = a.fallbacks;
        const head = a.default_model ?? (fallbacks.length ? main : null);
        const chain = head ? [head, ...fallbacks] : [];
        return (
          <li key={a.id} className={alert ? "bad" : ""}>
            <div className="agent-row">
              <AgentMark agent={a} size={32} />
              <div className="agent-info">
                <span className="agent-name">
                  {a.name} <span className="mono muted small">@{a.id}</span>
                </span>
                {a.description && <span className="muted small">{a.description}</span>}
                <span className="small agent-models">
                  {chain.length === 0 ? (
                    <span className={main && gone.includes(main) ? "gone" : "muted"}>
                      server default model{main && gone.includes(main) ? ` (${rawModel(main)}, unavailable)` : ""}
                    </span>
                  ) : (
                    chain.map((m, i) => (
                      <span key={m} className={gone.includes(m) ? "gone" : "muted"} title={gone.includes(m) ? "No longer available" : undefined}>
                        {i > 0 ? " → " : ""}
                        {rawModel(m)}
                        {gone.includes(m) ? " (unavailable)" : ""}
                      </span>
                    ))
                  )}
                </span>
                {alert && <span className="small error-text">{alert}</span>}
              </div>
              {props.admin && editing !== a.id && (
                <button
                  type="button"
                  className="link"
                  ref={(el) => {
                    if (el) edits.current.set(a.id, el);
                    else edits.current.delete(a.id);
                  }}
                  onClick={() => setEditing(a.id)}
                >
                  Edit look
                </button>
              )}
            </div>
            {props.admin && editing === a.id && (
              <AvatarEditor
                agent={a}
                onSaved={props.onChanged}
                onClose={() => {
                  back.current = a.id;
                  setEditing(null);
                }}
              />
            )}
          </li>
        );
      })}
    </ul>
  );
}
