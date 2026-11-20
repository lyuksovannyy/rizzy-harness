// Reviewing the newest plan of a chat in plan mode (ADOPTION_PLAN X.7): the
// plan as blocks, three kinds of note on a block or on text selected in one
// (comment, replacement, removal), and one decision: carry it out (only
// without notes) or send the notes back as ONE ordinary message the person
// reads and may edit first. Nothing else reaches the server: no new route,
// no new event. The design is plannotator's (research-skills/projects-a.md
// section 2); the code is ours.
//
// The plan is the agent's own words. It is drawn by the chat's Markdown
// renderer (no HTML, no remote images, links show their host); notes quote
// it as plain text. A revised plan is shown against the one before it, and a
// note stays with the plan it was written for: on a later plan it is listed
// as answered or orphaned, never moved onto text it was not written for.
// Its own lazy chunk, loaded on the first "Review the plan".
import { X } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { own } from "../emptyView";
import { t } from "../lib/i18n";
import "../lib/i18nPages";
import {
  diffBlocks,
  feedbackText,
  inPlanOrder,
  keepNotes,
  keptNotes,
  MAX_NOTE_CHARS,
  MAX_NOTES,
  splitPlan,
  type Block,
  type DiffRow,
  type Note,
  type NoteKind,
  type Plan,
} from "../lib/planReview";
import type { ApprovalMode } from "../types";
import { MODE_WORDS, whyNot } from "./ApprovalModePick";
import { Markdown } from "./Markdown";
import { CARRY_OUT, CHOICES } from "./PlanBar";
import "./planReview.css";

let seq = 0;

/** The text selected inside `el`, if any. */
function selectionIn(el: HTMLElement): string | null {
  const s = window.getSelection();
  if (!s || s.isCollapsed || !s.rangeCount || !el.contains(s.getRangeAt(0).commonAncestorContainer)) return null;
  const text = s.toString().trim();
  return text ? text.slice(0, 2000) : null;
}

interface Draft {
  blockId: string;
  /** What the block says, and what was selected in it. */
  whole: string;
  picked: string | null;
  whole_: boolean;
  kind: NoteKind;
  text: string;
  /** Editing this note. */
  id?: string;
}

const KIND_KEYS = { comment: "planReview.comment", replace: "planReview.replace", remove: "planReview.remove" } as const;
const FIELD_KEYS = { comment: "planReview.commentField", replace: "planReview.replaceField", remove: "planReview.removeField" } as const;

