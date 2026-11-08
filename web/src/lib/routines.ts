// Routines in the browser (docs/routines.md): the wire types, the API calls,
// and the schedule builder's shapes, kept out of api.ts and types.ts so the
// page's code stays in its lazy chunk. Next runs and rendered prompts come
// from the server (`/preview`); the only cron here is the few shapes the
// builder writes, read back so a saved schedule opens in the same shape.
import { call } from "../api";
import type { Effort } from "../types";

export type RoutineProfile = "chat" | "agentic";

/** `routines.spec`; the server refuses fields it does not know. */
export interface RoutineSpec {
  profile: RoutineProfile;
  agent_id: string;
  /** null: the agent's default in the workspace. */
  model: string | null;
  effort: Effort | null;
  node_id?: string | null;
  cwd?: string | null;
  approval_mode?: "ask" | "ask_all" | null;
  prompt: string;
  project_id: string | null;
  catch_up: "skip" | "once";
  on_missing: "empty" | "skip";
  notify: "always" | "failures" | "never";
  max_runtime_min: number;
}

export type ScheduleConfig =
  | { type: "cron"; expr: string; tz: string }
  | { type: "interval"; every_minutes: number; anchor_ms: number }
  | { type: "once"; at: string; tz: string };

export type TriggerInput =
  | { trigger_id?: string; kind: "schedule"; config: ScheduleConfig }
  | { trigger_id?: string; kind: "webhook"; config: Record<string, never> };

export interface RoutineTrigger {
  trigger_id: string;
  kind: "schedule" | "webhook";
  config: ScheduleConfig | Record<string, never>;
  next_run_ms: number | null;
  last_fired_ms: number | null;
  url_path?: string;
  secret_set?: boolean;
}

export interface Run {
  run_id: string;
  routine_id: string;
  trigger_kind: "schedule" | "webhook" | "manual";
  scheduled_ms: number | null;
  started_ms: number;
  ended_ms: number | null;
  status: string;
  reason: string | null;
  session_id: string | null;
  chat_deleted: boolean;
  usage: { input_tokens: number; output_tokens: number } | null;
  warnings: string[];
  untrusted: boolean;
  attempt: number;
  repeats: number;
  seen: boolean;
  /** The first sentence of the chat's latest message. */
  answer_preview: string | null;
}

export interface RunDetail extends Run {
  prompt: string | null;
  event: unknown;
}

export interface Routine {
  routine_id: string;
  workspace_id: string;
  name: string;
  enabled: boolean;
  paused_reason: "blocked" | "failures" | "cap" | null;
  spec: RoutineSpec;
  triggers: RoutineTrigger[];
  next_run_ms: number | null;
  last_run: Run | null;
  unseen: number;
  /** Why it could not run now (a live check of the owner's access). */
  blocked: string | null;
  /** Webhook: paths in the last body received. */
  event_paths: string[];
  created_ms: number;
  updated_ms: number;
}

export interface VarSpec {
  path: string;
  example: string;
}

export interface RoutineList {
  routines: Routine[];
  workspace_id: string | null;
  can_create: boolean;
  can_agentic: boolean;
  /** Why the viewer may look but not create here (a guest). */
  why: string | null;
  limits: Record<string, number>;
  variables: { namespace: string; vars: VarSpec[] }[];
  sources: { kind: "webhook"; vars: VarSpec[]; free_prefix: string }[];
  /** The viewer still owns routines here but may no longer run them. */
  orphaned: boolean;
}

export interface Preview {
  next_runs: number[];
  chats_per_day: number | null;
  rendered: string | null;
  untrusted: boolean;
  warnings: string[];
  errors: { offset: number; message: string }[];
  event_paths: string[];
}

export interface RoutineBody {
  workspace_id?: string;
  name: string;
  enabled: boolean;
  spec: RoutineSpec;
  triggers: TriggerInput[];
}

type Saved = { routine: Routine; secrets: Record<string, string> };
const one = (id: string) => `/api/routines/${encodeURIComponent(id)}`;

