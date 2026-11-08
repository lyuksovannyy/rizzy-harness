// One artifact (docs/artifacts.md): a name with its versions. The detail
// page (`#/artifacts/<id>`) shows the newest version's notes, what the node
// reported about it (toolchain, git, target, the files in a pack), a preview
// for images, video and audio, and every version. The pieces the list page
// reuses (the icon, the actions, the versions) live here too; both are one
// lazy chunk. Nothing here renders or runs an artifact's contents.
import { ArrowLeft, File, FileArchive, FileText, Film, Image as ImageIcon, Music, Package } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { HttpError } from "../api";
import {
  type Artifact,
  type ArtifactVersion,
  type ArtifactWithVersions,
  artifactsApi,
  chipOf,
  downloadUrl,
  kindLabel,
  latestUrl,
  percent,
  previewable,
  previewUrl,
  shortSha,
  skippedLine,
  sourceLine,
  versionThumbUrl,
} from "../lib/artifacts";
import { bytes } from "../lib/bytes";
import { mediaApi, safeToDecode } from "../lib/media";
import { artifactHash, mediaHash } from "../lib/routes";
import { when } from "../lib/when";
import type { ArtifactsPageProps, FeaturePageProps } from "./pages";

export const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
/** Progress lives in the server's memory: ask again this often while a version uploads. */
const POLL_MS = 1500;

/** What the viewer may do here. */
export interface Can {
  /** A team workspace: sharing exists. */
  team: boolean;
  /** May delete a colleague's shared artifact (Perm::Settings, or the API token). */
  manager: boolean;
  /** Media is shown, so "Also in Media" leads somewhere. */
  media: boolean;
}

export function canOf(p: Pick<FeaturePageProps, "me" | "features">): Can {
  const ws = p.me.workspace;
  return {
    team: !!ws && !ws.personal,
    manager: p.me.user_id === null || !!ws?.permissions.includes("settings"),
    media: p.features.media.show,
  };
}

const ICONS: Record<string, typeof File> = {
  binary: Package,
  package: Package,
  archive: FileArchive,
  document: FileText,
  image: ImageIcon,
  video: Film,
  audio: Music,
};

/** The kind's icon, or the newest version's thumbnail when one was made. */
export function ArtifactIcon({ a }: { a: Pick<Artifact, "artifact_id" | "kind" | "latest"> }) {
  const [broken, setBroken] = useState(false);
  const v = a.latest;
  if (v?.has_thumb && !broken) {
    return <img className="artifact-thumb" src={versionThumbUrl(a.artifact_id, v.version_id)} alt="" onError={() => setBroken(true)} />;
  }
  const Icon = ICONS[a.kind ?? ""] ?? File;
  return (
    <span className="artifact-icon" aria-hidden="true">
      <Icon size={20} />
    </span>
  );
}

/** One lineage with its versions, refetched when `version` changes and every
 * 1.5 s while one is still uploading. */
export function useArtifact(id: string, version: number) {
  const [data, setData] = useState<ArtifactWithVersions | null>(null);
  const [error, setError] = useState<{ text: string; gone: boolean } | null>(null);
  const load = useCallback(() => {
    artifactsApi
      .get(id)
      .then((a) => {
        setData(a);
        setError(null);
      })
      .catch((e) => setError({ text: message(e), gone: e instanceof HttpError && e.status === 404 }));
  }, [id]);
  useEffect(load, [load, version]);
  const uploading = !!data?.versions.some((v) => v.status === "pending");
  useEffect(() => {
    if (!uploading) return;
    const t = setTimeout(load, POLL_MS);
    return () => clearTimeout(t);
  }, [data, uploading, load]);
  return { data, error, load };
}

