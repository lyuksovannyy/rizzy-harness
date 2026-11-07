// Pick models out of a provider's listing: a search, pages of 100, select
// or deselect all of what the search shows, and the parameters the provider
// reported as chips. In multi mode a chosen model can be made the default
// of some agents (moving them off any other model).
import { Check, ChevronDown, ChevronLeft, ChevronRight } from "lucide-react";
import { type RefObject, useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { useAnchored } from "../md/anchored";
import type { Agent, ProviderModel } from "../types";

export const PAGE_SIZE = 100;

/** 131072 -> "131k", 1048576 -> "1M". */
export function compactCount(n: number): string {
  if (n >= 1_000_000) return `${Math.round(n / 100_000) / 10}M`;
  if (n >= 1000) return `${Math.round(n / 1000)}k`;
  return String(n);
}

/** What the provider reported, short; nothing for what it did not. */
export function modelChips(m: ProviderModel): string[] {
  const out: string[] = [];
  if (m.context) out.push(`${compactCount(m.context)} ctx`);
  if (m.max_output) out.push(`${compactCount(m.max_output)} out`);
  if (m.effort) out.push("effort");
  if (m.vision) out.push("vision");
  if (m.tools) out.push("tools");
  return out;
}

/** Models whose id or name holds every word of `q`. */
export function filterModels(models: ProviderModel[], q: string): ProviderModel[] {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return models;
  return models.filter((m) => {
    const hay = `${m.id} ${m.name ?? ""}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

export interface AgentAssign {
  agents: Agent[];
  /** Agent id to the raw id of its default model (this provider's). */
  assigned: Record<string, string>;
  /** null takes the agent off `model`. */
  onAssign: (agent: string, model: string | null) => void;
}

export function ModelPicker(props: {
  label: string;
  models: ProviderModel[];
  /** Raw ids. */
  selected: string[];
  onChange: (ids: string[]) => void;
  single?: boolean;
  sort?: (a: ProviderModel, b: ProviderModel) => number;
  agents?: AgentAssign;
}) {
  const { models, selected, sort } = props;
  const name = useId();
  const [q, setQ] = useState("");
  const [page, setPage] = useState(0);
  const sorted = useMemo(() => (sort ? [...models].sort(sort) : models), [models, sort]);
  const shown = useMemo(() => filterModels(sorted, q), [sorted, q]);
  const pages = Math.max(1, Math.ceil(shown.length / PAGE_SIZE));
  const at = Math.min(page, pages - 1);
  const rows = shown.slice(at * PAGE_SIZE, (at + 1) * PAGE_SIZE);
  const picked = new Set(selected);

  const toggle = (id: string, on: boolean) => {
    if (props.single) props.onChange(on ? [id] : []);
    else props.onChange(on ? [...selected.filter((x) => x !== id), id] : selected.filter((x) => x !== id));
  };
  const all = (on: boolean) => {
    const ids = new Set(shown.map((m) => m.id));
    const rest = selected.filter((x) => !ids.has(x));
    props.onChange(on ? [...rest, ...shown.map((m) => m.id)] : rest);
  };

  return (
    <div className="model-pick stack" role="group" aria-label={props.label}>
      <div className="row">
        <input
          className="grow"
          type="search"
          aria-label="Search models"
          placeholder={`Search ${models.length} ${models.length === 1 ? "model" : "models"}`}
          value={q}
          // A filter box never submits the form around it.
          onKeyDown={(e) => {
            if (e.key === "Enter") e.preventDefault();
          }}
          onChange={(e) => {
            setQ(e.target.value);
            setPage(0);
          }}
        />
        {!props.single && (
          <>
            <button type="button" onClick={() => all(true)} disabled={!shown.length}>
              Select all
            </button>
            <button type="button" onClick={() => all(false)} disabled={!shown.some((m) => picked.has(m.id))}>
              Deselect all
            </button>
          </>
        )}
      </div>
      <div className="row muted small">
        {!props.single && (
          <span aria-live="polite">
            {selected.length} of {models.length} selected
          </span>
        )}
        <span className="spacer" />
        {pages > 1 && (
          <span className="row pager">
            <button type="button" className="icon" aria-label="Previous page" disabled={at === 0} onClick={() => setPage(at - 1)}>
              <ChevronLeft size={16} />
            </button>
            <span>
              {at * PAGE_SIZE + 1}–{at * PAGE_SIZE + rows.length} of {shown.length}
            </span>
            <button type="button" className="icon" aria-label="Next page" disabled={at >= pages - 1} onClick={() => setPage(at + 1)}>
              <ChevronRight size={16} />
            </button>
          </span>
        )}
      </div>
      {rows.length === 0 ? (
        <div className="muted small">{models.length ? "No model matches." : "No models listed."}</div>
      ) : (
        <ul>
          {rows.map((m) => {
            const on = picked.has(m.id);
            const chips = modelChips(m);
            return (
              <li key={m.id} className={on ? "on" : ""}>
                <label className="check">
                  <input
                    type={props.single ? "radio" : "checkbox"}
                    name={props.single ? name : undefined}
                    checked={on}
                    onChange={(e) => toggle(m.id, e.target.checked)}
                  />
                  <span className="model-id">
                    <span className="mono">{m.id}</span>
                    {m.name && m.name !== m.id && <span className="muted small">{m.name}</span>}
                  </span>
                </label>
                {chips.length > 0 && (
                  <span className="model-chips">
                    {chips.map((c) => (
                      <span key={c} className="model-chip">
                        {c}
                      </span>
                    ))}
                  </span>
                )}
                {on && props.agents && !props.single && <AgentMenu model={m.id} a={props.agents} />}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

/** "Default for: General, Coder", opening a menu of agents to check. */
function AgentMenu(props: { model: string; a: AgentAssign }) {
  const { model, a } = props;
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const mine = a.agents.filter((x) => a.assigned[x.id] === model);
  const close = useCallback((refocus: boolean) => {
    setOpen(false);
    if (refocus) button.current?.focus();
  }, []);
  return (
    <>
      <button
        ref={button}
        type="button"
        className="agent-pick"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={`Default agents for ${model}: ${mine.map((x) => x.name).join(", ") || "none"}`}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="ellipsis">{mine.length ? `Default for: ${mine.map((x) => x.name).join(", ")}` : "Default for…"}</span>
        <ChevronDown size={14} />
      </button>
      {open && (
        <AgentList
          model={model}
          a={a}
          anchor={button}
          onClose={close}
        />
      )}
    </>
  );
}

function AgentList(props: {
  model: string;
  a: AgentAssign;
  anchor: RefObject<HTMLButtonElement | null>;
  onClose: (refocus: boolean) => void;
}) {
  const { model, a, anchor, onClose } = props;
  const box = useRef<HTMLDivElement>(null);
  useAnchored(box, anchor);
  useEffect(() => {
    box.current?.querySelector("button")?.focus();
    const onDown = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node) && !anchor.current?.contains(e.target as Node)) onClose(false);
    };
    document.addEventListener("mousedown", onDown);
    // The menu sits in the top layer; once the list scrolls its button
    // away it would float over whatever is there, so it closes.
    const list = anchor.current?.closest("ul");
    const onScroll = () => onClose(false);
    list?.addEventListener("scroll", onScroll);
    return () => {
      document.removeEventListener("mousedown", onDown);
      list?.removeEventListener("scroll", onScroll);
    };
  }, [anchor, onClose]);
  return (
    <div
      ref={box}
      className="agent-menu"
      popover="manual"
      role="menu"
      aria-label={`Default agents for ${model}`}
      onKeyDown={(e) => {
        const items = [...(box.current?.querySelectorAll("button") ?? [])];
        const i = items.indexOf(document.activeElement as HTMLButtonElement);
        if (e.key === "Escape" || e.key === "Tab") {
          e.preventDefault();
          onClose(true);
        } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          items[(i + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus();
        }
      }}
    >
      {a.agents.map((x) => {
        const on = a.assigned[x.id] === model;
        return (
          <button
            key={x.id}
            type="button"
            role="menuitemcheckbox"
            aria-checked={on}
            onClick={() => a.onAssign(x.id, on ? null : model)}
          >
            <span>{x.name}</span>
            {on && <Check size={14} />}
          </button>
        );
      })}
    </div>
  );
}
