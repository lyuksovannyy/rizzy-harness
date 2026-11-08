// The Routines page's own logic: the schedule builder writes cron or an
// interval and reads back only what it writes (anything else is Custom),
// the list's trigger line, the Runs tab's status, duration and tokens, and
// what a draft saves; the webhook card's sample box, localhost warning and
// curl line.
import { describe, expect, test } from "vitest";
import {
  curlExample,
  describeShape,
  draftOf,
  draftSpec,
  draftTriggers,
  duration,
  fromConfig,
  localOnly,
  newDraft,
  parseSample,
  type Routine,
  type RoutineTrigger,
  type Shape,
  statusLabel,
  toConfig,
  tokens,
  triggerSummary,
} from "./routines";

const TZ = "Europe/Riga";

describe("schedule builder", () => {
  test.each<[Shape, string]>([
    [{ kind: "daily", time: "08:30" }, "30 8 * * *"],
    [{ kind: "weekdays", time: "07:05" }, "5 7 * * 1-5"],
    [{ kind: "weekly", days: [5, 1, 3], time: "18:00" }, "0 18 * * 1,3,5"],
    [{ kind: "weekly", days: [0], time: "00:00" }, "0 0 * * 0"],
    [{ kind: "monthly", day: 15, time: "09:45" }, "45 9 15 * *"],
  ])("%j ↔ %s", (shape, expr) => {
    const c = toConfig(shape, TZ, 0);
    expect(c).toEqual({ type: "cron", expr, tz: TZ });
    const back = fromConfig(c);
    // Days come back sorted; everything else exactly.
    expect(back).toEqual(shape.kind === "weekly" ? { ...shape, days: [...shape.days].sort() } : shape);
    expect(toConfig(back, TZ, 0)).toEqual(c);
  });

  test("intervals keep their anchor and read back in hours when they can", () => {
    expect(toConfig({ kind: "every", n: 2, unit: "hours" }, TZ, 1234)).toEqual({ type: "interval", every_minutes: 120, anchor_ms: 1234 });
    expect(fromConfig({ type: "interval", every_minutes: 120, anchor_ms: 1 })).toEqual({ kind: "every", n: 2, unit: "hours" });
    expect(fromConfig({ type: "interval", every_minutes: 90, anchor_ms: 1 })).toEqual({ kind: "every", n: 90, unit: "minutes" });
  });

  test("once is a local time with its zone", () => {
    const c = toConfig({ kind: "once", at: "2026-11-10T09:00" }, TZ, 0);
    expect(c).toEqual({ type: "once", at: "2026-11-10T09:00", tz: TZ });
    expect(fromConfig(c)).toEqual({ kind: "once", at: "2026-11-10T09:00" });
  });

  test("anything the builder would not write is Custom, kept as typed", () => {
    for (const expr of ["*/15 * * * *", "30 8 * * 5,1", "08 8 * * *", "30 8 1 * 1", "30 25 * * *", "0 9 * 1 *", "30 8 * * MON"]) {
      expect(fromConfig({ type: "cron", expr, tz: TZ })).toEqual({ kind: "custom", expr });
    }
    expect(toConfig({ kind: "custom", expr: "  0  9 * * *  " }, TZ, 0)).toEqual({ type: "cron", expr: "0 9 * * *", tz: TZ });
  });

  test("described in a few words", () => {
    expect(describeShape({ kind: "weekdays", time: "08:30" })).toBe("Weekdays at 08:30");
    expect(describeShape({ kind: "weekly", days: [1, 3], time: "09:00" })).toBe("Mon, Wed at 09:00");
    expect(describeShape({ kind: "every", n: 1, unit: "hours" })).toBe("Every hour");
    expect(describeShape({ kind: "every", n: 15, unit: "minutes" })).toBe("Every 15 minutes");
    expect(describeShape({ kind: "monthly", day: 1, time: "09:00" })).toBe("Monthly on day 1 at 09:00");
  });
});

const trigger = (config: RoutineTrigger["config"], kind: RoutineTrigger["kind"] = "schedule"): RoutineTrigger => ({
  trigger_id: `t${Math.random()}`,
  kind,
  config,
  next_run_ms: null,
  last_fired_ms: null,
});

describe("trigger line", () => {
  test("the zone shows only when it is not the viewer's; more schedules are counted", () => {
    const riga = trigger({ type: "cron", expr: "30 8 * * 1-5", tz: TZ });
    expect(triggerSummary([riga], TZ)).toBe("Weekdays at 08:30");
    expect(triggerSummary([riga], "UTC")).toBe("Weekdays at 08:30 (Europe/Riga)");
    expect(triggerSummary([riga, trigger({ type: "interval", every_minutes: 30, anchor_ms: 0 })], TZ)).toBe("Weekdays at 08:30 +1 more");
    expect(triggerSummary([trigger({}, "webhook")], TZ)).toBe("When its webhook is called");
  });
});

