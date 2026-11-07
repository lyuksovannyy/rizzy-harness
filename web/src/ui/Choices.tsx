// A `ui:choices` block (rich_plan §6.4): the model's question and options
// as React text, answered with ids. A single choice without "Other" sends
// on the click; otherwise the user picks and presses the harness's Send.
// Keys 1-8 move to an option (and tick it in a multiple choice); they
// never send by themselves.
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { rt } from "../md/strings";
import type { ChoicesSpec } from "../types";
import { checkReply, isBlank } from "./form";
import { InputFrame, loadDraft, replyOf, saveDraft, SendButton, useSend, type InputProps } from "./Input";

interface Draft {
  selected: string[];
  other: string;
}

/** A stored draft, if it still fits the question. */
function restore(spec: ChoicesSpec, raw: unknown): Draft {
  const d = raw as Partial<Draft> | null;
  const selected = Array.isArray(d?.selected)
    ? d.selected.filter((id) => spec.options.some((o) => o.id === id))
    : [];
  return { selected, other: spec.other && typeof d?.other === "string" ? d.other : "" };
}

export default function Choices({ spec, input }: { spec: ChoicesSpec; input: InputProps }) {
  const [draft, setDraft] = useState<Draft>(() => restore(spec, loadDraft(input)));
  const { ready, busy, error, send, frame } = useSend(input);
  const buttons = useRef<(HTMLElement | null)[]>([]);
  const titleId = useId();
  const state = input.info.state;
  const open = state.kind === "open";
  useEffect(() => {
    if (open) saveDraft(input, draft, !draft.selected.length && !draft.other);
  }, [draft, open]);

  // What the card shows: the answer once given, else the user's picks.
  const shown = state.kind === "answered" ? restore(spec, state.values) : draft;
  const direct = !spec.multi && !spec.other;
  const values = () => ({ selected: draft.selected, ...(isBlank(draft.other) ? {} : { other: draft.other }) });
  const problem = checkReply("choices", spec, replyOf(input, values()));
  // A typed "Other" is one of the answers "Pick N" allows.
  const max = spec.max ?? Infinity;
  const answers = (d: Draft) => d.selected.length + (isBlank(d.other) ? 0 : 1);
  const full = spec.multi && answers(draft) >= max;

  const pick = (id: string) =>
    setDraft((d) => {
      if (spec.multi) {
        if (d.selected.includes(id)) return { ...d, selected: d.selected.filter((x) => x !== id) };
        return answers(d) < max ? { ...d, selected: [...d.selected, id] } : d;
      }
      // One answer: an option or "Other", not both.
      return { selected: d.selected[0] === id ? [] : [id], other: "" };
    });
  const onKey = (e: KeyboardEvent) => {
    const t = e.target as HTMLElement;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (t.tagName === "TEXTAREA" || (t.tagName === "INPUT" && (t as HTMLInputElement).type === "text")) return;
    const n = Number(e.key);
    const opt = spec.options[n - 1];
    if (!Number.isInteger(n) || !opt || !ready) return;
    e.preventDefault();
    buttons.current[n - 1]?.focus();
    if (!direct) pick(opt.id);
  };

  return (
    <InputFrame info={input.info} error={error}>
      {/* One group named by the question, so the options are read with it. */}
      <div className="ui-choices" tabIndex={-1} role="group" aria-labelledby={titleId} ref={frame} onKeyDown={onKey}>
        <p className="ui-title" id={titleId}>
          {spec.question}
        </p>
        {spec.multi && (
          <p className="ui-hint">
            {spec.min === spec.max
              ? rt("ui.pickN", { n: String(spec.min) })
              : rt("ui.pickRange", { min: String(spec.min), max: String(spec.max) })}
          </p>
        )}
        <div className={`ui-options${spec.multi ? " ui-options-multi" : ""}`}>
          {spec.options.map((o, i) => {
            const chosen = shown.selected.includes(o.id);
            const text = (
              <>
                <span className="ui-option-label">{o.label}</span>
                {o.description && <span className="ui-option-desc">{o.description}</span>}
              </>
            );
            const cls = `ui-option${chosen ? " ui-option-chosen" : ""}`;
            return spec.multi ? (
              <label key={o.id} className={cls}>
                <input
                  type="checkbox"
                  ref={(el) => void (buttons.current[i] = el)}
                  checked={chosen}
                  // At the most: the others wait until one is unticked.
                  disabled={!ready || (!chosen && full)}
                  onChange={() => pick(o.id)}
                />
                <span className="ui-option-text">{text}</span>
              </label>
            ) : (
              <button
                key={o.id}
                type="button"
                ref={(el) => void (buttons.current[i] = el)}
                className={cls}
                aria-pressed={chosen}
                disabled={!ready}
                onClick={() => {
                  if (!direct) return pick(o.id);
                  setDraft({ selected: [o.id], other: "" });
                  void send({ selected: [o.id] });
                }}
              >
                <span className="ui-option-text">{text}</span>
              </button>
            );
          })}
        </div>
        {spec.other &&
          (state.kind === "answered" ? (
            shown.other && (
              <p className="ui-other-answer">
                {rt("ui.otherLabel")}: {shown.other}
              </p>
            )
          ) : (
            <input
              type="text"
              className="ui-other"
              placeholder={rt("ui.other")}
              aria-label={rt("ui.otherLabel")}
              value={draft.other}
              // Once full it can still be cleared, never started.
              disabled={!ready || (full && isBlank(draft.other))}
              onChange={(e) => {
                const other = e.target.value;
                setDraft((d) => ({ selected: spec.multi || isBlank(other) ? d.selected : [], other }));
              }}
            />
          ))}
        {!direct && state.kind !== "answered" && (
          <div className="ui-actions">
            <SendButton disabled={!ready || problem !== null} busy={busy} onClick={() => void send(values())} />
          </div>
        )}
      </div>
    </InputFrame>
  );
}
