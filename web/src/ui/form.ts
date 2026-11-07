// The answer rules of crates/core/src/ui/answer.rs and reply.rs
// (`answer_reply`), for inline errors and the Send button only: the node
// checks every answer again against its own log, and that check is the one
// that counts. ui.json holds the two to the same verdict and the same first
// error for every reply case.
import type { Choice, ChoicesSpec, Field, FormSpec, FormValue } from "../types";

/** The longest "Other…" answer. */
export const MAX_OTHER = 500;

/** The largest answer, serialized, in bytes (MAX_REPLY in reply.rs). */
export const MAX_REPLY = 32 * 1024;
export const TOO_LARGE = "the answer is larger than 32 KiB";

export type Values = Record<string, unknown>;

/** An answer as it is sent; `values` may hold anything until checked. */
export interface Reply {
  message_id: string;
  block_id: string;
  values: Values;
}

type Err = string | null;

const VALID_ID = /^[a-z][a-z0-9_-]{0,31}$/;

// Rust's char::is_whitespace (Unicode White_Space), which String::trim
// uses; JS trim() differs (it trims U+FEFF, keeps U+0085).
const BLANK = /^[\t\n\v\f\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]*$/;
export const isBlank = (s: string) => BLANK.test(s);

const has = (v: Values, k: string) => Object.hasOwn(v, k) && v[k] !== undefined;
const keys = (v: Values) => Object.keys(v).filter((k) => has(v, k)).sort();

/** The first reason `reply` does not answer the block, worded as Rust
 * words it, or null when it does. */
export function checkReply(type: "choices" | "form", spec: ChoicesSpec | FormSpec, reply: Reply): Err {
  // Every field has its own limit; long text fields can pass them all and
  // still add up to more than the node takes. JSON.stringify escapes
  // strings as serde_json does, so the two count the same bytes (numbers
  // may differ by a byte or two in exponent form).
  if (new TextEncoder().encode(JSON.stringify(reply)).length > MAX_REPLY) return TOO_LARGE;
  const v = reply.values;
  return type === "choices" ? choices(spec as ChoicesSpec, v) : form(spec as FormSpec, v);
}

function choices(spec: ChoicesSpec, v: Values): Err {
  const unknown = unknownKey(v, (k) => k === "selected" || k === "other", "unknown key");
  if (unknown) return unknown;
  if (!has(v, "selected")) return "selected: missing";
  const picked = optionIds(v.selected, spec.options, "selected");
  if (typeof picked === "string") return picked;
  let other = false;
  if (has(v, "other")) {
    if (!spec.other) return "other: not allowed for this question";
    const e = typedText(v.other, "other", MAX_OTHER, false);
    if (e) return e;
    other = true;
  }
  const n = picked.length + (other ? 1 : 0);
  if (spec.multi) {
    // "Other" is one more answer, toward the minimum and the maximum alike.
    const min = spec.min ?? 1;
    const max = spec.max ?? Infinity;
    if (n > max) return `selected: pick at most ${max}`;
    if (n < min) return `selected: pick at least ${min}`;
  } else if (n === 0) return "selected: pick one";
  else if (n > 1) return "selected: pick only one";
  return null;
}

function form(spec: FormSpec, v: Values): Err {
  const field = (k: string) => spec.fields.find((f) => f.id === k);
  const unknown = unknownKey(v, (k) => !!field(k), "not a field of this form");
  if (unknown) return unknown;
  const secret = keys(v).find((k) => field(k)?.refused);
  if (secret) return `${secret}: asks for a secret, so it takes no value`;
  for (const f of spec.fields) {
    if (f.refused) continue;
    if (!has(v, f.id)) {
      if (f.required) return `${f.id}: required`;
      continue;
    }
    const e = fieldError(f, v[f.id]);
    if (e) return `${f.id}: ${e}`;
  }
  return null;
}

/** Why one field's value is wrong (without the field id), or null. */
export function fieldError(f: Field, v: unknown): Err {
  const strip = (e: Err) => e && e.slice(f.id.length + 2);
  switch (f.type) {
    case "text":
      return strip(typedText(v, f.id, f.max_length ?? 500, !!f.multiline));
    case "number": {
      if (typeof v !== "number" || !Number.isFinite(v)) return "expected a number";
      const min = typeof f.min === "number" ? f.min : undefined;
      const max = typeof f.max === "number" ? f.max : undefined;
      if (min !== undefined && v < min) return `must be at least ${num(min)}`;
      if (max !== undefined && v > max) return `must be at most ${num(max)}`;
      if (f.step !== undefined && !onGrid(v, min ?? 0, f.step)) {
        return `must be in steps of ${num(f.step)} from ${num(min ?? 0)}`;
      }
      return null;
    }
    case "choice": {
      const options = f.options ?? [];
      if (!f.multi) {
        if (typeof v !== "string") return "expected an option id";
        return options.some((o) => o.id === v) ? null : "not one of the options";
      }
      const picked = optionIds(v, options, f.id);
      if (typeof picked === "string") return strip(picked);
      if (typeof f.min === "number" && picked.length < f.min) return `pick at least ${f.min}`;
      if (typeof f.max === "number" && picked.length > f.max) return `pick at most ${f.max}`;
      return null;
    }
    case "toggle":
      return typeof v === "boolean" ? null : "expected true or false";
    case "date": {
      if (typeof v !== "string" || !isDate(v)) return "not a date (YYYY-MM-DD)";
      // YYYY-MM-DD strings order like the dates they name.
      if (typeof f.min === "string" && v < f.min) return `must be ${f.min} or later`;
      if (typeof f.max === "string" && v > f.max) return `must be ${f.max} or earlier`;
      return null;
    }
  }
}

