// The row under a finished turn's final answer: Copy, Fork from here, Pin,
// when the turn ended and what it used. Shown as "Copy" always was (the
// `.msg-actions` rule: on hover or focus, always on touch). The time and
// the tokens come from the turn's end item (fold.ts); the hover detail of
// the tokens reads the turn's sub-agents from their own chats.
// Loads with the conversation chunk.
import { Pin, PinOff } from "lucide-react";
import { type ReactNode, useContext, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { pageApi } from "../apiPages";
import { formatCost, modelCost, costText, estimate, pricesSet } from "../lib/cost";
import { errText } from "../lib/errorText";
import { t } from "../lib/i18n";
import "../lib/i18nPages";
import { LiveCtx } from "../lib/liveCtx";
import { fullTime, relativeTime, useNow } from "../lib/relativeTime";
import { childUsage, shortTokens, type TurnFigures } from "../lib/turnTokens";
import { CopyButton } from "../md/CopyMessage";
import type { ModelInfo, ModelTokens, Usage } from "../types";
import "./messageActions.css";

/** When a turn ended, relative; the full date on hover. */
export function RelTime({ at }: { at: number }) {
  const now = useNow();
  if (!Number.isFinite(at)) return null;
  return (
    <time className="msg-time" dateTime={new Date(at).toISOString()} title={fullTime(at)}>
      {relativeTime(at, now)}
    </time>
  );
}

const n = (x: number) => x.toLocaleString();

/** The money for one set of tokens, when the server prices the models; "" otherwise. */
function money(u: Usage, models: ModelInfo[]): string {
  if (!pricesSet(models)) return "";
  return costText(estimate(u, models))?.text ?? "";
}

function ModelTable(props: { usage: Usage; models: ModelInfo[] }) {
  const { usage, models } = props;
  const by = Object.entries(usage.by_model ?? {}).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
  const sumIn = by.reduce((s, [, x]) => s + x.input_tokens, 0);
  const sumOut = by.reduce((s, [, x]) => s + x.output_tokens, 0);
  const rest = { input_tokens: Math.max(0, usage.input_tokens - sumIn), output_tokens: Math.max(0, usage.output_tokens - sumOut) };
  const hasCache = !!usage.cache_read_tokens || !!usage.cache_write_tokens || by.some(([, x]) => !!x.cache_read_tokens || !!x.cache_write_tokens);
  const priced = pricesSet(models);
  const price = (tk: ModelTokens, id: string) => {
    const info = models.find((m) => m.id === id);
    const c = modelCost(tk, info);
    return c === null ? "—" : formatCost(c, info?.price?.currency ?? "");
  };
  return (
    <table className="tok-table">
      <thead>
        <tr>
          <th scope="col">{t("msg.byModel")}</th>
          <th scope="col">{t("msg.in")}</th>
          <th scope="col">{t("msg.out")}</th>
          {hasCache && <th scope="col">{t("msg.cacheRead")}</th>}
          {hasCache && <th scope="col">{t("msg.cacheWrite")}</th>}
          {priced && <th scope="col">{"≈"}</th>}
        </tr>
      </thead>
      <tbody>
        {by.map(([id, x]) => (
          <tr key={id}>
            <th scope="row" className="mono">
              {id}
            </th>
            <td>{n(x.input_tokens)}</td>
            <td>{n(x.output_tokens)}</td>
            {hasCache && <td>{n(x.cache_read_tokens ?? 0)}</td>}
            {hasCache && <td>{n(x.cache_write_tokens ?? 0)}</td>}
            {priced && <td>{price(x, id)}</td>}
          </tr>
        ))}
        {(rest.input_tokens > 0 || rest.output_tokens > 0) && (
          <tr>
            <th scope="row">{by.length ? "?" : "—"}</th>
            <td>{n(rest.input_tokens)}</td>
            <td>{n(rest.output_tokens)}</td>
            {hasCache && <td />}
            {hasCache && <td />}
            {priced && <td>{"—"}</td>}
          </tr>
        )}
      </tbody>
    </table>
  );
}

/** One sub-agent of the turn, read from its own chat (the same access check as opening it). */
function ChildLine(props: { child: string; agent: string; fig: TurnFigures; models: ModelInfo[]; name: (id: string) => string }) {
  const live = useContext(LiveCtx);
  useEffect(() => {
    if (!live) return;
    live.watch(props.child);
    return () => live.unwatch(props.child);
  }, [live, props.child]);
  const version = () => live?.version ?? 0;
  useSyncExternalStore((l) => (live ? live.subscribe(l) : () => {}), version, version);
  const v = live?.view(props.child);
  const used = v && v.head_seq > 0 ? childUsage(v, props.fig.after, props.fig.at) : null;
  const models = used ? Object.keys(used.by_model ?? {}).sort() : [];
  let what: ReactNode;
  if (used) {
    const cost = money(used, props.models);
    what = (
      <>
        <span className="mono">{models.length ? models.join(", ") : "—"}</span>
        {` · ${t("msg.tokens", { in: n(used.input_tokens), out: n(used.output_tokens) })}`}
        {cost && ` · ${cost}`}
      </>
    );
  } else if (live?.refused(props.child)) what = t("msg.subagentGone");
  else if (v && v.head_seq > 0) what = t("msg.subagentNone");
  else what = t("msg.subagentLoading");
  return (
    <li>
      <strong>{props.name(props.agent)}</strong> {what}
    </li>
  );
}

/** The detail: per model, then the sub-agents, then the total. */
function Detail(props: { fig: TurnFigures; usage: Usage; inherited: boolean; models: ModelInfo[]; name: (id: string) => string }) {
  const { fig, usage, models } = props;
  const total = money(usage, models);
  return (
    <>
      <p className="tok-title">{t("msg.tokensTitle")}</p>
      {props.inherited && <p className="muted">{t("msg.beforeFork")}</p>}
      <ModelTable usage={usage} models={models} />
      {fig.children.length > 0 && (
        <>
          <p className="tok-sub">{t("msg.subagents")}</p>
          <ul className="tok-children">
            {fig.children.map((c) => (
              <ChildLine key={c.child} child={c.child} agent={c.agent} fig={fig} models={models} name={props.name} />
            ))}
          </ul>
          <p className="muted">{t("msg.subagentsNote")}</p>
        </>
      )}
      <p className="tok-total">
        <strong>{t("msg.total")}</strong> {t("msg.tokens", { in: n(usage.input_tokens), out: n(usage.output_tokens) })}
        {total && ` · ${total}`}
      </p>
    </>
  );
}

/** "12.3k in · 1.2k out", and on hover, click or focus the detail (a hover-only detail would be out of reach on touch). Escape and a click outside close it. */
export function TokenFigure(props: { fig: TurnFigures; usage: Usage; inherited: boolean; models: ModelInfo[]; name: (id: string) => string }) {
  const [hover, setHover] = useState(false);
  const [focus, setFocus] = useState(false);
  const [sticky, setSticky] = useState(false);
  const open = hover || focus || sticky;
  const root = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = () => {
      setHover(false);
      setFocus(false);
      setSticky(false);
    };
    const down = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) close();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    document.addEventListener("pointerdown", down);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", down);
      document.removeEventListener("keydown", key);
    };
  }, [open]);
  const u = props.usage;
  return (
    <span
      ref={root}
      className="tok"
      onPointerEnter={(e) => e.pointerType === "mouse" && setHover(true)}
      onPointerLeave={(e) => e.pointerType === "mouse" && setHover(false)}
    >
      <button
        type="button"
        className="link small tok-btn"
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label={`${t("msg.tokensLabel")}: ${t("msg.tokens", { in: n(u.input_tokens), out: n(u.output_tokens) })}`}
        onFocus={() => setFocus(true)}
        onBlur={(e) => {
          if (!root.current?.contains(e.relatedTarget as Node | null)) setFocus(false);
        }}
        onClick={() => setSticky((s) => !s)}
      >
        {t("msg.tokens", { in: shortTokens(u.input_tokens), out: shortTokens(u.output_tokens) })}
      </button>
      {open && (
        <div role="dialog" aria-label={t("msg.tokensTitle")} className="tok-pop small">
          <Detail {...props} />
        </div>
      )}
    </span>
  );
}

