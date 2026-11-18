// Sidebar → Plugins (ADOPTION_PLAN X.34 steps A and B, PLAN.md T80, T81):
// one list of the skills and connectors (MCP servers) the nodes announce and
// the skills stored on the server (Built-in: the server's; Workspace: this
// one's), per workspace, with a filter and a search. Each item is Shared
// (every agent of the workspace may use it) or Assigned (only the agents
// picked); the workspace's editors choose, members read. Stored skills are
// written here (Add: write one, or upload a SKILL.md, checked on the
// server), opened, edited and deleted by their scope's editors. Everything
// shown comes from a node or the server and is rendered as text only. The
// person's own forge accounts (X.27) are listed first, in `Connections`, and
// the workspace's custom connectors (X.35) after them, in `CustomConnectors`.
import { own } from "../emptyView";
import { useCallback, useEffect, useState } from "react";
import { call } from "../api";
import Connections from "./Connections";
import CustomConnectors from "./CustomConnectors";
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
interface Stored {
  origin: "builtin" | "workspace";
  description: string;
  can_edit: boolean;
}
interface Item {
  kind: Kind;
  name: string;
  /** A skill stored on the server (step B). */
  stored?: Stored | null;
  /** A custom connector of this workspace (X.35). */
  custom?: { id: string; host: string | null; auth: string } | null;
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
  /** May write the server's own skills (a server admin). */
  can_edit_server?: boolean;
  /** This workspace's own skills a server skill's name hides (editors only). */
  hidden_skills?: { name: string; description: string }[];
  unenforced: NodeRef[];
  left_out: (NodeRef & { count: number })[];
}

const base = (ws: string) => `/api/workspaces/${encodeURIComponent(ws)}/plugins`;
const KIND: Record<Kind, string> = { skill: "Skill", mcp: "Connector" };
const host = (n: NodeRef) => shownName(n.hostname || n.node_id);
const skills = (ws: string) => `/api/workspaces/${encodeURIComponent(ws)}/skills`;
const scopeOf = (s: Stored) => (s.origin === "builtin" ? "server" : "workspace");
const ORIGIN = { builtin: "Built-in", workspace: "Workspace" };
/** The largest SKILL.md the server takes (`MAX_SKILL_MD`: 24 KiB of instructions and 16 KiB for front matter). */
const MAX_SKILL_MD = 40 * 1024;

