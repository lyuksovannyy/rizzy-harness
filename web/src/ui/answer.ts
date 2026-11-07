// An answer as the user reads it: the labels of what they picked and the
// values they typed, from the block it answers. Shared by the reply bubble
// (in the Conversation chunk) and the answered card. The model never sees
// this; it gets the canonical text of ids and values.
import { rt } from "../md/strings";
import type { Choice, Field, FormValue, UiBlock } from "../types";
import { num } from "./form";

export interface AnswerLine {
  /** The field's label; null for a question's single line. */
  label: string | null;
  value: string;
}

type Values = Record<string, FormValue>;

const ids = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);

const labels = (options: Choice[], picked: string[]) =>
  options.filter((o) => picked.includes(o.id)).map((o) => o.label);

/** The question or form title an answer is for. */
export function answerTitle(block: UiBlock): string {
  if (block.type === "choices") return block.spec?.question ?? "";
  if (block.type === "form") return block.spec?.title ?? "";
  return "";
}

export function answerLines(block: UiBlock, values: Values): AnswerLine[] {
  if (block.type === "choices" && block.spec) {
    const parts = labels(block.spec.options, ids(values.selected));
    if (typeof values.other === "string") parts.push(`${rt("ui.otherLabel")}: ${values.other.trim()}`);
    return [{ label: null, value: parts.join(", ") }];
  }
  if (block.type === "form" && block.spec) {
    return block.spec.fields
      .filter((f) => !f.refused && values[f.id] !== undefined)
      .map((f) => ({ label: f.label, value: fieldValue(f, values[f.id]!) }));
  }
  return [];
}

function fieldValue(f: Field, v: FormValue): string {
  switch (f.type) {
    case "number":
      return typeof v === "number" ? `${num(v)}${f.unit ? ` ${f.unit}` : ""}` : String(v);
    case "choice":
      return labels(f.options ?? [], Array.isArray(v) ? v : [String(v)]).join(", ");
    case "toggle":
      return rt(v === true ? "ui.yes" : "ui.no");
    default:
      return typeof v === "string" ? v.trim() : String(v);
  }
}
