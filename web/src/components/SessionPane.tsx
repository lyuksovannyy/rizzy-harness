import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Live } from "../live";
import type { NodeInfo, SessionRow } from "../types";
import { Markdown } from "./Markdown";
import { NodeBadge } from "./NodeBadge";
import { ToolCard } from "./ToolCard";

const draftKey = (id: string) => `harness.draft.${id}`;

export function SessionPane(props: {
  live: Live;
  sessionId: string;
  row: SessionRow | null;
  nodes: NodeInfo[];
}) {
  const { live, sessionId } = props;
  useEffect(() => {
    live.watch(sessionId);
    return () => live.unwatch(sessionId);
  }, [live, sessionId]);
  const view = live.view(sessionId);

  const [draft, setDraft] = useState(() => localStorage.getItem(draftKey(sessionId)) ?? "");
  const [err, setErr] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    if (draft) localStorage.setItem(draftKey(sessionId), draft);
    else localStorage.removeItem(draftKey(sessionId));
  }, [draft, sessionId]);

  // Follow the bottom unless the user scrolled up.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  });
  const onScroll = () => {
    const el = scroller.current;
    if (el) stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
  };

  const running = !!view?.running_turn;
  const meta = view?.meta;
  const node = props.nodes.find((n) => n.node_id === (meta?.node_id ?? props.row?.node_id));

  const send = async () => {
    const text = draft.trim();
    if (!text || running) return;
    setSending(true);
    setErr(null);
    try {
      await api.prompt(sessionId, text);
      setDraft("");
      stick.current = true;
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setSending(false);
    }
  };

  const stop = async () => {
    try {
      await api.cancel(sessionId);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    }
  };

  return (
    <section className="pane">
      <header className="pane-head">
        <div className="pane-title">
          <h2 className="ellipsis">{meta?.title ?? props.row?.title ?? "…"}</h2>
          <div className="pane-meta">
            <NodeBadge node={node} nodeId={meta?.node_id ?? props.row?.node_id ?? "?"} />
            <span className="mono ellipsis">{meta?.cwd}</span>
            <span className="muted">{meta?.model}</span>
            <span className="muted">approvals: {meta?.approval_mode}</span>
          </div>
        </div>
        {running && (
          <button className="danger" onClick={stop}>
            Stop
          </button>
        )}
      </header>

      <div className="timeline" ref={scroller} onScroll={onScroll}>
        {!view || view.head_seq === 0 ? (
          <div className="muted pad">Loading…</div>
        ) : (
          view.items.map((it, i) => {
            switch (it.type) {
              case "user":
                return (
                  <div key={i} className="msg user">
                    <div className="bubble">{it.text}</div>
                  </div>
                );
              case "assistant":
                return (
                  <div
                    key={i}
                    className={`msg assistant ${!it.done && running ? "streaming" : ""}`}
                  >
                    <Markdown text={it.text} />
                  </div>
                );
              case "tool":
                return <ToolCard key={i} sessionId={sessionId} item={it} />;
              case "error":
                return (
                  <div key={i} className="msg error">
                    {it.message}
                  </div>
                );
              case "agent_changed":
                return (
                  <div key={i} className="turn-end">
                    switched to {it.agent.name}
                  </div>
                );
              case "turn_end":
                return it.reason === "completed" ? (
                  <hr key={i} className="turn-sep" />
                ) : (
                  <div key={i} className={`turn-end r-${it.reason}`}>
                    turn {it.reason}
                  </div>
                );
            }
          })
        )}
        {running && <div className="thinking">working…</div>}
      </div>

      <div className="composer">
        <textarea
          value={draft}
          placeholder={running ? "Agent is working… (Stop to interrupt)" : "Message the agent. Enter to send, Shift+Enter for a newline."}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              void send();
            }
          }}
          rows={Math.min(10, Math.max(2, draft.split("\n").length))}
        />
        <div className="composer-side">
          {err && <div className="error-text">{err}</div>}
          <button className="primary" disabled={running || sending || !draft.trim()} onClick={send}>
            Send
          </button>
        </div>
      </div>
    </section>
  );
}
