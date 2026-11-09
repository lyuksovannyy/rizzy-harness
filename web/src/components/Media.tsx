// Sidebar → Media (docs/media.md): the account's own images, video and audio
// in this workspace, as a grid. Upload (button or drop, with progress),
// filter by kind (the route) and origin, search names, select several to
// delete (an agent's items can go from Artifacts too), and open one in the preview (MediaPreview, its own chunk, which
// chats use too). Thumbnails are made by this browser (lib/thumbs.ts) as
// tiles scroll into view. A lazy chunk; the strings live here.
import { Bot, File, Film, Image as ImageIcon, Music, Play, Upload } from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { HttpError } from "../api";
import { bytes } from "../lib/bytes";
import {
  chatReady,
  clockTime,
  deleteMedia,
  FILTER_KIND,
  KIND_LABEL,
  type MediaItem,
  mediaApi,
  type MyStorage,
  PENDING_IMAGES,
  storageApi,
  thumbUrl,
  upload,
} from "../lib/media";
import { type MediaFilter, mediaHash } from "../lib/routes";
import { queueThumb, wantsThumb } from "../lib/thumbs";
import { when } from "../lib/when";
import type { MediaPageProps } from "./pages";
import { Seg } from "./Seg";

const MediaPreview = lazy(() => import("./MediaPreview"));

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
/** Typing settles before the names are searched. */
const SEARCH_MS = 250;

const FILTERS: [MediaFilter | "all", string][] = [
  ["all", "All"],
  ["images", "Images"],
  ["videos", "Videos"],
  ["audio", "Audio"],
  ["other", "Other"],
];

const EMPTY: Record<MediaFilter, string> = {
  images: "No images here yet.",
  videos: "No videos here yet.",
  audio: "No audio here yet.",
  other: "No other files here yet.",
};

/** One file on its way up. */
interface Sending {
  key: string;
  name: string;
  share: number;
  error?: string;
  note?: string;
}

/** What a tile says to a screen reader: "beach.jpg, Image, 2.3 MB, added 3 days ago". */
export function tileLabel(m: MediaItem, now: number): string {
  const added = when(m.created_ms, now).replace(/^(Yesterday|Now)$/i, (w) => w.toLowerCase());
  return `${m.name}, ${KIND_LABEL[m.kind]}, ${bytes(m.size)}, added ${added}${m.origin === "agent" ? ", published by an agent" : ""}`;
}

/** "Use in chat": the id waits in this tab for the home bar (at most the 4 a message takes). */
export function keepForChat(id: string): void {
  try {
    const ids = JSON.parse(sessionStorage.getItem(PENDING_IMAGES) ?? "[]") as string[];
    sessionStorage.setItem(PENDING_IMAGES, JSON.stringify([...ids.filter((x) => x !== id), id].slice(-4)));
  } catch {
    /* storage unavailable: the bar starts empty */
  }
}

/** The empty grid names only sources that exist: agents publish once Artifacts shows. */
export const emptyText = (artifacts: boolean) =>
  artifacts ? "Nothing here yet. Upload images, video or music, or ask an agent to publish a file it made." : "Nothing here yet. Upload images, video or music.";

