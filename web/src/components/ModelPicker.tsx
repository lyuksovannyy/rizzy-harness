// Pick models out of a provider's listing: a search, pages of 100, select
// or deselect all of what the search shows, and a tag for each parameter
// the provider reported. One picker serves every form: several models
// (provider setup, Settings) or one (memory's embeddings model). It can
// list again on request, take models added by hand, and in multi mode
// give each chosen model to agents, as their main model or a fallback.
import {
  ArrowDown,
  ArrowUp,
  ArrowUpFromLine,
  Brain,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  Eye,
  Layers,
  Pencil,
  Plus,
  RefreshCw,
  SlidersHorizontal,
  Trash2,
  Wrench,
  X,
  type LucideIcon,
} from "lucide-react";
import { type RefObject, useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ago, dropModels, MAX_FALLBACKS, moveFallback, type Plan, planProblems, rolesOf, setMain, toggleFallback } from "../lib/models";
import { useAnchored } from "../md/anchored";
import type { Agent, ProviderModel } from "../types";
import { CustomModelForm, type TestTarget } from "./CustomModel";

export const PAGE_SIZE = 100;

/** 131072 -> "131k", 1048576 -> "1M". */
export function compactCount(n: number): string {
  if (n >= 1_000_000) return `${Math.round(n / 100_000) / 10}M`;
  if (n >= 1000) return `${Math.round(n / 1000)}k`;
  return String(n);
}

export type TagKind = "unavailable" | "custom" | "ctx" | "out" | "effort" | "vision" | "tools";
export interface ModelTag {
  kind: TagKind;
  text: string;
}

const TAG_ICON: Record<TagKind, LucideIcon> = {
  unavailable: CircleAlert,
  custom: SlidersHorizontal,
  ctx: Layers,
  out: ArrowUpFromLine,
  effort: Brain,
  vision: Eye,
  tools: Wrench,
};
const TAG_TITLE: Record<TagKind, string> = {
  unavailable: "The provider no longer lists this model",
  custom: "Added by hand",
  ctx: "Context window, in tokens",
  out: "Longest reply, in tokens",
  effort: "Takes a reasoning effort",
  vision: "Reads images",
  tools: "Calls tools",
};

/** What the provider reported, short, and how the model came to be here;
 * nothing for what it did not report. */
export function modelTags(m: ProviderModel): ModelTag[] {
  const out: ModelTag[] = [];
  if (m.unavailable && !m.custom) out.push({ kind: "unavailable", text: "unavailable" });
  if (m.custom) out.push({ kind: "custom", text: "custom" });
  if (m.context) out.push({ kind: "ctx", text: `${compactCount(m.context)} ctx` });
  if (m.max_output) out.push({ kind: "out", text: `${compactCount(m.max_output)} out` });
  if (m.effort) out.push({ kind: "effort", text: "effort" });
  if (m.vision) out.push({ kind: "vision", text: "vision" });
  if (m.tools) out.push({ kind: "tools", text: "tools" });
  return out;
}

export function ModelTags(props: { model: ProviderModel }) {
  const tags = modelTags(props.model);
  if (!tags.length) return null;
  return (
    <span className="model-tags">
      {tags.map((t) => {
        const Icon = TAG_ICON[t.kind];
        return (
          <span key={t.kind} className={`model-tag tag-${t.kind}`} title={TAG_TITLE[t.kind]}>
            <Icon size={11} aria-hidden />
            {t.text}
          </span>
        );
      })}
    </span>
  );
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
  /** Every agent's models, this picker's and other providers'. */
  plan: Plan;
  onPlan: (p: Plan) => void;
  /** The plan's id for one of this picker's models (a raw id). */
  ref: (raw: string) => string;
  /** A plan id as people read it. */
  label: (id: string) => string;
  /** Plan ids their provider no longer lists. */
  unavailable?: (id: string) => boolean;
  /** Agents may have no models in this plan and use a chain from elsewhere
   * (a workspace's agents use the server's). Every agent is listed then,
   * with what it uses (`note`) or a button (`label`) going back to it;
   * General needs no main model, and none is given one unasked. */
  inherit?: { label: string; note: (agent: string) => string };
}

