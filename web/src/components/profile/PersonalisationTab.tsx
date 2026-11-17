// Settings → Personalisation (README "Personalisation", PLAN.md T74): how
// every agent of the person's own chats replies to them, and their personal
// instructions. Each choice is one fixed sentence the server owns; the
// instructions are their own words, at most 1,024 characters. An agent whose
// "Personalisation" switch is off (the agent's page) gets neither. Rows as in
// the other settings pages: a title and a line on the left, the control on
// the right. A lazy chunk of its own.
import { useEffect, useId, useState } from "react";
import "./profile.css";
import { pageApi } from "../../apiPages";
import { MEMORY_HASH } from "../../lib/routes";
import type { Personalisation, Prefs, ReplyLevel, ReplyStyle } from "../../types";
import { chars } from "../forms";

/** The server's `MAX_INSTRUCTIONS`. */
export const MAX_INSTRUCTIONS = 1024;

const STYLES: { value: ReplyStyle; label: string }[] = [
  { value: "default", label: "Default" },
  { value: "friendly", label: "Friendly" },
  { value: "professional", label: "Professional" },
  { value: "direct", label: "Direct" },
  { value: "concise", label: "Concise" },
];
const LEVELS: { value: ReplyLevel; label: string }[] = [
  { value: "default", label: "Default" },
  { value: "more", label: "More" },
  { value: "less", label: "Less" },
];
type Trait = "warmth" | "enthusiasm" | "emoji" | "headings";
const TRAITS: { key: Trait; title: string; hint: string }[] = [
  { key: "warmth", title: "Warmth", hint: "How warm and personal replies sound" },
  { key: "enthusiasm", title: "Enthusiasm", hint: "How upbeat replies sound" },
  { key: "emoji", title: "Emoji", hint: "How often replies use emoji" },
  { key: "headings", title: "Headings and lists", hint: "How often replies use headings and lists" },
];

function Row(props: { id: string; title: string; hint: string; children: React.ReactNode }) {
  return (
    <div className="setting-row">
      <div className="setting-text">
        <label className="setting-title" htmlFor={props.id}>
          {props.title}
        </label>
        <span className="setting-hint muted small" id={`${props.id}-hint`}>
          {props.hint}
        </span>
      </div>
      <div className="setting-control">{props.children}</div>
    </div>
  );
}

function Choice<T extends string>(props: { id: string; value: T; options: { value: T; label: string }[]; onChange: (v: T) => void }) {
  return (
    <select id={props.id} aria-describedby={`${props.id}-hint`} value={props.value} onChange={(e) => props.onChange(e.target.value as T)}>
      {props.options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  );
}

export default function PersonalisationTab(props: { prefs: Prefs; onSaved: (p: Prefs) => void }) {
  const id = useId();
  const [p, setP] = useState<Personalisation>(props.prefs.personalisation ?? {});
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => setP(props.prefs.personalisation ?? {}), [props.prefs]);
  const text = p.instructions ?? "";
  const n = chars(text.trim());
  const over = n > MAX_INSTRUCTIONS;
  const set = (n: Personalisation) => {
    setP(n);
    setMsg(null);
  };
  const save = async () => {
    if (over) return;
    setBusy(true);
    setErr(null);
    try {
      const saved = await pageApi.savePersonalisation({ ...p, instructions: text.trim() || null });
      props.onSaved(saved);
      setMsg("Saved. It applies from your next message.");
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      className="card stack personalisation"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <p className="muted small">
        For every agent in your own chats, unless an agent's Personalisation switch is off. Nobody else's chats get it.
      </p>
      <h3>Memory</h3>
      <div className="setting-row">
        <div className="setting-text">
          <span className="setting-title">Your memory</span>
          <span className="setting-hint muted small">What agents remember about you, from your chats and what you add yourself.</span>
        </div>
        <div className="setting-control">
          <a className="button-like" href={MEMORY_HASH}>
            Manage
          </a>
        </div>
      </div>
      <h3>Replies</h3>
      <Row id={`${id}-style`} title="Base style and tone" hint="The overall way replies sound">
        <Choice id={`${id}-style`} value={p.style ?? "default"} options={STYLES} onChange={(v) => set({ ...p, style: v })} />
      </Row>
      {TRAITS.map((x) => (
        <Row key={x.key} id={`${id}-${x.key}`} title={x.title} hint={x.hint}>
          <Choice id={`${id}-${x.key}`} value={p[x.key] ?? "default"} options={LEVELS} onChange={(v) => set({ ...p, [x.key]: v })} />
        </Row>
      ))}
      <h3>Personal instructions</h3>
      <div className="field">
        <label className="field-label" htmlFor={`${id}-instr`}>
          What every agent should know or do for you
        </label>
        <textarea
          id={`${id}-instr`}
          rows={6}
          value={text}
          aria-invalid={over}
          aria-describedby={`${id}-count`}
          placeholder="e.g. I work in metric units. Explain shell commands before running them."
          onChange={(e) => set({ ...p, instructions: e.target.value })}
        />
        <span id={`${id}-count`} className={`field-hint ${over ? "error-text" : ""}`}>
          {n} / {MAX_INSTRUCTIONS}
          {over ? ": too long." : "."} Given to the agent as your words; an agent's own instructions still apply.
        </span>
      </div>
      {err && <div className="error-text">{err}</div>}
      <div className="row end">
        {msg && <span className="muted">{msg}</span>}
        <button className="primary" disabled={busy || over}>
          Save
        </button>
      </div>
    </form>
  );
}