/** Download latest, "Also in Media" and the ⋯ menu (share, rename, delete). */
export function ArtifactActions(props: { a: Artifact | ArtifactWithVersions; can: Can; onChanged: () => void; onDeleted: () => void }) {
  const { a, can } = props;
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState(a.name);
  const [error, setError] = useState<string | null>(null);
  const run = async (what: () => Promise<unknown>, after: () => void) => {
    setError(null);
    try {
      await what();
      after();
    } catch (e) {
      setError(message(e));
    }
  };
  const share = a.visibility === "private";
  const mayDelete = a.mine || (a.visibility === "workspace" && can.manager);
  const remove = () => {
    if (confirm(`Delete ${a.name} and all its versions?`)) void run(() => artifactsApi.remove(a.artifact_id), props.onDeleted);
  };
  const rename = () => {
    const next = name.trim();
    if (!next || next === a.name) return setRenaming(false);
    void run(
      () => artifactsApi.update(a.artifact_id, { name: next }),
      () => {
        setRenaming(false);
        props.onChanged();
      },
    );
  };
  return (
    <div className="artifact-actions">
      {renaming ? (
        <form
          className="row artifact-rename"
          onSubmit={(e) => {
            e.preventDefault();
            rename();
          }}
        >
          <input aria-label="New name" value={name} maxLength={80} autoFocus onChange={(e) => setName(e.target.value)} />
          <button className="primary">Save</button>
          <button type="button" onClick={() => setRenaming(false)}>
            Cancel
          </button>
        </form>
      ) : (
        <>
          {a.latest && (
            <a className="button-like" href={latestUrl(a.artifact_id)} download>
              Download
            </a>
          )}
          {a.media_id && can.media && (
            <a className="link small" href={mediaHash(null, a.media_id)}>
              Also in Media
            </a>
          )}
          {(a.mine || mayDelete) && (
            <details
              className="artifact-menu"
              // A native disclosure stays open on its own: close it when focus
              // leaves it, on Esc, and once an item is chosen.
              onBlur={(e) => !e.currentTarget.contains(e.relatedTarget as Node | null) && (e.currentTarget.open = false)}
              onKeyDown={(e) => e.key === "Escape" && (e.currentTarget.open = false)}
            >
              <summary className="icon" aria-label={`More for ${a.name}`} title="More">
                ⋯
              </summary>
              <div
                className="artifact-menu-items"
                // Browsers that do not focus a clicked button would blur the menu shut before the click lands.
                onMouseDown={(e) => e.preventDefault()}
                onClick={(e) => ((e.currentTarget.parentElement as HTMLDetailsElement).open = false)}
              >
                {a.mine && can.team && (
                  <button
                    type="button"
                    onClick={() => void run(() => artifactsApi.update(a.artifact_id, { visibility: share ? "workspace" : "private" }), props.onChanged)}
                  >
                    {share ? "Share with workspace" : "Make private"}
                  </button>
                )}
                {a.mine && (
                  <button
                    type="button"
                    onClick={() => {
                      setName(a.name);
                      setRenaming(true);
                    }}
                  >
                    Rename
                  </button>
                )}
                {mayDelete && (
                  <button type="button" className="danger" onClick={remove}>
                    Delete
                  </button>
                )}
              </div>
            </details>
          )}
        </>
      )}
      {error && (
        <span className="error-text small" role="alert">
          {error}
        </span>
      )}
    </div>
  );
}

const STATUS: Record<ArtifactVersion["status"], string> = { pending: "Uploading", ready: "Ready", failed: "Failed" };

