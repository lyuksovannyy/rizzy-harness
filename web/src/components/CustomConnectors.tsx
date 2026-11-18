// Plugins → Connectors → custom connectors (ADOPTION_PLAN X.35 step 1,
// PLAN.md T84): the workspace's remote MCP servers, added by its editors by
// address ("Add custom connector"), and each member's own account for one
// (a token they paste, for a connector that signs in with one). Check shows
// what the server offers, as text only. Agents do not use these yet.
import { useCallback, useEffect, useState } from "react";
import { call } from "../api";
import { Paste } from "./Connections";
import { shownName } from "../md/hiddenChars";
import { Seg } from "./Seg";
import type { Live } from "../live";

interface Tool {
  name: string;
  agent_name: string;
  description: string | null;
}
interface Offer {
  server_name: string | null;
  server_version: string | null;
  instructions: string | null;
  tools: Tool[];
  more: boolean;
  /** Tools left out: their agent name is another tool's. */
  left_out?: number;
}
export interface Custom {
  id: string;
  name: string;
  url: string;
  host: string;
  auth: "none" | "token";
  header: string;
  /** A node announces its own server of this name. */
  clash: boolean;
  /** `null`: this connector needs no sign-in. */
  connected: boolean | null;
  offer: Offer | null;
  /** How many of its tools the caller's agents are offered now; `null`: none read yet. */
  agent_tools?: number | null;
  /** Changes with the address, the sign-in and the header: sent with a token or a check. */
  stamp: string;
}
interface List {
  connectors: Custom[];
  can_edit: boolean;
  /** The caller is a person (the API token is not). */
  person: boolean;
  available: boolean;
}

const base = (ws: string) => `/api/workspaces/${encodeURIComponent(ws)}/connectors`;
const AGENTS = "Your agents (with the Connectors switch) use a connector with your own account, in the messages you send and your own routines, and each call is asked about as any MCP tool call; nobody else's agent uses your account.";
const NO_OAUTH =
  "Sign-in through a browser (OAuth) is not built yet; many hosted MCP servers need it and cannot be added until it is. A connector here signs in with a token you paste, or not at all.";

export default function CustomConnectors(props: { scope: string; live?: Live; adding: boolean; onAdded: () => void }) {
  const { scope } = props;
  const [list, setList] = useState<List | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    call<List>("GET", base(scope))
      .then((l) => {
        setList(l);
        setError(null);
      })
      .catch((e: Error) => setError(e.message));
  }, [scope]);
  useEffect(() => {
    load();
    // An admin's change, or one's own in another tab: look again.
    return props.live?.onTopic("connectors", load);
  }, [load, props.live]);
  return (
    <section aria-label="Custom connectors">
      <h3>Custom connectors</h3>
      <p className="muted small">{`Remote MCP servers this workspace's admins added by address. Each person connects their own account; nobody else sees or uses it, admins included. ${AGENTS}`}</p>
      {props.adding && list?.can_edit && (
        <Definition
          title="Add custom connector"
          onCancel={props.onAdded}
          onSave={async (b) => {
            await call("POST", base(scope), b);
            props.onAdded();
            load();
          }}
        />
      )}
      {list && !list.available && (list.connectors ?? []).some((c) => c.auth === "token") && (
        <div className="error-text">This server has no sealed storage set up, so it keeps no tokens.</div>
      )}
      {error && <div className="error-text">{error}</div>}
      {list && (list.connectors ?? []).length > 0 && (
        <ul className="card agent-list">
          {list.connectors.map((c) => (
            <Row key={c.id} c={c} list={list} path={`${base(scope)}/${encodeURIComponent(c.id)}`} onChanged={load} />
          ))}
        </ul>
      )}
    </section>
  );
}

