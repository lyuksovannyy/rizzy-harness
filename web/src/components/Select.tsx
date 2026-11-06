// A dropdown whose options have a secondary line (e.g. an engine's
// abilities). Keyboard: Enter/Space/↓ open, ↑/↓ move, Enter picks, Esc closes.
import { useEffect, useId, useRef, useState } from "react";
import { ChevronDown } from "lucide-react";

export interface SelectOption {
  value: string;
  label: string;
  hint?: string;
}

export function Select(props: {
  label: string;
  value: string;
  options: SelectOption[];
  onChange: (v: string) => void;
  compact?: boolean;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const current = props.options.find((o) => o.value === props.value);
  useEffect(() => {
    if (!open) return;
    setActive(Math.max(0, props.options.findIndex((o) => o.value === props.value)));
    const close = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open, props.options, props.value]);
  const pick = (o: SelectOption) => {
    props.onChange(o.value);
    setOpen(false);
  };
  return (
    <div className={`select ${props.compact ? "compact" : ""}`} ref={root}>
      <button
        type="button"
        className="select-button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={`${id}-list`}
        aria-label={`${props.label}: ${current?.label ?? "none"}`}
        onClick={() => setOpen((v) => !v)}
        onKeyDown={(e) => {
          if (!open && (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ")) {
            e.preventDefault();
            setOpen(true);
          } else if (open && e.key === "ArrowDown") {
            e.preventDefault();
            setActive((a) => Math.min(a + 1, props.options.length - 1));
          } else if (open && e.key === "ArrowUp") {
            e.preventDefault();
            setActive((a) => Math.max(a - 1, 0));
          } else if (open && (e.key === "Enter" || e.key === " ")) {
            e.preventDefault();
            const o = props.options[active];
            if (o) pick(o);
          } else if (e.key === "Escape") {
            setOpen(false);
          }
        }}
      >
        <span className="ellipsis">{current?.label ?? "Choose…"}</span>
        {current?.hint && !props.compact && <span className="option-hint">{current.hint}</span>}
        <ChevronDown size={14} />
      </button>
      {open && (
        <ul className="select-list" role="listbox" id={`${id}-list`} aria-label={props.label}>
          {props.options.map((o, i) => (
            <li
              key={o.value}
              role="option"
              aria-selected={o.value === props.value}
              className={`${i === active ? "on" : ""} ${o.value === props.value ? "chosen" : ""}`}
              onMouseEnter={() => setActive(i)}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => pick(o)}
            >
              <span>{o.label}</span>
              {o.hint && <span className="option-hint">{o.hint}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
