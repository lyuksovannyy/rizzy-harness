// One agent's model chain on its page (X.50): the main model (or none of its
// own) and the fallbacks in order, for the person who may change it in the
// scope the page is opened in (lib/agentModels.ts says who and how it is
// saved). A small single-agent editor on the same Plan helpers the Settings
// pages use; ModelPicker is built around choosing models first and handing
// them to many agents, which is the wrong shape for one agent. A lazy chunk
// with the Agents page.
import { ArrowDown, ArrowUp, CircleAlert, X } from "lucide-react";
import { type ReactNode, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, type Scope } from "../api";
import {
  type ChainTarget,
  chainProblems,
  NO_CHAIN,
  sameChain,
  saveChain,
  serverChain,
  workspaceChain,
} from "../lib/agentModels";
import { t } from "../lib/i18n";
import "../lib/i18nPages";
import { modelLabel, rawModel, resolveModel } from "../lib/models";
import { type Chain, MAX_FALLBACKS, moveFallback, setMain, toggleFallback } from "../lib/modelPlan";
import type { Agent, ModelInfo } from "../types";

const message = (x: unknown) => (x instanceof Error ? x.message : String(x));

/** The models the page offers, from `GET /api/models` in the page's scope;
 * empty until loaded, and when it cannot be (the chain still shows). */
