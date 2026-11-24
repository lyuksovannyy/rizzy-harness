// What a background command prints (X.62): the answer of
// `GET /api/sessions/{id}/background/{bg}/output`, and the words around it.
// Only the side panel's lazy chunk and its tests import this.
import { call } from "../api";

/** The node's answer: the last bytes it holds of the command's output as
 * plain text (the node removed terminal escapes and bounded it). */
export interface BgOutput {
  running: boolean;
  /** How long the command ran, so far or in all. */
  ran_ms: number;
  /** Bytes it printed in all. */
  printed: number;
  text: string;
  /** `text` is not all it printed. */
  cut?: boolean;
  /** The node no longer keeps its output. */
  dropped?: boolean;
}

/** How often an open row looks again while its command runs. */
export const POLL_MS = 3000;

export const readBgOutput = (session: string, bg: string, signal?: AbortSignal) =>
  call<BgOutput>("GET", `/api/sessions/${encodeURIComponent(session)}/background/${encodeURIComponent(bg)}/output`, undefined, signal);

/** `ms` as "12 s", "3 min 4 s", "1 h 5 min": coarse, for a row. */
export function ranWords(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min ${s % 60} s`;
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}

/** `n` bytes, as the node counts what a command printed. */
export function bytesWords(n: number): string {
  if (n < 1024) return `${n} bytes`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}
