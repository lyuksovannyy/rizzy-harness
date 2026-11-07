// An agent's face: a small 2.5D shape with eyes, in one of the accents.
// Drawn in SVG from theme tokens (styles.css, "Agent avatars"), so it
// follows the theme; the face shows what the agent is doing. Motion is CSS
// only and stops under reduced motion, where the expressions stay.
import { AlertTriangle } from "lucide-react";
import { useId, useRef } from "react";
import { useMedia } from "../bg/hooks";
import { rawModel } from "../lib/models";
import type { Agent, AgentAvatar as Avatar, AvatarAccent, AvatarEyes, AvatarShape, AvatarState } from "../types";

export const SHAPES: AvatarShape[] = ["circle", "triangle", "square", "hexagon", "diamond", "drop"];
export const EYES: AvatarEyes[] = ["round", "long", "short", "wide", "sleepy"];
export const AVATAR_ACCENTS: AvatarAccent[] = ["blue", "violet", "green", "orange", "pink", "gray"];
export const STATES: AvatarState[] = ["idle", "thinking", "working", "unsure", "done", "error"];

const REDUCE = "(prefers-reduced-motion: reduce)";

/** FNV-1a: small and stable, the same id always gives the same face. */
function hash(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

/** The face an agent has until an admin picks one (the server sends its own). */
export function defaultAvatar(id: string): Avatar {
  const h = hash(id);
  return {
    shape: SHAPES[h % SHAPES.length]!,
    eyes: EYES[(h >>> 8) % EYES.length]!,
    accent: AVATAR_ACCENTS[(h >>> 16) % AVATAR_ACCENTS.length]!,
  };
}

// The bodies in a 40×40 box, drawn with a round stroke of their own paint
// so the corners soften. `eye` is where the eyes sit: their height and half
// the gap between them.
const BODY: Record<AvatarShape, { d: string; eye: [number, number] }> = {
  circle: { d: "M20 6a13 13 0 1 1 0 26a13 13 0 1 1 0-26z", eye: [19, 5.4] },
  triangle: { d: "M20 7.5L32.5 30H7.5Z", eye: [23.5, 4.4] },
  square: { d: "M13 7h14a6 6 0 0 1 6 6v12a6 6 0 0 1-6 6H13a6 6 0 0 1-6-6V13a6 6 0 0 1 6-6z", eye: [18.5, 5.6] },
  hexagon: { d: "M20 6.5L31.5 13V25.5L20 32L8.5 25.5V13Z", eye: [19, 5.2] },
  diamond: { d: "M20 5.5L33.5 19L20 32.5L6.5 19Z", eye: [19, 4.8] },
  drop: { d: "M20 5.5C24.5 11 31.5 16.5 31.5 22.5a11.5 11.5 0 0 1-23 0C8.5 16.5 15.5 11 20 5.5z", eye: [22.5, 4.8] },
};

// Each eye style: the pupil's radii, and how much farther apart it sits.
const EYE: Record<AvatarEyes, { rx: number; ry: number; spread: number; dy: number }> = {
  round: { rx: 2.3, ry: 2.5, spread: 0, dy: 0 },
  long: { rx: 1.5, ry: 3.4, spread: -0.3, dy: 0 },
  short: { rx: 2, ry: 1.4, spread: -0.2, dy: 0.2 },
  wide: { rx: 3.2, ry: 2, spread: 1, dy: 0 },
  sleepy: { rx: 2.5, ry: 1, spread: 0.2, dy: 0.6 },
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
  const { shape, eyes, accent } = props.avatar;
  const state = props.state ?? "idle";
  const size = props.size ?? 24;
  const still = useMedia(REDUCE);
  const raw = useId().replace(/[^a-zA-Z0-9_-]/g, "");
  const shine = `av-shine-${raw}`;
  const shade = `av-shade-${raw}`;
  const svg = useRef<SVGSVGElement>(null);
  const body = BODY[shape] ?? BODY.circle;
  const eye = EYE[eyes] ?? EYE.round;
  const [cy, half] = body.eye;
  const gap = half + eye.spread;
  // Agents side by side should not blink together.
  const delay = `${(hash(`${shape}${eyes}${accent}${raw}`) % 4000) / 1000}s`;
  const one = (side: "l" | "r") => (
    <g className={`av-eye ${side}`} transform={`translate(${20 + (side === "l" ? -gap : gap)} ${cy + eye.dy})`}>
      <g className="av-gaze">
        <g className="av-look">
          <g className="av-lid">
            <ellipse className="av-pupil" rx={eye.rx} ry={eye.ry} />
            <path className="av-happy" d="M-2.5 0.9Q0 -2.3 2.5 0.9" />
          </g>
        </g>
      </g>
      <path className="av-brow" d={side === "l" ? "M-2.6 -4.2L2.3 -5.6" : "M2.6 -4.2L-2.3 -5.6"} />
    </g>
  );
  return (
    <svg
      ref={svg}
      className="agent-avatar"
      viewBox="0 0 40 40"
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
      style={{ "--av-c": `var(--accent-${accent})`, "--av-delay": delay } as React.CSSProperties}
      onPointerMove={still ? undefined : (e) => svg.current && gaze(svg.current, e)}
      onPointerLeave={() => {
        svg.current?.style.removeProperty("--av-gx");
        svg.current?.style.removeProperty("--av-gy");
      }}
    >
      {props.title && <title>{props.title}</title>}
      <defs>
        <radialGradient id={shine} cx="0.32" cy="0.26" r="0.62">
          <stop offset="0" className="av-shine-0" />
          <stop offset="1" className="av-shine-1" />
        </radialGradient>
        <linearGradient id={shade} x1="0" y1="0" x2="0.35" y2="1">
          <stop offset="0.35" className="av-shade-0" />
          <stop offset="1" className="av-shade-1" />
        </linearGradient>
      </defs>
      <ellipse className="av-ground" cx="20" cy="36.2" rx="10.5" ry="1.7" />
      <g className="av-figure">
        <g className="av-move">
          {/* The side under the face, then the face, its shading and its light. */}
          <path className="av-side" d={body.d} transform="translate(0 2.2)" />
          <path className="av-face" d={body.d} />
          <path className="av-layer" d={body.d} fill={`url(#${shade})`} stroke={`url(#${shade})`} />
          <path className="av-layer" d={body.d} fill={`url(#${shine})`} stroke={`url(#${shine})`} />
          {one("l")}
          {one("r")}
        </g>
      </g>
    </svg>
  );
}

/** The model an agent runs on: what the server says, else its own chain's;
 * with no chain it runs on the server default, the only model its
 * `unavailable` can then name. */
export function agentMainModel(agent: Pick<Agent, "main_model" | "default_model" | "fallbacks" | "unavailable">): string | null {
  return agent.main_model ?? agent.default_model ?? ((agent.fallbacks ?? []).length ? null : (agent.unavailable?.[0] ?? null));
}

/** Why an agent is in trouble, or null: its main model is gone, whether or
 * not a fallback still answers in its place. */
export function agentAlert(agent: Pick<Agent, "name" | "main_model" | "default_model" | "fallbacks" | "unavailable">): string | null {
  const main = agentMainModel(agent);
  const gone = agent.unavailable ?? [];
  if (!main || !gone.includes(main)) return null;
  const next = (agent.fallbacks ?? []).find((f) => f !== main && !gone.includes(f));
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
    <AgentAvatar
      avatar={props.agent.avatar ?? defaultAvatar(props.agent.id)}
      state={props.state}
      size={size}
      title={props.title}
    />
  );
}
