// A routine's schedules (docs/routines.md): one to eight, each a shape (every
// day, weekdays, weekly on days, monthly, every N minutes or hours, once, or
// a cron line) with its own timezone. The next runs and the chats-per-day
// note come from the server's preview, so they read exactly as it will fire.
import { Plus, X } from "lucide-react";
import { DAY_NAMES, type Draft, fullTime, type Shape, SHAPES, zones } from "../lib/routines";

type Schedule = Draft["schedules"][number];

/** The days in the order a week reads, Monday first. */
const WEEK = [1, 2, 3, 4, 5, 6, 0];

/** "HH:MM" a shape starts from when the user picks another kind. */
const timeOf = (s: Shape) => ("time" in s ? s.time : "09:00");

/** Tomorrow at 09:00 here, as a datetime-local value. */
function tomorrowAt9(): string {
  const d = new Date(Date.now() + 86_400_000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T09:00`;
}

/** The shape of `kind`, keeping what carries over (the time). */
export function switchShape(from: Shape, kind: Shape["kind"]): Shape {
  const time = timeOf(from);
  switch (kind) {
    case "daily":
    case "weekdays":
      return { kind, time };
    case "weekly":
      return { kind, days: from.kind === "weekly" ? from.days : [1], time };
    case "monthly":
      return { kind, day: 1, time };
    case "every":
      return { kind, n: 30, unit: "minutes" };
    case "once":
      return { kind, at: tomorrowAt9() };
    case "custom": {
      const [h, m] = time.split(":").map(Number);
      return { kind, expr: `${m ?? 0} ${h ?? 9} * * *` };
    }
  }
}

export function ScheduleBuilder(props: {
  schedules: Schedule[];
  onChange: (s: Schedule[]) => void;
  max: number;
  /** The viewer's zone: a new schedule's. */
  zone: string;
  /** From the preview, across every schedule. */
  nextRuns: number[];
  chats: string | null;
}) {
  const { schedules } = props;
  const set = (i: number, s: Partial<Schedule>) => props.onChange(schedules.map((x, j) => (j === i ? { ...x, ...s } : x)));
  return (
    <div className="stack schedule-builder">
      {schedules.map((s, i) => (
        <div key={s.trigger_id ?? `new-${i}`} className="schedule-row" role="group" aria-label={`Schedule ${i + 1}`}>
          <div className="row">
            <select aria-label="Repeats" value={s.shape.kind} onChange={(e) => set(i, { shape: switchShape(s.shape, e.target.value as Shape["kind"]) })}>
              {SHAPES.map(([k, label]) => (
                <option key={k} value={k}>
                  {label}
                </option>
              ))}
            </select>
            <ShapeFields shape={s.shape} onChange={(shape) => set(i, { shape })} />
            {schedules.length > 1 && (
              <button
                type="button"
                className="icon"
                aria-label={`Remove schedule ${i + 1}`}
                title="Remove"
                onClick={() => props.onChange(schedules.filter((_, j) => j !== i))}
              >
                <X size={16} />
              </button>
            )}
          </div>
          {s.shape.kind !== "every" && (
            <label className="small schedule-tz">
              Timezone
              <select value={s.tz} onChange={(e) => set(i, { tz: e.target.value })}>
                {zones(s.tz).map((z) => (
                  <option key={z} value={z}>
                    {z}
                  </option>
                ))}
              </select>
            </label>
          )}
        </div>
      ))}
      {schedules.length < props.max && (
        <button
          type="button"
          className="link small row"
          onClick={() =>
            props.onChange([...schedules, { shape: { kind: "daily", time: "09:00" }, tz: props.zone, anchor: Date.now() }])
          }
        >
          <Plus size={14} aria-hidden="true" /> Add a schedule
        </button>
      )}
      {props.nextRuns.length > 0 && (
        <div className="small muted schedule-next">
          Next runs:{" "}
          {props.nextRuns.slice(0, 5).map((ms, i) => (
            <span key={ms}>
              {i > 0 && " · "}
              {fullTime(ms, props.zone)}
            </span>
          ))}
        </div>
      )}
      {props.chats && <div className="small schedule-chats">{props.chats}</div>}
    </div>
  );
}

function ShapeFields({ shape, onChange }: { shape: Shape; onChange: (s: Shape) => void }) {
  const time =
    "time" in shape ? (
      <input
        type="time"
        aria-label="At"
        className="schedule-time"
        value={shape.time}
        onChange={(e) => e.target.value && onChange({ ...shape, time: e.target.value })}
      />
    ) : null;
  switch (shape.kind) {
    case "daily":
    case "weekdays":
      return <>at {time}</>;
    case "weekly":
      return (
        <>
          <span className="schedule-days" role="group" aria-label="Days">
            {WEEK.map((d) => {
              const on = shape.days.includes(d);
              return (
                <button
                  key={d}
                  type="button"
                  aria-pressed={on}
                  className={on ? "on" : ""}
                  // The last day stays: a weekly schedule needs one.
                  onClick={() => (on ? shape.days.length > 1 && onChange({ ...shape, days: shape.days.filter((x) => x !== d) }) : onChange({ ...shape, days: [...shape.days, d] }))}
                >
                  {DAY_NAMES[d]}
                </button>
              );
            })}
          </span>
          at {time}
        </>
      );
    case "monthly":
      return (
        <>
          on day
          <input
            type="number"
            aria-label="Day of the month"
            className="schedule-num"
            min={1}
            max={31}
            value={shape.day}
            onChange={(e) => onChange({ ...shape, day: Math.min(31, Math.max(1, Number(e.target.value) || 1)) })}
          />
          at {time}
        </>
      );
    case "every":
      return (
        <>
          <input
            type="number"
            aria-label="Every"
            className="schedule-num"
            min={1}
            value={shape.n}
            onChange={(e) => onChange({ ...shape, n: Math.max(1, Number(e.target.value) || 1) })}
          />
          <select aria-label="Unit" value={shape.unit} onChange={(e) => onChange({ ...shape, unit: e.target.value as "minutes" | "hours" })}>
            <option value="minutes">minutes</option>
            <option value="hours">hours</option>
          </select>
        </>
      );
    case "once":
      return (
        <input type="datetime-local" aria-label="On" value={shape.at} onChange={(e) => e.target.value && onChange({ ...shape, at: e.target.value })} />
      );
    case "custom":
      return (
        <input
          className="mono schedule-cron"
          aria-label="Cron"
          value={shape.expr}
          onChange={(e) => onChange({ ...shape, expr: e.target.value })}
          placeholder="minute hour day month weekday"
        />
      );
  }
}
