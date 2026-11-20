// An agent's face: a flat, soft blob with two pill eyes, in one colour.
// Drawn in SVG from theme tokens (styles.css, "Agent avatars"), so it
// follows the theme; the face shows what the agent is doing. Motion is CSS
// only and stops under reduced motion, where the expressions stay.
import { own } from "../emptyView";
import { AlertTriangle } from "lucide-react";
import { useId, useRef } from "react";
import { useMedia } from "../bg/hooks";
import { normalizeAccent } from "../lib/appearance";
import { fnv1a } from "../lib/fnv";
import { rawModel } from "../lib/models";
import type { Agent, AgentAvatar as Avatar, AvatarAccent, AvatarEyes, AvatarShape, AvatarState } from "../types";

export const SHAPES: AvatarShape[] = [
  "circle", "triangle", "square", "hexagon", "diamond", "drop", "cloud", "bean", "heart", "star", "flower",
  "ghost", "cat", "bunny", "mushroom", "leaf", "mug", "shield", "bubble", "hat", "rocket",
];
export const EYES: AvatarEyes[] = ["long", "round", "short", "wide", "sleepy"];
/** The colours the avatar picker offers, in its order. */
export const AVATAR_ACCENTS: AvatarAccent[] = [
  "white", "brown", "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink", "graphite", "black",
];
export const STATES: AvatarState[] = ["idle", "thinking", "working", "unsure", "done", "error"];

const REDUCE = "(prefers-reduced-motion: reduce)";
const OWN_ACCENTS = ["white", "black", "brown", "teal", "yellow", "green"];

/** The theme token a colour is drawn in: the UI accents' graphics tone, or
 * the avatar's own (white, black, brown, teal, and a yellow and a green that hold on a light or a dark page). Anything else is blue. */
export function accentVar(accent: string | null | undefined): string {
  if (accent && OWN_ACCENTS.includes(accent)) return `var(--av-c-${accent})`;
  return `var(--ac-${normalizeAccent(accent) ?? "blue"}-2)`;
}