export default function Media(props: MediaPageProps) {
  const { scope, filter, id, navigate, live } = props;
  const [items, setItems] = useState<MediaItem[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The server answers 404 while Media is turned off.
  const [off, setOff] = useState(false);
  const [origin, setOrigin] = useState<MediaItem["origin"] | "">("");
  const [typed, setTyped] = useState("");
  const [q, setQ] = useState("");
  const [usage, setUsage] = useState<MyStorage | null>(null);
  const [maxFile, setMaxFile] = useState(props.features.media.max_file);
  const [sending, setSending] = useState<Sending[]>([]);
  const [selecting, setSelecting] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirming, setConfirming] = useState(false);
  // What a "Delete from both" could not delete (kept apart from load errors, which a reload clears).
  const [leftOver, setLeftOver] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  // An item a link opened that the loaded page does not hold.
  const [extra, setExtra] = useState<MediaItem | null>(null);
  const picker = useRef<HTMLInputElement>(null);
  // Opened from a tile here, so closing goes Back instead of adding an entry.
  const pushed = useRef(false);

  useEffect(() => {
    const t = setTimeout(() => setQ(typed), SEARCH_MS);
    return () => clearTimeout(t);
  }, [typed]);

  const kind = filter ? FILTER_KIND[filter] : null;
  const load = useCallback(() => {
    mediaApi
      .list({ workspace: scope, kind, origin: origin || null, q })
      .then((p) => {
        setItems(p.items);
        setNext(p.next);
        setMaxFile(p.limits.max_file);
        setError(null);
      })
      .catch((e) => {
        if (e instanceof HttpError && e.status === 404) setOff(true);
        else setError(message(e));
      });
    storageApi
      .mine()
      .then(setUsage)
      .catch(() => {});
  }, [scope, kind, origin, q]);
  useEffect(() => {
    load();
    return live.onTopic("media", load);
  }, [load, live]);

  const more = async () => {
    try {
      const p = await mediaApi.list({ workspace: scope, kind, origin: origin || null, q, before: next });
      setItems((old) => [...(old ?? []), ...p.items]);
      setNext(p.next);
    } catch (e) {
      setError(message(e));
    }
  };

  /** An item changed here (a thumbnail made, a rename): the grid follows. */
  const patch = useCallback((m: Partial<MediaItem> & { media_id: string }) => {
    setItems((old) => old && old.map((x) => (x.media_id === m.media_id ? { ...x, ...m } : x)));
    setExtra((x) => (x && x.media_id === m.media_id ? { ...x, ...m } : x));
  }, []);
  const thumbDone = useCallback(
    (media_id: string) => (r: "made" | "none" | "failed") => patch(r === "made" ? { media_id, has_thumb: true } : { media_id, no_thumb: true }),
    [patch],
  );
  const onVisible = useCallback((m: MediaItem) => queueThumb(m, thumbDone(m.media_id)), [thumbDone]);

  const send = async (files: File[]) => {
    const jobs = files.map((f, i) => ({ f, key: `${Date.now()}-${i}-${f.name}` }));
    setSending((s) => [...s, ...jobs.map(({ f, key }) => ({ key, name: f.name, share: 0 }))]);
    const set = (key: string, v: Partial<Sending>) => setSending((s) => s.map((x) => (x.key === key ? { ...x, ...v } : x)));
    // One at a time: the server takes two per account and refuses a third.
    for (const { f, key } of jobs) {
      if (maxFile && f.size > maxFile) {
        set(key, { error: `Too large: the limit is ${bytes(maxFile)}.` });
        continue;
      }
      try {
        const { item, duplicate } = await upload(f, scope, (share) => set(key, { share }));
        if (duplicate) set(key, { share: 1, note: "Already in Media." });
        else setSending((s) => s.filter((x) => x.key !== key));
        if (!duplicate) {
          setItems((old) => (old && (!kind || item.kind === kind) ? [item, ...old] : old));
          queueThumb(item, thumbDone(item.media_id));
        }
      } catch (e) {
        set(key, { error: message(e) });
      }
    }
    load();
  };

  const chosen = (items ?? []).filter((m) => selected.has(m.media_id));
  const fromAgents = chosen.filter((m) => m.artifact).length;
  const inChats = chosen.filter((m) => m.chats > 0).length;
  const remove = async (both: boolean) => {
    setConfirming(false);
    try {
      setLeftOver(await deleteMedia(chosen, both));
      setSelected(new Set());
      setSelecting(false);
    } catch (e) {
      setError(message(e));
    }
    load();
  };

  const shown = items ?? [];
  const open = id ? (shown.find((m) => m.media_id === id) ?? (extra?.media_id === id ? extra : null)) : null;
  // A link to an item that is gone (or not this account's) lands on the grid.
  const leave = useRef(() => navigate({ kind: "media", filter, id: null }));
  useEffect(() => {
    leave.current = () => navigate({ kind: "media", filter, id: null });
  });
  useEffect(() => {
    if (!id || !items || items.some((m) => m.media_id === id) || extra?.media_id === id) return;
    mediaApi
      .get(id)
      .then(setExtra)
      .catch(() => leave.current());
  }, [id, items, extra]);
  const at = open ? shown.findIndex((m) => m.media_id === open.media_id) : -1;
  // ←/→ swap the item in place: the preview is one stop in history.
  const move = (step: number) => {
    const to = shown[at + step];
    if (to) location.replace(mediaHash(filter, to.media_id));
  };
  const close = () => {
    if (pushed.current) history.back();
    else navigate({ kind: "media", filter, id: null });
    pushed.current = false;
  };

  if (off) {
    return (
      <Page>
        <h1>Media</h1>
        <p className="muted">Media is not turned on on this server.</p>
      </Page>
    );
  }
  const filtered = !!(kind || origin || q);
  const now = Date.now();
  return (
    <Page
      dragging={dragging}
      onDragOver={(e) => {
        if (!e.dataTransfer.types.includes("Files")) return;
        e.preventDefault();
        setDragging(true);
      }}
      onDragLeave={(e) => e.currentTarget === e.target && setDragging(false)}
      onDrop={(e) => {
        e.preventDefault();
        setDragging(false);
        const files = [...e.dataTransfer.files];
        if (files.length) void send(files);
      }}
    >
      <div className="agents-head media-head">
        <h1>Media</h1>
        {usage && <Usage usage={usage} />}
        <button type="button" className="primary row" onClick={() => picker.current?.click()}>
          <Upload size={16} aria-hidden="true" /> Upload
        </button>
        <input
          ref={picker}
          type="file"
          multiple
          hidden
          accept="image/*,video/*,audio/*"
          aria-label="Files to upload"
          onChange={(e) => {
            const files = [...(e.target.files ?? [])];
            e.target.value = "";
            if (files.length) void send(files);
          }}
        />
      </div>
      <div className="row media-filters">
        <Seg
          label="Show"
          value={filter ?? "all"}
          options={FILTERS.map(([value, label]) => ({ value, label }))}
          onChange={(v) => navigate({ kind: "media", filter: v === "all" ? null : v, id: null })}
          wrap
        />
        <select aria-label="Origin" value={origin} onChange={(e) => setOrigin(e.target.value as MediaItem["origin"] | "")}>
          <option value="">Every origin</option>
          <option value="upload">Uploaded</option>
          <option value="chat">From chats</option>
          <option value="agent">From agents</option>
        </select>
        <input type="search" className="grow" aria-label="Search names" placeholder="Search names" value={typed} onChange={(e) => setTyped(e.target.value)} />
        {shown.length > 0 && (
          <button
            type="button"
            aria-pressed={selecting}
            onClick={() => {
              setSelecting(!selecting);
              setSelected(new Set());
              setConfirming(false);
            }}
          >
            {selecting ? "Done" : "Select"}
          </button>
        )}
      </div>
      {selecting && (
        <div className="row media-selection">
          {confirming ? (
            <>
              <span className="grow" role="status">
                Delete {chosen.length === 1 ? "this item" : `these ${chosen.length} items`} from Media?
                {fromAgents > 0 &&
                  ` ${fromAgents === 1 ? "One came from an agent and stays" : `${fromAgents} came from agents and stay`} in Artifacts; delete there to free space.`}
                {inChats > 0 &&
                  ` ${inChats === 1 ? "One is" : `${inChats} are`} still in chats; delete those chats to remove them there. Copies already sent to the model provider can't be recalled.`}
              </span>
              <button type="button" onClick={() => setConfirming(false)}>
                Cancel
              </button>
              <button type="button" className="danger" onClick={() => void remove(false)}>
                Delete
              </button>
              {fromAgents > 0 && (
                <button type="button" className="danger" onClick={() => void remove(true)}>
                  Delete from both
                </button>
              )}
            </>
          ) : (
            <>
              <span className="grow" role="status">
                {selected.size === 1 ? "1 selected" : `${selected.size} selected`}
              </span>
              <button type="button" className="danger" disabled={selected.size === 0} onClick={() => setConfirming(true)}>
                Delete
              </button>
            </>
          )}
        </div>
      )}
      {sending.length > 0 && (
        <ul className="media-uploads" aria-label="Uploads">
          {sending.map((s) => (
            <li key={s.key} className="row">
              <span className="ellipsis grow">{s.name}</span>
              {s.error ? (
                <span className="error-text">{s.error}</span>
              ) : s.note ? (
                <span className="muted small">{s.note}</span>
              ) : (
                <progress max={1} value={s.share} aria-label={`Uploading ${s.name}`} />
              )}
              {(s.error || s.note) && (
                <button type="button" className="link small" onClick={() => setSending((all) => all.filter((x) => x.key !== s.key))}>
                  Dismiss
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {error && <div className="error-text">{error}</div>}
      {leftOver && (
        <div className="error-text" role="status">
          {leftOver}
        </div>
      )}
      {!items && !error && <div className="muted">Loading…</div>}
      {items && shown.length === 0 && (
        <div className="media-empty muted">
          {q ? `Nothing matches “${q}”.` : filtered && filter ? EMPTY[filter] : filtered ? "Nothing here with that origin." : emptyText(props.features.artifacts.show)}
        </div>
      )}
      {shown.length > 0 && (
        <Grid
          items={shown}
          now={now}
          selecting={selecting}
          selected={selected}
          onOpen={(m) => {
            pushed.current = true;
            navigate({ kind: "media", filter, id: m.media_id });
          }}
          onToggle={(m) =>
            setSelected((s) => {
              const n = new Set(s);
              if (!n.delete(m.media_id)) n.add(m.media_id);
              return n;
            })
          }
          onVisible={onVisible}
        />
      )}
      {next && (
        <button type="button" className="routine-more" onClick={() => void more()}>
          Load more
        </button>
      )}
      {open && (
        <Suspense fallback={null}>
          <MediaPreview
            item={open}
            onPrev={at > 0 ? () => move(-1) : undefined}
            onNext={at >= 0 && at < shown.length - 1 ? () => move(1) : undefined}
            onClose={close}
            onChanged={patch}
            onUse={
              props.features.media.vision_images && chatReady(open)
                ? () => {
                    keepForChat(open.media_id);
                    navigate({ kind: "home" });
                  }
                : undefined
            }
            onDeleted={(_id, note) => {
              setLeftOver(note);
              close();
              load();
            }}
          />
        </Suspense>
      )}
    </Page>
  );
}

function Page(props: {
  children: React.ReactNode;
  dragging?: boolean;
  onDragOver?: React.DragEventHandler;
  onDragLeave?: React.DragEventHandler;
  onDrop?: React.DragEventHandler;
}) {
  return (
    <section
      className={`agents-page media-page ${props.dragging ? "dragging" : ""}`}
      onDragOver={props.onDragOver}
      onDragLeave={props.onDragLeave}
      onDrop={props.onDrop}
    >
      <div className="column wide">{props.children}</div>
    </section>
  );
}

function Usage({ usage }: { usage: MyStorage }) {
  const text = usage.quota ? `${bytes(usage.used)} of ${bytes(usage.quota)} used` : `${bytes(usage.used)} used`;
  return (
    <span className="media-usage small muted" title="Media and Artifacts share this space">
      {usage.quota > 0 && <meter min={0} max={usage.quota} value={usage.used} aria-hidden="true" />}
      <span className="meter">{text}</span>
    </span>
  );
}

function Grid(props: {
  items: MediaItem[];
  now: number;
  selecting: boolean;
  selected: Set<string>;
  onOpen: (m: MediaItem) => void;
  onToggle: (m: MediaItem) => void;
  onVisible: (m: MediaItem) => void;
}) {
  const grid = useRef<HTMLUListElement>(null);
  const { items, onVisible } = props;
  // Thumbnails are made as tiles come into view, not for the whole library.
  useEffect(() => {
    const byId = new Map(items.filter(wantsThumb).map((m) => [m.media_id, m]));
    if (!byId.size) return;
    if (typeof IntersectionObserver === "undefined") {
      byId.forEach((m) => onVisible(m));
      return;
    }
    const io = new IntersectionObserver((entries) => {
      for (const e of entries) {
        const m = e.isIntersecting ? byId.get((e.target as HTMLElement).dataset.id ?? "") : undefined;
        if (m) {
          io.unobserve(e.target);
          onVisible(m);
        }
      }
    });
    grid.current?.querySelectorAll<HTMLElement>("[data-id]").forEach((el) => byId.has(el.dataset.id!) && io.observe(el));
    return () => io.disconnect();
  }, [items, onVisible]);
  return (
    <ul className="media-grid" ref={grid}>
      {props.items.map((m) => (
        <li key={m.media_id} data-id={m.media_id}>
          <button
            type="button"
            className={`media-tile kind-${m.kind} ${props.selected.has(m.media_id) ? "selected" : ""}`}
            aria-label={tileLabel(m, props.now)}
            aria-pressed={props.selecting ? props.selected.has(m.media_id) : undefined}
            onClick={() => (props.selecting ? props.onToggle(m) : props.onOpen(m))}
          >
            <Tile m={m} />
          </button>
        </li>
      ))}
    </ul>
  );
}

/** A tile's face: the thumbnail, or the kind's icon with what helps tell files apart. */
export function Tile({ m }: { m: MediaItem }) {
  const [broken, setBroken] = useState(false);
  const pictured = m.has_thumb && !broken;
  const length = m.duration_ms ? clockTime(m.duration_ms) : null;
  const agent = m.origin === "agent" && (
    <span className="media-agent" aria-hidden="true" title="Published by an agent">
      <Bot size={12} />
    </span>
  );
  if (m.kind === "image" || m.kind === "video") {
    return (
      <>
        {agent}
        {pictured ? (
          <img src={thumbUrl(m.media_id)} alt="" loading="lazy" onError={() => setBroken(true)} />
        ) : (
          <span className="media-icon" aria-hidden="true">
            {m.kind === "image" ? <ImageIcon size={28} /> : <Film size={28} />}
          </span>
        )}
        {m.kind === "video" && (
          <span className="media-badge" aria-hidden="true">
            <Play size={12} /> {length}
          </span>
        )}
      </>
    );
  }
  return (
    <span className="media-icon" aria-hidden="true">
      {agent}
      {m.kind === "audio" ? <Music size={28} /> : <File size={28} />}
      <span className="media-name small ellipsis">{m.name}</span>
      <span className="small muted">{m.kind === "audio" ? length : m.format.toUpperCase()}</span>
    </span>
  );
}
