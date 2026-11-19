// "From a repository" in the Project popup (ADOPTION_PLAN X.27 step 3,
// PLAN.md T86): the person's own forge connection, its repositories (most
// recently updated first, a search that waits for the person to stop typing
// and cancels the request before), and Create, which asks the node to clone
// one into a new project's folder. The server answers at once with the
// project pending ("cloning"); it becomes a project when the node is done.
// Everything a forge says (names, descriptions, logins) is drawn as one line
// of text with hidden characters shown: never HTML, no link made from it, no
// image. Its own lazy chunk, loaded when the popup draws the section.
import { Lock } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { call, HttpError } from "../api";
import { LIMITS, type PendingProject, projectsApi } from "../lib/projects";
import { routeHash } from "../lib/routes";
import { when } from "../lib/when";
import { shownName } from "../md/hiddenChars";
import type { NodeInfo } from "../types";

interface Conn {
  id: string;
  host: string;
  allowed: boolean;
  login: string;
}
interface Repo {
  id: string;
  full_name: string;
  private: boolean;
  description: string | null;
  updated_ms: number | null;
  push: boolean | null;
}
interface Listing {
  repositories: Repo[];
  page: number;
  has_more: boolean;
  left_out: number;
  search: "none" | "forge" | "within_page";
}

/** How long the search waits after the last key (ms). */
export const DEBOUNCE_MS = 300;
/** Repositories GitHub's own page holds when we search within it. */
const WITHIN_PAGE = 100;
/** The one 429 that is retried: our earlier listing still runs on the server
 * (its error body's `code`; every other 429 is shown at once). Tried again
 * this often before its sentence is shown. */
const LISTING_BUSY = "listing_in_flight";
const RETRY_MS = 600;
const RETRIES = 5;

/** One line of a forge's text: hidden characters shown, runs of blank
 * collapsed, cut (the title keeps up to `title` characters). */
export function oneLine(text: string | null | undefined, max = 200): string {
  const t = shownName((text ?? "").replace(/\s+/g, " ").trim());
  return t.length > max ? `${t.slice(0, max)}…` : t;
}

interface Props {
  workspace?: string | null;
  node?: NodeInfo;
  nodeLabel: string;
  canCreate: boolean;
  /** The server took the clone: the pending project. */
  onStarted: (p: PendingProject) => void;
}