/** "Pin" / "Unpin": the owner's bookmark on this message. The route refuses what the rule refuses; its words are shown here. */
export function PinMessage(props: { sessionId: string; seq: number; pinned: boolean; onError: (e: string | null) => void }) {
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    props.onError(null);
    try {
      await pageApi.pinMessage(props.sessionId, props.seq, !props.pinned);
    } catch (x) {
      props.onError(errText(x));
    } finally {
      setBusy(false);
    }
  };
  const Icon = props.pinned ? PinOff : Pin;
  return (
    <button
      type="button"
      className="link small"
      disabled={busy}
      aria-pressed={props.pinned}
      title={props.pinned ? t("msg.unpinHint") : t("msg.pinHint")}
      onClick={() => void run()}
    >
      <Icon size={14} aria-hidden /> {props.pinned ? t("msg.unpin") : t("msg.pin")}
    </button>
  );
}

export interface AnswerActionsProps {
  sessionId: string;
  text: string;
  /** In a rewound block: the figures only, nothing to act on. */
  rewound: boolean;
  /** "Fork from here" where the server would offer it (already decided by the caller). */
  fork: ReactNode;
  /** The seq a pin names, and whether it is pinned; null where this viewer cannot pin. */
  pin: { seq: number; pinned: boolean } | null;
  fig: TurnFigures | null;
  /** The turn was copied from the chat this one was forked from. */
  inherited: boolean;
  models: ModelInfo[];
  name: (id: string) => string;
}

export function AnswerActions(props: AnswerActionsProps) {
  const [err, setErr] = useState<string | null>(null);
  const { fig } = props;
  const u = fig?.usage;
  const tokens = !!fig && !!u && u.input_tokens + u.output_tokens > 0;
  const time = !!fig && fig.at > 0;
  const acts = !props.rewound;
  if (!acts && !time && !tokens) return null;
  return (
    <div className="msg-actions answer-actions">
      {!props.rewound && <CopyButton text={props.text} />}
      {!props.rewound && props.fork}
      {!props.rewound && props.pin && <PinMessage sessionId={props.sessionId} seq={props.pin.seq} pinned={props.pin.pinned} onError={setErr} />}
      {time && <RelTime at={fig.at} />}
      {tokens && <TokenFigure fig={fig} usage={u} inherited={props.inherited} models={props.models} name={props.name} />}
      {err && (
        <div className="error-text small msg-actions-err" role="alert">
          {err}
        </div>
      )}
    </div>
  );
}