/** Every version, newest first: status, size, sha256, git, node, chat, pin, download, delete. */
export function Versions(props: { a: Pick<Artifact, "artifact_id" | "mine">; versions: ArtifactVersion[]; onChanged: () => void }) {
  const [copied, setCopied] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { a } = props;
  const act = async (what: () => Promise<unknown>) => {
    setError(null);
    try {
      await what();
      props.onChanged();
    } catch (e) {
      setError(message(e));
    }
  };
  const copy = async (v: ArtifactVersion) => {
    try {
      await navigator.clipboard.writeText(v.sha256);
      setCopied(v.version_id);
    } catch {
      setError("Couldn't copy; select the checksum on the artifact's page instead.");
    }
  };
  const now = Date.now();
  return (
    <div className="artifact-versions">
      {error && (
        <div className="error-text small" role="alert">
          {error}
        </div>
      )}
      <ul>
        {props.versions.map((v) => {
          const share = percent(v.progress);
          const git = v.meta.git;
          const chat = v.session;
          return (
            <li key={v.version_id} className={`artifact-version is-${v.status}`}>
              <div className="artifact-version-main">
                <span className="badge">v{v.seq}</span>
                <span className={v.status === "failed" ? "routine-bad" : ""}>
                  {STATUS[v.status]}
                  {v.pinned ? " · pinned" : ""}
                </span>
                {v.status === "pending" && v.progress && (
                  <progress max={v.progress.total || 1} value={v.progress.received} aria-label={`v${v.seq} uploaded ${share ?? ""}`} />
                )}
                <span>{bytes(v.size)}</span>
                <span className="mono" title={`sha256 ${v.sha256}`}>
                  {shortSha(v.sha256)}
                </span>
                <button type="button" className="link small" onClick={() => void copy(v)}>
                  {copied === v.version_id ? "Copied" : "Copy sha256"}
                </button>
                <span className="grow" />
                {v.status === "ready" && (
                  <a className="link small" href={downloadUrl(a.artifact_id, v.version_id)} download>
                    Download
                  </a>
                )}
                {a.mine && v.status === "ready" && (
                  <button type="button" className="link small" onClick={() => void act(() => artifactsApi.pin(a.artifact_id, v.version_id, !v.pinned))}>
                    {v.pinned ? "Unpin" : "Pin"}
                  </button>
                )}
                {a.mine && (
                  <button
                    type="button"
                    className="link small danger-text"
                    onClick={() => confirm(`Delete v${v.seq}?`) && void act(() => artifactsApi.removeVersion(a.artifact_id, v.version_id))}
                  >
                    Delete
                  </button>
                )}
              </div>
              {v.status === "failed" && v.error && <div className="error-text small">{v.error}</div>}
              <div className="artifact-version-facts small muted">
                {git && (
                  <span className="mono" title={`${git.branch ? `on ${git.branch}` : ""}${git.dirty ? ", built with uncommitted changes" : ""}`}>
                    {git.commit.slice(0, 7)}
                    {git.dirty ? " (dirty)" : ""}
                  </span>
                )}
                <span>{v.node.hostname}</span>
                {chat && "title" in chat ? (
                  <a href={`#/s/${chat.session_id}`}>{chat.title || "Its chat"}</a>
                ) : chat ? (
                  <span>from a chat you can't open</span>
                ) : null}
                <span>{v.by_user ? "saved by hand" : "published by an agent"}</span>
                <span>{when(v.created_ms, now)}</span>
              </div>
              {copied === v.version_id && (
                <div className="small muted" role="status">
                  Check a download with <code>sha256sum {v.filename}</code>
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/** A picture, a player, or nothing: images only when their size is known to
 * be safe to decode, which only the lineage's Media item tells. */
function Preview({ a, v }: { a: ArtifactWithVersions; v: ArtifactVersion }) {
  const [safe, setSafe] = useState(false);
  const [broken, setBroken] = useState(false);
  const mediaId = a.media_id;
  useEffect(() => {
    setSafe(false);
    if (v.kind !== "image" || !mediaId) return;
    mediaApi
      .get(mediaId)
      .then((m) => setSafe(m.artifact?.version_id === v.version_id && safeToDecode(m)))
      .catch(() => {});
  }, [mediaId, v.version_id, v.kind]);
  if (broken) return <p className="muted small">This browser can't show this file; download it instead.</p>;
  const src = previewUrl(a.artifact_id, v.version_id);
  if (v.kind === "video") return <video className="artifact-preview" src={src} controls preload="metadata" onError={() => setBroken(true)} />;
  if (v.kind === "audio") return <audio src={src} controls preload="metadata" onError={() => setBroken(true)} />;
  if (safe) return <img className="artifact-preview" src={src} alt={a.name} onError={() => setBroken(true)} />;
  return v.has_thumb ? <img className="artifact-thumb large" src={versionThumbUrl(a.artifact_id, v.version_id)} alt={a.name} /> : null;
}

/** What the node reported about a version, as label and value pairs. */
function facts(v: ArtifactVersion): [string, string][] {
  const m = v.meta;
  const rows: [string, string | null | undefined][] = [
    ["Kind", `${kindLabel(v.kind)}${v.format ? `, ${v.format.toUpperCase()}` : ""}${v.packed ? " (folder, packed as tar.gz)" : ""}`],
    ["File", v.filename],
    ["Size", bytes(v.size)],
    ["sha256", v.sha256],
    ["Toolchain", m.toolchain],
    ["Package", m.package ? `${m.package.name} ${m.package.version}` : null],
    ["Git", m.git ? `${m.git.commit.slice(0, 12)}${m.git.branch ? ` on ${m.git.branch}` : ""}${m.git.dirty ? ", with uncommitted changes" : ""}` : null],
    ["Target", m.target],
    ["From", m.source_path],
    ["Node", v.node.hostname],
    ["Left out", skippedLine(m.skipped)],
  ];
  return rows.filter((r): r is [string, string] => !!r[1]);
}

export function ArtifactPage(props: ArtifactsPageProps & { id: string }) {
  const can = canOf(props);
  const [version, setVersion] = useState(0);
  const { live, navigate } = props;
  useEffect(() => live.onTopic("artifacts", () => setVersion((n) => n + 1)), [live]);
  const { data: a, error, load } = useArtifact(props.id, version);
  const back = (
    <a
      className="agent-back link small row"
      href={artifactHash(null)}
      onClick={(e) => {
        e.preventDefault();
        navigate({ kind: "artifacts", id: null });
      }}
    >
      <ArrowLeft size={14} aria-hidden="true" /> Artifacts
    </a>
  );
  if (!a) {
    return (
      <Page>
        {back}
        {error ? (
          <p className={error.gone ? "muted" : "error-text"}>{error.gone ? "This artifact is gone, or it isn't shared with you." : error.text}</p>
        ) : (
          <p className="muted">Loading…</p>
        )}
      </Page>
    );
  }
  const v = a.latest ?? a.versions[0] ?? null;
  const from = sourceLine(a);
  const entries = v?.meta.entries ?? [];
  const total = v?.meta.entries_total ?? entries.length;
  return (
    <Page>
      {back}
      <div className="artifact-head">
        <ArtifactIcon a={a} />
        <div className="grow">
          <h1 className="artifact-title">{a.name}</h1>
          <div className="small muted row artifact-sub">
            {v && <span className="badge">v{v.seq}</span>}
            {v && chipOf(v) && <span className="tag">{chipOf(v)}</span>}
            {a.visibility === "workspace" && <span className="tag">Shared</span>}
            {from && <span>{from}</span>}
            <span>{when(a.updated_ms, Date.now())}</span>
          </div>
        </div>
        <ArtifactActions a={a} can={can} onChanged={load} onDeleted={() => navigate({ kind: "artifacts", id: null })} />
      </div>
      {!a.latest && <p className="muted">No version is ready yet.</p>}
      {v?.notes && <p className="artifact-notes">{v.notes}</p>}
      {v && previewable(v) && <Preview a={a} v={v} />}
      {v && (
        <dl className="media-facts small artifact-facts">
          {facts(v).map(([k, val]) => (
            <div key={k} className="artifact-fact">
              <dt>{k}</dt>
              <dd className={k === "sha256" ? "mono" : undefined}>{val}</dd>
            </div>
          ))}
        </dl>
      )}
      {entries.length > 0 && (
        <details className="artifact-entries" open={entries.length <= 20}>
          <summary className="small">{total === 1 ? "1 file" : `${total} files`} in the pack</summary>
          <ul className="mono small">
            {entries.map((e) => (
              <li key={e.path}>
                <span className="ellipsis">{e.path}</span> <span className="muted">{bytes(e.size)}</span>
              </li>
            ))}
            {total > entries.length && <li className="muted">and {total - entries.length} more</li>}
          </ul>
        </details>
      )}
      <h2 className="routine-section">Versions</h2>
      <Versions a={a} versions={a.versions} onChanged={load} />
    </Page>
  );
}

export function Page(props: { children: React.ReactNode }) {
  return (
    <section className="agents-page artifacts-page">
      <div className="column wide">{props.children}</div>
    </section>
  );
}
