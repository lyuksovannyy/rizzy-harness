// A chat's step limit (X.14, README "Step and time limits"): how many model
// calls with tools one turn of this chat may make, from 1 up to its node's
// `max_steps`. One compact control beside the approval mode: the limit now,
// and a number field with its range stated; out-of-range input is refused
// with the reason before anything is sent. The node decides: it clamps
// whatever is sent, and the chat shows what it recorded.
import { useState } from "react";

/** Why `raw` cannot be sent, or null. */
export function stepError(raw: string, max: number): string | null {
  const t = raw.trim();
  if (!/^\d+$/.test(t)) return `A whole number from 1 to ${max}`;
  const n = Number(t);
  if (n < 1 || n > max) return `From 1 to ${max} (this node's max_steps)`;
  return null;
}

export function StepLimitPick(props: { value: number; max: number; onChange: (n: number) => Promise<unknown> | void }) {
  const [open, setOpen] = useState(false);
  const [raw, setRaw] = useState(String(props.value));
  const err = stepError(raw, props.max);
  const send = () => {
    if (err) return;
    void props.onChange(Number(raw.trim()));
    setOpen(false);
  };
  return (
    <span className="step-pick">
      <button
        type="button"
        className="pill"
        aria-expanded={open}
        title={`Model calls with tools one turn of this chat may make (this node allows up to ${props.max})`}
        onClick={() => {
          setRaw(String(props.value));
          setOpen(!open);
        }}
      >
        {props.value} steps
      </button>
      {open && (
        <span className="step-pop" role="group" aria-label="Step limit">
          <label>
            Steps per turn (1 to {props.max})
            <input
              type="text"
              inputMode="numeric"
              aria-label="Step limit"
              aria-invalid={!!err}
              value={raw}
              onChange={(e) => setRaw(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  send();
                } else if (e.key === "Escape") setOpen(false);
              }}
            />
          </label>
          <button type="button" className="pill" disabled={!!err} onClick={send}>
            Set
          </button>
          {err && <span className="step-err">{err}</span>}
        </span>
      )}
    </span>
  );
}

export default StepLimitPick;
