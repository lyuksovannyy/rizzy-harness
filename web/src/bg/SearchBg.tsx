// Search mode: a magnifier that drops in from above the top edge, overshoots
// a little and settles, then floats and tilts slowly while a glint crosses
// the lens now and then. It is 2.5D for real: ten stacked copies of the
// glyph 3 px apart in a 3D context make its side face, so the tilt shows
// depth. All motion is CSS (search.css); this only places and builds it.
import { useEffect, useId, useRef, type CSSProperties } from "react";
import { placeMagnifier } from "./searchPlace";
import type { BgProps } from "./types";
import "./search.css";

const DEPTH = 10;
const ENTER_MS = 1000;

export default function SearchBg(p: BgProps & { typing: boolean }) {
  // useId's marks are not valid in url(#…) references.
  const id = useId().replace(/[^\w-]/g, "");
  const spot = placeMagnifier(p.zones);
  const onEntered = useRef(p.onEntered);
  onEntered.current = p.onEntered;
  const animated = p.motion === "animated";
  useEffect(() => {
    const t = setTimeout(() => onEntered.current(), animated ? ENTER_MS + p.delay : 0);
    return () => clearTimeout(t);
  }, [animated, p.delay]);
  if (!spot) return null;

  const [face, glass, glyph] = [`${id}f`, `${id}g`, `${id}s`];
  const vars = {
    "--mag-x": `${spot.x}px`,
    "--mag-y": `${spot.y}px`,
    "--mag-size": `${spot.size}px`,
    // Far enough up to start fully above the top edge.
    "--drop": `${-(spot.y + spot.size + 48)}px`,
    "--delay": `${p.delay}ms`,
  } as CSSProperties;
  return (
    <div className="mag" style={vars} data-motion={p.motion} data-state={p.state} data-typing={p.typing ? "" : undefined}>
      <svg className="mag-defs" width="0" height="0">
        <defs>
          <linearGradient id={face} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" style={{ stopColor: "var(--mag-hi)" }} />
            <stop offset="1" style={{ stopColor: "var(--mag-lo)" }} />
          </linearGradient>
          <radialGradient id={glass} cx=".38" cy=".34" r=".75">
            <stop offset="0" style={{ stopColor: "var(--fg)", stopOpacity: 0.06 }} />
            <stop offset="1" style={{ stopColor: "var(--fg)", stopOpacity: 0 }} />
          </radialGradient>
          <symbol id={glyph} viewBox="0 0 256 256">
            <circle cx="104" cy="104" r="64" fill="none" strokeWidth="20" />
            <rect x="-13" y="0" width="26" height="84" rx="13" transform="translate(150 150) rotate(-45)" />
          </symbol>
        </defs>
      </svg>
      <div className="mag-shadow" />
      <div className="mag-drop">
        <div className="mag-float">
          <div className="mag-tilt">
            {Array.from({ length: DEPTH }, (_, k) => DEPTH - 1 - k).map((i) => (
              <svg key={i} className="mag-layer" viewBox="0 0 256 256" style={{ "--i": i } as CSSProperties}>
                <use
                  href={`#${glyph}`}
                  style={i ? { fill: "var(--mag-side)", stroke: "var(--mag-side)" } : { fill: `url(#${face})`, stroke: `url(#${face})` }}
                />
                {i === 0 && (
                  <>
                    <circle cx="104" cy="104" r="54" fill={`url(#${glass})`} />
                    <circle className="mag-warm" cx="104" cy="104" r="54" />
                    <rect x="-15" y="-4" width="30" height="14" rx="4" transform="translate(146 146) rotate(-45)" style={{ fill: "var(--mag-hi)" }} />
                    <path d="M52 84 A56 56 0 0 1 84 50" fill="none" strokeWidth="3" strokeLinecap="round" style={{ stroke: "var(--mag-glint)" }} />
                  </>
                )}
              </svg>
            ))}
            <div className="mag-glint" />
          </div>
        </div>
      </div>
    </div>
  );
}
