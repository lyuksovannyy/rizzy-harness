// A `ui:form` block (rich_plan §6.4): the model's fields as plain inputs
// (no password or hidden type; a field that asks for a secret is only a
// warning), sent with the harness's Send once the answers pass the same
// rules the node applies. Once answered it shows the values read-only.
import { useEffect, useId, useState } from "react";
import { rt } from "../md/strings";
import type { Field, FormSpec } from "../types";
import { answerLines } from "./answer";
import { checkReply, fieldError, formValues, initialDraft, TOO_LARGE, type Draft } from "./form";
import { InputFrame, loadDraft, replyOf, saveDraft, SendButton, useSend, type InputProps } from "./Input";

/** A stored draft, kept only where it still fits a shown field. */
function restore(spec: FormSpec, raw: unknown): Draft {
  const draft = initialDraft(spec);
  if (!raw || typeof raw !== "object") return draft;
  for (const [k, v] of Object.entries(raw as Record<string, unknown>)) {
    const was = draft[k];
    const same = Array.isArray(was) ? Array.isArray(v) && v.every((x) => typeof x === "string") : typeof v === typeof was;
    if (was !== undefined && same) draft[k] = v as Draft[string];
  }
  return draft;
}

const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

export default function Form({ spec, input }: { spec: FormSpec; input: InputProps }) {
  const [draft, setDraft] = useState<Draft>(() => restore(spec, loadDraft(input)));
  const { ready, busy, error, send } = useSend(input);
  const state = input.info.state;
  const open = state.kind === "open";
  const values = formValues(spec, draft);
  useEffect(() => {
    if (open) saveDraft(input, draft, Object.keys(formValues(spec, draft)).length === 0);
  }, [draft, open]);

  if (state.kind === "answered") {
    return (
      <InputFrame info={input.info} error={null}>
        <div className="ui-form">
          <p className="ui-title">{spec.title}</p>
          <dl className="ui-answers">
            {answerLines({ type: "form", spec, ordinal: 0, hash: "", id: input.blockId }, state.values).map((l, i) => (
              <div key={i}>
                <dt>{l.label}</dt>
                <dd>{l.value}</dd>
              </div>
            ))}
          </dl>
        </div>
      </InputFrame>
    );
  }
  const problem = checkReply("form", spec, replyOf(input, values));
  const set = (id: string, v: Draft[string]) => setDraft((d) => ({ ...d, [id]: v }));
  return (
    <InputFrame info={input.info} error={error}>
      <div className="ui-form">
        <p className="ui-title">{spec.title}</p>
        {spec.description && <p className="ui-description">{spec.description}</p>}
        <ul className="ui-fields">
          {spec.fields.map((f) => (
            <li key={f.id}>
              {f.refused ? (
                <Refused field={f} />
              ) : (
                <FieldRow
                  field={f}
                  value={draft[f.id]!}
                  set={(v) => set(f.id, v)}
                  disabled={!ready}
                  // Only a value can be wrong; a missing required one
                  // just keeps Send off.
                  error={f.id in values ? fieldError(f, values[f.id]) : null}
                />
              )}
            </li>
          ))}
        </ul>
        <div className="ui-actions">
          {/* The one problem no single field shows: all of it together. */}
          {problem === TOO_LARGE && (
            <p className="ui-field-error" role="alert">
              {cap(problem)}
            </p>
          )}
          <SendButton disabled={!ready || problem !== null} busy={busy} onClick={() => void send(values)} />
        </div>
      </div>
    </InputFrame>
  );
}

/** A field that asks for a secret: never an input, only the warning. */
function Refused({ field }: { field: Field }) {
  return (
    <div className="ui-secret" role="note">
      <span className="ui-field-label">{field.label}</span>: {rt("ui.secret")}
    </div>
  );
}

interface RowProps {
  field: Field;
  value: Draft[string];
  set: (v: Draft[string]) => void;
  disabled: boolean;
  error: string | null;
}