export default function RepoPicker(props: Props) {
  const [conns, setConns] = useState<Conn[] | null>(null);
  const [connErr, setConnErr] = useState<string | null>(null);
  const [connId, setConnId] = useState("");
  const [q, setQ] = useState("");
  const [list, setList] = useState<Listing | null>(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  const [picked, setPicked] = useState<Repo | null>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [createErr, setCreateErr] = useState<string | null>(null);
  const ctl = useRef<AbortController | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // False once the component is gone: nothing answers into it then.
  const alive = useRef(true);
  const search = useRef<HTMLInputElement>(null);
  const nameField = useRef<HTMLInputElement>(null);
  const ids = useId();
  const conn = conns?.find((c) => c.id === connId);

  useEffect(() => {
    let live = true;
    call<{ connections: Conn[] }>("GET", "/api/connections").then(
      (l) => {
        if (!live) return;
        const ok = l.connections.filter((c) => c.allowed);
        setConns(ok);
        if (ok[0]) setConnId(ok[0].id);
      },
      (e: unknown) => live && setConnErr(e instanceof Error ? e.message : String(e)),
    );
    return () => {
      live = false;
    };
  }, []);

  /** Ask for `page` of the list (the next one appended): the request
   * before is cancelled first, so one of ours runs at a time. */
  const load = (page: number, append: boolean, tries = 0) => {
    if (!connId) return;
    clearTimeout(timer.current);
    ctl.current?.abort();
    const c = new AbortController();
    ctl.current = c;
    setLoading(true);
    const word = q.trim();
    const qs = new URLSearchParams();
    if (word) qs.set("q", word);
    if (page > 1) qs.set("page", String(page));
    const path = `/api/connections/${encodeURIComponent(connId)}/repositories${qs.toString() ? `?${qs}` : ""}`;
    call<Listing>("GET", path, undefined, c.signal).then(
      (l) => {
        if (c.signal.aborted) return;
        // By id: a repository that moved between pages is listed once.
        setList((prev) => {
          if (!(append && prev)) return l;
          const have = new Set(prev.repositories.map((r) => r.id));
          return { ...l, repositories: [...prev.repositories, ...l.repositories.filter((r) => !have.has(r.id))] };
        });
        if (!append) setActive(0);
        setErr(null);
        setLoading(false);
      },
      (e: unknown) => {
        if (c.signal.aborted) return;
        // Our earlier listing is still running on the server (it frees
        // its slot when it ends): still loading, not an error. Any other
        // 429 says its own sentence now.
        if (e instanceof HttpError && e.status === 429 && e.code === LISTING_BUSY && tries < RETRIES) {
          timer.current = setTimeout(() => load(page, append, tries + 1), RETRY_MS);
          return;
        }
        setErr(e instanceof Error ? e.message : String(e));
        setLoading(false);
      },
    );
  };

  // A new connection or search word: page 1, once the person stopped typing.
  useEffect(() => {
    if (!connId) return;
    clearTimeout(timer.current);
    timer.current = setTimeout(() => load(1, false), q.trim() ? DEBOUNCE_MS : 0);
    return () => clearTimeout(timer.current);
  }, [connId, q]);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      clearTimeout(timer.current);
      ctl.current?.abort();
    };
  }, []);

  const pick = (r: Repo) => {
    setPicked(r);
    setName(r.full_name.split("/").pop()!.slice(0, LIMITS.name));
    setCreateErr(null);
    setTimeout(() => nameField.current?.focus());
  };

  if (connErr) return <p className="error-text">{connErr}</p>;
  if (!conns) return <p className="muted">Loading…</p>;
  if (!conns.length)
    return (
      <p className="muted">
        No forge account is connected. <a href={routeHash({ kind: "plugins" })}>Connect one under Plugins → Connectors</a>.
      </p>
    );
  const repos = list?.repositories ?? [];
  const within = list?.search === "within_page";
  const opt = (i: number) => `${ids}-o${i}`;
  return (
    <div className="stack">
      {conns.length > 1 && (
        <select aria-label="Forge account" value={connId} onChange={(e) => setConnId(e.target.value)}>
          {conns.map((c) => (
            <option key={c.id} value={c.id}>
              {oneLine(`${c.host} · @${c.login}`, 80)}
            </option>
          ))}
        </select>
      )}
      <input
        ref={search}
        type="search"
        role="combobox"
        aria-label="Search repositories"
        aria-expanded={repos.length > 0}
        aria-controls={`${ids}-list`}
        aria-activedescendant={repos[active] ? opt(active) : undefined}
        placeholder={conn && conns.length === 1 ? `Search ${oneLine(conn.host, 60)} · @${oneLine(conn.login, 40)}` : "Search repositories"}
        maxLength={100}
        value={q}
        onChange={(e) => setQ(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            if (repos.length) setActive((a) => (a + (e.key === "ArrowDown" ? 1 : repos.length - 1)) % repos.length);
          } else if (e.key === "Enter") {
            e.preventDefault();
            if (repos[active]) pick(repos[active]);
          } else if (e.key === "Escape" && q) {
            // Escape leaves the search first, the popup second.
            e.preventDefault();
            e.stopPropagation();
            setQ("");
          }
        }}
      />
      {within && (
        <p className="muted small">
          Searched the {list.page * WITHIN_PAGE} most recently updated repositories (GitHub cannot search the rest by name from
          here).
        </p>
      )}
      <ul id={`${ids}-list`} role="listbox" aria-label="Repositories" className="rp-list">
        {repos.map((r, i) => {
          const full = oneLine(r.full_name, 512);
          return (
            <li
              key={r.id}
              id={opt(i)}
              role="option"
              aria-selected={picked?.id === r.id}
              className={i === active ? "active" : undefined}
              title={full}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                setActive(i);
                pick(r);
              }}
            >
              <span className="rp-name">
                {r.private && <Lock size={12} aria-label="private" />} {oneLine(r.full_name, 120)}
              </span>
              <span className="rp-when">{r.updated_ms ? when(r.updated_ms, Date.now()) : ""}</span>
              {r.description && <span className="rp-desc">{oneLine(r.description)}</span>}
              {r.push === false && <span className="rp-desc">This account cannot push to it.</span>}
            </li>
          );
        })}
      </ul>
      {loading && <p className="muted small">Loading…</p>}
      {!loading && list && !repos.length && !err && (
        <p className="muted small">{q.trim() ? "No repository matches." : "This account sees no repositories."}</p>
      )}
      {list && list.left_out > 0 && (
        <p className="muted small">Some repositories were left out because their names could not be shown safely.</p>
      )}
      {list?.has_more && !loading && (
        <button type="button" className="pp-row" onClick={() => load(list.page + 1, true)}>
          {within ? `Search the next ${WITHIN_PAGE}` : "Show more"}
        </button>
      )}
      {err && (
        <div className="row" role="alert">
          <p className="error-text grow">{err}</p>
          <button type="button" onClick={() => load(1, false)}>
            Retry
          </button>
        </div>
      )}
      {picked && (
        <form
          className="stack"
          onSubmit={(e) => {
            e.preventDefault();
            if (!props.node || !name.trim() || busy) return;
            setBusy(true);
            setCreateErr(null);
            const body = {
              name: name.trim(),
              folder_on: props.node.node_id,
              connection_id: connId,
              full_name: picked.full_name,
              ...(props.workspace ? { workspace_id: props.workspace } : {}),
            };
            // The listing, and a retry of it that waits, end here: nothing of
            // ours is left running to answer later. The clone has its own
            // slot on the server, so a listing never refuses it; any 429 it
            // does get is a limit, said at once.
            clearTimeout(timer.current);
            ctl.current?.abort();
            setLoading(false);
            projectsApi
              .clone(body)
              .then(
                (p) => {
                  if (!alive.current) return;
                  setPicked(null);
                  props.onStarted(p);
                },
                (e: unknown) => alive.current && setCreateErr(e instanceof Error ? e.message : String(e)),
              )
              .finally(() => alive.current && setBusy(false));
          }}
        >
          <input
            ref={nameField}
            aria-label="Project name for the repository"
            placeholder="Project name"
            maxLength={LIMITS.name}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <p className="pp-preview muted">
            Clones <span title={oneLine(picked.full_name, 512)}>{oneLine(picked.full_name, 120)}</span> into a new folder in{" "}
            {props.nodeLabel}'s projects folder.
          </p>
          {createErr && (
            <p className="error-text" role="alert">
              {createErr}
            </p>
          )}
          <div className="row end">
            <button type="submit" className="primary" disabled={!props.canCreate || !props.node || !name.trim() || busy}>
              {busy ? "Starting…" : "Create"}
            </button>
          </div>
        </form>
      )}
    </div>
  );
}
