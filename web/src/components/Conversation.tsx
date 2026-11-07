// An open session: header, timeline, reply composer. Loaded lazily so the
// new-tab home screen stays small.
import { FileText, Trash2 } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { t } from "../lib/i18n";
import { citeMaps, citeSignature, groupWeb, provenanceAt, provenanceIndex, provenanceSignature, sourceKeys } from "../lib/rich";
import type { Live } from "../live";
import { MessageCtx, ProgressCtx, type MessageInfo } from "../md/context";
import { CopyMessage } from "../md/CopyMessage";
import type { Agent, ModelInfo, NodeInfo, SessionRow } from "../types";
import { progressSignature, progressSlots } from "../ui/blocks";
import { Composer } from "./Composer";
import { Markdown } from "./Markdown";
import { NodeBadge } from "./NodeBadge";
import { ToolCard, WebGroupCard } from "./ToolCard";

function Undo(props: {
  sessionId: string;
  turnId: string;
  files: string[];
  restored: boolean;
  running: boolean;
  /** Later turns also changed files: offer to rewind them too. */
  laterChanges: boolean;
}) {
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [lastRewind, setLastRewind] = useState(false);
  const run = async (force: boolean, rewind = lastRewind) => {
    setBusy(true);
    setErr(null);
    setLastRewind(rewind);
    try {
      await api.restoreTurn(props.sessionId, props.turnId, force, rewind);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="undo small">
      <details>
        <summary>
          {props.files.length} file{props.files.length === 1 ? "" : "s"} changed{props.restored ? " · undone" : ""}
        </summary>
        <ul className="mono">
          {props.files.map((f) => (
            <li key={f}>{f}</li>
          ))}
        </ul>
      </details>
      {!props.restored && (
        <button type="button" className="link" disabled={busy || props.running} onClick={() => void run(false, false)}>
          Undo file changes
        </button>
      )}
      {props.laterChanges && (
        <button
          type="button"
          className="link"
          disabled={busy || props.running}
          title="Undo this turn and every later turn's file changes"
          onClick={() => void run(false, true)}
        >
          Rewind files to before this turn
        </button>
      )}
      {err && (
        <div className="error-text">
          {err}{" "}
          {err.includes("changed after the turn") && (
            <button type="button" className="link" onClick={() => void run(true)}>
              Undo anyway
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function compact(n: number): string {
  return n >= 1000 ? `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k` : String(n);
}

export default function Conversation(props: {
  live: Live;
  sessionId: string;
  row: SessionRow | null;
  nodes: NodeInfo[];
  agents: Agent[];
  models: ModelInfo[];
  voice?: boolean;
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
  const items = view?.items ?? [];
  // The view is mutated in place, so memoise on what the result depends on.
  const provSig = provenanceSignature(items);
  const firstSeen = useMemo(() => provenanceIndex(items), [view, provSig]);
  const citeSig = citeSignature(items);
  const cites = useMemo(() => citeMaps(items), [view, citeSig]);
  // One stable context value per message, so streaming does not re-render links.
  const infoAt = useMemo(() => {
    const cache = new Map<number, MessageInfo>();
    return (i: number): MessageInfo => {
      let info = cache.get(i);
      if (!info) {
        const c = cites(i);
        info = { provenance: provenanceAt(firstSeen, i), sources: sourceKeys(c), cites: c };
        cache.set(i, info);
      }
      return info;
    };
  }, [firstSeen, cites]);
  // Which progress blocks are the turn's live card and which are snapshots.
  const progressSig = view ? progressSignature(items, view) : "";
  const slots = useMemo(() => (view ? progressSlots(items, view) : new Map()), [view, progressSig]);
  // Runs of web tool calls show as one "Researched" row; the rest of a run
  // is rendered by its first item.
  const webGroups = groupWeb(items);
  let groupedUntil = -1;
  // Items before the running turn's user message are final even if their
  // stream was cut off (an interrupted turn leaves done=false).
  const liveFrom = running ? items.findLastIndex((x) => x.type === "user") : Infinity;
  const nodeId = meta?.node_id ?? props.row?.node_id ?? "";
  const node = props.nodes.find((n) => n.node_id === nodeId);
  const agentic = (meta?.profile ?? props.row?.profile) === "agentic";
  const activeAgent = view?.active_agent ?? props.row?.agent_id ?? "general";
  const agentName = (id: string | null | undefined) =>
    props.agents.find((a) => a.id === id)?.name ?? id ?? "";
  const incognito = !!(meta?.incognito ?? props.row?.incognito);
  const remove = async () => {
    if (!confirm(t("conv.deleteConfirm"))) return;
    await api.deleteSession(sessionId).catch(() => {});
    location.hash = "#/";
  };

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
            {view && view.usage.input_tokens + view.usage.output_tokens > 0 && (
              <span
                className="muted meter"
                title={`${view.usage.input_tokens.toLocaleString()} in · ${view.usage.output_tokens.toLocaleString()} out${
                  view.usage.cache_read_tokens
                    ? ` · ${view.usage.cache_read_tokens.toLocaleString()} of the input read from the provider's cache`
                    : ""
                }`}
              >
                {compact(view.usage.input_tokens + view.usage.output_tokens)} {t("conv.tokens")} ·{" "}
                {compact(view.context_tokens)} {t("conv.context")}
              </span>
            )}
          </div>
        </div>
        <button type="button" className="icon" aria-label={t("conv.delete")} title={t("conv.delete")} onClick={() => void remove()}>
          <Trash2 size={18} />
        </button>
      </header>
      {props.row?.parent_id && (
        <div className="incognito-banner">
          <span>
            Sub-task run by {agentName(activeAgent)} for another session.
          </span>
          <a className="link" href={`#/s/${props.row.parent_id}`}>
            Back to the main session
          </a>
        </div>
      )}
      {incognito && (
        <div className="incognito-banner">
          <span>{t("conv.incognito")}</span>
          <button type="button" className="link" onClick={() => void remove()}>
            {t("conv.deleteNow")}
          </button>
        </div>
      )}

      <div
        className="timeline"
        ref={scroller}
        onScroll={() => {
          const el = scroller.current;
          if (el) stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
        }}
      >
        <ProgressCtx.Provider value={slots}>
          <div className="column">
            {!view || view.head_seq === 0 ? (
              <div className="muted pad">Loading…</div>
            ) : (
              view.items.map((it, i) => {
                if (i <= groupedUntil) return null;
                const web = webGroups.get(i);
                if (web) {
                  groupedUntil = web.end;
                  return (
                    <WebGroupCard
                      key={i}
                      sessionId={sessionId}
                      group={web}
                      tools={web.members.map((m) => view.items[m] as Extract<typeof it, { type: "tool" }>)}
                    />
                  );
                }
                // Parallel answers: render the whole group once, side by side.
                if (it.type === "assistant" && it.variant) {
                  const g = it.variant.group;
                  const first = view.items.findIndex((x) => x.type === "assistant" && x.variant?.group === g);
                  if (first !== i) return null;
                  const answers = view.items.filter(
                    (x): x is Extract<typeof x, { type: "assistant" }> => x.type === "assistant" && x.variant?.group === g,
                  );
                  const chosen =
                    view.variant_selection[g] ?? answers.find((a) => a.variant?.index === 0)?.message_id;
                  const busy = running && view.running_turn === g;
                  return (
                    <div key={i} className="variants" style={{ gridTemplateColumns: `repeat(${answers.length}, minmax(0, 1fr))` }}>
                      {answers.map((a) => (
                        <div key={a.message_id} className={`msg assistant variant ${a.message_id === chosen ? "chosen" : ""}`}>
                          <div className="variant-head small">
                            <span className="mono">{a.variant?.model}</span>
                            {a.message_id === chosen ? (
                              <span className="muted">continuing from this</span>
                            ) : (
                              <button
                                type="button"
                                className="link"
                                disabled={busy}
                                onClick={() => void api.selectVariant(sessionId, g, a.message_id)}
                              >
                                Continue with this
                              </button>
                            )}
                          </div>
                          <MessageCtx.Provider value={infoAt(i)}>
                            <Markdown
                              text={a.text}
                              done={a.done || !busy}
                              blocks={a.blocks}
                              from={a.author ? agentName(a.author) : null}
                              messageId={a.message_id}
                            />
                          </MessageCtx.Provider>
                          {(a.done || !busy) && <CopyMessage text={a.text} />}
                        </div>
                      ))}
                    </div>
                  );
                }
                switch (it.type) {
                  case "user":
                    return (
                      <div key={i} className="msg user">
                        <div className="bubble">{it.text}</div>
                        {it.attachments && (
                          <ul className="chips">
                            {it.attachments.map((n, j) => (
                              <li key={j} className="chip">
                                <FileText size={14} /> {n}
                              </li>
                            ))}
                          </ul>
                        )}
                      </div>
                    );
                  case "assistant": {
                    const final = it.done || i < liveFrom;
                    return (
                      <div key={i} className={`msg assistant ${!it.done && running ? "streaming" : ""}`}>
                        {it.author && it.author !== activeAgent && (
                          <div className="author">{agentName(it.author)}</div>
                        )}
                        <MessageCtx.Provider value={infoAt(i)}>
                          <Markdown
                            text={it.text}
                            done={final}
                            blocks={it.blocks}
                            from={it.author ? agentName(it.author) : null}
                            messageId={it.message_id}
                          />
                        </MessageCtx.Provider>
                        {final && <CopyMessage text={it.text} />}
                      </div>
                    );
                  }
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
                  case "summary":
                    // Only the latest summary is what the model sees now.
                    return view.items.findLastIndex((x) => x.type === "summary") !== i ? null : (
                      <details key={i} className="divider summary">
                        <summary>{t("conv.summary")}</summary>
                        <p>{it.text}</p>
                      </details>
                    );
                  case "turn_end": {
                    const cp = view.checkpoints[it.turn_id];
                    const laterChanges = view.items.slice(i + 1).some((x) => {
                      if (x.type !== "turn_end") return false;
                      const c = view.checkpoints[x.turn_id];
                      return !!c && c.files.length > 0 && !c.restored;
                    });
                    const undo =
                      cp && cp.after && (cp.files.length > 0 || laterChanges) ? (
                        <Undo
                          key={`u${i}`}
                          sessionId={sessionId}
                          turnId={it.turn_id}
                          files={cp.files}
                          restored={cp.restored || cp.files.length === 0}
                          running={running}
                          laterChanges={laterChanges}
                        />
                      ) : null;
                    return it.reason === "completed" ? (
                      undo
                    ) : (
                      <div key={i}>
                        <div className={`divider r-${it.reason}`}>turn {it.reason}</div>
                        {undo}
                      </div>
                    );
                  }
                }
              })
            )}
            {running && view?.running_agent && (
              <div className="thinking">{agentName(view.running_agent.agent_id)} is working…</div>
            )}
          </div>
        </ProgressCtx.Provider>
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
            voice={props.voice}
            onStop={() => void api.cancel(sessionId)}
            onSubmit={async (s) => {
              stick.current = true;
              await api.prompt(sessionId, s.text, {
                ...(s.effort ? { effort: s.effort } : {}),
                ...(s.attachments.length ? { attachments: s.attachments } : {}),
              });
            }}
          />
        </div>
      </div>
    </section>
  );
}
