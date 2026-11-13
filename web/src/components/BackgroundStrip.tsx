// The chat's background strip (T.8d step 2, docs/background-commands.md
// §4.11): above the composer while a background command of the open chat
// runs. Each row is a fold of the log (id, the command's head, time left),
// so a reload and a second tab show the same; what this tab clicked
// (stopping, failed) is the only state of its own, and the log's end
// replaces it. Its own lazy chunk: the conversation loads it on the first
// running command.
import { useEffect, useState } from "react";
import { call } from "../api";
import { endOf, leftText, msLeft, reasonText, runningBackground } from "../lib/background";
import { showHidden } from "../md/hiddenChars";
import type { SessionView } from "../types";
import "./backgroundStrip.css";

/** How long an end this tab asked for stays in the strip. */
const ENDED_SHOWN_MS = 6000;
/** A stop the node took but whose end has not come back may be asked again after this. */
const RETRY_AFTER_MS = 10000;

type Click =
  | { state: "stopping"; at: number }
  | { state: "failed"; error: string }
  | { state: "ended"; at: number };

function message(x: unknown): string {
  return x instanceof Error ? x.message : String(x);
}

export default function BackgroundStrip(props: {
  sessionId: string;
  view: SessionView | null;
  /** The chat's node is known to be away (not merely not listed). */
  away: boolean;
  nodeLabel: string;
}) {
  const { sessionId, view, away } = props;
  const [clicks, setClicks] = useState<Record<string, Click>>({});
  const [now, setNow] = useState(() => Date.now());
  const rows = runningBackground(view);
  const live = rows.length > 0;
  // A coarse clock for "time left" while anything runs.
  useEffect(() => {
    if (!live) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [live]);
  // An end this tab asked for: shown a moment, then gone.
  const endedNow = Object.entries(clicks)
    .filter(([id, c]) => c.state === "stopping" && endOf(view, id))
    .map(([id]) => id);
  const endedKey = endedNow.join(",");
  useEffect(() => {
    if (!endedKey) return;
    const at = Date.now();
    setClicks((cs) => {
      const next = { ...cs };
      for (const id of endedKey.split(",")) next[id] = { state: "ended", at };
      return next;
    });
  }, [endedKey]);
  const shownEnds = Object.entries(clicks).filter(([, c]) => c.state === "ended") as [string, Extract<Click, { state: "ended" }>][];
  const firstEnd = shownEnds.reduce((a, [, c]) => Math.min(a, c.at), Infinity);
  useEffect(() => {
    if (firstEnd === Infinity) return;
    const t = setTimeout(
      () =>
        setClicks((cs) => {
          const cut = Date.now() - ENDED_SHOWN_MS + 50;
          return Object.fromEntries(Object.entries(cs).filter(([, c]) => !(c.state === "ended" && c.at <= cut)));
        }),
      Math.max(0, firstEnd + ENDED_SHOWN_MS - Date.now()),
    );
    return () => clearTimeout(t);
  }, [firstEnd]);

  if (!live && shownEnds.length === 0) return null;

  const set = (ids: string[], c: Click) =>
    setClicks((cs) => ({ ...cs, ...Object.fromEntries(ids.map((id) => [id, c])) }));
  const stop = async (ids: string[], all: boolean) => {
    set(ids, { state: "stopping", at: Date.now() });
    try {
      await call("POST", all ? `/api/sessions/${sessionId}/background/stop` : `/api/sessions/${sessionId}/background/${ids[0]}/stop`);
    } catch (x) {
      set(ids, { state: "failed", error: message(x) });
    }
  };
  const busy = (id: string) => {
    const c = clicks[id];
    return c?.state === "stopping" && now - c.at < RETRY_AFTER_MS;
  };
  const stoppable = rows.filter((r) => !busy(r.bg_id));

  return (
    <section className="bg-strip" aria-label="Background commands">
      <div className="bg-strip-head small">
        <span>
          {rows.length === 0 ? (
            "Background commands"
          ) : (
            <>
              {rows.length === 1 ? "1 background command" : `${rows.length} background commands`}
              <span className="muted">{rows.length === 1 ? " · keeps" : " · keep"} running when the turn stops</span>
            </>
          )}
        </span>
        {rows.length >= 2 && (
          <button
            type="button"
            className="danger small"
            disabled={away || stoppable.length === 0}
            onClick={() => void stop(rows.map((r) => r.bg_id), true)}
          >
            Stop all
          </button>
        )}
      </div>
      {away && (
        <div className="bg-strip-away small" role="note">
          The node {showHidden(props.nodeLabel)} is away, so a stop cannot reach it. These end with the node or at their
          timeout.
        </div>
      )}
      <ul className="bg-rows">
        {rows.map((r) => {
          const c = clicks[r.bg_id];
          const stopping = c?.state === "stopping";
          return (
            <li key={r.bg_id} className="bg-row" data-bg={r.bg_id}>
              <span className="mono bg-id">{showHidden(r.bg_id)}</span>
              {/* The model's text: shown as text, hidden characters marked. */}
              <code className="bg-cmd ellipsis" title={showHidden(r.head)}>
                {showHidden(r.head)}
                {r.cut ? " …" : ""}
              </code>
              <span className="muted bg-left">{away ? "node away" : leftText(msLeft(r, now))}</span>
              <span className="bg-state" role="status">
                {stopping ? "stopping…" : c?.state === "failed" ? <span className="error-text">{c.error}</span> : null}
              </span>
              <button
                type="button"
                className="danger small"
                aria-label={`Stop ${r.bg_id}`}
                disabled={away || busy(r.bg_id)}
                onClick={() => void stop([r.bg_id], false)}
              >
                {stopping && !busy(r.bg_id) ? "Stop again" : "Stop"}
              </button>
            </li>
          );
        })}
        {shownEnds.map(([id]) => {
          const end = endOf(view, id);
          return (
            <li key={id} className="bg-row bg-row-ended" data-bg={id}>
              <span className="mono bg-id">{showHidden(id)}</span>
              <span className="bg-state" role="status">
                ended: {end ? reasonText(end.reason, end.exit_code) : "ended"}
              </span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
