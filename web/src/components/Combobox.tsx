// A text field with a suggestion list (WAI-ARIA combobox): type to filter,
// ↑/↓ to move, Enter or click to pick, Esc to close. Free text stays
// possible unless the parent validates it.
import { useEffect, useId, useRef, useState } from "react";
import { X } from "lucide-react";
import type { Suggestion } from "../lib/geo";

export function Combobox(props: {
  value: string;
  onChange: (v: string) => void;
  suggest: (query: string) => Suggestion[];
  placeholder?: string;
  label: string;
  /** Called once on first focus (e.g. to load data). */
  onFirstFocus?: () => void;
  invalid?: boolean;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const focused = useRef(false);
  const items = open ? props.suggest(props.value) : [];
  useEffect(() => setActive(0), [props.value]);
  const pick = (s: Suggestion) => {
    props.onChange(s.value);
    setOpen(false);
  };
  return (
    <div className="combo">
      <input
        role="combobox"
        aria-label={props.label}
        aria-expanded={open && items.length > 0}
        aria-controls={`${id}-list`}
        aria-autocomplete="list"
        aria-activedescendant={open && items[active] ? `${id}-${active}` : undefined}
        aria-invalid={props.invalid || undefined}
        autoComplete="off"
        value={props.value}
        placeholder={props.placeholder}
        onFocus={() => {
          if (!focused.current) {
            focused.current = true;
            props.onFirstFocus?.();
          }
          setOpen(true);
        }}
        onBlur={() => setTimeout(() => setOpen(false), 120)}
        onChange={(e) => {
          props.onChange(e.target.value);
          setOpen(true);
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setOpen(true);
            setActive((a) => Math.min(a + 1, Math.max(items.length - 1, 0)));
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setActive((a) => Math.max(a - 1, 0));
          } else if (e.key === "Enter" && open && items[active]) {
            e.preventDefault();
            pick(items[active]);
          } else if (e.key === "Escape") {
            setOpen(false);
          }
        }}
      />
      {props.value && (
        <button
          type="button"
          className="icon combo-clear"
          aria-label={`Clear ${props.label.toLowerCase()}`}
          title="Clear"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => props.onChange("")}
        >
          <X size={14} />
        </button>
      )}
      {open && items.length > 0 && (
        <ul className="combo-list" role="listbox" id={`${id}-list`}>
          {items.map((s, i) => (
            <li
              key={s.value + i}
              id={`${id}-${i}`}
              role="option"
              aria-selected={i === active}
              className={i === active ? "on" : ""}
              onMouseDown={(e) => e.preventDefault()}
              onMouseEnter={() => setActive(i)}
              onClick={() => pick(s)}
            >
              <span>{s.label}</span>
              {s.hint && <span className="muted small">{s.hint}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
