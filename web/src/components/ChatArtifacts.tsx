// Artifacts inside a chat (docs/artifacts.md): the card a `publish` call
// becomes, the item a file saved by hand becomes, and "Save as artifact" on
// tool cards and in the header. Where each file stands (uploading, ready,
// failed) comes from `GET /api/sessions/{id}/artifacts`, asked once per chat
// for all of its cards, again on `artifacts_changed`, and every 1.5 s while a
// file is on its way, because upload progress lives in the server's memory.
import { Package } from "lucide-react";
import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { artifactsApi, downloadUrl, percent, type SessionArtifact } from "../lib/artifacts";
import { bytes } from "../lib/bytes";
import { artifactHash } from "../lib/routes";
import type { Live } from "../live";
import type { Item, StagedArtifact } from "../types";

const POLL_MS = 1500;
/** After this many polls (a minute), a staged file the server has no row for
 * is not coming (a throttled node), and an upload still pending (its node
 * went away) is asked about ten times less often. */
const MAX_MISSING_POLLS = 40;

export interface ChatArtifacts {
  sessionId: string;
  /** "Save as artifact" is offered: an agentic chat the viewer drives, with Artifacts on. */
  saveable: boolean;
  /** A staged file's row, by the publish call's id or the stage id. */
  find: (key: string) => SessionArtifact | undefined;
  /** Polling gave up on files the server never listed. */
  gaveUp: boolean;
}

export const ChatArtifactsCtx = createContext<ChatArtifacts | null>(null);

/** What a chat's cards wait for: each finished publish call and each saved file. */
export function stagedKeys(items: Item[]): string[] {
  return items.flatMap((it) =>
    it.type === "tool" && it.tool === "publish" && it.result?.ok ? [it.call_id] : it.type === "artifact" ? [it.staged.stage_id] : [],
  );
}

/** The chat's staged files, kept current while any card needs them. */
export function useChatArtifacts(sessionId: string, live: Live, keys: string[], saveable: boolean): ChatArtifacts {
  const [rows, setRows] = useState<SessionArtifact[] | null>(null);
  const polls = useRef(0);
  const wanted = keys.join(",");
  const load = useCallback(() => {
    artifactsApi
      .forSession(sessionId)
      .then(setRows)
      .catch(() => {});
  }, [sessionId]);
  useEffect(() => {
    if (!wanted) return;
    polls.current = 0;
    load();
    return live.onTopic("artifacts", load);
  }, [wanted, load, live]);

  const missing = !!rows && keys.some((k) => !rows.some((r) => r.call_id === k || r.stage_id === k));
  const gaveUp = missing && polls.current >= MAX_MISSING_POLLS;
  useEffect(() => {
    if (!rows || !wanted) return;
    if (!rows.some((r) => r.status === "pending") && (!missing || gaveUp)) return;
    const t = setTimeout(
      () => {
        polls.current += 1;
        load();
      },
      polls.current >= MAX_MISSING_POLLS ? POLL_MS * 10 : POLL_MS,
    );
    return () => clearTimeout(t);
  }, [rows, wanted, missing, gaveUp, load]);

  const find = (key: string) => rows?.find((r) => r.call_id === key || r.stage_id === key);
  return { sessionId, saveable, find, gaveUp };
}

