// One Media item, large (docs/media.md §6.8): a modal dialog (focus kept
// inside, Esc closes, ←/→ step through the grid) with the picture, player
// or a Download, and beside it the name to rename, what the file is, where
// it came from, the artifact an agent's item also lives in, "Use in chat",
// and Delete (with "Delete from both" for an agent's item, and what stays in
// the chats an image went into). Its own lazy chunk, shared with chats. An
// image too large to decode safely, a HEIC the browser cannot show and a MOV
// it cannot play all fall back to Download.
import { ChevronLeft, ChevronRight, Download, File, Film, Image as ImageIcon, Music, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { bytes } from "../lib/bytes";
import {
  artifactName,
  clockTime,
  contentUrl,
  deleteMedia,
  KIND_LABEL,
  type MediaItem,
  mediaApi,
  originLine,
  safeToDecode,
  stillIn,
} from "../lib/media";
import { artifactHash } from "../lib/routes";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const ICON = { image: ImageIcon, video: Film, audio: Music, other: File };

/** Why the stage shows a Download instead of the file, or null to show it. */
export function fallback(m: MediaItem, broken: boolean): string | null {
  if (m.display === "download") return "This file can only be downloaded.";
  if (m.kind === "image" && !safeToDecode(m)) {
    return m.width && m.height ? "This image is too large to show here safely." : "This image's size is unknown, so it is not shown here.";
  }
  if (broken) {
    if (m.kind === "video") return "This browser can't play this video.";
    if (m.kind === "audio") return "This browser can't play this audio.";
    return m.format === "heic" || m.format === "heif" ? "This browser can't show HEIC images." : "This browser can't show this image.";
  }
  return null;
}

export default function MediaPreview(props: {
  item: MediaItem;
  onPrev?: () => void;
  onNext?: () => void;
  onClose: () => void;
  onChanged: (m: MediaItem) => void;
  /** `note`: the item went, but something it asked for did not (an artifact version that stayed). */
  onDeleted: (id: string, note: string | null) => void;
  /** "Use in chat": offered only for an image a message can carry. */
  onUse?: () => void;
}) {
  const m = props.item;
  const dialog = useRef<HTMLDialogElement>(null);
  const [broken, setBroken] = useState(false);
  const [full, setFull] = useState(false);
  const [name, setName] = useState(m.name);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  useEffect(() => setName(m.name), [m.name]);
  // ←/→ swap the item under the same open dialog.
  useEffect(() => {
    setBroken(false);
    setFull(false);
    setError(null);
    setConfirming(false);
  }, [m.media_id]);

  useEffect(() => {
    const d = dialog.current;
    if (!d) return;
    // A modal dialog keeps focus inside and the page behind inert.
    if (typeof d.showModal === "function" && !d.open) d.showModal();
    else d.setAttribute("open", "");
    return () => {
      if (d.open && typeof d.close === "function") d.close();
    };
  }, []);

  const keys = (e: React.KeyboardEvent) => {
    const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
    if (typing || e.altKey || e.ctrlKey || e.metaKey) return;
    if (e.key === "ArrowLeft" && props.onPrev) {
      e.preventDefault();
      props.onPrev();
    } else if (e.key === "ArrowRight" && props.onNext) {
      e.preventDefault();
      props.onNext();
    }
  };

  const rename = async () => {
    const next = name.trim();
    if (!next || next === m.name) return;
    setError(null);
    try {
      props.onChanged(await mediaApi.update(m.media_id, { name: next }));
    } catch (e) {
      setError(message(e));
    }
  };
  const remove = async (both: boolean) => {
    try {
      props.onDeleted(m.media_id, await deleteMedia([m], both));
    } catch (e) {
      setError(message(e));
    }
  };

  const why = fallback(m, broken);
  const Icon = ICON[m.kind] ?? File;
  const download = (
    <a className="button-like row" href={contentUrl(m.media_id, true)} download={m.name}>
      <Download size={16} aria-hidden="true" /> Download
    </a>
  );
  return (
    <dialog
      ref={dialog}
      className="media-preview"
      aria-label={m.name}
      onCancel={(e) => {
        e.preventDefault();
        props.onClose();
      }}
      onKeyDown={keys}
    >
      <div className="media-stage">
        {why ? (
          <div className="media-fallback stack">
            <Icon size={48} aria-hidden="true" />
            <p className="muted">{why}</p>
            {download}
          </div>
        ) : m.kind === "image" ? (
          <img
            src={contentUrl(m.media_id)}
            alt={m.name}
            className={full ? "full" : "fit"}
            title={full ? "Fit to the window" : "Actual size"}
            onClick={() => setFull(!full)}
            onError={() => setBroken(true)}
          />
        ) : m.kind === "video" ? (
          <video src={contentUrl(m.media_id)} controls preload="metadata" onError={() => setBroken(true)} />
        ) : (
          <div className="media-fallback stack">
            <Music size={48} aria-hidden="true" />
            <audio src={contentUrl(m.media_id)} controls preload="metadata" onError={() => setBroken(true)} />
          </div>
        )}
        {props.onPrev && (
          <button type="button" className="icon media-prev" aria-label="Previous" onClick={props.onPrev}>
            <ChevronLeft size={20} />
          </button>
        )}
        {props.onNext && (
          <button type="button" className="icon media-next" aria-label="Next" onClick={props.onNext}>
            <ChevronRight size={20} />
          </button>
        )}
      </div>
      <aside className="media-side stack">
        <div className="row">
          <h2 className="grow ellipsis">{m.name}</h2>
          <button type="button" className="icon" aria-label="Close" onClick={props.onClose}>
            <X size={18} />
          </button>
        </div>
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            void rename();
          }}
        >
          <input aria-label="Name" value={name} maxLength={200} onChange={(e) => setName(e.target.value)} />
          <button disabled={!name.trim() || name.trim() === m.name}>Rename</button>
        </form>
        <dl className="media-facts small">
          <dt>Kind</dt>
          <dd>
            {KIND_LABEL[m.kind]}, {m.format.toUpperCase()}
          </dd>
          <dt>Size</dt>
          <dd>{bytes(m.size)}</dd>
          {m.width && m.height ? (
            <>
              <dt>Dimensions</dt>
              <dd>
                {m.width} × {m.height}
              </dd>
            </>
          ) : null}
          {m.duration_ms ? (
            <>
              <dt>Length</dt>
              <dd>{clockTime(m.duration_ms)}</dd>
            </>
          ) : null}
          <dt>Added</dt>
          <dd>{new Date(m.created_ms).toLocaleString()}</dd>
          <dt>Origin</dt>
          <dd>
            {m.source && m.source !== "deleted" ? (
              <a href={`#/s/${m.source.session_id}`}>{originLine(m)}</a>
            ) : m.source === "deleted" ? (
              "From a deleted chat"
            ) : (
              originLine(m)
            )}
          </dd>
          {m.artifact && (
            <>
              <dt>Artifacts</dt>
              <dd>
                <a href={artifactHash(m.artifact.artifact_id)}>Also in Artifacts: {artifactName(m.artifact)}</a>
              </dd>
            </>
          )}
        </dl>
        {error && (
          <div className="error-text" role="alert">
            {error}
          </div>
        )}
        {confirming ? (
          <div className="media-confirm stack" role="group" aria-label="Delete">
            <p>Delete {m.name} from Media?</p>
            {m.chats > 0 && <p className="small muted">{stillIn(m.chats)}</p>}
            {m.artifact && (
              <p className="small muted">The file stays in Artifacts ({artifactName(m.artifact)}); delete it there to free space.</p>
            )}
            <div className="row end">
              <button type="button" autoFocus onClick={() => setConfirming(false)}>
                Cancel
              </button>
              <button type="button" className="danger" onClick={() => void remove(false)}>
                Delete
              </button>
              {m.artifact && (
                <button type="button" className="danger" onClick={() => void remove(true)}>
                  Delete from both
                </button>
              )}
            </div>
          </div>
        ) : (
          <div className="row">
            {download}
            {props.onUse && (
              <button type="button" onClick={props.onUse}>
                Use in chat
              </button>
            )}
            <span className="grow" />
            <button type="button" className="danger" onClick={() => setConfirming(true)}>
              Delete
            </button>
          </div>
        )}
      </aside>
    </dialog>
  );
}