export const routinesApi = {
  list: (workspace: string | null) =>
    call<RoutineList>("GET", workspace ? `/api/routines?workspace=${encodeURIComponent(workspace)}` : "/api/routines"),
  get: (id: string) => call<Routine>("GET", one(id)),
  create: (b: RoutineBody) => call<Saved>("POST", "/api/routines", b),
  update: (id: string, b: Partial<Omit<RoutineBody, "workspace_id">>) => call<Saved>("PATCH", one(id), b),
  remove: (id: string) => call<{ ok: boolean }>("DELETE", one(id)),
  run: (id: string) => call<{ run_id?: string; skipped?: string }>("POST", `${one(id)}/run`, {}),
  runs: (id: string, before?: string | null) =>
    call<{ runs: Run[]; next: string | null }>("GET", `${one(id)}/runs?limit=50${before ? `&before=${encodeURIComponent(before)}` : ""}`),
  run_detail: (id: string, runId: string) => call<RunDetail>("GET", `${one(id)}/runs/${encodeURIComponent(runId)}`),
  preview: (
    b: { workspace_id?: string; routine_id?: string; prompt?: string; triggers?: TriggerInput[]; sample_event?: unknown },
    signal?: AbortSignal,
  ) => call<Preview>("POST", "/api/routines/preview", b, signal),
};

/** What Run now answered, in a few words. */
export function ranNow(res: { run_id?: string; skipped?: string }): string {
  if (res.skipped === "overlap") return "Skipped: a run is still going.";
  if (res.skipped) return "Skipped: a limit was reached.";
  return "Started. Its chat appears in Recent.";
}

// ── The schedule builder's shapes ──

/** One schedule as the builder edits it. `time` is "HH:MM"; days 0-6 from Sunday. */
export type Shape =
  | { kind: "daily"; time: string }
  | { kind: "weekdays"; time: string }
  | { kind: "weekly"; days: number[]; time: string }
  | { kind: "monthly"; day: number; time: string }
  | { kind: "every"; n: number; unit: "minutes" | "hours" }
  | { kind: "once"; at: string }
  | { kind: "custom"; expr: string };

export const SHAPES: [Shape["kind"], string][] = [
  ["daily", "Every day"],
  ["weekdays", "Weekdays"],
  ["weekly", "Weekly on days"],
  ["monthly", "Monthly"],
  ["every", "Every N minutes or hours"],
  ["once", "Once"],
  ["custom", "Custom (cron)"],
];

export const DAY_NAMES = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

const pad = (n: number) => String(n).padStart(2, "0");
const hm = (time: string): [number, number] => {
  const [h, m] = time.split(":").map(Number);
  return [h ?? 0, m ?? 0];
};

/** A builder shape as a trigger config; `anchor` keeps an interval in step
 * when only its timezone or another schedule changed. */
export function toConfig(s: Shape, tz: string, anchor: number): ScheduleConfig {
  const at = (rest: string, time: string) => {
    const [h, m] = hm(time);
    return { type: "cron" as const, expr: `${m} ${h} ${rest}`, tz };
  };
  switch (s.kind) {
    case "daily":
      return at("* * *", s.time);
    case "weekdays":
      return at("* * 1-5", s.time);
    case "weekly":
      return at(`* * ${[...new Set(s.days)].sort((a, b) => a - b).join(",")}`, s.time);
    case "monthly":
      return at(`${s.day} * *`, s.time);
    case "every":
      return { type: "interval", every_minutes: s.unit === "hours" ? s.n * 60 : s.n, anchor_ms: anchor };
    case "once":
      return { type: "once", at: s.at, tz };
    case "custom":
      return { type: "cron", expr: s.expr.trim().replace(/\s+/g, " "), tz };
  }
}

const CRON = /^(\d{1,2}) (\d{1,2}) (\*|\d{1,2}) \* (\*|1-5|[0-6](?:,[0-6])*)$/;

/** A saved config back as a shape: only what `toConfig` writes, exactly;
 * anything else is Custom, so nothing is ever rewritten behind the user. */
export function fromConfig(c: ScheduleConfig): Shape {
  if (c.type === "interval") {
    const hours = c.every_minutes % 60 === 0;
    return { kind: "every", n: hours ? c.every_minutes / 60 : c.every_minutes, unit: hours ? "hours" : "minutes" };
  }
  if (c.type === "once") return { kind: "once", at: c.at };
  const custom: Shape = { kind: "custom", expr: c.expr };
  const m = CRON.exec(c.expr);
  if (!m) return custom;
  const [, min, hour, dom, dow] = m as unknown as [string, string, string, string, string];
  if (+min > 59 || +hour > 23) return custom;
  const time = `${pad(+hour)}:${pad(+min)}`;
  const shape: Shape =
    dom !== "*"
      ? dow === "*"
        ? { kind: "monthly", day: +dom, time }
        : custom
      : dow === "*"
        ? { kind: "daily", time }
        : dow === "1-5"
          ? { kind: "weekdays", time }
          : { kind: "weekly", days: dow.split(",").map(Number), time };
  if (shape.kind === "custom") return shape;
  // Read back only when writing it again gives the same text.
  const again = toConfig(shape, c.tz, 0);
  return again.type === "cron" && again.expr === c.expr ? shape : custom;
}

