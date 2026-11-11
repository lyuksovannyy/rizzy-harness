// How an agent looks: shape, eyes and accent, with a live preview that also
// plays the faces it makes while it works. Opened from the agent's page.
import { useEffect, useRef, useState } from "react";
import { pageApi } from "../apiPages";
import { radioKeys, radioTab } from "../lib/radios";
import type { Agent, AgentAvatar as Avatar, AvatarState } from "../types";
import { AgentAvatar, AVATAR_ACCENTS, EYES, SHAPES, STATES } from "./AgentAvatar";

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

export function AvatarEditor(props: { agent: Agent; onSaved: () => void; onClose: () => void }) {
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
      await pageApi.saveAgentAvatar(props.agent.id, avatar);
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
