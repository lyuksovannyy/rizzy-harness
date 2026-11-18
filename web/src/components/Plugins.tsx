// Sidebar → Plugins (ADOPTION_PLAN X.34 step A, PLAN.md T80): one list of
// the skills and connectors (MCP servers) the nodes announce, per workspace,
// with a filter and a search. Each item is Shared (every agent of the
// workspace may use it) or Assigned (only the agents picked); the
// workspace's editors choose, members read. Everything shown comes from a
// node or the server and is rendered as text only.
import { useCallback, useEffect, useState } from "react";
import { call } from "../api";
import { shownName } from "../md/hiddenChars";
import type { FeaturePageProps } from "./pages";
import { Seg } from "./Seg";

type Kind = "skill" | "mcp";
type Access = "shared" | "assigned";
interface Announcer {
  node_id: string;
  hostname: string;
  connected: boolean;
  description: string | null;
}
interface Item {
  kind: Kind;
  name: string;
  nodes: Announcer[];
  access: Access;
  agents: string[];
}
interface PickAgent {
  id: string;
  name: string;
  skills: boolean;
  connectors: boolean;
}
interface NodeRef {
  node_id: string;
  hostname: string;
}
export interface PluginList {
  items: Item[];
  agents: PickAgent[];
  can_edit: boolean;
  unenforced: NodeRef[];
  left_out: (NodeRef & { count: number })[];
}

const base = (ws: string) => `/api/workspaces/${encodeURIComponent(ws)}/plugins`;
const KIND: Record<Kind, string> = { skill: "Skill", mcp: "Connector" };
const host = (n: NodeRef) => shownName(n.hostname || n.node_id);

export default function Plugins(props: FeaturePageProps) {
  const { scope } = props;
  const [list, setList] = useState<PluginList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<"all" | Kind>("all");
  const [q, setQ] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const load = useCallback(() => {
    if (!scope) return;
    call<PluginList>("GET", base(scope))
      .then((l) => {
        setList(l);
        setError(null);
      })
      .catch((e: Error) => setError(e.message));
  }, [scope]);
  useEffect(load, [load]);

  const needle = q.trim().toLowerCase();
  const rows = (list?.items ?? []).filter(
    (i) =>
      (filter === "all" || i.kind === filter) &&
      (!needle || i.name.toLowerCase().includes(needle) || i.nodes.some((n) => (n.description ?? "").toLowerCase().includes(needle))),
  );
  return (
    <section className="agents-page artifacts-page">
      <div className="column wide">
        <div className="agents-head">
          <h1>Plugins</h1>
        </div>
        <p className="muted small">
          The skills and connectors (MCP servers) this server's nodes have. Shared: every agent of this workspace may use it.
          Assigned: only the agents picked. An agent also needs its own Skills or Connectors switch. A repository's own skills come
          with the repository and are not listed here.
        </p>
        {list?.unenforced.map((n) => (
          <div key={n.node_id} className="error-text" role="note">
            {`Node ${host(n)} does not enforce these settings (an older node): there, an agent that is denied any skill gets no skills, and one denied any connector gets no connectors.`}
          </div>
        ))}
        {list?.left_out.map((n) => (
          <div key={n.node_id} className="muted small" role="note">
            {`Node ${host(n)} has ${n.count} more that are not listed (over the limit, or a name that is not allowed).`}
          </div>
        ))}
        <div className="row media-filters">
          <Seg
            label="Kind"
            value={filter}
            options={[
              { value: "all", label: "All" },
              { value: "mcp", label: "Connectors" },
              { value: "skill", label: "Skills" },
            ]}
            onChange={setFilter}
          />
          <input type="search" className="grow" aria-label="Search plugins" placeholder="Search names and descriptions" value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        {!scope && <div className="muted">Plugins are chosen per workspace.</div>}
        {error && <div className="error-text">{error}</div>}
        {scope && !list && !error && <div className="muted">Loading…</div>}
        {list && rows.length === 0 && (
          <div className="media-empty muted">{list.items.length ? "Nothing matches." : "No node has a skill or connector yet."}</div>
        )}
        {list && rows.length > 0 && (
          <ul className="card agent-list plugin-list">
            {rows.map((i) => {
              const key = `${i.kind}/${i.name}`;
              return (
                <Row
                  key={key}
                  item={i}
                  list={list}
                  open={open === key}
                  onToggle={() => setOpen(open === key ? null : key)}
                  onSave={async (access, agents) => {
                    await call("PUT", `${base(scope!)}/${i.kind}/${encodeURIComponent(i.name)}`, { access, agents: access === "assigned" ? agents : [] });
                    setOpen(null);
                    load();
                  }}
                />
              );
            })}
          </ul>
        )}
      </div>
    </section>
  );
}