/** A schedule in a few words: "Weekdays at 08:30", "Every 2 hours". */
export function describeShape(s: Shape): string {
  switch (s.kind) {
    case "daily":
      return `Every day at ${s.time}`;
    case "weekdays":
      return `Weekdays at ${s.time}`;
    case "weekly":
      return `${s.days.map((d) => DAY_NAMES[d]).join(", ")} at ${s.time}`;
    case "monthly":
      return `Monthly on day ${s.day} at ${s.time}`;
    case "every":
      return s.n === 1 ? `Every ${s.unit.slice(0, -1)}` : `Every ${s.n} ${s.unit}`;
    case "once":
      return `Once on ${s.at.replace("T", " ")}`;
    case "custom":
      return `Cron ${s.expr}`;
  }
}

/** The list row's trigger line; a timezone shows when it is not the viewer's. */
export function triggerSummary(triggers: RoutineTrigger[], zone: string): string {
  if (triggers.length === 0) return "Run now only";
  if (triggers[0]!.kind === "webhook") return "When its webhook is called";
  const [first, ...rest] = triggers;
  const c = first!.config as ScheduleConfig;
  const tz = "tz" in c && c.tz !== zone ? ` (${c.tz})` : "";
  return `${describeShape(fromConfig(c))}${tz}${rest.length ? ` +${rest.length} more` : ""}`;
}

/** The zone this browser is in (a new schedule's default without prefs.timezone). */
export function browserZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
}

/** Every zone the browser knows, for the schedule's picker. */
export function zones(current: string): string[] {
  let all: string[] = [];
  try {
    all = (Intl as unknown as { supportedValuesOf?: (k: string) => string[] }).supportedValuesOf?.("timeZone") ?? [];
  } catch {
    /* an older browser */
  }
  return all.includes(current) ? all : [current, ...all];
}

// ── Runs ──

const STATUS: Record<string, string> = {
  pending: "Starting",
  running: "Running",
  waiting: "Waiting for you",
  completed: "Finished",
  error: "Failed",
  failed: "Failed",
  cancelled: "Cancelled",
  interrupted: "Interrupted",
  timed_out: "Timed out",
  skipped: "Skipped",
  blocked: "Blocked",
  missed: "Missed",
};

/** A run's status as people read it; a coalesced row says how many: "Skipped ×3". */
export function statusLabel(r: Pick<Run, "status" | "repeats">): string {
  const label = STATUS[r.status] ?? r.status;
  return r.repeats > 1 ? `${label} ×${r.repeats}` : label;
}

/** Statuses that did not go well; the list and the Runs tab mark them. */
export const isBad = (status: string) => ["error", "failed", "timed_out", "blocked", "interrupted"].includes(status);

export const TRIGGER_LABEL: Record<Run["trigger_kind"], string> = {
  schedule: "Schedule",
  webhook: "Webhook",
  manual: "Run now",
};