export function useModels(scope: Scope | null): ModelInfo[] {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const key = scope === "server" ? "server" : (scope?.workspace ?? "");
  useEffect(() => {
    let live = true;
    Promise.resolve()
      .then(() => api.models(scope))
      .then((r) => live && setModels(r?.models ?? []))
      .catch(() => live && setModels([]));
    return () => {
      live = false;
    };
    // The scope's object may be new each render; its value is the key.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return models;
}

/** A model as people read it; one the list no longer has is unavailable. */
export function modelNamer(models: ModelInfo[]): (id: string) => string {
  return (id) => {
    const info = resolveModel(models, id);
    if (info) return modelLabel(info, models);
    return models.length ? `${rawModel(id)} (${t("agentModels.unavailable")})` : rawModel(id);
  };
}

/** Main model and fallbacks of one agent; controlled. `inherit` words the
 * choice of having none of its own (null: a main model is required). */
export function ChainFields(props: {
  agent: { id: string; name: string };
  chain: Chain;
  onChange: (c: Chain) => void;
  models: ModelInfo[];
  inherit: string | null;
  problems: string[];
  disabled?: boolean;
}) {
  const { agent, chain: c, models, inherit } = props;
  const uid = useId();
  const label = useMemo(() => modelNamer(models), [models]);
  const gone = (id: string) => {
    const info = resolveModel(models, id);
    return info ? info.unavailable : models.length > 0;
  };
  const full = c.fallbacks.length >= MAX_FALLBACKS;
  const plan = { [agent.id]: c };
  const put = (p: typeof plan) => props.onChange(p[agent.id] ?? NO_CHAIN);

  // A button that moves or takes away its own model would drop focus to the
  // page: each change names where it goes next, the first enabled one wins.
  const els = useRef(new Map<string, HTMLElement>());
  const next = useRef<string[] | null>(null);
  const reg = (k: string) => (el: HTMLElement | null) => {
    if (el) els.current.set(k, el);
    else els.current.delete(k);
  };
  useLayoutEffect(() => {
    const want = next.current;
    next.current = null;
    want?.map((k) => els.current.get(k)).find((e) => e && !(e as HTMLButtonElement).disabled)?.focus();
  }, [c]);
  const go = (p: typeof plan, focus: string[]) => {
    next.current = focus;
    put(p);
  };
  const k = (m: string, kind: "up" | "down" | "x") => `${m}\n${kind}`;

  const mainOptions = models.filter((m) => !m.unavailable || m.id === c.model);
  const orphan = (id: string | null) => !!id && models.length > 0 && !resolveModel(models, id);
  const addable = models.filter((m) => !m.unavailable && m.id !== c.model && !c.fallbacks.includes(m.id));
  const off = !!props.disabled;

  return (
    <div className="stack agent-chain-editor">
      <div className="field">
        <label htmlFor={`${uid}-main`} className="field-label">
          {t("agentModels.main")}
        </label>
        <select
          id={`${uid}-main`}
          value={c.model ?? ""}
          disabled={off}
          onChange={(e) => {
            const id = e.target.value;
            // None of its own drops the whole chain, fallbacks with it.
            put(id ? setMain(plan, agent.id, id) : { [agent.id]: NO_CHAIN });
          }}
        >
          {inherit ? (
            <option value="">{inherit}</option>
          ) : (
            c.model === null && (
              <option value="" disabled>
                {t("agentModels.choose")}
              </option>
            )
          )}
          {orphan(c.model) && <option value={c.model!}>{label(c.model!)}</option>}
          {mainOptions.map((m) => (
            <option key={m.id} value={m.id}>
              {modelLabel(m, models)}
            </option>
          ))}
        </select>
      </div>
      <div className="field">
        <span className="field-label">{t("agentModels.fallbacks")}</span>
        {c.fallbacks.length > 0 && (
          <ol className="chain-fallbacks agent-fallbacks" aria-label={t("agentModels.fallbacks")}>
            {c.fallbacks.map((f, i) => (
              <li key={f}>
                <span className="muted small" aria-hidden>
                  {i + 1}.
                </span>
                <span className="chain-item">
                  <span className={`mono chain-model${gone(f) ? " gone" : ""}`} title={gone(f) ? t("agentModels.unavailable") : undefined}>
                    {gone(f) && <CircleAlert size={12} aria-label={t("agentModels.unavailable")} />}
                    {label(f)}
                  </span>
                  <button
                    type="button"
                    className="icon"
                    aria-label={t("agentModels.up", { model: label(f) })}
                    disabled={off || i === 0}
                    ref={reg(k(f, "up"))}
                    onClick={() => go(moveFallback(plan, agent.id, i, -1), [k(f, "up"), k(f, "down"), k(f, "x")])}
                  >
                    <ArrowUp size={12} />
                  </button>
                  <button
                    type="button"
                    className="icon"
                    aria-label={t("agentModels.down", { model: label(f) })}
                    disabled={off || i === c.fallbacks.length - 1}
                    ref={reg(k(f, "down"))}
                    onClick={() => go(moveFallback(plan, agent.id, i, 1), [k(f, "down"), k(f, "up"), k(f, "x")])}
                  >
                    <ArrowDown size={12} />
                  </button>
                  <button
                    type="button"
                    className="icon"
                    aria-label={t("agentModels.remove", { model: label(f) })}
                    disabled={off}
                    ref={reg(k(f, "x"))}
                    onClick={() => go(toggleFallback(plan, agent.id, f), [`${uid}-add`])}
                  >
                    <X size={12} />
                  </button>
                </span>
              </li>
            ))}
          </ol>
        )}
        <label htmlFor={`${uid}-add`} className="field-hint">
          {t("agentModels.add")}
        </label>
        <select
          id={`${uid}-add`}
          ref={reg(`${uid}-add`)}
          value=""
          disabled={off || !c.model || full}
          onChange={(e) => e.target.value && put(toggleFallback(plan, agent.id, e.target.value))}
        >
          <option value="">{t("agentModels.addPlaceholder")}</option>
          {addable.map((m) => (
            <option key={m.id} value={m.id}>
              {modelLabel(m, models)}
            </option>
          ))}
        </select>
        <p className="field-hint">
          {!c.model ? t("agentModels.needMain") : full ? t("agentModels.full", { name: agent.name, n: String(MAX_FALLBACKS) }) : t("agentModels.fallbacksHelp")}
        </p>
      </div>
      {props.problems.length > 0 && (
        <div className="field-hint error-text" role="status">
          {props.problems.map((p) => (
            <div key={p}>{p}</div>
          ))}
        </div>
      )}
    </div>
  );
}

/** The words of the choice of having none of its own, where `at` is written;
 * null: General at the server, which needs a main model. */
export function inheritLabel(at: ChainTarget["at"], agent: { id: string }): string | null {
  if (at === "workspace") return t("agentModels.useServer");
  return agent.id === "general" ? null : t("agentModels.useDefault");
}

/** The chain of an agent made in a workspace has no server chain to fall back
 * to: it runs on the default model. */
const ownInherit = (at: ChainTarget["at"], a: { id: string }, own: boolean) =>
  at === "workspace" && own ? t("agentModels.useDefault") : inheritLabel(at, a);

/** What the page that holds the editor may ask of it: whether it has a
 * change to save, whether that change may not be saved yet, and the save
 * itself (it throws; the page words the failure). */
export interface ChainHandle {
  dirty: boolean;
  invalid: boolean;
  commit: () => Promise<void>;
}

/** An agent's page: its chain, edited and saved on its own, or, with
 * `embedded`, saved by the page's own Save (`register` hands the page the
 * handle). The chain it starts from is read where it will be written: the
 * server's from a listing at the server's scope, a workspace's from its
 * settings document. The listing the page itself holds is in whatever scope
 * the app is in, so its chain is never the starting point. `readOnly` is
 * what the page shows when the chain cannot be read. */
export function AgentModelEditor(props: {
  agent: Agent;
  target: ChainTarget;
  scope: Scope;
  onSaved: () => void;
  readOnly: ReactNode;
  embedded?: boolean;
  register?: (h: ChainHandle | null) => void;
}) {
  const { agent: a, target } = props;
  const models = useModels(props.scope);
  const label = useMemo(() => modelNamer(models), [models]);
  const own = a.scope === "workspace";
  const wsId = target.at === "workspace" ? target.id : null;

  // The saved chain at the place it is written (null: not read yet), and the
  // server's own listing of the agent (the chain in effect where none is set).
  const [base, setBase] = useState<Chain | null>(null);
  const [serverOf, setServerOf] = useState<Agent | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [tries, setTries] = useState(0);
  useEffect(() => {
    let live = true;
    setLoadErr(null);
    const server = () =>
      Promise.resolve()
        .then(() => api.agents("server"))
        .then((r) => r?.agents.find((x) => x.id === a.id) ?? null);
    if (wsId) {
      workspaceChain(wsId, a.id).then(
        (c) => live && setBase(c),
        (x) => live && setLoadErr(message(x)),
      );
      // Only the note of what "use the server's chain" means: not needed to edit.
      if (!own) server().then((s) => live && setServerOf(s), () => live && setServerOf(null));
    } else {
      server().then(
        (s) => {
          if (!live) return;
          if (!s) return setLoadErr(t("agentModels.notListed"));
          setServerOf(s);
          setBase(serverChain(s));
        },
        (x) => live && setLoadErr(message(x)),
      );
    }
    return () => {
      live = false;
    };
  }, [wsId, a.id, own, tries]);
  const [edit, setEdit] = useState<Chain | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [lost, setLost] = useState<string[]>([]);
  const [err, setErr] = useState<string | null>(null);

  const shown = edit ?? base ?? NO_CHAIN;
  const problems = chainProblems(shown, a, target.at === "workspace", label);
  const dirty = base !== null && edit !== null && !sameChain(edit, base);

  const commit = async () => {
    const r = await saveChain(target, a.id, shown, label);
    setBase(r.chain);
    setEdit(null);
    setLost(r.lost);
    props.onSaved();
  };
  // The page's Save calls the latest commit, whatever the render it was handed over in.
  const latest = useRef(commit);
  latest.current = commit;
  const invalid = problems.length > 0;
  const { register } = props;
  useEffect(() => {
    if (!register || !props.embedded || !base) return;
    register({ dirty, invalid, commit: () => latest.current() });
    return () => register(null);
  }, [register, props.embedded, base, dirty, invalid]);

  if (!base) {
    if (!loadErr) return <div className="muted small">{t("agentModels.loading")}</div>;
    return (
      <div className="stack">
        {props.readOnly}
        <div className="error-text" role="alert">
          {t("agentModels.loadFailed", { why: loadErr })}
        </div>
        <div className="row">
          <button type="button" onClick={() => setTries(tries + 1)}>
            {t("agentModels.loadRetry")}
          </button>
        </div>
      </div>
    );
  }
  const inherit = ownInherit(target.at, a, own);

  const save = async () => {
    setBusy(true);
    setErr(null);
    setMsg(null);
    setLost([]);
    try {
      await commit();
      setMsg(t("agentModels.saved"));
    } catch (x) {
      setErr(message(x));
    } finally {
      setBusy(false);
    }
  };

  const serverList = serverOf ? [serverOf.main_model, ...serverOf.fallbacks].filter((m): m is string => !!m) : [];
  const serverNote = serverList.length ? t("agentModels.serverChain", { list: serverList.map(label).join(" → ") }) : t("agentModels.serverChainBare");
  const inEffect = !wsId
    ? base.model
      ? null
      : serverOf?.main_model
        ? t("agentModels.notSet", { model: label(serverOf.main_model) })
        : t("agentModels.notSetNoModel")
    : base.model
      ? t("agentModels.wsOwn")
      : own
        ? t("agentModels.wsDefault")
        : t("agentModels.wsServer");

  return (
    <div className="stack agent-chain-editor-box">
      <ChainFields
        agent={a}
        chain={shown}
        onChange={(c) => {
          setMsg(null);
          setEdit(c);
        }}
        models={models}
        inherit={inherit}
        problems={problems}
        disabled={busy}
      />
      {inEffect && <p className="field-hint">{inEffect}</p>}
      {wsId && !own && !shown.model && <p className="field-hint">{serverNote}</p>}
      {lost.length > 0 && (
        <div className="field-hint" role="status">
          {t("agentModels.dropped", { list: lost.join(", ") })}
        </div>
      )}
      {err && <div className="error-text">{err}</div>}
      {!props.embedded && (
        <div className="row end">
          {msg && (
            <span className="muted small" role="status">
              {msg}
            </span>
          )}
          <button type="button" className="primary" disabled={busy || !dirty || problems.length > 0} onClick={() => void save()}>
            {t("agentModels.save")}
          </button>
        </div>
      )}
    </div>
  );
}

/** A new agent's chain, held by the page that makes the agent. */
export function NewAgentModels(props: {
  agent: { id: string; name: string };
  target: ChainTarget;
  scope: Scope;
  chain: Chain;
  onChange: (c: Chain) => void;
  disabled?: boolean;
}) {
  const models = useModels(props.scope);
  const label = useMemo(() => modelNamer(models), [models]);
  return (
    <ChainFields
      agent={props.agent}
      chain={props.chain}
      onChange={props.onChange}
      models={models}
      inherit={ownInherit(props.target.at, props.agent, props.target.at === "workspace")}
      problems={chainProblems(props.chain, props.agent, props.target.at === "workspace", label)}
      disabled={props.disabled}
    />
  );
}
