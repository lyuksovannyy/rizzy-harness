// Pure derivations over the folded timeline for ui blocks (rich_plan §6.2).
// Nothing here is stored: a reload folds the same log and gets the same
// answer.
import { turnStart } from "../lib/callback";
import type { FormValue, Item, ProgressSpec, SessionView, TurnEndReason } from "../types";

/** Where one progress block stands in its turn. */
export interface ProgressSlot {
  /** The model repeats a progress block with the same id after each phase:
   * the turn's latest one shows in full, earlier ones as one-line
   * snapshots of where the job stood then. */
  mode: "full" | "snapshot";
  /** The turn is over, so an active step will not move any more. */
  ended: boolean;
  /** How the turn ended, when its end is in the log. */
  reason?: TurnEndReason;
}

/** Keyed by slotKey(message_id, block ordinal). */
export type ProgressSlots = ReadonlyMap<string, ProgressSlot>;

export const slotKey = (messageId: string, ordinal: number) => `${messageId}#${ordinal}`;

type Assistant = Extract<Item, { type: "assistant" }>;

/** An answer of a parallel group that the conversation did not continue
 * from: its blocks are not part of the turn's job. */
function unchosen(a: Assistant, selection: Record<string, string>): boolean {
  if (!a.variant) return false;
  const chosen = selection[a.variant.group];
  return chosen === undefined ? a.variant.index !== 0 : chosen !== a.message_id;
}

/**
 * The slot of every shown progress block (one with a spec). A turn is the
 * items from one user item to the next; within it, blocks are grouped by id
 * across messages, and the last of each group is the full card. An
 * unchosen parallel answer keeps its own blocks in full, outside the group.
 */
export function progressSlots(
  items: readonly Item[],
  view: Pick<SessionView, "running_turn" | "variant_selection">,
): Map<string, ProgressSlot> {
  const slots = new Map<string, ProgressSlot>();
  const starts = items.flatMap((_, i) => (turnStart(items, i) ? [i] : []));
  const spans = [0, ...starts].map((from, s, all) => [from, all[s + 1] ?? items.length] as const);
  spans.forEach(([from, to], s) => {
    if (from === to) return;
    const span = items.slice(from, to);
    const end = span.find((it) => it.type === "turn_end");
    // A turn whose end is missing from the log is still over once a later
    // turn started or nothing runs; only the running one can move.
    const last = s === spans.length - 1;
    const ended = !!end || !last || !view.running_turn;
    const base = { ended, ...(end ? { reason: end.reason } : {}) };
    const groups = new Map<string, string[]>();
    for (const it of span) {
      if (it.type !== "assistant") continue;
      const own = unchosen(it, view.variant_selection);
      for (const b of it.blocks ?? []) {
        if (b.type !== "progress" || !b.spec) continue;
        const key = slotKey(it.message_id, b.ordinal);
        if (own) slots.set(key, { mode: "full", ...base });
        else groups.set(b.id, [...(groups.get(b.id) ?? []), key]);
      }
    }
    for (const keys of groups.values()) {
      keys.forEach((key, i) => slots.set(key, { mode: i === keys.length - 1 ? "full" : "snapshot", ...base }));
    }
  });
  return slots;
}

/** Changes whenever progressSlots' answer can: an item added, a message's
 * blocks stored, a turn starting or ending, another answer chosen. */
export function progressSignature(
  items: readonly Item[],
  view: Pick<SessionView, "running_turn" | "variant_selection">,
): string {
  let blocks = 0;
  for (const it of items) if (it.type === "assistant") blocks += it.blocks?.length ?? 0;
  return `${items.length}:${blocks}:${view.running_turn ?? ""}:${Object.values(view.variant_selection).join(",")}`;
}

export type ProgressStatus = "running" | "done" | "failed" | "stopped";

/** Steps that need no more work: done or skipped. */
export const finishedSteps = (spec: ProgressSpec) =>
  spec.steps.filter((s) => s.status === "done" || s.status === "skipped").length;

/**
 * The job's status: the model's when it gave one, else failed if a step
 * failed, done if every step is finished, else running. A job still
 * running when its turn ended has stopped: nothing will move it on.
 */
export function progressStatus(spec: ProgressSpec, ended: boolean): ProgressStatus {
  const derived = spec.steps.some((s) => s.status === "failed")
    ? "failed"
    : finishedSteps(spec) === spec.steps.length
      ? "done"
      : "running";
  const status = spec.status ?? derived;
  return status === "running" && ended ? "stopped" : status;
}

/** 0 to 1: the model's value, else the share of finished steps. */
export const progressValue = (spec: ProgressSpec) =>
  spec.value ?? (spec.steps.length ? finishedSteps(spec) / spec.steps.length : 0);

/** What a snapshot line names: the first active step, else the last done. */
export function currentStep(spec: ProgressSpec): string | null {
  const active = spec.steps.find((s) => s.status === "active");
  if (active) return active.label;
  return spec.steps.findLast((s) => s.status === "done")?.label ?? null;
}

/** Whether a question or form can be answered now, and if not, why not. */
export type InputState =
  | { kind: "open" }
  /** The next user message answered it, with these values. */
  | { kind: "answered"; values: Record<string, FormValue> }
  /** The conversation went on without an answer. */
  | { kind: "not_answered" }
  /** A turn is running; answers wait for it. */
  | { kind: "waiting" }
  /** One of several parallel answers that is not the one continued from. */
  | { kind: "choose_variant" }
  /** A sub-task's session: its questions go to the parent agent. */
  | { kind: "sub_task" };

/**
 * The state of the input block of the assistant item at `idx`, in this
 * order: a later user message closes it (answered when that message is
 * its reply); then a running turn, an unchosen parallel answer and a
 * sub-task session lock it; otherwise it is open. The node checks the same
 * against its log; this only decides what the card offers.
 */
export function inputState(
  items: readonly Item[],
  idx: number,
  view: Pick<SessionView, "running_turn" | "variant_selection" | "meta">,
): InputState {
  const it = items[idx];
  if (it?.type !== "assistant") return { kind: "not_answered" };
  const next = items.slice(idx + 1).find((x) => x.type === "user");
  if (next?.type === "user") {
    const block = inputBlock(it);
    const r = next.reply;
    return r && block && r.message_id === it.message_id && r.block_id === block.id
      ? { kind: "answered", values: r.values }
      : { kind: "not_answered" };
  }
  if (view.running_turn) return { kind: "waiting" };
  if (unchosen(it, view.variant_selection)) return { kind: "choose_variant" };
  if (view.meta?.parent_session) return { kind: "sub_task" };
  return { kind: "open" };
}

/** A message's shown question or form (Rust allows at most one). */
export const inputBlock = (it: Assistant) =>
  it.blocks?.find((b) => (b.type === "choices" || b.type === "form") && !!b.spec);