export default function Plugins(props: FeaturePageProps) {
  const { scope } = props;
  const [list, setList] = useState<PluginList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<"all" | Kind>("all");
  const [q, setQ] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const [adding, setAdding] = useState<"menu" | "write" | "upload" | "connector" | null>(null);
  const [said, setSaid] = useState<string[]>([]);
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
      (!needle ||
        i.name.toLowerCase().includes(needle) ||
        (i.stored?.description ?? "").toLowerCase().includes(needle) ||
        i.nodes.some((n) => (n.description ?? "").toLowerCase().includes(needle))),
  );
  return (
    <section className="agents-page artifacts-page">
      <div className="column wide">
        <div className="agents-head">
          <h1>Plugins</h1>
          {scope && list && (list.can_edit || list.can_edit_server) && (
            <button type="button" aria-haspopup="menu" onClick={() => setAdding(adding ? null : "menu")}>
              Add
            </button>
          )}
        </div>
        {adding === "menu" && (
          <div className="row" role="menu">
            <button type="button" role="menuitem" onClick={() => setAdding("write")}>
              Write a skill
            </button>
            <button type="button" role="menuitem" onClick={() => setAdding("upload")}>
              Upload SKILL.md
            </button>
            {list?.can_edit && (
              <button type="button" role="menuitem" onClick={() => setAdding("connector")}>
                Add custom connector
              </button>
            )}
          </div>
        )}
        {scope && list && (adding === "write" || adding === "upload") && (
          <SkillForm
            mode={adding}
            list={list}
            onCancel={() => setAdding(null)}
            onDone={(notes) => {
              setAdding(null);
              setSaid(notes);
              load();
            }}
            send={(body) => call<{ notes: string[] }>("POST", skills(scope), body)}
          />
        )}
        {said.map((n) => (
          <div key={n} className="muted small" role="status">
            {n}
          </div>
        ))}
        {list?.hidden_skills?.map((h) => (
          <div key={h.name} className="row muted small" role="note">
            <span className="grow">{`This workspace's skill ${shownName(h.name)} is hidden: a built-in skill has its name. Make it again under another name, or delete it.`}</span>
            <button
              type="button"
              onClick={() =>
                window.confirm(`Delete this workspace's skill ${h.name}?`) &&
                void call("DELETE", `${skills(scope!)}/workspace/${encodeURIComponent(h.name)}`)
                  .then(load)
                  .catch((e: Error) => setError(e.message))
              }
            >
              Delete
            </button>
          </div>
        ))}
        <p className="muted small">
          The skills and connectors (MCP servers) this server's nodes have, and the skills stored on this server: Built-in ones
          for every workspace, Workspace ones for this one. A stored skill is text only (a name, a description, instructions); a
          skill that brings programs is installed on a node by its owner. Shared: every agent of this workspace may use it.
          Assigned: only the agents picked. An agent also needs its own Skills or Connectors switch. A repository's own skills come
          with the repository and are not listed here.
        </p>
        <Connections />
        {scope && (
          <CustomConnectors
            scope={scope}
            live={props.live}
            adding={adding === "connector"}
            onAdded={() => {
              setAdding(null);
              load();
            }}
          />
        )}
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
          <div className="media-empty muted">{list.items.length ? "Nothing matches." : "No skill or connector yet."}</div>
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
                  scope={scope!}
                  onChanged={load}
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

function Row(props: {
  item: Item;
  list: PluginList;
  scope: string;
  open: boolean;
  onToggle: () => void;
  onChanged: () => void;
  onSave: (a: Access, agents: string[]) => Promise<void>;
}) {
  const { item: i, list } = props;
  const [shown, setShown] = useState(false);
  const names = new Map(list.agents.map((a) => [a.id, a.name]));
  const described = [...new Set([i.stored?.description, ...i.nodes.map((n) => n.description)].filter((d): d is string => !!d))];
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
          <strong>{shownName(i.name)}</strong> <span className="muted small">{own(KIND, i.kind) ?? i.kind}</span>
          {i.stored && <span className="muted small">{` · ${own(ORIGIN, i.stored.origin) ?? i.stored.origin}`}</span>}
        </div>
        {described[0] && <div className="small">{described[0]}</div>}
        {described.length > 1 && <div className="muted small">Nodes describe it differently; the setting is by name and holds for each.</div>}
        <div className="muted small">
          {i.custom
            ? `Custom connector of this workspace at ${i.custom.host ?? "?"}. Agents cannot use it in this version.`
            : i.stored
            ? i.nodes.length
              ? `Stored on the server. ${i.nodes.map(host).join(", ")} has its own skill of this name and uses that one there.`
              : "Stored on the server."
            : i.nodes.length
              ? `Node: ${i.nodes.map((n) => host(n) + (n.connected ? "" : " (offline)")).join(", ")}`
              : "No connected node has it now; the setting holds when one does."}
        </div>
        <div className="small">{who}</div>
        {props.open && <Editor item={i} list={list} onSave={props.onSave} onCancel={props.onToggle} />}
        {shown && i.stored && (
          <StoredView
            path={`${skills(props.scope)}/${scopeOf(i.stored)}/${encodeURIComponent(i.name)}`}
            onClose={() => setShown(false)}
            onChanged={props.onChanged}
          />
        )}
      </div>
      {i.stored && !shown && (
        <button type="button" onClick={() => setShown(true)} aria-label={`Open ${i.name}`}>
          Open
        </button>
      )}
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

interface Detail {
  name: string;
  description: string;
  body: string;
  can_edit: boolean;
}

/** A stored skill opened: its text as text; Edit and Delete for its scope's editors. */
function StoredView(props: { path: string; onClose: () => void; onChanged: () => void }) {
  const [d, setD] = useState<Detail | null>(null);
  const [edit, setEdit] = useState<{ description: string; body: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    call<Detail>("GET", props.path)
      .then(setD)
      .catch((e: Error) => setError(e.message));
  }, [props.path]);
  const act = async (f: () => Promise<unknown>) => {
    try {
      await f();
      props.onChanged();
      props.onClose();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  return (
    <div className="plugin-editor">
      {error && <div className="error-text">{error}</div>}
      {d && !edit && <pre className="skill-body">{d.body}</pre>}
      {edit && (
        <>
          <label>
            Description
            <input value={edit.description} maxLength={1024} onChange={(e) => setEdit({ ...edit, description: e.target.value })} />
          </label>
          <label>
            Instructions
            <textarea rows={12} value={edit.body} onChange={(e) => setEdit({ ...edit, body: e.target.value })} />
          </label>
        </>
      )}
      <div className="row">
        {d?.can_edit && !edit && (
          <>
            <button type="button" onClick={() => setEdit({ description: d.description, body: d.body })}>
              Edit
            </button>
            <button type="button" onClick={() => window.confirm(`Delete the skill ${d.name}?`) && void act(() => call("DELETE", props.path))}>
              Delete
            </button>
          </>
        )}
        {edit && (
          <button type="button" className="primary" onClick={() => void act(() => call("PUT", props.path, edit))}>
            Save
          </button>
        )}
        <button type="button" onClick={edit ? () => setEdit(null) : props.onClose}>
          {edit ? "Cancel" : "Close"}
        </button>
      </div>
    </div>
  );
}

/** Write a skill (name, description, instructions) or upload one SKILL.md, read here as text and checked on the server. */
function SkillForm(props: {
  mode: "write" | "upload";
  list: PluginList;
  onCancel: () => void;
  onDone: (notes: string[]) => void;
  send: (body: object) => Promise<{ notes: string[] }>;
}) {
  const scopes = [
    ...(props.list.can_edit ? [{ value: "workspace", label: "This workspace" }] : []),
    ...(props.list.can_edit_server ? [{ value: "server", label: "Built-in (every workspace)" }] : []),
  ];
  const [scope, setScope] = useState(scopes[0]!.value);
  const [f, setF] = useState({ name: "", description: "", body: "" });
  const [md, setMd] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pick = async (file: File | undefined) => {
    setError(null);
    if (!file) return setMd(null);
    // The server's limit (MAX_SKILL_MD), refused here before it is sent.
    if (file.size > MAX_SKILL_MD) return setError(`That file is ${Math.ceil(file.size / 1024)} KiB; a SKILL.md is at most 40 KiB.`);
    setMd(await file.text());
  };
  const save = async () => {
    try {
      const r = await props.send(props.mode === "upload" ? { scope, skill_md: md ?? "" } : { scope, ...f });
      props.onDone(r.notes ?? []);
    } catch (e) {
      setError((e as Error).message);
    }
  };
  return (
    <div className="card plugin-editor">
      {scopes.length > 1 && <Seg label="Where" value={scope} options={scopes} onChange={setScope} />}
      {props.mode === "upload" ? (
        <label>
          SKILL.md
          <input type="file" accept=".md,text/markdown,text/plain" onChange={(e) => void pick(e.target.files?.[0])} />
        </label>
      ) : (
        <>
          <label>
            Name
            <input value={f.name} placeholder="release-notes" maxLength={64} onChange={(e) => setF({ ...f, name: e.target.value })} />
          </label>
          <label>
            Description: what it is for and when to use it
            <input value={f.description} maxLength={1024} onChange={(e) => setF({ ...f, description: e.target.value })} />
          </label>
          <label>
            Instructions
            <textarea rows={12} value={f.body} onChange={(e) => setF({ ...f, body: e.target.value })} />
          </label>
        </>
      )}
      {error && <div className="error-text">{error}</div>}
      <div className="row">
        <button type="button" className="primary" disabled={props.mode === "upload" && md === null} onClick={() => void save()}>
          Save
        </button>
        <button type="button" onClick={props.onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}