/** One staged file: its name and size, where it stands, and its links once ready. */
function StagedFile(props: { label: string; name: string; size: number | null; row: SessionArtifact | undefined; gaveUp: boolean }) {
  const { row } = props;
  const share = row?.status === "pending" ? percent(row.progress) : null;
  const state = !row
    ? props.gaveUp
      ? "The server didn't take this file."
      : "Uploading to the server…"
    : row.status === "pending"
      ? `Uploading${share ? ` ${share}` : "…"}`
      : row.status === "ready"
        ? "In Artifacts"
        : `Failed: ${row.error ?? "unknown error"}`;
  return (
    <div className={`artifact-card is-${row?.status ?? "pending"}`}>
      <Package size={16} aria-hidden="true" />
      <span className="artifact-card-text">
        <span className="small muted">{props.label}</span> <strong className="ellipsis">{props.name}</strong>
        {props.size !== null && <span className="small muted"> · {bytes(props.size)}</span>}
      </span>
      <span className={`small ${row?.status === "failed" ? "error-text" : "muted"}`} role="status">
        {state}
      </span>
      {row?.status === "pending" && row.progress && <progress max={row.progress.total || 1} value={row.progress.received} aria-hidden="true" />}
      {row?.status === "ready" && row.artifact_id && row.version_id && (
        <a className="link small" href={downloadUrl(row.artifact_id, row.version_id)} download>
          Download
        </a>
      )}
      {row?.artifact_id && (
        <a className="link small" href={artifactHash(row.artifact_id)}>
          Open in Artifacts
        </a>
      )}
    </div>
  );
}

const baseName = (p: unknown) => (typeof p === "string" ? (p.replace(/\/+$/, "").split("/").pop() ?? p) : "");

/** A `publish` call that went through: the file the agent published. */
export function PublishCard({ item }: { item: Extract<Item, { type: "tool" }> }) {
  const ctx = useContext(ChatArtifactsCtx);
  const s = item.staged;
  const name = s?.name ?? (item.input?.name || baseName(item.input?.path));
  return <StagedFile label="Published" name={name} size={s?.size ?? null} row={ctx?.find(item.call_id)} gaveUp={!!ctx?.gaveUp} />;
}

/** A file the user saved from this chat. */
export function SavedItem({ staged }: { staged: StagedArtifact }) {
  const ctx = useContext(ChatArtifactsCtx);
  return <StagedFile label="Saved" name={staged.name} size={staged.size} row={ctx?.find(staged.stage_id)} gaveUp={!!ctx?.gaveUp} />;
}

/** Ask the chat's node to copy a file into Artifacts. Without `path`, a small
 * form asks for one (relative to the chat's folder) and an optional name. */
export function SaveAsArtifact(props: { path?: string; onDone?: () => void; autoFocus?: boolean }) {
  const ctx = useContext(ChatArtifactsCtx);
  const [open, setOpen] = useState(props.autoFocus ?? false);
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ text: string; bad: boolean } | null>(null);
  if (!ctx?.saveable) return null;
  const save = async (p: string) => {
    if (!p.trim()) return;
    setBusy(true);
    setNote(null);
    try {
      await artifactsApi.save(ctx.sessionId, p.trim(), name);
      setNote({ text: "Saving… it shows up in this chat once copied.", bad: false });
      setPath("");
      setName("");
      props.onDone?.();
    } catch (e) {
      setNote({ text: e instanceof Error ? e.message : String(e), bad: true });
    } finally {
      setBusy(false);
    }
  };
  const status = note && (
    <span className={`small ${note.bad ? "error-text" : "muted"}`} role="status">
      {note.text}
    </span>
  );
  if (props.path) {
    return (
      <div className="save-artifact row">
        <button type="button" className="link small" disabled={busy} onClick={() => void save(props.path!)}>
          Save as artifact
        </button>
        {status}
      </div>
    );
  }
  return (
    <div className="save-artifact">
      {open ? (
        <form
          className="row save-artifact-form"
          onSubmit={(e) => {
            e.preventDefault();
            void save(path);
          }}
        >
          <input
            aria-label="File or folder to save"
            placeholder="Path in the chat's folder, e.g. target/release/app"
            value={path}
            autoFocus={props.autoFocus}
            onChange={(e) => setPath(e.target.value)}
          />
          <input aria-label="Artifact name (optional)" placeholder="Name (optional)" value={name} maxLength={80} onChange={(e) => setName(e.target.value)} />
          <button className="primary" disabled={busy || !path.trim()}>
            Save as artifact
          </button>
        </form>
      ) : (
        <button type="button" className="link small" onClick={() => setOpen(true)}>
          Save a file as artifact…
        </button>
      )}
      {status}
    </div>
  );
}
