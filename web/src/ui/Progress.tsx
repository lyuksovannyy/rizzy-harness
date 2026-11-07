// A `ui:progress` block (rich_plan §6.3): the turn's latest block with an id
// as a card (title, bar, steps), earlier ones as one-line snapshots. All
// text is the model's, rendered as React text; the status words, the
// stopped state and the turn's end are the harness's.
import { Circle, CircleCheck, CircleMinus, CircleStop, CircleX, LoaderCircle, type LucideIcon } from "lucide-react";
import { rt } from "../md/strings";
import type { ProgressSpec, StepStatus } from "../types";
import { currentStep, finishedSteps, progressStatus, progressValue, type ProgressSlot, type ProgressStatus } from "./blocks";

/** A step as shown: an active step of an ended turn has stopped. */
type Shown = StepStatus | "stopped";

const STEP_ICON: Record<Shown, LucideIcon> = {
  pending: Circle,
  active: LoaderCircle,
  done: CircleCheck,
  failed: CircleX,
  skipped: CircleMinus,
  stopped: CircleStop,
};

const STATUS_ICON: Record<ProgressStatus, LucideIcon> = {
  running: LoaderCircle,
  done: CircleCheck,
  failed: CircleX,
  stopped: CircleStop,
};

/** An icon that names its status for screen readers (the label is text). */
function StatusIcon({ icon: Icon, label, spin }: { icon: LucideIcon; label: string; spin?: boolean }) {
  return (
    <span className="ui-icon" role="img" aria-label={label} title={label}>
      <Icon size={16} aria-hidden="true" className={spin ? "spin" : undefined} />
    </span>
  );
}

const NOT_ENDED: ProgressSlot = { mode: "full", ended: false };

export default function Progress({ spec, slot = NOT_ENDED }: { spec: ProgressSpec; slot?: ProgressSlot }) {
  return slot.mode === "snapshot" ? <Snapshot spec={spec} /> : <Card spec={spec} slot={slot} />;
}

function Card({ spec, slot }: { spec: ProgressSpec; slot: ProgressSlot }) {
  const status = progressStatus(spec, slot.ended);
  const pct = Math.round(progressValue(spec) * 100);
  // How the turn ended matters only when it did not simply finish.
  const why = slot.ended && slot.reason && slot.reason !== "completed" ? rt(`ui.turn.${slot.reason}`) : null;
  return (
    <div className={`ui-progress ui-progress-${status}`}>
      <div className="ui-progress-head">
        <p className="ui-title">{spec.title}</p>
        <span className="ui-progress-status">
          {rt(`ui.status.${status}`)}
          {why && ` · ${why}`}
        </span>
      </div>
      <div
        className="ui-bar"
        role="progressbar"
        aria-label={spec.title}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
      >
        <div className="ui-bar-fill" style={{ width: `${pct}%` }} />
      </div>
      <ol className="ui-steps">
        {spec.steps.map((s, i) => {
          const shown: Shown = s.status === "active" && slot.ended ? "stopped" : s.status;
          return (
            <li key={i} className={`ui-step ui-step-${shown}`}>
              <StatusIcon icon={STEP_ICON[shown]} label={rt(`ui.step.${shown}`)} spin={shown === "active"} />
              <span className="ui-step-text">
                <span className="ui-step-label">{s.label}</span>
                {s.detail && <span className="ui-step-detail">{s.detail}</span>}
              </span>
            </li>
          );
        })}
      </ol>
    </div>
  );
}

/** Where the job stood at an earlier message: `<icon> title · 2/4 · step`.
 * A past state, so nothing spins. */
function Snapshot({ spec }: { spec: ProgressSpec }) {
  const status = progressStatus(spec, false);
  const step = currentStep(spec);
  return (
    <div className={`ui-progress-snapshot ui-progress-${status}`}>
      <StatusIcon icon={STATUS_ICON[status]} label={rt(`ui.status.${status}`)} />
      <span className="ui-title">{spec.title}</span>
      <span className="muted">
        {` · ${finishedSteps(spec)}/${spec.steps.length}`}
        {step && ` · ${step}`}
      </span>
    </div>
  );
}
