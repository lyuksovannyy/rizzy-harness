// "You may have asked this before": earlier chats matching what is being
// typed. Suggestions only; nothing opens unless clicked.
import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import { api } from "../api";
import type { Mode } from "../lib/modes";
import { when } from "../lib/when";
import type { SearchResult } from "../types";
import { Highlight } from "./Highlight";
import timeSensitive from "../lib/time-sensitive.json";

const DEBOUNCE_MS = 200;

function looksTimeSensitive(text: string): boolean {
  const lower = text.toLowerCase();
  const words = lower.split(/[^\p{L}\p{N}]+/u);
  return (
    timeSensitive.words.some((w) => words.includes(w)) ||
    timeSensitive.phrases.some((p) => lower.includes(p))
  );
}

export interface SimilarHandle {
  /** Focus the first suggestion; false if there is none. */
  focusFirst: () => boolean;
}

export const Similar = forwardRef<
  SimilarHandle,
  {
    text: string;
    mode: Mode;
    /** Matches come from this workspace's chats (null: the API token's, all). */
    workspace: string | null;
    onOpen: (id: string) => void;
    onCount?: (n: number) => void;
  }
>(function Similar({ text, mode, workspace, onOpen, onCount }, ref) {
    const [hits, setHits] = useState<SearchResult[]>([]);
    useEffect(() => onCount?.(hits.length), [hits.length, onCount]);
    const list = useRef<HTMLUListElement>(null);
    useImperativeHandle(ref, () => ({
      focusFirst: () => {
        const b = list.current?.querySelector("button");
        if (!b) return false;
        b.focus();
        return true;
      },
    }));

    useEffect(() => {
      // Search-mode text never leaves the browser; time-sensitive asks get no matches.
      const q = text.trim();
      if (mode === "search" || q.split(/\s+/).length < 3 || looksTimeSensitive(q)) {
        setHits([]);
        return;
      }
      const ctl = new AbortController();
      const timer = setTimeout(() => {
        api
          .search(q, { suggest: true, signal: ctl.signal, workspace })
          .then((r) => setHits(r.results))
          .catch(() => {});
      }, DEBOUNCE_MS);
      return () => {
        clearTimeout(timer);
        ctl.abort();
      };
    }, [text, mode, workspace]);

    if (hits.length === 0) return null;
    const back = () => document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus();
    return (
      <div className="similar" aria-label="earlier chats">
        <div className="similar-label muted">You may have asked this before</div>
        <ul ref={list}>
          {hits.map((h, i) => (
            <li key={h.id}>
              <button
                type="button"
                onClick={() => onOpen(h.id)}
                onKeyDown={(e) => {
                  const items = list.current?.querySelectorAll("button");
                  if (e.key === "ArrowDown") {
                    e.preventDefault();
                    (items?.[i + 1] as HTMLButtonElement | undefined)?.focus();
                  } else if (e.key === "ArrowUp") {
                    e.preventDefault();
                    if (i === 0) back();
                    else (items?.[i - 1] as HTMLButtonElement | undefined)?.focus();
                  } else if (e.key === "Escape") {
                    e.preventDefault();
                    back();
                  }
                }}
              >
                <span className="similar-title ellipsis">
                  <Highlight text={h.title} />
                </span>
                <span className="similar-snippet ellipsis muted">
                  <Highlight text={h.snippet ?? h.preview} />
                </span>
                <span className="muted similar-age">{when(h.time_ms, Date.now())}</span>
              </button>
            </li>
          ))}
        </ul>
      </div>
    );
  });
