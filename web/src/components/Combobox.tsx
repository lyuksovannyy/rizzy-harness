// A text field with a suggestion list (WAI-ARIA combobox): type to filter,
// ↑/↓ to move, Enter or click to pick, Esc to close. Free text stays
// possible unless the parent validates it.
import { type CSSProperties, type ReactNode, useEffect, useId, useRef, useState } from "react";
import { X } from "lucide-react";
import type { Suggestion, SuggestionIcon } from "../lib/geo";

/** An icon button at the input's end, before the clear button. */
export interface ComboAction {
  label: string;
  icon: ReactNode;
  onClick: () => void;
}

/** A suggestion's image (a flag), or its text when the image does not load. */
function Icon(props: { icon: SuggestionIcon }) {
  const [failed, setFailed] = useState<string | null>(null);
  if (failed === props.icon.src) return <span className="combo-icon-text">{props.icon.text}</span>;
  return (
    <img
      className="combo-icon"
      src={props.icon.src}
      width={20}
      height={15}
      alt=""
      loading="lazy"
      onError={() => setFailed(props.icon.src)}
    />
  );
}

export function Combobox(props: {
  value: string;
  onChange: (v: string) => void;
  suggest: (query: string) => Suggestion[];
  placeholder?: string;
  label: string;
  /** The input's id, for a `<label htmlFor>`; without it `label` names the input. */
  id?: string;
  /** Shown inside the input, before the text (the chosen country's flag). */
  icon?: SuggestionIcon;
  actions?: ComboAction[];
  /** Under the suggestions (a data source's attribution). */
  footer?: ReactNode;
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
  const actions: ComboAction[] = [...(props.actions ?? [])];
  if (props.value) {
    actions.push({ label: `Clear ${props.label.toLowerCase()}`, icon: <X size={14} />, onClick: () => props.onChange("") });
  }
  // The input leaves room for every button, so none covers the text.
  const style = { "--combo-actions": actions.length } as CSSProperties;
  return (
    <div className={`combo ${props.icon ? "has-icon" : ""}`} style={style}>
      {props.icon && (
        <span className="combo-lead" aria-hidden="true">
          <Icon icon={props.icon} />
        </span>
      )}
      <input
        id={props.id}
        role="combobox"
        aria-label={props.id ? undefined : props.label}
        aria-expanded={open && items.length > 0}
        aria-controls={`${id}-list`}
        aria-autocomplete="list"
        aria-activedescendant={open && items[active] ? `${id}-${active}` : undefined}
        aria-invalid={props.invalid || undefined}
        autoComplete="off"
        // No intrinsic width: the combo sizes it, so two fit side by side in a row.
        size={1}
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
      {actions.length > 0 && (
        <span className="combo-actions">
          {actions.map((a) => (
            <button
              key={a.label}
              type="button"
              className="icon"
              aria-label={a.label}
              title={a.label}
              onMouseDown={(e) => e.preventDefault()}
              onClick={a.onClick}
            >
              {a.icon}
            </button>
          ))}
        </span>
      )}
      {open && items.length > 0 && (
        <div className="combo-list">
          <ul role="listbox" id={`${id}-list`} aria-label={props.label}>
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
                <span className="option-label">
                  {s.icon && <Icon icon={s.icon} />}
                  {s.label}
                </span>
                {s.hint && <span className="option-hint">{s.hint}</span>}
              </li>
            ))}
          </ul>
          {props.footer && (
            <div className="combo-foot" onMouseDown={(e) => e.preventDefault()}>
              {props.footer}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