export interface CustomModels {
  /** Adds a model set by hand, or replaces `was` (the id it had). */
  onSave: (m: ProviderModel, was: string | null) => void;
  onRemove: (id: string) => void;
  /** The provider a test goes to; null: not reachable yet. */
  test: (() => TestTarget) | null;
}

export function ModelPicker(props: {
  label: string;
  models: ProviderModel[];
  /** Raw ids. */
  selected: string[];
  onChange: (ids: string[]) => void;
  mode?: "multi" | "single";
  sort?: (a: ProviderModel, b: ProviderModel) => number;
  /** Multi mode: give chosen models to agents. */
  agents?: AgentAssign;
  /** Lets the admin add, edit and remove models by hand. */
  custom?: CustomModels;
  /** Lists the models again; the picker never does so by itself. */
  onRefresh?: () => void;
  refreshing?: boolean;
  /** When the listing was fetched (ms). */
  fetchedMs?: number | null;
  /** Drops a saved model its provider no longer lists (the admin's override). */
  onRemoveUnavailable?: (id: string) => void;
  /** Embeddings models: the custom form asks for no chat parameters. */
  embeddings?: boolean;
}) {
  const { models, selected, sort } = props;
  const single = props.mode === "single";
  const assign = single ? undefined : props.agents;
  const name = useId();
  const [q, setQ] = useState("");
  const [page, setPage] = useState(0);
  // The custom model form: "new", the id of the model edited, or closed.
  const [editing, setEditing] = useState<string | null>(null);
  // When the form closes, focus goes back to what opened it: the model's
  // pencil, or the add button.
  const addButton = useRef<HTMLButtonElement>(null);
  const pencils = useRef(new Map<string, HTMLButtonElement>());
  const back = useRef<string | null>(null);
  const closeForm = (to: string) => {
    back.current = to;
    setEditing(null);
  };
  useEffect(() => {
    if (editing !== null || back.current === null) return;
    (pencils.current.get(back.current) ?? addButton.current)?.focus();
    back.current = null;
  }, [editing]);
  const sorted = useMemo(() => (sort ? [...models].sort(sort) : models), [models, sort]);
  const shown = useMemo(() => filterModels(sorted, q), [sorted, q]);
  const pages = Math.max(1, Math.ceil(shown.length / PAGE_SIZE));
  const at = Math.min(page, pages - 1);
  const rows = shown.slice(at * PAGE_SIZE, (at + 1) * PAGE_SIZE);
  const picked = new Set(selected);

  // A model let go also leaves every agent it served.
  const change = (ids: string[]) => {
    if (assign) {
      const gone = new Set(selected.filter((x) => !ids.includes(x)).map(assign.ref));
      if (gone.size) assign.onPlan(dropModels(assign.plan, (id) => !gone.has(id)));
    }
    props.onChange(ids);
  };
  // The first model chosen starts as General's, the default for everyone.
  const general = (id: string | undefined) => {
    if (id && assign && !assign.inherit && !assign.plan.general?.model && assign.agents.some((a) => a.id === "general"))
      assign.onPlan(setMain(assign.plan, "general", assign.ref(id)));
  };
  const toggle = (id: string, on: boolean) => {
    if (single) return change(on ? [id] : []);
    if (on) general(id);
    change(on ? [...selected.filter((x) => x !== id), id] : selected.filter((x) => x !== id));
  };
  const all = (on: boolean) => {
    const ids = new Set(shown.map((m) => m.id));
    const rest = selected.filter((x) => !ids.has(x));
    if (on) general(shown[0]?.id);
    change(on ? [...rest, ...shown.map((m) => m.id)] : rest);
  };
  const edited = editing && editing !== "new" ? models.find((m) => m.id === editing) ?? null : null;

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
        {props.onRefresh && (
          <button
            type="button"
            className="icon"
            aria-label="Refresh model list"
            title="Refresh model list"
            // Off but focusable while it lists, so focus stays here.
            aria-disabled={props.refreshing || undefined}
            aria-busy={props.refreshing || undefined}
            onClick={() => {
              if (!props.refreshing) props.onRefresh?.();
            }}
          >
            <RefreshCw size={16} className={props.refreshing ? "spin" : undefined} />
          </button>
        )}
        {!single && (
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
        {!single && (
          <span aria-live="polite">
            {selected.length} of {models.length} selected
          </span>
        )}
        {props.fetchedMs != null && <span>Listed {ago(props.fetchedMs)}</span>}
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
        <div className="muted small">
          {models.length ? "No model matches." : props.custom ? "No models listed. Add one by hand below." : "No models listed."}
        </div>
      ) : (
        <ul>
          {rows.map((m) => {
            const on = picked.has(m.id);
            const gone = m.unavailable && !m.custom;
            return (
              <li key={m.id} className={`${on ? "on" : ""}${gone ? " gone" : ""}`}>
                <label className="check">
                  <input
                    type={single ? "radio" : "checkbox"}
                    name={single ? name : undefined}
                    checked={on}
                    onChange={(e) => toggle(m.id, e.target.checked)}
                  />
                  <span className="model-id">
                    <span className="mono">{m.id}</span>
                    {m.name && m.name !== m.id && <span className="muted small">{m.name}</span>}
                  </span>
                </label>
                <ModelTags model={m} />
                {m.custom && props.custom && (
                  <span className="model-actions">
                    <button
                      type="button"
                      className="icon"
                      aria-label={`Edit ${m.id}`}
                      ref={(el) => {
                        if (el) pencils.current.set(m.id, el);
                        else pencils.current.delete(m.id);
                      }}
                      onClick={() => setEditing(m.id)}
                    >
                      <Pencil size={14} />
                    </button>
                    <button type="button" className="icon" aria-label={`Remove ${m.id}`} onClick={() => props.custom!.onRemove(m.id)}>
                      <Trash2 size={14} />
                    </button>
                  </span>
                )}
                {gone && props.onRemoveUnavailable && (
                  <button
                    type="button"
                    className="danger small-button"
                    aria-label={`Remove ${m.id} from this provider and its agents`}
                    onClick={() => props.onRemoveUnavailable!(m.id)}
                  >
                    Remove
                  </button>
                )}
                {on && assign && <AgentMenu model={assign.ref(m.id)} label={m.id} a={assign} />}
              </li>
            );
          })}
        </ul>
      )}
      {props.custom &&
        (editing ? (
          <CustomModelForm
            key={editing}
            initial={edited}
            embeddings={props.embeddings}
            taken={(id) => id !== edited?.id && models.some((m) => m.id === id)}
            test={props.custom.test}
            onCancel={() => closeForm(edited?.id ?? "")}
            onSave={(m) => {
              props.custom!.onSave(m, edited?.id ?? null);
              closeForm(edited ? m.id : "");
            }}
          />
        ) : (
          <div>
            <button type="button" className="link small" ref={addButton} onClick={() => setEditing("new")}>
              <Plus size={14} /> Add custom model
            </button>
          </div>
        ))}
      {assign && <AgentPlan a={assign} selected={selected} />}
    </div>
  );
}

