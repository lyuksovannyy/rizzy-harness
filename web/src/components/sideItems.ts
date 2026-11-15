// What the side panel lists (SidePanel.tsx), derived from the chat's fold
// only, so a reload and a second tab show the same: the sub-agents its turn
// started, then its background commands. Rows of the latest turn stay
// listed once finished, so a person sees what ran; a background command
// still running from an earlier turn is listed too.
import { commandHead, endOf, type BgRow } from "../lib/background";
import { turnStart } from "../lib/callback";
import type { Item, SessionView } from "../types";
import { childStatus } from "./ToolCard";

export type RowState = "running" | "waiting" | "done" | "partial" | "failed" | "stopped";

export interface AgentRow {
  call_id: string;
  /** The child session, once the node linked it. */
  child: string | null;
  agent: string;
  /** The task's first line, as the model wrote it (shown as text). */
  task: string;
  cut: boolean;
  state: RowState;
  /** Its runs listed here: a follow-up (`delegate` with `session`, T68)
   * is the same sub-agent again, one row with its latest call. */
  runs: number;
}

export interface CommandRow extends BgRow {
  state: RowState;
  /** How it ended, in words, once it did. */
  ended: { reason: string; exit_code: number | null } | null;
}

type Tool = Extract<Item, { type: "tool" }>;

/** Where the latest turn starts in `items` (0 when none does). */
function latestTurn(items: readonly Item[]): number {
  for (let i = items.length - 1; i >= 0; i--) if (turnStart(items, i)) return i;
  return 0;
}

function agentState(it: Tool, turnRunning: boolean): RowState {
  if (it.approval && it.approval.decision === null) return "waiting";
  if (it.approval?.decision === "deny") return "stopped";
  if (!it.result) return turnRunning ? "running" : "stopped";
  const reported = childStatus(it.result.output);
  if (reported) return reported;
  return it.result.ok ? "done" : "failed";
}

function commandState(end: CommandRow["ended"]): RowState {
  if (!end) return "running";
  if (end.reason === "exited") return end.exit_code === 0 ? "done" : "failed";
  if (end.reason === "timed_out") return "failed";
  return "stopped";
}

/** `keep`: commands this tab asked to stop, listed until their end shows. */
export function sideItems(view: SessionView | null | undefined, keep: ReadonlySet<string> = new Set()): { agents: AgentRow[]; commands: CommandRow[] } {
  if (!view) return { agents: [], commands: [] };
  const items = view.items;
  const from = latestTurn(items);
  const turnRunning = view.running_turn !== null;
  const agents: AgentRow[] = [];
  const callIndex = new Map<string, number>();
  const runsOf = new Map<string, number>();
  items.forEach((it, i) => {
    if (it.type !== "tool") return;
    callIndex.set(it.call_id, i);
    if (it.tool !== "delegate") return;
    const output = it.result ? it.result.output : it.live_output;
    // Only the node's first line: a uuid the model wrote into an echoed
    // refusal is never taken for the child.
    const child = /^session: ([0-9a-f-]{36})(?:\n|$)/.exec(output)?.[1] ?? null;
    // Runs over the whole chat: a follow-up in a later turn counts the
    // earlier ones too.
    if (child) runsOf.set(child, (runsOf.get(child) ?? 0) + 1);
    const state = agentState(it, turnRunning);
    if (i < from && state !== "running") return;
    const { head, cut } = commandHead(typeof it.input?.task === "string" ? it.input.task : "");
    const row: AgentRow = {
      call_id: it.call_id,
      child,
      agent: typeof it.input?.agent === "string" ? it.input.agent : "?",
      task: head,
      cut,
      state,
      runs: child ? runsOf.get(child)! : 1,
    };
    // A follow-up to a child already listed takes its row's place.
    const at = child ? agents.findIndex((a) => a.child === child) : -1;
    if (at >= 0) agents[at] = row;
    else agents.push(row);
  });
  const commands: CommandRow[] = [];
  for (const [bg_id, b] of Object.entries(view.background ?? {})) {
    const ended = endOf(view, bg_id);
    const at = callIndex.get(b.call_id) ?? -1;
    if (ended && at < from && !keep.has(bg_id)) continue;
    const it = at >= 0 ? (items[at] as Tool) : undefined;
    commands.push({
      bg_id,
      ...commandHead(it?.input?.command),
      ends_by_ms: b.ends_by_ms,
      timeout_s: b.timeout_s,
      state: commandState(ended),
      ended,
    });
  }
  // bg-<n> numbers in start order: bg-10 after bg-9.
  const n = (id: string) => Number(id.slice(3)) || 0;
  commands.sort((a, b) => n(a.bg_id) - n(b.bg_id));
  return { agents, commands };
}