/** "45s", "3m 20s", "1h 5m"; null while it runs. */
export function duration(r: Pick<Run, "started_ms" | "ended_ms">): string | null {
  if (r.ended_ms === null) return null;
  const s = Math.max(0, Math.round((r.ended_ms - r.started_ms) / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** Input + output tokens, short: "850", "12.4k". */
export function tokens(u: Run["usage"]): string {
  if (!u) return "—";
  const n = u.input_tokens + u.output_tokens;
  return n < 1000 ? String(n) : `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
}

/** "08:30" in the zone, for "scheduled 08:30". */
export function clock(ms: number, zone: string): string {
  try {
    return new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", timeZone: zone }).format(ms);
  } catch {
    return new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit" }).format(ms);
  }
}

/** A next run in full: "Tue 10 Nov, 08:30". */
export function fullTime(ms: number, zone: string): string {
  const opts: Intl.DateTimeFormatOptions = { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" };
  try {
    return new Intl.DateTimeFormat("en-GB", { ...opts, timeZone: zone }).format(ms);
  } catch {
    return new Intl.DateTimeFormat("en-GB", opts).format(ms);
  }
}

// ── A new routine ──

export interface Draft {
  name: string;
  enabled: boolean;
  spec: RoutineSpec;
  /** "schedule": 1-8 schedules; "webhook": exactly one webhook. */
  when: "schedule" | "webhook";
  schedules: { trigger_id?: string; shape: Shape; tz: string; anchor: number }[];
  /** The webhook trigger's id once saved, so its URL survives an edit. */
  webhookId?: string;
}

export function blankSpec(agentId: string): RoutineSpec {
  return {
    profile: "chat",
    agent_id: agentId,
    model: null,
    effort: null,
    prompt: "",
    project_id: null,
    catch_up: "skip",
    on_missing: "empty",
    notify: "failures",
    max_runtime_min: 120,
  };
}

export function newDraft(agentId: string, tz: string, now: number): Draft {
  return {
    name: "",
    enabled: true,
    spec: blankSpec(agentId),
    when: "schedule",
    schedules: [{ shape: { kind: "weekdays", time: "08:30" }, tz, anchor: now }],
  };
}

/** A saved routine as the editor's draft. */
export function draftOf(r: Routine, now: number): Draft {
  const hook = r.triggers.find((t) => t.kind === "webhook");
  return {
    name: r.name,
    enabled: r.enabled,
    spec: r.spec,
    when: hook ? "webhook" : "schedule",
    webhookId: hook?.trigger_id,
    schedules: r.triggers
      .filter((t) => t.kind === "schedule")
      .map((t) => {
        const c = t.config as ScheduleConfig;
        return {
          trigger_id: t.trigger_id,
          shape: fromConfig(c),
          tz: "tz" in c ? c.tz : browserZone(),
          anchor: c.type === "interval" ? c.anchor_ms : now,
        };
      }),
  };
}

/** The triggers a draft saves: its schedules, or its one webhook. */
export function draftTriggers(d: Draft): TriggerInput[] {
  if (d.when === "webhook") return [{ ...(d.webhookId ? { trigger_id: d.webhookId } : {}), kind: "webhook", config: {} }];
  return d.schedules.map((s) => ({
    ...(s.trigger_id ? { trigger_id: s.trigger_id } : {}),
    kind: "schedule" as const,
    config: toConfig(s.shape, s.tz, s.anchor),
  }));
}

/** The spec a draft saves: chat routines carry no node, folder or approval mode. */
export function draftSpec(d: Draft): RoutineSpec {
  if (d.spec.profile === "agentic") return { ...d.spec, approval_mode: d.spec.approval_mode ?? "ask" };
  const { node_id: _n, cwd: _c, approval_mode: _a, ...rest } = d.spec;
  return rest;
}

/** The empty page's examples: each opens the editor filled in. */
export const TEMPLATES: { name: string; about: string; draft: (agentId: string, tz: string, now: number) => Draft }[] = [
  {
    name: "Morning briefing",
    about: "Weekdays at 08:00: a short plan for the day.",
    draft: (a, tz, now) => ({
      ...newDraft(a, tz, now),
      name: "Morning briefing",
      spec: { ...blankSpec(a), prompt: "It is {{now.weekday}}, {{now.date}}. Give me a short, practical plan for today." },
      schedules: [{ shape: { kind: "weekdays", time: "08:00" }, tz, anchor: now }],
    }),
  },
  {
    name: "Weekly review",
    about: "Fridays at 16:00: what went well and what to carry over.",
    draft: (a, tz, now) => ({
      ...newDraft(a, tz, now),
      name: "Weekly review",
      spec: {
        ...blankSpec(a),
        prompt: "It is the end of the week ({{now.date}}). Ask me three questions that help me review it, then suggest priorities for next week.",
      },
      schedules: [{ shape: { kind: "weekly", days: [5], time: "16:00" }, tz, anchor: now }],
    }),
  },
  {
    name: "Monthly check-in",
    about: "On the 1st at 09:00: a reminder with last month's routine status.",
    draft: (a, tz, now) => ({
      ...newDraft(a, tz, now),
      name: "Monthly check-in",
      spec: {
        ...blankSpec(a),
        prompt: "A new month starts today ({{now.date}}). The last run of this routine was {{last_run.status}}. List what usually needs doing at the start of a month.",
      },
      schedules: [{ shape: { kind: "monthly", day: 1, time: "09:00" }, tz, anchor: now }],
    }),
  },
];
