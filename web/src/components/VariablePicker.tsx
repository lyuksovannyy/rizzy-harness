// The { } menu beside a routine's prompt (docs/routines.md): the variables
// this routine can use, grouped by namespace, inserted as `{{ path }}` where
// the cursor is. Only what the server lists for this routine shows: a
// schedule routine has no webhook data, so no `event.*`.
import { Braces } from "lucide-react";
import { type RefObject, useEffect, useId, useRef, useState } from "react";
import type { Draft, RoutineList, VarSpec } from "../lib/routines";

export interface VarGroup {
  label: string;
  vars: VarSpec[];
}

const NAMESPACE: Record<string, string> = {
  now: "Time",
  routine: "Routine",
  run: "This run",
  user: "You",
  workspace: "Workspace",
  last_run: "Last run",
};

/** The groups a routine's picker lists: the server's namespaces, and for a
 * webhook routine what its source offers plus the paths of the last body. */
export function pickerGroups(list: Pick<RoutineList, "variables" | "sources">, when: Draft["when"], eventPaths: string[]): VarGroup[] {
  const groups = list.variables
    .filter((g) => g.namespace !== "event" && g.vars.length > 0)
    .map((g) => ({ label: NAMESPACE[g.namespace] ?? g.namespace, vars: g.vars }));
  if (when !== "webhook") return groups;
  const source = list.sources.find((s) => s.kind === "webhook");
  const seen = new Set((source?.vars ?? []).map((v) => v.path));
  // The same path can come from the sample and from the last call: once.
  const fromBody = [...new Set(eventPaths.map((p) => (p.startsWith("event.") ? p : `${source?.free_prefix ?? "event.body"}.${p}`)))]
    .filter((p) => !seen.has(p))
    .map((path) => ({ path, example: "" }));
  const vars = [...(source?.vars ?? []), ...fromBody];
  return vars.length ? [...groups, { label: "From the webhook", vars }] : groups;
}

/** `token` in place of the selection; the caret goes after it. */
export function insertAt(value: string, start: number, end: number, token: string): { value: string; caret: number } {
  return { value: value.slice(0, start) + token + value.slice(end), caret: start + token.length };
}

export function VariablePicker(props: {
  groups: VarGroup[];
  target: RefObject<HTMLTextAreaElement | null>;
  value: string;
  onChange: (v: string) => void;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const menu = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  // Where the caret goes once React has put the new text in.
  const caret = useRef<number | null>(null);

  useEffect(() => {
    if (caret.current === null) return;
    const ta = props.target.current;
    if (ta) {
      ta.focus();
      ta.setSelectionRange(caret.current, caret.current);
    }
    caret.current = null;
  }, [props.value, props.target]);

  useEffect(() => {
    if (!open) return;
    menu.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    const close = (e: MouseEvent) => {
      if (!menu.current?.contains(e.target as Node) && e.target !== button.current) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  const insert = (path: string) => {
    const ta = props.target.current;
    const start = ta?.selectionStart ?? props.value.length;
    const end = ta?.selectionEnd ?? start;
    const next = insertAt(props.value, start, end, `{{ ${path} }}`);
    caret.current = next.caret;
    setOpen(false);
    props.onChange(next.value);
  };

  const keys = (e: React.KeyboardEvent) => {
    const items = [...(menu.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const at = items.indexOf(document.activeElement as HTMLElement);
    const go = (i: number) => items[(i + items.length) % items.length]?.focus();
    if (e.key === "ArrowDown") go(at + 1);
    else if (e.key === "ArrowUp") go(at - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(items.length - 1);
    else if (e.key === "Escape") {
      setOpen(false);
      props.target.current?.focus();
    } else if (e.key === "Tab") setOpen(false);
    else return;
    if (e.key !== "Tab") e.preventDefault();
  };

  if (props.groups.length === 0) return null;
  return (
    <span className="var-picker">
      <button
        ref={button}
        type="button"
        className="icon"
        aria-label="Insert a variable"
        title="Insert a variable"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? `${id}-menu` : undefined}
        onClick={() => setOpen((o) => !o)}
      >
        <Braces size={16} />
      </button>
      {open && (
        <div ref={menu} id={`${id}-menu`} role="menu" aria-label="Variables" className="var-menu" onKeyDown={keys}>
          {props.groups.map((g) => (
            <div key={g.label} role="group" aria-label={g.label}>
              <div className="var-group small muted" aria-hidden="true">
                {g.label}
              </div>
              {g.vars.map((v) => (
                <button key={v.path} type="button" role="menuitem" tabIndex={-1} onClick={() => insert(v.path)}>
                  <span className="mono">{`{{ ${v.path} }}`}</span>
                  {v.example && <span className="muted small ellipsis">{v.example}</span>}
                </button>
              ))}
            </div>
          ))}
        </div>
      )}
    </span>
  );
}
