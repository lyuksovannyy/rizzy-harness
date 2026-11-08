// The search panel (docs/sidebar-search.md), a lazy chunk: a modal dialog
// over everything, opened from the sidebar, Ctrl/Cmd+K or "/". One input,
// a tab per kind, and a listbox the arrow keys move through while focus
// stays in the input. Nothing typed lists the newest items.
import { CalendarClock, File, Film, Image, MessageSquare, Music, Package, Terminal, X } from "lucide-react";
import { type KeyboardEvent, type ReactNode, useEffect, useId, useRef, useState } from "react";
import { api } from "../api";
import { fill, t } from "../lib/i18n";
import { ProjectIcon } from "../lib/projectIcons";
import type { ProjectSummary } from "../lib/projects";
import type { Route } from "../lib/routes";
import { when } from "../lib/when";
import type { SearchKind, SearchResult } from "../types";
import { Highlight } from "./Highlight";

export type Tab = "all" | SearchKind;

// The panel's own words live in its chunk, not in the first load's catalog
// (lib/i18n.ts says why the catalog is small).
const TEXT = {
  label: "Search",
  close: "Close search",
  kinds: "What to search",
  none: "No results",
  one: "1 result",
  many: "{n} results",
};
const TAB_LABEL: Record<Tab, string> = { all: "All", chats: "Chats", media: "Media", artifacts: "Artifacts", projects: "Projects" };
/** What each result is, as its row says. */
export const TYPE_LABEL: Record<SearchResult["type"], string> = {
  chat: "Chat",
  agentic: "Agentic chat",
  routine: "Routine run",
  project: "Project",
  image: "Image",
  video: "Video",
  audio: "Audio",
  media_other: "File",
  binary: "Program",
  package: "Package",
  archive: "Archive",
  document: "Document",
  other: "File",
};

/** The tabs in the order they show; only those the viewer has do. */
const ORDER: Tab[] = ["all", "chats", "media", "artifacts", "projects"];
const TAB_KEY = "harness.searchTab";
const DEBOUNCE_MS = 150;

/** The last tab used, kept across openings; All until there is one. */
function readTab(): Tab {
  try {
    const v = localStorage.getItem(TAB_KEY) as Tab | null;
    return v && ORDER.includes(v) ? v : "all";
  } catch {
    return "all";
  }
}

function saveTab(tab: Tab) {
  try {
    localStorage.setItem(TAB_KEY, tab);
  } catch {
    /* storage unavailable */
  }
}

/** Where a result opens. */
export function resultRoute(r: SearchResult): Route {
  if (r.kind === "chat") return { kind: "session", id: r.id };
  if (r.kind === "project") return { kind: "project", id: r.id };
  if (r.kind === "media") return { kind: "media", filter: null, id: r.id };
  return { kind: "artifacts", id: r.id };
}

const KIND_ICON: Partial<Record<SearchResult["type"], ReactNode>> = {
  image: <Image size={16} />,
  video: <Film size={16} />,
  audio: <Music size={16} />,
  binary: <Package size={16} />,
  package: <Package size={16} />,
  archive: <Package size={16} />,
};

/** Whether a thumbnail is a path on this origin (T14): never a URL, never
 * protocol-relative. */
export const sameOrigin = (thumb: string) => thumb.startsWith("/") && !thumb.startsWith("//") && !thumb.startsWith("/\\");

/** A result's leading icon: its thumbnail until that fails to load (the
 * file was deleted since, say), else the project's icon or the type's. */
function RowIcon({ r, projects }: { r: SearchResult; projects: ProjectSummary[] }) {
  const [broken, setBroken] = useState(false);
  if (r.thumb && sameOrigin(r.thumb) && !broken) {
    return <img className="search-thumb" src={r.thumb} alt="" width={32} height={32} loading="lazy" onError={() => setBroken(true)} />;
  }
  return icon(r, projects);
}