// Sorted, as in Rust. (JS sorts by UTF-16 units, Rust by bytes; they agree
// except between astral and high-BMP characters, which ids never hold.)
function unknownKey(v: Values, known: (k: string) => boolean, why: string): Err {
  const k = keys(v).find((k) => !known(k));
  if (k === undefined) return null;
  return VALID_ID.test(k) ? `${k}: ${why}` : `values: ${why}`;
}

function optionIds(v: unknown, options: Choice[], at: string): string[] | string {
  if (!Array.isArray(v)) return `${at}: expected a list of option ids`;
  const out: string[] = [];
  for (const id of v) {
    if (typeof id !== "string") return `${at}: expected a list of option ids`;
    if (!options.some((o) => o.id === id)) return `${at}: not one of the options`;
    if (out.includes(id)) return `${at}: an option is picked twice`;
    out.push(id);
  }
  return out;
}

function typedText(v: unknown, at: string, max: number, multiline: boolean): Err {
  if (typeof v !== "string") return `${at}: expected text`;
  if (isBlank(v)) return `${at}: must not be empty`;
  if (!multiline && /[\n\r]/.test(v)) return `${at}: must be one line`;
  // Unicode scalar values, as Rust's chars().count().
  if ([...v].length > max) return `${at}: longer than ${max} characters`;
  return null;
}

/** A number as the harness writes it: 3, not 3.0, and never with an
 * exponent (Rust's f64 Display): 1e21 is 1000000000000000000000. */
export function num(n: number): string {
  if (Number.isInteger(n) && Math.abs(n) < 9e15) return String(Math.trunc(n) || 0);
  // String() already gives the shortest digits that read back as n, as
  // Rust does; only the exponent form needs writing out.
  const m = /^(-?)(\d)(?:\.(\d+))?e([+-]\d+)$/.exec(String(n));
  if (!m) return String(n);
  const [, sign, lead, rest = "", exp] = m;
  const digits = lead + rest;
  // JS uses an exponent only from 1e21 up (more whole digits than the 17
  // significant ones) and below 1e-6, so the point never falls inside.
  const point = 1 + Number(exp);
  if (point <= 0) return `${sign}0.${"0".repeat(-point)}${digits}`;
  return sign + digits + "0".repeat(point - digits.length);
}

const onGrid = (v: number, base: number, step: number) => {
  const k = (v - base) / step;
  return Math.abs(k - Math.round(k)) < 1e-9;
};

/** A real calendar date written YYYY-MM-DD (year 0001 to 9999). */
export function isDate(s: string): boolean {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
  if (!m) return false;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const leap = (y % 4 === 0 && y % 100 !== 0) || y % 400 === 0;
  const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][mo - 1];
  return y >= 1 && days !== undefined && d >= 1 && d <= days;
}

/** What the form's inputs hold: text as typed, a toggle's state, picked
 * option ids. */
export type Draft = Record<string, string | boolean | string[]>;

/** The inputs a form starts with: its defaults (text fields have none). */
export function initialDraft(spec: FormSpec): Draft {
  const d: Draft = {};
  for (const f of spec.fields) {
    if (f.refused) continue;
    if (f.type === "toggle") d[f.id] = f.default === true;
    else if (f.type === "choice" && f.multi) d[f.id] = Array.isArray(f.default) ? f.default : [];
    else if (f.type !== "text" && f.default !== undefined) d[f.id] = String(f.default);
    else d[f.id] = "";
  }
  return d;
}

/** The values to send: blank inputs are left out, numbers parsed. */
export function formValues(spec: FormSpec, draft: Draft): Record<string, FormValue> {
  const out: Record<string, FormValue> = {};
  for (const f of spec.fields) {
    const v = draft[f.id];
    if (f.refused || v === undefined) continue;
    if (typeof v === "boolean") out[f.id] = v;
    else if (Array.isArray(v)) {
      if (v.length) out[f.id] = v;
    } else if (!isBlank(v)) {
      const n = Number(v);
      // An unparsable number stays text, so the check names it.
      out[f.id] = f.type === "number" && v.trim() !== "" && Number.isFinite(n) ? n : v;
    }
  }
  return out;
}