export default function PlanReview(props: {
  sessionId: string;
  /** Every plan of the chat, oldest first. */
  plans: readonly Plan[];
  /** The agent is answering: nothing is sent or carried out meanwhile. */
  running: boolean;
  /** The node's ceiling for the "carry out" choices. */
  max: ApprovalMode;
  /** Switch to the mode, then send the text (PlanBar's own route). */
  onCarry: (m: ApprovalMode, text: string) => Promise<void>;
  /** Send the text as the person's ordinary message in this chat. */
  onSend: (text: string) => Promise<void>;
  onClose: () => void;
}) {
  const { sessionId, plans, running } = props;
  const latest = plans[plans.length - 1] ?? null;
  const [shownId, setShownId] = useState(latest?.id ?? null);
  const shown = plans.find((p) => p.id === shownId) ?? latest;
  // A newer plan than the one on screen: the person has not looked at it.
  const stale = !!latest && !!shown && shown.id !== latest.id;
  const at = shown ? plans.indexOf(shown) : -1;
  const before = at > 0 ? plans[at - 1]! : null;

  const blocks = useMemo(() => (shown ? splitPlan(shown.text) : []), [shown?.id, shown?.text]);
  const rows: DiffRow[] = useMemo(
    () => (before ? diffBlocks(splitPlan(before.text), blocks) : blocks.map((block) => ({ status: "same" as const, block }))),
    [before?.id, before?.text, blocks],
  );
  const counts = { added: 0, changed: 0, removed: 0 };
  for (const r of rows) if (r.status !== "same") counts[r.status]++;

  const [notes, setNotes] = useState<Note[]>(() => keptNotes(sessionId));
  useEffect(() => keepNotes(sessionId, notes), [sessionId, notes]);
  const mine = notes.filter((n) => n.planId === shown?.id && !n.sent);
  const earlier = notes.filter((n) => n.planId !== shown?.id || n.sent);

  const [draft, setDraft] = useState<Draft | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [cursor, setCursor] = useState(0);
  const dialog = useRef<HTMLDivElement>(null);
  const head = useRef<HTMLHeadingElement>(null);
  const items = useRef(new Map<string, HTMLLIElement>());
  useEffect(() => head.current?.focus(), []);

  const annotatable = rows.filter((r) => r.status !== "removed");
  const move = (to: number) => {
    const i = Math.max(0, Math.min(annotatable.length - 1, to));
    setCursor(i);
    const r = annotatable[i];
    if (r) items.current.get(r.block.id)?.focus();
  };
  const open = (block: Block, el: HTMLElement, existing?: Note) => {
    const picked = existing ? (existing.selection ? existing.quote : null) : selectionIn(el);
    setErr(null);
    setDraft({
      blockId: block.id,
      whole: block.text,
      picked,
      whole_: !picked,
      kind: existing?.kind ?? "comment",
      text: existing?.text ?? "",
      id: existing?.id,
    });
  };
  const save = () => {
    if (!draft || !shown) return;
    const quote = draft.whole_ || !draft.picked ? draft.whole : draft.picked;
    const text = draft.text.slice(0, MAX_NOTE_CHARS);
    if (draft.kind !== "remove" && !text.trim()) return;
    if (!draft.id && notes.length >= MAX_NOTES) return setErr(t("planReview.limit"));
    const note: Note = {
      id: draft.id ?? `n${++seq}`,
      planId: shown.id,
      blockId: draft.blockId,
      quote,
      selection: !draft.whole_ && !!draft.picked,
      kind: draft.kind,
      text: draft.kind === "remove" ? text.trim() : text,
      sent: false,
    };
    setNotes((all) => (draft.id ? all.map((n) => (n.id === draft.id ? note : n)) : [...all, note]));
    setDraft(null);
    items.current.get(draft.blockId)?.focus();
  };

  const key = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      if (draft) {
        const b = draft.blockId;
        setDraft(null);
        items.current.get(b)?.focus();
      } else if (message !== null) setMessage(null);
      else props.onClose();
    } else if (e.key === "Tab" && dialog.current) {
      const f = [...dialog.current.querySelectorAll<HTMLElement>("button, textarea, input, summary, [tabindex='0']")].filter(
        (x) => !(x as HTMLButtonElement).disabled && x.tabIndex >= 0,
      );
      const first = f[0];
      const last = f[f.length - 1];
      if (first && last && e.shiftKey && document.activeElement === first) (e.preventDefault(), last.focus());
      else if (first && last && !e.shiftKey && document.activeElement === last) (e.preventDefault(), first.focus());
    }
  };

  const send = async () => {
    if (message === null || busy) return;
    setBusy(true);
    setErr(null);
    try {
      await props.onSend(message);
      const ids = new Set(mine.map((n) => n.id));
      keepNotes(sessionId, notes.map((n) => (ids.has(n.id) ? { ...n, sent: true } : n)));
      props.onClose();
    } catch (x) {
      setErr(t("planReview.sendFailed", { why: x instanceof Error ? x.message : String(x) }));
      setBusy(false);
    }
  };
  const carry = async (m: ApprovalMode) => {
    setBusy(true);
    setErr(null);
    try {
      await props.onCarry(m, CARRY_OUT);
      props.onClose();
    } catch (x) {
      setErr(x instanceof Error ? x.message : String(x));
      setBusy(false);
    }
  };

  const off = running || stale || busy;
  const forBlock = (id: string) => mine.filter((n) => n.blockId === id);

  return (
    <div className="plan-review-scrim">
      <div className="plan-review" role="dialog" aria-modal="true" aria-labelledby="plan-review-title" ref={dialog} onKeyDown={key}>
        <header className="pr-head">
          <h2 id="plan-review-title" className="pr-title" tabIndex={-1} ref={head}>
            {t("planReview.title")}
          </h2>
          <button type="button" className="icon" aria-label={t("planReview.close")} title={t("planReview.close")} onClick={props.onClose}>
            <X size={16} aria-hidden />
          </button>
        </header>
        {stale && (
          <div className="pr-banner" role="alert">
            {t("planReview.newer")}{" "}
            <button type="button" className="pr-btn" onClick={() => (setShownId(latest!.id), setDraft(null), setMessage(null), setCursor(0))}>
              {t("planReview.showNewer")}
            </button>
          </div>
        )}
        {running && (
          <div className="pr-banner" role="status">
            {t("planReview.running")}
          </div>
        )}
        {!shown ? (
          <p className="pr-body muted">{t("planReview.none")}</p>
        ) : message !== null ? (
          <div className="pr-body pr-compose">
            <p className="small">{t("planReview.preview")}</p>
            <label className="pr-field">
              <span className="small">{t("planReview.messageField")}</span>
              <textarea className="pr-text pr-message" value={message} onChange={(e) => setMessage(e.target.value)} rows={16} />
            </label>
          </div>
        ) : (
          <div className="pr-body">
            <div className="pr-plan">
              {before && (
                <p className="small muted" role="status">
                  {t("planReview.diff", { added: String(counts.added), changed: String(counts.changed), removed: String(counts.removed) })}
                </p>
              )}
              <ol className="pr-blocks" aria-label={t("planReview.planList")}>
                {rows.map((r) => {
                  const b = r.block;
                  const removed = r.status === "removed";
                  const idx = annotatable.indexOf(r);
                  const here = forBlock(b.id);
                  const kindWord = t(`planReview.kind.${b.kind}`);
                  return (
                    <li
                      key={`${r.status}:${b.id}`}
                      ref={(el) => {
                        if (el) items.current.set(b.id, el);
                        else items.current.delete(b.id);
                      }}
                      className={`pr-block pr-${r.status}`}
                      style={b.depth ? { marginLeft: `${Math.min(b.depth, 6) * 16}px` } : undefined}
                      data-block={b.id}
                      tabIndex={removed ? undefined : idx === cursor ? 0 : -1}
                      aria-label={removed ? undefined : t("planReview.block", { kind: kindWord, n: String(idx + 1), count: String(annotatable.length) })}
                      onFocus={(e) => {
                        if (!removed && e.target === e.currentTarget) setCursor(idx);
                      }}
                      onKeyDown={(e) => {
                        if (removed || e.target !== e.currentTarget) return;
                        if (e.key === "ArrowDown") (e.preventDefault(), move(idx + 1));
                        else if (e.key === "ArrowUp") (e.preventDefault(), move(idx - 1));
                        else if (e.key === "Home") (e.preventDefault(), move(0));
                        else if (e.key === "End") (e.preventDefault(), move(annotatable.length - 1));
                        else if (e.key === "n" || e.key === "N" || e.key === "Enter") (e.preventDefault(), open(b, e.currentTarget));
                      }}
                    >
                      {r.status !== "same" && <span className={`pr-flag pr-flag-${r.status}`}>{t(`planReview.${r.status}`)}</span>}
                      <div className="pr-md">
                        <Markdown text={b.text} done />
                      </div>
                      {r.before && (
                        <details className="pr-was">
                          <summary className="small">{t("planReview.before")}</summary>
                          <Markdown text={r.before.text} done />
                        </details>
                      )}
                      {!removed && (
                        <div className="pr-tools">
                          <button
                            type="button"
                            className="pr-btn"
                            tabIndex={idx === cursor ? 0 : -1}
                            onClick={(e) => open(b, e.currentTarget.closest("li")!)}
                          >
                            {t("planReview.addNote")}
                          </button>
                          {here.length > 0 && <span className="small muted">{t("planReview.noteCount", { n: String(here.length) })}</span>}
                        </div>
                      )}
                      {draft?.blockId === b.id && (
                        <form
                          className="pr-form"
                          onSubmit={(e) => {
                            e.preventDefault();
                            save();
                          }}
                        >
                          <fieldset className="pr-kinds">
                            <legend className="small">{t("planReview.kindLegend")}</legend>
                            {(Object.keys(KIND_KEYS) as NoteKind[]).map((k) => (
                              <label key={k}>
                                <input type="radio" name={`pr-kind-${b.id}`} checked={draft.kind === k} onChange={() => setDraft({ ...draft, kind: k })} />{" "}
                                {t(KIND_KEYS[k])}
                              </label>
                            ))}
                          </fieldset>
                          <p className="small muted">
                            {draft.picked && !draft.whole_ ? t("planReview.on") : t("planReview.onBlock")}{" "}
                            <q className="pr-quote">{(draft.picked && !draft.whole_ ? draft.picked : draft.whole).slice(0, 200)}</q>{" "}
                            {draft.picked && draft.id === undefined && (
                              <button type="button" className="pr-btn" onClick={() => setDraft({ ...draft, whole_: !draft.whole_ })}>
                                {draft.whole_ ? t("planReview.useSelection") : t("planReview.wholeBlock")}
                              </button>
                            )}
                          </p>
                          <label className="pr-field">
                            <span className="small">{t(FIELD_KEYS[draft.kind])}</span>
                            <textarea
                              className="pr-text"
                              autoFocus
                              value={draft.text}
                              maxLength={MAX_NOTE_CHARS}
                              rows={3}
                              onChange={(e) => setDraft({ ...draft, text: e.target.value })}
                            />
                          </label>
                          <div className="pr-row">
                            <button type="submit" className="pr-btn pr-primary">
                              {t("planReview.save")}
                            </button>
                            <button type="button" className="pr-btn" onClick={() => (setDraft(null), items.current.get(b.id)?.focus())}>
                              {t("planReview.cancel")}
                            </button>
                          </div>
                        </form>
                      )}
                    </li>
                  );
                })}
              </ol>
            </div>
            <aside className="pr-notes" aria-label={t("planReview.notes", { n: String(mine.length) })}>
              <h3 className="small pr-h" aria-live="polite">
                {t("planReview.notes", { n: String(mine.length) })}
              </h3>
              {mine.length === 0 && <p className="small muted">{t("planReview.noNotes")}</p>}
              <ul className="pr-list">
                {inPlanOrder(mine, blocks).map((n) => (
                  <NoteRow
                    key={n.id}
                    n={n}
                    onEdit={() => {
                      const b = blocks.find((x) => x.id === n.blockId);
                      const el = items.current.get(n.blockId);
                      if (b && el) open(b, el, n);
                    }}
                    onDelete={() => setNotes((all) => all.filter((x) => x.id !== n.id))}
                  />
                ))}
              </ul>
              {earlier.length > 0 && (
                <>
                  <h3 className="small pr-h">{t("planReview.earlier")}</h3>
                  <ul className="pr-list">
                    {earlier.map((n) => (
                      <NoteRow
                        key={n.id}
                        n={n}
                        status={n.sent ? (n.planId === shown.id ? t("planReview.sentSame") : t("planReview.answered")) : t("planReview.orphaned")}
                        onDelete={() => setNotes((all) => all.filter((x) => x.id !== n.id))}
                      />
                    ))}
                  </ul>
                </>
              )}
            </aside>
          </div>
        )}
        {err && (
          <div className="error-text small pr-err" role="alert">
            {err}
          </div>
        )}
        <footer className="pr-foot">
          {message !== null ? (
            <>
              <button type="button" className="pr-btn pr-primary" disabled={off || !message.trim()} onClick={() => void send()}>
                {t("planReview.sendNow")}
              </button>
              <button type="button" className="pr-btn" disabled={busy} onClick={() => setMessage(null)}>
                {t("planReview.back")}
              </button>
            </>
          ) : (
            <>
              {mine.length > 0 && (
                <button
                  type="button"
                  className="pr-btn pr-primary"
                  disabled={off}
                  onClick={() => setMessage(feedbackText(inPlanOrder(mine, blocks)))}
                >
                  {t("planReview.send", { n: String(mine.length) })}
                </button>
              )}
              <span className="pr-carry">
                <span className="small">{t("planReview.carryWith")}</span>
                {CHOICES.map((c) => {
                  const why = whyNot(c.mode, props.max);
                  return (
                    <button
                      key={c.mode}
                      type="button"
                      className="pr-btn"
                      disabled={off || !shown || mine.length > 0 || !!why}
                      title={why ?? `Switch to ${own(MODE_WORDS, c.mode)?.label ?? "another mode"} and send "${CARRY_OUT}"`}
                      onClick={() => void carry(c.mode)}
                    >
                      {c.label}
                    </button>
                  );
                })}
              </span>
              {mine.length > 0 && (
                <span className="pr-off small muted">
                  {t("planReview.carryOff")}{" "}
                  <button type="button" className="pr-btn" onClick={() => setNotes((all) => all.filter((n) => !mine.includes(n)))}>
                    {t("planReview.discard")}
                  </button>
                </span>
              )}
            </>
          )}
        </footer>
      </div>
    </div>
  );
}

function NoteRow(props: { n: Note; status?: string; onEdit?: () => void; onDelete: () => void }) {
  const { n } = props;
  return (
    <li className={`pr-note pr-note-${n.kind}`}>
      <span className="pr-kind">{t(KIND_KEYS[n.kind])}</span>
      {props.status && <span className="small muted"> {props.status}</span>}
      <blockquote className="pr-quote">{n.quote.length > 160 ? `${n.quote.slice(0, 160)}…` : n.quote}</blockquote>
      {n.text && <p className="pr-notetext">{n.text}</p>}
      <div className="pr-row">
        {props.onEdit && (
          <button type="button" className="pr-btn" onClick={props.onEdit}>
            {t("planReview.edit")}
          </button>
        )}
        <button type="button" className="pr-btn" onClick={props.onDelete}>
          {t("planReview.delete")}
        </button>
      </div>
    </li>
  );
}
