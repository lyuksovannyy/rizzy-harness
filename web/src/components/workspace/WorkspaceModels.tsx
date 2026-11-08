// Settings → Workspace admin → Models: which models this workspace's agents
// run on, with the same picker and chain editor as a provider's models. An
// agent without a chain here uses the server's; General's main model is the
// workspace's default model. A save sends the whole settings document, read
// again just before so that only the chains change (another admin's new
// default agent stays), and says what the server dropped as no longer known,
// if anything.
import { useEffect, useMemo, useState } from "react";
import { api } from "../../api";
import { modelLabel, type Plan, planProblems, resolveModel } from "../../lib/models";
import { chainsOf, dropped, workspacePlan } from "../../lib/workspaceAdmin";
import type { Agent, ProviderModel, WorkspaceDetail, WorkspaceSettings } from "../../types";
import { ModelPicker } from "../ModelPicker";
import { message, type WorkspacePanelProps } from "./panel";

/** The ids a plan uses, main models and fallbacks. */
const used = (p: Plan) => new Set(Object.values(p).flatMap((c) => (c.model ? [c.model, ...c.fallbacks] : c.fallbacks)));

export function WorkspaceModels(props: WorkspacePanelProps) {
  const { models, agents } = props;
  const id = props.workspace.workspace_id;
  const [detail, setDetail] = useState<WorkspaceDetail | null>(null);
  const [plan, setPlan] = useState<Plan>({});
  // Ticked in the picker, not given to an agent yet.
  const [loose, setLoose] = useState<string[]>([]);
  // The server's own chains, to say what an agent without one here uses.
  const [server, setServer] = useState<Agent[] | null>(null);
  const [lost, setLost] = useState<string[]>([]);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const label = (m: string) => {
    const info = resolveModel(models, m);
    return info ? modelLabel(info, models) : m;
  };
  const names = { agent: (a: string) => agents.find((x) => x.id === a)?.name ?? a, model: label };
  // The plan from what the server keeps, less what is no longer listed here
  // (lists not loaded yet leave everything in); and what either dropped.
  const show = (d: WorkspaceDetail, sent?: WorkspaceSettings) => {
    const p = workspacePlan(
      d.settings,
      (a) => !agents.length || agents.some((x) => x.id === a),
      (m) => !models.length || !!resolveModel(models, m),
    );
    const mine = { ...d.settings, agent_models: chainsOf(p) };
    setDetail(d);
    setPlan(p);
    setLoose([]);
    setLost([...new Set([...(sent ? dropped(sent, d.settings, names) : []), ...dropped(d.settings, mine, names)])]);
  };
  useEffect(() => {
    api
      .workspace(id)
      .then((d) => show(d))
      .catch((x) => setErr(message(x)));
    api
      .agents("server")
      .then((r) => setServer(r.agents))
      .catch(() => setServer([]));
  }, [id]);

  const inPlan = used(plan);
  const inPlanKey = [...inPlan].sort().join("\n");
  const selected = [...inPlan, ...loose.filter((m) => !inPlan.has(m))];
  // Listed models, and gone ones still in a chain (so they can be taken out).
  const offered: ProviderModel[] = useMemo(
    () =>
      models
        .filter((m) => !m.unavailable || inPlan.has(m.id))
        .map((m) => ({
          id: m.id,
          name: m.name,
          context: m.context,
          max_output: m.max_output,
          effort: m.effort,
          vision: m.vision,
          tools: m.tools,
          custom: false,
          unavailable: m.unavailable,
          missing_since: null,
        })),
    [models, inPlanKey],
  );
  if (!detail) return err ? <div className="error-text">{err}</div> : <div className="muted">Loading…</div>;

  const onPlan = (p: Plan) => {
    setPlan(p);
    setMsg(null);
    // A model given to an agent is no longer loose; one taken from every
    // agent is let go, so "Use server chain" leaves nothing to assign.
    setLoose((l) => l.filter((m) => !used(p).has(m)));
  };
  const note = (agent: string) => {
    const s = server?.find((x) => x.id === agent);
    const chain = s ? [s.main_model, ...s.fallbacks].filter((m): m is string => !!m) : [];
    return chain.length ? `Server chain: ${chain.map(label).join(" → ")}` : "Server chain";
  };
  const problems = planProblems(plan, selected, agents, label, true);
  const save = async () => {
    setBusy(true);
    setErr(null);
    setMsg(null);
    try {
      const fresh = (await api.workspace(id)).settings;
      const settings: WorkspaceSettings = { ...fresh, agent_models: chainsOf(plan) };
      show(await api.updateWorkspace(id, { settings }), settings);
      setMsg("Saved.");
      props.onChanged();
    } catch (x) {
      setErr(message(x));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="card stack">
      <p className="muted">
        Give models to this workspace's agents, as their main model or a fallback for when the main one fails. An agent
        without models here uses the server's chain. General's main model is this workspace's default model.
      </p>
      <ModelPicker
        label="Models"
        models={offered}
        selected={selected}
        onChange={(ids) => {
          setMsg(null);
          setLoose(ids.filter((m) => !inPlan.has(m)));
        }}
        agents={{
          agents,
          plan,
          onPlan,
          ref: (m) => m,
          label,
          unavailable: (m) => !!resolveModel(models, m)?.unavailable,
          inherit: { label: "Use server chain", note },
        }}
      />
      {lost.length > 0 && (
        <div className="field-hint" role="status">
          No longer known, so dropped: {lost.join(", ")}.
        </div>
      )}
      {err && <div className="error-text">{err}</div>}
      <div className="row end">
        {msg && <span className="muted">{msg}</span>}
        <button type="button" className="primary" disabled={busy || problems.length > 0} onClick={() => void save()}>
          Save
        </button>
      </div>
    </div>
  );
}
