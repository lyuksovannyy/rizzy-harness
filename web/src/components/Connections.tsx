// Plugins → Connectors → forge accounts (ADOPTION_PLAN X.27 step 1, PLAN.md
// T83): the person's own connections, the same in every workspace. One item
// per forge host the server allows: Connect (a pasted access token, sent once
// and never shown again), then @login with its host, Replace token and
// Disconnect. Everything the forge said is rendered as text only.
import { useCallback, useEffect, useState } from "react";
import { call } from "../api";
import { shownName } from "../md/hiddenChars";

interface Host {
  host: string;
  kind: string;
  token_page: string;
  scopes_needed: string;
}
interface Conn {
  id: string;
  host: string;
  allowed: boolean;
  login: string;
  display_name: string | null;
  commit_email: string | null;
  scopes: string[] | null;
  /** Scopes the forge reported beyond those kept. */
  scopes_more?: number;
}
interface List {
  hosts: Host[];
  connections: Conn[];
  available: boolean;
}
const KIND: Record<string, string> = { github: "GitHub", gitlab: "GitLab", gitea: "Gitea", forgejo: "Forgejo" };

export default function Connections() {
  const [list, setList] = useState<List | null>(null);
  const [error, setError] = useState<string | null>(null);
  // "host" to connect there, or a connection id to replace its token.
  const [pasting, setPasting] = useState<string | null>(null);
  const load = useCallback(() => {
    call<List>("GET", "/api/connections")
      .then((l) => {
        setList(l);
        setError(null);
      })
      .catch((e: Error) => setError(e.message));
  }, []);
  useEffect(load, [load]);
  const act = async (f: () => Promise<unknown>) => {
    try {
      await f();
      setPasting(null);
      setError(null);
      load();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  if (!list) return error ? <div className="error-text">{error}</div> : null;
  const orphans = list.connections.filter((c) => !c.allowed);
  return (
    <section aria-label="Forge accounts">
      <h2>Connectors</h2>
      <p className="muted small">
        Your own forge accounts, connected with an access token you paste. A connection is yours in every workspace; nobody else
        sees it, admins included. The token is stored sealed on this server and never shown again. In this version a connection
        is stored and checked only: cloning and pushing come next.
      </p>
      {!list.available && <div className="error-text">This server has no sealed storage set up, so it keeps no tokens.</div>}
      {error && <div className="error-text">{error}</div>}
      <ul className="card agent-list">
        {list.hosts.map((h) => {
          const mine = list.connections.filter((c) => c.host === h.host);
          return (
            <li key={h.host} className="agent-row connection-row">
              <div className="grow">
                <div>
                  <strong>{KIND[h.kind] ?? h.kind}</strong> <span className="muted small">{h.host}</span>
                </div>
                {mine.map((c) => (
                  <Account key={c.id} c={c} pasting={pasting === c.id} onPaste={() => setPasting(c.id)} act={act} />
                ))}
                {pasting === h.host && (
                  <Paste
                    host={h}
                    onCancel={() => setPasting(null)}
                    onSend={(token) => act(() => call("POST", "/api/connections", { host: h.host, token }))}
                  />
                )}
              </div>
              {pasting !== h.host && (
                <button type="button" onClick={() => setPasting(h.host)} aria-label={`Connect a ${h.host} account`}>
                  {mine.length ? "Add account" : "Connect"}
                </button>
              )}
            </li>
          );
        })}
        {orphans.map((c) => (
          <li key={c.id} className="agent-row connection-row">
            <div className="grow">
              <div className="muted small">{`${c.host}: this server no longer allows this host.`}</div>
              <Account c={c} pasting={false} onPaste={() => undefined} act={act} />
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

function Account(props: { c: Conn; pasting: boolean; onPaste: () => void; act: (f: () => Promise<unknown>) => Promise<void> }) {
  const { c } = props;
  const path = `/api/connections/${encodeURIComponent(c.id)}`;
  return (
    <div className="plugin-editor">
      <div>
        <strong>{`@${shownName(c.login)}`}</strong> <span className="muted small">{c.host}</span>
        {c.display_name && <span className="small">{` ${shownName(c.display_name)}`}</span>}
      </div>
      {c.commit_email && <div className="muted small">{`Commit e-mail: ${shownName(c.commit_email)}`}</div>}
      <div className="muted small">
        {c.scopes
          ? `Token scopes: ${c.scopes.join(", ") || "none"}${c.scopes_more ? ` and ${c.scopes_more} more` : ""}`
          : "This forge does not report the token's scopes."}
      </div>
      {props.pasting ? (
        <Paste onCancel={props.onPaste} onSend={(token) => props.act(() => call("PUT", `${path}/token`, { token }))} />
      ) : (
        <div className="row">
          {c.allowed && (
            <button type="button" onClick={props.onPaste}>
              Replace token
            </button>
          )}
          <button
            type="button"
            onClick={() => window.confirm(`Disconnect @${c.login} on ${c.host}? The stored token is deleted.`) && void props.act(() => call("DELETE", path))}
          >
            Disconnect
          </button>
        </div>
      )}
    </div>
  );
}

/** A token field (also custom connectors'): sent once, cleared at once, kept from browsers and password managers. */
export function Paste(props: { host?: Host; hint?: string; label?: string; onCancel: () => void; onSend: (token: string) => Promise<void> }) {
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const h = props.host;
  return (
    <form
      className="plugin-editor"
      onSubmit={(e) => {
        e.preventDefault();
        setBusy(true);
        const sent = token;
        // The token leaves this page's state at once, whatever the answer.
        setToken("");
        void props.onSend(sent).finally(() => setBusy(false));
      }}
    >
      {h && (
        <div className="muted small">
          {`Make a token at ${h.token_page}. Later steps (listing repositories, clone and push) will need: ${h.scopes_needed}.`}
        </div>
      )}
      {props.hint && <div className="muted small">{props.hint}</div>}
      <label>
        {props.label ?? "Access token"}
        {/* Not a password of this site: "new-password" keeps browsers from
            filling or saving it, and the data-* attributes ask password
            managers (1Password, LastPass, Bitwarden, Dashlane) to leave it. */}
        <input
          type="password"
          name="forge-access-token"
          autoComplete="new-password"
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
          data-1p-ignore=""
          data-lpignore="true"
          data-bwignore=""
          data-form-type="other"
          value={token}
          onChange={(e) => setToken(e.target.value)}
        />
      </label>
      <div className="row">
        <button type="submit" className="primary" disabled={busy || !token.trim()}>
          {busy ? "Checking…" : "Save"}
        </button>
        <button type="button" disabled={busy} onClick={props.onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}
