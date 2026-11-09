// Sidebar → Artifacts (docs/artifacts.md): files agents published and files
// saved from agentic chats, one row per name with a "v7" badge, filtered by
// kind, by whose (team workspaces) and by a name search. A row opens to its
// versions; its name opens the detail page (ArtifactDetail.tsx, the same lazy
// chunk). Artifacts are stored files to download: never rendered or run.
import { useCallback, useEffect, useState } from "react";
import { type Artifact, type ArtifactPage as Listing, artifactsApi, chipOf, KIND_FILTERS, type KindFilter, sourceLine } from "../lib/artifacts";
import { bytes } from "../lib/bytes";
import { artifactHash } from "../lib/routes";
import { when } from "../lib/when";
import { ArtifactActions, ArtifactIcon, ArtifactPage, type Can, canOf, message, Page, useArtifact, Versions } from "./ArtifactDetail";
import type { ArtifactsPageProps } from "./pages";
import { Seg } from "./Seg";

/** Typing settles before the names are searched. */
const SEARCH_MS = 250;
type Whose = "all" | "mine" | "shared";

export default function Artifacts(props: ArtifactsPageProps) {
  return props.id ? <ArtifactPage key={props.id} {...props} id={props.id} /> : <ArtifactList {...props} />;
}

function ArtifactList(props: ArtifactsPageProps) {
  const { scope, live, navigate } = props;
  const can = canOf(props);
  const [kind, setKind] = useState<KindFilter | "all">("all");
  const [whose, setWhose] = useState<Whose>("all");
  const [project, setProject] = useState("");
  const [typed, setTyped] = useState("");
  const [q, setQ] = useState("");
  const [page, setPage] = useState<Listing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<Set<string>>(new Set());
  // Bumped on every load, so an open row's versions follow the list.
  const [version, setVersion] = useState(0);

  useEffect(() => {
    const t = setTimeout(() => setQ(typed), SEARCH_MS);
    return () => clearTimeout(t);
  }, [typed]);

  const filters = { workspace: scope, project: project || null, kind: kind === "all" ? null : kind, mine: whose === "mine", q };
  const load = useCallback(() => {
    artifactsApi
      .list(filters)
      .then((p) => {
        setPage(p);
        setError(null);
      })
      .catch((e) => setError(message(e)));
    setVersion((n) => n + 1);
  }, [scope, project, kind, whose, q]);
  useEffect(() => {
    load();
    return live.onTopic("artifacts", load);
  }, [load, live]);

  const more = async () => {
    if (!page?.next_cursor) return;
    try {
      const p = await artifactsApi.list({ ...filters, cursor: page.next_cursor });
      setPage({ ...p, artifacts: [...page.artifacts, ...p.artifacts] });
    } catch (e) {
      setError(message(e));
    }
  };
  const toggle = (id: string) =>
    setOpen((s) => {
      const n = new Set(s);
      if (!n.delete(id)) n.add(id);
      return n;
    });

  // "Shared" has no server filter: the rows shared with the workspace, mine or not.
  const rows = (page?.artifacts ?? []).filter((a) => whose !== "shared" || a.visibility === "workspace");
  const filtered = kind !== "all" || whose !== "all" || !!project || !!q;
  const usage = page?.usage;
  return (
    <Page>
      <div className="agents-head media-head">
        <h1>Artifacts</h1>
        {usage && (
          <span className="media-usage small muted meter" title="Media and Artifacts share this space">
            {usage.quota ? `${bytes(usage.used)} of ${bytes(usage.quota)} used` : `${bytes(usage.used)} used`}
          </span>
        )}
      </div>
      <div className="row media-filters">
        <Seg label="Kind" value={kind} options={KIND_FILTERS.map(([value, label]) => ({ value, label }))} onChange={setKind} wrap />
        {can.team && (
          <Seg
            label="Whose"
            value={whose}
            options={[
              { value: "all", label: "All" },
              { value: "mine", label: "Mine" },
              { value: "shared", label: "Shared" },
            ]}
            onChange={setWhose}
          />
        )}
        {props.projects.length > 0 && (
          <select aria-label="Project" value={project} onChange={(e) => setProject(e.target.value)}>
            <option value="">Every project</option>
            {props.projects.map((p) => (
              <option key={p.project_id} value={p.project_id}>
                {p.name}
              </option>
            ))}
          </select>
        )}
        <input type="search" className="grow" aria-label="Search artifacts" placeholder="Search names, files and notes" value={typed} onChange={(e) => setTyped(e.target.value)} />
      </div>
      {error && <div className="error-text">{error}</div>}
      {!page && !error && <div className="muted">Loading…</div>}
      {page && rows.length === 0 && (
        <div className="media-empty muted">
          {q ? `Nothing matches “${q}”.` : filtered ? "Nothing here with these filters." : props.features.artifacts.why ?? EMPTY}
        </div>
      )}
      {rows.length > 0 && (
        <ul className="card agent-list artifact-list">
          {rows.map((a) => (
            <Row key={a.artifact_id} a={a} can={can} open={open.has(a.artifact_id)} version={version} onToggle={() => toggle(a.artifact_id)} onChanged={load} navigate={navigate} />
          ))}
        </ul>
      )}
      {page?.next_cursor && (
        <button type="button" className="routine-more" onClick={() => void more()}>
          Load more
        </button>
      )}
    </Page>
  );
}

const EMPTY = "No artifacts yet. Ask an agent to build something and publish it, or use “Save as artifact” on a file in an agentic chat.";

function Row(props: {
  a: Artifact;
  can: Can;
  open: boolean;
  version: number;
  onToggle: () => void;
  onChanged: () => void;
  navigate: ArtifactsPageProps["navigate"];
}) {
  const { a } = props;
  const v = a.latest;
  const chip = v ? chipOf(v) : null;
  const from = sourceLine(a);
  return (
    <li className="artifact-row">
      <div className="artifact-main">
        <ArtifactIcon a={a} />
        <div className="agent-info">
          <span className="row artifact-line">
            <a
              className="agent-name"
              href={artifactHash(a.artifact_id)}
              onClick={(e) => {
                e.preventDefault();
                props.navigate({ kind: "artifacts", id: a.artifact_id });
              }}
            >
              {a.name}
            </a>
            {v && <span className="badge">v{v.seq}</span>}
            {v && <span className="small">{bytes(v.size)}</span>}
            {chip && <span className="tag">{chip}</span>}
            {a.visibility === "workspace" && <span className="tag">Shared</span>}
          </span>
          <span className="small muted">
            {[from, v?.status === "ready" ? null : "no version ready yet", when(a.updated_ms, Date.now())].filter(Boolean).join(" · ")}
          </span>
        </div>
        <button type="button" className="link small" aria-expanded={props.open} onClick={props.onToggle}>
          {a.versions === 1 ? "1 version" : `${a.versions} versions`}
        </button>
        <ArtifactActions a={a} can={props.can} onChanged={props.onChanged} onDeleted={props.onChanged} />
      </div>
      {props.open && <RowVersions a={a} version={props.version} onChanged={props.onChanged} />}
    </li>
  );
}

function RowVersions(props: { a: Artifact; version: number; onChanged: () => void }) {
  const { data, error, load } = useArtifact(props.a.artifact_id, props.version);
  if (error) return <div className="error-text small">{error.text}</div>;
  if (!data) return <div className="muted small">Loading…</div>;
  return (
    <Versions
      a={props.a}
      versions={data.versions}
      onChanged={() => {
        load();
        props.onChanged();
      }}
    />
  );
}