describe("runs", () => {
  test("coalesced rows say how many; statuses read as words", () => {
    expect(statusLabel({ status: "skipped", repeats: 3 })).toBe("Skipped ×3");
    expect(statusLabel({ status: "missed", repeats: 2 })).toBe("Missed ×2");
    expect(statusLabel({ status: "completed", repeats: 1 })).toBe("Finished");
    expect(statusLabel({ status: "waiting", repeats: 1 })).toBe("Waiting for you");
    expect(statusLabel({ status: "timed_out", repeats: 1 })).toBe("Timed out");
  });

  test("duration and tokens", () => {
    expect(duration({ started_ms: 0, ended_ms: null })).toBeNull();
    expect(duration({ started_ms: 0, ended_ms: 45_000 })).toBe("45s");
    expect(duration({ started_ms: 0, ended_ms: 200_000 })).toBe("3m 20s");
    expect(duration({ started_ms: 0, ended_ms: 3_900_000 })).toBe("1h 5m");
    expect(tokens(null)).toBe("—");
    expect(tokens({ input_tokens: 800, output_tokens: 50 })).toBe("850");
    expect(tokens({ input_tokens: 12_000, output_tokens: 400 })).toBe("12k");
    expect(tokens({ input_tokens: 1_000, output_tokens: 234 })).toBe("1.2k");
  });
});

describe("drafts", () => {
  test("a chat routine saves no node, folder or approval mode; an agentic one asks by default", () => {
    const d = newDraft("general", TZ, 5);
    d.spec = { ...d.spec, node_id: "n1", cwd: "/x", approval_mode: "ask_all" };
    expect(draftSpec(d)).not.toHaveProperty("node_id");
    expect(draftSpec(d)).not.toHaveProperty("approval_mode");
    const a = { ...d, spec: { ...d.spec, profile: "agentic" as const, approval_mode: null } };
    expect(draftSpec(a)).toMatchObject({ profile: "agentic", node_id: "n1", cwd: "/x", approval_mode: "ask" });
  });

  test("saved triggers keep their ids, so a webhook's URL survives an edit", () => {
    const routine = {
      triggers: [
        { ...trigger({ type: "cron", expr: "0 9 * * *", tz: TZ }), trigger_id: "keep" },
        { ...trigger({ type: "interval", every_minutes: 60, anchor_ms: 42 }), trigger_id: "int" },
      ],
      name: "x",
      enabled: true,
      spec: newDraft("general", TZ, 0).spec,
      event_paths: [],
    } as unknown as Routine;
    const d = draftOf(routine, 99);
    expect(draftTriggers(d)).toEqual([
      { trigger_id: "keep", kind: "schedule", config: { type: "cron", expr: "0 9 * * *", tz: TZ } },
      { trigger_id: "int", kind: "schedule", config: { type: "interval", every_minutes: 60, anchor_ms: 42 } },
    ]);
    const hook = draftOf({ ...routine, triggers: [{ ...trigger({}, "webhook"), trigger_id: "hook" }] }, 0);
    expect(hook.when).toBe("webhook");
    expect(draftTriggers(hook)).toEqual([{ trigger_id: "hook", kind: "webhook", config: {} }]);
    // A new webhook routine sends one webhook and no schedules.
    expect(draftTriggers({ ...newDraft("general", TZ, 0), when: "webhook" })).toEqual([{ kind: "webhook", config: {} }]);
  });
});

describe("webhooks", () => {
  test("the sample box: empty, JSON, or not yet", () => {
    expect(parseSample("  ")).toBeNull();
    expect(parseSample('{"issue": {"number": 42}}')).toEqual({ value: { issue: { number: 42 } } });
    expect(parseSample("{oops")).toEqual({ error: "Not valid JSON yet." });
  });

  test("hosts only this machine can reach", () => {
    for (const h of ["localhost", "app.localhost", "127.0.0.1", "127.1.2.3", "[::1]", "::1"]) expect(localOnly(h)).toBe(true);
    for (const h of ["harness.example.com", "10.0.0.5", "192.168.1.2"]) expect(localOnly(h)).toBe(false);
  });

  test("the curl line: the secret when shown, else a variable; quotes survive", () => {
    const url = "https://h.example/api/hooks/abc";
    expect(curlExample(url, "s3cret", { text: "it's on" })).toBe(
      [
        "curl -X POST 'https://h.example/api/hooks/abc'",
        "-H 'Authorization: Bearer s3cret'",
        "-H 'Content-Type: application/json'",
        `-d '{"text":"it'\\''s on"}'`,
      ].join(" \\\n  "),
    );
    expect(curlExample(url, null, undefined)).toContain(`-H "Authorization: Bearer $HOOK_SECRET"`);
    expect(curlExample(url, null, undefined)).toContain(`-d '{"text":"hello"}'`);
  });
});