function Row(props: { c: Custom; list: List; path: string; onChanged: () => void }) {
  const { c, list, path } = props;
  const [mode, setMode] = useState<"paste" | "edit" | null>(null);
  // A form open on what changed under it closes: what it would send to
  // is not what the person saw.
  useEffect(() => setMode(null), [c.stamp]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await f();
      setMode(null);
      setError(null);
      props.onChanged();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const signIn = c.auth === "none" ? "No sign-in" : `Token in the ${c.header} header${c.header === "authorization" ? " (Bearer)" : ""}`;
  return (
    <li className="agent-row connection-row">
      <div className="grow">
        <div>
          <strong>{shownName(c.name)}</strong> <span className="muted small">Connector · Workspace</span>
        </div>
        <div className="muted small">{`${c.url} · ${signIn}`}</div>
        {c.clash && (
          <div className="error-text">{`A node of this server has its own MCP server named ${c.name}; until one of them is renamed, agents in chats on that node get the node's server under this name, not this connector.`}</div>
        )}
        {c.connected !== null && list.person && <div className="small">{c.connected ? "Your account is connected." : "Your account is not connected."}</div>}
        {list.person && (c.auth === "none" || c.connected) && (
          <div className="small">
            {typeof c.agent_tools === "number"
              ? `Your agents can use ${c.agent_tools} of its tools now.`
              : "Your agents get its tools once it has been checked (Check, or your next message reads them)."}
          </div>
        )}
        {c.offer && <OfferView o={c.offer} />}
        {mode === "paste" && (
          <Paste
            label="Token"
            hint={`Sent once to ${c.host} to check it, then stored sealed on this server and never shown again.`}
            onCancel={() => setMode(null)}
            onSend={(token) => act(() => call("PUT", `${path}/account`, { token, seen: c.stamp }))}
          />
        )}
        {mode === "edit" && (
          <Definition
            title={`Edit ${c.name}`}
            c={c}
            onCancel={() => setMode(null)}
            onSave={(b) => {
              const header = b.auth === "token" ? (b.header?.trim().toLowerCase() || "authorization") : c.header;
              const moved = b.url !== c.url || b.auth !== c.auth || header !== c.header;
              if (
                moved &&
                c.auth === "token" &&
                !window.confirm(`A new address, sign-in or header disconnects everyone's accounts for ${c.name}: their tokens were given for the old one. Save?`)
              ) {
                return Promise.resolve();
              }
              return act(() => call("PUT", path, b));
            }}
          />
        )}
        {error && <div className="error-text">{error}</div>}
        {!mode && (
          <div className="row">
            {list.person && (c.auth === "none" || c.connected) && (
              <button type="button" disabled={busy} onClick={() => void act(() => call("POST", `${path}/check`, { seen: c.stamp }))}>
                {busy ? "Checking…" : "Check"}
              </button>
            )}
            {list.person && c.auth === "token" && (
              <button type="button" disabled={busy || !list.available} onClick={() => setMode("paste")}>
                {c.connected ? "Replace token" : "Connect"}
              </button>
            )}
            {list.person && c.connected && (
              <button
                type="button"
                disabled={busy}
                onClick={() => window.confirm(`Disconnect your account from ${c.name}? The stored token is deleted.`) && void act(() => call("DELETE", `${path}/account`))}
              >
                Disconnect
              </button>
            )}
            {list.can_edit && (
              <>
                <button type="button" disabled={busy} onClick={() => setMode("edit")}>
                  Edit
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => window.confirm(`Delete the connector ${c.name}? Everyone's account for it is deleted too.`) && void act(() => call("DELETE", path))}
                >
                  Delete
                </button>
              </>
            )}
          </div>
        )}
      </div>
    </li>
  );
}

/** What a check found: the server's own words, as text only. */
function OfferView({ o }: { o: Offer }) {
  return (
    <div className="plugin-editor">
      <div className="small">
        {`${o.server_name ? shownName(o.server_name) : "The server"}${o.server_version ? ` ${shownName(o.server_version)}` : ""} offers ${o.tools.length}${o.more ? " or more" : ""} tool${o.tools.length === 1 ? "" : "s"}.`}
      </div>
      {o.instructions && <div className="muted small">{`Its instructions: ${o.instructions}`}</div>}
      <ul className="muted small">
        {o.tools.map((t) => (
          <li key={t.agent_name}>
            <code>{shownName(t.name)}</code>
            {t.description && ` ${t.description}`}
          </li>
        ))}
      </ul>
      {o.more && <div className="muted small">And more: only the first ones are read.</div>}
      {!!o.left_out && <div className="muted small">{`${o.left_out} tool${o.left_out === 1 ? "" : "s"} left out: the name an agent would see is another of its tools'.`}</div>}
    </div>
  );
}

interface Body {
  name?: string;
  url: string;
  auth: "none" | "token";
  header?: string;
}

/** Add (name, address, sign-in) or edit (address, sign-in) a connector. */
function Definition(props: { title: string; c?: Custom; onCancel: () => void; onSave: (b: Body) => Promise<unknown> }) {
  const { c } = props;
  const [name, setName] = useState(c?.name ?? "");
  const [url, setUrl] = useState(c?.url ?? "https://");
  const [auth, setAuth] = useState<"none" | "token">(c?.auth ?? "token");
  const [header, setHeader] = useState(c && c.header !== "authorization" ? c.header : "");
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    try {
      const b: Body = { url, auth, ...(auth === "token" && header.trim() ? { header } : {}) };
      await props.onSave(c ? b : { ...b, name });
    } catch (e) {
      setError((e as Error).message);
    }
  };
  return (
    <div className="card plugin-editor" role="group" aria-label={props.title}>
      <strong>{props.title}</strong>
      {!c && (
        <label>
          Name (a-z, 0-9, - and _; it does not change later)
          <input value={name} placeholder="linear" maxLength={32} onChange={(e) => setName(e.target.value)} />
        </label>
      )}
      <label>
        Address of the remote MCP server (https, a public host name, no port)
        <input value={url} maxLength={512} spellCheck={false} onChange={(e) => setUrl(e.target.value)} />
      </label>
      <Seg
        label="Sign-in"
        value={auth}
        options={[
          { value: "token", label: "Each person's token" },
          { value: "none", label: "None" },
        ]}
        onChange={setAuth}
      />
      {auth === "token" && (
        <label>
          Header the token goes in (empty: Authorization: Bearer …)
          <input value={header} placeholder="X-API-Key" maxLength={64} spellCheck={false} onChange={(e) => setHeader(e.target.value)} />
        </label>
      )}
      <div className="muted small">{NO_OAUTH}</div>
      {error && <div className="error-text">{error}</div>}
      <div className="row">
        <button type="button" className="primary" onClick={() => void save()}>
          Save
        </button>
        <button type="button" onClick={props.onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}
