// What the chat's background strip shows (T.8d step 2,
// docs/background-commands.md §4.11), derived from the fold only: the same
// after a reload and in a second tab. Only the strip's lazy chunk and its
// tests import this.
import type { SessionView } from "../types";

/** A command's head: its first line, at most this many characters. */
export const HEAD_CHARS = 80;

export interface BgRow {
  bg_id: string;
  /** The head of the command line as the model wrote it: untrusted text,
   * shown as text (never HTML), hidden characters marked by the caller. */
  head: string;
  /** The command has more than its head (more lines or characters). */
  cut: boolean;
  /** When its timeout ends it at the latest, on the node's clock. */
  ends_by_ms: number;
  timeout_s: number;
}

/** The head of `command`: its first non-empty line, cut at HEAD_CHARS
 * characters (code points, so a pair is never split). */
export function commandHead(command: unknown): { head: string; cut: boolean } {
  if (typeof command !== "string") return { head: "", cut: false };
  const lines = command.split(/\r\n|\r|\n/);
  const first = lines.findIndex((l) => l.trim() !== "");
  if (first < 0) return { head: "", cut: false };
  const line = lines[first]!.trim();
  const chars = Array.from(line);
  const more = lines.slice(first + 1).some((l) => l.trim() !== "");
  return { head: chars.slice(0, HEAD_CHARS).join(""), cut: chars.length > HEAD_CHARS || more };
}

/** The commands the log shows running, in the order they started. */
export function runningBackground(view: SessionView | null | undefined): BgRow[] {
  if (!view?.background) return [];
  const rows: BgRow[] = [];
  for (const [bg_id, b] of Object.entries(view.background)) {
    if (b.ended) continue;
    let command: unknown;
    for (let i = view.items.length - 1; i >= 0; i--) {
      const it = view.items[i]!;
      if (it.type === "tool" && it.call_id === b.call_id) {
        command = it.input?.command;
        break;
      }
    }
    rows.push({ bg_id, ...commandHead(command), ends_by_ms: b.ends_by_ms, timeout_s: b.timeout_s });
  }
  // bg-<n> numbers in start order: bg-10 after bg-9.
  const n = (id: string) => Number(id.slice(3)) || 0;
  return rows.sort((a, b) => n(a.bg_id) - n(b.bg_id));
}

/** How `bg_id` ended, as the log says, or null while it runs. */
export function endOf(view: SessionView | null | undefined, bg_id: string): { reason: string; exit_code: number | null } | null {
  if (!view?.background?.[bg_id]?.ended) return null;
  for (let i = view.items.length - 1; i >= 0; i--) {
    const it = view.items[i]!;
    if (it.type === "background_ended" && it.bg_id === bg_id) return { reason: it.reason, exit_code: it.exit_code };
  }
  return { reason: "ended", exit_code: null };
}

/**
 * Milliseconds left before the timeout ends `row`, as far as this browser
 * can tell. The deadline is the node's clock and `now` is the browser's;
 * they may differ, so the count is held to what cannot be wrong by more
 * than the skew: never below zero, never above the command's own timeout.
 * The strip never acts on it: only the log's end removes a row.
 */
export function msLeft(row: Pick<BgRow, "ends_by_ms" | "timeout_s">, now: number): number {
  return Math.min(Math.max(row.ends_by_ms - now, 0), row.timeout_s * 1000);
}

/** `ms` in words, coarse on purpose (the clocks are not exact). */
export function leftText(ms: number): string {
  if (ms <= 0) return "at its timeout";
  const s = Math.ceil(ms / 1000);
  if (s < 60) return `${s} s left`;
  const m = Math.ceil(s / 60);
  if (m < 120) return `${m} min left`;
  return `${Math.ceil(m / 60)} h left`;
}

/** An end's reason in words, for the strip (the log's snake_case). */
export function reasonText(reason: string, exit_code: number | null): string {
  const words: Record<string, string> = {
    exited: "exited",
    timed_out: "reached its timeout",
    stopped_by_agent: "stopped by the agent",
    stopped_by_user: "stopped",
    access_revoked: "stopped: the owner lost access",
    node_stopping: "stopped with its node",
    node_restarted: "ended with its node",
  };
  const w = words[reason] ?? reason.replace(/_/g, " ");
  return exit_code != null && reason === "exited" ? `${w} with code ${exit_code}` : w;
}
