// One routine on one page (docs/routines.md): its name, what runs (agent,
// model, chat or agentic with node and folder), when (schedules, or one
// webhook where the server takes them), the prompt with its variables, and
// the advanced choices. Next runs, the rendered prompt and template errors
// come from the server's preview, so no cron or template code lives here.
import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import {
  draftOf,
  draftSpec,
  draftTriggers,
  newDraft,
  type Draft,
  type Preview,
  type Routine,
  type RoutineList,
  type RoutineSpec,
  routinesApi,
  ranNow,
} from "../lib/routines";
import type { Agent, ModelInfo, NodeInfo } from "../types";
import { AgentMark } from "./AgentAvatar";
import type { RoutinesPageProps } from "./pages";
import { ScheduleBuilder } from "./ScheduleBuilder";
import { Seg } from "./Seg";
import { Select } from "./Select";
import { pickerGroups, VariablePicker } from "./VariablePicker";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
/** Typing settles before the server previews the prompt and schedules. */
const PREVIEW_MS = 400;

/** "Creates about 3 chats per day", or per week for rarer schedules. */
export function chatsNote(perDay: number | null): string | null {
  if (perDay === null || perDay <= 0) return null;
  if (perDay >= 1) return `Creates about ${Math.round(perDay)} ${Math.round(perDay) === 1 ? "chat" : "chats"} per day.`;
  const week = Math.round(perDay * 7);
  return week >= 1 ? `Creates about ${week} ${week === 1 ? "chat" : "chats"} per week.` : "Creates fewer than one chat a week.";
}

