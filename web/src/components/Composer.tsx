import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { t } from "../lib/i18n";
import { activeMention, filterAgents, insertMention, leadingMention } from "../lib/mentions";
import { cycleMode, enabledModes, MODES, type Mode } from "../lib/modes";
import type { Agent, Effort, ModelInfo, NodeInfo } from "../types";

const FOLDERS_KEY = "harness.folders";
const EFFORTS: Effort[] = ["low", "medium", "high", "max"];

function recentFolders(): string[] {
  try {
    return JSON.parse(localStorage.getItem(FOLDERS_KEY) ?? "[]") as string[];
  } catch {
    return [];
  }
}

function rememberFolder(f: string) {
  const list = [f, ...recentFolders().filter((x) => x !== f)].slice(0, 8);
  try {
    localStorage.setItem(FOLDERS_KEY, JSON.stringify(list));
  } catch {
    /* storage unavailable */
  }
}

export interface Submission {
  text: string;
  mode: Mode;
  agentId: string;
  model: string;
  effort: Effort | null;
  nodeId: string;
  cwd: string;
}

export function Composer(props: {
  /** `home` shows the mode switch and model picker; `session` is a reply box. */
  where: "home" | "session";
  agents: Agent[];
  models: ModelInfo[];
  workers: NodeInfo[];
  /** Selected agent (home: the choice for the new chat; session: the session's agent). */
  agentId: string;
  onAgentChange: (id: string) => void;
  /** Session model (session view), for the effort control. */
  sessionModel?: string;
  defaultModel?: string | null;
  agenticBlocked: string | null;
  running?: boolean;
  draftKey?: string;
  onSubmit: (s: Submission) => Promise<void>;
  onStop?: () => void;
  /** Home: the classifier's guess for this text (null = no opinion). */
  predict?: (text: string) => Mode | null;
  /** Home: called after a successful submit with the mode actually used. */
  onLearn?: (text: string, mode: Mode) => void;
  /** Home: text/mode changes, for the similar-chats panel. */
  onTextChange?: (text: string, mode: Mode) => void;
  /** Home: ArrowDown at the end of the text; return true if handled. */
  onArrowDown?: () => boolean;
}) {
  const home = props.where === "home";
  const [text, setText] = useState(() =>
    props.draftKey ? (localStorage.getItem(props.draftKey) ?? "") : "",
  );
  const [mode, setMode] = useState<Mode>("ask");
  // Once the user picks a mode for this message, the classifier stays quiet.
  const [manual, setManual] = useState(false);
  const [model, setModel] = useState(
    () => props.defaultModel ?? props.models.find((m) => m.default)?.id ?? props.models[0]?.id ?? "",
  );
  const [effort, setEffort] = useState<Effort | null>(null);
  const [nodeId, setNodeId] = useState(props.workers[0]?.node_id ?? "");
  const [cwd, setCwd] = useState(() => recentFolders()[0] ?? "");
  const [caret, setCaret] = useState(0);
  const [pick, setPick] = useState(0);
  const [popupClosed, setPopupClosed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const ta = useRef<HTMLTextAreaElement>(null);
  // Caret to restore right after React commits a programmatic text change,
  // before the next keystroke can land in the wrong place.
  const pendingCaret = useRef<number | null>(null);
  useLayoutEffect(() => {
    const c = pendingCaret.current;
    if (c !== null && ta.current) {
      ta.current.setSelectionRange(c, c);
      pendingCaret.current = null;
    }
  }, [text]);

  // Keep selections valid as lists load.
  useEffect(() => {
    if (!model && props.models.length) {
      setModel(props.defaultModel ?? props.models.find((m) => m.default)?.id ?? props.models[0]!.id);
    }
  }, [props.models, props.defaultModel, model]);
  useEffect(() => {
    if (!props.workers.some((w) => w.node_id === nodeId)) setNodeId(props.workers[0]?.node_id ?? "");
  }, [props.workers, nodeId]);
  useEffect(() => {
    if (mode === "agentic" && props.agenticBlocked) setMode("ask");
  }, [mode, props.agenticBlocked]);
  const { predict, onTextChange } = props;
  useEffect(() => {
    if (!home || manual || !predict) return;
    const guess = text.trim() ? predict(text) : "ask";
    if (guess) setMode(guess);
  }, [home, manual, predict, text]);
  useEffect(() => {
    onTextChange?.(text, mode);
  }, [onTextChange, text, mode]);
  useEffect(() => {
    if (!props.draftKey) return;
    try {
      if (text) localStorage.setItem(props.draftKey, text);
      else localStorage.removeItem(props.draftKey);
    } catch {
      /* storage unavailable */
    }
  }, [text, props.draftKey]);
  useEffect(() => {
    ta.current?.focus();
  }, []);

  const enabled = useMemo(() => enabledModes(props.agenticBlocked), [props.agenticBlocked]);
  const mention = mode === "search" || popupClosed ? null : activeMention(text, caret);
  const suggestions = mention ? filterAgents(props.agents, mention.query) : [];
  const popupOpen = suggestions.length > 0;
  const override = leadingMention(
    text,
    props.agents.map((a) => a.id),
  );
  const effortModel = home ? model : props.sessionModel;
  const effortOk = props.models.find((m) => m.id === effortModel)?.effort ?? false;

  const choose = (id: string) => {
    if (!mention) return;
    const r = insertMention(text, mention.start, caret, id);
    pendingCaret.current = r.caret;
    setText(r.text);
    setCaret(r.caret);
    ta.current?.focus();
  };

  const submit = async () => {
    const body = text.trim();
    if (!body || busy || props.running) return;
    if (mode === "agentic" && (!cwd.trim() || !nodeId)) {
      setErr(t("composer.folder"));
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await props.onSubmit({
        text: body,
        mode: home ? mode : "ask",
        agentId: props.agentId,
        model,
        effort: effortOk ? effort : null,
        nodeId,
        cwd: cwd.trim(),
      });
      if (mode === "agentic") rememberFolder(cwd.trim());
      if (home) props.onLearn?.(body, mode);
      setText("");
      setManual(false);
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (popupOpen) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const d = e.key === "ArrowDown" ? 1 : -1;
        setPick((p) => (p + d + suggestions.length) % suggestions.length);
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        choose(suggestions[Math.min(pick, suggestions.length - 1)]!.id);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setPopupClosed(true);
        return;
      }
    }
    if (e.key === "Tab" && home) {
      e.preventDefault();
      setManual(true);
      setMode((m) => cycleMode(m, enabled, e.shiftKey ? -1 : 1));
      return;
    }
    if (
      e.key === "ArrowDown" &&
      props.onArrowDown &&
      e.currentTarget.selectionStart === text.length &&
      props.onArrowDown()
    ) {
      e.preventDefault();
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void submit();
    }
  };

  const placeholder = props.running
    ? t("composer.running")
    : !home
      ? t("composer.placeholder.session")
      : t(`composer.placeholder.${mode}`);
  const lines = Math.min(10, Math.max(1, text.split("\n").length));

  return (
    <div className="composer-wrap">
      {popupOpen && (
        <ul className="mention-popup" role="listbox" aria-label="agents">
          {suggestions.map((a, i) => (
            <li
              key={a.id}
              role="option"
              aria-selected={i === pick}
              className={i === pick ? "active" : ""}
              onMouseDown={(e) => {
                e.preventDefault();
                choose(a.id);
              }}
            >
              <b>@{a.id}</b> <span className="muted">{a.description || a.name}</span>
            </li>
          ))}
        </ul>
      )}
      <div className={`composer ${mode === "search" && home ? "is-search" : ""}`}>
        <textarea
          ref={ta}
          rows={lines}
          value={text}
          placeholder={placeholder}
          aria-label={placeholder}
          onChange={(e) => {
            setText(e.target.value);
            setCaret(e.target.selectionStart);
            setPick(0);
            setPopupClosed(false);
          }}
          onSelect={(e) => setCaret(e.currentTarget.selectionStart)}
          onKeyDown={onKeyDown}
        />
        <div className="composer-row">
          {mode !== "search" && (
            <select
              className="pill"
              aria-label="agent"
              value={props.agentId}
              onChange={(e) => props.onAgentChange(e.target.value)}
              title={override ? `→ @${override} ${t("composer.forMessage")}` : undefined}
            >
              {props.agents.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          )}
          {override && override !== props.agentId && (
            <span className="pill override">
              → @{override} {t("composer.forMessage")}
            </span>
          )}
          {home && mode !== "search" && props.models.length > 1 && (
            <select className="pill" aria-label="model" value={model} onChange={(e) => setModel(e.target.value)}>
              {props.models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.id}
                </option>
              ))}
            </select>
          )}
          {mode !== "search" && effortOk && (
            <select
              className="pill"
              aria-label="effort"
              value={effort ?? ""}
              onChange={(e) => setEffort((e.target.value || null) as Effort | null)}
            >
              <option value="">{t("effort.default")}</option>
              {EFFORTS.map((x) => (
                <option key={x} value={x}>
                  {x}
                </option>
              ))}
            </select>
          )}
          <span className="spacer" />
          {home && (
            <div
              className={`mode-switch ${!manual && text.trim() ? "auto" : ""}`}
              role="radiogroup"
              aria-label="mode"
              title={t("mode.hint")}
            >
              {MODES.map((m) => {
                const disabled = !enabled.includes(m);
                return (
                  <button
                    key={m}
                    type="button"
                    role="radio"
                    aria-checked={mode === m}
                    className={mode === m ? "on" : ""}
                    disabled={disabled}
                    title={disabled ? (props.agenticBlocked ?? undefined) : undefined}
                    onClick={() => {
                      setManual(true);
                      setMode(m);
                      ta.current?.focus();
                    }}
                  >
                    {t(`mode.${m}`)}
                  </button>
                );
              })}
            </div>
          )}
          {props.running && props.onStop ? (
            <button type="button" className="send stop" onClick={props.onStop} aria-label={t("composer.stop")}>
              ■
            </button>
          ) : (
            <button
              type="button"
              className="send"
              onClick={() => void submit()}
              disabled={busy || !text.trim()}
              aria-label={t("composer.send")}
            >
              ↑
            </button>
          )}
        </div>
        {home && mode === "agentic" && (
          <div className="composer-row agentic-row">
            <label>
              {t("composer.node")}
              <select className="pill" value={nodeId} onChange={(e) => setNodeId(e.target.value)}>
                {props.workers.map((n) => (
                  <option key={n.node_id} value={n.node_id}>
                    {n.embedded ? `local (${n.hostname})` : n.hostname}
                  </option>
                ))}
              </select>
            </label>
            <label className="grow">
              {t("composer.folder")}
              <input
                className="mono"
                list="recent-folders"
                placeholder="/home/me/project"
                value={cwd}
                onChange={(e) => setCwd(e.target.value)}
              />
              <datalist id="recent-folders">
                {recentFolders().map((f) => (
                  <option key={f} value={f} />
                ))}
              </datalist>
            </label>
          </div>
        )}
      </div>
      {home && props.agenticBlocked && <div className="hint muted">{props.agenticBlocked}</div>}
      {err && <div className="error-text">{err}</div>}
    </div>
  );
}