/** Which agents a chosen model serves, opening a menu to change it. */
function AgentMenu(props: { model: string; label: string; a: AgentAssign }) {
  const { model, a } = props;
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const roles = rolesOf(a.plan, model);
  const names = roles
    .map((r) => {
      const n = a.agents.find((x) => x.id === r.agent)?.name;
      return n && (r.role === "main" ? n : `${n} (fallback)`);
    })
    .filter(Boolean)
    .join(", ");
  const close = useCallback((refocus: boolean) => {
    setOpen(false);
    if (refocus) button.current?.focus();
  }, []);
  return (
    <>
      <button
        ref={button}
        type="button"
        className={`agent-pick${names ? "" : " loose"}`}
        aria-haspopup="menu"
        aria-expanded={open}
        // The visible text first, so it can be said to pick the button.
        aria-label={`${names ? `For: ${names}` : "Assign to agents…"} (${props.label})`}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="ellipsis">{names ? `For: ${names}` : "Assign to agents…"}</span>
        <ChevronDown size={14} />
      </button>
      {open && <AgentList model={model} label={props.label} a={a} anchor={button} onClose={close} />}
    </>
  );
}

function AgentList(props: {
  model: string;
  label: string;
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
      aria-label={`Agents for ${props.label}`}
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
        const c = a.plan[x.id];
        const main = c?.model === model;
        const fallback = !!c?.fallbacks.includes(model);
        // As many fallbacks as the server keeps; off but focusable for the arrow keys.
        const full = !fallback && (c?.fallbacks.length ?? 0) >= MAX_FALLBACKS;
        // Choosing Main here replaces the agent's current main model.
        const other = c?.model && !main ? a.label(c.model) : null;
        return (
          <div key={x.id} role="group" aria-label={x.name} className="agent-menu-row">
            <span className="agent-menu-name">
              <span>{x.name}</span>
              {other && <span className="muted small ellipsis">main: {other}</span>}
            </span>
            <button
              type="button"
              role="menuitemradio"
              aria-checked={main}
              aria-label={`${x.name}: main model`}
              onClick={() => a.onPlan(setMain(a.plan, x.id, main ? null : model))}
            >
              Main
            </button>
            <button
              type="button"
              role="menuitemcheckbox"
              aria-checked={fallback}
              aria-label={`${x.name}: fallback`}
              aria-disabled={full || undefined}
              title={full ? `${x.name} has ${MAX_FALLBACKS} fallbacks, the most it can have` : undefined}
              onClick={() => {
                if (!full) a.onPlan(toggleFallback(a.plan, x.id, model));
              }}
            >
              Fallback
            </button>
          </div>
        );
      })}
    </div>
  );
}