function Row(props: { item: Item; list: PluginList; open: boolean; onToggle: () => void; onSave: (a: Access, agents: string[]) => Promise<void> }) {
  const { item: i, list } = props;
  const names = new Map(list.agents.map((a) => [a.id, a.name]));
  const described = [...new Set(i.nodes.map((n) => n.description).filter((d): d is string => !!d))];
  const who =
    i.access === "shared"
      ? "Shared: every agent"
      : i.agents.length
        ? `Assigned: ${i.agents.map((id) => names.get(id) ?? id).join(", ")}`
        : "Assigned to no agent";
  return (
    <li className="agent-row plugin-row">
      <div className="grow">
        <div>
          <strong>{shownName(i.name)}</strong> <span className="muted small">{KIND[i.kind]}</span>
        </div>
        {described[0] && <div className="small">{described[0]}</div>}
        {described.length > 1 && <div className="muted small">Nodes describe it differently; the setting is by name and holds for each.</div>}
        <div className="muted small">
          {i.nodes.length
            ? `Node: ${i.nodes.map((n) => host(n) + (n.connected ? "" : " (offline)")).join(", ")}`
            : "No connected node has it now; the setting holds when one does."}
        </div>
        <div className="small">{who}</div>
        {props.open && <Editor item={i} list={list} onSave={props.onSave} onCancel={props.onToggle} />}
      </div>
      {list.can_edit && !props.open && (
        <button type="button" onClick={props.onToggle} aria-label={`Change who may use ${i.name}`}>
          Change
        </button>
      )}
    </li>
  );
}

function Editor(props: { item: Item; list: PluginList; onSave: (a: Access, agents: string[]) => Promise<void>; onCancel: () => void }) {
  const { item: i } = props;
  const [access, setAccess] = useState<Access>(i.access);
  const [picked, setPicked] = useState<Set<string>>(new Set(i.agents));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const toggle = (id: string) =>
    setPicked((s) => {
      const n = new Set(s);
      if (!n.delete(id)) n.add(id);
      return n;
    });
  const save = async () => {
    setBusy(true);
    try {
      await props.onSave(access, [...picked]);
    } catch (e) {
      setError((e as Error).message);
      setBusy(false);
    }
  };
  return (
    <div className="plugin-editor">
      <Seg
        label={`Who may use ${i.name}`}
        value={access}
        options={[
          { value: "shared", label: "Shared" },
          { value: "assigned", label: "Assigned" },
        ]}
        onChange={setAccess}
      />
      {access === "assigned" && (
        <fieldset className="feature-group">
          <legend>Agents</legend>
          {props.list.agents.map((a) => {
            const off = i.kind === "skill" ? !a.skills : !a.connectors;
            return (
              <label key={a.id} className="feature">
                <input type="checkbox" checked={picked.has(a.id)} onChange={() => toggle(a.id)} />
                <span>
                  {shownName(a.name)} <span className="muted small">@{a.id}</span>
                  {off && <span className="muted small">{` (its ${i.kind === "skill" ? "Skills" : "Connectors"} switch is off)`}</span>}
                </span>
              </label>
            );
          })}
        </fieldset>
      )}
      {error && <div className="error-text">{error}</div>}
      <div className="row">
        <button type="button" className="primary" disabled={busy} onClick={() => void save()}>
          Save
        </button>
        <button type="button" disabled={busy} onClick={props.onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}