export function RoutineEditor(props: {
  list: RoutineList;
  /** null: a new routine. */
  routine: Routine | null;
  /** A template to start a new one from. */
  seed: Draft | null;
  agents: Agent[];
  nodes: NodeInfo[];
  scope: string | null;
  zone: string;
  /** The server takes webhooks (`features.routines.webhooks`). */
  webhooks: boolean;
  /** Secrets from the last save, by trigger id: shown once. */
  secrets: Record<string, string>;
  navigate: RoutinesPageProps["navigate"];
  onSaved: (r: Routine, secrets: Record<string, string>) => void;
  onDeleted: () => void;
}) {
  const { list, routine } = props;
  const firstAgent = props.agents[0]?.id ?? "general";
  const [draft, setDraft] = useState<Draft>(
    () => (routine ? draftOf(routine, Date.now()) : (props.seed ?? newDraft(firstAgent, props.zone, Date.now()))),
  );
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [preview, setPreview] = useState<Preview | null>(null);
  const prompt = useRef<HTMLTextAreaElement>(null);
  const workspaceId = list.workspace_id ?? props.scope ?? undefined;

  useEffect(() => {
    api
      .models(props.scope ? { workspace: props.scope } : null)
      .then((r) => setModels(r.models.filter((m) => !m.unavailable)))
      .catch(() => {});
  }, [props.scope]);

  const spec = draft.spec;
  const setSpec = (s: Partial<RoutineSpec>) => setDraft((d) => ({ ...d, spec: { ...d.spec, ...s } }));
  const triggers = useMemo(() => draftTriggers(draft), [draft]);

  // The server's preview of what this draft would do, after typing settles.
  const previewKey = JSON.stringify([spec.prompt, triggers]);
  useEffect(() => {
    const ctl = new AbortController();
    const t = setTimeout(() => {
      routinesApi
        .preview(
          {
            ...(workspaceId ? { workspace_id: workspaceId } : {}),
            ...(routine ? { routine_id: routine.routine_id } : {}),
            prompt: spec.prompt,
            triggers,
          },
          ctl.signal,
        )
        .then(setPreview)
        .catch(() => {});
    }, PREVIEW_MS);
    return () => {
      clearTimeout(t);
      ctl.abort();
    };
    // The key says everything the preview depends on.
  }, [previewKey]);

  const workers = props.nodes.filter((n) => n.profile === "agentic");
  const agenticWhy = list.can_agentic ? null : "You can't run agentic chats in this workspace.";
  const choice = props.webhooks || draft.when === "webhook";
  const groups = pickerGroups(list, draft.when, routine?.event_paths ?? []);
  const hook = routine?.triggers.find((t) => t.kind === "webhook");

  const save = async () => {
    setError(null);
    setNote(null);
    if (!draft.name.trim()) return setError("Give the routine a name.");
    if (spec.profile === "agentic" && (!spec.node_id || !spec.cwd?.trim())) return setError("Pick a node and a folder for an agentic routine.");
    if (draft.when === "schedule" && draft.schedules.length === 0) return setError("Add a schedule.");
    setBusy(true);
    try {
      const body = { name: draft.name.trim(), enabled: draft.enabled, spec: draftSpec(draft), triggers };
      const res = routine
        ? await routinesApi.update(routine.routine_id, body)
        : await routinesApi.create({ ...(workspaceId ? { workspace_id: workspaceId } : {}), ...body });
      setDraft(draftOf(res.routine, Date.now()));
      setNote("Saved.");
      props.onSaved(res.routine, res.secrets ?? {});
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!routine || !confirm(`Delete ${routine.name}? The chats of its runs stay.`)) return;
    try {
      await routinesApi.remove(routine.routine_id);
      props.onDeleted();
    } catch (e) {
      setError(message(e));
    }
  };

  return (
    <div className="routine-editor">
      <div className="agents-head">
        <h1>{routine ? routine.name : "New routine"}</h1>
        {routine && (
          <button
            type="button"
            onClick={() =>
              void routinesApi
                .run(routine.routine_id)
                .then((res) => setNote(ranNow(res)))
                .catch((e) => setError(message(e)))
            }
          >
            Run now
          </button>
        )}
      </div>
      {routine && <RoutineTabs id={routine.routine_id} tab="settings" navigate={props.navigate} />}
      <form
        className="card stack form"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <label>
          Name
          <input value={draft.name} maxLength={80} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="Morning briefing" />
        </label>

        <fieldset className="routine-fieldset stack">
          <legend>What runs</legend>
          <div className="field">
            Agent
            <Select
              label="Agent"
              value={spec.agent_id}
              options={props.agents.map((a) => ({ value: a.id, label: a.name, icon: <AgentMark agent={a} size={18} /> }))}
              onChange={(v) => setSpec({ agent_id: v })}
            />
          </div>
          <label>
            Model
            <select value={spec.model ?? ""} onChange={(e) => setSpec({ model: e.target.value || null })}>
              <option value="">Agent's default</option>
              {spec.model && !models.some((m) => m.id === spec.model) && <option value={spec.model}>{spec.model}</option>}
              {models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name ?? m.raw}
                </option>
              ))}
            </select>
          </label>
          <div className="field">
            Runs as
            <Seg
              label="Runs as"
              value={spec.profile}
              options={[
                { value: "chat", label: "Chat" },
                { value: "agentic", label: "Agentic", disabled: !!agenticWhy && spec.profile !== "agentic" },
              ]}
              onChange={(v) =>
                setSpec(v === "agentic" ? { profile: v, node_id: spec.node_id ?? workers.find((n) => n.connected)?.node_id ?? null } : { profile: v })
              }
            />
            {agenticWhy && <span className="field-hint">{agenticWhy}</span>}
          </div>
          {spec.profile === "agentic" && (
            <>
              <div className="field-row">
                <label className="grow">
                  Node
                  <select value={spec.node_id ?? ""} onChange={(e) => setSpec({ node_id: e.target.value || null })}>
                    <option value="">Pick a node</option>
                    {workers.map((n) => (
                      <option key={n.node_id} value={n.node_id}>
                        {n.embedded ? "local" : n.hostname}
                        {n.connected ? "" : " (offline)"}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="grow">
                  Folder on that node
                  <input value={spec.cwd ?? ""} onChange={(e) => setSpec({ cwd: e.target.value })} placeholder="/home/me/project" />
                </label>
              </div>
              <div className="field">
                Approvals
                <Seg
                  label="Approvals"
                  value={spec.approval_mode ?? "ask"}
                  options={[
                    { value: "ask", label: "Ask before changes" },
                    { value: "ask_all", label: "Ask before every tool" },
                  ]}
                  onChange={(v) => setSpec({ approval_mode: v })}
                />
                <span className="field-hint">
                  Asks before changing files or running commands. A run waiting for you is cancelled after 24 hours.
                </span>
              </div>
            </>
          )}
        </fieldset>

        <fieldset className="routine-fieldset stack">
          <legend>When</legend>
          {choice && (
            <Seg
              label="When it runs"
              value={draft.when}
              options={[
                { value: "schedule", label: "On a schedule" },
                { value: "webhook", label: "When a webhook is called", disabled: !props.webhooks && draft.when !== "webhook" },
              ]}
              onChange={(v) => setDraft({ ...draft, when: v })}
            />
          )}
          {draft.when === "schedule" ? (
            <ScheduleBuilder
              schedules={draft.schedules}
              onChange={(schedules) => setDraft((d) => ({ ...d, schedules }))}
              max={list.limits.max_triggers ?? 8}
              zone={props.zone}
              nextRuns={preview?.next_runs ?? []}
              chats={chatsNote(preview?.chats_per_day ?? null)}
            />
          ) : (
            <div className="stack routine-hook">
              <p className="muted small">
                Each call of its URL with the secret starts a run. What the call sends fills the prompt's webhook variables; that data
                is treated as untrusted.
              </p>
              {hook?.url_path ? (
                <div className="field">
                  URL
                  <code className="routine-code">{`${location.origin}${hook.url_path}`}</code>
                </div>
              ) : (
                <p className="muted small">The URL and its secret appear once you save.</p>
              )}
              {hook && props.secrets[hook.trigger_id] && (
                <div className="field">
                  Secret (shown once; send it as <code>Authorization: Bearer …</code>)
                  <code className="routine-code">{props.secrets[hook.trigger_id]}</code>
                </div>
              )}
            </div>
          )}
        </fieldset>

        <div className="field">
          <span className="row">
            <span className="grow">Prompt</span>
            <VariablePicker
              groups={groups}
              target={prompt}
              value={spec.prompt}
              onChange={(v) => setSpec({ prompt: v })}
            />
          </span>
          <textarea
            ref={prompt}
            className="routine-prompt"
            rows={6}
            maxLength={16000}
            aria-label="Prompt"
            value={spec.prompt}
            onChange={(e) => setSpec({ prompt: e.target.value })}
            placeholder="What should the agent do each time?"
          />
          {preview?.errors.map((x) => (
            <span key={`${x.offset}-${x.message}`} className="field-hint error-text">
              At character {x.offset + 1}: {x.message}
            </span>
          ))}
          {preview?.warnings.map((w) => (
            <span key={w} className="field-hint">
              {w}
            </span>
          ))}
          {preview?.rendered && (
            <details className="routine-rendered">
              <summary className="small muted">Preview of the next run's prompt</summary>
              <pre className="routine-code">{preview.rendered}</pre>
            </details>
          )}
        </div>

        <details className="routine-advanced">
          <summary>Advanced</summary>
          <div className="stack">
            <label>
              Missed runs (the server was off)
              <select value={spec.catch_up} onChange={(e) => setSpec({ catch_up: e.target.value as RoutineSpec["catch_up"] })}>
                <option value="skip">Skip them</option>
                <option value="once">Run once when back</option>
              </select>
            </label>
            <label>
              A variable with no value
              <select value={spec.on_missing} onChange={(e) => setSpec({ on_missing: e.target.value as RoutineSpec["on_missing"] })}>
                <option value="empty">Leave it empty</option>
                <option value="skip">Skip the run</option>
              </select>
            </label>
            <label>
              Notify me
              <select value={spec.notify} onChange={(e) => setSpec({ notify: e.target.value as RoutineSpec["notify"] })}>
                <option value="always">After every run</option>
                <option value="failures">When a run fails or the routine pauses</option>
                <option value="never">Never</option>
              </select>
            </label>
            <label>
              Longest run (minutes)
              <input
                type="number"
                min={1}
                max={1440}
                value={spec.max_runtime_min}
                onChange={(e) => setSpec({ max_runtime_min: Math.min(1440, Math.max(1, Number(e.target.value) || 1)) })}
              />
            </label>
          </div>
        </details>

        <label className="check">
          <input type="checkbox" checked={draft.enabled} onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })} />
          Turned on
        </label>
        {error && (
          <div className="error-text" role="alert">
            {error}
          </div>
        )}
        <div className="row end">
          {note && (
            <span className="muted" role="status">
              {note}
            </span>
          )}
          {routine && (
            <button type="button" className="danger" onClick={() => void remove()}>
              Delete
            </button>
          )}
          <button className="primary" disabled={busy}>
            {routine ? "Save" : "Create routine"}
          </button>
        </div>
      </form>
    </div>
  );
}

/** Settings | Runs, for a saved routine. */
export function RoutineTabs(props: { id: string; tab: "settings" | "runs"; navigate: RoutinesPageProps["navigate"] }) {
  const tabs: ["settings" | "runs", string][] = [
    ["settings", "Settings"],
    ["runs", "Runs"],
  ];
  return (
    <div className="tabs" role="tablist">
      {tabs.map(([x, label]) => (
        <button
          key={x}
          type="button"
          role="tab"
          aria-selected={props.tab === x}
          className={props.tab === x ? "on" : ""}
          onClick={() => props.navigate({ kind: "routines", id: props.id, ...(x === "runs" ? { tab: "runs" as const } : {}) })}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
