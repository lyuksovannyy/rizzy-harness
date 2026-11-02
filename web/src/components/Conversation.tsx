// An open session: header, timeline, reply composer. Loaded lazily so the
// new-tab home screen stays small.
import { useEffect, useLayoutEffect, useRef } from "react";
import { api } from "../api";
import type { Live } from "../live";
import type { Agent, ModelInfo, NodeInfo, SessionRow } from "../types";
import { Composer } from "./Composer";
import { Markdown } from "./Markdown";
import { NodeBadge } from "./NodeBadge";
import { ToolCard } from "./ToolCard";

export default function Conversation(props: {
  live: Live;
  sessionId: string;
  row: SessionRow | null;
  nodes: NodeInfo[];
  agents: Agent[];
  models: ModelInfo[];
}) {
  const { live, sessionId } = props;
  useEffect(() => {
    live.watch(sessionId);
    return () => live.unwatch(sessionId);
  }, [live, sessionId]);
  const view = live.view(sessionId);
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  });

  const meta = view?.meta;
  const running = !!view?.running_turn;
  const nodeId = meta?.node_id ?? props.row?.node_id ?? "";
  const node = props.nodes.find((n) => n.node_id === nodeId);
  const agentic = (meta?.profile ?? props.row?.profile) === "agentic";
  const activeAgent = view?.active_agent ?? props.row?.agent_id ?? "general";
  const agentName = (id: string | null | undefined) =>
    props.agents.find((a) => a.id === id)?.name ?? id ?? "";

  return (
    <section className="conversation">
      <header className="conv-head">
        <div className="conv-title">
          <h1 className="ellipsis">{meta?.title ?? props.row?.title ?? "…"}</h1>
          <div className="conv-meta">
            <span>{agentName(activeAgent)}</span>
            <span className="muted">{meta?.model}</span>
            {agentic && <NodeBadge node={node} nodeId={nodeId} />}
            {agentic && <span className="mono muted ellipsis">{meta?.cwd}</span>}
          </div>
        </div>
      </header>

      <div
        className="timeline"
        ref={scroller}
        onScroll={() => {
          const el = scroller.current;
          if (el) stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        }}
      >
        <div className="column">
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
                    <div key={i} className={`msg assistant ${!it.done && running ? "streaming" : ""}`}>
                      {it.author && it.author !== activeAgent && (
                        <div className="author">{agentName(it.author)}</div>
                      )}
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
                    <div key={i} className="divider">
                      {it.agent.name}
                    </div>
                  );
                case "turn_end":
                  return it.reason === "completed" ? null : (
                    <div key={i} className={`divider r-${it.reason}`}>
                      turn {it.reason}
                    </div>
                  );
              }
            })
          )}
          {running && view?.running_agent && (
            <div className="thinking">{agentName(view.running_agent.agent_id)} is working…</div>
          )}
        </div>
      </div>

      <div className="bottom">
        <div className="column">
          <Composer
            where="session"
            agents={props.agents}
            models={props.models}
            workers={[]}
            agentId={activeAgent}
            onAgentChange={(id) => void api.setAgent(sessionId, id)}
            sessionModel={meta?.model}
            agenticBlocked={null}
            running={running}
            draftKey={`harness.draft.${sessionId}`}
            onStop={() => void api.cancel(sessionId)}
            onSubmit={async (s) => {
              stick.current = true;
              await api.prompt(sessionId, s.text, s.effort ? { effort: s.effort } : {});
            }}
          />
        </div>
      </div>
    </section>
  );
}