/** Each agent's main model and fallbacks, in order; fallbacks move up and
 * down and anything can be taken off. Then what still blocks saving. */
function AgentPlan(props: { a: AgentAssign; selected: string[] }) {
  const { a } = props;
  const plan = a.plan;
  const own = (agent: string) => !!(plan[agent]?.model || plan[agent]?.fallbacks.length);
  const rows = a.inherit ? a.agents : a.agents.filter((x) => x.id === "general" || own(x.id));
  const problems = planProblems(plan, props.selected.map(a.ref), a.agents, a.label, !!a.inherit);
  // A button that moves or takes its own model away would drop focus to the
  // page; each change names where focus goes next, first one there wins.
  const els = useRef(new Map<string, HTMLElement>());
  const next = useRef<string[] | null>(null);
  const reg = (k: string) => (el: HTMLElement | null) => {
    if (el) els.current.set(k, el);
    else els.current.delete(k);
  };
  useLayoutEffect(() => {
    const want = next.current;
    next.current = null;
    const el = want?.map((k) => els.current.get(k)).find((e) => e && !(e as HTMLButtonElement).disabled);
    el?.focus();
  }, [plan]);
  const change = (p: Plan, focus: string[]) => {
    next.current = [...focus, ""];
    a.onPlan(p);
  };
  const k = (agent: string, model: string, kind: "up" | "down" | "x") => `${agent}\n${model}\n${kind}`;
  const name = (id: string) => (
    <span className={`mono chain-model${a.unavailable?.(id) ? " gone" : ""}`}>
      {a.unavailable?.(id) && <CircleAlert size={12} aria-label="unavailable" />}
      {a.label(id)}
    </span>
  );
  return (
    <div className="agent-plan stack">
      <ul className="agent-chains" aria-label="Agents and their models" tabIndex={-1} ref={reg("")}>
        {rows.map((x) => {
          const c = plan[x.id] ?? { model: null, fallbacks: [] };
          const fb = c.fallbacks;
          if (a.inherit && !own(x.id)) {
            return (
              <li key={x.id} tabIndex={-1} ref={reg(x.id)}>
                <span className="agent-chain-name">{x.name}</span>
                <span className="muted small">{a.inherit.note(x.id)}</span>
              </li>
            );
          }
          return (
            <li key={x.id} tabIndex={-1} ref={reg(x.id)}>
              <span className="agent-chain-name">{x.name}</span>
              {c.model ? (
                <span className="chain-item">
                  {name(c.model)}
                  <button
                    type="button"
                    className="icon"
                    aria-label={`Remove ${a.label(c.model)} as ${x.name}'s main model`}
                    ref={reg(k(x.id, c.model, "x"))}
                    onClick={() => change(setMain(plan, x.id, null), [fb[0] ? k(x.id, fb[0], "x") : "", x.id])}
                  >
                    <X size={12} />
                  </button>
                </span>
              ) : (
                <span className="muted small">no main model</span>
              )}
              {c.fallbacks.length > 0 && (
                <ol className="chain-fallbacks" aria-label={`${x.name}'s fallbacks`}>
                  {c.fallbacks.map((f, i) => (
                    <li key={f}>
                      <span className="muted" aria-hidden>
                        →
                      </span>
                      <span className="chain-item">
                        {name(f)}
                        <button
                          type="button"
                          className="icon"
                          aria-label={`Move ${a.label(f)} up in ${x.name}'s fallbacks`}
                          disabled={i === 0}
                          ref={reg(k(x.id, f, "up"))}
                          onClick={() => change(moveFallback(plan, x.id, i, -1), [k(x.id, f, "up"), k(x.id, f, "down"), k(x.id, f, "x")])}
                        >
                          <ArrowUp size={12} />
                        </button>
                        <button
                          type="button"
                          className="icon"
                          aria-label={`Move ${a.label(f)} down in ${x.name}'s fallbacks`}
                          disabled={i === fb.length - 1}
                          ref={reg(k(x.id, f, "down"))}
                          onClick={() => change(moveFallback(plan, x.id, i, 1), [k(x.id, f, "down"), k(x.id, f, "up"), k(x.id, f, "x")])}
                        >
                          <ArrowDown size={12} />
                        </button>
                        <button
                          type="button"
                          className="icon"
                          aria-label={`Remove ${a.label(f)} from ${x.name}'s fallbacks`}
                          ref={reg(k(x.id, f, "x"))}
                          onClick={() =>
                            change(
                              toggleFallback(plan, x.id, f),
                              [fb[i + 1], fb[i - 1]].filter((m): m is string => !!m).map((m) => k(x.id, m, "x")).concat(c.model ? [k(x.id, c.model, "x")] : [], [x.id]),
                            )
                          }
                        >
                          <X size={12} />
                        </button>
                      </span>
                    </li>
                  ))}
                </ol>
              )}
              {a.inherit && (
                <button
                  type="button"
                  className="link small chain-inherit"
                  aria-label={`${x.name}: ${a.inherit.label}`}
                  onClick={() => change(Object.fromEntries(Object.entries(plan).filter(([id]) => id !== x.id)), [x.id])}
                >
                  {a.inherit.label}
                </button>
              )}
            </li>
          );
        })}
      </ul>
      {problems.length > 0 && (
        <div className="field-hint error-text" role="status">
          {problems.map((p) => (
            <div key={p}>{p}</div>
          ))}
        </div>
      )}
    </div>
  );
}
