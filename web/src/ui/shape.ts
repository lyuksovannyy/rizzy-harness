// A `ui:` block's spec is checked in Rust where the node writes the message
// (crates/core/src/ui), and the fold keeps it as sent. A node that skipped
// that (older, buggy, hostile) can send any shape, and the cards read inside
// it, so each spec is checked here against types.ts before its card renders
// (review of PR 156: `{}` as a choices spec threw and took the chat down).
// A spec that fails shows one line instead of the card.
import type { UiBlock } from "../types";

type Check = (x: unknown) => boolean;
const obj = (x: unknown): x is Record<string, unknown> => !!x && typeof x == "object" && !Array.isArray(x);
const str: Check = (x) => typeof x == "string";
const num: Check = (x) => typeof x == "number" && Number.isFinite(x);
const bool: Check = (x) => typeof x == "boolean";
/** Rust's `valid_id` (crates/core/src/ui/read.rs): never "__proto__". */
const id: Check = (x) => typeof x == "string" && /^[a-z][a-z0-9_-]{0,31}$/.test(x);
const opt = (c: Check): Check => (x) => x === undefined || x === null || c(x);
const oneOf = (...v: string[]): Check => (x) => typeof x == "string" && v.includes(x);
const list = (c: Check): Check => (x) => Array.isArray(x) && x.every(c);
const either = (...cs: Check[]): Check => (x) => cs.some((c) => c(x));
const shape = (s: Record<string, Check>): Check => (x) => obj(x) && Object.entries(s).every(([k, c]) => c(Object.hasOwn(x, k) ? x[k] : undefined));

const choice = shape({ id, label: str, description: opt(str) });
const choices = shape({ question: str, options: list(choice), multi: opt(bool), other: opt(bool), min: opt(num), max: opt(num) });
const field = shape({
  id,
  label: str,
  type: oneOf("text", "number", "choice", "toggle", "date"),
  refused: opt(bool),
  required: opt(bool),
  help: opt(str),
  multiline: opt(bool),
  max_length: opt(num),
  placeholder: opt(str),
  min: opt(either(num, str)),
  max: opt(either(num, str)),
  step: opt(num),
  default: opt(either(str, num, bool, list(str))),
  unit: opt(str),
  options: opt(list(choice)),
  multi: opt(bool),
});
const form = shape({ title: str, description: opt(str), fields: list(field) });
const progress = shape({
  title: str,
  status: opt(oneOf("running", "done", "failed")),
  value: opt(num),
  steps: list(shape({ label: str, status: oneOf("pending", "active", "done", "failed", "skipped"), detail: opt(str) })),
});
const chart = shape({
  kind: oneOf("bar", "line"),
  title: opt(str),
  x: shape({ label: opt(str), values: either(list(str), list(num)) }),
  y: opt(shape({ label: opt(str), unit: opt(str) })),
  series: list(shape({ name: str, values: list(either(num, (x) => x === null)) })),
  stacked: opt(bool),
});
const SPECS: Record<UiBlock["type"], Check> = { choices, form, progress, chart };

/** The block's spec has the shape its card reads (an absent spec is the block's error line). */
export function specFits(block: UiBlock): boolean {
  const c = Object.hasOwn(SPECS, block.type) ? SPECS[block.type] : undefined;
  return !!c && c(block.spec);
}
