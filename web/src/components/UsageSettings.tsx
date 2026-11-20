// Settings → Server → Usage (ADOPTION_PLAN X.18, X.39): what each person
// used in a period, per model, with the cost by the prices set for the
// models. Numbers only; nothing about any one chat is ever sent here. Server
// admins and the API token. Limits and alerts are not built.
import "../lib/i18nPages";
import { useEffect, useMemo, useState } from "react";
import { t } from "../lib/i18n";
import {
  type PresetId,
  type PricedGroup,
  type SortBy,
  type UsageAnswer,
  preset,
  price,
  usageApi,
} from "../lib/usageTotals";
import type { ModelInfo } from "../types";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
const PRESETS: PresetId[] = ["month", "lastMonth", "days30"];
const n = (v: number) => v.toLocaleString();

function Table(props: { title: string; group: PricedGroup }) {
  const { group: g } = props;
  return (
    <div className="card stack usage-group">
      <h4>{props.title}</h4>
      <table className="users usage-table">
        <thead>
          <tr>
            <th>{t("usage.model")}</th>
            <th>{t("usage.input")}</th>
            <th>{t("usage.output")}</th>
            <th>{t("usage.cached")}</th>
            <th>{t("usage.turns")}</th>
            <th>{t("usage.cost")}</th>
          </tr>
        </thead>
        <tbody>
          {g.models.map((m) => (
            <tr key={m.row.model}>
              <td>{m.row.model}</td>
              <td>{n(m.row.input_tokens)}</td>
              <td>{n(m.row.output_tokens)}</td>
              <td>{n(m.row.cache_read_tokens)}</td>
              <td>{n(m.row.turns)}</td>
              <td>{m.cost ? <span title={m.cost.hint}>{m.cost.text}</span> : "–"}</td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr>
            <th>{t("usage.total")}</th>
            <td>{n(g.source.input_tokens)}</td>
            <td>{n(g.source.output_tokens)}</td>
            <td>{n(g.source.cache_read_tokens)}</td>
            <td>{n(g.source.turns)}</td>
            <td>{g.cost ? <span title={g.cost.hint}>{g.cost.text}</span> : "–"}</td>
          </tr>
        </tfoot>
      </table>
    </div>
  );
}

export function UsageSettings(props: { models: ModelInfo[] }) {
  const [id, setId] = useState<PresetId>("month");
  const [sort, setSort] = useState<SortBy>("cost");
  const [data, setData] = useState<UsageAnswer | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setData(null);
    setError(null);
    const p = preset(id);
    usageApi
      .get(p.from, p.to)
      .then((d) => live && setData(d))
      .catch((e) => live && setError(message(e)));
    return () => {
      live = false;
    };
  }, [id]);
  const people = useMemo(() => (data ? price(data.people, props.models, sort) : []), [data, props.models, sort]);
  const spaces = useMemo(() => (data ? price(data.workspaces, props.models, sort) : []), [data, props.models, sort]);
  const range = data
    ? `${new Date(data.from).toLocaleDateString()} – ${new Date(data.to - 1).toLocaleDateString()}`
    : "";
  // Any group with an unpriced part: say once what to do about it.
  const unpriced = [...people, ...spaces].some((g) => g.unpriced);
  return (
    <div className="stack usage-page">
      <div className="card stack">
        <h3>{t("usage.title")}</h3>
        <p className="muted small">{t("usage.what")}</p>
        <div className="row usage-controls">
          {PRESETS.map((p) => (
            <button key={p} className={p === id ? "primary" : ""} aria-pressed={p === id} onClick={() => setId(p)}>
              {t(`usage.preset.${p}`)}
            </button>
          ))}
          <label className="usage-sort">
            {t("usage.sort")}
            <select value={sort} onChange={(e) => setSort(e.target.value as SortBy)}>
              <option value="cost">{t("usage.sortCost")}</option>
              <option value="tokens">{t("usage.sortTokens")}</option>
            </select>
          </label>
        </div>
        {data && (
          <p className="small">
            {range} <span className="muted">{t("usage.surviving")}</span>
          </p>
        )}
        {error && (
          <div className="error-text" role="alert">
            {error}
          </div>
        )}
        {data?.cut && (
          <div className="notice" role="status">
            {t("usage.cut", { pct: String(data.cut.covered_percent) })}
          </div>
        )}
        {!data && !error && <div className="muted">{t("usage.loading")}</div>}
        {data && people.length === 0 && <p className="muted small">{t("usage.empty")}</p>}
        {unpriced && <p className="muted small">{t("usage.unpricedHint")}</p>}
      </div>
      {people.map((g) => {
        const p = g.source as { user_id: string | null; username: string | null };
        return <Table key={p.user_id ?? "token"} title={p.username ?? t("usage.noAccount")} group={g} />;
      })}
      {spaces.length > 0 && (
        <>
          <h3>{t("usage.byWorkspace")}</h3>
          {spaces.map((g) => {
            const w = g.source as { workspace_id: string | null; name: string | null };
            return <Table key={w.workspace_id ?? "personal"} title={w.name ?? t("usage.personal")} group={g} />;
          })}
        </>
      )}
    </div>
  );
}
