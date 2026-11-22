// The chat's execution mode in the composer's strip (N.10b, README
// "Execution modes"): "Commands: sandbox | host shell". A label where there
// is no choice (the node offers one mode, or the person's level allows one),
// a picker where the owner may switch; disabled, with its reason, while
// anything of the chat runs (the node refuses then too). Choosing the host
// shell opens the warning first; cancelling leaves the mode; going back to
// the sandbox needs none. The chat shows the mode the log says
// (`exec_mode_changed`), never the pick alone.
import { lazy, Suspense, useState } from "react";
import { MODE_LABEL } from "../lib/execWords";
import type { ExecMode } from "../types";
import { Select } from "./Select";

const HostWarning = lazy(() => import("./HostWarning"));

export function ExecSwitch(props: {
  /** The chat's mode now (its log); none when the log names one this page does not read. */
  value?: ExecMode;
  /** What the node offers. */
  offered: ExecMode[];
  /** The person's level allows `host` (the master decides again). */
  mayHost: boolean;
  node: string;
  /** The chat's approval mode: the warning says what Auto means there. */
  approval: string;
  /** Why it cannot be switched now, or null. */
  busy: string | null;
  onChange: (m: ExecMode) => void;
  /** A click or Enter on the control while it is disabled: the strip says why, in words. */
  onBlocked?: () => void;
}) {
  const [asking, setAsking] = useState(false);
  // A mode nobody reads: nothing runs in the chat, and no picker offers a
  // way out (a switch would not repair a log the node cannot read).
  if (!props.value)
    return (
      <span
        className="exec-only pill"
        aria-label="Commands: mode unknown"
        title="This chat's execution mode is not known here (its log names none this page reads), so it runs nothing: start a new chat"
      >
        mode unknown
      </span>
    );
  const other: ExecMode = props.value === "host" ? "sandbox" : "host";
  const reach = props.offered.includes(other) && (other === "sandbox" || props.mayHost);
  if (!reach)
    return (
      <span
        className="exec-only pill"
        aria-label={`Commands: ${MODE_LABEL[props.value]}`}
        title={
          props.offered.includes(other)
            ? "Only a person with the host level may switch this chat to the host shell"
            : props.value === "host"
              ? "This node offers only the host shell"
              : "This node offers only the sandbox: the host shell is off here ([execution] host = true in its node.toml turns it on)"
        }
      >
        {MODE_LABEL[props.value]}
      </span>
    );
  return (
    <span className="exec-switch" title={props.busy ?? undefined}>
      <Select
        compact
        stacked
        label="Commands"
        disabled={!!props.busy}
        onBlocked={props.onBlocked}
        value={props.value}
        options={(["sandbox", "host"] as ExecMode[]).map((m) => ({
          value: m,
          label: MODE_LABEL[m],
          hint:
            props.busy ??
            (m === "host"
              ? "No sandbox: commands run as the node's user"
              : "Commands run inside the sandbox"),
          disabled: !props.offered.includes(m) || (m === "host" && !props.mayHost),
        }))}
        onChange={(v) => {
          if (v === props.value) return;
          if (v === "host") setAsking(true);
          else props.onChange("sandbox");
        }}
      />
      {props.busy && (
        <span className="sr-only" role="note">
          {props.busy}
        </span>
      )}
      {asking && (
        <Suspense fallback={null}>
          <HostWarning
            node={props.node}
            switching={{ approval: props.approval }}
            onConfirm={() => {
              setAsking(false);
              props.onChange("host");
            }}
            onCancel={() => setAsking(false)}
          />
        </Suspense>
      )}
    </span>
  );
}

export default ExecSwitch;
