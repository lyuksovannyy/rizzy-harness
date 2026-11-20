// How an agent looks: a grid of shapes, a row of colours and the eyes, with
// a live preview that also plays the faces it makes while it works. Opened from the agent's page.
import { useEffect, useRef, useState } from "react";
import { pageApi } from "../apiPages";
import { radioKeys, radioTab } from "../lib/radios";
import type { Agent, AgentAvatar as Avatar, AvatarAccent, AvatarEyes, AvatarShape, AvatarState } from "../types";
import { accentVar, AgentAvatar, AVATAR_ACCENTS, EYES, SHAPES, STATES } from "./AgentAvatar";

const SHAPE_NAME: Record<AvatarShape, string> = {
  circle: "round blob", triangle: "rounded triangle", square: "soft square", hexagon: "hexagon", diamond: "diamond",
  drop: "drop", cloud: "cloud", bean: "bean", heart: "heart", star: "star", flower: "flower", ghost: "ghost",
  cat: "cat", bunny: "bunny", mushroom: "mushroom", leaf: "leaf", mug: "mug", shield: "shield",
  bubble: "speech bubble", hat: "wizard hat", rocket: "rocket",
};
const EYES_NAME: Record<AvatarEyes, string> = { long: "tall", round: "round", short: "short", wide: "wide", sleepy: "sleepy" };
const COLOUR_NAME: Partial<Record<AvatarAccent, string>> = { graphite: "grey" };

/** One radio group of avatar choices; `draw` paints each as it will look. */
function Picks<K extends keyof Avatar>(props: {
  label: string;
  className: string;
  part: K;
  values: Avatar[K][];
  names: (v: Avatar[K]) => string;
  avatar: Avatar;
  onPick: (a: Avatar) => void;
  draw: (v: Avatar[K], next: Avatar) => React.ReactNode;
}) {
  return (
    <div className="field">
      {props.label}
      <div className={`row wrap avatar-picks ${props.className}`} role="radiogroup" aria-label={props.label} onKeyDown={radioKeys}>
        {props.values.map((v) => {
          const on = props.avatar[props.part] === v;
          const next = { ...props.avatar, [props.part]: v };
          const name = props.names(v);
          return (
            <button
              key={v}
              type="button"
              role="radio"
              aria-checked={on}
              tabIndex={radioTab(on)}
              aria-label={name}
              title={name}
              className={on ? "on" : ""}
              onClick={() => props.onPick(next)}
            >
              {props.draw(v, next)}
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
  // Focus moves into the editor that opens, on the shape picked.
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => box.current?.querySelector<HTMLElement>('[aria-label="Shape"] [role="radio"][tabindex="0"]')?.focus(), []);
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
      <Picks
        label="Shape"
        className="avatar-shapes"
        part="shape"
        values={SHAPES}
        names={(v) => SHAPE_NAME[v]}
        avatar={avatar}
        onPick={setAvatar}
        draw={(_, next) => <AgentAvatar avatar={next} size={40} />}
      />
      <Picks
        label="Colour"
        className="avatar-swatches"
        part="accent"
        values={AVATAR_ACCENTS}
        names={(v) => COLOUR_NAME[v] ?? v}
        avatar={avatar}
        onPick={setAvatar}
        draw={(v) => <span className="avatar-swatch" data-accent={v} style={{ "--sw": accentVar(v) } as React.CSSProperties} />}
      />
      <Picks
        label="Eyes"
        className="avatar-eyes"
        part="eyes"
        values={EYES}
        names={(v) => EYES_NAME[v]}
        avatar={avatar}
        onPick={setAvatar}
        draw={(_, next) => <AgentAvatar avatar={next} size={32} />}
      />
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