function FieldRow(p: RowProps) {
  const id = useId();
  const f = p.field;
  // Radio and checkbox groups are labelled as a group; one control by its id.
  const group = f.type === "choice" && (f.multi || (f.options?.length ?? 0) <= 5);
  return (
    <div className={`ui-field ui-field-${f.type}`} role={group ? "group" : undefined} aria-labelledby={group ? `${id}-l` : undefined}>
      {f.type !== "toggle" &&
        (group ? (
          <span id={`${id}-l`} className="ui-field-label">
            {f.label}
          </span>
        ) : (
          <label htmlFor={id} className="ui-field-label">
            {f.label}
          </label>
        ))}
      {f.required && <span className="muted"> ({rt("ui.required")})</span>}
      {f.help && <div className="ui-help">{f.help}</div>}
      <Control {...p} id={id} />
      {p.error && (
        <div id={`${id}-e`} className="ui-field-error">
          {cap(p.error)}
        </div>
      )}
    </div>
  );
}

function Control(p: RowProps & { id: string }) {
  const f = p.field;
  const str = typeof p.value === "string" ? p.value : "";
  const list = Array.isArray(p.value) ? p.value : [];
  // The field's error is read out with it.
  const describedBy = p.error ? `${p.id}-e` : undefined;
  switch (f.type) {
    case "text": {
      const max = f.max_length ?? 500;
      const attrs = {
        id: p.id,
        value: str,
        placeholder: f.placeholder,
        disabled: p.disabled,
        "aria-describedby": describedBy,
      };
      return (
        <>
          {f.multiline ? (
            <textarea {...attrs} rows={3} onChange={(e) => p.set(e.target.value)} />
          ) : (
            <input {...attrs} type="text" onChange={(e) => p.set(e.target.value)} />
          )}
          <span className="ui-count">
            {[...str].length}/{max}
          </span>
        </>
      );
    }
    case "number": {
      const min = typeof f.min === "number" ? f.min : undefined;
      const max = typeof f.max === "number" ? f.max : undefined;
      const slider = min !== undefined && max !== undefined && !!f.step && (max - min) / f.step <= 200;
      return (
        <span className="ui-number">
          <input
            id={p.id}
            type="number"
            inputMode="decimal"
            min={min}
            max={max}
            step={f.step ?? "any"}
            value={str}
            aria-describedby={describedBy}
            disabled={p.disabled}
            onChange={(e) => p.set(e.target.value)}
          />
          {f.unit && <span className="muted">{f.unit}</span>}
          {slider && (
            <input
              type="range"
              aria-label={rt("ui.slider", { label: f.label })}
              min={min}
              max={max}
              step={f.step}
              value={str === "" ? min : str}
              disabled={p.disabled}
              onChange={(e) => p.set(e.target.value)}
            />
          )}
        </span>
      );
    }
    case "choice": {
      const options = f.options ?? [];
      if (f.multi) {
        const toggle = (id: string) => p.set(list.includes(id) ? list.filter((x) => x !== id) : [...list, id]);
        return (
          <span className="ui-choice-list">
            {options.map((o) => (
              <label key={o.id}>
                <input type="checkbox" checked={list.includes(o.id)} disabled={p.disabled} onChange={() => toggle(o.id)} />{" "}
                {o.label}
              </label>
            ))}
          </span>
        );
      }
      if (options.length <= 5) {
        return (
          <span className="ui-choice-list">
            {options.map((o) => (
              <label key={o.id}>
                <input type="radio" name={p.id} checked={str === o.id} disabled={p.disabled} onChange={() => p.set(o.id)} />{" "}
                {o.label}
              </label>
            ))}
          </span>
        );
      }
      return (
        <select id={p.id} value={str} aria-describedby={describedBy} disabled={p.disabled} onChange={(e) => p.set(e.target.value)}>
          <option value="">{rt("ui.choose")}</option>
          {options.map((o) => (
            <option key={o.id} value={o.id}>
              {o.label}
            </option>
          ))}
        </select>
      );
    }
    case "toggle":
      return (
        <label className="ui-switch">
          <input
            type="checkbox"
            role="switch"
            checked={p.value === true}
            disabled={p.disabled}
            onChange={(e) => p.set(e.target.checked)}
          />
          <span className="ui-field-label">{f.label}</span>
        </label>
      );
    case "date":
      return (
        <input
          id={p.id}
          type="date"
          min={typeof f.min === "string" ? f.min : undefined}
          max={typeof f.max === "string" ? f.max : undefined}
          value={str}
          aria-describedby={describedBy}
          disabled={p.disabled}
          onChange={(e) => p.set(e.target.value)}
        />
      );
  }
}
