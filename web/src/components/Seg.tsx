// A segmented switch: one of a few choices, as a radiogroup of buttons
// (one Tab stop, arrow keys move; lib/radios.ts). Every option is
// type="button", so the switch can sit in a form.
import type { ReactNode } from "react";
import { radioKeys, radioTab } from "../lib/radios";

export interface SegOption<T extends string> {
  value: T;
  label: ReactNode;
  disabled?: boolean;
}

export function Seg<T extends string>(props: {
  label: string;
  value: T;
  options: SegOption<T>[];
  onChange: (v: T) => void;
  /** Lets many options wrap onto more lines. */
  wrap?: boolean;
}) {
  const any = props.options.some((o) => o.value === props.value);
  return (
    <div className={props.wrap ? "seg wrap" : "seg"} role="radiogroup" aria-label={props.label} onKeyDown={radioKeys}>
      {props.options.map((o, i) => {
        const on = o.value === props.value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={on}
            tabIndex={radioTab(on || (!any && i === 0))}
            className={on ? "on" : ""}
            disabled={o.disabled}
            onClick={() => props.onChange(o.value)}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