function icon(r: SearchResult, projects: ProjectSummary[]): ReactNode {
  const pid = r.kind === "project" ? r.id : r.project_id;
  const p = pid ? projects.find((x) => x.project_id === pid) : undefined;
  if (p) return <ProjectIcon icon={p.icon} accent={p.accent} />;
  if (r.type === "routine") return <CalendarClock size={16} />;
  if (r.type === "agentic") return <Terminal size={16} />;
  if (r.kind === "chat") return <MessageSquare size={16} />;
  return KIND_ICON[r.type] ?? <File size={16} />;
}

interface Page {
  /** The tab and query it answers. */
  key: string;
  results: SearchResult[];
  next: string | null;
}

export default function SearchPanel(props: {
  /** The Shell's workspace: the one searched. */
  workspace: string | null;
  /** The tabs to start with (`features.search.kinds`); each answer updates them. */
  kinds: SearchKind[];
  /** For the icons of projects and of chats in them. */
  projects: ProjectSummary[];
  timeZone?: string | null;
  onOpen: (r: Route) => void;
  onClose: () => void;
}) {
  const { workspace } = props;
  const dialog = useRef<HTMLDialogElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const sentinel = useRef<HTMLLIElement>(null);
  const ids = useId();
  const [q, setQ] = useState("");
  const [kinds, setKinds] = useState(props.kinds);
  const [tab, setTab] = useState(readTab);
  const [page, setPage] = useState<Page | null>(null);
  const [active, setActive] = useState(0);
  const tabs = ORDER.filter((k) => k === "all" || kinds.includes(k));
  const current = tabs.includes(tab) ? tab : "all";
  const query = q.trim();
  const key = `${current}\n${query}`;
  const results = page?.results ?? [];
  const optionId = (i: number) => `${ids}-o${i}`;

  useEffect(() => {
    dialog.current?.showModal?.();
    input.current?.focus();
  }, []);

  // Each change of query or tab asks again, once typing pauses; a newer
  // request aborts the one before.
  useEffect(() => {
    const ctl = new AbortController();
    const timer = setTimeout(() => {
      api
        .search(query, { in: current, workspace, signal: ctl.signal })
        .then((r) => {
          setKinds(r.kinds);
          setPage({ key, results: r.results, next: r.next });
          setActive(0);
        })
        .catch(() => {});
    }, DEBOUNCE_MS);
    return () => {
      clearTimeout(timer);
      ctl.abort();
    };
  }, [key, query, current, workspace]);

  // A tab's next page loads when the end of the list scrolls into view (All
  // has none).
  const next = page?.key === key ? page.next : null;
  useEffect(() => {
    const el = sentinel.current;
    if (!el || !next || current === "all" || typeof IntersectionObserver === "undefined") return;
    const ctl = new AbortController();
    const io = new IntersectionObserver(
      (seen) => {
        if (!seen.some((x) => x.isIntersecting)) return;
        io.disconnect();
        api
          .search(query, { in: current, cursor: next, workspace, signal: ctl.signal })
          .then((r) => setPage((p) => (p && p.key === key ? { key, results: [...p.results, ...r.results], next: r.next } : p)))
          .catch(() => {});
      },
      { root: list.current },
    );
    io.observe(el);
    return () => {
      io.disconnect();
      ctl.abort();
    };
  }, [next, key, query, current, workspace]);

  useEffect(() => {
    document.getElementById(optionId(active))?.scrollIntoView?.({ block: "nearest" });
  });

  const choose = (k: Tab) => {
    setTab(k);
    saveTab(k);
  };
  const open = (r: SearchResult | undefined) => r && props.onOpen(resultRoute(r));
  const onInputKey = (e: KeyboardEvent) => {
    const n = results.length;
    const to =
      e.key === "ArrowDown" ? Math.min(active + 1, n - 1) : e.key === "ArrowUp" ? Math.max(active - 1, 0) : e.key === "Home" ? 0 : e.key === "End" ? n - 1 : null;
    if (to !== null && n > 0) {
      e.preventDefault();
      setActive(to);
    } else if (e.key === "Enter") {
      e.preventDefault();
      open(results[active]);
    }
  };
  // Roving tabindex with automatic activation.
  const onTabKey = (e: KeyboardEvent) => {
    const i = tabs.indexOf(current);
    const to =
      e.key === "ArrowRight" ? (i + 1) % tabs.length : e.key === "ArrowLeft" ? (i - 1 + tabs.length) % tabs.length : e.key === "Home" ? 0 : e.key === "End" ? tabs.length - 1 : -1;
    if (to < 0) return;
    e.preventDefault();
    choose(tabs[to]!);
    document.getElementById(`${ids}-t${tabs[to]}`)?.focus();
  };
  const now = Date.now();
  const status = page === null ? "" : results.length === 0 ? TEXT.none : results.length === 1 ? TEXT.one : fill(TEXT.many, { n: String(results.length) });

  return (
    <dialog
      ref={dialog}
      className="search-panel"
      aria-label={TEXT.label}
      onClose={props.onClose}
      onKeyDown={(e) => {
        // Pressed again while open: select what is typed.
        if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
          e.preventDefault();
          input.current?.select();
        }
      }}
      // A click on the backdrop lands on the dialog itself.
      onClick={(e) => e.target === dialog.current && dialog.current.close()}
    >
      <div className="search-box">
        <div className="search-top">
          <input
            ref={input}
            className="search-input"
            role="combobox"
            aria-expanded="true"
            aria-autocomplete="list"
            aria-controls={`${ids}-list`}
            aria-activedescendant={results.length ? optionId(active) : undefined}
            aria-label={TEXT.label}
            placeholder={TEXT.label}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={onInputKey}
          />
          <button type="button" className="icon search-close" aria-label={TEXT.close} onClick={() => dialog.current?.close()}>
            <X size={18} />
          </button>
        </div>
        <div role="tablist" aria-label={TEXT.kinds} className="search-tabs">
          {tabs.map((k) => (
            <button
              key={k}
              id={`${ids}-t${k}`}
              type="button"
              role="tab"
              aria-selected={k === current}
              aria-controls={`${ids}-list`}
              tabIndex={k === current ? 0 : -1}
              className={k === current ? "on" : ""}
              onClick={() => choose(k)}
              onKeyDown={onTabKey}
            >
              {TAB_LABEL[k]}
            </button>
          ))}
        </div>
        <div className="sr-only" aria-live="polite">
          {status}
        </div>
        <ul ref={list} id={`${ids}-list`} role="listbox" aria-label={TEXT.label} className="search-list">
          {results.map((r, i) => (
            <li
              key={`${r.kind}:${r.id}`}
              id={optionId(i)}
              role="option"
              aria-selected={i === active}
              className={`search-row${i === active ? " on" : ""}`}
              onClick={() => open(r)}
              onPointerMove={() => i !== active && setActive(i)}
            >
              <span className="search-icon">
                <RowIcon r={r} projects={props.projects} />
              </span>
              <span className="search-lines">
                <span className="search-line">
                  <span className="ellipsis grow">
                    <Highlight text={r.title || t("session.untitled")} />
                    <span className="muted"> · {TYPE_LABEL[r.type] ?? TYPE_LABEL.other}</span>
                  </span>
                  <span className="muted small search-when">{when(r.time_ms, now, props.timeZone)}</span>
                </span>
                {r.preview && (
                  <span className="ellipsis search-preview">
                    <Highlight text={r.preview} />
                  </span>
                )}
                {r.snippet && (
                  <span className="ellipsis muted small search-snippet">
                    <Highlight text={r.snippet} />
                  </span>
                )}
              </span>
            </li>
          ))}
          {next && <li ref={sentinel} className="search-more" aria-hidden="true" />}
        </ul>
        {page !== null && results.length === 0 && <div className="muted pad small">{TEXT.none}</div>}
      </div>
    </dialog>
  );
}
