// The bar's mode classifier: multinomial logistic regression over hashed
// features, trained online from the mode the user actually used. Rules decide
// until the model has proven itself on the same recent window.
import { fnv1a } from "./fnv";
import { MODES, type Mode } from "./modes";
import { ruleMode, signals } from "./rules";

export const BUCKETS = 4096;
const CLASSES = MODES.length;
const WINDOW = 100;
const MIN_LABELS = 30;
const LR = 0.5;

export interface ClassifierState {
  version: 1;
  weights: number[];
  bias: number[];
  labels: number;
  rules_window: number[];
  model_window: number[];
}

export function emptyState(): ClassifierState {
  return {
    version: 1,
    weights: new Array(BUCKETS * CLASSES).fill(0),
    bias: new Array(CLASSES).fill(0),
    labels: 0,
    rules_window: [],
    model_window: [],
  };
}

/** Accept a stored state only if it has the exact expected shape. */
export function parseState(v: unknown): ClassifierState {
  const s = v as ClassifierState | null;
  if (
    s &&
    s.version === 1 &&
    Array.isArray(s.weights) &&
    s.weights.length === BUCKETS * CLASSES &&
    Array.isArray(s.bias) &&
    s.bias.length === CLASSES
  ) {
    return { ...s, rules_window: s.rules_window ?? [], model_window: s.model_window ?? [] };
  }
  return emptyState();
}

const bucket = (str: string): number => fnv1a(str) % BUCKETS;

/** Hashed feature indices (with repeats) for a text. */
export function features(text: string): number[] {
  const lower = text.trim().toLowerCase();
  const out: number[] = [];
  const padded = ` ${lower.slice(0, 200)} `;
  for (let i = 0; i + 3 <= padded.length; i++) out.push(bucket(`c:${padded.slice(i, i + 3)}`));
  const s = signals(text);
  for (const w of s.words.slice(0, 40)) out.push(bucket(`w:${w}`));
  if (s.words[0]) out.push(bucket(`first:${s.words[0]}`));
  out.push(bucket(`len:${Math.min(s.words.length, 8)}`));
  for (const [k, on] of Object.entries({ url: s.url, path: s.path, code: s.code, dev: s.dev, q: s.question, m: s.mention })) {
    if (on) {
      // Strong signals get extra weight by repetition.
      for (let r = 0; r < 4; r++) out.push(bucket(`s:${k}`));
    }
  }
  return out;
}

function scores(st: ClassifierState, f: number[]): number[] {
  const norm = 1 / Math.sqrt(Math.max(1, f.length));
  return MODES.map((_, c) => {
    let z = st.bias[c]!;
    for (const i of f) z += st.weights[i * CLASSES + c]! * norm;
    return z;
  });
}

function softmax(z: number[]): number[] {
  const m = Math.max(...z);
  const e = z.map((x) => Math.exp(x - m));
  const sum = e.reduce((a, b) => a + b, 0);
  return e.map((x) => x / sum);
}

export function predict(st: ClassifierState, text: string): { mode: Mode; probs: number[] } {
  const p = softmax(scores(st, features(text)));
  let best = 0;
  for (let c = 1; c < CLASSES; c++) if (p[c]! > p[best]!) best = c;
  return { mode: MODES[best]!, probs: p };
}

/** One SGD step towards `label`. Mutates and returns `st`. */
export function update(st: ClassifierState, text: string, label: Mode): ClassifierState {
  const f = features(text);
  const p = softmax(scores(st, f));
  const y = MODES.indexOf(label);
  const norm = 1 / Math.sqrt(Math.max(1, f.length));
  for (let c = 0; c < CLASSES; c++) {
    const g = (c === y ? 1 : 0) - p[c]!;
    st.bias[c]! += LR * g * 0.1;
    for (const i of f) st.weights[i * CLASSES + c]! += LR * g * norm;
  }
  return st;
}

const acc = (w: number[]) => (w.length ? w.reduce((a, b) => a + b, 0) / w.length : 0);

/** The model only takes over once it beats the rules on the same window. */
export function modelTrusted(st: ClassifierState): boolean {
  return st.labels >= MIN_LABELS && acc(st.model_window) > acc(st.rules_window);
}

export function decide(st: ClassifierState, text: string, agenticAllowed: boolean): { mode: Mode; source: "rules" | "model" } {
  if (!modelTrusted(st)) return { mode: ruleMode(text, agenticAllowed), source: "rules" };
  const { probs } = predict(st, text);
  const order = MODES.map((m, i) => ({ m, p: probs[i]! })).sort((a, b) => b.p - a.p);
  const pick = order.find((o) => o.m !== "agentic" || agenticAllowed)!.m;
  return { mode: pick, source: "model" };
}

/** Learn from a submission: score both predictors, then train. */
export function record(st: ClassifierState, text: string, chosen: Mode, agenticAllowed: boolean): ClassifierState {
  const push = (w: number[], ok: boolean) => {
    w.push(ok ? 1 : 0);
    if (w.length > WINDOW) w.splice(0, w.length - WINDOW);
  };
  push(st.rules_window, ruleMode(text, agenticAllowed) === chosen);
  push(st.model_window, predict(st, text).mode === chosen);
  update(st, text, chosen);
  st.labels += 1;
  return st;
}

/** Compact for storage (4 decimals is plenty). */
export function serialize(st: ClassifierState): ClassifierState {
  const r = (x: number) => Math.round(x * 1e4) / 1e4;
  return { ...st, weights: st.weights.map(r), bias: st.bias.map(r) };
}