// The bodies in a 40×40 box, one flat fill. `eye` is where the eyes sit:
// their height, half the gap between them and, when the body is not
// centred, their middle.
const BODY: Record<AvatarShape, { d: string; eye: [number, number, number?] }> = {
  circle: { d: "M5.5 20a14.5 13.5 0 1 1 29 0a14.5 13.5 0 1 1-29 0z", eye: [19, 3.8] },
  triangle: { d: "M17.5 8.2Q20 4.5 22.5 8.2L34.5 28.5Q36.5 33 31.5 33H8.5Q3.5 33 5.5 28.5Z", eye: [25, 3.8] },
  square: { d: "M14 6h12c5 0 8 3 8 8v12c0 5-3 8-8 8H14c-5 0-8-3-8-8V14c0-5 3-8 8-8z", eye: [20, 3.8] },
  hexagon: { d: "M16.9 6.7Q20 5 23.1 6.7l7.3 4.1Q33.5 12.5 33.5 16v8q0 3.5-3.1 5.2l-7.3 4.1Q20 35 16.9 33.3L9.6 29.2Q6.5 27.5 6.5 24v-8q0-3.5 3.1-5.2z", eye: [20, 3.8] },
  diamond: { d: "M17 7.5Q20 4.5 23 7.5L32.5 17Q35.5 20 32.5 23L23 32.5Q20 35.5 17 32.5L7.5 23Q4.5 20 7.5 17Z", eye: [20, 3.8] },
  drop: { d: "M20 5.5c2.5 0 3 1.5 4.5 4C28 15 33 18.5 33 24.5a13 13 0 0 1-26 0C7 18.5 12 15 15.5 9.5 17 7 17.5 5.5 20 5.5z", eye: [25, 3.8] },
  cloud: { d: "M11 33a8 8 0 0 1-1.5-15.8A10 10 0 0 1 20 8a10 10 0 0 1 10 9.2A8 8 0 0 1 29 33z", eye: [23, 3.8] },
  bean: { d: "M9 16a11 11 0 0 1 22 0v8a11 11 0 0 1-22 0z", eye: [17.5, 3.8] },
  heart: { d: "M20 34C8 26 5 20 5 14.5a8.5 8.5 0 0 1 15-5.5a8.5 8.5 0 0 1 15 5.5C35 20 32 26 20 34z", eye: [18, 3.8] },
  star: { d: "M17.9 8.1q2.1-3.6 4.3 0l2.4 4.1q1 1.7 3 2.2l4.6 1q4.1.9 1.3 4l-3.1 3.5q-1.3 1.5-1.1 3.5l.5 4.7q.4 4.2-3.4 2.5l-4.3-1.9q-1.8-.8-3.7 0l-4.3 1.9q-3.9 1.7-3.4-2.5l.5-4.7q.2-2-1.1-3.5l-3.1-3.5q-2.8-3.1 1.3-4l4.6-1q2-.4 3-2.2z", eye: [21.3, 3.7] },
  flower: { d: "M32.4 25.6a5.2 5.2 0 0 1-7.3 7.3a5.2 5.2 0 0 1-10.3 0a5.2 5.2 0 0 1-7.3-7.3a5.2 5.2 0 0 1 0-10.3a5.2 5.2 0 0 1 7.3-7.3a5.2 5.2 0 0 1 10.3 0a5.2 5.2 0 0 1 7.3 7.3a5.2 5.2 0 0 1 0 10.3z", eye: [20.3, 3.8] },
  ghost: { d: "M7 31V19a13 13 0 0 1 26 0v12a4.33 4.33 0 0 1-8.67 0a4.33 4.33 0 0 1-8.66 0a4.33 4.33 0 0 1-8.67 0z", eye: [19, 3.8] },
  cat: { d: "M7 24Q7 14 8.5 7l8.5 5q3-1 6 0l8.5-5Q33 14 33 24a10 10 0 0 1-10 10H17A10 10 0 0 1 7 24z", eye: [22.5, 3.8] },
  bunny: { d: "M12 18C9 12 9.5 4 13.5 4S18 11 18.5 15.5q1-.2 1.5-.2t1.5.2C22 11 23 4 26.5 4S31 12 28 18a11 11 0 1 1-16 0z", eye: [25.5, 3.8] },
  mushroom: { d: "M5 20C5 11 11.5 5 20 5s15 6 15 15c0 1.5-1 2.5-2.5 2.5H28V30a4 4 0 0 1-4 4h-8a4 4 0 0 1-4-4v-7.5H7.5C6 22.5 5 21.5 5 20z", eye: [18.5, 3.8] },
  leaf: { d: "M8 32C6 18 14 7 33 6c1 19-6 27-18 27-3 0-5-.5-7-1z", eye: [21, 3.8, 20.5] },
  mug: { d: "M6 10a3 3 0 0 1 3-3h16a3 3 0 0 1 3 3v17a7 7 0 0 1-7 7H13a7 7 0 0 1-7-7zM28 13h2a6 6 0 0 1 0 12h-2v-3h2a3 3 0 0 0 0-6h-2z", eye: [20, 3.8, 17] },
  shield: { d: "M20 5c4 3 9 4 14 4.5V20c0 8-6 13-14 16C12 33 6 28 6 20V9.5C11 9 16 8 20 5z", eye: [18.5, 3.8] },
  bubble: { d: "M12 6h16a7 7 0 0 1 7 7v10a7 7 0 0 1-7 7H18l-8 6v-6.3A7 7 0 0 1 5 23V13a7 7 0 0 1 7-7z", eye: [17.5, 3.8] },
  hat: { d: "M3.5 31Q3.5 27.5 8 26.5L14.5 10.5Q16 6 21 4.5Q25 3.5 31 6.5Q28 8 26.5 12L33 26.5Q36.5 27.5 36.5 31Q36.5 35 20 35Q3.5 35 3.5 31z", eye: [23, 3.8, 19.8] },
  rocket: { d: "M10 20C10 13 14 7 17.5 3.8Q20 1.5 22.5 3.8C26 7 30 13 30 20V22Q35 23.5 37 32L30 29.5V34H10V29.5L3 32Q5 23.5 10 22z", eye: [17.5, 3.8] },
};

// Each eye style, a pill: its radii, how much farther apart it sits and how
// much lower. `long`, the tall pill, is the default look.
const EYE: Record<AvatarEyes, { rx: number; ry: number; spread: number; dy: number }> = {
  long: { rx: 2.1, ry: 5, spread: 0, dy: 0 },
  round: { rx: 3.3, ry: 3.5, spread: 0.7, dy: 0.3 },
  short: { rx: 2.3, ry: 3.3, spread: 0.1, dy: 0.7 },
  wide: { rx: 3.6, ry: 2.5, spread: 1.3, dy: 0.5 },
  sleepy: { rx: 3.2, ry: 1.2, spread: 0.8, dy: 1.3 },
};

/** Where the pointer is, -1..1 each way, for the eyes to follow. */
function gaze(svg: SVGSVGElement, e: React.PointerEvent) {
  const r = svg.getBoundingClientRect();
  const clamp = (v: number) => Math.max(-1, Math.min(1, v));
  svg.style.setProperty("--av-gx", String(clamp((e.clientX - r.left - r.width / 2) / r.width)));
  svg.style.setProperty("--av-gy", String(clamp((e.clientY - r.top - r.height / 2) / r.height)));
}

