// Plan mode (ADOPTION_PLAN X.7, README "Approval modes"): a bar above the
// composer while the chat is in plan mode and no turn runs, saying so, with
// the choices to carry the plan out; the message a choice sends is in the
// bar's visible text (no tooltip-only words). Each choice is the person's own act: it changes the
// chat's mode through the same route as the picker (the node clamps and
// records it), then sends "Carry out the plan." as their message. Nothing
// the agent writes reaches it. Its own lazy chunk: loaded on the first chat
// in plan mode.
import { ClipboardList } from "lucide-react";
import { useState } from "react";
import type { ApprovalMode } from "../types";
import { MODE_WORDS, whyNot } from "./ApprovalModePick";
import "./planBar.css";

/** The person's message a choice sends with the new mode. */
const CARRY_OUT = "Carry out the plan.";

const CHOICES: { mode: ApprovalMode; label: string }[] = [
  { mode: "auto", label: "Auto" },
  { mode: "accept_edits", label: "Accept edits" },
  { mode: "ask", label: "Ask" },
];

export default function PlanBar(props: {
  /** The node's ceiling: a choice above it is disabled with the reason. */
  max: ApprovalMode;
  /** Switch to `m`, then send `text` as the person's message. */
  onCarry: (m: ApprovalMode, text: string) => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  return (
    <div className="plan-bar" role="status">
      <ClipboardList size={14} aria-hidden />
      <span className="plan-bar-text">
        <b>Plan mode</b>: the agent looks and plans; nothing is changed. When the plan is right, switch and
        send &ldquo;{CARRY_OUT}&rdquo;:
      </span>
      {CHOICES.map((c) => {
        const why = whyNot(c.mode, props.max);
        return (
          <button
            key={c.mode}
            type="button"
            className="plan-bar-choice"
            disabled={busy || !!why}
            title={why ?? `Switch to ${MODE_WORDS[c.mode].label} and send "${CARRY_OUT}"`}
            onClick={() => {
              setBusy(true);
              setErr(null);
              props
                .onCarry(c.mode, CARRY_OUT)
                .catch((x) => setErr(x instanceof Error ? x.message : String(x)))
                .finally(() => setBusy(false));
            }}
          >
            {c.label}
          </button>
        );
      })}
      {err && <span className="error-text">{err}</span>}
    </div>
  );
}