export function AgentAvatar(props: {
  avatar: Avatar;
  state?: AvatarState;
  /** Pixels; 16 to 40 reads best. */
  size?: number;
  /** Read out and shown on hover; without one the avatar is decoration. */
  title?: string;
}) {
  const { shape, eyes } = props.avatar;
  // Anything that is not a colour of ours draws in the default.
  const accent = OWN_ACCENTS.includes(props.avatar.accent) ? props.avatar.accent : (normalizeAccent(props.avatar.accent) ?? "blue");
  const state = props.state ?? "idle";
  const size = props.size ?? 24;
  const still = useMedia(REDUCE);
  const raw = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const svg = useRef<SVGSVGElement>(null);
  const body = own(BODY, shape) ?? BODY.circle;
  const eye = own(EYE, eyes) ?? EYE.long;
  const [cy, half, cx = 20] = body.eye;
  const gap = half + eye.spread;
  // Agents side by side should not blink together.
  const delay = `${(fnv1a(`${shape}${eyes}${accent}${raw}`) % 4000) / 1000}s`;
  const one = (side: "l" | "r") => (
    <g className={`av-eye ${side}`} transform={`translate(${cx + (side === "l" ? -gap : gap)} ${cy + eye.dy})`}>
      <g className="av-gaze">
        <g className="av-look">
          <g className="av-lid">
            <rect className="av-pupil" x={-eye.rx} y={-eye.ry} width={eye.rx * 2} height={eye.ry * 2} rx={Math.min(eye.rx, eye.ry)} />
            {eye.ry > 1.5 && <circle className="av-glint" cx={eye.rx * 0.3} cy={-eye.ry * 0.4} r={eye.rx * 0.45} />}
            <path className="av-happy" d="M-3 1.3Q0 -3 3 1.3" />
          </g>
        </g>
      </g>
      <path className="av-brow" d={`M${side === "l" ? -3 : 3} ${-eye.ry - 2.3}L${side === "l" ? 2.4 : -2.4} ${-eye.ry - 3.7}`} />
    </g>
  );
  return (
    <svg
      ref={svg}
      className="agent-avatar"
      viewBox="2 2.5 36 36"
      width={size}
      height={size}
      data-shape={shape}
      data-eyes={eyes}
      data-accent={accent}
      data-state={state}
      data-motion={still ? "still" : "live"}
      role={props.title ? "img" : undefined}
      aria-label={props.title}
      aria-hidden={props.title ? undefined : true}
      style={{ "--av-c": accentVar(accent), "--av-delay": delay } as React.CSSProperties}
      onPointerMove={still ? undefined : (e) => svg.current && gaze(svg.current, e)}
      onPointerLeave={() => {
        svg.current?.style.removeProperty("--av-gx");
        svg.current?.style.removeProperty("--av-gy");
      }}
    >
      {props.title && <title>{props.title}</title>}
      <ellipse className="av-ground" cx="20" cy="36.2" rx="10.5" ry="1.7" />
      <g className="av-figure">
        <g className="av-move">
          <path className="av-face" d={body.d} />
          {one("l")}
          {one("r")}
        </g>
      </g>
    </svg>
  );
}

/** Why an agent is in trouble, or null: its main model is gone, whether or
 * not a fallback still answers in its place. */
export function agentAlert(agent: Pick<Agent, "name" | "main_model" | "fallbacks" | "unavailable">): string | null {
  const main = agent.main_model;
  const gone = agent.unavailable;
  if (!main || !gone.includes(main)) return null;
  const next = agent.fallbacks.find((f) => f !== main && !gone.includes(f));
  return next
    ? `${agent.name}'s model ${rawModel(main)} is no longer available; answering with fallback ${rawModel(next)}.`
    : `${agent.name}'s model ${rawModel(main)} is no longer available, and no fallback is left to answer instead.`;
}

/** An agent wherever it is listed: its avatar, or a red alert when its
 * model is gone. Callers show the name beside it, so the avatar itself is
 * decoration unless given a `title`. */
export function AgentMark(props: { agent: Agent; state?: AvatarState; size?: number; title?: string }) {
  const size = props.size ?? 18;
  const alert = agentAlert(props.agent);
  if (alert) {
    return (
      <span className="agent-alert" role="img" aria-label={alert} title={alert} style={{ width: size, height: size }}>
        <AlertTriangle size={Math.round(size * 0.78)} aria-hidden="true" />
      </span>
    );
  }
  return (
    <AgentAvatar avatar={props.agent.avatar} state={props.state} size={size} title={props.title} />
  );
}
